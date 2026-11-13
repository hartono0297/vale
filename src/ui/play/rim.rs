//! A point and an outward normal anywhere along the island's lower outline,
//! for both styles, so a limb winding round an end has one path to follow.

use crate::config::Style;
use crate::ui::geometry::{ShapeRect, corner_radius};
use std::f32::consts::{FRAC_PI_2, PI};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RimPt {
    pub x: f32,
    pub y: f32,
    /// Outward unit normal.
    pub nx: f32,
    pub ny: f32,
    /// Unit tangent in the direction of increasing `u`.
    pub tx: f32,
    pub ty: f32,
}

/// One piece of the outline: a straight run, or a circular arc whose angle
/// `a` (y down, so `a = PI/2` is the bottom) falls from `a0` to `a1`.
#[derive(Clone, Copy)]
enum Seg {
    Line { x: f32, y: f32, dx: f32, dy: f32, len: f32 },
    Arc { cx: f32, cy: f32, r: f32, a0: f32, a1: f32 },
}

impl Seg {
    fn len(self) -> f32 {
        match self {
            Seg::Line { len, .. } => len,
            Seg::Arc { r, a0, a1, .. } => r * (a0 - a1),
        }
    }

    fn at(self, d: f32) -> RimPt {
        match self {
            Seg::Line { x, y, dx, dy, .. } => RimPt {
                x: x + dx * d,
                y: y + dy * d,
                nx: -dy,
                ny: dx,
                tx: dx,
                ty: dy,
            },
            Seg::Arc { cx, cy, r, a0, .. } => {
                let (sin, cos) = (a0 - d / r).sin_cos();
                RimPt {
                    x: cx + r * cos,
                    y: cy + r * sin,
                    nx: cos,
                    ny: sin,
                    tx: sin,
                    ty: -cos,
                }
            }
        }
    }
}

/// `u` (0 to 1, clamped) along the island's lower outline, left to right:
/// from the top of the left edge (the screen edge for the Notch, the top of
/// the half-circle end for the Pill), round the bottom-left end, along the
/// lip, round the bottom-right end, up to the top of the right edge. Split by
/// arc length, so `u` lands as far along at any size.
pub fn rim(style: Style, s: ShapeRect, u: f32) -> RimPt {
    let r = corner_radius(style, s);
    let (left, right, top, bottom) = (s.x, s.x + s.w, s.y, s.y + s.h);
    let line = |x, y, dx, dy, len| Seg::Line { x, y, dx, dy, len };
    let arc = |cx, cy, a0, a1| Seg::Arc { cx, cy, r, a0, a1 };
    let (lo, hi) = (FRAC_PI_2, PI);
    // The Notch's sides start at the screen edge, as `notch_path` draws them
    // past the shoulder; the Pill's start at the top of its rounded corners.
    let segs: Vec<Seg> = match style {
        Style::Notch => {
            let side = bottom - r - top;
            vec![
                line(left, top, 0.0, 1.0, side),
                arc(left + r, bottom - r, hi, lo),
                line(left + r, bottom, 1.0, 0.0, s.w - 2.0 * r),
                arc(right - r, bottom - r, lo, 0.0),
                line(right, bottom - r, 0.0, -1.0, side),
            ]
        }
        Style::Pill => {
            let side = s.h - 2.0 * r;
            vec![
                arc(left + r, top + r, 3.0 * FRAC_PI_2, hi),
                line(left, top + r, 0.0, 1.0, side),
                arc(left + r, bottom - r, hi, lo),
                line(left + r, bottom, 1.0, 0.0, s.w - 2.0 * r),
                arc(right - r, bottom - r, lo, 0.0),
                line(right, bottom - r, 0.0, -1.0, side),
                arc(right - r, top + r, 0.0, -FRAC_PI_2),
            ]
        }
    };
    let total: f32 = segs.iter().map(|g| g.len()).sum();
    let mut d = u.clamp(0.0, 1.0) * total;
    for g in &segs {
        if d <= g.len() {
            return g.at(d);
        }
        d -= g.len();
    }
    segs[segs.len() - 1].at(segs[segs.len() - 1].len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::ui::geometry::shape_contains;
    use crate::ui::play::testing::shape;

    #[test]
    fn rim_midpoint_is_the_lip_centre_in_both_styles() {
        for style in [Style::Notch, Style::Pill] {
            let s = shape(style, 190.0, 34.0);
            let p = rim(style, s, 0.5);
            assert!((p.x - (s.x + s.w / 2.0)).abs() < 0.01, "{style:?}");
            assert!((p.y - (s.y + s.h)).abs() < 0.01, "{style:?}");
            assert!((p.ny - 1.0).abs() < 0.01, "{style:?} normal points down");
        }
    }

    #[test]
    fn rim_normals_point_out_of_the_shape() {
        for style in [Style::Notch, Style::Pill] {
            for i in 0..=100 {
                let u = i as f32 / 100.0;
                let s = shape(style, 190.0, 34.0);
                let p = rim(style, s, u);
                assert!(
                    !shape_contains(style, s, p.x + p.nx * 2.0, p.y + p.ny * 2.0),
                    "{style:?} u={u}"
                );
            }
        }
    }

    #[test]
    fn rim_is_continuous_at_every_size() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)] {
                let s = shape(style, w, h);
                let mut prev = rim(style, s, 0.0);
                for i in 1..=1000 {
                    let p = rim(style, s, i as f32 / 1000.0);
                    let step = ((p.x - prev.x).powi(2) + (p.y - prev.y).powi(2)).sqrt();
                    assert!(step < 2.0, "{style:?} {w}x{h} jumped {step} at {i}");
                    prev = p;
                }
            }
        }
    }

    #[test]
    fn the_pill_ends_are_half_circles_and_the_notch_sides_are_straight() {
        let pill = shape(Style::Pill, 190.0, 34.0);
        let left = (0..=210)
            .map(|i| rim(Style::Pill, pill, i as f32 / 2000.0))
            .min_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap();
        assert!(
            (left.y - (pill.y + pill.h / 2.0)).abs() < 0.3,
            "leftmost point is at the end's centre height"
        );

        let notch = shape(Style::Notch, 190.0, 34.0);
        let top = rim(Style::Notch, notch, 0.0);
        assert!((top.y - notch.y).abs() < 0.01, "the notch's side starts at the screen edge");
        assert!((top.x - notch.x).abs() < 0.01);
    }
}
