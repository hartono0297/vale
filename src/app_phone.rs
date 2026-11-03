//! The island's side of the phone link: the session (`phone::Handle`), what
//! its events said (the QR code and its clock, the phones that ask, the files
//! arriving), and the stage the shelf shows. A child of `app`, so it works on
//! `App` directly. The server, its rules and its network checks are `phone/`;
//! the reducer only knows the stage (`PhoneStage`) and which button was hit.
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
    Browser, DeviceKind, EndReason, FailReason, Handle, PhoneEvent, QrMatrix, Refusal,
};
use crate::shelf::{self, Landing};
use crate::ui::phone_view::{
    AskView, CodeView, PhoneView, QrView, RecvView, RowStatus, RowView, Warn,
};

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
}

/// A phone that asked and has not been answered.
struct Pending {
    id: u64,
    kind: DeviceKind,
    browser: Browser,
    ip: String,
    code: String,
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
    /// The token was voided (3 rejections) or ran out of time.
    void: bool,
    expired: bool,
    /// The token counts down: until a phone is approved (after that it
    /// lives as long as the session).
    timed: bool,
    /// Phones waiting for an answer, oldest first.
    pending: Vec<Pending>,
    /// The kinds of the phones approved in this session.
    devices: Vec<DeviceKind>,
    chip: String,
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

    fn ever_approved(&self) -> bool {
        !self.devices.is_empty()
    }

    fn remaining(&self, now: Instant) -> Option<u64> {
        let made = self.ready.as_ref()?.made;
        if self.void || self.expired {
            return None;
        }
        if !self.timed {
            return Some(0);
        }
        let left = TOKEN_TTL.saturating_sub(now.saturating_duration_since(made));
        (!left.is_zero()).then_some(left.as_secs() + u64::from(left.subsec_nanos() > 0))
    }

    /// The stage a returning owner sees: a phone waiting, files or a
    /// connected phone, else the code.
    fn resume_stage(&self) -> PhoneStage {
        if !self.pending.is_empty() {
            PhoneStage::Approval
        } else if self.ever_approved() || !self.rows.is_empty() {
            PhoneStage::Receiving
        } else {
            PhoneStage::Qr
        }
    }
}

