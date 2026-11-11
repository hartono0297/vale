//! The caption worker: one thread per caption session, at below-normal
//! priority. It opens its own capture of the PC audio (and the mic, when
//! "Saya" is on), so captions run with or without a recording, puts what it
//! hears into the chunking core (`live::Live`) and runs the speech engine on
//! what the core hands out. What the card shows comes back as `Event`s on
//! the worker thread.
//!
//! The capture thread only resamples and sends (`Feed`); everything else
//! happens here, one engine run at a time. Stopping drops the open chunks
//! and the held lines: nothing is flushed. No caption text is logged.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};

use super::live::{LAG_MS, Live, MAX_BACKLOG_MS, Out, Work};
use crate::recorder::capture::{
    AudioSink, Capture, CaptureError, ChoiceSet, QPC_UNITS_PER_SEC, Source, Sources, qpc_now,
};
use crate::recorder::resample::Resampler;
use crate::recorder::timeline::qpc_to_frame;
use crate::transcribe::chunk::{EnergyVad, FRAME};
use crate::transcribe::decode::RATE;
use crate::transcribe::engine::{Engine, EngineError, Model, Session, VadSession};
use crate::transcribe::output::Speaker;
use crate::transcribe::prefs::TrLang;
use crate::transcribe::runner::{FrameScorer, chunk_segments};

/// How long the worker waits for audio before it looks at the clock again
/// (a silent PC sends no packets, and a sentence still has to end).
const POLL: Duration = Duration::from_millis(200);
/// A packet stamped this close to the end of the one before continues it
/// (20 ms): device stamps jitter, and a hole of a sample or two would click.
const JOIN_FRAMES: i64 = RATE as i64 / 50;

/// One caption session to run.
#[derive(Debug, Clone)]
pub struct Spec {
    pub engine_dir: PathBuf,
    pub model_path: PathBuf,
    /// The Silero VAD model, or `None` for the energy detector.
    pub vad_path: Option<PathBuf>,
    /// Allow the GPU (Vulkan) when there is one.
    pub gpu: bool,
    pub threads: i32,
    /// Also caption the microphone, as "Saya".
    pub mic: bool,
    /// The endpoint of each source, `[mic, system]`; `None` follows Windows.
    pub devices: [Option<String>; 2],
    /// The language, as `lang_to_u8`; read again for every chunk, so the
    /// card can change it while the session runs.
    pub language: Arc<AtomicU8>,
}

/// A language as the value of `Spec::language`.
pub fn lang_to_u8(lang: TrLang) -> u8 {
    match lang {
        TrLang::Id => 0,
        TrLang::En => 1,
        TrLang::Auto => 2,
    }
}

/// The language of a `Spec::language` value (an unknown one: the default).
pub fn lang_from_u8(value: u8) -> TrLang {
    match value {
        1 => TrLang::En,
        2 => TrLang::Auto,
        _ => TrLang::Id,
    }
}

/// What a session tells its owner, on the worker thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The model is loaded and the capture runs: where the model runs. Comes
    /// again when a failed GPU made the session start over on the CPU.
    Started {
        backend: String,
    },
    Out(Out),
    /// A source stopped for good; the session goes on with the other.
    SourceLost(Source),
    /// The session cannot run or go on: the kind of failure ("engine",
    /// "model", "vad", "capture", "mic-denied"), never any caption text.
    Failed(String),
    /// The last event, always: the capture is closed and the model freed.
    Ended,
}

/// A caption session on its worker thread. Dropping it stops it like
/// [`Running::cancel`]: without waiting.
pub struct Running {
    cancel: Arc<AtomicBool>,
    wake: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    /// Stops the session without waiting for it, for a caller that must not
    /// block (the UI thread): a model that is loading cannot be interrupted
    /// and takes seconds. The thread ends on its own when it notices, closes
    /// its capture and frees the model there; events it still sends up to
    /// `Ended` are an old session's. Open chunks and held lines are dropped.
    pub fn cancel(self) {
        drop(self);
    }

