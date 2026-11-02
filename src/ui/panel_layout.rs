//! Layout of the open panel's pages, the waiting strip, the page dots and the
//! compact pomodoro, in window logical pixels. Pure and deterministic: text is
//! trimmed by the renderer inside the rects given here, and the one width that
//! depends on text (the calendar title, which the arrows follow) is passed in.

use std::collections::HashMap;

use chrono::{Days, NaiveDate, NaiveDateTime, NaiveTime, Timelike};

use super::geometry::{ExpandedLayout, Rect, ShapeRect};
use crate::calendar::{CalEvent, CalStatus, EventTime, events_on, range_for};
use crate::clean::{CleanCat, CleanMode};
use crate::cli_folders::{MenuItem, Section};
use crate::devices::{Device, Veto};
use crate::model::{CalView, Hit, Page, Shortcut};
use crate::mole::{MoleGroup, MoleOpt};
use crate::notes::Note;
use crate::reminders::{Reminder, local_time};
use crate::tiles::Tile;

/// A clickable rect and what clicking it means.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub rect: Rect,
    pub hit: Hit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CalendarLayout {
    pub view: CalView,
    pub selected: NaiveDate,
    /// Month name, week range or day title, as wide as the title drawn.
    pub header: Rect,
    /// Hari, Minggu, Bulan.
    pub view_buttons: [(CalView, Rect); 3],
    /// Right after the title.
    pub prev: Rect,
    pub next: Rect,
    /// Month view: one rect above each grid column for its weekday name.
    pub weekday_labels: Vec<Rect>,
    /// Month view: 42 days, Monday first.
    pub month_cells: Vec<(NaiveDate, Rect)>,
    /// Week view: 7 columns, Monday first; the top `WEEK_HEAD_H` holds the
    /// clickable day name and number.
    pub week_columns: Vec<(NaiveDate, Rect)>,
    /// Week view: "+N" at a column's foot, with the hidden count.
    pub week_more: Vec<(NaiveDate, usize, Rect)>,
    /// Month: the event list right of the grid. Week: the columns. Day: the
    /// timeline (its first hour line at the top).
    pub list: Rect,
    /// Month view: the selected day's caption at the top of the list.
    pub caption: Option<Rect>,
    /// Event id and row (month list), chip (week) or all-day band and
    /// time block (day).
    pub rows: Vec<(String, Rect)>,
    /// Month and day: events after the shown ones and the "+N lagi" rect;
    /// in the day view a count of 0 means the last page ("Kembali ke atas").
    pub more: Option<(usize, Rect)>,
    /// Day view: the hour lines shown, each hour with its line's y.
    pub hours: Vec<(u32, f32)>,
    /// Day view of today: the "now" line's y, when inside the hours shown.
    pub now_y: Option<f32>,
    /// "+ Tambah acara".
    pub add: Rect,
    /// "Hubungkan Google" / "Hubungkan ulang Google".
    pub connect: Option<Rect>,
    /// The "Offline" tag after the arrows, while the calendar is offline; the
    /// title is cut shorter so it always fits.
    pub offline: Option<Rect>,
}

/// The timer and its controls only; durations are set in Settings → Fitur → Pomodoro.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PomodoroLayout {
    /// "Pomodoro".
    pub header: Rect,
    /// "25 · 5 · 15 menit", right of the title.
    pub durations: Rect,
    pub ring_center: (f32, f32),
    pub ring_radius: f32,
    /// The countdown, a little above the ring's centre: as wide as the
    /// stroke's inner edge allows at the digits' top and bottom, so text
    /// centred in it never meets it.
    pub time: Rect,
    /// The phase in capitals ("FOKUS"), under the countdown inside the ring.
    pub phase: Rect,
    /// "Sesi 2 dari 4" and its dots, right of the ring.
    pub session: Rect,
    /// The big round primary button: start, pause or resume.
    pub start_pause: Rect,
    /// The small round buttons either side of it.
    pub reset: Rect,
    pub skip: Rect,
    /// "Berikutnya: Istirahat 5 menit".
    pub next_up: Rect,
}

/// The notes and reminders pages: a title, "+ Baru" and the items.
#[derive(Clone, Debug, PartialEq)]
pub struct ListLayout {
    /// "Notes" / "Pengingat" title.
    pub header: Rect,
    /// "+ Baru".
    pub new_button: Rect,
    /// Reminders: each day group's heading in this chunk, by the day it
    /// stands for ("Hari ini", "Besok", a date). Empty for notes.
    pub groups: Vec<(NaiveDate, Rect)>,
    /// Note id and card, or reminder id and row.
    pub rows: Vec<(String, Rect)>,
    /// Items after this chunk and the clickable rect: "+N lagi", or
    /// "Kembali ke atas" when the count is 0 (last chunk).
    pub more: Option<(usize, Rect)>,
}

/// The CLI page's header chips as the renderer measured them: each chip's
/// full width, left to right (the folder gives way when room is short), and
/// the Stop button's while a command runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CliHead {
    pub chips: Vec<f32>,
    pub stop: Option<f32>,
    /// The first chip is the accent "dari Explorer": the folder chip follows
    /// it, and it is dropped (width 0) before the folder is cut below
    /// `CLI_FOLDER_MIN`.
    pub explorer: bool,
    /// The folder chip (and "dari Explorer") open the folder menu: not
    /// while a command runs, when the first chip is the run's state.
    pub folder_menu: bool,
}

/// The CLI page: the title and header chips, then the terminal: output
/// filling from the bottom over the input line.
#[derive(Clone, Debug, PartialEq)]
pub struct CliLayout {
    /// "CLI".
    pub header: Rect,
    /// Folder, shell or the run's state, left to right as asked; a chip
    /// dropped for room is 0 wide.
    pub chips: Vec<Rect>,
    /// How many of the first chips open the folder menu.
    pub folder_chips: usize,
    /// While a command runs, right of the chips.
    pub stop: Option<Rect>,
    /// The enlarge / shrink button, top right.
    pub expand: Rect,
    /// The dark terminal under the header.
    pub term: Rect,
    /// Where output lines are drawn, the newest at the bottom.
    pub output: Rect,
    /// Output lines that fit, `CLI_LINE_H` apart.
    pub rows: usize,
    /// The input line along the terminal's bottom.
    pub input: Rect,
    /// The `›` prompt, then the typed text up to the hint.
    pub prompt: Rect,
    pub field: Rect,
    /// The open folder menu (`cli_folder_menu`), set by the renderer.
    pub menu: Option<MenuLayout>,
}

/// The CLI page's folder menu: a raised box under the header's right side.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuLayout {
    pub boxed: Rect,
    /// The lines that fit: their index in the items given, and their rect.
    pub rows: Vec<(usize, Rect)>,
    /// A folder row's star button (Explorer and recent rows): item index
    /// and rect, at the row's right end.
    pub stars: Vec<(usize, Rect)>,
    /// Picks and stars, for `regions`.
    pub hits: Vec<Region>,
}

/// The CLI output's line pitch: DM Mono 11 at the design's 1.62.
pub const CLI_LINE_H: f32 = 17.8;
/// The terminal's corners.
pub const CLI_TERM_RADIUS: f32 = 16.0;
/// A suggestion row's height.
pub const CLI_SUGGEST_ROW: f32 = 30.0;

/// The Pintasan page: up to three rows of four tiles.
#[derive(Clone, Debug, PartialEq)]
pub struct ShortcutsLayout {
    /// "Pintasan".
    pub header: Rect,
    /// In the order the page shows them, row by row: the built-in tile it
    /// is, or `None` for a custom one (its index here is its `Hit`).
    pub tiles: Vec<(Option<Shortcut>, Rect)>,
}

/// The Shelf page: up to twelve cards, four to a row, under a header with
/// "Kosongkan".
#[derive(Clone, Debug, PartialEq)]
pub struct ShelfLayout {
    pub header: Rect,
    /// "Kosongkan"; only with something on the shelf.
    pub clear: Option<Rect>,
    /// Everything under the header: where files are dropped, outlined while
    /// a drag hovers.
    pub zone: Rect,
    /// The hint of an empty shelf (the whole zone).
    pub empty: Option<Rect>,
    /// Lines a card's name gets: two, or one with three rows of cards.
    pub lines: usize,
    pub cards: Vec<ShelfCardLayout>,
}

/// A card, the X on its corner (a click takes the file off the shelf), its
/// icon and its name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShelfCardLayout {
    pub card: Rect,
    pub remove: Rect,
    pub glyph: Rect,
    pub label: Rect,
    /// Where "hilang" sits on a card whose file is gone.
    pub missing: Rect,
}

const SHELF_CLEAR_W: f32 = 84.0;
const SHELF_X: f32 = 18.0;
const SHELF_GLYPH_H: f32 = 28.0;
const SHELF_LINE_H: f32 = 13.5;
const SHELF_GAP: f32 = 6.0;

/// The Mixer page: a row per volume.
#[derive(Clone, Debug, PartialEq)]
pub struct MixerLayout {
    /// "Mixer".
    pub header: Rect,
    pub rows: Vec<MixerRowLayout>,
}

/// One row: the speaker (a click mutes), the name, the slider (its track is
/// what the level is read from) and the percent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixerRowLayout {
    pub icon: Rect,
    pub name: Rect,
    /// The slider's hit area; `MIXER_TRACK_H` of it is drawn.
    pub track: Rect,
    pub value: Rect,
}

/// The drawn track's thickness, in the middle of its taller hit area.
pub const MIXER_TRACK_H: f32 = 6.0;
const MIXER_ROW_MAX_H: f32 = 36.0;
const MIXER_ICON: f32 = 22.0;
const MIXER_NAME_W: f32 = 112.0;
const MIXER_VALUE_W: f32 = 40.0;
const MIXER_GAP: f32 = 10.0;

/// The Devices page's header as measured: the title, the "N tersambung"
/// chip and the "Eject semua" button, each with its padding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DevicesHead {
    pub title_w: f32,
    pub chip_w: f32,
    pub eject_all_w: f32,
}

/// What a card's height depends on: a refusal's row, and the details
/// (with the number of partitions they list).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceCardShape {
    pub veto: bool,
    pub details: Option<usize>,
}

/// The Devices page: a header, then a card per device in a list that
/// scrolls when it does not fit.
#[derive(Clone, Debug, PartialEq)]
pub struct DevicesLayout {
    /// "Perangkat".
    pub header: Rect,
    /// "3 tersambung", right after the title.
    pub chip: Option<Rect>,
    /// "Eject semua", at the right; only with devices listed.
    pub eject_all: Option<Rect>,
    /// Where the cards are (and are clipped to); the wheel over it scrolls
    /// them while they overflow.
    pub list: Rect,
    pub cards: Vec<DeviceCardLayout>,
    /// Nothing is plugged in: the message's area.
    pub empty: Option<Rect>,
    /// The scroll applied in px, kept in range, and the furthest it goes.
    pub scroll: f32,
    pub max_scroll: f32,
    /// The thin thumb beside the list while it scrolls.
    pub scrollbar: Option<Rect>,
}

/// One device card. Rects are where the card sits scrolled, so some may lie
/// outside `DevicesLayout::list` (clipped when drawn, cut in `regions`).
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceCardLayout {
    pub card: Rect,
    /// The 34 px icon well.
    pub well: Rect,
    pub title: Rect,
    pub subtitle: Rect,
    /// The folder, info and Eject buttons.
    pub open: Rect,
    pub detail: Rect,
    pub eject: Rect,
    /// The usage bar and its "used / total" at its right.
    pub bar: Rect,
    pub usage: Rect,
    /// The detail rows, each a label and a value rect (Model, Jalur, Sistem
    /// file, then one per partition).
    pub rows: Vec<(Rect, Rect)>,
    pub veto: Option<DeviceVetoLayout>,
}

/// The red row of a refused eject: the whole row, its text and "Coba lagi".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceVetoLayout {
    pub row: Rect,
    pub text: Rect,
    pub retry: Rect,
}

const DEV_HEAD_H: f32 = 24.0;
const DEV_HEAD_GAP: f32 = 6.0;
const DEV_CHIP_H: f32 = 20.0;
const DEV_CARD_GAP: f32 = 5.0;
const DEV_PAD_X: f32 = 10.0;
const DEV_PAD_Y: f32 = 5.0;
/// The icon well, as in the design's `dev()`.
pub const DEV_WELL: f32 = 34.0;
const DEV_BAR_GAP: f32 = 4.0;
const DEV_BAR_ROW: f32 = 12.0;
/// The usage bar's thickness.
pub const DEV_BAR_H: f32 = 5.0;
const DEV_USAGE_W: f32 = 118.0;
const DEV_BTN_H: f32 = 26.0;
const DEV_ICON_BTN_W: f32 = 28.0;
const DEV_EJECT_W: f32 = 84.0;
const DEV_BTN_GAP: f32 = 6.0;
const DEV_VETO_GAP: f32 = 4.0;
const DEV_VETO_H: f32 = 24.0;
const DEV_VETO_PAD: f32 = 9.0;
const DEV_RETRY_W: f32 = 66.0;
const DEV_RETRY_H: f32 = 18.0;
const DEV_DETAIL_GAP: f32 = 6.0;
const DEV_DETAIL_ROW: f32 = 15.0;
const DEV_DETAIL_LABEL_W: f32 = 78.0;
/// A wheel notch scrolls the list this many px.
pub const DEV_WHEEL_PX: f32 = 40.0;

/// A card's height: the well's row and the bar's, the details when it is
/// expanded (3 rows and one per partition), the refusal's row.
pub fn device_card_h(shape: DeviceCardShape) -> f32 {
    let mut h = DEV_PAD_Y + DEV_WELL + DEV_BAR_GAP + DEV_BAR_ROW + DEV_PAD_Y;
    if let Some(parts) = shape.details {
        h += DEV_DETAIL_GAP + (3 + parts) as f32 * DEV_DETAIL_ROW;
    }
    if shape.veto {
        h += DEV_VETO_GAP + DEV_VETO_H;
    }
    h
}

/// The shape of each card for `list`: a row for a refusal kept in `vetoes`,
/// the details for the device whose key is `expanded`.
pub fn device_shapes(
    list: &[Device],
    vetoes: &HashMap<String, Veto>,
    expanded: Option<&str>,
) -> Vec<DeviceCardShape> {
    list.iter()
        .map(|d| DeviceCardShape {
            veto: vetoes.contains_key(&d.key),
            details: (expanded == Some(d.key.as_str())).then_some(d.partitions.len()),
        })
        .collect()
}

/// All the cards with their gaps.
fn devices_content_h(shapes: &[DeviceCardShape]) -> f32 {
    let cards: f32 = shapes.iter().map(|s| device_card_h(*s)).sum();
    cards + shapes.len().saturating_sub(1) as f32 * DEV_CARD_GAP
}

/// Where the Devices page's cards show (under the header).
pub fn devices_list_rect(s: ShapeRect, strip: bool) -> Rect {
    let top = content_top(s, strip) + DEV_HEAD_H + DEV_HEAD_GAP;
    let (l, r) = (left(s), right(s));
    rect(l, top, r - l, content_bottom(s) - top)
}

/// The furthest the cards scroll (px); 0 when they all fit.
pub fn devices_max_scroll(s: ShapeRect, strip: bool, shapes: &[DeviceCardShape]) -> f32 {
    (devices_content_h(shapes) - devices_list_rect(s, strip).h).max(0.0)
}

/// The scroll that brings card `index` into view: unchanged while it shows
/// whole, else just enough (its top first when it is taller than the list).
pub fn devices_reveal(
    s: ShapeRect,
    strip: bool,
    shapes: &[DeviceCardShape],
    scroll: f32,
    index: usize,
) -> f32 {
    let max = devices_max_scroll(s, strip, shapes);
    let view = devices_list_rect(s, strip).h;
    let top: f32 = shapes
        .iter()
        .take(index)
        .map(|s| device_card_h(*s) + DEV_CARD_GAP)
        .sum();
    let bottom = top + shapes.get(index).map_or(0.0, |s| device_card_h(*s));
    let scroll = scroll.clamp(0.0, max);
    let wanted = if top < scroll || bottom - top > view {
        top
    } else if bottom > scroll + view {
        bottom - view
    } else {
        scroll
    };
    wanted.clamp(0.0, max)
}

/// The Devices page for `shapes` (one per device), scrolled `scroll` px.
/// Three cards, one with a refusal's row, fit without scrolling.
pub fn devices_layout(
    s: ShapeRect,
    strip: bool,
    head: &DevicesHead,
    shapes: &[DeviceCardShape],
    scroll: f32,
) -> DevicesLayout {
    let top = content_top(s, strip);
    let (l, r) = (left(s), right(s));
    let n = shapes.len();
    let eject_all = (n > 0).then(|| {
        rect(
            header_right(s) - head.eject_all_w,
            top,
            head.eject_all_w,
            DEV_HEAD_H,
        )
    });
    let header = rect(l, top, head.title_w, HEADER_H);
    let chip = (n > 0).then(|| {
        rect(
            header.right() + 8.0,
            top + (HEADER_H - DEV_CHIP_H) / 2.0,
            head.chip_w,
            DEV_CHIP_H,
        )
    });
    let list = devices_list_rect(s, strip);
    let max_scroll = devices_max_scroll(s, strip, shapes);
    let scroll = scroll.clamp(0.0, max_scroll);
    let mut y = list.y - scroll;
    let cards = shapes
        .iter()
        .map(|shape| {
            let h = device_card_h(*shape);
            let card = rect(l, y, r - l, h);
            y += h + DEV_CARD_GAP;
            let (il, ir) = (card.x + DEV_PAD_X, card.right() - DEV_PAD_X);
            let y0 = card.y + DEV_PAD_Y;
            let well = rect(il, y0, DEV_WELL, DEV_WELL);
            let cy = y0 + DEV_WELL / 2.0;
            let button = |x: f32, w: f32| rect(x, cy - DEV_BTN_H / 2.0, w, DEV_BTN_H);
            let eject = button(ir - DEV_EJECT_W, DEV_EJECT_W);
            let detail = button(eject.x - DEV_BTN_GAP - DEV_ICON_BTN_W, DEV_ICON_BTN_W);
            let open = button(detail.x - DEV_BTN_GAP - DEV_ICON_BTN_W, DEV_ICON_BTN_W);
            let text_x = well.right() + 10.0;
            let text_w = open.x - 8.0 - text_x;
            let title = rect(text_x, y0 + 1.0, text_w, 16.0);
            let subtitle = rect(text_x, y0 + 18.0, text_w, 14.0);
            let row_y = y0 + DEV_WELL + DEV_BAR_GAP;
            let usage = rect(ir - DEV_USAGE_W, row_y, DEV_USAGE_W, DEV_BAR_ROW);
            let bar = rect(
                il,
                row_y + (DEV_BAR_ROW - DEV_BAR_H) / 2.0,
                usage.x - 8.0 - il,
                DEV_BAR_H,
            );
            let mut next = row_y + DEV_BAR_ROW;
            let mut rows = Vec::new();
            if let Some(parts) = shape.details {
                next += DEV_DETAIL_GAP;
                for i in 0..3 + parts {
                    let ry = next + i as f32 * DEV_DETAIL_ROW;
                    let label = rect(il, ry, DEV_DETAIL_LABEL_W, DEV_DETAIL_ROW);
                    // The partition lines have no label: the value takes the row.
                    let (vx, vw) = if i < 3 {
                        (label.right() + 6.0, ir - label.right() - 6.0)
                    } else {
                        (il, ir - il)
                    };
                    rows.push((label, rect(vx, ry, vw, DEV_DETAIL_ROW)));
                }
                next += (3 + parts) as f32 * DEV_DETAIL_ROW;
            }
            let veto = shape.veto.then(|| {
                let row = rect(il, next + DEV_VETO_GAP, ir - il, DEV_VETO_H);
                let retry = rect(
                    row.right() - 6.0 - DEV_RETRY_W,
                    row.y + (DEV_VETO_H - DEV_RETRY_H) / 2.0,
                    DEV_RETRY_W,
                    DEV_RETRY_H,
                );
                let text = rect(
                    row.x + DEV_VETO_PAD,
                    row.y,
                    retry.x - 8.0 - row.x - DEV_VETO_PAD,
                    DEV_VETO_H,
                );
                DeviceVetoLayout { row, text, retry }
            });
            DeviceCardLayout {
                card,
                well,
                title,
                subtitle,
                open,
                detail,
                eject,
                bar,
                usage,
                rows,
                veto,
            }
        })
        .collect();
    let scrollbar = (max_scroll > 0.0).then(|| {
        let total = list.h + max_scroll;
        let h = (list.h * list.h / total).clamp(24.0, list.h);
        let y = list.y + (list.h - h) * scroll / max_scroll;
        rect(list.right() + 6.0, y, CLIP_SCROLLBAR_W, h)
    });
    DevicesLayout {
        header,
        chip,
        eject_all,
        list,
        cards,
        empty: (n == 0).then_some(list),
        scroll,
        max_scroll,
        scrollbar,
    }
}

// ------------------------------------------------------------------ Susun

/// The Susun page's header as measured: the title ("Susun", or "Ubin 2 dari
/// 3" while picking), each monitor chip and "Lewati ubin".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArrangeHead {
    pub title_w: f32,
    pub chip_w: Vec<f32>,
    pub skip_w: f32,
}

/// What the Susun page's content depends on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArrangeShape {
    /// The pick step (the window list) instead of the templates.
    pub picking: bool,
    /// Template cards: the built-ins and the user's.
    pub cards: usize,
    /// Saved arrangements.
    pub saved: usize,
    /// Windows in the pick list.
    pub windows: usize,
}

pub const ARR_CARD_W: f32 = 80.0;
pub const ARR_CARD_H: f32 = 64.0;
const ARR_GAP: f32 = 8.0;
const ARR_PER_ROW: usize = 6;
const ARR_HEAD_H: f32 = 22.0;
const ARR_HEAD_GAP: f32 = 8.0;
const ARR_CHIP_H: f32 = 22.0;
const ARR_SAVED_TITLE_H: f32 = 16.0;
const ARR_SAVED_ROW_H: f32 = 38.0;
const ARR_WIN_ROW_H: f32 = 30.0;
const ARR_HINT_H: f32 = 14.0;
const ARR_PLAY: f32 = 26.0;
const ARR_SKIP_H: f32 = 24.0;
/// The wheel's step over the Susun lists, per notch.
pub const ARR_WHEEL_PX: f32 = 40.0;

/// One template card: the card, its mini preview and its name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrangeCardLayout {
    pub card: Rect,
    pub preview: Rect,
    pub name: Rect,
}

/// One saved arrangement: a row with a mini preview, the name over the apps,
/// and a play button.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrangeSavedLayout {
    pub row: Rect,
    pub preview: Rect,
    pub name: Rect,
    pub apps: Rect,
    pub play: Rect,
}

/// One window of the pick list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrangeWindowLayout {
    pub row: Rect,
    pub icon: Rect,
    pub title: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeLayout {
    pub header: Rect,
    /// One per monitor, after the title (only with two or more monitors).
    pub chips: Vec<Rect>,
    /// "Lewati ubin" while picking.
    pub skip: Option<Rect>,
    /// Where the content shows and is clipped to; the wheel over it scrolls.
    pub list: Rect,
    pub cards: Vec<ArrangeCardLayout>,
    pub saved_title: Option<Rect>,
    pub saved: Vec<ArrangeSavedLayout>,
    pub windows: Vec<ArrangeWindowLayout>,
    /// "Backspace kembali · Esc batal", under the pick list.
    pub hint: Option<Rect>,
    /// Nothing to pick from: the message's area.
    pub empty: Option<Rect>,
    pub scroll: f32,
    pub max_scroll: f32,
    pub scrollbar: Option<Rect>,
}

fn arrange_rows(n: usize) -> usize {
    n.div_ceil(ARR_PER_ROW)
}

/// The content's height in px (what scrolls).
pub fn arrange_content_h(shape: ArrangeShape) -> f32 {
    if shape.picking {
        let n = shape.windows;
        return if n == 0 {
            0.0
        } else {
            n as f32 * (ARR_WIN_ROW_H + 4.0) - 4.0
        };
    }
    let rows = arrange_rows(shape.cards);
    let mut h = if rows == 0 {
        0.0
    } else {
        rows as f32 * (ARR_CARD_H + ARR_GAP) - ARR_GAP
    };
    if shape.saved > 0 {
        h += ARR_GAP + ARR_SAVED_TITLE_H + 4.0;
        h += shape.saved as f32 * (ARR_SAVED_ROW_H + 4.0) - 4.0;
    }
    h
}

/// Where the content shows (under the header; over the hint while picking).
pub fn arrange_list_rect(s: ShapeRect, strip: bool, picking: bool) -> Rect {
    let top = content_top(s, strip) + ARR_HEAD_H + ARR_HEAD_GAP;
    let (l, r) = (left(s), right(s));
    let bottom = content_bottom(s) - if picking { ARR_HINT_H + 4.0 } else { 0.0 };
    rect(l, top, r - l, bottom - top)
}

/// The furthest the content scrolls (px); 0 when it all fits.
pub fn arrange_max_scroll(s: ShapeRect, strip: bool, shape: ArrangeShape) -> f32 {
    (arrange_content_h(shape) - arrange_list_rect(s, strip, shape.picking).h).max(0.0)
}

/// The Susun page: a header (title, monitor chips or "Lewati ubin"), then
/// either the template cards over the saved arrangements, or the window list
/// with a hint under it; the content scrolls when it does not fit.
pub fn arrange_layout(
    s: ShapeRect,
    strip: bool,
    head: &ArrangeHead,
    shape: ArrangeShape,
    scroll: f32,
) -> ArrangeLayout {
    let top = content_top(s, strip);
    let (l, r) = (left(s), right(s));
    let header = rect(l, top, head.title_w, HEADER_H);
    let skip = shape.picking.then(|| {
        rect(
            header_right(s) - head.skip_w,
            top - 1.0,
            head.skip_w,
            ARR_SKIP_H,
        )
    });
    // The chips stay left of the Skip button (and so of the Settings button).
    let chips_end = skip.map_or(header_right(s), |k| k.x - 8.0);
    let mut chips = Vec::new();
    let mut x = header.right() + 12.0;
    for w in &head.chip_w {
        let w = w.min((chips_end - x).max(0.0));
        chips.push(rect(x, top + (HEADER_H - ARR_CHIP_H) / 2.0, w, ARR_CHIP_H));
        x += w + 6.0;
    }
    let list = arrange_list_rect(s, strip, shape.picking);
    let max_scroll = arrange_max_scroll(s, strip, shape);
    let scroll = scroll.clamp(0.0, max_scroll);
    let mut y = list.y - scroll;
    let mut cards = Vec::new();
    let mut saved_title = None;
    let mut saved = Vec::new();
    let mut windows = Vec::new();
    if shape.picking {
        for _ in 0..shape.windows {
            let row = rect(l, y, r - l, ARR_WIN_ROW_H);
            windows.push(ArrangeWindowLayout {
                row,
                icon: rect(row.x + 10.0, row.y + (row.h - 18.0) / 2.0, 18.0, 18.0),
                title: rect(row.x + 36.0, row.y, row.w - 46.0, row.h),
            });
            y += ARR_WIN_ROW_H + 4.0;
        }
    } else {
        for i in 0..shape.cards {
            let (col, row) = (i % ARR_PER_ROW, i / ARR_PER_ROW);
            let card = rect(
                l + col as f32 * (ARR_CARD_W + ARR_GAP),
                y + row as f32 * (ARR_CARD_H + ARR_GAP),
                ARR_CARD_W,
                ARR_CARD_H,
            );
            cards.push(ArrangeCardLayout {
                card,
                preview: rect(card.x + 8.0, card.y + 7.0, card.w - 16.0, 36.0),
                name: rect(card.x + 4.0, card.y + 45.0, card.w - 8.0, 14.0),
            });
        }
        y += arrange_rows(shape.cards) as f32 * (ARR_CARD_H + ARR_GAP);
        if shape.saved > 0 {
            saved_title = Some(rect(l, y, r - l, ARR_SAVED_TITLE_H));
            y += ARR_SAVED_TITLE_H + 4.0;
            for _ in 0..shape.saved {
                let row = rect(l, y, r - l, ARR_SAVED_ROW_H);
                let play = rect(
                    row.right() - 8.0 - ARR_PLAY,
                    row.y + (row.h - ARR_PLAY) / 2.0,
                    ARR_PLAY,
                    ARR_PLAY,
                );
                let preview = rect(row.x + 8.0, row.y + 6.0, 44.0, 26.0);
                let text_x = preview.right() + 10.0;
                let text_w = play.x - 8.0 - text_x;
                saved.push(ArrangeSavedLayout {
                    row,
                    preview,
                    name: rect(text_x, row.y + 4.0, text_w, 16.0),
                    apps: rect(text_x, row.y + 20.0, text_w, 14.0),
                    play,
                });
                y += ARR_SAVED_ROW_H + 4.0;
            }
        }
    }
    let scrollbar = (max_scroll > 0.0).then(|| {
        let total = list.h + max_scroll;
        let h = (list.h * list.h / total).clamp(24.0, list.h);
        let y = list.y + (list.h - h) * scroll / max_scroll;
        rect(list.right() + 6.0, y, CLIP_SCROLLBAR_W, h)
    });
    ArrangeLayout {
        header,
        chips,
        skip,
        list,
        cards,
        saved_title,
        saved,
        hint: shape
            .picking
            .then(|| rect(l, list.bottom() + 4.0, r - l, ARR_HINT_H)),
        empty: (shape.picking && shape.windows == 0).then_some(list),
        windows,
        scroll,
        max_scroll,
        scrollbar,
    }
}

/// `r` cut to `clip`; `None` when nothing of it is inside.
fn clipped(r: Rect, clip: Rect) -> Option<Rect> {
    let (x, y) = (r.x.max(clip.x), r.y.max(clip.y));
    let (right, bottom) = (r.right().min(clip.right()), r.bottom().min(clip.bottom()));
    (right > x && bottom > y).then(|| rect(x, y, right - x, bottom - y))
}

/// The quick search: the field on top, the result rows under it.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchLayout {
    /// The back button at the left of the field (closes the search, as Esc).
    pub back: Rect,
    pub field: Rect,
    /// One per result, top to bottom.
    pub rows: Vec<Rect>,
    /// Nothing to list: the message's area under the field.
    pub empty: Option<Rect>,
}

/// The search field's height, the rows' height and gap.
pub const SEARCH_FIELD_H: f32 = 32.0;
pub const SEARCH_ROW_H: f32 = 28.0;
const SEARCH_ROW_GAP: f32 = 4.0;
const SEARCH_FIELD_GAP: f32 = 8.0;
/// The back button's width (its height is the field's) and the gap to the
/// field.
pub const SEARCH_BACK_W: f32 = 32.0;
const SEARCH_BACK_GAP: f32 = 8.0;

/// A button on the selected Clipboard card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipButton {
    Pin,
    /// "Salin sebagai teks biasa" (not on an image card).
    Plain,
    Delete,
}

/// The Clipboard page as in keputusan-v3.1.html (`.search`, `.clips`,
/// `.clip`): the title and search field, then cards in two columns, a
/// whole row of cards per scroll step.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipLayout {
    /// "Clipboard".
    pub header: Rect,
    /// "Cari di N item…", right of the title.
    pub search: Rect,
    /// Where the cards are; the wheel over it scrolls them.
    pub list: Rect,
    /// Rows of cards that fit.
    pub rows: usize,
    /// The scroll applied (rows), kept in range.
    pub scroll: usize,
    /// Entry id, its index in the list given, and the card.
    pub cards: Vec<(u64, usize, Rect)>,
    /// The selected card's buttons, right to left from its top-right corner.
    pub buttons: Vec<(ClipButton, u64, Rect)>,
    /// Nothing to show (empty, no match, history off): the message's area.
    pub empty: Option<Rect>,
    /// The thin thumb beside the cards when they scroll: where in the list
    /// the view is.
    pub scrollbar: Option<Rect>,
}

/// The Clipboard cards' height and gap (`.clip`, `.clips`).
pub const CLIP_CARD_H: f32 = 64.0;
pub const CLIP_GAP: f32 = 7.0;
pub const CLIP_RADIUS: f32 = 13.0;
/// An image card's thumbnail strip.
pub const CLIP_THUMB_W: f32 = 92.0;
pub const CLIP_BUTTON: f32 = 20.0;
const CLIP_SEARCH_MAX_W: f32 = 250.0;
const CLIP_SEARCH_H: f32 = 28.0;
const CLIP_LIST_GAP: f32 = 9.0;
const CLIP_BUTTON_GAP: f32 = 2.0;

/// One page's layout, for `regions`.
// Built on the stack for one frame and dropped; boxing the calendar would
// only add an allocation per frame.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum PageLayout {
    Calendar(CalendarLayout),
    Pomodoro(PomodoroLayout),
    Reminders(ListLayout),
    Notes(ListLayout),
    Media(ExpandedLayout),
    Clipboard(ClipLayout),
    Notifications(NotifLayout),
    System(SystemLayout),
    /// The cleaner over the System page.
    Cleaner(CleanerLayout),
    /// The cleaner in its "Otomatis · Mole" mode.
    Mole(MoleLayout),
    Cli(CliLayout),
    Shortcuts(ShortcutsLayout),
    Mixer(MixerLayout),
    Search(SearchLayout),
    Shelf(ShelfLayout),
    Devices(DevicesLayout),
    Arrange(ArrangeLayout),
}

/// The cleaner behind "Bersihkan" exists (Run 5): the button is the accent
/// `.cbtn` and opens it. The dimmed "segera" look is kept for a build
/// without it.
pub const CLEANER_READY: bool = true;

/// The cleaner's header as the renderer measured it: the mode segment's two
/// labels ("Pilih sendiri", "Otomatis · Mole"), each with its padding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CleanHead {
    pub seg: [f32; 2],
}

/// The cleaner's footer as measured: "Dipilih 2,3 GB" and the button
/// ("Bersihkan 2,3 GB", or armed "Yakin? Hapus 2,3 GB").
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CleanFoot {
    pub picked_w: f32,
    pub go_w: f32,
}

/// One cleaner row (`.cl .row`): checkbox, icon, name over its subtitle,
/// size at the right. `row` is its click area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CleanRow {
    pub cat: CleanCat,
    pub row: Rect,
    pub check: Rect,
    pub icon: Rect,
    pub name: Rect,
    pub sub: Rect,
    pub size: Rect,
}

/// The cleaner after deepclean-opsi-d.html `cleanPanel2` / `ownPanel`: a
/// back button and "Bersihkan" with the mode segment on the right; the
/// seven rows in `CleanCat::ALL` order; the credit line at the bottom left,
/// "Dipilih" and the button at the right.
#[derive(Clone, Debug, PartialEq)]
pub struct CleanerLayout {
    /// The round back button (to Sistem), left of the title.
    pub back: Rect,
    pub header: Rect,
    /// The segment's track and its two items.
    pub seg: Rect,
    pub seg_items: [Rect; 2],
    pub rows: Vec<CleanRow>,
    pub credit: Rect,
    pub picked: Rect,
    pub go: Rect,
    /// The rows take clicks (not while a clean runs).
    pub rows_live: bool,
    /// The button takes clicks (something ticked; no scan or clean
    /// running).
    pub go_live: bool,
}

