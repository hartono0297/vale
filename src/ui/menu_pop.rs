//! Vale's own dropdown list, for the places where a native `TrackPopupMenu`
//! would show Windows' look instead of Vale's. A small layered, topmost,
//! frameless popup drawn with Direct2D in the window palette of the current
//! theme (`theme::current_window`: light, warm dark or the Terminal neutral).
//! Rows have a label, an optional second line in the secondary colour and an
//! optional check in the accent colour, or are separators; text that does not
//! fit ends in an ellipsis.
//!
//! It opens under an anchor rect (screen px), as wide as its longest text
//! within `MIN_W`..`MAX_W` dip, inside the work area of the anchor's monitor,
//! above the anchor when there is no room below, and scrolls when neither side
//! holds it. It takes the keyboard (Up and Down skip separators, Home, End,
//! Enter, Esc) and closes on a pick, on Esc, when it loses the activation and
//! on a press outside its plate. One is open at a time; the window exists only
//! while it is open, so it costs nothing at idle.
//!
//! The owner gets `WM_APP_MENU_DONE` exactly once, as the popup goes, with a
//! `Box<Done>` in `lparam` (`take_done`): the id picked, or `None`, and the
//! context the caller handed to `open`. Nothing runs inside the owner's state.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ROUNDED_RECT,
    ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP, IDWriteFactory,
    IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_RETURN,
    VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, IDC_ARROW, IsWindow,
    LoadCursorW, PostMessageW, RegisterClassExW, SetForegroundWindow, SW_SHOW, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Interface, PCWSTR, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::arrange_overlay::{Canvas, take_foreground};
use crate::capture::Rect;
use crate::config::UiFont;
use crate::shot_edit::gfx::{Gfx, color};
use crate::theme;
use crate::ui::fonts::{Face, Role};
use crate::ui::geometry::Rect as FRect;

/// Posted to the owner as the popup goes: `lparam` is a `Box<Done>` the owner
/// takes with `take_done`.
pub const WM_APP_MENU_DONE: u32 = WM_APP + 81;

/// Whether the popup is put on screen: always, except in tests.
const ON_SCREEN: bool = !cfg!(test);

const CLASS: PCWSTR = w!("ValeMenuPop");

/// The plate's narrowest and widest, dip.
pub const MIN_W: f32 = 220.0;
pub const MAX_W: f32 = 380.0;
/// From the plate's edge to the rows.
const PAD: f32 = 6.0;
/// A row with one line, with two lines, and a separator.
const ROW_H: f32 = 32.0;
const ROW2_H: f32 = 46.0;
const SEP_H: f32 = 9.0;
/// The column at the left of every row that holds the check.
const CHECK_W: f32 = 30.0;
/// From the text's end to the row's right edge.
const TEXT_R: f32 = 12.0;
const LABEL_SIZE: f32 = 13.0;
const DETAIL_SIZE: f32 = 11.5;
const RADIUS: f32 = 10.0;
const ROW_RADIUS: f32 = 6.0;
/// The see-through margin round the plate that holds its shadow.
const SHADOW: f32 = 10.0;
/// Between the anchor and the plate.
const GAP: f32 = 4.0;
/// How far one wheel notch scrolls.
const WHEEL_STEP: f32 = ROW_H * 2.0;

/// One line of the list.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Item {
        /// What the owner gets back when this row is picked.
        id: usize,
        label: String,
        /// A second, quieter line under the label.
        detail: Option<String>,
        checked: bool,
    },
    Separator,
}

/// How the popup ended, posted to its owner.
pub struct Done {
    /// The id of the row picked; `None` when it was dismissed.
    pub pick: Option<usize>,
    /// What the caller handed to `open`, back unchanged.
    pub context: Box<dyn Any>,
}

/// The `Done` of a `WM_APP_MENU_DONE`.
///
/// # Safety
/// `lparam` must come from a `WM_APP_MENU_DONE` this module posted, and be
/// taken once.
pub unsafe fn take_done(lparam: LPARAM) -> Option<Box<Done>> {
    let ptr = lparam.0 as *mut Done;
    (!ptr.is_null()).then(|| unsafe { Box::from_raw(ptr) })
}

// ------------------------------------------------------------------ layout

/// The rows laid out in the plate, dip, from the plate's top-left and before
/// any scroll.
#[derive(Clone, Debug, PartialEq)]
struct Layout {
    /// The plate's width.
    w: f32,
    rows: Vec<FRect>,
    /// Every row with the padding above and below.
    content_h: f32,
}

fn row_h(row: &Row) -> f32 {
    match row {
        Row::Item {
            detail: Some(_), ..
        } => ROW2_H,
        Row::Item { .. } => ROW_H,
        Row::Separator => SEP_H,
    }
}

/// The plate's width for text `text_w` dip wide, within `MIN_W`..`MAX_W`.
fn plate_w(text_w: f32) -> f32 {
    (text_w.ceil() + 2.0 * PAD + CHECK_W + TEXT_R).clamp(MIN_W, MAX_W)
}

