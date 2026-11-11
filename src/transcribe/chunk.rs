//! Cutting one channel's 16 kHz audio into speech chunks.
//!
//! The audio is looked at in 32 ms frames (512 samples, the Silero VAD
//! frame). Each frame gets a speech probability, from the engine's Silero
//! VAD when it is installed or from `EnergyVad` otherwise, and `Chunker`
//! turns the probabilities into chunks: speech with a little padding, ended
//! by a pause, never longer than `max_chunk_ms` (a long stretch is split at
//! its quietest frame). Each chunk keeps its start time, so the transcript
//! has times even from a model that gives none (Qwen3-ASR), and the channel
//! it came from ("Saya" or "Peserta").

use std::collections::VecDeque;

use super::decode::RATE;

/// Samples per frame.
pub const FRAME: usize = 512;
/// Milliseconds per frame.
pub const FRAME_MS: u64 = FRAME as u64 * 1000 / RATE as u64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChunkParams {
    /// A frame at or above this starts speech.
    pub threshold: f32,
    /// Speech goes on while frames stay at or above this.
    pub neg_threshold: f32,
    /// A chunk with less speech than this is dropped (a cough, a click).
    pub min_speech_ms: u64,
    /// A pause this long ends the chunk.
    pub min_silence_ms: u64,
    /// Audio kept before and after the speech.
    pub pad_ms: u64,
    /// No chunk is longer (the Qwen3-ASR input cap is far above it, and
    /// shorter chunks keep the times in the transcript close).
    pub max_chunk_ms: u64,
    /// A chunk at `max_chunk_ms` is split at the quietest frame of its last
    /// this-many milliseconds.
    pub split_window_ms: u64,
}

impl Default for ChunkParams {
    fn default() -> Self {
        ChunkParams {
            threshold: 0.5,
            neg_threshold: 0.35,
            min_speech_ms: 250,
            min_silence_ms: 600,
            pad_ms: 192,
            max_chunk_ms: 30_000,
            split_window_ms: 8_000,
        }
    }
}

impl ChunkParams {
    fn frames(ms: u64) -> usize {
        ms.div_ceil(FRAME_MS) as usize
    }
}

/// A piece of one channel to transcribe.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub channel: usize,
    /// From the start of the recording.
    pub start_ms: u64,
    /// 16 kHz mono, a whole number of frames.
    pub samples: Vec<f32>,
}

impl Chunk {
    pub fn end_ms(&self) -> u64 {
        self.start_ms + self.samples.len() as u64 * 1000 / RATE as u64
    }
}

/// Splits a stream of samples into whole frames; the last one is padded
/// with silence.
#[derive(Debug, Default)]
pub struct Framer {
    pending: Vec<f32>,
}

impl Framer {
    pub fn push(&mut self, samples: &[f32], mut frame: impl FnMut(&[f32])) {
        self.pending.extend_from_slice(samples);
        let whole = self.pending.len() / FRAME * FRAME;
        for f in self.pending[..whole].chunks_exact(FRAME) {
            frame(f);
        }
        self.pending.drain(..whole);
    }

    pub fn finish(&mut self, mut frame: impl FnMut(&[f32])) {
        if !self.pending.is_empty() {
            self.pending.resize(FRAME, 0.0);
            frame(&self.pending);
            self.pending.clear();
        }
    }
}

struct Active {
    /// Frame index (from the start of the channel) of the chunk's first frame.
    first: u64,
    samples: Vec<f32>,
    probs: Vec<f32>,
    /// Frames at the end below `neg_threshold`.
    silence_run: usize,
}

/// Turns per-frame speech probabilities into chunks, for one channel.
pub struct Chunker {
    channel: usize,
    params: ChunkParams,
    /// Frames pushed so far.
    seen: u64,
    /// The last few frames before speech (the leading pad).
    before: VecDeque<(Vec<f32>, f32)>,
    active: Option<Active>,
}

impl Chunker {
    pub fn new(channel: usize, params: ChunkParams) -> Self {
        Chunker {
            channel,
            params,
            seen: 0,
            before: VecDeque::new(),
            active: None,
        }
    }

