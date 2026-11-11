//! The caption card's pure rules: where the header buttons and the body sit,
//! which part a point is over, how the lines stack on the body's bottom, where
//! the window goes first, and the colours with their contrast. No Win32 and no
//! drawing, so all of it is tested. Units are DIPs unless a name says `px`.

use crate::config::CardPlace;
use crate::lyrics_card::colour;
pub use crate::lyrics_card::layout::RectF;
use crate::lyrics_card::layout::{Mon, place_with, px};
use crate::ui::settings_window::Rc;

// ------------------------------------------------------------------ sizes

pub const DEFAULT_W: f32 = 380.0;
pub const DEFAULT_H: f32 = 220.0;
pub const MIN_W: f32 = 260.0;
pub const MIN_H: f32 = 140.0;
pub const MAX_W: f32 = 760.0;
pub const MAX_H: f32 = 600.0;
pub const RADIUS: f32 = 14.0;
/// The side padding of the header and the body.
pub const PAD: f32 = 12.0;
pub const HEADER_H: f32 = 34.0;
pub const GRIP: f32 = 16.0;
/// Space between two caption lines.
pub const LINE_GAP: f32 = 8.0;
/// Space between a speaker label and its text.
pub const LABEL_GAP: f32 = 6.0;
/// The body fades out this far in at its top and bottom edge.
pub const EDGE_FADE: f32 = 24.0;
/// One wheel notch scrolls back this far.
pub const WHEEL_STEP: f32 = 48.0;
/// After this long without wheel input the list returns to the newest line.
pub const MANUAL_HOLD_MS: u32 = 3000;

/// A header button's hit square, and the gap between header parts.
const BUTTON: f32 = 24.0;
const PART_GAP: f32 = 4.0;
const DOT: f32 = 8.0;
const TIME_W: f32 = 40.0;
const COPY_W: f32 = 36.0;
const CHIP_W: f32 = 44.0;
const ACTION_W: f32 = 200.0;
const ACTION_H: f32 = 30.0;
/// Where the default card sits above the work area's bottom edge (subtitles).
const BOTTOM_MARGIN: f32 = 96.0;

/// Where each part of a card `w` x `h` sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rects {
    pub header: RectF,
    /// The status dot.
    pub dot: RectF,
    /// The session's elapsed time.
    pub time: RectF,
    /// "Caption langsung"; may be empty on a narrow card.
    pub title: RectF,
    /// The language chip.
    pub lang_chip: RectF,
    /// "Salin".
    pub copy: RectF,
    pub pause: RectF,
    pub close: RectF,
    /// The lines stack on this one's bottom.
    pub body: RectF,
    pub grip: RectF,
    /// The status button in the body ("Unduh model", "Tetap jalan",
    /// "Nyalakan Transkrip"); a hit only while shown.
    pub action: RectF,
}

pub fn card_rects(w: f32, h: f32) -> Rects {
    let by = (HEADER_H - BUTTON) / 2.0;
    let close = RectF::new(w - PAD - BUTTON, by, BUTTON, BUTTON);
    let pause = RectF::new(close.x - PART_GAP - BUTTON, by, BUTTON, BUTTON);
    let copy = RectF::new(pause.x - PART_GAP - COPY_W, by, COPY_W, BUTTON);
    let lang_chip = RectF::new(copy.x - PART_GAP - CHIP_W, by, CHIP_W, BUTTON);
    let dot = RectF::new(PAD, (HEADER_H - DOT) / 2.0, DOT, DOT);
    let time = RectF::new(dot.right() + 6.0, by, TIME_W, BUTTON);
    let title_x = time.right() + 6.0;
    let title = RectF::new(
        title_x,
        by,
        (lang_chip.x - PART_GAP - title_x).max(0.0),
        BUTTON,
    );
    let body = RectF::new(0.0, HEADER_H, w, (h - HEADER_H - PAD).max(0.0));
    let aw = ACTION_W.min((w - 2.0 * PAD).max(0.0));
    let ah = ACTION_H.min(body.h);
    let action = RectF::new((w - aw) / 2.0, body.y + (body.h - ah) / 2.0, aw, ah);
    Rects {
        header: RectF::new(0.0, 0.0, w, HEADER_H),
        dot,
        time,
        title,
        lang_chip,
        copy,
        pause,
        close,
        body,
        grip: RectF::new(w - GRIP, h - GRIP, GRIP, GRIP),
        action,
    }
}

