//! Where a widget may sit: the 8 dip grid, snapping to the placement area's
//! edges and to other widgets (12 dip apart, or edge to edge), the clamp that
//! keeps it fully inside the area, the size limits, and where a new note goes.
//!
//! Everything is in dip relative to the top-left of the monitor's placement
//! area (the work area less whatever the Dock will reserve), so the same
//! numbers hold on any monitor and at any DPI.

use crate::widgets::sticky::{
    DEFAULT_H, DEFAULT_W, DipRect, MAX_H, MAX_W, MIN_H, MIN_W, ROLLED_H,
};

pub const GRID: f32 = 8.0;
/// The space left between two widgets.
pub const GAP: f32 = 12.0;
/// How near an edge must come before it snaps.
pub const THRESHOLD: f32 = 8.0;

/// The taskbar-like zone at the bottom the Dock will take; widgets stay out of
/// it. The Dock's later phase feeds this; today nothing is reserved.
pub fn reserved_bottom_dip() -> f32 {
    0.0
}

/// The work area of a monitor, in dip, less the reserved bottom zone, moved
/// to the origin.
pub fn placement_area(work_w: f32, work_h: f32, reserved_bottom: f32) -> DipRect {
    DipRect::new(0.0, 0.0, work_w.max(0.0), (work_h - reserved_bottom).max(0.0))
}

/// Which edge a guide marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// A vertical line at an x position (left or right edges matched).
    X,
    /// A horizontal line at a y position.
    Y,
}

/// An edge of the moved widget that lines up with another widget's edge or
/// with the area's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub axis: Axis,
    /// The shared coordinate.
    pub pos: f32,
    /// The widget it lines up with (its place in `others`), `None` for the
    /// area's edge.
    pub other: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapped {
    pub rect: DipRect,
    pub guides: Vec<Guide>,
}

fn grid(v: f32) -> f32 {
    (v / GRID).round() * GRID
}

/// The size of `r` inside the limits, and in an area too small for them, the
/// area's own.
fn clamp_size(w: f32, h: f32, area: &DipRect) -> (f32, f32) {
    clamp_size_min(w, h, area, MIN_H)
}

/// `clamp_size` with its own smallest height: a rolled-up note is shown
/// `ROLLED_H` tall, below `MIN_H`.
fn clamp_size_min(w: f32, h: f32, area: &DipRect, min_h: f32) -> (f32, f32) {
    let max_w = MAX_W.min(area.w).max(0.0);
    let max_h = MAX_H.min(area.h).max(0.0);
    (w.clamp(MIN_W.min(max_w), max_w), h.clamp(min_h.min(max_h), max_h))
}

/// The smallest height a note is shown at.
fn min_h_of(rolled: bool) -> f32 {
    if rolled { ROLLED_H } else { MIN_H }
}

/// `r` with its size inside the limits and the whole rect inside `area`
/// (never over the Dock's zone or the taskbar).
pub fn clamp_rect(r: DipRect, area: &DipRect) -> DipRect {
    clamp_rect_for(r, area, false)
}

/// `clamp_rect` for a rect as shown: `rolled` lets it be `ROLLED_H` tall, so
/// a rolled-up note keeps its true height and can sit at the bottom edge.
pub fn clamp_rect_for(r: DipRect, area: &DipRect, rolled: bool) -> DipRect {
    let (w, h) = clamp_size_min(r.w, r.h, area, min_h_of(rolled));
    let x = r.x.clamp(area.x, (area.right() - w).max(area.x));
    let y = r.y.clamp(area.y, (area.bottom() - h).max(area.y));
    DipRect { x, y, w, h }
}

/// Whether the two ranges are within a gap (and a snap) of overlapping: only
/// then does a neighbour's edge pull a widget to the 12 dip distance.
fn near(a0: f32, a1: f32, b0: f32, b1: f32) -> bool {
    let reach = GAP + THRESHOLD;
    a0 <= b1 + reach && b0 <= a1 + reach
}

/// The candidate closest to `want` within `THRESHOLD`.
fn nearest(want: f32, candidates: &[f32]) -> Option<f32> {
    candidates
        .iter()
        .copied()
        .filter(|c| (c - want).abs() <= THRESHOLD)
        .min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()))
}

