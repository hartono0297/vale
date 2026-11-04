//! The island's side of the phone link: the session (`phone::Handle`), what
//! its events said (the QR code and its clock, the phones that joined, the
//! files arriving), the shelf files shared with the phones, and the stage the
//! shelf shows. A child of `app`, so it works on `App` directly. The server,
//! its rules and its network checks are `phone/`; the reducer only knows the
//! stage (`PhoneStage`) and which button was hit.
//!
//! There is no approval: a phone that scans the QR joins at once, and the
//! whole shelf (every file card, no folders, no missing files) is shared with
//! the phones that joined (`phone_sync_shared`). The QR view stays on screen
//! while the session runs and its code renews itself every five minutes.
//!
//! Nothing runs while no session does. During one the server thread is the
//! only cost, plus `TIMER_PHONE`: a one-shot re-armed each second only while
//! the QR view with a code that still counts down is on screen. The server's
//! sink queues `Event::Phone` and wakes the window, and never calls into the
//! app.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::{App, Event, Page, View, WM_APP_EVENT, poster};
use crate::i18n;
use crate::model::PhoneStage;
use crate::phone::session::TOKEN_TTL;
use crate::phone::{
    DeviceKind, EndReason, FailReason, Handle, NoteShare, PhoneEvent, QrMatrix, Refusal,
};
use crate::shelf::{self, Landing, ShelfCard, ShelfKind};
use crate::ui::phone_view::{CodeView, PhoneView, QrView, RecvView, RowStatus, RowView, Warn};

/// One-shot: the QR view's countdown, once a second while it shows. 42-47
/// are claimed by other runs (lyrics card, widgets); this run owns 48.
pub(super) const TIMER_PHONE: usize = 48;

/// Rows kept for the receiving view (the oldest go first).
const MAX_ROWS: usize = 200;

/// What `Ready` said about the listening server and its code.
struct Ready {
    qr: QrMatrix,
    network: String,
    /// "192.168.18.42:47390".
    addr: String,
    made: Instant,
    /// The network is Public (or unknown): the QR view warns.
    public: bool,
}

/// A shelf card that is shared with the phones, as the session's set has it:
/// matched to the card by its key (`shelf::key_of` of the path).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Mirror {
    key: String,
    id: String,
}

/// What one look at the shelf changes in what the phones can take.
#[derive(Debug, PartialEq, Eq)]
struct SyncPlan {
    /// The ids to stop sharing: their card left the shelf, went stale (the
    /// file is gone) or is no longer a file.
    unshare: Vec<String>,
    /// The cards (by place on the shelf) to share: files that are there, not
    /// shared yet and not on the loop guard's list.
    share: Vec<usize>,
}

/// The whole shelf is shared: every card that is a file (no folder) and is
/// not stale, once. `skipped` holds the keys of cards that could not be
/// shared (or whose id was revoked as gone): they are not tried again while
/// the card stays.
fn sync_plan(cards: &[ShelfCard], mirror: &[Mirror], skipped: &[String]) -> SyncPlan {
    let shareable = |c: &ShelfCard| c.kind != ShelfKind::Folder && !c.stale;
    let unshare = mirror
        .iter()
        .filter(|m| !cards.iter().any(|c| c.key == m.key && shareable(c)))
        .map(|m| m.id.clone())
        .collect();
    let share = cards
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            shareable(c)
                && !mirror.iter().any(|m| m.key == c.key)
                && !skipped.contains(&c.key)
        })
        .map(|(i, _)| i)
        .collect();
    SyncPlan { unshare, share }
}

/// The loop guard's list after a look at the shelf: a key stays only while
/// its card is on the shelf (it is forgotten when the card leaves), and a
/// card that went stale is forgotten too, so the file gets a fresh try when
/// it is back. Nothing here runs on a timer: a gone network file is skipped
/// once, not shared again on every sync.
fn prune_skipped(skipped: &mut Vec<String>, cards: &[ShelfCard]) {
    skipped.retain(|k| cards.iter().any(|c| &c.key == k && !c.stale));
}

/// How a batch of files is landing on the shelf.
struct BatchLanding {
    batch: String,
    landing: Landing,
    /// The folder card was added already (`Landing::Folder`).
    folder_added: bool,
}

