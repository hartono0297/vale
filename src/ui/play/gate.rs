//! Every condition that lets a play start, and every one that stops it.

use std::time::Duration;

use crate::theme::MascotPlay;
use crate::ui::play::trigger::{Moment, Story, cooldown, presence_ok};

/// What the app sees right now. A flag is named for the thing that blocks, so
/// `fullscreen: true` means a window is fullscreen, except `mascot_on`,
/// `motion_full`, `win_anim`, `idle_view` and `clock_on`, where true is the
/// permitting side.
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
}

/// Whether the island is in a state where a play may be shown at all. Shared
/// by starting and stopping. An open panel is left to the callers: it stops
/// everything but a manual start.
fn clear(env: &Env) -> bool {
    env.mascot_on
        && env.motion_full
        && !env.energy_saver
        && env.win_anim
        && env.idle_view
        && !env.menu_open
        && !env.fullscreen
        && !env.screen_away
        && !env.shrunk
        && !env.pointer_near
        && !env.asleep
        && !env.recording
}

/// Whether a play may begin. `Manual` skips the presence rule, the cooldown and
/// the open-panel clause, nothing else.
pub fn may_start(
    env: &Env,
    moment: Moment,
    preset: MascotPlay,
    since_input: Duration,
    since_last: Duration,
) -> bool {
    if !clear(env) {
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
    allowed && !env.panel_open && presence_ok(since_input) && since_last >= cooldown(preset)
}

/// Whether a running play has to end. It winds down unless `instant_stop`.
pub fn must_stop(env: &Env, playing: Story, shape_changed: bool, clock_jumped: bool) -> bool {
    !clear(env)
        || env.panel_open
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
            fullscreen, screen_away, shrunk, pointer_near, asleep, recording, clock_on,
        )
        .into_iter()
        .filter(|(name, _)| *name != "clock_on")
        .collect()
    }

    #[test]
    fn the_flips_cover_every_gate_but_the_clock() {
        assert_eq!(off_by_one_envs().len(), 13);
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

    #[test]
    fn manual_ignores_presence_cooldown_and_an_open_panel() {
        let mut env = ok_env();
        env.panel_open = true;
        assert!(may_start(&env, Moment::Manual, MascotPlay::Mati, Duration::ZERO, Duration::ZERO));
        env.fullscreen = true;
        assert!(!may_start(&env, Moment::Manual, MascotPlay::Mati, Duration::ZERO, Duration::ZERO));
    }

    #[test]
    fn manual_is_stopped_by_every_gate_except_the_open_panel() {
        for (name, env) in off_by_one_envs() {
            assert_eq!(
                may_start(&env, Moment::Manual, MascotPlay::Sesekali, Duration::ZERO, Duration::ZERO),
                name == "panel_open",
                "{name}"
            );
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
        assert!(must_stop(&env, Story::MalingAngka, false, false), "the pointer entered the hover zone");
        assert!(must_stop(&ok_env(), Story::MalingAngka, true, false), "the size or style changed");
        assert!(must_stop(&ok_env(), Story::MalingAngka, false, true), "the clock jumped");
        let mut no_clock = ok_env();
        no_clock.clock_on = false;
        assert!(must_stop(&no_clock, Story::MalingAngka, false, false), "a clock story with the clock off");
        assert!(!must_stop(&no_clock, Story::Kelelawar, false, false), "a story that needs no clock carries on");
        assert!(!must_stop(&ok_env(), Story::MalingAngka, false, false), "nothing wrong");
    }

    #[test]
    fn every_gate_but_the_clock_stops_a_running_play() {
        for (name, env) in off_by_one_envs() {
            assert!(must_stop(&env, Story::Kelelawar, false, false), "{name} did not stop it");
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
