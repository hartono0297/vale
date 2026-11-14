//! The seated cat's body, legs, paws and tail, ported from
//! `docs/design/maskot-notch-v3.html` (`catSlim`, `smallPaw`, `slimTail`,
//! `catAt`). Pure: a pose and a coat in, `Draw`s out on the head's 64-unit
//! grid, the seat at `SEAT`. The head is `Draw::Head`, the app's own
//! painter, so its coat and accessory come along.

use crate::theme::{CatColor, Mascot};
use crate::ui::cat_style::{CatLook, PATCH_DARK, PATCH_ORANGE, RIM_WIDTH};
use crate::ui::geometry::ROBOT_COMPACT;
use crate::ui::mascot::mix;
use crate::ui::play::art::tube::{LimbStyle, Stripes, along, catmull_rom, curl, limb, plen};
use crate::ui::play::stories::{Draw, HEAD_PIVOT, MascotShot, circle_segs};
use crate::ui::robot::{GRID, Seg};

/// The whole seated cat a touch smaller, so the ears clear the screen's top.
pub const CAT_SC: f32 = 0.94;
/// Where the cat sits, on its grid: a story places the figure by this point.
pub const SEAT: (f32, f32) = (32.0, 86.0);

/// The mockup's island fill, which `dk` mixes the coat toward (its `tint`).
const NOTCH: u32 = 0x17120F;
/// The mockup's `CS` table, one coat (terracotta): the fur (the head's
/// colour: legs, paws, tail), and the darker tones each coat takes in the
/// same proportion to its own head colour.
const FUR: u32 = 0xE2683C;
const BODY: u32 = 0xDB633A;
const HAUNCH: u32 = 0xCB582F;
const SIDE: u32 = 0xC4532D;
const SHADE: u32 = 0xB04526;
/// The toe beans, pink on every coat.
const BEAN: u32 = 0xEE9C94;
/// The neck the head turns about.
const NECK: (f32, f32) = (32.0, 52.0);
/// The front legs' tops, left then right; each ends in its paw 26 lower.
const LEG_X: [f32; 2] = [28.6, 35.4];

const SL_BODY: &str = "M24.5 51C21.5 59.5 17.5 67.5 16 75.5C15 81.5 17 86 21.5 86H42.5C47 86 49 81.5 48 75.5C46.5 67.5 42.5 59.5 39.5 51Z";
const SL_SIDE: &str = "M39.5 51C42.5 59.5 46.5 67.5 48 75.5C49 81.5 47 86 42.5 86H41C44.6 83 45.2 77 44 71C42.8 64.5 41.4 57.6 39.5 51Z";
const SL_HL: &str = "M15.6 80.5C15 73.5 18 69.6 21.8 70.4C25.4 71.2 26.6 75.5 25.6 86H20C17 86 15.8 83.8 15.6 80.5Z";
const SL_HR: &str = "M48.4 80.5C49 73.5 46 69.6 42.2 70.4C38.6 71.2 37.4 75.5 38.4 86H44C47 86 48.2 83.8 48.4 80.5Z";

/// Calico's two patches on the body, as on its head: orange up on the left
/// flank, dark low on the right haunch. (centre, radius, colour)
const PATCHES: [((f32, f32), f32, u32); 2] =
    [((20.0, 66.0), 9.0, PATCH_ORANGE), ((46.0, 82.0), 9.0, PATCH_DARK)];

/// A paw at the end of a front leg: moved by `dx`, `dy` from its rest
/// (grid units), `lift` above 0.45 shows its toe beans, `sq` squashes it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Paw {
    pub dx: f32,
    pub dy: f32,
    pub lift: f32,
    pub sq: f32,
}

/// The long S-shaped tail: `wag` sways its top, `curl` turns its tip
/// (radians, the mockup's -2.2 at rest), `puff` widens it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tail {
    pub wag: f32,
    pub curl: f32,
    pub puff: f32,
}

