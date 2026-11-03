//! The note, event and reminder editors: small frameless windows under the
//! panel, dark or light per the window mode (`theme::current_window`),
//! because the layered island cannot host native text controls. At most one
//! editor exists at a time; it lives on the island's UI thread and reports to
//! the island with `WM_APP_EDITOR_RESULT`.
//!
//! The frame is a `WS_POPUP` rounded by DWM (`DWMWCP_ROUND`, a 1 px border
//! in the palette's hairline colour) and drawn with Direct2D: header, close
//! button, chips, pills, toggle, segments, footer (`editor_paint`, laid out
//! by `editor_layout`). Text stays in real `EDIT` controls (IME, paste,
//! undo) without borders, coloured through `WM_CTLCOLOREDIT`; their frames
//! and placeholders are drawn around and behind them. Dates and times are
//! picked in popovers (`editor_popover`), never Windows' date controls.
//!
//! Every editor's last result is `NoteSaved`, `NoteDeleted`, `ReminderSaved`,
//! `ReminderDeleted` or `Closed` (`EditorResult::closes_editor`), posted as
//! the window goes away. `EventSave` is not last: the event editor waits for
//! `WM_APP_EDITOR_STATUS`.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Once;
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Weekday};
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE,
    WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BeginPaint, CLIP_DEFAULT_PRECIS, ClientToScreen, CreateFontW,
    CreateSolidBrush, DEFAULT_CHARSET, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_WORDBREAK,
    DeleteObject, DrawTextW, EndPaint, GetDC, GetMonitorInfoW, GetTextMetricsW, HBRUSH, HDC, HFONT,
    HGDIOBJ, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE,
    RedrawWindow, ReleaseDC, ScreenToClient, SelectObject, SetBkColor, SetBkMode, SetTextColor,
    TEXTMETRICW, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    EM_GETRECT, EM_SETMARGINS, EM_SETSEL, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
    InitCommonControlsEx, WM_MOUSELEAVE,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT,
    TrackMouseEvent, VK_A, VK_CONTROL, VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, DestroyWindow, EC_LEFTMARGIN, EC_RIGHTMARGIN,
    EN_CHANGE, EN_KILLFOCUS, EN_SETFOCUS, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE,
    ES_READONLY, ES_WANTRETURN, GW_OWNER, GetClientRect, GetForegroundWindow, GetParent, GetWindow,
    GetWindowTextLengthW, GetWindowTextW, HMENU, HTCAPTION, HTCLIENT, HideCaret, IDC_ARROW,
    IsChild, IsWindow, LoadCursorW, MoveWindow, PostMessageW, RegisterClassExW, SW_SHOW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos,
    SetWindowTextW, ShowCaret, ShowWindow, SwitchToThisWindow, WA_INACTIVE, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_ACTIVATE, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETFONT, WM_KEYDOWN,
    WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_NCHITTEST,
    WM_NCLBUTTONDBLCLK, WM_PAINT, WM_SETFOCUS, WM_SETFONT, WM_SIZE, WNDCLASSEXW, WS_CHILD,
    WS_CLIPCHILDREN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{Error, HSTRING, PCWSTR, Result, w};

use super::editor_layout::{
    CHEVRON, DANGER_PAD, Form, Item, Labels, Layout, QuickTime, ROW_ICON, Shortcut, Spec, Text,
    WEEK, layout,
};
use super::editor_paint::{Align, Gfx, Painter, Target};
use super::editor_popover::{
    self as popover, CLOSED_KEYBOARD, CLOSED_REFOCUS, CLOSED_SAVE, Kind, Pick,
    WM_APP_POPOVER_CLOSED, WM_APP_POPOVER_PICK, shift_end,
};
use super::geometry::Rect;
use super::icons::Icon;
use crate::calendar::EventDraft;
use crate::i18n::{self, Key, Lang, text, tr};
use crate::reminders::{Repeat, local_unix};
use crate::theme::{self, Accent, WindowPalette, radius};
use crate::ui::app_icon;

/// Sent to the event editor after a save (defined by the sync thread, which
/// posts it): `wparam` 1 = saved, the editor closes; 0 = failed, `lparam` is a
/// `Box<String>` raw pointer (or 0) the editor frees and shows in red.
pub use crate::calendar::sync::WM_APP_EDITOR_STATUS;

/// Posted by an editor to its owner: `lparam` is a `Box<EditorResult>` raw
/// pointer the owner frees (`take_result`). WM_APP + 1..5 are taken.
pub const WM_APP_EDITOR_RESULT: u32 = WM_APP + 6;
/// Posted by the note editor to itself on deactivation: it must not destroy
/// itself inside `WM_ACTIVATE`. Only editor windows receive it.
const WM_APP_EDITOR_CLOSE: u32 = WM_APP + 7;
/// Posted to the editor by its fields' keyboard: `wparam` is an `Act`.
const WM_APP_EDITOR_ACT: u32 = WM_APP + 22;

#[derive(Clone, Debug, PartialEq)]
pub enum EditorKind {
    /// `id` is `None` for a new note.
    Note { id: Option<String>, text: String },
    /// `event_id` is `None` for a new event. `read_only` (calendar not
    /// writable, or the event does not fit a draft) shows the fields
    /// disabled and only "Tutup". The footer names the Google account when
    /// `calendar_id` is one (it contains "@"), else just "Google Calendar".
    Event {
        event_id: Option<String>,
        calendar_id: Option<String>,
        draft: EventDraft,
        read_only: bool,
    },
    /// `id` is `None` for a new reminder. `days`: a weekly one's weekdays
    /// (an old one saved without them: its date's weekday).
    Reminder {
        id: Option<String>,
        text: String,
        date: NaiveDate,
        time: NaiveTime,
        repeat: Repeat,
        days: Vec<Weekday>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditorResult {
    /// Empty text means "delete" to the notes store.
    NoteSaved {
        id: Option<String>,
        text: String,
    },
    NoteDeleted {
        id: String,
    },
    /// The editor stays open, showing "Menyimpan…", until `WM_APP_EDITOR_STATUS`.
    EventSave {
        event_id: Option<String>,
        calendar_id: Option<String>,
        draft: EventDraft,
    },
    /// Validated: text not empty, and a one-off not in the past. `days`
    /// only for a weekly one (Monday first), else empty.
    ReminderSaved {
        id: Option<String>,
        text: String,
        date: NaiveDate,
        time: NaiveTime,
        repeat: Repeat,
        days: Vec<Weekday>,
    },
    ReminderDeleted {
        id: String,
    },
    Closed,
}

impl EditorResult {
    /// True for the last result an editor posts: the window is gone by the
    /// time the owner reads it.
    pub fn closes_editor(&self) -> bool {
        !matches!(self, EditorResult::EventSave { .. })
    }
}

/// Takes ownership of a `WM_APP_EDITOR_RESULT`'s `lparam`.
///
/// # Safety
/// `lparam` must come from a `WM_APP_EDITOR_RESULT` message, and each message
/// may be taken only once.
pub unsafe fn take_result(lparam: LPARAM) -> Option<EditorResult> {
    let ptr = lparam.0 as *mut EditorResult;
    (!ptr.is_null()).then(|| *unsafe { Box::from_raw(ptr) })
}

/// Gap between the anchor (the panel's bottom edge) and the editor, in px.
const GAP: i32 = 8;
/// A chip click this soon after its popover closed (the same click closed
/// it by deactivation) does not open it again.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// `0xRRGGBB` as GDI's `0x00BBGGRR`.
pub(super) const fn colorref(rgb: u32) -> COLORREF {
    COLORREF((rgb >> 16) & 0xFF | (rgb & 0xFF00) | (rgb & 0xFF) << 16)
}

/// Centered under `anchor` with a `GAP`, then moved inside `work` (or pinned
/// to its top-left corner when it is bigger than `work`).
pub fn editor_rect(anchor: POINT, size: SIZE, work: RECT) -> RECT {
    let clamp = |pos: i32, len: i32, lo: i32, hi: i32| pos.min(hi - len).max(lo);
    let left = clamp(anchor.x - size.cx / 2, size.cx, work.left, work.right);
    let top = clamp(anchor.y + GAP, size.cy, work.top, work.bottom);
    RECT {
        left,
        top,
        right: left + size.cx,
        bottom: top + size.cy,
    }
}

/// The checks behind "Simpan": a title, and for a timed event an end after
/// its start (Google also allows zero-length events; the island does not).
pub fn validate_event(d: &EventDraft) -> std::result::Result<(), Key> {
    if d.title.trim().is_empty() {
        return Err(Key::TitleRequired);
    }
    if !d.all_day && d.end <= d.start {
        return Err(Key::EndBeforeStart);
    }
    Ok(())
}

/// The checks behind the reminder's "Simpan": some text, a date and time
/// that exist as a local time (`due` is `None` when they cannot be worked
/// out; refused for every repeat, so a save is never dropped after the
/// editor has closed), and a one-off reminder due after `now`. A repeating
/// one set in the past simply starts at its next occurrence.
pub fn validate_reminder(
    text: &str,
    due: Option<i64>,
    repeat: Repeat,
    now: i64,
) -> std::result::Result<(), Key> {
    if text.trim().is_empty() {
        return Err(Key::TextRequired);
    }
    let Some(due) = due else {
        return Err(Key::SaveFailed);
    };
    if repeat == Repeat::Once && due <= now {
        return Err(Key::TimePassed);
    }
    Ok(())
}

/// Text for an EDIT control, which needs `\r\n` line breaks.
fn to_edit_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// Text from an EDIT control, with `\n` line breaks.
fn from_edit_text(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// The footer of the event editor: "Google Calendar · rani@example.com"
/// when the calendar is a Google account's own (its id is the address),
/// else just "Google Calendar". Shared, holiday and imported calendars
/// (`…@group.calendar.google.com` and the like) are not an account. Never
/// makes an address up.
pub fn account_label(lang: Lang, calendar_id: Option<&str>) -> String {
    let name = text(lang, Key::GoogleCalendar);
    let is_account = |c: &&str| {
        c.split_once('@').is_some_and(|(user, domain)| {
            !user.is_empty()
                && domain.contains('.')
                && !domain.to_ascii_lowercase().ends_with("calendar.google.com")
        })
    };
    match calendar_id.map(str::trim).filter(is_account) {
        Some(account) => format!("{name} · {account}"),
        None => name.to_owned(),
    }
}

/// The event's date and times as the editor holds them.
#[derive(Clone, Copy, Debug, PartialEq)]
struct EventState {
    date: NaiveDate,
    all_day: bool,
    start: NaiveTime,
    end: NaiveTime,
    /// For the shortcut pills.
    today: NaiveDate,
}

impl EventState {
    /// A timed event that ends at or before its start.
    fn times_invalid(&self) -> bool {
        !self.all_day && self.end <= self.start
    }

    fn draft(&self, title: &str, notes: &str) -> EventDraft {
        EventDraft {
            title: title.trim().to_owned(),
            date: self.date,
            all_day: self.all_day,
            start: self.start,
            end: self.end,
            description: from_edit_text(notes),
        }
    }

    /// Minutes from start to end, for the time row.
    fn length(&self) -> i64 {
        let m = |t: NaiveTime| i64::from(t.hour() * 60 + t.minute());
        m(self.end) - m(self.start)
    }
}

enum Mode {
    Note {
        id: Option<String>,
    },
    Event {
        event_id: Option<String>,
        calendar_id: Option<String>,
        ev: EventState,
        read_only: bool,
        /// An `EventSave` was posted; Simpan stays off until the status arrives.
        saving: bool,
    },
    Reminder {
        id: Option<String>,
        date: NaiveDate,
        time: NaiveTime,
        repeat: Repeat,
        /// The weekly day chips that are on, Monday first.
        days: Vec<Weekday>,
        /// A new reminder whose date, time and repeat are untouched: the
        /// summary invites a pick instead.
        fresh: bool,
    },
}

impl Mode {
    /// A reminder's date moves to `new`. A weekly one that rang only on the
    /// old date's weekday follows it (the old weekly), other days stay.
    fn set_reminder_date(&mut self, new: NaiveDate) {
        if let Mode::Reminder {
            date, days, fresh, ..
        } = self
        {
            if days[..] == [date.weekday()] {
                days[0] = new.weekday();
            }
            *date = new;
            *fresh = false;
        }
    }

    /// A quick-time pill: its date and time as of `now`; the repeat stays.
    fn apply_quick(&mut self, q: QuickTime, now: NaiveDateTime) {
        let (d, t) = q.target(now);
        self.set_reminder_date(d);
        if let Mode::Reminder { time, .. } = self {
            *time = t;
        }
    }

    /// A repeat segment; Weekly with no day on starts with the date's.
    fn set_repeat(&mut self, r: Repeat) {
        if let Mode::Reminder {
            date,
            repeat,
            days,
            fresh,
            ..
        } = self
        {
            *repeat = r;
            if r == Repeat::Weekly && days.is_empty() {
                days.push(date.weekday());
            }
            *fresh = false;
        }
    }

    /// A day chip: on or off. The last one on stays on (false).
    fn toggle_day(&mut self, day: Weekday) -> bool {
        let Mode::Reminder { days, fresh, .. } = self else {
            return false;
        };
        if let Some(i) = days.iter().position(|d| *d == day) {
            if days.len() == 1 {
                return false;
            }
            days.remove(i);
        } else {
            days.push(day);
            *days = crate::reminders::sorted_days(days);
        }
        *fresh = false;
        true
    }
    fn form(&self) -> Form {
        match self {
            Mode::Note { .. } => Form::Note,
            Mode::Event { .. } => Form::Event,
            Mode::Reminder { .. } => Form::Reminder,
        }
    }

    fn read_only(&self) -> bool {
        matches!(
            self,
            Mode::Event {
                read_only: true,
                ..
            }
        )
    }

    /// Simpan is on: the title (an event's) or text (a reminder's) is filled
    /// in, a timed event ends after it starts, and nothing is being saved.
    /// A note always allows it. A reminder's time is checked when it is
    /// pressed (a one-off in the past says so).
    fn can_save(&self, title: &str) -> bool {
        match self {
            Mode::Note { .. } => true,
            Mode::Reminder { .. } => !title.trim().is_empty(),
            Mode::Event {
                ev,
                read_only,
                saving,
                ..
            } => !read_only && !saving && validate_event(&ev.draft(title, "")).is_ok(),
        }
    }

    /// What Simpan posts for an event: `None` when it posts nothing (not an
    /// event, read-only, or already saving), else the checked `EventSave`.
    fn event_save(
        &self,
        title: &str,
        notes: &str,
    ) -> Option<std::result::Result<EditorResult, Key>> {
        let Mode::Event {
            event_id,
            calendar_id,
            ev,
            read_only: false,
            saving: false,
        } = self
        else {
            return None;
        };
        let draft = ev.draft(title, notes);
        Some(validate_event(&draft).map(|()| EditorResult::EventSave {
            event_id: event_id.clone(),
            calendar_id: calendar_id.clone(),
            draft,
        }))
    }

    /// What a reminder's Simpan posts at Unix time `now`: `None` when this
    /// is not a reminder, else the checked `ReminderSaved` (its days only
    /// when it repeats weekly).
    fn reminder_save(
        &self,
        title: &str,
        now: i64,
    ) -> Option<std::result::Result<EditorResult, Key>> {
        let Mode::Reminder {
            id,
            date,
            time,
            repeat,
            days,
            ..
        } = self
        else {
            return None;
        };
        let due = local_unix(*date, *time);
        Some(
            validate_reminder(title, due, *repeat, now).map(|()| EditorResult::ReminderSaved {
                id: id.clone(),
                text: title.trim().to_owned(),
                date: *date,
                time: *time,
                repeat: *repeat,
                days: if *repeat == Repeat::Weekly {
                    days.clone()
                } else {
                    Vec::new()
                },
            }),
        )
    }

    /// The item is usable (read-only events allow only the text and Tutup).
    fn enabled(&self, item: Item) -> bool {
        !self.read_only() || matches!(item, Item::Title | Item::Notes | Item::Cancel | Item::Close)
    }
}

/// The window title for an editor of this kind.
fn title_key(mode: &Mode) -> Key {
    match mode {
        Mode::Note { .. } => Key::EditorNote,
        Mode::Event {
            read_only: true, ..
        } => Key::EditorEvent,
        Mode::Event { event_id: None, .. } => Key::EditorAddEvent,
        Mode::Event { .. } => Key::EditorEditEvent,
        Mode::Reminder { id: None, .. } => Key::EditorNewReminder,
        Mode::Reminder { .. } => Key::EditorEditReminder,
    }
}

/// What a field's keyboard asks of the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Close = 1,
    /// Ctrl+Enter: save from anywhere.
    Save = 2,
    /// Enter in the title: save only when Simpan is on.
    SaveIfReady = 3,
}

impl Act {
    fn from_wparam(v: usize) -> Option<Act> {
        [Act::Close, Act::Save, Act::SaveIfReady]
            .into_iter()
            .find(|a| *a as usize == v)
    }
}

// Control ids of the EDITs.
const ID_TITLE: i32 = 201;
const ID_NOTES: i32 = 207;

/// What the keyboard subclass does with an EDIT (its `dwRefData`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Multi-line: Enter is a newline, Ctrl+Enter saves.
    Multi = 1,
    /// Single-line: Enter saves.
    Line = 2,
}

/// GDI fonts of the EDITs at the editor's DPI, deleted on drop (after the
/// controls are gone).
struct EditFonts {
    title: HFONT,
    notes: HFONT,
    /// Line heights in px, for centring a single-line EDIT.
    title_line: i32,
}

impl EditFonts {
    fn new(form: Form, scale: f32) -> Self {
        let notes_px = if form == Form::Note { 13.0 } else { 12.0 };
        let title = edit_font(scale, 17.0, 500);
        EditFonts {
            title_line: line_height(title),
            title,
            notes: edit_font(scale, notes_px, 400),
        }
    }

