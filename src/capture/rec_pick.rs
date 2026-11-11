//! The screen recording's picker, the part without a window: what the
//! pointer would pick in each mode, the bar under the selection (a three-way
//! switch, the Mic and PC audio toggles, Rekam), the keys, and how the answer
//! reaches the island.
//!
//! `overlay` (its `rec` child) owns the window: the same frozen frame, dim
//! and highlight as the screenshot pick. Nothing here runs while no picker
//! is up.

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::inline::{Side, place_side};
use super::win::TopWindow;
use super::{Candidate, Rect, WM_APP_SCREEN_PICK, drag_rect, pick};
use crate::model::RecSources;

/// An area narrower or lower than this cannot be recorded.
pub const MIN_AREA: i32 = 32;

/// What the picker chooses between.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PickMode {
    /// One whole monitor.
    Screen,
    /// One top-level window (the recording follows it).
    #[default]
    Window,
    /// A free rectangle, snapped like the screenshot tool or dragged.
    Area,
}

/// What was chosen. The numbers are an `HMONITOR` and an `HWND`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickTarget {
    Monitor(isize),
    Window(isize),
    /// Physical pixels, virtual-screen coordinates.
    Area(Rect),
}

/// The picker's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    pub target: PickTarget,
    pub mic: bool,
    pub system: bool,
    /// The window's text for a `Window`, else empty.
    pub title: String,
}

/// A control of the bar, in Tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarBtn {
    Screen,
    Window,
    Area,
    Mic,
    System,
    Record,
}

impl BarBtn {
    pub const ALL: [BarBtn; 6] = [
        BarBtn::Screen,
        BarBtn::Window,
        BarBtn::Area,
        BarBtn::Mic,
        BarBtn::System,
        BarBtn::Record,
    ];

    /// The mode a segment of the switch stands for.
    pub fn mode(self) -> Option<PickMode> {
        match self {
            BarBtn::Screen => Some(PickMode::Screen),
            BarBtn::Window => Some(PickMode::Window),
            BarBtn::Area => Some(PickMode::Area),
            _ => None,
        }
    }
}

/// What a key press asks of the overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Finish with the selection.
    Record,
    Cancel,
    /// A control was activated (a mode chosen, a toggle flipped): redraw.
    Toggle(BarBtn),
    /// The focus moved here.
    Focus(BarBtn),
    None,
}

const VK_TAB: u16 = 0x09;
const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;

/// The picker's choices so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecPick {
    pub mode: PickMode,
    pub mic: bool,
    pub system: bool,
    /// The selection: its rectangle (virtual-screen px) and what it is.
    pub sel: Option<(Rect, PickTarget)>,
    /// The control the keyboard is on (none until Tab is pressed).
    pub focus: Option<BarBtn>,
}

impl RecPick {
    /// A fresh picker on Jendela. The toggles start from the recording
    /// prefs; flipping them here is for this recording only, never saved.
    pub fn new(sources: RecSources) -> RecPick {
        RecPick {
            mode: PickMode::default(),
            mic: sources.mic,
            system: sources.system,
            sel: None,
            focus: None,
        }
    }

    /// What a click at `pt` would select. Screen: the monitor under `pt`.
    /// Window: the frontmost top-level window of `windows` under `pt`, never
    /// a box inside it (`windows` holds no window of Vale's own, so those are
    /// never picked). Area: what the screenshot tool snaps to, the part of it
    /// on the monitor under `pt`.
    pub fn hover(
        &self,
        pt: (i32, i32),
        cands: &[Candidate],
        windows: &[TopWindow],
        monitors: &[(isize, Rect)],
    ) -> Option<(Rect, PickTarget)> {
        match self.mode {
            PickMode::Screen => monitors
                .iter()
                .find(|(_, r)| r.contains(pt))
                .map(|(m, r)| (*r, PickTarget::Monitor(*m))),
            PickMode::Window => windows
                .iter()
                .filter(|w| w.rect.contains(pt))
                .min_by_key(|w| w.z)
                .map(|w| (w.rect, PickTarget::Window(w.hwnd))),
            // A snapped box is cut to the monitor under the pointer, like a
            // dragged one: the recording takes one monitor.
            PickMode::Area => pick(cands, pt)
                .map(|r| monitor_of(pt, monitors).map_or(r, |m| r.intersect(&m)))
                .filter(|r| !r.is_empty())
                .map(|r| (r, PickTarget::Area(r))),
        }
    }

