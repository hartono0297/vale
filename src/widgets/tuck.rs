//! Tucking a note to the left or right edge of the screen ("Selip ke tepi"):
//! when a drag ends pressed against an outer edge of the placement area the
//! note becomes a small tab (`TAB_W` wide) at that edge, and slides out while
//! the pointer is on it. Pure geometry and timing, tested without a window.
//!
//! Everything is in dip relative to the top-left of the monitor's placement
//! area, except `adjoin`, which compares work areas in physical pixels.

use serde::{Serialize, Serializer};

use crate::capture::Rect;
use crate::widgets::sticky::DipRect;

/// The tab's width, and the tallest it gets (it is as tall as the note when
/// the note is shorter).
pub const TAB_W: f32 = 16.0;
pub const TAB_MAX_H: f32 = 72.0;
/// How near the note's side must sit to the area's edge, and how near (or
/// past it) the pointer must be, for a release to tuck.
pub const EDGE_EPS: f32 = 0.5;
pub const PRESS_REACH: f32 = 4.0;
/// The slide in and out, and the wait between the pointer leaving a note
/// that is out and the slide back in. Both run on the owner's frame clock.
pub const SLIDE_MS: f32 = 180.0;
pub const GRACE_MS: f32 = 300.0;
/// The accent bar a dragged note shows on the edge a release would tuck to.
pub const CUE_W: f32 = 3.0;

/// The edge a note is tucked to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
}

impl Edge {
    pub fn name(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }

    /// Any case, spaces around it allowed; anything else is none.
    pub fn from_name(name: &str) -> Option<Edge> {
        let name = name.trim();
        [Edge::Left, Edge::Right]
            .into_iter()
            .find(|e| e.name().eq_ignore_ascii_case(name))
    }

    pub fn opposite(self) -> Edge {
        match self {
            Edge::Left => Edge::Right,
            Edge::Right => Edge::Left,
        }
    }

    /// The direction the note slides off: -1 to the left, 1 to the right.
    pub fn sign(self) -> f32 {
        match self {
            Edge::Left => -1.0,
            Edge::Right => 1.0,
        }
    }
}

impl Serialize for Edge {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

/// Which sides of a display have another display's work area against them
/// (across the note's vertical span). A note crosses an inner edge to the
/// next monitor; it only tucks to an outer one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Adjoin {
    pub left: bool,
    pub right: bool,
}

impl Adjoin {
    pub fn on(self, edge: Edge) -> bool {
        match edge {
            Edge::Left => self.left,
            Edge::Right => self.right,
        }
    }
}

/// Another display's work area touching `work` on its left or right, with
/// some of the vertical span `span` (top, bottom, px) in common. Work areas
/// of abutting monitors meet exactly; a pixel of slack covers rounding.
pub fn adjoin(work: &Rect, others: &[Rect], span: (i32, i32)) -> Adjoin {
    let shares = |o: &Rect| o.y < span.1 && span.0 < o.bottom();
    let mut out = Adjoin::default();
    for o in others {
        if o == work || !shares(o) {
            continue;
        }
        if (o.right() - work.x).abs() <= 1 {
            out.left = true;
        }
        if (o.x - work.right()).abs() <= 1 {
            out.right = true;
        }
    }
    out
}

/// The edge a Move drag that ends now tucks to: the note's left (or right)
/// side sits at the area's edge (within half a dip, after the snap clamp),
/// the cursor is within `PRESS_REACH` of or past that edge, and the edge is an
/// outer one. `None` otherwise.
pub fn press_edge(note: &DipRect, area: &DipRect, cursor: (f32, f32), adjoin: Adjoin) -> Option<Edge> {
    let left = !adjoin.left
        && (note.x - area.x).abs() <= EDGE_EPS
        && cursor.0 <= area.x + PRESS_REACH;
    let right = !adjoin.right
        && (note.right() - area.right()).abs() <= EDGE_EPS
        && cursor.0 >= area.right() - PRESS_REACH;
    match (left, right) {
        (true, true) => {
            // A note as wide as the area: the nearer side to the pointer.
            let mid = area.x + area.w / 2.0;
            Some(if cursor.0 <= mid { Edge::Left } else { Edge::Right })
        }
        (true, false) => Some(Edge::Left),
        (false, true) => Some(Edge::Right),
        (false, false) => None,
    }
}

