//! The live chunking core: 16 kHz audio in, work for the speech engine out.
//!
//! The caption worker `put`s what the capture threads hear on a shared
//! timeline (a source that sends no packets in silence still reads as
//! silence), `advance`s it to the clock (or `skip_to`s it after a clock jump), asks `next_work` what to run the
//! engine on and hands the result back with `final_done` or `interim_done`.
//! Those return what the card shows; `take_out` has what came up in between.
//! Nothing here touches a device, a thread or the engine, so the tests drive
//! it with made-up audio.

use std::collections::VecDeque;

use crate::recorder::timeline::Timeline;
use crate::transcribe::chunk::{Chunk, ChunkParams, Chunker, FRAME, Framer};
use crate::transcribe::decode::RATE;
use crate::transcribe::echo;
use crate::transcribe::engine::EngineError;
use crate::transcribe::output::{self, Segment, Speaker};
use crate::transcribe::runner::FrameScorer;

/// Audio newer than this is not taken yet: a packet arrives a little after
/// the time it is stamped with, and a frame taken early would read as silence.
pub const LAG_MS: u64 = 300;
/// How often the open chunk is run again for the grey line.
pub const INTERIM_EVERY_MS: u64 = 1_500;
/// An open chunk shorter than this gets no interim.
pub const INTERIM_MIN_MS: u64 = 1_000;
/// More finished audio than this waiting for the engine: the oldest is skipped.
pub const MAX_BACKLOG_MS: u64 = 15_000;
/// One `advance` takes at most this much, so a clock that ran on through a
/// sleep never makes one huge buffer.
pub const MAX_TAKE_MS: u64 = 1_000;
/// A "Peserta" line older than this (against the newest) is no longer kept
/// to compare the mic against.
const ECHO_WINDOW_MS: u64 = 30_000;

/// The offline cut, with a shorter pause and shorter chunks: a line shows
/// soon after its sentence ends, and a long stretch never waits 30 s.
pub fn live_params() -> ChunkParams {
    ChunkParams {
        min_silence_ms: 500,
        max_chunk_ms: 10_000,
        split_window_ms: 3_000,
        ..ChunkParams::default()
    }
}

/// What to run the engine on next.
#[derive(Debug, Clone, PartialEq)]
pub enum Work {
    /// A finished chunk; its result goes to `final_done`.
    Final(Chunk),
    /// The open chunk so far; its result goes to `interim_done`.
    Interim(Chunk),
}

/// What the card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Final(Segment),
    /// The grey line of a channel (replaces the one before).
    Interim { channel: usize, text: String },
    ClearInterim { channel: usize },
    /// Speech was skipped here because the engine fell behind.
    Gap,
    /// The model is heavy for captions on this PC (once in a session).
    Slow,
}

pub struct Live {
    speakers: Vec<Option<Speaker>>,
    /// The "Peserta" channel, when there is one.
    others: Option<usize>,
    timeline: Timeline,
    /// Frames of the session read from the timeline so far.
    taken: i64,
    framers: Vec<Framer>,
    chunkers: Vec<Chunker>,
    /// Closed chunks waiting for the engine, oldest first.
    finals: VecDeque<Chunk>,
    /// Per channel, the `now_ms` its last interim was handed out at.
    interim_at: Vec<u64>,
    /// "Saya" segments not shown yet: "Peserta" at that time may still come.
    held: Vec<Segment>,
    /// The "Peserta" segments of the last `ECHO_WINDOW_MS`.
    others_recent: VecDeque<Segment>,
    /// What `next_work` found on the way, for `take_out`.
    out: Vec<Out>,
    slow_told: bool,
}

impl Live {
    /// One channel per entry of `speakers`.
    pub fn new(speakers: Vec<Option<Speaker>>) -> Live {
        let channels = speakers.len();
        Live {
            others: speakers.iter().position(|s| *s == Some(Speaker::Others)),
            timeline: Timeline::new(RATE, channels),
            taken: 0,
            framers: (0..channels).map(|_| Framer::default()).collect(),
            chunkers: (0..channels).map(|c| Chunker::new(c, live_params())).collect(),
            finals: VecDeque::new(),
            interim_at: vec![0; channels],
            held: Vec::new(),
            others_recent: VecDeque::new(),
            out: Vec::new(),
            slow_told: false,
            speakers,
        }
    }

