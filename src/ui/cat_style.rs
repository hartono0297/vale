//! The cat's coat and what it wears (Settings -> Kucing): four colours and
//! seven accessories on the 64-unit grid of `robot.rs`.
//!
//! The cat is painted as ONE silhouette, in this order (`RobotPainter::paint`):
//!
//! 1. Black only: a light rim under everything, both turned ears stroked wider
//!    than their own stroke and the head stroked wider than its outline.
//! 2. The silhouette in the base colour: both ears (turned, filled, round
//!    stroke) and the head, all one colour.
//! 3. Calico only: two patches over the silhouette, as on the landing page
//!    cat: the left ear orange, the right ear dark, an orange circle top-left
//!    and a dark circle bottom-right on the head, cream elsewhere. The site
//!    paints the cream head over the ears, so each patch is its ear (turned
//!    and lifted, stroke included) minus the head, united with its circle
//!    clipped to the head: one geometry per colour, no seam between the ear
//!    and the circle.
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
    D2D1_COMBINE_MODE, D2D1_COMBINE_MODE_EXCLUDE, D2D1_COMBINE_MODE_INTERSECT,
    D2D1_COMBINE_MODE_UNION, ID2D1Geometry, ID2D1PathGeometry, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::core::{Interface, Result as WinResult};
use windows_numerics::{Matrix3x2, Vector2};

use super::geometry::{Rect, ShapeKind};
use super::mascot;
use super::robot::{CREAM, EAR_STROKE, Ears, Ink, RobotPainter, Seg, Shape, Side, path};
use crate::config::Config;
use crate::theme::{CatAccessory, CatColor, FalkColor, Mascot, OctopusColor};

/// The cat's colour and accessory, as the config holds them. Every caller of
/// `RobotPainter::paint` passes the config's values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CatLook {
    pub mascot: Mascot,
    pub color: CatColor,
    pub falk: FalkColor,
    pub octopus: OctopusColor,
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
/// Calico's orange: the left ear and the circle top-left on the head.
pub const PATCH_ORANGE: u32 = 0xE2683C;
/// Calico's dark: the right ear and the circle bottom-right on the head.
pub const PATCH_DARK: u32 = 0x2B2420;
/// Calico's cream, the head's own colour, as the swatch's first slice.
const CALICO_CREAM: u32 = 0xF4EDE4;

/// The Calico swatch as a conic gradient, like the site's chip
/// `conic-gradient(#F4EDE4 0 50%, #E2683C 0 75%, #2B2420 0)`: (from, to,
/// colour) in degrees clockwise from 12 o'clock. The right half is cream, the
/// bottom-left quarter orange, the top-left quarter dark.
pub fn calico_swatch_slices() -> [(f32, f32, u32); 3] {
    [
        (0.0, 180.0, CALICO_CREAM),
        (180.0, 270.0, PATCH_ORANGE),
        (270.0, 360.0, PATCH_DARK),
    ]
}

impl CatLook {
    pub fn of(cfg: &Config) -> Self {
        CatLook {
            mascot: cfg.mascot,
            color: cfg.cat_color,
            falk: cfg.falk_color,
            octopus: cfg.octopus_color,
            accessory: cfg.cat_accessory,
        }
    }

    /// Head and ears; the falk's egg, the octopus's coat.
    pub fn head_rgb(self) -> u32 {
        match self.mascot {
            Mascot::Falk => return mascot::falk_ink(self.falk).body,
            Mascot::Octopus => return mascot::octopus_rgb(self.octopus),
            Mascot::Cat => {}
        }
        match self.color {
            CatColor::Terracotta => 0xE2683C,
            CatColor::Calico => CALICO_CREAM,
            CatColor::White => 0xF6F1EA,
            CatColor::Black => 0x1E1B22,
        }
    }

    /// The Black cat's coat (its amber eyes, rim and cream frame); never the
    /// falk's or the octopus's.
    fn black_cat(self) -> bool {
        self.mascot == Mascot::Cat && self.color == CatColor::Black
    }

    /// The eyes, and the blink lines, happy and sleep arcs and alert dots that
    /// replace them. Extras beside the head stay cream (`Ink::Cream`).
    pub fn eye_rgb(self) -> u32 {
        if self.black_cat() { AMBER } else { EYES_DARK }
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
        self.black_cat().then_some(RIM)
    }

