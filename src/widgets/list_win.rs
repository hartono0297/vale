//! The sticky note list window ("Daftar catatan tempel"): one layered
//! `ValeNoteList` popup (`WS_POPUP`, `WS_EX_TOOLWINDOW | WS_EX_TOPMOST`: no
//! taskbar button, not in Alt+Tab) that shows every sticky note as a row, with
//! a search box. It is modelled on the colour popover (`pop.rs`): thread-local
//! state, an `arrange_overlay::Canvas` on the shared `Gfx`, `take_foreground`,
//! and it closes on Esc and when it is deactivated (posted, not done inside
//! the handler). The rules (rows, search, geometry, placement, selection,
//! undo) are in `list.rs`; this file draws and routes.
//!
//! The search box is the project's hidden-EDIT pattern (`widgets/edit.rs`): a
//! layered window never draws its children, so the list draws the text, the
//! caret and the selection itself, and the EDIT (answering `WM_NCHITTEST` with
//! `HTTRANSPARENT`) is there for the IME, paste and undo. The caret is steady:
//! no blink timer.
//!
//! Power: nothing runs while the list is closed. Open, it reacts to messages
//! only (hover rides `TrackMouseEvent`, a message of this window); the one
//! timer, `TIMER_LIST_UNDO` (49) on this window, is set while the undo strip
//! shows and killed when it hides, is clicked or the window goes.

use std::cell::{Cell, RefCell};

use chrono::Local;
use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ROUNDED_RECT,
    ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_HIT_TEST_METRICS, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{EM_GETSEL, EM_LIMITTEXT, EM_SETSEL};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_A, VK_CONTROL, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_NEXT, VK_PRIOR, VK_RETURN, VK_UP,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DLGC_WANTALLKEYS, DefWindowProcW, DestroyMenu,
    DestroyWindow, ES_AUTOHSCROLL, GetCursorPos, GetWindowTextLengthW, GetWindowTextW, HMENU,
    IDC_ARROW, IDC_HAND, IDC_IBEAM, KillTimer, LoadCursorW, MF_STRING, PostMessageW,
    RegisterClassExW, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW, SetCursor, SetTimer,
    SetWindowPos, ShowWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    WA_INACTIVE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE, WM_CAPTURECHANGED, WM_CHAR, WM_CLEAR,
    WM_CLOSE, WM_COMMAND, WM_CUT, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETDLGCODE,
    WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_STARTCOMPOSITION, WM_KEYDOWN, WM_KILLFOCUS,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_MOVE, WM_NCDESTROY, WM_NCHITTEST,
    WM_NULL, WM_PASTE, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS, WM_SYSCOMMAND, WM_TIMER, WM_UNDO,
    WNDCLASSEXW, WS_CHILD, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};
use windows::core::{HSTRING, Interface, PCWSTR, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::arrange_overlay::{Canvas, take_foreground};
use crate::capture::Rect as Px;
use crate::config::UiFont;
use crate::i18n::{self, Key, Lang, text};
use crate::notes::{self, Note};
use crate::shot_edit::gfx::color;
use crate::theme;
use crate::ui::geometry::Rect;
use crate::ui::icons::Icon;
use crate::widgets::edit;
use crate::widgets::host::WM_APP_WIDGET;
use crate::widgets::list::{
    self, Hit, Layout, Mark, ROW_H, Row, WIN_W, filter, has_meta, layout, marks, row_geo, rows,
};
use crate::widgets::paint::{Res, hit_index, settle_index};
use crate::widgets::{remind, sticky};

/// The one timer of this window: the undo strip's 5 s, alive only while the
/// strip shows.
pub const TIMER_LIST_UNDO: usize = 49;
const UNDO_MS: u32 = 5000;

const CLASS: PCWSTR = w!("ValeNoteList");
/// The search box's text size, and how far its text starts from the box's left.
const FIELD_PT: f32 = 13.0;
const FIELD_TEXT_X: f32 = 34.0;
const FIELD_TEXT_PAD: f32 = 12.0;
const SUBCLASS_ID: usize = 9;
const ID_SEARCH: isize = 301;
/// Longest query the box takes.
const QUERY_MAX: usize = 200;
/// A window is on screen only outside the tests.
const ON_SCREEN: bool = !cfg!(test);

const HTCLIENT: isize = 1;
const HTCAPTION: isize = 2;
const HTTRANSPARENT: isize = -1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_NCLBUTTONDBLCLK: u32 = 0x00A3;
const WM_NCRBUTTONDOWN: u32 = 0x00A4;
const WM_NCRBUTTONUP: u32 = 0x00A5;
const EN_CHANGE: u32 = 0x0300;
const SC_MAXIMIZE: usize = 0xF030;
const SC_MINIMIZE: usize = 0xF020;
const SC_KEYMENU: usize = 0xF100;
const CMD_OPEN: usize = 1;
const CMD_DELETE: usize = 2;

// ------------------------------------------------------------------ API types

/// Where the window goes (physical px).
#[derive(Clone, Copy, Debug)]
pub enum Anchor {
    /// Beside the note's window: right of it, else left.
    Beside { note_px: Px },
    /// The top right of this work area.
    Default { work_px: Px },
}

/// What the list shows. Built only while the list is open.
#[derive(Clone)]
pub struct ListData {
    pub notes: Vec<Note>,
    pub lang: Lang,
    pub ui: UiFont,
    /// Another sticky note fits (the cap is not reached): (+) and "Buat
    /// catatan" work.
    pub can_add: bool,
    /// The last delete can be taken back (the owner holds its snapshot and
    /// the desktop has room): "Urungkan" works.
    pub can_undo: bool,
}

/// What the user did in the list, for the owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListEvent {
    /// A row was chosen: show the note and give it the keyboard.
    Show(String),
    /// A row was deleted (Delete key or the menu).
    Delete(String),
    /// "Urungkan".
    Undo,
    /// (+) or "Buat catatan".
    New,
    /// The window went: what is not undone now is final.
    Closed,
}

// ------------------------------------------------------------------ events

thread_local! {
    static EVENTS: RefCell<Vec<ListEvent>> = const { RefCell::new(Vec::new()) };
    static POSTED: Cell<bool> = const { Cell::new(false) };
    static OWNER: Cell<isize> = const { Cell::new(0) };
    static STATE: RefCell<Option<List>> = const { RefCell::new(None) };
}

/// Queues `ev` and tells the owner once (`WM_APP_WIDGET`, like the host).
fn push(ev: ListEvent) {
    EVENTS.with(|e| e.borrow_mut().push(ev));
    if !POSTED.with(|p| p.replace(true)) {
        let owner = OWNER.with(Cell::get);
        if owner != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(owner as *mut _)),
                    WM_APP_WIDGET,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }
}