fn warn_of(r: &Refusal) -> (Warn, &str) {
    match r {
        Refusal::Public { network_name } => (Warn::Public, network_name),
        Refusal::NoLan => (Warn::NoLan, ""),
        Refusal::PortsBusy => (Warn::PortsBusy, ""),
        Refusal::Failed => (Warn::Failed, ""),
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
            (Some(r), _) => {
                let (w, n) = warn_of(r);
                QrView::Warning(w, n)
            }
            (None, Some(r)) => QrView::Code(CodeView {
                qr: &r.qr,
                network: &r.network,
                addr: &r.addr,
                remaining: st.remaining(now),
                timed: st.timed,
                void: st.void,
            }),
            (None, None) => QrView::Starting,
        };
        let ask = st.pending.first().map(|p| AskView {
            id: p.id,
            kind: p.kind,
            browser: p.browser.label(),
            ip: &p.ip,
            code: &p.code,
            more: st.pending.len() - 1,
        });
        Some(PhoneView {
            stage,
            qr,
            ask,
            recv: RecvView {
                rows: &st.rows,
                chip: &st.chip,
                files: st.files,
                bytes: st.bytes,
            },
        })
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
        self.phone.timed = true;
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
            self.phone.void = false;
            self.phone.expired = false;
            self.phone.timed = true;
            return;
        }
        self.phone_begin();
    }

    /// The back button: a session nobody connected to (no phone approved or
    /// waiting) ends; otherwise it goes on, and the "HP" pill stays lit.
    pub(super) fn phone_back(&mut self) {
        self.phone.refusal = None;
        if self.phone.handle.is_some() && !self.phone.ever_approved() && self.phone.pending.is_empty()
        {
            self.phone_stop(EndReason::Done);
        } else if self.phone.handle.is_none() {
            self.phone.forget_session();
        }
    }

    pub(super) fn phone_answer(&mut self, id: u64, allow: bool) {
        if let Some(handle) = &self.phone.handle {
            if allow {
                handle.approve(id);
            } else {
                handle.reject(id);
            }
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
                token_made,
                ..
            } => {
                if self.phone.handle.is_none() {
                    return;
                }
                self.phone.starting = false;
                self.phone.refusal = None;
                self.phone.ready = Some(Ready {
                    qr,
                    network: network_name,
                    addr: format!("{ip}:{port}"),
                    made: token_made,
                });
            }
            PhoneEvent::Refused(refusal) => {
                // The thread is done: drop the handle, say why on the card.
                self.phone.handle = None;
                self.phone.starting = false;
                self.phone.ready = None;
                self.phone.refusal = Some(refusal);
            }
            PhoneEvent::Pairing {
                device_id,
                kind,
                browser,
                ip,
                code,
            } => {
                if self.phone.handle.is_none() {
                    return;
                }
                self.phone.pending.push(Pending {
                    id: device_id,
                    kind,
                    browser,
                    ip: ip.to_string(),
                    code,
                });
                self.apply(Event::PhoneAttention(PhoneStage::Approval), now);
            }
            PhoneEvent::PairingEnded { device_id, outcome } => {
                let Some(at) = self.phone.pending.iter().position(|p| p.id == device_id) else {
                    return;
                };
                let gone = self.phone.pending.remove(at);
                if outcome == crate::phone::Outcome::Approved {
                    // The token stops counting down only on `TokenKept`: a
                    // phone of an older or expired token leaves it as it is.
                    self.phone.devices.push(gone.kind);
                    self.phone.chip = i18n::phone_devices_chip(i18n::lang(), &self.phone.devices);
                }
                // The view that was answering moves on to what is next. The
                // stage is read as the reducer keeps it: another page may
                // be showing, and a stage left at Approval with nobody
                // waiting would make the QR view's buttons do nothing.
                if self.state.phone_stage_kept() == Some(PhoneStage::Approval)
                    && self.phone.pending.is_empty()
                {
                    let next = self.phone.resume_stage();
                    self.apply(Event::PhoneStage(Some(next)), now);
                }
            }
            PhoneEvent::TokenExpired => self.phone.expired = true,
            PhoneEvent::TokenKept { token_made } => {
                // Only for the token on screen: a late one of a token
                // replaced since ("Buat QR baru") changes nothing.
                if self.phone.ready.as_ref().is_some_and(|r| r.made == token_made) {
                    self.phone.timed = false;
                }
            }
            PhoneEvent::TokenVoid => self.phone.void = true,
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
                // The owner is told once per batch, and never over a phone
                // that is waiting for its answer.
                if self.phone.pending.is_empty()
                    && self.phone.attended.as_deref() != Some(batch.as_str())
                {
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
    /// not fit).
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
    /// screen. Called after every refresh.
    pub(super) fn sync_phone_timer(&mut self, now: Instant) {
        let wanted = self.state.view() == View::Panel(Page::Shelf)
            && self.state.phone_stage() == Some(PhoneStage::Qr)
            && self.phone.ready.is_some()
            && self.phone.refusal.is_none()
            && self.phone.timed
            && self.phone.remaining(now).is_some();
        if wanted && !self.phone.timer {
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_PHONE, 1000, None);
            }
            self.phone.timer = true;
        } else if !wanted {
            self.kill_phone_timer();
        }
    }

    /// `TIMER_PHONE`: one second passed on the QR view.
    pub(super) fn on_phone_timer(&mut self) {
        self.kill_phone_timer();
        let now = Instant::now();
        if !self.animating() {
            self.draw(now);
        }
        self.sync_phone_timer(now);
        // When it ran out the view shows "Buat QR baru": a full refresh.
        if self.phone.remaining(now).is_none() && self.phone.ready.is_some() {
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
        use crate::shelf::ShelfKind;
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
        s.pending.push(Pending {
            id: 1,
            kind: DeviceKind::IPhone,
            browser: Browser::Safari,
            ip: "192.168.1.5".into(),
            code: "1234".into(),
        });
        assert_eq!(s.resume_stage(), PhoneStage::Approval);
    }

    #[test]
    fn the_token_counts_down_until_a_phone_is_approved() {
        let t0 = Instant::now();
        let mut s = PhoneState {
            ready: Some(Ready {
                qr: crate::phone::qr::make("http://10.0.0.2:47390/s/x").expect("qr"),
                network: "n".into(),
                addr: "10.0.0.2:47390".into(),
                made: t0,
            }),
            timed: true,
            ..PhoneState::default()
        };
        assert_eq!(s.remaining(t0), Some(300));
        assert_eq!(s.remaining(t0 + Duration::from_millis(100)), Some(300));
        assert_eq!(s.remaining(t0 + Duration::from_secs(299)), Some(1));
        assert_eq!(s.remaining(t0 + Duration::from_secs(300)), None);
        s.expired = true;
        assert_eq!(s.remaining(t0), None);
        s.expired = false;
        s.void = true;
        assert_eq!(s.remaining(t0), None);
        s.void = false;
        s.timed = false;
        assert_eq!(s.remaining(t0 + Duration::from_secs(9000)), Some(0));
        assert!(!s.running());
    }

    #[test]
    fn forgetting_a_session_keeps_only_the_timer_flag() {
        let mut s = PhoneState {
            timer: true,
            files: 3,
            void: true,
            ..PhoneState::default()
        };
        s.generation = 7;
        s.forget_session();
        assert!(s.timer && s.files == 0 && !s.void);
        assert_eq!(s.generation, 7, "the session number goes on counting");
    }
}
