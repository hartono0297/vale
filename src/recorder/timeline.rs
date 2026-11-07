//! The shared recording timeline. Packets from the mic and the loopback
//! arrive on different clocks and loopback sends nothing in silence, so each
//! packet is placed by its position (frames at the output rate) instead of
//! being appended. The writer then takes fixed blocks up to a frame: gaps
//! read as zeros, late samples and overlaps are dropped (the first write
//! wins), and paused spans are cut out of the timeline.
//!
//! Frames passed to `put`, `take`, `pause_at` and `resume_at` are "raw"
//! (as measured from the recording origin, pauses included); what `take`
//! returns is the paused-span-free stream.
// The recorder session (T3) uses these; until it lands they look unused.
#![allow(dead_code)]

use std::collections::VecDeque;

/// A write further ahead of the read point than this is dropped, which
/// bounds memory if a device reports a wild timestamp.
const MAX_AHEAD_SECS: i64 = 10;

/// Stands for "still paused" as the end of an open span.
const OPEN: i64 = i64::MAX / 4;

/// Converts a QPC reading to a frame at `rate`, relative to `origin`, with
/// 128-bit maths (QPC ticks times the rate overflow 64 bits within hours)
/// and rounding to the nearest frame (halves round up).
pub fn qpc_to_frame(qpc: u64, qpc_freq: u64, origin: u64, rate: u32) -> i64 {
    let ticks = i128::from(qpc) - i128::from(origin);
    let freq = i128::from(qpc_freq.max(1));
    let num = ticks * i128::from(rate);
    (2 * num + freq).div_euclid(2 * freq) as i64
}

#[derive(Default)]
struct Lane {
    samples: VecDeque<f32>,
    set: VecDeque<bool>,
}

pub struct Timeline {
    rate: u32,
    lanes: Vec<Lane>,
    /// First frame not yet taken (in the paused-span-free timeline).
    read: i64,
    /// Paused spans in raw frames, sorted, `[start, end)`; the last may be open.
    spans: Vec<(i64, i64)>,
}

impl Timeline {
    pub fn new(rate: u32, channels: usize) -> Self {
        Self {
            rate,
            lanes: (0..channels.max(1)).map(|_| Lane::default()).collect(),
            read: 0,
            spans: Vec::new(),
        }
    }

    pub fn channels(&self) -> usize {
        self.lanes.len()
    }

    /// `qpc_to_frame` at this timeline's rate.
    pub fn qpc_to_frame(&self, qpc: u64, qpc_freq: u64, origin: u64) -> i64 {
        qpc_to_frame(qpc, qpc_freq, origin, self.rate)
    }

    /// Frames held in memory (the longest lane), for tests and diagnostics.
    pub fn buffered_frames(&self) -> usize {
        self.lanes.iter().map(|l| l.samples.len()).max().unwrap_or(0)
    }

    /// Starts removing time from `frame` on. Ignored while already paused.
    pub fn pause_at(&mut self, frame: i64) {
        if self.spans.last().is_some_and(|s| s.1 == OPEN) {
            return;
        }
        let start = self.spans.last().map_or(frame, |s| frame.max(s.1));
        self.spans.push((start, OPEN));
    }

    /// Ends the open pause at `frame`. Ignored when not paused.
    pub fn resume_at(&mut self, frame: i64) {
        let Some(last) = self.spans.last_mut() else { return };
        if last.1 != OPEN {
            return;
        }
        if frame <= last.0 {
            self.spans.pop();
        } else {
            last.1 = frame;
        }
    }

    /// Splits the raw range `[s, e)` into the parts outside every paused
    /// span, each as `(raw_start, raw_end, shift)`; `raw - shift` is the
    /// position on the timeline.
    fn pieces(&self, s: i64, e: i64) -> Vec<(i64, i64, i64)> {
        let mut out = Vec::new();
        let (mut cur, mut shift) = (s, 0i64);
        for &(a, b) in &self.spans {
            if b <= cur {
                shift += b - a;
                continue;
            }
            if a >= e {
                break;
            }
            if a > cur {
                out.push((cur, a, shift));
            }
            shift += b - a;
            cur = cur.max(b);
            if cur >= e {
                return out;
            }
        }
        if cur < e {
            out.push((cur, e, shift));
        }
        out
    }

    /// Timeline position of raw frame `f`: `f` minus the paused time before it.
    fn map(&self, f: i64) -> i64 {
        let removed: i64 = self.spans.iter().map(|&(a, b)| (b.min(f) - a).max(0)).sum();
        f - removed
    }

