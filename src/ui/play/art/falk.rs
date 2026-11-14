//! The falk's wings and body, ported from `docs/design/maskot-notch-v3.html`
//! (`FK`, `WC`, `plume`, `wing`, `WING`, `wmix`, `bird`, `birdG`, and the
//! wing poses stories `f1` and `f3` name). Pure: a pose and a coat in,
//! `Draw`s out, in the mockup's bird-local dip (origin between the feet, up
//! is -y). The head is `Draw::Head`, the app's own falk painter, so its
//! crest pose, coat and accessory come along; the mockup's `falkHead` only
//! shows where it sits.
//!
//! Units: the body keeps the mockup's numbers, at its 0.4 dip per grid
//! unit, so the port stays verbatim; the head is the app's at the slot's
//! 26 / 64 (`ROBOT_COMPACT / GRID`), so it matches the slot head exactly
//! and is 1.6 % larger against the body than in the mockup.
//!
//! The colours are the head's: the body, and the wing's bone, are
//! `falk_ink(coat).body`. Lutino is the mockup's own table (its `FK` and
//! `WC`). Every other coat keeps its body's hue: each tone is the body moved
//! toward black or white as far, in lightness, as the Lutino tone moves from
//! the Lutino body (`tone`). Grey and WhiteFace share the grey body, so they
//! share one wing:
//!
//! | tone  | Lutino (mockup) | Grey, WhiteFace |
//! |-------|-----------------|-----------------|
//! | body, edge | F1E4A2     | 9A959C          |
//! | belly | F8F0CE          | CAC8CB          |
//! | tail  | EFE3AA          | A09BA2          |
//! | rim   | DCC67F          | 858086          |
//! | under | E2CE86          | 8A858B          |
//! | prim  | F8F1D2          | CECBCF          |
//! | prim2 | F4EAC2          | BBB8BC          |
//! | sec   | F3E7B0          | A9A5AB          |
//! | sec2  | F0E2A4          | 9B969D          |
//! | cov   | EEDC93          | 938E95          |
//! | foot  | C99289          | C99289          |
//!
//! The feet are the mockup's `FK.foot`, a shade of the beak, and the beak
//! (`FALK_BEAK`) is one colour on every coat, so the feet are too.

use crate::theme::FalkColor;
use crate::ui::geometry::ROBOT_COMPACT;
use crate::ui::mascot::{falk_ink, mix};
use crate::ui::play::art::tube::{LimbStyle, catmull_rom, limb, smooth};
use crate::ui::play::stories::{Draw, HEAD_PIVOT, MascotShot};
use crate::ui::robot::{GRID, Seg};

type Pt = (f32, f32);

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp2(a: Pt, b: Pt, t: f32) -> Pt {
    (lerp(a.0, b.0, t), lerp(a.1, b.1, t))
}

/// `l` from `o` heading `deg` (clockwise from +x).
fn toward(o: Pt, deg: f32, l: f32) -> Pt {
    let (sin, cos) = deg.to_radians().sin_cos();
    (o.0 + cos * l, o.1 + sin * l)
}

/// One wing's pose, the mockup's `{ arm, hand, ext, fan, sweep }`, in
/// degrees clockwise from +x for the right wing (the left mirrors it):
/// `arm` the arm's heading from the shoulder, `hand` the hand's turn past
/// it, `ext` how far both reach (0 folded, 1 open), `fan` how far the
/// primaries spread, `sweep` the feathers' turn from the bones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WingPose {
    pub arm: f32,
    pub hand: f32,
    pub ext: f32,
    pub fan: f32,
    pub sweep: f32,
}

impl WingPose {
    /// Folded at the side (`WING.fold`).
    pub const FOLD: WingPose = WingPose { arm: 96.0, hand: 4.0, ext: 0.3, fan: 0.08, sweep: 4.0 };
    /// Open wide (`WING.spread`).
    pub const SPREAD: WingPose = WingPose { arm: -14.0, hand: 10.0, ext: 1.0, fan: 1.0, sweep: 92.0 };
    /// Story f1's cloak, folded across the chest like crossed arms (`CROSS`).
    pub const CROSS: WingPose = WingPose { arm: 186.0, hand: 10.0, ext: 0.85, fan: 0.35, sweep: -42.0 };
    /// f1's cloak lowered a little to peek over (`LOW`).
    pub const LOW: WingPose = WingPose { arm: 166.0, hand: 6.0, ext: 0.8, fan: 0.35, sweep: -30.0 };
    /// Story f3's landing flare, braking forward; its 9 Hz flutter of 7
    /// degrees on `arm` is the story's.
    pub const FLARE: WingPose = WingPose { arm: -50.0, hand: 18.0, ext: 1.0, fan: 1.0, sweep: 72.0 };