/// How wide a row's text may be on a plate `w` wide; longer text is cut.
fn text_room(w: f32) -> f32 {
    w - 2.0 * PAD - CHECK_W - TEXT_R
}

/// `rows` stacked down a plate fitted to `text_w`, the widest text.
fn layout(rows: &[Row], text_w: f32) -> Layout {
    let w = plate_w(text_w);
    let mut y = PAD;
    let rects = rows
        .iter()
        .map(|row| {
            let h = row_h(row);
            let r = FRect {
                x: PAD,
                y,
                w: w - 2.0 * PAD,
                h,
            };
            y += h;
            r
        })
        .collect();
    Layout {
        w,
        rows: rects,
        content_h: y + PAD,
    }
}

/// The row a point of the plate (dip, scrolled by `scroll`) is on; never a
/// separator.
fn row_at(l: &Layout, rows: &[Row], scroll: f32, x: f32, y: f32) -> Option<usize> {
    let y = y + scroll;
    l.rows
        .iter()
        .zip(rows)
        .position(|(r, row)| matches!(row, Row::Item { .. }) && r.contains(x, y))
}

/// A key's move through the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nav {
    Up,
    Down,
    Home,
    End,
}

fn nav_of(vk: u16) -> Option<Nav> {
    match vk {
        k if k == VK_UP.0 => Some(Nav::Up),
        k if k == VK_DOWN.0 => Some(Nav::Down),
        k if k == VK_HOME.0 => Some(Nav::Home),
        k if k == VK_END.0 => Some(Nav::End),
        _ => None,
    }
}

/// The row the key lands on from `from` (`None`: nothing lit yet). Separators
/// are skipped and Up and Down wrap round, as in a native menu. `None` when
/// the list has no item.
fn step(rows: &[Row], from: Option<usize>, nav: Nav) -> Option<usize> {
    let items: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r, Row::Item { .. }))
        .map(|(i, _)| i)
        .collect();
    let (first, last) = (*items.first()?, *items.last()?);
    Some(match (nav, from) {
        (Nav::Home, _) | (Nav::Down, None) => first,
        (Nav::End, _) | (Nav::Up, None) => last,
        (Nav::Down, Some(i)) => items.iter().copied().find(|&j| j > i).unwrap_or(first),
        (Nav::Up, Some(i)) => items.iter().rev().copied().find(|&j| j < i).unwrap_or(last),
    })
}

/// The id row `i` hands back, when it is an item.
fn id_of(rows: &[Row], i: usize) -> Option<usize> {
    match rows.get(i)? {
        Row::Item { id, .. } => Some(*id),
        Row::Separator => None,
    }
}

/// Which edge of the anchor the plate lines up with: `Start` under a label
/// (the plate grows to the right), `End` under a button at the right of a row
/// (it grows to the left).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    End,
}

/// Where the plate goes, px: lined up with `anchor`'s `align` edge and `gap`
/// below it, inside `area`. With too little room below it goes above; when
/// neither side holds all of it, it takes the roomier side and is cut to fit
/// (the list scrolls).
fn place(anchor: Rect, align: Align, area: Rect, w: i32, h: i32, gap: i32) -> Rect {
    let w = w.min(area.w);
    let x = match align {
        Align::Start => anchor.x,
        Align::End => anchor.right() - w,
    };
    let x = x.clamp(area.x, (area.right() - w).max(area.x));
    let below_y = (anchor.bottom() + gap).max(area.y);
    let below = area.bottom() - below_y;
    let above_y = (anchor.y - gap).min(area.bottom());
    let above = above_y - area.y;
    if h <= below {
        Rect::new(x, below_y, w, h)
    } else if h <= above {
        Rect::new(x, above_y - h, w, h)
    } else if below >= above {
        Rect::new(x, below_y, w, below.max(0))
    } else {
        Rect::new(x, area.y, w, above.max(0))
    }
}

/// `scroll` kept between the top and the last row's bottom.
fn clamp_scroll(scroll: f32, content_h: f32, view_h: f32) -> f32 {
    scroll.clamp(0.0, (content_h - view_h).max(0.0))
}

/// The scroll that shows all of row `r` in a plate `view_h` tall, moving as
/// little as it can.
fn scroll_to_show(scroll: f32, r: FRect, view_h: f32) -> f32 {
    if r.y - PAD < scroll {
        r.y - PAD
    } else if r.bottom() + PAD > scroll + view_h {
        r.bottom() + PAD - view_h
    } else {
        scroll
    }
}

// ------------------------------------------------------------------ text

/// The UI face and its formats, for measuring and drawing the rows.
pub(crate) struct Text {
    dwrite: IDWriteFactory,
    face: Face,
    formats: RefCell<HashMap<(u32, u16), IDWriteTextFormat>>,
}

