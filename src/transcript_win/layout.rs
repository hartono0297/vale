//! The transcript window's rules, pure so they are tested without a window:
//! which buttons a status gives, where everything sits (dip), the line
//! geometry of the list (which rows are visible, which row is under the
//! pointer, the follow band), the seek maths and where the window goes.

use std::ops::Range;

use super::Status;
use crate::config::CardPlace;
use crate::i18n::Key;
use crate::ui::tooltip::{KeyHint, Tip};
use crate::lyrics_card::layout::{Mon, home_monitor, px};
use crate::ui::geometry::Rect;
use crate::ui::settings_window::Rc;

// ------------------------------------------------------------------ sizes

pub const DEFAULT_W: f32 = 640.0;
pub const DEFAULT_H: f32 = 720.0;
pub const MIN_W: f32 = 480.0;
pub const MIN_H: f32 = 400.0;

/// The left and right margin of everything.
pub const PAD: f32 = 20.0;
const HEADER_TOP: f32 = 16.0;
pub const TITLE_H: f32 = 24.0;
pub const META_H: f32 = 18.0;
const TITLE_GAP: f32 = 2.0;
/// Between the header and the buttons, and the buttons and the player.
const SECTION_GAP: f32 = 14.0;
pub const BTN_H: f32 = 30.0;
pub const BTN_GAP: f32 = 8.0;
/// A button's text sits this far from its sides.
pub const BTN_PAD_X: f32 = 12.0;
pub const PLAYER_H: f32 = 44.0;
pub const PLAY: f32 = 32.0;
/// The position's and the length's boxes.
pub const CLOCK_W: f32 = 58.0;
/// The seek bar's hit area is this tall; the track is drawn in its middle.
pub const BAR_HIT_H: f32 = 20.0;
pub const TRACK_H: f32 = 4.0;
pub const THUMB: f32 = 12.0;
pub const STATUS_H: f32 = 32.0;

/// The list's rows: the time column, the label chip's column (two-channel
/// recordings only) and the wrapped text.
pub const TIME_W: f32 = 58.0;
pub const CHIP_COL: f32 = 76.0;
pub const LINE_PAD_Y: f32 = 7.0;
/// The smallest text box a row has (one line of 13.5 px text).
pub const TEXT_MIN_H: f32 = 20.0;
/// The list's own space above its first row.
pub const LIST_TOP: f32 = 6.0;
/// A row's fill reaches this far past the side margins.
pub const ROW_BLEED: f32 = 8.0;
/// One wheel notch.
pub const WHEEL_STEP: f32 = 48.0;
/// The follow band: the playing line is kept between these shares of the
/// list's height.
pub const BAND_TOP: f32 = 0.25;
pub const BAND_BOTTOM: f32 = 0.60;
/// A second window opens this far (px) down and right of the newest one.
pub const CASCADE_PX: i32 = 24;

// ------------------------------------------------------------------ buttons

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Copy,
    OpenMd,
    OpenFolder,
    MakeNote,
    CopyPrompt,
    Redo,
    Cancel,
    Retry,
    Settings,
}

impl Btn {
    pub fn label(self) -> Key {
        match self {
            Btn::Copy => Key::TrBtnCopy,
            Btn::OpenMd => Key::TrBtnOpenMd,
            Btn::OpenFolder => Key::RecOpenFolder,
            Btn::MakeNote => Key::TrBtnNote,
            Btn::CopyPrompt => Key::TrBtnPrompt,
            Btn::Redo => Key::TrMenuRedo,
            Btn::Cancel => Key::TrCancel,
            Btn::Retry => Key::TrBtnRetry,
            Btn::Settings => Key::TrBtnSettings,
        }
    }

    /// The one accent-filled button.
    pub fn primary(self) -> bool {
        self == Btn::CopyPrompt
    }
}

