//! The falk (cockatiel) and the sitting octopus: their shapes as pure data on
//! the 64-unit grid, and their painting (`RobotPainter::paint` calls it in
//! place of the cat's rim, silhouette and calico patches).
//!
//! The falk is painted crest group (base and four feathers, turned and scaled
//! by `crest_pose`), egg, face mask clipped to the egg, cheeks; then, after
//! the cat's face marks, its beak. The octopus is painted six arms, mantle,
//! spots, suckers. Every still path is built once and kept
//! (`RobotPainter::mascot_path`); only the octopus's moving arms (dance,
//! greeting, typing) are built per frame while they move.

use windows::Win32::Graphics::Direct2D::{
    D2D1_COMBINE_MODE_INTERSECT, D2D1_ELLIPSE, ID2D1Geometry, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::core::{Interface, Result as WinResult};
use windows_numerics::{Matrix3x2, Vector2};

use crate::theme::{FalkColor, Mascot, OctopusColor};
use crate::ui::cat_style::solid;
use crate::ui::geometry::Rect;
use crate::ui::robot::{Expr, GRID, HEAD, PAWS, Pose, RobotPainter, Seg, head_rect, path};

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
/// The crest's base ellipse, under the feathers: cx, cy, rx, ry.
pub const CREST_BASE: (f32, f32, f32, f32) = (32.0, 26.0, 5.5, 4.0);

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
    if let Some(story) = pose.crest {
        return story;
    }
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
    if let Some(story) = pose.beak_open {
        return story;
    }
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

// ---------- the sitting octopus ----------

/// The mantle (the head), behind nothing: the arms are drawn first.
pub const MANTLE: &[Seg] = &[
    Seg::Move(32.0, 9.0),
    Seg::Cubic(50.0, 9.0, 55.0, 25.0, 50.0, 37.0),
    Seg::Quad(47.0, 45.0, 40.0, 46.0),
    Seg::Line(24.0, 46.0),
    Seg::Quad(17.0, 45.0, 14.0, 37.0),
    Seg::Cubic(9.0, 25.0, 14.0, 9.0, 32.0, 9.0),
    Seg::Close,
];

/// Shade spots on the mantle: cx, cy, r.
pub const OCT_SPOTS: [(f32, f32, f32); 3] = [(22.0, 20.0, 2.2), (40.0, 15.0, 1.6), (45.0, 24.0, 1.3)];
/// The face's marks sit this much higher than the cat's.
pub const OCT_EYE_LIFT: f32 = -3.0;
pub const ARM_W: f32 = 5.0;
pub const SUCKER_R: f32 = 0.85;
pub const FLUSH_RGB: u32 = 0xD63A55;

/// Left arms, outer to inner: start, control, end.
const ARM_REST: [((f32, f32), (f32, f32), (f32, f32)); 3] = [
    ((19.0, 42.0), (8.0, 51.0), (3.0, 54.0)),
    ((24.0, 45.0), (15.0, 57.0), (9.5, 57.5)),
    ((29.0, 46.0), (27.0, 58.0), (21.0, 58.5)),
];

/// One arm: a quad from `start` through `ctrl` to `end`, then a curl of
/// `curl` times its size toward `curl_side` (-1 left, 1 right).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arm {
    pub start: (f32, f32),
    pub ctrl: (f32, f32),
    pub end: (f32, f32),
    pub curl_side: f32,
    pub curl: f32,
}