    /// The mockup's `wmix`: each field `t` of the way from `a` to `b` (not
    /// clamped: f1 overshoots its spread to 1.2).
    pub fn mix(a: WingPose, b: WingPose, t: f32) -> WingPose {
        WingPose {
            arm: lerp(a.arm, b.arm, t),
            hand: lerp(a.hand, b.hand, t),
            ext: lerp(a.ext, b.ext, t),
            fan: lerp(a.fan, b.fan, t),
            sweep: lerp(a.sweep, b.sweep, t),
        }
    }
}

/// The mockup's Lutino tones (`FK`, `WC`), from the body the others relate to.
const LUTINO_BODY: u32 = 0xF1E4A2;
const BELLY: u32 = 0xF8F0CE;
const TAIL: u32 = 0xEFE3AA;
const RIM: u32 = 0xDCC67F;
const UNDER: u32 = 0xE2CE86;
const PRIM: u32 = 0xF8F1D2;
const PRIM2: u32 = 0xF4EAC2;
const SEC: u32 = 0xF3E7B0;
const SEC2: u32 = 0xF0E2A4;
const COV: u32 = 0xEEDC93;
const FOOT: u32 = 0xC99289;

/// One coat's colours for the wings and body (see the table above): the
/// body, the belly, the tail plumes, the toes, the rim every feather sits on,
/// the wing's underlay, the primaries and the secondaries (two tones each,
/// alternating), the coverts, and the bone's edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FalkCoat {
    pub body: u32,
    pub belly: u32,
    pub tail: u32,
    pub foot: u32,
    pub rim: u32,
    pub under: u32,
    pub prim: u32,
    pub prim2: u32,
    pub sec: u32,
    pub sec2: u32,
    pub cov: u32,
    pub edge: u32,
}

pub fn falk_coat(coat: FalkColor) -> FalkCoat {
    let body = falk_ink(coat).body;
    let t = |lutino| if coat == FalkColor::Lutino { lutino } else { tone(body, lutino) };
    FalkCoat {
        body,
        belly: t(BELLY),
        tail: t(TAIL),
        foot: FOOT,
        rim: t(RIM),
        under: t(UNDER),
        prim: t(PRIM),
        prim2: t(PRIM2),
        sec: t(SEC),
        sec2: t(SEC2),
        cov: t(COV),
        edge: body,
    }
}

/// `body` moved toward black or white, keeping its hue, as far in lightness
/// as `lutino` moves from `LUTINO_BODY`: darker, the body at the same share
/// of its lightness; lighter, the same share of the way to white.
fn tone(body: u32, lutino: u32) -> u32 {
    let (lb, lt) = (lightness(LUTINO_BODY), lightness(lutino));
    if lt <= lb { mix(0, body, lt / lb) } else { mix(body, 0xFFFFFF, (lt - lb) / (255.0 - lb)) }
}

/// HSL lightness, 0 to 255: halfway between the largest and smallest channel.
fn lightness(rgb: u32) -> f32 {
    let c = [rgb >> 16 & 0xFF, rgb >> 8 & 0xFF, rgb & 0xFF];
    (c.iter().max().unwrap_or(&0) + c.iter().min().unwrap_or(&0)) as f32 / 2.0
}

/// The mockup's `plume`: one soft feather from `root`, heading `deg`, `len`
/// long and `w` wide, half as wide at the root: two quads and a round tip
/// (the mockup's arc, as two quarter-circle cubics).
fn plume(root: Pt, deg: f32, len: f32, w: f32) -> Vec<Seg> {
    let (sin, cos) = deg.to_radians().sin_cos();
    let (d, n, h) = ((cos, sin), (-sin, cos), w / 2.0);
    let at = |p: Pt, v: Pt, k: f32| (p.0 + v.0 * k, p.1 + v.1 * k);
    let c = at(root, d, len - h);
    let (b1, b2) = (at(root, n, h * 0.5), at(root, n, -h * 0.5));
    let (s1, s2, tip) = (at(c, n, h), at(c, n, -h), at(c, d, h));
    let mid = at(root, d, len * 0.4);
    let (m1, m2) = (at(mid, n, h * 1.04), at(mid, n, -h * 1.04));
    // The control arm that keeps a quarter's cubic on its circle.
    let k = 0.552_284_8 * h;
    let (c1, c2, c3, c4) = (at(s1, d, k), at(tip, n, k), at(tip, n, -k), at(s2, d, k));
    vec![
        Seg::Move(b1.0, b1.1),
        Seg::Quad(m1.0, m1.1, s1.0, s1.1),
        Seg::Cubic(c1.0, c1.1, c2.0, c2.1, tip.0, tip.1),
        Seg::Cubic(c3.0, c3.1, c4.0, c4.1, s2.0, s2.1),
        Seg::Quad(m2.0, m2.1, b2.0, b2.1),
        Seg::Close,
    ]
}

