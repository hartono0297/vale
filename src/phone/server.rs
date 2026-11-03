//! The server thread: it exists only while a session runs. A non-blocking
//! listener on the PC's local address, polled every 200 ms (the same loop
//! watches the stop flag, the token, unanswered pairings, the idle time and
//! the network), one short-lived thread per connection (at most eight), and
//! the routes in one `match` so a later phase only adds lines to it.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::http::{
    HttpError, Method, Parsed, Request, Response, clear_cookie, parse_head, session_cookie,
};
use super::names::{clash_name, sanitize};
use super::net::{self, BoundCheck, is_private};
use super::session::{PairError, StateAnswer, Tick};
use super::ua;
use super::{EndReason, FailReason, Mode, PhoneEvent, Refusal, Runtime, ServerConfig, Shared};

/// The accept loop's sleep between empty polls.
const TICK: Duration = Duration::from_millis(200);
/// Connections served at once; more are dropped.
const MAX_CONNS: usize = 8;
/// A silent peer is given up on after this long (read and write).
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// The read timeout slice, so the stop flag is seen between reads.
const READ_SLICE: Duration = Duration::from_secs(1);
const CHUNK: usize = 64 * 1024;
/// At most this often per upload the progress is reported.
const PROGRESS_EVERY: Duration = Duration::from_millis(150);
/// After an early answer the rest of the body is read away, up to this much
/// and this long, so the close is not a reset (see `drain`).
const DRAIN_MAX: u64 = 64 * 1024 * 1024;
const DRAIN_TIME: Duration = Duration::from_secs(10);
/// One file may be this big.
const MAX_FILE: u64 = 4 * 1024 * 1024 * 1024;
/// The disk keeps at least this much free after a file.
const MIN_FREE: u64 = 1024 * 1024 * 1024;
const PAGE: &str = include_str!("page.html");
/// Replaced in the page by the boot data.
const BOOT_MARK: &str = "/*BOOT*/null";

pub(super) fn run(shared: &Arc<Shared>, mode: Mode) {
    let (cfg, watch) = match mode {
        #[cfg(any(test, debug_assertions))]
        Mode::Fixed(cfg) => (cfg, None),
        Mode::Lan { lang, pc_name } => match super::detect_lan(lang, &pc_name) {
            Ok((cfg, ip)) => (cfg, Some(ip)),
            Err(refusal) => {
                shared.emit(PhoneEvent::Refused(refusal));
                shared.ended.store(true, Ordering::SeqCst);
                return;
            }
        },
    };
    let listener = match bind(&cfg) {
        Ok(l) => l,
        Err(refusal) => {
            shared.emit(PhoneEvent::Refused(refusal));
            shared.ended.store(true, Ordering::SeqCst);
            return;
        }
    };
    let port = listener.local_addr().map_or(0, |a| a.port());
    let _ = shared.runtime.set(Runtime {
        ip: cfg.bind,
        port,
        dest_dir: cfg.dest_dir.clone(),
        lang: cfg.lang,
        pc_name: cfg.pc_name.clone(),
        network_name: cfg.network_name.clone(),
        allow_loopback: cfg.allow_loopback,
    });
    let Some(rt) = shared.runtime.get() else {
        shared.emit(PhoneEvent::Refused(Refusal::Failed));
        shared.ended.store(true, Ordering::SeqCst);
        return;
    };
    let ready = shared.session.lock().ok().and_then(|s| {
        let url = rt.url(s.token());
        super::qr::make(&url).map(|qr| PhoneEvent::Ready {
            ip: rt.ip,
            port,
            url,
            qr,
            network_name: rt.network_name.clone(),
            token_made: s.token_made(),
        })
    });
    let Some(ready) = ready else {
        shared.emit(PhoneEvent::Refused(Refusal::Failed));
        shared.ended.store(true, Ordering::SeqCst);
        return;
    };
    shared.emit(ready);

    let mut bound = BoundCheck::new();
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let mut busy = false;
        if let Ok((stream, peer)) = listener.accept() {
            // Stopped meanwhile: nobody new is served.
            if shared.stop.load(Ordering::SeqCst) {
                break;
            }
            if let Some(ip) = watch
                && !still_bound(&mut bound, ip)
            {
                shared.request_stop(EndReason::NetworkChanged);
                break;
            }
            accept(shared, rt, stream, peer);
            busy = true;
        }
        // The housekeeping runs on every pass, whatever the traffic.
        if let Some(reason) = tick(shared, watch, &mut bound) {
            shared.request_stop(reason);
            break;
        }
        // More connections may be waiting: look again before sleeping.
        if !busy {
            std::thread::park_timeout(TICK);
        }
    }
    drop(listener);
    // The connection threads see the flag between chunks.
    shared.stop.store(true, Ordering::SeqCst);
    let reason = shared
        .reason
        .lock()
        .map_or(EndReason::Error, |r| *r);
    shared.ended.store(true, Ordering::SeqCst);
    shared.emit(PhoneEvent::SessionEnded(reason));
}

