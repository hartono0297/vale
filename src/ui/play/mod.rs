//! Animations the mascot plays around the closed island.

// Used by the animations the next tasks add.
#[cfg_attr(not(test), allow(dead_code))]
pub mod gate;
#[cfg_attr(not(test), allow(dead_code))]
pub mod rim;
#[cfg_attr(not(test), allow(dead_code))]
pub mod shape;
#[cfg_attr(not(test), allow(dead_code))]
pub mod state;
pub mod stories;
#[cfg_attr(not(test), allow(dead_code))]
pub mod trigger;

use crate::config::Style;
use crate::ui::geometry::{ShapeRect, shape_contains};

/// The island takes a click at this logical point: it is inside the island's
/// own (resting) shape. While no play runs the alpha rule governs and the
/// answer is the same.
pub fn swallows(style: Style, s: ShapeRect, _playing: bool, x: f32, y: f32) -> bool {
    shape_contains(style, s, x, y)
}

/// The island window should pass clicks through its pixels: a play runs and
/// the cursor is not on the island itself.
pub fn click_through(playing: bool, inside: bool) -> bool {
    playing && !inside
}

#[cfg(test)]
pub(crate) mod testing {
    use crate::config::Style;
    use crate::ui::geometry::{NotchSize, ShapeKind, ShapeRect, shape_sized};
    use crate::ui::play::gate::Env;

    /// An environment where nothing blocks a play.
    pub(crate) fn ok_env() -> Env {
        Env {
            mascot_on: true,
            motion_full: true,
            energy_saver: false,
            win_anim: true,
            idle_view: true,
            panel_open: false,
            menu_open: false,
            fullscreen: false,
            screen_away: false,
            shrunk: false,
            pointer_near: false,
            asleep: false,
            recording: false,
            clock_on: true,
            has_story: true,
            working: false,
        }
    }

    /// The resting island at `w` x `h`: `y` is 0 for the Notch and the Pill's
    /// top gap for the Pill, with the radius the compact shapes use.
    pub(crate) fn shape(style: Style, w: f32, h: f32) -> ShapeRect {
        shape_sized(style, NotchSize { w, h }, ShapeKind::Idle)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::shape;
    use super::*;

    #[test]
    fn a_play_never_swallows_a_click_outside_the_island() {
        let s = shape(Style::Notch, 190.0, 34.0);
        assert!(swallows(Style::Notch, s, true, s.x + s.w / 2.0, s.y + 2.0), "the island itself still takes clicks");
        assert!(!swallows(Style::Notch, s, true, s.x + s.w / 2.0, s.y + s.h + 40.0), "a click under the island passes through");
        assert!(!swallows(Style::Notch, s, true, s.x - 20.0, s.y + 10.0), "beside it too");
        assert!(!swallows(Style::Notch, s, false, s.x - 60.0, s.y + 10.0));
    }

    #[test]
    fn the_window_is_click_through_only_during_a_play_with_the_cursor_outside() {
        assert!(click_through(true, false), "a play, cursor on an animation pixel: pass clicks through");
        assert!(!click_through(true, true), "a play, cursor on the island: the island takes the click");
        assert!(!click_through(false, false), "no play: never click-through, the alpha rule governs");
        assert!(!click_through(false, true));
    }
}
