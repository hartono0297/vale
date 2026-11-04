//! The editor window: a normal top-level window "Vale · Screenshot" that
//! opens straight after a capture with the Select tool ready.
//!
//! Layout and hit testing are pure (and tested) at the top; the rest is
//! Win32: Direct2D drawing, mouse and keyboard, a native EDIT control for
//! text (IME and paste work), and the workers that save and auto-copy.
//! Nothing runs at idle: no repeating timer and no thread. A short-lived
//! worker runs after Save, after the pick (the first auto-copy) and after a
//! change has settled; only one-shot timers (the debounce, a clipboard
//! retry) are ever set, each killed when it fires. The rules of auto-copy
//! are pure and live in `auto`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{
    D2DERR_RECREATE_TARGET, ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT,
    RECT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, ID2D1Bitmap,
    ID2D1RenderTarget,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_RETURN,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, IDC_ARROW,
    IDC_CROSS, IDC_IBEAM, IsWindow, LoadCursorW, MINMAXINFO, PostQuitMessage, RegisterClassExW,
    SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SetCursor, SetForegroundWindow, SetWindowPos,
    ShowWindow, WINDOW_EX_STYLE, WM_APP, WM_CAPTURECHANGED, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT,
    WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WM_SIZE, WNDCLASSEXW,
    WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW,
};
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer, WM_TIMER};
use windows::core::{Error, HSTRING, w};
use windows_numerics::Vector2;

use super::auto::{AutoCopy, DEBOUNCE_MS, ResultAction, Step, TimerAction, WriteGate};
use super::export::{self, CopyInbox, CopyReady, Inbox, Prepared, Put, SaveResult};
use super::gfx::{Align, Gfx, Target, bitmap, color, rect_f};
use super::icons::{Glyph, fill, fill_round, glyph, line};
use super::paint::{Pix, draw_annotations, draw_selection};
use super::text::{
    self, TextClick, TextEdit, WM_APP_TEXT, colorref, edit_text, text_click_action,
    text_msg_is_current,
};
use super::{
    Annotation, COLORS, FRect, Handle, Pt, STROKES, Session, Tool, View, initial_scale, text_size,
};
use crate::capture::Shot;
use crate::i18n::{Key, tr};
use crate::theme::{self, WindowPalette};
use crate::ui::geometry::Rect;

const CLASS: windows::core::PCWSTR = w!("ValeShotEditor");

/// Lost render targets made again in a row before a paint gives up (until
/// the next change asks for one).
const RETRIES: u8 = 3;
/// A save worker put its result in the inbox.
const WM_APP_SAVED: u32 = WM_APP + 51;
/// An auto-copy worker put its rendered picture in the copy inbox.
const WM_APP_COPIED: u32 = WM_APP + 52;
/// One-shot: a change settled (`DEBOUNCE_MS` after the last one); the
/// picture is rendered for the clipboard.
const ID_AUTO_DEBOUNCE: usize = 7101;
/// One-shot: the clipboard was busy; the rendered picture is written again.
const ID_AUTO_RETRY: usize = 7102;

// ---- layout (logical px, scaled by `ui`) ----

const TB_H: f32 = 56.0;
const FOOT_H: f32 = 40.0;
const PAD: f32 = 24.0;
const MARGIN: f32 = 12.0;
const BTN: f32 = 34.0;
const SWATCH: f32 = 26.0;
const STROKE_BTN: f32 = 30.0;
const BTN_H: f32 = 32.0;
const GAP: f32 = 2.0;
const TEXT_PAD: f32 = 12.0;
const UI_TEXT: f32 = 13.0;

/// What a toolbar or bar button does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Tool(Tool),
    Color(usize),
    Stroke(usize),
    Undo,
    Redo,
    Copy,
    Save,
    Close,
    /// The bar's "Buang" and "Batal".
    Discard,
    Keep,
}

/// Where everything sits in the client area (physical px).
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub toolbar: Rect,
    pub canvas: Rect,
    pub footer: Rect,
    pub items: Vec<(Hit, Rect)>,
    /// x of each separator line in the toolbar.
    pub seps: Vec<f32>,
    /// Where the left group ends.
    pub left_end: f32,
}

/// Widths of the text buttons: Salin, Simpan, Tutup, Buang, Batal.
pub type Labels = [f32; 5];

fn text_button_w(label: f32, ui: f32) -> f32 {
    label + 2.0 * TEXT_PAD * ui
}

/// The least client width at which the toolbar's two groups do not touch.
pub fn min_client_w(ui: f32, labels: &Labels) -> f32 {
    let l = layout(10_000.0, 400.0 * ui, ui, labels, false);
    let right: f32 = labels[..3]
        .iter()
        .map(|w| text_button_w(*w, ui) + 8.0 * ui)
        .sum();
    l.left_end + 16.0 * ui + right - 8.0 * ui + MARGIN * ui
}

/// The client area of `cw` × `ch` physical px laid out; `confirm` puts the
/// "Buang perubahan?" bar's buttons in the footer.
pub fn layout(cw: f32, ch: f32, ui: f32, labels: &Labels, confirm: bool) -> Layout {
    let (tb_h, foot_h) = (TB_H * ui, FOOT_H * ui);
    let toolbar = Rect {
        x: 0.0,
        y: 0.0,
        w: cw,
        h: tb_h,
    };
    let footer = Rect {
        x: 0.0,
        y: (ch - foot_h).max(tb_h),
        w: cw,
        h: foot_h,
    };
    let canvas = Rect {
        x: 0.0,
        y: tb_h,
        w: cw,
        h: (ch - tb_h - foot_h).max(0.0),
    };
    let mut items = Vec::new();
    let mut seps = Vec::new();
    let cy = tb_h / 2.0;
    let place = |x: f32, w: f32, h: f32| Rect {
        x,
        y: cy - h / 2.0,
        w,
        h,
    };
    let mut x = MARGIN * ui;
    for tool in Tool::ALL {
        items.push((Hit::Tool(tool), place(x, BTN * ui, BTN * ui)));
        x += (BTN + GAP) * ui;
    }
    let mut sep = |x: &mut f32| {
        *x += 6.0 * ui;
        seps.push(*x);
        *x += 7.0 * ui;
    };
    sep(&mut x);
    for i in 0..COLORS.len() {
        items.push((Hit::Color(i), place(x, SWATCH * ui, BTN * ui)));
        x += (SWATCH + GAP) * ui;
    }
    sep(&mut x);
    for i in 0..STROKES.len() {
        items.push((Hit::Stroke(i), place(x, STROKE_BTN * ui, BTN * ui)));
        x += (STROKE_BTN + GAP) * ui;
    }
    sep(&mut x);
    items.push((Hit::Undo, place(x, BTN * ui, BTN * ui)));
    x += (BTN + GAP) * ui;
    items.push((Hit::Redo, place(x, BTN * ui, BTN * ui)));
    let left_end = x + BTN * ui;
    // Right group, from the edge in: Tutup, Simpan, Salin.
    let mut right = cw - MARGIN * ui;
    for (hit, label) in [Hit::Close, Hit::Save, Hit::Copy]
        .into_iter()
        .zip([labels[2], labels[1], labels[0]])
    {
        let w = text_button_w(label, ui);
        items.push((hit, place(right - w, w, BTN_H * ui)));
        right -= w + 8.0 * ui;
    }
    if confirm {
        let h = 30.0 * ui;
        let fy = footer.y + (footer.h - h) / 2.0;
        let mut right = cw - MARGIN * ui;
        for (hit, label) in [(Hit::Keep, labels[4]), (Hit::Discard, labels[3])] {
            let w = text_button_w(label, ui);
            items.push((
                hit,
                Rect {
                    x: right - w,
                    y: fy,
                    w,
                    h,
                },
            ));
            right -= w + 8.0 * ui;
        }
    }
    Layout {
        toolbar,
        canvas,
        footer,
        items,
        seps,
        left_end,
    }
}

impl Layout {
    /// The button at `(x, y)`. While the bar is up only its two buttons and
    /// the toolbar's Close answer.
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        self.items
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(h, _)| *h)
    }

    pub fn rect_of(&self, hit: Hit) -> Option<Rect> {
        self.items.iter().find(|(h, _)| *h == hit).map(|(_, r)| *r)
    }
}

/// What the footer says while the pointer is over a button: its name and
/// shortcut. The bar's own buttons say nothing.
pub fn tip(hit: Hit) -> Option<String> {
    Some(match hit {
        Hit::Tool(t) => {
            let name = match t {
                Tool::Select => Key::EditSelect,
                Tool::Text => Key::EditText,
                Tool::Rect => Key::EditRect,
                Tool::Line => Key::EditLine,
                Tool::Arrow => Key::EditArrow,
                Tool::Pen => Key::EditPen,
                Tool::Marker => Key::EditMarker,
                Tool::Pixelate => Key::EditPixelate,
            };
            format!("{} ({})", tr(name), t.key())
        }
        Hit::Color(_) => tr(Key::EditColor).to_owned(),
        Hit::Stroke(_) => tr(Key::EditStroke).to_owned(),
        Hit::Undo => format!("{} (Ctrl+Z)", tr(Key::EditUndo)),
        Hit::Redo => format!("{} (Ctrl+Y)", tr(Key::EditRedo)),
        Hit::Copy => format!("{} (Ctrl+C)", tr(Key::EditCopy)),
        Hit::Save => format!("{} (Ctrl+S)", tr(Key::EditSave)),
        Hit::Close => format!("{} (Esc)", tr(Key::EditClose)),
        Hit::Discard | Hit::Keep => return None,
    })
}

