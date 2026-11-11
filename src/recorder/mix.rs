//! The recorder's mix mode: the sources summed into one mono stream that
//! feeds the screen recorder (`screenrec::audio::Forward`) instead of a file.

// Wired into the screen recorder by the next task.
#![cfg_attr(not(test), allow(dead_code))]

use super::{RATE, Sources};

/// Where the soft limiter starts to bend.
pub const KNEE: f32 = 0.7;

/// Passes `x` below [`KNEE`] and bends what is above it towards 1 without
/// ever reaching it (a tanh shoulder), so two loud sources do not clip.
pub fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= KNEE {
        return x;
    }
    let bent = KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh();
    bent.copysign(x)
}

/// Each frame's channels summed into one sample, then soft limited.
pub fn mix_mono(frames: &[f32], channels: usize) -> Vec<f32> {
    frames
        .chunks_exact(channels.max(1))
        .map(|frame| soft_limit(frame.iter().sum()))
        .collect()
}

/// The time of the next mono sample the recorder pushes, on the capture
/// clock (QPC, 100 ns): the recording's origin plus the timeline frames
/// taken so far, sleep included.
#[derive(Clone, Copy, Debug)]
pub struct MixClock {
    origin: i64,
    raw: u64,
}

impl MixClock {
    pub fn new(origin_hns: i64) -> MixClock {
        MixClock {
            origin: origin_hns,
            raw: 0,
        }
    }

    pub fn stamp(&self) -> i64 {
        self.origin + (i128::from(self.raw) * 10_000_000 / i128::from(RATE)) as i64
    }

    /// `frames` were pushed.
    pub fn advance(&mut self, frames: u64) {
        self.raw += frames;
    }

    /// `frames` of timeline were cut out (the PC slept): nothing is pushed for
    /// them, but the stamps move on.
    pub fn skip(&mut self, frames: u64) {
        self.raw += frames;
    }
}

/// Takes the mono mix, each chunk with the capture time of its first frame.
pub trait MixSink: Send {
    fn mix(&mut self, mono: &[f32], qpc_hns: i64);
}

/// What a mix recording captures.
#[derive(Clone, Debug)]
pub struct MixOptions {
    pub sources: Sources,
    /// The endpoint each source starts on, `[mic, system]` (`None`: follow
    /// the Windows default).
    pub devices: [Option<String>; 2],
    /// Ask Windows to cancel the echo of the speakers on the mic stream.
    pub echo_cancel: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_limit_leaves_the_normal_range_alone() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.7), -0.7);
        assert_eq!(soft_limit(0.0), 0.0);
    }

    #[test]
    fn soft_limit_bends_above_the_knee_and_never_clips() {
        assert!((soft_limit(1.0) - 0.9285).abs() < 1e-3);
        assert!(soft_limit(2.0) < 1.0);
        assert!(soft_limit(8.0) <= 1.0);
        // f32 stops telling inputs apart where tanh saturates (about 2.7
        // here): strictly rising up to 2, never falling beyond.
        let mut prev = soft_limit(-2.0);
        for i in -199..=200 {
            let y = soft_limit(i as f32 * 0.01);
            assert!(y > prev, "not rising at {}", i as f32 * 0.01);
            prev = y;
        }
        let mut prev = soft_limit(-4.0);
        for i in -399..=400 {
            let y = soft_limit(i as f32 * 0.01);
            assert!(y >= prev, "falling at {}", i as f32 * 0.01);
            prev = y;
        }
        for i in 0..=800 {
            let x = i as f32 * 0.01;
            assert_eq!(soft_limit(-x), -soft_limit(x));
        }
    }

    #[test]
    fn a_one_sided_passage_keeps_its_level() {
        assert_eq!(mix_mono(&[0.5, 0.0, 0.5, 0.0], 2), [0.5, 0.5]);
        assert_eq!(mix_mono(&[0.0, 0.3], 2), [0.3]);
    }

    #[test]
    fn both_sides_add_up_under_the_limit() {
        assert_eq!(mix_mono(&[0.5, 0.5], 2), [soft_limit(1.0)]);
    }

    #[test]
    fn mono_passes_through() {
        assert_eq!(mix_mono(&[0.2, -0.4], 1), [0.2, -0.4]);
    }

    #[test]
    fn the_mix_clock_counts_taken_frames_and_skipped_sleep() {
        let mut c = MixClock::new(1_000);
        c.advance(48_000);
        assert_eq!(c.stamp(), 10_001_000);
        c.skip(96_000);
        assert_eq!(c.stamp(), 30_001_000);
    }
}
