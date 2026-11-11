//! The pure numbers of a screen recording: the video size, the bitrate, the
//! clock with pauses, and where the zoom view lands in the frame.

use super::zoom::RectF;

/// Hardware H.264 encoders stop at level 5.1/5.2: 4096 x 2304.
pub const MAX_W: u32 = 4096;
pub const MAX_H: u32 = 2304;

/// Encoders refuse smaller frames (the Windows one wants about 64 px).
pub const MIN_SIDE: u32 = 64;

/// The encoded size for a source of `w` x `h`: the source's own size, scaled
/// down (aspect kept) to fit the encoder and `max_h` when set, scaled up
/// when a side is under [`MIN_SIDE`], both sides even (NV12 and H.264 need
/// it) and at least [`MIN_SIDE`] (a very thin source gets bars).
pub fn output_size(w: u32, h: u32, max_h: Option<u32>) -> (u32, u32) {
    let (w, h) = (w.max(2) as f64, h.max(2) as f64);
    let cap_h = max_h.map_or(MAX_H, |m| m.clamp(MIN_SIDE, MAX_H)) as f64;
    let mut scale = (MAX_W as f64 / w).min(cap_h / h).min(1.0);
    let min = MIN_SIDE as f64;
    if w * scale < min || h * scale < min {
        scale = (min / w)
            .max(min / h)
            .min(MAX_W as f64 / w)
            .min(MAX_H as f64 / h);
    }
    let even = |v: f64| ((v.floor() as u32) & !1).max(MIN_SIDE);
    (even(w * scale), even(h * scale))
}

/// The H.264 bitrate in bits per second: 0.08 bit per pixel per frame
/// (about 5 Mbit/s for 1080p30, 2.2 GB an hour), between 1 and 50 Mbit/s.
pub fn bitrate(w: u32, h: u32, fps: u32) -> u32 {
    let bits = w as f64 * h as f64 * fps as f64 * 0.08;
    bits.clamp(1_000_000.0, 50_000_000.0) as u32
}

/// An integer rect: left, top, right, bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl IRect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

/// Where the zoom `view` lands: the part of it inside `bounds` (the source
/// texture's captured area) and where that part goes in an `out_w` x `out_h`
/// frame. The rest of the frame is bars. `None` when nothing of the view is
/// inside the source.
pub fn map_view(view: RectF, bounds: RectF, out_w: u32, out_h: u32) -> Option<(IRect, IRect)> {
    if view.w <= 0.0 || view.h <= 0.0 {
        return None;
    }
    let left = view.x.max(bounds.x);
    let top = view.y.max(bounds.y);
    let right = view.right().min(bounds.right());
    let bottom = view.bottom().min(bounds.bottom());
    if right - left < 1.0 || bottom - top < 1.0 {
        return None;
    }
    let sx = out_w as f64 / view.w;
    let sy = out_h as f64 / view.h;
    let src = IRect {
        left: left.round() as i32,
        top: top.round() as i32,
        right: right.round() as i32,
        bottom: bottom.round() as i32,
    };
    let dst = IRect {
        left: ((left - view.x) * sx).round() as i32,
        top: ((top - view.y) * sy).round() as i32,
        right: ((right - view.x) * sx).round().min(out_w as f64) as i32,
        bottom: ((bottom - view.y) * sy).round().min(out_h as f64) as i32,
    };
    (src.width() > 0 && src.height() > 0 && dst.width() > 0 && dst.height() > 0)
        .then_some((src, dst))
}

/// QPC ticks to 100 ns units (Media Foundation's time).
pub fn ticks_to_hns(ticks: i64, freq: i64) -> i64 {
    (ticks as i128 * 10_000_000 / freq.max(1) as i128) as i64
}

/// The recording's clock: QPC readings to video time, with every paused
/// spell cut out (a pause leaves no gap in the video). The spans are kept,
/// so a reading from before a pause (audio arrives late) still maps.
#[derive(Clone, Debug)]
pub struct Timeline {
    start: i64,
    freq: i64,
    /// Ended pauses, oldest first: `[from, to)`.
    spans: Vec<(i64, i64)>,
    paused_since: Option<i64>,
}