    /// `cancel`, then waits for the thread: `Ended` was sent when this
    /// returns. Blocks as long as a model load or the engine call in
    /// progress takes to return.
    pub fn stop(mut self) {
        self.signal();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    fn signal(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.wake.send(Msg::Wake);
    }

    /// The thread has ended (after `Ended`).
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // Not joined: the thread is left to end on its own.
        self.signal();
    }
}

/// Starts a caption session on a new below-normal thread; `on_event` runs on
/// that thread and gets `Ended` last.
pub fn spawn(
    spec: Spec,
    mut on_event: impl FnMut(Event) + Send + 'static,
) -> std::io::Result<Running> {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let (tx, rx) = mpsc::channel();
    let wake = tx.clone();
    let thread = std::thread::Builder::new()
        .name("caption-worker".into())
        .spawn(move || {
            unsafe {
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
            }
            let failed = match run(&spec, &tx, &rx, &flag, &mut on_event) {
                Err(End::Failed(kind)) => Some(kind),
                // On the CPU there is no backend left to fall back from.
                Err(End::Backend) => Some("engine"),
                Err(End::Cancelled) | Ok(()) => None,
            };
            if let Some(kind) = failed {
                on_event(Event::Failed(kind.into()));
            }
            on_event(Event::Ended);
        })?;
    Ok(Running {
        cancel,
        wake,
        thread: Some(thread),
    })
}

/// The sources captured, in channel order, and who speaks on each.
fn layout(mic: bool) -> (Vec<Source>, Vec<Option<Speaker>>) {
    if mic {
        (
            vec![Source::Mic, Source::System],
            vec![Some(Speaker::Me), Some(Speaker::Others)],
        )
    } else {
        (vec![Source::System], vec![None])
    }
}

/// From the capture thread to the worker.
#[derive(Debug)]
enum Msg {
    /// 16 kHz samples of a channel, from `start` (frames of the session).
    Audio {
        channel: usize,
        start: i64,
        samples: Vec<f32>,
    },
    Lost(Source),
    /// Nothing to hear: look at the cancel flag.
    Wake,
}

/// One source's stream at one device rate.
struct Lane {
    rate: u32,
    resampler: Resampler,
    /// The frame after the last block sent.
    next: i64,
}

/// The capture thread's side: each packet resampled to 16 kHz, given its
/// frame in the session by its QPC stamp and sent to the worker.
struct Feed {
    /// The session's start, in `qpc_now` units.
    origin: u64,
    /// The channel of each source (`Source::index`), when it is captioned.
    channels: [Option<usize>; 2],
    lanes: [Option<Lane>; 2],
    tx: mpsc::Sender<Msg>,
}

impl Feed {
    fn new(mic: bool, origin: u64, tx: mpsc::Sender<Msg>) -> Feed {
        let mut channels = [None; 2];
        for (channel, source) in layout(mic).0.into_iter().enumerate() {
            channels[source.index()] = Some(channel);
        }
        Feed {
            origin,
            channels,
            lanes: [None, None],
            tx,
        }
    }
}

impl AudioSink for Feed {
    fn audio(&mut self, source: Source, qpc: u64, rate: u32, mono: &[f32]) {
        let Some(channel) = self.channels[source.index()] else {
            return;
        };
        if rate == 0 || mono.is_empty() {
            return;
        }
        let at = qpc_to_frame(qpc, QPC_UNITS_PER_SEC, self.origin, RATE);
        let slot = &mut self.lanes[source.index()];
        // Another device rate: a new resampler, placed by the stamp again.
        if slot.as_ref().is_none_or(|lane| lane.rate != rate) {
            *slot = Some(Lane {
                rate,
                resampler: Resampler::new(rate, RATE),
                next: at,
            });
        }
        let Some(lane) = slot else { return };
        let samples = lane.resampler.process(mono);
        let start = if (at - lane.next).abs() <= JOIN_FRAMES {
            lane.next
        } else {
            at
        };
        lane.next = start + samples.len() as i64;
        if !samples.is_empty() {
            let _ = self.tx.send(Msg::Audio {
                channel,
                start,
                samples,
            });
        }
    }

    fn lost(&mut self, source: Source, _: CaptureError) {
        let _ = self.tx.send(Msg::Lost(source));
    }
}

