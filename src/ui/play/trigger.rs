//! When a story may play and which one.

use std::time::Duration;

use crate::theme::{Mascot, MascotPlay};
use crate::ui::play::stories::ready;

/// An automatic story only plays once the owner has been idle this long: any
/// sooner they are mid-keystroke.
pub const PRESENCE_MIN: Duration = Duration::from_secs(15);
/// Past this they have left the desk and nobody would see it.
pub const PRESENCE_MAX: Duration = Duration::from_secs(300);
/// How long the island takes to settle back when a story is cut short.
pub const WIND_DOWN: Duration = Duration::from_millis(200);

/// Why a story is being picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    Idle,
    Hour,
    Greeting,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Story {
    PetakUmpet,
    BantalAngka,
    PianoEkor,
    SyalEkor,
    PenasaranJam,
    PelukPulau,
    Bungee,
    Kelelawar,
    MalingAngka,
    TerbangKeliling,
}

impl Story {
    pub fn mascot(self) -> Mascot {
        match self {
            Self::PetakUmpet | Self::BantalAngka | Self::PianoEkor | Self::SyalEkor => Mascot::Cat,
            Self::PenasaranJam | Self::PelukPulau | Self::Bungee => Mascot::Octopus,
            Self::Kelelawar | Self::MalingAngka | Self::TerbangKeliling => Mascot::Falk,
        }
    }

    /// The story plays with the clock digits, so it is skipped when the clock
    /// is off.
    pub fn needs_clock(self) -> bool {
        matches!(
            self,
            Self::BantalAngka | Self::PianoEkor | Self::PenasaranJam | Self::MalingAngka
        )
    }

    pub fn secs(self) -> f32 {
        match self {
            Self::PetakUmpet => 7.8,
            Self::BantalAngka => 8.6,
            Self::PianoEkor | Self::SyalEkor => 8.4,
            Self::PenasaranJam => 7.6,
            Self::PelukPulau => 7.8,
            Self::Bungee => 7.4,
            Self::Kelelawar => 7.6,
            Self::MalingAngka => 7.8,
            Self::TerbangKeliling => 7.9,
        }
    }

    /// The story preferred for a greeting.
    fn warm(mascot: Mascot) -> Self {
        match mascot {
            Mascot::Cat => Self::SyalEkor,
            Mascot::Octopus => Self::PelukPulau,
            Mascot::Falk => Self::TerbangKeliling,
        }
    }
}

const ALL: [Story; 10] = [
    Story::PetakUmpet,
    Story::BantalAngka,
    Story::PianoEkor,
    Story::SyalEkor,
    Story::PenasaranJam,
    Story::PelukPulau,
    Story::Bungee,
    Story::Kelelawar,
    Story::MalingAngka,
    Story::TerbangKeliling,
];

pub fn presence_ok(since_input: Duration) -> bool {
    (PRESENCE_MIN..=PRESENCE_MAX).contains(&since_input)
}

/// `story` is the mascot's, and not a clock one while the clock is off.
fn owns(story: Story, mascot: Mascot, clock_on: bool) -> bool {
    story.mascot() == mascot && (clock_on || !story.needs_clock())
}

/// `story` is the mascot's to play now: it owns it and it is ready
/// (`stories::ready`).
fn playable(story: Story, mascot: Mascot, clock_on: bool) -> bool {
    owns(story, mascot, clock_on) && ready(story)
}

/// Every story the mascot owns, ready or not, without the clock ones when
/// the clock is off.
fn owned(mascot: Mascot, clock_on: bool) -> Vec<Story> {
    ALL.into_iter().filter(|s| owns(*s, mascot, clock_on)).collect()
}

/// The stories the mascot can play. Empty when none is ready, and then it
/// never plays (`gate::Env::has_story`).
pub fn pool(mascot: Mascot, clock_on: bool) -> Vec<Story> {
    ALL.into_iter().filter(|s| playable(*s, mascot, clock_on)).collect()
}

