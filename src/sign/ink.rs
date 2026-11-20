//! Making a signature bitmap: from drawn strokes, a typed name or a photo.

use std::path::Path;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::{BOOL, HSTRING, Interface, w};
use windows_numerics::Vector2;

use super::Mark;
use super::raster;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ink {
    Blue,
    Black,
}

impl Ink {
    fn rgb(self) -> [u8; 3] {
        match self {
            Ink::Blue => [0x1F, 0x3A, 0x8A],
            Ink::Black => [0x1C, 0x1C, 0x1C],
        }
    }

    fn color(self) -> D2D1_COLOR_F {
        let [r, g, b] = self.rgb();
        D2D1_COLOR_F {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
            a: 1.0,
        }
    }
}

/// The logical drawing pad, and how many pixels one logical unit becomes.
const PAD_W: u32 = 300;
const PAD_H: u32 = 90;
const SCALE: u32 = 2;
const STROKE_WIDTH: f32 = 3.2;
/// Clear border around the ink in every finished mark.
const CROP_PAD: u32 = 4;
/// Typed names are cut to this many characters.
const MAX_NAME: usize = 40;
const TEXT_PX: f32 = 64.0;
/// Photos above this many pixels are refused; the long side is cut to
/// `PHOTO_SIDE`.
const MAX_PHOTO_PIXELS: u64 = 40_000_000;
const PHOTO_SIDE: u32 = 1600;

/// Drawn strokes (logical pad coordinates, 300 x 90) as a smooth mark.
pub fn from_strokes(strokes: &[Vec<(f32, f32)>], ink: Ink) -> Option<Mark> {
    let cleaned: Vec<Vec<(f32, f32)>> = strokes
        .iter()
        .map(|s| dedup(s))
        .filter(|s| !s.is_empty())
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let k = SCALE as f32;
    let pbgra = raster::offscreen(PAD_W * SCALE, PAD_H * SCALE, |rt, d2d, _| unsafe {
        let Ok(brush) = rt.CreateSolidColorBrush(&ink.color(), None) else {
            return;
        };
        let Ok(style) = d2d.CreateStrokeStyle(
            &D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            },
            None,
        ) else {
            return;
        };
        for s in &cleaned {
            let p = |i: usize| {
                let (x, y) = s[i.min(s.len() - 1)];
                Vector2::new(x * k, y * k)
            };
            if s.len() == 1 {
                let r = STROKE_WIDTH * k / 2.0;
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: p(0),
                        radiusX: r,
                        radiusY: r,
                    },
                    &brush,
                );
                continue;
            }
            let Ok(geometry) = d2d.CreatePathGeometry() else {
                continue;
            };
            let Ok(sink) = geometry.Open() else {
                continue;
            };
            sink.BeginFigure(p(0), D2D1_FIGURE_BEGIN_HOLLOW);
            for i in 0..s.len() - 1 {
                // Catmull-Rom through p(i)..p(i+1), ends repeated.
                let p0 = if i == 0 { p(0) } else { p(i - 1) };
                let (p1, p2, p3) = (p(i), p(i + 1), p(i + 2));
                sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: Vector2::new(
                        p1.X + (p2.X - p0.X) / 6.0,
                        p1.Y + (p2.Y - p0.Y) / 6.0,
                    ),
                    point2: Vector2::new(
                        p2.X - (p3.X - p1.X) / 6.0,
                        p2.Y - (p3.Y - p1.Y) / 6.0,
                    ),
                    point3: p2,
                });
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                rt.DrawGeometry(&geometry, &brush, STROKE_WIDTH * k, &style);
            }
        }
    })?;
    crop(
        raster::to_mark(PAD_W * SCALE, PAD_H * SCALE, &pbgra),
        CROP_PAD,
    )
}

/// Points with non-finite values and consecutive repeats removed.
fn dedup(s: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(s.len());
    for &(x, y) in s {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        if out.last() != Some(&(x, y)) {
            out.push((x, y));
        }
    }
    out
}

/// The first of Ink Free, Segoe Script, Segoe Print the system has.
fn handwriting_family(dwrite: &IDWriteFactory) -> Option<HSTRING> {
    unsafe {
        let mut collection = None;
        dwrite.GetSystemFontCollection(&mut collection, false).ok()?;
        let collection = collection?;
        ["Ink Free", "Segoe Script", "Segoe Print"]
            .into_iter()
            .map(HSTRING::from)
            .find(|name| {
                let (mut index, mut exists) = (0u32, BOOL(0));
                collection.FindFamilyName(name, &mut index, &mut exists).is_ok()
                    && exists.as_bool()
            })
    }
}

