//! Audio recording: the mic, the system audio (WASAPI loopback) or both to an
//! `.m4a` with a `.vale.json` sidecar. Design:
//! `docs/superpowers/specs/2026-11-07-recording-design.md`; plan:
//! `docs/superpowers/plans/2026-11-07-recording-phase1.md`.
//!
//! A [`Recorder`] is one recording. While it exists two threads run:
//!
//! - `rec-capture` ([`capture::Capture`]): delivers mono packets with their
//!   QPC time to a sink that resamples each source to 48 kHz on its own
//!   thread ([`Lanes`]), then, under a short lock, places the result on a
//!   shared [`timeline::Timeline`] by time ([`Placer`]) and feeds the peak
//!   meters.
//! - `rec-writer`: every 50 ms takes the timeline up to 200 ms before now
//!   (a second at a time) and encodes it into `<stem>.m4a.part`; on stop it
//!   takes the rest, finalizes, renames to `<stem>.m4a`, writes
//!   `<stem>.vale.json` and reports [`RecEvent::Saved`]. A tick that finds
//!   more than 2 s passed since the last one (the PC slept; QPC runs on
//!   through sleep) cuts that time out like a pause.
//!
//! [`Recorder::start_mix`] is the same capture, lanes, placer, mute and
//! meters with a `rec-mix` thread instead of `rec-writer`: it pushes the
//! mono [`mix`] to a [`mix::MixSink`] and writes no file (the screen
//! recorder's audio).
//!
//! Neither exists before [`Recorder::start`] or after [`Recorder::stop`]
//! returns, and no timer is created: the writer paces itself with a channel
//! receive timeout. When every recorded source is lost the capture thread
//! ends and the writer finishes by itself ([`RecEvent::NoInput`]).

pub mod capture;
pub mod devices;
pub mod encode;
pub mod level;
pub mod mix;
pub mod naming;
pub mod prefs;
pub mod resample;
pub mod sidecar;
pub mod timeline;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

pub use capture::{AudioSink, Capture, CaptureError, Source, Sources};
pub use encode::{Container, EncodeError};

use capture::{QPC_UNITS_PER_SEC, qpc_now};
use encode::M4aWriter;
use level::{PeakMeter, SharedLevels};
use resample::Resampler;
use sidecar::Sidecar;
use timeline::{Timeline, qpc_to_frame};

/// The file's sample rate.
pub const RATE: u32 = encode::RATE;
/// How often the writer takes from the timeline.
const TICK: Duration = Duration::from_millis(50);
/// The writer stays this far behind now, so late packets still land.
const LAG_FRAMES: i64 = RATE as i64 / 5; // 200 ms
/// The most the writer takes from the timeline at once (1 s).
const TAKE_FRAMES: usize = RATE as usize;
/// More than this (2 s) between two writer ticks: the PC slept, and that
/// time is cut out like a pause.
const SLEEP_GAP_FRAMES: i64 = 2 * RATE as i64;
/// A packet more than this (200 ms) away from where its source's stream is
/// expected starts a new run with a new resampler (a device restart).
const RESTART_FRAMES: i64 = RATE as i64 / 5;
/// A packet more than this (20 ms) but at most `RESTART_FRAMES` away moves
/// the stream to its stamp at once, keeping the resampler: a loopback
/// silence gap (zeros fill it) or a glitch.
const JUMP_FRAMES: i64 = RATE as i64 / 50;
/// Within `JUMP_FRAMES` the packets are laid end to end, so stamp jitter
/// never cuts the audio; when the smoothed stamp error passes this (1 ms),
/// one sample per packet is inserted or dropped until they agree again
/// (two clocks drifting apart).
const SLEW_FRAMES: f64 = RATE as f64 / 1000.0;
/// Packets the stamp error is smoothed over (an exponential average).
const SLEW_SMOOTHING: f64 = 16.0;
/// A recording holding less than this (0.5 s) is not kept: no file, no
/// notice. A longer one that no source delivered anything to is not kept
/// either, and says so (`RecError::Silent`).
const MIN_KEEP_FRAMES: u64 = RATE as u64 / 2;

/// What to record and where.
#[derive(Clone, Debug)]
pub struct RecOptions {
    pub sources: Sources,
    /// Created when missing.
    pub dir: PathBuf,
    /// Goes into the file name and the sidecar; may be empty.
    pub title: String,
    /// The app that was using the mic, for the sidecar.
    pub app: Option<String>,
    /// The endpoint each source starts on, `[mic, system]` (`None`: follow
    /// the Windows default).
    pub devices: [Option<String>; 2],
    /// Ask Windows to cancel the echo of the speakers on the mic stream.
    pub echo_cancel: bool,
}

/// A finished recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecResult {
    pub path: PathBuf,
    /// The audio in the file (pauses excluded).
    pub duration: Duration,
}

/// What a recording reports while it runs, from the writer thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecEvent {
    /// The file and its sidecar are written (also returned by `stop`).
    Saved { path: PathBuf, duration: Duration },
    /// The recording failed; a non-empty `.m4a.part` may be left (it plays).
    Failed(Failure),
    /// A source stopped for good while another one records; its channel
    /// stays silent from here.
    SourceLost(Source),
    /// The mic was refused by the Windows privacy setting; the mic channel
    /// stays silent.
    MicDenied,
    /// Every recorded source is lost: the recording stops by itself. `Saved`
    /// follows, `Failed(NoAudio)` when nothing came in, or nothing when too
    /// little was recorded to keep.
    NoInput,
    /// `source` moved to another device on its own: `name` is the one it
    /// records from now. `fell_back` says the device the user chose was
    /// missing and the Windows default took over; otherwise the default
    /// itself changed. `manual` is a switch the user asked for: no notice,
    /// but the names shown are read again.
    DeviceSwitched {
        source: Source,
        name: String,
        fell_back: bool,
        manual: bool,
    },
}

/// Why a recording failed, in the terms the island words it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Windows refused to write in the folder (Controlled folder access,
    /// permissions).
    FolderBlocked,
    /// The folder, or its drive, is gone.
    FolderGone,
    DiskFull,
    /// Media Foundation is missing (Windows N without the Media Feature Pack).
    NoMediaFoundation,
    /// No microphone or output device to record from.
    NoDevice,
    /// Nothing came in to record.
    NoAudio,
    /// The audio is complete in `part`, which could not take its final name.
    Unfinished { part: PathBuf },
    /// Anything else; the text is for the log.
    Other(String),
}

/// Why a recording could not start or finish.
#[derive(Debug)]
pub enum RecError {
    NoSources,
    Capture(CaptureError),
    Encode(EncodeError),
    Io(std::io::Error),
    /// The finished `.part` could not be renamed to its final name.
    Rename { part: PathBuf, err: std::io::Error },
    /// COM did not start on the writer thread.
    Com(windows::core::Error),
    /// Windows refused to make the folder, or a file in it (see `refused`).
    Refused(PathBuf),
    /// A thread could not start, panicked or ended early.
    Thread(String),
    /// Long enough to keep, but no source delivered a single packet (the
    /// PC's audio alone while nothing played): the file is removed.
    Silent,
}

/// A "file or path not found" (os error 2 or 3) while making a folder or a
/// file where the drive (or the folder) does exist: Controlled folder access
/// refuses that way, not with "access denied". Where the drive itself is
/// gone, it is a folder that is gone.
fn refused(code: Option<u32>, there: bool) -> bool {
    matches!(code, Some(2 | 3)) && there
}

/// The drive or share `dir` lives on is there.
fn root_exists(dir: &Path) -> bool {
    dir.ancestors().last().is_some_and(Path::exists)
}

/// `HRESULT_FROM_WIN32` codes carry the Win32 error in their low word.
fn win32_of_hresult(code: windows::core::HRESULT) -> Option<u32> {
    let raw = code.0 as u32;
    (raw & 0xFFFF_0000 == 0x8007_0000).then_some(raw & 0xFFFF)
}

impl RecError {
    /// The mic was refused by Windows: the app points at the privacy page.
    pub fn is_mic_denied(&self) -> bool {
        matches!(self, RecError::Capture(CaptureError::MicDenied))
    }

    /// Nothing worth keeping was recorded: a quiet cancel, not a failure.
    pub fn is_empty(&self) -> bool {
        matches!(self, RecError::Encode(EncodeError::Empty))
    }

    /// The Win32 error behind a file or encoder failure.
    fn win32(&self) -> Option<u32> {
        match self {
            Self::Io(e) | Self::Rename { err: e, .. } => {
                e.raw_os_error().and_then(|c| u32::try_from(c).ok())
            }
            Self::Encode(EncodeError::Io(e)) => win32_of_hresult(e.code()),
            _ => None,
        }
    }

    /// Windows refused to write in the folder: an `Io` error that is
    /// `PermissionDenied` (os error 5), or the encoder's `E_ACCESSDENIED`
    /// (0x80070005). Controlled folder access refuses this way.
    pub fn is_access_denied(&self) -> bool {
        matches!(self, Self::Refused(_))
            || matches!(self, Self::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied)
            || matches!(self, Self::Io(_) | Self::Encode(EncodeError::Io(_)) if self.win32() == Some(5))
    }

    /// What the island tells about it.
    pub fn failure(&self) -> Failure {
        if let Self::Rename { part, .. } = self {
            return Failure::Unfinished { part: part.clone() };
        }
        if self.is_access_denied() {
            return Failure::FolderBlocked;
        }
        match self.win32() {
            // ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL.
            Some(112 | 39) => return Failure::DiskFull,
            // File or path not found, the drive not ready, the network path
            // or name gone.
            Some(2 | 3 | 21 | 53 | 67) => return Failure::FolderGone,
            _ => {}
        }
        match self {
            Self::Io(e) if e.kind() == std::io::ErrorKind::NotFound => Failure::FolderGone,
            Self::Encode(EncodeError::NoMediaFoundation) => Failure::NoMediaFoundation,
            Self::Encode(EncodeError::Empty) | Self::NoSources | Self::Silent => {
                Failure::NoAudio
            }
            Self::Capture(CaptureError::NoDevice(_) | CaptureError::DeviceLost(_)) => {
                Failure::NoDevice
            }
            other => Failure::Other(other.to_string()),
        }
    }
}

impl fmt::Display for RecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSources => f.write_str("no source selected"),
            Self::Capture(e) => write!(f, "{e}"),
            Self::Encode(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "file error: {e}"),
            Self::Rename { part, err } => {
                write!(f, "{} could not be renamed: {err}", part.display())
            }
            Self::Com(e) => write!(f, "COM did not start on the writer thread: {e}"),
            Self::Refused(dir) => write!(f, "Windows refused to write in {}", dir.display()),
            Self::Thread(s) => f.write_str(s),
            Self::Silent => f.write_str("no source delivered any audio"),
        }
    }
}

impl std::error::Error for RecError {}

fn source_index(source: Source) -> usize {
    match source {
        Source::Mic => 0,
        Source::System => 1,
    }
}

fn source_name(source: Source) -> &'static str {
    match source {
        Source::Mic => "mic",
        Source::System => "system",
    }
}

// ---- placing packets on the timeline (pure) ------------------------------

/// One contiguous stretch of a source's packets at one rate.
struct Run {
    rate: u32,
    resampler: Resampler,
    /// Output frame of the run's first source frame.
    anchor: i64,
    /// Source frames fed so far.
    fed: u64,
    /// Output frames the resampler made so far.
    placed: i64,
    /// The resampler's lag in output frames, subtracted when placing.
    delay: i64,
    /// Output frames the stream was moved by: the jumps, plus one for each
    /// sample inserted and minus one for each dropped.
    shift: i64,
    /// The stamp error (stamp minus `expected`, output frames), smoothed.
    err: f64,
}

impl Run {
    fn new(rate: u32, anchor: i64) -> Self {
        let resampler = Resampler::new(rate, RATE);
        let delay = resampler.delay().round() as i64;
        Self {
            rate,
            resampler,
            anchor,
            fed: 0,
            placed: 0,
            delay,
            shift: 0,
            err: 0.0,
        }
    }

    /// Where the next packet should start, in output frames.
    fn expected(&self) -> i64 {
        self.anchor
            + self.shift
            + (i128::from(self.fed) * i128::from(RATE) / i128::from(self.rate)) as i64
    }
}

struct Lane {
    channel: usize,
    run: Option<Run>,
}

