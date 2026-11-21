//! Drawing and measuring styled text for the sign window and writers.

use std::sync::OnceLock;

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_KERNING, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::core::HSTRING;
use windows_numerics::Vector2;

use super::style::{TextStyle, Tint, UNDERLINE_POS, UNDERLINE_THICK};

/// Share of the box height the font size takes.
pub const TEXT_FILL: f32 = 0.8;

fn factory() -> Option<&'static IDWriteFactory> {
    static F: OnceLock<Option<IDWriteFactory>> = OnceLock::new();
    F.get_or_init(|| unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok() })
        .as_ref()
}

/// Kerning off for the whole text. The PDF writer sets Helvetica and Times
/// text without kerning, so the screen, image and measured widths must too.
fn unkern(dwrite: &IDWriteFactory, layout: &IDWriteTextLayout, len: usize) {
    unsafe {
        let Ok(typo) = dwrite.CreateTypography() else { return };
        let off = DWRITE_FONT_FEATURE { nameTag: DWRITE_FONT_FEATURE_TAG_KERNING, parameter: 0 };
        if typo.AddFontFeature(off).is_ok() {
            let _ = layout.SetTypography(
                &typo,
                DWRITE_TEXT_RANGE { startPosition: 0, length: len as u32 },
            );
        }
    }
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
        unkern(f, &layout, wide.len());
        let mut m = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut m).ok()?;
        Some(m.widthIncludingTrailingWhitespace)
    }
}

/// Advance of `s` at font size 1 in `st`'s face, weight and slant.
pub fn advance_em(s: &str, st: &TextStyle) -> f32 {
    measure(s, st).unwrap_or_else(|| s.chars().count() as f32 * 0.55)
}

/// The text format for `st` at `size`: its face, weight and slant, no wrap.
pub fn format(dwrite: &IDWriteFactory, st: &TextStyle, size: f32) -> Option<IDWriteTextFormat> {
    unsafe {
        let f = dwrite
            .CreateTextFormat(
                &HSTRING::from(st.family.face()),
                None,
                if st.bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL },
                if st.italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL },
                DWRITE_FONT_STRETCH_NORMAL,
                size.max(0.5),
                &HSTRING::from("en-us"),
            )
            .ok()?;
        let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        Some(f)
    }
}

/// Width of `s` at `size` in `st` (any unit), trailing spaces included.
pub fn width(dwrite: &IDWriteFactory, s: &str, st: &TextStyle, size: f32) -> f32 {
    let fallback = s.chars().count() as f32 * size * 0.55;
    let Some(f) = format(dwrite, st, size) else { return fallback };
    let wide: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        let Ok(layout) = dwrite.CreateTextLayout(&wide, &f, 1.0e6, size * 4.0) else { return fallback };
        unkern(dwrite, &layout, wide.len());
        let mut m = DWRITE_TEXT_METRICS::default();
        if layout.GetMetrics(&mut m).is_err() {
            return fallback;
        }
        m.widthIncludingTrailingWhitespace
    }
}

pub fn tint_color(t: Tint) -> D2D1_COLOR_F {
    let c = t.rgb();
    D2D1_COLOR_F {
        r: ((c >> 16) & 0xFF) as f32 / 255.0,
        g: ((c >> 8) & 0xFF) as f32 / 255.0,
        b: (c & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

/// `s` in `st` filling `TEXT_FILL` of the box height, left aligned, its
/// baseline a fifth of the height above the bottom (where the PDF writer
/// puts it), with the underline when asked. `rect` is (x, y, w, h), y down.
pub fn draw(
    rt: &ID2D1RenderTarget,
    dwrite: &IDWriteFactory,
    brush: &ID2D1SolidColorBrush,
    s: &str,
    st: &TextStyle,
    (x, y, _w, h): (f32, f32, f32, f32),
) {
    let size = h * TEXT_FILL;
    let Some(f) = format(dwrite, st, size) else { return };
    let wide: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        let Ok(layout) = dwrite.CreateTextLayout(&wide, &f, 1.0e6, size * 4.0) else { return };
        unkern(dwrite, &layout, wide.len());
        let mut lines = [DWRITE_LINE_METRICS::default()];
        let mut n = 0u32;
        let baseline = if layout.GetLineMetrics(Some(&mut lines), &mut n).is_ok() && n > 0 {
            lines[0].baseline
        } else {
            size * 0.905
        };
        brush.SetColor(&tint_color(st.tint));
        let base_y = y + h * 0.8;
        rt.DrawTextLayout(
            Vector2::new(x, base_y - baseline),
            &layout,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        if st.underline {
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_ok() {
                let centre = base_y - UNDERLINE_POS * size;
                let half = UNDERLINE_THICK * size / 2.0;
                rt.FillRectangle(
                    &windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                        left: x,
                        top: centre - half,
                        right: x + m.widthIncludingTrailingWhitespace,
                        bottom: centre + half,
                    },
                    brush,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::style::Family;

    #[test]
    fn width_matches_advance() {
        let dwrite = factory().expect("dwrite");
        let s = st(Family::Serif, true);
        let w = width(dwrite, "Budi", &s, 20.0);
        let want = advance_em("Budi", &s) * 20.0;
        assert!((w - want).abs() < 0.5, "{w} vs {want}");
    }

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

    #[test]
    fn kerned_pairs_advance_as_the_sum_of_singles() {
        // The PDF writer sets text unkerned, so measuring must be unkerned too.
        for family in [Family::Sans, Family::Serif] {
            let s = st(family, false);
            for word in ["AVATAR", "To", "AV"] {
                let sum: f32 = word.chars().map(|c| advance_em(&c.to_string(), &s)).sum();
                let whole = advance_em(word, &s);
                assert!((whole - sum).abs() < 1e-3, "{word}: {whole} vs {sum}");
            }
        }
    }
}
