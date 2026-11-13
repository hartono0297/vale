//! The island's outline as a point list the animations can dent, squeeze,
//! swing, shift and tilt, with the pivot each style needs.

use super::rim::rim;
use crate::config::Style;
use crate::ui::geometry::ShapeRect;

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
}

/// True when `d` leaves the island's geometry untouched; a dent with no depth
/// or no width counts as none.
/// Shift and tilt do nothing on the Notch, as in `outline`.
pub fn is_rest(style: Style, d: &Deform) -> bool {
    let pill = style == Style::Pill;
    d.squeeze == 0.0
        && d.swing_deg == 0.0
        && (!pill || (d.shift == (0.0, 0.0) && d.tilt_deg == 0.0))
        && d.dents.iter().all(|k| k.depth == 0.0 || k.width <= 0.0)
}

/// The deformed lower outline, left to right. The Notch's outline grows two
/// short runs above the screen edge while it swings, so the rotation never
/// leaves a gap at the top of the screen.
pub fn outline(style: Style, s: ShapeRect, d: &Deform) -> Vec<(f32, f32)> {
    let notch = style == Style::Notch;
    let (pivot_x, pivot_y) = if notch {
        (s.x + s.w / 2.0, s.y)
    } else {
        (s.x + s.w / 2.0, s.y + s.h / 2.0)
    };
    let (swing, shift) = if notch {
        (d.swing_deg, (0.0, 0.0))
    } else {
        (d.swing_deg + d.tilt_deg, d.shift)
    };
    let (sin, cos) = swing.to_radians().sin_cos();
    let keep = 1.0 - d.squeeze;
    let place = |x: f32, y: f32| {
        let (dx, dy) = ((x - pivot_x) * keep, y - pivot_y);
        (
            pivot_x + dx * cos - dy * sin + shift.0,
            pivot_y + dx * sin + dy * cos + shift.1,
        )
    };

    let mut out = Vec::with_capacity(OUTLINE_PTS + 4);
    let extend = notch && swing != 0.0;
    // Rotated by `a`, the side lines cross the screen edge `w/2 * tan a`
    // above the top corners; reaching a little past that keeps them crossing.
    let lift = s.w / 2.0 * swing.to_radians().abs().tan() + 1.0;
    if extend {
        out.push(place(s.x, s.y - lift));
    }
    for i in 0..OUTLINE_PTS {
        let p = rim(style, s, i as f32 / (OUTLINE_PTS - 1) as f32);
        let (mut x, mut y) = (p.x, p.y);
        for k in d.dents.iter().filter(|k| k.depth != 0.0 && k.width > 0.0) {
            let c = rim(style, s, k.u);
            let t = (x - c.x).hypot(y - c.y) / (k.width * s.h);
            if t < 1.0 {
                let push = k.depth * s.h * 0.5 * (1.0 + (t * std::f32::consts::PI).cos());
                x += p.nx * push;
                y += p.ny * push;
            }
        }
        out.push(place(x, y));
    }
    if extend {
        out.push(place(s.x + s.w, s.y - lift));
    }
    out
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
}
