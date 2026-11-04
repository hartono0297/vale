//! The decoration of a sticky note: the tilt, the faint ruling behind the
//! text, the tape and the pin, the stamp, the confetti and the small alarm
//! clock beside the cat. The geometry and the colours are pure and tested
//! without a window; the `draw_*` functions turn them into Direct2D calls on
//! the note's canvas (`paint::draw` calls them, in dip).

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::i18n::{Lang, text};
use crate::shot_edit::gfx::color;
use crate::ui::geometry::Rect;
use crate::ui::icons::Icon;
use crate::widgets::paint::{CAT_SIZE, FOOTER_H, HEADER_H, PAD_X, Res};
use crate::widgets::sticky::{Deco, Pattern, Stamp, TextSize, Tones, blend};

// ------------------------------------------------------------------ the tilt

/// The most a note leans, in degrees either way.
pub const MAX_TILT: f32 = 2.0;

/// The fixed angle a note leans when "Miring sedikit" is on: a stable hash of
/// its id spread over -2 to +2 degrees (FNV-1a), so a note leans the same way
/// every time and its neighbours differ.
pub fn tilt_deg(id: &str) -> f32 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    // A finaliser (splitmix64) so ids that differ in their last digit still
    // spread over the whole range.
    h ^= h >> 30;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^= h >> 31;
    let unit = ((h >> 40) % 10_000) as f32 / 9_999.0;
    -MAX_TILT + 2.0 * MAX_TILT * unit
}

/// `(x, y)` turned `deg` degrees clockwise about `(cx, cy)`: where a point of
/// a note lands once it is drawn leaning.
pub fn rotate_about(x: f32, y: f32, deg: f32, cx: f32, cy: f32) -> (f32, f32) {
    let (s, c) = deg.to_radians().sin_cos();
    let (dx, dy) = (x - cx, y - cy);
    (cx + dx * c - dy * s, cy + dx * s + dy * c)
}

/// The point of a note `w` x `h` (dip from its top-left, as drawn leaning by
/// `deg`) mapped back through the inverse rotation to where it lies on the
/// straight note: what hit-testing asks `paint::target`.
pub fn unrotate(x: f32, y: f32, w: f32, h: f32, deg: f32) -> (f32, f32) {
    if deg == 0.0 {
        return (x, y);
    }
    rotate_about(x, y, -deg, w / 2.0, h / 2.0)
}

/// How far a `w` x `h` note leaning `deg` degrees reaches past its straight
/// edges: sideways and up or down, in dip.
pub fn overhang(w: f32, h: f32, deg: f32) -> (f32, f32) {
    let (s, c) = deg.abs().to_radians().sin_cos();
    (
        ((w * c + h * s - w) / 2.0).max(0.0),
        ((w * s + h * c - h) / 2.0).max(0.0),
    )
}

/// The window's margin (left, top, right, bottom, dip) round a note `w` x `h`
/// that leans `deg` degrees: the usual margin, grown by the overhang (rounded
/// up) so a leaning corner is never clipped.
pub fn margins_for(w: f32, h: f32, deg: f32) -> (f32, f32, f32, f32) {
    let (l, t, r, b) = crate::widgets::paint::margins();
    let (ox, oy) = overhang(w, h, deg);
    let (ox, oy) = (ox.ceil(), oy.ceil());
    (l + ox, t + oy, r + ox, b + oy)
}

/// The transform that draws the note leaning `deg` about its centre
/// `(cx, cy)` and then scales dip to pixels by `k`.
pub fn tilt_transform(deg: f32, cx: f32, cy: f32, k: f32) -> Matrix3x2 {
    let (s, c) = deg.to_radians().sin_cos();
    Matrix3x2 {
        M11: c * k,
        M12: s * k,
        M21: -s * k,
        M22: c * k,
        M31: (cx - cx * c + cy * s) * k,
        M32: (cy - cx * s - cy * c) * k,
    }
}

// ------------------------------------------------------------------ the paper

/// The pitch of a Kotak grid and the dots of Titik, dip.
pub const GRID_PITCH: f32 = 12.0;

/// Where the horizontal rules of Garis sit under the text lines: the rule is
/// this far above the bottom of a text line's pitch.
const RULE_INSET: f32 = 4.0;

