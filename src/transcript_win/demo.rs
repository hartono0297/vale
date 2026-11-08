//! `--demo-transcript [scene]` (debug builds): the transcript window on
//! made-up files, with no engine and no island.
//!
//! Scenes: `lines` (default), `running`, `empty`, `failed`, `queued`, `off`,
//! `long` (2,000 lines). Each writes its fixture (`.srt` and `.vale.json`, no
//! audio) under `temp_dir()\vale-transcript-demo\<scene>\`. `DI_WINDOW`,
//! `DI_SURFACE` and `DI_ACCENT` pick the theme, `DI_LANG=en` English, and
//! `DI_TRANSCRIPT_POS=<ms>` puts the player at that time (the playing line).
//! What the window asks for is printed, never run; the place is not saved.

use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG, TranslateMessage};

use super::{Request, Status, View};
use crate::config::UiFont;
use crate::i18n::{self, Key, Lang, text};
use crate::theme;
use crate::transcribe::doc;
use crate::transcribe::output::{self, Segment, Speaker};

const SAMPLE: [(Speaker, &str); 12] = [
    (Speaker::Others, "Oke, kita mulai ya. Agenda hari ini tiga hal."),
    (Speaker::Others, "Pertama status onboarding, kedua release, ketiga budget Q1."),
    (Speaker::Me, "Onboarding design udah final, link Figma gw share di chat."),
    (Speaker::Others, "Sip. Dari sisi engineering, halaman daftar sudah jalan di staging, tinggal halaman verifikasi email yang masih nunggu copy dari tim konten."),
    (Speaker::Me, "Copy-nya gw kirim hari ini sebelum jam empat."),
    (Speaker::Others, "Can we push the release to next week? QA masih nemu dua bug."),
    (Speaker::Me, "Boleh, jadi deadline-nya Jumat ya."),
    (Speaker::Others, "Bug pertama di halaman pembayaran, kalau kartu ditolak pesannya kosong. Bug kedua cuma di Safari."),
    (Speaker::Me, "Yang Safari bisa nunggu, yang pembayaran harus beres dulu."),
    (Speaker::Others, "Setuju. Terakhir budget: kita butuh dua lisensi tambahan buat desainer baru."),
    (Speaker::Me, "Gw cek sama finance besok pagi, nanti gw kabarin di grup."),
    (Speaker::Others, "Oke, makasih semua. Sampai minggu depan."),
];

fn lines(n: usize) -> Vec<Segment> {
    (0..n)
        .map(|i| {
            let (who, t) = SAMPLE[i % SAMPLE.len()];
            let start = 2_000 + i as u64 * 7_300;
            Segment {
                start_ms: start,
                end_ms: start + 6_500,
                speaker: Some(who),
                text: t.to_owned(),
            }
        })
        .collect()
}

/// Writes the scene's sidecar (and `.srt` when it has lines) and returns the
/// audio's path.
fn fixture(scene: &str, segments: &[Segment], transcript: bool, lang: Lang) -> PathBuf {
    let dir = std::env::temp_dir().join("vale-transcript-demo").join(scene);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let audio = dir.join("2026-11-07 1002 Teams.m4a");
    let files = output::paths(&audio);
    let duration = segments.last().map_or(42 * 60_000 + 7_000, |s| s.end_ms + 3_000);
    let mut sidecar = serde_json::json!({
        "v": 1,
        "title": "Sync mingguan",
        "app": "Teams",
        "started": "2026-11-07T10:02:00+07:00",
        "duration_ms": duration,
        "channels": ["mic", "system"],
    });
    if transcript {
        sidecar["transcript"] = serde_json::json!({
            "model": "whisper-large-v3-turbo",
            "engine": "transcribe.cpp",
            "language": "id",
            "segments": segments.len(),
        });
        if !segments.is_empty() {
            let _ = std::fs::write(&files.srt, output::srt(segments, doc::labels(lang)));
        }
    }
    let _ = std::fs::write(&files.sidecar, sidecar.to_string());
    audio
}

fn view_of(scene: &str, lang: Lang) -> Option<View> {
    let (segments, transcript, status) = match scene {
        "lines" => (lines(SAMPLE.len() * 3), true, Status::None),
        "long" => (lines(2_000), true, Status::None),
        "running" => (
            lines(7),
            false,
            Status::Running {
                pct: 42,
                eta: Some(Duration::from_secs(6 * 60)),
            },
        ),
        "empty" => (Vec::new(), true, Status::None),
        "failed" => (Vec::new(), false, Status::Failed(text(lang, Key::TrWhyAudio).to_owned())),
        "queued" => (Vec::new(), false, Status::Queued),
        "off" => (Vec::new(), false, Status::Off),
        _ => return None,
    };
    let audio = fixture(scene, &segments, transcript, lang);
    let mut doc = doc::load(&audio, "Rekaman");
    if scene == "running" {
        // A running job's lines come from the worker, not the files.
        doc.segments = segments;
    }
    Some(View {
        doc,
        status,
        can_redo: scene != "off",
        lang,
        font: UiFont::Sans,
    })
}

fn describe(audio: &Path, req: &Request) {
    println!("transcript demo: {req:?} for {}", audio.display());
}

pub fn run(scene: Option<&str>) {
    let scene = scene.unwrap_or("lines");
    theme::set_current(theme::theme_from_env());
    theme::set_system_light(theme::read_system_light());
    let lang = i18n::lang();
    let Some(view) = view_of(scene, lang) else {
        eprintln!("vale: unknown scene `{scene}` (lines, running, empty, failed, queued, off, long)");
        return;
    };
    let audio = view.doc.audio.clone();
    if !super::open(HWND::default(), view, None) {
        eprintln!("vale: the transcript window could not open");
        return;
    }
    if let Some(ms) = std::env::var("DI_TRANSCRIPT_POS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
    {
        super::window::demo_position(&audio, ms);
    }
    println!("transcript demo `{scene}`: requests are printed, Esc or close to quit");
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        for (audio, req) in super::take_requests() {
            describe(&audio, &req);
        }
        if !super::is_open(&audio) {
            break;
        }
    }
    super::release();
}