#[derive(Default)]
pub(super) struct PhoneState {
    /// The number of the current session: each start takes the next one and
    /// its events carry it, so a late event of a session that was stopped
    /// is not applied to the one that follows. Kept across sessions.
    generation: u64,
    handle: Option<Handle>,
    /// A session was asked for and has said neither `Ready` nor `Refused`.
    starting: bool,
    ready: Option<Ready>,
    refusal: Option<Refusal>,
    /// A new code was asked for (the old one ran out while the QR view
    /// showed) and could not be made: the code shows dead, with "Buat QR
    /// baru". Cleared by a code that was made.
    renew_failed: bool,
    /// The kinds of the phones that joined in this session, one per join.
    devices: Vec<DeviceKind>,
    /// The receiving view's header chip: "Android", "2 HP".
    chip: String,
    /// Step 2's line once a phone joined: "Tersambung: Android".
    connected: String,
    /// Rows of the files received, oldest first, and the upload id of each.
    rows: Vec<RowView>,
    ids: Vec<u64>,
    /// The batch each row belongs to, in the same order as `ids`.
    row_batches: Vec<String>,
    /// Files and bytes that arrived whole.
    files: usize,
    bytes: u64,
    batches: Vec<BatchLanding>,
    /// The last batch that asked for the owner's attention.
    attended: Option<String>,
    /// `TIMER_PHONE` is armed.
    timer: bool,
    /// The shelf cards shared with the phones; gone with the session (it is
    /// rebuilt empty by `forget_session`).
    shared: Vec<Mirror>,
    /// Keys of cards that could not be shared, or whose id was revoked as
    /// gone: not tried again while the card stays on the shelf.
    skipped: Vec<String>,
    /// The id of the sticky note on the phones ("Kirim ke HP"); the snapshot
    /// itself lives in the session. Gone with the session.
    note: Option<String>,
}

/// What `phone_keep_fresh` did to the code on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fresh {
    /// It had not run out (or the QR view is not on screen).
    Same,
    /// It had run out and a new one was made.
    Renewed,
    /// It had run out and no new one could be made: it shows dead.
    Dead,
}

impl PhoneState {
    /// A session exists (starting, listening or just refused ones are not).
    pub(super) fn running(&self) -> bool {
        self.handle.is_some()
    }

    fn forget_session(&mut self) {
        *self = PhoneState {
            timer: self.timer,
            generation: self.generation,
            ..PhoneState::default()
        };
    }

    fn ever_joined(&self) -> bool {
        !self.devices.is_empty()
    }

    /// Seconds the code is still good for, counting down from five minutes
    /// whoever joined; `None` once it ran out (or could not be renewed).
    fn remaining(&self, now: Instant) -> Option<u64> {
        let made = self.ready.as_ref()?.made;
        if self.renew_failed {
            return None;
        }
        let left = TOKEN_TTL.saturating_sub(now.saturating_duration_since(made));
        (!left.is_zero()).then_some(left.as_secs() + u64::from(left.subsec_nanos() > 0))
    }

    /// The stage a returning owner sees: files or a connected phone, else
    /// the code.
    fn resume_stage(&self) -> PhoneStage {
        if self.ever_joined() || !self.rows.is_empty() {
            PhoneStage::Receiving
        } else {
            PhoneStage::Qr
        }
    }
}

/// Whether the note on the phones (`shared`, a note id) must come off: it
/// went or left the desktop (`gone`), or it is no longer a desktop note at
/// all (`on_desktop` says whether it still is one).
fn note_must_go(shared: Option<&str>, gone: &[String], on_desktop: impl Fn(&str) -> bool) -> bool {
    shared.is_some_and(|id| gone.iter().any(|g| g == id) || !on_desktop(id))
}

fn warn_of(r: &Refusal) -> Warn {
    match r {
        Refusal::NoLan => Warn::NoLan,
        Refusal::PortsBusy => Warn::PortsBusy,
        Refusal::Failed => Warn::Failed,
    }
}

/// The row of a file by its name: the icon follows the extension.
fn row_for(name: &str, size: u64) -> RowView {
    RowView {
        name: name.to_owned(),
        size,
        kind: shelf::kind_of(std::path::Path::new(name), false),
        status: RowStatus::Receiving(0),
    }
}

fn percent(received: u64, size: u64) -> u8 {
    if size == 0 {
        0
    } else {
        // Never 100 before the file is whole.
        (u128::from(received.min(size)) * 100 / u128::from(size)).min(99) as u8
    }
}

