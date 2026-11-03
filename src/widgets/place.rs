//! Turning a sticky's saved place (dip, relative to its monitor's placement
//! area, with the monitor's device path) into a window rectangle in physical
//! pixels on the screens that are connected now, and back.
//!
//! A monitor that is gone is not an error: the note shows on the primary
//! monitor, clamped inside it, and its saved monitor and rect are left alone,
//! so it returns when the monitor does (`notch_monitor`'s rule). A note the
//! user moves while on the fallback saves its new monitor.

use crate::capture::Rect;
use crate::notch_monitor::{Display, find_path};
use crate::widgets::paint::shown_h;
use crate::widgets::snap::{self, clamp_rect, clamp_rect_for, reserved_bottom_dip};
use crate::widgets::sticky::{DipRect, Sticky};

/// Where a sticky is shown now.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// The display it is on (its place in the list given).
    pub display: usize,
    /// The window rect of the note, physical px, desktop coordinates.
    pub rect: Rect,
    /// The note as shown, dip from the area's top-left: the saved rect with
    /// its size inside the limits and the area, and its place inside the
    /// area. `h` is the height unrolled (a rolled-up note shows `ROLLED_H` of
    /// it). The window, the layout and the hit-testing use this size, never
    /// the saved one, which can be larger than this area.
    pub view: DipRect,
    /// The monitor's DPI over 96.
    pub scale: f32,
    /// The placement area, physical px, desktop coordinates.
    pub area: Rect,
    /// Its own monitor is not connected: shown on the primary instead.
    pub fallback: bool,
}

/// DPI over 96.
pub fn scale_of(dpi: u32) -> f32 {
    dpi.max(96) as f32 / 96.0
}

/// A monitor's placement area in physical px: its work area less the zone the
/// Dock reserves at the bottom.
pub fn area_px(d: &Display) -> Rect {
    let scale = scale_of(d.monitor.dpi);
    let work = d.monitor.work;
    let reserved = (reserved_bottom_dip() * scale).round() as i32;
    Rect::new(work.x, work.y, work.w, (work.h - reserved).max(0))
}

/// The placement area in dip, at the origin.
pub fn area_dip(d: &Display) -> DipRect {
    let a = area_px(d);
    let s = scale_of(d.monitor.dpi);
    DipRect::new(0.0, 0.0, a.w as f32 / s, a.h as f32 / s)
}

/// `rel` (dip from the area's top-left) in physical px.
pub fn to_px(rel: &DipRect, area: &Rect, scale: f32) -> Rect {
    Rect::new(
        area.x + (rel.x * scale).round() as i32,
        area.y + (rel.y * scale).round() as i32,
        (rel.w * scale).round() as i32,
        (rel.h * scale).round() as i32,
    )
}

/// `px` (desktop coordinates) as dip from the area's top-left.
pub fn to_dip(px: &Rect, area: &Rect, scale: f32) -> DipRect {
    DipRect::new(
        (px.x - area.x) as f32 / scale,
        (px.y - area.y) as f32 / scale,
        px.w as f32 / scale,
        px.h as f32 / scale,
    )
}

/// The display a sticky's monitor path names; the primary (or the first) when
/// the path is empty or not connected, with whether that was a fallback.
/// `None` for an empty list.
pub fn resolve(path: &str, displays: &[Display]) -> Option<(usize, bool)> {
    let primary = || {
        displays
            .iter()
            .position(|d| d.primary)
            .or(if displays.is_empty() { None } else { Some(0) })
    };
    if path.is_empty() {
        return primary().map(|i| (i, false));
    }
    match find_path(displays, path) {
        Some(i) => Some((i, false)),
        None => primary().map(|i| (i, true)),
    }
}

/// The note of saved rect `saved` as shown on `area` (dip, at the origin):
/// see `Placement::view`. A rolled-up note is placed at its shown height.
pub fn view_on(saved: DipRect, rolled: bool, area: &DipRect) -> DipRect {
    let shown = DipRect {
        h: shown_h(saved.h, rolled),
        ..saved
    };
    let rel = clamp_rect_for(shown, area, rolled);
    let full = clamp_rect(saved, area);
    DipRect { h: full.h, ..rel }
}

