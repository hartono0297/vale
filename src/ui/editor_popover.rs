//! The editors' popovers: a month calendar under the date chip and a list
//! of times every 15 minutes under a time chip (`.cal`, `.g7`, `.times` in
//! `docs/design/editor-acara.html`). Each is a small frameless popup owned
//! by the editor, rounded by DWM and drawn with Direct2D like the editor.
//!
//! A popover takes the keyboard while it is open. It reports a pick with
//! `WM_APP_POPOVER_PICK` and its end with `WM_APP_POPOVER_CLOSED`; losing
//! activation (a click anywhere else) closes it without touching the editor.
//! At most one exists, on the editor's thread.

use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::Rc;

use chrono::{Days, Months, NaiveDate, NaiveTime, Timelike};
use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, DeleteObject, EndPaint, HDC, HFONT, HGDIOBJ, InvalidateRect, PAINTSTRUCT,
    SetBkColor, SetTextColor,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{EM_SETCUEBANNER, EM_SETSEL, WM_MOUSELEAVE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_A, VK_CONTROL, VK_DOWN, VK_END,
    VK_ESCAPE, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, DestroyWindow, EN_CHANGE, ES_AUTOHSCROLL,
    GetClientRect, GetParent, HMENU, IDC_ARROW, IsWindow, LoadCursorW, PostMessageW,
    RegisterClassExW, SW_SHOW, SetForegroundWindow, ShowWindow, WA_INACTIVE, WINDOW_EX_STYLE,
    WM_ACTIVATE, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT,
    WM_SETFONT, WM_SIZE, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP, WS_VISIBLE,
};
use windows::core::{Error, HSTRING, PCWSTR, Result, w};

use super::editor::{
    brushes, colorref, edit_font, key_down, monitor_at, send, set_frame, window_text,
};
use super::editor_layout::{Item, Text};
use super::editor_paint::{Align, Gfx, Painter, Target};
use super::geometry::Rect;
use super::tooltip::{self, KeyHint, Tip};
use super::icons::Icon;
use super::month_grid::{
    CellCanvas, CellLook, DayState, Filled, day_geometry, month_cells, paint_day,
};
use crate::i18n::{self, Key, tr};
use crate::theme;

/// Posted to the editor: `lparam` is a `Box<(Item, Pick)>` the editor frees.
pub(super) const WM_APP_POPOVER_PICK: u32 = WM_APP + 20;
/// Posted to the editor as the popover goes away; `wparam` holds `CLOSED_*`.
pub(super) const WM_APP_POPOVER_CLOSED: u32 = WM_APP + 21;
/// The editor should take the keyboard back on the chip.
pub(super) const CLOSED_REFOCUS: usize = 1;
/// ... and show the focus ring (closed from the keyboard).
pub(super) const CLOSED_KEYBOARD: usize = 2;
/// ... and save (Ctrl+Enter in the popover).
pub(super) const CLOSED_SAVE: usize = 4;

/// Minutes between two entries of the time list.
pub const STEP_MIN: u32 = 15;
const DAY_MIN: u32 = 24 * 60;

// ---- Times ----

fn minutes_of(t: NaiveTime) -> u32 {
    t.hour() * 60 + t.minute()
}

fn from_minutes(m: u32) -> NaiveTime {
    let m = m.min(DAY_MIN - 1);
    NaiveTime::from_hms_opt(m / 60, m % 60, 0).unwrap_or(NaiveTime::MIN)
}

/// A typed time: "9", "09", "930", "0930", "9:30", "09.30", "21.15".
/// Hours 0–23, minutes two digits 00–59; anything else is `None`.
pub fn parse_time(s: &str) -> Option<NaiveTime> {
    let s = s.trim();
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let (h, m) = match s.split_once([':', '.']) {
        Some((h, m)) => {
            if h.len() > 2 || m.len() != 2 {
                return None;
            }
            (h, m)
        }
        None => {
            if !digits(s) {
                return None;
            }
            match s.len() {
                1 | 2 => (s, "00"),
                3 => (&s[..1], &s[1..]),
                4 => (&s[..2], &s[2..]),
                _ => return None,
            }
        }
    };
    if !digits(h) || !digits(m) {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60)
        .then(|| NaiveTime::from_hms_opt(h, m, 0))
        .flatten()
}

/// The time list: every `STEP_MIN` from `start` (the end list) or from
/// midnight (the start list) up to the end of the day, with `value` added in
/// order when it is not on a step.
pub fn time_entries(value: NaiveTime, start: Option<NaiveTime>) -> Vec<NaiveTime> {
    let first = start.map_or(0, minutes_of);
    let mut list: Vec<NaiveTime> = (first..DAY_MIN)
        .step_by(STEP_MIN as usize)
        .map(from_minutes)
        .collect();
    let value = from_minutes(minutes_of(value));
    if let Err(at) = list.binary_search(&value) {
        list.insert(at, value);
    }
    list
}

/// An entry's length after the start, for the end list ("1 j 30 mnt");
/// empty at or before the start, and in the start list.
pub fn entry_duration(lang: i18n::Lang, t: NaiveTime, start: Option<NaiveTime>) -> String {
    match start {
        Some(s) if t > s => i18n::duration_label(lang, i64::from(minutes_of(t) - minutes_of(s))),
        _ => String::new(),
    }
}

/// The time of day the reminder's list names beside its row: 08:00 pagi,
/// 12:00 siang, 17:00 sore, 20:00 malam (morning, noon, afternoon,
/// evening); `None` for every other time.
pub fn period_label(lang: i18n::Lang, t: NaiveTime) -> Option<&'static str> {
    let key = match (t.hour(), t.minute()) {
        (8, 0) => Key::PeriodMorning,
        (12, 0) => Key::PeriodNoon,
        (17, 0) => Key::PeriodAfternoon,
        (20, 0) => Key::PeriodEvening,
        _ => return None,
    };
    Some(i18n::text(lang, key))
}

/// The end after the start moved from `old_start` to `new_start`: the same
/// length (an hour when the old end was not after the old start), cut at
/// the end of the day.
pub fn shift_end(old_start: NaiveTime, old_end: NaiveTime, new_start: NaiveTime) -> NaiveTime {
    let length = if old_end > old_start {
        minutes_of(old_end) - minutes_of(old_start)
    } else {
        60
    };
    from_minutes(minutes_of(new_start) + length)
}

