//! The thumbnail of a saved video: one frame decoded from the file by the
//! Media Foundation source reader and scaled down. It needs nothing from
//! the recording's GPU pipeline, so it runs after the file is closed.

use std::path::Path;

use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFAttributes, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSourceReader,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MFMediaType_Video,
    MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::core::{GUID, HSTRING, Interface, PCWSTR};

use crate::clipboard::image::Thumb;
use crate::mf::{MfSession, created};

/// The box a thumbnail fits in, in pixels.
pub const THUMB_MAX: u32 = 120;

/// Reads allowed to bring no picture (stream ticks, a format change) before
/// `grab` gives up.
const EMPTY_READS: usize = 16;

/// Where in a video of `duration_ms` the thumbnail is taken: one second in,
/// or the middle of a shorter one.
pub fn thumb_at(duration_ms: u64) -> u64 {
    (duration_ms / 2).min(1_000)
}

/// `w` x `h` scaled down to fit `max_w` x `max_h`: aspect kept, rounded, at
/// least 1 a side, never larger than the source.
pub fn fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let (w, h) = (w.max(1), h.max(1));
    let (max_w, max_h) = (max_w.max(1), max_h.max(1));
    if w <= max_w && h <= max_h {
        return (w, h);
    }
    // `a * b / c`, rounded, at least 1.
    let scaled = |a: u32, b: u32, c: u32| {
        let (a, b, c) = (u64::from(a), u64::from(b), u64::from(c));
        ((a * b + c / 2) / c).max(1) as u32
    };
    // The side that overshoots its limit by more decides the factor.
    if u64::from(w) * u64::from(max_h) >= u64::from(h) * u64::from(max_w) {
        (max_w, scaled(h, max_w, w).min(max_h))
    } else {
        (scaled(w, max_h, h).min(max_w), max_h)
    }
}

/// A top-down BGRA picture of `w` x `h` (rows `stride` bytes apart) scaled
/// to `out_w` x `out_h`: each pixel the average of the source pixels it
/// covers, alpha set to 255. `bgra` holds at least `(h - 1) * stride + w * 4`
/// bytes.
pub fn scale(bgra: &[u8], w: u32, h: u32, stride: usize, out_w: u32, out_h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let (out_w, out_h) = (out_w as usize, out_h as usize);
    // The source pixels that output pixel `i` of `n` covers along one axis.
    let span = |i: usize, n: usize, len: usize| {
        let from = i * len / n;
        let to = ((i + 1) * len).div_ceil(n).clamp(from + 1, len);
        from..to
    };
    let mut out = Vec::with_capacity(out_w * out_h * 4);
    for y in 0..out_h {
        let rows = span(y, out_h, h);
        for x in 0..out_w {
            let columns = span(x, out_w, w);
            let mut sum = [0u32; 3];
            for row in rows.clone() {
                let line = &bgra[row * stride..];
                for pixel in line[columns.start * 4..columns.end * 4].chunks_exact(4) {
                    for (total, &value) in sum.iter_mut().zip(pixel) {
                        *total += u32::from(value);
                    }
                }
            }
            let count = (rows.len() * columns.len()) as u32;
            out.extend(sum.map(|total| (total / count) as u8));
            out.push(255);
        }
    }
    out
}

/// The frame of `video` at `at_ms`, fitted into `THUMB_MAX` x `THUMB_MAX`.
/// `None` on any failure (no Media Foundation, no video stream, no decoder,
/// a file that cannot be read).
pub fn grab(video: &Path, at_ms: u64) -> Option<Thumb> {
    // Declared first, so dropped last: after every interface below.
    let mf = MfSession::start()?;
    let api = mf.api();
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    unsafe {
        // The reader converts the decoder's YUV to RGB32 itself.
        let attributes: IMFAttributes = created(|out| (api.create_attributes)(out, 1)).ok()?;
        attributes
            .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
            .ok()?;
        let url = HSTRING::from(video.as_os_str());
        let reader: IMFSourceReader = created(|out| {
            (api.create_source_reader)(PCWSTR(url.as_ptr()), attributes.as_raw(), out)
        })
        .ok()?;
        reader
            .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
            .ok()?;
        reader.SetStreamSelection(stream, true).ok()?;
        let wanted: IMFMediaType = created(|out| (api.create_media_type)(out)).ok()?;
        wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
        wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok()?;
        reader.SetCurrentMediaType(stream, None, &wanted).ok()?;
        // In 100 ns units; the reader lands on the key frame before it.
        let position = PROPVARIANT::from(i64::try_from(at_ms).ok()?.checked_mul(10_000)?);
        reader.SetCurrentPosition(&GUID::zeroed(), &position).ok()?;

        let sample = first_sample(&reader, stream)?;
        // Read after the sample: the decoder may only now know its size.
        let current = reader.GetCurrentMediaType(stream).ok()?;
        let size = current.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
        let (w, h) = ((size >> 32) as u32, size as u32);
        if w == 0 || h == 0 {
            return None;
        }
        // Negative for a bottom-up picture, which RGB32 is when the type
        // does not say.
        let stride = current
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map_or(-(w as i32 * 4), |stride| stride as i32);
        let buffer = sample.ConvertToContiguousBuffer().ok()?;
        let frame = top_down(&buffer, w, h, stride)?;
        let (out_w, out_h) = fit(w, h, THUMB_MAX, THUMB_MAX);
        Some(Thumb {
            w: out_w,
            h: out_h,
            bgra: scale(&frame, w, h, w as usize * 4, out_w, out_h),
        })
    }
}

