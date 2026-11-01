//! The cat's coat and what it wears (Settings -> Kucing): four colours and
//! seven accessories on the 64-unit grid of `robot.rs`.
//!
//! The cat is painted as ONE silhouette, in this order (`RobotPainter::paint`):
//!
//! 1. Black only: a light rim under everything, both turned ears stroked wider
//!    than their own stroke and the head stroked wider than its outline.
//! 2. The silhouette in the base colour: both ears (turned, filled, round
//!    stroke) and the head, all one colour.
//! 3. Calico only: two patches over the silhouette, cut to the WHOLE
//!    silhouette (the head united with both turned ears, their stroke
//!    included), never to the head alone, so no light seam shows between an
//!    ear and the head and no bite is taken out of an ear.
//! 4. The face marks.
//! 5. The accessories, on top of everything.
//!
//! Everything but the painting is pure data: the colours, the accessory
//! shapes and their boxes (the clamp test in `geometry.rs` reads those).
//! Paths and the calico masks are built once per ear state and kept; nothing
//! is rebuilt per frame and nothing here owns a timer.

use std::rc::Rc;

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_COMBINE_MODE_INTERSECT, D2D1_COMBINE_MODE_UNION, ID2D1Geometry, ID2D1PathGeometry,
    ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::core::{Interface, Result as WinResult};
use windows_numerics::{Matrix3x2, Vector2};

use super::robot::{CREAM, EAR_STROKE, Ears, Ink, RobotPainter, Seg, Shape, Side, path};
use crate::config::Config;
use crate::theme::{CatAccessory, CatColor};

/// The cat's colour and accessory, as the config holds them. Every caller of
/// `RobotPainter::paint` passes the config's values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CatLook {
    pub color: CatColor,
    pub accessory: CatAccessory,
}

/// The espresso of the eyes on the light coats.
pub const EYES_DARK: u32 = 0x17120F;
/// The Black cat's eyes and every mark of them.
pub const AMBER: u32 = 0xF2B33D;
/// The Black cat's rim.
pub const RIM: u32 = 0x8E7CB8;
/// How far the rim reaches past the silhouette, in grid units.
pub const RIM_WIDTH: f32 = 1.6;
/// Calico's patches: the orange one on the left, the dark one on the right.
pub const PATCH_ORANGE: u32 = 0xE2683C;
pub const PATCH_DARK: u32 = 0x2B2420;

impl CatLook {
    pub fn of(cfg: &Config) -> Self {
        CatLook {
            color: cfg.cat_color,
            accessory: cfg.cat_accessory,
        }
    }

    /// Head and ears.
    pub fn head_rgb(self) -> u32 {
        match self.color {
            CatColor::Terracotta => 0xE2683C,
            CatColor::Calico => 0xF4EDE4,
            CatColor::White => 0xF6F1EA,
            CatColor::Black => 0x1E1B22,
        }
    }

    /// The eyes, and the blink lines, happy and sleep arcs and alert dots that
    /// replace them. Extras beside the head stay cream (`Ink::Cream`).
    pub fn eye_rgb(self) -> u32 {
        match self.color {
            CatColor::Black => AMBER,
            _ => EYES_DARK,
        }
    }

    /// The colour of a face mark's ink.
    pub fn ink_rgb(self, ink: Ink) -> u32 {
        match ink {
            Ink::Espresso => self.eye_rgb(),
            Ink::Cream => CREAM,
        }
    }

    /// The rim colour: the Black cat only.
    pub fn rim(self) -> Option<u32> {
        (self.color == CatColor::Black).then_some(RIM)
    }

    /// The glasses' frame: cream on the Black cat, else espresso.
    fn frame_rgb(self) -> u32 {
        match self.color {
            CatColor::Black => CREAM,
            _ => EYES_DARK,
        }
    }
}

// ---- Accessories (pure data, grid units) ----