    /// One frame (`FRAME` samples) and its speech probability; a chunk when
    /// this frame finished one.
    pub fn push(&mut self, frame: &[f32], prob: f32) -> Option<Chunk> {
        debug_assert_eq!(frame.len(), FRAME);
        let p = self.params;
        let index = self.seen;
        self.seen += 1;
        let Some(active) = self.active.as_mut() else {
            if prob >= p.threshold {
                let first = index - self.before.len() as u64;
                let mut samples = Vec::with_capacity(FRAME * 64);
                let mut probs = Vec::new();
                for (f, q) in self.before.drain(..) {
                    samples.extend_from_slice(&f);
                    probs.push(q);
                }
                samples.extend_from_slice(frame);
                probs.push(prob);
                self.active = Some(Active {
                    first,
                    samples,
                    probs,
                    silence_run: 0,
                });
            } else {
                self.before.push_back((frame.to_vec(), prob));
                if self.before.len() > ChunkParams::frames(p.pad_ms) {
                    self.before.pop_front();
                }
            }
            return None;
        };
        active.samples.extend_from_slice(frame);
        active.probs.push(prob);
        active.silence_run = if prob >= p.neg_threshold {
            0
        } else {
            active.silence_run + 1
        };
        if active.silence_run >= ChunkParams::frames(p.min_silence_ms) {
            return self.close();
        }
        if active.probs.len() >= ChunkParams::frames(p.max_chunk_ms) {
            return self.split();
        }
        None
    }

    /// No chunk this chunker emits later starts before this: the open chunk's
    /// first frame, else the frame the leading pad reaches back to (the pad
    /// can still join a chunk that opens now).
    pub fn settled_ms(&self) -> u64 {
        let frame = match &self.active {
            Some(active) => active.first,
            None => self.seen - self.before.len() as u64,
        };
        frame * FRAME_MS
    }

    /// A copy of the open chunk so far, with the start it will keep when it
    /// closes (live captions show its text before the sentence ends).
    pub fn open_chunk(&self) -> Option<Chunk> {
        self.active.as_ref().map(|active| Chunk {
            channel: self.channel,
            start_ms: active.first * FRAME_MS,
            samples: active.samples.clone(),
        })
    }

    /// Continues at frame `index` of the channel, without the open chunk and
    /// the leading pad: the audio in between was skipped (live captions that
    /// fell behind), and a chunk that opens next starts no earlier.
    pub fn jump_to(&mut self, index: u64) {
        self.active = None;
        self.before.clear();
        self.seen = index;
    }

    /// The chunk still open at the end of the channel.
    pub fn finish(&mut self) -> Option<Chunk> {
        self.active.as_ref()?;
        self.close()
    }

    /// Ends the open chunk after its last speech plus the pad; the silence
    /// after that becomes the next chunk's leading pad.
    fn close(&mut self) -> Option<Chunk> {
        let mut active = self.active.take()?;
        let pad = ChunkParams::frames(self.params.pad_ms);
        let keep = active.probs.len() - active.silence_run.saturating_sub(pad);
        self.before.clear();
        let tail_from = keep.max(active.probs.len().saturating_sub(pad));
        for i in tail_from..active.probs.len() {
            self.before
                .push_back((active.samples[i * FRAME..(i + 1) * FRAME].to_vec(), active.probs[i]));
        }
        active.samples.truncate(keep * FRAME);
        active.probs.truncate(keep);
        self.emit(active)
    }

    /// Cuts a chunk that reached the length limit at its quietest frame
    /// near the end; the rest stays open.
    fn split(&mut self) -> Option<Chunk> {
        let mut active = self.active.take()?;
        let len = active.probs.len();
        let window = ChunkParams::frames(self.params.split_window_ms).min(len - 1);
        let from = len - window;
        // The latest of the quietest frames, so the first part is longest.
        let cut = (from..len)
            .rev()
            .min_by(|&a, &b| active.probs[a].total_cmp(&active.probs[b]))
            .unwrap_or(len - 1)
            .max(1);
        let rest_samples = active.samples.split_off(cut * FRAME);
        let rest_probs = active.probs.split_off(cut);
        let silence_run = rest_probs
            .iter()
            .rev()
            .take_while(|&&q| q < self.params.neg_threshold)
            .count();
        let rest = Active {
            first: active.first + cut as u64,
            samples: rest_samples,
            probs: rest_probs,
            silence_run,
        };
        active.silence_run = 0;
        self.active = Some(rest);
        self.emit(active)
    }