/// The six arms: 0..2 left outer to inner, 3..5 right outer to inner.
pub fn arms(expr: Expr, pose: &Pose) -> [Arm; 6] {
    let mut out = [Arm { start: (0.0, 0.0), ctrl: (0.0, 0.0), end: (0.0, 0.0), curl_side: 1.0, curl: 1.0 }; 6];
    for side_idx in 0..2 {
        let side: f32 = if side_idx == 0 { -1.0 } else { 1.0 };
        let mx = |v: f32| if side_idx == 0 { v } else { 64.0 - v };
        for (i, &(s, c, e)) in ARM_REST.iter().enumerate() {
            let mut ctrl = (mx(c.0), c.1);
            let mut end = (mx(e.0), e.1);
            let mut curl = 1.0;
            let mut o = side;
            if i == 2 {
                o = -o;
            }
            match expr {
                Expr::Happy => {
                    end.1 -= 3.0;
                    curl = 1.25;
                    // The outer arm's curl would reach the clock beside the
                    // robot when it looks right; pull its end in a little.
                    if i == 0 {
                        end.0 -= side * 0.4;
                    }
                }
                Expr::Alert => {
                    end.0 += side * 2.5;
                    end.1 -= 2.5;
                    curl = 1.3;
                }
                Expr::Sad => {
                    curl = 0.4;
                    end.1 += 1.0;
                }
                Expr::Sleep => {
                    curl = 1.8;
                    end = (mx(e.0 + 4.0), e.1 - 1.0);
                }
                _ => {}
            }
            if pose.dancing && matches!(expr, Expr::Music | Expr::Sing) {
                // beat = pi * t / BEAT_MS, so this is t / 190 ms.
                let t_190 = pose.beat * crate::ui::robot::BEAT_MS as f32 / (std::f32::consts::PI * 190.0);
                let w = (t_190 + i as f32 * 1.4 + side_idx as f32).sin();
                end.0 += w * 2.0;
                end.1 -= w.max(0.0) * 2.5;
            }
            if expr == Expr::Wave && side_idx == 1 && i == 0 {
                let w = if pose.waving { pose.wave } else { 0.0 };
                ctrl = (58.0, 40.0);
                end = (60.0 + w * 1.5, 28.0 + w * 3.0);
                curl = 1.1;
            }
            if expr == Expr::Typing && side_idx == 0 && i < 2 {
                ctrl = (mx(s.0) - 8.0, 50.0);
                end = (if i == 0 { 1.0 } else { 9.5 }, 53.0 - pose.paw_lift[i]);
                curl = 0.3;
            }
            out[side_idx * 3 + i] = Arm { start: (mx(s.0), s.1), ctrl, end, curl_side: o, curl };
        }
    }
    out
}

/// The arm's quad, then its two curl quads.
pub fn arm_segs(a: &Arm) -> Vec<Seg> {
    let (o, k) = (a.curl_side, a.curl);
    let e = a.end;
    let m = (e.0 + o * 3.2 * k, e.1 - 3.2 * k);
    vec![
        Seg::Move(a.start.0, a.start.1),
        Seg::Quad(a.ctrl.0, a.ctrl.1, e.0, e.1),
        Seg::Quad(e.0 + o * 3.4 * k, e.1 + 0.2, m.0, m.1),
        Seg::Quad(m.0 - 0.2, m.1 - 2.4 * k, m.0 - o * 2.4 * k, m.1 - 2.2 * k),
    ]
}

/// Two suckers along the arm's quad (t 0.55 and 0.8), 1.6 below it.
pub fn suckers(a: &Arm) -> [(f32, f32); 2] {
    let at = |t: f32| {
        let u = 1.0 - t;
        (
            u * u * a.start.0 + 2.0 * u * t * a.ctrl.0 + t * t * a.end.0,
            u * u * a.start.1 + 2.0 * u * t * a.ctrl.1 + t * t * a.end.1 + 1.6,
        )
    };
    [at(0.55), at(0.8)]
}

pub fn octopus_rgb(c: OctopusColor) -> u32 {
    match c {
        OctopusColor::Coral => 0xE87C8E,
        OctopusColor::Red => 0xD9505F,
        OctopusColor::Purple => 0xA98BD6,
        OctopusColor::Blue => 0x5FA8C9,
    }
}

/// Channel-wise blend of `a` toward `b` by `t` (0 is `a`, 1 is `b`).
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |sh: u32| {
        let (x, y) = (((a >> sh) & 0xFF) as f32, ((b >> sh) & 0xFF) as f32);
        ((x + (y - x) * t).round().clamp(0.0, 255.0) as u32) << sh
    };
    ch(16) | ch(8) | ch(0)
}

/// The coat at 0.82 of its brightness (spots).
pub fn shade(rgb: u32) -> u32 {
    mix(0, rgb, 0.82)
}

/// The coat 55 % toward white (suckers).
pub fn sucker_rgb(rgb: u32) -> u32 {
    mix(rgb, 0xFFFFFF, 0.55)
}

