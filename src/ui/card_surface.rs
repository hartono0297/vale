//! What the floating cards (the lyrics card, the caption card) draw with: a
//! software Direct2D target bound to a premultiplied DIB the size of the
//! window, pushed with `UpdateLayeredWindow` by the card, plus the DirectWrite
//! text, shape, layer and gradient helpers, and an eased value. Each card
//! keeps its own window, state and pointer handling.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::time::Instant;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Brush, ID2D1DCRenderTarget,
    ID2D1Factory, ID2D1Layer, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT, DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, HBITMAP, HDC, HGDIOBJ, SelectObject,
};
use windows::core::{Error, Interface, Result};
use windows_numerics::{Matrix3x2, Vector2};

use crate::config::UiFont;
use crate::lyrics_card::layout::{RectF, ease_out_cubic};
use crate::shot_edit::gfx::{color, rect_f};
use crate::ui::fonts::{Face, Role};
use crate::ui::geometry::Rect as GRect;
use crate::ui::icons::{Icon, IconSet};

/// The factories, the software target bound to a DIB the size of the window,
/// and what is cached per target.
pub(crate) struct Surface {
    // Declared first: released before the factories.
    icons: IconSet,
    layer: ID2D1Layer,
    pub(crate) brush: ID2D1SolidColorBrush,
    pub(crate) rt: ID2D1DCRenderTarget,
    face: Face,
    formats: HashMap<(u32, i32), IDWriteTextFormat>,
    dwrite: IDWriteFactory,
    /// Kept alive for the target and the icon geometries made from it.
    _d2d: ID2D1Factory,
    pub(crate) dc: HDC,
    dib: HBITMAP,
    previous: HGDIOBJ,
    size: (i32, i32),
}

fn make_dib(dc: HDC, size: (i32, i32)) -> Result<HBITMAP> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size.0,
            biHeight: -size.1,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    unsafe { CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) }
}