/// Why a session ended before `stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Cancelled,
    /// The GPU backend failed: the session starts over on the CPU.
    Backend,
    /// The failure's kind, for `Event::Failed`.
    Failed(&'static str),
}

/// An engine error at a step of kind `kind`. The error's own text (it can
/// hold a path) stays here.
fn end(error: EngineError, kind: &'static str) -> End {
    match error {
        EngineError::Cancelled => End::Cancelled,
        EngineError::Backend => End::Backend,
        _ => End::Failed(kind),
    }
}

fn capture_kind(error: &CaptureError) -> &'static str {
    match error {
        CaptureError::MicDenied => "mic-denied",
        _ => "capture",
    }
}

/// The session on the calling thread. A GPU that fails, at the load or
/// mid-run, is dropped and the session starts over on the CPU
/// (as `runner::run_job`).
fn run(
    spec: &Spec,
    tx: &mpsc::Sender<Msg>,
    rx: &mpsc::Receiver<Msg>,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(Event),
) -> Result<(), End> {
    let engine = Engine::load(&spec.engine_dir, spec.gpu).map_err(|e| end(e, "engine"))?;
    match attempt(spec, engine, !spec.gpu, tx, rx, cancel, on_event) {
        Err(End::Backend) if spec.gpu => {
            // Packets of the capture that just closed are on another clock.
            while rx.try_recv().is_ok() {}
            attempt(spec, engine, true, tx, rx, cancel, on_event)
        }
        other => other,
    }
}

fn attempt(
    spec: &Spec,
    engine: &'static Engine,
    cpu: bool,
    tx: &mpsc::Sender<Msg>,
    rx: &mpsc::Receiver<Msg>,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(Event),
) -> Result<(), End> {
    let model = Model::load(engine, &spec.model_path, cpu).map_err(|e| end(e, "model"))?;
    if !model.is_asr() {
        return Err(End::Failed("model"));
    }
    let vad_model = match &spec.vad_path {
        Some(path) => Some(Model::load(engine, path, true).map_err(|e| end(e, "vad"))?),
        None => None,
    };
    let (_, speakers) = layout(spec.mic);
    let mut scorers: Vec<Box<dyn FrameScorer + '_>> = Vec::new();
    for _ in &speakers {
        match &vad_model {
            Some(vad) => {
                let session = VadSession::new(vad).map_err(|e| end(e, "vad"))?;
                if session.frame_samples() != FRAME {
                    return Err(End::Failed("vad"));
                }
                scorers.push(Box::new(session));
            }
            None => scorers.push(Box::new(EnergyVad::default())),
        }
    }
    let mut session = Session::new(&model, spec.threads).map_err(|e| end(e, "engine"))?;
    if cancel.load(Ordering::Relaxed) {
        return Err(End::Cancelled);
    }

    let origin = qpc_now();
    let feed = Feed::new(spec.mic, origin, tx.clone());
    let choices = Arc::new(Mutex::new(ChoiceSet::new(spec.devices.clone())));
    // Echo cancellation off: captions never change the mic's mode.
    let sources = Sources {
        mic: spec.mic,
        system: true,
    };
    let capture = Capture::start(sources, choices, false, Box::new(feed))
        .map_err(|e| End::Failed(capture_kind(&e)))?;
    on_event(Event::Started {
        backend: model.backend(),
    });

    let mut live = Live::new(speakers.clone());
    // After engine work the next round does not wait: more may be due.
    let mut idle = true;
    // How long the engine ran since the last round, in frames of awake time.
    let mut ran = 0;
    let result = loop {
        let mut msgs = Vec::new();
        if idle {
            match rx.recv_timeout(POLL) {
                Ok(msg) => msgs.push(msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break Ok(()),
            }
        }
        msgs.extend(rx.try_iter());
        if cancel.load(Ordering::Relaxed) {
            break Err(End::Cancelled);
        }
        // One clock for the audio's frames and the core's milliseconds.
        let now_frame = qpc_to_frame(qpc_now(), QPC_UNITS_PER_SEC, origin, RATE).max(0);
        let now_ms = now_frame as u64 * 1000 / RATE as u64;
        let mut lost = Vec::new();
        let caught = catch_up(&mut live, &mut scorers, now_frame, ran, msgs, &mut |s| {
            lost.push(s)
        });
        ran = 0;
        if let Err(e) = caught {
            break Err(end(e, "vad"));
        }
        lost.into_iter()
            .for_each(|s| on_event(Event::SourceLost(s)));
        let work = live.next_work(now_ms);
        emit(live.take_out(), on_event);
        idle = work.is_none();
        let (chunk, last) = match &work {
            Some(Work::Final(chunk)) => (chunk, true),
            Some(Work::Interim(chunk)) => (chunk, false),
            None => continue,
        };
        let language = lang_from_u8(spec.language.load(Ordering::Relaxed)).code();
        let before = awake_now();
        let heard = match session.run(&chunk.samples, language, cancel) {
            Ok(heard) => heard,
            Err(e) => break Err(end(e, "engine")),
        };
        ran = awake_frames(before, awake_now());
        let segments = chunk_segments(chunk, &heard, speakers[chunk.channel]);
        if last {
            emit(live.final_done(chunk, segments), on_event);
        } else {
            let words: Vec<String> = segments.into_iter().map(|s| s.text).collect();
            emit(live.interim_done(chunk, words.join(" ")), on_event);
        }
    };
    capture.stop();
    result
}

/// The time this PC was awake, in 100 ns units: unlike the performance
/// counter it stands still through a sleep, so an engine run that a sleep
/// interrupted does not look long.
fn awake_now() -> u64 {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryUnbiasedInterruptTime(time: *mut u64) -> i32;
    }
    let mut time = 0u64;
    // SAFETY: writes one u64 through a valid pointer.
    unsafe {
        QueryUnbiasedInterruptTime(&mut time);
    }
    time
}

