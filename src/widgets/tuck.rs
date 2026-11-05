//! Tucking a note to the left or right edge of the screen ("Selip ke tepi"):
//! when a drag ends pressed against an outer edge of the placement area the
//! note becomes a small tab (`TAB_W` wide) at that edge, and pops out of it
//! (scale and fade, `Pop`) while the pointer is on it. Pure geometry and
//! timing, tested without a window.
//!
//! Everything is in dip relative to the top-left of the monitor's placement
//! area, except `adjoin`, which compares work areas in physical pixels.

use std::time::Instant;

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
/// The wait between the pointer leaving a note that is out and the pop back
/// into the tab. It runs on the owner's frame clock, like the pop.
pub const GRACE_MS: f32 = 300.0;
/// The pop: the smallest the note gets (about its tab), the way out (ease-out
/// with a little overshoot) and the way in (ease-in), in ms for a whole trip.
pub const MIN_SCALE: f32 = 0.12;
pub const OUT_MS: f32 = 300.0;
pub const IN_MS: f32 = 200.0;
/// The overshoot of the way out (ease-out-back's c1).
pub const BACK: f32 = 1.1;
/// The note is faded in over the first `NOTE_FADE` of the progress, the tab
/// out over the first `TAB_FADE` (the way in is the mirror of this).
pub const NOTE_FADE: f32 = 0.45;
pub const TAB_FADE: f32 = 0.35;
/// The largest scale of `out_scale`: 1 + 0.88 * 4 c^3 / (27 (c + 1)^2) for
/// `BACK` c, rounded up a hair. The window keeps `room` for it.
pub const PEAK_SCALE: f32 = 1.0394;
/// The hit keeper: while the pop is part way, the tab's rect is filled at
/// this alpha so the pointer there still reaches the window.
pub const KEEPER_ALPHA: f32 = 3.0 / 255.0;
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

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Ease-out-back: 0 at 0, 1 at 1, a little over 1 (`BACK`) on the way.
pub fn back_out(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0) - 1.0;
    1.0 + (BACK + 1.0) * u * u * u + BACK * u * u
}

/// The note's scale on the way out, `u` of the trip (0 to 1): `MIN_SCALE` to 1
/// with a small overshoot. A whole trip of `Pop` follows it (a part one starts
/// from where the scale was); the tests hold them together.
#[cfg_attr(not(test), allow(dead_code))]
pub fn out_scale(u: f32) -> f32 {
    MIN_SCALE + (1.0 - MIN_SCALE) * back_out(u)
}

/// The note's scale on the way in, `u` of the trip: 1 to `MIN_SCALE`, ease-in.
#[cfg_attr(not(test), allow(dead_code))]
pub fn in_scale(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    1.0 - (1.0 - MIN_SCALE) * u * u * u
}

/// How opaque the note is at progress `p` (0 only the tab, 1 all the way out).
pub fn note_alpha(p: f32) -> f32 {
    smoothstep(p / NOTE_FADE)
}

/// How opaque the tab is at progress `p`.
pub fn tab_alpha(p: f32) -> f32 {
    1.0 - smoothstep(p / TAB_FADE)
}

/// The point the note grows from, in note dip (0, 0 is its top-left): the
/// tab's vertical centre on the note's edge side.
pub fn origin(edge: Edge, w: f32, shown_h: f32) -> (f32, f32) {
    let x = match edge {
        Edge::Left => 0.0,
        Edge::Right => w,
    };
    (x, TAB_MAX_H.min(shown_h) / 2.0)
}

/// `pt` scaled by `s` about `origin`.
pub fn scale_about(pt: (f32, f32), origin: (f32, f32), s: f32) -> (f32, f32) {
    (
        origin.0 + (pt.0 - origin.0) * s,
        origin.1 + (pt.1 - origin.1) * s,
    )
}

/// The inverse of `scale_about`: the note point that is drawn at `pt`.
pub fn unscale(pt: (f32, f32), origin: (f32, f32), s: f32) -> (f32, f32) {
    if s <= 0.0 {
        return pt;
    }
    (
        origin.0 + (pt.0 - origin.0) / s,
        origin.1 + (pt.1 - origin.1) / s,
    )
}

