//! Streaming sample-rate conversion (mono, `f32`). The down-mix of a capture
//! packet to mono is `eq_audio::mix_to_mono`, shared with the equalizer.
//!
//! The resampler is a windowed-sinc interpolator that keeps its state across
//! calls, so chunk boundaries do not show. Output positions are computed
//! from integer counters (no drift). The signal comes out delayed by the
//! kernel's reach (`HALF_WIDTH` source frames when upsampling, more when
//! downsampling; well under a millisecond, see [`Resampler::delay`]) because
//! the filter is primed with that many zeros; in exchange the output length
//! is `input * to / from` to within one frame at every point of the stream.

use std::f64::consts::PI;

/// Filter half-width in zero crossings of the sinc (per side).
const HALF_WIDTH: f64 = 16.0;
/// A rate pair needing more phases than this (rates that share almost no
/// factor) computes each tap instead of keeping a table.
const MAX_PHASES: u64 = 1024;

/// Converts a mono stream from one rate to another, chunk by chunk.
pub struct Resampler {
    from: u64,
    to: u64,
    /// Sinc cutoff relative to the source Nyquist: 1 when upsampling, less
    /// when downsampling so nothing above the new Nyquist aliases.
    cutoff: f64,
    /// Kernel reach in source frames on each side.
    reach: i64,
    /// `gcd(from, to)`: output positions fall on multiples of `g / to`.
    g: u64,
    /// The normalized kernel per phase (`to / g` phases of `2 * reach`
    /// taps), or empty for the direct path.
    table: Vec<f32>,
    /// Source frames not yet released; `buf[0]` is padded index `base`.
    buf: Vec<f32>,
    base: i64,
    /// Output frames produced so far.
    produced: u64,
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

impl Resampler {
    /// The ratio is exact (`from / to` reduced by their gcd, 44100 to 48000 is
    /// 147/160), so every output frame falls on one of `to / gcd` phases; the
    /// kernel of each is computed once here.
    pub fn new(from_hz: u32, to_hz: u32) -> Self {
        let mut r = Self::direct(from_hz, to_hz);
        let phases = r.to / r.g;
        if r.from != r.to && phases <= MAX_PHASES {
            let width = 2 * r.reach as usize;
            r.table = Vec::with_capacity(phases as usize * width);
            for p in 0..phases {
                let frac = p as f64 / phases as f64;
                let w: Vec<f64> = (0..width).map(|t| r.weight(frac + (r.reach - 1) as f64 - t as f64)).collect();
                let norm: f64 = w.iter().sum();
                r.table.extend(w.iter().map(|x| (x / norm) as f32));
            }
        }
        r
    }

    /// Without the phase table: each tap is computed per output frame.
    pub(crate) fn direct(from_hz: u32, to_hz: u32) -> Self {
        let (from, to) = (u64::from(from_hz.max(1)), u64::from(to_hz.max(1)));
        let cutoff = (to as f64 / from as f64).min(1.0);
        let reach = (HALF_WIDTH / cutoff).ceil() as i64;
        let mut r = Self {
            from,
            to,
            cutoff,
            reach,
            g: gcd(from, to),
            table: Vec::new(),
            buf: Vec::new(),
            base: 0,
            produced: 0,
        };
        if from != to {
            r.buf = vec![0.0; reach as usize];
        }
        r
    }

    /// The number of kernels in the table, `None` on the direct path.
    #[cfg(test)]
    fn phases(&self) -> Option<usize> {
        (!self.table.is_empty()).then(|| self.table.len() / (2 * self.reach as usize))
    }