/// What the user did since the last call.
pub fn drain() -> Vec<ListEvent> {
    POSTED.with(|p| p.set(false));
    EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

// ------------------------------------------------------------------ state

struct List {
    hwnd: HWND,
    edit: HWND,
    scale: f32,
    /// The window's top-left on the desktop (px).
    origin: (i32, i32),
    /// The window's height in dip (`list::fit_height`).
    h_dip: f32,
    data: ListData,
    res: Res,
    canvas: Option<Canvas>,
    /// Every sticky note's row, and those that match the query.
    all: Vec<Row>,
    shown: Vec<Row>,
    query: String,
    /// The EDIT's selection (start, end), UTF-16 units of `query`.
    sel: (usize, usize),
    focused: bool,
    scroll: f32,
    /// How far the search text is scrolled sideways, dip.
    scroll_x: f32,
    hover: Hit,
    /// The row picked with the arrow keys, by note id.
    selected: Option<String>,
    /// The undo strip shows (and `TIMER_LIST_UNDO` runs).
    strip: bool,
    tracking: bool,
    /// A drag in the search box started at this index.
    drag_from: Option<usize>,
    /// A native menu is up (it takes the activation).
    menu_open: bool,
    closing: bool,
}

fn with<R>(f: impl FnOnce(&mut List) -> R) -> Option<R> {
    STATE.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

pub fn is_open() -> bool {
    STATE.with(|cell| cell.try_borrow().map(|s| s.is_some()).unwrap_or(true))
}

fn layout_of(l: &List) -> Layout {
    layout(WIN_W, l.h_dip, l.strip)
}

/// The rows follow the notes and the query; a selection that is gone is
/// dropped and the scroll is kept inside.
fn recompute(l: &mut List) {
    let now = Local::now().timestamp();
    l.all = rows(&l.data.notes, now);
    l.shown = filter(&l.all, &l.data.notes, &l.query);
    if let Some(id) = &l.selected
        && !l.shown.iter().any(|r| &r.id == id)
    {
        l.selected = None;
    }
    let lay = layout_of(l);
    l.scroll = list::clamp_scroll(l.scroll, &lay, l.shown.len());
}

fn selected_index(l: &List) -> Option<usize> {
    let id = l.selected.as_ref()?;
    l.shown.iter().position(|r| &r.id == id)
}

fn select(l: &mut List, index: Option<usize>) {
    l.selected = index.and_then(|i| l.shown.get(i)).map(|r| r.id.clone());
    if let Some(i) = index {
        let lay = layout_of(l);
        l.scroll = list::ensure_visible(l.scroll, i, &lay);
        l.scroll = list::clamp_scroll(l.scroll, &lay, l.shown.len());
    }
}

fn show_strip(l: &mut List) {
    l.strip = true;
    // Set again on a second delete: its 5 s start over.
    unsafe {
        SetTimer(Some(l.hwnd), TIMER_LIST_UNDO, UNDO_MS, None);
    }
    let lay = layout_of(l);
    l.scroll = list::clamp_scroll(l.scroll, &lay, l.shown.len());
}

fn hide_strip(l: &mut List) {
    if l.strip {
        l.strip = false;
        unsafe {
            let _ = KillTimer(Some(l.hwnd), TIMER_LIST_UNDO);
        }
        let lay = layout_of(l);
        l.scroll = list::clamp_scroll(l.scroll, &lay, l.shown.len());
    }
}

// ------------------------------------------------------------------ placement

/// Where the window goes and how big it is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    scale: f32,
    h_dip: f32,
    size: (i32, i32),
    origin: (i32, i32),
}

/// `anchor` on a monitor with work area `monitor_work` and `dpi` (a default
/// anchor carries its own work area).
fn spot_from(anchor: &Anchor, monitor_work: Px, dpi: u32) -> Spot {
    let scale = list::scale_of(dpi);
    let work = match anchor {
        Anchor::Default { work_px } => *work_px,
        Anchor::Beside { .. } => monitor_work,
    };
    let h_dip = list::fit_height(work.h, scale);
    let size = list::size_px(WIN_W, h_dip, scale);
    let origin = match anchor {
        Anchor::Beside { note_px } => {
            list::place_beside(*note_px, work, size, (8.0 * scale).round() as i32)
        }
        Anchor::Default { .. } => list::place_default(work, size, (24.0 * scale).round() as i32),
    };
    Spot { scale, h_dip, size, origin }
}

/// The work area and DPI of the monitor holding `pt` (px).
fn monitor_at(pt: (i32, i32)) -> (Px, u32) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: pt.0, y: pt.1 }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let r = info.rcWork;
            Px::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
        } else {
            Px::new(0, 0, 1920, 1040)
        };
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        (work, dx.max(96))
    }
}

fn spot_for(anchor: &Anchor) -> Spot {
    let at = match anchor {
        Anchor::Beside { note_px } => (note_px.x + note_px.w / 2, note_px.y + note_px.h / 2),
        Anchor::Default { work_px } => (work_px.x + work_px.w / 2, work_px.y + work_px.h / 2),
    };
    let (work, dpi) = monitor_at(at);
    spot_from(anchor, work, dpi)
}

// ------------------------------------------------------------------ window

fn register() -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(list_proc),
        hInstance: module.into(),
        hCursor: cursor,
        lpszClassName: CLASS,
        ..Default::default()
    };
    unsafe {
        if RegisterClassExW(&wc) == 0 {
            return windows::core::Error::from_thread().code()
                == ERROR_CLASS_ALREADY_EXISTS.to_hresult();
        }
    }
    true
}

/// The search EDIT's place: the box's text area (px, client of the window).
fn edit_rect(h_dip: f32, scale: f32) -> (i32, i32, i32, i32) {
    let lay = layout(WIN_W, h_dip, false);
    let r = lay.search;
    (
        ((r.x + FIELD_TEXT_X) * scale).round() as i32,
        (r.y * scale).round() as i32,
        ((r.w - FIELD_TEXT_X - FIELD_TEXT_PAD).max(20.0) * scale).round() as i32,
        (r.h * scale).round() as i32,
    )
}

/// Opens the list at `anchor`. Open already: the window moves to the new
/// anchor, shows the new data and takes the keyboard (never a second window).
pub fn open(owner: HWND, anchor: Anchor, data: ListData) {
    OWNER.with(|o| o.set(owner.0 as isize));
    let spot = spot_for(&anchor);
    if is_open() {
        move_to(spot, data);
        return;
    }
    if !register() {
        return;
    }
    let Ok(res) = Res::new(data.ui) else { return };
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return;
    };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS,
            w!("Vale sticky note list"),
            WS_POPUP,
            spot.origin.0,
            spot.origin.1,
            spot.size.0,
            spot.size.1,
            None,
            None,
            Some(module.into()),
            None,
        )
    };
    let Ok(hwnd) = hwnd else { return };
    let Some(edit_hwnd) = create_edit(hwnd, &spot) else {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return;
    };
    let canvas = Canvas::with_gfx(res.gfx.clone(), spot.size).ok();
    let mut state = List {
        hwnd,
        edit: edit_hwnd,
        scale: spot.scale,
        origin: spot.origin,
        h_dip: spot.h_dip,
        data,
        res,
        canvas,
        all: Vec::new(),
        shown: Vec::new(),
        query: String::new(),
        sel: (0, 0),
        focused: false,
        scroll: 0.0,
        scroll_x: 0.0,
        hover: Hit::None,
        selected: None,
        strip: false,
        tracking: false,
        drag_from: None,
        menu_open: false,
        closing: false,
    };
    recompute(&mut state);
    STATE.with(|cell| *cell.borrow_mut() = Some(state));
    redraw();
    if ON_SCREEN {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        take_foreground(hwnd);
    }
    unsafe {
        let _ = SetFocus(Some(edit_hwnd));
    }
}

fn create_edit(parent: HWND, spot: &Spot) -> Option<HWND> {
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    let (x, y, w, h) = edit_rect(spot.h_dip, spot.scale);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            x,
            y,
            w,
            h,
            Some(parent),
            Some(HMENU(ID_SEARCH as *mut _)),
            Some(module.into()),
            None,
        )
    }
    .ok()?;
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID, 0);
        SendMessageW(hwnd, EM_LIMITTEXT, Some(WPARAM(QUERY_MAX)), Some(LPARAM(0)));
    }
    Some(hwnd)
}