/// What the Mole mode's body shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoleBody {
    /// Not installed: the "Cara memasang" card.
    Card,
    /// The four groups and the two options.
    Rows,
    /// Mole's log (`result`: the measured result above it).
    Log { result: bool },
}

/// What the Mole mode shows and what takes a click (`render::CleanerView::
/// mole_look` decides from the reducer's phase).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoleLook {
    pub body: MoleBody,
    /// The mode segment (not while a clean runs).
    pub seg_live: bool,
    /// The two options (a new preview runs).
    pub opts_live: bool,
    /// "Lindungi folder…" shows and takes clicks (Mole found).
    pub protect: bool,
    /// The main button takes clicks: after a preview that finished well
    /// with these options, or after a run ("Pratinjau lagi").
    pub go: bool,
    /// By `MoleGroup::index`: the row's size says "lihat log" and shows
    /// the log when clicked.
    pub unreadable: [bool; 4],
    /// The preview's log shows: a click on it goes back to the groups.
    pub log_toggle: bool,
}

/// The Mole mode's text as the renderer measured it: the "Eksperimental"
/// chip, the options' labels, "Lindungi folder…", the footer label, the
/// main button and "Cara memasang ↗", full widths.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoleFoot {
    pub chip_w: f32,
    pub opt_w: [f32; 2],
    pub protect_w: f32,
    pub label_w: f32,
    pub go_w: f32,
    pub howto_w: f32,
}

/// One group row (`MG` in deepclean-opsi-d.html): icon, name over its
/// subtitle, the size at the right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoleRow {
    pub group: MoleGroup,
    pub row: Rect,
    pub icon: Rect,
    pub name: Rect,
    pub sub: Rect,
    pub size: Rect,
}

/// An option: its checkbox and label; `hit` covers both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoleOptRow {
    pub opt: MoleOpt,
    pub check: Rect,
    pub label: Rect,
    pub hit: Rect,
}

/// "Kalau Mole belum terpasang": Bata thinking, the title and line, the
/// ghost button at the right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoleCard {
    pub boxed: Rect,
    pub robot: Rect,
    pub title: Rect,
    pub sub: Rect,
    pub button: Rect,
}

/// The cleaner in Mole mode after deepclean-opsi-d.html `molePanel`: the
/// header as in the own mode; the ochre "Eksperimental" chip and its line;
/// the body (groups and options, the log, or the install card); the footer
/// with "Lindungi folder…" at the left, the preview's total and the button
/// at the right.
#[derive(Clone, Debug, PartialEq)]
pub struct MoleLayout {
    pub back: Rect,
    pub header: Rect,
    pub seg: Rect,
    pub seg_items: [Rect; 2],
    pub chip: Rect,
    pub intro: Rect,
    pub rows: Vec<MoleRow>,
    pub opts: Vec<MoleOptRow>,
    pub log: Option<Rect>,
    pub result: Option<Rect>,
    pub card: Option<MoleCard>,
    pub protect: Option<Rect>,
    pub label: Rect,
    pub go: Option<Rect>,
    pub look: MoleLook,
}

/// The System page's header as the renderer measured it: the uptime chip
/// and the "Bersihkan" button, full widths.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SystemHead {
    pub uptime_w: f32,
    pub clean_w: f32,
}

/// A labelled bar: the name on the left, the figures on the right, the bar
/// under them. `row` is the whole meter (a drive's click area).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterLayout {
    pub row: Rect,
    pub name: Rect,
    pub value: Rect,
    pub bar: Rect,
}

/// A busiest-process row: name, CPU and memory (both right-aligned).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcRow {
    pub row: Rect,
    pub name: Rect,
    pub cpu: Rect,
    pub mem: Rect,
}

/// The System page after keputusan-v3.1.html `sysPanel`: the title with the
/// uptime chip and "Bersihkan" on the header row; a 170 px column with the
/// CPU ring and its 60 s line; beside it the RAM meter, a meter per fixed
/// drive that fits, "Paling sibuk" and three process rows.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemLayout {
    pub header: Rect,
    pub uptime: Rect,
    pub clean: Rect,
    pub ring_center: (f32, f32),
    /// The stroke's centre line.
    pub ring_radius: f32,
    /// The big percentage and "CPU · 8 INTI" inside the ring.
    pub percent: Rect,
    pub caption: Rect,
    pub spark: Rect,
    pub ram: MeterLayout,
    /// The drives that fit, in the order given.
    pub drives: Vec<(char, MeterLayout)>,
    /// "Paling sibuk".
    pub busiest: Rect,
    pub procs: Vec<ProcRow>,
}

/// Side padding of the panel content.
const PAD_X: f32 = 20.0;
/// Content starts this far below the panel's top edge.
const PAD_TOP: f32 = 14.0;
const STRIP_TOP: f32 = 8.0;
pub const STRIP_H: f32 = 24.0;
const DOT: f32 = 5.0;
const DOT_GAP: f32 = 6.0;
pub(super) const DOT_BOTTOM: f32 = 11.0;
/// Clear space between the content and the page dots.
const ABOVE_DOTS: f32 = 6.0;
const HEADER_H: f32 = 22.0;
/// The panel header's Settings button: a square at the top right of every page.
const SETTINGS_BTN: f32 = 22.0;
/// Space between the Settings button and the header-row controls left of it.
const SETTINGS_GAP: f32 = 8.0;
const HEADER_GAP: f32 = 6.0;

const NAV_W: f32 = 22.0;
const NAV_GAP: f32 = 2.0;
/// Between the title and its arrows.
const TITLE_NAV_GAP: f32 = 8.0;
/// Between the arrows and the view selector, at the least.
const NAV_SEG_GAP: f32 = 12.0;
const VIEW_BUTTON_W: f32 = 50.0;
/// The view selector's track reaches this far around its buttons.
pub const SEG_PAD: f32 = 2.0;
/// The "Offline" tag and its gap after the arrows.
const OFFLINE_W: f32 = 52.0;
const OFFLINE_GAP: f32 = 8.0;

const CELL_W: f32 = 36.0;
/// Month rows are at most this far apart; less when the strip takes room.
const CELL_PITCH_MAX: f32 = 32.0;
const CELL_GAP_Y: f32 = 2.0;
const LABEL_H: f32 = 16.0;
/// Each side of the line between the month grid and its list.
const LIST_GAP: f32 = 16.0;
const CAPTION_H: f32 = 18.0;
const CAPTION_GAP: f32 = 6.0;
const ROW_H: f32 = 38.0;
const MORE_H: f32 = 20.0;
const MORE_W: f32 = 90.0;
const ADD_W: f32 = 130.0;
const ADD_H: f32 = 22.0;
const ADD_GAP: f32 = 4.0;
const CONNECT_W: f32 = 180.0;
const CONNECT_H: f32 = 26.0;
const CONNECT_GAP: f32 = 6.0;

const WEEK_GAP: f32 = 6.0;
/// Height of the clickable day name and number at the top of a week column.
pub const WEEK_HEAD_H: f32 = 36.0;
/// Event chips sit this far inside their column.
const CHIP_PAD: f32 = 5.0;
/// A timed event's chip (title over time) and an all-day band.
const CHIP_H: f32 = 30.0;
const BAND_H: f32 = 20.0;
const CHIP_GAP: f32 = 4.0;
const WEEK_MORE_H: f32 = 16.0;
const WEEK_ADD_GAP: f32 = 6.0;

/// The day view's all-day bands: per page, and how wide one gets.
const ALL_DAY_H: f32 = 22.0;
const ALL_DAY_PER_PAGE: usize = 2;
const ALL_DAY_MAX_W: f32 = 300.0;
const ALL_DAY_GAP: f32 = 6.0;
/// One hour of the day view's timeline.
const HOUR_H: f32 = 22.0;
/// Room left of the timeline for "09.00".
pub const HOUR_GUTTER: f32 = 40.0;
/// Half an hour label's height: kept clear above the first line.
const HOUR_LABEL_HALF: f32 = 6.0;
const BLOCK_INSET: f32 = 8.0;
/// Blocks end this short of the right edge, as in the design.
const BLOCK_RIGHT: f32 = 100.0;
const LANE_GAP: f32 = 4.0;
/// Overlapping blocks never get narrower than this; the ones that would
/// wait for a later page.
const MIN_LANE_W: f32 = 44.0;
const MIN_BLOCK_H: f32 = 18.0;
/// The first hour shown on a day without timed events.
const DAY_START_H: u32 = 8;
const MIN_HOURS: u32 = 2;

/// Big enough that the widest countdown (DM Sans 600 48, "100:00") clears
/// the stroke; the design's r = 100 does not fit under the waiting strip.
const RING_RADIUS: f32 = 92.0;
/// The ring's stroke, centred on `RING_RADIUS`.
pub const RING_STROKE: f32 = 8.0;
/// The countdown's box: one DM Sans 600 48 line (62.5 px).
const TIME_H: f32 = 63.0;
/// The digits' ink reaches this far above and below the box's centre
/// (DM Sans 600 48 measures cy - 17.8 to cy + 16.9).
const TIME_INK_HALF: f32 = 18.0;
/// The countdown sits this far above the ring's centre, the phase under it.
const TIME_LIFT: f32 = 9.0;
const PHASE_GAP: f32 = 8.0;
const PHASE_H: f32 = 16.0;
/// Between the ring and the column of the session, buttons and next phase.
const RING_GAP: f32 = 36.0;
const COLUMN_W: f32 = 230.0;
const SESSION_H: f32 = 20.0;
const COLUMN_GAP: f32 = 18.0;
const PRIMARY_D: f32 = 62.0;
const SECONDARY_D: f32 = 44.0;
const CONTROL_GAP: f32 = 14.0;
const NEXT_H: f32 = 18.0;
const DURATIONS_W: f32 = 160.0;

const NEW_W: f32 = 64.0;
const LIST_TOP_GAP: f32 = 6.0;
const REMINDER_H: f32 = 29.0;
const GROUP_H: f32 = 18.0;
/// Above every day group but the chunk's first.
const GROUP_GAP: f32 = 6.0;
const NOTE_COLUMNS: usize = 2;
const NOTE_ROWS: usize = 2;
const NOTE_GAP: f32 = 9.0;
const NOTE_MAX_H: f32 = 92.0;
const LIST_MORE_GAP: f32 = 4.0;
const LIST_MORE_W: f32 = 120.0;

/// The CLI page after the design's `.panel` (16 px over the header, 28 px
/// under the terminal for the dots), `.ph` (28 px, 8 under it), `.term`,
/// `.out` (10 / 14 px in) and `.inp` (44 px).
const CLI_TOP: f32 = 16.0;
const CLI_STRIP_GAP: f32 = 6.0;
const CLI_HEAD_H: f32 = 28.0;
const CLI_HEAD_GAP: f32 = 8.0;
const CLI_BOTTOM: f32 = 24.0;
const CLI_TITLE_W: f32 = 56.0;
const CLI_CHIP_H: f32 = 20.0;
const CLI_CHIP_GAP: f32 = 6.0;
const CLI_STOP_H: f32 = 22.0;
const CLI_EXPAND: f32 = 26.0;
const CLI_OUT_X: f32 = 14.0;
const CLI_OUT_TOP: f32 = 10.0;
const CLI_INPUT_H: f32 = 44.0;
const CLI_PROMPT_W: f32 = 16.0;
const CLI_SUGGEST_INSET: f32 = 10.0;
const CLI_SUGGEST_GAP: f32 = 6.0;
const CLI_SUGGEST_PAD: f32 = 5.0;
/// The folder chip is not cut below this while "dari Explorer" could go.
pub const CLI_FOLDER_MIN: f32 = 120.0;
/// The folder menu, after keputusan-v3.1.html `.menu`: 300 px wide (or
/// the terminal's width), 6 px in, 14 px corners; headers 9 px capitals
/// (6 above, 4 below), rows 6 / 8 px in with 8 px corners.
pub const MENU_W: f32 = 300.0;
pub const MENU_PAD: f32 = 6.0;
pub const MENU_RADIUS: f32 = 14.0;
pub const MENU_HEAD_H: f32 = 22.0;
pub const MENU_ROW_H: f32 = 26.0;
pub const MENU_ROW_RADIUS: f32 = 8.0;
/// A row's star button at its right end.
pub const MENU_STAR_W: f32 = 24.0;
/// Under the header row's chips.
const MENU_TOP_GAP: f32 = 2.0;
/// Explorer windows shown at most, unless the terminal is taller than
/// `MENU_ROOMY_TERM` (the enlarged panel), which shows more.
const MENU_EXPLORER_MAX: usize = 3;
const MENU_EXPLORER_MAX_TALL: usize = 6;
const MENU_ROOMY_TERM: f32 = 300.0;

const TILE_GAP: f32 = 10.0;
/// Tiles stay this short when the panel has room to spare.
const TILE_MAX_H: f32 = 98.0;
const TILE_GLYPH_H: f32 = 28.0;
const TILE_LABEL_H: f32 = 16.0;
const TILE_INNER_GAP: f32 = 8.0;

/// The cleaner, after deepclean-opsi-d.html `.panel` (16 px over the
/// header), `.ph` (28 px, 10 under it), `.cl .row` (3 / 8 px in, a
/// 18 + 22 px grid with 9 px gaps), the segment (2 px track, 3 / 10 px
/// items) and the footer 26 px over the bottom edge.
const CLEAN_TOP: f32 = 16.0;
const CLEAN_HEAD_H: f32 = 28.0;
const CLEAN_HEAD_GAP: f32 = 10.0;
const CLEAN_BACK: f32 = 24.0;
const CLEAN_BACK_GAP: f32 = 8.0;
const CLEAN_SEG_H: f32 = 24.0;
const CLEAN_SEG_PAD: f32 = 2.0;
/// One row and the 1 px under it; less when the waiting strip takes room.
const CLEAN_ROW_PITCH: f32 = 34.0;
const CLEAN_ROW_GAP: f32 = 1.0;
const CLEAN_ROW_PAD_X: f32 = 8.0;
pub const CLEAN_CHECK: f32 = 16.0;
const CLEAN_CHECK_COL: f32 = 18.0;
const CLEAN_ICON_COL: f32 = 22.0;
const CLEAN_COL_GAP: f32 = 9.0;
const CLEAN_NAME_H: f32 = 15.0;
const CLEAN_SUB_H: f32 = 13.0;
const CLEAN_SIZE_W: f32 = 76.0;
const CLEAN_FOOT_H: f32 = 24.0;
const CLEAN_FOOT_BOTTOM: f32 = 26.0;
const CLEAN_FOOT_GAP: f32 = 8.0;
/// Clear space between the last row and the footer.
const CLEAN_ROWS_FOOT_GAP: f32 = 6.0;
/// Mole mode (`molePanel`): the chip row under the header, the gaps
/// around the body, the options row (8 px over it, 14 px between them),
/// the result line over the log, the install card.
const MOLE_CHIP_GAP: f32 = 6.0;
const MOLE_CHIP_H: f32 = 20.0;
const MOLE_CHIP_TEXT_GAP: f32 = 8.0;
const MOLE_BODY_GAP: f32 = 8.0;
const MOLE_OPT_H: f32 = 18.0;
const MOLE_OPT_GAP: f32 = 14.0;
const MOLE_OPT_CHECK_GAP: f32 = 6.0;
const MOLE_RESULT_H: f32 = 20.0;
const MOLE_RESULT_GAP: f32 = 4.0;
const MOLE_CARD_TOP: f32 = 16.0;
const MOLE_CARD_H: f32 = 64.0;
const MOLE_CARD_PAD: f32 = 14.0;
pub const MOLE_CARD_ROBOT: f32 = 30.0;
const MOLE_CARD_GAP: f32 = 10.0;
/// The log's line pitch: DM Mono 9.5 at the design's 1.5.
pub const MOLE_LOG_LINE_H: f32 = 14.5;
/// The log's text inset inside its dark box.
pub const MOLE_LOG_PAD: f32 = 10.0;

/// How many log lines fit in the log box `area`: the painter's rows and the
/// wheel's scroll range.
pub fn mole_log_rows(area: Rect) -> usize {
    ((area.h - 2.0 * MOLE_LOG_PAD) / MOLE_LOG_LINE_H)
        .floor()
        .max(0.0) as usize
}

/// The first log line drawn: the newest `rows` lines, or the window
/// `scroll` lines up from them (never past the oldest line).
pub fn mole_log_skip(len: usize, rows: usize, scroll: usize) -> usize {
    let hidden = len.saturating_sub(rows);
    hidden - scroll.min(hidden)
}

/// The System page, after keputusan-v3.1.html `.sys`, `.ringw`, `.spark`,
/// `.meter`, `.bar`, `.procs` and `.cbtn`.
const SYS_HEAD_H: f32 = 28.0;
const SYS_HEAD_GAP: f32 = 8.0;
const SYS_CHIP_H: f32 = 20.0;
const SYS_CLEAN_H: f32 = 24.0;
const SYS_CHIP_GAP: f32 = 6.0;
const SYS_LEFT_W: f32 = 170.0;
const SYS_COL_GAP: f32 = 18.0;
/// The ring's box (150 px), its stroke and the room above it.
const SYS_RING_D: f32 = 150.0;
pub const SYS_RING_STROKE: f32 = 11.0;
const SYS_RING_TOP: f32 = 4.0;
const SYS_PERCENT_H: f32 = 38.0;
const SYS_CAPTION_H: f32 = 14.0;
/// The CPU line: 150 × 34, 10 px under the ring.
const SYS_SPARK_W: f32 = 150.0;
const SYS_SPARK_H: f32 = 34.0;
const SYS_SPARK_GAP: f32 = 10.0;
/// A meter: its 15 px heading, 5 px, the 7 px bar; 11 px to the next.
const METER_HEAD_H: f32 = 15.0;
const METER_BAR_GAP: f32 = 5.0;
pub const METER_BAR_H: f32 = 7.0;
const METER_H: f32 = METER_HEAD_H + METER_BAR_GAP + METER_BAR_H;
const METER_GAP: f32 = 11.0;
const METER_NAME_W: f32 = 70.0;
/// "Paling sibuk" (10 above, 4 under) and the process rows.
const BUSIEST_GAP: f32 = 10.0;
const BUSIEST_H: f32 = 14.0;
const BUSIEST_ROWS_GAP: f32 = 4.0;
const PROC_H: f32 = 16.0;
const PROC_CPU_W: f32 = 40.0;
const PROC_MEM_W: f32 = 56.0;
const PROC_COL_GAP: f32 = 12.0;

const COVER: f32 = 196.0;
const COVER_GAP: f32 = 24.0;
const MEDIA_LABEL_H: f32 = 14.0;
const MEDIA_TITLE_H: f32 = 38.0;
const MEDIA_ARTIST_H: f32 = 18.0;
/// The progress bar: this far below the artist, this thick; the times row
/// under it.
const MEDIA_BAR_GAP: f32 = 18.0;
const MEDIA_BAR_H: f32 = 4.0;
const MEDIA_TIMES_GAP: f32 = 6.0;
const MEDIA_TIMES_H: f32 = 13.0;
/// From the bar's top to the times' bottom.
const MEDIA_PROGRESS_H: f32 = MEDIA_BAR_H + MEDIA_TIMES_GAP + MEDIA_TIMES_H;
/// Between the times and the transport row.
const MEDIA_CONTROLS_GAP: f32 = 14.0;
const MEDIA_PLAY: f32 = 48.0;
const MEDIA_SIDE_W: f32 = 32.0;
const MEDIA_BUTTON_GAP: f32 = 12.0;
const LYRICS_W: f32 = 66.0;
const LYRICS_H: f32 = 26.0;

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect {
        x,
        y,
        w: w.max(0.0),
        h: h.max(0.0),
    }
}

fn left(s: ShapeRect) -> f32 {
    s.x + PAD_X
}

fn right(s: ShapeRect) -> f32 {
    s.x + s.w - PAD_X
}

/// The right edge of every header-row control: left of the Settings button
/// and its gap. Body content below the header row keeps `right`.
fn header_right(s: ShapeRect) -> f32 {
    right(s) - SETTINGS_BTN - SETTINGS_GAP
}

/// The panel header's Settings button. The same rect on every page (it never
/// depends on the page, so it never jumps between pages), in the header row
/// at the top right.
pub fn settings_button(s: ShapeRect, strip: bool) -> Rect {
    rect(
        right(s) - SETTINGS_BTN,
        content_top(s, strip),
        SETTINGS_BTN,
        SETTINGS_BTN,
    )
}

/// Top of the page content: below the waiting strip when it is shown.
fn content_top(s: ShapeRect, strip: bool) -> f32 {
    s.y + PAD_TOP + if strip { STRIP_H } else { 0.0 }
}

/// Lowest y the page content may reach: just above the page dots.
fn content_bottom(s: ShapeRect) -> f32 {
    s.y + s.h - DOT_BOTTOM - DOT - ABOVE_DOTS
}

/// How many of `n` rows of `pitch` fit in `avail`; when not all do, room is
/// kept for a `more_h` "+N" line under the shown rows.
fn rows_that_fit(n: usize, avail: f32, pitch: f32, gap: f32, more_h: f32) -> usize {
    if n as f32 * pitch - gap <= avail {
        n
    } else {
        ((avail - more_h) / pitch).floor().max(0.0) as usize
    }
}

/// One dot per page `shown` ("Halaman panel"), and which one is lit: none
/// for a page that is off but opened by its own button.
pub fn page_dots(s: ShapeRect, shown: &[Page], page: Page) -> (Vec<Rect>, Option<usize>) {
    let active = shown.iter().position(|p| *p == page);
    let count = shown.len() as f32;
    let total = count * DOT + (count - 1.0).max(0.0) * DOT_GAP;
    let x0 = s.x + (s.w - total) / 2.0;
    let y = s.y + s.h - DOT_BOTTOM - DOT;
    let dots = (0..shown.len())
        .map(|i| rect(x0 + i as f32 * (DOT + DOT_GAP), y, DOT, DOT))
        .collect();
    (dots, active)
}

/// The Claude waiting strip across the content width, under the top edge.
pub fn waiting_strip(s: ShapeRect) -> Rect {
    rect(left(s), s.y + STRIP_TOP, s.w - 2.0 * PAD_X, STRIP_H)
}

/// Which `shown` items of `n` to list when `k` fit: chunk `page` (wrapping)
/// once they do not all fit. Returns `(start, shown)`.
fn chunk(n: usize, k: usize, page: usize) -> (usize, usize) {
    if n <= k {
        (0, n)
    } else if k == 0 {
        (0, 0)
    } else {
        let start = (page % n.div_ceil(k)) * k;
        (start, k.min(n - start))
    }
}

/// Event rows stacked from `y` down to `bottom`: optional connect button,
/// rows, "+N lagi" if they do not all fit, then "+ Tambah acara".
struct Stack {
    connect: Option<Rect>,
    rows: Vec<(String, Rect)>,
    more: Option<(usize, Rect)>,
    add: Rect,
}

/// `page` picks the chunk of rows shown when they do not all fit.
fn stack(
    x: f32,
    w: f32,
    mut y: f32,
    bottom: f32,
    ids: Vec<String>,
    page: usize,
    connect: bool,
) -> Stack {
    let connect = if connect {
        let r = rect(x, y, w.min(CONNECT_W), CONNECT_H);
        y = r.bottom() + CONNECT_GAP;
        Some(r)
    } else {
        None
    };
    let avail = bottom - y - ADD_GAP - ADD_H;
    let total = ids.len();
    let k = rows_that_fit(total, avail, ROW_H, 0.0, MORE_H);
    let (start, shown) = chunk(total, k, page);
    let rows: Vec<(String, Rect)> = ids
        .into_iter()
        .skip(start)
        .take(shown)
        .enumerate()
        .map(|(i, id)| (id, rect(x, y + i as f32 * ROW_H, w, ROW_H)))
        .collect();
    y += shown as f32 * ROW_H;
    let more = if shown < total {
        let r = rect(x, y, w.min(MORE_W), MORE_H);
        y = r.bottom();
        Some((total - start - shown, r))
    } else {
        None
    };
    Stack {
        connect,
        rows,
        more,
        add: rect(x, y + ADD_GAP, w.min(ADD_W), ADD_H),
    }
}

/// An event's minutes on `day`, start and end clipped to the day; one with
/// no length counts as half an hour.
fn minutes_on(ev: &CalEvent, day: NaiveDate) -> (i64, i64) {
    let day_start = day.and_time(NaiveTime::MIN);
    let minute = |t: &EventTime| {
        let at = match t {
            EventTime::AllDay(d) => d.and_time(NaiveTime::MIN),
            EventTime::At(t) => t.naive_local(),
        };
        (at - day_start).num_minutes().clamp(0, 24 * 60)
    };
    let start = minute(&ev.start).min(24 * 60 - 1);
    let end = minute(&ev.end);
    (
        start,
        if end > start {
            end
        } else {
            (start + 30).min(24 * 60)
        },
    )
}

/// Month: grid on the left, the selected day's events on the right. Week:
/// seven columns of event chips. Day: all-day bands over an hour timeline;
/// `day_page` (wrapping) pages through what does not fit. `events` is the
/// whole snapshot; each day's share comes from `events_on`. `title_w` is the
/// drawn title's width (the arrows follow it); `now` the local time, which
/// the day view of today keeps in sight and marks.
#[allow(clippy::too_many_arguments)]
pub fn calendar_layout(
    s: ShapeRect,
    view: CalView,
    anchor: NaiveDate,
    selected: NaiveDate,
    events: &[CalEvent],
    status: &CalStatus,
    day_page: usize,
    strip: bool,
    title_w: f32,
    now: Option<NaiveDateTime>,
) -> CalendarLayout {
    let top = content_top(s, strip);
    let bottom = content_bottom(s);
    let (l, r) = (left(s), right(s));

    // Header row: the title and its arrows on the left, the view selector
    // on the right.
    let seg_w = 3.0 * VIEW_BUTTON_W;
    let seg_x = header_right(s) - SEG_PAD - seg_w;
    let views = [CalView::Day, CalView::Week, CalView::Month];
    let view_buttons = std::array::from_fn(|i| {
        let x = seg_x + i as f32 * VIEW_BUTTON_W;
        (views[i], rect(x, top, VIEW_BUTTON_W, HEADER_H))
    });
    let nav_w = 2.0 * NAV_W + NAV_GAP;
    let offline_on = *status == CalStatus::Offline;
    // Offline, the title gives up room for the tag rather than the tag hiding.
    let tag_w = if offline_on {
        OFFLINE_GAP + OFFLINE_W
    } else {
        0.0
    };
    let title_max = seg_x - SEG_PAD - NAV_SEG_GAP - tag_w - nav_w - TITLE_NAV_GAP - l;
    let header = rect(l, top, title_w.clamp(0.0, title_max.max(0.0)), HEADER_H);
    let prev = rect(header.right() + TITLE_NAV_GAP, top, NAV_W, HEADER_H);
    let next = rect(prev.right() + NAV_GAP, top, NAV_W, HEADER_H);
    let offline = offline_on.then(|| {
        rect(
            next.right() + OFFLINE_GAP,
            top + 3.0,
            OFFLINE_W,
            HEADER_H - 6.0,
        )
    });
    let body = header.bottom() + HEADER_GAP;

    let ids_on = |d: NaiveDate| -> Vec<String> {
        events_on(events, d)
            .into_iter()
            .map(|e| e.id.clone())
            .collect()
    };
    // Not configured also offers the button: clicking it tells the owner to
    // fill in the client ID, instead of an empty calendar saying nothing.
    let wants_connect = matches!(
        status,
        CalStatus::NotConfigured | CalStatus::NeedsLogin | CalStatus::Reconnect
    );
    let mut layout = CalendarLayout {
        view,
        selected,
        header,
        view_buttons,
        prev,
        next,
        weekday_labels: Vec::new(),
        month_cells: Vec::new(),
        week_columns: Vec::new(),
        week_more: Vec::new(),
        list: Rect::default(),
        caption: None,
        rows: Vec::new(),
        more: None,
        hours: Vec::new(),
        now_y: None,
        add: Rect::default(),
        connect: None,
        offline,
    };

    match view {
        CalView::Month => {
            let col_x = |c: usize| l + c as f32 * CELL_W;
            layout.weekday_labels = (0..7)
                .map(|c| rect(col_x(c), body, CELL_W, LABEL_H))
                .collect();
            let cells_y = body + LABEL_H + CELL_GAP_Y;
            let pitch = ((bottom - cells_y + CELL_GAP_Y) / 6.0)
                .floor()
                .min(CELL_PITCH_MAX);
            layout.month_cells =
                super::month_grid::month_cells(anchor, l, cells_y, CELL_W, pitch, CELL_GAP_Y);
            let list_x = col_x(7) + 2.0 * LIST_GAP;
            let list = rect(list_x, body, r - list_x, bottom - body);
            let caption = rect(list.x, list.y, list.w, CAPTION_H);
            let st = stack(
                list.x,
                list.w,
                caption.bottom() + CAPTION_GAP,
                bottom,
                ids_on(selected),
                0,
                wants_connect,
            );
            layout.list = list;
            layout.caption = Some(caption);
            layout.rows = st.rows;
            layout.more = st.more;
            layout.add = st.add;
            layout.connect = st.connect;
        }
        CalView::Day => day_timeline(
            &mut layout,
            (l, r, body, bottom),
            events,
            day_page,
            wants_connect,
            now,
        ),
        CalView::Week => {
            let (monday, _) = range_for(CalView::Week, anchor);
            let col_w = (r - l - 6.0 * WEEK_GAP) / 7.0;
            let col_h = bottom - body - WEEK_ADD_GAP - ADD_H;
            for i in 0..7usize {
                let Some(d) = monday.checked_add_days(Days::new(i as u64)) else {
                    break;
                };
                let x = l + i as f32 * (col_w + WEEK_GAP);
                let column = rect(x, body, col_w, col_h);
                layout.week_columns.push((d, column));
                let on = events_on(events, d);
                let heights: Vec<f32> = on
                    .iter()
                    .map(|e| if e.all_day() { BAND_H } else { CHIP_H })
                    .collect();
                let chips_top = body + WEEK_HEAD_H;
                let avail = column.bottom() - CHIP_PAD - chips_top;
                let all: f32 = heights.iter().map(|h| h + CHIP_GAP).sum::<f32>() - CHIP_GAP;
                let room = if all <= avail {
                    avail
                } else {
                    avail - WEEK_MORE_H - CHIP_GAP
                };
                let mut y = chips_top;
                let mut shown = 0;
                for (ev, h) in on.iter().zip(&heights) {
                    if y + h > chips_top + room {
                        break;
                    }
                    let chip = rect(x + CHIP_PAD, y, col_w - 2.0 * CHIP_PAD, *h);
                    layout.rows.push((ev.id.clone(), chip));
                    y += h + CHIP_GAP;
                    shown += 1;
                }
                if shown < on.len() {
                    let foot = column.bottom() - CHIP_PAD - WEEK_MORE_H;
                    let more = rect(x + CHIP_PAD, foot, col_w - 2.0 * CHIP_PAD, WEEK_MORE_H);
                    layout.week_more.push((d, on.len() - shown, more));
                }
            }
            layout.list = rect(l, body, r - l, col_h);
            layout.add = rect(l, layout.list.bottom() + WEEK_ADD_GAP, ADD_W, ADD_H);
            // As tall as in the other views, centered on the "+ Tambah acara" row.
            let y = layout.add.y + (ADD_H - CONNECT_H) / 2.0;
            layout.connect =
                wants_connect.then(|| rect(layout.add.right() + 12.0, y, CONNECT_W, CONNECT_H));
        }
    }
    layout
}