/// The ruling's offsets (dip from the text area's top-left) that show in an
/// area `len` long when the text is scrolled by `scroll`: one every `pitch`,
/// the first at `first` into the unscrolled text.
pub fn ruling(first: f32, pitch: f32, scroll: f32, len: f32) -> Vec<f32> {
    if pitch <= 0.0 {
        return Vec::new();
    }
    let mut at = (first - scroll).rem_euclid(pitch);
    let mut out = Vec::new();
    while at < len {
        out.push(at);
        at += pitch;
    }
    out
}

/// Draws the ruling of `pattern` in the text area at (`ax`, `ay`), `aw` x
/// `ah` dip, scrolled by `scroll` (the caller clips to the area).
#[allow(clippy::too_many_arguments)]
pub fn draw_pattern(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    pattern: Pattern,
    size: TextSize,
    (ax, ay, aw, ah): (f32, f32, f32, f32),
    scroll: f32,
    ink: u32,
) {
    if pattern == Pattern::Polos {
        return;
    }
    unsafe { brush.SetColor(&color(ink, 1.0)) };
    let rows = |pitch: f32, first: f32| ruling(first, pitch, scroll, ah);
    match pattern {
        Pattern::Polos => {}
        Pattern::Garis => {
            let pitch = size.line() + 1.0;
            for y in rows(pitch, pitch - RULE_INSET) {
                unsafe {
                    rt.DrawLine(
                        Vector2::new(ax, ay + y),
                        Vector2::new(ax + aw, ay + y),
                        brush,
                        1.0,
                        None,
                    );
                }
            }
        }
        Pattern::Kotak => {
            for y in rows(GRID_PITCH, 0.0) {
                unsafe {
                    rt.DrawLine(
                        Vector2::new(ax, ay + y),
                        Vector2::new(ax + aw, ay + y),
                        brush,
                        1.0,
                        None,
                    );
                }
            }
            let mut x = 0.0;
            while x <= aw {
                unsafe {
                    rt.DrawLine(
                        Vector2::new(ax + x, ay),
                        Vector2::new(ax + x, ay + ah),
                        brush,
                        1.0,
                        None,
                    );
                }
                x += GRID_PITCH;
            }
        }
        Pattern::Titik => {
            for y in rows(GRID_PITCH, 0.0) {
                let mut x = 0.0;
                while x <= aw {
                    unsafe {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE {
                                point: Vector2::new(ax + x, ay + y),
                                radiusX: 1.0,
                                radiusY: 1.0,
                            },
                            brush,
                        );
                    }
                    x += GRID_PITCH;
                }
            }
        }
    }
}

// ------------------------------------------------------------------ tape and pin

/// The tape's width and height, and the pin's radius, dip.
pub const TAPE_W: f32 = 58.0;
pub const TAPE_H: f32 = 16.0;
pub const PIN_R: f32 = 6.5;

/// The cat's left edge on a note `w` wide (`paint::draw` puts it there).
pub fn cat_left(w: f32) -> f32 {
    (w - 28.0 - CAT_SIZE).max(8.0)
}

/// The tape's or pin's horizontal centre on a note `w` wide: the top centre,
/// moved left on a narrow note so it never reaches the cat's face (the cat
/// may come to any note, so the room is kept whether it is there or not).
pub fn deco_centre(w: f32, half: f32) -> f32 {
    let cat = cat_left(w);
    let centre = w / 2.0;
    if centre + half + 4.0 <= cat {
        return centre;
    }
    (cat - 6.0 - half).max(half + 10.0)
}

/// The tape's stripe colours: the paper's accent and a light one.
pub fn tape_colours(t: &Tones) -> (u32, u32) {
    (t.accent, blend(0xFFFFFF, t.fill, 0.6))
}