/// The open window goes to `spot`, shows `data` and takes the keyboard.
fn move_to(spot: Spot, data: ListData) {
    let todo = with(|l| {
        if data.ui != l.data.ui {
            l.res.set_font(data.ui);
        }
        l.data = data;
        l.scale = spot.scale;
        l.h_dip = spot.h_dip;
        l.origin = spot.origin;
        l.canvas = Canvas::with_gfx(l.res.gfx.clone(), spot.size).ok();
        recompute(l);
        (l.hwnd, l.edit)
    });
    let Some((hwnd, edit_hwnd)) = todo else { return };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            None,
            spot.origin.0,
            spot.origin.1,
            spot.size.0,
            spot.size.1,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        let (x, y, w, h) = edit_rect(spot.h_dip, spot.scale);
        let _ = SetWindowPos(edit_hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    redraw();
    if ON_SCREEN {
        take_foreground(hwnd);
    }
    unsafe {
        let _ = SetFocus(Some(edit_hwnd));
    }
}

/// The notes (or the language, the font, the cap) changed: the rows follow.
/// Nothing when the list is closed.
pub fn refresh(data: ListData) {
    let _ = with(|l| {
        if data.ui != l.data.ui {
            l.res.set_font(data.ui);
        }
        l.data = data;
        recompute(l);
    });
    redraw();
}

/// The theme changed: the palette is read again at the next draw.
pub fn restyle() {
    redraw();
}

pub fn close() {
    let _ = with(|l| l.closing = true);
    let hwnd = STATE.with(|cell| cell.try_borrow().ok().and_then(|s| s.as_ref().map(|l| l.hwnd)));
    if let Some(h) = hwnd {
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

/// Asks the window to close itself (from inside a handler): `WM_CLOSE`.
fn post_close() {
    let hwnd = STATE.with(|cell| cell.try_borrow().ok().and_then(|s| s.as_ref().map(|l| l.hwnd)));
    if let Some(h) = hwnd {
        unsafe {
            let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

// ------------------------------------------------------------------ the search text

fn edit_text(edit: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(edit) }.max(0) as usize;
    let mut buf = vec![0u16; len + 1];
    let n = unsafe { GetWindowTextW(edit, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n.min(len)])
}

fn edit_selection(edit: HWND) -> (usize, usize) {
    let (mut a, mut b) = (0u32, 0u32);
    unsafe {
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut a as *mut u32 as usize)),
            Some(LPARAM(&mut b as *mut u32 as isize)),
        );
    }
    (a as usize, b as usize)
}

/// Reads the EDIT: a new text filters the rows, clears the selection and goes
/// back to the top. True when the text changed.
fn sync_from_edit(l: &mut List) -> bool {
    let q = edit_text(l.edit);
    l.sel = edit_selection(l.edit);
    if q == l.query {
        return false;
    }
    l.query = q;
    l.shown = filter(&l.all, &l.data.notes, &l.query);
    let sel = list::selection_after_typing(selected_index(l));
    select(l, sel);
    l.scroll = 0.0;
    true
}

/// The search text laid out in one line.
fn query_layout(res: &Res, query: &str) -> Option<windows::Win32::Graphics::DirectWrite::IDWriteTextLayout> {
    let l = res.layout(query, FIELD_PT, 400, 10_000.0).ok()?;
    unsafe {
        let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    }
    Some(l)
}

/// The x of the caret at `index` in the laid-out query (dip from its start).
fn caret_x(layout: &windows::Win32::Graphics::DirectWrite::IDWriteTextLayout, index: usize) -> f32 {
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let mut m = DWRITE_HIT_TEST_METRICS::default();
    if unsafe { layout.HitTestTextPosition(index as u32, false, &mut x, &mut y, &mut m) }.is_ok() {
        x
    } else {
        0.0
    }
}

/// The index in `query` at `x` dip from the start of its text.
fn index_at(res: &Res, query: &str, x: f32) -> usize {
    let Some(layout) = query_layout(res, query) else {
        return 0;
    };
    let units: Vec<u16> = query.encode_utf16().collect();
    let (mut trailing, mut inside) = (windows::core::BOOL(0), windows::core::BOOL(0));
    let mut m = DWRITE_HIT_TEST_METRICS::default();
    if unsafe { layout.HitTestPoint(x.max(0.0), 1.0, &mut trailing, &mut inside, &mut m) }.is_err() {
        return units.len();
    }
    let pos = hit_index(m.textPosition, m.length, trailing.as_bool());
    settle_index(&units, pos).min(units.len())
}

// ------------------------------------------------------------------ keys from the EDIT

/// What the search EDIT's subclass tells the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditKey {
    /// The text or the selection may have changed.
    Changed,
    Escape,
    Up,
    Down,
    PageUp,
    PageDown,
    Enter,
    /// Delete: only a row picked with the keys takes it; otherwise it is the
    /// EDIT's.
    Delete,
    Focus(bool),
    Wheel(i32),
}

/// Handles `key`; true when the list consumed it (the EDIT must not see it).
pub(crate) fn edit_key(key: EditKey) -> bool {
    match key {
        EditKey::Escape => {
            post_close();
            true
        }
        EditKey::Up | EditKey::Down | EditKey::PageUp | EditKey::PageDown => {
            let _ = with(|l| {
                let lay = layout_of(l);
                let page = ((lay.rows_view.h / ROW_H).floor() as i32).max(1);
                let delta = match key {
                    EditKey::Up => -1,
                    EditKey::Down => 1,
                    EditKey::PageUp => -page,
                    _ => page,
                };
                let next = list::move_selection(selected_index(l), delta, l.shown.len());
                select(l, next);
            });
            redraw();
            true
        }
        EditKey::Enter => {
            let _ = with(|l| {
                if let Some(i) = list::enter_target(selected_index(l), l.shown.len())
                    && let Some(r) = l.shown.get(i)
                {
                    push(ListEvent::Show(r.id.clone()));
                }
            });
            true
        }
        EditKey::Delete => {
            let consumed = with(|l| {
                let i = selected_index(l)?;
                let id = l.shown.get(i)?.id.clone();
                delete_row(l, &id, Some(i));
                Some(())
            })
            .flatten()
            .is_some();
            if consumed {
                redraw();
            }
            consumed
        }
        EditKey::Changed => {
            let _ = with(|l| {
                sync_from_edit(l);
            });
            redraw();
            false
        }
        EditKey::Focus(on) => {
            let _ = with(|l| {
                l.focused = on;
                sync_from_edit(l);
            });
            redraw();
            false
        }
        EditKey::Wheel(delta) => {
            wheel_by(delta);
            true
        }
    }
}

/// Pushes the delete of `id`, shows the strip and moves the keyboard
/// selection to the row that takes its place.
fn delete_row(l: &mut List, id: &str, picked: Option<usize>) {
    push(ListEvent::Delete(id.to_owned()));
    show_strip(l);
    if let Some(i) = picked {
        let n = l.shown.len();
        let next = if i + 1 < n { Some(i + 1) } else { i.checked_sub(1) };
        select(l, next);
    }
}

fn wheel_by(delta: i32) {
    let changed = with(|l| {
        let lay = layout_of(l);
        let to = list::wheel(l.scroll, delta, &lay, l.shown.len());
        let changed = (to - l.scroll).abs() > f32::EPSILON;
        l.scroll = to;
        changed
    })
    .unwrap_or(false);
    if changed {
        redraw();
    }
}

// ------------------------------------------------------------------ drawing

#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Center,
    Right,
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    res: &Res,
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    s: &str,
    r: Rect,
    size: f32,
    weight: u16,
    rgb: u32,
    alpha: f32,
    align: Align,
) {
    if s.is_empty() || r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let Ok(l) = res.layout(s, size, weight, r.w) else {
        return;
    };
    unsafe {
        let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = l.SetMaxHeight(r.h);
        let _ = l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        let _ = l.SetTextAlignment(match align {
            Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
            Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
        });
        // One line, cut with an ellipsis where the box ends.
        if let Ok(sign) = res.gfx.dwrite.CreateEllipsisTrimmingSign(&l) {
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            let _ = l.SetTrimming(&trimming, &sign);
        }
        brush.SetColor(&color(rgb, alpha));
        rt.DrawTextLayout(Vector2::new(r.x, r.y), &l, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
    }
}

/// How wide `s` is at `size`, dip.
fn text_w(res: &Res, s: &str, size: f32, weight: u16) -> f32 {
    let Ok(l) = res.layout(s, size, weight, 10_000.0) else {
        return 0.0;
    };
    let mut m = DWRITE_TEXT_METRICS::default();
    if unsafe { l.GetMetrics(&mut m) }.is_ok() { m.widthIncludingTrailingWhitespace } else { 0.0 }
}

fn rr(r: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h },
        radiusX: radius,
        radiusY: radius,
    }
}

fn shifted(r: Rect, dy: f32) -> Rect {
    Rect { y: r.y + dy, ..r }
}

fn redraw() {
    let _ = with(paint);
}

