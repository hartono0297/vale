//! The GPU side of a frame: the D3D11 device shared by the capture, the
//! compositor and the encoder; a copy of the newest captured frame with the
//! cursor and the click ripples drawn on it (Direct2D on the DXGI surface);
//! then the D3D11 video processor crops the zoom view, scales it to the
//! video size and converts it to NV12 for the encoder, in one pass.

use std::mem::ManuallyDrop;

use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::Foundation::{HMODULE, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
    D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_MULTI_THREADED, D2D1_INTERPOLATION_MODE_LINEAR, D2D1CreateFactory,
    ID2D1Bitmap1, ID2D1DeviceContext, ID2D1Factory1, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11_TEX2D_ARRAY_VPOV, D3D11_TEX2D_VPIV,
    D3D11_TEX2D_VPOV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_VIDEO_COLOR,
    D3D11_VIDEO_COLOR_0, D3D11_VIDEO_COLOR_RGBA, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT,
    D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_STREAM,
    D3D11_VIDEO_USAGE_OPTIMAL_QUALITY, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2DARRAY, D3D11CreateDevice,
    ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D, ID3D11VideoContext,
    ID3D11VideoContext1, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetObjectW, HGDIOBJ, SelectObject,
};
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, HICON, ICONINFO,
};
use windows::core::Interface;

use super::Error;
use super::plan::IRect;

/// The D3D11 device of a recording, shared with the capture (as a WinRT
/// device) and Media Foundation (through its DXGI device manager).
pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub winrt: IDirect3DDevice,
    /// The graphics adapter's name ("NVIDIA GeForce ...").
    pub adapter: String,
}

impl Gpu {
    pub fn new() -> Result<Gpu, Error> {
        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
        let make = |driver: D3D_DRIVER_TYPE| {
            let mut device = None;
            let mut context = None;
            unsafe {
                D3D11CreateDevice(
                    None,
                    driver,
                    HMODULE::default(),
                    flags,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    None,
                    Some(&mut context),
                )
            }
            .ok()
            .and_then(|_| Some((device?, context?)))
        };
        let (device, context) = make(D3D_DRIVER_TYPE_HARDWARE)
            .or_else(|| make(D3D_DRIVER_TYPE_WARP))
            .ok_or(Error::Gpu("D3D11CreateDevice".into()))?;
        // Media Foundation's encoder uses the same immediate context from
        // its own threads.
        if let Ok(mt) = device.cast::<ID3D11Multithread>() {
            unsafe {
                let _ = mt.SetMultithreadProtected(true);
            }
        }
        let dxgi: IDXGIDevice = device.cast().map_err(|e| Error::win("IDXGIDevice", &e))?;
        let winrt: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
            .and_then(|i| i.cast())
            .map_err(|e| Error::win("CreateDirect3D11DeviceFromDXGIDevice", &e))?;
        let adapter = unsafe { dxgi.GetAdapter().and_then(|a| a.GetDesc()) }
            .map(|d| {
                let n = d
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(d.Description.len());
                String::from_utf16_lossy(&d.Description[..n])
            })
            .unwrap_or_default();
        Ok(Gpu {
            device,
            context,
            winrt,
            adapter,
        })
    }
}

/// A click ripple: how big and how strong it is `age_ms` after the click,
/// `None` once it is over. `scale` follows the cursor's size (DPI, the
/// accessibility size).
pub fn ripple_at(age_ms: f64, scale: f64) -> Option<(f32, f32)> {
    const LIFE_MS: f64 = 450.0;
    if !(0.0..LIFE_MS).contains(&age_ms) {
        return None;
    }
    let t = age_ms / LIFE_MS;
    let grow = 1.0 - (1.0 - t).powi(3);
    let radius = (6.0 + 22.0 * grow) * scale;
    let alpha = 0.9 * (1.0 - t);
    Some((radius as f32, alpha as f32))
}

/// The radius of the highlight disc under the cursor; `scale` as for
/// [`ripple_at`].
pub fn highlight_radius(scale: f64) -> f32 {
    (20.0 * scale) as f32
}

