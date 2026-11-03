//! The server thread: it exists only while a session runs. A non-blocking
//! listener on the PC's local address, polled every 200 ms (the same loop
//! watches the stop flag, the token, unanswered pairings, the idle time and
//! the network), one short-lived thread per connection (at most eight), and
//! the routes in one `match` so a later phase only adds lines to it.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::files::{self, OpenError};
use super::http::{
    self, FileHead, HttpError, Method, Parsed, Request, Response, clear_cookie, parse_head,
    session_cookie,
};
use super::names::{clash_name, sanitize};
use super::net::{self, BoundCheck, is_private};
use super::session::{PairError, StateAnswer, Tick};
use super::share::{RangeAns, Route, counts_as_taken};
use super::{thumb, ua};
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
    // Back (or a sleep) may have come while the adapters were being read, which
    // can take a while: nothing listens then, so no Firewall prompt either.
    if shared.stop.load(Ordering::SeqCst) {
        end(shared);
        return;
    }
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
    end(shared);
}

/// The normal end: the stop flag (the connection threads see it between
/// chunks), then `ended`, then `SessionEnded` with the reason the stop recorded.
fn end(shared: &Shared) {
    shared.stop.store(true, Ordering::SeqCst);
    // Every shared id goes with the session.
    if let Ok(mut s) = shared.session.lock() {
        s.shared_mut().unshare_all();
    }
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

/// What a route did: an ordinary answer for `serve` to write, or a streamed
/// one the route wrote itself (a file, its head and its body).
enum Routed {
    Reply(Response),
    Streamed,
}

fn serve(shared: &Shared, mut stream: TcpStream, peer: SocketAddr) {
    match read_head(shared, &mut stream) {
        Ok((req, rest)) => match route(shared, &mut stream, &req, rest, peer) {
            Routed::Reply(response) => {
                let _ = response.write_to(&mut stream);
            }
            Routed::Streamed => {}
        },
        Err(Some(e)) => {
            let _ = Response::for_error(e).write_to(&mut stream);
        }
        Err(None) => return,
    }
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
) -> Routed {
    let reply = Routed::Reply;
    match (req.method, req.path.as_str()) {
        (Method::Get, "/") => reply(page(shared, req)),
        (Method::Get, "/api/state") => reply(state(shared, req)),
        (Method::Get, "/api/files") => reply(file_list(shared, req)),
        (Method::Post, "/api/upload") => reply(upload(shared, stream, req, rest, peer)),
        (Method::Get | Method::Head, path) if path.starts_with("/api/file/") => {
            download(shared, stream, req, &path["/api/file/".len()..], Route::File)
        }
        (Method::Get | Method::Head, path) if path.starts_with("/api/view/") => {
            download(shared, stream, req, &path["/api/view/".len()..], Route::View)
        }
        (Method::Get, path) if path.starts_with("/api/thumb/") => {
            reply(thumbnail(shared, req, &path["/api/thumb/".len()..]))
        }
        (Method::Get, path) if path.starts_with("/s/") => reply(scan(shared, req, &path[3..], peer)),
        _ => reply(Response::not_found()),
    }
}

/// The approved phone behind a read-only API request, or `None` (a bare
/// 404): anything but a session cookie, a pairing cookie included. Only
/// `touch` makes it activity: a list or a thumbnail is not, so an open page
/// cannot keep the server alive.
fn api_device(shared: &Shared, req: &Request, touch: bool) -> Option<u64> {
    let mut s = shared.session.lock().ok()?;
    let device = s.device_for_view_api(req.cookie("vs"))?;
    if touch {
        s.touch(Instant::now());
    }
    Some(device)
}

/// A shared file is gone: its id is revoked and the app is told, once.
fn revoke_gone(shared: &Shared, id: &str) {
    let revoked = shared
        .session
        .lock()
        .map(|mut s| s.shared_mut().unshare(id))
        .unwrap_or(false);
    if revoked {
        shared.emit(PhoneEvent::SharedGone { id: id.to_owned() });
    }
}

/// The page's word for a file's kind.
fn kind_word(kind: crate::shelf::ShelfKind) -> &'static str {
    use crate::shelf::ShelfKind;
    match kind {
        ShelfKind::Image => "image",
        ShelfKind::Video => "video",
        ShelfKind::Media => "audio",
        ShelfKind::Archive => "archive",
        ShelfKind::Code => "code",
        ShelfKind::Folder | ShelfKind::Document => "doc",
    }
}