fn still_bound(check: &mut BoundCheck, ip: Ipv4Addr) -> bool {
    check.check(Instant::now(), ip, net::read_adapters)
}

/// Binds the first free port of the range on the chosen address.
fn bind(cfg: &ServerConfig) -> Result<TcpListener, Refusal> {
    let mut busy = false;
    for port in cfg.ports.clone() {
        match TcpListener::bind(SocketAddr::new(cfg.bind, port)) {
            Ok(l) => {
                if l.set_nonblocking(true).is_err() {
                    return Err(Refusal::Failed);
                }
                return Ok(l);
            }
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => busy = true,
            // Windows says "access denied" for a port another program holds
            // exclusively: the next one may be free.
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => busy = true,
            Err(_) => return Err(Refusal::Failed),
        }
    }
    Err(if busy {
        Refusal::PortsBusy
    } else {
        Refusal::Failed
    })
}

/// One pass of the housekeeping: unanswered pairings, the token, the idle
/// time and (on a real network) whether the address is still ours. `Some`
/// when the session must end.
fn tick(shared: &Shared, watch: Option<Ipv4Addr>, bound: &mut BoundCheck) -> Option<EndReason> {
    let ticks = shared.session.lock().ok()?.tick(Instant::now());
    let mut end = None;
    for t in ticks {
        match t {
            Tick::PairingEnded { device_id, outcome } => {
                shared.emit(PhoneEvent::PairingEnded { device_id, outcome })
            }
            Tick::TokenExpired => shared.emit(PhoneEvent::TokenExpired),
            Tick::TokenVoid => shared.emit(PhoneEvent::TokenVoid),
            Tick::Idle => end = Some(EndReason::Idle),
        }
    }
    if end.is_none()
        && let Some(ip) = watch
        && !still_bound(bound, ip)
    {
        end = Some(EndReason::NetworkChanged);
    }
    end
}

/// Counts down a connection slot when the connection's thread ends.
struct Slot(Arc<Shared>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.conns.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A new connection: a peer that is not on a local network is dropped before
/// anything is read, as is one over the connection cap.
fn accept(shared: &Arc<Shared>, rt: &Runtime, stream: TcpStream, peer: SocketAddr) {
    if !is_private(peer.ip(), rt.allow_loopback) {
        return;
    }
    if shared.conns.fetch_add(1, Ordering::SeqCst) >= MAX_CONNS {
        shared.conns.fetch_sub(1, Ordering::SeqCst);
        return;
    }
    let slot = Slot(Arc::clone(shared));
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(READ_SLICE)).is_err()
        || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
    {
        return;
    }
    let shared = Arc::clone(shared);
    let _ = std::thread::Builder::new()
        .name("phone-conn".into())
        .spawn(move || {
            let _slot = slot;
            serve(&shared, stream, peer);
        });
}