/// Draws the tape or the pin over the note's top edge at (`ox`, `oy`).
pub fn draw_deco(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    deco: Deco,
    t: &Tones,
    (ox, oy, w): (f32, f32, f32),
) {
    match deco {
        Deco::None => {}
        Deco::Selotip => {
            let cx = ox + deco_centre(w, TAPE_W / 2.0);
            let (a, b) = tape_colours(t);
            // A slight lean, about the tape's own centre: stripes are plain
            // rectangles, so no clip is needed.
            let centre = (cx, oy);
            let (s, c) = (-3.0f32).to_radians().sin_cos();
            let save = unsafe {
                let mut m = Matrix3x2::identity();
                rt.GetTransform(&mut m);
                m
            };
            let lean = Matrix3x2 {
                M11: c,
                M12: s,
                M21: -s,
                M22: c,
                M31: centre.0 - centre.0 * c + centre.1 * s,
                M32: centre.1 - centre.0 * s - centre.1 * c,
            };
            unsafe { rt.SetTransform(&(lean * save)) };
            let left = cx - TAPE_W / 2.0;
            let top = oy - TAPE_H / 2.0;
            // A shadow under the strip, then its stripes.
            unsafe {
                brush.SetColor(&color(0x000000, 0.14));
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left,
                        top: top + 1.5,
                        right: left + TAPE_W,
                        bottom: top + TAPE_H + 1.5,
                    },
                    brush,
                );
            }
            let n = (TAPE_W / 6.0) as usize;
            for i in 0..n {
                let x = left + i as f32 * 6.0;
                unsafe {
                    brush.SetColor(&color(if i % 2 == 0 { a } else { b }, 0.82));
                    rt.FillRectangle(
                        &D2D_RECT_F {
                            left: x,
                            top,
                            right: x + 6.0,
                            bottom: top + TAPE_H,
                        },
                        brush,
                    );
                }
            }
            unsafe { rt.SetTransform(&save) };
        }
        Deco::Paku => {
            let cx = ox + deco_centre(w, PIN_R);
            let cy = oy + 1.0;
            let ell = |x: f32, y: f32, r: f32| D2D1_ELLIPSE {
                point: Vector2::new(x, y),
                radiusX: r,
                radiusY: r,
            };
            let dark = blend(0x000000, t.accent, 0.3);
            unsafe {
                brush.SetColor(&color(0x000000, 0.25));
                rt.FillEllipse(&ell(cx + 1.0, cy + 2.2, PIN_R), brush);
                brush.SetColor(&color(dark, 1.0));
                rt.FillEllipse(&ell(cx, cy, PIN_R), brush);
                brush.SetColor(&color(t.accent, 1.0));
                rt.FillEllipse(&ell(cx - 0.8, cy - 0.8, PIN_R - 1.4), brush);
                brush.SetColor(&color(0xFFFFFF, 0.55));
                rt.FillEllipse(&ell(cx - 2.2, cy - 2.4, 1.7), brush);
            }
        }
    }
}

// ------------------------------------------------------------------ the stamp

/// The stamp's radius and its lean, dip and degrees.
pub const STAMP_R: f32 = 21.0;
pub const STAMP_TILT: f32 = -12.0;

/// The stamp's centre on a note `w` x `h`: the bottom right of the text area,
/// above the footer and clear of the resize grip.
pub fn stamp_centre(w: f32, h: f32) -> (f32, f32) {
    (w - PAD_X - STAMP_R - 2.0, h - FOOTER_H - STAMP_R - 4.0)
}

/// The stamp's ink: the accent's dark shade on a pastel (the paper's own
/// accent, darkened toward the ink), the island's accent on Gelap. At least
/// 3:1 on the fill (a test checks every paper, surface and accent).
pub fn stamp_ink(t: &Tones) -> u32 {
    match t.strip {
        Some(_) => t.accent,
        None => blend(crate::widgets::sticky::INK, t.accent, 0.4),
    }
}

/// The glyph of a stamp: a line icon.
pub fn stamp_icon(s: Stamp) -> Option<Icon> {
    match s {
        Stamp::None => None,
        Stamp::Penting => Some(Icon::Star),
        Stamp::Beres => Some(Icon::Tick),
        Stamp::Mendesak => Some(Icon::Mark),
        Stamp::Suka => Some(Icon::Heart),
        Stamp::Nanti => Some(Icon::Cup),
        Stamp::Ide => Some(Icon::Sparkle),
    }
}