    /// True when Rekam can start: something is selected, and an area is at
    /// least `MIN_AREA` on each side.
    pub fn can_record(&self) -> bool {
        match self.sel {
            Some((_, PickTarget::Area(r))) => r.w >= MIN_AREA && r.h >= MIN_AREA,
            Some(_) => true,
            None => false,
        }
    }

    /// Another mode: what was selected in the old one goes.
    pub fn set_mode(&mut self, m: PickMode) {
        if m != self.mode {
            self.mode = m;
            self.sel = None;
        }
    }

    /// A click on `btn`, or Space / Enter while it has the focus.
    pub fn activate(&mut self, btn: BarBtn) -> Key {
        match btn {
            BarBtn::Mic => self.mic = !self.mic,
            BarBtn::System => self.system = !self.system,
            BarBtn::Record => {
                return if self.can_record() {
                    Key::Record
                } else {
                    Key::None
                };
            }
            BarBtn::Screen | BarBtn::Window | BarBtn::Area => {
                if let Some(mode) = btn.mode() {
                    self.set_mode(mode);
                }
            }
        }
        Key::Toggle(btn)
    }

    /// A key press. Enter records when something recordable is selected and
    /// no control has the focus, else it activates the focused control like
    /// Space; Tab and Shift+Tab walk the six controls and wrap; Esc cancels.
    pub fn key(&mut self, vk: u16, shift: bool) -> Key {
        match vk {
            VK_ESCAPE => Key::Cancel,
            VK_TAB => {
                let n = BarBtn::ALL.len();
                let at = self
                    .focus
                    .and_then(|f| BarBtn::ALL.iter().position(|b| *b == f));
                let next = match (at, shift) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                };
                self.focus = Some(BarBtn::ALL[next]);
                Key::Focus(BarBtn::ALL[next])
            }
            VK_RETURN => match self.focus {
                Some(btn) => self.activate(btn),
                None if self.can_record() => Key::Record,
                None => Key::None,
            },
            VK_SPACE => match self.focus {
                Some(btn) => self.activate(btn),
                None => Key::None,
            },
            _ => Key::None,
        }
    }
}

/// The monitor holding `pt`, or the nearest one.
pub fn monitor_of(pt: (i32, i32), monitors: &[(isize, Rect)]) -> Option<Rect> {
    let off = |r: &Rect| {
        let dx = (r.x - pt.0).max(pt.0 - (r.right() - 1)).max(0);
        let dy = (r.y - pt.1).max(pt.1 - (r.bottom() - 1)).max(0);
        i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy)
    };
    monitors.iter().map(|(_, r)| *r).min_by_key(off)
}

/// True when holding `vk` down may act again (auto-repeat). Not Enter or
/// Space: the first Enter can take the highlight, and its repeat half a
/// second later would start the recording.
pub fn repeats(vk: u16) -> bool {
    !matches!(vk, VK_RETURN | VK_SPACE | VK_ESCAPE)
}

/// The rectangle of a drag from `a` to `b`, kept on the monitor where it
/// began: an area never spans two monitors, so the recording holds exactly
/// what the overlay showed.
pub fn area_on_monitor(a: (i32, i32), b: (i32, i32), monitors: &[(isize, Rect)]) -> Rect {
    // A press in no monitor (a gap in the layout) takes the nearest one.
    match monitor_of(a, monitors) {
        Some(monitor) => drag_rect(a, b, monitor),
        None => Rect::default(),
    }
}

// ---- the bar (dip, scaled by `ui`) ----

/// The labels' size and weight.
pub const LABEL: f32 = 13.0;
pub const LABEL_WEIGHT: i32 = 500;
/// Height of every control.
const CTL: f32 = 32.0;
/// The bar's inset: room for the focus ring around a control.
const PAD: f32 = 6.0;
/// Left and right of a segment's label.
const SEG_PAD: f32 = 12.0;
/// Between the two toggles.
const TOGGLE_GAP: f32 = 4.0;
/// Between the groups; a hairline sits in the middle.
const GROUP_GAP: f32 = 13.0;
/// Rekam: inset, the dot, and the gap to its label.
const REC_PAD: f32 = 14.0;
pub const REC_DOT: f32 = 10.0;
pub const REC_DOT_GAP: f32 = 7.0;

