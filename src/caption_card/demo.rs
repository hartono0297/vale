//! `--demo-captions [scene]` (debug builds): the caption card on made-up
//! lines.
//!
//! Scenes: `two` (default: "Saya" / "Peserta" and one grey line), `one` (one
//! source, no labels), `long` (long wrapped lines to scroll back through),
//! `listening`, `needs-transkrip`, `needs-model`, `downloading`, `paused`,
//! `battery`, `gap` (a skipped stretch and the slow hint), `failed`, `cjk`.
//! `DI_LANG=en` picks English, `DI_CAP_SIZE=s|m|l` the text size,
//! `DI_CAP_STILL=1` turns the easing off.
//!
//! Nothing real is touched: no audio, no model, no clipboard, no config. The
//! buttons act on the demo only (pause and resume flip the status, the chip
//! steps the language, Ctrl + wheel the size); what was clicked is printed,
//! never a caption. The place goes to `temp_dir()/vale-caption-card-demo.json`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage,
};

use super::{CardContent, Change, Msg, OpenArgs};
use crate::captions::control::{CardStatus, next_lang};
use crate::captions::lines::Line;
use crate::captions::prefs::TextSize;
use crate::config::{NotchMonitor, UiFont};
use crate::transcribe::output::Speaker;
use crate::transcribe::prefs::TrLang;

/// What the card asked for, for the loop to apply.
static PENDING: Mutex<Vec<(Msg, isize)>> = Mutex::new(Vec::new());

fn place_file() -> PathBuf {
    std::env::temp_dir().join("vale-caption-card-demo.json")
}

/// Text from code points, so no literal non-Latin character sits in the
/// source (`ui/font_coverage.rs` scans it).
fn s(cps: &[u32]) -> String {
    cps.iter().filter_map(|&c| char::from_u32(c)).collect()
}

struct Scene {
    status: CardStatus,
    lines: Vec<Line>,
    interim: Vec<(Option<Speaker>, String)>,
    labelled: bool,
    slow_hint: bool,
}

fn lines_of(texts: &[(Option<Speaker>, String)]) -> Vec<Line> {
    texts
        .iter()
        .enumerate()
        .map(|(i, (speaker, text))| Line {
            speaker: *speaker,
            text: text.clone(),
            start_ms: i as u64 * 4_000,
            gap: false,
        })
        .collect()
}

fn gap_line(after: &[Line]) -> Line {
    Line {
        speaker: None,
        text: s(&[0x2026]),
        start_ms: after.last().map_or(0, |l| l.start_ms),
        gap: true,
    }
}

fn meeting() -> Vec<(Option<Speaker>, String)> {
    let (me, them) = (Some(Speaker::Me), Some(Speaker::Others));
    [
        (them, "Oke, untuk sprint ini kita fokus ke onboarding dulu."),
        (
            me,
            "Setuju. Design-nya udah final, tinggal review sama tim dev.",
        ),
        (
            them,
            "Can we push the release to next week? QA masih nemu dua bug.",
        ),
    ]
    .into_iter()
    .map(|(who, t)| (who, t.to_owned()))
    .collect()
}

fn video() -> Vec<(Option<Speaker>, String)> {
    [
        "Jadi hari ini kita bahas cara kerja kompresor di kulkas.",
        "Gas pendingin ditekan sampai panas, lalu dilepas panasnya di belakang.",
        "Begitu tekanannya turun, suhunya ikut turun jauh di bawah nol.",
    ]
    .into_iter()
    .map(|t| (None, t.to_owned()))
    .collect()
}

fn long_talk() -> Vec<(Option<Speaker>, String)> {
    let (me, them) = (Some(Speaker::Me), Some(Speaker::Others));
    let mut out = Vec::new();
    for round in 1..=6 {
        out.push((
            them,
            format!(
                "Poin {round}: kalau kita lihat angka minggu lalu, yang paling banyak makan waktu itu bukan build-nya, tapi nunggu review dari dua tim sekaligus sebelum bisa merge."
            ),
        ));
        out.push((
            me,
            format!(
                "Catatan {round}: aku usul review-nya dibagi per modul, jadi satu orang cukup pegang satu bagian dan gak perlu baca semua perubahan dari awal sampai akhir."
            ),
        ));
        out.push((them, "Oke, masuk akal.".to_owned()));
    }
    out
}

fn cjk() -> Vec<(Option<Speaker>, String)> {
    let (me, them) = (Some(Speaker::Me), Some(Speaker::Others));
    vec![
        (
            them,
            s(&[
                0x4ECA, 0x65E5, 0x306E, 0x4F1A, 0x8B70, 0x3092, 0x59CB, 0x3081, 0x307E, 0x3059,
            ]),
        ),
        (
            me,
            s(&[
                0x597D, 0x7684, 0xFF0C, 0x6211, 0x4EEC, 0x5F00, 0x59CB, 0x5427,
            ]),
        ),
        (
            them,
            s(&[
                0xC548, 0xB155, 0xD558, 0xC138, 0xC694, 0x2C, 0x20, 0xC2DC, 0xC791, 0xD560, 0xAE4C,
                0xC694,
            ]),
        ),
        (me, "Latin line between".to_owned()),
    ]
}

