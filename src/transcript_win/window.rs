//! The transcript windows: `ValeTranscript` top-level windows
//! (`WS_OVERLAPPEDWINDOW`, in the taskbar), one per recording, drawn with
//! Direct2D on a software `ID2D1HwndRenderTarget` (as Settings is) in
//! `theme::current_window()` colours, the title bar coloured through DWM.
//!
//! Thread-local state on the UI thread; the window procedure works on it
//! through `with_win` and never calls into the island: a request is queued
//! and `WM_APP_TRANSCRIPT` is posted to the owner once per batch.
//!
//! The list measures each line's wrapped height once per text width (a new
//! line of a running job is measured when it arrives) and lays out only the
//! rows on screen when it draws.
//!
//! Each window owns its player (`Playback`; a fake in the tests). While it
//! plays, timer 1 (a 200 ms one-shot, re-armed) moves the position, marks the
//! line and keeps it in view; the wheel or a key that moves the list holds
//! that for 3 s (timer 2, re-armed per notch). `WM_APP_PLAYER` says the end
//! (back to 0) or a failure (`Status::PlayerError`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE,
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWRITE_WORD_WRAPPING_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
    IDWriteTextLayout, IDWriteTypography,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTOPRIMARY,
    MONITORINFO, MonitorFromWindow, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN,
    VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    GetWindowRect, IDC_ARROW, IDC_HAND, IsIconic, IsZoomed, KillTimer, LoadCursorW, MINMAXINFO,
    PostMessageW, RegisterClassExW, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SetCursor,
    SetTimer, SetWindowPos, SetWindowTextW, ShowWindow, WINDOW_EX_STYLE, WM_CAPTURECHANGED,
    WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ENTERSIZEMOVE, WM_ERASEBKGND, WM_EXITSIZEMOVE,
    WM_GETMINMAXINFO, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETCURSOR,
    WM_SIZE, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{HSTRING, PCWSTR, Result, w};
use windows_numerics::Vector2;

use super::layout::{
    self, BAR_HIT_H, BTN_PAD_X, Btn, CHIP_COL, Frame, Hot, LINE_PAD_Y, LIST_TOP, META_H, PAD, PLAY,
    ROW_BLEED, TEXT_MIN_H, THUMB, TIME_W, TITLE_H, TRACK_H, WHEEL_STEP,
};
use super::player::{Playback, WM_APP_PLAYER};
use super::{Request, Status, View, WM_APP_TRANSCRIPT};
use crate::arrange_overlay::take_foreground;
use crate::config::{CardPlace, UiFont};
use crate::i18n::{self, Key, Lang, text};
use crate::lyrics_card::layout::Mon;
use crate::shot_edit::gfx::color;
use crate::theme::{self, WindowPalette};
use crate::transcribe::doc;
use crate::transcribe::output::{self, Speaker};
use crate::transcribe::queue::same_audio;
use crate::ui::fonts::{self, Face, Role};
use crate::ui::geometry::Rect;
use crate::ui::icons::{Icon, IconSet};
use crate::ui::settings_controls::{mix, primary_fill};
use crate::ui::settings_window::Rc;
use crate::ui::tooltip::{self, TipSpec, on_screen};

const CLASS: PCWSTR = w!("ValeTranscript");
/// A window is on screen only outside the tests.
const ON_SCREEN: bool = !cfg!(test);

// Text sizes (dip) and weights.
const TITLE_PT: f32 = 17.0;
const META_PT: f32 = 12.5;
const BTN_PT: f32 = 13.0;
const CLOCK_PT: f32 = 12.5;
const TIME_PT: f32 = 12.5;
const CHIP_PT: f32 = 11.5;
const LINE_PT: f32 = 13.5;
const STATUS_PT: f32 = 12.5;
const NORMAL: u16 = 400;
const STRONG: u16 = 600;

const BTN_RADIUS: f32 = 8.0;
const ROW_RADIUS: f32 = 6.0;
const CHIP_H: f32 = 20.0;
const CHIP_PAD_X: f32 = 7.0;
const PLAYING_BAR_W: f32 = 3.0;
const RING_W: f32 = 2.0;
const ICON: f32 = 16.0;
/// The arrow keys move the seek bar's position by this much.
const SEEK_STEP_MS: u64 = 5_000;
/// How far the empty state's text sits below the list's top.
const EMPTY_TOP: f32 = 28.0;

/// While playing: a one-shot every `PLAY_TICK_MS` moves the position and the
/// marked line.
const TIMER_PLAY: usize = 1;
const PLAY_TICK_MS: u32 = 200;
/// While playing, after the wheel or a key moved the list: a one-shot that
/// ends the pause in following the line, re-armed per notch.
const TIMER_HOLD: usize = 2;
const HOLD: Duration = Duration::from_secs(3);

const HTCLIENT: u16 = 1;

// ------------------------------------------------------------------ text

/// DirectWrite in the UI font, with the formats this window draws in. Works
/// without a window, so the tests measure with it.
struct Text {
    dwrite: IDWriteFactory,
    face: Face,
    tabular: IDWriteTypography,
    formats: RefCell<HashMap<(u32, u16, bool), IDWriteTextFormat>>,
}

impl Text {
    fn new(ui: UiFont) -> Result<Text> {
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        Ok(Text {
            face: Face::for_role_in(&dwrite, Role::Text, ui),
            tabular: fonts::tabular_figures(&dwrite)?,
            dwrite,
            formats: RefCell::new(HashMap::new()),
        })
    }

    fn format(&self, size: f32, weight: u16, wrap: bool) -> Result<IDWriteTextFormat> {
        let key = (size.to_bits(), weight, wrap);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        let f = self
            .face
            .format(&self.dwrite, DWRITE_FONT_WEIGHT(i32::from(weight)), size)?;
        unsafe {
            if wrap {
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            } else {
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                let sign = self.dwrite.CreateEllipsisTrimmingSign(&f)?;
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                f.SetTrimming(&trimming, &sign)?;
            }
        }
        self.formats.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    /// `s` in a `w` × `h` box: one line cut with an ellipsis, or wrapped.
    #[allow(clippy::too_many_arguments)]
    fn layout(
        &self,
        s: &str,
        size: f32,
        weight: u16,
        w: f32,
        h: f32,
        wrap: bool,
        align: DWRITE_TEXT_ALIGNMENT,
    ) -> Result<IDWriteTextLayout> {
        let format = self.format(size, weight, wrap)?;
        let wide: Vec<u16> = s.encode_utf16().collect();
        let l = fonts::text_layout(&self.dwrite, &wide, &format, w, h, &self.tabular)?;
        unsafe { l.SetTextAlignment(align)? };
        Ok(l)
    }

    fn metrics(l: &IDWriteTextLayout) -> DWRITE_TEXT_METRICS {
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = unsafe { l.GetMetrics(&mut m) };
        m
    }

    /// One line's width.
    fn width(&self, s: &str, size: f32, weight: u16) -> f32 {
        self.layout(s, size, weight, 10_000.0, 100.0, false, DWRITE_TEXT_ALIGNMENT_LEADING)
            .map_or(0.0, |l| Text::metrics(&l).widthIncludingTrailingWhitespace)
    }

    /// `s` wrapped to `w`: its height.
    fn height(&self, s: &str, size: f32, weight: u16, w: f32) -> f32 {
        self.layout(s, size, weight, w, 100_000.0, true, DWRITE_TEXT_ALIGNMENT_LEADING)
            .map_or(TEXT_MIN_H, |l| Text::metrics(&l).height)
    }

    fn button_w(&self, btn: Btn, lang: Lang) -> f32 {
        (self.width(text(lang, btn.label()), BTN_PT, btn_weight(btn)) + 2.0 * BTN_PAD_X).ceil()
    }
}

fn btn_weight(btn: Btn) -> u16 {
    if btn.primary() { STRONG } else { NORMAL }
}

/// The buttons with the widths they are drawn at, for the layout tests.
#[cfg(test)]
pub(super) fn measured_buttons(ui: UiFont, lang: Lang, btns: &[Btn]) -> Vec<(Btn, f32)> {
    let t = Text::new(ui).expect("DirectWrite");
    btns.iter().map(|&b| (b, t.button_w(b, lang))).collect()
}

// ------------------------------------------------------------------ state

/// What has the keyboard, in Tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Btn(usize),
    Play,
    Bar,
    List,
}

struct Target {
    rt: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
}

struct Win {
    hwnd: HWND,
    view: View,
    text: Text,
    d2d: ID2D1Factory,
    icons: IconSet,
    target: Option<Target>,
    dpi: u32,
    /// The client area, px.
    client: (u32, u32),
    buttons: Vec<(Btn, f32)>,
    frame: Frame,
    /// Each line's height, measured at `heights_w` (the text column's width).
    heights: Vec<f32>,
    heights_w: f32,
    tops: Vec<f32>,
    scroll: f32,
    hover: Option<Hot>,
    pressed: Option<Hot>,
    /// The pointer over the client area (dip), for hover after a scroll.
    cursor: Option<(f32, f32)>,
    focus: Focus,
    /// The focus ring shows (the keyboard was used since the last click).
    ring: bool,
    focus_line: usize,
    /// Where the player is; `None` until a place was picked.
    position: Option<u64>,
    player: Box<dyn Playback>,
    /// The playing line is not followed before this (the list was moved).
    hold_until: Option<Instant>,
    /// The audio could not be played: the status line says so until the next
    /// play that works or the next status from the island.
    failed: bool,
    /// Left/Right on the bar while playing: the position moves per key
    /// repeat, the player restarts there once, when the key is let go.
    key_seek: bool,
    seeking: bool,
    tracking: bool,
    /// Where on the seek bar (dip) the pointer was when its tip was first
    /// reported: the tip stays there while the pointer stays on the bar.
    tip_bar_x: Option<f32>,
    /// The title bar's colours last applied (light, bg, text).
    styled: Option<(bool, u32, u32)>,
}

thread_local! {
    static WINS: RefCell<Vec<Win>> = const { RefCell::new(Vec::new()) };
    static REQUESTS: RefCell<Vec<(PathBuf, Request)>> = const { RefCell::new(Vec::new()) };
    static POSTED: Cell<bool> = const { Cell::new(false) };
    static OWNER: Cell<isize> = const { Cell::new(0) };
    /// Lines measured, for the tests.
    static MEASURED: Cell<usize> = const { Cell::new(0) };
}

