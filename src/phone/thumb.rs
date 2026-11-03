//! The phone page's thumbnails: a small JPEG of a shared image, made with
//! WIC (the same way `screenshot::win::encode_png` writes a PNG). Only ever
//! a thumbnail: a failure (a HEIC without the codec, a file that is not an
//! image, one over 64 MB) is `None`, and the page then shows the type icon.
//! The full image is never sent in its place, since a phone tab decoding a
//! dozen 12 MP photos can crash.
//!
//! COM is joined (multithreaded) on the calling connection thread for the
//! call and released after; no thread of the island's has to know.

use std::fs::File;
use std::os::windows::io::AsRawHandle;

use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR,
    IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICBitmapTransformRotate90,
    WICBitmapTransformRotate180, WICBitmapTransformRotate270, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{
    PROPBAG2, PROPVARIANT, PropVariantClear,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    STREAM_SEEK_END, STREAM_SEEK_SET,
};
use windows::Win32::System::Variant::{VARIANT, VT_R4, VT_UI2};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::{Interface, PWSTR, w};

/// The long side of a thumbnail, in pixels.
pub const MAX_SIDE: u32 = 192;
/// A file over this is not decoded for a thumbnail.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// JPEG quality (0 to 1).
const QUALITY: f32 = 0.8;

/// The size a `w` x `h` image is scaled to: the long side at most
/// `MAX_SIDE`, the other in proportion (at least 1), never enlarged.
pub fn scaled_size(w: u32, h: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= MAX_SIDE || long == 0 {
        return (w.max(1), h.max(1));
    }
    let scale = |side: u32| ((u64::from(side) * u64::from(MAX_SIDE) + u64::from(long) / 2) / u64::from(long)).max(1) as u32;
    (scale(w), scale(h))
}

/// A JPEG thumbnail of the image in `file` (opened with the share flags the
/// caller wants), or `None`.
pub fn make(file: &File) -> Option<Vec<u8>> {
    if file.metadata().ok()?.len() > MAX_BYTES {
        return None;
    }
    let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let jpeg = decode_and_encode(file);
    if joined {
        unsafe { CoUninitialize() };
    }
    jpeg
}

/// The EXIF orientation of the first frame as a WIC rotation, when it says
/// 3, 6 or 8 (the others, mirrored ones included, are ignored).
fn orientation(frame: &windows::Win32::Graphics::Imaging::IWICBitmapFrameDecode) -> Option<i32> {
    unsafe {
        let reader = frame.GetMetadataQueryReader().ok()?;
        let mut value = PROPVARIANT::default();
        reader
            .GetMetadataByName(w!("/app1/ifd/{ushort=274}"), &mut value)
            .ok()?;
        let inner = &value.Anonymous.Anonymous;
        let code = (inner.vt == VT_UI2).then(|| inner.Anonymous.uiVal);
        let _ = PropVariantClear(&mut value);
        match code? {
            3 => Some(WICBitmapTransformRotate180.0),
            6 => Some(WICBitmapTransformRotate90.0),
            8 => Some(WICBitmapTransformRotate270.0),
            _ => None,
        }
    }
}

