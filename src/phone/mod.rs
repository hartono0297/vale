//! The phone link: a phone scans a QR code, opens a page served by the island
//! over the local network and sends files to the PC. No UI code lives here
//! and nothing needs a network to be tested: `session` holds the rules,
//! `http` the protocol limits, `names` the file names, `net` the choice of
//! network, `qr` the code and `server` the one thread that exists only while
//! a session runs. `app_phone.rs` is the island's side of it.
//!
//! The server is plain HTTP on the PC's local address (never 0.0.0.0), for
//! private peers only, behind a token in the QR code, the owner's approval of
//! each phone (a four digit code on both screens) and a session cookie.

#[cfg(test)]
mod e2e;
#[cfg(debug_assertions)]
pub mod debug;
pub mod http;
pub mod names;
pub mod net;
pub mod qr;
mod server;
pub mod session;
pub mod ua;

use std::net::{IpAddr, Ipv4Addr};
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Instant;

use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};

use crate::i18n::Lang;
pub use qr::QrMatrix;
use session::Session;
pub use ua::{Browser, DeviceKind};

/// The first port tried, and how far the next ones go when it is taken.
pub const PORTS: RangeInclusive<u16> = 47390..=47399;

/// The sink the server tells things to, from its own threads. It must only
/// queue the event (and wake the island's window): it never calls back into
/// the app.
pub type Sink = Arc<dyn Fn(PhoneEvent) + Send + Sync>;

#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The address to listen on: the PC's local one, never 0.0.0.0.
    pub bind: IpAddr,
    /// Tried in order; tests use `0..=0` (any free port).
    pub ports: RangeInclusive<u16>,
    /// Where received files go (made when the first one arrives).
    pub dest_dir: PathBuf,
    pub lang: Lang,
    pub pc_name: String,
    pub network_name: String,
    /// Loopback peers count as private: only for the debug flag and tests.
    pub allow_loopback: bool,
}

/// Why a session did not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The network is Public (or its category is not known).
    Public { network_name: String },
    /// No usable local network.
    NoLan,
    /// Every port of the range is taken.
    PortsBusy,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Approved,
    Rejected,
    TimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// The owner pressed Selesai (or Back with nothing connected).
    Done,
    /// Ten minutes with no meaningful request.
    Idle,
    Suspend,
    Lock,
    /// The address left the adapters.
    NetworkChanged,
    Error,
}

/// Why an upload did not complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailReason {
    Cancelled,
    /// The session ended.
    Stopped,
    /// The connection broke or the body was short.
    Broken,
    /// The disk refused.
    Disk,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PhoneEvent {
    Ready {
        ip: IpAddr,
        port: u16,
        url: String,
        qr: QrMatrix,
        network_name: String,
        token_made: Instant,
    },
    Refused(Refusal),
    Pairing {
        device_id: u64,
        kind: DeviceKind,
        browser: Browser,
        ip: IpAddr,
        code: String,
    },
    PairingEnded {
        device_id: u64,
        outcome: Outcome,
    },
    TokenExpired,
    TokenVoid,
    /// A phone was approved with the token made at `token_made` while it
    /// was good: it no longer runs out. An approval of a phone that paired
    /// with an older token, or after the token expired, does not say this.
    TokenKept {
        token_made: Instant,
    },
    UploadStarted {
        upload_id: u64,
        device_id: u64,
        name: String,
        size: u64,
        batch: String,
        index: u32,
        count: u32,
    },
    UploadProgress {
        upload_id: u64,
        received: u64,
    },
    UploadDone {
        upload_id: u64,
        path: PathBuf,
    },
    UploadFailed {
        upload_id: u64,
        reason: FailReason,
    },
    SessionEnded(EndReason),
}

/// A fresh link for the QR code.
#[derive(Clone, Debug, PartialEq)]
pub struct NewLink {
    pub url: String,
    pub qr: QrMatrix,
    pub token_made: Instant,
}

/// What a running server knows once it listens.
pub(crate) struct Runtime {
    pub ip: IpAddr,
    pub port: u16,
    pub dest_dir: PathBuf,
    pub lang: Lang,
    pub pc_name: String,
    pub network_name: String,
    pub allow_loopback: bool,
}