/// The mockup's `wing`: the wing on `side` (+1 right, -1 left) from
/// `shoulder`, as `now` has it, its hand and primaries as `before` had it
/// (a story passes its pose about 70 ms earlier, so the tip trails the arm).
/// In paint order: one soft hull under every feather (filled, and stroked 3
/// wide), the five primaries outer first, the four secondaries, one row of
/// four coverts, each feather on a rim plume a little larger, and the bone
/// as a tube.
pub fn wing(side: i8, shoulder: Pt, now: WingPose, before: WingPose, coat: FalkColor) -> Vec<Draw> {
    let c = falk_coat(coat);
    let (p, q, sh) = (now, before, shoulder);
    let m = |a: f32| if side > 0 { a } else { 180.0 - a };
    let wr = toward(sh, m(p.arm), lerp(5.0, 10.5, p.ext));
    let tip = toward(wr, m(q.arm + q.hand), lerp(3.5, 8.5, q.ext));
    // (root, heading, length, width)
    let prim: Vec<(Pt, f32, f32, f32)> = (0..5)
        .map(|j| {
            let u = j as f32 / 4.0;
            let a = m(q.arm + q.hand + q.sweep * (1.0 - u * 0.86 * q.fan));
            (lerp2(wr, tip, u * 0.85), a, lerp(10.5, 13.6, u), lerp(5.0, 4.4, u))
        })
        .collect();
    let sec: Vec<(Pt, f32, f32, f32)> = (0..4)
        .map(|i| {
            let u = 0.18 + 0.82 * i as f32 / 3.0;
            (lerp2(sh, wr, u), m(p.arm + p.sweep + (i as f32 - 1.5) * 3.0 * p.fan), 9.0, 5.2)
        })
        .collect();
    // One soft hull ties every feather into a single wing shape, scalloped
    // only at the tips.
    let mut hull = vec![sh, wr, tip];
    hull.extend(prim.iter().rev().map(|&(r, a, l, _)| toward(r, a, l - 1.5)));
    hull.extend(sec.iter().rev().map(|&(r, a, l, _)| toward(r, a, l - 1.5)));
    hull.push(sh);
    let mut path = vec![Seg::Move(sh.0, sh.1)];
    smooth(&hull, &mut path);
    path.push(Seg::Close);
    let mut out = vec![Draw::Fill { path: path.clone(), rgb: c.under }, Draw::Stroke { path, rgb: c.under, width: 3.0 }];
    let mut layer = |(r, a, l, w): (Pt, f32, f32, f32), rgb: u32| {
        out.push(Draw::Fill { path: plume(r, a, l + 0.5, w + 0.8), rgb: c.rim });
        out.push(Draw::Fill { path: plume(r, a, l, w), rgb });
    };
    // Outer first, the inner ones overlap.
    for j in (0..5).rev() {
        layer(prim[j], if j % 2 == 1 { c.prim2 } else { c.prim });
    }
    for i in (0..4).rev() {
        layer(sec[i], if i % 2 == 1 { c.sec2 } else { c.sec });
    }
    // The greater coverts: one soft scalloped row along the arm.
    for i in 0..4 {
        layer((lerp2(sh, wr, 0.15 + i as f32 * 0.3), m(p.arm + p.sweep * 0.9), 6.2, 5.6), c.cov);
    }
    let bone = LimbStyle {
        w0: 6.8,
        w1: 3.6,
        pow: 1.1,
        cap: true,
        side_shade: Some((if side > 0 { 1 } else { -1 }, 0.3)),
        shade_op: 0.06,
        ..LimbStyle::default()
    };
    out.extend(limb(&catmull_rom(&[sh, wr, tip], 6), &bone, c.edge));
    out
}

/// Where the right wing joins the body (the left mirrors it).
const SHOULDER: Pt = (5.6, -15.5);
/// Where the head's pivot (`HEAD_PIVOT`) sits, above the feet.
const HEAD_AT: Pt = (0.0, -24.0);

/// The mockup's `bird` options the stories set: each wing now and a moment
/// before (`wing`), which wings wrap in front of the head, the head's face
/// (its `rot_deg`, `sx`, `sy` turn and scale it about its pivot, the
/// mockup's `headRot`, `hsx`, `hsy`; `dx`, `dy` are ignored), the tail's
/// turn and fan (degrees), and the feet tucked away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BirdPose {
    pub wl: WingPose,
    pub wr: WingPose,
    pub wl_before: WingPose,
    pub wr_before: WingPose,
    pub wl_front: bool,
    pub wr_front: bool,
    pub face: MascotShot,
    pub tail_rot: f32,
    pub tail_spread: f32,
    pub tuck: bool,
}

/// The bird's pieces as the mockup's `bird` returns them, for a story that
/// paints them in different layers (f1: the tail and body behind the
/// island, the feet, head and wrapped wings in front). Each piece is in the
/// same bird-local dip, so placing every one with the same `bird_at` keeps
/// the head on the body.
#[derive(Clone, Debug, PartialEq)]
pub struct BirdParts {
    /// The three tail plumes, turned about their root.
    pub tail: Draw,
    /// The body, the belly and the wings not in front.
    pub body: Vec<Draw>,
    pub feet: Vec<Draw>,
    /// The app's falk head at the slot head's size.
    pub head: Draw,
    pub wings_front: Vec<Draw>,
}

