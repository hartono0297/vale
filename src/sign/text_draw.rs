//! Drawing and measuring styled text for the sign window and writers.

use std::sync::OnceLock;

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory,
};
use windows::core::HSTRING;

use super::style::TextStyle;

/// Share of the box height the font size takes.
#[allow(dead_code)] // used once text drawing moves here
pub const TEXT_FILL: f32 = 0.8;

fn factory() -> Option<&'static IDWriteFactory> {
    static F: OnceLock<Option<IDWriteFactory>> = OnceLock::new();
    F.get_or_init(|| unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok() })
        .as_ref()
}

fn measure(s: &str, st: &TextStyle) -> Option<f32> {
    let f = factory()?;
    let wide: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        let fmt = f
            .CreateTextFormat(
                &HSTRING::from(st.family.face()),
                None,
                if st.bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL },
                if st.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL },
                DWRITE_FONT_STRETCH_NORMAL,
                1.0,
                &HSTRING::from("en-us"),
            )
            .ok()?;
        let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let layout = f.CreateTextLayout(&wide, &fmt, 1.0e6, 4.0).ok()?;
        let mut m = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut m).ok()?;
        Some(m.widthIncludingTrailingWhitespace)
    }
}

/// Advance of `s` at font size 1 in `st`'s face, weight and slant.
pub fn advance_em(s: &str, st: &TextStyle) -> f32 {
    measure(s, st).unwrap_or_else(|| s.chars().count() as f32 * 0.55)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::style::Family;

    fn st(family: Family, bold: bool) -> TextStyle {
        TextStyle { family, bold, ..Default::default() }
    }

    #[test]
    fn mono_advance_is_fixed() {
        let m = st(Family::Mono, false);
        let (a, b) = (advance_em("iiii", &m), advance_em("MMMM", &m));
        assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        assert!((a - 2.4).abs() < 0.05, "{a}");
    }

    #[test]
    fn bold_advances_further() {
        let s = "Budi Santoso";
        assert!(advance_em(s, &st(Family::Sans, true)) > advance_em(s, &st(Family::Sans, false)));
    }

    #[test]
    fn families_differ() {
        assert_ne!(
            advance_em("Budi", &st(Family::Serif, false)),
            advance_em("Budi", &st(Family::Sans, false))
        );
    }
}
