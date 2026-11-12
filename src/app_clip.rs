//! The island's side of the clipboard history: the listener, reading a
//! change into the history, copying an entry back, the encrypted file, the
//! Clipboard page's search field and selection. A child of
//! `app`, so it works on `App` directly.
//!
//! Power: no repeating timer. A clipboard change while the page is hidden
//! redraws nothing (unless a notice shows); saves are one one-shot after a
//! change; a busy clipboard is retried with a short one-shot.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Local};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_NONE};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, KillTimer, SetForegroundWindow, SetTimer,
};

use super::shelf_page::{DRAG_START_DIP, past_threshold, primary_down};
use super::shot::ShotArrival;
use super::{App, WM_APP_EVENT, client_xy, poster};
use crate::clipboard::drag::{self, DragFiles};
use crate::clipboard::notice::{
    NoticeCtx, NoticeGate, PREVIEW_CHARS, clip_notice_lines, clip_notice_wanted,
};
use crate::clipboard::persist::{self, Write};
use crate::clipboard::pack;
use crate::clipboard::store::{self, ClipSnapshot, Ingested, Packed, ingest};
use crate::clipboard::win::{self, Read};
use crate::clipboard::{Clip, ClipData, History};
use crate::config::AfterCapture;
use crate::i18n::{self, Key, tr};
use crate::model::{
    CLIP_IMAGE_KEY, CLIP_PREVIEW_KEY, ClipAction, Event, Hit, Notice, NoticeKind, Page, View,
};
use crate::ui::cli_input::{CliInput, Key as InputKey};
use crate::ui::editor;
use crate::ui::geometry::{ShapeKind, shape_for};
use crate::ui::panel_layout::{clip_max_scroll, clip_rows, clip_scroll_to, clip_wheel_step};
use crate::ui::render::{ClipNoticeView, ClipView};

/// One-shot: the clipboard was busy; read it again. (17 and 18 are the
/// System page's timers in `app`.)
pub(super) const TIMER_CLIP_RETRY: usize = 20;
/// One-shot: write the history file once changes settle.
pub(super) const TIMER_CLIP_SAVE: usize = 21;
/// One-shot: the hotkey's panel (the Clipboard's or the Devices') was not
/// visited; it closes.
pub(super) const TIMER_CLIP_HOLD: usize = 22;
/// One-shot, set only while a dragged-out temp file exists: the nearest
/// one's time has come (`drag::KEEP` after its last drag).
pub(super) const TIMER_CLIP_DRAG: usize = 55;
const RETRY_MS: u32 = 60;
const RETRIES: u8 = 4;
const SAVE_MS: u32 = 1500;
const HOLD_MS: u32 = 8000;
/// Characters the search field takes.
pub(super) const SEARCH_LIMIT: usize = 256;

/// The history and the page's state that is not the reducer's.
#[derive(Default)]
pub(super) struct ClipState {
    pub history: History,
    /// The sequence number after the island's own last write.
    pub own_sequence: Option<u32>,
    /// `AddClipboardFormatListener` is on.
    pub listening: bool,
    /// Reads of a busy clipboard tried so far for this change.
    pub retries: u8,
    /// `TIMER_CLIP_SAVE` is set: a write is due.
    pub save_due: bool,
    /// The chosen card (an entry id); `None`: the first one shown.
    pub selected: Option<u64>,
    /// Rows of cards scrolled.
    pub scroll: usize,
    /// The search text as last seen.
    pub query: String,
    /// `clip_start` ran (before it a config read leaves the history alone).
    pub started: bool,
    /// Where the last mouse move over the page was (client pixels).
    pub pointer: Option<(i32, i32)>,
    /// The keyboard moved the selection: the pointer selects nothing until it
    /// really moves (`pointer_moved`).
    pub keyboard_held: bool,
    /// The entry the copy preview announced last, and when (`NoticeGate`).
    pub gate: NoticeGate,
    /// The card each screenshot editor's auto-copy made (one per capture).
    pub captures: store::CaptureCards,
    /// A left press on image card `id` at this client point: it may become
    /// a drag out.
    pub press: Option<(u64, i32, i32)>,
    /// The temporary PNGs handed to drags, and when each may go.
    pub drag_files: DragFiles,
}

/// An image card pulled off the page: the file the drag carries, and whether
/// it is only the small preview (the original is not kept).
pub(super) struct ClipDragOut {
    pub path: PathBuf,
    pub small: bool,
    pub screenshot: bool,
}

/// The card a left press on `hit` may drag out: only an image card itself,
/// never its buttons nor another kind of card.
fn drag_press_target(hit: Option<&Hit>, history: &History) -> Option<u64> {
    let Some(Hit::ClipCard(id)) = hit else {
        return None;
    };
    history.get(*id)?.image().map(|_| *id)
}

/// After a drop of a card whose original is no longer kept: the same body as
/// the copy's "Disalin", under "Ditarik".
fn dragged_notice(screenshot: bool) -> Notice {
    Notice {
        key: "clip-dragged".into(),
        source: tr(Key::ClipboardTitle).into(),
        title: tr(Key::ClipDragged).into(),
        body: i18n::clip_image_small(i18n::lang(), screenshot),
        kind: NoticeKind::Copied,
    }
}

/// What the island does with a clipboard change that an editor of ours wrote
/// (`shot_edit::own_copy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OwnRoute {
    /// Not an editor write, or one from an editor with auto-copy off: the
    /// normal path (history card, copy notice, screenshot preview rules).
    Normal,
    /// An auto-copying editor's write: one card per capture, and no notice,
    /// no "Disalin" and no screenshot preview.
    Managed { capture: u64 },
}

pub(super) fn own_route(own: Option<crate::shot_edit::OwnCopy>) -> OwnRoute {
    match own {
        Some(o) if o.managed => OwnRoute::Managed { capture: o.capture },
        _ => OwnRoute::Normal,
    }
}

/// Whether a mouse move at `at` is the pointer really moving, `last` the
/// move before (updated here). Windows also sends a move when the content
/// under a still pointer changes (a scroll, a redraw): the same spot is no
/// move. After the keyboard chose a card (`held`), a move only counts once
/// the pointer has a known spot and leaves it; that ends the hold.
pub(super) fn pointer_moved(
    last: &mut Option<(i32, i32)>,
    at: (i32, i32),
    held: &mut bool,
) -> bool {
    let before = last.replace(at);
    if before == Some(at) {
        return false;
    }
    if *held {
        if before.is_none() {
            return false;
        }
        *held = false;
    }
    true
}