/// How far the paper's shadow reaches beyond the note, in dip: left, top,
/// right, bottom (the shadow rects grow 5 and sit 2 down), and one dip slack.
pub const SHADOW_REACH: (f32, f32, f32, f32) = (6.0, 4.0, 6.0, 8.0);

/// How far the paper, its shadow and the `edges` sheet edges of a stack (0
/// for a plain note, 2 at most) reach beyond the note, dip: left, top, right,
/// bottom. A stack edge `k` (1-based) sits `k * STACK_STEP` down and right and
/// its shadow grows 1.5 (and sits 2 down), so it reaches 1.5 and 3.5 further.
pub fn reach(edges: usize) -> (f32, f32, f32, f32) {
    let r = SHADOW_REACH;
    if edges == 0 {
        return r;
    }
    let off = crate::widgets::paint::STACK_STEP * edges as f32;
    (r.0, r.1, r.2.max(off + 1.5), r.3.max(off + 3.5))
}

/// The margin a tucked note's window needs on top of the base one
/// (`paint::margins()`), dip, so that the paper, its shadow and a stack's
/// `edges` sheet edges, scaled by `PEAK_SCALE` about the `origin`, fit inside
/// it. Left, top, right, bottom.
pub fn room(w: f32, shown_h: f32, edge: Edge, edges: usize) -> (f32, f32, f32, f32) {
    let o = origin(edge, w, shown_h);
    let base = crate::widgets::paint::margins();
    let r = reach(edges);
    // The corners of the paper plus shadow, scaled about the origin.
    let lo = scale_about((-r.0, -r.1), o, PEAK_SCALE);
    let hi = scale_about((w + r.2, shown_h + r.3), o, PEAK_SCALE);
    let need = (-lo.0, -lo.1, hi.0 - w, hi.1 - shown_h);
    let side = |need: f32, base: f32| (need - base).max(0.0).ceil();
    (
        side(need.0, base.0),
        side(need.1, base.1),
        side(need.2, base.2),
        side(need.3, base.3),
    )
}

/// A trip of a tucked note between the tab (progress 0) and out (1): the
/// progress runs linearly, the scale follows its own curve, and a trip that
/// starts part way (a reversal) continues from the scale it was at.
#[derive(Clone, Copy, Debug)]
pub struct Pop {
    pub from: f32,
    pub to: f32,
    pub from_scale: f32,
    pub start: Instant,
    pub ms: f32,
}

impl Pop {
    /// Settled at progress `p` (0 or 1).
    pub fn at(p: f32) -> Pop {
        Pop {
            from: p,
            to: p,
            from_scale: if p >= 1.0 { 1.0 } else { MIN_SCALE },
            start: Instant::now(),
            ms: 0.0,
        }
    }

    /// How much of the trip is done, 0 to 1 (1 for a settled one).
    pub fn u(&self, now: Instant) -> f32 {
        if self.ms <= 0.0 {
            return 1.0;
        }
        (now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 / self.ms).clamp(0.0, 1.0)
    }

    /// 0 only the tab, 1 all the way out.
    pub fn progress(&self, now: Instant) -> f32 {
        self.from + (self.to - self.from) * self.u(now)
    }

    pub fn running(&self, now: Instant) -> bool {
        self.ms > 0.0 && now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 < self.ms
    }

    pub fn scale(&self, now: Instant) -> f32 {
        let u = self.u(now);
        if u >= 1.0 {
            // Settled: exactly the end, whatever the trip started from.
            return if self.to >= 1.0 { 1.0 } else { MIN_SCALE };
        }
        if self.to >= 1.0 {
            self.from_scale + (1.0 - self.from_scale) * back_out(u)
        } else {
            self.from_scale + (MIN_SCALE - self.from_scale) * u * u * u
        }
    }

    pub fn note_alpha(&self, now: Instant) -> f32 {
        note_alpha(self.progress(now))
    }

    pub fn tab_alpha(&self, now: Instant) -> f32 {
        tab_alpha(self.progress(now))
    }

    /// Heads for `to` (0 or 1) from wherever it is. The same target again
    /// changes nothing; a partial trip takes proportionally less time; without
    /// `animate` it is there at once.
    pub fn toward(&mut self, to: f32, now: Instant, animate: bool) {
        if (self.to - to).abs() < f32::EPSILON {
            return;
        }
        let from = self.progress(now);
        let from_scale = self.scale(now);
        let full = if to >= 1.0 { OUT_MS } else { IN_MS };
        *self = Pop {
            from,
            to,
            from_scale,
            start: now,
            ms: if animate { full * (to - from).abs() } else { 0.0 },
        };
    }
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