impl Text {
    pub(crate) fn new(dwrite: IDWriteFactory, ui: UiFont) -> Text {
        let face = Face::for_role_in(&dwrite, Role::Text, ui);
        Text {
            dwrite,
            face,
            formats: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) fn format(&self, size: f32, weight: u16) -> Result<IDWriteTextFormat> {
        let key = (size.to_bits(), weight);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        let f = self
            .face
            .format(&self.dwrite, DWRITE_FONT_WEIGHT(i32::from(weight)), size)?;
        self.formats.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    /// `s` on one line in a box `w` by `h`, centred down it and cut with an
    /// ellipsis where the box ends.
    pub(crate) fn line(
        &self,
        s: &str,
        size: f32,
        weight: u16,
        w: f32,
        h: f32,
    ) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = s.encode_utf16().collect();
        let format = self.format(size, weight)?;
        unsafe {
            let l = self
                .dwrite
                .CreateTextLayout(&wide, &format, w.max(1.0), h.max(1.0))?;
            l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            let sign = self.dwrite.CreateEllipsisTrimmingSign(&l)?;
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            l.SetTrimming(&trimming, &sign)?;
            Ok(l)
        }
    }

    /// `s` wrapped at word breaks in a box `w` wide, top aligned; its height
    /// is in `GetMetrics`.
    pub(crate) fn block(
        &self,
        s: &str,
        size: f32,
        weight: u16,
        w: f32,
    ) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = s.encode_utf16().collect();
        let format = self.format(size, weight)?;
        unsafe {
            let l = self
                .dwrite
                .CreateTextLayout(&wide, &format, w.max(1.0), 10_000.0)?;
            l.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
            l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            Ok(l)
        }
    }

    /// How wide `s` is, uncut.
    pub(crate) fn width(&self, s: &str, size: f32, weight: u16) -> f32 {
        let Ok(l) = self.line(s, size, weight, 100_000.0, 100.0) else {
            return 0.0;
        };
        let mut m = DWRITE_TEXT_METRICS::default();
        match unsafe { l.GetMetrics(&mut m) } {
            Ok(()) => m.widthIncludingTrailingWhitespace,
            Err(_) => 0.0,
        }
    }

    /// The widest label or second line of `rows`.
    fn widest(&self, rows: &[Row]) -> f32 {
        rows.iter()
            .map(|row| match row {
                Row::Item {
                    label,
                    detail,
                    checked,
                    ..
                } => {
                    let label = self.width(label, LABEL_SIZE, label_weight(*checked));
                    let detail = detail
                        .as_deref()
                        .map_or(0.0, |d| self.width(d, DETAIL_SIZE, 400));
                    label.max(detail)
                }
                Row::Separator => 0.0,
            })
            .fold(0.0, f32::max)
    }
}

/// The checked row's label stands out a little.
fn label_weight(checked: bool) -> u16 {
    if checked { 600 } else { 400 }
}

// ------------------------------------------------------------------ window

struct Pop {
    hwnd: HWND,
    owner: HWND,
    /// The window that had the activation before the dropdown took it.
    prev: HWND,
    /// The window's top-left, px (the plate sits `SHADOW` dip inside it).
    origin: (i32, i32),
    scale: f32,
    rows: Vec<Row>,
    layout: Layout,
    /// The plate's height on screen, dip (shorter than `content_h` when cut).
    view_h: f32,
    scroll: f32,
    /// The lit row, from the pointer or the keys.
    hot: Option<usize>,
    /// The left button went down on the plate (a release elsewhere is not a
    /// pick).
    pressed: bool,
    tracking: bool,
    pick: Option<usize>,
    context: Option<Box<dyn Any>>,
    text: Text,
    gfx: Rc<Gfx>,
    canvas: Option<Canvas>,
}

thread_local! {
    static POP: RefCell<Option<Pop>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Pop) -> R) -> Option<R> {
    POP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

/// Whether the open dropdown belongs to `owner`.
pub fn is_open_for(owner: HWND) -> bool {
    reap();
    POP.with(|cell| {
        cell.try_borrow()
            .map(|p| p.as_ref().is_some_and(|p| p.owner == owner))
            .unwrap_or(false)
    })
}

/// Whether a dropdown is open, whoever owns it.
pub fn is_open() -> bool {
    reap();
    POP.with(|cell| cell.try_borrow().map(|p| p.is_some()).unwrap_or(false))
}

/// An entry whose window is gone without its `WM_DESTROY` taking it (the
/// entry was borrowed then) is dropped here and its owner told, so no
/// dropdown stays "open" for good.
fn reap() {
    let dead = POP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        let alive = guard
            .as_ref()
            .is_some_and(|p| unsafe { IsWindow(Some(p.hwnd)) }.as_bool());
        if alive { None } else { guard.take() }
    });
    if let Some(p) = dead {
        tell_owner(p);
    }
}

