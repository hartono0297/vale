//! Signing an image: the items are drawn onto the pixels, the extra rotation
//! is applied, and the result is saved as PNG, JPG or a one-page PDF. COM
//! must be initialised by the caller.

use lopdf::{Dictionary, Document, Object, Stream};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1Factory, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR,
    GUID_WICPixelFormat32bppBGRA, IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapEncoderNoCache, WICBitmapPaletteTypeCustom,
};
use windows::Win32::System::Com::StructuredStorage::PROPBAG2;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, STREAM_SEEK_END, STREAM_SEEK_SET,
};
use windows::Win32::System::Variant::{VARIANT, VT_R4};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::{Interface, PWSTR, w};
use windows_numerics::{Matrix3x2, Vector2};

use super::geom::{PageGeom, item_view_rect};
use super::raster;
use super::session::Page;
use super::{Item, ItemKind, Mark, Quarter};

/// An image's page space is its pixels at 0.75 points each.
const PT_PER_PX: f32 = 0.75;
/// JPEG quality (0 to 1).
const JPEG_QUALITY: f32 = 0.92;
/// Text and dates fill this much of the item's height.
const TEXT_FILL: f32 = 0.8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageFormat {
    Png,
    Jpg,
    Pdf,
}

/// Page geometry of a `w_px` x `h_px` image: points are pixels x 0.75, never
/// rotated by the source.
pub fn image_geom(w_px: u32, h_px: u32) -> PageGeom {
    PageGeom { w: w_px as f32 * PT_PER_PX, h: h_px as f32 * PT_PER_PX, base: Quarter::R0 }
}

/// Ink blue `#1F3A8A`.
fn ink() -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: 31.0 / 255.0, g: 58.0 / 255.0, b: 138.0 / 255.0, a: 1.0 }
}

/// Straight RGBA mark as premultiplied BGRA.
fn premultiplied_bgra(m: &Mark) -> Vec<u8> {
    let mut out = Vec::with_capacity(m.rgba.len());
    for p in m.rgba.chunks_exact(4) {
        let a = u32::from(p[3]);
        let pm = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
        out.extend_from_slice(&[pm(p[2]), pm(p[1]), pm(p[0]), p[3]]);
    }
    out
}

/// Rotation about `(cx, cy)` that is the item's turn: counter-clockwise on
/// the page, so a clockwise screen rotation by minus the turn.
fn turn_matrix(turn: Quarter, cx: f32, cy: f32) -> Matrix3x2 {
    // A screen rotation by theta (y down) is x' = x cos - y sin, y' = x sin + y cos.
    let (sin, cos) = match turn {
        Quarter::R0 => (0.0, 1.0),
        Quarter::R90 => (-1.0, 0.0),
        Quarter::R180 => (0.0, -1.0),
        Quarter::R270 => (1.0, 0.0),
    };
    Matrix3x2 { M11: cos, M12: sin, M21: -sin, M22: cos, M31: cx, M32: cy }
}