/// The guides of `r`: every edge equal (within half a dip) to the area's edge
/// or to an edge of one of `others`.
fn guides_of(r: &DipRect, area: &DipRect, others: &[DipRect]) -> Vec<Guide> {
    let eq = |a: f32, b: f32| (a - b).abs() < 0.5;
    let mut out: Vec<Guide> = Vec::new();
    let mut add = |g: Guide| {
        if !out
            .iter()
            .any(|o| o.axis == g.axis && eq(o.pos, g.pos) && o.other == g.other)
        {
            out.push(g);
        }
    };
    for (x, edge) in [(r.x, area.x), (r.right(), area.right())] {
        if eq(x, edge) {
            add(Guide { axis: Axis::X, pos: edge, other: None });
        }
    }
    for (y, edge) in [(r.y, area.y), (r.bottom(), area.bottom())] {
        if eq(y, edge) {
            add(Guide { axis: Axis::Y, pos: edge, other: None });
        }
    }
    for (i, o) in others.iter().enumerate() {
        for x in [r.x, r.right()] {
            for ox in [o.x, o.right()] {
                if eq(x, ox) {
                    add(Guide { axis: Axis::X, pos: ox, other: Some(i) });
                }
            }
        }
        for y in [r.y, r.bottom()] {
            for oy in [o.y, o.bottom()] {
                if eq(y, oy) {
                    add(Guide { axis: Axis::Y, pos: oy, other: Some(i) });
                }
            }
        }
    }
    out
}

/// A widget of size `want.w` x `want.h` dragged to `want`'s place: the place
/// it snaps to. The size stays. `alt` (Alt held) turns snapping off; the
/// clamp stays.
#[cfg(test)]
pub fn snap_move(want: DipRect, area: &DipRect, others: &[DipRect], alt: bool) -> Snapped {
    snap_move_for(want, area, others, alt, false)
}

/// `snap_move` for a note that is rolled up (`want.h` is `ROLLED_H`): its
/// edges are the ones on screen.
pub fn snap_move_for(
    want: DipRect,
    area: &DipRect,
    others: &[DipRect],
    alt: bool,
    rolled: bool,
) -> Snapped {
    let base = clamp_rect_for(want, area, rolled);
    if alt {
        return Snapped { rect: base, guides: Vec::new() };
    }
    let (w, h) = (base.w, base.h);
    let mut xs = vec![area.x, area.right() - w];
    for o in others {
        xs.extend([o.x, o.right() - w]);
        if near(want.y, want.y + h, o.y, o.bottom()) {
            xs.extend([o.right() + GAP, o.x - GAP - w]);
        }
    }
    let x = nearest(want.x, &xs).unwrap_or_else(|| grid(want.x));
    // The vertical pull depends on where the widget ended up horizontally.
    let mut ys = vec![area.y, area.bottom() - h];
    for o in others {
        ys.extend([o.y, o.bottom() - h]);
        if near(x, x + w, o.x, o.right()) {
            ys.extend([o.bottom() + GAP, o.y - GAP - h]);
        }
    }
    let y = nearest(want.y, &ys).unwrap_or_else(|| grid(want.y));
    let rect = clamp_rect_for(DipRect { x, y, w, h }, area, rolled);
    let guides = guides_of(&rect, area, others);
    Snapped { rect, guides }
}

/// A widget resized by its bottom-right grip to `want` (its top-left stays):
/// the size snaps to the grid, to the area's edges and to other widgets, and
/// is kept inside the limits and the area.
pub fn snap_resize(want: DipRect, area: &DipRect, others: &[DipRect], alt: bool) -> Snapped {
    let limited = |w: f32, h: f32| {
        let room_w = (area.right() - want.x).max(0.0);
        let room_h = (area.bottom() - want.y).max(0.0);
        let (mut w, mut h) = clamp_size(w, h, area);
        w = w.min(room_w.max(MIN_W.min(area.w)));
        h = h.min(room_h.max(MIN_H.min(area.h)));
        (w, h)
    };
    let (mut w, mut h) = limited(want.w, want.h);
    if !alt {
        let right = want.x + want.w;
        let bottom = want.y + want.h;
        let mut xs = vec![area.right()];
        let mut ys = vec![area.bottom()];
        for o in others {
            if near(want.y, bottom, o.y, o.bottom()) {
                xs.extend([o.x - GAP, o.right()]);
            }
            if near(want.x, right, o.x, o.right()) {
                ys.extend([o.y - GAP, o.bottom()]);
            }
        }
        let snapped_right = nearest(right, &xs).unwrap_or_else(|| want.x + grid(want.w));
        let snapped_bottom = nearest(bottom, &ys).unwrap_or_else(|| want.y + grid(want.h));
        (w, h) = limited(snapped_right - want.x, snapped_bottom - want.y);
    }
    let rect = clamp_rect(DipRect { x: want.x, y: want.y, w, h }, area);
    let guides = if alt { Vec::new() } else { guides_of(&rect, area, others) };
    Snapped { rect, guides }
}

