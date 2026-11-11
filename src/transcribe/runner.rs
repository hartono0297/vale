//! One transcription, start to end, on a worker thread at below-normal
//! priority: decode the audio, cut every channel into speech chunks, run
//! each chunk through the model as soon as it is cut, merge the channels in
//! time order and write `.md`, `.srt` and the sidecar's `"transcript"`.
//!
//! The pipeline (`transcribe`) works on traits so the tests drive it with
//! made-up audio and a fake recognizer; `run_job` plugs in the real decoder
//! and engine. Memory stays small for long meetings: audio is decoded about
//! a second at a time and only the open chunk of each channel is kept.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};

use super::catalog::{ENGINE, Model as CatalogModel};
use super::chunk::{Chunk, ChunkParams, Chunker, EnergyVad, FRAME, Framer};
use super::decode::{DecodeError, Decoder, RATE};
use super::echo;
use super::engine::{Engine, EngineError, Model, RunOutput, Session, VadSession};
use super::output::{self, Header, Labels, Segment, Speaker, TranscriptInfo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    Decode(DecodeError),
    Engine(EngineError),
    /// Writing the transcript files failed.
    Io(String),
    Cancelled,
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Decode(e) => e.fmt(f),
            RunError::Engine(e) => e.fmt(f),
            RunError::Io(why) => write!(f, "cannot write the transcript: {why}"),
            RunError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl From<DecodeError> for RunError {
    fn from(e: DecodeError) -> Self {
        RunError::Decode(e)
    }
}

impl From<EngineError> for RunError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::Cancelled => RunError::Cancelled,
            other => RunError::Engine(other),
        }
    }
}

/// Audio, one 16 kHz vector per channel at a time.
pub trait AudioSource {
    fn channels(&self) -> usize;
    fn duration_ms(&self) -> Option<u64>;
    fn next_block(&mut self) -> Result<Option<Vec<Vec<f32>>>, DecodeError>;
}

impl AudioSource for Decoder {
    fn channels(&self) -> usize {
        Decoder::channels(self)
    }

    fn duration_ms(&self) -> Option<u64> {
        Decoder::duration_ms(self)
    }

    fn next_block(&mut self) -> Result<Option<Vec<Vec<f32>>>, DecodeError> {
        Decoder::next_block(self)
    }
}

/// A speech probability per frame.
pub trait FrameScorer {
    fn score(&mut self, frame: &[f32]) -> Result<f32, EngineError>;
}

impl FrameScorer for EnergyVad {
    fn score(&mut self, frame: &[f32]) -> Result<f32, EngineError> {
        Ok(self.prob(frame))
    }
}

impl FrameScorer for VadSession<'_> {
    fn score(&mut self, frame: &[f32]) -> Result<f32, EngineError> {
        // One frame in, one probability out (the frame sizes match).
        Ok(self.feed(frame)?.last().copied().unwrap_or(0.0))
    }
}

/// Speech to text for one chunk.
pub trait Recognizer {
    fn recognize(&mut self, pcm: &[f32], cancel: &AtomicBool) -> Result<RunOutput, EngineError>;
}

struct EngineRecognizer<'m> {
    session: Session<'m>,
    language: Option<String>,
}

impl Recognizer for EngineRecognizer<'_> {
    fn recognize(&mut self, pcm: &[f32], cancel: &AtomicBool) -> Result<RunOutput, EngineError> {
        self.session.run(pcm, self.language.as_deref(), cancel)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The model is loaded: where it runs, and which voice detector cuts.
    Started { backend: String, vad: &'static str },
    /// Audio read so far, out of the whole (when the file says).
    /// `others_settled_ms`: no "Peserta" segment can still start before it;
    /// `u64::MAX` means fully read or no "Peserta" channel.
    Progress { done_ms: u64, total_ms: Option<u64>, others_settled_ms: u64 },
    /// A segment, as soon as its chunk is done (not yet in time order).
    Segment(Segment),
    Finished(Result<Report, RunError>),
}

/// What a finished transcription produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// Both channels, in time order, without the mic's echo of the meeting.
    pub segments: Vec<Segment>,
    /// The language most chunks were detected as.
    pub language: Option<String>,
    pub duration_ms: u64,
    /// Chunks whose decode stopped early (see `RunOutput::partial`).
    pub partial_chunks: usize,
}

