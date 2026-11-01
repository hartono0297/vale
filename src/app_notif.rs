//! The island's side of the Notifikasi page: the history of the toasts the
//! poller catches, their logos, the selection and scroll, the encrypted file,
//! the page's keyboard and the "Hapus semua" / "Jeda" / exclusion buttons. A
//! child of `app`, so it works on `App` directly. The model is `notif/`, the
//! layout `panel_layout::notif_layout`, the drawing
//! `render_panel::paint_notifications`.
//!
//! Power: no repeating timer anywhere. Saves are one one-shot
//! (`TIMER_NOTIF_SAVE`) 1.5 s after the last change; "Hapus semua" rides the
//! reducer's `next_deadline`; relative times are worked out when drawn.
//! Nothing here touches Windows' own Action Center.

use std::collections::HashMap;
use std::time::Instant;

use chrono::Local;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, KillTimer, SetForegroundWindow, SetTimer,
};

use super::App;
use super::clip::pointer_moved;
use crate::clipboard::image::Thumb;
use crate::config::{self, NotifSettings};
use crate::model::{Event, Hit, Notice, NoticeKind, NotifAction, Page, Toast, View};
use crate::notif::persist::{self, Saved, Write};
use crate::notif::store::clamp_cap;
use crate::notif::{NotifHistory, logo_key, route_toast};
use crate::sources::notifications::{self, is_muted};
use crate::ui::cli_input::{CliInput, Key as InputKey};
use crate::ui::render::ToastLogoView;
use crate::ui::editor;
use crate::ui::geometry::{ShapeKind, shape_for};
use crate::ui::panel_layout::{
    notif_max_scroll, notif_rows, notif_scroll_to, notif_visible_row, notif_wheel_step,
};
use crate::ui::render::NotifView;

/// One-shot: write the history file once changes settle.
pub(super) const TIMER_NOTIF_SAVE: usize = 34;
const SAVE_MS: u32 = 1500;

/// The history and the page's state that is not the reducer's.
#[derive(Default)]
pub(super) struct NotifState {
    pub history: NotifHistory,
    /// App logos by logo key; restored from the file, filled by the poller.
    pub logos: HashMap<String, Thumb>,
    /// The chosen row (an entry id); `None`: no row is lit.
    pub selected: Option<u64>,
    /// Rows scrolled.
    pub scroll: usize,
    /// Where the last mouse move over the page was (client pixels).
    pub pointer: Option<(i32, i32)>,
    /// The keyboard moved the selection: the pointer selects nothing until it
    /// really moves (`pointer_moved`).
    pub keyboard_held: bool,
    /// `TIMER_NOTIF_SAVE` is set: a write is due.
    pub save_due: bool,
    /// The listener may read the Action Center (read at start, when the page
    /// opens and after the access answer).
    pub access: bool,
    pub poller_started: bool,
    /// `notif_start` ran: the config may be followed.
    started: bool,
    /// The page showed at the last `sync_notif`.
    shown: bool,
    /// What `notif_follow_config` last saw, to act only on a change.
    persist_seen: bool,
    muted_seen: Vec<String>,
}

/// The row the pointer moved onto, when it is not the selected one: the row
/// under it or one of its buttons, listed in `ids`. `None` keeps the
/// selection.
pub(super) fn hover_target(ids: &[u64], chosen: Option<u64>, hit: Option<&Hit>) -> Option<u64> {
    let id = match hit? {
        Hit::NotifRow(id) | Hit::NotifOpen(id) | Hit::NotifMute(id) | Hit::NotifRemove(id) => *id,
        _ => return None,
    };
    let current = chosen.filter(|c| ids.contains(c));
    (ids.contains(&id) && current != Some(id)).then_some(id)
}

/// The selection after the keyboard moved it: Up and Down from `chosen`
/// (an index into the list of `n`), None starting at the first row either
/// way. `None` for an empty list.
pub(super) fn step_selection(chosen: Option<usize>, n: usize, up: bool) -> Option<usize> {
    if n == 0 {
        return None;
    }
    Some(match (chosen, up) {
        (None, _) => 0,
        (Some(at), true) => at.saturating_sub(1),
        (Some(at), false) => (at + 1).min(n - 1),
    })
}

