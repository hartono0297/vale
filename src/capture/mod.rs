//! Vale's own region capture: the screen is frozen once, the overlay snaps
//! to the window, panel or box under the pointer, and the chosen part goes
//! straight to the screenshot editor (`shot_edit`).
//!
//! This file is the pure part (snapping, click and drag, cropping); `win`
//! grabs the screen and lists the windows, `uia` finds the boxes inside
//! them on a short-lived worker, `overlay` is the window. Nothing here runs
//! while no capture is up, and nothing is kept: the frozen frame is freed
//! when the overlay closes.

#[cfg(debug_assertions)]
pub mod demo;
pub mod overlay;
pub mod uia;
pub mod win;

/// A rectangle in physical pixels (virtual-screen coordinates for the
/// frame and the candidates, image coordinates inside a crop).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn area(&self) -> i64 {
        i64::from(self.w.max(0)) * i64::from(self.h.max(0))
    }

    /// Half-open: the right and bottom edges are outside.
    pub fn contains(&self, pt: (i32, i32)) -> bool {
        pt.0 >= self.x && pt.0 < self.right() && pt.1 >= self.y && pt.1 < self.bottom()
    }

    /// The part both share; empty (zero size) when they do not meet.
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        Rect {
            x,
            y,
            w: (r - x).max(0),
            h: (b - y).max(0),
        }
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

/// Smaller boxes than this (either side) are never snapped to: they are
/// icons' hit areas and separators, not something to cut out.
pub const MIN_SNAP: i32 = 12;

/// A pointer that moved this far (or less) between press and release is a
/// click.
pub const CLICK_SLOP: i32 = 4;

/// Something the pointer can snap to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Physical pixels, virtual-screen coordinates.
    pub rect: Rect,
    /// Position of its top-level window in the z-order, 0 the frontmost.
    pub z: u32,
    /// 0 for the window itself, more for an element inside it.
    pub depth: u16,
}

/// What a click at `pt` would capture: the frontmost top-level window that
/// holds `pt`, and inside it the smallest box that does (the window itself
/// when no element is smaller). Boxes under `MIN_SNAP` are skipped, and
/// elements of windows behind the frontmost one never win.
pub fn pick(cands: &[Candidate], pt: (i32, i32)) -> Option<Rect> {
    let z = cands
        .iter()
        .filter(|c| c.depth == 0 && c.rect.contains(pt))
        .map(|c| c.z)
        .min()?;
    cands
        .iter()
        .filter(|c| c.z == z && c.rect.contains(pt) && c.rect.w >= MIN_SNAP && c.rect.h >= MIN_SNAP)
        // The smallest; of two equal boxes the deeper one.
        .min_by_key(|c| (c.rect.area(), std::cmp::Reverse(c.depth)))
        .map(|c| c.rect)
}

/// A press and a release that stayed within `CLICK_SLOP` px (each axis).
pub fn is_click(down: (i32, i32), up: (i32, i32)) -> bool {
    (up.0 - down.0).abs() <= CLICK_SLOP && (up.1 - down.1).abs() <= CLICK_SLOP
}

/// The rectangle of a drag from `a` to `b`, in any direction, kept inside
/// `bounds` (the virtual screen).
pub fn drag_rect(a: (i32, i32), b: (i32, i32), bounds: Rect) -> Rect {
    let clamp = |p: (i32, i32)| {
        (
            p.0.clamp(bounds.x, bounds.right().max(bounds.x)),
            p.1.clamp(bounds.y, bounds.bottom().max(bounds.y)),
        )
    };
    let (a, b) = (clamp(a), clamp(b));
    Rect {
        x: a.0.min(b.0),
        y: a.1.min(b.1),
        w: (a.0 - b.0).abs(),
        h: (a.1 - b.1).abs(),
    }
}

/// A picture of 32-bit BGRA pixels, top-down, `w * h * 4` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shot {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
}

