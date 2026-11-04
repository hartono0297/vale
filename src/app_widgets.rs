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
//!
//! Reminders ("Ingatkan aku"): exactly one more one-shot, `TIMER_STICKY_REMIND`
//! (47) on the island's window, armed for the nearest unfired reminder
//! (`remind::nearest`) and killed and re-armed on every reminder change, in
//! `widgets_sync` (startup included), on a clock jump (`WM_TIMECHANGE`, resume)
//! and on each fire. It is killed first on a fire; nothing polls. A fire marks
//! the due notes fired, raises them (`WidgetHost::raise`) and puts one sticky
//! notice on the island per note; the notice stays while the note is overdue
//! and goes when it is opened, snoozed, cleared, unstuck or deleted.

use std::time::Instant;

use chrono::Local;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::App;
use crate::i18n::{self, Key, tr};
use crate::model::Event;
use crate::widgets::apply::{apply, set_reminder, settle, stack_out, sticky_for};
use crate::widgets::cliptext;
use crate::widgets::host::{Ctx, WidgetHost};
use crate::widgets::prefs::{FullscreenRule, WidgetLayer};
use crate::widgets::remind;
use crate::widgets::sticky::{Sticky, can_add};
use crate::widgets::templates::{self, Template};

/// One-shot: write the notes once widget changes settle.
pub(super) const TIMER_WIDGET_SAVE: usize = 45;
const SAVE_MS: u32 = 1500;
/// One-shot: the nearest sticky note reminder is due.
pub(super) const TIMER_STICKY_REMIND: usize = 47;

#[derive(Default)]
pub(super) struct WidgetsState {
    host: Option<WidgetHost>,
    /// `TIMER_WIDGET_SAVE` is set: a write is due.
    save_due: bool,
    /// The notes whose reminder notice is on the island.
    reminded: Vec<String>,
}

impl App {
    /// Makes the host and brings the windows to the notes (startup).
    pub(super) fn widgets_start(&mut self) {
        if self.widgets.host.is_none() {
            self.widgets.host = Some(WidgetHost::new(self.hwnd));
        }
        // A stack shows the sheet whose reminder is overdue.
        if settle(&mut self.notes, Local::now().timestamp(), true) {
            self.arm_widget_save();
        }
        self.widgets_sync();
    }