/// Draws the stamp: two rings, the icon and the label, leaning.
pub fn draw_stamp(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    res: &Res,
    stamp: Stamp,
    lang: Lang,
    t: &Tones,
    (ox, oy, w, h): (f32, f32, f32, f32),
) {
    let Some(icon) = stamp_icon(stamp) else { return };
    let (cx, cy) = stamp_centre(w, h);
    let (cx, cy) = (ox + cx, oy + cy);
    let ink = stamp_ink(t);
    let save = unsafe {
        let mut m = Matrix3x2::identity();
        rt.GetTransform(&mut m);
        m
    };
    let (s, c) = STAMP_TILT.to_radians().sin_cos();
    let lean = Matrix3x2 {
        M11: c,
        M12: s,
        M21: -s,
        M22: c,
        M31: cx - cx * c + cy * s,
        M32: cy - cx * s - cy * c,
    };
    unsafe { rt.SetTransform(&(lean * save)) };
    let ring = |r: f32| D2D1_ELLIPSE {
        point: Vector2::new(cx, cy),
        radiusX: r,
        radiusY: r,
    };
    unsafe {
        brush.SetColor(&color(ink, 0.9));
        rt.DrawEllipse(&ring(STAMP_R - 1.0), brush, 2.0, None);
        brush.SetColor(&color(ink, 0.6));
        rt.DrawEllipse(&ring(STAMP_R - 4.5), brush, 0.8, None);
    }
    let _ = res.icons.draw(
        rt,
        brush,
        icon,
        Rect { x: cx - 8.0, y: cy - 15.0, w: 16.0, h: 16.0 },
        13.0,
        ink,
        0.95,
        None,
    );
    let label = text(lang, stamp.label_key()).to_uppercase();
    if let Ok(l) = res.layout(&label, 6.5, 700, STAMP_R * 2.0 - 8.0) {
        unsafe {
            let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = l.SetMaxHeight(10.0);
            brush.SetColor(&color(ink, 0.95));
            rt.DrawTextLayout(
                Vector2::new(cx - (STAMP_R - 4.0), cy + 3.0),
                &l,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }
    unsafe { rt.SetTransform(&save) };
}

// ------------------------------------------------------------------ progress

/// The progress bar's height and where it sits: right above the text area, at
/// the bottom of the strip, `PAD_X` in from each side.
pub const PROGRESS_H: f32 = 2.0;

pub fn progress_rect(w: f32) -> Rect {
    Rect {
        x: PAD_X,
        y: HEADER_H - PROGRESS_H - 1.0,
        w: (w - 2.0 * PAD_X).max(0.0),
        h: PROGRESS_H,
    }
}

/// How much of the bar is full, 0 to 1.
pub fn progress_fraction(done: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        (done as f32 / total as f32).clamp(0.0, 1.0)
    }
}

// ------------------------------------------------------------------ confetti

/// How long the burst lasts, ms.
pub const CONFETTI_MS: f32 = 800.0;
/// How many pieces.
const PIECES: usize = 40;

/// One piece of paper: its centre and size (dip, in the window), its lean,
/// which of the palette it is and how opaque.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rot: f32,
    pub colour: usize,
    pub alpha: f32,
}

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((v >> 40) as f32) / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

/// The pieces of the burst `t_ms` after it began, in a window `w` x `h` dip.
/// They fly up from the middle of the note and fall; each stays inside the
/// window (a piece that reaches its edge rests on it) and the list is empty
/// from `CONFETTI_MS` on. The same `seed` gives the same burst.
pub fn confetti(seed: u64, t_ms: f32, w: f32, h: f32) -> Vec<Piece> {
    if !(0.0..CONFETTI_MS).contains(&t_ms) || w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    let mut rng = Rng(seed ^ 0x9E37_79B9_7F4A_7C15 | 1);
    let t = t_ms / 1000.0;
    let fade = 1.0 - (t_ms / CONFETTI_MS).powi(3);
    (0..PIECES)
        .map(|i| {
            let angle = rng.range(-2.7, -0.45); // radians, mostly upward
            let speed = rng.range(140.0, 380.0);
            let (vx, vy) = (angle.cos() * speed, angle.sin() * speed);
            let (pw, ph) = (rng.range(4.0, 6.5), rng.range(6.0, 10.0));
            let spin = rng.range(-540.0, 540.0);
            let start = rng.range(0.0, 360.0);
            // The burst's source: the middle of the note, spread a little.
            let (x0, y0) = (w / 2.0 + rng.range(-24.0, 24.0), h * 0.55 + rng.range(-8.0, 8.0));
            let x = x0 + vx * t;
            let y = y0 + vy * t + 0.5 * 620.0 * t * t;
            // Half the piece's diagonal keeps it wholly inside.
            let r = (pw * pw + ph * ph).sqrt() / 2.0;
            Piece {
                x: x.clamp(r, (w - r).max(r)),
                y: y.clamp(r, (h - r).max(r)),
                w: pw,
                h: ph,
                rot: start + spin * t,
                colour: i % 4,
                alpha: fade.clamp(0.0, 1.0),
            }
        })
        .collect()
}

/// The palette the pieces take their `colour` from, for a note's tones: the
/// accent, the paper's own accent lightened and darkened, and a warm
/// contrast, all opaque.
pub fn confetti_palette(t: &Tones) -> [u32; 4] {
    [
        t.accent,
        blend(0xFFFFFF, t.accent, 0.45),
        blend(crate::widgets::sticky::INK, t.fill, 0.55),
        blend(0xE0A53C, t.fill, 0.9),
    ]
}