/// What an accessory piece moves with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The head: the cat's slide and tilt.
    Head,
    /// The face's marks: the head, then the eyes' extra slide (glasses).
    Eyes,
    /// An ear: the head, and the ear's own droop turn and alert lift.
    Ear(Side),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Fixed(u32),
    /// Espresso, or cream on the Black cat.
    Frame,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// A closed shape, filled, with an optional round edge to soften corners.
    Shape {
        segs: Vec<Seg>,
        fill: Tone,
        edge: Option<f32>,
    },
    /// An open line with round caps.
    Stroke {
        segs: Vec<Seg>,
        tone: Tone,
        width: f32,
    },
    Dot {
        cx: f32,
        cy: f32,
        r: f32,
        tone: Tone,
    },
    Ring {
        cx: f32,
        cy: f32,
        r: f32,
        tone: Tone,
        width: f32,
    },
}

/// How much of an ear's alert lift (`Ears::up`, which raises its tip) an
/// accessory on that ear follows: it sits at the middle of the ear.
const EAR_LIFT: f32 = 0.6;

const RASPBERRY: u32 = 0xD23F6A;
const BOW_KNOT: u32 = 0xF07A9A;
const NAVY: u32 = 0x2F4A7A;
const MUSTARD: u32 = 0xE0A526;
const MUSTARD_DARK: u32 = 0xC48A18;
const PINK: u32 = 0xF29BB5;
const GOLD: u32 = 0xF2B33D;
const GOLD_DARK: u32 = 0xC8861C;

fn fixed(rgb: u32) -> Tone {
    Tone::Fixed(rgb)
}

/// The pieces of `accessory`, each with what it moves with, in paint order.
pub fn pieces(accessory: CatAccessory) -> Vec<(Anchor, Piece)> {
    use Seg::{Close, Line as To, Move, Quad};
    match accessory {
        CatAccessory::None => vec![],
        CatAccessory::Glasses => vec![
            (
                Anchor::Eyes,
                Piece::Ring {
                    cx: 24.0,
                    cy: 40.0,
                    r: 6.6,
                    tone: Tone::Frame,
                    width: 1.9,
                },
            ),
            (
                Anchor::Eyes,
                Piece::Ring {
                    cx: 40.0,
                    cy: 40.0,
                    r: 6.6,
                    tone: Tone::Frame,
                    width: 1.9,
                },
            ),
            (
                Anchor::Eyes,
                Piece::Stroke {
                    segs: vec![Move(30.6, 40.0), To(33.4, 40.0)],
                    tone: Tone::Frame,
                    width: 1.9,
                },
            ),
        ],
        CatAccessory::Bow => {
            let wing = |x: f32| Piece::Shape {
                segs: vec![Move(45.0, 23.0), To(x, 18.5), To(x, 27.5), Close],
                fill: fixed(RASPBERRY),
                edge: Some(1.6),
            };
            vec![
                (Anchor::Ear(Side::Right), wing(37.0)),
                (Anchor::Ear(Side::Right), wing(53.0)),
                (
                    Anchor::Ear(Side::Right),
                    Piece::Dot {
                        cx: 45.0,
                        cy: 23.0,
                        r: 2.4,
                        tone: fixed(BOW_KNOT),
                    },
                ),
            ]
        }
        CatAccessory::Nightcap => vec![
            // The cone over the dome, its tip to the upper left.
            (
                Anchor::Head,
                Piece::Shape {
                    segs: vec![
                        Move(19.0, 25.8),
                        Quad(32.0, 21.5, 45.0, 25.8),
                        Quad(40.0, 6.0, 14.5, 7.0),
                        Quad(10.5, 17.0, 19.0, 25.8),
                        Close,
                    ],
                    fill: fixed(NAVY),
                    edge: Some(1.2),
                },
            ),
            (
                Anchor::Head,
                Piece::Stroke {
                    segs: vec![Move(19.0, 25.8), Quad(32.0, 21.5, 45.0, 25.8)],
                    tone: fixed(CREAM),
                    width: 4.4,
                },
            ),
            (
                Anchor::Head,
                Piece::Dot {
                    cx: 14.5,
                    cy: 7.0,
                    r: 3.0,
                    tone: fixed(CREAM),
                },
            ),
        ],
        CatAccessory::Scarf => vec![
            // The tail first: it comes out from under the band.
            (
                Anchor::Head,
                Piece::Shape {
                    segs: vec![
                        Move(13.0, 52.0),
                        To(22.0, 52.0),
                        To(23.0, 60.5),
                        To(15.0, 61.5),
                        Close,
                    ],
                    fill: fixed(MUSTARD_DARK),
                    edge: Some(1.6),
                },
            ),
            (
                Anchor::Head,
                Piece::Shape {
                    segs: vec![
                        Move(9.5, 47.0),
                        To(54.5, 47.0),
                        Quad(58.0, 47.0, 58.0, 50.5),
                        To(58.0, 51.0),
                        Quad(58.0, 55.0, 54.0, 55.0),
                        To(10.0, 55.0),
                        Quad(6.0, 55.0, 6.0, 51.0),
                        To(6.0, 50.5),
                        Quad(6.0, 47.0, 9.5, 47.0),
                        Close,
                    ],
                    fill: fixed(MUSTARD),
                    edge: None,
                },
            ),
        ],
        CatAccessory::Flower => {
            let (cx, cy) = (16.0_f32, 23.0_f32);
            let mut v: Vec<(Anchor, Piece)> = (0..5)
                .map(|i| {
                    let a = (-90.0 + 72.0 * i as f32).to_radians();
                    (
                        Anchor::Ear(Side::Left),
                        Piece::Dot {
                            cx: cx + 3.6 * a.cos(),
                            cy: cy + 3.6 * a.sin(),
                            r: 2.6,
                            tone: fixed(PINK),
                        },
                    )
                })
                .collect();
            v.push((
                Anchor::Ear(Side::Left),
                Piece::Dot {
                    cx,
                    cy,
                    r: 2.0,
                    tone: fixed(GOLD),
                },
            ));
            v
        }
        CatAccessory::Crown => vec![
            (
                Anchor::Head,
                Piece::Shape {
                    segs: vec![
                        Move(23.5, 27.5),
                        To(23.5, 19.0),
                        To(28.0, 23.5),
                        To(32.0, 16.0),
                        To(36.0, 23.5),
                        To(40.5, 19.0),
                        To(40.5, 27.5),
                        Close,
                    ],
                    fill: fixed(GOLD),
                    edge: Some(1.4),
                },
            ),
            (
                Anchor::Head,
                Piece::Stroke {
                    segs: vec![Move(25.0, 25.8), To(39.0, 25.8)],
                    tone: fixed(GOLD_DARK),
                    width: 1.8,
                },
            ),
        ],
    }
}