/// A cursor image with straight-from-the-system pixels: premultiplied BGRA
/// from two draws of the cursor, one on black and one on white. A pixel that
/// is the same on both is opaque; the difference gives the coverage. A pixel
/// that inverts the screen (the text cursor) has no colour of its own: it is
/// drawn opaque black.
pub fn alpha_from_black_white(black: &[u8], white: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; black.len().min(white.len()) / 4 * 4];
    for ((o, b), w) in out
        .chunks_exact_mut(4)
        .zip(black.chunks_exact(4))
        .zip(white.chunks_exact(4))
    {
        let diff: i32 = (0..3).map(|c| w[c] as i32 - b[c] as i32).sum::<i32>() / 3;
        if diff < -16 {
            o.copy_from_slice(&[0, 0, 0, 255]);
            continue;
        }
        let a = (255 - diff.max(0)).clamp(0, 255) as u8;
        for c in 0..3 {
            o[c] = b[c].min(a);
        }
        o[3] = a;
    }
    out
}

/// The cursor's picture, made once per cursor handle.
struct CursorImage {
    handle: isize,
    bitmap: ID2D1Bitmap1,
    hotspot: (i32, i32),
    size: (u32, u32),
}

/// Draws `cursor` into a 32-bit DIB filled with `fill`, returns the pixels.
unsafe fn draw_cursor_on(cursor: HICON, w: i32, h: i32, fill: u8) -> Option<Vec<u8>> {
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let mut bits = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok();
        let mut out = None;
        if let Some(dib) = dib {
            let len = (w * h * 4) as usize;
            let pixels = std::slice::from_raw_parts_mut(bits.cast::<u8>(), len);
            pixels.fill(fill);
            let old = SelectObject(dc, HGDIOBJ(dib.0));
            if DrawIconEx(dc, 0, 0, cursor, w, h, 0, None, DI_NORMAL).is_ok() {
                out = Some(pixels.to_vec());
            }
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(dib.0));
        }
        let _ = DeleteDC(dc);
        out
    }
}

/// The size and hotspot of a cursor.
unsafe fn cursor_shape(cursor: HICON) -> Option<((i32, i32), (i32, i32))> {
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(cursor, &mut info).ok()?;
        let mut bm = BITMAP::default();
        let size = if !info.hbmColor.is_invalid() {
            GetObjectW(
                HGDIOBJ(info.hbmColor.0),
                size_of::<BITMAP>() as i32,
                Some((&raw mut bm).cast()),
            );
            (bm.bmWidth, bm.bmHeight)
        } else {
            GetObjectW(
                HGDIOBJ(info.hbmMask.0),
                size_of::<BITMAP>() as i32,
                Some((&raw mut bm).cast()),
            );
            // A monochrome cursor's mask holds the AND and XOR halves.
            (bm.bmWidth, bm.bmHeight / 2)
        };
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        }
        (size.0 > 0 && size.1 > 0).then_some((size, (info.xHotspot as i32, info.yHotspot as i32)))
    }
}

/// The cursor now: screen position and handle; `None` while it is hidden.
pub fn cursor_now() -> Option<((i32, i32), isize)> {
    let mut info = CURSORINFO {
        cbSize: size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetCursorInfo(&mut info) }.ok()?;
    let pos = (info.ptScreenPos.x, info.ptScreenPos.y);
    (info.flags.0 & CURSOR_SHOWING.0 != 0 && !info.hCursor.is_invalid())
        .then_some((pos, info.hCursor.0 as isize))
}

/// What goes on top of a frame, in source pixels.
pub struct Overlay<'a> {
    /// Cursor position and handle (`None`: hidden or outside).
    pub cursor: Option<((f32, f32), isize)>,
    /// Ripples: position and age in ms.
    pub ripples: &'a [((f32, f32), f64)],
    /// A soft disc under the cursor.
    pub highlight: bool,
}

struct Processor {
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    input: ID3D11VideoProcessorInputView,
}

