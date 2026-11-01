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
    ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::core::Result as WinResult;
use windows_numerics::{Matrix3x2, Vector2};

use super::cat_style::{CatLook, Placed, RIM_WIDTH};
use super::geometry::{EYES_H, Rect, ShapeRect, shape_contains};
use crate::config::Style;
use crate::greeting::{self, DayPeriod, GreetMark};
use crate::i18n::Key;
use crate::model::{NoticeKind, View};
use crate::theme::{CatAccessory, CatColor};

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

/// The ten faces. Blinking is not one of them: it is a `Pose` input.
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
}

impl Expr {
    // The demo sheet (debug builds) and tests list and name every face.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub const ALL: [Expr; 10] = [
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
    ];

    /// Its eyes are open (pills or round): a blink shows. Arcs do not blink.
    pub fn blinks(self) -> bool {
        matches!(
            self,
            Expr::Idle | Expr::Focus | Expr::Alert | Expr::Think | Expr::Sad
        )
    }

    /// The face turns toward the pointer (`Pose::looking_as`): only the faces
    /// with nothing floating beside the head. The thought dots and the z's
    /// sit beside the right ear, a few dip from the idle clock, and would
    /// slide into it; a sleeping or thinking cat does not follow anyway.
    pub fn looks(self) -> bool {
        !matches!(
            self,
            Expr::Think | Expr::Sleep | Expr::Music | Expr::Sing | Expr::Wave
        )
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
}

impl Pose {
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
/// clockwise).
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub parts: Vec<Part>,
    pub ears: Ears,
    pub bob: f32,
    pub tilt: f32,
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
                    *b = (b.0.min(x - pad), b.1.min(y - pad), b.2.max(x + pad), b.3.max(y + pad));
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
#[cfg_attr(not(test), allow(dead_code))]
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
        }
    }
}

/// The cat's paths in grid units, built on first use and kept. The render
/// target's transform scales them (and their widths) to any size.
pub struct RobotPainter {
    pub(super) factory: ID2D1Factory,
    paths: RefCell<Vec<(Shape, ID2D1PathGeometry)>>,
    /// The calico patches cut to the silhouette, per ear state (`cat_style`).
    pub(super) calico: RefCell<Vec<((u32, u32), [ID2D1PathGeometry; 2])>>,
    /// Each accessory's pieces and their paths (`cat_style`).
    pub(super) extras: RefCell<Vec<(CatAccessory, Rc<Vec<Placed>>)>>,
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

