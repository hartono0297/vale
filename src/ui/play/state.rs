//! The clock of one play: how far a story has got, how it eases home when
//! something cuts it short, and how the island makes room while it plays.

use std::time::{Duration, Instant};

use crate::ui::geometry::{CLOCK_W, IdleLayout, Rect, ShapeRect};
use crate::ui::motion::ease_in_out_cubic;
use crate::ui::play::trigger::{Story, WIND_DOWN};

/// What a running play asks of the island at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beat {
    /// 0 with the mascot and the clock at rest, 1 with them at the island's ends.
    pub spread: f32,
    /// Scales every deformation; exactly 0.0 leaves the island at rest.
    pub alpha: f32,
    /// The story's own seconds; keeps advancing through the wind-down.
    pub t: f32,
}

pub struct Play {
    story: Story,
    started: Instant,
    ending: Option<Instant>,
    instant: bool,
}

impl Play {
    pub fn start(story: Story, now: Instant) -> Self {
        Self {
            story,
            started: now,
            ending: None,
            instant: false,
        }
    }

    pub fn story(&self) -> Story {
        self.story
    }

    /// When the wind-down begins: the first `end`, or the story's own time.
    fn wind_start(&self) -> Instant {
        let natural = self.started + Duration::from_secs_f32(self.story.secs());
        self.ending.map_or(natural, |e| e.min(natural))
    }

    pub fn beat(&self, now: Instant) -> Beat {
        if self.instant {
            return Beat { spread: 0.0, alpha: 0.0, t: 0.0 };
        }
        let t = now.saturating_duration_since(self.started).as_secs_f32();
        let wind = self.wind_start();
        // The opening stops where the cut finds it, so the spread only falls.
        let opened = now.min(wind).saturating_duration_since(self.started);
        let rise = ease_in_out_cubic(opened.as_secs_f32() / WIND_DOWN.as_secs_f32());
        let down = now.saturating_duration_since(wind).as_secs_f32();
        let alpha = 1.0 - ease_in_out_cubic(down / WIND_DOWN.as_secs_f32());
        Beat { spread: rise * alpha, alpha, t }
    }

    /// Begin the wind-down now. The first call fixes the end.
    pub fn end(&mut self, now: Instant) {
        self.ending.get_or_insert(now);
    }

    pub fn done(&self, now: Instant) -> bool {
        self.instant || now >= self.wind_start() + WIND_DOWN
    }

    /// Land at rest at once, for when motion is off.
    pub fn finish_now(&mut self) {
        self.instant = true;
    }
}

