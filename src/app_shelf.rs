//! The island's side of the file shelf: what is on it, the cards drawn from
//! that, a dropped list, and a card pressed and pulled out. A child of `app`,
//! so it works on `App` directly. The list and the cards' data are
//! `shelf.rs`; the OLE drop target and `DoDragDrop` are `shelf_ole.rs`; the
//! reducer only knows the shelf is open and which card was clicked.
//!
//! The shelf lives in memory: a restart empties it. Nothing runs while it
//! is closed; whether a file is still there is looked at when the page
//! opens, after a drop and before a card is opened or dragged.
//!
//! Cards animate in, out and across (`ui::shelf_motion`): a drop, a removal
//! and "Kosongkan" snapshot the cards and their places before the list
//! changes and hand both to the motion state. It counts in `animating_at`,
//! so the frame clock runs only while something moves.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::SystemServices::MK_LBUTTON;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, KillTimer, SetTimer};

use super::{App, Event, Page, View, client_xy};
use crate::shelf::{self, Probe, Shelf, ShelfCard};
use crate::shortcuts;
use crate::ui::geometry::{Rect, ShapeKind, shape_for};
use crate::ui::panel_layout::shelf_layout;
use crate::ui::shelf_motion::ShelfMotion;

/// One-shot: the drag has left the island; when it fires, the drag's hold on
/// the panel ends (a drop kills it, the drop latches the panel itself).
pub(super) const TIMER_SHELF_HOLD: usize = 27;
/// A drag that leaves the island keeps the panel this long, so one that
/// strays off the edge and back does not flicker it away.
const SHELF_LEAVE_MS: u32 = 600;
/// A press on a card turns into a drag out after this many logical pixels.
const DRAG_START_DIP: f32 = 6.0;
/// Files dropped while the panel is shut wait this long before the cards
/// come in, so the panel's own opening (240 ms) is not what hides them.
const OPEN_DELAY_MS: u64 = 120;

#[derive(Default)]
pub(super) struct ShelfState {
    pub shelf: Shelf,
    /// What the page draws, from `shelf` and the file system as last looked.
    pub cards: Vec<ShelfCard>,
    /// The page showed at the last `sync_shelf`.
    shown: bool,
    /// Files are over the island right now (the drop zone lights).
    pub dragging: bool,
    /// A left press on card `i` at this client point: it may become a drag.
    press: Option<(usize, i32, i32)>,
    /// Cards coming in, going out or sliding; empty at rest.
    pub motion: ShelfMotion,
}

impl App {
    /// Looks at the files again (the page opened, the list changed). Every
    /// change of the shelf (a drop, a file from a phone, a card taken off,
    /// "Kosongkan") ends with this, so it is also where what the phones can
    /// take follows the shelf (`phone_sync_shared`).
    pub(super) fn shelf_look(&mut self) {
        self.shelf.cards = self.shelf.shelf.cards(shelf::probe);
        self.phone_sync_shared();
    }