/// Queues `req` for `audio` and tells the owner once per batch.
fn push(audio: &Path, req: Request) {
    REQUESTS.with(|r| r.borrow_mut().push((audio.to_path_buf(), req)));
    if !POSTED.with(|p| p.replace(true)) {
        let owner = OWNER.with(Cell::get);
        if owner != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(owner as *mut _)),
                    WM_APP_TRANSCRIPT,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }
}

pub fn take_requests() -> Vec<(PathBuf, Request)> {
    POSTED.with(|p| p.set(false));
    REQUESTS.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

fn with_win<R>(hwnd: HWND, f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    WINS.with(|cell| {
        let mut wins = cell.try_borrow_mut().ok()?;
        wins.iter_mut().find(|w| w.hwnd == hwnd).map(f)
    })
}

fn hwnd_of(audio: &Path) -> Option<HWND> {
    WINS.with(|cell| {
        cell.try_borrow()
            .ok()?
            .iter()
            .find(|w| same_audio(&w.view.doc.audio, audio))
            .map(|w| w.hwnd)
    })
}

pub fn is_open(audio: &Path) -> bool {
    hwnd_of(audio).is_some()
}

pub fn open_audios() -> Vec<PathBuf> {
    WINS.with(|cell| {
        cell.try_borrow()
            .map(|wins| wins.iter().map(|w| w.view.doc.audio.clone()).collect())
            .unwrap_or_default()
    })
}

fn all_hwnds() -> Vec<HWND> {
    WINS.with(|cell| {
        cell.try_borrow()
            .map(|wins| wins.iter().map(|w| w.hwnd).collect())
            .unwrap_or_default()
    })
}

// ------------------------------------------------------------------ open

fn register() -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let (large, small) = crate::ui::app_icon::class_icons(module.into());
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wnd_proc),
        hInstance: module.into(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        hIcon: large,
        hIconSm: small,
        lpszClassName: CLASS,
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&class) };
    atom != 0 || windows::core::Error::from_thread().code() == ERROR_CLASS_ALREADY_EXISTS.to_hresult()
}

/// The monitor a window's place falls back to: the owner's (the island sits
/// on the notch monitor), else the primary.
fn fallback_mon(owner: HWND) -> Mon {
    unsafe {
        let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let rc = |r: RECT| Rc {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        };
        Mon {
            rect: rc(info.rcMonitor),
            work: rc(info.rcWork),
            dpi: dx.max(96),
        }
    }
}

fn monitors() -> Vec<Mon> {
    crate::arrange_win::displays()
        .iter()
        .map(crate::lyrics_card::layout::Mon::of)
        .collect()
}

fn window_rc(hwnd: HWND) -> Option<Rc> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    Some(Rc {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    })
}

pub fn open(owner: HWND, view: View, place: Option<CardPlace>) -> bool {
    OWNER.with(|o| o.set(owner.0 as isize));
    if let Some(hwnd) = hwnd_of(&view.doc.audio) {
        update(&view.doc.audio.clone(), view);
        if ON_SCREEN {
            unsafe {
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
            }
            take_foreground(hwnd);
        }
        return true;
    }
    if !register() {
        return false;
    }
    let fallback = fallback_mon(owner);
    let newest = all_hwnds().last().and_then(|&h| window_rc(h));
    let rc = match newest {
        Some(r) => layout::cascade(r, fallback.work),
        None => layout::place(place, &monitors(), fallback),
    };
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let title = HSTRING::from(view.doc.title.as_str());
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            &title,
            WS_OVERLAPPEDWINDOW,
            if rc.width() > 0 { rc.left } else { CW_USEDEFAULT },
            rc.top,
            rc.width(),
            rc.height(),
            None,
            None,
            Some(module.into()),
            None,
        )
    };
    let Ok(hwnd) = hwnd else { return false };
    let Some(win) = new_win(hwnd, view) else {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return false;
    };
    WINS.with(|cell| cell.borrow_mut().push(win));
    let _ = with_win(hwnd, relayout);
    restyle_one(hwnd);
    if ON_SCREEN {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        take_foreground(hwnd);
    }
    true
}

fn client_px(hwnd: HWND) -> (u32, u32) {
    let mut r = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut r) };
    ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32)
}

fn new_win(hwnd: HWND, view: View) -> Option<Win> {
    let d2d: ID2D1Factory =
        unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }.ok()?;
    let icons = IconSet::new(&d2d).ok()?;
    let text = Text::new(view.font).ok()?;
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    Some(Win {
        hwnd,
        text,
        d2d,
        icons,
        target: None,
        dpi,
        client: client_px(hwnd),
        buttons: Vec::new(),
        frame: layout::frame(layout::MIN_W, layout::MIN_H, &[], false),
        heights: Vec::new(),
        heights_w: 0.0,
        tops: Vec::new(),
        scroll: 0.0,
        hover: None,
        pressed: None,
        cursor: None,
        focus: Focus::List,
        ring: false,
        focus_line: 0,
        position: None,
        player: new_player(&view.doc.audio),
        hold_until: None,
        failed: false,
        key_seek: false,
        seeking: false,
        tracking: false,
        tip_bar_x: None,
        styled: None,
        view,
    })
}

#[cfg(not(test))]
fn new_player(audio: &Path) -> Box<dyn Playback> {
    Box::new(super::player::Player::new(audio.to_path_buf()))
}

#[cfg(test)]
fn new_player(_audio: &Path) -> Box<dyn Playback> {
    Box::new(tests::FakePlayer)
}

// ------------------------------------------------------------------ update

/// The island's status moved to another kind (a new percent of the running
/// job is not one): a player error shown until then gives way.
fn status_kind_changed(old: &Status, new: &Status) -> bool {
    std::mem::discriminant(old) != std::mem::discriminant(new)
}

/// How many leading segments two lists share.
fn common_prefix(a: &[output::Segment], b: &[output::Segment]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

pub fn update(audio: &Path, view: View) {
    let Some(hwnd) = hwnd_of(audio) else { return };
    let retitle = with_win(hwnd, |w| {
        // A list read to its end stays at its end as lines arrive.
        let max = layout::max_scroll(&w.tops, &w.heights, list_h(w));
        let at_end = max > 0.0 && w.scroll >= max - 0.5;
        let keep = common_prefix(&w.view.doc.segments, &view.doc.segments);
        w.heights.truncate(keep);
        if view.font != w.view.font
            && let Ok(t) = Text::new(view.font)
        {
            w.text = t;
            w.heights.clear();
        }
        let retitle = (view.doc.title != w.view.doc.title).then(|| view.doc.title.clone());
        // What the pointer was pressed on, so a press survives an update
        // only while the same control or line sits under it.
        let pressed = w.pressed.map(|hot| target_of(w, hot));
        if status_kind_changed(&w.view.status, &view.status) {
            w.failed = false;
        }
        w.view = view;
        relayout(w);
        if at_end {
            w.scroll = layout::max_scroll(&w.tops, &w.heights, list_h(w));
        }
        w.pressed = w.pressed.filter(|&hot| pressed == Some(target_of(w, hot)));
        rehover(w);
        retitle
    });
    if let Some(Some(title)) = retitle {
        unsafe {
            let _ = SetWindowTextW(hwnd, &HSTRING::from(title.as_str()));
        }
    }
    invalidate(hwnd);
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn scale(w: &Win) -> f32 {
    w.dpi as f32 / 96.0
}

fn size_dip(w: &Win) -> (f32, f32) {
    let k = scale(w);
    (w.client.0 as f32 / k, w.client.1 as f32 / k)
}

fn list_h(w: &Win) -> f32 {
    w.frame.list.h
}

fn has_status_line(s: &Status) -> bool {
    !matches!(s, Status::None | Status::Off)
}

fn text_x(two_channel: bool) -> f32 {
    PAD + TIME_W + if two_channel { CHIP_COL } else { 0.0 }
}

/// Buttons, frame, line heights (only what the width or new lines need) and
/// the scroll range, after a size, view or font change.
fn relayout(w: &mut Win) {
    let (wd, hd) = size_dip(w);
    let has_lines = !w.view.doc.segments.is_empty();
    let lang = w.view.lang;
    w.buttons = layout::buttons_for(&w.view.status, has_lines, w.view.can_redo)
        .into_iter()
        .map(|b| (b, w.text.button_w(b, lang)))
        .collect();
    let status_line = has_status_line(status(w));
    w.frame = layout::frame(wd, hd, &w.buttons, status_line);
    let tw = (wd - PAD - text_x(w.view.doc.two_channel)).max(40.0);
    if (tw - w.heights_w).abs() > 0.01 {
        w.heights.clear();
        w.heights_w = tw;
    }
    let segments = &w.view.doc.segments;
    w.heights.truncate(segments.len());
    for s in &segments[w.heights.len()..] {
        let h = w.text.height(&s.text, LINE_PT, NORMAL, tw).max(TEXT_MIN_H);
        w.heights.push(h + 2.0 * LINE_PAD_Y);
        MEASURED.with(|m| m.set(m.get() + 1));
    }
    w.tops = layout::line_tops(&w.heights);
    let max = layout::max_scroll(&w.tops, &w.heights, list_h(w));
    w.scroll = w.scroll.clamp(0.0, max);
    w.focus_line = w.focus_line.min(segments.len().saturating_sub(1));
    if let Focus::Btn(i) = w.focus
        && i >= w.buttons.len()
    {
        w.focus = Focus::List;
    }
}

// ------------------------------------------------------------------ theme

/// Dark or light title bar, coloured like the window (Settings' way).
fn apply_mode(hwnd: HWND, p: &WindowPalette) {
    let dark = i32::from(!p.light);
    let rgb = |c: u32| ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF);
    let caption = rgb(p.bg);
    let ink = rgb(p.text);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const i32 as *const _,
            size_of::<i32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &caption as *const u32 as *const _,
            size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            &ink as *const u32 as *const _,
            size_of::<u32>() as u32,
        );
    }
}

fn restyle_one(hwnd: HWND) {
    let p = theme::current_window();
    let key = (p.light, p.bg, p.text);
    let changed = with_win(hwnd, |w| w.styled.replace(key) != Some(key)).unwrap_or(false);
    if changed {
        apply_mode(hwnd, &p);
    }
    invalidate(hwnd);
}

