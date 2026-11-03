//! `--demo-lyrics-card [scene]` (debug builds): the card on made-up data.
//!
//! Scenes: `synced` (default), `cyrillic`, `cjk`, `loading`, `none`,
//! `nomedia`, `noart`, `long`. A fake playing timeline loops through the
//! lines; the art is a red picture built in memory. `DI_LANG=en` picks
//! English, `DI_CARD_POS=<ms>` freezes the position for screenshots.
//!
//! Nothing real is touched: no media session, no media keys, no user data.
//! Commands (seek, prev, play/pause, next) are printed, never sent; a seek
//! only moves the fake timeline. The place goes to
//! `temp_dir()/vale-lyrics-card-demo.json`, never the real config.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, MSG, PostQuitMessage, SetTimer, TranslateMessage,
    WM_TIMER,
};

use super::{CardContent, Change, Msg, OpenArgs, Status};
use crate::config::{NotchMonitor, UiFont};
use crate::lyrics::LrcLine;
use crate::model::Timeline;

/// A seek the card asked for, in ms (-1: none), for the loop to apply.
static SEEK_TO: AtomicI64 = AtomicI64::new(-1);

fn place_file() -> PathBuf {
    std::env::temp_dir().join("vale-lyrics-card-demo.json")
}

/// Text from code points, so no literal non-Latin character sits in the
/// source (`ui/font_coverage.rs` scans it).
fn s(cps: &[u32]) -> String {
    cps.iter().filter_map(|&c| char::from_u32(c)).collect()
}

fn lines_of(texts: &[String], step_ms: u64) -> Vec<LrcLine> {
    texts
        .iter()
        .enumerate()
        .map(|(i, t)| LrcLine {
            at_ms: 2_000 + i as u64 * step_ms,
            text: t.clone(),
        })
        .collect()
}

fn english() -> Vec<String> {
    [
        "Walking down the avenue tonight",
        "Streetlights painting gold on every wall",
        "",
        "I can hear the city start to hum",
        "Every window has a story to tell",
        "And I am carried by the rhythm of the rain",
        "Tell me where the quiet houses go",
        "When the morning comes to wake them all",
        "",
        "We were only ever passing through",
        "Leaving little echoes as we go",
        "Hold the light a little longer now",
        "Let the whole sky open up for you",
        "Nothing here was ever meant to stay",
        "But the song goes on",
    ]
    .iter()
    .map(|t| (*t).to_owned())
    .collect()
}

fn long_lines() -> Vec<String> {
    [
        "Sampai jadi debu, sampai semua yang pernah kita bicarakan hilang terbawa angin yang entah ke mana arahnya",
        "A very long line that keeps going and going so that it has to wrap onto a second and even a third line inside the card",
        "Pendek",
        "Somewhere between the first light and the last train home there is a moment that belongs to nobody at all",
        "",
        "Dan kita masih di sini, menunggu pagi yang tak pernah datang tepat waktu, seperti biasa",
    ]
    .iter()
    .map(|t| (*t).to_owned())
    .collect()
}

fn cyrillic() -> Vec<String> {
    vec![
        s(&[
            0x41F, 0x440, 0x438, 0x432, 0x435, 0x442, 0x20, 0x43C, 0x438, 0x440,
        ]),
        s(&[
            0x41D, 0x43E, 0x447, 0x44C, 0x2C, 0x20, 0x443, 0x43B, 0x438, 0x446, 0x430, 0x2C, 0x20,
            0x444, 0x43E, 0x43D, 0x430, 0x440, 0x44C,
        ]),
        String::new(),
        s(&[
            0x42F, 0x20, 0x43B, 0x44E, 0x431, 0x43B, 0x44E, 0x20, 0x442, 0x435, 0x431, 0x44F,
        ]),
        s(&[
            0x421, 0x43F, 0x43E, 0x43A, 0x43E, 0x439, 0x43D, 0x43E, 0x439, 0x20, 0x43D, 0x43E,
            0x447, 0x438,
        ]),
        s(&[
            0x414, 0x43E, 0x431, 0x440, 0x44B, 0x439, 0x20, 0x432, 0x435, 0x447, 0x435, 0x440,
            0x20, 0x432, 0x441, 0x435, 0x43C,
        ]),
        "Latin line between".to_owned(),
        s(&[
            0x41D, 0x43E, 0x447, 0x44C, 0x20, 0x43D, 0x435, 0x20, 0x43A, 0x43E, 0x043D, 0x447,
            0x430, 0x435, 0x442, 0x441, 0x44F,
        ]),
    ]
}

fn cjk() -> Vec<String> {
    vec![
        s(&[0x4F60, 0x597D, 0x4E16, 0x754C]),
        s(&[0x591C, 0x7A7A, 0x4E2D, 0x6700, 0x4EAE, 0x7684, 0x661F]),
        String::new(),
        s(&[0x611B, 0x3057, 0x3066, 0x308B]),
        s(&[0x5922, 0x3092, 0x898B, 0x3066, 0x3044, 0x305F]),
        s(&[0xC548, 0xB155, 0xD558, 0xC138, 0xC694]),
        "Latin line between".to_owned(),
        s(&[0x6708, 0x5149, 0x7167, 0x5728, 0x6C34, 0x9762, 0x4E0A]),
    ]
}

