//! Vale's cat, the notch assistant: a terracotta head with a flat base and
//! two ears, and plain espresso pill eyes; nothing else on the face. The
//! cat floats in the island (its edge does not cut it). Shapes follow
//! variant D1 of `docs/design/vale-kucing-minimal.html` (a 64-unit grid) and
//! are drawn with Direct2D at any size; colours are the fixed brand colours,
//! not the user's accent.
//!
//! Everything here except `RobotPainter` is pure: which face to show
//! (`choose_expr`), when it blinks, sleeps, bounces or lifts, and which
//! one-shot timers that needs (`plan_timers`). `app.rs` only feeds it events
//! and arms the timers it asks for.
//!
//! While work runs (a Claude Code session, a CLI command, a Mole clean) the
//! idle cat types on a small laptop (`Expr::Typing`, the "Typing" section):
//! its paws take turns on a one-shot timer, its eyes (only its eyes) glance
//! at the pointer, and the island grows `TYPING_LEAD` to the left to hold the
//! laptop. `Expr::Think` stays for the demo sheet and the notice scenes, but
//! `choose_expr` never picks it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::NaiveDateTime;
// `named_enum!` below names these unqualified.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_ELLIPSE, D2D1_LINE_JOIN_ROUND,
    D2D1_QUADRATIC_BEZIER_SEGMENT, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, ID2D1Factory,
    ID2D1Geometry, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::core::Result as WinResult;
use windows_numerics::{Matrix3x2, Vector2};

use super::cat_style::{CatLook, Placed, RIM_WIDTH, anchors};
use super::mascot::MascotShape;
use super::geometry::{EYES_H, Rect, ShapeRect, shape_contains};
use crate::config::Style;
use crate::greeting::{self, DayPeriod, GreetMark};
use crate::i18n::Key;
use crate::model::{NoticeKind, View};
use crate::theme::{CatAccessory, CatColor, Mascot};

/// The brand colour: the logo, and the Terracotta coat (`cat_style::CatLook`).
pub const TERRACOTTA: u32 = 0xE2683C;
/// The marks beside the head (the logo's dot).
pub const CREAM: u32 = 0xF4EDE4;

/// The design grid.
pub const GRID: f32 = 64.0;
/// Eye centres and their line.
const L: f32 = 24.0;
const R: f32 = 40.0;
const Y: f32 = 40.0;

/// The eleven faces. Blinking is not one of them: it is a `Pose` input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Expr {
    Idle,
    Happy,
    Focus,
    /// "Mendengar": music plays.
    Music,
    Sing,
    Alert,
    Think,
    Sleep,
    /// "Lesu".
    Sad,
    /// "Menyapa".
    Wave,
    /// "Mengetik": work runs; the cat types on a laptop beside it.
    Typing,
}

impl Expr {
    // The demo sheet (debug builds) and tests list and name every face.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub const ALL: [Expr; 11] = [
        Expr::Idle,
        Expr::Happy,
        Expr::Focus,
        Expr::Music,
        Expr::Sing,
        Expr::Alert,
        Expr::Think,
        Expr::Sleep,
        Expr::Sad,
        Expr::Wave,
        Expr::Typing,
    ];

    /// Its eyes are open (pills or round): a blink shows. Arcs do not blink.
    pub fn blinks(self) -> bool {
        matches!(
            self,
            Expr::Idle | Expr::Focus | Expr::Alert | Expr::Think | Expr::Sad | Expr::Typing
        )
    }

    /// The face turns toward the pointer (`Pose::looking_as`): only the faces
    /// with nothing floating beside the head. The thought dots and the z's
    /// sit beside the right ear, a few dip from the idle clock, and would
    /// slide into it; a sleeping or thinking cat does not follow anyway.
    pub fn looks(self) -> bool {
        !matches!(
            self,
            Expr::Think | Expr::Sleep | Expr::Music | Expr::Sing | Expr::Wave | Expr::Typing
        )
    }

    /// Only its eyes glance at the pointer (`Pose::glancing`): the typing
    /// cat. Its head, ears, body and laptop never move.
    pub fn glances(self) -> bool {
        self == Expr::Typing
    }

    /// The pointer poll matters to this face: it looks or glances.
    pub fn follows(self) -> bool {
        self.looks() || self.glances()
    }

    /// The name the demo sheet labels it with.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn key(self) -> Key {
        match self {
            Expr::Idle => Key::ExprIdle,
            Expr::Happy => Key::ExprHappy,
            Expr::Focus => Key::ExprFocus,
            Expr::Music => Key::ExprMusic,
            Expr::Sing => Key::ExprSing,
            Expr::Alert => Key::ExprAlert,
            Expr::Think => Key::ExprThink,
            Expr::Sleep => Key::ExprSleep,
            Expr::Sad => Key::ExprSad,
            Expr::Wave => Key::ExprWave,
            Expr::Typing => Key::ExprTyping,
        }
    }
}

/// How far the idle cat turns toward a pointer at the far edge of the screen
/// (a look of 1): the head and ears slide `LOOK_HEAD_DIP`, tilt `LOOK_TILT_DEG`
/// degrees toward it, and the eyes slide `LOOK_EYE_DIP` further across the
/// face. Small enough that the head, the ears (a sad cat's drooped ones
/// too) and every mark of a face that looks stay inside the idle island and
/// clear of its clock (`the_looking_cat_stays_in_the_idle_island`); at 8 the
/// sad cat's drooped right ear touched the clock.
pub const LOOK_HEAD_DIP: f32 = 7.0;
pub const LOOK_TILT_DEG: f32 = 6.0;
pub const LOOK_EYE_DIP: f32 = 4.0;

/// How far the typing cat's eyes (and only its eyes) slide toward a pointer
/// at the edge of the glance's reach, in grid units: sideways and down or up.
/// Small enough that the pills stay inside the head with room to spare
/// (`the_typing_cat_stays_in_the_idle_island`).
pub const GLANCE_X: f32 = 4.0;
pub const GLANCE_Y: f32 = 1.5;
/// How high a typing paw lifts, in grid units.
pub const PAW_LIFT: f32 = 1.6;

/// The glance in grid units for a pointer at `(vx, vy)` (each -1 to 1,
/// clamped; NaN is 0).
pub fn glance_offset(vx: f32, vy: f32) -> (f32, f32) {
    let unit = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(-1.0, 1.0) };
    (GLANCE_X * unit(vx), GLANCE_Y * unit(vy))
}

/// Moment-to-moment state on top of the face: eyes shut for a blink, how far
/// the whole cat is raised (hover lift, alert bounce), in px, and where it
/// looks (the idle cat following the pointer, `pointer_watch::LookEase`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub blink: bool,
    pub lift_px: f32,
    pub bounce_px: f32,
    /// The whole cat (head and ears) slides sideways, in dip.
    pub head_dx_dip: f32,
    /// And tilts, in degrees clockwise, on top of the face's own tilt.
    pub tilt_deg: f32,
    /// The face's marks slide further across the head, in dip.
    pub eye_dx_dip: f32,
    /// The dance's nod: the whole cat moves down, in dip (0 is at rest).
    pub nod_dip: f32,
    /// The nod's squash, 0 to 1: width grows by `SQUASH_W` of it, height
    /// shrinks by `SQUASH_H`, around the head's base centre.
    pub squash: f32,
    /// The music note's marks move further, in dip (negative is up). Set by
    /// the renderer from `note_rise` once it knows how much room the shape
    /// leaves above the note (`note_lift_dip`); never by `dance`.
    pub note_dy_dip: f32,
    /// 0 is a fully visible note, 1 is gone: the marks' alpha is `1 - note_fade`.
    pub note_fade: f32,
    /// The dance is on (`dance` sets it): the renderer then clamps the note
    /// inside the shape. Off, every field above stays as it was.
    pub dancing: bool,
    /// How far through its rise the note is, 0 to 1 (`note_phase`).
    pub note_rise: f32,
    /// The typing cat's glance: its face marks slide further by this much,
    /// in grid units (`glance_offset`). The head and body stay.
    pub glance_x: f32,
    pub glance_y: f32,
    /// How high the typing cat's left (0) and right (1) paw are raised, in
    /// grid units.
    pub paw_lift: [f32; 2],
    /// The dance's phase in radians (`pi * elapsed / BEAT_MS`); the falk's
    /// crest sways with its sine.
    pub beat: f32,
    /// The falk's hop on a "done" notice, in px.
    pub hop_px: f32,
    /// The greeting's swing, -1 to 1 (0 at rest).
    pub wave: f32,
    /// The greeting is running (`wave` is then read).
    pub waving: bool,
    /// The octopus's flush, 0 to 1.
    pub flush: f32,
}

impl Pose {
    /// The dancing pose: nod and squash by `amount` (0 to 1), the note
    /// `note_rise` of the way up (0 to 1) and faded by `note_fade`.
    pub fn dance(self, amount: f32, note_rise: f32, note_fade: f32) -> Pose {
        let amount = if amount.is_nan() {
            0.0
        } else {
            amount.clamp(0.0, 1.0)
        };
        Pose {
            nod_dip: NOD_MAX_DIP * amount,
            squash: amount,
            note_fade: note_fade.clamp(0.0, 1.0),
            dancing: true,
            note_rise: note_rise.clamp(0.0, 1.0),
            ..self
        }
    }

    /// The cat looking `v` of the way to the right edge (-1 to 1; negative
    /// to the left): 0 is the pose as it has always been.
    pub fn looking(self, v: f32) -> Pose {
        let v = v.clamp(-1.0, 1.0);
        Pose {
            head_dx_dip: LOOK_HEAD_DIP * v,
            tilt_deg: LOOK_TILT_DEG * v,
            eye_dx_dip: LOOK_EYE_DIP * v,
            ..self
        }
    }

    /// The typing cat glancing at a pointer `(vx, vy)` (each -1 to 1): the
    /// eyes only, no head slide or tilt.
    pub fn glancing(self, vx: f32, vy: f32) -> Pose {
        let (glance_x, glance_y) = glance_offset(vx, vy);
        Pose {
            glance_x,
            glance_y,
            ..self
        }
    }

    /// `looking(v)` for a face that looks (`Expr::looks`); any other face
    /// keeps the pose as it is, centred.
    pub fn looking_as(self, expr: Expr, v: f32) -> Pose {
        if expr.looks() { self.looking(v) } else { self }
    }
}

// ---- Shapes (pure data, in grid units) ----

/// The colour of a face mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Espresso,
    Cream,
}

/// A stroked line of the face, with round caps and joins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Line {
    /// The happy eyes: two arcs bending up.
    Happy,
    /// A blink: two short level lines.
    Blink,
    /// Closed eyes bending down.
    Sleep,
    /// The two small z's.
    Zz,
    /// A music note's stem and beam, its foot at (x, y).
    Note { x: f32, y: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    /// Control point, then end point (an SVG `q`, made absolute).
    Quad(f32, f32, f32, f32),
    /// Two control points, then the end point (an SVG `C`).
    Cubic(f32, f32, f32, f32, f32, f32),
    /// Closes the figure (an SVG `Z`).
    Close,
}

impl Line {
    fn width(self) -> f32 {
        match self {
            Line::Happy | Line::Blink => 2.4,
            Line::Sleep => 2.2,
            Line::Note { .. } => 1.6,
            Line::Zz => 1.5,
        }
    }

    pub(super) fn ink(self) -> Ink {
        match self {
            Line::Zz | Line::Note { .. } => Ink::Cream,
            _ => Ink::Espresso,
        }
    }

    fn alpha(self) -> f32 {
        if self == Line::Zz { 0.85 } else { 1.0 }
    }

    /// The design's path data, absolute.
    fn segments(self) -> Vec<Seg> {
        use Seg::{Line as To, Move, Quad};
        match self {
            // M L-3.8 Y+1.8 q3.8-5.4 7.6 0, twice.
            Line::Happy => vec![
                Move(L - 3.8, Y + 1.8),
                Quad(L, Y - 3.6, L + 3.8, Y + 1.8),
                Move(R - 3.8, Y + 1.8),
                Quad(R, Y - 3.6, R + 3.8, Y + 1.8),
            ],
            // M L-3.4 Y+.8 h6.8, twice.
            Line::Blink => vec![
                Move(L - 3.4, Y + 0.8),
                To(L + 3.4, Y + 0.8),
                Move(R - 3.4, Y + 0.8),
                To(R + 3.4, Y + 0.8),
            ],
            // M L-3.6 Y q3.6 3.4 7.2 0, twice.
            Line::Sleep => vec![
                Move(L - 3.6, Y),
                Quad(L, Y + 3.4, L + 3.6, Y),
                Move(R - 3.6, Y),
                Quad(R, Y + 3.4, R + 3.6, Y),
            ],
            // M55 3h5l-5 5.5h5 M62-4h3.6l-3.6 4h3.6, clear of the right ear.
            Line::Zz => vec![
                Move(55.0, 3.0),
                To(60.0, 3.0),
                To(55.0, 8.5),
                To(60.0, 8.5),
                Move(62.0, -4.0),
                To(65.6, -4.0),
                To(62.0, 0.0),
                To(65.6, 0.0),
            ],
            // note(x, y): M x y v-8 l5-1.5 v8
            Line::Note { x, y } => vec![
                Move(x, y),
                To(x, y - 8.0),
                To(x + 5.0, y - 9.5),
                To(x + 5.0, y - 1.5),
            ],
        }
    }
}

/// One mark of a face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    /// A filled espresso rounded rect (the open eyes are pills; a short
    /// one has elliptic corners, `rx` across and `ry` down).
    Pill {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rx: f32,
        ry: f32,
    },
    /// A filled circle (round eyes, the note's heads, thinking dots).
    Dot {
        cx: f32,
        cy: f32,
        r: f32,
        ink: Ink,
    },
    Stroke(Line),
    /// The cream heart beside a waving cat, its top notch at (x, y).
    Heart {
        x: f32,
        y: f32,
    },
}

/// Where the ears stand: `up` raises the tips, `droop` turns both ears
/// outward by `droop * 3` degrees around their base.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ears {
    pub up: f32,
    pub droop: f32,
}

/// A face: its marks, the ears, the vertical bob and the tilt (degrees,
/// clockwise). `eye_shift` moves every mark across the head, in grid units,
/// before the tilt (the typing cat looks down and to the laptop); it is
/// `(0, 0)` for every other face.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub parts: Vec<Part>,
    pub ears: Ears,
    pub bob: f32,
    pub tilt: f32,
    pub eye_shift: (f32, f32),
}

/// An espresso pill centred on (x, y), with SVG's corner rule: `rx` is
/// half the width and `ry` the same, each capped at half its side.
fn pill(x: f32, y: f32, w: f32, h: f32) -> Part {
    let rx = w / 2.0;
    Part::Pill {
        x: x - w / 2.0,
        y: y - h / 2.0,
        w,
        h,
        rx,
        ry: rx.min(h / 2.0),
    }
}

fn dot(cx: f32, cy: f32, r: f32, ink: Ink) -> Part {
    Part::Dot { cx, cy, r, ink }
}

/// The design's `open(h, w, dy)`: two pills, fully rounded.
fn open(h: f32, w: f32, dy: f32) -> [Part; 2] {
    [pill(L, Y + dy, w, h), pill(R, Y + dy, w, h)]
}

/// The design's `note(x, y)`: stem and beam, then the two heads.
fn note(x: f32, y: f32) -> [Part; 3] {
    [
        Part::Stroke(Line::Note { x, y }),
        dot(x - 1.6, y, 1.9, Ink::Cream),
        dot(x + 3.4, y - 1.5, 1.9, Ink::Cream),
    ]
}

/// The typing cat's whole body tilts this many degrees (clockwise), toward
/// the laptop on its left; and its eyes sit this far across and down the head
/// (grid units), so the idle pills' centres land at (20.5, 41) and (36.5, 41).
const TYPING_TILT: f32 = -4.0;
const TYPING_EYE_SHIFT: (f32, f32) = (-3.5, 1.0);

/// The marks of `expr`: its eyes first, then what floats beside the head.
/// With `blink`, open eyes (pills and round ones) become the design's
/// `blink` eyes; arcs (happy, asleep) stay.
pub fn face(expr: Expr, blink: bool) -> Face {
    let happy = vec![Part::Stroke(Line::Happy)];
    let still = Ears::default();
    let (eyes, extra, ears, bob, tilt): (Vec<Part>, Vec<Part>, Ears, f32, f32) = match expr {
        Expr::Idle => (open(10.0, 5.6, 0.0).to_vec(), vec![], still, 0.0, 0.0),
        Expr::Happy => (happy, vec![], still, 0.0, 0.0),
        Expr::Focus => (open(4.0, 6.0, 1.0).to_vec(), vec![], still, 0.0, 0.0),
        Expr::Music => (happy, note(57.0, 13.0).to_vec(), still, 0.0, 0.0),
        Expr::Sing => {
            let mut notes = note(57.0, 13.0).to_vec();
            notes.extend(note(64.0, 3.0));
            (happy, notes, still, 0.0, 0.0)
        }
        Expr::Alert => {
            let eyes = vec![dot(L, Y, 4.6, Ink::Espresso), dot(R, Y, 4.6, Ink::Espresso)];
            let ears = Ears {
                up: 4.0,
                droop: 0.0,
            };
            (eyes, vec![], ears, -1.5, 0.0)
        }
        Expr::Think => (
            // Looking up and to the right.
            vec![
                pill(L + 2.0, Y - 2.5, 5.6, 9.0),
                pill(R + 2.0, Y - 2.5, 5.6, 9.0),
            ],
            vec![
                dot(58.0, 6.0, 1.4, Ink::Cream),
                dot(62.0, 3.0, 1.8, Ink::Cream),
                dot(66.0, 0.0, 2.2, Ink::Cream),
            ],
            still,
            0.0,
            0.0,
        ),
        Expr::Sleep => (
            vec![Part::Stroke(Line::Sleep)],
            vec![Part::Stroke(Line::Zz)],
            Ears {
                up: 0.0,
                droop: 4.0,
            },
            1.5,
            0.0,
        ),
        Expr::Sad => (
            open(5.0, 5.6, 2.5).to_vec(),
            vec![],
            Ears {
                up: 0.0,
                droop: 8.0,
            },
            1.5,
            0.0,
        ),
        Expr::Wave => (
            happy,
            vec![Part::Heart { x: 4.0, y: 1.0 }],
            still,
            0.0,
            -7.0,
        ),
        // The idle pills, shifted (below) toward the laptop on the left.
        Expr::Typing => (
            open(10.0, 5.6, 0.0).to_vec(),
            vec![],
            still,
            0.0,
            TYPING_TILT,
        ),
    };
    let eyes = if blink && expr.blinks() {
        vec![Part::Stroke(Line::Blink)]
    } else {
        eyes
    };
    let mut parts = eyes;
    parts.extend(extra);
    Face {
        parts,
        ears,
        bob,
        tilt,
        eye_shift: if expr == Expr::Typing {
            TYPING_EYE_SHIFT
        } else {
            (0.0, 0.0)
        },
    }
}