pub fn restyle() {
    for hwnd in all_hwnds() {
        restyle_one(hwnd);
    }
}

pub fn close_all() {
    for hwnd in all_hwnds() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

pub fn release() {
    close_all();
    let left = WINS.with(|cell| cell.try_borrow_mut().map(|mut w| std::mem::take(&mut *w)));
    drop(left);
}

// ------------------------------------------------------------------ paint

fn rect_f(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}

fn rr(r: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: rect_f(r),
        radiusX: radius,
        radiusY: radius,
    }
}

fn inflate(r: Rect, d: f32) -> Rect {
    Rect {
        x: r.x - d,
        y: r.y - d,
        w: r.w + 2.0 * d,
        h: r.h + 2.0 * d,
    }
}

/// A `0xRRGGBBAA` palette colour as (rgb, alpha).
fn rgba(c: u32) -> (u32, f32) {
    (c >> 8, (c & 0xFF) as f32 / 255.0)
}

struct Pen<'a> {
    rt: &'a ID2D1RenderTarget,
    brush: &'a ID2D1SolidColorBrush,
    text: &'a Text,
}

impl Pen<'_> {
    fn fill(&self, r: Rect, radius: f32, rgb: u32, a: f32) {
        unsafe {
            self.brush.SetColor(&color(rgb, a));
            self.rt.FillRoundedRectangle(&rr(r, radius), self.brush);
        }
    }

    fn ring(&self, r: Rect, radius: f32, rgb: u32) {
        unsafe {
            self.brush.SetColor(&color(rgb, 1.0));
            self.rt
                .DrawRoundedRectangle(&rr(r, radius), self.brush, RING_W, None);
        }
    }

    fn circle(&self, cx: f32, cy: f32, r: f32, rgb: u32) {
        unsafe {
            self.brush.SetColor(&color(rgb, 1.0));
            let e = D2D1_ELLIPSE {
                point: Vector2::new(cx, cy),
                radiusX: r,
                radiusY: r,
            };
            self.rt.FillEllipse(&e, self.brush);
        }
    }

    fn hline(&self, x0: f32, x1: f32, y: f32, rgba_c: u32) {
        let (rgb, a) = rgba(rgba_c);
        self.fill(Rect { x: x0, y, w: x1 - x0, h: 1.0 }, 0.0, rgb, a);
    }

    #[allow(clippy::too_many_arguments)]
    fn str(
        &self,
        s: &str,
        r: Rect,
        size: f32,
        weight: u16,
        rgb: u32,
        align: DWRITE_TEXT_ALIGNMENT,
        wrap: bool,
    ) {
        if s.is_empty() || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let Ok(l) = self.text.layout(s, size, weight, r.w, r.h, wrap, align) else {
            return;
        };
        unsafe {
            self.brush.SetColor(&color(rgb, 1.0));
            self.rt.DrawTextLayout(
                Vector2::new(r.x, r.y),
                &l,
                self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
        }
    }
}

fn ensure_target(w: &mut Win) -> Option<()> {
    if w.target.is_none() {
        let k = w.dpi as f32;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: k,
            dpiY: k,
            ..Default::default()
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: w.hwnd,
            pixelSize: D2D_SIZE_U {
                width: w.client.0.max(1),
                height: w.client.1.max(1),
            },
            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
        };
        unsafe {
            let rt = w.d2d.CreateHwndRenderTarget(&props, &hwnd_props).ok()?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
            let brush = rt.CreateSolidColorBrush(&color(0, 1.0), None).ok()?;
            w.target = Some(Target { rt, brush });
        }
    }
    Some(())
}

/// The status line's words and whether they are an error.
fn status_words(status: &Status, lang: Lang) -> Option<(String, bool)> {
    let t = |k| text(lang, k).to_owned();
    Some(match status {
        Status::None | Status::Off => return None,
        Status::Queued => (t(Key::TrStatusQueued), false),
        Status::Waiting => (t(Key::TrStatusWaiting), false),
        Status::Held => (t(Key::TrStatusHeld), false),
        Status::Running { pct, eta } => (i18n::tr_running(lang, *pct, *eta), false),
        Status::Failed(why) => (why.clone(), true),
        Status::PlayerError => (t(Key::TrPlayerError), true),
    })
}

/// What the empty list says, if anything.
fn empty_words(view: &View) -> Option<&'static str> {
    if !view.doc.segments.is_empty() {
        return None;
    }
    match view.status {
        Status::Off => Some(text(view.lang, Key::TrOffTitle)),
        Status::None | Status::PlayerError if view.doc.has_transcript => {
            Some(text(view.lang, Key::TrEmpty))
        }
        _ => None,
    }
}

fn current_line(w: &Win) -> Option<usize> {
    w.position
        .and_then(|ms| doc::current_line(&w.view.doc.segments, ms))
}