    /// Places 16 kHz samples of one channel; `start_frame` counts from the
    /// start of the caption session.
    pub fn put(&mut self, channel: usize, start_frame: i64, samples: &[f32]) {
        self.timeline.put(channel, start_frame, samples);
    }

    /// Reads the audio up to `LAG_MS` before `now_frame` (the session clock,
    /// in 16 kHz frames), at most `MAX_TAKE_MS` of it. `true` when more is
    /// left up to there: the caller calls again until it has caught up.
    /// Chunks that close wait for `next_work`. After an `Err` (a scorer
    /// failed) part of the audio was never scored: the session is over.
    pub fn advance(
        &mut self,
        now_frame: i64,
        scorers: &mut [Box<dyn FrameScorer + '_>],
    ) -> Result<bool, EngineError> {
        let channels = self.speakers.len();
        assert_eq!(scorers.len(), channels, "one scorer per channel");
        let up_to = now_frame - ms_to_frames(LAG_MS) as i64;
        let block = self.timeline.take_max(up_to, ms_to_frames(MAX_TAKE_MS));
        self.taken += (block.len() / channels) as i64;
        let mut failure = None;
        for c in 0..channels {
            let samples: Vec<f32> = block.iter().skip(c).step_by(channels).copied().collect();
            let (scorer, chunker, finals) = (&mut scorers[c], &mut self.chunkers[c], &mut self.finals);
            self.framers[c].push(&samples, |frame| match scorer.score(frame) {
                Ok(p) => finals.extend(chunker.push(frame, p)),
                Err(e) => failure = Some(e),
            });
        }
        failure.map_or(Ok(self.taken < up_to), Err)
    }

    /// The frame of the session `advance` has read up to.
    pub fn taken_frames(&self) -> i64 {
        self.taken
    }

    /// Moves on to `frame` (rounded down to a whole VAD frame) without
    /// reading what lies before it: for a clock that ran far ahead of the
    /// audio (the PC slept, the worker stalled), where replaying the silence
    /// or the stale audio helps nobody. The buffered audio, the open chunks
    /// and their grey lines are dropped, chunks that were already whole
    /// stay, and `take_out` gets one `Gap`. What is heard after it keeps its
    /// time in the session. Nothing happens when `advance` is already there.
    pub fn skip_to(&mut self, frame: i64) {
        let to = frame - frame.rem_euclid(FRAME as i64);
        if to <= self.taken {
            return;
        }
        self.timeline.skip_to(to);
        self.taken = to;
        for (c, chunker) in self.chunkers.iter_mut().enumerate() {
            if chunker.open_chunk().is_some() {
                self.out.push(Out::ClearInterim { channel: c });
            }
            chunker.jump_to((to / FRAME as i64) as u64);
            self.framers[c] = Framer::default();
        }
        self.out.push(Out::Gap);
    }

    /// What the engine should run on now: the oldest final, else an interim
    /// of the open chunk that waited longest for one, else nothing. `now_ms`
    /// is the session clock `advance` gets, in milliseconds. Skips the oldest
    /// finals when more than `MAX_BACKLOG_MS` of audio waits, and queues for
    /// `take_out` the gap that leaves, the slow hint, and "Saya" lines whose
    /// hold ended.
    pub fn next_work(&mut self, now_ms: u64) -> Option<Work> {
        if self.backlog_ms() > MAX_BACKLOG_MS {
            while self.backlog_ms() > MAX_BACKLOG_MS {
                self.finals.pop_front();
            }
            self.out.push(Out::Gap);
            if !self.slow_told {
                self.slow_told = true;
                self.out.push(Out::Slow);
            }
        }
        let released = self.release();
        self.out.extend(released.into_iter().map(Out::Final));
        if let Some(chunk) = self.finals.pop_front() {
            return Some(Work::Final(chunk));
        }
        let mut best: Option<(u64, Chunk)> = None;
        for (c, chunker) in self.chunkers.iter().enumerate() {
            if now_ms.saturating_sub(self.interim_at[c]) < INTERIM_EVERY_MS {
                continue;
            }
            let Some(open) = chunker.open_chunk() else { continue };
            // Counted from the last interim, or from when the chunk opened.
            let since = self.interim_at[c].max(open.start_ms);
            let due = now_ms.saturating_sub(since) >= INTERIM_EVERY_MS
                && open.end_ms() - open.start_ms >= INTERIM_MIN_MS;
            if due && best.as_ref().is_none_or(|(oldest, _)| since < *oldest) {
                best = Some((since, open));
            }
        }
        let (_, open) = best?;
        self.interim_at[open.channel] = now_ms;
        Some(Work::Interim(open))
    }

    /// The segments the engine heard in a final from `next_work`: what to
    /// show now. A "Saya" segment waits until no "Peserta" speech of its time
    /// can still come (it may be the speakers heard by the mic); one that
    /// turns out to be that echo is never shown.
    pub fn final_done(&mut self, chunk: &Chunk, segments: Vec<Segment>) -> Vec<Out> {
        let mut shown = Vec::new();
        for segment in segments {
            match segment.speaker {
                Some(Speaker::Me) => self.held.push(segment),
                Some(Speaker::Others) => {
                    self.others_recent.push_back(segment.clone());
                    shown.push(segment);
                }
                None => shown.push(segment),
            }
        }
        let newest = self.others_recent.iter().map(|s| s.end_ms).max().unwrap_or(0);
        self.others_recent.retain(|s| s.end_ms + ECHO_WINDOW_MS >= newest);
        shown.extend(self.release());
        let mut out = vec![Out::ClearInterim { channel: chunk.channel }];
        out.extend(output::merge(shown).into_iter().map(Out::Final));
        out
    }

    /// The text the engine heard in an interim from `next_work`. Nothing when
    /// that chunk is no longer the open one (its final replaces it).
    pub fn interim_done(&mut self, chunk: &Chunk, text: String) -> Vec<Out> {
        let open = self.chunkers.get(chunk.channel).and_then(Chunker::open_chunk);
        let text = text.trim();
        if open.is_none_or(|open| open.start_ms != chunk.start_ms) || text.is_empty() {
            return Vec::new();
        }
        vec![Out::Interim { channel: chunk.channel, text: text.to_string() }]
    }

    /// What `next_work` queued since the last call, in order.
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// The finished audio waiting for the engine.
    fn backlog_ms(&self) -> u64 {
        self.finals.iter().map(|c| c.end_ms() - c.start_ms).sum()
    }

    /// No "Peserta" segment that comes later starts before this: where its
    /// chunker is settled, or the first "Peserta" chunk still waiting for the
    /// engine. `u64::MAX` without a "Peserta" channel.
    fn others_settled_ms(&self) -> u64 {
        let Some(others) = self.others else { return u64::MAX };
        self.finals
            .iter()
            .filter(|c| c.channel == others)
            .map(|c| c.start_ms)
            .fold(self.chunkers[others].settled_ms(), u64::min)
    }

    /// Takes the held "Saya" segments whose hold ended, without the ones
    /// that echo "Peserta".
    fn release(&mut self) -> Vec<Segment> {
        let mark = self.others_settled_ms();
        let (ready, held): (Vec<Segment>, Vec<Segment>) =
            std::mem::take(&mut self.held).into_iter().partition(|s| echo::settled(s, mark));
        self.held = held;
        if ready.is_empty() {
            return ready;
        }
        let mut all: Vec<Segment> = self.others_recent.iter().cloned().collect();
        all.extend(ready);
        echo::drop_echo(all)
            .into_iter()
            .filter(|s| s.speaker == Some(Speaker::Me))
            .collect()
    }
}

fn ms_to_frames(ms: u64) -> usize {
    (ms * RATE as u64 / 1000) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Speech when the frame is loud at all.
    struct Loud;

    impl FrameScorer for Loud {
        fn score(&mut self, frame: &[f32]) -> Result<f32, EngineError> {
            let power = frame.iter().map(|v| v * v).sum::<f32>() / frame.len() as f32;
            Ok(if power.sqrt() > 0.01 { 1.0 } else { 0.0 })
        }
    }

    struct Broken;

    impl FrameScorer for Broken {
        fn score(&mut self, _: &[f32]) -> Result<f32, EngineError> {
            Err(EngineError::NoBackend)
        }
    }

    fn frames(ms: u64) -> usize {
        (ms * RATE as u64 / 1000) as usize
    }

    fn tone(ms: u64) -> Vec<f32> {
        (0..frames(ms))
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 300.0 * i as f32 / RATE as f32).sin())
            .collect()
    }

    fn silence(ms: u64) -> Vec<f32> {
        vec![0.0; frames(ms)]
    }

    fn len_ms(chunk: &Chunk) -> u64 {
        chunk.end_ms() - chunk.start_ms
    }

    /// A caption session with a clock: `play` sends packets as they would
    /// arrive, `wait` lets time pass without any.
    struct Rig {
        live: Live,
        scorers: Vec<Box<dyn FrameScorer>>,
        /// The session clock, in frames.
        now: i64,
    }

    impl Rig {
        fn new(speakers: Vec<Option<Speaker>>) -> Rig {
            let scorers = speakers.iter().map(|_| Box::new(Loud) as Box<dyn FrameScorer>).collect();
            Rig { live: Live::new(speakers), scorers, now: 0 }
        }

        fn play(&mut self, channel: usize, samples: &[f32]) {
            for second in samples.chunks(RATE as usize) {
                self.live.put(channel, self.now, second);
                self.pass(second.len() as i64);
            }
        }

        fn wait(&mut self, ms: u64) {
            for _ in 0..ms / 100 {
                self.pass(frames(100) as i64);
            }
        }

        /// At most a second passes, which one `advance` takes.
        fn pass(&mut self, frames: i64) {
            self.now += frames;
            self.live.advance(self.now, &mut self.scorers).unwrap();
        }

        fn now_ms(&self) -> u64 {
            self.now as u64 * 1000 / RATE as u64
        }

        /// The finals waiting now.
        fn finals(&mut self) -> Vec<Chunk> {
            let now_ms = self.now_ms();
            let mut out = Vec::new();
            while let Some(Work::Final(chunk)) = self.live.next_work(now_ms) {
                out.push(chunk);
            }
            out
        }
    }

    fn seg(start_ms: u64, end_ms: u64, speaker: Speaker, text: &str) -> Segment {
        Segment { start_ms, end_ms, speaker: Some(speaker), text: text.into() }
    }

    fn chunk(channel: usize, start_ms: u64, ms: u64) -> Chunk {
        Chunk { channel, start_ms, samples: silence(ms) }
    }

    fn pair() -> Rig {
        Rig::new(vec![Some(Speaker::Me), Some(Speaker::Others)])
    }

    #[test]
    fn live_params_close_sooner_and_cut_shorter() {
        let p = live_params();
        assert_eq!((p.min_silence_ms, p.max_chunk_ms, p.split_window_ms), (500, 10_000, 3_000));
        let d = ChunkParams::default();
        let rest = ChunkParams {
            min_silence_ms: d.min_silence_ms,
            max_chunk_ms: d.max_chunk_ms,
            split_window_ms: d.split_window_ms,
            ..p
        };
        assert_eq!(rest, d, "everything else as the offline transcript");
    }

    #[test]
    fn a_pause_closes_a_final() {
        let mut rig = Rig::new(vec![None]);
        rig.play(0, &tone(2_000));
        assert_eq!(rig.finals(), vec![], "still open");
        rig.play(0, &silence(1_000));
        rig.wait(300);
        let finals = rig.finals();
        assert_eq!(finals.len(), 1);
        assert_eq!(finals[0].channel, 0);
        assert!(finals[0].start_ms <= 200, "{}", finals[0].start_ms);
        assert!((2_000..=2_800).contains(&len_ms(&finals[0])), "{}", len_ms(&finals[0]));
    }

    #[test]
    fn a_sentence_ends_when_the_pc_goes_silent_without_packets() {
        let mut rig = Rig::new(vec![None]);
        rig.play(0, &tone(2_000));
        rig.wait(1_300);
        let finals = rig.finals();
        assert_eq!(finals.len(), 1, "the frames no packet wrote read as silence");
        assert!((2_000..=2_800).contains(&len_ms(&finals[0])), "{}", len_ms(&finals[0]));
    }

    #[test]
    fn a_packet_inside_the_lag_is_still_heard() {
        let mut rig = Rig::new(vec![None]);
        rig.play(0, &tone(600));
        // The packet for 0.6-1.6 s arrives at 0.8 s; nothing past 0.5 s was
        // taken yet.
        rig.wait(200);
        rig.live.put(0, frames(600) as i64, &tone(1_000));
        rig.wait(2_000);
        let finals = rig.finals();
        assert_eq!(finals.len(), 1);
        let holes = finals[0].samples[..frames(1_600)].iter().filter(|v| **v == 0.0).count();
        assert!(holes < 100, "{holes} samples read as a gap");
    }

    #[test]
    fn one_advance_takes_at_most_a_second() {
        let mut live = Live::new(vec![None]);
        let mut scorers: Vec<Box<dyn FrameScorer>> = vec![Box::new(Loud)];
        live.put(0, 0, &tone(4_000));
        let now = frames(4_000 + LAG_MS) as i64;
        assert_eq!(live.advance(now, &mut scorers), Ok(true), "more is left");
        assert_eq!(live.advance(now, &mut scorers), Ok(true));
        let Some(Work::Interim(open)) = live.next_work(60_000) else { panic!() };
        assert!((1_900..=2 * MAX_TAKE_MS).contains(&len_ms(&open)), "{}", len_ms(&open));
        assert_eq!(live.advance(now, &mut scorers), Ok(true));
        assert_eq!(live.advance(now, &mut scorers), Ok(false), "caught up");
        assert_eq!(live.advance(now, &mut scorers), Ok(false));
        assert_eq!(live.taken_frames(), frames(4_000) as i64);
        let Some(Work::Interim(open)) = live.next_work(120_000) else { panic!() };
        assert!((3_900..=4_000).contains(&len_ms(&open)), "caught up: {}", len_ms(&open));
    }

    #[test]
    fn a_scorer_failure_is_returned() {
        let mut live = Live::new(vec![None]);
        let mut scorers: Vec<Box<dyn FrameScorer>> = vec![Box::new(Broken)];
        live.put(0, 0, &tone(1_000));
        assert_eq!(live.advance(frames(2_000) as i64, &mut scorers), Err(EngineError::NoBackend));
    }

    #[test]
    fn interim_comes_every_1500_ms_only_without_finals() {
        let mut rig = Rig::new(vec![None]);
        let talk = tone(4_000);
        rig.play(0, &talk[..frames(1_200)]);
        assert_eq!(rig.live.next_work(1_200), None, "under a second of speech so far");
        rig.play(0, &talk[frames(1_200)..frames(2_000)]);
        let Some(Work::Interim(first)) = rig.live.next_work(2_000) else { panic!() };
        assert_eq!((first.channel, first.start_ms), (0, 0));
        assert!(len_ms(&first) >= INTERIM_MIN_MS);
        rig.play(0, &talk[frames(2_000)..frames(3_000)]);
        assert_eq!(rig.live.next_work(3_000), None);
        assert_eq!(rig.live.next_work(3_499), None);
        let Some(Work::Interim(second)) = rig.live.next_work(3_500) else { panic!() };
        assert_eq!(second.start_ms, first.start_ms);
        assert!(second.samples.len() > first.samples.len());
        // The sentence ends: its final comes, and no interim after it.
        rig.play(0, &talk[frames(3_000)..]);
        rig.wait(1_500);
        let Some(Work::Final(done)) = rig.live.next_work(9_000) else { panic!() };
        assert_eq!(done.start_ms, first.start_ms);
        assert_eq!(rig.live.next_work(9_000), None);
    }

    #[test]
    fn a_final_comes_before_an_interim() {
        let mut rig = pair();
        // "Saya" says a sentence and stops while "Peserta" keeps talking.
        rig.live.put(1, 0, &tone(5_000));
        rig.play(0, &tone(2_000));
        rig.wait(3_000);
        let now_ms = rig.now_ms();
        let Some(Work::Final(done)) = rig.live.next_work(now_ms) else { panic!() };
        assert_eq!(done.channel, 0);
        let Some(Work::Interim(open)) = rig.live.next_work(now_ms) else { panic!() };
        assert_eq!(open.channel, 1);
        assert_eq!(rig.live.next_work(now_ms), None);
    }

    #[test]
    fn the_channel_that_waited_longest_gets_the_interim() {
        let mut rig = pair();
        rig.live.put(0, frames(1_000) as i64, &tone(4_000));
        rig.play(1, &tone(5_000));
        rig.wait(300);
        let mut order = Vec::new();
        while let Some(Work::Interim(open)) = rig.live.next_work(5_300) {
            order.push(open.channel);
        }
        assert_eq!(order, vec![1, 0], "the chunk that opened first, then the other");
    }

    #[test]
    fn a_long_stretch_is_split_at_10_s() {
        let mut rig = Rig::new(vec![None]);
        let mut finals = Vec::new();
        for part in tone(25_000).chunks(frames(5_000)) {
            rig.play(0, part);
            finals.extend(rig.finals());
        }
        rig.wait(300);
        finals.extend(rig.finals());
        assert_eq!(finals.len(), 2, "the rest is still open");
        for f in &finals {
            assert!(len_ms(f) <= 10_000 && len_ms(f) > 9_000, "{}", len_ms(f));
        }
        assert_eq!(finals[0].end_ms(), finals[1].start_ms, "nothing lost at the cut");
    }

    #[test]
    fn the_backlog_is_cut_to_15_s() {
        let mut rig = Rig::new(vec![None]);
        let sentence = [tone(2_000), silence(1_000)].concat();
        for _ in 0..20 {
            rig.play(0, &sentence);
        }
        assert!(rig.live.backlog_ms() >= 40_000, "{}", rig.live.backlog_ms());
        assert_eq!(rig.live.take_out(), vec![]);
        let Some(Work::Final(first)) = rig.live.next_work(rig.now_ms()) else { panic!() };
        assert!(first.start_ms > 30_000, "the oldest were skipped: {}", first.start_ms);
        let left = rig.live.backlog_ms() + len_ms(&first);
        assert!(left <= MAX_BACKLOG_MS && left > MAX_BACKLOG_MS - 3_000, "{left}");
        assert_eq!(rig.live.take_out(), vec![Out::Gap, Out::Slow]);
        assert_eq!(rig.live.take_out(), vec![]);
        // Under the cap nothing is skipped.
        assert!(matches!(rig.live.next_work(rig.now_ms()), Some(Work::Final(_))));
        assert_eq!(rig.live.take_out(), vec![]);
        // The hint comes once in a session.
        for _ in 0..10 {
            rig.play(0, &sentence);
        }
        assert!(matches!(rig.live.next_work(rig.now_ms()), Some(Work::Final(_))));
        assert!(rig.live.backlog_ms() <= MAX_BACKLOG_MS);
        assert_eq!(rig.live.take_out(), vec![Out::Gap]);
    }

    #[test]
    fn a_clock_jump_skips_ahead_with_one_gap() {
        let mut rig = Rig::new(vec![None]);
        rig.play(0, &tone(2_000));
        assert!(matches!(rig.live.next_work(rig.now_ms()), Some(Work::Interim(_))), "a sentence is open");
        // The PC slept for a minute: the clock ran on, no packet came.
        rig.now += frames(60_000) as i64;
        rig.live.skip_to(rig.now - frames(LAG_MS) as i64);
        assert_eq!(rig.live.advance(rig.now, &mut rig.scorers), Ok(false), "nothing to replay");
        assert_eq!(rig.live.next_work(rig.now_ms()), None, "the open sentence went with the skip");
        assert_eq!(rig.live.take_out(), vec![Out::ClearInterim { channel: 0 }, Out::Gap]);
        // What is said after the jump has its own time.
        rig.play(0, &tone(2_000));
        rig.wait(1_300);
        let finals = rig.finals();
        assert_eq!(finals.len(), 1);
        assert!((61_700..=62_100).contains(&finals[0].start_ms), "{}", finals[0].start_ms);
        assert!((2_000..=2_800).contains(&len_ms(&finals[0])), "{}", len_ms(&finals[0]));
        assert_eq!(rig.live.take_out(), vec![], "one gap for the jump");
    }

    #[test]
    fn a_skip_that_is_not_ahead_does_nothing() {
        let mut rig = Rig::new(vec![None]);
        rig.play(0, &tone(2_000));
        rig.live.skip_to(frames(1_000) as i64);
        assert_eq!(rig.live.take_out(), vec![]);
        rig.wait(1_300);
        let finals = rig.finals();
        assert_eq!(finals.len(), 1);
        assert!(finals[0].start_ms <= 200, "{}", finals[0].start_ms);
    }

    #[test]
    fn closed_chunks_survive_a_skip() {
        let mut rig = Rig::new(vec![None]);
        rig.play(0, &tone(2_000));
        rig.wait(1_300);
        rig.now += frames(60_000) as i64;
        rig.live.skip_to(rig.now);
        assert_eq!(rig.finals().len(), 1, "it was whole before the jump");
        assert_eq!(rig.live.take_out(), vec![Out::Gap], "no grey line was open");
    }

    const ECHOED: &str = "jadi deadline-nya jumat ya";
    const REPLY: &str = "boleh, nanti saya kabari senin";

    /// Both channels heard out to 6 s, "Peserta" with nothing waiting.
    fn meeting() -> Rig {
        let mut rig = pair();
        rig.wait(6_000);
        rig
    }

    #[test]
    fn an_echo_of_peserta_on_the_mic_is_dropped() {
        let mut rig = meeting();
        let others = seg(1_000, 3_000, Speaker::Others, ECHOED);
        assert_eq!(
            rig.live.final_done(&chunk(1, 1_000, 2_000), vec![others.clone()]),
            vec![Out::ClearInterim { channel: 1 }, Out::Final(others)]
        );
        let me = seg(1_100, 3_100, Speaker::Me, ECHOED);
        assert_eq!(
            rig.live.final_done(&chunk(0, 1_100, 2_000), vec![me]),
            vec![Out::ClearInterim { channel: 0 }]
        );
        assert_eq!(rig.live.next_work(rig.now_ms()), None);
        assert_eq!(rig.live.take_out(), vec![], "dropped, not held");
    }

    #[test]
    fn a_real_reply_on_the_mic_stays() {
        let mut rig = meeting();
        let others = seg(1_000, 3_000, Speaker::Others, ECHOED);
        let me = seg(1_100, 3_100, Speaker::Me, REPLY);
        rig.live.final_done(&chunk(1, 1_000, 2_000), vec![others]);
        assert_eq!(
            rig.live.final_done(&chunk(0, 1_100, 2_000), vec![me.clone()]),
            vec![Out::ClearInterim { channel: 0 }, Out::Final(me)]
        );
    }

    #[test]
    fn a_saya_line_waits_for_the_peserta_chunk_before_it() {
        let mut rig = pair();
        // The mic hears the speakers, and its chunk closes first.
        rig.live.put(0, frames(1_100) as i64, &tone(1_700));
        rig.live.put(1, frames(1_000) as i64, &tone(2_000));
        rig.wait(6_000);
        let now_ms = rig.now_ms();
        let Some(Work::Final(mic)) = rig.live.next_work(now_ms) else { panic!() };
        assert_eq!(mic.channel, 0);
        let me = seg(mic.start_ms, mic.end_ms(), Speaker::Me, ECHOED);
        assert_eq!(
            rig.live.final_done(&mic, vec![me]),
            vec![Out::ClearInterim { channel: 0 }],
            "the Peserta chunk of that time is not done yet"
        );
        let Some(Work::Final(pc)) = rig.live.next_work(now_ms) else { panic!() };
        assert_eq!(pc.channel, 1);
        assert_eq!(rig.live.take_out(), vec![], "still held");
        let others = seg(pc.start_ms, pc.end_ms(), Speaker::Others, ECHOED);
        assert_eq!(
            rig.live.final_done(&pc, vec![others.clone()]),
            vec![Out::ClearInterim { channel: 1 }, Out::Final(others)],
            "and then it is seen to be the echo"
        );
        assert_eq!(rig.live.next_work(now_ms), None);
        assert_eq!(rig.live.take_out(), vec![]);
    }

    #[test]
    fn a_held_saya_line_comes_out_once_peserta_is_settled() {
        let mut rig = pair();
        rig.wait(3_500);
        // "Peserta" is read up to about 3.0 s: a line that ended at 3.1 s can
        // still turn out to be an echo.
        let me = seg(1_100, 3_100, Speaker::Me, REPLY);
        assert_eq!(
            rig.live.final_done(&chunk(0, 1_100, 2_000), vec![me.clone()]),
            vec![Out::ClearInterim { channel: 0 }]
        );
        assert_eq!(rig.live.next_work(rig.now_ms()), None);
        assert_eq!(rig.live.take_out(), vec![]);
        rig.wait(1_500);
        assert_eq!(rig.live.next_work(rig.now_ms()), None);
        assert_eq!(rig.live.take_out(), vec![Out::Final(me)]);
        assert_eq!(rig.live.take_out(), vec![]);
    }

    #[test]
    fn held_lines_and_new_ones_come_out_in_start_order() {
        let mut rig = pair();
        rig.wait(3_500);
        let me = seg(1_100, 3_100, Speaker::Me, REPLY);
        rig.live.final_done(&chunk(0, 1_100, 2_000), vec![me.clone()]);
        rig.wait(2_500);
        let early = seg(500, 900, Speaker::Others, "halo semuanya");
        let late = seg(3_400, 4_000, Speaker::Others, "sip");
        assert_eq!(
            rig.live.final_done(&chunk(1, 500, 3_500), vec![late.clone(), early.clone()]),
            vec![
                Out::ClearInterim { channel: 1 },
                Out::Final(early),
                Out::Final(me),
                Out::Final(late),
            ]
        );
    }

    #[test]
    fn without_a_peserta_channel_saya_is_never_held() {
        let mut rig = Rig::new(vec![Some(Speaker::Me)]);
        let me = seg(1_100, 3_100, Speaker::Me, ECHOED);
        assert_eq!(
            rig.live.final_done(&chunk(0, 1_100, 2_000), vec![me.clone()]),
            vec![Out::ClearInterim { channel: 0 }, Out::Final(me)]
        );
    }

    #[test]
    fn peserta_lines_older_than_30_s_no_longer_count_as_the_echo_source() {
        let mut rig = pair();
        rig.wait(6_000);
        rig.live.final_done(&chunk(1, 1_000, 2_000), vec![seg(1_000, 3_000, Speaker::Others, ECHOED)]);
        rig.live.final_done(&chunk(1, 40_000, 1_000), vec![seg(40_000, 41_000, Speaker::Others, "sip")]);
        assert_eq!(rig.live.others_recent.len(), 1);
    }

    #[test]
    fn an_interim_of_the_open_chunk_is_shown() {
        let mut rig = Rig::new(vec![None]);
        rig.play(0, &tone(2_000));
        let Some(Work::Interim(open)) = rig.live.next_work(2_000) else { panic!() };
        assert_eq!(
            rig.live.interim_done(&open, " jadi deadline ".into()),
            vec![Out::Interim { channel: 0, text: "jadi deadline".into() }]
        );
        assert_eq!(rig.live.interim_done(&open, "  ".into()), vec![], "nothing heard yet");
    }

    #[test]
    fn a_stale_interim_is_ignored() {
        let mut rig = Rig::new(vec![None]);
        rig.play(0, &tone(2_000));
        let Some(Work::Interim(open)) = rig.live.next_work(2_000) else { panic!() };
        // The sentence ended while the engine ran on the interim.
        rig.wait(1_500);
        assert_eq!(rig.live.interim_done(&open, "jadi".into()), vec![]);
        // And the next sentence is another chunk.
        rig.play(0, &tone(2_000));
        assert_eq!(rig.live.interim_done(&open, "jadi".into()), vec![]);
        assert_eq!(rig.live.interim_done(&chunk(0, 123_456, 1_000), "jadi".into()), vec![]);
    }
}
