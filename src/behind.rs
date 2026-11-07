//! "Di belakang jendela": with the setting on, the island does not stay on
//! top of everything. It sits at the bottom of the window order, so ordinary
//! windows cover it, and comes up when it has something to say or the pointer
//! touches the top edge of the screen over it. The decision is pure; `App`
//! reads the pointer and applies the order (`IslandWindow::set_front`).

use std::time::{Duration, Instant};

use crate::model::View;

/// The strip along the top edge of the screen that brings the island up, in
/// physical pixels.
pub const STRIP_PX: i32 = 3;
/// How long the island stays up after the pointer leaves it (and nothing is
/// showing) before it goes back down.
pub const LOWER_AFTER: Duration = Duration::from_millis(250);
/// How often the pointer is read near the top edge, or while lowering is due.
pub const POLL_MS: u32 = 80;
/// How often it is read otherwise: the pointer far below the top edge cannot
/// reach the strip within one slow poll unnoticed for long.
pub const SLOW_MS: u32 = 350;
/// "Near the top edge", in logical pixels from it.
pub const NEAR_TOP_DIP: f32 = 150.0;

/// The poll's pace: quick while lowering is due (`LOWER_AFTER` must be kept)
/// or the pointer is near the top edge (the strip, the island); slow
/// otherwise. A view that must be seen needs no poll to stay up: a change of
/// view re-reads at once.
pub fn poll_ms(lower_pending: bool, pointer_near_top: bool) -> u32 {
    if lower_pending || pointer_near_top {
        POLL_MS
    } else {
        SLOW_MS
    }
}

/// Where the island sits in the window order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Z {
    /// Above every window, as without the setting.
    Top,
    /// Below ordinary windows.
    Bottom,
}

/// Whether what the island shows must be seen: anything but the quiet
/// compact states. A notice, a panel, the search and a screenshot preview all
/// are; the idle island, the music and the timer in the notch are not.
pub fn view_needs_front(view: &View) -> bool {
    !matches!(
        view,
        View::Hidden
            | View::Idle
            | View::MediaCompact
            | View::MediaLyric
            | View::PomodoroCompact
            | View::RecordCompact
    )
}

/// What the decision reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    /// The view must be seen (`view_needs_front`).
    pub needs_front: bool,
    /// The pointer is on the top-edge strip over the island's width.
    pub on_strip: bool,
    /// The pointer is over the drawn island (counts only while it is up).
    pub over_island: bool,
}

/// The island's place in the window order and the time it has been idle up
/// there.
#[derive(Debug)]
pub struct Behind {
    z: Z,
    idle_since: Option<Instant>,
}

impl Default for Behind {
    fn default() -> Self {
        Behind {
            z: Z::Top,
            idle_since: None,
        }
    }
}

impl Behind {
    pub fn z(&self) -> Z {
        self.z
    }

    /// Up and counting down to going back down.
    pub fn lower_pending(&self) -> bool {
        self.idle_since.is_some()
    }

    /// Back to the start: up, nothing waiting.
    pub fn reset(&mut self) {
        *self = Behind::default();
    }

    /// The place the island should have now. Something to show, the pointer
    /// on the strip, or the pointer still on the island while it is up, keeps
    /// it up; once none of those holds for `LOWER_AFTER` it goes down.
    pub fn decide(&mut self, inputs: Inputs, now: Instant) -> Z {
        let hold =
            inputs.needs_front || inputs.on_strip || (inputs.over_island && self.z == Z::Top);
        if hold {
            self.z = Z::Top;
            self.idle_since = None;
        } else if self.z == Z::Top {
            let since = *self.idle_since.get_or_insert(now);
            if now.saturating_duration_since(since) >= LOWER_AFTER {
                self.z = Z::Bottom;
                self.idle_since = None;
            }
        }
        self.z
    }
}

/// How far past the drawn island the strip reaches on each side, in logical
/// pixels.
pub const STRIP_MARGIN_DIP: f32 = 12.0;

/// The strip along the top of the screen (`top` is its first row) over the
/// island's `span` (screen x, from and up to) and the point `pt` on the
/// screen: true when the point is on it. The span is the drawn island plus a
/// margin, not the window, which is wider than the island.
pub fn on_strip(top: i32, span: (i32, i32), pt: (i32, i32)) -> bool {
    pt.1 >= top && pt.1 < top + STRIP_PX && pt.0 >= span.0 && pt.0 < span.1
}

/// How long a rise takes with Motion on: a fade and a slide. Short enough to
/// feel like a reply to the touch, long enough to be seen as movement and not
/// a flicker (the morphs take 240 ms).
pub const RISE_MS: u64 = 180;
/// How long it takes with Motion not Full (Saver, Off, Energy Saver): a fade
/// only, so quicker, since nothing else shows that it is coming.
pub const RISE_SAVER_MS: u64 = 120;
/// How far above its place the island starts a sliding rise, in logical
/// pixels: it comes down out of the screen's top edge. Small, so the slide
/// reads as a settle and not as a drop.
pub const RISE_DROP_DIP: f32 = 8.0;