/// Draws the burst, in window dip.
pub fn draw_confetti(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    pieces: &[Piece],
    palette: &[u32; 4],
) {
    let save = unsafe {
        let mut m = Matrix3x2::identity();
        rt.GetTransform(&mut m);
        m
    };
    for p in pieces {
        let (s, c) = p.rot.to_radians().sin_cos();
        let turn = Matrix3x2 {
            M11: c,
            M12: s,
            M21: -s,
            M22: c,
            M31: p.x - p.x * c + p.y * s,
            M32: p.y - p.x * s - p.y * c,
        };
        unsafe {
            rt.SetTransform(&(turn * save));
            brush.SetColor(&color(palette[p.colour % palette.len()], p.alpha));
            rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: p.x - p.w / 2.0,
                        top: p.y - p.h / 2.0,
                        right: p.x + p.w / 2.0,
                        bottom: p.y + p.h / 2.0,
                    },
                    radiusX: 1.0,
                    radiusY: 1.0,
                },
                brush,
            );
        }
    }
    unsafe { rt.SetTransform(&save) };
}

// ------------------------------------------------------------------ the clock

/// The small alarm clock beside the cat while a reminder is overdue: its
/// centre and radius, on a note `w` wide (in the top margin, left of the cat).
pub fn clock_centre(w: f32) -> (f32, f32) {
    (cat_left(w) - 12.0, -10.0)
}

pub const CLOCK_R: f32 = 7.0;