/// One packet resampled and given its place: `samples` go on `channel`
/// from raw frame `at`.
pub struct Placed {
    channel: usize,
    at: i64,
    samples: Vec<f32>,
}

/// The capture thread's side of placing: one channel per source (mic
/// first), each source resampled on its own to 48 kHz and positioned by its
/// QPC time relative to `origin`. Owned by the capture thread, so the
/// resampling runs outside the [`Placer`] lock.
///
/// Within a run the packets are laid end to end. Two clocks drift apart
/// (100 ppm is 6 ms in a minute), so when the smoothed stamp error passes
/// 1 ms one sample per packet is inserted (a copy of its first) or dropped
/// until they agree; a jump of 20 to 200 ms (a loopback silence gap) moves
/// the stream at once and keeps the resampler; more than 200 ms or a new
/// rate starts a new run.
pub struct Lanes {
    origin: u64,
    lanes: [Option<Lane>; 2],
}

impl Lanes {
    /// `origin` is the recording's start in `qpc_now` units.
    pub fn new(sources: Sources, origin: u64) -> Self {
        let mut lanes = [None, None];
        for (channel, source) in sources.list().into_iter().enumerate() {
            lanes[source_index(source)] = Some(Lane { channel, run: None });
        }
        Self { origin, lanes }
    }

    /// `source` is one of the recorded ones.
    pub fn records(&self, source: Source) -> bool {
        self.lanes[source_index(source)].is_some()
    }

    /// Resamples one capture packet and says where it goes; `None` for a
    /// source that is not recorded or an empty packet.
    pub fn place(&mut self, source: Source, qpc: u64, rate: u32, mono: &[f32]) -> Option<Placed> {
        let at = qpc_to_frame(qpc, QPC_UNITS_PER_SEC, self.origin, RATE);
        let lane = self.lanes[source_index(source)].as_mut()?;
        if rate == 0 || mono.is_empty() {
            return None;
        }
        let fresh = lane
            .run
            .as_ref()
            .is_none_or(|r| r.rate != rate || (at - r.expected()).abs() > RESTART_FRAMES);
        if fresh {
            lane.run = Some(Run::new(rate, at));
        }
        let run = lane.run.as_mut().expect("a run was just made");
        let err = at - run.expected();
        if err.abs() > JUMP_FRAMES {
            run.shift += err;
            run.err = 0.0;
        } else {
            run.err += (err as f64 - run.err) / SLEW_SMOOTHING;
        }
        let mut out = run.resampler.process(mono);
        let made = out.len() as i64;
        let pos = run.anchor - run.delay + run.placed + run.shift;
        if run.err > SLEW_FRAMES && !out.is_empty() {
            // Placed too early for the stamps: one more sample.
            out.insert(0, out[0]);
            run.shift += 1;
            run.err -= 1.0;
        } else if run.err < -SLEW_FRAMES && out.len() > 1 {
            // Placed too late: one sample fewer.
            out.remove(0);
            run.shift -= 1;
            run.err += 1.0;
        }
        run.placed += made;
        run.fed += mono.len() as u64;
        Some(Placed {
            channel: lane.channel,
            at: pos,
            samples: out,
        })
    }
}

/// The shared side, behind the recording's lock: the file's 48 kHz
/// timeline (gaps read as zeros; see [`timeline::Timeline`] for pauses and
/// late packets) and the peak meters.
pub struct Placer {
    origin: u64,
    sources: Sources,
    timeline: Timeline,
    meters: [PeakMeter; 2],
}

impl Placer {
    /// `origin` is the recording's start in `qpc_now` units.
    pub fn new(sources: Sources, origin: u64) -> Self {
        Self {
            origin,
            sources,
            timeline: Timeline::new(RATE, sources.list().len()),
            meters: [PeakMeter::new(), PeakMeter::new()],
        }
    }

    pub fn channels(&self) -> usize {
        self.timeline.channels()
    }

    /// The raw timeline frame (pauses included) at `qpc`.
    pub fn frame_at(&self, qpc: u64) -> i64 {
        qpc_to_frame(qpc, QPC_UNITS_PER_SEC, self.origin, RATE)
    }

    /// Puts a packet [`Lanes::place`] made on the timeline.
    pub fn put(&mut self, placed: &Placed) {
        self.timeline.put(placed.channel, placed.at, &placed.samples);
    }

    /// Feeds `source`'s meter with a packet's samples.
    pub fn meter(&mut self, source: Source, mono: &[f32], now: Instant) {
        self.meters[source_index(source)].feed_at(mono, now);
    }

    /// Drops `source`'s held peak: its meter reads 0 now.
    fn silence_meter(&mut self, source: Source) {
        self.meters[source_index(source)] = PeakMeter::new();
    }

    /// At most a second of what lies before raw frame `frame`, interleaved
    /// (see `Timeline::take_max`); call again for the rest.
    pub fn take(&mut self, frame: i64) -> Vec<f32> {
        self.timeline.take_max(frame, TAKE_FRAMES)
    }

    pub fn pause_at(&mut self, frame: i64) {
        self.timeline.pause_at(frame);
    }

    pub fn resume_at(&mut self, frame: i64) {
        self.timeline.resume_at(frame);
    }

    /// Whether [`Placer::bridge`] would cut between these ticks.
    pub fn cuts(&self, last: i64, now: i64) -> bool {
        now - last > SLEEP_GAP_FRAMES && !self.timeline.paused()
    }

    /// The writer ticked at raw frame `last` and again at `now`: more than
    /// 2 s between them means the PC slept (QPC counts on through sleep),
    /// and that span is cut out like a pause, unless a pause is open anyway.
    /// Says whether it cut.
    pub fn bridge(&mut self, last: i64, now: i64) -> bool {
        if self.cuts(last, now) {
            self.timeline.pause_at(last);
            self.timeline.resume_at(now);
            return true;
        }
        false
    }

    /// Frames taken so far that held a sample of some source.
    pub fn heard(&self) -> u64 {
        self.timeline.heard()
    }

    /// The decaying peak of `source`, 0..=1.
    pub fn level(&self, source: Source, now: Instant) -> f32 {
        self.meters[source_index(source)].level(now)
    }

    fn publish(&self, levels: &SharedLevels, now: Instant) {
        levels.set_mic(self.level(Source::Mic, now));
        levels.set_system(self.level(Source::System, now));
    }
}

// ---- the session -----------------------------------------------------------

/// A running capture, as the recorder sees it (a fake one in tests).
trait Feed: Send {
    fn stop(self: Box<Self>);
    /// Switches `source` to the endpoint `id` (`None`: the Windows default).
    fn set_device(&self, source: Source, id: Option<String>);
    /// The names of the devices in use, `[mic, system]`.
    fn device_names(&self) -> [String; 2];
}

impl Feed for Capture {
    fn stop(self: Box<Self>) {
        Capture::stop(*self);
    }

    fn set_device(&self, source: Source, id: Option<String>) {
        Capture::set_device(self, source, id);
    }

    fn device_names(&self) -> [String; 2] {
        Capture::device_names(self)
    }
}

/// What the writer thread is told.
enum Msg {
    /// The capture is running: start taking from the timeline.
    Go,
    /// The capture could not start: remove the file and end.
    Abort,
    /// Finish at this raw frame.
    Stop(i64),
    /// From the capture thread: a source stopped (`true`: the mic was refused).
    Lost(Source, bool),
    /// From the capture thread: a source moved to the device named, because
    /// of a fall back (`true`), a new Windows default or the user's pick
    /// (`manual`).
    Switched(Source, String, bool, bool),
}

/// Which sources are muted, shared between the island (which flips them) and
/// the capture thread (which reads them for every packet).
#[derive(Default)]
struct Muted([AtomicBool; 2]);

impl Muted {
    fn get(&self, source: Source) -> bool {
        self.0[source_index(source)].load(Ordering::Relaxed)
    }

    fn set(&self, source: Source, on: bool) {
        self.0[source_index(source)].store(on, Ordering::Relaxed);
    }
}

/// The capture side: places packets and reports lost sources to the writer.
struct Sink {
    lanes: Lanes,
    placer: Arc<Mutex<Placer>>,
    levels: Arc<SharedLevels>,
    muted: Arc<Muted>,
    tx: Sender<Msg>,
}

impl AudioSink for Sink {
    fn audio(&mut self, source: Source, qpc: u64, rate: u32, mono: &[f32]) {
        if !self.lanes.records(source) {
            return;
        }
        let now = Instant::now();
        // The resampling runs before the lock; the writer waits only for
        // the copy onto the timeline.
        let mut placed = self.lanes.place(source, qpc, rate, mono);
        // A muted source keeps its packets (they hold the timeline) but
        // writes zeros, and its meter reads 0 at once.
        let muted = self.muted.get(source);
        if muted && let Some(placed) = &mut placed {
            placed.samples.fill(0.0);
        }
        let p = &mut *lock(&self.placer);
        if let Some(placed) = &placed {
            p.put(placed);
        }
        if muted {
            p.silence_meter(source);
        } else {
            p.meter(source, mono, now);
        }
        p.publish(&self.levels, now);
    }

    fn lost(&mut self, source: Source, err: CaptureError) {
        let denied = matches!(err, CaptureError::MicDenied);
        let _ = self.tx.send(Msg::Lost(source, denied));
    }

    fn switched(&mut self, source: Source, name: &str, fell_back: bool, manual: bool) {
        let _ = self
            .tx
            .send(Msg::Switched(source, name.to_owned(), fell_back, manual));
    }
}

fn lock(placer: &Mutex<Placer>) -> MutexGuard<'_, Placer> {
    placer.lock().unwrap_or_else(|e| e.into_inner())
}

/// One recording. Dropping it without `stop` stops and saves it all the same.
pub struct Recorder {
    path: PathBuf,
    /// For the debug CLI and the tests.
    #[cfg(any(test, debug_assertions))]
    container: Container,
    placer: Arc<Mutex<Placer>>,
    levels: Arc<SharedLevels>,
    muted: Arc<Muted>,
    started: Instant,
    paused_since: Option<Instant>,
    paused_total: Duration,
    feed: Option<Box<dyn Feed>>,
    writer: Option<(Sender<Msg>, Writer)>,
    /// A mix recording: no file, and pauses mean nothing.
    mix: bool,
}

/// The writer thread, which returns the outcome.
type Writer = JoinHandle<Result<RecResult, RecError>>;

impl Recorder {
    /// Starts recording `opts.sources` into a new file in `opts.dir`.
    /// `events` is called from the writer thread (lost sources, then the
    /// outcome). Fails, with no thread left and no file, when the folder,
    /// the encoder or every source fails; `RecError::is_mic_denied` tells a
    /// refused mic.
    pub fn start(
        opts: RecOptions,
        events: Box<dyn Fn(RecEvent) + Send>,
    ) -> Result<Recorder, RecError> {
        let choices = Arc::new(Mutex::new(capture::ChoiceSet::new(opts.devices.clone())));
        let echo_cancel = opts.echo_cancel;
        Self::start_with(opts, events, |sources, sink| {
            Capture::start(sources, choices, echo_cancel, sink).map(|c| Box::new(c) as Box<dyn Feed>)
        })
    }

    fn start_with(
        opts: RecOptions,
        events: Box<dyn Fn(RecEvent) + Send>,
        open: impl FnOnce(Sources, Box<dyn AudioSink>) -> Result<Box<dyn Feed>, CaptureError>,
    ) -> Result<Recorder, RecError> {
        if !opts.sources.any() {
            return Err(RecError::NoSources);
        }
        if let Err(e) = std::fs::create_dir_all(&opts.dir) {
            let code = e.raw_os_error().and_then(|c| u32::try_from(c).ok());
            return Err(if refused(code, root_exists(&opts.dir)) {
                RecError::Refused(opts.dir.clone())
            } else {
                RecError::Io(e)
            });
        }
        let started_at = chrono::Local::now();
        let stem = naming::file_stem(started_at.naive_local(), &opts.title);
        let stem = naming::unique_stem(&opts.dir, &stem, Path::exists);
        let [path, part, sidecar] = naming::files_of(&opts.dir, &stem);
        let files = Files {
            part,
            sidecar,
            path: path.clone(),
        };
        let meta = Sidecar {
            v: 1,
            kind: "audio".into(),
            title: opts.title.clone(),
            started: started_at.fixed_offset(),
            duration_ms: 0,
            channels: opts
                .sources
                .list()
                .into_iter()
                .map(|s| source_name(s).to_string())
                .collect(),
            app: opts.app.clone(),
        };

        let placer = Arc::new(Mutex::new(Placer::new(opts.sources, qpc_now())));
        let levels = Arc::new(SharedLevels::new());
        let muted = Arc::new(Muted::default());
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let writer = {
            let placer = Arc::clone(&placer);
            let levels = Arc::clone(&levels);
            std::thread::Builder::new()
                .name("rec-writer".into())
                .spawn(move || write_main(files, meta, &placer, &levels, &rx, &ready_tx, events))
                .map_err(|e| RecError::Thread(format!("no writer thread: {e}")))?
        };
        let _container = match ready_rx.recv() {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                let _ = writer.join();
                // The folder is there, yet the file could not be made in it.
                return Err(match e {
                    RecError::Encode(EncodeError::Io(ref x))
                        if refused(win32_of_hresult(x.code()), opts.dir.is_dir()) =>
                    {
                        RecError::Refused(opts.dir.clone())
                    }
                    e => e,
                });
            }
            Err(_) => {
                return Err(match writer.join() {
                    Ok(Err(e)) => e,
                    _ => RecError::Thread("the writer thread ended early".into()),
                });
            }
        };