    fn for_item(&self, item: Item) -> HFONT {
        if item == Item::Title {
            self.title
        } else {
            self.notes
        }
    }
}

impl Drop for EditFonts {
    fn drop(&mut self) {
        for font in [self.title, self.notes] {
            if !font.is_invalid() {
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(font.0));
                }
            }
        }
    }
}

/// DM Sans (or the GDI fallback) at `px` logical px and a CSS `weight`, with
/// grayscale antialiasing like the Direct2D text around it.
pub(super) fn edit_font(scale: f32, px: f32, weight: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -(px * scale).round() as i32,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            0,
            super::fonts::gdi_text_face(),
        )
    }
}

fn line_height(font: HFONT) -> i32 {
    unsafe {
        let hdc = GetDC(None);
        let old = SelectObject(hdc, HGDIOBJ(font.0));
        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        SelectObject(hdc, old);
        ReleaseDC(None, hdc);
        tm.tmHeight.max(1)
    }
}

struct Editor {
    hwnd: HWND,
    owner: HWND,
    /// Foreground window before the editor opened; gets the focus back.
    previous: HWND,
    mode: Mode,
    scale: f32,
    gfx: Rc<Gfx>,
    target: Option<Target>,
    fonts: EditFonts,
    edits: Vec<(Item, HWND)>,
    layout: Layout,
    /// The drawn item (or EDIT) with the keyboard.
    focus: Option<Item>,
    /// The focus came from the keyboard: draw its ring.
    focus_visible: bool,
    hover: Option<Item>,
    pressed: Option<Item>,
    tracking: bool,
    /// The chip whose popover is open.
    popover: Option<Item>,
    /// The chip whose popover closed last, and when.
    dismissed: Option<(Item, Instant)>,
    /// Footer text instead of the account: "Menyimpan…" or a failure (red).
    status: Option<(String, bool)>,
    /// The one-line error under the time / repeat row.
    error: Option<Key>,
    /// The title EDIT's text, kept current on `EN_CHANGE`.
    title: String,
    /// Set once the window was active: only then does deactivation close a note.
    was_active: bool,
    /// The final result was posted.
    finished: bool,
}

/// Whether the editor may appear on screen and take the foreground. Always
/// true in the app; tests build the real window but never show it, so a test
/// run opens no visible window and steals no focus.
const ON_SCREEN: bool = !cfg!(test);

thread_local! {
    static EDITOR: RefCell<Option<Editor>> = const { RefCell::new(None) };
    /// The window that last had the keyboard (an EDIT or the editor itself),
    /// restored on reactivation.
    static LAST_FOCUS: Cell<isize> = const { Cell::new(0) };
    /// Placeholder of each EDIT, read by its paint (without borrowing `EDITOR`).
    static PLACEHOLDERS: RefCell<Vec<(isize, Key)>> = const { RefCell::new(Vec::new()) };
    /// The EDITs on the window background (not on a field fill).
    static ON_BG: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
    /// Per `(bg, field)` colour pair: the light palette, the warm dark one and
    /// the neutral dark one (Terminal surface) are the only three there are.
    static BRUSHES: Cell<[Option<BrushSlot>; BRUSH_SLOTS]> =
        const { Cell::new([None; BRUSH_SLOTS]) };
}

/// How many `(bg, field)` pairs `WindowPalette` can produce: light, warm dark
/// and neutral dark.
const BRUSH_SLOTS: usize = 3;

/// `((bg, field), (window brush, field brush))`.
type BrushSlot = ((u32, u32), (HBRUSH, HBRUSH));

/// Where the brushes of `key` (a `(bg, field)` pair) live in `keys`: the slot
/// that already holds it, else the first free one. `None` when all are taken
/// by other pairs (cannot happen with the three palettes).
fn brush_slot(keys: &[Option<(u32, u32)>], key: (u32, u32)) -> Option<usize> {
    keys.iter()
        .position(|k| *k == Some(key))
        .or_else(|| keys.iter().position(Option::is_none))
}

/// `(window, field)` brushes of `p`, created once per `(bg, field)` pair (so
/// once per palette, not per mode: Terminal's dark differs from the warm one)
/// and kept for the process lifetime, at most `BRUSH_SLOTS` of them.
pub(super) fn brushes(p: &WindowPalette) -> (HBRUSH, HBRUSH) {
    BRUSHES.with(|b| {
        let mut all = b.get();
        let key = (p.bg, p.field);
        let keys: Vec<Option<(u32, u32)>> = all.iter().map(|s| s.map(|(k, _)| k)).collect();
        let slot = brush_slot(&keys, key).unwrap_or(BRUSH_SLOTS - 1);
        if let Some((k, made)) = all[slot]
            && k == key
        {
            return made;
        }
        let made = unsafe {
            (
                CreateSolidBrush(colorref(p.bg)),
                CreateSolidBrush(colorref(p.field)),
            )
        };
        all[slot] = Some((key, made));
        b.set(all);
        made
    })
}

/// A frameless editor or popover window: Windows 11 rounds its corners and
/// draws a 1 px border in the palette's hairline (over the window's own
/// fill, `popover` or `bg`). Older systems ignore both and keep square edges.
pub(super) fn set_frame(hwnd: HWND, p: &WindowPalette, popover: bool) {
    let corners = DWMWCP_ROUND;
    let base = if popover { p.popover } else { p.bg };
    let alpha = (p.border & 0xFF) as f32 / 255.0;
    let border = colorref(blend(base, p.border >> 8, alpha)).0;
    let dark = i32::from(!p.light);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corners as *const _ as *const c_void,
            size_of_val(&corners) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        );
    }
}

/// Repaints the open editor (if any) and its popover in the window palette
/// now in effect.
pub fn restyle_open_editor() {
    let Some(hwnd) = open_hwnd() else {
        return;
    };
    set_frame(hwnd, &theme::current_window(), false);
    unsafe {
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN,
        );
    }
    popover::restyle();
}

/// Redraws the open editor (if any) in the language now in effect: title,
/// labels, the date chip, placeholders and its popover. A status message
/// already shown keeps its language until the next one.
pub fn relabel_open_editor() {
    let Some(hwnd) = open_hwnd() else {
        return;
    };
    if let Some(title) = with_editor(hwnd, |ed| title_key(&ed.mode)) {
        unsafe {
            let _ = SetWindowTextW(hwnd, &HSTRING::from(tr(title)));
        }
    }
    refresh(hwnd);
    unsafe {
        let _ = RedrawWindow(Some(hwnd), None, None, RDW_INVALIDATE | RDW_ALLCHILDREN);
    }
    popover::restyle();
}

fn open_hwnd() -> Option<HWND> {
    EDITOR.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|slot| slot.as_ref().map(|ed| ed.hwnd))
    })
}

/// Runs `f` on the editor for `hwnd`, or returns `None` when there is none or
/// a handler further up the stack holds it.
fn with_editor<R>(hwnd: HWND, f: impl FnOnce(&mut Editor) -> R) -> Option<R> {
    EDITOR.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        slot.as_mut().filter(|ed| ed.hwnd == hwnd).map(f)
    })
}

/// Removes the editor for `hwnd` from the slot.
fn take_editor(hwnd: HWND) -> Option<Editor> {
    EDITOR.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        if slot.as_ref().is_some_and(|ed| ed.hwnd == hwnd) {
            slot.take()
        } else {
            None
        }
    })
}

pub(super) fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))).0 }
}

pub(super) fn window_text(hwnd: HWND) -> String {
    let len = usize::try_from(unsafe { GetWindowTextLengthW(hwnd) }).unwrap_or(0);
    let mut buf = vec![0u16; len + 1];
    let copied = usize::try_from(unsafe { GetWindowTextW(hwnd, &mut buf) }).unwrap_or(0);
    String::from_utf16_lossy(buf.get(..copied).unwrap_or(&[]))
}

fn post_result(owner: HWND, result: EditorResult) -> bool {
    let ptr = Box::into_raw(Box::new(result));
    let posted = unsafe {
        PostMessageW(
            Some(owner),
            WM_APP_EDITOR_RESULT,
            WPARAM(0),
            LPARAM(ptr as isize),
        )
    };
    if posted.is_err() {
        // The owner never sees it; nobody else will free it.
        drop(unsafe { Box::from_raw(ptr) });
        return false;
    }
    true
}

pub(super) fn init_common_controls() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES,
        };
        unsafe {
            let _ = InitCommonControlsEx(&icc);
        }
    });
}

const CLASS_NAME: PCWSTR = w!("ValeEditor");

fn register_class(instance: HINSTANCE) -> Result<()> {
    let (icon, icon_small) = app_icon::class_icons(instance);
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hIcon: icon,
            hIconSm: icon_small,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            // No class brush: Direct2D paints every pixel.
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

/// DPI scale and work area of the monitor under `pt`.
pub(super) fn monitor_at(pt: POINT) -> (f32, RECT) {
    unsafe {
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi_x, mut dpi_y) = (96, 96);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_err() {
            dpi_x = 96;
        }
        (dpi_x as f32 / 96.0, info.rcWork)
    }
}