/// One read that waits out `IO_TIMEOUT` in `READ_SLICE`s and stops early when
/// the session ends.
fn read_some(shared: &Shared, stream: &mut TcpStream, buf: &mut [u8]) -> io::Result<usize> {
    let mut waited = Duration::ZERO;
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        match stream.read(buf) {
            Ok(n) => return Ok(n),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                waited += READ_SLICE;
                if waited >= IO_TIMEOUT {
                    return Err(io::ErrorKind::TimedOut.into());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// The request head, and what was read past it (the start of the body).
fn read_head(
    shared: &Shared,
    stream: &mut TcpStream,
) -> Result<(Request, Vec<u8>), Option<HttpError>> {
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    loop {
        match parse_head(&buf) {
            Ok(Parsed::Done(req, used)) => return Ok((req, buf[used..].to_vec())),
            Ok(Parsed::Need) => {}
            Err(e) => return Err(Some(e)),
        }
        match read_some(shared, stream, &mut chunk) {
            Ok(0) | Err(_) => return Err(None),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

fn serve(shared: &Shared, mut stream: TcpStream, peer: SocketAddr) {
    let response = match read_head(shared, &mut stream) {
        Ok((req, rest)) => route(shared, &mut stream, &req, rest, peer),
        Err(Some(e)) => Response::for_error(e),
        Err(None) => return,
    };
    let _ = response.write_to(&mut stream);
    let _ = stream.shutdown(Shutdown::Write);
    drain(shared, &mut stream);
}

/// Reads and drops what the phone still sends after the answer (the body of
/// an upload refused before or while it was read: 409, 413, 507, 400), until
/// it closes or goes quiet. Closing a socket with unread data sends a reset,
/// and the phone's browser then reports a network error instead of the
/// answer. A body past `DRAIN_MAX` or `DRAIN_TIME` is given up on; the page
/// then asks `/api/state` why the file failed.
fn drain(shared: &Shared, stream: &mut TcpStream) {
    let _ = stream.set_read_timeout(Some(READ_SLICE));
    let deadline = Instant::now() + DRAIN_TIME;
    let mut sink = vec![0u8; CHUNK];
    let mut drained = 0u64;
    while drained < DRAIN_MAX
        && Instant::now() < deadline
        && !shared.stop.load(Ordering::SeqCst)
    {
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => drained += n as u64,
        }
    }
}

/// Every route, in one place. Anything else is a bare 404.
fn route(
    shared: &Shared,
    stream: &mut TcpStream,
    req: &Request,
    rest: Vec<u8>,
    peer: SocketAddr,
) -> Response {
    match (req.method, req.path.as_str()) {
        (Method::Get, "/") => page(shared, req),
        (Method::Get, "/api/state") => state(shared, req),
        (Method::Post, "/api/upload") => upload(shared, stream, req, rest, peer),
        (Method::Get, path) if path.starts_with("/s/") => scan(shared, req, &path[3..], peer),
        _ => Response::not_found(),
    }
}

/// The page with its boot data (language, PC, network, how far the pairing
/// is), JSON-escaped so that nothing in it can close the script.
fn page_with(shared: &Shared, state: &str, code: Option<&str>) -> Response {
    let Some(rt) = shared.runtime.get() else {
        return Response::not_found();
    };
    let boot = serde_json::json!({
        "lang": rt.lang.code(),
        "pc": rt.pc_name,
        "net": rt.network_name,
        "state": state,
        "code": code,
    });
    let json = boot
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Response::html(PAGE.replacen(BOOT_MARK, &json, 1))
}

/// What a state answer sets on the phone.
fn state_cookies(mut r: Response, answer: &StateAnswer) -> Response {
    if let StateAnswer::Approved {
        session_cookie: vs,
        clear_pairing,
    } = answer
    {
        if let Some(vs) = vs {
            r = r.with_cookie(session_cookie("vs", vs));
        }
        if *clear_pairing {
            r = r.with_cookie(clear_cookie("vp"));
        }
    }
    r
}

/// The page for what `answer_state` said (the pairing's progress).
fn answer_page(shared: &Shared, answer: &StateAnswer) -> Response {
    match answer {
        StateAnswer::Unknown => Response::not_found(),
        StateAnswer::Waiting { code } => page_with(shared, "waiting", Some(code)),
        StateAnswer::Approved { .. } => state_cookies(page_with(shared, "approved", None), answer),
        StateAnswer::Rejected => page_with(shared, "rejected", None),
    }
}

/// `GET /s/<token>`: the QR's address. A valid token makes a pairing (the
/// owner is told and the phone gets its pairing cookie); a phone that already
/// has a cookie of this session just gets the page again, except one whose
/// pairing is over (turned down or timed out): the page tells it to scan
/// again, so a valid token starts a new pairing for it. With a bad token it
/// still gets the "rejected" page.
fn scan(shared: &Shared, req: &Request, token: &str, peer: SocketAddr) -> Response {
    let now = Instant::now();
    let (vp, vs) = (req.cookie("vp"), req.cookie("vs"));
    let Ok(mut s) = shared.session.lock() else {
        return Response::not_found();
    };
    let retry = s.pairing_over(vp, vs);
    if !retry && s.page_access(vp, vs, now) {
        let answer = s.answer_state(vp, vs, now);
        drop(s);
        return answer_page(shared, &answer);
    }
    let pairing = match s.begin_pairing(token, now) {
        Ok(p) => p,
        Err(_) if retry => {
            s.page_access(vp, vs, now);
            let answer = s.answer_state(vp, vs, now);
            drop(s);
            return answer_page(shared, &answer);
        }
        Err(PairError::BadToken | PairError::Busy | PairError::NoRandom) => {
            return Response::not_found();
        }
    };
    drop(s);
    let (kind, browser) = ua::parse(req.header("user-agent").unwrap_or(""));
    shared.emit(PhoneEvent::Pairing {
        device_id: pairing.device_id,
        kind,
        browser,
        ip: peer.ip(),
        code: pairing.code.clone(),
    });
    page_with(shared, "waiting", Some(&pairing.code)).with_cookie(session_cookie("vp", &pairing.cookie))
}

/// `GET /`: the page, for a phone with either cookie.
fn page(shared: &Shared, req: &Request) -> Response {
    let now = Instant::now();
    let (vp, vs) = (req.cookie("vp"), req.cookie("vs"));
    let Ok(mut s) = shared.session.lock() else {
        return Response::not_found();
    };
    if !s.page_access(vp, vs, now) {
        return Response::not_found();
    }
    let answer = s.answer_state(vp, vs, now);
    drop(s);
    answer_page(shared, &answer)
}

/// `GET /api/state`: how far the pairing is.
fn state(shared: &Shared, req: &Request) -> Response {
    let now = Instant::now();
    let (answer, cancelled) = {
        let Ok(mut s) = shared.session.lock() else {
            return Response::not_found();
        };
        let answer = s.answer_state(req.cookie("vp"), req.cookie("vs"), now);
        let cancelled = matches!(answer, StateAnswer::Approved { .. })
            .then(|| s.cancelled_batches());
        (answer, cancelled)
    };
    let Some(rt) = shared.runtime.get() else {
        return Response::not_found();
    };
    let (name, code) = match &answer {
        StateAnswer::Unknown => return Response::not_found(),
        StateAnswer::Waiting { code } => ("waiting", Some(code.as_str())),
        StateAnswer::Approved { .. } => ("approved", None),
        StateAnswer::Rejected => ("rejected", None),
    };
    let mut body = serde_json::json!({
        "state": name,
        "code": code,
        "pc": rt.pc_name,
        "net": rt.network_name,
    });
    // An approved phone also learns why an upload may have failed when its
    // answer was lost to a reset: the cancelled picks and how big a file the
    // disk still takes (the 507 rule).
    if let Some(cancelled) = cancelled {
        body["cancelled"] = serde_json::json!(cancelled);
        if let Some(free) = free_bytes(&rt.dest_dir) {
            body["room"] = serde_json::json!(free.saturating_sub(MIN_FREE));
        }
    }
    let body = body.to_string();
    state_cookies(Response::json(200, body), &answer)
}

/// A batch id from the phone: short, plain characters.
fn valid_batch(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn number(req: &Request, name: &str) -> Option<u32> {
    req.header(name)?.parse().ok()
}

/// Free bytes where `dir` is.
fn free_bytes(dir: &Path) -> Option<u64> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::HSTRING;
    let path = HSTRING::from(dir.as_os_str());
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(&path, Some(&mut free), None, None) }.ok()?;
    Some(free)
}

/// Opens `<name>.part` new (never over another one): `<name> (2).part` and
/// so on when taken.
fn create_part(dir: &Path, name: &str) -> io::Result<(PathBuf, File)> {
    for n in 1..=10_000u32 {
        let path = dir.join(format!("{}.part", clash_name(name, n)));
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

/// Renames `from` to `to` without replacing anything: `MoveFileExW` with no
/// flags fails when `to` exists. (`std::fs::rename` replaces on Windows.)
fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
    use windows::core::HSTRING;
    unsafe {
        MoveFileExW(
            &HSTRING::from(from.as_os_str()),
            &HSTRING::from(to.as_os_str()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(|e| {
        // HRESULT_FROM_WIN32 keeps the Win32 code in the low 16 bits.
        let code = e.code().0 as u32;
        if code >> 16 == 0x8007 {
            io::Error::from_raw_os_error((code & 0xffff) as i32)
        } else {
            io::Error::other(e.message())
        }
    })
}

/// `name`, or its first free numbered form, moved from `part` into `dir`.
fn finish_file(dir: &Path, part: &Path, name: &str) -> io::Result<PathBuf> {
    for n in 1..=10_000u32 {
        let target = dir.join(clash_name(name, n));
        match move_no_replace(part, &target) {
            Ok(()) => return Ok(target),
            // ERROR_FILE_EXISTS (80), ERROR_ALREADY_EXISTS (183).
            Err(e) if matches!(e.raw_os_error(), Some(80 | 183)) => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

fn json_name(name: &str) -> Response {
    Response::json(
        200,
        serde_json::json!({ "ok": true, "name": name }).to_string(),
    )
}

/// `POST /api/upload`: one file as the raw body, its details in headers.
fn upload(
    shared: &Shared,
    stream: &mut TcpStream,
    req: &Request,
    rest: Vec<u8>,
    _peer: SocketAddr,
) -> Response {
    let now = Instant::now();
    let device = match shared.session.lock() {
        Ok(mut s) => s.device_for_api(req.cookie("vs"), now),
        Err(_) => None,
    };
    let Some(device_id) = device else {
        return Response::not_found();
    };
    let Some(rt) = shared.runtime.get() else {
        return Response::not_found();
    };
    let len = req.content_length.unwrap_or(0);
    let batch = req.header("x-vale-batch").unwrap_or("");
    let (Some(index), Some(count)) = (number(req, "x-vale-index"), number(req, "x-vale-count"))
    else {
        return Response::error(400, "bad_request");
    };
    if !valid_batch(batch) || count == 0 || index >= count {
        return Response::error(400, "bad_request");
    }
    if len > MAX_FILE {
        return Response::error(413, "too_big");
    }
    if fs::create_dir_all(&rt.dest_dir).is_err() {
        return Response::error(500, "disk");
    }
    if free_bytes(&rt.dest_dir).is_some_and(|free| free < len.saturating_add(MIN_FREE)) {
        return Response::error(507, "no_space");
    }
    let begun = shared
        .session
        .lock()
        .map(|mut s| s.start_batch(batch))
        .unwrap_or(false);
    if !begun {
        return Response::error(409, "cancelled");
    }
    let name = sanitize(req.header("x-vale-name").unwrap_or(""), rt.lang);
    let Ok((part, mut file)) = create_part(&rt.dest_dir, &name) else {
        return Response::error(500, "disk");
    };
    let upload_id = shared.next_upload.fetch_add(1, Ordering::SeqCst);
    shared.emit(PhoneEvent::UploadStarted {
        upload_id,
        device_id,
        name: name.clone(),
        size: len,
        batch: batch.to_owned(),
        index,
        count,
    });
    let outcome = receive(shared, stream, &mut file, rest, len, batch, upload_id);
    drop(file);
    match outcome {
        Ok(()) => match finish_file(&rt.dest_dir, &part, &name) {
            Ok(path) => {
                let final_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or(name);
                shared.emit(PhoneEvent::UploadDone { upload_id, path });
                json_name(&final_name)
            }
            Err(_) => {
                let _ = fs::remove_file(&part);
                shared.emit(PhoneEvent::UploadFailed {
                    upload_id,
                    reason: FailReason::Disk,
                });
                Response::error(500, "disk")
            }
        },
        Err(reason) => {
            let _ = fs::remove_file(&part);
            shared.emit(PhoneEvent::UploadFailed { upload_id, reason });
            match reason {
                FailReason::Cancelled => Response::error(409, "cancelled"),
                FailReason::Disk => Response::error(500, "disk"),
                FailReason::Stopped | FailReason::Broken => Response::error(400, "broken"),
            }
        }
    }
}

/// Copies the body into `file`, 64 KB at a time, looking at the stop flag and
/// the cancelled batches at every step and reporting progress every 150 ms.
fn receive(
    shared: &Shared,
    stream: &mut TcpStream,
    file: &mut File,
    rest: Vec<u8>,
    len: u64,
    batch: &str,
    upload_id: u64,
) -> Result<(), FailReason> {
    let mut received = 0u64;
    let mut last = Instant::now();
    let mut buf = vec![0u8; CHUNK];
    let mut first = Some(rest);
    while received < len {
        let want = (len - received).min(CHUNK as u64) as usize;
        let n = match first.take() {
            Some(rest) if !rest.is_empty() => {
                let n = rest.len().min(want);
                buf[..n].copy_from_slice(&rest[..n]);
                n
            }
            _ => match read_some(shared, stream, &mut buf[..want]) {
                Ok(0) => return Err(FailReason::Broken),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => return Err(FailReason::Stopped),
                Err(_) => return Err(FailReason::Broken),
            },
        };
        file.write_all(&buf[..n]).map_err(|_| FailReason::Disk)?;
        received += n as u64;
        if shared.stop.load(Ordering::SeqCst) {
            return Err(FailReason::Stopped);
        }
        {
            let Ok(mut s) = shared.session.lock() else {
                return Err(FailReason::Stopped);
            };
            if s.is_cancelled(batch) {
                return Err(FailReason::Cancelled);
            }
            // A long upload is activity: the idle clock must not end it.
            s.touch(Instant::now());
        }
        if last.elapsed() >= PROGRESS_EVERY {
            last = Instant::now();
            shared.emit(PhoneEvent::UploadProgress {
                upload_id,
                received,
            });
        }
    }
    file.flush().map_err(|_| FailReason::Disk)?;
    // A cancel that came with the last chunk still counts.
    if shared
        .session
        .lock()
        .map(|s| s.is_cancelled(batch))
        .unwrap_or(true)
    {
        return Err(FailReason::Cancelled);
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::PAGE;

    /// The end of the call that opens at `open` (the index of its `(`),
    /// skipping string literals.
    fn call_end(s: &[u8], open: usize) -> usize {
        let (mut depth, mut i, mut quote) = (0usize, open, None::<u8>);
        while i < s.len() {
            let c = s[i];
            match quote {
                Some(q) => {
                    if c == b'\\' {
                        i += 1;
                    } else if c == q {
                        quote = None;
                    }
                }
                None => match c {
                    b'"' | b'\'' => quote = Some(c),
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            return i;
                        }
                    }
                    _ => {}
                },
            }
            i += 1;
        }
        panic!("unclosed call at {open}");
    }

    /// `h(...)` makes an element: `+` after it turns the element into the
    /// text "[object HTMLElement]" on the phone.
    #[test]
    fn no_page_element_is_added_to_a_string() {
        let s = PAGE.as_bytes();
        let mut checked = 0;
        for (at, _) in PAGE.match_indices("h(\"") {
            if at > 0 && (s[at - 1].is_ascii_alphanumeric() || s[at - 1] == b'_') {
                continue;
            }
            let end = call_end(s, at + 1);
            let after = PAGE[end + 1..].trim_start();
            assert!(
                !after.starts_with('+'),
                "an element is added to a string: {}",
                &PAGE[at..=end]
            );
            checked += 1;
        }
        assert!(checked > 20, "only {checked} calls found");
    }
}