// ---- Dates ----

/// A keyboard step in the calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateStep {
    Days(i64),
    Months(i32),
}

impl DateStep {
    /// Arrows move a day or a week, PageUp/PageDown a month.
    pub fn for_key(key: u16) -> Option<DateStep> {
        Some(match key {
            k if k == VK_LEFT.0 => DateStep::Days(-1),
            k if k == VK_RIGHT.0 => DateStep::Days(1),
            k if k == VK_UP.0 => DateStep::Days(-7),
            k if k == VK_DOWN.0 => DateStep::Days(7),
            k if k == VK_PRIOR.0 => DateStep::Months(-1),
            k if k == VK_NEXT.0 => DateStep::Months(1),
            _ => return None,
        })
    }
}

/// `d` moved by `step`; a month step keeps the day, clamped to the month's
/// last ("31 Jan" + 1 month is "28 Feb"). Out of range leaves `d`.
pub fn step_date(d: NaiveDate, step: DateStep) -> NaiveDate {
    match step {
        DateStep::Days(n) if n >= 0 => d.checked_add_days(Days::new(n as u64)),
        DateStep::Days(n) => d.checked_sub_days(Days::new(n.unsigned_abs())),
        DateStep::Months(n) if n >= 0 => d.checked_add_months(Months::new(n as u32)),
        DateStep::Months(n) => d.checked_sub_months(Months::new(n.unsigned_abs())),
    }
    .unwrap_or(d)
}

// ---- Geometry (logical px) ----

const CAL_W: f32 = 268.0;
const CAL_PAD: f32 = 12.0;
const CAL_HEAD_H: f32 = 24.0;
const WEEK_Y: f32 = CAL_PAD + CAL_HEAD_H + 8.0;
const WEEK_H: f32 = 16.0;
const GRID_Y: f32 = WEEK_Y + WEEK_H;
const CELL_PITCH: f32 = 34.0;
const CELL_GAP: f32 = 2.0;
const DAY_CIRCLE: f32 = 30.0;
const CAL_FOOT_Y: f32 = GRID_Y + 5.0 * CELL_PITCH + (CELL_PITCH - CELL_GAP) + 6.0;
const CAL_H: f32 = CAL_FOOT_Y + 1.0 + 4.0 + 20.0 + 9.0;

const TIMES_W: f32 = 186.0;
const TIMES_PAD: f32 = 6.0;
const TIME_ROW: f32 = 30.0;
const TIME_ROWS: usize = 7;
const TYPE_H: f32 = 30.0;
const LIST_Y: f32 = TIMES_PAD + TYPE_H + 5.0;
const TIMES_H: f32 = LIST_Y + TIME_ROWS as f32 * TIME_ROW + TIMES_PAD;
/// Rows of the list: radius 9.
const ROW_RADIUS: f32 = 9.0;
/// The popover's own corners (DWM rounds the window; the design's 16).
const GAP_UNDER_CHIP: f32 = 6.0;

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

fn cal_cell_w() -> f32 {
    (CAL_W - 2.0 * CAL_PAD) / 7.0
}

fn cal_prev() -> Rect {
    rect(CAL_W - CAL_PAD - 52.0, CAL_PAD, 26.0, CAL_HEAD_H)
}

fn cal_next() -> Rect {
    rect(CAL_W - CAL_PAD - 26.0, CAL_PAD, 26.0, CAL_HEAD_H)
}

fn cal_today_link(width: f32) -> Rect {
    rect(CAL_PAD, CAL_FOOT_Y + 5.0, width + 8.0, 20.0)
}

fn time_row(i: usize) -> Rect {
    rect(
        TIMES_PAD,
        LIST_Y + i as f32 * TIME_ROW,
        TIMES_W - 2.0 * TIMES_PAD,
        TIME_ROW,
    )
}

fn type_box() -> Rect {
    rect(TIMES_PAD, TIMES_PAD, TIMES_W - 2.0 * TIMES_PAD, TYPE_H)
}

// ---- State ----

/// What a popover is opened with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    Date {
        selected: NaiveDate,
        today: NaiveDate,
        /// Days before today drawn dim (the reminder's picker).
        dim_past: bool,
    },
    Time {
        value: NaiveTime,
        /// The event's start, for the end list's durations.
        start: Option<NaiveTime>,
        /// Name the times of day beside 08:00, 12:00, 17:00 and 20:00 (the
        /// reminder's list).
        periods: bool,
    },
}

/// What the user picked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Pick {
    Date(NaiveDate),
    Time(NaiveTime),
}

enum Content {
    Calendar {
        today: NaiveDate,
        /// The keyboard's day, drawn as the pick; its month is shown.
        cursor: NaiveDate,
        hover: Option<NaiveDate>,
        dim_past: bool,
    },
    Times {
        entries: Vec<NaiveTime>,
        start: Option<NaiveTime>,
        periods: bool,
        /// The chip's value when opened, in the accent.
        value: Option<usize>,
        /// The keyboard's row.
        cursor: usize,
        /// The first row shown.
        scroll: usize,
        hover: Option<usize>,
        /// The EDIT was typed into since the last arrow key.
        typed: bool,
        /// What was typed does not read as a time.
        invalid: bool,
    },
}

/// Something in a popover the mouse can hit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Spot {
    Prev,
    Next,
    Today,
    Day(NaiveDate),
    Row(usize),
}

struct Popover {
    hwnd: HWND,
    editor: HWND,
    item: Item,
    gfx: Rc<Gfx>,
    target: Option<Target>,
    scale: f32,
    content: Content,
    /// The time list's typing field.
    edit: Option<HWND>,
    font: HFONT,
    pressed: Option<Spot>,
    tracking: bool,
    /// Set once closing started (deactivated, picked or Esc).
    closing: bool,
    /// `CLOSED_*` flags sent with `WM_APP_POPOVER_CLOSED`.
    flags: usize,
}

impl Drop for Popover {
    fn drop(&mut self) {
        if !self.font.is_invalid() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.font.0));
            }
        }
    }
}

thread_local! {
    static POPOVER: RefCell<Option<Popover>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Popover) -> R) -> Option<R> {
    POPOVER.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
}

/// The tip of the date popover's month arrows; PgUp / PgDn step the same way.
fn month_nav_tip(next: bool) -> Tip {
    if next {
        Tip::named(Key::TipCalNextMonth).key(KeyHint::Raw("PgDn"))
    } else {
        Tip::named(Key::TipCalPrevMonth).key(KeyHint::Raw("PgUp"))
    }
}

