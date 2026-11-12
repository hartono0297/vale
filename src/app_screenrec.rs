//! The island's side of the screen recorder (`crate::screenrec`): where a
//! screen recording stands, from the picker through the 3-2-1 to the saved
//! file, and what the Rekam page's Layar tab, the screen hotkey and the tray
//! ask of it. A child of `app`, so it works on `App` directly.
//!
//! The picker answers with `WM_APP_SCREEN_PICK`; the countdown is one
//! one-shot timer a second (`TIMER_SCREEN_COUNTDOWN`) with Esc registered for
//! its three seconds. Start and stop run on short-lived threads
//! (`screen-start`, `screen-stop`): the capture, the encoder and the audio
//! open, and the file finalizes, off the UI thread. Each posts
//! `ScreenNote::Started` / `Stopped` back, tagged with its session number;
//! a word of an older session is ignored. A toggle while the picker is up or
//! a start or a stop runs is ignored.
//!
//! The sound is phase 1's recorder in its mix mode (`rec-mix`), pushing mono
//! into the screen recorder. It is stopped first, so the rest of its
//! timeline still reaches the video.
//!
//! Power: nothing here runs at idle. A thread, a timer or a hook exists only
//! while the picker is up, the countdown runs or a screen recording runs.
//! The video list is read on a short-lived thread (`rec-scan`) when the
//! Rekam page comes on screen and after a recording ends.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::Local;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, KillTimer, SetTimer, WM_APP};

use super::record::{
    self, RECENT_LIMIT, choose_dir, device_switched_notice, fallback_notice, folder_blocked_notice,
    is_recorded, mic_denied_notice, probe_writable, scan_recent, source_lost_notice,
};
use super::{App, WM_APP_EVENT, notif_open, poster};
use crate::capture::rec_pick::{PickTarget, Picked};
use crate::clipboard::image::Thumb;
use crate::i18n::{self, Key, Lang, text};
use crate::model::{
    Event, Notice, NoticeKind, Page, RecEntry, RecKind, RecSources, RecStatus, ScreenNote,
    TranscriptOffer,
};
use crate::recorder::mix::MixOptions;
use crate::recorder::naming::file_stem;
use crate::recorder::prefs::{VideoQuality, video_fallback_folder};
use crate::recorder::{Failure, RecError, RecEvent, Recorder, Source, Sources, devices};
use crate::screenrec::audio::Forward;
use crate::screenrec::exclude::Excluder;
use crate::screenrec::files::{existing_files, files_of, part_of, unique_stem};
use crate::screenrec::session::{CursorEffects, PauseWhy, ScreenClock, ScreenPause, options_for};
use crate::screenrec::thumb::{grab, thumb_at};
use crate::screenrec::watch::WindowWatch;
use crate::screenrec::wheel::WheelHook;
use crate::screenrec::zoom::ZoomState;
use crate::screenrec::{EndReason, Error, ScreenRecorder, Summary, Target};
use crate::shortcuts;
use crate::transcribe::decode;
use crate::ui::render::VideoThumbView;
use crate::ui::{settings, tray};

/// The one-shot behind each number of the 3-2-1.
pub(super) const TIMER_SCREEN_COUNTDOWN: usize = 70;
/// The number the countdown starts on.
pub(super) const COUNTDOWN_FROM: u8 = 3;
/// How long each number shows.
pub(super) const COUNTDOWN_STEP_MS: u32 = 1_000;
/// The window watch's word: the recorded window was minimised (`wparam` 1)
/// or restored (0).
pub(super) const WM_APP_SCREEN_WATCH: u32 = WM_APP + 91;

/// Where a screen recording stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ScreenPhase {
    /// None: nothing is picked, counted down or recorded.
    #[default]
    Idle,
    /// The picker is up: a screen, a window or an area is being chosen.
    Picking,
    /// The 3-2-1 before the recording, with the number on show.
    Countdown(u8),
    /// The start thread opens the capture and the encoder.
    Starting,
    Recording,
    /// The stop thread finalizes the file.
    Stopping,
}

impl ScreenPhase {
    /// A screen recording is somewhere between the picker and the saved
    /// file: an audio recording must not start.
    pub(super) fn busy(self) -> bool {
        self != ScreenPhase::Idle
    }

    /// It is about to record, records or finalizes: transcript jobs hold
    /// (the picker alone holds nothing, it may still be cancelled).
    pub(super) fn engaged(self) -> bool {
        !matches!(self, ScreenPhase::Idle | ScreenPhase::Picking)
    }

    pub(super) fn recording(self) -> bool {
        self == ScreenPhase::Recording
    }
}

/// The screen recording's session, as `App` keeps it.
pub(super) struct ScreenState {
    phase: ScreenPhase,
    /// The newest saved video's thumbnail and the id its notice names
    /// (`NoticeKind::RecSaved::thumb`).
    thumb: Option<(u64, Thumb)>,
    /// The newest session's number; a word of an older one is ignored.
    session: u64,
    recorder: Option<ScreenRecorder>,
    /// The mix recorder feeding the video's sound; `None` with both sources
    /// off, or when the sound could not start.
    audio: Option<Recorder>,
    /// What the picker chose, from the countdown until the recording is over.
    picked: Option<Picked>,
    /// The sources that are recorded (the picker's toggles; none when the
    /// sound could not start, and once every one of them is lost).
    sources: RecSources,
    /// The video has sound in it (the mix started), whatever was lost since.
    sound: bool,
    /// The mix recorder's words that came before the start thread reported
    /// (`AudioWord::Keep`), oldest first.
    early_audio: Vec<RecEvent>,
    /// The sources whose loss a notice named, `[mic, system]`.
    told_lost: [bool; 2],
    /// The video being written (`<name>.mp4`), from the start to the stop.
    path: Option<PathBuf>,
    clock: ScreenClock,
    pause: ScreenPause,
    /// Keeps Vale's windows out of the video; lives from just before the
    /// capture starts until the recording thread has ended.
    excluder: Option<Excluder>,
    /// Tells when the recorded window is minimised or restored.
    watch: Option<WindowWatch>,
    /// Esc is registered for the countdown.
    esc: bool,
    /// The capture ended by itself before the start thread reported.
    ended: bool,
    /// The last thumbnail id given out; every saved video gets a new one.
    next_thumb: u64,
    /// The island's mirror of the video's zoom: on or off, and the level.
    /// A new recording starts unzoomed at 2x.
    zoom: ZoomState,
    /// Ctrl+Alt+wheel changes the level; exists only while the video is zoomed.
    wheel: Option<WheelHook>,
    /// The last scan of the video folder as it came (the marks of the queue
    /// go over a copy of it).
    pub(super) videos: Vec<RecEntry>,
    /// Every video in the folder, not just the listed ones.
    pub(super) total: usize,
    /// The Rekam page was on screen at the last refresh (a new showing
    /// re-reads the folder).
    page_shown: bool,
    starting: Option<JoinHandle<Started>>,
    stopping: Option<JoinHandle<Stopped>>,
}

impl Default for ScreenState {
    fn default() -> ScreenState {
        ScreenState {
            phase: ScreenPhase::Idle,
            thumb: None,
            session: 0,
            recorder: None,
            audio: None,
            picked: None,
            sources: RecSources {
                mic: false,
                system: false,
            },
            sound: false,
            early_audio: Vec::new(),
            told_lost: [false; 2],
            path: None,
            clock: ScreenClock::new(Instant::now()),
            pause: ScreenPause::default(),
            excluder: None,
            watch: None,
            esc: false,
            ended: false,
            next_thumb: 0,
            zoom: ZoomState::default(),
            wheel: None,
            videos: Vec::new(),
            total: 0,
            page_shown: false,
            starting: None,
            stopping: None,
        }
    }
}

