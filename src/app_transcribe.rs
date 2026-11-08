//! The island's side of transcripts: the queue and its worker thread, the
//! recording hold, the startup settle, model downloads, the notices with
//! their reasons in plain words, the marks on the Rekam rows and what
//! Settings' Transkrip card shows. The transcript window and the file picker
//! come in later tasks (their methods are stubs here). A child of `app`, so
//! it works on `App` directly.
//!
//! Power: no thread and no timer outside work. The worker (`transcribe`) lives
//! for one job and a download thread for one download; the only timer is the
//! one-shot `TIMER_TRANSCRIBE` that settles the queue 30 s after the start, and
//! only when transcripts are on. The queue waits that long (and while any
//! recording runs) so a boot or a recording is never slowed by a job.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::Local;
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::record::display_title;
use super::{App, DEFERRED_OPEN_CARD, WM_APP_EVENT, notif_open, poster};
use crate::i18n::{self, Key, Lang, text};
use crate::model::{Event, Notice, NoticeKind, RecEntry, RowTranscript, TranscriptOffer};
use crate::transcribe::catalog::{self, ENGINE, Model};
use crate::transcribe::download::{self, DownloadError, Item, Progress};
use crate::transcribe::engine::{self, EngineError};
use crate::transcribe::output::{self, Segment};
use crate::transcribe::queue::{Hold, JobState, Queue, QueuedJob, percent, same_audio};
use crate::transcribe::runner::{self, RunError};
use crate::transcribe::store::Store;
use crate::transcribe::{doc, gpu};
use crate::ui::menu_pop::Row;
use crate::ui::settings;
use crate::ui::settings_layout::{CardId, TranscriptView};
use crate::ui::transcript_menu::{self, MenuChoice};

/// A job that started this many times and never ended (a crash each time)
/// is failed, not run again.
pub(super) const MAX_TRIES: u8 = 2;

/// One-shot, 30 s after the start: the queue of the last run resumes.
pub(super) const TIMER_TRANSCRIBE: usize = 65;
pub(super) const SETTLE_MS: u32 = 30_000;
/// A progress word reaches the island at most this often unless the percent
/// moved.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The job on its worker thread.
// The window of Task 11 reads the live fields.
#[allow(dead_code)]
pub(super) struct Worker {
    id: u64,
    audio: PathBuf,
    model: &'static Model,
    running: runner::Running,
    /// Set when the thread sent `Finished`: a thread that ended without it
    /// died (a panic).
    finish_sent: Arc<AtomicBool>,
    started: Instant,
    /// Audio read so far, out of the whole.
    progress: Option<(u64, Option<u64>)>,
    segments: Vec<Segment>,
    title: String,
    /// The island asked it to stop (a hold, a cancel): its `Cancelled` is
    /// not a failure.
    stopping: bool,
}

/// The model download in flight.
pub(super) struct DownloadRun {
    id: u64,
    item: Item,
    /// The model the download is for (`None`: the engine alone).
    model: Option<&'static Model>,
    download: download::Download,
    progress: Option<Progress>,
    /// Stopped by the user: its partial files are thrown away.
    by_user_cancel: bool,
}