/// The grid box `(left, top, right, bottom)` a face mark is drawn in, stroke
/// width included (a curve's control points bound it). For the tests that
/// keep the cat clear of what is beside it.
#[cfg(test)]
pub fn mark_bounds(part: &Part) -> (f32, f32, f32, f32) {
    let points = |segs: Vec<Seg>, pad: f32| {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let mut add = |x: f32, y: f32| {
            b = (
                b.0.min(x - pad),
                b.1.min(y - pad),
                b.2.max(x + pad),
                b.3.max(y + pad),
            );
        };
        for seg in segs {
            match seg {
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
        b
    };
    match *part {
        Part::Pill { x, y, w, h, .. } => (x, y, x + w, y + h),
        Part::Dot { cx, cy, r, .. } => (cx - r, cy - r, cx + r, cy + r),
        Part::Stroke(line) => points(line.segments(), line.width() / 2.0),
        Part::Heart { x, y } => points(heart(x, y), 0.0),
    }
}

/// The idle face's two open eyes: the pills the shrunk notch shows alone.
pub fn eye_pills() -> Vec<Part> {
    face(Expr::Idle, false)
        .parts
        .into_iter()
        .filter(|part| matches!(part, Part::Pill { .. }))
        .collect()
}

/// How many dip one grid unit is when the eyes alone are drawn in the shrunk
/// notch: the 10-unit eyes become 8 dip tall in its 14 dip.
const EYES_SCALE: f32 = 0.8;

/// The square `paint_eyes` takes so that the eyes are centred on `centre`
/// (the middle of the line between them, grid point (32, 40)).
pub fn eyes_area(centre: (f32, f32)) -> Rect {
    Rect {
        x: centre.0 - 32.0 * EYES_SCALE,
        y: centre.1 - Y * EYES_SCALE,
        w: GRID * EYES_SCALE,
        h: GRID * EYES_SCALE,
    }
}

// ---- The shrunk notch's eyes: where they look and when they blink ----

/// What the shrunk notch's eyes do this frame: slid `dx_dip` across and
/// `dy_dip` down from the centre (the look), or shut (a blink). The default
/// is centred and open.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EyesPose {
    pub dx_dip: f32,
    pub dy_dip: f32,
    pub blink: bool,
}

/// The grid boxes `(left, top, right, bottom)` of the eyes' marks: the two
/// open pills, or the two blink lines with their round caps (stroke width
/// included).
fn eyes_grid_boxes(blink: bool) -> Vec<(f32, f32, f32, f32)> {
    if !blink {
        return eye_pills()
            .into_iter()
            .filter_map(|part| match part {
                Part::Pill { x, y, w, h, .. } => Some((x, y, x + w, y + h)),
                _ => None,
            })
            .collect();
    }
    let line = Line::Blink;
    let pad = line.width() / 2.0;
    let mut boxes: Vec<(f32, f32, f32, f32)> = Vec::new();
    for seg in line.segments() {
        match seg {
            Seg::Move(x, y) => boxes.push((x - pad, y - pad, x + pad, y + pad)),
            Seg::Line(x, y) => {
                if let Some(b) = boxes.last_mut() {
                    *b = (
                        b.0.min(x - pad),
                        b.1.min(y - pad),
                        b.2.max(x + pad),
                        b.3.max(y + pad),
                    );
                }
            }
            _ => {}
        }
    }
    boxes
}

/// The boxes of both eyes in dip, as `paint_eyes` draws them when centred on
/// `centre` and slid by `(dx, dy)`.
pub fn eyes_boxes_at(
    centre: (f32, f32),
    dx: f32,
    dy: f32,
    blink: bool,
) -> Vec<(f32, f32, f32, f32)> {
    let at = |x: f32, y: f32| {
        (
            centre.0 + (x - 32.0) * EYES_SCALE + dx,
            centre.1 + (y - Y) * EYES_SCALE + dy,
        )
    };
    eyes_grid_boxes(blink)
        .into_iter()
        .map(|(l, t, r, b)| {
            let (left, top) = at(l, t);
            let (right, bottom) = at(r, b);
            (left, top, right, bottom)
        })
        .collect()
}

/// Where the shrunk notch's eyes sit in `shape` (a pill, or the morph toward
/// it): the middle of the line between them, as `paint_eyes_pill` anchors it.
pub fn eyes_centre(shape: ShapeRect) -> (f32, f32) {
    (shape.x + shape.w / 2.0, shape.y + EYES_H / 2.0)
}

/// The slide `(dx, dy)` of the eyes in dip, cut back so both eyes stay inside
/// `shape` (the pill, or any shape of the morph to it): open or shut, so a
/// blink never moves them. The shapes' corners are tested on the main outline
/// (the Notch's shoulders are outside it, so this is the stricter side). At the
/// final pill the look's own extremes fit, so this is a guard that the morph
/// and odd sizes never trip over, not a visible limit.
pub fn eyes_offset_in(style: Style, shape: ShapeRect, dx: f32, dy: f32) -> (f32, f32) {
    let centre = eyes_centre(shape);
    let fits = |ox: f32, oy: f32| {
        [false, true].into_iter().all(|blink| {
            eyes_boxes_at(centre, ox, oy, blink)
                .into_iter()
                .all(|(l, t, r, b)| {
                    [(l, t), (r, t), (l, b), (r, b)]
                        .into_iter()
                        .all(|(x, y)| shape_contains(style, shape, x, y))
                })
        })
    };
    // The set of slides that fit is convex, so along a line from the middle
    // it is an interval: bisect for its end.
    let reach = |want: f32, fits: &dyn Fn(f32) -> bool| {
        if want.is_nan() || want == 0.0 || !fits(0.0) {
            return 0.0;
        }
        if fits(want) {
            return want;
        }
        let (mut ok, mut bad) = (0.0f32, want);
        for _ in 0..24 {
            let mid = (ok + bad) / 2.0;
            if fits(mid) {
                ok = mid;
            } else {
                bad = mid;
            }
        }
        ok
    };
    let y = reach(dy, &|v| fits(0.0, v));
    let x = reach(dx, &|v| fits(v, y));
    (x, y)
}

pub const EYES_BLINK_MIN_MS: u32 = 3_000;
pub const EYES_BLINK_MAX_MS: u32 = 6_000;
/// A double blink: its second shut starts this long after the first one starts.
pub const EYES_DOUBLE_MS: u32 = 260;

/// The shrunk notch's blink schedule: eyes open for 3 to 6 s, shut for
/// `BLINK_MS`, and one blink in four is a double (shut, open 140 ms, shut
/// again). `next` is called each time `TIMER_BLINK` fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EyesBlink {
    /// Open, waiting for the next blink.
    #[default]
    Open,
    /// Shut; a second shut follows when `second_follows`.
    Shut { second_follows: bool },
    /// Open between the two shuts of a double blink.
    Gap,
}

impl EyesBlink {
    /// The wait while open: 3 to 6 s. The first one is what arms the timer.
    pub fn open_wait(rng: &mut Rng) -> u32 {
        EYES_BLINK_MIN_MS + rng.next_u32() % (EYES_BLINK_MAX_MS - EYES_BLINK_MIN_MS + 1)
    }

    /// The eyes are shut now.
    pub fn shut(&self) -> bool {
        matches!(self, EyesBlink::Shut { .. })
    }

    /// The timer fired: the next step, and the milliseconds until the one
    /// after it.
    pub fn next(&mut self, rng: &mut Rng) -> u32 {
        match *self {
            EyesBlink::Open => {
                *self = EyesBlink::Shut {
                    second_follows: rng.next_u32() % 4 == 0,
                };
                BLINK_MS
            }
            EyesBlink::Shut {
                second_follows: true,
            } => {
                *self = EyesBlink::Gap;
                EYES_DOUBLE_MS - BLINK_MS
            }
            EyesBlink::Gap => {
                *self = EyesBlink::Shut {
                    second_follows: false,
                };
                BLINK_MS
            }
            EyesBlink::Shut {
                second_follows: false,
            } => {
                *self = EyesBlink::Open;
                Self::open_wait(rng)
            }
        }
    }
}

/// Head: x 7 y 24, 50 × 30, a dome on top (radius 17) and a flat base.
pub const HEAD: (f32, f32, f32, f32, f32) = (7.0, 24.0, 50.0, 30.0, 17.0);
/// How far an ear's stroke (width 5, round joins) reaches past its triangle.
pub const EAR_STROKE: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// The point an ear turns around when it droops.
    pub(super) fn pivot(self) -> Vector2 {
        match self {
            Side::Left => Vector2::new(17.0, 30.0),
            Side::Right => Vector2::new(47.0, 30.0),
        }
    }

    /// Degrees (clockwise) an ear turns for `droop`: both turn outward.
    pub(super) fn turn(self, droop: f32) -> f32 {
        match self {
            Side::Left => -droop * 3.0,
            Side::Right => droop * 3.0,
        }
    }
}

/// An ear's triangle before its rounded stroke: outer base, tip (raised by
/// `up`), inner base.
pub fn ear(side: Side, up: f32) -> [(f32, f32); 3] {
    match side {
        Side::Left => [(9.0, 37.0), (12.0, 10.0 - up), (27.0, 26.0)],
        Side::Right => [(55.0, 37.0), (52.0, 10.0 - up), (37.0, 26.0)],
    }
}

/// `ear(side, ears.up)` turned for `ears.droop` as `RobotPainter::paint`
/// draws it, in grid units. For the tests that keep the cat clear of what
/// is beside it.
#[cfg(test)]
pub fn ear_drawn(side: Side, ears: Ears) -> [(f32, f32); 3] {
    let p = side.pivot();
    let a = side.turn(ears.droop).to_radians();
    ear(side, ears.up).map(|(x, y)| {
        let (dx, dy) = (x - p.X, y - p.Y);
        (
            p.X + dx * a.cos() - dy * a.sin(),
            p.Y + dx * a.sin() + dy * a.cos(),
        )
    })
}

/// The head's rounded rect for a cat drawn in `area` (a square), in px:
/// what must stay inside the island. Ears and extras may draw outside it.
pub fn head_rect(area: Rect) -> (Rect, f32) {
    let k = area.w / GRID;
    let (x, y, w, h, r) = HEAD;
    (
        Rect {
            x: area.x + x * k,
            y: area.y + y * k,
            w: w * k,
            h: h * k,
        },
        r * k,
    )
}

/// The head: a dome with a flat base and small bottom corners.
fn head_path() -> Vec<Seg> {
    use Seg::{Close, Line as To, Move, Quad};
    vec![
        Move(7.0, 50.0),
        To(7.0, 41.0),
        Quad(7.0, 24.0, 32.0, 24.0),
        Quad(57.0, 24.0, 57.0, 41.0),
        To(57.0, 50.0),
        Quad(57.0, 54.0, 53.0, 54.0),
        To(11.0, 54.0),
        Quad(7.0, 54.0, 7.0, 50.0),
        Close,
    ]
}

/// The heart: `M0 3C-4-1-5.5 2.5-3 5L0 8L3 5C5.5 2.5 4-1 0 3Z`, moved to (x, y).
fn heart(x: f32, y: f32) -> Vec<Seg> {
    use Seg::{Close, Cubic, Line as To, Move};
    vec![
        Move(x, y + 3.0),
        Cubic(x - 4.0, y - 1.0, x - 5.5, y + 2.5, x - 3.0, y + 5.0),
        To(x, y + 8.0),
        To(x + 3.0, y + 5.0),
        Cubic(x + 5.5, y + 2.5, x + 4.0, y - 1.0, x, y + 3.0),
        Close,
    ]
}

// ---- Drawing ----

/// A path the painter builds once and keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Shape {
    Line(Line),
    Ear(Side, f32),
    Head,
    Heart(f32, f32),
    /// The typing cat's laptop lid and the light along its edge.
    Lid,
    LidEdge,
}

impl Shape {
    fn segments(self) -> Vec<Seg> {
        match self {
            Shape::Line(line) => line.segments(),
            Shape::Ear(side, up) => {
                let [a, b, c] = ear(side, up);
                vec![
                    Seg::Move(a.0, a.1),
                    Seg::Line(b.0, b.1),
                    Seg::Line(c.0, c.1),
                    Seg::Close,
                ]
            }
            Shape::Head => head_path(),
            Shape::Heart(x, y) => heart(x, y),
            Shape::Lid => polygon(&LID),
            Shape::LidEdge => polygon(&LID_EDGE),
        }
    }
}

/// A closed polygon through `points`.
fn polygon(points: &[(f32, f32)]) -> Vec<Seg> {
    let mut segs = Vec::with_capacity(points.len() + 1);
    for (i, &(x, y)) in points.iter().enumerate() {
        segs.push(if i == 0 {
            Seg::Move(x, y)
        } else {
            Seg::Line(x, y)
        });
    }
    segs.push(Seg::Close);
    segs
}

/// The cat's paths in grid units, built on first use and kept. The render
/// target's transform scales them (and their widths) to any size.
pub struct RobotPainter {
    pub(super) factory: ID2D1Factory,
    paths: RefCell<Vec<(Shape, ID2D1PathGeometry)>>,
    /// The calico patches (orange, dark) per ear state, built from the ears
    /// and the head (`cat_style`).
    pub(super) calico: RefCell<Vec<((u32, u32), [ID2D1PathGeometry; 2])>>,
    /// Each accessory's pieces and their paths (`cat_style`).
    pub(super) extras: RefCell<Vec<((Mascot, CatAccessory), Rc<Vec<Placed>>)>>,
    /// The falk's and the octopus's still paths (`mascot`).
    pub(super) mascot_paths: RefCell<Vec<(MascotShape, ID2D1Geometry)>>,
    pub(super) round: ID2D1StrokeStyle,
}

impl RobotPainter {
    pub fn new(factory: &ID2D1Factory) -> WinResult<Self> {
        let round = unsafe {
            factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?
        };
        Ok(Self {
            factory: factory.clone(),
            paths: RefCell::new(Vec::new()),
            calico: RefCell::new(Vec::new()),
            extras: RefCell::new(Vec::new()),
            mascot_paths: RefCell::new(Vec::new()),
            round,
        })
    }

    /// `shape`'s path, built the first time it is asked for.
    pub(super) fn path(&self, shape: Shape) -> Option<ID2D1PathGeometry> {
        if let Some((_, g)) = self.paths.borrow().iter().find(|(s, _)| *s == shape) {
            return Some(g.clone());
        }
        let g = path(&self.factory, &shape.segments()).ok()?;
        self.paths.borrow_mut().push((shape, g.clone()));
        Some(g)
    }

    /// Draws the mascot filling the square `area` (its 64-unit grid scaled to
    /// `area.w`), raised by the pose's lift, bounce and hop, at `alpha`. The
    /// render target's transform is restored; the brush colour is not.
    ///
    /// `look` is the mascot, its coat and accessory. The cat's order is one
    /// silhouette (see `cat_style`): the Black cat's rim, the ears and head in
    /// one colour, the calico patches, the face marks, the accessory. The
    /// falk and the octopus paint their own body in place of the first three
    /// (`mascot`). A caller that fades the mascot does it with a layer round
    /// this call, as the renderer does, so the passes do not show through
    /// one another.
    pub fn paint(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        area: Rect,
        expr: Expr,
        pose: Pose,
        alpha: f32,
        look: CatLook,
    ) {
        if area.w <= 0.0 {
            return;
        }
        let face = face(expr, pose.blink);
        let k = area.w / GRID;
        // The look turns and slides the whole cat; its marks slide a little
        // further. The dance's squash comes first (in the grid, round the
        // head's base), its nod goes with the placement (in dip).
        let (ox, oy) = SQUASH_ORIGIN;
        let (sx, sy) = (1.0 + SQUASH_W * pose.squash, 1.0 - SQUASH_H * pose.squash);
        let squash = Matrix3x2 {
            M11: sx,
            M12: 0.0,
            M21: 0.0,
            M22: sy,
            M31: ox * (1.0 - sx),
            M32: oy * (1.0 - sy),
        };
        let shape = squash
            * Matrix3x2::rotation_around(face.tilt + pose.tilt_deg, Vector2::new(32.0, 40.0))
            * Matrix3x2::translation(0.0, face.bob)
            * Matrix3x2::scale(k, k);
        let place = Matrix3x2::translation(
            area.x + pose.head_dx_dip,
            area.y - pose.lift_px - pose.bounce_px - pose.hop_px + pose.nod_dip,
        );
        let local = shape * place;
        let color = |rgb: u32, a: f32| {
            let ch = |s: u32| ((rgb >> s) & 0xFF) as f32 / 255.0;
            D2D1_COLOR_F {
                r: ch(16),
                g: ch(8),
                b: ch(0),
                a: a * alpha,
            }
        };
        let rounded = |(x, y, w, h, r): (f32, f32, f32, f32, f32)| D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            radiusX: r,
            radiusY: r,
        };
        let circle = |cx: f32, cy: f32, r: f32| D2D1_ELLIPSE {
            point: Vector2::new(cx, cy),
            radiusX: r,
            radiusY: r,
        };
        unsafe {
            let mut previous = Matrix3x2::default();
            rt.GetTransform(&mut previous);
            let world = local * previous;
            // The typing cat's laptop and paws sit in the cat's grid and are
            // placed like it, but the face's tilt does not turn them.
            let typing = expr == Expr::Typing;
            let fixed = Matrix3x2::scale(k, k) * place * previous;
            // 0. The laptop, under the cat.
            if typing {
                self.paint_laptop(rt, brush, &fixed, &color);
            }
            // The face's own shift (the typing cat looks down at its laptop)
            // and the glance move the face marks in the grid, before the
            // tilt. The eyes (and what sits on them) also rise by the
            // mascot's eye lift; the cream extras beside the head do not.
            let slide = pose.eye_dx_dip / k + face.eye_shift.0 + pose.glance_x;
            let marks = Matrix3x2::translation(slide, face.eye_shift.1 + pose.glance_y) * world;
            let eye_lift = anchors(look.mascot).eye_lift;
            let eyes = if eye_lift == 0.0 {
                marks
            } else {
                Matrix3x2::translation(slide, face.eye_shift.1 + pose.glance_y + eye_lift) * world
            };
            match look.mascot {
                Mascot::Cat => {
                    // 1. The Black cat's rim, under everything: the ears
                    // stroked wider than their own stroke and the head
                    // stroked wider than its outline, so it reaches
                    // `RIM_WIDTH` past the silhouette.
                    if let Some(rim) = look.rim() {
                        brush.SetColor(&color(rim, 1.0));
                        for side in [Side::Left, Side::Right] {
                            if let Some(g) = self.path(Shape::Ear(side, face.ears.up)) {
                                let turn = Matrix3x2::rotation_around(
                                    side.turn(face.ears.droop),
                                    side.pivot(),
                                );
                                rt.SetTransform(&(turn * world));
                                rt.DrawGeometry(
                                    &g,
                                    brush,
                                    EAR_STROKE + 2.0 * RIM_WIDTH,
                                    &self.round,
                                );
                            }
                        }
                        rt.SetTransform(&world);
                        if let Some(g) = self.path(Shape::Head) {
                            rt.DrawGeometry(&g, brush, 2.0 * RIM_WIDTH, &self.round);
                        }
                    }
                    // 2. The silhouette in one colour: ears behind the head,
                    // a triangle with a fat round stroke, then the head.
                    brush.SetColor(&color(look.head_rgb(), 1.0));
                    for side in [Side::Left, Side::Right] {
                        if let Some(g) = self.path(Shape::Ear(side, face.ears.up)) {
                            let turn =
                                Matrix3x2::rotation_around(side.turn(face.ears.droop), side.pivot());
                            rt.SetTransform(&(turn * world));
                            rt.FillGeometry(&g, brush, None);
                            rt.DrawGeometry(&g, brush, EAR_STROKE, &self.round);
                        }
                    }
                    rt.SetTransform(&world);
                    if let Some(g) = self.path(Shape::Head) {
                        rt.FillGeometry(&g, brush, None);
                    }
                    // 3. Calico: each patch is its ear minus the head, united
                    // with its circle clipped to the head.
                    if look.color == CatColor::Calico {
                        self.paint_patches(rt, brush, &world, face.ears, alpha);
                    }
                }
                Mascot::Falk => {
                    // The cheeks slide half as far as the eyes, so they stay
                    // on the head.
                    let cheeks = Matrix3x2::translation(
                        (pose.eye_dx_dip / k + pose.glance_x) / 2.0,
                        0.0,
                    ) * world;
                    self.paint_falk_body(rt, brush, look.falk, expr, &pose, (&world, &cheeks), alpha);
                }
                Mascot::Octopus => {
                    self.paint_octopus_body(rt, brush, look.octopus, expr, &pose, &world, alpha);
                }
            }
            // 4. The face marks.
            // The dancing note's marks move on their own, in dip, and fade.
            let notes =
                Matrix3x2::translation(pose.eye_dx_dip / k + face.eye_shift.0, face.eye_shift.1)
                    * shape
                    * Matrix3x2::translation(0.0, pose.note_dy_dip)
                    * place
                    * previous;
            let note_alpha = 1.0 - pose.note_fade.clamp(0.0, 1.0);
            for part in &face.parts {
                let note = is_note(part);
                rt.SetTransform(if note {
                    &notes
                } else if is_eye(part) {
                    &eyes
                } else {
                    &marks
                });
                let fade = if note { note_alpha } else { 1.0 };
                match *part {
                    Part::Pill { x, y, w, h, rx, ry } => {
                        brush.SetColor(&color(look.eye_rgb(), 1.0));
                        rt.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                radiusX: rx,
                                radiusY: ry,
                                ..rounded((x, y, w, h, 0.0))
                            },
                            brush,
                        );
                    }
                    Part::Dot { cx, cy, r, ink } => {
                        brush.SetColor(&color(look.ink_rgb(ink), fade));
                        rt.FillEllipse(&circle(cx, cy, r), brush);
                    }
                    Part::Stroke(line) => {
                        if let Some(g) = self.path(Shape::Line(line)) {
                            brush.SetColor(&color(look.ink_rgb(line.ink()), line.alpha() * fade));
                            rt.DrawGeometry(&g, brush, line.width(), &self.round);
                        }
                    }
                    Part::Heart { x, y } => {
                        if let Some(g) = self.path(Shape::Heart(x, y)) {
                            brush.SetColor(&color(CREAM, 1.0));
                            rt.FillGeometry(&g, brush, None);
                        }
                    }
                }
            }
            // The falk's beak is a face mark too.
            if look.mascot == Mascot::Falk {
                self.paint_falk_beak(rt, brush, expr, &pose, &eyes, alpha);
            }
            // 5. The accessory, on top of everything.
            self.paint_accessory(rt, brush, look, face.ears, (&world, &eyes), alpha);
            // 6. The typing cat's paws, over the laptop and the head; the
            // falk's feather tips; the octopus types with its arms.
            if typing && look.mascot == Mascot::Falk {
                self.paint_falk_tips(rt, brush, look.falk, pose.paw_lift, &fixed, alpha);
            }
            if typing && look.mascot == Mascot::Cat {
                rt.SetTransform(&fixed);
                for (i, &(cx, cy)) in PAWS.iter().enumerate() {
                    let ellipse = D2D1_ELLIPSE {
                        point: Vector2::new(cx, cy - pose.paw_lift[i]),
                        radiusX: PAW_RX,
                        radiusY: PAW_RY,
                    };
                    brush.SetColor(&color(look.head_rgb(), 1.0));
                    rt.FillEllipse(&ellipse, brush);
                    // Only the Black coat is outlined, in its rim colour.
                    if let Some(rim) = look.rim() {
                        brush.SetColor(&color(rim, 1.0));
                        rt.DrawEllipse(&ellipse, brush, PAW_OUTLINE, None);
                    }
                }
            }
            rt.SetTransform(&previous);
        }
    }

    /// The laptop, in the cat's grid: base, lid, the screen's lit edge and
    /// the logo dot, in fixed colours (not the coat's). `fixed` is the grid's
    /// transform (scaled, placed, not tilted).
    fn paint_laptop(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        fixed: &Matrix3x2,
        color: &dyn Fn(u32, f32) -> D2D1_COLOR_F,
    ) {
        unsafe {
            rt.SetTransform(fixed);
            let (x, y, w, h, r) = LAPTOP_BASE;
            brush.SetColor(&color(LAPTOP_BASE_RGB, 1.0));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    },
                    radiusX: r,
                    radiusY: r,
                },
                brush,
            );
            if let Some(g) = self.path(Shape::Lid) {
                brush.SetColor(&color(LAPTOP_LID_RGB, 1.0));
                rt.FillGeometry(&g, brush, None);
            }
            if let Some(g) = self.path(Shape::LidEdge) {
                brush.SetColor(&color(LAPTOP_EDGE_RGB, LAPTOP_EDGE_ALPHA));
                rt.FillGeometry(&g, brush, None);
            }
            brush.SetColor(&color(CREAM, LAPTOP_LOGO_ALPHA));
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2::new(LAPTOP_LOGO.0, LAPTOP_LOGO.1),
                    radiusX: LAPTOP_LOGO.2,
                    radiusY: LAPTOP_LOGO.2,
                },
                brush,
            );
        }
    }
}

