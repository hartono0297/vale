//! `--phone-debug` (debug builds only): the phone link on the loopback
//! address with the files going to `target/phone-debug/`, so the page and the
//! whole flow can be tried from a browser on this PC without a phone, a
//! network, or the real Downloads folder. It prints the link and every
//! pairing's code to the console, and approves each pairing two seconds
//! after it shows, so the waiting screen is seen first. The language comes
//! from `DI_LANG`. Runs until the process is killed.
//!
//! Once the server listens, every regular file of
//! `target/phone-debug/share/` is shared with the phones (made-up ones are
//! written there first when the folder has no file), and each id is printed,
//! so the "Dari laptop" tab can be tried: the list, the thumbnails, a save, a
//! resume with `curl -r`, an id that is revoked. Nothing outside `target/` is
//! read or written, the real Downloads folder least of all.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Handle, PORTS, PhoneEvent, ServerConfig, pc_name, start};
use crate::i18n;

/// How long a pairing waits before it is approved.
const APPROVE_AFTER: Duration = Duration::from_secs(2);

/// A 640 x 480 colour gradient as a PNG (WIC, with COM joined for the call).
fn gradient_png() -> Option<Vec<u8>> {
    use windows::Win32::System::Com::{
        COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
    };
    const W: u32 = 640;
    const H: u32 = 480;
    let mut bgra = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            bgra.extend_from_slice(&[
                (y * 255 / H) as u8,
                (255 - x * 255 / W) as u8,
                (x * 255 / W) as u8,
                255,
            ]);
        }
    }
    let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let png = crate::screenshot::win::encode_png(W, H, &bgra);
    if joined {
        unsafe { CoUninitialize() };
    }
    png
}

/// The files of `dir` (not folders), by name.
fn regular_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Shares what is in `dir`, first making three made-up files when it has
/// none: a PNG, a PDF-named blob of about 300 KB and a text file with a
/// non-ASCII name.
fn share_made_up(handle: &Handle, dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    if regular_files(dir).is_empty() {
        if let Some(png) = gradient_png() {
            let _ = std::fs::write(dir.join("foto-lokasi.png"), png);
        }
        let blob: Vec<u8> = (0..300 * 1024u32).map(|i| (i.wrapping_mul(31) >> 3) as u8).collect();
        let _ = std::fs::write(dir.join("denah-lantai-2.pdf"), blob);
        let _ = std::fs::write(
            dir.join("catatan-ruang-\u{e9}.txt"),
            "Catatan ruang: made-up text for the phone-debug flag.\n",
        );
    }
    for path in regular_files(dir) {
        match handle.share(&path) {
            Some(info) => println!(
                "phone-debug: sharing {} ({} bytes), id {}",
                path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
                info.size,
                info.id
            ),
            None => println!("phone-debug: could not share {}", path.display()),
        }
    }
}

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
                PhoneEvent::Ready { url, .. } => {
                    println!("phone-debug: open {url}");
                    share_made_up(&handle, &dest.join("share"));
                }
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