fn paint(l: &mut List) {
    let pal = theme::current_window();
    let k = l.scale;
    let lay = layout_of(l);
    // The caret's place decides how the search text is scrolled.
    let view_w = lay.search.w - FIELD_TEXT_X - FIELD_TEXT_PAD;
    let q_layout = query_layout(&l.res, &l.query);
    let caret_index = if l.sel.0 == l.sel.1 { l.sel.1 } else { l.sel.1 };
    let caret_at = q_layout.as_ref().map_or(0.0, |q| caret_x(q, caret_index));
    l.scroll_x = list::follow_caret(l.scroll_x, caret_at, view_w);
    // The IME's composition window goes at the caret.
    edit::place_ime(l.edit, ((caret_at - l.scroll_x) * k).round() as i32, 0);
    let List {
        hwnd,
        origin,
        res,
        canvas,
        data,
        all,
        shown,
        query,
        sel,
        focused,
        scroll,
        scroll_x,
        hover,
        selected,
        strip,
        ..
    } = &*l;
    let Some(canvas) = canvas else { return };
    let lang = data.lang;
    let t = theme::current();
    let island = theme::island(t.accent, t.surface);
    let today = Local::now().date_naive();
    let selected_at = selected.as_ref().and_then(|id| shown.iter().position(|r| &r.id == id));
    let _ = canvas.paint_with(*hwnd, *origin, 0.0, 255, |c| {
        let (rt, brush, _) = c.parts();
        let Ok(rt) = rt.cast::<ID2D1RenderTarget>() else {
            return;
        };
        let set = |rgb: u32, a: f32| unsafe { brush.SetColor(&color(rgb, a)) };
        let set_rgba = |rgba: u32, mul: f32| unsafe {
            brush.SetColor(&color(rgba >> 8, (rgba & 0xFF) as f32 / 255.0 * mul))
        };
        let fill = |r: Rect, radius: f32| unsafe { rt.FillRoundedRectangle(&rr(r, radius), brush) };
        let stroke = |r: Rect, radius: f32, w: f32| unsafe {
            rt.DrawRoundedRectangle(&rr(r, radius), brush, w, None)
        };
        let icon = |ic: Icon, r: Rect, size: f32, ink: u32, a: f32| {
            let _ = res.icons.draw(&rt, brush, ic, r, size, ink, a, None);
        };
        let text = |s: &str, r: Rect, size: f32, weight: u16, rgb: u32, a: f32, align: Align| {
            draw_text(res, &rt, brush, s, r, size, weight, rgb, a, align);
        };
        unsafe { rt.SetTransform(&Matrix3x2::scale(k, k)) };

        // The plate, with a hairline.
        let whole = Rect { x: 0.5, y: 0.5, w: WIN_W - 1.0, h: lay.h - 1.0 };
        set(pal.bg, 1.0);
        fill(whole, 12.0);
        set_rgba(pal.border, 2.0);
        stroke(whole, 12.0, 1.0);

        // The header: the title over the count (and the cap's reason), then (+) and the close button.
        let title_box = Rect { x: 16.0, y: 7.0, w: lay.plus.x - 16.0 - 8.0, h: 20.0 };
        text(i18n::text(lang, Key::StickyList), title_box, 14.5, 600, pal.text, 1.0, Align::Left);
        let mut sub = i18n::count_label(lang, all.len());
        if !data.can_add {
            sub = format!("{sub} \u{b7} {}", i18n::text(lang, Key::StickyMax));
        }
        let sub_box = Rect { x: 16.0, y: 27.0, w: title_box.w, h: 16.0 };
        text(&sub, sub_box, 11.0, 400, pal.secondary, 1.0, Align::Left);
        for (r, hit, ic, enabled) in [
            (lay.plus, Hit::Plus, Icon::Plus, data.can_add),
            (lay.close, Hit::Close, Icon::X, true),
        ] {
            if *hover == hit && enabled {
                set(pal.hover, 1.0);
                fill(r, 8.0);
            }
            icon(ic, r, 14.0, pal.secondary, if enabled { 1.0 } else { 0.4 });
        }

        // The search box.
        let field = lay.search;
        set(pal.field, 1.0);
        fill(field, 10.0);
        if *focused {
            set(pal.accent, 0.9);
            stroke(Rect { x: field.x + 0.75, y: field.y + 0.75, w: field.w - 1.5, h: field.h - 1.5 }, 9.5, 1.5);
        } else {
            set_rgba(pal.border, 2.0);
            stroke(Rect { x: field.x + 0.5, y: field.y + 0.5, w: field.w - 1.0, h: field.h - 1.0 }, 9.5, 1.0);
        }
        icon(
            Icon::Search,
            Rect { x: field.x + 8.0, y: field.y, w: 20.0, h: field.h },
            14.0,
            pal.secondary,
            1.0,
        );
        let text_box = Rect { x: field.x + FIELD_TEXT_X, y: field.y, w: view_w, h: field.h };
        if query.is_empty() {
            text(
                i18n::text(lang, Key::StickyListSearch),
                text_box,
                FIELD_PT,
                400,
                pal.tertiary,
                1.0,
                Align::Left,
            );
        } else if let Some(q) = &q_layout {
            unsafe {
                rt.PushAxisAlignedClip(
                    &D2D_RECT_F {
                        left: text_box.x,
                        top: text_box.y,
                        right: text_box.x + text_box.w,
                        bottom: text_box.y + text_box.h,
                    },
                    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                );
                let _ = q.SetMaxHeight(text_box.h);
                let _ = q.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            }
            let ox = text_box.x - scroll_x;
            // The selection first, under the text.
            if sel.0 != sel.1 {
                let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
                let mut count = 0u32;
                let _ = unsafe { q.HitTestTextRange(a as u32, (b - a) as u32, 0.0, 0.0, None, &mut count) };
                let mut metrics = vec![DWRITE_HIT_TEST_METRICS::default(); count as usize];
                if unsafe {
                    q.HitTestTextRange(a as u32, (b - a) as u32, 0.0, 0.0, Some(&mut metrics), &mut count)
                }
                .is_ok()
                {
                    set_rgba(pal.selection, 1.0);
                    for m in metrics.iter().take(count as usize) {
                        fill(
                            Rect { x: ox + m.left, y: text_box.y + m.top, w: m.width, h: m.height },
                            2.0,
                        );
                    }
                }
            }
            set(pal.text, 1.0);
            unsafe { rt.DrawTextLayout(Vector2::new(ox, text_box.y), q, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP) };
            unsafe { rt.PopAxisAlignedClip() };
        }
        // The caret: steady, only with the keyboard and no selection.
        if *focused && sel.0 == sel.1 {
            let x = text_box.x + caret_at - scroll_x;
            set(pal.text, 1.0);
            unsafe {
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left: x,
                        top: field.y + 9.0,
                        right: x + 1.5,
                        bottom: field.y + field.h - 9.0,
                    },
                    brush,
                );
            }
        }

        // The rows, or what stands in for them.
        let view = lay.rows_view;
        let n = shown.len();
        if n == 0 {
            let msg = if all.is_empty() {
                i18n::text(lang, Key::StickyListNone)
            } else {
                i18n::text(lang, Key::StickyListNoMatch)
            };
            text(msg, list::empty_message(&lay), 13.5, 600, pal.text, 1.0, Align::Center);
            if all.is_empty() {
                let b = list::empty_button(&lay);
                let on = data.can_add;
                if on {
                    set(pal.accent, if *hover == Hit::None { 1.0 } else { 1.0 });
                } else {
                    set(pal.button, 1.0);
                }
                fill(b, 10.0);
                text(
                    i18n::text(lang, Key::StickyListNew),
                    b,
                    12.5,
                    600,
                    if on { pal.on_accent } else { pal.secondary },
                    1.0,
                    Align::Center,
                );
                if !on {
                    text(
                        i18n::text(lang, Key::StickyMax),
                        Rect { x: SIDE_PAD, y: b.bottom() + 6.0, w: WIN_W - 2.0 * SIDE_PAD, h: 18.0 },
                        11.0,
                        400,
                        pal.secondary,
                        1.0,
                        Align::Center,
                    );
                }
            }
        } else {
            unsafe {
                rt.PushAxisAlignedClip(
                    &D2D_RECT_F {
                        left: view.x,
                        top: view.y,
                        right: view.x + view.w,
                        bottom: view.y + view.h,
                    },
                    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                );
            }
            let first = (scroll / ROW_H).floor().max(0.0) as usize;
            let last = ((scroll + view.h) / ROW_H).ceil().max(0.0) as usize;
            for i in first..last.min(n) {
                let row = &shown[i];
                let top = view.y + i as f32 * ROW_H - scroll;
                let g = row_geo(WIN_W, has_meta(row));
                let hot = *hover == Hit::Row(i);
                let picked = selected_at == Some(i);
                if hot || picked {
                    set(pal.hover, 1.0);
                    fill(shifted(g.card, top), 10.0);
                }
                if picked {
                    set(pal.accent, 0.9);
                    stroke(
                        shifted(
                            Rect {
                                x: g.card.x + 0.75,
                                y: g.card.y + 0.75,
                                w: g.card.w - 1.5,
                                h: g.card.h - 1.5,
                            },
                            top,
                        ),
                        9.5,
                        1.5,
                    );
                }
                // The paper's own colour is the only paper colour here.
                let tones = sticky::tones(row.paper, &island, t.accent);
                let strip_r = shifted(g.strip, top);
                set(tones.fill, 1.0);
                fill(strip_r, 2.0);
                set(pal.text, 0.22);
                stroke(strip_r, 2.0, 0.75);
                // Title and snippet.
                if row.title.is_empty() {
                    text(
                        i18n::text(lang, Key::StickyEmpty),
                        shifted(g.title, top),
                        13.5,
                        600,
                        pal.secondary,
                        1.0,
                        Align::Left,
                    );
                } else {
                    text(&row.title, shifted(g.title, top), 13.5, 600, pal.text, 1.0, Align::Left);
                }
                text(&row.snippet, shifted(g.snippet, top), 12.0, 400, pal.secondary, 1.0, Align::Left);
                // The time, and the status icons under it.
                let when = notes::note_time_label(lang, remind::from_unix(row.updated), today);
                text(&when, shifted(g.time, top), 11.0, 400, pal.secondary, 1.0, Align::Right);
                let ms = marks(row);
                let icons = shifted(g.icons, top);
                for (j, m) in ms.iter().enumerate() {
                    let r = list::mark_rect(icons, j, ms.len());
                    let (ic, ink) = match m {
                        Mark::Pinned => (Icon::Pin, pal.secondary),
                        Mark::Rolled => (Icon::ChevU, pal.secondary),
                        Mark::Tucked => (Icon::Ext, pal.secondary),
                        Mark::Reminder(true) => (Icon::Bell, pal.accent),
                        Mark::Reminder(false) => (Icon::Clock, pal.secondary),
                    };
                    icon(ic, r, list::ICON, ink, 1.0);
                }
                // The third line: the stack's place and the checklist's count.
                if let Some(meta) = g.meta {
                    let meta = shifted(meta, top);
                    let mut x = meta.x;
                    if let Some((pos, len)) = row.stack {
                        let badge = i18n::stack_badge(lang, pos, len);
                        icon(
                            Icon::Copy,
                            Rect { x, y: meta.y, w: 12.0, h: meta.h },
                            12.0,
                            pal.secondary,
                            1.0,
                        );
                        let bw = text_w(res, &badge, 10.5, 500).ceil() + 2.0;
                        text(
                            &badge,
                            Rect { x: x + 16.0, y: meta.y, w: bw, h: meta.h },
                            10.5,
                            500,
                            pal.secondary,
                            1.0,
                            Align::Left,
                        );
                        x += 16.0 + bw + 10.0;
                    }
                    if let Some((done, total)) = row.progress {
                        let ink = if done == total { pal.accent } else { pal.secondary };
                        icon(Icon::Check, Rect { x, y: meta.y, w: 12.0, h: meta.h }, 12.0, ink, 1.0);
                        text(
                            &format!("{done}/{total}"),
                            Rect { x: x + 16.0, y: meta.y, w: 48.0, h: meta.h },
                            10.5,
                            500,
                            ink,
                            1.0,
                            Align::Left,
                        );
                    }
                }
            }
            unsafe { rt.PopAxisAlignedClip() };
            // A thin scroll thumb while the rows overflow.
            let max = list::max_scroll(&lay, n);
            if max > 0.0 {
                let track = view.h - 8.0;
                let thumb = (track * view.h / (n as f32 * ROW_H)).max(24.0);
                let y = view.y + 4.0 + (track - thumb) * (scroll / max);
                set(pal.secondary, 0.35);
                fill(Rect { x: WIN_W - 6.0, y, w: 3.0, h: thumb }, 1.5);
            }
        }

        // The undo strip.
        if let (Some(s), Some(b)) = (lay.strip, lay.undo_btn) {
            set(pal.card, 1.0);
            unsafe {
                rt.PushAxisAlignedClip(
                    &D2D_RECT_F { left: s.x, top: s.y, right: s.x + s.w, bottom: s.y + s.h },
                    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                );
            }
            fill(Rect { x: 0.5, y: s.y - 14.0, w: WIN_W - 1.0, h: s.h + 14.0 - 0.5 }, 12.0);
            unsafe { rt.PopAxisAlignedClip() };
            set_rgba(pal.border, 2.0);
            unsafe {
                rt.DrawLine(
                    Vector2::new(1.0, s.y),
                    Vector2::new(WIN_W - 1.0, s.y),
                    brush,
                    1.0,
                    None,
                );
            }
            text(
                i18n::text(lang, Key::StickyListDeleted),
                Rect { x: SIDE_PAD, y: s.y, w: b.x - SIDE_PAD - 8.0, h: s.h },
                12.0,
                400,
                pal.text,
                1.0,
                Align::Left,
            );
            let on = data.can_undo;
            if on && *hover == Hit::Undo {
                set(pal.hover, 1.0);
                fill(b, 8.0);
            }
            text(
                i18n::text(lang, Key::StickyListUndo),
                b,
                12.0,
                600,
                if on { pal.accent } else { pal.secondary },
                if on { 1.0 } else { 0.5 },
                Align::Center,
            );
        }
        let _ = strip;
        unsafe { rt.SetTransform(&Matrix3x2::identity()) };
    });
}