fn decode_and_encode(file: &File) -> Option<Vec<u8>> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let handle = file.as_raw_handle() as usize;
        let decoder = wic
            .CreateDecoderFromFileHandle(handle, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let turn = orientation(&frame);
        let mut source: IWICBitmapSource = frame.cast().ok()?;
        if let Some(options) = turn {
            let rotator = wic.CreateBitmapFlipRotator().ok()?;
            rotator
                .Initialize(
                    &source,
                    windows::Win32::Graphics::Imaging::WICBitmapTransformOptions(options),
                )
                .ok()?;
            source = rotator.cast().ok()?;
        }
        let (mut w, mut h) = (0u32, 0u32);
        source.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        let (tw, th) = scaled_size(w, h);
        let scaler = wic.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&source, tw, th, WICBitmapInterpolationModeFant)
            .ok()?;
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
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
            let _ = bag.Write(1, &prop, &VARIANT::from(QUALITY));
        }
        out.Initialize(options.as_ref()).ok()?;
        out.SetSize(tw, th).ok()?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        out.SetPixelFormat(&mut format).ok()?;
        out.WriteSource(&converter, std::ptr::null()).ok()?;
        out.Commit().ok()?;
        encoder.Commit().ok()?;
        let mut len = 0u64;
        stream.Seek(0, STREAM_SEEK_END, Some(&mut len)).ok()?;
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let mut bytes = vec![0u8; usize::try_from(len).ok()?];
        let mut read = 0u32;
        stream
            .Read(
                bytes.as_mut_ptr().cast(),
                u32::try_from(len).ok()?,
                Some(&mut read),
            )
            .ok()
            .ok()?;
        (read as usize == bytes.len() && !bytes.is_empty()).then_some(bytes)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::phone::files::tests::temp_dir;

    /// The width and height a JPEG's first SOF marker says.
    fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                return None;
            }
            let marker = b[i + 1];
            let len = usize::from(b[i + 2]) << 8 | usize::from(b[i + 3]);
            if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u32::from(b[i + 5]) << 8 | u32::from(b[i + 6]);
                let w = u32::from(b[i + 7]) << 8 | u32::from(b[i + 8]);
                return Some((w, h));
            }
            i += 2 + len;
        }
        None
    }

    /// A `w` x `h` PNG of a colour gradient, written to `path`.
    pub(crate) fn write_gradient_png(path: &std::path::Path, w: u32, h: u32) {
        let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                bgra.extend_from_slice(&[
                    (y * 255 / h.max(1)) as u8,
                    (x * 255 / w.max(1)) as u8,
                    (255 - x * 255 / w.max(1)) as u8,
                    255,
                ]);
            }
        }
        let png = crate::screenshot::win::encode_png(w, h, &bgra).expect("a png");
        if joined {
            unsafe { CoUninitialize() };
        }
        std::fs::write(path, png).expect("write png");
    }

    #[test]
    fn a_size_is_scaled_to_the_long_side_and_never_enlarged() {
        assert_eq!(scaled_size(640, 480), (192, 144));
        assert_eq!(scaled_size(480, 640), (144, 192));
        assert_eq!(scaled_size(4000, 3000), (192, 144));
        assert_eq!(scaled_size(192, 100), (192, 100));
        assert_eq!(scaled_size(100, 50), (100, 50));
        assert_eq!(scaled_size(1, 1), (1, 1));
        // A very thin image keeps at least one pixel.
        assert_eq!(scaled_size(100_000, 10), (192, 1));
        assert_eq!(scaled_size(0, 0), (1, 1));
    }

    #[test]
    fn the_thumbnail_of_a_png_is_a_small_jpeg() {
        let dir = temp_dir("phone-thumb");
        let path = dir.join("foto.png");
        write_gradient_png(&path, 640, 480);
        let file = std::fs::File::open(&path).expect("open");
        let jpeg = make(&file).expect("a thumbnail");
        assert_eq!(&jpeg[..2], [0xFF, 0xD8], "a JPEG starts with SOI");
        let (w, h) = jpeg_size(&jpeg).expect("a size");
        assert!(w.max(h) <= MAX_SIDE, "{w} x {h}");
        assert_eq!((w, h), (192, 144));
        assert!(jpeg.len() < 20_000, "a few KB, not {}", jpeg.len());
        // A small image is not enlarged.
        let small = dir.join("small.png");
        write_gradient_png(&small, 50, 40);
        let jpeg = make(&std::fs::File::open(&small).expect("open")).expect("thumb");
        assert_eq!(jpeg_size(&jpeg), Some((50, 40)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_an_image_or_too_big_has_no_thumbnail() {
        let dir = temp_dir("phone-thumb-no");
        let txt = dir.join("a.png");
        std::fs::write(&txt, b"this is not a png").expect("write");
        assert!(make(&std::fs::File::open(&txt).expect("open")).is_none());
        let empty = dir.join("empty.png");
        std::fs::write(&empty, b"").expect("write");
        assert!(make(&std::fs::File::open(&empty).expect("open")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