pub(super) struct TranscribeState {
    queue: Queue,
    next_job: u64,
    next_download: u64,
    worker: Option<Worker>,
    download: Option<DownloadRun>,
    /// Why a recording's transcript failed, this run only.
    failed: HashMap<PathBuf, String>,
    /// The engine cannot load for want of the Visual C++ runtime: the words
    /// that say so, until a download or a job shows it fine.
    problem: Option<String>,
    /// The last download failed: for which model (`None`: the engine alone)
    /// and why, until the next download starts.
    download_error: Option<(Option<&'static Model>, String)>,
    /// The startup hold is released (the 30 s passed, or transcripts were
    /// turned on after the start).
    settled: bool,
    /// The queue file of the last run is read into the queue.
    loaded: bool,
    /// Jobs the file brought back that already started `MAX_TRIES` times
    /// without ending: they fail at settle instead of running.
    crashed: Vec<PathBuf>,
    /// Audio whose "Bikin transkrip" notice was shown this run.
    told: Vec<PathBuf>,
    store: Store,
}

impl TranscribeState {
    pub fn new() -> Self {
        TranscribeState {
            queue: Queue::new(&[Hold::Startup]),
            next_job: 0,
            next_download: 0,
            worker: None,
            download: None,
            failed: HashMap::new(),
            problem: None,
            download_error: None,
            settled: false,
            loaded: false,
            crashed: Vec::new(),
            told: Vec::new(),
            store: Store::default_location(),
        }
    }
}

// ---- pure parts --------------------------------------------------------------

/// What a saved recording does about a transcript: whether to queue it, the
/// offer on the notice, and whether the one-time setup offer is now used up.
/// On and automatic with the model there (or on its way): queue it, say
/// nothing. On and manual with a model: the chip "Transkrip". Otherwise the
/// "Pengaturan" chip, once only.
pub(super) fn offer_for(
    enabled: bool,
    auto: bool,
    active_installed: bool,
    active_downloading: bool,
    any_installed: bool,
    setup_offered: bool,
) -> (bool, TranscriptOffer, bool) {
    if enabled && auto && (active_installed || active_downloading) {
        return (true, TranscriptOffer::None, false);
    }
    if enabled && !auto && any_installed {
        return (false, TranscriptOffer::Transcribe, false);
    }
    if !setup_offered {
        return (false, TranscriptOffer::Setup, true);
    }
    (false, TranscriptOffer::None, false)
}

/// The jobs of the last run's file that may run again, and the audio of
/// those that already started `MAX_TRIES` times.
pub(super) fn split_crashed(jobs: Vec<QueuedJob>) -> (Vec<QueuedJob>, Vec<PathBuf>) {
    let (keep, crashed): (Vec<_>, Vec<_>) = jobs.into_iter().partition(|j| j.tries < MAX_TRIES);
    (keep, crashed.into_iter().map(|j| j.audio).collect())
}

/// A worker thread ended without sending `Finished`.
pub(super) fn worker_died(thread_ended: bool, finish_sent: bool) -> bool {
    thread_ended && !finish_sent
}

/// What a cut download resumes: the model whose own part is on disk (with
/// what it needs), else the engine and the voice model when only their
/// parts are, else nothing. Never a model nobody started.
pub(super) fn resume_of(model_with_part: Option<&'static Model>, base_part: bool) -> Option<Item> {
    match model_with_part {
        Some(model) => Some(Item::Setup(model)),
        None if base_part => Some(Item::Engine),
        None => None,
    }
}

/// Why a transcript job failed, for the notice.
pub(super) fn job_reason(lang: Lang, e: &RunError) -> String {
    let key = match e {
        RunError::Decode(crate::transcribe::decode::DecodeError::NoMediaFoundation) => {
            Key::RecWhyNoMf
        }
        RunError::Decode(_) => Key::TrWhyAudio,
        RunError::Engine(EngineError::RuntimeMissing(_)) => Key::TrWhyRuntime,
        RunError::Engine(
            EngineError::NotInstalled
            | EngineError::BadLibrary(_)
            | EngineError::Mismatch(_)
            | EngineError::OtherFolder(_)
            | EngineError::Model(_),
        ) => Key::TrWhyEngine,
        RunError::Engine(EngineError::NoBackend) => Key::TrWhyNoDevice,
        RunError::Engine(EngineError::TooLong) => Key::TrWhyTooLong,
        RunError::Io(_) => Key::TrWhyWrite,
        RunError::Engine(
            EngineError::Backend | EngineError::Status(..) | EngineError::Cancelled,
        )
        | RunError::Cancelled => Key::TrWhyOther,
    };
    text(lang, key).to_owned()
}

/// Why a model download failed, for the notice.
pub(super) fn download_reason(lang: Lang, e: &DownloadError) -> String {
    match e {
        DownloadError::Status(code) => i18n::tr_why_refused(lang, *code),
        DownloadError::NoNetwork => text(lang, Key::TrWhyNoNetwork).to_owned(),
        DownloadError::Checksum(_) => text(lang, Key::TrWhyChecksum).to_owned(),
        DownloadError::DiskFull => text(lang, Key::TrWhyDiskFull).to_owned(),
        DownloadError::Disk(_) => text(lang, Key::TrWhyDisk).to_owned(),
        DownloadError::Archive(_) => text(lang, Key::TrWhyArchive).to_owned(),
        DownloadError::Cancelled => text(lang, Key::TrWhyOther).to_owned(),
    }
}

/// Whether a file picked with "Transkrip file lain..." gets a job: it has no
/// transcript yet and Transkrip is on.
fn wants_job(has_transcript: bool, enabled: bool) -> bool {
    enabled && !has_transcript
}

/// The mark of a row: what the scan found (`None` or `Done`, from the
/// sidecar) under what the island knows now. `waiting_model` is a queued
/// job that cannot start (its model is not there, or a recording holds the
/// queue).
pub(super) fn mark_of(
    scanned: RowTranscript,
    enabled: bool,
    state: Option<JobState>,
    waiting_model: bool,
    pct: Option<u8>,
    failed: bool,
) -> RowTranscript {
    match state {
        Some(JobState::Running) => RowTranscript::Running(pct.unwrap_or(0)),
        Some(JobState::Queued) if waiting_model => RowTranscript::Waiting,
        Some(JobState::Queued) => RowTranscript::Queued,
        None if failed => RowTranscript::Failed,
        None if scanned == RowTranscript::Done => RowTranscript::Done,
        None if !enabled => RowTranscript::Off,
        None => RowTranscript::None,
    }
}

fn tr_notice(lang: Lang, key: &str, title: String, body: String, kind: NoticeKind) -> Notice {
    Notice {
        key: key.into(),
        source: text(lang, Key::TrNoticeSource).into(),
        title,
        body,
        kind,
    }
}

// ---- the App side ------------------------------------------------------------

impl App {
    /// After the island is up: with transcripts on, the queue file of the
    /// last run is read now (so a job queued before the settle joins it, not
    /// replaces it) and the rest settles 30 s from now (`transcribe_settle`).
    /// Off: nothing runs.
    pub(super) fn transcribe_init(&mut self) {
        if !self.config.transcript.enabled {
            return;
        }
        self.transcribe_load();
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_TRANSCRIBE, SETTLE_MS, None);
        }
    }

    /// Reads the last run's queue file into the queue, ahead of anything
    /// queued since. Once.
    fn transcribe_load(&mut self) {
        if std::mem::replace(&mut self.tr.loaded, true) {
            return;
        }
        let text = std::fs::read_to_string(self.tr.store.queue_path()).unwrap_or_default();
        let jobs = Queue::from_json(&text, |p| p.is_file())
            .into_iter()
            .filter(|j| catalog::model(&j.model).is_some())
            .collect();
        let (keep, crashed) = split_crashed(jobs);
        // A job that comes back is not announced again.
        self.tr.told.extend(keep.iter().map(|j| j.audio.clone()));
        self.tr.queue.restore(keep);
        self.tr.crashed.extend(crashed);
    }

    /// `TIMER_TRANSCRIBE`, or Transkrip turned on before it fired: the
    /// startup hold ends. The store is swept on its own thread, a download
    /// that was cut resumes (after the sweep), the jobs that crashed Vale
    /// fail, and the first job starts when the model is there.
    pub(super) fn transcribe_settle(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TRANSCRIBE);
        }
        if self.tr.settled || !self.config.transcript.enabled {
            return;
        }
        self.tr.settled = true;
        self.transcribe_load();
        let store = self.tr.store.clone();
        let sweep = std::thread::Builder::new()
            .name("transcribe-sweep".into())
            .spawn(move || {
                let _ = store.sweep();
            });
        if let Some(item) = self.cut_download() {
            // The sweep clears the engine's staging folder: it ends first.
            if let Ok(sweep) = sweep {
                let _ = sweep.join();
            }
            self.transcribe_download_item(item);
        }
        for audio in std::mem::take(&mut self.tr.crashed) {
            let why = text(self.config.language, Key::TrWhyOther).to_owned();
            self.transcribe_failed(audio, why);
        }
        self.tr.queue.set_hold(Hold::Startup, false);
        self.transcribe_sync_hold();
        self.transcribe_try_next();
    }

    /// What the last run's cut download resumes (`resume_of`).
    fn cut_download(&self) -> Option<Item> {
        let store = &self.tr.store;
        let part = |asset| store.part_path(asset).exists();
        resume_of(
            catalog::MODELS.iter().find(|m| part(&m.asset)),
            part(&ENGINE.asset) || part(&catalog::VAD),
        )
    }

    /// Queues a transcript of `audio` with `model` (`None`: the chosen one).
    /// A repeat keeps its place. With transcripts off it says how to turn
    /// them on instead.
    pub(super) fn transcribe_queue(&mut self, audio: PathBuf, model: Option<&'static Model>) {
        if !self.config.transcript.enabled {
            self.transcript_off_note();
            return;
        }
        let model = model
            .unwrap_or_else(|| self.config.transcript.model_for(gpu::has_hardware_gpu()));
        self.tr.failed.retain(|p, _| !same_audio(p, &audio));
        let job = QueuedJob {
            audio,
            model: model.key.to_owned(),
            tries: 0,
        };
        if self.tr.queue.push(job) {
            self.transcribe_changed();
        }
        self.transcribe_try_next();
    }

    /// Takes `audio` out of the queue; a running job is stopped.
    pub(super) fn transcribe_cancel(&mut self, audio: &Path) {
        let Some(state) = self.tr.queue.cancel(audio) else {
            return;
        };
        if state == JobState::Running
            && let Some(w) = self.tr.worker.as_mut()
        {
            w.stopping = true;
            w.running.cancel();
        }
        self.transcribe_changed();
        self.transcribe_try_next();
    }

    /// A recording (or a screen recording) holds the queue: the running job
    /// goes back to the head and its thread stops; when none runs any more
    /// the next job may start.
    pub(super) fn transcribe_sync_hold(&mut self) {
        let on = self.rec.engaged();
        if self.tr.queue.set_hold(Hold::Recording, on).is_some()
            && let Some(w) = self.tr.worker.as_mut()
        {
            w.stopping = true;
            w.running.cancel();
            self.transcribe_marks_changed();
        }
        if !on {
            self.transcribe_try_next();
        }
    }

    /// Starts the head of the queue when nothing runs, nothing holds it and
    /// its model (and the engine) is installed and not being downloaded.
    pub(super) fn transcribe_try_next(&mut self) {
        if let Some(w) = self.tr.worker.as_ref() {
            // A thread that ended without a word died: its job fails.
            if worker_died(w.running.is_finished(), w.finish_sent.load(Ordering::Relaxed))
                && let Some(w) = self.tr.worker.take()
            {
                w.running.join();
                eprintln!("vale: the transcript thread ended without finishing");
                let why = text(self.config.language, Key::TrWhyOther).to_owned();
                self.transcribe_failed(w.audio, why);
            }
            return;
        }
        if !self.config.transcript.enabled {
            return;
        }
        let store = &self.tr.store;
        let engine_ok = store.engine_installed();
        let downloading = self
            .tr
            .download
            .as_ref()
            .and_then(|d| d.model)
            .map(|m| m.key);
        let ready = |j: &QueuedJob| match catalog::model(&j.model) {
            // An unknown model is let through to fail with its reason.
            None => true,
            Some(m) => engine_ok && store.model_installed(m) && downloading != Some(m.key),
        };
        let Some(job) = self.tr.queue.start_next(ready) else {
            return;
        };
        self.transcribe_start(job);
    }

    /// The worker thread of `job` (already the queue's running job).
    fn transcribe_start(&mut self, job: QueuedJob) {
        let lang = self.config.language;
        let Some(model) = catalog::model(&job.model) else {
            self.transcribe_failed(job.audio, text(lang, Key::TrWhyOther).to_owned());
            return;
        };
        let store = &self.tr.store;
        let prefs = &self.config.transcript;
        let spec = runner::Job {
            audio: job.audio.clone(),
            model_path: store.model_path(model),
            model: Some(model),
            engine_dir: store.engine_dir(),
            vad_path: store.vad_installed().then(|| store.vad_path()),
            language: prefs.language.code().map(str::to_owned),
            gpu: gpu::has_hardware_gpu(),
            threads: runner::default_threads(),
            labels: doc::labels(lang),
        };
        self.tr.next_job += 1;
        let id = self.tr.next_job;
        let (tx, wake) = (self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        let mut last: Option<(Option<u8>, Instant)> = None;
        let finish_sent = Arc::new(AtomicBool::new(false));
        let sent = finish_sent.clone();
        let spawned = runner::spawn(spec, move |ev| {
            if let runner::Event::Progress { done_ms, total_ms } = &ev {
                let pct = percent(*done_ms, *total_ms);
                if last.is_some_and(|(p, at)| p == pct && at.elapsed() < PROGRESS_EVERY) {
                    return;
                }
                last = Some((pct, Instant::now()));
            }
            if matches!(ev, runner::Event::Finished(_)) {
                sent.store(true, Ordering::Relaxed);
            }
            let _ = tx.send(Event::Transcribe(id, ev));
            wake();
        });
        let running = match spawned {
            Ok(r) => r,
            Err(e) => {
                eprintln!("vale: transcript thread did not start: {e}");
                self.transcribe_failed(job.audio, text(lang, Key::TrWhyOther).to_owned());
                return;
            }
        };
        let audio = job.audio.clone();
        let today = Local::now().date_naive();
        let title = display_title(&job.audio, lang, today);
        if !self.tr.told.iter().any(|p| same_audio(p, &job.audio)) {
            self.tr.told.push(job.audio.clone());
            let n = tr_notice(
                lang,
                "tr-job",
                text(lang, Key::TrStarting).into(),
                title.clone(),
                NoticeKind::TranscriptInfo,
            );
            self.apply_now(Event::Notice(n));
        }
        self.tr.worker = Some(Worker {
            id,
            audio: job.audio,
            model,
            running,
            finish_sent,
            started: Instant::now(),
            progress: None,
            segments: Vec::new(),
            title,
            stopping: false,
        });
        // The start is counted and written before the engine can crash.
        self.tr.queue.bump_tries(&audio);
        self.transcribe_changed();
    }

    /// A word of the worker thread, from the event queue. One of an older
    /// job is ignored.
    pub(super) fn on_transcribe(&mut self, id: u64, event: runner::Event) {
        let Some(w) = self.tr.worker.as_mut().filter(|w| w.id == id) else {
            return;
        };
        match event {
            runner::Event::Started { .. } => {}
            runner::Event::Progress { done_ms, total_ms } => {
                let before = w.progress.and_then(|(d, t)| percent(d, t));
                w.progress = Some((done_ms, total_ms));
                if before != percent(done_ms, total_ms) {
                    self.transcribe_percent_changed();
                }
            }
            runner::Event::Segment(segment) => w.segments.push(segment),
            runner::Event::Finished(result) => self.transcribe_finished(result),
        }
    }

    /// The worker is done: its thread is joined, the notice says how it
    /// went (nothing for a stop the island asked for) and the queue goes on.
    fn transcribe_finished(&mut self, result: Result<runner::Report, RunError>) {
        let Some(w) = self.tr.worker.take() else {
            return;
        };
        w.running.join();
        let lang = self.config.language;
        let audio = w.audio;
        // A stop the island asked for leaves the job where the queue put it
        // (back at the head for a hold, gone for a cancel). Anything else
        // ends the job, also one that finished as the hold came.
        if !matches!(result, Err(RunError::Cancelled)) {
            self.tr.queue.cancel(&audio);
        }
        match result {
            Ok(_) => {
                self.tr.failed.retain(|p, _| !same_audio(p, &audio));
                let title = display_title(&audio, lang, Local::now().date_naive());
                let n = tr_notice(
                    lang,
                    "tr-job",
                    text(lang, Key::TrReady).into(),
                    format!("{title} \u{b7} {}", text(lang, Key::RecClickToOpen)),
                    NoticeKind::TranscriptReady { audio },
                );
                self.apply_now(Event::Notice(n));
            }
            Err(RunError::Cancelled) => self.tr.queue.clear_tries(&audio),
            Err(e) => {
                if matches!(e, RunError::Engine(EngineError::RuntimeMissing(_))) {
                    self.tr.problem = Some(text(lang, Key::TrWhyRuntime).to_owned());
                }
                let why = job_reason(lang, &e);
                eprintln!("vale: transcript failed: {e}");
                self.transcribe_failed(audio, why);
                return;
            }
        }
        self.transcribe_changed();
        self.transcribe_try_next();
    }

    /// Records and says that the transcript of `audio` failed, then goes on
    /// with the queue.
    fn transcribe_failed(&mut self, audio: PathBuf, why: String) {
        let lang = self.config.language;
        self.tr.queue.cancel(&audio);
        self.tr.failed.insert(audio.clone(), why.clone());
        let n = tr_notice(
            lang,
            "tr-job",
            text(lang, Key::TrFailedTitle).into(),
            why,
            NoticeKind::TranscriptFailed { audio },
        );
        self.apply_now(Event::Notice(n));
        self.transcribe_changed();
        self.transcribe_try_next();
    }

    /// A recording was saved: queues its transcript when that is automatic,
    /// and says what the "Rekaman tersimpan" notice offers.
    pub(super) fn transcribe_saved(&mut self, path: &Path) -> TranscriptOffer {
        let prefs = &self.config.transcript;
        let model = prefs.model_for(gpu::has_hardware_gpu());
        let store = &self.tr.store;
        let engine = store.engine_installed();
        let (queue, offer, mark) = offer_for(
            prefs.enabled,
            prefs.auto,
            engine && store.model_installed(model),
            self.tr
                .download
                .as_ref()
                .is_some_and(|d| d.model.is_some_and(|m| m.key == model.key)),
            engine && !store.installed_models().is_empty(),
            prefs.setup_offered,
        );
        if mark {
            self.config.transcript.setup_offered = true;
            self.save_config();
        }
        if queue {
            self.transcribe_queue(path.to_path_buf(), None);
        }
        offer
    }

    /// Settings' switch. Off stops the job, empties the queue and cancels a
    /// download (its partial files go; models and transcripts stay). On
    /// after the start releases the startup hold at once.
    pub(super) fn transcribe_set_enabled(&mut self, on: bool) {
        if self.config.transcript.enabled != on {
            self.config.transcript.enabled = on;
            self.save_config();
        }
        if on {
            // Before the settle ran, it runs now (load, sweep, resume).
            if self.tr.settled {
                self.transcribe_try_next();
            } else {
                self.transcribe_settle();
            }
            return;
        }
        if self.tr.queue.clear().is_some()
            && let Some(w) = self.tr.worker.as_mut()
        {
            w.stopping = true;
            w.running.cancel();
        }
        self.transcribe_cancel_download();
        self.tr.failed.clear();
        self.transcribe_changed();
    }

    /// Downloads what `model` needs (the engine and the voice model when
    /// missing). One download at a time.
    pub(super) fn transcribe_download(&mut self, model: &'static Model) {
        self.transcribe_download_item(Item::Setup(model));
    }

    fn transcribe_download_item(&mut self, item: Item) {
        if self.tr.download.is_some() {
            return;
        }
        self.tr.next_download += 1;
        let id = self.tr.next_download;
        let (tx, wake) = (self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        let started = download::start(self.tr.store.clone(), item, move |ev| {
            let _ = tx.send(Event::TranscribeDownload(id, ev));
            wake();
        });
        match started {
            Ok(download) => {
                self.tr.download_error = None;
                let model = match item {
                    Item::Model(m) | Item::Setup(m) => Some(m),
                    Item::Engine => None,
                };
                self.tr.download = Some(DownloadRun {
                    id,
                    item,
                    model,
                    download,
                    progress: None,
                    by_user_cancel: false,
                });
            }
            Err(e) => eprintln!("vale: download thread did not start: {e}"),
        }
    }

    /// Stops the download; its partial files are discarded when it ends.
    pub(super) fn transcribe_cancel_download(&mut self) {
        if let Some(d) = self.tr.download.as_mut() {
            d.by_user_cancel = true;
            d.download.cancel();
        }
    }

    /// Deletes an installed model. Refused for the model of the running job
    /// (Settings shows that one as in use, its Hapus off).
    pub(super) fn transcribe_delete(&mut self, model: &'static Model) {
        if self.tr.worker.as_ref().is_some_and(|w| w.model.key == model.key) {
            return;
        }
        if let Err(e) = self.tr.store.delete_model(model) {
            eprintln!("vale: cannot delete {}: {e}", model.key);
        }
    }

    /// Settings' radio: the model new transcripts use.
    pub(super) fn transcribe_pick(&mut self, key: &str) {
        let Some(model) = catalog::model(key) else {
            return;
        };
        self.config.transcript.model = Some(model.key.to_owned());
        self.save_config();
    }

    /// Settings' "Unduh" (of the offer or a model row). While the active
    /// model is not installed, the one fetched becomes the active one, so
    /// the next recording does not wait on a model nobody downloads.
    pub(super) fn transcribe_download_key(&mut self, key: &str) {
        let Some(model) = catalog::model(key) else {
            return;
        };
        let active = self.config.transcript.model_for(gpu::has_hardware_gpu());
        if active.key != model.key && !self.tr.store.model_installed(active) {
            self.config.transcript.model = Some(model.key.to_owned());
            self.save_config();
        }
        self.transcribe_download(model);
    }

    /// Settings' "Buka folder": Explorer on the engine and models' folder
    /// (made first when missing).
    pub(super) fn transcribe_open_folder(&mut self) {
        let dir = self.tr.store.root().to_path_buf();
        let _ = std::fs::create_dir_all(&dir);
        notif_open::allow_foreground();
        let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
    }

    /// What Settings' Transkrip card shows. Read when Settings opens or
    /// follows a change: a few file checks and the size of the store.
    pub(super) fn transcript_view(&self) -> TranscriptView {
        let store = &self.tr.store;
        let index = |m: &Model| catalog::MODELS.iter().position(|x| x.key == m.key);
        let download = self.tr.download.as_ref().and_then(|d| {
            let i = index(d.model?)?;
            // Before the thread's first word: nothing done of the whole.
            let progress = d.progress.unwrap_or_else(|| Progress {
                done: 0,
                total: download::missing(store, d.item).iter().map(|a| a.size).sum(),
            });
            Some((i, progress))
        });
        let (download_error, engine_error) = match &self.tr.download_error {
            Some((Some(m), why)) => (index(m).map(|i| (i, why.clone())), None),
            Some((None, why)) => (None, Some(why.clone())),
            None => (None, None),
        };
        TranscriptView {
            prefs: self.config.transcript.clone(),
            gpu: gpu::has_hardware_gpu(),
            installed: catalog::MODELS.map(|m| store.model_installed(&m)),
            engine_installed: store.engine_installed(),
            download,
            download_error,
            in_use: self.tr.worker.as_ref().and_then(|w| index(w.model)),
            used_bytes: store.used_bytes(),
            problem: self.tr.problem.clone().or(engine_error),
        }
    }

    /// A word of the download thread. One of an older download is ignored.
    /// An open Settings follows each one (the progress, the end).
    pub(super) fn on_transcribe_download(&mut self, id: u64, event: download::Event) {
        let Some(run) = self.tr.download.as_mut().filter(|d| d.id == id) else {
            return;
        };
        let download::Event::Finished(result) = event else {
            if let download::Event::Progress(p) = event {
                run.progress = Some(p);
                self.sync_settings();
            }
            return;
        };
        let Some(run) = self.tr.download.take() else {
            return;
        };
        run.download.join();
        let (item, model, by_user) = (run.item, run.model, run.by_user_cancel);
        let lang = self.config.language;
        let notice = match result {
            Ok(()) => {
                let missing = engine::runtime_missing(&self.tr.store.engine_dir());
                if missing.is_empty() {
                    self.tr.problem = None;
                    model.map(|m| (Key::TrModelReady, m.name.to_owned()))
                } else {
                    // The download worked; what is missing is Windows'.
                    let why = text(lang, Key::TrWhyRuntime).to_owned();
                    self.tr.problem = Some(why.clone());
                    Some((Key::TrNoticeSource, why))
                }
            }
            Err(DownloadError::Cancelled) => {
                if by_user {
                    let _ = self
                        .tr
                        .store
                        .discard_parts(&download::assets_of(item));
                }
                None
            }
            Err(e) => {
                let why = download_reason(lang, &e);
                self.tr.download_error = Some((model, why.clone()));
                Some((Key::TrDownloadFailed, why))
            }
        };
        // Settings shows its own result while it is open.
        if let Some((title, body)) = notice
            && !crate::ui::settings::is_open()
        {
            let n = tr_notice(
                lang,
                "tr-setup",
                text(lang, title).into(),
                body,
                NoticeKind::TranscriptSettings,
            );
            self.apply_now(Event::Notice(n));
        }
        self.transcribe_try_next();
        self.sync_settings();
    }

    /// The queue changed: the file follows, the Rekam rows re-read and an
    /// open Settings follows (the model in use).
    fn transcribe_changed(&mut self) {
        let path = self.tr.store.queue_path();
        if self.tr.queue.is_empty() {
            let _ = std::fs::remove_file(&path);
        } else {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = output::write_atomic(&path, &self.tr.queue.to_json()) {
                eprintln!("vale: cannot write the transcript queue: {e}");
            }
        }
        self.transcribe_marks_changed();
        self.sync_settings();
    }

    /// A row's state changed: the Rekam page, when it shows, puts the new
    /// marks on the rows it has at once and reads the folder again (a finished
    /// job left an `.srt`).
    fn transcribe_marks_changed(&mut self) {
        if self.rec.page_shown() {
            self.rec_remark();
            self.rec_rescan();
        }
    }

    /// The running job's percent moved: only its row needs the new number.
    fn transcribe_percent_changed(&mut self) {
        if !self.rec.page_shown() {
            return;
        }
        let Some(audio) = self.tr.worker.as_ref().map(|w| w.audio.clone()) else {
            return;
        };
        if self.rec.recent.iter().any(|r| same_audio(&r.path, &audio)) {
            self.rec_remark();
        }
    }

    /// The last scan with the queue's marks over it, shown now.
    fn rec_remark(&mut self) {
        let rows = self.transcribe_marks(self.rec.recent.clone());
        self.apply_now(Event::RecRecent(rows));
    }

    /// `entries` with the transcript marks the island knows over what the
    /// scan found (`None` or `Done`).
    pub(super) fn transcribe_marks(&self, entries: Vec<RecEntry>) -> Vec<RecEntry> {
        let enabled = self.config.transcript.enabled;
        let tr = &self.tr;
        entries
            .into_iter()
            .map(|mut e| {
                let state = tr.queue.state(&e.path);
                let waiting = tr.queue.held()
                    || tr
                        .queue
                        .jobs()
                        .find(|j| same_audio(&j.audio, &e.path))
                        .and_then(|j| catalog::model(&j.model))
                        .is_none_or(|m| !tr.store.model_installed(m));
                let pct = tr
                    .worker
                    .as_ref()
                    .filter(|w| same_audio(&w.audio, &e.path))
                    .and_then(|w| w.progress)
                    .and_then(|(d, t)| percent(d, t));
                let failed = tr.failed.keys().any(|p| same_audio(p, &e.path));
                let scanned = if e.transcript == RowTranscript::Done {
                    RowTranscript::Done
                } else {
                    RowTranscript::None
                };
                e.transcript = mark_of(scanned, enabled, state, waiting, pct, failed);
                e
            })
            .collect()
    }

    /// Quitting: the worker is stopped and joined (the queue file keeps its
    /// job for the next run) and the download stops keeping its partial files.
    pub(super) fn transcribe_shutdown(&mut self) {
        if let Some(w) = self.tr.worker.take() {
            w.running.cancel();
            w.running.join();
        }
        if let Some(d) = self.tr.download.take() {
            d.download.cancel();
        }
    }

    /// Opens the transcript window of `audio`.
    pub(super) fn open_transcript(&mut self, audio: PathBuf) {
        let _ = audio;
    }

    /// Opens Settings at the Transkrip card, once the handler holding the
    /// app returns (opening a window sends messages).
    pub(super) fn open_transcript_settings(&mut self) {
        DEFERRED_OPEN_CARD.set(Some(CardId::Transcript));
    }

    /// Shows the file picker for a transcript of any audio file, opening in
    /// the recordings folder. The choice comes back as `transcribe_file`.
    pub(super) fn rec_pick_file(&mut self) {
        notif_open::allow_foreground();
        settings::pick_rec_audio(self.hwnd, self.config.recording.dir());
    }

    /// An audio file picked with "Transkrip file lain...": a job is queued
    /// when it has no transcript and Transkrip is on, then its window opens.
    pub(super) fn transcribe_file(&mut self, audio: PathBuf) {
        let has = doc::load(&audio, "").has_transcript;
        if wants_job(has, self.config.transcript.enabled) {
            self.transcribe_queue(audio.clone(), None);
        }
        self.open_transcript(audio);
    }

    /// The recording of Rekam row `i` and its menu rows, for its mark now.
    pub(super) fn rec_row_menu(&self, i: usize) -> Option<(PathBuf, Vec<Row>)> {
        let entry = self.state.rec_recent().get(i)?;
        let installed: Vec<&'static Model> = catalog::MODELS
            .iter()
            .filter(|m| self.tr.store.model_installed(m))
            .collect();
        let active = self.config.transcript.model_for(gpu::has_hardware_gpu());
        let rows =
            transcript_menu::rows(self.config.language, entry.transcript, &installed, active);
        Some((entry.path.clone(), rows))
    }

    /// A row of the transcript menu was picked.
    pub(super) fn transcript_menu_choice(&mut self, choice: MenuChoice) {
        match choice {
            MenuChoice::View(audio) => self.open_transcript(audio),
            MenuChoice::Transcribe(audio) => self.transcribe_queue(audio, None),
            MenuChoice::Redo(audio, model) => self.transcribe_queue(audio, Some(model)),
            MenuChoice::Cancel(audio) => self.transcribe_cancel(&audio),
        }
    }

    /// Says that transcripts are off and where to turn them on.
    pub(super) fn transcript_off_note(&mut self) {
        let lang = self.config.language;
        let n = tr_notice(
            lang,
            "tr-off",
            text(lang, Key::TrOffTitle).into(),
            text(lang, Key::TrOffBody).into(),
            NoticeKind::TranscriptSettings,
        );
        self.apply_now(Event::Notice(n));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::decode::DecodeError;

    fn qj(audio: &str, tries: u8) -> QueuedJob {
        QueuedJob {
            audio: PathBuf::from(audio),
            model: "whisper-small".into(),
            tries,
        }
    }

    #[test]
    fn a_picked_file_is_queued_only_without_a_transcript_and_with_transcripts_on() {
        assert!(wants_job(false, true));
        assert!(!wants_job(true, true), "it has one: just open it");
        assert!(!wants_job(false, false), "transcripts are off");
    }

    #[test]
    fn a_job_that_crashed_twice_is_failed_not_run() {
        let (keep, crashed) = split_crashed(vec![qj("a", 0), qj("b", 1), qj("c", 2), qj("d", 3)]);
        assert_eq!(keep, [qj("a", 0), qj("b", 1)]);
        assert_eq!(crashed, [PathBuf::from("c"), PathBuf::from("d")]);
    }

    #[test]
    fn a_job_queued_before_the_settle_joins_the_last_runs_jobs() {
        // The file is read at init; a pill clicked in the first 30 s pushes
        // after it was read. Reading late must still put the old jobs first.
        let mut queue = Queue::new(&[Hold::Startup]);
        queue.push(qj("new", 0));
        let (keep, _) = split_crashed(vec![qj("old1", 1), qj("old2", 0)]);
        queue.restore(keep);
        let order: Vec<_> = queue.jobs().map(|j| j.audio.to_string_lossy().into_owned()).collect();
        assert_eq!(order, ["old1", "old2", "new"]);
        assert_eq!(queue.jobs().next().map(|j| j.tries), Some(1));
    }

    #[test]
    fn only_a_thread_that_ended_without_finished_died() {
        assert!(worker_died(true, false));
        assert!(!worker_died(true, true), "Finished is on its way");
        assert!(!worker_died(false, false));
        assert!(!worker_died(false, true));
    }

    #[test]
    fn a_cut_download_resumes_only_what_has_parts() {
        let small = &catalog::MODELS[1];
        assert_eq!(resume_of(Some(small), false), Some(Item::Setup(small)));
        assert_eq!(resume_of(Some(small), true), Some(Item::Setup(small)));
        assert_eq!(resume_of(None, true), Some(Item::Engine));
        assert_eq!(resume_of(None, false), None);
    }

    #[test]
    fn offer_for_each_case() {
        use TranscriptOffer::{None as Nothing, Setup, Transcribe};
        // (enabled, auto, active_installed, active_downloading, any_installed, setup_offered)
        let table = [
            // On and automatic, model there or coming: queue, silent.
            ((true, true, true, false, true, false), (true, Nothing, false)),
            ((true, true, false, true, false, false), (true, Nothing, false)),
            ((true, true, true, false, true, true), (true, Nothing, false)),
            // On and manual with a model: the chip.
            ((true, false, true, false, true, false), (false, Transcribe, false)),
            ((true, false, false, false, true, true), (false, Transcribe, false)),
            // On, automatic, only another model installed: set up, once.
            ((true, true, false, false, true, false), (false, Setup, true)),
            ((true, true, false, false, true, true), (false, Nothing, false)),
            // On with nothing installed: set up, once.
            ((true, false, false, false, false, false), (false, Setup, true)),
            ((true, true, false, false, false, true), (false, Nothing, false)),
            // Off: set up once, then silent, whatever is installed.
            ((false, true, true, false, true, false), (false, Setup, true)),
            ((false, false, false, false, false, false), (false, Setup, true)),
            ((false, true, true, false, true, true), (false, Nothing, false)),
            ((false, true, false, false, false, true), (false, Nothing, false)),
        ];
        for ((on, auto, active, loading, any, offered), want) in table {
            assert_eq!(
                offer_for(on, auto, active, loading, any, offered),
                want,
                "{on} {auto} {active} {loading} {any} {offered}"
            );
        }
    }

    #[test]
    fn job_reason_names_each_failure() {
        use crate::transcribe::runner::RunError::{Cancelled, Decode, Engine, Io};
        let id = |e: RunError| job_reason(Lang::Id, &e);
        assert_eq!(id(Decode(DecodeError::NoMediaFoundation)), "Media Foundation tidak ada di Windows ini");
        for e in [
            DecodeError::Open("x".into()),
            DecodeError::Read("x".into()),
            DecodeError::Format("x".into()),
        ] {
            assert_eq!(id(Decode(e)), "File audio tidak bisa dibaca");
        }
        assert!(id(Engine(EngineError::RuntimeMissing(vec!["msvcp140.dll"]))).starts_with("Butuh Microsoft Visual C++"));
        let damaged = "Mesin transkrip rusak. Hapus modelnya lalu unduh lagi di Pengaturan";
        for e in [
            EngineError::NotInstalled,
            EngineError::BadLibrary("x".into()),
            EngineError::Mismatch("x".into()),
            EngineError::OtherFolder(PathBuf::from("x")),
            EngineError::Model("x".into()),
        ] {
            assert_eq!(id(Engine(e)), damaged);
        }
        assert_eq!(id(Engine(EngineError::NoBackend)), "Mesin transkrip tidak bisa jalan di PC ini");
        assert_eq!(id(Engine(EngineError::TooLong)), "Rekaman terlalu panjang untuk model ini");
        assert_eq!(id(Io("x".into())), "Transkrip tidak bisa disimpan di folder ini");
        for e in [EngineError::Backend, EngineError::Status(3, "x".into())] {
            assert_eq!(id(Engine(e)), "Mesin transkrip gagal. Coba lagi");
        }
        assert_eq!(id(Cancelled), "Mesin transkrip gagal. Coba lagi");
        assert_eq!(
            job_reason(Lang::En, &Engine(EngineError::TooLong)),
            "The recording is too long for this model"
        );
    }

    #[test]
    fn download_reason_in_plain_words() {
        let id = |e: DownloadError| download_reason(Lang::Id, &e);
        assert_eq!(id(DownloadError::DiskFull), "Disk penuh");
        assert_eq!(id(DownloadError::NoNetwork), "Tidak ada jaringan");
        assert_eq!(id(DownloadError::Status(404)), "Server menolak unduhan (404)");
        assert_eq!(id(DownloadError::Checksum("m.gguf")), "File unduhan rusak, coba lagi");
        assert_eq!(id(DownloadError::Disk("x".into())), "File tidak bisa ditulis");
        assert_eq!(id(DownloadError::Archive("x".into())), "Paket mesin tidak cocok");
        assert_eq!(
            download_reason(Lang::En, &DownloadError::Status(404)),
            "The server refused the download (404)"
        );
    }

    #[test]
    fn marks_overlay_the_queue() {
        use RowTranscript::{Done, Failed, Off, Queued, Running, Waiting};
        let none = RowTranscript::None;
        let q = Some(JobState::Queued);
        // Off with nothing on disk, and with a transcript.
        assert_eq!(mark_of(none, false, None, false, None, false), Off);
        assert_eq!(mark_of(Done, false, None, false, None, false), Done);
        // On with nothing, with a transcript.
        assert_eq!(mark_of(none, true, None, false, None, false), none);
        assert_eq!(mark_of(Done, true, None, false, None, false), Done);
        // Queued, and queued with its model missing.
        assert_eq!(mark_of(none, true, q, false, None, false), Queued);
        assert_eq!(mark_of(none, true, q, true, None, false), Waiting);
        // Running: the percent, 0 when the total is not known.
        assert_eq!(mark_of(none, true, Some(JobState::Running), false, Some(42), false), Running(42));
        assert_eq!(mark_of(none, true, Some(JobState::Running), false, None, false), Running(0));
        // Failed this run.
        assert_eq!(mark_of(none, true, None, false, None, true), Failed);
        // Queued again after a failure shows the queue, not the failure.
        assert_eq!(mark_of(none, true, q, false, None, true), Queued);
    }
}