impl Timeline {
    pub fn new(start_qpc: i64, freq: i64) -> Timeline {
        Timeline {
            start: start_qpc,
            freq,
            spans: Vec::new(),
            paused_since: None,
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_since.is_some()
    }

    pub fn pause(&mut self, qpc: i64) {
        if self.paused_since.is_none() {
            self.paused_since = Some(qpc);
        }
    }

    pub fn resume(&mut self, qpc: i64) {
        if let Some(since) = self.paused_since.take() {
            self.spans.push((since, qpc.max(since)));
        }
    }

    /// The video time of `qpc` in 100 ns units; `None` before the start, at
    /// or after the start of an open pause and inside an ended one.
    pub fn media_time(&self, qpc: i64) -> Option<i64> {
        if qpc < self.start || self.paused_since.is_some_and(|since| qpc >= since) {
            return None;
        }
        let mut cut = 0;
        for &(from, to) in &self.spans {
            if qpc < from {
                break;
            }
            if qpc < to {
                return None;
            }
            cut += to - from;
        }
        Some(ticks_to_hns(qpc - self.start - cut, self.freq))
    }

    /// How many of `frames` frames (at `rate` Hz) of a chunk starting at
    /// `qpc` lie before the next pause starts; all of them when no pause
    /// follows. A chunk that straddles a pause is kept only up to it, so
    /// what comes after the resume never starts before it ends.
    pub fn frames_before_pause(&self, qpc: i64, frames: u64, rate: u32) -> u64 {
        let next = self
            .spans
            .iter()
            .map(|&(from, _)| from)
            .chain(self.paused_since)
            .find(|&from| from > qpc);
        match next {
            Some(from) => {
                let fit = ((from - qpc) as i128 * rate as i128 / self.freq.max(1) as i128) as u64;
                fit.min(frames)
            }
            None => frames,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_keeps_the_source_size_when_it_fits() {
        assert_eq!(output_size(1920, 1080, None), (1920, 1080));
        assert_eq!(output_size(2560, 1440, None), (2560, 1440));
    }

    #[test]
    fn odd_sizes_become_even() {
        assert_eq!(output_size(1281, 721, None), (1280, 720));
        assert_eq!(output_size(1001, 601, None), (1000, 600));
    }

    #[test]
    fn tiny_sources_are_scaled_up_to_what_encoders_take() {
        assert_eq!(output_size(1, 1, None), (64, 64));
        assert_eq!(output_size(0, 0, None), (64, 64));
        assert_eq!(output_size(32, 32, None), (64, 64));
        assert_eq!(output_size(200, 40, None), (320, 64));
        // A sliver: the long side hits the limit, the short one gets bars.
        let (w, h) = output_size(8000, 10, None);
        assert_eq!(w, 4096);
        assert_eq!(h, 64);
    }

    #[test]
    fn big_sources_are_scaled_to_the_encoder_limit() {
        let (w, h) = output_size(5120, 1440, None);
        assert_eq!(w, 4096);
        assert!(h <= MAX_H && h % 2 == 0);
        assert_eq!(h, 1152);
        let (w, h) = output_size(3840, 2400, None);
        assert!(w <= MAX_W && h <= MAX_H);
    }

    #[test]
    fn a_1080p_cap_scales_a_4k_source() {
        assert_eq!(output_size(3840, 2160, Some(1080)), (1920, 1080));
        // A cap above the source does not scale up.
        assert_eq!(output_size(1280, 720, Some(1080)), (1280, 720));
    }

    #[test]
    fn bitrate_grows_with_size_and_rate() {
        assert_eq!(bitrate(1920, 1080, 30), 4_976_640);
        assert!(bitrate(1920, 1080, 60) > bitrate(1920, 1080, 30));
        assert_eq!(bitrate(100, 100, 30), 1_000_000);
        assert_eq!(bitrate(8000, 8000, 60), 50_000_000);
    }

    #[test]
    fn the_whole_view_fills_the_frame() {
        let b = RectF::new(0.0, 0.0, 1920.0, 1080.0);
        let (src, dst) = map_view(b, b, 1280, 720).unwrap();
        assert_eq!(
            src,
            IRect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080
            }
        );
        assert_eq!(
            dst,
            IRect {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720
            }
        );
    }

    #[test]
    fn a_zoomed_view_maps_its_part_to_the_whole_frame() {
        let b = RectF::new(0.0, 0.0, 1920.0, 1080.0);
        let v = RectF::new(480.0, 270.0, 960.0, 540.0);
        let (src, dst) = map_view(v, b, 1920, 1080).unwrap();
        assert_eq!(
            (src.left, src.top, src.right, src.bottom),
            (480, 270, 1440, 810)
        );
        assert_eq!(
            (dst.left, dst.top, dst.right, dst.bottom),
            (0, 0, 1920, 1080)
        );
    }

    #[test]
    fn a_view_wider_than_the_source_leaves_bars() {
        // A window 800 x 900 in a 16:9 video: bars left and right.
        let b = RectF::new(0.0, 0.0, 800.0, 900.0);
        let v = RectF::new(-400.0, 0.0, 1600.0, 900.0);
        let (src, dst) = map_view(v, b, 1600, 900).unwrap();
        assert_eq!((src.left, src.right), (0, 800));
        assert_eq!(
            (dst.left, dst.right, dst.top, dst.bottom),
            (400, 1200, 0, 900)
        );
    }

    #[test]
    fn a_region_maps_from_its_offset() {
        let b = RectF::new(100.0, 50.0, 640.0, 360.0);
        let (src, dst) = map_view(b, b, 640, 360).unwrap();
        assert_eq!(
            (src.left, src.top, src.right, src.bottom),
            (100, 50, 740, 410)
        );
        assert_eq!((dst.left, dst.top, dst.right, dst.bottom), (0, 0, 640, 360));
    }

    #[test]
    fn a_view_outside_the_source_maps_to_nothing() {
        let b = RectF::new(0.0, 0.0, 100.0, 100.0);
        assert!(map_view(RectF::new(200.0, 0.0, 50.0, 50.0), b, 100, 100).is_none());
        assert!(map_view(RectF::new(0.0, 0.0, 0.0, 50.0), b, 100, 100).is_none());
    }

    #[test]
    fn ticks_convert_to_media_time() {
        assert_eq!(ticks_to_hns(10_000_000, 10_000_000), 10_000_000);
        assert_eq!(ticks_to_hns(3_000_000, 3_000_000), 10_000_000);
        // No overflow on a long uptime.
        assert_eq!(ticks_to_hns(i64::MAX / 2, 10_000_000), i64::MAX / 2);
    }

    #[test]
    fn a_pause_is_cut_out_of_the_video_time() {
        let f = 1000;
        let mut t = Timeline::new(5000, f);
        assert_eq!(t.media_time(4000), None);
        assert_eq!(t.media_time(6000), Some(10_000_000));
        t.pause(7000);
        assert!(t.is_paused());
        assert_eq!(t.media_time(8000), None);
        t.resume(10_000);
        // 3 s paused: 11 s on the clock is 3 s of video.
        assert_eq!(t.media_time(11_000), Some(30_000_000));
        // A second pause call keeps the first start; resume twice is harmless.
        t.pause(12_000);
        t.pause(13_000);
        t.resume(14_000);
        t.resume(15_000);
        assert_eq!(t.media_time(15_000), Some(50_000_000));
    }

    const F: i64 = 10_000_000;
    const S: i64 = 10_000_000;

    #[test]
    fn the_timeline_cuts_each_pause_once() {
        let mut t = Timeline::new(0, F);
        t.pause(10 * S);
        t.resume(15 * S);
        assert_eq!(t.media_time(12 * S), None);
        assert_eq!(t.media_time(20 * S), Some(150_000_000));
        assert_eq!(t.media_time(8 * S), Some(80_000_000));
        // The resume instant itself is video time again.
        assert_eq!(t.media_time(15 * S), Some(100_000_000));
        // Before the start there is no time.
        assert_eq!(Timeline::new(5 * S, F).media_time(4 * S), None);
    }

    #[test]
    fn a_chunk_over_a_pause_keeps_only_its_head() {
        // 48 kHz at 10 MHz ticks: 480 frames are 100_000 ticks (10 ms).
        let mut t = Timeline::new(0, F);
        // Wholly before any pause: untouched.
        assert_eq!(t.frames_before_pause(0, 480, 48_000), 480);
        t.pause(10 * S);
        assert_eq!(t.frames_before_pause(10 * S - 100_000, 480, 48_000), 480);
        // Straddles the open pause: 5 ms of 10 ms stays.
        assert_eq!(t.frames_before_pause(10 * S - 50_000, 480, 48_000), 240);
        t.resume(15 * S);
        // Straddles the ended span: the same.
        assert_eq!(t.frames_before_pause(10 * S - 50_000, 480, 48_000), 240);
        // After the resume nothing cuts it, even with a later pause far off.
        t.pause(20 * S);
        assert_eq!(t.frames_before_pause(15 * S, 480, 48_000), 480);
        assert_eq!(t.frames_before_pause(20 * S - 25_000, 480, 48_000), 120);
    }

    #[test]
    fn a_qpc_from_before_an_open_pause_still_maps() {
        let mut t = Timeline::new(0, F);
        t.pause(10 * S);
        assert_eq!(t.media_time(99 * S / 10), Some(99_000_000));
        assert_eq!(t.media_time(10 * S), None);
    }

    #[test]
    fn two_pauses_add_up() {
        let mut t = Timeline::new(0, F);
        t.pause(10 * S);
        t.resume(15 * S);
        t.pause(20 * S);
        t.resume(22 * S);
        assert_eq!(t.media_time(30 * S), Some(230_000_000));
        assert_eq!(t.media_time(18 * S), Some(130_000_000));
    }
}
