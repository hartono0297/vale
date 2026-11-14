//! The stories themselves: what each one does to the island, the clock and
//! the mascot at any moment of its timeline (`Shot`). Pure: the renderer
//! paints a shot, nothing here touches Direct2D.

pub mod bantal_angka;
pub mod bungee;
pub mod kelelawar;
pub mod maling_angka;
pub mod peluk_pulau;
pub mod penasaran_jam;
pub mod petak_umpet;
pub mod piano_ekor;
pub mod syal_ekor;
pub mod terbang_keliling;

use std::f32::consts::{PI, TAU};

use windows_numerics::{Matrix3x2, Vector2};

use crate::config::Style;
use crate::theme::Mascot;
use crate::ui::cat_style::CatLook;
use crate::ui::geometry::{IdleLayout, ROBOT_COMPACT, Rect, ShapeRect, shoulder_depth};
use crate::ui::mascot::CrestPose;
use crate::ui::play::shape::{Deform, outline, turn_about};
use crate::ui::play::state::{Beat, spread_offsets, wind_progress};
use crate::ui::play::trigger::Story;
use crate::ui::render::DigitFx;
use crate::ui::robot::{Expr, GRID, Pose, Seg};

/// The story has a timeline to play. Only ready stories enter the pool
/// (`trigger::pool`), so a mascot with none never plays.
pub fn ready(story: Story) -> bool {
    matches!(
        story,
        Story::MalingAngka
            | Story::SyalEkor
            | Story::PetakUmpet
            | Story::BantalAngka
            | Story::PianoEkor
            | Story::PenasaranJam
            | Story::PelukPulau
            | Story::Bungee
            | Story::Kelelawar
            | Story::TerbangKeliling
    )
}

/// What a story plays on: the island, where the play stands, the mascot's
/// look (its body's coat; the head's painter takes it from the renderer),
/// and the idle layout the renderer places the slot and the clock by before
/// the play spreads them (`idle_layout_lead` with the frame's own robot,
/// clock and lead), so a story lines up with them whatever they are.
/// `minute` is the clock's last character as the frame shows it (`None`
/// without the clock): a story that swaps the minute shows the real next one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stage {
    pub style: Style,
    pub shape: ShapeRect,
    pub beat: Beat,
    pub look: CatLook,
    pub layout: IdleLayout,
    pub minute: Option<char>,
}

impl Stage {
    /// The slot and the clock where the renderer puts them at this beat.
    pub fn room(&self) -> IdleLayout {
        spread_offsets(self.layout, self.shape, self.beat.spread)
    }

    /// The idle island with the mascot, the clock (the mockup's 14:01) on or
    /// off and no lead.
    #[cfg(test)]
    pub fn idle(style: Style, shape: ShapeRect, beat: Beat, look: CatLook, clock: bool) -> Stage {
        let layout = crate::ui::geometry::idle_layout_lead(shape, true, clock, 0.0);
        Stage { style, shape, beat, look, layout, minute: clock.then_some('1') }
    }
}

/// One moment of a story, for the renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub deform: Deform,
    /// One per character of the clock (`%H:%M`).
    pub digits: [DigitFx; 5],
    /// What the mascot's slot shows.
    pub slot: Slot,
    /// How much of the see-through halo a story head in the slot keeps, 0 to
    /// 1 (1 as the app draws it): a story fades it where its own figure,
    /// which has none, takes over from the slot head.
    pub halo: f32,
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
            slot: Slot::App,
            halo: 1.0,
            parts: Vec::new(),
        }
    }

    /// The story's head in the slot, if it moves one.
    #[cfg(test)]
    pub fn head(&self) -> Option<MascotShot> {
        match self.slot {
            Slot::Story(m) => Some(m),
            _ => None,
        }
    }

    /// Everything the shot paints, in window dip. Every story's tests check
    /// it stays inside the window.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bounds(&self) -> Rect {
        self.parts.iter().map(|p| p.draw.bounds()).fold(self.island, union)
    }
}

/// What fills the mascot's slot while a story plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slot {
    /// The app's own head, as it draws it at rest.
    App,
    /// The head as the story moves it, painted in `Layer::Dark` (clipped to
    /// the island).
    Story(MascotShot),
    /// Nothing: the mascot has left its slot and the story draws it as parts.
    Empty,
}

/// Where a part is painted, as the mockup's `compose` stacks it (see
/// `PAINT_ORDER`). Nothing leans on the island's fill being opaque: what
/// must not show is clipped away.
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
#[derive(Clone, Debug, PartialEq)]
pub enum Draw {
    /// The filled figures of `path` (each `Move` starts one), nonzero as the
    /// mockup's SVG fills: figures drawn the same way round fill where they
    /// overlap.
    Fill { path: Vec<Seg>, rgb: u32 },
    /// `Fill`, translucent: the brush at `alpha` (times the part's), drawn
    /// straight over what is under it, no layer. A limb's shade band is
    /// black at 0.17.
    Tint { path: Vec<Seg>, rgb: u32, alpha: f32 },
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
    /// The clock's character `index`, moved from where the clock draws it
    /// by `at - home` and turned by `rot_deg` about its cell's centre,
    /// outlined in the island's colour so it reads over the mascot and off
    /// the island. `home` is where the story reckons that centre to be
    /// (`DIGIT_X`) and `at` where it carries the character, so a digit
    /// taken up at `home` starts exactly on the clock's own, whatever the
    /// typeface; the bounds take `at` within `NEAR_SLACK`.
    Digit { index: usize, at: (f32, f32), rot_deg: f32, home: (f32, f32) },
    /// The clock's character `index` in its own cell, where the clock lays it
    /// out on the island at rest, under `fx` as the clock's per-character
    /// path draws it (about its cell's centre, `fx.ch` in its place),
    /// outlined as a `Digit` is. `near` is where the story reckons that
    /// centre to be (`DIGIT_X`); only the bounds use it. It does not dim
    /// with the clock beside a sleeping Bata (a play never starts asleep).
    ClockChar { index: usize, fx: DigitFx, near: (f32, f32) },
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
    /// `items` cut to `clip`'s figures (filled nonzero): what falls outside
    /// it is not drawn (the Calico cat's patches on its body).
    Clipped { clip: Vec<Seg>, items: Vec<Draw> },
}