/// Opens an editor owned by `owner` (the island), centered `GAP` px below
/// `anchor_bottom_center` (screen px) and kept on that monitor's work area.
/// If an editor is already open, focuses it and returns its handle instead.
pub fn open_editor(owner: HWND, anchor_bottom_center: POINT, kind: EditorKind) -> Result<HWND> {
    if let Some(hwnd) = open_hwnd() {
        if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            bring_to_front(hwnd);
            return Ok(hwnd);
        }
        // Stale: the window died without its destroy messages reaching us.
        EDITOR.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut() {
                *slot = None;
            }
        });
    }

    init_common_controls();
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    register_class(instance)?;
    let gfx = Rc::new(Gfx::new()?);

    let (scale, work) = monitor_at(anchor_bottom_center);
    let today = chrono::Local::now().date_naive();
    let (mode, title, notes) = match kind {
        EditorKind::Note { id, text } => (Mode::Note { id }, None, Some(text)),
        EditorKind::Event {
            event_id,
            calendar_id,
            draft,
            read_only,
        } => (
            Mode::Event {
                event_id,
                calendar_id,
                ev: EventState {
                    date: draft.date,
                    all_day: draft.all_day,
                    start: draft.start,
                    end: draft.end,
                    today,
                },
                read_only,
                saving: false,
            },
            Some(draft.title),
            Some(draft.description),
        ),
        EditorKind::Reminder {
            id,
            text,
            date,
            time,
            repeat,
            days,
        } => {
            let mut days = crate::reminders::sorted_days(&days);
            if repeat == Repeat::Weekly && days.is_empty() {
                days.push(date.weekday());
            }
            (
                Mode::Reminder {
                    fresh: id.is_none(),
                    id,
                    date,
                    time,
                    repeat,
                    days,
                },
                Some(text),
                None,
            )
        }
    };
    let form = mode.form();
    let error = match &mode {
        Mode::Event { ev, .. } if ev.times_invalid() => Some(Key::EndBeforeStart),
        _ => None,
    };
    let lang = i18n::lang();
    let ed_layout = compute_layout(&mode, error.is_some(), lang, &gfx);
    let px = |v: f32| (v * scale).round() as i32;
    let size = SIZE {
        cx: px(ed_layout.size.0),
        cy: px(ed_layout.size.1),
    };
    let rc = editor_rect(anchor_bottom_center, size, work);
    let previous = unsafe { GetForegroundWindow() };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS_NAME,
            &HSTRING::from(tr(title_key(&mode))),
            WS_POPUP | WS_CLIPCHILDREN,
            rc.left,
            rc.top,
            size.cx,
            size.cy,
            Some(owner),
            None,
            Some(instance),
            None,
        )?
    };
    set_frame(hwnd, &theme::current_window(), false);
    let fonts = EditFonts::new(form, scale);
    let read_only = mode.read_only();
    let mut edits = Vec::new();
    let mut make = |item: Item, value: &str| -> Result<()> {
        let (id, role, mut style) = match item {
            Item::Title => (ID_TITLE, Role::Line, ES_AUTOHSCROLL as u32),
            _ => (
                ID_NOTES,
                Role::Multi,
                (ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32,
            ),
        };
        if read_only {
            style |= ES_READONLY as u32;
        }
        let edit = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                &HSTRING::from(to_edit_text(value)),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(style),
                0,
                0,
                0,
                0,
                Some(hwnd),
                Some(HMENU(id as isize as *mut c_void)),
                Some(instance),
                None,
            )?
        };
        send(edit, WM_SETFONT, fonts.for_item(item).0 as usize, 0);
        send(
            edit,
            EM_SETMARGINS,
            (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize,
            0,
        );
        let placeholder = match (form, item) {
            (Form::Event, Item::Title) => Key::TitlePlaceholder,
            (Form::Reminder, Item::Title) => Key::ReminderPlaceholder,
            (Form::Note, _) => Key::NotePlaceholder,
            _ => Key::NotesPlaceholder,
        };
        PLACEHOLDERS.with(|p| p.borrow_mut().push((edit.0 as isize, placeholder)));
        if item == Item::Title {
            ON_BG.with(|b| b.borrow_mut().push(edit.0 as isize));
        }
        unsafe {
            let _ = SetWindowSubclass(edit, Some(control_proc), SUBCLASS_ID, role as usize);
        }
        edits.push((item, edit));
        Ok(())
    };
    let made = (|| {
        if let Some(title) = &title {
            make(Item::Title, title)?;
        }
        if let Some(notes) = &notes {
            make(Item::Notes, notes)?;
        }
        Ok::<(), Error>(())
    })();
    if let Err(error) = made {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(error);
    }
    let first = edits.first().map(|(item, h)| (*item, *h));
    let editor = Editor {
        hwnd,
        owner,
        previous,
        mode,
        scale,
        gfx,
        target: None,
        fonts,
        edits,
        layout: ed_layout,
        focus: first.map(|(item, _)| item),
        focus_visible: false,
        hover: None,
        pressed: None,
        tracking: false,
        popover: None,
        dismissed: None,
        status: None,
        error,
        title: title.unwrap_or_default(),
        was_active: false,
        finished: false,
    };
    LAST_FOCUS.set(first.map_or(hwnd.0 as isize, |(_, h)| h.0 as isize));
    let stored = EDITOR.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            *slot = Some(editor);
            None
        }
        Err(_) => Some(editor),
    });
    if let Some(editor) = stored {
        // Called from inside an editor handler: an unmanaged window could never close.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        drop(editor);
        return Err(Error::empty());
    }
    refresh(hwnd);
    if ON_SCREEN {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
    }
    bring_to_front(hwnd);
    if let Some((_, edit)) = first {
        // Caret at the end of the existing text.
        let end = unsafe { GetWindowTextLengthW(edit) }.max(0) as usize;
        send(edit, EM_SETSEL, end, end as isize);
    }
    Ok(hwnd)
}

/// Brings the open editor forward. False when none is open.
pub fn focus_open_editor() -> bool {
    match open_hwnd() {
        Some(hwnd) if unsafe { IsWindow(Some(hwnd)) }.as_bool() => {
            bring_to_front(hwnd);
            true
        }
        _ => false,
    }
}

/// Makes `hwnd` the foreground window and focuses its last-focused control.
/// The island process received the click that opened the editor, so it may
/// set the foreground; `SwitchToThisWindow` is the fallback if Windows still
/// refuses (it flashes the window rather than failing silently).
fn bring_to_front(hwnd: HWND) {
    if ON_SCREEN {
        unsafe {
            let _ = SetForegroundWindow(hwnd);
            if GetForegroundWindow() != hwnd {
                SwitchToThisWindow(hwnd, true);
            }
        }
    }
    focus_last(hwnd);
}

fn focus_last(hwnd: HWND) {
    let last = HWND(LAST_FOCUS.get() as *mut c_void);
    unsafe {
        if !last.is_invalid() && (last == hwnd || IsChild(hwnd, last).as_bool()) {
            let _ = SetFocus(Some(last));
        }
    }
}

/// What a popover for `item` opens with, `today` being the local date: the
/// reminder's calendar dims the days before today and its time list names
/// the times of day; the event's do neither.
fn popover_kind(mode: &Mode, item: Item, today: NaiveDate) -> Option<Kind> {
    match (mode, item) {
        (Mode::Event { ev, .. }, Item::DateChip) => Some(Kind::Date {
            selected: ev.date,
            today: ev.today,
            dim_past: false,
        }),
        (Mode::Reminder { date, .. }, Item::DateChip) => Some(Kind::Date {
            selected: *date,
            today,
            dim_past: true,
        }),
        (Mode::Event { ev, .. }, Item::StartChip) => Some(Kind::Time {
            value: ev.start,
            start: None,
            periods: false,
        }),
        (Mode::Event { ev, .. }, Item::EndChip) => Some(Kind::Time {
            value: ev.end,
            start: Some(ev.start),
            periods: false,
        }),
        (Mode::Reminder { time, .. }, Item::TimeChip) => Some(Kind::Time {
            value: *time,
            start: None,
            periods: true,
        }),
        _ => None,
    }
}

/// The labels for the layout, in `lang`.
struct LabelText {
    date: String,
    start: String,
    end: String,
    time: String,
    pills: [String; 4],
    /// The reminder's quick-time pills.
    quick: [&'static str; 4],
}

/// A quick-time pill's text: "30 mnt lagi", "1 jam lagi", "Nanti malam",
/// "Besok pagi".
fn quick_label(lang: Lang, q: QuickTime) -> &'static str {
    let key = match q {
        QuickTime::HalfHour => Key::QuickHalfHour,
        QuickTime::Hour => Key::QuickHour,
        QuickTime::Tonight => Key::QuickTonight,
        QuickTime::TomorrowMorning => Key::QuickTomorrowMorning,
    };
    text(lang, key)
}

fn label_text(mode: &Mode, lang: Lang) -> LabelText {
    let now = chrono::Local::now().date_naive();
    let (date, start, end, time, today) = match mode {
        Mode::Event { ev, .. } => (
            i18n::editor_date(lang, ev.date),
            Some(ev.start),
            Some(ev.end),
            None,
            ev.today,
        ),
        Mode::Reminder {
            date, time, repeat, ..
        } => (
            i18n::reminder_date_chip(lang, *repeat, *date, now),
            None,
            None,
            Some(*time),
            now,
        ),
        Mode::Note { .. } => (String::new(), None, None, None, now),
    };
    let t = |v: Option<NaiveTime>| v.map(|t| i18n::time_label(lang, t)).unwrap_or_default();
    LabelText {
        date,
        start: t(start),
        end: t(end),
        time: t(time),
        pills: Shortcut::ALL.map(|s| pill_label(lang, s, today)),
        quick: QuickTime::ALL.map(|q| quick_label(lang, q)),
    }
}

/// A shortcut pill's text: "Hari ini", "Besok", the weekday after tomorrow
/// ("Jumat"), "Minggu depan".
fn pill_label(lang: Lang, s: Shortcut, today: NaiveDate) -> String {
    match s {
        Shortcut::Today => text(lang, Key::DayToday).into(),
        Shortcut::Tomorrow => text(lang, Key::DayTomorrow).into(),
        Shortcut::DayAfter => i18n::day_long(lang, s.date(today)).into(),
        Shortcut::NextWeek => text(lang, Key::NextWeek).into(),
    }
}

fn compute_layout(mode: &Mode, error: bool, lang: Lang, gfx: &Gfx) -> Layout {
    let labels = label_text(mode, lang);
    let (all_day, delete) = match mode {
        Mode::Event { ev, .. } => (ev.all_day, false),
        Mode::Note { id } => (false, id.is_some()),
        Mode::Reminder { id, .. } => (false, id.is_some()),
    };
    let cancel = if mode.read_only() {
        Key::Close
    } else {
        Key::Cancel
    };
    let spec = Spec {
        form: mode.form(),
        read_only: mode.read_only(),
        all_day,
        error,
        delete,
        weekly: matches!(
            mode,
            Mode::Reminder {
                repeat: Repeat::Weekly,
                ..
            }
        ),
        labels: Labels {
            date: &labels.date,
            start: &labels.start,
            end: &labels.end,
            time: &labels.time,
            pills: [
                &labels.pills[0],
                &labels.pills[1],
                &labels.pills[2],
                &labels.pills[3],
            ],
            all_day: text(lang, Key::AllDayToggle),
            repeat: [
                text(lang, Key::RepeatOnce),
                text(lang, Key::RepeatDaily),
                text(lang, Key::RepeatWeekly),
            ],
            quick: labels.quick,
            delete: text(lang, Key::Delete),
            cancel: text(lang, cancel),
            save: text(lang, Key::Save),
        },
    };
    layout(&spec, &|s, t| gfx.width(s, t))
}

/// Where each EDIT goes (client px) and the window's size.
struct Placement {
    size: (i32, i32),
    edits: Vec<(HWND, RECT)>,
}

impl Editor {
    fn edit(&self, item: Item) -> Option<HWND> {
        self.edits.iter().find(|(i, _)| *i == item).map(|(_, h)| *h)
    }