fn paint(w: &mut Win) {
    if ensure_target(w).is_none() {
        return;
    }
    let p = theme::current_window();
    let target = w.target.as_ref().expect("ensured");
    let rt: &ID2D1RenderTarget = &target.rt;
    let pen = Pen {
        rt,
        brush: &target.brush,
        text: &w.text,
    };
    let (wd, _) = size_dip(w);
    let f = &w.frame;
    let lang = w.view.lang;
    let ring = |focus: Focus| w.ring && w.focus == focus;
    unsafe {
        rt.SetDpi(w.dpi as f32, w.dpi as f32);
        rt.BeginDraw();
        rt.Clear(Some(&color(p.bg, 1.0)));
    }
    // Header: the title and what is known about the recording.
    let h = f.header;
    pen.str(
        &w.view.doc.title,
        Rect { h: TITLE_H, ..h },
        TITLE_PT,
        STRONG,
        p.text,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        false,
    );
    pen.str(
        &doc::meta_line(&w.view.doc, lang),
        Rect {
            y: h.bottom() - META_H,
            h: META_H,
            ..h
        },
        META_PT,
        NORMAL,
        p.secondary,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        false,
    );
    // The buttons.
    for (i, (btn, r)) in f.buttons.iter().enumerate() {
        let hot = w.hover == Some(Hot::Btn(i));
        let (fill, ink) = if btn.primary() {
            let base = primary_fill(&p);
            (if hot { mix(base, p.text, 0.12) } else { base }, p.on_accent)
        } else {
            (if hot { p.button_hover } else { p.button }, p.text)
        };
        pen.fill(*r, BTN_RADIUS, fill, 1.0);
        pen.str(
            text(lang, btn.label()),
            *r,
            BTN_PT,
            btn_weight(*btn),
            ink,
            DWRITE_TEXT_ALIGNMENT_CENTER,
            false,
        );
        if ring(Focus::Btn(i)) {
            pen.ring(inflate(*r, 2.0), BTN_RADIUS + 2.0, p.accent);
        }
    }
    // The player bar, between two hairlines.
    let top = f.position.y;
    let bottom = f.position.bottom();
    pen.hline(0.0, wd, top, p.border);
    pen.hline(0.0, wd, bottom - 1.0, p.border);
    let (cx, cy) = (f.play.x + PLAY / 2.0, f.play.y + PLAY / 2.0);
    let play_fill = if w.hover == Some(Hot::Play) { p.button_hover } else { p.button };
    pen.circle(cx, cy, PLAY / 2.0, play_fill);
    let icon = if w.player.playing() { Icon::Pause } else { Icon::Play };
    let _ = w.icons.draw(rt, &target.brush, icon, f.play, ICON, p.text, 1.0, None);
    if ring(Focus::Play) {
        pen.ring(inflate(f.play, 2.0), PLAY / 2.0 + 2.0, p.accent);
    }
    let length = duration(w);
    let total = length.unwrap_or(0);
    let pos = w.position.unwrap_or(0).min(total);
    pen.str(
        &output::short_time(pos),
        f.position,
        CLOCK_PT,
        NORMAL,
        p.text,
        DWRITE_TEXT_ALIGNMENT_LEADING,
        false,
    );
    if let Some(ms) = length {
        pen.str(
            &output::short_time(ms),
            f.length,
            CLOCK_PT,
            NORMAL,
            p.secondary,
            DWRITE_TEXT_ALIGNMENT_TRAILING,
            false,
        );
    }
    let bar = f.bar;
    let track = Rect {
        x: bar.x,
        y: bar.y + (BAR_HIT_H - TRACK_H) / 2.0,
        w: bar.w,
        h: TRACK_H,
    };
    pen.fill(track, TRACK_H / 2.0, track_colour(&p), 1.0);
    let frac = if total > 0 { pos as f32 / total as f32 } else { 0.0 };
    pen.fill(Rect { w: bar.w * frac, ..track }, TRACK_H / 2.0, p.accent, 1.0);
    let grow = if w.hover == Some(Hot::Bar) || w.seeking { 1.0 } else { 0.0 };
    pen.circle(bar.x + bar.w * frac, bar.y + BAR_HIT_H / 2.0, THUMB / 2.0 + grow, p.accent);
    if ring(Focus::Bar) {
        pen.ring(inflate(bar, 4.0), BAR_HIT_H / 2.0 + 4.0, p.accent);
    }
    // The status line.
    if let (Some(r), Some((words, error))) = (f.status, status_words(status(w), lang)) {
        let ink = if error { p.error } else { p.secondary };
        pen.str(&words, r, STATUS_PT, NORMAL, ink, DWRITE_TEXT_ALIGNMENT_LEADING, false);
        pen.hline(0.0, wd, r.bottom() - 1.0, p.border);
    }
    // The lines.
    let list = f.list;
    unsafe { rt.PushAxisAlignedClip(&rect_f(list), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    if let Some(words) = empty_words(&w.view) {
        let r = Rect {
            x: PAD,
            y: list.y + EMPTY_TOP,
            w: list.w - 2.0 * PAD,
            h: TEXT_MIN_H + 4.0,
        };
        pen.str(words, r, LINE_PT, NORMAL, p.secondary, DWRITE_TEXT_ALIGNMENT_CENTER, false);
    }
    let current = current_line(w);
    let labels = doc::labels(lang);
    let two = w.view.doc.two_channel;
    let tx = text_x(two);
    let (sel, sel_a) = rgba(p.selection);
    for i in layout::visible(&w.tops, &w.heights, w.scroll, list.h) {
        let s = &w.view.doc.segments[i];
        let y = list.y + LIST_TOP + w.tops[i] - w.scroll;
        let row = Rect {
            x: PAD - ROW_BLEED,
            y,
            w: wd - 2.0 * (PAD - ROW_BLEED),
            h: w.heights[i],
        };
        let hot = w.hover == Some(Hot::Line(i));
        if current == Some(i) {
            pen.fill(row, ROW_RADIUS, sel, sel_a);
            pen.fill(Rect { w: PLAYING_BAR_W, ..row }, PLAYING_BAR_W / 2.0, p.accent, 1.0);
        } else if hot {
            pen.fill(row, ROW_RADIUS, p.hover, 1.0);
        }
        let ty = y + LINE_PAD_Y;
        pen.str(
            &output::short_time(s.start_ms),
            Rect { x: PAD, y: ty, w: TIME_W, h: TEXT_MIN_H },
            TIME_PT,
            NORMAL,
            time_colour(&p, hot || current == Some(i)),
            DWRITE_TEXT_ALIGNMENT_LEADING,
            false,
        );
        if two && let Some(who) = s.speaker {
            let label = labels.of(who);
            let cw = (w.text.width(label, CHIP_PT, STRONG) + 2.0 * CHIP_PAD_X).ceil();
            let chip = Rect {
                x: PAD + TIME_W,
                y: ty + (TEXT_MIN_H - CHIP_H) / 2.0,
                w: cw.min(CHIP_COL - 8.0),
                h: CHIP_H,
            };
            match who {
                Speaker::Me => pen.fill(chip, ROW_RADIUS, sel, sel_a),
                Speaker::Others => {
                    let fill = if hot && current != Some(i) { p.button_hover } else { p.hover };
                    pen.fill(chip, ROW_RADIUS, fill, 1.0);
                }
            }
            pen.str(label, chip, CHIP_PT, STRONG, p.text, DWRITE_TEXT_ALIGNMENT_CENTER, false);
        }
        pen.str(
            &s.text,
            Rect {
                x: tx,
                y: ty,
                w: w.heights_w,
                h: row.h,
            },
            LINE_PT,
            NORMAL,
            p.text,
            DWRITE_TEXT_ALIGNMENT_LEADING,
            true,
        );
        if ring(Focus::List) && w.focus_line == i {
            pen.ring(inflate(row, -1.0), ROW_RADIUS, p.accent);
        }
    }
    if ring(Focus::List) && w.view.doc.segments.is_empty() {
        pen.ring(inflate(list, -2.0), ROW_RADIUS, p.accent);
    }
    unsafe { rt.PopAxisAlignedClip() };
    let ended = unsafe { rt.EndDraw(None, None) };
    if ended.is_err() {
        // D2DERR_RECREATE_TARGET and the like: a new target, and a frame
        // drawn on it, so the window is not left blank.
        w.target = None;
        invalidate(w.hwnd);
    }
}

/// The seek track: a little of the secondary ink in the window's base, so the
/// accent fill and thumb hold 3:1 against it (3.05 light, 3.92 warm dark,
/// 3.57 Terminal).
fn track_colour(p: &WindowPalette) -> u32 {
    mix(p.bg, p.secondary, 0.18)
}

/// A line's time: the secondary ink, pulled toward the text on a highlighted
/// row (light secondary on the playing line's fill is only 3.91:1).
fn time_colour(p: &WindowPalette, highlighted: bool) -> u32 {
    if highlighted { mix(p.secondary, p.text, 0.35) } else { p.secondary }
}

// ------------------------------------------------------------------ input

fn hit(w: &Win, x: f32, y: f32) -> Option<Hot> {
    let f = &w.frame;
    if let Some(i) = f.buttons.iter().position(|(_, r)| r.contains(x, y)) {
        return Some(Hot::Btn(i));
    }
    if f.play.contains(x, y) {
        return Some(Hot::Play);
    }
    if f.bar.contains(x, y) {
        return Some(Hot::Bar);
    }
    if f.list.contains(x, y) {
        return layout::line_at(&w.tops, &w.heights, w.scroll, y - f.list.y).map(Hot::Line);
    }
    None
}

fn lo(l: LPARAM) -> i32 {
    (l.0 & 0xFFFF) as i16 as i32
}

fn hi(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xFFFF) as i16 as i32
}

fn to_dip(w: &Win, l: LPARAM) -> (f32, f32) {
    let k = scale(w);
    (lo(l) as f32 / k, hi(l) as f32 / k)
}

/// The play button and Space: pauses, or plays from the position (from the
/// start once the end was reached).
fn play_pause(w: &mut Win) {
    if w.player.playing() {
        pause(w);
        return;
    }
    let at = w.position.unwrap_or(0);
    let at = if duration(w).is_some_and(|d| at >= d) { 0 } else { at };
    start(w, at);
}

/// Plays from line `i`'s start.
fn play_from(w: &mut Win, i: usize) {
    if let Some(s) = w.view.doc.segments.get(i) {
        w.focus_line = i;
        let at = s.start_ms;
        start(w, at);
    }
}

fn arm(w: &Win, id: usize, ms: u32) {
    unsafe {
        SetTimer(Some(w.hwnd), id, ms, None);
    }
}

fn disarm(w: &Win) {
    unsafe {
        let _ = KillTimer(Some(w.hwnd), TIMER_PLAY);
        let _ = KillTimer(Some(w.hwnd), TIMER_HOLD);
    }
}

/// The recording's length: the sidecar's, else what the player found.
fn duration(w: &Win) -> Option<u64> {
    w.view.doc.duration_ms.or_else(|| w.player.duration_ms())
}

fn set_failed(w: &mut Win, failed: bool) {
    if w.failed != failed {
        w.failed = failed;
        relayout(w);
    }
}

/// What the status line says: the player's error over the job's status.
fn status(w: &Win) -> &Status {
    if w.failed { &Status::PlayerError } else { &w.view.status }
}

/// Plays from `ms`; the play timer runs while it plays.
fn start(w: &mut Win, ms: u64) {
    w.position = Some(ms);
    match w.player.play_from(ms, w.hwnd) {
        Ok(()) => {
            set_failed(w, false);
            arm(w, TIMER_PLAY, PLAY_TICK_MS);
            follow(w, Instant::now());
        }
        Err(e) => {
            eprintln!("vale: transcript player: {e:?}");
            disarm(w);
            set_failed(w, true);
        }
    }
}

/// Pauses where the player is.
fn pause(w: &mut Win) {
    w.player.pause();
    w.position = Some(w.player.position_ms());
    disarm(w);
}

/// Keeps the playing line in view, unless the list was moved by hand less
/// than `HOLD` ago.
fn follow(w: &mut Win, now: Instant) {
    if w.hold_until.is_some_and(|until| now < until) {
        return;
    }
    if let Some(i) = current_line(w) {
        w.scroll = layout::follow_scroll(&w.tops, &w.heights, i, w.scroll, list_h(w));
        rehover(w);
    }
}

/// The play timer: the position and the marked line move on. A player that
/// stopped (the end, or another window's player started) stops the timer.
fn tick(w: &mut Win, now: Instant) {
    unsafe {
        let _ = KillTimer(Some(w.hwnd), TIMER_PLAY);
    }
    let picking = w.seeking || w.key_seek;
    if !w.player.playing() {
        if !picking {
            w.position = Some(w.player.position_ms());
        }
        disarm(w);
        return;
    }
    if !picking {
        w.position = Some(w.player.position_ms());
    }
    follow(w, now);
    arm(w, TIMER_PLAY, PLAY_TICK_MS);
}

/// The wheel or a key moved the list: the line is not followed for `HOLD`.
fn hold_follow(w: &mut Win, now: Instant) {
    w.hold_until = Some(now + HOLD);
    if w.player.playing() {
        arm(w, TIMER_HOLD, HOLD.as_millis() as u32);
    }
}

/// `WM_APP_PLAYER`: the end (`code` 0) puts the position back to the start,
/// a failure (1) says the audio cannot be played. A message from a run that
/// another play has replaced is ignored.
fn on_player(w: &mut Win, code: usize) {
    if w.player.playing() {
        return;
    }
    disarm(w);
    if code == 0 {
        w.position = Some(0);
    } else {
        w.position = Some(w.player.position_ms());
        set_failed(w, true);
    }
}

/// Left/Right on the bar was let go (or the window lost the keyboard):
/// playing, it plays from there.
fn end_key_seek(w: &mut Win) {
    if !w.key_seek {
        return;
    }
    w.key_seek = false;
    if w.player.playing() {
        start(w, w.position.unwrap_or(0));
    }
}

/// A press on the bar was let go (or lost): playing, it plays from there.
fn end_seek(w: &mut Win) {
    if !w.seeking {
        return;
    }
    w.seeking = false;
    if w.player.playing() {
        start(w, w.position.unwrap_or(0));
    }
}

/// The bar under `x` moves the position (the player follows on release).
fn seek_to(w: &mut Win, x: f32) {
    let Some(duration) = duration(w).filter(|&d| d > 0) else {
        return;
    };
    w.position = Some(layout::seek_ms(w.frame.bar, x, duration));
}

fn screen_rect(w: &Win, r: Rect) -> crate::capture::Rect {
    let k = scale(w);
    let mut pt = POINT {
        x: (r.x * k).round() as i32,
        y: (r.y * k).round() as i32,
    };
    let _ = unsafe { ClientToScreen(w.hwnd, &mut pt) };
    crate::capture::Rect::new(pt.x, pt.y, (r.w * k).round() as i32, (r.h * k).round() as i32)
}

/// The tip of `hot` and where it sits in the client area. The bar's anchor
/// is a small box at the pointer's x, fixed when the bar is first reported
/// and kept while the pointer stays on it.
fn tip_spec(w: &mut Win, hot: Option<Hot>) -> Option<TipSpec> {
    let Some(hot) = hot else {
        w.tip_bar_x = None;
        return None;
    };
    if hot != Hot::Bar {
        w.tip_bar_x = None;
    }
    let playing = w.player.playing();
    let (rect, btn) = match hot {
        Hot::Btn(i) => {
            let &(btn, r) = w.frame.buttons.get(i)?;
            (r, Some(btn))
        }
        Hot::Play => (w.frame.play, None),
        Hot::Bar => {
            let bar = w.frame.bar;
            let at = *w.tip_bar_x.get_or_insert_with(|| {
                w.cursor.map_or(bar.x + bar.w / 2.0, |(x, _)| x).clamp(bar.x, bar.right())
            });
            let side = 2.0 * bar.h;
            (Rect { x: at - side / 2.0, y: bar.y, w: side, h: bar.h }, None)
        }
        Hot::Line(_) => return None,
    };
    let tip = layout::tip_of(hot, playing, btn)?;
    Some(TipSpec { id: layout::tip_id(hot, btn), rect, scale: scale(w), tip })
}

fn request_of(btn: Btn, anchor: crate::capture::Rect) -> Request {
    match btn {
        Btn::Copy => Request::Copy,
        Btn::OpenMd => Request::OpenMd,
        Btn::OpenFolder => Request::OpenFolder,
        Btn::MakeNote => Request::MakeNote,
        Btn::CopyPrompt => Request::CopyPrompt,
        Btn::Redo => Request::Redo { anchor },
        Btn::Cancel => Request::Cancel,
        Btn::Retry => Request::Retry,
        Btn::Settings => Request::Settings,
    }
}

fn press_button(w: &mut Win, i: usize) {
    let Some(&(btn, r)) = w.frame.buttons.get(i) else {
        return;
    };
    let anchor = screen_rect(w, r);
    push(&w.view.doc.audio, request_of(btn, anchor));
}

fn activate(w: &mut Win, hot: Hot) {
    match hot {
        Hot::Btn(i) => press_button(w, i),
        Hot::Play => play_pause(w),
        Hot::Bar => {}
        Hot::Line(i) => play_from(w, i),
    }
}

/// Tab order: the buttons, play, the seek bar, the list.
fn focus_order(w: &Win) -> Vec<Focus> {
    let mut order: Vec<Focus> = (0..w.buttons.len()).map(Focus::Btn).collect();
    order.extend([Focus::Play, Focus::Bar, Focus::List]);
    order
}

fn move_focus(w: &mut Win, back: bool) {
    let order = focus_order(w);
    let at = order.iter().position(|f| *f == w.focus).unwrap_or(0);
    let n = order.len();
    w.focus = order[if back { (at + n - 1) % n } else { (at + 1) % n }];
}

/// What a hot spot stands for: a button by its kind, a line by its place in
/// time and text, so an index that moved to something else is told apart.
#[derive(Clone, Debug, PartialEq)]
enum Pressed {
    Btn(Option<Btn>),
    Play,
    Bar,
    Line(Option<(u64, String)>),
}

fn target_of(w: &Win, hot: Hot) -> Pressed {
    match hot {
        Hot::Btn(i) => Pressed::Btn(w.frame.buttons.get(i).map(|(b, _)| *b)),
        Hot::Play => Pressed::Play,
        Hot::Bar => Pressed::Bar,
        Hot::Line(i) => Pressed::Line(w.view.doc.segments.get(i).map(|s| (s.start_ms, s.text.clone()))),
    }
}

/// The hover follows what is under the resting pointer now (after a
/// scroll or an update moved the rows).
fn rehover(w: &mut Win) {
    w.hover = w.cursor.and_then(|(x, y)| hit(w, x, y));
}

fn scroll_by(w: &mut Win, dy: f32) {
    let max = layout::max_scroll(&w.tops, &w.heights, list_h(w));
    w.scroll = (w.scroll + dy).clamp(0.0, max);
}

fn focus_line_to(w: &mut Win, i: usize) {
    let n = w.view.doc.segments.len();
    if n == 0 {
        return;
    }
    w.focus_line = i.min(n - 1);
    w.scroll = layout::ensure_visible(&w.tops, &w.heights, w.focus_line, w.scroll, list_h(w));
}

fn key_down(hwnd: HWND, vk: u16) -> bool {
    // Any key ends the tip; Tab onto the play button shows its tip anew.
    tooltip::hide(hwnd);
    let handled = key_down_in(hwnd, vk);
    if vk == VK_TAB.0 {
        let spec = with_win(hwnd, |w| (w.focus == Focus::Play).then(|| tip_spec(w, Some(Hot::Play))))
            .flatten()
            .flatten();
        if let Some(spec) = spec {
            tooltip::hover(hwnd, Some(on_screen(hwnd, spec)));
        }
    }
    handled
}

fn key_down_in(hwnd: HWND, vk: u16) -> bool {
    let down = |k: u16| unsafe { GetKeyState(i32::from(k)) } < 0;
    if vk == VK_ESCAPE.0 {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return true;
    }
    with_win(hwnd, |w| {
        let page = (list_h(w) * 0.9).max(WHEEL_STEP);
        let in_list = w.focus == Focus::List;
        let scroll = w.scroll;
        // Only these move the list by hand; a play's own follow does not hold.
        let moves_list = [VK_PRIOR, VK_NEXT, VK_HOME, VK_END].iter().any(|k| k.0 == vk)
            || (in_list && (vk == VK_UP.0 || vk == VK_DOWN.0));
        match vk {
            v if v == VK_TAB.0 => move_focus(w, down(VK_SHIFT.0)),
            0x43 if down(VK_CONTROL.0) => {
                if w.buttons.iter().any(|(b, _)| *b == Btn::Copy) {
                    push(&w.view.doc.audio, Request::Copy);
                }
                return true;
            }
            v if v == VK_RETURN.0 || v == VK_SPACE.0 => match w.focus {
                Focus::Btn(i) => press_button(w, i),
                Focus::Play => play_pause(w),
                Focus::Bar => {
                    if v == VK_SPACE.0 {
                        play_pause(w);
                    }
                }
                Focus::List => {
                    if v == VK_RETURN.0 {
                        let i = w.focus_line;
                        play_from(w, i);
                    } else {
                        play_pause(w);
                    }
                }
            },
            v if v == VK_UP.0 && in_list => focus_line_to(w, w.focus_line.saturating_sub(1)),
            v if v == VK_DOWN.0 && in_list => focus_line_to(w, w.focus_line + 1),
            v if (v == VK_LEFT.0 || v == VK_RIGHT.0) && w.focus == Focus::Bar => {
                let duration = duration(w).unwrap_or(0);
                let at = w.position.unwrap_or(0);
                let to = if v == VK_LEFT.0 {
                    at.saturating_sub(SEEK_STEP_MS)
                } else {
                    (at + SEEK_STEP_MS).min(duration)
                };
                w.position = Some(to);
                w.key_seek = w.player.playing();
            }
            v if v == VK_PRIOR.0 => scroll_by(w, -page),
            v if v == VK_NEXT.0 => scroll_by(w, page),
            v if v == VK_HOME.0 => {
                w.scroll = 0.0;
                if in_list {
                    w.focus_line = 0;
                }
            }
            v if v == VK_END.0 => {
                w.scroll = layout::max_scroll(&w.tops, &w.heights, list_h(w));
                if in_list {
                    w.focus_line = w.view.doc.segments.len().saturating_sub(1);
                }
            }
            _ => return false,
        }
        if moves_list && w.scroll != scroll {
            hold_follow(w, Instant::now());
        }
        rehover(w);
        w.ring = true;
        true
    })
    .unwrap_or(false)
}

fn track_leave(w: &mut Win) {
    if w.tracking {
        return;
    }
    let mut t = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: w.hwnd,
        dwHoverTime: 0,
    };
    w.tracking = unsafe { TrackMouseEvent(&mut t) }.is_ok();
}

