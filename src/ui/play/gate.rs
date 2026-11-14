//! Every condition that lets a play start, and every one that stops it.

use std::time::Duration;

use crate::theme::MascotPlay;
use crate::ui::play::trigger::{Moment, Story, cooldown, presence_ok};

/// What the app sees right now. A flag is named for the thing that blocks, so
/// `fullscreen: true` means a window is fullscreen, except `mascot_on`,
/// `motion_full`, `win_anim`, `idle_view`, `clock_on` and `has_story`, where
/// true is the permitting side.
#[derive(Clone, Copy, Debug, Default)]
pub struct Env {
    pub mascot_on: bool,
    pub motion_full: bool,
    pub energy_saver: bool,
    pub win_anim: bool,
    pub idle_view: bool,
    pub panel_open: bool,
    pub menu_open: bool,
    pub fullscreen: bool,
    pub screen_away: bool,
    pub shrunk: bool,
    pub pointer_near: bool,
    pub asleep: bool,
    pub recording: bool,
    pub clock_on: bool,
    /// The mascot has a ready story to play (`trigger::pool` is not empty).
    pub has_story: bool,
}

/// The gates that bind every play, a `Manual` one included.
fn permanent_ok(env: &Env) -> bool {
    env.mascot_on
        && env.motion_full
        && !env.energy_saver
        && env.win_anim
        && env.idle_view
        && !env.panel_open
        && !env.fullscreen
        && !env.screen_away
        && !env.asleep
        && !env.recording
        && env.has_story
}

/// The gates that asking for a play by hand switches on (reaching the island to
/// right-click it), so a `Manual` play skips them.
fn transient_ok(env: &Env) -> bool {
    !env.pointer_near && !env.shrunk && !env.menu_open
}

/// Whether a play may begin. `Manual` skips the presence rule, the cooldown and
/// the transient gates, nothing else. `Greeting` skips the presence rule only.
pub fn may_start(
    env: &Env,
    moment: Moment,
    preset: MascotPlay,
    since_input: Duration,
    since_last: Duration,
) -> bool {
    if !permanent_ok(env) {
        return false;
    }
    if moment == Moment::Manual {
        return true;
    }
    let allowed = match preset {
        MascotPlay::Mati => false,
        MascotPlay::Sapaan => matches!(moment, Moment::Greeting | Moment::Hour),
        _ => true,
    };
    // A greeting fires just after the owner touched the PC: it stands in for
    // the presence rule. The cooldown and every gate still bind it.
    let present = moment == Moment::Greeting || presence_ok(since_input);
    allowed && transient_ok(env) && present && since_last >= cooldown(preset)
}

/// Whether a running play has to end. It winds down unless `instant_stop`.
pub fn must_stop(
    env: &Env,
    playing: Story,
    moment: Moment,
    shape_changed: bool,
    clock_jumped: bool,
) -> bool {
    !permanent_ok(env)
        || (moment != Moment::Manual && !transient_ok(env))
        || shape_changed
        || clock_jumped
        || (playing.needs_clock() && !env.clock_on)
}

