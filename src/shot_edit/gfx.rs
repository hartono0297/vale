//! Direct2D and DirectWrite plumbing shared by the capture overlay and the
//! editor (and the export worker, which makes its own `Gfx`): factories,
//! cached text formats, brushes, bitmaps. Pixels, not DIPs: every target
//! here is set to 96 dpi, so one unit is one physical pixel.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_PROPERTIES, D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_DASH,
    D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINE_JOIN_MITER, D2D1_LINE_JOIN_ROUND,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory,
    ID2D1HwndRenderTarget, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::Result;
use windows_numerics::Vector2;

use crate::ui::fonts::{Face, Role};

/// Where text sits in its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// The factories and the font, made once per window (or worker).
pub struct Gfx {
    pub d2d: ID2D1Factory,
    pub dwrite: IDWriteFactory,
    face: Face,
    formats: RefCell<HashMap<(u32, i32), IDWriteTextFormat>>,
    /// Round caps and joins: pens, arrows, outlines.
    pub round: ID2D1StrokeStyle,
    /// Flat dashes: the inline capture's selection border.
    pub dash: ID2D1StrokeStyle,
}

impl Gfx {
    pub fn new() -> Result<Gfx> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let face = Face::for_role(&dwrite, Role::Text);
            let round = d2d.CreateStrokeStyle(
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
            )?;
            let dash = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_FLAT,
                    endCap: D2D1_CAP_STYLE_FLAT,
                    dashCap: D2D1_CAP_STYLE_FLAT,
                    lineJoin: D2D1_LINE_JOIN_MITER,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_DASH,
                    dashOffset: 0.0,
                },
                None,
            )?;
            Ok(Gfx {
                d2d,
                dwrite,
                face,
                formats: RefCell::new(HashMap::new()),
                round,
                dash,
            })
        }
    }

    fn format(&self, size: f32, weight: i32) -> Result<IDWriteTextFormat> {
        let key = (size.to_bits(), weight);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        let f = self
            .face
            .format(&self.dwrite, DWRITE_FONT_WEIGHT(weight), size)?;
        self.formats.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    /// `text` laid out unwrapped (its own lines at '\n') at `size` px.
    pub fn layout(&self, text: &str, size: f32, weight: i32) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let format = self.format(size, weight)?;
        unsafe { self.dwrite.CreateTextLayout(&wide, &format, 8192.0, 8192.0) }
    }

    /// Width and height of `text`.
    pub fn measure(&self, text: &str, size: f32, weight: i32) -> (f32, f32) {
        let Ok(layout) = self.layout(text, size, weight) else {
            return crate::shot_edit::text_extent(text, size);
        };
        let mut m = DWRITE_TEXT_METRICS::default();
        if unsafe { layout.GetMetrics(&mut m) }.is_err() {
            return crate::shot_edit::text_extent(text, size);
        }
        (m.widthIncludingTrailingWhitespace.ceil(), m.height.ceil())
    }

    /// One line (or a few) of `text` with its top-left at `(x, y)`.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        text: &str,
        (x, y): (f32, f32),
        size: f32,
        weight: i32,
        rgb: u32,
        alpha: f32,
    ) {
        let Ok(layout) = self.layout(text, size, weight) else {
            return;
        };
        unsafe {
            brush.SetColor(&color(rgb, alpha));
            rt.DrawTextLayout(
                Vector2::new(x, y),
                &layout,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    /// `text` on one line, centred vertically in the box and aligned inside
    /// it, trimmed with the box.
    #[allow(clippy::too_many_arguments)]
    pub fn text_in(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        text: &str,
        (x, y, w, h): (f32, f32, f32, f32),
        size: f32,
        weight: i32,
        rgb: u32,
        alpha: f32,
        align: Align,
    ) {
        if text.is_empty() {
            return;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let Ok(format) = self.format(size, weight) else {
            return;
        };
        let Ok(layout) = (unsafe { self.dwrite.CreateTextLayout(&wide, &format, w, h) }) else {
            return;
        };
        unsafe {
            let _ = layout.SetTextAlignment(match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            });
            let _ = layout.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = layout.SetWordWrapping(
                windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_NO_WRAP,
            );
            brush.SetColor(&color(rgb, alpha));
            rt.DrawTextLayout(
                Vector2::new(x, y),
                &layout,
                brush,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
        }
    }

    /// A render target on `hwnd` of `size` physical pixels, at 96 dpi so a
    /// unit is a pixel. Hardware when it can, software when it must.
    pub fn hwnd_target(&self, hwnd: HWND, size: (u32, u32)) -> Result<Target> {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: D2D_SIZE_U {
                width: size.0.max(1),
                height: size.1.max(1),
            },
            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
        };
        unsafe {
            let rt = self.d2d.CreateHwndRenderTarget(&props, &hwnd_props)?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush = rt.CreateSolidColorBrush(&color(0, 1.0), None)?;
            Ok(Target { rt, brush })
        }
    }
}

/// A window's render target and its one brush.
pub struct Target {
    pub rt: ID2D1HwndRenderTarget,
    pub brush: ID2D1SolidColorBrush,
}

impl Target {
    pub fn resize(&self, w: u32, h: u32) {
        unsafe {
            let _ = self.rt.Resize(&D2D_SIZE_U {
                width: w.max(1),
                height: h.max(1),
            });
        }
    }
}

/// `bgra` (`w` × `h`, top-down, opaque) as a bitmap of `rt`.
pub fn bitmap(rt: &ID2D1RenderTarget, w: u32, h: u32, bgra: &[u8]) -> Result<ID2D1Bitmap> {
    let props = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
    };
    unsafe {
        rt.CreateBitmap(
            D2D_SIZE_U {
                width: w,
                height: h,
            },
            Some(bgra.as_ptr().cast()),
            w * 4,
            &props,
        )
    }
}

/// `0xRRGGBB` and an alpha as a Direct2D colour.
pub fn color(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    let channel = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
    D2D1_COLOR_F {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: alpha,
    }
}

pub fn rect_f(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_split_into_channels() {
        let c = color(0xE5484D, 0.5);
        assert!((c.r - 229.0 / 255.0).abs() < 1e-6);
        assert!((c.g - 72.0 / 255.0).abs() < 1e-6);
        assert!((c.b - 77.0 / 255.0).abs() < 1e-6);
        assert_eq!(c.a, 0.5);
    }

    #[test]
    fn a_rect_is_edges_not_a_size() {
        let r = rect_f(10.0, 20.0, 30.0, 40.0);
        assert_eq!((r.left, r.top, r.right, r.bottom), (10.0, 20.0, 40.0, 60.0));
    }
}