impl ScreenState {
    /// The newest saved video's thumbnail, for the notice that names it.
    pub(super) fn video_notice(&self) -> Option<VideoThumbView<'_>> {
        self.thumb
            .as_ref()
            .map(|(id, thumb)| VideoThumbView { id: *id, thumb })
    }

    pub(super) fn busy(&self) -> bool {
        self.phase.busy()
    }

    pub(super) fn engaged(&self) -> bool {
        self.phase.engaged()
    }

    pub(super) fn recording(&self) -> bool {
        self.phase.recording()
    }

    /// The countdown runs and Esc was registered for it: a registration
    /// that was taken away meanwhile (Settings recorded a combo) comes back.
    pub(super) fn wants_esc(&self) -> bool {
        self.esc && matches!(self.phase, ScreenPhase::Countdown(_))
    }

    /// The devices the sound of the screen recording is taken from, `[mic,
    /// system]`; `None` when no sound is recorded.
    pub(super) fn device_names(&self) -> Option<[String; 2]> {
        self.audio.as_ref().map(Recorder::device_names)
    }

    /// The recorded target is one window (it can be minimised or closed).
    fn records_window(&self) -> bool {
        matches!(
            self.picked.as_ref().map(|p| &p.target),
            Some(PickTarget::Window(_))
        )
    }
}

// ---- the rules (pure) ----------------------------------------------------------

/// What the screen hotkey, the tray entry and the Layar tab's button do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScreenToggle {
    /// Opens the picker.
    Pick,
    CancelCountdown,
    Stop,
    /// The picker is up, or a start or a stop runs: a press is ignored.
    Nothing,
}

pub(super) fn toggle_of(phase: ScreenPhase) -> ScreenToggle {
    match phase {
        ScreenPhase::Idle => ScreenToggle::Pick,
        ScreenPhase::Countdown(_) => ScreenToggle::CancelCountdown,
        ScreenPhase::Recording => ScreenToggle::Stop,
        ScreenPhase::Picking | ScreenPhase::Starting | ScreenPhase::Stopping => {
            ScreenToggle::Nothing
        }
    }
}

/// The number after `n` in the countdown; `None` after the 1, when the
/// recording starts.
fn next_count(n: u8) -> Option<u8> {
    n.checked_sub(1).filter(|&next| next > 0)
}

/// A recording of the other kind is in the way of starting or toggling one
/// of `kind`: only one runs at a time.
pub(super) fn busy_with_other(kind: RecKind, audio_busy: bool, screen_busy: bool) -> bool {
    match kind {
        RecKind::Audio => screen_busy,
        RecKind::Screen => audio_busy,
    }
}

/// "Rekaman lain sedang jalan".
pub(crate) fn busy_notice(lang: Lang) -> Notice {
    record::notice(
        "rec-busy",
        lang,
        text(lang, Key::RecBusyTitle).into(),
        text(lang, Key::RecBusyBody).into(),
        NoticeKind::RecInfo,
    )
}

/// Why a saved screen recording ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ended {
    /// The user stopped it.
    Stopped,
    /// The recorded window was closed.
    WindowClosed,
    /// The recorded monitor went away.
    DisplayGone,
}

/// `window`: one window was recorded, not a monitor or an area.
pub(crate) fn ended_of(reason: EndReason, window: bool) -> Ended {
    match (reason, window) {
        (EndReason::Stopped, _) => Ended::Stopped,
        (EndReason::TargetClosed, true) => Ended::WindowClosed,
        (EndReason::TargetClosed, false) => Ended::DisplayGone,
    }
}

/// "Video tersimpan" (or why it ended) over "3 mnt · klik untuk buka", as
/// `record::saved_notice`. `thumb` is the id of the video's thumbnail, when
/// one could be read.
pub(crate) fn video_saved_notice(
    lang: Lang,
    path: PathBuf,
    duration: Duration,
    ended: Ended,
    offer: TranscriptOffer,
    thumb: Option<u64>,
) -> Notice {
    let title = match ended {
        Ended::Stopped => Key::ScreenSavedTitle,
        Ended::WindowClosed => Key::ScreenWindowClosedTitle,
        Ended::DisplayGone => Key::ScreenDisplayGoneTitle,
    };
    let hint = if offer == TranscriptOffer::Setup {
        Key::TrWantOne
    } else {
        Key::RecClickToOpen
    };
    record::notice(
        "rec-saved",
        lang,
        text(lang, title).into(),
        format!(
            "{} \u{b7} {}",
            i18n::rec_duration(lang, duration),
            text(lang, hint)
        ),
        NoticeKind::RecSaved { path, offer, thumb },
    )
}

/// The reason of a screen recording's failure, in the user's words.
fn why_of(e: &Error) -> Key {
    match e {
        Error::NoMediaFoundation => Key::RecWhyNoMf,
        Error::CaptureUnsupported => Key::ScreenWhyUnsupported,
        Error::TargetGone => Key::ScreenWhyGone,
        Error::TargetMinimized => Key::ScreenWhyMinimized,
        Error::NoEncoder(_) => Key::ScreenWhyEncoder,
        Error::Gpu(_) => Key::ScreenWhyGpu,
        Error::Io(_) => Key::ScreenWhyWrite,
        Error::Windows { .. } => Key::ScreenWhyOther,
    }
}

/// "Rekam layar gagal" over the reason. With `part` (what was recorded up to
/// the failure) a click shows that file.
pub(crate) fn screen_failure_notice(lang: Lang, e: &Error, part: Option<PathBuf>) -> Notice {
    record::notice(
        "rec-failed",
        lang,
        text(lang, Key::ScreenFailedTitle).into(),
        text(lang, why_of(e)).into(),
        NoticeKind::RecFailed { reveal: part },
    )
}

/// "Video jalan tanpa suara" over the reason: the sound could not start or
/// broke off, and the video records on, silent.
pub(crate) fn no_sound_notice(lang: Lang, failure: &Failure) -> Notice {
    let why = match failure {
        Failure::NoDevice => Key::RecWhyNoDevice,
        Failure::NoMediaFoundation => Key::RecWhyNoMf,
        Failure::NoAudio => Key::RecWhyNoAudio,
        // A mix writes no file: the folder and disk reasons cannot come.
        Failure::FolderBlocked
        | Failure::FolderGone
        | Failure::DiskFull
        | Failure::Unfinished { .. }
        | Failure::Other(_) => Key::RecWhyOther,
    };
    record::notice(
        "rec-no-sound",
        lang,
        text(lang, Key::ScreenNoSoundTitle).into(),
        text(lang, why).into(),
        NoticeKind::RecInfo,
    )
}

/// What becomes of a word of the mix recorder that arrives in a phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AudioWord {
    /// The recording runs: the word is acted on now.
    Tell,
    /// The start thread has not reported yet (the mix speaks as soon as its
    /// sources are open): kept until it has, dropped if the start fails.
    Keep,
    Drop,
}

pub(super) fn audio_word_in(phase: ScreenPhase) -> AudioWord {
    match phase {
        ScreenPhase::Recording => AudioWord::Tell,
        ScreenPhase::Starting => AudioWord::Keep,
        ScreenPhase::Idle
        | ScreenPhase::Picking
        | ScreenPhase::Countdown(_)
        | ScreenPhase::Stopping => AudioWord::Drop,
    }
}

/// Every recorded source is lost (`RecEvent::NoInput`): the notice for it.
/// The last loss comes without a `SourceLost` of its own, so this is its
/// only word. `told`: the sources whose loss a notice already named, as
/// `[mic, system]`; `None` when that covers every recorded one (the last
/// loss was the mic Windows refused, which has its own notice).
pub(crate) fn no_input_notice(lang: Lang, recorded: RecSources, told: [bool; 2]) -> Option<Notice> {
    let untold = [Source::Mic, Source::System]
        .into_iter()
        .any(|s| is_recorded(recorded, s) && !told[s.index()]);
    untold.then(|| no_sound_notice(lang, &Failure::NoDevice))
}

