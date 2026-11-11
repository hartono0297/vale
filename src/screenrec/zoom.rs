//! The zoom camera of a screen recording: which part of the captured
//! source goes into the video. Pure maths, no Windows calls; the recorder
//! feeds it the cursor and the clock once per frame.
//!
//! Zoom is in the video only: the user's screen never changes. The view is a
//! rect in source pixels with the output's aspect. At level 1 it covers the
//! whole source (with bars when the aspects differ, after a window resize);
//! at level `L` it is `1/L` of that, centred on the cursor and kept inside
//! the source. Level changes ease over [`TRANSITION_MS`]; the centre follows
//! the cursor with an exponential smoothing that gives the same path at 30
//! and 60 fps.

/// A rect in source pixels (fractions allowed: the compositor rounds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl RectF {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> RectF {
        RectF { x, y, w, h }
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
}

pub const MIN_LEVEL: f64 = 1.0;
pub const MAX_LEVEL: f64 = 4.0;
pub const DEFAULT_LEVEL: f64 = 2.0;
/// The lowest level a `zoom_step` reaches: stepping out stops here, the
/// whole view is the toggle's job.
pub const MIN_STEP_LEVEL: f64 = 1.5;
/// One `zoom_step`.
pub const LEVEL_STEP: f64 = 0.5;
/// Zooming in or out takes this long.
pub const TRANSITION_MS: f64 = 350.0;
/// The follow's time constant: after this long the centre has covered 63 %
/// of the way to the cursor.
pub const FOLLOW_TAU_MS: f64 = 140.0;

/// The smallest rect with `aspect` (w / h) that covers `bounds`, centred on it.
fn cover(bounds: RectF, aspect: f64) -> RectF {
    let (cx, cy) = bounds.center();
    let (w, h) = if bounds.w / bounds.h.max(1e-9) >= aspect {
        (bounds.w, bounds.w / aspect)
    } else {
        (bounds.h * aspect, bounds.h)
    };
    RectF::new(cx - w / 2.0, cy - h / 2.0, w, h)
}

/// One axis of the clamp: the start of a span `len` centred near `want`,
/// kept inside `[lo, lo + room]`; centred on the room when it does not fit.
fn clamp_axis(want: f64, len: f64, lo: f64, room: f64) -> f64 {
    if len >= room {
        lo + (room - len) / 2.0
    } else {
        (want - len / 2.0).clamp(lo, lo + room - len)
    }
}

/// The view at `level` around `center`: `1/level` of the rect covering
/// `bounds` with `aspect`, kept inside `bounds` on each axis it fits in.
pub fn view_rect(bounds: RectF, aspect: f64, level: f64, center: (f64, f64)) -> RectF {
    let base = cover(bounds, aspect);
    let level = level.clamp(MIN_LEVEL, MAX_LEVEL);
    let (w, h) = (base.w / level, base.h / level);
    RectF::new(
        clamp_axis(center.0, w, bounds.x, bounds.w),
        clamp_axis(center.1, h, bounds.y, bounds.h),
        w,
        h,
    )
}

/// Ease in and out (cubic), `t` in 0..=1.
pub fn ease_in_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// The share of the remaining distance covered in `dt_ms` by the follow.
/// `1 - e^(-dt/tau)`: two steps of `dt` equal one step of `2 dt`.
pub fn follow_alpha(dt_ms: f64) -> f64 {
    if dt_ms <= 0.0 {
        0.0
    } else {
        1.0 - (-dt_ms / FOLLOW_TAU_MS).exp()
    }
}

/// The level after one step up (`dir > 0`) or down: half a level, kept in
/// `[MIN_STEP_LEVEL, MAX_LEVEL]`. `dir` 0 keeps the level.
pub fn next_level(level: f64, dir: i32) -> f64 {
    (level + LEVEL_STEP * dir.signum() as f64).clamp(MIN_STEP_LEVEL, MAX_LEVEL)
}

