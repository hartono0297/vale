//! The lyrics card's pure rules: where things sit, how the lines stack and
//! scroll, which line a point is over, what a click on a line seeks to, how a
//! manual scroll holds and returns, where the window goes, and when the card
//! opens or closes. No Win32 and no drawing, so all of it is tested.
//!
//! Units are DIPs unless a name says `px`. The window draws at `dpi / 96`.

use crate::config::CardPlace;
use crate::lyrics::{LrcLine, OUTPUT_LAG_MS};
use crate::ui::settings_window::Rc;

// ------------------------------------------------------------------ sizes

pub const DEFAULT_W: f32 = 300.0;
pub const DEFAULT_H: f32 = 420.0;
pub const MIN_W: f32 = 240.0;
pub const MIN_H: f32 = 300.0;
pub const MAX_W: f32 = 520.0;
pub const MAX_H: f32 = 800.0;
pub const RADIUS: f32 = 16.0;
/// The header's padding, and the body's side padding.
pub const PAD: f32 = 14.0;
pub const THUMB: f32 = 30.0;
pub const THUMB_RADIUS: f32 = 6.0;
/// The close button's hit square.
pub const CLOSE: f32 = 24.0;
pub const BAR_H: f32 = 3.0;
pub const GRIP: f32 = 16.0;
/// The body fades out this far in at its top and bottom edge.
pub const EDGE_FADE: f32 = 28.0;
/// The current line's centre sits at this fraction of the body's height.
pub const CURRENT_AT: f32 = 0.40;
/// One wheel notch scrolls this far.
pub const WHEEL_STEP: f32 = 48.0;
/// After this long without wheel input the list goes back to the song.
pub const MANUAL_HOLD_MS: u32 = 3000;
/// The progress bar and the next-line timer tick at least this often.
pub const PROGRESS_TICK_MS: u32 = 1000;
/// Added to a line's time for a click on it (after the output lag).
pub const SEEK_MARGIN_MS: u64 = 30;
/// Space between two lines.
pub const LINE_GAP: f32 = 14.0;
/// Type sizes: the current line, the others (px at 96 dpi).
pub const SIZE_CURRENT: f32 = 20.0;
pub const SIZE_OTHER: f32 = 17.5;
/// The current line is drawn this much larger than the others.
pub const CURRENT_SCALE: f32 = SIZE_CURRENT / SIZE_OTHER;
/// The scroll ease and the colour crossfade, in milliseconds.
pub const SCROLL_MS: u32 = 350;
pub const FADE_MS: u32 = 300;
pub const HOVER_MS: u32 = 150;
/// The hover controls (prev, play/pause, next): a square each.
pub const CONTROL: f32 = 30.0;
pub const CONTROL_GAP: f32 = 14.0;

/// A rectangle in DIPs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// Where each part of a card `w` x `h` sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rects {
    pub header: RectF,
    pub thumb: RectF,
    pub title: RectF,
    pub artist: RectF,
    /// The close button's 24 dip hit square.
    pub close: RectF,
    /// The lyric lines scroll in here (under the header, over the footer).
    pub body: RectF,
    /// The 3 dip progress bar.
    pub bar: RectF,
    /// Prev, play/pause, next: above the bar, over the body's bottom fade.
    pub controls: [RectF; 3],
    pub grip: RectF,
}

pub fn card_rects(w: f32, h: f32) -> Rects {
    let header_h = PAD + THUMB + 10.0;
    let thumb = RectF::new(PAD, PAD, THUMB, THUMB);
    let close = RectF::new(w - PAD - CLOSE, PAD + (THUMB - CLOSE) / 2.0, CLOSE, CLOSE);
    let text_x = thumb.right() + 10.0;
    let text_w = (close.x - 6.0 - text_x).max(0.0);
    let title = RectF::new(text_x, PAD, text_w, 15.0);
    let artist = RectF::new(text_x, PAD + 16.0, text_w, 14.0);
    let bar = RectF::new(PAD, h - PAD - BAR_H, (w - 2.0 * PAD).max(0.0), BAR_H);
    let row_y = bar.y - 10.0 - CONTROL;
    let row_w = 3.0 * CONTROL + 2.0 * CONTROL_GAP;
    let x0 = (w - row_w) / 2.0;
    let controls = std::array::from_fn(|i| {
        RectF::new(
            x0 + i as f32 * (CONTROL + CONTROL_GAP),
            row_y,
            CONTROL,
            CONTROL,
        )
    });
    let body_top = header_h;
    let body_bottom = bar.y - 10.0;
    Rects {
        header: RectF::new(0.0, 0.0, w, header_h),
        thumb,
        title,
        artist,
        close,
        body: RectF::new(0.0, body_top, w, (body_bottom - body_top).max(0.0)),
        bar,
        controls,
        grip: RectF::new(w - GRIP, h - GRIP, GRIP, GRIP),
    }
}