/// `sel` cut out of a frame of `src` (its place in the virtual screen)
/// whose rows are `stride` bytes apart. `sel` is clipped to the frame;
/// `None` when nothing is left or the buffer is too short.
pub fn crop(bgra: &[u8], stride: usize, src: Rect, sel: Rect) -> Option<Shot> {
    let s = sel.intersect(&src);
    if s.is_empty() || src.w <= 0 || src.h <= 0 {
        return None;
    }
    if stride < src.w as usize * 4 || bgra.len() < stride * src.h as usize {
        return None;
    }
    let (w, h) = (s.w as usize, s.h as usize);
    let (cx, cy) = ((s.x - src.x) as usize, (s.y - src.y) as usize);
    let mut out = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let from = (cy + row) * stride + cx * 4;
        out.extend_from_slice(&bgra[from..from + w * 4]);
    }
    Some(Shot {
        w: w as u32,
        h: h as u32,
        bgra: out,
    })
}

/// "1280 × 720", the size label beside the highlight.
pub fn size_label(r: Rect) -> String {
    format!("{} × {}", r.w, r.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(x: i32, y: i32, w: i32, h: i32, z: u32) -> Candidate {
        Candidate {
            rect: Rect::new(x, y, w, h),
            z,
            depth: 0,
        }
    }

    fn inner(x: i32, y: i32, w: i32, h: i32, z: u32) -> Candidate {
        Candidate {
            rect: Rect::new(x, y, w, h),
            z,
            depth: 1,
        }
    }

    #[test]
    fn the_smallest_box_of_the_frontmost_window_wins() {
        let cands = [
            win(0, 0, 800, 600, 0),
            inner(10, 40, 780, 500, 0),
            inner(20, 60, 200, 100, 0),
            inner(30, 70, 50, 20, 0),
        ];
        assert_eq!(pick(&cands, (35, 75)), Some(Rect::new(30, 70, 50, 20)));
        assert_eq!(pick(&cands, (100, 100)), Some(Rect::new(20, 60, 200, 100)));
        assert_eq!(pick(&cands, (500, 300)), Some(Rect::new(10, 40, 780, 500)));
        assert_eq!(pick(&cands, (5, 5)), Some(Rect::new(0, 0, 800, 600)));
    }

    #[test]
    fn a_window_in_front_hides_the_boxes_of_the_ones_behind() {
        let cands = [
            win(100, 100, 300, 200, 0),
            inner(110, 130, 60, 30, 0),
            win(0, 0, 800, 600, 1),
            inner(120, 140, 40, 20, 1),
            inner(500, 400, 100, 50, 1),
        ];
        // Inside the front window: its own small box, not the one behind.
        assert_eq!(pick(&cands, (125, 145)), Some(Rect::new(110, 130, 60, 30)));
        assert_eq!(
            pick(&cands, (300, 250)),
            Some(Rect::new(100, 100, 300, 200))
        );
        // Outside it the one behind shows through.
        assert_eq!(pick(&cands, (510, 410)), Some(Rect::new(500, 400, 100, 50)));
        assert_eq!(pick(&cands, (50, 50)), Some(Rect::new(0, 0, 800, 600)));
    }

    #[test]
    fn z_decides_not_the_order_of_the_list() {
        let cands = [win(0, 0, 800, 600, 3), win(0, 0, 400, 300, 1)];
        assert_eq!(pick(&cands, (10, 10)), Some(Rect::new(0, 0, 400, 300)));
        let swapped = [cands[1], cands[0]];
        assert_eq!(pick(&swapped, (10, 10)), Some(Rect::new(0, 0, 400, 300)));
    }

    #[test]
    fn tiny_boxes_are_skipped() {
        let cands = [
            win(0, 0, 400, 300, 0),
            inner(50, 50, 11, 40, 0),
            inner(50, 120, 40, 11, 0),
            inner(50, 200, 12, 12, 0),
        ];
        assert_eq!(pick(&cands, (55, 70)), Some(Rect::new(0, 0, 400, 300)));
        assert_eq!(pick(&cands, (60, 125)), Some(Rect::new(0, 0, 400, 300)));
        assert_eq!(pick(&cands, (55, 205)), Some(Rect::new(50, 200, 12, 12)));
    }

    #[test]
    fn nothing_under_the_pointer_is_nothing() {
        assert_eq!(pick(&[], (5, 5)), None);
        let cands = [win(0, 0, 100, 100, 0), inner(10, 10, 50, 50, 0)];
        assert_eq!(pick(&cands, (500, 500)), None);
        // The right and bottom edges are outside.
        assert_eq!(pick(&cands, (100, 50)), None);
        // Elements alone (no window found) never snap.
        assert_eq!(pick(&[inner(10, 10, 50, 50, 0)], (20, 20)), None);
    }

    #[test]
    fn equal_boxes_prefer_the_one_inside() {
        let cands = [win(0, 0, 200, 200, 0), inner(0, 0, 200, 200, 0)];
        let picked = pick(&cands, (5, 5));
        assert_eq!(picked, Some(Rect::new(0, 0, 200, 200)));
    }

    #[test]
    fn a_click_is_a_press_and_release_within_four_pixels() {
        assert!(is_click((100, 100), (100, 100)));
        assert!(is_click((100, 100), (104, 96)));
        assert!(!is_click((100, 100), (105, 100)));
        assert!(!is_click((100, 100), (100, 95)));
    }

    #[test]
    fn a_drag_gives_the_same_rectangle_in_every_direction() {
        let screen = Rect::new(-1920, 0, 3840, 1080);
        let want = Rect::new(100, 200, 300, 150);
        for (a, b) in [
            ((100, 200), (400, 350)),
            ((400, 350), (100, 200)),
            ((100, 350), (400, 200)),
            ((400, 200), (100, 350)),
        ] {
            assert_eq!(drag_rect(a, b, screen), want, "{a:?} to {b:?}");
        }
    }

    #[test]
    fn a_drag_stays_inside_the_virtual_screen() {
        let screen = Rect::new(-1920, 0, 3840, 1080);
        assert_eq!(
            drag_rect((-2500, -50), (100, 100), screen),
            Rect::new(-1920, 0, 2020, 100)
        );
        assert_eq!(
            drag_rect((1900, 1000), (5000, 3000), screen),
            Rect::new(1900, 1000, 20, 80)
        );
        let none = drag_rect((5000, 5000), (6000, 6000), screen);
        assert!(none.is_empty());
    }

    fn frame(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                v.extend([x as u8, y as u8, 7, 255]);
            }
        }
        v
    }

    #[test]
    fn a_crop_copies_the_selection_rows() {
        let f = frame(10, 6);
        // The frame sits at (-5, 100) in the virtual screen.
        let src = Rect::new(-5, 100, 10, 6);
        let c = crop(&f, 40, src, Rect::new(-3, 101, 4, 2)).expect("a crop");
        assert_eq!((c.w, c.h), (4, 2));
        assert_eq!(c.bgra.len(), 4 * 2 * 4);
        // First pixel is frame (2, 1), the last frame (5, 2).
        assert_eq!(&c.bgra[0..4], &[2, 1, 7, 255]);
        assert_eq!(&c.bgra[c.bgra.len() - 4..], &[5, 2, 7, 255]);
    }

    #[test]
    fn a_crop_is_clipped_to_the_frame_and_honours_the_stride() {
        // Rows padded to 48 bytes.
        let mut padded = Vec::new();
        for y in 0..6usize {
            let row = &frame(10, 6)[y * 40..(y + 1) * 40];
            padded.extend_from_slice(row);
            padded.extend([0xEE; 8]);
        }
        let src = Rect::new(0, 0, 10, 6);
        let c = crop(&padded, 48, src, Rect::new(8, 4, 20, 20)).expect("clipped");
        assert_eq!((c.w, c.h), (2, 2));
        assert_eq!(&c.bgra[0..4], &[8, 4, 7, 255]);
        assert_eq!(&c.bgra[12..16], &[9, 5, 7, 255]);
        assert!(crop(&padded, 48, src, Rect::new(20, 20, 5, 5)).is_none());
        assert!(crop(&padded[..100], 48, src, Rect::new(0, 0, 5, 5)).is_none());
        assert!(crop(&padded, 20, src, Rect::new(0, 0, 5, 5)).is_none());
    }

    #[test]
    fn the_label_reads_width_by_height() {
        assert_eq!(size_label(Rect::new(0, 0, 1280, 720)), "1280 × 720");
    }
}
