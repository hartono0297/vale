//! What a closed island frame needs redrawn and presented: the pure half of
//! the dirty-rect present (`render.rs` does the drawing), plus the key that
//! lets an equalizer tick skip a frame that would repeat the last one.
//!
//! The rule is "the pixels equal a full redraw". A frame is only a `Dirty`
//! redraw when both it and the frame before it are closed views whose paint
//! stays inside their outline bounds (and the drag halo), and nothing else
//! about the surface changed; anything else is a `Full` redraw.

use super::geometry::{BAR_COUNT, Rect, ShapeRect, shoulder_depth};
use super::render::{NoticeCloseView, Progress, RobotFrame};
use super::cat_style::CatLook;
use crate::config::{Style, UiFont};
use crate::model::View;
use crate::theme::Palette;

/// The drag halo reaches this far beside the island, logical px.
pub const HALO_SIDE: f32 = 14.0;
/// And this far below it.
pub const HALO_BELOW: f32 = 8.0;

/// A rectangle in logical pixels (dip).
pub type RectDip = Rect;

/// A rectangle in whole physical pixels: `left`/`top` inclusive,
/// `right`/`bottom` exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectI {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectI {
    pub fn is_empty(&self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
}

/// How the frame's view paints, as far as the bounds are concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    /// Idle, the compact views and notices: clipped to the shape.
    Closed,
    /// The fill alone.
    Hidden,
    /// The open panel (and anything else the bounds cannot vouch for).
    Panel,
}

impl ViewKind {
    pub fn of(view: &View) -> Self {
        match view {
            View::Panel(_) => ViewKind::Panel,
            View::Hidden => ViewKind::Hidden,
            _ => ViewKind::Closed,
        }
    }
}

/// Where a closed frame can paint, in dip: the outline (a Notch's shoulders
/// reach `shoulder_depth` to each side of the shape) and, when the drag halo
/// is drawn, the halo's rectangle. `None` for the panel: a full redraw.
pub fn painted_bounds(
    style: Style,
    shape: ShapeRect,
    view: ViewKind,
    eyes: bool,
    halo: bool,
) -> Option<RectDip> {
    if view == ViewKind::Panel {
        return None;
    }
    let side = match style {
        Style::Notch => shoulder_depth(shape),
        Style::Pill => 0.0,
    };
    let outline = Rect {
        x: shape.x - side,
        y: shape.y,
        w: shape.w + 2.0 * side,
        h: shape.h,
    };
    if halo && !eyes && view == ViewKind::Closed {
        let halo = Rect {
            x: shape.x - HALO_SIDE,
            y: 0.0,
            w: shape.w + 2.0 * HALO_SIDE,
            h: shape.y + shape.h + HALO_BELOW,
        };
        Some(union_dip(outline, halo))
    } else {
        Some(outline)
    }
}

fn union_dip(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    let (r, bottom) = (a.right().max(b.right()), a.bottom().max(b.bottom()));
    Rect {
        x,
        y,
        w: r - x,
        h: bottom - y,
    }
}

/// The safety margin around a device rect: anti-aliased edges, a rounded
/// clip's bleed and rounding all stay inside it. At least `ceil(2 * scale) + 1`.
pub fn margin_px(scale: f32) -> i32 {
    (2.0 * scale.max(1.0)).ceil() as i32 + 1
}

/// `rect` in whole physical pixels (floor the near edges, ceil the far ones),
/// grown by `margin_px` on every side.
pub fn to_device(rect: RectDip, scale: f32, margin_px: i32) -> RectI {
    RectI {
        left: (rect.x * scale).floor() as i32 - margin_px,
        top: (rect.y * scale).floor() as i32 - margin_px,
        right: (rect.right() * scale).ceil() as i32 + margin_px,
        bottom: (rect.bottom() * scale).ceil() as i32 + margin_px,
    }
}