/// The card to keep selected after the cards scrolled to row `scroll` with
/// `rows` rows shown: card `at` (of `n`, two a row) if it still shows, else
/// the card in its column on the nearest row that does. So Enter never
/// copies a card scrolled out of sight.
pub(super) fn visible_card(at: usize, scroll: usize, rows: usize, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let last_row = scroll + rows.max(1) - 1;
    let row = (at / 2).clamp(scroll, last_row);
    (row * 2 + at % 2).min(n - 1)
}

/// The entries the page lists for `query`, newest first.
fn shown<'a>(history: &'a History, query: &str) -> Vec<&'a Clip> {
    history.search(query, |k| tr(i18n::clip_kind_key(k)))
}

/// The selected card among `items`: the chosen one while it is listed,
/// else the first.
pub(super) fn effective(items: &[&Clip], chosen: Option<u64>) -> Option<u64> {
    chosen
        .filter(|id| items.iter().any(|c| c.id == *id))
        .or_else(|| items.first().map(|c| c.id))
}

/// The card the pointer moved onto, when it is not the selected one: the
/// card under it or one of its buttons, listed in `ids`. `None` keeps the
/// selection (the pointer is elsewhere, or already on the selected card).
pub(super) fn hover_target(ids: &[u64], chosen: Option<u64>, hit: Option<&Hit>) -> Option<u64> {
    let id = match hit? {
        Hit::ClipCard(id) | Hit::ClipPin(id) | Hit::ClipPlain(id) | Hit::ClipDelete(id) => *id,
        _ => return None,
    };
    let current = chosen.filter(|c| ids.contains(c)).or(ids.first().copied());
    (ids.contains(&id) && current != Some(id)).then_some(id)
}

/// What the Clipboard page draws, from the parts of `App` it needs (the
/// renderer is borrowed mutably meanwhile).
pub(super) fn clip_view<'a>(
    clip: &'a ClipState,
    enabled: bool,
    query: &'a str,
    selection: (usize, usize),
    focused: bool,
) -> ClipView<'a> {
    let items = shown(&clip.history, query);
    let selected = effective(&items, clip.selected);
    ClipView {
        enabled,
        total: clip.history.len(),
        items,
        query,
        selection,
        focused,
        selected,
        scroll: clip.scroll,
        now: Local::now().naive_local(),
    }
}

/// The "Disalin" notice for `clip`: its one-line preview.
fn copied_notice(clip: &Clip) -> Notice {
    let lang = i18n::lang();
    let body = match &clip.data {
        ClipData::Text(t) => store::preview(t, PREVIEW_CHARS),
        ClipData::Files(paths) => {
            let first = paths
                .first()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            i18n::clip_files(lang, paths.len(), &first)
        }
        // Only the thumbnail is left: it pastes small, and says so.
        ClipData::Image(img) if !img.has_original() => {
            i18n::clip_image_small(lang, img.screenshot)
        }
        ClipData::Image(img) => i18n::clip_image_meta(lang, img.screenshot, img.width, img.height),
    };
    Notice {
        key: "clip-copied".into(),
        source: tr(Key::ClipboardTitle).into(),
        title: tr(Key::ClipCopied).into(),
        body,
        kind: NoticeKind::Copied,
    }
}

/// The copy preview for `clip`: text lines, "Gambar disalin" with its size,
/// or "Berkas disalin" with the files. Never logged.
pub(super) fn clip_preview_notice(clip: &Clip) -> Notice {
    let lang = i18n::lang();
    let (title, body, key) = match &clip.data {
        ClipData::Text(t) => {
            let (first, second) = clip_notice_lines(t);
            let body =
                second.unwrap_or_else(|| i18n::clip_text_meta(lang, clip.kind, t.chars().count()));
            (first, body, CLIP_PREVIEW_KEY)
        }
        ClipData::Files(paths) => {
            let first = paths
                .first()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (
                tr(Key::ClipPreviewFiles).to_owned(),
                i18n::clip_files(lang, paths.len(), &first),
                CLIP_PREVIEW_KEY,
            )
        }
        ClipData::Image(img) => {
            let title = if img.screenshot {
                tr(Key::ShotTitle)
            } else {
                tr(Key::ClipPreviewImage)
            };
            (
                title.to_owned(),
                i18n::clip_preview_size(img.width, img.height),
                CLIP_IMAGE_KEY,
            )
        }
    };
    Notice {
        key: key.into(),
        source: tr(Key::ClipboardTitle).into(),
        title,
        body,
        kind: NoticeKind::ClipPreview { id: clip.id },
    }
}

/// The thumbnail the frame draws in a copy preview's badge spot: only for an
/// image entry still in the history (else the Clipboard badge shows).
pub(super) fn clip_notice_view<'a>(
    history: &'a History,
    view: &View,
) -> Option<ClipNoticeView<'a>> {
    let View::Notice(Notice {
        kind: NoticeKind::ClipPreview { id },
        ..
    }) = view
    else {
        return None;
    };
    let img = history.get(*id)?.image()?;
    Some(ClipNoticeView {
        id: *id,
        thumb: &img.thumb,
    })
}

impl App {
    /// At startup: the saved history (when kept) and the listener.
    pub(super) fn clip_start(&mut self) {
        // Files a crash left behind from earlier drags.
        drag::sweep(&drag::root());
        let cfg = self.config.clipboard;
        let items = persist::start_items(&persist::path(), cfg.persist);
        self.clip.history = History::from_items(cfg.max_items, items);
        crate::shot_edit::set_auto_copy(self.config.screenshot.auto_copy);
        crate::capture::set_after_capture(
            self.config.screenshot.after_capture == AfterCapture::Inline,
        );
        self.sync_clip_listener();
        self.clip.started = true;
    }

    /// config.json changed (a save, or a hand edit picked up): the cap and the
    /// listener follow, and the screenshot editor's "Salin otomatis" (latched
    /// by each editor when it opens). Nothing else before `clip_start`.
    pub(super) fn clip_follow_config(&mut self) {
        crate::shot_edit::set_auto_copy(self.config.screenshot.auto_copy);
        crate::capture::set_after_capture(
            self.config.screenshot.after_capture == AfterCapture::Inline,
        );
        if !self.clip.started {
            return;
        }
        self.clip.history.set_cap(self.config.clipboard.max_items);
        self.clip_schedule_packing();
        self.sync_clip_listener();
    }