    use std::time::Duration;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn the_way_out_grows_from_the_tab_with_a_small_overshoot() {
        assert!(near(out_scale(0.0), MIN_SCALE));
        assert!(near(out_scale(1.0), 1.0));
        // Out of range is clamped.
        assert!(near(out_scale(-1.0), MIN_SCALE));
        assert!(near(out_scale(2.0), 1.0));
        let (mut peak, mut at) = (0.0f32, 0.0f32);
        for i in 0..=1000 {
            let u = i as f32 / 1000.0;
            let v = out_scale(u);
            if v > peak {
                (peak, at) = (v, u);
            }
        }
        assert!(peak > 1.0 && peak <= 1.041, "peak {peak}");
        assert!((0.6..0.7).contains(&at), "at {at}");
        // The constant covers the peak, closely.
        assert!(PEAK_SCALE >= peak && PEAK_SCALE - peak < 1e-3, "{PEAK_SCALE} vs {peak}");
        // Non-increasing after the peak.
        let mut last = f32::MAX;
        for i in 0..=200 {
            let u = at + (1.0 - at) * i as f32 / 200.0;
            let v = out_scale(u);
            assert!(v <= last + 1e-6);
            last = v;
        }
    }

    #[test]
    fn the_way_in_shrinks_to_the_tab_without_a_bounce() {
        assert!(near(in_scale(0.0), 1.0));
        assert!(near(in_scale(1.0), MIN_SCALE));
        let mut last = f32::MAX;
        for i in 0..=100 {
            let v = in_scale(i as f32 / 100.0);
            assert!(v <= last);
            last = v;
        }
    }

    #[test]
    fn the_note_and_the_tab_cross_fade_the_same_way_in_both_directions() {
        assert_eq!(note_alpha(0.0), 0.0);
        assert!(near(note_alpha(NOTE_FADE), 1.0));
        assert_eq!(note_alpha(1.0), 1.0);
        assert!(near(note_alpha(NOTE_FADE / 2.0), 0.5), "smoothstep midpoint");
        assert_eq!(tab_alpha(0.0), 1.0);
        assert!(near(tab_alpha(TAB_FADE), 0.0));
        assert_eq!(tab_alpha(1.0), 0.0);
        assert!(near(tab_alpha(TAB_FADE / 2.0), 0.5));
        assert_eq!(note_alpha(-1.0), 0.0);
        assert_eq!(tab_alpha(3.0), 0.0);
    }

    #[test]
    fn the_note_grows_from_the_tabs_centre_on_its_edge_side() {
        assert_eq!(origin(Edge::Right, 240.0, 208.0), (240.0, 36.0));
        assert_eq!(origin(Edge::Left, 240.0, 208.0), (0.0, 36.0));
        // A rolled note is shorter than the tab: the centre of what shows.
        assert_eq!(origin(Edge::Left, 240.0, 40.0), (0.0, 20.0));
        assert_eq!(origin(Edge::Right, 300.0, 64.0), (300.0, 32.0));
    }

    #[test]
    fn unscale_undoes_scale_about() {
        let o = (240.0, 36.0);
        for s in [0.12, 0.5, 1.0, 1.039] {
            for p in [(0.0, 0.0), (120.0, 90.0), (240.0, 36.0), (-10.0, 300.0)] {
                let q = unscale(scale_about(p, o, s), o, s);
                assert!(near(q.0, p.0) && near(q.1, p.1), "{p:?} at {s}: {q:?}");
            }
        }
        // The origin does not move, and a bad scale leaves the point alone.
        assert_eq!(scale_about(o, o, 0.3), o);
        assert_eq!(unscale((5.0, 6.0), o, 0.0), (5.0, 6.0));
    }

    fn at(p: &Pop, ms: u64) -> Instant {
        p.start + Duration::from_millis(ms)
    }

