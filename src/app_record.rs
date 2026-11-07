//! The island's side of the audio recorder (`crate::recorder`): start and
//! stop (the Rekam page, the hotkey and the tray all come here), pause, the
//! levels, the notices, the list of recent recordings and the tray's red dot.
//! A child of `app`, so it works on `App` directly.
//!
//! Power: no thread and no timer exist outside a recording. The recorder's
//! own threads (`rec-capture`, `rec-writer`) live from Start to Stop; the UI
//! only reads three atomics at draw time (`levels`), and the clock and the
//! meter redraws are the `TIMER_REC` / `TIMER_REC_LEVEL` one-shots of
//! `sync_rec_timers`. The recent list is read when the Rekam page comes on
//! screen and after a save, never at startup and never on a timer.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, NaiveDateTime};

use super::{App, WM_APP_EVENT, notif_open, poster};
use crate::i18n::{self, Key, Lang, text};
use crate::model::{Event, Notice, NoticeKind, Page, RecEntry, RecSources, RecStatus, View};
use crate::recorder::sidecar::Sidecar;
use crate::recorder::{RecEvent, RecOptions, Recorder, Source, Sources};
use crate::shortcuts;
use crate::ui::tray;

/// How many recordings the Rekam page lists.
pub(super) const RECENT_LIMIT: usize = 5;
/// The consent notice's time (the default notice time).
const MIC_SETTINGS: &str = "ms-settings:privacy-microphone";

/// What the island keeps of the recorder: the running session, and whether
/// the Rekam page was on screen at the last refresh (a new showing re-reads
/// the folder).
#[derive(Default)]
pub(super) struct RecState {
    recorder: Option<Recorder>,
    page_shown: bool,
}

impl RecState {
    pub fn active(&self) -> bool {
        self.recorder.is_some()
    }
}

// ---- notices (pure) ----------------------------------------------------------

fn notice(key: &str, lang: Lang, title: String, body: String, kind: NoticeKind) -> Notice {
    Notice {
        key: key.into(),
        source: text(lang, Key::RecNoticeSource).into(),
        title,
        body,
        kind,
    }
}

/// "Rekaman tersimpan" over "42 mnt · klik untuk buka". A click shows the
/// file in Explorer.
pub(crate) fn saved_notice(lang: Lang, path: PathBuf, duration: Duration) -> Notice {
    notice(
        "rec-saved",
        lang,
        text(lang, Key::RecSavedTitle).into(),
        format!(
            "{} \u{b7} {}",
            i18n::rec_duration(lang, duration),
            text(lang, Key::RecClickToOpen)
        ),
        NoticeKind::RecSaved { path },
    )
}

/// "Rekaman gagal" with what went wrong.
pub(crate) fn failed_notice(lang: Lang, reason: &str) -> Notice {
    notice(
        "rec-failed",
        lang,
        text(lang, Key::RecFailedTitle).into(),
        reason.trim().to_owned(),
        NoticeKind::RecFailed,
    )
}

/// The mic was refused: a click opens Windows' mic privacy page.
pub(crate) fn mic_denied_notice(lang: Lang) -> Notice {
    notice(
        "rec-mic-denied",
        lang,
        text(lang, Key::RecMicDeniedTitle).into(),
        text(lang, Key::RecMicDeniedBody).into(),
        NoticeKind::RecMicDenied,
    )
}

/// A source dropped out; the recording goes on.
pub(crate) fn source_lost_notice(lang: Lang, source: Source) -> Notice {
    let title = match source {
        Source::Mic => Key::RecMicLost,
        Source::System => Key::RecSystemLost,
    };
    notice(
        "rec-lost",
        lang,
        text(lang, title).into(),
        text(lang, Key::RecKeepsGoing).into(),
        NoticeKind::RecInfo,
    )
}

/// The one-time "ask everyone first" note.
pub(crate) fn consent_notice(lang: Lang) -> Notice {
    notice(
        "rec-consent",
        lang,
        text(lang, Key::RecRecording).into(),
        text(lang, Key::RecConsent).into(),
        NoticeKind::RecInfo,
    )
}

// ---- the recent list ---------------------------------------------------------

/// The `limit` newest `.m4a` files of `dir` as the page lists them. The
/// title and the start come from the sidecar; a file with none shows its
/// name and its file time, and an unknown length is zero (the page draws a
/// dash). Never fails: an unreadable folder is an empty list.
pub(super) fn scan_recent(dir: &Path, lang: Lang, today: NaiveDate, limit: usize) -> Vec<RecEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    // `DirEntry::metadata` is free on Windows (the listing carries it), so
    // the cost is the sort; only the newest few files are opened further.
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = read
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("m4a"))
        })
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| (meta.modified().unwrap_or(std::time::UNIX_EPOCH), e.path()))
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.truncate(limit);
    files
        .into_iter()
        .map(|(modified, path)| entry_of(&path, modified, lang, today))
        .collect()
}