    /// Brings the windows to the notes and the settings, and the tray's count.
    /// What the windows still have queued is applied to the notes first, or
    /// the sync would put the store's older copy (a paper, a pin, a box) back
    /// on the window.
    pub(super) fn widgets_sync(&mut self) {
        self.widgets_apply_events(true);
        // Stacks made right: one sheet shown, a lone sheet is a plain note
        // (a sheet deleted on the Notes page).
        if settle(&mut self.notes, Local::now().timestamp(), false) {
            self.arm_widget_save();
        }
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
        // A reminder may have changed: the notices are settled and the one
        // timer follows the new nearest one.
        self.sticky_remind_settle();
        // A shared note that is not a desktop note any more (deleted or
        // unstuck from the Notes page) leaves the phone.
        self.phone_note_check();
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

    /// The 1 s system tick, after the fullscreen check: the cat's note is
    /// drawn again only when its mood changed (asleep after three days).
    pub(super) fn widgets_mood_tick(&mut self) {
        if let Some(host) = self.widgets.host.as_mut() {
            host.mood_tick(Local::now().timestamp());
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
        let (mut changed, structural, clock) = (out.changed, out.structural, out.clock);
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
        for (id, template) in out.beside {
            self.new_sticky(Some(&id), template);
        }
        // A sheet taken out of its stack goes beside it (needs the displays).
        for id in &out.unstack {
            changed |= stack_out(&mut self.notes, &self.displays, id);
        }
        if out.reminders || !out.gone.is_empty() {
            self.sticky_remind_settle();
        }
        // A note that went or came off the desktop is no longer on the phone.
        for id in &out.gone {
            self.phone_note_gone(id);
        }
        if let Some(id) = &out.send_phone {
            self.phone_send_note(id);
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

    // ------------------------------------------------------------ reminders

    /// Arms `TIMER_STICKY_REMIND` for the nearest unfired reminder, or leaves
    /// it killed when there is none. A wait longer than a timer takes is
    /// clamped; the fire just recomputes.
    fn sticky_remind_arm(&self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_STICKY_REMIND);
            if let Some(at) = remind::nearest(self.notes.list()) {
                let wait = remind::wait_ms(at, Local::now().timestamp());
                SetTimer(Some(self.hwnd), TIMER_STICKY_REMIND, wait, None);
            }
        }
    }

    /// The notice of every note that is not overdue any more (snoozed, cleared,
    /// opened, unstuck, deleted) comes down; the timer is armed again.
    pub(super) fn sticky_remind_settle(&mut self) {
        let unix = Local::now().timestamp();
        let stale: Vec<String> = self
            .widgets
            .reminded
            .iter()
            .filter(|id| {
                !self
                    .notes
                    .get(id)
                    .and_then(|n| n.sticky.as_ref())
                    .is_some_and(|s| remind::overdue(s, unix))
            })
            .cloned()
            .collect();
        for id in stale {
            self.widgets.reminded.retain(|r| *r != id);
            self.apply_now(Event::StickyReminderCleared(id));
        }
        self.sticky_remind_arm();
    }

    /// `TIMER_STICKY_REMIND`, startup and a clock jump: the reminders that are
    /// due (also the ones missed while the PC was off or asleep) fire. Killed
    /// first; re-armed for the next one. An overdue note without its notice
    /// (it rang before the island was last closed) gets the notice back,
    /// without being raised or shaken again.
    pub(super) fn sticky_remind_fire(&mut self) {
        // What the windows still have queued comes first: a reminder just
        // cleared or moved in the menu must not ring at its old time.
        self.widgets_events();
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_STICKY_REMIND);
        }
        let unix = Local::now().timestamp();
        // The clock went back past a fired reminder: it is ahead of us again
        // and will ring when its time comes.
        let mut rearmed = false;
        for (id, at) in remind::fired_in_future(self.notes.list(), unix) {
            rearmed |= set_reminder(&mut self.notes, &id, Some(at), false);
        }
        if rearmed {
            self.arm_widget_save();
            self.widgets_sync();
        }
        // Overdue before the island started (the notice lives in memory
        // only): the notice comes back, quietly.
        let quiet: Vec<String> = remind::overdue_ids(self.notes.list(), unix)
            .into_iter()
            .filter(|id| !self.widgets.reminded.contains(id))
            .collect();
        for id in &quiet {
            self.sticky_remind_notice(id);
        }
        let due = remind::due(self.notes.list(), unix);
        if due.is_empty() {
            self.sticky_remind_arm();
            return;
        }
        // A note that comes to the front must be somewhere to be seen.
        self.widgets_reveal();
        for id in &due {
            let Some(at) = self.notes.get(id).and_then(|n| n.sticky.as_ref()).and_then(|s| s.remind_at) else {
                continue;
            };
            set_reminder(&mut self.notes, id, Some(at), true);
        }
        // A stack that holds the sheet shows it now.
        settle(&mut self.notes, unix, true);
        self.arm_widget_save();
        self.widgets_sync();
        if let Some(host) = self.widgets.host.as_mut() {
            host.raise(&due);
        }
        for id in &due {
            self.sticky_remind_notice(id);
        }
        self.sync_settings();
        self.sticky_remind_arm();
    }

    /// Puts note `id`'s reminder notice on the island (its title, and the
    /// time on its chip) and remembers it, so a settle can take it down.
    fn sticky_remind_notice(&mut self, id: &str) {
        let Some(note) = self.notes.get(id) else { return };
        let Some(at) = note.sticky.as_ref().and_then(|s| s.remind_at) else {
            return;
        };
        let title = crate::widgets::checklist::title_of(&note.text);
        let title = if title.is_empty() {
            tr(Key::StickyNoteTitle).to_owned()
        } else {
            title
        };
        let body = remind::chip_label(i18n::lang(), remind::from_unix(at), Local::now().date_naive());
        if !self.widgets.reminded.iter().any(|r| r == id) {
            self.widgets.reminded.push(id.to_owned());
        }
        self.apply_now(Event::Notice(super::sticky_reminder_notice(id, &title, &body)));
    }