/// Posts the gone dropdown's `Done` to its owner, once. An owner that is no
/// longer a window never reads it: it is dropped here instead of leaking.
fn tell_owner(mut p: Pop) {
    let done = Box::new(Done {
        pick: p.pick,
        context: p.context.take().unwrap_or_else(|| Box::new(())),
    });
    if !unsafe { IsWindow(Some(p.owner)) }.as_bool() {
        return;
    }
    let ptr = Box::into_raw(done);
    let posted = unsafe {
        PostMessageW(
            Some(p.owner),
            WM_APP_MENU_DONE,
            WPARAM(0),
            LPARAM(ptr as isize),
        )
    };
    if posted.is_err() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

/// The activation goes back to the window that had it before the dropdown
/// opened only when the dropdown still has it (a pick, Esc, a press on the
/// island) and that window is still there; when another window took it
/// (which closed the dropdown) it stays there.
fn gives_back(dropdown_has_it: bool, prev_alive: bool) -> bool {
    dropdown_has_it && prev_alive
}

fn register() -> bool {
    register_class(CLASS, pop_proc)
}

/// Registers the window class `class` with `proc` and the arrow cursor; true
/// when it is registered, also when it already was.
pub(crate) fn register_class(
    class: PCWSTR,
    proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(proc),
        hInstance: module.into(),
        hCursor: cursor,
        lpszClassName: class,
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

/// The work area (px) and DPI scale of the monitor under `pt`.
pub(crate) fn monitor_at(pt: (i32, i32)) -> (Rect, f32) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: pt.0, y: pt.1 }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let r = info.rcWork;
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        (
            Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top),
            dx.max(96) as f32 / 96.0,
        )
    }
}

/// Opens the dropdown with `rows` under `anchor` (screen px, lined up with its
/// `align` edge) for `owner`, in
/// Vale's UI font `ui`. Any dropdown already open closes first. `context`
/// comes back in the `Done` posted to `owner`. Returns whether it opened; when
/// it did not, nothing is posted.
pub fn open(
    owner: HWND,
    anchor: Rect,
    align: Align,
    rows: Vec<Row>,
    ui: UiFont,
    context: Box<dyn Any>,
) -> bool {
    close();
    if rows.is_empty() || !register() {
        return false;
    }
    let Ok(gfx) = Gfx::new() else { return false };
    let gfx = Rc::new(gfx);
    let text = Text::new(gfx.dwrite.clone(), ui);
    let layout = layout(&rows, text.widest(&rows));
    let (area, scale) = monitor_at((anchor.x, anchor.bottom()));
    let px = |dip: f32| (dip * scale).round() as i32;
    let plate = place(
        anchor,
        align,
        area,
        px(layout.w),
        px(layout.content_h),
        px(GAP),
    );
    let margin = px(SHADOW);
    let origin = (plate.x - margin, plate.y - margin);
    let size = (plate.w + 2 * margin, plate.h + 2 * margin);
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS,
            w!("Vale menu"),
            WS_POPUP,
            origin.0,
            origin.1,
            size.0,
            size.1,
            Some(owner),
            None,
            Some(module.into()),
            None,
        )
    };
    let Ok(hwnd) = hwnd else { return false };
    let prev = unsafe { GetForegroundWindow() };
    let canvas = Canvas::with_gfx(gfx.clone(), size).ok();
    let view_h = plate.h as f32 / scale;
    // Opened on the current choice: it is in sight even in a cut list.
    let checked = rows
        .iter()
        .position(|r| matches!(r, Row::Item { checked: true, .. }));
    let scroll = checked.map_or(0.0, |i| {
        clamp_scroll(
            scroll_to_show(0.0, layout.rows[i], view_h),
            layout.content_h,
            view_h,
        )
    });
    POP.with(|cell| {
        *cell.borrow_mut() = Some(Pop {
            hwnd,
            owner,
            prev,
            origin,
            scale,
            rows,
            layout,
            view_h,
            scroll,
            hot: None,
            pressed: false,
            tracking: false,
            pick: None,
            context: Some(context),
            text,
            gfx,
            canvas,
        });
    });
    redraw();
    if ON_SCREEN {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        take_foreground(hwnd);
    }
    true
}

/// Closes the open dropdown, if any; its owner gets `Done` with no pick.
/// The activation goes back where it was (`gives_back`).
pub fn close() {
    reap();
    let open = POP.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|p| p.as_ref().map(|p| (p.hwnd, p.prev)))
    });
    if let Some((h, prev)) = open {
        unsafe {
            let has_it = GetForegroundWindow() == h;
            let _ = DestroyWindow(h);
            if gives_back(has_it, IsWindow(Some(prev)).as_bool()) {
                let _ = SetForegroundWindow(prev);
            }
        }
    }
}

/// Picks row `i` (an item) and closes.
fn pick(i: usize) {
    let picked = with(|p| {
        p.pick = id_of(&p.rows, i);
        p.pick.is_some()
    })
    .unwrap_or(false);
    if picked {
        close();
    }
}