pub fn bird_parts(pose: &BirdPose, coat: FalkColor) -> BirdParts {
    let c = falk_coat(coat);
    let mut plumes = Vec::new();
    for (a, len) in [(-1.4f32, 18.0), (1.4, 18.0), (0.0, 19.5)] {
        let sign = if a > 0.0 { 1.0 } else if a < 0.0 { -1.0 } else { 0.0 };
        let (root, deg) = ((a * 0.6, -3.0), 90.0 - a * 2.0 - pose.tail_spread * sign);
        plumes.push(Draw::Fill { path: plume(root, deg, len + 0.6, 4.6), rgb: c.rim });
        plumes.push(Draw::Fill { path: plume(root, deg, len, 3.6), rgb: c.tail });
    }
    let tail = Draw::Group { at: (0.0, -3.0), rot_deg: pose.tail_rot, scale: (1.0, 1.0), pivot: (0.0, -3.0), items: plumes };
    let wl = wing(-1, (-SHOULDER.0, SHOULDER.1), pose.wl, pose.wl_before, coat);
    let wr = wing(1, SHOULDER, pose.wr, pose.wr_before, coat);
    let mut body = vec![
        Draw::Ellipse { c: (0.0, -10.0), r: (7.6, 9.6), rgb: c.body },
        Draw::Ellipse { c: (0.0, -6.6), r: (4.8, 6.2), rgb: c.belly },
    ];
    // The wings lie over the body's sides (front view), under the head
    // unless they wrap in front of it.
    let mut wings_front = Vec::new();
    for (w, front) in [(wl, pose.wl_front), (wr, pose.wr_front)] {
        if front { wings_front.extend(w) } else { body.extend(w) }
    }
    let toe = |x: f32| Draw::Stroke {
        path: vec![Seg::Move(x - 1.7, -0.6), Seg::Quad(x, 2.4, x + 1.7, -0.6)],
        rgb: c.foot,
        width: 1.35,
    };
    let feet = if pose.tuck { Vec::new() } else { vec![toe(-2.6), toe(2.6)] };
    let unit = ROBOT_COMPACT / GRID;
    let head = Draw::Group {
        at: HEAD_AT,
        rot_deg: 0.0,
        scale: (unit, unit),
        pivot: HEAD_PIVOT,
        items: vec![Draw::Head { at: HEAD_PIVOT, shot: pose.face }],
    };
    BirdParts { tail, body, feet, head, wings_front }
}

/// The whole bird in one `Group` on its local dip (origin between the
/// feet), in the mockup's order: tail, body and the wings behind, feet,
/// head, the wings in front.
pub fn bird(pose: &BirdPose, coat: FalkColor) -> Draw {
    let p = bird_parts(pose, coat);
    let mut items = vec![p.tail];
    items.extend(p.body);
    items.extend(p.feet);
    items.push(p.head);
    items.extend(p.wings_front);
    Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items }
}