/// The side margin the text-only lines use.
const SIDE_PAD: f32 = list::SIDE;

// ------------------------------------------------------------------ messages

fn lo(l: LPARAM) -> i32 {
    (l.0 & 0xFFFF) as i16 as i32
}

fn hi(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xFFFF) as i16 as i32
}

fn to_dip(l: &List, x: i32, y: i32) -> (f32, f32) {
    (x as f32 / l.scale, y as f32 / l.scale)
}

/// What a point (dip) is on, the empty-state button included (it acts like
/// (+) and is reported as `Plus`).
fn hit_with_empty(l: &List, lay: &Layout, x: f32, y: f32) -> Hit {
    if l.all.is_empty() && list::empty_button(lay).contains(x, y) {
        return Hit::Plus;
    }
    list::hit(lay, x, y, l.scroll, l.shown.len())
}

enum Act {
    Nothing,
    New,
    Close,
    Undo,
    Show(String),
    /// A press in the search box at this index.
    Field(HWND, usize),
}

fn left_down(hwnd: HWND, x: i32, y: i32) {
    let act = with(|l| {
        let (dx, dy) = to_dip(l, x, y);
        let lay = layout_of(l);
        match hit_with_empty(l, &lay, dx, dy) {
            Hit::Plus if l.data.can_add => Act::New,
            Hit::Plus => Act::Nothing,
            Hit::Close => Act::Close,
            Hit::Undo if l.data.can_undo => {
                hide_strip(l);
                Act::Undo
            }
            Hit::Row(i) => l.shown.get(i).map_or(Act::Nothing, |r| Act::Show(r.id.clone())),
            Hit::Search => {
                let text_x = dx - lay.search.x - FIELD_TEXT_X + l.scroll_x;
                Act::Field(l.edit, index_at(&l.res, &l.query, text_x))
            }
            Hit::Undo | Hit::Header | Hit::None => Act::Nothing,
        }
    })
    .unwrap_or(Act::Nothing);
    match act {
        Act::Nothing => {}
        Act::New => push(ListEvent::New),
        Act::Close => close(),
        Act::Undo => {
            push(ListEvent::Undo);
            redraw();
        }
        Act::Show(id) => push(ListEvent::Show(id)),
        Act::Field(edit_hwnd, at) => {
            unsafe {
                let _ = SetFocus(Some(edit_hwnd));
                SendMessageW(edit_hwnd, EM_SETSEL, Some(WPARAM(at)), Some(LPARAM(at as isize)));
                SetCapture(hwnd);
            }
            let _ = with(|l| {
                l.drag_from = Some(at);
                l.sel = (at, at);
            });
            redraw();
        }
    }
}

/// What a mouse move asks for.
enum Move {
    Nothing,
    Redraw,
    /// The drag in the search box now ends at this index (EDIT, from, to).
    Select(HWND, usize, usize),
}

fn mouse_move(hwnd: HWND, x: i32, y: i32) {
    let act = with(|l| {
        let (dx, dy) = to_dip(l, x, y);
        let lay = layout_of(l);
        if let Some(from) = l.drag_from {
            let text_x = dx - lay.search.x - FIELD_TEXT_X + l.scroll_x;
            return Move::Select(l.edit, from, index_at(&l.res, &l.query, text_x));
        }
        // Hover is only this window's: `TME_LEAVE` says when the pointer goes.
        if !l.tracking {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            if unsafe { TrackMouseEvent(&mut track) }.is_ok() {
                l.tracking = true;
            }
        }
        let now = hit_with_empty(l, &lay, dx, dy);
        if now == l.hover {
            return Move::Nothing;
        }
        l.hover = now;
        Move::Redraw
    })
    .unwrap_or(Move::Nothing);
    match act {
        Move::Nothing => {}
        Move::Redraw => redraw(),
        Move::Select(edit_hwnd, from, to) => {
            unsafe {
                SendMessageW(edit_hwnd, EM_SETSEL, Some(WPARAM(from)), Some(LPARAM(to as isize)));
            }
            let _ = with(|l| l.sel = (from.min(to), from.max(to)));
            redraw();
        }
    }
}

fn right_up(hwnd: HWND, x: i32, y: i32) {
    let row = with(|l| {
        let (dx, dy) = to_dip(l, x, y);
        let lay = layout_of(l);
        match list::hit(&lay, dx, dy, l.scroll, l.shown.len()) {
            Hit::Row(i) => {
                l.menu_open = true;
                l.shown.get(i).map(|r| (r.id.clone(), l.data.lang, l.origin))
            }
            _ => None,
        }
    })
    .flatten();
    let Some((id, lang, origin)) = row else { return };
    let chosen = unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            let _ = with(|l| l.menu_open = false);
            return;
        };
        let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN, &HSTRING::from(text(lang, Key::StickyListOpen)));
        let _ = AppendMenuW(menu, MF_STRING, CMD_DELETE, &HSTRING::from(text(lang, Key::StickyListDelete)));
        let mut at = POINT { x: origin.0 + x, y: origin.1 + y };
        let _ = GetCursorPos(&mut at);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            at.x,
            at.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    };
    let edit_hwnd = with(|l| {
        l.menu_open = false;
        l.edit
    });
    match chosen {
        CMD_OPEN => push(ListEvent::Show(id)),
        CMD_DELETE => {
            let _ = with(|l| {
                let at = l.shown.iter().position(|r| r.id == id);
                delete_row(l, &id, at.filter(|i| selected_index(l) == Some(*i)));
            });
            redraw();
        }
        _ => {}
    }
    // The keyboard goes back to the search box.
    if let Some(e) = edit_hwnd {
        unsafe {
            let _ = SetFocus(Some(e));
        }
    }
}

