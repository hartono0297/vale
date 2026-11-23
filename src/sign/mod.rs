//! Signing documents ("Tanda tangan"): shared types for the editor, the
//! writers and the shelf entry point.

pub mod geom;
pub mod hand;
pub mod image_out;
pub mod ink;
pub mod names;
pub mod office;
pub mod pdf_write;
pub mod raster;
pub mod render;
pub mod session;
pub mod store;
pub mod style;
pub mod text_draw;
pub mod winansi;
pub mod window;

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What a document is, by its extension.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Pdf,
    Image,
    Office,
}

/// A file picked for signing.
#[derive(Clone, Debug)]
pub struct Source {
    pub path: PathBuf,
    pub kind: Kind,
}

/// The kind of a file from its lowercased extension; `None` when Vale cannot
/// sign it.
pub fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => Some(Kind::Pdf),
        "png" | "jpg" | "jpeg" => Some(Kind::Image),
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => Some(Kind::Office),
        _ => None,
    }
}

pub fn is_signable(path: &Path) -> bool {
    kind_of(path).is_some()
}

/// A rotation in quarter turns. A page's quarter is clockwise as displayed
/// (PDF `/Rotate`); an item's `turn` is counter-clockwise in page space (PDF `cm`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quarter {
    R0,
    R90,
    R180,
    R270,
}

impl Quarter {
    /// Any multiple of 90 degrees (negative or past 360 included); anything
    /// else is no rotation.
    pub fn from_degrees(deg: i64) -> Quarter {
        if deg % 90 != 0 {
            return Quarter::R0;
        }
        match deg.rem_euclid(360) {
            90 => Quarter::R90,
            180 => Quarter::R180,
            270 => Quarter::R270,
            _ => Quarter::R0,
        }
    }

    pub fn degrees(self) -> i64 {
        match self {
            Quarter::R0 => 0,
            Quarter::R90 => 90,
            Quarter::R180 => 180,
            Quarter::R270 => 270,
        }
    }

    pub fn add(self, other: Quarter) -> Quarter {
        Quarter::from_degrees(self.degrees() + other.degrees())
    }

    /// One quarter turn clockwise.
    pub fn turn_cw(self) -> Quarter {
        Quarter::from_degrees(self.degrees() + 90)
    }
}