/// The grid points of every accessory piece's box corners as the painter
/// places them for `ears` (before the head's own tilt, slide and bob), each
/// flagged when it also takes the eyes' extra slide. For the clamp test in
/// `geometry.rs`.
#[cfg(test)]
pub fn accessory_corners(accessory: CatAccessory, ears: Ears) -> Vec<((f32, f32), bool)> {
    accessory_piece_corners(accessory, ears)
        .into_iter()
        .flatten()
        .collect()
}

/// `accessory_corners`, kept apart per piece (each piece's four corners).
#[cfg(test)]
fn accessory_piece_corners(accessory: CatAccessory, ears: Ears) -> Vec<Vec<((f32, f32), bool)>> {
    let mut out = Vec::new();
    for (anchor, piece) in pieces(accessory) {
        let (l, t, r, b) = piece_bounds(&piece);
        let mut corners = Vec::with_capacity(4);
        for (x, y) in [(l, t), (r, t), (r, b), (l, b)] {
            let p = match anchor {
                Anchor::Head | Anchor::Eyes => (x, y),
                Anchor::Ear(side) => {
                    let pv = side.pivot();
                    let a = side.turn(ears.droop).to_radians();
                    let (dx, dy) = (x - pv.X, y - ears.up * EAR_LIFT - pv.Y);
                    (
                        pv.X + dx * a.cos() - dy * a.sin(),
                        pv.Y + dx * a.sin() + dy * a.cos(),
                    )
                }
            };
            corners.push((p, anchor == Anchor::Eyes));
        }
        out.push(corners);
    }
    out
}