/// Places the window was moved or sized to, unless it is maximized or
/// minimized.
fn placed(hwnd: HWND) -> Option<CardPlace> {
    if unsafe { IsZoomed(hwnd).as_bool() || IsIconic(hwnd).as_bool() } {
        return None;
    }
    let r = window_rc(hwnd)?;
    Some(CardPlace {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    })
}

fn audio_of(hwnd: HWND) -> Option<PathBuf> {
    with_win(hwnd, |w| w.view.doc.audio.clone())
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            let _ = with_win(hwnd, paint);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_GETMINMAXINFO => {
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
            let k = dpi as f32 / 96.0;
            let mut rc = RECT {
                left: 0,
                top: 0,
                right: (layout::MIN_W * k).round() as i32,
                bottom: (layout::MIN_H * k).round() as i32,
            };
            unsafe {
                let _ = AdjustWindowRectExForDpi(&mut rc, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0), dpi);
            }
            if let Some(info) = unsafe { (lparam.0 as *mut MINMAXINFO).as_mut() } {
                info.ptMinTrackSize = POINT {
                    x: rc.right - rc.left,
                    y: rc.bottom - rc.top,
                };
            }
            LRESULT(0)
        }
        WM_ENTERSIZEMOVE => {
            tooltip::hide(hwnd);
            LRESULT(0)
        }
        WM_SIZE => {
            tooltip::hide(hwnd);
            let size = ((lparam.0 & 0xFFFF) as u32, ((lparam.0 >> 16) & 0xFFFF) as u32);
            let _ = with_win(hwnd, |w| {
                // Minimized has no client area: keep the last size.
                if size.0 == 0 || size.1 == 0 {
                    return;
                }
                w.client = size;
                if let Some(t) = &w.target {
                    let _ = unsafe {
                        t.rt.Resize(&D2D_SIZE_U {
                            width: size.0,
                            height: size.1,
                        })
                    };
                }
                relayout(w);
            });
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xFFFF) as u32;
            let _ = with_win(hwnd, |w| {
                w.dpi = dpi.max(96);
                w.target = None;
            });
            if let Some(rc) = unsafe { (lparam.0 as *const RECT).as_ref() } {
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
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            if let (Some(audio), Some(place)) = (audio_of(hwnd), placed(hwnd)) {
                push(&audio, Request::Placed(place));
            }
            LRESULT(0)
        }
        WM_SETCURSOR if (lparam.0 & 0xFFFF) as u16 == HTCLIENT => {
            let hand = with_win(hwnd, |w| w.hover.is_some() || w.seeking).unwrap_or(false);
            let cursor = if hand { IDC_HAND } else { IDC_ARROW };
            unsafe {
                if let Ok(c) = LoadCursorW(None, cursor) {
                    SetCursor(Some(c));
                }
            }
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            let moved = with_win(hwnd, |w| {
                track_leave(w);
                let (x, y) = to_dip(w, lparam);
                w.cursor = Some((x, y));
                if w.seeking {
                    seek_to(w, x);
                    return (true, None);
                }
                let hot = hit(w, x, y);
                let changed = std::mem::replace(&mut w.hover, hot) != hot;
                (changed, Some(tip_spec(w, hot)))
            });
            if let Some((changed, tip)) = moved {
                if let Some(spec) = tip {
                    tooltip::hover(hwnd, spec.map(|s| on_screen(hwnd, s)));
                }
                if changed {
                    invalidate(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let _ = with_win(hwnd, |w| {
                w.tracking = false;
                w.hover = None;
                w.cursor = None;
                w.tip_bar_x = None;
            });
            tooltip::hover(hwnd, None);
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            tooltip::pressed(hwnd);
            let bar = with_win(hwnd, |w| {
                let (x, y) = to_dip(w, lparam);
                let hot = hit(w, x, y);
                w.pressed = hot;
                w.ring = false;
                match hot {
                    Some(Hot::Btn(i)) => w.focus = Focus::Btn(i),
                    Some(Hot::Play) => w.focus = Focus::Play,
                    Some(Hot::Bar) => {
                        w.focus = Focus::Bar;
                        w.seeking = true;
                        seek_to(w, x);
                    }
                    Some(Hot::Line(i)) => {
                        w.focus = Focus::List;
                        w.focus_line = i;
                    }
                    None => {}
                }
                w.seeking
            });
            if bar == Some(true) {
                unsafe { SetCapture(hwnd) };
            }
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let release = with_win(hwnd, |w| {
                let (x, y) = to_dip(w, lparam);
                let pressed = w.pressed.take();
                if w.seeking {
                    end_seek(w);
                    return true;
                }
                if let Some(hot) = pressed
                    && hit(w, x, y) == Some(hot)
                {
                    activate(w, hot);
                }
                false
            });
            if release == Some(true) {
                let _ = unsafe { ReleaseCapture() };
            }
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            let _ = with_win(hwnd, end_seek);
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            tooltip::hide(hwnd);
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32;
            let _ = with_win(hwnd, |w| {
                scroll_by(w, -delta / 120.0 * WHEEL_STEP);
                hold_follow(w, Instant::now());
                rehover(w);
            });
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            let now = Instant::now();
            let _ = with_win(hwnd, |w| match wparam.0 {
                TIMER_PLAY => tick(w, now),
                TIMER_HOLD => {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), TIMER_HOLD);
                    }
                    w.hold_until = None;
                    tick(w, now);
                }
                _ => {}
            });
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_APP_PLAYER => {
            let _ = with_win(hwnd, |w| on_player(w, wparam.0));
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_KEYUP if wparam.0 as u16 == VK_LEFT.0 || wparam.0 as u16 == VK_RIGHT.0 => {
            let _ = with_win(hwnd, end_key_seek);
            invalidate(hwnd);
            LRESULT(0)
        }
        // Focus lost (Alt+Tab, a click elsewhere): a tip that Tab showed must
        // not stay over the other app.
        WM_KILLFOCUS => {
            tooltip::hide(hwnd);
            let _ = with_win(hwnd, end_key_seek);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SYSKEYDOWN => {
            tooltip::hide(hwnd);
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_KEYDOWN => {
            if key_down(hwnd, wparam.0 as u16) {
                invalidate(hwnd);
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_DESTROY => {
            tooltip::hide(hwnd);
            let gone = WINS.with(|cell| {
                let mut wins = cell.try_borrow_mut().ok()?;
                let i = wins.iter().position(|w| w.hwnd == hwnd)?;
                Some(wins.remove(i))
            });
            if let Some(w) = gone {
                let audio = w.view.doc.audio.clone();
                drop(w);
                push(&audio, Request::Closed);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// ------------------------------------------------------------------ debug

/// Puts the player of the window of `audio` at `ms` (the demo's screenshots
/// of the playing line).
#[cfg(debug_assertions)]
pub fn demo_position(audio: &Path, ms: u64) {
    if let Some(hwnd) = hwnd_of(audio) {
        let _ = with_win(hwnd, |w| {
            w.position = Some(ms);
            if let Some(i) = current_line(w) {
                w.focus_line = i;
                w.scroll = layout::follow_scroll(&w.tops, &w.heights, i, w.scroll, list_h(w));
            }
        });
        invalidate(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::super::player::PlayerError;
    use super::*;
    use crate::transcribe::doc::Doc;
    use crate::transcribe::output::Segment;
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, IsWindow, SendMessageW};

    fn seg(i: u64, text: &str) -> Segment {
        Segment {
            start_ms: i * 5_000,
            end_ms: i * 5_000 + 4_000,
            speaker: Some(if i.is_multiple_of(2) { Speaker::Others } else { Speaker::Me }),
            text: text.into(),
        }
    }

    fn view(audio: &str, lines: usize, status: Status) -> View {
        View {
            doc: Doc {
                audio: PathBuf::from(audio),
                md: PathBuf::from(audio).with_extension("md"),
                title: "Sync mingguan".into(),
                app: None,
                started: None,
                duration_ms: Some(lines as u64 * 5_000),
                model: None,
                language: None,
                two_channel: true,
                has_transcript: true,
                segments: (0..lines as u64)
                    .map(|i| seg(i, "Onboarding design udah final, link Figma gw share di chat"))
                    .collect(),
            },
            status,
            can_redo: true,
            lang: Lang::Id,
            font: UiFont::Sans,
        }
    }

    /// What the fake player was asked and where it is, per test thread.
    #[derive(Default)]
    pub(super) struct Fake {
        pub calls: Vec<u64>,
        pub playing: bool,
        pub position: u64,
        pub fail: bool,
    }

    thread_local! {
        pub(super) static FAKE: RefCell<Fake> = RefCell::new(Fake::default());
    }

    /// Stands in for the real player: records the requests, plays nothing.
    pub(super) struct FakePlayer;

    impl Playback for FakePlayer {
        fn play_from(&mut self, ms: u64, _notify: HWND) -> std::result::Result<(), PlayerError> {
            FAKE.with(|f| {
                let mut f = f.borrow_mut();
                f.calls.push(ms);
                if f.fail {
                    f.playing = false;
                    return Err(PlayerError::Device("fake".into()));
                }
                f.playing = true;
                f.position = ms;
                Ok(())
            })
        }
        fn pause(&mut self) {
            FAKE.with(|f| f.borrow_mut().playing = false);
        }
        fn playing(&self) -> bool {
            FAKE.with(|f| f.borrow().playing)
        }
        fn position_ms(&self) -> u64 {
            FAKE.with(|f| f.borrow().position)
        }
        fn duration_ms(&self) -> Option<u64> {
            None
        }
    }

    fn fake<R>(f: impl FnOnce(&mut Fake) -> R) -> R {
        FAKE.with(|cell| f(&mut cell.borrow_mut()))
    }

    /// A timer that exists can be killed (Ok); one that does not, cannot.
    fn timer_set(h: HWND, id: usize) -> bool {
        unsafe { KillTimer(Some(h), id) }.is_ok()
    }

    fn point(hwnd: HWND, x: f32, y: f32) -> LPARAM {
        let k = with_win(hwnd, |w| scale(w)).unwrap();
        LPARAM(((((y * k) as isize) & 0xFFFF) << 16) | (((x * k) as isize) & 0xFFFF))
    }

    /// A press and release at the client point (dip) `(x, y)`.
    fn click(hwnd: HWND, x: f32, y: f32) {
        let at = point(hwnd, x, y);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(0)), Some(at));
            SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(at));
        }
    }

    /// The middle of line `i`'s text.
    fn line_point(hwnd: HWND, i: usize) -> (f32, f32) {
        with_win(hwnd, |w| {
            let y = w.frame.list.y + LIST_TOP + w.tops[i] - w.scroll + w.heights[i] / 2.0;
            (text_x(w.view.doc.two_channel) + 10.0, y)
        })
        .unwrap()
    }

    #[test]
    fn a_click_on_a_line_plays_from_its_start() {
        let a = r"C:\rec\click.m4a";
        assert!(open(HWND::default(), view(a, 6, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        let (x, y) = line_point(hwnd, 2);
        click(hwnd, x, y);
        let start = with_win(hwnd, |w| w.view.doc.segments[2].start_ms).unwrap();
        assert_eq!(fake(|f| f.calls.clone()), [start]);
        assert_eq!(with_win(hwnd, |w| w.position).unwrap(), Some(start));
        assert!(fake(|f| f.playing));
        // A press on one line released on another (a drag) plays nothing.
        let (_, y3) = line_point(hwnd, 3);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(0)), Some(point(hwnd, x, y3)));
            SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(point(hwnd, x, y)));
        }
        assert_eq!(fake(|f| f.calls.len()), 1);
        // A player that cannot play: the status says so, and a later play
        // that works takes it back.
        fake(|f| f.fail = true);
        click(hwnd, x, y);
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::PlayerError);
        fake(|f| f.fail = false);
        click(hwnd, x, y);
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::None);
        finish();
    }

    #[test]
    fn a_click_or_drag_on_the_bar_seeks() {
        let a = r"C:\rec\seek.m4a";
        assert!(open(HWND::default(), view(a, 12, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        let (bar, duration) = with_win(hwnd, |w| (w.frame.bar, w.view.doc.duration_ms.unwrap())).unwrap();
        let mid = bar.y + bar.h / 2.0;
        // Where a point lands after the trip through whole pixels.
        let k = with_win(hwnd, |w| scale(w)).unwrap();
        let ms_at = |x: f32| layout::seek_ms(bar, ((x * k) as isize) as f32 / k, duration);
        // Paused: a click only moves the position.
        click(hwnd, bar.x + bar.w / 2.0, mid);
        assert_eq!(with_win(hwnd, |w| w.position).unwrap(), Some(ms_at(bar.x + bar.w / 2.0)));
        assert!(fake(|f| f.calls.is_empty()));
        // Playing: a drag plays from where it is let go.
        let (x, y) = line_point(hwnd, 0);
        click(hwnd, x, y);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(0)), Some(point(hwnd, bar.x + bar.w / 4.0, mid)));
            SendMessageW(hwnd, WM_MOUSEMOVE, Some(WPARAM(0)), Some(point(hwnd, bar.x + bar.w * 0.75, mid)));
            SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(point(hwnd, bar.x + bar.w * 0.75, mid)));
        }
        let want = ms_at(bar.x + bar.w * 0.75);
        assert_eq!(fake(|f| f.calls.clone()), [0, want]);
        assert_eq!(with_win(hwnd, |w| w.position).unwrap(), Some(want));
        finish();
    }

    #[test]
    fn a_manual_scroll_holds_following_for_three_seconds() {
        let a = r"C:\rec\follow.m4a";
        assert!(open(HWND::default(), view(a, 80, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        let (x, y) = line_point(hwnd, 0);
        click(hwnd, x, y);
        assert!(fake(|f| f.playing));
        // The player reaches line 50, far below the list's end.
        let target = with_win(hwnd, |w| w.view.doc.segments[50].start_ms).unwrap();
        fake(|f| f.position = target + 100);
        // One wheel notch, then a tick right after and 2.9 s later: held.
        let notch = WPARAM(((-120i16) as u16 as usize) << 16);
        unsafe {
            SendMessageW(hwnd, WM_MOUSEWHEEL, Some(notch), Some(LPARAM(0)));
        }
        assert!(timer_set(hwnd, TIMER_HOLD), "the hold's one-shot is armed");
        let held = with_win(hwnd, |w| w.scroll).unwrap();
        let now = Instant::now();
        with_win(hwnd, |w| tick(w, now));
        assert_eq!(with_win(hwnd, |w| w.scroll).unwrap(), held);
        with_win(hwnd, |w| tick(w, now + Duration::from_millis(2_900)));
        assert_eq!(with_win(hwnd, |w| w.scroll).unwrap(), held);
        // Three seconds on: the line is followed into view.
        with_win(hwnd, |w| tick(w, now + HOLD + Duration::from_millis(1)));
        let (scroll, want) = with_win(hwnd, |w| {
            (w.scroll, layout::follow_scroll(&w.tops, &w.heights, 50, held, list_h(w)))
        })
        .unwrap();
        assert_ne!(scroll, held);
        assert_eq!(scroll, want);
        // A key that moves the list holds it too.
        with_win(hwnd, |w| w.focus = Focus::List);
        assert!(key_down(hwnd, VK_HOME.0));
        with_win(hwnd, |w| tick(w, Instant::now()));
        assert_eq!(with_win(hwnd, |w| w.scroll).unwrap(), 0.0);
        finish();
    }

    #[test]
    fn the_play_timer_runs_only_while_playing() {
        let a = r"C:\rec\timer.m4a";
        assert!(open(HWND::default(), view(a, 6, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        assert!(!timer_set(hwnd, TIMER_PLAY), "opened: none");
        let (x, y) = line_point(hwnd, 1);
        click(hwnd, x, y);
        assert!(timer_set(hwnd, TIMER_PLAY), "playing: armed");
        // Each tick re-arms it while playing.
        with_win(hwnd, |w| tick(w, Instant::now()));
        assert!(timer_set(hwnd, TIMER_PLAY), "re-armed by the tick");
        // Space in the list pauses: none; again plays from where it was.
        with_win(hwnd, |w| tick(w, Instant::now()));
        fake(|f| f.position = 6_000);
        with_win(hwnd, |w| w.focus = Focus::List);
        assert!(key_down(hwnd, VK_SPACE.0));
        assert!(!fake(|f| f.playing));
        assert!(!timer_set(hwnd, TIMER_PLAY), "paused: none");
        assert_eq!(with_win(hwnd, |w| w.position).unwrap(), Some(6_000));
        assert!(key_down(hwnd, VK_SPACE.0));
        assert_eq!(fake(|f| *f.calls.last().unwrap()), 6_000);
        assert!(timer_set(hwnd, TIMER_PLAY), "playing again: armed");
        // The end: back to 0, play shows, no timer.
        with_win(hwnd, |w| tick(w, Instant::now()));
        fake(|f| f.playing = false);
        unsafe {
            SendMessageW(hwnd, WM_APP_PLAYER, Some(WPARAM(0)), Some(LPARAM(0)));
        }
        assert_eq!(with_win(hwnd, |w| w.position).unwrap(), Some(0));
        assert!(!timer_set(hwnd, TIMER_PLAY), "ended: none");
        // Stopped by another window's player: the next tick stops the timer.
        assert!(key_down(hwnd, VK_SPACE.0));
        fake(|f| f.playing = false);
        with_win(hwnd, |w| tick(w, Instant::now()));
        assert!(!timer_set(hwnd, TIMER_PLAY), "stopped elsewhere: none");
        // A failure mid-play shows the player's error.
        assert!(key_down(hwnd, VK_SPACE.0));
        fake(|f| f.playing = false);
        unsafe {
            SendMessageW(hwnd, WM_APP_PLAYER, Some(WPARAM(1)), Some(LPARAM(0)));
        }
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::PlayerError);
        assert!(!timer_set(hwnd, TIMER_PLAY), "failed: none");
        finish();
    }

    #[test]
    fn keyboard_plays_keep_following() {
        let a = r"C:\rec\keyfollow.m4a";
        assert!(open(HWND::default(), view(a, 80, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        // Enter on line 50, far below the list's end: it plays and the list
        // follows it, without a hold.
        with_win(hwnd, |w| {
            w.focus = Focus::List;
            w.focus_line = 50;
        });
        assert!(key_down(hwnd, VK_RETURN.0));
        let start = with_win(hwnd, |w| w.view.doc.segments[50].start_ms).unwrap();
        assert_eq!(fake(|f| f.calls.clone()), [start]);
        let (scroll, hold) = with_win(hwnd, |w| (w.scroll, w.hold_until)).unwrap();
        assert!(scroll > 0.0, "followed to line 50");
        assert_eq!(hold, None, "a play is not a manual scroll");
        // The player moves on to line 60: the next tick follows it.
        let later = with_win(hwnd, |w| w.view.doc.segments[60].start_ms).unwrap();
        fake(|f| f.position = later + 100);
        with_win(hwnd, |w| tick(w, Instant::now()));
        let (scroll, want) = with_win(hwnd, |w| {
            (w.scroll, layout::follow_scroll(&w.tops, &w.heights, 60, scroll, list_h(w)))
        })
        .unwrap();
        assert_eq!(scroll, want);
        // Space pauses and plays again: still no hold.
        assert!(key_down(hwnd, VK_SPACE.0));
        assert!(key_down(hwnd, VK_SPACE.0));
        assert_eq!(with_win(hwnd, |w| w.hold_until).unwrap(), None);
        finish();
    }

    #[test]
    fn a_player_error_shows_over_a_job_until_the_next_status() {
        let a = r"C:\rec\jobfail.m4a";
        let queued = view(a, 6, Status::Queued);
        assert!(open(HWND::default(), queued.clone(), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        fake(|f| f.fail = true);
        let (x, y) = line_point(hwnd, 1);
        click(hwnd, x, y);
        // The status line says it; the buttons stay the job's.
        let (shown, buttons, line) = with_win(hwnd, |w| {
            (status(w).clone(), w.buttons.iter().map(|(b, _)| *b).collect::<Vec<_>>(), w.frame.status)
        })
        .unwrap();
        assert_eq!(shown, Status::PlayerError);
        assert_eq!(buttons, layout::buttons_for(&Status::Queued, true, true));
        assert!(line.is_some());
        // The same status again keeps it.
        update(Path::new(a), queued.clone());
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::PlayerError);
        // The job moves on: its status replaces the error, which never
        // comes back on its own.
        let running = view(a, 6, Status::Running { pct: 10, eta: None });
        update(Path::new(a), running);
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::Running { pct: 10, eta: None });
        update(Path::new(a), view(a, 6, Status::None));
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::None);
        assert!(with_win(hwnd, |w| w.frame.status).unwrap().is_none());
        // With no job, the error shows until a play works.
        click(hwnd, x, y);
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::PlayerError);
        fake(|f| f.fail = false);
        click(hwnd, x, y);
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::None);
        finish();
    }

    #[test]
    fn a_player_error_outlasts_the_running_jobs_progress_ticks() {
        let a = r"C:\rec\jobtick.m4a";
        assert!(open(HWND::default(), view(a, 6, Status::Running { pct: 10, eta: None }), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        fake(|f| f.fail = true);
        let (x, y) = line_point(hwnd, 1);
        click(hwnd, x, y);
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::PlayerError);
        // Only the percent and the time left moved: the error stays.
        let tick = Status::Running {
            pct: 11,
            eta: Some(Duration::from_secs(40)),
        };
        update(Path::new(a), view(a, 6, tick));
        assert_eq!(with_win(hwnd, |w| status(w).clone()).unwrap(), Status::PlayerError);
        assert!(!status_kind_changed(&Status::Failed("a".into()), &Status::Failed("b".into())));
        assert!(status_kind_changed(&Status::Queued, &Status::Held));
        fake(|f| f.fail = false);
        finish();
    }

    #[test]
    fn left_right_on_the_bar_restart_the_player_once_on_key_up() {
        let a = r"C:\rec\keyseek.m4a";
        assert!(open(HWND::default(), view(a, 12, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        let (x, y) = line_point(hwnd, 0);
        click(hwnd, x, y);
        assert_eq!(fake(|f| f.calls.clone()), [0]);
        with_win(hwnd, |w| w.focus = Focus::Bar);
        // A held Right: three repeats move the position, nothing restarts,
        // and the tick leaves the picked position alone.
        for _ in 0..3 {
            assert!(key_down(hwnd, VK_RIGHT.0));
        }
        assert_eq!(fake(|f| f.calls.len()), 1);
        fake(|f| f.position = 400);
        with_win(hwnd, |w| tick(w, Instant::now()));
        assert_eq!(with_win(hwnd, |w| w.position).unwrap(), Some(3 * SEEK_STEP_MS));
        // Let go: one restart there.
        unsafe {
            SendMessageW(hwnd, WM_KEYUP, Some(WPARAM(VK_RIGHT.0 as usize)), Some(LPARAM(0)));
        }
        assert_eq!(fake(|f| f.calls.clone()), [0, 3 * SEEK_STEP_MS]);
        // Paused: Left only moves the position, key-up plays nothing.
        assert!(key_down(hwnd, VK_SPACE.0));
        assert!(key_down(hwnd, VK_LEFT.0));
        unsafe {
            SendMessageW(hwnd, WM_KEYUP, Some(WPARAM(VK_LEFT.0 as usize)), Some(LPARAM(0)));
        }
        assert_eq!(fake(|f| f.calls.len()), 2);
        assert_eq!(with_win(hwnd, |w| w.position).unwrap(), Some(2 * SEEK_STEP_MS));
        finish();
    }

    fn count_windows() -> usize {
        all_hwnds()
            .into_iter()
            .filter(|&h| unsafe {
                let mut name = [0u16; 32];
                let n = GetClassNameW(h, &mut name) as usize;
                String::from_utf16_lossy(&name[..n]) == "ValeTranscript"
            })
            .count()
    }

    /// `top` (0xRRGGBB) at `alpha` over `base`.
    fn over(base: u32, top: u32, alpha: f32) -> u32 {
        mix(base, top, alpha)
    }

    #[test]
    fn every_drawn_pair_holds_its_contrast_in_each_window_palette() {
        use crate::lyrics_card::colour::contrast;
        use crate::theme::{Accent, Surface, WindowTheme, window_for};
        for accent in [Accent::Terracotta, Accent::Sage, Accent::Ochre, Accent::Clay, Accent::Olive, Accent::Plum] {
            for (surface, mode) in [
                (Surface::Espresso, WindowTheme::Light),
                (Surface::Espresso, WindowTheme::Dark),
                (Surface::Terminal, WindowTheme::Dark),
            ] {
                let p = window_for(accent, surface, mode, false);
                let what = format!("{accent:?} {surface:?} {mode:?}");
                let (sel, a) = rgba(p.selection);
                let playing = over(p.bg, sel, a);
                let saya_on_playing = over(playing, sel, a);
                // Text: the line, the title and the chips on every fill.
                for fill in [p.bg, playing, saya_on_playing, p.hover, p.button, p.button_hover] {
                    assert!(contrast(p.text, fill) >= 4.5, "{what}: text on {fill:06X}");
                }
                // The secondary ink on the base, the time on a highlighted row.
                assert!(contrast(p.secondary, p.bg) >= 4.5, "{what}: secondary");
                for fill in [playing, p.hover] {
                    assert!(contrast(time_colour(&p, true), fill) >= 4.5, "{what}: time on {fill:06X}");
                }
                // The primary button's label, at rest and hovered.
                let primary = primary_fill(&p);
                for fill in [primary, mix(primary, p.text, 0.12)] {
                    assert!(contrast(p.on_accent, fill) >= 4.5, "{what}: primary on {fill:06X}");
                }
                // The failure reason.
                assert!(contrast(p.error, p.bg) >= 4.5, "{what}: error");
                // Marks: the seek fill on its track, the playing bar on its
                // row, the focus ring on the base and on a highlighted row.
                assert!(contrast(p.accent, track_colour(&p)) >= 3.0, "{what}: seek fill");
                for fill in [p.bg, playing, p.hover] {
                    assert!(contrast(p.accent, fill) >= 3.0, "{what}: accent on {fill:06X}");
                }
            }
        }
    }

    #[test]
    fn a_press_survives_an_update_only_on_the_same_control() {
        let a = r"C:\rec\press.m4a";
        assert!(open(HWND::default(), view(a, 3, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        // Pressed on Salin (button 0), then an update that keeps the buttons.
        with_win(hwnd, |w| w.pressed = Some(Hot::Btn(0)));
        update(Path::new(a), view(a, 3, Status::None));
        assert_eq!(with_win(hwnd, |w| w.pressed).unwrap(), Some(Hot::Btn(0)));
        // A job starts: button 0 is now Batal, so the press is dropped.
        update(Path::new(a), view(a, 3, Status::Queued));
        assert_eq!(with_win(hwnd, |w| w.pressed).unwrap(), None);
        // A line pressed, then the lines change under it: dropped too.
        with_win(hwnd, |w| w.pressed = Some(Hot::Line(1)));
        let mut other = view(a, 3, Status::Queued);
        other.doc.segments[1].text = "lain".into();
        update(Path::new(a), other);
        assert_eq!(with_win(hwnd, |w| w.pressed).unwrap(), None);
        // The hover follows the resting pointer after a scroll.
        update(Path::new(a), view(a, 60, Status::None));
        let (y, row) = with_win(hwnd, |w| {
            let y = w.frame.list.y + LIST_TOP + w.tops[2] + 2.0;
            w.cursor = Some((PAD + 4.0, y));
            rehover(w);
            (y, w.hover)
        })
        .unwrap();
        assert_eq!(row, Some(Hot::Line(2)));
        let after = with_win(hwnd, |w| {
            scroll_by(w, w.heights[0] + w.heights[1]);
            rehover(w);
            (w.hover, layout::line_at(&w.tops, &w.heights, w.scroll, y - w.frame.list.y))
        })
        .unwrap();
        assert_eq!(after.0, after.1.map(Hot::Line));
        assert_ne!(after.0, Some(Hot::Line(2)));
        finish();
    }

    fn finish() {
        close_all();
        take_requests();
        assert!(all_hwnds().is_empty());
    }

    #[test]
    fn opening_the_same_recording_twice_keeps_one_window() {
        let a = r"C:\rec\a.m4a";
        assert!(open(HWND::default(), view(a, 3, Status::Queued), None));
        let first = hwnd_of(Path::new(a)).unwrap();
        // The same audio (the case differs): the one window takes the new view.
        assert!(open(HWND::default(), view(r"c:\REC\A.m4a", 5, Status::None), None));
        assert_eq!(count_windows(), 1);
        assert_eq!(hwnd_of(Path::new(a)), Some(first));
        let shown = with_win(first, |w| (w.view.status.clone(), w.view.doc.segments.len())).unwrap();
        assert_eq!(shown, (Status::None, 5));
        assert!(unsafe { !windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(first).as_bool() });
        // Another recording: a second window, 24 px down and right.
        let b = r"C:\rec\b.m4a";
        assert!(open(HWND::default(), view(b, 1, Status::None), None));
        assert_eq!(count_windows(), 2);
        let (ra, rb) = (window_rc(first).unwrap(), window_rc(hwnd_of(Path::new(b)).unwrap()).unwrap());
        assert_eq!((rb.left - ra.left, rb.top - ra.top), (layout::CASCADE_PX, layout::CASCADE_PX));
        assert!(is_open(Path::new(b)));
        assert_eq!(open_audios().len(), 2);
        finish();
    }

    #[test]
    fn closing_posts_closed_once() {
        let a = r"C:\rec\close.m4a";
        assert!(open(HWND::default(), view(a, 2, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        take_requests();
        unsafe {
            let _ = SendMessageW(hwnd, WM_CLOSE, Some(WPARAM(0)), Some(LPARAM(0)));
        }
        assert!(unsafe { !IsWindow(Some(hwnd)).as_bool() });
        assert!(!is_open(Path::new(a)));
        let closed: Vec<_> = take_requests()
            .into_iter()
            .filter(|(p, r)| *r == Request::Closed && p == Path::new(a))
            .collect();
        assert_eq!(closed.len(), 1);
        // Nothing more comes for it.
        close_all();
        assert!(take_requests().is_empty());
        finish();
    }

    #[test]
    fn line_heights_are_measured_once_per_width() {
        let a = r"C:\rec\measure.m4a";
        let before = MEASURED.with(Cell::get);
        assert!(open(HWND::default(), view(a, 50, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        let measured = || MEASURED.with(Cell::get) - before;
        assert_eq!(measured(), 50);
        // The same view again, or a view that only changes the status: no
        // line is measured again.
        update(Path::new(a), view(a, 50, Status::None));
        update(Path::new(a), view(a, 50, Status::Queued));
        assert_eq!(measured(), 50);
        // The same size: nothing.
        let r = window_rc(hwnd).unwrap();
        let resize = |w: i32| unsafe {
            let _ = SetWindowPos(hwnd, None, r.left, r.top, w, r.height(), SWP_NOZORDER | SWP_NOACTIVATE);
        };
        resize(r.width());
        assert_eq!(measured(), 50);
        // Another width: every line once more.
        resize(r.width() + 120);
        assert_eq!(measured(), 100);
        // A running job's new line: only that one.
        update(Path::new(a), view(a, 51, Status::Queued));
        assert_eq!(measured(), 101);
        finish();
    }

    #[test]
    fn a_click_on_a_button_queues_its_request_and_tab_reaches_every_control() {
        let a = r"C:\rec\keys.m4a";
        assert!(open(HWND::default(), view(a, 3, Status::None), None));
        let hwnd = hwnd_of(Path::new(a)).unwrap();
        take_requests();
        // Tab walks the six buttons, play, the bar and the list, then wraps.
        let mut seen = Vec::new();
        for _ in 0..10 {
            assert!(key_down(hwnd, VK_TAB.0));
            seen.push(with_win(hwnd, |w| w.focus).unwrap());
        }
        let mut want: Vec<Focus> = (0..6).map(Focus::Btn).collect();
        want.extend([Focus::Play, Focus::Bar, Focus::List, Focus::Btn(0)]);
        assert_eq!(seen, want);
        // Enter on the first button (Salin) asks for a copy.
        assert!(key_down(hwnd, VK_RETURN.0));
        assert_eq!(take_requests(), [(PathBuf::from(a), Request::Copy)]);
        // A click on the primary button.
        let (r, k) = with_win(hwnd, |w| (w.frame.buttons[4].1, scale(w))).unwrap();
        let at = LPARAM(((((r.y + 5.0) * k) as isize) << 16) | ((r.x + 5.0) * k) as isize);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(0)), Some(at));
            SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(at));
        }
        assert_eq!(take_requests(), [(PathBuf::from(a), Request::CopyPrompt)]);
        // In the list, Down then Enter puts the position at line 1's start.
        with_win(hwnd, |w| w.focus = Focus::List);
        assert!(key_down(hwnd, VK_DOWN.0));
        assert!(key_down(hwnd, VK_RETURN.0));
        assert_eq!(with_win(hwnd, |w| w.position).unwrap(), Some(5_000));
        finish();
    }
}
