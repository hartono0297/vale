//! The Settings window's placement maths, pure so it is tested without a
//! window: workspace to screen coordinates, "is it still on a screen", and
//! the minimum size.

/// A rectangle in physical pixels, edges like a Win32 `RECT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rc {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rc {
    pub fn width(self) -> i32 {
        self.right - self.left
    }

    pub fn height(self) -> i32 {
        self.bottom - self.top
    }

    fn area(self) -> i64 {
        i64::from(self.width().max(0)) * i64::from(self.height().max(0))
    }

    fn intersection(self, o: Rc) -> i64 {
        Rc {
            left: self.left.max(o.left),
            top: self.top.max(o.top),
            right: self.right.min(o.right),
            bottom: self.bottom.min(o.bottom),
        }
        .area()
    }
}

/// `GetWindowPlacement`'s `rcNormalPosition` is in workspace coordinates:
/// relative to the primary monitor's work area, not to the screen, which
/// differs when the taskbar is on the top or the left. `primary_monitor` is
/// that monitor's full rectangle and `primary_work` its work area.
pub fn workspace_to_screen(r: Rc, primary_monitor: Rc, primary_work: Rc) -> Rc {
    let dx = primary_work.left - primary_monitor.left;
    let dy = primary_work.top - primary_monitor.top;
    Rc {
        left: r.left + dx,
        top: r.top + dy,
        right: r.right + dx,
        bottom: r.bottom + dy,
    }
}

/// At least half of `r` lies on one monitor (`monitors` are the full
/// rectangles), so the window can still be reached. A window with no area is
/// never visible.
pub fn mostly_visible(r: Rc, monitors: &[Rc]) -> bool {
    let area = r.area();
    area > 0 && monitors.iter().any(|m| r.intersection(*m) * 2 >= area)
}

/// `r` grown to the right and down until its outer size is at least
/// `min_w` x `min_h`.
pub fn clamp_min(r: Rc, min_w: i32, min_h: i32) -> Rc {
    Rc {
        right: r.left + r.width().max(min_w),
        bottom: r.top + r.height().max(min_h),
        ..r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rc(left: i32, top: i32, right: i32, bottom: i32) -> Rc {
        Rc {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn workspace_coordinates_gain_the_work_area_offset() {
        let mon = rc(0, 0, 1920, 1080);
        // Taskbar at the bottom: no offset.
        let r = rc(100, 80, 820, 600);
        assert_eq!(workspace_to_screen(r, mon, rc(0, 0, 1920, 1040)), r);
        // Taskbar on the top (48 px): everything moves down by 48.
        assert_eq!(
            workspace_to_screen(r, mon, rc(0, 48, 1920, 1080)),
            rc(100, 128, 820, 648)
        );
        // Taskbar on the left (60 px).
        assert_eq!(
            workspace_to_screen(r, mon, rc(60, 0, 1920, 1080)),
            rc(160, 80, 880, 600)
        );
        // A primary monitor that is not at the origin.
        assert_eq!(
            workspace_to_screen(r, rc(-1920, 0, 0, 1080), rc(-1920, 40, 0, 1080)),
            rc(100, 120, 820, 640)
        );
    }

    #[test]
    fn a_window_needs_half_its_area_on_one_monitor() {
        let monitors = [rc(0, 0, 1000, 800), rc(1000, 0, 2000, 800)];
        // Wholly on one.
        assert!(mostly_visible(rc(100, 100, 500, 400), &monitors));
        // Exactly half off the right edge of the second.
        assert!(mostly_visible(rc(1800, 100, 2200, 400), &monitors));
        // A little less than half.
        assert!(!mostly_visible(rc(1801, 100, 2201, 400), &monitors));
        // Straddling two monitors: each holds half, so the larger is half.
        assert!(mostly_visible(rc(800, 100, 1200, 400), &monitors));
        // Off every screen, or no monitors at all.
        assert!(!mostly_visible(rc(5000, 5000, 5400, 5300), &monitors));
        assert!(!mostly_visible(rc(100, 100, 500, 400), &[]));
        // No area.
        assert!(!mostly_visible(rc(100, 100, 100, 400), &monitors));
        // Over the gap between two monitors, neither holds half.
        let apart = [rc(0, 0, 1000, 800), rc(1200, 0, 2200, 800)];
        assert!(!mostly_visible(rc(900, 100, 1300, 400), &apart));
    }

    #[test]
    fn clamp_min_grows_right_and_bottom_only() {
        assert_eq!(
            clamp_min(rc(10, 20, 110, 70), 300, 200),
            rc(10, 20, 310, 220)
        );
        let big = rc(10, 20, 810, 620);
        assert_eq!(clamp_min(big, 300, 200), big);
        assert_eq!(clamp_min(rc(0, 0, 400, 50), 300, 200), rc(0, 0, 400, 200));
    }
}
