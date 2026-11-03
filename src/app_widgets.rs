//! The island's side of the desktop widgets (`crate::widgets`): it owns the
//! `WidgetHost`, applies what the user does to a note on the desktop to the
//! `NotesStore` (the same notes as the Notes page), makes new notes (the
//! gallery's +, the tray, the hotkey, the Notes page's pin, a note's own +),
//! and keeps the Settings -> Widget page up to date. A child of `app`, so it
//! works on `App` directly.
//!
//! Power: no repeating timer. The one timer here, `TIMER_WIDGET_SAVE` (45), is
//! a one-shot armed 1.5 s after the last widget change (a keystroke, a drag,
//! a colour) and flushed on `WM_DESTROY` and `WM_ENDSESSION`; the Notes page's
//! own saves still write at once and clear it. The fades of a note's header
//! and the gallery's highlight ride the island's frame clock only while they
//! move (`widgets_moving` in `animating_at`). The fullscreen check rides the
//! existing 1 s `TIMER_SYSTEM` tick and does nothing unless a pinned note (or
//! the whole "Di atas jendela" layer) is on screen.

use std::time::Instant;

use chrono::Local;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::App;
use crate::i18n::{self, Key, tr};
use crate::model::Event;
use crate::widgets::apply::{apply, sticky_for};
use crate::widgets::host::{Ctx, WidgetHost};
use crate::widgets::prefs::{FullscreenRule, WidgetLayer};
use crate::widgets::sticky::{Sticky, can_add};

/// One-shot: write the notes once widget changes settle.
pub(super) const TIMER_WIDGET_SAVE: usize = 45;
const SAVE_MS: u32 = 1500;

#[derive(Default)]
pub(super) struct WidgetsState {
    host: Option<WidgetHost>,
    /// `TIMER_WIDGET_SAVE` is set: a write is due.
    save_due: bool,
}

impl App {
    /// Makes the host and brings the windows to the notes (startup).
    pub(super) fn widgets_start(&mut self) {
        if self.widgets.host.is_none() {
            self.widgets.host = Some(WidgetHost::new(self.hwnd));
        }
        self.widgets_sync();
    }

    /// Brings the windows to the notes and the settings, and the tray's count.
    /// What the windows still have queued is applied to the notes first, or
    /// the sync would put the store's older copy (a paper, a pin, a box) back
    /// on the window.
    pub(super) fn widgets_sync(&mut self) {
        self.widgets_apply_events(true);
        let animate = self.motion().moves() && self.animations;
        let App {
            notes,
            displays,
            config,
            widgets,
            ..
        } = self;
        crate::widgets::set_sticky_count(notes.sticky_count());
        let Some(host) = widgets.host.as_mut() else {
            return;
        };
        host.sync(&Ctx {
            notes: notes.list(),
            prefs: config.widgets,
            displays,
            lang: i18n::lang(),
            font: config.font,
            coat: config.cat_color,
            animate,
        });
    }

    /// The config was adopted (a save, a hand edit): the widgets follow it and
    /// draw once.
    pub(super) fn widgets_follow_config(&mut self) {
        self.widgets_sync();
        self.widgets_restyle();
    }

    /// The theme, the language or Windows' colours changed: draw every note.
    pub(super) fn widgets_restyle(&mut self) {
        if let Some(host) = self.widgets.host.as_mut() {
            host.restyle();
        }
    }

    /// `TaskbarCreated`: the shell is new, the notes are owned by it again.
    pub(super) fn widgets_taskbar(&mut self) {
        if let Some(host) = self.widgets.host.as_mut() {
            host.on_taskbar_created();
        }
    }

    /// The monitors changed: the notes are placed again.
    pub(super) fn widgets_displays(&mut self) {
        let App {
            displays, widgets, ..
        } = self;
        if let Some(host) = widgets.host.as_mut() {
            host.displays_changed(displays);
        }
    }

    /// The 1 s system tick.
    pub(super) fn widgets_fullscreen_tick(&mut self) {
        if let Some(host) = self.widgets.host.as_mut() {
            host.fullscreen_tick(|hwnd| crate::sources::fullscreen::is_fullscreen(Some(hwnd)));
        }
    }

    pub(super) fn widgets_moving(&self, now: Instant) -> bool {
        self.widgets
            .host
            .as_ref()
            .is_some_and(|host| host.moving(now))
    }

    /// One frame of the headers' fades and the highlight.
    pub(super) fn widgets_step(&mut self, now: Instant) {
        if let Some(host) = self.widgets.host.as_mut() {
            host.tick(now);
        }
    }

    // ------------------------------------------------------------ saving