/// Where `sticky` is shown on `displays`. `None` when there is no display.
pub fn place(sticky: &Sticky, displays: &[Display]) -> Option<Placement> {
    let (display, fallback) = resolve(&sticky.monitor, displays)?;
    let d = &displays[display];
    let scale = scale_of(d.monitor.dpi);
    let area = area_px(d);
    // Clamped for the screen it is shown on, not written back.
    let view = view_on(sticky.rect, sticky.rolled, &area_dip(d));
    let shown = DipRect {
        h: shown_h(view.h, sticky.rolled),
        ..view
    };
    Some(Placement {
        display,
        rect: to_px(&shown, &area, scale),
        view,
        scale,
        area,
        fallback,
    })
}

/// The display holding the point, else the nearest one by centre distance.
pub fn display_at(displays: &[Display], point: (i32, i32)) -> Option<usize> {
    if let Some(i) = displays.iter().position(|d| d.monitor.rect.contains(point)) {
        return Some(i);
    }
    let dist = |d: &Display| {
        let r = d.monitor.rect;
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        i64::from(cx - point.0).pow(2) + i64::from(cy - point.1).pow(2)
    };
    (0..displays.len()).min_by_key(|i| dist(&displays[*i]))
}

/// The monitor path to save for a note that now sits on `d`: always its own
/// path, so the note follows that monitor, not Windows' choice of primary.
pub fn path_to_save(d: &Display) -> String {
    d.path.clone()
}

/// The notes on `display` as dip rects (for snapping), skipping `except`.
pub fn others_on(
    notes: &[(usize, &Sticky)],
    displays: &[Display],
    display: usize,
    except: usize,
) -> Vec<DipRect> {
    notes
        .iter()
        .filter(|(id, _)| *id != except)
        .filter(|(_, s)| resolve(&s.monitor, displays).is_some_and(|(i, _)| i == display))
        .map(|(_, s)| s.rect)
        .collect()
}

