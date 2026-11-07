//! The red recording dot on the tray icon: a pure compositor on a BGRA buffer
//! and the GDI glue that turns the logo into a dotted icon.

use std::mem::size_of;

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, DIB_RGB_COLORS, DeleteObject,
    GetDC, GetDIBits, GetObjectW, HGDIOBJ, ReleaseDC,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, GetIconInfo, HICON, ICONINFO,
};

/// The dot's 1 px ring (RGB): the island's darkest fill.
const RING: u32 = 0x17120F;
/// The dot's width as a share of the icon's.
const DOT_SHARE: f32 = 0.40;
/// Samples per pixel side for the edge.
const SAMPLES: usize = 4;

fn rgb_to_bgr_f(c: u32) -> [f32; 3] {
    [
        (c & 0xFF) as f32,
        ((c >> 8) & 0xFF) as f32,
        ((c >> 16) & 0xFF) as f32,
    ]
}

/// The dot's centre and radii for a `w` x `h` icon: `(cx, cy, r, outer)`.
fn dot_geometry(w: usize, h: usize) -> (f32, f32, f32, f32) {
    let side = w.min(h) as f32;
    let r = (side * DOT_SHARE / 2.0).max(1.5);
    let outer = r + 1.0;
    (w as f32 - outer, h as f32 - outer, r, outer)
}