impl RobotPainter {
    /// Only the two open eyes of the idle face, in the brand terracotta (the
    /// espresso of the cat's own eyes would vanish on the dark notch), over a
    /// square `area` as `eyes_area` gives; with `blink`, the cat's two short
    /// level lines instead, in the same colour. The render target's transform
    /// is restored; the brush colour is not.
    pub fn paint_eyes(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        area: Rect,
        alpha: f32,
        blink: bool,
    ) {
        if area.w <= 0.0 {
            return;
        }
        let k = area.w / GRID;
        let ch = |s: u32| ((TERRACOTTA >> s) & 0xFF) as f32 / 255.0;
        unsafe {
            let mut previous = Matrix3x2::default();
            rt.GetTransform(&mut previous);
            let local = Matrix3x2::scale(k, k) * Matrix3x2::translation(area.x, area.y);
            rt.SetTransform(&(local * previous));
            brush.SetColor(&D2D1_COLOR_F {
                r: ch(16),
                g: ch(8),
                b: ch(0),
                a: alpha,
            });
            if blink {
                let line = Line::Blink;
                if let Some(g) = self.path(Shape::Line(line)) {
                    rt.DrawGeometry(&g, brush, line.width(), &self.round);
                }
            }
            for part in eye_pills().into_iter().filter(|_| !blink) {
                if let Part::Pill { x, y, w, h, rx, ry } = part {
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: x,
                                top: y,
                                right: x + w,
                                bottom: y + h,
                            },
                            radiusX: rx,
                            radiusY: ry,
                        },
                        brush,
                    );
                }
            }
            rt.SetTransform(&previous);
        }
    }
}

/// Figures from `segs`: each `Move` starts one, `Close` closes it; figures
/// left open are drawn as lines and never filled.
pub(super) fn path(factory: &ID2D1Factory, segs: &[Seg]) -> WinResult<ID2D1PathGeometry> {
    unsafe {
        let path = factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        let mut open = false;
        for seg in segs {
            match *seg {
                Seg::Move(x, y) => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(Vector2::new(x, y), D2D1_FIGURE_BEGIN_FILLED);
                    open = true;
                }
                Seg::Line(x, y) => sink.AddLine(Vector2::new(x, y)),
                Seg::Quad(cx, cy, x, y) => {
                    sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
                        point1: Vector2::new(cx, cy),
                        point2: Vector2::new(x, y),
                    })
                }
                Seg::Cubic(ax, ay, bx, by, x, y) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: Vector2::new(ax, ay),
                    point2: Vector2::new(bx, by),
                    point3: Vector2::new(x, y),
                }),
                Seg::Close => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        open = false;
                    }
                }
            }
        }
        if open {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close()?;
        Ok(path)
    }
}

// ---- Which face ----

/// What a notice is, as far as the robot cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeSort {
    /// Another app's notification (or the island's own messages, which use
    /// the same kind): keeps its badge, no robot.
    App,
    Reminder,
    PomodoroDone,
    ClaudeWaiting,
    ClaudeDone,
    BatteryLow,
    /// A drive low on space or RAM high for a minute.
    SystemLow,
    /// The cleaner finished.
    CleanDone,
    /// A Mole clean finished.
    MoleDone,
    /// A Mole clean failed or stopped.
    MoleFailed,
    Greeting(DayPeriod),
    /// "Disalin" after a copy.
    Copied,
    /// The preview of a copy made elsewhere (text or files): Bata happy.
    ClipPreview,
    /// The same for an image: its thumbnail takes the badge spot, no robot.
    ClipImage,
    /// The screenshot preview.
    Screenshot,
    /// "Disimpan" after its Simpan.
    ShotSaved,
    /// "Rekaman tersimpan".
    RecSaved,
    /// A recording failed or the mic was refused.
    RecFailed,
    /// A word about a recording (consent, a lost source).
    RecInfo,
    /// "Dihapus" after its Hapus.
    ShotDeleted,
    /// A drive was plugged in.
    DevicePlugged,
    /// A device is safe to remove.
    DeviceSafe,
    /// Susun arranged the windows.
    Arranged,
    /// "Vale X.Y.Z tersedia".
    UpdateAvailable,
    /// The download's percent.
    UpdateProgress,
    /// "Update gagal".
    UpdateFailed,
    /// Up to date, updated, off in debug builds.
    UpdateInfo,
}

impl NoticeSort {
    pub fn of(kind: &NoticeKind) -> Self {
        match kind {
            NoticeKind::App { .. } => NoticeSort::App,
            // A sticky note's reminder looks like a reminder.
            NoticeKind::Reminder { .. } | NoticeKind::StickyReminder { .. } => NoticeSort::Reminder,
            NoticeKind::Pomodoro => NoticeSort::PomodoroDone,
            NoticeKind::ClaudeWaiting { .. } => NoticeSort::ClaudeWaiting,
            NoticeKind::ClaudeDone { .. } => NoticeSort::ClaudeDone,
            NoticeKind::BatteryLow => NoticeSort::BatteryLow,
            NoticeKind::SystemLow => NoticeSort::SystemLow,
            NoticeKind::CleanDone => NoticeSort::CleanDone,
            NoticeKind::MoleDone => NoticeSort::MoleDone,
            NoticeKind::MoleFailed => NoticeSort::MoleFailed,
            NoticeKind::Greeting { period } => NoticeSort::Greeting(*period),
            NoticeKind::Copied => NoticeSort::Copied,
            NoticeKind::ClipPreview { .. } => NoticeSort::ClipPreview,
            NoticeKind::Screenshot { .. } => NoticeSort::Screenshot,
            NoticeKind::ShotSaved { .. } => NoticeSort::ShotSaved,
            NoticeKind::RecSaved { .. } => NoticeSort::RecSaved,
            NoticeKind::RecFailed { .. }
            | NoticeKind::RecMicDenied
            | NoticeKind::RecFolderBlocked { .. } => NoticeSort::RecFailed,
            NoticeKind::RecInfo => NoticeSort::RecInfo,
            // A transcript notice looks like its recording sibling: ready
            // happy, failed sad, a word at rest.
            NoticeKind::TranscriptReady { .. } => NoticeSort::RecSaved,
            NoticeKind::TranscriptFailed { .. } => NoticeSort::RecFailed,
            NoticeKind::TranscriptInfo
            | NoticeKind::TranscriptStarting { .. }
            | NoticeKind::TranscriptSettings => NoticeSort::RecInfo,
            NoticeKind::ShotDeleted => NoticeSort::ShotDeleted,
            NoticeKind::DevicePlugged { .. } => NoticeSort::DevicePlugged,
            NoticeKind::DeviceSafe => NoticeSort::DeviceSafe,
            NoticeKind::Arranged { .. } => NoticeSort::Arranged,
            NoticeKind::UpdateAvailable { .. } => NoticeSort::UpdateAvailable,
            NoticeKind::UpdateProgress { .. } => NoticeSort::UpdateProgress,
            NoticeKind::UpdateFailed => NoticeSort::UpdateFailed,
            NoticeKind::UpdateInfo | NoticeKind::WhatsNew => NoticeSort::UpdateInfo,
        }
    }
}

/// Where the island is, as far as the robot cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    /// Fullscreen: nothing shows.
    Hidden,
    /// The open panel: never a robot.
    Panel,
    Idle,
    Pomodoro,
    Media,
    Lyric,
    Notice(NoticeSort),
}

impl Spot {
    pub fn of(view: &View) -> Self {
        match view {
            View::Hidden => Spot::Hidden,
            View::Panel(_) => Spot::Panel,
            View::Idle => Spot::Idle,
            // The REC view wears the pomodoro's focused face, and so does
            // the countdown that leads to it and the caption line (it is
            // listening).
            View::PomodoroCompact
            | View::RecordCompact
            | View::CaptionCompact
            | View::ScreenCountdown(_) => {
                Spot::Pomodoro
            }
            View::MediaCompact => Spot::Media,
            View::MediaLyric => Spot::Lyric,
            // A copied image shows its own thumbnail where Bata would be.
            View::Notice(n)
                if matches!(n.kind, NoticeKind::ClipPreview { .. })
                    && n.key == crate::model::CLIP_IMAGE_KEY =>
            {
                Spot::Notice(NoticeSort::ClipImage)
            }
            View::Notice(n) => Spot::Notice(NoticeSort::of(&n.kind)),
        }
    }

    /// A compact island: the pointer over it turns the face Happy and lifts
    /// it. Notices keep their face, the panel and hidden show no robot.
    pub fn compact(self) -> bool {
        matches!(
            self,
            Spot::Idle | Spot::Pomodoro | Spot::Media | Spot::Lyric
        )
    }
}

/// The CLI's last word, for the idle face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cli {
    #[default]
    None,
    Running,
    /// Ended with exit 0, a few seconds ago.
    Done,
    /// Ended otherwise, a few seconds ago.
    Failed,
}

/// Everything the face depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RobotInputs {
    pub spot: Spot,
    /// The pointer is over the island (before the panel opens).
    pub hover: bool,
    /// No activity for the sleep time.
    pub asleep: bool,
    /// A Claude Code session is busy.
    pub claude_working: bool,
    pub cli: Cli,
    /// A Mole clean runs (the cleaner's "Otomatis · Mole" mode).
    pub mole_running: bool,
    /// Below the battery threshold and discharging.
    pub battery_low: bool,
}

/// The face for `i`, or `None` where no robot shows. Priority, highest first:
///
/// 1. Hidden and the open panel: no robot.
/// 2. A notice decides by its kind: reminder and Claude waiting → Alert,
///    pomodoro phase done, Claude done, a finished clean (own or Mole) and
///    "Disalin" → Happy, low battery, low disk or high RAM and a failed Mole
///    clean → Sad, greeting → its period's face. Other apps' notifications (`App`) → no
///    robot: they keep their app badge.
/// 3. The pointer over a compact island → Happy (it also wakes it).
/// 4. Pomodoro → Focus, media → Music, lyric line → Sing.
/// 5. Idle: CLI just done → Happy, just failed → Sad; Claude working, a
///    CLI command or a Mole clean running → Typing (never Think, which only
///    the demo sheet and the Claude notice scene still draw); low battery →
///    Sad; asleep → Sleep; otherwise Idle.
pub fn choose_expr(i: &RobotInputs) -> Option<Expr> {
    let compact = match i.spot {
        Spot::Hidden | Spot::Panel => return None,
        Spot::Notice(sort) => {
            return match sort {
                NoticeSort::App | NoticeSort::ClipImage => None,
                NoticeSort::Reminder | NoticeSort::ClaudeWaiting => Some(Expr::Alert),
                NoticeSort::PomodoroDone
                | NoticeSort::ClaudeDone
                | NoticeSort::CleanDone
                | NoticeSort::MoleDone
                | NoticeSort::Copied
                | NoticeSort::ClipPreview
                | NoticeSort::Screenshot
                | NoticeSort::DeviceSafe
                | NoticeSort::Arranged
                | NoticeSort::UpdateInfo
                | NoticeSort::RecSaved
                | NoticeSort::ShotSaved => Some(Expr::Happy),
                // keputusan-v3.1 p6: "Dihapus" with Bata at rest; a drive
                // plugged in is news, not an event: Bata at rest as well.
                // An update on offer or on its way is the same kind of news.
                NoticeSort::ShotDeleted
                | NoticeSort::DevicePlugged
                | NoticeSort::UpdateAvailable
                | NoticeSort::RecInfo
                | NoticeSort::UpdateProgress => Some(Expr::Idle),
                NoticeSort::BatteryLow
                | NoticeSort::SystemLow
                | NoticeSort::MoleFailed
                | NoticeSort::RecFailed
                | NoticeSort::UpdateFailed => Some(Expr::Sad),
                NoticeSort::Greeting(period) => Some(greeting::face(period)),
            };
        }
        compact => compact,
    };
    if i.hover {
        return Some(Expr::Happy);
    }
    Some(match compact {
        Spot::Pomodoro => Expr::Focus,
        Spot::Media => Expr::Music,
        Spot::Lyric => Expr::Sing,
        _ => match i.cli {
            Cli::Done => Expr::Happy,
            Cli::Failed => Expr::Sad,
            _ if i.claude_working || i.cli == Cli::Running || i.mole_running => Expr::Typing,
            _ if i.battery_low => Expr::Sad,
            _ if i.asleep => Expr::Sleep,
            _ => Expr::Idle,
        },
    })
}

// ---- Motion policy ----

crate::theme::named_enum! {
    /// "Animasi": Penuh (blink and move), Hemat (no blink or motion; faces
    /// and sleep still change), Mati (static: no robot timers at all).
    Motion { Full, Saver, Off }
}

impl Motion {
    /// What runs with Windows' Energy Saver on: Full drops to Saver.
    pub fn effective(self, energy_saver: bool) -> Motion {
        match self {
            Motion::Full if energy_saver => Motion::Saver,
            m => m,
        }
    }

    /// Eyes blink (Full only).
    pub fn blinks(self) -> bool {
        self == Motion::Full
    }

    /// Hover lift and alert bounce (Full only).
    pub fn moves(self) -> bool {
        self == Motion::Full
    }

    /// Any robot timer at all (sleep, transient faces): not when Off.
    pub fn timed(self) -> bool {
        self != Motion::Off
    }
}

// ---- Animation math ----

pub const BLINK_MS: u32 = 120;
pub const BLINK_MIN_MS: u32 = 4_000;
pub const BLINK_MAX_MS: u32 = 7_000;
/// Hover: the robot rises this far over `LIFT_MS`.
pub const LIFT_PX: f32 = 2.0;
pub const LIFT_MS: u64 = 140;
/// Alert: two bounces this high, each `BOUNCE_MS` long, then still.
pub const BOUNCE_PX: f32 = 3.0;
pub const BOUNCE_MS: u64 = 260;
pub const BOUNCES: u64 = 2;
/// The falk's hop on a "done" notice: one half-sine this high, `HOP_MS` long.
/// 1.875 px is the spec's 3 grid units in the 40 px notice badge (the only place it hops).
pub const HOP_PX: f32 = 1.875;
pub const HOP_MS: u64 = 400;
/// The greeting's wave: `WAVE_SWINGS` swings over `WAVE_MS`, then the raised
/// arm holds.
pub const WAVE_MS: u64 = 900;
pub const WAVE_SWINGS: u32 = 3;
/// The octopus's flush on an alert: one half-sine, `FLUSH_MS` long.
pub const FLUSH_MS: u64 = 700;
/// A Claude session counts as working this long after its last activity.
pub const CLAUDE_WORKING_FOR: Duration = Duration::from_secs(120);
/// A finished CLI command shows Happy or Sad this long.
pub const CLI_MOOD_FOR: Duration = Duration::from_secs(4);
/// The sleep setting's range, in minutes.
pub const SLEEP_MIN: u32 = 1;
pub const SLEEP_MAX: u32 = 60;

/// A small xorshift generator: enough for blink delays, seedable in tests.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }
}

/// The wait before the next blink: 4 to 7 s.
pub fn blink_delay_ms(rng: &mut Rng) -> u32 {
    BLINK_MIN_MS + rng.next_u32() % (BLINK_MAX_MS - BLINK_MIN_MS + 1)
}

/// The sleep setting kept in its range (hand-edited files).
pub fn clamp_sleep(minutes: u32) -> u32 {
    minutes.clamp(SLEEP_MIN, SLEEP_MAX)
}

/// The sleep stepper one step up (`dir > 0`) or down: by 1 up to 5 minutes,
/// by 5 above.
pub fn sleep_step(minutes: u32, dir: i32) -> u32 {
    let m = clamp_sleep(minutes);
    let next = if dir > 0 {
        if m < 5 { m + 1 } else { (m / 5 + 1) * 5 }
    } else if m <= 5 {
        m.saturating_sub(1)
    } else {
        (m - 1) / 5 * 5
    };
    clamp_sleep(next)
}

/// Nothing happened for `after`.
pub fn asleep(last_activity: Instant, now: Instant, after: Duration) -> bool {
    now.saturating_duration_since(last_activity) >= after
}

/// How long until `asleep` turns true; `None` once it is.
pub fn time_to_sleep(last_activity: Instant, now: Instant, after: Duration) -> Option<Duration> {
    let idle = now.saturating_duration_since(last_activity);
    after.checked_sub(idle).filter(|d| !d.is_zero())
}

/// How high the alert bounce lifts the robot `elapsed` after it began: two
/// half-sine hops, then 0.
pub fn bounce_offset(elapsed: Duration) -> f32 {
    let ms = elapsed.as_millis() as u64;
    if ms >= BOUNCE_MS * BOUNCES {
        return 0.0;
    }
    let t = (ms % BOUNCE_MS) as f32 / BOUNCE_MS as f32;
    BOUNCE_PX * (std::f32::consts::PI * t).sin()
}