/// The day view: an optional connect button, the all-day bands, then an
/// hour timeline with the timed events as blocks (side by side where they
/// overlap), and "+ Tambah acara" with "+N lagi" at the foot.
///
/// Pages: the first shows the hours from just before the first event (or
/// around `now` on today); each further one the hours of the earliest event
/// not seen yet, and the next two all-day bands when there are more than
/// two. Blocks that overlap share the width in lanes no narrower than
/// `MIN_LANE_W`; any that do not fit wait for the next page. "+N lagi"
/// counts what the following pages add.
fn day_timeline(
    layout: &mut CalendarLayout,
    (l, r, body, bottom): (f32, f32, f32, f32),
    events: &[CalEvent],
    day_page: usize,
    wants_connect: bool,
    now: Option<NaiveDateTime>,
) {
    let day = layout.selected;
    let mut y = body;
    if wants_connect {
        let c = rect(l, y, (r - l).min(CONNECT_W), CONNECT_H);
        y = c.bottom() + CONNECT_GAP;
        layout.connect = Some(c);
    }
    let on = events_on(events, day);
    let (all_day, timed): (Vec<&CalEvent>, Vec<&CalEvent>) =
        on.into_iter().partition(|e| e.all_day());
    let band_y = y;
    if !all_day.is_empty() {
        y += ALL_DAY_H + ALL_DAY_GAP;
    }
    let foot = bottom - ADD_H;
    layout.add = rect(l, foot, ADD_W, ADD_H);
    let tl_top = y + HOUR_LABEL_HALF;
    let tl_bottom = foot - ADD_GAP - HOUR_LABEL_HALF;
    let span = (((tl_bottom - tl_top) / HOUR_H).floor().max(0.0) as u32).clamp(MIN_HOURS, 24);

    let minutes: Vec<(i64, i64)> = timed.iter().map(|e| minutes_on(e, day)).collect();
    let in_window =
        |h: u32, (s, e): (i64, i64)| s < i64::from(h + span) * 60 && e > i64::from(h) * 60;
    let mut first = minutes
        .first()
        .map_or(DAY_START_H, |(s, _)| ((*s / 60) as u32).saturating_sub(1));
    let today_now = now.filter(|n| n.date() == day);
    if let Some(n) = today_now
        && !(first..first + span).contains(&n.hour())
    {
        first = n.hour().saturating_sub(span / 2);
    }
    let tl_end = tl_top + span as f32 * HOUR_H;
    let area_x = l + HOUR_GUTTER + BLOCK_INSET;
    let area_w = r - BLOCK_RIGHT - area_x;
    let max_lanes = (((area_w + LANE_GAP) / (MIN_LANE_W + LANE_GAP)).floor() as usize).max(1);
    // An event's block (top, height) in the hours from `window`.
    let block = |window: u32, (s, e): (i64, i64)| {
        let y_of = |minute: i64| tl_top + (minute - i64::from(window) * 60) as f32 / 60.0 * HOUR_H;
        let (y0, y1) = (y_of(s).max(tl_top), y_of(e).min(tl_end));
        let h = (y1 - y0 - 2.0).max(MIN_BLOCK_H);
        ((y0 + 1.0).min(tl_end - h), h)
    };
    // A placed block: index in `timed`, top, height, lane.
    type Block = (usize, f32, f32, usize);
    // The blocks of one window: each in the first lane free over its time,
    // events not shown yet first. Past `max_lanes` a block waits for a
    // later page.
    let place = |window: u32, seen: &[bool]| {
        let mut order: Vec<usize> = (0..minutes.len())
            .filter(|&i| in_window(window, minutes[i]))
            .collect();
        order.sort_by_key(|&i| (seen[i], minutes[i].0, i));
        let mut lanes: Vec<Vec<(f32, f32)>> = Vec::new();
        let mut placed: Vec<Block> = Vec::new();
        for i in order {
            let (top, h) = block(window, minutes[i]);
            let (a, b) = (top, top + h + 1.0);
            let free = |lane: &Vec<(f32, f32)>| lane.iter().all(|(s, e)| b <= *s || *e <= a);
            let lane = match lanes.iter().position(free) {
                Some(k) => k,
                None if lanes.len() < max_lanes => {
                    lanes.push(Vec::new());
                    lanes.len() - 1
                }
                None => continue,
            };
            lanes[lane].push((a, b));
            placed.push((i, top, h, lane));
        }
        placed
    };
    // Pages of the timeline: each further one starts at the hour of the
    // earliest event not shown yet, which always gets a lane there.
    let mut seen = vec![false; minutes.len()];
    let mut windows: Vec<(u32, Vec<Block>)> = Vec::new();
    let mut window = first.min(24 - span);
    loop {
        let placed = place(window, &seen);
        for (i, ..) in &placed {
            seen[*i] = true;
        }
        windows.push((window, placed));
        let next = (0..minutes.len())
            .filter(|&i| !seen[i])
            .min_by_key(|&i| (minutes[i].0, i));
        let Some(i) = next else { break };
        window = ((minutes[i].0 / 60) as u32).min(24 - span);
    }
    let band_pages = if all_day.len() <= ALL_DAY_PER_PAGE {
        1
    } else {
        all_day.len().div_ceil(ALL_DAY_PER_PAGE)
    };
    let pages = windows.len().max(band_pages);
    let page = day_page % pages;
    let (window, placed) = &windows[page.min(windows.len() - 1)];
    let window = *window;

    // All-day bands of this page.
    let bands: &[&CalEvent] = if all_day.len() <= ALL_DAY_PER_PAGE {
        &all_day
    } else if page < band_pages {
        let start = page * ALL_DAY_PER_PAGE;
        &all_day[start..(start + ALL_DAY_PER_PAGE).min(all_day.len())]
    } else {
        &[]
    };
    if !bands.is_empty() {
        let k = bands.len() as f32;
        let w = ((r - l - (k - 1.0) * ALL_DAY_GAP) / k).min(ALL_DAY_MAX_W);
        for (i, ev) in bands.iter().enumerate() {
            let x = l + i as f32 * (w + ALL_DAY_GAP);
            layout
                .rows
                .push((ev.id.clone(), rect(x, band_y, w, ALL_DAY_H)));
        }
    }

    // The timeline and its blocks.
    let y_of = |minute: i64| tl_top + (minute - i64::from(window) * 60) as f32 / 60.0 * HOUR_H;
    layout.list = rect(l, tl_top, r - l, tl_end - tl_top);
    layout.hours = (0..=span)
        .map(|k| (window + k, tl_top + k as f32 * HOUR_H))
        .collect();
    layout.now_y = today_now.and_then(|n| {
        let minute = i64::from(n.hour() * 60 + n.minute());
        let y = y_of(minute);
        (tl_top..=tl_end).contains(&y).then_some(y)
    });
    // Lanes split the width only within a run of overlapping blocks.
    let mut by_top: Vec<&Block> = placed.iter().collect();
    by_top.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut start = 0;
    while start < by_top.len() {
        let mut end_y = by_top[start].1 + by_top[start].2 + 1.0;
        let mut stop = start + 1;
        while stop < by_top.len() && by_top[stop].1 < end_y {
            end_y = end_y.max(by_top[stop].1 + by_top[stop].2 + 1.0);
            stop += 1;
        }
        let cluster = &by_top[start..stop];
        // The run's lanes renumbered 0, 1, 2…, so a lane no block of the run
        // uses never takes width (blocks are placed unseen-first, not in time
        // order).
        let mut used: Vec<usize> = cluster.iter().map(|b| b.3).collect();
        used.sort_unstable();
        used.dedup();
        let n = used.len().max(1) as f32;
        let lane_w = ((area_w - (n - 1.0) * LANE_GAP) / n).max(0.0);
        for (i, top, h, lane) in cluster {
            let rank = used.binary_search(lane).unwrap_or(0);
            let x = area_x + rank as f32 * (lane_w + LANE_GAP);
            layout
                .rows
                .push((timed[*i].id.clone(), rect(x, *top, lane_w, *h)));
        }
        start = stop;
    }

    // "+N lagi": what the next pages add; "Kembali ke atas" on the last.
    if pages > 1 {
        let shown_bands = if all_day.len() <= ALL_DAY_PER_PAGE {
            all_day.len()
        } else {
            ((page + 1) * ALL_DAY_PER_PAGE).min(all_day.len())
        };
        let mut shown = vec![false; minutes.len()];
        for (_, placed) in &windows[..=page.min(windows.len() - 1)] {
            for (i, ..) in placed {
                shown[*i] = true;
            }
        }
        let shown_timed = shown.iter().filter(|s| **s).count();
        let later = all_day.len() + timed.len() - shown_bands - shown_timed;
        let count = if page + 1 == pages { 0 } else { later };
        let more = rect(layout.add.right() + 12.0, foot, MORE_W, ADD_H);
        layout.more = Some((count, more));
    }
}

/// Big ring on the left with the countdown and the phase inside; right of
/// it a column with the session, the three round buttons (the primary one
/// in the middle) and the phase that comes next. Ring and column together
/// are centred in the panel.
pub fn pomodoro_layout(s: ShapeRect, strip: bool) -> PomodoroLayout {
    let top = content_top(s, strip);
    let bottom = content_bottom(s);
    let (l, r) = (left(s), right(s));
    let durations = rect(header_right(s) - DURATIONS_W, top, DURATIONS_W, HEADER_H);
    let header = rect(l, top, durations.x - 8.0 - l, HEADER_H);
    let body = header.bottom() + 8.0;
    let cy = body + (bottom - body) / 2.0;
    let outer = RING_RADIUS + RING_STROKE / 2.0;
    let group_w = 2.0 * outer + RING_GAP + COLUMN_W;
    let group_x = l + ((r - l) - group_w) / 2.0;
    let cx = group_x + outer;
    let inner = RING_RADIUS - RING_STROKE / 2.0;
    // Widest where the inner edge allows at the farther of the ink's edges.
    let half_at = |dy: f32| (inner * inner - dy * dy).max(0.0).sqrt();
    let time_cy = cy - TIME_LIFT;
    let time_half = half_at(TIME_LIFT + TIME_INK_HALF);
    let time = rect(
        cx - time_half,
        time_cy - TIME_H / 2.0,
        2.0 * time_half,
        TIME_H,
    );
    let phase_y = time_cy + TIME_INK_HALF + PHASE_GAP;
    let phase_half = half_at(phase_y + PHASE_H - cy);
    let phase = rect(cx - phase_half, phase_y, 2.0 * phase_half, PHASE_H);

    let x = group_x + 2.0 * outer + RING_GAP;
    let column_h = SESSION_H + COLUMN_GAP + PRIMARY_D + COLUMN_GAP + NEXT_H;
    let session = rect(x, cy - column_h / 2.0, COLUMN_W, SESSION_H);
    let row_y = session.bottom() + COLUMN_GAP;
    let side_y = row_y + (PRIMARY_D - SECONDARY_D) / 2.0;
    let reset = rect(x, side_y, SECONDARY_D, SECONDARY_D);
    let start_pause = rect(reset.right() + CONTROL_GAP, row_y, PRIMARY_D, PRIMARY_D);
    let skip = rect(
        start_pause.right() + CONTROL_GAP,
        side_y,
        SECONDARY_D,
        SECONDARY_D,
    );
    let next_up = rect(x, start_pause.bottom() + COLUMN_GAP, COLUMN_W, NEXT_H);
    PomodoroLayout {
        header,
        durations,
        ring_center: (cx, cy),
        ring_radius: RING_RADIUS,
        time,
        phase,
        session,
        start_pause,
        reset,
        skip,
        next_up,
    }
}

/// The title and "+ Baru" of the notes and reminders pages, and the y the
/// items start at.
fn list_header(s: ShapeRect, strip: bool) -> (Rect, Rect, f32) {
    let top = content_top(s, strip);
    let (l, hr) = (left(s), header_right(s));
    let header = rect(l, top, hr - l - NEW_W - 8.0, HEADER_H);
    let new_button = rect(hr - NEW_W, top, NEW_W, HEADER_H);
    (header, new_button, header.bottom() + LIST_TOP_GAP)
}

/// Cards in two columns, newest top left, two rows of them. When there are
/// more, the page shows chunk `notes_page` (wrapping) and a "+N lagi" /
/// "Kembali ke atas" line under the cards.
pub fn notes_layout(s: ShapeRect, notes: &[Note], notes_page: usize, strip: bool) -> ListLayout {
    let (header, new_button, list_y) = list_header(s, strip);
    let (l, r) = (left(s), right(s));
    let bottom = content_bottom(s);
    let per_page = NOTE_COLUMNS * NOTE_ROWS;
    let rows_f = NOTE_ROWS as f32;
    let card_h = ((bottom - list_y - LIST_MORE_GAP - MORE_H - (rows_f - 1.0) * NOTE_GAP) / rows_f)
        .min(NOTE_MAX_H);
    let card_w = (r - l - (NOTE_COLUMNS as f32 - 1.0) * NOTE_GAP) / NOTE_COLUMNS as f32;
    let n = notes.len();
    let (start, shown) = chunk(n, per_page, notes_page);
    let rows = notes
        .iter()
        .skip(start)
        .take(shown)
        .enumerate()
        .map(|(i, note)| {
            let (row, col) = ((i / NOTE_COLUMNS) as f32, (i % NOTE_COLUMNS) as f32);
            let x = l + col * (card_w + NOTE_GAP);
            let y = list_y + row * (card_h + NOTE_GAP);
            (note.id.clone(), rect(x, y, card_w, card_h))
        })
        .collect();
    let more = (n > per_page).then(|| {
        let y = list_y + rows_f * card_h + (rows_f - 1.0) * NOTE_GAP + LIST_MORE_GAP;
        (n - start - shown, rect(l, y, LIST_MORE_W, MORE_H))
    });
    ListLayout {
        header,
        new_button,
        groups: Vec::new(),
        rows,
        more,
    }
}

/// The upcoming reminders in rows grouped by day, each group under its
/// heading: overdue ones join today's group. When they do not all fit, the
/// page shows chunk `page` (wrapping), each chunk starting with its group's
/// heading, and a "+N lagi" / "Kembali ke atas" line.
pub fn reminders_layout(
    s: ShapeRect,
    reminders: &[Reminder],
    page: usize,
    strip: bool,
    today: NaiveDate,
) -> ListLayout {
    let (header, new_button, list_y) = list_header(s, strip);
    let (l, r) = (left(s), right(s));
    let bottom = content_bottom(s);
    let days: Vec<NaiveDate> = reminders
        .iter()
        .map(|x| {
            local_time(x.due_unix)
                .map_or(today, |t| t.date())
                .max(today)
        })
        .collect();
    // Height of items `start..end` from the top of a chunk.
    let height = |start: usize, end: usize| -> f32 {
        (start..end)
            .map(|i| {
                let heading = if i == start {
                    GROUP_H
                } else if days[i] != days[i - 1] {
                    GROUP_GAP + GROUP_H
                } else {
                    0.0
                };
                heading + REMINDER_H
            })
            .sum()
    };
    let n = days.len();
    let mut chunks = Vec::new();
    if height(0, n) <= bottom - list_y {
        chunks.push((0, n));
    } else {
        let room = bottom - list_y - LIST_MORE_GAP - MORE_H;
        let mut start = 0;
        while start < n {
            let mut end = start + 1;
            while end < n && height(start, end + 1) <= room {
                end += 1;
            }
            chunks.push((start, end));
            start = end;
        }
    }
    let mut groups = Vec::new();
    let mut rows = Vec::new();
    let mut y = list_y;
    let mut more = None;
    if let Some(&(start, end)) = chunks.get(page % chunks.len().max(1)) {
        for i in start..end {
            if i == start || days[i] != days[i - 1] {
                if i != start {
                    y += GROUP_GAP;
                }
                groups.push((days[i], rect(l, y, r - l, GROUP_H)));
                y += GROUP_H;
            }
            rows.push((reminders[i].id.clone(), rect(l, y, r - l, REMINDER_H)));
            y += REMINDER_H;
        }
        if chunks.len() > 1 {
            more = Some((n - end, rect(l, y + LIST_MORE_GAP, LIST_MORE_W, MORE_H)));
        }
    }
    ListLayout {
        header,
        new_button,
        groups,
        rows,
        more,
    }
}

/// The Music page's progress bar (a thin full-width track) and, under it,
/// the elapsed time at its left and the total at its right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaProgress {
    pub bar: Rect,
    pub elapsed: Rect,
    pub total: Rect,
}

/// The big cover on the left; beside it, centred on it, "SEDANG DIPUTAR",
/// the title over the artist, room for the progress bar and its times
/// (`media_progress_layout`), and the transport row (prev, the round
/// play/pause, next) with "Lirik" at its right end.
pub fn media_panel_layout(s: ShapeRect, strip: bool) -> ExpandedLayout {
    let top = content_top(s, strip);
    let bottom = content_bottom(s);
    let (l, r) = (left(s), right(s));
    let side = COVER.min(bottom - top);
    let art = rect(l, top + (bottom - top - side) / 2.0, side, side);
    let x = art.right() + COVER_GAP;
    let w = r - x;
    let block_h = MEDIA_LABEL_H
        + 6.0
        + MEDIA_TITLE_H
        + 2.0
        + MEDIA_ARTIST_H
        + MEDIA_BAR_GAP
        + MEDIA_PROGRESS_H
        + MEDIA_CONTROLS_GAP
        + MEDIA_PLAY;
    let label = rect(x, art.y + (side - block_h) / 2.0, w, MEDIA_LABEL_H);
    let title = rect(x, label.bottom() + 6.0, w, MEDIA_TITLE_H);
    let artist = rect(x, title.bottom() + 2.0, w, MEDIA_ARTIST_H);
    let row_y = artist.bottom() + MEDIA_BAR_GAP + MEDIA_PROGRESS_H + MEDIA_CONTROLS_GAP;
    let prev = rect(x, row_y, MEDIA_SIDE_W, MEDIA_PLAY);
    let play = rect(
        prev.right() + MEDIA_BUTTON_GAP,
        row_y,
        MEDIA_PLAY,
        MEDIA_PLAY,
    );
    let next = rect(
        play.right() + MEDIA_BUTTON_GAP,
        row_y,
        MEDIA_SIDE_W,
        MEDIA_PLAY,
    );
    let mut lyrics = rect(
        r - LYRICS_W,
        row_y + (MEDIA_PLAY - LYRICS_H) / 2.0,
        LYRICS_W,
        LYRICS_H,
    );
    // The page has no header row, but the Settings button sits at its top
    // right: whatever of the text column would reach it ends left of it.
    let (mut label, mut title, mut artist) = (label, title, artist);
    let btn = settings_button(s, strip);
    let hr = header_right(s);
    let reaches = |q: &Rect| {
        q.x < btn.right() + SETTINGS_GAP
            && q.right() > btn.x - SETTINGS_GAP
            && q.y < btn.bottom() + SETTINGS_GAP
            && q.bottom() > btn.y - SETTINGS_GAP
    };
    for q in [&mut label, &mut title, &mut artist] {
        if reaches(q) {
            q.w = (hr - q.x).max(0.0);
        }
    }
    if reaches(&lyrics) {
        lyrics.x = hr - lyrics.w;
    }
    ExpandedLayout {
        art,
        label,
        title,
        artist,
        buttons: [prev, play, next],
        lyrics,
    }
}

/// The bar under the artist, as wide as the text column, and the times
/// row under it split in two halves (elapsed left, total right-aligned).
pub fn media_progress_layout(l: &ExpandedLayout) -> MediaProgress {
    let (x, w) = (l.artist.x, l.artist.w);
    let bar = rect(x, l.artist.bottom() + MEDIA_BAR_GAP, w, MEDIA_BAR_H);
    let times_y = bar.bottom() + MEDIA_TIMES_GAP;
    let half = w / 2.0;
    MediaProgress {
        bar,
        elapsed: rect(x, times_y, half, MEDIA_TIMES_H),
        total: rect(x + half, times_y, w - half, MEDIA_TIMES_H),
    }
}

/// The title on the left; the chips, Stop and the enlarge button right-
/// aligned on the same row; under them the terminal down to the dots: the
/// output area over the input line. Works for the small and the enlarged
/// panel alike (`s` is the shape drawn).
pub fn cli_layout(s: ShapeRect, strip: bool, head: &CliHead) -> CliLayout {
    let top = if strip {
        waiting_strip(s).bottom() + CLI_STRIP_GAP
    } else {
        s.y + CLI_TOP
    };
    let (l, r) = (left(s), right(s));
    let mid = top + CLI_HEAD_H / 2.0;
    let header = rect(l, top, CLI_TITLE_W, CLI_HEAD_H);
    let expand = rect(
        header_right(s) - CLI_EXPAND,
        mid - CLI_EXPAND / 2.0,
        CLI_EXPAND,
        CLI_EXPAND,
    );
    // Right to left from the button: Stop, then the chips.
    let mut x = expand.x - CLI_CHIP_GAP;
    let stop = head.stop.map(|w| {
        x -= w;
        let r = rect(x, mid - CLI_STOP_H / 2.0, w, CLI_STOP_H);
        x -= CLI_CHIP_GAP;
        r
    });
    let floor = header.right() + CLI_CHIP_GAP;
    // The folder chip takes what the others leave; "dari Explorer" before
    // it goes first when that would cut the folder short.
    let flex = usize::from(head.explorer && head.chips.len() > 1);
    let fixed: f32 = head
        .chips
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != flex)
        .map(|(_, w)| w + CLI_CHIP_GAP)
        .sum();
    let drop_explorer = flex == 1 && x - floor - fixed < head.chips[flex].min(CLI_FOLDER_MIN);
    let mut chips = Vec::with_capacity(head.chips.len());
    for (i, &want) in head.chips.iter().enumerate().rev() {
        let w = if flex == 1 && i == 0 && drop_explorer {
            0.0
        } else if i == flex {
            let before = if flex == 1 && !drop_explorer {
                head.chips[0] + CLI_CHIP_GAP
            } else {
                0.0
            };
            want.min(x - floor - before).max(0.0)
        } else {
            want
        };
        if w <= 0.0 && i < flex {
            chips.push(rect(x, mid - CLI_CHIP_H / 2.0, 0.0, CLI_CHIP_H));
            continue;
        }
        x -= w;
        chips.push(rect(x, mid - CLI_CHIP_H / 2.0, w, CLI_CHIP_H));
        x -= CLI_CHIP_GAP;
    }
    chips.reverse();
    let folder_chips = if head.folder_menu {
        (flex + 1).min(chips.len())
    } else {
        0
    };
    let term_top = header.bottom() + CLI_HEAD_GAP;
    let term = rect(l, term_top, r - l, s.y + s.h - CLI_BOTTOM - term_top);
    let input = rect(term.x, term.bottom() - CLI_INPUT_H, term.w, CLI_INPUT_H);
    let output = rect(
        term.x + CLI_OUT_X,
        term.y + CLI_OUT_TOP,
        term.w - 2.0 * CLI_OUT_X,
        input.y - term.y - CLI_OUT_TOP,
    );
    let rows = (output.h / CLI_LINE_H).floor().max(0.0) as usize;
    let prompt = rect(input.x + CLI_OUT_X, input.y, CLI_PROMPT_W, input.h);
    let field = rect(
        prompt.right(),
        input.y,
        input.right() - CLI_OUT_X - prompt.right(),
        input.h,
    );
    CliLayout {
        header,
        chips,
        folder_chips,
        stop,
        expand,
        term,
        output,
        rows,
        input,
        prompt,
        field,
        menu: None,
    }
}

/// The folder menu under the header, right-aligned under the chips, as
/// wide as `MENU_W` or the terminal. Its lines (`items`) are kept top to
/// bottom while they fit above the terminal's bottom; no more than
/// `MENU_EXPLORER_MAX` Explorer windows in the normal panel, and a section
/// header never stays without its first row.
pub fn cli_folder_menu(l: &CliLayout, items: &[MenuItem]) -> MenuLayout {
    let w = MENU_W.min(l.term.w);
    let x = (l.expand.x - CLI_CHIP_GAP - w).max(l.term.x);
    let top = l.header.bottom() + MENU_TOP_GAP;
    let avail = l.term.bottom() - top - 2.0 * MENU_PAD;
    let explorer_max = if l.term.h > MENU_ROOMY_TERM {
        MENU_EXPLORER_MAX_TALL
    } else {
        MENU_EXPLORER_MAX
    };
    let height = |item: &MenuItem| match item {
        MenuItem::Header(_) => MENU_HEAD_H,
        _ => MENU_ROW_H,
    };
    let mut kept: Vec<usize> = Vec::new();
    let mut used = 0.0;
    let mut explorer = 0;
    let mut i = 0;
    while i < items.len() {
        let item = &items[i];
        let h = height(item);
        match item {
            MenuItem::Header(_) => {
                // With its first row, or not at all.
                let first = items
                    .get(i + 1)
                    .filter(|n| !matches!(n, MenuItem::Header(_)));
                if first.is_none_or(|f| used + h + height(f) > avail) {
                    i += 1;
                    while i < items.len() && !matches!(items[i], MenuItem::Header(_)) {
                        i += 1;
                    }
                    continue;
                }
            }
            MenuItem::Folder {
                section: Section::Explorer,
                ..
            } if explorer >= explorer_max => {
                i += 1;
                continue;
            }
            _ => {}
        }
        if used + h > avail {
            // A row that does not fit ends its section; a later header
            // gets its own chance (and its own check).
            i += 1;
            while i < items.len() && !matches!(items[i], MenuItem::Header(_)) {
                i += 1;
            }
            continue;
        }
        if matches!(
            item,
            MenuItem::Folder {
                section: Section::Explorer,
                ..
            }
        ) {
            explorer += 1;
        }
        kept.push(i);
        used += h;
        i += 1;
    }
    let boxed = rect(x, top, w, used + 2.0 * MENU_PAD);
    let mut y = top + MENU_PAD;
    let mut rows = Vec::with_capacity(kept.len());
    let mut stars = Vec::new();
    let mut hits = Vec::new();
    for i in kept {
        let h = height(&items[i]);
        let r = rect(x + MENU_PAD, y, w - 2.0 * MENU_PAD, h);
        y += h;
        rows.push((i, r));
        if let MenuItem::Folder { section, row } = &items[i] {
            let mut pick = r;
            if *section != Section::Favorites {
                let star = rect(r.right() - MENU_STAR_W, r.y, MENU_STAR_W, r.h);
                pick.w -= MENU_STAR_W;
                stars.push((i, star));
                hits.push(Region {
                    rect: star,
                    hit: Hit::CliFolderStar {
                        path: row.path.clone(),
                        on: !row.favorite,
                    },
                });
            }
            hits.push(Region {
                rect: pick,
                hit: Hit::CliFolderPick {
                    path: row.path.clone(),
                    favorite: *section == Section::Favorites,
                },
            });
        }
    }
    MenuLayout {
        boxed,
        rows,
        stars,
        hits,
    }
}

/// While the folder menu is open, a click anywhere else on the CLI page
/// (header row and terminal) closes it.
pub fn cli_menu_backdrop(l: &CliLayout) -> Rect {
    rect(l.term.x, l.header.y, l.term.w, l.term.bottom() - l.header.y)
}

/// The suggestion box just above the input, `n` rows tall, and its rows.
pub fn cli_suggestions(l: &CliLayout, n: usize) -> (Rect, Vec<Rect>) {
    let h = n as f32 * CLI_SUGGEST_ROW + 2.0 * CLI_SUGGEST_PAD;
    let boxed = rect(
        l.term.x + CLI_SUGGEST_INSET,
        l.input.y - CLI_SUGGEST_GAP - h,
        l.term.w - 2.0 * CLI_SUGGEST_INSET,
        h,
    );
    let rows = (0..n)
        .map(|i| {
            rect(
                boxed.x + CLI_SUGGEST_PAD,
                boxed.y + CLI_SUGGEST_PAD + i as f32 * CLI_SUGGEST_ROW,
                boxed.w - 2.0 * CLI_SUGGEST_PAD,
                CLI_SUGGEST_ROW,
            )
        })
        .collect();
    (boxed, rows)
}

/// Four equal tiles per row, two rows, centred under the header.
/// The title row and the card area of the Clipboard page.
fn clip_frame(s: ShapeRect, strip: bool) -> (Rect, Rect, Rect) {
    let top = content_top(s, strip);
    let (l, r) = (left(s), right(s));
    let hr = header_right(s);
    let search_w = CLIP_SEARCH_MAX_W.min(hr - l - 120.0);
    let search = rect(
        hr - search_w,
        top + (HEADER_H - CLIP_SEARCH_H) / 2.0,
        search_w,
        CLIP_SEARCH_H,
    );
    let header = rect(l, top, search.x - 12.0 - l, HEADER_H);
    let list_y = search.bottom() + CLIP_LIST_GAP;
    let list = rect(l, list_y, r - l, content_bottom(s) - list_y);
    (header, search, list)
}

/// Rows of Clipboard cards the page shows at once.
pub fn clip_rows(s: ShapeRect, strip: bool) -> usize {
    let (_, _, list) = clip_frame(s, strip);
    (((list.h + CLIP_GAP) / (CLIP_CARD_H + CLIP_GAP)).floor() as usize).max(1)
}

/// The furthest the cards scroll (rows) with `n` of them.
pub fn clip_max_scroll(n: usize, rows: usize) -> usize {
    n.div_ceil(2).saturating_sub(rows)
}

/// One wheel notch (`delta`, 120 a notch, positive away from the user) over
/// the cards at row `scroll` of `max`: the row it scrolls to, an end keeping
/// the row (the notch is spent there, so a long scroll never turns the page by
/// accident). `None` when there is nothing to scroll (`max` 0): the notch
/// turns the page instead, as anywhere else on the panel.
pub fn clip_wheel_step(delta: i32, scroll: usize, max: usize) -> Option<usize> {
    if max == 0 {
        return None;
    }
    let rows = (-delta / 120).clamp(-20, 20);
    let rows = if rows == 0 { -delta.signum() } else { rows };
    Some((scroll as i64 + i64::from(rows)).clamp(0, max as i64) as usize)
}

/// The thumb of the Clipboard scrollbar for `n` cards, `rows` shown from row
/// `scroll`; `None` when everything fits. It sits in the page's right margin,
/// as high as the share of the rows that shows (never under 24 px).
pub fn clip_scrollbar(list: Rect, n: usize, rows: usize, scroll: usize) -> Option<Rect> {
    let max = clip_max_scroll(n, rows);
    if max == 0 {
        return None;
    }
    let total = n.div_ceil(2) as f32;
    let h = (list.h * rows as f32 / total).clamp(24.0, list.h);
    let y = list.y + (list.h - h) * scroll.min(max) as f32 / max as f32;
    Some(rect(list.right() + 6.0, y, CLIP_SCROLLBAR_W, h))
}

/// The scrollbar's width.
pub const CLIP_SCROLLBAR_W: f32 = 3.0;

/// The scroll that keeps card `selected` in view: unchanged when it shows,
/// else just enough rows to bring it in.
pub fn clip_scroll_to(selected: usize, scroll: usize, rows: usize) -> usize {
    let row = selected / 2;
    if row < scroll {
        row
    } else if row >= scroll + rows.max(1) {
        row + 1 - rows.max(1)
    } else {
        scroll
    }
}

/// `ids` (the entries shown, newest first) as cards from row `scroll`, two
/// per row; the selected one (`selected`, an id) gets its buttons, and
/// `image(id)` says whether an entry is an image (no plain-text copy).
pub fn clipboard_layout(
    s: ShapeRect,
    strip: bool,
    ids: &[u64],
    selected: Option<u64>,
    scroll: usize,
    image: impl Fn(u64) -> bool,
) -> ClipLayout {
    let (header, search, list) = clip_frame(s, strip);
    let rows = clip_rows(s, strip);
    let scroll = scroll.min(clip_max_scroll(ids.len(), rows));
    let card_w = (list.w - CLIP_GAP) / 2.0;
    let cards: Vec<(u64, usize, Rect)> = ids
        .iter()
        .enumerate()
        .skip(scroll * 2)
        .take(rows * 2)
        .map(|(i, &id)| {
            let (row, col) = ((i / 2 - scroll) as f32, (i % 2) as f32);
            let x = list.x + col * (card_w + CLIP_GAP);
            let y = list.y + row * (CLIP_CARD_H + CLIP_GAP);
            (id, i, rect(x, y, card_w, CLIP_CARD_H))
        })
        .collect();
    let mut buttons = Vec::new();
    if let Some(&(id, _, card)) = cards.iter().find(|(id, _, _)| Some(*id) == selected) {
        let kinds: &[ClipButton] = if image(id) {
            &[ClipButton::Delete, ClipButton::Pin]
        } else {
            &[ClipButton::Delete, ClipButton::Plain, ClipButton::Pin]
        };
        let mut x = card.right() - 5.0 - CLIP_BUTTON;
        for &b in kinds {
            buttons.push((b, id, rect(x, card.y + 4.0, CLIP_BUTTON, CLIP_BUTTON)));
            x -= CLIP_BUTTON + CLIP_BUTTON_GAP;
        }
    }
    ClipLayout {
        header,
        search,
        list,
        rows,
        scroll,
        empty: ids.is_empty().then_some(list),
        scrollbar: clip_scrollbar(list, ids.len(), rows, scroll),
        cards,
        buttons,
    }
}

/// The Notifikasi page's header as the renderer measured it: the title, the
/// "n / max" count, the "Jeda" label and "Hapus semua" (or "Yakin?") with
/// their padding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NotifHead {
    pub title_w: f32,
    pub count_w: f32,
    pub pause_label_w: f32,
    pub clear_w: f32,
}

/// The selected row's three buttons: "Buka", "Kecualikan app ini" and the X.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotifButtons {
    pub open: Rect,
    pub mute: Rect,
    pub remove: Rect,
}

/// One notification row, 44 dip tall: the logo, the app name and the time on
/// the first line, "title — body" on the second.
#[derive(Clone, Debug, PartialEq)]
pub struct NotifRowLayout {
    pub id: u64,
    /// Its place in the list given (not the place on screen).
    pub index: usize,
    pub row: Rect,
    pub logo: Rect,
    pub name: Rect,
    /// Right-aligned, on the first line.
    pub time: Rect,
    /// The unread dot, left of the time.
    pub dot: Rect,
    /// The second line; it stops before the buttons of a selected row.
    pub text: Rect,
    pub buttons: Option<NotifButtons>,
}

/// The Notifikasi page: a header, an optional one-line note, then one column
/// of rows, a row per wheel notch.
#[derive(Clone, Debug, PartialEq)]
pub struct NotifLayout {
    /// "Notifikasi".
    pub header: Rect,
    /// "n / max", right after the title.
    pub count: Option<Rect>,
    /// "Jeda": the whole control, its label and its switch.
    pub pause: Option<Rect>,
    pub pause_label: Option<Rect>,
    pub pause_switch: Option<Rect>,
    /// "Hapus semua", at the right; only with entries.
    pub clear: Option<Rect>,
    /// "Simpan notifikasi mati" under the header while recording is off.
    pub note: Option<Rect>,
    /// Where the rows are; the wheel over it scrolls them.
    pub list: Rect,
    /// Rows that fit, and the scroll applied (rows), kept in range.
    pub rows_fit: usize,
    pub scroll: usize,
    pub rows: Vec<NotifRowLayout>,
    /// Nothing to show (empty, no access): the message's area.
    pub empty: Option<Rect>,
    /// "Buka pengaturan", under the no-access message.
    pub settings_button: Option<Rect>,
    /// Everything under the header's top edge down to the dots: a click on
    /// it that no button or row took gives the page the keyboard.
    pub background: Option<Rect>,
    pub scrollbar: Option<Rect>,
}

pub const NOTIF_ROW_H: f32 = 44.0;
pub const NOTIF_ROW_GAP: f32 = 4.0;
pub const NOTIF_RADIUS: f32 = 12.0;
pub const NOTIF_LOGO: f32 = 24.0;
pub const NOTIF_DOT: f32 = 6.0;
pub const NOTIF_SWITCH_W: f32 = 26.0;
pub const NOTIF_SWITCH_H: f32 = 14.0;
const NOTIF_HEAD_GAP: f32 = 6.0;
const NOTIF_NOTE_H: f32 = 16.0;
const NOTIF_PAD_X: f32 = 10.0;
const NOTIF_TIME_W: f32 = 64.0;
const NOTIF_BTN_H: f32 = 20.0;
const NOTIF_OPEN_W: f32 = 48.0;
const NOTIF_MUTE_W: f32 = 124.0;
const NOTIF_X_W: f32 = 22.0;
const NOTIF_BTN_GAP: f32 = 4.0;
const NOTIF_SETTINGS_W: f32 = 150.0;
const NOTIF_SETTINGS_H: f32 = 26.0;
const NOTIF_COUNT_GAP: f32 = 8.0;
const NOTIF_CONTROL_GAP: f32 = 10.0;

/// The header's height (title row) and what sits under it: the optional
/// note, then the list.
fn notif_list_rect(s: ShapeRect, strip: bool, note: bool) -> Rect {
    let mut top = content_top(s, strip) + DEV_HEAD_H + NOTIF_HEAD_GAP;
    if note {
        top += NOTIF_NOTE_H + NOTIF_HEAD_GAP / 2.0;
    }
    let (l, r) = (left(s), right(s));
    rect(l, top, r - l, content_bottom(s) - top)
}

/// Rows of notifications the page shows at once (whole rows only).
pub fn notif_rows(s: ShapeRect, strip: bool, note: bool) -> usize {
    let list = notif_list_rect(s, strip, note);
    (((list.h + NOTIF_ROW_GAP) / (NOTIF_ROW_H + NOTIF_ROW_GAP)).floor() as usize).max(1)
}

/// The furthest the rows scroll (rows) with `n` of them.
pub fn notif_max_scroll(n: usize, rows: usize) -> usize {
    n.saturating_sub(rows)
}

/// One wheel notch over the rows: `clip_wheel_step`'s rules. `None` when
/// everything fits (the notch turns the page), else the row it scrolls to, an
/// end spending the notch.
pub fn notif_wheel_step(delta: i32, scroll: usize, max: usize) -> Option<usize> {
    clip_wheel_step(delta, scroll, max)
}

/// The scroll that keeps row `selected` in view: unchanged when it shows,
/// else just enough rows to bring it in.
pub fn notif_scroll_to(selected: usize, scroll: usize, rows: usize) -> usize {
    let rows = rows.max(1);
    if selected < scroll {
        selected
    } else if selected >= scroll + rows {
        selected + 1 - rows
    } else {
        scroll
    }
}

/// The row to keep selected once the list scrolled to `scroll` with `rows`
/// shown: row `at` (of `n`) when it still shows, else the nearest that does.
pub fn notif_visible_row(at: usize, scroll: usize, rows: usize, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    at.clamp(scroll, scroll + rows.max(1) - 1).min(n - 1)
}