/// Whether `r` fits at its place without touching any of `others` (with the gap).
fn free(r: &DipRect, others: &[DipRect]) -> bool {
    let grown = DipRect::new(r.x - GAP, r.y - GAP, r.w + 2.0 * GAP, r.h + 2.0 * GAP);
    !others.iter().any(|o| grown.intersects(o))
}

/// The place of a new note of the default size: the free grid slot nearest to
/// just under the notch (top centre of the area). A crowded area gives the
/// spot under the notch anyway.
pub fn place_new(area: &DipRect, others: &[DipRect]) -> DipRect {
    let (w, h) = clamp_size(DEFAULT_W, DEFAULT_H, area);
    let target = (grid(area.x + (area.w - w) / 2.0), grid(area.y + 3.0 * GRID));
    let target = clamp_rect(DipRect::new(target.0, target.1, w, h), area);
    let mut best: Option<(f32, DipRect)> = None;
    let cols = ((area.w - w) / GRID).floor().max(0.0) as i32;
    let rows = ((area.h - h) / GRID).floor().max(0.0) as i32;
    for row in 0..=rows {
        for col in 0..=cols {
            let r = DipRect::new(area.x + col as f32 * GRID, area.y + row as f32 * GRID, w, h);
            let d = (r.x - target.x).powi(2) + (r.y - target.y).powi(2);
            if best.is_some_and(|(b, _)| b <= d) || !free(&r, others) {
                continue;
            }
            best = Some((d, r));
        }
    }
    best.map_or(target, |(_, r)| r)
}