/// The box of one piece, stroke included (curves by their control points).
#[cfg(test)]
fn piece_bounds(piece: &Piece) -> (f32, f32, f32, f32) {
    let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut add = |x: f32, y: f32, pad: f32| {
        b = (
            b.0.min(x - pad),
            b.1.min(y - pad),
            b.2.max(x + pad),
            b.3.max(y + pad),
        );
    };
    let mut segs = |segs: &[Seg], pad: f32| {
        for seg in segs {
            match *seg {
                Seg::Move(x, y) | Seg::Line(x, y) => add(x, y, pad),
                Seg::Quad(a, c, x, y) => {
                    add(a, c, pad);
                    add(x, y, pad);
                }
                Seg::Cubic(a, c, d, e, x, y) => {
                    add(a, c, pad);
                    add(d, e, pad);
                    add(x, y, pad);
                }
                Seg::Close => {}
            }
        }
    };
    match piece {
        Piece::Shape { segs: s, edge, .. } => segs(s, edge.unwrap_or(0.0) / 2.0),
        Piece::Stroke { segs: s, width, .. } => segs(s, width / 2.0),
        Piece::Dot { cx, cy, r, .. } => add(*cx, *cy, *r),
        Piece::Ring {
            cx, cy, r, width, ..
        } => add(*cx, *cy, r + width / 2.0),
    }
    b
}

// ---- Calico patches (grid units) ----

/// Left orange patch: `M-12 -12H30Q30 25 27 28Q19 34 4 40L-12 42Z`.
fn patch_left() -> Vec<Seg> {
    use Seg::{Close, Line as To, Move, Quad};
    vec![
        Move(-12.0, -12.0),
        To(30.0, -12.0),
        Quad(30.0, 25.0, 27.0, 28.0),
        Quad(19.0, 34.0, 4.0, 40.0),
        To(-12.0, 42.0),
        Close,
    ]
}

/// Right dark patch: `M76 -12H34Q34 25 37 27Q46 32 60 36L76 38Z`.
fn patch_right() -> Vec<Seg> {
    use Seg::{Close, Line as To, Move, Quad};
    vec![
        Move(76.0, -12.0),
        To(34.0, -12.0),
        Quad(34.0, 25.0, 37.0, 27.0),
        Quad(46.0, 32.0, 60.0, 36.0),
        To(76.0, 38.0),
        Close,
    ]
}

/// Flattening tolerance of the geometry operations, in grid units.
const TOLERANCE: f32 = 0.05;

/// A piece's built path, kept with the piece.
pub(super) struct Placed {
    anchor: Anchor,
    piece: Piece,
    geom: Option<ID2D1PathGeometry>,
}

impl RobotPainter {
    /// The head united with both ears as drawn for `ears` (each turned about
    /// its pivot, fill and round stroke), in grid units.
    fn silhouette(&self, ears: Ears) -> WinResult<ID2D1PathGeometry> {
        let missing = || windows::core::Error::from(windows::Win32::Foundation::E_FAIL);
        let head = self.path(Shape::Head).ok_or_else(missing)?;
        let mut acc: ID2D1Geometry = head.cast()?;
        for side in [Side::Left, Side::Right] {
            let ear = self.path(Shape::Ear(side, ears.up)).ok_or_else(missing)?;
            let turn = Matrix3x2::rotation_around(side.turn(ears.droop), side.pivot());
            unsafe {
                let fill = self.factory.CreateTransformedGeometry(&ear, &turn)?;
                acc = self.union(&acc, &fill.cast::<ID2D1Geometry>()?)?;
                let band = self.factory.CreatePathGeometry()?;
                let sink = band.Open()?;
                ear.Widen(EAR_STROKE, &self.round, Some(&turn), TOLERANCE, &sink)?;
                sink.Close()?;
                acc = self.union(&acc, &band.cast::<ID2D1Geometry>()?)?;
            }
        }
        acc.cast::<ID2D1PathGeometry>().or_else(|_| {
            // `acc` is a path geometry from the second union on; the cast only
            // fails if both ears were skipped, which `missing` rules out.
            Err(missing())
        })
    }

