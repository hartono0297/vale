//! Audio recording: the mic, the system audio (WASAPI loopback) or both to an
//! `.m4a` with a `.vale.json` sidecar. Design:
//! `docs/superpowers/specs/2026-11-07-recording-design.md`; plan:
//! `docs/superpowers/plans/2026-11-07-recording-phase1.md`.
//!
//! A [`Recorder`] is one recording. While it exists two threads run:
//!
//! - `rec-capture` ([`capture::Capture`]): delivers mono packets with their
//!   QPC time to a sink that resamples each source to 48 kHz and places it on
//!   a shared [`timeline::Timeline`] by time ([`Placer`]), and feeds the peak
//!   meters.
//! - `rec-writer`: every 50 ms takes the timeline up to 200 ms before now and
//!   encodes it into `<stem>.m4a.part`; on stop it takes the rest, finalizes,
//!   renames to `<stem>.m4a`, writes `<stem>.vale.json` and reports
//!   [`RecEvent::Saved`].
//!
//! Neither exists before [`Recorder::start`] or after [`Recorder::stop`]
//! returns, and no timer is created: the writer paces itself with a channel
//! receive timeout.

pub mod capture;
pub mod encode;
pub mod level;
pub mod naming;
pub mod prefs;
pub mod resample;
pub mod sidecar;
pub mod timeline;

use std::fmt;
use std::path::{Path, PathBuf};
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
/// A packet more than this many frames (20 ms) away from where its source's
/// stream is expected starts a new run (a loopback silence gap, clock drift,
/// a glitch). Within it, packets are laid end to end, so timestamp jitter
/// never cuts the audio.
const REANCHOR_FRAMES: i64 = RATE as i64 / 50;

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
    Failed(String),
    /// A source stopped for good; its channel stays silent from here.
    SourceLost(Source),
    /// The mic was refused by the Windows privacy setting while the system
    /// audio records; the mic channel stays silent.
    MicDenied,
}

/// Why a recording could not start or finish.
#[derive(Debug)]
pub enum RecError {
    NoSources,
    Capture(CaptureError),
    Encode(EncodeError),
    Io(std::io::Error),
    /// A thread could not start, panicked or ended early.
    Thread(String),
}

impl RecError {
    /// The mic was refused by Windows: the app points at the privacy page.
    pub fn is_mic_denied(&self) -> bool {
        matches!(self, RecError::Capture(CaptureError::MicDenied))
    }
}

impl fmt::Display for RecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSources => f.write_str("no source selected"),
            Self::Capture(e) => write!(f, "{e}"),
            Self::Encode(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "file error: {e}"),
            Self::Thread(s) => f.write_str(s),
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
    /// Output frames placed so far.
    placed: i64,
    /// The resampler's lag in output frames, subtracted when placing.
    delay: i64,
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
        }
    }

    /// Where the next packet should start, in output frames.
    fn expected(&self) -> i64 {
        self.anchor + (i128::from(self.fed) * i128::from(RATE) / i128::from(self.rate)) as i64
    }
}

struct Lane {
    channel: usize,
    run: Option<Run>,
}

/// Turns capture packets into the file's 48 kHz timeline: one channel per
/// source (mic first), each source resampled on its own and placed by its
/// QPC time relative to `origin`. Gaps read as zeros; see
/// [`timeline::Timeline`] for pauses and late packets.
pub struct Placer {
    origin: u64,
    timeline: Timeline,
    lanes: [Option<Lane>; 2],
    meters: [PeakMeter; 2],
}