/// Grid points of the mantle, every arm (quad and curl, with round caps of
/// `ARM_W / 2`) and the suckers; the clamp tests read it.
#[cfg_attr(not(test), allow(dead_code))]
pub fn octopus_outline(expr: Expr, pose: &Pose) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    sample(MANTLE, 16, &mut out);
    let ring = |c: (f32, f32), r: f32, out: &mut Vec<(f32, f32)>| {
        for k in 0..8 {
            let g = k as f32 * std::f32::consts::FRAC_PI_4;
            out.push((c.0 + r * g.cos(), c.1 + r * g.sin()));
        }
    };
    for a in &arms(expr, pose) {
        let mut pts = Vec::new();
        sample(&arm_segs(a), 12, &mut pts);
        for p in pts {
            ring(p, ARM_W / 2.0, &mut out);
        }
        for s in suckers(a) {
            ring(s, SUCKER_R, &mut out);
        }
    }
    out
}

// ---------- the halo ----------

/// The box the see-through halo covers for mascot `m` drawn in `area` (a
/// square), in px, and its corner radius (the cat head's, scaled). The cat:
/// its head (`head_rect`). The falk: the egg's width, from the top of its
/// crest for `expr` and `pose` down to the egg's base. The octopus: the
/// mantle and the arms with their curls, x -3..67, y 9..61.
pub fn mascot_box(m: Mascot, area: Rect, expr: Expr, pose: &Pose) -> (Rect, f32) {
    let k = area.w / GRID;
    let (x0, y0, x1, y1) = match m {
        Mascot::Cat => return head_rect(area),
        Mascot::Falk => {
            let top = falk_outline(expr, pose).iter().map(|p| p.1).fold(f32::MAX, f32::min);
            (9.0, top, 55.0, 57.0)
        }
        Mascot::Octopus => (-3.0, 9.0, 67.0, 61.0),
    };
    (
        Rect {
            x: area.x + x0 * k,
            y: area.y + y0 * k,
            w: (x1 - x0) * k,
            h: (y1 - y0) * k,
        },
        HEAD.4 * k,
    )
}

// ---------- painting ----------

/// A falk or octopus path the painter builds once and keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum MascotShape {
    FalkBody,
    /// `CREST[i]`'s feather.
    Feather(usize),
    /// The face mask ellipse clipped to the egg.
    FaceOnBody,
    /// The beak, closed or open (both halves in one path).
    Beak(bool),
    /// A typing feather tip at (0, 0); it is moved to each key.
    Tip,
    Mantle,
    /// Arm `i` of `expr` at rest (`arms(expr, &Pose::default())`).
    Arm(Expr, usize),
}

impl MascotShape {
    fn segments(self) -> Vec<Seg> {
        match self {
            MascotShape::FalkBody | MascotShape::FaceOnBody => FALK_BODY.to_vec(),
            MascotShape::Feather(i) => {
                let (base, tip, w, bend) = CREST[i];
                feather(base, tip, w, bend)
            }
            MascotShape::Beak(open) => falk_beak(open).concat(),
            MascotShape::Tip => feather_tip(0.0, 0.0),
            MascotShape::Mantle => MANTLE.to_vec(),
            MascotShape::Arm(expr, i) => arm_segs(&arms(expr, &Pose::default())[i]),
        }
    }
}

fn ellipse((cx, cy, rx, ry): (f32, f32, f32, f32)) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: Vector2::new(cx, cy),
        radiusX: rx,
        radiusY: ry,
    }
}

impl RobotPainter {
    /// `shape`'s geometry, built the first time it is asked for.
    fn mascot_path(&self, shape: MascotShape) -> Option<ID2D1Geometry> {
        if let Some((_, g)) = self.mascot_paths.borrow().iter().find(|(s, _)| *s == shape) {
            return Some(g.clone());
        }
        let built = (|| -> WinResult<ID2D1Geometry> {
            let g: ID2D1Geometry = path(&self.factory, &shape.segments())?.cast()?;
            if shape != MascotShape::FaceOnBody {
                return Ok(g);
            }
            let face: ID2D1Geometry =
                unsafe { self.factory.CreateEllipseGeometry(&ellipse(FALK_FACE))? }.cast()?;
            self.combine(&face, &g, D2D1_COMBINE_MODE_INTERSECT)
        })()
        .ok()?;
        self.mascot_paths.borrow_mut().push((shape, built.clone()));
        Some(built)
    }