    fn union(&self, a: &ID2D1Geometry, b: &ID2D1Geometry) -> WinResult<ID2D1Geometry> {
        unsafe {
            let out = self.factory.CreatePathGeometry()?;
            let sink = out.Open()?;
            a.CombineWithGeometry(b, D2D1_COMBINE_MODE_UNION, None, TOLERANCE, &sink)?;
            sink.Close()?;
            out.cast()
        }
    }

    /// The two calico patches cut to the whole silhouette for `ears`
    /// (orange, dark), built the first time that ear state is drawn.
    fn calico_patches(&self, ears: Ears) -> Option<[ID2D1PathGeometry; 2]> {
        let key = (ears.up.to_bits(), ears.droop.to_bits());
        if let Some((_, g)) = self.calico.borrow().iter().find(|(k, _)| *k == key) {
            return Some(g.clone());
        }
        let built = (|| -> WinResult<[ID2D1PathGeometry; 2]> {
            let whole = self.silhouette(ears)?;
            let cut = |segs: Vec<Seg>| -> WinResult<ID2D1PathGeometry> {
                let patch = path(&self.factory, &segs)?;
                unsafe {
                    let out = self.factory.CreatePathGeometry()?;
                    let sink = out.Open()?;
                    patch.CombineWithGeometry(
                        &whole,
                        D2D1_COMBINE_MODE_INTERSECT,
                        None,
                        TOLERANCE,
                        &sink,
                    )?;
                    sink.Close()?;
                    Ok(out)
                }
            };
            Ok([cut(patch_left())?, cut(patch_right())?])
        })()
        .ok()?;
        self.calico.borrow_mut().push((key, built.clone()));
        Some(built)
    }

    /// The accessory's pieces with their paths, built the first time.
    fn extras(&self, accessory: CatAccessory) -> Rc<Vec<Placed>> {
        if let Some((_, v)) = self.extras.borrow().iter().find(|(a, _)| *a == accessory) {
            return v.clone();
        }
        let built: Vec<Placed> = pieces(accessory)
            .into_iter()
            .map(|(anchor, piece)| {
                let geom = match &piece {
                    Piece::Shape { segs, .. } | Piece::Stroke { segs, .. } => {
                        path(&self.factory, segs).ok()
                    }
                    _ => None,
                };
                Placed {
                    anchor,
                    piece,
                    geom,
                }
            })
            .collect();
        let built = Rc::new(built);
        self.extras.borrow_mut().push((accessory, built.clone()));
        built
    }

    /// Paints the calico patches over the silhouette, in `world`.
    pub(super) fn paint_patches(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        world: &Matrix3x2,
        ears: Ears,
        alpha: f32,
    ) {
        let Some([orange, dark]) = self.calico_patches(ears) else {
            return;
        };
        unsafe {
            rt.SetTransform(world);
            for (g, rgb) in [(orange, PATCH_ORANGE), (dark, PATCH_DARK)] {
                brush.SetColor(&solid(rgb, alpha));
                rt.FillGeometry(&g, brush, None);
            }
        }
    }

