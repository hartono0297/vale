//! Decodes an image file to BGRA pixels with WIC, for the shelf's card
//! thumbnails and its large preview. Shares the decode, EXIF rotation and
//! scale steps with the phone page's thumbnails (`phone::thumb`).
//!
//! COM is joined (multithreaded) on the calling thread for the call and
//! released after, like `phone::thumb::make`.

use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
    WICRect,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};

use crate::clipboard::image::Thumb;
use crate::phone::thumb::{MAX_BYTES, oriented_source, scaled_to};

/// The long side of a card thumbnail, in pixels.
pub const CARD_SIDE: u32 = 192;
/// The long side of the large preview, in pixels.
pub const PREVIEW_SIDE: u32 = 1024;

/// The most pixels an image may declare (WIC decodes them before scaling, so
/// a small, highly compressed file can still ask for gigabytes).
const MAX_PIXELS: u64 = 250_000_000;

fn too_many_pixels(w: u32, h: u32) -> bool {
    u64::from(w) * u64::from(h) > MAX_PIXELS
}

/// The image at `path` scaled to `max_side` on its long side (never
/// enlarged), as top-down premultiplied BGRA (alpha kept), or `None` (not a file,
/// empty, over `MAX_BYTES`, over `MAX_PIXELS`, not an image WIC can decode).
pub fn decode(path: &Path, max_side: u32) -> Option<Thumb> {
    // Share read, write and delete so a file another program has open, or
    // is about to move, still opens.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(path)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_BYTES {
        return None;
    }
    let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let thumb = unsafe { decode_file(&file, max_side) };
    if joined {
        unsafe { CoUninitialize() };
    }
    thumb
}

unsafe fn decode_file(file: &std::fs::File, max_side: u32) -> Option<Thumb> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let source = oriented_source(&wic, file)?;
        let (mut w, mut h) = (0u32, 0u32);
        source.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || too_many_pixels(w, h) {
            return None;
        }
        let (tw, th) = scaled_to(w, h, max_side);
        let scaler = wic.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&source, tw, th, WICBitmapInterpolationModeFant)
            .ok()?;
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let stride = tw.checked_mul(4)?;
        let mut bgra = vec![0u8; usize::try_from(stride).ok()?.checked_mul(th as usize)?];
        let rect = WICRect {
            X: 0,
            Y: 0,
            Width: i32::try_from(tw).ok()?,
            Height: i32::try_from(th).ok()?,
        };
        converter.CopyPixels(&rect, stride, &mut bgra).ok()?;
        Some(Thumb { w: tw, h: th, bgra })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::files::tests::temp_dir;
    use crate::phone::thumb::tests::write_gradient_png;

    #[test]
    fn a_big_png_is_scaled_to_the_card_side() {
        let dir = temp_dir("shelf-thumb-card");
        let path = dir.join("foto.png");
        write_gradient_png(&path, 640, 480);
        let thumb = decode(&path, CARD_SIDE).expect("a thumb");
        assert_eq!((thumb.w, thumb.h), (192, 144));
        assert_eq!(thumb.bgra.len(), 192 * 144 * 4);
        assert!(thumb.bgra.chunks_exact(4).all(|p| p[3] == 255));
        // Channel order and orientation: the gradient is B = y, G = x, R = 255 - x,
        // top-down, so the top-left is red and the bottom-right is blue-green.
        let near = |got: u8, want: u8| got.abs_diff(want) <= 4;
        let px = |x: usize, y: usize| &thumb.bgra[(y * 192 + x) * 4..][..3];
        let tl = px(0, 0);
        assert!(near(tl[0], 0) && near(tl[1], 0) && near(tl[2], 255), "{tl:?}");
        let br = px(191, 143);
        assert!(near(br[0], 255) && near(br[1], 255) && near(br[2], 0), "{br:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_preview_side_scales_a_big_image_and_never_enlarges_a_small_one() {
        let dir = temp_dir("shelf-thumb-preview");
        let big = dir.join("big.png");
        write_gradient_png(&big, 2000, 1500);
        let thumb = decode(&big, PREVIEW_SIDE).expect("a thumb");
        assert_eq!((thumb.w, thumb.h), (1024, 768));
        let small = dir.join("small.png");
        write_gradient_png(&small, 50, 40);
        let thumb = decode(&small, PREVIEW_SIDE).expect("a thumb");
        assert_eq!((thumb.w, thumb.h), (50, 40));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_transparent_png_keeps_its_alpha_and_comes_out_premultiplied() {
        let dir = temp_dir("shelf-thumb-alpha");
        let path = dir.join("alpha.png");
        // Straight BGRA: fully transparent with a colour in it, opaque red,
        // and a half-transparent red.
        let bgra = [10, 200, 30, 0, 0, 0, 255, 255, 0, 0, 200, 128];
        let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let png = crate::screenshot::win::encode_png(3, 1, &bgra).expect("a png");
        if joined {
            unsafe { CoUninitialize() };
        }
        std::fs::write(&path, png).expect("write");
        let thumb = decode(&path, CARD_SIDE).expect("a thumb");
        assert_eq!((thumb.w, thumb.h), (3, 1));
        assert_eq!(&thumb.bgra[0..4], [0, 0, 0, 0], "transparent: zeroed colour");
        assert_eq!(&thumb.bgra[4..8], [0, 0, 255, 255], "opaque is unchanged");
        let half = &thumb.bgra[8..12];
        assert_eq!((half[0], half[1], half[3]), (0, 0, 128), "{half:?}");
        assert!(half[2].abs_diff(100) <= 2, "premultiplied red: {half:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_declared_size_over_the_pixel_cap_is_refused() {
        assert!(!too_many_pixels(16_000, 15_625)); // 250 M exactly
        assert!(too_many_pixels(16_000, 15_626));
        assert!(too_many_pixels(u32::MAX, u32::MAX));
        assert!(!too_many_pixels(640, 480));
    }

    #[test]
    fn a_file_that_is_not_an_image_is_none() {
        let dir = temp_dir("shelf-thumb-not");
        let txt = dir.join("a.png");
        std::fs::write(&txt, b"this is not a png").expect("write");
        assert!(decode(&txt, CARD_SIDE).is_none());
        let empty = dir.join("empty.png");
        std::fs::write(&empty, b"").expect("write");
        assert!(decode(&empty, CARD_SIDE).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_over_the_cap_is_none_without_reading_it() {
        let dir = temp_dir("shelf-thumb-cap");
        let path = dir.join("huge.png");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(crate::phone::thumb::MAX_BYTES + 1).expect("sparse");
        drop(file);
        assert!(decode(&path, CARD_SIDE).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_path_or_a_folder_is_none() {
        let dir = temp_dir("shelf-thumb-missing");
        assert!(decode(&dir.join("nope.png"), CARD_SIDE).is_none());
        assert!(decode(&dir, CARD_SIDE).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