    fn is_note(&self) -> bool {
        matches!(self.mode, Mode::Note { .. })
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn notes_text(&self) -> String {
        self.edit(Item::Notes).map(window_text).unwrap_or_default()
    }

    fn relayout(&mut self) {
        let error = self.error.is_some();
        self.layout = compute_layout(&self.mode, error, i18n::lang(), &self.gfx);
    }

    fn placement(&self) -> Placement {
        let s = self.scale;
        let px = |v: f32| (v * s).round() as i32;
        let edits = self
            .edits
            .iter()
            .filter_map(|(item, hwnd)| {
                let r = self.layout.edits.iter().find(|(i, _)| i == item)?.1;
                let rc = if *item == Item::Title {
                    let mid = px(r.y + r.h / 2.0);
                    let line = self.fonts.title_line;
                    RECT {
                        left: px(r.x),
                        top: mid - line / 2,
                        right: px(r.right()),
                        bottom: mid - line / 2 + line,
                    }
                } else {
                    RECT {
                        left: px(r.x),
                        top: px(r.y),
                        right: px(r.right()),
                        bottom: px(r.bottom()),
                    }
                };
                Some((*hwnd, rc))
            })
            .collect();
        Placement {
            size: (px(self.layout.size.0), px(self.layout.size.1)),
            edits,
        }
    }

    /// The layout item under a client point (px).
    fn hit_at(&self, lparam: LPARAM) -> Option<Item> {
        let x = (lparam.0 & 0xFFFF) as i16 as f32 / self.scale;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / self.scale;
        self.layout.hit(x, y)
    }

    /// Items Tab stops at, in order.
    fn tab_stops(&self) -> Vec<Item> {
        self.layout
            .focus_order()
            .into_iter()
            .filter(|i| self.mode.enabled(*i))
            .collect()
    }

    /// The result closing the window would post: the note's text, or `Closed`.
    fn closing_result(&self) -> EditorResult {
        match &self.mode {
            Mode::Note { id } => EditorResult::NoteSaved {
                id: id.clone(),
                text: from_edit_text(&self.notes_text()),
            },
            Mode::Event { .. } | Mode::Reminder { .. } => EditorResult::Closed,
        }
    }

    /// Reminder "Simpan": the validated result, or `None` with the reason
    /// under the repeat row.
    fn save_reminder(&mut self) -> Option<EditorResult> {
        let now = chrono::Local::now().timestamp();
        match self.mode.reminder_save(&self.title, now)? {
            Ok(result) => Some(result),
            Err(message) => {
                self.error = Some(message);
                None
            }
        }
    }

    /// Simpan: validates, posts `EventSave` and waits for the status.
    fn save_event(&mut self) {
        let notes = self.notes_text();
        match self.mode.event_save(&self.title, &notes) {
            None => {}
            Some(Err(message)) => self.status = Some((tr(message).into(), true)),
            Some(Ok(result)) => {
                if !post_result(self.owner, result) {
                    self.status = Some((tr(Key::SaveFailed).into(), true));
                    return;
                }
                if let Mode::Event { saving, .. } = &mut self.mode {
                    *saving = true;
                }
                self.status = Some((tr(Key::Saving).into(), false));
            }
        }
        self.invalidate();
    }

    /// Applies a popover's pick to the chip it came from.
    fn apply_pick(&mut self, item: Item, pick: Pick) {
        match (&mut self.mode, item, pick) {
            (Mode::Event { ev, .. }, Item::DateChip, Pick::Date(d)) => ev.date = d,
            (Mode::Event { ev, .. }, Item::StartChip, Pick::Time(t)) => {
                ev.end = shift_end(ev.start, ev.end, t);
                ev.start = t;
            }
            (Mode::Event { ev, .. }, Item::EndChip, Pick::Time(t)) => ev.end = t,
            (mode @ Mode::Reminder { .. }, Item::DateChip, Pick::Date(d)) => {
                mode.set_reminder_date(d)
            }
            (Mode::Reminder { time, fresh, .. }, Item::TimeChip, Pick::Time(t)) => {
                *time = t;
                *fresh = false;
            }
            _ => return,
        }
        self.after_change();
    }

    /// The date, times or repeat changed: the error line follows the times.
    fn after_change(&mut self) {
        self.error = match &self.mode {
            Mode::Event { ev, .. } if ev.times_invalid() => Some(Key::EndBeforeStart),
            _ => None,
        };
        if !matches!(self.mode, Mode::Event { saving: true, .. }) {
            self.status = None;
        }
        self.relayout();
        self.invalidate();
    }

    /// What a popover for `item` opens with.
    fn popover_kind(&self, item: Item) -> Option<Kind> {
        popover_kind(&self.mode, item, chrono::Local::now().date_naive())
    }

    /// `item`'s rect on screen.
    fn screen_rect(&self, item: Item) -> Option<RECT> {
        let r = self.layout.item(item)?;
        let s = self.scale;
        let mut a = POINT {
            x: (r.x * s).round() as i32,
            y: (r.y * s).round() as i32,
        };
        let mut b = POINT {
            x: (r.right() * s).round() as i32,
            y: (r.bottom() * s).round() as i32,
        };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut a);
            let _ = ClientToScreen(self.hwnd, &mut b);
        }
        Some(RECT {
            left: a.x,
            top: a.y,
            right: b.x,
            bottom: b.y,
        })
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
        let has_focus = unsafe { GetFocus() } == self.hwnd;
        let gfx = Rc::clone(&self.gfx);
        let mut target = self.target.take();
        let result = gfx.paint(
            &mut target,
            self.hwnd,
            size,
            self.scale * 96.0,
            p,
            p.bg,
            |pt| self.draw(pt, has_focus),
        );
        self.target = target;
        if let Err(error) = result {
            eprintln!("vale: editor draw failed: {error}");
        }
    }

    /// The whole frame. `has_focus`: the editor itself (not an EDIT) has the
    /// keyboard, so a drawn item may show its focus ring.
    fn draw(&self, pt: &Painter, has_focus: bool) {
        let p = pt.p;
        let l = &self.layout;
        let lang = i18n::lang();
        let ring_on = |item: Item| has_focus && self.focus_visible && self.focus == Some(item);
        let hovered = |item: Item| self.hover == Some(item) && self.mode.enabled(item);

        // Header.
        pt.text(
            tr(title_key(&self.mode)),
            Text::Heading,
            Align::Left,
            l.heading,
            p.text,
            1.0,
        );
        if let Some(close) = l.item(Item::Close) {
            let fill = if hovered(Item::Close) {
                p.button_hover
            } else {
                p.hover
            };
            pt.fill_round(close, close.w / 2.0, fill, 1.0);
            let ink = if hovered(Item::Close) {
                p.text
            } else {
                p.secondary
            };
            pt.icon(Icon::X, close, 12.0, ink, 1.0, false);
        }

        // The title line and its rule (accent while the EDIT has the keyboard).
        if let Some(rule) = l.title_rule {
            let title_edit = self.edit(Item::Title);
            let focused =
                title_edit.is_some() && unsafe { GetFocus() } == title_edit.unwrap_or_default();
            if focused {
                pt.fill_round(rule, 0.0, p.accent, 1.0);
            } else {
                pt.rule(rule, p.border);
            }
        }

        for (icon, r) in &l.icons {
            pt.icon(*icon, *r, ROW_ICON, p.secondary, 1.0, false);
        }

        let labels = label_text(&self.mode, lang);
        let (enabled_a, chips_on) = if self.mode.read_only() {
            (0.5, false)
        } else {
            (1.0, true)
        };
        let chip = |item: Item, label: &str, chevron: bool, bad: bool| {
            let Some(r) = l.item(item) else {
                return;
            };
            let open = self.popover == Some(item);
            let a = enabled_a;
            if chips_on && (open || ring_on(item)) {
                let glow = outset(r, 3.0);
                pt.fill_rgba(glow, radius::CONTROL + 3.0, p.selection, 1.0);
            }
            let fill = if hovered(item) && chips_on && !open {
                p.hover
            } else {
                p.field
            };
            pt.fill_round(r, radius::CONTROL, fill, a);
            if chips_on && (open || ring_on(item)) {
                pt.stroke(r, radius::CONTROL, 1.0, p.accent, 1.0);
            } else if bad {
                pt.stroke(r, radius::CONTROL, 1.0, p.error, a);
            } else {
                pt.hairline(r, radius::CONTROL, p.border, a);
            }
            let inner = Rect {
                x: r.x + 12.0,
                w: r.w - 24.0,
                ..r
            };
            pt.text(label, Text::Body, Align::Left, inner, p.text, a);
            if chevron {
                let c = Rect {
                    x: r.right() - 12.0 - CHEVRON,
                    w: CHEVRON,
                    ..r
                };
                pt.icon(Icon::ChevD, c, CHEVRON, p.secondary, a, false);
            }
        };

        match &self.mode {
            Mode::Event { ev, .. } => {
                chip(Item::DateChip, &labels.date, true, false);
                let active = Shortcut::matching(ev.today, ev.date);
                for (s, label) in Shortcut::ALL.into_iter().zip(&labels.pills) {
                    let item = Item::Pill(s);
                    let Some(r) = l.item(item) else {
                        continue;
                    };
                    let a = enabled_a;
                    let full = r.h / 2.0;
                    let ink = if active == Some(s) {
                        pt.fill_rgba(r, full, p.selection, a);
                        p.accent
                    } else {
                        if hovered(item) {
                            pt.fill_round(r, full, p.hover, a);
                        }
                        pt.hairline(r, full, p.border, a);
                        p.secondary
                    };
                    pt.text(label, Text::Mini, Align::Center, r, ink, a);
                    if ring_on(item) {
                        focus_ring(pt, r, full);
                    }
                }
                if let (Some(label), Some(toggle)) = (l.all_day_label, l.toggle) {
                    let all_day = tr(Key::AllDayToggle);
                    pt.text(
                        all_day,
                        Text::Small,
                        Align::Right,
                        label,
                        p.secondary,
                        enabled_a,
                    );
                    pt.toggle(toggle, ev.all_day, hovered(Item::AllDay), enabled_a);
                    if ring_on(Item::AllDay) {
                        focus_ring(pt, toggle, toggle.h / 2.0);
                    }
                }
                if !ev.all_day {
                    let bad = ev.times_invalid();
                    chip(Item::StartChip, &labels.start, false, false);
                    chip(Item::EndChip, &labels.end, false, bad);
                    if let Some(arrow) = l.arrow {
                        pt.text("→", Text::Small, Align::Center, arrow, p.secondary, 1.0);
                    }
                    if let Some(d) = l.duration
                        && !bad
                    {
                        let length = i18n::duration_label(lang, ev.length());
                        pt.text(&length, Text::Small, Align::Left, d, p.secondary, 1.0);
                    }
                }
            }
            Mode::Reminder {
                date,
                time,
                repeat,
                days,
                fresh,
                ..
            } => {
                chip(Item::DateChip, &labels.date, true, false);
                chip(Item::TimeChip, &labels.time, true, false);
                // `.quick`: the pill whose time is the one set reads as on.
                let now = chrono::Local::now().naive_local();
                for (q, label) in QuickTime::ALL.into_iter().zip(labels.quick) {
                    let item = Item::Quick(q);
                    let Some(r) = l.item(item) else {
                        continue;
                    };
                    let full = r.h / 2.0;
                    let ink = if q.target(now) == (*date, *time) {
                        pt.fill_rgba(r, full, p.selection, 1.0);
                        p.accent
                    } else {
                        if hovered(item) {
                            pt.fill_round(r, full, p.hover, 1.0);
                        }
                        pt.hairline(r, full, p.border, 1.0);
                        p.secondary
                    };
                    pt.text(label, Text::Mini, Align::Center, r, ink, 1.0);
                    if ring_on(item) {
                        focus_ring(pt, r, full);
                    }
                }
                // `.days`: accent circles for the days on, rings for the rest.
                for day in WEEK {
                    let item = Item::Day(day);
                    let Some(r) = l.item(item) else {
                        continue;
                    };
                    let radius = r.w / 2.0;
                    let on = days.contains(&day);
                    let letter = i18n::weekday_letter(lang, day);
                    if on {
                        let fill = if hovered(item) {
                            mix(p.accent, p.text, 0.1)
                        } else {
                            p.accent
                        };
                        pt.fill_round(r, radius, fill, 1.0);
                        pt.text(letter, Text::MiniBold, Align::Center, r, p.on_accent, 1.0);
                    } else {
                        if hovered(item) {
                            pt.fill_round(r, radius, p.hover, 1.0);
                        }
                        pt.hairline(r, radius, p.border, 1.0);
                        pt.text(letter, Text::Mini, Align::Center, r, p.secondary, 1.0);
                    }
                    if ring_on(item) {
                        focus_ring(pt, r, radius);
                    }
                }
                if let Some(track) = l.repeat_track {
                    // `.seg`: a field-filled track with a hairline.
                    pt.fill_round(track, radius::CONTROL, p.field, 1.0);
                    pt.hairline(track, radius::CONTROL, p.border, 1.0);
                    for (r, key) in [
                        (Repeat::Once, Key::RepeatOnce),
                        (Repeat::Daily, Key::RepeatDaily),
                        (Repeat::Weekly, Key::RepeatWeekly),
                    ] {
                        let item = Item::Repeat(r);
                        let Some(seg) = l.item(item) else {
                            continue;
                        };
                        let on = *repeat == r;
                        if on {
                            pt.fill_round(seg, radius::SEGMENT, p.accent, 1.0);
                        } else if hovered(item) {
                            pt.fill_round(seg, radius::SEGMENT, p.text, 0.07);
                        }
                        let (style, ink) = if on {
                            (Text::SmallStrong, p.on_accent)
                        } else {
                            (Text::Small, p.secondary)
                        };
                        pt.text(tr(key), style, Align::Center, seg, ink, 1.0);
                        if ring_on(item) {
                            focus_ring(pt, seg, radius::SEGMENT);
                        }
                    }
                }
                if let Some(line) = l.summary
                    && *fresh
                {
                    let hint = text(lang, Key::ReminderPickHint);
                    pt.text(hint, Text::Small, Align::Left, line, p.secondary, 1.0);
                } else if let Some(line) = l.summary {
                    let s = i18n::reminder_summary(lang, *repeat, *date, days, *time, now);
                    let (ink, strong) = if s.passed {
                        (p.error, p.error)
                    } else {
                        (p.secondary, p.text)
                    };
                    draw_runs(
                        pt,
                        line,
                        &[
                            (&s.lead, Text::Small, ink),
                            (&s.bold, Text::SmallStrong, strong),
                            (&s.tail, Text::Small, ink),
                        ],
                    );
                }
            }
            Mode::Note { .. } => {}
        }

        if let (Some(err), Some(key)) = (l.error, self.error) {
            // The reminder's error sits in its summary's place, at its size.
            let style = if self.mode.form() == Form::Reminder {
                Text::Small
            } else {
                Text::Mini
            };
            pt.text(tr(key), style, Align::Left, err, p.error, 1.0);
        }

        if let Some(b) = l.notes_box {
            let notes_edit = self.edit(Item::Notes);
            let focused =
                notes_edit.is_some() && unsafe { GetFocus() } == notes_edit.unwrap_or_default();
            pt.fill_round(b, 12.0, p.field, 1.0);
            if focused && !self.mode.read_only() {
                pt.stroke(b, 12.0, 1.0, p.accent, 1.0);
            } else {
                pt.hairline(b, 12.0, p.border, 1.0);
            }
        }

        // Footer.
        pt.rule(l.footer_rule, p.border);
        if let Some(dot) = l.dot {
            let sage = if p.light {
                Accent::Sage.light_rgb()
            } else {
                Accent::Sage.rgb()
            };
            pt.circle(
                dot.x + dot.w / 2.0,
                dot.y + dot.h / 2.0,
                dot.w / 2.0,
                sage,
                1.0,
            );
        }
        let footer = match (&self.status, &self.mode) {
            (Some((s, error)), _) => Some((s.clone(), *error)),
            (None, Mode::Event { calendar_id, .. }) => {
                Some((account_label(lang, calendar_id.as_deref()), false))
            }
            (None, Mode::Reminder { id: None, .. }) => {
                Some((text(lang, Key::SavedOnThisComputer).to_owned(), false))
            }
            _ => None,
        };
        if let Some((s, error)) = footer {
            let ink = if error { p.error } else { p.secondary };
            pt.text(&s, Text::Mini, Align::Left, l.status, ink, 1.0);
        }
        let can_save = self.mode.can_save(&self.title);
        for (item, r) in &l.items {
            let full = r.h / 2.0;
            let (label, primary) = match item {
                Item::Save => (tr(Key::Save), true),
                Item::Cancel if self.mode.read_only() => (tr(Key::Close), false),
                Item::Cancel => (tr(Key::Cancel), false),
                Item::Delete => (tr(Key::Delete), false),
                _ => continue,
            };
            let h = hovered(*item) && (!primary || can_save);
            let pressed = h && self.pressed == Some(*item);
            if primary {
                let a = if can_save { 1.0 } else { 0.4 };
                let fill = if pressed {
                    mix(p.accent, p.text, 0.2)
                } else if h {
                    mix(p.accent, p.text, 0.1)
                } else {
                    p.accent
                };
                pt.fill_round(*r, full, fill, a);
                pt.text(label, Text::ButtonStrong, Align::Center, *r, p.on_accent, a);
            } else if *item == Item::Delete && self.mode.form() == Form::Reminder {
                // `.btn.dng`: no fill until hovered, the text in the error colour.
                if pressed {
                    pt.fill_round(*r, full, p.button_hover, 1.0);
                } else if h {
                    pt.fill_round(*r, full, p.hover, 1.0);
                }
                let inner = Rect {
                    x: r.x + DANGER_PAD.0,
                    w: r.w - DANGER_PAD.0,
                    ..*r
                };
                pt.text(label, Text::Button, Align::Left, inner, p.error, 1.0);
            } else {
                let fill = if pressed {
                    mix(p.button_hover, p.text, 0.06)
                } else if h {
                    p.button_hover
                } else {
                    p.hover
                };
                pt.fill_round(*r, full, fill, 1.0);
                let ink = if *item == Item::Delete {
                    p.error
                } else {
                    p.text
                };
                pt.text(label, Text::Button, Align::Center, *r, ink, 1.0);
            }
            if ring_on(*item) {
                focus_ring(pt, *r, full);
            }
        }
    }
}