        // The recording starts now: the encoder is ready, the devices open next.
        let origin = qpc_now();
        let started = Instant::now();
        lock(&placer).origin = origin;
        let sink = Box::new(Sink {
            lanes: Lanes::new(opts.sources, origin),
            placer: Arc::clone(&placer),
            levels: Arc::clone(&levels),
            muted: Arc::clone(&muted),
            tx: tx.clone(),
        });
        let feed = match open(opts.sources, sink) {
            Ok(feed) => feed,
            Err(e) => {
                let _ = tx.send(Msg::Abort);
                let _ = writer.join();
                return Err(RecError::Capture(e));
            }
        };
        let _ = tx.send(Msg::Go);
        Ok(Recorder {
            path,
            #[cfg(any(test, debug_assertions))]
            container: _container,
            placer,
            levels,
            muted,
            started,
            paused_since: None,
            paused_total: Duration::ZERO,
            feed: Some(feed),
            writer: Some((tx, writer)),
            mix: false,
        })
    }

    // Wired into the screen recorder by the next task.
    #[allow(dead_code)]
    /// Like [`Recorder::start`], but nothing is written: the sources are
    /// mixed into mono and pushed to `sink`. See [`mix`].
    pub fn start_mix(
        opts: mix::MixOptions,
        sink: Box<dyn mix::MixSink>,
        events: Box<dyn Fn(RecEvent) + Send>,
    ) -> Result<Recorder, RecError> {
        let choices = Arc::new(Mutex::new(capture::ChoiceSet::new(opts.devices.clone())));
        let echo_cancel = opts.echo_cancel;
        Self::start_mix_with(opts, sink, events, |sources, audio| {
            Capture::start(sources, choices, echo_cancel, audio).map(|c| Box::new(c) as Box<dyn Feed>)
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn start_mix_with(
        opts: mix::MixOptions,
        sink: Box<dyn mix::MixSink>,
        events: Box<dyn Fn(RecEvent) + Send>,
        open: impl FnOnce(Sources, Box<dyn AudioSink>) -> Result<Box<dyn Feed>, CaptureError>,
    ) -> Result<Recorder, RecError> {
        if !opts.sources.any() {
            return Err(RecError::NoSources);
        }
        let origin = qpc_now();
        let placer = Arc::new(Mutex::new(Placer::new(opts.sources, origin)));
        let levels = Arc::new(SharedLevels::new());
        let muted = Arc::new(Muted::default());
        let (tx, rx) = mpsc::channel();
        let writer = {
            let placer = Arc::clone(&placer);
            let levels = Arc::clone(&levels);
            let sources = opts.sources;
            std::thread::Builder::new()
                .name("rec-mix".into())
                .spawn(move || {
                    mix_main(sources, origin as i64, sink, &placer, &levels, &rx, events)
                })
                .map_err(|e| RecError::Thread(format!("no mix thread: {e}")))?
        };
        let started = Instant::now();
        let audio = Box::new(Sink {
            lanes: Lanes::new(opts.sources, origin),
            placer: Arc::clone(&placer),
            levels: Arc::clone(&levels),
            muted: Arc::clone(&muted),
            tx: tx.clone(),
        });
        let feed = match open(opts.sources, audio) {
            Ok(feed) => feed,
            Err(e) => {
                let _ = tx.send(Msg::Abort);
                let _ = writer.join();
                return Err(RecError::Capture(e));
            }
        };
        let _ = tx.send(Msg::Go);
        Ok(Recorder {
            path: PathBuf::new(),
            #[cfg(any(test, debug_assertions))]
            // Only the debug CLI and the tests read it; a mix recording has no file.
            container: Container::Fragmented,
            placer,
            levels,
            muted,
            started,
            paused_since: None,
            paused_total: Duration::ZERO,
            feed: Some(feed),
            writer: Some((tx, writer)),
            mix: true,
        })
    }

    /// Pauses: the audio from now until `resume` is left out of the file. A
    /// mix recording ignores it (the screen recorder drops paused audio).
    pub fn pause(&mut self) {
        if self.mix || self.paused_since.is_some() {
            return;
        }
        let p = &mut *lock(&self.placer);
        let frame = p.frame_at(qpc_now());
        p.pause_at(frame);
        self.paused_since = Some(Instant::now());
    }

    pub fn resume(&mut self) {
        if self.mix {
            return;
        }
        let Some(since) = self.paused_since.take() else {
            return;
        };
        self.paused_total += since.elapsed();
        let p = &mut *lock(&self.placer);
        let frame = p.frame_at(qpc_now());
        p.resume_at(frame);
    }

    pub fn paused(&self) -> bool {
        self.paused_since.is_some()
    }

    /// Time recorded so far, pauses excluded (the debug CLI and the tests;
    /// the island draws from [Recorder::clock]).
    #[cfg(any(test, debug_assertions))]
    pub fn elapsed(&self) -> Duration {
        let end = self.paused_since.unwrap_or_else(Instant::now);
        end.saturating_duration_since(self.started)
            .saturating_sub(self.paused_total)
    }

    /// The recording's clock as `(started, paused_total, paused_since)`:
    /// the time since `started` without the pauses is the recorded time,
    /// which the saved duration matches (`started` is taken with the
    /// timeline's origin). The island's REC clock draws from these.
    pub fn clock(&self) -> (Instant, Duration, Option<Instant>) {
        (self.started, self.paused_total, self.paused_since)
    }

    /// The input levels `(mic, system)`, 0..=1, decaying over 300 ms; a
    /// source that is not recorded reads 0. Never waits.
    pub fn levels(&self) -> (f32, f32) {
        (self.levels.mic(), self.levels.system())
    }

    /// Mutes or unmutes `source`: while muted its packets still arrive and
    /// keep the timeline, but zeros are written and its meter reads 0. A
    /// source that is not recorded is unaffected. Never waits.
    pub fn set_muted(&self, source: Source, on: bool) {
        self.muted.set(source, on);
    }

    /// Records `source` from the endpoint `id` from now on (`None`: the
    /// Windows default, followed). The switch happens at once, with a gap of
    /// about 100 ms. A source that is not recorded is unaffected. Never waits.
    pub fn set_device(&self, source: Source, id: Option<String>) {
        if let Some(feed) = &self.feed {
            feed.set_device(source, id);
        }
    }

    /// The names of the devices the sources record from now, `[mic, system]`
    /// (empty for a source that is not recorded). Never waits on a device.
    pub fn device_names(&self) -> [String; 2] {
        self.feed.as_ref().map(|f| f.device_names()).unwrap_or_default()
    }

    /// Which sources are muted, `[mic, system]`.
    pub fn muted(&self) -> [bool; 2] {
        [self.muted.get(Source::Mic), self.muted.get(Source::System)]
    }

    /// The final `.m4a` path (the file is `<path>.part` until `stop`).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The MP4 flavour the encoder accepted.
    #[cfg(any(test, debug_assertions))]
    pub fn container(&self) -> Container {
        self.container
    }

    /// Stops and finalizes; blocks until the file and the sidecar are written
    /// (a mix recording: until the rest of the mix is pushed to its sink) and
    /// the threads have ended. The outcome is also sent to `events`.
    pub fn stop(mut self) -> Result<RecResult, RecError> {
        self.finish()
    }

    fn finish(&mut self) -> Result<RecResult, RecError> {
        let frame = lock(&self.placer).frame_at(qpc_now());
        if let Some(feed) = self.feed.take() {
            feed.stop();
        }
        let Some((tx, writer)) = self.writer.take() else {
            return Err(RecError::Thread("the recording was already stopped".into()));
        };
        let _ = tx.send(Msg::Stop(frame));
        drop(tx);
        writer
            .join()
            .unwrap_or_else(|_| Err(RecError::Thread("the writer thread panicked".into())))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if self.writer.is_some() {
            let _ = self.finish();
        }
    }
}

/// The files of one recording.
struct Files {
    path: PathBuf,
    part: PathBuf,
    sidecar: PathBuf,
}

/// The writer thread: COM, the encoder, the 50 ms pull loop and the finish.
fn write_main(
    files: Files,
    meta: Sidecar,
    placer: &Mutex<Placer>,
    levels: &SharedLevels,
    rx: &Receiver<Msg>,
    ready: &SyncSender<Result<Container, RecError>>,
    events: Box<dyn Fn(RecEvent) + Send>,
) -> Result<RecResult, RecError> {
    if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        let _ = ready.send(Err(RecError::Com(e.clone())));
        return Err(RecError::Com(e));
    }
    let (channels, sources) = {
        let p = lock(placer);
        (p.channels() as u16, p.sources)
    };
    let result = match M4aWriter::create(&files.part, channels, RATE) {
        Err(e) => {
            let _ = std::fs::remove_file(&files.part);
            let _ = ready.send(Err(RecError::Encode(e)));
            Err(RecError::Thread("the encoder did not start".into()))
        }
        Ok(writer) => {
            let _ = ready.send(Ok(writer.container()));
            let mut w = Writing {
                files: &files,
                writer: Some(writer),
                channels: u64::from(channels),
                written: 0,
                last_tick: None,
            };
            let mut losses = Losses::new(sources);
            let outcome = w.run(&meta, placer, levels, rx, &*events, &mut losses);
            match &outcome {
                Ok(r) => events(RecEvent::Saved {
                    path: r.path.clone(),
                    duration: r.duration,
                }),
                Err(None) => {}
                // Too little to keep: a quiet cancel.
                Err(Some(e)) if e.is_empty() => {}
                Err(Some(e)) => events(RecEvent::Failed(e.failure())),
            }
            outcome.map_err(|e| {
                e.unwrap_or_else(|| RecError::Thread("the recording did not start".into()))
            })
        }
    };
    unsafe { CoUninitialize() };
    result
}

/// The sources a recording takes, and which of them are lost.
struct Losses {
    recorded: Vec<Source>,
    gone: [bool; 2],
}

impl Losses {
    fn new(sources: Sources) -> Self {
        Self {
            recorded: sources.list(),
            gone: [false; 2],
        }
    }

    /// `source` is lost: tells it, and says whether every recorded source
    /// is lost now (then the last loss is `NoInput`, not "keeps going").
    fn lose(&mut self, source: Source, denied: bool, events: &dyn Fn(RecEvent)) -> bool {
        self.gone[source_index(source)] = true;
        let all = self.recorded.iter().all(|s| self.gone[source_index(*s)]);
        if denied {
            events(RecEvent::MicDenied);
        } else if !all {
            events(RecEvent::SourceLost(source));
        }
        all
    }
}

/// A tick at raw frame `now`, after the one at `last_tick`: a sleep in
/// between is cut out ([`Placer::bridge`]). Returns the frames it cut.
fn tick(placer: &Mutex<Placer>, last_tick: &mut Option<i64>, now: i64) -> Option<i64> {
    let mut gap = None;
    if let Some(last) = *last_tick
        && lock(placer).bridge(last, now)
    {
        gap = Some(now - last);
    }
    *last_tick = Some(now);
    gap
}

/// At most a second of what lies `lag` frames or more before raw frame
/// `now` and was not taken yet; empty when there is nothing more.
fn next_block(placer: &Mutex<Placer>, now: i64, lag: i64) -> Vec<f32> {
    lock(placer).take(now - lag)
}

/// Why [`pump`] ended without a stop frame.
enum Pumped {
    /// The capture never started: nothing to report.
    Aborted,
    /// A tick failed.
    Failed(RecError),
}

/// The writer's paced loop, shared by the file and the mix writer: waits
/// for the capture to start (what it lost or switched while opening is
/// told once it runs), then reports losses and switches, calls `on_tick`
/// with the raw frame every 50 ms (publishing the meters first) and returns
/// the raw frame to finish at: a stop, every source lost, or the session
/// dropped.
fn pump(
    placer: &Mutex<Placer>,
    levels: &SharedLevels,
    rx: &Receiver<Msg>,
    events: &dyn Fn(RecEvent),
    losses: &mut Losses,
    mut on_tick: impl FnMut(i64) -> Result<(), RecError>,
) -> Result<i64, Pumped> {
    let mut early = Vec::new();
    let mut switched = Vec::new();
    loop {
        match rx.recv() {
            Ok(Msg::Go) => break,
            Ok(Msg::Lost(source, denied)) => early.push((source, denied)),
            Ok(Msg::Switched(source, name, fell_back, manual)) => {
                switched.push((source, name, fell_back, manual));
            }
            Ok(Msg::Abort | Msg::Stop(_)) | Err(_) => return Err(Pumped::Aborted),
        }
    }
    let mut no_input = false;
    for (source, denied) in early {
        no_input |= losses.lose(source, denied, events);
    }
    for (source, name, fell_back, manual) in switched {
        events(RecEvent::DeviceSwitched {
            source,
            name,
            fell_back,
            manual,
        });
    }
    let mut next = Instant::now() + TICK;
    loop {
        if no_input {
            // Nothing comes in any more: finish now.
            events(RecEvent::NoInput);
            return Ok(lock(placer).frame_at(qpc_now()));
        }
        match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(Msg::Lost(source, denied)) => no_input = losses.lose(source, denied, events),
            Ok(Msg::Switched(source, name, fell_back, manual)) => {
                events(RecEvent::DeviceSwitched {
                    source,
                    name,
                    fell_back,
                    manual,
                });
            }
            Ok(Msg::Stop(frame)) => return Ok(frame),
            Ok(Msg::Go | Msg::Abort) => {}
            Err(RecvTimeoutError::Timeout) => {
                next += TICK;
                let now = Instant::now();
                if next < now {
                    next = now + TICK;
                }
                let frame = {
                    let p = &mut *lock(placer);
                    p.publish(levels, now);
                    p.frame_at(qpc_now())
                };
                on_tick(frame).map_err(Pumped::Failed)?;
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(lock(placer).frame_at(qpc_now())),
        }
    }
}

struct Writing<'a> {
    files: &'a Files,
    writer: Option<M4aWriter>,
    channels: u64,
    /// Samples handed to the encoder so far.
    written: u64,
    /// The raw frame of the previous tick, to notice a sleep.
    last_tick: Option<i64>,
}