impl Default for Tail {
    fn default() -> Self {
        Tail { wag: 0.0, curl: -2.2, puff: 1.0 }
    }
}

/// The mockup's `catSlim` options. `dk` is how deep in the island's dark
/// the body is (0 out, 1 deep; the tail drops from 0.6), `rise` how tall it
/// sits (0 crouched, 1 tall; `None` is the mockup's `1 - dk`), `head` the head's (hx, hy, hr): moved on the
/// grid and turned (degrees, clockwise) about the neck. `face` is the head
/// as the slot head's shot has it; its own turn and scale add to `hr`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CatPose {
    pub dk: f32,
    pub rise: Option<f32>,
    pub head: (f32, f32, f32),
    pub face: MascotShot,
    pub paws: [Paw; 2],
    pub tail: Option<Tail>,
}

/// The seated cat on its 64-unit grid: a `Group` that leaves the grid as it
/// is (its pivot and `at` both `SEAT`).
pub fn cat(pose: &CatPose, look: CatLook) -> Draw {
    Draw::Group { at: SEAT, rot_deg: 0.0, scale: (1.0, 1.0), pivot: SEAT, items: figure(pose, look) }
}

/// The mockup's `catAt`: the seated cat with its seat at `at` (window dip),
/// turned by `rot_deg` and scaled by `sx`, `sy` (times `CAT_SC`, at the slot
/// head's size), in one `Group`.
pub fn cat_at(at: (f32, f32), rot_deg: f32, sx: f32, sy: f32, pose: &CatPose, look: CatLook) -> Draw {
    let unit = CAT_SC * ROBOT_COMPACT / GRID;
    Draw::Group { at, rot_deg, scale: (sx * unit, sy * unit), pivot: SEAT, items: figure(pose, look) }
}

/// The coat's colours, `dk` deep in the dark (the mockup's `CS` through
/// its `tint`).
#[derive(Clone, Copy)]
struct Coat {
    fur: u32,
    body: u32,
    haunch: u32,
    side: u32,
    shade: u32,
    bean: u32,
}

impl Coat {
    fn of(head: u32, dk: f32) -> Coat {
        let t = |rgb| tint(rgb, dk);
        Coat {
            fur: t(head),
            body: t(tone(head, BODY)),
            haunch: t(tone(head, HAUNCH)),
            side: t(tone(head, SIDE)),
            shade: t(tone(head, SHADE)),
            bean: t(BEAN),
        }
    }
}

/// `head` darkened channel by channel as the terracotta `FUR` darkens to
/// `cs`: the terracotta's own tones come back exactly.
fn tone(head: u32, cs: u32) -> u32 {
    let ch = |sh: u32| {
        let at = |rgb: u32| ((rgb >> sh) & 0xFF) as f32;
        ((at(head) * at(cs) / at(FUR)).round().min(255.0) as u32) << sh
    };
    ch(16) | ch(8) | ch(0)
}

/// The mockup's `tint`: deeper in the island, closer to its fill.
fn tint(rgb: u32, dk: f32) -> u32 {
    if dk > 0.001 { mix(rgb, NOTCH, dk.clamp(0.0, 1.0) * 0.9) } else { rgb }
}

/// The tail's width along it: slimming toward the tip, a small puff at it.
fn tail_width(s: f32) -> f32 {
    6.4 + (5.2 - 6.4) * s + 0.9 * ((s - 0.85) / 0.15).clamp(0.0, 1.0).powf(1.5)
}