/// The smallest rect holding both; an empty one adds nothing.
pub fn union(a: RectI, b: RectI) -> RectI {
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    RectI {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

/// `rect` cut to the surface (0, 0, width, height).
pub fn clamp(rect: RectI, size: (i32, i32)) -> RectI {
    RectI {
        left: rect.left.clamp(0, size.0),
        top: rect.top.clamp(0, size.1),
        right: rect.right.clamp(0, size.0),
        bottom: rect.bottom.clamp(0, size.1),
    }
}

/// `rect` in dip, so its edges fall on whole physical pixels at `scale`.
pub fn to_dip(rect: RectI, scale: f32) -> Rect {
    Rect {
        x: rect.left as f32 / scale,
        y: rect.top as f32 / scale,
        w: (rect.right - rect.left) as f32 / scale,
        h: (rect.bottom - rect.top) as f32 / scale,
    }
}

/// The signature of a presented frame: all that decides whether the next one
/// may redraw only a part of the surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub size: (i32, i32),
    pub scale: f32,
    pub origin: (i32, i32),
    pub opacity: f32,
    pub fill_alpha: f32,
    pub legibility: f32,
    pub palette: Palette,
    pub ui_font: UiFont,
    pub cat: CatLook,
    pub style: Style,
    pub halo: bool,
    /// The frame moves content outside its bounds (a morph, a slide, a fade)
    /// or is a panel: it is drawn in full, and so is the one after it.
    pub force_full: bool,
    /// A play runs: it paints far outside the island's bounds (a dent, a
    /// swing, the story's art), so this frame is drawn in full, and so is
    /// the one after it.
    pub play: bool,
    /// Where the frame paints, in device pixels with the margin; `None` when
    /// the bounds cannot be trusted.
    pub bounds: Option<RectI>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    Full,
    Dirty(RectI),
}

/// How to redraw `cur` given the frame on the surface now (`prev`).
pub fn plan(prev: Option<&Drawn>, cur: &Drawn) -> Plan {
    let Some(prev) = prev else {
        return Plan::Full;
    };
    // A frame that forced a full redraw may have painted past its bounds, so
    // the frame after it cannot trust them either.
    if cur.force_full || prev.force_full {
        return Plan::Full;
    }
    // A play paints outside any bounds; the frame after it clears what it left.
    if cur.play || prev.play {
        return Plan::Full;
    }
    let (Some(a), Some(b)) = (prev.bounds, cur.bounds) else {
        return Plan::Full;
    };
    let same = prev.size == cur.size
        && prev.scale == cur.scale
        && prev.origin == cur.origin
        && prev.opacity == cur.opacity
        && prev.fill_alpha == cur.fill_alpha
        && prev.legibility == cur.legibility
        && prev.palette == cur.palette
        && prev.ui_font == cur.ui_font
        && prev.cat == cur.cat
        && prev.style == cur.style
        && prev.halo == cur.halo;
    if !same {
        return Plan::Full;
    }
    let rect = clamp(union(a, b), cur.size);
    if rect.is_empty() {
        Plan::Full
    } else {
        Plan::Dirty(rect)
    }
}

/// Everything a compact-view frame draws that can change between two
/// equalizer ticks. Two equal keys draw the same pixels, except when
/// `time_based` (the sine bars follow the clock).
#[derive(Clone, Debug, PartialEq)]
pub struct EqKey {
    /// Bumped by every message, event and input that reaches the app.
    pub draw_gen: u64,
    /// The animated shape (x, y, w, h, radius) as f32 bits, and the opacity.
    pub shape: [u32; 5],
    pub opacity: u32,
    pub eq_heights: Option<[u32; BAR_COUNT]>,
    /// The renderer would draw the sine bars: never equal to the last key.
    pub time_based: bool,
    pub robot: Option<RobotFrame>,
    pub progress: Option<Progress>,
    /// The lyric line, the line fading out (hashes) and the crossfade (bits).
    pub lyric: Option<(u64, Option<u64>, u32)>,
    pub lyric_scroll: [u32; 2],
    pub fill_alpha: u32,
    pub toast_slot_w: u32,
    pub toast_dot: bool,
    pub volume: Option<u32>,
    pub notice_close: Option<NoticeCloseView>,
    pub clock: Option<String>,
    pub view: View,
}

/// Whether an equalizer tick has to draw: nothing drawn yet, a time-based
/// frame, or any field that differs from the last drawn key.
pub fn eq_draw_needed(last: Option<&EqKey>, cur: &EqKey) -> bool {
    match last {
        None => true,
        Some(last) => cur.time_based || last != cur,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Page;

    const SCALES: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

    fn shape(x: f32, y: f32, w: f32, h: f32) -> ShapeRect {
        ShapeRect {
            x,
            y,
            w,
            h,
            radius: 16.0,
        }
    }

    fn rect_i(left: i32, top: i32, right: i32, bottom: i32) -> RectI {
        RectI {
            left,
            top,
            right,
            bottom,
        }
    }

    fn drawn(bounds: Option<RectI>) -> Drawn {
        Drawn {
            size: (640, 200),
            scale: 1.0,
            origin: (100, 0),
            opacity: 1.0,
            fill_alpha: 1.0,
            legibility: 0.0,
            palette: crate::theme::island(
                crate::theme::Accent::default(),
                crate::theme::Surface::Espresso,
            ),
            ui_font: UiFont::Sans,
            cat: CatLook::default(),
            style: Style::Notch,
            halo: false,
            force_full: false,
            play: false,
            bounds,
        }
    }

    #[test]
    fn panel_has_no_bounds() {
        let s = shape(100.0, 0.0, 300.0, 200.0);
        assert_eq!(
            painted_bounds(Style::Pill, s, ViewKind::Panel, false, true),
            None
        );
    }

    #[test]
    fn pill_bounds_are_the_shape() {
        let s = shape(100.0, 4.0, 200.0, 32.0);
        let b = painted_bounds(Style::Pill, s, ViewKind::Closed, false, false).unwrap();
        assert_eq!(b, Rect { x: 100.0, y: 4.0, w: 200.0, h: 32.0 });
    }

    #[test]
    fn notch_bounds_include_the_shoulders() {
        let s = shape(100.0, 0.0, 200.0, 32.0);
        let d = shoulder_depth(s);
        assert!(d > 0.0);
        let b = painted_bounds(Style::Notch, s, ViewKind::Closed, false, false).unwrap();
        assert_eq!(b.x, 100.0 - d);
        assert_eq!(b.right(), 300.0 + d);
        assert_eq!((b.y, b.bottom()), (0.0, 32.0));
        // Hidden paints the fill the same way.
        let h = painted_bounds(Style::Notch, s, ViewKind::Hidden, false, false).unwrap();
        assert_eq!(h, b);
    }

    #[test]
    fn halo_widens_the_bounds_only_when_it_is_drawn() {
        let s = shape(100.0, 4.0, 200.0, 32.0);
        let with = painted_bounds(Style::Pill, s, ViewKind::Closed, false, true).unwrap();
        assert_eq!(with.x, 100.0 - HALO_SIDE);
        assert_eq!(with.right(), 300.0 + HALO_SIDE);
        assert_eq!((with.y, with.bottom()), (0.0, 36.0 + HALO_BELOW));
        let plain = painted_bounds(Style::Pill, s, ViewKind::Closed, false, false).unwrap();
        // Not with the eyes, nor on a hidden island.
        assert_eq!(
            painted_bounds(Style::Pill, s, ViewKind::Closed, true, true),
            Some(plain)
        );
        assert_eq!(
            painted_bounds(Style::Pill, s, ViewKind::Hidden, false, true),
            Some(plain)
        );
    }

    #[test]
    fn device_rect_floors_ceils_and_grows_by_the_margin() {
        for k in SCALES {
            let r = Rect { x: 10.3, y: 2.2, w: 100.4, h: 30.1 };
            let m = margin_px(k);
            let d = to_device(r, k, m);
            assert_eq!(d.left, (10.3 * k).floor() as i32 - m);
            assert_eq!(d.top, (2.2 * k).floor() as i32 - m);
            assert_eq!(d.right, (110.7 * k).ceil() as i32 + m);
            assert_eq!(d.bottom, (32.3 * k).ceil() as i32 + m);
            // The exact rect is always inside, with the margin to spare.
            assert!(d.left as f32 <= r.x * k - 2.0 * k);
            assert!(d.right as f32 >= r.right() * k + 2.0 * k);
        }
    }

    #[test]
    fn margin_is_at_least_two_dip_plus_one_pixel() {
        for k in SCALES {
            assert!(margin_px(k) as f32 >= 2.0 * k + 1.0);
        }
        assert_eq!(margin_px(1.0), 3);
        assert_eq!(margin_px(1.25), 4);
        assert_eq!(margin_px(1.5), 4);
        assert_eq!(margin_px(2.0), 5);
    }

    #[test]
    fn union_covers_both_and_ignores_an_empty_one() {
        let a = rect_i(10, 0, 50, 20);
        let b = rect_i(30, 5, 90, 40);
        assert_eq!(union(a, b), rect_i(10, 0, 90, 40));
        assert_eq!(union(a, rect_i(0, 0, 0, 0)), a);
        assert_eq!(union(rect_i(7, 7, 7, 9), b), b);
    }

    #[test]
    fn clamp_cuts_to_the_surface() {
        let r = clamp(rect_i(-5, -3, 700, 250), (640, 200));
        assert_eq!(r, rect_i(0, 0, 640, 200));
        assert!(clamp(rect_i(700, 10, 720, 20), (640, 200)).is_empty());
        assert!(!clamp(rect_i(1, 1, 5, 5), (640, 200)).is_empty());
    }

    #[test]
    fn dip_conversion_lands_on_whole_pixels() {
        for k in SCALES {
            let r = rect_i(11, 3, 203, 61);
            let d = to_dip(r, k);
            assert!((d.x * k - 11.0).abs() < 1e-3);
            assert!((d.y * k - 3.0).abs() < 1e-3);
            assert!((d.right() * k - 203.0).abs() < 1e-3);
            assert!((d.bottom() * k - 61.0).abs() < 1e-3);
        }
    }

    #[test]
    fn plan_is_full_without_a_previous_frame() {
        assert_eq!(plan(None, &drawn(Some(rect_i(0, 0, 10, 10)))), Plan::Full);
    }

    #[test]
    fn plan_is_full_without_bounds_in_either_frame() {
        let a = drawn(Some(rect_i(0, 0, 10, 10)));
        let none = drawn(None);
        assert_eq!(plan(Some(&a), &none), Plan::Full);
        assert_eq!(plan(Some(&none), &a), Plan::Full);
    }

    #[test]
    fn plan_is_full_when_either_frame_forces_it() {
        let a = drawn(Some(rect_i(0, 0, 10, 10)));
        let mut forced = a.clone();
        forced.force_full = true;
        assert_eq!(plan(Some(&a), &forced), Plan::Full);
        // The frame after a forced one is full too.
        assert_eq!(plan(Some(&forced), &a), Plan::Full);
    }

    /// Two frames on the same surface that differ only in the play: `cur`
    /// runs one, `prev` does not.
    fn two_frames_differing_only_by_play() -> (Drawn, Drawn) {
        let prev = drawn(Some(rect_i(20, 0, 120, 40)));
        let mut cur = prev.clone();
        cur.play = true;
        (prev, cur)
    }

    #[test]
    fn a_running_play_forces_a_full_frame() {
        let (prev, cur) = two_frames_differing_only_by_play();
        // Without the play the same two frames are a dirty redraw, so the
        // full frame below is the play's doing and nothing else's.
        assert_eq!(
            plan(Some(&prev), &prev.clone()),
            Plan::Dirty(rect_i(20, 0, 120, 40))
        );
        assert_eq!(plan(Some(&prev), &cur), Plan::Full, "the play starts");
        assert_eq!(plan(Some(&cur), &cur.clone()), Plan::Full, "the play runs");
        // It painted far past its bounds, so the frame after it clears them.
        assert_eq!(plan(Some(&cur), &prev), Plan::Full, "the play ended");
    }

    #[test]
    fn plan_is_full_when_any_signature_field_differs() {
        let base = drawn(Some(rect_i(20, 0, 120, 40)));
        assert_eq!(
            plan(Some(&base), &base.clone()),
            Plan::Dirty(rect_i(20, 0, 120, 40))
        );
        let changed: Vec<Box<dyn Fn(&mut Drawn)>> = vec![
            Box::new(|d| d.size = (700, 200)),
            Box::new(|d| d.scale = 1.25),
            Box::new(|d| d.origin = (101, 0)),
            Box::new(|d| d.opacity = 0.5),
            Box::new(|d| d.fill_alpha = 0.4),
            Box::new(|d| d.legibility = 0.3),
            Box::new(|d| d.palette.surface ^= 0x010101),
            Box::new(|d| d.ui_font = UiFont::Mono),
            Box::new(|d| d.cat.color = crate::theme::CatColor::Black),
            Box::new(|d| d.style = Style::Pill),
            Box::new(|d| d.halo = true),
        ];
        for (i, change) in changed.iter().enumerate() {
            let mut cur = base.clone();
            change(&mut cur);
            assert_eq!(plan(Some(&base), &cur), Plan::Full, "field {i}");
        }
    }

    #[test]
    fn plan_covers_a_shrinking_and_a_growing_shape() {
        for k in SCALES {
            let m = margin_px(k);
            let size = (640, 200);
            let big = painted_bounds(
                Style::Pill,
                shape(100.0, 0.0, 300.0, 60.0),
                ViewKind::Closed,
                false,
                false,
            )
            .unwrap();
            let small = painted_bounds(
                Style::Pill,
                shape(180.0, 0.0, 120.0, 32.0),
                ViewKind::Closed,
                false,
                false,
            )
            .unwrap();
            let (big_i, small_i) = (to_device(big, k, m), to_device(small, k, m));
            let (a, b) = (drawn(Some(big_i)), drawn(Some(small_i)));
            let want = clamp(union(big_i, small_i), size);
            // Prev bigger: its pixels are cleared.
            assert_eq!(plan(Some(&a), &b), Plan::Dirty(want));
            // Prev smaller: the new pixels are covered.
            assert_eq!(plan(Some(&b), &a), Plan::Dirty(want));
            assert!(want.left <= big_i.left.max(0) && want.right >= big_i.right.min(size.0));
        }
    }

    #[test]
    fn plan_clamps_to_the_surface() {
        let a = drawn(Some(rect_i(-20, -4, 100, 40)));
        let b = drawn(Some(rect_i(30, 0, 700, 300)));
        assert_eq!(plan(Some(&a), &b), Plan::Dirty(rect_i(0, 0, 640, 200)));
    }

    #[test]
    fn plan_is_full_when_the_clamped_rect_is_empty() {
        let a = drawn(Some(rect_i(900, 0, 950, 40)));
        assert_eq!(plan(Some(&a), &a.clone()), Plan::Full);
    }

    fn key() -> EqKey {
        EqKey {
            draw_gen: 7,
            shape: [1, 2, 3, 4, 5],
            opacity: 1.0f32.to_bits(),
            eq_heights: Some([1; BAR_COUNT]),
            time_based: false,
            robot: None,
            progress: None,
            lyric: None,
            lyric_scroll: [0, 0],
            fill_alpha: 1.0f32.to_bits(),
            toast_slot_w: 0,
            toast_dot: false,
            volume: None,
            notice_close: None,
            clock: None,
            view: View::MediaCompact,
        }
    }

    #[test]
    fn eq_draws_the_first_frame() {
        assert!(eq_draw_needed(None, &key()));
    }

    #[test]
    fn eq_skips_an_identical_key() {
        let k = key();
        assert!(!eq_draw_needed(Some(&k), &k.clone()));
    }

    #[test]
    fn eq_never_skips_a_time_based_key() {
        let mut k = key();
        k.eq_heights = None;
        k.time_based = true;
        assert!(eq_draw_needed(Some(&k), &k.clone()));
    }

    #[test]
    fn eq_draws_when_any_field_differs() {
        let base = key();
        let changed: Vec<Box<dyn Fn(&mut EqKey)>> = vec![
            Box::new(|k| k.draw_gen += 1),
            Box::new(|k| k.shape[2] += 1),
            Box::new(|k| k.opacity = 0.5f32.to_bits()),
            Box::new(|k| k.eq_heights = Some([2; BAR_COUNT])),
            Box::new(|k| {
                k.robot = Some(RobotFrame {
                    expr: crate::ui::robot::Expr::Idle,
                    pose: Default::default(),
                })
            }),
            Box::new(|k| {
                k.progress = Some(Progress {
                    position_ms: 1,
                    end_ms: 10,
                })
            }),
            Box::new(|k| k.lyric = Some((1, None, 0))),
            Box::new(|k| k.lyric_scroll = [1, 0]),
            Box::new(|k| k.fill_alpha = 0.5f32.to_bits()),
            Box::new(|k| k.toast_slot_w = 1),
            Box::new(|k| k.toast_dot = true),
            Box::new(|k| k.volume = Some(0)),
            Box::new(|k| {
                k.notice_close = Some(NoticeCloseView {
                    alpha: 1.0,
                    hot: false,
                })
            }),
            Box::new(|k| k.clock = Some("12:00".into())),
            Box::new(|k| k.view = View::Idle),
            Box::new(|k| k.view = View::Panel(Page::Media)),
        ];
        for (i, change) in changed.iter().enumerate() {
            let mut cur = base.clone();
            change(&mut cur);
            assert!(eq_draw_needed(Some(&base), &cur), "field {i}");
        }
    }
}
