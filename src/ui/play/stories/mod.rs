//! The stories themselves: what each one does to the island, the clock and
//! the mascot at any moment of its timeline (`Shot`). Pure: the renderer
//! paints a shot, nothing here touches Direct2D.

pub mod maling_angka;

use std::f32::consts::{PI, TAU};

use windows_numerics::{Matrix3x2, Vector2};

use crate::config::Style;
use crate::ui::geometry::{ROBOT_COMPACT, Rect, ShapeRect, shoulder_depth};
use crate::ui::mascot::CrestPose;
use crate::ui::play::shape::{Deform, outline, turn_about};
use crate::ui::play::state::Beat;
use crate::ui::play::trigger::Story;
use crate::ui::render::DigitFx;
use crate::ui::robot::{Expr, GRID, Pose, Seg};

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
    /// The mascot in its slot as the story moves it, painted in `Layer::Dark`
    /// (clipped to the island); `None` leaves it as the app draws it.
    pub mascot: Option<MascotShot>,
    /// Everything else the story draws, each part in its layer; within a
    /// layer in this order.
    pub parts: Vec<Part>,
    /// The deformed island's box, window dip. The slot mascot is painted
    /// inside the island's clip, so this box holds it too.
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
            parts: Vec::new(),
        }
    }

    /// Everything the shot paints, in window dip. Every story's tests check
    /// it stays inside the window.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bounds(&self) -> Rect {
        self.parts.iter().map(|p| p.draw.bounds()).fold(self.island, union)
    }
}

/// Where a part is painted, as the mockup's `compose` stacks it (see
/// `PAINT_ORDER`). Nothing leans on the island's fill being opaque: what
/// must not show is clipped away.
// Only Maling angka's front parts are built yet; the next stories build the rest.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    /// Behind the island: clipped to the window without the played island
    /// (even-odd), painted before the island's fill.
    Back,
    /// In the island's dark: clipped to the island, over the slot mascot.
    Dark,
    /// Unclipped, under the clock: a digit's ink wins where they overlap.
    Inside,
    /// On the island's face, over the clock: clipped to the island, and to
    /// the rect (window dip) when one is given.
    Wrap(Option<Rect>),
    /// In front of everything, unclipped.
    Front,
}

/// One step of painting a played island: the story's layers and the
/// island's own two steps, its fill and its clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pass {
    Back,
    Fill,
    Dark,
    Clock,
    Inside,
    Wrap,
    Front,
}

/// The order the renderer paints a played island in, bottom first: the
/// mockup's `compose`.
pub const PAINT_ORDER: [Pass; 7] = [
    Pass::Back,
    Pass::Fill,
    Pass::Dark,
    Pass::Inside,
    Pass::Clock,
    Pass::Wrap,
    Pass::Front,
];

impl Layer {
    /// The layer's step in `PAINT_ORDER`.
    pub fn pass(self) -> Pass {
        match self {
            Layer::Back => Pass::Back,
            Layer::Dark => Pass::Dark,
            Layer::Inside => Pass::Inside,
            Layer::Wrap(_) => Pass::Wrap,
            Layer::Front => Pass::Front,
        }
    }
}

/// One thing a story draws: `draw` in `layer`, at `alpha`.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub layer: Layer,
    pub draw: Draw,
    pub alpha: f32,
}

/// What a part draws, in window dip, or on the grid of the `Group` it is in.
/// Colours are `0xRRGGBB`.
// Only Maling angka's front parts are built yet; the next stories build the rest.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    /// The filled figures of `path` (each `Move` starts one), nonzero as the
    /// mockup's SVG fills: figures drawn the same way round fill where they
    /// overlap.
    Fill { path: Vec<Seg>, rgb: u32 },
    /// `path`'s lines, `width` wide with round caps. A ring is a stroke of
    /// `circle_segs`.
    Stroke { path: Vec<Seg>, rgb: u32, width: f32 },
    /// A filled ellipse about `c` with radii `r`, its axes on the grid's; a
    /// turned ellipse goes inside a turned `Group`.
    Ellipse { c: (f32, f32), r: (f32, f32), rgb: u32 },
    /// The mascot's head anywhere, by the same painter as the slot head, so
    /// its coat and accessory come along. Drawn at the slot head's size
    /// (`ROBOT_COMPACT`; inside a `Group`, the grid's 64 units, the group's
    /// scale sizing it) scaled by `shot.sx`, `shot.sy` and turned by
    /// `shot.rot_deg`, with its pivot (`HEAD_PIVOT`) at `at`. `shot.dx`,
    /// `shot.dy` are ignored here: they move the slot head from its slot.
    Head { at: (f32, f32), shot: MascotShot },
    /// The clock's character `index`, its cell centred on `at`, turned by
    /// `rot_deg`, outlined in the island's colour so it reads over the
    /// mascot and off the island.
    Digit { index: usize, at: (f32, f32), rot_deg: f32 },
    /// Three small whistle arcs, the first starting near `at`.
    Whistle { at: (f32, f32) },
    /// `items` on a grid of their own: its point `pivot` placed at `at`, the
    /// grid scaled by `scale` (in its own axes) and turned by `rot_deg`
    /// (clockwise) about it. A whole figure moves in one transform. Its items
    /// have no alpha of their own: the part's fades the figure as one, and a
    /// piece that fades alone is a `Part` of its own.
    Group {
        at: (f32, f32),
        rot_deg: f32,
        scale: (f32, f32),
        pivot: (f32, f32),
        items: Vec<Draw>,
    },
}