/// The pipeline: decode, chunk, recognize, merge, drop the mic's echo. `scorers` and `speakers`
/// have one entry per channel.
pub fn transcribe(
    source: &mut dyn AudioSource,
    scorers: &mut [Box<dyn FrameScorer + '_>],
    speakers: &[Option<Speaker>],
    params: ChunkParams,
    recognizer: &mut dyn Recognizer,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(Event),
) -> Result<Transcript, RunError> {
    let channels = source.channels();
    assert_eq!(scorers.len(), channels, "one scorer per channel");
    let total_ms = source.duration_ms();
    let mut framers: Vec<Framer> = (0..channels).map(|_| Framer::default()).collect();
    let mut chunkers: Vec<Chunker> = (0..channels).map(|c| Chunker::new(c, params)).collect();
    let mut segments = Vec::new();
    let mut languages: Vec<String> = Vec::new();
    let mut partial_chunks = 0;
    let mut decoded: u64 = 0;
    let mut finished = false;
    let others = speakers.iter().position(|s| *s == Some(Speaker::Others));
    while !finished {
        if cancel.load(Ordering::Relaxed) {
            return Err(RunError::Cancelled);
        }
        let mut ready: Vec<Chunk> = Vec::new();
        let mut failure = None;
        match source.next_block()? {
            Some(block) => {
                decoded += block.first().map_or(0, |b| b.len() as u64);
                for (c, samples) in block.iter().enumerate() {
                    let (scorer, chunker) = (&mut scorers[c], &mut chunkers[c]);
                    framers[c].push(samples, |frame| match scorer.score(frame) {
                        Ok(p) => ready.extend(chunker.push(frame, p)),
                        Err(e) => failure = Some(e),
                    });
                }
            }
            None => {
                finished = true;
                for c in 0..channels {
                    let (scorer, chunker) = (&mut scorers[c], &mut chunkers[c]);
                    framers[c].finish(|frame| match scorer.score(frame) {
                        Ok(p) => ready.extend(chunker.push(frame, p)),
                        Err(e) => failure = Some(e),
                    });
                    ready.extend(chunker.finish());
                }
            }
        }
        if let Some(error) = failure {
            return Err(error.into());
        }
        for chunk in ready {
            let out = recognizer.recognize(&chunk.samples, cancel)?;
            if out.partial {
                partial_chunks += 1;
            }
            languages.extend(out.language.clone());
            let speaker = speakers.get(chunk.channel).copied().flatten();
            for segment in chunk_segments(&chunk, &out, speaker) {
                on_event(Event::Segment(segment.clone()));
                segments.push(segment);
            }
        }
        on_event(Event::Progress {
            done_ms: decoded * 1000 / RATE as u64,
            total_ms,
            others_settled_ms: match others {
                Some(c) if !finished => chunkers[c].settled_ms(),
                _ => u64::MAX,
            },
        });
    }
    Ok(Transcript {
        segments: echo::drop_echo(output::merge(segments)),
        language: most_common(&languages),
        duration_ms: total_ms.unwrap_or(decoded * 1000 / RATE as u64),
        partial_chunks,
    })
}

/// A chunk's result as timed segments of the recording: the model's own
/// segments moved by the chunk's start, or the whole chunk as one segment
/// when the model gives no times.
pub fn chunk_segments(chunk: &Chunk, out: &RunOutput, speaker: Option<Speaker>) -> Vec<Segment> {
    let end = chunk.end_ms();
    let timed: Vec<Segment> = out
        .segments
        .iter()
        .filter(|s| !s.text.trim().is_empty())
        .map(|s| Segment {
            start_ms: (chunk.start_ms + s.start_ms).min(end),
            end_ms: (chunk.start_ms + s.end_ms).min(end),
            speaker,
            text: s.text.trim().to_string(),
        })
        .collect();
    if !timed.is_empty() {
        return timed;
    }
    if out.text.trim().is_empty() {
        return Vec::new();
    }
    vec![Segment {
        start_ms: chunk.start_ms,
        end_ms: end,
        speaker,
        text: out.text.trim().to_string(),
    }]
}

fn most_common(items: &[String]) -> Option<String> {
    let mut best: Option<(&String, usize)> = None;
    for item in items {
        let n = items.iter().filter(|i| *i == item).count();
        if best.is_none_or(|(_, m)| n > m) {
            best = Some((item, n));
        }
    }
    best.map(|(item, _)| item.clone())
}

/// Half the logical processors (1 to 8): the island and the user's work
/// keep the rest.
pub fn default_threads() -> i32 {
    let n = std::thread::available_parallelism().map_or(2, |n| n.get());
    (n / 2).clamp(1, 8) as i32
}

/// One transcription to do.
#[derive(Debug, Clone)]
pub struct Job {
    pub audio: PathBuf,
    pub model_path: PathBuf,
    /// For the Markdown header and the sidecar (`None`: the file name).
    pub model: Option<&'static CatalogModel>,
    pub engine_dir: PathBuf,
    /// The Silero VAD model, or `None` for the energy detector.
    pub vad_path: Option<PathBuf>,
    /// "id", "en", or `None` to detect.
    pub language: Option<String>,
    /// Allow the GPU (Vulkan) when there is one.
    pub gpu: bool,
    pub threads: i32,
    pub labels: Labels,
}

/// Where the transcript went and how it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub md: PathBuf,
    pub srt: PathBuf,
    /// Set when the recording had a sidecar to update.
    pub sidecar: Option<PathBuf>,
    pub segments: usize,
    pub language: Option<String>,
    pub backend: String,
    pub elapsed_ms: u64,
    pub partial_chunks: usize,
}