/// The client size the picture needs: the picture at `scale` with its
/// margins between the toolbar and the footer, at least `min_w` wide.
pub fn client_size(img: (u32, u32), scale: f32, ui: f32, min_w: f32) -> (f32, f32) {
    let w = (img.0 as f32 * scale + 2.0 * PAD * ui).max(min_w);
    let h = img.1 as f32 * scale + 2.0 * PAD * ui + (TB_H + FOOT_H) * ui;
    (w.ceil(), h.ceil())
}

/// The scale at which the picture shows when the window opens: 100 % when
/// it fits in 85 % of the work area, else shrunk to fit, with room kept for
/// the toolbar and footer.
pub fn open_scale(img: (u32, u32), work: (f32, f32), ui: f32) -> f32 {
    let chrome = (TB_H + FOOT_H + 2.0 * PAD) * ui;
    let h_limit = (work.1 * super::FIT_SHARE).min(work.1 - chrome).max(64.0);
    initial_scale(img, (work.0, h_limit / super::FIT_SHARE))
}

// ---- state ----

/// How the footer's line reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Hint,
    Ok,
    Error,
}

enum Drag {
    /// A new annotation being drawn; `from` is where the press was.
    Draw {
        idx: usize,
        from: Pt,
    },
    Move {
        idx: usize,
        last: Pt,
    },
    Resize {
        idx: usize,
        handle: Handle,
    },
}

struct Editor {
    hwnd: HWND,
    base: Arc<Shot>,
    session: Session,
    gfx: Gfx,
    target: Option<Target>,
    picture: Option<ID2D1Bitmap>,
    pix: Pix,
    dpi: u32,
    client: (u32, u32),
    /// Image px → client px.
    scale: f32,
    view: View,
    layout: Layout,
    labels: Labels,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    drag: Option<Drag>,
    text: Option<TextEdit>,
    footer: Option<(String, Tone)>,
    confirm: bool,
    saving: bool,
    /// The document's revision the running save writes.
    save_rev: u64,
    /// The picture could not be made a bitmap (too big for the device): the
    /// canvas shows without it and the footer says so.
    no_picture: bool,
    /// Lost targets in a row (see `RETRIES`).
    lost: u8,
    demo: bool,
    inbox: Inbox,
    p: WindowPalette,
    /// This editor's capture number (`shot_edit::next_capture_id`): the
    /// island keeps one card per capture.
    capture: u64,
    /// When the picture is copied by itself (`auto`).
    auto: AutoCopy<Prepared>,
    /// Where the auto-copy worker leaves its rendered picture.
    copy_inbox: CopyInbox,
    /// The document revision `note_change` saw last.
    seen_rev: u64,
}

thread_local! {
    static EDITORS: RefCell<HashMap<isize, Editor>> = RefCell::new(HashMap::new());
}

fn with<R>(hwnd: HWND, f: impl FnOnce(&mut Editor) -> R) -> Option<R> {
    EDITORS.with(|m| {
        m.try_borrow_mut()
            .ok()
            .and_then(|mut m| m.get_mut(&(hwnd.0 as isize)).map(f))
    })
}

fn key_down(vk: u16) -> bool {
    unsafe { GetKeyState(i32::from(vk)) < 0 }
}

/// Opens the editor on `shot` (cropped by the capture), centred on the
/// monitor holding `anchor`, in front with the keyboard. `demo` makes Copy
/// inert, Save write to `target/demo-shots/`, and the end of the process
/// when the window closes.
pub fn open(shot: Shot, anchor: (i32, i32), demo: bool) {
    open_with(shot, anchor, demo, Vec::new());
}

/// `open`, with annotations already on the picture (the demo's).
pub fn open_with(shot: Shot, anchor: (i32, i32), demo: bool, doc: Vec<Annotation>) {
    if let Err(e) = try_open(shot, anchor, demo, doc) {
        eprintln!("vale: the screenshot editor could not open: {e}");
        if demo {
            unsafe { PostQuitMessage(0) };
        }
    }
}

fn register(instance: HINSTANCE) -> windows::core::Result<()> {
    unsafe {
        let (icon, small) = crate::ui::app_icon::class_icons(instance);
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            hIcon: icon,
            hIconSm: small,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS,
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

fn try_open(
    shot: Shot,
    anchor: (i32, i32),
    demo: bool,
    doc: Vec<Annotation>,
) -> windows::core::Result<()> {
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    register(instance)?;
    let (work, dpi) = unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: anchor.0,
                y: anchor.1,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        (info.rcWork, dx.max(96))
    };
    let ui = dpi as f32 / 96.0;
    let gfx = Gfx::new()?;
    let labels = measure_labels(&gfx, ui);
    let img = (shot.w, shot.h);
    let work_size = (
        (work.right - work.left) as f32,
        (work.bottom - work.top) as f32,
    );
    let scale = open_scale(img, work_size, ui);
    let (cw, ch) = client_size(img, scale, ui, min_client_w(ui, &labels));
    let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: cw as i32,
        bottom: ch as i32,
    };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WINDOW_EX_STYLE(0), dpi);
    }
    let (ow, oh) = (rc.right - rc.left, rc.bottom - rc.top);
    let x = work.left + ((work.right - work.left) - ow).max(0) / 2;
    let y = work.top + ((work.bottom - work.top) - oh).max(0) / 2;
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            &HSTRING::from(tr(Key::EditTitle)),
            style,
            x,
            y,
            ow,
            oh,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    let p = theme::current_window();
    let real_dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let ui = real_dpi as f32 / 96.0;
    let labels = measure_labels(&gfx, ui);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let client = (
        (client.right - client.left).max(1) as u32,
        (client.bottom - client.top).max(1) as u32,
    );
    let lay = layout(client.0 as f32, client.1 as f32, ui, &labels, false);
    let mut e = Editor {
        hwnd,
        base: Arc::new(shot),
        session: Session::with_doc(doc),
        gfx,
        target: None,
        picture: None,
        pix: Pix::default(),
        dpi: real_dpi,
        client,
        scale,
        view: View {
            scale,
            ox: 0.0,
            oy: 0.0,
        },
        layout: lay,
        labels,
        hover: None,
        pressed: None,
        drag: None,
        text: None,
        footer: None,
        confirm: false,
        saving: false,
        save_rev: 0,
        no_picture: false,
        lost: 0,
        demo,
        inbox: Arc::new(Mutex::new(Vec::new())),
        p,
        capture: super::next_capture_id(),
        // Latched now: a change in Settings meanwhile applies to the next
        // editor. A demo never touches the clipboard.
        auto: AutoCopy::new(super::auto_copy_setting() && !demo),
        copy_inbox: Arc::new(Mutex::new(Vec::new())),
        seen_rev: 0,
    };
    recompute_view(&mut e);
    let stored = EDITORS.with(|m| match m.try_borrow_mut() {
        Ok(mut m) => {
            m.insert(hwnd.0 as isize, e);
            true
        }
        Err(_) => false,
    });
    if !stored {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(Error::empty());
    }
    apply_frame(hwnd, &p);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
    // The pick itself is copied: a worker renders it (the window never
    // waits); the clipboard is written when it rings back.
    if with(hwnd, |e| e.auto.wants_first()) == Some(true) {
        start_worker(hwnd);
    }
    Ok(())
}

fn measure_labels(gfx: &Gfx, ui: f32) -> Labels {
    [
        Key::EditCopy,
        Key::EditSave,
        Key::EditClose,
        Key::EditDiscard,
        Key::EditKeep,
    ]
    .map(|k| gfx.measure(tr(k), UI_TEXT * ui, 500).0)
}

/// Dark or light title bar in the window's own colours, rounded corners.
fn apply_frame(hwnd: HWND, p: &WindowPalette) {
    let dark = i32::from(!p.light);
    let caption = colorref(p.bg).0;
    let text = colorref(p.text).0;
    let round = DWMWCP_ROUND.0;
    unsafe {
        let set = |attr, v: *const c_void, n: usize| {
            let _ = DwmSetWindowAttribute(hwnd, attr, v, n as u32);
        };
        set(
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const dark).cast(),
            size_of::<i32>(),
        );
        set(
            DWMWA_CAPTION_COLOR,
            (&raw const caption).cast(),
            size_of::<u32>(),
        );
        set(DWMWA_TEXT_COLOR, (&raw const text).cast(), size_of::<u32>());
        set(
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&raw const round).cast(),
            size_of::<i32>(),
        );
    }
}