/// `size` kept in the card's limits.
pub fn clamp_size(w: f32, h: f32) -> (f32, f32) {
    (w.clamp(MIN_W, MAX_W), h.clamp(MIN_H, MAX_H))
}

// ------------------------------------------------------------------- zones

/// What a point in the card is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// The header outside its buttons (a press drags the card).
    Header,
    LangChip,
    Copy,
    Pause,
    Close,
    Action,
    Body,
    Grip,
}

/// The part under `pt` (DIPs from the card's top left), or `None` outside the
/// card. The status button only counts while `action_shown`.
pub fn zone_at(r: &Rects, pt: (f32, f32), action_shown: bool) -> Option<Zone> {
    let (x, y) = pt;
    let card = RectF::new(0.0, 0.0, r.header.w, r.grip.bottom());
    if !card.contains(x, y) {
        return None;
    }
    let buttons = [
        (r.close, Zone::Close),
        (r.pause, Zone::Pause),
        (r.copy, Zone::Copy),
        (r.lang_chip, Zone::LangChip),
    ];
    if let Some((_, z)) = buttons.iter().find(|(b, _)| b.contains(x, y)) {
        return Some(*z);
    }
    if action_shown && r.action.contains(x, y) {
        return Some(Zone::Action);
    }
    if r.grip.contains(x, y) {
        return Some(Zone::Grip);
    }
    Some(if r.header.contains(x, y) {
        Zone::Header
    } else {
        Zone::Body
    })
}

// ---------------------------------------------------------------- stacking

/// The height of all lines and the gaps between them.
fn stack_height(heights: &[f32], gap: f32) -> f32 {
    if heights.is_empty() {
        return 0.0;
    }
    heights.iter().sum::<f32>() + gap * (heights.len() - 1) as f32
}

/// How far the list can be scrolled back: until the oldest line's top is at
/// the body's top. 0 when everything fits.
pub fn scroll_back_max(heights: &[f32], gap: f32, body_h: f32) -> f32 {
    (stack_height(heights, gap) - body_h).max(0.0)
}

/// The top of each line (from the body's top), the newest resting on the
/// body's bottom. `scroll_back` (>= 0) moves older lines into view, kept at
/// `scroll_back_max`.
pub fn bottom_stack(heights: &[f32], gap: f32, body_h: f32, scroll_back: f32) -> Vec<f32> {
    let back = scroll_back.clamp(0.0, scroll_back_max(heights, gap, body_h));
    let mut y = body_h - stack_height(heights, gap) + back;
    heights
        .iter()
        .map(|h| {
            let top = y;
            y += h + gap;
            top
        })
        .collect()
}

// --------------------------------------------------------------- placement

/// The card's place: the saved one, else the default size centred
/// horizontally at the bottom of the notch monitor's work area, 96 dip above
/// its bottom (where subtitles sit), never bigger than the work area.
pub fn place_default(saved: Option<CardPlace>, monitors: &[Mon], notch: Option<Mon>) -> Option<Rc> {
    place_with(
        saved,
        monitors,
        notch,
        |m| {
            let w = px(DEFAULT_W, m.dpi).min(m.work.width().max(1));
            let h = px(DEFAULT_H, m.dpi).min(m.work.height().max(1));
            let left = m.work.left + (m.work.width() - w) / 2;
            let bottom = m.work.bottom - px(BOTTOM_MARGIN, m.dpi);
            let top = (bottom - h).max(m.work.top);
            Rc {
                left,
                top,
                right: left + w,
                bottom: top + h,
            }
        },
        ((MIN_W, MIN_H), (MAX_W, MAX_H)),
    )
}

// ------------------------------------------------------------------ colours

/// The card's colours (`0xRRGGBB`). The card is always dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub bg: u32,
    /// Final lines.
    pub text: u32,
    /// The interim line, still being heard.
    pub grey: u32,
    /// The "Saya" label (the microphone).
    pub me: u32,
    /// The "Peserta" label (the others on the PC's sound).
    pub others: u32,
    /// The quiet header text (time, title), a step under `grey`.
    pub gap: u32,
}

/// The palette on `dark_card`, the dark window palette's card colour.
/// The two labels differ in hue (amber, teal) and carry their name, so colour
/// is never the only cue.
pub fn palette(dark_card: u32) -> Palette {
    Palette {
        bg: dark_card,
        text: 0xFFFFFF,
        grey: 0xA8ABB0,
        me: 0xE8B16A,
        others: 0x7FC8C0,
        gap: 0x9A9EA3,
    }
}