/// A typed name in a handwriting font as a mark.
pub fn from_text(name: &str, ink: Ink) -> Option<Mark> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let text: String = name.chars().take(MAX_NAME).collect();
    let wide: Vec<u16> = text.encode_utf16().collect();
    let w = (wide.len() as u32 * 80 + 80).min(4096);
    let h = 200u32;
    let pbgra = raster::offscreen(w, h, |rt, _, dwrite| unsafe {
        let Some(family) = handwriting_family(dwrite) else {
            return;
        };
        let Ok(format) = dwrite.CreateTextFormat(
            &family,
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            TEXT_PX,
            w!("en-us"),
        ) else {
            return;
        };
        let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let Ok(layout) = dwrite.CreateTextLayout(&wide, &format, w as f32 - 40.0, h as f32) else {
            return;
        };
        let Ok(brush) = rt.CreateSolidColorBrush(&ink.color(), None) else {
            return;
        };
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        rt.DrawTextLayout(
            Vector2::new(20.0, 20.0),
            &layout,
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    })?;
    crop(raster::to_mark(w, h, &pbgra), CROP_PAD)
}

/// A photo of a signature on paper: the paper goes transparent, the ink
/// takes `ink`'s colour. `None` when the file is not a usable image or no ink
/// stands out from the paper.
pub fn from_photo(path: &Path, ink: Ink) -> Option<Mark> {
    let file = std::fs::File::open(path).ok()?;
    let (w, h, bgra) = unsafe { decode_photo(&file)? };
    let luma: Vec<u8> = bgra
        .chunks_exact(4)
        .map(|p| {
            // Transparent parts count as white paper.
            let paper = 255 - u32::from(p[3]);
            let c = |v: u8| u32::from(v) + paper;
            ((c(p[2]) * 299 + c(p[1]) * 587 + c(p[0]) * 114) / 1000).min(255) as u8
        })
        .collect();
    let mut histogram = [0u64; 256];
    for &l in &luma {
        histogram[l as usize] += 1;
    }
    let half = luma.len() as u64 / 2;
    let (mut seen, mut median) = (0u64, 255i32);
    for (level, &count) in histogram.iter().enumerate() {
        seen += count;
        if seen > half {
            median = level as i32;
            break;
        }
    }
    let t_high = (median - 25).clamp(40, 235);
    let t_low = (t_high - 60).max(10);
    let span = (t_high - t_low) as u32;
    let [r, g, b] = ink.rgb();
    let mut rgba = Vec::with_capacity(luma.len() * 4);
    for &l in &luma {
        let l = i32::from(l);
        let a = if l <= t_low {
            255
        } else if l >= t_high {
            0
        } else {
            ((t_high - l) as u32 * 255 / span) as u8
        };
        rgba.extend_from_slice(&[r, g, b, a]);
    }
    crop(Mark { w, h, rgba }, CROP_PAD)
}

/// The first frame of `file` (EXIF-rotated), downscaled so the long side is
/// at most `PHOTO_SIDE`, as `(w, h, premultiplied BGRA)`.
unsafe fn decode_photo(file: &std::fs::File) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let source = crate::phone::thumb::oriented_source(&wic, file)?;
        let (mut w, mut h) = (0u32, 0u32);
        source.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PHOTO_PIXELS {
            return None;
        }
        let mut source: IWICBitmapSource = source;
        let long = w.max(h);
        if long > PHOTO_SIDE {
            let nw = ((u64::from(w) * u64::from(PHOTO_SIDE) / u64::from(long)) as u32).max(1);
            let nh = ((u64::from(h) * u64::from(PHOTO_SIDE) / u64::from(long)) as u32).max(1);
            let scaler = wic.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&source, nw, nh, WICBitmapInterpolationModeFant)
                .ok()?;
            source = scaler.cast().ok()?;
            (w, h) = (nw, nh);
        }
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &source,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut out = vec![0u8; w as usize * h as usize * 4];
        converter.CopyPixels(std::ptr::null(), w * 4, &mut out).ok()?;
        Some((w, h, out))
    }
}