impl Placer {
    /// `origin` is the recording's start in `qpc_now` units.
    pub fn new(sources: Sources, origin: u64) -> Self {
        let mut lanes = [None, None];
        for (channel, source) in sources.list().into_iter().enumerate() {
            lanes[source_index(source)] = Some(Lane { channel, run: None });
        }
        Self {
            origin,
            timeline: Timeline::new(RATE, sources.list().len()),
            lanes,
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

    /// Places one capture packet. A source that is not recorded is ignored.
    pub fn audio(&mut self, source: Source, qpc: u64, rate: u32, mono: &[f32], now: Instant) {
        let at = self.frame_at(qpc);
        let Some(lane) = self.lanes[source_index(source)].as_mut() else {
            return;
        };
        if rate == 0 || mono.is_empty() {
            return;
        }
        let fresh = lane
            .run
            .as_ref()
            .is_none_or(|r| r.rate != rate || (at - r.expected()).abs() > REANCHOR_FRAMES);
        if fresh {
            lane.run = Some(Run::new(rate, at));
        }
        let run = lane.run.as_mut().expect("a run was just made");
        let out = run.resampler.process(mono);
        let pos = run.anchor - run.delay + run.placed;
        self.timeline.put(lane.channel, pos, &out);
        run.placed += out.len() as i64;
        run.fed += mono.len() as u64;
        self.meters[source_index(source)].feed_at(mono, now);
    }

    /// Everything up to raw frame `frame`, interleaved (see `Timeline::take`).
    pub fn take(&mut self, frame: i64) -> Vec<f32> {
        self.timeline.take(frame)
    }

    pub fn pause_at(&mut self, frame: i64) {
        self.timeline.pause_at(frame);
    }

    pub fn resume_at(&mut self, frame: i64) {
        self.timeline.resume_at(frame);
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
}

impl Feed for Capture {
    fn stop(self: Box<Self>) {
        Capture::stop(*self);
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
}

/// The capture side: places packets and reports lost sources to the writer.
struct Sink {
    placer: Arc<Mutex<Placer>>,
    levels: Arc<SharedLevels>,
    tx: Sender<Msg>,
}

impl AudioSink for Sink {
    fn audio(&mut self, source: Source, qpc: u64, rate: u32, mono: &[f32]) {
        let now = Instant::now();
        let p = &mut *lock(&self.placer);
        p.audio(source, qpc, rate, mono, now);
        p.publish(&self.levels, now);
    }

    fn lost(&mut self, source: Source, err: CaptureError) {
        let denied = matches!(err, CaptureError::MicDenied);
        let _ = self.tx.send(Msg::Lost(source, denied));
    }
}

fn lock(placer: &Mutex<Placer>) -> MutexGuard<'_, Placer> {
    placer.lock().unwrap_or_else(|e| e.into_inner())
}

/// One recording. Dropping it without `stop` stops and saves it all the same.
pub struct Recorder {
    path: PathBuf,
    container: Container,
    placer: Arc<Mutex<Placer>>,
    levels: Arc<SharedLevels>,
    started: Instant,
    paused_since: Option<Instant>,
    paused_total: Duration,
    feed: Option<Box<dyn Feed>>,
    writer: Option<(Sender<Msg>, JoinHandle<Result<RecResult, RecError>>)>,
}

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
        Self::start_with(opts, events, |sources, sink| {
            Capture::start(sources, sink).map(|c| Box::new(c) as Box<dyn Feed>)
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
        std::fs::create_dir_all(&opts.dir).map_err(RecError::Io)?;
        let started_at = chrono::Local::now();
        let stem = naming::file_stem(started_at.naive_local(), &opts.title);
        let path = naming::unique_path(&opts.dir, &stem, "m4a", Path::exists);
        let stem = path
            .file_name()
            .map(|n| n.to_string_lossy().trim_end_matches(".m4a").to_string())
            .unwrap_or(stem);
        let files = Files {
            part: opts.dir.join(format!("{stem}.m4a.part")),
            sidecar: opts.dir.join(format!("{stem}.vale.json")),
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
        let container = match ready_rx.recv() {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                let _ = writer.join();
                return Err(e);
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
            placer: Arc::clone(&placer),
            levels: Arc::clone(&levels),
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
            container,
            placer,
            levels,
            started,
            paused_since: None,
            paused_total: Duration::ZERO,
            feed: Some(feed),
            writer: Some((tx, writer)),
        })
    }

    /// Pauses: the audio from now until `resume` is left out of the file.
    pub fn pause(&mut self) {
        if self.paused_since.is_some() {
            return;
        }
        let p = &mut *lock(&self.placer);
        let frame = p.frame_at(qpc_now());
        p.pause_at(frame);
        self.paused_since = Some(Instant::now());
    }

    pub fn resume(&mut self) {
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

    /// Time recorded so far, pauses excluded.
    pub fn elapsed(&self) -> Duration {
        let end = self.paused_since.unwrap_or_else(Instant::now);
        end.saturating_duration_since(self.started)
            .saturating_sub(self.paused_total)
    }

    /// The input levels `(mic, system)`, 0..=1, decaying over 300 ms; a
    /// source that is not recorded reads 0. Never waits.
    pub fn levels(&self) -> (f32, f32) {
        (self.levels.mic(), self.levels.system())
    }

    /// The final `.m4a` path (the file is `<path>.part` until `stop`).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The MP4 flavour the encoder accepted.
    pub fn container(&self) -> Container {
        self.container
    }

    /// Stops and finalizes; blocks until the file and the sidecar are written
    /// and both threads have ended. The outcome is also sent to `events`.
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
        let _ = ready.send(Err(RecError::Capture(CaptureError::Com(e))));
        return Err(RecError::Thread(
            "COM did not start on the writer thread".into(),
        ));
    }
    let channels = lock(placer).channels() as u16;
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
                written: 0,
            };
            let outcome = w.run(&meta, placer, levels, rx, &*events);
            match &outcome {
                Ok(r) => events(RecEvent::Saved {
                    path: r.path.clone(),
                    duration: r.duration,
                }),
                Err(None) => {}
                Err(Some(e)) => events(RecEvent::Failed(e.to_string())),
            }
            outcome.map_err(|e| {
                e.unwrap_or_else(|| RecError::Thread("the recording did not start".into()))
            })
        }
    };
    unsafe { CoUninitialize() };
    result
}