/// The island's mirror of the camera's switch and level, so it can show
/// the zoom at once while the recorder thread applies the same commands in
/// order.
// Wired to the island's hotkeys in the next task.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomState {
    pub on: bool,
    pub level: f64,
}

impl Default for ZoomState {
    fn default() -> ZoomState {
        ZoomState {
            on: false,
            level: DEFAULT_LEVEL,
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
impl ZoomState {
    pub fn toggle(&mut self) {
        self.on = !self.on;
    }

    /// One step; true when the level changed (on or off, like the camera).
    pub fn step(&mut self, dir: i32) -> bool {
        let next = next_level(self.level, dir);
        let changed = next != self.level;
        self.level = next;
        changed
    }

    /// The level while on.
    pub fn shown(&self) -> Option<f64> {
        self.on.then_some(self.level)
    }
}

/// A level change in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Transition {
    from: f64,
    to: f64,
    start_ms: f64,
}

impl Transition {
    fn at(&self, now_ms: f64) -> f64 {
        let t = (now_ms - self.start_ms) / TRANSITION_MS;
        self.from + (self.to - self.from) * ease_in_out(t)
    }

    fn done(&self, now_ms: f64) -> bool {
        now_ms - self.start_ms >= TRANSITION_MS
    }
}

/// The camera: on or off, the level to use when on, the animated level and
/// the smoothed centre.
#[derive(Clone, Debug)]
pub struct ZoomCamera {
    bounds: RectF,
    aspect: f64,
    on: bool,
    /// The level used while on (`zoom_step` changes it).
    level: f64,
    /// The level shown at `last_ms`.
    shown: f64,
    transition: Option<Transition>,
    center: (f64, f64),
    last_ms: Option<f64>,
}

impl ZoomCamera {
    /// A camera over `bounds` (source pixels) for an `out_w` x `out_h` video.
    pub fn new(bounds: RectF, out_w: u32, out_h: u32) -> ZoomCamera {
        ZoomCamera {
            bounds,
            aspect: out_w.max(1) as f64 / out_h.max(1) as f64,
            on: false,
            level: DEFAULT_LEVEL,
            shown: MIN_LEVEL,
            transition: None,
            center: bounds.center(),
            last_ms: None,
        }
    }

    /// The source changed size (a monitor mode change, a window resize).
    pub fn set_bounds(&mut self, bounds: RectF) {
        self.bounds = bounds;
    }

    pub fn bounds(&self) -> RectF {
        self.bounds
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// The level used while on.
    pub fn level(&self) -> f64 {
        self.level
    }

    /// The level on screen now (between 1 and `level` during a transition).
    #[cfg(test)]
    pub fn shown_level(&self) -> f64 {
        self.shown
    }

    fn animate_to(&mut self, to: f64, now_ms: f64) {
        let from = self.level_at(now_ms);
        self.transition = Some(Transition {
            from,
            to,
            start_ms: now_ms,
        });
    }

    fn level_at(&self, now_ms: f64) -> f64 {
        match self.transition {
            Some(t) => t.at(now_ms),
            None => self.shown,
        }
    }

    /// Zoom in to the cursor, or back out.
    pub fn toggle(&mut self, now_ms: f64, cursor: (f64, f64)) {
        self.set_on(!self.on, now_ms, cursor);
    }

    pub fn set_on(&mut self, on: bool, now_ms: f64, cursor: (f64, f64)) {
        if on == self.on {
            return;
        }
        // Zooming in from the whole view starts at the cursor: no glide
        // from wherever the centre was left.
        if on && self.level_at(now_ms) <= MIN_LEVEL + 1e-6 {
            self.center = cursor;
        }
        self.on = on;
        let to = if on { self.level } else { MIN_LEVEL };
        self.animate_to(to, now_ms);
    }

    /// One step up (`dir > 0`) or down. While on, the view eases to the new
    /// level. Returns the new level.
    pub fn step(&mut self, dir: i32, now_ms: f64) -> f64 {
        let next = next_level(self.level, dir);
        if next != self.level {
            self.level = next;
            if self.on {
                self.animate_to(next, now_ms);
            }
        }
        self.level
    }

    /// Advances to `now_ms` with the cursor at `cursor` (source pixels) and
    /// returns the view.
    pub fn update(&mut self, now_ms: f64, cursor: (f64, f64)) -> RectF {
        let dt = self.last_ms.map_or(0.0, |last| now_ms - last);
        self.last_ms = Some(now_ms);
        let a = follow_alpha(dt);
        self.center.0 += (cursor.0 - self.center.0) * a;
        self.center.1 += (cursor.1 - self.center.1) * a;
        self.shown = self.level_at(now_ms);
        if self.transition.is_some_and(|t| t.done(now_ms)) {
            self.transition = None;
        }
        view_rect(self.bounds, self.aspect, self.shown, self.center)
    }
}

// The auto-zoom below is phase 4's design hook: tested, not wired yet.

/// What the later auto-zoom reacts to. Positions in source pixels.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    Click {
        at: (f64, f64),
    },
    /// A key that types (letters, digits, Enter, Backspace), at the cursor
    /// (the caret is not known to the recorder).
    Typing {
        at: (f64, f64),
    },
    Move {
        at: (f64, f64),
    },
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AutoAction {
    ZoomIn { at: (f64, f64) },
    ZoomOut,
}

/// The auto-zoom rules (phase 4, not wired yet): a click or typing zooms in;
/// a quiet spell or a long move away zooms back out.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct AutoZoom {
    /// Out after this long without a click or a key.
    pub idle_ms: f64,
    /// Out when the cursor goes further than this from the zoom's anchor,
    /// as a share of the source's diagonal.
    pub leave_share: f64,
    /// No second zoom-in within this long of a zoom-out (no ping-pong).
    pub cooldown_ms: f64,
    diagonal: f64,
    zoomed: Option<(f64, f64)>,
    last_activity_ms: f64,
    out_at_ms: f64,
}

#[cfg_attr(not(test), allow(dead_code))]
impl AutoZoom {
    pub fn new(source_w: f64, source_h: f64) -> AutoZoom {
        AutoZoom {
            idle_ms: 2500.0,
            leave_share: 0.35,
            cooldown_ms: 600.0,
            diagonal: source_w.hypot(source_h),
            zoomed: None,
            last_activity_ms: f64::NEG_INFINITY,
            out_at_ms: f64::NEG_INFINITY,
        }
    }