/// The edge "Selipkan ke tepi" tucks to: the nearer one to the note's centre,
/// and only an outer one; the other side when the nearer is an inner edge;
/// `None` when neither is outer (the menu item is greyed).
pub fn menu_edge(note: &DipRect, area: &DipRect, adjoin: Adjoin) -> Option<Edge> {
    let centre = note.x + note.w / 2.0;
    let nearer = if centre <= area.x + area.w / 2.0 {
        Edge::Left
    } else {
        Edge::Right
    };
    [nearer, nearer.opposite()]
        .into_iter()
        .find(|e| !adjoin.on(*e))
}

/// `view` (the note as shown) flush against `edge` of `area`, its height
/// clamped into the area. This is where the window stays while tucked.
pub fn out_rect(view: &DipRect, edge: Edge, area: &DipRect) -> DipRect {
    let x = match edge {
        Edge::Left => area.x,
        Edge::Right => (area.right() - view.w).max(area.x),
    };
    let y = view.y.clamp(area.y, (area.bottom() - view.h).max(area.y));
    DipRect { x, y, ..*view }
}

/// The tab of the note whose out rect is `out`: `TAB_W` wide, as tall as the
/// note up to `TAB_MAX_H`, at the note's top, flush against the screen edge.
pub fn tab_rect(out: &DipRect, edge: Edge) -> DipRect {
    let x = match edge {
        Edge::Left => out.x,
        Edge::Right => out.right() - TAB_W,
    };
    DipRect {
        x,
        y: out.y,
        w: TAB_W,
        h: out.h.min(TAB_MAX_H),
    }
}