fn entry_of(path: &Path, modified: std::time::SystemTime, lang: Lang, today: NaiveDate) -> RecEntry {
    let side = sidecar_of(path);
    let file_time: NaiveDateTime = chrono::DateTime::<Local>::from(modified).naive_local();
    // A sidecar with no start (epoch) says nothing about when.
    let started = side
        .as_ref()
        .filter(|s| s.started.timestamp() > 0)
        .map_or(file_time, |s| s.started.with_timezone(&Local).naive_local());
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = match &side {
        Some(s) if !s.title.trim().is_empty() => s.title.trim().to_owned(),
        Some(_) => format!(
            "{} {} {}",
            text(lang, Key::RecNoticeSource),
            i18n::short_date(lang, started.date()),
            started.format("%H:%M")
        ),
        None => stem,
    };
    RecEntry {
        path: path.to_path_buf(),
        title,
        started: i18n::rec_started(lang, started, today),
        duration: side.map_or(Duration::ZERO, |s| Duration::from_millis(s.duration_ms)),
    }
}

/// `<name>.vale.json` next to `<name>.m4a`, read leniently; `None` when it is
/// missing or not JSON.
fn sidecar_of(audio: &Path) -> Option<Sidecar> {
    let stem = audio.file_stem()?.to_string_lossy();
    let file = audio.with_file_name(format!("{stem}.vale.json"));
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    Some(Sidecar::from_value(&value))
}

// ---- the App side --------------------------------------------------------------

impl App {
    /// The Rekam page's start button, the hotkey and the tray entry.
    pub(super) fn rec_toggle(&mut self) {
        if self.rec.active() {
            self.rec_stop();
        } else {
            self.rec_start();
        }
    }

