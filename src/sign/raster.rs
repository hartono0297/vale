//! Shared pixel plumbing for signatures: an offscreen Direct2D target on a
//! WIC bitmap and lossless PNG with alpha. COM must be initialised by the
//! caller.

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
    GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory, WICBitmapCacheOnLoad,
    WICBitmapDitherTypeNone, WICBitmapEncoderNoCache, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, STREAM_SEEK_END, STREAM_SEEK_SET,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::Interface;

use super::Mark;

/// The most pixels a decoded mark may have (a corrupt header must not ask
/// for gigabytes).
const MAX_PIXELS: u64 = 40_000_000;

/// Runs `draw` on a transparent `w` x `h` Direct2D target over a WIC bitmap
/// (96 dpi, one unit is one pixel). Returns premultiplied BGRA, top-down,
/// `w * h * 4` bytes.
pub fn offscreen(
    w: u32,
    h: u32,
    draw: impl FnOnce(&ID2D1RenderTarget, &ID2D1Factory, &IDWriteFactory),
) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    unsafe {
        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let target = wic
            .CreateBitmap(w, h, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)
            .ok()?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let rt: ID2D1RenderTarget = d2d.CreateWicBitmapRenderTarget(&target, &props).ok()?;
        rt.BeginDraw();
        rt.Clear(Some(&D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }));
        draw(&rt, &d2d, &dwrite);
        rt.EndDraw(None, None).ok()?;
        let mut out = vec![0u8; w as usize * h as usize * 4];
        target.CopyPixels(std::ptr::null(), w * 4, &mut out).ok()?;
        Some(out)
    }
}

/// Premultiplied BGRA (`w` x `h`) as a straight RGBA `Mark`.
pub fn to_mark(w: u32, h: u32, pbgra: &[u8]) -> Mark {
    let mut rgba = Vec::with_capacity(pbgra.len());
    for px in pbgra.chunks_exact(4) {
        let a = u32::from(px[3]);
        let un = |c: u8| -> u8 {
            if a == 0 {
                0
            } else {
                ((u32::from(c) * 255 + a / 2) / a).min(255) as u8
            }
        };
        rgba.extend_from_slice(&[un(px[2]), un(px[1]), un(px[0]), px[3]]);
    }
    Mark { w, h, rgba }
}

/// `m` as a PNG with alpha (WIC, in a memory stream).
pub fn encode_png_rgba(m: &Mark) -> Option<Vec<u8>> {
    let expected = (m.w as usize).checked_mul(m.h as usize)?.checked_mul(4)?;
    if m.w == 0 || m.h == 0 || m.rgba.len() != expected {
        return None;
    }
    let mut bgra = m.rgba.clone();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let encoder = wic
            .CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())
            .ok()?;
        let stream = SHCreateMemStream(None)?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
        let mut frame = None;
        let mut options = None;
        encoder.CreateNewFrame(&mut frame, &mut options).ok()?;
        let frame = frame?;
        frame.Initialize(options.as_ref()).ok()?;
        frame.SetSize(m.w, m.h).ok()?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format).ok()?;
        if format != GUID_WICPixelFormat32bppBGRA {
            return None;
        }
        frame.WritePixels(m.h, m.w.checked_mul(4)?, &bgra).ok()?;
        frame.Commit().ok()?;
        encoder.Commit().ok()?;
        let mut len = 0u64;
        stream.Seek(0, STREAM_SEEK_END, Some(&mut len)).ok()?;
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let len32 = u32::try_from(len).ok()?;
        let mut bytes = vec![0u8; len32 as usize];
        let mut read = 0u32;
        stream
            .Read(bytes.as_mut_ptr().cast(), len32, Some(&mut read))
            .ok()
            .ok()?;
        (read == len32).then_some(bytes)
    }
}

/// A PNG (any colour type) as a straight RGBA `Mark`; `None` for anything
/// WIC cannot read or that is absurdly large.
pub fn decode_png_rgba(bytes: &[u8]) -> Option<Mark> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = wic.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let decoder = wic
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
            return None;
        }
        let source: IWICBitmapSource = frame.cast().ok()?;
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &source,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut rgba = vec![0u8; w as usize * h as usize * 4];
        converter.CopyPixels(std::ptr::null(), w * 4, &mut rgba).ok()?;
        for px in rgba.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        Some(Mark { w, h, rgba })
    }
}