/// Between the selection and the bar, px.
const SEL_GAP: i32 = 8;
/// With nothing selected the bar's bottom sits this far above the monitor's.
const FROM_BOTTOM: i32 = 48;

/// The bar's controls. Laid out at the origin by `bar_layout`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecBar {
    pub items: Vec<(BarBtn, Rect)>,
}

impl RecBar {
    /// The control under `pt`.
    pub fn hit(&self, pt: (i32, i32)) -> Option<BarBtn> {
        self.items
            .iter()
            .find(|(_, r)| r.contains(pt))
            .map(|(b, _)| *b)
    }

    pub fn rect_of(&self, btn: BarBtn) -> Option<Rect> {
        self.items.iter().find(|(b, _)| *b == btn).map(|(_, r)| *r)
    }

    /// The whole bar: the controls and the inset around them.
    pub fn rect(&self) -> Rect {
        let (Some((_, first)), Some((_, last))) = (self.items.first(), self.items.last()) else {
            return Rect::default();
        };
        // The inset is the same on every side, `PAD` for a control `CTL` high.
        let pad = (first.h as f32 * PAD / CTL).round() as i32;
        Rect::new(
            first.x - pad,
            first.y - pad,
            last.right() - first.x + 2 * pad,
            first.h + 2 * pad,
        )
    }

    /// The same bar with its top-left at `(x, y)`.
    pub fn moved_to(&self, x: i32, y: i32) -> RecBar {
        let from = self.rect();
        let (dx, dy) = (x - from.x, y - from.y);
        RecBar {
            items: self
                .items
                .iter()
                .map(|(b, r)| (*b, Rect::new(r.x + dx, r.y + dy, r.w, r.h)))
                .collect(),
        }
    }

    /// The hairlines between the switch, the toggles and Rekam.
    pub fn seps(&self) -> Vec<Rect> {
        [
            (BarBtn::Area, BarBtn::Mic),
            (BarBtn::System, BarBtn::Record),
        ]
        .into_iter()
        .filter_map(|(left, right)| {
            let (l, r) = (self.rect_of(left)?, self.rect_of(right)?);
            let h = l.h * 9 / 16;
            Some(Rect::new((l.right() + r.x) / 2, l.y + (l.h - h) / 2, 1, h))
        })
        .collect()
    }
}

/// The bar at the origin: the three-way switch, a gap, the two square
/// toggles, a gap, Rekam. `widths` are the measured labels of Layar,
/// Jendela, Area and Rekam in px; every control is 32 x `ui` high.
pub fn bar_layout(ui: f32, widths: &[f32; 4]) -> RecBar {
    let s = |dip: f32| (dip * ui).round() as i32;
    let (h, pad) = (s(CTL), s(PAD));
    let mut items = Vec::with_capacity(BarBtn::ALL.len());
    let mut x = pad;
    // The switch: three segments side by side, no gap.
    for (btn, w) in [BarBtn::Screen, BarBtn::Window, BarBtn::Area]
        .into_iter()
        .zip(widths)
    {
        let w = w.ceil() as i32 + 2 * s(SEG_PAD);
        items.push((btn, Rect::new(x, pad, w, h)));
        x += w;
    }
    x += s(GROUP_GAP);
    items.push((BarBtn::Mic, Rect::new(x, pad, h, h)));
    x += h + s(TOGGLE_GAP);
    items.push((BarBtn::System, Rect::new(x, pad, h, h)));
    x += h + s(GROUP_GAP);
    let w = widths[3].ceil() as i32 + s(2.0 * REC_PAD + REC_DOT + REC_DOT_GAP);
    items.push((BarBtn::Record, Rect::new(x, pad, w, h)));
    RecBar { items }
}

/// Where a bar of `size` goes on `monitor`: under the selection (above it
/// when there is no room below, as `place_side` decides); with nothing
/// selected, or a selection that leaves room neither below nor above (a whole
/// screen, a maximised window), centred `FROM_BOTTOM` px above the monitor's
/// bottom.
pub fn place_bar(sel: Option<Rect>, size: (i32, i32), monitor: Rect) -> Rect {
    let (w, h) = size;
    let room =
        |s: &Rect| s.bottom() + SEL_GAP + h <= monitor.bottom() || s.y - SEL_GAP - h >= monitor.y;
    match sel {
        Some(s) if room(&s) => place_side(s, size, monitor, Side::Below, SEL_GAP),
        _ => Rect::new(
            monitor.x + (monitor.w - w) / 2,
            monitor.bottom() - FROM_BOTTOM - h,
            w,
            h,
        ),
    }
}