    /// Places `samples` (one channel) starting at raw frame `start_frame`.
    /// Parts inside a pause, before the read point, past 10 s ahead of it, or
    /// on frames this channel already holds are dropped.
    pub fn put(&mut self, channel: usize, start_frame: i64, samples: &[f32]) {
        if channel >= self.lanes.len() || samples.is_empty() {
            return;
        }
        let end_frame = start_frame.saturating_add(samples.len() as i64);
        let limit = self.read + MAX_AHEAD_SECS * i64::from(self.rate);
        for (rs, re, shift) in self.pieces(start_frame, end_frame) {
            let lo = (rs - shift).max(self.read);
            let hi = (re - shift).min(limit);
            let lane = &mut self.lanes[channel];
            for frame in lo..hi {
                let src = (frame + shift - start_frame) as usize;
                let off = (frame - self.read) as usize;
                if lane.samples.len() <= off {
                    lane.samples.resize(off + 1, 0.0);
                    lane.set.resize(off + 1, false);
                }
                if !lane.set[off] {
                    lane.samples[off] = samples[src];
                    lane.set[off] = true;
                }
            }
        }
    }

    /// Takes every frame before raw frame `up_to_frame` that was not taken
    /// yet, channels interleaved. Unwritten frames are zeros. Frames older
    /// than this are freed, and later writes to them are dropped.
    pub fn take(&mut self, up_to_frame: i64) -> Vec<f32> {
        let target = self.map(up_to_frame);
        if target <= self.read {
            return Vec::new();
        }
        let n = (target - self.read) as usize;
        let ch = self.lanes.len();
        let mut out = vec![0.0f32; n * ch];
        for (c, lane) in self.lanes.iter_mut().enumerate() {
            let have = lane.samples.len().min(n);
            for i in 0..have {
                if lane.set[i] {
                    out[i * ch + c] = lane.samples[i];
                }
            }
            lane.samples.drain(..have);
            lane.set.drain(..have);
        }
        self.read = target;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 1000; // small numbers keep the tests readable

    fn one() -> Timeline {
        Timeline::new(RATE, 1)
    }

    fn ramp(from: i64, n: usize) -> Vec<f32> {
        (0..n).map(|i| (from + i as i64) as f32).collect()
    }

    #[test]
    fn contiguous_writes_come_out_in_order() {
        let mut t = one();
        t.put(0, 0, &ramp(1, 5));
        t.put(0, 5, &ramp(6, 5));
        assert_eq!(t.take(10), ramp(1, 10));
    }

    #[test]
    fn a_gap_reads_as_zeros() {
        let mut t = one();
        t.put(0, 0, &[1.0, 1.0]);
        t.put(0, 6, &[2.0, 2.0]);
        assert_eq!(t.take(8), vec![1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 2.0]);
    }

    #[test]
    fn take_never_goes_past_the_requested_frame() {
        let mut t = one();
        t.put(0, 0, &ramp(1, 10));
        assert_eq!(t.take(4), ramp(1, 4));
        assert_eq!(t.take(4), Vec::<f32>::new());
        assert_eq!(t.take(2), Vec::<f32>::new());
        assert_eq!(t.take(10), ramp(5, 6));
    }

    #[test]
    fn take_past_the_data_pads_with_zeros() {
        let mut t = one();
        t.put(0, 0, &[1.0, 2.0]);
        assert_eq!(t.take(4), vec![1.0, 2.0, 0.0, 0.0]);
    }

    #[test]
    fn late_samples_are_dropped() {
        let mut t = one();
        t.put(0, 0, &ramp(1, 4));
        t.take(4);
        t.put(0, 2, &[9.0; 4]); // frames 2,3 already taken; 4,5 still fine
        assert_eq!(t.take(6), vec![9.0, 9.0]);
    }

    #[test]
    fn overlap_keeps_the_first_write() {
        let mut t = one();
        t.put(0, 0, &[1.0, 1.0, 1.0, 1.0]);
        t.put(0, 2, &[2.0, 2.0, 2.0, 2.0]);
        assert_eq!(t.take(6), vec![1.0, 1.0, 1.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn channels_are_interleaved_and_independent() {
        let mut t = Timeline::new(RATE, 2);
        t.put(0, 0, &[1.0, 2.0, 3.0]);
        t.put(1, 1, &[7.0, 8.0]);
        assert_eq!(t.take(3), vec![1.0, 0.0, 2.0, 7.0, 3.0, 8.0]);
        assert_eq!(t.channels(), 2);
    }

    #[test]
    fn a_bad_channel_is_ignored() {
        let mut t = one();
        t.put(3, 0, &[1.0]);
        assert_eq!(t.take(1), vec![0.0]);
    }

    #[test]
    fn pause_removes_the_span() {
        let mut t = one();
        t.put(0, 0, &ramp(1, 4)); // frames 0..4
        t.pause_at(4);
        t.put(0, 4, &[9.0; 4]); // stamped inside the pause: dropped
        t.resume_at(8);
        t.put(0, 8, &ramp(101, 4)); // raw 8..12 -> timeline 4..8
        assert_eq!(t.take(12), vec![1.0, 2.0, 3.0, 4.0, 101.0, 102.0, 103.0, 104.0]);
    }

    #[test]
    fn a_packet_straddling_a_pause_edge_is_cut() {
        let mut t = one();
        t.pause_at(3);
        t.resume_at(5);
        t.put(0, 0, &ramp(1, 8)); // raw 0..8: keeps raw 0,1,2 and 5,6,7
        assert_eq!(t.take(8), vec![1.0, 2.0, 3.0, 6.0, 7.0, 8.0]);
    }

    #[test]
    fn take_while_paused_stops_at_the_pause() {
        let mut t = one();
        t.put(0, 0, &ramp(1, 4));
        t.pause_at(4);
        assert_eq!(t.take(50), ramp(1, 4));
        t.resume_at(50);
        t.put(0, 50, &[5.0, 5.0]);
        assert_eq!(t.take(52), vec![5.0, 5.0]);
    }

    #[test]
    fn two_pauses_shift_by_both() {
        let mut t = one();
        t.pause_at(2);
        t.resume_at(4);
        t.pause_at(6);
        t.resume_at(10);
        t.put(0, 10, &[7.0, 7.0]); // timeline: 10 - 2 - 4 = 4
        t.put(0, 4, &[3.0, 3.0]); // raw 4,5 -> timeline 2,3
        t.put(0, 0, &[1.0, 1.0]); // raw 0,1 -> timeline 0,1
        assert_eq!(t.take(12), vec![1.0, 1.0, 3.0, 3.0, 7.0, 7.0]);
    }

    #[test]
    fn pause_and_resume_edge_cases() {
        let mut t = one();
        t.resume_at(5); // not paused: ignored
        t.pause_at(3);
        t.pause_at(4); // already paused: ignored
        t.resume_at(3); // empty span: removed
        t.put(0, 0, &ramp(1, 6));
        assert_eq!(t.take(6), ramp(1, 6));
    }

    #[test]
    fn memory_is_freed_by_take() {
        let mut t = one();
        for k in 0..100 {
            t.put(0, k * 100, &[0.5; 100]);
            t.take((k + 1) * 100);
            assert!(t.buffered_frames() <= 100);
        }
        assert_eq!(t.buffered_frames(), 0);
    }

    #[test]
    fn a_write_far_ahead_is_dropped() {
        let mut t = one(); // limit: 10 s * 1000 = 10_000 frames past the read point
        t.put(0, 20_000, &[1.0; 10]);
        assert_eq!(t.buffered_frames(), 0);
        t.put(0, 9_990, &[1.0; 100]); // only the first 10 frames fit
        assert_eq!(t.buffered_frames(), 10_000);
        let out = t.take(10_001);
        assert_eq!(out.len(), 10_001);
        assert_eq!(out[9_999], 1.0);
        assert_eq!(out[10_000], 0.0);
        assert_eq!(t.buffered_frames(), 0);
    }

    #[test]
    fn qpc_conversion_rounds_to_the_nearest_frame() {
        // 10 MHz QPC, 48 kHz: 1 frame = 208.33.. ticks
        assert_eq!(qpc_to_frame(10_000_000, 10_000_000, 0, 48_000), 48_000);
        assert_eq!(qpc_to_frame(1_000_100, 10_000_000, 1_000_000, 48_000), 0); // 0.48 frame
        assert_eq!(qpc_to_frame(1_000_104, 10_000_000, 1_000_000, 48_000), 0); // 0.499
        assert_eq!(qpc_to_frame(1_000_105, 10_000_000, 1_000_000, 48_000), 1); // 0.504
        // exact half rounds up: 1 tick at freq 2, rate 1 = 0.5 frame
        assert_eq!(qpc_to_frame(1, 2, 0, 1), 1);
        // before the origin: negative, rounded to nearest
        assert_eq!(qpc_to_frame(999_895, 10_000_000, 1_000_000, 48_000), -1); // -0.504
        assert_eq!(qpc_to_frame(999_900, 10_000_000, 1_000_000, 48_000), 0); // -0.48
    }

    #[test]
    fn qpc_conversion_does_not_overflow() {
        // ~100 days of 10 MHz ticks times 48_000 exceeds u64 but not i128.
        let qpc = 10_000_000u64 * 86_400 * 100;
        let f = qpc_to_frame(qpc, 10_000_000, 0, 48_000);
        assert_eq!(f, 86_400 * 100 * 48_000);
        assert_eq!(Timeline::new(48_000, 1).qpc_to_frame(qpc, 10_000_000, 0), f);
    }
}
