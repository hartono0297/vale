//! Tapered tubes, ported line for line from `docs/design/maskot-notch-v3.html`
//! (`normals` ... `limb`). Pure: points in, `Seg` paths and `Draw`s out, in
//! the units of the input points.

use crate::ui::play::stories::Draw;
use crate::ui::robot::Seg;

type Pt = (f32, f32);

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp2(a: Pt, b: Pt, t: f32) -> Pt {
    (lerp(a.0, b.0, t), lerp(a.1, b.1, t))
}

/// The unit normal at each point (from its neighbours), `(0, -1)` where
/// the neighbours coincide.
pub fn normals(p: &[Pt]) -> Vec<Pt> {
    let n = p.len();
    (0..n)
        .map(|i| {
            let a = p[i.saturating_sub(1)];
            let b = p[(i + 1).min(n - 1)];
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = dx.hypot(dy);
            if l < 1e-6 { (0.0, -1.0) } else { (-dy / l, dx / l) }
        })
        .collect()
}

/// A smooth run through `pts`, continuing from the current point: a quad
/// per inner point to the midpoint after it, then a line to the last.
fn smooth(pts: &[Pt], out: &mut Vec<Seg>) {
    let n = pts.len();
    for i in 1..n.saturating_sub(1) {
        out.push(Seg::Quad(
            pts[i].0,
            pts[i].1,
            (pts[i].0 + pts[i + 1].0) / 2.0,
            (pts[i].1 + pts[i + 1].1) / 2.0,
        ));
    }
    if n > 1 {
        out.push(Seg::Line(pts[n - 1].0, pts[n - 1].1));
    }
}

/// The outline of a tube about `p`, `w[i]` wide at point `i`, its tip
/// rounded and its base too when `base_cap`.
fn tube_path(p: &[Pt], w: &[f32], nm: &[Pt], base_cap: bool) -> Vec<Seg> {
    let n = p.len();
    let mut lf = Vec::with_capacity(n);
    let mut rt = Vec::with_capacity(n);
    for i in 0..n {
        let h = w[i] / 2.0;
        lf.push((p[i].0 + nm[i].0 * h, p[i].1 + nm[i].1 * h));
        rt.push((p[i].0 - nm[i].0 * h, p[i].1 - nm[i].1 * h));
    }
    let te = (nm[n - 1].1, -nm[n - 1].0);
    let k = w[n - 1] * 0.66;
    let mut d = vec![Seg::Move(lf[0].0, lf[0].1)];
    smooth(&lf, &mut d);
    d.push(Seg::Cubic(
        lf[n - 1].0 + te.0 * k,
        lf[n - 1].1 + te.1 * k,
        rt[n - 1].0 + te.0 * k,
        rt[n - 1].1 + te.1 * k,
        rt[n - 1].0,
        rt[n - 1].1,
    ));
    rt.reverse();
    smooth(&rt, &mut d);
    if base_cap {
        // `rt` is reversed now: its last point is the base's right edge.
        let tb = (nm[0].1, -nm[0].0);
        let kb = w[0] * 0.66;
        d.push(Seg::Cubic(
            rt[n - 1].0 - tb.0 * kb,
            rt[n - 1].1 - tb.1 * kb,
            lf[0].0 - tb.0 * kb,
            lf[0].1 - tb.1 * kb,
            lf[0].0,
            lf[0].1,
        ));
    }
    d.push(Seg::Close);
    d
}

/// A band along one side of the tube, `frac` of its width deep.
fn strip_path(p: &[Pt], w: &[f32], nm: &[Pt], side: f32, frac: f32) -> Vec<Seg> {
    let mut o = Vec::with_capacity(p.len());
    let mut i2 = Vec::with_capacity(p.len());
    for i in 0..p.len() {
        let h = side * w[i] / 2.0;
        let h2 = h * (1.0 - 2.0 * frac);
        o.push((p[i].0 + nm[i].0 * h, p[i].1 + nm[i].1 * h));
        i2.push((p[i].0 + nm[i].0 * h2, p[i].1 + nm[i].1 * h2));
    }
    let last = i2[i2.len() - 1];
    let mut d = vec![Seg::Move(o[0].0, o[0].1)];
    smooth(&o, &mut d);
    d.push(Seg::Line(last.0, last.1));
    i2.reverse();
    smooth(&i2, &mut d);
    d.push(Seg::Close);
    d
}