    /// The falk in `world`, under its face marks: the crest group, the egg,
    /// the face mask on the egg, and the cheeks in `cheeks` (the head plus
    /// half the eyes' slide).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_falk_body(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        coat: FalkColor,
        expr: Expr,
        pose: &Pose,
        (world, cheeks): (&Matrix3x2, &Matrix3x2),
        alpha: f32,
    ) {
        let ink = falk_ink(coat);
        let cp = crest_pose(expr, pose);
        let (px, py) = CREST_PIVOT;
        let crest = Matrix3x2::translation(-px, -py)
            * Matrix3x2::scale(cp.scale, cp.scale)
            * Matrix3x2::translation(px, py)
            * Matrix3x2::rotation_around(cp.rot_deg, Vector2::new(px, py))
            * *world;
        unsafe {
            rt.SetTransform(&crest);
            brush.SetColor(&solid(ink.crest, alpha));
            rt.FillEllipse(&ellipse(CREST_BASE), brush);
            for i in 0..CREST.len() {
                if let Some(g) = self.mascot_path(MascotShape::Feather(i)) {
                    rt.FillGeometry(&g, brush, None);
                }
            }
            rt.SetTransform(world);
            if let Some(g) = self.mascot_path(MascotShape::FalkBody) {
                brush.SetColor(&solid(ink.body, alpha));
                rt.FillGeometry(&g, brush, None);
            }
            if let Some(g) = self.mascot_path(MascotShape::FaceOnBody) {
                brush.SetColor(&solid(ink.face, alpha));
                rt.FillGeometry(&g, brush, None);
            }
            if let Some(rgb) = ink.cheek {
                rt.SetTransform(cheeks);
                brush.SetColor(&solid(rgb, alpha));
                for (cx, cy) in FALK_CHEEKS {
                    rt.FillEllipse(&ellipse((cx, cy, CHEEK_R, CHEEK_R)), brush);
                }
            }
        }
    }

    /// The falk's beak, a face mark, in the eyes' transform.
    pub(super) fn paint_falk_beak(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        expr: Expr,
        pose: &Pose,
        eyes: &Matrix3x2,
        alpha: f32,
    ) {
        if let Some(g) = self.mascot_path(MascotShape::Beak(beak_open(expr, pose))) {
            unsafe {
                rt.SetTransform(eyes);
                brush.SetColor(&solid(FALK_BEAK, alpha));
                rt.FillGeometry(&g, brush, None);
            }
        }
    }

    /// The typing falk's two feather tips on the keys, at the cat's paw
    /// places lifted by `paw_lift`, in `fixed` (the grid, not tilted).
    pub(super) fn paint_falk_tips(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        coat: FalkColor,
        paw_lift: [f32; 2],
        fixed: &Matrix3x2,
        alpha: f32,
    ) {
        let Some(g) = self.mascot_path(MascotShape::Tip) else {
            return;
        };
        unsafe {
            brush.SetColor(&solid(falk_ink(coat).body, alpha));
            for (i, &(x, y)) in PAWS.iter().enumerate() {
                rt.SetTransform(&(Matrix3x2::translation(x, y - paw_lift[i]) * *fixed));
                rt.FillGeometry(&g, brush, None);
            }
        }
    }

    /// The octopus in `world`, under its face marks: the six arms, the
    /// mantle, the spots and the suckers. The flush tints the mantle and the
    /// arms; the spots and the suckers keep the coat's own tints.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_octopus_body(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        coat: OctopusColor,
        expr: Expr,
        pose: &Pose,
        world: &Matrix3x2,
        alpha: f32,
    ) {
        let base = octopus_rgb(coat);
        let col = mix(base, FLUSH_RGB, pose.flush.clamp(0.0, 1.0));
        let now = arms(expr, pose);
        // At rest the arms are the face's own, kept; moving (dance, greeting,
        // typing) they are built for this frame.
        let still = now == arms(expr, &Pose::default());
        unsafe {
            rt.SetTransform(world);
            brush.SetColor(&solid(col, alpha));
            for (i, arm) in now.iter().enumerate() {
                let g = if still {
                    self.mascot_path(MascotShape::Arm(expr, i))
                } else {
                    path(&self.factory, &arm_segs(arm)).ok().and_then(|g| g.cast().ok())
                };
                if let Some(g) = g {
                    rt.DrawGeometry(&g, brush, ARM_W, &self.round);
                }
            }
            if let Some(g) = self.mascot_path(MascotShape::Mantle) {
                rt.FillGeometry(&g, brush, None);
            }
            brush.SetColor(&solid(shade(base), alpha));
            for (cx, cy, r) in OCT_SPOTS {
                rt.FillEllipse(&ellipse((cx, cy, r, r)), brush);
            }
            brush.SetColor(&solid(sucker_rgb(base), alpha));
            for arm in &now {
                for (cx, cy) in suckers(arm) {
                    rt.FillEllipse(&ellipse((cx, cy, SUCKER_R, SUCKER_R)), brush);
                }
            }
        }
    }
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

    #[test]
    fn octopus_arms_mirror_and_rest_on_the_floor() {
        let a = arms(Expr::Idle, &Pose::default());
        assert_eq!(a[0].start, (19.0, 42.0)); assert_eq!(a[0].end, (3.0, 54.0));
        assert_eq!(a[2].end, (21.0, 58.5));
        for i in 0..3 { assert_eq!(a[i + 3].start.0, 64.0 - a[i].start.0); assert_eq!(a[i + 3].end, (64.0 - a[i].end.0, a[i].end.1)); }
        assert!(a.iter().all(|x| x.curl == 1.0));
    }

    #[test]
    fn octopus_arms_carry_the_mood() {
        let p = Pose::default();
        assert!(arms(Expr::Happy, &p).iter().all(|x| x.curl == 1.25));
        assert!(arms(Expr::Alert, &p)[0].end.0 < 3.0 && arms(Expr::Alert, &p)[0].curl == 1.3);
        assert!(arms(Expr::Sad, &p).iter().all(|x| x.curl == 0.4));
        assert!(arms(Expr::Sleep, &p).iter().all(|x| x.curl == 1.8)); // coiled
        let wave = arms(Expr::Wave, &Pose { waving: true, wave: 0.0, ..p });
        assert_eq!(wave[3].end, (60.0, 28.0));
        let typing = arms(Expr::Typing, &Pose { paw_lift: [1.6, 0.0], ..p });
        assert_eq!(typing[0].end, (1.0, 53.0 - 1.6)); assert_eq!(typing[1].end, (9.5, 53.0));
    }

    #[test]
    fn the_dancing_arms_sway_with_a_190_ms_phase() {
        // beat = pi * t / BEAT_MS, so t = 190 ms gives sin(1 + i*1.4 + side).
        let beat = std::f32::consts::PI * 190.0 / crate::ui::robot::BEAT_MS as f32;
        let rest = arms(Expr::Music, &Pose::default());
        let dance = arms(Expr::Music, &Pose { dancing: true, beat, ..Pose::default() });
        for side in 0..2 {
            for i in 0..3 {
                let k = side * 3 + i;
                let want = 2.0 * (1.0 + i as f32 * 1.4 + side as f32).sin();
                let got = dance[k].end.0 - rest[k].end.0;
                assert!((got - want).abs() < 1e-4, "arm {k}: {got} vs {want}");
            }
        }
    }

    #[test]
    fn octopus_coats_and_tints() {
        assert_eq!(octopus_rgb(OctopusColor::Coral), 0xE87C8E);
        assert_eq!(octopus_rgb(OctopusColor::Blue), 0x5FA8C9);
        assert_eq!(mix(0xE87C8E, FLUSH_RGB, 0.0), 0xE87C8E);
        assert_eq!(mix(0xE87C8E, FLUSH_RGB, 1.0), FLUSH_RGB);
    }

    use crate::theme::{CatAccessory, Mascot};
    use crate::ui::cat_style::{CatLook, solid};
    use crate::ui::geometry::Rect;
    use crate::ui::robot::RobotPainter;

    fn painter() -> RobotPainter {
        use windows::Win32::Graphics::Direct2D::{
            D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1Factory,
        };
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }.expect("factory");
        RobotPainter::new(&factory).expect("painter")
    }

    /// The mascot painted into a 64 x 64 px area (one px per grid unit) on
    /// black, by Direct2D's software rasterizer into a WIC bitmap: no window.
    /// Each pixel is `0xRRGGBB`.
    fn render(look: CatLook, expr: Expr, pose: Pose) -> Vec<u32> {
        render_on(&painter(), look, expr, pose)
    }

    /// `render` with `painter` (and its factory), so a test can read its caches.
    fn render_on(painter: &RobotPainter, look: CatLook, expr: Expr, pose: Pose) -> Vec<u32> {
        use windows::Win32::Graphics::Direct2D::Common::{
            D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
        };
        use windows::Win32::Graphics::Direct2D::{
            D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        };
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
        use windows::Win32::Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
            WICBitmapCacheOnLoad,
        };
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        };
        const SIZE: u32 = 64;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .expect("wic");
            let bitmap = wic
                .CreateBitmap(SIZE, SIZE, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)
                .expect("bitmap");
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let rt = painter
                .factory
                .CreateWicBitmapRenderTarget(&bitmap, &props)
                .expect("target");
            let brush = rt.CreateSolidColorBrush(&solid(0, 1.0), None).expect("brush");
            rt.BeginDraw();
            rt.Clear(Some(&solid(0x000000, 1.0)));
            let area = Rect { x: 0.0, y: 0.0, w: 64.0, h: 64.0 };
            painter.paint(&rt, &brush, area, expr, pose, 1.0, look);
            rt.EndDraw(None, None).expect("end draw");
            let mut out = vec![0u8; (SIZE * SIZE * 4) as usize];
            bitmap.CopyPixels(std::ptr::null(), SIZE * 4, &mut out).expect("pixels");
            out.chunks(4)
                .map(|p| (p[2] as u32) << 16 | (p[1] as u32) << 8 | p[0] as u32)
                .collect()
        }
    }

    fn px(img: &[u32], x: u32, y: u32) -> u32 {
        img[(y * 64 + x) as usize]
    }

    #[track_caller]
    fn assert_near(got: u32, want: u32) {
        let off = [16, 8, 0]
            .iter()
            .map(|s| (((got >> s) & 0xFF) as i32 - ((want >> s) & 0xFF) as i32).abs())
            .max()
            .unwrap();
        assert!(off <= 6, "got #{got:06X}, want #{want:06X}");
    }

    fn falk(c: FalkColor) -> CatLook {
        CatLook { mascot: Mascot::Falk, falk: c, ..CatLook::default() }
    }

    fn octopus(c: OctopusColor) -> CatLook {
        CatLook { mascot: Mascot::Octopus, octopus: c, ..CatLook::default() }
    }

    #[test]
    fn the_falk_paints_its_coat_and_face() {
        let img = render(falk(FalkColor::Lutino), Expr::Idle, Pose::default());
        // (16, 45), not (15, 44): the face ellipse's edge at y 44 is x 15.2.
        assert_near(px(&img, 16, 45), 0xEC6B3A); // a cheek
        assert_near(px(&img, 32, 28), 0xF6D04A); // the face mask
        assert_near(px(&img, 32, 55), 0xF1E4A2); // the body below the mask
        assert_near(px(&img, 24, 40), 0x17120F); // an eye
        assert_near(px(&img, 32, 46), 0xD9978F); // the beak
        let white = render(falk(FalkColor::WhiteFace), Expr::Idle, Pose::default());
        assert_near(px(&white, 17, 44), 0xF4EFE6); // no cheek on Muka putih
    }

    #[test]
    fn the_octopus_paints_low_eyes_arms_and_flush() {
        let img = render(octopus(OctopusColor::Coral), Expr::Idle, Pose::default());
        assert_near(px(&img, 32, 20), 0xE87C8E);
        assert_near(px(&img, 24, 37), 0x17120F); // eyes lifted by 3
        // Only the lifted pill (y 32..42) covers (24, 33); the cat's (35..45)
        // would cover (24, 44) and not this one.
        assert_near(px(&img, 24, 33), 0x17120F);
        assert_near(px(&img, 24, 44), 0xE87C8E);
        // (4, 52), not (5, 53): the arm's t 0.8 sucker at (5.2, 54.2) covers (5, 53).
        assert_near(px(&img, 4, 52), 0xE87C8E); // an outer arm on the floor
        let red = render(
            octopus(OctopusColor::Coral),
            Expr::Alert,
            Pose { flush: 1.0, ..Pose::default() },
        );
        assert_near(px(&red, 32, 20), FLUSH_RGB);
    }

    #[test]
    fn the_cat_ignores_the_other_mascots_pose_fields() {
        let busy = Pose { flush: 1.0, hop_px: 0.0, waving: true, wave: 1.0, beat: 1.0, ..Pose::default() };
        let a = render(CatLook::default(), Expr::Idle, Pose::default());
        let b = render(CatLook::default(), Expr::Idle, busy);
        assert_eq!(a, b);
    }

    #[test]
    fn every_mascot_paints_every_face_and_accessory() {
        for m in Mascot::ALL {
            for e in Expr::ALL {
                for acc in CatAccessory::ALL {
                    let look = CatLook { mascot: *m, accessory: *acc, ..CatLook::default() };
                    render(look, e, Pose::default()); // no panic, EndDraw Ok
                }
            }
        }
    }

    #[test]
    fn still_paths_are_built_once_and_moving_arms_are_not_kept() {
        let p = painter();
        let count = || p.mascot_paths.borrow().len();
        for look in [falk(FalkColor::Grey), octopus(OctopusColor::Blue)] {
            render_on(&p, look, Expr::Typing, Pose::default());
        }
        let once = count();
        for look in [falk(FalkColor::Lutino), octopus(OctopusColor::Coral)] {
            render_on(&p, look, Expr::Typing, Pose { paw_lift: [1.6, 0.0], ..Pose::default() });
        }
        assert_eq!(count(), once, "a coat or a lifted tip builds nothing new");
        let dancing = |beat| Pose { beat, ..Pose::default() }.dance(1.0, 0.5, 0.0);
        let still = render_on(&p, octopus(OctopusColor::Coral), Expr::Music, Pose::default());
        let a = render_on(&p, octopus(OctopusColor::Coral), Expr::Music, dancing(0.3));
        let b = render_on(&p, octopus(OctopusColor::Coral), Expr::Music, dancing(1.9));
        assert_ne!(a, b, "the dancing arms sway");
        assert_ne!(still, a);
        let music = count();
        render_on(&p, octopus(OctopusColor::Coral), Expr::Music, dancing(2.7));
        assert_eq!(count(), music, "moving arms are built for the frame, not kept");
    }

    #[test]
    fn the_octopus_marks_are_espresso_and_nothing_wears_the_black_cats_rim() {
        use crate::theme::CatColor;
        for &color in CatColor::ALL {
            for m in [Mascot::Falk, Mascot::Octopus] {
                let look = CatLook { mascot: m, color, ..CatLook::default() };
                assert_eq!(look.eye_rgb(), 0x17120F, "{m:?} {color:?}");
                assert_eq!(look.rim(), None, "{m:?} {color:?}");
            }
        }
    }

    #[test]
    fn the_halo_box_holds_each_mascot() {
        let area = Rect { x: 10.0, y: 0.0, w: 32.0, h: 32.0 };
        let p = Pose::default();
        assert_eq!(mascot_box(Mascot::Cat, area, Expr::Idle, &p), crate::ui::robot::head_rect(area));
        let (o, r) = mascot_box(Mascot::Octopus, area, Expr::Idle, &p);
        assert_eq!((o.x, o.y, o.w, o.h, r), (8.5, 4.5, 35.0, 26.0, 8.5));
        let (f, _) = mascot_box(Mascot::Falk, area, Expr::Idle, &p);
        assert_eq!((f.x, f.w), (14.5, 23.0));
        assert!((f.y + f.h - 28.5).abs() < 1e-4);
        assert!(f.y < 4.0, "the crest is inside: {}", f.y);
        let (alert, _) = mascot_box(Mascot::Falk, area, Expr::Alert, &p);
        assert!(alert.y < f.y, "the Alert crest stands taller");
    }

    #[test]
    fn the_octopus_outline_holds_the_mantle_and_the_curls() {
        let o = octopus_outline(Expr::Idle, &Pose::default());
        let (l, r) = o.iter().fold((f32::MAX, f32::MIN), |(l, r), p| (l.min(p.0), r.max(p.0)));
        assert!(l < 3.0 && r > 61.0, "arms reach the sides: {l}..{r}");
        assert!(o.iter().any(|p| p.1 < 10.0)); // the mantle top
    }
}