    fn emit(&self, active: Active) -> Option<Chunk> {
        let speech = active
            .probs
            .iter()
            .filter(|&&q| q >= self.params.threshold)
            .count() as u64;
        if speech * FRAME_MS < self.params.min_speech_ms || active.samples.is_empty() {
            return None;
        }
        Some(Chunk {
            channel: self.channel,
            start_ms: active.first * FRAME_MS,
            samples: active.samples,
        })
    }
}

/// A speech probability per frame from loudness alone: the level against a
/// noise floor that follows quiet stretches quickly and loud ones slowly.
/// The fallback when the Silero VAD model is not installed.
#[derive(Debug, Clone)]
pub struct EnergyVad {
    floor_db: f32,
}

impl Default for EnergyVad {
    fn default() -> Self {
        EnergyVad { floor_db: -60.0 }
    }
}

impl EnergyVad {
    /// Speech must be this far above the floor...
    const OVER_FLOOR_DB: f32 = 12.0;
    /// ...and at least this loud.
    const MIN_DB: f32 = -50.0;

    pub fn prob(&mut self, frame: &[f32]) -> f32 {
        let power = frame.iter().map(|v| v * v).sum::<f32>() / frame.len().max(1) as f32;
        let db = 10.0 * (power + 1e-12).log10();
        let gate = (self.floor_db + Self::OVER_FLOOR_DB).max(Self::MIN_DB);
        let prob = 1.0 / (1.0 + (-(db - gate) / 3.0).exp());
        // Down fast, up slowly, and slower still through speech.
        let rate = if db < self.floor_db {
            0.3
        } else if prob >= 0.5 {
            0.0001
        } else {
            0.01
        };
        self.floor_db = (self.floor_db + (db - self.floor_db) * rate).clamp(-90.0, -20.0);
        prob
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEECH: f32 = 0.9;
    const QUIET: f32 = 0.05;

    /// Runs probabilities through a chunker (frames of the frame index, so
    /// a chunk's samples say which frames it holds).
    fn chunks_of(probs: &[f32], params: ChunkParams) -> Vec<(u64, u64)> {
        let mut chunker = Chunker::new(1, params);
        let mut out = Vec::new();
        for (i, &p) in probs.iter().enumerate() {
            let frame = vec![i as f32; FRAME];
            out.extend(chunker.push(&frame, p));
        }
        out.extend(chunker.finish());
        for c in &out {
            assert_eq!(c.channel, 1);
            assert_eq!(c.samples.len() % FRAME, 0);
            // The samples are the frames from the start time on, in order.
            let first = c.start_ms / FRAME_MS;
            for (k, f) in c.samples.chunks(FRAME).enumerate() {
                assert_eq!(f[0], (first + k as u64) as f32);
            }
        }
        out.iter().map(|c| (c.start_ms, c.end_ms())).collect()
    }

    fn pattern(parts: &[(usize, f32)]) -> Vec<f32> {
        parts.iter().flat_map(|&(n, p)| std::iter::repeat_n(p, n)).collect()
    }

    #[test]
    fn a_jump_drops_the_open_chunk_and_keeps_the_time() {
        let mut chunker = Chunker::new(0, ChunkParams::default());
        for _ in 0..20 {
            assert_eq!(chunker.push(&[0.0; FRAME], SPEECH), None);
        }
        assert!(chunker.open_chunk().is_some());
        chunker.jump_to(1_000);
        assert_eq!(chunker.open_chunk(), None);
        assert_eq!(chunker.settled_ms(), 1_000 * FRAME_MS, "no pad from before the jump");
        chunker.push(&[0.0; FRAME], SPEECH);
        assert_eq!(chunker.open_chunk().unwrap().start_ms, 1_000 * FRAME_MS);
    }

    #[test]
    fn silence_gives_nothing_and_a_blip_is_dropped() {
        let params = ChunkParams::default();
        assert!(chunks_of(&pattern(&[(500, QUIET)]), params).is_empty());
        // 5 frames = 160 ms < 250 ms.
        let blip = pattern(&[(50, QUIET), (5, SPEECH), (100, QUIET)]);
        assert!(chunks_of(&blip, params).is_empty());
    }

    #[test]
    fn speech_is_padded_and_ended_by_a_pause() {
        let params = ChunkParams::default();
        // Speech in frames 100..150 and 300..340.
        let probs = pattern(&[(100, QUIET), (50, SPEECH), (150, QUIET), (40, SPEECH), (100, QUIET)]);
        let chunks = chunks_of(&probs, params);
        let pad = ChunkParams::frames(params.pad_ms) as u64;
        assert_eq!(
            chunks,
            vec![
                ((100 - pad) * FRAME_MS, (150 + pad) * FRAME_MS),
                ((300 - pad) * FRAME_MS, (340 + pad) * FRAME_MS),
            ]
        );
    }

    #[test]
    fn a_short_pause_does_not_split_and_the_end_closes_the_last_chunk() {
        let params = ChunkParams::default();
        // A 10-frame (320 ms) pause inside speech, speech up to the end.
        let probs = pattern(&[(20, QUIET), (30, SPEECH), (10, QUIET), (30, SPEECH)]);
        let chunks = chunks_of(&probs, params);
        let pad = ChunkParams::frames(params.pad_ms) as u64;
        assert_eq!(chunks, vec![((20 - pad) * FRAME_MS, 90 * FRAME_MS)]);
    }

    #[test]
    fn a_long_stretch_is_split_at_its_quietest_frame_near_the_limit() {
        let params = ChunkParams::default();
        let max = ChunkParams::frames(params.max_chunk_ms);
        // 2000 frames (64 s) of speech; one soft dip at frame 900.
        let mut probs = pattern(&[(2000, SPEECH)]);
        probs[900] = 0.4;
        let chunks = chunks_of(&probs, params);
        assert!(chunks.len() >= 3, "{chunks:?}");
        assert_eq!(chunks[0], (0, 900 * FRAME_MS), "cut at the dip");
        for (start, end) in &chunks {
            assert!(end - start <= max as u64 * FRAME_MS);
        }
        // No audio lost or doubled between the pieces.
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
        assert_eq!(chunks.last().unwrap().1, 2000 * FRAME_MS);
    }

    #[test]
    fn the_framer_hands_out_whole_frames_and_pads_the_last() {
        let mut framer = Framer::default();
        let mut sizes = Vec::new();
        framer.push(&vec![0.1; 700], |f| sizes.push(f.len()));
        framer.push(&vec![0.1; 400], |f| sizes.push(f.len()));
        assert_eq!(sizes, vec![FRAME, FRAME]);
        let mut last = Vec::new();
        framer.finish(|f| last.extend_from_slice(f));
        assert_eq!(last.len(), FRAME);
        assert_eq!(last[75], 0.1);
        assert_eq!(last[76], 0.0);
    }

    fn noise(seed: &mut u32, amp: f32) -> f32 {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 * amp - amp
    }

    #[test]
    fn the_energy_detector_finds_talk_over_room_noise() {
        let mut seed = 7;
        let mut vad = EnergyVad::default();
        let mut chunker = Chunker::new(0, ChunkParams::default());
        let mut chunks = Vec::new();
        // 10 s: room noise at about -55 dBFS, "talk" (a 300 Hz buzz at about
        // -20 dBFS) from 2.0 to 4.0 s and from 6.0 to 7.0 s.
        let total = RATE as usize * 10;
        let audio: Vec<f32> = (0..total)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let talk = (2.0..4.0).contains(&t) || (6.0..7.0).contains(&t);
                let voice = if talk {
                    0.14 * (2.0 * std::f32::consts::PI * 300.0 * t).sin()
                } else {
                    0.0
                };
                voice + noise(&mut seed, 0.003)
            })
            .collect();
        for frame in audio.chunks_exact(FRAME) {
            let p = vad.prob(frame);
            chunks.extend(chunker.push(frame, p));
        }
        chunks.extend(chunker.finish());
        let spans: Vec<(u64, u64)> = chunks.iter().map(|c| (c.start_ms, c.end_ms())).collect();
        assert_eq!(spans.len(), 2, "{spans:?}");
        let near = |a: u64, b: u64| a.abs_diff(b) <= 300;
        assert!(near(spans[0].0, 2000) && near(spans[0].1, 4000), "{spans:?}");
        assert!(near(spans[1].0, 6000) && near(spans[1].1, 7000), "{spans:?}");
    }

    #[test]
    fn the_energy_detector_keeps_a_long_talk_as_speech() {
        let mut vad = EnergyVad::default();
        let frame: Vec<f32> = (0..FRAME)
            .map(|i| 0.1 * (i as f32 * 0.1).sin())
            .collect();
        let quiet = vec![0.0005; FRAME];
        for _ in 0..100 {
            vad.prob(&quiet);
        }
        // Two minutes of steady talk stays speech the whole time.
        let probs: Vec<f32> = (0..3750).map(|_| vad.prob(&frame)).collect();
        assert!(probs.iter().all(|&p| p > 0.5), "{:?}", probs.last());
        assert!(vad.prob(&quiet) < 0.1);
    }

    #[test]
    fn settled_is_the_open_chunks_start_or_the_pad_before_now() {
        let mut c = Chunker::new(0, ChunkParams::default());
        let (quiet, loud) = (vec![0.0; FRAME], vec![0.5; FRAME]);
        let mut marks = Vec::new();
        let mut chunks = Vec::new();
        for i in 0..400u64 {
            let speech = (50..120).contains(&i) || (200..260).contains(&i);
            let f = if speech { &loud } else { &quiet };
            chunks.extend(c.push(f, if speech { 1.0 } else { 0.0 }).map(|ch| (i, ch)));
            marks.push((i, c.settled_ms()));
            if i == 49 {
                assert_eq!(c.settled_ms(), (50 - 6) * FRAME_MS, "the 192 ms pad may still join");
            }
        }
        assert!(marks.windows(2).all(|w| w[0].1 <= w[1].1), "never goes back");
        // No chunk emitted at frame i starts before a mark read before i.
        for (at, ch) in &chunks {
            for (i, m) in &marks {
                if i < at {
                    assert!(ch.start_ms >= *m, "chunk at {} ms emitted at frame {at}, mark {m} at {i}", ch.start_ms);
                }
            }
        }
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn open_chunk_copies_the_open_speech() {
        let mut c = Chunker::new(1, ChunkParams::default());
        let (quiet, loud) = (vec![0.0; FRAME], vec![0.5; FRAME]);
        let pad = ChunkParams::frames(ChunkParams::default().pad_ms);
        for _ in 0..20 {
            assert_eq!(c.push(&quiet, 0.0), None);
        }
        assert_eq!(c.open_chunk(), None, "nothing is open before speech");
        for _ in 0..10 {
            assert_eq!(c.push(&loud, 1.0), None);
        }
        let open = c.open_chunk().expect("speech is open");
        assert_eq!(open.channel, 1);
        assert_eq!(open.samples.len(), (10 + pad) * FRAME);
        assert_eq!(open.start_ms, (20 - pad as u64) * FRAME_MS);
        assert_eq!(c.open_chunk(), Some(open.clone()), "a copy: the chunk stays open");
        let closed = (0..100).find_map(|_| c.push(&quiet, 0.0)).expect("the pause closes it");
        assert_eq!(closed.start_ms, open.start_ms);
        assert_eq!(closed.samples[..open.samples.len()], open.samples[..]);
        assert_eq!(c.open_chunk(), None);
    }
}