fn redraw() {
    let _ = with(|p| {
        let Some(canvas) = &p.canvas else { return };
        let pal = theme::current_window();
        let k = p.scale;
        let (w, view_h) = (p.layout.w, p.view_h);
        let _ = canvas.paint_with(p.hwnd, p.origin, 0.0, 255, |c| {
            let (rt, brush, _) = c.parts();
            let Ok(rt) = rt.cast::<ID2D1RenderTarget>() else {
                return;
            };
            unsafe { rt.SetTransform(&Matrix3x2::scale(k, k)) };
            let plate = FRect {
                x: SHADOW,
                y: SHADOW,
                w,
                h: view_h,
            };
            paint_plate(&rt, brush, plate, RADIUS, &pal);
            unsafe {
                rt.PushAxisAlignedClip(
                    &D2D_RECT_F {
                        left: plate.x,
                        top: plate.y + 1.0,
                        right: plate.x + plate.w,
                        bottom: plate.y + plate.h - 1.0,
                    },
                    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                );
                rt.SetTransform(
                    &(Matrix3x2::translation(SHADOW, SHADOW - p.scroll) * Matrix3x2::scale(k, k)),
                );
            }
            for (i, (row, r)) in p.rows.iter().zip(&p.layout.rows).enumerate() {
                if r.bottom() < p.scroll || r.y > p.scroll + view_h {
                    continue;
                }
                paint_row(&rt, brush, &p.text, &p.gfx, row, *r, p.hot == Some(i), &pal);
            }
            unsafe {
                rt.PopAxisAlignedClip();
                rt.SetTransform(&Matrix3x2::identity());
            }
        });
    });
}

/// A popup plate: a soft shadow of rings growing out from under it, the
/// `popover` fill and a `border` hairline. Shared with the tooltip.
pub(crate) fn paint_plate(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    plate: FRect,
    radius: f32,
    pal: &theme::WindowPalette,
) {
    let set = |rgb: u32, a: f32| unsafe { brush.SetColor(&color(rgb, a)) };
    let shadow = if pal.light { 0.035 } else { 0.10 };
    for i in 1..=4 {
        let g = i as f32 * 2.0;
        let r = FRect {
            x: plate.x - g / 2.0,
            y: plate.y - g / 2.0 + 2.0,
            w: plate.w + g,
            h: plate.h + g,
        };
        set(0x000000, shadow);
        unsafe { rt.FillRoundedRectangle(&rounded(r, radius + g / 2.0), brush) };
    }
    set(pal.popover, 1.0);
    unsafe { rt.FillRoundedRectangle(&rounded(plate, radius), brush) };
    let hair = FRect {
        x: plate.x + 0.5,
        y: plate.y + 0.5,
        w: plate.w - 1.0,
        h: plate.h - 1.0,
    };
    set(pal.border >> 8, (pal.border & 0xFF) as f32 / 255.0 * 2.0);
    unsafe { rt.DrawRoundedRectangle(&rounded(hair, radius - 0.5), brush, 1.0, None) };
}

pub(crate) fn rounded(r: FRect, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: r.x,
            top: r.y,
            right: r.x + r.w,
            bottom: r.y + r.h,
        },
        radiusX: radius,
        radiusY: radius,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_row(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    text: &Text,
    gfx: &Gfx,
    row: &Row,
    r: FRect,
    hot: bool,
    pal: &theme::WindowPalette,
) {
    let set = |rgb: u32, a: f32| unsafe { brush.SetColor(&color(rgb, a)) };
    let Row::Item {
        label,
        detail,
        checked,
        ..
    } = row
    else {
        let y = (r.y + r.h / 2.0).round() + 0.5;
        set(pal.border >> 8, (pal.border & 0xFF) as f32 / 255.0 * 2.0);
        unsafe {
            rt.DrawLine(
                Vector2::new(r.x + 8.0, y),
                Vector2::new(r.right() - 8.0, y),
                brush,
                1.0,
                None,
            );
        }
        return;
    };
    if hot {
        set(pal.hover, 1.0);
        unsafe { rt.FillRoundedRectangle(&rounded(r, ROW_RADIUS), brush) };
    }
    if *checked {
        let (cx, cy) = (r.x + CHECK_W / 2.0 + 1.0, r.y + r.h / 2.0);
        set(pal.accent, 1.0);
        unsafe {
            rt.DrawLine(
                Vector2::new(cx - 5.0, cy + 0.5),
                Vector2::new(cx - 1.5, cy + 4.0),
                brush,
                1.8,
                &gfx.round,
            );
            rt.DrawLine(
                Vector2::new(cx - 1.5, cy + 4.0),
                Vector2::new(cx + 5.5, cy - 4.0),
                brush,
                1.8,
                &gfx.round,
            );
        }
    }
    let x = r.x + CHECK_W;
    let room = text_room(r.w + 2.0 * PAD);
    let draw = |s: &str, size: f32, weight: u16, y: f32, h: f32, ink: u32| {
        if let Ok(l) = text.line(s, size, weight, room, h) {
            set(ink, 1.0);
            unsafe {
                rt.DrawTextLayout(Vector2::new(x, y), &l, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP)
            };
        }
    };
    match detail {
        Some(d) => {
            draw(
                label,
                LABEL_SIZE,
                label_weight(*checked),
                r.y + 6.0,
                19.0,
                pal.text,
            );
            draw(d, DETAIL_SIZE, 400, r.y + 24.0, 16.0, pal.secondary);
        }
        None => draw(
            label,
            LABEL_SIZE,
            label_weight(*checked),
            r.y,
            r.h,
            pal.text,
        ),
    }
}