    fn rec_start(&mut self) {
        let lang = self.config.language;
        let sources = self.rec_sources;
        let opts = RecOptions {
            sources: Sources {
                mic: sources.mic,
                system: sources.system,
            },
            dir: self.config.recording.dir(),
            // Phase 1: no meeting-app detection, so no title and no app.
            title: String::new(),
            app: None,
        };
        let (tx, wake) = (self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        let events = Box::new(move |event: RecEvent| {
            let _ = tx.send(Event::Rec(event));
            wake();
        });
        match Recorder::start(opts, events) {
            Ok(recorder) => {
                self.rec.recorder = Some(recorder);
                tray::set_recording(self.hwnd, true);
                self.apply_now(Event::RecStatus(Some(RecStatus {
                    started: Instant::now(),
                    paused_total: Duration::ZERO,
                    paused_since: None,
                    sources,
                })));
                self.rec_consent_once();
            }
            Err(e) if e.is_mic_denied() => {
                self.apply_now(Event::Notice(mic_denied_notice(lang)));
            }
            Err(e) => {
                self.apply_now(Event::Notice(failed_notice(lang, &e.to_string())));
            }
        }
    }

    /// The first recording ever: a short reminder to ask everyone first.
    fn rec_consent_once(&mut self) {
        if self.config.recording.consent_shown {
            return;
        }
        self.config.recording.consent_shown = true;
        self.save_config();
        let lang = self.config.language;
        self.apply_now(Event::Notice(consent_notice(lang)));
    }

    /// Stops and finalizes on this thread (the file closes in well under a
    /// second); the writer's `Saved` event makes the notice.
    fn rec_stop(&mut self) {
        let Some(recorder) = self.rec.recorder.take() else {
            return;
        };
        let t0 = Instant::now();
        let outcome = recorder.stop();
        if std::env::var_os("DI_REC_TIMING").is_some() {
            eprintln!("vale: recorder stop took {:?}", t0.elapsed());
        }
        tray::set_recording(self.hwnd, false);
        self.apply_now(Event::RecStatus(None));
        // The writer already sent `Saved` or `Failed` through the events;
        // only a thread that died without a word is told here.
        if let Err(e @ crate::recorder::RecError::Thread(_)) = outcome {
            let lang = self.config.language;
            self.apply_now(Event::Notice(failed_notice(lang, &e.to_string())));
        }
    }

    /// Pause when recording, resume when paused.
    pub(super) fn rec_pause(&mut self) {
        let (Some(recorder), Some(mut status)) = (self.rec.recorder.as_mut(), self.state.rec().cloned())
        else {
            return;
        };
        let now = Instant::now();
        if recorder.paused() {
            recorder.resume();
            if let Some(since) = status.paused_since.take() {
                status.paused_total += now.saturating_duration_since(since);
            }
        } else {
            recorder.pause();
            status.paused_since = Some(now);
        }
        self.apply_now(Event::RecStatus(Some(status)));
    }

    /// The Rekam page's or Settings' source choice: kept and saved.
    pub(super) fn rec_set_sources(&mut self, sources: RecSources) {
        if self.rec.active() || !(sources.mic || sources.system) {
            // Settings shows the choice that is really in effect.
            self.sync_settings();
            return;
        }
        self.rec_sources = sources;
        if self.config.recording.sources != sources {
            self.config.recording.sources = sources;
            self.save_config();
            self.sync_settings();
        }
    }

    /// The levels `(mic, system)` for the frame; zero when not recording.
    pub(super) fn rec_levels(&self) -> (f32, f32) {
        self.rec.recorder.as_ref().map_or((0.0, 0.0), Recorder::levels)
    }

    /// The writer thread's word, from the event queue.
    pub(super) fn on_rec_event(&mut self, event: RecEvent) {
        let lang = self.config.language;
        match event {
            RecEvent::Saved { path, duration } => {
                self.apply_now(Event::Notice(saved_notice(lang, path, duration)));
                self.rec_rescan();
            }
            RecEvent::Failed(reason) => {
                // The recording is over whatever the reason: let go of it.
                if let Some(recorder) = self.rec.recorder.take() {
                    drop(recorder);
                    tray::set_recording(self.hwnd, false);
                    self.apply_now(Event::RecStatus(None));
                }
                self.apply_now(Event::Notice(failed_notice(lang, &reason)));
            }
            RecEvent::SourceLost(source) => {
                self.apply_now(Event::Notice(source_lost_notice(lang, source)));
            }
            RecEvent::MicDenied => self.apply_now(Event::Notice(mic_denied_notice(lang))),
        }
    }

    /// The folder's newest recordings, as the page's event.
    fn rec_recent_event(&self) -> Event {
        let today = Local::now().date_naive();
        Event::RecRecent(scan_recent(
            &self.config.recording.dir(),
            self.config.language,
            today,
            RECENT_LIMIT,
        ))
    }

    /// Reads the folder for the Rekam page (after a save).
    pub(super) fn rec_rescan(&mut self) {
        let event = self.rec_recent_event();
        self.apply_now(event);
    }

    /// Called from `refresh`: a new showing of the Rekam page re-reads the
    /// folder (once per showing, not per frame).
    pub(super) fn rec_follow_page(&mut self, view: &View) {
        let shown = matches!(view, View::Panel(Page::Record));
        if shown && !self.rec.page_shown {
            self.rec.page_shown = true;
            // Inside `refresh`: apply without another refresh.
            let event = self.rec_recent_event();
            self.apply(event, Instant::now());
        } else if !shown {
            self.rec.page_shown = false;
        }
    }

    pub(super) fn rec_reveal(&mut self, file: PathBuf) {
        notif_open::allow_foreground();
        crate::sources::reveal::reveal(file);
    }

    /// Explorer on the recordings folder (made first when it is missing).
    pub(super) fn rec_open_folder(&mut self) {
        let dir = self.config.recording.dir();
        let _ = std::fs::create_dir_all(&dir);
        notif_open::allow_foreground();
        let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
    }

    /// The mic notice's click: Windows' microphone privacy page.
    pub(super) fn rec_mic_settings(&mut self) {
        notif_open::allow_foreground();
        self.queue_shortcut(shortcuts::Action::Open(MIC_SETTINGS));
    }

    /// Vale is closing (or the session ends): finalize a running recording
    /// first, so the file is closed and the sidecar written.
    pub(super) fn rec_shutdown(&mut self) {
        if let Some(recorder) = self.rec.recorder.take() {
            let _ = recorder.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 7).unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vale-rec-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_saved_notice_names_the_length_and_a_click_reveals_the_file() {
        let n = saved_notice(Lang::Id, PathBuf::from(r"C:\r\a.m4a"), Duration::from_secs(42 * 60));
        assert_eq!(n.title, "Rekaman tersimpan");
        assert_eq!(n.body, "42 mnt \u{b7} klik untuk buka");
        assert_eq!(n.kind, NoticeKind::RecSaved { path: PathBuf::from(r"C:\r\a.m4a") });
        let en = saved_notice(Lang::En, PathBuf::new(), Duration::from_secs(5));
        assert_eq!((en.title.as_str(), en.body.as_str()), ("Recording saved", "5 s \u{b7} click to open"));
    }

    #[test]
    fn the_other_notices_say_what_happened_in_both_languages() {
        assert_eq!(source_lost_notice(Lang::Id, Source::Mic).title, "Mic terputus");
        assert_eq!(source_lost_notice(Lang::Id, Source::System).title, "Suara PC terputus");
        assert_eq!(source_lost_notice(Lang::En, Source::Mic).title, "Mic disconnected");
        assert_eq!(source_lost_notice(Lang::En, Source::System).title, "PC audio disconnected");
        assert_eq!(source_lost_notice(Lang::En, Source::Mic).kind, NoticeKind::RecInfo);
        assert_eq!(mic_denied_notice(Lang::En).kind, NoticeKind::RecMicDenied);
        assert_eq!(failed_notice(Lang::Id, " disk penuh ").body, "disk penuh");
        assert_eq!(failed_notice(Lang::En, "x").title, "Recording failed");
        assert_eq!(
            consent_notice(Lang::Id).body,
            "Ingat: minta izin peserta sebelum merekam"
        );
        assert_eq!(
            consent_notice(Lang::En).body,
            "Remember to ask everyone before recording"
        );
    }

    #[test]
    fn the_recent_list_is_the_newest_five_with_sidecar_details() {
        let dir = scratch("list");
        let write = |name: &str, side: Option<&str>| {
            fs::write(dir.join(format!("{name}.m4a")), b"x").unwrap();
            if let Some(json) = side {
                fs::write(dir.join(format!("{name}.vale.json")), json).unwrap();
            }
            // File times need to differ for the order.
            std::thread::sleep(Duration::from_millis(20));
        };
        write("old", None);
        write("titled", Some(r#"{"v":1,"title":"Sync mingguan","started":"2026-11-07T10:02:00+07:00","duration_ms":2520000}"#));
        write("plain", Some(r#"{"v":1,"title":"","started":"2026-11-06T14:30:00+07:00","duration_ms":65000}"#));
        for i in 0..4 {
            write(&format!("filler{i}"), None);
        }
        fs::write(dir.join("notes.txt"), b"x").unwrap();
        fs::write(dir.join("half.m4a.part"), b"x").unwrap();
        let list = scan_recent(&dir, Lang::Id, today(), RECENT_LIMIT);
        assert_eq!(list.len(), 5, "five at most: {list:?}");
        assert_eq!(list[0].title, "filler3", "newest first, no .part, no .txt");
        assert_eq!(list[0].duration, Duration::ZERO, "no sidecar: the length is unknown");
        assert_eq!(list[0].started.len() > 0, true);
        let all = scan_recent(&dir, Lang::Id, today(), 20);
        assert_eq!(all.len(), 7);
        let titled = all.iter().find(|e| e.path.ends_with("titled.m4a")).unwrap();
        assert_eq!(titled.title, "Sync mingguan");
        assert_eq!(titled.duration, Duration::from_secs(42 * 60));
        let plain = all.iter().find(|e| e.path.ends_with("plain.m4a")).unwrap();
        assert_eq!(plain.title, "Rekaman 6 Nov 14:30", "no title: the start time");
        assert_eq!(plain.duration, Duration::from_secs(65));
        let old = all.iter().find(|e| e.path.ends_with("old.m4a")).unwrap();
        assert_eq!(old.title, "old", "no sidecar: the file name");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_or_a_broken_sidecar_is_not_an_error() {
        assert!(scan_recent(Path::new(r"Z:\no\such\folder"), Lang::Id, today(), 5).is_empty());
        let dir = scratch("broken");
        fs::write(dir.join("a.m4a"), b"x").unwrap();
        fs::write(dir.join("a.vale.json"), b"{not json").unwrap();
        let list = scan_recent(&dir, Lang::En, today(), 5);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "a");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scanning_two_hundred_files_is_fast_enough_for_the_ui_thread() {
        let dir = scratch("many");
        for i in 0..200 {
            fs::write(dir.join(format!("2026-11-07 {i:04}.m4a")), b"x").unwrap();
        }
        let t0 = Instant::now();
        let list = scan_recent(&dir, Lang::Id, today(), RECENT_LIMIT);
        let took = t0.elapsed();
        println!("scan of 200 files: {took:?}");
        assert_eq!(list.len(), 5);
        // The budget is about 5 ms; the bound leaves room for a busy CI box.
        assert!(took < Duration::from_millis(100), "{took:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