/// The mockup's `catSlim`, in its order: the tail behind, the body, its
/// side shade and haunches (Calico's patches over them), the front legs,
/// the chin's shadow, the head, the paws.
fn figure(pose: &CatPose, look: CatLook) -> Vec<Draw> {
    let c = Coat::of(look.head_rgb(), pose.dk);
    // The Black cat's rim darkens in the dark with the body. A stroke
    // `RIM_WIDTH` wide under the fill: 0.8 of it shows past the body.
    let rim = look.rim().map(|rgb| tint(rgb, pose.dk));
    let mut out = Vec::new();
    // The long S-shaped tail rises behind the right haunch.
    if let Some(t) = pose.tail.filter(|_| pose.dk < 0.6) {
        let w = t.wag;
        let d = catmull_rom(
            &[(44.0, 82.0), (53.0, 85.0), (60.5, 80.0), (61.0, 70.0), (64.0 + w * 0.08, 61.0), (70.0 + w * 0.12, 56.0)],
            6,
        );
        let p = curl(&along(&d, plen(&d), 26), t.curl, 9.0, 10, (w * 0.6).to_radians());
        let style = LimbStyle {
            wf: Some(tail_width),
            // The mockup's `tl.puff || 1`.
            wm: if t.puff == 0.0 { 1.0 } else { t.puff },
            fluff: 0.05,
            side_shade: Some((1, 0.3)),
            stripes: Some(Stripes { start: 8.0, every: 7.5, w: 3.2, skip_base: 5.0, rgb: c.shade }),
            tip: Some((6.0, c.shade)),
            ..LimbStyle::default()
        };
        let tail = limb(&p, &style, c.fur);
        if let (Some(rgb), Some(Draw::Fill { path, .. })) = (rim, tail.first()) {
            out.push(Draw::Stroke { path: path.clone(), rgb, width: RIM_WIDTH });
        }
        out.extend(tail);
    }
    let (body, side, hips) = (svg(SL_BODY), svg(SL_SIDE), [svg(SL_HL), svg(SL_HR)]);
    if let Some(rgb) = rim {
        for path in [&body, &hips[0], &hips[1]] {
            out.push(Draw::Stroke { path: path.clone(), rgb, width: RIM_WIDTH });
        }
    }
    let flank = |c: Coat| {
        vec![
            Draw::Fill { path: body.clone(), rgb: c.body },
            Draw::Fill { path: side.clone(), rgb: c.side },
            Draw::Fill { path: hips[0].clone(), rgb: c.haunch },
            Draw::Fill { path: hips[1].clone(), rgb: c.haunch },
        ]
    };
    out.extend(flank(c));
    // Calico: each patch is the body again in the patch's colour, its own
    // shade and haunches included, cut to its circle.
    if look.mascot == Mascot::Cat && look.color == CatColor::Calico {
        for (at, r, rgb) in PATCHES {
            out.push(Draw::Clipped { clip: circle_segs(at, r), items: flank(Coat::of(rgb, pose.dk)) });
        }
    }
    // Slim front legs that always end in their paw.
    let leg = LimbStyle { w0: 5.8, w1: 5.2, cap: true, side_shade: None, ..LimbStyle::default() };
    for (x0, p) in LEG_X.iter().zip(&pose.paws) {
        out.extend(limb(&leg_line(*x0, p), &leg, c.fur));
    }
    // The chin's shadow on the chest.
    out.push(Draw::Ellipse { c: (32.0, 55.6), r: (8.6, 2.3), rgb: c.side });
    // The head: lowered as the cat crouches, turned about the neck.
    let (hx, hy, hr) = pose.head;
    let (sin, cos) = hr.to_radians().sin_cos();
    let v = (HEAD_PIVOT.0 - NECK.0, HEAD_PIVOT.1 - NECK.1);
    out.push(Draw::Head {
        at: (
            NECK.0 + v.0 * cos - v.1 * sin + hx,
            NECK.1 + v.0 * sin + v.1 * cos + hy + 14.0 * (1.0 - pose.rise.unwrap_or(1.0 - pose.dk)),
        ),
        shot: MascotShot { rot_deg: pose.face.rot_deg + hr, ..pose.face },
    });
    for (x0, p) in LEG_X.iter().zip(&pose.paws) {
        out.push(paw((x0 + p.dx, 84.0 + p.dy), c, p));
    }
    out
}