/// The 3 px thumb of the scrollbar for `n` rows, `rows` shown from `scroll`;
/// `None` when everything fits. In the page's right margin, at least 24 px.
pub fn notif_scrollbar(list: Rect, n: usize, rows: usize, scroll: usize) -> Option<Rect> {
    let max = notif_max_scroll(n, rows);
    if max == 0 {
        return None;
    }
    let h = (list.h * rows as f32 / n as f32).clamp(24.0, list.h);
    let y = list.y + (list.h - h) * scroll.min(max) as f32 / max as f32;
    Some(rect(list.right() + 6.0, y, CLIP_SCROLLBAR_W, h))
}

/// The Notifikasi page for `ids` (the entries listed, newest first), scrolled
/// `scroll` rows; `selected` (an id) gets its buttons. `note` is the "saving
/// is off" line, `no_access` replaces the list with the access message.
#[allow(clippy::too_many_arguments)]
pub fn notif_layout(
    s: ShapeRect,
    strip: bool,
    head: &NotifHead,
    ids: &[u64],
    selected: Option<u64>,
    scroll: usize,
    note: bool,
    no_access: bool,
) -> NotifLayout {
    let top = content_top(s, strip);
    let (l, r) = (left(s), right(s));
    let note = note && !no_access;
    let header = rect(l, top, head.title_w, HEADER_H);
    let list = notif_list_rect(s, strip, note);
    let n = if no_access { 0 } else { ids.len() };
    let rows_fit = notif_rows(s, strip, note);
    let scroll = scroll.min(notif_max_scroll(n, rows_fit));

    // Header controls: none without access; "Hapus semua" only with entries.
    let hr = header_right(s);
    let clear = (n > 0).then(|| rect(hr - head.clear_w, top, head.clear_w, DEV_HEAD_H));
    let pause = (!no_access).then(|| {
        let w = head.pause_label_w + 6.0 + NOTIF_SWITCH_W;
        let right_edge = clear.map_or(hr, |c| c.x - NOTIF_CONTROL_GAP);
        rect(right_edge - w, top, w, DEV_HEAD_H)
    });
    let pause_label = pause.map(|p| rect(p.x, p.y, head.pause_label_w, p.h));
    let pause_switch = pause.map(|p| {
        rect(
            p.right() - NOTIF_SWITCH_W,
            p.y + (p.h - NOTIF_SWITCH_H) / 2.0,
            NOTIF_SWITCH_W,
            NOTIF_SWITCH_H,
        )
    });
    let count = (!no_access).then(|| {
        rect(
            header.right() + NOTIF_COUNT_GAP,
            top + (HEADER_H - DEV_CHIP_H) / 2.0,
            head.count_w,
            DEV_CHIP_H,
        )
    });
    let note_rect = note.then(|| {
        rect(
            l,
            top + DEV_HEAD_H + NOTIF_HEAD_GAP / 2.0,
            r - l,
            NOTIF_NOTE_H,
        )
    });

    let pitch = NOTIF_ROW_H + NOTIF_ROW_GAP;
    let rows: Vec<NotifRowLayout> = ids
        .iter()
        .enumerate()
        .skip(scroll)
        .take(if no_access { 0 } else { rows_fit })
        .map(|(i, &id)| {
            let row = rect(
                list.x,
                list.y + (i - scroll) as f32 * pitch,
                list.w,
                NOTIF_ROW_H,
            );
            let logo = rect(
                row.x + NOTIF_PAD_X,
                row.y + (NOTIF_ROW_H - NOTIF_LOGO) / 2.0,
                NOTIF_LOGO,
                NOTIF_LOGO,
            );
            let text_x = logo.right() + 8.0;
            let time = rect(
                row.right() - NOTIF_PAD_X - NOTIF_TIME_W,
                row.y + 5.0,
                NOTIF_TIME_W,
                16.0,
            );
            let dot = rect(
                time.x - 8.0 - NOTIF_DOT,
                row.y + 5.0 + (16.0 - NOTIF_DOT) / 2.0,
                NOTIF_DOT,
                NOTIF_DOT,
            );
            let name = rect(text_x, row.y + 5.0, dot.x - 6.0 - text_x, 16.0);
            let buttons = (Some(id) == selected).then(|| {
                let y = row.y + 21.0;
                let remove = rect(
                    row.right() - NOTIF_PAD_X - NOTIF_X_W,
                    y,
                    NOTIF_X_W,
                    NOTIF_BTN_H,
                );
                let mute = rect(
                    remove.x - NOTIF_BTN_GAP - NOTIF_MUTE_W,
                    y,
                    NOTIF_MUTE_W,
                    NOTIF_BTN_H,
                );
                let open = rect(
                    mute.x - NOTIF_BTN_GAP - NOTIF_OPEN_W,
                    y,
                    NOTIF_OPEN_W,
                    NOTIF_BTN_H,
                );
                NotifButtons { open, mute, remove }
            });
            // The text ends before the buttons when they show.
            let text_end = buttons.map_or(row.right() - NOTIF_PAD_X, |b| b.open.x - 6.0);
            let text = rect(text_x, row.y + 23.0, text_end - text_x, 16.0);
            NotifRowLayout {
                id,
                index: i,
                row,
                logo,
                name,
                time,
                dot,
                text,
                buttons,
            }
        })
        .collect();

    let empty = (n == 0).then_some(list);
    let settings_button = no_access.then(|| {
        rect(
            list.x + (list.w - NOTIF_SETTINGS_W) / 2.0,
            list.y + list.h / 2.0 + 20.0,
            NOTIF_SETTINGS_W,
            NOTIF_SETTINGS_H,
        )
    });
    NotifLayout {
        header,
        count,
        pause,
        pause_label,
        pause_switch,
        clear,
        note: note_rect,
        list,
        rows_fit,
        scroll,
        rows,
        empty,
        settings_button,
        background: (!no_access).then(|| rect(l, top, r - l, content_bottom(s) - top)),
        scrollbar: notif_scrollbar(list, n, rows_fit, scroll),
    }
}

/// The quick search with `n` results (at most `search::MAX_RESULTS` rows
/// fit under the field, strip or not).
pub fn search_layout(s: ShapeRect, strip: bool, n: usize) -> SearchLayout {
    let top = content_top(s, strip);
    let (l, r) = (left(s), right(s));
    let back = rect(l, top, SEARCH_BACK_W, SEARCH_FIELD_H);
    let fx = back.right() + SEARCH_BACK_GAP;
    let field = rect(fx, top, (header_right(s) - fx).max(0.0), SEARCH_FIELD_H);
    let first = field.bottom() + SEARCH_FIELD_GAP;
    let rows = (0..n.min(crate::search::MAX_RESULTS))
        .map(|i| {
            rect(
                l,
                first + i as f32 * (SEARCH_ROW_H + SEARCH_ROW_GAP),
                r - l,
                SEARCH_ROW_H,
            )
        })
        .collect();
    let empty = (n == 0).then(|| rect(l, first, r - l, (content_bottom(s) - first).max(0.0)));
    SearchLayout {
        back,
        field,
        rows,
        empty,
    }
}

pub fn shortcuts_layout(s: ShapeRect, strip: bool, tiles: &[Tile]) -> ShortcutsLayout {
    let top = content_top(s, strip);
    let bottom = content_bottom(s);
    let (l, r) = (left(s), right(s));
    let header = rect(l, top, header_right(s) - l, HEADER_H);
    let body = header.bottom() + 8.0;
    let w = (r - l - 3.0 * TILE_GAP) / 4.0;
    let rows = tiles.len().div_ceil(4).max(1) as f32;
    let h = ((bottom - body - (rows - 1.0) * TILE_GAP) / rows).min(TILE_MAX_H);
    let y0 = body + ((bottom - body) - (rows * h + (rows - 1.0) * TILE_GAP)) / 2.0;
    let tiles = tiles
        .iter()
        .enumerate()
        .map(|(i, tile)| {
            let (row, col) = ((i / 4) as f32, (i % 4) as f32);
            let x = l + col * (w + TILE_GAP);
            let builtin = match tile {
                Tile::Builtin(s) => Some(*s),
                Tile::Custom(_) => None,
            };
            (builtin, rect(x, y0 + row * (h + TILE_GAP), w, h))
        })
        .collect();
    ShortcutsLayout { header, tiles }
}

/// The parts of a shelf card drawn in `card` with `lines` name lines. The
/// page lays its cards out with it, and the shelf's motion draws a card
/// that is between two places with it.
pub fn shelf_card_layout(card: Rect, lines: usize) -> ShelfCardLayout {
    let group = SHELF_GLYPH_H + SHELF_GAP + lines as f32 * SHELF_LINE_H;
    let remove = rect(card.right() - SHELF_X - 4.0, card.y + 4.0, SHELF_X, SHELF_X);
    let top = card.y + (card.h - group) / 2.0;
    let glyph = rect(card.x, top, card.w, SHELF_GLYPH_H);
    let label = rect(
        card.x + 6.0,
        glyph.bottom() + SHELF_GAP,
        card.w - 12.0,
        lines as f32 * SHELF_LINE_H,
    );
    let missing = rect(card.x + 6.0, card.y + 6.0, 40.0, 15.0);
    ShelfCardLayout {
        card,
        remove,
        glyph,
        label,
        missing,
    }
}

/// The Shelf page for `n` cards (at most twelve). Cards keep the size they
/// have with two rows, so a single file is not a giant tile; they start at
/// the top.
pub fn shelf_layout(s: ShapeRect, strip: bool, n: usize) -> ShelfLayout {
    let top = content_top(s, strip);
    let bottom = content_bottom(s);
    let (l, r) = (left(s), right(s));
    let hr = header_right(s);
    let header = rect(l, top, hr - l - SHELF_CLEAR_W - 8.0, HEADER_H);
    let clear = (n > 0).then(|| rect(hr - SHELF_CLEAR_W, top, SHELF_CLEAR_W, HEADER_H));
    let body = header.bottom() + 8.0;
    let w = (r - l - 3.0 * TILE_GAP) / 4.0;
    let rows = n.div_ceil(4).max(2) as f32;
    let h = ((bottom - body - (rows - 1.0) * TILE_GAP) / rows).min(TILE_MAX_H);
    let lines = if rows > 2.0 { 1 } else { 2 };
    let cards = (0..n)
        .map(|i| {
            let (row, col) = ((i / 4) as f32, (i % 4) as f32);
            shelf_card_layout(
                rect(l + col * (w + TILE_GAP), body + row * (h + TILE_GAP), w, h),
                lines,
            )
        })
        .collect();
    let zone = rect(l, body, r - l, (bottom - body).max(0.0));
    let empty = (n == 0).then_some(zone);
    ShelfLayout {
        header,
        clear,
        zone,
        empty,
        lines,
        cards,
    }
}

/// The Mixer page for `n` rows (at least one): rows of up to
/// `MIXER_ROW_MAX_H`, shrinking so that they all fit above the dots.
pub fn mixer_layout(s: ShapeRect, strip: bool, n: usize) -> MixerLayout {
    let top = content_top(s, strip);
    let bottom = content_bottom(s);
    let (l, r) = (left(s), right(s));
    let header = rect(l, top, header_right(s) - l, HEADER_H);
    let body = header.bottom() + 6.0;
    let n = n.max(1);
    let pitch = ((bottom - body) / n as f32).min(MIXER_ROW_MAX_H);
    let rows = (0..n)
        .map(|i| {
            let y = body + i as f32 * pitch;
            let cy = y + pitch / 2.0;
            let icon = rect(l, cy - MIXER_ICON / 2.0, MIXER_ICON, MIXER_ICON);
            let name = rect(icon.right() + MIXER_GAP, cy - 10.0, MIXER_NAME_W, 20.0);
            let value = rect(r - MIXER_VALUE_W, cy - 10.0, MIXER_VALUE_W, 20.0);
            let track_x = name.right() + MIXER_GAP;
            let track = rect(
                track_x,
                cy - pitch.min(MIXER_ROW_MAX_H) / 2.0 + 2.0,
                value.x - MIXER_GAP - track_x,
                pitch.min(MIXER_ROW_MAX_H) - 4.0,
            );
            MixerRowLayout {
                icon,
                name,
                track,
                value,
            }
        })
        .collect();
    MixerLayout { header, rows }
}

/// The System page for `drives` (fixed drives, in order; as many as fit
/// are shown, fewer under the waiting strip) and `procs` process rows (at
/// most `TOP_PROCESSES`). Room for three rows is kept either way, so the
/// meters do not jump when the first sample arrives.
pub fn system_layout(
    s: ShapeRect,
    strip: bool,
    head: &SystemHead,
    drives: &[char],
    procs: usize,
) -> SystemLayout {
    use crate::system::TOP_PROCESSES;
    let top = content_top(s, strip);
    let bottom = content_bottom(s);
    let (l, r) = (left(s), right(s));

    let clean = rect(
        header_right(s) - head.clean_w,
        top + (SYS_HEAD_H - SYS_CLEAN_H) / 2.0,
        head.clean_w,
        SYS_CLEAN_H,
    );
    let uptime = rect(
        clean.x - SYS_CHIP_GAP - head.uptime_w,
        top + (SYS_HEAD_H - SYS_CHIP_H) / 2.0,
        head.uptime_w,
        SYS_CHIP_H,
    );
    let header = rect(l, top, uptime.x - 8.0 - l, SYS_HEAD_H);
    let body = top + SYS_HEAD_H + SYS_HEAD_GAP;

    // Left: the ring as large as the design allows above its line.
    let ring_d = SYS_RING_D.min(bottom - body - SYS_RING_TOP - SYS_SPARK_GAP - SYS_SPARK_H);
    let cx = l + SYS_LEFT_W / 2.0;
    let cy = body + SYS_RING_TOP + ring_d / 2.0;
    let ring_radius = (ring_d - SYS_RING_STROKE) / 2.0 - 1.0;
    let inner = ring_radius - SYS_RING_STROKE / 2.0 - 4.0;
    let percent = rect(cx - inner, cy - 26.0, 2.0 * inner, SYS_PERCENT_H);
    let caption = rect(cx - inner, cy + 12.0, 2.0 * inner, SYS_CAPTION_H);
    let spark = rect(
        cx - SYS_SPARK_W / 2.0,
        cy + ring_d / 2.0 + SYS_SPARK_GAP,
        SYS_SPARK_W,
        SYS_SPARK_H,
    );

    // Right: meters, then "Paling sibuk" and its rows.
    let x = l + SYS_LEFT_W + SYS_COL_GAP;
    let w = r - x;
    let tail = BUSIEST_GAP + BUSIEST_H + BUSIEST_ROWS_GAP + TOP_PROCESSES as f32 * PROC_H;
    let meters_h = |k: usize| k as f32 * METER_H + (k as f32 - 1.0) * METER_GAP;
    let shown = (0..=drives.len())
        .rev()
        .find(|n| body + meters_h(n + 1) + tail <= bottom + 0.01)
        .unwrap_or(0);
    let meter = |y: f32| MeterLayout {
        row: rect(x, y, w, METER_H),
        name: rect(x, y, METER_NAME_W, METER_HEAD_H),
        value: rect(x + METER_NAME_W, y, w - METER_NAME_W, METER_HEAD_H),
        bar: rect(x, y + METER_HEAD_H + METER_BAR_GAP, w, METER_BAR_H),
    };
    let ram = meter(body);
    let drives: Vec<(char, MeterLayout)> = drives
        .iter()
        .take(shown)
        .enumerate()
        .map(|(i, letter)| {
            (
                *letter,
                meter(body + (i + 1) as f32 * (METER_H + METER_GAP)),
            )
        })
        .collect();
    let meters_bottom = body + meters_h(shown + 1);
    let busiest = rect(x, meters_bottom + BUSIEST_GAP, w, BUSIEST_H);
    let rows_y = busiest.bottom() + BUSIEST_ROWS_GAP;
    let procs = (0..procs.min(TOP_PROCESSES))
        .map(|i| {
            let row = rect(x, rows_y + i as f32 * PROC_H, w, PROC_H);
            let mem = rect(r - PROC_MEM_W, row.y, PROC_MEM_W, PROC_H);
            let cpu = rect(mem.x - PROC_COL_GAP - PROC_CPU_W, row.y, PROC_CPU_W, PROC_H);
            let name = rect(x, row.y, cpu.x - PROC_COL_GAP - x, PROC_H);
            ProcRow {
                row,
                name,
                cpu,
                mem,
            }
        })
        .collect();
    SystemLayout {
        header,
        uptime,
        clean,
        ring_center: (cx, cy),
        ring_radius,
        percent,
        caption,
        spark,
        ram,
        drives,
        busiest,
        procs,
    }
}

/// The cleaner in `s` (`ShapeKind::PanelClean`): `head` and `foot` as the
/// renderer measured them; `rows_live` / `go_live` say what takes clicks.
pub fn cleaner_layout(
    s: ShapeRect,
    strip: bool,
    head: &CleanHead,
    foot: &CleanFoot,
    rows_live: bool,
    go_live: bool,
) -> CleanerLayout {
    let (top, back, header, seg, [first, second]) = clean_head(s, strip, head);
    let (l, r) = (left(s), right(s));

    let foot_y = s.y + s.h - CLEAN_FOOT_BOTTOM - CLEAN_FOOT_H;
    let go = rect(r - foot.go_w, foot_y, foot.go_w, CLEAN_FOOT_H);
    let picked = rect(
        go.x - CLEAN_FOOT_GAP - foot.picked_w,
        foot_y,
        foot.picked_w,
        CLEAN_FOOT_H,
    );
    let credit = rect(l, foot_y, picked.x - 12.0 - l, CLEAN_FOOT_H);

    let rows_top = top + CLEAN_HEAD_H + CLEAN_HEAD_GAP;
    let n = CleanCat::ALL.len() as f32;
    let avail = foot_y - CLEAN_ROWS_FOOT_GAP - rows_top + CLEAN_ROW_GAP;
    let pitch = CLEAN_ROW_PITCH.min(avail / n);
    let rows = CleanCat::ALL
        .iter()
        .enumerate()
        .map(|(i, cat)| {
            let row = rect(l, rows_top + i as f32 * pitch, r - l, pitch - CLEAN_ROW_GAP);
            let mid = row.y + row.h / 2.0;
            let check_x = row.x + CLEAN_ROW_PAD_X + (CLEAN_CHECK_COL - CLEAN_CHECK) / 2.0;
            let check = rect(check_x, mid - CLEAN_CHECK / 2.0, CLEAN_CHECK, CLEAN_CHECK);
            let icon_x = row.x + CLEAN_ROW_PAD_X + CLEAN_CHECK_COL + CLEAN_COL_GAP;
            let icon = rect(icon_x, row.y, CLEAN_ICON_COL, row.h);
            let size = rect(
                row.right() - CLEAN_ROW_PAD_X - CLEAN_SIZE_W,
                row.y,
                CLEAN_SIZE_W,
                row.h,
            );
            let text_x = icon.right() + CLEAN_COL_GAP;
            let text_w = size.x - CLEAN_COL_GAP - text_x;
            let lines = CLEAN_NAME_H + CLEAN_SUB_H;
            let name = rect(text_x, mid - lines / 2.0, text_w, CLEAN_NAME_H);
            let sub = rect(text_x, name.bottom(), text_w, CLEAN_SUB_H);
            CleanRow {
                cat: *cat,
                row,
                check,
                icon,
                name,
                sub,
                size,
            }
        })
        .collect();
    CleanerLayout {
        back,
        header,
        seg,
        seg_items: [first, second],
        rows,
        credit,
        picked,
        go,
        rows_live,
        go_live,
    }
}

/// The cleaner's header, either mode: its top, the back button, the
/// title, the segment's track and its two items.
fn clean_head(s: ShapeRect, strip: bool, head: &CleanHead) -> (f32, Rect, Rect, Rect, [Rect; 2]) {
    let top = s.y + CLEAN_TOP + if strip { STRIP_H } else { 0.0 };
    let l = left(s);
    let back = rect(
        l,
        top + (CLEAN_HEAD_H - CLEAN_BACK) / 2.0,
        CLEAN_BACK,
        CLEAN_BACK,
    );
    let seg_w = head.seg[0] + head.seg[1] + 2.0 * CLEAN_SEG_PAD;
    let seg = rect(
        header_right(s) - seg_w,
        top + (CLEAN_HEAD_H - CLEAN_SEG_H) / 2.0,
        seg_w,
        CLEAN_SEG_H,
    );
    let item_h = CLEAN_SEG_H - 2.0 * CLEAN_SEG_PAD;
    let first = rect(
        seg.x + CLEAN_SEG_PAD,
        seg.y + CLEAN_SEG_PAD,
        head.seg[0],
        item_h,
    );
    let second = rect(first.right(), first.y, head.seg[1], item_h);
    let title_x = back.right() + CLEAN_BACK_GAP;
    let header = rect(title_x, top, seg.x - 8.0 - title_x, CLEAN_HEAD_H);
    (top, back, header, seg, [first, second])
}

/// The cleaner in Mole mode in `s` (`ShapeKind::PanelClean`, as the own
/// mode, so switching does not morph): `head` and `foot` as measured,
/// `look` what shows and takes clicks.
pub fn mole_layout(
    s: ShapeRect,
    strip: bool,
    head: &CleanHead,
    foot: &MoleFoot,
    look: MoleLook,
) -> MoleLayout {
    let (top, back, header, seg, seg_items) = clean_head(s, strip, head);
    let (l, r) = (left(s), right(s));
    let chip_y = top + CLEAN_HEAD_H + MOLE_CHIP_GAP;
    let chip = rect(l, chip_y, foot.chip_w, MOLE_CHIP_H);
    let intro_x = chip.right() + MOLE_CHIP_TEXT_GAP;
    let intro = rect(intro_x, chip_y, r - intro_x, MOLE_CHIP_H);
    let body_top = chip.bottom() + MOLE_BODY_GAP;

    let foot_y = s.y + s.h - CLEAN_FOOT_BOTTOM - CLEAN_FOOT_H;
    let go =
        (look.body != MoleBody::Card).then(|| rect(r - foot.go_w, foot_y, foot.go_w, CLEAN_FOOT_H));
    let label_right = go.map_or(r, |g| g.x - CLEAN_FOOT_GAP);
    let label = rect(
        label_right - foot.label_w,
        foot_y,
        foot.label_w,
        CLEAN_FOOT_H,
    );
    let protect = look
        .protect
        .then(|| rect(l, foot_y, foot.protect_w, CLEAN_FOOT_H));
    let body_bottom = foot_y - MOLE_BODY_GAP;

    let (mut rows, mut opts, mut log, mut result, mut card) =
        (Vec::new(), Vec::new(), None, None, None);
    match look.body {
        MoleBody::Rows => {
            // The options sit 8 px under the rows (`margin: 8px 8px 0`),
            // as high as the footer leaves room for.
            let lowest_opt = body_bottom - MOLE_OPT_H;
            let n = MoleGroup::ALL.len() as f32;
            let room = lowest_opt - MOLE_BODY_GAP - body_top + CLEAN_ROW_GAP;
            let pitch = CLEAN_ROW_PITCH.min(room / n);
            let rows_bottom = body_top + n * pitch - CLEAN_ROW_GAP;
            let opt_y = (rows_bottom + MOLE_BODY_GAP).min(lowest_opt);
            for (i, group) in MoleGroup::ALL.into_iter().enumerate() {
                let row = rect(l, body_top + i as f32 * pitch, r - l, pitch - CLEAN_ROW_GAP);
                let mid = row.y + row.h / 2.0;
                let icon = rect(row.x + CLEAN_ROW_PAD_X, row.y, CLEAN_ICON_COL, row.h);
                let size = rect(
                    row.right() - CLEAN_ROW_PAD_X - CLEAN_SIZE_W,
                    row.y,
                    CLEAN_SIZE_W,
                    row.h,
                );
                let text_x = icon.right() + CLEAN_COL_GAP;
                let text_w = size.x - CLEAN_COL_GAP - text_x;
                let lines = CLEAN_NAME_H + CLEAN_SUB_H;
                let name = rect(text_x, mid - lines / 2.0, text_w, CLEAN_NAME_H);
                let sub = rect(text_x, name.bottom(), text_w, CLEAN_SUB_H);
                rows.push(MoleRow {
                    group,
                    row,
                    icon,
                    name,
                    sub,
                    size,
                });
            }
            let mut x = l + CLEAN_ROW_PAD_X;
            for (i, opt) in [MoleOpt::System, MoleOpt::GameMedia]
                .into_iter()
                .enumerate()
            {
                let check = rect(
                    x,
                    opt_y + (MOLE_OPT_H - CLEAN_CHECK) / 2.0,
                    CLEAN_CHECK,
                    CLEAN_CHECK,
                );
                let label = rect(
                    check.right() + MOLE_OPT_CHECK_GAP,
                    opt_y,
                    foot.opt_w[i],
                    MOLE_OPT_H,
                );
                let hit = rect(check.x, opt_y, label.right() - check.x, MOLE_OPT_H);
                opts.push(MoleOptRow {
                    opt,
                    check,
                    label,
                    hit,
                });
                x = label.right() + MOLE_OPT_GAP;
            }
        }
        MoleBody::Log { result: with } => {
            let mut y = body_top;
            if with {
                result = Some(rect(l, y, r - l, MOLE_RESULT_H));
                y += MOLE_RESULT_H + MOLE_RESULT_GAP;
            }
            log = Some(rect(l, y, r - l, body_bottom - y));
        }
        MoleBody::Card => {
            let boxed = rect(l, body_top + MOLE_CARD_TOP, r - l, MOLE_CARD_H);
            let mid = boxed.y + boxed.h / 2.0;
            let robot = rect(
                boxed.x + MOLE_CARD_PAD,
                mid - MOLE_CARD_ROBOT / 2.0,
                MOLE_CARD_ROBOT,
                MOLE_CARD_ROBOT,
            );
            let button = rect(
                boxed.right() - MOLE_CARD_PAD - foot.howto_w,
                mid - CLEAN_FOOT_H / 2.0,
                foot.howto_w,
                CLEAN_FOOT_H,
            );
            let text_x = robot.right() + MOLE_CARD_GAP;
            let text_w = button.x - MOLE_CARD_GAP - text_x;
            let title = rect(text_x, mid - CLEAN_NAME_H - 1.0, text_w, CLEAN_NAME_H + 2.0);
            let sub = rect(text_x, mid + 1.0, text_w, CLEAN_SUB_H + 2.0);
            card = Some(MoleCard {
                boxed,
                robot,
                title,
                sub,
                button,
            });
        }
    }
    MoleLayout {
        back,
        header,
        seg,
        seg_items,
        chip,
        intro,
        rows,
        opts,
        log,
        result,
        card,
        protect,
        label,
        go,
        look,
    }
}

/// The volume plate's height over the cover and on the compact island.
pub const VOLUME_HUD_H: f32 = 28.0;

/// The parts of the volume plate in `area`: the speaker, the bar and the
/// percentage, all centred on the plate's middle line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeHud {
    pub icon: Rect,
    pub track: Rect,
    pub value: Rect,
}

pub fn volume_hud_layout(area: Rect) -> VolumeHud {
    let pad = 10.0;
    let icon_w = 16.0;
    let value_w = 36.0;
    let gap = 6.0;
    let cy = area.y + area.h / 2.0;
    let icon = rect(area.x + pad, cy - icon_w / 2.0, icon_w, icon_w);
    let value = rect(area.right() - pad - value_w, cy - 8.0, value_w, 16.0);
    let track_x = icon.right() + gap;
    let track = rect(track_x, cy - 2.0, (value.x - gap - track_x).max(0.0), 4.0);
    VolumeHud { icon, track, value }
}

/// A tile's icon and, under it, its label; the two centred in the tile.
pub fn tile_parts(tile: Rect) -> (Rect, Rect) {
    let h = TILE_GLYPH_H + TILE_INNER_GAP + TILE_LABEL_H;
    let y = tile.y + (tile.h - h) / 2.0;
    let glyph = rect(tile.x, y, tile.w, TILE_GLYPH_H);
    let label = rect(
        tile.x + 4.0,
        glyph.bottom() + TILE_INNER_GAP,
        tile.w - 8.0,
        TILE_LABEL_H,
    );
    (glyph, label)
}

/// Small ring on the left (center, radius 8, its left edge `COMPACT_INSET`
/// in), then the time and label.
pub fn pomodoro_compact_layout(s: ShapeRect) -> ((f32, f32), f32, Rect) {
    const RADIUS: f32 = 8.0;
    let center = (
        s.x + crate::ui::geometry::COMPACT_INSET + RADIUS,
        s.y + s.h / 2.0,
    );
    let text_x = center.0 + RADIUS + 10.0;
    let text = rect(text_x, s.y, s.x + s.w - 18.0 - text_x, s.h);
    (center, RADIUS, text)
}

/// With the robot (`shape_with_robot`): its square at the left, then the
/// ring and the text as without it, moved past the robot.
pub fn pomodoro_compact_layout_robot(s: ShapeRect) -> (Rect, (f32, f32), f32, Rect) {
    const RADIUS: f32 = 8.0;
    let robot = crate::ui::geometry::robot_slot(s);
    let center = (robot.right() + 8.0 + RADIUS, s.y + s.h / 2.0);
    let text_x = center.0 + RADIUS + 10.0;
    let text = rect(text_x, s.y, s.x + s.w - 18.0 - text_x, s.h);
    (robot, center, RADIUS, text)
}