/// No point easing home: the motion itself is not wanted.
pub fn instant_stop(env: &Env) -> bool {
    !env.motion_full || env.energy_saver || !env.win_anim
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::play::testing::ok_env;

    /// One env per field, with that field flipped from `ok_env`. The field list
    /// is written once: the macro builds both the flips and an exhaustive
    /// destructure of `Env`, so a field added to `Env` and not listed here
    /// does not compile. `clock_on` is left out of the result because it picks
    /// the story and never blocks the start.
    fn off_by_one_envs() -> Vec<(&'static str, Env)> {
        macro_rules! flips {
            ($($f:ident),+ $(,)?) => {{
                let Env { $($f: _),+ } = ok_env();
                vec![$((stringify!($f), { let mut e = ok_env(); e.$f = !e.$f; e })),+]
            }};
        }
        flips!(
            mascot_on, motion_full, energy_saver, win_anim, idle_view, panel_open, menu_open,
            fullscreen, screen_away, shrunk, pointer_near, asleep, recording, clock_on, has_story,
        )
        .into_iter()
        .filter(|(name, _)| *name != "clock_on")
        .collect()
    }

    #[test]
    fn the_flips_cover_every_gate_but_the_clock() {
        assert_eq!(off_by_one_envs().len(), 14);
    }

    #[test]
    fn a_mascot_without_a_ready_story_never_starts() {
        let mut env = ok_env();
        env.has_story = false;
        for moment in [Moment::Idle, Moment::Hour, Moment::Greeting, Moment::Manual] {
            for p in [MascotPlay::Sering, MascotPlay::Sesekali, MascotPlay::Jarang, MascotPlay::Sapaan, MascotPlay::Mati] {
                assert!(!may_start(&env, moment, p, Duration::from_secs(60), Duration::from_secs(3600)), "{moment:?} {p:?}");
            }
        }
    }

    #[test]
    fn every_gate_can_stop_an_automatic_play() {
        let base = (ok_env(), Moment::Idle, MascotPlay::Sesekali, Duration::from_secs(60), Duration::from_secs(3600));
        assert!(may_start(&base.0, base.1, base.2, base.3, base.4));
        for (name, env) in off_by_one_envs() {
            assert!(!may_start(&env, base.1, base.2, base.3, base.4), "{name} did not stop it");
        }
    }

    #[test]
    fn the_clock_being_off_does_not_stop_a_play_from_starting() {
        let mut env = ok_env();
        env.clock_on = false;
        assert!(may_start(&env, Moment::Idle, MascotPlay::Sesekali, Duration::from_secs(60), Duration::from_secs(3600)));
    }

    const TRANSIENT: [&str; 3] = ["pointer_near", "shrunk", "menu_open"];

    #[test]
    fn manual_ignores_presence_cooldown_and_the_gates_that_asking_switches_on() {
        let mut env = ok_env();
        env.pointer_near = true;
        env.shrunk = true;
        env.menu_open = true;
        assert!(may_start(&env, Moment::Manual, MascotPlay::Mati, Duration::ZERO, Duration::ZERO));
        env.fullscreen = true;
        assert!(!may_start(&env, Moment::Manual, MascotPlay::Mati, Duration::ZERO, Duration::ZERO));
        let mut panel = ok_env();
        panel.panel_open = true;
        assert!(!may_start(&panel, Moment::Manual, MascotPlay::Mati, Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn manual_is_stopped_by_every_permanent_gate_and_only_those() {
        for (name, env) in off_by_one_envs() {
            assert_eq!(
                may_start(&env, Moment::Manual, MascotPlay::Sesekali, Duration::ZERO, Duration::ZERO),
                TRANSIENT.contains(&name),
                "{name}"
            );
        }
    }

    #[test]
    fn an_env_that_may_start_a_manual_play_does_not_stop_it_on_the_first_check() {
        let mut env = ok_env();
        env.pointer_near = true;
        env.shrunk = true;
        env.menu_open = true;
        assert!(may_start(&env, Moment::Manual, MascotPlay::Mati, Duration::ZERO, Duration::ZERO));
        assert!(
            !must_stop(&env, Story::MalingAngka, Moment::Manual, false, false),
            "may_start and must_stop disagree about the same env"
        );
        assert!(must_stop(&env, Story::MalingAngka, Moment::Idle, false, false));
    }

    #[test]
    fn for_a_manual_play_must_stop_agrees_with_may_start_on_every_gate() {
        for (name, env) in off_by_one_envs() {
            let start = may_start(&env, Moment::Manual, MascotPlay::Sesekali, Duration::ZERO, Duration::ZERO);
            assert_eq!(must_stop(&env, Story::Kelelawar, Moment::Manual, false, false), !start, "{name}");
        }
    }

    #[test]
    fn an_automatic_play_needs_the_owner_present_and_the_cooldown_over() {
        let env = ok_env();
        let p = MascotPlay::Sesekali;
        let ok = (Duration::from_secs(60), Duration::from_secs(12 * 60));
        assert!(may_start(&env, Moment::Idle, p, ok.0, ok.1));
        assert!(!may_start(&env, Moment::Idle, p, Duration::from_secs(5), ok.1), "mid-keystroke");
        assert!(!may_start(&env, Moment::Idle, p, Duration::from_secs(600), ok.1), "away");
        assert!(!may_start(&env, Moment::Idle, p, ok.0, ok.1 - Duration::from_secs(1)), "cooling down");
    }

    #[test]
    fn a_greeting_stands_in_for_presence_but_not_for_the_cooldown() {
        let p = MascotPlay::Sesekali;
        let cooled = Duration::from_secs(12 * 60);
        for since_input in [Duration::ZERO, Duration::from_secs(5), Duration::from_secs(600)] {
            assert!(may_start(&ok_env(), Moment::Greeting, p, since_input, cooled), "{since_input:?}");
            assert!(!may_start(&ok_env(), Moment::Idle, p, since_input, cooled), "{since_input:?}");
        }
        assert!(!may_start(&ok_env(), Moment::Greeting, p, Duration::ZERO, cooled - Duration::from_secs(1)));
        for (name, env) in off_by_one_envs() {
            assert!(!may_start(&env, Moment::Greeting, p, Duration::ZERO, cooled), "{name}");
        }
    }

    #[test]
    fn mati_allows_no_automatic_moment() {
        for moment in [Moment::Idle, Moment::Hour, Moment::Greeting] {
            assert!(!may_start(&ok_env(), moment, MascotPlay::Mati, Duration::from_secs(60), Duration::from_secs(3600)));
        }
    }

    #[test]
    fn sapaan_allows_only_the_greeting_and_the_hour() {
        let e = (ok_env(), Duration::from_secs(60), Duration::from_secs(3600));
        assert!(!may_start(&e.0, Moment::Idle, MascotPlay::Sapaan, e.1, e.2));
        assert!(may_start(&e.0, Moment::Greeting, MascotPlay::Sapaan, e.1, e.2));
        assert!(may_start(&e.0, Moment::Hour, MascotPlay::Sapaan, e.1, e.2));
    }

    #[test]
    fn the_timed_presets_allow_every_automatic_moment() {
        for p in [MascotPlay::Sering, MascotPlay::Sesekali, MascotPlay::Jarang] {
            for m in [Moment::Idle, Moment::Hour, Moment::Greeting] {
                assert!(may_start(&ok_env(), m, p, Duration::from_secs(60), Duration::from_secs(3600)), "{p:?} {m:?}");
            }
        }
    }

    #[test]
    fn a_running_play_stops_on_use_a_shape_change_or_a_clock_jump() {
        let mut env = ok_env();
        env.pointer_near = true;
        assert!(must_stop(&env, Story::MalingAngka, Moment::Idle, false, false), "the pointer entered the hover zone");
        assert!(must_stop(&ok_env(), Story::MalingAngka, Moment::Idle, true, false), "the size or style changed");
        assert!(must_stop(&ok_env(), Story::MalingAngka, Moment::Idle, false, true), "the clock jumped");
        let mut no_clock = ok_env();
        no_clock.clock_on = false;
        assert!(must_stop(&no_clock, Story::MalingAngka, Moment::Idle, false, false), "a clock story with the clock off");
        assert!(!must_stop(&no_clock, Story::Kelelawar, Moment::Idle, false, false), "a story that needs no clock carries on");
        assert!(!must_stop(&ok_env(), Story::MalingAngka, Moment::Idle, false, false), "nothing wrong");
    }

    #[test]
    fn every_gate_but_the_clock_stops_a_running_play() {
        for (name, env) in off_by_one_envs() {
            assert!(must_stop(&env, Story::Kelelawar, Moment::Idle, false, false), "{name} did not stop it");
        }
    }

    #[test]
    fn energy_saver_or_windows_animations_off_stops_a_play_instantly() {
        let mut env = ok_env();
        env.energy_saver = true;
        assert!(instant_stop(&env));
        let mut env = ok_env();
        env.win_anim = false;
        assert!(instant_stop(&env));
        let mut env = ok_env();
        env.motion_full = false;
        assert!(instant_stop(&env));
        assert!(!instant_stop(&ok_env()));
    }

    #[test]
    fn only_those_three_stop_instantly() {
        let instant = ["motion_full", "energy_saver", "win_anim"];
        for (name, env) in off_by_one_envs() {
            assert_eq!(instant_stop(&env), instant.contains(&name), "{name}");
        }
    }
}
