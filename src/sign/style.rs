//! Text style of Text and Date items, and the last-used style kept in memory.

use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Family {
    #[default]
    Sans,
    Serif,
    Mono,
}

impl Family {
    pub const ALL: [Family; 3] = [Family::Sans, Family::Serif, Family::Mono];

    /// The Windows face used on screen and for measuring.
    pub fn face(self) -> &'static str {
        match self {
            Family::Sans => "Arial",
            Family::Serif => "Times New Roman",
            Family::Mono => "Courier New",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tint {
    #[default]
    Black,
    Blue,
    Red,
}

impl Tint {
    #[allow(dead_code)] // used by the format bar
    pub const ALL: [Tint; 3] = [Tint::Black, Tint::Blue, Tint::Red];

    pub fn rgb(self) -> u32 {
        match self {
            Tint::Black => 0x1C1C1C,
            Tint::Blue => 0x1F3A8A,
            Tint::Red => 0xA61B1B,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextStyle {
    pub family: Family,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub tint: Tint,
}

/// Underline centre, in em below the baseline (negative = below).
#[allow(dead_code)] // used by the renderers
pub const UNDERLINE_POS: f32 = -0.1;
/// Underline thickness in em.
#[allow(dead_code)] // used by the renderers
pub const UNDERLINE_THICK: f32 = 0.05;

impl TextStyle {
    /// One of the 14 standard PDF fonts, so nothing needs embedding.
    pub fn pdf_base_font(&self) -> &'static str {
        match (self.family, self.bold, self.italic) {
            (Family::Sans, false, false) => "Helvetica",
            (Family::Sans, true, false) => "Helvetica-Bold",
            (Family::Sans, false, true) => "Helvetica-Oblique",
            (Family::Sans, true, true) => "Helvetica-BoldOblique",
            (Family::Serif, false, false) => "Times-Roman",
            (Family::Serif, true, false) => "Times-Bold",
            (Family::Serif, false, true) => "Times-Italic",
            (Family::Serif, true, true) => "Times-BoldItalic",
            (Family::Mono, false, false) => "Courier",
            (Family::Mono, true, false) => "Courier-Bold",
            (Family::Mono, false, true) => "Courier-Oblique",
            (Family::Mono, true, true) => "Courier-BoldOblique",
        }
    }

    /// Whether switching between the two styles changes glyph advances.
    pub fn changes_width(&self, other: &TextStyle) -> bool {
        self.family != other.family || self.bold != other.bold || self.italic != other.italic
    }
}

/// Tests that set or depend on the last-used style take this lock, since
/// the last-used statics are process-wide and tests run in parallel.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

static LAST_TEXT: Mutex<TextStyle> = Mutex::new(TextStyle {
    family: Family::Sans,
    bold: false,
    italic: false,
    underline: false,
    tint: Tint::Black,
});
static LAST_CHECK: Mutex<Tint> = Mutex::new(Tint::Black);

/// Style of the last Text or Date the user styled; new ones start from it.
pub fn last_text() -> TextStyle {
    *LAST_TEXT.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_last_text(s: TextStyle) {
    *LAST_TEXT.lock().unwrap_or_else(|e| e.into_inner()) = s;
}

pub fn last_check() -> Tint {
    *LAST_CHECK.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_last_check(t: Tint) {
    *LAST_CHECK.lock().unwrap_or_else(|e| e.into_inner()) = t;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn base_fonts_cover_all_twelve() {
        let want = [
            "Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Helvetica-BoldOblique",
            "Times-Roman", "Times-Bold", "Times-Italic", "Times-BoldItalic",
            "Courier", "Courier-Bold", "Courier-Oblique", "Courier-BoldOblique",
        ];
        let mut seen = HashSet::new();
        for (fi, f) in Family::ALL.iter().enumerate() {
            for (bi, bold) in [false, true].iter().enumerate() {
                for (ii, italic) in [false, true].iter().enumerate() {
                    let st = TextStyle { family: *f, bold: *bold, italic: *italic, ..Default::default() };
                    assert_eq!(st.pdf_base_font(), want[fi * 4 + bi + ii * 2]);
                    seen.insert(st.pdf_base_font());
                }
            }
        }
        assert_eq!(seen.len(), 12);
        assert_eq!(Family::Serif.face(), "Times New Roman");
        assert_eq!(Family::Mono.face(), "Courier New");
        assert_eq!(Family::Sans.face(), "Arial");
    }

    #[test]
    fn tints_are_the_spec_colours() {
        assert_eq!(Tint::Black.rgb(), 0x1C1C1C);
        assert_eq!(Tint::Blue.rgb(), 0x1F3A8A);
        assert_eq!(Tint::Red.rgb(), 0xA61B1B);
    }

    #[test]
    fn defaults_are_sans_plain_black() {
        let d = TextStyle::default();
        assert_eq!(
            d,
            TextStyle { family: Family::Sans, bold: false, italic: false, underline: false, tint: Tint::Black }
        );
    }

    /// Restores the last-used defaults when dropped, even on panic.
    struct RestoreDefaults;

    impl Drop for RestoreDefaults {
        fn drop(&mut self) {
            set_last_text(TextStyle::default());
            set_last_check(Tint::default());
        }
    }

    #[test]
    fn last_used_round_trips() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _restore = RestoreDefaults;
        let st = TextStyle { family: Family::Mono, bold: true, italic: false, underline: true, tint: Tint::Red };
        set_last_text(st);
        assert_eq!(last_text(), st);
        set_last_check(Tint::Blue);
        assert_eq!(last_check(), Tint::Blue);
        set_last_text(TextStyle::default());
        set_last_check(Tint::Black);
        assert_eq!(last_text(), TextStyle::default());
        assert_eq!(last_check(), Tint::Black);
    }

    #[test]
    fn width_changes_only_for_face_weight_slant() {
        let a = TextStyle::default();
        assert!(!a.changes_width(&TextStyle { underline: true, ..a }));
        assert!(!a.changes_width(&TextStyle { tint: Tint::Red, ..a }));
        assert!(a.changes_width(&TextStyle { family: Family::Serif, ..a }));
        assert!(a.changes_width(&TextStyle { bold: true, ..a }));
        assert!(a.changes_width(&TextStyle { italic: true, ..a }));
    }
}
