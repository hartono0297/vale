//! One end-to-end test over the loopback: the real server thread on
//! 127.0.0.1, a plain `TcpStream` as the phone. It binds no other address,
//! reads no adapters and never touches the real Downloads folder.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{EndReason, PhoneEvent, ServerConfig, start};
use crate::i18n::Lang;

struct Reply {
    status: u16,
    head: String,
    body: String,
}

fn parse_reply(raw: &[u8]) -> Reply {
    let text = String::from_utf8_lossy(raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Reply {
        status,
        head: head.to_owned(),
        body: body.to_owned(),
    }
}

fn connect(addr: SocketAddr) -> TcpStream {
    let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(10))).expect("timeout");
    s
}

fn request(addr: SocketAddr, head: &str, body: &[u8]) -> Reply {
    let mut s = connect(addr);
    s.write_all(head.as_bytes()).expect("write head");
    s.write_all(body).expect("write body");
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    parse_reply(&raw)
}

fn get(addr: SocketAddr, path: &str, cookie: Option<&str>, ua: &str) -> Reply {
    let cookie = cookie.map_or(String::new(), |c| format!("Cookie: {c}\r\n"));
    request(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: x\r\nUser-Agent: {ua}\r\n{cookie}\r\n"),
        b"",
    )
}

fn upload_head(name: &str, batch: &str, idx: u32, count: u32, len: usize, cookie: &str) -> String {
    format!(
        "POST /api/upload HTTP/1.1\r\nHost: x\r\nCookie: {cookie}\r\nX-Vale-Name: {name}\r\nX-Vale-Batch: {batch}\r\nX-Vale-Index: {idx}\r\nX-Vale-Count: {count}\r\nContent-Length: {len}\r\n\r\n"
    )
}

fn wait_for(rx: &Receiver<PhoneEvent>, mut want: impl FnMut(&PhoneEvent) -> bool) -> PhoneEvent {
    loop {
        let e = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("an event in time");
        if want(&e) {
            return e;
        }
    }
}

fn cookie_of(reply: &Reply, name: &str) -> String {
    let key = format!("Set-Cookie: {name}=");
    let line = reply
        .head
        .lines()
        .find(|l| l.starts_with(&key))
        .unwrap_or_else(|| panic!("no {name} cookie in\n{}", reply.head));
    let value = line[key.len()..].split(';').next().expect("value");
    format!("{name}={value}")
}

