//! Images on a sticky note: the pure rules (what a file name may be, how many
//! images fit, the grid they are drawn in, which files go away and which are
//! left over). The Win32 side (WIC decode, the dialogs, the worker) is
//! `image_io`.
//!
//! A note keeps only file NAMES (`sticky.images`); the files live in the
//! `note-images` folder next to `notes.json`. Every name read from JSON goes
//! through `valid_name` before any path is joined, opened or deleted.

use std::collections::HashSet;

use crate::ui::geometry::Rect;

/// At most this many images on one note.
pub const MAX_IMAGES: usize = 4;
/// A source file bigger than this is refused before it is decoded.
pub const MAX_SOURCE_BYTES: u64 = 25 * 1024 * 1024;
/// The longest side of a saved copy, in pixels.
pub const MAX_SIDE: u32 = 1600;
/// An unreferenced file younger than this is kept (a note may still be being
/// written, or an undo may bring it back).
pub const ORPHAN_AGE_SECS: i64 = 24 * 60 * 60;
/// The grid's side padding, the gap between cells and the gap below the block.
pub const SIDE: f32 = 8.0;
pub const CELL_GAP: f32 = 4.0;
pub const BELOW: f32 = 8.0;
/// The block is at most this much of the note's height.
pub const MAX_BLOCK: f32 = 0.45;

/// The only names the folder holds: `[A-Za-z0-9._-]+`, ending in `.png`,
/// `.jpg` or `.jpeg`, no path separator, no `..`, at most 64 characters, and
/// no Windows device name (`CON.png` still opens the console device).
///
/// SECURITY: this guards every join of a name read from `notes.json` with the
/// images folder (open, delete), so a hand-edited file can not point outside
/// it.
pub fn valid_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 || name.contains("..") {
        return false;
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    let Some(stem) = [".png", ".jpg", ".jpeg"]
        .iter()
        .find_map(|ext| lower.strip_suffix(ext))
    else {
        return false;
    };
    if stem.is_empty() || stem.starts_with('.') {
        return false;
    }
    // Reserved device names, with or without further dots.
    let device = stem.split('.').next().unwrap_or_default();
    let reserved = matches!(device, "con" | "prn" | "aux" | "nul")
        || (device.len() == 4
            && (device.starts_with("com") || device.starts_with("lpt"))
            && matches!(device.as_bytes()[3], b'1'..=b'9'));
    !reserved
}

/// A path that names an image file by its extension: png, jpg, jpeg, gif,
/// bmp or webp (any case).
pub fn is_image_path(p: &str) -> bool {
    let name = p.rsplit(['\\', '/']).next().unwrap_or(p);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && matches!(
            ext.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp"
        )
}

/// Dropped files: the images first, the others second, each in order.
pub fn split_files(paths: &[String]) -> (Vec<String>, Vec<String>) {
    paths
        .iter()
        .cloned()
        .partition(|p| is_image_path(p))
}

/// Of `n` new images for a note that has `current` and `pending` imports in
/// flight: how many are taken and how many are refused.
pub fn admit(current: usize, pending: usize, n: usize) -> (usize, usize) {
    let room = MAX_IMAGES.saturating_sub(current + pending);
    let take = n.min(room);
    (take, n - take)
}

fn cell_aspect(a: Option<&f32>) -> f32 {
    match a {
        Some(a) if a.is_finite() && *a > 0.01 => *a,
        _ => 4.0 / 3.0,
    }
}