/// Restyles every open editor for the window palette in effect (Windows
/// switched between light and dark, or the window mode, accent or base colour
/// changed): the stored palette, the title bar and a repaint. The editors live
/// on the UI thread (the capture overlay opens them there), so this runs on
/// it. The DWM calls can re-enter `wnd_proc`, so they run after the map's
/// borrow is released; a map that is busy (we are inside one of its handlers)
/// is skipped, and its editor catches up on the next change.
pub fn restyle_open() {
    let p = theme::current_window();
    let open: Vec<HWND> = EDITORS.with(|m| match m.try_borrow_mut() {
        Ok(mut m) => m
            .iter_mut()
            .map(|(h, e)| {
                e.p = p;
                HWND(*h as *mut c_void)
            })
            .collect(),
        Err(_) => Vec::new(),
    });
    for hwnd in open {
        apply_frame(hwnd, &p);
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

/// The island is ending: every open editor window is destroyed (its
/// `WM_DESTROY` frees the picture and the Direct2D objects), and whatever is
/// still registered after that is dropped here, so nothing is left to the
/// thread-local destructors during process exit.
pub fn release() {
    let open: Vec<HWND> = EDITORS.with(|m| match m.try_borrow() {
        Ok(m) => m.keys().map(|h| HWND(*h as *mut c_void)).collect(),
        Err(_) => Vec::new(),
    });
    for hwnd in open {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    let left = EDITORS.with(|m| match m.try_borrow_mut() {
        Ok(mut m) => std::mem::take(&mut *m),
        Err(_) => HashMap::new(),
    });
    for (_, mut e) in left {
        if let Some(t) = e.text.take() {
            text::free_gdi(&t);
        }
    }
}

/// The picture centred in the canvas.
fn recompute_view(e: &mut Editor) {
    let ui = e.dpi as f32 / 96.0;
    e.layout = layout(
        e.client.0 as f32,
        e.client.1 as f32,
        ui,
        &e.labels,
        e.confirm,
    );
    let c = e.layout.canvas;
    let v = View::centered(e.scale, (e.base.w, e.base.h), (c.w, c.h));
    e.view = View {
        scale: e.scale,
        ox: c.x + v.ox.max(0.0),
        oy: c.y + v.oy.max(0.0),
    };
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn say(e: &mut Editor, text: impl Into<String>, tone: Tone) {
    e.footer = Some((text.into(), tone));
}

// ---- actions ----

fn do_hit(hwnd: HWND, hit: Hit) {
    commit_text(hwnd);
    match hit {
        Hit::Tool(t) => {
            with(hwnd, |e| {
                e.session.tool = t;
                if t != Tool::Select {
                    e.session.selected = None;
                }
            });
        }
        Hit::Color(i) => {
            with(hwnd, |e| {
                e.session.pick_color(COLORS[i.min(COLORS.len() - 1)]);
            });
        }
        Hit::Stroke(i) => {
            with(hwnd, |e| {
                e.session.pick_stroke(i);
            });
        }
        Hit::Undo => {
            with(hwnd, |e| e.session.undo());
        }
        Hit::Redo => {
            with(hwnd, |e| e.session.redo());
        }
        Hit::Copy => do_copy(hwnd),
        Hit::Save => do_save(hwnd),
        Hit::Close => request_close(hwnd),
        Hit::Discard => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        Hit::Keep => {
            with(hwnd, |e| {
                e.confirm = false;
                recompute_view(e);
            });
        }
    }
    invalidate(hwnd);
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = SetFocus(Some(hwnd));
        }
    }
}

fn do_copy(hwnd: HWND) {
    let job = with(hwnd, |e| {
        if e.demo {
            say(e, tr(Key::EditDemoNoClipboard), Tone::Hint);
            None
        } else {
            Some((
                e.base.clone(),
                e.session.doc.clone(),
                e.session.revision(),
                e.capture,
                e.auto.enabled(),
            ))
        }
    })
    .flatten();
    let Some((base, doc, rev, capture, managed)) = job else {
        return;
    };
    // The clipboard is held for a moment; rendering happens before.
    let seq = export::copy(hwnd, &base, &doc, capture, managed);
    with(hwnd, |e| {
        if let Some(seq) = seq {
            e.session.mark_clean();
            // The clipboard is this editor's again: auto-copy resumes (it
            // may have stopped because somebody else copied).
            e.auto.on_manual_copy(seq, rev);
            say(e, tr(Key::EditCopied), Tone::Ok);
        } else {
            say(e, tr(Key::EditCopyFailed), Tone::Error);
        }
    });
    if seq.is_some() {
        kill_auto_timers(hwnd);
    }
}

// ---- auto-copy ----

/// Kills both auto-copy timers and tells the model, so a retry waiting for
/// a timer that is gone can never hold up the next render.
fn kill_auto_timers(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), ID_AUTO_DEBOUNCE);
        let _ = KillTimer(Some(hwnd), ID_AUTO_RETRY);
    }
    with(hwnd, |e| e.auto.timers_cancelled());
}

fn arm_debounce(hwnd: HWND) {
    // The same id again restarts it: that is the debounce.
    unsafe {
        SetTimer(Some(hwnd), ID_AUTO_DEBOUNCE, DEBOUNCE_MS, None);
    }
}

/// After anything that can end an action: when the document's revision moved
/// since last time, the debounce starts over. Cheap (a number compared).
fn note_change(hwnd: HWND) {
    let arm = with(hwnd, |e| {
        let rev = e.session.revision();
        if rev == e.seen_rev {
            return false;
        }
        e.seen_rev = rev;
        e.auto.on_change()
    })
    .unwrap_or(false);
    if arm {
        arm_debounce(hwnd);
    }
}

/// Starts a worker rendering the current document for the clipboard, unless
/// rule 3 already says the clipboard is not ours any more. Never called with
/// an EDIT open or a drag running (the document is not settled then); the
/// callers check.
fn start_worker(hwnd: HWND) {
    let job = with(hwnd, |e| {
        if e.auto.may_write(crate::clipboard::win::sequence_now()) == WriteGate::NotOurs {
            return None;
        }
        let rev = e.session.revision();
        e.auto.begin_worker(rev);
        Some((
            e.base.clone(),
            e.session.doc.clone(),
            rev,
            e.copy_inbox.clone(),
        ))
    })
    .flatten();
    let Some((base, doc, rev, inbox)) = job else {
        return;
    };
    if !export::spawn_auto_copy(base, doc, rev, inbox, hwnd, WM_APP_COPIED) {
        with(hwnd, |e| {
            e.auto.worker_failed(rev);
            say(e, tr(Key::EditAutoCopyFailed), Tone::Error);
        });
        invalidate(hwnd);
    }
}

/// An EDIT is open or a drag is running.
fn editing(e: &Editor) -> bool {
    e.text.is_some() || e.drag.is_some()
}

/// The debounce fired.
fn on_debounce(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), ID_AUTO_DEBOUNCE);
    }
    let action = with(hwnd, |e| {
        let busy = editing(e);
        e.auto.on_timer(e.session.revision(), busy)
    });
    match action {
        Some(TimerAction::Rearm) => arm_debounce(hwnd),
        Some(TimerAction::Start) => start_worker(hwnd),
        _ => {}
    }
}

/// The worker rang: the rendered pictures are written when still current.
fn on_copied(hwnd: HWND) {
    let ready: Vec<CopyReady> = with(hwnd, |e| {
        e.copy_inbox
            .lock()
            .map(|mut l| std::mem::take(&mut *l))
            .unwrap_or_default()
    })
    .unwrap_or_default();
    for r in ready {
        handle_ready(hwnd, r);
    }
}

fn handle_ready(hwnd: HWND, ready: CopyReady) {
    enum Next {
        Nothing,
        Restart,
        Put(Prepared, u64),
    }
    let next = with(hwnd, |e| {
        let current = e.session.revision();
        match e.auto.on_result(ready.rev, current) {
            ResultAction::Drop => Next::Nothing,
            ResultAction::Restart => Next::Restart,
            ResultAction::Write => match ready.prepared {
                None => {
                    say(e, tr(Key::EditAutoCopyFailed), Tone::Error);
                    Next::Nothing
                }
                Some(p) => {
                    match e.auto.may_write(crate::clipboard::win::sequence_now()) {
                        WriteGate::Write => Next::Put(p, ready.rev),
                        // Not ours: stopped, silently.
                        WriteGate::NotOurs => Next::Nothing,
                    }
                }
            },
        }
    });
    match next {
        Some(Next::Put(p, rev)) => put_prepared(hwnd, p, rev),
        Some(Next::Restart) => {
            // The document moved on while the worker ran: render the
            // current one, once nothing is being typed or dragged.
            let busy = with(hwnd, |e| {
                let busy = editing(e);
                if busy {
                    e.auto.on_change();
                }
                busy
            })
            .unwrap_or(true);
            if busy {
                arm_debounce(hwnd);
            } else {
                start_worker(hwnd);
            }
        }
        _ => {}
    }
    invalidate(hwnd);
}