/// The falk's hop `elapsed` after a "done" notice appeared: one half-sine of
/// `HOP_PX`, then 0.
pub fn hop_offset(elapsed: Duration) -> f32 {
    let ms = elapsed.as_millis() as u64;
    if ms >= HOP_MS {
        return 0.0;
    }
    HOP_PX * (std::f32::consts::PI * ms as f32 / HOP_MS as f32).sin()
}

/// The greeting's swing, -1 to 1, `elapsed` after it began; `None` once the
/// swings are done (the arm then holds still).
pub fn wave_swing(elapsed: Duration) -> Option<f32> {
    let ms = elapsed.as_millis() as u64;
    if ms >= WAVE_MS {
        return None;
    }
    let t = ms as f32 / WAVE_MS as f32;
    Some((2.0 * std::f32::consts::PI * WAVE_SWINGS as f32 * t).sin())
}

/// The octopus's flush, 0 to 1, `elapsed` after the alert appeared.
pub fn flush_amount(elapsed: Duration) -> f32 {
    let ms = elapsed.as_millis() as u64;
    if ms >= FLUSH_MS {
        return 0.0;
    }
    (std::f32::consts::PI * ms as f32 / FLUSH_MS as f32).sin()
}

/// The hover lift `elapsed` after the pointer arrived: eases up to `LIFT_PX`.
pub fn lift_offset(elapsed: Duration) -> f32 {
    let t = (elapsed.as_millis() as f32 / LIFT_MS as f32).clamp(0.0, 1.0);
    LIFT_PX * (1.0 - (1.0 - t).powi(3))
}

// ---- Dance ----

/// How far the whole cat dips at the deepest nod (a level of 1), in dip.
pub const NOD_MAX_DIP: f32 = 1.5;
/// The nod's squash at its deepest: the width grows and the height shrinks
/// by these fractions, around the head's base centre.
pub const SQUASH_W: f32 = 0.04;
pub const SQUASH_H: f32 = 0.06;
/// The fallback beat when capture is not live: 600 ms, 100 BPM.
pub const BEAT_MS: u64 = 600;
/// The floating note rises and fades over this long, then starts again.
pub const NOTE_LOOP_MS: u64 = 1_200;
/// How far the note rises over a loop, at most, in dip.
pub const NOTE_RISE_DIP: f32 = 3.0;
/// The room kept between the note and the top of the shape, in dip.
pub const NOTE_MARGIN_DIP: f32 = 0.5;

/// The grid point the squash is made around: the head's base centre.
pub const SQUASH_ORIGIN: (f32, f32) = (32.0, 54.0);

/// The cat nods to the music: only the faces with the note (`Music` in the
/// compact media view, `Sing` in the lyric view), only with the cat on, the
/// setting on, full motion (`motion` already folds in Energy Saver), Windows'
/// animations on, the music playing and `TIMER_EQ` running (it is the only
/// tick that redraws). Everything else is drawn at rest.
pub fn dance_on(
    robot: bool,
    cat_dance: bool,
    motion: Motion,
    windows_animations: bool,
    playing: bool,
    eq_timer_on: bool,
    expr: Expr,
) -> bool {
    robot
        && cat_dance
        && motion.moves()
        && windows_animations
        && playing
        && eq_timer_on
        && matches!(expr, Expr::Music | Expr::Sing)
}

/// The nod for a bass `level`, 0 (rest) to 1; a level that is not a number,
/// or is negative, is rest.
pub fn nod_amount(level: f32) -> f32 {
    if level.is_nan() || level <= 0.0 {
        0.0
    } else {
        level.min(1.0)
    }
}

/// The nod when capture is not live: `sin(pi * t / BEAT_MS)^2`, 0 on each
/// beat boundary and 1 in the middle of the beat.
pub fn fallback_beat(elapsed: Duration) -> f32 {
    let beat = u128::from(BEAT_MS) * 1_000;
    let t = (elapsed.as_micros() % beat) as f32 / beat as f32;
    (std::f32::consts::PI * t).sin().powi(2)
}

/// The note `elapsed` into its loop: how far up it is (0 to 1, easing out)
/// and how faded (0 for the first half, then linearly to 1). At the start of
/// a loop the note is back at its rest place, fully visible.
pub fn note_phase(elapsed: Duration) -> (f32, f32) {
    let loop_us = u128::from(NOTE_LOOP_MS) * 1_000;
    let p = (elapsed.as_micros() % loop_us) as f32 / loop_us as f32;
    let rise = 1.0 - (1.0 - p).powi(3);
    let fade = if p < 0.5 { 0.0 } else { (p - 0.5) * 2.0 };
    (rise, fade)
}

/// The grid top of the note marks of `expr` (the top of the highest note's
/// beam, stroke included), or `None` for a face without notes.
pub fn notes_top(expr: Expr) -> Option<f32> {
    // `note(x, y)`: the beam's corner is at y - 9.5, the stroke is round.
    let top = |y: f32| y - 9.5 - Line::Note { x: 0.0, y: 0.0 }.width() / 2.0;
    match expr {
        Expr::Music => Some(top(13.0)),
        Expr::Sing => Some(top(3.0).min(top(13.0))),
        _ => None,
    }
}

/// How far the dancing note is lifted (dip, up is positive) when the shape
/// leaves `rest_top_room_dip` between the top of the note at rest and the top
/// of the shape: it rises `rise` of the way up to `NOTE_RISE_DIP`, but never
/// past `NOTE_MARGIN_DIP` from the top. With less room than the margin (the
/// upper note of the singing cat sits above the top of a 28 to 30 dip notch
/// at rest) it is shifted down by the shortfall instead and does not rise, so
/// the note is never clipped. Only the dancing note is moved.
pub fn note_lift_dip(rest_top_room_dip: f32, rise: f32) -> f32 {
    let room = rest_top_room_dip - NOTE_MARGIN_DIP;
    if room >= 0.0 {
        rise.clamp(0.0, 1.0) * NOTE_RISE_DIP.min(room)
    } else {
        room
    }
}

/// A mark of the floating music note: its stem and beam and its cream heads.
/// (Only the faces with a note ever dance, so the dance fields of a `Pose`
/// reach no other cream mark.)
fn is_note(part: &Part) -> bool {
    matches!(
        part,
        Part::Stroke(Line::Note { .. })
            | Part::Dot {
                ink: Ink::Cream,
                ..
            }
    )
}

/// A mark of the eyes (espresso ink): it rises with the mascot's eye lift.
/// The cream extras beside the head stay at their grid places.
fn is_eye(part: &Part) -> bool {
    match *part {
        Part::Pill { .. } => true,
        Part::Dot { ink, .. } => ink == Ink::Espresso,
        Part::Stroke(line) => line.ink() == Ink::Espresso,
        Part::Heart { .. } => false,
    }
}

/// How much room (dip) lies between the top of `expr`'s note marks and
/// `shape_top`, for the cat drawn in `area` with the nod and squash of
/// `pose`: the note's grid top squashed round the head's base, scaled, and
/// moved down by the nod. Negative when the note is above the shape. `None`
/// for a face without notes.
pub fn note_top_room(expr: Expr, pose: Pose, area: Rect, shape_top: f32) -> Option<f32> {
    let top = notes_top(expr)?;
    let k = area.w / GRID;
    let squashed = SQUASH_ORIGIN.1 + (top - SQUASH_ORIGIN.1) * (1.0 - SQUASH_H * pose.squash);
    let y = area.y - pose.lift_px - pose.bounce_px + pose.nod_dip + squashed * k;
    Some(y - shape_top)
}

/// `pose` with the dancing note placed for a shape whose top is `shape_top`
/// (`note_lift_dip` for the room the cat has above its note). A pose that is
/// not dancing, or a face without notes, comes back as it is.
pub fn with_note_lift(pose: Pose, expr: Expr, area: Rect, shape_top: f32) -> Pose {
    if !pose.dancing {
        return pose;
    }
    match note_top_room(expr, pose, area, shape_top) {
        Some(room) => Pose {
            note_dy_dip: -note_lift_dip(room, pose.note_rise),
            ..pose
        },
        None => pose,
    }
}

// ---- Typing ----

// The laptop and the paws, in the cat's 64-unit grid. They are drawn only for
// `Expr::Typing`, in fixed colours, and are not turned by the face's tilt.

/// The laptop's base: x, y, w, h, corner radius (x -27 to 9, y 53 to 56.4).
const LAPTOP_BASE: (f32, f32, f32, f32, f32) = (-27.0, 53.0, 36.0, 3.4, 1.5);
const LAPTOP_BASE_RGB: u32 = 0x8F857C;
/// The lid, leaning back to the left of the cat.
const LID: [(f32, f32); 4] = [(-23.0, 54.0), (-17.5, 54.0), (-20.5, 27.5), (-25.6, 28.2)];
const LAPTOP_LID_RGB: u32 = 0x4A423C;
/// The screen's light along the lid's right edge.
const LID_EDGE: [(f32, f32); 4] = [(-20.5, 27.5), (-18.4, 27.3), (-15.3, 53.6), (-17.5, 54.0)];
const LAPTOP_EDGE_RGB: u32 = 0x9CC8E8;
const LAPTOP_EDGE_ALPHA: f32 = 0.55;
/// The logo dot on the lid: cx, cy, r.
const LAPTOP_LOGO: (f32, f32, f32) = (-21.6, 40.5, 1.5);
const LAPTOP_LOGO_ALPHA: f32 = 0.85;
/// The two paws (left, right) at rest, and their size. The Black coat's paws
/// get a thin outline in its rim colour.
pub(super) const PAWS: [(f32, f32); 2] = [(1.0, 52.6), (9.5, 52.6)];
const PAW_RX: f32 = 3.8;
const PAW_RY: f32 = 2.4;
const PAW_OUTLINE: f32 = 1.0;

/// The laptop's box in dip for a cat drawn in `area`: grid x -27 to 9, y
/// 27.3 (the lid's top edge, its round stroke excluded) to 56.4. It sticks out
/// to the left of the cat's own square, which is why the island grows to the
/// left (`geometry::TYPING_LEAD`) and why the see-through halo covers it.
pub fn laptop_rect(area: Rect) -> Rect {
    let k = area.w / GRID;
    Rect {
        x: area.x - 27.0 * k,
        y: area.y + 27.3 * k,
        w: 36.0 * k,
        h: (56.4 - 27.3) * k,
    }
}

/// Every grid point the laptop and the paws reach, for the clamp test: the
/// lid and its lit edge, the base's corners, the logo's box, and each paw's
/// box with its lift and the Black coat's outline included. `paw_lift` is
/// per paw, in grid units.
#[cfg(test)]
pub fn typing_points(paw_lift: [f32; 2]) -> Vec<(f32, f32)> {
    let mut points: Vec<(f32, f32)> = LID.iter().chain(LID_EDGE.iter()).copied().collect();
    let (x, y, w, h, _) = LAPTOP_BASE;
    points.extend([(x, y), (x + w, y), (x + w, y + h), (x, y + h)]);
    let (cx, cy, r) = LAPTOP_LOGO;
    points.extend([
        (cx - r, cy - r),
        (cx + r, cy - r),
        (cx + r, cy + r),
        (cx - r, cy + r),
    ]);
    for (i, &(px, py)) in PAWS.iter().enumerate() {
        let py = py - paw_lift[i];
        let (rx, ry) = (PAW_RX + PAW_OUTLINE / 2.0, PAW_RY + PAW_OUTLINE / 2.0);
        points.extend([
            (px - rx, py - ry),
            (px + rx, py - ry),
            (px + rx, py + ry),
            (px - rx, py + ry),
        ]);
    }
    points
}

/// The shortest and longest wait between two paw steps, in milliseconds.
pub const TYPE_STEP_MIN_MS: u32 = 110;
pub const TYPE_STEP_MAX_MS: u32 = 200;
/// About `TYPE_GAP_PCT` percent of the steps wait this long instead (a
/// thinking beat).
pub const TYPE_GAP_MS: u32 = 420;
pub const TYPE_GAP_PCT: u32 = 12;
/// About one step in `TYPE_PAUSE_ONE_IN` starts a pause: both paws down this
/// long (a pause about every 8 s on average).
pub const TYPE_PAUSE_MS: u32 = 900;
pub const TYPE_PAUSE_ONE_IN: u32 = 43;

/// The typing paws' rhythm: they take turns (left up, then right up), with
/// now and then a longer beat and a pause with both down. Pure; `App` fires
/// `next` on a one-shot timer and draws once per step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypingPaws {
    /// The paw that is up now; `None` is a pause or the start (both down).
    up: Option<Side>,
    /// The paw that was up last, so the next one is the other.
    last: Option<Side>,
}

impl TypingPaws {
    /// The paw that is up now.
    pub fn up(&self) -> Option<Side> {
        self.up
    }

    /// Both paws down, as at the start.
    pub fn reset(&mut self) {
        *self = TypingPaws::default();
    }

    /// The timer fired: a step now, and the milliseconds until the next one.
    /// From a paw up, about one step in `TYPE_PAUSE_ONE_IN` puts both down
    /// for `TYPE_PAUSE_MS`; otherwise (and out of a pause) the other paw
    /// goes up, after 110 to 200 ms, or `TYPE_GAP_MS` in `TYPE_GAP_PCT`
    /// percent of the steps.
    pub fn next(&mut self, rng: &mut Rng) -> u32 {
        if self.up.is_some() && rng.next_u32() % TYPE_PAUSE_ONE_IN == 0 {
            self.up = None;
            return TYPE_PAUSE_MS;
        }
        let paw = match self.last {
            Some(Side::Left) => Side::Right,
            _ => Side::Left,
        };
        self.up = Some(paw);
        self.last = Some(paw);
        if rng.next_u32() % 100 < TYPE_GAP_PCT {
            TYPE_GAP_MS
        } else {
            TYPE_STEP_MIN_MS + rng.next_u32() % (TYPE_STEP_MAX_MS - TYPE_STEP_MIN_MS + 1)
        }
    }

    /// How high each paw (left, right) is raised now, in grid units.
    pub fn lift(&self) -> [f32; 2] {
        [
            if self.up == Some(Side::Left) {
                PAW_LIFT
            } else {
                0.0
            },
            if self.up == Some(Side::Right) {
                PAW_LIFT
            } else {
                0.0
            },
        ]
    }
}

/// The paws move: the face is Typing, motion is Full (`motion` already folds
/// in Energy Saver), Windows' animations are on, nothing is fullscreen, the
/// screen is not locked or off, and the notch is not shrunk to the eyes.
/// Otherwise the paws stay down and nothing is timed.
pub fn typing_anim_on(
    expr: Option<Expr>,
    motion: Motion,
    windows_animations: bool,
    fullscreen: bool,
    screen_away: bool,
    shrunk: bool,
) -> bool {
    expr == Some(Expr::Typing)
        && motion.moves()
        && windows_animations
        && !fullscreen
        && !screen_away
        && !shrunk
}

/// The idle island has grown to hold the laptop: the face would be Typing
/// without the hover (which shows Happy). The shape follows this, not the
/// face, so the island does not shrink under the pointer.
pub fn typing_wanted(i: &RobotInputs) -> bool {
    choose_expr(&RobotInputs { hover: false, ..*i }) == Some(Expr::Typing)
}

// ---- The robot's state ----

/// What the robot remembers between events. Pure: the caller passes the time.
#[derive(Clone, Debug)]
pub struct Brain {
    last_activity: Instant,
    /// Busy Claude sessions and when each was last heard from.
    claude: Vec<(String, Instant)>,
    cli_running: bool,
    /// A Mole clean runs.
    pub mole_running: bool,
    /// A finished command's face and until when it shows.
    cli_mood: Option<(Cli, Instant)>,
    pub battery_low: bool,
    /// The pointer arrived over the island at this time.
    hover: Option<Instant>,
    /// An alert bounce began.
    bounce: Option<Instant>,
    /// The octopus's flush began (with the bounce).
    flush: Option<Instant>,
    /// The falk's hop began.
    hop: Option<Instant>,
    /// The greeting's wave began; kept while the Wave face shows.
    wave: Option<Instant>,
    /// The notice token last observed (see `observe`).
    notice: Option<u64>,
    /// The face last drawn.
    shown: Option<Expr>,
}

impl Brain {
    pub fn new(now: Instant) -> Self {
        Self {
            last_activity: now,
            claude: Vec::new(),
            cli_running: false,
            mole_running: false,
            cli_mood: None,
            battery_low: false,
            hover: None,
            bounce: None,
            flush: None,
            hop: None,
            wave: None,
            notice: None,
            shown: None,
        }
    }

    /// Something happened: the sleep clock starts over.
    pub fn activity(&mut self, now: Instant) {
        self.last_activity = now;
    }

    pub fn claude_activity(&mut self, session: &str, now: Instant) {
        self.claude.retain(|(s, _)| s != session);
        self.claude.push((session.to_owned(), now));
    }

    /// The session finished or waits for the user: not working.
    pub fn claude_ended(&mut self, session: &str) {
        self.claude.retain(|(s, _)| s != session);
    }

    /// A session showed activity within `CLAUDE_WORKING_FOR`.
    pub fn claude_working(&self, now: Instant) -> bool {
        self.claude
            .iter()
            .any(|(_, at)| now.saturating_duration_since(*at) < CLAUDE_WORKING_FOR)
    }

    pub fn cli_started(&mut self) {
        self.cli_running = true;
        self.cli_mood = None;
    }

    pub fn cli_finished(&mut self, ok: bool, now: Instant) {
        self.cli_running = false;
        let mood = if ok { Cli::Done } else { Cli::Failed };
        self.cli_mood = Some((mood, now + CLI_MOOD_FOR));
    }

    pub fn cli(&self, now: Instant) -> Cli {
        match self.cli_mood {
            Some((mood, until)) if now < until => mood,
            _ if self.cli_running => Cli::Running,
            _ => Cli::None,
        }
    }

    /// Mouse or keyboard input somewhere on the PC, `input_idle` ago (from
    /// `GetLastInputInfo`): the sleep clock restarts at that moment if it is
    /// newer than the last activity.
    pub fn input_seen(&mut self, now: Instant, input_idle: Duration) {
        self.last_activity = fold_input(self.last_activity, now, input_idle);
    }

    /// The face last drawn (see `observe`).
    pub fn shown(&self) -> Option<Expr> {
        self.shown
    }

    /// The pointer came over the island (`true`) or left.
    pub fn hover(&mut self, on: bool, now: Instant) {
        if on {
            self.hover.get_or_insert(now);
            self.last_activity = now;
        } else {
            self.hover = None;
        }
    }

    /// The inputs `choose_expr` needs at `now`. With motion `Off` nothing
    /// is timed: no sleep and no transient CLI faces.
    pub fn inputs(
        &self,
        spot: Spot,
        now: Instant,
        sleep_after: Duration,
        motion: Motion,
    ) -> RobotInputs {
        let timed = motion.timed();
        let cli = match self.cli(now) {
            Cli::Done | Cli::Failed if !timed => {
                if self.cli_running {
                    Cli::Running
                } else {
                    Cli::None
                }
            }
            c => c,
        };
        RobotInputs {
            spot,
            hover: self.hover.is_some(),
            asleep: timed && asleep(self.last_activity, now, sleep_after),
            claude_working: self.claude_working(now),
            cli,
            mole_running: self.mole_running,
            battery_low: self.battery_low,
        }
    }

    /// Records the face drawn; an alert that just appeared starts a bounce
    /// (and the octopus's flush) when motion is allowed. True when a bounce
    /// started. `notice` is a token that changes with every notice shown
    /// (`None` for any other view): a new token with a Happy face starts the
    /// falk's hop, with the Wave face the greeting's wave. Hover never has one.
    pub fn observe(
        &mut self,
        expr: Option<Expr>,
        notice: Option<u64>,
        now: Instant,
        moves: bool,
    ) -> bool {
        let was = std::mem::replace(&mut self.shown, expr);
        let fresh = notice.is_some() && notice != std::mem::replace(&mut self.notice, notice);
        let started = moves && expr == Some(Expr::Alert) && was != Some(Expr::Alert);
        if started {
            self.bounce = Some(now);
            self.flush = Some(now);
        }
        if moves && fresh {
            match expr {
                Some(Expr::Happy) => self.hop = Some(now),
                Some(Expr::Wave) => self.wave = Some(now),
                _ => {}
            }
        }
        if expr != Some(Expr::Alert) {
            self.bounce = None;
            self.flush = None;
        }
        if notice.is_none() {
            self.hop = None;
        }
        if expr != Some(Expr::Wave) {
            self.wave = None;
        }
        started
    }