/// The width the lines wrap to.
pub fn line_width(w: f32) -> f32 {
    (w - 2.0 * PAD - 6.0).max(40.0)
}

/// `size` kept in the card's limits.
pub fn clamp_size(w: f32, h: f32) -> (f32, f32) {
    (w.clamp(MIN_W, MAX_W), h.clamp(MIN_H, MAX_H))
}

// ---------------------------------------------------------------- stacking

/// The top of each line when `heights` stack with `gap` between them.
pub fn stack(heights: &[f32], gap: f32) -> Vec<f32> {
    let mut y = 0.0;
    heights
        .iter()
        .map(|h| {
            let top = y;
            y += h + gap;
            top
        })
        .collect()
}

fn centre(tops: &[f32], heights: &[f32], i: usize) -> f32 {
    tops[i] + heights[i] / 2.0
}

/// The scroll offset (how far the list is moved up) that puts the current
/// line's centre at `CURRENT_AT` of the body's height. Before the first line
/// (`None`) the first line sits there, dim. No lines: 0.
pub fn scroll_target(tops: &[f32], heights: &[f32], current: Option<usize>, body_h: f32) -> f32 {
    if tops.is_empty() || heights.len() != tops.len() {
        return 0.0;
    }
    let i = current.unwrap_or(0).min(tops.len() - 1);
    centre(tops, heights, i) - CURRENT_AT * body_h
}

/// The lowest and highest offset a manual scroll may reach: the first and the
/// last line can each get to the 40 % line.
pub fn scroll_bounds(tops: &[f32], heights: &[f32], body_h: f32) -> (f32, f32) {
    if tops.is_empty() || heights.len() != tops.len() {
        return (0.0, 0.0);
    }
    let lo = centre(tops, heights, 0) - CURRENT_AT * body_h;
    let hi = centre(tops, heights, tops.len() - 1) - CURRENT_AT * body_h;
    (lo.min(hi), hi.max(lo))
}

/// Whether a screen point (px) is inside a window at `origin` of `size` (px):
/// the right and bottom edges are outside.
pub fn point_in(pt: (i32, i32), origin: (i32, i32), size: (i32, i32)) -> bool {
    pt.0 >= origin.0 && pt.0 < origin.0 + size.0 && pt.1 >= origin.1 && pt.1 < origin.1 + size.1
}

/// The line under `y` (DIPs from the card's top), for a list scrolled by
/// `scroll` in a body at `body_top` that is `body_h` tall. A point in the gap
/// between two lines belongs to the nearer one; outside the body, none.
pub fn hit_line(
    tops: &[f32],
    heights: &[f32],
    scroll: f32,
    body_top: f32,
    body_h: f32,
    y: f32,
) -> Option<usize> {
    if tops.is_empty() || heights.len() != tops.len() || y < body_top || y >= body_top + body_h {
        return None;
    }
    let content_y = y - body_top + scroll;
    let half = LINE_GAP / 2.0;
    (0..tops.len()).find(|&i| content_y >= tops[i] - half && content_y < tops[i] + heights[i] + half)
}

// ------------------------------------------------------------ manual scroll

/// A wheel scroll that holds for `MANUAL_HOLD_MS` after the last notch.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ManualScroll {
    /// The held offset, `None` while the list follows the song.
    offset: Option<f32>,
    last_input_ms: u64,
}

impl ManualScroll {
    /// `base` is the offset showing now (a hold starts from it); `notches`
    /// positive scrolls up (towards the earlier lines). Returns the new offset
    /// kept inside `bounds`.
    pub fn wheel(&mut self, base: f32, notches: f32, bounds: (f32, f32), now_ms: u64) -> f32 {
        let from = self.offset.unwrap_or(base);
        let next = (from - notches * WHEEL_STEP).clamp(bounds.0, bounds.1.max(bounds.0));
        self.offset = Some(next);
        self.last_input_ms = now_ms;
        next
    }

    /// The held offset, if the list is held.
    pub fn offset(&self) -> Option<f32> {
        self.offset
    }