/// Running arc length at each point.
pub fn measure(p: &[Pt]) -> Vec<f32> {
    let mut c: Vec<f32> = Vec::with_capacity(p.len());
    for i in 0..p.len() {
        let prev = if i == 0 { 0.0 } else { c[i - 1] + (p[i].0 - p[i - 1].0).hypot(p[i].1 - p[i - 1].1) };
        c.push(prev);
    }
    c
}

/// The path's total length.
pub fn plen(p: &[Pt]) -> f32 {
    measure(p).last().copied().unwrap_or(0.0)
}

fn idx_at(c: &[f32], v: f32) -> usize {
    let mut i = 0;
    while i + 1 < c.len() && c[i + 1] < v {
        i += 1;
    }
    i
}

fn sample_at(p: &[Pt], c: &[f32], v: f32) -> Pt {
    if v <= 0.0 {
        return p[0];
    }
    let i = idx_at(c, v);
    if i >= p.len() - 1 {
        return p[p.len() - 1];
    }
    let span = c[i + 1] - c[i];
    let u = (v - c[i]) / if span == 0.0 { 1.0 } else { span };
    lerp2(p[i], p[i + 1], u)
}

/// `n + 1` points spaced evenly along the first `len` of the path (or all
/// of it, if shorter).
pub fn along(d: &[Pt], len: f32, n: usize) -> Vec<Pt> {
    if d.is_empty() {
        return Vec::new();
    }
    let c = measure(d);
    let e = len.min(c[c.len() - 1]);
    (0..=n).map(|k| sample_at(d, &c, e * k as f32 / n as f32)).collect()
}

