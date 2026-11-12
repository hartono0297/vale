//! The falk (cockatiel) mascot's shapes as pure data, on the 64-unit grid.
//! Painting lives elsewhere.

// Painted from Task 6 on.
#![allow(dead_code)]

use crate::theme::FalkColor;
use crate::ui::robot::{Expr, Pose, Seg};

/// The egg.
pub const FALK_BODY: &[Seg] = &[
    Seg::Move(9.0, 43.0),
    Seg::Quad(9.0, 22.0, 32.0, 22.0),
    Seg::Quad(55.0, 22.0, 55.0, 43.0),
    Seg::Quad(55.0, 57.0, 32.0, 57.0),
    Seg::Quad(9.0, 57.0, 9.0, 43.0),
    Seg::Close,
];

/// The crest's feathers: base, tip, half-width, sideways bend.
pub const CREST: [((f32, f32), (f32, f32), f32, f32); 4] = [
    ((28.0, 27.0), (27.0, 11.0), 2.8, -3.5),
    ((30.5, 26.0), (35.0, 3.0), 3.2, -5.0),
    ((33.0, 26.0), (42.0, 6.0), 3.2, -5.0),
    ((35.5, 27.0), (46.0, 14.0), 2.6, -3.5),
];

/// The crest rotates and scales round this point.
pub const CREST_PIVOT: (f32, f32) = (32.0, 27.0);

pub const FALK_CHEEKS: [(f32, f32); 2] = [(15.5, 44.5), (48.5, 44.5)];
pub const CHEEK_R: f32 = 4.4;
/// The face mask ellipse: cx, cy, rx, ry.
pub const FALK_FACE: (f32, f32, f32, f32) = (32.0, 33.0, 22.0, 17.0);
pub const FALK_BEAK: u32 = 0xD9978F;

/// A tapered quad leaf from `base` to `tip`: the normal of base to tip, the
/// mid point pushed by `bend` along it, `half_w` either side.
pub fn feather(base: (f32, f32), tip: (f32, f32), half_w: f32, bend: f32) -> Vec<Seg> {
    let (dx, dy) = (tip.0 - base.0, tip.1 - base.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let n = (-dy / len, dx / len);
    let mid = ((base.0 + tip.0) / 2.0 + n.0 * bend, (base.1 + tip.1) / 2.0 + n.1 * bend);
    vec![
        Seg::Move(base.0, base.1),
        Seg::Quad(mid.0 + n.0 * half_w, mid.1 + n.1 * half_w, tip.0, tip.1),
        Seg::Quad(mid.0 - n.0 * half_w, mid.1 - n.1 * half_w, base.0, base.1),
        Seg::Close,
    ]
}

/// The crest's turn (degrees clockwise round `CREST_PIVOT`) and scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrestPose {
    pub rot_deg: f32,
    pub scale: f32,
}

pub fn crest_pose(expr: Expr, pose: &Pose) -> CrestPose {
    let (rot_deg, scale) = match expr {
        Expr::Idle | Expr::Focus | Expr::Think | Expr::Typing => (10.0, 1.0),
        Expr::Happy => (2.0, 1.08),
        Expr::Music | Expr::Sing => {
            let sway = if pose.dancing { 9.0 * pose.beat.sin() } else { 0.0 };
            (6.0 + sway, 1.0)
        }
        Expr::Alert => (-4.0, 1.22),
        Expr::Sleep => (62.0, 0.8),
        Expr::Sad => (46.0, 0.75),
        Expr::Wave => (10.0 + if pose.waving { 10.0 * pose.wave } else { 0.0 }, 1.0),
    };
    CrestPose { rot_deg, scale }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FalkInk {
    pub body: u32,
    pub face: u32,
    pub crest: u32,
    pub cheek: Option<u32>,
}

pub fn falk_ink(c: FalkColor) -> FalkInk {
    match c {
        FalkColor::Lutino => FalkInk { body: 0xF1E4A2, face: 0xF6D04A, crest: 0xF6D04A, cheek: Some(0xEC6B3A) },
        FalkColor::Grey => FalkInk { body: 0x9A959C, face: 0xF2D35A, crest: 0xE9CF62, cheek: Some(0xEC6B3A) },
        FalkColor::WhiteFace => FalkInk { body: 0x9A959C, face: 0xF4EFE6, crest: 0xE4DED6, cheek: None },
    }
}

/// The beak: closed is one path, open is the upper and the lower.
pub fn falk_beak(open: bool) -> Vec<Vec<Seg>> {
    if open {
        vec![
            vec![
                Seg::Move(29.4, 43.8),
                Seg::Quad(32.0, 42.0, 34.6, 43.8),
                Seg::Quad(34.2, 46.8, 32.0, 47.6),
                Seg::Quad(29.8, 46.8, 29.4, 43.8),
                Seg::Close,
            ],
            vec![
                Seg::Move(30.4, 49.2),
                Seg::Quad(32.0, 48.6, 33.6, 49.2),
                Seg::Quad(33.0, 51.2, 32.0, 51.4),
                Seg::Quad(31.0, 51.2, 30.4, 49.2),
                Seg::Close,
            ],
        ]
    } else {
        vec![vec![
            Seg::Move(29.4, 44.2),
            Seg::Quad(32.0, 42.4, 34.6, 44.2),
            Seg::Quad(34.4, 48.4, 32.0, 49.8),
            Seg::Quad(29.6, 48.4, 29.4, 44.2),
            Seg::Close,
        ]]
    }
}

pub fn beak_open(expr: Expr, pose: &Pose) -> bool {
    match expr {
        Expr::Sing | Expr::Alert => true,
        Expr::Music => pose.dancing,
        _ => false,
    }
}

/// The typing feather tip at (x, y).
pub fn feather_tip(x: f32, y: f32) -> Vec<Seg> {
    vec![
        Seg::Move(x - 4.0, y + 1.0),
        Seg::Quad(x - 3.5, y - 2.6, x, y - 2.6),
        Seg::Quad(x + 4.4, y - 2.4, x + 4.0, y + 1.2),
        Seg::Quad(x, y + 2.4, x - 4.0, y + 1.0),
        Seg::Close,
    ]
}

fn sample(segs: &[Seg], steps: usize, out: &mut Vec<(f32, f32)>) {
    let mut cur = (0.0, 0.0);
    for s in segs {
        match *s {
            Seg::Move(x, y) | Seg::Line(x, y) => {
                cur = (x, y);
                out.push(cur);
            }
            Seg::Quad(cx, cy, x, y) => {
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    let u = 1.0 - t;
                    out.push((
                        u * u * cur.0 + 2.0 * u * t * cx + t * t * x,
                        u * u * cur.1 + 2.0 * u * t * cy + t * t * y,
                    ));
                }
                cur = (x, y);
            }
            Seg::Cubic(c1x, c1y, c2x, c2y, x, y) => {
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    let u = 1.0 - t;
                    out.push((
                        u * u * u * cur.0 + 3.0 * u * u * t * c1x + 3.0 * u * t * t * c2x + t * t * t * x,
                        u * u * u * cur.1 + 3.0 * u * u * t * c1y + 3.0 * u * t * t * c2y + t * t * t * y,
                    ));
                }
                cur = (x, y);
            }
            Seg::Close => {}
        }
    }
}