    /// The OLE drop target saw files enter the island or leave it.
    pub(super) fn on_shelf_drag(&mut self, on: bool) -> Option<Event> {
        self.shelf.dragging = on;
        if on {
            // A drag that comes back cancels the pending close.
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_SHELF_HOLD);
            }
            Some(Event::ShelfDrag(true))
        } else {
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_SHELF_HOLD, SHELF_LEAVE_MS, None);
            }
            // The drop zone's outline goes now, not with the panel.
            if !self.animating() {
                self.draw(Instant::now());
            }
            None
        }
    }

    /// The Shelf page showed: look at the files once, then not again until
    /// it is shown again.
    pub(super) fn sync_shelf(&mut self) {
        let wanted = self.state.view() == View::Panel(Page::Shelf);
        if wanted && !self.shelf.shown {
            self.shelf_look();
        }
        self.shelf.shown = wanted;
        // Nothing animates while the page is not on screen (or with
        // Windows' animation effects off): the state is simply dropped.
        if (!wanted || !self.animations) && !self.shelf.motion.is_empty() {
            self.shelf.motion.clear();
        }
    }

    /// Where the page puts `n` cards and how many lines their names get:
    /// the panel's resting shape, with the strip as it is drawn (the same
    /// layout the renderer builds).
    fn shelf_rects(&self, n: usize) -> (Vec<Rect>, usize) {
        let shape = shape_for(self.config.style, ShapeKind::Panel);
        let strip = self.state.waiting_strip().is_some();
        let l = shelf_layout(shape, strip, n);
        (l.cards.iter().map(|c| c.card).collect(), l.lines)
    }

    /// The cards changed from `old` to what `self.shelf.cards` is now:
    /// records the motion (when the effects are on) and starts the frame
    /// clock if something moves. `delay` holds the start back.
    fn shelf_animate(&mut self, old: &[ShelfCard], now: Instant, delay: Duration) {
        let (old_rects, old_lines) = self.shelf_rects(old.len());
        let (new_rects, _) = self.shelf_rects(self.shelf.cards.len());
        let start = now + delay;
        self.shelf.motion.on_change(
            old,
            &old_rects,
            old_lines,
            &self.shelf.cards,
            &new_rects,
            start,
            self.animations,
        );
        if self.shelf.motion.running(now) {
            self.ensure_clock();
        }
    }

    /// Files dropped on the island: they go on the shelf and the panel stays
    /// on it, latched like a click (`ShelfDropped`), so the cards are seen
    /// arriving whatever the pointer does next.
    pub(super) fn on_shelf_drop(&mut self, paths: Vec<PathBuf>, now: Instant) {
        self.shelf.dragging = false;
        let old = self.shelf.cards.clone();
        self.shelf.shelf.add(paths);
        self.shelf_look();
        // The cards on a page that is already showing come in at once; a
        // panel that is still opening gets them once it has grown a little.
        let delay = if self.shelf.shown {
            Duration::ZERO
        } else {
            Duration::from_millis(OPEN_DELAY_MS)
        };
        self.shelf_animate(&old, now, delay);
        // The drag is over: a pending 600 ms leave must not end anything.
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SHELF_HOLD);
        }
        self.apply(Event::ShelfDropped, now);
        self.refresh(now);
    }

    /// Files (or the folder) that came from a phone: cards on the shelf that
    /// say so, shared back with the phones like the rest of the shelf. The
    /// panel is already open on the phone link, latched by `PhoneAttention`,
    /// so nothing is applied to the reducer; the cards come in through the
    /// shelf's own motion like dropped ones.
    pub(super) fn shelf_add_phone(&mut self, paths: Vec<PathBuf>, now: Instant) {
        let old = self.shelf.cards.clone();
        if self.shelf.shelf.add_from(paths, shelf::Origin::Phone) == 0 {
            return;
        }
        self.shelf_look();
        let delay = if self.shelf.shown {
            Duration::ZERO
        } else {
            Duration::from_millis(OPEN_DELAY_MS)
        };
        self.shelf_animate(&old, now, delay);
    }

    /// `TIMER_SHELF_HOLD`: the drag stayed away; its hold on the panel ends.
    pub(super) fn on_shelf_hold_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SHELF_HOLD);
        }
        self.apply(Event::ShelfDrag(false), Instant::now());
    }

    /// A card's file opens, after the panel has collapsed. A file that is
    /// gone is only marked: the card turns dim.
    pub(super) fn shelf_open(&mut self, i: usize) {
        let Some(path) = self.shelf.shelf.get(i).map(PathBuf::from) else {
            return;
        };
        if shelf::probe(&path) == Probe::Missing {
            self.shelf_look();
            return;
        }
        self.queue_shortcut(shortcuts::Action::OpenPath(path));
    }

    pub(super) fn shelf_remove(&mut self, i: usize) {
        let old = self.shelf.cards.clone();
        if self.shelf.shelf.remove(i) {
            // A card taken off stops being shared with the phones.
            self.shelf_look();
            let now = Instant::now();
            self.shelf_animate(&old, now, Duration::ZERO);
            self.refresh(now);
        }
    }

    pub(super) fn shelf_clear(&mut self) {
        let old = self.shelf.cards.clone();
        if self.shelf.shelf.clear() {
            // Nothing stays shared with the phones.
            self.shelf_look();
            let now = Instant::now();
            self.shelf_animate(&old, now, Duration::ZERO);
            self.refresh(now);
        }
    }

    /// A left press: on a card whose file is there, it may become a drag.
    pub(super) fn shelf_press(
        &mut self,
        on_card: Option<usize>,
        lparam: windows::Win32::Foundation::LPARAM,
    ) {
        self.shelf.press = on_card
            .filter(|&i| self.shelf.cards.get(i).is_some_and(|c| !c.stale))
            .map(|i| {
                let (x, y) = client_xy(lparam);
                (i, x, y)
            });
    }

    pub(super) fn shelf_release(&mut self) {
        self.shelf.press = None;
    }

    /// The pointer moved with a card pressed: past a few pixels with the
    /// button still down it is a drag out, and the file to carry comes back
    /// (the caller starts `DoDragDrop` outside the app borrow). The click
    /// that would have opened the card is dropped. The button is read from
    /// the move's own `wparam` (`MK_LBUTTON`, the primary button): a press let
    /// go outside the island, whose release never came here, is forgotten on
    /// the first move without it.
    pub(super) fn shelf_drag_start(
        &mut self,
        wparam: WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> Option<PathBuf> {
        let (i, x0, y0) = self.shelf.press?;
        if !primary_down(wparam) || self.state.view() != View::Panel(Page::Shelf) {
            self.shelf.press = None;
            return None;
        }
        let (x, y) = client_xy(lparam);
        let min = DRAG_START_DIP * self.gesture_scale();
        let (dx, dy) = ((x - x0) as f32, (y - y0) as f32);
        if dx * dx + dy * dy < min * min {
            return None;
        }
        self.shelf.press = None;
        self.click.clear();
        let path = PathBuf::from(self.shelf.shelf.get(i)?);
        if shelf::probe(&path) == Probe::Missing {
            self.shelf_look();
            self.refresh(Instant::now());
            return None;
        }
        Some(path)
    }

    /// `DoDragDrop` returned: it swallowed the button's release, so the
    /// press's swipe is dropped here, and a pointer that ended off the
    /// island leaves it (no `WM_MOUSELEAVE` came while OLE held the mouse).
    pub(super) fn shelf_after_drag_out(&mut self) {
        self.shelf.press = None;
        self.swipe.cancel();
        self.click.clear();
        let mut pt = POINT::default();
        let on_island = unsafe {
            GetCursorPos(&mut pt).is_ok() && ScreenToClient(self.hwnd, &mut pt).as_bool()
        } && self.renderer.hit_test(pt.x as f32, pt.y as f32).is_some();
        if !on_island {
            self.on_mouse_leave();
        }
    }
}

/// A `WM_MOUSEMOVE`'s `wparam` says the primary button is down.
fn primary_down(wparam: WPARAM) -> bool {
    wparam.0 as u32 & MK_LBUTTON.0 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_without_the_button_ends_a_press() {
        assert!(primary_down(WPARAM(MK_LBUTTON.0 as usize)));
        // Shift held with the button: still down.
        assert!(primary_down(WPARAM(0x0001 | 0x0004)));
        assert!(!primary_down(WPARAM(0)));
        // The right button alone (MK_RBUTTON) is not a drag.
        assert!(!primary_down(WPARAM(0x0002)));
    }
}