struct Writing<'a> {
    files: &'a Files,
    writer: Option<M4aWriter>,
    /// Samples handed to the encoder so far.
    written: u64,
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
    ) -> Result<RecResult, Option<RecError>> {
        // Wait for the capture to start (or not).
        loop {
            match rx.recv() {
                Ok(Msg::Go) => break,
                Ok(Msg::Lost(..)) => {}
                Ok(Msg::Abort | Msg::Stop(_)) | Err(_) => {
                    self.discard();
                    return Err(None);
                }
            }
        }
        let mut next = Instant::now() + TICK;
        let stop_frame = loop {
            match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                Ok(Msg::Lost(source, denied)) => events(if denied {
                    RecEvent::MicDenied
                } else {
                    RecEvent::SourceLost(source)
                }),
                Ok(Msg::Stop(frame)) => break frame,
                Ok(Msg::Go | Msg::Abort) => {}
                Err(RecvTimeoutError::Timeout) => {
                    next += TICK;
                    let now = Instant::now();
                    if next < now {
                        next = now + TICK;
                    }
                    let block = {
                        let p = &mut *lock(placer);
                        p.publish(levels, now);
                        let frame = p.frame_at(qpc_now()) - LAG_FRAMES;
                        p.take(frame)
                    };
                    self.write(&block).map_err(Some)?;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    break lock(placer).frame_at(qpc_now());
                }
            }
        };
        let tail = lock(placer).take(stop_frame);
        self.write(&tail).map_err(Some)?;
        let duration = self.finalize().map_err(Some)?;
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

    /// Finishes the file and renames it to its final name.
    fn finalize(&mut self) -> Result<Duration, RecError> {
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
        std::fs::rename(&self.files.part, &self.files.path).map_err(RecError::Io)?;
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

/// `--record-test <seconds> [mic|system|both]` (debug builds): records into
/// `DI_REC_DIR` or `target/rec-test/` and prints the path, the duration, the
/// container, the highest mic and system levels seen and any lost source.
/// Returns the process exit code.
#[cfg(debug_assertions)]
pub fn record_test(args: &[String]) -> i32 {
    let secs: f64 = match args.first().map(|s| s.parse::<f64>()) {
        None => 5.0,
        Some(Ok(s)) if s > 0.0 && s.is_finite() => s,
        Some(_) => {
            eprintln!("usage: vale --record-test <seconds> [mic|system|both]");
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
        RecEvent::Failed(m) => println!("failed: {m}"),
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
mod tests {
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

    /// Feeds `source` 10 ms packets of `value(k)` at `rate` from `from_ms` to
    /// `to_ms`, each stamped at its nominal time plus `jitter(k)` (qpc units).
    fn feed(
        p: &mut Placer,
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
        let mut p = Placer::new(Sources::BOTH, ORIGIN);
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
        let mut p = Placer::new(
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
        let mut p = Placer::new(
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
        let mut p = Placer::new(
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
        let mut p = Placer::new(
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
        let mut p = Placer::new(
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
        let mut p = Placer::new(
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
        let mut p = Placer::new(
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
        let mut p = Placer::new(
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
        let mut p = Placer::new(Sources::BOTH, ORIGIN);
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

    // ---- sessions on a made-up capture -------------------------------------

    /// Sessions run one at a time, so the `rec-*` thread count is theirs.
    static SESSION: Mutex<()> = Mutex::new(());

    fn session() -> MutexGuard<'static, ()> {
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
    }

    /// A capture made up in code, on its own `rec-capture` thread: a 440 Hz
    /// tone on the mic at 44.1 kHz and an 880 Hz tone on the system audio at
    /// 48 kHz, in 10 ms packets stamped with the real clock. With `lose`, it
    /// first reports the mic refused and the system device lost.
    fn fake_open(
        sources: Sources,
        mut sink: Box<dyn AudioSink>,
        lose: bool,
    ) -> Result<Box<dyn Feed>, CaptureError> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("rec-capture".into())
            .spawn(move || {
                if lose {
                    sink.lost(Source::Mic, CaptureError::MicDenied);
                    sink.lost(Source::System, CaptureError::DeviceLost(Source::System));
                }
                let start = qpc_now();
                let tones = [
                    (Source::Mic, 44_100u32, 440.0f64),
                    (Source::System, 48_000, 880.0),
                ];
                let mut sent = [0u64; 2];
                while !flag.load(Ordering::SeqCst) {
                    let now = qpc_now();
                    for (i, &(source, rate, hz)) in tones.iter().enumerate() {
                        if !sources.list().contains(&source) {
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

    fn fake(dir: &Path, sources: Sources, lose: bool) -> (Recorder, Receiver<RecEvent>) {
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
        let (mut rec, rx) = fake(&dir, Sources::BOTH, false);
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
        rec.resume();
        assert!(!rec.paused());
        std::thread::sleep(Duration::from_millis(500));
        let elapsed = rec.elapsed();
        let result = rec.stop().expect("stop");
        assert_eq!(rec_threads(), 0, "after stop");

        assert!(
            (ms(result.duration) - ms(elapsed)).abs() <= 50,
            "{result:?} vs {elapsed:?}"
        );
        assert!((950..1200).contains(&ms(result.duration)), "{result:?}");
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
            let (rec, _rx) = fake(&dir, mic, false);
            std::thread::sleep(Duration::from_millis(150));
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
    fn lost_sources_are_reported() {
        let _one = session();
        let dir = temp_dir("rec-lost");
        let (rec, rx) = fake(&dir, Sources::BOTH, true);
        std::thread::sleep(Duration::from_millis(150));
        rec.stop().expect("stop");
        let got: Vec<_> = rx.try_iter().collect();
        assert_eq!(
            got[..2],
            [RecEvent::MicDenied, RecEvent::SourceLost(Source::System)]
        );
        assert!(matches!(got[2], RecEvent::Saved { .. }), "{got:?}");
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
            fake_open(s, sink, false)
        });
        assert!(matches!(r, Err(RecError::NoSources)));
    }

    #[test]
    fn dropping_a_recorder_still_saves_it() {
        let _one = session();
        let dir = temp_dir("rec-drop");
        let (rec, rx) = fake(&dir, Sources::BOTH, false);
        std::thread::sleep(Duration::from_millis(150));
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
}