/// The row to select once `id` is removed from `ids`: the one that takes its
/// place, else the one before it.
pub(super) fn selection_after_remove(ids: &[u64], id: u64) -> Option<u64> {
    let at = ids.iter().position(|i| *i == id)?;
    ids.get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|j| ids.get(j)))
        .copied()
}

/// What the Notifikasi page draws, from the parts of `App` it needs.
pub(super) fn notif_view<'a>(
    notif: &'a NotifState,
    cfg: NotifSettings,
    armed: bool,
) -> NotifView<'a> {
    let items = notif.history.items();
    NotifView {
        items,
        logos: &notif.logos,
        selected: notif
            .selected
            .filter(|id| items.iter().any(|e| e.id == *id)),
        scroll: notif.scroll,
        cap: notif.history.cap(),
        record: cfg.record,
        paused: cfg.paused,
        no_access: !notif.access,
        armed,
        now: Local::now(),
    }
}

impl App {
    /// At startup: the saved history (when kept, minus what is muted now) and
    /// the listener, if it may run.
    pub(super) fn notif_start(&mut self) {
        let cfg = self.config.notifications;
        let saved = persist::start_items(&persist::path(), cfg.persist);
        let mut history = NotifHistory::from_parts(cfg.max_items, saved.next_id, saved.items);
        history.remove_muted(&self.config.muted_apps);
        self.notif.history = history;
        self.notif.logos = saved.logos;
        self.notif.persist_seen = cfg.persist;
        self.notif.muted_seen = self.config.muted_apps.clone();
        self.notif.started = true;
        self.spawn_notifications();
    }

    /// Starts the notification poller once, when the listener may read the
    /// Action Center (it would exit at once otherwise). Reads the access
    /// again each time, so the page opening or the access answer catches a
    /// grant made since. Never asks for access.
    pub(super) fn spawn_notifications(&mut self) {
        let was = self.notif.access;
        self.notif.access = notifications::access_allowed();
        if self.notif.access && !self.notif.poller_started {
            self.notif.poller_started = true;
            notifications::spawn_poller(
                self.tx.clone(),
                super::poster(self.hwnd, super::WM_APP_EVENT),
            );
        }
        if was != self.notif.access {
            self.redraw_notif();
        }
    }

    /// The Notifikasi page just opened: access is read again (the poller
    /// starts if it was granted meanwhile) and the page starts at the top
    /// with nothing selected. It leaving does nothing (no timer ran).
    pub(super) fn sync_notif(&mut self) {
        let wanted = self.state.page() == Page::Notifications
            && self.state.panel_is_open()
            && !self.state.fullscreen();
        if wanted && !self.notif.shown {
            self.notif.scroll = 0;
            self.notif.selected = None;
            self.notif.pointer = None;
            self.notif.keyboard_held = false;
            self.spawn_notifications();
        }
        self.notif.shown = wanted;
    }

    /// A toast from the poller: its logo is kept, then `route_toast` decides.
    /// The notch gets its notice exactly as before (a muted app's never
    /// reaches it; `muted_apps` is read now, not when the poller started),
    /// the history gets an entry while recording and not paused.
    pub(super) fn on_toast(&mut self, toast: Toast, now: Instant) {
        let Toast { notice, at, logo } = toast;
        // `notif.logos` is never pruned during a run (clear, remove and mute
        // leave it alone): the notch notice of a later toast, one with
        // "Simpan notifikasi" off, paused or unrecorded, still finds its
        // logo here (`toast_logo_view`). Only the saved file drops unused ones.
        if let Some((key, thumb)) = logo {
            self.notif.logos.insert(key, thumb);
        }
        let cfg = self.config.notifications;
        let (notch, history) = route_toast(
            &notice.source,
            &self.config.muted_apps,
            cfg.record,
            cfg.paused,
        );
        if history {
            let (app_id, site) = match &notice.kind {
                NoticeKind::App { app_id, toast } => (
                    app_id.clone(),
                    toast.as_ref().and_then(|origin| origin.site.clone()),
                ),
                _ => (None, None),
            };
            self.notif.history.push(
                at,
                &notice.source,
                app_id.as_deref(),
                site.as_deref(),
                &notice.title,
                &notice.body,
            );
            // A list scrolled down keeps showing the same rows.
            if self.notif.scroll > 0 {
                self.notif.scroll += 1;
            }
            self.clamp_notif_scroll();
            self.schedule_notif_save();
        }
        if notch {
            // The drain refreshes the island for the notice (`raised`).
            self.system.raised = true;
            self.apply(Event::Notice(notice), now);
        }
        if history {
            self.redraw_notif();
        }
    }