/// A signature or initials bitmap: straight (not premultiplied) RGBA,
/// top-down, cropped to the ink.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Mark {
    /// The swatch this mark is drawn in: the colour of its most opaque pixel,
    /// if that colour is within 24 per channel of a swatch.
    pub fn tint(&self) -> Option<style::Tint> {
        if self.rgba.len() < self.w as usize * self.h as usize * 4 {
            return None;
        }
        let px = self.rgba.chunks_exact(4).min_by_key(|p| std::cmp::Reverse(p[3]))?;
        if px[3] <= 8 {
            return None;
        }
        let rgb = [px[0], px[1], px[2]].map(i32::from);
        let (tint, diff) = style::Tint::ALL
            .iter()
            .map(|&t| {
                let c = t.rgb();
                let swatch = [(c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF].map(|v| v as i32);
                (t, [rgb[0] - swatch[0], rgb[1] - swatch[1], rgb[2] - swatch[2]])
            })
            .min_by_key(|(_, d)| d.iter().map(|x| x * x).sum::<i32>())?;
        (diff.iter().all(|d| d.abs() <= 24)).then_some(tint)
    }

    /// The same shape and alpha, every pixel's colour set to `t`.
    pub fn recolored(&self, t: style::Tint) -> Mark {
        let c = t.rgb();
        let mut rgba = self.rgba.clone();
        for p in rgba.chunks_exact_mut(4) {
            p[0] = (c >> 16) as u8;
            p[1] = (c >> 8) as u8;
            p[2] = c as u8;
        }
        Mark { w: self.w, h: self.h, rgba }
    }

    /// Whether `other` has the same size and the same alpha in every pixel.
    pub fn same_shape(&self, other: &Mark) -> bool {
        self.w == other.w
            && self.h == other.h
            && self.rgba.len() == other.rgba.len()
            && self.rgba.chunks_exact(4).zip(other.rgba.chunks_exact(4)).all(|(a, b)| a[3] == b[3])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Signature(Arc<Mark>),
    Initials(Arc<Mark>),
    Date(String, style::TextStyle),
    Text(String, style::TextStyle),
    Check(style::Tint),
}

/// One thing placed on a page. `x, y` is the lower-left corner in unrotated
/// page space (PDF points, origin at the page box's lower-left); `w, h` is
/// the size in the item's own upright frame; `turn` is its rotation relative
/// to page space.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub turn: Quarter,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_of_by_extension() {
        assert_eq!(kind_of(Path::new("a.PDF")), Some(Kind::Pdf));
        assert_eq!(kind_of(Path::new("b.jpeg")), Some(Kind::Image));
        assert_eq!(kind_of(Path::new("c.Docx")), Some(Kind::Office));
        assert_eq!(kind_of(Path::new("d.pptx")), Some(Kind::Office));
        assert_eq!(kind_of(Path::new("e.heic")), None);
        assert_eq!(kind_of(Path::new("f")), None);
        assert_eq!(kind_of(Path::new("g.pdf.txt")), None);
        assert!(is_signable(Path::new("x.png")));
        assert!(!is_signable(Path::new("x.zip")));
    }

    #[test]
    fn quarter_normalises() {
        assert_eq!(Quarter::from_degrees(-90), Quarter::R270);
        assert_eq!(Quarter::from_degrees(450), Quarter::R90);
        assert_eq!(Quarter::from_degrees(45), Quarter::R0);
        assert_eq!(Quarter::R270.turn_cw(), Quarter::R0);
        assert_eq!(Quarter::R0.turn_cw(), Quarter::R90);
        assert_eq!(Quarter::R90.add(Quarter::R270), Quarter::R0);
        assert_eq!(Quarter::R180.degrees(), 180);
    }

    #[test]
    fn mark_tint_reads_the_most_opaque_pixel() {
        use style::Tint;
        // A faint edge pixel in a slightly off colour, then the solid ink.
        let m = Mark { w: 2, h: 1, rgba: vec![0x40, 0x50, 0x70, 60, 0x1F, 0x3A, 0x8A, 255] };
        assert_eq!(m.tint(), Some(Tint::Blue));
        let near = Mark { w: 1, h: 1, rgba: vec![0x30, 0x24, 0x20, 200] };
        assert_eq!(near.tint(), Some(Tint::Black), "within 24 per channel");
        assert_eq!(Mark { w: 1, h: 1, rgba: vec![0, 200, 0, 255] }.tint(), None, "green is no ink");
        assert_eq!(Mark { w: 1, h: 1, rgba: vec![0x1F, 0x3A, 0x8A, 8] }.tint(), None, "nothing opaque");
        assert_eq!(Mark { w: 2, h: 2, rgba: vec![0; 4] }.tint(), None, "short buffer");
    }

    #[test]
    fn mark_recolored_keeps_alpha() {
        use style::Tint;
        let m = Mark { w: 2, h: 1, rgba: vec![0x1F, 0x3A, 0x8A, 255, 0x1F, 0x3A, 0x8A, 30] };
        let r = m.recolored(Tint::Red);
        assert_eq!(r.rgba, vec![0xA6, 0x1B, 0x1B, 255, 0xA6, 0x1B, 0x1B, 30]);
        assert_eq!((r.w, r.h), (2, 1));
        assert_eq!(r.tint(), Some(Tint::Red));
        assert!(r.same_shape(&m) && m.same_shape(&r));
        assert!(!m.same_shape(&Mark { w: 2, h: 1, rgba: vec![0x1F, 0x3A, 0x8A, 255, 0x1F, 0x3A, 0x8A, 31] }));
        assert!(!m.same_shape(&Mark { w: 1, h: 2, rgba: m.rgba.clone() }));
        assert!(Mark { w: 0, h: 0, rgba: Vec::new() }.recolored(Tint::Blue).rgba.is_empty());
    }
}