    /// Feeds one event; returns what the camera should do, if anything.
    pub fn feed(&mut self, event: InputEvent, now_ms: f64) -> Option<AutoAction> {
        match event {
            InputEvent::Click { at } | InputEvent::Typing { at } => {
                self.last_activity_ms = now_ms;
                match self.zoomed {
                    Some(_) => {
                        // Already in: the click moves the anchor along.
                        self.zoomed = Some(at);
                        None
                    }
                    None if now_ms - self.out_at_ms < self.cooldown_ms => None,
                    None => {
                        self.zoomed = Some(at);
                        Some(AutoAction::ZoomIn { at })
                    }
                }
            }
            InputEvent::Move { at } => {
                let anchor = self.zoomed?;
                let far =
                    (at.0 - anchor.0).hypot(at.1 - anchor.1) > self.leave_share * self.diagonal;
                far.then(|| self.out(now_ms))
            }
        }
    }

    /// Called every frame: zooms out after the quiet spell.
    pub fn tick(&mut self, now_ms: f64) -> Option<AutoAction> {
        (self.zoomed.is_some() && now_ms - self.last_activity_ms >= self.idle_ms)
            .then(|| self.out(now_ms))
    }

    fn out(&mut self, now_ms: f64) -> AutoAction {
        self.zoomed = None;
        self.out_at_ms = now_ms;
        AutoAction::ZoomOut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HD: RectF = RectF::new(0.0, 0.0, 1920.0, 1080.0);

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn rect_close(a: RectF, b: RectF) -> bool {
        close(a.x, b.x) && close(a.y, b.y) && close(a.w, b.w) && close(a.h, b.h)
    }

    #[test]
    fn level_one_shows_the_whole_source() {
        let v = view_rect(HD, 16.0 / 9.0, 1.0, (100.0, 100.0));
        assert!(rect_close(v, HD));
    }

    #[test]
    fn level_two_is_half_the_size_around_the_cursor() {
        let v = view_rect(HD, 16.0 / 9.0, 2.0, (960.0, 540.0));
        assert!(rect_close(v, RectF::new(480.0, 270.0, 960.0, 540.0)));
    }

    #[test]
    fn the_view_stays_inside_the_source_at_the_edges() {
        let v = view_rect(HD, 16.0 / 9.0, 2.0, (0.0, 0.0));
        assert!(rect_close(v, RectF::new(0.0, 0.0, 960.0, 540.0)));
        let v = view_rect(HD, 16.0 / 9.0, 4.0, (1920.0, 1080.0));
        assert!(rect_close(v, RectF::new(1440.0, 810.0, 480.0, 270.0)));
    }

    #[test]
    fn the_view_keeps_the_output_aspect() {
        // A region of another shape: the video is 4:3, the source 16:9.
        let v = view_rect(HD, 4.0 / 3.0, 2.0, (960.0, 540.0));
        assert!(close(v.w / v.h, 4.0 / 3.0));
    }

    #[test]
    fn a_source_of_another_shape_gets_bars_at_level_one() {
        // A window grew taller than the video's aspect: the view covers it,
        // centred, wider than the source (the compositor draws bars).
        let tall = RectF::new(0.0, 0.0, 800.0, 900.0);
        let v = view_rect(tall, 16.0 / 9.0, 1.0, (0.0, 0.0));
        assert!(close(v.h, 900.0));
        assert!(close(v.w, 1600.0));
        assert!(close(v.center().0, 400.0));
        // Zoomed, the axis that fits is clamped again.
        let v = view_rect(tall, 16.0 / 9.0, 4.0, (0.0, 0.0));
        assert!(close(v.x, 0.0) && close(v.y, 0.0));
    }

    #[test]
    fn a_region_is_offset_inside_the_monitor() {
        let region = RectF::new(100.0, 200.0, 640.0, 360.0);
        let v = view_rect(region, 16.0 / 9.0, 2.0, (0.0, 0.0));
        assert!(rect_close(v, RectF::new(100.0, 200.0, 320.0, 180.0)));
    }

    #[test]
    fn levels_are_clamped() {
        let v = view_rect(HD, 16.0 / 9.0, 10.0, (960.0, 540.0));
        assert!(close(v.w, 480.0));
        let v = view_rect(HD, 16.0 / 9.0, 0.2, (960.0, 540.0));
        assert!(close(v.w, 1920.0));
    }

    #[test]
    fn the_ease_starts_and_ends_still_and_is_monotonic() {
        assert!(close(ease_in_out(0.0), 0.0));
        assert!(close(ease_in_out(0.5), 0.5));
        assert!(close(ease_in_out(1.0), 1.0));
        assert!(close(ease_in_out(2.0), 1.0));
        let mut last = 0.0;
        for i in 1..=100 {
            let v = ease_in_out(i as f64 / 100.0);
            assert!(v >= last);
            last = v;
        }
        // Slow at the ends.
        assert!(ease_in_out(0.05) < 0.05);
        assert!(ease_in_out(0.95) > 0.95);
    }

    #[test]
    fn zooming_in_takes_the_transition_time() {
        let mut cam = ZoomCamera::new(HD, 1920, 1080);
        cam.update(0.0, (960.0, 540.0));
        cam.toggle(0.0, (960.0, 540.0));
        assert!(cam.is_on());
        cam.update(0.0, (960.0, 540.0));
        assert!(close(cam.shown_level(), 1.0));
        cam.update(TRANSITION_MS / 2.0, (960.0, 540.0));
        assert!(close(cam.shown_level(), 1.5));
        let v = cam.update(TRANSITION_MS, (960.0, 540.0));
        assert!(close(cam.shown_level(), 2.0));
        assert!(close(v.w, 960.0));
        cam.update(TRANSITION_MS + 1000.0, (960.0, 540.0));
        assert!(close(cam.shown_level(), 2.0));
    }

    #[test]
    fn toggling_mid_way_turns_back_without_a_jump() {
        let mut cam = ZoomCamera::new(HD, 1920, 1080);
        cam.update(0.0, (960.0, 540.0));
        cam.toggle(0.0, (960.0, 540.0));
        cam.update(175.0, (960.0, 540.0));
        let mid = cam.shown_level();
        cam.toggle(175.0, (960.0, 540.0));
        assert!(!cam.is_on());
        cam.update(175.0, (960.0, 540.0));
        assert!(close(cam.shown_level(), mid));
        cam.update(175.0 + TRANSITION_MS, (960.0, 540.0));
        assert!(close(cam.shown_level(), 1.0));
    }

    #[test]
    fn zooming_in_starts_at_the_cursor() {
        let mut cam = ZoomCamera::new(HD, 1920, 1080);
        cam.update(0.0, (960.0, 540.0));
        cam.toggle(10.0, (300.0, 300.0));
        let v = cam.update(10.0 + TRANSITION_MS, (300.0, 300.0));
        assert!(rect_close(v, RectF::new(0.0, 30.0, 960.0, 540.0)));
    }

    #[test]
    fn the_follow_is_frame_rate_independent() {
        let run = |fps: f64| {
            let mut cam = ZoomCamera::new(HD, 1920, 1080);
            cam.update(0.0, (960.0, 540.0));
            cam.toggle(0.0, (960.0, 540.0));
            let frame = 1000.0 / fps;
            let mut t = 0.0;
            let mut v = cam.update(0.0, (960.0, 540.0));
            while t < 1000.0 - 1e-9 {
                t += frame;
                v = cam.update(t, (1400.0, 700.0));
            }
            v
        };
        let (a, b) = (run(30.0), run(60.0));
        assert!((a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5);
        // And it gets there.
        assert!((a.center().0 - 1400.0).abs() < 5.0);
    }

    #[test]
    fn follow_alpha_composes() {
        let one = follow_alpha(66.0);
        let two = 1.0 - (1.0 - follow_alpha(33.0)).powi(2);
        assert!(close(one, two));
        assert!(close(follow_alpha(0.0), 0.0));
        assert!(close(follow_alpha(-5.0), 0.0));
    }

    #[test]
    fn next_level_clamps() {
        assert!(close(next_level(2.0, 1), 2.5));
        assert!(close(next_level(2.0, -1), 1.5));
        assert!(close(next_level(1.5, -1), 1.5));
        assert!(close(next_level(4.0, 1), 4.0));
        assert!(close(next_level(2.0, 0), 2.0));
        assert!(close(next_level(2.0, 7), 2.5), "one step whatever the size");
    }

    #[test]
    fn a_step_at_the_edge_changes_nothing() {
        let mut z = ZoomState { on: true, level: MAX_LEVEL };
        assert!(!z.step(1));
        assert_eq!(z.shown(), Some(MAX_LEVEL));
        let mut z = ZoomState { on: true, level: MIN_STEP_LEVEL };
        assert!(!z.step(-1));
        assert!(z.step(1));
        assert_eq!(z.shown(), Some(2.0));
    }

    #[test]
    fn the_mirror_starts_off_at_two_and_toggles() {
        let mut z = ZoomState::default();
        assert_eq!(z.shown(), None);
        z.toggle();
        assert_eq!(z.shown(), Some(DEFAULT_LEVEL));
        z.toggle();
        assert_eq!(z.shown(), None);
    }

    #[test]
    fn the_mirror_agrees_with_the_camera() {
        // The island applies each press to its ZoomState at once; the recorder
        // thread applies the same commands later, in order (also after a pause).
        let mut cam = ZoomCamera::new(HD, 1920, 1080);
        let mut z = ZoomState::default();
        let presses = [0, 1, 1, 1, 1, 1, 0, -1, 0, -1, -1, -1, -1, 0, 1];
        for (i, &p) in presses.iter().enumerate() {
            let t = i as f64 * 1000.0;
            if p == 0 {
                z.toggle();
                cam.toggle(t, (100.0, 100.0));
            } else if z.step(p) {
                cam.step(p, t);
            }
            assert_eq!(cam.is_on(), z.on, "press {i}");
            assert!(close(cam.level(), z.level), "press {i}");
        }
    }

    #[test]
    fn steps_change_the_level_within_bounds() {
        let mut cam = ZoomCamera::new(HD, 1920, 1080);
        assert!(close(cam.level(), DEFAULT_LEVEL));
        for _ in 0..10 {
            cam.step(1, 0.0);
        }
        assert!(close(cam.level(), MAX_LEVEL));
        for _ in 0..10 {
            cam.step(-1, 0.0);
        }
        assert!(close(cam.level(), MIN_STEP_LEVEL));
        // Off: the step changes the level but not the view.
        cam.step(1, 0.0);
        let v = cam.update(1000.0, (0.0, 0.0));
        assert!(rect_close(v, HD));
    }

    #[test]
    fn a_step_while_zoomed_eases_to_the_new_level() {
        let mut cam = ZoomCamera::new(HD, 1920, 1080);
        cam.update(0.0, (960.0, 540.0));
        cam.toggle(0.0, (960.0, 540.0));
        cam.update(TRANSITION_MS, (960.0, 540.0));
        cam.step(1, 1000.0);
        cam.update(1000.0 + TRANSITION_MS / 2.0, (960.0, 540.0));
        assert!(close(cam.shown_level(), 2.25));
        cam.update(1000.0 + TRANSITION_MS, (960.0, 540.0));
        assert!(close(cam.shown_level(), 2.5));
    }

    #[test]
    fn auto_zoom_goes_in_on_a_click_and_out_when_quiet() {
        let mut auto = AutoZoom::new(1920.0, 1080.0);
        let at = (500.0, 400.0);
        assert_eq!(
            auto.feed(InputEvent::Click { at }, 0.0),
            Some(AutoAction::ZoomIn { at })
        );
        // Typing keeps it in and is not a second zoom-in.
        assert_eq!(auto.feed(InputEvent::Typing { at }, 2000.0), None);
        assert_eq!(auto.tick(4000.0), None);
        assert_eq!(auto.tick(4500.0), Some(AutoAction::ZoomOut));
        assert_eq!(auto.tick(9000.0), None);
    }

    #[test]
    fn auto_zoom_goes_out_on_a_long_move_and_waits_before_going_back_in() {
        let mut auto = AutoZoom::new(1920.0, 1080.0);
        auto.feed(InputEvent::Click { at: (100.0, 100.0) }, 0.0);
        // A short move stays.
        assert_eq!(
            auto.feed(InputEvent::Move { at: (300.0, 200.0) }, 100.0),
            None
        );
        assert_eq!(
            auto.feed(
                InputEvent::Move {
                    at: (1500.0, 900.0)
                },
                200.0
            ),
            Some(AutoAction::ZoomOut)
        );
        // Moves while out do nothing.
        assert_eq!(auto.feed(InputEvent::Move { at: (0.0, 0.0) }, 250.0), None);
        // Too soon after the zoom-out.
        assert_eq!(
            auto.feed(InputEvent::Click { at: (10.0, 10.0) }, 400.0),
            None
        );
        assert_eq!(
            auto.feed(InputEvent::Click { at: (10.0, 10.0) }, 900.0),
            Some(AutoAction::ZoomIn { at: (10.0, 10.0) })
        );
    }
}
