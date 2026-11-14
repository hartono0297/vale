//! The octopus's long arms and its mantle placed, ported from
//! `docs/design/maskot-notch-v3.html` (`octCols`, `OROOT`, `rootAt`,
//! `octArm`, and the `place(..., mantle(...), [32, 30])` the stories draw it
//! with). Pure: a centre line and a coat in, `Draw`s out, in the units of
//! the line (window dip, for a story that builds it from `rim`).
//!
//! When a story draws these arms, its head is the mantle alone (`mantle`),
//! not the sitting octopus with its six short arms: the app's own painter,
//! with `Pose::bare_octopus`, so the coat, the face and the accessory come
//! along and the arms do not double.
//!
//! The colours are the head's own (`mascot::octopus_rgb`, `shade`,
//! `sucker_rgb`), so every coat matches it exactly. The mockup's `octCols`
//! differ a little: its shade is the coat 20 % toward black where the app's
//! `shade` is 18 % (0.82 of its brightness), and its suckers are 58 % toward
//! white where the app's are 55 %. The flush (the mockup mixes the whole
//! coat toward #D2304C) is not here: the app flushes the mantle toward
//! #D63A55 through the pose and keeps the spots and suckers unflushed.

use crate::theme::OctopusColor;
use crate::ui::geometry::ROBOT_COMPACT;
use crate::ui::mascot::{octopus_rgb, shade, sucker_rgb};
use crate::ui::play::art::tube::{LimbStyle, Suckers, limb};
use crate::ui::play::stories::{Draw, HEAD_PIVOT, MascotShot};
use crate::ui::robot::GRID;

/// The mantle's pivot on the head's 64-unit grid: the point the mockup's
/// `place(..., mantle(...), [32, 30])` puts at the story's coordinates.
/// `Draw::Head` pivots at `HEAD_PIVOT` (32, 40), ten units lower.
pub const MANTLE_PIVOT: (f32, f32) = (32.0, 30.0);

/// Where the six arms leave the mantle, left to right, from `MANTLE_PIVOT`:
/// the mockup's `OROOT`, in dip at the mockup's 0.4 per grid unit.
const OROOT: [(f32, f32); 6] = [(-5.2, 4.9), (-3.3, 6.0), (-1.2, 6.4), (1.2, 6.4), (3.3, 6.0), (5.2, 4.9)];

/// The mockup's `octArm`: a long arm about the centre line `path` (root
/// first), 4.3 wide at the root narrowing to 0.9 at the tip (along `s^0.75`)
/// times `wm`, with suckers every 2.3 back from the tip from 1.5, on `side`
/// (+1 or -1) of the arm; the volume band takes the other. Nothing for a
/// path shorter than 0.6.
pub fn arm(path: &[(f32, f32)], coat: OctopusColor, side: i8, wm: f32) -> Vec<Draw> {
    let col = octopus_rgb(coat);
    let style = LimbStyle {
        w0: 4.3,
        w1: 0.9,
        wm,
        pow: 0.75,
        side_shade: Some((-side, 0.34)),
        suckers: Some(Suckers { start: 1.5, every: 2.3, side: f32::from(side), rgb: sucker_rgb(col), inner: shade(col) }),
        ..LimbStyle::default()
    };
    limb(path, &style, col)
}

/// The mockup's `rootAt`: where arm `i` (0 to 5, left to right) leaves a
/// mantle whose pivot is at `mantle_at`, turned `rot_deg` (clockwise) and
/// scaled `sx`, `sy` as the story places it (`mantle`): its offset from the
/// pivot scaled in the mantle's own axes, then turned.
pub fn root(mantle_at: (f32, f32), i: usize, rot_deg: f32, sx: f32, sy: f32) -> (f32, f32) {
    let (x, y) = (OROOT[i].0 * sx, OROOT[i].1 * sy);
    let (sin, cos) = rot_deg.to_radians().sin_cos();
    (mantle_at.0 + x * cos - y * sin, mantle_at.1 + x * sin + y * cos)
}