/// `m` cut to the box around every pixel with alpha above 8, plus `pad`
/// pixels (kept inside the image). `None` when nothing is that opaque.
pub fn crop(m: Mark, pad: u32) -> Option<Mark> {
    let (w, h) = (m.w as usize, m.h as usize);
    if w == 0 || h == 0 || m.rgba.len() != w.checked_mul(h)?.checked_mul(4)? {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for (i, px) in m.rgba.chunks_exact(4).enumerate() {
        if px[3] > 8 {
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 > x1 || y0 > y1 {
        return None;
    }
    let pad = pad as usize;
    let (x0, y0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
    let (x1, y1) = ((x1 + pad).min(w - 1), (y1 + pad).min(h - 1));
    let (nw, nh) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut rgba = Vec::with_capacity(nw * nh * 4);
    for y in y0..=y1 {
        rgba.extend_from_slice(&m.rgba[(y * w + x0) * 4..(y * w + x1 + 1) * 4]);
    }
    Some(Mark {
        w: nw as u32,
        h: nh as u32,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    struct Com(bool);
    impl Com {
        fn new() -> Com {
            Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
        }
    }
    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    fn diagonal() -> Vec<Vec<(f32, f32)>> {
        vec![(0..40).map(|i| (20.0 + i as f32 * 5.0, 15.0 + i as f32 * 1.5)).collect()]
    }

    #[test]
    fn strokes_make_a_cropped_mark() {
        let _com = Com::new();
        let m = from_strokes(&diagonal(), Ink::Blue).expect("a mark");
        assert!(m.w < 600 && m.h < 180, "{} x {}", m.w, m.h);
        assert_eq!(m.rgba.len(), (m.w * m.h * 4) as usize);
        let solid = m.rgba.chunks(4).any(|p| {
            p[3] == 255
                && (p[0] as i32 - 0x1F).abs() <= 4
                && (p[1] as i32 - 0x3A).abs() <= 4
                && (p[2] as i32 - 0x8A).abs() <= 4
        });
        assert!(solid, "a fully opaque ink-blue pixel");
    }

    #[test]
    fn empty_strokes_give_none() {
        let _com = Com::new();
        assert!(from_strokes(&[], Ink::Blue).is_none());
        assert!(from_strokes(&[vec![]], Ink::Black).is_none());
    }

    #[test]
    fn single_point_stroke_is_a_dot() {
        let _com = Com::new();
        let m = from_strokes(&[vec![(100.0, 40.0)]], Ink::Black).expect("a dot");
        assert!(m.w >= 6 && m.w < 40 && m.h >= 6 && m.h < 40, "{} x {}", m.w, m.h);
        assert!(m.rgba.chunks(4).any(|p| p[3] == 255 && p[0] == 0x1C));
    }

    #[test]
    fn typed_name_uses_installed_font() {
        let _com = Com::new();
        let m = from_text("Siti", Ink::Blue).expect("a mark");
        assert!(m.w > 20 && m.h > 10);
        assert!(from_text("   ", Ink::Blue).is_none());
    }

    /// 400x200 white PNG with a dark grey horizontal line 8 px thick.
    fn write_line_png(path: &Path, line: bool) {
        let (w, h) = (400u32, 200u32);
        let mut bgra = vec![255u8; (w * h * 4) as usize];
        if line {
            for y in 96..104 {
                for x in 60..340 {
                    let i = ((y * w + x) * 4) as usize;
                    bgra[i..i + 3].copy_from_slice(&[60, 60, 60]);
                }
            }
        }
        let png = crate::screenshot::win::encode_png(w, h, &bgra).expect("png");
        std::fs::write(path, png).expect("write");
    }

    #[test]
    fn photo_white_background_becomes_transparent() {
        let _com = Com::new();
        let dir = crate::phone::files::tests::temp_dir("sign-photo");
        let path = dir.join("sig.png");
        write_line_png(&path, true);
        let m = from_photo(&path, Ink::Blue).expect("a mark");
        assert!(m.w < 400 && m.h < 200, "{} x {}", m.w, m.h);
        assert_eq!(m.rgba[3], 0, "corner is transparent");
        let mid = ((m.h / 2 * m.w + m.w / 2) * 4) as usize;
        assert!(m.rgba[mid + 3] > 200, "line alpha {}", m.rgba[mid + 3]);
        assert_eq!(&m.rgba[mid..mid + 3], &[0x1F, 0x3A, 0x8A]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blank_photo_gives_none() {
        let _com = Com::new();
        let dir = crate::phone::files::tests::temp_dir("sign-blank");
        let path = dir.join("blank.png");
        write_line_png(&path, false);
        assert!(from_photo(&path, Ink::Blue).is_none());
        assert!(from_photo(&dir.join("missing.png"), Ink::Blue).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
