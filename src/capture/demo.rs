//! Debug-only demos for the capture and the editor: a desktop drawn in code
//! (fake windows with inner panels) and fixed candidates. They never grab
//! the screen, list windows or ask UI Automation, and touch no user data.

use super::win::{Frozen, virtual_screen};
use super::{Candidate, Rect, Shot};

/// Fills `r` of a `w`-wide BGRA buffer with `rgb`.
fn fill(buf: &mut [u8], w: usize, r: Rect, rgb: u32) {
    let px = [rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8, 255];
    let rows = buf.len() / (w * 4);
    for y in r.y.max(0) as usize..(r.bottom().max(0) as usize).min(rows) {
        for x in r.x.max(0) as usize..(r.right().max(0) as usize).min(w) {
            buf[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&px);
        }
    }
}

/// A made-up page of `w` × `h`: a header, a sidebar of text-like lines and
/// cards, so annotations and the mosaic have something to sit on.
pub fn picture(w: u32, h: u32) -> Shot {
    let (wu, hu) = (w as usize, h as usize);
    let mut bgra = vec![0u8; wu * hu * 4];
    // A soft vertical gradient behind everything.
    for y in 0..hu {
        let t = y as f32 / hu.max(1) as f32;
        let v = (0xF1 as f32 - t * 18.0) as u32;
        fill(
            &mut bgra,
            wu,
            Rect::new(0, y as i32, w as i32, 1),
            v << 16 | (v - 2) << 8 | (v - 8),
        );
    }
    let mut r = |x: i32, y: i32, w: i32, h: i32, rgb: u32| {
        fill(&mut bgra, wu, Rect::new(x, y, w, h), rgb);
    };
    r(0, 0, w as i32, 56, 0x2B3A55);
    r(24, 18, 120, 20, 0x8FA3C7);
    // The sidebar: rows of fake text.
    r(0, 56, 260, h as i32 - 56, 0xE4E8F0);
    for i in 0..12 {
        let y = 80 + i * 32;
        r(24, y, 60 + (i * 37 % 120), 12, 0x7A869B);
        r(24, y + 16, 40 + (i * 53 % 150), 8, 0xA9B2C2);
    }
    // Cards.
    for (x, y, cw, ch, rgb) in [
        (300, 120, 260, 150, 0xFFFFFF),
        (600, 120, 300, 150, 0xFFFFFF),
        (300, 300, 260, 140, 0xFFFFFF),
        (600, 300, 300, 140, 0xFFF4E0),
    ] {
        r(x, y, cw, ch, rgb);
        r(x + 16, y + 18, cw / 2, 14, 0x3C4A66);
        for l in 0..4 {
            r(x + 16, y + 46 + l * 22, cw - 32 - l * 30, 8, 0xB7BFCD);
        }
    }
    r(300, 470, 600, 90, 0xDDE6F6);
    r(316, 490, 280, 12, 0x3C4A66);
    r(316, 514, 420, 8, 0x8794AB);
    Shot { w, h, bgra }
}