/// How far a note `w` wide is drawn off the edge `t` of the way out: all but
/// the tab at 0 (only the tab shows), none at 1 (the full note).
pub fn slide_offset(t: f32, w: f32) -> f32 {
    (w - TAB_W).max(0.0) * (1.0 - t.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: DipRect = DipRect::new(0.0, 0.0, 1280.0, 720.0);

    fn note(x: f32) -> DipRect {
        DipRect::new(x, 200.0, 240.0, 208.0)
    }

    #[test]
    fn edge_names_are_any_case_and_a_wrong_one_is_none() {
        assert_eq!(Edge::from_name("left"), Some(Edge::Left));
        assert_eq!(Edge::from_name(" RIGHT "), Some(Edge::Right));
        assert_eq!(Edge::from_name("Left"), Some(Edge::Left));
        assert_eq!(Edge::from_name("top"), None);
        assert_eq!(Edge::from_name(""), None);
        assert_eq!(Edge::Left.opposite(), Edge::Right);
        assert_eq!(serde_json::to_value(Edge::Right).unwrap(), "right");
        assert_eq!(serde_json::to_value(Edge::Left).unwrap(), "left");
    }

    #[test]
    fn a_release_pressed_on_an_outer_edge_tucks() {
        let none = Adjoin::default();
        // Flush left, the pointer at and past the edge.
        assert_eq!(press_edge(&note(0.0), &AREA, (0.0, 300.0), none), Some(Edge::Left));
        assert_eq!(press_edge(&note(0.0), &AREA, (-30.0, 300.0), none), Some(Edge::Left));
        assert_eq!(press_edge(&note(0.0), &AREA, (4.0, 300.0), none), Some(Edge::Left));
        // Flush right.
        let right = note(1280.0 - 240.0);
        assert_eq!(press_edge(&right, &AREA, (1280.0, 300.0), none), Some(Edge::Right));
        assert_eq!(press_edge(&right, &AREA, (1276.0, 300.0), none), Some(Edge::Right));
        assert_eq!(press_edge(&right, &AREA, (1400.0, 300.0), none), Some(Edge::Right));
    }

    #[test]
    fn it_needs_both_the_note_at_the_edge_and_the_pointer_near_it() {
        let none = Adjoin::default();
        // The pointer is 5 dip in: not pressing.
        assert_eq!(press_edge(&note(0.0), &AREA, (5.0, 300.0), none), None);
        // The pointer is at the edge but the note is half a dip too far in...
        assert_eq!(press_edge(&note(0.5), &AREA, (0.0, 300.0), none), Some(Edge::Left));
        assert_eq!(press_edge(&note(0.6), &AREA, (0.0, 300.0), none), None);
        // ... or 12 dip in (a gap snap), however hard the pointer presses.
        assert_eq!(press_edge(&note(12.0), &AREA, (0.0, 300.0), none), None);
        assert_eq!(press_edge(&note(1280.0 - 240.0 - 0.6), &AREA, (1280.0, 300.0), none), None);
        // The middle of the screen.
        assert_eq!(press_edge(&note(500.0), &AREA, (640.0, 300.0), none), None);
        // The pointer at the right edge pulls nothing left.
        assert_eq!(press_edge(&note(0.0), &AREA, (1280.0, 300.0), none), None);
    }

    #[test]
    fn an_inner_edge_never_tucks() {
        let inner_left = Adjoin { left: true, right: false };
        let inner_right = Adjoin { left: false, right: true };
        assert_eq!(press_edge(&note(0.0), &AREA, (-10.0, 300.0), inner_left), None);
        // The other side of the same display is outer.
        let right = note(1280.0 - 240.0);
        assert_eq!(press_edge(&right, &AREA, (1290.0, 300.0), inner_left), Some(Edge::Right));
        assert_eq!(press_edge(&right, &AREA, (1290.0, 300.0), inner_right), None);
        assert_eq!(press_edge(&note(0.0), &AREA, (-10.0, 300.0), inner_right), Some(Edge::Left));
    }

    #[test]
    fn a_note_as_wide_as_the_area_tucks_to_the_side_the_pointer_is_on() {
        let none = Adjoin::default();
        let full = DipRect::new(0.0, 100.0, 1280.0, 200.0);
        assert_eq!(press_edge(&full, &AREA, (2.0, 100.0), none), Some(Edge::Left));
        assert_eq!(press_edge(&full, &AREA, (1278.0, 100.0), none), Some(Edge::Right));
        assert_eq!(press_edge(&full, &AREA, (640.0, 100.0), none), None);
    }

    #[test]
    fn adjoining_displays_are_found_across_the_notes_span() {
        let a = Rect::new(0, 0, 1920, 1040);
        let b = Rect::new(1920, 0, 1920, 1040);
        let c = Rect::new(-1920, 0, 1920, 1040);
        // B is on A's right edge: inner there, outer on the left.
        assert_eq!(adjoin(&a, &[a, b], (200, 400)), Adjoin { left: false, right: true });
        assert_eq!(adjoin(&b, &[a, b], (200, 400)), Adjoin { left: true, right: false });
        assert_eq!(adjoin(&a, &[c, a, b], (200, 400)), Adjoin { left: true, right: true });
        // One display: both outer.
        assert_eq!(adjoin(&a, &[a], (200, 400)), Adjoin::default());
        // A smaller display beside the upper part only: the span decides.
        let small = Rect::new(1920, 0, 1280, 500);
        assert!(adjoin(&a, &[a, small], (100, 300)).right);
        assert!(!adjoin(&a, &[a, small], (600, 800)).right, "below it: outer");
        // A display that only touches at a corner is not adjoining.
        assert!(!adjoin(&a, &[a, Rect::new(1920, 1040, 1920, 1040)], (800, 1040)).right);
        // Not touching (a gap): outer.
        assert!(!adjoin(&a, &[a, Rect::new(2100, 0, 1920, 1040)], (0, 1040)).right);
    }

    #[test]
    fn the_menu_picks_the_nearer_outer_edge() {
        let none = Adjoin::default();
        assert_eq!(menu_edge(&note(100.0), &AREA, none), Some(Edge::Left));
        assert_eq!(menu_edge(&note(900.0), &AREA, none), Some(Edge::Right));
        // The nearer side is inner: the far side if that is outer.
        assert_eq!(menu_edge(&note(100.0), &AREA, Adjoin { left: true, right: false }), Some(Edge::Right));
        assert_eq!(menu_edge(&note(900.0), &AREA, Adjoin { left: false, right: true }), Some(Edge::Left));
        // Both inner: greyed.
        assert_eq!(menu_edge(&note(100.0), &AREA, Adjoin { left: true, right: true }), None);
    }

    #[test]
    fn the_out_rect_is_flush_and_clamped() {
        let n = DipRect::new(300.0, 200.0, 240.0, 208.0);
        assert_eq!(out_rect(&n, Edge::Left, &AREA), DipRect::new(0.0, 200.0, 240.0, 208.0));
        assert_eq!(out_rect(&n, Edge::Right, &AREA), DipRect::new(1040.0, 200.0, 240.0, 208.0));
        // y is clamped into the area, both ways.
        let low = DipRect::new(300.0, 700.0, 240.0, 208.0);
        assert_eq!(out_rect(&low, Edge::Right, &AREA).y, 720.0 - 208.0);
        let high = DipRect::new(300.0, -50.0, 240.0, 208.0);
        assert_eq!(out_rect(&high, Edge::Left, &AREA).y, 0.0);
        // The size never changes; an area with an offset is honoured.
        let area = DipRect::new(10.0, 20.0, 1000.0, 600.0);
        let o = out_rect(&n, Edge::Right, &area);
        assert_eq!((o.w, o.h, o.x), (240.0, 208.0, 770.0));
        assert_eq!(out_rect(&n, Edge::Left, &area).x, 10.0);
        // A note wider than the area cannot go left of it.
        let wide = DipRect::new(0.0, 0.0, 1100.0, 100.0);
        assert_eq!(out_rect(&wide, Edge::Right, &area).x, 10.0);
    }

    #[test]
    fn the_tab_is_16_wide_flush_with_the_edge_and_at_most_72_tall() {
        let out = DipRect::new(1040.0, 200.0, 240.0, 208.0);
        let t = tab_rect(&out, Edge::Right);
        assert_eq!((t.x, t.y, t.w, t.h), (1264.0, 200.0, 16.0, 72.0));
        assert_eq!(t.right(), out.right(), "against the edge");
        let out = DipRect::new(0.0, 50.0, 240.0, 208.0);
        let t = tab_rect(&out, Edge::Left);
        assert_eq!((t.x, t.y, t.w, t.h), (0.0, 50.0, 16.0, 72.0));
        // A short (rolled up) note gives a short tab.
        let rolled = DipRect::new(0.0, 50.0, 240.0, 64.0);
        assert_eq!(tab_rect(&rolled, Edge::Left).h, 64.0);
        assert_eq!(TAB_W, 16.0);
    }

    #[test]
    fn the_slide_offset_runs_from_the_tab_to_the_full_note() {
        assert_eq!(slide_offset(0.0, 240.0), 240.0 - TAB_W, "only the tab shows");
        assert_eq!(slide_offset(1.0, 240.0), 0.0, "the whole note");
        assert!((slide_offset(0.5, 240.0) - 112.0).abs() < 1e-4);
        // Out of range is clamped; a note narrower than the tab never moves.
        assert_eq!(slide_offset(-1.0, 240.0), 240.0 - TAB_W);
        assert_eq!(slide_offset(2.0, 240.0), 0.0);
        assert_eq!(slide_offset(0.0, 10.0), 0.0);
        // Monotonic.
        let mut last = f32::MAX;
        for i in 0..=20 {
            let o = slide_offset(i as f32 / 20.0, 300.0);
            assert!(o <= last);
            last = o;
        }
    }

    #[test]
    fn the_timing_constants() {
        assert_eq!(SLIDE_MS, 180.0);
        assert_eq!(GRACE_MS, 300.0);
        assert_eq!(CUE_W, 3.0);
    }
}