/// A front leg's centreline: from the chest at `x0` to just above its paw.
fn leg_line(x0: f32, p: &Paw) -> [(f32, f32); 3] {
    let (px, py) = (x0 + p.dx, 84.0 + p.dy - 1.5);
    [(x0, 58.0), ((x0 + px) / 2.0, (58.0 + py) / 2.0), (px, py)]
}

/// The mockup's `smallPaw`: a small neat paw, two toe lines, or its pink
/// beans when lifted.
fn paw(at: (f32, f32), c: Coat, p: &Paw) -> Draw {
    let r = (3.6, 2.4);
    let mut items = vec![
        Draw::Ellipse { c: (0.0, 0.0), r, rgb: c.fur },
        Draw::Stroke { path: ellipse_segs(r), rgb: c.shade, width: 0.7 },
    ];
    if p.lift > 0.45 {
        items.push(Draw::Ellipse { c: (0.0, 0.5), r: (1.6, 1.2), rgb: c.bean });
        for d in [-1.9, 0.0, 1.9] {
            items.push(Draw::Ellipse { c: (d, -1.4), r: (0.7, 0.7), rgb: c.bean });
        }
    } else {
        let toes = vec![Seg::Move(-1.2, 2.3), Seg::Line(-1.2, 1.0), Seg::Move(1.2, 2.3), Seg::Line(1.2, 1.0)];
        items.push(Draw::Stroke { path: toes, rgb: c.shade, width: 0.7 });
    }
    Draw::Group {
        at,
        rot_deg: 0.0,
        scale: (1.0 + 0.2 * p.sq, 1.0 - 0.22 * p.sq),
        pivot: (0.0, 0.0),
        items,
    }
}

/// A closed ellipse about the origin with radii `r`.
fn ellipse_segs(r: (f32, f32)) -> Vec<Seg> {
    circle_segs((0.0, 0.0), 1.0)
        .into_iter()
        .map(|s| match s {
            Seg::Move(x, y) => Seg::Move(x * r.0, y * r.1),
            Seg::Cubic(a, b, c, d, x, y) => Seg::Cubic(a * r.0, b * r.1, c * r.0, d * r.1, x * r.0, y * r.1),
            other => other,
        })
        .collect()
}