/// Draws the recording dot onto a straight-alpha BGRA buffer of `w` x `h`:
/// `theme::REC_RED`, bottom right, about 40 % of the icon wide, inside a 1 px
/// dark ring so it reads on a light or a dark taskbar. A buffer that is not
/// `w * h * 4` bytes is left alone.
pub fn overlay_rec_dot(bgra: &mut [u8], w: usize, h: usize) {
    if w == 0 || h == 0 || bgra.len() != w * h * 4 {
        return;
    }
    let (cx, cy, r, outer) = dot_geometry(w, h);
    let dot = rgb_to_bgr_f(crate::theme::REC_RED);
    let ring = rgb_to_bgr_f(RING);
    let total = (SAMPLES * SAMPLES) as f32;
    for y in 0..h {
        for x in 0..w {
            let (mut in_dot, mut in_outer) = (0u32, 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    let d2 = (px - cx).powi(2) + (py - cy).powi(2);
                    in_dot += u32::from(d2 <= r * r);
                    in_outer += u32::from(d2 <= outer * outer);
                }
            }
            if in_outer == 0 {
                continue;
            }
            let c_dot = in_dot as f32 / total;
            let c_ring = in_outer as f32 / total - c_dot;
            let i = (y * w + x) * 4;
            let under_a = f32::from(bgra[i + 3]) / 255.0;
            // The ring over the icon, then the dot over that.
            let a1 = c_ring + under_a * (1.0 - c_ring);
            let mut mid = [0f32; 3];
            for k in 0..3 {
                if a1 > 0.0 {
                    mid[k] = (ring[k] * c_ring + f32::from(bgra[i + k]) * under_a * (1.0 - c_ring))
                        / a1;
                }
            }
            let a2 = c_dot + a1 * (1.0 - c_dot);
            for k in 0..3 {
                let v = if a2 > 0.0 {
                    (dot[k] * c_dot + mid[k] * a1 * (1.0 - c_dot)) / a2
                } else {
                    0.0
                };
                bgra[i + k] = v.round().clamp(0.0, 255.0) as u8;
            }
            bgra[i + 3] = (a2 * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// `logo` with the recording dot, as a new icon the caller destroys; `None`
/// when GDI refuses somewhere. The logo is left alone.
pub fn with_rec_dot(logo: HICON) -> Option<HICON> {
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(logo, &mut info).ok()?;
        let free = |info: &ICONINFO| {
            let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
            let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        };
        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(info.hbmColor.0),
            size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut BITMAP as *mut _),
        );
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if got == 0 || w <= 0 || h <= 0 {
            free(&info);
            return None;
        }
        let mut header = BITMAPINFO {
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
        let mut bits = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let read = GetDIBits(
            dc,
            info.hbmColor,
            0,
            h as u32,
            Some(bits.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        free(&info);
        if read == 0 {
            return None;
        }
        // An icon with no alpha at all would read as invisible: make it opaque.
        if bits.chunks_exact(4).all(|px| px[3] == 0) {
            bits.chunks_exact_mut(4).for_each(|px| px[3] = 255);
        }
        overlay_rec_dot(&mut bits, w as usize, h as usize);
        let color = CreateBitmap(w, h, 1, 32, Some(bits.as_ptr().cast()));
        // An all-zero mask: the colour bitmap's alpha decides.
        let mask_bits = vec![0u8; (((w + 15) / 16) * 2 * h) as usize];
        let mask = CreateBitmap(w, h, 1, 1, Some(mask_bits.as_ptr().cast()));
        let made = CreateIconIndirect(&ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        });
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        made.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(buf: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn the_dot_sits_bottom_right_in_rec_red_and_the_rest_is_untouched() {
        for size in [16usize, 20, 24, 32] {
            let mut buf = vec![0u8; size * size * 4];
            // A solid mid-grey logo, so what the dot leaves alone shows.
            buf.chunks_exact_mut(4)
                .for_each(|p| p.copy_from_slice(&[0x80, 0x80, 0x80, 0xFF]));
            overlay_rec_dot(&mut buf, size, size);
            let (cx, cy, r, outer) = dot_geometry(size, size);
            let (x, y) = (cx as usize, cy as usize);
            // The centre is REC_RED, opaque (BGRA: E5484D -> 4D, 48, E5).
            assert_eq!(px(&buf, size, x, y), [0x4D, 0x48, 0xE5, 0xFF], "{size}");
            // The dot is about 40 % of the icon wide.
            let width = 2.0 * r / size as f32;
            assert!((0.35..=0.5).contains(&width), "{size}: {width}");
            // The top-left corner is the logo's.
            assert_eq!(px(&buf, size, 0, 0), [0x80, 0x80, 0x80, 0xFF], "{size}");
            // A pixel just outside the ring is the logo's too.
            let out = (cx - outer - 1.5).max(0.0) as usize;
            assert_eq!(px(&buf, size, out, y), [0x80, 0x80, 0x80, 0xFF], "{size}");
        }
    }

    #[test]
    fn a_dark_one_pixel_ring_surrounds_the_dot() {
        let size = 32;
        let mut buf = vec![0u8; size * size * 4];
        buf.chunks_exact_mut(4)
            .for_each(|p| p.copy_from_slice(&[0xF0, 0xF0, 0xF0, 0xFF]));
        overlay_rec_dot(&mut buf, size, size);
        let (cx, cy, r, _) = dot_geometry(size, size);
        // Half a pixel past the dot's edge, on the row of its centre.
        let x = (cx - r - 0.5) as usize;
        let [b, g, rr, a] = px(&buf, size, x, cy as usize);
        assert_eq!(a, 0xFF);
        assert!(b < 0x80 && g < 0x80 && rr < 0x80, "{b} {g} {rr}");
    }

    #[test]
    fn on_a_transparent_icon_only_the_dot_and_its_ring_are_drawn() {
        let size = 16;
        let mut buf = vec![0u8; size * size * 4];
        overlay_rec_dot(&mut buf, size, size);
        assert_eq!(px(&buf, size, 0, 0), [0, 0, 0, 0]);
        let (cx, cy, ..) = dot_geometry(size, size);
        assert_eq!(px(&buf, size, cx as usize, cy as usize)[3], 0xFF);
        let drawn = buf.chunks_exact(4).filter(|p| p[3] != 0).count();
        assert!((20..=90).contains(&drawn), "{drawn} px at 16 px");
    }

    #[test]
    fn a_buffer_of_the_wrong_size_is_left_alone() {
        let mut buf = vec![7u8; 10];
        overlay_rec_dot(&mut buf, 16, 16);
        assert_eq!(buf, vec![7u8; 10]);
        overlay_rec_dot(&mut [], 0, 0);
    }
}