    /// Held now: the song's line changes do not move the list.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn held(&self) -> bool {
        self.offset.is_some()
    }

    /// The hold is over at `now_ms` (3 s without input).
    pub fn due(&self, now_ms: u64) -> bool {
        self.offset.is_some()
            && now_ms.saturating_sub(self.last_input_ms) >= u64::from(MANUAL_HOLD_MS)
    }

    /// Milliseconds until the hold ends, for the one-shot timer.
    pub fn remaining_ms(&self, now_ms: u64) -> Option<u32> {
        self.offset?;
        let left = u64::from(MANUAL_HOLD_MS).saturating_sub(now_ms.saturating_sub(self.last_input_ms));
        Some(left.max(1) as u32)
    }

    /// Back to following the song.
    pub fn release(&mut self) {
        self.offset = None;
    }

    /// The bounds moved (the lines or the size changed): keep the offset in them.
    pub fn clamp(&mut self, bounds: (f32, f32)) {
        if let Some(o) = &mut self.offset {
            *o = o.clamp(bounds.0, bounds.1.max(bounds.0));
        }
    }
}

// ------------------------------------------------------------------- seeking

/// Where a click on line `i` seeks to: the line's time, plus the output lag
/// the notch applies (`OUTPUT_LAG_MS`) and a small margin, so the line chosen
/// at that position is `i`; capped to stay below line `i + 1`'s due time. With
/// distinct line times `current_line(lines, lyric_position(seek_ms(..))) == i`
/// (two lines sharing one time cannot be told apart).
pub fn seek_ms(lines: &[LrcLine], i: usize) -> Option<u64> {
    let at = lines.get(i)?.at_ms;
    let lag = OUTPUT_LAG_MS as u64;
    let mut seek = at.saturating_add(lag).saturating_add(SEEK_MARGIN_MS);
    if let Some(next) = lines.get(i + 1) {
        let next_due = next.at_ms.saturating_add(lag);
        if next_due > at.saturating_add(lag) {
            seek = seek.min(next_due - 1);
        }
    }
    Some(seek)
}

/// The progress bar's fill (0..=1) at `position_ms` of a track `end_ms` long.
pub fn progress(position_ms: u64, end_ms: u64) -> f32 {
    if end_ms == 0 {
        return 0.0;
    }
    (position_ms as f64 / end_ms as f64).clamp(0.0, 1.0) as f32
}

/// The next-line timer's wait: the line's own wait, but at most
/// `PROGRESS_TICK_MS` so the bar moves; never 0.
pub fn timer_ms(next_line_wait: Option<u64>) -> u32 {
    let wait = next_line_wait.unwrap_or(u64::from(PROGRESS_TICK_MS));
    wait.min(u64::from(PROGRESS_TICK_MS)).max(1) as u32
}

/// Whether a press that moved `dx`, `dy` physical px is a drag (more than the
/// system's drag width or height), not a click.
pub fn is_drag(dx: i32, dy: i32, thr_x: i32, thr_y: i32) -> bool {
    dx.abs() > thr_x.max(1) || dy.abs() > thr_y.max(1)
}

/// A bottom-right resize: the size after the pointer moved `dx`, `dy` DIPs
/// from where the grip was pressed, kept within the limits.
pub fn resized(start: (f32, f32), dx: f32, dy: f32) -> (f32, f32) {
    clamp_size(start.0 + dx, start.1 + dy)
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

// ---------------------------------------------------------- what is under it

/// What a point in the card is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Close,
    Prev,
    PlayPause,
    Next,
    Grip,
    /// The lines (a click may seek; the highlight goes here).
    Body,
    /// The header outside the close button.
    Header,
    Outside,
}

/// `hovering`: the pointer is in the card, so the grip is shown (and so can
/// be hit), and the transport controls too when `has_media` (a session to
/// control; the Music panel drops them the same way).
pub fn zone_at(
    r: &Rects,
    w: f32,
    h: f32,
    x: f32,
    y: f32,
    hovering: bool,
    has_media: bool,
) -> Zone {
    if x < 0.0 || y < 0.0 || x >= w || y >= h {
        return Zone::Outside;
    }
    if r.close.contains(x, y) {
        return Zone::Close;
    }
    if hovering {
        if r.grip.contains(x, y) {
            return Zone::Grip;
        }
        for (i, c) in r.controls.iter().enumerate() {
            if has_media && c.contains(x, y) {
                return [Zone::Prev, Zone::PlayPause, Zone::Next][i];
            }
        }
    }
    if r.header.contains(x, y) {
        Zone::Header
    } else {
        Zone::Body
    }
}

// ----------------------------------------------------------------- toggling

/// What asked the card to open or close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Req {
    /// The hotkey or the panel button.
    Toggle,
    /// The lyrics setting went off (the Music page, Settings, a config read).
    LyricsOff,
    /// The X, Esc, or the app ending.
    Close,
}

