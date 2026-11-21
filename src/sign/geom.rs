//! Page geometry for the signing editor.
//!
//! Conventions: page space is PDF points, y up, origin at the lower-left of
//! the unrotated page box. View space is pixels, y down, origin at the
//! top-left of the page as shown. `PageGeom::base` and the session's `extra`
//! are clockwise display rotations, exactly PDF `/Rotate`. An item's `turn`
//! is counter-clockwise in page space, as PDF `cm` rotates.

use crate::sign::{Item, Quarter};

#[derive(Clone, Copy)]
pub struct PageGeom {
    pub w: f32,
    pub h: f32,
    pub base: Quarter,
}

fn odd(q: Quarter) -> bool {
    matches!(q, Quarter::R90 | Quarter::R270)
}

/// Displayed width and height after `base + extra`.
pub fn shown_size(g: PageGeom, extra: Quarter) -> (f32, f32) {
    if odd(g.base.add(extra)) { (g.h, g.w) } else { (g.w, g.h) }
}

/// Page space (y up) to view pixels (y down, top-left of the shown page).
pub fn page_to_view(g: PageGeom, extra: Quarter, scale: f32, p: (f32, f32)) -> (f32, f32) {
    let (x, y) = p;
    // Clockwise display rotation; the result is still y up in the shown box.
    let (sx, sy) = match g.base.add(extra) {
        Quarter::R0 => (x, y),
        Quarter::R90 => (y, g.w - x),
        Quarter::R180 => (g.w - x, g.h - y),
        Quarter::R270 => (g.h - y, x),
    };
    let (_, sh) = shown_size(g, extra);
    (sx * scale, (sh - sy) * scale)
}

/// Exact inverse of `page_to_view`.
pub fn view_to_page(g: PageGeom, extra: Quarter, scale: f32, v: (f32, f32)) -> (f32, f32) {
    let (_, sh) = shown_size(g, extra);
    let (sx, sy) = (v.0 / scale, sh - v.1 / scale);
    match g.base.add(extra) {
        Quarter::R0 => (sx, sy),
        Quarter::R90 => (g.w - sy, sx),
        Quarter::R180 => (g.w - sx, g.h - sy),
        Quarter::R270 => (sy, g.h - sx),
    }
}

/// The `turn` a new item needs to look upright in the current view. The page
/// is displayed clockwise by `base + extra` and `turn` is counter-clockwise,
/// so the same quarter cancels it.
pub fn upright_turn(g: PageGeom, extra: Quarter) -> Quarter {
    g.base.add(extra)
}

/// PDF `cm` matrix mapping the unit square onto the item in page space.
pub fn item_matrix(item: &Item) -> [f32; 6] {
    let (x, y, w, h) = (item.x, item.y, item.w, item.h);
    match item.turn {
        Quarter::R0 => [w, 0.0, 0.0, h, x, y],
        Quarter::R90 => [0.0, w, -h, 0.0, x + h, y],
        Quarter::R180 => [-w, 0.0, 0.0, -h, x + w, y + h],
        Quarter::R270 => [0.0, -w, h, 0.0, x, y + w],
    }
}

/// Size of the box the item occupies in page space.
pub fn occupied(item: &Item) -> (f32, f32) {
    if odd(item.turn) { (item.h, item.w) } else { (item.w, item.h) }
}

/// Axis-aligned view rect `(x, y, w, h)` of the item.
pub fn item_view_rect(item: &Item, g: PageGeom, extra: Quarter, scale: f32) -> (f32, f32, f32, f32) {
    let (bw, bh) = occupied(item);
    let a = page_to_view(g, extra, scale, (item.x, item.y));
    let b = page_to_view(g, extra, scale, (item.x + bw, item.y + bh));
    (a.0.min(b.0), a.1.min(b.1), (a.0 - b.0).abs(), (a.1 - b.1).abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::{Item, ItemKind, Quarter};

    const ALL: [Quarter; 4] = [Quarter::R0, Quarter::R90, Quarter::R180, Quarter::R270];

    fn item(turn: Quarter) -> Item {
        Item { kind: ItemKind::Check(Default::default()), page: 0, x: 100.0, y: 200.0, w: 150.0, h: 50.0, turn }
    }

    #[test]
    fn roundtrip_all_quarters() {
        for base in ALL {
            for extra in ALL {
                let g = PageGeom { w: 612.0, h: 792.0, base };
                for p in [(0.0, 0.0), (612.0, 792.0), (100.0, 300.5), (45.25, 700.0)] {
                    let v = page_to_view(g, extra, 1.5, p);
                    let q = view_to_page(g, extra, 1.5, v);
                    assert!((q.0 - p.0).abs() < 0.01 && (q.1 - p.1).abs() < 0.01);
                }
            }
        }
    }

    #[test]
    fn shown_size_swaps_on_odd_quarters() {
        let g = PageGeom { w: 612.0, h: 792.0, base: Quarter::R90 };
        assert_eq!(shown_size(g, Quarter::R0), (792.0, 612.0));
        assert_eq!(shown_size(g, Quarter::R90), (612.0, 792.0));
    }

    #[test]
    fn placing_on_a_page_shown_rotated_is_upright() {
        let g = PageGeom { w: 612.0, h: 792.0, base: Quarter::R90 };
        assert_eq!(upright_turn(g, Quarter::R0), Quarter::R90);
        let r = item_view_rect(&item(upright_turn(g, Quarter::R0)), g, Quarter::R0, 2.0);
        assert!((r.2 - 300.0).abs() < 0.01 && (r.3 - 100.0).abs() < 0.01);
    }

    #[test]
    fn item_matrix_r0() {
        assert_eq!(item_matrix(&item(Quarter::R0)), [150.0, 0.0, 0.0, 50.0, 100.0, 200.0]);
    }

    #[test]
    fn item_matrix_r90() {
        assert_eq!(item_matrix(&item(Quarter::R90)), [0.0, 150.0, -50.0, 0.0, 150.0, 200.0]);
    }
}