    #[test]
    fn a_pop_settles_at_its_ends() {
        let now = Instant::now();
        let tab = Pop::at(0.0);
        assert!(!tab.running(now));
        assert_eq!((tab.progress(now), tab.scale(now)), (0.0, MIN_SCALE));
        assert_eq!((tab.note_alpha(now), tab.tab_alpha(now)), (0.0, 1.0));
        let out = Pop::at(1.0);
        assert_eq!((out.progress(now), out.scale(now)), (1.0, 1.0));
        assert_eq!((out.note_alpha(now), out.tab_alpha(now)), (1.0, 0.0));
    }

    #[test]
    fn a_full_trip_follows_the_curves() {
        let now = Instant::now();
        let mut p = Pop::at(0.0);
        p.toward(1.0, now, true);
        assert!(p.running(now));
        assert_eq!(p.ms, OUT_MS);
        for ms in [0u64, 60, 150, 195, 250, 300, 500] {
            let t = at(&p, ms);
            let u = (ms as f32 / OUT_MS).min(1.0);
            assert!(near(p.scale(t), out_scale(u)), "out at {ms}");
            assert!(near(p.progress(t), u), "progress at {ms}");
        }
        assert!(!p.running(at(&p, 300)));
        assert!(p.running(at(&p, 299)));
        let mut q = Pop::at(1.0);
        q.toward(0.0, now, true);
        assert_eq!(q.ms, IN_MS);
        for ms in [0u64, 50, 100, 150, 200, 400] {
            let t = at(&q, ms);
            let u = (ms as f32 / IN_MS).min(1.0);
            assert!(near(q.scale(t), in_scale(u)), "in at {ms}");
            assert!(near(q.progress(t), 1.0 - u), "progress at {ms}");
        }
    }

    fn state(p: &Pop, t: Instant) -> [f32; 3] {
        [p.scale(t), p.note_alpha(t), p.tab_alpha(t)]
    }

    #[test]
    fn a_reversal_part_way_continues_without_a_jump() {
        // Going out, turned back at several points.
        for cut in [20u64, 90, 180, 240, 290] {
            let mut p = Pop::at(0.0);
            let t0 = p.start;
            p.toward(1.0, t0, true);
            let t = t0 + Duration::from_millis(cut);
            let before = state(&p, t);
            p.toward(0.0, t, true);
            let after = state(&p, t);
            for k in 0..3 {
                assert!((before[k] - after[k]).abs() < 1e-4, "out then in at {cut}: {before:?} {after:?}");
            }
            assert_eq!(p.to, 0.0);
            // And it ends at the tab.
            let end = t + Duration::from_millis(400);
            assert!(near(p.scale(end), MIN_SCALE) && near(p.progress(end), 0.0));
        }
        // Going in, turned back.
        for cut in [20u64, 80, 140, 190] {
            let mut p = Pop::at(1.0);
            let t0 = p.start;
            p.toward(0.0, t0, true);
            let t = t0 + Duration::from_millis(cut);
            let before = state(&p, t);
            p.toward(1.0, t, true);
            let after = state(&p, t);
            for k in 0..3 {
                assert!((before[k] - after[k]).abs() < 1e-4, "in then out at {cut}: {before:?} {after:?}");
            }
            let end = t + Duration::from_millis(400);
            assert!(near(p.scale(end), 1.0) && near(p.progress(end), 1.0));
        }
    }

    #[test]
    fn a_partial_trip_takes_proportionally_less_time() {
        let mut p = Pop::at(0.0);
        let t0 = p.start;
        p.toward(1.0, t0, true);
        let t = t0 + Duration::from_millis(150);
        assert!(near(p.progress(t), 0.5));
        p.toward(0.0, t, true);
        assert!(near(p.ms, IN_MS * 0.5), "{}", p.ms);
        // Out again from a quarter.
        let mut q = Pop::at(1.0);
        q.toward(0.0, q.start, true);
        let t = q.start + Duration::from_millis(150);
        assert!(near(q.progress(t), 0.25));
        q.toward(1.0, t, true);
        assert!(near(q.ms, OUT_MS * 0.75), "{}", q.ms);
    }