/// `pool` is not empty, without building it (read on every frame of a play).
pub fn has_ready(mascot: Mascot, clock_on: bool) -> bool {
    ALL.into_iter().any(|s| playable(s, mascot, clock_on))
}

/// Never returns `last`, unless it is the pool's only story. `roll` is a
/// random number from the caller, so the choice is deterministic in it. The
/// pool is not empty: the gates keep a mascot without one from starting.
pub fn pick(
    mascot: Mascot,
    clock_on: bool,
    last: Option<Story>,
    moment: Moment,
    roll: u32,
) -> Story {
    pick_from(&pool(mascot, clock_on), mascot, last, moment, roll)
}

/// `pick` from `pool`, the stories of `mascot`.
fn pick_from(pool: &[Story], mascot: Mascot, last: Option<Story>, moment: Moment, roll: u32) -> Story {
    let mut fresh: Vec<Story> = pool.iter().copied().filter(|s| Some(*s) != last).collect();
    // A one-story pool plays it again rather than nothing.
    if fresh.is_empty() {
        fresh = pool.to_vec();
    }
    if moment == Moment::Greeting {
        let warm = Story::warm(mascot);
        if fresh.contains(&warm) {
            return warm;
        }
    }
    if moment == Moment::Hour {
        let clock: Vec<Story> = fresh.iter().copied().filter(|s| s.needs_clock()).collect();
        if !clock.is_empty() {
            return clock[roll as usize % clock.len()];
        }
    }
    fresh[roll as usize % fresh.len()]
}

/// How long to wait before an idle story, `None` when the preset has no timer.
pub fn wait_for(preset: MascotPlay, roll: u32) -> Option<Duration> {
    let (lo, hi) = match preset {
        MascotPlay::Sering => (8, 15),
        MascotPlay::Sesekali => (15, 35),
        MascotPlay::Jarang => (45, 90),
        MascotPlay::Sapaan | MascotPlay::Mati => return None,
    };
    let (lo, hi) = (lo * 60, hi * 60);
    Some(Duration::from_secs(lo + u64::from(roll) % (hi - lo + 1)))
}