/// A transcription running on its worker thread.
pub struct Running {
    cancel: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Running {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    pub fn join(self) {
        let _ = self.thread.join();
    }
}

/// Runs `job` on a new below-normal thread; `on_event` runs on that thread
/// and gets `Finished` last. The thread ends with the job.
pub fn spawn(job: Job, mut on_event: impl FnMut(Event) + Send + 'static) -> std::io::Result<Running> {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let thread = std::thread::Builder::new()
        .name("transcribe".into())
        .spawn(move || {
            unsafe {
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
            }
            let result = run_job(&job, &flag, &mut on_event);
            on_event(Event::Finished(result));
        })?;
    Ok(Running { cancel, thread })
}

/// The whole job on the calling thread: engine, model, pipeline, files. A
/// GPU that fails mid-run is dropped and the job starts over on the CPU.
pub fn run_job(
    job: &Job,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(Event),
) -> Result<Report, RunError> {
    let started = Instant::now();
    let engine = Engine::load(&job.engine_dir, job.gpu)?;
    let result = match attempt(job, engine, !job.gpu, cancel, on_event) {
        Err(RunError::Engine(EngineError::Backend)) if job.gpu => {
            attempt(job, engine, true, cancel, on_event)
        }
        other => other,
    };
    let (transcript, backend) = result?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    write_files(job, &transcript, backend, elapsed_ms)
}

fn attempt(
    job: &Job,
    engine: &'static Engine,
    cpu: bool,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(Event),
) -> Result<(Transcript, String), RunError> {
    let model = Model::load(engine, &job.model_path, cpu)?;
    if !model.is_asr() {
        return Err(EngineError::Model("not a speech recognition model".into()).into());
    }
    let vad_model = match &job.vad_path {
        Some(path) => Some(Model::load(engine, path, true)?),
        None => None,
    };
    let mut decoder = Decoder::open(&job.audio)?;
    let channels = decoder.channels();
    let sidecar = std::fs::read_to_string(output::paths(&job.audio).sidecar).ok();
    let names = sidecar.as_deref().and_then(output::sidecar_channels);
    let speakers = output::speakers(channels, names.as_deref());

    let mut scorers: Vec<Box<dyn FrameScorer + '_>> = Vec::new();
    for _ in 0..channels {
        match &vad_model {
            Some(vad) => {
                let session = VadSession::new(vad)?;
                if session.frame_samples() != FRAME {
                    return Err(EngineError::Mismatch("VAD frame size".into()).into());
                }
                scorers.push(Box::new(session));
            }
            None => scorers.push(Box::new(EnergyVad::default())),
        }
    }
    let mut params = ChunkParams::default();
    if let Some(max) = model.max_audio_ms() {
        params.max_chunk_ms = params.max_chunk_ms.min(max.saturating_sub(1_000).max(5_000));
    }
    let backend = model.backend();
    on_event(Event::Started {
        backend: backend.clone(),
        vad: if vad_model.is_some() { "silero" } else { "energy" },
    });
    let mut recognizer = EngineRecognizer {
        session: Session::new(&model, job.threads)?,
        language: job.language.clone(),
    };
    let transcript = transcribe(
        &mut decoder,
        &mut scorers,
        &speakers,
        params,
        &mut recognizer,
        cancel,
        on_event,
    )?;
    Ok((transcript, backend))
}

/// Writes `.md`, `.srt` (neither when there are no segments) and, when the
/// recording has one, the sidecar. A user's own `<stem>.md` or `.srt` is
/// never replaced or removed (`output::target_files`).
fn write_files(
    job: &Job,
    transcript: &Transcript,
    backend: String,
    elapsed_ms: u64,
) -> Result<Report, RunError> {
    let io = |e: std::io::Error| RunError::Io(e.to_string());
    let paths = output::paths(&job.audio);
    let sidecar = std::fs::read_to_string(&paths.sidecar).ok();
    let value: Option<serde_json::Value> =
        sidecar.as_deref().and_then(|s| serde_json::from_str(s).ok());
    let files = output::target_files(&job.audio, value.as_ref());
    let stem = job
        .audio
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let model_name = match job.model {
        Some(m) => output::model_label(m),
        None => job
            .model_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    };
    let header = Header {
        title: sidecar.as_deref().and_then(output::sidecar_title).unwrap_or(stem),
        started: sidecar.as_deref().and_then(sidecar_started),
        duration_ms: Some(transcript.duration_ms),
        model: model_name,
    };
    if transcript.segments.is_empty() {
        // Nothing was said: no files, and no stale ones from an earlier run,
        // but only those the sidecar says Vale wrote.
        let old = value.as_ref().and_then(|v| output::named_files(&job.audio, v));
        for old in old.iter().flat_map(|f| [&f.md, &f.srt]) {
            match std::fs::remove_file(old) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(e)),
                _ => {}
            }
        }
    } else {
        output::write_atomic(&files.md, &output::markdown(&header, &transcript.segments, job.labels))
            .map_err(io)?;
        output::write_atomic(&files.srt, &output::srt(&transcript.segments, job.labels))
            .map_err(io)?;
    }
    let mut updated = None;
    if let Some(text) = sidecar {
        let name = |p: &Path| p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let info = TranscriptInfo {
            model: job.model.map_or_else(|| name(&job.model_path), |m| m.key.to_string()),
            engine: ENGINE.version.to_string(),
            language: job.language.clone().or_else(|| transcript.language.clone()),
            created: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            md: name(&files.md),
            srt: name(&files.srt),
            segments: transcript.segments.len(),
            elapsed_ms,
        };
        let new = output::with_transcript(&text, &info).map_err(RunError::Io)?;
        output::write_atomic(&paths.sidecar, &new).map_err(io)?;
        updated = Some(paths.sidecar.clone());
    }
    Ok(Report {
        md: files.md,
        srt: files.srt,
        sidecar: updated,
        segments: transcript.segments.len(),
        language: job.language.clone().or_else(|| transcript.language.clone()),
        backend,
        elapsed_ms,
        partial_chunks: transcript.partial_chunks,
    })
}