impl App {
    /// What the phone views draw, while the shelf shows one.
    pub(super) fn phone_view<'a>(
        state: &crate::state::IslandState,
        st: &'a PhoneState,
        now: Instant,
    ) -> Option<PhoneView<'a>> {
        let stage = state.phone_stage()?;
        let qr = match (&st.refusal, &st.ready) {
            (Some(r), _) => QrView::Warning(warn_of(r)),
            (None, Some(r)) => QrView::Code(CodeView {
                qr: &r.qr,
                network: &r.network,
                addr: &r.addr,
                remaining: st.remaining(now),
                public: r.public,
                joined: &st.connected,
            }),
            (None, None) => QrView::Starting,
        };
        Some(PhoneView {
            stage,
            qr,
            recv: RecvView {
                rows: &st.rows,
                chip: &st.chip,
                files: st.files,
                bytes: st.bytes,
            },
        })
    }

    /// The whole shelf is shared with the phones while a session listens:
    /// every card that is a file and is there, once; what left the shelf or
    /// went missing stops being shared at once (and "Kosongkan" stops
    /// everything). Runs at `Ready` and at every look at the shelf
    /// (`shelf_look`, which every change of the shelf ends with).
    ///
    /// A file on the network is never looked at here (that could hold the
    /// UI thread): `Handle::share` hands it to a thread of its own, which
    /// revokes it if it is not there (`SharedGone`). A card that cannot be
    /// shared, or whose id was revoked as gone, is on the loop guard's list
    /// and is not tried again while it stays on the shelf.
    pub(super) fn phone_sync_shared(&mut self) {
        if self.phone.ready.is_none() {
            return;
        }
        let Some(handle) = &self.phone.handle else {
            return;
        };
        let cards = &self.shelf.cards;
        prune_skipped(&mut self.phone.skipped, cards);
        let plan = sync_plan(cards, &self.phone.shared, &self.phone.skipped);
        for id in &plan.unshare {
            handle.unshare(id);
        }
        self.phone.shared.retain(|m| !plan.unshare.contains(&m.id));
        for i in plan.share {
            let key = cards[i].key.clone();
            let id = self
                .shelf
                .shelf
                .get(i)
                .map(PathBuf::from)
                .and_then(|path| handle.share(&path));
            match id {
                Some(id) => self.phone.shared.push(Mirror { key, id }),
                None => self.phone.skipped.push(key),
            }
        }
    }

    /// "Kirim ke HP" on sticky note `id`: the note as it is now is handed to
    /// the phone server (it replaces the one before), a session starts if none
    /// runs (as the shelf's HP pill does), and the island panel opens on the
    /// Shelf page at the QR view, latched like a drop. With a session already
    /// running it shows the QR view too. Nothing is written about the note by
    /// the phone side: the page is read only.
    pub(super) fn phone_send_note(&mut self, id: &str) {
        let Some(note) = self.notes.get(id) else {
            return;
        };
        let Some(sticky) = note.sticky.as_ref() else {
            return;
        };
        let accent = crate::theme::current().accent;
        let island = crate::theme::island(accent, crate::theme::current().surface);
        let share = NoteShare::from_note(
            &note.text,
            sticky.paper,
            &island,
            accent,
            &crate::phone::pc_name(),
        );
        if self.phone.handle.is_none() {
            self.phone_begin();
        }
        let Some(handle) = &self.phone.handle else {
            return;
        };
        handle.share_note(Some(share));
        self.phone.note = Some(id.to_owned());
        let now = Instant::now();
        // The QR view, with the panel opened and latched like `PhoneAttention`
        // does for a drop (nothing in fullscreen, where the island is hidden).
        self.apply(Event::PhoneAttention(PhoneStage::Qr), now);
        self.refresh(now);
    }

    /// Takes the note off the phones (the owner's side forgets it too).
    fn phone_note_off(&mut self) {
        if let Some(handle) = &self.phone.handle {
            handle.share_note(None);
        }
        self.phone.note = None;
    }

    /// Note `id` was deleted or taken off the desktop: if it is the one on
    /// the phones, it leaves them.
    pub(super) fn phone_note_gone(&mut self, id: &str) {
        if self.phone.note.as_deref() == Some(id) {
            self.phone_note_off();
        }
    }

    /// After a sync of the notes: a shared note that is no longer a desktop
    /// note (deleted or unstuck from the Notes page) leaves the phones.
    pub(super) fn phone_note_check(&mut self) {
        let notes = &self.notes;
        let goes = note_must_go(self.phone.note.as_deref(), &[], |id| {
            notes.get(id).is_some_and(|n| n.sticky.is_some())
        });
        if goes {
            self.phone_note_off();
        }
    }

    /// Starts a session on the PC's local network. The network, the adapter
    /// and the folder are looked at on the server thread.
    fn phone_begin(&mut self) {
        let tx = self.tx.clone();
        let wake = poster(self.hwnd, WM_APP_EVENT);
        self.phone.forget_session();
        self.phone.generation += 1;
        let session = self.phone.generation;
        let sink: crate::phone::Sink = Arc::new(move |event| {
            let _ = tx.send(Event::Phone(session, event));
            wake();
        });
        self.phone.starting = true;
        self.phone.handle = Some(crate::phone::start_lan(i18n::lang(), sink));
    }

    /// Ends the session for `reason`, forgets what it knew and clears the
    /// stage. Files already received stay (on the shelf and on disk).
    pub(super) fn phone_stop(&mut self, reason: EndReason) {
        let Some(handle) = self.phone.handle.take() else {
            self.phone.forget_session();
            return;
        };
        handle.stop_with(reason);
        self.phone.forget_session();
        self.kill_phone_timer();
    }

    /// The "HP" pill: a session starts if none runs, and the view shows the
    /// stage that fits where it stands.
    pub(super) fn phone_pill(&mut self) -> Option<Event> {
        if self.phone.handle.is_none() {
            self.phone_begin();
            return Some(Event::PhoneStage(Some(PhoneStage::Qr)));
        }
        Some(Event::PhoneStage(Some(self.phone.resume_stage())))
    }

    /// "Buat QR baru" (and "Coba lagi" on a warning): a fresh token for a
    /// listening server, else a new start.
    pub(super) fn phone_new_qr(&mut self) {
        if let (Some(handle), true) = (&self.phone.handle, self.phone.ready.is_some())
            && let Some(link) = handle.new_token()
        {
            if let Some(ready) = &mut self.phone.ready {
                ready.qr = link.qr;
                ready.made = link.token_made;
            }
            self.phone.renew_failed = false;
            return;
        }
        self.phone_begin();
    }

    /// The back button: a session no phone ever joined ends; otherwise it
    /// goes on, and the "HP" pill stays lit.
    pub(super) fn phone_back(&mut self) {
        self.phone.refusal = None;
        if self.phone.handle.is_some() && !self.phone.ever_joined() {
            self.phone_stop(EndReason::Done);
        } else if self.phone.handle.is_none() {
            self.phone.forget_session();
        }
    }

    pub(super) fn phone_cancel(&mut self) {
        if let Some(handle) = &self.phone.handle {
            handle.cancel_uploads();
        }
    }

    /// The PC is going to sleep or the screen was locked: the session ends.
    pub(super) fn phone_on_away(&mut self, reason: EndReason, now: Instant) {
        if self.phone.handle.is_some() {
            self.phone_stop(reason);
            self.apply(Event::PhoneStage(None), now);
            self.refresh(now);
        }
    }

    /// Back from sleep: a session left over (the thread outlived the sleep)
    /// is ended; the phone's connection is gone anyway.
    pub(super) fn phone_on_resume(&mut self, now: Instant) {
        self.phone_on_away(EndReason::Suspend, now);
    }

    /// The Shelf page shows a phone view on an open panel.
    fn phone_view_shown(&self) -> bool {
        self.state.phone_stage().is_some() && self.state.view() == View::Panel(Page::Shelf)
    }

    /// The QR view with a code is what the panel shows.
    fn phone_qr_on_screen(&self) -> bool {
        self.state.view() == View::Panel(Page::Shelf)
            && self.state.phone_stage() == Some(PhoneStage::Qr)
            && self.phone.handle.is_some()
            && self.phone.ready.is_some()
            && self.phone.refusal.is_none()
    }

    /// While the QR view is on screen its code never stays run out: a new
    /// one is made by itself, so the owner never has to press anything. Only
    /// when the system gives no random bytes does the code show dead (with
    /// "Buat QR baru"). The phones that joined are not touched.
    fn phone_keep_fresh(&mut self, now: Instant) -> Fresh {
        if !self.phone_qr_on_screen()
            || self.phone.renew_failed
            || self.phone.remaining(now).is_some()
        {
            return Fresh::Same;
        }
        match self.phone.handle.as_ref().and_then(Handle::new_token) {
            Some(link) => {
                if let Some(ready) = &mut self.phone.ready {
                    ready.qr = link.qr;
                    ready.made = link.token_made;
                }
                Fresh::Renewed
            }
            None => {
                self.phone.renew_failed = true;
                Fresh::Dead
            }
        }
    }

    /// One thing the phone link said.
    pub(super) fn on_phone_event(&mut self, session: u64, event: PhoneEvent, now: Instant) {
        if session != self.phone.generation {
            return;
        }
        match event {
            PhoneEvent::Ready {
                ip,
                port,
                qr,
                network_name,
                public,
                token_made,
                ..
            } => {
                if self.phone.handle.is_none() {
                    return;
                }
                self.phone.starting = false;
                self.phone.refusal = None;
                self.phone.renew_failed = false;
                self.phone.ready = Some(Ready {
                    qr,
                    network: network_name,
                    addr: format!("{ip}:{port}"),
                    made: token_made,
                    public,
                });
                // The shelf as it is now goes to the phones.
                self.shelf_look();
            }
            PhoneEvent::Refused(refusal) => {
                // The thread is done: drop the handle, say why on the card.
                self.phone.handle = None;
                self.phone.starting = false;
                self.phone.ready = None;
                self.phone.refusal = Some(refusal);
                self.phone.shared.clear();
                self.phone.skipped.clear();
                self.phone.note = None;
            }
            PhoneEvent::SharedGone { id } => {
                // The file is missing: its id was revoked, and the card is
                // not shared again while it stays (the loop guard).
                if let Some(at) = self.phone.shared.iter().position(|m| m.id == id) {
                    let gone = self.phone.shared.remove(at);
                    if !self.phone.skipped.contains(&gone.key) {
                        self.phone.skipped.push(gone.key);
                    }
                }
                // Its card turns dim.
                self.shelf_look();
                if self.state.view() == View::Panel(Page::Shelf) && !self.animating() {
                    self.draw(now);
                }
            }
            PhoneEvent::Joined { kind, .. } => {
                if self.phone.handle.is_none() {
                    return;
                }
                // The view that shows stays as it is; only who is connected
                // changes (the chip and step 2's line).
                self.phone.devices.push(kind);
                let lang = i18n::lang();
                self.phone.chip = i18n::phone_devices_chip(lang, &self.phone.devices);
                self.phone.connected = i18n::phone_connected(lang, &self.phone.devices);
                if self.phone_view_shown() && !self.animating() {
                    self.draw(now);
                }
            }
            PhoneEvent::TokenExpired => {
                // The code on screen renews itself; with another view
                // showing it is renewed when the QR view comes back.
                if self.phone_keep_fresh(now) != Fresh::Same
                    && self.phone_view_shown()
                    && !self.animating()
                {
                    self.draw(now);
                }
            }
            PhoneEvent::UploadStarted {
                upload_id,
                name,
                size,
                batch,
                index,
                count,
                ..
            } => {
                if self.phone.handle.is_none() {
                    return;
                }
                if self.phone.rows.len() >= MAX_ROWS {
                    self.phone.rows.remove(0);
                    self.phone.ids.remove(0);
                    self.phone.row_batches.remove(0);
                }
                self.phone.rows.push(row_for(&name, size));
                self.phone.ids.push(upload_id);
                self.phone.row_batches.push(batch.clone());
                if index == 0 || !self.phone.batches.iter().any(|b| b.batch == batch) {
                    // How the batch lands is decided once, with the whole
                    // batch in view: it must fit under the shelf's cap.
                    let landing = shelf::phone_landing(self.shelf.shelf.len(), count as usize);
                    self.phone.batches.retain(|b| b.batch != batch);
                    self.phone.batches.push(BatchLanding {
                        batch: batch.clone(),
                        landing,
                        folder_added: false,
                    });
                    if self.phone.batches.len() > 16 {
                        self.phone.batches.remove(0);
                    }
                }
                // The owner is told once per batch.
                if self.phone.attended.as_deref() != Some(batch.as_str()) {
                    self.phone.attended = Some(batch);
                    self.apply(Event::PhoneAttention(PhoneStage::Receiving), now);
                }
            }
            PhoneEvent::UploadProgress { upload_id, received } => {
                if let Some(i) = self.phone.ids.iter().position(|&id| id == upload_id) {
                    let row = &mut self.phone.rows[i];
                    row.status = RowStatus::Receiving(percent(received, row.size));
                }
                // The frame clock draws anyway while something animates.
                if self.state.phone_stage() == Some(PhoneStage::Receiving)
                    && self.state.view() == View::Panel(Page::Shelf)
                    && !self.animating()
                {
                    self.draw(now);
                }
            }
            PhoneEvent::UploadDone { upload_id, path } => {
                self.phone_upload_done(upload_id, path, now);
            }
            PhoneEvent::UploadFailed { upload_id, reason } => {
                if let Some(i) = self.phone.ids.iter().position(|&id| id == upload_id) {
                    self.phone.rows[i].status = match reason {
                        FailReason::Cancelled => RowStatus::Cancelled,
                        FailReason::Stopped | FailReason::Broken | FailReason::Disk => {
                            RowStatus::Failed
                        }
                    };
                }
            }
            PhoneEvent::SessionEnded(_) => {
                // Only a session that ended by itself (idle, the network
                // changed, an error): one that was stopped here has no
                // handle left, and a stale event from an earlier session
                // meets a handle that is still running.
                if self.phone.handle.as_ref().is_some_and(Handle::has_ended) {
                    self.phone.handle = None;
                    self.phone.forget_session();
                    self.kill_phone_timer();
                    // Whichever page shows: a stage kept with no session
                    // behind it would show a dead view on the way back
                    // and keep the "HP" pill from starting a new one.
                    // It opens nothing.
                    self.apply(Event::PhoneStage(None), now);
                }
            }
        }
    }

    /// A file arrived whole: its row is done, and its card lands on the
    /// shelf (a card of its own, or one for the folder when the batch would
    /// not fit). The shelf is shared, so the phones see it too.
    fn phone_upload_done(&mut self, upload_id: u64, path: PathBuf, now: Instant) {
        let Some(i) = self.phone.ids.iter().position(|&id| id == upload_id) else {
            return;
        };
        self.phone.rows[i].status = RowStatus::Done;
        self.phone.files += 1;
        self.phone.bytes += self.phone.rows[i].size;
        // The batch this upload belongs to (two phones may send at once).
        let batch = self.phone.row_batches[i].clone();
        let landing = self.phone.batches.iter_mut().find(|b| b.batch == batch);
        let cards = match landing {
            Some(b) if b.landing == Landing::Folder => {
                if b.folder_added {
                    Vec::new()
                } else {
                    b.folder_added = true;
                    path.parent().map(|p| vec![p.to_path_buf()]).unwrap_or_default()
                }
            }
            _ => vec![path],
        };
        if !cards.is_empty() {
            self.shelf_add_phone(cards, now);
        }
    }

    fn kill_phone_timer(&mut self) {
        if self.phone.timer {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_PHONE);
            }
            self.phone.timer = false;
        }
    }

    /// The countdown runs only while the QR view with a ticking code is on
    /// screen. Called after every refresh. A code that had run out (the QR
    /// view was away, or it came back to one) is renewed first.
    pub(super) fn sync_phone_timer(&mut self, now: Instant) {
        if self.phone_keep_fresh(now) != Fresh::Same && !self.animating() {
            self.draw(now);
        }
        let wanted = self.phone_qr_on_screen() && self.phone.remaining(now).is_some();
        if wanted && !self.phone.timer {
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_PHONE, 1000, None);
            }
            self.phone.timer = true;
        } else if !wanted {
            self.kill_phone_timer();
        }
    }

    /// `TIMER_PHONE`: one second passed on the QR view. The code that ran
    /// out is renewed before the frame is drawn.
    pub(super) fn on_phone_timer(&mut self) {
        self.kill_phone_timer();
        let now = Instant::now();
        let fresh = self.phone_keep_fresh(now);
        if !self.animating() {
            self.draw(now);
        }
        self.sync_phone_timer(now);
        // A code that could not be renewed shows "Buat QR baru": a full
        // refresh.
        if fresh == Fresh::Dead {
            self.refresh(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use super::*;

    #[test]
    fn progress_never_reads_100_before_the_file_is_whole() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(0, 1000), 0);
        assert_eq!(percent(620, 1000), 62);
        assert_eq!(percent(1000, 1000), 99);
        assert_eq!(percent(5000, 1000), 99);
        assert_eq!(percent(u64::MAX, u64::MAX), 99);
    }

    #[test]
    fn a_row_takes_its_icon_from_the_extension() {
        assert_eq!(row_for("IMG_0531.png", 5).kind, ShelfKind::Image);
        assert_eq!(row_for("VID.mp4", 5).kind, ShelfKind::Video);
        assert_eq!(row_for("song.m4a", 5).kind, ShelfKind::Media);
        assert_eq!(row_for("a.zip", 5).kind, ShelfKind::Archive);
        assert_eq!(row_for("notes", 5).kind, ShelfKind::Document);
        assert_eq!(row_for("x.png", 7).status, RowStatus::Receiving(0));
    }

    #[test]
    fn the_resume_stage_follows_where_the_session_stands() {
        let mut s = PhoneState::default();
        assert_eq!(s.resume_stage(), PhoneStage::Qr);
        s.devices.push(DeviceKind::Android);
        assert_eq!(s.resume_stage(), PhoneStage::Receiving);
        // Files received without a device on record (rows only) show too.
        let mut s = PhoneState::default();
        s.rows.push(row_for("a.png", 1));
        assert_eq!(s.resume_stage(), PhoneStage::Receiving);
    }

    fn ready_at(made: Instant) -> Ready {
        Ready {
            qr: crate::phone::qr::make("http://10.0.0.2:47390/s/x").expect("qr"),
            network: "n".into(),
            addr: "10.0.0.2:47390".into(),
            made,
            public: false,
        }
    }

    #[test]
    fn the_token_always_counts_down_whoever_joined() {
        let t0 = Instant::now();
        let mut s = PhoneState {
            ready: Some(ready_at(t0)),
            ..PhoneState::default()
        };
        assert_eq!(s.remaining(t0), Some(300));
        assert_eq!(s.remaining(t0 + Duration::from_millis(100)), Some(300));
        assert_eq!(s.remaining(t0 + Duration::from_secs(299)), Some(1));
        assert_eq!(s.remaining(t0 + Duration::from_secs(300)), None);
        // A phone that joined does not stop the clock: no "never runs out".
        s.devices.push(DeviceKind::IPhone);
        assert_eq!(s.remaining(t0 + Duration::from_secs(120)), Some(180));
        assert_eq!(s.remaining(t0 + Duration::from_secs(9000)), None);
        // A new code starts its five minutes over.
        s.ready = Some(ready_at(t0 + Duration::from_secs(400)));
        assert_eq!(s.remaining(t0 + Duration::from_secs(400)), Some(300));
        // A code that could not be renewed shows dead.
        s.renew_failed = true;
        assert_eq!(s.remaining(t0 + Duration::from_secs(400)), None);
        assert!(!s.running());
    }

    fn card(key: &str, kind: ShelfKind, stale: bool) -> ShelfCard {
        ShelfCard {
            key: key.to_owned(),
            name: key.to_owned(),
            kind,
            stale,
            from_phone: false,
        }
    }

    fn mirror(key: &str, id: &str) -> Mirror {
        Mirror {
            key: key.to_owned(),
            id: id.to_owned(),
        }
    }

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn every_file_card_is_listed_and_folders_and_stale_cards_are_not() {
        let cards = [
            card("a.pdf", ShelfKind::Document, false),
            card("b.png", ShelfKind::Image, false),
            card("gambar kerja", ShelfKind::Folder, false),
            card("c.zip", ShelfKind::Archive, true),
            card(r"\\srv\x\d.mp4", ShelfKind::Video, false),
        ];
        let plan = sync_plan(&cards, &[], &[]);
        assert_eq!(plan.share, [0, 1, 4], "files that are there, a network one too");
        assert!(plan.unshare.is_empty());
        // The empty shelf lists nothing.
        assert_eq!(sync_plan(&[], &[], &[]), SyncPlan { unshare: vec![], share: vec![] });
    }

    #[test]
    fn a_shared_card_is_not_shared_again() {
        let cards = [
            card("a.pdf", ShelfKind::Document, false),
            card("b.png", ShelfKind::Image, false),
        ];
        let shared = [mirror("a.pdf", "id1")];
        let plan = sync_plan(&cards, &shared, &[]);
        assert_eq!(plan.share, [1]);
        assert!(plan.unshare.is_empty());
        // Everything shared: nothing to do (a sync is cheap and repeatable).
        let both = [mirror("a.pdf", "id1"), mirror("b.png", "id2")];
        assert_eq!(
            sync_plan(&cards, &both, &[]),
            SyncPlan { unshare: vec![], share: vec![] }
        );
    }

    #[test]
    fn a_card_removed_or_gone_stale_or_the_shelf_cleared_revokes_its_id() {
        let shared = [
            mirror("a.pdf", "id1"),
            mirror("b.png", "id2"),
            mirror("c.txt", "id3"),
        ];
        // A card taken off.
        let cards = [
            card("a.pdf", ShelfKind::Document, false),
            card("c.txt", ShelfKind::Document, false),
        ];
        let plan = sync_plan(&cards, &shared, &[]);
        assert_eq!(plan.unshare, ["id2"]);
        assert!(plan.share.is_empty());
        // A file that went missing: its card is stale.
        let cards = [
            card("a.pdf", ShelfKind::Document, true),
            card("b.png", ShelfKind::Image, false),
            card("c.txt", ShelfKind::Document, false),
        ];
        assert_eq!(sync_plan(&cards, &shared, &[]).unshare, ["id1"]);
        // A path that became a folder is not shared either.
        let cards = [
            card("a.pdf", ShelfKind::Folder, false),
            card("b.png", ShelfKind::Image, false),
            card("c.txt", ShelfKind::Document, false),
        ];
        assert_eq!(sync_plan(&cards, &shared, &[]).unshare, ["id1"]);
        // Kosongkan: all of them, in order.
        assert_eq!(sync_plan(&[], &shared, &[]).unshare, ["id1", "id2", "id3"]);
        assert!(sync_plan(&cards, &[], &[]).unshare.is_empty());
    }

    #[test]
    fn a_skipped_card_is_not_retried_while_it_stays_and_is_forgotten_when_it_leaves() {
        let cards = [
            card(r"\\srv\x\gone.pdf", ShelfKind::Document, false),
            card("b.png", ShelfKind::Image, false),
        ];
        let mut skipped = keys(&[r"\\srv\x\gone.pdf"]);
        // Sync after sync: the gone file is not shared again, the other is.
        for _ in 0..3 {
            prune_skipped(&mut skipped, &cards);
            assert_eq!(skipped, [r"\\srv\x\gone.pdf"]);
            assert_eq!(sync_plan(&cards, &[], &skipped).share, [1]);
        }
        // Its card leaves: the key is forgotten (a new card of that path is
        // a fresh try).
        let after = [card("b.png", ShelfKind::Image, false)];
        prune_skipped(&mut skipped, &after);
        assert!(skipped.is_empty());
        assert_eq!(sync_plan(&cards, &[], &skipped).share, [0, 1]);
        // A skipped card that went stale is forgotten too: when the file is
        // back it gets a fresh try (a stale card is never shared meanwhile).
        let mut skipped = keys(&["a.pdf"]);
        let stale = [card("a.pdf", ShelfKind::Document, true)];
        prune_skipped(&mut skipped, &stale);
        assert!(skipped.is_empty());
        assert!(sync_plan(&stale, &[], &skipped).share.is_empty());
        // The shelf cleared: nothing is kept.
        let mut skipped = keys(&["a.pdf", "b.png"]);
        prune_skipped(&mut skipped, &[]);
        assert!(skipped.is_empty());
    }

    #[test]
    fn a_session_that_goes_forgets_what_was_shared_and_skipped() {
        let mut s = PhoneState {
            shared: vec![mirror("a.pdf", "id1")],
            skipped: keys(&["b.png"]),
            devices: vec![DeviceKind::Android],
            connected: "Tersambung: Android".into(),
            ..PhoneState::default()
        };
        s.forget_session();
        assert!(s.shared.is_empty(), "the next session starts with nothing shared");
        assert!(s.skipped.is_empty());
        assert!(s.devices.is_empty() && s.connected.is_empty());
    }

    #[test]
    fn forgetting_a_session_keeps_only_the_timer_flag() {
        let mut s = PhoneState {
            timer: true,
            files: 3,
            renew_failed: true,
            ..PhoneState::default()
        };
        s.generation = 7;
        s.forget_session();
        assert!(s.timer && s.files == 0 && !s.renew_failed);
        assert_eq!(s.generation, 7, "the session number goes on counting");
    }

    #[test]
    fn the_note_on_the_phones_leaves_when_it_goes_or_is_no_longer_on_the_desktop() {
        let gone = vec!["n1".to_owned()];
        let desktop = |id: &str| id != "n2";
        assert!(note_must_go(Some("n1"), &gone, desktop), "deleted or unstuck by the widget");
        assert!(note_must_go(Some("n2"), &[], desktop), "unstuck from the Notes page");
        assert!(!note_must_go(Some("n3"), &gone, desktop), "another note went");
        assert!(!note_must_go(None, &gone, desktop), "nothing is shared");
    }

    #[test]
    fn a_session_that_goes_forgets_the_note_it_showed() {
        let mut s = PhoneState {
            note: Some("n1".into()),
            ..PhoneState::default()
        };
        s.forget_session();
        assert!(s.note.is_none(), "the next session starts with no note");
    }

    #[test]
    fn the_warnings_follow_the_refusal() {
        assert_eq!(warn_of(&Refusal::NoLan), Warn::NoLan);
        assert_eq!(warn_of(&Refusal::PortsBusy), Warn::PortsBusy);
        assert_eq!(warn_of(&Refusal::Failed), Warn::Failed);
    }
}