impl Runtime {
    pub fn url(&self, token: &str) -> String {
        format!("http://{}:{}/s/{token}", self.ip, self.port)
    }
}

/// What the server's threads share.
pub(crate) struct Shared {
    pub session: Mutex<Session>,
    pub stop: AtomicBool,
    pub reason: Mutex<EndReason>,
    /// Set (before `SessionEnded` is sent) when the server thread is done.
    pub ended: AtomicBool,
    pub sink: Sink,
    pub runtime: OnceLock<Runtime>,
    pub conns: AtomicUsize,
    pub next_upload: AtomicU64,
}

impl Shared {
    pub fn emit(&self, event: PhoneEvent) {
        (self.sink)(event);
    }

    /// Asks the server to end, for `reason` (the first reason given stays).
    fn request_stop(&self, reason: EndReason) {
        if let Ok(mut r) = self.reason.lock()
            && !self.stop.load(Ordering::SeqCst)
        {
            *r = reason;
        }
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// What a server thread starts from.
pub(crate) enum Mode {
    /// A fixed address (tests, the debug flag).
    #[cfg(any(test, debug_assertions))]
    Fixed(ServerConfig),
    /// The PC's real local network, found on the thread.
    Lan { lang: Lang, pc_name: String },
}

/// The running link. Dropping it ends the session and its thread.
pub struct Handle {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

/// The system's random bytes (BCrypt). `false` if it fails.
pub fn os_random(buf: &mut [u8]) -> bool {
    unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.is_ok()
}

pub(crate) fn base64url(bytes: &[u8]) -> String {
    crate::google::oauth::base64url(bytes)
}

/// The PC's name (`COMPUTERNAME`), or "PC".
pub fn pc_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "PC".to_owned())
}

fn launch(mode: Mode, sink: Sink) -> Handle {
    let session = Session::new(Instant::now(), os_random);
    let shared = Arc::new(Shared {
        session: Mutex::new(session.unwrap_or_else(|| {
            // No random bytes: a session that cannot pair anyone. The thread
            // below says so and ends.
            Session::new(Instant::now(), |b| {
                b.fill(0);
                true
            })
            .expect("a session from fixed bytes")
        })),
        stop: AtomicBool::new(false),
        reason: Mutex::new(EndReason::Done),
        ended: AtomicBool::new(false),
        sink,
        runtime: OnceLock::new(),
        conns: AtomicUsize::new(0),
        next_upload: AtomicU64::new(1),
    });
    let no_random = session_failed(&shared);
    let thread = {
        let shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("phone-server".into())
            .spawn(move || {
                if no_random {
                    shared.emit(PhoneEvent::Refused(Refusal::Failed));
                    shared.ended.store(true, Ordering::SeqCst);
                    return;
                }
                server::run(&shared, mode);
            })
            .ok()
    };
    if thread.is_none() {
        shared.emit(PhoneEvent::Refused(Refusal::Failed));
        shared.ended.store(true, Ordering::SeqCst);
    }
    Handle { shared, thread }
}

/// Whether the session was made from the fixed fallback (no system random).
fn session_failed(shared: &Shared) -> bool {
    // A fallback session has the all-zero token.
    shared
        .session
        .lock()
        .map(|s| s.token() == base64url(&[0u8; 16]))
        .unwrap_or(true)
}

/// Starts a session on `cfg`'s address. Tells `sink` `Ready` or `Refused`.
/// For the tests and the debug flag; the app uses `start_lan`.
#[cfg(any(test, debug_assertions))]
pub fn start(cfg: ServerConfig, sink: Sink) -> Handle {
    launch(Mode::Fixed(cfg), sink)
}

/// Starts a session on the PC's real local network: the adapter, its
/// category and the Downloads folder are looked up on the server thread
/// (never the caller's), and the sink gets `Refused` (a Public network, no
/// LAN) or `Ready`.
pub fn start_lan(lang: Lang, sink: Sink) -> Handle {
    launch(
        Mode::Lan {
            lang,
            pc_name: pc_name(),
        },
        sink,
    )
}

impl Handle {
    /// The owner said yes to `device_id`.
    pub fn approve(&self, device_id: u64) {
        let answer = self.shared.session.lock().ok().and_then(|mut s| {
            let approval = s.approve(device_id, Instant::now())?;
            Some((approval, s.token_made()))
        });
        let Some((approval, token_made)) = answer else {
            return;
        };
        self.shared.emit(PhoneEvent::PairingEnded {
            device_id,
            outcome: Outcome::Approved,
        });
        if approval.token_kept {
            self.shared.emit(PhoneEvent::TokenKept { token_made });
        }
    }