/// The buttons of a window in `status`, in the spec's order.
pub fn buttons_for(status: &Status, has_lines: bool, can_redo: bool) -> Vec<Btn> {
    let redo = can_redo.then_some(Btn::Redo);
    match status {
        Status::Queued | Status::Waiting | Status::Held | Status::Running { .. } => vec![Btn::Cancel],
        Status::Failed(_) => vec![Btn::Retry, Btn::OpenFolder],
        Status::Off => vec![Btn::Settings],
        Status::None | Status::PlayerError if has_lines => [
            Btn::Copy,
            Btn::OpenMd,
            Btn::OpenFolder,
            Btn::MakeNote,
            Btn::CopyPrompt,
        ]
        .into_iter()
        .chain(redo)
        .collect(),
        // An empty transcript (or none yet).
        Status::None | Status::PlayerError => redo.into_iter().chain([Btn::OpenFolder]).collect(),
    }
}

// ------------------------------------------------------------------ tips

/// What the pointer is over in the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hot {
    Btn(usize),
    Play,
    Bar,
    Line(usize),
}

/// The tip of `hot`. `playing` is whether the player runs (the play button
/// says what a click does now); `btn` is the button under `Hot::Btn`. The
/// rows of the list and the buttons that say what they do have none. Ctrl+C
/// triggers the plain Salin button, so the key is on that one.
pub fn tip_of(hot: Hot, playing: bool, btn: Option<Btn>) -> Option<Tip> {
    match hot {
        Hot::Play if playing => {
            Some(Tip::named(Key::TipPause).key(KeyHint::Fixed(Key::TipKeySpace)))
        }
        Hot::Play => Some(
            Tip::named(Key::TipPlay)
                .about(Key::TipTrPlayAbout)
                .key(KeyHint::Fixed(Key::TipKeySpace)),
        ),
        Hot::Bar => Some(
            Tip::named(Key::TipTrSeek)
                .about(Key::TipTrSeekAbout)
                .key(KeyHint::Raw("\u{2190} \u{2192}")),
        ),
        Hot::Btn(_) => match btn? {
            Btn::Copy => Some(Tip::short(Key::TipTrCopyAbout).key(KeyHint::Raw("Ctrl+C"))),
            Btn::CopyPrompt => Some(Tip::short(Key::TipTrPromptAbout)),
            Btn::MakeNote => Some(Tip::short(Key::TipTrNoteAbout)),
            Btn::OpenMd => Some(Tip::short(Key::TipTrOpenMdAbout)),
            Btn::Redo => Some(Tip::short(Key::TipTrRedoAbout)),
            Btn::Settings => Some(Tip::short(Key::TipTrSettingsAbout)),
            Btn::OpenFolder | Btn::Cancel | Btn::Retry => None,
        },
        Hot::Line(_) => None,
    }
}

// ------------------------------------------------------------------ frame

/// Where everything sits in a window `w` × `h` dip.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub header: Rect,
    pub buttons: Vec<(Btn, Rect)>,
    pub play: Rect,
    pub position: Rect,
    /// The seek bar's hit area.
    pub bar: Rect,
    pub length: Rect,
    pub status: Option<Rect>,
    pub list: Rect,
}