/// Draws the alarm clock about (`cx`, `cy`) in window dip: a face, two bells
/// on top, two feet and the hands.
pub fn draw_clock(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    (cx, cy): (f32, f32),
    face: u32,
    ink: u32,
    bell: u32,
) {
    let ell = |x: f32, y: f32, rx: f32, ry: f32| D2D1_ELLIPSE {
        point: Vector2::new(x, y),
        radiusX: rx,
        radiusY: ry,
    };
    unsafe {
        // Bells, feet.
        brush.SetColor(&color(bell, 1.0));
        rt.FillEllipse(&ell(cx - 5.2, cy - CLOCK_R + 0.4, 2.6, 2.0), brush);
        rt.FillEllipse(&ell(cx + 5.2, cy - CLOCK_R + 0.4, 2.6, 2.0), brush);
        brush.SetColor(&color(ink, 1.0));
        rt.DrawLine(
            Vector2::new(cx - 4.0, cy + CLOCK_R - 0.6),
            Vector2::new(cx - 5.6, cy + CLOCK_R + 2.0),
            brush,
            1.3,
            None,
        );
        rt.DrawLine(
            Vector2::new(cx + 4.0, cy + CLOCK_R - 0.6),
            Vector2::new(cx + 5.6, cy + CLOCK_R + 2.0),
            brush,
            1.3,
            None,
        );
        // Face and rim.
        brush.SetColor(&color(face, 1.0));
        rt.FillEllipse(&ell(cx, cy, CLOCK_R, CLOCK_R), brush);
        brush.SetColor(&color(ink, 1.0));
        rt.DrawEllipse(&ell(cx, cy, CLOCK_R, CLOCK_R), brush, 1.3, None);
        // Hands: ten past two.
        rt.DrawLine(Vector2::new(cx, cy), Vector2::new(cx, cy - 4.2), brush, 1.2, None);
        rt.DrawLine(Vector2::new(cx, cy), Vector2::new(cx + 3.0, cy + 1.2), brush, 1.2, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::paint::{GRIP, Target, body_rect, button_rect, grip_rect, margins, target};
    use crate::theme::{Accent, Surface, island};
    use crate::widgets::sticky::{MAX_H, MAX_W, MIN_H, MIN_W, Paper, contrast, tones};

    const SURFACES: [Surface; 6] = [
        Surface::Espresso,
        Surface::Graphite,
        Surface::Sea,
        Surface::Night,
        Surface::Plum,
        Surface::Terminal,
    ];

    #[test]
    fn the_tilt_is_stable_within_two_degrees_and_differs_between_notes() {
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..40 {
            let id = format!("n1700000000-{i}");
            let a = tilt_deg(&id);
            assert_eq!(a, tilt_deg(&id), "deterministic");
            assert!((-MAX_TILT..=MAX_TILT).contains(&a), "{id}: {a}");
            seen.insert((a * 1000.0) as i32);
        }
        assert!(seen.len() >= 35, "notes differ: only {} angles of 40", seen.len());
        assert_ne!(tilt_deg("n1-0"), tilt_deg("n1-1"));
        // Both leans occur.
        let angles: Vec<f32> = (0..40).map(|i| tilt_deg(&format!("n9-{i}"))).collect();
        assert!(angles.iter().any(|a| *a < -0.5) && angles.iter().any(|a| *a > 0.5));
        assert!(tilt_deg("").abs() <= MAX_TILT);
    }

    #[test]
    fn a_rotation_and_its_inverse_give_the_identity() {
        let (w, h) = (240.0, 208.0);
        for deg in [-2.0, -0.7, 0.0, 1.3, 2.0] {
            for (x, y) in [(0.0, 0.0), (w, h), (17.5, 120.25), (w / 2.0, h / 2.0), (-8.0, 300.0)] {
                let (rx, ry) = rotate_about(x, y, deg, w / 2.0, h / 2.0);
                let (bx, by) = unrotate(rx, ry, w, h, deg);
                assert!((bx - x).abs() < 0.001 && (by - y).abs() < 0.001, "{deg}: ({x},{y})");
            }
        }
        // The centre does not move.
        assert_eq!(rotate_about(w / 2.0, h / 2.0, 2.0, w / 2.0, h / 2.0), (w / 2.0, h / 2.0));
        assert_eq!(unrotate(5.0, 6.0, w, h, 0.0), (5.0, 6.0));
    }

    #[test]
    fn a_corner_of_a_leaning_note_maps_back_to_the_right_target() {
        let (w, h) = (240.0, 208.0);
        for deg in [-2.0f32, 2.0] {
            // Where each straight spot lands once drawn leaning; hit-testing
            // maps the pointer there back and must find the same target.
            let drawn = |x: f32, y: f32| rotate_about(x, y, deg, w / 2.0, h / 2.0);
            let hit = |px: (f32, f32)| {
                let (x, y) = unrotate(px.0, px.1, w, h, deg);
                target(x, y, w, h, false, None, 0.0, true)
            };
            // The grip's corner, the strip's left end, a button, the text.
            assert_eq!(hit(drawn(w - 4.0, h - 4.0)), Target::Grip, "{deg}");
            assert_eq!(hit(drawn(6.0, 8.0)), Target::Header, "{deg}");
            let close = button_rect(crate::widgets::paint::Btn::Close, w);
            assert_eq!(
                hit(drawn(close.x + close.w / 2.0, close.y + close.h / 2.0)),
                Target::Button(crate::widgets::paint::Btn::Close),
                "{deg}"
            );
            assert_eq!(hit(drawn(w / 2.0, h / 2.0)), Target::Body, "{deg}");
            assert_eq!(hit(drawn(40.0, h - 8.0)), Target::Footer, "{deg}");
            // Just outside a corner of the leaning note is outside.
            let out = drawn(-10.0, -10.0);
            assert_eq!(hit(out), Target::Outside, "{deg}");
        }
    }

    #[test]
    fn the_margin_grows_by_the_overhang_and_never_shrinks() {
        let base = margins();
        // Straight: the usual margin.
        assert_eq!(margins_for(240.0, 208.0, 0.0), base);
        for (w, h) in [(MIN_W, MIN_H), (240.0, 208.0), (MAX_W, MAX_H), (MAX_W, MIN_H)] {
            for deg in [-2.0f32, 2.0] {
                let m = margins_for(w, h, deg);
                assert!(m.0 >= base.0 && m.1 >= base.1 && m.2 >= base.2 && m.3 >= base.3);
                // The leaning corners are inside the window: each corner's
                // drawn place lies in [-m, size + m].
                for (x, y) in [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)] {
                    let (rx, ry) = rotate_about(x, y, deg, w / 2.0, h / 2.0);
                    assert!(rx >= base.0 - m.0 - 0.001, "{w}x{h} {deg} x {rx}");
                    assert!(rx <= w + m.2 - base.2 + 0.001, "{w}x{h} {deg} x {rx}");
                    assert!(ry >= base.1 - m.1 - 0.001, "{w}x{h} {deg} y {ry}");
                    assert!(ry <= h + m.3 - base.3 + 0.001, "{w}x{h} {deg} y {ry}");
                }
            }
        }
        // The tallest note leans furthest sideways: about h * sin / 2.
        let (ox, _) = overhang(MAX_W, MAX_H, 2.0);
        assert!((ox - (800.0 * 2.0f32.to_radians().sin() / 2.0)).abs() < 1.0, "{ox}");
        assert_eq!(overhang(100.0, 100.0, 0.0), (0.0, 0.0));
    }

    #[test]
    fn the_tilt_transform_keeps_the_centre_and_scales() {
        let m = tilt_transform(0.0, 100.0, 80.0, 1.5);
        assert_eq!((m.M11, m.M12, m.M21, m.M22, m.M31, m.M32), (1.5, 0.0, 0.0, 1.5, 0.0, 0.0));
        let m = tilt_transform(2.0, 100.0, 80.0, 1.0);
        let (x, y) = (100.0 * m.M11 + 80.0 * m.M21 + m.M31, 100.0 * m.M12 + 80.0 * m.M22 + m.M32);
        assert!((x - 100.0).abs() < 0.001 && (y - 80.0).abs() < 0.001, "the centre stays");
    }

    #[test]
    fn the_ruling_scrolls_with_the_text() {
        // Rules every 20, the first at 16, nothing scrolled.
        assert_eq!(ruling(16.0, 20.0, 0.0, 70.0), vec![16.0, 36.0, 56.0]);
        // Scrolled by 20: the same rules (one pitch).
        assert_eq!(ruling(16.0, 20.0, 20.0, 70.0), vec![16.0, 36.0, 56.0]);
        // Scrolled by 5: they move up by 5.
        assert_eq!(ruling(16.0, 20.0, 5.0, 70.0), vec![11.0, 31.0, 51.0, 71.0].into_iter().filter(|y| *y < 70.0).collect::<Vec<_>>());
        // Scrolled far down: still one rule per pitch (and 1005 is 5 past a pitch).
        assert_eq!(ruling(16.0, 20.0, 1005.0, 70.0), vec![11.0, 31.0, 51.0]);
        assert!(ruling(0.0, 0.0, 0.0, 10.0).is_empty());
    }

    #[test]
    fn every_text_tone_reads_over_the_ruling_on_every_paper_and_surface() {
        for surface in SURFACES {
            for accent in Accent::ALL {
                let isl = island(*accent, surface);
                for paper in Paper::ALL {
                    let t = tones(paper, &isl, *accent);
                    for (name, ink) in [
                        ("title", t.title),
                        ("body", t.body),
                        ("secondary", t.secondary),
                        ("done", t.done),
                    ] {
                        let c = contrast(ink, t.line);
                        assert!(c >= 4.5, "{paper:?} {name} on its ruling, {surface:?}/{accent:?}: {c:.2}");
                    }
                    // The ruling is there to see: a hair off the paper.
                    assert_ne!(t.line, t.fill, "{paper:?}");
                }
            }
        }
    }

    #[test]
    fn the_stamp_reads_on_every_paper_surface_and_accent() {
        for surface in SURFACES {
            for accent in Accent::ALL {
                let isl = island(*accent, surface);
                for paper in Paper::ALL {
                    let t = tones(paper, &isl, *accent);
                    let c = contrast(stamp_ink(&t), t.fill);
                    assert!(c >= 3.0, "{paper:?} stamp on {surface:?}/{accent:?}: {c:.2}");
                }
            }
        }
    }

    #[test]
    fn the_stamp_sits_in_the_text_area_clear_of_the_footer_and_the_grip() {
        for (w, h) in [(MIN_W, MIN_H), (240.0, 208.0), (MAX_W, MAX_H), (MIN_W, MAX_H), (MAX_W, MIN_H)] {
            let (cx, cy) = stamp_centre(w, h);
            let area = body_rect(w, h);
            let r = STAMP_R + 1.0;
            assert!(cx - r >= area.x && cx + r <= area.x + area.w, "{w}x{h}: x {cx}");
            assert!(cy - r >= area.y && cy + r <= area.y + area.h, "{w}x{h}: y {cy}");
            // Clear of the footer's band and of the grip.
            assert!(cy + r <= h - FOOTER_H, "{w}x{h}");
            let grip = grip_rect(w, h);
            let (nx, ny) = (cx.clamp(grip.x, grip.x + grip.w), cy.clamp(grip.y, grip.y + grip.h));
            assert!(((cx - nx).powi(2) + (cy - ny).powi(2)).sqrt() > r, "{w}x{h}: the grip");
            assert!(GRIP <= 16.0);
        }
    }

    #[test]
    fn the_tape_and_the_pin_keep_clear_of_the_cats_face() {
        for w in [MIN_W, 180.0, 200.0, 220.0, 240.0, 320.0, 480.0, MAX_W] {
            let cat = cat_left(w);
            for half in [TAPE_W / 2.0, PIN_R] {
                let c = deco_centre(w, half);
                assert!(c - half >= 0.0 && c + half <= w, "{w}: inside the note");
                assert!(
                    c + half <= cat || c - half >= cat + CAT_SIZE,
                    "{w}: deco {}..{} meets the cat {}..{}",
                    c - half,
                    c + half,
                    cat,
                    cat + CAT_SIZE
                );
            }
        }
        // On a wide note the tape is the top centre.
        assert_eq!(deco_centre(480.0, TAPE_W / 2.0), 240.0);
        // The tape's stripes are the accent and a light one.
        let isl = island(Accent::Terracotta, Surface::Espresso);
        let t = tones(Paper::Krem, &isl, Accent::Terracotta);
        let (a, b) = tape_colours(&t);
        assert_ne!(a, b);
    }

    #[test]
    fn the_stamps_have_icons_and_the_progress_bar_fits_the_strip() {
        assert_eq!(stamp_icon(Stamp::None), None);
        let icons: Vec<Icon> = Stamp::ALL.iter().filter_map(|s| stamp_icon(*s)).collect();
        assert_eq!(icons.len(), 6);
        let mut dedup = icons.clone();
        dedup.dedup();
        assert_eq!(dedup.len(), 6, "each its own");
        let r = progress_rect(240.0);
        assert_eq!((r.x, r.w, r.h), (PAD_X, 240.0 - 2.0 * PAD_X, 2.0));
        assert!(r.y + r.h <= HEADER_H, "under the strip's buttons, above the text");
        assert_eq!(progress_fraction(0, 0), 0.0);
        assert_eq!(progress_fraction(1, 4), 0.25);
        assert_eq!(progress_fraction(4, 4), 1.0);
        assert_eq!(progress_fraction(9, 4), 1.0);
    }

    #[test]
    fn confetti_stays_in_the_window_and_is_gone_after_800_ms() {
        for (w, h) in [(256.0, 270.0), (176.0, 190.0), (656.0, 868.0)] {
            for seed in [0u64, 1, 42, u64::MAX] {
                let mut t = 0.0;
                while t < CONFETTI_MS {
                    let pieces = confetti(seed, t, w, h);
                    assert_eq!(pieces.len(), PIECES, "t {t}");
                    for p in &pieces {
                        assert!(p.x >= 0.0 && p.x <= w && p.y >= 0.0 && p.y <= h, "{w}x{h} t {t}: {p:?}");
                        assert!(p.x - p.w / 2.0 >= 0.0 || p.x - p.h / 2.0 >= 0.0);
                        assert!((0.0..=1.0).contains(&p.alpha) && p.colour < 4);
                    }
                    t += 20.0;
                }
                assert!(confetti(seed, CONFETTI_MS, w, h).is_empty());
                assert!(confetti(seed, CONFETTI_MS + 500.0, w, h).is_empty());
                assert!(confetti(seed, -1.0, w, h).is_empty());
            }
        }
        // The same seed, the same burst; another seed, another.
        assert_eq!(confetti(7, 300.0, 256.0, 270.0), confetti(7, 300.0, 256.0, 270.0));
        assert_ne!(confetti(7, 300.0, 256.0, 270.0), confetti(8, 300.0, 256.0, 270.0));
        // It moves, and fades towards the end.
        assert_ne!(confetti(7, 100.0, 256.0, 270.0), confetti(7, 400.0, 256.0, 270.0));
        let late = confetti(7, 790.0, 256.0, 270.0);
        assert!(late.iter().all(|p| p.alpha < 0.1));
        // The palette is from the paper and accent.
        let isl = island(Accent::Sage, Surface::Night);
        let t = tones(Paper::Krem, &isl, Accent::Sage);
        let palette = confetti_palette(&t);
        assert_eq!(palette.len(), 4);
        assert_eq!(palette[0], t.accent);
    }

    #[test]
    fn the_clock_sits_in_the_top_margin_left_of_the_cat() {
        for w in [MIN_W, 240.0, MAX_W] {
            let (cx, cy) = clock_centre(w);
            assert!(cy < 0.0 && cy > -crate::widgets::paint::CAT_ROOM, "{w}: in the margin");
            assert!(cx + CLOCK_R + 3.0 <= cat_left(w) + 0.001, "{w}: left of the cat");
            assert!(cx - CLOCK_R - 6.0 >= 0.0, "{w}: inside the note's width");
        }
        let _ = (button_rect, Lang::Id);
    }
}