/// The open popover's chip, and whether it is already closing.
pub(super) fn current() -> Option<(Item, bool)> {
    POPOVER.with(|cell| {
        cell.try_borrow()
            .ok()?
            .as_ref()
            .map(|p| (p.item, p.closing))
    })
}

/// Closes the popover (if any) without handing the keyboard back.
pub(super) fn close() {
    let hwnd = with(|p| {
        p.closing = true;
        p.hwnd
    });
    if let Some(hwnd) = hwnd {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

/// The island is ending: closes the popover and drops what is left in its
/// slot (its Direct2D objects) now, not in the thread-local destructors
/// during process exit.
pub(super) fn release() {
    close();
    let left = POPOVER.with(|cell| cell.try_borrow_mut().ok().and_then(|mut s| s.take()));
    drop(left);
}

/// Repaints in the palette (and language) now in effect.
pub(super) fn restyle() {
    let Some((hwnd, edit)) = with(|p| (p.hwnd, p.edit)) else {
        return;
    };
    set_frame(hwnd, &theme::current_window(), true);
    if let Some(edit) = edit {
        set_cue(edit);
    }
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
        if let Some(edit) = edit {
            let _ = InvalidateRect(Some(edit), None, true);
        }
    }
}

/// The typing EDIT's cue banner, in the language now in effect.
fn set_cue(edit: HWND) {
    let cue = HSTRING::from(tr(Key::TypeTime));
    send(edit, EM_SETCUEBANNER, 1, cue.as_ptr() as isize);
}

const CLASS_NAME: PCWSTR = w!("ValeEditorPopover");
const SUBCLASS_ID: usize = 2;
const ID_TYPE: i32 = 401;

fn register_class(instance: HINSTANCE) -> Result<()> {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            let error = Error::from_thread();
            if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                return Err(error);
            }
        }
    }
    Ok(())
}

/// Where a `w` × `h` popover goes for a chip at `chip` (screen px): under
/// it, left edges aligned, or above it when the work area ends first; kept
/// inside `work` sideways.
pub fn place(chip: RECT, w: i32, h: i32, gap: i32, work: RECT) -> POINT {
    let x = chip.left.min(work.right - w).max(work.left);
    let below = chip.bottom + gap;
    let y = if below + h <= work.bottom {
        below
    } else {
        (chip.top - gap - h).max(work.top)
    };
    POINT { x, y }
}

/// Opens the popover for `item` under `chip` (screen px). Closes any other.
pub(super) fn open(editor: HWND, chip: RECT, gfx: Rc<Gfx>, item: Item, kind: Kind) -> Result<HWND> {
    close();
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    register_class(instance)?;
    let (scale, work) = monitor_at(POINT {
        x: chip.left,
        y: chip.bottom,
    });
    let (lw, lh) = match kind {
        Kind::Date { .. } => (CAL_W, CAL_H),
        Kind::Time { .. } => (TIMES_W, TIMES_H),
    };
    let px = |v: f32| (v * scale).round() as i32;
    let (w, h) = (px(lw), px(lh));
    let at = place(chip, w, h, px(GAP_UNDER_CHIP), work);
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS_NAME,
            w!(""),
            WS_POPUP | WS_CLIPCHILDREN,
            at.x,
            at.y,
            w,
            h,
            Some(editor),
            None,
            Some(instance),
            None,
        )?
    };
    set_frame(hwnd, &theme::current_window(), true);
    let content = match kind {
        Kind::Date {
            selected,
            today,
            dim_past,
        } => Content::Calendar {
            today,
            cursor: selected,
            hover: None,
            dim_past,
        },
        Kind::Time {
            value,
            start,
            periods,
        } => {
            let entries = time_entries(value, start);
            let at = entries
                .iter()
                .position(|t| *t == from_minutes(minutes_of(value)));
            let cursor = at.unwrap_or(0);
            Content::Times {
                scroll: scroll_for(cursor, entries.len(), None),
                entries,
                start,
                periods,
                value: at,
                cursor,
                hover: None,
                typed: false,
                invalid: false,
            }
        }
    };
    let (edit, font) = match kind {
        Kind::Time { value, .. } => {
            let font = edit_font(scale, 12.0, 500);
            let r = type_box();
            let line = (16.0 * scale).round() as i32;
            let edit = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("EDIT"),
                    &HSTRING::from(i18n::time_label(i18n::lang(), value)),
                    WS_CHILD
                        | WS_VISIBLE
                        | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                            ES_AUTOHSCROLL as u32,
                        ),
                    px(r.x + 10.0),
                    px(r.y + r.h / 2.0) - line / 2,
                    px(r.w - 20.0),
                    line,
                    Some(hwnd),
                    Some(HMENU(ID_TYPE as isize as *mut c_void)),
                    Some(instance),
                    None,
                )
            }
            .ok();
            if let Some(edit) = edit {
                send(edit, WM_SETFONT, font.0 as usize, 0);
                set_cue(edit);
                unsafe {
                    let _ = SetWindowSubclass(edit, Some(edit_proc), SUBCLASS_ID, 0);
                }
            }
            (edit, font)
        }
        Kind::Date { .. } => (None, HFONT::default()),
    };
    let state = Popover {
        hwnd,
        editor,
        item,
        gfx,
        target: None,
        scale,
        content,
        edit,
        font,
        pressed: None,
        tracking: false,
        closing: false,
        flags: 0,
    };
    let stored = POPOVER.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            *slot = Some(state);
            None
        }
        Err(_) => Some(state),
    });
    if stored.is_some() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(Error::empty());
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        match edit {
            Some(edit) => {
                let _ = SetFocus(Some(edit));
                send(edit, EM_SETSEL, 0, -1);
            }
            None => {
                let _ = SetFocus(Some(hwnd));
            }
        }
    }
    Ok(hwnd)
}

/// The first row to show so `cursor` is in view: centred-ish on open
/// (`current` = `None`), otherwise moved only as far as needed.
fn scroll_for(cursor: usize, len: usize, current: Option<usize>) -> usize {
    let max = len.saturating_sub(TIME_ROWS);
    let scroll = match current {
        None => cursor.saturating_sub(2),
        Some(s) if cursor < s => cursor,
        Some(s) if cursor >= s + TIME_ROWS => cursor + 1 - TIME_ROWS,
        Some(s) => s,
    };
    scroll.min(max)
}