    /// Draws the cat filling the square `area` (its 64-unit grid scaled to
    /// `area.w`), raised by the pose's lift and bounce, at `alpha`. The
    /// render target's transform is restored; the brush colour is not.
    ///
    /// `look` is the coat and accessory. The order is one silhouette (see
    /// `cat_style`): the Black cat's rim, the ears and head in one colour,
    /// the calico patches, the face marks, the accessory. A caller that
    /// fades the cat does it with a layer round this call, as the renderer
    /// does, so the passes do not show through one another.
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
        // The look turns and slides the whole cat; its marks slide a little further.
        let local = Matrix3x2::rotation_around(face.tilt + pose.tilt_deg, Vector2::new(32.0, 40.0))
            * Matrix3x2::translation(0.0, face.bob)
            * Matrix3x2::scale(k, k)
            * Matrix3x2::translation(
                area.x + pose.head_dx_dip,
                area.y - pose.lift_px - pose.bounce_px,
            );
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
            // 1. The Black cat's rim, under everything: the ears stroked
            // wider than their own stroke and the head stroked wider than
            // its outline, so it reaches `RIM_WIDTH` past the silhouette.
            if let Some(rim) = look.rim() {
                brush.SetColor(&color(rim, 1.0));
                for side in [Side::Left, Side::Right] {
                    if let Some(g) = self.path(Shape::Ear(side, face.ears.up)) {
                        let turn =
                            Matrix3x2::rotation_around(side.turn(face.ears.droop), side.pivot());
                        rt.SetTransform(&(turn * world));
                        rt.DrawGeometry(&g, brush, EAR_STROKE + 2.0 * RIM_WIDTH, &self.round);
                    }
                }
                rt.SetTransform(&world);
                if let Some(g) = self.path(Shape::Head) {
                    rt.DrawGeometry(&g, brush, 2.0 * RIM_WIDTH, &self.round);
                }
            }
            // 2. The silhouette in one colour: ears behind the head, a
            // triangle with a fat round stroke, then the head.
            brush.SetColor(&color(look.head_rgb(), 1.0));
            for side in [Side::Left, Side::Right] {
                if let Some(g) = self.path(Shape::Ear(side, face.ears.up)) {
                    let turn = Matrix3x2::rotation_around(side.turn(face.ears.droop), side.pivot());
                    rt.SetTransform(&(turn * world));
                    rt.FillGeometry(&g, brush, None);
                    rt.DrawGeometry(&g, brush, EAR_STROKE, &self.round);
                }
            }
            rt.SetTransform(&world);
            if let Some(g) = self.path(Shape::Head) {
                rt.FillGeometry(&g, brush, None);
            }
            // 3. Calico: the patches, cut to the whole silhouette.
            if look.color == CatColor::Calico {
                self.paint_patches(rt, brush, &world, face.ears, alpha);
            }
            // 4. The face marks.
            let eyes = Matrix3x2::translation(pose.eye_dx_dip / k, 0.0) * world;
            rt.SetTransform(&eyes);
            for part in &face.parts {
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
                        brush.SetColor(&color(look.ink_rgb(ink), 1.0));
                        rt.FillEllipse(&circle(cx, cy, r), brush);
                    }
                    Part::Stroke(line) => {
                        if let Some(g) = self.path(Shape::Line(line)) {
                            brush.SetColor(&color(look.ink_rgb(line.ink()), line.alpha()));
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
            // 5. The accessory, on top of everything.
            self.paint_accessory(rt, brush, look, face.ears, (&world, &eyes), alpha);
            rt.SetTransform(&previous);
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
    /// "Dihapus" after its Hapus.
    ShotDeleted,
    /// A drive was plugged in.
    DevicePlugged,
    /// A device is safe to remove.
    DeviceSafe,
    /// Susun arranged the windows.
    Arranged,
}

impl NoticeSort {
    pub fn of(kind: &NoticeKind) -> Self {
        match kind {
            NoticeKind::App { .. } => NoticeSort::App,
            NoticeKind::Reminder { .. } => NoticeSort::Reminder,
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
            NoticeKind::ShotSaved => NoticeSort::ShotSaved,
            NoticeKind::ShotDeleted => NoticeSort::ShotDeleted,
            NoticeKind::DevicePlugged { .. } => NoticeSort::DevicePlugged,
            NoticeKind::DeviceSafe => NoticeSort::DeviceSafe,
            NoticeKind::Arranged { .. } => NoticeSort::Arranged,
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
            View::PomodoroCompact => Spot::Pomodoro,
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
///    CLI command or a Mole clean running → Think; low battery → Sad;
///    asleep → Sleep; otherwise Idle.
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
                | NoticeSort::ShotSaved => Some(Expr::Happy),
                // keputusan-v3.1 p6: "Dihapus" with Bata at rest; a drive
                // plugged in is news, not an event: Bata at rest as well.
                NoticeSort::ShotDeleted | NoticeSort::DevicePlugged => Some(Expr::Idle),
                NoticeSort::BatteryLow | NoticeSort::SystemLow | NoticeSort::MoleFailed => {
                    Some(Expr::Sad)
                }
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
            _ if i.claude_working || i.cli == Cli::Running || i.mole_running => Expr::Think,
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

/// The hover lift `elapsed` after the pointer arrived: eases up to `LIFT_PX`.
pub fn lift_offset(elapsed: Duration) -> f32 {
    let t = (elapsed.as_millis() as f32 / LIFT_MS as f32).clamp(0.0, 1.0);
    LIFT_PX * (1.0 - (1.0 - t).powi(3))
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
    /// when motion is allowed. True when a bounce started.
    pub fn observe(&mut self, expr: Option<Expr>, now: Instant, moves: bool) -> bool {
        let was = std::mem::replace(&mut self.shown, expr);
        let started = moves && expr == Some(Expr::Alert) && was != Some(Expr::Alert);
        if started {
            self.bounce = Some(now);
        }
        if expr != Some(Expr::Alert) {
            self.bounce = None;
        }
        started
    }

    /// When the hover lift began, if it applies at `spot`: only a compact
    /// island lifts (its face turns Happy); a notice keeps its face still.
    fn lift_since(&self, spot: Spot) -> Option<Instant> {
        self.hover.filter(|_| spot.compact())
    }

    /// Lift and bounce at `spot` and `now` (none without motion), and the
    /// blink.
    pub fn pose(&self, spot: Spot, now: Instant, moves: bool, blink: bool) -> Pose {
        if !moves {
            return Pose {
                blink: false,
                ..Pose::default()
            };
        }
        Pose {
            blink,
            lift_px: self
                .lift_since(spot)
                .map_or(0.0, |at| lift_offset(now.saturating_duration_since(at))),
            bounce_px: self
                .bounce
                .map_or(0.0, |at| bounce_offset(now.saturating_duration_since(at))),
            ..Pose::default()
        }
    }

    /// A lift or bounce at `spot` is still moving: the frame clock must run.
    pub fn moving(&self, spot: Spot, now: Instant, moves: bool) -> bool {
        let within = |at: Option<Instant>, ms: u64| {
            at.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_millis(ms))
        };
        moves
            && (within(self.lift_since(spot), LIFT_MS) || within(self.bounce, BOUNCE_MS * BOUNCES))
    }

    /// The next moment the face may change by itself (falling asleep, a
    /// Claude session going quiet, a CLI face ending), for a one-shot timer.
    pub fn next_change(&self, now: Instant, sleep_after: Duration) -> Option<Instant> {
        let sleep = time_to_sleep(self.last_activity, now, sleep_after).map(|d| now + d);
        let claude = self
            .claude
            .iter()
            .map(|(_, at)| *at + CLAUDE_WORKING_FOR)
            .filter(|t| *t > now);
        let cli = self.cli_mood.map(|(_, until)| until).filter(|t| *t > now);
        sleep.into_iter().chain(claude).chain(cli).min()
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
            (NoticeSort::ShotDeleted, Expr::Idle),
            (NoticeSort::DevicePlugged, Expr::Idle),
            (NoticeSort::DeviceSafe, Expr::Happy),
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
        assert_eq!(e(working), Some(Expr::Think));
        let running = RobotInputs {
            cli: Cli::Running,
            claude_working: false,
            ..busy
        };
        assert_eq!(e(running), Some(Expr::Think));
        // A Mole clean thinks like a CLI command.
        let mole = RobotInputs {
            cli: Cli::None,
            claude_working: false,
            mole_running: true,
            ..busy
        };
        assert_eq!(e(mole), Some(Expr::Think));
        assert_eq!(
            e(RobotInputs {
                mole_running: true,
                ..base
            }),
            Some(Expr::Think)
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
            Spot::of(&View::Notice(n(NoticeKind::App { app_id: None }))),
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
        assert!(b.observe(Some(Expr::Alert), t0, true));
        assert!(!b.observe(Some(Expr::Alert), t0, true), "already showing");
        let alert = Spot::Notice(NoticeSort::Reminder);
        assert!(b.moving(alert, t0 + ms(100), true));
        assert!(!b.moving(alert, t0 + ms(2 * BOUNCE_MS), true));
        b.observe(Some(Expr::Idle), t0, true);
        assert!(!b.observe(Some(Expr::Alert), t0, false), "Hemat: no bounce");
        assert!(!b.moving(alert, t0, true));
    }

    #[test]
    fn hover_lifts_two_pixels() {
        let t0 = Instant::now();
        let idle = Spot::Idle;
        let mut b = Brain::new(t0);
        b.hover(true, t0);
        assert_eq!(b.pose(idle, t0, true, false).lift_px, 0.0);
        let up = b.pose(idle, t0 + Duration::from_millis(LIFT_MS), true, false);
        assert_eq!(up.lift_px, LIFT_PX);
        assert!(
            b.moving(idle, t0, true) && !b.moving(idle, t0 + Duration::from_millis(LIFT_MS), true)
        );
        // No motion: no lift, no blink.
        assert_eq!(
            b.pose(idle, t0 + Duration::from_secs(1), false, true),
            Pose::default()
        );
        // A second move while hovering keeps the first arrival.
        b.hover(true, t0 + Duration::from_secs(1));
        assert_eq!(
            b.pose(idle, t0 + Duration::from_secs(1), true, true)
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
                b.pose(spot, later, true, false).lift_px,
                LIFT_PX,
                "{spot:?}"
            );
            assert!(b.moving(spot, t0, true), "{spot:?}");
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
            assert_eq!(b.pose(spot, later, true, false).lift_px, 0.0, "{spot:?}");
            assert!(!b.moving(spot, t0, true), "{spot:?}");
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
}