    /// The listener runs while "Riwayat clipboard" or the screenshot
    /// preview is on.
    pub(super) fn sync_clip_listener(&mut self) {
        let want = self.config.clipboard.enabled || self.config.screenshot.preview;
        if want && !self.clip.listening {
            self.clip.listening = win::add_listener(self.hwnd);
        } else if !want && self.clip.listening {
            win::remove_listener(self.hwnd);
            self.clip.listening = false;
        }
    }

    /// `WM_DESTROY`: the listener goes; a pending save is written now; the
    /// dragged-out temp files go.
    pub(super) fn clip_shutdown(&mut self) {
        self.clip_drag_clear_all();
        if self.clip.listening {
            win::remove_listener(self.hwnd);
            self.clip.listening = false;
        }
        if std::mem::take(&mut self.clip.save_due) && self.config.clipboard.persist {
            persist::write_now(
                &persist::path(),
                Write::Save(self.clip.history.items().to_vec()),
            );
        }
    }

    /// `WM_CLIPBOARDUPDATE`: the change goes into the history. The island's
    /// own copies add nothing; a skipped (password manager) change stores and
    /// says nothing. Only a visible page or a notice redraws.
    pub(super) fn on_clipboard_update(&mut self) {
        self.clip.retries = 0;
        self.read_clipboard();
    }

    fn read_clipboard(&mut self) {
        let history = self.config.clipboard.enabled;
        if !history && !self.config.screenshot.preview {
            return;
        }
        let snap: ClipSnapshot = match win::read(self.hwnd, self.clip.own_sequence) {
            Read::Own => return,
            Read::Busy => {
                if self.clip.retries < RETRIES {
                    self.clip.retries += 1;
                    unsafe {
                        SetTimer(Some(self.hwnd), TIMER_CLIP_RETRY, RETRY_MS, None);
                    }
                }
                return;
            }
            Read::Snapshot(snap) => snap,
        };
        let sequence = snap.sequence;
        // A write of a screenshot editor that auto-copies: one card per
        // capture and nothing said (the owner did not copy anything).
        if let OwnRoute::Managed { capture } = own_route(crate::shot_edit::own_copy(sequence)) {
            if history {
                self.keep_capture(snap, capture);
            }
            return;
        }
        // The history off, the preview on: only an image is looked at, by
        // the history's rules, and nothing is kept.
        if !history {
            if let Some(img) = store::image_only(snap)
                && self.shot_from_image(None, &img, sequence).raised()
            {
                self.refresh(Instant::now());
            }
            return;
        }
        let at = Local::now().timestamp();
        let outcome = ingest(&mut self.clip.history, snap, at);
        let (id, arrival) = match outcome {
            Ingested::Skipped | Ingested::Nothing => return,
            Ingested::Added { id, image: true } => (id, self.clip_image_arrived(id, sequence)),
            Ingested::Added { id, image: false } => (id, ShotArrival::Nothing),
            // The same screenshot again (PrtSc twice on an unchanged
            // screen): one card, moved up, and a preview of it.
            Ingested::Bumped { id } => {
                let again = self
                    .clip
                    .history
                    .get(id)
                    .and_then(Clip::image)
                    .is_some_and(|i| i.screenshot);
                let arrival = if again {
                    self.clip_image_arrived(id, sequence)
                } else {
                    ShotArrival::Nothing
                };
                (id, arrival)
            }
        };
        self.clip_schedule_packing();
        self.schedule_clip_save();
        // The preview took the notice's place ("Disalin" would only be
        // replaced by it); an update of a capture Simpan or Hapus already
        // answered says nothing over that answer's notice.
        if arrival.raised() {
            self.refresh(Instant::now());
            return;
        }
        if !arrival.copy_feedback_due() {
            self.redraw_clip();
            return;
        }
        let notice = self.copy_feedback(id, outcome);
        match notice {
            Some(n) => self.apply_now(Event::Notice(n)),
            None => self.redraw_clip(),
        }
    }

    /// An auto-copy of capture `capture`: its card is replaced by the new
    /// picture (`store::ingest_replacing`), quietly.
    fn keep_capture(&mut self, snap: ClipSnapshot, capture: u64) {
        let at = Local::now().timestamp();
        let old = self.clip.captures.card(capture);
        let outcome = store::ingest_replacing(&mut self.clip.history, snap, at, old);
        let (Ingested::Added { id, .. } | Ingested::Bumped { id }) = outcome else {
            return;
        };
        self.clip.captures.set(capture, id);
        self.clip_schedule_packing();
        self.schedule_clip_save();
        self.redraw_clip();
    }

    /// What an external copy that went into the history says: the preview
    /// ("Notifikasi salinan", which then supersedes "Disalin untuk semua
    /// salinan" even when a rule keeps it quiet), else "Disalin" with that
    /// setting, else nothing. A copy made from the island never gets here
    /// (`Read::Own`); its "Disalin" is the card's own.
    fn copy_feedback(&mut self, id: u64, outcome: Ingested) -> Option<Notice> {
        let cfg = self.config.clipboard;
        if cfg.copy_notice {
            let ctx = NoticeCtx {
                setting: true,
                history: cfg.enabled,
                outcome,
                own: false,
                // The preview's own case never gets here (`ShotArrival`).
                shot: false,
                clip_page_open: self.state.view() == View::Panel(Page::Clipboard),
                fullscreen: self.state.fullscreen(),
            };
            let wanted = clip_notice_wanted(&ctx) && self.clip.gate.pass(id, Instant::now());
            return wanted
                .then(|| self.clip.history.get(id).map(clip_preview_notice))
                .flatten();
        }
        cfg.copied_feedback_all
            .then(|| self.clip.history.get(id).map(copied_notice))
            .flatten()
    }

    /// A new image entry `id` arrived (a screenshot, a copied picture): it
    /// becomes the screenshot preview's candidate (`app_shot`). True when
    /// the preview went up now (`ShotArrival`).
    fn clip_image_arrived(&mut self, id: u64, sequence: u32) -> ShotArrival {
        let Some(img) = self.clip.history.get(id).and_then(Clip::image).cloned() else {
            return ShotArrival::Nothing;
        };
        self.shot_from_image(Some(id), &img, sequence)
    }