/// Where `n` images go in a note `note_w` x `note_h` dip, and how tall the
/// block is (with the gap below it). The rects are in dip from the text
/// area's top, in the note's x (so the 8 dip side padding is real): the text
/// starts `block_h` below. One image is full width, as high as its aspect
/// says (4:3 when unknown) up to 45 % of the note; two sit side by side; three
/// are two cells and then one full-width cell; four are 2 x 2. Cells are
/// cover-cropped by the painter.
pub fn grid(n: usize, note_w: f32, note_h: f32, aspects: &[f32]) -> (Vec<Rect>, f32) {
    if n == 0 {
        return (Vec::new(), 0.0);
    }
    let n = n.min(MAX_IMAGES);
    let width = (note_w - 2.0 * SIDE).max(1.0);
    let cap = (MAX_BLOCK * note_h).max(1.0);
    let cell_w = (width - CELL_GAP) / 2.0;
    let rect = |x: f32, y: f32, w: f32, h: f32| Rect { x, y, w, h };
    let mut out = Vec::new();
    let bottom = match n {
        1 => {
            let h = (width / cell_aspect(aspects.first())).min(cap).max(1.0);
            out.push(rect(SIDE, 0.0, width, h));
            h
        }
        2 => {
            let h = (cell_w * 0.75).min(cap).max(1.0);
            out.push(rect(SIDE, 0.0, cell_w, h));
            out.push(rect(SIDE + cell_w + CELL_GAP, 0.0, cell_w, h));
            h
        }
        3 => {
            let h = (cell_w * 0.75).min((cap - CELL_GAP) / 2.0).max(1.0);
            out.push(rect(SIDE, 0.0, cell_w, h));
            out.push(rect(SIDE + cell_w + CELL_GAP, 0.0, cell_w, h));
            out.push(rect(SIDE, h + CELL_GAP, width, h));
            2.0 * h + CELL_GAP
        }
        _ => {
            let h = (cell_w * 0.75).min((cap - CELL_GAP) / 2.0).max(1.0);
            for row in 0..2 {
                let y = row as f32 * (h + CELL_GAP);
                out.push(rect(SIDE, y, cell_w, h));
                out.push(rect(SIDE + cell_w + CELL_GAP, y, cell_w, h));
            }
            2.0 * h + CELL_GAP
        }
    };
    (out, bottom + BELOW)
}

/// The files to delete after a save: what was referenced before (`known`) and
/// is not now. Sorted, so the result is stable.
pub fn to_release(known: &HashSet<String>, now_referenced: &HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = known
        .iter()
        .filter(|n| !now_referenced.contains(*n))
        .cloned()
        .collect();
    out.sort();
    out
}

/// The files of the folder (`(name, modified unix seconds)`) that nothing
/// references and that are older than 24 hours at `now`. Only valid names are
/// ever returned: a stray file the user put there is left alone.
pub fn orphans(
    listing: &[(String, i64)],
    referenced: &HashSet<String>,
    now: i64,
) -> Vec<String> {
    let mut out: Vec<String> = listing
        .iter()
        .filter(|(name, modified)| {
            valid_name(name) && !referenced.contains(name) && now - modified >= ORPHAN_AGE_SECS
        })
        .map(|(name, _)| name.clone())
        .collect();
    out.sort();
    out
}