/// `GET /api/files`: what this phone may take, in the order it was shared:
/// id, name, size (read now), kind, whether the page may show it inline,
/// whether this phone finished it and whether it is downloading it. No path
/// ever appears. A file found gone is left out, revoked and reported.
fn file_list(shared: &Shared, req: &Request) -> Response {
    let Some(device) = api_device(shared, req, false) else {
        return Response::not_found();
    };
    let snapshot: Vec<_> = match shared.session.lock() {
        Ok(s) => s
            .shared()
            .entries()
            .iter()
            .map(|e| {
                (
                    e.id.clone(),
                    e.path.clone(),
                    e.name.clone(),
                    e.kind,
                    e.saved_by.contains(&device),
                    e.busy.contains(&device),
                )
            })
            .collect(),
        Err(_) => return Response::not_found(),
    };
    // The sizes are read without the session's lock held.
    let mut files = Vec::new();
    let mut gone = Vec::new();
    for (id, path, name, kind, saved, busy) in snapshot {
        match files::regular_size(&path) {
            Some(size) => files.push(serde_json::json!({
                "id": id,
                "name": name,
                "size": size,
                "kind": kind_word(kind),
                "view": http::is_viewable(&name),
                "saved": saved,
                "busy": busy,
            })),
            None => gone.push(id),
        }
    }
    let mut revoked = Vec::new();
    let rev = match shared.session.lock() {
        Ok(mut s) => {
            for id in gone {
                if s.shared_mut().unshare(&id) {
                    revoked.push(id);
                }
            }
            s.shared().rev()
        }
        Err(_) => return Response::not_found(),
    };
    for id in revoked {
        shared.emit(PhoneEvent::SharedGone { id });
    }
    Response::json(
        200,
        serde_json::json!({ "rev": rev, "files": files }).to_string(),
    )
}

/// Marks the phone busy with a file while it streams, and clears it at the
/// end whatever way the stream ends.
struct Busy<'a> {
    shared: &'a Shared,
    id: &'a str,
    device: u64,
}

impl<'a> Busy<'a> {
    fn new(shared: &'a Shared, id: &'a str, device: u64) -> Self {
        if let Ok(mut s) = shared.session.lock() {
            s.shared_mut().set_busy(id, device, true);
        }
        Busy { shared, id, device }
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        if let Ok(mut s) = self.shared.session.lock() {
            s.shared_mut().set_busy(self.id, self.device, false);
        }
    }
}

/// The body of a file in 64 KB chunks from `start`, `len` bytes: it stops
/// (and the connection is closed short) when the session ends or the id is
/// no longer shared (unticked), or the phone stops reading. Every chunk
/// counts as activity. `true` if every byte was written.
fn stream_body(
    shared: &Shared,
    stream: &mut TcpStream,
    file: &mut File,
    start: u64,
    len: u64,
    id: &str,
) -> bool {
    if start > 0 && file.seek(SeekFrom::Start(start)).is_err() {
        return false;
    }
    let mut buf = vec![0u8; CHUNK];
    let mut left = len;
    while left > 0 {
        if shared.stop.load(Ordering::SeqCst) {
            return false;
        }
        {
            let Ok(mut s) = shared.session.lock() else {
                return false;
            };
            if !s.shared().contains(id) {
                return false;
            }
            s.touch(Instant::now());
        }
        let want = left.min(CHUNK as u64) as usize;
        let n = match file.read(&mut buf[..want]) {
            Ok(0) => return false,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        };
        if stream.write_all(&buf[..n]).is_err() {
            return false;
        }
        left -= n as u64;
    }
    stream.flush().is_ok()
}