/// A rise from the bottom of the window order: the island, covered until now,
/// fades in (and slides down into its place) instead of popping up in one
/// frame. Pure; `App` draws it through `rise_frame` while it runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rise {
    start: Instant,
    ms: u64,
    drop_dip: f32,
}

impl Rise {
    /// A rise that starts at `now`; with `moves` false (Motion not Full) it
    /// only fades, and quicker.
    pub fn new(now: Instant, moves: bool) -> Self {
        Rise {
            start: now,
            ms: if moves { RISE_MS } else { RISE_SAVER_MS },
            drop_dip: if moves { RISE_DROP_DIP } else { 0.0 },
        }
    }

    /// How far along it is, 0 to 1.
    fn progress(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32();
        (elapsed * 1000.0 / self.ms as f32).clamp(0.0, 1.0)
    }

    /// The opacity factor (0 to 1) and the offset down in logical pixels
    /// (`-drop_dip` to 0) at `now`: ease-out cubic, so most of the change is
    /// early and the island settles gently.
    pub fn at(&self, now: Instant) -> (f32, f32) {
        let t = self.progress(now);
        if t >= 1.0 {
            return (1.0, 0.0);
        }
        let ease = 1.0 - (1.0 - t).powi(3);
        (ease, -self.drop_dip * (1.0 - ease))
    }

    /// It has not finished at `now`.
    pub fn running(&self, now: Instant) -> bool {
        self.progress(now) < 1.0
    }
}