// ---- the answer ----

thread_local! {
    /// What the last picker chose, until the island takes it.
    static PICKED: RefCell<Option<Picked>> = const { RefCell::new(None) };
}

/// The pick a `WM_APP_SCREEN_PICK` announced; `None` when it was cancelled
/// (or already taken).
pub fn take_picked() -> Option<Picked> {
    PICKED.with(|p| p.borrow_mut().take())
}

/// Tells the picker's owner how it ended, exactly once: `send` with the
/// pick, or by being dropped (every other way out: Esc, a right click, a
/// lost foreground, a failed start) with nothing.
pub struct PickReport {
    owner: isize,
    sent: bool,
}

impl PickReport {
    /// `owner` 0 (the demo) posts to nobody.
    pub fn new(owner: isize) -> PickReport {
        PickReport { owner, sent: false }
    }

    /// Leaves `picked` for `take_picked` and tells the owner.
    pub fn send(self, picked: Picked) {
        let mut report = self;
        PICKED.with(|p| *p.borrow_mut() = Some(picked));
        report.post();
    }

    fn post(&mut self) {
        if self.sent {
            return;
        }
        self.sent = true;
        if self.owner != 0 {
            let owner = HWND(self.owner as *mut core::ffi::c_void);
            unsafe {
                let _ = PostMessageW(Some(owner), WM_APP_SCREEN_PICK, WPARAM(0), LPARAM(0));
            }
        }
    }
}