/// "Video dibuang": the files went to the Recycle Bin.
pub(crate) fn discarded_notice(lang: Lang) -> Notice {
    record::notice(
        "rec-discarded",
        lang,
        text(lang, Key::ScreenDiscardedTitle).into(),
        String::new(),
        NoticeKind::RecInfo,
    )
}

/// The picker's choice as the recorder takes it.
pub(super) fn target_of(t: &PickTarget) -> Target {
    match *t {
        PickTarget::Monitor(m) => Target::Monitor(m),
        PickTarget::Window(w) => Target::Window(w),
        PickTarget::Area(r) => Target::Region {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        },
    }
}

/// The island's REC clock of a screen recording.
fn status_of(
    clock: &ScreenClock,
    sources: RecSources,
    muted: [bool; 2],
    zoom: Option<f64>,
) -> RecStatus {
    RecStatus {
        kind: RecKind::Screen,
        started: clock.started,
        paused_total: clock.paused_total,
        paused_since: clock.paused_since,
        sources,
        muted,
        zoom,
    }
}

// ---- the start and stop threads ----------------------------------------------

/// What the `screen-start` thread is given.
struct StartJob {
    target: Target,
    /// The recorded window's text, else empty.
    title: String,
    sources: RecSources,
    fps: u32,
    /// The size cap, as set when the recording started.
    quality: VideoQuality,
    /// The configured video folder.
    dir: PathBuf,
    /// The endpoint each source starts on, `[mic, system]`.
    devices: [Option<String>; 2],
    echo_cancel: bool,
    /// The cursor effects, as set when the recording started.
    effects: CursorEffects,
}

/// What the `screen-start` thread hands back.
struct Started {
    /// The video's final path; empty when no folder could be written.
    path: PathBuf,
    screen: Result<ScreenRecorder, StartError>,
    /// The mix recorder for the sound: `None` with both sources off (or no
    /// video to feed), an error when it could not start.
    audio: Option<Result<Recorder, RecError>>,
}

enum StartError {
    /// Windows refused the video folder and the fallback folder.
    FolderBlocked(PathBuf),
    Screen(Error),
}

/// Runs on `screen-start`: the folder, the name, the capture and the encoder,
/// then the sound.
fn start_recording(
    job: StartJob,
    on_end: Box<dyn Fn() + Send>,
    events: Box<dyn Fn(RecEvent) + Send>,
) -> Started {
    // Controlled folder access may guard `Videos`: then the profile folder's
    // `Vale\Video` takes the recording.
    let fallback = video_fallback_folder();
    let writable = |d: &Path| probe_writable(d).is_ok();
    let Some(dir) = choose_dir(&job.dir, fallback.as_deref(), writable) else {
        return Started {
            path: PathBuf::new(),
            screen: Err(StartError::FolderBlocked(job.dir)),
            audio: None,
        };
    };
    let stem = file_stem(Local::now().naive_local(), &job.title);
    let stem = unique_stem(&dir, &stem, |p| p.exists());
    let [path, ..] = files_of(&dir, &stem);
    let options = options_for(
        job.fps,
        job.sources,
        &job.title,
        job.effects,
        job.quality,
    );
    let recorder = match ScreenRecorder::start(job.target, &path, options, Some(on_end)) {
        Ok(recorder) => recorder,
        Err(e) => {
            return Started {
                path,
                screen: Err(StartError::Screen(e)),
                audio: None,
            };
        }
    };
    let audio = recorder.audio_input().map(|input| {
        Recorder::start_mix(
            MixOptions {
                sources: Sources {
                    mic: job.sources.mic,
                    system: job.sources.system,
                },
                devices: job.devices,
                echo_cancel: job.echo_cancel,
            },
            Box::new(Forward(input)),
            events,
        )
    });
    Started {
        path,
        screen: Ok(recorder),
        audio,
    }
}

/// Says `note` when it drops: the word of a start or stop thread reaches the
/// island also when the thread panics, so the phase never stays stuck.
struct SayAtEnd<F: Fn(ScreenNote)>(F, ScreenNote);

impl<F: Fn(ScreenNote)> Drop for SayAtEnd<F> {
    fn drop(&mut self) {
        (self.0)(self.1.clone());
    }
}

/// What the `screen-stop` thread hands back.
struct Stopped {
    result: Result<Summary, Error>,
    /// A picture of the saved video.
    thumb: Option<Thumb>,
    /// The user threw the recording away: its files are in the Recycle Bin.
    discard: bool,
}

/// Runs on `screen-stop`: ends the sound, then the video, and either reads
/// the thumbnail of what was saved or sends the files to the Recycle Bin
/// (`discard`, or a recording without a single frame).
fn stop_recording(
    recorder: ScreenRecorder,
    audio: Option<Recorder>,
    path: &Path,
    discard: bool,
) -> Stopped {
    // The mix first: its stop pushes the rest of its timeline to the video.
    if let Some(audio) = audio {
        let _ = audio.stop();
    }
    let result = recorder.stop();
    let empty = matches!(&result, Ok(s) if s.frames == 0);
    let thumb = match &result {
        Ok(s) if !discard && !empty => grab(&s.path, thumb_at(s.duration_ms)),
        _ => None,
    };
    if discard || empty {
        recycle_files(path);
    }
    Stopped {
        result,
        thumb,
        discard,
    }
}

/// Sends what exists of `video`'s files to the Recycle Bin.
fn recycle_files(video: &Path) {
    let files = existing_files(video);
    if files.is_empty() {
        return;
    }
    // The shell's file operation wants COM on this thread.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let left = crate::clean::recycle(&files);
    if com {
        unsafe { CoUninitialize() };
    }
    if left > 0 {
        eprintln!("vale: screen recording: {left} file(s) could not go to the Recycle Bin");
    }
}

// ---- the App side --------------------------------------------------------------

impl App {
    /// The screen hotkey, the tray entry and the Layar tab's button: opens
    /// the picker, cancels the countdown or stops the recording. While an
    /// audio recording runs, only a notice says so.
    pub(super) fn screen_toggle(&mut self) {
        match toggle_of(self.screen.phase) {
            ScreenToggle::Pick => {
                if busy_with_other(RecKind::Screen, self.rec.engaged(), self.screen.busy()) {
                    let lang = self.config.language;
                    self.apply_now(Event::Notice(busy_notice(lang)));
                    return;
                }
                let sources = self.config.recording.sources;
                self.queue_shortcut(shortcuts::Action::ScreenPick {
                    owner: self.hwnd.0 as isize,
                    mic: sources.mic,
                    system: sources.system,
                });
                self.screen_set_phase(ScreenPhase::Picking);
            }
            ScreenToggle::CancelCountdown => self.screen_cancel_countdown(),
            ScreenToggle::Stop => self.screen_stop(false),
            ScreenToggle::Nothing => {}
        }
    }

    /// The phase changed: the transcript queue's hold follows it.
    fn screen_set_phase(&mut self, phase: ScreenPhase) {
        self.screen.phase = phase;
        self.transcribe_sync_hold();
    }

    /// Back to idle: nothing of a recording is left. Also lets go of the
    /// excluder, so only once the recording thread has ended.
    fn screen_idle(&mut self) {
        self.screen.recorder = None;
        self.screen.audio = None;
        self.screen.watch = None;
        self.screen.wheel = None;
        self.screen.excluder = None;
        self.screen.picked = None;
        self.screen.path = None;
        self.screen.zoom = ZoomState::default();
        // What the sound said before a start that failed is dropped here.
        self.screen.early_audio.clear();
        self.screen_set_phase(ScreenPhase::Idle);
    }

    /// Posts `note` of `session` to the island from any thread.
    fn screen_poster(&self, session: u64) -> impl Fn(ScreenNote) + Send + 'static {
        let (tx, wake) = (self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        move |note| {
            let _ = tx.send(Event::Screen(session, note));
            wake();
        }
    }