/// `GET` and `HEAD` of `/api/file/<id>` (an attachment) and `/api/view/<id>`
/// (an image, inline): the file is looked at again every time, so a file
/// deleted since it was ticked is 410 and its id is revoked. Range and
/// If-Range are honoured (200, 206, 416). The start of the request and each
/// chunk keep the session alive. After the last byte of a request that counts
/// (`counts_as_taken`) the file is marked saved for this phone and, the first
/// time, `FileTaken` is sent.
fn download(
    shared: &Shared,
    stream: &mut TcpStream,
    req: &Request,
    id: &str,
    route: Route,
) -> Routed {
    let bare = || Routed::Reply(Response::not_found());
    let found = {
        let Ok(mut s) = shared.session.lock() else {
            return bare();
        };
        let Some(device) = s.device_for_view_api(req.cookie("vs")) else {
            return bare();
        };
        s.touch(Instant::now());
        s.shared()
            .find(id)
            .map(|e| (device, e.name.clone(), e.path.clone()))
    };
    let Some((device, name, path)) = found else {
        return bare();
    };
    if route == Route::View && !http::is_viewable(&name) {
        return bare();
    }
    let mut opened = match files::open_shared(&path) {
        Ok(o) => o,
        Err(OpenError::Gone) => {
            revoke_gone(shared, id);
            return Routed::Reply(Response::gone());
        }
        Err(OpenError::Busy) => return Routed::Reply(Response::error(500, "busy")),
    };
    let len = opened.len;
    let tag = http::etag(len, opened.mtime);
    let date = http::http_date(opened.mtime);
    let answer = http::answer_range(
        req.header("range"),
        req.header("if-range"),
        len,
        &tag,
        date.as_deref().unwrap_or(""),
    );
    let (status, start, body_len, content_range) = match answer {
        RangeAns::Full => (200, 0, len, None),
        RangeAns::Partial {
            start, end_incl, ..
        } => (
            206,
            start,
            end_incl - start + 1,
            Some(format!("bytes {start}-{end_incl}/{len}")),
        ),
        RangeAns::Unsatisfiable => (416, 0, 0, Some(format!("bytes */{len}"))),
    };
    let mut extra = vec![
        ("Accept-Ranges", "bytes".to_owned()),
        ("ETag", tag),
        (
            "Content-Disposition",
            http::content_disposition(
                if route == Route::File {
                    "attachment"
                } else {
                    "inline"
                },
                &name,
            ),
        ),
    ];
    if let Some(date) = date {
        extra.push(("Last-Modified", date));
    }
    if let Some(range) = content_range {
        extra.push(("Content-Range", range));
    }
    let head = FileHead {
        status,
        content_type: http::mime_for(&name),
        content_length: body_len,
        cache_control: "no-store",
        extra,
    };
    if http::write_file_head(stream, &head).is_err()
        || req.method == Method::Head
        || status == 416
    {
        return Routed::Streamed;
    }
    let busy = Busy::new(shared, id, device);
    let all = stream_body(shared, stream, &mut opened.file, start, body_len, id);
    if counts_as_taken(route, req.method, answer, len, all) {
        let first = shared
            .session
            .lock()
            .ok()
            .and_then(|mut s| s.shared_mut().mark_saved(id, device));
        if first == Some(true) {
            shared.emit(PhoneEvent::FileTaken { id: id.to_owned() });
        }
    }
    drop(busy);
    Routed::Streamed
}