fn dir_names(dir: &PathBuf) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .expect("dest dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// A cookie that looks like the old pairing cookie, and one made up: neither
/// is any session's.
const OLD_VP: &str = "vp=AAAAAAAAAAAAAAAAAAAAAA";
const MADE_UP: &str = "vs=AAAAAAAAAAAAAAAAAAAAAA";

/// A phone opens the QR's link: the page and its session cookie come back at
/// once, with a `Joined` event. There is no approval step.
fn join(addr: SocketAddr, token: &str) -> String {
    let r = get(addr, &format!("/s/{token}"), None, UA);
    assert_eq!(r.status, 200, "{}", r.head);
    assert!(r.body.contains("\"state\":\"approved\""), "{}", r.body);
    cookie_of(&r, "vs")
}

#[test]
fn a_phone_joins_with_the_token_uploads_is_cancelled_and_the_server_stops() {
    let dest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp")
        .join(format!("phone-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    let cfg = ServerConfig {
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        ports: 0..=0,
        dest_dir: dest.clone(),
        lang: Lang::En,
        pc_name: "TEST-PC".into(),
        network_name: "Test net".into(),
        public: false,
        allow_loopback: true,
    };
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let handle = start(
        cfg,
        Arc::new(move |e| {
            let _ = tx.lock().expect("lock").send(e);
        }),
    );
    let PhoneEvent::Ready { port, url, public, .. } =
        wait_for(&rx, |e| matches!(e, PhoneEvent::Ready { .. }))
    else {
        unreachable!()
    };
    assert!(!public);
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let token = url.rsplit('/').next().expect("token").to_owned();
    let ua = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 Chrome/126.0.0.0 Mobile Safari/537.36";

    // The QR's address: the page, a session cookie and a join event, with
    // nobody to answer.
    let r = get(addr, &format!("/s/{token}"), None, ua);
    assert_eq!(r.status, 200);
    assert!(r.body.contains("TEST-PC"), "boot data is in the page");
    assert!(r.body.contains("\"state\":\"approved\""));
    assert!(
        !r.body.contains("\"state\":\"waiting\"") && !r.body.contains("\"code\":"),
        "no pairing code"
    );
    assert!(!r.body.contains("/*BOOT*/"), "the placeholder is replaced");
    assert!(r.head.contains("Cache-Control: no-store"));
    let vs = cookie_of(&r, "vs");
    assert!(r.head.contains("Set-Cookie: vs="));
    assert!(r.head.contains("HttpOnly; SameSite=Strict; Path=/"));
    assert!(!r.head.contains("Set-Cookie: vp="), "no pairing cookie any more");
    let PhoneEvent::Joined { kind, .. } = wait_for(&rx, |e| matches!(e, PhoneEvent::Joined { .. }))
    else {
        unreachable!()
    };
    assert_eq!(kind, super::DeviceKind::Android);
    // The state and the page are open at once, and set no cookie.
    let s = get(addr, "/api/state", Some(&vs), ua);
    assert_eq!(s.status, 200);
    assert!(s.body.contains("\"state\":\"approved\""));
    assert!(!s.head.contains("Set-Cookie"));
    let page = get(addr, "/", Some(&vs), ua);
    assert_eq!(page.status, 200);
    assert!(page.body.contains("\"state\":\"approved\""));

    // Three files: two with one name and one that tries to leave the folder.
    for (name, body) in [
        ("a.txt", "one"),
        ("a.txt", "two!"),
        ("..%2Fevil.txt", "evil"),
    ] {
        let r = request(addr, &upload_head(name, "b1", 0, 3, body.len(), &vs), body.as_bytes());
        assert_eq!(r.status, 200, "{name}: {}", r.body);
        assert!(r.body.contains("\"ok\":true"));
    }
    assert_eq!(dir_names(&dest), ["a (2).txt", "a.txt", "evil.txt"]);
    assert_eq!(std::fs::read_to_string(dest.join("a.txt")).expect("a"), "one");
    assert_eq!(std::fs::read_to_string(dest.join("a (2).txt")).expect("a2"), "two!");
    assert_eq!(std::fs::read_to_string(dest.join("evil.txt")).expect("evil"), "evil");
    assert!(
        !dest.parent().expect("parent").join("evil.txt").exists(),
        "nothing leaves the folder"
    );

    // Everything else is a bare 404: no cookie, the old pairing cookie, a
    // made-up session cookie, the token as a cookie, a wrong token.
    let bare = |r: Reply| assert_eq!((r.status, r.body.as_str()), (404, ""), "{}", r.head);
    bare(request(addr, &upload_head("n.txt", "b2", 0, 1, 1, MADE_UP), b"x"));
    bare(request(addr, &upload_head("n.txt", "b2", 0, 1, 1, OLD_VP), b"x"));
    bare(request(addr, &upload_head("n.txt", "b2", 0, 1, 1, &format!("vs={token}")), b"x"));
    for cookie in [None, Some(OLD_VP), Some(MADE_UP)] {
        bare(get(addr, "/", cookie, ua));
        bare(get(addr, "/api/state", cookie, ua));
        bare(get(addr, "/api/files", cookie, ua));
    }
    bare(get(addr, "/etc/passwd", Some(&vs), ua));
    bare(get(addr, &format!("/s/{}", "A".repeat(22)), None, ua));
    bare(get(addr, &format!("/s/{}", "A".repeat(22)), Some(OLD_VP), ua));
    bare(get(addr, "/api/upload", Some(&vs), ua));
    assert_eq!(dir_names(&dest).len(), 3);

    // A POST with no Content-Length is refused with 411.
    let r = request(
        addr,
        &format!("POST /api/upload HTTP/1.1\r\nHost: x\r\nCookie: {vs}\r\nX-Vale-Name: a\r\n\r\n"),
        b"",
    );
    assert_eq!(r.status, 411);

    // Batal in the middle of an upload: the part file goes and the batch's
    // queue is refused.
    let mut s = connect(addr);
    s.write_all(upload_head("big.bin", "bx", 0, 2, 200, &vs).as_bytes())
        .expect("head");
    s.write_all(&[7u8; 100]).expect("part of the body");
    wait_for(&rx, |e| matches!(e, PhoneEvent::UploadStarted { name, .. } if name == "big.bin"));
    assert!(dir_names(&dest).contains(&"big.bin.part".to_owned()));
    handle.cancel_uploads();
    s.write_all(&[7u8; 100]).expect("rest of the body");
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    assert_eq!(parse_reply(&raw).status, 409);
    wait_for(&rx, |e| matches!(e, PhoneEvent::UploadFailed { .. }));
    assert_eq!(dir_names(&dest), ["a (2).txt", "a.txt", "evil.txt"], "no .part left");
    let queued = request(addr, &upload_head("next.bin", "bx", 1, 2, 3, &vs), b"abc");
    assert_eq!(queued.status, 409);
    // A queued file the phone keeps sending for longer than a second still
    // gets its 409: the early answer is not lost to a reset.
    let mut slow = connect(addr);
    let chunk = [5u8; 4096];
    let chunks = 30;
    slow.write_all(upload_head("slow.bin", "bx", 1, 2, chunk.len() * chunks, &vs).as_bytes())
        .expect("head");
    for _ in 0..chunks {
        if slow.write_all(&chunk).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
    let mut raw = Vec::new();
    let _ = slow.read_to_end(&mut raw);
    assert_eq!(parse_reply(&raw).status, 409, "slow queued upload");
    // The page learns the cancelled batches from its poll, so it can stop
    // the rest of a pick even when an answer was lost.
    let s = get(addr, "/api/state", Some(&vs), ua);
    let j: serde_json::Value = serde_json::from_str(&s.body).expect("state json");
    let cancelled = j["cancelled"].as_array().expect("cancelled list");
    assert!(cancelled.iter().any(|b| b == "bx"), "{}", s.body);
    assert!(j["room"].is_u64(), "{}", s.body);
    // A new pick is a new batch and works.
    let fresh = request(addr, &upload_head("next.bin", "by", 0, 1, 3, &vs), b"abc");
    assert_eq!(fresh.status, 200);

    // Stopping ends the thread: nothing listens any more.
    handle.stop();
    wait_for(&rx, |e| matches!(e, PhoneEvent::SessionEnded(EndReason::Done)));
    assert!(
        TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_err(),
        "the listener is gone"
    );
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn a_mid_upload_stop_removes_the_part_file() {
    let dest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp")
        .join(format!("phone-e2e-stop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    let cfg = ServerConfig {
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        ports: 0..=0,
        dest_dir: dest.clone(),
        lang: Lang::Id,
        pc_name: "PC".into(),
        network_name: "n".into(),
        public: false,
        allow_loopback: true,
    };
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let handle = start(
        cfg,
        Arc::new(move |e| {
            let _ = tx.lock().expect("lock").send(e);
        }),
    );
    let PhoneEvent::Ready { port, url, .. } =
        wait_for(&rx, |e| matches!(e, PhoneEvent::Ready { .. }))
    else {
        unreachable!()
    };
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let token = url.rsplit('/').next().expect("token").to_owned();
    let vs = join(addr, &token);
    let mut up = connect(addr);
    up.write_all(upload_head("half.bin", "b", 0, 1, 1000, &vs).as_bytes())
        .expect("head");
    up.write_all(&[1u8; 10]).expect("some body");
    wait_for(&rx, |e| matches!(e, PhoneEvent::UploadStarted { .. }));
    handle.stop();
    wait_for(&rx, |e| matches!(e, PhoneEvent::UploadFailed { .. }));
    let left: Vec<String> = dir_names(&dest);
    assert!(left.is_empty(), "{left:?}");
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn two_phones_join_with_one_token_and_a_replaced_token_gets_a_404_with_no_cookie() {
    let r = rig("phone-e2e-two");
    let first = get(r.addr, &format!("/s/{}", r.token), None, UA);
    let vs1 = cookie_of(&first, "vs");
    let PhoneEvent::Joined { device_id: d1, kind: k1 } =
        wait_for(&r.rx, |e| matches!(e, PhoneEvent::Joined { .. }))
    else {
        unreachable!()
    };
    assert_eq!(k1, super::DeviceKind::Android);
    // A second phone (an iPhone) with the very same token: its own cookie.
    let iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) Mobile/15E148 Safari/604.1";
    let second = get(r.addr, &format!("/s/{}", r.token), None, iphone);
    assert_eq!(second.status, 200);
    let vs2 = cookie_of(&second, "vs");
    assert_ne!(vs1, vs2);
    let PhoneEvent::Joined { device_id: d2, kind: k2 } =
        wait_for(&r.rx, |e| matches!(e, PhoneEvent::Joined { .. }))
    else {
        unreachable!()
    };
    assert_ne!(d1, d2);
    assert_eq!(k2, super::DeviceKind::IPhone);
    // Both open the API: each uploads a file.
    for (n, vs) in [(1, &vs1), (2, &vs2)] {
        let body = format!("from phone {n}");
        let up = request(
            r.addr,
            &upload_head(&format!("p{n}.txt"), &format!("b{n}"), 0, 1, body.len(), vs),
            body.as_bytes(),
        );
        assert_eq!(up.status, 200, "{}", up.body);
    }

    // A phone that opens the link again with its cookie is no new device,
    // and is not told to join again.
    let reload = get(r.addr, &format!("/s/{}", r.token), Some(&vs1), UA);
    assert_eq!(reload.status, 200);
    assert!(!reload.head.contains("Set-Cookie"), "{}", reload.head);
    // ... even through a token that is no longer good.
    let link = r.handle.new_token().expect("a new token");
    let new_token = link.url.rsplit('/').next().expect("token").to_owned();
    assert_ne!(new_token, r.token);
    let still = get(r.addr, &format!("/s/{}", r.token), Some(&vs2), UA);
    assert_eq!(still.status, 200, "a phone that is in stays in");
    assert!(!still.head.contains("Set-Cookie"));
    assert!(
        r.events().iter().all(|e| !matches!(e, PhoneEvent::Joined { .. })),
        "a reload is not a join"
    );

    // The replaced token is a bare 404 with no cookie and no event.
    let old = get(r.addr, &format!("/s/{}", r.token), None, UA);
    assert_eq!((old.status, old.body.as_str()), (404, ""));
    assert!(!old.head.contains("Set-Cookie"));
    // A wrong token too, and an old pairing cookie does not help.
    for cookie in [None, Some(OLD_VP)] {
        let wrong = get(r.addr, &format!("/s/{}", "B".repeat(22)), cookie, UA);
        assert_eq!((wrong.status, wrong.body.as_str()), (404, ""));
        assert!(!wrong.head.contains("Set-Cookie"));
    }
    assert!(
        r.events().iter().all(|e| !matches!(e, PhoneEvent::Joined { .. })),
        "a refused link is no join"
    );
    // The phones that joined carry on with the new token in place.
    assert_eq!(get(r.addr, "/api/state", Some(&vs1), UA).status, 200);
    assert_eq!(get(r.addr, "/", Some(&vs2), UA).status, 200);
    // The new token joins a third phone.
    let vs3 = join(r.addr, &new_token);
    let PhoneEvent::Joined { device_id: d3, .. } =
        wait_for(&r.rx, |e| matches!(e, PhoneEvent::Joined { .. }))
    else {
        unreachable!()
    };
    assert!(d3 > d2);
    assert_ne!(vs3, vs1);
    let dir = r.dir.clone();
    r.handle.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_public_network_is_flagged_in_ready_and_changes_nothing_else() {
    for public in [true, false] {
        let r = rig_with("phone-e2e-public", public);
        // The flag is what the notch reads; the server behaves the same.
        assert_eq!(r.public, public);
        let vs = join(r.addr, &r.token);
        assert_eq!(get(r.addr, "/api/state", Some(&vs), UA).status, 200);
        let dir = r.dir.clone();
        r.handle.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ---- The files shared with the phone ----

/// A reply with the body as bytes (a PNG or a JPEG is not text).
struct Raw {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Raw {
    /// A header's value, by name in any case.
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("a json body")
    }
}

fn send(addr: SocketAddr, method: &str, path: &str, cookie: Option<&str>, extra: &str) -> Raw {
    let cookie = cookie.map_or(String::new(), |c| format!("Cookie: {c}\r\n"));
    let mut s = connect(addr);
    s.write_all(format!("{method} {path} HTTP/1.1\r\nHost: x\r\n{cookie}{extra}\r\n").as_bytes())
        .expect("write");
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let at = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a complete head");
    let head = String::from_utf8_lossy(&raw[..at]).into_owned();
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Raw {
        status,
        head,
        body: raw[at + 4..].to_vec(),
    }
}

/// A server on the loopback with its events in `rx`.
struct Rig {
    handle: super::Handle,
    rx: Receiver<PhoneEvent>,
    addr: SocketAddr,
    token: String,
    dir: PathBuf,
    /// What `Ready` said about the network.
    public: bool,
}

const UA: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) Chrome/126.0.0.0 Mobile Safari/537.36";

fn rig(name: &str) -> Rig {
    rig_with(name, false)
}

fn rig_with(name: &str, public: bool) -> Rig {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("test dir");
    let cfg = ServerConfig {
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        ports: 0..=0,
        dest_dir: dir.join("received"),
        lang: Lang::En,
        pc_name: "TEST-PC".into(),
        network_name: "Test net".into(),
        public,
        allow_loopback: true,
    };
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let handle = start(
        cfg,
        Arc::new(move |e| {
            let _ = tx.lock().expect("lock").send(e);
        }),
    );
    let PhoneEvent::Ready {
        port,
        url,
        public: said,
        ..
    } = wait_for(&rx, |e| matches!(e, PhoneEvent::Ready { .. }))
    else {
        unreachable!()
    };
    Rig {
        handle,
        rx,
        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        token: url.rsplit('/').next().expect("token").to_owned(),
        dir,
        public: said,
    }
}

impl Rig {
    /// A phone opens the link: it is in at once, with its session cookie.
    fn join(&self) -> (u64, String) {
        let vs = join(self.addr, &self.token);
        let PhoneEvent::Joined { device_id, .. } =
            wait_for(&self.rx, |e| matches!(e, PhoneEvent::Joined { .. }))
        else {
            unreachable!()
        };
        (device_id, vs)
    }

    /// Every event waiting now.
    fn events(&self) -> Vec<PhoneEvent> {
        self.rx.try_iter().collect()
    }
}

fn files_of(r: &Rig, vs: &str) -> Vec<serde_json::Value> {
    let l = send(r.addr, "GET", "/api/files", Some(vs), "");
    assert_eq!(l.status, 200);
    l.json()["files"].as_array().expect("files").clone()
}

/// The page's "Tersimpan" for this phone: it took the whole file.
fn saved(r: &Rig, vs: &str, id: &str) -> bool {
    entry_of(&files_of(r, vs), id)["saved"] == true
}

fn entry_of(files: &[serde_json::Value], id: &str) -> serde_json::Value {
    files
        .iter()
        .find(|f| f["id"] == id)
        .unwrap_or_else(|| panic!("{id} is not listed"))
        .clone()
}

#[test]
fn a_phone_lists_downloads_resumes_and_is_cut_off_by_the_shared_files() {
    let r = rig("phone-e2e-share");
    let share = r.dir.join("share");
    std::fs::create_dir_all(&share).expect("share dir");
    // One file with a non-ASCII name and text we can slice, one PNG.
    let text: Vec<u8> = (0..300u32).map(|i| b'a' + (i % 26) as u8).collect();
    let txt = share.join("catatan ruang \u{e9}.txt");
    std::fs::write(&txt, &text).expect("write txt");
    let png = share.join("foto.png");
    super::thumb::tests::write_gradient_png(&png, 640, 480);
    let png_bytes = std::fs::read(&png).expect("read png");
    let (_d1, vs) = r.join();

    // Only a regular file can be shared.
    assert!(r.handle.share(&share).is_none(), "a folder");
    assert!(r.handle.share(&share.join("nothing.pdf")).is_none(), "a missing path");
    let txt_id = r.handle.share(&txt).expect("share txt");
    let png_id = r.handle.share(&png).expect("share png");
    assert_eq!(r.handle.share(&txt).expect("again"), txt_id, "the same path, the same id");
    assert_eq!(png_id.len(), 22, "16 random bytes, base64url");
    assert_ne!(txt_id, png_id);

    // /api/state carries the list's revision and the count.
    let st = send(r.addr, "GET", "/api/state", Some(&vs), "").json();
    assert_eq!(st["shared"], 2, "{st}");
    let rev = st["rev"].as_u64().expect("rev");

    // /api/files: both, in the order shared, and no path anywhere.
    let list = send(r.addr, "GET", "/api/files", Some(&vs), "");
    assert_eq!(list.status, 200);
    assert_eq!(list.header("content-type"), Some("application/json; charset=utf-8"));
    assert_eq!(list.json()["rev"].as_u64(), Some(rev));
    let body = String::from_utf8_lossy(&list.body).into_owned();
    assert!(!body.contains("phone-e2e-share"), "no path in {body}");
    assert!(!body.contains("test-tmp") && !body.contains("target"), "no path in {body}");
    assert!(!body.contains(&share.to_string_lossy().replace('\\', "\\\\")));
    let files = files_of(&r, &vs);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["id"], txt_id.as_str());
    assert_eq!(files[0]["name"], "catatan ruang \u{e9}.txt");
    assert_eq!(files[0]["size"], text.len());
    assert_eq!(files[0]["kind"], "doc");
    assert_eq!(files[0]["view"], false);
    assert_eq!((files[0]["saved"].as_bool(), files[0]["busy"].as_bool()), (Some(false), Some(false)));
    assert_eq!(files[1]["kind"], "image");
    assert_eq!(files[1]["view"], true);
    for f in &files {
        assert_eq!(f.as_object().expect("object").len(), 8, "id name size v kind view saved busy: {f}");
    }

    // A full GET: exactly the bytes, with the headers a download needs.
    let got = send(r.addr, "GET", &format!("/api/file/{txt_id}"), Some(&vs), "");
    assert_eq!(got.status, 200);
    assert_eq!(got.body, text);
    assert_eq!(got.header("content-length"), Some("300"));
    assert_eq!(got.header("accept-ranges"), Some("bytes"));
    assert_eq!(got.header("content-type"), Some("application/octet-stream"));
    assert_eq!(got.header("cache-control"), Some("no-store"));
    assert_eq!(got.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(got.header("connection"), Some("close"));
    let cd = got.header("content-disposition").expect("disposition");
    assert!(cd.starts_with("attachment; filename=\"catatan ruang _.txt\""), "{cd}");
    assert!(cd.ends_with("filename*=UTF-8''catatan%20ruang%20%C3%A9.txt"), "{cd}");
    let etag = got.header("etag").expect("etag").to_owned();
    let date = got.header("last-modified").expect("date").to_owned();
    assert!(date.ends_with(" GMT"));
    // A whole download is saved for this phone.
    let after = files_of(&r, &vs);
    assert_eq!(entry_of(&after, &txt_id)["saved"], true);
    assert_eq!(entry_of(&after, &txt_id)["busy"], false);
    assert_eq!(entry_of(&after, &png_id)["saved"], false);
    let st2 = send(r.addr, "GET", "/api/state", Some(&vs), "").json();
    assert!(st2["rev"].as_u64().expect("rev") > rev, "saving changes the revision");

    // HEAD: the same headers and no body.
    let head = send(r.addr, "HEAD", &format!("/api/file/{txt_id}"), Some(&vs), "");
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_eq!(head.header("content-length"), Some("300"));
    assert_eq!(head.header("content-disposition"), Some(cd));
    assert_eq!(head.header("etag"), Some(etag.as_str()));
    assert_eq!(head.header("accept-ranges"), Some("bytes"));

    // Range: a slice, 206 and Content-Range.
    let part = send(r.addr, "GET", &format!("/api/file/{txt_id}"), Some(&vs), "Range: bytes=10-19\r\n");
    assert_eq!(part.status, 206);
    assert_eq!(part.body, text[10..20]);
    assert_eq!(part.header("content-range"), Some("bytes 10-19/300"));
    assert_eq!(part.header("content-length"), Some("10"));
    // The end clamped; a suffix; an If-Range that matches and one that does not.
    let clamp = send(r.addr, "GET", &format!("/api/file/{txt_id}"), Some(&vs), "Range: bytes=290-9999\r\n");
    assert_eq!((clamp.status, clamp.body.as_slice()), (206, &text[290..]));
    let suffix = send(r.addr, "GET", &format!("/api/file/{txt_id}"), Some(&vs), "Range: bytes=-5\r\n");
    assert_eq!((suffix.status, suffix.body.as_slice()), (206, &text[295..]));
    let fresh = send(
        r.addr,
        "GET",
        &format!("/api/file/{txt_id}"),
        Some(&vs),
        &format!("Range: bytes=10-19\r\nIf-Range: {etag}\r\n"),
    );
    assert_eq!(fresh.status, 206);
    let stale = send(
        r.addr,
        "GET",
        &format!("/api/file/{txt_id}"),
        Some(&vs),
        "Range: bytes=10-19\r\nIf-Range: \"stale\"\r\n",
    );
    assert_eq!((stale.status, stale.body.as_slice()), (200, text.as_slice()));
    // A multi-range is answered with the whole file.
    let multi = send(r.addr, "GET", &format!("/api/file/{txt_id}"), Some(&vs), "Range: bytes=0-1,5-6\r\n");
    assert_eq!((multi.status, multi.body.as_slice()), (200, text.as_slice()));
    // Unsatisfiable: 416 with the size and no body.
    let past = send(r.addr, "GET", &format!("/api/file/{txt_id}"), Some(&vs), "Range: bytes=999999-\r\n");
    assert_eq!(past.status, 416);
    assert_eq!(past.header("content-range"), Some("bytes */300"));
    assert!(past.body.is_empty());

    // A range request saves nothing unless it is a resume to the end:
    // tried on the PNG, which this phone has not saved yet.
    let n = png_bytes.len();
    let ranged = |range: String| {
        send(r.addr, "GET", &format!("/api/file/{png_id}"), Some(&vs), &format!("Range: {range}\r\n"))
    };
    assert_eq!(ranged("bytes=0-9".into()).status, 206);
    assert_eq!(ranged("bytes=-5".into()).status, 206);
    // A closed range that reaches the last byte is still not a download.
    let closed = ranged(format!("bytes={}-{}", n - 5, n - 1));
    assert_eq!((closed.status, closed.body.as_slice()), (206, &png_bytes[n - 5..]));
    assert!(!saved(&r, &vs, &png_id), "closed and suffix ranges save nothing");
    // A HEAD of the whole file saves nothing either.
    assert_eq!(send(r.addr, "HEAD", &format!("/api/file/{png_id}"), Some(&vs), "").status, 200);
    assert!(!saved(&r, &vs, &png_id));
    // A resume `bytes=N-` to the end does.
    let resume = ranged(format!("bytes={}-", n - 100));
    assert_eq!((resume.status, resume.body.as_slice()), (206, &png_bytes[n - 100..]));
    assert!(saved(&r, &vs, &png_id), "saved by the resume to the end");

    // Two phones taking one file: saved is per phone.
    let (_d2, vs2) = r.join();
    let second = r.dir.join("two.txt");
    std::fs::write(&second, b"two phones").expect("write");
    let two = r.handle.share(&second).expect("share two");
    assert_eq!(send(r.addr, "GET", &format!("/api/file/{two}"), Some(&vs), "").body, b"two phones");
    assert_eq!(send(r.addr, "GET", &format!("/api/file/{two}"), Some(&vs2), "").body, b"two phones");
    assert_eq!(send(r.addr, "GET", &format!("/api/file/{two}"), Some(&vs2), "").status, 200);
    assert!(saved(&r, &vs, &two) && saved(&r, &vs2, &two), "saved for each phone");
    assert_eq!(entry_of(&files_of(&r, &vs2), &two)["saved"], true);
    assert_eq!(entry_of(&files_of(&r, &vs2), &png_id)["saved"], false, "phone 2 has not saved the PNG");
    assert_eq!(entry_of(&files_of(&r, &vs), &png_id)["saved"], true);

    // /api/view and /api/thumb: images only, inline and small.
    let view = send(r.addr, "GET", &format!("/api/view/{png_id}"), Some(&vs), "");
    assert_eq!(view.status, 200);
    assert_eq!(view.body, png_bytes);
    assert_eq!(view.header("content-type"), Some("image/png"));
    assert!(view.header("content-disposition").expect("cd").starts_with("inline; filename=\"foto.png\""));
    assert_eq!(
        send(r.addr, "GET", &format!("/api/view/{png_id}"), Some(&vs), "Range: bytes=1-3\r\n").body,
        png_bytes[1..4]
    );
    assert_eq!(send(r.addr, "GET", &format!("/api/view/{txt_id}"), Some(&vs), "").status, 404, "not an image");
    let thumb = send(r.addr, "GET", &format!("/api/thumb/{png_id}"), Some(&vs), "");
    assert_eq!(thumb.status, 200);
    assert_eq!(&thumb.body[..2], [0xFF, 0xD8]);
    assert_eq!(thumb.header("content-type"), Some("image/jpeg"));
    assert_eq!(thumb.header("cache-control"), Some("private, max-age=3600"));
    assert!(thumb.body.len() < png_bytes.len(), "a thumbnail, not the image");
    let again = send(r.addr, "GET", &format!("/api/thumb/{png_id}"), Some(&vs), "");
    assert_eq!(again.body, thumb.body, "the second one comes from the set");
    // The PNG is written over in place (same path, same id): the list's
    // version moves, so the page asks for a new URL, and the thumbnail is
    // made again rather than the old picture.
    let v_before = entry_of(&files_of(&r, &vs), &png_id)["v"].as_str().expect("v").to_owned();
    super::thumb::tests::write_gradient_png(&png, 480, 640);
    let v_after = entry_of(&files_of(&r, &vs), &png_id)["v"].as_str().expect("v").to_owned();
    assert_ne!(v_before, v_after, "the version follows the file");
    let fresh = send(r.addr, "GET", &format!("/api/thumb/{png_id}?v={v_after}"), Some(&vs), "");
    assert_eq!(fresh.status, 200);
    assert_ne!(fresh.body, thumb.body, "a file written over gets a new thumbnail");
    assert_eq!(
        super::thumb::tests::jpeg_size(&fresh.body),
        Some((144, 192)),
        "the new picture's shape"
    );
    // Back to the first picture, so what follows reads the same bytes.
    super::thumb::tests::write_gradient_png(&png, 640, 480);
    assert_eq!(send(r.addr, "GET", &format!("/api/thumb/{txt_id}"), Some(&vs), "").status, 404);
    // An SVG is never shown inline, never a thumbnail; only a download.
    let svg = r.dir.join("logo.svg");
    std::fs::write(&svg, b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>").expect("write");
    let svg_id = r.handle.share(&svg).expect("share svg");
    assert_eq!(entry_of(&files_of(&r, &vs), &svg_id)["view"], false);
    assert_eq!(send(r.addr, "GET", &format!("/api/view/{svg_id}"), Some(&vs), "").status, 404);
    assert_eq!(send(r.addr, "GET", &format!("/api/thumb/{svg_id}"), Some(&vs), "").status, 404);
    let dl = send(r.addr, "GET", &format!("/api/file/{svg_id}"), Some(&vs), "");
    assert_eq!(dl.status, 200);
    assert_eq!(dl.header("content-type"), Some("application/octet-stream"));
    assert!(dl.header("content-disposition").expect("cd").starts_with("attachment;"));

    // Anything but a joined phone's session cookie gets the bare 404.
    for (method, path) in [
        ("GET", "/api/files".to_owned()),
        ("GET", format!("/api/file/{txt_id}")),
        ("HEAD", format!("/api/file/{txt_id}")),
        ("GET", format!("/api/view/{png_id}")),
        ("GET", format!("/api/thumb/{png_id}")),
    ] {
        for cookie in [None, Some("vs=nope"), Some(OLD_VP), Some(MADE_UP)] {
            let x = send(r.addr, method, &path, cookie, "");
            assert_eq!(x.status, 404, "{method} {path} {cookie:?}");
            assert!(x.body.is_empty());
        }
    }
    // An unknown id, and a revoked one.
    assert_eq!(send(r.addr, "GET", "/api/file/nope", Some(&vs), "").status, 404);
    assert_eq!(send(r.addr, "GET", &format!("/api/file/{}", "A".repeat(22)), Some(&vs), "").status, 404);
    r.handle.unshare(&two);
    for p in [format!("/api/file/{two}"), format!("/api/thumb/{two}"), format!("/api/view/{two}")] {
        assert_eq!(send(r.addr, "GET", &p, Some(&vs), "").status, 404, "{p}");
    }
    assert_eq!(files_of(&r, &vs).len(), 3, "txt, png, svg");
    assert!(r.events().iter().all(|e| !matches!(e, PhoneEvent::SharedGone { .. })), "an unshare is not a gone file");

    // A file deleted on the PC: 410 and the id is revoked, once told.
    let doomed = r.dir.join("doomed.bin");
    std::fs::write(&doomed, b"soon gone").expect("write");
    let doomed_id = r.handle.share(&doomed).expect("share");
    std::fs::remove_file(&doomed).expect("remove");
    let gone = send(r.addr, "GET", &format!("/api/file/{doomed_id}"), Some(&vs), "");
    assert_eq!((gone.status, gone.body.is_empty()), (410, true));
    wait_for(&r.rx, |e| matches!(e, PhoneEvent::SharedGone { id } if *id == doomed_id));
    assert_eq!(send(r.addr, "GET", &format!("/api/file/{doomed_id}"), Some(&vs), "").status, 404);
    assert!(r.events().iter().all(|e| !matches!(e, PhoneEvent::SharedGone { .. })), "told once");
    // One found missing by the list is left out, revoked and reported.
    let listed = r.dir.join("listed.txt");
    std::fs::write(&listed, b"x").expect("write");
    let listed_id = r.handle.share(&listed).expect("share");
    assert_eq!(files_of(&r, &vs).len(), 4);
    std::fs::remove_file(&listed).expect("remove");
    let now = files_of(&r, &vs);
    assert_eq!(now.len(), 3);
    assert!(now.iter().all(|f| f["id"] != listed_id.as_str()));
    wait_for(&r.rx, |e| matches!(e, PhoneEvent::SharedGone { id } if *id == listed_id));
    // A file replaced by a folder is gone too.
    let swap = r.dir.join("swap.txt");
    std::fs::write(&swap, b"x").expect("write");
    let swap_id = r.handle.share(&swap).expect("share");
    std::fs::remove_file(&swap).expect("remove");
    std::fs::create_dir(&swap).expect("dir");
    assert_eq!(send(r.addr, "GET", &format!("/api/file/{swap_id}"), Some(&vs), "").status, 410);

    // Session end drops every id.
    r.handle.unshare_all();
    assert!(files_of(&r, &vs).is_empty());
    let kept = r.handle.share(&txt).expect("share again");
    assert_ne!(kept, txt_id, "a new id for a path shared again");
    assert_eq!(send(r.addr, "GET", &format!("/api/file/{txt_id}"), Some(&vs), "").status, 404);
    let dir = r.dir.clone();
    let Rig { handle, rx, .. } = r;
    handle.stop();
    wait_for(&rx, |e| matches!(e, PhoneEvent::SessionEnded(EndReason::Done)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file on the network is shared like any other, but not looked at by the
/// caller (the UI thread): its id comes at once, and a thread of its own then
/// looks, and revokes it with `SharedGone` if it is not there. A `\\?\` path
/// counts as the network (any `\\` form does), so the loopback test needs no
/// real share.
#[test]
fn a_file_on_the_network_is_shared_without_being_looked_at_then_revoked_if_gone() {
    let r = rig("phone-e2e-network");
    let (_d, vs) = r.join();
    let local = r.dir.join("denah.pdf");
    std::fs::write(&local, vec![1u8; 4321]).expect("write");
    // `canonicalize` gives the `\\?\C:\…` form.
    let net = std::fs::canonicalize(&local).expect("canonical");
    assert!(crate::shelf::is_network(&net), "{}", net.display());
    let id = r.handle.share(&net).expect("shared at once");
    // The phone gets it like any other file; its size is read when listed.
    assert_eq!(entry_of(&files_of(&r, &vs), &id)["size"], 4321);
    let got = send(r.addr, "GET", &format!("/api/file/{id}"), Some(&vs), "");
    assert_eq!((got.status, got.body.len()), (200, 4321));

    // One that is not there is revoked once it has been looked at, and said
    // once.
    let missing = net.with_file_name("hilang.pdf");
    let gone = r.handle.share(&missing).expect("shared at once");
    let said = wait_for(&r.rx, |e| matches!(e, PhoneEvent::SharedGone { .. }));
    assert_eq!(said, PhoneEvent::SharedGone { id: gone.clone() });
    let ids: Vec<_> = files_of(&r, &vs).iter().map(|f| f["id"].clone()).collect();
    assert_eq!(ids, [serde_json::Value::from(id.as_str())], "only the one that is there");
    assert_eq!(send(r.addr, "GET", &format!("/api/file/{gone}"), Some(&vs), "").status, 404);
    assert!(r.events().iter().all(|e| !matches!(e, PhoneEvent::SharedGone { .. })), "told once");
    let _ = std::fs::remove_dir_all(&r.dir);
}

#[test]
fn a_card_taken_off_cuts_a_download_in_flight_and_a_stalled_one_is_busy() {
    let r = rig("phone-e2e-cut");
    let big = r.dir.join("big.bin");
    // Bigger than the socket buffers, so the server is still writing while
    // the phone has read only a little.
    let bytes: Vec<u8> = (0..24 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&big, &bytes).expect("write");
    let (_d, vs) = r.join();
    let id = r.handle.share(&big).expect("share");

    let mut s = connect(r.addr);
    s.write_all(
        format!("GET /api/file/{id} HTTP/1.1\r\nHost: x\r\nCookie: {vs}\r\n\r\n").as_bytes(),
    )
    .expect("write");
    // Read the head and a little body, then stop reading: the server blocks
    // on a full socket with the file busy for this phone.
    let mut first = vec![0u8; 8192];
    let mut got = 0;
    while got < 1024 {
        let n = s.read(&mut first[got..]).expect("read");
        assert!(n > 0);
        got += n;
    }
    let mut busy = false;
    for _ in 0..50 {
        let f = files_of(&r, &vs);
        if entry_of(&f, &id)["busy"] == true {
            busy = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(busy, "a download in flight is busy");
    assert_eq!(entry_of(&files_of(&r, &vs), &id)["saved"], false);
    // The card leaves the shelf (its id is revoked): the connection is closed
    // mid-body.
    r.handle.unshare(&id);
    let mut rest = Vec::new();
    s.set_read_timeout(Some(Duration::from_secs(20))).expect("timeout");
    let _ = s.read_to_end(&mut rest);
    assert!(
        got + rest.len() < bytes.len(),
        "cut short: {} of {} bytes",
        got + rest.len(),
        bytes.len()
    );
    assert_eq!(send(r.addr, "GET", &format!("/api/file/{id}"), Some(&vs), "").status, 404);
    // Busy is cleared whatever way the stream ended: the set no longer has
    // the id, so a new share shows it free.
    let again = r.handle.share(&big).expect("share again");
    assert_eq!(entry_of(&files_of(&r, &vs), &again)["busy"], false);
    assert_eq!(entry_of(&files_of(&r, &vs), &again)["saved"], false, "a cut download is not saved");
    let dir = r.dir.clone();
    r.handle.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_resume_after_a_cut_download_is_saved_and_ending_the_session_drops_the_ids() {
    let r = rig("phone-e2e-end");
    let f = r.dir.join("a.txt");
    let text: Vec<u8> = (0..100u8).collect();
    std::fs::write(&f, &text).expect("write");
    let (_d, vs) = r.join();
    let id = r.handle.share(&f).expect("share");
    // A half read, then a resume from where it stopped.
    let part = send(r.addr, "GET", &format!("/api/file/{id}"), Some(&vs), "Range: bytes=0-49\r\n");
    assert_eq!(part.status, 206);
    assert!(!saved(&r, &vs, &id));
    let rest = send(r.addr, "GET", &format!("/api/file/{id}"), Some(&vs), "Range: bytes=50-\r\n");
    assert_eq!((rest.status, rest.body.as_slice()), (206, &text[50..]));
    assert!(saved(&r, &vs, &id));
    // A zero length file served in full counts as saved.
    let empty = r.dir.join("empty.txt");
    std::fs::write(&empty, b"").expect("write");
    let empty_id = r.handle.share(&empty).expect("share empty");
    let none = send(r.addr, "GET", &format!("/api/file/{empty_id}"), Some(&vs), "");
    assert_eq!((none.status, none.body.len()), (200, 0));
    assert!(saved(&r, &vs, &empty_id));
    // Any range on it is 416.
    let bad = send(r.addr, "GET", &format!("/api/file/{empty_id}"), Some(&vs), "Range: bytes=0-\r\n");
    assert_eq!(bad.status, 416);
    // Ending the session empties the set (the thread does it before it says so).
    let shared = Arc::clone(&r.handle.shared);
    let dir = r.dir.clone();
    let Rig { handle, rx, .. } = r;
    handle.stop();
    wait_for(&rx, |e| matches!(e, PhoneEvent::SessionEnded(_)));
    assert!(shared.session.lock().expect("lock").shared().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
