//! The island's outline as a point list the animations can dent, squeeze,
//! swing, shift and tilt, with the pivot each style needs.

use super::rim::rim;
use crate::config::Style;
use crate::ui::geometry::{ShapeRect, shoulder_depth};
use windows_numerics::Matrix3x2;

/// How many rim samples the outline takes. A swinging Notch returns more
/// points than this, for its extension above the screen edge.
pub const OUTLINE_PTS: usize = 90;

/// A bell on the rim at `u`. `depth` and `width` are fractions of the island's
/// height: positive depth pushes the outline out, negative pulls it in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dent {
    pub u: f32,
    pub depth: f32,
    pub width: f32,
}

/// `shift` and `tilt_deg` only act on the Pill; the Notch stays welded to the
/// screen edge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deform {
    pub dents: Vec<Dent>,
    /// The island's width shrinks by this fraction, about its centre.
    pub squeeze: f32,
    pub swing_deg: f32,
    pub shift: (f32, f32),
    pub tilt_deg: f32,
    /// The left end pushed out by this many dip (negative pulls it in), most
    /// at mid-height and fading toward the island's left quarter: the
    /// mockup's `bulgeL`.
    pub bulge_left: f32,
    /// The island alone moves sideways this many dip, in both styles (the
    /// Notch along the screen edge); unlike `shift`, what sits on it (the
    /// clock) stays put: the mockup's `dx` (a sneeze's jolt).
    pub jolt: f32,
}

/// True when `d` leaves the island's geometry untouched; a dent with no depth
/// or no width counts as none.
///
/// Shift and tilt do nothing on the Notch, as in `outline`.
pub fn is_rest(style: Style, d: &Deform) -> bool {
    let pill = style == Style::Pill;
    d.squeeze == 0.0
        && d.swing_deg == 0.0
        && d.bulge_left == 0.0
        && d.jolt == 0.0
        && (!pill || (d.shift == (0.0, 0.0) && d.tilt_deg == 0.0))
        && d.dents.iter().all(|k| k.depth == 0.0 || k.width <= 0.0)
}

/// What turns and moves the island as a whole: where it turns, by how much,
/// and how far it shifts. The Notch stays welded to the screen edge, so it only
/// swings, about the edge's middle; the Pill also tilts and shifts, about its
/// centre.
struct Motion {
    pivot: (f32, f32),
    deg: f32,
    shift: (f32, f32),
}

fn motion(style: Style, s: ShapeRect, d: &Deform) -> Motion {
    let cx = s.x + s.w / 2.0;
    if style == Style::Notch {
        Motion {
            pivot: (cx, s.y),
            deg: d.swing_deg,
            shift: (0.0, 0.0),
        }
    } else {
        Motion {
            pivot: (cx, s.y + s.h / 2.0),
            deg: d.swing_deg + d.tilt_deg,
            shift: d.shift,
        }
    }
}

/// `m` as a Direct2D matrix (row vectors: `x' = x * M11 + y * M21 + M31`).
fn matrix(m: &Motion) -> Matrix3x2 {
    let ((px, py), (sin, cos)) = (m.pivot, m.deg.to_radians().sin_cos());
    Matrix3x2 {
        M11: cos,
        M12: sin,
        M21: -sin,
        M22: cos,
        M31: px - px * cos + py * sin + m.shift.0,
        M32: py - px * sin - py * cos + m.shift.1,
    }
}

/// The island's rigid motion as one matrix: the swing (plus the Pill's tilt)
/// about the style's pivot, and the Pill's shift. Not the squeeze, the dents,
/// the bulge or the jolt. `outline` places every point with it, and whatever sits on the
/// island (the clock) draws under it, so the two cannot drift apart.
pub fn rigid(style: Style, s: ShapeRect, d: &Deform) -> Matrix3x2 {
    matrix(&motion(style, s, d))
}

/// A turn of `deg` about `pivot`, then a shift: the one rotation `rigid` is
/// made of, for a piece that turns about its own centre.
pub fn turn_about(deg: f32, pivot: (f32, f32), shift: (f32, f32)) -> Matrix3x2 {
    matrix(&Motion { pivot, deg, shift })
}