/// `GET /api/thumb/<id>`: a small JPEG of a shared image (never an SVG, never
/// the full image in its place). Any failure is a bare 404 and the page draws
/// the type icon. The thumbnail is kept in the set for the session.
fn thumbnail(shared: &Shared, req: &Request, id: &str) -> Response {
    if api_device(shared, req, false).is_none() {
        return Response::not_found();
    }
    let found = shared
        .session
        .lock()
        .ok()
        .and_then(|s| s.shared().find(id).map(|e| (e.name.clone(), e.path.clone())));
    let Some((name, path)) = found else {
        return Response::not_found();
    };
    if !http::is_viewable(&name) {
        return Response::not_found();
    }
    let opened = match files::open_shared(&path) {
        Ok(o) => o,
        Err(OpenError::Gone) => {
            revoke_gone(shared, id);
            return Response::gone();
        }
        Err(OpenError::Busy) => return Response::not_found(),
    };
    let cached = shared.session.lock().ok().and_then(|s| s.shared().thumb(id));
    if let Some(bytes) = cached {
        return Response::jpeg_thumb(bytes.as_ref().clone());
    }
    let Some(jpeg) = thumb::make(&opened.file) else {
        return Response::not_found();
    };
    if let Ok(mut s) = shared.session.lock() {
        s.shared_mut().set_thumb(id, jpeg.clone());
    }
    Response::jpeg_thumb(jpeg)
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
    let (answer, cancelled, set) = {
        let Ok(mut s) = shared.session.lock() else {
            return Response::not_found();
        };
        let answer = s.answer_state(req.cookie("vp"), req.cookie("vs"), now);
        let approved = matches!(answer, StateAnswer::Approved { .. });
        let cancelled = approved.then(|| s.cancelled_batches());
        let set = approved.then(|| (s.shared().rev(), s.shared().len()));
        (answer, cancelled, set)
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
    // The files ticked for the phone: how many (the page's tab count) and the
    // list's revision (when it differs from the page's, it fetches the list).
    if let Some((rev, count)) = set {
        body["rev"] = serde_json::json!(rev);
        body["shared"] = serde_json::json!(count);
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

    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    /// Not random: a counter run through a mixer, so each call differs.
    fn counter(buf: &mut [u8]) -> bool {
        static N: AtomicU64 = AtomicU64::new(1);
        for b in buf.iter_mut() {
            let mut x = N.fetch_add(0x9E37_79B9_7F4A_7C15, AtomicOrdering::Relaxed);
            x ^= x >> 29;
            x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            *b = (x >> 13) as u8;
        }
        true
    }

    /// A server's shared state whose last activity was `ago` back, with one
    /// approved phone: its session cookie comes with it.
    fn quiet_server(ago: std::time::Duration) -> Option<(Arc<super::Shared>, String, std::time::Instant)> {
        use super::super::session::{Session, StateAnswer};
        use super::super::{EndReason, Shared};
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        use std::sync::{Mutex, OnceLock};
        let now = std::time::Instant::now();
        let old = now.checked_sub(ago)?;
        let mut session = Session::new(old, counter).expect("a session");
        let token = session.token().to_owned();
        let p = session.begin_pairing(&token, old).expect("pairing");
        session.approve(p.device_id, old).expect("approved");
        let StateAnswer::Approved {
            session_cookie: Some(vs),
            ..
        } = session.answer_state(Some(&p.cookie), None, old)
        else {
            panic!("a session cookie");
        };
        let shared = Arc::new(Shared {
            session: Mutex::new(session),
            stop: AtomicBool::new(false),
            reason: Mutex::new(EndReason::Done),
            ended: AtomicBool::new(false),
            sink: Arc::new(|_| {}),
            runtime: OnceLock::new(),
            conns: AtomicUsize::new(0),
            next_upload: AtomicU64::new(1),
        });
        Some((shared, format!("vs={vs}"), now))
    }

    fn get_request(path: &str, cookie: &str) -> super::Request {
        super::Request {
            method: super::Method::Get,
            path: path.to_owned(),
            headers: vec![("cookie".to_owned(), cookie.to_owned())],
            content_length: None,
        }
    }

    /// The idle rule: the file list, a thumbnail and the approved state polls
    /// are not activity (an open page cannot keep the server alive); the
    /// start of a download and each chunk of it are.
    #[test]
    fn the_list_a_thumbnail_and_state_polls_do_not_keep_the_server_alive_but_a_download_does() {
        use std::io::Write;
        use std::net::{TcpListener, TcpStream};
        let ago = std::time::Duration::from_secs(700);
        let Some((shared, cookie, _)) = quiet_server(ago) else {
            eprintln!("skipped: the clock has not run {ago:?} yet");
            return;
        };
        let idle = |s: &super::Shared| s.session.lock().expect("lock").idle(std::time::Instant::now());
        assert!(idle(&shared), "it starts idle");
        // The list, a thumbnail, a state poll: still idle.
        assert_eq!(super::file_list(&shared, &get_request("/api/files", &cookie)).status, 200);
        assert_eq!(super::thumbnail(&shared, &get_request("/api/thumb/x", &cookie), "x").status, 404);
        assert!(super::api_device(&shared, &get_request("/api/files", &cookie), false).is_some());
        assert!(idle(&shared), "the list and a thumbnail are not activity");
        // A request that starts a download is.
        assert!(super::api_device(&shared, &get_request("/api/file/x", &cookie), true).is_some());
        assert!(!idle(&shared));

        // Each chunk written counts: a long download keeps it alive.
        let (shared, _cookie, _) = quiet_server(ago).expect("a clock");
        let dir = crate::phone::files::tests::temp_dir("phone-idle");
        let path = dir.join("a.bin");
        std::fs::write(&path, vec![7u8; 100_000]).expect("write");
        let id = shared
            .session
            .lock()
            .expect("lock")
            .share(&path)
            .expect("shared");
        assert!(idle(&shared));
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let client = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (mut server_side, _) = listener.accept().expect("accept");
        let mut file = std::fs::File::open(&path).expect("open");
        assert!(super::stream_body(&shared, &mut server_side, &mut file, 0, 100_000, &id));
        server_side.flush().expect("flush");
        assert!(!idle(&shared), "a chunk written is activity");
        // A stop (or an unticked id) ends the stream before the next chunk.
        let mut file = std::fs::File::open(&path).expect("open");
        shared.session.lock().expect("lock").shared_mut().unshare(&id);
        assert!(!super::stream_body(&shared, &mut server_side, &mut file, 0, 100_000, &id));
        client.shutdown(std::net::Shutdown::Both).expect("shutdown");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The page is one file and stays under its cap.
    #[test]
    fn the_page_stays_under_30_kb() {
        assert!(PAGE.len() < 30 * 1024, "{} bytes", PAGE.len());
    }

    /// The keys of the language `lang` ("id" or "en") in the page's `S`
    /// object: every `name:` outside strings and arrays, between that
    /// language's `{` and its matching `}`.
    fn dict_keys(lang: &str) -> Vec<String> {
        let start = PAGE.find("var S={").expect("the dictionary");
        let open = format!("{lang}:{{");
        let at = start + PAGE[start..].find(&open).expect("a language") + open.len();
        let b = PAGE.as_bytes();
        let (mut depth, mut i, mut quote) = (0i32, at, None::<u8>);
        let mut keys = Vec::new();
        let mut word = String::new();
        while i < b.len() {
            let c = b[i];
            match quote {
                Some(q) => {
                    if c == b'\\' {
                        i += 1;
                    } else if c == q {
                        quote = None;
                    }
                }
                None => match c {
                    b'"' => quote = Some(c),
                    b'[' | b'{' => depth += 1,
                    b']' => depth -= 1,
                    b'}' => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    b':' if depth == 0 && !word.is_empty() => {
                        keys.push(std::mem::take(&mut word));
                    }
                    c if (c as char).is_ascii_alphanumeric() || c == b'_' => word.push(c as char),
                    _ => word.clear(),
                },
            }
            i += 1;
        }
        keys
    }

    /// Both languages say the same things: a word missing in one would show
    /// as "undefined" on the phone.
    #[test]
    fn the_id_and_en_dictionaries_have_exactly_the_same_keys() {
        let (id, en) = (dict_keys("id"), dict_keys("en"));
        assert!(id.len() > 35, "only {} keys found", id.len());
        let mut a = id.clone();
        let mut b = en.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b);
        let mut dup = a.clone();
        dup.dedup();
        assert_eq!(dup.len(), a.len(), "a key twice in one language");
        // The "soon" tag is gone with the tab enabled.
        assert!(!id.contains(&"soon".to_owned()));
        for key in ["save", "saved", "saveAll", "saving", "gone", "noFiles", "only", "hintIos", "hintAnd"] {
            assert!(id.contains(&key.to_owned()), "{key}");
        }
    }

    /// The new tab reads the list and the download routes and nothing more:
    /// no path, no `innerHTML`, no outside address.
    #[test]
    fn the_page_builds_names_with_text_nodes_and_loads_nothing_outside() {
        // The logo's own SVG is the one `innerHTML` assignment, a literal.
        assert_eq!(PAGE.matches(".innerHTML").count(), 1);
        for outside in ["http://", "https://", "//cdn", "@import", "url("] {
            let n = PAGE.matches(outside).count();
            // The SVG namespace is the only http:// there is.
            let allowed = usize::from(outside == "http://");
            assert!(n <= allowed, "{outside}: {n}");
        }
        for route in ["/api/files", "/api/file/", "/api/view/", "/api/thumb/"] {
            assert!(PAGE.contains(route), "{route}");
        }
    }

    /// A stop asked before the thread got to `bind` (Back pressed while the
    /// adapters were read) ends the session with nothing listening.
    #[test]
    fn a_stop_before_the_bind_listens_nowhere() {
        use super::super::session::Session;
        use super::super::{EndReason, Mode, PhoneEvent, ServerConfig, Shared};
        use crate::i18n::Lang;
        use std::net::{IpAddr, Ipv4Addr};
        use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex, OnceLock};
        use std::time::Instant;

        let events = Arc::new(Mutex::new(Vec::<PhoneEvent>::new()));
        let sink_events = Arc::clone(&events);
        let session = Session::new(Instant::now(), |b| {
            b.fill(7);
            true
        })
        .expect("a session");
        let shared = Arc::new(Shared {
            session: Mutex::new(session),
            stop: AtomicBool::new(false),
            reason: Mutex::new(EndReason::Done),
            ended: AtomicBool::new(false),
            sink: Arc::new(move |e| sink_events.lock().expect("lock").push(e)),
            runtime: OnceLock::new(),
            conns: AtomicUsize::new(0),
            next_upload: AtomicU64::new(1),
        });
        shared.request_stop(EndReason::Suspend);
        let cfg = ServerConfig {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            ports: 0..=0,
            dest_dir: std::env::temp_dir().join("vale-never-made"),
            lang: Lang::En,
            pc_name: "TEST-PC".into(),
            network_name: "Test net".into(),
            allow_loopback: true,
        };
        super::run(&shared, Mode::Fixed(cfg));
        assert!(shared.runtime.get().is_none(), "a listener was made");
        assert!(shared.ended.load(Ordering::SeqCst));
        let seen = events.lock().expect("lock");
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert!(
            matches!(seen[0], PhoneEvent::SessionEnded(EndReason::Suspend)),
            "{seen:?}"
        );
    }
}