fn scene_of(name: &str) -> Scene {
    let me = Some(Speaker::Me);
    let running = |texts: Vec<(Option<Speaker>, String)>, labelled, interim| Scene {
        status: CardStatus::Running,
        lines: lines_of(&texts),
        interim,
        labelled,
        slow_hint: false,
    };
    let bare = |status| Scene {
        status,
        lines: Vec::new(),
        interim: Vec::new(),
        labelled: false,
        slow_hint: false,
    };
    let live = vec![(me, "Boleh, jadi deadline-nya Jumat ya".to_owned())];
    match name {
        "one" => running(
            video(),
            false,
            vec![(None, "Dari situ udara dingin ditiup ke".to_owned())],
        ),
        "long" => running(long_talk(), true, live),
        "listening" => bare(CardStatus::Listening),
        "needs-transkrip" => bare(CardStatus::NeedsTranskrip),
        "needs-model" => bare(CardStatus::NeedsModel),
        "downloading" => bare(CardStatus::Downloading(42)),
        "battery" => bare(CardStatus::Battery),
        "paused" => Scene {
            status: CardStatus::Paused,
            ..running(meeting(), true, Vec::new())
        },
        "failed" => Scene {
            status: CardStatus::Failed("device lost".to_owned()),
            ..running(meeting(), true, Vec::new())
        },
        "gap" => {
            let mut sc = running(meeting(), true, live);
            let gap = gap_line(&sc.lines[..2]);
            sc.lines.insert(2, gap);
            sc.slow_hint = true;
            sc
        }
        "cjk" => running(cjk(), true, Vec::new()),
        _ => running(meeting(), true, live),
    }
}

/// What the demo's buttons change.
struct Live {
    status: CardStatus,
    lang: TrLang,
    size: TextSize,
    since: Instant,
}

fn content(sc: &Scene, live: &Live, first: bool) -> CardContent {
    let runs = matches!(live.status, CardStatus::Running | CardStatus::Listening);
    CardContent {
        status: live.status.clone(),
        lines: if first {
            Change::Set(Arc::new(sc.lines.clone()))
        } else {
            Change::Keep
        },
        interim: if runs { sc.interim.clone() } else { Vec::new() },
        labelled: sc.labelled,
        running_since: runs.then_some(live.since),
        lang: live.lang,
        size: live.size,
        slow_hint: sc.slow_hint,
        palette: crate::lyrics_card::fallback_palette(),
        font: UiFont::Sans,
        // `DI_CAP_STILL=1`: as with the Windows animations turned off.
        animations: std::env::var_os("DI_CAP_STILL").is_none(),
    }
}

/// The card's command sink in the demo.
pub fn emit(msg: Msg, value: isize) {
    match msg {
        Msg::Close => {
            println!("close");
            unsafe { PostQuitMessage(0) };
        }
        Msg::Placed => {
            if let Some(p) = super::take_placed()
                && let Ok(json) = serde_json::to_string(&p)
            {
                let _ = std::fs::write(place_file(), &json);
                println!("placed {json} -> {}", place_file().display());
            }
        }
        _ => {
            if let Ok(mut q) = PENDING.lock() {
                q.push((msg, value));
            }
        }
    }
}

pub fn run(scene: Option<&str>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let name = scene.unwrap_or("two");
    let sc = scene_of(name);
    let size = match std::env::var("DI_CAP_SIZE").ok().as_deref().map(str::trim) {
        Some("s") => TextSize::Small,
        Some("l") => TextSize::Large,
        _ => TextSize::Medium,
    };
    let mut live = Live {
        status: sc.status.clone(),
        lang: TrLang::Id,
        size,
        // A session that has run for a while, so the clock shows digits.
        since: Instant::now() - std::time::Duration::from_secs(12 * 60 + 48),
    };
    let displays = crate::arrange_win::displays();
    let notch = crate::notch_monitor::resolve(&NotchMonitor::Primary, &displays);
    let (monitors, notch) = crate::lyrics_card::monitors_of(&displays, notch);
    let saved = std::fs::read_to_string(place_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .and_then(crate::config::card_place_from_json);
    let args = OpenArgs {
        island: HWND::default(),
        saved,
        monitors,
        notch,
        demo: true,
    };
    if !super::open(args, content(&sc, &live, true)) {
        eprintln!("vale: the caption card could not open");
        return;
    }
    println!(
        "caption card demo `{name}`: X to quit, wheel scrolls back, Ctrl + wheel sizes the text"
    );
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let asked: Vec<(Msg, isize)> = PENDING
            .lock()
            .map(|mut q| q.drain(..).collect())
            .unwrap_or_default();
        if asked.is_empty() {
            continue;
        }
        for (m, value) in asked {
            match m {
                Msg::Copy => println!("copy (nothing is copied in the demo)"),
                Msg::Pause => {
                    println!("pause");
                    live.status = CardStatus::Paused;
                }
                Msg::Resume => {
                    println!("resume");
                    live.status = if sc.lines.is_empty() {
                        CardStatus::Listening
                    } else {
                        CardStatus::Running
                    };
                }
                Msg::Lang => {
                    live.lang = next_lang(live.lang);
                    println!("language -> {:?}", live.lang);
                }
                Msg::Action => {
                    println!("action for {:?} (nothing happens in the demo)", live.status)
                }
                Msg::Size => {
                    live.size = live.size.step(value == 1);
                    println!("size -> {:?}", live.size);
                }
                Msg::Close | Msg::Placed => {}
            }
        }
        super::update(content(&sc, &live, false));
    }
    super::release();
}