/// A mouse message's point on the plate, dip (negative or past the plate in
/// the shadow margin).
fn plate_point(p: &Pop, lparam: LPARAM) -> (f32, f32) {
    let x = (lparam.0 & 0xFFFF) as i16 as f32 / p.scale - SHADOW;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / p.scale - SHADOW;
    (x, y)
}

fn on_plate(p: &Pop, x: f32, y: f32) -> bool {
    x >= 0.0 && x < p.layout.w && y >= 0.0 && y < p.view_h
}

unsafe extern "system" fn pop_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEMOVE => {
            let changed = with(|p| {
                if !p.tracking {
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    p.tracking = unsafe { TrackMouseEvent(&mut track) }.is_ok();
                }
                let (x, y) = plate_point(p, lparam);
                let hot = if on_plate(p, x, y) {
                    row_at(&p.layout, &p.rows, p.scroll, x, y)
                } else {
                    None
                };
                std::mem::replace(&mut p.hot, hot) != hot
            })
            .unwrap_or(false);
            if changed {
                redraw();
            }
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            let changed = with(|p| {
                p.tracking = false;
                p.hot.take().is_some()
            })
            .unwrap_or(false);
            if changed {
                redraw();
            }
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let inside = with(|p| {
                let (x, y) = plate_point(p, lparam);
                p.pressed = on_plate(p, x, y);
                p.pressed
            })
            .unwrap_or(false);
            // The shadow margin is outside: a press there closes, as one
            // anywhere else does.
            if !inside {
                close();
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let row = with(|p| {
                let (x, y) = plate_point(p, lparam);
                let pressed = std::mem::take(&mut p.pressed);
                (pressed && on_plate(p, x, y))
                    .then(|| row_at(&p.layout, &p.rows, p.scroll, x, y))
                    .flatten()
            })
            .flatten();
            if let Some(i) = row {
                pick(i);
            }
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            let delta = f32::from((wparam.0 >> 16) as u16 as i16) / 120.0;
            let changed = with(|p| {
                let to = clamp_scroll(p.scroll - delta * WHEEL_STEP, p.layout.content_h, p.view_h);
                std::mem::replace(&mut p.scroll, to) != to
            })
            .unwrap_or(false);
            if changed {
                redraw();
            }
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            let vk = (wparam.0 & 0xFFFF) as u16;
            if vk == VK_ESCAPE.0 {
                close();
            } else if vk == VK_RETURN.0 {
                if let Some(i) = with(|p| p.hot).flatten() {
                    pick(i);
                }
            } else if let Some(nav) = nav_of(vk) {
                let _ = with(|p| {
                    p.hot = step(&p.rows, p.hot, nav);
                    if let Some(i) = p.hot {
                        let to = scroll_to_show(p.scroll, p.layout.rows[i], p.view_h);
                        p.scroll = clamp_scroll(to, p.layout.content_h, p.view_h);
                    }
                });
                redraw();
            }
            return LRESULT(0);
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                close();
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            // Borrowed now (`reap` takes it later), or already reaped.
            let gone = POP.with(|cell| {
                let mut guard = cell.try_borrow_mut().ok()?;
                guard.take_if(|p| p.hwnd == hwnd)
            });
            if let Some(p) = gone {
                tell_owner(p);
            }
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: usize, label: &str, checked: bool) -> Row {
        Row::Item {
            id,
            label: label.into(),
            detail: None,
            checked,
        }
    }

    fn rows() -> Vec<Row> {
        vec![
            Row::Item {
                id: 1,
                label: "Ikuti Windows".into(),
                detail: Some("Mikrofon (Realtek)".into()),
                checked: true,
            },
            Row::Separator,
            item(2, "Headset (USB)", false),
            item(3, "Mic B", false),
        ]
    }

    #[test]
    fn rows_stack_down_the_plate_without_gaps_and_a_detail_makes_a_row_taller() {
        let l = layout(&rows(), 100.0);
        assert_eq!(l.rows.len(), 4);
        assert_eq!(l.rows[0].y, PAD);
        assert_eq!(l.rows[0].h, ROW2_H);
        assert_eq!(l.rows[1].h, SEP_H);
        assert_eq!(l.rows[2].h, ROW_H);
        for pair in l.rows.windows(2) {
            assert_eq!(pair[0].bottom(), pair[1].y);
        }
        assert_eq!(l.content_h, l.rows[3].bottom() + PAD);
        for r in &l.rows {
            assert_eq!(r.x, PAD);
            assert_eq!(r.right(), l.w - PAD);
        }
    }

    #[test]
    fn the_plate_is_as_wide_as_its_text_within_the_limits() {
        assert_eq!(plate_w(0.0), MIN_W);
        assert_eq!(plate_w(10_000.0), MAX_W);
        let mid = 250.0;
        let w = plate_w(mid);
        assert!(w > MIN_W && w < MAX_W);
        assert_eq!(text_room(w), mid);
        // Room for the text never runs past the plate.
        assert!(text_room(MAX_W) < MAX_W - CHECK_W);
    }

    #[test]
    fn a_point_on_a_row_is_that_row_and_never_a_separator() {
        let rows = rows();
        let l = layout(&rows, 100.0);
        let mid = |r: FRect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (x, y) = mid(l.rows[0]);
        assert_eq!(row_at(&l, &rows, 0.0, x, y), Some(0));
        let (x, y) = mid(l.rows[1]);
        assert_eq!(row_at(&l, &rows, 0.0, x, y), None, "the separator");
        let (x, y) = mid(l.rows[3]);
        assert_eq!(row_at(&l, &rows, 0.0, x, y), Some(3));
        // In the padding above the first row: nothing.
        assert_eq!(row_at(&l, &rows, 0.0, x, 2.0), None);
        // Scrolled by one row's height, the same point is the next row down.
        let (x, y) = mid(l.rows[2]);
        assert_eq!(row_at(&l, &rows, ROW_H, x, y), Some(3));
    }

    #[test]
    fn the_keys_walk_the_items_and_skip_separators() {
        let rows = rows();
        assert_eq!(step(&rows, None, Nav::Down), Some(0));
        assert_eq!(step(&rows, None, Nav::Up), Some(3));
        assert_eq!(
            step(&rows, Some(0), Nav::Down),
            Some(2),
            "over the separator"
        );
        assert_eq!(step(&rows, Some(2), Nav::Up), Some(0), "over the separator");
        assert_eq!(step(&rows, Some(3), Nav::Down), Some(0), "wraps to the top");
        assert_eq!(
            step(&rows, Some(0), Nav::Up),
            Some(3),
            "wraps to the bottom"
        );
        assert_eq!(step(&rows, Some(2), Nav::Home), Some(0));
        assert_eq!(step(&rows, Some(0), Nav::End), Some(3));
        // Separators at the ends are skipped too.
        let edged = vec![Row::Separator, item(7, "A", false), Row::Separator];
        assert_eq!(step(&edged, None, Nav::Home), Some(1));
        assert_eq!(step(&edged, None, Nav::End), Some(1));
        assert_eq!(step(&edged, Some(1), Nav::Down), Some(1));
        // Nothing to light.
        assert_eq!(step(&[Row::Separator], None, Nav::Down), None);
        assert_eq!(step(&[], None, Nav::Home), None);
        assert_eq!(nav_of(VK_DOWN.0), Some(Nav::Down));
        assert_eq!(nav_of(VK_HOME.0), Some(Nav::Home));
        assert_eq!(nav_of(VK_RETURN.0), None);
    }

    #[test]
    fn a_pick_hands_back_the_rows_id() {
        let rows = rows();
        assert_eq!(id_of(&rows, 0), Some(1));
        assert_eq!(id_of(&rows, 1), None, "a separator is no pick");
        assert_eq!(id_of(&rows, 3), Some(3));
        assert_eq!(id_of(&rows, 9), None);
    }

    #[test]
    fn the_plate_goes_under_the_anchor_inside_the_work_area() {
        let area = Rect::new(0, 0, 1920, 1040);
        let anchor = Rect::new(500, 100, 120, 24);
        assert_eq!(
            place(anchor, Align::Start, area, 300, 200, 6),
            Rect::new(500, 130, 300, 200)
        );
        // Near the right edge it slides left.
        let anchor = Rect::new(1800, 100, 100, 24);
        assert_eq!(place(anchor, Align::Start, area, 300, 200, 6).x, 1620);
        // Near the left edge of a second monitor it stays on that monitor.
        let left = Rect::new(-1280, 0, 1280, 1000);
        assert_eq!(
            place(Rect::new(-1300, 50, 40, 20), Align::Start, left, 300, 200, 6).x,
            -1280
        );
    }

    #[test]
    fn under_a_button_at_the_right_it_lines_up_with_the_buttons_right_edge() {
        let area = Rect::new(0, 0, 1920, 1040);
        let ganti = Rect::new(1200, 300, 80, 28);
        assert_eq!(
            place(ganti, Align::End, area, 300, 200, 6),
            Rect::new(980, 334, 300, 200)
        );
        // Near the left edge it slides right, onto the work area.
        let ganti = Rect::new(40, 300, 80, 28);
        assert_eq!(place(ganti, Align::End, area, 300, 200, 6).x, 0);
    }

    #[test]
    fn with_no_room_below_it_flips_above_and_when_neither_side_holds_it_it_is_cut() {
        let area = Rect::new(0, 0, 1920, 1040);
        // Low on the screen: above the anchor, its bottom `gap` over it.
        let anchor = Rect::new(500, 950, 120, 24);
        assert_eq!(
            place(anchor, Align::Start, area, 300, 200, 6),
            Rect::new(500, 744, 300, 200)
        );
        // Too tall for either side: the roomier side, cut to its room.
        let short = Rect::new(0, 0, 1920, 400);
        let anchor = Rect::new(500, 300, 120, 24);
        let r = place(anchor, Align::Start, short, 300, 600, 6);
        assert_eq!(r, Rect::new(500, 0, 300, 294));
        let anchor = Rect::new(500, 60, 120, 24);
        let r = place(anchor, Align::Start, short, 300, 600, 6);
        assert_eq!(r, Rect::new(500, 90, 300, 310));
        assert!(r.bottom() <= short.bottom());
        // Wider than the area: as wide as the area.
        let narrow = Rect::new(0, 0, 200, 1000);
        assert_eq!(place(Rect::new(10, 10, 20, 20), Align::Start, narrow, 300, 100, 6).w, 200);
    }

    #[test]
    fn a_cut_list_scrolls_only_as_far_as_its_rows_and_keeps_the_lit_one_in_sight() {
        assert_eq!(clamp_scroll(-5.0, 400.0, 200.0), 0.0);
        assert_eq!(clamp_scroll(500.0, 400.0, 200.0), 200.0);
        assert_eq!(
            clamp_scroll(50.0, 100.0, 200.0),
            0.0,
            "a list that fits never scrolls"
        );
        let r = FRect {
            x: PAD,
            y: 300.0,
            w: 200.0,
            h: ROW_H,
        };
        // Below the view: scrolled until its bottom shows.
        assert_eq!(scroll_to_show(0.0, r, 200.0), r.bottom() + PAD - 200.0);
        // Above it: scrolled up to its top.
        assert_eq!(scroll_to_show(400.0, r, 200.0), r.y - PAD);
        // In sight: no move.
        assert_eq!(scroll_to_show(200.0, r, 200.0), 200.0);
    }

    #[test]
    fn a_long_label_is_cut_with_an_ellipsis_inside_its_room() {
        let gfx = Gfx::new().expect("DirectWrite works without a window");
        let text = Text::new(gfx.dwrite.clone(), UiFont::Sans);
        let long = "Microphone Array (Intel® Smart Sound Technology for Digital Microphones)";
        let rows = vec![item(1, long, false)];
        let wide = text.widest(&rows);
        let l = layout(&rows, wide);
        assert!(
            wide > text_room(MAX_W),
            "the label is longer than the widest plate"
        );
        assert_eq!(l.w, MAX_W);
        let room = text_room(l.w);
        let cut = text
            .line(long, LABEL_SIZE, 400, room, ROW_H)
            .expect("layout");
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { cut.GetMetrics(&mut m) }.expect("metrics");
        assert!(m.width <= room + 0.5, "{} in {room}", m.width);
        // A short label fits whole on the narrowest plate.
        let short = vec![item(1, "Mic B", true)];
        assert_eq!(layout(&short, text.widest(&short)).w, MIN_W);
        assert!(text.widest(&short) < text_room(MIN_W));
    }

    #[test]
    fn the_activation_goes_back_only_from_a_dropdown_that_still_has_it() {
        // Picked, Esc, a press on the island: back to the window before.
        assert!(gives_back(true, true));
        // Another window took the activation (that closed it): it stays there.
        assert!(!gives_back(false, true));
        // The window before is gone.
        assert!(!gives_back(true, false));
    }

    #[test]
    fn an_entry_whose_window_died_unseen_is_dropped_and_its_done_not_leaked() {
        // No owner window: the `Done` has nowhere to go and is dropped.
        let owner = HWND::default();
        let anchor = Rect::new(100, 100, 120, 24);
        assert!(open(owner, anchor, Align::Start, rows(), UiFont::Sans, Box::new(7u8)));
        assert!(is_open());
        let hwnd = POP.with(|c| c.borrow().as_ref().map(|p| p.hwnd)).expect("open");
        // Destroyed while the entry is borrowed: its WM_DESTROY cannot take it.
        POP.with(|c| {
            let _held = c.borrow_mut();
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        });
        assert!(POP.with(|c| c.borrow().is_some()), "the dead entry stayed");
        assert!(!is_open_for(owner), "a dead window is not an open dropdown");
        assert!(POP.with(|c| c.borrow().is_none()), "and the entry is gone");
        assert!(!is_open());
        // `close` on nothing is fine.
        close();
    }
}