/// The mockup's `place(x, y, r, sx, sy, mantle(...), [32, 30])`: the mantle
/// alone, with grid (32, 30) at `at` (window dip), turned `rot_deg`
/// (clockwise) and scaled `sx`, `sy` about it, at the slot head's size, by
/// the app's own painter. `face` is the story's face (expression, blink,
/// glance); its own placement (`dx`, `dy`, turn, scale) is replaced.
///
/// The conversion: `Draw::Head` turns and scales about `HEAD_PIVOT`, ten grid
/// units below `MANTLE_PIVOT`, so its `at` is `at` moved that far down the
/// mantle's own axis: `at + R(rot_deg, (0, 10 * unit * sy))`, `unit` being
/// `ROBOT_COMPACT / GRID`. (The mockup's own unit is 0.4, the slot's 26 / 64.)
pub fn mantle(at: (f32, f32), rot_deg: f32, sx: f32, sy: f32, face: &MascotShot) -> Draw {
    let unit = ROBOT_COMPACT / GRID;
    let down = (HEAD_PIVOT.1 - MANTLE_PIVOT.1) * unit * sy;
    let (sin, cos) = rot_deg.to_radians().sin_cos();
    Draw::Head {
        at: (at.0 - down * sin, at.1 + down * cos),
        shot: MascotShot { dx: 0.0, dy: 0.0, rot_deg, sx, sy, bare_octopus: true, ..*face },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::OctopusColor;
    use crate::ui::mascot::{CrestPose, octopus_rgb, shade, sucker_rgb};
    use crate::ui::play::stories::head_matrix;
    use crate::ui::robot::{Expr, Seg};

    fn line(n: usize, len: f32) -> Vec<(f32, f32)> {
        (0..n).map(|i| (len * i as f32 / (n - 1) as f32, 0.0)).collect()
    }

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
        }
    }

    /// Half the body's width at each point of an arm along +x: how far its
    /// left edge lies below the centre line. The right edge ends the rounded
    /// tip at the same distance above it, which is checked too.
    fn half_widths(d: &[Draw]) -> Vec<f32> {
        let Some(Draw::Fill { path, .. }) = d.first() else {
            panic!("no body: {d:?}");
        };
        let tip = path.iter().position(|s| matches!(s, Seg::Cubic(..))).expect("a rounded tip");
        let half: Vec<f32> = path[..tip]
            .iter()
            .map(|s| match *s {
                Seg::Move(_, y) | Seg::Line(_, y) | Seg::Quad(_, y, _, _) => y,
                other => panic!("{other:?}"),
            })
            .collect();
        let Seg::Cubic(.., right) = path[tip] else {
            unreachable!()
        };
        assert!((right + half[half.len() - 1]).abs() < 1e-4, "the tip is round: {right} {half:?}");
        half
    }

    fn rings(d: &[Draw], rgb: u32) -> Vec<(f32, f32)> {
        d.iter()
            .filter_map(|x| match x {
                Draw::Ellipse { c, rgb: v, .. } if *v == rgb => Some(*c),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_arm_tapers_from_root_to_tip() {
        let p = line(41, 20.0);
        let half = half_widths(&arm(&p, OctopusColor::Coral, 1, 1.0));
        // 4.3 wide at the root, 0.9 at the tip, narrowing all the way between.
        assert!((half[0] * 2.0 - 4.3).abs() < 1e-3, "{half:?}");
        assert!((half[half.len() - 1] * 2.0 - 0.9).abs() < 1e-3, "{half:?}");
        assert!(half.windows(2).all(|w| w[1] < w[0]), "{half:?}");
        // `wm` scales it whole.
        let slim = half_widths(&arm(&p, OctopusColor::Coral, 1, 0.5));
        assert!((slim[0] * 2.0 - 2.15).abs() < 1e-3 && (slim[slim.len() - 1] * 2.0 - 0.45).abs() < 1e-3, "{slim:?}");
        // Nothing is built from a path too short to be an arm.
        assert!(arm(&[(0.0, 0.0), (0.2, 0.0)], OctopusColor::Coral, 1, 1.0).is_empty());
    }

    #[test]
    fn suckers_sit_on_the_inner_side() {
        let p = line(201, 20.0);
        let coat = octopus_rgb(OctopusColor::Blue);
        // Heading right, the normal points down: side +1 puts the suckers
        // below the centre line, side -1 above; the volume band takes the
        // other side of the arm.
        for (side, below) in [(1i8, true), (-1, false)] {
            let d = arm(&p, OctopusColor::Blue, side, 1.0);
            let (rims, dots) = (rings(&d, sucker_rgb(coat)), rings(&d, shade(coat)));
            assert_eq!(rims.len(), 7, "{rims:?}");
            assert_eq!(dots, rims, "each sucker has its dot at its centre");
            assert!(rims.iter().all(|c| (c.1 > 0.0) == below && c.1 != 0.0), "side {side}: {rims:?}");
            // One every 2.3 back from the tip, the first at 1.5 (too thin to
            // show, so 3.8 is the first drawn).
            for w in rims.windows(2) {
                assert!((w[0].0 - w[1].0 - 2.3).abs() < 0.11, "{rims:?}");
            }
            assert!((rims[0].0 - (20.0 - 1.5 - 2.3)).abs() < 0.11, "{rims:?}");
            let Some(Draw::Tint { path, rgb, alpha }) = d.last() else {
                panic!("no band: {:?}", d.last());
            };
            assert_eq!((*rgb, *alpha), (0, 0.17));
            let ys = path.iter().flat_map(|s| match *s {
                Seg::Move(_, y) | Seg::Line(_, y) => vec![y],
                Seg::Quad(_, a, _, b) => vec![a, b],
                _ => vec![],
            });
            for y in ys {
                assert!((y < 0.0) == below, "side {side}: the band lies on the other side: {y}");
            }
        }
    }

    #[test]
    fn every_coat_colours_the_arm_like_the_head() {
        let mut bodies = Vec::new();
        for coat in OctopusColor::ALL {
            let d = arm(&line(41, 20.0), *coat, 1, 1.0);
            let base = octopus_rgb(*coat);
            assert!(matches!(d[0], Draw::Fill { rgb, .. } if rgb == base), "{coat:?}");
            assert!(!rings(&d, sucker_rgb(base)).is_empty(), "{coat:?}: rims");
            assert!(!rings(&d, shade(base)).is_empty(), "{coat:?}: dots");
            bodies.push(base);
        }
        assert_eq!(bodies, [0xE87C8E, 0xD9505F, 0xA98BD6, 0x5FA8C9]);
        // The head's own tints: the coat at 0.82 of its brightness, 55 % toward white.
        assert_eq!((shade(0xE87C8E), sucker_rgb(0xE87C8E)), (0xBE6674, 0xF5C4CC));
    }

    #[test]
    fn the_roots_follow_the_mantle_turn() {
        let m = (100.0, 50.0);
        let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4;
        // The mockup's `OROOT`, left to right, from the mantle's pivot.
        let rest = [(-5.2, 4.9), (-3.3, 6.0), (-1.2, 6.4), (1.2, 6.4), (3.3, 6.0), (5.2, 4.9)];
        for (i, (x, y)) in rest.iter().enumerate() {
            assert!(near(root(m, i, 0.0, 1.0, 1.0), (m.0 + x, m.1 + y)), "{i}");
            // Scaled in its own axes first, then turned.
            assert!(near(root(m, i, 0.0, 2.0, 0.5), (m.0 + 2.0 * x, m.1 + 0.5 * y)), "{i}");
            // A quarter turn clockwise (y down) sends (x, y) to (-y, x).
            assert!(near(root(m, i, 90.0, 1.0, 1.0), (m.0 - y, m.1 + x)), "{i}");
            assert!(near(root(m, i, 180.0, 1.0, 1.0), (m.0 - x, m.1 - y)), "{i}");
            assert!(near(root(m, i, 90.0, 2.0, 0.5), (m.0 - 0.5 * y, m.1 + 2.0 * x)), "{i}");
        }
    }

    #[test]
    fn the_mantle_is_placed_by_its_own_pivot() {
        // The mockup's `place(x, y, r, sx, sy, mantle, [32, 30])`: grid
        // (32, 30) lands on (x, y) whatever the turn and scale.
        let unit = ROBOT_COMPACT / GRID;
        for (r, sx, sy) in [(0.0, 1.0, 1.0), (30.0, 1.0, 1.0), (-18.0, 0.9, 1.2), (90.0, 1.5, 0.7)] {
            let at = (210.0, 33.0);
            let Draw::Head { at: pivot, shot } = mantle(at, r, sx, sy, &face()) else {
                panic!("not a head");
            };
            assert!(shot.bare_octopus, "the mantle alone");
            assert_eq!((shot.dx, shot.dy, shot.rot_deg, shot.sx, shot.sy), (0.0, 0.0, r, sx, sy));
            // Grid (32, 30) lies 10 units above the head's pivot (32, 40),
            // before the head's own turn and scale.
            let p = (pivot.0, pivot.1 - 10.0 * unit);
            let m = head_matrix(pivot, &shot);
            let on = (p.0 * m.M11 + p.1 * m.M21 + m.M31, p.0 * m.M12 + p.1 * m.M22 + m.M32);
            assert!((on.0 - at.0).abs() < 1e-3 && (on.1 - at.1).abs() < 1e-3, "{r} {sx} {sy}: {on:?}");
        }
        // The face the story chose is kept; what moves a slot head is not.
        let shot = MascotShot { expr: Expr::Alert, blink: true, glance: (-2.4, 0.5), dx: 9.0, ..face() };
        let Draw::Head { shot: kept, .. } = mantle((0.0, 0.0), 0.0, 1.0, 1.0, &shot) else {
            panic!("not a head");
        };
        assert_eq!((kept.expr, kept.blink, kept.glance, kept.dx), (Expr::Alert, true, (-2.4, 0.5), 0.0));
    }
}
