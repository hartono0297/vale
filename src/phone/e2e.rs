//! One end-to-end test over the loopback: the real server thread on
//! 127.0.0.1, a plain `TcpStream` as the phone. It binds no other address,
//! reads no adapters and never touches the real Downloads folder.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{EndReason, Outcome, PhoneEvent, ServerConfig, start};
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

#[test]
fn a_phone_pairs_uploads_is_cancelled_and_the_server_stops() {
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
    let ua = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 Chrome/126.0.0.0 Mobile Safari/537.36";

    // The QR's address: the page, a pairing cookie and a pairing event.
    let r = get(addr, &format!("/s/{token}"), None, ua);
    assert_eq!(r.status, 200);
    assert!(r.body.contains("TEST-PC"), "boot data is in the page");
    assert!(r.body.contains("\"state\":\"waiting\""));
    assert!(!r.body.contains("/*BOOT*/"), "the placeholder is replaced");
    assert!(r.head.contains("Cache-Control: no-store"));
    let vp = cookie_of(&r, "vp");
    assert!(r.head.contains("HttpOnly; SameSite=Strict"));
    let PhoneEvent::Pairing {
        device_id,
        code,
        kind,
        ..
    } = wait_for(&rx, |e| matches!(e, PhoneEvent::Pairing { .. }))
    else {
        unreachable!()
    };
    assert_eq!(kind, super::DeviceKind::Android);
    let s = get(addr, "/api/state", Some(&vp), ua);
    assert_eq!(s.status, 200);
    assert!(s.body.contains("\"state\":\"waiting\"") && s.body.contains(&code));
    // Not yet: the pairing cookie opens no upload.
    let early = request(addr, &upload_head("a.txt", "b0", 0, 1, 1, &vp), b"x");
    assert_eq!((early.status, early.body.as_str()), (404, ""));

    // The owner says yes; the next state answer gives the session cookie.
    handle.approve(device_id);
    wait_for(&rx, |e| {
        matches!(e, PhoneEvent::PairingEnded { outcome: Outcome::Approved, .. })
    });
    let s = get(addr, "/api/state", Some(&vp), ua);
    assert!(s.body.contains("\"state\":\"approved\""));
    let vs = cookie_of(&s, "vs");
    assert!(s.head.contains("Set-Cookie: vp=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"));
    // A second tab (or a reload racing that poll) still sends `vp`: it is
    // told "approved" and cleared, and no other `vs` replaces the first.
    let again = get(addr, "/api/state", Some(&vp), ua);
    assert!(again.body.contains("\"state\":\"approved\""));
    assert!(!again.head.contains("Set-Cookie: vs="));
    assert!(again.head.contains("Set-Cookie: vp=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"));
    let page = get(addr, "/", Some(&vs), ua);
    assert_eq!(page.status, 200);

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

    // Everything else is a bare 404.
    let bare = |r: Reply| assert_eq!((r.status, r.body.as_str()), (404, ""), "{}", r.head);
    bare(request(addr, &upload_head("n.txt", "b2", 0, 1, 1, "x=y"), b"x"));
    bare(request(addr, &upload_head("n.txt", "b2", 0, 1, 1, &vp), b"x"));
    bare(get(addr, "/etc/passwd", Some(&vs), ua));
    bare(get(addr, "/api/state", None, ua));
    bare(get(addr, "/", None, ua));
    bare(get(addr, &format!("/s/{}", "A".repeat(22)), None, ua));
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
    let r = get(addr, &format!("/s/{token}"), None, "x");
    let vp = cookie_of(&r, "vp");
    let PhoneEvent::Pairing { device_id, .. } =
        wait_for(&rx, |e| matches!(e, PhoneEvent::Pairing { .. }))
    else {
        unreachable!()
    };
    handle.approve(device_id);
    let s = get(addr, "/api/state", Some(&vp), "x");
    let vs = cookie_of(&s, "vs");
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
fn a_phone_that_was_turned_down_scans_again_and_gets_a_new_pairing() {
    let dest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp")
        .join(format!("phone-e2e-retry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    let cfg = ServerConfig {
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        ports: 0..=0,
        dest_dir: dest.clone(),
        lang: Lang::En,
        pc_name: "TEST-PC".into(),
        network_name: "Test net".into(),
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
    let ua = "Mozilla/5.0 (Linux; Android 14; Pixel 8) Chrome/126.0.0.0 Mobile Safari/537.36";

    let r = get(addr, &format!("/s/{token}"), None, ua);
    let vp = cookie_of(&r, "vp");
    let PhoneEvent::Pairing { device_id: first, .. } =
        wait_for(&rx, |e| matches!(e, PhoneEvent::Pairing { .. }))
    else {
        unreachable!()
    };
    handle.reject(first);
    wait_for(&rx, |e| matches!(e, PhoneEvent::PairingEnded { outcome: Outcome::Rejected, .. }));
    let s = get(addr, "/api/state", Some(&vp), ua);
    assert!(s.body.contains("\"state\":\"rejected\""));

    // The same phone scans the QR again with its old cookie.
    let again = get(addr, &format!("/s/{token}"), Some(&vp), ua);
    assert_eq!(again.status, 200);
    assert!(again.body.contains("\"state\":\"waiting\""), "a new pairing, not the old answer");
    let vp2 = cookie_of(&again, "vp");
    assert_ne!(vp2, vp);
    let PhoneEvent::Pairing { device_id: second, .. } =
        wait_for(&rx, |e| matches!(e, PhoneEvent::Pairing { .. }))
    else {
        unreachable!()
    };
    assert_ne!(second, first);
    handle.approve(second);
    wait_for(&rx, |e| matches!(e, PhoneEvent::PairingEnded { outcome: Outcome::Approved, .. }));
    assert!(get(addr, "/api/state", Some(&vp2), ua).body.contains("\"state\":\"approved\""));

    // With a bad token the old cookie still gets the "rejected" page.
    let bad = get(addr, &format!("/s/{}", "A".repeat(22)), Some(&vp), ua);
    assert_eq!(bad.status, 200);
    assert!(bad.body.contains("\"state\":\"rejected\""));

    handle.stop();
    let _ = std::fs::remove_dir_all(&dest);
}