    /// When the hover lift began, if it applies at `spot`: only a compact
    /// island lifts (its face turns Happy); a notice keeps its face still.
    fn lift_since(&self, spot: Spot) -> Option<Instant> {
        self.hover.filter(|_| spot.compact())
    }

    /// Lift and bounce at `spot` and `now` (none without motion), the
    /// mascot's own one-shots (falk hop, falk and octopus wave, octopus
    /// flush), and the blink.
    pub fn pose(
        &self,
        spot: Spot,
        now: Instant,
        moves: bool,
        blink: bool,
        mascot: Mascot,
    ) -> Pose {
        if !moves {
            return Pose {
                blink: false,
                ..Pose::default()
            };
        }
        let since = |at: Instant| now.saturating_duration_since(at);
        let waves = mascot != Mascot::Cat;
        Pose {
            blink,
            lift_px: self.lift_since(spot).map_or(0.0, |at| lift_offset(since(at))),
            bounce_px: self.bounce.map_or(0.0, |at| bounce_offset(since(at))),
            hop_px: self
                .hop
                .filter(|_| mascot == Mascot::Falk)
                .map_or(0.0, |at| hop_offset(since(at))),
            waving: waves && self.wave.is_some(),
            wave: self
                .wave
                .filter(|_| waves)
                .and_then(|at| wave_swing(since(at)))
                .unwrap_or(0.0),
            flush: self
                .flush
                .filter(|_| mascot == Mascot::Octopus)
                .map_or(0.0, |at| flush_amount(since(at))),
            ..Pose::default()
        }
    }

    /// A lift, bounce or the mascot's one-shot at `spot` is still moving: the
    /// frame clock must run.
    pub fn moving(&self, spot: Spot, now: Instant, moves: bool, mascot: Mascot) -> bool {
        let within = |at: Option<Instant>, ms: u64| {
            at.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_millis(ms))
        };
        moves
            && (within(self.lift_since(spot), LIFT_MS)
                || within(self.bounce, BOUNCE_MS * BOUNCES)
                || (mascot == Mascot::Falk && within(self.hop, HOP_MS))
                || (mascot != Mascot::Cat && within(self.wave, WAVE_MS))
                || (mascot == Mascot::Octopus && within(self.flush, FLUSH_MS)))
    }

    /// The next moment the face may change by itself (falling asleep, a
    /// Claude session going quiet, a CLI face ending), for a one-shot timer.
    pub fn next_change(&self, now: Instant, sleep_after: Duration) -> Option<Instant> {
        let sleep = time_to_sleep(self.last_activity, now, sleep_after).map(|d| now + d);
        let cli = self.cli_mood.map(|(_, until)| until).filter(|t| *t > now);
        sleep
            .into_iter()
            .chain(self.next_quiet(now))
            .chain(cli)
            .min()
    }

    /// The next moment a busy Claude session goes quiet
    /// (`CLAUDE_WORKING_FOR` after it was last heard from).
    pub fn next_quiet(&self, now: Instant) -> Option<Instant> {
        self.claude
            .iter()
            .map(|(_, at)| *at + CLAUDE_WORKING_FOR)
            .filter(|t| *t > now)
            .min()
    }
}

/// When `TIMER_ROBOT` fires: `Brain::next_change` when the plan has the
/// one-shot. Motion Off has none (the face stays as it is), but while the idle
/// island holds the typing cat's wider shape (`typing_shape`) it still fires
/// when a Claude session goes quiet, so the island shrinks back then and not
/// at some later unrelated refresh. A CLI or Mole end refreshes by itself.
pub fn change_at(
    brain: &Brain,
    plan: TimerPlan,
    typing_shape: bool,
    now: Instant,
    sleep_after: Duration,
) -> Option<Instant> {
    if plan.change {
        brain.next_change(now, sleep_after)
    } else if typing_shape {
        brain.next_quiet(now)
    } else {
        None
    }
}

/// Which robot timers run. `face`: the robot on screen (compact view or
/// notice), if any. `motion`: the effective policy. `animating`: the frame
/// clock runs anyway (morphs, lifts), so no blink starts meanwhile. Only
/// open eyes blink: a sleeping, happy or singing robot runs no blink timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerPlan {
    /// The one-shot blink timer (4–7 s, then 120 ms).
    pub blink: bool,
    /// The one-shot for `Brain::next_change`.
    pub change: bool,
}

pub fn plan_timers(face: Option<Expr>, motion: Motion, animating: bool) -> TimerPlan {
    TimerPlan {
        blink: face.is_some_and(Expr::blinks) && motion.blinks() && !animating,
        change: face.is_some() && motion.timed(),
    }
}

// ---- Sleep follows the PC's input ----

/// How often a sleeping cat looks for new PC input. 1.8 s, not 2 s: the wake
/// must land within 2 s of the input, and a coalescable timer may fire up to
/// `WAKE_POLL_SLACK_MS` late, so interval + slack stays at or under 2 000 ms.
pub const WAKE_POLL_MS: u32 = 1_800;
/// The coalescing slack of that poll.
pub const WAKE_POLL_SLACK_MS: u32 = 200;

/// `last_activity` moved forward to the last PC input (`input_idle` before
/// `now`), never backwards. The input moment is never after `now`. An idle
/// time too long for `Instant` to hold leaves it as it is.
pub fn fold_input(last_activity: Instant, now: Instant, input_idle: Duration) -> Instant {
    match now.checked_sub(input_idle) {
        Some(input_at) => last_activity.max(input_at),
        None => last_activity,
    }
}

/// Whether `TIMER_ROBOT` also polls for input: only while the Sleep face is on
/// screen (`face` is `None` with the robot off, the panel open or the island
/// hidden), the idle view shows, motion is timed, and nothing is shrunk,
/// fullscreen, locked or dark.
pub fn wake_poll_on(
    face: Option<Expr>,
    spot: Spot,
    motion: Motion,
    shrunk: bool,
    fullscreen: bool,
    screen_away: bool,
) -> bool {
    face == Some(Expr::Sleep)
        && spot == Spot::Idle
        && motion.timed()
        && !shrunk
        && !fullscreen
        && !screen_away
}

/// When `TIMER_ROBOT` fires, and whether that is the wake poll. With the poll
/// on, the earlier of `change` and `now + WAKE_POLL_MS`; without it, `change`.
pub fn robot_timer_at(
    change: Option<Instant>,
    wake_poll: bool,
    now: Instant,
) -> Option<(Instant, bool)> {
    if !wake_poll {
        return change.map(|c| (c, false));
    }
    let poll = now + Duration::from_millis(u64::from(WAKE_POLL_MS));
    match change {
        Some(c) if c < poll => Some((c, false)),
        _ => Some((poll, true)),
    }
}

/// A wake-poll fire where the Sleep face is already drawn and still right
/// needs no refresh and no draw, only a re-arm.
pub fn wake_tick_quiet(shown: Option<Expr>, face_now: Option<Expr>) -> bool {
    shown == Some(Expr::Sleep) && face_now == Some(Expr::Sleep)
}

// ---- Greeting ----

/// The greeting's switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GreetSettings {
    pub robot: bool,
    pub greeting: bool,
}