/// A 32 x 32 red picture as a BMP (WIC decodes it): a little lighter at the
/// top left, so the thumbnail is not a flat square.
fn red_art() -> Vec<u8> {
    const N: u32 = 32;
    let row = N * 3;
    let mut px = Vec::with_capacity((row * N) as usize);
    // BMP rows run bottom to top.
    for y in (0..N).rev() {
        for x in 0..N {
            let glow = 40u32.saturating_sub((x + y) * 2) as u8;
            px.extend([glow / 3, glow / 2, 190u8.saturating_add(glow)]); // B, G, R
        }
    }
    let size = 14 + 40 + px.len() as u32;
    let mut out = Vec::new();
    out.extend(b"BM");
    out.extend(size.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend((14u32 + 40).to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend((N as i32).to_le_bytes());
    out.extend((N as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend((px.len() as u32).to_le_bytes());
    out.extend([0u8; 16]);
    out.extend(px);
    out
}

struct Scene {
    status: Status,
    lines: Vec<LrcLine>,
    art: bool,
    media: bool,
}

fn scene_of(name: &str) -> Scene {
    let synced = |texts: Vec<String>, step| Scene {
        status: Status::Synced,
        lines: lines_of(&texts, step),
        art: true,
        media: true,
    };
    match name {
        "cyrillic" => synced(cyrillic(), 4_000),
        "cjk" => synced(cjk(), 4_000),
        "long" => synced(long_lines(), 5_000),
        "loading" => Scene {
            status: Status::Pending,
            lines: Vec::new(),
            art: true,
            media: true,
        },
        "none" => Scene {
            status: Status::Missing,
            lines: Vec::new(),
            art: true,
            media: true,
        },
        "nomedia" => Scene {
            status: Status::NoMedia,
            lines: Vec::new(),
            art: false,
            media: false,
        },
        "noart" => Scene {
            art: false,
            ..synced(english(), 4_000)
        },
        _ => synced(english(), 4_000),
    }
}

fn content(sc: &Scene, first: bool, timeline: Option<Timeline>, frozen: bool) -> CardContent {
    CardContent {
        status: sc.status,
        title: if sc.media { "Senja di Kemang".into() } else { String::new() },
        artist: if sc.media { "Studio Ruang".into() } else { String::new() },
        art: if first {
            Change::Set(sc.art.then(red_art))
        } else {
            Change::Keep
        },
        lines: if first {
            Change::Set(Arc::new(sc.lines.clone()))
        } else {
            Change::Keep
        },
        timeline,
        playing: sc.media && !frozen,
        can_seek: sc.media,
        session_key: 1,
        palette: super::fallback_palette(),
        font: UiFont::Sans,
        animations: true,
    }
}

fn timeline(sc: &Scene, from_ms: u64) -> Option<Timeline> {
    sc.media.then(|| Timeline {
        position_ms: from_ms,
        end_ms: sc.lines.last().map_or(40_000, |l| l.at_ms) + 12_000,
        updated: Instant::now(),
        rate: 1.0,
    })
}

/// The card's command sink in the demo.
pub fn emit(msg: Msg, value: isize) {
    match msg {
        Msg::Close => {
            println!("close");
            unsafe { PostQuitMessage(0) };
        }
        Msg::Seek => {
            println!("seek {value} ms (not sent; the fake timeline jumps)");
            SEEK_TO.store(value as i64, Ordering::SeqCst);
        }
        Msg::Prev => println!("prev (not sent)"),
        Msg::PlayPause => println!("play/pause (not sent)"),
        Msg::Next => println!("next (not sent)"),
        Msg::Placed => {
            if let Some(p) = super::take_placed()
                && let Ok(json) = serde_json::to_string(&p)
            {
                let _ = std::fs::write(place_file(), &json);
                println!("placed {json} -> {}", place_file().display());
            }
        }
    }
}

pub fn run(scene: Option<&str>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let sc = scene_of(scene.unwrap_or("synced"));
    let frozen_ms: Option<u64> = std::env::var("DI_CARD_POS")
        .ok()
        .and_then(|v| v.trim().parse().ok());
    let displays = crate::arrange_win::displays();
    let notch = crate::notch_monitor::resolve(&NotchMonitor::Primary, &displays);
    let (monitors, notch) = super::monitors_of(&displays, notch);
    let saved = std::fs::read_to_string(place_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .and_then(crate::config::card_place_from_json);
    let start = frozen_ms.unwrap_or(0);
    let args = OpenArgs {
        island: HWND::default(),
        saved,
        monitors,
        notch,
        hidden: false,
        demo: true,
        frozen_ms,
    };
    if !super::open(args, content(&sc, true, timeline(&sc, start), frozen_ms.is_some())) {
        eprintln!("vale: the lyrics card could not open");
        return;
    }
    println!(
        "lyrics card demo `{}`: click a line to seek (fake), Esc or X to quit",
        scene.unwrap_or("synced")
    );
    // The demo may poll: a one-second timer loops the fake timeline.
    let timer = unsafe { SetTimer(None, 0, 1000, None) };
    let mut began = Instant::now();
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let seek = SEEK_TO.swap(-1, Ordering::SeqCst);
        let looped = frozen_ms.is_none()
            && sc.media
            && msg.message == WM_TIMER
            && timeline(&sc, 0).is_some_and(|t| began.elapsed() >= Duration::from_millis(t.end_ms));
        if seek >= 0 && frozen_ms.is_none() {
            began = Instant::now() - Duration::from_millis(seek as u64);
            super::update(content(&sc, false, timeline(&sc, seek as u64), false));
        } else if looped {
            began = Instant::now();
            super::update(content(&sc, false, timeline(&sc, 0), false));
        }
    }
    unsafe {
        let _ = KillTimer(None, timer);
    }
    super::release();
}