/// The per-recording compositor.
pub struct Compositor {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    d2d: ID2D1DeviceContext,
    fill: ID2D1SolidColorBrush,
    ring: ID2D1SolidColorBrush,
    glow: ID2D1SolidColorBrush,
    fps: u32,
    out: (u32, u32),
    src: (u32, u32),
    /// The newest captured frame, untouched.
    latest: ID3D11Texture2D,
    /// `latest` plus the overlay, read by the video processor.
    canvas: ID3D11Texture2D,
    canvas_target: ID2D1Bitmap1,
    processor: Processor,
    outputs: Vec<(usize, u32, ID3D11VideoProcessorOutputView)>,
    cursor: Option<CursorImage>,
}

fn texture(device: &ID3D11Device, w: u32, h: u32) -> Result<ID3D11Texture2D, Error> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        ..Default::default()
    };
    let mut tex = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut tex)) }
        .map_err(|e| Error::win("CreateTexture2D", &e))?;
    tex.ok_or(Error::Gpu("CreateTexture2D".into()))
}

fn rect(r: IRect) -> RECT {
    RECT {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

impl Compositor {
    /// A compositor for `src` sized frames into an `out` sized video.
    pub fn new(gpu: &Gpu, src: (u32, u32), out: (u32, u32), fps: u32) -> Result<Compositor, Error> {
        let video_device: ID3D11VideoDevice = gpu
            .device
            .cast()
            .map_err(|e| Error::win("ID3D11VideoDevice", &e))?;
        let video_context: ID3D11VideoContext = gpu
            .context
            .cast()
            .map_err(|e| Error::win("ID3D11VideoContext", &e))?;
        let factory: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None) }
                .map_err(|e| Error::win("D2D1CreateFactory", &e))?;
        let dxgi: IDXGIDevice = gpu
            .device
            .cast()
            .map_err(|e| Error::win("IDXGIDevice", &e))?;
        let d2d = unsafe {
            factory
                .CreateDevice(&dxgi)
                .and_then(|d| d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE))
        }
        .map_err(|e| Error::win("ID2D1DeviceContext", &e))?;
        let brush = |r: f32, g: f32, b: f32| unsafe {
            d2d.CreateSolidColorBrush(&D2D1_COLOR_F { r, g, b, a: 1.0 }, None)
                .map_err(|e| Error::win("CreateSolidColorBrush", &e))
        };
        let fill = brush(1.0, 1.0, 1.0)?;
        let ring = brush(0.13, 0.13, 0.15)?;
        let glow = brush(1.0, 0.835, 0.29)?;
        let latest = texture(&gpu.device, src.0, src.1)?;
        let canvas = texture(&gpu.device, src.0, src.1)?;
        let canvas_target = Self::target_for(&d2d, &canvas)?;
        let processor = Self::processor(&video_device, &canvas, src, out, fps)?;
        let me = Compositor {
            device: gpu.device.clone(),
            context: gpu.context.clone(),
            video_device,
            video_context,
            d2d,
            fill,
            ring,
            glow,
            fps,
            out,
            src,
            latest,
            canvas,
            canvas_target,
            processor,
            outputs: Vec::new(),
            cursor: None,
        };
        me.configure();
        Ok(me)
    }

    fn target_for(d2d: &ID2D1DeviceContext, tex: &ID3D11Texture2D) -> Result<ID2D1Bitmap1, Error> {
        let surface: IDXGISurface = tex.cast().map_err(|e| Error::win("IDXGISurface", &e))?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        unsafe { d2d.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
            .map_err(|e| Error::win("CreateBitmapFromDxgiSurface", &e))
    }

    fn processor(
        video: &ID3D11VideoDevice,
        canvas: &ID3D11Texture2D,
        src: (u32, u32),
        out: (u32, u32),
        fps: u32,
    ) -> Result<Processor, Error> {
        let rate = DXGI_RATIONAL {
            Numerator: fps,
            Denominator: 1,
        };
        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: src.0,
            InputHeight: src.1,
            OutputFrameRate: rate,
            OutputWidth: out.0,
            OutputHeight: out.1,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_QUALITY,
        };
        let enumerator = unsafe { video.CreateVideoProcessorEnumerator(&desc) }
            .map_err(|e| Error::win("CreateVideoProcessorEnumerator", &e))?;
        let supports = |format, flag: i32| unsafe {
            enumerator
                .CheckVideoProcessorFormat(format)
                .is_ok_and(|s| s as i32 & flag != 0)
        };
        if !supports(
            DXGI_FORMAT_B8G8R8A8_UNORM,
            D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0,
        ) || !supports(
            DXGI_FORMAT_NV12,
            D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0,
        ) {
            return Err(Error::Gpu(
                "the video processor cannot convert BGRA to NV12".into(),
            ));
        }
        let processor = unsafe { video.CreateVideoProcessor(&enumerator, 0) }
            .map_err(|e| Error::win("CreateVideoProcessor", &e))?;
        let view_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut input = None;
        unsafe {
            video.CreateVideoProcessorInputView(canvas, &enumerator, &view_desc, Some(&mut input))
        }
        .map_err(|e| Error::win("CreateVideoProcessorInputView", &e))?;
        Ok(Processor {
            enumerator,
            processor,
            input: input.ok_or(Error::Gpu("CreateVideoProcessorInputView".into()))?,
        })
    }

    /// The processor's fixed settings: progressive, no driver "enhancements",
    /// black bars, full-range RGB in, BT.709 studio-range YUV out.
    fn configure(&self) {
        let p = &self.processor.processor;
        let vc = &self.video_context;
        unsafe {
            vc.VideoProcessorSetStreamFrameFormat(p, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
            vc.VideoProcessorSetStreamAutoProcessingMode(p, 0, false);
            let black = D3D11_VIDEO_COLOR {
                Anonymous: D3D11_VIDEO_COLOR_0 {
                    RGBA: D3D11_VIDEO_COLOR_RGBA {
                        R: 0.0,
                        G: 0.0,
                        B: 0.0,
                        A: 1.0,
                    },
                },
            };
            vc.VideoProcessorSetOutputBackgroundColor(p, false, &black);
            let full = RECT {
                left: 0,
                top: 0,
                right: self.out.0 as i32,
                bottom: self.out.1 as i32,
            };
            vc.VideoProcessorSetOutputTargetRect(p, true, Some(&full));
            if let Ok(vc1) = vc.cast::<ID3D11VideoContext1>() {
                vc1.VideoProcessorSetStreamColorSpace1(
                    p,
                    0,
                    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
                );
                vc1.VideoProcessorSetOutputColorSpace1(
                    p,
                    DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                );
            }
        }
    }

    pub fn source_size(&self) -> (u32, u32) {
        self.src
    }

    /// The source changed size: new textures and a new processor. The
    /// video's size stays.
    pub fn resize_source(&mut self, src: (u32, u32)) -> Result<(), Error> {
        if src == self.src {
            return Ok(());
        }
        unsafe { self.d2d.SetTarget(None) };
        self.latest = texture(&self.device, src.0, src.1)?;
        self.canvas = texture(&self.device, src.0, src.1)?;
        self.canvas_target = Self::target_for(&self.d2d, &self.canvas)?;
        self.processor =
            Self::processor(&self.video_device, &self.canvas, src, self.out, self.fps)?;
        self.outputs.clear();
        self.src = src;
        self.configure();
        Ok(())
    }

    /// Keeps a copy of a captured frame (the capture's buffer goes back to
    /// its pool right after).
    pub fn take_frame(&self, frame: &ID3D11Texture2D) {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { frame.GetDesc(&mut desc) };
        if (desc.Width, desc.Height) == self.src {
            unsafe { self.context.CopyResource(&self.latest, frame) };
        } else {
            let b = windows::Win32::Graphics::Direct3D11::D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: desc.Width.min(self.src.0),
                bottom: desc.Height.min(self.src.1),
                back: 1,
            };
            unsafe {
                self.context
                    .CopySubresourceRegion(&self.latest, 0, 0, 0, 0, frame, 0, Some(&b))
            };
        }
    }

    fn cursor_image(&mut self, handle: isize) -> Option<&CursorImage> {
        if self.cursor.as_ref().is_some_and(|c| c.handle == handle) {
            return self.cursor.as_ref();
        }
        let cursor = HICON(handle as _);
        let ((w, h), hotspot) = unsafe { cursor_shape(cursor) }?;
        let black = unsafe { draw_cursor_on(cursor, w, h, 0) }?;
        let white = unsafe { draw_cursor_on(cursor, w, h, 255) }?;
        let pixels = alpha_from_black_white(&black, &white);
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: ManuallyDrop::new(None),
        };
        let bitmap = unsafe {
            self.d2d.CreateBitmap(
                D2D_SIZE_U {
                    width: w as u32,
                    height: h as u32,
                },
                Some(pixels.as_ptr().cast()),
                (w * 4) as u32,
                &props,
            )
        }
        .ok()?;
        self.cursor = Some(CursorImage {
            handle,
            bitmap,
            hotspot,
            size: (w as u32, h as u32),
        });
        self.cursor.as_ref()
    }

    /// Copies the newest frame to the canvas and draws the overlay on it.
    pub fn draw_overlay(&mut self, overlay: &Overlay) -> Result<(), Error> {
        unsafe { self.context.CopyResource(&self.canvas, &self.latest) };
        if overlay.cursor.is_none() && overlay.ripples.is_empty() {
            return Ok(());
        }
        let cursor = overlay.cursor.and_then(|(at, handle)| {
            self.cursor_image(handle)
                .map(|c| (at, c.bitmap.clone(), c.hotspot, c.size))
        });
        // The ripple grows with the cursor (DPI, accessibility size).
        let scale = cursor
            .as_ref()
            .map_or(1.0, |c| c.3.1 as f64 / 32.0)
            .max(0.5);
        let d2d = &self.d2d;
        unsafe {
            d2d.SetTarget(&self.canvas_target);
            d2d.BeginDraw();
            if let (true, Some(((x, y), _))) = (overlay.highlight, overlay.cursor)
                && cursor.is_some()
            {
                let radius = highlight_radius(scale);
                let disc = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: x, Y: y },
                    radiusX: radius,
                    radiusY: radius,
                };
                self.glow.SetOpacity(0.35);
                d2d.FillEllipse(&disc, &self.glow);
            }
            for &((x, y), age) in overlay.ripples {
                let Some((radius, alpha)) = ripple_at(age, scale) else {
                    continue;
                };
                let ellipse = D2D1_ELLIPSE {
                    point: windows_numerics::Vector2 { X: x, Y: y },
                    radiusX: radius,
                    radiusY: radius,
                };
                self.fill.SetOpacity(alpha * 0.35);
                d2d.FillEllipse(&ellipse, &self.fill);
                self.ring.SetOpacity(alpha * 0.8);
                d2d.DrawEllipse(&ellipse, &self.ring, 4.0 * scale as f32, None);
                self.fill.SetOpacity(alpha);
                d2d.DrawEllipse(&ellipse, &self.fill, 2.0 * scale as f32, None);
            }
            if let Some(((x, y), bitmap, hotspot, size)) = &cursor {
                let left = x - hotspot.0 as f32;
                let top = y - hotspot.1 as f32;
                let dest = D2D_RECT_F {
                    left,
                    top,
                    right: left + size.0 as f32,
                    bottom: top + size.1 as f32,
                };
                d2d.DrawBitmap(
                    bitmap,
                    Some(&dest),
                    1.0,
                    D2D1_INTERPOLATION_MODE_LINEAR,
                    None,
                    None,
                );
            }
            let done = d2d.EndDraw(None, None);
            d2d.SetTarget(None);
            done.map_err(|e| Error::win("EndDraw", &e))
        }
    }

    fn output_view(
        &mut self,
        out: &ID3D11Texture2D,
        subresource: u32,
    ) -> Result<ID3D11VideoProcessorOutputView, Error> {
        let key = out.as_raw() as usize;
        if let Some((_, _, v)) = self
            .outputs
            .iter()
            .find(|(k, s, _)| *k == key && *s == subresource)
        {
            return Ok(v.clone());
        }
        let desc = if subresource == 0 {
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            }
        } else {
            D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2DARRAY,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2DArray: D3D11_TEX2D_ARRAY_VPOV {
                        MipSlice: 0,
                        FirstArraySlice: subresource,
                        ArraySize: 1,
                    },
                },
            }
        };
        let mut view = None;
        unsafe {
            self.video_device.CreateVideoProcessorOutputView(
                out,
                &self.processor.enumerator,
                &desc,
                Some(&mut view),
            )
        }
        .map_err(|e| Error::win("CreateVideoProcessorOutputView", &e))?;
        let view = view.ok_or(Error::Gpu("CreateVideoProcessorOutputView".into()))?;
        // The allocator cycles through a handful of textures.
        if self.outputs.len() >= 16 {
            self.outputs.remove(0);
        }
        // The view holds a reference to `out`, so the key cannot be reused
        // by another texture while it is cached.
        self.outputs.push((key, subresource, view.clone()));
        Ok(view)
    }

    /// Scales `src` of the canvas into `dst` of `out` (NV12, the video
    /// size); the rest of `out` is black. `None` view: all black.
    pub fn render(
        &mut self,
        view: Option<(IRect, IRect)>,
        out: &ID3D11Texture2D,
        subresource: u32,
    ) -> Result<(), Error> {
        let output = self.output_view(out, subresource)?;
        let p = self.processor.processor.clone();
        let vc = self.video_context.clone();
        let (src, dst) = view.unwrap_or((
            IRect {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            },
            IRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
        ));
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: view.is_some().into(),
            pInputSurface: ManuallyDrop::new(Some(self.processor.input.clone())),
            ..Default::default()
        };
        let mut streams = [stream];
        let result = unsafe {
            vc.VideoProcessorSetStreamSourceRect(&p, 0, true, Some(&rect(src)));
            vc.VideoProcessorSetStreamDestRect(&p, 0, true, Some(&rect(dst)));
            vc.VideoProcessorBlt(&p, &output, 0, &streams)
        };
        unsafe { ManuallyDrop::drop(&mut streams[0].pInputSurface) };
        result.map_err(|e| Error::win("VideoProcessorBlt", &e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highlight_follows_the_cursor_size() {
        assert_eq!(highlight_radius(1.0), 20.0);
        assert_eq!(highlight_radius(2.0), 40.0);
        // Wider than a fresh ripple, so the ripple starts inside it.
        assert!(highlight_radius(1.0) > ripple_at(0.0, 1.0).unwrap().0);
    }

    #[test]
    fn a_ripple_grows_and_fades() {
        let (r0, a0) = ripple_at(0.0, 1.0).unwrap();
        let (r1, a1) = ripple_at(200.0, 1.0).unwrap();
        let (r2, a2) = ripple_at(440.0, 1.0).unwrap();
        assert!(r0 < r1 && r1 < r2);
        assert!(a0 > a1 && a1 > a2);
        assert!(ripple_at(450.0, 1.0).is_none());
        assert!(ripple_at(-1.0, 1.0).is_none());
        let (big, _) = ripple_at(200.0, 2.0).unwrap();
        assert!((big - 2.0 * r1).abs() < 1e-3);
    }

    #[test]
    fn cursor_pixels_get_their_alpha_from_two_draws() {
        let black = [
            0, 0, 0, 0, // transparent
            10, 20, 30, 0, // opaque colour
            50, 50, 50, 0, // half covered grey
            255, 255, 255, 0, // inverting (white on black, black on white)
        ];
        let white = [
            255, 255, 255, 0, //
            10, 20, 30, 0, //
            178, 178, 178, 0, //
            0, 0, 0, 0,
        ];
        let px = alpha_from_black_white(&black, &white);
        assert_eq!(&px[0..4], &[0, 0, 0, 0]);
        assert_eq!(&px[4..8], &[10, 20, 30, 255]);
        assert_eq!(&px[8..12], &[50, 50, 50, 127]);
        assert_eq!(&px[12..16], &[0, 0, 0, 255]);
    }
}