/// The head's ink on its 64-unit grid, whichever mascot, face but the typing
/// laptop, crest (up to the startle's 1.75) and accessory: (left, top, right,
/// bottom). Measured -6.6, -15.0 (the falk's startled crest), 70.6 (the
/// octopus's arms), 64.6, plus 2.4 to 4 units for the pixels the measure
/// rounds to (`a_head_part_stays_inside_its_bounds` measures it).
pub(crate) const HEAD_INK: (f32, f32, f32, f32) = (-9.0, -19.0, 73.0, 68.0);

/// `HEAD_INK` for the octopus as its mantle alone (`MascotShot::bare_octopus`),
/// any face but the typing laptop, any accessory, flushed or not. Measured
/// -5, -8 (the sung note), 70 (the sung note), 55 (the scarf of the waving
/// face), plus 1.5 units for the pixels the measure rounds to. The mantle
/// itself is x 9..55, y 9..46; the rest is faces' marks and accessories. Only
/// an octopus story draws a bare head.
pub(crate) const BARE_OCTOPUS_INK: (f32, f32, f32, f32) = (-6.5, -9.5, 71.5, 56.5);

impl Draw {
    /// The box the part's ink stays in, window dip.
    pub fn bounds(&self) -> Rect {
        self.bounds_at(ROBOT_COMPACT / GRID)
    }