    /// Redraws the page if it is on screen.
    pub(super) fn redraw_clip(&mut self) {
        if self.state.view() == View::Panel(Page::Clipboard) && !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// The file follows a change once, `SAVE_MS` later (a burst of copies
    /// writes once). Nothing with "hanya di memori".
    pub(super) fn schedule_clip_save(&mut self) {
        if !self.config.clipboard.persist {
            return;
        }
        self.clip.save_due = true;
        // Replaces a pending one: the last change sets the time.
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_CLIP_SAVE, SAVE_MS, None);
        }
    }

    pub(super) fn on_clip_timer(&mut self, id: usize) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), id);
        }
        match id {
            TIMER_CLIP_RETRY => self.read_clipboard(),
            TIMER_CLIP_SAVE => {
                if std::mem::take(&mut self.clip.save_due) && self.config.clipboard.persist {
                    let items = self.clip.history.items().to_vec();
                    persist::write_in_background(persist::path(), Write::Save(items));
                }
            }
            TIMER_CLIP_HOLD if self.state.hotkey_held() => self.apply_now(Event::ClipHoldEnd),
            TIMER_CLIP_DRAG => self.clip_drag_expire(),
            _ => {}
        }
    }

    /// `WM_HOTKEY`: the page opens (or closes) without taking the foreground.
    pub(super) fn on_clip_hotkey(&mut self) {
        self.apply_now(Event::ClipHotkey);
        self.arm_hotkey_hold();
    }

    /// A page hotkey (the Clipboard's or the Devices') opened the panel held
    /// open: the hold ends after `HOLD_MS` unless the pointer comes
    /// (`TIMER_CLIP_HOLD` is shared, the reducer has one hold).
    pub(super) fn arm_hotkey_hold(&mut self) {
        if self.state.hotkey_held() {
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_CLIP_HOLD, HOLD_MS, None);
            }
        }
    }

    /// The screen locked: with "Hapus saat layar dikunci", unpinned entries
    /// go from memory and the file. Pins stay. Every dragged-out temp file
    /// goes too.
    pub(super) fn clip_on_lock(&mut self) {
        if !self.config.clipboard.clear_on_lock {
            return;
        }
        self.clip_drag_clear_all();
        if self.clip.history.clear_unpinned() {
            self.clip_schedule_packing();
            self.clip.selected = None;
            self.clip.scroll = 0;
            self.write_clip_file_now();
        }
    }

    /// The file (or its removal) now, in the background: after a clear.
    fn write_clip_file_now(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CLIP_SAVE);
        }
        self.clip.save_due = false;
        if self.config.clipboard.persist {
            let items = self.clip.history.items().to_vec();
            persist::write_in_background(persist::path(), Write::Save(items));
        }
    }

    /// A card's click or button, from the reducer.
    pub(super) fn perform_clip(&mut self, action: ClipAction) -> Option<Event> {
        match action {
            ClipAction::Copy(id) => return self.copy_clip(id, false),
            ClipAction::CopyPlain(id) => return self.copy_clip(id, true),
            ClipAction::TogglePin(id) => {
                let pinned = self.clip.history.get(id).is_some_and(|c| c.pinned);
                self.clip.selected = Some(id);
                if self.clip.history.set_pinned(id, !pinned) {
                    self.schedule_clip_save();
                }
            }
            ClipAction::Delete(id) => {
                // The selection moves to the card that takes its place.
                let items = shown(&self.clip.history, &self.clip.query);
                let next = items
                    .iter()
                    .position(|c| c.id == id)
                    .and_then(|i| {
                        items
                            .get(i + 1)
                            .or_else(|| i.checked_sub(1).and_then(|j| items.get(j)))
                    })
                    .map(|c| c.id);
                if self.clip.history.delete(id) {
                    self.clip.selected = next;
                    self.clip_schedule_packing();
                    self.clamp_clip_scroll();
                    self.schedule_clip_save();
                    self.clip_drag_forget_missing();
                }
            }
        }
        self.redraw_clip();
        None
    }

    /// Puts entry `id` on the clipboard (as text only with `plain`); it moves
    /// to the front, no duplicate, and "Disalin" shows.
    fn copy_clip(&mut self, id: u64, plain: bool) -> Option<Event> {
        let clip = self.clip.history.get(id)?;
        let packed = !plain && clip.image().is_some_and(|i| i.packed().is_some());
        let mut formats = win::formats_for(clip, plain);
        let sequence = win::write(self.hwnd, &formats)?;
        self.clip.own_sequence = Some(sequence);
        let mut notice = copied_notice(clip);
        self.clip.history.bump(id, Local::now().timestamp());
        if packed {
            // The entry is the newest image now: keep what was decoded for
            // the paste as its raw original (it must be as long as the
            // original, else the pack did not decode and the paste was the
            // thumbnail's).
            let decoded = formats.pop().map(|(_, dib)| Arc::new(dib));
            if !decoded.is_some_and(|dib| self.clip.history.set_raw(id, dib)) {
                self.clip.history.forget_original(id);
                if let Some(c) = self.clip.history.get(id) {
                    notice = copied_notice(c);
                }
            }
        }
        self.clip_schedule_packing();
        self.clip.selected = Some(id);
        self.clip.scroll = 0;
        self.schedule_clip_save();
        Some(Event::Notice(notice))
    }

    /// A left press: on an image card it may become a drag out.
    pub(super) fn clip_press(&mut self, hit: Option<&Hit>, lparam: LPARAM) {
        self.clip.press = drag_press_target(hit, &self.clip.history).map(|id| {
            let (x, y) = client_xy(lparam);
            (id, x, y)
        });
    }

    pub(super) fn clip_release(&mut self) {
        self.clip.press = None;
    }

    /// The pointer moved with an image card pressed: past `DRAG_START_DIP`
    /// with the primary button still down and the Clipboard page open, it is
    /// a drag out, and the temporary PNG to carry comes back (the caller
    /// starts `DoDragDrop` outside the app borrow). The click that would have
    /// copied the card is dropped. The history does not change: no bump, no
    /// original kept from the unpack, nothing saved.
    pub(super) fn clip_drag_start(
        &mut self,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<ClipDragOut> {
        let (id, x0, y0) = self.clip.press?;
        if !primary_down(wparam) || self.state.view() != View::Panel(Page::Clipboard) {
            self.clip.press = None;
            return None;
        }
        let min = DRAG_START_DIP * self.gesture_scale();
        if !past_threshold((x0, y0), client_xy(lparam), min) {
            return None;
        }
        self.clip.press = None;
        self.click.clear();
        let clip = self.clip.history.get(id)?;
        let img = clip.image()?;
        let (small, screenshot) = (!img.has_original(), img.screenshot);
        let kept = self.clip.drag_files.path(id).map(PathBuf::from);
        let path = match kept {
            // Dragged again within `KEEP`: the same file, no new encode.
            Some(p) if p.exists() => p,
            other => {
                // A kept entry whose file went is written again in its place.
                let path = other.unwrap_or_else(|| {
                    let lang = i18n::lang();
                    let what = if screenshot {
                        "Screenshot"
                    } else {
                        i18n::text(lang, Key::ClipKindImage)
                    };
                    let at = DateTime::from_timestamp(clip.at, 0)
                        .map(|t| t.with_timezone(&Local).naive_local())
                        .unwrap_or_else(|| Local::now().naive_local());
                    drag::card_path(&drag::root(), id, &drag::file_name(what, at))
                });
                // The picture as a paste would give it: the original (packed
                // ones decoded here, on the UI thread), else the thumbnail.
                let (_, dib) = win::formats_for(clip, false).pop()?;
                if !drag::write_png(&dib, &path) {
                    return None;
                }
                path
            }
        };
        let now = Instant::now();
        self.clip.drag_files.touch(id, path.clone(), now);
        self.arm_clip_drag_timer(now);
        Some(ClipDragOut {
            path,
            small,
            screenshot,
        })
    }

    /// `DoDragDrop` returned (`effect` is what it reported, `None` when the
    /// drag never started): the shelf's after-drag rules, and a small
    /// preview that was dropped somewhere says so, as its copy would.
    pub(super) fn clip_after_drag_out(&mut self, out: &ClipDragOut, effect: Option<DROPEFFECT>) {
        self.clip.press = None;
        self.drag_out_ended();
        if out.small && effect.is_some_and(|e| e != DROPEFFECT_NONE) {
            self.apply_now(Event::Notice(dragged_notice(out.screenshot)));
        }
    }

    /// `TIMER_CLIP_DRAG`: the files whose time has come go.
    pub(super) fn clip_drag_expire(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CLIP_DRAG);
        }
        let now = Instant::now();
        let due = self.clip.drag_files.expire(now);
        self.clip_drag_remove(due, now);
    }

    /// Cards that left the history (a delete, "Hapus riwayat") take their
    /// temp files with them.
    pub(super) fn clip_drag_forget_missing(&mut self) {
        if self.clip.drag_files.is_empty() {
            return;
        }
        let live: Vec<u64> = self.clip.history.items().iter().map(|c| c.id).collect();
        let gone = self.clip.drag_files.retain_ids(&live);
        self.clip_drag_remove(gone, Instant::now());
    }

    /// Every temp file goes (exit, the lock with "Hapus saat layar dikunci").
    pub(super) fn clip_drag_clear_all(&mut self) {
        let all = self.clip.drag_files.take_all();
        self.clip_drag_remove(all, Instant::now());
    }

    /// Removes `files`; one still in use is tried again after `drag::RETRY`.
    /// The timer then follows what is left.
    fn clip_drag_remove(&mut self, files: Vec<(u64, PathBuf)>, now: Instant) {
        for (id, path) in files {
            if !drag::remove(&path) {
                self.clip.drag_files.retry(id, path, now);
            }
        }
        self.arm_clip_drag_timer(now);
    }

    /// One one-shot for the nearest deadline while any file is kept; none
    /// once there is nothing left.
    fn arm_clip_drag_timer(&mut self, now: Instant) {
        unsafe {
            match self.clip.drag_files.next_due(now) {
                Some(wait) => {
                    let ms = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
                    SetTimer(Some(self.hwnd), TIMER_CLIP_DRAG, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_CLIP_DRAG);
                }
            }
        }
    }

    /// Hands the originals that are not the newest image's to one short-lived
    /// worker thread (never the UI thread) to pack; each answer comes back as
    /// `Event::ClipPacked`. Called after every change that can make another
    /// image the newest.
    pub(super) fn clip_schedule_packing(&mut self) {
        let jobs = self.clip.history.pack_jobs();
        if jobs.is_empty() {
            return;
        }
        let keys: Vec<(u64, u64)> = jobs.iter().map(|j| (j.id, j.fingerprint)).collect();
        let (tx, wake) = (self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        let spawned = std::thread::Builder::new()
            .name("clip-pack".into())
            .spawn(move || {
                let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
                for job in jobs {
                    let packed = pack::pack(&job.raw).map(Arc::new);
                    let (id, fingerprint) = (job.id, job.fingerprint);
                    // The big picture is not held while the next one packs.
                    drop(job);
                    if tx
                        .send(Event::ClipPacked {
                            id,
                            fingerprint,
                            packed,
                        })
                        .is_ok()
                    {
                        wake();
                    }
                }
                if inited {
                    unsafe { CoUninitialize() };
                }
            });
        if spawned.is_err() {
            // No thread: they stay raw.
            for (id, fingerprint) in keys {
                self.clip.history.set_packed(id, fingerprint, None);
            }
        }
    }

    /// A worker's answer: the entry swaps its raw original for the packed
    /// one if it is still waiting for it. Nothing to draw or save (originals
    /// are never saved).
    pub(super) fn on_clip_packed(&mut self, id: u64, fingerprint: u64, packed: Option<Arc<Packed>>) {
        self.clip.history.set_packed(id, fingerprint, packed);
    }

    pub(super) fn clamp_clip_scroll(&mut self) {
        let n = shown(&self.clip.history, &self.clip.query).len();
        let max = clip_max_scroll(n, self.clip_rows());
        self.clip.scroll = self.clip.scroll.min(max);
    }

    fn clip_rows(&self) -> usize {
        let s = shape_for(self.config.style, ShapeKind::Panel);
        clip_rows(s, self.state.waiting_strip().is_some())
    }

    /// The wheel over the cards: a row per notch. False when there is
    /// nothing to scroll (the cards all fit): the caller turns the page.
    pub(super) fn clip_wheel(&mut self, delta: i32) -> bool {
        let n = shown(&self.clip.history, &self.clip.query).len();
        let max = clip_max_scroll(n, self.clip_rows());
        let Some(to) = clip_wheel_step(delta, self.clip.scroll, max) else {
            return false;
        };
        if to != self.clip.scroll {
            self.clip.scroll = to;
            // The selection stays on a card that shows.
            let items = shown(&self.clip.history, &self.clip.query);
            if let Some(id) = effective(&items, self.clip.selected) {
                let at = items.iter().position(|c| c.id == id).unwrap_or(0);
                let keep = visible_card(at, to, self.clip_rows(), items.len());
                self.clip.selected = items.get(keep).map(|c| c.id);
            }
            self.redraw_clip();
        }
        true
    }

    /// The pointer over a card selects it, so its buttons (pin, plain text,
    /// delete) show on the card under the pointer, not only on the first.
    /// Only a real move does (`pointer_moved`): a still pointer over cards
    /// that scrolled, or one resting after the keyboard chose, keeps the
    /// selection.
    pub(super) fn clip_hover(&mut self, at: (i32, i32), hit: Option<&Hit>) {
        if self.state.view() != View::Panel(Page::Clipboard) {
            return;
        }
        if !pointer_moved(&mut self.clip.pointer, at, &mut self.clip.keyboard_held) {
            return;
        }
        // Every mouse move comes here: only a card or one of its buttons
        // that is not the chosen card needs the list looked at.
        let id = match hit {
            Some(
                Hit::ClipCard(id) | Hit::ClipPin(id) | Hit::ClipPlain(id) | Hit::ClipDelete(id),
            ) => *id,
            _ => return,
        };
        if self.clip.selected == Some(id) {
            return;
        }
        let ids: Vec<u64> = shown(&self.clip.history, &self.clip.query)
            .iter()
            .map(|c| c.id)
            .collect();
        if let Some(id) = hover_target(&ids, self.clip.selected, hit) {
            self.clip.selected = Some(id);
            self.redraw_clip();
        }
    }

    /// A right-click on a card selects it (its buttons show); true when it
    /// was one, so the tray menu stays shut.
    pub(super) fn clip_right_click(&mut self, hit: Option<&Hit>) -> bool {
        if self.state.view() != View::Panel(Page::Clipboard) {
            return false;
        }
        match hit {
            Some(Hit::ClipCard(id)) => {
                self.clip.selected = Some(*id);
                self.redraw_clip();
                true
            }
            _ => false,
        }
    }

    /// After the reducer: a closed panel forgets the search, the selection
    /// and the scroll.
    pub(super) fn follow_clip_page(&mut self, was_open: bool) {
        if was_open && !self.state.panel_is_open() {
            self.clip.selected = None;
            self.clip.scroll = 0;
            self.clip.query.clear();
            self.clip.pointer = None;
            self.clip.keyboard_held = false;
            if let Some(input) = &self.clip_input
                && !input.text().is_empty()
            {
                input.set_text("");
            }
        }
    }

    /// The search field has the keyboard for real.
    pub(super) fn clip_has_keyboard(&self) -> bool {
        self.clip_input.as_ref().is_some_and(CliInput::has_focus)
            && unsafe { GetForegroundWindow() } == self.hwnd
    }

    /// A click on the search field: the island activates (this process got
    /// the click) and the hidden EDIT takes the keyboard, as for the CLI.
    pub(super) fn focus_clip_search(&mut self) -> Option<Event> {
        if editor::focus_open_editor() {
            return None;
        }
        let input = self.clip_input.as_ref()?;
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
        (!self.state.clip_focused()).then_some(Event::ClipFocus(true))
    }

    /// Esc on an empty field: the panel unlocks and the keyboard goes back.
    fn release_clip(&mut self) {
        self.return_keyboard();
        if self.state.clip_focused() {
            self.apply_now(Event::ClipFocus(false));
        }
    }

    /// A key the search field hands over.
    pub(super) fn on_clip_key(&mut self, key: InputKey) {
        match key {
            InputKey::Enter => {
                let items = shown(&self.clip.history, &self.clip.query);
                let Some(id) = effective(&items, self.clip.selected) else {
                    return;
                };
                // Not written (another app holds the clipboard): the page
                // stays, so Enter can be tried again.
                let Some(notice) = self.copy_clip(id, false) else {
                    return;
                };
                // The panel closes and the keyboard goes back (the reducer
                // lets go), then "Disalin".
                self.apply_now(Event::ClipCommit);
                self.apply_now(notice);
            }
            InputKey::Up | InputKey::Down => {
                let items = shown(&self.clip.history, &self.clip.query);
                let Some(id) = effective(&items, self.clip.selected) else {
                    return;
                };
                let at = items.iter().position(|c| c.id == id).unwrap_or(0);
                let to = if key == InputKey::Up {
                    at.saturating_sub(1)
                } else {
                    (at + 1).min(items.len() - 1)
                };
                let next = items[to].id;
                let rows = self.clip_rows();
                self.clip.selected = Some(next);
                self.clip.scroll = clip_scroll_to(to, self.clip.scroll, rows);
                self.clip.keyboard_held = true;
                self.redraw_clip();
            }
            InputKey::PageUp | InputKey::PageDown => {
                self.clip.keyboard_held = true;
                let rows = self.clip_rows() as i32 * 120;
                self.clip_wheel(if key == InputKey::PageUp { rows } else { -rows });
            }
            // Text first; the next Esc lets go.
            InputKey::Escape => match &self.clip_input {
                Some(input) if !input.text().is_empty() => input.set_text(""),
                _ => self.release_clip(),
            },
            InputKey::Blur => {
                if self.state.clip_focused() && !self.clip_has_keyboard() {
                    self.cli_prev = None;
                    self.apply_now(Event::ClipFocus(false));
                }
            }
            InputKey::Tab | InputKey::Stop | InputKey::Delete => {}
        }
    }

    /// The field's text or selection changed: the list filters again from
    /// its first card.
    pub(super) fn on_clip_changed(&mut self) {
        let typed = self
            .clip_input
            .as_ref()
            .map(CliInput::text)
            .unwrap_or_default();
        if typed != self.clip.query {
            self.clip.query = typed;
            self.clip.selected = None;
            self.clip.scroll = 0;
        }
        self.redraw_clip();
    }

    /// Settings → Clipboard.
    pub(super) fn on_clip_setting(&mut self, change: ClipSetting) {
        let cfg = &mut self.config.clipboard;
        match change {
            ClipSetting::Enabled(on) => cfg.enabled = on,
            ClipSetting::Items(n) => cfg.max_items = store::clamp_items(n),
            ClipSetting::Persist(on) => cfg.persist = on,
            ClipSetting::ClearOnLock(on) => cfg.clear_on_lock = on,
            ClipSetting::FeedbackAll(on) => cfg.copied_feedback_all = on,
            ClipSetting::CopyNotice(on) => cfg.copy_notice = on,
            ClipSetting::Clear => {
                if self.clip.history.clear_unpinned() {
                    self.clip_schedule_packing();
                    self.clip.selected = None;
                    self.clip.scroll = 0;
                    self.write_clip_file_now();
                    self.clip_drag_forget_missing();
                }
                self.redraw_clip();
                return;
            }
        }
        self.save_config();
        match change {
            ClipSetting::Enabled(_) => {
                self.sync_clip_listener();
                self.redraw_clip();
            }
            ClipSetting::Items(_) => {
                self.clip.history.set_cap(self.config.clipboard.max_items);
                self.clip_schedule_packing();
                self.clamp_clip_scroll();
                self.schedule_clip_save();
                self.redraw_clip();
            }
            // Off: the file goes. On: it is written now.
            ClipSetting::Persist(false) => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_CLIP_SAVE);
                }
                self.clip.save_due = false;
                persist::write_in_background(persist::path(), Write::Remove);
            }
            ClipSetting::Persist(true) => self.write_clip_file_now(),
            _ => {}
        }
    }
}