/// The period to greet in at `now`: once per period per calendar day, with
/// the robot and the greeting on. `last` is the mark of the last greeting;
/// a later period the same day greets again.
pub fn should_greet(
    last: Option<GreetMark>,
    now: NaiveDateTime,
    settings: GreetSettings,
) -> Option<DayPeriod> {
    let period = greeting::period_at(now.time());
    let this = GreetMark {
        date: now.date(),
        period,
    };
    (settings.robot && settings.greeting && last != Some(this)).then_some(period)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Notice;

    fn inputs(spot: Spot) -> RobotInputs {
        RobotInputs {
            spot,
            hover: false,
            asleep: false,
            claude_working: false,
            cli: Cli::None,
            mole_running: false,
            battery_low: false,
        }
    }

    #[test]
    fn the_look_turns_the_idle_cat_toward_the_pointer() {
        // No look: the pose is exactly as it was.
        assert_eq!(Pose::default().looking(0.0), Pose::default());
        let left = Pose::default().looking(-1.0);
        let right = Pose::default().looking(1.0);
        assert_eq!(
            (right.head_dx_dip, right.tilt_deg, right.eye_dx_dip),
            (7.0, 6.0, 4.0)
        );
        assert_eq!(
            (left.head_dx_dip, left.tilt_deg, left.eye_dx_dip),
            (-7.0, -6.0, -4.0)
        );
        // Half way is half the way; past the ends is clamped.
        assert_eq!(Pose::default().looking(0.5).head_dx_dip, 3.5);
        assert_eq!(Pose::default().looking(9.0), right);
        // Blink, lift and bounce carry on under the look.
        let lifted = Pose {
            blink: true,
            lift_px: 2.0,
            bounce_px: 1.0,
            ..Pose::default()
        };
        let looked = lifted.looking(1.0);
        assert!(looked.blink && looked.lift_px == 2.0 && looked.bounce_px == 1.0);
        // A sleeping or thinking cat keeps still: its z's and thought dots
        // float beside the right ear, a few dip from the idle clock.
        for expr in [Expr::Sleep, Expr::Think] {
            assert!(!expr.looks(), "{expr:?}");
            assert_eq!(lifted.looking_as(expr, 1.0), lifted, "{expr:?}");
        }
        for expr in [Expr::Idle, Expr::Happy, Expr::Sad] {
            assert!(expr.looks(), "{expr:?}");
            assert_eq!(lifted.looking_as(expr, 1.0), looked, "{expr:?}");
        }
    }

    #[test]
    fn the_shrunk_notch_shows_only_the_two_open_eyes() {
        use super::super::geometry::{EYES_H, EYES_W};
        let pills = eye_pills();
        assert_eq!(pills.len(), 2);
        // The very pills of the idle face.
        let idle = face(Expr::Idle, false).parts;
        assert_eq!(pills, idle);
        // Centred in the 34 x 14 pill, with room round them.
        let centre = (100.0, 50.0);
        let area = eyes_area(centre);
        assert_eq!(area.w, area.h);
        let k = area.w / GRID;
        let (mut left, mut top, mut right, mut bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for part in pills {
            let Part::Pill { x, y, w, h, .. } = part else {
                panic!("an eye is a pill");
            };
            left = left.min(area.x + x * k);
            top = top.min(area.y + y * k);
            right = right.max(area.x + (x + w) * k);
            bottom = bottom.max(area.y + (y + h) * k);
        }
        assert!(((left + right) / 2.0 - centre.0).abs() < 0.01);
        assert!(((top + bottom) / 2.0 - centre.1).abs() < 0.01);
        assert!(right - left < EYES_W - 12.0, "{}", right - left);
        assert!(bottom - top <= EYES_H - 4.0, "{}", bottom - top);
    }

    #[test]
    fn faces_follow_the_cat_design() {
        // Idle: two plain espresso pills 5.6 x 10 on (24, 40) and (40, 40),
        // nothing else on the face (no shine, no lashes); the ears stand still.
        let idle = face(Expr::Idle, false);
        assert_eq!(
            idle.parts,
            vec![
                Part::Pill {
                    x: 21.2,
                    y: 35.0,
                    w: 5.6,
                    h: 10.0,
                    rx: 2.8,
                    ry: 2.8
                },
                Part::Pill {
                    x: 37.2,
                    y: 35.0,
                    w: 5.6,
                    h: 10.0,
                    rx: 2.8,
                    ry: 2.8
                },
            ]
        );
        assert_eq!(
            (idle.ears, idle.bob, idle.tilt),
            (Ears::default(), 0.0, 0.0)
        );
        // The design's rounded rects: rx = w / 2, ry capped at h / 2, so a
        // short eye is a flat ellipse (focus 6 x 4, low 5.6 x 5).
        let corners = |e| match face(e, false).parts[0] {
            Part::Pill { rx, ry, .. } => (rx, ry),
            _ => panic!("{e:?} has no pill eyes"),
        };
        assert_eq!(corners(Expr::Focus), (3.0, 2.0));
        assert_eq!(corners(Expr::Sad), (2.8, 2.5));
        // The ears carry the mood: up when alert, down asleep, drooped when low.
        let ears = |e| face(e, false).ears;
        assert_eq!(
            ears(Expr::Alert),
            Ears {
                up: 4.0,
                droop: 0.0
            }
        );
        assert_eq!(
            ears(Expr::Sleep),
            Ears {
                up: 0.0,
                droop: 4.0
            }
        );
        assert_eq!(
            ears(Expr::Sad),
            Ears {
                up: 0.0,
                droop: 8.0
            }
        );
        assert_eq!(face(Expr::Alert, false).bob, -1.5);
        assert_eq!(face(Expr::Sleep, false).bob, 1.5);
        assert_eq!(face(Expr::Sad, false).bob, 1.5);
        // Waving tilts the head and shows a heart.
        let wave = face(Expr::Wave, false);
        assert_eq!(wave.tilt, -7.0);
        assert!(wave.parts.contains(&Part::Heart { x: 4.0, y: 1.0 }));
        // No mouth: listening carries one note, singing two.
        let notes = |e| {
            face(e, false)
                .parts
                .iter()
                .filter(|p| matches!(p, Part::Stroke(Line::Note { .. })))
                .count()
        };
        assert_eq!((notes(Expr::Music), notes(Expr::Sing)), (1, 2));
        assert!(face(Expr::Music, false).parts.contains(&Part::Dot {
            cx: 57.0 - 1.6,
            cy: 13.0,
            r: 1.9,
            ink: Ink::Cream
        }));
        // A blink closes open eyes to the design's two lines; the ears and
        // extras stay.
        for expr in [Expr::Idle, Expr::Focus, Expr::Alert, Expr::Think, Expr::Sad] {
            let shut = face(expr, true);
            assert_eq!(shut.parts[..1], [Part::Stroke(Line::Blink)], "{expr:?}");
            assert_eq!(shut.ears, face(expr, false).ears, "{expr:?}");
        }
        assert_eq!(face(Expr::Happy, true), face(Expr::Happy, false));
        assert_eq!(
            face(Expr::Think, true).parts[1..],
            face(Expr::Think, false).parts[2..]
        );
        // Every face is eyes and, at most, what floats beside the head: no
        // other marks on it.
        for expr in Expr::ALL {
            assert!(
                face(expr, false).parts.iter().all(|p| !matches!(
                    p,
                    Part::Dot {
                        ink: Ink::Cream,
                        cx,
                        ..
                    } if *cx < 50.0
                )),
                "{expr:?}"
            );
        }
        // The design's paths, made absolute.
        assert_eq!(
            Line::Happy.segments()[..2],
            [
                Seg::Move(L - 3.8, Y + 1.8),
                Seg::Quad(L, Y - 3.6, L + 3.8, Y + 1.8)
            ]
        );
        assert_eq!(
            (Line::Zz.alpha(), Line::Happy.width(), Line::Zz.ink()),
            (0.85, 2.4, Ink::Cream)
        );
    }

    #[test]
    fn ears_are_triangles_that_rise_and_turn_outward() {
        assert_eq!(
            ear(Side::Left, 0.0),
            [(9.0, 37.0), (12.0, 10.0), (27.0, 26.0)]
        );
        assert_eq!(
            ear(Side::Right, 3.0),
            [(55.0, 37.0), (52.0, 7.0), (37.0, 26.0)]
        );
        // Drooping turns the left ear anticlockwise and the right one clockwise.
        assert_eq!((Side::Left.turn(4.0), Side::Right.turn(4.0)), (-12.0, 12.0));
    }

    #[test]
    fn head_scales_with_the_size() {
        let (head, r) = head_rect(Rect {
            x: 10.0,
            y: 0.0,
            w: 32.0,
            h: 32.0,
        });
        assert_eq!(
            (head.x, head.y, head.w, head.h, r),
            (13.5, 12.0, 25.0, 15.0, 8.5)
        );
    }

    #[test]
    fn the_head_has_a_flat_base_and_floats_inside_the_grid() {
        // A dome on top, a flat base, small bottom corners; closed.
        let segs = head_path();
        assert_eq!(segs.first(), Some(&Seg::Move(7.0, 50.0)));
        assert_eq!(segs.last(), Some(&Seg::Close));
        assert!(segs.contains(&Seg::Line(11.0, 54.0)), "a flat base");
        // The cat sits inside the grid with room under it, so it does not
        // touch the island's lower edge: ears' tips (stroke included) to the
        // base span about 7.5 to 54 of 64.
        let (_, y, _, h, _) = HEAD;
        assert_eq!(y + h, 54.0);
        let tip = ear(Side::Left, 0.0)[1].1 - EAR_STROKE / 2.0;
        assert!(tip >= 5.0 && 64.0 - (y + h) >= 8.0, "tip {tip}");
    }

    #[test]
    fn expression_priority() {
        let base = inputs(Spot::Idle);
        let e = |i: RobotInputs| choose_expr(&i);
        // No robot in the panel, hidden, or on other apps' notifications.
        for spot in [Spot::Panel, Spot::Hidden, Spot::Notice(NoticeSort::App)] {
            assert_eq!(
                e(RobotInputs {
                    hover: true,
                    ..inputs(spot)
                }),
                None
            );
        }
        // Notices by kind, whatever else is going on.
        let busy = RobotInputs {
            hover: true,
            asleep: true,
            claude_working: true,
            cli: Cli::Failed,
            battery_low: true,
            ..base
        };
        for (sort, expr) in [
            (NoticeSort::Reminder, Expr::Alert),
            (NoticeSort::ClaudeWaiting, Expr::Alert),
            (NoticeSort::PomodoroDone, Expr::Happy),
            (NoticeSort::ClaudeDone, Expr::Happy),
            (NoticeSort::BatteryLow, Expr::Sad),
            (NoticeSort::SystemLow, Expr::Sad),
            (NoticeSort::CleanDone, Expr::Happy),
            (NoticeSort::MoleDone, Expr::Happy),
            (NoticeSort::MoleFailed, Expr::Sad),
            (NoticeSort::Greeting(DayPeriod::Morning), Expr::Wave),
            (NoticeSort::Greeting(DayPeriod::Midday), Expr::Happy),
            (NoticeSort::Greeting(DayPeriod::Afternoon), Expr::Wave),
            (NoticeSort::Greeting(DayPeriod::Evening), Expr::Happy),
            (NoticeSort::Greeting(DayPeriod::Night), Expr::Sleep),
            (NoticeSort::Copied, Expr::Happy),
            (NoticeSort::ClipPreview, Expr::Happy),
            (NoticeSort::Screenshot, Expr::Happy),
            (NoticeSort::ShotSaved, Expr::Happy),
            (NoticeSort::RecSaved, Expr::Happy),
            (NoticeSort::RecFailed, Expr::Sad),
            (NoticeSort::RecInfo, Expr::Idle),
            (NoticeSort::ShotDeleted, Expr::Idle),
            (NoticeSort::DevicePlugged, Expr::Idle),
            (NoticeSort::DeviceSafe, Expr::Happy),
            (NoticeSort::UpdateAvailable, Expr::Idle),
            (NoticeSort::UpdateProgress, Expr::Idle),
            (NoticeSort::UpdateFailed, Expr::Sad),
            (NoticeSort::UpdateInfo, Expr::Happy),
        ] {
            let spot = Spot::Notice(sort);
            assert_eq!(e(RobotInputs { spot, ..busy }), Some(expr), "{sort:?}");
            assert_eq!(e(inputs(spot)), Some(expr), "{sort:?}");
        }
        // Hover beats every compact face.
        for spot in [Spot::Idle, Spot::Pomodoro, Spot::Media, Spot::Lyric] {
            assert_eq!(e(RobotInputs { spot, ..busy }), Some(Expr::Happy));
        }
        let busy = RobotInputs {
            hover: false,
            ..busy
        };
        assert_eq!(
            e(RobotInputs {
                spot: Spot::Pomodoro,
                ..busy
            }),
            Some(Expr::Focus)
        );
        assert_eq!(
            e(RobotInputs {
                spot: Spot::Media,
                ..busy
            }),
            Some(Expr::Music)
        );
        assert_eq!(
            e(RobotInputs {
                spot: Spot::Lyric,
                ..busy
            }),
            Some(Expr::Sing)
        );
        // Idle: CLI result > working > battery > sleep > idle.
        assert_eq!(e(busy), Some(Expr::Sad));
        assert_eq!(
            e(RobotInputs {
                cli: Cli::Done,
                ..busy
            }),
            Some(Expr::Happy)
        );
        let working = RobotInputs {
            cli: Cli::None,
            ..busy
        };
        assert_eq!(e(working), Some(Expr::Typing));
        let running = RobotInputs {
            cli: Cli::Running,
            claude_working: false,
            ..busy
        };
        assert_eq!(e(running), Some(Expr::Typing));
        // A Mole clean types like a CLI command.
        let mole = RobotInputs {
            cli: Cli::None,
            claude_working: false,
            mole_running: true,
            ..busy
        };
        assert_eq!(e(mole), Some(Expr::Typing));
        assert_eq!(
            e(RobotInputs {
                mole_running: true,
                ..base
            }),
            Some(Expr::Typing)
        );
        let low = RobotInputs {
            cli: Cli::None,
            claude_working: false,
            ..busy
        };
        assert_eq!(e(low), Some(Expr::Sad));
        let sleepy = RobotInputs {
            battery_low: false,
            ..low
        };
        assert_eq!(e(sleepy), Some(Expr::Sleep));
        assert_eq!(e(base), Some(Expr::Idle));
    }

    #[test]
    fn notice_sorts_and_spots() {
        let n = |kind| Notice {
            key: "k".into(),
            source: "s".into(),
            title: "t".into(),
            body: "b".into(),
            kind,
        };
        assert_eq!(
            Spot::of(&View::Notice(n(NoticeKind::App {
                app_id: None,
                toast: None
            }))),
            Spot::Notice(NoticeSort::App)
        );
        assert_eq!(
            Spot::of(&View::Notice(n(NoticeKind::Greeting {
                period: DayPeriod::Evening
            }))),
            Spot::Notice(NoticeSort::Greeting(DayPeriod::Evening))
        );
        assert_eq!(Spot::of(&View::MediaLyric), Spot::Lyric);
        assert_eq!(Spot::of(&View::Panel(crate::model::Page::Cli)), Spot::Panel);
    }

    #[test]
    fn a_copy_preview_has_bata_but_a_copied_image_has_its_thumbnail() {
        let preview = |key: &str| {
            View::Notice(Notice {
                key: key.into(),
                source: "Clipboard".into(),
                title: "t".into(),
                body: "b".into(),
                kind: NoticeKind::ClipPreview { id: 1 },
            })
        };
        // Text and files: Bata, happy.
        let text = Spot::of(&preview(crate::model::CLIP_PREVIEW_KEY));
        assert_eq!(text, Spot::Notice(NoticeSort::ClipPreview));
        // An image: no robot, its thumbnail takes the badge spot.
        let image = Spot::of(&preview(crate::model::CLIP_IMAGE_KEY));
        assert_eq!(image, Spot::Notice(NoticeSort::ClipImage));
        let face = |spot| {
            choose_expr(&RobotInputs {
                spot,
                hover: false,
                asleep: false,
                claude_working: false,
                cli: Cli::None,
                mole_running: false,
                battery_low: false,
            })
        };
        assert_eq!(face(text), Some(Expr::Happy));
        assert_eq!(face(image), None);
    }

    #[test]
    fn blink_delay_stays_in_range() {
        let mut rng = Rng::new(42);
        let delays: Vec<u32> = (0..2000).map(|_| blink_delay_ms(&mut rng)).collect();
        assert!(delays.iter().all(|d| (4_000..=7_000).contains(d)));
        // Spread over the range, not stuck.
        assert!(delays.iter().any(|d| *d < 4_500) && delays.iter().any(|d| *d > 6_500));
        // Seeded: the same seed gives the same delays.
        let mut again = Rng::new(42);
        assert_eq!(blink_delay_ms(&mut again), delays[0]);
        assert_eq!(BLINK_MS, 120);
    }

    /// The eyes' blink run until `blinks` shuts have happened: `(time the
    /// timer fired, shut after that, wait until the next firing)`, from an
    /// open start at time 0 (the first firing comes after the open wait).
    fn eyes_blink_run(seed: u64, blinks: usize) -> Vec<(u64, bool, u32)> {
        let mut rng = Rng::new(seed);
        let mut state = EyesBlink::default();
        let mut t = u64::from(EyesBlink::open_wait(&mut rng));
        let mut log = Vec::new();
        let mut shuts = 0;
        while shuts < blinks {
            let wait = state.next(&mut rng);
            log.push((t, state.shut(), wait));
            if state.shut() {
                shuts += 1;
            }
            t += u64::from(wait);
        }
        log
    }

    #[test]
    fn the_eyes_open_wait_is_three_to_six_seconds() {
        assert_eq!((EYES_BLINK_MIN_MS, EYES_BLINK_MAX_MS), (3_000, 6_000));
        for seed in 0..200u64 {
            let mut rng = Rng::new(seed);
            for _ in 0..20 {
                let w = EyesBlink::open_wait(&mut rng);
                assert!((3_000..=6_000).contains(&w), "{w}");
            }
            // And the wait after the last shut of a blink (next from Shut).
            let mut state = EyesBlink::Shut {
                second_follows: false,
            };
            let w = state.next(&mut rng);
            assert!((3_000..=6_000).contains(&w), "{w}");
            assert_eq!(state, EyesBlink::Open);
            assert!(!state.shut());
        }
        // Spread over the range, not stuck.
        let mut rng = Rng::new(7);
        let waits: Vec<u32> = (0..500).map(|_| EyesBlink::open_wait(&mut rng)).collect();
        assert!(waits.iter().any(|w| *w < 3_500) && waits.iter().any(|w| *w > 5_500));
    }

    #[test]
    fn a_blink_shuts_the_eyes_for_the_blink_time() {
        // Open -> Shut: the shut lasts BLINK_MS (120 ms) before the next step.
        for seed in 0..100u64 {
            let mut rng = Rng::new(seed);
            let mut state = EyesBlink::default();
            assert!(!state.shut());
            assert_eq!(state.next(&mut rng), BLINK_MS);
            assert!(state.shut());
        }
    }

    #[test]
    fn about_a_quarter_of_the_eyes_blinks_are_doubles() {
        let mut rng = Rng::new(12345);
        let mut state = EyesBlink::default();
        let (mut blinks, mut doubles) = (0u32, 0u32);
        for _ in 0..20_000 {
            let was_open = state == EyesBlink::Open;
            state.next(&mut rng);
            if was_open {
                blinks += 1;
                if let EyesBlink::Shut { second_follows } = state {
                    doubles += u32::from(second_follows);
                }
            }
            if blinks == 1000 {
                break;
            }
        }
        assert_eq!(blinks, 1000);
        let share = doubles as f32 / blinks as f32;
        assert!((0.15..=0.35).contains(&share), "{doubles} of {blinks}");
        // Over many seeds too, not one lucky stream.
        let total: u32 = (0..100u64)
            .map(|seed| {
                let mut rng = Rng::new(seed * 7919 + 3);
                (0..40)
                    .filter(|_| {
                        let mut s = EyesBlink::Open;
                        s.next(&mut rng);
                        s == EyesBlink::Shut {
                            second_follows: true,
                        }
                    })
                    .count() as u32
            })
            .sum();
        let share = total as f32 / 4000.0;
        assert!((0.15..=0.35).contains(&share), "{total} of 4000");
    }

    #[test]
    fn a_double_blinks_second_shut_starts_260_ms_after_the_first() {
        let mut found = 0;
        for seed in 0..40u64 {
            let log = eyes_blink_run(seed, 60);
            for w in log.windows(3) {
                let (first, gap, second) = (w[0], w[1], w[2]);
                // The gap's wait is what is left of the 260 ms: a double.
                if first.1 && !gap.1 && first.2 == BLINK_MS && gap.2 == EYES_DOUBLE_MS - BLINK_MS {
                    assert!(second.1, "the second shut follows the gap");
                    assert_eq!(gap.0 - first.0, u64::from(BLINK_MS));
                    assert_eq!(second.0 - first.0, 260, "seed {seed}");
                    assert_eq!(second.2, BLINK_MS);
                    found += 1;
                }
            }
        }
        assert_eq!(EYES_DOUBLE_MS, 260);
        assert!(found > 20, "{found} doubles seen");
    }

    #[test]
    fn a_single_blink_goes_straight_back_to_open() {
        let mut found = 0;
        for seed in 0..40u64 {
            let log = eyes_blink_run(seed, 60);
            for pair in log.windows(2) {
                // Shut, then open with the 3 to 6 s wait ahead: no gap.
                if pair[0].1 && !pair[1].1 && pair[1].2 >= EYES_BLINK_MIN_MS {
                    assert_eq!(pair[1].0 - pair[0].0, u64::from(BLINK_MS));
                    found += 1;
                }
            }
        }
        assert!(found > 100, "{found}");
    }

    #[test]
    fn the_eyes_boxes_follow_the_offset_and_stay_in_the_pill() {
        use super::super::geometry::{EYES_W, ShapeKind, shape_for};
        for style in [Style::Pill, Style::Notch] {
            let pill = shape_for(style, ShapeKind::Eyes);
            let centre = eyes_centre(pill);
            assert_eq!(centre, (pill.x + EYES_W / 2.0, pill.y + EYES_H / 2.0));
            for blink in [false, true] {
                let still = eyes_boxes_at(centre, 0.0, 0.0, blink);
                let slid = eyes_boxes_at(centre, 3.0, 1.0, blink);
                assert_eq!(still.len(), 2, "two eyes");
                for (a, b) in still.iter().zip(&slid) {
                    assert!((b.0 - a.0 - 3.0).abs() < 1e-4 && (b.1 - a.1 - 1.0).abs() < 1e-4);
                    assert!((b.2 - a.2 - 3.0).abs() < 1e-4 && (b.3 - a.3 - 1.0).abs() < 1e-4);
                }
                // Centred, the eyes are symmetric about the pill's middle.
                let (left, right) = (still[0], still[1]);
                assert!(((centre.0 - left.0) - (right.2 - centre.0)).abs() < 1e-4);
            }
            // An open eye is 8 dip tall; the blink's lines are far flatter.
            let open = eyes_boxes_at(centre, 0.0, 0.0, false);
            assert!((open[0].3 - open[0].1 - 8.0).abs() < 1e-4);
            let shut = eyes_boxes_at(centre, 0.0, 0.0, true);
            assert!(shut[0].3 - shut[0].1 < 2.0);
            // A request in range passes untouched, one past it is cut back
            // (and NaN is no slide).
            assert_eq!(eyes_offset_in(style, pill, 4.0, 1.5), (4.0, 1.5));
            let (x, y) = eyes_offset_in(style, pill, 40.0, -40.0);
            assert!(x > 0.0 && x < 40.0 && y < 0.0 && y > -40.0, "{x} {y}");
            assert_eq!(eyes_offset_in(style, pill, f32::NAN, f32::NAN), (0.0, 0.0));
        }
    }

    #[test]
    fn sleep_schedule() {
        let t0 = Instant::now();
        let ten = Duration::from_secs(600);
        assert!(!asleep(t0, t0 + Duration::from_secs(599), ten));
        assert!(asleep(t0, t0 + ten, ten));
        assert_eq!(time_to_sleep(t0, t0, ten), Some(ten));
        assert_eq!(
            time_to_sleep(t0, t0 + Duration::from_secs(60), ten),
            Some(Duration::from_secs(540))
        );
        assert_eq!(time_to_sleep(t0, t0 + ten, ten), None);
        assert_eq!(time_to_sleep(t0, t0 + 2 * ten, ten), None);
        // The brain: activity restarts the clock; hover wakes it.
        let mut b = Brain::new(t0);
        let later = t0 + ten;
        assert!(b.inputs(Spot::Idle, later, ten, Motion::Full).asleep);
        assert!(
            b.inputs(Spot::Idle, later, ten, Motion::Saver).asleep,
            "Hemat still sleeps"
        );
        assert!(
            !b.inputs(Spot::Idle, later, ten, Motion::Off).asleep,
            "Mati never sleeps"
        );
        assert_eq!(b.next_change(t0, ten), Some(t0 + ten));
        b.hover(true, later);
        assert!(!b.inputs(Spot::Idle, later, ten, Motion::Full).asleep);
        b.hover(false, later);
        b.activity(later);
        assert_eq!(b.next_change(later, ten), Some(later + ten));
    }

    #[test]
    fn sleep_setting_steps_and_clamps() {
        assert_eq!(clamp_sleep(0), 1);
        assert_eq!(clamp_sleep(999), 60);
        assert_eq!(sleep_step(10, 1), 15);
        assert_eq!(sleep_step(10, -1), 5);
        assert_eq!(sleep_step(5, -1), 4);
        assert_eq!(sleep_step(1, -1), 1);
        assert_eq!(sleep_step(4, 1), 5);
        assert_eq!(sleep_step(60, 1), 60);
        assert_eq!(sleep_step(12, 1), 15, "off-grid snaps");
        assert_eq!(sleep_step(12, -1), 10);
    }

    #[test]
    fn claude_works_until_done_or_quiet() {
        let t0 = Instant::now();
        let mut b = Brain::new(t0);
        assert!(!b.claude_working(t0));
        b.claude_activity("a", t0);
        assert!(b.claude_working(t0 + Duration::from_secs(119)));
        assert!(!b.claude_working(t0 + CLAUDE_WORKING_FOR), "gone quiet");
        let far = Duration::from_secs(3600);
        assert_eq!(b.next_change(t0, far), Some(t0 + CLAUDE_WORKING_FOR));
        b.claude_ended("a");
        assert!(!b.claude_working(t0));
        assert_eq!(b.next_change(t0, far), Some(t0 + far));
    }

    #[test]
    fn motion_off_still_shrinks_the_typing_island_when_claude_goes_quiet() {
        let t0 = Instant::now();
        let far = Duration::from_secs(3600);
        let mut b = Brain::new(t0);
        b.claude_activity("a", t0);
        b.claude_activity("b", t0 + Duration::from_secs(30));
        let quiet = t0 + CLAUDE_WORKING_FOR;
        assert_eq!(b.next_quiet(t0), Some(quiet));
        assert_eq!(
            b.next_quiet(quiet),
            Some(quiet + Duration::from_secs(30)),
            "the next session"
        );
        let face = Some(Expr::Typing);
        // Full and Saver: the usual one-shot.
        for motion in [Motion::Full, Motion::Saver] {
            let plan = plan_timers(face, motion, false);
            assert_eq!(change_at(&b, plan, true, t0, far), b.next_change(t0, far));
        }
        // Off: only for the typing shape, only when a session goes quiet.
        let off = plan_timers(face, Motion::Off, false);
        assert!(!off.change);
        assert_eq!(change_at(&b, off, true, t0, far), Some(quiet));
        assert_eq!(change_at(&b, off, false, t0, far), None);
        b.claude_ended("a");
        b.claude_ended("b");
        assert_eq!(
            change_at(&b, off, true, t0, far),
            None,
            "nothing to wait for"
        );
    }

    #[test]
    fn cli_faces_last_a_few_seconds() {
        let t0 = Instant::now();
        let mut b = Brain::new(t0);
        b.cli_started();
        assert_eq!(b.cli(t0), Cli::Running);
        b.cli_finished(false, t0);
        assert_eq!(b.cli(t0 + Duration::from_secs(3)), Cli::Failed);
        assert_eq!(b.cli(t0 + CLI_MOOD_FOR), Cli::None);
        let far = Duration::from_secs(3600);
        assert_eq!(b.next_change(t0, far), Some(t0 + CLI_MOOD_FOR));
        b.cli_finished(true, t0);
        assert_eq!(b.cli(t0), Cli::Done);
        // Mati: no timer could end it, so it never shows.
        assert_eq!(b.inputs(Spot::Idle, t0, far, Motion::Off).cli, Cli::None);
    }

    #[test]
    fn alert_bounces_twice_then_rests() {
        let ms = Duration::from_millis;
        assert_eq!(bounce_offset(ms(0)), 0.0);
        assert!((bounce_offset(ms(BOUNCE_MS / 2)) - BOUNCE_PX).abs() < 1e-3);
        assert!((bounce_offset(ms(BOUNCE_MS + BOUNCE_MS / 2)) - BOUNCE_PX).abs() < 1e-3);
        assert_eq!(bounce_offset(ms(2 * BOUNCE_MS)), 0.0);
        assert_eq!(bounce_offset(ms(10_000)), 0.0);
        // The brain starts it only when an alert appears, and with motion.
        let t0 = Instant::now();
        let mut b = Brain::new(t0);
        assert!(b.observe(Some(Expr::Alert), None, t0, true));
        assert!(!b.observe(Some(Expr::Alert), None, t0, true), "already showing");
        let alert = Spot::Notice(NoticeSort::Reminder);
        assert!(b.moving(alert, t0 + ms(100), true, Mascot::Cat));
        assert!(!b.moving(alert, t0 + ms(2 * BOUNCE_MS), true, Mascot::Cat));
        b.observe(Some(Expr::Idle), None, t0, true);
        assert!(!b.observe(Some(Expr::Alert), None, t0, false), "Hemat: no bounce");
        assert!(!b.moving(alert, t0, true, Mascot::Cat));
    }

    #[test]
    fn the_falk_hops_once_per_happy_notice() {
        let t0 = Instant::now();
        let mut b = Brain::new(t0);
        let n = Spot::Notice(NoticeSort::ClaudeDone);
        b.observe(Some(Expr::Happy), Some(1), t0, true);
        let at = |ms| {
            b.pose(n, t0 + Duration::from_millis(ms), true, false, Mascot::Falk)
                .hop_px
        };
        assert!((at(200) - HOP_PX).abs() < 1e-3);
        assert_eq!(at(400), 0.0);
        assert!(b.moving(n, t0 + Duration::from_millis(399), true, Mascot::Falk));
        assert!(!b.moving(n, t0 + Duration::from_millis(400), true, Mascot::Falk));
        assert_eq!(
            b.pose(n, t0 + Duration::from_millis(200), true, false, Mascot::Cat)
                .hop_px,
            0.0
        );
        assert!(!b.moving(n, t0 + Duration::from_millis(200), true, Mascot::Cat));
        // a second Happy notice restarts it
        let t1 = t0 + Duration::from_secs(2);
        b.observe(Some(Expr::Happy), Some(2), t1, true);
        assert!(
            b.pose(n, t1 + Duration::from_millis(200), true, false, Mascot::Falk)
                .hop_px
                > 1.0
        );
        // the same notice again does not
        b.observe(Some(Expr::Happy), Some(2), t1 + Duration::from_secs(1), true);
        assert_eq!(
            b.pose(n, t1 + Duration::from_millis(1200), true, false, Mascot::Falk)
                .hop_px,
            0.0
        );
    }

    #[test]
    fn hover_and_saver_never_hop_wave_or_flush() {
        let t0 = Instant::now();
        let mut b = Brain::new(t0);
        b.hover(true, t0);
        b.observe(Some(Expr::Happy), None, t0, true);
        assert_eq!(
            b.pose(Spot::Idle, t0 + Duration::from_millis(200), true, false, Mascot::Falk)
                .hop_px,
            0.0
        );
        let mut s = Brain::new(t0);
        s.observe(Some(Expr::Alert), Some(5), t0, false);
        let p = s.pose(
            Spot::Notice(NoticeSort::Reminder),
            t0 + Duration::from_millis(300),
            false,
            false,
            Mascot::Octopus,
        );
        assert_eq!((p.flush, p.hop_px, p.waving), (0.0, 0.0, false));
        assert!(!s.moving(Spot::Notice(NoticeSort::Reminder), t0, false, Mascot::Octopus));
    }

    #[test]
    fn the_octopus_flushes_for_700_ms_on_alert_and_the_bounce_keeps_its_numbers() {
        let t0 = Instant::now();
        let mut b = Brain::new(t0);
        let r = Spot::Notice(NoticeSort::Reminder);
        assert!(b.observe(Some(Expr::Alert), Some(9), t0, true));
        let at = |ms, m| b.pose(r, t0 + Duration::from_millis(ms), true, false, m);
        assert!((at(350, Mascot::Octopus).flush - 1.0).abs() < 1e-3);
        assert_eq!(at(350, Mascot::Cat).flush, 0.0);
        assert!(b.moving(r, t0 + Duration::from_millis(650), true, Mascot::Octopus));
        assert!(!b.moving(r, t0 + Duration::from_millis(650), true, Mascot::Cat)); // the cat's 520 ms
        assert_eq!(at(130, Mascot::Octopus).bounce_px, at(130, Mascot::Cat).bounce_px);
    }

    #[test]
    fn the_greeting_waves_three_times_then_holds() {
        let t0 = Instant::now();
        let mut b = Brain::new(t0);
        let g = Spot::Notice(NoticeSort::Greeting(DayPeriod::Morning));
        b.observe(Some(Expr::Wave), Some(3), t0, true);
        let p = b.pose(g, t0 + Duration::from_millis(75), true, false, Mascot::Octopus);
        assert!(p.waving && (p.wave - 1.0).abs() < 1e-3);
        let held = b.pose(g, t0 + Duration::from_millis(950), true, false, Mascot::Octopus);
        assert!(held.waving && held.wave == 0.0);
        assert!(!b.moving(g, t0 + Duration::from_millis(950), true, Mascot::Falk));
        assert!(
            !b.pose(g, t0 + Duration::from_millis(75), true, false, Mascot::Cat)
                .waving
        );
    }

    #[test]
    fn hover_lifts_two_pixels() {
        let t0 = Instant::now();
        let idle = Spot::Idle;
        let mut b = Brain::new(t0);
        b.hover(true, t0);
        assert_eq!(b.pose(idle, t0, true, false, Mascot::Cat).lift_px, 0.0);
        let up = b.pose(idle, t0 + Duration::from_millis(LIFT_MS), true, false, Mascot::Cat);
        assert_eq!(up.lift_px, LIFT_PX);
        assert!(
            b.moving(idle, t0, true, Mascot::Cat) && !b.moving(idle, t0 + Duration::from_millis(LIFT_MS), true, Mascot::Cat)
        );
        // No motion: no lift, no blink.
        assert_eq!(
            b.pose(idle, t0 + Duration::from_secs(1), false, true, Mascot::Cat),
            Pose::default()
        );
        // A second move while hovering keeps the first arrival.
        b.hover(true, t0 + Duration::from_secs(1));
        assert_eq!(
            b.pose(idle, t0 + Duration::from_secs(1), true, true, Mascot::Cat)
                .lift_px,
            LIFT_PX
        );
    }

    #[test]
    fn hover_lifts_only_a_compact_island() {
        let t0 = Instant::now();
        let mut b = Brain::new(t0);
        b.hover(true, t0);
        let later = t0 + Duration::from_millis(LIFT_MS);
        for spot in [Spot::Idle, Spot::Pomodoro, Spot::Media, Spot::Lyric] {
            assert_eq!(
                b.pose(spot, later, true, false, Mascot::Cat).lift_px,
                LIFT_PX,
                "{spot:?}"
            );
            assert!(b.moving(spot, t0, true, Mascot::Cat), "{spot:?}");
        }
        // A notice keeps its own face, still; the panel draws no robot, so
        // re-entering the island there starts no frame clock.
        for spot in [
            Spot::Notice(NoticeSort::Reminder),
            Spot::Notice(NoticeSort::Greeting(DayPeriod::Morning)),
            Spot::Notice(NoticeSort::BatteryLow),
            Spot::Notice(NoticeSort::SystemLow),
            Spot::Panel,
            Spot::Hidden,
        ] {
            assert_eq!(b.pose(spot, later, true, false, Mascot::Cat).lift_px, 0.0, "{spot:?}");
            assert!(!b.moving(spot, t0, true, Mascot::Cat), "{spot:?}");
        }
    }

    #[test]
    fn motion_policy_and_energy_saver() {
        assert_eq!(Motion::Full.effective(true), Motion::Saver);
        assert_eq!(Motion::Full.effective(false), Motion::Full);
        assert_eq!(Motion::Off.effective(true), Motion::Off);
        assert!(Motion::Full.blinks() && Motion::Full.moves() && Motion::Full.timed());
        assert!(!Motion::Saver.blinks() && !Motion::Saver.moves() && Motion::Saver.timed());
        assert!(!Motion::Off.blinks() && !Motion::Off.timed());
        assert_eq!(Motion::from_name("hemat"), None);
        assert_eq!(Motion::from_name("saver"), Some(Motion::Saver));
    }

    #[test]
    fn timers_run_only_while_a_robot_shows() {
        let plan = |shown: bool, motion, animating| {
            plan_timers(shown.then_some(Expr::Idle), motion, animating)
        };
        // Faces without open eyes never blink, so no blink timer for them.
        for expr in Expr::ALL {
            let blinks = expr.blinks();
            assert_eq!(plan_timers(Some(expr), Motion::Full, false).blink, blinks);
            assert_eq!(face(expr, true) != face(expr, false), blinks, "{expr:?}");
        }
        assert_eq!(
            plan(true, Motion::Full, false),
            TimerPlan {
                blink: true,
                change: true
            }
        );
        // The frame clock draws anyway; the blink waits.
        assert!(!plan(true, Motion::Full, true).blink);
        assert_eq!(
            plan(true, Motion::Saver, false),
            TimerPlan {
                blink: false,
                change: true
            }
        );
        let none = TimerPlan {
            blink: false,
            change: false,
        };
        assert_eq!(plan(true, Motion::Off, false), none);
        for m in [Motion::Full, Motion::Saver, Motion::Off] {
            assert_eq!(plan(false, m, false), none, "{m:?}");
        }
    }

    #[test]
    fn greeting_once_per_period_per_day() {
        use DayPeriod::*;
        use chrono::NaiveDate;
        let now = |d, h, m| {
            NaiveDate::from_ymd_opt(2026, 10, d)
                .unwrap()
                .and_hms_opt(h, m, 0)
                .unwrap()
        };
        let mark = |d, period| {
            Some(GreetMark {
                date: NaiveDate::from_ymd_opt(2026, 10, d).unwrap(),
                period,
            })
        };
        let on = GreetSettings {
            robot: true,
            greeting: true,
        };
        // Nothing yet: the period of the moment, at any hour.
        for (h, m, want) in [
            (7, 30, Morning),
            (11, 0, Midday),
            (16, 0, Afternoon),
            (19, 0, Evening),
            (2, 0, Night),
        ] {
            assert_eq!(should_greet(None, now(27, h, m), on), Some(want), "{h}:{m}");
        }
        // The noon limit is gone: a day's first greeting may come at 14:00.
        assert_eq!(
            should_greet(mark(26, Evening), now(27, 14, 0), on),
            Some(Midday)
        );
        // Each period greets once.
        for (h, p) in [
            (8, Morning),
            (12, Midday),
            (16, Afternoon),
            (20, Evening),
            (1, Night),
        ] {
            assert_eq!(should_greet(mark(27, p), now(27, h, 5), on), None, "{p:?}");
        }
        // An old morning mark (a plain date) blocks the morning, not midday.
        assert_eq!(should_greet(mark(27, Morning), now(27, 8, 0), on), None);
        assert_eq!(
            should_greet(mark(27, Morning), now(27, 11, 0), on),
            Some(Midday)
        );
        // A later period the same day greets again; so does the next day.
        assert_eq!(
            should_greet(mark(27, Midday), now(27, 18, 0), on),
            Some(Evening)
        );
        assert_eq!(
            should_greet(mark(27, Evening), now(28, 19, 0), on),
            Some(Evening)
        );
        // Night is 00:00-03:59 of its own calendar date.
        assert_eq!(
            should_greet(mark(27, Evening), now(28, 0, 30), on),
            Some(Night)
        );
        // The switches.
        assert_eq!(
            should_greet(
                None,
                now(27, 8, 0),
                GreetSettings {
                    greeting: false,
                    ..on
                }
            ),
            None
        );
        assert_eq!(
            should_greet(None, now(27, 8, 0), GreetSettings { robot: false, ..on }),
            None
        );
    }

    #[test]
    fn nod_amount_clamps_and_rests_on_junk() {
        assert_eq!(nod_amount(0.0), 0.0);
        assert_eq!(nod_amount(1.0), 1.0);
        assert_eq!(nod_amount(0.4), 0.4);
        assert_eq!(nod_amount(2.0), 1.0);
        assert_eq!(nod_amount(f32::NAN), 0.0);
        assert_eq!(nod_amount(-1.0), 0.0);
        assert_eq!(nod_amount(f32::INFINITY), 1.0);
        assert_eq!(nod_amount(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn the_fallback_beat_is_a_600_ms_swell() {
        let at = |ms: u64| fallback_beat(Duration::from_millis(ms));
        assert_eq!(at(0), 0.0);
        assert!(at(600) < 1e-6, "{}", at(600));
        assert!(at(1_200) < 1e-6);
        assert!((at(300) - 1.0).abs() < 1e-6);
        assert!((at(900) - 1.0).abs() < 1e-6);
        // Rises to mid-beat, falls after it, and repeats.
        assert!(at(100) < at(200) && at(200) < at(300));
        assert!(at(400) > at(500));
        assert!((at(150) - at(750)).abs() < 1e-5);
        for ms in (0..5_000).step_by(7) {
            let v = at(ms);
            assert!((0.0..=1.0).contains(&v), "{ms} ms: {v}");
        }
    }

    #[test]
    fn the_note_loops_every_1200_ms_rising_then_fading() {
        let at = |ms: u64| note_phase(Duration::from_millis(ms));
        assert_eq!(at(0), (0.0, 0.0), "back at its rest place, fully visible");
        let (rise, fade) = at(1_199);
        assert!(rise > 0.999 && fade > 0.99, "{rise} {fade}");
        // Periodic: the loop starts again.
        assert_eq!(at(1_200), (0.0, 0.0));
        assert_eq!(at(2_400 + 300), at(300));
        // Rises all the way (easing out), faded only in the second half.
        let mut last = 0.0;
        for ms in (0..1_200).step_by(10) {
            let (rise, fade) = at(ms);
            assert!(rise >= last, "{ms} ms");
            last = rise;
            assert!((0.0..=1.0).contains(&rise) && (0.0..=1.0).contains(&fade));
            assert_eq!(fade == 0.0, ms <= 600, "{ms} ms: {fade}");
        }
        // Ease out: more than half way up by the half loop.
        assert!(at(600).0 > 0.5);
        assert!((at(900).1 - 0.5).abs() < 1e-5);
    }

    #[test]
    fn the_note_rises_only_as_far_as_the_room_allows() {
        // Plenty of room: rises the full way, in proportion to `rise`.
        assert_eq!(note_lift_dip(10.0, 0.0), 0.0);
        assert_eq!(note_lift_dip(10.0, 1.0), NOTE_RISE_DIP);
        assert_eq!(note_lift_dip(10.0, 0.5), NOTE_RISE_DIP / 2.0);
        // Exactly the rise plus the margin.
        assert_eq!(note_lift_dip(NOTE_RISE_DIP + NOTE_MARGIN_DIP, 1.0), 3.0);
        // A small room caps it, leaving the margin.
        let lift = note_lift_dip(2.1, 1.0);
        assert!((lift - 1.6).abs() < 1e-6, "{lift}");
        assert!((note_lift_dip(2.1, 0.5) - 0.8).abs() < 1e-6);
        // Exactly the margin: no room to rise.
        assert_eq!(note_lift_dip(NOTE_MARGIN_DIP, 1.0), 0.0);
        // Less than the margin (or above the shape): shifted down by the
        // shortfall, and it does not rise whatever `rise` is.
        for rise in [0.0, 0.5, 1.0] {
            assert_eq!(note_lift_dip(0.0, rise), -NOTE_MARGIN_DIP);
            assert!((note_lift_dip(-2.0, rise) + 2.5).abs() < 1e-6);
        }
        // A rise outside 0..=1 is clamped.
        assert_eq!(note_lift_dip(10.0, 2.0), NOTE_RISE_DIP);
        assert_eq!(note_lift_dip(10.0, -1.0), 0.0);
    }

    #[test]
    fn the_notes_top_is_the_top_of_the_note_marks() {
        use super::mark_bounds;
        for expr in Expr::ALL {
            let top = notes_top(expr);
            let notes: Vec<f32> = face(expr, false)
                .parts
                .iter()
                .filter(|p| is_note(p))
                .map(|p| mark_bounds(p).1)
                .collect();
            match top {
                Some(t) => {
                    assert!(matches!(expr, Expr::Music | Expr::Sing), "{expr:?}");
                    let lowest = notes.iter().cloned().fold(f32::MAX, f32::min);
                    assert!((t - lowest).abs() < 1e-5, "{expr:?}: {t} vs {lowest}");
                }
                None => {
                    // No note marks, except Think's cream dots (not a note).
                    assert!(
                        notes.is_empty() || expr == Expr::Think,
                        "{expr:?} has notes"
                    );
                }
            }
        }
        // The singing cat's upper note sits above the top of the grid.
        assert!(notes_top(Expr::Sing).unwrap() < 0.0);
        assert!(notes_top(Expr::Music).unwrap() > 0.0);
    }

    #[test]
    fn the_cat_dances_only_where_every_condition_holds() {
        // Every Motion, saver, animations off, paused, timer off, every Expr.
        for motion in [Motion::Full, Motion::Saver, Motion::Off] {
            for saver in [false, true] {
                let effective = motion.effective(saver);
                for expr in Expr::ALL {
                    let on = dance_on(true, true, effective, true, true, true, expr);
                    let want = motion == Motion::Full
                        && !saver
                        && matches!(expr, Expr::Music | Expr::Sing);
                    assert_eq!(on, want, "{motion:?} saver {saver} {expr:?}");
                }
            }
        }
        let all = |robot, dance, anim, playing, timer| {
            dance_on(
                robot,
                dance,
                Motion::Full,
                anim,
                playing,
                timer,
                Expr::Music,
            )
        };
        assert!(all(true, true, true, true, true));
        assert!(!all(false, true, true, true, true), "cat off");
        assert!(!all(true, false, true, true, true), "setting off");
        assert!(
            !all(true, true, false, true, true),
            "Windows animations off"
        );
        assert!(!all(true, true, true, false, true), "paused");
        assert!(!all(true, true, true, true, false), "timer off");
        assert!(dance_on(
            true,
            true,
            Motion::Full,
            true,
            true,
            true,
            Expr::Sing
        ));
        assert!(!dance_on(
            true,
            true,
            Motion::Full,
            true,
            true,
            true,
            Expr::Happy
        ));
    }

    #[test]
    fn a_pose_without_the_dance_is_the_pose_as_it_was() {
        let rest = Pose::default();
        assert_eq!(
            (rest.nod_dip, rest.squash, rest.note_dy_dip, rest.note_fade),
            (0.0, 0.0, 0.0, 0.0)
        );
        assert!(!rest.dancing);
        let area = Rect {
            x: 10.0,
            y: 1.0,
            w: 26.0,
            h: 26.0,
        };
        // Not dancing: the renderer's note placement leaves it alone.
        assert_eq!(with_note_lift(rest, Expr::Sing, area, 0.0), rest);
        // The dance sets the nod, the squash and the note, and nothing else.
        let p = Pose::default().dance(1.0, 0.5, 0.3);
        assert_eq!((p.nod_dip, p.squash), (NOD_MAX_DIP, 1.0));
        assert_eq!((p.note_rise, p.note_fade, p.dancing), (0.5, 0.3, true));
        assert_eq!((p.lift_px, p.head_dx_dip, p.blink), (0.0, 0.0, false));
        let half = Pose::default().dance(0.5, 0.0, 0.0);
        assert_eq!((half.nod_dip, half.squash), (NOD_MAX_DIP / 2.0, 0.5));
        let junk = Pose::default().dance(f32::NAN, 0.0, 0.0);
        assert_eq!((junk.nod_dip, junk.squash), (0.0, 0.0));
        // A face without a note gets no note offset.
        assert_eq!(with_note_lift(p, Expr::Happy, area, 0.0), p);
    }

    #[test]
    fn a_dancing_note_is_placed_inside_the_shape() {
        // A 28 dip tall notch: the cat's 26 px slot is 1 px from the top.
        let area = Rect {
            x: 10.0,
            y: 1.0,
            w: 26.0,
            h: 26.0,
        };
        for expr in [Expr::Music, Expr::Sing] {
            let top = notes_top(expr).unwrap();
            for amount in [0.0, 1.0] {
                for rise in [0.0, 0.5, 1.0] {
                    let p =
                        with_note_lift(Pose::default().dance(amount, rise, 0.0), expr, area, 0.0);
                    let k = area.w / GRID;
                    let squashed = 54.0 + (top - 54.0) * (1.0 - SQUASH_H * p.squash);
                    let y = area.y + p.nod_dip + squashed * k + p.note_dy_dip;
                    assert!(
                        y >= NOTE_MARGIN_DIP - 1e-4,
                        "{expr:?} amount {amount} rise {rise}: top at {y}"
                    );
                }
            }
        }
    }

    // ---- The typing cat ----

    #[test]
    fn work_running_makes_the_idle_cat_type_never_think() {
        let base = inputs(Spot::Idle);
        let e = |i: RobotInputs| choose_expr(&i);
        let claude = RobotInputs {
            claude_working: true,
            ..base
        };
        let cli = RobotInputs {
            cli: Cli::Running,
            ..base
        };
        let mole = RobotInputs {
            mole_running: true,
            ..base
        };
        for (what, i) in [("claude", claude), ("cli", cli), ("mole", mole)] {
            assert_eq!(e(i), Some(Expr::Typing), "{what}");
            // The shape wants the laptop, hovering or not (hover shows Happy).
            assert!(typing_wanted(&i), "{what}");
            let hover = RobotInputs { hover: true, ..i };
            assert_eq!(e(hover), Some(Expr::Happy), "{what} hover");
            assert!(typing_wanted(&hover), "{what} hover");
            // A command that just ended still wins.
            let done = RobotInputs {
                cli: Cli::Done,
                ..i
            };
            let failed = RobotInputs {
                cli: Cli::Failed,
                ..i
            };
            assert_eq!(e(done), Some(Expr::Happy), "{what}");
            assert_eq!(e(failed), Some(Expr::Sad), "{what}");
            assert!(!typing_wanted(&done) && !typing_wanted(&failed), "{what}");
            // Media, lyric, pomodoro and notices are as they were.
            assert_eq!(
                e(RobotInputs {
                    spot: Spot::Media,
                    ..i
                }),
                Some(Expr::Music)
            );
            assert_eq!(
                e(RobotInputs {
                    spot: Spot::Lyric,
                    ..i
                }),
                Some(Expr::Sing)
            );
            assert_eq!(
                e(RobotInputs {
                    spot: Spot::Pomodoro,
                    ..i
                }),
                Some(Expr::Focus)
            );
            assert_eq!(
                e(RobotInputs {
                    spot: Spot::Notice(NoticeSort::ClaudeWaiting),
                    ..i
                }),
                Some(Expr::Alert)
            );
            assert!(
                !typing_wanted(&RobotInputs {
                    spot: Spot::Media,
                    ..i
                }),
                "{what}"
            );
        }
        assert_eq!(e(base), Some(Expr::Idle));
        assert!(!typing_wanted(&base));
        // Think stays a face (the demo sheet, the Claude notice scene) but no
        // input ever chooses it.
        let spots = [
            Spot::Hidden,
            Spot::Panel,
            Spot::Idle,
            Spot::Pomodoro,
            Spot::Media,
            Spot::Lyric,
            Spot::Notice(NoticeSort::App),
            Spot::Notice(NoticeSort::Reminder),
            Spot::Notice(NoticeSort::ClaudeWaiting),
            Spot::Notice(NoticeSort::ClaudeDone),
            Spot::Notice(NoticeSort::BatteryLow),
            Spot::Notice(NoticeSort::MoleDone),
        ];
        for spot in spots {
            for bits in 0..32u32 {
                for cli in [Cli::None, Cli::Running, Cli::Done, Cli::Failed] {
                    let i = RobotInputs {
                        spot,
                        hover: bits & 1 != 0,
                        asleep: bits & 2 != 0,
                        claude_working: bits & 4 != 0,
                        cli,
                        mole_running: bits & 8 != 0,
                        battery_low: bits & 16 != 0,
                    };
                    assert_ne!(e(i), Some(Expr::Think), "{i:?}");
                }
            }
        }
        assert!(Expr::ALL.contains(&Expr::Think));
        assert_eq!(Expr::ALL.len(), 11);
        assert_eq!(Expr::ALL[10], Expr::Typing);
    }

    #[test]
    fn the_typing_face_blinks_glances_and_never_turns_its_head() {
        assert!(Expr::Typing.blinks());
        assert!(!Expr::Typing.looks());
        assert!(Expr::Typing.glances());
        assert!(Expr::Typing.follows());
        for expr in Expr::ALL {
            assert_eq!(expr.glances(), expr == Expr::Typing, "{expr:?}");
            assert_eq!(expr.follows(), expr.looks() || expr.glances(), "{expr:?}");
            if expr != Expr::Typing {
                assert_eq!(face(expr, false).eye_shift, (0.0, 0.0), "{expr:?}");
            }
        }
        // The idle pills, shifted toward the laptop: centres (20.5, 41) and
        // (36.5, 41); the cat leans 4 degrees; no extras, the ears at rest.
        let open = face(Expr::Typing, false);
        assert_eq!(open.parts, face(Expr::Idle, false).parts);
        assert_eq!(open.eye_shift, (-3.5, 1.0));
        assert_eq!(
            (open.ears, open.bob, open.tilt),
            (Ears::default(), 0.0, -4.0)
        );
        let centre = |part: &Part, shift: (f32, f32)| match *part {
            Part::Pill { x, y, w, h, .. } => (x + w / 2.0 + shift.0, y + h / 2.0 + shift.1),
            _ => panic!("an eye is a pill"),
        };
        assert_eq!(centre(&open.parts[0], open.eye_shift), (20.5, 41.0));
        assert_eq!(centre(&open.parts[1], open.eye_shift), (36.5, 41.0));
        // A blink gives the Blink lines, shifted the same way.
        let shut = face(Expr::Typing, true);
        assert_eq!(shut.parts, vec![Part::Stroke(Line::Blink)]);
        assert_eq!(shut.eye_shift, (-3.5, 1.0));
        assert_eq!(shut.ears, Ears::default());
        // The pose carries the glance for the eyes only.
        let p = Pose::default().glancing(1.0, -1.0);
        assert_eq!((p.glance_x, p.glance_y), (GLANCE_X, -GLANCE_Y));
        assert_eq!((p.head_dx_dip, p.tilt_deg, p.eye_dx_dip), (0.0, 0.0, 0.0));
        assert_eq!(Pose::default().glancing(0.0, 0.0), Pose::default());
        assert_eq!(Pose::default().paw_lift, [0.0, 0.0]);
    }

    #[test]
    fn the_glance_maps_and_clamps() {
        assert_eq!((GLANCE_X, GLANCE_Y, PAW_LIFT), (4.0, 1.5, 1.6));
        assert_eq!(glance_offset(0.0, 0.0), (0.0, 0.0));
        assert_eq!(glance_offset(1.0, 1.0), (4.0, 1.5));
        assert_eq!(glance_offset(-1.0, -1.0), (-4.0, -1.5));
        assert_eq!(glance_offset(0.5, -0.5), (2.0, -0.75));
        assert_eq!(glance_offset(9.0, -9.0), (4.0, -1.5));
        assert_eq!(glance_offset(f32::NAN, 1.0), (0.0, 1.5));
        assert_eq!(glance_offset(1.0, f32::NAN), (4.0, 0.0));
        assert_eq!(glance_offset(f32::INFINITY, f32::NEG_INFINITY), (4.0, -1.5));
    }

    /// The head's outline as a closed polygon in grid units (the real path's
    /// curves sampled).
    fn head_polygon() -> Vec<(f32, f32)> {
        let mut points = Vec::new();
        let mut at = (0.0f32, 0.0f32);
        for seg in head_path() {
            match seg {
                Seg::Move(x, y) | Seg::Line(x, y) => {
                    points.push((x, y));
                    at = (x, y);
                }
                Seg::Quad(cx, cy, x, y) => {
                    for i in 1..=16 {
                        let t = i as f32 / 16.0;
                        let u = 1.0 - t;
                        points.push((
                            u * u * at.0 + 2.0 * u * t * cx + t * t * x,
                            u * u * at.1 + 2.0 * u * t * cy + t * t * y,
                        ));
                    }
                    at = (x, y);
                }
                Seg::Cubic(..) | Seg::Close => {}
            }
        }
        points
    }

    /// How far inside the polygon `p` is: negative when outside.
    fn depth_in(poly: &[(f32, f32)], p: (f32, f32)) -> f32 {
        let mut inside = false;
        let mut nearest = f32::MAX;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
                inside = !inside;
            }
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len2 = dx * dx + dy * dy;
            let t = if len2 == 0.0 {
                0.0
            } else {
                (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
            };
            nearest = nearest.min((p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy));
        }
        if inside { nearest } else { -nearest }
    }

    #[test]
    fn the_glancing_eyes_stay_inside_the_head_at_every_extreme() {
        let head = head_polygon();
        for blink in [false, true] {
            let f = face(Expr::Typing, blink);
            for (vx, vy) in [
                (-1.0, -1.0),
                (-1.0, 1.0),
                (1.0, -1.0),
                (1.0, 1.0),
                (0.0, 0.0),
                (-1.0, 0.0),
                (1.0, 0.0),
                (0.0, 1.0),
                (0.0, -1.0),
            ] {
                let p = Pose::default().glancing(vx, vy);
                for part in &f.parts {
                    let (l, t, r, b) = mark_bounds(part);
                    let (dx, dy) = (f.eye_shift.0 + p.glance_x, f.eye_shift.1 + p.glance_y);
                    for corner in [(l, t), (r, t), (r, b), (l, b)] {
                        let at = (corner.0 + dx, corner.1 + dy);
                        let depth = depth_in(&head, at);
                        assert!(
                            depth >= 0.5,
                            "blink {blink} glance ({vx}, {vy}): {at:?} is only {depth} inside the head"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_paws_take_turns_with_beats_and_pauses() {
        let mut rng = Rng::new(0x5EED_CAFE);
        let mut paws = TypingPaws::default();
        assert_eq!((paws.up(), paws.lift()), (None, [0.0, 0.0]));
        let steps = 20_000u32;
        let (mut gaps, mut moving, mut pauses) = (0u32, 0u32, 0u32);
        let (mut clock, mut first_pause, mut last_pause) = (0u64, None, 0u64);
        let mut last_paw = None;
        let mut in_pause = false;
        for _ in 0..steps {
            let wait = paws.next(&mut rng);
            assert!(
                (TYPE_STEP_MIN_MS..=TYPE_STEP_MAX_MS).contains(&wait)
                    || wait == TYPE_GAP_MS
                    || wait == TYPE_PAUSE_MS,
                "wait {wait}"
            );
            match paws.up() {
                None => {
                    // A pause: both paws down for 900 ms.
                    assert_eq!(wait, TYPE_PAUSE_MS);
                    assert_eq!(paws.lift(), [0.0, 0.0]);
                    pauses += 1;
                    in_pause = true;
                    first_pause.get_or_insert(clock);
                    last_pause = clock;
                }
                Some(paw) => {
                    assert_ne!(wait, TYPE_PAUSE_MS);
                    moving += 1;
                    gaps += u32::from(wait == TYPE_GAP_MS);
                    // The paw that is up alternates, across pauses too.
                    if let Some(last) = last_paw {
                        assert_ne!(paw, last, "the same paw twice");
                    }
                    last_paw = Some(paw);
                    in_pause = false;
                    let lift = paws.lift();
                    match paw {
                        Side::Left => assert_eq!(lift, [PAW_LIFT, 0.0]),
                        Side::Right => assert_eq!(lift, [0.0, PAW_LIFT]),
                    }
                }
            }
            clock += u64::from(wait);
        }
        let _ = in_pause;
        let share = f64::from(gaps) / f64::from(moving);
        assert!((0.09..=0.15).contains(&share), "420 ms share {share}");
        assert!(pauses > 100, "{pauses} pauses");
        let mean = (last_pause - first_pause.unwrap()) as f64 / f64::from(pauses - 1) / 1000.0;
        assert!((6.0..=10.0).contains(&mean), "a pause every {mean} s");
        // The very first step lifts the left paw; reset puts both down.
        let mut fresh = TypingPaws::default();
        fresh.next(&mut Rng::new(7));
        assert_eq!(fresh.up(), Some(Side::Left));
        fresh.reset();
        assert_eq!((fresh.up(), fresh.lift()), (None, [0.0, 0.0]));
        assert_eq!(fresh, TypingPaws::default());
    }

    #[test]
    fn the_paws_move_only_where_the_gate_allows() {
        let on = |expr, motion, anim, full, away, shrunk| {
            typing_anim_on(expr, motion, anim, full, away, shrunk)
        };
        assert!(on(
            Some(Expr::Typing),
            Motion::Full,
            true,
            false,
            false,
            false
        ));
        // Every argument flipped in turn.
        assert!(!on(
            Some(Expr::Idle),
            Motion::Full,
            true,
            false,
            false,
            false
        ));
        assert!(!on(None, Motion::Full, true, false, false, false));
        assert!(!on(
            Some(Expr::Typing),
            Motion::Full,
            false,
            false,
            false,
            false
        ));
        assert!(!on(
            Some(Expr::Typing),
            Motion::Full,
            true,
            true,
            false,
            false
        ));
        assert!(!on(
            Some(Expr::Typing),
            Motion::Full,
            true,
            false,
            true,
            false
        ));
        assert!(!on(
            Some(Expr::Typing),
            Motion::Full,
            true,
            false,
            false,
            true
        ));
        // Only full motion (Energy Saver already folded in) moves them.
        for (motion, want) in [
            (Motion::Full, true),
            (Motion::Saver, false),
            (Motion::Off, false),
        ] {
            assert_eq!(
                on(Some(Expr::Typing), motion, true, false, false, false),
                want,
                "{motion:?}"
            );
        }
        assert!(!on(
            Some(Expr::Typing),
            Motion::Full.effective(true),
            true,
            false,
            false,
            false
        ));
        for expr in Expr::ALL {
            assert_eq!(
                on(Some(expr), Motion::Full, true, false, false, false),
                expr == Expr::Typing,
                "{expr:?}"
            );
        }
    }

    #[test]
    fn the_laptop_box_reaches_left_of_the_cat() {
        let area = Rect {
            x: 100.0,
            y: 3.0,
            w: 64.0,
            h: 64.0,
        };
        let r = laptop_rect(area);
        assert_eq!((r.x, r.w), (73.0, 36.0));
        assert!(
            (r.y - 30.3).abs() < 1e-4 && (r.h - 29.1).abs() < 1e-4,
            "{r:?}"
        );
        // Every laptop and paw point lies inside the box, the paws' lift and
        // outline aside (a paw raised past the lid's top is still low).
        for (x, y) in typing_points([0.0, 0.0]) {
            assert!(x >= -27.0 - 1e-4 && x <= 13.8 + 1e-4, "{x}");
            assert!(y >= 27.3 - 0.21 && y <= 56.4 + 1e-4, "{y}");
        }
        // The paws' lift moves their boxes up by exactly that much.
        let rest = typing_points([0.0, 0.0]);
        let lifted = typing_points([PAW_LIFT, 0.0]);
        assert_eq!(rest.len(), lifted.len());
        let moved: Vec<_> = rest
            .iter()
            .zip(&lifted)
            .filter(|(a, b)| a != b)
            .map(|(a, b)| a.1 - b.1)
            .collect();
        assert_eq!(moved.len(), 4);
        assert!(moved.iter().all(|d| (d - PAW_LIFT).abs() < 1e-5));
    }

    #[test]
    fn a_working_session_after_music_gives_a_typing_cat_that_glances() {
        use crate::pointer_watch::{GlanceRest, LookEase, glance_target, look_allowed, look_on};
        let t0 = Instant::now();
        let after = Duration::from_secs(600);
        let mut brain = Brain::new(t0);
        brain.claude_activity("session", t0);
        let face_at = |brain: &Brain, spot| {
            choose_expr(&brain.inputs(spot, t0 + Duration::from_secs(5), after, Motion::Full))
        };
        // The reported case: the music ends, the island is idle again, and a
        // Claude session was active: Typing, not Think (which never looked).
        assert_eq!(face_at(&brain, Spot::Media), Some(Expr::Music));
        let expr = face_at(&brain, Spot::Idle);
        assert_eq!(expr, Some(Expr::Typing));
        let allowed = look_allowed(true, true);
        assert!(look_on(allowed, true, true, false, false, false, expr));
        // "Ikuti kursor" off, Motion Saver (or Energy Saver), another view,
        // shrunk, fullscreen, locked: no.
        assert!(!look_on(
            look_allowed(true, false),
            true,
            true,
            false,
            false,
            false,
            expr
        ));
        assert!(!look_on(
            look_allowed(false, true),
            true,
            true,
            false,
            false,
            false,
            expr
        ));
        assert!(!look_on(
            allowed,
            Motion::Saver.moves(),
            true,
            false,
            false,
            false,
            expr
        ));
        assert!(!look_on(
            allowed,
            Motion::Off.moves(),
            true,
            false,
            false,
            false,
            expr
        ));
        assert!(!look_on(allowed, true, false, false, false, false, expr));
        assert!(!look_on(allowed, true, true, true, false, false, expr));
        assert!(!look_on(allowed, true, true, false, true, false, expr));
        assert!(!look_on(allowed, true, true, false, false, true, expr));
        assert!(!look_on(allowed, true, true, false, false, false, None));
        assert!(!look_on(
            allowed,
            true,
            true,
            false,
            false,
            false,
            Some(Expr::Sleep)
        ));
        // A moved pointer: the glance follows it and eases off zero.
        let mut rest = GlanceRest::default();
        assert!(!rest.reading((100, 100), t0));
        let moved = t0 + Duration::from_millis(33);
        assert!(rest.reading((900, 40), moved));
        let (vx, vy) = glance_target(1500.0, 200.0, 960.0, 0.0, 1920.0, 17.0);
        let mut glance = crate::pointer_watch::EyesLook::default();
        assert!(glance.set_target(vx, vy));
        glance.advance(Duration::from_millis(60));
        let (gx, gy) = glance.value();
        assert!(gx != 0.0 && gy != 0.0, "({gx}, {gy})");
        let p = Pose::default().glancing(gx, gy);
        assert!(p.glance_x > 0.0 && p.glance_y > 0.0);
        // Without Claude, a CLI command or Mole the face is Idle: it looks
        // (the head) exactly as before.
        brain.claude_ended("session");
        let idle = face_at(&brain, Spot::Idle);
        assert_eq!(idle, Some(Expr::Idle));
        assert!(look_on(allowed, true, true, false, false, false, idle));
        let mut look = LookEase::default();
        assert!(look.set_target(0.5));
        look.advance(Duration::from_millis(60));
        assert!(look.value() > 0.0);
    }

    // ---- Sleep follows the PC's input ----

    #[test]
    fn fold_input_keeps_the_later_moment() {
        let base = Instant::now();
        let now = base + Duration::from_secs(1_000);
        let old = base + Duration::from_secs(100);
        // Input 30 s ago is newer than the old activity.
        assert_eq!(
            fold_input(old, now, Duration::from_secs(30)),
            now - Duration::from_secs(30)
        );
        // Input older than the last activity changes nothing.
        let recent = now - Duration::from_secs(5);
        assert_eq!(fold_input(recent, now, Duration::from_secs(30)), recent);
        // Zero idle is `now`, never later.
        assert_eq!(fold_input(old, now, Duration::ZERO), now);
        // An activity already ahead of `now` is not pulled back.
        let ahead = now + Duration::from_secs(3);
        assert_eq!(fold_input(ahead, now, Duration::ZERO), ahead);
    }

    #[test]
    fn fold_input_survives_a_huge_idle() {
        let now = Instant::now();
        assert_eq!(fold_input(now, now, Duration::from_secs(u64::MAX / 4)), now);
        assert_eq!(fold_input(now, now, Duration::MAX), now);
    }

    #[test]
    fn recent_pc_input_postpones_the_sleep() {
        let base = Instant::now();
        let mut brain = Brain::new(base);
        let now = base + Duration::from_secs(20 * 60);
        let after = Duration::from_secs(10 * 60);
        assert!(brain.inputs(Spot::Idle, now, after, Motion::Full).asleep);
        // Input 30 s ago: awake, and the deadline is input + 10 min.
        brain.input_seen(now, Duration::from_secs(30));
        assert!(!brain.inputs(Spot::Idle, now, after, Motion::Full).asleep);
        assert_eq!(
            brain.next_change(now, after),
            Some(now - Duration::from_secs(30) + after)
        );
    }

    #[test]
    fn old_pc_input_leaves_the_cat_asleep() {
        let base = Instant::now();
        let mut brain = Brain::new(base);
        let now = base + Duration::from_secs(30 * 60);
        let after = Duration::from_secs(10 * 60);
        brain.input_seen(now, Duration::from_secs(11 * 60));
        assert!(brain.inputs(Spot::Idle, now, after, Motion::Full).asleep);
        assert_eq!(brain.next_change(now, after), None);
    }

    #[test]
    fn a_sleeping_cat_wakes_on_newer_input() {
        let base = Instant::now();
        let mut brain = Brain::new(base);
        let now = base + Duration::from_secs(30 * 60);
        let after = Duration::from_secs(10 * 60);
        assert!(brain.inputs(Spot::Idle, now, after, Motion::Full).asleep);
        brain.input_seen(now, Duration::from_millis(500));
        assert!(!brain.inputs(Spot::Idle, now, after, Motion::Full).asleep);
        // Input older than the activity just recorded changes nothing.
        let before = brain.next_change(now, after);
        brain.input_seen(now, Duration::from_secs(60));
        assert_eq!(brain.next_change(now, after), before);
    }

    #[test]
    fn shown_is_the_face_last_observed() {
        let now = Instant::now();
        let mut brain = Brain::new(now);
        assert_eq!(brain.shown(), None);
        brain.observe(Some(Expr::Sleep), None, now, true);
        assert_eq!(brain.shown(), Some(Expr::Sleep));
        brain.observe(None, None, now, true);
        assert_eq!(brain.shown(), None);
    }

    #[test]
    fn the_wake_poll_runs_only_for_the_sleep_face_on_the_idle_view() {
        let s = Some(Expr::Sleep);
        let on = |face, spot, motion, shrunk, full, away| {
            wake_poll_on(face, spot, motion, shrunk, full, away)
        };
        assert!(on(s, Spot::Idle, Motion::Full, false, false, false));
        // Hemat keeps sleep, so it keeps the poll.
        assert!(on(s, Spot::Idle, Motion::Saver, false, false, false));
        assert!(!on(s, Spot::Idle, Motion::Off, false, false, false));
        assert!(!on(None, Spot::Idle, Motion::Full, false, false, false));
        for face in [Expr::Idle, Expr::Typing, Expr::Happy] {
            assert!(!on(Some(face), Spot::Idle, Motion::Full, false, false, false));
        }
        for spot in [Spot::Media, Spot::Lyric, Spot::Pomodoro, Spot::Panel] {
            assert!(!on(s, spot, Motion::Full, false, false, false));
        }
        assert!(!on(s, Spot::Idle, Motion::Full, true, false, false));
        assert!(!on(s, Spot::Idle, Motion::Full, false, true, false));
        assert!(!on(s, Spot::Idle, Motion::Full, false, false, true));
    }

    #[test]
    fn the_robot_timer_picks_the_poll_or_the_change() {
        let now = Instant::now();
        let poll = now + Duration::from_millis(1_800);
        let soon = now + Duration::from_millis(500);
        let late = now + Duration::from_secs(60);
        assert_eq!(robot_timer_at(None, true, now), Some((poll, true)));
        assert_eq!(robot_timer_at(Some(late), true, now), Some((poll, true)));
        assert_eq!(robot_timer_at(Some(soon), true, now), Some((soon, false)));
        assert_eq!(robot_timer_at(Some(late), false, now), Some((late, false)));
        assert_eq!(robot_timer_at(None, false, now), None);
    }

    #[test]
    fn the_wake_poll_lands_within_two_seconds() {
        assert!(WAKE_POLL_MS + WAKE_POLL_SLACK_MS <= 2_000);
    }

    #[test]
    fn a_quiet_wake_tick_is_only_sleep_to_sleep() {
        let s = Some(Expr::Sleep);
        assert!(wake_tick_quiet(s, s));
        // Falling asleep: the Sleep face still has to be drawn.
        assert!(!wake_tick_quiet(Some(Expr::Idle), s));
        assert!(!wake_tick_quiet(None, s));
        // Waking.
        assert!(!wake_tick_quiet(s, Some(Expr::Idle)));
        assert!(!wake_tick_quiet(s, None));
    }
}