/// Whether the card is open after `req`. A toggle with lyrics off does
/// nothing and keeps it closed; lyrics off and close always close it.
pub fn card_next(open: bool, lyrics_on: bool, req: Req) -> bool {
    match req {
        Req::Toggle => lyrics_on && !open,
        Req::LyricsOff | Req::Close => false,
    }
}

// ---------------------------------------------------------------- placement

/// A monitor as placement sees it (physical px).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mon {
    pub rect: Rc,
    pub work: Rc,
    pub dpi: u32,
}

fn px(dip: f32, dpi: u32) -> i32 {
    (dip * dpi.max(96) as f32 / 96.0).round() as i32
}

fn overlap(a: Rc, b: Rc) -> i64 {
    let w = (a.right.min(b.right) - a.left.max(b.left)).max(0);
    let h = (a.bottom.min(b.bottom) - a.top.max(b.top)).max(0);
    i64::from(w) * i64::from(h)
}

/// Where the card goes. The saved rect is used when at least half of it lies
/// on one monitor (then kept inside the size limits at that monitor's DPI,
/// its top left staying); otherwise (first open, or the monitor is gone) the
/// default on `notch`: the right edge 24 dip in from the work area's right,
/// the top 72 dip below its top, the default size. `None` for no monitors.
pub fn place(saved: Option<CardPlace>, monitors: &[Mon], notch: Option<Mon>) -> Option<Rc> {
    if let Some(p) = saved {
        let r = Rc {
            left: p.left,
            top: p.top,
            right: p.right,
            bottom: p.bottom,
        };
        let area = i64::from(r.width().max(0)) * i64::from(r.height().max(0));
        let best = monitors
            .iter()
            .max_by_key(|m| overlap(r, m.rect))
            .filter(|m| area > 0 && overlap(r, m.rect) * 2 >= area);
        if let Some(m) = best {
            let k = m.dpi.max(96) as f32 / 96.0;
            let (w, h) = clamp_size(r.width() as f32 / k, r.height() as f32 / k);
            return Some(Rc {
                left: r.left,
                top: r.top,
                right: r.left + px(w, m.dpi),
                bottom: r.top + px(h, m.dpi),
            });
        }
    }
    let m = notch.or_else(|| monitors.first().copied())?;
    let (mut w, mut h) = (px(DEFAULT_W, m.dpi), px(DEFAULT_H, m.dpi));
    // A small screen: never taller or wider than its work area.
    w = w.min(m.work.width().max(1));
    h = h.min(m.work.height().max(1));
    let right = m.work.right - px(24.0, m.dpi);
    let top = m.work.top + px(72.0, m.dpi);
    let left = (right - w).max(m.work.left);
    let top = top.min((m.work.bottom - h).max(m.work.top));
    Some(Rc {
        left,
        top,
        right: left + w,
        bottom: top + h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::{current_line, lyric_position};

    #[test]
    fn a_point_is_in_a_window_until_its_right_and_bottom_edge() {
        let (origin, size) = ((100, 200), (300, 420));
        assert!(point_in((100, 200), origin, size), "the top left corner is in");
        assert!(point_in((250, 300), origin, size));
        assert!(point_in((399, 619), origin, size), "the last pixel is in");
        assert!(!point_in((400, 300), origin, size), "the right edge is out");
        assert!(!point_in((250, 620), origin, size), "the bottom edge is out");
        assert!(!point_in((99, 300), origin, size));
        assert!(!point_in((250, 199), origin, size));
        assert!(!point_in((-5000, -5000), origin, size));
        // Monitors left of or above the primary have negative origins.
        assert!(point_in((-250, -100), (-300, -200), size));
    }

    fn lines(times: &[u64]) -> Vec<LrcLine> {
        times
            .iter()
            .enumerate()
            .map(|(i, &at_ms)| LrcLine {
                at_ms,
                text: format!("line {i}"),
            })
            .collect()
    }

    fn rc(left: i32, top: i32, right: i32, bottom: i32) -> Rc {
        Rc {
            left,
            top,
            right,
            bottom,
        }
    }

    fn mon(rect: Rc, dpi: u32) -> Mon {
        Mon {
            rect,
            work: Rc {
                bottom: rect.bottom - 40 * dpi.max(96) as i32 / 96,
                ..rect
            },
            dpi,
        }
    }

    // ---- stacking and the scroll target

    #[test]
    fn lines_stack_top_down_with_the_gap() {
        let tops = stack(&[20.0, 40.0, 20.0], 14.0);
        assert_eq!(tops, vec![0.0, 34.0, 88.0]);
        assert!(stack(&[], 14.0).is_empty());
    }

    #[test]
    fn the_current_lines_centre_goes_to_forty_percent_of_the_body() {
        let heights = [24.0, 48.0, 24.0, 24.0];
        let tops = stack(&heights, LINE_GAP);
        let body_h = 300.0;
        for i in 0..4 {
            let s = scroll_target(&tops, &heights, Some(i), body_h);
            // Drawn at tops[i] - s: the centre lands at 0.4 of the body.
            let centre = tops[i] - s + heights[i] / 2.0;
            assert!((centre - 0.4 * body_h).abs() < 1e-3, "line {i}");
        }
        // A wrapped (taller) line is centred by its own height.
        let s1 = scroll_target(&tops, &heights, Some(1), body_h);
        assert!((tops[1] - s1 - (0.4 * body_h - 24.0)).abs() < 1e-3);
    }

    #[test]
    fn before_the_first_line_the_first_line_waits_at_the_mark() {
        let heights = [24.0, 24.0];
        let tops = stack(&heights, LINE_GAP);
        let s = scroll_target(&tops, &heights, None, 300.0);
        assert_eq!(s, scroll_target(&tops, &heights, Some(0), 300.0));
        // And the first line is below the top of the body.
        assert!(tops[0] - s > 0.0);
    }

    #[test]
    fn the_last_line_can_reach_the_mark_and_an_empty_list_rests() {
        let heights = [30.0; 5];
        let tops = stack(&heights, LINE_GAP);
        let (lo, hi) = scroll_bounds(&tops, &heights, 280.0);
        assert_eq!(lo, scroll_target(&tops, &heights, Some(0), 280.0));
        assert_eq!(hi, scroll_target(&tops, &heights, Some(4), 280.0));
        assert!(lo < hi);
        assert_eq!(scroll_bounds(&[], &[], 280.0), (0.0, 0.0));
        assert_eq!(scroll_target(&[], &[], None, 280.0), 0.0);
        // One line: the bounds meet.
        let t = stack(&[30.0], LINE_GAP);
        let (a, b) = scroll_bounds(&t, &[30.0], 280.0);
        assert_eq!(a, b);
        // A current index past the end is the last line.
        assert_eq!(
            scroll_target(&tops, &heights, Some(99), 280.0),
            scroll_target(&tops, &heights, Some(4), 280.0)
        );
    }

    // ---- hit testing

    #[test]
    fn a_y_maps_to_the_line_under_it_with_the_scroll_offset() {
        let heights = [30.0, 60.0, 30.0];
        let tops = stack(&heights, LINE_GAP); // 0, 44, 118
        let (body_top, body_h) = (54.0, 300.0);
        let hit = |scroll: f32, y: f32| hit_line(&tops, &heights, scroll, body_top, body_h, y);
        // Unscrolled: the body's top is the list's top.
        assert_eq!(hit(0.0, body_top + 5.0), Some(0));
        assert_eq!(hit(0.0, body_top + 50.0), Some(1));
        assert_eq!(hit(0.0, body_top + 130.0), Some(2));
        // Scrolled by 44: line 1 starts at the body's top.
        assert_eq!(hit(44.0, body_top + 1.0), Some(1));
        assert_eq!(hit(44.0, body_top + 80.0), Some(2));
        // The gap belongs to the nearer line (half the gap each side).
        assert_eq!(hit(0.0, body_top + 30.0 + 6.0), Some(0));
        assert_eq!(hit(0.0, body_top + 44.0 - 6.0), Some(1));
        // Beyond the last line, above the body, below it: none.
        assert_eq!(hit(0.0, body_top + 200.0), None);
        assert_eq!(hit(0.0, body_top - 1.0), None);
        assert_eq!(hit(0.0, body_top + body_h), None);
        assert_eq!(hit_line(&[], &[], 0.0, 0.0, 100.0, 10.0), None);
    }

    // ---- the manual scroll

    #[test]
    fn a_wheel_notch_scrolls_48_dip_and_stays_inside_the_bounds() {
        let mut m = ManualScroll::default();
        assert!(!m.held() && m.offset().is_none());
        let bounds = (0.0, 500.0);
        // One notch away from the user scrolls up: the offset shrinks.
        assert_eq!(m.wheel(200.0, 1.0, bounds, 0), 152.0);
        // The hold carries on from its own offset, not from the song's.
        assert_eq!(m.wheel(999.0, -2.0, bounds, 100), 248.0);
        assert_eq!(m.wheel(0.0, 100.0, bounds, 200), 0.0, "clamped at the first line");
        assert_eq!(m.wheel(0.0, -100.0, bounds, 300), 500.0, "and at the last");
        m.clamp((0.0, 100.0));
        assert_eq!(m.offset(), Some(100.0));
    }

    #[test]
    fn the_hold_ends_three_seconds_after_the_last_notch() {
        let mut m = ManualScroll::default();
        assert!(!m.due(10_000), "nothing held, nothing to return");
        assert_eq!(m.remaining_ms(0), None);
        m.wheel(0.0, -1.0, (0.0, 500.0), 1_000);
        assert!(m.held());
        assert!(!m.due(1_000 + 2_999));
        assert!(m.due(1_000 + 3_000));
        assert_eq!(m.remaining_ms(1_000), Some(MANUAL_HOLD_MS));
        assert_eq!(m.remaining_ms(3_500), Some(500));
        // Another notch pushes the end out.
        m.wheel(0.0, -1.0, (0.0, 500.0), 3_500);
        assert!(!m.due(3_500 + 2_999));
        assert!(m.due(3_500 + 3_000));
        m.release();
        assert!(!m.held() && !m.due(99_999));
        assert_eq!(MANUAL_HOLD_MS, 3000);
    }

    // ---- seeking

    #[test]
    fn a_click_seeks_to_its_line_with_the_lag() {
        let l = lines(&[0, 10_000, 20_000, 30_000]);
        for i in 0..l.len() {
            let ms = seek_ms(&l, i).unwrap();
            assert_eq!(ms, l[i].at_ms + OUTPUT_LAG_MS as u64 + SEEK_MARGIN_MS);
            // The line the notch would choose at that position is the one clicked.
            let got = current_line(&l, lyric_position(ms)).map(|(j, _)| j);
            assert_eq!(got, Some(i), "line {i}");
        }
        assert_eq!(seek_ms(&l, 9), None);
        assert_eq!(seek_ms(&[], 0), None);
    }

    #[test]
    fn a_seek_stays_below_the_next_lines_due_time() {
        // 40 ms apart: the margin would pass the next line's due time.
        let l = lines(&[5_000, 5_040, 9_000]);
        let ms = seek_ms(&l, 0).unwrap();
        assert!(ms < l[1].at_ms + OUTPUT_LAG_MS as u64, "{ms}");
        assert_eq!(
            current_line(&l, lyric_position(ms)).map(|(j, _)| j),
            Some(0)
        );
        // Right at the cap for lines 31 ms apart; 30 ms is the margin's reach.
        let l = lines(&[1_000, 1_031]);
        let ms = seek_ms(&l, 0).unwrap();
        assert_eq!(ms, 1_000 + 100 + 30);
        assert_eq!(current_line(&l, lyric_position(ms)).map(|(j, _)| j), Some(0));
        // The last line has nothing to stay below.
        let last = seek_ms(&l, 1).unwrap();
        assert_eq!(last, 1_031 + 130);
    }

    #[test]
    fn a_shifted_lrc_seeks_by_the_times_it_holds() {
        // The cache holds the intro-shifted text (`pick_best` shifted every
        // line 19 s later), so its `at_ms` already carry the shift and the
        // click adds nothing more.
        let original = [2_980u64, 8_400, 14_900];
        let shifted: Vec<u64> = original.iter().map(|t| t + 19_000).collect();
        let l = lines(&shifted);
        for i in 0..l.len() {
            let ms = seek_ms(&l, i).unwrap();
            assert_eq!(ms, original[i] + 19_000 + 130);
            assert_eq!(
                current_line(&l, lyric_position(ms)).map(|(j, _)| j),
                Some(i)
            );
        }
    }

    #[test]
    fn a_line_at_the_very_start_seeks_past_zero() {
        let l = lines(&[0, 4_000]);
        let ms = seek_ms(&l, 0).unwrap();
        assert_eq!(ms, 130);
        assert_eq!(current_line(&l, lyric_position(ms)).map(|(j, _)| j), Some(0));
    }

    // ---- small rules

    #[test]
    fn the_progress_fill_and_the_timer() {
        assert_eq!(progress(0, 0), 0.0);
        assert_eq!(progress(50, 200), 0.25);
        assert_eq!(progress(500, 200), 1.0);
        assert_eq!(timer_ms(None), 1000);
        assert_eq!(timer_ms(Some(250)), 250);
        assert_eq!(timer_ms(Some(60_000)), 1000);
        assert_eq!(timer_ms(Some(0)), 1);
    }

    #[test]
    fn a_press_is_a_drag_only_past_the_system_distance() {
        assert!(!is_drag(0, 0, 4, 4));
        assert!(!is_drag(4, -4, 4, 4));
        assert!(is_drag(5, 0, 4, 4));
        assert!(is_drag(0, -5, 4, 4));
    }

    #[test]
    fn a_resize_stays_inside_240_by_300_and_520_by_800() {
        assert_eq!(resized((300.0, 420.0), 50.0, 60.0), (350.0, 480.0));
        assert_eq!(resized((300.0, 420.0), -500.0, -500.0), (240.0, 300.0));
        assert_eq!(resized((300.0, 420.0), 900.0, 900.0), (520.0, 800.0));
        assert_eq!((MIN_W, MIN_H, MAX_W, MAX_H), (240.0, 300.0, 520.0, 800.0));
    }

    #[test]
    fn the_parts_sit_where_the_spec_says() {
        for (w, h) in [(300.0, 420.0), (240.0, 300.0), (520.0, 800.0)] {
            let r = card_rects(w, h);
            // The thumbnail: 30 dip at the 14 dip padding.
            assert_eq!(r.thumb, RectF::new(14.0, 14.0, 30.0, 30.0));
            // The close button: a 24 dip square at the top right, centred on the thumbnail.
            assert_eq!((r.close.w, r.close.h), (24.0, 24.0));
            assert!(r.close.right() <= w - 14.0 + 0.01);
            assert!((r.close.y + 12.0 - (r.thumb.y + 15.0)).abs() < 0.01);
            // Title and artist sit between the thumbnail and the close button.
            assert!(r.title.x >= r.thumb.right() && r.title.right() <= r.close.x);
            assert!(r.title.bottom() <= r.artist.y);
            // The body is between the header and the footer, no overlap.
            assert!(r.body.y >= r.header.bottom() - 0.01);
            assert!(r.body.bottom() < r.bar.y);
            assert_eq!(r.bar.h, 3.0);
            // The controls sit above the bar, inside the card, in a row.
            for c in r.controls {
                assert!(c.bottom() < r.bar.y && c.x >= 0.0 && c.right() <= w);
                assert_eq!(c.y, r.controls[0].y);
            }
            assert!(r.controls[0].right() < r.controls[1].x);
            assert!(r.controls[1].right() < r.controls[2].x);
            // The grip is 16 dip in the bottom right corner.
            assert_eq!(r.grip, RectF::new(w - 16.0, h - 16.0, 16.0, 16.0));
            assert!(r.body.h > 100.0, "{w}x{h}");
        }
    }

    #[test]
    fn zones_follow_what_is_shown() {
        let (w, h) = (300.0, 420.0);
        let r = card_rects(w, h);
        let at = |c: RectF, hovering| zone_at(&r, w, h, c.x + 1.0, c.y + 1.0, hovering, true);
        assert_eq!(at(r.close, false), Zone::Close);
        assert_eq!(at(r.close, true), Zone::Close);
        // The controls and the grip only exist while the pointer is in.
        assert_eq!(at(r.controls[1], true), Zone::PlayPause);
        assert_eq!(at(r.controls[0], true), Zone::Prev);
        assert_eq!(at(r.controls[2], true), Zone::Next);
        assert_eq!(at(r.controls[1], false), Zone::Body);
        assert_eq!(at(r.grip, true), Zone::Grip);
        assert_eq!(at(r.grip, false), Zone::Body);
        assert_eq!(zone_at(&r, w, h, 100.0, 5.0, true, true), Zone::Header);
        assert_eq!(zone_at(&r, w, h, 100.0, 150.0, true, true), Zone::Body);
        assert_eq!(zone_at(&r, w, h, -1.0, 150.0, true, true), Zone::Outside);
        assert_eq!(zone_at(&r, w, h, 100.0, h, true, true), Zone::Outside);
    }

    #[test]
    fn no_media_has_no_transport_controls() {
        let (w, h) = (300.0, 420.0);
        let r = card_rects(w, h);
        let at = |c: RectF| zone_at(&r, w, h, c.x + 1.0, c.y + 1.0, true, false);
        // Nothing to control: prev, play/pause and next are not there.
        for c in r.controls {
            assert_eq!(at(c), Zone::Body);
        }
        // The grip and the close button still work.
        assert_eq!(at(r.grip), Zone::Grip);
        assert_eq!(at(r.close), Zone::Close);
    }

    // ---- toggling

    #[test]
    fn the_toggle_rules() {
        // Lyrics on: a toggle flips it.
        assert!(card_next(false, true, Req::Toggle));
        assert!(!card_next(true, true, Req::Toggle));
        // Lyrics off: a toggle does nothing and keeps it closed.
        assert!(!card_next(false, false, Req::Toggle));
        // (An open card with lyrics off cannot be; a stale toggle closes it.)
        assert!(!card_next(true, false, Req::Toggle));
        // Lyrics off and Close close it whatever it was.
        for open in [false, true] {
            for on in [false, true] {
                assert!(!card_next(open, on, Req::LyricsOff));
                assert!(!card_next(open, on, Req::Close));
            }
        }
    }

    // ---- placement

    #[test]
    fn a_first_open_goes_top_right_of_the_notch_monitor() {
        let m = mon(rc(0, 0, 1920, 1080), 96);
        let r = place(None, &[m], Some(m)).unwrap();
        assert_eq!((r.width(), r.height()), (300, 420));
        assert_eq!(r.right, 1920 - 24);
        assert_eq!(r.top, 72);
        // At 150 % the same in dip.
        let m = mon(rc(1920, 0, 1920 + 2880, 1620), 144);
        let r = place(None, &[m], Some(m)).unwrap();
        assert_eq!((r.width(), r.height()), (450, 630));
        assert_eq!(r.right, 1920 + 2880 - 36);
        assert_eq!(r.top, 108);
        // No monitor at all: nothing to place.
        assert_eq!(place(None, &[], None), None);
    }

    #[test]
    fn a_small_work_area_keeps_the_card_inside() {
        let m = mon(rc(0, 0, 400, 380), 96);
        let r = place(None, &[m], Some(m)).unwrap();
        assert!(r.left >= m.work.left && r.right <= m.work.right);
        assert!(r.top >= m.work.top && r.bottom <= m.work.bottom, "{r:?}");
    }

    #[test]
    fn a_saved_place_that_is_mostly_visible_is_used_and_clamped() {
        let a = mon(rc(0, 0, 1920, 1080), 96);
        let b = mon(rc(1920, 0, 3840, 1080), 96);
        let saved = CardPlace {
            left: 2000,
            top: 100,
            right: 2400,
            bottom: 600,
        };
        // 400 x 500 is inside the limits: as saved.
        let r = place(Some(saved), &[a, b], Some(a)).unwrap();
        assert_eq!(r, rc(2000, 100, 2400, 600));
        // Too big or too small: pulled into 240..520 x 300..800, top left kept.
        let big = CardPlace {
            right: 2000 + 900,
            bottom: 100 + 1000,
            ..saved
        };
        assert_eq!(place(Some(big), &[a, b], Some(a)).unwrap(), rc(2000, 100, 2520, 900));
        let small = CardPlace {
            right: 2000 + 100,
            bottom: 100 + 100,
            ..saved
        };
        assert_eq!(place(Some(small), &[a, b], Some(a)).unwrap(), rc(2000, 100, 2240, 400));
        // At 200 % the limits double.
        let hi = mon(rc(0, 0, 3840, 2160), 192);
        let tiny = CardPlace {
            left: 100,
            top: 100,
            right: 200,
            bottom: 200,
        };
        assert_eq!(place(Some(tiny), &[hi], Some(hi)).unwrap(), rc(100, 100, 580, 700));
    }

    #[test]
    fn a_saved_place_off_every_monitor_falls_back_to_the_default() {
        let a = mon(rc(0, 0, 1920, 1080), 96);
        let gone = CardPlace {
            left: 4000,
            top: 100,
            right: 4300,
            bottom: 520,
        };
        let r = place(Some(gone), &[a], Some(a)).unwrap();
        assert_eq!(r, place(None, &[a], Some(a)).unwrap());
        // Mostly off: less than half on the monitor is "gone" too.
        let edge = CardPlace {
            left: 1800,
            top: 100,
            right: 2100,
            bottom: 520,
        };
        assert_eq!(place(Some(edge), &[a], Some(a)).unwrap(), place(None, &[a], Some(a)).unwrap());
        // Exactly half on is still used.
        let half = CardPlace {
            left: 1770,
            top: 100,
            right: 2070,
            bottom: 520,
        };
        assert_eq!(place(Some(half), &[a], Some(a)).unwrap().left, 1770);
        // An empty rect is never visible.
        let flat = CardPlace {
            left: 100,
            top: 100,
            right: 100,
            bottom: 500,
        };
        assert_eq!(place(Some(flat), &[a], Some(a)).unwrap(), place(None, &[a], Some(a)).unwrap());
    }
}