    /// The picker's answer (`WM_APP_SCREEN_PICK`), or the picker never came
    /// up: `None` goes back to idle, a pick starts the 3-2-1.
    pub(super) fn screen_pick_done(&mut self, picked: Option<Picked>) {
        if self.screen.phase != ScreenPhase::Picking {
            return;
        }
        let Some(picked) = picked else {
            self.screen_set_phase(ScreenPhase::Idle);
            return;
        };
        self.screen.sources = RecSources {
            mic: picked.mic,
            system: picked.system,
        };
        self.screen.picked = Some(picked);
        // Another app may hold a bare Esc: the screen hotkey still cancels.
        self.screen.esc = self.register_countdown_esc();
        self.screen_set_phase(ScreenPhase::Countdown(COUNTDOWN_FROM));
        self.apply_now(Event::ScreenCountdown(Some(COUNTDOWN_FROM)));
        self.screen_arm_countdown();
    }

    /// One-shot to the next number (killed when it fires).
    fn screen_arm_countdown(&self) {
        unsafe {
            SetTimer(
                Some(self.hwnd),
                TIMER_SCREEN_COUNTDOWN,
                COUNTDOWN_STEP_MS,
                None,
            );
        }
    }

    /// `TIMER_SCREEN_COUNTDOWN`: 3 to 2 to 1, then the recording starts.
    pub(super) fn screen_countdown_tick(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SCREEN_COUNTDOWN);
        }
        let ScreenPhase::Countdown(n) = self.screen.phase else {
            return;
        };
        match next_count(n) {
            Some(next) => {
                self.screen.phase = ScreenPhase::Countdown(next);
                self.apply_now(Event::ScreenCountdown(Some(next)));
                self.screen_arm_countdown();
            }
            None => self.screen_start(),
        }
    }

    /// The countdown leaves the screen: its timer and its Esc go.
    fn screen_end_countdown(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SCREEN_COUNTDOWN);
        }
        if std::mem::take(&mut self.screen.esc) {
            self.unregister_countdown_esc();
        }
        self.apply_now(Event::ScreenCountdown(None));
    }

    /// Esc or the screen hotkey during the 3-2-1: nothing is recorded and
    /// nothing is said. An Esc that arrives after the countdown does nothing.
    pub(super) fn screen_cancel_countdown(&mut self) {
        if !matches!(self.screen.phase, ScreenPhase::Countdown(_)) {
            return;
        }
        self.screen_end_countdown();
        self.screen_idle();
    }

    /// After the 1: Vale's windows leave the capture, then the `screen-start`
    /// thread opens the capture, the encoder and the sound. The REC view
    /// shows once it reports (`screen_started`).
    fn screen_start(&mut self) {
        self.screen_end_countdown();
        let Some(picked) = self.screen.picked.clone() else {
            self.screen_idle();
            return;
        };
        self.screen.session += 1;
        let session = self.screen.session;
        self.screen.ended = false;
        self.screen.sound = false;
        self.screen.early_audio.clear();
        self.screen.told_lost = [false; 2];
        self.screen.pause = ScreenPause::default();
        // On this thread, before the capture exists. A hook Windows refused
        // leaves windows opened later in the video; the recording goes on.
        let excluder = Excluder::start();
        if !excluder.hooked() {
            eprintln!("vale: screen recording: windows Vale opens from now on may be in the video");
        }
        self.screen.excluder = Some(excluder);
        let recording = &self.config.recording;
        let job = StartJob {
            target: target_of(&picked.target),
            title: picked.title,
            sources: self.screen.sources,
            fps: recording.video_fps,
            quality: recording.video_quality,
            dir: recording.video_dir(),
            devices: [
                recording.mic_device.clone(),
                recording.system_device.clone(),
            ],
            echo_cancel: recording.echo_cancel,
            effects: CursorEffects {
                highlight: recording.cursor_highlight,
                ripples: recording.click_ripples,
            },
        };
        let say = self.screen_poster(session);
        let on_end: Box<dyn Fn() + Send> = Box::new({
            let say = self.screen_poster(session);
            move || say(ScreenNote::Ended)
        });
        let events: Box<dyn Fn(RecEvent) + Send> = Box::new({
            let say = self.screen_poster(session);
            move |event| say(ScreenNote::Audio(event))
        });
        let timing = std::env::var_os("DI_REC_TIMING").is_some();
        let spawned = std::thread::Builder::new()
            .name("screen-start".into())
            .spawn(move || {
                let _told = SayAtEnd(say, ScreenNote::Started);
                let t0 = Instant::now();
                let started = start_recording(job, on_end, events);
                if timing {
                    eprintln!("vale: screen recorder start took {:?}", t0.elapsed());
                }
                started
            });
        match spawned {
            Ok(handle) => {
                self.screen.starting = Some(handle);
                self.screen_set_phase(ScreenPhase::Starting);
            }
            Err(e) => {
                self.screen_idle();
                let lang = self.config.language;
                let n = screen_failure_notice(lang, &Error::Io(e.to_string()), None);
                self.apply_now(Event::Notice(n));
            }
        }
    }

    /// The start thread is done: the recording runs, or the notice says why
    /// not. A sound that could not start leaves the video going, silent.
    fn screen_started(&mut self) {
        let Some(handle) = self.screen.starting.take() else {
            return;
        };
        let lang = self.config.language;
        let started = handle.join().unwrap_or_else(|_| Started {
            path: PathBuf::new(),
            screen: Err(StartError::Screen(Error::Io(
                "the start thread panicked".into(),
            ))),
            audio: None,
        });
        let recorder = match started.screen {
            Ok(recorder) => recorder,
            Err(e) => {
                self.screen_idle();
                let n = match e {
                    StartError::FolderBlocked(dir) => folder_blocked_notice(lang, dir, true),
                    StartError::Screen(e) => {
                        eprintln!("vale: screen recording did not start: {e}");
                        let part = part_of(&started.path);
                        screen_failure_notice(lang, &e, part.is_file().then_some(part))
                    }
                };
                self.apply_now(Event::Notice(n));
                return;
            }
        };
        let sound_failed = match started.audio {
            Some(Ok(audio)) => {
                self.screen.audio = Some(audio);
                self.screen.sound = true;
                None
            }
            Some(Err(e)) => {
                eprintln!("vale: screen recording has no sound: {e}");
                self.screen.sources = RecSources {
                    mic: false,
                    system: false,
                };
                Some(if e.is_mic_denied() {
                    mic_denied_notice(lang)
                } else {
                    no_sound_notice(lang, &e.failure())
                })
            }
            None => None,
        };
        let used = started.path.parent().map(Path::to_path_buf);
        self.screen.recorder = Some(recorder);
        self.screen.path = Some(started.path);
        self.screen.clock = ScreenClock::new(Instant::now());
        self.screen.zoom = ZoomState::default();
        self.screen_set_phase(ScreenPhase::Recording);
        if let Some(n) = sound_failed {
            self.apply_now(Event::Notice(n));
        }
        // The window closed (or the display went) while it was starting:
        // what was recorded is saved.
        if self.screen.ended {
            self.screen_stop(false);
            return;
        }
        tray::set_screen_recording(self.hwnd, true);
        tray::set_mic_muted(false);
        self.screen_show_status();
        // The Rekam page names the devices the sound is taken from.
        self.rec_read_devices();
        // The mute hotkey is registered while a recording runs.
        self.sync_hotkeys();
        let target = self.screen.picked.as_ref().map(|p| p.target.clone());
        if let Some(PickTarget::Window(target)) = target {
            self.screen.watch = WindowWatch::start(target, self.hwnd, WM_APP_SCREEN_WATCH);
            // The watch tells changes only: one minimised since the start
            // is asked about here.
            if unsafe { IsIconic(HWND(target as *mut c_void)) }.as_bool() {
                self.screen_on_window(true);
            }
        }
        // Locked during the countdown: the lock screen is not recorded.
        if self.session_locked() {
            self.screen_on_away(true);
        }
        if let Some(used) = used {
            self.screen_fell_back(&used);
        }
        // What the sound said while the start was still running (a mic
        // Windows refused, a device that fell back): told now, once.
        for event in std::mem::take(&mut self.screen.early_audio) {
            self.on_screen_audio(event);
        }
    }

    /// The video went to `used`, not to the configured folder (Windows
    /// refused that one). The default folder moves there for good; a folder
    /// the user picked stays theirs. Either way a notice says where it goes.
    fn screen_fell_back(&mut self, used: &Path) {
        if used == self.config.recording.video_dir() {
            return;
        }
        if self.config.recording.video_folder.is_none() {
            self.config.recording.video_folder = Some(used.to_path_buf());
            self.save_config();
            self.sync_settings();
        }
        let lang = self.config.language;
        self.apply_now(Event::Notice(fallback_notice(lang, used)));
    }

    /// Ends the recording now on screen and finalizes it on the
    /// `screen-stop` thread, which reports `Stopped`. With `discard` the
    /// files go to the Recycle Bin instead.
    fn screen_stop(&mut self, discard: bool) {
        if self.screen.phase != ScreenPhase::Recording {
            return;
        }
        let (Some(recorder), Some(path)) = (self.screen.recorder.take(), self.screen.path.clone())
        else {
            self.screen_idle();
            return;
        };
        let audio = self.screen.audio.take();
        // Nothing is left to pause or to zoom.
        self.screen.watch = None;
        self.screen.wheel = None;
        tray::set_screen_recording(self.hwnd, false);
        tray::set_mic_muted(false);
        self.apply_now(Event::RecStatus(None));
        self.screen.phase = ScreenPhase::Stopping;
        self.sync_hotkeys();
        let say = self.screen_poster(self.screen.session);
        let timing = std::env::var_os("DI_REC_TIMING").is_some();
        // A spawn that fails drops the closure, and the recorders' `Drop`
        // finishes the file here instead.
        let spawned = std::thread::Builder::new()
            .name("screen-stop".into())
            .spawn(move || {
                let _told = SayAtEnd(say, ScreenNote::Stopped);
                let t0 = Instant::now();
                let stopped = stop_recording(recorder, audio, &path, discard);
                if timing {
                    eprintln!("vale: screen recorder stop took {:?}", t0.elapsed());
                }
                stopped
            });
        match spawned {
            Ok(handle) => {
                self.screen.stopping = Some(handle);
                // Still engaged: the queue keeps holding until `Stopped`.
                self.transcribe_sync_hold();
            }
            // No `Stopped` comes: the recording is over now.
            Err(_) => self.screen_idle(),
        }
    }

    /// The stop thread is done: Vale's windows are capturable again, and the
    /// notice says what became of the video.
    fn screen_stopped(&mut self) {
        let Some(handle) = self.screen.stopping.take() else {
            return;
        };
        let stopped = handle.join().ok();
        let window = self.screen.records_window();
        let had_sound = self.screen.sound;
        let path = self.screen.path.clone();
        self.screen_idle();
        // Back to the idle choices on the Rekam page.
        self.rec_read_devices();
        let lang = self.config.language;
        let unfinished = || {
            let part = path.as_deref().map(part_of)?;
            part.is_file().then_some(part)
        };
        let notice = match stopped {
            None => {
                let e = Error::Io("the stop thread panicked".into());
                Some(screen_failure_notice(lang, &e, unfinished()))
            }
            Some(Stopped { discard: true, .. }) => Some(discarded_notice(lang)),
            // Not one frame: the files are gone, and nothing is said.
            Some(Stopped {
                result: Ok(summary),
                ..
            }) if summary.frames == 0 => None,
            Some(Stopped {
                result: Ok(summary),
                thumb,
                ..
            }) => {
                // A video without sound has nothing to transcribe.
                let offer = if had_sound {
                    self.transcribe_saved(&summary.path)
                } else {
                    TranscriptOffer::None
                };
                // A new id each time: the picture's cache goes by the id.
                self.screen.next_thumb += 1;
                let id = self.screen.next_thumb;
                self.screen.thumb = thumb.map(|t| (id, t));
                Some(video_saved_notice(
                    lang,
                    summary.path,
                    Duration::from_millis(summary.duration_ms),
                    ended_of(summary.ended, window),
                    offer,
                    self.screen.thumb.as_ref().map(|(id, _)| *id),
                ))
            }
            Some(Stopped { result: Err(e), .. }) => {
                eprintln!("vale: screen recording failed: {e}");
                Some(screen_failure_notice(lang, &e, unfinished()))
            }
        };
        if let Some(n) = notice {
            self.apply_now(Event::Notice(n));
        }
        if self.screen.page_shown {
            self.screen_rescan();
        }
    }

    /// The status the island shows, from the screen recording's clock.
    fn screen_show_status(&mut self) {
        let muted = self
            .screen
            .audio
            .as_ref()
            .map_or([false; 2], Recorder::muted);
        let status = status_of(
            &self.screen.clock,
            self.screen.sources,
            muted,
            self.screen.zoom.shown(),
        );
        self.apply_now(Event::RecStatus(Some(status)));
    }

    /// The zoom hotkey while a screen recording runs: the video zooms in to
    /// the cursor, or back out.
    pub(super) fn screen_toggle_zoom(&mut self) {
        if !self.screen.recording() {
            return;
        }
        let Some(recorder) = self.screen.recorder.as_ref() else {
            return;
        };
        recorder.toggle_zoom();
        self.screen.zoom.toggle();
        if self.screen.zoom.on {
            // A `None` only loses the wheel; the zoom still works.
            self.screen.wheel = WheelHook::start(self.hwnd);
        } else if let Some(wheel) = self.screen.wheel.take() {
            wheel.stop();
        }
        self.screen_show_status();
    }

    /// A screen recording runs and its video is zoomed.
    pub(super) fn screen_zoomed(&self) -> bool {
        self.screen.recording() && self.screen.zoom.on
    }

    /// One level up (`dir > 0`) or down, only while the video is zoomed.
    pub(super) fn screen_zoom_step(&mut self, dir: i32) {
        if !self.screen.recording() || !self.screen.zoom.on {
            return;
        }
        let Some(recorder) = self.screen.recorder.as_ref() else {
            return;
        };
        if self.screen.zoom.step(dir) {
            recorder.zoom_step(dir);
            self.screen_show_status();
        }
    }

    /// The recording pauses or goes on, and the clock with it.
    fn screen_apply_pause(&mut self, paused: bool) {
        let Some(recorder) = self.screen.recorder.as_ref() else {
            return;
        };
        let now = Instant::now();
        if paused {
            recorder.pause();
            self.screen.clock.pause(now);
        } else {
            recorder.resume();
            self.screen.clock.resume(now);
        }
        self.screen_show_status();
    }

    /// The pause button: pauses, or resumes whatever paused it.
    pub(super) fn screen_pause(&mut self) {
        if !self.screen.recording() {
            return;
        }
        let paused = self.screen.pause.user_toggle();
        self.screen_apply_pause(paused);
    }

    /// The PC sleeps or is locked (`on`), or is back: the recording pauses
    /// meanwhile, and goes on unless something else holds it paused.
    pub(super) fn screen_on_away(&mut self, on: bool) {
        if !self.screen.recording() {
            return;
        }
        if let Some(paused) = self.screen.pause.set(PauseWhy::Away, on) {
            self.screen_apply_pause(paused);
        }
    }

    /// The recorded window was minimised or restored (`WM_APP_SCREEN_WATCH`).
    pub(super) fn screen_on_window(&mut self, minimised: bool) {
        if !self.screen.recording() || !self.screen.records_window() {
            return;
        }
        if let Some(paused) = self.screen.pause.set(PauseWhy::Minimised, minimised) {
            self.screen_apply_pause(paused);
        }
    }

    /// A word from a screen recording's threads, from the event queue. One
    /// of an older session than the newest is ignored.
    pub(super) fn on_screen_note(&mut self, session: u64, note: ScreenNote) {
        if session != self.screen.session {
            return;
        }
        match note {
            ScreenNote::Started => self.screen_started(),
            ScreenNote::Stopped => self.screen_stopped(),
            ScreenNote::Ended => match self.screen.phase {
                ScreenPhase::Recording => self.screen_stop(false),
                // The start thread has not reported yet.
                ScreenPhase::Starting => self.screen.ended = true,
                _ => {}
            },
            ScreenNote::Audio(event) => match audio_word_in(self.screen.phase) {
                AudioWord::Tell => self.on_screen_audio(event),
                AudioWord::Keep => self.screen.early_audio.push(event),
                AudioWord::Drop => {}
            },
        }
    }

    /// The mix recorder's word while the video records: the video goes on
    /// whatever happens to the sound.
    fn on_screen_audio(&mut self, event: RecEvent) {
        if !self.screen.recording() {
            return;
        }
        let lang = self.config.language;
        let notice = match event {
            RecEvent::SourceLost(source) => {
                self.screen.told_lost[source.index()] = true;
                source_lost_notice(lang, source)
            }
            RecEvent::MicDenied => {
                self.screen.told_lost[Source::Mic.index()] = true;
                mic_denied_notice(lang)
            }
            RecEvent::DeviceSwitched {
                source,
                name,
                fell_back,
                manual,
            } => {
                // The Rekam page names the device in use now.
                self.rec_read_devices();
                if manual {
                    return;
                }
                let chosen = self
                    .config
                    .recording
                    .device(source)
                    .and_then(devices::name_of);
                device_switched_notice(lang, source, &name, fell_back, chosen.as_deref())
            }
            RecEvent::Failed(failure) => {
                eprintln!("vale: screen recording lost its sound: {failure:?}");
                no_sound_notice(lang, &failure)
            }
            // Every recorded source is lost. The last one comes without a
            // `SourceLost` of its own: this is its only word. No meters
            // from here, as when the sound could not start.
            RecEvent::NoInput => {
                let recorded = self.screen.sources;
                self.screen.sources = RecSources {
                    mic: false,
                    system: false,
                };
                tray::set_mic_muted(false);
                self.screen_show_status();
                match no_input_notice(lang, recorded, self.screen.told_lost) {
                    Some(notice) => notice,
                    None => return,
                }
            }
            // A mix writes no file.
            RecEvent::Saved { .. } => return,
        };
        self.apply_now(Event::Notice(notice));
    }

    /// The mute buttons, the mute hotkey and the tray entry while a screen
    /// recording runs. Ignored when `source` is not one of the recorded.
    pub(super) fn screen_toggle_mute(&mut self, source: Source) {
        if !self.screen.recording() || !is_recorded(self.screen.sources, source) {
            return;
        }
        let Some(audio) = self.screen.audio.as_ref() else {
            return;
        };
        let now = !audio.muted()[source.index()];
        audio.set_muted(source, now);
        tray::set_mic_muted(audio.muted()[0]);
        self.screen_show_status();
    }

    /// A device picked on the page or in Settings while a screen recording
    /// runs: its sound switches to it at once, as an audio recording does.
    pub(super) fn screen_set_device(&self, source: Source, id: Option<String>) {
        if let Some(audio) = self.screen.audio.as_ref() {
            audio.set_device(source, id);
        }
    }

    /// The levels `(mic, system)` of the screen recording's sound.
    pub(super) fn screen_levels(&self) -> (f32, f32) {
        self.screen
            .audio
            .as_ref()
            .map_or((0.0, 0.0), Recorder::levels)
    }

    /// "Buang", confirmed: stops the screen recording and sends its files to
    /// the Recycle Bin.
    pub(super) fn screen_discard(&mut self) {
        self.screen_stop(true);
    }

    /// Reads the video folder for the Layar tab on the `rec-scan` thread;
    /// the list comes back as `Event::VideoRecent`.
    pub(super) fn screen_rescan(&mut self) {
        let dir = self.config.recording.video_dir();
        let lang = self.config.language;
        let skip = self.screen.path.as_deref().map(part_of);
        let (tx, wake) = (self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        let _ = std::thread::Builder::new()
            .name("rec-scan".into())
            .spawn(move || {
                let today = Local::now().date_naive();
                let list = scan_recent(
                    &dir,
                    RecKind::Screen,
                    lang,
                    today,
                    RECENT_LIMIT,
                    skip.as_deref(),
                    &decode::probe_duration_ms,
                );
                let _ = tx.send(Event::VideoRecent(list.entries, list.total));
                wake();
            });
    }

    /// Called from `refresh`: a new showing of the Rekam page re-reads the
    /// video folder (once per showing, not per frame). `under`: the open
    /// panel's page, also while a notice covers it.
    pub(super) fn screen_follow_page(&mut self, under: Option<Page>) {
        let shown = under == Some(Page::Record);
        if shown && !self.screen.page_shown {
            self.screen_rescan();
        }
        self.screen.page_shown = shown;
    }

    /// Explorer on the video folder (made first when it is missing).
    pub(super) fn screen_open_folder(&mut self) {
        let dir = self.config.recording.video_dir();
        let _ = std::fs::create_dir_all(&dir);
        notif_open::allow_foreground();
        let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
    }

    /// The blocked-folder notice's click: the folder picker, whose choice
    /// comes back through `screen_set_folder`.
    pub(super) fn screen_pick_folder(&mut self) {
        notif_open::allow_foreground();
        settings::pick_video_folder(self.hwnd);
    }

    /// The picker's (or Settings') video folder. Refused while a screen
    /// recording is anywhere between its picker and its saved file; a folder
    /// Windows will not let Vale write in is not saved, and the blocked
    /// notice shows again.
    pub(super) fn screen_set_folder(&mut self, dir: Option<PathBuf>) {
        if self.screen.busy() {
            self.sync_settings();
            return;
        }
        if let Some(d) = &dir
            && let Err(e) = probe_writable(d)
        {
            eprintln!("vale: video folder {} refused: {e}", d.display());
            let lang = self.config.language;
            self.apply_now(Event::Notice(folder_blocked_notice(lang, d.clone(), true)));
            self.sync_settings();
            return;
        }
        self.config.recording.video_folder = dir;
        self.save_config();
        self.sync_settings();
        if self.screen.page_shown {
            self.screen_rescan();
        }
    }

    /// Settings' frames per second: 60, anything else is 30. Refused while a
    /// screen recording is busy.
    pub(super) fn screen_set_fps(&mut self, fps: u32) {
        let fps = if fps == 60 { 60 } else { 30 };
        if !self.screen.busy() && self.config.recording.video_fps != fps {
            self.config.recording.video_fps = fps;
            self.save_config();
        }
        self.sync_settings();
    }

    /// Settings' video quality. Refused while a screen recording is busy; a
    /// recording keeps the quality it started with.
    pub(super) fn screen_set_quality(&mut self, q: VideoQuality) {
        if !self.screen.busy() && self.config.recording.video_quality != q {
            self.config.recording.video_quality = q;
            self.save_config();
        }
        self.sync_settings();
    }

    /// Vale is closing (or the session ends): the countdown is dropped, a
    /// start or a stop in flight is waited for, and a running recording is
    /// finalized, so the video is closed and its sidecar written. No notice.
    pub(super) fn screen_shutdown(&mut self) {
        if matches!(self.screen.phase, ScreenPhase::Countdown(_)) {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_SCREEN_COUNTDOWN);
            }
            if std::mem::take(&mut self.screen.esc) {
                self.unregister_countdown_esc();
            }
        }
        if let Some(handle) = self.screen.starting.take()
            && let Ok(started) = handle.join()
        {
            // The sound first, as in `stop_recording`.
            if let Some(Ok(audio)) = started.audio {
                let _ = audio.stop();
            }
            if let Ok(recorder) = started.screen {
                let _ = recorder.stop();
            }
        }
        if let Some(handle) = self.screen.stopping.take() {
            let _ = handle.join();
        }
        if let Some(audio) = self.screen.audio.take() {
            let _ = audio.stop();
        }
        if let Some(recorder) = self.screen.recorder.take() {
            let _ = recorder.stop();
        }
        // The recording thread has ended: Vale's windows are capturable again.
        self.screen.watch = None;
        self.screen.wheel = None;
        self.screen.excluder = None;
        self.screen.picked = None;
        self.screen.path = None;
        self.screen.phase = ScreenPhase::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::Rect;
    use crate::i18n::rec_duration;

    #[test]
    fn the_hold_follows_the_screen_phase() {
        use ScreenPhase::*;
        // (phase, busy, engaged, recording)
        let table = [
            (Idle, false, false, false),
            (Picking, true, false, false),
            (Countdown(3), true, true, false),
            (Starting, true, true, false),
            (Recording, true, true, true),
            (Stopping, true, true, false),
        ];
        for (phase, busy, engaged, recording) in table {
            assert_eq!(phase.busy(), busy, "{phase:?} busy");
            assert_eq!(phase.engaged(), engaged, "{phase:?} engaged");
            assert_eq!(phase.recording(), recording, "{phase:?} recording");
            let state = ScreenState {
                phase,
                ..ScreenState::default()
            };
            assert_eq!(
                (state.busy(), state.engaged(), state.recording()),
                (busy, engaged, recording),
                "{phase:?} through the state"
            );
        }
    }

    #[test]
    fn the_toggle_does_what_the_phase_says() {
        use ScreenPhase::*;
        assert_eq!(toggle_of(Idle), ScreenToggle::Pick);
        // The picker is up: a second one would lose the first one's answer.
        assert_eq!(toggle_of(Picking), ScreenToggle::Nothing);
        for n in [3, 2, 1] {
            assert_eq!(
                toggle_of(Countdown(n)),
                ScreenToggle::CancelCountdown,
                "{n}"
            );
        }
        // A start or a stop in flight: nothing doubles.
        assert_eq!(toggle_of(Starting), ScreenToggle::Nothing);
        assert_eq!(toggle_of(Recording), ScreenToggle::Stop);
        assert_eq!(toggle_of(Stopping), ScreenToggle::Nothing);
    }

    #[test]
    fn the_countdown_counts_three_two_one() {
        assert_eq!(COUNTDOWN_FROM, 3);
        assert_eq!(COUNTDOWN_STEP_MS, 1_000);
        assert_eq!(next_count(3), Some(2));
        assert_eq!(next_count(2), Some(1));
        assert_eq!(next_count(1), None, "the recording starts after the 1");
        assert_eq!(next_count(0), None);
    }

    #[test]
    fn another_recording_blocks_the_start() {
        assert!(busy_with_other(RecKind::Screen, true, false));
        assert!(busy_with_other(RecKind::Audio, false, true));
        // Its own kind is the toggle's business (a stop, or nothing).
        assert!(!busy_with_other(RecKind::Screen, false, true));
        assert!(!busy_with_other(RecKind::Audio, true, false));
        assert!(!busy_with_other(RecKind::Screen, false, false));
        let id = busy_notice(Lang::Id);
        assert_eq!(id.title, "Rekaman lain sedang jalan");
        assert_eq!(id.body, "Hentikan dulu, lalu mulai lagi");
        assert_eq!(id.kind, NoticeKind::RecInfo);
        let en = busy_notice(Lang::En);
        assert_eq!(en.title, "Another recording is running");
        assert_eq!(en.body, "Stop it first, then start again");
    }

    #[test]
    fn the_saved_notice_names_the_length_and_why_it_ended() {
        let path = PathBuf::from(r"C:\v\a.mp4");
        let length = Duration::from_secs(197);
        let at = |lang, ended, offer| {
            video_saved_notice(lang, path.clone(), length, ended, offer, Some(7))
        };
        let id = at(Lang::Id, Ended::Stopped, TranscriptOffer::None);
        assert_eq!(id.title, "Video tersimpan");
        assert_eq!(
            id.body,
            format!("{} \u{b7} klik untuk buka", rec_duration(Lang::Id, length))
        );
        assert_eq!(
            id.kind,
            NoticeKind::RecSaved {
                path: path.clone(),
                offer: TranscriptOffer::None,
                thumb: Some(7),
            }
        );
        assert_eq!(
            at(Lang::Id, Ended::WindowClosed, TranscriptOffer::None).title,
            "Jendela ditutup, video tersimpan"
        );
        assert_eq!(
            at(Lang::Id, Ended::DisplayGone, TranscriptOffer::None).title,
            "Layar terputus, video tersimpan"
        );
        let en = at(Lang::En, Ended::Stopped, TranscriptOffer::Transcribe);
        assert_eq!(en.title, "Video saved");
        assert_eq!(
            en.body,
            format!("{} \u{b7} click to open", rec_duration(Lang::En, length))
        );
        assert_eq!(
            at(Lang::En, Ended::WindowClosed, TranscriptOffer::None).title,
            "Window closed, video saved"
        );
        assert_eq!(
            at(Lang::En, Ended::DisplayGone, TranscriptOffer::None).title,
            "Display disconnected, video saved"
        );
        // With the setup offer the body asks, as the audio notice does.
        let setup = at(Lang::Id, Ended::Stopped, TranscriptOffer::Setup);
        assert!(setup.body.ends_with("Bikin transkrip?"), "{}", setup.body);
        let setup = at(Lang::En, Ended::Stopped, TranscriptOffer::Setup);
        assert!(setup.body.ends_with("Want a transcript?"), "{}", setup.body);
        // No picture could be read: the notice names none.
        let bare = video_saved_notice(
            Lang::Id,
            path.clone(),
            length,
            Ended::Stopped,
            TranscriptOffer::None,
            None,
        );
        assert!(matches!(
            bare.kind,
            NoticeKind::RecSaved { thumb: None, .. }
        ));
        assert_eq!(discarded_notice(Lang::Id).title, "Video dibuang");
        assert_eq!(discarded_notice(Lang::En).title, "Video discarded");
        assert_eq!(discarded_notice(Lang::Id).kind, NoticeKind::RecInfo);
    }

    #[test]
    fn a_video_without_sound_says_so_and_why() {
        let cases = [
            (
                Failure::NoDevice,
                "Mic atau speaker tidak ditemukan",
                "No microphone or speaker found",
            ),
            (
                Failure::NoMediaFoundation,
                "Media Foundation tidak ada di Windows ini",
                "Media Foundation is missing on this Windows",
            ),
            (
                Failure::NoAudio,
                "Tidak ada suara masuk",
                "No audio came in",
            ),
            (
                Failure::Other("mix thread".into()),
                "Ada yang tidak beres saat merekam",
                "Something went wrong while recording",
            ),
            // Never a folder notice: a mix writes no file.
            (
                Failure::FolderBlocked,
                "Ada yang tidak beres saat merekam",
                "Something went wrong while recording",
            ),
        ];
        for (failure, id, en) in cases {
            let n = no_sound_notice(Lang::Id, &failure);
            assert_eq!(n.title, "Video jalan tanpa suara", "{failure:?}");
            assert_eq!(n.body, id, "{failure:?}");
            // The video is still recording: a word, not a failure.
            assert_eq!(n.kind, NoticeKind::RecInfo, "{failure:?}");
            let n = no_sound_notice(Lang::En, &failure);
            assert_eq!(n.title, "Recording video without sound", "{failure:?}");
            assert_eq!(n.body, en, "{failure:?}");
        }
    }

    #[test]
    fn a_word_of_the_sound_waits_for_the_start_to_report() {
        use ScreenPhase::*;
        // The mix speaks right after its sources open, often before the
        // start thread's `Started` is read: nothing it says may be lost.
        assert_eq!(audio_word_in(Starting), AudioWord::Keep);
        assert_eq!(audio_word_in(Recording), AudioWord::Tell);
        for phase in [Idle, Picking, Countdown(2), Stopping] {
            assert_eq!(audio_word_in(phase), AudioWord::Drop, "{phase:?}");
        }
    }

    #[test]
    fn the_last_source_lost_says_the_video_goes_on_without_sound() {
        let sources = |mic, system| RecSources { mic, system };
        // Only the mic, unplugged: `NoInput` is the only word that comes.
        let n = no_input_notice(Lang::Id, sources(true, false), [false, false]).expect("a notice");
        assert_eq!(n.title, "Video jalan tanpa suara");
        assert_eq!(n.body, "Mic atau speaker tidak ditemukan");
        assert_eq!(n.kind, NoticeKind::RecInfo);
        assert_eq!(n, no_sound_notice(Lang::Id, &Failure::NoDevice));
        let n = no_input_notice(Lang::En, sources(false, true), [false, false]).expect("a notice");
        assert_eq!(n.title, "Recording video without sound");
        assert_eq!(n.body, "No microphone or speaker found");
        // Both recorded, the mic was told lost, now PC audio goes too.
        assert!(no_input_notice(Lang::Id, sources(true, true), [true, false]).is_some());
        assert!(no_input_notice(Lang::Id, sources(true, true), [false, true]).is_some());
        // The last loss was the mic Windows refused: its own notice (a click
        // opens the privacy page) stays, nothing is said over it.
        assert_eq!(no_input_notice(Lang::Id, sources(true, false), [true, false]), None);
        assert_eq!(no_input_notice(Lang::Id, sources(true, true), [true, true]), None);
        // A source that was not recorded counts for nothing.
        assert!(no_input_notice(Lang::Id, sources(true, false), [false, true]).is_some());
    }

    #[test]
    fn esc_comes_back_only_during_a_countdown_that_had_it() {
        use ScreenPhase::*;
        let state = |phase, esc| ScreenState {
            phase,
            esc,
            ..ScreenState::default()
        };
        assert!(state(Countdown(3), true).wants_esc());
        assert!(state(Countdown(1), true).wants_esc());
        // Another app held Esc when the countdown began: it stays theirs.
        assert!(!state(Countdown(3), false).wants_esc());
        for phase in [Idle, Picking, Starting, Recording, Stopping] {
            assert!(!state(phase, true).wants_esc(), "{phase:?}");
        }
    }

    #[test]
    fn the_word_of_a_thread_is_said_even_when_it_panics() {
        use std::sync::{Arc, Mutex};
        let said = Arc::new(Mutex::new(Vec::new()));
        let say = {
            let said = Arc::clone(&said);
            move |note| said.lock().unwrap().push(note)
        };
        // Quiet: the panic below is the test's own.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let joined = std::thread::spawn(move || {
            let _told = SayAtEnd(say, ScreenNote::Stopped);
            if std::hint::black_box(true) {
                panic!("the stop thread died");
            }
        })
        .join();
        std::panic::set_hook(hook);
        assert!(joined.is_err());
        assert_eq!(*said.lock().unwrap(), [ScreenNote::Stopped]);
        // And once, at the end, when nothing goes wrong.
        let said = Arc::new(Mutex::new(Vec::new()));
        {
            let sink = Arc::clone(&said);
            let _told = SayAtEnd(
                move |note| sink.lock().unwrap().push(note),
                ScreenNote::Started,
            );
            assert!(said.lock().unwrap().is_empty());
        }
        assert_eq!(*said.lock().unwrap(), [ScreenNote::Started]);
    }

    #[test]
    fn ended_of_maps_the_reason() {
        assert_eq!(ended_of(EndReason::TargetClosed, true), Ended::WindowClosed);
        assert_eq!(ended_of(EndReason::TargetClosed, false), Ended::DisplayGone);
        assert_eq!(ended_of(EndReason::Stopped, true), Ended::Stopped);
        assert_eq!(ended_of(EndReason::Stopped, false), Ended::Stopped);
    }

    #[test]
    fn every_screen_error_reads_in_both_languages() {
        let cases = [
            (Error::NoMediaFoundation, Key::RecWhyNoMf),
            (Error::CaptureUnsupported, Key::ScreenWhyUnsupported),
            (Error::TargetGone, Key::ScreenWhyGone),
            (Error::TargetMinimized, Key::ScreenWhyMinimized),
            (Error::NoEncoder("no MFT".into()), Key::ScreenWhyEncoder),
            (Error::Gpu("no device".into()), Key::ScreenWhyGpu),
            (
                Error::Windows {
                    step: "CreateForWindow",
                    code: -1,
                    message: "x".into(),
                },
                Key::ScreenWhyOther,
            ),
            (Error::Io("disk".into()), Key::ScreenWhyWrite),
        ];
        for (error, key) in cases {
            for (lang, title) in [
                (Lang::Id, "Rekam layar gagal"),
                (Lang::En, "Screen recording failed"),
            ] {
                let n = screen_failure_notice(lang, &error, None);
                assert_eq!(n.title, title, "{error:?}");
                assert_eq!(n.body, text(lang, key), "{error:?}");
                assert_eq!(n.kind, NoticeKind::RecFailed { reveal: None }, "{error:?}");
            }
        }
        assert_eq!(
            screen_failure_notice(Lang::Id, &Error::NoMediaFoundation, None).body,
            "Media Foundation tidak ada di Windows ini"
        );
        assert_eq!(
            screen_failure_notice(Lang::Id, &Error::TargetMinimized, None).body,
            "Jendelanya sedang diminimalkan. Buka dulu, lalu coba lagi"
        );
        // What was recorded is kept in the `.part`: a click shows it.
        let part = PathBuf::from(r"C:\v\a.mp4.part");
        assert_eq!(
            screen_failure_notice(Lang::Id, &Error::Io("x".into()), Some(part.clone())).kind,
            NoticeKind::RecFailed { reveal: Some(part) }
        );
    }

    #[test]
    fn the_target_of_a_pick() {
        assert_eq!(target_of(&PickTarget::Monitor(5)), Target::Monitor(5));
        assert_eq!(target_of(&PickTarget::Window(9)), Target::Window(9));
        assert_eq!(
            target_of(&PickTarget::Area(Rect::new(10, 20, 300, 200))),
            Target::Region {
                x: 10,
                y: 20,
                w: 300,
                h: 200
            }
        );
    }

    #[test]
    fn the_status_is_the_screen_clock_with_the_sources_and_the_mutes() {
        let t0 = Instant::now();
        let sources = RecSources {
            mic: true,
            system: false,
        };
        let mut clock = ScreenClock::new(t0);
        let running = status_of(&clock, sources, [true, false], None);
        assert_eq!(running.kind, RecKind::Screen);
        assert_eq!(running.started, t0);
        assert!(!running.paused());
        assert_eq!((running.sources, running.muted), (sources, [true, false]));
        assert_eq!(running.zoom, None);
        let zoomed = status_of(&clock, sources, [false, false], Some(2.0));
        assert_eq!(zoomed.zoom, Some(2.0));
        clock.pause(t0 + Duration::from_secs(10));
        let paused = status_of(&clock, sources, [false, false], None);
        assert_eq!(paused.paused_since, Some(t0 + Duration::from_secs(10)));
        assert_eq!(
            paused.elapsed(t0 + Duration::from_secs(60)),
            Duration::from_secs(10)
        );
        clock.resume(t0 + Duration::from_secs(30));
        let resumed = status_of(&clock, sources, [false, false], None);
        assert_eq!(
            resumed.elapsed(t0 + Duration::from_secs(60)),
            Duration::from_secs(40)
        );
    }
}