/// The frame of a `w` × `h` window with `buttons` (each with its measured
/// width) and, when `status`, a status line above the list. The buttons are
/// `BTN_H` high and `BTN_GAP` apart, wrapping to more rows.
pub fn frame(w: f32, h: f32, buttons: &[(Btn, f32)], status: bool) -> Frame {
    let inner = (w - 2.0 * PAD).max(0.0);
    let header = Rect {
        x: PAD,
        y: HEADER_TOP,
        w: inner,
        h: TITLE_H + TITLE_GAP + META_H,
    };
    let mut placed = Vec::with_capacity(buttons.len());
    let (mut x, mut y) = (PAD, header.bottom() + SECTION_GAP);
    for &(btn, bw) in buttons {
        let bw = bw.min(inner);
        if x > PAD && x + bw > w - PAD {
            x = PAD;
            y += BTN_H + BTN_GAP;
        }
        placed.push((btn, Rect { x, y, w: bw, h: BTN_H }));
        x += bw + BTN_GAP;
    }
    let player = if buttons.is_empty() {
        header.bottom() + SECTION_GAP
    } else {
        y + BTN_H + SECTION_GAP
    };
    let play = Rect {
        x: PAD,
        y: player + (PLAYER_H - PLAY) / 2.0,
        w: PLAY,
        h: PLAY,
    };
    let position = Rect {
        x: play.right() + 12.0,
        y: player,
        w: CLOCK_W,
        h: PLAYER_H,
    };
    let length = Rect {
        x: w - PAD - CLOCK_W,
        y: player,
        w: CLOCK_W,
        h: PLAYER_H,
    };
    let bar_x = position.right() + 8.0;
    let bar = Rect {
        x: bar_x,
        y: player + (PLAYER_H - BAR_HIT_H) / 2.0,
        w: (length.x - 8.0 - bar_x).max(0.0),
        h: BAR_HIT_H,
    };
    let mut top = player + PLAYER_H;
    let status = status.then(|| {
        let r = Rect {
            x: PAD,
            y: top,
            w: inner,
            h: STATUS_H,
        };
        top += STATUS_H;
        r
    });
    Frame {
        header,
        buttons: placed,
        play,
        position,
        bar,
        length,
        status,
        list: Rect {
            x: 0.0,
            y: top,
            w,
            h: (h - top).max(0.0),
        },
    }
}

// ------------------------------------------------------------------ lines

/// The top of each row in the list's content (the first at 0).
pub fn line_tops(heights: &[f32]) -> Vec<f32> {
    let mut at = 0.0;
    heights
        .iter()
        .map(|h| {
            let top = at;
            at += h;
            top
        })
        .collect()
}