/// WCAG contrast of `fg` on `bg`.
pub fn contrast(fg: u32, bg: u32) -> f32 {
    colour::contrast(fg, bg) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics_card::layout::{Mon, px};
    use crate::theme::{Accent, Surface, WindowTheme, window_for};
    use crate::ui::settings_window::Rc;

    #[test]
    fn the_newest_line_rests_on_the_bottom() {
        let heights = [20.0, 40.0, 20.0];
        let tops = bottom_stack(&heights, LINE_GAP, 200.0, 0.0);
        let last = tops.len() - 1;
        assert_eq!(tops[last] + heights[last], 200.0);
        assert_eq!(tops[1] + heights[1] + LINE_GAP, tops[2]);
        assert!(bottom_stack(&[], LINE_GAP, 200.0, 0.0).is_empty());
        // Few lines sit at the bottom, not the top.
        assert!(tops[0] > 0.0);
    }

    #[test]
    fn scroll_back_is_clamped_to_the_oldest_line() {
        let heights = [30.0; 10];
        let body_h = 100.0;
        let max = scroll_back_max(&heights, 8.0, body_h);
        assert_eq!(max, 10.0 * 30.0 + 9.0 * 8.0 - body_h);
        let at_max = bottom_stack(&heights, 8.0, body_h, max);
        assert_eq!(at_max[0], 0.0, "the oldest line reaches the body's top");
        assert_eq!(bottom_stack(&heights, 8.0, body_h, max + 500.0), at_max);
        assert_eq!(
            bottom_stack(&heights, 8.0, body_h, -50.0),
            bottom_stack(&heights, 8.0, body_h, 0.0)
        );
        // Everything fits: nothing to scroll back to.
        assert_eq!(scroll_back_max(&[20.0, 20.0], 8.0, 200.0), 0.0);
        assert_eq!(scroll_back_max(&[], 8.0, 200.0), 0.0);
    }

    fn centre(r: RectF) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn zones_cover_the_header_buttons() {
        for (w, h) in [(DEFAULT_W, DEFAULT_H), (MIN_W, MIN_H), (MAX_W, MAX_H)] {
            let r = card_rects(w, h);
            let at = |c: RectF, shown| zone_at(&r, centre(c), shown);
            assert_eq!(at(r.close, false), Some(Zone::Close));
            assert_eq!(at(r.pause, false), Some(Zone::Pause));
            assert_eq!(at(r.copy, false), Some(Zone::Copy));
            assert_eq!(at(r.lang_chip, false), Some(Zone::LangChip));
            assert_eq!(at(r.grip, false), Some(Zone::Grip));
            assert_eq!(at(r.action, true), Some(Zone::Action));
            assert_eq!(at(r.action, false), Some(Zone::Body), "no button, no zone");
            assert_eq!(zone_at(&r, (-1.0, 50.0), true), None);
            assert_eq!(zone_at(&r, (w, 50.0), true), None);
            assert_eq!(zone_at(&r, (50.0, h), true), None);
            // The header parts keep their order and stay apart.
            let row = [
                r.dot,
                r.time,
                r.title,
                r.lang_chip,
                r.copy,
                r.pause,
                r.close,
            ];
            for pair in row.windows(2) {
                assert!(pair[0].right() <= pair[1].x + 0.01, "{w}x{h}: {pair:?}");
            }
            assert!(r.title.w >= 0.0);
            assert!(r.close.right() <= w - PAD + 0.01);
            for c in row {
                assert!(c.y >= 0.0 && c.bottom() <= HEADER_H, "{w}x{h}");
            }
            // The body sits under the header and holds the action button.
            assert_eq!(r.body.y, HEADER_H);
            assert!(r.body.bottom() <= h);
            assert!(r.action.x >= 0.0 && r.action.right() <= w);
            assert!(r.action.y >= r.body.y && r.action.bottom() <= r.body.bottom());
            assert_eq!(r.grip, RectF::new(w - GRIP, h - GRIP, GRIP, GRIP));
        }
        let r = card_rects(DEFAULT_W, DEFAULT_H);
        assert_eq!(zone_at(&r, (100.0, 3.0), false), Some(Zone::Header));
        assert_eq!(zone_at(&r, (100.0, 100.0), false), Some(Zone::Body));
    }

    #[test]
    fn sizes_clamp() {
        assert_eq!(clamp_size(10.0, 10.0), (MIN_W, MIN_H));
        assert_eq!(clamp_size(5000.0, 5000.0), (MAX_W, MAX_H));
        assert_eq!(clamp_size(400.0, 300.0), (400.0, 300.0));
        assert_eq!((DEFAULT_W, DEFAULT_H), (380.0, 220.0));
    }

    fn rc(left: i32, top: i32, right: i32, bottom: i32) -> Rc {
        Rc {
            left,
            top,
            right,
            bottom,
        }
    }

    fn mon(rect: Rc, dpi: u32) -> Mon {
        Mon {
            rect,
            work: Rc {
                bottom: rect.bottom - px(40.0, dpi),
                ..rect
            },
            dpi,
        }
    }

    #[test]
    fn the_default_place_is_bottom_centre_of_the_notch_monitor() {
        let m = mon(rc(0, 0, 1920, 1080), 96);
        let r = place_default(None, &[m], Some(m)).unwrap();
        assert_eq!((r.width(), r.height()), (380, 220));
        assert_eq!(r.left, (1920 - 380) / 2);
        assert_eq!(r.bottom, m.work.bottom - 96);
        // 150 %: the same in dip, on a monitor that is not at the origin.
        let m = mon(rc(1920, 0, 1920 + 2880, 1620), 144);
        let r = place_default(None, &[m], Some(m)).unwrap();
        assert_eq!((r.width(), r.height()), (570, 330));
        assert_eq!(r.left - m.work.left, (m.work.width() - 570) / 2);
        assert_eq!(m.work.bottom - r.bottom, 144);
        // A small work area keeps it inside.
        let m = mon(rc(0, 0, 300, 250), 96);
        let r = place_default(None, &[m], Some(m)).unwrap();
        assert!(r.left >= m.work.left && r.right <= m.work.right);
        assert!(r.top >= m.work.top && r.bottom <= m.work.bottom, "{r:?}");
        assert_eq!(place_default(None, &[], None), None);
    }

    #[test]
    fn a_saved_place_is_kept_inside_the_card_limits() {
        use crate::config::CardPlace;
        let m = mon(rc(0, 0, 1920, 1080), 96);
        let big = CardPlace {
            left: 100,
            top: 100,
            right: 1500,
            bottom: 1000,
        };
        let r = place_default(Some(big), &[m], Some(m)).unwrap();
        assert_eq!((r.width(), r.height()), (760, 600));
        let tiny = CardPlace {
            left: 100,
            top: 100,
            right: 150,
            bottom: 150,
        };
        let r = place_default(Some(tiny), &[m], Some(m)).unwrap();
        assert_eq!((r.width(), r.height()), (260, 140));
    }

    #[test]
    fn contrast_numbers() {
        assert!((contrast(0xFFFFFF, 0x000000) - 21.0).abs() < 0.01);
        assert!((contrast(0x777777, 0xFFFFFF) - 4.48).abs() < 0.01);
        // The dark window palette of every surface and accent: the card
        // colour is the background, and the palette's bg is checked too.
        let mut seen = std::collections::BTreeSet::new();
        for &surface in Surface::ALL {
            for &accent in Accent::ALL {
                let w = window_for(accent, surface, WindowTheme::Dark, false);
                for (what, base) in [("card", w.card), ("bg", w.bg)] {
                    if !seen.insert(base) {
                        continue;
                    }
                    let p = palette(base);
                    let t = contrast(p.text, p.bg);
                    let g = contrast(p.grey, p.bg);
                    let me = contrast(p.me, p.bg);
                    let o = contrast(p.others, p.bg);
                    let gap = contrast(p.gap, p.bg);
                    println!(
                        "{surface:?} {what} #{base:06X}: text {t:.2} grey {g:.2} me {me:.2} others {o:.2} gap {gap:.2}"
                    );
                    assert!(t >= 7.0, "{surface:?} {what} text {t}");
                    assert!(g >= 4.5, "{surface:?} {what} grey {g}");
                    assert!(me >= 4.5, "{surface:?} {what} me {me}");
                    assert!(o >= 4.5, "{surface:?} {what} others {o}");
                    assert!(gap >= 4.5, "{surface:?} {what} gap {gap}");
                    // The interim line reads clearly greyer than final text.
                    assert!(t > g + 3.0);
                }
            }
        }
        assert!(seen.len() >= 4, "warm and Terminal, card and bg");
        assert_eq!(palette(0x232527).text, 0xFFFFFF);
    }
}
