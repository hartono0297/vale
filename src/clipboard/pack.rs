//! Older images keep their original DIB PNG-compressed in memory (the
//! clipboard history's `Original::Packed`). The pixel array goes through WIC's
//! PNG codec; the bytes before it (header, masks, colour table) and after it
//! stay verbatim, so `unpack(pack(x)) == x` byte for byte. `pack` checks that
//! itself and gives `None` otherwise (the original then stays raw).
//!
//! Layouts: 32 bpp is encoded as BGRA, `w` x `h` (PNG is not premultiplied,
//! so colour under alpha 0 survives); 24 bpp as 8-bit gray of `stride` x `h`,
//! which carries the rows with their padding exactly. Needs COM on the calling
//! thread (WIC).

use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat8bppGray,
    GUID_WICPixelFormat32bppBGRA, IWICImagingFactory, WICBitmapEncoderNoCache,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, STREAM_SEEK_END, STREAM_SEEK_SET,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::GUID;

use super::image;
use super::store::{Packed, PngLayout};

/// What the PNG holds: its size, WIC pixel format and bytes per pixel.
fn shape(layout: PngLayout) -> (u32, u32, GUID, usize) {
    match layout {
        PngLayout::Bgra { w, h } => (w, h, GUID_WICPixelFormat32bppBGRA, 4),
        PngLayout::Gray { stride, h } => (stride, h, GUID_WICPixelFormat8bppGray, 1),
    }
}

/// `raw` (a packed DIB as `image::dib_info` reads it) with its pixels
/// compressed, or `None` when it is not one, WIC fails, or the result does
/// not decode back to exactly `raw`.
pub fn pack(raw: &[u8]) -> Option<Packed> {
    let info = image::dib_info(raw)?;
    let end = info.offset.checked_add(info.stride.checked_mul(info.h as usize)?)?;
    let pixels = raw.get(info.offset..end)?;
    let layout = match info.bpp {
        32 => PngLayout::Bgra {
            w: info.w,
            h: info.h,
        },
        24 => PngLayout::Gray {
            stride: u32::try_from(info.stride).ok()?,
            h: info.h,
        },
        _ => return None,
    };
    let packed = Packed {
        head: raw[..info.offset].to_vec(),
        png: encode(layout, pixels)?,
        tail: raw[end..].to_vec(),
        raw_len: raw.len(),
        layout,
    };
    (unpack(&packed)?.as_slice() == raw).then_some(packed)
}

/// The DIB `packed` was made from, or `None` when the PNG does not decode to
/// the layout it says.
pub fn unpack(packed: &Packed) -> Option<Vec<u8>> {
    let (w, h, format, bytes) = shape(packed.layout);
    let len = (w as usize).checked_mul(h as usize)?.checked_mul(bytes)?;
    let total = packed
        .head
        .len()
        .checked_add(len)?
        .checked_add(packed.tail.len())?;
    if total != packed.raw_len {
        return None;
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&packed.head);
    out.resize(packed.head.len() + len, 0);
    decode(
        &packed.png,
        (w, h, format),
        w * bytes as u32,
        &mut out[packed.head.len()..],
    )?;
    out.extend_from_slice(&packed.tail);
    Some(out)
}

fn encode(layout: PngLayout, pixels: &[u8]) -> Option<Vec<u8>> {
    let (w, h, format, bytes) = shape(layout);
    encode_png(w, h, format, w.checked_mul(bytes as u32)?, pixels)
}

/// A PNG of `pixels` (`h` rows of `stride` bytes, `w` pixels each) in WIC
/// pixel `format`; `None` when the size does not fit, WIC fails, or the codec
/// would convert the format.
pub(super) fn encode_png(
    w: u32,
    h: u32,
    format: GUID,
    stride: u32,
    pixels: &[u8],
) -> Option<Vec<u8>> {
    if pixels.len() != (stride as usize).checked_mul(h as usize)? {
        return None;
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
        frame.SetSize(w, h).ok()?;
        let mut chosen = format;
        frame.SetPixelFormat(&mut chosen).ok()?;
        if chosen != format {
            // The codec would convert: the bytes would not come back.
            return None;
        }
        frame.WritePixels(h, stride, pixels).ok()?;
        frame.Commit().ok()?;
        encoder.Commit().ok()?;
        let mut len = 0u64;
        stream.Seek(0, STREAM_SEEK_END, Some(&mut len)).ok()?;
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let len32 = u32::try_from(len).ok()?;
        let mut png = vec![0u8; len32 as usize];
        let mut read = 0u32;
        stream
            .Read(png.as_mut_ptr().cast(), len32, Some(&mut read))
            .ok()
            .ok()?;
        (read == len32).then_some(png)
    }
}