/// The 16 kHz frames between two `awake_now` readings.
fn awake_frames(before: u64, after: u64) -> i64 {
    (u128::from(after.saturating_sub(before)) * u128::from(RATE) / u128::from(QPC_UNITS_PER_SEC))
        as i64
}

fn emit(out: Vec<Out>, on_event: &mut dyn FnMut(Event)) {
    out.into_iter().for_each(|o| on_event(Event::Out(o)));
}

/// Brings `live` up to the session clock `now_frame` with what the capture
/// sent since the last round, and tells `lost` the sources that stopped.
///
/// A clock more than `MAX_BACKLOG_MS` ahead of what was read (the PC slept,
/// the engine took that long) skips ahead: the audio of that time is not
/// worth showing late. `ran` is how long the engine ran since the last round
/// (frames of awake time): when that is most of the way behind, the model is
/// too heavy and the skip carries the slow hint; a sleep does not. Otherwise
/// the audio is read up to each packet's start before the packet is placed,
/// as if it had just arrived: the timeline holds only 10 s ahead of its read
/// point, and an engine run or a silent stretch can be longer.
fn catch_up(
    live: &mut Live,
    scorers: &mut [Box<dyn FrameScorer + '_>],
    now_frame: i64,
    ran: i64,
    msgs: Vec<Msg>,
    lost: &mut dyn FnMut(Source),
) -> Result<(), EngineError> {
    let lag = (LAG_MS * RATE as u64 / 1000) as i64;
    let max_behind = (MAX_BACKLOG_MS * RATE as u64 / 1000) as i64;
    let behind = now_frame - live.taken_frames();
    if behind > max_behind {
        live.skip_to(now_frame - lag, 2 * ran >= behind);
    }
    for msg in msgs {
        match msg {
            Msg::Audio {
                channel,
                start,
                samples,
            } => {
                while live.advance(start.min(now_frame), scorers)? {}
                live.put(channel, start, &samples);
            }
            Msg::Lost(source) => lost(source),
            Msg::Wake => {}
        }
    }
    while live.advance(now_frame, scorers)? {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::captions::live::Work;

    const ORIGIN: u64 = 5_000_000_000;

    /// The qpc `ms` after the session started.
    fn at(ms: u64) -> u64 {
        ORIGIN + ms * QPC_UNITS_PER_SEC / 1000
    }

    fn feed(mic: bool) -> (Feed, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        (Feed::new(mic, ORIGIN, tx), rx)
    }

    fn audio(rx: &mpsc::Receiver<Msg>) -> (usize, i64, Vec<f32>) {
        match rx.try_recv() {
            Ok(Msg::Audio {
                channel,
                start,
                samples,
            }) => (channel, start, samples),
            other => panic!("no audio: {other:?}"),
        }
    }

    fn tone(rate: u32, ms: u64) -> Vec<f32> {
        (0..rate as u64 * ms / 1000)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 300.0 * i as f32 / rate as f32).sin())
            .collect()
    }

    fn frames(ms: u64) -> i64 {
        (ms * RATE as u64 / 1000) as i64
    }

    #[test]
    fn packets_are_placed_by_qpc() {
        let (mut feed, rx) = feed(false);
        feed.audio(Source::System, at(0), 16_000, &tone(16_000, 10));
        feed.audio(Source::System, at(1_000), 16_000, &tone(16_000, 10));
        let (channel, start, samples) = audio(&rx);
        assert_eq!((channel, start, samples.len()), (0, 0, 160));
        assert_eq!(audio(&rx).1, 16_000);
    }

    #[test]
    fn a_rate_change_rebuilds_the_resampler() {
        let (mut feed, rx) = feed(false);
        feed.audio(Source::System, at(0), 48_000, &tone(48_000, 10));
        let (_, start, samples) = audio(&rx);
        assert_eq!(start, 0);
        assert!((158..=160).contains(&samples.len()), "{}", samples.len());
        // The device changed: the same 10 ms is 441 frames now.
        feed.audio(Source::System, at(10), 44_100, &tone(44_100, 10));
        let (_, start, samples) = audio(&rx);
        assert_eq!(start, 160, "by the qpc, not 441 frames at the first rate");
        let want = 441 * 16_000 / 44_100;
        assert!(
            (want - 2..=want).contains(&samples.len()),
            "{}",
            samples.len()
        );
    }

    #[test]
    fn packets_one_after_another_are_laid_end_to_end() {
        let (mut feed, rx) = feed(false);
        let mut next = None;
        for k in 0..50u64 {
            // Stamps a little off, as a device's are.
            let jitter = if k % 2 == 0 { 3 } else { 0 };
            feed.audio(
                Source::System,
                at(k * 10) + jitter * 1_000,
                48_000,
                &tone(48_000, 10),
            );
            let (_, start, samples) = audio(&rx);
            if let Some(next) = next {
                assert_eq!(start, next, "packet {k}: no hole and no overlap");
            }
            next = Some(start + samples.len() as i64);
        }
        // From the first stamp (0.3 ms in) to half a second of audio.
        assert!((next.unwrap() - 5 - 8_000).abs() <= 2, "{next:?}");
    }

    #[test]
    fn the_mic_is_its_own_channel_and_an_unused_source_is_dropped() {
        let (mut both, rx) = feed(true);
        both.audio(Source::System, at(0), 16_000, &tone(16_000, 10));
        both.audio(Source::Mic, at(0), 16_000, &tone(16_000, 10));
        assert_eq!(audio(&rx).0, 1, "Peserta");
        assert_eq!(audio(&rx).0, 0, "Saya");
        let (mut pc, rx) = feed(false);
        pc.audio(Source::Mic, at(0), 16_000, &tone(16_000, 10));
        pc.audio(Source::System, at(0), 0, &[0.0; 4]);
        pc.audio(Source::System, at(0), 16_000, &[]);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_lost_source_is_told() {
        let (mut feed, rx) = feed(true);
        feed.lost(Source::Mic, CaptureError::MicDenied);
        assert!(matches!(rx.try_recv(), Ok(Msg::Lost(Source::Mic))));
    }

    #[test]
    fn lang_atomic_round_trips() {
        for lang in [TrLang::Id, TrLang::En, TrLang::Auto] {
            let cell = AtomicU8::new(lang_to_u8(lang));
            assert_eq!(lang_from_u8(cell.load(Ordering::Relaxed)), lang);
        }
        assert_eq!(
            (
                lang_to_u8(TrLang::Id),
                lang_to_u8(TrLang::En),
                lang_to_u8(TrLang::Auto)
            ),
            (0, 1, 2)
        );
        assert_eq!(
            lang_from_u8(9),
            TrLang::Id,
            "an unknown value is the default"
        );
    }

    #[test]
    fn channels_follow_the_mic_switch() {
        assert_eq!(layout(false), (vec![Source::System], vec![None]));
        assert_eq!(
            layout(true),
            (
                vec![Source::Mic, Source::System],
                vec![Some(Speaker::Me), Some(Speaker::Others)]
            )
        );
    }

    #[test]
    fn failures_have_a_kind_and_no_text() {
        assert_eq!(capture_kind(&CaptureError::MicDenied), "mic-denied");
        assert_eq!(
            capture_kind(&CaptureError::NoDevice(Source::System)),
            "capture"
        );
        assert_eq!(
            end(EngineError::Model("C:/Users/x/model.gguf".into()), "model"),
            End::Failed("model")
        );
        assert_eq!(end(EngineError::Backend, "model"), End::Backend);
        assert_eq!(end(EngineError::Cancelled, "engine"), End::Cancelled);
    }

    fn scorers() -> Vec<Box<dyn FrameScorer>> {
        vec![Box::new(EnergyVad::default())]
    }

    /// `ms` of a tone from the session start, as 10 ms packets.
    fn talk(ms: u64) -> Vec<Msg> {
        tone(RATE, ms)
            .chunks(160)
            .enumerate()
            .map(|(k, p)| Msg::Audio {
                channel: 0,
                start: k as i64 * 160,
                samples: p.to_vec(),
            })
            .collect()
    }

    #[test]
    fn audio_that_waited_for_the_engine_is_all_heard() {
        // The engine ran for 12 s: more waits than the timeline holds ahead.
        let mut live = Live::new(vec![None]);
        let now = frames(12_000);
        catch_up(&mut live, &mut scorers(), now, 0, talk(12_000), &mut |_| {}).unwrap();
        assert_eq!(live.taken_frames(), now - frames(LAG_MS));
        let Some(Work::Final(first)) = live.next_work(12_000) else {
            panic!("no final")
        };
        assert_eq!(live.take_out(), vec![], "nothing was skipped");
        let Some(Work::Interim(rest)) = live.next_work(12_000) else {
            panic!("no open chunk")
        };
        let heard = first.samples.len() + rest.samples.len();
        assert!(heard as i64 >= frames(11_500), "{heard}");
        let holes = rest.samples.iter().filter(|v| **v == 0.0).count();
        assert!(
            holes < 200,
            "{holes} samples of the last seconds were dropped"
        );
    }

    #[test]
    fn a_stalled_clock_skips_ahead_instead_of_replaying() {
        let mut live = Live::new(vec![None]);
        let mut scorers = scorers();
        catch_up(
            &mut live,
            &mut scorers,
            frames(2_000),
            0,
            talk(2_000),
            &mut |_| {},
        )
        .unwrap();
        // The PC slept for a minute.
        let now = frames(62_000);
        catch_up(&mut live, &mut scorers, now, 0, Vec::new(), &mut |_| {}).unwrap();
        assert!(now - live.taken_frames() <= frames(LAG_MS) + 512);
        assert_eq!(live.next_work(62_000), None);
        assert_eq!(
            live.take_out(),
            vec![Out::ClearInterim { channel: 0 }, Out::Gap]
        );
    }

    #[test]
    fn a_skip_after_a_long_engine_run_says_the_model_is_slow_once() {
        let mut live = Live::new(vec![None]);
        let mut scorers = scorers();
        // The engine ran for 20 s and the clock moved by as much.
        let now = frames(20_500);
        catch_up(
            &mut live,
            &mut scorers,
            now,
            frames(20_000),
            talk(2_000),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            live.next_work(20_500),
            None,
            "the old audio is not replayed"
        );
        assert_eq!(live.take_out(), vec![Out::Gap, Out::Slow]);
        let now = frames(41_000);
        catch_up(
            &mut live,
            &mut scorers,
            now,
            frames(20_000),
            Vec::new(),
            &mut |_| {},
        )
        .unwrap();
        live.next_work(41_000);
        assert_eq!(live.take_out(), vec![Out::Gap], "once in a session");
    }

    #[test]
    fn a_sleep_during_a_short_engine_run_is_not_the_models_fault() {
        let mut live = Live::new(vec![None]);
        // The run took a second of awake time; the clock ran on for a minute.
        catch_up(
            &mut live,
            &mut scorers(),
            frames(60_000),
            frames(1_000),
            Vec::new(),
            &mut |_| {},
        )
        .unwrap();
        live.next_work(60_000);
        assert_eq!(live.take_out(), vec![Out::Gap]);
    }

    #[test]
    fn a_packet_far_ahead_of_what_was_read_is_still_heard() {
        // 12 s of silence went by unread (under the skip), then one packet.
        let mut live = Live::new(vec![None]);
        let packet = Msg::Audio {
            channel: 0,
            start: frames(12_000),
            samples: tone(RATE, 1_500),
        };
        catch_up(
            &mut live,
            &mut scorers(),
            frames(14_000),
            0,
            vec![packet],
            &mut |_| {},
        )
        .unwrap();
        let Some(Work::Interim(open)) = live.next_work(14_000) else {
            panic!("the packet was dropped")
        };
        assert!(
            (11_700..=12_100).contains(&open.start_ms),
            "{}",
            open.start_ms
        );
        assert!(
            open.samples.len() as i64 >= frames(1_400),
            "{}",
            open.samples.len()
        );
    }

    #[test]
    fn the_awake_clock_counts_in_100_ns() {
        let before = awake_now();
        std::thread::sleep(Duration::from_millis(30));
        let passed = awake_now() - before;
        assert!((200_000..20_000_000).contains(&passed), "{passed}");
        assert_eq!(awake_frames(10_000_000, 30_000_000), 32_000);
        assert_eq!(awake_frames(30_000_000, 10_000_000), 0);
    }

    #[test]
    fn cancel_does_not_wait_for_a_worker_that_is_busy() {
        // A worker stuck in a model load: it ends only when the test lets it.
        let (release, blocked) = mpsc::channel::<()>();
        let (done_tx, done) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (wake, woken) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = blocked.recv();
            let _ = done_tx.send(());
        });
        let running = Running {
            cancel: cancel.clone(),
            wake,
            thread: Some(thread),
        };
        assert!(!running.is_finished());
        running.cancel();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(matches!(woken.try_recv(), Ok(Msg::Wake)));
        assert!(
            done.try_recv().is_err(),
            "the worker is still busy: nobody waited for it"
        );
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn dropping_does_not_wait_and_stop_does() {
        let (release, blocked) = mpsc::channel::<()>();
        let cancel = Arc::new(AtomicBool::new(false));
        let (wake, _woken) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = blocked.recv();
        });
        drop(Running {
            cancel: cancel.clone(),
            wake,
            thread: Some(thread),
        });
        assert!(
            cancel.load(Ordering::Relaxed),
            "a drop still stops the session"
        );
        drop(release);

        // `stop` returns only after the thread ended.
        let ended = Arc::new(AtomicBool::new(false));
        let flag = ended.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let seen = cancel.clone();
        let (wake, woken) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = woken.recv();
            assert!(seen.load(Ordering::Relaxed));
            std::thread::sleep(Duration::from_millis(50));
            flag.store(true, Ordering::Relaxed);
        });
        Running {
            cancel,
            wake,
            thread: Some(thread),
        }
        .stop();
        assert!(ended.load(Ordering::Relaxed));
    }

    #[test]
    fn a_lost_marker_reaches_the_caller_in_order() {
        let mut live = Live::new(vec![None]);
        let mut lost = Vec::new();
        let msgs = vec![Msg::Wake, Msg::Lost(Source::System)];
        catch_up(&mut live, &mut scorers(), 0, 0, msgs, &mut |s| lost.push(s)).unwrap();
        assert_eq!(lost, vec![Source::System]);
    }
}