/// Runs of text one after another on one line in `r`, each in its own
/// style and colour (the reminder's summary), clipped at `r`'s right.
fn draw_runs(pt: &Painter, r: Rect, runs: &[(&str, Text, u32)]) {
    let mut x = r.x;
    for (s, style, ink) in runs {
        if s.is_empty() {
            continue;
        }
        let w = (r.right() - x).max(0.0);
        if w <= 0.0 {
            break;
        }
        pt.text(s, *style, Align::Left, Rect { x, w, ..r }, *ink, 1.0);
        x += pt.width(s, *style);
    }
}

/// `a` blended toward `b` by `t` (`0xRRGGBB`).
fn mix(a: u32, b: u32, t: f32) -> u32 {
    super::settings_controls::mix(a, b, t)
}

fn outset(r: Rect, by: f32) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

/// The keyboard focus on a drawn item: a 1.5 px accent ring 2 px outside.
fn focus_ring(pt: &Painter, r: Rect, radius: f32) {
    pt.stroke(outset(r, 2.5), radius + 2.5, 1.5, pt.p.accent, 1.0);
}

/// Lays the editor out again and moves the window size and EDITs to match.
/// Call with no editor borrow held: resizing sends `WM_SIZE`.
fn refresh(hwnd: HWND) {
    let Some(plan) = with_editor(hwnd, |ed| {
        ed.relayout();
        ed.placement()
    }) else {
        return;
    };
    unsafe {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        if (rc.right - rc.left, rc.bottom - rc.top) != plan.size {
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                plan.size.0,
                plan.size.1,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        for (edit, rc) in plan.edits {
            let _ = MoveWindow(
                edit,
                rc.left,
                rc.top,
                (rc.right - rc.left).max(0),
                (rc.bottom - rc.top).max(0),
                true,
            );
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// Posts the editor's final result (built by `result` while the controls
/// still exist), hands the foreground back if the editor still has it, and
/// destroys the window.
fn finish(hwnd: HWND, result: impl FnOnce(&Editor) -> EditorResult) {
    if let Some((owner, result)) = close(hwnd, result) {
        post_result(owner, result);
    }
}

/// Closes the open editor now and returns its final result instead of
/// posting it, for an owner that is about to be destroyed (a posted result
/// would never be read). `None` when no editor is open or it already finished.
pub fn close_open_editor() -> Option<EditorResult> {
    let hwnd = open_hwnd()?;
    close(hwnd, Editor::closing_result).map(|(_, result)| result)
}

/// The island is ending: the editor and its popover are closed (their last
/// result is not posted: nobody reads it any more, `shutdown` flushed it) and
/// what is left in the slots (the Direct2D objects) is dropped now, not in the
/// thread-local destructors during process exit.
pub fn release() {
    drop(close_open_editor());
    popover::release();
    let left = EDITOR.with(|cell| cell.try_borrow_mut().ok().and_then(|mut s| s.take()));
    drop(left);
}

/// `finish` without the post: the owner and final result (`None` if it was
/// already sent), with the window destroyed.
fn close(hwnd: HWND, result: impl FnOnce(&Editor) -> EditorResult) -> Option<(HWND, EditorResult)> {
    let mut editor = take_editor(hwnd)?;
    let last = (!editor.finished).then(|| {
        editor.finished = true;
        (editor.owner, result(&editor))
    });
    unsafe {
        // Only when closed from the keyboard or a button: a click elsewhere
        // already chose the next foreground window.
        // The editor or its popover is in front.
        let active = GetForegroundWindow();
        let ours = active == hwnd || GetWindow(active, GW_OWNER).ok() == Some(hwnd);
        if ours && !editor.previous.is_invalid() && IsWindow(Some(editor.previous)).as_bool() {
            let _ = SetForegroundWindow(editor.previous);
        }
        // Owned popovers go with the window.
        let _ = DestroyWindow(hwnd);
    }
    LAST_FOCUS.set(0);
    let gone: Vec<isize> = editor.edits.iter().map(|(_, h)| h.0 as isize).collect();
    PLACEHOLDERS.with(|p| p.borrow_mut().retain(|(h, _)| !gone.contains(h)));
    ON_BG.with(|b| b.borrow_mut().retain(|h| !gone.contains(h)));
    // Dropped after the controls are gone: deletes the fonts they used.
    drop(editor);
    last
}

/// What happens after a handler released the editor.
enum Next {
    Nothing,
    Refresh,
    Close,
    Finish(EditorResult),
    Popover(Item, RECT, Rc<Gfx>, Kind),
    Focus(HWND),
}

fn run(hwnd: HWND, next: Option<Next>) {
    match next {
        Some(Next::Refresh) => refresh(hwnd),
        Some(Next::Close) => finish(hwnd, Editor::closing_result),
        Some(Next::Finish(result)) => finish(hwnd, move |_| result),
        Some(Next::Popover(item, chip, gfx, kind)) => {
            match popover::open(hwnd, chip, gfx, item, kind) {
                Ok(_) => {
                    with_editor(hwnd, |ed| {
                        ed.popover = Some(item);
                        ed.invalidate();
                    });
                }
                Err(error) => eprintln!("vale: cannot open the popover: {error}"),
            }
        }
        Some(Next::Focus(target)) => unsafe {
            let _ = SetFocus(Some(target));
        },
        Some(Next::Nothing) | None => {}
    }
}

/// Activates `item` (a click, or Enter/Space on the focused item).
fn activate(hwnd: HWND, item: Item, keyboard: bool) {
    // The same click that closed this chip's popover must not reopen it.
    let closing_same = popover::current().is_some_and(|(open, _)| open == item);
    let next = with_editor(hwnd, |ed| {
        if !ed.mode.enabled(item) {
            return Next::Nothing;
        }
        match item {
            Item::Close | Item::Cancel => Next::Close,
            Item::Save => save_next(ed),
            Item::Delete => match &ed.mode {
                Mode::Note { id: Some(id) } => {
                    Next::Finish(EditorResult::NoteDeleted { id: id.clone() })
                }
                Mode::Reminder { id: Some(id), .. } => {
                    Next::Finish(EditorResult::ReminderDeleted { id: id.clone() })
                }
                _ => Next::Nothing,
            },
            Item::Title | Item::Notes => ed.edit(item).map_or(Next::Nothing, Next::Focus),
            Item::Pill(s) => {
                if let Mode::Event { ev, .. } = &mut ed.mode {
                    ev.date = s.date(ev.today);
                }
                ed.after_change();
                Next::Refresh
            }
            Item::AllDay => {
                if let Mode::Event { ev, .. } = &mut ed.mode {
                    ev.all_day = !ev.all_day;
                }
                ed.after_change();
                Next::Refresh
            }
            // The window grows or shrinks with the day row: `refresh` resizes
            // it once the editor is released.
            Item::Repeat(r) => {
                ed.mode.set_repeat(r);
                ed.after_change();
                Next::Refresh
            }
            Item::Quick(q) => {
                ed.mode.apply_quick(q, chrono::Local::now().naive_local());
                ed.after_change();
                Next::Refresh
            }
            Item::Day(day) => {
                if ed.mode.toggle_day(day) {
                    ed.after_change();
                }
                Next::Nothing
            }
            Item::DateChip | Item::StartChip | Item::EndChip | Item::TimeChip => {
                let recently = ed
                    .dismissed
                    .is_some_and(|(i, at)| i == item && at.elapsed() < REOPEN_GUARD);
                if closing_same || (!keyboard && recently) {
                    return Next::Nothing;
                }
                ed.focus = Some(item);
                match (ed.popover_kind(item), ed.screen_rect(item)) {
                    (Some(kind), Some(chip)) => Next::Popover(item, chip, Rc::clone(&ed.gfx), kind),
                    _ => Next::Nothing,
                }
            }
        }
    });
    if closing_same {
        popover::close();
    }
    run(hwnd, next);
}

/// Simpan, from a button or the keyboard.
fn save_next(ed: &mut Editor) -> Next {
    match ed.mode.form() {
        Form::Note => Next::Close,
        // Simpan is off without text: nothing happens, as with the button.
        Form::Reminder if !ed.mode.can_save(&ed.title) => Next::Nothing,
        Form::Reminder => match ed.save_reminder() {
            Some(result) => Next::Finish(result),
            None => Next::Refresh,
        },
        Form::Event => {
            ed.save_event();
            Next::Nothing
        }
    }
}

fn on_act(hwnd: HWND, act: Act) {
    let next = with_editor(hwnd, |ed| match act {
        Act::Close => Next::Close,
        Act::Save => save_next(ed),
        Act::SaveIfReady if ed.mode.can_save(&ed.title) => save_next(ed),
        Act::SaveIfReady => Next::Nothing,
    });
    run(hwnd, next);
}

/// Moves the keyboard to the next (or previous) Tab stop.
fn tab(hwnd: HWND, back: bool) {
    let target = with_editor(hwnd, |ed| {
        let stops = ed.tab_stops();
        if stops.is_empty() {
            return None;
        }
        let n = stops.len();
        let at = ed.focus.and_then(|f| stops.iter().position(|s| *s == f));
        let i = match (at, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        let item = stops[i];
        ed.focus = Some(item);
        ed.focus_visible = true;
        ed.invalidate();
        Some(ed.edit(item).unwrap_or(ed.hwnd))
    })
    .flatten();
    if let Some(target) = target {
        unsafe {
            let _ = SetFocus(Some(target));
        }
        if target == hwnd {
            LAST_FOCUS.set(hwnd.0 as isize);
        } else {
            send(target, EM_SETSEL, 0, -1);
        }
    }
}

/// Keys while the editor itself (a drawn item) has the keyboard.
fn on_key(hwnd: HWND, key: u16) -> bool {
    let ctrl = key_down(VK_CONTROL.0);
    if key == VK_TAB.0 {
        tab(hwnd, key_down(VK_SHIFT.0));
        return true;
    }
    if key == VK_ESCAPE.0 {
        on_act(hwnd, Act::Close);
        return true;
    }
    if key == VK_RETURN.0 && ctrl {
        on_act(hwnd, Act::Save);
        return true;
    }
    if key == VK_RETURN.0 || key == VK_SPACE.0 {
        if let Some(Some(item)) = with_editor(hwnd, |ed| ed.focus) {
            activate(hwnd, item, true);
        }
        return true;
    }
    false
}

fn on_pick(hwnd: HWND, lparam: LPARAM) {
    let ptr = lparam.0 as *mut (Item, Pick);
    if ptr.is_null() {
        return;
    }
    let (item, pick) = *unsafe { Box::from_raw(ptr) };
    with_editor(hwnd, |ed| ed.apply_pick(item, pick));
    refresh(hwnd);
}

fn on_popover_closed(hwnd: HWND, flags: usize) {
    let back = with_editor(hwnd, |ed| {
        let item = ed.popover.take();
        if let Some(item) = item {
            ed.dismissed = Some((item, Instant::now()));
            if flags & CLOSED_REFOCUS != 0 {
                ed.focus = Some(item);
                ed.focus_visible = flags & CLOSED_KEYBOARD != 0;
            }
        }
        ed.invalidate();
        flags & CLOSED_REFOCUS != 0
    });
    if back == Some(true) {
        unsafe {
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
        }
        LAST_FOCUS.set(hwnd.0 as isize);
    }
    if flags & CLOSED_SAVE != 0 {
        on_act(hwnd, Act::Save);
    }
}

fn on_command(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let id = (wparam.0 & 0xFFFF) as i32;
    let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
    let item = match id {
        ID_TITLE => Item::Title,
        ID_NOTES => Item::Notes,
        _ => return,
    };
    let edit = HWND(lparam.0 as *mut c_void);
    if code == EN_CHANGE {
        // The placeholder is painted from the EDIT's own WM_PAINT, and the
        // parent's WS_CLIPCHILDREN keeps `ed.invalidate()` from reaching it:
        // emptied text must repaint the EDIT itself to bring it back.
        unsafe {
            let _ = InvalidateRect(Some(edit), None, true);
        }
    }
    match code {
        EN_CHANGE if item == Item::Title => {
            let value = window_text(edit);
            let next = with_editor(hwnd, |ed| {
                ed.title = value;
                if !matches!(ed.mode, Mode::Event { saving: true, .. }) {
                    ed.status = None;
                }
                ed.invalidate();
                // A reminder's "Teks wajib diisi" goes once there is text.
                if ed.mode.form() == Form::Reminder && ed.error.take().is_some() {
                    Next::Refresh
                } else {
                    Next::Nothing
                }
            });
            run(hwnd, next);
        }
        EN_SETFOCUS => {
            LAST_FOCUS.set(edit.0 as isize);
            with_editor(hwnd, |ed| {
                ed.focus = Some(item);
                ed.invalidate();
            });
        }
        EN_KILLFOCUS => {
            with_editor(hwnd, |ed| ed.invalidate());
        }
        _ => {}
    }
}

fn on_status(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    // The editor owns the message whatever happens next.
    let ptr = lparam.0 as *mut String;
    let message = (!ptr.is_null()).then(|| *unsafe { Box::from_raw(ptr) });
    let saved = wparam.0 == 1;
    let waiting = with_editor(hwnd, |ed| {
        let Mode::Event { saving, .. } = &mut ed.mode else {
            return false;
        };
        if !*saving {
            return false;
        }
        if !saved {
            *saving = false;
            let text = message.as_deref().unwrap_or(tr(Key::SaveFailed));
            ed.status = Some((text.to_owned(), true));
            ed.invalidate();
        }
        true
    });
    if waiting == Some(true) && saved {
        finish(hwnd, |_| EditorResult::Closed);
    }
}

fn on_activate(hwnd: HWND, state: u32) {
    if state == WA_INACTIVE {
        let close = with_editor(hwnd, |ed| {
            ed.invalidate();
            ed.is_note() && ed.was_active && !ed.finished
        });
        if close == Some(true) {
            // Never destroy inside WM_ACTIVATE.
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_APP_EDITOR_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    } else {
        with_editor(hwnd, |ed| {
            ed.was_active = true;
            ed.invalidate();
        });
        // A popover that is open keeps the keyboard.
        if popover::current().is_none() {
            focus_last(hwnd);
        }
    }
}

fn on_dpi_changed(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    popover::close();
    let dpi = (wparam.0 & 0xFFFF) as f32;
    let scale = if dpi > 0.0 { dpi / 96.0 } else { 1.0 };
    let old = with_editor(hwnd, |ed| {
        ed.scale = scale;
        ed.target = None;
        let fonts = EditFonts::new(ed.mode.form(), scale);
        for (item, edit) in &ed.edits {
            send(*edit, WM_SETFONT, fonts.for_item(*item).0 as usize, 1);
        }
        // The old fonts are deleted only once no control uses them.
        std::mem::replace(&mut ed.fonts, fonts)
    });
    drop(old);
    let suggested = lparam.0 as *const RECT;
    if let Some(rc) = unsafe { suggested.as_ref() } {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                rc.left,
                rc.top,
                rc.right - rc.left,
                rc.bottom - rc.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
    // The size follows the layout, not the suggestion.
    refresh(hwnd);
}

/// The window went away without `finish` (e.g. its owner was destroyed):
/// still post the final result once.
fn on_destroy(hwnd: HWND) {
    with_editor(hwnd, |ed| {
        if !ed.finished {
            ed.finished = true;
            post_result(ed.owner, ed.closing_result());
        }
    });
}

fn on_mouse_move(hwnd: HWND, lparam: LPARAM) {
    let arm = with_editor(hwnd, |ed| {
        let hit = ed.hit_at(lparam).filter(|i| !i.is_edit());
        if hit != ed.hover {
            ed.hover = hit;
            ed.invalidate();
        }
        !std::mem::replace(&mut ed.tracking, true)
    });
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

fn on_button_down(hwnd: HWND, lparam: LPARAM) {
    let hit = with_editor(hwnd, |ed| {
        let hit = ed.hit_at(lparam);
        ed.pressed = hit;
        ed.invalidate();
        // A click on a drawn item (or nowhere) takes the keyboard from the
        // fields; a click on a field's padding gives it to that field.
        match hit {
            Some(item) if item.is_edit() => (hit, ed.edit(item)),
            Some(item) => {
                if ed.mode.enabled(item) && item != Item::Close {
                    ed.focus = Some(item);
                    ed.focus_visible = false;
                }
                (hit, Some(ed.hwnd))
            }
            None => (hit, Some(ed.hwnd)),
        }
    });
    let Some((hit, target)) = hit else {
        return;
    };
    unsafe {
        if let Some(target) = target {
            let _ = SetFocus(Some(target));
            if target == hwnd {
                LAST_FOCUS.set(hwnd.0 as isize);
            }
        }
        if hit.is_some_and(|i| !i.is_edit()) {
            SetCapture(hwnd);
        }
    }
}

fn on_button_up(hwnd: HWND, lparam: LPARAM) {
    let clicked = with_editor(hwnd, |ed| {
        let hit = ed.hit_at(lparam);
        let pressed = ed.pressed.take();
        ed.invalidate();
        hit.filter(|h| Some(*h) == pressed && !h.is_edit())
    })
    .flatten();
    unsafe {
        let _ = ReleaseCapture();
    }
    if let Some(item) = clicked {
        activate(hwnd, item, false);
    }
}

fn on_hit_test(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hit = unsafe { DefWindowProcW(hwnd, WM_NCHITTEST, wparam, lparam) };
    if hit.0 != HTCLIENT as isize {
        return hit;
    }
    let mut pt = POINT {
        x: (lparam.0 & 0xFFFF) as i16 as i32,
        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
    }
    let caption = with_editor(hwnd, |ed| {
        ed.layout
            .is_caption(pt.x as f32 / ed.scale, pt.y as f32 / ed.scale)
    });
    if caption == Some(true) {
        LRESULT(HTCAPTION as isize)
    } else {
        hit
    }
}

fn color_edit(wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let hdc = HDC(wparam.0 as *mut c_void);
    let control = lparam.0;
    let p = theme::current_window();
    let (window, field) = brushes(&p);
    let on_bg = ON_BG.with(|b| b.borrow().contains(&control));
    unsafe {
        SetTextColor(hdc, colorref(p.text));
        SetBkColor(hdc, colorref(if on_bg { p.bg } else { p.field }));
    }
    LRESULT(if on_bg { window.0 } else { field.0 } as isize)
}

/// `over` at `alpha` on `base` (`0xRRGGBB`).
pub(super) fn blend(base: u32, over: u32, alpha: f32) -> u32 {
    let ch = |shift: u32| {
        let (b, o) = (
            ((base >> shift) & 0xFF) as f32,
            ((over >> shift) & 0xFF) as f32,
        );
        ((b + (o - b) * alpha).round() as u32) << shift
    };
    ch(16) | ch(8) | ch(0)
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
            with_editor(hwnd, Editor::render);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        // Direct2D paints every pixel.
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            let (w, h) = (
                (lparam.0 & 0xFFFF) as u32,
                ((lparam.0 >> 16) & 0xFFFF) as u32,
            );
            with_editor(hwnd, |ed| {
                if let Some(t) = &ed.target {
                    t.resize(w, h);
                }
                ed.invalidate();
            });
            LRESULT(0)
        }
        WM_NCHITTEST => on_hit_test(hwnd, wparam, lparam),
        // No maximizing from the drawn caption.
        WM_NCLBUTTONDBLCLK if wparam.0 == HTCAPTION as usize => LRESULT(0),
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => color_edit(wparam, lparam),
        WM_COMMAND => {
            on_command(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            on_mouse_move(hwnd, lparam);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_editor(hwnd, |ed| {
                ed.tracking = false;
                if ed.hover.take().is_some() {
                    ed.invalidate();
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            on_button_down(hwnd, lparam);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_button_up(hwnd, lparam);
            LRESULT(0)
        }
        WM_KEYDOWN if on_key(hwnd, (wparam.0 & 0xFFFF) as u16) => LRESULT(0),
        // Not recorded in LAST_FOCUS: activation focuses the frame itself
        // before `focus_last` hands the keyboard to the remembered control.
        WM_SETFOCUS | WM_KILLFOCUS => {
            with_editor(hwnd, |ed| ed.invalidate());
            LRESULT(0)
        }
        WM_ACTIVATE => {
            let result = unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            on_activate(hwnd, (wparam.0 & 0xFFFF) as u32);
            result
        }
        WM_CLOSE | WM_APP_EDITOR_CLOSE => {
            finish(hwnd, Editor::closing_result);
            LRESULT(0)
        }
        WM_APP_EDITOR_ACT => {
            if let Some(act) = Act::from_wparam(wparam.0) {
                on_act(hwnd, act);
            }
            LRESULT(0)
        }
        WM_APP_EDITOR_STATUS => {
            on_status(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_APP_POPOVER_PICK => {
            on_pick(hwnd, lparam);
            LRESULT(0)
        }
        WM_APP_POPOVER_CLOSED => {
            on_popover_closed(hwnd, wparam.0);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            on_dpi_changed(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_DESTROY => {
            on_destroy(hwnd);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            // Frees the fonts once the controls are gone, if `finish` did not.
            drop(take_editor(hwnd));
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

const SUBCLASS_ID: usize = 1;

fn post_act(control: HWND, act: Act) {
    unsafe {
        if let Ok(parent) = GetParent(control) {
            let _ = PostMessageW(
                Some(parent),
                WM_APP_EDITOR_ACT,
                WPARAM(act as usize),
                LPARAM(0),
            );
        }
    }
}

pub(super) fn key_down(key: u16) -> bool {
    unsafe { GetKeyState(i32::from(key)) < 0 }
}

/// An empty EDIT's placeholder, in the tertiary text colour over what the
/// control just painted. The caret is hidden meanwhile so its blink cannot
/// invert the letters.
fn paint_placeholder(hwnd: HWND, key: Key, multi: bool) {
    if unsafe { GetWindowTextLengthW(hwnd) } > 0 {
        return;
    }
    let p = theme::current_window();
    let mut rc = RECT::default();
    send(hwnd, EM_GETRECT, 0, &mut rc as *mut RECT as isize);
    let font = send(hwnd, WM_GETFONT, 0, 0);
    let mut label: Vec<u16> = tr(key).encode_utf16().collect();
    unsafe {
        let _ = HideCaret(Some(hwnd));
        let hdc = GetDC(Some(hwnd));
        let old = (font != 0).then(|| SelectObject(hdc, HGDIOBJ(font as *mut c_void)));
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, colorref(p.tertiary));
        let flags = if multi {
            DT_WORDBREAK | DT_NOPREFIX
        } else {
            DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS
        };
        DrawTextW(hdc, &mut label, &mut rc, flags);
        if let Some(old) = old {
            SelectObject(hdc, old);
        }
        ReleaseDC(Some(hwnd), hdc);
        let _ = ShowCaret(Some(hwnd));
    }
}

/// Keyboard and placeholder for the EDITs: Esc closes, Ctrl+Enter (or Enter
/// in the title) saves, Tab / Shift+Tab move through the editor's stops,
/// Ctrl+A selects all.
unsafe extern "system" fn control_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let multi = data == Role::Multi as usize;
    match msg {
        WM_PAINT => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            let key = PLACEHOLDERS.with(|p| {
                p.try_borrow()
                    .ok()?
                    .iter()
                    .find(|(h, _)| *h == hwnd.0 as isize)
                    .map(|(_, k)| *k)
            });
            if let Some(key) = key {
                paint_placeholder(hwnd, key, multi);
            }
            return result;
        }
        WM_KEYDOWN => {
            let key = (wparam.0 & 0xFFFF) as u16;
            let ctrl = key_down(VK_CONTROL.0);
            if key == VK_ESCAPE.0 {
                post_act(hwnd, Act::Close);
                return LRESULT(0);
            }
            if key == VK_RETURN.0 && (ctrl || !multi) {
                post_act(hwnd, if ctrl { Act::Save } else { Act::SaveIfReady });
                return LRESULT(0);
            }
            if key == VK_TAB.0 && !ctrl {
                if let Ok(parent) = unsafe { GetParent(hwnd) } {
                    tab(parent, key_down(VK_SHIFT.0));
                }
                return LRESULT(0);
            }
            if key == VK_A.0 && ctrl {
                send(hwnd, EM_SETSEL, 0, -1);
                return LRESULT(0);
            }
        }
        WM_CHAR => {
            // Swallow the characters of the keys handled above, so fields
            // neither beep nor insert them.
            let swallowed = match wparam.0 {
                0x01 | 0x09 | 0x0A | 0x1B => true,
                0x0D => !multi,
                _ => false,
            };
            if swallowed {
                return LRESULT(0);
            }
        }
        WM_SETFOCUS => LAST_FOCUS.set(hwnd.0 as isize),
        WM_NCDESTROY => unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(control_proc), SUBCLASS_ID);
        },
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Opens the open editor's popover for a chip (demo screenshots).
#[cfg(debug_assertions)]
fn demo_popover(which: &str) {
    let Some(hwnd) = open_hwnd() else {
        return;
    };
    let item = match which {
        "calendar" | "date" => Item::DateChip,
        "start" => Item::StartChip,
        "time" | "end" => {
            let reminder = with_editor(hwnd, |ed| ed.mode.form() == Form::Reminder);
            if reminder == Some(true) {
                Item::TimeChip
            } else {
                Item::EndChip
            }
        }
        other => {
            eprintln!("demo-editor: unknown DI_POPOVER `{other}` (calendar|time|start)");
            return;
        }
    };
    activate(hwnd, item, true);
}

/// `--demo-editor note|note-new|event|event-new|event-ro|event-allday|event-bad|reminder|reminder-new|reminder-weekly|reminder-past`
/// (debug builds only): opens one editor under where the panel would be,
/// with a hidden owner that prints each result. `EventSave` gets a failure
/// after 1.5 s, then success on the next try. Ends when the editor closes.
///
/// `DI_WINDOW=system|dark|light` picks the window mode (default: system),
/// `DI_SURFACE` (`terminal` gives the neutral dark window; unset or `auto` is
/// the default Espresso) and `DI_ACCENT` pick the island theme,
/// `DI_WINDOW_FLIP=ms` swaps dark and light after that long, to check the
/// live restyle, and `DI_POPOVER=calendar|time|start` opens that popover
/// half a second after the editor (for screenshots).
#[cfg(debug_assertions)]
pub fn run_demo(which: Option<&str>) {
    use windows::Win32::Graphics::Gdi::MONITOR_DEFAULTTOPRIMARY;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, KillTimer, MSG, PostQuitMessage, SetTimer, TranslateMessage,
        WM_TIMER,
    };

    thread_local! {
        static DEMO_EDITOR: Cell<isize> = const { Cell::new(0) };
        static SAVES: Cell<u32> = const { Cell::new(0) };
    }

    const SAVE_TIMER: usize = 1;
    const FLIP_TIMER: usize = 2;
    const POPOVER_TIMER: usize = 3;

    unsafe extern "system" fn owner_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_APP_EDITOR_RESULT => {
                if let Some(result) = unsafe { take_result(lparam) } {
                    println!("demo-editor: {result:?}");
                    if matches!(result, EditorResult::EventSave { .. }) {
                        unsafe { SetTimer(Some(hwnd), SAVE_TIMER, 1500, None) };
                    } else if result.closes_editor() {
                        unsafe { PostQuitMessage(0) };
                    }
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == POPOVER_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(hwnd), POPOVER_TIMER);
                }
                if let Ok(which) = std::env::var("DI_POPOVER") {
                    demo_popover(&which);
                }
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == FLIP_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(hwnd), FLIP_TIMER);
                }
                let mut t = theme::current();
                t.window = if theme::current_window().light {
                    WindowTheme::Dark
                } else {
                    WindowTheme::Light
                };
                theme::set_current(t);
                println!("demo-editor: window mode now {}", t.window.name());
                restyle_open_editor();
                LRESULT(0)
            }
            WM_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(hwnd), SAVE_TIMER);
                }
                let editor = HWND(DEMO_EDITOR.get() as *mut c_void);
                let n = SAVES.get();
                SAVES.set(n + 1);
                let (ok, message) = if n == 0 {
                    let reason = tr(Key::ReasonOffline);
                    let text = crate::i18n::save_failed_because(crate::i18n::lang(), reason);
                    let text = format!("{text} (demo)");
                    (0, Box::into_raw(Box::new(text)) as isize)
                } else {
                    (1, 0)
                };
                let posted = unsafe {
                    PostMessageW(
                        Some(editor),
                        WM_APP_EDITOR_STATUS,
                        WPARAM(ok),
                        LPARAM(message),
                    )
                };
                if posted.is_err() && message != 0 {
                    drop(unsafe { Box::from_raw(message as *mut String) });
                }
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    use crate::theme::WindowTheme;
    theme::set_system_light(theme::read_system_light());
    theme::set_current(theme::theme_from_env());

    let date = chrono::Local::now().date_naive();
    let hm = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap_or(NaiveTime::MIN);
    let draft = |title: &str, all_day, start, end| EventDraft {
        title: title.into(),
        date: date + chrono::Days::new(2),
        all_day,
        start,
        end,
        description: "Bawa maket skala 1:100".into(),
    };
    let event =
        |event_id: Option<&str>, calendar: &str, draft: EventDraft, read_only| EditorKind::Event {
            event_id: event_id.map(Into::into),
            calendar_id: Some(calendar.into()),
            draft,
            read_only,
        };
    let account = "kamu@example.com";
    let review = "Review desain klien";
    let kind = match which.unwrap_or("note") {
        "note" => EditorKind::Note {
            id: Some("demo".into()),
            text: "Beli kopi\nTelepon ibu jam 7 malam\nCek tiket kereta".into(),
        },
        "note-new" => EditorKind::Note {
            id: None,
            text: String::new(),
        },
        "event" => event(
            Some("demo-event"),
            account,
            draft(review, false, hm(10, 0), hm(11, 30)),
            false,
        ),
        "event-new" => EditorKind::Event {
            event_id: None,
            calendar_id: None,
            draft: EventDraft {
                title: String::new(),
                date,
                description: String::new(),
                ..draft("", false, hm(9, 0), hm(10, 0))
            },
            read_only: false,
        },
        "event-ro" => event(
            Some("demo-event"),
            "id.indonesian#holiday@group.v.calendar.google.com",
            draft("Libur nasional", true, hm(0, 0), hm(0, 0)),
            true,
        ),
        "event-allday" => event(
            Some("demo-event"),
            account,
            draft("Workshop arsitektur", true, hm(9, 0), hm(10, 0)),
            false,
        ),
        "event-bad" => event(
            Some("demo-event"),
            account,
            draft(review, false, hm(10, 0), hm(9, 30)),
            false,
        ),
        "reminder" => EditorKind::Reminder {
            id: Some("demo-reminder".into()),
            text: "Minum vitamin".into(),
            date,
            time: hm(8, 0),
            repeat: Repeat::Daily,
            days: Vec::new(),
        },
        "reminder-new" => {
            // Like the panel's "+ Baru": the next full hour.
            let now = chrono::Local::now().naive_local();
            let next = now.date().and_time(hm(now.hour(), 0)) + chrono::Duration::hours(1);
            EditorKind::Reminder {
                id: None,
                text: String::new(),
                date: next.date(),
                time: next.time(),
                repeat: Repeat::Once,
                days: Vec::new(),
            }
        }
        // The design's state 3: Mon, Wed, Fri at 09:00 from this week's Monday.
        "reminder-weekly" => EditorKind::Reminder {
            id: Some("demo-reminder".into()),
            text: "Backup file proyek".into(),
            date: date - chrono::Days::new(u64::from(date.weekday().num_days_from_monday())),
            time: hm(9, 0),
            repeat: Repeat::Weekly,
            days: vec![Weekday::Mon, Weekday::Wed, Weekday::Fri],
        },
        "reminder-past" => EditorKind::Reminder {
            id: Some("demo-reminder".into()),
            text: "Kirim revisi denah ke klien".into(),
            date: date - chrono::Days::new(1),
            time: hm(16, 30),
            repeat: Repeat::Once,
            days: Vec::new(),
        },
        other => {
            eprintln!(
                "demo-editor: unknown kind `{other}` (note|note-new|event|event-new|event-ro|\
                 event-allday|event-bad|reminder|reminder-new|reminder-weekly|reminder-past)"
            );
            return;
        }
    };

    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let instance: HINSTANCE = module.into();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(owner_proc),
            hInstance: instance,
            lpszClassName: w!("ValeEditorDemoOwner"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        // Hidden, like the island is to the editor: just something to own it.
        let Ok(owner) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("ValeEditorDemoOwner"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        ) else {
            eprintln!("demo-editor: cannot create the owner window");
            return;
        };
        // Bottom center of where the open panel sits on the primary monitor.
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (scale, _) = monitor_at(POINT {
            x: info.rcMonitor.left,
            y: info.rcMonitor.top,
        });
        let anchor = POINT {
            x: (info.rcMonitor.left + info.rcMonitor.right) / 2,
            y: info.rcMonitor.top + (super::geometry::WINDOW_H * scale).round() as i32,
        };
        match open_editor(owner, anchor, kind) {
            Ok(editor) => {
                DEMO_EDITOR.set(editor.0 as isize);
                println!("demo-editor: hwnd {:#x}", editor.0 as usize);
                let flip = std::env::var("DI_WINDOW_FLIP").ok();
                if let Some(ms) = flip.and_then(|v| v.parse::<u32>().ok()) {
                    SetTimer(Some(owner), FLIP_TIMER, ms, None);
                }
                if std::env::var("DI_POPOVER").is_ok() {
                    SetTimer(Some(owner), POPOVER_TIMER, 500, None);
                }
            }
            Err(error) => {
                eprintln!("demo-editor: open failed: {error}");
                return;
            }
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = DestroyWindow(owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_slots_are_keyed_by_the_colour_pair_not_the_mode() {
        let warm = theme::window(Accent::Terracotta, theme::WindowTheme::Dark, false);
        let neutral = theme::window_for(
            Accent::Terracotta,
            theme::Surface::Terminal,
            theme::WindowTheme::Dark,
            false,
        );
        let light = theme::window(Accent::Terracotta, theme::WindowTheme::Light, false);
        let key = |p: &WindowPalette| (p.bg, p.field);
        // Same `light` flag, different colours: not the same slot.
        assert!(!warm.light && !neutral.light);
        assert_ne!(key(&warm), key(&neutral));
        let mut keys: [Option<(u32, u32)>; BRUSH_SLOTS] = [None; BRUSH_SLOTS];
        for (i, p) in [&warm, &neutral, &light].into_iter().enumerate() {
            let slot = brush_slot(&keys, key(p)).expect("a free slot");
            assert_eq!(slot, i);
            keys[slot] = Some(key(p));
        }
        // Each palette finds its own slot again, and the table stays full.
        for (i, p) in [&warm, &neutral, &light].into_iter().enumerate() {
            assert_eq!(brush_slot(&keys, key(p)), Some(i));
        }
        assert_eq!(brush_slot(&keys, (1, 2)), None);
        // Every accent shares the pair, so a palette never needs a fourth slot.
        for a in Accent::ALL {
            let n = theme::window_for(
                *a,
                theme::Surface::Terminal,
                theme::WindowTheme::Dark,
                false,
            );
            assert_eq!(key(&n), key(&neutral));
        }
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn date(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
    }

    fn draft(title: &str, all_day: bool, start: (u32, u32), end: (u32, u32)) -> EventDraft {
        EventDraft {
            title: title.into(),
            date: date(26),
            all_day,
            start: t(start.0, start.1),
            end: t(end.0, end.1),
            description: String::new(),
        }
    }

    fn event_mode(read_only: bool, saving: bool, ev: EventState) -> Mode {
        Mode::Event {
            event_id: Some("e1".into()),
            calendar_id: Some("rani@example.com".into()),
            ev,
            read_only,
            saving,
        }
    }

    fn ev(all_day: bool, start: NaiveTime, end: NaiveTime) -> EventState {
        EventState {
            date: date(28),
            all_day,
            start,
            end,
            today: date(26),
        }
    }

    const SIZE_400: SIZE = SIZE { cx: 400, cy: 220 };

    /// Opens a real note editor (a real window, never shown under test) and
    /// closes it the way the island does on shutdown: the result comes back,
    /// nothing is posted.
    #[test]
    fn close_open_editor_returns_the_note_without_posting() {
        use windows::Win32::UI::WindowsAndMessaging::{
            IsWindowVisible, MSG, PM_REMOVE, PeekMessageW,
        };
        unsafe {
            let owner = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .expect("owner window");
            let kind = EditorKind::Note {
                id: Some("n1-0".into()),
                text: "baris 1\nbaris 2".into(),
            };
            let anchor = POINT { x: 400, y: 300 };
            let editor = open_editor(owner, anchor, kind.clone()).expect("editor");
            assert!(!IsWindowVisible(editor).as_bool(), "never on screen in tests");
            // One editor at a time: a second open returns the same window.
            assert_eq!(open_editor(owner, anchor, kind).ok(), Some(editor));
            assert_eq!(
                close_open_editor(),
                Some(EditorResult::NoteSaved {
                    id: Some("n1-0".into()),
                    text: "baris 1\nbaris 2".into(),
                })
            );
            assert!(!IsWindow(Some(editor)).as_bool());
            assert_eq!(close_open_editor(), None);
            let mut msg = MSG::default();
            let posted = PeekMessageW(
                &mut msg,
                Some(owner),
                WM_APP_EDITOR_RESULT,
                WM_APP_EDITOR_RESULT,
                PM_REMOVE,
            );
            assert!(!posted.as_bool(), "the result was returned, not posted");
            let _ = DestroyWindow(owner);
        }
    }

    /// The reminder editor opens in the new frame (no date control) and
    /// closes with `Closed`.
    #[test]
    fn reminder_editor_opens_without_windows_date_controls() {
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        unsafe {
            let owner = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .expect("owner window");
            let editor = open_editor(owner, POINT { x: 400, y: 300 }, reminder_kind(Some("r1")))
                .expect("editor");
            assert!(!IsWindowVisible(editor).as_bool(), "never on screen in tests");
            let kids = with_editor(editor, |ed| ed.edits.len()).expect("editor state");
            assert_eq!(kids, 1, "only the text is an EDIT");
            let size = with_editor(editor, |ed| ed.layout.size).unwrap();
            let mut rc = RECT::default();
            let _ = GetClientRect(editor, &mut rc);
            let scale = with_editor(editor, |ed| ed.scale).unwrap();
            assert_eq!(rc.right - rc.left, (size.0 * scale).round() as i32);
            assert_eq!(close_open_editor(), Some(EditorResult::Closed));
            let _ = DestroyWindow(owner);
        }
    }

    #[test]
    fn centered_below_anchor() {
        let work = rect(0, 0, 1920, 1040);
        assert_eq!(
            editor_rect(POINT { x: 960, y: 330 }, SIZE_400, work),
            rect(760, 338, 1160, 558)
        );
    }

    #[test]
    fn clamped_to_work_area() {
        let work = rect(0, 0, 1920, 1040);
        // Off the right and bottom edges.
        assert_eq!(
            editor_rect(POINT { x: 1900, y: 1000 }, SIZE_400, work),
            rect(1520, 820, 1920, 1040)
        );
        // Off the left edge of a monitor left of the primary one.
        let left_monitor = rect(-1280, 0, 0, 984);
        assert_eq!(
            editor_rect(POINT { x: -1250, y: 100 }, SIZE_400, left_monitor),
            rect(-1280, 108, -880, 328)
        );
        // Larger than the work area: pinned to its top-left corner.
        let tiny = rect(0, 0, 300, 200);
        assert_eq!(
            editor_rect(POINT { x: 150, y: 50 }, SIZE_400, tiny),
            rect(0, 0, 400, 220)
        );
    }

    #[test]
    fn empty_title_rejected() {
        assert_eq!(
            validate_event(&draft("", false, (9, 0), (10, 0))),
            Err(Key::TitleRequired)
        );
        assert_eq!(
            validate_event(&draft("  \t ", true, (9, 0), (10, 0))),
            Err(Key::TitleRequired)
        );
        assert_eq!(
            validate_event(&draft("Rapat", false, (9, 0), (10, 0))),
            Ok(())
        );
    }

    #[test]
    fn end_before_start_rejected() {
        let msg = Err(Key::EndBeforeStart);
        assert_eq!(
            validate_event(&draft("Rapat", false, (10, 0), (9, 30))),
            msg
        );
        assert_eq!(
            validate_event(&draft("Rapat", false, (10, 0), (10, 0))),
            msg
        );
        assert_eq!(
            validate_event(&draft("Rapat", false, (10, 0), (10, 1))),
            Ok(())
        );
    }

    #[test]
    fn all_day_ignores_times() {
        assert_eq!(
            validate_event(&draft("Libur", true, (10, 0), (9, 0))),
            Ok(())
        );
        assert_eq!(
            validate_event(&draft("Libur", true, (10, 0), (10, 0))),
            Ok(())
        );
        assert!(!ev(true, t(10, 0), t(9, 0)).times_invalid());
        assert!(ev(false, t(10, 0), t(9, 0)).times_invalid());
        assert!(ev(false, t(10, 0), t(10, 0)).times_invalid());
    }

    #[test]
    fn save_is_enabled_by_the_title_and_the_times() {
        let good = event_mode(false, false, ev(false, t(10, 0), t(11, 30)));
        assert!(good.can_save("Review"));
        assert!(!good.can_save(""));
        assert!(!good.can_save("   "));
        let bad = event_mode(false, false, ev(false, t(10, 0), t(9, 30)));
        assert!(!bad.can_save("Review"));
        // All day: the times do not matter.
        let all_day = event_mode(false, false, ev(true, t(10, 0), t(9, 30)));
        assert!(all_day.can_save("Libur"));
        assert!(!event_mode(true, false, ev(false, t(10, 0), t(11, 0))).can_save("Rapat"));
        assert!(!event_mode(false, true, ev(false, t(10, 0), t(11, 0))).can_save("Rapat"));
    }

    #[test]
    fn save_builds_the_draft_from_the_editor_state() {
        let mode = event_mode(false, false, ev(false, t(10, 0), t(11, 30)));
        let result = mode.event_save("  Review desain klien ", "Bawa maket\r\nskala 1:100");
        assert_eq!(
            result,
            Some(Ok(EditorResult::EventSave {
                event_id: Some("e1".into()),
                calendar_id: Some("rani@example.com".into()),
                draft: EventDraft {
                    title: "Review desain klien".into(),
                    date: date(28),
                    all_day: false,
                    start: t(10, 0),
                    end: t(11, 30),
                    description: "Bawa maket\nskala 1:100".into(),
                },
            }))
        );
        // Still checked on save.
        let bad = event_mode(false, false, ev(false, t(10, 0), t(10, 0)));
        assert_eq!(bad.event_save("Rapat", ""), Some(Err(Key::EndBeforeStart)));
        assert_eq!(
            event_mode(false, false, ev(false, t(10, 0), t(11, 0))).event_save("", ""),
            Some(Err(Key::TitleRequired))
        );
    }

    #[test]
    fn read_only_or_saving_events_post_no_save() {
        let ro = event_mode(true, false, ev(false, t(10, 0), t(11, 0)));
        assert_eq!(ro.event_save("Libur", ""), None);
        let saving = event_mode(false, true, ev(false, t(10, 0), t(11, 0)));
        assert_eq!(saving.event_save("Rapat", ""), None);
        assert_eq!(Mode::Note { id: None }.event_save("x", ""), None);
        // Read-only allows only the text and Tutup.
        assert!(!ro.enabled(Item::DateChip));
        assert!(!ro.enabled(Item::Pill(Shortcut::Today)));
        assert!(!ro.enabled(Item::AllDay));
        assert!(!ro.enabled(Item::Save));
        assert!(ro.enabled(Item::Cancel) && ro.enabled(Item::Notes));
        assert_eq!(title_key(&ro), Key::EditorEvent);
    }

    #[test]
    fn picking_a_start_moves_the_end_with_it() {
        let mut state = ev(false, t(10, 0), t(11, 30));
        state.end = shift_end(state.start, state.end, t(13, 0));
        state.start = t(13, 0);
        assert_eq!((state.start, state.end), (t(13, 0), t(14, 30)));
        assert_eq!(state.length(), 90);
    }

    #[test]
    fn the_footer_names_the_account_only_when_known() {
        assert_eq!(
            account_label(Lang::Id, Some("rani@studio.id")),
            "Google Calendar · rani@studio.id"
        );
        assert_eq!(account_label(Lang::En, Some("primary")), "Google Calendar");
        assert_eq!(account_label(Lang::En, None), "Google Calendar");
        assert_eq!(account_label(Lang::Id, Some("  ")), "Google Calendar");
        for shared in [
            "id.indonesian#holiday@group.v.calendar.google.com",
            "abc123@group.calendar.google.com",
            "xyz@import.calendar.google.com",
            "@example.com",
        ] {
            assert_eq!(
                account_label(Lang::Id, Some(shared)),
                "Google Calendar",
                "{shared}"
            );
        }
    }

    #[test]
    fn pills_name_today_tomorrow_the_weekday_and_next_week() {
        let mon = date(26);
        let labels: Vec<String> = Shortcut::ALL
            .iter()
            .map(|s| pill_label(Lang::Id, *s, mon))
            .collect();
        assert_eq!(labels, ["Hari ini", "Besok", "Rabu", "Minggu depan"]);
        let wed = date(28);
        assert_eq!(pill_label(Lang::Id, Shortcut::DayAfter, wed), "Jumat");
        assert_eq!(pill_label(Lang::En, Shortcut::DayAfter, wed), "Friday");
        assert_eq!(pill_label(Lang::En, Shortcut::NextWeek, wed), "Next week");
    }

    #[test]
    fn edit_text_line_breaks_round_trip() {
        assert_eq!(to_edit_text("a\nb\r\nc"), "a\r\nb\r\nc");
        assert_eq!(from_edit_text("a\r\nb\r\n"), "a\nb\n");
        let text = "Beli kopi\n\nTelepon ibu";
        assert_eq!(from_edit_text(&to_edit_text(text)), text);
    }

    #[test]
    fn only_event_save_keeps_the_editor() {
        let save = EditorResult::EventSave {
            event_id: None,
            calendar_id: None,
            draft: draft("Rapat", false, (9, 0), (10, 0)),
        };
        assert!(!save.closes_editor());
        assert!(EditorResult::Closed.closes_editor());
        assert!(EditorResult::NoteDeleted { id: "n".into() }.closes_editor());
        let saved = EditorResult::NoteSaved {
            id: None,
            text: String::new(),
        };
        assert!(saved.closes_editor());
        let lparam = LPARAM(Box::into_raw(Box::new(saved.clone())) as isize);
        assert_eq!(unsafe { take_result(lparam) }, Some(saved));
        assert_eq!(unsafe { take_result(LPARAM(0)) }, None);
    }

    #[test]
    fn colorref_swaps_red_and_blue() {
        assert_eq!(colorref(0x1B2B3C), COLORREF(0x3C2B1B));
        assert_eq!(blend(0xF3F3F5, 0x000000, 0.0), 0xF3F3F5);
        assert_eq!(blend(0xFFFFFF, 0x000000, 1.0), 0x000000);
    }

    fn reminder_kind(id: Option<&str>) -> EditorKind {
        EditorKind::Reminder {
            id: id.map(Into::into),
            text: "Minum obat".into(),
            date: NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
            time: NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            repeat: Repeat::Daily,
            days: Vec::new(),
        }
    }

    #[test]
    fn reminder_needs_text_and_a_future_time_unless_it_repeats() {
        let now = 1_000;
        assert_eq!(
            validate_reminder("  ", Some(2_000), Repeat::Once, now),
            Err(Key::TextRequired)
        );
        assert_eq!(
            validate_reminder("Minum obat", Some(2_000), Repeat::Once, now),
            Ok(())
        );
        for past in [Some(1_000), Some(10)] {
            assert_eq!(
                validate_reminder("Minum obat", past, Repeat::Once, now),
                Err(Key::TimePassed),
                "{past:?}"
            );
        }
        // A repeating one set in the past just starts at its next time.
        assert_eq!(
            validate_reminder("Minum obat", Some(10), Repeat::Daily, now),
            Ok(())
        );
        assert_eq!(
            validate_reminder("Minum obat", Some(10), Repeat::Weekly, now),
            Ok(())
        );
        assert_eq!(tr(Key::TimePassed), "Waktunya sudah lewat");
    }

    #[test]
    fn unresolvable_local_time_is_refused_for_every_repeat() {
        // The island would drop such a save after the editor closed.
        for repeat in [Repeat::Once, Repeat::Daily, Repeat::Weekly] {
            assert_eq!(
                validate_reminder("Minum obat", None, repeat, 1_000),
                Err(Key::SaveFailed),
                "{repeat:?}"
            );
        }
        // Empty text is still the first thing said.
        assert_eq!(
            validate_reminder(" ", None, Repeat::Daily, 1_000),
            Err(Key::TextRequired)
        );
    }

    fn reminder_mode(date: NaiveDate, time: NaiveTime, repeat: Repeat) -> Mode {
        Mode::Reminder {
            id: None,
            date,
            time,
            repeat,
            days: Vec::new(),
            fresh: true,
        }
    }

    fn reminder_days(mode: &Mode) -> Vec<Weekday> {
        match mode {
            Mode::Reminder { days, .. } => days.clone(),
            _ => panic!("not a reminder"),
        }
    }

    #[test]
    fn weekly_days_toggle_but_the_last_one_stays() {
        use Weekday::{Fri, Mon, Wed};
        // Monday 26 October.
        let mut mode = reminder_mode(date(26), t(9, 0), Repeat::Once);
        // Weekly with no day on starts with the date's weekday.
        mode.set_repeat(Repeat::Weekly);
        assert_eq!(reminder_days(&mode), [Mon]);
        assert!(matches!(mode, Mode::Reminder { fresh: false, .. }));
        // Several may be on, kept Monday first.
        assert!(mode.toggle_day(Fri));
        assert!(mode.toggle_day(Wed));
        assert_eq!(reminder_days(&mode), [Mon, Wed, Fri]);
        // Off again, down to one; that one stays on.
        assert!(mode.toggle_day(Mon));
        assert!(mode.toggle_day(Fri));
        assert_eq!(reminder_days(&mode), [Wed]);
        assert!(!mode.toggle_day(Wed), "the last day is refused");
        assert_eq!(reminder_days(&mode), [Wed]);
        // Back to once and weekly again: the days are kept.
        mode.set_repeat(Repeat::Once);
        mode.set_repeat(Repeat::Weekly);
        assert_eq!(reminder_days(&mode), [Wed]);
        assert!(!Mode::Note { id: None }.toggle_day(Mon));
    }

    #[test]
    fn a_single_weekly_day_follows_the_date() {
        use Weekday::{Fri, Mon, Thu};
        let mut mode = reminder_mode(date(26), t(9, 0), Repeat::Weekly);
        mode.set_repeat(Repeat::Weekly);
        // The old weekly: its day moves with the date (Thursday 29).
        mode.set_reminder_date(date(29));
        assert_eq!(reminder_days(&mode), [Thu]);
        // Days picked on purpose stay where they are.
        mode.toggle_day(Mon);
        mode.set_reminder_date(date(28));
        assert_eq!(reminder_days(&mode), [Mon, Thu]);
        let mut other = reminder_mode(date(26), t(9, 0), Repeat::Weekly);
        other.toggle_day(Fri);
        other.set_reminder_date(date(28));
        assert_eq!(reminder_days(&other), [Fri], "Friday was not Monday's");
    }

    #[test]
    fn quick_pill_fills_the_date_and_time_and_keeps_the_repeat() {
        let mut mode = reminder_mode(date(26), t(9, 0), Repeat::Daily);
        let now = date(26).and_hms_opt(21, 10, 0).unwrap();
        mode.apply_quick(QuickTime::Tonight, now);
        match &mode {
            Mode::Reminder {
                date: d,
                time,
                repeat,
                fresh,
                ..
            } => {
                assert_eq!((*d, *time, *repeat), (date(27), t(20, 0), Repeat::Daily));
                assert!(!fresh);
            }
            _ => unreachable!(),
        }
        mode.apply_quick(QuickTime::HalfHour, now);
        assert!(matches!(
            mode,
            Mode::Reminder { date: d, time, .. } if d == date(26) && time == t(21, 40)
        ));
    }

    #[test]
    fn reminder_saved_carries_its_days() {
        use Weekday::{Fri, Mon, Wed};
        let mut mode = Mode::Reminder {
            id: Some("r1".into()),
            date: date(26),
            time: t(9, 0),
            repeat: Repeat::Weekly,
            days: vec![Mon, Wed, Fri],
            fresh: false,
        };
        // Long before: a weekly one saves whatever its start.
        let saved = mode.reminder_save("  Backup file proyek ", 0);
        assert_eq!(
            saved,
            Some(Ok(EditorResult::ReminderSaved {
                id: Some("r1".into()),
                text: "Backup file proyek".into(),
                date: date(26),
                time: t(9, 0),
                repeat: Repeat::Weekly,
                days: vec![Mon, Wed, Fri],
            }))
        );
        // Not weekly: no days are sent.
        mode.set_repeat(Repeat::Daily);
        match mode.reminder_save("Backup", 0) {
            Some(Ok(EditorResult::ReminderSaved { days, repeat, .. })) => {
                assert_eq!(repeat, Repeat::Daily);
                assert!(days.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(mode.reminder_save(" ", 0), Some(Err(Key::TextRequired)));
        assert_eq!(Mode::Note { id: None }.reminder_save("x", 0), None);
    }

    #[test]
    fn quick_pills_read_in_both_languages() {
        let id = QuickTime::ALL.map(|q| quick_label(Lang::Id, q));
        assert_eq!(
            id,
            ["30 mnt lagi", "1 jam lagi", "Nanti malam", "Besok pagi"]
        );
        let en = QuickTime::ALL.map(|q| quick_label(Lang::En, q));
        assert_eq!(
            en,
            ["In 30 min", "In 1 hour", "Tonight", "Tomorrow morning"]
        );
    }

    #[test]
    fn reminder_save_needs_text() {
        let mode = reminder_mode(date(27), t(8, 0), Repeat::Once);
        assert!(!mode.can_save(""));
        assert!(!mode.can_save("  \t "));
        assert!(mode.can_save("Minum obat"));
        assert!(mode.can_save("  x "));
        // Notes always save; events still follow their own rules.
        assert!(Mode::Note { id: None }.can_save(""));
    }

    #[test]
    fn reminder_start_in_the_past_saves_unless_it_is_a_one_off() {
        let now = 1_000_000;
        let day = 86_400;
        // A daily or weekly reminder whose start lies days back saves: it
        // starts at its next time.
        for repeat in [Repeat::Daily, Repeat::Weekly] {
            assert_eq!(
                validate_reminder("Minum vitamin", Some(now - 3 * day), repeat, now),
                Ok(()),
                "{repeat:?}"
            );
        }
        // A one-off in the past is refused, even a second ago.
        assert_eq!(
            validate_reminder("Kirim revisi", Some(now - day), Repeat::Once, now),
            Err(Key::TimePassed)
        );
        assert_eq!(
            validate_reminder("Kirim revisi", Some(now), Repeat::Once, now),
            Err(Key::TimePassed)
        );
    }

    #[test]
    fn reminder_popovers_dim_the_past_and_name_times_of_day() {
        let today = date(26);
        let mode = reminder_mode(date(27), t(16, 30), Repeat::Weekly);
        assert_eq!(
            popover_kind(&mode, Item::DateChip, today),
            Some(Kind::Date {
                selected: date(27),
                today,
                dim_past: true,
            })
        );
        assert_eq!(
            popover_kind(&mode, Item::TimeChip, today),
            Some(Kind::Time {
                value: t(16, 30),
                start: None,
                periods: true,
            })
        );
        // The event's popovers are as they were.
        let event = event_mode(false, false, ev(false, t(10, 0), t(11, 0)));
        assert!(matches!(
            popover_kind(&event, Item::DateChip, today),
            Some(Kind::Date {
                dim_past: false,
                ..
            })
        ));
        assert!(matches!(
            popover_kind(&event, Item::StartChip, today),
            Some(Kind::Time {
                start: None,
                periods: false,
                ..
            })
        ));
        assert!(matches!(
            popover_kind(&event, Item::EndChip, today),
            Some(Kind::Time {
                periods: false,
                start: Some(_),
                ..
            })
        ));
        // The chips read as the design's.
        let labels = label_text(&mode, Lang::Id);
        assert!(labels.date.starts_with("Mulai "), "{}", labels.date);
        assert_eq!(labels.time, "16.30");
        let once = reminder_mode(date(27), t(16, 30), Repeat::Once);
        let en = label_text(&once, Lang::En);
        assert_eq!(en.time, "16:30");
        assert!(!en.date.starts_with("From"));
    }

    #[test]
    fn reminder_picks_land_on_its_chips() {
        let mode = Mode::Reminder {
            id: Some("r1".into()),
            date: date(27),
            time: t(8, 0),
            repeat: Repeat::Once,
            days: Vec::new(),
            fresh: false,
        };
        assert_eq!(mode.form(), Form::Reminder);
        assert_eq!(title_key(&mode), Key::EditorEditReminder);
        assert!(mode.enabled(Item::Delete));
    }
}