/// Clickable regions of a page, plus the waiting strip when it is shown.
/// Clicks inside the panel that miss every region count as `Hit::Body`.
pub fn regions(s: ShapeRect, layout: &PageLayout, strip: bool) -> Vec<Region> {
    let mut out = Vec::new();
    let mut add = |rect: Rect, hit: Hit| out.push(Region { rect, hit });
    if strip {
        add(waiting_strip(s), Hit::Strip);
    }
    // Fixed chrome on every page: first among the page's regions (after the
    // strip), so it wins over underlays such as `NotifFocus`.
    add(settings_button(s, strip), Hit::OpenSettings);
    match layout {
        PageLayout::Calendar(l) => {
            for (view, r) in l.view_buttons {
                add(r, Hit::CalViewSel(view));
            }
            add(l.prev, Hit::CalPrev);
            add(l.next, Hit::CalNext);
            for (d, r) in &l.month_cells {
                add(*r, Hit::CalDate(*d));
            }
            for (d, r) in &l.week_columns {
                let head = Rect {
                    h: WEEK_HEAD_H,
                    ..*r
                };
                add(head, Hit::CalDate(*d));
            }
            for (id, r) in &l.rows {
                add(*r, Hit::CalEvent(id.clone()));
            }
            // Month: opens that day. Day: shows the day's next chunk of events.
            if let Some((_, r)) = l.more {
                add(r, Hit::CalMore(l.selected));
            }
            for (d, _, r) in &l.week_more {
                add(*r, Hit::CalMore(*d));
            }
            add(l.add, Hit::CalAdd);
            if let Some(r) = l.connect {
                add(r, Hit::CalConnect);
            }
        }
        PageLayout::Pomodoro(l) => {
            add(l.start_pause, Hit::PomoStartPause);
            add(l.reset, Hit::PomoReset);
            add(l.skip, Hit::PomoSkip);
        }
        PageLayout::Reminders(l) => {
            add(l.new_button, Hit::ReminderNew);
            for (id, r) in &l.rows {
                add(*r, Hit::ReminderOpen(id.clone()));
            }
            if let Some((_, r)) = l.more {
                add(r, Hit::RemindersMore);
            }
        }
        PageLayout::Notes(l) => {
            add(l.new_button, Hit::NoteNew);
            for (id, r) in &l.rows {
                add(*r, Hit::NoteOpen(id.clone()));
            }
            if let Some((_, r)) = l.more {
                add(r, Hit::NotesMore);
            }
        }
        PageLayout::Media(l) => {
            let [prev, play, next] = l.buttons;
            add(prev, Hit::Prev);
            add(play, Hit::PlayPause);
            add(next, Hit::Next);
            add(l.lyrics, Hit::LyricsToggle);
        }
        PageLayout::Cli(l) => {
            let folder = l.chips[..l.folder_chips].iter().filter(|c| c.w > 0.0);
            if let Some(menu) = &l.menu {
                // The menu over the page; the chips close it again, and the
                // rest of the page underneath does too.
                for r in &menu.hits {
                    add(r.rect, r.hit.clone());
                }
                for chip in folder {
                    add(*chip, Hit::CliFolder);
                }
                add(cli_menu_backdrop(l), Hit::CliMenuDismiss);
            } else {
                for chip in folder {
                    add(*chip, Hit::CliFolder);
                }
                if let Some(stop) = l.stop {
                    add(stop, Hit::CliStop);
                }
                add(l.expand, Hit::CliExpand);
                add(l.term, Hit::CliInput);
            }
        }
        PageLayout::Clipboard(l) => {
            add(l.search, Hit::ClipSearch);
            // The buttons sit on their card: they come first.
            for &(b, id, r) in &l.buttons {
                let hit = match b {
                    ClipButton::Pin => Hit::ClipPin(id),
                    ClipButton::Plain => Hit::ClipPlain(id),
                    ClipButton::Delete => Hit::ClipDelete(id),
                };
                add(r, hit);
            }
            for &(id, _, r) in &l.cards {
                add(r, Hit::ClipCard(id));
            }
        }
        PageLayout::Notifications(l) => {
            if let Some(r) = l.clear {
                add(r, Hit::NotifClear);
            }
            if let Some(r) = l.pause {
                add(r, Hit::NotifPause);
            }
            if let Some(r) = l.settings_button {
                add(r, Hit::NotifSettings);
            }
            // The buttons sit on their row: they come first.
            for row in &l.rows {
                if let Some(b) = row.buttons {
                    add(b.remove, Hit::NotifRemove(row.id));
                    add(b.mute, Hit::NotifMute(row.id));
                    add(b.open, Hit::NotifOpen(row.id));
                }
            }
            for row in &l.rows {
                add(row.row, Hit::NotifRow(row.id));
            }
            // Last: the rest of the page (the list's background, the title)
            // gives it the keyboard.
            if let Some(r) = l.background {
                add(r, Hit::NotifFocus);
            }
        }
        PageLayout::Shortcuts(l) => {
            for (i, (builtin, r)) in l.tiles.iter().enumerate() {
                add(*r, builtin.map_or(Hit::CustomTile(i), Hit::Shortcut));
            }
        }
        PageLayout::Search(l) => {
            add(l.back, Hit::SearchBack);
            for (i, r) in l.rows.iter().enumerate() {
                add(*r, Hit::SearchResult(i));
            }
        }
        PageLayout::Mixer(l) => {
            for (i, row) in l.rows.iter().enumerate() {
                add(row.icon, Hit::MixerMute(i));
                add(row.track, Hit::MixerSlider(i));
            }
        }
        PageLayout::Shelf(l) => {
            if let Some(clear) = l.clear {
                add(clear, Hit::ShelfClear);
            }
            // The X sits on its card: it comes first.
            for (i, c) in l.cards.iter().enumerate() {
                add(c.remove, Hit::ShelfRemove(i));
            }
            for (i, c) in l.cards.iter().enumerate() {
                add(c.card, Hit::ShelfCard(i));
            }
        }
        PageLayout::Arrange(l) => {
            for (i, r) in l.chips.iter().enumerate() {
                add(*r, Hit::ArrangeMonitor(i));
            }
            if let Some(r) = l.skip {
                add(r, Hit::ArrangeSkip);
            }
            // Content scrolled partly out of the list only takes clicks
            // where the list shows it. A play button sits on its row.
            for (i, c) in l.cards.iter().enumerate() {
                if let Some(r) = clipped(c.card, l.list) {
                    add(r, Hit::ArrangeCard(i));
                }
            }
            for (i, c) in l.saved.iter().enumerate() {
                if let Some(r) = clipped(c.play, l.list) {
                    add(r, Hit::ArrangePlay(i));
                }
            }
            for (i, c) in l.windows.iter().enumerate() {
                if let Some(r) = clipped(c.row, l.list) {
                    add(r, Hit::ArrangeWindow(i));
                }
            }
        }
        PageLayout::Devices(l) => {
            if let Some(r) = l.eject_all {
                add(r, Hit::DevicesEjectAll);
            }
            // The buttons of a card scrolled partly out of the list only
            // take clicks where the list shows them.
            for (i, c) in l.cards.iter().enumerate() {
                let mut on_card = vec![
                    (c.eject, Hit::DeviceEject(i)),
                    (c.detail, Hit::DeviceDetail(i)),
                    (c.open, Hit::DeviceOpen(i)),
                ];
                if let Some(v) = &c.veto {
                    on_card.push((v.retry, Hit::DeviceRetry(i)));
                }
                for (r, hit) in on_card {
                    if let Some(r) = clipped(r, l.list) {
                        add(r, hit);
                    }
                }
            }
        }
        PageLayout::Cleaner(l) => {
            add(l.back, Hit::CleanBack);
            // The mode segment, like the rows, not while a clean runs.
            if l.rows_live {
                add(l.seg_items[0], Hit::CleanMode(CleanMode::Own));
                add(l.seg_items[1], Hit::CleanMode(CleanMode::Mole));
            }
            if l.rows_live {
                for row in &l.rows {
                    add(row.row, Hit::CleanToggle(row.cat));
                }
            }
            if l.go_live {
                add(l.go, Hit::CleanGo);
            }
        }
        PageLayout::Mole(l) => {
            add(l.back, Hit::CleanBack);
            let look = l.look;
            if look.seg_live {
                add(l.seg_items[0], Hit::CleanMode(CleanMode::Own));
                add(l.seg_items[1], Hit::CleanMode(CleanMode::Mole));
            }
            if let Some(card) = &l.card {
                add(card.button, Hit::MoleInstall);
            }
            for row in &l.rows {
                if look.unreadable[row.group.index()] {
                    add(row.size, Hit::MoleShowLog);
                }
            }
            if let Some(log) = l.log
                && look.log_toggle
            {
                add(log, Hit::MoleShowLog);
            }
            if look.opts_live {
                for o in &l.opts {
                    add(o.hit, Hit::MoleOpt(o.opt));
                }
            }
            if let Some(p) = l.protect {
                add(p, Hit::MoleProtect);
            }
            if let Some(go) = l.go
                && look.go
            {
                add(go, Hit::MoleGo);
            }
        }
        PageLayout::System(l) => {
            if CLEANER_READY {
                add(l.clean, Hit::SystemClean);
            }
            for (letter, m) in &l.drives {
                add(m.row, Hit::SystemDrive(*letter));
            }
            for p in &l.procs {
                add(p.row, Hit::SystemProcess);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::EventTime;
    use crate::config::Style;
    use crate::ui::geometry::{ShapeKind, shape_contains, shape_for};

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn the_mole_log_window_follows_the_newest_or_the_scroll() {
        let area = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 2.0 * MOLE_LOG_PAD + 6.5 * MOLE_LOG_LINE_H,
        };
        assert_eq!(mole_log_rows(area), 6);
        assert_eq!(mole_log_rows(Rect { h: 5.0, ..area }), 0);
        // The newest rows by default, scrolled up a line at a time, never
        // past the oldest line; a log that fits starts at the top.
        assert_eq!(mole_log_skip(20, 6, 0), 14);
        assert_eq!(mole_log_skip(20, 6, 3), 11);
        assert_eq!(mole_log_skip(20, 6, 14), 0);
        assert_eq!(mole_log_skip(20, 6, 99), 0);
        assert_eq!(mole_log_skip(4, 6, 2), 0);
    }

    fn all_day(id: &str, d: NaiveDate) -> CalEvent {
        CalEvent {
            id: id.into(),
            calendar_id: "primary".into(),
            title: id.into(),
            start: EventTime::AllDay(d),
            end: EventTime::AllDay(d.succ_opt().unwrap()),
            description: String::new(),
            color: None,
            editable: true,
        }
    }

    fn many_events(n: usize, d: NaiveDate) -> Vec<CalEvent> {
        (0..n).map(|i| all_day(&format!("ev{i}"), d)).collect()
    }

    fn at(d: NaiveDate, h: u32, m: u32) -> NaiveDateTime {
        d.and_time(NaiveTime::from_hms_opt(h, m, 0).unwrap())
    }

    fn timed(id: &str, start: NaiveDateTime, end: NaiveDateTime) -> CalEvent {
        use chrono::{Local, TimeZone};
        let local = |t| EventTime::At(Local.from_local_datetime(&t).earliest().unwrap());
        CalEvent {
            start: local(start),
            end: local(end),
            ..all_day(id, start.date())
        }
    }

    /// The title width the tests assume: "Oktober 2026" in DM Sans 600 20,
    /// tracked (127 px), plus the renderer's 1 px of slack.
    const TITLE_W: f32 = 128.0;

    /// The layout without a title width or a clock: tests that only page
    /// or count need neither.
    #[allow(clippy::too_many_arguments)]
    fn calendar_layout(
        s: ShapeRect,
        view: CalView,
        anchor: NaiveDate,
        selected: NaiveDate,
        events: &[CalEvent],
        status: &CalStatus,
        day_page: usize,
        strip: bool,
    ) -> CalendarLayout {
        super::calendar_layout(
            s, view, anchor, selected, events, status, day_page, strip, TITLE_W, None,
        )
    }

    fn reminders_layout(s: ShapeRect, r: &[Reminder], page: usize, strip: bool) -> ListLayout {
        super::reminders_layout(s, r, page, strip, oct25())
    }

    fn notes(n: usize) -> Vec<Note> {
        (0..n)
            .map(|i| Note {
                id: format!("n{i}"),
                text: format!("note {i}"),
                updated: 1000 - i as i64,
            })
            .collect()
    }

    fn reminders(n: usize) -> Vec<Reminder> {
        (0..n)
            .map(|i| Reminder {
                id: format!("r{i}"),
                text: format!("reminder {i}"),
                due_unix: 1000 + i as i64 * 60,
                repeat: Default::default(),
                done: false,
                days: Vec::new(),
            })
            .collect()
    }

    const ARR_HEAD: ArrangeHead = ArrangeHead {
        title_w: 64.0,
        chip_w: Vec::new(),
        skip_w: 96.0,
    };

    /// The Susun page's states: the template step with only the built-ins,
    /// with customs and a long saved list (it scrolls), and the pick step
    /// with few and many windows.
    fn arrange_cases() -> Vec<(&'static str, ArrangeShape, f32)> {
        let step = |cards, saved| ArrangeShape {
            picking: false,
            cards,
            saved,
            windows: 0,
        };
        let pick = |windows| ArrangeShape {
            picking: true,
            cards: 0,
            saved: 0,
            windows,
        };
        vec![
            ("builtins", step(6, 0), 0.0),
            ("with saved", step(6, 2), 0.0),
            ("full", step(18, 12), 0.0),
            ("full scrolled", step(18, 12), 9999.0),
            ("pick none", pick(0), 0.0),
            ("pick 3", pick(3), 0.0),
            ("pick 20", pick(20), 0.0),
            ("pick 20 scrolled", pick(20), 9999.0),
        ]
    }

    fn panel(style: Style) -> ShapeRect {
        shape_for(style, ShapeKind::Panel)
    }

    fn oct25() -> NaiveDate {
        date(2026, 10, 25)
    }

    /// Every page layout worth checking: all calendar views (with overflow,
    /// connect button), pomodoro, notes (overflow) and media.
    fn all_layouts(s: ShapeRect, strip: bool) -> Vec<(String, PageLayout)> {
        let mut events = many_events(20, oct25());
        // Spread some over the week so several week columns overflow.
        for d in 19..=24 {
            events.extend(many_events(5, date(2026, 10, d)).into_iter().map(|mut e| {
                e.id = format!("{}-{d}", e.id);
                e
            }));
        }
        // A busy timed day too: overlapping, short and late events.
        let d = date(2026, 10, 26);
        for (i, (h0, m0, h1, m1)) in [
            (9, 0, 10, 30),
            (9, 30, 11, 0),
            (10, 0, 10, 10),
            (13, 0, 14, 0),
            (13, 0, 13, 5),
            (21, 0, 23, 30),
            (23, 50, 23, 50),
        ]
        .into_iter()
        .enumerate()
        {
            events.push(timed(&format!("t{i}"), at(d, h0, m0), at(d, h1, m1)));
        }
        let mut out = Vec::new();
        for page in [0, 1, 2, 3] {
            for now in [None, Some(at(d, 13, 10)), Some(at(d, 23, 59))] {
                let l = super::calendar_layout(
                    s,
                    CalView::Day,
                    d,
                    d,
                    &events,
                    &CalStatus::NeedsLogin,
                    page,
                    strip,
                    600.0,
                    now,
                );
                out.push((
                    format!("day timed p{page} {now:?}"),
                    PageLayout::Calendar(l),
                ));
            }
        }
        for view in [CalView::Day, CalView::Week, CalView::Month] {
            for status in [CalStatus::Ok, CalStatus::Reconnect] {
                for page in [0, 1, 9] {
                    let l =
                        calendar_layout(s, view, oct25(), oct25(), &events, &status, page, strip);
                    out.push((
                        format!("calendar {view:?} {status:?} p{page}"),
                        PageLayout::Calendar(l),
                    ));
                }
            }
        }
        out.push((
            "pomodoro".into(),
            PageLayout::Pomodoro(pomodoro_layout(s, strip)),
        ));
        for (n, page) in [(0, 0), (3, 0), (30, 0), (30, 7)] {
            let l = notes_layout(s, &notes(n), page, strip);
            out.push((format!("notes {n} p{page}"), PageLayout::Notes(l)));
            let l = reminders_layout(s, &reminders(n), page, strip);
            out.push((format!("reminders {n} p{page}"), PageLayout::Reminders(l)));
        }
        out.push((
            "media".into(),
            PageLayout::Media(media_panel_layout(s, strip)),
        ));
        for head in cli_heads() {
            let l = cli_layout(s, strip, &head);
            out.push((format!("cli {head:?}"), PageLayout::Cli(l)));
        }
        for (n, items) in menu_cases().into_iter().enumerate() {
            let mut l = cli_layout(s, strip, &cli_heads()[4]);
            l.menu = Some(cli_folder_menu(&l, &items));
            out.push((format!("cli menu {n}"), PageLayout::Cli(l)));
        }
        for (n, scroll) in [(0, 0), (5, 0), (48, 0), (48, 30)] {
            let ids: Vec<u64> = (1..=n).collect();
            let l = clipboard_layout(s, strip, &ids, ids.get(4).copied(), scroll, |id| id == 2);
            out.push((format!("clipboard {n} s{scroll}"), PageLayout::Clipboard(l)));
        }
        for n in [1, 2, 5, 6] {
            out.push((
                format!("mixer {n}"),
                PageLayout::Mixer(mixer_layout(s, strip, n)),
            ));
        }
        for n in [0, 1, 5, 9, 12] {
            out.push((
                format!("shelf {n}"),
                PageLayout::Shelf(shelf_layout(s, strip, n)),
            ));
        }
        for n in [0, 1, 3, crate::search::MAX_RESULTS] {
            out.push((
                format!("search {n}"),
                PageLayout::Search(search_layout(s, strip, n)),
            ));
        }
        for n in [1, 5, 8, 12] {
            out.push((
                format!("shortcuts {n}"),
                PageLayout::Shortcuts(shortcuts_layout(s, strip, &sample_tiles(n))),
            ));
        }
        for (drives, procs) in [(&[][..], 0), (&['C', 'D'][..], 3), (&SIX_DRIVES[..], 3)] {
            let l = system_layout(s, strip, &SYS_HEAD, drives, procs);
            out.push((format!("system {drives:?}"), PageLayout::System(l)));
        }
        for (name, shapes, scroll) in devices_cases() {
            let l = devices_layout(s, strip, &DEV_HEAD, &shapes, scroll);
            out.push((format!("devices {name}"), PageLayout::Devices(l)));
        }
        for (name, shape, scroll) in arrange_cases() {
            let l = arrange_layout(s, strip, &ARR_HEAD, shape, scroll);
            out.push((format!("arrange {name}"), PageLayout::Arrange(l)));
        }
        let head = NotifHead {
            title_w: 96.0,
            count_w: 56.0,
            pause_label_w: 30.0,
            clear_w: 100.0,
        };
        for (name, n, selected, scroll, note, no_access) in [
            ("none", 0u64, None, 0, false, false),
            ("three", 3, Some(2), 0, false, false),
            ("many, one lit, scrolled", 40, Some(30), 7, true, false),
            ("many, scrolled past the end", 40, None, 999, false, false),
            ("no access", 3, Some(1), 0, true, true),
        ] {
            let ids: Vec<u64> = (1..=n).rev().collect();
            let l = notif_layout(s, strip, &head, &ids, selected, scroll, note, no_access);
            out.push((
                format!("notifications {name}"),
                PageLayout::Notifications(l),
            ));
        }
        out
    }

    /// A header as the renderer would measure it.
    const DEV_HEAD: DevicesHead = DevicesHead {
        title_w: 96.0,
        chip_w: 84.0,
        eject_all_w: 108.0,
    };

    /// The Devices page with none, one, three (one refused), many and an
    /// expanded card, at the top and scrolled.
    fn devices_cases() -> Vec<(&'static str, Vec<DeviceCardShape>, f32)> {
        let plain = DeviceCardShape::default();
        let veto = DeviceCardShape {
            veto: true,
            details: None,
        };
        let open = |parts| DeviceCardShape {
            veto: false,
            details: Some(parts),
        };
        vec![
            ("none", vec![], 0.0),
            ("one", vec![plain], 0.0),
            ("three, one refused", vec![plain, veto, plain], 0.0),
            ("six", vec![plain; 6], 0.0),
            ("six scrolled", vec![plain; 6], 120.0),
            ("six scrolled past the end", vec![plain; 6], 9999.0),
            ("expanded", vec![plain, open(2), plain], 0.0),
            ("expanded, refused", vec![veto, open(3), veto], 40.0),
        ]
    }

    const SIX_DRIVES: [char; 6] = ['C', 'D', 'E', 'F', 'G', 'H'];
    /// "Uptime 6 j 1 m" and the ghost "Bersihkan · segera", measured.
    const SYS_HEAD: SystemHead = SystemHead {
        uptime_w: 96.0,
        clean_w: 140.0,
    };

    #[test]
    fn three_devices_with_one_refusal_fit_the_panel_without_scrolling() {
        let plain = DeviceCardShape::default();
        let veto = DeviceCardShape {
            veto: true,
            details: None,
        };
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            let (dots, _) = page_dots(s, &Page::ALL, Page::Devices);
            let l = devices_layout(s, false, &DEV_HEAD, &[plain, veto, plain], 0.0);
            assert_eq!(l.max_scroll, 0.0, "{style:?}");
            assert!(l.scrollbar.is_none());
            assert!(l.empty.is_none());
            assert_eq!(l.cards.len(), 3);
            for c in &l.cards {
                assert!(c.card.y >= l.list.y - 0.01, "{style:?} {c:?}");
                assert!(c.card.bottom() <= l.list.bottom() + 0.01, "{style:?} {c:?}");
            }
            assert!(l.list.bottom() <= dots[0].y - 6.0 + 0.01);
            // The refusal's row sits inside its card, under the bar.
            let v = l.cards[1].veto.expect("row");
            assert!(v.row.y >= l.cards[1].bar.bottom());
            assert!(v.row.bottom() <= l.cards[1].card.bottom());
            assert!(v.retry.right() <= v.row.right() && v.text.right() < v.retry.x);
            assert!(l.cards[0].veto.is_none());
            // Under the waiting strip the same three need to scroll.
            let strip = devices_layout(s, true, &DEV_HEAD, &[plain, veto, plain], 0.0);
            assert!(strip.max_scroll > 0.0, "{style:?}");
        }
    }

    #[test]
    fn the_header_holds_the_title_the_chip_and_eject_all() {
        let s = panel(Style::Pill);
        let plain = DeviceCardShape::default();
        let l = devices_layout(s, false, &DEV_HEAD, &[plain, plain], 0.0);
        let chip = l.chip.expect("chip");
        let all = l.eject_all.expect("eject all");
        assert!(l.header.right() < chip.x && chip.right() < all.x);
        assert_eq!((chip.w, all.w, l.header.w), (84.0, 108.0, 96.0));
        assert_eq!(all.right(), header_right(s));
        // Nothing listed: no chip, no button, the message takes the list.
        let none = devices_layout(s, false, &DEV_HEAD, &[], 0.0);
        assert!(none.chip.is_none() && none.eject_all.is_none());
        assert_eq!(none.empty, Some(none.list));
        assert!(none.cards.is_empty());
        let hits: Vec<Hit> = regions(s, &PageLayout::Devices(none), false)
            .into_iter()
            .map(|r| r.hit)
            .collect();
        assert_eq!(hits, [Hit::OpenSettings], "only the Settings button clicks");
    }

    #[test]
    fn more_devices_or_an_expanded_card_scroll_the_list() {
        let s = panel(Style::Pill);
        let plain = DeviceCardShape::default();
        let six = [plain; 6];
        let top = devices_layout(s, false, &DEV_HEAD, &six, 0.0);
        assert!(top.max_scroll > 0.0);
        let thumb = top.scrollbar.expect("thumb");
        assert!(thumb.x > top.list.right() && thumb.y >= top.list.y);
        // Scrolled: the cards move up by it, kept in range.
        let moved = devices_layout(s, false, &DEV_HEAD, &six, 50.0);
        assert_eq!(moved.cards[0].card.y, top.cards[0].card.y - 50.0);
        assert_eq!(moved.scroll, 50.0);
        let end = devices_layout(s, false, &DEV_HEAD, &six, 9999.0);
        assert_eq!(end.scroll, end.max_scroll);
        let last = end.cards.last().unwrap().card;
        assert!((last.bottom() - end.list.bottom()).abs() < 0.01);
        assert!(end.scrollbar.unwrap().bottom() <= end.list.bottom() + 0.01);
        let back = devices_layout(s, false, &DEV_HEAD, &six, -5.0);
        assert_eq!(back.scroll, 0.0);
        // Expanding one card is enough on its own.
        let open = DeviceCardShape {
            veto: false,
            details: Some(2),
        };
        assert!(devices_max_scroll(s, false, &[plain, open, plain]) > 0.0);
        assert_eq!(devices_max_scroll(s, false, &[plain, plain, plain]), 0.0);
        // The card's rows: three fixed ones and a line per partition, in it.
        let l = devices_layout(s, false, &DEV_HEAD, &[plain, open, plain], 0.0);
        let card = &l.cards[1];
        assert_eq!(card.rows.len(), 5);
        for (label, value) in &card.rows {
            assert!(label.y >= card.bar.bottom() && value.bottom() <= card.card.bottom());
            assert!(label.right() <= value.right());
        }
        assert!(l.cards[0].rows.is_empty());
        // Heights: the expanded one is taller by its rows.
        assert_eq!(
            device_card_h(open) - device_card_h(plain),
            DEV_DETAIL_GAP + 5.0 * DEV_DETAIL_ROW
        );
    }

    #[test]
    fn the_buttons_of_a_card_scrolled_half_out_only_click_inside_the_list() {
        let s = panel(Style::Pill);
        let plain = DeviceCardShape::default();
        for scroll in [0.0, 20.0, 45.0, 9999.0] {
            let l = devices_layout(s, false, &DEV_HEAD, &[plain; 6], scroll);
            let hits = regions(s, &PageLayout::Devices(l.clone()), false);
            for r in hits
                .iter()
                .filter(|r| !matches!(r.hit, Hit::DevicesEjectAll | Hit::OpenSettings))
            {
                assert!(
                    r.rect.y >= l.list.y - 0.01 && r.rect.bottom() <= l.list.bottom() + 0.01,
                    "scroll {scroll}: {r:?} outside {:?}",
                    l.list
                );
            }
            // Each card that shows has its three buttons (or what of them shows).
            let ejects = hits
                .iter()
                .filter(|r| matches!(r.hit, Hit::DeviceEject(_)))
                .count();
            assert!(ejects >= 3 && ejects <= 6, "scroll {scroll}: {ejects}");
        }
        // At the top the first card's buttons are whole and map to index 0.
        let l = devices_layout(s, false, &DEV_HEAD, &[plain; 2], 0.0);
        let hits = regions(s, &PageLayout::Devices(l.clone()), false);
        let at = |r: Rect| {
            hits.iter()
                .find(|h| h.rect.contains(r.x + r.w / 2.0, r.y + r.h / 2.0))
                .map(|h| h.hit.clone())
        };
        assert_eq!(at(l.cards[0].eject), Some(Hit::DeviceEject(0)));
        assert_eq!(at(l.cards[0].open), Some(Hit::DeviceOpen(0)));
        assert_eq!(at(l.cards[0].detail), Some(Hit::DeviceDetail(0)));
        assert_eq!(at(l.cards[1].eject), Some(Hit::DeviceEject(1)));
        assert_eq!(at(l.eject_all.unwrap()), Some(Hit::DevicesEjectAll));
        // A refused card's "Coba lagi" asks for its own card again.
        let veto = DeviceCardShape {
            veto: true,
            details: None,
        };
        let l = devices_layout(s, false, &DEV_HEAD, &[plain, veto], 0.0);
        let hits = regions(s, &PageLayout::Devices(l.clone()), false);
        let retry = l.cards[1].veto.unwrap().retry;
        let hit = hits
            .iter()
            .find(|h| h.rect.contains(retry.x + 1.0, retry.y + 1.0))
            .map(|h| h.hit.clone());
        assert_eq!(hit, Some(Hit::DeviceRetry(1)));
    }

    #[test]
    fn an_expanded_card_is_scrolled_into_view_and_a_shown_one_stays() {
        let s = panel(Style::Pill);
        let plain = DeviceCardShape::default();
        let open = DeviceCardShape {
            veto: false,
            details: Some(2),
        };
        let shapes = [plain, plain, open, plain];
        let view = devices_list_rect(s, false).h;
        let max = devices_max_scroll(s, false, &shapes);
        // The card is below the fold: scrolled until its bottom shows.
        let at = devices_reveal(s, false, &shapes, 0.0, 2);
        let top = 2.0 * (device_card_h(plain) + DEV_CARD_GAP);
        let bottom = top + device_card_h(open);
        assert!(at > 0.0 && at <= max);
        assert!(bottom <= at + view + 0.01 || at == max, "bottom shows");
        assert!(at <= top + 0.01, "its top stays in view");
        // Already whole: the scroll stays.
        let shown = devices_reveal(s, false, &shapes, at, 2);
        assert_eq!(shown, at);
        // Scrolled past it (its top above the view): brought back to its top.
        let past = devices_reveal(s, false, &shapes, max, 0);
        assert_eq!(past, 0.0);
        // Out of range, or nothing to scroll: kept in range.
        assert_eq!(devices_reveal(s, false, &shapes, 500.0, 9), max);
        assert_eq!(devices_reveal(s, false, &[plain], 30.0, 0), 0.0);
    }

    #[test]
    fn card_shapes_follow_the_refusals_and_the_expanded_key() {
        use crate::devices::{Bus, Device, DeviceKind, Partition, VetoKind};
        let dev = |key: &str, parts: usize| Device {
            key: key.into(),
            name: key.into(),
            model: key.into(),
            kind: DeviceKind::Hdd,
            bus: Bus::Usb,
            partitions: (0..parts)
                .map(|i| Partition {
                    letter: char::from(b'E' + i as u8),
                    label: String::new(),
                    fs: "NTFS".into(),
                    total: 0,
                    free: None,
                })
                .collect(),
        };
        let list = [dev("A", 1), dev("B", 2), dev("C", 1)];
        let mut vetoes = HashMap::new();
        vetoes.insert(
            "B".to_owned(),
            Veto {
                kind: VetoKind::Device,
                name: String::new(),
            },
        );
        let shapes = device_shapes(&list, &vetoes, Some("B"));
        assert_eq!(
            shapes,
            vec![
                DeviceCardShape::default(),
                DeviceCardShape {
                    veto: true,
                    details: Some(2)
                },
                DeviceCardShape::default(),
            ]
        );
        assert_eq!(
            device_shapes(&list, &HashMap::new(), None),
            vec![DeviceCardShape::default(); 3]
        );
        // A key that is not listed expands nothing.
        assert_eq!(
            device_shapes(&list, &HashMap::new(), Some("Z")),
            vec![DeviceCardShape::default(); 3]
        );
    }

    #[test]
    fn system_page_fits_drives_and_keeps_clear_of_the_dots() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            let (dots, _) = page_dots(s, &Page::ALL, Page::System);
            for (strip, most) in [(false, 3), (true, 2)] {
                let l = system_layout(s, strip, &SYS_HEAD, &SIX_DRIVES, 3);
                // As many drives as fit, in order; fewer under the strip.
                let letters: Vec<char> = l.drives.iter().map(|(c, _)| *c).collect();
                assert_eq!(letters, SIX_DRIVES[..most], "{style:?} strip={strip}");
                let few = system_layout(s, strip, &SYS_HEAD, &['C'], 3);
                assert_eq!(few.drives.len(), 1);
                // Nothing reaches the dots.
                let last = l.procs.last().expect("three rows").row;
                assert_eq!(l.procs.len(), 3);
                assert!(last.bottom() <= dots[0].y - 6.0 + 0.01, "{last:?}");
                assert!(l.spark.bottom() <= dots[0].y - 6.0 + 0.01);
                if strip {
                    assert!(l.header.y >= waiting_strip(s).bottom());
                }
                // Header: title, then the chip and the button, right-aligned.
                assert!(l.header.right() < l.uptime.x);
                assert!(l.uptime.right() < l.clean.x);
                assert_eq!(l.clean.right(), header_right(s));
                assert_eq!((l.uptime.w, l.clean.w), (96.0, 140.0));
                // The 170 px column: the ring (150 px, less under the strip)
                // over its 150 × 34 line; the meters 18 px to its right.
                let (cx, cy) = l.ring_center;
                assert_eq!(cx, left(s) + 85.0);
                let outer = l.ring_radius + SYS_RING_STROKE / 2.0;
                assert!(outer * 2.0 <= 150.0 && outer * 2.0 >= 140.0, "{outer}");
                assert!(cy - outer >= l.header.bottom());
                assert!(l.spark.y >= cy + outer);
                assert_eq!((l.spark.w, l.spark.h), (150.0, 34.0));
                assert!(l.percent.bottom() <= l.caption.y + 0.01);
                assert!(l.percent.w <= 2.0 * (l.ring_radius - SYS_RING_STROKE / 2.0));
                assert_eq!(l.ram.row.x, left(s) + 188.0);
                assert_eq!(l.ram.row.right(), right(s));
                assert_eq!(l.ram.bar.h, 7.0);
                for (_, m) in &l.drives {
                    assert!(m.row.y >= l.ram.row.bottom() + 11.0 - 0.01);
                    assert!(m.name.right() <= m.value.x);
                }
                assert!(l.busiest.y >= l.drives.last().unwrap().1.row.bottom());
                for p in &l.procs {
                    assert!(p.name.right() < p.cpu.x && p.cpu.right() < p.mem.x);
                    assert_eq!(p.mem.right(), right(s));
                }
            }
        }
    }

    /// "Pilih sendiri" and "Otomatis · Mole", "Dipilih 2,3 GB" and
    /// "Yakin? Hapus 2,3 GB", measured with their padding.
    const CLEAN_HEAD: CleanHead = CleanHead { seg: [82.0, 100.0] };
    const CLEAN_FOOT: CleanFoot = CleanFoot {
        picked_w: 76.0,
        go_w: 132.0,
    };

    #[test]
    fn cleaner_fits_its_panel() {
        use crate::ui::geometry::ShapeKind;
        for style in [Style::Pill, Style::Notch] {
            let s = shape_for(style, ShapeKind::PanelClean);
            for strip in [false, true] {
                let l = cleaner_layout(s, strip, &CLEAN_HEAD, &CLEAN_FOOT, true, true);
                let tag = format!("{style:?} strip={strip}");
                // Header: back, title, the segment at the right edge.
                assert!(l.back.right() < l.header.x, "{tag}");
                assert!(l.header.right() < l.seg.x, "{tag}");
                assert_eq!(l.seg.right(), header_right(s));
                assert_eq!(l.seg_items[0].right(), l.seg_items[1].x);
                assert!(l.seg_items[1].right() <= l.seg.right() - 1.99);
                if strip {
                    assert!(l.back.y >= waiting_strip(s).bottom(), "{tag}");
                } else {
                    assert_eq!(l.back.y, s.y + 16.0 + 2.0);
                }
                // Seven rows in order, stacked, clear of the footer.
                let cats: Vec<CleanCat> = l.rows.iter().map(|r| r.cat).collect();
                assert_eq!(cats, CleanCat::ALL);
                for pair in l.rows.windows(2) {
                    assert!(pair[0].row.bottom() <= pair[1].row.y, "{tag}");
                }
                let last = l.rows.last().unwrap().row;
                assert!(last.bottom() + 5.99 <= l.go.y, "{tag}: {last:?}");
                assert!(l.rows[0].row.y >= l.header.bottom() + 9.99);
                if !strip {
                    assert_eq!(l.rows[0].row.h, 33.0, "the design's row");
                }
                for r in &l.rows {
                    // Checkbox, icon, text, size from left to right, the
                    // two lines inside the row.
                    assert!(r.check.right() < r.icon.x && r.icon.right() < r.name.x);
                    assert!(r.name.right() < r.size.x && r.size.right() <= r.row.right());
                    assert_eq!(r.sub.y, r.name.bottom());
                    assert!(r.name.y >= r.row.y && r.sub.bottom() <= r.row.bottom() + 0.01);
                    assert_eq!(r.check.w, CLEAN_CHECK);
                }
                // Footer: credit at the left, "Dipilih" then the button at
                // the right, 26 px over the bottom edge.
                assert_eq!(l.go.right(), right(s));
                assert_eq!(l.go.bottom(), s.y + s.h - 26.0);
                assert!(l.picked.right() < l.go.x && l.credit.right() < l.picked.x);
                assert!(l.credit.w > 200.0, "{tag}: room for the credit");
                for r in regions(s, &PageLayout::Cleaner(l.clone()), strip) {
                    assert!(rect_inside(style, s, r.rect), "{tag}: {r:?}");
                }
            }
        }
    }

    #[test]
    fn cleaner_regions_follow_what_is_live() {
        use crate::ui::geometry::ShapeKind;
        let s = shape_for(Style::Pill, ShapeKind::PanelClean);
        let hits = |rows, go| {
            let l = cleaner_layout(s, false, &CLEAN_HEAD, &CLEAN_FOOT, rows, go);
            regions(s, &PageLayout::Cleaner(l), false)
                .into_iter()
                .map(|r| r.hit)
                .filter(|h| *h != Hit::OpenSettings)
                .collect::<Vec<Hit>>()
        };
        let mut want = vec![
            Hit::CleanBack,
            Hit::CleanMode(CleanMode::Own),
            Hit::CleanMode(CleanMode::Mole),
        ];
        want.extend(CleanCat::ALL.map(Hit::CleanToggle));
        want.push(Hit::CleanGo);
        assert_eq!(hits(true, true), want);
        // While a clean runs only the way back takes a click, not the
        // mode segment either.
        assert_eq!(hits(false, false), [Hit::CleanBack]);
        assert!(!hits(true, false).contains(&Hit::CleanGo));
        let l = cleaner_layout(s, false, &CLEAN_HEAD, &CLEAN_FOOT, true, true);
        let r = regions(s, &PageLayout::Cleaner(l.clone()), false);
        assert_eq!(r[0].hit, Hit::OpenSettings, "the Settings button first");
        for (i, a) in r.iter().enumerate() {
            for b in &r[i + 1..] {
                assert!(disjoint(a.rect, b.rect), "{a:?} overlaps {b:?}");
            }
        }
        let seg: Vec<&Region> = r.iter().filter(|x| l.seg_items.contains(&x.rect)).collect();
        assert_eq!(seg.len(), 2);
        assert_eq!(seg[1].hit, Hit::CleanMode(CleanMode::Mole));
    }

    const MOLE_FOOT: MoleFoot = MoleFoot {
        chip_w: 84.0,
        opt_w: [92.0, 128.0],
        protect_w: 110.0,
        label_w: 104.0,
        go_w: 110.0,
        howto_w: 124.0,
    };

    fn look(body: MoleBody) -> MoleLook {
        MoleLook {
            body,
            seg_live: true,
            opts_live: true,
            protect: true,
            go: true,
            unreadable: [false; 4],
            log_toggle: false,
        }
    }

    #[test]
    fn mole_mode_fits_the_same_panel() {
        use crate::ui::geometry::ShapeKind;
        for style in [Style::Pill, Style::Notch] {
            let s = shape_for(style, ShapeKind::PanelClean);
            for strip in [false, true] {
                let tag = format!("{style:?} strip={strip}");
                let own = cleaner_layout(s, strip, &CLEAN_HEAD, &CLEAN_FOOT, true, true);
                let l = mole_layout(s, strip, &CLEAN_HEAD, &MOLE_FOOT, look(MoleBody::Rows));
                // The header does not move between the modes.
                assert_eq!((l.back, l.header, l.seg), (own.back, own.header, own.seg));
                assert_eq!(l.seg_items, own.seg_items);
                // The chip line under it, then four rows and the options,
                // clear of the footer.
                assert!(l.chip.y >= l.header.bottom() && l.intro.x > l.chip.right());
                let groups: Vec<MoleGroup> = l.rows.iter().map(|r| r.group).collect();
                assert_eq!(groups, MoleGroup::ALL);
                assert!(l.rows[0].row.y >= l.chip.bottom() + 7.99, "{tag}");
                for pair in l.rows.windows(2) {
                    assert!(pair[0].row.bottom() <= pair[1].row.y, "{tag}");
                }
                let go = l.go.expect("button");
                let last = l.rows.last().unwrap().row;
                assert_eq!(l.opts.len(), 2);
                assert!(last.bottom() <= l.opts[0].hit.y, "{tag}");
                assert!(l.opts[0].hit.right() < l.opts[1].hit.x);
                assert!(l.opts[1].hit.bottom() + 7.99 <= go.y, "{tag}");
                for r in &l.rows {
                    assert!(r.icon.right() < r.name.x && r.name.right() < r.size.x);
                    assert!(r.size.right() <= r.row.right());
                }
                // Footer: protect at the left, the label and the button at
                // the right, where the own mode's button is.
                assert_eq!(go.right(), right(s));
                assert_eq!(go.bottom(), s.y + s.h - 26.0);
                let protect = l.protect.expect("protect");
                assert!(protect.x == left(s) && protect.right() < l.label.x);
                assert!(l.label.right() < go.x);
                for body in [
                    MoleBody::Rows,
                    MoleBody::Log { result: true },
                    MoleBody::Log { result: false },
                    MoleBody::Card,
                ] {
                    let l = mole_layout(s, strip, &CLEAN_HEAD, &MOLE_FOOT, look(body));
                    for r in regions(s, &PageLayout::Mole(l.clone()), strip) {
                        assert!(rect_inside(style, s, r.rect), "{tag} {body:?}: {r:?}");
                    }
                    if let Some(log) = l.log {
                        assert!(log.y >= l.chip.bottom() && log.bottom() < l.label.y);
                        assert!(log.h >= 150.0, "{tag}: room for ten lines");
                    }
                    if let (Some(res), Some(log)) = (l.result, l.log) {
                        assert!(res.bottom() <= log.y);
                    }
                    if let Some(card) = l.card {
                        assert!(card.robot.right() < card.title.x);
                        assert!(card.title.right() < card.button.x);
                        assert!(card.button.right() <= card.boxed.right());
                        assert_eq!(l.go, None, "nothing to run without Mole");
                    }
                }
            }
        }
    }

    #[test]
    fn mole_regions_follow_what_is_live() {
        use crate::ui::geometry::ShapeKind;
        let s = shape_for(Style::Pill, ShapeKind::PanelClean);
        let hits = |look: MoleLook| {
            let l = mole_layout(s, false, &CLEAN_HEAD, &MOLE_FOOT, look);
            regions(s, &PageLayout::Mole(l), false)
                .into_iter()
                .map(|r| r.hit)
                .filter(|h| *h != Hit::OpenSettings)
                .collect::<Vec<Hit>>()
        };
        let seg = [
            Hit::CleanMode(CleanMode::Own),
            Hit::CleanMode(CleanMode::Mole),
        ];
        let opts = [
            Hit::MoleOpt(MoleOpt::System),
            Hit::MoleOpt(MoleOpt::GameMedia),
        ];
        // Previewed: the segment, the options, protect and the button.
        let mut want = vec![Hit::CleanBack];
        want.extend(seg.clone());
        want.extend(opts.clone());
        want.extend([Hit::MoleProtect, Hit::MoleGo]);
        assert_eq!(hits(look(MoleBody::Rows)), want);
        // Scanning: no button yet.
        let scanning = MoleLook {
            go: false,
            ..look(MoleBody::Rows)
        };
        assert!(!hits(scanning).contains(&Hit::MoleGo));
        // An unreadable group's "lihat log" opens the log; the log goes back.
        let unreadable = MoleLook {
            unreadable: [false, true, false, false],
            ..look(MoleBody::Rows)
        };
        assert_eq!(
            hits(unreadable)
                .iter()
                .filter(|h| **h == Hit::MoleShowLog)
                .count(),
            1
        );
        let shown = MoleLook {
            log_toggle: true,
            ..look(MoleBody::Log { result: false })
        };
        assert!(hits(shown).contains(&Hit::MoleShowLog));
        // Running: only the way back.
        let running = MoleLook {
            body: MoleBody::Log { result: false },
            seg_live: false,
            opts_live: false,
            protect: false,
            go: false,
            unreadable: [false; 4],
            log_toggle: false,
        };
        assert_eq!(hits(running), [Hit::CleanBack]);
        // Not installed: the segment and "Cara memasang", nothing to run.
        let missing = MoleLook {
            body: MoleBody::Card,
            opts_live: false,
            protect: false,
            go: false,
            ..look(MoleBody::Card)
        };
        let mut want = vec![Hit::CleanBack];
        want.extend(seg);
        want.push(Hit::MoleInstall);
        assert_eq!(hits(missing), want);
        // Regions never overlap.
        let l = mole_layout(s, false, &CLEAN_HEAD, &MOLE_FOOT, unreadable);
        let r = regions(s, &PageLayout::Mole(l), false);
        for (i, a) in r.iter().enumerate() {
            for b in &r[i + 1..] {
                assert!(disjoint(a.rect, b.rect), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn system_regions_per_drive_and_process_and_the_clean_button() {
        let s = panel(Style::Pill);
        let l = system_layout(s, false, &SYS_HEAD, &['C', 'D'], 3);
        let hits = regions(s, &PageLayout::System(l.clone()), false);
        let drives: Vec<(Hit, Rect)> = hits
            .iter()
            .filter(|r| matches!(r.hit, Hit::SystemDrive(_)))
            .map(|r| (r.hit.clone(), r.rect))
            .collect();
        assert_eq!(
            drives,
            [
                (Hit::SystemDrive('C'), l.drives[0].1.row),
                (Hit::SystemDrive('D'), l.drives[1].1.row)
            ]
        );
        let procs: Vec<Rect> = hits
            .iter()
            .filter(|r| r.hit == Hit::SystemProcess)
            .map(|r| r.rect)
            .collect();
        assert_eq!(procs, l.procs.iter().map(|p| p.row).collect::<Vec<_>>());
        // "Bersihkan" opens the cleaner (Run 5).
        const { assert!(CLEANER_READY) };
        let clean: Vec<Rect> = hits
            .iter()
            .filter(|r| r.hit == Hit::SystemClean)
            .map(|r| r.rect)
            .collect();
        assert_eq!(clean, [l.clean]);
        // Before the first sample: no drives, no rows; only the button.
        let empty = system_layout(s, false, &SYS_HEAD, &[], 0);
        let hits = regions(s, &PageLayout::System(empty), false);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].hit, Hit::OpenSettings);
        assert_eq!(hits[1].hit, Hit::SystemClean);
        // With the strip: the strip, then the page's own regions.
        let l = system_layout(s, true, &SYS_HEAD, &['C', 'D', 'E'], 3);
        let hits = regions(s, &PageLayout::System(l), true);
        assert_eq!(hits[0].hit, Hit::Strip);
        assert_eq!(hits[1].hit, Hit::OpenSettings);
        assert_eq!(hits.len(), 1 + 1 + 1 + 2 + 3);
    }

    /// Header chips of the CLI page: folder and shell, running with Stop,
    /// a result, and a folder far too long for the row.
    fn cli_heads() -> Vec<CliHead> {
        vec![
            CliHead {
                chips: vec![150.0, 76.0],
                stop: None,
                explorer: false,
                folder_menu: true,
            },
            CliHead {
                chips: vec![110.0],
                stop: Some(56.0),
                explorer: false,
                folder_menu: false,
            },
            CliHead {
                chips: vec![600.0, 76.0],
                stop: None,
                explorer: false,
                folder_menu: true,
            },
            CliHead::default(),
            // After an Explorer switch: "dari Explorer", folder, shell.
            CliHead {
                chips: vec![104.0, 150.0, 76.0],
                stop: None,
                explorer: true,
                folder_menu: true,
            },
            // The same once a command ended: the result chip too.
            CliHead {
                chips: vec![104.0, 220.0, 120.0, 76.0],
                stop: None,
                explorer: true,
                folder_menu: true,
            },
        ]
    }

    fn folder(section: Section, path: &str, favorite: bool) -> MenuItem {
        MenuItem::Folder {
            section,
            row: crate::cli_folders::MenuRow {
                path: path.into(),
                note: crate::cli_folders::RowNote::None,
                favorite,
            },
        }
    }

    /// The design's menu (2 Explorer, 1 favourite, 1 recent), a crowded one
    /// (8 Explorer windows, 4 favourites, 5 recent) and the empty one.
    fn menu_cases() -> Vec<Vec<MenuItem>> {
        let design = vec![
            MenuItem::Header(Section::Explorer),
            folder(Section::Explorer, r"D:\github\project-a", false),
            folder(Section::Explorer, r"D:\Proyek\rumah-kemang\gambar", false),
            MenuItem::Header(Section::Favorites),
            folder(Section::Favorites, r"D:\github\dynamic-island", true),
            MenuItem::Header(Section::Recent),
            folder(Section::Recent, r"C:\Users\hartono", false),
        ];
        let mut crowded = vec![MenuItem::Header(Section::Explorer)];
        crowded.extend((0..8).map(|i| folder(Section::Explorer, &format!(r"D:\w{i}"), i == 1)));
        crowded.push(MenuItem::Header(Section::Favorites));
        crowded.extend((0..4).map(|i| folder(Section::Favorites, &format!(r"D:\f{i}"), true)));
        crowded.push(MenuItem::Header(Section::Recent));
        crowded.extend((0..5).map(|i| folder(Section::Recent, &format!(r"D:\r{i}"), false)));
        vec![design, crowded, vec![MenuItem::Empty]]
    }

    fn disjoint(a: Rect, b: Rect) -> bool {
        a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y
    }

    /// A page's own regions: `regions` without the Settings button every
    /// page carries (`settings_button_is_clear_of_every_page` covers it).
    fn page_regions(s: ShapeRect, layout: &PageLayout, strip: bool) -> Vec<Region> {
        let mut r = regions(s, layout, strip);
        r.retain(|x| x.hit != Hit::OpenSettings);
        r
    }

    fn rect_inside(style: Style, s: ShapeRect, r: Rect) -> bool {
        const E: f32 = 0.01;
        r.w > 0.0
            && r.h > 0.0
            && [
                (r.x, r.y),
                (r.right() - E, r.y),
                (r.x, r.bottom() - E),
                (r.right() - E, r.bottom() - E),
            ]
            .into_iter()
            .all(|(x, y)| shape_contains(style, s, x, y))
    }

    #[test]
    fn page_dots_centered_and_highlight_index() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for (i, page) in Page::ALL.into_iter().enumerate() {
                let (dots, active) = page_dots(s, &Page::ALL, page);
                assert_eq!(active, Some(i), "{page:?}");
                assert_eq!(dots.len(), Page::ALL.len());
                for d in &dots {
                    assert_eq!((d.w, d.h), (5.0, 5.0));
                    assert_eq!(d.bottom(), s.y + s.h - 11.0);
                }
                for pair in dots.windows(2) {
                    assert_eq!(pair[1].x - pair[0].right(), 6.0);
                }
                let left = dots[0].x - s.x;
                let right = s.x + s.w - dots[dots.len() - 1].right();
                assert_eq!(left, right, "centered");
            }
        }
    }

    #[test]
    fn page_dots_count_only_the_pages_shown() {
        let s = panel(Style::Pill);
        let shown = [Page::Calendar, Page::Notes, Page::Media, Page::Shortcuts];
        let (dots, active) = page_dots(s, &shown, Page::Media);
        assert_eq!(dots.len(), 4);
        assert_eq!(active, Some(2));
        let left = dots[0].x - s.x;
        let right = s.x + s.w - dots[3].right();
        assert_eq!(left, right, "centered");
        // A page opened by its button while it is off lights no dot.
        let (dots, active) = page_dots(s, &shown, Page::Clipboard);
        assert_eq!((dots.len(), active), (4, None));
    }

    #[test]
    fn month_cells_42_starting_monday() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let l = calendar_layout(
            s,
            CalView::Month,
            date(2026, 10, 1),
            date(2026, 10, 25),
            &[],
            &CalStatus::Ok,
            0,
            false,
        );
        assert_eq!(l.month_cells.len(), 42);
        assert_eq!(l.month_cells[0].0, date(2026, 9, 28));
        assert_eq!(l.month_cells[41].0, date(2026, 11, 8));
        for pair in l.month_cells.windows(2) {
            assert_eq!(pair[0].0.succ_opt(), Some(pair[1].0));
        }
        let first = l.month_cells[0].1;
        assert_eq!((first.w, first.h), (36.0, 30.0));
        assert_eq!(first.x, s.x + 20.0);
        assert_eq!(l.month_cells[1].1.x, first.right());
        assert_eq!(l.month_cells[7].1.y, first.bottom() + 2.0);
        assert_eq!(l.month_cells[7].1.x, first.x);
        assert!(first.y >= l.header.bottom() + 6.0);
        // Weekday labels sit over the grid columns.
        assert_eq!(l.weekday_labels.len(), 7);
        for (label, (_, cell)) in l.weekday_labels.iter().zip(&l.month_cells) {
            assert_eq!((label.x, label.w), (cell.x, cell.w));
            assert!(label.bottom() <= cell.y);
        }
        // The list sits right of the grid, past the dividing line.
        assert_eq!(l.list.x, l.month_cells[6].1.right() + 32.0);
        // The arrows follow the title; the selector keeps to the right.
        assert_eq!(l.header.w, TITLE_W);
        assert_eq!(l.prev.x, l.header.right() + 8.0);
        assert!(l.next.x >= l.prev.right());
        assert!(l.next.right() + 12.0 <= l.view_buttons[0].1.x);
        assert_eq!(l.view_buttons[2].1.right() + SEG_PAD, header_right(s));
        // A long title is cut short so nothing overlaps.
        let wide = super::calendar_layout(
            s,
            CalView::Month,
            date(2026, 10, 1),
            date(2026, 10, 25),
            &[],
            &CalStatus::Ok,
            0,
            false,
            900.0,
            None,
        );
        assert!(wide.next.right() + 12.0 <= wide.view_buttons[0].1.x - SEG_PAD + 0.01);
        assert_eq!(wide.offline, None);
        // Offline, a long title is cut shorter still: the tag always shows.
        let offline = super::calendar_layout(
            s,
            CalView::Month,
            date(2026, 10, 1),
            date(2026, 10, 25),
            &[],
            &CalStatus::Offline,
            0,
            false,
            900.0,
            None,
        );
        let tag = offline.offline.expect("offline tag");
        assert!(offline.header.w < wide.header.w);
        assert!(tag.x >= offline.next.right());
        assert!(tag.right() + 12.0 <= offline.view_buttons[0].1.x - SEG_PAD + 0.01);
        assert_eq!(l.list.right(), s.x + s.w - 20.0);
        assert!(l.week_columns.is_empty());
    }

    #[test]
    fn week_columns_monday_to_sunday() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let events = many_events(9, date(2026, 10, 21));
        let l = calendar_layout(
            s,
            CalView::Week,
            oct25(),
            oct25(),
            &events,
            &CalStatus::Ok,
            0,
            false,
        );
        let days: Vec<NaiveDate> = l.week_columns.iter().map(|(d, _)| *d).collect();
        assert_eq!(days.first(), Some(&date(2026, 10, 19)));
        assert_eq!(days.last(), Some(&date(2026, 10, 25)));
        assert_eq!(days.len(), 7);
        let shown = l.rows.len();
        assert!((3..9).contains(&shown), "{shown} chips");
        assert_eq!(l.week_more.len(), 1);
        assert_eq!(l.week_more[0].0, date(2026, 10, 21));
        assert_eq!(l.week_more[0].1, 9 - shown);
        assert!(l.month_cells.is_empty());
        // Chips sit inside their column under its head, the "+N" at its foot.
        let (_, col) = l.week_columns[2];
        for (_, chip) in &l.rows {
            assert!(chip.x > col.x && chip.right() < col.right());
            assert!(chip.y >= col.y + WEEK_HEAD_H && chip.bottom() <= l.week_more[0].2.y);
        }
        assert!(l.week_more[0].2.bottom() <= col.bottom());
        // Few events all fit, all-day bands shorter than timed chips.
        let d = date(2026, 10, 22);
        let mut few = many_events(2, d);
        few.push(timed("t", at(d, 10, 0), at(d, 11, 0)));
        let l = calendar_layout(
            s,
            CalView::Week,
            oct25(),
            oct25(),
            &few,
            &CalStatus::Ok,
            0,
            false,
        );
        assert_eq!((l.rows.len(), l.week_more.len()), (3, 0));
        assert!(l.rows[0].1.h < l.rows[2].1.h);
    }

    #[test]
    fn day_list_overflow_shows_more_count() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let events = many_events(20, oct25());
        let l = calendar_layout(
            s,
            CalView::Day,
            oct25(),
            oct25(),
            &events,
            &CalStatus::Ok,
            0,
            false,
        );
        // Twenty all-day events: two bands a page.
        assert_eq!(l.rows.len(), 2);
        let (hidden, more) = l.more.expect("overflow");
        assert_eq!(hidden, 20 - l.rows.len());
        let (dots, _) = page_dots(s, &Page::ALL, Page::Calendar);
        assert!(l.add.bottom() <= dots[0].y, "everything above the dots");
        assert!(more.y >= l.rows.last().unwrap().1.bottom());
        assert_eq!((more.y, more.h), (l.add.y, l.add.h), "beside the add row");
        for (_, r) in &l.rows {
            assert_eq!(r.h, 22.0);
            assert!(r.bottom() <= l.list.y, "bands above the timeline");
        }
        // Few events: all shown, no "+N".
        let l = calendar_layout(
            s,
            CalView::Day,
            oct25(),
            oct25(),
            &events[..2],
            &CalStatus::Ok,
            0,
            false,
        );
        assert_eq!(l.rows.len(), 2);
        assert_eq!(l.more, None);
    }

    #[test]
    fn connect_only_when_login_needed() {
        let s = panel(Style::Notch);
        let layout =
            |status| calendar_layout(s, CalView::Month, oct25(), oct25(), &[], &status, 0, false);
        assert!(layout(CalStatus::NotConfigured).connect.is_some());
        assert!(layout(CalStatus::NeedsLogin).connect.is_some());
        assert!(layout(CalStatus::Reconnect).connect.is_some());
        assert!(layout(CalStatus::Ok).connect.is_none());
        assert!(layout(CalStatus::Offline).connect.is_none());
    }

    #[test]
    fn connect_button_26_high_in_every_view() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        for view in [CalView::Day, CalView::Week, CalView::Month] {
            let l = calendar_layout(
                s,
                view,
                oct25(),
                oct25(),
                &[],
                &CalStatus::NeedsLogin,
                0,
                false,
            );
            assert_eq!(l.connect.expect("connect").h, 26.0, "{view:?}");
        }
    }

    #[test]
    fn day_list_pages_through_chunks_and_wraps() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let events = many_events(20, oct25());
        let day = |page| {
            calendar_layout(
                s,
                CalView::Day,
                oct25(),
                oct25(),
                &events,
                &CalStatus::Ok,
                page,
                false,
            )
        };
        let first = day(0);
        let k = first.rows.len();
        assert!((1..20).contains(&k));
        assert_eq!(first.rows[0].0, "ev0");
        assert_eq!(first.more.map(|m| m.0), Some(20 - k));
        let second = day(1);
        assert_eq!(second.rows[0].0, format!("ev{k}"));
        assert_eq!(second.rows[0].1, first.rows[0].1, "same row slots");
        let chunks = 20usize.div_ceil(k);
        let last = day(chunks - 1);
        assert_eq!(last.rows.last().unwrap().0, "ev19");
        assert_eq!(last.more.map(|m| m.0), Some(0), "last chunk: back to top");
        assert_eq!(day(chunks).rows[0].0, "ev0", "wraps");
        // The month list always shows the first chunk.
        let month = calendar_layout(
            s,
            CalView::Month,
            oct25(),
            oct25(),
            &events,
            &CalStatus::Ok,
            3,
            false,
        );
        assert_eq!(month.rows[0].0, "ev0");
    }

    #[test]
    fn day_timeline_places_blocks_by_time_and_marks_now() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let d = date(2026, 10, 26);
        let mut events = vec![all_day("expo", d)];
        events.push(timed("review", at(d, 10, 0), at(d, 11, 30)));
        events.push(timed("mep", at(d, 14, 0), at(d, 15, 0)));
        events.push(timed("late", at(d, 21, 0), at(d, 22, 0)));
        let day = |page, now| {
            super::calendar_layout(
                s,
                CalView::Day,
                d,
                d,
                &events,
                &CalStatus::Ok,
                page,
                false,
                200.0,
                now,
            )
        };
        let l = day(0, Some(at(d, 13, 10)));
        // 09.00 to 16.00, as in the design: from an hour before the first event.
        let hours: Vec<u32> = l.hours.iter().map(|(h, _)| *h).collect();
        assert_eq!(hours, (9..=16).collect::<Vec<_>>());
        let line = |h: u32| l.hours.iter().find(|(x, _)| *x == h).unwrap().1;
        for pair in l.hours.windows(2) {
            assert_eq!(pair[1].1 - pair[0].1, 22.0);
        }
        let row = |id: &str| l.rows.iter().find(|(x, _)| x == id).map(|(_, r)| *r);
        let band = row("expo").expect("all-day band");
        assert!(band.bottom() <= line(9));
        let review = row("review").expect("10.00 block");
        assert!((review.y - line(10)).abs() <= 1.0);
        assert!(
            (review.bottom() - (line(11) + 11.0)).abs() <= 1.0,
            "an hour and a half"
        );
        let mep = row("mep").expect("14.00 block");
        assert!((mep.y - line(14)).abs() <= 1.0);
        assert_eq!(row("late"), None, "21.00 is on the next page");
        // The blocks leave the hour labels' gutter free.
        assert!(review.x >= l.list.x + HOUR_GUTTER);
        // Now: between 13.00 and 14.00, a sixth of the way.
        let now = l.now_y.expect("now line");
        assert!((now - (line(13) + 22.0 / 6.0)).abs() < 0.01);
        assert_eq!(l.more.map(|m| m.0), Some(1));
        // Page two shows the late event; the all-day band stays.
        let p1 = day(1, Some(at(d, 13, 10)));
        assert!(p1.rows.iter().any(|(id, _)| id == "late"));
        assert!(p1.rows.iter().any(|(id, _)| id == "expo"));
        assert_eq!(p1.now_y, None, "13.10 is not in these hours");
        assert_eq!(p1.more.map(|m| m.0), Some(0), "last page: back to top");
        assert_eq!(day(2, None).hours, day(0, None).hours, "wraps");
        // Another day has no now line; an evening "now" moves the hours.
        assert_eq!(day(0, None).now_y, None);
        let evening = day(0, Some(at(d, 19, 30)));
        assert!(evening.now_y.is_some());
        assert!(evening.hours[0].0 <= 19 && evening.hours.last().unwrap().0 > 19);
        // Overlapping events go side by side.
        let both = [
            timed("a", at(d, 10, 0), at(d, 11, 0)),
            timed("b", at(d, 10, 30), at(d, 12, 0)),
        ];
        let l = super::calendar_layout(
            s,
            CalView::Day,
            d,
            d,
            &both,
            &CalStatus::Ok,
            0,
            false,
            200.0,
            None,
        );
        let (a, b) = (l.rows[0].1, l.rows[1].1);
        assert!(disjoint(a, b));
        assert_eq!(a.w, b.w);
        assert!(a.right() <= b.x || b.right() <= a.x);
        assert_eq!(l.more, None);
    }

    #[test]
    fn day_lanes_split_only_overlapping_blocks() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let d = date(2026, 10, 26);
        let day = |events: &[CalEvent], page| {
            super::calendar_layout(
                s,
                CalView::Day,
                d,
                d,
                events,
                &CalStatus::Ok,
                page,
                false,
                200.0,
                None,
            )
        };
        let events = [
            timed("a", at(d, 10, 0), at(d, 11, 0)),
            timed("b", at(d, 10, 30), at(d, 12, 0)),
            timed("c", at(d, 14, 0), at(d, 15, 0)),
        ];
        let l = day(&events, 0);
        let row = |id: &str| l.rows.iter().find(|(x, _)| x == id).unwrap().1;
        let (a, b, c) = (row("a"), row("b"), row("c"));
        assert!(disjoint(a, b));
        assert_eq!(c.x, a.x);
        assert!(
            (c.w - (b.right() - a.x)).abs() < 0.01,
            "a lone block keeps the full width"
        );

        // Far more at once than fit side by side: lanes keep a minimum
        // width and the rest come on the next pages, each shown once.
        let crowd: Vec<CalEvent> = (0..100)
            .map(|i| timed(&format!("e{i}"), at(d, 10, 0), at(d, 11, 0)))
            .collect();
        let first = day(&crowd, 0);
        let per_page = first.rows.len();
        assert!(per_page > 1 && per_page < 100);
        let span = |l: &CalendarLayout| {
            let x0 = l.rows.iter().map(|(_, r)| r.x).fold(f32::MAX, f32::min);
            let x1 = l
                .rows
                .iter()
                .map(|(_, r)| r.right())
                .fold(f32::MIN, f32::max);
            (x0, x1)
        };
        let full = span(&first);
        let mut ids = std::collections::BTreeSet::new();
        let mut page = 0;
        loop {
            let l = day(&crowd, page);
            for (id, r) in &l.rows {
                assert!(r.w >= MIN_LANE_W, "{id}: {r:?}");
                ids.insert(id.clone());
            }
            // Every page's run fills the width: no blank lane.
            let (x0, x1) = span(&l);
            assert!((x0 - full.0).abs() < 0.01 && (x1 - full.1).abs() < 0.01);
            let (count, _) = l.more.expect("paged");
            if count == 0 {
                break;
            }
            assert_eq!(count, 100 - ids.len());
            page += 1;
            assert!(page < 100, "pages end");
        }
        assert_eq!(ids.len(), 100);
        assert_eq!(page + 1, 100usize.div_ceil(per_page));
    }

    #[test]
    fn notes_list_caps_rows() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let all = notes(30);
        let l = notes_layout(s, &all, 0, false);
        let k = l.rows.len();
        assert_eq!(k, 4, "two rows of two cards");
        assert_eq!(l.rows[0].0, "n0");
        assert_eq!(l.more.as_ref().map(|m| m.0), Some(30 - k));
        for (_, r) in &l.rows {
            assert_eq!(r.h, 92.0);
        }
        // Two columns: newest top left, then across.
        let r: Vec<Rect> = l.rows.iter().map(|(_, r)| *r).collect();
        assert_eq!((r[0].y, r[1].y), (r[0].y, r[0].y));
        assert_eq!((r[2].x, r[3].x), (r[0].x, r[1].x));
        assert_eq!(r[1].x - r[0].right(), 9.0);
        assert_eq!(r[2].y - r[0].bottom(), 9.0);
        assert_eq!(r[0].x, left(s));
        assert!((r[1].right() - right(s)).abs() < 0.01);
        assert!(l.groups.is_empty());
        let (dots, _) = page_dots(s, &Page::ALL, Page::Notes);
        assert!(l.more.unwrap().1.bottom() <= dots[0].y);

        // The next chunk starts after the first.
        let l1 = notes_layout(s, &all, 1, false);
        assert_eq!(l1.rows[0].0, format!("n{k}"));
        // The last chunk offers "Kembali ke atas" (count 0); one more wraps.
        let chunks = 30usize.div_ceil(k);
        let last = notes_layout(s, &all, chunks - 1, false);
        assert_eq!(last.more.as_ref().map(|m| m.0), Some(0));
        assert_eq!(last.rows.last().unwrap().0, "n29");
        let wrapped = notes_layout(s, &all, chunks, false);
        assert_eq!(wrapped.rows[0].0, "n0");

        // Few notes: no "+N"; none: no rows.
        let l = notes_layout(s, &notes(2), 0, false);
        assert_eq!((l.rows.len(), l.more), (2, None));
        let l = notes_layout(s, &[], 3, false);
        assert_eq!((l.rows.len(), l.more), (0, None));
    }

    #[test]
    fn reminder_list_pages_like_notes() {
        let s = panel(Style::Notch);
        let all = reminders(12);
        let first = reminders_layout(s, &all, 0, false);
        let k = first.rows.len();
        assert!((3..12).contains(&k), "{k} rows");
        assert_eq!(first.rows[0].0, "r0");
        assert_eq!(first.more.as_ref().map(|m| m.0), Some(12 - k));
        let second = reminders_layout(s, &all, 1, false);
        assert_eq!(second.rows[0].0, format!("r{k}"));
        assert_eq!(second.rows[0].1, first.rows[0].1, "same row slots");
        let chunks = 12usize.div_ceil(k);
        let last = reminders_layout(s, &all, chunks - 1, false);
        assert_eq!(last.more.as_ref().map(|m| m.0), Some(0), "back to top");
        assert_eq!(reminders_layout(s, &all, chunks, false).rows[0].0, "r0");
        // Three fit without paging, under one heading (overdue ones join
        // today's group).
        let three = reminders_layout(s, &all[..3], 0, false);
        assert_eq!((three.rows.len(), three.more.is_none()), (3, true));
        assert_eq!(three.groups.len(), 1);
        assert_eq!(three.groups[0].0, oct25());
        assert!(three.groups[0].1.bottom() <= three.rows[0].1.y);
        let notes3 = notes_layout(s, &notes(3), 0, false);
        assert_eq!(three.new_button, notes3.new_button);
        // Every chunk starts with its heading.
        assert_eq!(second.groups[0].1, first.groups[0].1);
    }

    #[test]
    fn reminders_group_by_day_with_overdue_under_today() {
        use chrono::{Local, TimeZone};
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let today = oct25();
        let due = |id: &str, t: NaiveDateTime| Reminder {
            id: id.into(),
            text: id.into(),
            due_unix: Local
                .from_local_datetime(&t)
                .earliest()
                .unwrap()
                .timestamp(),
            repeat: Default::default(),
            done: false,
            days: Vec::new(),
        };
        let all = [
            due("late", at(date(2026, 10, 24), 8, 0)),
            due("today", at(today, 16, 30)),
            due("tomorrow", at(date(2026, 10, 26), 9, 30)),
            due("tomorrow2", at(date(2026, 10, 26), 13, 0)),
            due("friday", at(date(2026, 10, 30), 13, 0)),
        ];
        let l = super::reminders_layout(s, &all, 0, false, today);
        let days: Vec<NaiveDate> = l.groups.iter().map(|(d, _)| *d).collect();
        assert_eq!(days, [today, date(2026, 10, 26), date(2026, 10, 30)]);
        assert_eq!(l.rows.len(), 5);
        assert_eq!(l.more, None);
        // Each heading sits over its first row; later groups a step apart.
        let row = |id: &str| l.rows.iter().find(|(x, _)| x == id).unwrap().1;
        assert_eq!(l.groups[0].1.bottom(), row("late").y);
        assert_eq!(row("today").y, row("late").bottom());
        assert_eq!(l.groups[1].1.bottom(), row("tomorrow").y);
        assert!(l.groups[1].1.y > row("today").bottom());
        assert_eq!(l.groups[2].1.bottom(), row("friday").y);
        let (dots, _) = page_dots(s, &Page::ALL, Page::Reminders);
        assert!(row("friday").bottom() <= dots[0].y);
    }

    #[test]
    fn strip_shifts_content_down_24() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            let strip = waiting_strip(s);
            assert_eq!(strip.h, 24.0);
            assert!(strip.y >= s.y && strip.y < s.y + 14.0, "under the top edge");

            let cal = |strip| {
                calendar_layout(
                    s,
                    CalView::Month,
                    oct25(),
                    oct25(),
                    &[],
                    &CalStatus::Ok,
                    0,
                    strip,
                )
            };
            let (a, b) = (cal(false), cal(true));
            assert_eq!(b.header.y - a.header.y, 24.0);
            assert_eq!(b.month_cells[0].1.y - a.month_cells[0].1.y, 24.0);
            assert!(b.header.y >= strip.bottom());

            let (a, b) = (pomodoro_layout(s, false), pomodoro_layout(s, true));
            assert_eq!(b.header.y - a.header.y, 24.0);
            assert!(b.ring_center.1 > a.ring_center.1);

            let n = notes(3);
            let (a, b) = (notes_layout(s, &n, 0, false), notes_layout(s, &n, 0, true));
            assert_eq!(b.new_button.y - a.new_button.y, 24.0);
            assert_eq!(b.rows[0].1.y - a.rows[0].1.y, 24.0);

            let (a, b) = (media_panel_layout(s, false), media_panel_layout(s, true));
            assert!(b.art.y > a.art.y && b.art.y >= strip.bottom());
            assert!(b.lyrics.y > a.lyrics.y && b.label.y > a.label.y);
            let (pa, pb) = (media_progress_layout(&a), media_progress_layout(&b));
            assert!(pb.bar.y > pa.bar.y && pb.elapsed.y > pa.elapsed.y);
        }
    }

    #[test]
    fn strip_region_maps_to_strip_hit() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let layout = PageLayout::Pomodoro(pomodoro_layout(s, true));
        let with = regions(s, &layout, true);
        assert!(with.contains(&Region {
            rect: waiting_strip(s),
            hit: Hit::Strip
        }));
        let layout = PageLayout::Pomodoro(pomodoro_layout(s, false));
        assert!(
            !regions(s, &layout, false)
                .iter()
                .any(|r| r.hit == Hit::Strip)
        );
    }

    #[test]
    fn regions_map_to_page_hits() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let has = |regions: &[Region], hit: Hit| regions.iter().any(|r| r.hit == hit);

        let events = many_events(20, oct25());
        let l = calendar_layout(
            s,
            CalView::Month,
            oct25(),
            oct25(),
            &events,
            &CalStatus::NeedsLogin,
            0,
            false,
        );
        let r = regions(s, &PageLayout::Calendar(l), false);
        for hit in [
            Hit::CalPrev,
            Hit::CalNext,
            Hit::CalAdd,
            Hit::CalConnect,
            Hit::CalViewSel(CalView::Day),
            Hit::CalViewSel(CalView::Week),
            Hit::CalViewSel(CalView::Month),
            Hit::CalDate(date(2026, 9, 28)),
            Hit::CalEvent("ev0".into()),
            Hit::CalMore(oct25()),
        ] {
            assert!(has(&r, hit.clone()), "{hit:?}");
        }

        let l = calendar_layout(
            s,
            CalView::Week,
            oct25(),
            oct25(),
            &events,
            &CalStatus::Ok,
            0,
            false,
        );
        let r = regions(s, &PageLayout::Calendar(l), false);
        assert!(has(&r, Hit::CalDate(date(2026, 10, 19))));
        assert!(has(&r, Hit::CalMore(oct25())));

        // Day view's "+N lagi" pages through the open day.
        let l = calendar_layout(
            s,
            CalView::Day,
            oct25(),
            oct25(),
            &events,
            &CalStatus::Ok,
            0,
            false,
        );
        assert!(l.more.is_some());
        let r = regions(s, &PageLayout::Calendar(l), false);
        assert!(has(&r, Hit::CalMore(oct25())));

        let r = regions(s, &PageLayout::Pomodoro(pomodoro_layout(s, false)), false);
        for hit in [Hit::PomoStartPause, Hit::PomoReset, Hit::PomoSkip] {
            assert!(has(&r, hit.clone()), "{hit:?}");
        }
        // Durations are set in the settings window only.
        assert!(!r.iter().any(|r| matches!(r.hit, Hit::PomoAdjust(..))));

        let r = regions(
            s,
            &PageLayout::Notes(notes_layout(s, &notes(30), 0, false)),
            false,
        );
        for hit in [Hit::NoteNew, Hit::NoteOpen("n0".into()), Hit::NotesMore] {
            assert!(has(&r, hit.clone()), "{hit:?}");
        }

        let r = regions(
            s,
            &PageLayout::Reminders(reminders_layout(s, &reminders(12), 0, false)),
            false,
        );
        for hit in [
            Hit::ReminderNew,
            Hit::ReminderOpen("r0".into()),
            Hit::RemindersMore,
        ] {
            assert!(has(&r, hit.clone()), "{hit:?}");
        }
        assert!(!r.iter().any(|r| matches!(r.hit, Hit::NoteOpen(_))));

        let media = media_panel_layout(s, false);
        let r = regions(s, &PageLayout::Media(media), false);
        for hit in [Hit::Prev, Hit::PlayPause, Hit::Next, Hit::LyricsToggle] {
            assert!(has(&r, hit.clone()), "{hit:?}");
        }
        // The Settings button and the four buttons: no lyric offset region
        // is left.
        assert_eq!(r.len(), 5);
        // "Lirik" ends at the text column's right edge, clear of the transport
        // buttons.
        assert_eq!(
            media.lyrics.x + media.lyrics.w,
            media.title.x + media.title.w
        );
        assert!(media.buttons.iter().all(|b| b.x + b.w <= media.lyrics.x));
        // The progress bar shows the position only: no click lands on it.
        let bar = media_progress_layout(&media).bar;
        let mid = (bar.x + bar.w / 2.0, bar.y + bar.h / 2.0);
        assert!(!r.iter().any(|r| r.rect.contains(mid.0, mid.1)));

        let idle = cli_layout(s, false, &cli_heads()[0]);
        let r = page_regions(s, &PageLayout::Cli(idle.clone()), false);
        let region = |rect, hit| Region { rect, hit };
        assert_eq!(
            r,
            [
                region(idle.chips[0], Hit::CliFolder),
                region(idle.expand, Hit::CliExpand),
                region(idle.term, Hit::CliInput)
            ]
        );
        // While a command runs the run's chip opens nothing.
        let running = cli_layout(s, false, &cli_heads()[1]);
        let r = page_regions(s, &PageLayout::Cli(running.clone()), false);
        assert_eq!(r[0], region(running.stop.unwrap(), Hit::CliStop));
        assert_eq!(r.len(), 3);
        // After an Explorer switch both chips open the menu.
        let explorer = cli_layout(s, false, &cli_heads()[4]);
        let r = page_regions(s, &PageLayout::Cli(explorer.clone()), false);
        assert_eq!(r[0], region(explorer.chips[0], Hit::CliFolder));
        assert_eq!(r[1], region(explorer.chips[1], Hit::CliFolder));
        assert_eq!(r.len(), 4);

        let tiles = sample_tiles(ORIGINAL_NINE);
        let r = page_regions(
            s,
            &PageLayout::Shortcuts(shortcuts_layout(s, false, &tiles)),
            false,
        );
        let hits: Vec<Hit> = r.iter().map(|r| r.hit.clone()).collect();
        let want: Vec<Hit> = Shortcut::ALL[..ORIGINAL_NINE]
            .iter()
            .copied()
            .map(Hit::Shortcut)
            .collect();
        assert_eq!(hits, want);
        // A custom tile answers with its place among the tiles shown.
        let builtin = ORIGINAL_NINE;
        let mixed = sample_tiles(builtin + 2);
        let r = page_regions(
            s,
            &PageLayout::Shortcuts(shortcuts_layout(s, false, &mixed)),
            false,
        );
        assert_eq!(r[builtin - 1].hit, Hit::Shortcut(Shortcut::Lock));
        assert_eq!(r[builtin].hit, Hit::CustomTile(builtin));
        assert_eq!(r[builtin + 1].hit, Hit::CustomTile(builtin + 1));
    }

    #[test]
    fn the_volume_plate_fits_the_cover_strip_and_the_compact_island() {
        // The strip over the cover, and a compact island's height.
        for area in [
            rect(20.0, 100.0, 134.0, VOLUME_HUD_H),
            rect(6.0, 4.0, 188.0, 28.0),
        ] {
            let l = volume_hud_layout(area);
            for r in [l.icon, l.track, l.value] {
                assert!(r.x >= area.x && r.right() <= area.right() + 0.01, "{r:?}");
                assert!(r.y >= area.y && r.bottom() <= area.bottom(), "{r:?}");
            }
            assert!(l.icon.right() <= l.track.x && l.track.right() <= l.value.x);
            assert!(l.track.w >= 40.0, "the bar has room: {}", l.track.w);
        }
    }

    /// The built-in tiles of the first versions, ahead of the page tiles.
    const ORIGINAL_NINE: usize = 9;

    /// `n` tiles: the nine original built-in ones first, then custom ones.
    fn sample_tiles(n: usize) -> Vec<Tile> {
        use crate::tiles::{CustomKind, CustomTile};
        (0..n)
            .map(|i| match Shortcut::ALL[..ORIGINAL_NINE].get(i) {
                Some(s) => Tile::Builtin(*s),
                None => Tile::Custom(
                    CustomTile::new(&format!("T{i}"), CustomKind::App, r"C:\a.exe").expect("valid"),
                ),
            })
            .collect()
    }

    #[test]
    fn page_and_settings_tiles_go_through_the_built_in_path() {
        use crate::model::Page;
        let go = [
            Shortcut::Lock,
            Shortcut::Go(Page::Shelf),
            Shortcut::Go(Page::Calendar),
            Shortcut::Go(Page::Cli),
        ];
        let tiles: Vec<Tile> = go.iter().map(|s| Tile::Builtin(*s)).collect();
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            let l = shortcuts_layout(s, false, &tiles);
            let order: Vec<Option<Shortcut>> = l.tiles.iter().map(|(t, _)| *t).collect();
            assert_eq!(order, go.map(Some));
            let r = regions(s, &PageLayout::Shortcuts(l), false);
            let hits: Vec<Hit> = r
                .iter()
                .map(|r| r.hit.clone())
                .filter(|h| *h != Hit::OpenSettings)
                .collect();
            let want: Vec<Hit> = go.into_iter().map(Hit::Shortcut).collect();
            assert_eq!(hits, want);
        }
    }

    #[test]
    fn shortcut_tiles_four_to_a_row_in_order() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for strip in [false, true] {
                let l = shortcuts_layout(s, strip, &sample_tiles(ORIGINAL_NINE));
                let order: Vec<Option<Shortcut>> = l.tiles.iter().map(|(t, _)| *t).collect();
                let want: Vec<Option<Shortcut>> = Shortcut::ALL[..ORIGINAL_NINE]
                    .iter()
                    .copied()
                    .map(Some)
                    .collect();
                assert_eq!(order, want);
                let first = l.tiles[0].1;
                assert!(first.h >= 60.0, "{style:?} strip={strip}: {first:?}");
                assert!(first.y >= l.header.bottom());
                for (i, (_, t)) in l.tiles.iter().enumerate() {
                    assert_eq!((t.w, t.h), (first.w, first.h), "equal tiles");
                    let (row, col) = (i / 4, i % 4);
                    assert_eq!(t.y, l.tiles[row * 4].1.y, "rows line up");
                    assert_eq!(t.x, l.tiles[col].1.x, "columns line up");
                    let (glyph, label) = tile_parts(*t);
                    assert!(glyph.y >= t.y && label.bottom() <= t.bottom());
                    assert!(glyph.bottom() <= label.y);
                }
                // Edge to edge across the content width, 10 px apart.
                assert_eq!(first.x, left(s));
                assert!((l.tiles[3].1.right() - right(s)).abs() < 0.01);
                assert_eq!(l.tiles[1].1.x - first.right(), 10.0);
                assert_eq!(l.tiles[4].1.y - first.bottom(), 10.0);
            }
        }
    }

    #[test]
    fn shelf_cards_wrap_four_to_a_row_and_hit_by_place() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for strip in [false, true] {
                for n in [0, 1, 4, 5, 12] {
                    let l = shelf_layout(s, strip, n);
                    assert_eq!(l.cards.len(), n);
                    assert_eq!(l.clear.is_some(), n > 0);
                    assert_eq!(l.empty.is_some(), n == 0);
                    assert!(l.header.right() < l.clear.map_or(f32::MAX, |c| c.x));
                    for (i, c) in l.cards.iter().enumerate() {
                        assert!(c.card.y >= l.header.bottom(), "{n}: under the header");
                        assert!(c.card.bottom() <= content_bottom(s) + 0.01, "{n}: inside");
                        assert_eq!(c.card.x, l.cards[i % 4].card.x, "{n}: columns line up");
                        assert_eq!(c.card.w, l.cards[0].card.w);
                        assert_eq!(c.card.h, l.cards[0].card.h);
                        assert!(c.card.h >= 56.0, "{style:?} strip={strip}: {:?}", c.card);
                        // The icon and the name sit inside the card, the icon
                        // over the name, and neither under the X.
                        assert!(c.glyph.y >= c.card.y && c.label.bottom() <= c.card.bottom());
                        assert!(c.glyph.bottom() <= c.label.y);
                        assert!(c.card.contains(c.label.x, c.label.y));
                        assert!(c.remove.y >= c.card.y && c.remove.right() <= c.card.right());
                        assert!(c.missing.right() <= c.remove.x, "the tag clears the X");
                        assert_eq!(l.lines, if n > 8 { 1 } else { 2 }, "{n}");
                        assert!(c.card.bottom() <= l.zone.bottom() + 0.01);
                    }
                    assert!(l.zone.y >= l.header.bottom());
                    assert!(l.zone.bottom() <= content_bottom(s) + 0.01);
                    for pair in l.cards.windows(2) {
                        let (a, b) = (pair[0].card, pair[1].card);
                        let same_row = (a.y - b.y).abs() < 0.01;
                        assert!(!same_row || a.right() <= b.x, "{n}: no overlap in a row");
                    }
                }
            }
        }
        let s = panel(Style::Pill);
        let l = shelf_layout(s, false, 3);
        let r = regions(s, &PageLayout::Shelf(l), false);
        let hits: Vec<Hit> = r.iter().map(|r| r.hit.clone()).collect();
        assert_eq!(
            hits,
            [
                Hit::OpenSettings,
                Hit::ShelfClear,
                Hit::ShelfRemove(0),
                Hit::ShelfRemove(1),
                Hit::ShelfRemove(2),
                Hit::ShelfCard(0),
                Hit::ShelfCard(1),
                Hit::ShelfCard(2),
            ]
        );
        // An empty shelf has no buttons.
        let empty = shelf_layout(s, false, 0);
        let none: Vec<Hit> = regions(s, &PageLayout::Shelf(empty), false)
            .into_iter()
            .map(|r| r.hit)
            .collect();
        assert_eq!(none, [Hit::OpenSettings]);
    }

    #[test]
    fn mixer_rows_fit_above_the_dots_in_order_and_hit_by_place() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for strip in [false, true] {
                for n in [1, 3, 6] {
                    let l = mixer_layout(s, strip, n);
                    assert_eq!(l.rows.len(), n);
                    let mut last = l.header.bottom();
                    for row in &l.rows {
                        assert!(row.icon.y >= last - 0.01, "{n}: rows in order");
                        assert!(row.track.bottom() <= content_bottom(s) + 0.01);
                        assert!(row.icon.right() < row.name.x);
                        assert!(row.name.right() < row.track.x);
                        assert!(row.track.right() < row.value.x);
                        assert!(row.track.w >= 100.0, "{n}: a usable track");
                        assert!(row.track.h >= MIXER_TRACK_H);
                        last = row.track.y;
                    }
                }
            }
        }
        let s = panel(Style::Pill);
        let l = mixer_layout(s, false, 3);
        let r = regions(s, &PageLayout::Mixer(l.clone()), false);
        let hits: Vec<Hit> = r.iter().map(|r| r.hit.clone()).collect();
        assert_eq!(
            hits,
            [
                Hit::OpenSettings,
                Hit::MixerMute(0),
                Hit::MixerSlider(0),
                Hit::MixerMute(1),
                Hit::MixerSlider(1),
                Hit::MixerMute(2),
                Hit::MixerSlider(2),
            ]
        );
    }

    #[test]
    fn shortcut_tiles_wrap_to_rows_and_fit_up_to_twelve() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for strip in [false, true] {
                for n in [1, 4, 5, 9, 12] {
                    let l = shortcuts_layout(s, strip, &sample_tiles(n));
                    assert_eq!(l.tiles.len(), n);
                    let first = l.tiles[0].1;
                    for (i, (_, t)) in l.tiles.iter().enumerate() {
                        assert_eq!((t.w, t.h), (first.w, first.h), "{n}: equal tiles");
                        assert!(t.y >= l.header.bottom(), "{n}: under the header");
                        assert!(t.bottom() <= content_bottom(s) + 0.01, "{n}: inside");
                        assert_eq!(t.x, l.tiles[i % 4].1.x, "{n}: columns line up");
                        let (glyph, label) = tile_parts(*t);
                        assert!(
                            glyph.y >= t.y - 0.01 && label.bottom() <= t.bottom() + 0.01,
                            "{style:?} strip={strip} n={n}: icon and label inside {t:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn cli_terminal_follows_the_design_in_both_sizes() {
        for style in [Style::Pill, Style::Notch] {
            for kind in [ShapeKind::Panel, ShapeKind::PanelTall] {
                let s = shape_for(style, kind);
                let head = &cli_heads()[0];
                let l = cli_layout(s, false, head);
                // 16 px over the 28 px header, 8 under it, 24 under the
                // terminal: 214 px small, 364 enlarged.
                assert_eq!(l.header.y, s.y + 16.0);
                assert_eq!(l.term.y, l.header.bottom() + 8.0);
                assert_eq!(l.term.bottom(), s.y + s.h - 24.0);
                let want = if kind == ShapeKind::Panel {
                    214.0
                } else {
                    364.0
                };
                assert_eq!(l.term.h, want, "{style:?} {kind:?}");
                assert_eq!((l.term.x, l.term.right()), (left(s), right(s)));
                // The input line along the bottom, the output above it.
                assert_eq!(l.input.h, 44.0);
                assert_eq!(l.input.bottom(), l.term.bottom());
                assert!(l.output.bottom() <= l.input.y);
                assert!(l.output.y >= l.term.y + 10.0);
                assert!(l.rows as f32 * CLI_LINE_H <= l.output.h);
                assert!(l.rows >= if kind == ShapeKind::Panel { 8 } else { 17 });
                assert!(l.prompt.right() <= l.field.x);
                assert!(l.field.right() <= l.input.right() - 14.0);
                // Chips and the button on the header row, right-aligned, in order.
                assert_eq!(l.expand.right(), header_right(s));
                assert_eq!(l.expand.w, 26.0);
                let mut edge = l.header.right();
                for c in &l.chips {
                    assert!(c.x >= edge, "{c:?}");
                    edge = c.right();
                }
                assert!(edge < l.expand.x);
                assert_eq!(l.chips[0].w, head.chips[0]);
                // The dots stay clear.
                let (dots, _) = page_dots(s, &Page::ALL, Page::Cli);
                assert!(l.term.bottom() < dots[0].y);
            }
            let s = panel(style);
            // The waiting strip pushes it all down.
            let (a, b) = (
                cli_layout(s, false, &cli_heads()[0]),
                cli_layout(s, true, &cli_heads()[0]),
            );
            assert!(b.header.y >= waiting_strip(s).bottom());
            assert!(b.header.y > a.header.y && b.term.bottom() == a.term.bottom());
            // A long folder gives way to the shell chip and the button.
            let long = cli_layout(s, false, &cli_heads()[2]);
            assert!(long.chips[0].x >= long.header.right());
            assert!(long.chips[0].w < 600.0 && long.chips[0].w > 100.0);
            assert_eq!(long.chips[1].w, 76.0);
            // Stop sits right of the state chip, before the button.
            let run = cli_layout(s, false, &cli_heads()[1]);
            let stop = run.stop.unwrap();
            assert!(run.chips[0].right() < stop.x && stop.right() < run.expand.x);
        }
    }

    #[test]
    fn the_explorer_chip_gives_way_before_the_folder() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            // Idle: room for all three, the folder whole.
            let idle = cli_layout(s, false, &cli_heads()[4]);
            assert_eq!(
                idle.chips.iter().map(|c| c.w).collect::<Vec<_>>(),
                [104.0, 150.0, 76.0]
            );
            assert_eq!(idle.folder_chips, 2);
            assert!(idle.chips[0].x >= idle.header.right());
            assert!(idle.chips[0].right() < idle.chips[1].x);
            // Ended with a long folder: "dari Explorer" goes, the folder
            // keeps what is left.
            let ended = cli_layout(s, false, &cli_heads()[5]);
            assert_eq!(ended.chips[0].w, 0.0, "dropped");
            assert!(ended.chips[1].w >= CLI_FOLDER_MIN, "{:?}", ended.chips[1]);
            assert!(ended.chips[1].x >= ended.header.right());
            assert_eq!((ended.chips[2].w, ended.chips[3].w), (120.0, 76.0));
            let r = regions(s, &PageLayout::Cli(ended.clone()), false);
            assert_eq!(
                r.iter().filter(|r| r.hit == Hit::CliFolder).count(),
                1,
                "a dropped chip takes no click"
            );
        }
    }

    #[test]
    fn folder_menu_hangs_under_the_header_on_the_right() {
        for style in [Style::Pill, Style::Notch] {
            for kind in [ShapeKind::Panel, ShapeKind::PanelTall] {
                let s = shape_for(style, kind);
                let l = cli_layout(s, false, &cli_heads()[4]);
                let design = &menu_cases()[0];
                let m = cli_folder_menu(&l, design);
                // 300 px, just under the header, ending where the chips end.
                assert_eq!(m.boxed.w, MENU_W);
                assert!(m.boxed.y >= l.header.bottom() && m.boxed.y < l.term.y + 4.0);
                assert_eq!(m.boxed.right(), l.expand.x - 6.0);
                assert!(m.boxed.bottom() <= l.term.bottom());
                // The design's menu fits whole: 3 headers and 4 rows.
                assert_eq!(m.rows.len(), design.len(), "{style:?} {kind:?}");
                assert_eq!(
                    m.boxed.h,
                    3.0 * MENU_HEAD_H + 4.0 * MENU_ROW_H + 2.0 * MENU_PAD
                );
                for w in m.rows.windows(2) {
                    assert_eq!(w[0].1.bottom(), w[1].1.y, "rows stack");
                }
                // Explorer and recent rows have a star; the favourite not.
                assert_eq!(
                    m.stars.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
                    [1, 2, 6]
                );
                let picks: Vec<&Hit> = m
                    .hits
                    .iter()
                    .map(|r| &r.hit)
                    .filter(|h| matches!(h, Hit::CliFolderPick { .. }))
                    .collect();
                assert_eq!(picks.len(), 4);
                assert!(m.hits.contains(&Region {
                    rect: m.rows[4].1,
                    hit: Hit::CliFolderPick {
                        path: r"D:\github\dynamic-island".into(),
                        favorite: true
                    }
                }));
                assert!(m.hits.iter().any(|r| r.hit
                    == Hit::CliFolderStar {
                        path: r"D:\github\project-a".into(),
                        on: true
                    }));
            }
        }
        // A narrow terminal: the menu takes its width and stays inside it.
        let mut l = cli_layout(panel(Style::Pill), false, &cli_heads()[0]);
        l.term.w = 240.0;
        let m = cli_folder_menu(&l, &menu_cases()[0]);
        assert_eq!(m.boxed.w, 240.0);
        assert!(m.boxed.x >= l.term.x);
    }

    #[test]
    fn a_crowded_menu_is_trimmed_to_the_panel() {
        let crowded = &menu_cases()[1];
        for style in [Style::Pill, Style::Notch] {
            for kind in [ShapeKind::Panel, ShapeKind::PanelTall] {
                let s = shape_for(style, kind);
                for strip in [false, true] {
                    let l = cli_layout(s, strip, &cli_heads()[4]);
                    let m = cli_folder_menu(&l, crowded);
                    assert!(m.boxed.bottom() <= l.term.bottom() + 0.01);
                    let kept: Vec<&MenuItem> = m.rows.iter().map(|(i, _)| &crowded[*i]).collect();
                    let explorer = kept
                        .iter()
                        .filter(|i| {
                            matches!(
                                i,
                                MenuItem::Folder {
                                    section: Section::Explorer,
                                    ..
                                }
                            )
                        })
                        .count();
                    let want = if kind == ShapeKind::Panel { 3 } else { 6 };
                    assert_eq!(explorer, want, "{style:?} {kind:?} strip={strip}");
                    // Every kept header has its first row right under it.
                    for (k, item) in kept.iter().enumerate() {
                        if matches!(item, MenuItem::Header(_)) {
                            assert!(
                                matches!(kept.get(k + 1), Some(MenuItem::Folder { .. })),
                                "{style:?} {kind:?} strip={strip}: header {k} alone"
                            );
                        }
                    }
                    // The favourites follow the windows.
                    assert!(kept.contains(&&MenuItem::Header(Section::Favorites)));
                }
            }
        }
        // The empty menu: one quiet line.
        let l = cli_layout(panel(Style::Pill), false, &cli_heads()[0]);
        let m = cli_folder_menu(&l, &[MenuItem::Empty]);
        assert_eq!(m.rows.len(), 1);
        assert!(m.hits.is_empty() && m.stars.is_empty());
    }

    #[test]
    fn an_open_menu_takes_the_page_clicks() {
        for style in [Style::Pill, Style::Notch] {
            for kind in [ShapeKind::Panel, ShapeKind::PanelTall] {
                let s = shape_for(style, kind);
                let mut l = cli_layout(s, false, &cli_heads()[4]);
                let m = cli_folder_menu(&l, &menu_cases()[0]);
                l.menu = Some(m.clone());
                let mut r = regions(s, &PageLayout::Cli(l.clone()), false);
                // The Settings button leads, over the menu and its backdrop.
                assert_eq!(r.remove(0).hit, Hit::OpenSettings);
                // Menu first, then the folder chips, then the backdrop.
                assert_eq!(r[..m.hits.len()], m.hits[..]);
                assert_eq!(r[m.hits.len()].hit, Hit::CliFolder);
                assert_eq!(r[m.hits.len() + 1].hit, Hit::CliFolder);
                let last = r.last().unwrap();
                assert_eq!(last.hit, Hit::CliMenuDismiss);
                assert!(
                    !r.iter()
                        .any(|r| matches!(r.hit, Hit::CliInput | Hit::CliExpand | Hit::CliStop))
                );
                // The terminal and the enlarge button now close the menu.
                let first_hit = |x: f32, y: f32| {
                    r.iter()
                        .find(|r| r.rect.contains(x, y))
                        .map(|r| r.hit.clone())
                };
                let (tx, ty) = (l.term.x + 10.0, l.term.bottom() - 10.0);
                assert_eq!(first_hit(tx, ty), Some(Hit::CliMenuDismiss));
                let e = l.expand;
                assert_eq!(
                    first_hit(e.x + e.w / 2.0, e.y + e.h / 2.0),
                    Some(Hit::CliMenuDismiss)
                );
                // A row is a pick, its star a star.
                let (_, row) = m.rows[1];
                assert!(matches!(
                    first_hit(row.x + 20.0, row.y + row.h / 2.0),
                    Some(Hit::CliFolderPick { .. })
                ));
                assert!(matches!(
                    first_hit(row.right() - 4.0, row.y + row.h / 2.0),
                    Some(Hit::CliFolderStar { .. })
                ));
            }
        }
    }

    #[test]
    fn cli_suggestions_sit_just_above_the_input() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let l = cli_layout(s, false, &cli_heads()[0]);
        let (boxed, rows) = cli_suggestions(&l, 3);
        assert_eq!(rows.len(), 3);
        assert_eq!(boxed.bottom(), l.input.y - 6.0);
        assert_eq!(
            (boxed.x, boxed.right()),
            (l.term.x + 10.0, l.term.right() - 10.0)
        );
        assert!(boxed.y >= l.term.y, "three rows fit the small terminal");
        for r in &rows {
            assert!(r.y >= boxed.y && r.bottom() <= boxed.bottom());
        }
        assert!(cli_suggestions(&l, 0).1.is_empty());
    }

    #[test]
    fn the_search_back_button_sits_left_of_the_field_without_touching_it() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for strip in [false, true] {
                for n in [0, 3, crate::search::MAX_RESULTS] {
                    let l = search_layout(s, strip, n);
                    assert!(l.back.right() < l.field.x, "a gap, no overlap");
                    assert_eq!(l.back.x, left(s));
                    assert_eq!(l.field.right(), header_right(s));
                    assert!(l.back.w >= 32.0 && l.back.h >= SEARCH_FIELD_H);
                    assert_eq!((l.back.y, l.back.h), (l.field.y, l.field.h));
                    assert!(l.back.y >= content_top(s, strip));
                    assert!(l.field.w > 100.0, "the field keeps its room");
                    for row in &l.rows {
                        assert!(row.y >= l.back.bottom() && row.y >= l.field.bottom());
                    }
                    // Registered once.
                    let hits: Vec<_> = regions(s, &PageLayout::Search(l.clone()), strip)
                        .into_iter()
                        .map(|r| r.hit)
                        .collect();
                    assert_eq!(hits.iter().filter(|h| **h == Hit::SearchBack).count(), 1);
                }
            }
        }
    }

    #[test]
    fn regions_have_no_overlaps_within_page() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for strip in [false, true] {
                for (name, layout) in all_layouts(s, strip) {
                    let mut r = regions(s, &layout, strip);
                    // The open menu's backdrop is the underlay: it comes
                    // last, so everything over it is found first.
                    if let Some(i) = r.iter().position(|r| r.hit == Hit::CliMenuDismiss) {
                        assert_eq!(i, r.len() - 1, "{name}: the backdrop is last");
                        r.pop();
                    }
                    // A Shelf card's X sits on it and comes first.
                    if let PageLayout::Shelf(l) = &layout {
                        let first = r.iter().position(|x| matches!(x.hit, Hit::ShelfRemove(_)));
                        if let Some(i) = first {
                            let cards = r.iter().position(|x| matches!(x.hit, Hit::ShelfCard(_)));
                            assert!(cards.is_none_or(|c| c > i), "{name}: X before cards");
                            for (n, c) in l.cards.iter().enumerate() {
                                assert!(
                                    c.remove.x >= c.card.x && c.remove.bottom() <= c.card.bottom(),
                                    "{name}: X {n} inside its card"
                                );
                                assert!(c.remove.right() <= c.card.right());
                            }
                            r.drain(i..i + l.cards.len());
                        }
                    }
                    // A Clipboard card's buttons sit on it and come first;
                    // the cards themselves must not overlap anything else.
                    if let PageLayout::Clipboard(l) = &layout {
                        let buttons = l.buttons.len();
                        let first = r.iter().position(|x| {
                            matches!(
                                x.hit,
                                Hit::ClipPin(_) | Hit::ClipPlain(_) | Hit::ClipDelete(_)
                            )
                        });
                        if let Some(i) = first {
                            let button = l.buttons[0].2;
                            let host = l.cards.iter().find(|c| c.0 == l.buttons[0].1).unwrap().2;
                            assert!(
                                button.x >= host.x && button.bottom() <= host.bottom(),
                                "{name}"
                            );
                            let cards = r.iter().position(|x| matches!(x.hit, Hit::ClipCard(_)));
                            assert!(cards.is_none_or(|c| c > i), "{name}: buttons before cards");
                            r.drain(i..i + buttons);
                        }
                    }
                    // A Notifikasi row's buttons sit on it, and the page's
                    // background lies under everything (a click nothing else
                    // took gives the page the keyboard): both overlap by design.
                    if let PageLayout::Notifications(l) = &layout {
                        for row in &l.rows {
                            if let Some(b) = row.buttons {
                                for part in [b.open, b.mute, b.remove] {
                                    assert!(
                                        part.x >= row.row.x && part.right() <= row.row.right(),
                                        "{name}"
                                    );
                                    assert!(
                                        part.y >= row.row.y && part.bottom() <= row.row.bottom(),
                                        "{name}"
                                    );
                                }
                            }
                        }
                        let first = |pred: fn(&Hit) -> bool| r.iter().position(|x| pred(&x.hit));
                        let rows = first(|h| matches!(h, Hit::NotifRow(_)));
                        let buttons = first(|h| matches!(h, Hit::NotifOpen(_)));
                        assert!(buttons.is_none_or(|b| rows.is_none_or(|w| b < w)), "{name}");
                        r.retain(|x| {
                            !matches!(
                                x.hit,
                                Hit::NotifOpen(_)
                                    | Hit::NotifMute(_)
                                    | Hit::NotifRemove(_)
                                    | Hit::NotifFocus
                            )
                        });
                    }
                    for (i, a) in r.iter().enumerate() {
                        for b in &r[i + 1..] {
                            assert!(
                                disjoint(a.rect, b.rect),
                                "{style:?} strip={strip} {name}: {a:?} overlaps {b:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn all_regions_inside_panel() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            let (dots, _) = page_dots(s, &Page::ALL, Page::Calendar);
            for &d in &dots {
                assert!(rect_inside(style, s, d), "{style:?} dot {d:?}");
            }
            for strip in [false, true] {
                for (name, layout) in all_layouts(s, strip) {
                    for r in regions(s, &layout, strip) {
                        assert!(
                            rect_inside(style, s, r.rect),
                            "{style:?} strip={strip} {name}: {r:?} outside"
                        );
                        if r.hit != Hit::Strip {
                            assert!(r.rect.bottom() <= dots[0].y, "{name}: {r:?} over the dots");
                        }
                    }
                }
            }
        }
    }

    /// Every rect a page draws as text or a control (not its click underlays),
    /// the shared collection for the "inside the panel" and the "clear of the
    /// Settings button" checks. Lists that scroll reach past the panel
    /// (clipped to `list` when drawn): only their fixed parts.
    fn drawn_rects(layout: &PageLayout) -> Vec<Rect> {
        let mut drawn = Vec::new();
        match layout {
            PageLayout::Calendar(l) => {
                drawn.extend([l.header, l.list, l.prev, l.next, l.add]);
                drawn.extend(l.view_buttons.iter().map(|(_, r)| *r));
                drawn.extend(l.offline);
                drawn.extend(l.connect);
                drawn.extend(l.weekday_labels.iter().copied());
                drawn.extend(l.week_columns.iter().map(|(_, r)| *r));
                drawn.extend(l.week_more.iter().map(|(_, _, r)| *r));
                drawn.extend(l.caption);
                drawn.extend(l.more.map(|(_, r)| r));
            }
            PageLayout::Pomodoro(l) => {
                drawn.push(l.header);
                let (cx, cy) = l.ring_center;
                let r = l.ring_radius + RING_STROKE / 2.0;
                drawn.push(Rect {
                    x: cx - r,
                    y: cy - r,
                    w: 2.0 * r,
                    h: 2.0 * r,
                });
                drawn.extend([l.time, l.phase, l.session, l.durations, l.next_up]);
                drawn.extend([l.start_pause, l.reset, l.skip]);
            }
            PageLayout::System(l) => {
                let (cx, cy) = l.ring_center;
                let r = l.ring_radius + SYS_RING_STROKE / 2.0;
                drawn.push(Rect {
                    x: cx - r,
                    y: cy - r,
                    w: 2.0 * r,
                    h: 2.0 * r,
                });
                drawn.extend([l.header, l.uptime, l.clean, l.spark, l.busiest]);
                drawn.extend([l.percent, l.caption, l.ram.row]);
                drawn.extend(l.drives.iter().map(|(_, m)| m.row));
                drawn.extend(l.procs.iter().map(|p| p.row));
            }
            PageLayout::Notes(l) | PageLayout::Reminders(l) => {
                drawn.extend([l.header, l.new_button]);
                drawn.extend(l.groups.iter().map(|(_, r)| *r));
            }
            PageLayout::Media(l) => {
                drawn.extend([l.art, l.label, l.title, l.artist, l.lyrics]);
                drawn.extend(l.buttons);
                let p = media_progress_layout(l);
                drawn.extend([p.bar, p.elapsed, p.total]);
            }
            PageLayout::Cli(l) => {
                drawn.extend([l.header, l.output, l.prompt, l.field, l.expand]);
                drawn.extend(l.stop);
                drawn.extend(l.chips.iter().filter(|c| c.w > 0.0));
                if let Some(m) = &l.menu {
                    drawn.push(m.boxed);
                    drawn.extend(m.rows.iter().map(|(_, r)| *r));
                }
            }
            PageLayout::Clipboard(l) => {
                drawn.extend([l.header, l.search, l.list]);
                drawn.extend(l.cards.iter().map(|c| c.2));
                drawn.extend(l.buttons.iter().map(|b| b.2));
            }
            PageLayout::Search(l) => {
                drawn.extend([l.back, l.field]);
                drawn.extend(l.rows.iter().copied());
                drawn.extend(l.empty);
            }
            PageLayout::Mixer(l) => {
                drawn.push(l.header);
                for row in &l.rows {
                    drawn.extend([row.icon, row.name, row.track, row.value]);
                }
            }
            PageLayout::Shelf(l) => {
                drawn.push(l.header);
                drawn.extend(l.clear);
                drawn.extend(l.empty);
                for c in &l.cards {
                    let (glyph, label) = tile_parts(c.card);
                    drawn.extend([c.card, c.remove, glyph, label]);
                }
            }
            PageLayout::Shortcuts(l) => {
                drawn.push(l.header);
                for (_, tile) in &l.tiles {
                    let (glyph, label) = tile_parts(*tile);
                    drawn.extend([glyph, label]);
                }
            }
            PageLayout::Devices(l) => {
                drawn.extend([l.header, l.list]);
                drawn.extend(l.chip);
                drawn.extend(l.eject_all);
            }
            // The same for Susun: the header and the chips are fixed, the
            // content is clipped to `list`.
            PageLayout::Arrange(l) => {
                drawn.extend([l.header, l.list]);
                drawn.extend(l.chips.iter().copied());
                drawn.extend(l.skip);
                drawn.extend(l.hint);
            }
            PageLayout::Notifications(l) => {
                drawn.extend([l.header, l.list]);
                drawn.extend(l.count);
                drawn.extend(l.pause);
                drawn.extend(l.pause_label);
                drawn.extend(l.pause_switch);
                drawn.extend(l.clear);
                drawn.extend(l.note);
                drawn.extend(l.settings_button);
                drawn.extend(l.scrollbar);
                drawn.extend(l.rows.iter().map(|r| r.row));
            }
            // On their own, taller shape: `cleaner_fits_its_panel`; the
            // header row's parts are the ones the Settings button must clear.
            PageLayout::Cleaner(l) => {
                drawn.extend([l.back, l.header, l.seg, l.credit, l.picked, l.go]);
                drawn.extend(l.seg_items);
                drawn.extend(l.rows.iter().map(|r| r.row));
            }
            PageLayout::Mole(l) => {
                drawn.extend([l.back, l.header, l.seg, l.chip, l.intro, l.label]);
                drawn.extend(l.seg_items);
                drawn.extend(l.rows.iter().map(|r| r.row));
                drawn.extend(l.opts.iter().map(|o| o.hit));
                drawn.extend([l.log, l.result, l.protect, l.go].into_iter().flatten());
                if let Some(c) = l.card {
                    drawn.extend([c.boxed, c.robot, c.title, c.sub, c.button]);
                }
            }
        }
        drawn
    }

    #[test]
    fn drawn_rects_inside_panel() {
        // Rects that are drawn but not clickable must stay inside too.
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for strip in [false, true] {
                let mut drawn = Vec::new();
                for (_, layout) in all_layouts(s, strip) {
                    // Cleaner and Mole: their own shape, below.
                    if !matches!(layout, PageLayout::Mole(_) | PageLayout::Cleaner(_)) {
                        drawn.extend(drawn_rects(&layout));
                    }
                }
                for r in drawn {
                    assert!(rect_inside(style, s, r), "{style:?} strip={strip} {r:?}");
                }
            }
        }
    }

    #[test]
    fn pomodoro_ring_left_controls_right() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for strip in [false, true] {
                let l = pomodoro_layout(s, strip);
                assert_eq!(l.ring_radius, 92.0);
                let (cx, cy) = l.ring_center;
                // The ring and its stroke stay between the header and the dots.
                let outer = l.ring_radius + RING_STROKE / 2.0;
                assert!(
                    cy + outer <= content_bottom(s) + 0.01,
                    "{style:?} strip={strip}"
                );
                assert!(cy - outer >= l.header.bottom() - 0.01);
                // The countdown box is centred on the ring's axis, a little
                // above its centre, and wide enough for the widest countdown
                // in DM Sans 600 48, tracked ("100:00", 148.6 px).
                assert!((l.time.x + l.time.w / 2.0 - cx).abs() < 0.01);
                let time_cy = l.time.y + l.time.h / 2.0;
                assert!(time_cy < cy && cy - time_cy <= 12.0);
                assert!(l.time.w >= 149.0, "time box {}", l.time.w);
                assert!(l.time.h >= 62.5, "one line tall: {}", l.time.h);
                // The digits' ink band across the box stays inside the
                // stroke's inner edge.
                let inner = l.ring_radius - RING_STROKE / 2.0;
                let far = (time_cy - TIME_INK_HALF - cy).abs();
                let corner = (l.time.w / 2.0).hypot(far);
                assert!(corner <= inner + 0.01, "corner {corner} vs inner {inner}");
                // The phase sits under the digits, inside the ring too.
                assert!(l.phase.y >= time_cy + TIME_INK_HALF);
                assert!((l.phase.x + l.phase.w / 2.0 - cx).abs() < 0.01);
                let low = l.phase.bottom() - cy;
                assert!((l.phase.w / 2.0).hypot(low) <= inner + 0.01);
                assert!(l.phase.w >= 120.0, "phase box {}", l.phase.w);
                assert!(cx < s.x + s.w / 2.0, "ring in the left half");
                // Ring and column together sit centred in the panel.
                let group_l = cx - outer;
                let group_r = l.session.right();
                let (ml, mr) = (group_l - left(s), right(s) - group_r);
                assert!((ml - mr).abs() < 0.01, "{style:?}: margins {ml} / {mr}");
                // The column starts right of the ring, top to bottom.
                for r in [l.session, l.reset, l.next_up] {
                    assert!(r.x >= cx + outer + 8.0, "{r:?} right of the ring");
                }
                assert!(l.session.bottom() <= l.start_pause.y);
                assert!(l.start_pause.bottom() <= l.next_up.y);
                // Reset, the big primary button, skip: in order, round,
                // centred on one line, without touching.
                assert!(l.reset.right() < l.start_pause.x && l.start_pause.right() < l.skip.x);
                assert!(l.start_pause.w > l.reset.w);
                for b in [l.reset, l.start_pause, l.skip] {
                    assert_eq!(b.w, b.h, "round");
                    let mid = b.y + b.h / 2.0;
                    assert!((mid - (l.start_pause.y + l.start_pause.h / 2.0)).abs() < 0.01);
                }
                assert!(l.skip.right() <= l.session.right());
                // The column is centred on the ring, below the header.
                let mid = (l.session.y + l.next_up.bottom()) / 2.0;
                assert!((mid - cy).abs() < 0.01);
                // Durations right of the title.
                assert!(l.header.right() < l.durations.x);
                assert_eq!(l.durations.right(), header_right(s));
            }
        }
    }

    #[test]
    fn pomodoro_compact_fits_its_shape() {
        for style in [Style::Pill, Style::Notch] {
            let s = shape_for(style, ShapeKind::PomodoroCompact);
            let ((cx, cy), r, text) = pomodoro_compact_layout(s);
            assert_eq!(r, 8.0);
            assert_eq!(cy, s.y + s.h / 2.0);
            assert!(cx - r - 1.5 >= s.x + 4.0);
            assert!(text.x >= cx + r);
            assert!(text.right() <= s.x + s.w);
            assert!(rect_inside(style, s, text), "{style:?} {text:?}");
        }
    }

    /// Sizes spanning the slider ranges, corners included.
    fn spread() -> Vec<crate::ui::geometry::NotchSize> {
        [(160, 28), (190, 34), (300, 44), (160, 44), (300, 28)]
            .into_iter()
            .map(|(w, h)| crate::ui::geometry::NotchSize::from_px(w, h))
            .collect()
    }

    #[test]
    fn pomodoro_compact_sits_the_inset_from_both_ends() {
        use crate::ui::geometry::{COMPACT_INSET, shape_with_robot_sized};
        for style in [Style::Pill, Style::Notch] {
            for size in spread() {
                let tag = format!("{style:?} {size:?}");
                let s = shape_with_robot_sized(style, size, ShapeKind::PomodoroCompact, false);
                let ((cx, _), r, text) = pomodoro_compact_layout(s);
                assert!((cx - r - (s.x + COMPACT_INSET)).abs() < 1e-3, "{tag}");
                assert!((cx - (s.x + 24.0)).abs() < 1e-3, "{tag}");
                assert!(cx - r >= s.x + 16.0 && text.right() <= s.x + s.w - 16.0);

                let s = shape_with_robot_sized(style, size, ShapeKind::PomodoroCompact, true);
                let (robot, (cx, _), r, text) = pomodoro_compact_layout_robot(s);
                assert!((robot.x - (s.x + COMPACT_INSET)).abs() < 1e-3, "{tag}");
                assert!((cx - (s.x + 58.0)).abs() < 1e-3, "{tag}");
                assert!(robot.right() + 8.0 <= cx - r + 1e-3);
                assert!(robot.x >= s.x + 16.0 && text.right() <= s.x + s.w - 16.0);
            }
        }
    }

    #[test]
    fn pomodoro_compact_keeps_the_text_room_it_had() {
        use crate::ui::geometry::shape_with_robot_sized;
        // Today's layout from the old numbers: the shape w + 20, the ring's
        // centre 22 in, text 18 from the right; with the robot (6 + 26, then
        // 8 + 8 to the centre) the shape w + 46.
        for style in [Style::Pill, Style::Notch] {
            for size in spread() {
                let old_plain = (size.w + 20.0) - (22.0 + 8.0 + 10.0) - 18.0;
                let old_robot = (size.w + 46.0) - (6.0 + 26.0 + 8.0 + 8.0 + 8.0 + 10.0) - 18.0;
                let s = shape_with_robot_sized(style, size, ShapeKind::PomodoroCompact, false);
                assert!((s.w - (size.w + 22.0)).abs() < 1e-3);
                let (_, _, text) = pomodoro_compact_layout(s);
                assert!((text.w - old_plain).abs() < 1e-3, "{size:?}");
                assert!((text.w - (size.w - 38.0)).abs() < 1e-3);
                let s = shape_with_robot_sized(style, size, ShapeKind::PomodoroCompact, true);
                assert!((s.w - (size.w + 56.0)).abs() < 1e-3);
                let (_, _, _, text) = pomodoro_compact_layout_robot(s);
                assert!((text.w - old_robot).abs() < 1e-3, "{size:?}");
                assert!((text.w - (size.w - 38.0)).abs() < 1e-3);
            }
        }
    }

    /// Every layout the Settings button must clear, each with the shape it
    /// is drawn in: the pages at the default panel (`all_layouts`), a
    /// calendar offline with a long title (the tag takes room), the CLI in the
    /// tall panel, and the cleaner in both modes on its own shape.
    fn settings_cases(style: Style, strip: bool) -> Vec<(ShapeRect, String, PageLayout)> {
        let s = panel(style);
        let mut out: Vec<(ShapeRect, String, PageLayout)> = all_layouts(s, strip)
            .into_iter()
            .map(|(name, layout)| (s, name, layout))
            .collect();
        let events = many_events(3, oct25());
        for view in [CalView::Day, CalView::Week, CalView::Month] {
            for title_w in [TITLE_W, 600.0] {
                let l = super::calendar_layout(
                    s,
                    view,
                    oct25(),
                    oct25(),
                    &events,
                    &CalStatus::Offline,
                    0,
                    strip,
                    title_w,
                    None,
                );
                assert!(l.offline.is_some());
                out.push((
                    s,
                    format!("calendar offline {view:?} title {title_w}"),
                    PageLayout::Calendar(l),
                ));
            }
        }
        let tall = shape_for(style, ShapeKind::PanelTall);
        for (n, head) in cli_heads().into_iter().enumerate() {
            let mut l = cli_layout(tall, strip, &head);
            out.push((tall, format!("cli tall {n}"), PageLayout::Cli(l.clone())));
            l.menu = Some(cli_folder_menu(&l, &menu_cases()[0]));
            out.push((tall, format!("cli tall menu {n}"), PageLayout::Cli(l)));
        }
        let clean = shape_for(style, ShapeKind::PanelClean);
        let wide = CleanHead {
            seg: [140.0, 170.0],
        };
        for (n, head) in [CLEAN_HEAD, wide].iter().enumerate() {
            let l = cleaner_layout(clean, strip, head, &CLEAN_FOOT, true, true);
            out.push((clean, format!("cleaner {n}"), PageLayout::Cleaner(l)));
            for body in [
                MoleBody::Rows,
                MoleBody::Log { result: true },
                MoleBody::Log { result: false },
                MoleBody::Card,
            ] {
                let l = mole_layout(clean, strip, head, &MOLE_FOOT, look(body));
                out.push((clean, format!("mole {n} {body:?}"), PageLayout::Mole(l)));
            }
        }
        out
    }

    #[test]
    fn settings_button_is_clear_of_every_page() {
        for style in [Style::Pill, Style::Notch] {
            for strip in [false, true] {
                for (s, name, layout) in settings_cases(style, strip) {
                    let tag = format!("{style:?} strip={strip} {name}");
                    let btn = settings_button(s, strip);
                    // In the header row, at the top right, inside the panel
                    // and above the page dots.
                    assert!(rect_inside(style, s, btn), "{tag}: {btn:?} outside");
                    assert_eq!(btn.y, content_top(s, strip), "{tag}");
                    assert_eq!(btn.h, 22.0, "{tag}");
                    assert_eq!(btn.right(), right(s), "{tag}");
                    let (dots, _) = page_dots(s, &Page::ALL, Page::Calendar);
                    assert!(btn.bottom() <= dots[0].y, "{tag}: over the dots");

                    // One region, right after the strip (first without it).
                    let r = regions(s, &layout, strip);
                    let at: Vec<usize> = (0..r.len())
                        .filter(|&i| r[i].hit == Hit::OpenSettings)
                        .collect();
                    assert_eq!(at, [usize::from(strip)], "{tag}: one, in its place");
                    if strip {
                        assert_eq!(r[0].hit, Hit::Strip, "{tag}");
                    }
                    assert_eq!(r[at[0]].rect, btn, "{tag}");

                    // Clear of every other region; the underlays that lie
                    // beneath it come after it (hit-testing is first match).
                    for (i, reg) in r.iter().enumerate() {
                        if reg.hit == Hit::OpenSettings {
                            continue;
                        }
                        if matches!(reg.hit, Hit::NotifFocus | Hit::CliMenuDismiss) {
                            assert!(i > at[0], "{tag}: {:?} must come after", reg.hit);
                            continue;
                        }
                        assert!(disjoint(btn, reg.rect), "{tag}: {btn:?} overlaps {reg:?}");
                    }

                    // And clear of everything the page draws.
                    for d in drawn_rects(&layout) {
                        assert!(disjoint(btn, d), "{tag}: {btn:?} overlaps drawn {d:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_settings_button_is_the_same_rect_on_every_page() {
        for style in [Style::Pill, Style::Notch] {
            for strip in [false, true] {
                let s = panel(style);
                let btn = settings_button(s, strip);
                for (name, layout) in all_layouts(s, strip) {
                    let r = regions(s, &layout, strip);
                    let reg = r.iter().find(|x| x.hit == Hit::OpenSettings).unwrap();
                    assert_eq!(reg.rect, btn, "{name}");
                }
            }
        }
    }

    #[test]
    fn header_controls_end_left_of_the_settings_button() {
        let s = panel(Style::Pill);
        let hr = header_right(s);
        assert_eq!(hr, settings_button(s, false).x - SETTINGS_GAP);
        let c = calendar_layout(
            s,
            CalView::Month,
            oct25(),
            oct25(),
            &[],
            &CalStatus::Ok,
            0,
            false,
        );
        // The view selector ends at the header edge, the body keeps the
        // full width.
        assert_eq!(c.view_buttons[2].1.right() + SEG_PAD, hr);
        assert_eq!(c.list.right(), right(s));
        // The media text column keeps its full width at the default panel.
        let m = media_panel_layout(s, false);
        assert_eq!(m.title.right(), right(s));
        assert_eq!(m.lyrics.right(), right(s));
    }

    #[test]
    fn the_media_text_column_gives_way_to_the_settings_button_in_a_short_panel() {
        // A panel short enough that the label rises to the button's rows:
        // what would reach the button ends left of it.
        let full = panel(Style::Pill);
        let s = ShapeRect { h: 232.0, ..full };
        let m = media_panel_layout(s, false);
        let btn = settings_button(s, false);
        let keep = Rect {
            x: btn.x - SETTINGS_GAP,
            y: btn.y - SETTINGS_GAP,
            w: btn.w + 2.0 * SETTINGS_GAP,
            h: btn.h + 2.0 * SETTINGS_GAP,
        };
        let rows_reached = [m.label, m.title, m.artist]
            .into_iter()
            .filter(|r| r.y < keep.bottom() && r.bottom() > keep.y)
            .count();
        assert!(rows_reached > 0, "the case must reach the button's rows");
        for r in [m.label, m.title, m.artist] {
            assert!(disjoint(r, keep), "{r:?} reaches {keep:?}");
        }
        // Nothing the page draws touches the button either.
        for d in drawn_rects(&PageLayout::Media(m)) {
            assert!(disjoint(btn, d), "{d:?}");
        }
    }
}

#[cfg(test)]
mod media_tests {
    use super::*;
    use crate::config::Style;
    use crate::ui::geometry::{ShapeKind, shape_for};

    #[test]
    fn lyric_button_ends_the_transport_row() {
        for style in [Style::Pill, Style::Notch] {
            let s = shape_for(style, ShapeKind::Panel);
            for strip in [false, true] {
                let l = media_panel_layout(s, strip);
                assert_eq!(l.lyrics.right(), right(s));
                // On the buttons' line, right of them, touching none.
                let [prev, play, next] = l.buttons;
                let mid = |r: Rect| r.y + r.h / 2.0;
                assert!((mid(l.lyrics) - mid(play)).abs() < 0.01);
                assert!(prev.right() < play.x && play.right() < next.x);
                assert!(next.right() < l.lyrics.x);
                assert_eq!(play.w, play.h, "the round primary button");
                // The text column: label, title, artist, then the row.
                assert!(l.label.bottom() <= l.title.y && l.title.bottom() <= l.artist.y);
                assert!(l.artist.bottom() <= play.y);
                assert!(l.label.x >= l.art.right() + 16.0);
                // A big square cover, centred between the header and the dots.
                assert_eq!(l.art.w, l.art.h);
                assert!(l.art.w >= 180.0, "{:?}", l.art);
                assert_eq!(l.art.x, left(s));
                let gap_top = l.art.y - content_top(s, strip);
                let gap_bottom = content_bottom(s) - l.art.bottom();
                assert!((gap_top - gap_bottom).abs() < 0.01);
            }
        }
    }

    #[test]
    fn progress_bar_and_times_sit_between_artist_and_buttons() {
        for style in [Style::Pill, Style::Notch] {
            let s = shape_for(style, ShapeKind::Panel);
            for strip in [false, true] {
                let l = media_panel_layout(s, strip);
                let p = media_progress_layout(&l);
                let [prev, play, _] = l.buttons;
                let top_of_row = prev.y.min(play.y).min(l.lyrics.y);
                // As in the design: 4 px bar 18 px under the artist, the
                // times 6 px under it, the controls 14 px under those.
                assert_eq!(p.bar.h, 4.0);
                assert!((p.bar.y - l.artist.bottom() - 18.0).abs() < 0.01);
                assert!((p.elapsed.y - p.bar.bottom() - 6.0).abs() < 0.01);
                assert!((play.y - p.elapsed.bottom() - 14.0).abs() < 0.01);
                assert!(p.elapsed.bottom() <= top_of_row);
                // The text column's width; elapsed left of the total, one row.
                assert_eq!((p.bar.x, p.bar.right()), (l.artist.x, l.artist.right()));
                assert_eq!(p.elapsed.x, p.bar.x);
                assert!((p.total.right() - p.bar.right()).abs() < 0.01);
                assert!(p.elapsed.right() <= p.total.x);
                assert_eq!(p.elapsed.y, p.total.y);
                // The whole column still fits beside the cover, inside the panel.
                assert!(l.label.y >= l.art.y - 0.01, "{style:?} strip={strip}");
                assert!(
                    play.bottom() <= l.art.bottom() + 0.01,
                    "{style:?} strip={strip}"
                );
                for r in [p.bar, p.elapsed, p.total] {
                    assert!(r.x >= s.x && r.right() <= s.x + s.w, "{r:?}");
                    assert!(r.y >= s.y && r.bottom() <= content_bottom(s), "{r:?}");
                }
            }
        }
    }

    #[test]
    fn clipboard_cards_two_columns_sixty_four_tall() {
        for style in [Style::Pill, Style::Notch] {
            let s = shape_for(style, ShapeKind::Panel);
            let ids: Vec<u64> = (1..=48).collect();
            let l = clipboard_layout(s, false, &ids, Some(1), 0, |id| id == 3);
            assert_eq!(l.rows, 3);
            assert_eq!(l.cards.len(), 6);
            let (_, _, a) = l.cards[0];
            let (_, _, b) = l.cards[1];
            let (_, _, c) = l.cards[2];
            assert_eq!(a.h, CLIP_CARD_H);
            assert!((b.x - a.right() - CLIP_GAP).abs() < 0.01);
            assert!((c.y - a.bottom() - CLIP_GAP).abs() < 0.01);
            assert_eq!(a.x, l.list.x);
            assert!((b.right() - l.list.right()).abs() < 0.01);
            // All inside the panel, above the dots.
            let (dots, _) = page_dots(s, &Page::ALL, Page::Clipboard);
            for (_, _, r) in &l.cards {
                assert!(r.bottom() <= dots[0].y - 6.0 + 0.01, "{r:?}");
            }
            // The search field sits right of the title, at most 250 wide.
            assert!(l.search.w <= 250.0 && l.search.x > l.header.right());
            assert!(l.search.bottom() < l.list.y);
            // The selected card's three buttons sit inside its top-right.
            assert_eq!(l.buttons.len(), 3);
            for (_, id, r) in &l.buttons {
                assert_eq!(*id, 1);
                assert!(r.x >= a.x && r.right() <= a.right() && r.y >= a.y);
            }
            assert!(l.empty.is_none());
        }
    }

    #[test]
    fn a_wheel_notch_scrolls_a_row_or_turns_the_page_when_nothing_scrolls() {
        // Nothing to scroll (6 cards or fewer fit): the notch turns the page.
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let rows = clip_rows(s, false);
        for n in [0, 1, 2 * rows] {
            assert_eq!(clip_max_scroll(n, rows), 0, "{n} cards");
            assert_eq!(clip_wheel_step(-120, 0, 0), None);
            assert_eq!(clip_wheel_step(120, 0, 0), None);
        }
        // 50 cards: 25 rows, `rows` shown; a notch down is a row.
        let max = clip_max_scroll(50, rows);
        assert_eq!(max, 25 - rows);
        assert_eq!(clip_wheel_step(-120, 0, max), Some(1));
        assert_eq!(clip_wheel_step(120, 5, max), Some(4));
        // A big or a fine (touchpad) delta moves at least a row, at most 20.
        assert_eq!(clip_wheel_step(-30, 0, max), Some(1));
        assert_eq!(clip_wheel_step(-120 * 50, 0, max), Some(20.min(max)));
        // The ends spend the notch (the row stays); the page does not turn.
        assert_eq!(clip_wheel_step(120, 0, max), Some(0));
        assert_eq!(clip_wheel_step(-120, max, max), Some(max));
    }

    #[test]
    fn the_scrollbar_shows_only_when_the_cards_scroll_and_follows_the_row() {
        for style in [Style::Pill, Style::Notch] {
            let s = shape_for(style, ShapeKind::Panel);
            let rows = clip_rows(s, false);
            // Everything fits: no bar.
            let few: Vec<u64> = (1..=2 * rows as u64).collect();
            assert!(
                clipboard_layout(s, false, &few, None, 0, |_| false)
                    .scrollbar
                    .is_none()
            );
            // 50 cards: a bar in the right margin, inside the list's height.
            let ids: Vec<u64> = (1..=50).collect();
            let top = clipboard_layout(s, false, &ids, None, 0, |_| false);
            let bar = top.scrollbar.expect("a scrollbar");
            assert!(
                bar.x > top.list.right() && bar.right() <= s.x + s.w - 4.0,
                "{bar:?}"
            );
            assert_eq!(bar.y, top.list.y, "at the top");
            assert!(bar.h >= 24.0 && bar.h < top.list.h);
            let max = clip_max_scroll(50, rows);
            let end = clipboard_layout(s, false, &ids, None, max, |_| false)
                .scrollbar
                .unwrap();
            assert!(
                (end.bottom() - top.list.bottom()).abs() < 0.01,
                "at the end"
            );
            let mid = clipboard_layout(s, false, &ids, None, max / 2, |_| false)
                .scrollbar
                .unwrap();
            assert!(mid.y > bar.y && mid.y < end.y);
            // Same height whatever the row.
            assert!((bar.h - end.h).abs() < 0.01);
        }
    }

    #[test]
    fn clipboard_scrolls_by_rows_and_keeps_the_selection_in_view() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let ids: Vec<u64> = (1..=11).collect();
        // 11 cards: 6 rows, 3 shown, at most 3 rows down.
        assert_eq!(clip_max_scroll(11, 3), 3);
        let l = clipboard_layout(s, false, &ids, Some(3), 9, |_| false);
        assert_eq!(l.scroll, 3, "clamped");
        let shown: Vec<u64> = l.cards.iter().map(|c| c.0).collect();
        assert_eq!(shown, [7, 8, 9, 10, 11]);
        // Card 3 is scrolled away: no buttons.
        assert!(l.buttons.is_empty());
        assert_eq!(clip_scroll_to(0, 2, 3), 0);
        assert_eq!(clip_scroll_to(7, 0, 3), 1);
        assert_eq!(clip_scroll_to(10, 0, 3), 3);
        assert_eq!(clip_scroll_to(3, 1, 3), 1);
        // An image card has no plain-text button.
        let l = clipboard_layout(s, false, &ids, Some(2), 0, |id| id == 2);
        let kinds: Vec<ClipButton> = l.buttons.iter().map(|b| b.0).collect();
        assert_eq!(kinds, [ClipButton::Delete, ClipButton::Pin]);
    }

    #[test]
    fn clipboard_regions_and_empty_state() {
        let s = shape_for(Style::Pill, ShapeKind::Panel);
        let l = clipboard_layout(s, true, &[5, 6], Some(5), 0, |_| false);
        let r = regions(s, &PageLayout::Clipboard(l.clone()), true);
        let first = |hit: Hit| r.iter().position(|x| x.hit == hit);
        for hit in [
            Hit::Strip,
            Hit::ClipSearch,
            Hit::ClipCard(5),
            Hit::ClipCard(6),
            Hit::ClipPin(5),
            Hit::ClipPlain(5),
            Hit::ClipDelete(5),
        ] {
            assert!(first(hit.clone()).is_some(), "{hit:?}");
        }
        // Buttons win over their card.
        assert!(first(Hit::ClipPin(5)) < first(Hit::ClipCard(5)));
        let empty = clipboard_layout(s, false, &[], None, 0, |_| false);
        assert!(empty.empty.is_some() && empty.cards.is_empty());
    }

    fn panel(style: Style) -> ShapeRect {
        shape_for(style, ShapeKind::Panel)
    }

    const NOTIF_HEAD: NotifHead = NotifHead {
        title_w: 90.0,
        count_w: 52.0,
        pause_label_w: 30.0,
        clear_w: 92.0,
    };

    fn inside_rect(outer: Rect, r: Rect) -> bool {
        r.x >= outer.x - 0.01
            && r.y >= outer.y - 0.01
            && r.right() <= outer.right() + 0.01
            && r.bottom() <= outer.bottom() + 0.01
    }

    fn notif(s: ShapeRect, strip: bool, n: u64, sel: Option<u64>, scroll: usize) -> NotifLayout {
        let ids: Vec<u64> = (1..=n).rev().collect();
        notif_layout(s, strip, &NOTIF_HEAD, &ids, sel, scroll, false, false)
    }

    #[test]
    fn notification_rows_are_44_tall_on_a_48_pitch_and_whole_rows_only() {
        for style in [Style::Pill, Style::Notch] {
            let s = panel(style);
            for (strip, note) in [(false, false), (true, false), (false, true), (true, true)] {
                let fit = notif_rows(s, strip, note);
                assert!(fit >= 3, "{style:?} strip={strip} note={note}: {fit}");
                let ids: Vec<u64> = (1..=20).collect();
                let l = notif_layout(s, strip, &NOTIF_HEAD, &ids, None, 0, note, false);
                assert_eq!(l.rows.len(), fit);
                for (i, row) in l.rows.iter().enumerate() {
                    assert_eq!(row.row.h, NOTIF_ROW_H);
                    assert_eq!(row.row.y, l.list.y + i as f32 * 48.0);
                    assert!(inside_rect(l.list, row.row), "whole rows stay in the list");
                    assert!(inside_rect(row.row, row.logo) && row.logo.w == 24.0);
                    for part in [row.name, row.time, row.dot, row.text] {
                        assert!(inside_rect(row.row, part), "{part:?} in {:?}", row.row);
                    }
                    // Line one above line two; the name stops before the dot.
                    assert!(row.name.bottom() <= row.text.y + 1.0);
                    assert!(row.name.right() <= row.dot.x);
                    assert!(row.dot.right() <= row.time.x);
                }
                // The list ends above the page dots.
                let (dots, _) = page_dots(s, &Page::ALL, Page::Notifications);
                assert!(l.list.bottom() <= dots[0].y);
            }
        }
    }

    #[test]
    fn the_selected_row_gets_three_buttons_and_its_text_stops_before_them() {
        let s = panel(Style::Pill);
        let l = notif(s, false, 6, Some(5), 0);
        let with: Vec<&NotifRowLayout> = l.rows.iter().filter(|r| r.buttons.is_some()).collect();
        assert_eq!(with.len(), 1);
        let row = with[0];
        assert_eq!(row.id, 5);
        let b = row.buttons.unwrap();
        // Right to left on line two: X, Kecualikan, Buka; clear of one another.
        assert!(b.remove.x > b.mute.right() && b.mute.x > b.open.right());
        for r in [b.open, b.mute, b.remove] {
            assert!(inside_rect(row.row, r));
            assert!(r.y >= row.name.bottom() - 1.0, "on the second line");
        }
        assert!(row.text.right() <= b.open.x);
        // Another row's text runs to the edge instead.
        let other = l.rows.iter().find(|r| r.id == 6).unwrap();
        assert!(other.text.right() > row.text.right());
        assert!(other.buttons.is_none());
        // A selection that is scrolled out of the list shows no buttons.
        let l = notif(s, false, 10, Some(10), 4);
        assert!(l.rows.iter().all(|r| r.buttons.is_none()));
    }

    #[test]
    fn scroll_is_in_whole_rows_and_stays_in_range() {
        let s = panel(Style::Pill);
        let fit = notif_rows(s, false, false);
        let n = 40u64;
        let max = notif_max_scroll(n as usize, fit);
        assert_eq!(max, n as usize - fit);
        let l = notif(s, false, n, None, 999);
        assert_eq!(l.scroll, max, "clamped");
        assert_eq!(l.rows.last().unwrap().index, n as usize - 1);
        let l = notif(s, false, n, None, 3);
        assert_eq!(l.rows[0].index, 3);
        // Nothing to scroll with few rows: no thumb.
        assert_eq!(notif_max_scroll(fit, fit), 0);
        assert!(notif(s, false, fit as u64, None, 5).scrollbar.is_none());
    }

    #[test]
    fn the_wheel_scrolls_a_row_a_notch_and_an_end_spends_it() {
        // Nothing to scroll: the notch turns the page.
        assert_eq!(notif_wheel_step(-120, 0, 0), None);
        assert_eq!(notif_wheel_step(120, 0, 0), None);
        // Toward the user scrolls down a row; away scrolls up.
        assert_eq!(notif_wheel_step(-120, 2, 10), Some(3));
        assert_eq!(notif_wheel_step(120, 2, 10), Some(1));
        assert_eq!(notif_wheel_step(-360, 2, 10), Some(5));
        // The ends keep the row: the notch is spent.
        assert_eq!(notif_wheel_step(120, 0, 10), Some(0));
        assert_eq!(notif_wheel_step(-120, 10, 10), Some(10));
        assert_eq!(notif_wheel_step(-1200, 8, 10), Some(10));
        // A touchpad's small delta still moves one row.
        assert_eq!(notif_wheel_step(-30, 0, 10), Some(1));
        assert_eq!(notif_wheel_step(30, 5, 10), Some(4));
    }

    #[test]
    fn keyboard_moves_keep_the_selection_in_view() {
        assert_eq!(notif_scroll_to(2, 0, 4), 0, "already shows");
        assert_eq!(notif_scroll_to(4, 0, 4), 1, "one row past the bottom");
        assert_eq!(notif_scroll_to(1, 3, 4), 1, "above the top");
        assert_eq!(notif_scroll_to(9, 0, 4), 6);
        // A wheel scroll keeps the selection on a row that shows.
        assert_eq!(notif_visible_row(0, 3, 4, 20), 3);
        assert_eq!(notif_visible_row(12, 3, 4, 20), 6);
        assert_eq!(notif_visible_row(5, 3, 4, 20), 5);
        assert_eq!(notif_visible_row(5, 0, 4, 0), 0);
        assert_eq!(notif_visible_row(9, 8, 4, 10), 9, "never past the last row");
    }

    #[test]
    fn the_scrollbar_thumb_sits_in_the_margin_and_follows_the_scroll() {
        let s = panel(Style::Pill);
        let fit = notif_rows(s, false, false);
        let top = notif(s, false, 40, None, 0);
        let end = notif(s, false, 40, None, 40 - fit);
        let (t, e) = (top.scrollbar.unwrap(), end.scrollbar.unwrap());
        assert_eq!(t.w, 3.0);
        assert!(t.x >= top.list.right(), "in the right margin");
        assert!(right(s) < s.x + s.w && t.right() <= s.x + s.w);
        assert_eq!(t.y, top.list.y);
        assert!((e.bottom() - end.list.bottom()).abs() < 0.01);
        assert!(t.h >= 24.0 && t.h < top.list.h);
        // More rows, a shorter thumb.
        let more = notif(s, false, 120, None, 0).scrollbar.unwrap();
        assert!(more.h <= t.h);
    }

    #[test]
    fn the_header_holds_the_title_the_count_the_switch_and_clear() {
        let s = panel(Style::Pill);
        let l = notif(s, false, 3, None, 0);
        let (count, pause, clear) = (l.count.unwrap(), l.pause.unwrap(), l.clear.unwrap());
        assert!(count.x >= l.header.right());
        assert!(count.right() <= pause.x, "count, then the switch");
        assert!(pause.right() <= clear.x, "then Hapus semua at the right");
        assert_eq!(clear.right(), header_right(s));
        let (label, switch) = (l.pause_label.unwrap(), l.pause_switch.unwrap());
        assert!(label.right() <= switch.x);
        assert!(inside_rect(pause, label) && inside_rect(pause, switch));
        assert_eq!((switch.w, switch.h), (26.0, 14.0));
        // No entries: nothing to clear, the switch moves to the edge.
        let none = notif(s, false, 0, None, 0);
        assert!(none.clear.is_none());
        assert_eq!(none.pause.unwrap().right(), header_right(s));
        assert!(none.empty.is_some() && none.rows.is_empty());
    }

    #[test]
    fn the_off_note_sits_under_the_header_and_pushes_the_list_down() {
        let s = panel(Style::Pill);
        let ids = [3, 2, 1];
        let plain = notif_layout(s, false, &NOTIF_HEAD, &ids, None, 0, false, false);
        let noted = notif_layout(s, false, &NOTIF_HEAD, &ids, None, 0, true, false);
        assert!(plain.note.is_none());
        let note = noted.note.unwrap();
        assert!(note.y >= noted.header.bottom() && note.bottom() <= noted.list.y);
        assert!(noted.list.y > plain.list.y);
        assert_eq!(noted.rows.len(), 3, "the entries stay while it is off");
    }

    #[test]
    fn without_access_there_is_a_message_and_one_button_and_no_rows() {
        let s = panel(Style::Pill);
        let ids = [3, 2, 1];
        let l = notif_layout(s, false, &NOTIF_HEAD, &ids, Some(2), 0, true, true);
        assert!(l.rows.is_empty() && l.empty.is_some());
        let b = l.settings_button.unwrap();
        assert!(inside_rect(l.list, b));
        assert!(l.pause.is_none() && l.clear.is_none() && l.count.is_none());
        assert!(l.note.is_none() && l.scrollbar.is_none());
        // The keyboard is not offered a page it cannot show.
        assert!(l.background.is_none());
        let r = regions(s, &PageLayout::Notifications(l), false);
        assert!(r.iter().any(|x| x.hit == Hit::NotifSettings));
        assert!(!r.iter().any(|x| x.hit == Hit::NotifFocus));
    }

    #[test]
    fn notification_regions_put_buttons_over_rows_over_the_background() {
        let s = panel(Style::Pill);
        let l = notif(s, true, 4, Some(3), 0);
        let r = regions(s, &PageLayout::Notifications(l.clone()), true);
        let at = |hit: Hit| r.iter().position(|x| x.hit == hit);
        for hit in [
            Hit::Strip,
            Hit::NotifClear,
            Hit::NotifPause,
            Hit::NotifOpen(3),
            Hit::NotifMute(3),
            Hit::NotifRemove(3),
            Hit::NotifRow(1),
            Hit::NotifRow(4),
            Hit::NotifFocus,
        ] {
            assert!(at(hit.clone()).is_some(), "{hit:?}");
        }
        assert!(at(Hit::NotifOpen(3)) < at(Hit::NotifRow(3)));
        assert!(at(Hit::NotifRemove(3)) < at(Hit::NotifRow(3)));
        assert!(at(Hit::NotifRow(4)) < at(Hit::NotifFocus));
        assert!(at(Hit::NotifClear) < at(Hit::NotifFocus));
        // Only the selected row has buttons.
        assert!(at(Hit::NotifOpen(4)).is_none());
        // The first region under a point on a row's background is the row;
        // under a point past the last row it is the page's keyboard.
        let first = |x: f32, y: f32| r.iter().find(|g| g.rect.contains(x, y)).map(|g| &g.hit);
        let row4 = l.rows.iter().find(|w| w.id == 4).unwrap();
        assert_eq!(
            first(row4.name.x + 4.0, row4.row.y + 3.0),
            Some(&Hit::NotifRow(4))
        );
        let below = l.rows.last().unwrap().row.bottom() + 2.0;
        assert_eq!(first(l.list.x + 20.0, below), Some(&Hit::NotifFocus));
        assert_eq!(
            first(l.header.x + 4.0, l.header.y + 4.0),
            Some(&Hit::NotifFocus),
            "the title"
        );
    }
}