/// The head's ink on its 64-unit grid, whichever mascot, face, crest (up to
/// the startle's 1.75) and accessory: (left, top, right, bottom). Measured
/// -6.2, -15.4 (the falk's startled crest), 70.2 (the octopus's arms), 60.9,
/// plus 3 units for the pixels the measure rounds to.
const HEAD_INK: (f32, f32, f32, f32) = (-9.0, -19.0, 73.0, 64.0);

impl Draw {
    /// The box the part's ink stays in, window dip.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bounds(&self) -> Rect {
        self.bounds_at(ROBOT_COMPACT / GRID)
    }

    /// `bounds`, a head being `unit` per grid unit.
    #[cfg_attr(not(test), allow(dead_code))]
    fn bounds_at(&self, unit: f32) -> Rect {
        match self {
            Draw::Fill { path, .. } => path_box(path, 0.0),
            Draw::Stroke { path, width, .. } => path_box(path, width / 2.0),
            Draw::Ellipse { c, r, .. } => Rect {
                x: c.0 - r.0,
                y: c.1 - r.1,
                w: 2.0 * r.0,
                h: 2.0 * r.1,
            },
            Draw::Head { at, shot } => {
                let (l, t, r, b) = HEAD_INK;
                let place = |x: f32, y: f32| {
                    (at.0 + (x - HEAD_PIVOT.0) * unit, at.1 + (y - HEAD_PIVOT.1) * unit)
                };
                let (x0, y0) = place(l, t);
                let (x1, y1) = place(r, b);
                let ink = Rect {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                };
                moved(&head_matrix(*at, shot), ink)
            }
            Draw::Digit { at, .. } => Rect {
                x: at.0 - DIGIT_REACH,
                y: at.1 - DIGIT_REACH,
                w: 2.0 * DIGIT_REACH,
                h: 2.0 * DIGIT_REACH,
            },
            // The arcs (`whistle_arcs`) with their stroke.
            Draw::Whistle { at } => Rect {
                x: at.0 + 2.5,
                y: at.1 - 4.9,
                w: 8.0,
                h: 11.0,
            },
            Draw::Group {
                at,
                rot_deg,
                scale,
                pivot,
                items,
            } => {
                let mut boxes = items.iter().map(|d| d.bounds_at(1.0));
                let Some(first) = boxes.next() else {
                    return Rect { x: at.0, y: at.1, w: 0.0, h: 0.0 };
                };
                moved(&group_matrix(*at, *rot_deg, *scale, *pivot), boxes.fold(first, union))
            }
        }
    }
}

/// A closed circle about `c` of radius `r` as four cubics, one per quarter
/// (the mockup's rings, `popRing`, are strokes of it).
#[cfg_attr(not(test), allow(dead_code))]
pub fn circle_segs(c: (f32, f32), r: f32) -> Vec<Seg> {
    // The control arm that keeps a quarter's cubic on its circle.
    let k = 0.552_284_8 * r;
    let (x, y) = c;
    vec![
        Seg::Move(x + r, y),
        Seg::Cubic(x + r, y + k, x + k, y + r, x, y + r),
        Seg::Cubic(x - k, y + r, x - r, y + k, x - r, y),
        Seg::Cubic(x - r, y - k, x - k, y - r, x, y - r),
        Seg::Cubic(x + k, y - r, x + r, y - k, x + r, y),
        Seg::Close,
    ]
}

/// A `Draw::Head`'s turn and scale about its pivot at `at`.
pub fn head_matrix(at: (f32, f32), shot: &MascotShot) -> Matrix3x2 {
    Matrix3x2::scale_around(shot.sx, shot.sy, Vector2::new(at.0, at.1))
        * turn_about(shot.rot_deg, at, (0.0, 0.0))
}