impl Popover {
    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn spot_at(&self, lparam: LPARAM) -> Option<Spot> {
        let x = (lparam.0 & 0xFFFF) as i16 as f32 / self.scale;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / self.scale;
        match &self.content {
            Content::Calendar { cursor, .. } => {
                if cal_prev().contains(x, y) {
                    return Some(Spot::Prev);
                }
                if cal_next().contains(x, y) {
                    return Some(Spot::Next);
                }
                let today_w = self.gfx.width(tr(Key::DayToday), Text::Small);
                if cal_today_link(today_w).contains(x, y) {
                    return Some(Spot::Today);
                }
                cells(*cursor)
                    .into_iter()
                    .find(|(_, r)| r.contains(x, y))
                    .map(|(d, _)| Spot::Day(d))
            }
            Content::Times {
                entries, scroll, ..
            } => (0..TIME_ROWS)
                .map(|i| (scroll + i, time_row(i)))
                .find(|(i, r)| *i < entries.len() && r.contains(x, y))
                .map(|(i, _)| Spot::Row(i)),
        }
    }

    fn set_hover(&mut self, spot: Option<Spot>) {
        match &mut self.content {
            Content::Calendar { hover, .. } => {
                let day = match spot {
                    Some(Spot::Day(d)) => Some(d),
                    _ => None,
                };
                if *hover != day {
                    *hover = day;
                    self.invalidate();
                }
            }
            Content::Times { hover, .. } => {
                let row = match spot {
                    Some(Spot::Row(i)) => Some(i),
                    _ => None,
                };
                if *hover != row {
                    *hover = row;
                    self.invalidate();
                }
            }
        }
    }

    /// Moves the calendar's day.
    fn step(&mut self, step: DateStep) {
        if let Content::Calendar { cursor, .. } = &mut self.content {
            *cursor = step_date(*cursor, step);
            self.invalidate();
        }
    }

    /// Moves the time list's row by `by`, writing it into the typing field.
    fn move_row(&mut self, by: isize) -> Option<(HWND, String)> {
        let Content::Times {
            entries,
            cursor,
            scroll,
            typed,
            invalid,
            ..
        } = &mut self.content
        else {
            return None;
        };
        let last = entries.len().saturating_sub(1) as isize;
        *cursor = (*cursor as isize + by).clamp(0, last) as usize;
        *scroll = scroll_for(*cursor, entries.len(), Some(*scroll));
        *typed = false;
        *invalid = false;
        let label = i18n::time_label(i18n::lang(), entries[*cursor]);
        self.invalidate();
        self.edit.map(|e| (e, label))
    }

    fn scroll_by(&mut self, rows: isize) {
        if let Content::Times {
            entries, scroll, ..
        } = &mut self.content
        {
            let max = entries.len().saturating_sub(TIME_ROWS) as isize;
            *scroll = (*scroll as isize + rows).clamp(0, max) as usize;
            self.invalidate();
        }
    }

    /// What Enter picks now: the calendar's day, the typed time, or the
    /// time list's row. `None` when the typed text is not a time.
    fn enter_pick(&mut self) -> Option<Pick> {
        let typed_text = self.edit.map(window_text);
        match &mut self.content {
            Content::Calendar { cursor, .. } => Some(Pick::Date(*cursor)),
            Content::Times {
                entries,
                cursor,
                typed,
                invalid,
                ..
            } => {
                if *typed {
                    let parsed = typed_text.as_deref().and_then(parse_time);
                    *invalid = parsed.is_none();
                    self.invalidate();
                    parsed.map(Pick::Time)
                } else {
                    entries.get(*cursor).copied().map(Pick::Time)
                }
            }
        }
    }

    /// The typing field changed: follow it in the list when it reads as a time.
    fn on_typed(&mut self, text: &str) {
        if let Content::Times {
            entries,
            cursor,
            scroll,
            typed,
            invalid,
            ..
        } = &mut self.content
        {
            *typed = true;
            *invalid = false;
            if let Some(t) = parse_time(text)
                && let Some(i) = entries.iter().position(|e| *e == t)
            {
                *cursor = i;
                *scroll = scroll_for(i, entries.len(), Some(*scroll));
            }
            self.invalidate();
        }
    }

    fn render(&mut self) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let size = (
            (rc.right - rc.left).max(1) as u32,
            (rc.bottom - rc.top).max(1) as u32,
        );
        let p = theme::current_window();
        let gfx = Rc::clone(&self.gfx);
        let content = &self.content;
        let result = gfx.paint(
            &mut self.target,
            self.hwnd,
            size,
            self.scale * 96.0,
            p,
            p.popover,
            |painter| match content {
                Content::Calendar {
                    today,
                    cursor,
                    hover,
                    dim_past,
                } => paint_calendar(painter, *today, *cursor, *hover, *dim_past),
                Content::Times {
                    entries,
                    start,
                    periods,
                    value,
                    cursor,
                    scroll,
                    hover,
                    invalid,
                    ..
                } => paint_times(
                    painter,
                    entries,
                    RowNote {
                        start: *start,
                        periods: *periods,
                    },
                    *value,
                    *cursor,
                    *scroll,
                    *hover,
                    *invalid,
                ),
            },
        );
        if let Err(error) = result {
            eprintln!("vale: popover draw failed: {error}");
        }
    }
}

/// The 42 day cells of `cursor`'s month.
fn cells(cursor: NaiveDate) -> Vec<(NaiveDate, Rect)> {
    month_cells(cursor, CAL_PAD, GRID_Y, cal_cell_w(), CELL_PITCH, CELL_GAP)
}

impl CellCanvas for Painter<'_> {
    fn disc(&self, circle: Rect, rgb: u32) {
        self.fill_round(circle, circle.w / 2.0, rgb, 1.0);
    }

    fn ring(&self, circle: Rect, width: f32, rgb: u32) {
        self.stroke(circle, circle.w / 2.0, width, rgb, 1.0);
    }

    fn number(&self, text: &str, area: Rect, rgb: u32, bold: bool) {
        let style = if bold { Text::DayBold } else { Text::Day };
        self.text(text, style, Align::Center, area, rgb, 1.0);
    }
}