/// The place of a note made from the (+) in `source`'s header: to its right
/// with the gap if that fits, else to its left, else below, clamped.
pub fn place_beside(source: &DipRect, area: &DipRect) -> DipRect {
    let (w, h) = clamp_size(DEFAULT_W, DEFAULT_H, area);
    let right = source.right() + GAP;
    let left = source.x - GAP - w;
    let below = source.bottom() + GAP;
    let at = if right + w <= area.right() {
        (right, source.y)
    } else if left >= area.x {
        (left, source.y)
    } else if below + h <= area.bottom() {
        (source.x, below)
    } else {
        (source.x + 2.0 * GRID, source.y + 2.0 * GRID)
    };
    clamp_rect(DipRect::new(grid(at.0), grid(at.1), w, h), area)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: DipRect = DipRect::new(0.0, 0.0, 1280.0, 720.0);

    fn r(x: f32, y: f32, w: f32, h: f32) -> DipRect {
        DipRect::new(x, y, w, h)
    }

    #[test]
    fn a_free_move_goes_to_the_grid() {
        let s = snap_move(r(203.0, 117.0, 240.0, 208.0), &AREA, &[], false);
        assert_eq!((s.rect.x, s.rect.y), (200.0, 120.0));
        assert!(s.guides.is_empty());
        assert_eq!((s.rect.w, s.rect.h), (240.0, 208.0), "the size stays");
    }

    #[test]
    fn it_snaps_to_the_area_edges() {
        let s = snap_move(r(5.0, 3.0, 240.0, 208.0), &AREA, &[], false);
        assert_eq!((s.rect.x, s.rect.y), (0.0, 0.0));
        assert!(s.guides.contains(&Guide { axis: Axis::X, pos: 0.0, other: None }));
        assert!(s.guides.contains(&Guide { axis: Axis::Y, pos: 0.0, other: None }));
        let s = snap_move(r(1035.0, 510.0, 240.0, 208.0), &AREA, &[], false);
        assert_eq!(s.rect.right(), 1280.0);
        assert_eq!(s.rect.bottom(), 720.0);
    }

    #[test]
    fn it_snaps_to_another_widget_with_the_gap() {
        let other = r(100.0, 100.0, 240.0, 208.0);
        // Dragged to just right of it, 8 dip off the 12 dip gap.
        let s = snap_move(r(100.0 + 240.0 + 17.0, 104.0, 240.0, 208.0), &AREA, &[other], false);
        assert_eq!(s.rect.x, 352.0, "right edge + 12");
        // The tops are within the threshold: aligned, with a guide.
        assert_eq!(s.rect.y, 100.0);
        assert!(s.guides.contains(&Guide { axis: Axis::Y, pos: 100.0, other: Some(0) }));
        // Left of another one that has room on that side.
        let far = r(500.0, 100.0, 240.0, 208.0);
        let s = snap_move(r(245.0, 100.0, 240.0, 208.0), &AREA, &[far], false);
        assert_eq!(s.rect.right(), 488.0);
        // Below it.
        let s = snap_move(r(100.0, 308.0 + 12.0 + 4.0, 240.0, 208.0), &AREA, &[other], false);
        assert_eq!(s.rect.y, 320.0);
        assert!(s.guides.iter().any(|g| g.axis == Axis::X && g.other == Some(0)));
    }

    #[test]
    fn it_aligns_edges_with_another_widget() {
        let other = r(400.0, 100.0, 200.0, 150.0);
        // Moved far below it: only the left edges line up.
        let s = snap_move(r(405.0, 400.0, 240.0, 208.0), &AREA, &[other], false);
        assert_eq!(s.rect.x, 400.0);
        assert!(s.guides.contains(&Guide { axis: Axis::X, pos: 400.0, other: Some(0) }));
        // Right edges: 600 - 240 = 360.
        let s = snap_move(r(357.0, 400.0, 240.0, 208.0), &AREA, &[other], false);
        assert_eq!(s.rect.right(), 600.0);
    }

    #[test]
    fn a_rolled_note_keeps_its_shown_height_at_the_bottom_and_under_a_neighbour() {
        let h = ROLLED_H;
        // The clamp keeps 56 tall and lets it sit on the bottom edge.
        let c = clamp_rect_for(r(10.0, 5000.0, 240.0, h), &AREA, true);
        assert_eq!((c.h, c.bottom()), (h, AREA.bottom()));
        // Without the flag the old 120 minimum raises it.
        assert_eq!(clamp_rect(r(10.0, 5000.0, 240.0, h), &AREA).h, MIN_H);
        // Dragged to the bottom: it snaps to the area's bottom, no 64 dip lost.
        let s = snap_move_for(r(100.0, 720.0 - h - 3.0, 240.0, h), &AREA, &[], false, true);
        assert_eq!((s.rect.h, s.rect.bottom()), (h, AREA.bottom()));
        assert!(s.guides.iter().any(|g| g.axis == Axis::Y && g.pos == AREA.bottom()));
        // Above a neighbour: the gap is 12, not 12 + 64.
        let other = r(100.0, 400.0, 240.0, 150.0);
        let s = snap_move_for(r(100.0, 400.0 - GAP - h + 3.0, 240.0, h), &AREA, &[other], false, true);
        assert_eq!(s.rect.bottom() + GAP, other.y);
        // Under it likewise.
        let s = snap_move_for(r(100.0, other.bottom() + GAP - 3.0, 240.0, h), &AREA, &[other], false, true);
        assert_eq!(s.rect.y, other.bottom() + GAP);
        assert_eq!(s.rect.h, h);
    }

    #[test]
    fn a_widget_far_away_does_not_pull() {
        let other = r(900.0, 600.0, 200.0, 100.0);
        let s = snap_move(r(100.0, 100.0, 240.0, 208.0), &AREA, &[other], false);
        assert_eq!((s.rect.x, s.rect.y), (104.0, 104.0), "plain grid");
    }

    #[test]
    fn alt_turns_snapping_off_but_not_the_clamp() {
        let other = r(100.0, 100.0, 240.0, 208.0);
        let s = snap_move(r(353.0, 103.0, 240.0, 208.0), &AREA, &[other], true);
        assert_eq!((s.rect.x, s.rect.y), (353.0, 103.0));
        assert!(s.guides.is_empty());
        let s = snap_move(r(-50.0, 9000.0, 240.0, 208.0), &AREA, &[], true);
        assert_eq!((s.rect.x, s.rect.y), (0.0, 720.0 - 208.0), "still inside");
    }

    #[test]
    fn the_clamp_keeps_a_widget_inside_the_area() {
        let c = clamp_rect(r(-20.0, -5.0, 240.0, 208.0), &AREA);
        assert_eq!((c.x, c.y), (0.0, 0.0));
        let c = clamp_rect(r(1200.0, 700.0, 240.0, 208.0), &AREA);
        assert_eq!((c.right(), c.bottom()), (1280.0, 720.0));
        // A reserved bottom zone keeps it above the taskbar or the Dock.
        let area = placement_area(1280.0, 720.0, 64.0);
        assert_eq!(area.h, 656.0);
        let c = clamp_rect(r(10.0, 600.0, 240.0, 208.0), &area);
        assert!(c.bottom() <= 656.0);
        // Sizes are limited.
        let c = clamp_rect(r(0.0, 0.0, 10.0, 10.0), &AREA);
        assert_eq!((c.w, c.h), (MIN_W, MIN_H));
        let c = clamp_rect(r(0.0, 0.0, 5000.0, 5000.0), &AREA);
        assert_eq!((c.w, c.h), (MAX_W, 720.0), "the area is smaller than the maximum");
    }

    #[test]
    fn no_reserved_zone_today() {
        assert_eq!(reserved_bottom_dip(), 0.0);
    }

    #[test]
    fn resizing_snaps_the_size_and_respects_the_limits() {
        let s = snap_resize(r(100.0, 100.0, 253.0, 211.0), &AREA, &[], false);
        assert_eq!((s.rect.w, s.rect.h), (256.0, 208.0));
        assert_eq!((s.rect.x, s.rect.y), (100.0, 100.0), "the corner stays");
        let s = snap_resize(r(100.0, 100.0, 40.0, 30.0), &AREA, &[], false);
        assert_eq!((s.rect.w, s.rect.h), (MIN_W, MIN_H));
        let s = snap_resize(r(100.0, 100.0, 4000.0, 4000.0), &AREA, &[], false);
        assert_eq!((s.rect.w, s.rect.h), (MAX_W, MAX_H.min(720.0 - 100.0)));
        // Never past the area's right edge.
        let s = snap_resize(r(1200.0, 100.0, 300.0, 200.0), &AREA, &[], false);
        assert!(s.rect.right() <= 1280.0, "{s:?}");
        // Alt: exact.
        let s = snap_resize(r(100.0, 100.0, 253.0, 211.0), &AREA, &[], true);
        assert_eq!((s.rect.w, s.rect.h), (253.0, 211.0));
    }

    #[test]
    fn resizing_snaps_to_a_neighbour_with_the_gap() {
        let other = r(500.0, 100.0, 200.0, 200.0);
        let s = snap_resize(r(100.0, 100.0, 385.0, 208.0), &AREA, &[other], false);
        assert_eq!(s.rect.right(), 488.0, "12 before the neighbour");
    }

    #[test]
    fn a_new_note_goes_under_the_notch() {
        let p = place_new(&AREA, &[]);
        assert_eq!((p.w, p.h), (DEFAULT_W, DEFAULT_H));
        assert_eq!(p.x, 520.0, "centred on 1280 (grid 8)");
        assert_eq!(p.y, 24.0);
        assert!(AREA.contains(p.x, p.y) && p.right() <= 1280.0 && p.bottom() <= 720.0);
    }

    #[test]
    fn a_new_note_takes_the_nearest_free_slot() {
        let first = place_new(&AREA, &[]);
        let second = place_new(&AREA, &[first]);
        assert!(!second.intersects(&first), "{second:?} overlaps {first:?}");
        assert!(second.x % GRID == 0.0 && second.y % GRID == 0.0, "on the grid");
        // And it is close: within one note's size of the spot under the notch.
        assert!((second.x - first.x).abs() <= DEFAULT_W + 2.0 * GAP + GRID);
        let mut all = vec![first, second];
        for _ in 0..6 {
            let next = place_new(&AREA, &all);
            assert!(all.iter().all(|o| !next.intersects(o)), "{next:?}");
            all.push(next);
        }
    }

    #[test]
    fn a_crowded_area_still_gives_a_place() {
        let tiny = DipRect::new(0.0, 0.0, 300.0, 250.0);
        let block = r(0.0, 0.0, 300.0, 250.0);
        let p = place_new(&tiny, &[block]);
        assert!(tiny.contains(p.x, p.y));
    }

    #[test]
    fn beside_prefers_right_then_left_then_below() {
        let src = r(100.0, 100.0, 240.0, 208.0);
        let p = place_beside(&src, &AREA);
        assert_eq!((p.x, p.y), (352.0, 104.0), "right with the gap, on the grid");
        // No room on the right.
        let src = r(1100.0, 100.0, 240.0, 208.0);
        let p = place_beside(&src, &AREA);
        assert_eq!(p.x, 848.0, "left of it");
        assert!(p.right() <= src.x);
        // No room on either side: below.
        let wide = DipRect::new(0.0, 0.0, 300.0, 720.0);
        let src = r(30.0, 40.0, 240.0, 208.0);
        let p = place_beside(&src, &wide);
        assert!(p.y >= src.bottom(), "{p:?}");
        // Nothing fits: still inside.
        let p = place_beside(&r(0.0, 0.0, 300.0, 250.0), &DipRect::new(0.0, 0.0, 300.0, 250.0));
        assert!(p.right() <= 300.0 && p.bottom() <= 250.0);
    }
}