    /// Paints the accessory on top of the cat. `world` is the head's
    /// transform and `eyes` the eyes' (the head's, plus their slide).
    pub(super) fn paint_accessory(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        look: CatLook,
        ears: Ears,
        (world, eyes): (&Matrix3x2, &Matrix3x2),
        alpha: f32,
    ) {
        if look.accessory == CatAccessory::None {
            return;
        }
        let tone = |t: Tone| match t {
            Tone::Fixed(rgb) => rgb,
            Tone::Frame => look.frame_rgb(),
        };
        let extras = self.extras(look.accessory);
        unsafe {
            for placed in extras.iter() {
                let m = match placed.anchor {
                    Anchor::Head => *world,
                    Anchor::Eyes => *eyes,
                    Anchor::Ear(side) => {
                        Matrix3x2::translation(0.0, -ears.up * EAR_LIFT)
                            * Matrix3x2::rotation_around(side.turn(ears.droop), side.pivot())
                            * *world
                    }
                };
                rt.SetTransform(&m);
                match &placed.piece {
                    Piece::Shape { fill, edge, .. } => {
                        if let Some(g) = &placed.geom {
                            brush.SetColor(&solid(tone(*fill), alpha));
                            rt.FillGeometry(g, brush, None);
                            if let Some(w) = edge {
                                rt.DrawGeometry(g, brush, *w, &self.round);
                            }
                        }
                    }
                    Piece::Stroke { tone: t, width, .. } => {
                        if let Some(g) = &placed.geom {
                            brush.SetColor(&solid(tone(*t), alpha));
                            rt.DrawGeometry(g, brush, *width, &self.round);
                        }
                    }
                    Piece::Dot { cx, cy, r, tone: t } => {
                        brush.SetColor(&solid(tone(*t), alpha));
                        rt.FillEllipse(&circle(*cx, *cy, *r), brush);
                    }
                    Piece::Ring {
                        cx,
                        cy,
                        r,
                        tone: t,
                        width,
                    } => {
                        brush.SetColor(&solid(tone(*t), alpha));
                        rt.DrawEllipse(&circle(*cx, *cy, *r), brush, *width, &self.round);
                    }
                }
            }
        }
    }
}

fn circle(cx: f32, cy: f32, r: f32) -> windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
    windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
        point: Vector2::new(cx, cy),
        radiusX: r,
        radiusY: r,
    }
}