/// How day `d` of the calendar showing `cursor` is drawn: the cursor is the
/// pick; with `dim_past`, days before `today` are dim.
fn calendar_day_state(
    d: NaiveDate,
    today: NaiveDate,
    cursor: NaiveDate,
    hover: Option<NaiveDate>,
    dim_past: bool,
) -> DayState {
    DayState {
        hover: hover == Some(d),
        past: dim_past && d < today,
        ..DayState::of(d, today, cursor, cursor)
    }
}

fn paint_calendar(
    pt: &Painter,
    today: NaiveDate,
    cursor: NaiveDate,
    hover: Option<NaiveDate>,
    dim_past: bool,
) {
    let p = pt.p;
    let lang = i18n::lang();
    let title = rect(CAL_PAD + 2.0, CAL_PAD, 180.0, CAL_HEAD_H);
    pt.text(
        &i18n::month_title(lang, cursor),
        Text::Month,
        Align::Left,
        title,
        p.text,
        1.0,
    );
    pt.icon(Icon::ChevL, cal_prev(), 16.0, p.secondary, 1.0, false);
    pt.icon(Icon::ChevR, cal_next(), 16.0, p.secondary, 1.0, false);
    let grid = cells(cursor);
    for (d, r) in grid.iter().take(7) {
        let label = rect(r.x, WEEK_Y, r.w, WEEK_H);
        pt.text(
            i18n::day_short(lang, *d),
            Text::Weekday,
            Align::Center,
            label,
            p.secondary,
            1.0,
        );
    }
    let look = CellLook {
        filled: Filled::Selected,
        ring_w: 1.5,
        ring_ink_accent: true,
        accent: p.accent,
        on_accent: p.on_accent,
        text: p.text,
        dim: p.tertiary,
        dot: p.accent,
        hover: Some(p.hover),
    };
    for (d, r) in &grid {
        let state = calendar_day_state(*d, today, cursor, hover, dim_past);
        paint_day(pt, *d, day_geometry(*r, 0.0, DAY_CIRCLE), state, &look);
    }
    let rule = rect(CAL_PAD, CAL_FOOT_Y, CAL_W - 2.0 * CAL_PAD, 1.0);
    pt.rule(rule, p.border);
    let today_label = tr(Key::DayToday);
    let link = cal_today_link(pt.width(today_label, Text::Small));
    let inner = rect(link.x + 4.0, link.y, link.w - 4.0, link.h);
    pt.text(today_label, Text::Small, Align::Left, inner, p.accent, 1.0);
    let picked = rect(
        link.right(),
        link.y,
        CAL_W - CAL_PAD - 4.0 - link.right(),
        link.h,
    );
    let label = calendar_foot_note(lang, cursor, dim_past);
    pt.text(&label, Text::Small, Align::Right, picked, p.secondary, 1.0);
}

/// The calendar footer's right side. The event picker names the picked day; the
/// reminder's calendar dims past days, so it says why instead.
fn calendar_foot_note(lang: i18n::Lang, cursor: NaiveDate, dim_past: bool) -> String {
    if dim_past {
        i18n::text(lang, Key::PastDatesDimmed).to_owned()
    } else {
        i18n::day_date_short(lang, cursor)
    }
}

/// What the time list writes right of each row.
#[derive(Clone, Copy)]
struct RowNote {
    /// The end list: the length after this start.
    start: Option<NaiveTime>,
    /// The reminder's list: the time of day.
    periods: bool,
}