fn draw_item(
    rt: &ID2D1RenderTarget,
    d2d: &ID2D1Factory,
    dwrite: &IDWriteFactory,
    item: &Item,
    g: PageGeom,
    scale: f32,
) {
    let (vx, vy, vw, vh) = item_view_rect(item, g, Quarter::R0, scale);
    let (bw, bh) = (item.w * scale, item.h * scale);
    let local = D2D_RECT_F { left: -bw / 2.0, top: -bh / 2.0, right: bw / 2.0, bottom: bh / 2.0 };
    unsafe {
        rt.SetTransform(&turn_matrix(item.turn, vx + vw / 2.0, vy + vh / 2.0));
        match &item.kind {
            ItemKind::Signature(m) | ItemKind::Initials(m) => {
                let pm = premultiplied_bgra(m);
                if let Ok(bitmap) = crate::shot_edit::gfx::bitmap(rt, m.w, m.h, &pm) {
                    rt.DrawBitmap(
                        &bitmap,
                        Some(&local),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                }
            }
            ItemKind::Text(s) | ItemKind::Date(s) => draw_text(rt, dwrite, s, bw, bh),
            ItemKind::Check => draw_check(rt, d2d, bw, bh),
        }
        rt.SetTransform(&Matrix3x2::identity());
    }
}

unsafe fn draw_text(rt: &ID2D1RenderTarget, dwrite: &IDWriteFactory, s: &str, bw: f32, bh: f32) {
    unsafe {
        let wide: Vec<u16> = s.encode_utf16().collect();
        let Ok(format) = dwrite.CreateTextFormat(
            w!("Arial"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            bh * TEXT_FILL,
            w!("en-us"),
        ) else {
            return;
        };
        let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        let Ok(layout) = dwrite.CreateTextLayout(&wide, &format, bw, bh) else {
            return;
        };
        let Ok(brush) = rt.CreateSolidColorBrush(&ink(), None) else {
            return;
        };
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        rt.DrawTextLayout(
            Vector2::new(-bw / 2.0, -bh / 2.0),
            &layout,
            &brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }
}

unsafe fn draw_check(rt: &ID2D1RenderTarget, d2d: &ID2D1Factory, bw: f32, bh: f32) {
    unsafe {
        let Ok(brush) = rt.CreateSolidColorBrush(&ink(), None) else {
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
        let at = |fx: f32, fy: f32| Vector2::new((fx - 0.5) * bw, (fy - 0.5) * bh);
        let width = (bw.min(bh) * 0.12).max(1.5);
        rt.DrawLine(at(0.15, 0.55), at(0.40, 0.80), &brush, width, &style);
        rt.DrawLine(at(0.40, 0.80), at(0.85, 0.20), &brush, width, &style);
    }
}

/// Rotates `w` x `h` four-byte pixels clockwise by `q`; returns the new size.
fn rotate_pixels(pix: Vec<u8>, w: u32, h: u32, q: Quarter) -> (u32, u32, Vec<u8>) {
    if q == Quarter::R0 {
        return (w, h, pix);
    }
    let (nw, nh) = match q {
        Quarter::R90 | Quarter::R270 => (h, w),
        _ => (w, h),
    };
    let mut out = vec![0u8; pix.len()];
    let (wu, hu, nwu) = (w as usize, h as usize, nw as usize);
    for y in 0..hu {
        for x in 0..wu {
            let (nx, ny) = match q {
                Quarter::R90 => (hu - 1 - y, x),
                Quarter::R180 => (wu - 1 - x, hu - 1 - y),
                _ => (y, wu - 1 - x),
            };
            let (s, d) = ((y * wu + x) * 4, (ny * nwu + nx) * 4);
            out[d..d + 4].copy_from_slice(&pix[s..s + 4]);
        }
    }
    (nw, nh, out)
}

/// The image with `items` drawn on it and `page.extra` applied: premultiplied
/// BGRA, top-down. Text, dates and checks are drawn directly (Arial in ink
/// blue, a stroked tick), so `marks_for_text` is never called. When drawing
/// fails the untouched pixels come back rotated.
pub fn compose(
    src_bgra: &[u8],
    w: u32,
    h: u32,
    page: &Page,
    items: &[Item],
    _marks_for_text: &dyn Fn(&str, f32) -> Mark,
) -> (u32, u32, Vec<u8>) {
    let scale = 1.0 / PT_PER_PX;
    let g = page.geom;
    let drawn = if src_bgra.len() == w as usize * h as usize * 4 {
        raster::offscreen(w, h, |rt, d2d, dwrite| {
            if let Ok(bitmap) = crate::shot_edit::gfx::bitmap(rt, w, h, src_bgra) {
                let dest = D2D_RECT_F { left: 0.0, top: 0.0, right: w as f32, bottom: h as f32 };
                unsafe {
                    rt.DrawBitmap(
                        &bitmap,
                        Some(&dest),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                }
            }
            for item in items {
                draw_item(rt, d2d, dwrite, item, g, scale);
            }
        })
    } else {
        None
    };
    let pixels = drawn.unwrap_or_else(|| {
        let mut v = src_bgra.to_vec();
        v.resize(w as usize * h as usize * 4, 255);
        v
    });
    rotate_pixels(pixels, w, h, page.extra)
}

/// A one-page PDF of `w_pt` x `h_pt` points showing `img` full page (RGB,
/// Flate; any transparency is flattened onto white). Empty when saving fails.
pub fn new_pdf_from_image(img: &Mark, w_pt: f32, h_pt: f32) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(img.rgba.len() / 4 * 3);
    for p in img.rgba.chunks_exact(4) {
        let a = u32::from(p[3]);
        let over = |c: u8| ((u32::from(c) * a + 255 * (255 - a) + 127) / 255) as u8;
        rgb.extend_from_slice(&[over(p[0]), over(p[1]), over(p[2])]);
    }
    let mut doc = Document::with_version("1.5");
    let mut image = Dictionary::new();
    image.set("Type", "XObject");
    image.set("Subtype", "Image");
    image.set("Width", i64::from(img.w));
    image.set("Height", i64::from(img.h));
    image.set("ColorSpace", "DeviceRGB");
    image.set("BitsPerComponent", 8i64);
    image.set("Filter", "FlateDecode");
    let data = miniz_oxide::deflate::compress_to_vec_zlib(&rgb, 6);
    let image_id = doc.add_object(Stream::new(image, data));
    let content = format!("q {w_pt} 0 0 {h_pt} 0 0 cm /Im0 Do Q");
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    let pages_id = doc.new_object_id();
    let mut xobjects = Dictionary::new();
    xobjects.set("Im0", Object::Reference(image_id));
    let mut resources = Dictionary::new();
    resources.set("XObject", Object::Dictionary(xobjects));
    let mut page = Dictionary::new();
    page.set("Type", "Page");
    page.set("Parent", Object::Reference(pages_id));
    page.set(
        "MediaBox",
        vec![Object::Integer(0), Object::Integer(0), Object::Real(w_pt), Object::Real(h_pt)],
    );
    page.set("Resources", Object::Dictionary(resources));
    page.set("Contents", Object::Reference(content_id));
    let page_id = doc.add_object(page);
    let mut pages = Dictionary::new();
    pages.set("Type", "Pages");
    pages.set("Kids", vec![Object::Reference(page_id)]);
    pages.set("Count", 1i64);
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let mut catalog = Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut out = Vec::new();
    if doc.save_to(&mut out).is_err() {
        out.clear();
    }
    out
}

/// Premultiplied BGRA flattened onto white: opaque BGRA.
fn flatten_on_white(bgra: &[u8]) -> Vec<u8> {
    let mut out = bgra.to_vec();
    for p in out.chunks_exact_mut(4) {
        let inv = 255 - p[3];
        for c in &mut p[..3] {
            *c = c.saturating_add(inv);
        }
        p[3] = 255;
    }
    out
}

fn encode_jpeg(w: u32, h: u32, bgra: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let bitmap = wic
            .CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppBGRA, w.checked_mul(4)?, bgra)
            .ok()?;
        let source: IWICBitmapSource = bitmap.cast().ok()?;
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &source,
                &GUID_WICPixelFormat24bppBGR,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let encoder = wic.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null()).ok()?;
        let stream = SHCreateMemStream(None)?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
        let mut out = None;
        let mut options = None;
        encoder.CreateNewFrame(&mut out, &mut options).ok()?;
        let out = out?;
        if let Some(bag) = &options {
            // A failed quality hint leaves WIC's default: not worth failing.
            let mut name: Vec<u16> = "ImageQuality\0".encode_utf16().collect();
            let prop = PROPBAG2 {
                dwType: 1,
                vt: VT_R4,
                pstrName: PWSTR(name.as_mut_ptr()),
                ..Default::default()
            };
            let _ = bag.Write(1, &prop, &VARIANT::from(JPEG_QUALITY));
        }
        out.Initialize(options.as_ref()).ok()?;
        out.SetSize(w, h).ok()?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        out.SetPixelFormat(&mut format).ok()?;
        out.WriteSource(&converter, std::ptr::null()).ok()?;
        out.Commit().ok()?;
        encoder.Commit().ok()?;
        let mut len = 0u64;
        stream.Seek(0, STREAM_SEEK_END, Some(&mut len)).ok()?;
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let len32 = u32::try_from(len).ok()?;
        let mut bytes = vec![0u8; len32 as usize];
        let mut read = 0u32;
        stream.Read(bytes.as_mut_ptr().cast(), len32, Some(&mut read)).ok().ok()?;
        (read == len32 && !bytes.is_empty()).then_some(bytes)
    }
}

/// The composed image as PNG, JPG (quality 0.92) or a one-page PDF whose page
/// is the image in points. Everything is flattened onto white first, so a
/// source with transparency never saves as garbage.
pub fn save_image(composed: (u32, u32, Vec<u8>), fmt: ImageFormat) -> Result<Vec<u8>, String> {
    let (w, h, pbgra) = composed;
    if w == 0 || h == 0 || pbgra.len() != w as usize * h as usize * 4 {
        return Err("Gambar tidak valid".to_string());
    }
    let flat = flatten_on_white(&pbgra);
    let bytes = match fmt {
        ImageFormat::Png => crate::screenshot::win::encode_png(w, h, &flat),
        ImageFormat::Jpg => encode_jpeg(w, h, &flat),
        ImageFormat::Pdf => {
            let mark = raster::to_mark(w, h, &flat);
            let pdf = new_pdf_from_image(&mark, w as f32 * PT_PER_PX, h as f32 * PT_PER_PX);
            (!pdf.is_empty()).then_some(pdf)
        }
    };
    bytes.ok_or_else(|| "Gambar tidak bisa disimpan".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::geom::{item_view_rect, page_to_view};
    use std::sync::Arc;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    struct Com;
    impl Com {
        fn new() -> Com {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            Com
        }
    }
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    fn white(w: u32, h: u32) -> Vec<u8> {
        vec![255u8; w as usize * h as usize * 4]
    }

    fn blue_mark(w: u32, h: u32) -> Mark {
        let mut rgba = Vec::new();
        for _ in 0..w * h {
            rgba.extend_from_slice(&[0x1F, 0x3A, 0x8A, 255]);
        }
        Mark { w, h, rgba }
    }

    fn page(g: PageGeom, extra: Quarter) -> Page {
        Page { geom: g, extra, deleted: false }
    }

    fn sig(mark: Mark, x: f32, y: f32, w: f32, h: f32, turn: Quarter) -> Item {
        Item { kind: ItemKind::Signature(Arc::new(mark)), page: 0, x, y, w, h, turn }
    }

    fn no_text(_: &str, _: f32) -> Mark {
        blue_mark(1, 1)
    }

    fn px(img: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * img.0 as usize + x as usize) * 4;
        [img.2[i], img.2[i + 1], img.2[i + 2], img.2[i + 3]]
    }

    #[test]
    fn geom_is_three_quarters_of_pixels() {
        let g = image_geom(1600, 1200);
        assert_eq!((g.w, g.h, g.base), (1200.0, 900.0, Quarter::R0));
    }

    #[test]
    fn compose_puts_signature_pixels_at_item() {
        let _com = Com::new();
        let g = image_geom(400, 300);
        let it = sig(blue_mark(40, 20), 30.0, 30.0, 40.0, 20.0, Quarter::R0);
        let out = compose(&white(400, 300), 400, 300, &page(g, Quarter::R0), &[it.clone()], &no_text);
        assert_eq!((out.0, out.1), (400, 300));
        let (rx, ry, rw, rh) = item_view_rect(&it, g, Quarter::R0, 4.0 / 3.0);
        let p = px(&out, (rx + rw / 2.0) as u32, (ry + rh / 2.0) as u32);
        assert!(p[0] > 90 && p[2] < 80 && p[1] < 100, "b g r = {p:?}");
        let c = px(&out, 390, 5);
        assert_eq!(c, [255, 255, 255, 255]);
    }

    #[test]
    fn turned_item_lands_in_its_box() {
        let _com = Com::new();
        let g = image_geom(400, 300);
        // A 60 x 10 bar turned a quarter occupies 10 wide x 60 tall.
        let it = sig(blue_mark(60, 10), 100.0, 50.0, 60.0, 10.0, Quarter::R90);
        let out = compose(&white(400, 300), 400, 300, &page(g, Quarter::R0), &[it.clone()], &no_text);
        let (rx, ry, rw, rh) = item_view_rect(&it, g, Quarter::R0, 4.0 / 3.0);
        assert!(rh > rw);
        let inside = px(&out, (rx + rw / 2.0) as u32, (ry + rh * 0.9) as u32);
        assert!(inside[2] < 80, "{inside:?}");
        let outside = px(&out, (rx + rw / 2.0 + 40.0) as u32, (ry + rh / 2.0) as u32);
        assert_eq!(outside, [255, 255, 255, 255]);
    }

    #[test]
    fn text_and_check_draw_ink() {
        let _com = Com::new();
        let g = image_geom(400, 300);
        let t = Item { kind: ItemKind::Text("MMMM".into()), page: 0, x: 20.0, y: 150.0, w: 100.0, h: 30.0, turn: Quarter::R0 };
        let c = Item { kind: ItemKind::Check, page: 0, x: 200.0, y: 150.0, w: 30.0, h: 30.0, turn: Quarter::R0 };
        let out = compose(&white(400, 300), 400, 300, &page(g, Quarter::R0), &[t.clone(), c.clone()], &no_text);
        for it in [&t, &c] {
            let (rx, ry, rw, rh) = item_view_rect(it, g, Quarter::R0, 4.0 / 3.0);
            let mut dark = 0;
            for y in ry as u32..(ry + rh) as u32 {
                for x in rx as u32..(rx + rw) as u32 {
                    if px(&out, x, y)[2] < 120 {
                        dark += 1;
                    }
                }
            }
            assert!(dark > 20, "{:?} drew {dark} dark pixels", it.kind);
        }
    }

    #[test]
    fn rotation_rotates_output() {
        let _com = Com::new();
        let g = image_geom(400, 300);
        let out = compose(&white(400, 300), 400, 300, &page(g, Quarter::R90), &[], &no_text);
        assert_eq!((out.0, out.1), (300, 400));
        assert_eq!(out.2.len(), 300 * 400 * 4);
    }

    #[test]
    fn rotation_is_clockwise_as_displayed() {
        let _com = Com::new();
        let g = image_geom(400, 300);
        // Mark the top-left corner: after a clockwise quarter it is top-right.
        let it = sig(blue_mark(20, 20), 0.0, 225.0 - 20.0, 20.0, 20.0, Quarter::R0);
        for (extra, size, at) in [
            (Quarter::R90, (300, 400), (290, 10)),
            (Quarter::R180, (400, 300), (390, 290)),
            (Quarter::R270, (300, 400), (10, 390)),
        ] {
            let out = compose(&white(400, 300), 400, 300, &page(g, extra), &[it.clone()], &no_text);
            assert_eq!((out.0, out.1), size);
            assert!(px(&out, at.0, at.1)[2] < 80, "{extra:?}");
            assert_eq!(px(&out, size.0 / 2, size.1 / 2), [255, 255, 255, 255]);
        }
        let _ = page_to_view;
    }

    #[test]
    fn pdf_from_image_has_one_page_of_matching_size() {
        let img = blue_mark(400, 300);
        let bytes = new_pdf_from_image(&img, 300.0, 225.0);
        let doc = lopdf::Document::load_mem(&bytes).expect("reload");
        let pages = doc.get_pages();
        assert_eq!(pages.len(), 1);
        let id = *pages.values().next().unwrap();
        let page = doc.get_dictionary(id).unwrap();
        let mb: Vec<f32> = page
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_float().unwrap())
            .collect();
        assert_eq!(mb, vec![0.0, 0.0, 300.0, 225.0]);
    }

    #[test]
    fn jpg_and_png_headers() {
        let _com = Com::new();
        let img = (64u32, 48u32, white(64, 48));
        let png = save_image(img.clone(), ImageFormat::Png).unwrap();
        assert_eq!(&png[..4], b"\x89PNG");
        let jpg = save_image(img.clone(), ImageFormat::Jpg).unwrap();
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
        let pdf = save_image(img, ImageFormat::Pdf).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        let doc = lopdf::Document::load_mem(&pdf).unwrap();
        assert_eq!(doc.get_pages().len(), 1);
    }
}