/// `layout` with the mascot eased toward the island's left end and the clock
/// toward its right end by `spread` (0 leaves it untouched). A clock that
/// filled the island shrinks to its own width on the way.
pub fn spread_offsets(layout: IdleLayout, s: ShapeRect, spread: f32) -> IdleLayout {
    if spread == 0.0 {
        return layout;
    }
    // Clear of the rounded ends, in step with the island's height and radius.
    let margin = s.radius * 0.5 + (s.h - crate::ui::geometry::ROBOT_COMPACT).max(0.0) / 2.0;
    let lerp = |a: f32, b: f32| a + (b - a) * spread;
    IdleLayout {
        robot: layout.robot.map(|r| Rect { x: lerp(r.x, s.x + margin), ..r }),
        clock: layout.clock.map(|c| Rect {
            x: lerp(c.x, s.x + s.w - margin - CLOCK_W),
            w: lerp(c.w, CLOCK_W),
            ..c
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::ui::geometry::idle_layout_lead;
    use crate::ui::play::testing::shape;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn a_wind_down_takes_two_hundred_ms_and_lands_at_rest() {
        let t0 = Instant::now();
        let mut p = Play::start(Story::MalingAngka, t0);
        p.end(t0 + Duration::from_secs(1));
        assert!(!p.done(t0 + MS(1199)));
        assert!(p.done(t0 + MS(1200)));
        let last = p.beat(t0 + MS(1200));
        assert_eq!((last.alpha, last.spread), (0.0, 0.0), "the island is exactly at rest");
    }

    #[test]
    fn ending_twice_does_not_extend_the_wind_down() {
        let t0 = Instant::now();
        let mut p = Play::start(Story::Bungee, t0);
        p.end(t0 + Duration::from_secs(1));
        p.end(t0 + MS(1100));
        assert!(p.done(t0 + MS(1200)));
    }

    #[test]
    fn finish_now_lands_at_rest_in_one_step() {
        let t0 = Instant::now();
        let mut p = Play::start(Story::Kelelawar, t0);
        p.finish_now();
        assert!(p.done(t0 + MS(1)));
        let b = p.beat(t0 + MS(1));
        assert_eq!((b.alpha, b.spread), (0.0, 0.0));
    }

    #[test]
    fn a_story_winds_down_by_itself_at_the_end_of_its_time() {
        let t0 = Instant::now();
        let p = Play::start(Story::MalingAngka, t0);
        let secs = Story::MalingAngka.secs();
        assert!(!p.done(t0 + Duration::from_secs_f32(secs - 0.1)));
        assert!(!p.done(t0 + Duration::from_secs_f32(secs + 0.1)));
        assert!(p.done(t0 + Duration::from_secs_f32(secs + 0.3)));
    }

    #[test]
    fn the_beat_opens_spreads_holds_and_closes() {
        let t0 = Instant::now();
        let p = Play::start(Story::MalingAngka, t0);
        let secs = Story::MalingAngka.secs();
        let at = |ms: u64| p.beat(t0 + MS(ms));
        assert_eq!(at(0).spread, 0.0);
        let mid = at(100).spread;
        assert!(mid > 0.05 && mid < 0.95, "easing in, got {mid}");
        assert_eq!(at(200).spread, 1.0);
        assert_eq!(at(4000).spread, 1.0);
        assert_eq!(at(4000).alpha, 1.0);
        assert!((at(4000).t - 4.0).abs() < 1e-3, "t is the story's own seconds");
        let close = (secs * 1000.0) as u64 + 100;
        let b = at(close);
        assert!(b.spread > 0.05 && b.spread < 0.95, "easing out, got {}", b.spread);
        assert!(b.alpha > 0.05 && b.alpha < 0.95, "alpha falls with it, got {}", b.alpha);
    }

    #[test]
    fn an_interruption_mid_story_eases_alpha_and_spread_down_and_time_keeps_going() {
        let t0 = Instant::now();
        let mut p = Play::start(Story::Bungee, t0);
        p.end(t0 + Duration::from_secs(3));
        let b0 = p.beat(t0 + Duration::from_secs(3));
        assert_eq!((b0.alpha, b0.spread), (1.0, 1.0), "starts from the full play");
        let b1 = p.beat(t0 + MS(3050));
        let b2 = p.beat(t0 + MS(3100));
        let b3 = p.beat(t0 + MS(3150));
        assert!(b0.alpha > b1.alpha && b1.alpha > b2.alpha && b2.alpha > b3.alpha);
        assert!(b3.alpha > 0.0);
        assert!(b0.spread > b1.spread && b1.spread > b2.spread && b2.spread > b3.spread);
        assert!(b0.t < b1.t && b1.t < b2.t && b2.t < b3.t, "painters never see time go back");
        assert!(b2.alpha > 0.3 && b2.alpha < 0.7, "halfway through is about half, got {}", b2.alpha);
    }

    #[test]
    fn an_interruption_during_the_opening_never_pops_the_spread_up() {
        let t0 = Instant::now();
        let mut p = Play::start(Story::Bungee, t0);
        let before = p.beat(t0 + MS(100)).spread;
        p.end(t0 + MS(100));
        let at_cut = p.beat(t0 + MS(100));
        assert_eq!(at_cut.spread, before, "no jump at the moment of the cut");
        assert_eq!(at_cut.alpha, 1.0);
        assert!(p.beat(t0 + MS(150)).spread < before);
        assert_eq!(p.beat(t0 + MS(300)).spread, 0.0);
    }

    #[test]
    fn a_play_that_never_started_the_clock_is_not_done_early() {
        let t0 = Instant::now();
        let p = Play::start(Story::PetakUmpet, t0);
        assert!(!p.done(t0));
        assert_eq!(p.story(), Story::PetakUmpet);
    }

    #[test]
    fn spread_moves_the_mascot_left_and_the_clock_right() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let rest = idle_layout_lead(s, true, true, 0.0);
        let out = spread_offsets(rest, s, 1.0);
        assert!(out.robot.unwrap().x < rest.robot.unwrap().x - 20.0);
        assert!(out.clock.unwrap().x > rest.clock.unwrap().x + 20.0);
        assert_eq!(spread_offsets(rest, s, 0.0), rest, "no spread, no move");
    }

    #[test]
    fn spread_pulls_the_two_apart_to_the_ends_at_every_island_size() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0), (160.0, 44.0), (300.0, 28.0)] {
                let s = shape(style, w, h);
                let rest = idle_layout_lead(s, true, true, 0.0);
                let out = spread_offsets(rest, s, 1.0);
                let (r, c) = (out.robot.unwrap(), out.clock.unwrap());
                let (r0, c0) = (rest.robot.unwrap(), rest.clock.unwrap());
                let gap_rest = c0.x - r0.right();
                let gap_out = c.x - r.right();
                assert!(gap_out > gap_rest + (w - 78.0) * 0.5, "{style:?} {w}x{h}: gap {gap_out} vs {gap_rest}");
                assert!(r.x >= s.x && r.x < s.x + 0.2 * w, "{style:?} {w}x{h}: robot at {}", r.x);
                assert!(c.right() <= s.x + s.w && c.right() > s.x + 0.8 * w, "{style:?} {w}x{h}: clock ends {}", c.right());
                assert_eq!((r.y, r.w, r.h), (r0.y, r0.w, r0.h), "the mascot only slides");
                assert_eq!((c.y, c.w, c.h), (c0.y, c0.w, c0.h), "the clock only slides");
            }
        }
    }

    #[test]
    fn half_way_is_between_the_rest_and_the_ends() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let rest = idle_layout_lead(s, true, true, 0.0);
        let full = spread_offsets(rest, s, 1.0);
        let half = spread_offsets(rest, s, 0.5);
        let (r0, r1, rh) = (rest.robot.unwrap().x, full.robot.unwrap().x, half.robot.unwrap().x);
        let (c0, c1, ch) = (rest.clock.unwrap().x, full.clock.unwrap().x, half.clock.unwrap().x);
        assert!((rh - (r0 + r1) / 2.0).abs() < 1e-3);
        assert!((ch - (c0 + c1) / 2.0).abs() < 1e-3);
    }

    #[test]
    fn spread_copes_with_a_missing_mascot_or_clock() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let only_robot = idle_layout_lead(s, true, false, 0.0);
        let out = spread_offsets(only_robot, s, 1.0);
        assert!(out.robot.unwrap().x < only_robot.robot.unwrap().x - 20.0);
        assert_eq!(out.clock, None);

        let only_clock = idle_layout_lead(s, false, true, 0.0);
        let out = spread_offsets(only_clock, s, 1.0);
        assert_eq!(out.robot, None);
        let c = out.clock.unwrap();
        assert!(c.right() <= s.x + s.w && c.right() > s.x + 0.8 * s.w, "clock goes to the right end, got {c:?}");
        assert!(c.x > only_clock.clock.unwrap().x + 20.0);
        assert_eq!(spread_offsets(only_clock, s, 0.0), only_clock);

        let none = idle_layout_lead(s, false, false, 0.0);
        assert_eq!(spread_offsets(none, s, 1.0), none);
    }
}
