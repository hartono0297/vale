//! The island's side of the audio recorder (`crate::recorder`): start and
//! stop (the Rekam page, the hotkey and the tray all come here), pause, the
//! levels, the notices, the list of recent recordings and the tray's red dot.
//! A child of `app`, so it works on `App` directly.
//!
//! Start and stop run on short-lived threads (`rec-start`, `rec-stop`): the
//! devices and the encoder open, and the file finalizes, off the UI thread.
//! Each posts `RecNote::Started` / `RecNote::Stopped` back, tagged with its
//! session number like every word of the recorder, and a word of an older
//! session is ignored. A toggle while a start or a stop runs is ignored.
//!
//! Power: no thread and no timer exist outside a recording. The recorder's
//! own threads (`rec-capture`, `rec-writer`) live from Start to Stop; the UI
//! only reads three atomics at draw time (`levels`), and the clock and the
//! meter redraws are the `TIMER_REC` / `TIMER_REC_LEVEL` one-shots of
//! `sync_rec_timers`. The recent list is read on a short-lived thread
//! (`rec-scan`) when the Rekam page comes on screen and after a save, never
//! at startup and never on a timer.

use std::path::{Component, Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{Local, NaiveDate, NaiveDateTime};

use super::{App, WM_APP_EVENT, WM_APP_REC_DEVICE, notif_open, poster};
use crate::i18n::{self, Key, Lang, text};
use crate::model::{
    Event, Hit, Notice, NoticeKind, Page, RecEntry, RecNote, RecSources, RecStatus, RowTranscript,
    TranscriptOffer,
};
use crate::recorder::devices;
use crate::recorder::sidecar::Sidecar;
use crate::recorder::{Failure, RecError, RecEvent, RecOptions, Recorder, Source, Sources};
use crate::shortcuts;
use crate::ui::{settings, tray};

/// How many recordings the Rekam page lists.
pub(super) const RECENT_LIMIT: usize = 5;
/// Windows' microphone privacy page, which the mic notice's click opens.
const MIC_SETTINGS: &str = "ms-settings:privacy-microphone";

/// What the island keeps of the recorder.
#[derive(Default)]
pub(super) struct RecState {
    recorder: Option<Recorder>,
    /// The newest session's number; a word of an older one is ignored.
    session: u64,
    /// A start or a stop running on its own thread.
    busy: Option<Busy>,
    /// Why the recording is paused: the user or a sleep.
    sleep: SleepPause,
    /// The session ran out of input (`RecEvent::NoInput`) and stopped.
    no_input: bool,
    /// The session's `Saved` or `Failed` arrived (its notice is shown).
    told: bool,
    /// The writer finished by itself (`Failed`, `NoInput`), perhaps before
    /// the start thread reported.
    ended: bool,
    /// The Rekam page was on screen at the last refresh (a new showing
    /// re-reads the folder).
    page_shown: bool,
    /// The device names the Rekam page shows, `[mic, system]`: the ones the
    /// recording uses, or the chosen ones (the Windows default when none)
    /// while idle. Read when the page opens and when a choice changes.
    devices: [String; 2],
    /// The devices the starting recording was told to use, `[mic, system]`:
    /// a pick made while it was starting is applied once it runs.
    started_with: [Option<String>; 2],
}

enum Busy {
    Starting(JoinHandle<Result<Recorder, RecError>>),
    Stopping(JoinHandle<()>),
}

impl RecState {
    pub fn active(&self) -> bool {
        self.recorder.is_some()
    }

    /// A recording runs, or one is starting or stopping.
    pub fn engaged(&self) -> bool {
        self.recorder.is_some() || self.busy.is_some()
    }

    /// The Rekam page is on screen (the rows' marks re-read while it is).
    pub fn page_shown(&self) -> bool {
        self.page_shown
    }

    /// What a toggle does now: nothing while a start or a stop runs.
    fn toggle(&self) -> Option<Toggle> {
        toggle_of(self.busy.is_some(), self.recorder.is_some())
    }

    /// The device names for the Rekam page.
    pub fn devices(&self) -> &[String; 2] {
        &self.devices
    }

    /// A word of `session` is for the newest session (older ones are
    /// ignored, so a late `Failed` never ends the next recording).
    fn current(&self, session: u64) -> bool {
        session == self.session
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Toggle {
    Start,
    Stop,
}

/// The sources whose choice in `now` is not the one a recording started with:
/// they are handed to the running recorder (a pick made while it was
/// starting).
fn changed_picks(
    started: &[Option<String>; 2],
    now: &[Option<String>; 2],
) -> Vec<(Source, Option<String>)> {
    [Source::Mic, Source::System]
        .into_iter()
        .filter(|s| started[s.index()] != now[s.index()])
        .map(|s| (s, now[s.index()].clone()))
        .collect()
}

/// Whether `source` is one of the sources chosen for the recording.
fn is_recorded(sources: RecSources, source: Source) -> bool {
    match source {
        Source::Mic => sources.mic,
        Source::System => sources.system,
    }
}

fn toggle_of(busy: bool, active: bool) -> Option<Toggle> {
    match (busy, active) {
        (true, _) => None,
        (false, true) => Some(Toggle::Stop),
        (false, false) => Some(Toggle::Start),
    }
}

/// The pause a sleep puts on a recording, kept apart from the user's own: a
/// recording the user paused stays paused after the wake, and one the sleep
/// paused goes on, unless the user touched the pause meanwhile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SleepPause {
    /// The recording was paused because the PC went to sleep.
    paused_for_sleep: bool,
}

impl SleepPause {
    /// Going to sleep; `paused`: the recording is paused now. True when it
    /// should be paused for the sleep.
    fn suspend(&mut self, paused: bool) -> bool {
        if paused {
            return false;
        }
        self.paused_for_sleep = true;
        true
    }

    /// Awake again; true when the recording should go on.
    fn resume(&mut self, paused: bool) -> bool {
        std::mem::take(&mut self.paused_for_sleep) && paused
    }

    /// The user paused or resumed: theirs is the pause from now on.
    fn user(&mut self) {
        self.paused_for_sleep = false;
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

/// "Rekaman tersimpan" over "42 mnt · klik untuk buka"; "Rekaman dihentikan:
/// tidak ada suara masuk" when every source was lost (`no_input`). A click
/// shows the file in Explorer. `offer` adds a chip about a transcript; with
/// `Setup` the body asks "Bikin transkrip?" in place of "klik untuk buka"
/// (the chip is how to go on, the body click still shows the file).
pub(crate) fn saved_notice(
    lang: Lang,
    path: PathBuf,
    duration: Duration,
    no_input: bool,
    offer: TranscriptOffer,
) -> Notice {
    let title = if no_input {
        Key::RecStoppedNoInput
    } else {
        Key::RecSavedTitle
    };
    notice(
        "rec-saved",
        lang,
        text(lang, title).into(),
        format!(
            "{} \u{b7} {}",
            i18n::rec_duration(lang, duration),
            text(
                lang,
                if offer == TranscriptOffer::Setup {
                    Key::TrWantOne
                } else {
                    Key::RecClickToOpen
                }
            )
        ),
        NoticeKind::RecSaved { path, offer },
    )
}

/// Every source was lost before anything worth keeping was recorded.
pub(crate) fn no_input_notice(lang: Lang) -> Notice {
    notice(
        "rec-lost",
        lang,
        text(lang, Key::RecStoppedNoInput).into(),
        String::new(),
        NoticeKind::RecInfo,
    )
}

/// What went wrong, in words: "Rekaman gagal" over the reason. A blocked
/// folder (`dir`) gets its own notice whose click picks another folder, and
/// an unfinished file one whose click shows the `.part`.
pub(crate) fn failure_notice(lang: Lang, failure: &Failure, dir: &Path) -> Notice {
    let why = match failure {
        Failure::FolderBlocked => return folder_blocked_notice(lang, dir.to_path_buf()),
        Failure::Unfinished { part } => {
            return notice(
                "rec-failed",
                lang,
                text(lang, Key::RecFailedTitle).into(),
                text(lang, Key::RecWhyUnfinished).into(),
                NoticeKind::RecFailed {
                    reveal: Some(part.clone()),
                },
            );
        }
        Failure::FolderGone => Key::RecWhyFolderGone,
        Failure::DiskFull => Key::RecWhyDiskFull,
        Failure::NoMediaFoundation => Key::RecWhyNoMf,
        Failure::NoDevice => Key::RecWhyNoDevice,
        // Nothing came in: not a failure of Vale's, so a plain word.
        Failure::NoAudio => {
            return notice(
                "rec-failed",
                lang,
                text(lang, Key::RecNotSavedTitle).into(),
                text(lang, Key::RecWhyNoAudio).into(),
                NoticeKind::RecInfo,
            );
        }
        Failure::Other(detail) => {
            eprintln!("vale: recording failed: {detail}");
            Key::RecWhyOther
        }
    };
    notice(
        "rec-failed",
        lang,
        text(lang, Key::RecFailedTitle).into(),
        text(lang, why).into(),
        NoticeKind::RecFailed { reveal: None },
    )
}

/// Windows refused to write in `dir`: a click picks another folder.
pub(crate) fn folder_blocked_notice(lang: Lang, dir: PathBuf) -> Notice {
    notice(
        "rec-folder",
        lang,
        text(lang, Key::RecFolderBlockedTitle).into(),
        text(lang, Key::RecFolderBlockedBody).into(),
        NoticeKind::RecFolderBlocked { dir },
    )
}

/// Windows refused the chosen folder and the recording went to `dir` (the
/// profile folder's `Vale\Rekaman`) instead.
pub(crate) fn fallback_notice(lang: Lang, dir: &Path) -> Notice {
    notice(
        "rec-folder",
        lang,
        text(lang, Key::RecFallbackTitle).into(),
        format!(
            "{} {} \u{b7} {}",
            text(lang, Key::RecSavingTo),
            short_dir(dir),
            text(lang, Key::RecChangeInSettings)
        ),
        NoticeKind::RecInfo,
    )
}

/// The last three names of `dir` (`a\Vale\Rekaman`), or all of it when it
/// is shorter, so the notice says which `Vale\Rekaman` it is.
fn short_dir(dir: &Path) -> String {
    let names: Vec<_> = dir
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .collect();
    if names.len() < 3 {
        return dir.display().to_string();
    }
    let tail: PathBuf = names[names.len() - 3..].iter().collect();
    tail.display().to_string()
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

/// A source moved to another device by itself: the Windows default changed,
/// or the device the user chose was not there (`fell_back`) and the default
/// took over. `name` is the device in use now; `chosen` the name of the
/// device the user chose, when it can still be looked up, so the notice says
/// which one is missing.
pub(crate) fn device_switched_notice(
    lang: Lang,
    source: Source,
    name: &str,
    fell_back: bool,
    chosen: Option<&str>,
) -> Notice {
    let using = format!("{} {name}", text(lang, Key::RecUsing));
    let (title, body) = match (fell_back, source) {
        (false, Source::Mic) => (text(lang, Key::RecMicSwitched).to_owned(), name.to_owned()),
        (false, Source::System) => (text(lang, Key::RecSystemSwitched).to_owned(), name.to_owned()),
        (true, source) => (missing_title(lang, source, chosen), using),
    };
    notice("rec-device", lang, title, body, NoticeKind::RecInfo)
}

/// "Mic <chosen> tidak ada" / "<chosen> not found", or the plain "chosen
/// device not found" when the name is not known.
fn missing_title(lang: Lang, source: Source, chosen: Option<&str>) -> String {
    let Some(chosen) = chosen else {
        return text(
            lang,
            match source {
                Source::Mic => Key::RecMicMissing,
                Source::System => Key::RecSystemMissing,
            },
        )
        .to_owned();
    };
    let not_found = text(lang, Key::RecNotFound);
    match lang {
        Lang::Id => {
            let word = match source {
                Source::Mic => Key::RecMicWord,
                Source::System => Key::RecSystemWord,
            };
            format!("{} {chosen} {not_found}", text(lang, word))
        }
        Lang::En => format!("{chosen} {not_found}"),
    }
}

/// What the Rekam page shows for a source while idle: the chosen device's
/// name, else the Windows default's (which is what a "follow" choice uses).
fn idle_device_name(chosen: Option<String>, default: Option<String>) -> String {
    chosen.or(default).unwrap_or_default()
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

/// The island's REC clock from the recorder's own, so the clock shows what
/// the file will hold.
fn status_of(recorder: &Recorder, sources: RecSources) -> RecStatus {
    let (started, paused_total, paused_since) = recorder.clock();
    RecStatus {
        started,
        paused_total,
        paused_since,
        sources,
        muted: recorder.muted(),
    }
}

// ---- the folder ----------------------------------------------------------------

/// Whether recordings can be written in `dir`: it is made when missing, then
/// a probe file is created and deleted. Controlled folder access refuses the
/// create.
pub(super) fn probe_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(format!(".vale-probe-{}.tmp", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)?;
    std::fs::remove_file(&probe)
}

/// Where a recording goes: `chosen` when Vale may write there, else
/// `fallback`; `None` when neither (the start then fails and the
/// blocked-folder notice offers the picker).
pub(super) fn choose_dir(
    chosen: &Path,
    fallback: Option<&Path>,
    writable: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if writable(chosen) {
        return Some(chosen.to_path_buf());
    }
    let other = fallback.filter(|f| *f != chosen)?;
    writable(other).then(|| other.to_path_buf())
}

/// `<name>.m4a.part` beside `<name>.m4a`.
fn part_of(audio: &Path) -> PathBuf {
    let mut name = audio.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

// ---- the recent list ---------------------------------------------------------

/// What a recordings folder file is to the list: a finished `.m4a`, an
/// unfinished `.m4a.part` (a crash left it), or nothing.
fn listed(path: &Path) -> Option<bool> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".m4a.part") {
        Some(true)
    } else if name.ends_with(".m4a") {
        Some(false)
    } else {
        None
    }
}

/// The `limit` newest recordings of `dir` as the page lists them, `skip`
/// (the running recording's `.part`) left out. The title and the start come
/// from the sidecar; a file with none shows its name and its file time, and
/// an unknown length is zero. A `.m4a.part` is listed as unfinished. Never
/// fails: an unreadable folder is an empty list.
pub(super) fn scan_recent(
    dir: &Path,
    lang: Lang,
    today: NaiveDate,
    limit: usize,
    skip: Option<&Path>,
) -> Vec<RecEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    // `DirEntry::metadata` is free on Windows (the listing carries it), so
    // the cost is the sort; only the newest few files are opened further.
    let mut files: Vec<(std::time::SystemTime, PathBuf, bool)> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            let unfinished = listed(&path)?;
            if skip.is_some_and(|s| s == path) {
                return None;
            }
            let meta = e.metadata().ok()?;
            meta.is_file()
                .then(|| (meta.modified().unwrap_or(std::time::UNIX_EPOCH), path, unfinished))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.truncate(limit);
    files
        .into_iter()
        .map(|(modified, path, unfinished)| entry_of(&path, modified, unfinished, lang, today))
        .collect()
}

/// The recording's name without `.m4a` or `.m4a.part`.
fn stem_of(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    for ext in [".m4a.part", ".m4a"] {
        if lower.ends_with(ext) {
            return name[..name.len() - ext.len()].to_string();
        }
    }
    name
}

/// The Rekam page's title of a recording: the sidecar's, else "Rekaman
/// <date> <time>" for one with a sidecar but no title, else the file name.
/// Reads the sidecar beside `path`.
pub(crate) fn display_title(path: &Path, lang: Lang, _today: NaiveDate) -> String {
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::UNIX_EPOCH);
    let stem = stem_of(path);
    let side = sidecar_of(path, &stem);
    title_of(side.as_ref(), stem, modified, lang).0
}

/// The title and the start of a recording from its sidecar (when it has
/// one) and its file time.
fn title_of(
    side: Option<&Sidecar>,
    stem: String,
    modified: std::time::SystemTime,
    lang: Lang,
) -> (String, NaiveDateTime) {
    let file_time: NaiveDateTime = chrono::DateTime::<Local>::from(modified).naive_local();
    // A sidecar with no start (epoch) says nothing about when.
    let started = side
        .filter(|s| s.started.timestamp() > 0)
        .map_or(file_time, |s| s.started.with_timezone(&Local).naive_local());
    let title = match side {
        Some(s) if !s.title.trim().is_empty() => s.title.trim().to_owned(),
        Some(_) => format!(
            "{} {} {}",
            text(lang, Key::RecNoticeSource),
            i18n::short_date(lang, started.date()),
            started.format("%H:%M")
        ),
        None => stem,
    };
    (title, started)
}

fn entry_of(
    path: &Path,
    modified: std::time::SystemTime,
    unfinished: bool,
    lang: Lang,
    today: NaiveDate,
) -> RecEntry {
    let stem = stem_of(path);
    let side = sidecar_of(path, &stem);
    let (title, started) = title_of(side.as_ref(), stem, modified, lang);
    RecEntry {
        path: path.to_path_buf(),
        title,
        started: i18n::rec_started(lang, started, today),
        duration: side.map_or(Duration::ZERO, |s| Duration::from_millis(s.duration_ms)),
        unfinished,
        // The scan reads what is on disk; `transcribe_marks` puts the queue's
        // state over it.
        transcript: RowTranscript::None,
    }
}

/// `<stem>.vale.json` beside the recording, read leniently; `None` when it is
/// missing or not JSON.
fn sidecar_of(audio: &Path, stem: &str) -> Option<Sidecar> {
    let file = audio.with_file_name(format!("{stem}.vale.json"));
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    Some(Sidecar::from_value(&value))
}

// ---- the App side --------------------------------------------------------------

impl App {
    /// The Rekam page's start button, the hotkey and the tray entry. Ignored
    /// while a start or a stop is still running.
    pub(super) fn rec_toggle(&mut self) {
        match self.rec.toggle() {
            Some(Toggle::Stop) => self.rec_stop(),
            Some(Toggle::Start) => self.rec_start(),
            None => {}
        }
    }

    /// The Rekam page's button, the hotkey and the tray entry: mutes or
    /// unmutes `source` of the running recording. Ignored when none runs or
    /// the source is not one of the recorded.
    pub(super) fn rec_toggle_mute(&mut self, source: Source) {
        let Some(recorder) = self.rec.recorder.as_ref() else {
            return;
        };
        let sources = self.state.rec().map_or(self.rec_sources, |s| s.sources);
        if !is_recorded(sources, source) {
            return;
        }
        let now = !recorder.muted()[usize::from(source == Source::System)];
        recorder.set_muted(source, now);
        tray::set_mic_muted(recorder.muted()[0]);
        self.rec_show_status();
    }

    /// Posts `note` of `session` to the island from any thread.
    fn rec_poster(&self, session: u64) -> impl Fn(RecNote) + Send + 'static {
        let (tx, wake) = (self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        move |note| {
            let _ = tx.send(Event::Rec(session, note));
            wake();
        }
    }

    /// Starts a recording on the `rec-start` thread; the REC view shows once
    /// it reports (`rec_started`). `DI_REC_TIMING` prints how long it took.
    fn rec_start(&mut self) {
        self.rec.session += 1;
        let session = self.rec.session;
        self.rec.no_input = false;
        self.rec.told = false;
        self.rec.ended = false;
        self.rec.sleep = SleepPause::default();
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
            devices: [
                self.config.recording.mic_device.clone(),
                self.config.recording.system_device.clone(),
            ],
            echo_cancel: self.config.recording.echo_cancel,
        };
        self.rec.started_with = opts.devices.clone();
        let say = self.rec_poster(session);
        let events: Box<dyn Fn(RecEvent) + Send> = Box::new({
            let say = self.rec_poster(session);
            move |event| say(RecNote::Recorder(event))
        });
        let timing = std::env::var_os("DI_REC_TIMING").is_some();
        let started = std::thread::Builder::new()
            .name("rec-start".into())
            .spawn(move || {
                let t0 = Instant::now();
                let mut opts = opts;
                // Controlled folder access may guard the folder: then the
                // profile folder's `Vale\Rekaman` takes the recording.
                let fallback = crate::recorder::prefs::fallback_folder();
                let writable = |d: &Path| probe_writable(d).is_ok();
                if let Some(dir) = choose_dir(&opts.dir, fallback.as_deref(), writable) {
                    opts.dir = dir;
                }
                let result = Recorder::start(opts, events);
                if timing {
                    eprintln!("vale: recorder start took {:?}", t0.elapsed());
                }
                say(RecNote::Started);
                result
            });
        match started {
            Ok(handle) => self.rec.busy = Some(Busy::Starting(handle)),
            Err(e) => {
                let lang = self.config.language;
                let n = failure_notice(lang, &Failure::Other(e.to_string()), &self.config.recording.dir());
                self.apply_now(Event::Notice(n));
            }
        }
        // A recording is starting: the transcript queue holds.
        self.transcribe_sync_hold();
    }

    /// The start thread is done: the recorder runs, or the notice says why not.
    fn rec_started(&mut self) {
        let Some(Busy::Starting(handle)) = self.rec.busy.take() else {
            return;
        };
        let lang = self.config.language;
        let result = handle
            .join()
            .unwrap_or_else(|_| Err(RecError::Thread("the start thread panicked".into())));
        match result {
            // The writer already gave up: only its threads are let go of.
            Ok(recorder) if self.rec.ended => {
                self.rec.recorder = Some(recorder);
                self.rec_stop();
            }
            Ok(recorder) => {
                let status = status_of(&recorder, self.rec_sources);
                let used = recorder.path().parent().map(Path::to_path_buf);
                self.rec.recorder = Some(recorder);
                // A device picked while the recording was starting is saved
                // but the capture was opened with the old one.
                let now = [Source::Mic, Source::System]
                    .map(|s| self.config.recording.device(s).map(str::to_owned));
                if let Some(recorder) = self.rec.recorder.as_ref() {
                    for (source, id) in changed_picks(&self.rec.started_with, &now) {
                        recorder.set_device(source, id);
                    }
                }
                self.rec_read_devices();
                tray::set_recording(self.hwnd, true);
                tray::set_mic_muted(false);
                self.apply_now(Event::RecStatus(Some(status)));
                // The mute hotkey is registered while a recording runs.
                self.sync_hotkeys();
                if let Some(used) = used {
                    self.rec_fell_back(&used);
                }
                self.rec_consent_once();
            }
            Err(e) if e.is_mic_denied() => {
                self.apply_now(Event::Notice(mic_denied_notice(lang)));
            }
            Err(e) => {
                let dir = self.config.recording.dir();
                self.apply_now(Event::Notice(failure_notice(lang, &e.failure(), &dir)));
            }
        }
        // Whichever way the start went, the queue's hold follows the recorder.
        self.transcribe_sync_hold();
    }

    /// The recording went to `used`, not to the configured folder (Windows
    /// refused that one). The default folder moves there for good, so
    /// Settings and the Rekam page show it; a folder the user picked stays
    /// theirs. Either way a notice says where the recording goes.
    fn rec_fell_back(&mut self, used: &Path) {
        if used == self.config.recording.dir() {
            return;
        }
        if self.config.recording.folder.is_none() {
            self.config.recording.folder = Some(used.to_path_buf());
            self.save_config();
            self.sync_settings();
        }
        let lang = self.config.language;
        self.apply_now(Event::Notice(fallback_notice(lang, used)));
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

    /// Ends the recording now on screen and finalizes it on the `rec-stop`
    /// thread; the writer's `Saved` (or `Failed`) makes the notice, then the
    /// thread reports `Stopped`.
    fn rec_stop(&mut self) {
        let Some(recorder) = self.rec.recorder.take() else {
            return;
        };
        tray::set_recording(self.hwnd, false);
        tray::set_mic_muted(false);
        self.apply_now(Event::RecStatus(None));
        self.sync_hotkeys();
        let say = self.rec_poster(self.rec.session);
        let timing = std::env::var_os("DI_REC_TIMING").is_some();
        // A spawn that fails drops the closure, and the recorder's `Drop`
        // finishes it here instead.
        if let Ok(handle) = std::thread::Builder::new()
            .name("rec-stop".into())
            .spawn(move || {
                let t0 = Instant::now();
                let outcome = recorder.stop();
                if timing {
                    eprintln!("vale: recorder stop took {:?}", t0.elapsed());
                }
                // The writer already told `Saved` or `Failed`; only a
                // thread that died without a word is told here.
                if let Err(e @ RecError::Thread(_)) = outcome {
                    say(RecNote::Recorder(RecEvent::Failed(e.failure())));
                }
                say(RecNote::Stopped);
            })
        {
            self.rec.busy = Some(Busy::Stopping(handle));
        }
        // No `Stopped` comes when the thread did not start: the hold follows
        // the recorder now, not only when that word arrives.
        self.transcribe_sync_hold();
    }

    /// The stop thread is done.
    fn rec_stopped(&mut self) {
        if let Some(Busy::Stopping(handle)) = self.rec.busy.take() {
            let _ = handle.join();
        }
        self.rec_read_devices();
        // Every source went before anything was kept: say so all the same.
        if self.rec.no_input && !self.rec.told {
            let lang = self.config.language;
            self.apply_now(Event::Notice(no_input_notice(lang)));
        }
        self.rec_rescan();
        // The recording is over: a job held back may start.
        self.transcribe_sync_hold();
    }

    /// The status the island shows, from the running recorder.
    fn rec_show_status(&mut self) {
        let Some(recorder) = self.rec.recorder.as_ref() else {
            return;
        };
        let sources = self.state.rec().map_or(self.rec_sources, |s| s.sources);
        let status = status_of(recorder, sources);
        self.apply_now(Event::RecStatus(Some(status)));
    }

    /// Pause when recording, resume when paused.
    pub(super) fn rec_pause(&mut self) {
        let Some(recorder) = self.rec.recorder.as_mut() else {
            return;
        };
        if recorder.paused() {
            recorder.resume();
        } else {
            recorder.pause();
        }
        self.rec.sleep.user();
        self.rec_show_status();
    }

    /// The PC is going to sleep: a running recording pauses.
    pub(super) fn rec_on_suspend(&mut self) {
        let Some(recorder) = self.rec.recorder.as_mut() else {
            return;
        };
        if self.rec.sleep.suspend(recorder.paused()) {
            recorder.pause();
            self.rec_show_status();
        }
    }

    /// Awake again: a recording the sleep paused goes on; one the user
    /// paused stays paused.
    pub(super) fn rec_on_resume(&mut self) {
        let Some(recorder) = self.rec.recorder.as_mut() else {
            return;
        };
        if self.rec.sleep.resume(recorder.paused()) {
            recorder.resume();
            self.rec_show_status();
        }
    }

    /// The Rekam page's or Settings' source choice: kept and saved.
    pub(super) fn rec_set_sources(&mut self, sources: RecSources) {
        if self.rec.engaged() || !(sources.mic || sources.system) {
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

    /// Reads the device names the Rekam page shows: from the running
    /// recorder, or from the choices (and the Windows default) while idle.
    /// Called when the page opens, a recording starts or ends, and a choice
    /// or a device changes: never on a timer.
    fn rec_read_devices(&mut self) {
        let names = match self.rec.recorder.as_ref() {
            Some(recorder) => recorder.device_names(),
            None => [Source::Mic, Source::System].map(|source| {
                let chosen = self
                    .config
                    .recording
                    .device(source)
                    .and_then(devices::name_of);
                idle_device_name(chosen, devices::default_endpoint(source).map(|d| d.name))
            }),
        };
        self.rec.devices = names;
    }

    /// The Rekam page's device name was clicked: the dropdown opens from the
    /// main loop (taking the foreground sends messages back to the island, so
    /// it must not run inside the app's borrow).
    pub(super) fn rec_open_device_menu(&mut self, source: Source) {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
        let _ = unsafe {
            PostMessageW(
                Some(self.hwnd),
                WM_APP_REC_DEVICE,
                WPARAM(usize::from(source == Source::System)),
                LPARAM(0),
            )
        };
    }

    /// The device name of `source` on the Rekam page as last drawn, screen
    /// px: where its dropdown opens. `None` when the page does not show it.
    pub(super) fn rec_device_anchor(&self, source: Source) -> Option<crate::capture::Rect> {
        let r = self.renderer.region_of(&Hit::RecDevice(source))?;
        let (origin, k) = self.win.as_ref().map(|w| (w.screen_origin(), w.scale()))?;
        let px = |v: f32| (v * k).round() as i32;
        Some(crate::capture::Rect::new(
            origin.0 + px(r.x),
            origin.1 + px(r.y),
            px(r.w),
            px(r.h),
        ))
    }

    /// The device dropdown went: the panel lets go of it (and lingers), and a
    /// pick is applied.
    pub(super) fn rec_menu_done(&mut self, pick: Option<(Source, Option<String>)>) {
        if let Some((source, id)) = pick {
            self.rec_set_device(source, id);
        }
        self.apply_now(Event::MenuOpen(false));
    }

    /// A device picked in the dropdown or in Settings: saved, and while a
    /// recording runs the source switches to it at once. A pick the user
    /// made by hand gives no notice. `None` follows the Windows default.
    pub(super) fn rec_set_device(&mut self, source: Source, id: Option<String>) {
        if self.config.recording.device(source) != id.as_deref() {
            self.config.recording.set_device(source, id.clone());
            self.save_config();
        }
        if let Some(recorder) = self.rec.recorder.as_ref() {
            recorder.set_device(source, id);
        }
        self.rec_read_devices();
        self.sync_settings();
        self.refresh(Instant::now());
    }

    /// Settings' (or the picker's) folder choice. Refused while a recording
    /// runs (Settings snaps back); a folder Windows will not let Vale write
    /// in is not saved, and the blocked notice shows again.
    pub(super) fn rec_set_folder(&mut self, dir: Option<PathBuf>) {
        if self.rec.engaged() {
            self.sync_settings();
            return;
        }
        if let Some(d) = &dir
            && let Err(e) = probe_writable(d)
        {
            eprintln!("vale: recordings folder {} refused: {e}", d.display());
            let lang = self.config.language;
            self.apply_now(Event::Notice(folder_blocked_notice(lang, d.clone())));
            self.sync_settings();
            return;
        }
        self.config.recording.folder = dir;
        self.save_config();
        self.sync_settings();
        if self.rec.page_shown {
            self.rec_rescan();
        }
    }

    /// The blocked-folder notice's click: the folder picker, whose choice
    /// comes back through `rec_set_folder`.
    pub(super) fn rec_pick_folder(&mut self) {
        notif_open::allow_foreground();
        settings::pick_rec_folder(self.hwnd);
    }

    /// The levels `(mic, system)` for the frame; zero when not recording.
    pub(super) fn rec_levels(&self) -> (f32, f32) {
        self.rec.recorder.as_ref().map_or((0.0, 0.0), Recorder::levels)
    }

    /// A word from a recording's threads, from the event queue. One of an
    /// older session than the newest is ignored.
    pub(super) fn on_rec_note(&mut self, session: u64, note: RecNote) {
        if !self.rec.current(session) {
            return;
        }
        match note {
            RecNote::Started => self.rec_started(),
            RecNote::Stopped => self.rec_stopped(),
            RecNote::Recorder(event) => self.on_rec_event(event),
        }
    }

    fn on_rec_event(&mut self, event: RecEvent) {
        let lang = self.config.language;
        match event {
            RecEvent::Saved { path, duration } => {
                self.rec.told = true;
                let offer = self.transcribe_saved(&path);
                let n = saved_notice(lang, path, duration, self.rec.no_input, offer);
                self.apply_now(Event::Notice(n));
            }
            RecEvent::Failed(failure) => {
                // The recording is over whatever the reason: let go of it.
                self.rec.told = true;
                self.rec.ended = true;
                self.rec_stop();
                let dir = self.config.recording.dir();
                self.apply_now(Event::Notice(failure_notice(lang, &failure, &dir)));
            }
            RecEvent::SourceLost(source) => {
                self.apply_now(Event::Notice(source_lost_notice(lang, source)));
            }
            RecEvent::MicDenied => self.apply_now(Event::Notice(mic_denied_notice(lang))),
            RecEvent::DeviceSwitched {
                source,
                name,
                fell_back,
                manual,
            } => {
                // The capture thread has switched now: the names are read
                // after it, not when the pick was made.
                self.rec_read_devices();
                if manual {
                    self.refresh(Instant::now());
                    return;
                }
                let chosen = self.config.recording.device(source).and_then(devices::name_of);
                let n = device_switched_notice(lang, source, &name, fell_back, chosen.as_deref());
                self.apply_now(Event::Notice(n));
            }
            // The writer finishes by itself; the threads are let go of.
            RecEvent::NoInput => {
                self.rec.no_input = true;
                self.rec.ended = true;
                self.rec_stop();
            }
        }
    }

    /// Reads the folder for the Rekam page on the `rec-scan` thread (a
    /// OneDrive folder may fetch a sidecar while it is read); the list comes
    /// back as `Event::RecRecent`.
    pub(super) fn rec_rescan(&mut self) {
        let dir = self.config.recording.dir();
        let lang = self.config.language;
        let skip = self.rec.recorder.as_ref().map(|r| part_of(r.path()));
        let (tx, wake) = (self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        let _ = std::thread::Builder::new()
            .name("rec-scan".into())
            .spawn(move || {
                let today = Local::now().date_naive();
                let list = scan_recent(&dir, lang, today, RECENT_LIMIT, skip.as_deref());
                let _ = tx.send(Event::RecRecent(list));
                wake();
            });
    }

    /// Called from `refresh`: a new showing of the Rekam page re-reads the
    /// folder (once per showing, not per frame).
    /// `under`: the open panel's page, also while a notice covers it (a
    /// "Mic pindah" over the page must not close its dropdown).
    pub(super) fn rec_follow_page(&mut self, under: Option<Page>) {
        let shown = under == Some(Page::Record);
        if shown && !self.rec.page_shown {
            self.rec.page_shown = true;
            self.rec_rescan();
            self.rec_read_devices();
        } else if !shown {
            self.rec.page_shown = false;
            // The device dropdown belongs to the page: it goes with it.
            if crate::ui::menu_pop::is_open_for(self.hwnd) {
                crate::ui::menu_pop::close();
            }
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

    /// Vale is closing (or the session ends): a start or a stop in flight is
    /// waited for, and a running recording is finalized, so the file is
    /// closed and the sidecar written.
    pub(super) fn rec_shutdown(&mut self) {
        match self.rec.busy.take() {
            Some(Busy::Starting(handle)) => {
                if let Ok(Ok(recorder)) = handle.join() {
                    let _ = recorder.stop();
                }
            }
            Some(Busy::Stopping(handle)) => {
                let _ = handle.join();
            }
            None => {}
        }
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

    /// A fresh folder under `target/test-tmp/`.
    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("app-rec-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_saved_notice_names_the_length_and_a_click_reveals_the_file() {
        let n = saved_notice(Lang::Id, PathBuf::from(r"C:\r\a.m4a"), Duration::from_secs(42 * 60), false, TranscriptOffer::None);
        assert_eq!(n.title, "Rekaman tersimpan");
        assert_eq!(n.body, "42 mnt \u{b7} klik untuk buka");
        assert_eq!(
            n.kind,
            NoticeKind::RecSaved {
                path: PathBuf::from(r"C:\r\a.m4a"),
                offer: TranscriptOffer::None
            }
        );
        let en = saved_notice(Lang::En, PathBuf::new(), Duration::from_secs(5), false, TranscriptOffer::None);
        assert_eq!((en.title.as_str(), en.body.as_str()), ("Recording saved", "5 s \u{b7} click to open"));
    }

    #[test]
    fn saved_notice_bodies_per_offer() {
        let at = |lang, offer| {
            saved_notice(lang, PathBuf::from(r"C:\r\a.m4a"), Duration::from_secs(42 * 60), false, offer)
        };
        for offer in [TranscriptOffer::None, TranscriptOffer::Transcribe] {
            assert_eq!(at(Lang::Id, offer).body, "42 mnt \u{b7} klik untuk buka", "{offer:?}");
            assert_eq!(at(Lang::En, offer).body, "42 min \u{b7} click to open", "{offer:?}");
        }
        let id = at(Lang::Id, TranscriptOffer::Setup);
        assert_eq!(id.body, "42 mnt \u{b7} Bikin transkrip?");
        assert_eq!(id.title, "Rekaman tersimpan");
        let en = at(Lang::En, TranscriptOffer::Setup);
        assert_eq!(en.body, "42 min \u{b7} Want a transcript?");
        // The offer rides on the kind, with the file.
        assert_eq!(
            at(Lang::Id, TranscriptOffer::Transcribe).kind,
            NoticeKind::RecSaved {
                path: PathBuf::from(r"C:\r\a.m4a"),
                offer: TranscriptOffer::Transcribe
            }
        );
    }

    #[test]
    fn a_recording_that_ran_out_of_input_says_so() {
        let n = saved_notice(Lang::Id, PathBuf::from(r"C:\r\a.m4a"), Duration::from_secs(90), true, TranscriptOffer::None);
        assert_eq!(n.title, "Rekaman dihentikan: tidak ada suara masuk");
        assert!(matches!(n.kind, NoticeKind::RecSaved { .. }));
        let en = no_input_notice(Lang::En);
        assert_eq!(en.title, "Recording stopped: no audio input");
        assert_eq!(en.kind, NoticeKind::RecInfo);
    }

    #[test]
    fn every_failure_reads_in_both_languages() {
        let dir = Path::new(r"C:\Users\a\Documents\Vale\Rekaman");
        let cases = [
            (Failure::FolderGone, "Folder rekaman tidak ditemukan", "The recordings folder is gone"),
            (Failure::DiskFull, "Disk penuh", "The disk is full"),
            (
                Failure::NoMediaFoundation,
                "Media Foundation tidak ada di Windows ini",
                "Media Foundation is missing on this Windows",
            ),
            (Failure::NoDevice, "Mic atau speaker tidak ditemukan", "No microphone or speaker found"),
            (
                Failure::Other("audio encoder failed: 0x1234".into()),
                "Ada yang tidak beres saat merekam",
                "Something went wrong while recording",
            ),
        ];
        for (failure, id, en) in cases {
            let n = failure_notice(Lang::Id, &failure, dir);
            assert_eq!((n.title.as_str(), n.body.as_str()), ("Rekaman gagal", id));
            assert_eq!(n.kind, NoticeKind::RecFailed { reveal: None });
            let n = failure_notice(Lang::En, &failure, dir);
            assert_eq!((n.title.as_str(), n.body.as_str()), ("Recording failed", en));
        }
    }

    #[test]
    fn a_recording_that_heard_nothing_says_it_was_not_saved() {
        let dir = Path::new(r"C:\r");
        let n = failure_notice(Lang::Id, &Failure::NoAudio, dir);
        assert_eq!((n.title.as_str(), n.body.as_str()), ("Rekaman tidak disimpan", "Tidak ada suara masuk"));
        assert_eq!(n.kind, NoticeKind::RecInfo);
        let en = failure_notice(Lang::En, &Failure::NoAudio, dir);
        assert_eq!((en.title.as_str(), en.body.as_str()), ("Recording not saved", "No audio came in"));
    }

    #[test]
    fn a_blocked_folder_falls_back_to_the_profile_folder() {
        let docs = Path::new(r"C:\Users\a\Documents\Vale\Rekaman");
        let home = Path::new(r"C:\Users\a\Vale\Rekaman");
        let all = |_: &Path| true;
        let none = |_: &Path| false;
        let only_home = |d: &Path| d == home;
        assert_eq!(choose_dir(docs, Some(home), all), Some(docs.to_path_buf()), "the choice when it works");
        assert_eq!(choose_dir(docs, Some(home), only_home), Some(home.to_path_buf()));
        assert_eq!(choose_dir(docs, Some(home), none), None, "both refused");
        assert_eq!(choose_dir(docs, None, only_home), None, "no fallback known");
        assert_eq!(choose_dir(home, Some(home), none), None, "the fallback itself refused");
    }

    #[test]
    fn the_fallback_notice_names_the_folder_in_both_languages() {
        let home = Path::new(r"C:\Users\a\Vale\Rekaman");
        let n = fallback_notice(Lang::Id, home);
        assert_eq!(n.title, "Windows melindungi folder rekaman");
        assert_eq!(n.body, "Disimpan ke a\\Vale\\Rekaman \u{b7} ganti di Pengaturan");
        assert_eq!(n.kind, NoticeKind::RecInfo);
        let en = fallback_notice(Lang::En, home);
        assert_eq!(en.title, "Windows protects the recordings folder");
        assert_eq!(en.body, "Saving to a\\Vale\\Rekaman \u{b7} change it in Settings");
        assert_eq!(short_dir(Path::new(r"D:\Rec")), r"D:\Rec");
    }

    #[test]
    fn a_blocked_folder_and_an_unfinished_file_get_their_own_click() {
        let dir = Path::new(r"C:\Users\a\Documents\Vale\Rekaman");
        let n = failure_notice(Lang::Id, &Failure::FolderBlocked, dir);
        assert_eq!(n.title, "Folder rekaman diblokir Windows");
        assert_eq!(n.body, "Klik untuk pilih folder lain");
        assert_eq!(n.kind, NoticeKind::RecFolderBlocked { dir: dir.to_path_buf() });
        let en = folder_blocked_notice(Lang::En, dir.to_path_buf());
        assert_eq!(
            (en.title.as_str(), en.body.as_str()),
            ("Windows blocked the recordings folder", "Click to choose another folder")
        );
        let part = PathBuf::from(r"C:\r\a.m4a.part");
        let n = failure_notice(Lang::En, &Failure::Unfinished { part: part.clone() }, dir);
        assert_eq!(n.kind, NoticeKind::RecFailed { reveal: Some(part) });
        assert_eq!(n.body, "The file kept its temporary name \u{b7} click to show");
    }

    #[test]
    fn the_other_notices_say_what_happened_in_both_languages() {
        assert_eq!(source_lost_notice(Lang::Id, Source::Mic).title, "Mic terputus");
        assert_eq!(source_lost_notice(Lang::Id, Source::System).title, "Suara PC terputus");
        assert_eq!(source_lost_notice(Lang::En, Source::Mic).title, "Mic disconnected");
        assert_eq!(source_lost_notice(Lang::En, Source::System).title, "PC audio disconnected");
        assert_eq!(source_lost_notice(Lang::En, Source::Mic).kind, NoticeKind::RecInfo);
        assert_eq!(mic_denied_notice(Lang::En).kind, NoticeKind::RecMicDenied);
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
    fn a_device_switch_notice_says_which_source_moved_where() {
        let n = device_switched_notice(Lang::Id, Source::Mic, "Headset", false, None);
        assert_eq!((n.title.as_str(), n.body.as_str()), ("Mic pindah", "Headset"));
        assert_eq!(n.kind, NoticeKind::RecInfo);
        let n = device_switched_notice(Lang::En, Source::System, "Speakers", false, None);
        assert_eq!((n.title.as_str(), n.body.as_str()), ("PC audio switched", "Speakers"));
        let n = device_switched_notice(Lang::Id, Source::Mic, "Realtek", true, None);
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("Mic pilihan tidak ada", "Pakai Realtek")
        );
        let n = device_switched_notice(Lang::En, Source::System, "Speakers", true, None);
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("Chosen PC audio not found", "Using Speakers")
        );
        let n = device_switched_notice(Lang::Id, Source::System, "Speakers", true, None);
        assert_eq!(n.title, "Suara PC pilihan tidak ada");
    }

    #[test]
    fn the_fall_back_notice_names_the_chosen_device_when_it_is_known() {
        let n = device_switched_notice(Lang::Id, Source::Mic, "Realtek", true, Some("Headset"));
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("Mic Headset tidak ada", "Pakai Realtek")
        );
        let n = device_switched_notice(Lang::Id, Source::System, "Speakers", true, Some("TV"));
        assert_eq!(n.title, "Suara PC TV tidak ada");
        let n = device_switched_notice(Lang::En, Source::Mic, "Realtek", true, Some("Headset"));
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            ("Headset not found", "Using Realtek")
        );
        let n = device_switched_notice(Lang::En, Source::System, "Speakers", true, Some("TV"));
        assert_eq!(n.title, "TV not found");
    }

    #[test]
    fn a_pick_made_while_starting_is_handed_to_the_recorder() {
        let (a, b) = (Some("a".to_owned()), Some("b".to_owned()));
        assert!(changed_picks(&[a.clone(), None], &[a.clone(), None]).is_empty());
        assert_eq!(
            changed_picks(&[a.clone(), None], &[a.clone(), b.clone()]),
            [(Source::System, b.clone())]
        );
        // Back to following Windows is a change too.
        assert_eq!(
            changed_picks(&[a.clone(), b.clone()], &[None, b]),
            [(Source::Mic, None)]
        );
    }

    #[test]
    fn idle_the_page_shows_the_chosen_device_else_the_default() {
        assert_eq!(idle_device_name(Some("A".into()), Some("D".into())), "A");
        assert_eq!(idle_device_name(None, Some("D".into())), "D");
        assert_eq!(idle_device_name(None, None), "");
    }

    #[test]
    fn a_toggle_waits_for_a_start_or_a_stop_and_old_words_are_ignored() {
        assert_eq!(toggle_of(false, false), Some(Toggle::Start));
        assert_eq!(toggle_of(false, true), Some(Toggle::Stop));
        assert_eq!(toggle_of(true, false), None, "starting");
        assert_eq!(toggle_of(true, true), None, "stopping");
        let mut s = RecState::default();
        assert!(!s.engaged());
        assert_eq!(s.toggle(), Some(Toggle::Start));
        s.session = 3;
        assert!(s.current(3));
        assert!(!s.current(2), "a word of the session before");
    }

    #[test]
    fn a_source_can_be_muted_only_when_it_is_recorded() {
        let mic = RecSources { mic: true, system: false };
        let system = RecSources { mic: false, system: true };
        let both = RecSources { mic: true, system: true };
        assert!(is_recorded(mic, Source::Mic) && !is_recorded(mic, Source::System));
        assert!(!is_recorded(system, Source::Mic) && is_recorded(system, Source::System));
        assert!(is_recorded(both, Source::Mic) && is_recorded(both, Source::System));
    }

    #[test]
    fn a_sleep_pause_never_overrides_the_users_own() {
        // Recording, the PC sleeps and wakes: paused, then on again.
        let mut s = SleepPause::default();
        assert!(s.suspend(false));
        assert!(s.resume(true));
        // Paused by the user before the sleep: stays paused after it.
        let mut s = SleepPause::default();
        assert!(!s.suspend(true));
        assert!(!s.resume(true));
        // The user resumed or paused meanwhile: theirs stands.
        let mut s = SleepPause::default();
        assert!(s.suspend(false));
        s.user();
        assert!(!s.resume(true));
        // A wake without a sleep before it does nothing.
        let mut s = SleepPause::default();
        assert!(!s.resume(true));
        // Two wakes after one sleep resume once.
        let mut s = SleepPause::default();
        assert!(s.suspend(false));
        assert!(s.resume(true));
        assert!(!s.resume(true));
    }

    #[test]
    fn a_folder_is_writable_only_when_a_file_can_be_made_in_it() {
        let dir = scratch("probe");
        assert!(probe_writable(&dir.join("new").join("deeper")).is_ok(), "made when missing");
        assert_eq!(fs::read_dir(dir.join("new").join("deeper")).unwrap().count(), 0, "no probe left");
        let file = dir.join("a file");
        fs::write(&file, b"x").unwrap();
        assert!(probe_writable(&file).is_err(), "a file is not a folder");
        assert!(probe_writable(Path::new(r"Z:\no\such\drive")).is_err());
        let _ = fs::remove_dir_all(&dir);
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
        let list = scan_recent(&dir, Lang::Id, today(), RECENT_LIMIT, None);
        assert_eq!(list.len(), 5, "five at most: {list:?}");
        assert_eq!(list[0].title, "filler3", "newest first, no .txt");
        assert_eq!(list[0].duration, Duration::ZERO, "no sidecar: the length is unknown");
        assert!(!list[0].started.is_empty());
        assert!(!list[0].unfinished);
        let all = scan_recent(&dir, Lang::Id, today(), 20, None);
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
    fn a_part_left_by_a_crash_is_listed_unfinished_but_not_the_running_one() {
        let dir = scratch("parts");
        fs::write(dir.join("2026-11-07 0900.m4a.part"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let running = dir.join("2026-11-07 1000.m4a.part");
        fs::write(&running, b"x").unwrap();
        let list = scan_recent(&dir, Lang::Id, today(), RECENT_LIMIT, Some(&running));
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(list[0].title, "2026-11-07 0900");
        assert!(list[0].unfinished);
        assert_eq!(
            i18n::rec_entry_sub(Lang::Id, &list[0].started, list[0].duration, true),
            format!("{} \u{b7} belum selesai", list[0].started)
        );
        assert_eq!(
            i18n::rec_entry_sub(Lang::En, "Today 10:02", Duration::ZERO, true),
            "Today 10:02 \u{b7} unfinished"
        );
        assert_eq!(part_of(Path::new(r"C:\r\a.m4a")), PathBuf::from(r"C:\r\a.m4a.part"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_or_a_broken_sidecar_is_not_an_error() {
        assert!(scan_recent(Path::new(r"Z:\no\such\folder"), Lang::Id, today(), 5, None).is_empty());
        let dir = scratch("broken");
        fs::write(dir.join("a.m4a"), b"x").unwrap();
        fs::write(dir.join("a.vale.json"), b"{not json").unwrap();
        let list = scan_recent(&dir, Lang::En, today(), 5, None);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "a");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scanning_two_hundred_files_stays_quick() {
        let dir = scratch("many");
        for i in 0..200 {
            fs::write(dir.join(format!("2026-11-07 {i:04}.m4a")), b"x").unwrap();
        }
        let t0 = Instant::now();
        let list = scan_recent(&dir, Lang::Id, today(), RECENT_LIMIT, None);
        let took = t0.elapsed();
        println!("scan of 200 files: {took:?}");
        assert_eq!(list.len(), 5);
        // It runs on its own thread now; the bound only catches a blow-up
        // and leaves room for a busy machine.
        assert!(took < Duration::from_millis(500), "{took:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