/// The first picture the reader hands out from its position.
unsafe fn first_sample(reader: &IMFSourceReader, stream: u32) -> Option<IMFSample> {
    for _ in 0..EMPTY_READS {
        let mut flags = 0u32;
        let mut sample = None;
        unsafe {
            reader
                .ReadSample(
                    stream,
                    0,
                    None,
                    Some(&raw mut flags),
                    None,
                    Some(&raw mut sample),
                )
                .ok()?;
        }
        if sample.is_some() {
            return sample;
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            return None;
        }
    }
    None
}

/// The picture in `buffer` as tight top-down rows of `w * 4` bytes.
/// `default_stride` is the media type's; a 2D buffer knows its own pitch
/// (the decoder's rows can be wider than the picture) and that one wins.
unsafe fn top_down(
    buffer: &IMFMediaBuffer,
    w: u32,
    h: u32,
    default_stride: i32,
) -> Option<Vec<u8>> {
    let (row_len, rows) = (w as usize * 4, h as usize);
    let mut frame = Vec::with_capacity(row_len * rows);
    unsafe {
        if let Ok(buffer_2d) = buffer.cast::<IMF2DBuffer>() {
            let mut scan0 = std::ptr::null_mut();
            let mut pitch = 0i32;
            buffer_2d.Lock2D(&mut scan0, &mut pitch).ok()?;
            // `scan0` is the top row whatever the sign of the pitch.
            if !scan0.is_null() && pitch.unsigned_abs() as usize >= row_len {
                for y in 0..rows {
                    let row = scan0.offset(y as isize * pitch as isize);
                    frame.extend_from_slice(std::slice::from_raw_parts(row, row_len));
                }
            }
            let _ = buffer_2d.Unlock2D();
        } else {
            let mut data = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&mut data, None, Some(&raw mut len)).ok()?;
            let bytes = std::slice::from_raw_parts(data, len as usize);
            let stride = default_stride.unsigned_abs() as usize;
            for y in 0..rows {
                // Bottom-up: the last row in memory is the top of the picture.
                let row = if default_stride < 0 { rows - 1 - y } else { y };
                match bytes.get(row * stride..row * stride + row_len) {
                    Some(line) => frame.extend_from_slice(line),
                    None => break,
                }
            }
            let _ = buffer.Unlock();
        }
    }
    (frame.len() == row_len * rows).then_some(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_time_is_one_second_or_the_middle() {
        assert_eq!(thumb_at(60_000), 1_000);
        assert_eq!(thumb_at(1_200), 600);
        assert_eq!(thumb_at(0), 0);
    }

    #[test]
    fn fit_keeps_the_aspect_and_never_grows() {
        assert_eq!(fit(1920, 1080, 120, 120), (120, 68));
        assert_eq!(fit(1080, 1920, 120, 120), (68, 120));
        assert_eq!(fit(64, 48, 120, 120), (64, 48));
        assert_eq!(fit(4000, 2, 120, 120), (120, 1));
    }

    #[test]
    fn scale_makes_an_opaque_picture_of_that_size() {
        // 4 x 2, rows 20 bytes apart (4 bytes of padding each). The left
        // 2 x 2 block is 0 and 200 in a checker, the right one all 40.
        let px = |v: u8| [v, v, v, 0];
        let mut src = Vec::new();
        for row in [[0, 200, 40, 40], [200, 0, 40, 40]] {
            for v in row {
                src.extend(px(v));
            }
            src.extend([9, 9, 9, 9]);
        }
        let out = scale(&src, 4, 2, 20, 2, 1);
        assert_eq!(out.len(), 8);
        assert!(out.chunks_exact(4).all(|p| p[3] == 255));
        assert_eq!(out, [100, 100, 100, 255, 40, 40, 40, 255]);
    }

    /// `DI_THUMB_VIDEO=<an .mp4>`, for example one `--screenrec-test` wrote.
    /// Leaves the picture in `target/test-tmp/screenrec-thumb/thumb.bmp`.
    #[test]
    #[ignore]
    fn grabs_a_frame_from_a_real_video() {
        let video = std::env::var_os("DI_THUMB_VIDEO").expect("set DI_THUMB_VIDEO");
        let thumb = grab(Path::new(&video), thumb_at(3_000)).expect("a frame");
        assert!(
            thumb.w == THUMB_MAX || thumb.h == THUMB_MAX,
            "{} x {}",
            thumb.w,
            thumb.h
        );
        assert_eq!(thumb.bgra.len(), (thumb.w * thumb.h * 4) as usize);
        assert!(thumb.bgra.chunks_exact(4).all(|p| p[3] == 255));
        // For a look by eye: the right way up, the colours right.
        let dir = Path::new("target/test-tmp/screenrec-thumb");
        std::fs::create_dir_all(dir).unwrap();
        let dib = crate::clipboard::image::dib_from_thumb(&thumb);
        let mut bmp = b"BM".to_vec();
        bmp.extend((14 + dib.len() as u32).to_le_bytes());
        bmp.extend([0; 4]);
        bmp.extend(54u32.to_le_bytes());
        bmp.extend(dib);
        std::fs::write(dir.join("thumb.bmp"), bmp).unwrap();
    }
}
