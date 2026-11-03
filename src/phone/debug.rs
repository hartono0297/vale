//! `--phone-debug` (debug builds only): the phone link on the loopback
//! address with the files going to `target/phone-debug/`, so the page and the
//! whole flow can be tried from a browser on this PC without a phone, a
//! network, or the real Downloads folder. It prints the link and every
//! pairing's code to the console, and approves each pairing two seconds
//! after it shows, so the waiting screen is seen first. The language comes
//! from `DI_LANG`. Runs until the process is killed.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{PORTS, PhoneEvent, ServerConfig, pc_name, start};
use crate::i18n;

/// How long a pairing waits before it is approved.
const APPROVE_AFTER: Duration = Duration::from_secs(2);

pub fn run() {
    let dest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("phone-debug");
    let cfg = ServerConfig {
        bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
        ports: PORTS,
        dest_dir: dest.clone(),
        lang: i18n::lang(),
        pc_name: pc_name(),
        network_name: "phone-debug (loopback)".into(),
        allow_loopback: true,
    };
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let handle = start(
        cfg,
        Arc::new(move |event| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(event);
            }
        }),
    );
    println!("phone-debug: files go to {}", dest.display());
    let mut due: Vec<(u64, Instant)> = Vec::new();
    loop {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
            match event {
                PhoneEvent::Ready { url, .. } => println!("phone-debug: open {url}"),
                PhoneEvent::Refused(why) => {
                    println!("phone-debug: refused: {why:?}");
                    return;
                }
                PhoneEvent::Pairing {
                    device_id, code, ..
                } => {
                    println!("phone-debug: pairing {device_id}, code {code}, approving in 2 s");
                    due.push((device_id, Instant::now() + APPROVE_AFTER));
                }
                PhoneEvent::SessionEnded(reason) => {
                    println!("phone-debug: session ended: {reason:?}");
                    return;
                }
                other => println!("phone-debug: {other:?}"),
            }
        }
        let now = Instant::now();
        due.retain(|&(id, at)| {
            if at <= now {
                handle.approve(id);
                false
            } else {
                true
            }
        });
    }
}
