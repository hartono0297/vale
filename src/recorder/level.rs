//! Input level for the two REC bars: a peak meter that decays over 300 ms,
//! and two atomics the capture threads write and the UI reads without waiting.
// The recorder session (T3) uses these; until it lands they look unused.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Time for a full-scale peak to fall to zero.
const DECAY: Duration = Duration::from_millis(300);

/// Tracks the recent peak of a sample stream, `0.0..=1.0`, falling linearly
/// to zero over 300 ms.
#[derive(Clone, Debug)]
pub struct PeakMeter {
    peak: f32,
    at: Instant,
}

impl Default for PeakMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl PeakMeter {
    pub fn new() -> Self {
        Self { peak: 0.0, at: Instant::now() }
    }

    /// The held peak decayed to `now`.
    fn decayed(&self, now: Instant) -> f32 {
        let dt = now.saturating_duration_since(self.at).as_secs_f32();
        (self.peak * (1.0 - dt / DECAY.as_secs_f32())).max(0.0)
    }

    /// Feeds samples (any channel layout) as of the current time.
    pub fn feed(&mut self, samples: &[f32]) {
        self.feed_at(samples, Instant::now());
    }

    /// Feeds samples as of `now`; the new level is the larger of the decayed
    /// old one and the peak of `samples`.
    pub fn feed_at(&mut self, samples: &[f32], now: Instant) {
        let peak = samples.iter().fold(0.0f32, |m, s| if s.is_nan() { m } else { m.max(s.abs()) });
        self.peak = self.decayed(now).max(peak.min(1.0));
        self.at = now;
    }

    /// The level at `now`, `0.0..=1.0`.
    pub fn level(&self, now: Instant) -> f32 {
        self.decayed(now)
    }
}

/// The two levels the UI polls, as `f32` bits in atomics.
#[derive(Debug, Default)]
pub struct SharedLevels {
    mic: AtomicU32,
    system: AtomicU32,
}

impl SharedLevels {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_mic(&self, v: f32) {
        self.mic.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn set_system(&self, v: f32) {
        self.system.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn mic(&self) -> f32 {
        f32::from_bits(self.mic.load(Ordering::Relaxed))
    }

    pub fn system(&self) -> f32 {
        f32::from_bits(self.system.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn silence_is_zero() {
        let mut m = PeakMeter::new();
        let t = Instant::now();
        m.feed_at(&[0.0; 100], t);
        assert_eq!(m.level(t), 0.0);
    }

    #[test]
    fn takes_the_absolute_peak() {
        let mut m = PeakMeter::new();
        let t = Instant::now();
        m.feed_at(&[0.1, -0.6, 0.3], t);
        assert!((m.level(t) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn clamps_above_full_scale_and_ignores_nan() {
        let mut m = PeakMeter::new();
        let t = Instant::now();
        m.feed_at(&[f32::NAN, 2.5], t);
        assert_eq!(m.level(t), 1.0);
    }

    #[test]
    fn decays_to_zero_over_300_ms() {
        let mut m = PeakMeter::new();
        let t = Instant::now();
        m.feed_at(&[1.0], t);
        assert!((m.level(t + ms(150)) - 0.5).abs() < 0.01);
        assert_eq!(m.level(t + ms(300)), 0.0);
        assert_eq!(m.level(t + ms(900)), 0.0);
    }

    #[test]
    fn a_quieter_feed_does_not_lower_a_held_peak() {
        let mut m = PeakMeter::new();
        let t = Instant::now();
        m.feed_at(&[0.8], t);
        m.feed_at(&[0.1], t + ms(30));
        let held = m.level(t + ms(30));
        assert!((held - 0.72).abs() < 0.01, "{held}");
        m.feed_at(&[0.9], t + ms(60));
        assert!((m.level(t + ms(60)) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn shared_levels_round_trip() {
        let s = SharedLevels::new();
        assert_eq!((s.mic(), s.system()), (0.0, 0.0));
        s.set_mic(0.25);
        s.set_system(0.75);
        assert_eq!((s.mic(), s.system()), (0.25, 0.75));
    }
}