/// `0xRRGGBB` at `alpha`.
pub(super) fn solid(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    let ch = |s: u32| ((rgb >> s) & 0xFF) as f32 / 255.0;
    D2D1_COLOR_F {
        r: ch(16),
        g: ch(8),
        b: ch(0),
        a: alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::robot::{Expr, Part, face, mark_bounds};
    use windows::Win32::Graphics::Direct2D::{
        D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Factory,
    };

    fn painter() -> RobotPainter {
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }.unwrap();
        RobotPainter::new(&factory).unwrap()
    }

    fn contains(g: &ID2D1PathGeometry, x: f32, y: f32) -> bool {
        unsafe { g.FillContainsPoint(Vector2::new(x, y), None, TOLERANCE) }
            .unwrap()
            .as_bool()
    }

    #[test]
    fn every_coat_has_its_pinned_colours() {
        let look = |color| CatLook {
            color,
            accessory: CatAccessory::None,
        };
        let c = |color| {
            (
                look(color).head_rgb(),
                look(color).eye_rgb(),
                look(color).rim(),
            )
        };
        assert_eq!(c(CatColor::Terracotta), (0xE2683C, 0x17120F, None));
        assert_eq!(c(CatColor::Calico), (0xF4EDE4, 0x17120F, None));
        assert_eq!(c(CatColor::White), (0xF6F1EA, 0x17120F, None));
        assert_eq!(c(CatColor::Black), (0x1E1B22, 0xF2B33D, Some(0x8E7CB8)));
        assert_eq!(CatLook::default(), look(CatColor::Terracotta));
    }

    #[test]
    fn the_black_cat_marks_its_eyes_in_amber_and_keeps_the_extras_cream() {
        let black = CatLook {
            color: CatColor::Black,
            accessory: CatAccessory::None,
        };
        let terracotta = CatLook::default();
        for expr in Expr::ALL {
            for blink in [false, true] {
                for part in face(expr, blink).parts {
                    let (ink, eye) = match part {
                        Part::Pill { .. } => (None, true),
                        Part::Dot { ink, .. } => (Some(ink), ink == Ink::Espresso),
                        Part::Stroke(line) => (Some(line.ink()), line.ink() == Ink::Espresso),
                        Part::Heart { .. } => (None, false),
                    };
                    // Every eye mark (pills, alert dots, blink, happy and sleep
                    // lines) is amber; z's, notes, thought dots and the heart stay cream.
                    if eye {
                        let rgb = ink.map_or(black.eye_rgb(), |i| black.ink_rgb(i));
                        assert_eq!(rgb, AMBER, "{expr:?} {part:?}");
                        let dark = ink.map_or(terracotta.eye_rgb(), |i| terracotta.ink_rgb(i));
                        assert_eq!(dark, EYES_DARK);
                    } else if let Some(ink) = ink {
                        assert_eq!(black.ink_rgb(ink), CREAM, "{expr:?} {part:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_accessory_has_pieces_on_the_right_anchor() {
        assert!(pieces(CatAccessory::None).is_empty());
        assert_eq!(CatAccessory::ALL.len(), 7);
        for &a in &CatAccessory::ALL[1..] {
            assert!(!pieces(a).is_empty(), "{a:?}");
            assert!(!accessory_corners(a, Ears::default()).is_empty(), "{a:?}");
        }
        let anchors = |a| {
            pieces(a)
                .into_iter()
                .map(|(anchor, _)| anchor)
                .collect::<Vec<_>>()
        };
        assert!(
            anchors(CatAccessory::Bow)
                .iter()
                .all(|a| *a == Anchor::Ear(Side::Right))
        );
        assert!(
            anchors(CatAccessory::Flower)
                .iter()
                .all(|a| *a == Anchor::Ear(Side::Left))
        );
        assert!(
            anchors(CatAccessory::Glasses)
                .iter()
                .all(|a| *a == Anchor::Eyes)
        );
        for a in [
            CatAccessory::Nightcap,
            CatAccessory::Scarf,
            CatAccessory::Crown,
        ] {
            assert!(anchors(a).iter().all(|x| *x == Anchor::Head), "{a:?}");
        }
        // The glasses are two rings of 6.6 round the eyes at (24, 40) and (40, 40).
        let rings: Vec<_> = pieces(CatAccessory::Glasses)
            .into_iter()
            .filter_map(|(_, p)| match p {
                Piece::Ring { cx, cy, r, .. } => Some((cx, cy, r)),
                _ => None,
            })
            .collect();
        assert_eq!(rings, [(24.0, 40.0, 6.6), (40.0, 40.0, 6.6)]);
    }

    #[test]
    fn ear_accessories_follow_their_ears_turn_and_lift() {
        let at = |ears| {
            let c = accessory_corners(CatAccessory::Bow, ears);
            (c[0].0.0, c[0].0.1)
        };
        let rest = at(Ears::default());
        let up = at(Ears {
            up: 4.0,
            droop: 0.0,
        });
        assert!(up.1 < rest.1, "the alert lift raises the bow");
        let sad = at(Ears {
            up: 0.0,
            droop: 8.0,
        });
        assert!(
            (sad.0 - rest.0).abs() > 1.0,
            "the droop turns it with the ear"
        );
        // The glasses ignore the ears.
        let g = |ears| accessory_corners(CatAccessory::Glasses, ears);
        assert_eq!(
            g(Ears::default()),
            g(Ears {
                up: 4.0,
                droop: 8.0
            })
        );
    }

    /// Every accessory is drawn on every face, painted last: none of its
    /// pieces may cover the cream extras beside the head (the wave's heart,
    /// the z's, the notes, the thought dots), or a cream piece merges with
    /// them and the mark cannot be read.
    #[test]
    fn no_accessory_covers_a_faces_cream_extras() {
        let overlap = |a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)| {
            a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
        };
        for expr in Expr::ALL {
            for blink in [false, true] {
                let f = face(expr, blink);
                let cream: Vec<_> = f
                    .parts
                    .iter()
                    .filter(|part| match part {
                        Part::Pill { .. } => false,
                        Part::Dot { ink, .. } => *ink == Ink::Cream,
                        Part::Stroke(line) => line.ink() == Ink::Cream,
                        Part::Heart { .. } => true,
                    })
                    .collect();
                if cream.is_empty() {
                    continue;
                }
                // The marks slide with the eyes and the head's pieces do not;
                // a face with extras never looks, so both share one transform.
                assert!(!expr.looks(), "{expr:?}");
                for accessory in CatAccessory::ALL {
                    for corners in accessory_piece_corners(*accessory, f.ears) {
                        let piece = corners.iter().fold(
                            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                            |b, &((x, y), _)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)),
                        );
                        for part in &cream {
                            let mark = mark_bounds(part);
                            assert!(
                                !overlap(piece, mark),
                                "{accessory:?} piece {piece:?} covers {expr:?}'s {part:?} at {mark:?}"
                            );
                        }
                    }
                }
            }
        }
        // The dancing note floats up to `NOTE_RISE_DIP`: every accessory also
        // stays clear of the whole way it rises (a compact cat is
        // `ROBOT_COMPACT` px, so a dip is `1 / k` grid units). Sing's notes,
        // shifted down under a 28 to 30 dip notch, do not rise, and the bow's
        // box only grazes that shift (0.3 grid units at the very worst: the
        // 28 dip notch with no bass), so only the rise is swept.
        let k = crate::ui::geometry::ROBOT_COMPACT / crate::ui::robot::GRID;
        let up = crate::ui::robot::NOTE_RISE_DIP / k;
        for expr in [Expr::Music, Expr::Sing] {
            let f = face(expr, false);
            let notes = f.parts.iter().filter(|part| match part {
                Part::Dot { ink, .. } => *ink == Ink::Cream,
                Part::Stroke(line) => line.ink() == Ink::Cream,
                _ => false,
            });
            for part in notes {
                let m = mark_bounds(part);
                let swept = (m.0, m.1 - up, m.2, m.3);
                for accessory in CatAccessory::ALL {
                    for corners in accessory_piece_corners(*accessory, f.ears) {
                        let piece = corners.iter().fold(
                            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                            |b, &((x, y), _)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)),
                        );
                        assert!(
                            !overlap(piece, swept),
                            "{accessory:?} piece {piece:?} is in the way of {expr:?}'s risen {part:?} ({swept:?})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn calico_patches_are_cut_to_the_whole_silhouette_ears_included() {
        let p = painter();
        for ears in [
            Ears::default(),
            Ears {
                up: 4.0,
                droop: 0.0,
            },
            Ears {
                up: 0.0,
                droop: 4.0,
            },
            Ears {
                up: 0.0,
                droop: 8.0,
            },
        ] {
            let [orange, dark] = p.calico_patches(ears).expect("patches build");
            // Inside a left ear, above the head: the orange patch covers it
            // (clipped to the head alone it would be cut off).
            assert!(contains(&orange, 14.0, 18.0), "{ears:?}");
            assert!(contains(&dark, 50.0, 18.0), "{ears:?}");
            // The forehead between the patches stays cream.
            assert!(
                !contains(&orange, 32.0, 32.0) && !contains(&dark, 32.0, 32.0),
                "{ears:?}"
            );
            // Nothing outside the silhouette: far above the head, and the
            // left patch never reaches the right side.
            for (x, y) in [(0.0, 0.0), (32.0, 2.0), (60.0, 60.0), (-5.0, 30.0)] {
                assert!(
                    !contains(&orange, x, y) && !contains(&dark, x, y),
                    "({x}, {y})"
                );
            }
            assert!(!contains(&orange, 50.0, 18.0) && !contains(&dark, 14.0, 18.0));
            // The silhouette itself holds the head and both ears with their stroke.
            let whole = p.silhouette(ears).unwrap();
            for (x, y) in [(32.0, 40.0), (14.0, 18.0), (50.0, 18.0)] {
                assert!(contains(&whole, x, y), "{ears:?} ({x}, {y})");
            }
            assert!(!contains(&whole, 32.0, 6.0));
        }
        // Built once per ear state.
        assert_eq!(p.calico.borrow().len(), 4);
        p.calico_patches(Ears::default()).unwrap();
        assert_eq!(p.calico.borrow().len(), 4);
    }

    #[test]
    fn accessory_paths_are_built_once() {
        let p = painter();
        for &a in &CatAccessory::ALL[1..] {
            let built = p.extras(a);
            assert_eq!(built.len(), pieces(a).len());
            assert!(Rc::ptr_eq(&built, &p.extras(a)), "{a:?} is kept");
        }
        assert_eq!(p.extras.borrow().len(), 6);
    }
}