/// An SVG path of the subset the mockup's body paths use (absolute `M`,
/// `L`, `H`, `V`, `C`, `Z`) as segments.
fn svg(d: &str) -> Vec<Seg> {
    let mut cmds: Vec<(char, Vec<f32>)> = Vec::new();
    let mut num = String::new();
    let end = |num: &mut String, cmds: &mut Vec<(char, Vec<f32>)>| {
        if let (false, Some(c)) = (num.is_empty(), cmds.last_mut()) {
            c.1.push(num.parse().expect("a number in a mockup path"));
        }
        num.clear();
    };
    for ch in d.chars() {
        match ch {
            'M' | 'L' | 'H' | 'V' | 'C' | 'Z' => {
                end(&mut num, &mut cmds);
                cmds.push((ch, Vec::new()));
            }
            '-' => {
                end(&mut num, &mut cmds);
                num.push(ch);
            }
            '0'..='9' | '.' => num.push(ch),
            _ => end(&mut num, &mut cmds),
        }
    }
    end(&mut num, &mut cmds);
    let mut out = Vec::new();
    let (mut x, mut y) = (0.0, 0.0);
    for (c, a) in cmds {
        match c {
            'M' | 'L' => {
                (x, y) = (a[0], a[1]);
                out.push(if c == 'M' { Seg::Move(x, y) } else { Seg::Line(x, y) });
            }
            'H' | 'V' => {
                if c == 'H' {
                    x = a[0];
                } else {
                    y = a[0];
                }
                out.push(Seg::Line(x, y));
            }
            'C' => {
                for k in a.chunks_exact(6) {
                    out.push(Seg::Cubic(k[0], k[1], k[2], k[3], k[4], k[5]));
                    (x, y) = (k[4], k[5]);
                }
            }
            _ => out.push(Seg::Close),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::cat_style::RIM;
    use crate::ui::mascot::CrestPose;
    use crate::ui::robot::Expr;

    fn look(color: CatColor) -> CatLook {
        CatLook { mascot: Mascot::Cat, color, ..CatLook::default() }
    }

    fn face() -> MascotShot {
        MascotShot {
            dx: 0.0,
            dy: 0.0,
            rot_deg: 0.0,
            sx: 1.0,
            sy: 1.0,
            expr: Expr::Idle,
            crest: CrestPose { rot_deg: 0.0, scale: 1.0 },
            glance: (0.0, 0.0),
            blink: false,
            beak_open: false,
            bare_octopus: false,
            flush: 0.0,
        }
    }

    fn sitting() -> CatPose {
        CatPose {
            dk: 0.0,
            rise: Some(1.0),
            head: (0.0, 0.0, 0.0),
            face: face(),
            paws: [Paw::default(); 2],
            tail: Some(Tail::default()),
        }
    }

    fn items(d: &Draw) -> &[Draw] {
        match d {
            Draw::Group { items, .. } => items,
            other => panic!("not a group: {other:?}"),
        }
    }

    /// Every drawable, groups and clips opened, in paint order.
    fn flat(d: &[Draw]) -> Vec<&Draw> {
        let mut out = Vec::new();
        for x in d {
            out.push(x);
            match x {
                Draw::Group { items, .. } | Draw::Clipped { items, .. } => out.extend(flat(items)),
                _ => {}
            }
        }
        out
    }

    /// The fill of the body's own path, outside any patch.
    fn body_fill(d: &[Draw]) -> Option<(usize, u32)> {
        let body = svg(SL_BODY);
        d.iter().enumerate().find_map(|(i, x)| match x {
            Draw::Fill { path, rgb } if *path == body => Some((i, *rgb)),
            _ => None,
        })
    }

    #[test]
    fn svg_reads_the_mockups_subset() {
        let d = svg("M1 2C3 4 5 6 7 8H9V10L11 12Z");
        assert_eq!(
            d,
            vec![
                Seg::Move(1.0, 2.0),
                Seg::Cubic(3.0, 4.0, 5.0, 6.0, 7.0, 8.0),
                Seg::Line(9.0, 8.0),
                Seg::Line(9.0, 10.0),
                Seg::Line(11.0, 12.0),
                Seg::Close,
            ]
        );
        // The body: one move, four cubics, a line and the close.
        let body = svg(SL_BODY);
        assert_eq!(body.len(), 7, "{body:?}");
        assert_eq!(body[0], Seg::Move(24.5, 51.0));
        assert_eq!(body[3], Seg::Line(42.5, 86.0));
    }

    /// The first leg's fill: the first `Fill` after the haunches (and
    /// Calico's patches).
    fn leg_fill(d: &[Draw]) -> Option<u32> {
        let (body, _) = body_fill(d)?;
        d[body + 4..].iter().find_map(|x| match x {
            Draw::Fill { rgb, .. } => Some(*rgb),
            _ => None,
        })
    }

    #[test]
    fn the_body_takes_every_coats_colours() {
        // The mockup's body is a shade under its fur; the fur (legs, paws,
        // tail) is the head's colour, so the legs read on the body.
        let mut seen = Vec::new();
        for color in [CatColor::Terracotta, CatColor::Calico, CatColor::White, CatColor::Black] {
            let l = look(color);
            let fig = cat(&sitting(), l);
            let (_, body) = body_fill(items(&fig)).unwrap_or_else(|| panic!("{color:?}: no body"));
            let leg = leg_fill(items(&fig)).unwrap_or_else(|| panic!("{color:?}: no leg"));
            assert_eq!(leg, l.head_rgb(), "{color:?}: the legs");
            assert_ne!(body, leg, "{color:?}: the body is the legs' colour");
            seen.push(body);
        }
        // Terracotta is the mockup's table; White and Black darken alike.
        assert_eq!(seen, vec![0xDB633A, 0xECE2DC, 0xEEE5E2, 0x1D1A21], "{seen:x?}");
        // The terracotta's darker tones are the mockup's own.
        let fig = cat(&sitting(), look(CatColor::Terracotta));
        let fills: Vec<u32> = flat(items(&fig))
            .into_iter()
            .filter_map(|d| match d {
                Draw::Fill { rgb, .. } | Draw::Ellipse { rgb, .. } => Some(*rgb),
                _ => None,
            })
            .collect();
        for tone in [0xCB582F, 0xC4532D, 0xB04526] {
            assert!(fills.contains(&tone), "{tone:x} missing from {fills:x?}");
        }
    }

    #[test]
    fn calico_has_two_patches_on_the_body() {
        let patches = |color| {
            let fig = cat(&sitting(), look(color));
            items(&fig)
                .iter()
                .filter_map(|d| match d {
                    // A patch: the body repainted in the patch's colour, cut to a circle.
                    Draw::Clipped { items, .. } => body_fill(items).map(|(_, rgb)| rgb),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // Each patch's body is a shade under its colour, as the coat's is.
        assert_eq!(patches(CatColor::Calico), vec![0xDB633A, 0x2A221F]);
        for color in [CatColor::Terracotta, CatColor::White, CatColor::Black] {
            assert!(patches(color).is_empty(), "{color:?}");
        }
        // Over the cream body, under the legs.
        let fig = cat(&sitting(), look(CatColor::Calico));
        let d = items(&fig);
        let (body, _) = body_fill(d).expect("a body");
        let first = d.iter().position(|x| matches!(x, Draw::Clipped { .. })).expect("a patch");
        let leg = d.iter().rposition(|x| matches!(x, Draw::Clipped { .. })).unwrap() + 1;
        assert!(first > body, "patch at {first}, body at {body}");
        assert!(matches!(d[leg], Draw::Fill { rgb, .. } if rgb == look(CatColor::Calico).head_rgb()), "{:?}", d[leg]);
    }

    #[test]
    fn black_has_a_rim_round_the_body() {
        let rim = |color| {
            let fig = cat(&sitting(), look(color));
            let d = items(&fig);
            let body = svg(SL_BODY);
            let at = d.iter().position(|x| matches!(x, Draw::Stroke { path, rgb, width } if *path == body && *rgb == RIM && *width == RIM_WIDTH));
            at.map(|i| (i, body_fill(d).expect("a body").0))
        };
        let (stroke, body) = rim(CatColor::Black).expect("the Black cat's rim");
        assert!(stroke < body, "the rim goes under the body: {stroke} vs {body}");
        for color in [CatColor::Terracotta, CatColor::Calico, CatColor::White] {
            assert!(rim(color).is_none(), "{color:?}");
        }
    }

    #[test]
    fn the_rim_darkens_with_the_body_in_the_dark() {
        let fig = cat(&CatPose { dk: 0.5, ..sitting() }, look(CatColor::Black));
        let rims: Vec<u32> = items(&fig)
            .iter()
            .filter_map(|d| match d {
                Draw::Stroke { rgb, width, .. } if *width == RIM_WIDTH => Some(*rgb),
                _ => None,
            })
            .collect();
        // The tail's rim, the body's and the two haunches'.
        assert_eq!(rims, vec![crate::ui::mascot::mix(RIM, 0x17120F, 0.45); 4]);
    }

    #[test]
    fn the_legs_end_in_their_paws() {
        let p = Paw { dx: 2.0, dy: -3.0, lift: 0.0, sq: 0.5 };
        // From the chest to just above the paw, through the middle.
        assert_eq!(leg_line(35.4, &p), [(35.4, 58.0), (36.4, 68.75), (37.4, 79.5)]);
        let fig = cat(&CatPose { paws: [Paw::default(), p], ..sitting() }, look(CatColor::Terracotta));
        let paws: Vec<_> = items(&fig)
            .iter()
            .filter_map(|d| match d {
                Draw::Group { at, scale, rot_deg, pivot, .. } => Some((*at, *scale, *rot_deg, *pivot)),
                _ => None,
            })
            .collect();
        let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5;
        assert_eq!(paws.len(), 2);
        assert!(near(paws[0].0, (28.6, 84.0)) && near(paws[0].1, (1.0, 1.0)), "{:?}", paws[0]);
        assert!(near(paws[1].0, (37.4, 81.0)) && near(paws[1].1, (1.1, 0.89)), "{:?}", paws[1]);
        assert!(paws.iter().all(|p| p.2 == 0.0 && p.3 == (0.0, 0.0)), "{paws:?}");
    }

    #[test]
    fn the_figure_paints_in_the_mockups_order() {
        let order = |color| {
            let fig = cat(&sitting(), look(color));
            let (body, side, hl, hr) = (svg(SL_BODY), svg(SL_SIDE), svg(SL_HL), svg(SL_HR));
            let mut seen_body = false;
            let mut out: Vec<&str> = Vec::new();
            for d in items(&fig) {
                let label = match d {
                    Draw::Stroke { rgb, .. } if *rgb == RIM => "rim",
                    Draw::Fill { path, .. } if *path == body => {
                        seen_body = true;
                        "body"
                    }
                    Draw::Fill { path, .. } if *path == side => "side",
                    Draw::Fill { path, .. } if *path == hl || *path == hr => "haunch",
                    Draw::Clipped { .. } => "patch",
                    Draw::Ellipse { c, .. } if *c == (32.0, 55.6) => "chin",
                    Draw::Head { .. } => "head",
                    Draw::Group { .. } => "paw",
                    Draw::Fill { .. } | Draw::Tint { .. } if seen_body => "leg",
                    Draw::Fill { .. } | Draw::Tint { .. } => "tail",
                    other => panic!("{other:?}"),
                };
                if out.last() != Some(&label) {
                    out.push(label);
                }
            }
            out
        };
        assert_eq!(order(CatColor::Terracotta), ["tail", "body", "side", "haunch", "leg", "chin", "head", "paw"]);
        assert_eq!(order(CatColor::Calico), ["tail", "body", "side", "haunch", "patch", "leg", "chin", "head", "paw"]);
        assert_eq!(order(CatColor::Black), ["rim", "tail", "rim", "body", "side", "haunch", "leg", "chin", "head", "paw"]);
    }

    #[test]
    fn a_tail_of_no_puff_is_a_plain_tail() {
        // The mockup's `tl.puff || 1`.
        let with = |puff: f32| cat(&CatPose { tail: Some(Tail { puff, ..Tail::default() }), ..sitting() }, look(CatColor::Terracotta));
        assert_eq!(with(0.0), with(1.0));
        assert_ne!(with(1.4), with(1.0));
    }

    #[test]
    fn a_lifted_paw_shows_its_beans() {
        let beans = |lift: f32| {
            let mut pose = sitting();
            pose.paws[1] = Paw { dx: 1.5, dy: -6.0, lift, sq: 0.0 };
            let fig = cat(&pose, look(CatColor::Terracotta));
            flat(items(&fig)).into_iter().filter(|d| matches!(d, Draw::Ellipse { rgb, .. } if *rgb == BEAN)).count()
        };
        assert_eq!(beans(0.5), 4);
        assert_eq!(beans(0.4), 0);
    }

    #[test]
    fn the_head_sits_on_the_neck_whatever_the_rise() {
        let head_at = |dk: f32, rise: Option<f32>, h: (f32, f32, f32)| {
            let fig = cat(&CatPose { dk, rise, head: h, ..sitting() }, look(CatColor::Terracotta));
            items(&fig)
                .iter()
                .find_map(|d| match d {
                    Draw::Head { at, shot } => Some((*at, shot.rot_deg)),
                    _ => None,
                })
                .expect("a head")
        };
        let head = |rise: f32, h| head_at(0.0, Some(rise), h);
        let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
        // The head's pivot (32, 40) on the grid, lowered 14 * (1 - rise).
        for (rise, y) in [(1.0, 40.0), (0.5, 47.0), (0.0, 54.0)] {
            let (at, rot) = head(rise, (0.0, 0.0, 0.0));
            assert!(near(at, (32.0, y)), "rise {rise}: {at:?}");
            assert_eq!(rot, 0.0);
        }
        // Moved by (hx, hy) and turned about the neck (32, 52), not its pivot.
        let (at, rot) = head(1.0, (3.0, -2.0, 90.0));
        assert!(near(at, (32.0 + 12.0 + 3.0, 52.0 - 2.0)), "{at:?}");
        assert_eq!(rot, 90.0);
        // No rise given: the mockup's `1 - dk`, whatever the rise of a deep body.
        for (dk, y) in [(0.0, 40.0), (0.5, 47.0), (1.0, 54.0)] {
            let (at, _) = head_at(dk, None, (0.0, 0.0, 0.0));
            assert!(near(at, (32.0, y)), "dk {dk}: {at:?}");
        }
        let (at, _) = head_at(1.0, Some(1.0), (0.0, 0.0, 0.0));
        assert!(near(at, (32.0, 40.0)), "a given rise wins: {at:?}");
    }

    #[test]
    fn the_tail_is_dropped_when_crouched() {
        // Only the tail has a shade band (the legs have none).
        let tail = |dk: f32, tail: Option<Tail>| {
            let fig = cat(&CatPose { dk, tail, ..sitting() }, look(CatColor::Terracotta));
            items(&fig).iter().any(|d| matches!(d, Draw::Tint { .. }))
        };
        assert!(tail(0.0, Some(Tail::default())));
        assert!(tail(0.59, Some(Tail::default())));
        assert!(!tail(0.6, Some(Tail::default())));
        assert!(!tail(0.0, None));
    }

    #[test]
    fn deep_in_the_dark_the_body_takes_the_islands_colour() {
        // The mockup's `tint`: 0.9 of the way to the notch at dk 1.
        let fig = cat(&CatPose { dk: 1.0, ..sitting() }, look(CatColor::White));
        let (_, rgb) = body_fill(items(&fig)).expect("a body");
        assert_eq!(rgb, crate::ui::mascot::mix(0xEEE5E2, 0x17120F, 0.9));
        assert_eq!(leg_fill(items(&fig)), Some(crate::ui::mascot::mix(0xF6F1EA, 0x17120F, 0.9)));
    }

    #[test]
    fn cat_at_places_the_seat_in_one_group() {
        let fig = cat_at((300.0, 60.0), 12.0, 1.0, 0.9, &sitting(), look(CatColor::Calico));
        let Draw::Group { at, rot_deg, scale, pivot, items: placed } = &fig else {
            panic!("{fig:?}");
        };
        let unit = ROBOT_COMPACT / GRID;
        assert_eq!((*at, *rot_deg, *pivot), ((300.0, 60.0), 12.0, SEAT));
        assert!((scale.0 - CAT_SC * unit).abs() < 1e-6 && (scale.1 - 0.9 * CAT_SC * unit).abs() < 1e-6, "{scale:?}");
        assert_eq!(placed.as_slice(), items(&cat(&sitting(), look(CatColor::Calico))));
        // On its own grid the figure stays where it is drawn.
        let Draw::Group { at, rot_deg, scale, pivot, .. } = cat(&sitting(), look(CatColor::Calico)) else {
            unreachable!()
        };
        assert_eq!((at, rot_deg, scale, pivot), (SEAT, 0.0, (1.0, 1.0), SEAT));
    }
}