/// Writes `prepared` (the document at `rev`) to the clipboard, only while it
/// still holds this editor's last write. Never inside a borrow of the
/// editor (`EmptyClipboard` may message another editor on this thread).
fn put_prepared(hwnd: HWND, prepared: Prepared, rev: u64) {
    let Some((expected, capture, managed)) =
        with(hwnd, |e| (e.auto.expected(), e.capture, e.auto.enabled()))
    else {
        return;
    };
    let result = export::put(hwnd, &prepared, expected, capture, managed);
    let retry = with(hwnd, |e| match result {
        Put::Written(seq) => {
            e.auto.on_written(seq, rev);
            e.session.mark_clean_at(rev);
            say(e, tr(Key::EditCopied), Tone::Ok);
            None
        }
        Put::NotOurs => {
            e.auto.stop();
            None
        }
        Put::Busy | Put::Emptied(_) => {
            if let Put::Emptied(seq) = result {
                // The empty clipboard is still this editor's.
                e.auto.on_emptied(seq);
            }
            match e.auto.on_busy(rev, prepared) {
                Step::Again(ms) => Some(ms),
                Step::GiveUp => {
                    say(e, tr(Key::EditAutoCopyFailed), Tone::Error);
                    None
                }
            }
        }
    })
    .flatten();
    if let Some(ms) = retry {
        unsafe {
            SetTimer(Some(hwnd), ID_AUTO_RETRY, ms, None);
        }
    }
    invalidate(hwnd);
}

/// The retry timer fired.
fn on_retry(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(Some(hwnd), ID_AUTO_RETRY);
    }
    let job = with(hwnd, |e| {
        let (rev, p) = e.auto.take_retry(e.session.revision())?;
        // Rule 3 again: the clipboard may have changed meanwhile.
        match e.auto.may_write(crate::clipboard::win::sequence_now()) {
            WriteGate::Write => Some((p, rev)),
            WriteGate::NotOurs => None,
        }
    })
    .flatten();
    if let Some((p, rev)) = job {
        put_prepared(hwnd, p, rev);
    }
}

/// Closing: when the clipboard may lack the current picture (a change
/// still waiting for its debounce, a render in flight, a retry pending), it
/// is rendered and written now, on this thread, like Copy. A picture that is
/// on the clipboard counts as saved, so the discard bar only shows when it
/// is neither there nor in a file.
fn flush_on_close(hwnd: HWND) {
    let job = with(hwnd, |e| {
        if !e.auto.enabled() {
            return None;
        }
        // Closed mid-drag (Alt+F4, capture lost): the action ends as a
        // release would, so the document and its revision agree.
        if e.drag.is_some() {
            finish_drag(e);
        }
        let rev = e.session.revision();
        if !e.auto.needs_flush(rev) {
            return None;
        }
        if e.auto.may_write(crate::clipboard::win::sequence_now()) == WriteGate::NotOurs {
            return None;
        }
        Some((e.base.clone(), e.session.doc.clone(), rev))
    })
    .flatten();
    kill_auto_timers(hwnd);
    let Some((base, doc, rev)) = job else {
        return;
    };
    let Some(prepared) = export::prepare(&base, &doc) else {
        return;
    };
    let Some((expected, capture)) = with(hwnd, |e| (e.auto.expected(), e.capture)) else {
        return;
    };
    let result = export::put(hwnd, &prepared, expected, capture, true);
    let again = with(hwnd, |e| match result {
        Put::Written(seq) => {
            e.auto.on_written(seq, rev);
            e.session.mark_clean_at(rev);
            false
        }
        Put::NotOurs => {
            e.auto.stop();
            false
        }
        Put::Busy | Put::Emptied(_) => {
            if let Put::Emptied(seq) = result {
                // The empty clipboard is still this editor's.
                e.auto.on_emptied(seq);
            }
            // The timers were killed above: the debounce is due again.
            e.auto.on_change()
        }
    })
    .unwrap_or(false);
    if again {
        // The window stays (the bar asks): the debounce tries again.
        arm_debounce(hwnd);
    }
}

fn do_save(hwnd: HWND) {
    let job = with(hwnd, |e| {
        if e.saving {
            return None;
        }
        e.saving = true;
        e.save_rev = e.session.revision();
        say(e, tr(Key::EditSaving), Tone::Hint);
        let dir = e.demo.then(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .join("target")
                .join("demo-shots")
        });
        Some((e.base.clone(), e.session.doc.clone(), dir, e.inbox.clone()))
    })
    .flatten();
    let Some((base, doc, dir, inbox)) = job else {
        return;
    };
    if !export::spawn_save(base, doc, dir, inbox, hwnd, WM_APP_SAVED) {
        with(hwnd, |e| {
            e.saving = false;
            say(e, tr(Key::ShotSaveFailed), Tone::Error);
        });
    }
}

