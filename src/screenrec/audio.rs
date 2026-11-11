//! The audio side of a screen recording: what track A's capture pushes in
//! ([`AudioInput`]), the queue that carries it to the recording thread, and
//! the clock that stamps it in the video's time.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// The only rate the recorder takes (the recording contract's rate).
pub const SAMPLE_RATE: u32 = 48_000;

/// Where audio goes into a screen recording. Track A's capture sink calls
/// it on its capture thread; it only copies and queues, never blocks on the
/// encoder.
pub trait AudioInput: Send + Sync {
    /// `frames`: interleaved f32 samples (-1..1), `channels` per frame, at
    /// `sample_rate` (must be 48 000; other rates are counted as rejected
    /// and dropped). `qpc_hns`: the QPC clock at the first frame in 100 ns
    /// units, the unit WASAPI's `GetBuffer` reports (`u64QPCPosition`);
    /// [`crate::recorder::capture::qpc_hns`] reads the same clock. Audio captured inside a
    /// pause is dropped (the contract: a pause leaves no silence).
    fn push_audio(&self, frames: &[f32], channels: u16, sample_rate: u32, qpc_hns: i64);
}

/// `frames` (interleaved, `from` channels) as 16-bit PCM with `to` channels:
/// mono is copied to both sides, stereo is averaged into mono, extra
/// channels are dropped. Out-of-range samples are clipped.
pub fn to_pcm16(frames: &[f32], from: u16, to: u16) -> Vec<i16> {
    let (from, to) = (from.max(1) as usize, to.max(1) as usize);
    let q = |s: f32| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    let count = frames.len() / from;
    let mut out = Vec::with_capacity(count * to);
    for frame in frames.chunks_exact(from) {
        if to == 1 {
            let take = from.min(2);
            let sum: f32 = frame[..take].iter().sum();
            out.push(q(sum / take as f32));
        } else {
            for c in 0..to {
                out.push(q(frame[c.min(from - 1)]));
            }
        }
    }
    out
}

/// One pushed chunk, already in the video's format.
pub struct Chunk {
    /// 100 ns units of the QPC clock.
    pub qpc: i64,
    pub pcm: Vec<i16>,
}

/// The queue between the audio thread and the recording thread.
pub struct AudioFeed {
    channels: u16,
    /// At most this many frames wait; older ones are dropped first.
    max_frames: usize,
    queue: Mutex<(VecDeque<Chunk>, usize)>,
    dropped_frames: AtomicU64,
    rejected_chunks: AtomicU64,
}

impl AudioFeed {
    pub fn new(channels: u16) -> AudioFeed {
        AudioFeed {
            channels: channels.clamp(1, 2),
            max_frames: SAMPLE_RATE as usize * 2,
            queue: Mutex::new((VecDeque::new(), 0)),
            dropped_frames: AtomicU64::new(0),
            rejected_chunks: AtomicU64::new(0),
        }
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Everything queued so far, oldest first.
    pub fn drain(&self) -> Vec<Chunk> {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.1 = 0;
        q.0.drain(..).collect()
    }

    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    pub fn rejected_chunks(&self) -> u64 {
        self.rejected_chunks.load(Ordering::Relaxed)
    }
}

impl AudioInput for AudioFeed {
    fn push_audio(&self, frames: &[f32], channels: u16, sample_rate: u32, qpc_hns: i64) {
        if sample_rate != SAMPLE_RATE || channels == 0 {
            self.rejected_chunks.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let pcm = to_pcm16(frames, channels, self.channels);
        if pcm.is_empty() {
            return;
        }
        let n = pcm.len() / self.channels as usize;
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        q.0.push_back(Chunk { qpc: qpc_hns, pcm });
        q.1 += n;
        while q.1 > self.max_frames {
            let Some(old) = q.0.pop_front() else { break };
            let gone = old.pcm.len() / self.channels as usize;
            q.1 -= gone;
            self.dropped_frames
                .fetch_add(gone as u64, Ordering::Relaxed);
        }
    }
}

/// Stamps audio in video time. Chunks follow each other sample by sample
/// from an anchor (no jitter from the capture thread's wake-ups); when the
/// chunk's own clock drifts more than [`AudioClock::SLACK_HNS`] from that, or
/// after a resume, it anchors again.
#[derive(Clone, Copy, Debug, Default)]
pub struct AudioClock {
    anchor: Option<(i64, u64)>,
}

impl AudioClock {
    pub const SLACK_HNS: i64 = 1_000_000;