impl Writing<'_> {
    /// `Err(None)`: aborted before the start (nothing to report).
    fn run(
        &mut self,
        meta: &Sidecar,
        placer: &Mutex<Placer>,
        levels: &SharedLevels,
        rx: &Receiver<Msg>,
        events: &dyn Fn(RecEvent),
        losses: &mut Losses,
    ) -> Result<RecResult, Option<RecError>> {
        let stop_frame = match pump(placer, levels, rx, events, losses, |frame| {
            self.drain(placer, frame, LAG_FRAMES)
        }) {
            Ok(frame) => frame,
            Err(Pumped::Aborted) => {
                self.discard();
                return Err(None);
            }
            Err(Pumped::Failed(e)) => return Err(Some(e)),
        };
        self.drain(placer, stop_frame, 0).map_err(Some)?;
        let heard = lock(placer).heard();
        let duration = self.finalize(heard).map_err(Some)?;
        let sidecar = Sidecar {
            duration_ms: duration.as_millis() as u64,
            ..meta.clone()
        };
        if let Err(e) = write_sidecar(&self.files.sidecar, &sidecar) {
            // The recording itself is saved; only its metadata is missing.
            eprintln!("vale: recording sidecar not written: {e}");
        }
        Ok(RecResult {
            path: self.files.path.clone(),
            duration,
        })
    }

    /// A tick at raw frame `now`: a sleep since the last one is cut out
    /// ([`Placer::bridge`]), then everything up to `lag` frames before
    /// `now` is encoded, a second at a time.
    fn drain(&mut self, placer: &Mutex<Placer>, now: i64, lag: i64) -> Result<(), RecError> {
        tick(placer, &mut self.last_tick, now);
        loop {
            let block = next_block(placer, now, lag);
            if block.is_empty() {
                return Ok(());
            }
            self.write(&block)?;
        }
    }

    fn write(&mut self, block: &[f32]) -> Result<(), RecError> {
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };
        if block.is_empty() {
            return Ok(());
        }
        match writer.write(block) {
            Ok(()) => {
                self.written += block.len() as u64;
                Ok(())
            }
            Err(e) => {
                self.give_up();
                Err(RecError::Encode(e))
            }
        }
    }

    /// Finishes the file and renames it to its final name. Under half a
    /// second the file is removed instead (`EncodeError::Empty`, a quiet
    /// cancel); with nothing heard from any source too (`RecError::Silent`,
    /// which the island words).
    fn finalize(&mut self, heard: u64) -> Result<Duration, RecError> {
        if self.written / self.channels.max(1) < MIN_KEEP_FRAMES {
            self.discard();
            return Err(RecError::Encode(EncodeError::Empty));
        }
        if heard == 0 {
            self.discard();
            return Err(RecError::Silent);
        }
        let writer = self.writer.take().expect("finalize runs once");
        let duration = match writer.finish() {
            Ok(d) => d,
            // `finish` removed the empty file itself.
            Err(EncodeError::Empty) => return Err(RecError::Encode(EncodeError::Empty)),
            Err(e) => {
                self.give_up();
                return Err(RecError::Encode(e));
            }
        };
        std::fs::rename(&self.files.part, &self.files.path).map_err(|err| RecError::Rename {
            part: self.files.part.clone(),
            err,
        })?;
        Ok(duration)
    }

    /// After an error: the `.part` stays when it holds audio (fragmented MP4
    /// plays), and goes when nothing was written.
    fn give_up(&mut self) {
        self.writer = None;
        if self.written == 0 {
            let _ = std::fs::remove_file(&self.files.part);
        }
    }

    fn discard(&mut self) {
        self.writer = None;
        let _ = std::fs::remove_file(&self.files.part);
    }
}

/// What `mix_main` keeps between ticks: the stamps, the previous tick and
/// the frames pushed.
struct MixOut {
    clock: mix::MixClock,
    channels: usize,
    last_tick: Option<i64>,
    pushed: u64,
}

impl MixOut {
    /// Pushes what lies `lag` frames or more before raw frame `frame`.
    fn push_up_to(&mut self, placer: &Mutex<Placer>, frame: i64, lag: i64, sink: &mut dyn mix::MixSink) {
        loop {
            let block = next_block(placer, frame, lag);
            if block.is_empty() {
                return;
            }
            let mono = mix::mix_mono(&block, self.channels);
            sink.mix(&mono, self.clock.stamp());
            self.clock.advance(mono.len() as u64);
            self.pushed += mono.len() as u64;
        }
    }
}

/// A tick of a mix recording at raw frame `frame`: everything up to `lag`
/// frames before it goes out as mono, stamped by the clock. A sleep since
/// the last tick is cut out like in [`tick`], but first the audio before it
/// is pushed in full, so it keeps its own time and not the sleep's end.
fn drain_mix(
    placer: &Mutex<Placer>,
    out: &mut MixOut,
    frame: i64,
    lag: i64,
    sink: &mut dyn mix::MixSink,
) {
    if let Some(last) = out.last_tick
        && lock(placer).cuts(last, frame)
    {
        out.push_up_to(placer, last, 0, sink);
    }
    if let Some(gap) = tick(placer, &mut out.last_tick, frame) {
        out.clock.skip(gap as u64);
    }
    out.push_up_to(placer, frame, lag, sink);
}

/// The writer thread of a mix recording: every 50 ms the timeline up to
/// 200 ms before now goes through [`mix::mix_mono`] to `sink`, stamped from
/// `origin_hns`; the rest goes at stop. Nothing is written to disk.
fn mix_main(
    sources: Sources,
    origin_hns: i64,
    mut sink: Box<dyn mix::MixSink>,
    placer: &Mutex<Placer>,
    levels: &SharedLevels,
    rx: &Receiver<Msg>,
    events: Box<dyn Fn(RecEvent) + Send>,
) -> Result<RecResult, RecError> {
    let mut out = MixOut {
        clock: mix::MixClock::new(origin_hns),
        channels: lock(placer).channels(),
        last_tick: None,
        pushed: 0,
    };
    let mut losses = Losses::new(sources);
    let stop_frame = pump(placer, levels, rx, &*events, &mut losses, |frame| {
        drain_mix(placer, &mut out, frame, LAG_FRAMES, &mut *sink);
        Ok(())
    });
    let stop_frame = match stop_frame {
        Ok(frame) => frame,
        Err(_) => return Err(RecError::Thread("the recording did not start".into())),
    };
    // The rest of the timeline goes before `stop` returns.
    drain_mix(placer, &mut out, stop_frame, 0, &mut *sink);
    Ok(RecResult {
        path: PathBuf::new(),
        duration: Duration::from_micros(out.pushed * 1_000_000 / u64::from(RATE)),
    })
}

/// Writes the sidecar, keeping keys another track added to an existing one.
fn write_sidecar(path: &Path, sidecar: &Sidecar) -> std::io::Result<()> {
    let existing = std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null);
    let value = sidecar.merge_into(existing);
    let text = serde_json::to_string_pretty(&value).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

// ---- debug CLI ---------------------------------------------------------------