/// A Catmull-Rom spline through `pts`, `seg` points per span.
pub fn catmull_rom(pts: &[Pt], seg: usize) -> Vec<Pt> {
    let mut out = Vec::new();
    for i in 0..pts.len().saturating_sub(1) {
        let p0 = if i > 0 { pts[i - 1] } else { pts[i] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts.get(i + 2).copied().unwrap_or(p2);
        for s in 0..seg {
            let t = s as f32 / seg as f32;
            let (t2, t3) = (t * t, t * t * t);
            let k = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (-a + c) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push((k(p0.0, p1.0, p2.0, p3.0), k(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    if let Some(&l) = pts.last() {
        out.push(l);
    }
    out
}

/// The path continued by a tightening spiral of length `len` in `m` steps
/// (the tip curl), turning `ang` radians in all, started `sway` off the
/// path's heading.
pub fn curl(p: &[Pt], ang: f32, len: f32, m: usize, sway: f32) -> Vec<Pt> {
    if p.len() < 2 || len <= 0.0 || m == 0 {
        return p.to_vec();
    }
    let a0 = p[p.len() - 1];
    let b = p[p.len() - 2];
    let mut a = (a0.1 - b.1).atan2(a0.0 - b.0) + sway;
    let (mut x, mut y) = a0;
    let mut out = p.to_vec();
    let st = len / m as f32;
    let mf = m as f32;
    for j in 1..=m {
        a += ang * 2.0 * j as f32 / (mf * (mf + 1.0));
        x += a.cos() * st;
        y += a.sin() * st;
        out.push((x, y));
    }
    out
}

/// A centreline from a base point at angle `a`, `len` long in `n` steps,
/// bending by `k(s)` radians over the whole length at fraction `s`.
pub fn chain(x: f32, y: f32, a: f32, len: f32, n: usize, k: impl Fn(f32) -> f32) -> Vec<Pt> {
    let (mut x, mut y, mut a) = (x, y, a);
    let mut p = vec![(x, y)];
    for i in 0..n {
        let s = (i as f32 + 0.5) / n as f32;
        a += k(s) / n as f32;
        x += a.cos() * len / n as f32;
        y += a.sin() * len / n as f32;
        p.push((x, y));
    }
    p
}

/// The joint (knee, elbow) of two bones `l1`, `l2` long from `a` toward
/// `b`; `bend` (+1 or -1) picks the side. Out of reach, it stretches.
pub fn ik(a: Pt, b: Pt, l1: f32, l2: f32, bend: f32) -> Pt {
    if l1 <= 0.0 {
        return a;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let mut d = dx.hypot(dy).min(l1 + l2 - 0.01);
    if d == 0.0 || d.is_nan() {
        d = 0.01;
    }
    let ang = dy.atan2(dx);
    let c = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0);
    let g = ang - bend * c.acos();
    (a.0 + g.cos() * l1, a.1 + g.sin() * l1)
}

/// Rings around the tube: bands `w` long, one every `every` from `start`
/// back from the tip, none within `skip_base` of the base.
#[derive(Clone, Copy, Debug)]
pub struct Stripes {
    pub start: f32,
    pub every: f32,
    pub w: f32,
    pub skip_base: f32,
    pub rgb: u32,
}

/// Round suckers along one side (`side` +1 or -1), `rgb` rimmed round an
/// `inner` dot.
#[derive(Clone, Copy, Debug)]
pub struct Suckers {
    pub start: f32,
    pub every: f32,
    pub side: f32,
    pub rgb: u32,
    pub inner: u32,
}

/// The mockup's `limb` options. The width at fraction `s` of the length is
/// `wf(s)`, or `w0` to `w1` along `s^pow`, times `wm`, with `fluff`
/// ripples toward the tip; `cap` rounds the base too. `side_shade` is the
/// side (+1 or -1) and depth (fraction of the width) of the volume band,
/// darkened by `shade_op` (the mockup's `sop`). `tip` is the dark tip's
/// length and colour.
#[derive(Clone, Copy, Debug)]
pub struct LimbStyle {
    pub w0: f32,
    pub w1: f32,
    pub wf: Option<fn(f32) -> f32>,
    pub pow: f32,
    pub wm: f32,
    pub fluff: f32,
    pub cap: bool,
    pub side_shade: Option<(i8, f32)>,
    pub shade_op: f32,
    pub stripes: Option<Stripes>,
    pub tip: Option<(f32, u32)>,
    pub suckers: Option<Suckers>,
}

impl Default for LimbStyle {
    fn default() -> Self {
        LimbStyle {
            w0: 1.0,
            w1: 1.0,
            wf: None,
            pow: 1.0,
            wm: 1.0,
            fluff: 0.0,
            cap: false,
            side_shade: Some((1, 0.32)),
            shade_op: 0.17,
            stripes: None,
            tip: None,
            suckers: None,
        }
    }
}

/// `rgb` darkened toward black by `op`: black at `op` alpha over `rgb`.
fn darken(rgb: u32, op: f32) -> u32 {
    let ch = |sh: u32| {
        let v = ((rgb >> sh) & 0xFF) as f32 * (1.0 - op.clamp(0.0, 1.0));
        (v.round() as u32).min(255) << sh
    };
    ch(16) | ch(8) | ch(0)
}

/// One limb about the centreline `p`: the body, stripes, dark tip, suckers,
/// then a thin shade band for volume, as the mockup draws them. Nothing
/// for fewer than two points or a path shorter than 0.6.
pub fn limb(p: &[Pt], st: &LimbStyle, rgb: u32) -> Vec<Draw> {
    if p.len() < 2 {
        return Vec::new();
    }
    let c = measure(p);
    let tot = c[c.len() - 1];
    if tot < 0.6 {
        return Vec::new();
    }
    let pow = if st.pow == 0.0 { 1.0 } else { st.pow };
    let w: Vec<f32> = c
        .iter()
        .map(|&v| {
            let base = match st.wf {
                Some(f) => f(v / tot),
                None => lerp(st.w0, st.w1, (v / tot).powf(pow)),
            };
            let fl = if st.fluff != 0.0 {
                1.0 + st.fluff * ((tot - v) * 1.9).sin() * ((tot - v) / 6.0).clamp(0.0, 1.0)
            } else {
                1.0
            };
            (base * st.wm * fl).max(0.3)
        })
        .collect();
    let nm = normals(p);
    let mut out = vec![Draw::Fill { path: tube_path(p, &w, &nm, st.cap), rgb }];
    if let Some(q) = st.stripes {
        // Exact band edges at arc length v, curved like a ring round a cylinder.
        let edge = |v: f32| {
            let i = idx_at(&c, v);
            (sample_at(p, &c, v), w[i] * 0.49, nm[i])
        };
        let pt = |e: (Pt, f32, Pt), s: f32| (e.0.0 + e.2.0 * s * e.1, e.0.1 + e.2.1 * s * e.1);
        let cp = |e: (Pt, f32, Pt)| (e.0.0 + e.2.1 * e.1 * 0.8, e.0.1 - e.2.0 * e.1 * 0.8);
        let mut d = q.start;
        while q.every > 0.0 && d < tot - q.w - q.skip_base {
            let a = edge(tot - d - q.w / 2.0);
            let b = edge(tot - d + q.w / 2.0);
            let (a1, b1, an, bn) = (pt(a, 1.0), pt(b, 1.0), pt(a, -1.0), pt(b, -1.0));
            let (ca, cb) = (cp(a), cp(b));
            out.push(Draw::Fill {
                path: vec![
                    Seg::Move(a1.0, a1.1),
                    Seg::Line(b1.0, b1.1),
                    Seg::Quad(cb.0, cb.1, bn.0, bn.1),
                    Seg::Line(an.0, an.1),
                    Seg::Quad(ca.0, ca.1, a1.0, a1.1),
                    Seg::Close,
                ],
                rgb: q.rgb,
            });
            d += q.every;
        }
    }
    if let Some((len, tip_rgb)) = st.tip {
        let i = idx_at(&c, tot - len);
        out.push(Draw::Fill { path: tube_path(&p[i..], &w[i..], &nm[i..], false), rgb: tip_rgb });
    }
    if let Some(q) = st.suckers {
        let mut d = q.start;
        while q.every > 0.0 && d < tot - 1.2 {
            let i = idx_at(&c, tot - d);
            let h = q.side * w[i] * 0.33;
            let r = w[i] * 0.25;
            d += q.every;
            if r < 0.28 {
                continue;
            }
            let at = (p[i].0 + nm[i].0 * h, p[i].1 + nm[i].1 * h);
            out.push(Draw::Ellipse { c: at, r: (r, r), rgb: q.rgb });
            out.push(Draw::Ellipse { c: at, r: (r * 0.42, r * 0.42), rgb: q.inner });
        }
    }
    if let Some((side, frac)) = st.side_shade {
        out.push(Draw::Fill {
            path: strip_path(p, &w, &nm, side as f32, frac),
            rgb: darken(rgb, st.shade_op),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    fn all_finite(d: &[Draw]) -> bool {
        d.iter().all(|x| match x {
            Draw::Fill { path, .. } => path.iter().all(seg_finite),
            Draw::Ellipse { c, r, .. } => [c.0, c.1, r.0, r.1].iter().all(|v| v.is_finite()),
            _ => true,
        })
    }

    fn seg_finite(s: &Seg) -> bool {
        match *s {
            Seg::Move(a, b) | Seg::Line(a, b) => a.is_finite() && b.is_finite(),
            Seg::Quad(a, b, c, d) => [a, b, c, d].iter().all(|v| v.is_finite()),
            Seg::Cubic(a, b, c, d, e, f) => [a, b, c, d, e, f].iter().all(|v| v.is_finite()),
            Seg::Close => true,
        }
    }

    #[test]
    fn catmull_rom_passes_through_its_points() {
        let pts = [(0.0, 0.0), (4.0, 3.0), (9.0, -2.0), (12.0, 5.0)];
        let out = catmull_rom(&pts, 8);
        assert_eq!(out.len(), 3 * 8 + 1);
        for (i, p) in pts.iter().enumerate() {
            let q = out[i * 8];
            assert!(close(q.0, p.0, 1e-4) && close(q.1, p.1, 1e-4), "{i}: {q:?} vs {p:?}");
        }
    }

    #[test]
    fn along_returns_n_plus_one_points_spaced_evenly() {
        let path = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let out = along(&path, 20.0, 10);
        assert_eq!(out.len(), 11);
        for k in 0..10 {
            let d = (out[k + 1].0 - out[k].0).hypot(out[k + 1].1 - out[k].1);
            assert!(close(d, 2.0, 1e-4), "step {k} is {d}");
        }
    }

    #[test]
    fn a_tube_is_as_wide_as_its_width_function() {
        let path = along(&[(0.0, 0.0), (10.0, 0.0)], 10.0, 24);
        let st = LimbStyle { w0: 4.0, w1: 2.0, side_shade: None, ..LimbStyle::default() };
        let out = limb(&path, &st, 0x808080);
        let Draw::Fill { path: segs, .. } = &out[0] else { panic!("no body") };
        // The left edge sits half a width off the centreline (+y here).
        let Seg::Move(_, y0) = segs[0] else { panic!() };
        assert!(close(y0, 2.0, 0.05), "base {y0}");
        let mid = segs
            .iter()
            .find_map(|s| match *s {
                Seg::Quad(cx, cy, ..) if close(cx, 5.0, 1e-3) => Some(cy),
                _ => None,
            })
            .expect("a point at the middle");
        assert!(close(mid, 1.5, 0.05), "middle {mid}");
        let end = segs
            .iter()
            .find_map(|s| match *s {
                Seg::Line(x, y) if close(x, 10.0, 1e-3) => Some(y),
                _ => None,
            })
            .expect("the tip");
        assert!(close(end, 1.0, 0.05), "tip {end}");
    }

    #[test]
    fn ik_keeps_both_bone_lengths() {
        let (a, b) = ((0.0, 0.0), (6.0, 3.0));
        for bend in [1.0, -1.0] {
            let k = ik(a, b, 5.0, 4.0, bend);
            assert!(close(k.0.hypot(k.1), 5.0, 1e-3));
            assert!(close((b.0 - k.0).hypot(b.1 - k.1), 4.0, 1e-3));
        }
    }

    #[test]
    fn a_limb_has_no_nan_for_a_degenerate_path() {
        let st = LimbStyle {
            tip: Some((2.0, 0x101010)),
            stripes: Some(Stripes { start: 0.0, every: 1.0, w: 0.5, skip_base: 0.0, rgb: 1 }),
            suckers: Some(Suckers { start: 0.0, every: 1.0, side: 1.0, rgb: 2, inner: 3 }),
            ..LimbStyle::default()
        };
        for path in [vec![], vec![(1.0, 1.0)], vec![(1.0, 1.0), (1.0, 1.0)]] {
            assert!(all_finite(&limb(&path, &st, 0x808080)));
        }
        // Long enough to draw, but folded back on itself.
        let fold = [(0.0, 0.0), (5.0, 0.0), (0.0, 0.0)];
        assert!(all_finite(&limb(&fold, &st, 0x808080)));
        for pts in [vec![], vec![(2.0, 2.0)], vec![(2.0, 2.0), (2.0, 2.0)]] {
            assert!(catmull_rom(&pts, 8).iter().all(|p| p.0.is_finite() && p.1.is_finite()));
            assert!(along(&pts, 5.0, 4).iter().all(|p| p.0.is_finite() && p.1.is_finite()));
            assert!(curl(&pts, 1.0, 3.0, 8, 0.0).iter().all(|p| p.0.is_finite() && p.1.is_finite()));
        }
        let k = ik((1.0, 1.0), (1.0, 1.0), 3.0, 3.0, 1.0);
        assert!(k.0.is_finite() && k.1.is_finite());
    }
}