/// The desktop of `--demo-snap`: wallpaper, two overlapping fake windows
/// with panels in them, and the candidates that go with them (the front
/// window `z` 0, the one behind `z` 1).
pub fn scene() -> (Frozen, Vec<Candidate>) {
    let area = virtual_screen();
    let (w, h) = (area.w.max(640), area.h.max(480));
    let (wu, hu) = (w as usize, h as usize);
    let mut bgra = vec![0u8; wu * hu * 4];
    for y in 0..hu {
        let t = y as f32 / hu as f32;
        let (r, g, b) = (
            (0x2A as f32 + t * 40.0) as u32,
            (0x3F as f32 + t * 30.0) as u32,
            (0x6B as f32 + t * 50.0) as u32,
        );
        fill(
            &mut bgra,
            wu,
            Rect::new(0, y as i32, w, 1),
            r << 16 | g << 8 | b,
        );
    }
    let sx = |f: f32| (w as f32 * f) as i32;
    let sy = |f: f32| (h as f32 * f) as i32;
    let mut cands = Vec::new();
    // (frame, z): drawn back to front.
    for (frame, z) in [
        (Rect::new(sx(0.05), sy(0.08), sx(0.52), sy(0.62)), 1u32),
        (Rect::new(sx(0.36), sy(0.28), sx(0.55), sy(0.60)), 0u32),
    ] {
        fill(&mut bgra, wu, frame, 0xF6F4F0);
        let title = Rect::new(frame.x, frame.y, frame.w, 38);
        let side = Rect::new(frame.x, frame.y + 38, frame.w / 5, frame.h - 38);
        let body = Rect::new(
            frame.x + side.w,
            frame.y + 38,
            frame.w - side.w,
            frame.h - 38,
        );
        fill(&mut bgra, wu, title, 0x3A4357);
        fill(&mut bgra, wu, side, 0xE2E6EE);
        let card1 = Rect::new(body.x + 20, body.y + 20, body.w / 2 - 30, body.h / 2 - 30);
        let card2 = Rect::new(
            body.x + body.w / 2 + 10,
            body.y + 20,
            body.w / 2 - 30,
            body.h / 2 - 30,
        );
        let strip = Rect::new(
            body.x + 20,
            body.y + body.h / 2 + 10,
            body.w - 40,
            body.h / 2 - 30,
        );
        fill(&mut bgra, wu, card1, 0xFFFFFF);
        fill(&mut bgra, wu, card2, 0xFFE9C7);
        fill(&mut bgra, wu, strip, 0xD6E4FA);
        for c in [card1, card2] {
            fill(
                &mut bgra,
                wu,
                Rect::new(c.x + 14, c.y + 16, c.w / 2, 12),
                0x3C4A66,
            );
            fill(
                &mut bgra,
                wu,
                Rect::new(c.x + 14, c.y + 40, c.w - 28, 8),
                0xB7BFCD,
            );
        }
        cands.push(Candidate {
            rect: frame,
            z,
            depth: 0,
        });
        for (rect, depth) in [
            (title, 1),
            (side, 1),
            (body, 1),
            (card1, 2),
            (card2, 2),
            (strip, 2),
        ] {
            cands.push(Candidate { rect, z, depth });
        }
    }
    (
        Frozen {
            w: w as u32,
            h: h as u32,
            origin: (area.x, area.y),
            bgra,
        },
        // The candidates were made in window-local numbers; the frame sits
        // at the virtual screen's origin.
        cands
            .into_iter()
            .map(|mut c| {
                c.rect.x += area.x;
                c.rect.y += area.y;
                c
            })
            .collect(),
    )
}

/// `--demo-snap`: the overlay on `scene()`. A click opens the editor on the
/// crop; the process ends when either closes.
pub fn run_snap() {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, TranslateMessage,
    };

    crate::theme::set_system_light(crate::theme::read_system_light());
    // The editor it opens follows `DI_WINDOW`, `DI_SURFACE` and `DI_ACCENT`.
    crate::theme::set_current(crate::theme::theme_from_env());
    let (frame, cands) = scene();
    if !super::overlay::start_demo(frame, cands) {
        eprintln!("vale: the demo overlay could not start");
        return;
    }
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::pick;

    #[test]
    fn the_picture_is_opaque_and_full_size() {
        let s = picture(320, 200);
        assert_eq!(s.bgra.len(), 320 * 200 * 4);
        assert!(s.bgra.chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn the_demo_scene_snaps_front_window_first_and_has_no_real_screen() {
        let (frame, cands) = scene();
        assert_eq!(frame.bgra.len(), frame.w as usize * frame.h as usize * 4);
        let o = frame.origin;
        // The middle of the front window's first card is a card.
        let front = cands
            .iter()
            .find(|c| c.z == 0 && c.depth == 0)
            .expect("front window");
        let c = front.rect;
        let inside = (c.x + c.w * 3 / 5, c.y + c.h / 4);
        let picked = pick(&cands, inside).expect("something");
        assert!(picked.w < c.w && picked.h < c.h, "a panel, not the window");
        // The corner of the front window is its title bar, still of the
        // front window and not of the one behind it.
        let corner = pick(&cands, (c.x + 2, c.y + 2)).expect("the title bar");
        assert_eq!((corner.w, corner.h), (c.w, 38));
        assert!(c.x >= o.0 && c.y >= o.1, "in virtual-screen numbers");
    }
}
