//! The island's side of the sticky note list (`crate::widgets::list_win`): it
//! opens the window (the hotkey row, the tray item, or the list button in a
//! note's header), builds the data it shows, and applies what the user did in
//! it (show a note, delete one, undo, make a new one). A child of `app`, so it
//! works on `App` directly.
//!
//! Power: nothing here runs while the list is closed. The data is built only
//! while the window is open (`note_list_refresh` does nothing otherwise), and
//! the list's one timer (`TIMER_LIST_UNDO`, 49) lives on its own window.

use std::time::Instant;

use chrono::Local;

use super::App;
use crate::capture::Rect as Px;
use crate::i18n;
use crate::notch_monitor;
use crate::widgets::apply::{apply, settle};
use crate::widgets::host::WidgetEvent;
use crate::widgets::list::{self, Undo};
use crate::widgets::list_win::{self, Anchor, ListData, ListEvent};
use crate::widgets::sticky::can_add;
use crate::widgets::templates::Template;

impl App {
    /// Opens the list: beside note `beside` (its list button was pressed),
    /// else at the top right of the notch's monitor. Open already: it moves
    /// there and takes the keyboard (never a second window).
    pub(super) fn open_note_list(&mut self, beside: Option<&str>) {
        let anchor = self.note_list_anchor(beside);
        let data = self.note_list_data();
        list_win::open(self.hwnd, anchor, data);
    }

    /// Where the window goes: the note's window on the desktop when it has
    /// one, else the notch monitor's work area (the monitor `sticky_for` puts
    /// a new note on).
    fn note_list_anchor(&self, beside: Option<&str>) -> Anchor {
        let note = beside.and_then(|id| {
            self.widgets
                .host
                .as_ref()
                .and_then(|host| host.note_rect_px(id))
        });
        if let Some(note_px) = note {
            return Anchor::Beside { note_px };
        }
        let work = notch_monitor::resolve(&self.config.notch_monitor, &self.displays)
            .and_then(|i| self.displays.get(i))
            .map_or(Px::new(0, 0, 1920, 1040), |d| d.monitor.work);
        Anchor::Default { work_px: work }
    }

    /// What the window shows now: a copy of the notes, built only while it is
    /// open.
    fn note_list_data(&self) -> ListData {
        let room = can_add(self.notes.sticky_count());
        ListData {
            notes: self.notes.list().to_vec(),
            lang: i18n::lang(),
            ui: self.config.font,
            can_add: room,
            can_undo: self.widgets.list_undo.is_some() && room,
        }
    }

    /// The notes (or the cap, the language, the font) changed: the open list
    /// follows. Nothing when it is closed.
    pub(super) fn note_list_refresh(&mut self) {
        if list_win::is_open() {
            let data = self.note_list_data();
            list_win::refresh(data);
        }
    }

    /// What the user did in the list since the last call.
    pub(super) fn note_list_events(&mut self) {
        for ev in list_win::drain() {
            match ev {
                ListEvent::Show(id) => self.note_list_show(&id),
                ListEvent::Delete(id) => self.note_list_delete(&id),
                ListEvent::Undo => self.note_list_undo(),
                ListEvent::New => {
                    // The new note takes the keyboard, so the list closes by
                    // being deactivated.
                    self.new_sticky(None, Template::Kosong);
                }
                // The undo is final: what only it kept (a deleted note's
                // images) can go now.
                ListEvent::Closed | ListEvent::UndoExpired => {
                    self.widgets.list_undo = None;
                    self.widgets_images_release();
                }
            }
        }
    }

    /// "Show and focus": the note comes out of its tuck, unrolls, comes to the
    /// top of its stack, goes to the front and takes the keyboard; then the
    /// list closes.
    fn note_list_show(&mut self, id: &str) {
        // The notes may be hidden (Settings, "Tampilkan").
        self.widgets_reveal();
        let events = list::show_events(self.notes.list(), id);
        if !events.is_empty() {
            let out = apply(&mut self.notes, events, Local::now().timestamp());
            if out.changed {
                self.arm_widget_save();
            }
        }
        self.widgets_sync();
        if let Some(host) = self.widgets.host.as_mut() {
            host.bring_front(id);
        }
        list_win::close();
    }

    /// A delete from the list: the snapshot for "Urungkan" is taken first,
    /// then the note goes the way every other delete does.
    fn note_list_delete(&mut self, id: &str) {
        self.widgets.list_undo = Undo::capture(self.notes.list(), id);
        let structural =
            self.widgets_apply_list(vec![WidgetEvent::Delete { id: id.to_owned() }], true);
        if structural {
            self.widgets_sync();
        }
        self.note_list_refresh();
    }

    /// "Urungkan": the deleted note comes back (a stack sheet rejoins its
    /// stack). Refused on a full desktop: the snapshot is kept.
    fn note_list_undo(&mut self) {
        let Some(undo) = self.widgets.list_undo.clone() else {
            return;
        };
        if undo.restore(&mut self.notes) {
            self.widgets.list_undo = None;
            settle(&mut self.notes, Local::now().timestamp(), false);
            self.arm_widget_save();
            self.widgets_sync();
            // The Notes page shows the same notes: it takes the restored one back.
            if matches!(self.state.view(), crate::model::View::Panel(crate::model::Page::Notes)) {
                self.draw(Instant::now());
            }
            self.sync_settings();
        } else {
            self.note_list_refresh();
        }
    }
}