/// A new file name: the moment (unix ms), a counter for several in the same
/// millisecond and the extension (`jpg` for a JPEG, anything else `png`).
pub fn unique_name(unix_ms: u64, n: u32, ext: &str) -> String {
    let ext = match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "jpg",
        _ => "png",
    };
    format!("img-{unix_ms}-{n}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn valid_names_are_plain_png_and_jpeg_files() {
        for ok in [
            "a.png",
            "img-1730000000000-0.png",
            "photo_1.JPG",
            "x.y.jpeg",
            "A-b_c.9.png",
        ] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in [
            "",
            ".png",
            "a.gif",
            "a.png.exe",
            "a",
            "a/b.png",
            "a\\b.png",
            "..\\a.png",
            "../a.png",
            "a..png",
            "a b.png",
            "a:b.png",
            "c:a.png",
            "\u{e9}.png",
            "a\u{0}.png",
            "con.png",
            "NUL.jpg",
            "com1.png",
            "LPT9.jpeg",
            "aux.x.png",
        ] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert!(valid_name(&format!("{}.png", "a".repeat(60))), "64 characters");
        assert!(!valid_name(&format!("{}.png", "a".repeat(61))), "65 characters");
        assert!(valid_name("com10.png"), "only COM1-9 are devices");
        assert!(valid_name("console.png"));
    }

    #[test]
    fn a_name_joined_to_the_folder_never_leaves_it() {
        use std::path::Path;
        let dir = Path::new("C:\\data\\note-images");
        for name in ["a.png", "img-1-0.jpg"] {
            assert!(valid_name(name));
            assert_eq!(dir.join(name).parent(), Some(dir));
        }
    }

    #[test]
    fn image_paths_are_found_by_extension() {
        for p in [
            "C:\\a\\b.PNG",
            "x.jpg",
            "x.jpeg",
            "d:/pics/x.gif",
            "x.bmp",
            "x.webp",
            "a.b.png",
        ] {
            assert!(is_image_path(p), "{p}");
        }
        for p in ["x.txt", "png", "C:\\a.png\\b", "x.png.exe", "", ".png", "dir\\"] {
            assert!(!is_image_path(p), "{p:?}");
        }
    }

    #[test]
    fn a_drop_is_split_in_order() {
        let paths: Vec<String> = ["a.png", "b.txt", "c.JPG", "d", "e.webp"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let (img, other) = split_files(&paths);
        assert_eq!(img, ["a.png", "c.JPG", "e.webp"]);
        assert_eq!(other, ["b.txt", "d"]);
        assert_eq!(split_files(&[]), (vec![], vec![]));
    }

    #[test]
    fn admit_counts_what_is_already_there_and_in_flight() {
        assert_eq!(admit(0, 0, 3), (3, 0));
        assert_eq!(admit(2, 0, 3), (2, 1));
        assert_eq!(admit(2, 1, 3), (1, 2));
        assert_eq!(admit(4, 0, 1), (0, 1));
        assert_eq!(admit(0, 4, 2), (0, 2));
        assert_eq!(admit(3, 3, 1), (0, 1), "never negative room");
        assert_eq!(admit(1, 0, 0), (0, 0));
    }

    fn inside(r: &Rect, w: f32) -> bool {
        r.x >= SIDE - 0.001 && r.right() <= w - SIDE + 0.001 && r.w > 0.0 && r.h > 0.0
    }

    #[test]
    fn one_image_is_full_width_with_its_aspect_up_to_45_percent() {
        let (r, block) = grid(1, 240.0, 208.0, &[3.0]);
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].x, r[0].y, r[0].w), (8.0, 0.0, 224.0));
        assert!((r[0].h - 224.0 / 3.0).abs() < 0.01, "224 / 3");
        assert!((block - (224.0 / 3.0 + 8.0)).abs() < 0.01);
        // A tall picture is capped at 45 % of the note.
        let (r, block) = grid(1, 240.0, 208.0, &[0.5]);
        assert!((r[0].h - 0.45 * 208.0).abs() < 0.01);
        assert!((block - (0.45 * 208.0 + 8.0)).abs() < 0.01);
        // Unknown aspect is 4:3.
        let (r, _) = grid(1, 240.0, 800.0, &[]);
        assert!((r[0].h - 224.0 * 0.75).abs() < 0.01);
        let (r, _) = grid(1, 240.0, 800.0, &[f32::NAN]);
        assert!((r[0].h - 224.0 * 0.75).abs() < 0.01);
        let (r, _) = grid(1, 240.0, 800.0, &[0.0]);
        assert!((r[0].h - 224.0 * 0.75).abs() < 0.01);
    }

    #[test]
    fn two_sit_side_by_side_with_a_4_dip_gap() {
        let (r, block) = grid(2, 240.0, 600.0, &[]);
        assert_eq!(r.len(), 2);
        let cell = (224.0 - 4.0) / 2.0;
        assert!((r[0].w - cell).abs() < 0.01 && (r[1].w - cell).abs() < 0.01);
        assert!((r[1].x - r[0].right() - 4.0).abs() < 0.01);
        assert_eq!(r[0].y, r[1].y);
        assert!((r[0].h - cell * 0.75).abs() < 0.01);
        assert!((block - (cell * 0.75 + 8.0)).abs() < 0.01);
        // Capped at 45 % of a short note.
        let (r, _) = grid(2, 240.0, 120.0, &[]);
        assert!((r[0].h - 54.0).abs() < 0.01);
    }

    #[test]
    fn three_are_two_cells_then_one_full_width() {
        let (r, block) = grid(3, 240.0, 600.0, &[]);
        assert_eq!(r.len(), 3);
        assert!((r[2].w - 224.0).abs() < 0.01 && (r[2].x - 8.0).abs() < 0.01);
        assert!((r[2].y - (r[0].h + 4.0)).abs() < 0.01);
        assert_eq!(r[0].h, r[2].h, "every row height is the same");
        assert!((block - (2.0 * r[0].h + 4.0 + 8.0)).abs() < 0.01);
    }

    #[test]
    fn four_are_two_by_two_and_every_row_fits_the_cap() {
        let (r, block) = grid(4, 240.0, 208.0, &[]);
        assert_eq!(r.len(), 4);
        assert_eq!((r[0].y, r[1].y), (0.0, 0.0));
        assert_eq!(r[2].y, r[3].y);
        assert!(r[2].y > r[0].bottom());
        let cap = 0.45 * 208.0;
        assert!(block - 8.0 <= cap + 0.01, "{block} vs {cap}");
        assert!((r[0].h - ((224.0f32 - 4.0) / 2.0 * 0.75).min((cap - 4.0) / 2.0)).abs() < 0.01);
    }

    #[test]
    fn the_grid_stays_inside_the_note_at_every_size() {
        for w in [160.0, 240.0, 640.0] {
            for h in [120.0, 208.0, 800.0] {
                for n in 1..=4 {
                    let (r, block) = grid(n, w, h, &[1.5; 4]);
                    assert_eq!(r.len(), n);
                    for c in &r {
                        assert!(inside(c, w), "{n} at {w}x{h}: {c:?}");
                        assert!(c.bottom() <= block - BELOW + 0.01);
                    }
                    for (i, a) in r.iter().enumerate() {
                        for b in &r[i + 1..] {
                            let hit = a.x < b.right() - 0.001
                                && b.x < a.right() - 0.001
                                && a.y < b.bottom() - 0.001
                                && b.y < a.bottom() - 0.001;
                            assert!(!hit, "{n} at {w}x{h}: {a:?} {b:?}");
                        }
                    }
                    assert!(block - BELOW <= MAX_BLOCK * h + 0.01 + 1.0, "{n} {w}x{h}");
                }
            }
        }
        assert_eq!(grid(0, 240.0, 208.0, &[]), (vec![], 0.0));
    }

    #[test]
    fn what_was_known_and_is_no_longer_referenced_is_released() {
        let known = set(&["a.png", "b.png", "c.png"]);
        let now = set(&["b.png", "d.png"]);
        assert_eq!(to_release(&known, &now), ["a.png", "c.png"]);
        assert!(to_release(&known, &known).is_empty());
        assert!(to_release(&set(&[]), &now).is_empty());
        assert_eq!(to_release(&known, &set(&[])), ["a.png", "b.png", "c.png"]);
    }

    #[test]
    fn orphans_are_old_unreferenced_and_valid() {
        let now = 1_000_000;
        let day = ORPHAN_AGE_SECS;
        let listing = vec![
            ("old.png".to_owned(), now - day - 1),
            ("exactly.png".to_owned(), now - day),
            ("fresh.png".to_owned(), now - 60),
            ("used.png".to_owned(), now - 10 * day),
            ("notes.txt".to_owned(), now - 10 * day),
            ("con.png".to_owned(), now - 10 * day),
            ("a b.png".to_owned(), now - 10 * day),
        ];
        let used = set(&["used.png"]);
        assert_eq!(orphans(&listing, &used, now), ["exactly.png", "old.png"]);
    }

    #[test]
    fn unique_names_are_valid_names() {
        assert_eq!(unique_name(1_730_000_000_123, 0, "png"), "img-1730000000123-0.png");
        assert_eq!(unique_name(5, 2, "JPEG"), "img-5-2.jpg");
        assert_eq!(unique_name(5, 2, "webp"), "img-5-2.png", "anything else is a PNG");
        for n in [unique_name(1, 0, "jpg"), unique_name(u64::MAX, u32::MAX, "png")] {
            assert!(valid_name(&n), "{n}");
        }
        assert_ne!(unique_name(1, 0, "png"), unique_name(1, 1, "png"));
    }
}