impl Surface {
    pub(crate) fn new(size: (i32, i32), font: UiFont) -> Result<Surface> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let face = Face::for_role_in(&dwrite, Role::Text, font);
            let dc = CreateCompatibleDC(None);
            let dib = match make_dib(dc, size) {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            let previous = SelectObject(dc, dib.into());
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let made = (|| {
                let rt = d2d.CreateDCRenderTarget(&props)?;
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                let brush = rt.CreateSolidColorBrush(&color(0, 1.0), None)?;
                let layer = rt.CreateLayer(None)?;
                let icons = IconSet::new(&d2d)?;
                Ok::<_, Error>((rt, brush, layer, icons))
            })();
            match made {
                Ok((rt, brush, layer, icons)) => Ok(Surface {
                    icons,
                    layer,
                    brush,
                    rt,
                    face,
                    formats: HashMap::new(),
                    dwrite,
                    _d2d: d2d,
                    dc,
                    dib,
                    previous,
                    size,
                }),
                Err(e) => {
                    SelectObject(dc, previous);
                    let _ = DeleteObject(dib.into());
                    let _ = DeleteDC(dc);
                    Err(e)
                }
            }
        }
    }

    /// A DIB of the new size (the target and the brushes stay).
    pub(crate) fn resize(&mut self, size: (i32, i32)) -> Result<()> {
        if size == self.size {
            return Ok(());
        }
        unsafe {
            let fresh = make_dib(self.dc, size)?;
            SelectObject(self.dc, fresh.into());
            let _ = DeleteObject(self.dib.into());
            self.dib = fresh;
        }
        self.size = size;
        Ok(())
    }

    pub(crate) fn set_font(&mut self, font: UiFont) {
        self.face = Face::for_role_in(&self.dwrite, Role::Text, font);
        self.formats.clear();
    }

    pub(crate) fn format(&mut self, size: f32, weight: i32) -> Option<IDWriteTextFormat> {
        let key = (size.to_bits(), weight);
        if let Some(f) = self.formats.get(&key) {
            return Some(f.clone());
        }
        let f = self
            .face
            .format(&self.dwrite, DWRITE_FONT_WEIGHT(weight), size)
            .ok()?;
        self.formats.insert(key, f.clone());
        Some(f)
    }

    /// `text` laid out `width` wide (wrapping), at `size` dip.
    pub(crate) fn layout(
        &mut self,
        text: &str,
        size: f32,
        weight: i32,
        width: f32,
    ) -> Option<IDWriteTextLayout> {
        let format = self.format(size, weight)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.dwrite
                .CreateTextLayout(&wide, &format, width.max(1.0), 8192.0)
                .ok()
        }
    }

    /// One line cut with an ellipsis at `width`.
    pub(crate) fn one_line(
        &mut self,
        text: &str,
        size: f32,
        weight: i32,
        width: f32,
    ) -> Option<IDWriteTextLayout> {
        let format = self.format(size, weight)?;
        let layout = self.layout(text, size, weight, width)?;
        unsafe {
            layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).ok()?;
            let ellipsis = self.dwrite.CreateEllipsisTrimmingSign(&format).ok()?;
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            layout.SetTrimming(&trimming, &ellipsis).ok()?;
        }
        Some(layout)
    }

    pub(crate) fn height_of(layout: &IDWriteTextLayout) -> f32 {
        Surface::metrics(layout).height
    }

    pub(crate) fn metrics(layout: &IDWriteTextLayout) -> DWRITE_TEXT_METRICS {
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = unsafe { layout.GetMetrics(&mut m) };
        m
    }

    /// How far the first line's baseline is below the layout's top.
    // Only the caption card asks, and nothing opens it in a release build yet.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) fn first_baseline(layout: &IDWriteTextLayout) -> f32 {
        let count = Surface::metrics(layout).lineCount.max(1) as usize;
        let mut lines = vec![DWRITE_LINE_METRICS::default(); count];
        let mut got = 0u32;
        match unsafe { layout.GetLineMetrics(Some(&mut lines), &mut got) } {
            Ok(()) if got > 0 => lines[0].baseline,
            _ => 0.0,
        }
    }

    /// `layout` centred in `area`, both ways.
    // Only the caption card asks, and nothing opens it in a release build yet.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) fn draw_centred(
        &self,
        area: RectF,
        layout: &IDWriteTextLayout,
        rgb: u32,
        alpha: f32,
    ) {
        let m = Surface::metrics(layout);
        let x = area.x + (area.w - m.widthIncludingTrailingWhitespace) / 2.0;
        let y = area.y + (area.h - m.height) / 2.0;
        self.draw_text((x - m.left, y), layout, rgb, alpha);
    }

    // Only the caption card asks, and nothing opens it in a release build yet.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) fn stroke_round(&self, r: RectF, radius: f32, rgb: u32, alpha: f32, width: f32) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.rt
                .DrawRoundedRectangle(&rounded(r, radius), &self.brush, width, None);
        }
    }

    pub(crate) fn draw_text(
        &self,
        origin: (f32, f32),
        layout: &IDWriteTextLayout,
        rgb: u32,
        alpha: f32,
    ) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.rt.DrawTextLayout(
                Vector2::new(origin.0, origin.1),
                layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }
    }

    pub(crate) fn fill_round(&self, r: RectF, radius: f32, rgb: u32, alpha: f32) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.rt
                .FillRoundedRectangle(&rounded(r, radius), &self.brush);
        }
    }

    pub(crate) fn icon(&self, icon: Icon, area: RectF, size: f32, rgb: u32, alpha: f32) {
        let _ = self.icons.draw(
            &self.rt,
            &self.brush,
            icon,
            GRect {
                x: area.x,
                y: area.y,
                w: area.w,
                h: area.h,
            },
            size,
            rgb,
            alpha,
            None,
        );
    }

    /// A vertical gradient brush over `[y0, y1]` from `top` to `bottom`.
    pub(crate) fn vertical_gradient(
        &self,
        y0: f32,
        y1: f32,
        stops: &[(f32, u32, f32)],
    ) -> Option<ID2D1Brush> {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(position, rgb, alpha)| D2D1_GRADIENT_STOP {
                position,
                color: color(rgb, alpha),
            })
            .collect();
        unsafe {
            let collection = self
                .rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                .ok()?;
            let brush = self
                .rt
                .CreateLinearGradientBrush(
                    &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: Vector2::new(0.0, y0),
                        endPoint: Vector2::new(0.0, y1),
                    },
                    None,
                    &collection,
                )
                .ok()?;
            brush.cast().ok()
        }
    }

    /// Pushes a layer over `bounds` that fades by `opacity`'s alpha. Give the
    /// returned brush back to `pop_layer` (it keeps the reference the layer
    /// was given until the layer is popped).
    pub(crate) fn push_layer(
        &self,
        bounds: RectF,
        opacity: Option<ID2D1Brush>,
    ) -> Option<ID2D1Brush> {
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: D2D_RECT_F {
                left: bounds.x,
                top: bounds.y,
                right: bounds.right(),
                bottom: bounds.bottom(),
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(opacity.clone()),
            layerOptions: D2D1_LAYER_OPTIONS_NONE,
        };
        unsafe { self.rt.PushLayer(&params, &self.layer) };
        drop(ManuallyDrop::into_inner(params.opacityBrush));
        opacity
    }

    pub(crate) fn pop_layer(&self, keep: Option<ID2D1Brush>) {
        unsafe { self.rt.PopLayer() };
        drop(keep);
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.dib.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

pub(crate) fn rounded(r: RectF, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: rect_f(r.x, r.y, r.w, r.h),
        radiusX: radius,
        radiusY: radius,
    }
}

/// An eased value.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tween {
    pub(crate) from: f32,
    pub(crate) to: f32,
    pub(crate) start: Instant,
    pub(crate) ms: u32,
}

impl Tween {
    pub(crate) fn at_rest(v: f32, now: Instant) -> Tween {
        Tween {
            from: v,
            to: v,
            start: now,
            ms: 0,
        }
    }

    pub(crate) fn t(&self, now: Instant) -> f32 {
        if self.ms == 0 {
            return 1.0;
        }
        (now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 / self.ms as f32)
            .clamp(0.0, 1.0)
    }

    pub(crate) fn at(&self, now: Instant) -> f32 {
        self.from + (self.to - self.from) * ease_out_cubic(self.t(now))
    }

    pub(crate) fn moving(&self, now: Instant) -> bool {
        self.ms > 0 && self.t(now) < 1.0
    }

    /// Heads for `to` from where it is now; at once when `animate` is false.
    pub(crate) fn go(&mut self, to: f32, ms: u32, animate: bool, now: Instant) {
        let here = self.at(now);
        *self = if animate && ms > 0 {
            Tween {
                from: here,
                to,
                start: now,
                ms,
            }
        } else {
            Tween::at_rest(to, now)
        };
    }
}