    /// Forget the anchor (after a resume).
    pub fn reset(&mut self) {
        self.anchor = None;
    }

    /// The time (100 ns) of a chunk of `frames` frames whose first frame the
    /// capture clock put at `measured`.
    pub fn place(&mut self, measured: i64, frames: u64) -> i64 {
        let at = match self.anchor {
            Some((start, done)) => {
                let expected = start + (done as i128 * 10_000_000 / SAMPLE_RATE as i128) as i64;
                if (expected - measured).abs() <= Self::SLACK_HNS {
                    self.anchor = Some((start, done + frames));
                    return expected;
                }
                measured
            }
            None => measured,
        };
        self.anchor = Some((at, frames));
        at
    }
}

/// The length of `frames` frames at 48 kHz, in 100 ns.
pub fn frames_hns(frames: u64) -> i64 {
    (frames as i128 * 10_000_000 / SAMPLE_RATE as i128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_passes_through_as_pcm() {
        let pcm = to_pcm16(&[0.0, 1.0, -1.0, 0.5, 2.0, -3.0], 2, 2);
        assert_eq!(pcm, vec![0, 32767, -32767, 16384, 32767, -32767]);
    }

    #[test]
    fn mono_goes_to_both_sides_and_stereo_to_mono() {
        assert_eq!(
            to_pcm16(&[0.5, -0.5], 1, 2),
            vec![16384, 16384, -16384, -16384]
        );
        assert_eq!(to_pcm16(&[1.0, 0.0, 0.5, 0.5], 2, 1), vec![16384, 16384]);
        // Extra channels are dropped; a broken last frame is ignored.
        assert_eq!(to_pcm16(&[0.0, 1.0, 1.0, 0.25], 3, 2), vec![0, 32767]);
    }

    #[test]
    fn the_feed_rejects_other_rates_and_keeps_two_seconds() {
        let feed = AudioFeed::new(2);
        feed.push_audio(&[0.0; 960], 2, 44_100, 0);
        assert_eq!(feed.rejected_chunks(), 1);
        assert!(feed.drain().is_empty());
        // 3 s of 10 ms chunks: the oldest second goes.
        for i in 0..300 {
            feed.push_audio(&[0.0; 960], 2, SAMPLE_RATE, i);
        }
        let left = feed.drain();
        assert_eq!(left.len(), 200);
        assert_eq!(left[0].qpc, 100);
        assert_eq!(feed.dropped_frames(), 48_000);
        assert!(feed.drain().is_empty());
    }

    #[test]
    fn the_clock_runs_on_samples_and_reanchors_on_drift() {
        let mut c = AudioClock::default();
        assert_eq!(c.place(1_000_000, 480), 1_000_000);
        // 10 ms later by the samples, whatever the jittery measurement says.
        assert_eq!(c.place(1_130_000, 480), 1_100_000);
        assert_eq!(c.place(1_150_000, 480), 1_200_000);
        // A 2 s jump (a capture gap): anchors again.
        assert_eq!(c.place(21_300_000, 480), 21_300_000);
        assert_eq!(c.place(21_400_500, 480), 21_400_000);
        c.reset();
        assert_eq!(c.place(50_000_000, 480), 50_000_000);
        assert_eq!(frames_hns(48_000), 10_000_000);
    }
}