/// The deformed lower outline, left to right. The Notch's outline grows two
/// short runs above the screen edge while it swings, so the rotation never
/// leaves a gap at the top of the screen.
pub fn outline(style: Style, s: ShapeRect, d: &Deform) -> Vec<(f32, f32)> {
    let notch = style == Style::Notch;
    let swing = motion(style, s, d).deg;
    let rigid = rigid(style, s, d);
    let pivot_x = s.x + s.w / 2.0;
    let keep = 1.0 - d.squeeze;
    // The squeeze about the centre, then the island's own motion.
    let place = |x: f32, y: f32| {
        let x = pivot_x + (x - pivot_x) * keep;
        (
            x * rigid.M11 + y * rigid.M21 + rigid.M31,
            x * rigid.M12 + y * rigid.M22 + rigid.M32,
        )
    };

    let mut out = Vec::with_capacity(OUTLINE_PTS + 2);
    let step = 1.0 / (OUTLINE_PTS - 1) as f32;
    out.extend((0..OUTLINE_PTS).map(|i| {
        let p = rim(style, s, i as f32 * step);
        (p.x, p.y)
    }));
    // Each dent measures from the undisplaced rim and displacements add, so
    // overlapping dents do not depend on their order.
    for k in d.dents.iter().filter(|k| k.depth != 0.0 && k.width > 0.0) {
        let c = rim(style, s, k.u);
        let reach = k.width * s.h;
        for (i, pt) in out.iter_mut().enumerate() {
            let p = rim(style, s, i as f32 * step);
            let t = (p.x - c.x).hypot(p.y - c.y) / reach;
            if t < 1.0 {
                let push = k.depth * s.h * 0.5 * (1.0 + (t * std::f32::consts::PI).cos());
                pt.0 += p.nx * push;
                pt.1 += p.ny * push;
            }
        }
    }
    if d.bulge_left != 0.0 {
        for pt in out.iter_mut() {
            pt.0 += left_pull(s, d.bulge_left, pt.0, pt.1);
        }
    }
    for pt in out.iter_mut() {
        *pt = place(pt.0, pt.1);
        pt.0 += d.jolt;
    }
    if notch && swing != 0.0 {
        // Rotated by `a`, the side lines cross the screen edge `w/2 * tan a`
        // above the top corners; reaching a little past that keeps them
        // crossing. Assumes `squeeze >= 0`: a bulge (`keep > 1`) swinging at
        // once would need more.
        let lift = s.w / 2.0 * swing.to_radians().abs().tan() + 1.0;
        let ext = |x: f32| {
            let (x, y) = place(x, s.y - lift);
            (x + d.jolt, y)
        };
        out.insert(0, ext(s.x));
        out.push(ext(s.x + s.w));
    }
    out
}

/// How far `bulge` (`Deform::bulge_left`) moves the point (x, y) of the
/// island `s` sideways: the mockup's `bulgeL`, a bell about mid-height times
/// a ramp from the left quarter (none) to the left end (all).
fn left_pull(s: ShapeRect, bulge: f32, x: f32, y: f32) -> f32 {
    let quarter = s.w / 4.0;
    let ramp = ((s.x + quarter - x) / quarter).clamp(0.0, 1.0);
    let bell = (-((y - (s.y + s.h / 2.0)) / (0.6 * s.h)).powi(2)).exp();
    -bulge * bell * ramp
}

/// The body's left and right side at the foot of the Notch's shoulders
/// (`shoulder_depth` below the screen edge), where each shoulder meets it:
/// moved by the squeeze, the left bulge, the jolt and the swing (a swung
/// side crosses the foot's height elsewhere: on the side, its corner or the
/// run above the screen edge, as `outline` draws them). The shoulders
/// themselves never turn: they slide along the edge with it. Unswung, a dent
/// is not followed; none reaches that high. The Pill has no shoulders.
pub fn top_corners(s: ShapeRect, d: &Deform) -> (f32, f32) {
    let cx = s.x + s.w / 2.0;
    let foot = s.y + shoulder_depth(s);
    let keep = 1.0 - d.squeeze;
    if d.swing_deg == 0.0 {
        let left = s.x + left_pull(s, d.bulge_left, s.x, foot);
        let right = s.x + s.w;
        return (cx + (left - cx) * keep + d.jolt, cx + (right - cx) * keep + d.jolt);
    }
    // The first crossing of the foot's height from each end of the drawn
    // outline, which starts and ends above the screen edge while it swings.
    let pts = outline(Style::Notch, s, d);
    let cross = |a: &(f32, f32), b: &(f32, f32)| {
        ((a.1 - foot) * (b.1 - foot) <= 0.0 && a.1 != b.1).then(|| a.0 + (foot - a.1) / (b.1 - a.1) * (b.0 - a.0))
    };
    let left = pts.windows(2).find_map(|w| cross(&w[0], &w[1]));
    let right = pts.windows(2).rev().find_map(|w| cross(&w[1], &w[0]));
    (left.unwrap_or(s.x), right.unwrap_or(s.x + s.w))
}