/// A change in Settings → Clipboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipSetting {
    Enabled(bool),
    Items(u32),
    Persist(bool),
    ClearOnLock(bool),
    FeedbackAll(bool),
    /// "Notifikasi salinan".
    CopyNotice(bool),
    /// "Hapus riwayat": everything not pinned.
    Clear,
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_managed_editor_copy_shows_nothing_and_any_other_is_as_before() {
        use super::{OwnRoute, own_route};
        use crate::shot_edit::OwnCopy;
        let own = |managed| {
            Some(OwnCopy {
                seq: 5,
                capture: 9,
                managed,
            })
        };
        assert_eq!(own_route(own(true)), OwnRoute::Managed { capture: 9 });
        // An editor opened with auto-copy off: today's path.
        assert_eq!(own_route(own(false)), OwnRoute::Normal);
        // Not an editor write at all.
        assert_eq!(own_route(None), OwnRoute::Normal);
    }

    #[test]
    fn the_pointer_selects_the_card_it_is_over_or_a_button_of_it() {
        use super::{Hit, hover_target};
        let ids = [10, 20, 30, 40];
        // Nothing chosen: the first is selected, so it is not a change.
        assert_eq!(hover_target(&ids, None, Some(&Hit::ClipCard(10))), None);
        assert_eq!(hover_target(&ids, None, Some(&Hit::ClipCard(30))), Some(30));
        // On a button of another card, the same.
        assert_eq!(
            hover_target(&ids, Some(10), Some(&Hit::ClipDelete(20))),
            Some(20)
        );
        assert_eq!(hover_target(&ids, Some(20), Some(&Hit::ClipPin(20))), None);
        // A chosen card that is no longer listed counts as the first.
        assert_eq!(hover_target(&ids, Some(99), Some(&Hit::ClipCard(10))), None);
        assert_eq!(
            hover_target(&ids, Some(99), Some(&Hit::ClipCard(40))),
            Some(40)
        );
        // Other targets, a card not listed, and no target keep the selection.
        assert_eq!(hover_target(&ids, Some(20), Some(&Hit::ClipSearch)), None);
        assert_eq!(hover_target(&ids, Some(20), Some(&Hit::ClipCard(77))), None);
        assert_eq!(hover_target(&ids, Some(20), None), None);
        assert_eq!(hover_target(&[], None, Some(&Hit::ClipCard(1))), None);
    }

    #[test]
    fn a_still_pointer_never_takes_the_selection_from_the_keyboard() {
        use super::pointer_moved;
        let (mut last, mut held) = (None, false);
        // The first move over the page counts.
        assert!(pointer_moved(&mut last, (10, 10), &mut held));
        // The same spot again (the cards scrolled under it): no move.
        assert!(!pointer_moved(&mut last, (10, 10), &mut held));
        assert!(pointer_moved(&mut last, (12, 10), &mut held));
        // The keyboard chose a card: a move at the resting spot is nothing,
        // a real move takes over again.
        held = true;
        assert!(!pointer_moved(&mut last, (12, 10), &mut held));
        assert!(held);
        assert!(pointer_moved(&mut last, (13, 11), &mut held));
        assert!(!held);
        // Held with no known spot: the first move only records where it is.
        let (mut last, mut held) = (None, true);
        assert!(!pointer_moved(&mut last, (5, 5), &mut held));
        assert!(held);
        assert!(pointer_moved(&mut last, (6, 5), &mut held));
    }

    #[test]
    fn after_a_scroll_the_selection_stays_on_a_card_that_shows() {
        use super::visible_card;
        // 10 cards, 2 a row, 3 rows shown.
        // Still in sight: kept.
        assert_eq!(visible_card(4, 1, 3, 10), 4);
        // Scrolled off the top: the same column on the first row shown.
        assert_eq!(visible_card(1, 2, 3, 10), 5);
        assert_eq!(visible_card(0, 2, 3, 10), 4);
        // Off the bottom: the last row shown.
        assert_eq!(visible_card(9, 0, 3, 10), 5);
        // An odd count: the last row's right column does not exist.
        assert_eq!(visible_card(1, 4, 3, 9), 8);
        assert_eq!(visible_card(0, 0, 3, 0), 0);
    }

    use super::*;
    use crate::clipboard::store::tests::{image, text};

    #[test]
    fn the_selection_falls_back_to_the_first_card_shown() {
        let mut h = History::new(50);
        for s in ["a", "b", "c"] {
            ingest(&mut h, text(s), 0);
        }
        let items = shown(&h, "");
        let ids: Vec<u64> = items.iter().map(|c| c.id).collect();
        assert_eq!(effective(&items, None), Some(ids[0]));
        assert_eq!(effective(&items, Some(ids[2])), Some(ids[2]));
        // Filtered away: the first match.
        let only_a = shown(&h, "a");
        assert_eq!(effective(&only_a, Some(ids[0])), Some(ids[2]));
        assert_eq!(effective(&[], Some(1)), None);
    }

    #[test]
    fn copied_previews_one_line() {
        let mut h = History::new(50);
        ingest(&mut h, text("Denah lantai 2\n  sudah saya revisi"), 0);
        let n = copied_notice(&h.items()[0]);
        assert_eq!(n.kind, NoticeKind::Copied);
        assert_eq!(n.title, "Disalin");
        assert_eq!(n.body, "Denah lantai 2 sudah saya revisi");
    }

    fn preview_of(snap: ClipSnapshot) -> (Notice, u64, History) {
        let mut h = History::new(50);
        let Ingested::Added { id, .. } = ingest(&mut h, snap, 0) else {
            panic!("the fake copy was not added");
        };
        let n = clip_preview_notice(h.get(id).expect("entry"));
        (n, id, h)
    }

    #[test]
    fn the_preview_of_text_is_its_first_two_lines_or_a_meta_line() {
        let (n, id, _) = preview_of(text(
            "\r\n  Denah lantai 2 \t\r\nTolong dicek\r\nbaris tiga",
        ));
        assert_eq!(n.kind, NoticeKind::ClipPreview { id });
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("Denah lantai 2", "Tolong dicek")
        );
        assert_eq!(n.source, "Clipboard");
        assert_eq!(n.key, CLIP_PREVIEW_KEY);
        // One line: the kind and the length.
        let (n, _, _) = preview_of(text("cuma satu baris"));
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("cuma satu baris", "Teks · 15 karakter")
        );
        // A link or a colour: the kind's name alone.
        let (n, _, _) = preview_of(text("https://example.com/denah"));
        assert_eq!(n.body, "Tautan");
        let (n, _, _) = preview_of(text("#E2683C"));
        assert_eq!(n.body, "Warna");
        // Code counts characters, not bytes.
        let (n, _, _) = preview_of(text("cargo test --locked"));
        assert_eq!(n.body, "Kode · 19 karakter");
        let (n, _, _) = preview_of(text("日本語のテキスト"));
        assert_eq!(n.body, "Teks · 8 karakter");
    }

    #[test]
    fn the_preview_of_an_image_names_its_size_and_asks_for_the_thumbnail() {
        let (n, id, h) = preview_of(image(1280, 720));
        assert_eq!(n.key, CLIP_IMAGE_KEY);
        // Only bitmap formats were on the clipboard: a screenshot.
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("Screenshot", "1280 × 720")
        );
        let view = View::Notice(n);
        let thumb = clip_notice_view(&h, &view).expect("the image's thumbnail");
        assert_eq!(thumb.id, id);
        assert!(thumb.thumb.w > 0 && thumb.thumb.h > 0);
        // Text has no thumbnail, and an entry that is gone has none.
        let (n, _, h) = preview_of(text("abc"));
        assert!(clip_notice_view(&h, &View::Notice(n)).is_none());
        let gone = View::Notice(Notice {
            kind: NoticeKind::ClipPreview { id: 999 },
            ..clip_preview_notice(&Clip {
                id: 999,
                kind: crate::clipboard::ClipKind::Text,
                data: ClipData::Text("x".into()),
                pinned: false,
                at: 0,
            })
        });
        assert!(clip_notice_view(&History::new(50), &gone).is_none());
        assert!(clip_notice_view(&h, &View::Idle).is_none());
    }

    #[test]
    fn the_preview_of_files_names_them() {
        let snap = |paths: &[&str]| ClipSnapshot {
            files: paths.iter().map(std::path::PathBuf::from).collect(),
            ..ClipSnapshot::default()
        };
        let (n, _, _) = preview_of(snap(&[r"C:\Proyek\laporan.pdf"]));
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("Berkas disalin", "laporan.pdf")
        );
        let (n, _, _) = preview_of(snap(&[r"C:\a.txt", r"C:\b.txt", r"C:\c.txt"]));
        assert_eq!(n.body, "3 file");
        assert_eq!(n.key, CLIP_PREVIEW_KEY);
    }

    #[test]
    fn private_content_never_reaches_the_preview_rules() {
        // A password manager's copy: `ingest` skips it, so no entry, and the
        // rules say no whatever the setting.
        let mut h = History::new(50);
        let secret = ClipSnapshot {
            formats: vec![("ExcludeClipboardContentFromMonitorProcessing".into(), None)],
            text: Some("hunter2".into()),
            ..ClipSnapshot::default()
        };
        let outcome = ingest(&mut h, secret, 0);
        assert_eq!(outcome, Ingested::Skipped);
        assert!(h.is_empty());
        let ctx = NoticeCtx {
            setting: true,
            history: true,
            outcome,
            own: false,
            shot: false,
            clip_page_open: false,
            fullscreen: false,
        };
        assert!(!clip_notice_wanted(&ctx));
        // The same text copied normally is announced, then its repeat is held.
        let outcome = ingest(&mut h, text("hunter2"), 1);
        assert!(clip_notice_wanted(&NoticeCtx { outcome, ..ctx }));
    }

    #[test]
    fn only_an_image_card_starts_a_drag() {
        let mut h = History::new(50);
        let Ingested::Added { id: t, .. } = ingest(&mut h, text("teks"), 0) else {
            panic!("the text was not added");
        };
        let Ingested::Added { id: i, .. } = ingest(&mut h, image(64, 48), 1) else {
            panic!("the image was not added");
        };
        assert_eq!((t, i), (1, 2));
        assert_eq!(drag_press_target(Some(&Hit::ClipCard(2)), &h), Some(2));
        assert_eq!(drag_press_target(Some(&Hit::ClipCard(1)), &h), None);
        // Its buttons never start one.
        assert_eq!(drag_press_target(Some(&Hit::ClipPin(2)), &h), None);
        assert_eq!(drag_press_target(Some(&Hit::ClipDelete(2)), &h), None);
        assert_eq!(drag_press_target(Some(&Hit::ClipCard(99)), &h), None);
        assert_eq!(drag_press_target(None, &h), None);
    }

    #[test]
    fn six_dip_starts_a_drag() {
        use super::super::shelf_page::past_threshold;
        assert!(!past_threshold((0, 0), (4, 4), 6.0));
        assert!(past_threshold((0, 0), (5, 4), 6.0));
        assert!(past_threshold((10, 10), (10, 16), 6.0));
    }

    #[test]
    fn a_small_drag_says_so_like_the_copy() {
        let n = dragged_notice(true);
        assert_eq!(n.key, "clip-dragged");
        assert_eq!(n.title, "Ditarik");
        assert_eq!(n.source, "Clipboard");
        assert_eq!(n.kind, NoticeKind::Copied);
        assert_eq!(
            n.body,
            "Screenshot · pratinjau kecil, aslinya tidak tersimpan"
        );
        assert_eq!(
            dragged_notice(false).body,
            "Gambar · pratinjau kecil, aslinya tidak tersimpan"
        );
    }

    #[test]
    fn a_card_without_its_original_says_it_pastes_small() {
        let mut h = History::new(50);
        ingest(&mut h, image(1280, 720), 0);
        let mut clip = h.items()[0].clone();
        assert_eq!(copied_notice(&clip).body, "Screenshot · 1280 × 720");
        // Restored after a restart, or past the full-size budget.
        if let ClipData::Image(img) = &mut clip.data {
            img.full = None;
        }
        assert_eq!(
            copied_notice(&clip).body,
            "Screenshot · pratinjau kecil, aslinya tidak tersimpan"
        );
    }
}