impl Drop for PickReport {
    fn drop(&mut self) {
        if !self.sent {
            // Nothing picked: an older answer nobody took must not be read
            // as this picker's.
            PICKED.with(|p| *p.borrow_mut() = None);
            self.post();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::inline::ui_for;
    use crate::i18n::{self, Lang};

    const MONITORS: [(isize, Rect); 2] = [
        (1, Rect::new(0, 0, 1920, 1080)),
        (2, Rect::new(1920, 0, 2560, 1440)),
    ];

    fn both() -> RecSources {
        RecSources {
            mic: true,
            system: true,
        }
    }

    fn window(hwnd: isize, x: i32, y: i32, w: i32, h: i32, z: u32) -> TopWindow {
        TopWindow {
            hwnd,
            rect: Rect::new(x, y, w, h),
            z,
        }
    }

    fn inner(x: i32, y: i32, w: i32, h: i32, z: u32) -> Candidate {
        Candidate {
            rect: Rect::new(x, y, w, h),
            z,
            depth: 1,
        }
    }

    fn in_mode(mode: PickMode) -> RecPick {
        let mut p = RecPick::new(both());
        p.set_mode(mode);
        p
    }

    #[test]
    fn the_picker_opens_on_jendela_with_nothing_selected() {
        let p = RecPick::new(both());
        assert_eq!(p.mode, PickMode::Window);
        assert_eq!(p.sel, None);
        assert_eq!(p.focus, None);
        assert!(!p.can_record());
    }

    #[test]
    fn layar_picks_the_monitor_under_the_pointer() {
        let p = in_mode(PickMode::Screen);
        assert_eq!(
            p.hover((2000, 50), &[], &[], &MONITORS),
            Some((Rect::new(1920, 0, 2560, 1440), PickTarget::Monitor(2)))
        );
        assert_eq!(
            p.hover((10, 10), &[], &[], &MONITORS),
            Some((Rect::new(0, 0, 1920, 1080), PickTarget::Monitor(1)))
        );
        // Under the shorter monitor there is no screen.
        assert_eq!(p.hover((100, 1200), &[], &[], &MONITORS), None);
    }

    #[test]
    fn jendela_picks_the_frontmost_window_not_a_box() {
        let p = in_mode(PickMode::Window);
        let windows = [
            window(11, 100, 100, 600, 400, 0),
            window(22, 0, 0, 1200, 900, 1),
        ];
        let cands = [
            windows[0].candidate(),
            inner(120, 140, 200, 100, 0),
            windows[1].candidate(),
        ];
        // A depth-1 box is under the pointer: the window is picked, whole.
        assert_eq!(
            p.hover((150, 160), &cands, &windows, &MONITORS),
            Some((Rect::new(100, 100, 600, 400), PickTarget::Window(11)))
        );
        // Beside the front window the one behind shows.
        assert_eq!(
            p.hover((900, 700), &cands, &windows, &MONITORS),
            Some((Rect::new(0, 0, 1200, 900), PickTarget::Window(22)))
        );
        assert_eq!(p.hover((1500, 1000), &cands, &windows, &MONITORS), None);
    }

    /// Vale's own windows are never recording targets: `win::top_windows`
    /// leaves them out of `windows`, and Jendela picks from that list alone,
    /// so a top-level candidate that is not in it (the frozen frame still
    /// shows Settings, say) is passed over for the window behind.
    #[test]
    fn jendela_never_picks_a_window_that_is_not_in_the_list() {
        let p = in_mode(PickMode::Window);
        let behind = window(22, 0, 0, 1200, 900, 1);
        let vale = Candidate {
            rect: Rect::new(100, 100, 600, 400),
            z: 0,
            depth: 0,
        };
        let cands = [vale, behind.candidate()];
        assert_eq!(
            p.hover((150, 160), &cands, &[behind], &MONITORS),
            Some((Rect::new(0, 0, 1200, 900), PickTarget::Window(22)))
        );
        assert_eq!(p.hover((150, 160), &cands, &[], &MONITORS), None);
    }

    #[test]
    fn area_snaps_like_the_screenshot_tool() {
        let p = in_mode(PickMode::Area);
        let windows = [window(11, 100, 100, 600, 400, 0)];
        let cands = [
            windows[0].candidate(),
            inner(120, 140, 200, 100, 0),
            inner(130, 150, 50, 40, 0),
        ];
        // Inside one monitor it is the screenshot tool's snap, untouched.
        for pt in [(135, 155), (200, 200), (600, 450), (5, 5)] {
            let want = pick(&cands, pt).map(|r| (r, PickTarget::Area(r)));
            assert_eq!(p.hover(pt, &cands, &windows, &MONITORS), want, "{pt:?}");
        }
        assert_eq!(
            p.hover((135, 155), &cands, &windows, &MONITORS),
            Some((
                Rect::new(130, 150, 50, 40),
                PickTarget::Area(Rect::new(130, 150, 50, 40))
            ))
        );
    }

    /// A window across two monitors (or any window before its inner boxes
    /// are known) snaps to the part on the monitor under the pointer: the
    /// recording takes one monitor.
    #[test]
    fn a_snapped_area_is_kept_on_the_monitor_under_the_pointer() {
        let p = in_mode(PickMode::Area);
        let windows = [window(11, 1800, 100, 400, 300, 0)];
        let cands = [windows[0].candidate()];
        let left = Rect::new(1800, 100, 120, 300);
        assert_eq!(
            p.hover((1850, 200), &cands, &windows, &MONITORS),
            Some((left, PickTarget::Area(left)))
        );
        let right = Rect::new(1920, 100, 280, 300);
        assert_eq!(
            p.hover((2000, 200), &cands, &windows, &MONITORS),
            Some((right, PickTarget::Area(right)))
        );
        // Jendela still takes the whole window: it is followed, not cropped.
        assert_eq!(
            in_mode(PickMode::Window).hover((1850, 200), &cands, &windows, &MONITORS),
            Some((Rect::new(1800, 100, 400, 300), PickTarget::Window(11)))
        );
        assert_eq!(monitor_of((1850, 200), &MONITORS), Some(MONITORS[0].1));
        assert_eq!(monitor_of((2000, 200), &MONITORS), Some(MONITORS[1].1));
        // Below the shorter monitor: the nearest one.
        assert_eq!(monitor_of((100, 1200), &MONITORS), Some(MONITORS[0].1));
        assert_eq!(monitor_of((5, 5), &[]), None);
    }

    #[test]
    fn a_held_enter_or_space_does_not_repeat() {
        // The first Enter takes the highlight; its auto-repeat must not
        // start the recording.
        assert!(!repeats(VK_RETURN));
        assert!(!repeats(VK_SPACE));
        assert!(!repeats(VK_ESCAPE));
        // A held Tab keeps walking the bar.
        assert!(repeats(VK_TAB));
    }

    #[test]
    fn an_area_is_kept_on_the_monitor_it_started_on() {
        assert_eq!(
            area_on_monitor((100, 100), (2500, 900), &MONITORS),
            Rect::new(100, 100, 1820, 800)
        );
        // The other way round it stays on the second monitor.
        assert_eq!(
            area_on_monitor((2500, 900), (100, 100), &MONITORS),
            Rect::new(1920, 100, 580, 800)
        );
        // Inside one monitor it is the plain drag, in any direction.
        assert_eq!(
            area_on_monitor((400, 350), (100, 200), &MONITORS),
            Rect::new(100, 200, 300, 150)
        );
        assert!(area_on_monitor((10, 10), (500, 500), &[]).is_empty());
    }

    #[test]
    fn a_tiny_area_cannot_be_recorded() {
        let mut p = in_mode(PickMode::Area);
        let area = |w: i32, h: i32| {
            let r = Rect::new(50, 50, w, h);
            Some((r, PickTarget::Area(r)))
        };
        p.sel = area(31, 200);
        assert!(!p.can_record());
        p.sel = area(200, 31);
        assert!(!p.can_record());
        p.sel = area(32, 32);
        assert!(p.can_record());
        assert_eq!(MIN_AREA, 32);
        // A window or a screen is recordable as it is.
        p.sel = Some((Rect::new(0, 0, 20, 20), PickTarget::Window(7)));
        assert!(p.can_record());
    }

    #[test]
    fn switching_mode_clears_the_selection() {
        let mut p = RecPick::new(both());
        p.sel = Some((Rect::new(0, 0, 800, 600), PickTarget::Window(7)));
        // The mode it is already in keeps it.
        p.set_mode(PickMode::Window);
        assert!(p.sel.is_some());
        p.set_mode(PickMode::Area);
        assert_eq!(p.mode, PickMode::Area);
        assert_eq!(p.sel, None);
        assert!(!p.can_record());
    }

    #[test]
    fn toggles_start_from_the_recording_prefs() {
        for (mic, system) in [(true, true), (true, false), (false, true), (false, false)] {
            let p = RecPick::new(RecSources { mic, system });
            assert_eq!((p.mic, p.system), (mic, system));
        }
    }

    const WIDTHS: [f32; 4] = [38.0, 52.0, 30.0, 40.0];

    #[test]
    fn the_bar_sits_under_the_selection_inside_the_monitor() {
        let monitor = Rect::new(0, 0, 1920, 1080);
        let size = (300, 44);
        let inside = |r: Rect| r.intersect(&monitor) == r;
        // Under it, right edges together.
        let under = place_bar(Some(Rect::new(400, 200, 800, 500)), size, monitor);
        assert_eq!(under, Rect::new(900, 708, 300, 44));
        // A selection at the monitor's bottom: above it.
        let above = place_bar(Some(Rect::new(400, 600, 800, 480)), size, monitor);
        assert_eq!(above, Rect::new(900, 548, 300, 44));
        // A narrow selection at the left edge: still all on the monitor.
        let edge = place_bar(Some(Rect::new(0, 100, 60, 300)), size, monitor);
        assert_eq!(edge, Rect::new(0, 408, 300, 44));
        // Nothing selected, a whole screen, a maximised window: centred near
        // the bottom.
        let centred = Rect::new(810, 988, 300, 44);
        assert_eq!(place_bar(None, size, monitor), centred);
        assert_eq!(place_bar(Some(monitor), size, monitor), centred);
        assert_eq!(
            place_bar(Some(Rect::new(0, 0, 1920, 1040)), size, monitor),
            centred
        );
        for r in [under, above, edge, centred] {
            assert!(inside(r), "{r:?}");
        }
        // On a second monitor it stays there.
        let second = Rect::new(1920, 0, 2560, 1440);
        let r = place_bar(None, size, second);
        assert_eq!(r, Rect::new(1920 + 1130, 1440 - 48 - 44, 300, 44));
    }

    #[test]
    fn the_bar_hits_each_control() {
        for ui in [1.0, 1.5] {
            let bar = bar_layout(ui, &WIDTHS).moved_to(-300, 700);
            assert_eq!(
                bar.items.iter().map(|(b, _)| *b).collect::<Vec<_>>(),
                BarBtn::ALL
            );
            let h = (32.0 * ui).round() as i32;
            for (btn, r) in &bar.items {
                assert_eq!(r.h, h, "{btn:?}");
                assert_eq!(bar.hit(r.center()), Some(*btn));
                assert_eq!(bar.hit((r.x, r.y)), Some(*btn));
                assert_eq!(bar.rect_of(*btn), Some(*r));
                assert_eq!(bar.rect().intersect(r), *r, "{btn:?} is on the bar");
            }
            // The toggles are squares.
            for btn in [BarBtn::Mic, BarBtn::System] {
                let r = bar.rect_of(btn).expect("a toggle");
                assert_eq!(r.w, r.h);
            }
            // The inset and the gaps between the groups hit nothing.
            let whole = bar.rect();
            assert_eq!((whole.x, whole.y), (-300, 700));
            assert_eq!(bar.hit((whole.x + 1, whole.y + 1)), None);
            assert_eq!(bar.seps().len(), 2);
            for sep in bar.seps() {
                assert_eq!(bar.hit(sep.center()), None);
                assert_eq!(whole.intersect(&sep), sep);
            }
            assert_eq!(bar.hit((whole.x - 5, whole.y + 10)), None);
        }
    }

    #[test]
    fn tab_moves_focus_through_the_bar_and_wraps() {
        let mut p = RecPick::new(both());
        for want in BarBtn::ALL {
            assert_eq!(p.key(VK_TAB, false), Key::Focus(want));
            assert_eq!(p.focus, Some(want));
        }
        // Past Rekam it starts again.
        assert_eq!(p.key(VK_TAB, false), Key::Focus(BarBtn::Screen));
        // Shift+Tab goes back, past the first to the last.
        assert_eq!(p.key(VK_TAB, true), Key::Focus(BarBtn::Record));
        assert_eq!(p.key(VK_TAB, true), Key::Focus(BarBtn::System));
        // From no focus Shift+Tab lands on the last.
        let mut q = RecPick::new(both());
        assert_eq!(q.key(VK_TAB, true), Key::Focus(BarBtn::Record));
        // Other keys do nothing; Esc cancels wherever the focus is.
        assert_eq!(q.key(0x41, false), Key::None);
        assert_eq!(q.key(VK_ESCAPE, false), Key::Cancel);
        assert_eq!(RecPick::new(both()).key(VK_ESCAPE, false), Key::Cancel);
    }

    #[test]
    fn enter_records_only_with_a_selection() {
        let mut p = RecPick::new(both());
        assert_eq!(p.key(VK_RETURN, false), Key::None);
        p.sel = Some((Rect::new(0, 0, 800, 600), PickTarget::Window(7)));
        assert_eq!(p.key(VK_RETURN, false), Key::Record);
        // An area too small to record does not start either.
        let tiny = Rect::new(0, 0, 10, 10);
        p.set_mode(PickMode::Area);
        p.sel = Some((tiny, PickTarget::Area(tiny)));
        assert_eq!(p.key(VK_RETURN, false), Key::None);
        // With a control focused Enter is that control, not Rekam.
        let mut q = RecPick::new(both());
        q.sel = Some((Rect::new(0, 0, 800, 600), PickTarget::Window(7)));
        q.focus = Some(BarBtn::Mic);
        assert_eq!(q.key(VK_RETURN, false), Key::Toggle(BarBtn::Mic));
        assert!(!q.mic);
        assert!(q.sel.is_some());
        // On Rekam itself it records, or does nothing without a selection.
        q.focus = Some(BarBtn::Record);
        assert_eq!(q.key(VK_RETURN, false), Key::Record);
        assert_eq!(q.key(VK_SPACE, false), Key::Record);
        q.sel = None;
        assert_eq!(q.key(VK_RETURN, false), Key::None);
    }

    #[test]
    fn space_flips_the_focused_toggle() {
        let mut p = RecPick::new(both());
        // Nothing focused: Space is nothing.
        assert_eq!(p.key(VK_SPACE, false), Key::None);
        p.focus = Some(BarBtn::Mic);
        assert_eq!(p.key(VK_SPACE, false), Key::Toggle(BarBtn::Mic));
        assert_eq!((p.mic, p.system), (false, true));
        assert_eq!(p.key(VK_SPACE, false), Key::Toggle(BarBtn::Mic));
        assert!(p.mic);
        p.focus = Some(BarBtn::System);
        assert_eq!(p.key(VK_SPACE, false), Key::Toggle(BarBtn::System));
        assert_eq!((p.mic, p.system), (true, false));
        // On a segment it chooses that mode.
        p.focus = Some(BarBtn::Screen);
        assert_eq!(p.key(VK_SPACE, false), Key::Toggle(BarBtn::Screen));
        assert_eq!(p.mode, PickMode::Screen);
        assert_eq!(p.focus, Some(BarBtn::Screen), "the focus stays");
    }

    /// How many `WM_APP_SCREEN_PICK` wait for `hwnd`.
    fn posted(hwnd: HWND) -> usize {
        use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW};
        let mut n = 0;
        let mut m = MSG::default();
        while unsafe {
            PeekMessageW(
                &mut m,
                Some(hwnd),
                WM_APP_SCREEN_PICK,
                WM_APP_SCREEN_PICK,
                PM_REMOVE,
            )
        }
        .as_bool()
        {
            n += 1;
        }
        n
    }

    #[test]
    fn the_pick_report_posts_once() {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
        };
        use windows::core::w;
        // Message-only: never on screen.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .expect("a message-only window");
        let owner = hwnd.0 as isize;
        let picked = Picked {
            target: PickTarget::Window(7),
            mic: true,
            system: false,
            title: "Catatan".into(),
        };

        // Sent, then dropped inside `send`: one message, and the pick waits.
        PickReport::new(owner).send(picked.clone());
        assert_eq!(posted(hwnd), 1);
        assert_eq!(take_picked(), Some(picked.clone()));
        assert_eq!(take_picked(), None, "taken once");

        // Dropped alone (a cancel): one message, nothing picked.
        drop(PickReport::new(owner));
        assert_eq!(posted(hwnd), 1);
        assert_eq!(take_picked(), None);

        // A cancel after an answer nobody took leaves no stale pick behind.
        PickReport::new(owner).send(picked);
        drop(PickReport::new(owner));
        assert_eq!(posted(hwnd), 2);
        assert_eq!(take_picked(), None);

        // The demo's report (no owner) posts nothing.
        drop(PickReport::new(0));
        assert_eq!(posted(hwnd), 0);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    #[test]
    fn bar_labels_fit_in_both_languages() {
        let gfx = crate::shot_edit::gfx::Gfx::new().expect("DirectWrite works without a window");
        for lang in [Lang::Id, Lang::En] {
            for dpi in [96, 144] {
                let ui = ui_for(dpi);
                let widths = [
                    i18n::Key::ScreenPickScreen,
                    i18n::Key::ScreenPickWindow,
                    i18n::Key::ScreenPickArea,
                    i18n::Key::ScreenPickRecord,
                ]
                .map(|k| gfx.measure(i18n::text(lang, k), LABEL * ui, LABEL_WEIGHT).0);
                let bar = bar_layout(ui, &widths);
                let whole = bar.rect();
                for (i, (a, ra)) in bar.items.iter().enumerate() {
                    assert_eq!(whole.intersect(ra), *ra, "{lang:?} {dpi}: {a:?} on the bar");
                    for (b, rb) in &bar.items[i + 1..] {
                        assert!(
                            ra.intersect(rb).is_empty(),
                            "{lang:?} {dpi}: {a:?} overlaps {b:?}"
                        );
                    }
                }
                // Each label has its width and the inset on both sides.
                let pad = (SEG_PAD * ui).round() as i32;
                for (i, btn) in [BarBtn::Screen, BarBtn::Window, BarBtn::Area]
                    .into_iter()
                    .enumerate()
                {
                    let r = bar.rect_of(btn).expect("a segment");
                    assert!(r.w >= widths[i].ceil() as i32 + 2 * pad, "{lang:?} {btn:?}");
                }
                let rec = bar.rect_of(BarBtn::Record).expect("Rekam");
                let fixed = ((2.0 * REC_PAD + REC_DOT + REC_DOT_GAP) * ui).round() as i32;
                assert!(rec.w >= widths[3].ceil() as i32 + fixed, "{lang:?} Rekam");
            }
        }
    }
}