/// The mockup's dent `{x, w, d}` (a bell on the lower outline at `x`, `w`
/// its half-width at 1/e, `d` dip, positive down) as a `Dent`: placed where
/// the lower outline is at `x`, as wide at half depth, and as deep there as
/// the mockup's weight (`bw`, 1 on the lip, 0 at mid-height) leaves it.
pub fn lip_dent(style: Style, s: ShapeRect, x: f32, w: f32, d: f32) -> Dent {
    let mid = s.y + s.h / 2.0;
    // The lower half: the outline falls to the lip over the first half of
    // `u` and is symmetric, so its ends are where it crosses mid-height.
    let (mut a, mut b) = (0.0_f32, 0.5_f32);
    for _ in 0..24 {
        let m = (a + b) / 2.0;
        if rim(style, s, m).y < mid {
            a = m;
        } else {
            b = m;
        }
    }
    // Along it x only rises.
    let (mut lo, mut hi) = (b, 1.0 - b);
    for _ in 0..24 {
        let m = (lo + hi) / 2.0;
        if rim(style, s, m).x < x {
            lo = m;
        } else {
            hi = m;
        }
    }
    let u = (lo + hi) / 2.0;
    let weight = ((rim(style, s, u).y - mid) / (s.h / 2.0)).clamp(0.0, 1.0);
    // A cosine bell is at half depth half its reach out; the mockup's
    // Gaussian is at w * sqrt(ln 2).
    let reach = 2.0 * w * std::f32::consts::LN_2.sqrt();
    Dent { u, depth: d * weight / s.h, width: reach / s.h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::play::testing::shape;

    #[test]
    fn a_rest_deform_is_exactly_the_rim() {
        for style in [Style::Notch, Style::Pill] {
            let s = shape(style, 190.0, 34.0);
            let pts = outline(style, s, &Deform::default());
            assert_eq!(pts.len(), OUTLINE_PTS);
            for (i, (x, y)) in pts.iter().enumerate() {
                let p = rim(style, s, i as f32 / (OUTLINE_PTS - 1) as f32);
                assert!((x - p.x).abs() < 0.01 && (y - p.y).abs() < 0.01, "{style:?} point {i}");
            }
        }
    }

    #[test]
    fn a_dent_moves_only_the_outline_near_it() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let rest = outline(Style::Notch, s, &Deform::default());
        let d = Deform { dents: vec![Dent { u: 0.5, depth: -0.3, width: 0.08 }], ..Default::default() };
        let bent = outline(Style::Notch, s, &d);
        assert_eq!(bent.len(), rest.len());
        let moved: Vec<usize> = bent.iter().zip(&rest).enumerate()
            .filter(|(_, (b, r))| (b.0 - r.0).abs() + (b.1 - r.1).abs() > 0.01)
            .map(|(i, _)| i)
            .collect();
        assert!(!moved.is_empty(), "the dent moved nothing");
        let deepest = bent.iter().zip(&rest).map(|(b, r)| (b.1 - r.1).abs()).fold(0.0_f32, f32::max);
        assert!(deepest > 3.0, "the dent is only {deepest} deep");
        let centre = rim(Style::Notch, s, 0.5);
        for i in moved {
            let p = rim(Style::Notch, s, i as f32 / (OUTLINE_PTS - 1) as f32);
            assert!((p.x - centre.x).hypot(p.y - centre.y) < 0.08 * s.h + 0.01,
                "point {i} moved but lies outside the dent's width");
        }
    }

    #[test]
    fn a_dent_pushes_the_lip_the_way_its_sign_says_at_every_size() {
        for (w, h) in [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)] {
            let s = shape(Style::Notch, w, h);
            let rest = outline(Style::Notch, s, &Deform::default());
            let mid = rim(Style::Notch, s, 0.5);
            let nearest = (0..OUTLINE_PTS)
                .min_by(|&a, &b| {
                    let da = (rest[a].0 - mid.x).hypot(rest[a].1 - mid.y);
                    let db = (rest[b].0 - mid.x).hypot(rest[b].1 - mid.y);
                    da.total_cmp(&db)
                })
                .unwrap();
            for (depth, down) in [(-0.3, false), (0.3, true)] {
                let d = Deform { dents: vec![Dent { u: 0.5, depth, width: 0.3 }], ..Default::default() };
                let bent = outline(Style::Notch, s, &d);
                let (dx, dy) = (bent[nearest].0 - rest[nearest].0, bent[nearest].1 - rest[nearest].1);
                // The lip's outward normal is straight down, so a dent moves it in y only.
                assert!(dx.abs() < 0.05 * h, "{w}x{h}: moved sideways by {dx}");
                assert!(dy.abs() > 0.2 * h, "{w}x{h}: depth {depth} moved only {dy}");
                assert_eq!(dy > 0.0, down, "{w}x{h}: depth {depth} went the wrong way ({dy})");
            }
        }
    }

    #[test]
    fn overlapping_dents_add_and_do_not_depend_on_their_order() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let a = Dent { u: 0.5, depth: -0.3, width: 0.4 };
        let b = Dent { u: 0.55, depth: 0.2, width: 0.4 };
        let run = |dents: Vec<Dent>| outline(Style::Notch, s, &Deform { dents, ..Default::default() });
        let (rest, only_a, only_b) = (run(vec![]), run(vec![a]), run(vec![b]));
        let (ab, ba) = (run(vec![a, b]), run(vec![b, a]));
        for i in 0..OUTLINE_PTS {
            let sum_y = rest[i].1 + (only_a[i].1 - rest[i].1) + (only_b[i].1 - rest[i].1);
            assert!((ab[i].1 - sum_y).abs() < 0.001, "point {i}: not the sum of the two dents");
            assert!((ab[i].1 - ba[i].1).abs() < 0.001, "point {i}: depends on the order");
        }
        assert!((0..OUTLINE_PTS).any(|i| only_a[i].1 != rest[i].1 && only_b[i].1 != rest[i].1), "they overlap");
    }

    #[test]
    fn each_style_swings_about_its_own_pivot() {
        for (w, h) in [(160.0, 28.0), (300.0, 44.0)] {
            for style in [Style::Notch, Style::Pill] {
                let s = shape(style, w, h);
                let rest = outline(style, s, &Deform::default());
                let swung = outline(style, s, &Deform { swing_deg: 10.0, ..Default::default() });
                // The Notch's extension adds one point at each end; the rest line up.
                let off = (swung.len() - OUTLINE_PTS) / 2;
                let (top, mid) = ((s.x + s.w / 2.0, s.y), (s.x + s.w / 2.0, s.y + s.h / 2.0));
                // A rotation keeps every point's distance to its pivot, and changes it about any other.
                let spread = |pv: (f32, f32)| {
                    (0..OUTLINE_PTS)
                        .map(|i| {
                            let r = (rest[i].0 - pv.0).hypot(rest[i].1 - pv.1);
                            let q = (swung[i + off].0 - pv.0).hypot(swung[i + off].1 - pv.1);
                            (r - q).abs()
                        })
                        .fold(0.0_f32, f32::max)
                };
                let (own, other) = if style == Style::Notch { (top, mid) } else { (mid, top) };
                assert!(spread(own) < 0.01, "{style:?} {w}x{h} does not turn about its own pivot");
                assert!(spread(other) > 1.0, "{style:?} {w}x{h} also turns about the other pivot");
            }
        }
    }

    #[test]
    fn a_swinging_notch_leaves_no_hole_at_the_screen_edge() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let pts = outline(Style::Notch, s, &Deform { swing_deg: 6.0, ..Default::default() });
        assert!(pts.iter().any(|p| p.1 < s.y - 0.5), "the top is extended behind the screen edge");
        // Interpolate where each segment actually crosses the screen edge; taking
        // the earlier point's x instead would measure the sampling, not the shape.
        let crossings: Vec<f32> = pts.windows(2)
            .filter(|w| (w[0].1 - s.y) * (w[1].1 - s.y) <= 0.0 && w[0].1 != w[1].1)
            .map(|w| w[0].0 + (s.y - w[0].1) / (w[1].1 - w[0].1) * (w[1].0 - w[0].0))
            .collect();
        assert!(crossings.iter().any(|&x| x <= s.x), "still covers the left end at the edge");
        assert!(crossings.iter().any(|&x| x >= s.x + s.w), "and the right end");
    }

    #[test]
    fn shift_and_tilt_are_pill_only() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let d = Deform { shift: (0.0, 6.0), tilt_deg: 4.0, ..Default::default() };
        assert_eq!(outline(Style::Notch, s, &d), outline(Style::Notch, s, &Deform::default()));
    }

    #[test]
    fn is_rest_is_true_only_for_an_untouched_deform() {
        assert!(is_rest(Style::Notch, &Deform::default()));
        assert!(!is_rest(Style::Notch, &Deform { squeeze: 0.02, ..Default::default() }));
        // shift and tilt do nothing on the Notch, so `is_rest` must agree with `outline`.
        assert!(is_rest(Style::Notch, &Deform { shift: (0.0, 6.0), tilt_deg: 4.0, ..Default::default() }));
        assert!(!is_rest(Style::Pill, &Deform { tilt_deg: 4.0, ..Default::default() }));
    }

    /// `m` applied to a point, as Direct2D does (row vectors).
    fn apply(m: &windows_numerics::Matrix3x2, (x, y): (f32, f32)) -> (f32, f32) {
        (x * m.M11 + y * m.M21 + m.M31, x * m.M12 + y * m.M22 + m.M32)
    }

    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 0.001 && (a.1 - b.1).abs() < 0.001
    }

    #[test]
    fn the_rigid_motion_keeps_each_styles_pivot_where_it_is() {
        for (w, h) in [(160.0, 28.0), (300.0, 44.0)] {
            let d = Deform { swing_deg: 7.0, shift: (3.0, 5.0), tilt_deg: 4.0, squeeze: 0.1, ..Default::default() };
            let (n, p) = (shape(Style::Notch, w, h), shape(Style::Pill, w, h));
            // The Notch turns about the middle of the screen edge and never shifts.
            let top = (n.x + n.w / 2.0, n.y);
            assert!(near(apply(&rigid(Style::Notch, n, &d), top), top), "{w}x{h} Notch");
            // The Pill turns about its centre, which only the shift moves.
            let mid = (p.x + p.w / 2.0, p.y + p.h / 2.0);
            let moved = apply(&rigid(Style::Pill, p, &d), mid);
            assert!(near(moved, (mid.0 + 3.0, mid.1 + 5.0)), "{w}x{h} Pill centre went to {moved:?}");
            // And it does turn: the Notch's corner leaves its place by the swing.
            let corner = (n.x, n.y + n.h);
            let swung = apply(&rigid(Style::Notch, n, &d), corner);
            let want = ((corner.0 - top.0) * 7.0_f32.to_radians().cos() - (corner.1 - top.1) * 7.0_f32.to_radians().sin() + top.0,
                (corner.0 - top.0) * 7.0_f32.to_radians().sin() + (corner.1 - top.1) * 7.0_f32.to_radians().cos() + top.1);
            assert!(near(swung, want), "{w}x{h}: {swung:?} is not {want:?}");
        }
    }

    #[test]
    fn a_rest_deform_has_no_rigid_motion_and_the_notch_ignores_shift_and_tilt() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let id = windows_numerics::Matrix3x2::identity();
        assert_eq!(rigid(Style::Notch, s, &Deform::default()), id);
        assert_eq!(rigid(Style::Pill, shape(Style::Pill, 190.0, 34.0), &Deform::default()), id);
        // A squeeze and a dent are not rigid.
        let soft = Deform { squeeze: 0.2, dents: vec![Dent { u: 0.5, depth: 0.3, width: 0.3 }], ..Default::default() };
        assert_eq!(rigid(Style::Notch, s, &soft), id);
        let d = Deform { shift: (0.0, 6.0), tilt_deg: 4.0, ..Default::default() };
        assert_eq!(rigid(Style::Notch, s, &d), id);
    }

    #[test]
    fn the_outline_is_the_squeezed_rim_under_the_rigid_motion() {
        // The clock draws under `rigid`; if `outline` placed its points any
        // other way the clock would slide off a swinging island.
        for style in [Style::Notch, Style::Pill] {
            let s = shape(style, 190.0, 34.0);
            let d = Deform { swing_deg: 5.0, shift: (2.0, 3.0), tilt_deg: 3.0, squeeze: 0.1, ..Default::default() };
            let (m, pts) = (rigid(style, s, &d), outline(style, s, &d));
            let off = (pts.len() - OUTLINE_PTS) / 2;
            let cx = s.x + s.w / 2.0;
            for i in 0..OUTLINE_PTS {
                let p = rim(style, s, i as f32 / (OUTLINE_PTS - 1) as f32);
                let squeezed = (cx + (p.x - cx) * 0.9, p.y);
                assert!(near(pts[i + off], apply(&m, squeezed)), "{style:?} point {i}");
            }
        }
    }

    const SIZES: [(f32, f32); 3] = [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)];

    #[test]
    fn a_left_bulge_pulls_the_left_end_in_and_leaves_the_right_half_alone() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let case = format!("{style:?} {w}x{h}");
                let rest = outline(style, s, &Deform::default());
                // Zero is no bulge at all.
                assert_eq!(outline(style, s, &Deform { bulge_left: 0.0, ..Default::default() }), rest, "{case}");
                let bent = outline(style, s, &Deform { bulge_left: -2.2, ..Default::default() });
                let cx = s.x + s.w / 2.0;
                for (b, r) in bent.iter().zip(&rest) {
                    if r.0 >= cx {
                        assert_eq!(b, r, "{case}: the right half moved");
                    }
                    assert!(b.1 == r.1 && b.0 >= r.0 - 1e-4 && b.0 <= r.0 + 2.2 + 1e-4, "{case}: {r:?} went to {b:?}");
                }
                // The end's point nearest the middle height goes in by the
                // whole bulge (the mockup's `bulgeL`, its bell at mid-height).
                let mid = s.y + s.h / 2.0;
                let i = (0..rest.len())
                    .filter(|&i| rest[i].0 < s.x + 1.0)
                    .min_by(|&a, &b| (rest[a].1 - mid).abs().total_cmp(&(rest[b].1 - mid).abs()))
                    .unwrap();
                let ramp = ((s.x + s.w / 4.0 - rest[i].0) / (s.w / 4.0)).min(1.0);
                let want = 2.2 * ramp * (-((rest[i].1 - mid) / (0.6 * s.h)).powi(2)).exp();
                assert!((bent[i].0 - rest[i].0 - want).abs() < 1e-3, "{case}: moved {} not {want}", bent[i].0 - rest[i].0);
                assert!(want > 1.5, "{case}: the sample is near the middle");
                assert!(!is_rest(style, &Deform { bulge_left: -2.2, ..Default::default() }), "{case}");
            }
        }
    }

    #[test]
    fn a_jolt_moves_the_island_alone_and_never_what_rides_it() {
        // The sneeze's jolt (the mockup's `dx`): the outline moves sideways in
        // both styles, the Notch along the screen edge; the clock draws under
        // `rigid`, which the jolt leaves alone, so the clock stays put.
        for style in [Style::Notch, Style::Pill] {
            let s = shape(style, 190.0, 34.0);
            let rest = outline(style, s, &Deform::default());
            let d = Deform { jolt: -1.6, ..Default::default() };
            for (m, r) in outline(style, s, &d).iter().zip(&rest) {
                assert!(near(*m, (r.0 - 1.6, r.1)), "{style:?}: {r:?} went to {m:?}");
            }
            assert_eq!(rigid(style, s, &d), windows_numerics::Matrix3x2::identity(), "{style:?}");
            assert!(!is_rest(style, &d), "{style:?}");
            // With a swing the outline still rides `rigid`, moved by the jolt.
            let swung = Deform { swing_deg: 3.0, jolt: 1.0, ..Default::default() };
            let plain = outline(style, s, &Deform { swing_deg: 3.0, ..Default::default() });
            for (m, r) in outline(style, s, &swung).iter().zip(&plain) {
                assert!(near(*m, (r.0 + 1.0, r.1)), "{style:?} swung: {r:?} went to {m:?}");
            }
        }
        // The Notch never takes the ordinary shift.
        assert!(is_rest(Style::Notch, &Deform { shift: (1.6, 5.0), ..Default::default() }));
    }

    /// The outline's left side at height `y`, between its two samples.
    fn left_side_at(pts: &[(f32, f32)], y: f32) -> f32 {
        let w = pts.windows(2).find(|w| (w[0].1 - y) * (w[1].1 - y) <= 0.0 && w[0].1 != w[1].1).unwrap();
        w[0].0 + (y - w[0].1) / (w[1].1 - w[0].1) * (w[1].0 - w[0].0)
    }

    #[test]
    fn the_shoulders_meet_the_body_where_its_sides_are() {
        for (w, h) in SIZES {
            let s = shape(Style::Notch, w, h);
            let foot = s.y + crate::ui::geometry::shoulder_depth(s);
            assert_eq!(top_corners(s, &Deform::default()), (s.x, s.x + s.w), "{w}x{h}");
            let squeezed = top_corners(s, &Deform { squeeze: 0.1, ..Default::default() });
            let cx = s.x + s.w / 2.0;
            assert!(near(squeezed, (cx - s.w * 0.45, cx + s.w * 0.45)), "{w}x{h}: {squeezed:?}");
            // A pulled-in left end and a jolt: the shoulder's foot is where
            // the body's side is at its height, so no ledge.
            let d = Deform { bulge_left: -2.2, jolt: 1.6, squeeze: 0.018, ..Default::default() };
            let (l, r) = top_corners(s, &d);
            let side = left_side_at(&outline(Style::Notch, s, &d), foot);
            assert!((l - side).abs() < 0.05, "{w}x{h}: shoulder at {l}, side at {side}");
            assert!((r - (cx + s.w / 2.0 * 0.982 + 1.6)).abs() < 1e-3, "{w}x{h}: {r}");
        }
    }

    #[test]
    fn a_swinging_notchs_shoulders_meet_its_sides_at_their_foot() {
        // The swing turns the body's sides about the screen edge's middle;
        // each shoulder's foot is where its side crosses the foot's height,
        // so the side that swings in leaves no ledge under its shoulder.
        for (w, h) in SIZES {
            let s = shape(Style::Notch, w, h);
            let foot = s.y + crate::ui::geometry::shoulder_depth(s);
            for swing_deg in [-12.0, -2.6, 2.6, 12.0] {
                for d in [
                    Deform { swing_deg, ..Default::default() },
                    Deform { swing_deg, bulge_left: -2.2, jolt: 1.6, squeeze: 0.018, ..Default::default() },
                ] {
                    let pts = outline(Style::Notch, s, &d);
                    let right: Vec<_> = pts.iter().rev().copied().collect();
                    let (l, r) = top_corners(s, &d);
                    let (side_l, side_r) = (left_side_at(&pts, foot), left_side_at(&right, foot));
                    assert!((l - side_l).abs() < 1e-3 && (r - side_r).abs() < 1e-3,
                        "{w}x{h} {d:?}: shoulders at {l}, {r}, sides at {side_l}, {side_r}");
                }
            }
        }
    }

    #[test]
    fn a_mockup_dent_is_as_deep_and_as_wide_as_it_says_where_it_says() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let case = format!("{style:?} {w}x{h}");
                let rest = outline(style, s, &Deform::default());
                for (x, wide, deep) in [(s.x + 40.0, 12.0, 1.8), (s.x + s.w / 2.0, 30.0, -2.5)] {
                    let d = Deform { dents: vec![lip_dent(style, s, x, wide, deep)], ..Default::default() };
                    let bent = outline(style, s, &d);
                    let (i, push) = bent.iter().zip(&rest).map(|(b, r)| b.1 - r.1).enumerate()
                        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap();
                    // Down for a positive depth, as the mockup's `y += d`.
                    assert!((push - deep).abs() < 0.08 * deep.abs(), "{case} x={x}: pushed {push}, not {deep}");
                    assert!((rest[i].0 - x).abs() < 3.0, "{case}: deepest at {} not {x}", rest[i].0);
                    // Gone two widths away on the lip.
                    for (b, r) in bent.iter().zip(&rest) {
                        if (r.0 - x).abs() > 2.0 * wide && r.1 > s.y + s.h - 0.5 {
                            assert!((b.1 - r.1).abs() < 1e-3, "{case}: {r:?} moved");
                        }
                    }
                }
                // On the rounded end the lip leaves the bottom, and the mockup's
                // weight (`bw`) gives the dent less of it.
                let corner = lip_dent(style, s, s.x + 4.0, 6.0, -1.2);
                let flat = lip_dent(style, s, s.x + 40.0, 6.0, -1.2);
                assert!(corner.depth < 0.0 && corner.depth > flat.depth, "{case}: {corner:?} {flat:?}");
                assert!(corner.u < flat.u && rim(style, s, corner.u).y > s.y + s.h / 2.0, "{case}: {corner:?}");
            }
        }
    }
}