    /// The owner said no. Three in a session void the token.
    pub fn reject(&self, device_id: u64) {
        let r = self.shared.session.lock().map(|mut s| s.reject(device_id));
        if let Ok(r) = r
            && r.was_pending
        {
            self.shared.emit(PhoneEvent::PairingEnded {
                device_id,
                outcome: Outcome::Rejected,
            });
            if r.voided {
                self.shared.emit(PhoneEvent::TokenVoid);
            }
        }
    }

    /// Batal: uploads in flight stop and their batches' queues are refused.
    pub fn cancel_uploads(&self) {
        if let Ok(mut s) = self.shared.session.lock() {
            s.cancel_batches();
        }
    }

    /// A new token and its QR ("Buat QR baru"). `None` before the server
    /// listens.
    pub fn new_token(&self) -> Option<NewLink> {
        let rt = self.shared.runtime.get()?;
        let mut s = self.shared.session.lock().ok()?;
        if !s.new_token(Instant::now()) {
            return None;
        }
        let url = rt.url(s.token());
        let qr = qr::make(&url)?;
        Some(NewLink {
            url,
            qr,
            token_made: s.token_made(),
        })
    }

    /// Whether the server thread has finished (it ended by itself, or was
    /// stopped).
    pub fn has_ended(&self) -> bool {
        self.shared.ended.load(Ordering::SeqCst)
    }

    /// Ends the session now, saying why. The thread is woken and goes within
    /// a moment; the caller (the island's UI thread) never waits for it, as
    /// the network look it may still be in is not the UI's to wait on.
    pub fn stop_with(mut self, reason: EndReason) {
        self.shut(reason, false);
    }

    /// Ends the session (`Done`) and waits for its thread: the listener is
    /// gone when this returns. For the tests.
    #[cfg(test)]
    pub fn stop(mut self) {
        self.shut(EndReason::Done, true);
    }

    fn shut(&mut self, reason: EndReason, join: bool) {
        self.shared.request_stop(reason);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            if join {
                let _ = t.join();
            }
        }
    }
}

/// Dropping the handle ends the session like `stop_with(Done)`.
impl Drop for Handle {
    fn drop(&mut self) {
        self.shut(EndReason::Done, false);
    }
}

/// The folder files land in: `Downloads\Vale`.
fn downloads_vale() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    unsafe {
        let raw = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok()?;
        let path = raw.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(raw.0 as *const _));
        path.filter(|p| p.is_absolute()).map(|p| p.join("Vale"))
    }
}

/// Finds the local network a session may use: the adapter, its Network List
/// Manager category (Public is refused) and the destination folder. Runs on
/// the server thread, with COM initialised there. The binding address is the
/// adapter's local IPv4.
pub(crate) fn detect_lan(lang: Lang, pc_name: &str) -> Result<(ServerConfig, Ipv4Addr), Refusal> {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let found = (|| {
        let adapter = net::pick_adapter(&net::read_adapters()).ok_or(Refusal::NoLan)?;
        let ip = adapter.private_ip().ok_or(Refusal::NoLan)?;
        let known = net::network_for(&adapter.guid);
        let name = known
            .as_ref()
            .map(|(_, n)| n.clone())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| adapter.name.clone());
        net::start_allowed(known.map(|(c, _)| c)).map_err(|r| match r {
            Refusal::Public { .. } => Refusal::Public {
                network_name: name.clone(),
            },
            other => other,
        })?;
        let dest_dir = downloads_vale().ok_or(Refusal::Failed)?;
        Ok((
            ServerConfig {
                bind: IpAddr::V4(ip),
                ports: PORTS,
                dest_dir,
                lang,
                pc_name: pc_name.to_owned(),
                network_name: name,
                allow_loopback: false,
            },
            ip,
        ))
    })();
    if com {
        unsafe { CoUninitialize() };
    }
    found
}