/// The area in dip of the placement area of `d`, for the snap module.
pub fn snap_area(d: &Display) -> DipRect {
    let a = area_dip(d);
    snap::placement_area(a.w, a.h, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrange::Monitor;
    use crate::widgets::snap::clamp_rect;
    use crate::widgets::sticky::ROLLED_H;

    fn display(n: usize, x: i32, primary: bool, path: &str, dpi: u32) -> Display {
        let (w, h) = (1920, 1080);
        Display {
            monitor: Monitor {
                device: format!(r"\\.\DISPLAY{n}"),
                rect: Rect::new(x, 0, w, h),
                work: Rect::new(x, 0, w, h - 40),
                dpi,
            },
            hmonitor: n as isize,
            primary,
            path: path.to_owned(),
            name: String::new(),
            number: n,
        }
    }

    fn sticky_on(path: &str, rect: DipRect) -> Sticky {
        Sticky {
            monitor: path.to_owned(),
            rect,
            ..Sticky::default()
        }
    }

    fn two() -> Vec<Display> {
        vec![
            display(1, 0, true, "A", 96),
            display(2, 1920, false, "B", 96),
        ]
    }

    #[test]
    fn dip_and_pixels_convert_with_the_dpi() {
        let area = Rect::new(100, 50, 1000, 600);
        let rel = DipRect::new(10.0, 20.0, 240.0, 200.0);
        for (dpi, factor) in [(96u32, 1.0f32), (120, 1.25), (144, 1.5), (192, 2.0)] {
            assert_eq!(scale_of(dpi), factor);
            let px = to_px(&rel, &area, factor);
            assert_eq!(px.x, 100 + (10.0 * factor).round() as i32);
            assert_eq!(px.w, (240.0 * factor).round() as i32);
            let back = to_dip(&px, &area, factor);
            assert!((back.x - rel.x).abs() < 1.0 / factor, "{dpi}");
            assert!((back.w - rel.w).abs() < 1.0 / factor, "{dpi}");
        }
        assert_eq!(scale_of(0), 1.0, "a missing dpi is 96");
    }

    #[test]
    fn a_connected_monitor_is_used_as_saved() {
        let d = two();
        let s = sticky_on("B", DipRect::new(16.0, 24.0, 240.0, 208.0));
        let p = place(&s, &d).unwrap();
        assert_eq!(p.display, 1);
        assert!(!p.fallback);
        assert_eq!(p.rect, Rect::new(1920 + 16, 24, 240, 208));
        // Any case of the path finds it.
        let s = sticky_on("b", DipRect::new(16.0, 24.0, 240.0, 208.0));
        assert_eq!(place(&s, &d).unwrap().display, 1);
    }

    #[test]
    fn the_empty_path_is_the_primary_monitor() {
        let d = two();
        let s = sticky_on("", DipRect::new(0.0, 0.0, 240.0, 208.0));
        let p = place(&s, &d).unwrap();
        assert_eq!((p.display, p.fallback), (0, false));
        // The primary need not be first in the list.
        let swapped = vec![
            display(1, 0, false, "A", 96),
            display(2, 1920, true, "B", 96),
        ];
        assert_eq!(place(&s, &swapped).unwrap().display, 1);
    }

    #[test]
    fn a_missing_monitor_falls_back_to_the_primary_clamped_and_leaves_the_save_alone() {
        let one = vec![display(1, 0, true, "A", 96)];
        // On a wide second monitor at x 3000 dip: off the 1920 wide primary.
        let saved = DipRect::new(3000.0, 500.0, 240.0, 208.0);
        let s = sticky_on("B", saved);
        let p = place(&s, &one).unwrap();
        assert!(p.fallback);
        assert_eq!(p.display, 0);
        assert_eq!(p.rect, Rect::new(1920 - 240, 500, 240, 208), "clamped inside");
        // Nothing was written back.
        assert_eq!(s.monitor, "B");
        assert_eq!(s.rect, saved);
        // The monitor comes back: it returns to where it was.
        let mut wide = two();
        wide[1].monitor.rect.w = 3840;
        wide[1].monitor.work.w = 3840;
        let back = place(&s, &wide).unwrap();
        assert!(!back.fallback);
        assert_eq!(back.display, 1);
        assert_eq!(back.rect, Rect::new(1920 + 3000, 500, 240, 208));
    }

    #[test]
    fn a_user_move_on_the_fallback_saves_the_new_monitor() {
        let one = vec![display(1, 0, true, "A", 96)];
        let mut s = sticky_on("B", DipRect::new(3000.0, 500.0, 240.0, 208.0));
        let p = place(&s, &one).unwrap();
        assert!(p.fallback);
        // Dragged a little on the primary: what the host saves at the drop is
        // the display it is on (`path_to_save`) and the rect in that display's dip.
        let moved = Rect::new(p.rect.x - 40, p.rect.y - 40, p.rect.w, p.rect.h);
        s.rect = clamp_rect(to_dip(&moved, &area_px(&one[0]), p.scale), &area_dip(&one[0]));
        s.monitor = path_to_save(&one[0]);
        assert_eq!(s.monitor, "A");
        let again = place(&s, &one).unwrap();
        assert!(!again.fallback);
        assert_eq!(again.rect, moved);
    }

    #[test]
    fn dpi_scales_the_rect_and_the_area() {
        let mut d = two();
        d[0].monitor.dpi = 144;
        let s = sticky_on("A", DipRect::new(16.0, 24.0, 240.0, 208.0));
        let p = place(&s, &d).unwrap();
        assert_eq!(p.scale, 1.5);
        assert_eq!(p.rect, Rect::new(24, 36, 360, 312));
        let area = area_dip(&d[0]);
        assert_eq!(area.w, 1280.0);
        assert!((area.h - 693.333).abs() < 0.01, "{area:?}");
        // A move at 150 % saves dip.
        let moved = to_dip(&Rect::new(48, 60, 360, 312), &area_px(&d[0]), 1.5);
        assert_eq!(moved, DipRect::new(32.0, 40.0, 240.0, 208.0));
    }

    #[test]
    fn a_saved_rect_off_its_monitor_is_clamped_when_shown() {
        let d = two();
        let s = sticky_on("A", DipRect::new(5000.0, -20.0, 240.0, 208.0));
        let p = place(&s, &d).unwrap();
        assert_eq!(p.rect.right(), 1920);
        assert_eq!(p.rect.y, 0);
    }

    #[test]
    fn a_rolled_note_is_placed_at_its_shown_height() {
        let d = two();
        // Shown 56 tall, in the bottom 64 dip.
        let mut s = sticky_on("A", DipRect::new(16.0, 1040.0, 240.0, 208.0));
        s.rolled = true;
        let p = place(&s, &d).unwrap();
        assert_eq!(p.rect.h, ROLLED_H as i32);
        assert_eq!(p.rect.bottom(), 1040, "the area is 1040 tall; it sits on its edge");
        assert_eq!(p.view.h, 208.0, "the unrolled height is kept for unrolling");
        let mut s = sticky_on("A", DipRect::new(16.0, 900.0, 240.0, 208.0));
        s.rolled = true;
        let p = place(&s, &d).unwrap();
        assert_eq!((p.rect.y, p.rect.h), (900, ROLLED_H as i32), "not raised to 120");
    }

    #[test]
    fn a_note_larger_than_the_area_is_shown_at_the_area_size() {
        // Resized to the largest size on a big monitor, shown on 1920 x 1080
        // at 150 %: the area is 1280 x 693.3 dip.
        let mut d = two();
        d[0].monitor.dpi = 144;
        let saved = DipRect::new(400.0, 200.0, 640.0, 800.0);
        let s = sticky_on("A", saved);
        let p = place(&s, &d).unwrap();
        let area = area_px(&d[0]);
        assert!((p.view.h - 693.333).abs() < 0.01, "{:?}", p.view);
        assert_eq!((p.view.y, p.view.w), (0.0, 640.0));
        // The window is the view, fully inside the area (never over the taskbar).
        assert_eq!(p.rect, to_px(&p.view, &area, 1.5));
        assert_eq!(p.rect.y, area.y);
        assert!(p.rect.bottom() <= area.bottom(), "{:?} in {area:?}", p.rect);
        assert!(p.rect.right() <= area.right());
        // The saved rect is untouched: on its own monitor it comes back.
        assert_eq!(s.rect, saved);
        let back = place(&s, &two()).unwrap();
        assert_eq!(back.view, saved);
        // Rolled up, it still unrolls to the area's height, not the saved one.
        let mut rolled = s.clone();
        rolled.rolled = true;
        let p = place(&rolled, &d).unwrap();
        assert_eq!(p.rect.h, (ROLLED_H * 1.5).round() as i32);
        assert!((p.view.h - 693.333).abs() < 0.01);
    }

    #[test]
    fn no_display_places_nothing() {
        assert_eq!(place(&Sticky::default(), &[]), None);
        assert_eq!(resolve("A", &[]), None);
        assert_eq!(display_at(&[], (0, 0)), None);
    }

    #[test]
    fn the_display_under_a_point_or_the_nearest() {
        let d = two();
        assert_eq!(display_at(&d, (100, 100)), Some(0));
        assert_eq!(display_at(&d, (2000, 500)), Some(1));
        assert_eq!(display_at(&d, (-300, 500)), Some(0));
        assert_eq!(display_at(&d, (9000, 500)), Some(1));
    }

    #[test]
    fn neighbours_are_the_notes_on_the_same_display() {
        let d = two();
        let a = sticky_on("A", DipRect::new(0.0, 0.0, 200.0, 150.0));
        let b = sticky_on("B", DipRect::new(10.0, 10.0, 200.0, 150.0));
        let c = sticky_on("", DipRect::new(300.0, 10.0, 200.0, 150.0));
        let all = [(0usize, &a), (1, &b), (2, &c)];
        let on_first = others_on(&all, &d, 0, 0);
        assert_eq!(on_first, vec![c.rect], "c is on the primary, a is the one moved");
        assert_eq!(others_on(&all, &d, 1, 9), vec![b.rect]);
    }
}