unsafe extern "system" fn list_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        // The header outside its buttons drags the window.
        WM_NCHITTEST => {
            let (sx, sy) = (lo(lparam), hi(lparam));
            let part = with(|l| {
                let (dx, dy) = to_dip(l, sx - l.origin.0, sy - l.origin.1);
                let lay = layout_of(l);
                list::hit(&lay, dx, dy, l.scroll, l.shown.len())
            });
            return LRESULT(if part == Some(Hit::Header) { HTCAPTION } else { HTCLIENT });
        }
        // No maximise, no system menu: it is a small popup, only moved.
        WM_NCLBUTTONDBLCLK | WM_NCRBUTTONDOWN | WM_NCRBUTTONUP => return LRESULT(0),
        WM_SYSCOMMAND => {
            let cmd = wparam.0 & 0xFFF0;
            if cmd == SC_MAXIMIZE || cmd == SC_MINIMIZE || cmd == SC_KEYMENU {
                return LRESULT(0);
            }
        }
        WM_MOVE => {
            let _ = with(|l| l.origin = (lo(lparam), hi(lparam)));
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            mouse_move(hwnd, lo(lparam), hi(lparam));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            let changed = with(|l| {
                l.tracking = false;
                let changed = l.hover != Hit::None;
                l.hover = Hit::None;
                changed
            })
            .unwrap_or(false);
            if changed {
                redraw();
            }
            return LRESULT(0);
        }
        WM_SETCURSOR => {
            if (lparam.0 & 0xFFFF) as isize == HTCLIENT {
                let which = with(|l| match l.hover {
                    Hit::Search => IDC_IBEAM,
                    Hit::Plus if !l.data.can_add => IDC_ARROW,
                    Hit::Undo if !l.data.can_undo => IDC_ARROW,
                    Hit::Plus | Hit::Close | Hit::Row(_) | Hit::Undo => IDC_HAND,
                    _ => IDC_ARROW,
                })
                .unwrap_or(IDC_ARROW);
                unsafe {
                    if let Ok(c) = LoadCursorW(None, which) {
                        SetCursor(Some(c));
                    }
                }
                return LRESULT(1);
            }
        }
        WM_LBUTTONDOWN => {
            left_down(hwnd, lo(lparam), hi(lparam));
            return LRESULT(0);
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            let held = with(|l| l.drag_from.take().is_some()).unwrap_or(false);
            if held && msg == WM_LBUTTONUP {
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            return LRESULT(0);
        }
        WM_RBUTTONUP => {
            right_up(hwnd, lo(lparam), hi(lparam));
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            wheel_by(((wparam.0 >> 16) & 0xFFFF) as i16 as i32);
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            if (wparam.0 & 0xFFFF) as u16 == VK_ESCAPE.0 {
                post_close();
            }
            return LRESULT(0);
        }
        // The search box's text changed.
        WM_COMMAND => {
            if ((wparam.0 >> 16) & 0xFFFF) as u32 == EN_CHANGE {
                let _ = with(|l| {
                    sync_from_edit(l);
                });
                redraw();
            }
            return LRESULT(0);
        }
        WM_SETFOCUS => {
            // The keyboard belongs to the search box.
            if let Some(e) = with(|l| l.edit) {
                unsafe {
                    let _ = SetFocus(Some(e));
                }
            }
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == TIMER_LIST_UNDO => {
            // Five seconds are up: the delete stands.
            let _ = with(hide_strip);
            redraw();
            return LRESULT(0);
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                let leave = with(|l| !l.menu_open && !l.closing).unwrap_or(false);
                if leave {
                    post_close();
                }
            }
            return LRESULT(0);
        }
        WM_CLOSE => {
            close();
            return LRESULT(0);
        }
        WM_DPICHANGED => {
            // The window keeps its top-left and takes the new monitor's scale.
            let dpi = (wparam.0 & 0xFFFF) as u32;
            let todo = with(|l| {
                let scale = list::scale_of(dpi);
                if (scale - l.scale).abs() < 0.001 {
                    return None;
                }
                l.scale = scale;
                let size = list::size_px(WIN_W, l.h_dip, scale);
                l.canvas = Canvas::with_gfx(l.res.gfx.clone(), size).ok();
                Some((l.hwnd, l.edit, l.origin, size, l.h_dip, scale))
            })
            .flatten();
            if let Some((h, e, origin, size, h_dip, scale)) = todo {
                unsafe {
                    let _ = SetWindowPos(
                        h,
                        None,
                        origin.0,
                        origin.1,
                        size.0,
                        size.1,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    let (x, y, w, hh) = edit_rect(h_dip, scale);
                    let _ = SetWindowPos(e, None, x, y, w, hh, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                redraw();
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_LIST_UNDO);
            }
            let gone = STATE.with(|cell| cell.try_borrow_mut().ok().and_then(|mut s| s.take()));
            if gone.is_some() {
                drop(gone);
                push(ListEvent::Closed);
            }
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

// ------------------------------------------------------------------ the EDIT's subclass

fn ctrl_down() -> bool {
    unsafe { GetKeyState(i32::from(VK_CONTROL.0)) < 0 }
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        // Never hit: every press goes through it to the list's own procedure.
        WM_NCHITTEST => return LRESULT(HTTRANSPARENT),
        WM_GETDLGCODE => {
            let code = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            return LRESULT(code.0 | DLGC_WANTALLKEYS as isize);
        }
        WM_KEYDOWN => {
            let key = (wparam.0 & 0xFFFF) as u16;
            let mine = match key {
                k if k == VK_ESCAPE.0 => Some(EditKey::Escape),
                k if k == VK_UP.0 => Some(EditKey::Up),
                k if k == VK_DOWN.0 => Some(EditKey::Down),
                k if k == VK_PRIOR.0 => Some(EditKey::PageUp),
                k if k == VK_NEXT.0 => Some(EditKey::PageDown),
                k if k == VK_RETURN.0 => Some(EditKey::Enter),
                k if k == VK_DELETE.0 => Some(EditKey::Delete),
                _ => None,
            };
            if let Some(k) = mine
                && edit_key(k)
            {
                return LRESULT(0);
            }
            if key == VK_A.0 && ctrl_down() {
                unsafe {
                    SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                }
                edit_key(EditKey::Changed);
                return LRESULT(0);
            }
            let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            edit_key(EditKey::Changed);
            return r;
        }
        WM_CHAR => {
            // Esc, Enter, Tab and Ctrl+A are handled on the key down: their
            // characters must not beep or type.
            let ch = (wparam.0 & 0xFFFF) as u16;
            if matches!(ch, 0x1B | 0x0D | 0x09 | 0x01) {
                return LRESULT(0);
            }
            let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            edit_key(EditKey::Changed);
            return r;
        }
        WM_PASTE | WM_CUT | WM_CLEAR | WM_UNDO | WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION
        | WM_IME_ENDCOMPOSITION => {
            let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            edit_key(EditKey::Changed);
            return r;
        }
        WM_SETFOCUS => {
            let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            edit_key(EditKey::Focus(true));
            return r;
        }
        WM_KILLFOCUS => {
            let r = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            edit_key(EditKey::Focus(false));
            return r;
        }
        WM_MOUSEWHEEL => {
            edit_key(EditKey::Wheel(((wparam.0 >> 16) & 0xFFFF) as i16 as i32));
            return LRESULT(0);
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
    use crate::widgets::sticky::Sticky;
    use windows::Win32::UI::WindowsAndMessaging::SetWindowTextW;

    fn note(id: &str, text: &str, updated: i64) -> Note {
        Note { id: id.into(), text: text.into(), updated, sticky: Some(Sticky::default()) }
    }

    fn data(notes: Vec<Note>) -> ListData {
        ListData { notes, lang: Lang::Id, ui: UiFont::Sans, can_add: true, can_undo: false }
    }

    const WORK: Px = Px::new(0, 0, 1920, 1040);

    fn owner() -> HWND {
        HWND::default()
    }

    /// One test at a time may use the list's thread-local state on its own
    /// thread; each test thread has its own, so no lock is needed. The state
    /// is cleaned at the end of each test.
    fn finish() {
        close();
        drain();
        assert!(!is_open());
    }

    fn type_query(q: &str) {
        let e = with(|l| l.edit).unwrap();
        unsafe {
            let _ = SetWindowTextW(e, &HSTRING::from(q));
        }
        // Setting the text sends EN_CHANGE to the list; read it again anyway.
        edit_key(EditKey::Changed);
    }

    fn shown_ids() -> Vec<String> {
        with(|l| l.shown.iter().map(|r| r.id.clone()).collect()).unwrap()
    }

    #[test]
    fn a_second_open_moves_the_one_window_and_never_makes_another() {
        open(owner(), Anchor::Default { work_px: WORK }, data(vec![note("a", "A", 1)]));
        assert!(is_open());
        let first = with(|l| (l.hwnd, l.origin)).unwrap();
        // Beside a note near the left edge: it moves, the same window.
        open(
            owner(),
            Anchor::Beside { note_px: Px::new(100, 100, 240, 208) },
            data(vec![note("a", "A", 1), note("b", "B", 2)]),
        );
        let second = with(|l| (l.hwnd, l.origin, l.shown.len())).unwrap();
        assert_eq!(second.0, first.0, "the same window");
        assert_ne!(second.1, first.1, "it moved");
        assert_eq!(second.2, 2, "it shows the new data");
        // Exactly one window of the class exists on this thread.
        let mut count = 0usize;
        unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> windows::core::BOOL {
            let mut name = [0u16; 32];
            let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(h, &mut name) } as usize;
            if String::from_utf16_lossy(&name[..n]) == "ValeNoteList" {
                unsafe { *(lp.0 as *mut usize) += 1 };
            }
            true.into()
        }
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::EnumThreadWindows(
                windows::Win32::System::Threading::GetCurrentThreadId(),
                Some(each),
                LPARAM(&mut count as *mut usize as isize),
            );
        }
        assert_eq!(count, 1);
        finish();
    }

    #[test]
    fn the_window_is_a_topmost_toolwindow_popup_without_a_taskbar_button() {
        use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW};
        open(owner(), Anchor::Default { work_px: WORK }, data(vec![]));
        let h = with(|l| l.hwnd).unwrap();
        let ex = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
        let st = unsafe { GetWindowLongPtrW(h, GWL_STYLE) } as u32;
        assert!(ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_TOPMOST.0 != 0 && ex & WS_EX_LAYERED.0 != 0);
        assert!(st & WS_POPUP.0 != 0);
        assert!(!unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(h) }.as_bool(), "tests show nothing");
        finish();
    }

    #[test]
    fn opening_at_the_default_anchor_sits_top_right_and_beside_sits_beside() {
        let spot = spot_from(&Anchor::Default { work_px: WORK }, WORK, 96);
        assert_eq!(spot.size, (340, 460));
        assert_eq!(spot.origin, (1920 - 340 - 24, 24));
        let note = Px::new(200, 300, 240, 208);
        let spot = spot_from(&Anchor::Beside { note_px: note }, WORK, 96);
        assert_eq!(spot.origin, (448, 300));
        // At 150 %: 510 x 690 px, a 12 px gap, a 36 px margin.
        let spot = spot_from(&Anchor::Default { work_px: WORK }, WORK, 144);
        assert_eq!((spot.scale, spot.size), (1.5, (510, 690)));
        assert_eq!(spot.origin, (1920 - 510 - 36, 36));
        let spot = spot_from(&Anchor::Beside { note_px: note }, WORK, 144);
        assert_eq!(spot.origin.0, note.right() + 12);
        // A short screen shrinks the height, and the window stays inside.
        let short = Px::new(0, 0, 1366, 400);
        let spot = spot_from(&Anchor::Default { work_px: short }, short, 96);
        assert!(spot.origin.1 + spot.size.1 <= 400 && spot.size.1 >= 240);
    }

    #[test]
    fn every_sticky_note_has_a_row_and_a_query_filters_resets_the_selection_and_the_scroll() {
        let notes: Vec<Note> = (0..12).map(|i| note(&format!("n{i}"), &format!("Catatan {i}\nisi {i}"), 100 - i)).collect();
        open(owner(), Anchor::Default { work_px: WORK }, data(notes));
        assert_eq!(shown_ids().len(), 12);
        // Pick the third row with the keys and scroll.
        edit_key(EditKey::Down);
        edit_key(EditKey::Down);
        edit_key(EditKey::Down);
        assert_eq!(with(|l| selected_index(l)).unwrap(), Some(2));
        wheel_by(-120 * 3);
        assert!(with(|l| l.scroll).unwrap() > 0.0);
        // Typing filters, clears the selection and goes back to the top.
        type_query("catatan 1");
        assert_eq!(shown_ids(), ["n1", "n10", "n11"]);
        assert_eq!(with(|l| selected_index(l)).unwrap(), None);
        assert_eq!(with(|l| l.scroll).unwrap(), 0.0);
        // Case and blanks do not matter; the body is searched too.
        type_query("  ISI 7 ");
        assert_eq!(shown_ids(), ["n7"]);
        type_query("tidak ada");
        assert!(shown_ids().is_empty());
        type_query("");
        assert_eq!(shown_ids().len(), 12);
        finish();
    }

    #[test]
    fn the_arrow_keys_move_the_selection_and_keep_it_in_view() {
        let notes: Vec<Note> = (0..20).map(|i| note(&format!("n{i:02}"), &format!("T{i}"), 100 - i)).collect();
        open(owner(), Anchor::Default { work_px: WORK }, data(notes));
        edit_key(EditKey::Up);
        assert_eq!(with(|l| selected_index(l)).unwrap(), Some(19), "Up from nothing is the last row");
        let lay = with(|l| layout_of(l)).unwrap();
        let scroll = with(|l| l.scroll).unwrap();
        assert_eq!(scroll, list::max_scroll(&lay, 20), "it scrolled to show it");
        for _ in 0..30 {
            edit_key(EditKey::Up);
        }
        assert_eq!(with(|l| selected_index(l)).unwrap(), Some(0));
        assert_eq!(with(|l| l.scroll).unwrap(), 0.0);
        edit_key(EditKey::PageDown);
        assert!(with(|l| selected_index(l)).unwrap().unwrap() >= 5);
        finish();
    }

    #[test]
    fn enter_opens_the_selected_row_or_the_first_visible_one() {
        open(
            owner(),
            Anchor::Default { work_px: WORK },
            data(vec![note("a", "alfa", 3), note("b", "beta", 2), note("c", "gamma", 1)]),
        );
        drain();
        edit_key(EditKey::Enter);
        assert_eq!(drain(), [ListEvent::Show("a".into())], "nothing picked: the first");
        type_query("gamma");
        edit_key(EditKey::Enter);
        assert_eq!(drain(), [ListEvent::Show("c".into())], "the first visible");
        type_query("");
        edit_key(EditKey::Down);
        edit_key(EditKey::Down);
        edit_key(EditKey::Enter);
        assert_eq!(drain(), [ListEvent::Show("b".into())], "the one picked");
        type_query("zzz");
        edit_key(EditKey::Enter);
        assert!(drain().is_empty(), "no rows, nothing to open");
        finish();
    }

    #[test]
    fn delete_belongs_to_the_edit_unless_a_row_was_picked_with_the_keys() {
        open(
            owner(),
            Anchor::Default { work_px: WORK },
            data(vec![note("a", "alfa", 3), note("b", "beta", 2), note("c", "gamma", 1)]),
        );
        drain();
        // Nothing picked: the EDIT keeps Delete, no event, no strip.
        assert!(!edit_key(EditKey::Delete));
        assert!(drain().is_empty());
        assert!(!with(|l| l.strip).unwrap());
        // A row picked: Delete deletes that row and the strip shows.
        edit_key(EditKey::Down);
        edit_key(EditKey::Down);
        assert!(edit_key(EditKey::Delete));
        assert_eq!(drain(), [ListEvent::Delete("b".into())]);
        assert!(with(|l| l.strip).unwrap(), "the undo strip is up");
        // The pick moved to the row that takes its place.
        assert_eq!(with(|l| l.selected.clone()).unwrap(), Some("c".into()));
        // Typing hands Delete back to the EDIT.
        type_query("a");
        assert!(!edit_key(EditKey::Delete));
        assert!(drain().is_empty());
        finish();
    }

    #[test]
    fn the_undo_strip_shows_on_a_delete_and_hides_when_its_time_is_up_or_it_is_clicked() {
        open(
            owner(),
            Anchor::Default { work_px: WORK },
            ListData { can_undo: true, ..data(vec![note("a", "alfa", 2), note("b", "beta", 1)]) },
        );
        drain();
        edit_key(EditKey::Down);
        edit_key(EditKey::Delete);
        assert!(with(|l| l.strip).unwrap());
        let (h, hit_point) = with(|l| {
            let lay = layout_of(l);
            let b = lay.undo_btn.expect("the strip has its button");
            (l.hwnd, ((b.x + b.w / 2.0) * l.scale, (b.y + b.h / 2.0) * l.scale))
        })
        .unwrap();
        // The strip takes 40 dip off the rows.
        let lay = with(|l| layout_of(l)).unwrap();
        assert_eq!(lay.rows_view.bottom(), lay.h - list::STRIP_H);
        // Urungkan pushes Undo and hides the strip.
        drain();
        left_down(h, hit_point.0 as i32, hit_point.1 as i32);
        assert_eq!(drain(), [ListEvent::Undo]);
        assert!(!with(|l| l.strip).unwrap());
        // A second delete and the five seconds running out: the strip goes,
        // no event is made.
        edit_key(EditKey::Down);
        edit_key(EditKey::Delete);
        assert!(with(|l| l.strip).unwrap());
        drain();
        let _ = with(hide_strip);
        assert!(!with(|l| l.strip).unwrap());
        assert!(drain().is_empty());
        finish();
    }

    #[test]
    fn urungkan_does_nothing_while_the_desktop_is_full() {
        open(
            owner(),
            Anchor::Default { work_px: WORK },
            ListData { can_undo: false, ..data(vec![note("a", "alfa", 2), note("b", "beta", 1)]) },
        );
        drain();
        edit_key(EditKey::Down);
        edit_key(EditKey::Delete);
        drain();
        let (h, at) = with(|l| {
            let b = layout_of(l).undo_btn.unwrap();
            (l.hwnd, ((b.x + 4.0) * l.scale, (b.y + 4.0) * l.scale))
        })
        .unwrap();
        left_down(h, at.0 as i32, at.1 as i32);
        assert!(drain().is_empty(), "greyed: no Undo");
        assert!(with(|l| l.strip).unwrap(), "the strip stays");
        finish();
    }

    #[test]
    fn a_click_on_a_row_shows_it_and_plus_makes_a_note_unless_the_cap_is_reached() {
        open(
            owner(),
            Anchor::Default { work_px: WORK },
            data(vec![note("a", "alfa", 2), note("b", "beta", 1)]),
        );
        drain();
        let (h, row1, plus) = with(|l| {
            let lay = layout_of(l);
            let k = l.scale;
            (
                l.hwnd,
                (100.0 * k, (lay.rows_view.y + ROW_H + 10.0) * k),
                ((lay.plus.x + 4.0) * k, (lay.plus.y + 4.0) * k),
            )
        })
        .unwrap();
        left_down(h, row1.0 as i32, row1.1 as i32);
        assert_eq!(drain(), [ListEvent::Show("b".into())]);
        left_down(h, plus.0 as i32, plus.1 as i32);
        assert_eq!(drain(), [ListEvent::New]);
        // At the cap (+) is greyed: nothing is pushed.
        refresh(ListData { can_add: false, ..data(vec![note("a", "alfa", 2), note("b", "beta", 1)]) });
        left_down(h, plus.0 as i32, plus.1 as i32);
        assert!(drain().is_empty());
        finish();
    }

    #[test]
    fn the_empty_state_has_a_button_that_acts_like_plus() {
        open(owner(), Anchor::Default { work_px: WORK }, data(vec![]));
        drain();
        let (h, at) = with(|l| {
            let b = list::empty_button(&layout_of(l));
            (l.hwnd, ((b.x + b.w / 2.0) * l.scale, (b.y + b.h / 2.0) * l.scale))
        })
        .unwrap();
        left_down(h, at.0 as i32, at.1 as i32);
        assert_eq!(drain(), [ListEvent::New]);
        // With rows the same spot is just the rows' area.
        refresh(data(vec![note("a", "alfa", 1)]));
        left_down(h, at.0 as i32, at.1 as i32);
        assert!(drain().is_empty());
        finish();
    }

    #[test]
    fn a_refresh_follows_the_notes_and_drops_a_selection_that_is_gone() {
        open(
            owner(),
            Anchor::Default { work_px: WORK },
            data(vec![note("a", "alfa", 3), note("b", "beta", 2), note("c", "gamma", 1)]),
        );
        edit_key(EditKey::Down);
        edit_key(EditKey::Down);
        assert_eq!(with(|l| l.selected.clone()).unwrap(), Some("b".into()));
        // A new note on top: the pick stays on "b", now the third row.
        refresh(data(vec![note("z", "zeta", 9), note("a", "alfa", 3), note("b", "beta", 2), note("c", "gamma", 1)]));
        assert_eq!(with(|l| selected_index(l)).unwrap(), Some(2));
        // "b" is gone: no pick.
        refresh(data(vec![note("a", "alfa", 3), note("c", "gamma", 1)]));
        assert_eq!(with(|l| l.selected.clone()).unwrap(), None);
        assert_eq!(shown_ids(), ["a", "c"]);
        // Closed: nothing happens.
        close();
        refresh(data(vec![]));
        restyle();
        assert!(!is_open());
        drain();
    }

    #[test]
    fn closing_pushes_closed_once_and_escape_posts_a_close() {
        open(owner(), Anchor::Default { work_px: WORK }, data(vec![note("a", "alfa", 1)]));
        drain();
        assert!(edit_key(EditKey::Escape), "Esc is the list's");
        // Esc posts WM_CLOSE (not done inside the handler): pump it.
        unsafe {
            let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
            while windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                &mut msg,
                None,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::PM_REMOVE,
            )
            .as_bool()
            {
                windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
            }
        }
        assert!(!is_open());
        assert_eq!(drain(), [ListEvent::Closed]);
        close();
        assert!(drain().is_empty(), "closing a closed list is nothing");
    }

    #[test]
    fn a_deactivation_is_posted_not_handled_inside_and_a_menu_does_not_close_it() {
        open(owner(), Anchor::Default { work_px: WORK }, data(vec![note("a", "alfa", 1)]));
        let h = with(|l| l.hwnd).unwrap();
        // With a native menu up the window is deactivated and stays.
        let _ = with(|l| l.menu_open = true);
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(h, WM_ACTIVATE, Some(WPARAM(WA_INACTIVE as usize)), Some(LPARAM(0)));
        }
        assert!(is_open());
        let _ = with(|l| l.menu_open = false);
        // Deactivated for real: still open inside the handler (it is posted).
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(h, WM_ACTIVATE, Some(WPARAM(WA_INACTIVE as usize)), Some(LPARAM(0)));
        }
        assert!(is_open(), "the close is posted");
        unsafe {
            let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
            while windows::Win32::UI::WindowsAndMessaging::PeekMessageW(
                &mut msg,
                None,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::PM_REMOVE,
            )
            .as_bool()
            {
                windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
            }
        }
        assert!(!is_open());
        drain();
    }

    #[test]
    fn the_header_drags_and_the_rest_is_client() {
        open(owner(), Anchor::Default { work_px: WORK }, data(vec![note("a", "alfa", 1)]));
        let (h, origin, k) = with(|l| (l.hwnd, l.origin, l.scale)).unwrap();
        let ask = |dx: f32, dy: f32| {
            let (sx, sy) = (origin.0 + (dx * k) as i32, origin.1 + (dy * k) as i32);
            let lparam = LPARAM(((sy as isize & 0xFFFF) << 16) | (sx as isize & 0xFFFF));
            unsafe { windows::Win32::UI::WindowsAndMessaging::SendMessageW(h, WM_NCHITTEST, None, Some(lparam)) }.0
        };
        assert_eq!(ask(20.0, 10.0), HTCAPTION, "the header drags");
        assert_eq!(ask(300.0, 24.0), HTCLIENT, "a button");
        assert_eq!(ask(100.0, 60.0), HTCLIENT, "the search box");
        assert_eq!(ask(100.0, 200.0), HTCLIENT, "the rows");
        finish();
    }

    #[test]
    fn the_search_edit_is_transparent_to_the_pointer_and_takes_the_query() {
        open(owner(), Anchor::Default { work_px: WORK }, data(vec![note("a", "alfa", 1)]));
        let e = with(|l| l.edit).unwrap();
        let r = unsafe { windows::Win32::UI::WindowsAndMessaging::SendMessageW(e, WM_NCHITTEST, None, Some(LPARAM(0))) };
        assert_eq!(r.0, HTTRANSPARENT);
        type_query("alfa");
        assert_eq!(with(|l| l.query.clone()).unwrap(), "alfa");
        finish();
    }

    #[test]
    fn the_row_hit_matches_what_is_drawn_after_a_scroll() {
        let notes: Vec<Note> = (0..12).map(|i| note(&format!("n{i:02}"), &format!("T{i}"), 100 - i)).collect();
        open(owner(), Anchor::Default { work_px: WORK }, data(notes));
        wheel_by(-120 * 4);
        let (h, at, scroll) = with(|l| {
            let lay = layout_of(l);
            (l.hwnd, (100.0 * l.scale, (lay.rows_view.y + 10.0) * l.scale), l.scroll)
        })
        .unwrap();
        assert_eq!(scroll, 4.0 * ROW_H);
        drain();
        left_down(h, at.0 as i32, at.1 as i32);
        assert_eq!(drain(), [ListEvent::Show("n04".into())], "the first visible row is the fifth");
        finish();
    }

    #[test]
    fn the_events_post_one_message_until_drained() {
        // The owner is a message-only window that counts WM_APP_WIDGET.
        use windows::Win32::UI::WindowsAndMessaging::{HWND_MESSAGE, PM_REMOVE, PeekMessageW, MSG};
        open(owner(), Anchor::Default { work_px: WORK }, data(vec![note("a", "alfa", 1)]));
        let module = unsafe { GetModuleHandleW(None) }.unwrap();
        let target = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(module.into()),
                None,
            )
        }
        .unwrap();
        OWNER.with(|o| o.set(target.0 as isize));
        drain();
        push(ListEvent::New);
        push(ListEvent::New);
        push(ListEvent::Undo);
        let mut posted = 0;
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(target), WM_APP_WIDGET, WM_APP_WIDGET, PM_REMOVE).as_bool() {
                posted += 1;
            }
        }
        assert_eq!(posted, 1, "one message for three events");
        assert_eq!(drain().len(), 3);
        push(ListEvent::New);
        unsafe {
            let mut msg = MSG::default();
            assert!(PeekMessageW(&mut msg, Some(target), WM_APP_WIDGET, WM_APP_WIDGET, PM_REMOVE).as_bool(), "after a drain it posts again");
            let _ = DestroyWindow(target);
        }
        drain();
        finish();
    }
}