impl RowNote {
    fn text(self, lang: i18n::Lang, t: NaiveTime) -> String {
        if self.periods {
            period_label(lang, t).unwrap_or_default().to_owned()
        } else {
            entry_duration(lang, t, self.start)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_times(
    pt: &Painter,
    entries: &[NaiveTime],
    note: RowNote,
    value: Option<usize>,
    cursor: usize,
    scroll: usize,
    hover: Option<usize>,
    invalid: bool,
) {
    let p = pt.p;
    let lang = i18n::lang();
    // The typing field: field fill, hairline, accent (or error) underline.
    let field = type_box();
    pt.fill_round(field, ROW_RADIUS, p.field, 1.0);
    pt.hairline(field, ROW_RADIUS, p.border, 1.0);
    let under = if invalid { p.error } else { p.accent };
    pt.fill_round(
        rect(field.x + 8.0, field.bottom() - 4.0, field.w - 16.0, 1.5),
        0.75,
        under,
        1.0,
    );
    pt.icon(
        Icon::Clock,
        rect(field.right() - 26.0, field.y, 20.0, field.h),
        13.0,
        p.secondary,
        1.0,
        false,
    );
    pt.rule(
        rect(
            TIMES_PAD + 4.0,
            LIST_Y - 3.0,
            TIMES_W - 2.0 * TIMES_PAD - 8.0,
            1.0,
        ),
        p.border,
    );
    for row in 0..TIME_ROWS {
        let i = scroll + row;
        let Some(t) = entries.get(i) else {
            break;
        };
        let r = time_row(row);
        let on = value == Some(i);
        if on {
            pt.fill_rgba(r, ROW_RADIUS, p.selection, 1.0);
        } else if hover == Some(i) || cursor == i {
            pt.fill_round(r, ROW_RADIUS, p.hover, 1.0);
        }
        if cursor == i && !on {
            // The keyboard's row also gets a thin ring.
            pt.stroke(r, ROW_RADIUS, 1.0, p.accent, 0.6);
        } else if cursor == i && on {
            pt.stroke(r, ROW_RADIUS, 1.0, p.accent, 0.8);
        }
        let inner = rect(r.x + 10.0, r.y, r.w - 20.0, r.h);
        // A time of day stays secondary on the picked row (`.times small`).
        let (style, ink, ink2) = if on {
            let note_ink = if note.periods { p.secondary } else { p.accent };
            (Text::BodyStrong, p.accent, note_ink)
        } else {
            (Text::Body, p.text, p.secondary)
        };
        pt.text(
            &i18n::time_label(lang, *t),
            style,
            Align::Left,
            inner,
            ink,
            1.0,
        );
        let side = note.text(lang, *t);
        pt.text(&side, Text::Mini, Align::Right, inner, ink2, 1.0);
    }
}

// ---- Window ----

/// Hands `pick` to the editor and closes.
fn finish(hwnd: HWND, pick: Option<Pick>, flags: usize) {
    let target = with(|p| {
        p.closing = true;
        p.flags = flags;
        (p.editor, p.item)
    });
    let Some((editor, item)) = target else {
        return;
    };
    if let Some(pick) = pick {
        let ptr = Box::into_raw(Box::new((item, pick)));
        let posted = unsafe {
            PostMessageW(
                Some(editor),
                WM_APP_POPOVER_PICK,
                WPARAM(0),
                LPARAM(ptr as isize),
            )
        };
        if posted.is_err() {
            drop(unsafe { Box::from_raw(ptr) });
        }
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

/// Keys for the whole popover (also from the typing field). True when used.
fn on_key(hwnd: HWND, key: u16) -> bool {
    tooltip::hide(hwnd);
    let ctrl = key_down(VK_CONTROL.0);
    let back = CLOSED_REFOCUS | CLOSED_KEYBOARD;
    if key == VK_ESCAPE.0 || key == VK_TAB.0 {
        finish(hwnd, None, back);
        return true;
    }
    if key == VK_RETURN.0 && ctrl {
        // Ctrl+Enter saves from anywhere: take the current pick along.
        let pick = with(Popover::enter_pick).flatten();
        finish(hwnd, pick, back | CLOSED_SAVE);
        return true;
    }
    if key == VK_RETURN.0 || (key == VK_SPACE.0 && with(|p| p.edit.is_none()) == Some(true)) {
        // Text that is not a time stays, underlined in the error colour.
        if let Some(pick) = with(Popover::enter_pick).flatten() {
            finish(hwnd, Some(pick), back);
        }
        return true;
    }
    let is_calendar = with(|p| matches!(p.content, Content::Calendar { .. })) == Some(true);
    if is_calendar {
        if let Some(step) = DateStep::for_key(key) {
            with(|p| p.step(step));
            return true;
        }
        if key == VK_HOME.0 {
            with(|p| {
                if let Content::Calendar { today, cursor, .. } = &mut p.content {
                    *cursor = *today;
                }
                p.invalidate();
            });
            return true;
        }
        return false;
    }
    let by = match key {
        k if k == VK_UP.0 => -1,
        k if k == VK_DOWN.0 => 1,
        k if k == VK_PRIOR.0 => -(TIME_ROWS as isize),
        k if k == VK_NEXT.0 => TIME_ROWS as isize,
        k if k == VK_HOME.0 && ctrl => isize::MIN / 2,
        k if k == VK_END.0 && ctrl => isize::MAX / 2,
        _ => return false,
    };
    if let Some(Some((edit, label))) = with(|p| p.move_row(by)) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
                edit,
                &HSTRING::from(label),
            );
        }
        send(edit, EM_SETSEL, 0, -1);
        // SetWindowText sent EN_CHANGE: that was not typing.
        with(|p| {
            if let Content::Times { typed, .. } = &mut p.content {
                *typed = false;
            }
        });
    }
    true
}

fn on_mouse_move(hwnd: HWND, lparam: LPARAM) {
    let moved = with(|p| {
        let spot = p.spot_at(lparam);
        p.set_hover(spot);
        // The month arrows have a tip; mapped to the screen after the
        // borrow, since `ClientToScreen` can send messages to this window.
        let nav = match spot {
            Some(Spot::Prev) => Some((false, cal_prev(), p.scale)),
            Some(Spot::Next) => Some((true, cal_next(), p.scale)),
            _ => None,
        };
        (nav, !std::mem::replace(&mut p.tracking, true))
    });
    if let Some((nav, _)) = &moved {
        tooltip::hover(
            hwnd,
            nav.map(|(next, r, scale)| tooltip::Target {
                id: tooltip::id_of(&(2u8, next)),
                anchor: tooltip::to_screen(hwnd, r, scale),
                side: tooltip::Side::Auto,
                tip: month_nav_tip(next),
            }),
        );
    }
    let arm = moved.map(|(_, arm)| arm);
    if arm == Some(true) {
        let mut track = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        unsafe {
            let _ = TrackMouseEvent(&mut track);
        }
    }
}

fn on_click(hwnd: HWND, lparam: LPARAM) {
    let clicked = with(|p| {
        let spot = p.spot_at(lparam);
        let pressed = p.pressed.take();
        spot.filter(|s| Some(*s) == pressed)
    })
    .flatten();
    let back = CLOSED_REFOCUS;
    match clicked {
        Some(Spot::Prev) => {
            with(|p| p.step(DateStep::Months(-1)));
        }
        Some(Spot::Next) => {
            with(|p| p.step(DateStep::Months(1)));
        }
        Some(Spot::Today) => {
            let today = with(|p| match p.content {
                Content::Calendar { today, .. } => Some(today),
                Content::Times { .. } => None,
            })
            .flatten();
            if let Some(today) = today {
                finish(hwnd, Some(Pick::Date(today)), back);
            }
        }
        Some(Spot::Day(d)) => finish(hwnd, Some(Pick::Date(d)), back),
        Some(Spot::Row(i)) => {
            let t = with(|p| match &p.content {
                Content::Times { entries, .. } => entries.get(i).copied(),
                Content::Calendar { .. } => None,
            })
            .flatten();
            if let Some(t) = t {
                finish(hwnd, Some(Pick::Time(t)), back);
            }
        }
        None => {}
    }
}

fn color_edit(wparam: WPARAM) -> LRESULT {
    let hdc = HDC(wparam.0 as *mut c_void);
    let p = theme::current_window();
    let invalid = with(|pop| matches!(pop.content, Content::Times { invalid: true, .. }));
    unsafe {
        SetTextColor(
            hdc,
            colorref(if invalid == Some(true) {
                p.error
            } else {
                p.text
            }),
        );
        SetBkColor(hdc, colorref(p.field));
    }
    LRESULT(brushes(&p).1.0 as isize)
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            with(Popover::render);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            let (w, h) = (
                (lparam.0 & 0xFFFF) as u32,
                ((lparam.0 >> 16) & 0xFFFF) as u32,
            );
            with(|p| {
                if let Some(t) = &p.target {
                    t.resize(w, h);
                }
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            on_mouse_move(hwnd, lparam);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|p| {
                p.tracking = false;
                p.set_hover(None);
            });
            tooltip::hide(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            tooltip::pressed(hwnd);
            with(|p| p.pressed = p.spot_at(lparam));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_click(hwnd, lparam);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            tooltip::hide(hwnd);
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
            let notches = -(delta as isize) / 40;
            let notches = if notches == 0 {
                -(delta.signum() as isize)
            } else {
                notches
            };
            with(|p| match p.content {
                Content::Calendar { .. } => p.step(DateStep::Months(notches.signum() as i32)),
                Content::Times { .. } => p.scroll_by(notches),
            });
            LRESULT(0)
        }
        WM_KEYDOWN if on_key(hwnd, (wparam.0 & 0xFFFF) as u16) => LRESULT(0),
        WM_COMMAND => {
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            if code == EN_CHANGE {
                let edit = HWND(lparam.0 as *mut c_void);
                let text = window_text(edit);
                with(|p| p.on_typed(&text));
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => color_edit(wparam),
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                tooltip::hide(hwnd);
                // A click elsewhere: close, leaving the editor as it is.
                let first = with(|p| !std::mem::replace(&mut p.closing, true));
                if first == Some(true) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            tooltip::hide(hwnd);
            if let Some((editor, flags)) = with(|p| {
                p.closing = true;
                (p.editor, p.flags)
            }) && unsafe { IsWindow(Some(editor)) }.as_bool()
            {
                unsafe {
                    let _ = PostMessageW(
                        Some(editor),
                        WM_APP_POPOVER_CLOSED,
                        WPARAM(flags),
                        LPARAM(0),
                    );
                }
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let gone = POPOVER.with(|cell| {
                let mut slot = cell.try_borrow_mut().ok()?;
                if slot.as_ref().is_some_and(|p| p.hwnd == hwnd) {
                    slot.take()
                } else {
                    None
                }
            });
            drop(gone);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// The typing field: the list's keys go to the popover; Ctrl+A selects all.
unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_KEYDOWN => {
            let key = (wparam.0 & 0xFFFF) as u16;
            if key == VK_A.0 && key_down(VK_CONTROL.0) {
                send(hwnd, EM_SETSEL, 0, -1);
                return LRESULT(0);
            }
            let handled = [
                VK_UP, VK_DOWN, VK_PRIOR, VK_NEXT, VK_RETURN, VK_ESCAPE, VK_TAB,
            ]
            .iter()
            .any(|k| k.0 == key);
            if handled && let Ok(parent) = unsafe { GetParent(hwnd) } {
                on_key(parent, key);
                return LRESULT(0);
            }
        }
        WM_CHAR => {
            if matches!(wparam.0, 0x01 | 0x09 | 0x0A | 0x0D | 0x1B) {
                return LRESULT(0);
            }
        }
        WM_NCDESTROY => unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID);
        },
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn the_calendar_footer_explains_dimmed_days_only_when_they_are_dimmed() {
        let day = date(2026, 10, 26);
        assert_eq!(
            calendar_foot_note(Lang::Id, day, true),
            "Tanggal lewat diredupkan"
        );
        assert_eq!(
            calendar_foot_note(Lang::En, day, true),
            "Past dates are dimmed"
        );
        assert_eq!(
            calendar_foot_note(Lang::Id, day, false),
            i18n::day_date_short(Lang::Id, day)
        );
        assert_eq!(
            calendar_foot_note(Lang::En, day, false),
            i18n::day_date_short(Lang::En, day)
        );
    }

    #[test]
    fn typed_times_parse_in_every_accepted_shape() {
        for (s, want) in [
            ("9", t(9, 0)),
            ("09", t(9, 0)),
            ("930", t(9, 30)),
            ("0930", t(9, 30)),
            ("9:30", t(9, 30)),
            ("09.30", t(9, 30)),
            ("21.15", t(21, 15)),
            ("  7:05 ", t(7, 5)),
            ("0", t(0, 0)),
            ("2359", t(23, 59)),
        ] {
            assert_eq!(parse_time(s), Some(want), "{s:?}");
        }
    }

    #[test]
    fn invalid_typed_times_are_refused() {
        for s in [
            "", " ", "abc", "24", "2400", "9:60", "9:3", "9:300", "123:00", "12345", "9 30", "-9",
            "9:", ":30", "9.3a", "٩:٣٠", "1e3",
        ] {
            assert_eq!(parse_time(s), None, "{s:?}");
        }
    }

    #[test]
    fn the_start_list_runs_every_fifteen_minutes_through_the_day() {
        let list = time_entries(t(10, 0), None);
        assert_eq!(list.len(), 96);
        assert_eq!(list[0], t(0, 0));
        assert_eq!(list[1], t(0, 15));
        assert_eq!(list[95], t(23, 45));
        assert!(list.windows(2).all(|w| w[0] < w[1]));
        // An off-step value is added in order.
        let odd = time_entries(t(13, 20), None);
        assert_eq!(odd.len(), 97);
        let at = odd.iter().position(|x| *x == t(13, 20)).unwrap();
        assert_eq!((odd[at - 1], odd[at + 1]), (t(13, 15), t(13, 30)));
    }

    #[test]
    fn the_end_list_starts_at_the_start_with_durations() {
        let start = t(10, 0);
        let list = time_entries(t(11, 30), Some(start));
        assert_eq!(&list[..3], &[t(10, 0), t(10, 15), t(10, 30)]);
        assert_eq!(*list.last().unwrap(), t(23, 45));
        let id = |x| entry_duration(Lang::Id, x, Some(start));
        assert_eq!(id(t(10, 0)), "");
        assert_eq!(id(t(10, 15)), "15 mnt");
        assert_eq!(id(t(10, 30)), "30 mnt");
        assert_eq!(id(t(11, 0)), "1 j");
        assert_eq!(id(t(11, 30)), "1 j 30 mnt");
        assert_eq!(
            entry_duration(Lang::En, t(11, 30), Some(start)),
            "1 hr 30 min"
        );
        assert_eq!(entry_duration(Lang::En, t(12, 0), Some(start)), "2 hr");
        // The start list has no durations.
        assert_eq!(entry_duration(Lang::Id, t(11, 0), None), "");
        // An end before the start (an invalid event) still shows in the list.
        let bad = time_entries(t(9, 30), Some(start));
        assert_eq!(bad[0], t(9, 30));
    }

    #[test]
    fn the_reminder_list_names_four_times_of_day() {
        let id: Vec<(NaiveTime, &str)> = time_entries(t(9, 0), None)
            .into_iter()
            .filter_map(|x| period_label(Lang::Id, x).map(|p| (x, p)))
            .collect();
        assert_eq!(
            id,
            [
                (t(8, 0), "pagi"),
                (t(12, 0), "siang"),
                (t(17, 0), "sore"),
                (t(20, 0), "malam")
            ]
        );
        assert_eq!(period_label(Lang::En, t(8, 0)), Some("morning"));
        assert_eq!(period_label(Lang::En, t(12, 0)), Some("noon"));
        assert_eq!(period_label(Lang::En, t(17, 0)), Some("afternoon"));
        assert_eq!(period_label(Lang::En, t(20, 0)), Some("evening"));
        assert_eq!(period_label(Lang::Id, t(8, 15)), None);
        // Rows say the period only in the reminder's list; the event's
        // lists keep their durations (the end) or nothing (the start).
        let reminder = RowNote {
            start: None,
            periods: true,
        };
        assert_eq!(reminder.text(Lang::Id, t(17, 0)), "sore");
        assert_eq!(reminder.text(Lang::Id, t(17, 15)), "");
        let end = RowNote {
            start: Some(t(16, 0)),
            periods: false,
        };
        assert_eq!(end.text(Lang::Id, t(17, 0)), "1 j");
        let start = RowNote {
            start: None,
            periods: false,
        };
        assert_eq!(start.text(Lang::Id, t(8, 0)), "");
    }

    #[test]
    fn the_reminder_calendar_dims_days_before_today() {
        let today = date(2026, 10, 26);
        let cursor = date(2026, 10, 27);
        let state = |d, dim| calendar_day_state(d, today, cursor, None, dim);
        assert!(state(date(2026, 10, 25), true).past);
        assert!(state(date(2026, 9, 28), true).past);
        assert!(!state(today, true).past);
        assert!(!state(cursor, true).past);
        // The event's picker dims nothing.
        assert!(!state(date(2026, 10, 25), false).past);
        let day = date(2026, 10, 20);
        let hovered = calendar_day_state(day, today, cursor, Some(day), true);
        assert!(hovered.hover && hovered.past && hovered.in_month);
        assert!(state(cursor, true).selected);
        assert!(state(today, true).today);
    }

    #[test]
    fn moving_the_start_keeps_the_length() {
        assert_eq!(shift_end(t(10, 0), t(11, 30), t(13, 0)), t(14, 30));
        assert_eq!(shift_end(t(10, 0), t(11, 30), t(9, 15)), t(10, 45));
        // Cut at the end of the day.
        assert_eq!(shift_end(t(10, 0), t(12, 0), t(23, 0)), t(23, 59));
        // An invalid pair gets an hour.
        assert_eq!(shift_end(t(10, 0), t(9, 0), t(14, 0)), t(15, 0));
    }

    #[test]
    fn calendar_keys_step_days_weeks_and_months_across_boundaries() {
        let step = |d, key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| {
            step_date(d, DateStep::for_key(key.0).expect("calendar key"))
        };
        let oct31 = date(2026, 10, 31);
        assert_eq!(step(oct31, VK_RIGHT), date(2026, 11, 1));
        assert_eq!(step(date(2026, 11, 1), VK_LEFT), oct31);
        assert_eq!(step(date(2026, 10, 28), VK_DOWN), date(2026, 11, 4));
        assert_eq!(step(date(2026, 10, 3), VK_UP), date(2026, 9, 26));
        assert_eq!(step(date(2026, 12, 30), VK_DOWN), date(2027, 1, 6));
        assert_eq!(step(date(2026, 10, 26), VK_NEXT), date(2026, 11, 26));
        assert_eq!(step(date(2026, 10, 26), VK_PRIOR), date(2026, 9, 26));
        // A month step keeps the day where it can.
        assert_eq!(step(date(2027, 1, 31), VK_NEXT), date(2027, 2, 28));
        assert_eq!(step(date(2026, 3, 31), VK_PRIOR), date(2026, 2, 28));
        assert_eq!(step(date(2027, 1, 15), VK_PRIOR), date(2026, 12, 15));
        assert_eq!(DateStep::for_key(VK_RETURN.0), None);
    }

    #[test]
    fn popovers_open_under_the_chip_and_stay_on_screen() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let chip = RECT {
            left: 800,
            top: 300,
            right: 950,
            bottom: 330,
        };
        assert_eq!(place(chip, 268, 302, 6, work), POINT { x: 800, y: 336 });
        // Near the bottom: above the chip.
        let low = RECT {
            top: 900,
            bottom: 930,
            ..chip
        };
        assert_eq!(place(low, 268, 302, 6, work), POINT { x: 800, y: 592 });
        // Near the right edge: pulled in.
        let right = RECT {
            left: 1800,
            right: 1900,
            ..chip
        };
        assert_eq!(place(right, 268, 302, 6, work).x, 1652);
    }

    #[test]
    fn the_grid_and_list_fit_their_popovers() {
        let grid = cells(date(2026, 10, 28));
        let last = grid.last().unwrap().1;
        assert!(last.right() <= CAL_W - CAL_PAD + 0.01);
        assert!(last.bottom() <= CAL_FOOT_Y);
        assert!(cal_today_link(60.0).bottom() <= CAL_H);
        assert!(time_row(TIME_ROWS - 1).bottom() <= TIMES_H);
        assert_eq!(scroll_for(0, 96, None), 0);
        assert_eq!(scroll_for(40, 96, None), 38);
        assert_eq!(scroll_for(95, 96, None), 89);
        assert_eq!(scroll_for(20, 96, Some(10)), 14);
        assert_eq!(scroll_for(5, 96, Some(10)), 5);
        assert_eq!(scroll_for(12, 96, Some(10)), 10);
    }

    #[test]
    fn the_month_arrows_name_the_month_and_their_keys() {
        use crate::ui::tooltip::KeyHint;
        for lang in [Lang::Id, Lang::En] {
            crate::i18n::set_lang(lang);
            let prev = month_nav_tip(false);
            let next = month_nav_tip(true);
            assert_eq!(prev.keys, [KeyHint::Raw("PgUp")]);
            assert_eq!(next.keys, [KeyHint::Raw("PgDn")]);
            assert_eq!(
                next.name.as_deref(),
                Some(crate::i18n::text(lang, Key::TipCalNextMonth))
            );
            assert_ne!(prev.name, next.name);
        }
    }
}
