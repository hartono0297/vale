//! The stories themselves: what each one does to the island, the clock and
//! the mascot at any moment of its timeline (`Shot`). Pure: the renderer
//! paints a shot, nothing here touches Direct2D.

pub mod maling_angka;

use std::f32::consts::{PI, TAU};

use crate::config::Style;
use crate::ui::geometry::{Rect, ShapeRect, shoulder_depth};
use crate::ui::mascot::CrestPose;
use crate::ui::play::shape::{Deform, outline};
use crate::ui::play::state::Beat;
use crate::ui::play::trigger::Story;
use crate::ui::render::DigitFx;
use crate::ui::robot::{Expr, Pose};

/// The story has a timeline to play. Only ready stories enter the pool
/// (`trigger::pool`), so a mascot with none never plays.
pub fn ready(story: Story) -> bool {
    matches!(story, Story::MalingAngka)
}

/// One moment of a story, for the renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub deform: Deform,
    /// One per character of the clock (`%H:%M`).
    pub digits: [DigitFx; 5],
    /// The mascot as the story moves it; `None` leaves it as the app draws it.
    pub mascot: Option<MascotShot>,
    /// Painted in front of everything, the island's edge included.
    pub props: Vec<Prop>,
    /// The deformed island's box, window dip. The mascot is painted inside
    /// the island's clip, so this box holds it too.
    pub island: Rect,
}

impl Shot {
    /// The island, the mascot and the clock exactly at rest.
    pub fn rest(style: Style, s: ShapeRect) -> Shot {
        let deform = Deform::default();
        Shot {
            island: island_box(style, s, &deform),
            deform,
            digits: [DigitFx::IDENTITY; 5],
            mascot: None,
            props: Vec::new(),
        }
    }

    /// Everything the shot paints, in window dip. Every story's tests check
    /// it stays inside the window.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bounds(&self) -> Rect {
        self.props.iter().map(Prop::bounds).fold(self.island, union)
    }
}

/// The head's pivot on its 64-unit grid: a `MascotShot` moves, turns and
/// scales the head about it.
pub const HEAD_PIVOT: (f32, f32) = (32.0, 40.0);

/// The mascot's head as a story moves it, from its resting place in the
/// slot: its pivot (grid (32, 40)) moved by `dx`, `dy` (dip), the head turned
/// by `rot_deg` (clockwise) and scaled by `sx`, `sy` (in its own axes) about
/// it. Painted by the mascot's own painter, so the coat and the accessory
/// come along unchanged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MascotShot {
    pub dx: f32,
    pub dy: f32,
    pub rot_deg: f32,
    pub sx: f32,
    pub sy: f32,
    pub expr: Expr,
    /// The falk's crest, in place of the face's own.
    pub crest: CrestPose,
    /// The face marks slide this much further, grid units.
    pub glance: (f32, f32),
    pub blink: bool,
    pub beak_open: bool,
}

impl MascotShot {
    /// `base` (the app's pose: its look, its lift) with the story's face on it.
    pub fn pose(&self, base: Pose) -> Pose {
        Pose {
            blink: self.blink,
            crest: Some(self.crest),
            beak_open: Some(self.beak_open),
            glance_x: base.glance_x + self.glance.0,
            glance_y: base.glance_y + self.glance.1,
            ..base
        }
    }
}

/// Something a story paints in front of everything, in window dip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prop {
    /// The clock's character `index`, its cell centred on `at`, turned by
    /// `rot_deg`, outlined in the island's colour so it reads over the
    /// mascot and off the island.
    Digit {
        index: usize,
        at: (f32, f32),
        rot_deg: f32,
        alpha: f32,
    },
    /// Three small whistle arcs, the first starting near `at`.
    Whistle { at: (f32, f32), alpha: f32 },
}

/// How far a digit prop's ink can reach from its centre, turned any way,
/// outline included: half the diagonal of a clock character's line box.
#[cfg_attr(not(test), allow(dead_code))]
pub const DIGIT_REACH: f32 = 13.0;

impl Prop {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bounds(&self) -> Rect {
        match *self {
            Prop::Digit { at, .. } => Rect {
                x: at.0 - DIGIT_REACH,
                y: at.1 - DIGIT_REACH,
                w: 2.0 * DIGIT_REACH,
                h: 2.0 * DIGIT_REACH,
            },
            // The arcs (`whistle_arcs`) with their stroke.
            Prop::Whistle { at, .. } => Rect {
                x: at.0 + 2.5,
                y: at.1 - 4.9,
                w: 8.0,
                h: 11.0,
            },
        }
    }
}