/// The island rises smoothly only when it goes from the bottom to the top
/// and something covered it: one that was already visible (nothing over it)
/// must not blink out and back, and lowering stays instant.
pub fn rise_wanted(was: Z, now: Z, covered: bool) -> bool {
    was == Z::Bottom && now == Z::Top && covered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Notice, NoticeKind, Page};

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn quiet() -> Inputs {
        Inputs::default()
    }

    #[test]
    fn it_starts_up_and_goes_down_only_after_a_quiet_moment() {
        let t0 = Instant::now();
        let mut b = Behind::default();
        assert_eq!(b.z(), Z::Top);
        assert_eq!(b.decide(quiet(), t0), Z::Top, "the wait starts");
        assert_eq!(b.decide(quiet(), t0 + ms(240)), Z::Top);
        assert_eq!(b.decide(quiet(), t0 + ms(250)), Z::Bottom);
        assert_eq!(b.decide(quiet(), t0 + ms(900)), Z::Bottom, "it stays down");
    }

    #[test]
    fn the_strip_brings_it_up_and_the_pointer_on_it_keeps_it_up() {
        let t0 = Instant::now();
        let mut b = Behind::default();
        b.decide(quiet(), t0);
        b.decide(quiet(), t0 + ms(300));
        assert_eq!(b.z(), Z::Bottom);
        let strip = Inputs {
            on_strip: true,
            ..quiet()
        };
        assert_eq!(b.decide(strip, t0 + ms(400)), Z::Top);
        // The pointer comes down onto the island: it holds.
        let over = Inputs {
            over_island: true,
            ..quiet()
        };
        for i in 1..20 {
            assert_eq!(b.decide(over, t0 + ms(400 + i * 80)), Z::Top);
        }
        // It leaves: 250 ms later the island goes down.
        let gone = t0 + ms(2_000);
        assert_eq!(b.decide(quiet(), gone), Z::Top);
        assert_eq!(b.decide(quiet(), gone + ms(250)), Z::Bottom);
    }

    #[test]
    fn being_over_the_island_alone_does_not_raise_it_from_the_bottom() {
        // Down there another window may be what the pointer is over.
        let t0 = Instant::now();
        let mut b = Behind::default();
        b.decide(quiet(), t0);
        b.decide(quiet(), t0 + ms(300));
        let over = Inputs {
            over_island: true,
            ..quiet()
        };
        assert_eq!(b.decide(over, t0 + ms(400)), Z::Bottom);
    }

    #[test]
    fn something_to_show_raises_it_at_once_and_keeps_it_up() {
        let t0 = Instant::now();
        let mut b = Behind::default();
        b.decide(quiet(), t0);
        b.decide(quiet(), t0 + ms(300));
        let notice = Inputs {
            needs_front: true,
            ..quiet()
        };
        assert_eq!(b.decide(notice, t0 + ms(310)), Z::Top);
        assert_eq!(b.decide(notice, t0 + ms(60_000)), Z::Top);
        // Once it is gone the quiet moment starts from then.
        let gone = t0 + ms(61_000);
        assert_eq!(b.decide(quiet(), gone), Z::Top);
        assert_eq!(b.decide(quiet(), gone + ms(250)), Z::Bottom);
    }

    #[test]
    fn a_pointer_that_returns_restarts_the_wait() {
        let t0 = Instant::now();
        let mut b = Behind::default();
        b.decide(quiet(), t0);
        assert_eq!(b.decide(quiet(), t0 + ms(200)), Z::Top);
        let over = Inputs {
            over_island: true,
            ..quiet()
        };
        b.decide(over, t0 + ms(220));
        assert_eq!(b.decide(quiet(), t0 + ms(240)), Z::Top, "a new wait");
        assert_eq!(b.decide(quiet(), t0 + ms(490)), Z::Bottom);
    }

    #[test]
    fn the_poll_slows_down_only_far_from_the_top_with_nothing_due() {
        assert_eq!(poll_ms(false, false), SLOW_MS);
        assert_eq!(poll_ms(false, true), POLL_MS, "near the strip");
        assert_eq!(poll_ms(true, false), POLL_MS, "lowering is due");
        // The countdown to lowering shows as pending until it lands.
        let t0 = Instant::now();
        let mut b = Behind::default();
        assert!(!b.lower_pending());
        b.decide(quiet(), t0);
        assert!(b.lower_pending());
        b.decide(quiet(), t0 + LOWER_AFTER);
        assert!(!b.lower_pending() && b.z() == Z::Bottom);
    }

    #[test]
    fn reset_puts_it_back_up() {
        let t0 = Instant::now();
        let mut b = Behind::default();
        b.decide(quiet(), t0);
        b.decide(quiet(), t0 + ms(300));
        b.reset();
        assert_eq!(b.z(), Z::Top);
    }

    #[test]
    fn only_the_quiet_views_may_stay_behind() {
        for view in [
            View::Hidden,
            View::Idle,
            View::MediaCompact,
            View::MediaLyric,
            View::PomodoroCompact,
        ] {
            assert!(!view_needs_front(&view), "{view:?}");
        }
        assert!(view_needs_front(&View::Panel(Page::Calendar)));
        assert!(view_needs_front(&View::Panel(Page::Search)));
        let notice = Notice {
            key: "n".into(),
            source: "s".into(),
            title: "t".into(),
            body: String::new(),
            kind: NoticeKind::Copied,
        };
        assert!(view_needs_front(&View::Notice(notice)));
    }

    #[test]
    fn the_strip_is_three_pixels_over_the_drawn_island_only() {
        // The island is drawn from x 400 to 600; the window around it is wider.
        let span = (400, 600);
        assert!(on_strip(0, span, (500, 0)));
        assert!(on_strip(0, span, (500, 2)));
        assert!(!on_strip(0, span, (500, 3)));
        assert!(!on_strip(0, span, (399, 0)));
        assert!(on_strip(0, span, (400, 0)));
        assert!(!on_strip(0, span, (600, 0)));
        assert!(
            !on_strip(0, span, (200, 0)),
            "beside the island, over the window"
        );
        assert!(!on_strip(0, span, (500, -1)));
    }

    #[test]
    fn a_covered_island_rises_only_from_the_bottom() {
        assert!(rise_wanted(Z::Bottom, Z::Top, true));
        assert!(!rise_wanted(Z::Bottom, Z::Top, false), "nothing covers it");
        assert!(!rise_wanted(Z::Top, Z::Top, true), "already up");
        assert!(
            !rise_wanted(Z::Top, Z::Bottom, true),
            "lowering stays instant"
        );
        assert!(!rise_wanted(Z::Bottom, Z::Bottom, true), "still down");
    }

    #[test]
    fn a_rise_fades_in_and_slides_down_over_its_time() {
        let t0 = Instant::now();
        let rise = Rise::new(t0, true);
        assert_eq!(rise.at(t0), (0.0, -RISE_DROP_DIP));
        let (opacity, dy) = rise.at(t0 + ms(90));
        assert!(opacity > 0.0 && opacity < 1.0, "{opacity}");
        assert!(dy > -RISE_DROP_DIP && dy < 0.0, "{dy}");
        assert!(opacity > 0.5, "ease-out: more than half done at half time");
        assert_eq!(rise.at(t0 + ms(180)), (1.0, 0.0));
        assert_eq!(rise.at(t0 + ms(500)), (1.0, 0.0));
        assert!(rise.running(t0));
        assert!(rise.running(t0 + ms(179)));
        assert!(!rise.running(t0 + ms(180)));
        assert!(!rise.running(t0 + ms(500)));
    }

    #[test]
    fn a_rise_without_motion_only_fades_and_is_quicker() {
        let t0 = Instant::now();
        let rise = Rise::new(t0, false);
        for t in (0..=200).step_by(10) {
            assert_eq!(rise.at(t0 + ms(t)).1, 0.0, "no slide at {t} ms");
        }
        assert_eq!(rise.at(t0).0, 0.0);
        let mid = rise.at(t0 + ms(60)).0;
        assert!(mid > 0.0 && mid < 1.0, "{mid}");
        assert!(rise.running(t0 + ms(119)));
        assert!(!rise.running(t0 + ms(120)));
        assert_eq!(rise.at(t0 + ms(120)), (1.0, 0.0));
    }

    #[test]
    fn a_rise_never_goes_backwards() {
        let t0 = Instant::now();
        for moves in [true, false] {
            let rise = Rise::new(t0, moves);
            let (mut opacity, mut dy) = rise.at(t0);
            for t in 0..=200 {
                let (o, d) = rise.at(t0 + ms(t));
                assert!(o >= opacity, "opacity fell at {t} ms");
                assert!(d >= dy && d <= 0.0, "dy moved away from 0 at {t} ms");
                (opacity, dy) = (o, d);
            }
        }
    }
}