/// Decodes `png` (which must be `size` and in `format`, no conversion) into
/// `out` with row stride `stride`.
fn decode(png: &[u8], size: (u32, u32, GUID), stride: u32, out: &mut [u8]) -> Option<()> {
    let (w, h, format) = size;
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = wic.CreateStream().ok()?;
        stream.InitializeFromMemory(png).ok()?;
        let decoder = wic
            .CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut fw, mut fh) = (0u32, 0u32);
        frame.GetSize(&mut fw, &mut fh).ok()?;
        if (fw, fh) != (w, h) || frame.GetPixelFormat().ok()? != format {
            return None;
        }
        frame.CopyPixels(std::ptr::null(), stride, out).ok()?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    /// COM for this test thread.
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

    /// A deterministic, non-trivial byte for pixel position `i`.
    fn noise(i: usize) -> u8 {
        (i.wrapping_mul(2654435761) >> 11) as u8
    }

    fn header(size: u32, w: i32, h: i32, bpp: u16, compression: u32, colors: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(size.to_le_bytes());
        b.extend(w.to_le_bytes());
        b.extend(h.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(bpp.to_le_bytes());
        b.extend(compression.to_le_bytes());
        b.extend(0u32.to_le_bytes()); // image size
        b.extend([0u8; 8]); // pels per metre
        b.extend(colors.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b
    }

    const MASKS: [u8; 12] = [
        0, 0, 0xFF, 0, // red
        0, 0xFF, 0, 0, // green
        0xFF, 0, 0, 0, // blue
    ];

    fn pixels(len: usize) -> Vec<u8> {
        (0..len).map(noise).collect()
    }

    fn round_trip(dib: &[u8]) -> Packed {
        let packed = pack(dib).expect("packs");
        assert_eq!(packed.raw_len, dib.len());
        assert_eq!(unpack(&packed).as_deref(), Some(dib));
        packed
    }

    #[test]
    fn a_32_bit_bi_rgb_dib_comes_back_bottom_up_and_top_down() {
        let _com = Com::new();
        for h in [7, -7] {
            let mut dib = header(40, 13, h, 32, 0, 0);
            dib.extend(pixels(13 * 7 * 4));
            let packed = round_trip(&dib);
            assert_eq!(packed.layout, PngLayout::Bgra { w: 13, h: 7 });
            assert_eq!(packed.head.len(), 40);
            assert!(packed.tail.is_empty());
        }
    }

    #[test]
    fn a_flat_picture_shrinks() {
        let _com = Com::new();
        let mut dib = header(40, 256, 256, 32, 0, 0);
        dib.extend(std::iter::repeat_n([10u8, 20, 30, 255], 256 * 256).flatten());
        let packed = round_trip(&dib);
        assert!(packed.bytes() * 20 < dib.len(), "{}", packed.bytes());
    }

    #[test]
    fn a_32_bit_bitfields_dib_with_its_masks() {
        let _com = Com::new();
        let mut dib = header(40, 9, 5, 32, 3, 0);
        dib.extend(MASKS);
        dib.extend(pixels(9 * 5 * 4));
        let packed = round_trip(&dib);
        assert_eq!(packed.head.len(), 52, "header and masks verbatim");
    }

    #[test]
    fn a_v5_header_dib() {
        let _com = Com::new();
        let mut dib = header(124, 11, -6, 32, 3, 0);
        dib.extend(MASKS); // the masks sit at 40..52 of the header
        dib.extend(vec![0u8; 124 - dib.len()]);
        dib.extend(pixels(11 * 6 * 4));
        let packed = round_trip(&dib);
        assert_eq!(packed.head.len(), 124);
    }

    #[test]
    fn a_24_bit_dib_keeps_its_row_padding() {
        let _com = Com::new();
        // 13 pixels = 39 bytes a row, padded to 40.
        for h in [5, -5] {
            let mut dib = header(40, 13, h, 24, 0, 0);
            dib.extend(pixels(40 * 5));
            let packed = round_trip(&dib);
            assert_eq!(packed.layout, PngLayout::Gray { stride: 40, h: 5 });
        }
    }

    #[test]
    fn colour_under_alpha_zero_survives() {
        let _com = Com::new();
        let mut dib = header(40, 8, 8, 32, 0, 0);
        for i in 0..64usize {
            // Alpha 0 on every other pixel, with colour in the rest.
            let a = if i % 2 == 0 { 0 } else { noise(i + 1) };
            dib.extend([noise(i), noise(i + 100), noise(i + 200), a]);
        }
        round_trip(&dib);
    }

    #[test]
    fn bytes_after_the_pixels_are_kept() {
        let _com = Com::new();
        let mut dib = header(40, 4, 4, 32, 0, 0);
        dib.extend(pixels(4 * 4 * 4));
        dib.extend([1, 2, 3, 4, 5]);
        let packed = round_trip(&dib);
        assert_eq!(packed.tail, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_colour_table_is_kept_in_the_head() {
        let _com = Com::new();
        let mut dib = header(40, 4, 4, 24, 0, 3);
        dib.extend([9u8; 12]);
        dib.extend(pixels(12 * 4));
        let packed = round_trip(&dib);
        assert_eq!(packed.head.len(), 52);
    }

    #[test]
    fn garbage_gives_none() {
        let _com = Com::new();
        assert!(pack(b"not a bitmap at all, just bytes").is_none());
        assert!(pack(&[]).is_none());
        // A header that promises more pixels than there are.
        let mut short = header(40, 100, 100, 32, 0, 0);
        short.extend(pixels(64));
        assert!(pack(&short).is_none());
        // A packed value whose PNG is not one, or does not fit its layout.
        let mut dib = header(40, 4, 4, 32, 0, 0);
        dib.extend(pixels(64));
        let good = pack(&dib).unwrap();
        let junk = Packed {
            png: vec![1, 2, 3],
            ..good.clone()
        };
        assert!(unpack(&junk).is_none());
        let wrong = Packed {
            layout: PngLayout::Bgra { w: 2, h: 4 },
            ..good
        };
        assert!(unpack(&wrong).is_none());
    }
}