    fn arm_widget_save(&mut self) {
        self.widgets.save_due = true;
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_WIDGET_SAVE, SAVE_MS, None);
        }
    }

    /// The notes were written (by anyone): nothing is due any more.
    pub(super) fn widgets_saved(&mut self) {
        if self.widgets.save_due {
            self.widgets.save_due = false;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_WIDGET_SAVE);
            }
        }
    }

    /// `TIMER_WIDGET_SAVE`.
    pub(super) fn on_widget_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_WIDGET_SAVE);
        }
        if self.widgets.save_due {
            self.persist_notes(Instant::now());
        }
    }

    /// `WM_ENDSESSION`: a change still waiting for its save is written now,
    /// with what the windows still have queued.
    pub(super) fn widgets_flush(&mut self) {
        self.widgets_apply_events(false);
        if self.widgets.save_due {
            self.persist_notes(Instant::now());
        }
    }

    /// `WM_DESTROY`: the last save, then the windows and their Direct2D
    /// objects go (they must not outlive the loop).
    pub(super) fn widgets_shutdown(&mut self) {
        self.widgets_flush();
        if let Some(mut host) = self.widgets.host.take() {
            host.shutdown();
        }
    }

    // ------------------------------------------------------------ events

    /// `WM_APP_WIDGET`: what the user did on the desktop.
    pub(super) fn widgets_events(&mut self) {
        if self.widgets_apply_events(true) {
            self.widgets_sync();
        }
    }

    /// Applies what the windows queued to the notes (the queue is emptied, so
    /// a sync it leads to finds nothing more). True when a note came off the
    /// desktop or went, and the windows need a sync. `live` false while the
    /// island ends: the notes and the save only, no new note and no drawing.
    fn widgets_apply_events(&mut self, live: bool) -> bool {
        let events = match self.widgets.host.as_mut() {
            Some(host) => host.drain(),
            None => return false,
        };
        if events.is_empty() {
            return false;
        }
        let out = apply(&mut self.notes, events, Local::now().timestamp());
        let (changed, structural, clock) = (out.changed, out.structural, out.clock);
        if let Some(on) = out.cat_peek {
            self.config.widgets.cat_peek = on;
            self.save_config();
        }
        if !live {
            if changed {
                self.widgets.save_due = true;
            }
            return structural;
        }
        for id in out.beside {
            self.new_sticky(Some(&id));
        }
        if changed {
            self.arm_widget_save();
            // The Notes page shows the same notes: it follows what is typed.
            if matches!(self.state.view(), crate::model::View::Panel(crate::model::Page::Notes)) {
                self.draw(Instant::now());
            }
            self.sync_settings();
        }
        if clock {
            self.ensure_clock();
        }
        structural
    }

    // ------------------------------------------------------------ new notes

    /// Where a new note goes: beside note `beside` (its + was pressed), else
    /// near the notch on the notch's monitor. The sticky part is empty-Krem.
    fn sticky_for(&self, beside: Option<&str>) -> Sticky {
        sticky_for(
            self.notes.list(),
            &self.displays,
            &self.config.notch_monitor,
            beside,
            Local::now().timestamp(),
        )
    }

    /// One empty note on the desktop, with the keyboard. The widgets come
    /// back if they were hidden. At the cap nothing is made and the island
    /// says why. True when a note was made.
    pub(super) fn new_sticky(&mut self, beside: Option<&str>) -> bool {
        if !can_add(self.notes.sticky_count()) {
            self.sticky_full_notice();
            return false;
        }
        if !self.config.widgets.show {
            self.config.widgets.show = true;
            self.save_config();
        }
        let sticky = self.sticky_for(beside);
        let Some(id) = self.notes.create_sticky(sticky, Local::now().timestamp()) else {
            return false;
        };
        self.arm_widget_save();
        self.widgets_sync();
        if let Some(host) = self.widgets.host.as_mut() {
            host.focus(&id);
        }
        self.sync_settings();
        true
    }

    fn sticky_full_notice(&mut self) {
        let notice = super::island_notice("sticky-full", tr(Key::StickyFull), tr(Key::StickyMax));
        self.apply_now(Event::Notice(notice));
    }

    /// The hotkey row "Catatan tempel baru".
    pub(super) fn on_sticky_hotkey(&mut self) {
        self.new_sticky(None);
    }

    /// The tray's "Catatan tempel baru".
    pub(super) fn on_sticky_menu(&mut self) {
        self.new_sticky(None);
    }

    /// The pin on a Notes page card: stick the note to the desktop, or unstick
    /// it (an empty one is gone then).
    pub(super) fn sticky_toggle(&mut self, id: &str) {
        let on_desktop = self.notes.get(id).is_some_and(|n| n.sticky.is_some());
        let done = if on_desktop {
            self.notes.set_sticky(id, None)
        } else {
            if !can_add(self.notes.sticky_count()) {
                self.sticky_full_notice();
                return;
            }
            if !self.config.widgets.show {
                self.config.widgets.show = true;
                self.save_config();
            }
            let sticky = self.sticky_for(None);
            self.notes.set_sticky(id, Some(sticky))
        };
        if done {
            self.persist_notes(Instant::now());
            self.widgets_sync();
            self.sync_settings();
        }
    }

    // ------------------------------------------------------------ Settings

    pub(super) fn widgets_show(&mut self, on: bool) {
        self.config.widgets.show = on;
        self.save_config();
    }

    pub(super) fn widgets_layer(&mut self, layer: WidgetLayer) {
        self.config.widgets.layer = layer;
        self.save_config();
    }

    pub(super) fn widgets_fullscreen(&mut self, rule: FullscreenRule) {
        self.config.widgets.fullscreen = rule;
        self.save_config();
        // Back to the shown state at once when the notes may stay.
        self.widgets_fullscreen_tick();
    }

    pub(super) fn widgets_lock(&mut self, on: bool) {
        self.config.widgets.lock = on;
        self.save_config();
    }

    pub(super) fn widgets_peek(&mut self, on: bool) {
        self.config.widgets.cat_peek = on;
        self.save_config();
    }

    /// A click on the gallery's active card: the notes flash.
    pub(super) fn widgets_highlight(&mut self) {
        if let Some(host) = self.widgets.host.as_mut() {
            host.highlight();
        }
    }
}