fn on_saved(hwnd: HWND) {
    with(hwnd, |e| {
        let results: Vec<SaveResult> = e
            .inbox
            .lock()
            .map(|mut l| std::mem::take(&mut *l))
            .unwrap_or_default();
        for r in results {
            e.saving = false;
            match r {
                SaveResult::Saved(path) => {
                    // Clean only if nothing changed after Save was pressed:
                    // a later change is not in the file.
                    e.session.mark_clean_at(e.save_rev);
                    let dir = path.parent().map(crate::screenshot::folder_label);
                    let msg = format!("{} · {}", tr(Key::ShotSaved), dir.unwrap_or_default());
                    say(e, msg, Tone::Ok);
                    // The notch says it too (not in the demo).
                    if !e.demo {
                        super::tell_saved(Some(path.clone()));
                    }
                }
                SaveResult::Failed => {
                    let msg = format!(
                        "{} · {}",
                        tr(Key::ShotSaveFailed),
                        tr(Key::ShotMaybeBlocked)
                    );
                    say(e, msg, Tone::Error);
                }
            }
        }
    });
    invalidate(hwnd);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseAction {
    Destroy,
    Ask,
}

/// Closing with nothing changed destroys at once; with changes it asks.
fn close_action(dirty: bool) -> CloseAction {
    if dirty {
        CloseAction::Ask
    } else {
        CloseAction::Destroy
    }
}

/// The keys the discard bar answers: Enter discards, Esc keeps editing.
fn confirm_key(vk: u16) -> Option<Hit> {
    if vk == VK_RETURN.0 {
        Some(Hit::Discard)
    } else if vk == VK_ESCAPE.0 {
        Some(Hit::Keep)
    } else {
        None
    }
}

/// The window's close: asks first when something changed. Also reached from
/// WM_CLOSE (the X button, Alt+F4); with the bar already up it stays up.
fn request_close(hwnd: HWND) {
    // First the picture goes to the clipboard when auto-copy is behind: what
    // is there counts as kept, so the bar below asks only about a picture
    // that is neither copied nor saved.
    flush_on_close(hwnd);
    let ask = with(hwnd, |e| {
        if close_action(e.session.dirty()) == CloseAction::Ask {
            e.confirm = true;
            recompute_view(e);
            true
        } else {
            false
        }
    });
    if ask == Some(true) {
        // The window, not an EDIT, holds the keys the bar answers.
        let editing = with(hwnd, |e| e.text.is_some()).unwrap_or(false);
        if !editing {
            unsafe {
                let _ = SetFocus(Some(hwnd));
            }
        }
        invalidate(hwnd);
    } else {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

/// Esc: first the text being typed, then the selection, then the window.
fn on_escape(hwnd: HWND) {
    enum Step {
        Text,
        Bar,
        Select,
        Close,
    }
    let step = with(hwnd, |e| {
        if e.text.is_some() {
            Step::Text
        } else if e.confirm {
            Step::Bar
        } else if e.session.selected.is_some() {
            e.session.selected = None;
            Step::Select
        } else {
            Step::Close
        }
    });
    match step {
        Some(Step::Text) => cancel_text(hwnd),
        Some(Step::Bar) => do_hit(hwnd, Hit::Keep),
        Some(Step::Select) => invalidate(hwnd),
        Some(Step::Close) => request_close(hwnd),
        None => {}
    }
}

// ---- text ----

/// Puts a native EDIT at `pos` (image px): new text, or `index`'s text.
fn begin_text(hwnd: HWND, pos: Pt, index: Option<usize>) {
    commit_text(hwnd);
    struct Plan {
        client_pos: (f32, f32),
        pos: Pt,
        initial: String,
        size: f32,
        color: u32,
        view_scale: f32,
        ui: f32,
    }
    let plan = with(hwnd, |e| {
        let (initial, size, color, pos) = match index.and_then(|i| e.session.doc.get(i)) {
            Some(Annotation::Text {
                text,
                size,
                color,
                pos,
            }) => (text.clone(), *size, *color, *pos),
            _ => (
                String::new(),
                text_size(e.session.stroke),
                e.session.color,
                pos,
            ),
        };
        let (x, y) = e.view.to_view(pos);
        Some(Plan {
            client_pos: (x, y),
            pos,
            initial,
            size,
            color,
            view_scale: e.view.scale,
            ui: e.dpi as f32 / 96.0,
        })
    })
    .flatten();
    let Some(plan) = plan else { return };
    let px = (plan.size * plan.view_scale).round().max(8.0 * plan.ui);
    let Some(mut t) = text::create(
        hwnd,
        plan.client_pos,
        &plan.initial,
        px,
        plan.color,
        plan.ui,
    ) else {
        return;
    };
    t.index = index;
    t.pos = plan.pos;
    t.size = plan.size;
    let edit = t.edit;
    let mut slot = Some(t);
    with(hwnd, |e| e.text = slot.take());
    if let Some(t) = slot {
        // The window's state is gone (a re-entrant borrow): take the EDIT
        // back down.
        text::destroy(t);
        return;
    }
    fit_edit(hwnd);
    unsafe {
        let _ = SetFocus(Some(edit));
    }
    invalidate(hwnd);
}

/// Grows the EDIT with its text.
fn fit_edit(hwnd: HWND) {
    let plan = with(hwnd, |e| {
        let t = e.text.as_ref()?;
        let ui = e.dpi as f32 / 96.0;
        Some((t.edit, t.size * e.view.scale, ui))
    })
    .flatten();
    let Some((edit, px, ui)) = plan else { return };
    text::fit(hwnd, edit, px, ui, |s| {
        with(hwnd, |e| e.gfx.measure(s, px, 400)).unwrap_or((0.0, 0.0))
    });
}

/// Takes the typed text into the picture (an empty text is dropped) and
/// removes the EDIT. True when an EDIT was open.
fn commit_text(hwnd: HWND) -> bool {
    finish_text(hwnd, true)
}

fn cancel_text(hwnd: HWND) {
    finish_text(hwnd, false);
}

fn finish_text(hwnd: HWND, keep: bool) -> bool {
    let Some(Some(t)) = with(hwnd, |e| e.text.take()) else {
        return false;
    };
    let typed = edit_text(t.edit);
    let (index, pos) = (t.index, t.pos);
    text::destroy(t);
    with(hwnd, |e| {
        if keep {
            match index {
                Some(i) => {
                    e.session.set_text(i, &typed);
                }
                None => {
                    // In the colour and size it was typed in; blank is dropped.
                    e.session.add_text(pos, &typed);
                }
            }
        }
    });
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = SetFocus(Some(hwnd));
        }
    }
    invalidate(hwnd);
    true
}

// ---- mouse ----

fn canvas_point(e: &Editor, x: f32, y: f32) -> Pt {
    let p = e.view.to_image(x, y);
    Pt::new(
        p.x.clamp(0.0, e.base.w as f32),
        p.y.clamp(0.0, e.base.h as f32),
    )
}

fn xy(lparam: LPARAM) -> (f32, f32) {
    (
        f32::from(lparam.0 as i16),
        f32::from((lparam.0 >> 16) as i16),
    )
}

fn on_down(hwnd: HWND, lparam: LPARAM, double: bool) {
    let (x, y) = xy(lparam);
    let edit_was_open = commit_text(hwnd);
    let toolbar_hit = with(hwnd, |e| {
        let hit = e.layout.hit(x, y);
        if let Some(h) = hit {
            // While the bar is up the picture is inert.
            e.pressed = Some(h);
        }
        hit
    })
    .flatten();
    unsafe {
        let _ = SetCapture(hwnd);
    }
    if toolbar_hit.is_some() {
        invalidate(hwnd);
        return;
    }
    // Text: a click places the EDIT (after this borrow ends).
    let place = with(hwnd, |e| {
        if e.confirm || !e.layout.canvas.contains(x, y) {
            return None;
        }
        let p = canvas_point(e, x, y);
        let ui = e.dpi as f32 / 96.0;
        let tol = 6.0 * ui / e.view.scale;
        match e.session.tool {
            Tool::Select => {
                // A handle of the selected one, else the topmost under it.
                if let Some(i) = e.session.selected
                    && let Some(h) = e.session.doc.get(i).and_then(|a| a.handle_at(p, tol))
                {
                    e.session.begin();
                    e.drag = Some(Drag::Resize { idx: i, handle: h });
                    return None;
                }
                match e.session.hit(p, tol) {
                    Some(i) => {
                        e.session.selected = Some(i);
                        if let Some(c) = e.session.doc[i].color() {
                            e.session.color = c;
                        }
                        if double && matches!(e.session.doc[i], Annotation::Text { .. }) {
                            return Some((p, Some(i)));
                        }
                        e.session.begin();
                        e.drag = Some(Drag::Move { idx: i, last: p });
                    }
                    None => e.session.selected = None,
                }
                None
            }
            Tool::Text => match text_click_action(edit_was_open, Tool::Text) {
                TextClick::Place => Some((p, None)),
                // The click only finished the open text.
                TextClick::CommitOnly | TextClick::NotText => None,
            },
            tool => {
                let style = e.session.style();
                let ann = match tool {
                    Tool::Rect => Annotation::Rect {
                        r: FRect::new(p.x, p.y, 0.0, 0.0),
                        style,
                    },
                    Tool::Line => Annotation::Line { a: p, b: p, style },
                    Tool::Arrow => Annotation::Arrow { a: p, b: p, style },
                    Tool::Pen => Annotation::Pen {
                        pts: vec![p],
                        style,
                    },
                    Tool::Marker => Annotation::Marker {
                        pts: vec![p],
                        style,
                    },
                    Tool::Pixelate | Tool::Select | Tool::Text => Annotation::Pixelate {
                        r: FRect::new(p.x, p.y, 0.0, 0.0),
                    },
                };
                e.session.begin();
                e.session.doc.push(ann);
                e.session.selected = None;
                e.drag = Some(Drag::Draw {
                    idx: e.session.doc.len() - 1,
                    from: p,
                });
                None
            }
        }
    })
    .flatten();
    if let Some((p, index)) = place {
        unsafe {
            let _ = ReleaseCapture();
        }
        begin_text(hwnd, p, index);
        return;
    }
    invalidate(hwnd);
}

/// Ends the drag in progress as a release would: a click with a drawing tool
/// leaves nothing behind, and the step's snapshot is closed. False when there
/// was no drag.
fn finish_drag(e: &mut Editor) -> bool {
    let Some(drag) = e.drag.take() else {
        return false;
    };
    if let Drag::Draw { idx, .. } = drag
        && e.session
            .doc
            .get(idx)
            .is_some_and(Annotation::is_degenerate)
    {
        e.session.doc.remove(idx);
    }
    e.session.end();
    true
}

fn on_move(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    const MK_LBUTTON: usize = 0x0001;
    if wparam.0 & MK_LBUTTON == 0 {
        // The release went elsewhere (capture lost): the drag is over.
        if with(hwnd, finish_drag) == Some(true) {
            invalidate(hwnd);
        }
    }
    let (x, y) = xy(lparam);
    let changed = with(hwnd, |e| {
        let mut changed = false;
        let hover = e.layout.hit(x, y);
        if hover != e.hover {
            e.hover = hover;
            changed = true;
        }
        let Some(drag) = e.drag.as_ref() else {
            return changed;
        };
        let p = canvas_point(e, x, y);
        match drag {
            Drag::Draw { idx, from } => {
                let (idx, from) = (*idx, *from);
                if let Some(a) = e.session.doc.get_mut(idx) {
                    match a {
                        Annotation::Rect { r, .. } | Annotation::Pixelate { r } => {
                            *r = FRect::between(from, p);
                        }
                        Annotation::Arrow { b, .. } | Annotation::Line { b, .. } => *b = p,
                        Annotation::Pen { pts, .. } | Annotation::Marker { pts, .. } => {
                            let far = pts.last().is_none_or(|l| {
                                (l.x - p.x).hypot(l.y - p.y) >= 1.5 / e.view.scale.max(0.05)
                            });
                            if far {
                                pts.push(p);
                            }
                        }
                        Annotation::Text { .. } => {}
                    }
                }
            }
            Drag::Move { idx, last } => {
                let (idx, last) = (*idx, *last);
                if let Some(a) = e.session.doc.get_mut(idx) {
                    a.move_by(p.x - last.x, p.y - last.y);
                }
                e.drag = Some(Drag::Move { idx, last: p });
            }
            Drag::Resize { idx, handle } => {
                let (idx, handle) = (*idx, *handle);
                if let Some(a) = e.session.doc.get_mut(idx) {
                    a.resize(handle, p);
                }
            }
        }
        true
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

fn on_up(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = xy(lparam);
    let action = with(hwnd, |e| {
        let pressed = e.pressed.take();
        if finish_drag(e) {
            return None;
        }
        let hit = e.layout.hit(x, y)?;
        // The same button as the press; with the bar up, only its own.
        let live = !e.confirm || matches!(hit, Hit::Keep | Hit::Discard);
        (pressed == Some(hit) && live).then_some(hit)
    })
    .flatten();
    // Released after the state above is settled: WM_CAPTURECHANGED finds no
    // drag and no press left to cancel.
    unsafe {
        let _ = ReleaseCapture();
    }
    if let Some(hit) = action {
        // A click on a button the bar leaves live: only its own and Close.
        do_hit(hwnd, hit);
        return;
    }
    invalidate(hwnd);
}

fn on_key(hwnd: HWND, vk: u16) {
    let ctrl = key_down(VK_CONTROL.0);
    let shift = key_down(VK_SHIFT.0);
    // Typing goes to the EDIT; mid-drag, an edit or undo would land under the
    // drag's pending snapshot.
    let busy = with(hwnd, |e| e.text.is_some() || e.drag.is_some()).unwrap_or(false);
    if busy {
        return;
    }
    // While the bar asks, only Enter and Esc count.
    if with(hwnd, |e| e.confirm).unwrap_or(false) {
        if let Some(hit) = confirm_key(vk) {
            do_hit(hwnd, hit);
        }
        return;
    }
    if vk == VK_ESCAPE.0 {
        on_escape(hwnd);
        return;
    }
    if ctrl {
        match vk {
            0x5A => do_hit(hwnd, if shift { Hit::Redo } else { Hit::Undo }),
            0x59 => do_hit(hwnd, Hit::Redo),
            0x43 => do_hit(hwnd, Hit::Copy),
            0x53 => do_hit(hwnd, Hit::Save),
            _ => {}
        }
        return;
    }
    if vk == VK_DELETE.0 {
        with(hwnd, |e| e.session.delete_selected());
        invalidate(hwnd);
        return;
    }
    if (0x41..=0x5A).contains(&vk)
        && !key_down(0x12)
        && let Some(tool) = Tool::from_key(char::from(vk as u8))
    {
        do_hit(hwnd, Hit::Tool(tool));
    }
}

// ---- window procedure ----

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe {
                BeginPaint(hwnd, &mut ps);
            }
            with(hwnd, |e| match draw(e) {
                Ok(()) => e.lost = 0,
                Err(err) if err.code() == D2DERR_RECREATE_TARGET => {
                    // A lost device: made again on the next paint, a few
                    // times in a row at most.
                    e.target = None;
                    e.picture = None;
                    e.pix.clear();
                    if e.lost < RETRIES {
                        e.lost += 1;
                        invalidate(hwnd);
                    }
                }
                Err(err) => {
                    // Anything else would fail again: no repaint loop, the
                    // next change tries once more.
                    eprintln!("vale: the screenshot editor could not draw: {err}");
                }
            });
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let (w, h) = (
                (lparam.0 & 0xFFFF) as u32,
                ((lparam.0 >> 16) & 0xFFFF) as u32,
            );
            with(hwnd, |e| {
                e.client = (w.max(1), h.max(1));
                if let Some(t) = &e.target {
                    t.resize(w, h);
                }
                recompute_view(e);
            });
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let min = with(hwnd, |e| {
                let ui = e.dpi as f32 / 96.0;
                (min_client_w(ui, &e.labels), 300.0 * ui)
            });
            if let Some((w, h)) = min {
                let mut frame = RECT {
                    left: 0,
                    top: 0,
                    right: w as i32,
                    bottom: h as i32,
                };
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
                unsafe {
                    let _ = AdjustWindowRectExForDpi(
                        &mut frame,
                        WS_OVERLAPPEDWINDOW,
                        false,
                        WINDOW_EX_STYLE(0),
                        dpi,
                    );
                    let info = &mut *(lparam.0 as *mut MINMAXINFO);
                    info.ptMinTrackSize.x = frame.right - frame.left;
                    info.ptMinTrackSize.y = frame.bottom - frame.top;
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xFFFF) as u32;
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            with(hwnd, |e| {
                e.dpi = dpi.max(96);
                let ui = e.dpi as f32 / 96.0;
                e.labels = measure_labels(&e.gfx, ui);
                recompute_view(e);
            });
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            on_down(hwnd, lparam, false);
            note_change(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            on_down(hwnd, lparam, true);
            note_change(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            on_move(hwnd, wparam, lparam);
            // A move without the button ends a drag whose release was lost.
            note_change(hwnd);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            // Capture lost mid-drag (Alt+Tab, Win, another window): the
            // release will not come here, so end the drag and the press now.
            if lparam.0 != hwnd.0 as isize {
                let changed = with(hwnd, |e| {
                    let pressed = e.pressed.take().is_some();
                    finish_drag(e) || pressed
                });
                if changed == Some(true) {
                    invalidate(hwnd);
                }
                note_change(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_up(hwnd, lparam);
            note_change(hwnd);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            const HTCLIENT: isize = 1;
            let over_canvas = if lparam.0 & 0xFFFF == HTCLIENT {
                with(hwnd, |e| {
                    let mut at = POINT::default();
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut at);
                        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut at);
                    }
                    let (x, y) = (at.x as f32, at.y as f32);
                    if e.confirm || !e.layout.canvas.contains(x, y) {
                        return None;
                    }
                    Some(e.session.tool)
                })
                .flatten()
            } else {
                None
            };
            match over_canvas {
                Some(tool) => {
                    let cursor = match tool {
                        Tool::Select => IDC_ARROW,
                        Tool::Text => IDC_IBEAM,
                        _ => IDC_CROSS,
                    };
                    unsafe {
                        if let Ok(c) = LoadCursorW(None, cursor) {
                            SetCursor(Some(c));
                        }
                    }
                    LRESULT(1)
                }
                None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        WM_KEYDOWN => {
            on_key(hwnd, (wparam.0 & 0xFFFF) as u16);
            note_change(hwnd);
            LRESULT(0)
        }
        WM_APP_TEXT => {
            let current = with(hwnd, |e| e.text.as_ref().map(|t| t.edit.0 as isize)).flatten();
            if !text_msg_is_current(current, lparam.0) {
                // From an EDIT already committed or cancelled.
                return LRESULT(0);
            }
            if wparam.0 == 1 {
                commit_text(hwnd);
            } else {
                cancel_text(hwnd);
            }
            note_change(hwnd);
            LRESULT(0)
        }
        WM_APP_SAVED => {
            on_saved(hwnd);
            LRESULT(0)
        }
        WM_APP_COPIED => {
            on_copied(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                ID_AUTO_DEBOUNCE => on_debounce(hwnd),
                ID_AUTO_RETRY => on_retry(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            // EN_CHANGE of the text EDIT: grow it with its text.
            if text::is_change(wparam) {
                fit_edit(hwnd);
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let brush = with(hwnd, |e| {
                e.text.as_ref().map(|t| text::ctl_color(t, wparam))
            })
            .flatten();
            match brush {
                Some(b) => b,
                None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        WM_CLOSE => {
            commit_text(hwnd);
            // The committed text is a change; the flush in `request_close`
            // writes it (and kills the debounce this arms).
            note_change(hwnd);
            request_close(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            // Free the picture, the annotations and the target at once.
            let removed = EDITORS.with(|m| {
                m.try_borrow_mut()
                    .ok()
                    .and_then(|mut m| m.remove(&(hwnd.0 as isize)))
            });
            if let Some(mut e) = removed {
                if let Some(t) = e.text.take() {
                    text::free_gdi(&t);
                }
                let demo = e.demo;
                drop(e);
                if demo {
                    unsafe { PostQuitMessage(0) };
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// ---- drawing ----

fn rgba(packed: u32) -> (u32, f32) {
    (packed >> 8, (packed & 0xFF) as f32 / 255.0)
}

/// The stages of one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Canvas,
    ToolbarBack,
    FooterBack,
    Buttons,
    FooterText,
}

/// Direct2D paints in call order, so a region's background has to come before
/// the buttons that sit in it (the discard bar's buttons live in the footer).
const PAINT_ORDER: [Layer; 5] = [
    Layer::Canvas,
    Layer::ToolbarBack,
    Layer::FooterBack,
    Layer::Buttons,
    Layer::FooterText,
];

fn draw(e: &mut Editor) -> windows::core::Result<()> {
    let (cw, ch) = e.client;
    if e.target.is_none() {
        e.target = Some(e.gfx.hwnd_target(e.hwnd, (cw, ch))?);
        e.picture = None;
        e.pix.clear();
    }
    let ui = e.dpi as f32 / 96.0;
    let Editor {
        target,
        picture,
        no_picture,
        pix,
        gfx,
        session,
        base,
        layout,
        view,
        hover,
        text,
        footer,
        confirm,
        saving,
        demo,
        p,
        ..
    } = e;
    let t = target.as_ref().expect("made above");
    let rt: &ID2D1RenderTarget = &t.rt;
    let brush = &t.brush;
    if picture.is_none() && !*no_picture {
        match bitmap(rt, base.w, base.h, &base.bgra) {
            Ok(b) => *picture = Some(b),
            Err(err) if err.code() == D2DERR_RECREATE_TARGET => return Err(err),
            Err(err) => {
                // Too big for the device: the rest still shows.
                eprintln!("vale: the screenshot cannot be shown: {err}");
                *no_picture = true;
                *footer = Some((tr(Key::EditPreviewFailed).to_owned(), Tone::Error));
            }
        }
    }
    unsafe {
        rt.BeginDraw();
        rt.Clear(Some(&color(p.bg, 1.0)));
    }
    let (border, ba) = rgba(p.border);
    let (sel_rgb, sel_a) = rgba(p.selection);
    // Direct2D paints in call order: PAINT_ORDER is the order, and a region's
    // background always comes before the buttons that live in it.
    for layer in PAINT_ORDER {
        match layer {
            Layer::Canvas => {
                fill(rt, brush, layout.canvas, p.card, 1.0);
                let img = Rect {
                    x: view.ox,
                    y: view.oy,
                    w: base.w as f32 * view.scale,
                    h: base.h as f32 * view.scale,
                };
                unsafe {
                    if let Some(picture) = picture.as_ref() {
                        rt.DrawBitmap(
                            picture,
                            Some(&rect_f(img.x, img.y, img.w, img.h)),
                            1.0,
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            None,
                        );
                    }
                    // Annotations stay inside the picture.
                    rt.PushAxisAlignedClip(
                        &rect_f(img.x, img.y, img.w, img.h),
                        D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    );
                }
                let editing = text.as_ref().and_then(|t| t.index);
                draw_annotations(rt, brush, gfx, base, &session.doc, view, editing, pix);
                unsafe {
                    rt.PopAxisAlignedClip();
                }
                unsafe {
                    brush.SetColor(&color(border, ba.max(0.12)));
                    rt.DrawRectangle(
                        &rect_f(img.x - 0.5, img.y - 0.5, img.w + 1.0, img.h + 1.0),
                        brush,
                        1.0,
                        None,
                    );
                }
                if let Some(a) = session.selected.and_then(|i| session.doc.get(i))
                    && editing.is_none()
                {
                    draw_selection(rt, brush, a, view, p.accent, ui);
                }
            }
            Layer::ToolbarBack => {
                fill(rt, brush, layout.toolbar, p.bg, 1.0);
                fill(
                    rt,
                    brush,
                    Rect {
                        x: 0.0,
                        y: layout.toolbar.h - 1.0,
                        w: layout.toolbar.w,
                        h: 1.0,
                    },
                    border,
                    ba.max(0.12),
                );
                for x in &layout.seps {
                    fill(
                        rt,
                        brush,
                        Rect {
                            x: *x,
                            y: layout.toolbar.h * 0.28,
                            w: 1.0,
                            h: layout.toolbar.h * 0.44,
                        },
                        border,
                        ba.max(0.18),
                    );
                }
            }
            Layer::FooterBack => {
                fill(rt, brush, layout.footer, p.bg, 1.0);
                fill(
                    rt,
                    brush,
                    Rect {
                        x: 0.0,
                        y: layout.footer.y,
                        w: layout.footer.w,
                        h: 1.0,
                    },
                    border,
                    ba.max(0.12),
                );
            }
            Layer::Buttons => {
                for (hit, r) in &layout.items {
                    // While the bar asks, only its two buttons are live.
                    let live = !*confirm || matches!(hit, Hit::Keep | Hit::Discard);
                    let hot = live && *hover == Some(*hit);
                    let a = if live { 1.0 } else { 0.35 };
                    match *hit {
                        Hit::Tool(tool) => {
                            let on = session.tool == tool;
                            if on {
                                fill_round(rt, brush, *r, 9.0 * ui, sel_rgb, sel_a);
                            } else if hot {
                                fill_round(rt, brush, *r, 9.0 * ui, p.hover, 1.0);
                            }
                            glyph(
                                rt,
                                brush,
                                gfx,
                                Glyph::Tool(tool),
                                *r,
                                if on { p.accent } else { p.text },
                                a,
                                ui,
                            );
                        }
                        Hit::Color(i) => {
                            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                            let on = session.color == COLORS[i];
                            let radius = 9.0 * ui;
                            if hot && !on {
                                fill_round(rt, brush, *r, 9.0 * ui, p.hover, 1.0);
                            }
                            unsafe {
                                use windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE;
                                let ell = |rad: f32| D2D1_ELLIPSE {
                                    point: Vector2::new(cx, cy),
                                    radiusX: rad,
                                    radiusY: rad,
                                };
                                brush.SetColor(&color(COLORS[i], a));
                                rt.FillEllipse(&ell(radius), brush);
                                brush.SetColor(&color(p.text, 0.22 * a));
                                rt.DrawEllipse(&ell(radius), brush, 1.0, None);
                                if on {
                                    brush.SetColor(&color(p.accent, a));
                                    rt.DrawEllipse(&ell(radius + 3.5 * ui), brush, 2.0 * ui, None);
                                }
                            }
                        }
                        Hit::Stroke(i) => {
                            let on = session.stroke == i;
                            if on {
                                fill_round(rt, brush, *r, 9.0 * ui, sel_rgb, sel_a);
                            } else if hot {
                                fill_round(rt, brush, *r, 9.0 * ui, p.hover, 1.0);
                            }
                            let wpx = [2.0, 4.0, 7.0][i] * ui;
                            unsafe {
                                brush.SetColor(&color(if on { p.accent } else { p.text }, a))
                            };
                            line(
                                rt,
                                brush,
                                gfx,
                                (r.x + r.w * 0.28, r.y + r.h / 2.0),
                                (r.x + r.w * 0.72, r.y + r.h / 2.0),
                                wpx,
                            );
                        }
                        Hit::Undo | Hit::Redo => {
                            let can = if matches!(hit, Hit::Undo) {
                                session.can_undo()
                            } else {
                                session.can_redo()
                            };
                            if hot && can {
                                fill_round(rt, brush, *r, 9.0 * ui, p.hover, 1.0);
                            }
                            let g = if matches!(hit, Hit::Undo) {
                                Glyph::Undo
                            } else {
                                Glyph::Redo
                            };
                            glyph(
                                rt,
                                brush,
                                gfx,
                                g,
                                *r,
                                p.text,
                                if can { a } else { 0.3 * a },
                                ui,
                            );
                        }
                        Hit::Copy | Hit::Save | Hit::Close | Hit::Discard | Hit::Keep => {
                            let (label, primary, danger) = match hit {
                                Hit::Copy => (tr(Key::EditCopy), false, false),
                                Hit::Save => (tr(Key::EditSave), true, false),
                                Hit::Close => (tr(Key::EditClose), false, false),
                                Hit::Discard => (tr(Key::EditDiscard), false, true),
                                _ => (tr(Key::EditKeep), false, false),
                            };
                            let busy = matches!(hit, Hit::Save) && *saving;
                            let (bg, fg) = if primary {
                                (p.accent, p.on_accent)
                            } else if danger {
                                (p.error, if p.light { 0xFFFFFF } else { 0x1A1210 })
                            } else if hot {
                                (p.button_hover, p.text)
                            } else {
                                (p.button, p.text)
                            };
                            let mut alpha = a;
                            // Copy is inert in the demos.
                            if (matches!(hit, Hit::Copy) && *demo) || busy {
                                alpha *= 0.5;
                            }
                            fill_round(rt, brush, *r, 10.0 * ui, bg, alpha);
                            gfx.text_in(
                                rt,
                                brush,
                                label,
                                (r.x, r.y, r.w, r.h),
                                UI_TEXT * ui,
                                500,
                                fg,
                                alpha,
                                Align::Center,
                            );
                        }
                    }
                }
            }
            Layer::FooterText => {
                let pad = MARGIN * ui;
                let mut text_box = (
                    pad,
                    layout.footer.y,
                    (layout.footer.w - 2.0 * pad).max(0.0),
                    layout.footer.h,
                );
                if *confirm {
                    // The question ends before the Discard button.
                    if let Some(d) = layout.rect_of(Hit::Discard) {
                        text_box.2 = (d.x - 12.0 * ui - text_box.0).max(0.0);
                    }
                    gfx.text_in(
                        rt,
                        brush,
                        tr(Key::EditDiscardAsk),
                        text_box,
                        UI_TEXT * ui,
                        600,
                        p.text,
                        1.0,
                        Align::Left,
                    );
                } else {
                    // The button under the pointer names itself; else the last result;
                    // else the hint.
                    let (msg, tone) = (*hover)
                        .and_then(tip)
                        .map(|t| (t, Tone::Hint))
                        .or_else(|| footer.clone())
                        .unwrap_or_else(|| (tr(Key::EditHint).to_owned(), Tone::Hint));
                    let rgb = match tone {
                        Tone::Hint => p.secondary,
                        Tone::Ok => p.text,
                        Tone::Error => p.error,
                    };
                    let size_info = format!(
                        "{} × {} · {}%",
                        base.w,
                        base.h,
                        (view.scale * 100.0).round() as i32
                    );
                    let info_w = gfx.measure(&size_info, 12.0 * ui, 400).0 + 4.0;
                    gfx.text_in(
                        rt,
                        brush,
                        &msg,
                        (
                            text_box.0,
                            text_box.1,
                            (text_box.2 - info_w - 12.0 * ui).max(0.0),
                            text_box.3,
                        ),
                        12.5 * ui,
                        400,
                        rgb,
                        1.0,
                        Align::Left,
                    );
                    gfx.text_in(
                        rt,
                        brush,
                        &size_info,
                        (
                            text_box.0 + text_box.2 - info_w,
                            text_box.1,
                            info_w,
                            text_box.3,
                        ),
                        12.0 * ui,
                        400,
                        p.tertiary,
                        1.0,
                        Align::Right,
                    );
                }
            }
        }
    }
    unsafe { rt.EndDraw(None, None) }
}

// ---- demo ----

/// `--demo-shot-editor [empty]`: the editor on a made-up picture, with a few
/// annotations (none with `empty`). Copy is inert, Save writes under
/// `target/demo-shots/`; the process ends when the window closes.
/// `DI_WINDOW=system|dark|light`, `DI_SURFACE` (`terminal` gives the neutral
/// dark window) and `DI_ACCENT` pick the theme; unset is the default one.
#[cfg(debug_assertions)]
pub fn run_demo(empty: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, TranslateMessage,
    };

    theme::set_system_light(theme::read_system_light());
    theme::set_current(theme::theme_from_env());
    let shot = crate::capture::demo::picture(960, 600);
    let doc = if empty { Vec::new() } else { demo_doc() };
    let anchor = {
        let area = crate::capture::win::virtual_screen();
        (area.x + area.w / 2, area.y + area.h / 2)
    };
    open_with(shot, anchor, true, doc);
    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// A few of each kind, over `capture::demo::picture`.
#[cfg(debug_assertions)]
fn demo_doc() -> Vec<Annotation> {
    use super::Style;
    let style = |c: u32, w: f32| Style { color: c, width: w };
    vec![
        Annotation::Rect {
            r: FRect::new(300.0, 120.0, 260.0, 150.0),
            style: style(COLORS[0], STROKES[1]),
        },
        Annotation::Arrow {
            a: Pt::new(150.0, 480.0),
            b: Pt::new(330.0, 300.0),
            style: style(COLORS[3], STROKES[1]),
        },
        Annotation::Pen {
            pts: (0..40)
                .map(|i| {
                    let t = i as f32 / 39.0;
                    Pt::new(600.0 + t * 260.0, 430.0 + (t * 12.0).sin() * 22.0)
                })
                .collect(),
            style: style(COLORS[2], STROKES[1]),
        },
        Annotation::Marker {
            pts: vec![
                Pt::new(320.0, 330.0),
                Pt::new(470.0, 330.0),
                Pt::new(560.0, 336.0),
            ],
            style: style(COLORS[1], STROKES[1]),
        },
        Annotation::Text {
            pos: Pt::new(330.0, 70.0),
            text: "Periksa bagian ini".into(),
            size: text_size(1),
            color: COLORS[0],
        },
        Annotation::Pixelate {
            r: FRect::new(90.0, 110.0, 170.0, 60.0),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABELS: Labels = [44.0, 52.0, 40.0, 40.0, 38.0];

    #[test]
    fn the_footer_background_is_painted_before_the_buttons() {
        let at = |l: Layer| PAINT_ORDER.iter().position(|x| *x == l).unwrap();
        assert!(at(Layer::FooterBack) < at(Layer::Buttons));
        assert!(at(Layer::ToolbarBack) < at(Layer::Buttons));
        assert!(at(Layer::FooterText) > at(Layer::FooterBack));
    }

    #[test]
    fn closing_asks_only_when_something_changed() {
        assert_eq!(close_action(false), CloseAction::Destroy);
        assert_eq!(close_action(true), CloseAction::Ask);
    }

    #[test]
    fn the_bar_answers_enter_and_esc_only() {
        assert_eq!(confirm_key(VK_RETURN.0), Some(Hit::Discard));
        assert_eq!(confirm_key(VK_ESCAPE.0), Some(Hit::Keep));
        assert_eq!(confirm_key(0x5A), None);
        assert_eq!(confirm_key(VK_DELETE.0), None);
    }

    fn all_overlaps(items: &[(Hit, Rect)]) -> Vec<(Hit, Hit)> {
        let mut bad = Vec::new();
        for (i, (ha, a)) in items.iter().enumerate() {
            for (hb, b) in &items[i + 1..] {
                let apart = a.right() <= b.x + 0.01
                    || b.right() <= a.x + 0.01
                    || a.bottom() <= b.y + 0.01
                    || b.bottom() <= a.y + 0.01;
                if !apart {
                    bad.push((*ha, *hb));
                }
            }
        }
        bad
    }

    #[test]
    fn the_toolbar_holds_everything_in_order_without_overlap() {
        for ui in [1.0, 1.25, 1.5, 2.0] {
            let w = min_client_w(ui, &LABELS);
            let l = layout(w, 500.0 * ui, ui, &LABELS, false);
            for (hit, r) in &l.items {
                assert!(
                    r.x >= 0.0 && r.right() <= w + 0.01,
                    "{hit:?} at {ui}: {r:?}"
                );
                assert!(r.y >= 0.0 && r.bottom() <= l.toolbar.h + 0.01, "{hit:?}");
            }
            assert!(
                all_overlaps(&l.items).is_empty(),
                "{:?}",
                all_overlaps(&l.items)
            );
            let order: Vec<Hit> = l.items.iter().map(|(h, _)| *h).collect();
            let tools: Vec<Hit> = Tool::ALL.iter().map(|t| Hit::Tool(*t)).collect();
            assert_eq!(
                &order[..8],
                tools.as_slice(),
                "Select first, then T R L A P M B"
            );
            assert_eq!(l.seps.len(), 3);
            // The two groups apart at the least width.
            let copy = l.rect_of(Hit::Copy).expect("copy");
            assert!(copy.x >= l.left_end, "{ui}");
        }
    }

    #[test]
    fn the_editor_places_text_by_the_shared_rules() {
        // The rules and their tests moved to `text`; the editor uses them.
        assert_eq!(text_click_action(true, Tool::Text), TextClick::CommitOnly);
        assert_eq!(text_click_action(false, Tool::Text), TextClick::Place);
        assert!(text_msg_is_current(Some(7), 7) && !text_msg_is_current(Some(7), 8));
    }

    #[test]
    fn the_regions_stack_toolbar_canvas_footer() {
        let l = layout(900.0, 700.0, 1.0, &LABELS, false);
        assert_eq!(l.toolbar.y, 0.0);
        assert_eq!(l.canvas.y, l.toolbar.bottom());
        assert_eq!(l.footer.y, l.canvas.bottom());
        assert!((l.footer.bottom() - 700.0).abs() < 0.01);
        // A window shorter than the chrome never gives a negative canvas.
        let tiny = layout(900.0, 50.0, 1.0, &LABELS, false);
        assert!(tiny.canvas.h >= 0.0);
    }

    #[test]
    fn buttons_are_found_by_position() {
        let l = layout(900.0, 700.0, 1.0, &LABELS, false);
        let undo = l.rect_of(Hit::Undo).expect("undo");
        assert_eq!(l.hit(undo.x + 2.0, undo.y + 2.0), Some(Hit::Undo));
        let save = l.rect_of(Hit::Save).expect("save");
        assert_eq!(
            l.hit(save.x + save.w / 2.0, save.y + save.h / 2.0),
            Some(Hit::Save)
        );
        assert_eq!(l.hit(450.0, 400.0), None, "the canvas is no button");
        assert!(l.rect_of(Hit::Discard).is_none());
    }

    #[test]
    fn the_discard_bar_puts_two_buttons_in_the_footer() {
        let l = layout(1000.0, 700.0, 1.0, &LABELS, true);
        let keep = l.rect_of(Hit::Keep).expect("batal");
        let discard = l.rect_of(Hit::Discard).expect("buang");
        for r in [keep, discard] {
            assert!(r.y >= l.footer.y && r.bottom() <= l.footer.bottom());
            assert!(r.right() <= 1000.0);
        }
        assert!(discard.right() <= keep.x, "Buang left of Batal");
        assert!(
            all_overlaps(&l.items).is_empty(),
            "{:?}",
            all_overlaps(&l.items)
        );
    }

    #[test]
    fn a_picture_that_fits_opens_at_100_percent_and_a_big_one_shrinks() {
        let work = (1920.0, 1040.0);
        assert_eq!(open_scale((800, 500), work, 1.0), 1.0);
        let s = open_scale((3840, 2160), work, 1.0);
        assert!(s < 0.5, "{s}");
        // The window it asks for fits the work area.
        let (cw, ch) = client_size((3840, 2160), s, 1.0, 600.0);
        assert!(cw <= work.0 && ch <= work.1, "{cw} x {ch}");
        // 100 % when the image alone is within 85 % of the area.
        assert_eq!(open_scale((1600, 800), work, 1.0), 1.0);
        // A short work area still leaves room for the chrome.
        let s = open_scale((1400, 900), (1366.0, 728.0), 1.0);
        let (_, ch) = client_size((1400, 900), s, 1.0, 600.0);
        assert!(ch <= 728.0 + 1.0, "{ch}");
    }

    #[test]
    fn a_small_picture_still_gets_a_window_wide_enough_for_the_toolbar() {
        let min = min_client_w(1.0, &LABELS);
        let (cw, _) = client_size((200, 100), 1.0, 1.0, min);
        assert_eq!(cw, min.ceil());
        let (cw, ch) = client_size((2000, 1000), 1.0, 1.0, min);
        assert_eq!(cw, 2048.0);
        assert_eq!(ch, 1000.0 + 48.0 + 96.0);
    }

    #[test]
    fn a_button_under_the_pointer_names_itself_with_its_shortcut() {
        crate::i18n::set_lang(crate::i18n::Lang::En);
        assert_eq!(tip(Hit::Tool(Tool::Text)).as_deref(), Some("Text (T)"));
        assert_eq!(
            tip(Hit::Tool(Tool::Pixelate)).as_deref(),
            Some("Pixelate (B)")
        );
        assert_eq!(tip(Hit::Undo).as_deref(), Some("Undo (Ctrl+Z)"));
        assert_eq!(tip(Hit::Redo).as_deref(), Some("Redo (Ctrl+Y)"));
        assert_eq!(tip(Hit::Save).as_deref(), Some("Save (Ctrl+S)"));
        assert_eq!(tip(Hit::Copy).as_deref(), Some("Copy (Ctrl+C)"));
        assert!(tip(Hit::Keep).is_none() && tip(Hit::Discard).is_none());
        for t in Tool::ALL {
            let s = tip(Hit::Tool(t)).expect("a tip");
            assert!(s.ends_with(&format!("({})", t.key())), "{s}");
        }
    }

    #[test]
    fn rgba_splits_a_packed_colour() {
        let (rgb, a) = rgba(0xF4EDE414);
        assert_eq!(rgb, 0xF4EDE4);
        assert!((a - 20.0 / 255.0).abs() < 1e-6);
    }
}