/// The least time between two automatic stories.
pub fn cooldown(preset: MascotPlay) -> Duration {
    let mins = match preset {
        MascotPlay::Sering => 6,
        MascotPlay::Sesekali | MascotPlay::Sapaan => 12,
        MascotPlay::Jarang => 40,
        MascotPlay::Mati => 0,
    };
    Duration::from_secs(mins * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_presence_window_is_fifteen_seconds_to_five_minutes() {
        assert!(!presence_ok(Duration::from_secs(14)));
        assert!(presence_ok(Duration::from_secs(15)));
        assert!(presence_ok(Duration::from_secs(299)));
        assert!(presence_ok(Duration::from_secs(300)));
        assert!(!presence_ok(Duration::from_millis(300_001)));
        assert!(!presence_ok(Duration::from_secs(301)));
    }

    #[test]
    fn every_mascot_owns_at_least_two_stories_without_the_clock() {
        // By design, once every story is ready (Part 2).
        for m in [Mascot::Cat, Mascot::Falk, Mascot::Octopus] {
            assert!(owned(m, true).len() >= 3, "{m:?}");
            assert!(owned(m, false).len() >= 2, "{m:?} without the clock");
            assert!(owned(m, false).iter().all(|s| !s.needs_clock()), "{m:?}");
        }
    }

    #[test]
    fn only_ready_stories_are_in_the_pool() {
        for m in [Mascot::Cat, Mascot::Falk, Mascot::Octopus] {
            for clock_on in [true, false] {
                let want: Vec<Story> = owned(m, clock_on).into_iter().filter(|s| ready(*s)).collect();
                assert_eq!(pool(m, clock_on), want, "{m:?} clock_on={clock_on}");
            }
        }
        // Part 1 ships the falk's Maling angka alone.
        assert_eq!(pool(Mascot::Falk, true), vec![Story::MalingAngka]);
        assert!(pool(Mascot::Falk, false).is_empty(), "Maling angka needs the clock");
        assert!(pool(Mascot::Cat, true).is_empty(), "no cat story yet");
        assert!(pool(Mascot::Octopus, true).is_empty(), "no octopus story yet");
    }

    #[test]
    fn has_ready_says_whether_the_pool_has_a_story() {
        for m in [Mascot::Cat, Mascot::Falk, Mascot::Octopus] {
            for clock_on in [true, false] {
                assert_eq!(has_ready(m, clock_on), !pool(m, clock_on).is_empty(), "{m:?} clock_on={clock_on}");
            }
        }
        assert!(has_ready(Mascot::Falk, true));
    }

    #[test]
    fn a_one_story_pool_plays_its_story_again() {
        for moment in [Moment::Idle, Moment::Hour, Moment::Greeting, Moment::Manual] {
            for roll in 0..20 {
                let s = pick(Mascot::Falk, true, Some(Story::MalingAngka), moment, roll);
                assert_eq!(s, Story::MalingAngka, "{moment:?} roll={roll}");
            }
        }
    }

    #[test]
    fn the_clock_stories_are_exactly_the_four_with_digits() {
        let all = [
            Story::PetakUmpet,
            Story::BantalAngka,
            Story::PianoEkor,
            Story::SyalEkor,
            Story::PenasaranJam,
            Story::PelukPulau,
            Story::Bungee,
            Story::Kelelawar,
            Story::MalingAngka,
            Story::TerbangKeliling,
        ];
        let clock: Vec<Story> = all.into_iter().filter(|s| s.needs_clock()).collect();
        assert_eq!(
            clock,
            [
                Story::BantalAngka,
                Story::PianoEkor,
                Story::PenasaranJam,
                Story::MalingAngka
            ]
        );
    }

    #[test]
    fn a_pick_never_repeats_the_last_story() {
        for m in [Mascot::Cat, Mascot::Falk, Mascot::Octopus] {
            for clock_on in [true, false] {
                let p = owned(m, clock_on);
                for &last in &p {
                    for moment in [Moment::Idle, Moment::Hour, Moment::Greeting, Moment::Manual] {
                        for roll in 0..50 {
                            assert_ne!(
                                pick_from(&p, m, Some(last), moment, roll),
                                last,
                                "{m:?} {moment:?} roll={roll}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_pick_reaches_every_other_story_in_the_pool() {
        for m in [Mascot::Cat, Mascot::Falk, Mascot::Octopus] {
            let p = owned(m, true);
            let last = Some(p[0]);
            let seen: std::collections::HashSet<Story> = (0..50)
                .map(|r| pick_from(&p, m, last, Moment::Idle, r))
                .collect();
            assert_eq!(seen.len(), p.len() - 1, "{m:?}");
        }
    }

    #[test]
    fn the_hour_prefers_a_clock_story_and_the_greeting_a_warm_one() {
        let pick = |m: Mascot, clock_on: bool, last: Option<Story>, moment: Moment, roll: u32| {
            pick_from(&owned(m, clock_on), m, last, moment, roll)
        };
        for roll in 0..20 {
            assert!(pick(Mascot::Cat, true, None, Moment::Hour, roll).needs_clock());
            assert_eq!(
                pick(Mascot::Octopus, true, None, Moment::Greeting, roll),
                Story::PelukPulau
            );
            assert_eq!(
                pick(Mascot::Cat, true, None, Moment::Greeting, roll),
                Story::SyalEkor
            );
            assert_eq!(
                pick(Mascot::Falk, false, None, Moment::Greeting, roll),
                Story::TerbangKeliling
            );
        }
        assert!(
            !pick(Mascot::Cat, false, None, Moment::Hour, 0).needs_clock(),
            "no clock, no clock story"
        );
    }

    #[test]
    fn the_hour_still_avoids_a_repeat_when_one_clock_story_is_left() {
        // The octopus has one clock story: it was just played, so the hour
        // falls back to the ordinary pick instead of repeating it.
        let s = pick_from(
            &owned(Mascot::Octopus, true),
            Mascot::Octopus,
            Some(Story::PenasaranJam),
            Moment::Hour,
            0,
        );
        assert_ne!(s, Story::PenasaranJam);
        assert_eq!(s.mascot(), Mascot::Octopus);
    }

    #[test]
    fn the_presets_carry_the_specs_waits_and_cooldowns() {
        assert_eq!(wait_for(MascotPlay::Mati, 0), None);
        assert_eq!(wait_for(MascotPlay::Sapaan, 0), None);
        for (p, lo, hi) in [
            (MascotPlay::Sering, 8, 15),
            (MascotPlay::Sesekali, 15, 35),
            (MascotPlay::Jarang, 45, 90),
        ] {
            let waits: Vec<u64> = (0..3000)
                .map(|r| wait_for(p, r).unwrap().as_secs())
                .collect();
            assert!(
                waits.iter().all(|w| (lo * 60..=hi * 60).contains(w)),
                "{p:?}"
            );
            assert_eq!(
                *waits.iter().min().unwrap(),
                lo * 60,
                "{p:?} reaches the low end"
            );
            assert_eq!(
                *waits.iter().max().unwrap(),
                hi * 60,
                "{p:?} reaches the high end"
            );
        }
        for (p, mins) in [
            (MascotPlay::Sering, 6),
            (MascotPlay::Sesekali, 12),
            (MascotPlay::Jarang, 40),
            (MascotPlay::Sapaan, 12),
        ] {
            assert_eq!(cooldown(p), Duration::from_secs(mins * 60), "{p:?}");
        }
    }

    #[test]
    fn the_wind_down_is_two_hundred_milliseconds() {
        assert_eq!(WIND_DOWN, Duration::from_millis(200));
    }

    #[test]
    fn each_story_lasts_as_long_as_the_mockup() {
        let secs: Vec<f32> = ALL.iter().map(|s| s.secs()).collect();
        assert_eq!(secs, [7.8, 8.6, 8.4, 8.4, 7.6, 7.8, 7.4, 7.6, 7.8, 7.9]);
    }

    #[test]
    fn each_mascot_owns_exactly_its_stories() {
        assert_eq!(
            owned(Mascot::Cat, true),
            vec![
                Story::PetakUmpet,
                Story::BantalAngka,
                Story::PianoEkor,
                Story::SyalEkor
            ]
        );
        assert_eq!(
            owned(Mascot::Octopus, true),
            vec![Story::PenasaranJam, Story::PelukPulau, Story::Bungee]
        );
        assert_eq!(
            owned(Mascot::Falk, true),
            vec![Story::Kelelawar, Story::MalingAngka, Story::TerbangKeliling]
        );
    }

    #[test]
    fn all_holds_every_story_once() {
        // An exhaustive match with no wildcard: adding a variant to `Story`
        // stops this compiling until it is added here and to `ALL` too.
        fn index(s: Story) -> usize {
            match s {
                Story::PetakUmpet => 0,
                Story::BantalAngka => 1,
                Story::PianoEkor => 2,
                Story::SyalEkor => 3,
                Story::PenasaranJam => 4,
                Story::PelukPulau => 5,
                Story::Bungee => 6,
                Story::Kelelawar => 7,
                Story::MalingAngka => 8,
                Story::TerbangKeliling => 9,
            }
        }
        let mut count = [0; 10];
        for s in ALL {
            count[index(s)] += 1;
        }
        assert_eq!(count, [1; 10], "each story must appear in ALL exactly once");
    }
}
