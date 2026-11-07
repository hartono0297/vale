//! Streaming sample-rate conversion (mono, `f32`) and the mono down-mix.
//!
//! The resampler is a windowed-sinc interpolator that keeps its state across
//! calls, so chunk boundaries do not show. Output positions are computed
//! from integer counters (no drift). The signal comes out delayed by a
//! fixed `HALF_WIDTH` source frames (well under a millisecond) because the
//! filter is primed with that many zeros; in exchange the output length is
//! `input * to / from` to within one frame at every point of the stream.
// The recorder session (T3) uses these; until it lands they look unused.
#![allow(dead_code)]

use std::f64::consts::PI;

/// Filter half-width in zero crossings of the sinc (per side).
const HALF_WIDTH: f64 = 16.0;

/// Converts a mono stream from one rate to another, chunk by chunk.
pub struct Resampler {
    from: u64,
    to: u64,
    /// Sinc cutoff relative to the source Nyquist: 1 when upsampling, less
    /// when downsampling so nothing above the new Nyquist aliases.
    cutoff: f64,
    /// Kernel reach in source frames on each side.
    reach: i64,
    /// Source frames not yet released; `buf[0]` is padded index `base`.
    buf: Vec<f32>,
    base: i64,
    /// Output frames produced so far.
    produced: u64,
}

impl Resampler {
    pub fn new(from_hz: u32, to_hz: u32) -> Self {
        let (from, to) = (u64::from(from_hz.max(1)), u64::from(to_hz.max(1)));
        let cutoff = (to as f64 / from as f64).min(1.0);
        let reach = (HALF_WIDTH / cutoff).ceil() as i64;
        let mut r = Self { from, to, cutoff, reach, buf: Vec::new(), base: 0, produced: 0 };
        if from != to {
            r.buf = vec![0.0; reach as usize];
        }
        r
    }

    /// Feeds source frames and returns every output frame that is now
    /// complete. Equal rates pass the input through untouched.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.from == self.to {
            return input.to_vec();
        }
        self.buf.extend_from_slice(input);
        let end = self.base + self.buf.len() as i64;
        let mut out = Vec::with_capacity((input.len() as u64 * self.to / self.from) as usize + 2);
        loop {
            let num = self.produced * self.from;
            let center = (num / self.to) as i64;
            if center + self.reach >= end {
                break;
            }
            let frac = (num % self.to) as f64 / self.to as f64;
            out.push(self.interpolate(center, frac));
            self.produced += 1;
        }
        // Frames left of the next output's kernel are never needed again.
        let next_center = (self.produced * self.from / self.to) as i64;
        let keep_from = (next_center - self.reach + 1).clamp(self.base, end);
        self.buf.drain(..(keep_from - self.base) as usize);
        self.base = keep_from;
        out
    }

    /// One output sample at source position `center + frac`.
    fn interpolate(&self, center: i64, frac: f64) -> f32 {
        let reach = self.reach as f64;
        let (mut acc, mut norm) = (0.0f64, 0.0f64);
        for j in (center - self.reach + 1)..=(center + self.reach) {
            let d = center as f64 + frac - j as f64;
            let x = PI * self.cutoff * d;
            let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
            let window = 0.5 * (1.0 + (PI * d / reach).cos());
            let w = self.cutoff * sinc * window;
            norm += w;
            if j >= self.base {
                acc += w * f64::from(self.buf[(j - self.base) as usize]);
            }
        }
        (acc / norm) as f32
    }
}

/// Averages interleaved frames of `channels` channels into one mono channel.
/// A trailing partial frame is ignored; 0 or 1 channels return a copy.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let inv = 1.0 / channels as f32;
    interleaved.chunks_exact(channels).map(|f| f.iter().sum::<f32>() * inv).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, rate: u32, frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .map(|i| amp * (2.0 * PI * hz * i as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    /// Feeds `x` in chunks of the given sizes (cycled) and joins the output.
    fn run_chunked(r: &mut Resampler, x: &[f32], sizes: &[usize]) -> Vec<f32> {
        let mut out = Vec::new();
        let (mut at, mut i) = (0, 0);
        while at < x.len() {
            let n = sizes[i % sizes.len()].min(x.len() - at);
            out.extend(r.process(&x[at..at + n]));
            at += n;
            i += 1;
        }
        out
    }

    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
    }

    #[test]
    fn equal_rates_pass_through_exactly() {
        let x = sine(440.0, 48_000, 1000, 0.5);
        let mut r = Resampler::new(48_000, 48_000);
        assert_eq!(r.process(&x[..333]), x[..333]);
        assert_eq!(r.process(&x[333..]), x[333..]);
    }

    #[test]
    fn length_ratio_holds_within_one_frame_across_chunks() {
        for (from, to) in [(44_100u32, 48_000u32), (16_000, 48_000), (96_000, 48_000), (48_000, 44_100)] {
            let mut r = Resampler::new(from, to);
            let (mut fed, mut got) = (0usize, 0usize);
            for k in 0..400 {
                let n = 1 + (k * 37) % 211; // odd, small chunks
                got += r.process(&vec![0.1; n]).len();
                fed += n;
                let want = fed as f64 * f64::from(to) / f64::from(from);
                assert!((got as f64 - want).abs() <= 1.0, "{from}->{to} after {fed}: {got} vs {want}");
            }
        }
    }

    #[test]
    fn a_sine_keeps_its_frequency_and_amplitude() {
        for (from, to) in [(44_100u32, 48_000u32), (16_000, 48_000), (96_000, 48_000)] {
            let hz = 1000.0;
            let x = sine(hz, from, from as usize, 0.8); // one second
            let mut r = Resampler::new(from, to);
            let y = run_chunked(&mut r, &x, &[441, 100, 997]);
            // Skip the start-up (the onset rings) and the last frames.
            let mid = &y[200..y.len() - 200];
            let n = crossings(mid);
            let want = mid.len() as f64 * hz / f64::from(to);
            assert!((n as f64 - want).abs() <= 1.0, "{from}->{to}: {n} crossings, want {want}");
            let peak = mid.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!((peak - 0.8).abs() < 0.8 * 0.05, "{from}->{to}: peak {peak}");
        }
    }

    #[test]
    fn chunked_equals_one_shot() {
        let x = sine(300.0, 44_100, 9_000, 0.6);
        let one = Resampler::new(44_100, 48_000).process(&x);
        let mut r = Resampler::new(44_100, 48_000);
        let many = run_chunked(&mut r, &x, &[1, 7, 480, 64, 2000]);
        assert_eq!(one, many);
    }

    #[test]
    fn dc_stays_dc() {
        let mut r = Resampler::new(16_000, 48_000);
        let y = r.process(&vec![0.5; 4_000]);
        assert!(y[200..].iter().all(|s| (s - 0.5).abs() < 1e-3));
    }

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix(&[1.0, 3.0, 2.0, 4.0], 2), vec![2.0, 3.0]);
        assert_eq!(downmix(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 3), vec![2.0, 5.0]);
        assert_eq!(downmix(&[0.1, 0.2], 1), vec![0.1, 0.2]);
        assert_eq!(downmix(&[1.0, 2.0, 3.0], 2), vec![1.5]);
        assert!(downmix(&[], 2).is_empty());
    }
}