    /// Redraws the page if it is on screen.
    pub(super) fn redraw_notif(&mut self) {
        if self.state.view() == View::Panel(Page::Notifications) && !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// What goes to the file: the entries, the counter and the logos in use.
    fn notif_saved(&self) -> Saved {
        Saved::of(
            self.notif.history.next_id(),
            self.notif.history.items(),
            &self.notif.logos,
        )
    }

    /// The file follows a change once, `SAVE_MS` later (a burst writes once).
    /// Nothing with "Simpan setelah restart" off.
    pub(super) fn schedule_notif_save(&mut self) {
        if !self.config.notifications.persist {
            return;
        }
        self.notif.save_due = true;
        // Replaces a pending one: the last change sets the time.
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_NOTIF_SAVE, SAVE_MS, None);
        }
    }

    /// `TIMER_NOTIF_SAVE`.
    pub(super) fn on_notif_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_NOTIF_SAVE);
        }
        if std::mem::take(&mut self.notif.save_due) && self.config.notifications.persist {
            persist::write_in_background(persist::path(), Write::Save(self.notif_saved()));
        }
    }

    /// The file (or its removal) now, in the background: after a clear.
    fn write_notif_file_now(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_NOTIF_SAVE);
        }
        self.notif.save_due = false;
        if self.config.notifications.persist {
            persist::write_in_background(persist::path(), Write::Save(self.notif_saved()));
        }
    }

    /// `WM_DESTROY` and `WM_ENDSESSION`: a pending save is written now.
    pub(super) fn notif_shutdown(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_NOTIF_SAVE);
        }
        if std::mem::take(&mut self.notif.save_due) && self.config.notifications.persist {
            persist::write_now(&persist::path(), Write::Save(self.notif_saved()));
        }
    }

    /// config.json changed (a save, or a hand edit picked up): the cap trims,
    /// "Simpan setelah restart" off deletes the file and cancels the save (on:
    /// a save is scheduled), and apps muted now lose their entries. Nothing
    /// before `notif_start`.
    pub(super) fn notif_follow_config(&mut self) {
        if !self.notif.started {
            return;
        }
        let cfg = self.config.notifications;
        let mut changed = false;
        if self.notif.history.cap() != clamp_cap(cfg.max_items) {
            self.notif.history.set_cap(cfg.max_items);
            changed = true;
        }
        if cfg.persist != self.notif.persist_seen {
            self.notif.persist_seen = cfg.persist;
            if cfg.persist {
                self.schedule_notif_save();
            } else {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_NOTIF_SAVE);
                }
                self.notif.save_due = false;
                persist::write_in_background(persist::path(), Write::Remove);
            }
        }
        if self.notif.muted_seen != self.config.muted_apps {
            self.notif.muted_seen = self.config.muted_apps.clone();
            if self.notif.history.remove_muted(&self.config.muted_apps) > 0 {
                changed = true;
            }
        }
        if changed {
            self.fix_notif_selection();
            self.clamp_notif_scroll();
            self.schedule_notif_save();
            self.redraw_notif();
        }
    }

    /// The selection must be a listed row.
    fn fix_notif_selection(&mut self) {
        let history = &self.notif.history;
        if self
            .notif
            .selected
            .is_some_and(|id| history.get(id).is_none())
        {
            self.notif.selected = None;
        }
    }

    /// Entries the page lists: none without access.
    fn notif_count(&self) -> usize {
        if self.notif.access {
            self.notif.history.len()
        } else {
            0
        }
    }

    fn notif_row_room(&self) -> usize {
        let s = shape_for(self.config.style, ShapeKind::Panel);
        notif_rows(
            s,
            self.state.waiting_strip().is_some(),
            !self.config.notifications.record,
        )
    }

    pub(super) fn clamp_notif_scroll(&mut self) {
        let max = notif_max_scroll(self.notif_count(), self.notif_row_room());
        self.notif.scroll = self.notif.scroll.min(max);
    }

    /// The wheel over the rows: a row per notch. False when there is nothing
    /// to scroll (the rows all fit): the caller turns the page.
    pub(super) fn notif_wheel(&mut self, delta: i32) -> bool {
        let (n, rows) = (self.notif_count(), self.notif_row_room());
        let Some(to) = notif_wheel_step(delta, self.notif.scroll, notif_max_scroll(n, rows)) else {
            return false;
        };
        if to != self.notif.scroll {
            self.notif.scroll = to;
            // The selection stays on a row that shows.
            let items = self.notif.history.items();
            if let Some(at) = self
                .notif
                .selected
                .and_then(|id| items.iter().position(|e| e.id == id))
            {
                let keep = notif_visible_row(at, to, rows, n);
                self.notif.selected = items.get(keep).map(|e| e.id);
            }
            self.redraw_notif();
        }
        true
    }

    /// The pointer over a row or one of its buttons selects that row, so its
    /// buttons show under the pointer. Only a real move does
    /// (`pointer_moved`): a still pointer over rows that scrolled, or one
    /// resting after the keyboard chose, keeps the selection.
    pub(super) fn notif_hover(&mut self, at: (i32, i32), hit: Option<&Hit>) {
        if self.state.view() != View::Panel(Page::Notifications) {
            return;
        }
        if !pointer_moved(&mut self.notif.pointer, at, &mut self.notif.keyboard_held) {
            return;
        }
        // Every mouse move comes here: only a row or one of its buttons that
        // is not the chosen row needs the list looked at.
        let id = match hit {
            Some(
                Hit::NotifRow(id) | Hit::NotifOpen(id) | Hit::NotifMute(id) | Hit::NotifRemove(id),
            ) => *id,
            _ => return,
        };
        if self.notif.selected == Some(id) {
            return;
        }
        let ids: Vec<u64> = self.notif.history.items().iter().map(|e| e.id).collect();
        if let Some(id) = hover_target(&ids, self.notif.selected, hit) {
            self.notif.selected = Some(id);
            self.redraw_notif();
        }
    }

    /// After the reducer: a closed panel forgets the selection and the scroll.
    pub(super) fn follow_notif_page(&mut self, was_open: bool) {
        if was_open && !self.state.panel_is_open() {
            self.notif.selected = None;
            self.notif.scroll = 0;
            self.notif.pointer = None;
            self.notif.keyboard_held = false;
        }
    }

    /// A row's click or button, "Jeda", "Hapus semua" or the access button,
    /// from the reducer. Nothing here touches the Windows Action Center.
    pub(super) fn perform_notif(&mut self, action: NotifAction) {
        match action {
            NotifAction::Open(id) => {
                // Read, and its app comes forward when the toast knew it.
                let app = self.notif.history.get(id).and_then(|e| e.app_id.clone());
                self.notif.selected = Some(id);
                if self.notif.history.mark_read(id) {
                    self.schedule_notif_save();
                }
                if let Some(app) = app {
                    notifications::open_app(&app);
                }
            }
            NotifAction::Remove(id) => {
                let ids: Vec<u64> = self.notif.history.items().iter().map(|e| e.id).collect();
                let next = selection_after_remove(&ids, id);
                if self.notif.history.remove(id) {
                    // Keeps its place for the pointer; the keyboard's row
                    // moves to the one that takes over.
                    self.notif.selected = next;
                    self.clamp_notif_scroll();
                    self.schedule_notif_save();
                }
            }
            NotifAction::Mute(id) => self.mute_notif_app(id),
            NotifAction::TogglePause => {
                self.config.notifications.paused = !self.config.notifications.paused;
                self.save_config();
            }
            NotifAction::ClearAll => self.clear_notif_history(),
            NotifAction::OpenSettings => notifications::open_privacy_settings(),
        }
        self.redraw_notif();
    }

    /// "Kecualikan app ini": the entry's app joins `muted_apps` (unless it is
    /// muted already). The edit is made to the file's list inside the save,
    /// so names edited by hand meanwhile stay. `adopt_config` then removes
    /// every entry now muted, and an open Settings window is brought up to
    /// date.
    fn mute_notif_app(&mut self, id: u64) {
        let Some(source) = self.notif.history.get(id).map(|e| e.source.clone()) else {
            return;
        };
        self.save_config_with(|c| {
            if !is_muted(&source, &c.muted_apps) {
                config::add_muted(c, &source);
            }
        });
        self.sync_settings();
    }

    /// Everything out of Vale's list (never Windows' own): memory and file.
    pub(super) fn clear_notif_history(&mut self) {
        if self.notif.history.clear() {
            self.write_notif_file_now();
        }
        self.notif.selected = None;
        self.notif.scroll = 0;
        self.redraw_notif();
    }

    // ---- The page's keyboard (cli-panel-keyboard-focus.md) ----

    /// The page's input has the keyboard for real.
    pub(super) fn notif_has_keyboard(&self) -> bool {
        self.notif_input.as_ref().is_some_and(CliInput::has_focus)
            && unsafe { GetForegroundWindow() } == self.hwnd
    }

    /// A click on the list's background or the title: the island activates
    /// (this process got the click) and the hidden EDIT takes the keyboard.
    /// The window that had it is remembered. An open editor comes forward
    /// instead: it may hold unsaved text.
    pub(super) fn focus_notif_input(&mut self) -> Option<Event> {
        if editor::focus_open_editor() {
            return None;
        }
        let input = self.notif_input.as_ref()?;
        unsafe {
            let fg = GetForegroundWindow();
            if fg != self.hwnd {
                self.cli_prev = (!fg.is_invalid()).then_some(fg);
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
        let ok = input.focus() && unsafe { GetForegroundWindow() } == self.hwnd;
        if !ok {
            self.return_keyboard();
            return None;
        }
        (!self.state.notif_focused()).then_some(Event::NotifFocus(true))
    }

    /// Esc: the panel unlocks and the keyboard goes back.
    fn release_notif(&mut self) {
        self.return_keyboard();
        if self.state.notif_focused() {
            self.apply_now(Event::NotifFocus(false));
        }
    }

    /// A key the hidden input hands over: Up and Down move the selection
    /// (scrolling to keep it in view), Enter opens it, Delete removes it.
    pub(super) fn on_notif_key(&mut self, key: InputKey) {
        match key {
            InputKey::Up | InputKey::Down => {
                let items = self.notif.history.items();
                let at = self
                    .notif
                    .selected
                    .and_then(|id| items.iter().position(|e| e.id == id));
                let Some(to) = step_selection(at, items.len(), key == InputKey::Up) else {
                    return;
                };
                let id = items[to].id;
                let rows = self.notif_row_room();
                self.notif.selected = Some(id);
                self.notif.scroll = notif_scroll_to(to, self.notif.scroll, rows);
                self.notif.keyboard_held = true;
                self.redraw_notif();
            }
            InputKey::PageUp | InputKey::PageDown => {
                self.notif.keyboard_held = true;
                let rows = self.notif_row_room() as i32 * 120;
                self.notif_wheel(if key == InputKey::PageUp { rows } else { -rows });
            }
            InputKey::Enter => {
                if let Some(id) = self.notif_selected_id() {
                    self.perform_notif(NotifAction::Open(id));
                }
            }
            InputKey::Delete => {
                if let Some(id) = self.notif_selected_id() {
                    self.perform_notif(NotifAction::Remove(id));
                }
            }
            InputKey::Escape => self.release_notif(),
            InputKey::Blur => {
                // Another window took the keyboard (a click outside the
                // panel, Alt+Tab): no foreground to hand back.
                if self.state.notif_focused() && !self.notif_has_keyboard() {
                    self.cli_prev = None;
                    self.apply_now(Event::NotifFocus(false));
                }
            }
            InputKey::Tab | InputKey::Stop => {}
        }
    }

    /// The selected row, if it is listed.
    fn notif_selected_id(&self) -> Option<u64> {
        let id = self.notif.selected?;
        self.notif.history.get(id).map(|e| e.id)
    }

    /// The hidden input's text changed: it is only there to catch keys, so
    /// typed characters are dropped at once.
    pub(super) fn on_notif_changed(&mut self) {
        if let Some(input) = &self.notif_input
            && !input.text().is_empty()
        {
            input.set_text("");
        }
    }

    // ---- Settings → Fitur → Notifikasi ----

    /// A change in the Settings card.
    pub(super) fn on_notif_setting(&mut self, change: NotifSetting) {
        match change {
            NotifSetting::Record(on) => self.config.notifications.record = on,
            NotifSetting::Items(n) => self.config.notifications.max_items = clamp_cap(n),
            NotifSetting::Persist(on) => self.config.notifications.persist = on,
            // Both edit the file's list inside the save (`add_muted`), so
            // names edited by hand meanwhile stay.
            NotifSetting::MutedAdd(name) => {
                self.save_config_with(|c| config::add_muted(c, &name));
                return;
            }
            NotifSetting::MutedRemove(name) => {
                self.save_config_with(|c| config::remove_muted(c, &name));
                return;
            }
            NotifSetting::Clear => {
                self.clear_notif_history();
                return;
            }
        }
        // `adopt_config` (from the save) trims for a new cap, deletes or
        // writes the file for a new "Simpan setelah restart".
        self.save_config();
        self.clamp_notif_scroll();
        self.redraw_notif();
    }
}

/// A change in Settings → Fitur → Notifikasi.
#[derive(Clone, Debug, PartialEq)]
pub enum NotifSetting {
    Record(bool),
    Items(u32),
    Persist(bool),
    /// A name to add to the excluded apps.
    MutedAdd(String),
    /// An excluded app's name to take out of the list.
    MutedRemove(String),
    /// "Hapus riwayat notifikasi".
    Clear,
}

/// The logo the notch draws in a toast notice's badge: only for a notice made
/// from a real Windows toast (`toast: Some`) whose logo key is known. An
/// island-owned App notice, or a toast whose logo failed, gets `None` (the
/// letter badge).
pub(super) fn toast_logo_view<'a>(
    logos: &'a HashMap<String, Thumb>,
    view: &View,
) -> Option<ToastLogoView<'a>> {
    let View::Notice(Notice {
        kind: NoticeKind::App {
            app_id,
            toast: Some(_),
        },
        source,
        ..
    }) = view
    else {
        return None;
    };
    let (key, thumb) = logos.get_key_value(&logo_key(app_id.as_deref(), source))?;
    Some(ToastLogoView { key, thumb })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ToastOrigin;

    #[test]
    fn the_pointer_selects_the_row_it_is_over_or_a_button_of_it() {
        let ids = [10, 20, 30];
        // Nothing chosen: the first row is not lit either, so it is a change.
        assert_eq!(hover_target(&ids, None, Some(&Hit::NotifRow(10))), Some(10));
        assert_eq!(hover_target(&ids, None, Some(&Hit::NotifRow(30))), Some(30));
        // A button of another row selects that row.
        assert_eq!(
            hover_target(&ids, Some(10), Some(&Hit::NotifRemove(20))),
            Some(20)
        );
        assert_eq!(
            hover_target(&ids, Some(10), Some(&Hit::NotifMute(20))),
            Some(20)
        );
        assert_eq!(
            hover_target(&ids, Some(10), Some(&Hit::NotifOpen(30))),
            Some(30)
        );
        // Already selected: no change.
        assert_eq!(hover_target(&ids, Some(20), Some(&Hit::NotifRow(20))), None);
        assert_eq!(
            hover_target(&ids, Some(20), Some(&Hit::NotifOpen(20))),
            None
        );
        // A chosen row that is no longer listed counts as none.
        assert_eq!(
            hover_target(&ids, Some(99), Some(&Hit::NotifRow(10))),
            Some(10)
        );
        // Other targets, an unlisted row and no target keep the selection.
        assert_eq!(hover_target(&ids, Some(20), Some(&Hit::NotifClear)), None);
        assert_eq!(hover_target(&ids, Some(20), Some(&Hit::NotifFocus)), None);
        assert_eq!(hover_target(&ids, Some(20), Some(&Hit::NotifRow(77))), None);
        assert_eq!(hover_target(&ids, Some(20), None), None);
        assert_eq!(hover_target(&[], None, Some(&Hit::NotifRow(1))), None);
    }

    #[test]
    fn a_move_at_the_same_spot_is_ignored_and_the_keyboard_holds_the_selection() {
        let mut last = None;
        let mut held = false;
        assert!(pointer_moved(&mut last, (5, 5), &mut held), "first move");
        assert!(!pointer_moved(&mut last, (5, 5), &mut held), "same spot");
        // After a key move, a move needs a known spot and a new one.
        held = true;
        assert!(!pointer_moved(&mut last, (5, 5), &mut held));
        assert!(held);
        assert!(pointer_moved(&mut last, (6, 5), &mut held));
        assert!(!held, "a real move ends the hold");
        // With no known spot yet, the first move only records it.
        let (mut last, mut held) = (None, true);
        assert!(!pointer_moved(&mut last, (1, 1), &mut held));
        assert!(held);
    }

    #[test]
    fn up_and_down_step_through_the_list_and_stop_at_the_ends() {
        assert_eq!(step_selection(None, 4, false), Some(0));
        assert_eq!(step_selection(None, 4, true), Some(0));
        assert_eq!(step_selection(Some(0), 4, true), Some(0));
        assert_eq!(step_selection(Some(0), 4, false), Some(1));
        assert_eq!(step_selection(Some(3), 4, false), Some(3));
        assert_eq!(step_selection(Some(3), 4, true), Some(2));
        assert_eq!(step_selection(None, 0, false), None);
        assert_eq!(step_selection(Some(0), 0, true), None);
    }

    #[test]
    fn a_removed_row_hands_the_selection_to_the_one_that_takes_its_place() {
        let ids = [4, 3, 2, 1];
        assert_eq!(selection_after_remove(&ids, 3), Some(2));
        assert_eq!(selection_after_remove(&ids, 4), Some(3));
        assert_eq!(
            selection_after_remove(&ids, 1),
            Some(2),
            "the last: the one before"
        );
        assert_eq!(selection_after_remove(&[7], 7), None);
        assert_eq!(selection_after_remove(&ids, 9), None);
    }

    fn toast_notice(app_id: Option<&str>, source: &str, toast: Option<ToastOrigin>) -> View {
        View::Notice(Notice {
            key: "k".into(),
            source: source.into(),
            title: "t".into(),
            body: "b".into(),
            kind: NoticeKind::App {
                app_id: app_id.map(str::to_owned),
                toast,
            },
        })
    }

    fn logo_map() -> HashMap<String, Thumb> {
        let thumb = Thumb {
            w: 1,
            h: 1,
            bgra: vec![1, 2, 3, 255],
        };
        HashMap::from([
            ("5319.WhatsApp!App".to_owned(), thumb.clone()),
            ("Chrome".to_owned(), thumb),
        ])
    }

    #[test]
    fn a_toast_notice_finds_its_logo_by_the_logo_key() {
        let logos = logo_map();
        let origin = Some(ToastOrigin::default());
        let view = toast_notice(Some("5319.WhatsApp!App"), "WhatsApp", origin.clone());
        let found = toast_logo_view(&logos, &view).expect("logo");
        assert_eq!(found.key, "5319.WhatsApp!App");
        assert_eq!(found.thumb, &logos["5319.WhatsApp!App"]);
        // No AUMID: the display name keys it.
        let by_name = toast_notice(None, "Chrome", origin);
        assert_eq!(toast_logo_view(&logos, &by_name).expect("logo").key, "Chrome");
    }

    #[test]
    fn an_island_owned_app_notice_has_no_toast_logo() {
        let logos = logo_map();
        let own = toast_notice(Some("5319.WhatsApp!App"), "WhatsApp", None);
        assert_eq!(toast_logo_view(&logos, &own), None);
    }

    #[test]
    fn a_toast_whose_key_is_not_in_the_map_has_no_logo() {
        let logos = logo_map();
        let view = toast_notice(Some("Slack!App"), "Slack", Some(ToastOrigin::default()));
        assert_eq!(toast_logo_view(&logos, &view), None);
        assert_eq!(
            toast_logo_view(&HashMap::new(), &toast_notice(None, "Chrome", Some(ToastOrigin::default()))),
            None
        );
        assert_eq!(toast_logo_view(&logos, &View::Idle), None);
    }

    #[test]
    fn the_view_lights_only_a_listed_row_and_has_no_rows_without_access_to_scroll() {
        let mut notif = NotifState::default();
        let a = notif.history.push(100, "WhatsApp", None, None, "Hi", "yo");
        notif.access = true;
        notif.selected = Some(a);
        let view = notif_view(&notif, NotifSettings::default(), false);
        assert_eq!(view.selected, Some(a));
        assert!(!view.no_access && view.record && !view.paused);
        notif.selected = Some(a + 5);
        assert_eq!(
            notif_view(&notif, NotifSettings::default(), true).selected,
            None
        );
        assert!(notif_view(&notif, NotifSettings::default(), true).armed);
        notif.access = false;
        assert!(notif_view(&notif, NotifSettings::default(), false).no_access);
    }
}