/// A `Draw::Group`'s grid to its parent's: `pivot` to `at`, scaled and
/// turned about it.
pub fn group_matrix(
    at: (f32, f32),
    rot_deg: f32,
    scale: (f32, f32),
    pivot: (f32, f32),
) -> Matrix3x2 {
    Matrix3x2::scale_around(scale.0, scale.1, Vector2::new(pivot.0, pivot.1))
        * turn_about(rot_deg, pivot, (at.0 - pivot.0, at.1 - pivot.1))
}

/// The box `r` covers once moved by `m`.
#[cfg_attr(not(test), allow(dead_code))]
fn moved(m: &Matrix3x2, r: Rect) -> Rect {
    let corners = [(r.x, r.y), (r.x + r.w, r.y), (r.x, r.y + r.h), (r.x + r.w, r.y + r.h)];
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in corners {
        let (x, y) = (x * m.M11 + y * m.M21 + m.M31, x * m.M12 + y * m.M22 + m.M32);
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// The box of `path`'s points and control points (a curve stays inside
/// them), grown by `pad`.
#[cfg_attr(not(test), allow(dead_code))]
fn path_box(path: &[Seg], pad: f32) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut add = |x: f32, y: f32| (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    for seg in path {
        match *seg {
            Seg::Move(x, y) | Seg::Line(x, y) => add(x, y),
            Seg::Quad(a, b, x, y) => {
                add(a, b);
                add(x, y);
            }
            Seg::Cubic(a, b, c, d, x, y) => {
                add(a, b);
                add(c, d);
                add(x, y);
            }
            Seg::Close => {}
        }
    }
    if x0 > x1 {
        return Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
    }
    Rect {
        x: x0 - pad,
        y: y0 - pad,
        w: x1 - x0 + 2.0 * pad,
        h: y1 - y0 + 2.0 * pad,
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

/// How far a `Draw::Digit`'s ink can reach from its centre, turned any way,
/// outline included: half the diagonal of a clock character's line box.
#[cfg_attr(not(test), allow(dead_code))]
pub const DIGIT_REACH: f32 = 13.0;

/// The three whistle arcs of a `Draw::Whistle` at `at`: each runs from its
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::robot::Seg;

    fn near(a: Rect, b: Rect) -> bool {
        [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)].iter().all(|(p, q)| (p - q).abs() < 1e-3)
    }

    #[test]
    fn layers_paint_in_the_mockups_order() {
        use Pass::*;
        // The mockup's `compose`: the inside under the clock.
        assert_eq!(PAINT_ORDER, [Back, Fill, Dark, Inside, Clock, Wrap, Front]);
        // Each layer has its one place in it, whatever its wrap rect.
        let r = Rect { x: 1.0, y: 2.0, w: 3.0, h: 4.0 };
        let cases = [
            (Layer::Back, Back),
            (Layer::Dark, Dark),
            (Layer::Inside, Inside),
            (Layer::Wrap(None), Wrap),
            (Layer::Wrap(Some(r)), Wrap),
            (Layer::Front, Front),
        ];
        for (layer, pass) in cases {
            assert_eq!(layer.pass(), pass, "{layer:?}");
        }
    }

    #[test]
    fn a_group_moves_its_items_by_its_transform() {
        // A dot 10 units right of the pivot: scaled 2 wide, turned a quarter
        // clockwise, it lands 20 below `at`, its box turned with it.
        let dot = Draw::Ellipse { c: (42.0, 40.0), r: (1.0, 1.0), rgb: 0 };
        let group = Draw::Group {
            at: (100.0, 50.0),
            rot_deg: 90.0,
            scale: (2.0, 1.0),
            pivot: (32.0, 40.0),
            items: vec![dot.clone()],
        };
        assert!(near(dot.bounds(), Rect { x: 41.0, y: 39.0, w: 2.0, h: 2.0 }), "{:?}", dot.bounds());
        assert!(near(group.bounds(), Rect { x: 99.0, y: 68.0, w: 2.0, h: 4.0 }), "{:?}", group.bounds());
        // Unturned and unscaled, a group only moves its pivot to `at`.
        let moved = Draw::Group {
            at: (10.0, 20.0),
            rot_deg: 0.0,
            scale: (1.0, 1.0),
            pivot: (0.0, 0.0),
            items: vec![dot],
        };
        assert!(near(moved.bounds(), Rect { x: 51.0, y: 59.0, w: 2.0, h: 2.0 }), "{:?}", moved.bounds());
    }

    #[test]
    fn a_path_is_bounded_by_its_points_and_its_stroke() {
        let path = vec![Seg::Move(0.0, 0.0), Seg::Quad(10.0, -4.0, 20.0, 0.0), Seg::Line(20.0, 6.0), Seg::Close];
        let fill = Draw::Fill { path: path.clone(), rgb: 0 };
        assert!(near(fill.bounds(), Rect { x: 0.0, y: -4.0, w: 20.0, h: 10.0 }), "{:?}", fill.bounds());
        let stroke = Draw::Stroke { path, rgb: 0, width: 2.0 };
        assert!(near(stroke.bounds(), Rect { x: -1.0, y: -5.0, w: 22.0, h: 12.0 }), "{:?}", stroke.bounds());
    }

    #[test]
    fn a_shot_is_bounded_by_the_island_and_every_part() {
        let s = crate::ui::play::testing::shape(Style::Notch, 190.0, 34.0);
        let mut shot = Shot::rest(Style::Notch, s);
        assert_eq!(shot.bounds(), shot.island);
        let far = (shot.island.x - 40.0, shot.island.y + shot.island.h + 50.0);
        shot.parts.push(Part {
            layer: Layer::Back,
            draw: Draw::Ellipse { c: far, r: (2.0, 3.0), rgb: 0 },
            alpha: 1.0,
        });
        let b = shot.bounds();
        assert!((b.x - (far.0 - 2.0)).abs() < 1e-3 && (b.y + b.h - (far.1 + 3.0)).abs() < 1e-3, "{b:?}");
        assert!((b.x + b.w - (shot.island.x + shot.island.w)).abs() < 1e-3 && (b.y - shot.island.y).abs() < 1e-3, "{b:?}");
    }

    #[test]
    fn a_circle_path_lies_on_its_circle() {
        let (c, r) = ((40.0, -7.0), 6.5);
        let segs = circle_segs(c, r);
        let Seg::Move(mut x, mut y) = segs[0] else {
            panic!("starts with a move: {segs:?}");
        };
        let on = |(px, py): (f32, f32)| ((px - c.0).hypot(py - c.1) - r).abs() <= 0.01 * r;
        assert!(on((x, y)));
        let mut curves = 0;
        for seg in &segs[1..] {
            match *seg {
                Seg::Cubic(ax, ay, bx, by, ex, ey) => {
                    for i in 0..=20 {
                        let t = i as f32 / 20.0;
                        let u = 1.0 - t;
                        let p = (
                            u * u * u * x + 3.0 * u * u * t * ax + 3.0 * u * t * t * bx + t * t * t * ex,
                            u * u * u * y + 3.0 * u * u * t * ay + 3.0 * u * t * t * by + t * t * t * ey,
                        );
                        assert!(on(p), "{p:?} is off the circle");
                    }
                    (x, y) = (ex, ey);
                    curves += 1;
                }
                Seg::Close => {}
                ref other => panic!("not a cubic: {other:?}"),
            }
        }
        assert_eq!(curves, 4);
        assert!(matches!(segs.last(), Some(Seg::Close)));
        // Its box is the circle's.
        let b = Draw::Stroke { path: segs, rgb: 0, width: 0.0 }.bounds();
        assert!(near(b, Rect { x: c.0 - r, y: c.1 - r, w: 2.0 * r, h: 2.0 * r }), "{b:?}");
    }

    #[test]
    fn a_head_is_bounded_about_its_pivot_at_the_slots_size() {
        let shot = MascotShot {
            dx: 0.0,
            dy: 0.0,
            rot_deg: 0.0,
            sx: 1.0,
            sy: 1.0,
            expr: Expr::Idle,
            crest: CrestPose { rot_deg: 10.0, scale: 1.0 },
            glance: (0.0, 0.0),
            blink: false,
            beak_open: false,
        };
        let at = (300.0, 100.0);
        let b = Draw::Head { at, shot }.bounds();
        // It holds the slot head's square placed by its pivot.
        let k = ROBOT_COMPACT / GRID;
        let (x0, y0) = (at.0 - HEAD_PIVOT.0 * k, at.1 - HEAD_PIVOT.1 * k);
        assert!(b.x <= x0 && b.y <= y0 && b.x + b.w >= x0 + ROBOT_COMPACT && b.y + b.h >= y0 + ROBOT_COMPACT, "{b:?}");
        // Twice the size is twice the box; the offsets of a slot head do not move it.
        let big = Draw::Head { at, shot: MascotShot { sx: 2.0, sy: 2.0, dx: 50.0, dy: 50.0, ..shot } }.bounds();
        assert!((big.w - 2.0 * b.w).abs() < 1e-3 && (big.h - 2.0 * b.h).abs() < 1e-3, "{big:?}");
        assert!((big.x + big.w / 2.0 - (b.x + b.w / 2.0) * 2.0 + at.0).abs() < 1e-3, "{big:?} {b:?}");
    }
}