/// Grid points of the body and of every crest feather (its half-width
/// included) after the crest's pose; the clamp tests read it.
pub fn falk_outline(expr: Expr, pose: &Pose) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    sample(FALK_BODY, 16, &mut out);
    let cp = crest_pose(expr, pose);
    let (sin, cos) = cp.rot_deg.to_radians().sin_cos();
    let mut crest = Vec::new();
    for &(base, tip, w, bend) in &CREST {
        sample(&feather(base, tip, w, bend), 12, &mut crest);
    }
    for (x, y) in crest {
        let (dx, dy) = ((x - CREST_PIVOT.0) * cp.scale, (y - CREST_PIVOT.1) * cp.scale);
        out.push((CREST_PIVOT.0 + dx * cos - dy * sin, CREST_PIVOT.1 + dx * sin + dy * cos));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crest_carries_the_mood() {
        let p = Pose::default();
        let c = |e| crest_pose(e, &p);
        assert_eq!((c(Expr::Idle).rot_deg, c(Expr::Idle).scale), (10.0, 1.0));
        assert_eq!((c(Expr::Happy).rot_deg, c(Expr::Happy).scale), (2.0, 1.08));
        assert_eq!((c(Expr::Alert).rot_deg, c(Expr::Alert).scale), (-4.0, 1.22));
        assert_eq!((c(Expr::Sleep).rot_deg, c(Expr::Sleep).scale), (62.0, 0.8));
        assert_eq!((c(Expr::Sad).rot_deg, c(Expr::Sad).scale), (46.0, 0.75));
        for e in [Expr::Focus, Expr::Think, Expr::Typing] { assert_eq!(c(e).rot_deg, 10.0); }
        assert_eq!(c(Expr::Music).rot_deg, 6.0); // at rest, not dancing
        let dancing = Pose { dancing: true, beat: std::f32::consts::FRAC_PI_2, ..Pose::default() };
        assert!((crest_pose(Expr::Music, &dancing).rot_deg - 15.0).abs() < 1e-4); // 6 + 9·sin
        let waving = Pose { waving: true, wave: -1.0, ..Pose::default() };
        assert!((crest_pose(Expr::Wave, &waving).rot_deg - 0.0).abs() < 1e-4); // 10 + 10·wave
    }

    #[test]
    fn falk_coats_are_the_specs() {
        assert_eq!(falk_ink(FalkColor::Lutino), FalkInk { body: 0xF1E4A2, face: 0xF6D04A, crest: 0xF6D04A, cheek: Some(0xEC6B3A) });
        assert_eq!(falk_ink(FalkColor::Grey), FalkInk { body: 0x9A959C, face: 0xF2D35A, crest: 0xE9CF62, cheek: Some(0xEC6B3A) });
        assert_eq!(falk_ink(FalkColor::WhiteFace), FalkInk { body: 0x9A959C, face: 0xF4EFE6, crest: 0xE4DED6, cheek: None });
    }

    #[test]
    fn the_falk_outline_holds_the_crest_tip_and_the_body() {
        let o = falk_outline(Expr::Idle, &Pose::default());
        let top = o.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let bottom = o.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        assert!(top < 8.0, "the crest reaches near the grid top, got {top}");
        assert!((bottom - 57.0).abs() < 0.5);
        // the Alert crest is the tallest
        let alert = falk_outline(Expr::Alert, &Pose::default()).iter().map(|p| p.1).fold(f32::MAX, f32::min);
        assert!(alert < top);
    }

    #[test]
    fn the_beak_opens_to_sing_and_on_alert() {
        assert!(beak_open(Expr::Sing, &Pose::default()));
        assert!(beak_open(Expr::Alert, &Pose::default()));
        assert!(!beak_open(Expr::Music, &Pose::default()));
        assert!(beak_open(Expr::Music, &Pose { dancing: true, ..Pose::default() }));
        assert_eq!(falk_beak(false).len(), 1);
        assert_eq!(falk_beak(true).len(), 2);
    }
}