/// The three whistle arcs of a `Prop::Whistle` at `at`: each runs from its
/// start straight down by its chord, bowing right on a circle of its radius,
/// at its share of the prop's alpha. (start, chord, radius, alpha)
pub fn whistle_arcs(at: (f32, f32)) -> [((f32, f32), f32, f32, f32); 3] {
    std::array::from_fn(|i| {
        let i = i as f32;
        (
            (at.0 + 3.0 + i * 2.4, at.1 - 2.0 - i * 1.2),
            4.0 + i * 3.0,
            3.0 + i * 2.2,
            1.0 - i * 0.25,
        )
    })
}

/// `story`'s shot at `beat` on the island `s`. A story that is not ready
/// leaves everything at rest.
pub fn shot(story: Story, style: Style, s: ShapeRect, beat: Beat) -> Shot {
    match story {
        Story::MalingAngka => maling_angka::shot(style, s, beat),
        _ => Shot::rest(style, s),
    }
}

/// The deformed island's box: its outline, and the Notch's shoulders.
fn island_box(style: Style, s: ShapeRect, d: &Deform) -> Rect {
    let pts = outline(style, s, d);
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in &pts {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    if style == Style::Notch {
        let shoulder = shoulder_depth(s);
        (x0, x1, y0) = (x0 - shoulder, x1 + shoulder, y0.min(s.y));
    }
    Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn union(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect {
        x,
        y,
        w: (a.x + a.w).max(b.x + b.w) - x,
        h: (a.y + a.h).max(b.y + b.h) - y,
    }
}

// ---------- timeline helpers, as `docs/design/maskot-notch-v3.html` has them ----------

/// The easings the mockup's keyframes use.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ease {
    /// Cubic in and out (the mockup's default, `io`).
    Io,
    /// Cubic out (`o`).
    O,
    /// Sine in and out (`sio`).
    Sio,
    /// A soft overshoot (`sb`).
    Sb,
    Lin,
}

impl Ease {
    fn at(self, t: f32) -> f32 {
        match self {
            Ease::Io if t < 0.5 => 4.0 * t * t * t,
            Ease::Io => 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0,
            Ease::O => 1.0 - (1.0 - t).powi(3),
            Ease::Sio => -((PI * t).cos() - 1.0) / 2.0,
            Ease::Sb => {
                let (c1, c3) = (0.9, 1.9);
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
            Ease::Lin => t,
        }
    }
}

/// The value at `t` of keyframes (time, value, ease into it): held before
/// the first and after the last.
fn kf(t: f32, keys: &[(f32, f32, Ease)]) -> f32 {
    let Some(&(t0, v0, _)) = keys.first() else {
        return 0.0;
    };
    if t <= t0 {
        return v0;
    }
    for w in keys.windows(2) {
        let ((ta, va, _), (tb, vb, ease)) = (w[0], w[1]);
        if t <= tb {
            let u = ((t - ta) / (tb - ta).max(1e-6)).clamp(0.0, 1.0);
            return va + (vb - va) * ease.at(u);
        }
    }
    keys[keys.len() - 1].1
}

/// Half a sine from `t0`, `d` long, 0 outside it.
fn bump(t: f32, t0: f32, d: f32) -> f32 {
    if t > t0 && t < t0 + d {
        (PI * (t - t0) / d).sin()
    } else {
        0.0
    }
}

/// A damped ring from `t0`: `amp`, `freq` Hz, decaying at `damp` per second.
fn ring(t: f32, t0: f32, amp: f32, freq: f32, damp: f32) -> f32 {
    if t < t0 {
        return 0.0;
    }
    amp * (-damp * (t - t0)).exp() * (TAU * freq * (t - t0)).sin()
}

/// How shut the eyes are for blinks centred at `at`, each 0.16 s long.
fn blinks(t: f32, at: &[f32]) -> f32 {
    at.iter()
        .map(|m| 1.0 - (t - m).abs() / 0.08)
        .fold(0.0_f32, f32::max)
        .clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// `fx` eased toward the character at rest as `alpha` falls to 0.
fn toward_rest(fx: DigitFx, alpha: f32) -> DigitFx {
    let id = DigitFx::IDENTITY;
    DigitFx {
        dx: lerp(id.dx, fx.dx, alpha),
        dy: lerp(id.dy, fx.dy, alpha),
        rot_deg: lerp(id.rot_deg, fx.rot_deg, alpha),
        scale: lerp(id.scale, fx.scale, alpha),
        alpha: lerp(id.alpha, fx.alpha, alpha),
    }
}