/// The mockup's `birdG`: `items` in bird-local dip with the feet's point at
/// `at` (window dip), turned `rot_deg` (clockwise) and scaled `sx`, `sy`.
pub fn bird_at(at: Pt, rot_deg: f32, sx: f32, sy: f32, items: Vec<Draw>) -> Draw {
    Draw::Group { at, rot_deg, scale: (sx, sy), pivot: (0.0, 0.0), items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::geometry::{ROBOT_COMPACT, Rect};
    use crate::ui::mascot::{CrestPose, falk_ink};
    use crate::ui::play::stories::HEAD_PIVOT;
    use crate::ui::robot::{Expr, GRID, Seg};

    /// The right wing's shoulder, the mockup's.
    const SH: (f32, f32) = (5.6, -15.5);

    fn face() -> MascotShot {
        MascotShot {
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
        }
    }

    fn resting() -> BirdPose {
        BirdPose {
            wl: WingPose::FOLD,
            wr: WingPose::FOLD,
            wl_before: WingPose::FOLD,
            wr_before: WingPose::FOLD,
            wl_front: false,
            wr_front: false,
            face: face(),
            tail_rot: 0.0,
            tail_spread: 0.0,
            tuck: false,
        }
    }

    fn items(d: &Draw) -> &[Draw] {
        match d {
            Draw::Group { items, .. } => items,
            other => panic!("not a group: {other:?}"),
        }
    }

    fn bounds(d: &[Draw]) -> Rect {
        Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items: d.to_vec() }.bounds()
    }

    /// The paths of the fills in any of `rgb`, in paint order.
    fn fills<'a>(d: &'a [Draw], rgb: &[u32]) -> Vec<&'a Vec<Seg>> {
        d.iter()
            .filter_map(|x| match x {
                Draw::Fill { path, rgb: v } if rgb.contains(v) => Some(path),
                _ => None,
            })
            .collect()
    }

    /// A feather's root (between its two base points) and its round tip
    /// (where the tip's two arcs meet).
    fn root_tip(p: &[Seg]) -> ((f32, f32), (f32, f32)) {
        let (Seg::Move(ax, ay), Seg::Cubic(.., tx, ty), Seg::Quad(.., bx, by)) = (p[0], p[2], p[4]) else {
            panic!("not a feather: {p:?}");
        };
        (((ax + bx) / 2.0, (ay + by) / 2.0), (tx, ty))
    }

    fn heading(p: &[Seg]) -> f32 {
        let (r, t) = root_tip(p);
        (t.1 - r.1).atan2(t.0 - r.0).to_degrees()
    }

    fn channels(rgb: u32) -> [u32; 3] {
        [rgb >> 16 & 0xFF, rgb >> 8 & 0xFF, rgb & 0xFF]
    }

    #[test]
    fn pose_mix_at_0_and_1_returns_the_ends() {
        let (a, b) = (WingPose::FOLD, WingPose::SPREAD);
        assert_eq!(WingPose::mix(a, b, 0.0), a);
        assert_eq!(WingPose::mix(a, b, 1.0), b);
        let h = WingPose::mix(a, b, 0.5);
        let want = [41.0, 7.0, 0.65, 0.54, 48.0];
        for (v, w) in [h.arm, h.hand, h.ext, h.fan, h.sweep].iter().zip(want) {
            assert!((v - w).abs() < 1e-4, "{h:?}");
        }
        // Past the ends it carries on, as story f1's spread overshoots to 1.2.
        assert!((WingPose::mix(a, b, 1.2).arm - (96.0 - 110.0 * 1.2)).abs() < 1e-4);
    }

    #[test]
    fn the_named_poses_are_the_mockups() {
        let p = |arm, hand, ext, fan, sweep| WingPose { arm, hand, ext, fan, sweep };
        // `WING.fold`, `WING.spread`; f1's cloak `CROSS` and its peek `LOW`;
        // f3's landing flare (before its 9 Hz flutter of 7 degrees).
        assert_eq!(WingPose::FOLD, p(96.0, 4.0, 0.3, 0.08, 4.0));
        assert_eq!(WingPose::SPREAD, p(-14.0, 10.0, 1.0, 1.0, 92.0));
        assert_eq!(WingPose::CROSS, p(186.0, 10.0, 0.85, 0.35, -42.0));
        assert_eq!(WingPose::LOW, p(166.0, 6.0, 0.8, 0.35, -30.0));
        assert_eq!(WingPose::FLARE, p(-50.0, 18.0, 1.0, 1.0, 72.0));
    }

    #[test]
    fn an_open_wing_has_nine_feathers_and_a_covert_row() {
        let c = falk_coat(FalkColor::Lutino);
        let d = wing(1, SH, WingPose::SPREAD, WingPose::SPREAD, FalkColor::Lutino);
        // The base first: one soft hull, filled and stroked 3 wide.
        assert!(matches!(&d[0], Draw::Fill { rgb, .. } if *rgb == c.under), "{:?}", d[0]);
        assert!(matches!(&d[1], Draw::Stroke { rgb, width, .. } if *rgb == c.under && *width == 3.0), "{:?}", d[1]);
        // Five primaries (outer first, alternating), four secondaries, the
        // row of four coverts; each on a rim plume drawn just before it, a
        // little longer and wider.
        let colours: Vec<u32> = d[2..28]
            .chunks(2)
            .map(|pair| match pair {
                [Draw::Fill { rgb: r, .. }, Draw::Fill { rgb, .. }] => {
                    assert_eq!(*r, c.rim);
                    let (a, b) = (bounds(&pair[..1]), bounds(&pair[1..]));
                    assert!(a.x <= b.x && a.y <= b.y && a.x + a.w >= b.x + b.w && a.y + a.h >= b.y + b.h, "{a:?} {b:?}");
                    *rgb
                }
                other => panic!("not a feather on its rim: {other:?}"),
            })
            .collect();
        let (p1, p2, s1, s2, cv) = (c.prim, c.prim2, c.sec, c.sec2, c.cov);
        assert_eq!(colours, [p1, p2, p1, p2, p1, s2, s1, s2, s1, cv, cv, cv, cv]);
        // Every feather is a capsule with a round tip: two quads and the
        // tip's two arcs.
        for p in fills(&d, &[c.rim, p1, p2, s1, s2, cv]) {
            let quads = p.iter().filter(|s| matches!(s, Seg::Quad(..))).count();
            let arcs = p.iter().filter(|s| matches!(s, Seg::Cubic(..))).count();
            assert_eq!((p.len(), quads, arcs), (6, 2, 2), "{p:?}");
        }
        // The coverts root along the arm, shoulder to wrist.
        let a = (-14.0f32).to_radians();
        let wrist = (SH.0 + a.cos() * 10.5, SH.1 + a.sin() * 10.5);
        for (i, p) in fills(&d, &[cv]).iter().enumerate() {
            let u = 0.15 + i as f32 * 0.3;
            let want = (SH.0 + (wrist.0 - SH.0) * u, SH.1 + (wrist.1 - SH.1) * u);
            let (r, _) = root_tip(p);
            assert!((r.0 - want.0).abs() < 1e-3 && (r.1 - want.1).abs() < 1e-3, "{i}: {r:?} {want:?}");
        }
        // The bone last, a tube in the edge colour with its faint shade band.
        assert_eq!(d.len(), 30);
        assert!(matches!(&d[28], Draw::Fill { rgb, .. } if *rgb == c.edge), "{:?}", d[28]);
        assert!(matches!(&d[29], Draw::Tint { rgb: 0, alpha, .. } if *alpha == 0.06), "{:?}", d[29]);
    }

    #[test]
    fn a_folded_wing_hugs_the_body() {
        // The mockup's front view: folded, the wing lies over the body's side
        // (the body is 7.6 either side of x 0), past it only by the bone's
        // round edge, and no higher than the bone's cap over the shoulder;
        // the primaries hang down past the belly as a folded bird's do.
        let fold = bounds(&wing(1, SH, WingPose::FOLD, WingPose::FOLD, FalkColor::Lutino));
        assert!(fold.x >= -7.6 && fold.x + fold.w <= 7.6 + 2.5, "{fold:?}");
        assert!(fold.y >= SH.1 - 5.0 && fold.y + fold.h > 0.0, "{fold:?}");
        // The left wing is its mirror.
        let left = bounds(&wing(-1, (-SH.0, SH.1), WingPose::FOLD, WingPose::FOLD, FalkColor::Lutino));
        assert!(
            (left.x + fold.x + fold.w).abs() < 1e-3 && (left.w - fold.w).abs() < 1e-3 && (left.y - fold.y).abs() < 1e-3,
            "{left:?} {fold:?}"
        );
        // Open, it reaches far out past the body, and over the shoulder.
        let open = bounds(&wing(1, SH, WingPose::SPREAD, WingPose::SPREAD, FalkColor::Lutino));
        assert!(open.x + open.w > 30.0 && open.y < SH.1 - 4.0, "{open:?}");
    }

    #[test]
    fn the_primaries_use_the_pose_before() {
        let c = falk_coat(FalkColor::Lutino);
        let (now, then) = (WingPose::SPREAD, WingPose::mix(WingPose::SPREAD, WingPose::FOLD, 0.3));
        let lag = wing(1, SH, now, then, FalkColor::Lutino);
        let same = wing(1, SH, now, now, FalkColor::Lutino);
        // Only the primaries move with `before`; the secondaries and the
        // coverts follow the arm as it is now.
        assert_ne!(fills(&lag, &[c.prim, c.prim2]), fills(&same, &[c.prim, c.prim2]));
        assert_eq!(fills(&lag, &[c.sec, c.sec2, c.cov]), fills(&same, &[c.sec, c.sec2, c.cov]));
        // The outermost primary (painted first) points where `before` aims
        // it: arm + hand + sweep, the fan closing it 0.86 of the sweep.
        let want = then.arm + then.hand + then.sweep * (1.0 - 0.86 * then.fan);
        let got = heading(fills(&lag, &[c.prim])[0]);
        assert!((got - want).abs() < 0.01, "{got} {want}");
        // Mirrored for the left wing.
        let left = wing(-1, (-SH.0, SH.1), now, then, FalkColor::Lutino);
        let got = heading(fills(&left, &[c.prim])[0]);
        assert!((got - (180.0 - want)).abs() < 0.01, "{got} {}", 180.0 - want);
    }

    #[test]
    fn every_coat_colours_the_wing_like_the_head() {
        // Lutino is the mockup's own table (`FK`, `WC`), exactly.
        let l = falk_coat(FalkColor::Lutino);
        assert_eq!(
            [l.body, l.belly, l.tail, l.foot, l.rim, l.under, l.prim, l.prim2, l.sec, l.sec2, l.cov, l.edge],
            [
                0xF1E4A2, 0xF8F0CE, 0xEFE3AA, 0xC99289, 0xDCC67F, 0xE2CE86, 0xF8F1D2, 0xF4EAC2, 0xF3E7B0, 0xF0E2A4,
                0xEEDC93, 0xF1E4A2
            ]
        );
        for coat in FalkColor::ALL {
            let (ink, c) = (falk_ink(*coat), falk_coat(*coat));
            // The body and the wing's bone are the head's own body colour.
            assert_eq!((c.body, c.edge), (ink.body, ink.body), "{coat:?}");
            let d = wing(1, SH, WingPose::SPREAD, WingPose::SPREAD, *coat);
            assert!(matches!(&d[0], Draw::Fill { rgb, .. } if *rgb == c.under), "{coat:?}");
            assert!(matches!(&d[28], Draw::Fill { rgb, .. } if *rgb == ink.body), "{coat:?}");
            let b = bird(&resting(), *coat);
            assert!(
                items(&b).iter().any(|x| matches!(x, Draw::Ellipse { c: (0.0, -10.0), rgb, .. } if *rgb == ink.body)),
                "{coat:?}: the body"
            );
            // The darker tones stay darker than the body, the lighter ones
            // lighter, channel by channel.
            let body = channels(c.body);
            for t in [c.rim, c.under, c.cov] {
                assert!(channels(t).iter().zip(body).all(|(v, b)| *v <= b), "{coat:?}: {t:06X}");
            }
            for t in [c.belly, c.prim, c.prim2, c.sec] {
                assert!(channels(t).iter().zip(body).all(|(v, b)| *v >= b), "{coat:?}: {t:06X}");
            }
        }
        // Grey and WhiteFace share the grey body, so their wings are one.
        let g = falk_coat(FalkColor::Grey);
        assert_eq!(g, falk_coat(FalkColor::WhiteFace));
        assert_eq!((g.prim, g.rim, g.belly), (0xCECBCF, 0x858086, 0xCAC8CB), "the table in the module's notes");
        // Their tones keep the body's hue: each is the body moved toward
        // black or white as far, in lightness, as the Lutino tone moves from
        // the Lutino body, so its channel spread is the body's scaled by that
        // move, with no cast of Lutino's yellow.
        let light = |rgb: u32| {
            let c = channels(rgb);
            (c.iter().max().unwrap() + c.iter().min().unwrap()) as f32 / 2.0
        };
        let spread = |rgb: u32| {
            let c = channels(rgb);
            (c.iter().max().unwrap() - c.iter().min().unwrap()) as f32
        };
        let pairs = [
            (g.belly, l.belly),
            (g.tail, l.tail),
            (g.rim, l.rim),
            (g.under, l.under),
            (g.prim, l.prim),
            (g.prim2, l.prim2),
            (g.sec, l.sec),
            (g.sec2, l.sec2),
            (g.cov, l.cov),
        ];
        let moved = |body: f32, t: f32| if t <= body { t / body } else { (t - body) / (255.0 - body) };
        for (t, lt) in pairs {
            let (lb, lg) = (light(g.body), light(t));
            assert_eq!(lg <= lb, light(lt) <= light(l.body), "{t:06X}: the same way as {lt:06X}");
            assert!((moved(lb, lg) - moved(light(l.body), light(lt))).abs() < 0.02, "{t:06X}: as far as {lt:06X}");
            let k = if lg <= lb { lg / lb } else { (255.0 - lg) / (255.0 - lb) };
            assert!((spread(t) - spread(g.body) * k).abs() <= 1.5, "{t:06X}: a cast ({} vs {})", spread(t), spread(g.body) * k);
        }
    }

    /// A feather's root, length (root to its round tip) and width (across
    /// its tip's two ends); its base is half as wide.
    fn size(p: &[Seg]) -> ((f32, f32), f32, f32) {
        let ((Seg::Quad(.., ax, ay), Seg::Cubic(.., bx, by)), (Seg::Move(mx, my), Seg::Quad(.., nx, ny))) = ((p[1], p[3]), (p[0], p[4])) else {
            panic!("not a feather: {p:?}");
        };
        let (root, tip) = root_tip(p);
        let w = (ax - bx).hypot(ay - by);
        assert!(((mx - nx).hypot(my - ny) - w / 2.0).abs() < 1e-3, "{p:?}");
        (root, (tip.0 - root.0).hypot(tip.1 - root.1), w)
    }

    #[test]
    fn the_feathers_are_the_mockups_size_and_the_hand_is_before() {
        let c = falk_coat(FalkColor::Lutino);
        let (now, then) = (WingPose::SPREAD, WingPose::mix(WingPose::SPREAD, WingPose::FOLD, 0.4));
        let d = wing(1, SH, now, then, FalkColor::Lutino);
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        // The wrist where the arm puts it now; the wing's tip where the hand
        // was a moment before: `before.arm + before.hand`, `before.ext` long.
        let wrist = toward(SH, now.arm, lerp(5.0, 10.5, now.ext));
        let tip = toward(wrist, then.arm + then.hand, lerp(3.5, 8.5, then.ext));
        // The primaries root from the wrist toward that tip (painted outer
        // first), 10.5 to 13.6 long and 5 to 4.4 wide.
        for (k, p) in fills(&d, &[c.prim, c.prim2]).iter().enumerate() {
            let u = (4 - k) as f32 / 4.0;
            let (root, len, w) = size(p);
            let want = lerp2(wrist, tip, u * 0.85);
            assert!(near(root.0, want.0) && near(root.1, want.1), "{k}: {root:?} {want:?}");
            assert!(near(len, lerp(10.5, 13.6, u)) && near(w, lerp(5.0, 4.4, u)), "{k}: {len} {w}");
        }
        for p in fills(&d, &[c.sec, c.sec2]) {
            let (_, len, w) = size(p);
            assert!(near(len, 9.0) && near(w, 5.2), "{len} {w}");
        }
        for p in fills(&d, &[c.cov]) {
            let (_, len, w) = size(p);
            assert!(near(len, 6.2) && near(w, 5.6), "{len} {w}");
        }
        // Each rim is its feather from the same root, 0.5 longer, 0.8 wider.
        for pair in d[2..28].chunks(2) {
            let [Draw::Fill { path: rim, .. }, Draw::Fill { path, .. }] = pair else {
                panic!("{pair:?}");
            };
            let ((r0, l0, w0), (r1, l1, w1)) = (size(rim), size(path));
            assert!(near(r0.0, r1.0) && near(r0.1, r1.1) && near(l0 - l1, 0.5) && near(w0 - w1, 0.8), "{rim:?} {path:?}");
        }
    }

    #[test]
    fn the_head_sits_on_the_body_in_one_group() {
        let unit = ROBOT_COMPACT / GRID;
        let d = bird(&resting(), FalkColor::Lutino);
        let Draw::Group { at, rot_deg, scale, pivot, items: all } = &d else {
            panic!("not one group: {d:?}");
        };
        assert_eq!((*at, *rot_deg, *scale, *pivot), ((0.0, 0.0), 0.0, (1.0, 1.0), (0.0, 0.0)));
        // The head last (no wing in front): the app's painter at the slot
        // head's size, its pivot 24 above the feet.
        let Some(Draw::Group { at, rot_deg, scale, pivot, items: head }) = all.last() else {
            panic!("no head last: {:?}", all.last());
        };
        assert_eq!((*at, *rot_deg, *scale, *pivot), ((0.0, -24.0), 0.0, (unit, unit), HEAD_PIVOT));
        assert_eq!(head.as_slice(), [Draw::Head { at: HEAD_PIVOT, shot: face() }]);
        // The egg's bottom (grid 57) lands inside the body (top at -19.6).
        let chin = -24.0 + (57.0 - HEAD_PIVOT.1) * unit;
        assert!(chin > -19.6 && chin < -10.0, "{chin}");
        // `bird_at` moves the whole figure in one transform, as the mockup's `birdG`.
        let g = bird_at((100.0, 40.0), 180.0, 1.0, 0.9, vec![d.clone()]);
        assert_eq!(g, Draw::Group { at: (100.0, 40.0), rot_deg: 180.0, scale: (1.0, 0.9), pivot: (0.0, 0.0), items: vec![d] });
    }

    #[test]
    fn a_wing_in_front_is_drawn_after_the_head() {
        let c = falk_coat(FalkColor::Lutino);
        let under = |d: &[Draw]| d.iter().filter(|x| matches!(x, Draw::Fill { rgb, .. } if *rgb == c.under)).count();
        let head = |d: &[Draw]| {
            d.iter()
                .position(|x| matches!(x, Draw::Group { items, .. } if matches!(items.as_slice(), [Draw::Head { .. }])))
                .expect("a head")
        };
        let back = bird(&resting(), FalkColor::Lutino);
        let all = items(&back);
        assert_eq!((under(&all[..head(all)]), under(&all[head(all)..])), (2, 0));
        let front = bird(&BirdPose { wl_front: true, ..resting() }, FalkColor::Lutino);
        let all = items(&front);
        assert_eq!((under(&all[..head(all)]), under(&all[head(all)..])), (1, 1));
    }

    #[test]
    fn a_tucked_bird_has_no_feet() {
        let c = falk_coat(FalkColor::Lutino);
        let feet = |p: &BirdPose| {
            items(&bird(p, FalkColor::Lutino))
                .iter()
                .filter(|x| matches!(x, Draw::Stroke { rgb, width, .. } if *rgb == c.foot && *width == 1.35))
                .count()
        };
        assert_eq!(feet(&resting()), 2);
        assert_eq!(feet(&BirdPose { tuck: true, ..resting() }), 0);
    }

    #[test]
    fn the_tail_fans_and_turns_about_its_root() {
        let c = falk_coat(FalkColor::Lutino);
        let tail = |p: &BirdPose| match &items(&bird(p, FalkColor::Lutino))[0] {
            Draw::Group { at, rot_deg, pivot, items, .. } => {
                assert_eq!((*at, *pivot), ((0.0, -3.0), (0.0, -3.0)));
                (*rot_deg, fills(items, &[c.tail]).iter().map(|p| heading(p)).collect::<Vec<_>>())
            }
            other => panic!("no tail first: {other:?}"),
        };
        // Three plumes straight down, the outer two 2.8 degrees out.
        let (rot, h) = tail(&resting());
        assert_eq!(rot, 0.0);
        for (got, want) in h.iter().zip([92.8, 87.2, 90.0]) {
            assert!((got - want).abs() < 0.01, "{h:?}");
        }
        // Spread fans the outer two, a turn turns all three.
        let (rot, h) = tail(&BirdPose { tail_spread: 14.0, tail_rot: -6.0, ..resting() });
        assert_eq!(rot, -6.0);
        for (got, want) in h.iter().zip([106.8, 73.2, 90.0]) {
            assert!((got - want).abs() < 0.01, "{h:?}");
        }
    }
}