    /// `bounds`, a head being `unit` per grid unit.
    fn bounds_at(&self, unit: f32) -> Rect {
        match self {
            Draw::Fill { path, .. } | Draw::Tint { path, .. } => path_box(path, 0.0),
            Draw::Stroke { path, width, .. } => path_box(path, width / 2.0),
            Draw::Ellipse { c, r, .. } => Rect {
                x: c.0 - r.0,
                y: c.1 - r.1,
                w: 2.0 * r.0,
                h: 2.0 * r.1,
            },
            Draw::Head { at, shot } => {
                let (l, t, r, b) = if shot.bare_octopus { BARE_OCTOPUS_INK } else { HEAD_INK };
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
            Draw::Digit { at, .. } => {
                let reach = DIGIT_REACH + NEAR_SLACK;
                Rect {
                    x: at.0 - reach,
                    y: at.1 - reach,
                    w: 2.0 * reach,
                    h: 2.0 * reach,
                }
            }
            // Scaled up to its widest, moved, and the reckoned centre
            // `NEAR_SLACK` off DirectWrite's either way.
            Draw::ClockChar { fx, near, .. } => {
                let reach = DIGIT_REACH * fx.scale * fx.scale_x.max(1.0) + NEAR_SLACK;
                Rect {
                    x: near.0 + fx.dx - reach,
                    y: near.1 + fx.dy - reach,
                    w: 2.0 * reach,
                    h: 2.0 * reach,
                }
            }
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
            Draw::Clipped { clip, items } => {
                let mut boxes = items.iter().map(|d| d.bounds_at(unit));
                // Empty, the clip's own box: never a point that pulls a
                // group's union to the origin.
                let Some(first) = boxes.next() else {
                    return path_box(clip, 0.0);
                };
                let (a, b) = (boxes.fold(first, union), path_box(clip, 0.0));
                let (x, y) = (a.x.max(b.x), a.y.max(b.y));
                Rect {
                    x,
                    y,
                    w: ((a.x + a.w).min(b.x + b.w) - x).max(0.0),
                    h: ((a.y + a.h).min(b.y + b.h) - y).max(0.0),
                }
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
pub(crate) fn path_box(path: &[Seg], pad: f32) -> Rect {
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
    /// The octopus as its mantle alone, without its six short arms: a story
    /// that draws the long arms itself (`art::octopus`) turns it on, or the
    /// arms would double. The coat, face and accessory come along; the
    /// other mascots ignore it.
    pub bare_octopus: bool,
    /// The octopus's flush, 0 to 1: its mantle (and the long arms, `arm`)
    /// mixed toward the flush colour. Never lowers the app's own flush.
    pub flush: f32,
    /// How much of the head shows, 0 to 1: the slot head and its halo fade
    /// as one through a layer (a head part fades by its part's alpha).
    pub alpha: f32,
}

impl MascotShot {
    /// The head at rest in its slot with the idle face (the falk's idle
    /// crest; the other mascots have none).
    pub const REST: MascotShot = MascotShot {
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
        bare_octopus: false,
        flush: 0.0,
        alpha: 1.0,
    };

    /// `base` (the app's pose: its look, its lift) with the story's face on it.
    pub fn pose(&self, base: Pose) -> Pose {
        Pose {
            blink: self.blink,
            crest: Some(self.crest),
            beak_open: Some(self.beak_open),
            bare_octopus: self.bare_octopus,
            flush: base.flush.max(self.flush),
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

/// How far a story's reckoning of a clock character's centre (`DIGIT_X`)
/// may be from where DirectWrite puts it, either way (dip): 1.6 for DM
/// Sans, 1.1 for JetBrains Mono (`the_stories_digit_table_is_within_its_slack_of_the_clock`).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const NEAR_SLACK: f32 = 2.0;

/// The clock's character centres from the clock box's centre (DM Sans at
/// 16, `%H:%M`), as the mockup measures them: where a story aims at a
/// character (an arm's reach, a tail's tap, a seat beside it). The clock's
/// own characters are placed by DirectWrite, within `NEAR_SLACK` of these;
/// a carried digit is placed from its own (`Draw::Digit`'s `home`). The one
/// table every story uses.
pub(crate) const DIGIT_X: [f32; 5] = [-17.2, -7.6, 0.0, 7.6, 17.2];

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

/// `story`'s shot on `stage`.
pub fn shot(story: Story, stage: &Stage) -> Shot {
    let shot = match story {
        Story::MalingAngka => maling_angka::shot(stage),
        Story::SyalEkor => syal_ekor::shot(stage),
        Story::PetakUmpet => petak_umpet::shot(stage),
        Story::BantalAngka => bantal_angka::shot(stage),
        Story::PianoEkor => piano_ekor::shot(stage),
        Story::PenasaranJam => penasaran_jam::shot(stage),
        Story::PelukPulau => peluk_pulau::shot(stage),
        Story::Bungee => bungee::shot(stage),
        Story::Kelelawar => kelelawar::shot(stage),
        Story::TerbangKeliling => terbang_keliling::shot(stage),
    };
    debug_assert!(bare_heads_fit(story, &shot), "{story:?} draws a bare head");
    shot
}

/// Every head of `shot` asking for the bare mantle is in an octopus story:
/// only the octopus's painter drops the arms, so only there does the bare
/// box (`BARE_OCTOPUS_INK`) hold what is painted.
fn bare_heads_fit(story: Story, shot: &Shot) -> bool {
    fn bare(d: &Draw) -> bool {
        match d {
            Draw::Head { shot, .. } => shot.bare_octopus,
            Draw::Group { items, .. } | Draw::Clipped { items, .. } => items.iter().any(bare),
            _ => false,
        }
    }
    let any = matches!(shot.slot, Slot::Story(m) if m.bare_octopus) || shot.parts.iter().any(|p| bare(&p.draw));
    !any || story.mascot() == Mascot::Octopus
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
    /// Cubic in (`i`).
    I,
    /// Quadratic in (`qi`).
    Qi,
    /// An overshoot (`ob`).
    Ob,
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
            Ease::I => t * t * t,
            Ease::Qi => t * t,
            Ease::Ob => {
                let (c1, c3) = (1.9, 2.9);
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
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

/// A point `off` outside one end of the island (the mockup's `rimPt`): the
/// left end for `side` -1, the right for 1; `u` 0 at the top, 1 where the
/// lip starts. The Notch's straight side to 0.55, then its corner; the
/// Pill's half circle all the way.
fn rim_pt(style: Style, s: ShapeRect, side: f32, u: f32, off: f32) -> (f32, f32) {
    let r = crate::ui::geometry::corner_radius(style, s);
    let (top, bottom) = (s.y, s.y + s.h);
    let (edge, cx) = if side < 0.0 { (s.x, s.x + r) } else { (s.x + s.w, s.x + s.w - r) };
    let big = r + off;
    if style == Style::Pill {
        let a = (-90.0 + 180.0 * u.clamp(0.0, 1.0)).to_radians();
        return (cx + side * big * a.cos(), top + s.h / 2.0 + big * a.sin());
    }
    if u < 0.55 {
        return (edge + side * off, lerp(top - off, bottom - r, u / 0.55));
    }
    let a = (90.0 * (u - 0.55) / 0.45).to_radians();
    (cx + side * big * a.cos(), bottom - r + big * a.sin())
}

/// The crouched cat's size, where its hop starts and ends.
const CROUCH: f32 = 0.9;

/// The cat's hop (the mockup's `hopIn`/`hopOut`): how deep in the dark it is
/// (`dk`), its size, its lift (dip, negative up) and its squash.
#[derive(Clone, Copy)]
struct Hop {
    dk: f32,
    sc: f32,
    y: f32,
    sq: f32,
}

/// Out of the dark from `a`: settle, a small hop forward (growing a touch),
/// a soft landing.
fn hop_in(t: f32, a: f32) -> Hop {
    let k = ((t - a) / 0.42).clamp(0.0, 1.0);
    Hop {
        dk: 1.0 - Ease::Sio.at(((t - a - 0.08) / 0.3).clamp(0.0, 1.0)),
        sc: lerp(CROUCH, 1.0, Ease::O.at(k)),
        y: -3.2 * (PI * ((t - a - 0.12) / 0.3).clamp(0.0, 1.0)).sin(),
        sq: 1.0 - 0.08 * bump(t, a - 0.02, 0.16) + 0.06 * bump(t, a + 0.14, 0.18) + ring(t, a + 0.42, -0.09, 3.0, 6.0),
    }
}

/// Back into the dark from `a`.
fn hop_out(t: f32, a: f32) -> Hop {
    let k = ((t - a) / 0.4).clamp(0.0, 1.0);
    Hop {
        dk: Ease::Sio.at(((t - a - 0.1) / 0.3).clamp(0.0, 1.0)),
        sc: lerp(1.0, CROUCH, Ease::Io.at(k)),
        y: -2.4 * (PI * ((t - a - 0.08) / 0.28).clamp(0.0, 1.0)).sin(),
        sq: 1.0 - 0.08 * bump(t, a - 0.05, 0.16),
    }
}

/// The mockup's `mixHop`: in at `a_in`, out at `a_out`.
fn mix_hop(t: f32, a_in: f32, a_out: f32) -> Hop {
    if t < a_out - 0.1 { hop_in(t, a_in) } else { hop_out(t, a_out) }
}

/// How a story fades or moves something home during a wind-down: by the
/// wind-down's time, never by bands of `alpha`, which eases and crosses its
/// middle band in about one frame. 1 until the wind-down is `from` of the
/// way through its time, then falling evenly to 0 at `to` (1 while the
/// story plays). A fade may change by `FRAME_FADE` at most in a 60 Hz frame,
/// so `to - from` is at least 0.56 (112 of the wind-down's 200 ms).
fn kept(alpha: f32, from: f32, to: f32) -> f32 {
    1.0 - ((wind_progress(alpha) - from) / (to - from)).clamp(0.0, 1.0)
}

/// `fx` eased toward the character at rest as `alpha` falls to 0.
fn toward_rest(fx: DigitFx, alpha: f32) -> DigitFx {
    let id = DigitFx::IDENTITY;
    DigitFx {
        dx: lerp(id.dx, fx.dx, alpha),
        dy: lerp(id.dy, fx.dy, alpha),
        rot_deg: lerp(id.rot_deg, fx.rot_deg, alpha),
        scale: lerp(id.scale, fx.scale, alpha),
        scale_x: lerp(id.scale_x, fx.scale_x, alpha),
        alpha: lerp(id.alpha, fx.alpha, alpha),
        ch: if alpha > 0.0 { fx.ch } else { id.ch },
    }
}

/// The sizes every story is tested at: the tightest, the default, the largest.
#[cfg(test)]
pub(crate) const CONTRACT_SIZES: [(f32, f32); 3] = [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)];

/// `shot` is exactly at rest: the island, the clock, the slot (the app's
/// head, or a story head equal to the resting one) and no part showing.
#[cfg(test)]
pub(crate) fn at_rest(style: Style, shot: &Shot) -> Result<(), String> {
    if !crate::ui::play::shape::is_rest(style, &shot.deform) {
        return Err(format!("the island is deformed: {:?}", shot.deform));
    }
    if shot.digits.iter().any(|d| *d != DigitFx::IDENTITY) {
        return Err(format!("the clock moved: {:?}", shot.digits));
    }
    if !matches!(shot.slot, Slot::App) && shot.slot != Slot::Story(MascotShot::REST) {
        return Err(format!("the slot is not at rest: {:?}", shot.slot));
    }
    if shot.halo != 1.0 {
        return Err(format!("the slot head's halo is {}", shot.halo));
    }
    near_rest(shot, 0.0)
}

/// The mascot is within `tol` of its rest: the slot holds the app's head or
/// a story head that far from the resting one, with the idle face, and no
/// part shows more than `tol`.
#[cfg(test)]
pub(crate) fn near_rest(shot: &Shot, tol: f32) -> Result<(), String> {
    let r = MascotShot::REST;
    match shot.slot {
        Slot::App => {}
        Slot::Story(m) => {
            let off = [
                m.dx, m.dy, m.rot_deg, m.sx - r.sx, m.sy - r.sy, m.glance.0, m.glance.1, m.flush,
                m.crest.rot_deg - r.crest.rot_deg, m.crest.scale - r.crest.scale, m.alpha - r.alpha,
            ];
            if off.iter().any(|v| v.abs() > tol) || m.expr != r.expr || m.blink || m.beak_open {
                return Err(format!("the slot head is not at rest: {m:?}"));
            }
        }
        Slot::Empty => return Err("the slot is empty".into()),
    }
    if (shot.halo - 1.0).abs() > tol {
        return Err(format!("the slot head's halo is {}", shot.halo));
    }
    match shot.parts.iter().find(|p| p.alpha > tol) {
        Some(p) => Err(format!("a part shows: {p:?}")),
        None => Ok(()),
    }
}

/// The four tests every story passes, in its tests module:
/// `story_contract_tests!(Story::X, shot)`, `shot` the story's
/// `fn(&Stage) -> Shot`. It starts and ends exactly at rest; `alpha` 0 is
/// rest at any moment; nothing leaves the window, wound down or not; the
/// mascot is at rest just after the start and just before the end. With the
/// clock on and, unless the story needs it, off.
#[cfg(test)]
macro_rules! story_contract_tests {
    ($story:expr, $shot:expr) => {
        /// The story played by its own mascot, the clock on or off.
        fn contract_shot(style: $crate::config::Style, w: f32, h: f32, beat: $crate::ui::play::state::Beat, clock: bool) -> $crate::ui::play::stories::Shot {
            let look = $crate::ui::cat_style::CatLook { mascot: $story.mascot(), ..Default::default() };
            let s = $crate::ui::play::testing::shape(style, w, h);
            $shot(&$crate::ui::play::stories::Stage::idle(style, s, beat, look, clock))
        }

        /// The clock states the story plays with.
        fn contract_clocks() -> Vec<bool> {
            if $story.needs_clock() { vec![true] } else { vec![true, false] }
        }

        #[test]
        fn the_story_starts_and_ends_at_rest_in_both_styles() {
            use $crate::config::Style;
            use $crate::ui::play::state::Beat;
            for style in [Style::Notch, Style::Pill] {
                for (w, h) in $crate::ui::play::stories::CONTRACT_SIZES {
                    for clock in contract_clocks() {
                        for t in [0.0, $story.secs()] {
                            for spread in [0.0, 1.0] {
                                let shot = contract_shot(style, w, h, Beat { spread, alpha: 1.0, t, cut_at: None }, clock);
                                if let Err(why) = $crate::ui::play::stories::at_rest(style, &shot) {
                                    panic!("{style:?} {w}x{h} clock={clock} t={t} spread={spread}: {why}");
                                }
                            }
                        }
                    }
                }
            }
        }

        #[test]
        fn alpha_zero_is_rest_at_any_moment() {
            use $crate::config::Style;
            use $crate::ui::play::state::Beat;
            for style in [Style::Notch, Style::Pill] {
                for clock in contract_clocks() {
                    let mut t = 0.0;
                    while t <= $story.secs() {
                        for spread in [0.0, 1.0] {
                            let shot = contract_shot(style, 190.0, 34.0, Beat { spread, alpha: 0.0, t, cut_at: None }, clock);
                            if let Err(why) = $crate::ui::play::stories::at_rest(style, &shot) {
                                panic!("{style:?} clock={clock} t={t} spread={spread}: {why}");
                            }
                        }
                        t += 0.1;
                    }
                }
            }
        }

        #[test]
        fn nothing_leaves_the_window_at_any_size_or_style() {
            use $crate::config::Style;
            use $crate::ui::geometry::{WINDOW_H, WINDOW_W};
            use $crate::ui::play::state::Beat;
            for style in [Style::Notch, Style::Pill] {
                for (w, h) in $crate::ui::play::stories::CONTRACT_SIZES {
                    for clock in contract_clocks() {
                        // Playing, and wound down part way (the spread falls with it).
                        for alpha in [1.0, 0.5, 0.2] {
                            let mut t = 0.0;
                            while t <= $story.secs() {
                                let beat = Beat { spread: alpha, alpha, t, cut_at: None };
                                let b = contract_shot(style, w, h, beat, clock).bounds();
                                // Above the screen's top edge is hidden by it, as the
                                // mockup's bezel hides it.
                                assert!(b.x >= 0.0 && b.x + b.w <= WINDOW_W && b.y + b.h <= WINDOW_H,
                                    "{style:?} {w}x{h} clock={clock} alpha={alpha} t={t}: {b:?}");
                                t += 0.04;
                            }
                        }
                    }
                }
            }
        }

        #[test]
        fn the_mascot_rests_just_after_the_start_and_just_before_the_end() {
            use $crate::config::Style;
            use $crate::ui::play::state::Beat;
            for style in [Style::Notch, Style::Pill] {
                for (w, h) in $crate::ui::play::stories::CONTRACT_SIZES {
                    for clock in contract_clocks() {
                        for t in [0.001, $story.secs() - 0.001] {
                            let shot = contract_shot(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, clock);
                            if let Err(why) = $crate::ui::play::stories::near_rest(&shot, 1e-3) {
                                panic!("{style:?} {w}x{h} clock={clock} t={t}: {why}");
                            }
                        }
                    }
                }
            }
        }

        #[test]
        fn a_wind_down_fades_every_part_and_the_halo_frame_by_frame() {
            use $crate::config::Style;
            for style in [Style::Notch, Style::Pill] {
                for clock in contract_clocks() {
                    for part in [0.3, 0.5, 0.7] {
                        let cut = $story.secs() * part;
                        for phase in [0.0, 8.0] {
                            let shots = $crate::ui::play::stories::wind_down_frames($story, cut, phase, |beat| contract_shot(style, 190.0, 34.0, beat, clock));
                            if let Err(why) = $crate::ui::play::stories::fades_by_frames(&shots) {
                                panic!("{style:?} clock={clock} cut at {cut}: {why}");
                            }
                        }
                    }
                }
            }
        }

        #[test]
        fn the_slot_heads_alpha_changes_by_a_frames_fade_at_most() {
            use $crate::config::Style;
            use $crate::ui::play::state::Beat;
            use $crate::ui::play::stories::{slot_head_fades_by_frames, wind_down_frames};
            for style in [Style::Notch, Style::Pill] {
                for clock in contract_clocks() {
                    // Playing, one 60 Hz frame at a time, from two phases.
                    for phase in [0.0, 1.0 / 120.0] {
                        let mut frames = Vec::new();
                        let mut t = phase;
                        while t <= $story.secs() {
                            let beat = Beat { spread: 1.0, alpha: 1.0, t, cut_at: None };
                            frames.push((t * 1000.0, beat, contract_shot(style, 190.0, 34.0, beat, clock)));
                            t += 1.0 / 60.0;
                        }
                        if let Err(why) = slot_head_fades_by_frames(&frames) {
                            panic!("{style:?} clock={clock} playing: {why}");
                        }
                    }
                    // Wound down from a cut every 0.1 s.
                    let mut cut = 0.05;
                    while cut < $story.secs() {
                        for phase in [0.0, 8.0] {
                            let shots = wind_down_frames($story, cut, phase, |beat| contract_shot(style, 190.0, 34.0, beat, clock));
                            if let Err(why) = slot_head_fades_by_frames(&shots) {
                                panic!("{style:?} clock={clock} cut at {cut}: {why}");
                            }
                        }
                        cut += 0.1;
                    }
                }
            }
        }
    };
}
#[cfg(test)]
pub(crate) use story_contract_tests;

/// The most a part's alpha, the halo or the slot head's alpha may change in
/// one 60 Hz frame of a wind-down; the slot head's while it plays too.
#[cfg(test)]
pub(crate) const FRAME_FADE: f32 = 0.15;

/// The slot head's alpha in `shot`: the app's head is whole, an empty slot
/// has none to compare (a story empties it only under a figure that has
/// taken its place whole).
#[cfg(test)]
fn slot_head_alpha(shot: &Shot) -> Option<f32> {
    match shot.slot {
        Slot::App => Some(1.0),
        Slot::Story(m) => Some(m.alpha),
        Slot::Empty => None,
    }
}

/// The slot head's alpha changes by at most `FRAME_FADE` from one frame of
/// `shots` to the next (a frame with the slot empty is skipped).
#[cfg(test)]
pub(crate) fn slot_head_fades_by_frames(shots: &[(f32, Beat, Shot)]) -> Result<(), String> {
    for w in shots.windows(2) {
        let ((_, _, a), (ms, beat, b)) = (&w[0], &w[1]);
        if let (Some(u), Some(v)) = (slot_head_alpha(a), slot_head_alpha(b)) {
            if (u - v).abs() > FRAME_FADE + 1e-4 {
                return Err(format!("+{ms:.1} ms (t {}, alpha {}): the slot head from {u} to {v}", beat.t, beat.alpha));
            }
        }
    }
    Ok(())
}

/// The shots of a real wind-down cut `cut` seconds into `story`, one per
/// 60 Hz frame from `phase` ms after the cut until past its end, the first
/// at the cut itself when `phase` is 0. `shot` turns a beat into the shot.
#[cfg(test)]
pub(crate) fn wind_down_frames(story: Story, cut: f32, phase: f32, shot: impl Fn(Beat) -> Shot) -> Vec<(f32, Beat, Shot)> {
    use std::time::{Duration, Instant};
    let t0 = Instant::now();
    let mut play = crate::ui::play::state::Play::start(story, t0);
    let at = t0 + Duration::from_secs_f32(cut);
    play.end(at);
    let mut out = Vec::new();
    let mut ms = phase;
    while ms <= 230.0 {
        let beat = play.beat(at + Duration::from_secs_f32(ms / 1000.0));
        out.push((ms, beat, shot(beat)));
        ms += 1000.0 / 60.0;
    }
    out
}

/// Every part's alpha, the halo and the slot head's alpha change by at most
/// `FRAME_FADE` from one frame of `shots` to the next. Parts are matched by
/// layer and kind (a head, a figure, a prop, a digit by its index, a
/// whistle), in order; one gone counts as 0, so nothing may vanish or appear
/// at more than a frame's fade.
#[cfg(test)]
pub(crate) fn fades_by_frames(shots: &[(f32, Beat, Shot)]) -> Result<(), String> {
    fn kind(d: &Draw) -> usize {
        match d {
            Draw::Head { .. } => 0,
            Draw::Group { items, .. } if items.iter().any(|i| matches!(i, Draw::Head { .. })) => 1,
            Draw::Group { .. } => 2,
            Draw::Whistle { .. } => 3,
            Draw::Digit { index, .. } => 10 + index,
            Draw::ClockChar { index, .. } => 20 + index,
            _ => 4,
        }
    }
    let alphas = |shot: &Shot| {
        let mut out: Vec<((usize, usize), Vec<f32>)> = Vec::new();
        for p in &shot.parts {
            let pass = PAINT_ORDER.iter().position(|q| *q == p.layer.pass()).unwrap_or(0);
            let key = (pass, kind(&p.draw));
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push(p.alpha),
                None => out.push((key, vec![p.alpha])),
            }
        }
        out
    };
    for w in shots.windows(2) {
        let ((_, _, a), (ms, beat, b)) = (&w[0], &w[1]);
        let case = format!("+{ms:.1} ms (t {}, alpha {})", beat.t, beat.alpha);
        let (pa, pb) = (alphas(a), alphas(b));
        let none = Vec::new();
        for (key, _) in pa.iter().chain(&pb) {
            let get = |p: &Vec<((usize, usize), Vec<f32>)>| p.iter().find(|(k, _)| k == key).map_or(&none, |(_, v)| v).clone();
            let (x, y) = (get(&pa), get(&pb));
            for k in 0..x.len().max(y.len()) {
                let (u, v) = (x.get(k).copied().unwrap_or(0.0), y.get(k).copied().unwrap_or(0.0));
                if (u - v).abs() > FRAME_FADE + 1e-4 {
                    return Err(format!("{case}: part {k} of kind {key:?} from {u} to {v}"));
                }
            }
        }
        if (a.halo - b.halo).abs() > FRAME_FADE + 1e-4 {
            return Err(format!("{case}: the halo from {} to {}", a.halo, b.halo));
        }
        if let (Slot::Story(m), Slot::Story(n)) = (a.slot, b.slot) {
            if (m.alpha - n.alpha).abs() > FRAME_FADE + 1e-4 {
                return Err(format!("{case}: the slot head from {} to {}", m.alpha, n.alpha));
            }
        }
    }
    Ok(())
}

/// Every head the figures of `shot` draw: its pivot in window dip (through
/// the groups it is in) and its part's alpha.
#[cfg(test)]
fn figure_heads(shot: &Shot) -> Vec<((f32, f32), f32)> {
    fn walk(d: &Draw, m: Matrix3x2, alpha: f32, out: &mut Vec<((f32, f32), f32)>) {
        match d {
            Draw::Head { at, .. } => out.push(((at.0 * m.M11 + at.1 * m.M21 + m.M31, at.0 * m.M12 + at.1 * m.M22 + m.M32), alpha)),
            Draw::Group { at, rot_deg, scale, pivot, items } => {
                let inner = group_matrix(*at, *rot_deg, *scale, *pivot) * m;
                items.iter().for_each(|i| walk(i, inner, alpha, out));
            }
            Draw::Clipped { items, .. } => items.iter().for_each(|i| walk(i, m, alpha, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for p in &shot.parts {
        walk(&p.draw, Matrix3x2::identity(), p.alpha, &mut out);
    }
    out
}

/// One mascot at a time (P26): a slot head that shows and a figure's head
/// that shows somewhere else never add up past one head, so a wind-down
/// crossfades the two as one. Two heads in the same place (within 0.5 dip)
/// are a hand-over, where one covers the other.
#[cfg(test)]
pub(crate) fn one_mascot_at_a_time(st: &Stage, shot: &Shot) -> Result<(), String> {
    let r = st.room().robot.ok_or("no slot")?;
    let k = r.w / GRID;
    let (slot, alpha) = match shot.slot {
        Slot::App => ((r.x + HEAD_PIVOT.0 * k, r.y + HEAD_PIVOT.1 * k), 1.0),
        Slot::Story(m) => ((r.x + HEAD_PIVOT.0 * k + m.dx, r.y + HEAD_PIVOT.1 * k + m.dy), m.alpha),
        Slot::Empty => return Ok(()),
    };
    for (at, shown) in figure_heads(shot) {
        let apart = (at.0 - slot.0).hypot(at.1 - slot.1) > 0.5;
        if apart && alpha > 0.0 && shown > 0.0 && alpha + shown > 1.0 + 1e-4 {
            return Err(format!("the slot head ({alpha}) at {slot:?} and a figure's ({shown}) at {at:?}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::robot::Seg;

    fn near(a: Rect, b: Rect) -> bool {
        [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)].iter().all(|(p, q)| (p - q).abs() < 1e-3)
    }

    #[test]
    fn kept_paces_a_fade_by_the_wind_downs_time() {
        // Stepped through a real wind-down in 60 Hz frames: whole at the
        // cut, gone at its end, never more than a frame's fade at a time.
        let frames = wind_down_frames(Story::MalingAngka, 3.0, 0.0, |_| Shot::rest(Style::Pill, crate::ui::play::testing::shape(Style::Pill, 190.0, 34.0)));
        let k: Vec<f32> = frames.iter().map(|(_, b, _)| kept(b.alpha, 0.2, 0.8)).collect();
        assert_eq!((k[0], *k.last().unwrap()), (1.0, 0.0));
        for w in k.windows(2) {
            assert!(w[1] <= w[0] && w[0] - w[1] <= FRAME_FADE, "{k:?}");
        }
        assert_eq!(kept(1.0, 0.2, 0.8), 1.0, "while it plays");
    }

    #[test]
    fn every_story_is_ready() {
        // All ten have a timeline: each is in the pool and moves something
        // halfway through, its own mascot with the clock on.
        for story in crate::ui::play::trigger::ALL {
            assert!(ready(story), "{story:?}");
            let look = CatLook { mascot: story.mascot(), ..Default::default() };
            let s = crate::ui::play::testing::shape(Style::Notch, 190.0, 34.0);
            let beat = Beat { spread: 1.0, alpha: 1.0, t: story.secs() / 2.0, cut_at: None };
            let mid = shot(story, &Stage::idle(Style::Notch, s, beat, look, true));
            assert!(at_rest(Style::Notch, &mid).is_err(), "{story:?} plays nothing halfway");
        }
    }

    #[test]
    fn two_heads_in_two_places_never_add_up_past_one() {
        let s = crate::ui::play::testing::shape(Style::Notch, 190.0, 34.0);
        let look = CatLook { mascot: Mascot::Cat, ..Default::default() };
        let st = Stage::idle(Style::Notch, s, Beat { spread: 0.0, alpha: 1.0, t: 1.0, cut_at: None }, look, true);
        let r = st.room().robot.unwrap();
        let k = r.w / GRID;
        let pivot = (r.x + HEAD_PIVOT.0 * k, r.y + HEAD_PIVOT.1 * k);
        // A figure carrying a head at `at` (inside a moved group), at `alpha`,
        // beside a slot head at `slot`.
        let with = |slot: f32, at: (f32, f32), alpha: f32| {
            let head = Draw::Head { at: (at.0 - 10.0, at.1 + 5.0), shot: MascotShot::REST };
            let draw = Draw::Group { at: (10.0, -5.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items: vec![head] };
            let mut shot = Shot { slot: Slot::Story(MascotShot { alpha: slot, ..MascotShot::REST }), ..Shot::rest(Style::Notch, s) };
            shot.parts.push(Part { layer: Layer::Inside, draw, alpha });
            shot
        };
        let away = (pivot.0 + 40.0, pivot.1 + 10.0);
        assert!(one_mascot_at_a_time(&st, &with(1.0, away, 0.5)).is_err());
        assert!(one_mascot_at_a_time(&st, &with(0.5, away, 0.5)).is_ok());
        assert!(one_mascot_at_a_time(&st, &with(0.0, away, 1.0)).is_ok());
        // In the slot head's own place it is a hand-over: one covers the other.
        assert!(one_mascot_at_a_time(&st, &with(1.0, pivot, 0.7)).is_ok());
        // The app's head counts as whole; an empty slot has none.
        let app = Shot { slot: Slot::App, ..with(1.0, away, 0.3) };
        assert!(one_mascot_at_a_time(&st, &app).is_err());
        assert!(one_mascot_at_a_time(&st, &Shot { slot: Slot::Empty, ..app }).is_ok());
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
    fn a_clipped_part_is_bounded_by_its_items_within_its_clip() {
        let clip = circle_segs((10.0, 10.0), 5.0);
        let dot = |x: f32| Draw::Ellipse { c: (x, 10.0), r: (2.0, 2.0), rgb: 0 };
        // Two dots, the right one half out of the clip: the box is the
        // clip's where the items reach past it.
        let cut = Draw::Clipped { clip, items: vec![dot(8.0), dot(15.0)] };
        assert!(near(cut.bounds(), Rect { x: 6.0, y: 8.0, w: 9.0, h: 4.0 }), "{:?}", cut.bounds());
        // With nothing in it, its box is the clip's own: in a group it never
        // pulls the union to the grid's origin.
        let empty = Draw::Clipped { clip: circle_segs((10.0, 10.0), 5.0), items: Vec::new() };
        assert!(near(empty.bounds(), Rect { x: 5.0, y: 5.0, w: 10.0, h: 10.0 }), "{:?}", empty.bounds());
        let group = Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items: vec![empty, dot(8.0)] };
        assert!(near(group.bounds(), Rect { x: 5.0, y: 5.0, w: 10.0, h: 10.0 }), "{:?}", group.bounds());
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
    fn a_resting_shot_leaves_the_slot_to_the_app() {
        for style in [Style::Notch, Style::Pill] {
            let s = crate::ui::play::testing::shape(style, 190.0, 34.0);
            let rest = Shot::rest(style, s);
            assert_eq!(rest.slot, Slot::App);
            assert!(at_rest(style, &rest).is_ok());
            // A story head at rest in its slot is rest too; one moved, or an
            // empty slot, is not.
            assert!(at_rest(style, &Shot { slot: Slot::Story(MascotShot::REST), ..rest.clone() }).is_ok());
            let moved = MascotShot { dy: 2.0, ..MascotShot::REST };
            assert!(at_rest(style, &Shot { slot: Slot::Story(moved), ..rest.clone() }).is_err());
            assert!(at_rest(style, &Shot { slot: Slot::Empty, ..rest.clone() }).is_err());
            assert!(near_rest(&Shot { slot: Slot::Story(MascotShot { dy: 5e-4, ..MascotShot::REST }), ..rest.clone() }, 1e-3).is_ok());
            assert!(near_rest(&Shot { slot: Slot::Story(moved), ..rest }, 1e-3).is_err());
        }
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
    fn a_shot_drops_the_octopus_arms_only_when_it_asks() {
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
            bare_octopus: false,
            flush: 0.0,
            alpha: 1.0,
        };
        // The app's own pose has the arms, and a shot leaves them there.
        assert!(!Pose::default().bare_octopus);
        assert!(!shot.pose(Pose::default()).bare_octopus);
        assert!(MascotShot { bare_octopus: true, ..shot }.pose(Pose::default()).bare_octopus);
        // Nothing else of the app's pose is touched by it.
        let base = Pose { flush: 0.5, lift_px: 2.0, ..Pose::default() };
        let bare = MascotShot { bare_octopus: true, ..shot }.pose(base);
        assert_eq!(Pose { bare_octopus: false, ..bare }, shot.pose(base));
    }

    #[test]
    fn a_shots_flush_reaches_the_pose_and_never_lowers_the_apps() {
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
            bare_octopus: false,
            flush: 0.0,
            alpha: 1.0,
        };
        assert_eq!(shot.pose(Pose::default()).flush, 0.0);
        assert_eq!(MascotShot { flush: 0.6, ..shot }.pose(Pose::default()).flush, 0.6);
        // The app's own flush (a reminder alert) stays under a story's.
        let alert = Pose { flush: 0.7, ..Pose::default() };
        assert_eq!(shot.pose(alert).flush, 0.7);
        assert_eq!(MascotShot { flush: 0.2, ..shot }.pose(alert).flush, 0.7);
        assert_eq!(MascotShot { flush: 1.0, ..shot }.pose(alert).flush, 1.0);
    }

    #[test]
    fn a_bare_head_belongs_to_an_octopus_story() {
        let face = MascotShot {
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
            bare_octopus: false,
            flush: 0.0,
            alpha: 1.0,
        };
        let s = crate::ui::play::testing::shape(Style::Notch, 190.0, 34.0);
        // A bare head deep in a figure, or the slot head bare.
        let nested = |bare: bool| {
            let head = Draw::Head { at: (0.0, 0.0), shot: MascotShot { bare_octopus: bare, ..face } };
            let clipped = Draw::Clipped { clip: circle_segs((0.0, 0.0), 5.0), items: vec![head] };
            let mut shot = Shot::rest(Style::Notch, s);
            shot.parts.push(Part {
                layer: Layer::Back,
                draw: Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items: vec![clipped] },
                alpha: 1.0,
            });
            shot
        };
        let slot = |bare: bool| Shot { slot: Slot::Story(MascotShot { bare_octopus: bare, ..face }), ..Shot::rest(Style::Notch, s) };
        // Only the octopus's painter drops the arms; any other mascot paints
        // its whole head, which the bare box would not hold.
        for shot in [nested(true), slot(true)] {
            assert!(bare_heads_fit(Story::Bungee, &shot));
            assert!(!bare_heads_fit(Story::Kelelawar, &shot));
            assert!(!bare_heads_fit(Story::PianoEkor, &shot));
        }
        for shot in [nested(false), slot(false), Shot::rest(Style::Notch, s)] {
            for story in [Story::Bungee, Story::Kelelawar, Story::PianoEkor] {
                assert!(bare_heads_fit(story, &shot), "{story:?}");
            }
        }
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
            bare_octopus: false,
            flush: 0.0,
            alpha: 1.0,
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