    /// The notes come back when they were hidden (Settings, "Tampilkan"), for
    /// whatever is about to bring one to the front. Saved; the caller syncs.
    fn widgets_reveal(&mut self) {
        if !self.config.widgets.show {
            self.config.widgets.show = true;
            self.save_config();
        }
    }

    /// "Tunda 10 mnt": remind again ten minutes from now.
    pub(super) fn sticky_snooze(&mut self, id: &str) {
        let at = remind::snooze(Local::now().timestamp());
        if set_reminder(&mut self.notes, id, Some(at), false) {
            self.arm_widget_save();
        }
        self.widgets_sync();
    }

    /// "Buka": the note's reminder is done, and the note takes the keyboard.
    pub(super) fn sticky_open(&mut self, id: &str) {
        if set_reminder(&mut self.notes, id, None, false) {
            self.arm_widget_save();
        }
        // The keyboard must not go to a note nobody can see.
        self.widgets_reveal();
        self.widgets_sync();
        if let Some(host) = self.widgets.host.as_mut() {
            host.focus(id);
        }
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
    pub(super) fn new_sticky(&mut self, beside: Option<&str>, template: Template) -> bool {
        self.new_sticky_with(beside, template, None)
    }

    /// `new_sticky`, starting with `prefill` (the clipboard's text, already
    /// cleaned by `cliptext::prefill`) instead of a template's text: the caret
    /// goes to the end of it. The cap and its notice are unchanged.
    pub(super) fn new_sticky_with(
        &mut self,
        beside: Option<&str>,
        template: Template,
        prefill: Option<String>,
    ) -> bool {
        if !can_add(self.notes.sticky_count()) {
            self.sticky_full_notice();
            return false;
        }
        self.widgets_reveal();
        let sticky = self.sticky_for(beside);
        let now = Local::now();
        let Some(id) = self.notes.create_sticky(sticky, now.timestamp()) else {
            return false;
        };
        // A template starts the note with its text; the caret goes to the end
        // of its first line. A prefill is the text copied: the caret goes to
        // the end of it.
        let filled = prefill.is_some();
        let start = prefill
            .unwrap_or_else(|| templates::text_of(i18n::lang(), template, now.date_naive()));
        if !start.is_empty() {
            self.notes.set_text(&id, &start, now.timestamp());
        }
        self.arm_widget_save();
        self.widgets_sync();
        if let Some(host) = self.widgets.host.as_mut() {
            let caret = (!start.is_empty() && !filled).then(|| templates::caret_of(&start));
            host.focus_at(&id, caret);
        }
        self.sync_settings();
        true
    }

    fn sticky_full_notice(&mut self) {
        let notice = super::island_notice("sticky-full", tr(Key::StickyFull), tr(Key::StickyMax));
        self.apply_now(Event::Notice(notice));
    }

    /// The text a new note from the hotkey or the tray starts with, when
    /// "Isi dari clipboard" is on: the clipboard is read once, now, on the
    /// island's window (no listener, no retry, no polling). Nothing at the
    /// cap (no note is made, so nothing is read), when another app holds the
    /// clipboard, for a copy a password manager excluded, and for files and
    /// pictures. The text is never logged.
    fn sticky_prefill(&self) -> Option<String> {
        if !cliptext::fill_wanted(self.config.widgets.clip_fill, self.notes.sticky_count()) {
            return None;
        }
        match crate::clipboard::win::read_now(self.hwnd) {
            crate::clipboard::win::Read::Snapshot(snap) => cliptext::from_snapshot(&snap),
            _ => None,
        }
    }

    /// The hotkey row "Catatan tempel baru".
    pub(super) fn on_sticky_hotkey(&mut self) {
        let fill = self.sticky_prefill();
        self.new_sticky_with(None, Template::Kosong, fill);
    }

    /// The tray's "Catatan tempel baru".
    pub(super) fn on_sticky_menu(&mut self) {
        let fill = self.sticky_prefill();
        self.new_sticky_with(None, Template::Kosong, fill);
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
            self.widgets_reveal();
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

    pub(super) fn widgets_tilt(&mut self, on: bool) {
        self.config.widgets.tilt = on;
        self.save_config();
    }

    pub(super) fn widgets_clip_fill(&mut self, on: bool) {
        self.config.widgets.clip_fill = on;
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