/// The sidecar's `"started"` (RFC 3339) as `YYYY-MM-DD HH:MM`.
fn sidecar_started(sidecar: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(sidecar).ok()?;
    let started = value.get("started")?.as_str()?;
    let time = chrono::DateTime::parse_from_rfc3339(started).ok()?;
    Some(time.format("%Y-%m-%d %H:%M").to_string())
}

/// For the debug CLI: what the engine said last.
#[cfg(debug_assertions)]
pub fn engine_log() -> Vec<String> {
    super::engine::take_log()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::engine::RawSegment;

    struct Memory {
        channels: Vec<Vec<f32>>,
        at: usize,
    }

    impl AudioSource for Memory {
        fn channels(&self) -> usize {
            self.channels.len()
        }
        fn duration_ms(&self) -> Option<u64> {
            Some(self.channels[0].len() as u64 * 1000 / RATE as u64)
        }
        fn next_block(&mut self) -> Result<Option<Vec<Vec<f32>>>, DecodeError> {
            let len = self.channels[0].len();
            if self.at >= len {
                return Ok(None);
            }
            let end = (self.at + 7_000).min(len);
            let block = self.channels.iter().map(|c| c[self.at..end].to_vec()).collect();
            self.at = end;
            Ok(Some(block))
        }
    }

    /// Talk (a 200 Hz buzz) in the given seconds, quiet elsewhere.
    fn channel(seconds: f32, talk: &[(f32, f32)]) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                if talk.iter().any(|&(a, b)| t >= a && t < b) {
                    0.2 * (2.0 * std::f32::consts::PI * 200.0 * t).sin()
                } else {
                    0.0005 * ((i * 7919 % 13) as f32 - 6.0) / 6.0
                }
            })
            .collect()
    }

    /// Says how long each chunk was; with `timed`, one segment per second.
    struct Fake {
        timed: bool,
        calls: usize,
    }

    impl Recognizer for Fake {
        fn recognize(&mut self, pcm: &[f32], _: &AtomicBool) -> Result<RunOutput, EngineError> {
            self.calls += 1;
            let ms = pcm.len() as u64 * 1000 / RATE as u64;
            let segments = if self.timed {
                (0..ms / 1000)
                    .map(|s| RawSegment {
                        start_ms: s * 1000,
                        end_ms: s * 1000 + 900,
                        text: format!("second {s}"),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Ok(RunOutput {
                text: format!("{ms} ms"),
                segments,
                language: Some("id".into()),
                partial: false,
            })
        }
    }

    fn run(
        source: &mut Memory,
        recognizer: &mut Fake,
        cancel: &AtomicBool,
        events: &mut Vec<Event>,
    ) -> Result<Transcript, RunError> {
        let mut scorers: Vec<Box<dyn FrameScorer>> = (0..source.channels.len())
            .map(|_| Box::new(EnergyVad::default()) as Box<dyn FrameScorer>)
            .collect();
        let speakers = output::speakers(source.channels.len(), None);
        transcribe(
            source,
            &mut scorers,
            &speakers,
            ChunkParams::default(),
            recognizer,
            cancel,
            &mut |e| events.push(e),
        )
    }

    #[test]
    fn two_channels_become_one_timeline_with_speakers() {
        let mut source = Memory {
            channels: vec![
                channel(10.0, &[(1.0, 3.0), (8.3, 9.3)]),
                channel(10.0, &[(5.0, 6.5)]),
            ],
            at: 0,
        };
        let mut fake = Fake { timed: false, calls: 0 };
        let mut events = Vec::new();
        let transcript = run(&mut source, &mut fake, &AtomicBool::new(false), &mut events).unwrap();
        let who: Vec<(Option<Speaker>, u64)> = transcript
            .segments
            .iter()
            .map(|s| (s.speaker, s.start_ms / 1000))
            .collect();
        assert_eq!(
            who,
            vec![
                (Some(Speaker::Me), 0),
                (Some(Speaker::Others), 4),
                (Some(Speaker::Me), 8),
            ],
            "{:?}",
            transcript.segments
        );
        // Each untimed chunk is one segment spanning the chunk.
        let first = &transcript.segments[0];
        assert!(first.start_ms >= 700 && first.end_ms <= 3_400, "{first:?}");
        assert_eq!(fake.calls, 3);
        assert_eq!(transcript.language.as_deref(), Some("id"));
        assert_eq!(transcript.duration_ms, 10_000);
        // Progress only goes up and ends at the whole file.
        let progress: Vec<u64> = events
            .iter()
            .filter_map(|e| match e {
                Event::Progress { done_ms, .. } => Some(*done_ms),
                _ => None,
            })
            .collect();
        assert!(progress.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(progress.last(), Some(&10_000));
        let segments = events.iter().filter(|e| matches!(e, Event::Segment(_))).count();
        assert_eq!(segments, 3);
    }

    #[test]
    fn a_models_own_times_are_moved_to_the_recording() {
        let mut source = Memory {
            channels: vec![channel(8.0, &[(2.0, 5.0)])],
            at: 0,
        };
        let mut fake = Fake { timed: true, calls: 0 };
        let transcript =
            run(&mut source, &mut fake, &AtomicBool::new(false), &mut Vec::new()).unwrap();
        assert!(transcript.segments.len() >= 3);
        let first = &transcript.segments[0];
        assert_eq!(first.text, "second 0");
        assert_eq!(first.speaker, None, "one channel without a sidecar");
        let chunk_start = first.start_ms;
        assert!((1_700..=2_000).contains(&chunk_start), "{chunk_start}");
        assert_eq!(transcript.segments[1].start_ms, chunk_start + 1_000);
    }

    #[test]
    fn cancel_stops_before_the_next_block() {
        let mut source = Memory {
            channels: vec![channel(5.0, &[(1.0, 2.0)])],
            at: 0,
        };
        let mut fake = Fake { timed: false, calls: 0 };
        let result = run(&mut source, &mut fake, &AtomicBool::new(true), &mut Vec::new());
        assert_eq!(result, Err(RunError::Cancelled));
        assert_eq!(fake.calls, 0);
    }

    #[test]
    fn untimed_and_empty_results() {
        let chunk = Chunk {
            channel: 1,
            start_ms: 60_000,
            samples: vec![0.0; RATE as usize * 3],
        };
        let out = |text: &str, segments: Vec<RawSegment>| RunOutput {
            text: text.into(),
            segments,
            language: None,
            partial: false,
        };
        assert!(chunk_segments(&chunk, &out("  ", vec![]), None).is_empty());
        let one = chunk_segments(&chunk, &out(" Halo semua. ", vec![]), Some(Speaker::Others));
        assert_eq!(one.len(), 1);
        assert_eq!((one[0].start_ms, one[0].end_ms), (60_000, 63_000));
        assert_eq!(one[0].text, "Halo semua.");
        // A model time past the chunk is clamped to its end.
        let late = vec![RawSegment {
            start_ms: 2_500,
            end_ms: 9_000,
            text: "x".into(),
        }];
        let clamped = chunk_segments(&chunk, &out("x", late), None);
        assert_eq!((clamped[0].start_ms, clamped[0].end_ms), (62_500, 63_000));
    }

    #[test]
    fn the_pipeline_drops_echoed_me_segments() {
        let talk = channel(8.0, &[(1.0, 3.0)]);
        let mut source = Memory {
            channels: vec![talk.clone(), talk],
            at: 0,
        };
        let mut fake = Fake { timed: false, calls: 0 };
        let transcript =
            run(&mut source, &mut fake, &AtomicBool::new(false), &mut Vec::new()).unwrap();
        assert_eq!(fake.calls, 2);
        assert_eq!(transcript.segments.len(), 1, "{:?}", transcript.segments);
        assert_eq!(transcript.segments[0].speaker, Some(Speaker::Others));
    }

    fn files_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("transcribe-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn files_job(audio: PathBuf) -> Job {
        Job {
            audio,
            model_path: PathBuf::from("model.gguf"),
            model: None,
            engine_dir: PathBuf::new(),
            vad_path: None,
            language: None,
            gpu: false,
            threads: 1,
            labels: output::LABELS_ID,
        }
    }

    fn result_of(segments: Vec<Segment>) -> Transcript {
        Transcript {
            segments,
            language: None,
            duration_ms: 5_000,
            partial_chunks: 0,
        }
    }

    #[test]
    fn an_empty_result_leaves_no_transcript_files() {
        let dir = files_dir("empty");
        let audio = dir.join("meeting.m4a");
        let paths = output::paths(&audio);
        std::fs::write(&paths.md, "old").unwrap();
        std::fs::write(&paths.srt, "old").unwrap();
        std::fs::write(
            &paths.sidecar,
            r#"{"title": "Weekly", "channels": ["mic", "system"], "transcript": {"md": "meeting.md", "srt": "meeting.srt"}}"#,
        )
        .unwrap();
        let report = write_files(&files_job(audio), &result_of(Vec::new()), "cpu".into(), 10).unwrap();
        assert_eq!(report.segments, 0);
        assert!(!paths.md.exists() && !paths.srt.exists(), "the sidecar says they are Vale's");
        let sidecar: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&paths.sidecar).unwrap()).unwrap();
        assert_eq!(sidecar["transcript"]["segments"], 0);
        assert_eq!(sidecar["title"], "Weekly");
        assert_eq!(sidecar["channels"][1], "system");
    }

    #[test]
    fn a_users_own_md_beside_a_picked_file_is_never_touched() {
        let dir = files_dir("foreign");
        let audio = dir.join("meeting.wav");
        let paths = output::paths(&audio);
        let mine = "# Meeting\n\nmy own notes\n";
        std::fs::write(&paths.md, mine).unwrap();
        // Nothing said: the user's notes stay.
        let empty = write_files(&files_job(audio.clone()), &result_of(Vec::new()), "cpu".into(), 10)
            .unwrap();
        assert_eq!(empty.segments, 0);
        assert_eq!(std::fs::read_to_string(&paths.md).unwrap(), mine);
        assert!(!paths.sidecar.exists());
        // Words: written beside them, and found there again.
        let said = vec![Segment {
            start_ms: 0,
            end_ms: 1_000,
            speaker: None,
            text: "halo".into(),
        }];
        let report = write_files(&files_job(audio.clone()), &result_of(said), "cpu".into(), 10)
            .unwrap();
        assert_eq!(std::fs::read_to_string(&paths.md).unwrap(), mine);
        assert!(!paths.srt.exists());
        assert_eq!(report.md, dir.join("meeting.transcript.md"));
        assert_eq!(report.srt, dir.join("meeting.transcript.srt"));
        assert!(report.md.is_file() && report.srt.is_file());
        let doc = crate::transcribe::doc::load(&audio, "x");
        assert!(doc.has_transcript);
        assert_eq!(doc.md, report.md);
        assert_eq!(doc.segments.len(), 1);
    }

    #[test]
    fn helpers() {
        let langs = ["id", "en", "id"].map(String::from);
        assert_eq!(most_common(&langs).as_deref(), Some("id"));
        assert_eq!(most_common(&[]), None);
        assert!((1..=8).contains(&default_threads()));
        assert_eq!(
            sidecar_started(r#"{"started": "2026-11-07T10:02:33+07:00"}"#).as_deref(),
            Some("2026-11-07 10:02")
        );
        assert_eq!(sidecar_started("{}"), None);
    }

    #[test]
    fn progress_says_how_far_others_is_read() {
        // Me talks 1-3 s; Others talks over it, 1.5-6.5 s, and its chunk closes later.
        let mut source = Memory {
            channels: vec![channel(10.0, &[(1.0, 3.0)]), channel(10.0, &[(1.5, 6.5)])],
            at: 0,
        };
        let mut events = Vec::new();
        run(&mut source, &mut Fake { timed: false, calls: 0 }, &AtomicBool::new(false), &mut events).unwrap();
        let mut settled = 0;
        let mut marks = Vec::new();
        for e in &events {
            match e {
                Event::Progress { others_settled_ms, .. } => {
                    marks.push(*others_settled_ms);
                    settled = *others_settled_ms;
                }
                Event::Segment(s) if s.speaker == Some(Speaker::Others) => {
                    assert!(s.start_ms >= settled, "{s:?} came after the mark {settled}");
                }
                _ => {}
            }
        }
        assert!(marks.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(marks.last(), Some(&u64::MAX));
        // The Me line was sent before Others was read past it.
        let me_at = events.iter().position(|e| matches!(e, Event::Segment(s) if s.speaker == Some(Speaker::Me))).unwrap();
        let first_mark_after = events[me_at..].iter().find_map(|e| match e {
            Event::Progress { others_settled_ms, .. } => Some(*others_settled_ms),
            _ => None,
        });
        assert!(first_mark_after.unwrap() < 4_000, "it is held at first");
    }

    #[test]
    fn one_channel_never_holds() {
        let mut source = Memory { channels: vec![channel(5.0, &[(1.0, 2.0)])], at: 0 };
        let mut events = Vec::new();
        run(&mut source, &mut Fake { timed: false, calls: 0 }, &AtomicBool::new(false), &mut events).unwrap();
        assert!(events.iter().all(|e| !matches!(e, Event::Progress { others_settled_ms, .. } if *others_settled_ms != u64::MAX)));
    }
}
