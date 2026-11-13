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
#[cfg_attr(not(test), allow(dead_code))]
pub mod trigger;

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
        }
    }

    /// The resting island at `w` x `h`: `y` is 0 for the Notch and the Pill's
    /// top gap for the Pill, with the radius the compact shapes use.
    pub(crate) fn shape(style: Style, w: f32, h: f32) -> ShapeRect {
        shape_sized(style, NotchSize { w, h }, ShapeKind::Idle)
    }
}