    /// How far the output lags the input, in output frames: output frame
    /// `j` holds the signal at input time `j - delay()` (output frames).
    /// Zero when the rates are equal.
    pub fn delay(&self) -> f64 {
        if self.from == self.to {
            0.0
        } else {
            self.reach as f64 * self.to as f64 / self.from as f64
        }
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
            let rem = num % self.to;
            out.push(if self.table.is_empty() {
                self.interpolate(center, rem as f64 / self.to as f64)
            } else {
                self.from_table(center, (rem / self.g) as usize)
            });
            self.produced += 1;
        }
        // Frames left of the next output's kernel are never needed again.
        let next_center = (self.produced * self.from / self.to) as i64;
        let keep_from = (next_center - self.reach + 1).clamp(self.base, end);
        self.buf.drain(..(keep_from - self.base) as usize);
        self.base = keep_from;
        out
    }

    /// The windowed-sinc tap for a source frame `d` frames before the
    /// output position.
    fn weight(&self, d: f64) -> f64 {
        let x = PI * self.cutoff * d;
        let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
        let window = 0.5 * (1.0 + (PI * d / self.reach as f64).cos());
        self.cutoff * sinc * window
    }

    /// One output sample at source position `center + frac`.
    fn interpolate(&self, center: i64, frac: f64) -> f32 {
        let (mut acc, mut norm) = (0.0f64, 0.0f64);
        for j in (center - self.reach + 1)..=(center + self.reach) {
            let w = self.weight(center as f64 + frac - j as f64);
            norm += w;
            if j >= self.base {
                acc += w * f64::from(self.buf[(j - self.base) as usize]);
            }
        }
        (acc / norm) as f32
    }

    /// `interpolate` with the kernel of `phase` from the table.
    fn from_table(&self, center: i64, phase: usize) -> f32 {
        let width = 2 * self.reach as usize;
        let taps = &self.table[phase * width..(phase + 1) * width];
        let first = center - self.reach + 1;
        // Frames before `base` are the zero padding, already released.
        let skip = (self.base - first).max(0) as usize;
        let start = (first + skip as i64 - self.base) as usize;
        taps[skip..]
            .iter()
            .zip(&self.buf[start..])
            .map(|(w, x)| w * x)
            .sum()
    }
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
    fn the_phase_table_matches_the_direct_formula() {
        for (from, to, phases) in [
            (44_100u32, 48_000u32, Some(160usize)),
            (48_000, 44_100, Some(147)),
            (16_000, 48_000, Some(3)),
            (96_000, 48_000, Some(1)),
            (22_050, 48_000, Some(320)),
            // 48001 and 48000 share no factor: 48000 phases, the direct path.
            (48_001, 48_000, None),
        ] {
            let table = Resampler::new(from, to);
            assert_eq!(table.phases(), phases, "{from}->{to}");
            let x = sine(997.0, from, 6_000, 0.7);
            let (mut a, mut b) = (table, Resampler::direct(from, to));
            let ya = run_chunked(&mut a, &x, &[441, 13, 1000]);
            let yb = run_chunked(&mut b, &x, &[441, 13, 1000]);
            assert_eq!(ya.len(), yb.len());
            for (i, (p, q)) in ya.iter().zip(&yb).enumerate() {
                assert!((p - q).abs() < 1e-5, "{from}->{to} frame {i}: {p} vs {q}");
            }
        }
    }

    #[test]
    fn delay_says_where_an_impulse_lands() {
        assert_eq!(Resampler::new(48_000, 48_000).delay(), 0.0);
        for (from, to) in [(44_100u32, 48_000u32), (16_000, 48_000), (96_000, 48_000)] {
            let mut r = Resampler::new(from, to);
            let at = 1000usize; // input frame of the impulse
            let mut x = vec![0.0f32; 4000];
            x[at] = 1.0;
            let y = r.process(&x);
            let peak = (0..y.len()).max_by(|&a, &b| y[a].total_cmp(&y[b])).unwrap();
            let want = at as f64 * f64::from(to) / f64::from(from) + r.delay();
            assert!((peak as f64 - want).abs() <= 1.0, "{from}->{to}: peak {peak}, want {want}");
        }
    }
}