    #[test]
    fn the_same_target_again_changes_nothing_and_without_animation_it_is_instant() {
        let now = Instant::now();
        let mut p = Pop::at(0.0);
        p.toward(1.0, now, true);
        let before = p;
        p.toward(1.0, now + Duration::from_millis(50), true);
        assert_eq!((p.from, p.ms), (before.from, before.ms));
        assert_eq!(p.start, before.start);
        let mut q = Pop::at(0.0);
        q.toward(1.0, now, false);
        assert!(!q.running(now));
        assert_eq!((q.progress(now), q.scale(now)), (1.0, 1.0));
        q.toward(0.0, now, false);
        assert_eq!((q.progress(now), q.scale(now)), (0.0, MIN_SCALE));
        assert_eq!((q.note_alpha(now), q.tab_alpha(now)), (0.0, 1.0));
    }

    #[test]
    fn the_window_keeps_room_for_the_overshoot() {
        let base = crate::widgets::paint::margins();
        for edge in [Edge::Left, Edge::Right] {
            let (w, h) = (240.0, 208.0);
            let r = room(w, h, edge, 0);
            assert!(r.0 >= 0.0 && r.1 >= 0.0 && r.2 >= 0.0 && r.3 >= 0.0);
            assert_eq!(r.1, 0.0, "nothing on top");
            let o = origin(edge, w, h);
            let reach = SHADOW_REACH;
            let lo = scale_about((-reach.0, -reach.1), o, PEAK_SCALE);
            let hi = scale_about((w + reach.2, h + reach.3), o, PEAK_SCALE);
            assert!(lo.0 >= -(base.0 + r.0) && lo.1 >= -(base.1 + r.1), "{edge:?} top-left");
            assert!(hi.0 <= w + base.2 + r.2 && hi.1 <= h + base.3 + r.3, "{edge:?} bottom-right");
        }
        // The side the note grows from takes the room: the left of a right-edge note.
        let r = room(240.0, 208.0, Edge::Right, 0);
        assert!((7.0..=9.0).contains(&r.0) && (7.0..=9.0).contains(&r.3), "{r:?}");
        assert_eq!(r.2, 0.0);
        let l = room(240.0, 208.0, Edge::Left, 0);
        assert!((7.0..=9.0).contains(&l.2) && (7.0..=9.0).contains(&l.3) && l.0 == 0.0, "{l:?}");
        // A rolled note is short: nothing on top either.
        assert_eq!(room(240.0, 40.0, Edge::Right, 0).1, 0.0);
    }

    #[test]
    fn the_window_keeps_room_for_a_stacks_edges_at_the_peak() {
        let base = crate::widgets::paint::margins();
        for edges in [1usize, 2] {
            for edge in [Edge::Left, Edge::Right] {
                let (w, h) = (240.0, 208.0);
                let r = room(w, h, edge, edges);
                let o = origin(edge, w, h);
                let reach = reach(edges);
                let lo = scale_about((-reach.0, -reach.1), o, PEAK_SCALE);
                let hi = scale_about((w + reach.2, h + reach.3), o, PEAK_SCALE);
                assert!(lo.0 >= -(base.0 + r.0) && lo.1 >= -(base.1 + r.1), "{edge:?} {edges} top-left");
                assert!(hi.0 <= w + base.2 + r.2 && hi.1 <= h + base.3 + r.3, "{edge:?} {edges} bottom-right");
            }
        }
        // The two edges reach 7.5 right and 9.5 down, beyond a plain note's 6 and 8.
        assert_eq!(reach(2), (6.0, 4.0, 7.5, 9.5));
        assert_eq!(reach(0), SHADOW_REACH);
        // A stack needs at least as much room as a plain note, more at the far side.
        let (plain, stack) = (room(240.0, 208.0, Edge::Left, 0), room(240.0, 208.0, Edge::Left, 2));
        assert!(stack.2 > plain.2 && stack.3 >= plain.3, "{plain:?} {stack:?}");
    }

    #[test]
    fn the_timing_constants() {
        assert_eq!(GRACE_MS, 300.0);
        assert_eq!(OUT_MS, 300.0);
        assert_eq!(IN_MS, 200.0);
        assert_eq!(MIN_SCALE, 0.12);
        assert_eq!(BACK, 1.1);
        assert_eq!(NOTE_FADE, 0.45);
        assert_eq!(TAB_FADE, 0.35);
        assert!((KEEPER_ALPHA - 0.012).abs() < 1e-3);
        assert_eq!(CUE_W, 3.0);
    }
}