    /// The glasses' frame: cream on the Black cat, else espresso.
    fn frame_rgb(self) -> u32 {
        if self.black_cat() { CREAM } else { EYES_DARK }
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
/// The falk crown's rim, and the new pieces' colours.
const CROWN_RIM: u32 = 0x7A5210;
const CREAM_PIECE: u32 = 0xF4EDE4;
const SAILOR_WHITE: u32 = 0xF6F1EA;
const TIE_KNOT: u32 = 0x5878AE;
/// The falk scarf's tail: the navy band, 22 % darker.
const NAVY_DARK: u32 = 0x253A5F;

fn fixed(rgb: u32) -> Tone {
    Tone::Fixed(rgb)
}

/// Where a mascot wears things, in grid units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchors {
    /// Crown and caps.
    pub top: (f32, f32),
    /// The flower.
    pub left: (f32, f32),
    /// The bow.
    pub right: (f32, f32),
    /// The scarf's band: y0 and y1.
    pub band: (f32, f32),
    /// The bow tie's centre.
    pub tie: (f32, f32),
    /// The headphone: x0, x1, cup y and the band's top.
    pub hp: (f32, f32, f32, f32),
    /// How far the eyes sit above the cat's (negative is up).
    pub eye_lift: f32,
}

pub fn anchors(m: Mascot) -> Anchors {
    match m {
        Mascot::Cat => Anchors {
            top: (32.0, 24.0),
            left: (16.0, 23.0),
            right: (45.0, 23.0),
            // The cat's scarf keeps its own geometry: this is its actual band
            // (47..55); its pieces are not built from this anchor.
            band: (47.0, 55.0),
            tie: (32.0, 51.5),
            // The spec says 16; the band's box (control points, stroke) would
            // then touch the Music note's dot at (55.4, 13).
            hp: (7.0, 57.0, 41.0, 16.4),
            eye_lift: 0.0,
        },
        Mascot::Falk => Anchors {
            top: (32.0, 26.0),
            left: (20.0, 29.0),
            right: (44.0, 29.0),
            band: (48.5, 53.5),
            tie: (32.0, 52.5),
            hp: (9.0, 55.0, 40.0, 17.0),
            eye_lift: 0.0,
        },
        Mascot::Octopus => Anchors {
            top: (32.0, 9.0),
            left: (19.0, 17.0),
            right: (45.0, 17.0),
            band: (43.5, 47.5),
            tie: (32.0, 45.5),
            hp: (11.0, 53.0, 33.0, 3.0),
            eye_lift: mascot::OCT_EYE_LIFT,
        },
    }
}

/// A rounded rectangle as a closed path.
fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Seg> {
    use Seg::{Close, Line as To, Move, Quad};
    let (x1, y1) = (x + w, y + h);
    vec![
        Move(x + r, y),
        To(x1 - r, y),
        Quad(x1, y, x1, y + r),
        To(x1, y1 - r),
        Quad(x1, y1, x1 - r, y1),
        To(x + r, y1),
        Quad(x, y1, x, y1 - r),
        To(x, y + r),
        Quad(x, y, x + r, y),
        Close,
    ]
}

/// The falk's body between y0 and y1, edge to edge: the egg's own curve on
/// both sides (`mascot::FALK_BODY`), so the band needs no clip.
fn falk_band(y0: f32, y1: f32) -> Vec<Seg> {
    use Seg::{Close, Line as To, Move, Quad};
    // The right flank is the quad (55, 43), (55, 57), (32, 57).
    let (p0, p1, p2) = (
        (55.0_f32, 43.0_f32),
        (55.0_f32, 57.0_f32),
        (32.0_f32, 57.0_f32),
    );
    let at = |t: f32| {
        let u = 1.0 - t;
        (
            u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
            u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
        )
    };
    // y(t) = 43 + 28 t - 14 t^2: the smaller root for a given y.
    let t_of = |y: f32| (28.0 - (784.0 - 56.0 * (y - 43.0)).sqrt()) / 28.0;
    let (a, b) = (t_of(y0), t_of(y1));
    let (s, e) = (at(a), at(b));
    // The sub-curve's control point.
    let c = (
        (1.0 - a) * (1.0 - b) * p0.0 + ((1.0 - a) * b + a * (1.0 - b)) * p1.0 + a * b * p2.0,
        (1.0 - a) * (1.0 - b) * p0.1 + ((1.0 - a) * b + a * (1.0 - b)) * p1.1 + a * b * p2.1,
    );
    let m = |x: f32| 64.0 - x;
    vec![
        Move(m(s.0), s.1),
        To(s.0, s.1),
        Quad(c.0, c.1, e.0, e.1),
        To(m(e.0), e.1),
        Quad(m(c.0), c.1, m(s.0), s.1),
        Close,
    ]
}

/// The pieces of `accessory` on mascot `m`, each with what it moves with, in
/// paint order. The cat's seven keep their own geometry; the falk and the
/// octopus wear the same things placed on their anchors (the mockup's
/// `accessory()`), and all three wear the headphone, the sailor cap and the
/// bow tie.
pub fn pieces(m: Mascot, accessory: CatAccessory) -> Vec<(Anchor, Piece)> {
    match accessory {
        CatAccessory::None => vec![],
        CatAccessory::Headphone | CatAccessory::Sailor | CatAccessory::Bowtie => {
            new_pieces(m, accessory)
        }
        _ if m == Mascot::Cat => cat_pieces(accessory),
        _ => mascot_pieces(m, accessory),
    }
}

/// Headphone, sailor cap and bow tie, from `anchors(m)`.
fn new_pieces(m: Mascot, accessory: CatAccessory) -> Vec<(Anchor, Piece)> {
    use Seg::{Close, Cubic, Line as To, Move, Quad};
    let a = anchors(m);
    let (tx, ty) = a.top;
    let head = |p: Piece| (Anchor::Head, p);
    match accessory {
        CatAccessory::Headphone => {
            let (x0, x1, y, top) = a.hp;
            // The octopus's band would otherwise reach the Music note's dot at
            // (55.4, 13) by its box; its curve is pulled in 0.9 to stay clear.
            let pull = if m == Mascot::Octopus { 0.9 } else { 0.0 };
            let cup = |x: f32, inner: f32| {
                [
                    head(Piece::Shape {
                        segs: rrect(x, y - 6.0, 7.0, 12.0, 3.0),
                        fill: fixed(CREAM_PIECE),
                        edge: None,
                    }),
                    head(Piece::Shape {
                        segs: rrect(inner, y - 3.5, 3.8, 7.0, 1.6),
                        fill: fixed(NAVY),
                        edge: None,
                    }),
                ]
            };
            let mut v = vec![head(Piece::Stroke {
                segs: vec![
                    Move(x0 + 1.0, y - 3.0),
                    Cubic(x0 + pull, top, x1 - pull, top, x1 - 1.0, y - 3.0),
                ],
                tone: fixed(CREAM_PIECE),
                width: 2.6,
            })];
            v.extend(cup(x0 - 3.0, x0 - 1.4));
            v.extend(cup(x1 - 4.0, x1 - 2.4));
            v
        }
        CatAccessory::Sailor => vec![
            head(Piece::Shape {
                segs: vec![
                    Move(tx - 11.0, ty + 1.5),
                    Quad(tx - 12.0, ty - 8.0, tx, ty - 9.0),
                    Quad(tx + 12.0, ty - 8.0, tx + 11.0, ty + 1.5),
                    Close,
                ],
                fill: fixed(SAILOR_WHITE),
                edge: None,
            }),
            head(Piece::Shape {
                segs: vec![
                    Move(tx - 11.2, ty - 2.4),
                    To(tx + 11.2, ty - 2.4),
                    To(tx + 11.2, ty + 1.2),
                    To(tx - 11.2, ty + 1.2),
                    Close,
                ],
                fill: fixed(NAVY),
                edge: None,
            }),
            head(Piece::Shape {
                segs: rrect(tx - 13.0, ty + 0.6, 26.0, 2.4, 1.2),
                fill: fixed(NAVY),
                edge: None,
            }),
        ],
        CatAccessory::Bowtie => {
            let (x, y) = a.tie;
            let wing = |s: f32| {
                head(Piece::Shape {
                    segs: vec![
                        Move(x, y),
                        To(x + s * 6.0, y - 3.6),
                        To(x + s * 6.0, y + 3.6),
                        Close,
                    ],
                    fill: fixed(NAVY),
                    edge: Some(1.2),
                })
            };
            vec![
                wing(-1.0),
                wing(1.0),
                head(Piece::Dot {
                    cx: x,
                    cy: y,
                    r: 1.6,
                    tone: fixed(TIE_KNOT),
                }),
            ]
        }
        _ => vec![],
    }
}

const NIGHTCAP_TIP_OCTOPUS: f32 = 6.5;

/// The cat's seven accessories, re-placed on the falk or the octopus. Every
/// piece rides the head (`Anchor::Head`), the glasses the eyes.
fn mascot_pieces(m: Mascot, accessory: CatAccessory) -> Vec<(Anchor, Piece)> {
    use Seg::{Close, Line as To, Move, Quad};
    let a = anchors(m);
    let (tx, ty) = a.top;
    let head = |p: Piece| (Anchor::Head, p);
    match accessory {
        CatAccessory::Glasses => pieces(Mascot::Cat, CatAccessory::Glasses),
        CatAccessory::Bow => {
            let (x, y) = a.right;
            let wing = |s: f32| {
                head(Piece::Shape {
                    segs: vec![
                        Move(x, y),
                        To(x + s * 6.0, y - 4.0),
                        To(x + s * 6.0, y + 4.0),
                        Close,
                    ],
                    fill: fixed(RASPBERRY),
                    edge: Some(1.4),
                })
            };
            vec![
                wing(-1.0),
                wing(1.0),
                head(Piece::Dot {
                    cx: x,
                    cy: y,
                    r: 1.9,
                    tone: fixed(BOW_KNOT),
                }),
            ]
        }
        CatAccessory::Flower => {
            let (cx, cy) = a.left;
            let mut v: Vec<(Anchor, Piece)> = (0..5)
                .map(|i| {
                    let g = (-90.0 + 72.0 * i as f32).to_radians();
                    head(Piece::Dot {
                        cx: cx + 3.0 * g.cos(),
                        cy: cy + 3.0 * g.sin(),
                        r: 2.5,
                        tone: fixed(PINK),
                    })
                })
                .collect();
            v.push(head(Piece::Dot {
                cx,
                cy,
                r: 1.8,
                tone: fixed(GOLD),
            }));
            v
        }
        CatAccessory::Crown => {
            let segs = vec![
                Move(tx - 7.0, ty + 1.0),
                To(tx - 7.0, ty - 6.0),
                To(tx - 3.5, ty - 2.5),
                To(tx, ty - 8.0),
                To(tx + 3.5, ty - 2.5),
                To(tx + 7.0, ty - 6.0),
                To(tx + 7.0, ty + 1.0),
                Close,
            ];
            let mut v = vec![head(Piece::Shape {
                segs: segs.clone(),
                fill: fixed(GOLD),
                edge: None,
            })];
            if m == Mascot::Falk {
                // Gold on a yellow head needs a rim.
                v.push(head(Piece::Stroke {
                    segs,
                    tone: fixed(CROWN_RIM),
                    width: 0.9,
                }));
            }
            v
        }
        CatAccessory::Nightcap => {
            // The octopus's mantle already stands 15 grid units above the
            // cat's head, so its cone is shorter: the pom's top stays inside
            // the island at every notch size.
            let tip = if m == Mascot::Octopus {
                NIGHTCAP_TIP_OCTOPUS
            } else {
                14.0
            };
            vec![
                head(Piece::Shape {
                    segs: vec![
                        Move(tx - 15.0, ty + 3.0),
                        To(tx + 14.0, ty + 2.0),
                        To(tx - 13.0, ty - tip),
                        Close,
                    ],
                    fill: fixed(NAVY),
                    edge: Some(2.0),
                }),
                head(Piece::Stroke {
                    segs: vec![
                        Move(tx - 16.0, ty + 3.2),
                        Quad(tx, ty - 1.5, tx + 15.0, ty + 2.4),
                    ],
                    tone: fixed(CREAM),
                    width: 3.0,
                }),
                head(Piece::Dot {
                    cx: tx - 13.0,
                    cy: ty - tip,
                    r: 2.5,
                    tone: fixed(CREAM),
                }),
            ]
        }
        CatAccessory::Scarf => {
            let (y0, y1) = a.band;
            let (band, tail_x, colour, tail) = if m == Mascot::Falk {
                (falk_band(y0, y1), 13.0, NAVY, NAVY_DARK)
            } else {
                (
                    rrect(14.0, y0, 36.0, y1 - y0, 2.0),
                    17.0,
                    MUSTARD,
                    MUSTARD_DARK,
                )
            };
            vec![
                head(Piece::Shape {
                    segs: band,
                    fill: fixed(colour),
                    edge: None,
                }),
                head(Piece::Shape {
                    segs: rrect(tail_x, y0 + 1.0, 4.5, y1 - y0 + 4.0, 1.4),
                    fill: fixed(tail),
                    edge: None,
                }),
            ]
        }
        _ => vec![],
    }
}

/// The cat's seven, with the geometry they have always had.
fn cat_pieces(accessory: CatAccessory) -> Vec<(Anchor, Piece)> {
    use Seg::{Close, Line as To, Move, Quad};
    match accessory {
        // Not the cat's seven.
        CatAccessory::None
        | CatAccessory::Headphone
        | CatAccessory::Sailor
        | CatAccessory::Bowtie => vec![],
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
pub fn accessory_corners(
    mascot: Mascot,
    accessory: CatAccessory,
    ears: Ears,
) -> Vec<((f32, f32), bool)> {
    accessory_piece_corners(mascot, accessory, ears)
        .into_iter()
        .flatten()
        .collect()
}

/// `accessory_corners`, kept apart per piece (each piece's four corners).
#[cfg(test)]
fn accessory_piece_corners(
    mascot: Mascot,
    accessory: CatAccessory,
    ears: Ears,
) -> Vec<Vec<((f32, f32), bool)>> {
    let mut out = Vec::new();
    for (anchor, piece) in pieces(mascot, accessory) {
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
pub(crate) fn piece_bounds(piece: &Piece) -> (f32, f32, f32, f32) {
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

// ---- Accessories on the face notch ("Jadi muka") ----

/// The crown's darker gold edge on a face, so it reads on a yellow one.
pub const CROWN_EDGE: u32 = 0x9A6A12;
/// The width of the edge a face piece gets, outside its outline, in dip.
pub const FACE_EDGE_DIP: f32 = 1.0;
/// Half the box, in grid units from the eyes' centre (32, 40), that the
/// face's marks stay in: the heart beside the left eye reaches 23.3 across,
/// the thought dots 7.7 up.
pub const FACE_HALF_W: f32 = 23.5;
pub const FACE_HALF_H: f32 = 8.0;
/// The same down with the bow tie under the eyes (it reaches 9.8).
pub const FACE_HALF_H_TIE: f32 = 10.5;
/// The eyes alone, every expression open and shut, in grid units from the
/// eyes' centre (32, 40): 13 across each way and 7 up (the thinking eyes look
/// up). What the hats' brims keep clear of.
pub const FACE_EYES_HALF_W: f32 = 13.0;
pub const FACE_EYES_UP: f32 = 7.0;
/// The bow tie under the eyes: its size against the head's and how far
/// below the eye line its knot sits, in grid units.
const FACE_TIE_K: f32 = 0.55;
const FACE_TIE_DY: f32 = 7.5;
/// The earcups: their size against the head's, how far in from the notch's
/// ends (dip).
const FACE_CUP_K: f32 = 0.7;
const FACE_CUP_IN: f32 = 3.5;
/// An earcup's width in grid units (before `FACE_CUP_K`).
const FACE_CUP_W: f32 = 7.0;

/// The left of the right earcup on the face notch `shape` with `unit` dip
/// per grid unit, its edge included (`FacePiece::corners`): what the unread
/// dot keeps clear of (`geometry::face_toast_dot`).
pub fn face_cup_left(shape: Rect, unit: f32) -> f32 {
    shape.right() - FACE_CUP_IN - FACE_CUP_W * FACE_CUP_K * unit - FACE_EDGE_DIP
}
/// The scarf on the notch's bottom edge: band and tail height (grid units)
/// and the room kept under the tail (dip), past its edge.
const FACE_BAND_H: f32 = 2.4;
const FACE_TAIL_H: f32 = 1.6;
const FACE_SCARF_LIFT: f32 = 2.25;
/// The corner pieces (bow, flower) in the notch's left corner: the room kept
/// from the face's box and from the notch's top and bottom (dip), and where
/// their centre sits down the notch (a share of its height).
const CORNER_GAP: f32 = 1.0;
const CORNER_PAD: f32 = 3.0;
const CORNER_DOWN: f32 = 0.45;
/// The hats' brims ("pinggiran"): the room kept above the eyes' box (dip);
/// how far past the face's box they reach across (dip, well inside the gap to
/// the clock); the least the hanging detail (the nightcap's tip, the sailor's
/// tails) shrinks to, against the face's unit, before it is left off.
const RIM_GAP: f32 = 0.5;
const RIM_PAST: f32 = 2.5;
const RIM_DETAIL_MIN: f32 = 0.5;

/// What a face piece moves with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceAnchor {
    /// The eyes: slides with the look and nods with the face (the glasses).
    Eyes,
    /// The face: nods with it (the bow tie).
    Face,
    /// The notch's left corner, left of the face: stays put (the bow, the
    /// flower).
    Corner,
    /// The notch's ends and bottom edge: stays put (the earcups, the scarf).
    Notch,
    /// The notch's top edge over the face: stays put (the hats' brims, the
    /// rest of the hat past the screen edge).
    Rim,
}

/// One piece of an accessory on the face notch: `piece` in its own units
/// (the head's grid, or dip), placed by moving `origin` to the start,
/// scaling by `k` (dip per unit), turning `turn` degrees clockwise and
/// moving to `at` (dip).
#[derive(Clone, Debug, PartialEq)]
pub struct FacePiece {
    pub piece: Piece,
    pub origin: (f32, f32),
    pub k: f32,
    pub turn: f32,
    pub at: (f32, f32),
    pub anchor: FaceAnchor,
    /// A detail drawn over another piece (a knot, a band, an inner cup): it
    /// sits on the accessory, not on the face, so it never gets an edge.
    pub inner: bool,
    /// An edge it always has (the crown's darker gold).
    pub fixed_edge: Option<u32>,
    /// The face it is worn on: the eyes' centre (dip) and dip per grid unit,
    /// to know what lies under it (the calico patches).
    pub face: ((f32, f32), f32),
}

impl FacePiece {
    /// Its units to dip, slid by `slide` when on the eyes and nodded `nod`
    /// down when on the eyes or the face.
    pub fn place(&self, slide: (f32, f32), nod: f32) -> Matrix3x2 {
        let (sx, sy) = if self.anchor == FaceAnchor::Eyes {
            slide
        } else {
            (0.0, 0.0)
        };
        let nod = if matches!(self.anchor, FaceAnchor::Eyes | FaceAnchor::Face) {
            nod
        } else {
            0.0
        };
        Matrix3x2::translation(-self.origin.0, -self.origin.1)
            * Matrix3x2::scale(self.k, self.k)
            * Matrix3x2::rotation_around(self.turn, Vector2::new(0.0, 0.0))
            * Matrix3x2::translation(self.at.0 + sx, self.at.1 + sy + nod)
    }

    /// The four corners of its box as drawn, in dip, the edge included
    /// (whether or not it gets one): a turned piece's box turns with it.
    pub fn corners(&self, slide: (f32, f32), nod: f32) -> [(f32, f32); 4] {
        let (l, t, r, b) = piece_bounds(&self.piece);
        let g = if self.k > 0.0 { FACE_EDGE_DIP / self.k } else { 0.0 };
        let m = self.place(slide, nod);
        [(l - g, t - g), (r + g, t - g), (r + g, b + g), (l - g, b + g)]
            .map(|(x, y)| (x * m.M11 + y * m.M21 + m.M31, x * m.M12 + y * m.M22 + m.M32))
    }

    /// Its colour on `look`.
    pub fn main_rgb(&self, look: CatLook) -> u32 {
        let tone = match &self.piece {
            Piece::Shape { fill, .. } => *fill,
            Piece::Stroke { tone, .. } | Piece::Dot { tone, .. } | Piece::Ring { tone, .. } => {
                *tone
            }
        };
        match tone {
            Tone::Fixed(rgb) => rgb,
            Tone::Frame => look.frame_rgb(),
        }
    }

    /// The colours under it on `look`'s face notch: the face's fill and each
    /// calico patch its box (at rest) reaches.
    pub fn colours_under(&self, look: CatLook) -> Vec<u32> {
        use super::peek_face::PeekMark;
        use super::robot::{FaceBoxKind, face_coat};
        let corners = self.corners((0.0, 0.0), 0.0);
        let (l, t, r, b) = corners.iter().fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |a, &(x, y)| (a.0.min(x), a.1.min(y), a.2.max(x), a.3.max(y)),
        );
        let ((ex, ey), u) = self.face;
        let mut under = vec![super::peek_face::peek_face(look, 0).fill];
        for (mark, kind) in face_coat(look) {
            if let (PeekMark::Dot { cx, cy, r: rad, rgb }, FaceBoxKind::Patch) = (mark, kind) {
                let (px, py, pr) = (ex + (cx - 32.0) * u, ey + (cy - 40.0) * u, rad * u);
                let (nx, ny) = (px.clamp(l, r), py.clamp(t, b));
                if (nx - px).powi(2) + (ny - py).powi(2) < pr * pr {
                    under.push(rgb);
                }
            }
        }
        under
    }

    /// The edge it gets on `look`'s face notch: its own; else, where its
    /// colour is under 3:1 against anything under it (`colours_under`), a
    /// 1 dip edge in the face's eye colour (cream where that would read less
    /// on what is under it); never on a detail.
    pub fn edge(&self, look: CatLook) -> Option<u32> {
        use super::notch_colour::{CREAM as CREAM_TEXT, contrast};
        if self.inner {
            return None;
        }
        if self.fixed_edge.is_some() {
            return self.fixed_edge;
        }
        let under = self.colours_under(look);
        let worst = |c: u32| under.iter().map(|&u| contrast(c, u)).fold(f32::MAX, f32::min);
        if worst(self.main_rgb(look)) >= 3.0 {
            return None;
        }
        let eye = look.eye_rgb();
        Some(if worst(eye) >= 3.0 || worst(eye) >= worst(CREAM_TEXT) {
            eye
        } else {
            CREAM_TEXT
        })
    }
}

/// Half the box (across, down), in grid units from the eyes' centre, that
/// the face's marks and the pieces of `accessory` worn on the face (the
/// glasses, the bow tie) stay in, edges aside (`geometry::face_scale` keeps
/// room for those). The corner pieces sit outside it, in the notch's corner,
/// so they never make the face smaller.
pub fn face_half(accessory: CatAccessory) -> (f32, f32) {
    match accessory {
        CatAccessory::Bowtie => (FACE_HALF_W, FACE_HALF_H_TIE),
        _ => (FACE_HALF_W, FACE_HALF_H),
    }
}

/// How far in from a notch end a box `depth` dip above its bottom must
/// start to clear the rounded bottom corner, for either style (the Notch's
/// 16 dip corner or the Pill's half height, whichever cuts more).
fn corner_in(shape: Rect, depth: f32) -> f32 {
    let r = (shape.h / 2.0).max(super::geometry::COMPACT_INSET);
    let d = depth.clamp(0.0, r);
    r - (r * r - (r - d) * (r - d)).max(0.0).sqrt()
}

/// A point inside `shape` as either style rounds it (the Pill's half-height
/// ends, the Notch's 16 dip bottom corners).
fn inside_both(shape: Rect, x: f32, y: f32) -> bool {
    use super::geometry::{COMPACT_INSET, ShapeRect, shape_contains};
    use crate::config::Style;
    let s = |radius| ShapeRect {
        x: shape.x,
        y: shape.y,
        w: shape.w,
        h: shape.h,
        radius,
    };
    shape_contains(Style::Pill, s(shape.h / 2.0), x, y)
        && shape_contains(Style::Notch, s(COMPACT_INSET), x, y)
}

/// Whether `accessory` is drawn on the face notch of a `kind` view: every one
/// on the idle notch; on the narrow views (media, lyric, pomodoro, REC) the
/// glasses, the bow tie, the earcups and the hats' brims (they hug the top
/// edge over the face), not the bow, the flower (no room left of the face
/// for the corner) or the scarf (it would run under the text).
pub fn face_wears(accessory: CatAccessory, kind: ShapeKind) -> bool {
    kind == ShapeKind::Idle
        || matches!(
            accessory,
            CatAccessory::Glasses
                | CatAccessory::Bowtie
                | CatAccessory::Headphone
                | CatAccessory::Nightcap
                | CatAccessory::Crown
                | CatAccessory::Sailor
        )
}

/// `accessory` on the face notch of a `kind` view, in paint order: `shape`
/// is the notch, `eyes` the eyes' centre (dip, before the look), `unit` dip
/// per grid unit (`robot::face_unit`). Empty where `face_wears` says no.
///
/// - Glasses: on the eyes, sliding with the look.
/// - Headphone: two earcups at the notch's left and right ends (no band).
/// - Bow tie: small, under the eyes.
/// - Scarf: a band along the notch's bottom edge, its tail hanging from it.
/// - Bow, flower: in the notch's left corner, left of the face.
/// - Nightcap, crown, sailor cap: worn, the top past the screen edge, so
///   only the brim shows along the notch's top edge over the face
///   ("pinggiran"), with one detail hanging left of the eyes (`rim_pieces`).
///
/// A piece under 3:1 against what lies under it gets a 1 dip edge
/// (`FacePiece::edge`); the crown always has its darker gold one.
pub fn face_pieces(
    accessory: CatAccessory,
    kind: ShapeKind,
    shape: Rect,
    eyes: (f32, f32),
    unit: f32,
) -> Vec<FacePiece> {
    if !face_wears(accessory, kind) {
        return Vec::new();
    }
    let make = |piece: Piece, origin, k, turn, at, anchor, inner| FacePiece {
        piece,
        origin,
        k,
        turn,
        at,
        anchor,
        inner,
        fixed_edge: None,
        face: (eyes, unit),
    };
    let head = |a: CatAccessory| pieces(Mascot::Cat, a).into_iter().map(|(_, p)| p);
    match accessory {
        CatAccessory::None => Vec::new(),
        CatAccessory::Glasses => head(accessory)
            .map(|p| make(p, (32.0, 40.0), unit, 0.0, eyes, FaceAnchor::Eyes, false))
            .collect(),
        CatAccessory::Bowtie => {
            let at = (eyes.0, eyes.1 + FACE_TIE_DY * unit);
            head(accessory)
                .map(|p| {
                    let knot = matches!(p, Piece::Dot { .. });
                    make(p, anchors(Mascot::Cat).tie, FACE_TIE_K * unit, 0.0, at, FaceAnchor::Face, knot)
                })
                .collect()
        }
        CatAccessory::Headphone => {
            let k = FACE_CUP_K * unit;
            let mid = shape.y + shape.h / 2.0;
            let cup = |origin: (f32, f32), at: (f32, f32)| {
                [
                    make(
                        Piece::Shape {
                            segs: rrect(0.0, 0.0, FACE_CUP_W, 12.0, 3.0),
                            fill: fixed(CREAM_PIECE),
                            edge: None,
                        },
                        origin,
                        k,
                        0.0,
                        at,
                        FaceAnchor::Notch,
                        false,
                    ),
                    make(
                        Piece::Shape {
                            segs: rrect(1.6, 2.5, 3.8, 7.0, 1.6),
                            fill: fixed(NAVY),
                            edge: None,
                        },
                        origin,
                        k,
                        0.0,
                        at,
                        FaceAnchor::Notch,
                        true,
                    ),
                ]
            };
            let mut v = cup((0.0, 6.0), (shape.x + FACE_CUP_IN, mid)).to_vec();
            v.extend(cup((FACE_CUP_W, 6.0), (shape.right() - FACE_CUP_IN, mid)));
            v
        }
        CatAccessory::Scarf => {
            use Seg::{Close, Line as To, Move};
            let (band_h, tail_h) = (FACE_BAND_H * unit, FACE_TAIL_H * unit);
            let bottom = shape.bottom() - FACE_SCARF_LIFT;
            let (band_t, band_b) = (bottom - tail_h - band_h, bottom - tail_h);
            // Each box, its edge included, clears the rounded bottom corners.
            let band_in = corner_in(shape, shape.bottom() - band_b - FACE_EDGE_DIP)
                + FACE_EDGE_DIP
                + 0.25;
            let tail_x = shape.x
                + corner_in(shape, shape.bottom() - bottom - FACE_EDGE_DIP)
                + FACE_EDGE_DIP
                + 0.25;
            let tail_w = 2.6 * unit;
            let band = rrect(
                shape.x + band_in,
                band_t,
                (shape.w - 2.0 * band_in).max(0.0),
                band_h,
                band_h / 2.0,
            );
            let tail = vec![
                Move(tail_x, band_t + band_h / 2.0),
                To(tail_x + tail_w, band_t + band_h / 2.0),
                To(tail_x + tail_w + 0.6 * unit, bottom),
                To(tail_x + 0.6 * unit, bottom),
                Close,
            ];
            let at_notch = |segs, rgb| {
                make(
                    Piece::Shape {
                        segs,
                        fill: fixed(rgb),
                        edge: None,
                    },
                    (0.0, 0.0),
                    1.0,
                    0.0,
                    (0.0, 0.0),
                    FaceAnchor::Notch,
                    false,
                )
            };
            // The tail first: it comes out from under the band.
            vec![at_notch(tail, MUSTARD_DARK), at_notch(band, MUSTARD)]
        }
        CatAccessory::Bow | CatAccessory::Flower => {
            let parts: Vec<Piece> = head(accessory).collect();
            // The knot, the flower's heart: the last piece, over the rest.
            let last = parts.len().saturating_sub(1);
            corner_pieces(parts, |i| i == last, 0.6, shape, eyes, unit)
        }
        CatAccessory::Nightcap | CatAccessory::Crown | CatAccessory::Sailor => {
            rim_pieces(accessory, shape, eyes, unit)
        }
    }
}

/// A face piece's box as drawn at rest (left, top, right, bottom), its
/// edge included.
fn drawn_box(p: &FacePiece) -> (f32, f32, f32, f32) {
    p.corners((0.0, 0.0), 0.0).iter().fold(
        (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
        |a, &(x, y)| (a.0.min(x), a.1.min(y), a.2.max(x), a.3.max(y)),
    )
}

/// How far in from a notch end a box from `top` down to `bottom` (dip) must
/// start for its corners to stay inside either style: the Pill's rounded top
/// and bottom (half the height) and the Notch's rounded bottom
/// (`corner_in`).
fn side_in(shape: Rect, top: f32, bottom: f32) -> f32 {
    let r = shape.h / 2.0;
    let d = (top - shape.y).clamp(0.0, r);
    let pill_top = r - (r * r - (r - d) * (r - d)).max(0.0).sqrt();
    pill_top.max(corner_in(shape, shape.bottom() - bottom))
}

/// A hat (`hat`: the nightcap, the crown, the sailor cap) worn on the face
/// notch, its top past the screen edge ("pinggiran", `notch-muka.html`'s
/// `hatOnNotch(acc, 'rim')`): only its lower part shows, along the notch's
/// top edge over the face, plus one detail hanging on the left. The mockup's
/// sizes are in grid units of the face (`unit` dip each, the mockup's 1.28),
/// centred on the eyes' x.
///
/// - The brim starts `FACE_EDGE_DIP` below the top edge (its edge, when it
///   has one, stays inside the notch) and keeps `RIM_GAP` above the eyes'
///   box (`FACE_EYES_UP`), so it is shallower than the mockup where the eyes
///   sit high.
/// - It reaches across at most `RIM_PAST` past the face's box, short of the
///   clock, and each end comes in to clear both styles' rounded corners.
/// - The detail (the nightcap's tip and pompom, the sailor's two tails)
///   hangs from the brim's left end. It shrinks until it is inside the notch
///   and left of the eyes' reach (`FACE_EYES_HALF_W` and the look's slide),
///   and is left off below `RIM_DETAIL_MIN` of the face's unit.
fn rim_pieces(hat: CatAccessory, shape: Rect, eyes: (f32, f32), unit: f32) -> Vec<FacePiece> {
    use super::geometry::FACE_MARGIN;
    use super::robot::{GLANCE_X, LOOK_EYE_DIP};
    use Seg::{Close, Line as To, Move, Quad};
    let u = unit;
    // The mockup's depth and half width of the brim, in grid units.
    let (depth, half) = match hat {
        CatAccessory::Nightcap => (6.25, 28.125),
        CatAccessory::Crown => (5.86, 19.53),
        _ => (6.72, 27.34),
    };
    let y0 = shape.y + FACE_EDGE_DIP;
    let eye_top = eyes.1 - FACE_EYES_UP * u;
    // Grid units down, shallower where the eyes sit high.
    let v = ((eye_top - RIM_GAP - FACE_EDGE_DIP - y0) / depth).min(u);
    if !(v > 0.0 && u > 0.0) {
        return Vec::new();
    }
    let bottom = y0 + depth * v + FACE_EDGE_DIP;
    let in_x = side_in(shape, shape.y, bottom) + FACE_EDGE_DIP + 0.05;
    let reach = (half * u).min(FACE_HALF_W * u + FACE_MARGIN + RIM_PAST);
    let left = (eyes.0 - reach).max(shape.x + in_x);
    let right = (eyes.0 + reach).min(shape.right() - in_x);
    if right - left < 2.0 * u {
        return Vec::new();
    }
    let at = move |piece: Piece, inner: bool| FacePiece {
        piece,
        origin: (0.0, 0.0),
        k: 1.0,
        turn: 0.0,
        at: (0.0, 0.0),
        anchor: FaceAnchor::Rim,
        inner,
        fixed_edge: None,
        face: (eyes, unit),
    };
    let shape_of = |segs: Vec<Seg>, rgb: u32| Piece::Shape {
        segs,
        fill: fixed(rgb),
        edge: None,
    };
    let rect = |l: f32, t: f32, r: f32, b: f32| vec![Move(l, t), To(r, t), To(r, b), To(l, b), Close];
    // The brim, in paint order, and the detail at a scale `k` (dip per grid
    // unit), drawn under the brim.
    let (brim, detail): (Vec<FacePiece>, Box<dyn Fn(f32) -> Vec<FacePiece>>) = match hat {
        CatAccessory::Nightcap => {
            // Navy band, cream fur over its lower edge (1.5625 wider each way).
            let (bl, br) = (left + 1.5625 * u, right - 1.5625 * u);
            let fur = (y0 + 2.75 * v, y0 + 6.25 * v);
            let mid = y0 + 1.75 * v;
            let brim = vec![
                at(shape_of(rect(bl, y0, br, y0 + 3.5 * v), NAVY), false),
                at(
                    shape_of(rrect(left, fur.0, right - left, fur.1 - fur.0, (fur.1 - fur.0) / 2.0), CREAM_PIECE),
                    false,
                ),
            ];
            // The tip: out from under the band's left end, flopping down the
            // left of the face, no thicker than the band; the pompom on its end.
            let detail = move |k: f32| {
                let p = |x: f32, y: f32| (left + x * k, mid + y * k);
                let (s, c1, m, c2, e) = (p(3.0, 0.0), p(-6.25, 1.37), p(-7.8, 9.96), p(-8.6, 14.65), p(-10.16, 17.0));
                let pom = p(-10.16, 18.16);
                let tip = Piece::Stroke {
                    segs: vec![Move(s.0, s.1), Quad(c1.0, c1.1, m.0, m.1), Quad(c2.0, c2.1, e.0, e.1)],
                    width: (4.7 * k).min(3.5 * v),
                    tone: fixed(NAVY),
                };
                let pompom = Piece::Dot { cx: pom.0, cy: pom.1, r: 3.1 * k, tone: fixed(CREAM_PIECE) };
                vec![at(tip, false), at(pompom, false)]
            };
            (brim, Box::new(detail))
        }
        CatAccessory::Crown => {
            // Gold band, its lower edge zig-zagging down to points.
            let (body, tip) = (y0 + 4.3 * v, y0 + 5.86 * v);
            let teeth = ((right - left) / (7.8 * u)).round().max(2.0) as usize;
            let w = (right - left) / teeth as f32;
            let mut segs = vec![Move(left, y0), To(right, y0), To(right, body)];
            for i in (0..teeth).rev() {
                let x = left + w * i as f32;
                segs.push(To(x + w / 2.0, tip));
                segs.push(To(x, body));
            }
            segs.push(Close);
            let mut band = at(shape_of(segs, GOLD), false);
            band.fixed_edge = Some(CROWN_EDGE);
            // A darker strip along the top and three jewels, on the band.
            let c = (left + right) / 2.0;
            let gap = (10.16 * u).min((right - left) / 2.0 - 2.5 * v).max(0.0);
            let jy = y0 + 2.81 * v;
            let jewel = |x: f32, r: f32, rgb: u32| at(Piece::Dot { cx: x, cy: jy, r: r * v, tone: fixed(rgb) }, true);
            let brim = vec![
                band,
                at(shape_of(rect(left, y0, right, y0 + 1.56 * v), GOLD_DARK), true),
                jewel(c - gap, 1.09, RASPBERRY),
                jewel(c, 1.25, TIE_KNOT),
                jewel(c + gap, 1.09, RASPBERRY),
            ];
            (brim, Box::new(|_: f32| Vec::new()))
        }
        _ => {
            // White band with a navy ribbon stripe, a navy rim under it
            // (2.34 wider each way).
            let (wl, wr) = (left + 2.34 * u, right - 2.34 * u);
            let (rim_t, rim_b) = (y0 + 4.69 * v, y0 + 6.72 * v);
            let brim = vec![
                at(shape_of(rect(wl, y0, wr, rim_t), SAILOR_WHITE), false),
                at(shape_of(rect(wl, y0 + 2.03 * v, wr, y0 + 4.06 * v), NAVY), true),
                at(shape_of(rrect(left, rim_t, right - left, rim_b - rim_t, (rim_b - rim_t) / 2.0), NAVY), false),
            ];
            // Two short ribbon tails from under the rim's left end.
            let hide = rim_b - (rim_b - rim_t) / 2.0;
            let detail = move |k: f32| {
                // Each tail: its top corners (hidden under the rim) and the
                // notched end hanging below it, in grid units from the rim's
                // left end and bottom.
                let tail = |top: (f32, f32), end: [(f32, f32); 3]| {
                    let mut segs = vec![Move(left + top.0 * k, hide)];
                    segs.extend(end.map(|(x, y)| To(left + x * k, rim_b + y * k)));
                    segs.extend([To(left + top.1 * k, hide), Close]);
                    at(shape_of(segs, NAVY), false)
                };
                vec![
                    tail((4.75, 7.09), [(0.84, 8.13), (3.58, 7.34), (5.14, 9.69)]),
                    tail((7.88, 10.61), [(6.7, 8.9), (9.05, 7.73), (10.61, 9.69)]),
                ]
            };
            (brim, Box::new(detail))
        }
    };
    // The eyes' reach: their box, slid by the look or the typing glance.
    let eyes_left = eyes.0 - FACE_EYES_HALF_W * u - LOOK_EYE_DIP.max(GLANCE_X * u);
    let fits = |ps: &[FacePiece]| {
        ps.iter().all(|p| {
            let (_, t, r, b) = drawn_box(p);
            (r <= eyes_left - RIM_GAP || b <= eye_top - RIM_GAP)
                && t >= shape.y
                && p.corners((0.0, 0.0), 0.0).into_iter().all(|(x, y)| inside_both(shape, x, y))
        })
    };
    let mut k = u;
    let mut hanging = detail(k);
    while !hanging.is_empty() && !fits(&hanging) {
        k *= 0.95;
        hanging = if k < RIM_DETAIL_MIN * u { Vec::new() } else { detail(k) };
    }
    // The detail comes out from under the brim.
    hanging.into_iter().chain(brim).collect()
}

/// `parts` (in the head's grid) in the notch's left corner: between
/// `FACE_SIDE` from the notch's left end and the face's box (`FACE_HALF_W`
/// across from `eyes`, plus `geometry::FACE_MARGIN`), clear of the notch's
/// top and bottom, at most `cap` grid units of the face per unit of theirs,
/// and shrunk until every corner of their box clears both styles' rounded
/// corners. `inner(i)` marks the details.
fn corner_pieces(
    parts: Vec<Piece>,
    inner: impl Fn(usize) -> bool,
    cap: f32,
    shape: Rect,
    eyes: (f32, f32),
    unit: f32,
) -> Vec<FacePiece> {
    use super::geometry::{FACE_MARGIN, FACE_SIDE};
    let (l, t, r, b) = parts.iter().map(piece_bounds).fold(
        (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
        |a, p| (a.0.min(p.0), a.1.min(p.1), a.2.max(p.2), a.3.max(p.3)),
    );
    let (w, h) = (r - l, b - t);
    let edge = FACE_EDGE_DIP;
    let left = shape.x + FACE_SIDE;
    let right = eyes.0 - FACE_HALF_W * unit - FACE_MARGIN - CORNER_GAP;
    let cy = shape.y + CORNER_DOWN * shape.h;
    let tall = 2.0 * (cy - shape.y).min(shape.bottom() - cy) - 2.0 * CORNER_PAD;
    let mut k = (cap * unit)
        .min((right - left - 2.0 * edge) / w)
        .min((tall - 2.0 * edge) / h)
        .max(0.0);
    let at = ((left + right) / 2.0, cy);
    let build = |k: f32| -> Vec<FacePiece> {
        parts
            .iter()
            .enumerate()
            .map(|(i, piece)| FacePiece {
                piece: piece.clone(),
                origin: ((l + r) / 2.0, (t + b) / 2.0),
                k,
                turn: 0.0,
                at,
                anchor: FaceAnchor::Corner,
                inner: inner(i),
                fixed_edge: None,
                face: (eyes, unit),
            })
            .collect()
    };
    let clear = |v: &[FacePiece]| {
        v.iter().all(|p| {
            p.corners((0.0, 0.0), 0.0)
                .into_iter()
                .all(|(x, y)| inside_both(shape, x, y))
        })
    };
    for _ in 0..64 {
        if k <= 0.0 || clear(&build(k)) {
            break;
        }
        k *= 0.95;
    }
    build(k)
}

// ---- Calico patches (grid units) ----

/// The orange circle on the head, top-left: the site's circle cx 60 cy 118
/// r 62 on the 256 grid, mapped to the 64 grid (`x = sx / 4.6 + 4.17391`,
/// `y = sy / 4.6 + 2.91304`).
pub(crate) const ORANGE_CIRCLE: (f32, f32, f32) = (17.217, 28.565, 13.478);
/// The dark circle on the head, bottom-right: the site's cx 214 cy 226 r 46.
pub(crate) const DARK_CIRCLE: (f32, f32, f32) = (50.696, 52.043, 10.0);

fn missing_geometry() -> windows::core::Error {
    windows::core::Error::from(windows::Win32::Foundation::E_FAIL)
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
    /// One ear as drawn for `ears`: lifted, turned about its pivot, its fill
    /// united with its round stroke band, in grid units.
    fn ear_whole(&self, side: Side, ears: Ears) -> WinResult<ID2D1Geometry> {
        let ear = self
            .path(Shape::Ear(side, ears.up))
            .ok_or_else(missing_geometry)?;
        let turn = Matrix3x2::rotation_around(side.turn(ears.droop), side.pivot());
        unsafe {
            let fill = self.factory.CreateTransformedGeometry(&ear, &turn)?;
            let band = self.factory.CreatePathGeometry()?;
            let sink = band.Open()?;
            ear.Widen(EAR_STROKE, &self.round, Some(&turn), TOLERANCE, &sink)?;
            sink.Close()?;
            self.combine(
                &fill.cast::<ID2D1Geometry>()?,
                &band.cast::<ID2D1Geometry>()?,
                D2D1_COMBINE_MODE_UNION,
            )
        }
    }

    /// The head united with both ears as drawn for `ears` (each turned about
    /// its pivot, fill and round stroke), in grid units. The painter no longer
    /// needs it (the patches are built from the ears and the head); the tests
    /// use it as the outline nothing may leave.
    #[cfg(test)]
    fn silhouette(&self, ears: Ears) -> WinResult<ID2D1PathGeometry> {
        let head = self.path(Shape::Head).ok_or_else(missing_geometry)?;
        let mut acc: ID2D1Geometry = head.cast()?;
        for side in [Side::Left, Side::Right] {
            acc = self.combine(&acc, &self.ear_whole(side, ears)?, D2D1_COMBINE_MODE_UNION)?;
        }
        acc.cast()
    }

    /// `a` combined with `b` by `mode`, as a new path geometry.
    pub(super) fn combine(
        &self,
        a: &ID2D1Geometry,
        b: &ID2D1Geometry,
        mode: D2D1_COMBINE_MODE,
    ) -> WinResult<ID2D1Geometry> {
        unsafe {
            let out = self.factory.CreatePathGeometry()?;
            let sink = out.Open()?;
            a.CombineWithGeometry(b, mode, None, TOLERANCE, &sink)?;
            sink.Close()?;
            out.cast()
        }
    }

    /// The two calico patches for `ears` (orange, dark), built the first time
    /// that ear state is drawn. Each is its ear minus the head (the site
    /// paints the cream head over the ears) united with its circle clipped to
    /// the head.
    fn calico_patches(&self, ears: Ears) -> Option<[ID2D1PathGeometry; 2]> {
        let key = (ears.up.to_bits(), ears.droop.to_bits());
        if let Some((_, g)) = self.calico.borrow().iter().find(|(k, _)| *k == key) {
            return Some(g.clone());
        }
        let built = (|| -> WinResult<[ID2D1PathGeometry; 2]> {
            let head: ID2D1Geometry = self
                .path(Shape::Head)
                .ok_or_else(missing_geometry)?
                .cast()?;
            let patch = |side: Side,
                         (cx, cy, r): (f32, f32, f32)|
             -> WinResult<ID2D1PathGeometry> {
                let ear_only = self.combine(
                    &self.ear_whole(side, ears)?,
                    &head,
                    D2D1_COMBINE_MODE_EXCLUDE,
                )?;
                let disc: ID2D1Geometry =
                    unsafe { self.factory.CreateEllipseGeometry(&circle(cx, cy, r))? }.cast()?;
                let on_head = self.combine(&disc, &head, D2D1_COMBINE_MODE_INTERSECT)?;
                self.combine(&ear_only, &on_head, D2D1_COMBINE_MODE_UNION)?
                    .cast()
            };
            Ok([
                patch(Side::Left, ORANGE_CIRCLE)?,
                patch(Side::Right, DARK_CIRCLE)?,
            ])
        })()
        .ok()?;
        self.calico.borrow_mut().push((key, built.clone()));
        Some(built)
    }

    /// The accessory's pieces with their paths, built the first time.
    fn extras(&self, mascot: Mascot, accessory: CatAccessory) -> Rc<Vec<Placed>> {
        if let Some((_, v)) = self
            .extras
            .borrow()
            .iter()
            .find(|(k, _)| *k == (mascot, accessory))
        {
            return v.clone();
        }
        let built: Vec<Placed> = pieces(mascot, accessory)
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
        self.extras
            .borrow_mut()
            .push(((mascot, accessory), built.clone()));
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
        let extras = self.extras(look.mascot, look.accessory);
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

/// How many face piece paths are kept: every accessory's pieces at a few
/// notch sizes. Past it the cache starts over.
const FACE_PATHS_KEPT: usize = 48;

impl RobotPainter {
    /// The path of a face piece's `segs`, built the first time and kept, as
    /// `extras` keeps the head's.
    fn face_path(&self, segs: &[Seg]) -> WinResult<ID2D1PathGeometry> {
        if let Some((_, g)) = self.face_paths.borrow().iter().find(|(s, _)| s == segs) {
            return Ok(g.clone());
        }
        let g = path(&self.factory, segs)?;
        let mut kept = self.face_paths.borrow_mut();
        if kept.len() >= FACE_PATHS_KEPT {
            kept.clear();
        }
        kept.push((segs.to_vec(), g.clone()));
        Ok(g)
    }

    /// Paints `pieces` (`face_pieces`) on the face notch for `look`: the ones
    /// on the eyes slid by the face's look (`robot::face_slide` of `pose` at
    /// `scale`, cut back with `robot::face_pose_in`), all but the notch's
    /// nodded with the face. Each piece's edge (`FacePiece::edge`) is drawn
    /// under it, `FACE_EDGE_DIP` past its outline. Painted with the caller's
    /// `brush` (its colour left changed). The render target's transform is
    /// restored.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_face_pieces(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        pieces: &[FacePiece],
        scale: f32,
        pose: &super::robot::Pose,
        look: CatLook,
        alpha: f32,
    ) -> WinResult<()> {
        if pieces.is_empty() {
            return Ok(());
        }
        let slide = super::robot::face_slide(pose, scale);
        unsafe {
            let mut previous = Matrix3x2::default();
            rt.GetTransform(&mut previous);
            for p in pieces {
                if p.k <= 0.0 {
                    continue;
                }
                rt.SetTransform(&(p.place(slide, pose.nod_dip) * previous));
                // The edge in local units: twice the dip width, half of it
                // under the piece.
                let grow = 2.0 * FACE_EDGE_DIP / p.k;
                let edge = p.edge(look);
                let main = solid(p.main_rgb(look), alpha);
                match &p.piece {
                    Piece::Shape { segs, edge: own, .. } => {
                        let g = self.face_path(segs)?;
                        let own = own.unwrap_or(0.0);
                        if let Some(rgb) = edge {
                            brush.SetColor(&solid(rgb, alpha));
                            rt.DrawGeometry(&g, brush, own + grow, &self.round);
                        }
                        brush.SetColor(&main);
                        rt.FillGeometry(&g, brush, None);
                        if own > 0.0 {
                            rt.DrawGeometry(&g, brush, own, &self.round);
                        }
                    }
                    Piece::Stroke { segs, width, .. } => {
                        let g = self.face_path(segs)?;
                        if let Some(rgb) = edge {
                            brush.SetColor(&solid(rgb, alpha));
                            rt.DrawGeometry(&g, brush, width + grow, &self.round);
                        }
                        brush.SetColor(&main);
                        rt.DrawGeometry(&g, brush, *width, &self.round);
                    }
                    Piece::Dot { cx, cy, r, .. } => {
                        if let Some(rgb) = edge {
                            brush.SetColor(&solid(rgb, alpha));
                            rt.FillEllipse(&circle(*cx, *cy, r + grow / 2.0), brush);
                        }
                        brush.SetColor(&main);
                        rt.FillEllipse(&circle(*cx, *cy, *r), brush);
                    }
                    Piece::Ring { cx, cy, r, width, .. } => {
                        if let Some(rgb) = edge {
                            brush.SetColor(&solid(rgb, alpha));
                            rt.DrawEllipse(&circle(*cx, *cy, *r), brush, width + grow, &self.round);
                        }
                        brush.SetColor(&main);
                        rt.DrawEllipse(&circle(*cx, *cy, *r), brush, *width, &self.round);
                    }
                }
            }
            rt.SetTransform(&previous);
        }
        Ok(())
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
            ..CatLook::default()
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
            ..CatLook::default()
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

    /// Every coat of every mascot, wearing `accessory`.
    fn every_coat(accessory: CatAccessory) -> Vec<CatLook> {
        let mut v = Vec::new();
        for &mascot in Mascot::ALL {
            for coat in crate::theme::Coat::all_of(mascot) {
                let mut l = CatLook { mascot, accessory, ..CatLook::default() };
                match coat {
                    crate::theme::Coat::Cat(x) => l.color = x,
                    crate::theme::Coat::Falk(x) => l.falk = x,
                    crate::theme::Coat::Octopus(x) => l.octopus = x,
                }
                v.push(l);
            }
        }
        v
    }

    /// The idle face notch at `size` in `style` wearing `accessory`: the
    /// shape, the eyes' centre and the face's grid unit.
    fn idle_face(
        style: crate::config::Style,
        size: crate::ui::geometry::NotchSize,
        accessory: CatAccessory,
    ) -> (Rect, (f32, f32), f32) {
        use crate::ui::geometry::{face_idle_layout, face_scale, shape_with_robot_sized};
        use crate::ui::robot::face_unit;
        let s = shape_with_robot_sized(style, size, ShapeKind::Idle, true);
        let (area, _) = face_idle_layout(s, true, accessory);
        let shape = Rect { x: s.x, y: s.y, w: s.w, h: s.h };
        let eyes = (area.x + area.w / 2.0, area.y + area.h / 2.0);
        (shape, eyes, face_unit(face_scale(area, accessory)))
    }

    #[test]
    fn face_pieces_hold_3_to_1_or_have_an_edge() {
        use crate::config::Style;
        use crate::ui::geometry::NotchSize;
        use crate::ui::notch_colour::{CREAM as CREAM_TEXT, contrast};
        let sizes = [
            NotchSize::DEFAULT,
            NotchSize::from_px(160, 28),
            NotchSize::from_px(300, 44),
        ];
        let mut over_a_patch = 0;
        for &accessory in CatAccessory::ALL {
            for size in sizes {
                let (shape, eyes, unit) = idle_face(Style::Notch, size, accessory);
                let pieces = face_pieces(accessory, ShapeKind::Idle, shape, eyes, unit);
                assert_eq!(pieces.is_empty(), accessory == CatAccessory::None, "{accessory:?}");
                let coats = every_coat(accessory);
                assert_eq!(coats.len(), 11);
                for look in coats {
                    let outline: Vec<&FacePiece> = pieces.iter().filter(|p| !p.inner).collect();
                    assert!(accessory == CatAccessory::None || !outline.is_empty());
                    for p in outline {
                        let main = p.main_rgb(look);
                        let edge = p.edge(look);
                        // Everything under it: the face and any calico patch.
                        let under = p.colours_under(look);
                        let low = under.iter().any(|&c| contrast(main, c) < 3.0);
                        assert!(
                            !low || edge.is_some(),
                            "{accessory:?} {look:?}: #{main:06X} on {under:06X?} has no edge"
                        );
                        if under.len() > 1 && low {
                            over_a_patch += 1;
                        }
                        // The contrast edge is the face's eye colour where that
                        // reads on everything under it, else cream.
                        if let (Some(e), None) = (edge, p.fixed_edge) {
                            let reads = under.iter().all(|&c| contrast(look.eye_rgb(), c) >= 3.0);
                            if reads {
                                assert_eq!(e, look.eye_rgb());
                            } else {
                                assert!(e == look.eye_rgb() || e == CREAM_TEXT);
                            }
                        }
                        if accessory == CatAccessory::Crown && matches!(p.piece, Piece::Shape { .. }) {
                            assert_eq!(edge, Some(CROWN_EDGE), "{look:?}");
                        }
                    }
                }
            }
        }
        // Some piece really sits on a calico patch it would melt into.
        assert!(over_a_patch > 0);
        assert_eq!(CROWN_EDGE, 0x9A6A12);
    }

    #[test]
    fn a_piece_on_a_calico_patch_is_edged_against_the_patch() {
        use crate::ui::notch_colour::contrast;
        // The raspberry bow straight on the orange patch (top-left of the
        // eyes) of a Calico face: cream under the rest of it reads, the
        // patch does not.
        let calico = CatLook {
            color: CatColor::Calico,
            accessory: CatAccessory::Bow,
            ..CatLook::default()
        };
        let (eyes, unit) = ((100.0, 17.0), 1.28);
        let (cx, cy, _) = ORANGE_CIRCLE;
        let bow = FacePiece {
            piece: Piece::Dot { cx: 0.0, cy: 0.0, r: 2.0, tone: fixed(RASPBERRY) },
            origin: (0.0, 0.0),
            k: 1.0,
            turn: 0.0,
            at: (eyes.0 + (cx - 32.0) * unit, eyes.1 + (cy - 40.0) * unit),
            anchor: FaceAnchor::Corner,
            inner: false,
            fixed_edge: None,
            face: (eyes, unit),
        };
        assert!(contrast(RASPBERRY, CALICO_CREAM) >= 3.0);
        assert!(contrast(RASPBERRY, PATCH_ORANGE) < 3.0);
        assert!(bow.colours_under(calico).contains(&PATCH_ORANGE));
        assert!(bow.edge(calico).is_some());
        // The same piece away from the patch needs none.
        let away = FacePiece { at: (eyes.0, eyes.1 + 3.0 * unit), ..bow };
        assert_eq!(away.colours_under(calico), vec![CALICO_CREAM]);
        assert_eq!(away.edge(calico), None);
    }

    #[test]
    fn the_face_eyes_box_holds_every_eye() {
        use crate::ui::robot::{GLANCE_Y, face_marks};
        // Every eye, open and shut, in every expression: 13 units across
        // each way from the eye line's centre and 7 up (the thinking eyes
        // look up). The typing eyes glance up only from lower down.
        let mut widest = 0.0f32;
        let mut highest = 0.0f32;
        for expr in Expr::ALL {
            for blink in [false, true] {
                for m in face_marks(expr, blink).into_iter().filter(|m| m.k == 1.0) {
                    let (l, t, r, _) = mark_bounds(&m.part);
                    let up = if expr == Expr::Typing { GLANCE_Y } else { 0.0 };
                    widest = widest.max(32.0 - (l + m.at.0)).max(r + m.at.0 - 32.0);
                    highest = highest.max(40.0 - (t + m.at.1) + up);
                }
            }
        }
        assert!(widest <= FACE_EYES_HALF_W, "{widest}");
        assert!(highest <= FACE_EYES_UP, "{highest}");
        // Tight: the box is the eyes', not a guess.
        assert!(FACE_EYES_HALF_W - widest < 0.5 && FACE_EYES_UP - highest < 0.5);
    }

    #[test]
    fn face_hats_are_brims_along_the_top_edge_over_the_face() {
        use crate::config::Style;
        use crate::ui::geometry::NotchSize;
        use crate::ui::robot::{GLANCE_X, LOOK_EYE_DIP};
        let hats = [CatAccessory::Nightcap, CatAccessory::Crown, CatAccessory::Sailor];
        // Worn in every view: they hug the top edge over the face. The bow and
        // the flower stay in the idle notch's left corner.
        for kind in [
            ShapeKind::Idle,
            ShapeKind::MediaCompact,
            ShapeKind::MediaLyric,
            ShapeKind::PomodoroCompact,
            ShapeKind::RecordCompact,
        ] {
            for hat in hats {
                assert!(face_wears(hat, kind), "{hat:?} {kind:?}");
            }
            for corner in [CatAccessory::Bow, CatAccessory::Flower] {
                assert_eq!(face_wears(corner, kind), kind == ShapeKind::Idle, "{corner:?} {kind:?}");
            }
        }
        let look = CatLook::default();
        for style in [Style::Pill, Style::Notch] {
            for hat in hats {
                let what = format!("{style:?} {hat:?}");
                let (shape, eyes, unit) = idle_face(style, NotchSize::DEFAULT, hat);
                let face = face_pieces(hat, ShapeKind::Idle, shape, eyes, unit);
                assert!(!face.is_empty(), "{what}");
                let eye_top = eyes.1 - FACE_EYES_UP * unit;
                let eyes_left = eyes.0 - FACE_EYES_HALF_W * unit - LOOK_EYE_DIP.max(GLANCE_X * unit);
                for p in &face {
                    assert_eq!(p.anchor, FaceAnchor::Rim, "{what}");
                    assert_eq!(p.turn, 0.0, "{what}");
                    // Above the eyes, or (the hanging detail) left of them.
                    let (_, _, r, b) = drawn_box(p);
                    assert!(b <= eye_top || r <= eyes_left, "{what}: {p:?}");
                }
                // The brim: from the top edge, across both eyes, and not a
                // hairline (the mockup's is 8 dip at this size).
                let boxes: Vec<_> = face.iter().map(drawn_box).collect();
                let (l, r) = boxes
                    .iter()
                    .filter(|b| (b.1 - shape.y).abs() < 1e-3)
                    .fold((f32::MAX, f32::MIN), |a, b| (a.0.min(b.0), a.1.max(b.2)));
                let depth = boxes.iter().filter(|b| b.3 <= eye_top).fold(0.0f32, |a, b| a.max(b.3 - shape.y));
                assert!(l < eyes.0 - 8.0 * unit && r > eyes.0 + 8.0 * unit, "{what}: {l}..{r}");
                assert!(depth >= 6.0, "{what}: {depth} dip deep");
                // Its own colours and the signature detail, hanging left of
                // the eyes.
                let hangs = |p: &&FacePiece| {
                    let (_, _, r, b) = drawn_box(p);
                    b > eye_top && r <= eyes_left
                };
                let rgb = |p: &FacePiece| p.main_rgb(look);
                match hat {
                    CatAccessory::Nightcap => {
                        assert!(face.iter().any(|p| matches!(p.piece, Piece::Shape { .. }) && rgb(p) == NAVY));
                        assert!(face.iter().any(|p| matches!(p.piece, Piece::Shape { .. }) && rgb(p) == CREAM_PIECE));
                        // The tip flops down on the left, ending in the pompom.
                        let hanging: Vec<_> = face.iter().filter(hangs).collect();
                        assert!(hanging.iter().any(|p| matches!(p.piece, Piece::Stroke { .. }) && rgb(p) == NAVY), "{what}");
                        assert!(hanging.iter().any(|p| matches!(p.piece, Piece::Dot { .. }) && rgb(p) == CREAM_PIECE), "{what}");
                    }
                    CatAccessory::Crown => {
                        let band: Vec<_> = face.iter().filter(|p| rgb(p) == GOLD).collect();
                        assert_eq!(band.len(), 1, "{what}");
                        assert_eq!(band[0].fixed_edge, Some(CROWN_EDGE));
                        // The zig-zag lower edge: more than a box's corners.
                        let Piece::Shape { segs, .. } = &band[0].piece else { panic!("{what}") };
                        assert!(segs.len() >= 8, "{what}");
                        // Three jewels on the band, left to right.
                        let mut jewels: Vec<_> = face.iter().filter(|p| matches!(p.piece, Piece::Dot { .. })).collect();
                        jewels.sort_by(|a, b| drawn_box(a).0.total_cmp(&drawn_box(b).0));
                        assert_eq!(jewels.iter().map(|p| rgb(p)).collect::<Vec<_>>(), vec![RASPBERRY, TIE_KNOT, RASPBERRY]);
                        assert!(jewels.iter().all(|p| p.inner));
                    }
                    CatAccessory::Sailor => {
                        assert!(face.iter().any(|p| rgb(p) == SAILOR_WHITE && !p.inner));
                        // The ribbon stripe sits on the white band; the rim under it.
                        assert!(face.iter().any(|p| rgb(p) == NAVY && p.inner));
                        assert!(face.iter().any(|p| rgb(p) == NAVY && !p.inner && !hangs(&p)));
                        // Two ribbon tails on the left.
                        assert_eq!(face.iter().filter(hangs).filter(|p| rgb(p) == NAVY).count(), 2, "{what}");
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn every_accessory_has_pieces_on_the_right_anchor() {
        assert!(pieces(Mascot::Cat, CatAccessory::None).is_empty());
        assert_eq!(CatAccessory::ALL.len(), 10);
        for &a in &CatAccessory::ALL[1..] {
            assert!(!pieces(Mascot::Cat, a).is_empty(), "{a:?}");
            assert!(
                !accessory_corners(Mascot::Cat, a, Ears::default()).is_empty(),
                "{a:?}"
            );
        }
        let anchors = |a| {
            pieces(Mascot::Cat, a)
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
        let rings: Vec<_> = pieces(Mascot::Cat, CatAccessory::Glasses)
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
            let c = accessory_corners(Mascot::Cat, CatAccessory::Bow, ears);
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
        let g = |ears| accessory_corners(Mascot::Cat, CatAccessory::Glasses, ears);
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
                for m in Mascot::ALL {
                    for accessory in CatAccessory::ALL {
                        for corners in accessory_piece_corners(*m, *accessory, f.ears) {
                            let piece = corners.iter().fold(
                                (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                                |b, &((x, y), _)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)),
                            );
                            for part in &cream {
                                let mark = mark_bounds(part);
                                assert!(
                                    !overlap(piece, mark),
                                    "{m:?} {accessory:?} piece {piece:?} covers {expr:?}'s {part:?} at {mark:?}"
                                );
                            }
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
                for m in Mascot::ALL {
                    for accessory in CatAccessory::ALL {
                        for corners in accessory_piece_corners(*m, *accessory, f.ears) {
                            let piece = corners.iter().fold(
                                (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
                                |b, &((x, y), _)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)),
                            );
                            assert!(
                                !overlap(piece, swept),
                                "{m:?} {accessory:?} piece {piece:?} is in the way of {expr:?}'s risen {part:?} ({swept:?})"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn calico_patches_match_the_site_in_every_ear_state() {
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
            let both = |x: f32, y: f32| (contains(&orange, x, y), contains(&dark, x, y));
            // The ears above the head: the left one orange, the right one dark.
            assert_eq!(both(14.0, 18.0), (true, false), "{ears:?}");
            assert_eq!(both(50.0, 18.0), (false, true), "{ears:?}");
            // The circles on the head: orange top-left, dark bottom-right.
            assert_eq!(both(12.0, 30.0), (true, false), "{ears:?}");
            assert_eq!(both(50.0, 50.0), (false, true), "{ears:?}");
            // The forehead and the face centre stay cream.
            for (x, y) in [(32.0, 30.0), (32.0, 40.0)] {
                assert_eq!(both(x, y), (false, false), "{ears:?} ({x}, {y})");
            }
            // The ears' base corners lie under the cream head, as on the site.
            for (x, y) in [(38.0, 27.0), (55.0, 38.0)] {
                assert_eq!(both(x, y), (false, false), "{ears:?} ({x}, {y})");
            }
            // Nothing outside the silhouette; the orange circle reaches x 3.7
            // but is clipped to the head.
            for (x, y) in [
                (0.0, 0.0),
                (32.0, 2.0),
                (60.0, 60.0),
                (-5.0, 30.0),
                (3.0, 30.0),
            ] {
                assert_eq!(both(x, y), (false, false), "{ears:?} ({x}, {y})");
            }
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
    fn the_calico_swatch_is_the_sites_conic_gradient() {
        let slices = calico_swatch_slices();
        assert_eq!(slices[0].0, 0.0);
        assert_eq!(slices[2].1, 360.0);
        for w in slices.windows(2) {
            assert_eq!(w[0].1, w[1].0, "slices are contiguous");
        }
        assert_eq!(
            slices.map(|(_, _, rgb)| rgb),
            [0xF4EDE4, PATCH_ORANGE, PATCH_DARK]
        );
        let calico = CatLook {
            color: CatColor::Calico,
            accessory: CatAccessory::None,
            ..CatLook::default()
        };
        assert_eq!(slices[0].2, calico.head_rgb());
        assert_eq!(
            slices.map(|(a, b, _)| (a, b)),
            [(0.0, 180.0), (180.0, 270.0), (270.0, 360.0)]
        );
    }

    #[test]
    fn accessory_paths_are_built_once() {
        let p = painter();
        for &m in Mascot::ALL {
            for &a in &CatAccessory::ALL[1..] {
                let built = p.extras(m, a);
                assert_eq!(built.len(), pieces(m, a).len());
                assert!(Rc::ptr_eq(&built, &p.extras(m, a)), "{m:?} {a:?} is kept");
            }
        }
        assert_eq!(p.extras.borrow().len(), 27);
    }

    /// A literal copy of the cat's pieces before the mascots arrived.
    fn old_pieces(accessory: CatAccessory) -> Vec<(Anchor, Piece)> {
        use Seg::{Close, Line as To, Move, Quad};
        match accessory {
            // Drawn by the mascot painters; the cat has no pieces for these.
            CatAccessory::None
            | CatAccessory::Headphone
            | CatAccessory::Sailor
            | CatAccessory::Bowtie => vec![],
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

    fn bounds_of(p: &[(Anchor, Piece)]) -> (f32, f32, f32, f32) {
        p.iter()
            .fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |b, (_, piece)| {
                let (l, t, r, bt) = piece_bounds(piece);
                (b.0.min(l), b.1.min(t), b.2.max(r), b.3.max(bt))
            })
    }

    #[test]
    fn the_cats_accessories_are_unchanged() {
        for acc in &CatAccessory::ALL[..7] {
            assert_eq!(pieces(Mascot::Cat, *acc), old_pieces(*acc), "{acc:?}");
        }
    }

    #[test]
    fn every_mascot_wears_every_accessory() {
        for m in Mascot::ALL {
            for acc in CatAccessory::ALL {
                let p = pieces(*m, *acc);
                assert_eq!(p.is_empty(), *acc == CatAccessory::None, "{m:?} {acc:?}");
                if *m != Mascot::Cat {
                    assert!(
                        p.iter().all(|(a, _)| !matches!(a, Anchor::Ear(_))),
                        "{m:?} {acc:?} rides no ear"
                    );
                }
            }
        }
    }

    #[test]
    fn caps_sit_on_each_mascots_top() {
        for m in Mascot::ALL {
            let (_, t, _, b) = bounds_of(&pieces(*m, CatAccessory::Sailor));
            let top = anchors(*m).top.1;
            assert!(
                t >= top - 9.5 && b <= top + 3.5,
                "{m:?}: {t}..{b} around {top}"
            );
        }
        assert_eq!(anchors(Mascot::Octopus).top, (32.0, 9.0));
    }

    #[test]
    fn the_falk_crown_is_outlined_and_its_scarf_navy() {
        let crown = pieces(Mascot::Falk, CatAccessory::Crown);
        assert!(crown.iter().any(|(_, p)| matches!(
            p,
            Piece::Stroke {
                tone: Tone::Fixed(0x7A5210),
                ..
            } | Piece::Shape {
                fill: Tone::Fixed(0x7A5210),
                ..
            }
        )));
        let scarf = pieces(Mascot::Falk, CatAccessory::Scarf);
        assert!(scarf.iter().any(|(_, p)| matches!(
            p,
            Piece::Shape {
                fill: Tone::Fixed(NAVY),
                ..
            }
        )));
    }
}