/// The first index in `0..n` for which `past` is false (`past` holds for a
/// leading run only).
fn first_not(n: usize, past: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if past(mid) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The content's height.
pub fn content_h(tops: &[f32], heights: &[f32]) -> f32 {
    match (tops.last(), heights.last()) {
        (Some(t), Some(h)) => t + h,
        _ => 0.0,
    }
}

/// The furthest the list scrolls.
pub fn max_scroll(tops: &[f32], heights: &[f32], list_h: f32) -> f32 {
    (content_h(tops, heights) + LIST_TOP - list_h).max(0.0)
}

/// The rows any part of which shows in a list `list_h` high scrolled by
/// `scroll`: only these are laid out and drawn.
pub fn visible(tops: &[f32], heights: &[f32], scroll: f32, list_h: f32) -> Range<usize> {
    let n = tops.len().min(heights.len());
    let start = first_not(n, |i| LIST_TOP + tops[i] + heights[i] <= scroll);
    let end = first_not(n, |i| LIST_TOP + tops[i] < scroll + list_h);
    start..end.max(start)
}

/// The row at `y` (from the list's top, `LIST_TOP` included) when scrolled by
/// `scroll`.
pub fn line_at(tops: &[f32], heights: &[f32], scroll: f32, y: f32) -> Option<usize> {
    let n = tops.len().min(heights.len());
    let at = y + scroll - LIST_TOP;
    let i = first_not(n, |i| tops[i] + heights[i] <= at);
    (i < n && tops[i] <= at).then_some(i)
}

/// The scroll that keeps row `current` between `BAND_TOP` and `BAND_BOTTOM` of
/// the list (its top at `BAND_TOP` when it is taller than the band), moving as
/// little as it can, inside the scroll range.
pub fn follow_scroll(tops: &[f32], heights: &[f32], current: usize, scroll: f32, list_h: f32) -> f32 {
    let (Some(&top), Some(&h)) = (tops.get(current), heights.get(current)) else {
        return scroll;
    };
    let (band_top, band_bottom) = (BAND_TOP * list_h, BAND_BOTTOM * list_h);
    let top = LIST_TOP + top;
    let mut s = scroll;
    if top - s < band_top {
        s = top - band_top;
    } else if top + h - s > band_bottom {
        // As little as it takes, but never past the band's top.
        s = (top + h - band_bottom).min(top - band_top);
    }
    s.clamp(0.0, max_scroll(tops, heights, list_h))
}

/// The least scroll that shows all of row `i` (its top when it is taller than
/// the list).
pub fn ensure_visible(tops: &[f32], heights: &[f32], i: usize, scroll: f32, list_h: f32) -> f32 {
    let (Some(&top), Some(&h)) = (tops.get(i), heights.get(i)) else {
        return scroll;
    };
    let top = top + LIST_TOP;
    let s = if top < scroll {
        top - LIST_TOP
    } else if top + h > scroll + list_h {
        (top + h - list_h).min(top - LIST_TOP)
    } else {
        scroll
    };
    s.clamp(0.0, max_scroll(tops, heights, list_h))
}

/// The time under `x` on the seek bar `bar`, kept inside the recording.
pub fn seek_ms(bar: Rect, x: f32, duration_ms: u64) -> u64 {
    if bar.w <= 0.0 {
        return 0;
    }
    let share = ((x - bar.x) / bar.w).clamp(0.0, 1.0);
    ((f64::from(share) * duration_ms as f64).round() as u64).min(duration_ms)
}

// ------------------------------------------------------------------ placement

/// Where the window goes (screen px). The saved rect is used when at least
/// half of it lies on one monitor, its top left kept and its size within the
/// minimum and that monitor's work area; otherwise (first open, the monitor is
/// gone) the default size centred on `fallback`'s work area, shrunk to fit it.
pub fn place(saved: Option<CardPlace>, monitors: &[Mon], fallback: Mon) -> Rc {
    if let Some(p) = saved {
        let r = Rc {
            left: p.left,
            top: p.top,
            right: p.right,
            bottom: p.bottom,
        };
        if let Some(m) = home_monitor(r, monitors) {
            let w = r.width().max(px(MIN_W, m.dpi)).min(m.work.width().max(1));
            let h = r.height().max(px(MIN_H, m.dpi)).min(m.work.height().max(1));
            return Rc {
                left: r.left,
                top: r.top,
                right: r.left + w,
                bottom: r.top + h,
            };
        }
    }
    let work = fallback.work;
    let w = px(DEFAULT_W, fallback.dpi).min(work.width().max(1));
    let h = px(DEFAULT_H, fallback.dpi).min(work.height().max(1));
    let left = work.left + (work.width() - w) / 2;
    let top = work.top + (work.height() - h) / 2;
    Rc {
        left,
        top,
        right: left + w,
        bottom: top + h,
    }
}

/// A window opened while `newest` is open: `CASCADE_PX` down and right of it,
/// back to the work area's top left when that would leave `work`.
pub fn cascade(newest: Rc, work: Rc) -> Rc {
    let (w, h) = (newest.width(), newest.height());
    let mut left = newest.left + CASCADE_PX;
    let mut top = newest.top + CASCADE_PX;
    if left + w > work.right || top + h > work.bottom {
        left = work.left;
        top = work.top;
    }
    Rc {
        left,
        top,
        right: left + w,
        bottom: top + h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UiFont;
    use crate::i18n::Lang;
    use std::time::Duration;

    const COPY_ROW: [Btn; 5] = [Btn::Copy, Btn::OpenMd, Btn::OpenFolder, Btn::MakeNote, Btn::CopyPrompt];

    #[test]
    fn buttons_for_each_status() {
        // Lines and no job: the five, then Transkrip ulang only with a redo.
        let mut all = COPY_ROW.to_vec();
        assert_eq!(buttons_for(&Status::None, true, false), all);
        all.push(Btn::Redo);
        assert_eq!(buttons_for(&Status::None, true, true), all);
        // A job in the queue or running: Batal, whatever the lines.
        let running = Status::Running { pct: 40, eta: Some(Duration::from_secs(60)) };
        for status in [Status::Queued, Status::Waiting, Status::Held, running] {
            assert_eq!(buttons_for(&status, false, true), [Btn::Cancel], "{status:?}");
            assert_eq!(buttons_for(&status, true, true), [Btn::Cancel], "{status:?}");
        }
        // Failed: Coba lagi and Buka folder.
        let failed = Status::Failed("Disk penuh".into());
        assert_eq!(buttons_for(&failed, false, true), [Btn::Retry, Btn::OpenFolder]);
        // An empty transcript: Transkrip ulang (when possible) and Buka folder.
        assert_eq!(buttons_for(&Status::None, false, true), [Btn::Redo, Btn::OpenFolder]);
        assert_eq!(buttons_for(&Status::None, false, false), [Btn::OpenFolder]);
        // Transcripts off: Buka Pengaturan.
        assert_eq!(buttons_for(&Status::Off, false, false), [Btn::Settings]);
        // The player failing keeps the lines' buttons.
        assert_eq!(buttons_for(&Status::PlayerError, true, false), COPY_ROW);
        // Only Salin + prompt ringkasan is the primary.
        let primary: Vec<Btn> = COPY_ROW.into_iter().filter(|b| b.primary()).collect();
        assert_eq!(primary, [Btn::CopyPrompt]);
    }

    fn inside(r: Rect, w: f32, h: f32) -> bool {
        r.x >= 0.0 && r.y >= 0.0 && r.right() <= w && r.bottom() <= h
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    #[test]
    fn buttons_wrap_at_the_minimum_width_in_both_languages() {
        let mut every = COPY_ROW.to_vec();
        every.push(Btn::Redo);
        for font in [UiFont::Sans, UiFont::Mono] {
            for lang in [Lang::Id, Lang::En] {
                let measured = super::super::window::measured_buttons(font, lang, &every);
                assert!(measured.iter().all(|(_, w)| *w > 2.0 * BTN_PAD_X), "{measured:?}");
                for w in [MIN_W, DEFAULT_W] {
                    let f = frame(w, MIN_H, &measured, true);
                    let rects: Vec<Rect> = f.buttons.iter().map(|(_, r)| *r).collect();
                    assert_eq!(rects.len(), every.len());
                    let mut all = rects.clone();
                    all.extend([f.play, f.position, f.bar, f.length, f.list]);
                    all.extend(f.status);
                    for (i, a) in all.iter().enumerate() {
                        assert!(inside(*a, w, MIN_H), "{font:?} {lang:?} {w}: {a:?} outside");
                        for b in &all[i + 1..] {
                            assert!(!overlaps(*a, *b), "{font:?} {lang:?} {w}: {a:?} over {b:?}");
                        }
                    }
                    // Each button sits inside the margins, BTN_H high.
                    for r in &rects {
                        assert!(r.x >= PAD && r.right() <= w - PAD + 0.01, "{r:?}");
                        assert_eq!(r.h, BTN_H);
                    }
                    // In order, left to right then down.
                    for pair in rects.windows(2) {
                        let (a, b) = (pair[0], pair[1]);
                        assert!(b.y > a.y || (b.y == a.y && b.x >= a.right() + BTN_GAP - 0.01));
                    }
                    // The list keeps room to read.
                    assert!(f.list.h >= 150.0, "{font:?} {lang:?} {w}: list {:?}", f.list);
                }
                // At the minimum width the six wrap to more than one row.
                let f = frame(MIN_W, MIN_H, &measured, false);
                assert!(f.buttons.last().unwrap().1.y > f.buttons[0].1.y, "{font:?} {lang:?}");
            }
        }
    }

    #[test]
    fn the_frame_stacks_header_buttons_player_status_and_list() {
        let f = frame(640.0, 720.0, &[(Btn::Cancel, 70.0)], true);
        assert!(f.header.bottom() <= f.buttons[0].1.y);
        assert!(f.buttons[0].1.bottom() <= f.play.y);
        assert_eq!(f.play.w, PLAY);
        assert_eq!(f.play.h, PLAY);
        // The bar runs between the two clocks.
        assert!(f.position.right() <= f.bar.x && f.bar.right() <= f.length.x);
        assert_eq!(f.length.right(), 640.0 - PAD);
        let status = f.status.unwrap();
        assert_eq!(status.h, STATUS_H);
        assert_eq!(f.list.y, status.bottom());
        assert_eq!(f.list.bottom(), 720.0);
        // Without a status line the list starts under the player.
        let g = frame(640.0, 720.0, &[(Btn::Cancel, 70.0)], false);
        assert_eq!(g.status, None);
        assert_eq!(g.list.y, status.y);
    }

    #[test]
    fn line_tops_and_line_at_agree() {
        let heights = [20.0, 34.0, 20.0, 48.0];
        let tops = line_tops(&heights);
        assert_eq!(tops, [0.0, 20.0, 54.0, 74.0]);
        assert_eq!(content_h(&tops, &heights), 122.0);
        for scroll in [0.0, 10.0, 37.5] {
            for (i, (&t, &h)) in tops.iter().zip(&heights).enumerate() {
                // The first and last dip of each row, in list coordinates.
                for y in [t, t + h - 0.5] {
                    let at = y + LIST_TOP - scroll;
                    assert_eq!(line_at(&tops, &heights, scroll, at), Some(i), "{scroll} {y}");
                }
            }
        }
        // Above the first row and below the last: nothing.
        assert_eq!(line_at(&tops, &heights, 0.0, 2.0), None);
        assert_eq!(line_at(&tops, &heights, 0.0, 122.0 + LIST_TOP), None);
        assert_eq!(line_at(&[], &[], 0.0, 10.0), None);
    }

    #[test]
    fn only_visible_rows_are_laid_out() {
        let heights = vec![20.0; 2_000];
        let tops = line_tops(&heights);
        for scroll in [0.0, 7.0, 10_000.0, 19_990.0, max_scroll(&tops, &heights, 400.0)] {
            let r = visible(&tops, &heights, scroll, 400.0);
            assert!(r.len() <= 21, "{scroll}: {r:?}");
            assert!(!r.is_empty());
            // Every row on screen is in the range.
            for y in [LIST_TOP + 0.5, 200.0, 399.5] {
                if let Some(i) = line_at(&tops, &heights, scroll, y) {
                    assert!(r.contains(&i), "{scroll}: row {i} at {y} not in {r:?}");
                }
            }
        }
        assert_eq!(visible(&[], &[], 0.0, 400.0), 0..0);
    }

    #[test]
    fn follow_scroll_keeps_the_current_line_in_the_band() {
        let heights = vec![30.0; 200];
        let tops = line_tops(&heights);
        let list_h = 400.0;
        let in_band = |i: usize, s: f32| {
            let y = tops[i] + LIST_TOP - s;
            y >= BAND_TOP * list_h - 0.01 && y + heights[i] <= BAND_BOTTOM * list_h + 0.01
        };
        for current in [20, 21, 60, 120, 150] {
            for scroll in [0.0, 300.0, 1_000.0, 3_000.0] {
                let s = follow_scroll(&tops, &heights, current, scroll, list_h);
                assert!(in_band(current, s), "{current} from {scroll}: {s}");
            }
        }
        // A line already in the band does not move the list.
        let s = follow_scroll(&tops, &heights, 20, 500.0, list_h);
        assert_eq!(follow_scroll(&tops, &heights, 21, s, list_h), s);
        // The first lines and the last stay inside the scroll range.
        assert_eq!(follow_scroll(&tops, &heights, 0, 900.0, list_h), 0.0);
        let max = max_scroll(&tops, &heights, list_h);
        assert_eq!(follow_scroll(&tops, &heights, 199, 0.0, list_h), max);
        // A line taller than the band: its top at the band's top.
        let tall = [30.0, 30.0, 30.0, 30.0, 30.0, 300.0, 30.0, 30.0, 30.0, 30.0, 30.0, 30.0];
        let tt = line_tops(&tall);
        let s = follow_scroll(&tt, &tall, 5, 0.0, list_h);
        assert!((tt[5] + LIST_TOP - s - BAND_TOP * list_h).abs() < 0.01, "{s}");
    }

    #[test]
    fn seek_ms_clamps_to_the_bar() {
        let bar = Rect { x: 100.0, y: 0.0, w: 400.0, h: 20.0 };
        assert_eq!(seek_ms(bar, 100.0, 60_000), 0);
        assert_eq!(seek_ms(bar, 300.0, 60_000), 30_000);
        assert_eq!(seek_ms(bar, 500.0, 60_000), 60_000);
        assert_eq!(seek_ms(bar, -50.0, 60_000), 0);
        assert_eq!(seek_ms(bar, 900.0, 60_000), 60_000);
        assert_eq!(seek_ms(bar, 200.0, 0), 0);
        let empty = Rect { w: 0.0, ..bar };
        assert_eq!(seek_ms(empty, 100.0, 60_000), 0);
    }

    fn mon(left: i32, dpi: u32) -> Mon {
        let rect = Rc { left, top: 0, right: left + 1920, bottom: 1080 };
        Mon { rect, work: Rc { bottom: 1040, ..rect }, dpi }
    }

    #[test]
    fn place_falls_back_when_the_saved_rect_is_off_screen() {
        let (a, b) = (mon(0, 96), mon(1920, 144));
        // First open: the default size centred on the fallback's work area.
        let r = place(None, &[a, b], a);
        assert_eq!((r.width(), r.height()), (640, 720));
        assert_eq!((r.left, r.top), ((1920 - 640) / 2, (1040 - 720) / 2));
        // At 150 %: 960 x 1080 does not fit 1040 high, so it shrinks.
        let r = place(None, &[a, b], b);
        assert_eq!((r.width(), r.height()), (960, 1040));
        assert_eq!((r.left, r.top), (1920 + 480, 0));
        // A saved rect on the second monitor is kept.
        let saved = CardPlace { left: 2000, top: 100, right: 2900, bottom: 900 };
        let r = place(Some(saved), &[a, b], a);
        assert_eq!((r.left, r.top, r.right, r.bottom), (2000, 100, 2900, 900));
        // Too small for that monitor's DPI: grown to the minimum, top left kept.
        let tiny = CardPlace { left: 2000, top: 100, right: 2100, bottom: 200 };
        let r = place(Some(tiny), &[a, b], a);
        assert_eq!((r.left, r.top, r.width(), r.height()), (2000, 100, 720, 600));
        // Off every screen, or with less than half on one: the fallback.
        let gone = CardPlace { left: 5000, top: 100, right: 5600, bottom: 800 };
        assert_eq!(place(Some(gone), &[a, b], a), place(None, &[a, b], a));
        let edge = CardPlace { left: -500, top: 100, right: 100, bottom: 800 };
        assert_eq!(place(Some(edge), &[a, b], a), place(None, &[a, b], a));
        // The monitor of the saved rect was unplugged.
        let r = place(Some(saved), &[a], a);
        assert_eq!(r, place(None, &[a], a));
    }

    #[test]
    fn a_second_window_cascades_and_wraps_inside_the_work_area() {
        let work = Rc { left: 0, top: 0, right: 1920, bottom: 1040 };
        let first = Rc { left: 100, top: 100, right: 740, bottom: 820 };
        let second = cascade(first, work);
        assert_eq!((second.left, second.top), (124, 124));
        assert_eq!((second.width(), second.height()), (640, 720));
        let low = Rc { left: 100, top: 300, right: 740, bottom: 1030 };
        assert_eq!((cascade(low, work).left, cascade(low, work).top), (0, 0));
    }

    mod tips {
        use super::*;
        use crate::i18n::{self, Lang};
        use crate::ui::tooltip::KeyHint;

        const LANGS: [Lang; 2] = [Lang::Id, Lang::En];
        const WITH_TIP: [Btn; 6] = [
            Btn::Copy,
            Btn::OpenMd,
            Btn::MakeNote,
            Btn::CopyPrompt,
            Btn::Redo,
            Btn::Settings,
        ];

        #[test]
        fn every_icon_only_control_and_the_described_buttons_have_a_tip() {
            for lang in LANGS {
                i18n::set_lang(lang);
                for playing in [false, true] {
                    assert!(tip_of(Hot::Play, playing, None).is_some());
                }
                assert!(tip_of(Hot::Bar, false, None).is_some());
                for b in WITH_TIP {
                    let tip = tip_of(Hot::Btn(0), false, Some(b)).unwrap();
                    assert!(tip.name.is_none(), "{b:?} is the short form");
                    assert!(tip.about.is_some_and(|a| !a.is_empty()), "{lang:?} {b:?}");
                }
            }
        }

        #[test]
        fn the_text_rows_and_the_self_explaining_buttons_have_none() {
            assert!(tip_of(Hot::Line(0), false, None).is_none());
            assert!(tip_of(Hot::Btn(0), false, None).is_none());
            for b in [Btn::OpenFolder, Btn::Cancel, Btn::Retry] {
                assert!(tip_of(Hot::Btn(0), false, Some(b)).is_none(), "{b:?}");
            }
        }

        #[test]
        fn the_play_name_follows_the_player() {
            for lang in LANGS {
                i18n::set_lang(lang);
                let play = tip_of(Hot::Play, false, None).unwrap();
                let pause = tip_of(Hot::Play, true, None).unwrap();
                assert_ne!(play.name, pause.name, "{lang:?}");
                assert_eq!(play.keys, [KeyHint::Fixed(Key::TipKeySpace)]);
                assert_eq!(pause.keys, [KeyHint::Fixed(Key::TipKeySpace)]);
            }
        }

        #[test]
        fn the_bar_names_its_arrow_keys() {
            let bar = tip_of(Hot::Bar, false, None).unwrap();
            assert_eq!(bar.keys, [KeyHint::Raw("\u{2190} \u{2192}")]);
            assert!(bar.name.is_some() && bar.about.is_some());
        }

        #[test]
        fn ctrl_c_is_on_the_button_it_triggers() {
            for b in WITH_TIP {
                let keys = tip_of(Hot::Btn(0), false, Some(b)).unwrap().keys;
                if b == Btn::Copy {
                    assert_eq!(keys, [KeyHint::Raw("Ctrl+C")]);
                } else {
                    assert!(keys.is_empty(), "{b:?}");
                }
            }
        }

        #[test]
        fn no_text_has_an_em_dash() {
            for lang in LANGS {
                i18n::set_lang(lang);
                let mut all = vec![
                    tip_of(Hot::Play, false, None),
                    tip_of(Hot::Play, true, None),
                    tip_of(Hot::Bar, false, None),
                ];
                all.extend(WITH_TIP.map(|b| tip_of(Hot::Btn(0), false, Some(b))));
                for tip in all.into_iter().flatten() {
                    for s in tip.name.iter().chain(tip.about.iter()) {
                        assert!(!s.contains('\u{2014}') && !s.is_empty(), "{lang:?}: {s}");
                    }
                }
            }
        }
    }
}