/// `--record-test <seconds> [mic|system|both] [--no-aec]` (debug builds): records into
/// `DI_REC_DIR` or `target/rec-test/` and prints the path, the duration, the
/// container, the highest mic and system levels seen and any lost source.
/// Returns the process exit code.
#[cfg(debug_assertions)]
pub fn record_test(args: &[String]) -> i32 {
    let echo_cancel = !args.iter().any(|a| a == "--no-aec");
    let args: Vec<String> = args.iter().filter(|a| *a != "--no-aec").cloned().collect();
    let secs: f64 = match args.first().map(|s| s.parse::<f64>()) {
        None => 5.0,
        Some(Ok(s)) if s > 0.0 && s.is_finite() => s,
        Some(_) => {
            eprintln!("usage: vale --record-test <seconds> [mic|system|both] [--no-aec]");
            return 2;
        }
    };
    let sources = match args.get(1).map(String::as_str) {
        None | Some("both") => Sources::BOTH,
        Some("mic") => Sources {
            mic: true,
            system: false,
        },
        Some("system") => Sources {
            mic: false,
            system: true,
        },
        Some(other) => {
            eprintln!("vale: unknown source `{other}` (mic, system or both)");
            return 2;
        }
    };
    let dir = std::env::var_os("DI_REC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("rec-test")
        });
    let (etx, erx) = mpsc::channel();
    let events: Box<dyn Fn(RecEvent) + Send> = Box::new(move |e| {
        let _ = etx.send(e);
    });
    let opts = RecOptions {
        sources,
        dir,
        title: "record-test".into(),
        app: None,
        devices: [None, None],
        echo_cancel,
    };
    let rec = match Recorder::start(opts, events) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("vale: cannot record: {e}");
            if e.is_mic_denied() {
                eprintln!("vale: turn on Settings > Privacy & security > Microphone");
            }
            return 1;
        }
    };
    println!(
        "recording {secs} s of {sources:?} to {}",
        rec.path().display()
    );
    let print = |e: RecEvent| match e {
        RecEvent::SourceLost(s) => println!("lost: {s}"),
        RecEvent::MicDenied => println!("lost: microphone (denied by Windows)"),
        RecEvent::Failed(f) => println!("failed: {f:?}"),
        RecEvent::NoInput => println!("stopped: no source left"),
        RecEvent::DeviceSwitched {
            source,
            name,
            fell_back,
            manual,
        } => println!("switched: {source} to {name} (fell back: {fell_back}, by hand: {manual})"),
        RecEvent::Saved { .. } => {}
    };
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let (mut mic, mut system) = (0.0f32, 0.0f32);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
        let (m, s) = rec.levels();
        mic = mic.max(m);
        system = system.max(s);
        while let Ok(e) = erx.try_recv() {
            print(e);
        }
    }
    let container = rec.container();
    let elapsed = rec.elapsed();
    let result = rec.stop();
    while let Ok(e) = erx.try_recv() {
        print(e);
    }
    match result {
        Ok(r) => {
            println!("path:      {}", r.path.display());
            println!(
                "duration:  {:.3} s (clock {:.3} s)",
                r.duration.as_secs_f64(),
                elapsed.as_secs_f64()
            );
            println!("container: {container:?}");
            println!("peak mic:    {mic:.3}");
            println!("peak system: {system:.3}");
            0
        }
        Err(e) => {
            eprintln!("vale: recording failed: {e}");
            1
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// One millisecond in `qpc` units.
    const MS: u64 = 10_000;
    const ORIGIN: u64 = 1_000_000_000;

    fn at_ms(ms: u64) -> u64 {
        ORIGIN + ms * MS
    }

    fn frames_ms(ms: i64) -> i64 {
        ms * i64::from(RATE) / 1000
    }

    /// Both sides of placing, as the sink and the writer use them.
    struct Rig {
        lanes: Lanes,
        placer: Placer,
    }

    impl Rig {
        fn new(sources: Sources, origin: u64) -> Self {
            Self {
                lanes: Lanes::new(sources, origin),
                placer: Placer::new(sources, origin),
            }
        }

        /// What `Sink::audio` does.
        fn audio(&mut self, source: Source, qpc: u64, rate: u32, mono: &[f32], now: Instant) {
            if !self.lanes.records(source) {
                return;
            }
            if let Some(placed) = self.lanes.place(source, qpc, rate, mono) {
                self.placer.put(&placed);
            }
            self.placer.meter(source, mono, now);
        }

        /// Everything up to `frame`, in as many one-second takes as needed.
        fn take(&mut self, frame: i64) -> Vec<f32> {
            let mut all = Vec::new();
            loop {
                let block = self.placer.take(frame);
                if block.is_empty() {
                    return all;
                }
                all.extend(block);
            }
        }
    }

    impl std::ops::Deref for Rig {
        type Target = Placer;
        fn deref(&self) -> &Placer {
            &self.placer
        }
    }

    impl std::ops::DerefMut for Rig {
        fn deref_mut(&mut self) -> &mut Placer {
            &mut self.placer
        }
    }

    /// A device whose clock runs `ppm` fast (negative: slow) delivers ten
    /// minutes of 10 ms packets, stamped with the true time. The value of
    /// each sample is `1 + n mod 2^23` (`n` its index), so a zero is a gap
    /// and each frame tells which sample it holds.
    fn drifting(ppm: f64) {
        let mut p = Rig::new(
            Sources {
                mic: true,
                system: false,
            },
            ORIGIN,
        );
        let true_rate = 48_000.0 * (1.0 + ppm * 1e-6);
        let packets = 60_000u64; // ten minutes
        let mut n = 0u64;
        let mut frame = 0i64; // output frames checked so far
        let mut worst = 0i64;
        for k in 0..packets {
            let secs = n as f64 / true_rate;
            let qpc = ORIGIN + (secs * 1e7).round() as u64;
            let mono: Vec<f32> = (n..n + 480).map(|i| (1 + i % (1 << 23)) as f32).collect();
            p.audio(Source::Mic, qpc, 48_000, &mono, Instant::now());
            n += 480;
            if k % 5 == 4 {
                // A writer tick, 200 ms behind the last stamp.
                let up_to = p.frame_at(qpc) - LAG_FRAMES;
                let block = p.take(up_to);
                if let Some(i) = block.iter().position(|x| *x == 0.0) {
                    panic!("{ppm} ppm: a gap at frame {}", frame + i as i64);
                }
                for (i, x) in block.iter().enumerate().step_by(97) {
                    let f = frame + i as i64;
                    // The sample this frame holds (the one with this value
                    // nearest the frame's time), and the frame its stamp is.
                    let wrap = 1i64 << 23;
                    let guess = (f as f64 * true_rate / 48_000.0) as i64;
                    let mut d = (*x as i64 - 1 - guess).rem_euclid(wrap);
                    if d > wrap / 2 {
                        d -= wrap;
                    }
                    let due = ((guess + d) as f64 * 48_000.0 / true_rate).round() as i64;
                    worst = worst.max((f - due).abs());
                }
                frame += block.len() as i64;
            }
        }
        assert!(frame > frames_ms(599_000), "{ppm} ppm: only {frame} frames");
        assert!(
            worst <= frames_ms(2),
            "{ppm} ppm: a frame sits {worst} frames from its stamp"
        );
    }

    #[test]
    fn a_fast_device_clock_is_followed_without_a_gap() {
        drifting(100.0);
    }

    #[test]
    fn a_slow_device_clock_is_followed_without_a_gap() {
        drifting(-100.0);
    }

    #[test]
    fn an_hour_asleep_gives_bounded_takes_and_no_hour_of_zeros() {
        let mut p = Rig::new(
            Sources {
                mic: false,
                system: true,
            },
            ORIGIN,
        );
        feed(&mut p, Source::System, 48_000, |_| 0.5, (0, 1000), none);
        let before = p.take(frames_ms(1000) - LAG_FRAMES).len() as i64;
        // QPC ran on through an hour of sleep; the next tick sees it.
        let (last, now) = (frames_ms(1000), frames_ms(1000 + 3_600_000));
        p.bridge(last, now);
        feed(&mut p, Source::System, 48_000, |_| 0.5, (3_601_000, 3_601_500), none);
        let mut after = 0i64;
        loop {
            let block = p.placer.take(now + frames_ms(500) - LAG_FRAMES);
            assert!(block.len() <= TAKE_FRAMES, "one take is at most a second");
            if block.is_empty() {
                break;
            }
            after += block.len() as i64;
        }
        // 0.8 s before the sleep, then the last 0.2 s of it and 0.3 s after:
        // not an hour.
        assert_eq!(before, frames_ms(800));
        assert!(after <= frames_ms(1000), "{after} frames after the wake");
        // An open pause is left alone: the sleep falls inside it.
        let mut q = Rig::new(Sources::BOTH, ORIGIN);
        q.pause_at(frames_ms(500));
        q.bridge(frames_ms(1000), frames_ms(9000));
        q.resume_at(frames_ms(10_000));
        assert_eq!(q.take(frames_ms(10_500)).len() as i64, 2 * frames_ms(1000));
    }

    #[test]
    fn a_far_stamp_still_restarts_the_run_and_a_gap_keeps_the_resampler() {
        let mut p = Rig::new(
            Sources {
                mic: true,
                system: false,
            },
            ORIGIN,
        );
        feed(&mut p, Source::Mic, 44_100, |_| 0.4, (0, 300), none);
        let run = |p: &Rig| p.lanes.lanes[0].as_ref().unwrap().run.as_ref().unwrap().fed;
        let fed = run(&p);
        // 100 ms of nothing (a gap): the same run moves to the new stamp.
        feed(&mut p, Source::Mic, 44_100, |_| 0.4, (400, 500), none);
        assert_eq!(run(&p), fed + 10 * 441, "the same resampler");
        // A second away: a new run.
        feed(&mut p, Source::Mic, 44_100, |_| 0.4, (1500, 1510), none);
        assert_eq!(run(&p), 441);
        let m = p.take(frames_ms(1400));
        all_near(&m[48..frames_ms(290) as usize], 0.4, "before the gap");
        all_near(&m[frames_ms(301) as usize..frames_ms(399) as usize], 0.0, "the gap");
        all_near(&m[frames_ms(401) as usize..frames_ms(490) as usize], 0.4, "after the gap");
    }

    /// Feeds `source` 10 ms packets of `value(k)` at `rate` from `from_ms` to
    /// `to_ms`, each stamped at its nominal time plus `jitter(k)` (qpc units).
    fn feed(
        p: &mut Rig,
        source: Source,
        rate: u32,
        value: impl Fn(usize) -> f32,
        (from_ms, to_ms): (u64, u64),
        jitter: impl Fn(usize) -> i64,
    ) {
        let per = rate as usize / 100;
        for (k, ms) in (from_ms..to_ms).step_by(10).enumerate() {
            let qpc = (at_ms(ms) as i64 + jitter(k)) as u64;
            p.audio(source, qpc, rate, &vec![value(k); per], Instant::now());
        }
    }

    fn none(_: usize) -> i64 {
        0
    }

    fn lane(block: &[f32], ch: usize, channels: usize) -> Vec<f32> {
        block.iter().skip(ch).step_by(channels).copied().collect()
    }

    fn all_near(xs: &[f32], want: f32, what: &str) {
        for (i, x) in xs.iter().enumerate() {
            assert!(
                (x - want).abs() < 2e-3,
                "{what}: frame {i} is {x}, want {want}"
            );
        }
    }

    #[test]
    fn both_sources_land_on_their_own_channel_at_48k() {
        let mut p = Rig::new(Sources::BOTH, ORIGIN);
        assert_eq!(p.channels(), 2);
        feed(&mut p, Source::Mic, 44_100, |_| 0.5, (0, 1000), none);
        feed(&mut p, Source::System, 48_000, |_| -0.25, (0, 1000), none);
        let block = p.take(frames_ms(900));
        assert_eq!(block.len(), frames_ms(900) as usize * 2);
        let (mic, system) = (lane(&block, 0, 2), lane(&block, 1, 2));
        // The resampler's onset ramp sits on frame 0, so skip a millisecond.
        all_near(&mic[48..], 0.5, "mic");
        all_near(&system, -0.25, "system");
    }

    #[test]
    fn packet_times_are_100ns_units() {
        // A packet stamped one second (10^7 units) after the origin starts
        // at frame 48000, whatever QueryPerformanceFrequency is.
        let mut p = Rig::new(
            Sources {
                mic: false,
                system: true,
            },
            ORIGIN,
        );
        assert_eq!(p.frame_at(ORIGIN + 10_000_000), 48_000);
        assert_eq!(p.frame_at(ORIGIN + 10_000_000 / 48_000 * 10), 10);
        p.audio(
            Source::System,
            ORIGIN + 10_000_000,
            48_000,
            &[0.7; 480],
            Instant::now(),
        );
        let s = p.take(48_480);
        all_near(&s[..48_000], 0.0, "before the packet");
        all_near(&s[48_000..], 0.7, "the packet");
    }

    #[test]
    fn the_resampler_delay_is_taken_out() {
        let mut p = Rig::new(
            Sources {
                mic: true,
                system: false,
            },
            ORIGIN,
        );
        // An impulse on the first frame of the packet stamped 500 ms.
        for k in 0..100u64 {
            let mut x = vec![0.0f32; 441];
            if k == 50 {
                x[0] = 1.0;
            }
            p.audio(Source::Mic, at_ms(k * 10), 44_100, &x, Instant::now());
        }
        let mic = p.take(frames_ms(990));
        let peak = (0..mic.len())
            .max_by(|&a, &b| mic[a].total_cmp(&mic[b]))
            .unwrap() as i64;
        assert!(
            (peak - frames_ms(500)).abs() <= 1,
            "impulse at frame {peak}"
        );
    }

    #[test]
    fn a_silent_loopback_gap_reads_as_zeros() {
        let mut p = Rig::new(
            Sources {
                mic: false,
                system: true,
            },
            ORIGIN,
        );
        feed(&mut p, Source::System, 48_000, |_| 0.3, (0, 100), none);
        // Nothing plays from 100 to 300 ms: loopback sends no packets.
        feed(&mut p, Source::System, 48_000, |_| 0.3, (300, 400), none);
        let s = p.take(frames_ms(400));
        assert_eq!(s.len(), frames_ms(400) as usize);
        all_near(&s[..4800], 0.3, "before");
        all_near(&s[4800..14_400], 0.0, "gap");
        all_near(&s[14_400..], 0.3, "after");
    }

    #[test]
    fn a_rate_change_starts_a_new_resampler() {
        let mut p = Rig::new(
            Sources {
                mic: true,
                system: false,
            },
            ORIGIN,
        );
        feed(&mut p, Source::Mic, 48_000, |_| 0.2, (0, 200), none);
        // The default mic changed to a 44.1 kHz device.
        feed(&mut p, Source::Mic, 44_100, |_| 0.4, (200, 400), none);
        let m = p.take(frames_ms(380));
        all_near(&m[..9600], 0.2, "48 kHz part");
        all_near(&m[9600 + 48..], 0.4, "44.1 kHz part");
    }

    #[test]
    fn a_pause_cuts_the_span_out() {
        let mut p = Rig::new(
            Sources {
                mic: false,
                system: true,
            },
            ORIGIN,
        );
        p.pause_at(frames_ms(100));
        p.resume_at(frames_ms(200));
        // Packet k carries the value k.
        feed(&mut p, Source::System, 48_000, |k| k as f32, (0, 400), none);
        let s = p.take(frames_ms(400));
        assert_eq!(s.len(), frames_ms(300) as usize);
        for (i, x) in s.iter().enumerate() {
            let packet = i / 480;
            let want = if packet < 10 { packet } else { packet + 10 } as f32;
            assert_eq!(*x, want, "frame {i}");
        }
    }

    #[test]
    fn stamp_jitter_does_not_cut_the_audio() {
        let mut p = Rig::new(
            Sources {
                mic: true,
                system: false,
            },
            ORIGIN,
        );
        let jitter = |k: usize| match k {
            0 => 0,
            k if k % 2 == 1 => MS as i64,
            _ => -(MS as i64),
        };
        feed(&mut p, Source::Mic, 48_000, |_| 0.5, (0, 1000), jitter);
        all_near(&p.take(frames_ms(990)), 0.5, "jittered mic");
    }

    #[test]
    fn a_stamp_far_from_the_run_starts_a_new_one() {
        let mut p = Rig::new(
            Sources {
                mic: false,
                system: true,
            },
            ORIGIN,
        );
        feed(&mut p, Source::System, 48_000, |_| 0.1, (0, 100), none);
        // 15 ms late: within 20 ms, so laid straight after (100..110 ms).
        p.audio(
            Source::System,
            at_ms(115),
            48_000,
            &[0.2; 480],
            Instant::now(),
        );
        // 50 ms away from the run: placed at its own stamp (160..170 ms).
        p.audio(
            Source::System,
            at_ms(160),
            48_000,
            &[0.3; 480],
            Instant::now(),
        );
        let s = p.take(frames_ms(170));
        all_near(&s[..4800], 0.1, "run");
        all_near(&s[4800..5280], 0.2, "jittered packet");
        all_near(&s[5280..7680], 0.0, "gap");
        all_near(&s[7680..], 0.3, "new run");
    }

    #[test]
    fn one_source_is_mono_and_the_other_is_ignored() {
        let mut p = Rig::new(
            Sources {
                mic: true,
                system: false,
            },
            ORIGIN,
        );
        assert_eq!(p.channels(), 1);
        feed(&mut p, Source::System, 48_000, |_| 0.9, (0, 100), none);
        feed(&mut p, Source::Mic, 48_000, |_| 0.4, (0, 100), none);
        all_near(&p.take(frames_ms(100)), 0.4, "mic");
        // A packet with no samples or no rate is harmless.
        p.audio(Source::Mic, at_ms(100), 0, &[1.0], Instant::now());
        p.audio(Source::Mic, at_ms(100), 48_000, &[], Instant::now());
    }

    #[test]
    fn levels_follow_each_source() {
        let mut p = Rig::new(Sources::BOTH, ORIGIN);
        let now = Instant::now();
        p.audio(Source::Mic, at_ms(0), 48_000, &[0.1, -0.8], now);
        assert!((p.level(Source::Mic, now) - 0.8).abs() < 1e-6);
        assert_eq!(p.level(Source::System, now), 0.0);
        let shared = SharedLevels::new();
        p.publish(&shared, now);
        assert!((shared.mic() - 0.8).abs() < 1e-6);
        // Silence on the loopback means no packets: the level still falls.
        p.publish(&shared, now + Duration::from_millis(400));
        assert_eq!(shared.mic(), 0.0);
    }

    #[test]
    fn the_sidecar_keeps_keys_from_other_tracks() {
        let dir = temp_dir("rec-sidecar");
        let path = dir.join("a.vale.json");
        std::fs::write(
            &path,
            r#"{"v":1,"transcript":{"lang":"id"},"duration_ms":1}"#,
        )
        .unwrap();
        let s = Sidecar {
            duration_ms: 1234,
            channels: vec!["mic".into()],
            ..Sidecar::default()
        };
        write_sidecar(&path, &s).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["transcript"]["lang"], "id");
        assert_eq!(v["duration_ms"], 1234);
        // A fresh one, too.
        let fresh = dir.join("b.vale.json");
        write_sidecar(&fresh, &s).unwrap();
        assert!(fresh.exists());
    }

    #[test]
    fn errors_read_well() {
        assert_eq!(RecError::NoSources.to_string(), "no source selected");
        assert!(RecError::Capture(CaptureError::MicDenied).is_mic_denied());
        assert!(!RecError::NoSources.is_mic_denied());
    }

    #[test]
    fn a_not_found_under_a_root_or_folder_that_exists_is_a_refusal() {
        // Controlled folder access answers a new folder or file in
        // Documents with os error 2, although Documents is there.
        assert!(refused(Some(2), true));
        assert!(refused(Some(3), true));
        assert!(!refused(Some(2), false), "the drive or share is gone");
        assert!(!refused(Some(112), true));
        assert!(!refused(None, true));
        let e = RecError::Refused(PathBuf::from(r"C:\Users\a\Documents\Vale\Rekaman"));
        assert!(e.is_access_denied());
        assert_eq!(e.failure(), Failure::FolderBlocked);
        assert!(root_exists(Path::new(env!("CARGO_MANIFEST_DIR"))));
        assert!(!root_exists(Path::new(r"\\no-such-server-vale\share\x")));
    }

    #[test]
    fn failures_are_sorted_into_what_the_island_can_say() {
        use std::io::{Error, ErrorKind};
        use windows::core::HRESULT;
        let hr = |code: u32| RecError::Encode(EncodeError::Io(HRESULT(code as i32).into()));
        let os = |code: i32| RecError::Io(Error::from_raw_os_error(code));
        // Controlled folder access refuses with access denied.
        for e in [
            os(5),
            RecError::Io(Error::from(ErrorKind::PermissionDenied)),
            hr(0x8007_0005),
        ] {
            assert!(e.is_access_denied(), "{e}");
            assert_eq!(e.failure(), Failure::FolderBlocked, "{e}");
        }
        assert!(!os(112).is_access_denied());
        // A refused loopback is not a folder problem.
        let loopback = RecError::Capture(CaptureError::Com(HRESULT(0x8007_0005_u32 as i32).into()));
        assert!(!loopback.is_access_denied());
        assert_eq!(os(112).failure(), Failure::DiskFull);
        assert_eq!(hr(0x8007_0070).failure(), Failure::DiskFull);
        assert_eq!(os(3).failure(), Failure::FolderGone);
        assert_eq!(hr(0x8007_0003).failure(), Failure::FolderGone);
        assert_eq!(
            RecError::Io(Error::from(ErrorKind::NotFound)).failure(),
            Failure::FolderGone
        );
        assert_eq!(
            RecError::Encode(EncodeError::NoMediaFoundation).failure(),
            Failure::NoMediaFoundation
        );
        assert_eq!(
            RecError::Capture(CaptureError::NoDevice(Source::Mic)).failure(),
            Failure::NoDevice
        );
        assert_eq!(RecError::Encode(EncodeError::Empty).failure(), Failure::NoAudio);
        assert!(RecError::Encode(EncodeError::Empty).is_empty());
        assert_eq!(RecError::Silent.failure(), Failure::NoAudio);
        assert!(!RecError::Silent.is_empty());
        let part = PathBuf::from(r"C:\r\a.m4a.part");
        assert_eq!(
            RecError::Rename {
                part: part.clone(),
                err: Error::from_raw_os_error(5)
            }
            .failure(),
            Failure::Unfinished { part }
        );
        assert!(matches!(
            RecError::Thread("x".into()).failure(),
            Failure::Other(s) if s == "x"
        ));
    }

    // ---- sessions on a made-up capture -------------------------------------

    /// Sessions run one at a time, so the `rec-*` thread count is theirs.
    static SESSION: Mutex<()> = Mutex::new(());

    pub(super) fn session() -> MutexGuard<'static, ()> {
        SESSION.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("test dir");
        dir
    }

    fn com() {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    /// Threads of this process whose name starts with `rec-`.
    fn rec_threads() -> usize {
        use windows::Win32::Foundation::{CloseHandle, HLOCAL, LocalFree};
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        };
        use windows::Win32::System::Threading::{
            GetCurrentProcessId, GetThreadDescription, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).expect("snapshot");
            let pid = GetCurrentProcessId();
            let mut entry = THREADENTRY32 {
                dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut count = 0;
            let mut more = Thread32First(snap, &mut entry).is_ok();
            while more {
                if entry.th32OwnerProcessID == pid
                    && let Ok(h) =
                        OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, entry.th32ThreadID)
                {
                    if let Ok(name) = GetThreadDescription(h) {
                        if name.to_string().is_ok_and(|n| n.starts_with("rec-")) {
                            count += 1;
                        }
                        LocalFree(Some(HLOCAL(name.0.cast())));
                    }
                    let _ = CloseHandle(h);
                }
                more = Thread32Next(snap, &mut entry).is_ok();
            }
            let _ = CloseHandle(snap);
            count
        }
    }

    struct FakeFeed {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl Feed for FakeFeed {
        fn stop(mut self: Box<Self>) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                t.join().expect("fake capture thread");
            }
        }

        fn set_device(&self, _: Source, _: Option<String>) {}

        fn device_names(&self) -> [String; 2] {
            ["Fake mic".into(), "Fake speakers".into()]
        }
    }

    /// What a made-up capture loses.
    #[derive(Clone, Copy, PartialEq)]
    enum Lose {
        Nothing,
        /// The mic is refused while the capture opens: `sink.lost` runs
        /// before `open` returns, as `Capture::run` does it.
        MicAtOpen,
        /// After 600 ms both devices go, and nothing comes any more.
        All,
        /// The devices open but no packet ever comes (loopback while
        /// nothing plays).
        Silent,
    }

    /// A capture made up in code, on its own `rec-capture` thread: a 440 Hz
    /// tone on the mic at 44.1 kHz and an 880 Hz tone on the system audio at
    /// 48 kHz, in 10 ms packets stamped with the real clock.
    fn fake_open(
        sources: Sources,
        mut sink: Box<dyn AudioSink>,
        lose: Lose,
    ) -> Result<Box<dyn Feed>, CaptureError> {
        let mut sources = sources;
        if lose == Lose::MicAtOpen {
            sink.lost(Source::Mic, CaptureError::MicDenied);
            sources.mic = false;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("rec-capture".into())
            .spawn(move || {
                let start = qpc_now();
                let tones = [
                    (Source::Mic, 44_100u32, 440.0f64),
                    (Source::System, 48_000, 880.0),
                ];
                let mut sent = [0u64; 2];
                while !flag.load(Ordering::SeqCst) {
                    let now = qpc_now();
                    if lose == Lose::All && now >= start + 600 * MS && sources.any() {
                        sink.lost(Source::Mic, CaptureError::DeviceLost(Source::Mic));
                        sink.lost(Source::System, CaptureError::DeviceLost(Source::System));
                        sources = Sources {
                            mic: false,
                            system: false,
                        };
                    }
                    for (i, &(source, rate, hz)) in tones.iter().enumerate() {
                        if lose == Lose::Silent || !sources.list().contains(&source) {
                            continue;
                        }
                        let per = u64::from(rate) / 100;
                        while start + (sent[i] + 1) * 10 * MS <= now {
                            let first = sent[i] * per;
                            let mono: Vec<f32> = (0..per)
                                .map(|n| {
                                    let t = (first + n) as f64 / f64::from(rate);
                                    (0.5 * (std::f64::consts::TAU * hz * t).sin()) as f32
                                })
                                .collect();
                            sink.audio(source, start + sent[i] * 10 * MS, rate, &mono);
                            sent[i] += 1;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .expect("fake capture thread");
        Ok(Box::new(FakeFeed {
            stop,
            thread: Some(thread),
        }))
    }

    fn opts(dir: &Path, sources: Sources) -> RecOptions {
        RecOptions {
            sources,
            dir: dir.to_path_buf(),
            title: "Zoom".into(),
            app: Some("Zoom.exe".into()),
            devices: [None, None],
            echo_cancel: true,
        }
    }

    type Events = Box<dyn Fn(RecEvent) + Send>;

    fn collect() -> (Events, Receiver<RecEvent>) {
        let (tx, rx) = mpsc::channel();
        let events: Events = Box::new(move |e| {
            let _ = tx.send(e);
        });
        (events, rx)
    }

    fn fake(dir: &Path, sources: Sources, lose: Lose) -> (Recorder, Receiver<RecEvent>) {
        let (events, rx) = collect();
        let rec = Recorder::start_with(opts(dir, sources), events, |s, sink| {
            fake_open(s, sink, lose)
        })
        .expect("start");
        (rec, rx)
    }

    fn ms(d: Duration) -> i64 {
        d.as_millis() as i64
    }

    #[test]
    fn a_session_records_pauses_and_saves_and_leaves_no_thread() {
        let _one = session();
        com();
        let dir = temp_dir("rec-session").join("new folder");
        assert_eq!(rec_threads(), 0, "before start");
        let (mut rec, rx) = fake(&dir, Sources::BOTH, Lose::Nothing);
        assert_eq!(rec_threads(), 2, "while recording");
        let name = rec
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(name.ends_with(" Zoom.m4a"), "{name}");
        assert!(dir.join(format!("{name}.part")).exists());

        std::thread::sleep(Duration::from_millis(500));
        let (mic, system) = rec.levels();
        assert!(mic > 0.3 && system > 0.3, "levels {mic} {system}");
        rec.pause();
        assert!(rec.paused());
        std::thread::sleep(Duration::from_millis(300));
        let held = rec.elapsed();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(rec.elapsed(), held, "the clock stands still while paused");
        let (started, paused_total, paused_since) = rec.clock();
        assert!(paused_since.is_some());
        assert_eq!(
            paused_since.unwrap().duration_since(started) - paused_total,
            held,
            "the clock the island draws is the recorder's"
        );
        rec.resume();
        assert!(!rec.paused());
        std::thread::sleep(Duration::from_millis(500));
        let elapsed = rec.elapsed();
        let result = rec.stop().expect("stop");
        assert_eq!(rec_threads(), 0, "after stop");

        assert!(
            (ms(result.duration) - ms(elapsed)).abs() <= 80,
            "{result:?} vs {elapsed:?}"
        );
        // About a second; a busy machine oversleeps.
        assert!((950..1500).contains(&ms(result.duration)), "{result:?}");
        assert_eq!(
            rx.try_iter().collect::<Vec<_>>(),
            vec![RecEvent::Saved {
                path: result.path.clone(),
                duration: result.duration
            }]
        );
        assert!(result.path.exists());
        assert!(!dir.join(format!("{name}.part")).exists());

        let stem = name.trim_end_matches(".m4a");
        let json = std::fs::read(dir.join(format!("{stem}.vale.json"))).expect("sidecar");
        let v: serde_json::Value = serde_json::from_slice(&json).unwrap();
        let s = Sidecar::from_value(&v);
        assert_eq!((s.kind.as_str(), s.title.as_str()), ("audio", "Zoom"));
        assert_eq!(s.channels, vec!["mic", "system"]);
        assert_eq!(s.app.as_deref(), Some("Zoom.exe"));
        assert_eq!(s.duration_ms, result.duration.as_millis() as u64);
        assert!(v["started"].as_str().unwrap().len() >= 25, "RFC 3339: {v}");

        let dec = encode::tests::decode(&result.path);
        assert_eq!((dec.channels, dec.rate), (2, RATE));
        let decoded = dec.duration();
        assert!(
            (ms(decoded) - ms(result.duration)).abs() <= 100,
            "decoded {decoded:?}"
        );
        for (ch, hz) in [(0, 440.0), (1, 880.0)] {
            let f = dec.frequency(ch);
            assert!((f - hz).abs() < hz * 0.05, "channel {ch}: {f} Hz");
        }
    }

    /// The peak of one channel between `from_ms` and `to_ms` of a decoded file.
    fn peak_between(dec: &encode::tests::Decoded, channel: usize, from_ms: usize, to_ms: usize) -> f32 {
        let ch = dec.channels as usize;
        let per_ms = dec.rate as usize / 1000;
        dec.samples
            .iter()
            .skip(channel)
            .step_by(ch)
            .skip(from_ms * per_ms)
            .take((to_ms - from_ms) * per_ms)
            .fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn a_mute_span_writes_zeros_in_that_channel_only_and_unmute_brings_the_audio_back() {
        let _one = session();
        com();
        let dir = temp_dir("rec-mute");
        let (rec, _rx) = fake(&dir, Sources::BOTH, Lose::Nothing);
        assert_eq!(rec.muted(), [false, false], "a recording starts unmuted");
        std::thread::sleep(Duration::from_millis(700));
        rec.set_muted(Source::Mic, true);
        assert_eq!(rec.muted(), [true, false]);
        std::thread::sleep(Duration::from_millis(100));
        let (mic, system) = rec.levels();
        assert!(mic == 0.0 && system > 0.3, "levels {mic} {system}");
        std::thread::sleep(Duration::from_millis(700));
        rec.set_muted(Source::Mic, false);
        assert_eq!(rec.muted(), [false, false]);
        std::thread::sleep(Duration::from_millis(700));
        let result = rec.stop().expect("stop");

        let dec = encode::tests::decode(&result.path);
        assert_eq!(dec.channels, 2);
        // The span 700..1500 ms is muted; the edges are left out of the
        // checks (the encoder smears a few ms, the packets are 10 ms).
        assert!(peak_between(&dec, 0, 900, 1400) < 0.01, "the mic is silent while muted");
        assert!(peak_between(&dec, 1, 900, 1400) > 0.3, "the other channel goes on");
        assert!(peak_between(&dec, 1, 200, 600) > 0.3);
        assert!(peak_between(&dec, 1, 1600, 2000) > 0.3);
        assert!(peak_between(&dec, 0, 200, 600) > 0.3, "audio before the mute");
        assert!(peak_between(&dec, 0, 1700, 2000) > 0.3, "audio after the unmute");
        assert!(
            (1900..2600).contains(&ms(result.duration)),
            "the mute leaves the duration alone: {result:?}"
        );
    }

    #[test]
    fn one_source_records_mono_and_a_clash_gets_a_counter() {
        let _one = session();
        com();
        let dir = temp_dir("rec-counter");
        let mic = Sources {
            mic: true,
            system: false,
        };
        let mut paths = Vec::new();
        for _ in 0..2 {
            let (rec, _rx) = fake(&dir, mic, Lose::Nothing);
            std::thread::sleep(Duration::from_millis(800));
            paths.push(rec.stop().expect("stop").path);
        }
        let minute = |p: &PathBuf| p.file_name().unwrap().to_string_lossy()[..15].to_string();
        // Unless the minute turned between the two.
        if minute(&paths[0]) == minute(&paths[1]) {
            assert!(
                paths[1].to_string_lossy().ends_with(" Zoom (2).m4a"),
                "{paths:?}"
            );
        }
        assert_eq!(encode::tests::decode(&paths[0]).channels, 1);
    }

    #[test]
    fn a_source_lost_while_the_capture_opens_is_reported_once_it_runs() {
        let _one = session();
        let dir = temp_dir("rec-lost-at-open");
        let (rec, rx) = fake(&dir, Sources::BOTH, Lose::MicAtOpen);
        std::thread::sleep(Duration::from_millis(800));
        rec.stop().expect("stop");
        let got: Vec<_> = rx.try_iter().collect();
        assert_eq!(got.first(), Some(&RecEvent::MicDenied), "{got:?}");
        assert!(matches!(got[1], RecEvent::Saved { .. }), "{got:?}");
        assert_eq!(got.len(), 2, "{got:?}");
    }

    #[test]
    fn losing_every_source_stops_and_saves_by_itself() {
        let _one = session();
        let dir = temp_dir("rec-lost-all");
        let (rec, rx) = fake(&dir, Sources::BOTH, Lose::All);
        // The writer finishes by itself once both are gone.
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !got.iter().any(|e| matches!(e, RecEvent::Saved { .. })) {
            assert!(Instant::now() < deadline, "no save: {got:?}");
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(50)) {
                got.push(e);
            }
        }
        assert_eq!(
            got[..2],
            [RecEvent::SourceLost(Source::Mic), RecEvent::NoInput],
            "the last loss is not a 'keeps going'"
        );
        let saved = rec.stop().expect("the saved recording");
        assert!((550..900).contains(&ms(saved.duration)), "{saved:?}");
        assert!(saved.path.exists());
        assert_eq!(rec_threads(), 0);
    }

    #[test]
    fn a_recording_with_nothing_kept_is_a_quiet_cancel() {
        let _one = session();
        let dir = temp_dir("rec-empty");
        let (mut rec, rx) = fake(&dir, Sources::BOTH, Lose::Nothing);
        rec.pause();
        std::thread::sleep(Duration::from_millis(400));
        let r = rec.stop();
        assert!(r.as_ref().err().is_some_and(RecError::is_empty), "{r:?}");
        assert!(rx.try_iter().next().is_none(), "no notice for nothing");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "no file left");
    }

    #[test]
    fn a_recording_that_heard_nothing_is_not_kept_and_says_so() {
        let _one = session();
        let dir = temp_dir("rec-silent");
        let (rec, rx) = fake(&dir, Sources::BOTH, Lose::Silent);
        std::thread::sleep(Duration::from_millis(900));
        let r = rec.stop();
        assert!(matches!(r, Err(RecError::Silent)), "{r:?}");
        assert!(!r.as_ref().err().is_some_and(RecError::is_empty), "not a quiet cancel");
        assert_eq!(
            rx.try_iter().collect::<Vec<_>>(),
            vec![RecEvent::Failed(Failure::NoAudio)]
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "no file left");
    }

    #[test]
    fn a_rename_that_fails_reports_the_part() {
        let _one = session();
        let dir = temp_dir("rec-rename");
        let (rec, rx) = fake(&dir, Sources::BOTH, Lose::Nothing);
        // A folder took the final name meanwhile.
        std::fs::create_dir(rec.path()).unwrap();
        let part = PathBuf::from(format!("{}.part", rec.path().display()));
        std::thread::sleep(Duration::from_millis(700));
        let r = rec.stop();
        assert!(matches!(r, Err(RecError::Rename { .. })), "{r:?}");
        assert_eq!(
            rx.try_iter().collect::<Vec<_>>(),
            vec![RecEvent::Failed(Failure::Unfinished { part: part.clone() })]
        );
        assert!(part.exists(), "the audio is kept");
    }

    #[test]
    fn a_name_left_by_a_crash_is_not_reused() {
        let _one = session();
        let dir = temp_dir("rec-orphan");
        let stem = naming::file_stem(chrono::Local::now().naive_local(), "Zoom");
        std::fs::write(dir.join(format!("{stem}.m4a.part")), b"x").unwrap();
        let (rec, _rx) = fake(&dir, Sources::BOTH, Lose::Nothing);
        let name = rec.path().file_name().unwrap().to_string_lossy().to_string();
        drop(rec);
        // Unless the minute turned meanwhile.
        if name.starts_with(&stem) {
            assert_eq!(name, format!("{stem} (2).m4a"));
        }
    }

    #[test]
    fn a_capture_that_cannot_start_leaves_no_file_and_no_thread() {
        let _one = session();
        let dir = temp_dir("rec-denied");
        let (events, rx) = collect();
        let r = Recorder::start_with(opts(&dir, Sources::BOTH), events, |_, _| {
            Err(CaptureError::MicDenied)
        });
        assert!(r.as_ref().err().is_some_and(RecError::is_mic_denied));
        assert_eq!(rec_threads(), 0);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "no .part left");
        assert!(rx.try_iter().next().is_none(), "nothing to report");

        let nothing = Sources {
            mic: false,
            system: false,
        };
        let (events, _rx) = collect();
        let r = Recorder::start_with(opts(&dir, nothing), events, |s, sink| {
            fake_open(s, sink, Lose::Nothing)
        });
        assert!(matches!(r, Err(RecError::NoSources)));
    }

    /// The devices a [`Spy`] was asked for, in order.
    type Picked = Arc<Mutex<Vec<(Source, Option<String>)>>>;

    /// A capture that notes the devices asked for.
    struct Spy {
        inner: Box<dyn Feed>,
        picked: Picked,
    }

    impl Feed for Spy {
        fn stop(self: Box<Self>) {
            self.inner.stop();
        }

        fn set_device(&self, source: Source, id: Option<String>) {
            self.picked.lock().unwrap().push((source, id));
        }

        fn device_names(&self) -> [String; 2] {
            self.inner.device_names()
        }
    }

    #[test]
    fn a_picked_device_reaches_the_capture_and_a_switch_is_reported() {
        let _one = session();
        com();
        let dir = temp_dir("rec-switch");
        let (events, rx) = collect();
        let picked = Arc::new(Mutex::new(Vec::new()));
        let spy = Arc::clone(&picked);
        let rec = Recorder::start_with(opts(&dir, Sources::BOTH), events, move |s, mut sink| {
            // As `Capture` does when the chosen mic is missing at the start.
            sink.switched(Source::Mic, "Speakers mic", true, false);
            let inner = fake_open(s, sink, Lose::Nothing)?;
            Ok(Box::new(Spy { inner, picked: spy }) as Box<dyn Feed>)
        })
        .expect("start");
        assert_eq!(rec.device_names(), ["Fake mic", "Fake speakers"]);
        rec.set_device(Source::System, Some("{id}".into()));
        rec.set_device(Source::Mic, None);
        std::thread::sleep(Duration::from_millis(900));
        assert_eq!(
            *picked.lock().unwrap(),
            [(Source::System, Some("{id}".to_string())), (Source::Mic, None)]
        );
        let result = rec.stop().expect("saved");
        assert!(result.duration > Duration::ZERO);
        let events: Vec<RecEvent> = rx.try_iter().collect();
        assert_eq!(
            events.first(),
            Some(&RecEvent::DeviceSwitched {
                source: Source::Mic,
                name: "Speakers mic".into(),
                fell_back: true,
                manual: false,
            })
        );
        assert!(matches!(events.last(), Some(RecEvent::Saved { .. })));
        assert_eq!(rec_threads(), 0);
    }

    #[test]
    fn dropping_a_recorder_still_saves_it() {
        let _one = session();
        let dir = temp_dir("rec-drop");
        let (rec, rx) = fake(&dir, Sources::BOTH, Lose::Nothing);
        std::thread::sleep(Duration::from_millis(800));
        let path = rec.path().to_path_buf();
        drop(rec);
        assert_eq!(rec_threads(), 0);
        assert!(path.exists());
        assert!(matches!(rx.try_iter().last(), Some(RecEvent::Saved { .. })));
    }

    /// Opens the real default mic and output (play something first). Run by
    /// hand: `cargo test --locked records_the_real_devices -- --ignored --nocapture`.
    #[test]
    #[ignore = "opens the real microphone and speakers"]
    fn records_the_real_devices() {
        let _one = session();
        com();
        let dir = temp_dir("rec-real");
        let (events, rx) = collect();
        let rec = Recorder::start(opts(&dir, Sources::BOTH), events).expect("start");
        assert_eq!(rec_threads(), 2);
        let mut peaks = (0.0f32, 0.0f32);
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
            let (m, s) = rec.levels();
            peaks = (peaks.0.max(m), peaks.1.max(s));
        }
        // The clock starts before the devices open, so it is a bit over 3 s.
        let elapsed = rec.elapsed();
        let result = rec.stop().expect("stop");
        assert_eq!(rec_threads(), 0);
        let got: Vec<_> = rx.try_iter().collect();
        eprintln!("{result:?} clock {elapsed:?} peaks {peaks:?} events {got:?}");
        let dec = encode::tests::decode(&result.path);
        assert_eq!(dec.channels, 2);
        let decoded = dec.duration();
        assert!(
            (ms(decoded) - ms(result.duration)).abs() <= 100,
            "decoded {decoded:?}"
        );
        assert!(
            (ms(result.duration) - ms(elapsed)).abs() <= 50,
            "{result:?}"
        );
        assert!(result.duration >= Duration::from_secs(3), "{result:?}");
    }

    // ---- the mix mode ------------------------------------------------------

    /// What a [`Collect`] sink got: `(stamp, samples)` per call.
    type Mixed = Arc<Mutex<Vec<(i64, Vec<f32>)>>>;

    struct Collect(Mixed);

    impl mix::MixSink for Collect {
        fn mix(&mut self, mono: &[f32], qpc_hns: i64) {
            self.0.lock().unwrap().push((qpc_hns, mono.to_vec()));
        }
    }

    /// A capture made up in code: constant `[mic, system]` levels at 48 kHz
    /// in 10 ms packets stamped with the real clock.
    fn level_open(
        levels: [f32; 2],
        mut sink: Box<dyn AudioSink>,
    ) -> Result<Box<dyn Feed>, CaptureError> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("rec-capture".into())
            .spawn(move || {
                let start = qpc_now();
                let mut sent = 0u64;
                while !flag.load(Ordering::SeqCst) {
                    let now = qpc_now();
                    while start + (sent + 1) * 10 * MS <= now {
                        for (source, level) in [Source::Mic, Source::System].into_iter().zip(levels)
                        {
                            sink.audio(source, start + sent * 10 * MS, 48_000, &[level; 480]);
                        }
                        sent += 1;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .expect("fake capture thread");
        Ok(Box::new(FakeFeed {
            stop,
            thread: Some(thread),
        }))
    }

    fn mix_opts() -> mix::MixOptions {
        mix::MixOptions {
            sources: Sources::BOTH,
            devices: [None, None],
            echo_cancel: true,
        }
    }

    fn start_level_mix(levels: [f32; 2]) -> (Recorder, Mixed, Receiver<RecEvent>) {
        let got = Mixed::default();
        let (events, rx) = collect();
        let rec = Recorder::start_mix_with(
            mix_opts(),
            Box::new(Collect(Arc::clone(&got))),
            events,
            |_, sink| level_open(levels, sink),
        )
        .expect("start");
        (rec, got, rx)
    }

    /// Every sample pushed, in order.
    fn flat(got: &Mixed) -> Vec<f32> {
        got.lock().unwrap().iter().flat_map(|(_, m)| m.clone()).collect()
    }

    /// The last 30 ms of what a mix holds are not checked: the stop frame is
    /// taken before the capture is stopped, so the last, incomplete packet
    /// reads as silence (phase 1 ends the same way).
    const TAIL: usize = 1440;

    /// All of `xs[from..to)` within `tol` of `want`.
    fn steady(xs: &[f32], from: usize, to: usize, want: f32, what: &str) {
        for (i, x) in xs[from..to].iter().enumerate() {
            assert!((x - want).abs() < 0.01, "{what}: {x} at {}", from + i);
        }
    }


    /// The Placer and the mix state of a system-only recording, by hand.
    fn mix_rig() -> (Mutex<Placer>, MixOut) {
        let sources = Sources {
            mic: false,
            system: true,
        };
        let out = MixOut {
            clock: mix::MixClock::new(0),
            channels: 1,
            last_tick: None,
            pushed: 0,
        };
        (Mutex::new(Placer::new(sources, ORIGIN)), out)
    }

    fn put_level(placer: &Mutex<Placer>, from_ms: i64, to_ms: i64, level: f32) {
        let samples = vec![level; (frames_ms(to_ms) - frames_ms(from_ms)) as usize];
        lock(placer).put(&Placed {
            channel: 0,
            at: frames_ms(from_ms),
            samples,
        });
    }

    #[test]
    fn audio_before_a_sleep_keeps_its_time_and_the_wake_is_stamped_at_the_wake() {
        let (placer, mut out) = mix_rig();
        let got = Mixed::default();
        let mut sink = Collect(Arc::clone(&got));
        let hns = |ms: i64| ms * 10_000;
        put_level(&placer, 0, 1000, 0.5);
        drain_mix(&placer, &mut out, frames_ms(1000), LAG_FRAMES, &mut sink);
        let n = flat(&got).len();
        assert_eq!(n as i64, frames_ms(800), "the first tick leaves 200 ms back");
        // The PC sleeps for an hour; the next tick finds the gap. The 200 ms
        // held back before the sleep go out with their own stamps, nothing
        // is pushed for the sleep itself.
        let wake = 1000 + 3_600_000;
        drain_mix(&placer, &mut out, frames_ms(wake), LAG_FRAMES, &mut sink);
        let chunks = got.lock().unwrap().clone();
        let before: Vec<_> = chunks.iter().skip(1).collect();
        assert_eq!(before.iter().map(|c| c.1.len()).sum::<usize>() as i64, frames_ms(200));
        assert_eq!(before[0].0, hns(800), "starts where the first tick stopped");
        // Audio after the wake lands at the wake's time.
        put_level(&placer, wake, wake + 500, 0.25);
        drain_mix(&placer, &mut out, frames_ms(wake + 500), LAG_FRAMES, &mut sink);
        let chunks = got.lock().unwrap().clone();
        let after = chunks.last().expect("a chunk after the wake");
        assert_eq!(after.0, hns(wake), "first sample after the wake");
        assert_eq!(after.1.len() as i64, frames_ms(300));
        assert!(after.1.iter().all(|&x| (x - 0.25).abs() < 1e-6));
    }
    #[test]
    fn a_mix_recording_pushes_mono_stamped_from_its_origin() {
        let _one = session();
        let before = capture::qpc_hns();
        let (rec, got, _rx) = start_level_mix([0.5, 0.0]);
        let after = capture::qpc_hns();
        std::thread::sleep(Duration::from_millis(700));
        rec.stop().expect("stop");
        assert_eq!(rec_threads(), 0);
        let chunks = got.lock().unwrap().clone();
        assert!(chunks.len() >= 2, "{} chunks", chunks.len());
        assert!(
            (before..=after).contains(&chunks[0].0),
            "first stamp {} outside {before}..{after}",
            chunks[0].0
        );
        for pair in chunks.windows(2) {
            let want = pair[0].0 + pair[0].1.len() as i64 * 10_000_000 / 48_000;
            assert!((pair[1].0 - want).abs() <= 1, "{} vs {want}", pair[1].0);
        }
        let all = flat(&got);
        steady(&all, 480, all.len() - TAIL, 0.5, "mix");
    }

    #[test]
    fn a_muted_source_is_left_out_of_the_mix() {
        let _one = session();
        let (rec, got, _rx) = start_level_mix([0.5, 0.25]);
        rec.set_muted(Source::Mic, true);
        std::thread::sleep(Duration::from_millis(700));
        rec.stop().expect("stop");
        let all = flat(&got);
        assert!(all.len() > 9600, "{} samples", all.len());
        steady(&all, 4800, all.len() - TAIL, 0.25, "muted mic");
    }

    #[test]
    fn a_mix_recording_writes_no_file() {
        let _one = session();
        let (rec, _got, rx) = start_level_mix([0.5, 0.0]);
        std::thread::sleep(Duration::from_millis(600));
        assert!(rec.path().as_os_str().is_empty());
        let result = rec.stop().expect("stop");
        assert!(result.path.as_os_str().is_empty());
        assert!(result.duration > Duration::from_millis(300), "{result:?}");
        assert!(
            !rx.try_iter()
                .any(|e| matches!(e, RecEvent::Saved { .. } | RecEvent::Failed(_))),
            "a mix recording saves nothing and does not say it heard nothing"
        );
    }

    #[test]
    fn a_mix_recorder_ignores_pause() {
        let _one = session();
        let (mut rec, got, _rx) = start_level_mix([0.5, 0.0]);
        std::thread::sleep(Duration::from_millis(300));
        rec.pause();
        assert!(!rec.paused());
        std::thread::sleep(Duration::from_millis(300));
        rec.resume();
        std::thread::sleep(Duration::from_millis(300));
        let elapsed = rec.started.elapsed();
        rec.stop().expect("stop");
        let all = flat(&got);
        // Nothing cut out: the whole span arrives, without a hole.
        let want = elapsed.as_millis() as usize * 48;
        assert!(all.len() + 48 * 40 >= want, "{} of {want} samples", all.len());
        steady(&all, 480, all.len() - TAIL, 0.5, "across the pause");
    }

    #[test]
    fn stop_pushes_the_rest_of_the_timeline() {
        let _one = session();
        let (rec, got, _rx) = start_level_mix([0.5, 0.0]);
        std::thread::sleep(Duration::from_millis(500));
        // Stopped between two ticks: the last 200 ms are still on the
        // timeline, and must be pushed before `stop` returns.
        let elapsed = rec.started.elapsed();
        rec.stop().expect("stop");
        let all = flat(&got);
        let want = elapsed.as_millis() as usize * 48;
        assert!(all.len() + 48 * 40 >= want, "{} of {want} samples", all.len());
        let end = all.len();
        steady(&all, end - 48 * 150, end - 48 * 30, 0.5, "the tail");
    }
}
