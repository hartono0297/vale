//! The caption card's window: a layered popup drawn into a premultiplied DIB
//! by a software Direct2D target and pushed with `UpdateLayeredWindow`, the
//! way the lyrics card does it (`lyrics_card::window`), with DirectWrite text
//! and the pointer and wheel handling.
//!
//! The card never takes the focus: `WS_EX_NOACTIVATE`, shown with
//! `SW_SHOWNOACTIVATE`, and `WM_MOUSEACTIVATE` answers `MA_NOACTIVATE`, so a
//! click or a drag leaves the meeting or the video active. It has no keyboard
//! input of its own for the same reason.
//!
//! State lives in this module's `thread_local!` (`CARD`). The window procedure
//! never calls `app::with_app`: it tells the island what the user did with
//! `PostMessageW(island, WM_APP_CAPTION_CARD, code, value)`. The borrow of
//! `CARD` is never held across `ShowWindow`, `SetCapture`, `ReleaseCapture` or
//! `DestroyWindow` (they send messages to this window synchronously): such
//! calls are collected as `Fx` and run after the borrow ends, and the
//! procedure uses `try_borrow_mut` everywhere.
//!
//! Power: two one-shot timers on the card's own hwnd (the header clock, only
//! while a session runs and the card is shown; the manual-scroll return, only
//! while held) and a `FrameClock` only while something eases. Nothing repeats.
//!
//! Caption text is drawn and nothing else: it is never logged or printed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::time::Instant;

use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Brush, ID2D1DCRenderTarget,
    ID2D1Factory, ID2D1Layer, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT, DWRITE_LINE_METRICS,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics, GetWindowRect,
    HTCLIENT, IDC_ARROW, IDC_HAND, IDC_SIZENWSE, KillTimer, LoadCursorW, MA_NOACTIVATE,
    PostMessageW, RegisterClassExW, SM_CXDRAG, SM_CYDRAG, SW_SHOWNOACTIVATE, SetCursor, SetTimer,
    ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WM_CAPTURECHANGED, WM_CLOSE, WM_DESTROY,
    WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_SETCURSOR, WM_TIMER, WM_WINDOWPOSCHANGED, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, Interface, PCWSTR, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use super::layout::{self, Palette, RectF, Rects, Zone};
use super::{CardContent, Change, Msg, OpenArgs, WM_APP_CAPTION_CARD};
use crate::captions::control::CardStatus;
use crate::captions::lines::Line;
use crate::captions::prefs::TextSize;
use crate::config::{CardPlace, UiFont};
use crate::i18n::{Key, tr};
use crate::lyrics_card::layout::{ManualScroll, ease_out_cubic, is_drag, point_in, px};
use crate::shot_edit::gfx::{color, rect_f};
use crate::transcribe::output::Speaker;
use crate::transcribe::prefs::TrLang;
use crate::ui::fonts::{Face, Role};
use crate::ui::frame_clock::{FrameClock, WM_APP_FRAME};
use crate::ui::geometry::Rect as GRect;
use crate::ui::icons::{Icon, IconSet};

/// The window class.
const CLASS: PCWSTR = w!("ValeCaptionCard");
/// The class name, for the test that pins it.
#[cfg(test)]
pub const CLASS_NAME: &str = "ValeCaptionCard";
/// The card's own timer ids (the card has its own id space).
const TIMER_CLOCK: usize = 1;
const TIMER_MANUAL: usize = 2;
/// `MK_CONTROL`: Ctrl is held, in a mouse message's `wparam`.
const MK_CONTROL: usize = 0x0008;
/// The header's text, and the smallest a hint or a label gets.
const HEADER_PX: f32 = 12.0;
const CHIP_PX: f32 = 11.0;
const MESSAGE_PX: f32 = 14.0;
const SMALL_MIN_PX: f32 = 12.0;
/// A speaker label and a hint, as a part of the caption text's height.
const SMALL_SCALE: f32 = 0.72;
/// How long the list takes back to the newest line, a wheel step, the hover.
const RETURN_MS: u32 = 350;
const WHEEL_MS: u32 = 120;
const HOVER_MS: u32 = 150;

// `ManualScroll` (the lyrics card's) steps and holds by its own numbers: the
// card's layout says the same ones.
const _: () = assert!(
    layout::WHEEL_STEP == crate::lyrics_card::layout::WHEEL_STEP
        && layout::MANUAL_HOLD_MS == crate::lyrics_card::layout::MANUAL_HOLD_MS
);

// ------------------------------------------------------------------ surface

/// The factories, the software target bound to a DIB the size of the window,
/// and what is cached per target (the lyrics card's surface, without art).
struct Surface {
    // Declared first: released before the factories.
    icons: IconSet,
    layer: ID2D1Layer,
    brush: ID2D1SolidColorBrush,
    rt: ID2D1DCRenderTarget,
    face: Face,
    formats: HashMap<(u32, i32), IDWriteTextFormat>,
    dwrite: IDWriteFactory,
    /// Kept alive for the target and the icon geometries made from it.
    _d2d: ID2D1Factory,
    dc: HDC,
    dib: HBITMAP,
    previous: HGDIOBJ,
    size: (i32, i32),
}

fn make_dib(dc: HDC, size: (i32, i32)) -> Result<HBITMAP> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size.0,
            biHeight: -size.1,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    unsafe { CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) }
}

impl Surface {
    fn new(size: (i32, i32), font: UiFont) -> Result<Surface> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let face = Face::for_role_in(&dwrite, Role::Text, font);
            let dc = CreateCompatibleDC(None);
            let dib = match make_dib(dc, size) {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            let previous = SelectObject(dc, dib.into());
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let made = (|| {
                let rt = d2d.CreateDCRenderTarget(&props)?;
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                let brush = rt.CreateSolidColorBrush(&color(0, 1.0), None)?;
                let layer = rt.CreateLayer(None)?;
                let icons = IconSet::new(&d2d)?;
                Ok::<_, Error>((rt, brush, layer, icons))
            })();
            match made {
                Ok((rt, brush, layer, icons)) => Ok(Surface {
                    icons,
                    layer,
                    brush,
                    rt,
                    face,
                    formats: HashMap::new(),
                    dwrite,
                    _d2d: d2d,
                    dc,
                    dib,
                    previous,
                    size,
                }),
                Err(e) => {
                    SelectObject(dc, previous);
                    let _ = DeleteObject(dib.into());
                    let _ = DeleteDC(dc);
                    Err(e)
                }
            }
        }
    }

    /// A DIB of the new size (the target and the brushes stay).
    fn resize(&mut self, size: (i32, i32)) -> Result<()> {
        if size == self.size {
            return Ok(());
        }
        unsafe {
            let fresh = make_dib(self.dc, size)?;
            SelectObject(self.dc, fresh.into());
            let _ = DeleteObject(self.dib.into());
            self.dib = fresh;
        }
        self.size = size;
        Ok(())
    }

    fn set_font(&mut self, font: UiFont) {
        self.face = Face::for_role_in(&self.dwrite, Role::Text, font);
        self.formats.clear();
    }

    fn format(&mut self, size: f32, weight: i32) -> Option<IDWriteTextFormat> {
        let key = (size.to_bits(), weight);
        if let Some(f) = self.formats.get(&key) {
            return Some(f.clone());
        }
        let f = self
            .face
            .format(&self.dwrite, DWRITE_FONT_WEIGHT(weight), size)
            .ok()?;
        self.formats.insert(key, f.clone());
        Some(f)
    }

    /// `text` laid out `width` wide (wrapping), at `size` dip.
    fn layout(
        &mut self,
        text: &str,
        size: f32,
        weight: i32,
        width: f32,
    ) -> Option<IDWriteTextLayout> {
        let format = self.format(size, weight)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.dwrite
                .CreateTextLayout(&wide, &format, width.max(1.0), 8192.0)
                .ok()
        }
    }

    /// One line cut with an ellipsis at `width`.
    fn one_line(
        &mut self,
        text: &str,
        size: f32,
        weight: i32,
        width: f32,
    ) -> Option<IDWriteTextLayout> {
        let format = self.format(size, weight)?;
        let layout = self.layout(text, size, weight, width)?;
        unsafe {
            layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).ok()?;
            let ellipsis = self.dwrite.CreateEllipsisTrimmingSign(&format).ok()?;
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            layout.SetTrimming(&trimming, &ellipsis).ok()?;
        }
        Some(layout)
    }

    fn metrics(layout: &IDWriteTextLayout) -> DWRITE_TEXT_METRICS {
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = unsafe { layout.GetMetrics(&mut m) };
        m
    }

    fn height_of(layout: &IDWriteTextLayout) -> f32 {
        Surface::metrics(layout).height
    }

    /// How far the first line's baseline is below the layout's top.
    fn first_baseline(layout: &IDWriteTextLayout) -> f32 {
        let count = Surface::metrics(layout).lineCount.max(1) as usize;
        let mut lines = vec![DWRITE_LINE_METRICS::default(); count];
        let mut got = 0u32;
        match unsafe { layout.GetLineMetrics(Some(&mut lines), &mut got) } {
            Ok(()) if got > 0 => lines[0].baseline,
            _ => 0.0,
        }
    }

    fn draw_text(&self, origin: (f32, f32), layout: &IDWriteTextLayout, rgb: u32, alpha: f32) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.rt.DrawTextLayout(
                Vector2::new(origin.0, origin.1),
                layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            );
        }
    }

    /// `layout` centred in `area`, both ways.
    fn draw_centred(&self, area: RectF, layout: &IDWriteTextLayout, rgb: u32, alpha: f32) {
        let m = Surface::metrics(layout);
        let x = area.x + (area.w - m.widthIncludingTrailingWhitespace) / 2.0;
        let y = area.y + (area.h - m.height) / 2.0;
        self.draw_text((x - m.left, y), layout, rgb, alpha);
    }

    fn fill_round(&self, r: RectF, radius: f32, rgb: u32, alpha: f32) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.rt
                .FillRoundedRectangle(&rounded(r, radius), &self.brush);
        }
    }

    fn stroke_round(&self, r: RectF, radius: f32, rgb: u32, alpha: f32, width: f32) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.rt
                .DrawRoundedRectangle(&rounded(r, radius), &self.brush, width, None);
        }
    }

    fn icon(&self, icon: Icon, area: RectF, size: f32, rgb: u32, alpha: f32) {
        let _ = self.icons.draw(
            &self.rt,
            &self.brush,
            icon,
            GRect {
                x: area.x,
                y: area.y,
                w: area.w,
                h: area.h,
            },
            size,
            rgb,
            alpha,
            None,
        );
    }

    /// A vertical opacity gradient over `[y0, y1]`.
    fn vertical_gradient(&self, y0: f32, y1: f32, stops: &[(f32, f32)]) -> Option<ID2D1Brush> {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(position, alpha)| D2D1_GRADIENT_STOP {
                position,
                color: color(0, alpha),
            })
            .collect();
        unsafe {
            let collection = self
                .rt
                .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                .ok()?;
            let brush = self
                .rt
                .CreateLinearGradientBrush(
                    &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: Vector2::new(0.0, y0),
                        endPoint: Vector2::new(0.0, y1),
                    },
                    None,
                    &collection,
                )
                .ok()?;
            brush.cast().ok()
        }
    }

    /// Pushes a layer over `bounds` that fades by `opacity`'s alpha. Give the
    /// returned brush back to `pop_layer` (it keeps the reference the layer
    /// was given until the layer is popped).
    fn push_layer(&self, bounds: RectF, opacity: Option<ID2D1Brush>) -> Option<ID2D1Brush> {
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: D2D_RECT_F {
                left: bounds.x,
                top: bounds.y,
                right: bounds.right(),
                bottom: bounds.bottom(),
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(opacity.clone()),
            layerOptions: D2D1_LAYER_OPTIONS_NONE,
        };
        unsafe { self.rt.PushLayer(&params, &self.layer) };
        drop(ManuallyDrop::into_inner(params.opacityBrush));
        opacity
    }

    fn pop_layer(&self, keep: Option<ID2D1Brush>) {
        unsafe { self.rt.PopLayer() };
        drop(keep);
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.dib.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

fn rounded(r: RectF, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: rect_f(r.x, r.y, r.w, r.h),
        radiusX: radius,
        radiusY: radius,
    }
}

// ------------------------------------------------------------------- state

/// An eased value.
#[derive(Clone, Copy, Debug)]
struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    ms: u32,
}

impl Tween {
    fn at_rest(v: f32, now: Instant) -> Tween {
        Tween {
            from: v,
            to: v,
            start: now,
            ms: 0,
        }
    }

    fn t(&self, now: Instant) -> f32 {
        if self.ms == 0 {
            return 1.0;
        }
        (now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 / self.ms as f32)
            .clamp(0.0, 1.0)
    }

    fn at(&self, now: Instant) -> f32 {
        self.from + (self.to - self.from) * ease_out_cubic(self.t(now))
    }

    fn moving(&self, now: Instant) -> bool {
        self.ms > 0 && self.t(now) < 1.0
    }

    /// Heads for `to` from where it is now; at once when `animate` is false.
    fn go(&mut self, to: f32, ms: u32, animate: bool, now: Instant) {
        let here = self.at(now);
        *self = if animate && ms > 0 {
            Tween {
                from: here,
                to,
                start: now,
                ms,
            }
        } else {
            Tween::at_rest(to, now)
        };
    }
}

/// What the island last said the card shows.
struct Content {
    status: CardStatus,
    lines: Arc<Vec<Line>>,
    interim: Vec<(Option<Speaker>, String)>,
    labelled: bool,
    running_since: Option<Instant>,
    lang: TrLang,
    size: TextSize,
}

/// One wrapped row of the body.
struct Row {
    /// Whose label stands left of it (only while labelled).
    who: Option<Speaker>,
    text: IDWriteTextLayout,
    rgb: u32,
    h: f32,
    /// The first line's baseline, for the label to sit on.
    baseline: f32,
}

/// The two speaker labels at the current text size.
struct Labels {
    me: IDWriteTextLayout,
    others: IDWriteTextLayout,
    /// The label column: the wider of the two.
    w: f32,
    baseline: f32,
}

/// What the body shows.
enum View {
    /// The caption lines, stacked on the body's bottom.
    Rows,
    /// One centred sentence (listening, paused, stopped).
    Message(String),
    /// A sentence and the status button under it.
    Prompt(&'static str, &'static str),
    /// The model download, in percent.
    Progress(u8),
}

struct Press {
    zone: Option<Zone>,
    /// Where the pointer was (screen px) and where the window was.
    start: (i32, i32),
    origin: (i32, i32),
    size_dip: (f32, f32),
    dragging: bool,
}

#[derive(Clone, Copy)]
enum Pointer {
    Arrow,
    Hand,
    Grip,
}

/// What to do once the borrow is released.
enum Fx {
    Capture(bool),
    Cursor(Pointer),
}

pub(super) struct Card {
    hwnd: HWND,
    island: HWND,
    surf: Surface,
    dpi: u32,
    origin: (i32, i32),
    size_dip: (f32, f32),
    c: Content,
    pal: Palette,
    accent: u32,
    font: UiFont,
    animations: bool,
    /// The engine fell behind once: the hint stays until the card closes.
    slow: bool,
    labels: Option<Labels>,
    /// The final lines, then the grey lines, the status line and the hint.
    finals: Vec<Row>,
    tail: Vec<Row>,
    heights: Vec<f32>,
    started: Instant,
    // view
    /// How far the list is scrolled back from the newest line.
    back: Tween,
    manual: ManualScroll,
    hover: Tween,
    hovering: bool,
    hover_zone: Option<Zone>,
    tracking: bool,
    press: Option<Press>,
    /// On screen (or would be: tests never show it). False until the card is
    /// stored, so nothing is timed for a card nobody can find yet.
    shown: bool,
    /// The window was destroyed by someone other than `close` (App was asked
    /// to close): nothing is drawn or timed on the dead hwnd any more.
    gone: bool,
    clock: Option<FrameClock>,
    placed: Option<CardPlace>,
    demo: bool,
}

thread_local! {
    static CARD: RefCell<Option<Card>> = const { RefCell::new(None) };
}

fn register() -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(proc),
        hInstance: module.into(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    unsafe {
        if RegisterClassExW(&wc) == 0 {
            return Error::from_thread().code() == ERROR_CLASS_ALREADY_EXISTS.to_hresult();
        }
    }
    true
}

fn create_window(origin: (i32, i32), size: (i32, i32)) -> Option<HWND> {
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            CLASS,
            w!("Vale captions"),
            WS_POPUP,
            origin.0,
            origin.1,
            size.0,
            size.1,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .ok()
}

/// Tests make the window but never put it on screen (`test-spawns-no-window`).
const ON_SCREEN: bool = !cfg!(test);

fn run_fx(fx: Vec<Fx>, hwnd: HWND) {
    for f in fx {
        match f {
            Fx::Capture(true) => unsafe {
                let _ = SetCapture(hwnd);
            },
            Fx::Capture(false) => unsafe {
                let _ = ReleaseCapture();
            },
            Fx::Cursor(which) => unsafe {
                let id = match which {
                    Pointer::Grip => IDC_SIZENWSE,
                    Pointer::Hand => IDC_HAND,
                    Pointer::Arrow => IDC_ARROW,
                };
                if let Ok(c) = LoadCursorW(None, id) {
                    SetCursor(Some(c));
                }
            },
        }
    }
}

/// Runs `f` on the card (if there is one and it is not borrowed), then the
/// window calls it asked for.
fn with<R>(f: impl FnOnce(&mut Card, &mut Vec<Fx>) -> R) -> Option<R> {
    let mut fx = Vec::new();
    let (out, hwnd) = CARD.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        let card = guard.as_mut()?;
        let hwnd = card.hwnd;
        Some((f(card, &mut fx), hwnd))
    })?;
    run_fx(fx, hwnd);
    Some(out)
}

// -------------------------------------------------------------------- API

pub(super) fn open(args: OpenArgs, content: CardContent) -> bool {
    if CARD.with(|c| c.try_borrow().map_or(true, |c| c.is_some())) {
        return false;
    }
    if !register() {
        return false;
    }
    let Some(rc) = layout::place_default(args.saved, &args.monitors, args.notch) else {
        return false;
    };
    let dpi = args
        .monitors
        .iter()
        .chain(args.notch.iter())
        .max_by_key(|m| {
            let w = (rc.right.min(m.rect.right) - rc.left.max(m.rect.left)).max(0);
            let h = (rc.bottom.min(m.rect.bottom) - rc.top.max(m.rect.top)).max(0);
            i64::from(w) * i64::from(h)
        })
        .map_or(96, |m| m.dpi.max(96));
    let k = dpi as f32 / 96.0;
    let size_dip = layout::clamp_size(rc.width() as f32 / k, rc.height() as f32 / k);
    let size = (px(size_dip.0, dpi), px(size_dip.1, dpi));
    let origin = (rc.left, rc.top);
    let Some(hwnd) = create_window(origin, size) else {
        return false;
    };
    let Ok(surf) = Surface::new(size, content.font) else {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return false;
    };
    let now = Instant::now();
    let mut card = Card {
        hwnd,
        island: args.island,
        surf,
        dpi,
        origin,
        size_dip,
        c: Content {
            status: CardStatus::Listening,
            lines: Arc::new(Vec::new()),
            interim: Vec::new(),
            labelled: false,
            running_since: None,
            lang: content.lang,
            size: content.size,
        },
        pal: layout::palette(content.palette[0]),
        accent: content.palette[2],
        font: content.font,
        animations: content.animations,
        slow: false,
        labels: None,
        finals: Vec::new(),
        tail: Vec::new(),
        heights: Vec::new(),
        started: now,
        back: Tween::at_rest(0.0, now),
        manual: ManualScroll::default(),
        hover: Tween::at_rest(0.0, now),
        hovering: false,
        hover_zone: None,
        tracking: false,
        press: None,
        shown: true,
        gone: false,
        clock: None,
        placed: None,
        demo: args.demo,
    };
    // Drawn before it is stored; the timers wait until it can be found.
    card.take(content);
    card.render(now);
    card.shown = false;
    CARD.with(|c| *c.borrow_mut() = Some(card));
    if ON_SCREEN {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
    let _ = with(|c, _| {
        let now = Instant::now();
        c.shown = true;
        c.sync_timers(now);
        c.sync_clock(now);
    });
    true
}

pub(super) fn update(content: CardContent) {
    let _ = with(|c, _| {
        let now = Instant::now();
        c.take(content);
        c.render(now);
        c.sync_timers(now);
        c.sync_clock(now);
    });
}

/// Destroys the window and drops every Direct2D object now.
pub(super) fn close() {
    let card = CARD.with(|c| c.try_borrow_mut().ok().and_then(|mut c| c.take()));
    let Some(mut card) = card else {
        return;
    };
    card.kill_timers();
    card.clock = None;
    let hwnd = card.hwnd;
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    drop(card);
}

pub(super) fn hwnd() -> Option<HWND> {
    CARD.with(|c| c.try_borrow().ok().and_then(|c| c.as_ref().map(|c| c.hwnd)))
}

pub(super) fn take_placed() -> Option<CardPlace> {
    CARD.with(|c| {
        c.try_borrow_mut()
            .ok()
            .and_then(|mut c| c.as_mut().and_then(|c| c.placed.take()))
    })
}

/// The header clock: minutes and seconds of the session (the minutes keep
/// counting past the hour, so the text stays as narrow as its slot).
fn clock_text(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn failed_text(why: &str) -> String {
    if why.is_empty() {
        tr(Key::CapFailed).to_owned()
    } else {
        format!("{}: {why}", tr(Key::CapFailed))
    }
}

fn lang_text(lang: TrLang) -> &'static str {
    match lang {
        TrLang::Id => "ID",
        TrLang::En => "EN",
        TrLang::Auto => "Auto",
    }
}

// -------------------------------------------------------------- the card

impl Card {
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    fn size_px(&self) -> (i32, i32) {
        (px(self.size_dip.0, self.dpi), px(self.size_dip.1, self.dpi))
    }

    fn rects(&self) -> Rects {
        layout::card_rects(self.size_dip.0, self.size_dip.1)
    }

    fn elapsed_ms(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.started).as_millis() as u64
    }

    fn visible(&self) -> bool {
        self.shown && !self.gone
    }

    /// Captions are being heard (the dot is full, the clock runs).
    fn live(&self) -> bool {
        matches!(self.c.status, CardStatus::Running | CardStatus::Listening)
    }

    /// Pause and resume mean something: a session runs or is held.
    fn can_pause(&self) -> bool {
        matches!(
            self.c.status,
            CardStatus::Running | CardStatus::Listening | CardStatus::Paused | CardStatus::Battery
        )
    }

    /// The body's button is there to click.
    fn action_shown(&self) -> bool {
        matches!(self.view(), View::Prompt(..))
    }

    fn view(&self) -> View {
        match &self.c.status {
            CardStatus::Battery => View::Prompt(tr(Key::CapBattery), tr(Key::CapKeepGoing)),
            CardStatus::NeedsTranskrip => {
                View::Prompt(tr(Key::CapNeedsTranskrip), tr(Key::CapTurnOnTranskrip))
            }
            CardStatus::NeedsModel => View::Prompt(tr(Key::CapNeedsModel), tr(Key::CapDownload)),
            CardStatus::Downloading(p) => View::Progress((*p).min(100)),
            _ if !self.heights.is_empty() => View::Rows,
            CardStatus::Paused => View::Message(tr(Key::CapPaused).to_owned()),
            CardStatus::Failed(why) => View::Message(failed_text(why)),
            CardStatus::Listening | CardStatus::Running => {
                View::Message(tr(Key::CapListening).to_owned())
            }
        }
    }

    /// The line that says why no new captions come, under the ones there are.
    fn status_line(&self) -> Option<String> {
        match &self.c.status {
            CardStatus::Paused => Some(tr(Key::CapPaused).to_owned()),
            CardStatus::Failed(why) => Some(failed_text(why)),
            _ => None,
        }
    }

    fn body_px(&self) -> f32 {
        self.c.size.body_px()
    }

    /// Labels, the status line and the hint: smaller than the captions.
    fn small_px(&self) -> f32 {
        (self.body_px() * SMALL_SCALE).round().max(SMALL_MIN_PX)
    }

    /// Where the caption text starts: after the label column while labelled.
    fn text_x(&self) -> f32 {
        layout::PAD
            + self
                .labels
                .as_ref()
                .map_or(0.0, |l| l.w + layout::LABEL_GAP)
    }

    fn text_w(&self) -> f32 {
        (self.size_dip.0 - layout::PAD - self.text_x()).max(1.0)
    }

    /// Takes a new snapshot: only what changed is laid out again.
    fn take(&mut self, content: CardContent) {
        let mut all = false;
        if content.font != self.font {
            self.font = content.font;
            self.surf.set_font(content.font);
            all = true;
        }
        self.animations = content.animations;
        self.pal = layout::palette(content.palette[0]);
        self.accent = content.palette[2];
        all |= content.size != self.c.size || content.labelled != self.c.labelled;
        if let Change::Set(lines) = content.lines {
            self.c.lines = lines;
            all = true;
        }
        self.c.status = content.status;
        self.c.interim = content.interim;
        self.c.labelled = content.labelled;
        self.c.running_since = content.running_since;
        self.c.lang = content.lang;
        self.c.size = content.size;
        self.slow |= content.slow_hint;
        if all {
            self.build_labels();
            self.build_finals();
        }
        self.build_tail();
    }

    fn build_labels(&mut self) {
        self.labels = None;
        if !self.c.labelled {
            return;
        }
        let size = self.small_px();
        let me = self.surf.layout(tr(Key::CapMe), size, 600, 4096.0);
        let others = self.surf.layout(tr(Key::CapOthers), size, 600, 4096.0);
        let (Some(me), Some(others)) = (me, others) else {
            return;
        };
        let width = |l: &IDWriteTextLayout| Surface::metrics(l).widthIncludingTrailingWhitespace;
        self.labels = Some(Labels {
            w: width(&me).max(width(&others)).ceil(),
            baseline: Surface::first_baseline(&me),
            me,
            others,
        });
    }

    fn row(
        &mut self,
        who: Option<Speaker>,
        text: &str,
        rgb: u32,
        size: f32,
        weight: i32,
    ) -> Option<Row> {
        let width = self.text_w();
        let text = self.surf.layout(text, size, weight, width)?;
        Some(Row {
            who: who.filter(|_| self.labels.is_some()),
            h: Surface::height_of(&text),
            baseline: Surface::first_baseline(&text),
            text,
            rgb,
        })
    }

    /// The final lines, wrapped to the card's width.
    fn build_finals(&mut self) {
        let lines = self.c.lines.clone();
        let size = self.body_px();
        let (white, grey) = (self.pal.text, self.pal.grey);
        self.finals = lines
            .iter()
            .filter_map(|l| {
                if l.gap {
                    self.row(None, &l.text, grey, size, 400)
                } else {
                    self.row(l.speaker, &l.text, white, size, 400)
                }
            })
            .collect();
    }

    /// What follows the final lines: each channel's grey line, then (only
    /// under lines) why nothing new comes and the slow hint.
    fn build_tail(&mut self) {
        let grey = self.pal.grey;
        let (size, small) = (self.body_px(), self.small_px());
        let interim = std::mem::take(&mut self.c.interim);
        let mut tail: Vec<Row> = interim
            .iter()
            .filter(|(_, t)| !t.is_empty())
            .filter_map(|(who, t)| self.row(*who, t, grey, size, 400))
            .collect();
        self.c.interim = interim;
        if !self.finals.is_empty() || !tail.is_empty() {
            if let Some(why) = self.status_line() {
                tail.extend(self.row(None, &why, grey, small, 600));
            }
            if self.slow {
                tail.extend(self.row(None, tr(Key::CapSlow), grey, small, 400));
            }
        }
        self.tail = tail;
        self.heights = self.finals.iter().chain(&self.tail).map(|r| r.h).collect();
        let max = self.back_max();
        self.manual.clamp((0.0, max));
        if self.back.to > max {
            self.back = Tween::at_rest(max, Instant::now());
        }
    }

    fn back_max(&self) -> f32 {
        layout::scroll_back_max(&self.heights, layout::LINE_GAP, self.rects().body.h)
    }

    fn moving(&self, now: Instant) -> bool {
        self.back.moving(now) || self.hover.moving(now)
    }

    fn sync_clock(&mut self, now: Instant) {
        let want = self.visible() && self.moving(now);
        if want && self.clock.is_none() {
            self.clock = Some(FrameClock::start(self.hwnd.0 as isize));
        } else if !want {
            self.clock = None;
        }
    }

    fn kill_timers(&self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_CLOCK);
            let _ = KillTimer(Some(self.hwnd), TIMER_MANUAL);
        }
    }

    /// The one-shot timers: the header clock's next second (only while a
    /// session runs and the card is shown) and the manual-scroll return (only
    /// while held).
    fn sync_timers(&mut self, now: Instant) {
        let visible = self.visible();
        unsafe {
            match self.c.running_since.filter(|_| visible) {
                Some(since) => {
                    let into = now.saturating_duration_since(since).subsec_millis();
                    SetTimer(Some(self.hwnd), TIMER_CLOCK, (1000 - into).max(10), None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_CLOCK);
                }
            }
            let hold = if visible {
                self.manual.remaining_ms(self.elapsed_ms(now))
            } else {
                None
            };
            match hold {
                Some(ms) => {
                    SetTimer(Some(self.hwnd), TIMER_MANUAL, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_MANUAL);
                }
            }
        }
    }

    // ------------------------------------------------------------ drawing

    fn render(&mut self, now: Instant) {
        if !self.visible() {
            return;
        }
        let size = self.size_px();
        if self.surf.resize(size).is_err() {
            return;
        }
        let k = self.scale();
        let r = self.rects();
        let (w, h) = self.size_dip;
        let bounds = RECT {
            left: 0,
            top: 0,
            right: size.0,
            bottom: size.1,
        };
        {
            let rt = &self.surf.rt;
            unsafe {
                if rt.BindDC(self.surf.dc, &bounds).is_err() {
                    return;
                }
                rt.SetDpi(96.0 * k, 96.0 * k);
                rt.BeginDraw();
                rt.Clear(Some(&color(0, 0.0)));
            }
        }
        // The card: the dark window surface, with a hairline so its edge is
        // found over a dark video too.
        self.surf
            .fill_round(RectF::new(0.0, 0.0, w, h), layout::RADIUS, self.pal.bg, 1.0);
        self.surf.stroke_round(
            RectF::new(0.5, 0.5, w - 1.0, h - 1.0),
            layout::RADIUS - 0.5,
            self.pal.text,
            0.10,
            1.0,
        );
        self.paint_body(&r, now);
        self.paint_header(&r, now);
        let hover = self.hover.at(now);
        if hover > 0.001 {
            self.paint_grip(r.grip, 0.45 * hover);
        }
        let drawn = unsafe { self.surf.rt.EndDraw(None, None) };
        if drawn.is_ok() {
            self.present();
        }
    }

    /// A header button's plate under the pointer.
    fn plate(&self, zone: Zone, area: RectF) -> bool {
        let lit = self.hover_zone == Some(zone);
        if lit {
            self.surf.fill_round(area, 6.0, self.pal.text, 0.12);
        }
        lit
    }

    fn paint_header(&mut self, r: &Rects, now: Instant) {
        let pal = self.pal;
        // The dot: full while captions are heard, a ring while they are not
        // (the shape says it, not only the colour).
        let dot_rgb = if layout::contrast(self.accent, pal.bg) >= 3.0 {
            self.accent
        } else {
            pal.text
        };
        let centre = Vector2::new(r.dot.x + r.dot.w / 2.0, r.dot.y + r.dot.h / 2.0);
        unsafe {
            if self.live() {
                self.surf.brush.SetColor(&color(dot_rgb, 1.0));
                let e = D2D1_ELLIPSE {
                    point: centre,
                    radiusX: r.dot.w / 2.0,
                    radiusY: r.dot.h / 2.0,
                };
                self.surf.rt.FillEllipse(&e, &self.surf.brush);
            } else {
                self.surf.brush.SetColor(&color(pal.gap, 1.0));
                let e = D2D1_ELLIPSE {
                    point: centre,
                    radiusX: r.dot.w / 2.0 - 0.75,
                    radiusY: r.dot.h / 2.0 - 0.75,
                };
                self.surf.rt.DrawEllipse(&e, &self.surf.brush, 1.5, None);
            }
        }
        // The session's time, then the title (cut with an ellipsis).
        let time = match self.c.running_since {
            Some(since) => clock_text(now.saturating_duration_since(since).as_secs()),
            None => "--:--".to_owned(),
        };
        if let Some(l) = self.surf.one_line(&time, HEADER_PX, 600, r.time.w) {
            let y = r.time.y + (r.time.h - Surface::height_of(&l)) / 2.0;
            self.surf.draw_text((r.time.x, y), &l, pal.gap, 1.0);
        }
        if r.title.w >= HEADER_PX
            && let Some(l) = self
                .surf
                .one_line(tr(Key::CapTitle), HEADER_PX, 400, r.title.w)
        {
            let y = r.title.y + (r.title.h - Surface::height_of(&l)) / 2.0;
            self.surf.draw_text((r.title.x, y), &l, pal.gap, 1.0);
        }
        // The language chip: outlined, so it reads as something to click.
        let chip = RectF::new(
            r.lang_chip.x + 2.0,
            r.lang_chip.y + 2.0,
            r.lang_chip.w - 4.0,
            r.lang_chip.h - 4.0,
        );
        self.plate(Zone::LangChip, chip);
        self.surf.stroke_round(
            RectF::new(chip.x + 0.5, chip.y + 0.5, chip.w - 1.0, chip.h - 1.0),
            5.5,
            pal.gap,
            1.0,
            1.0,
        );
        if let Some(l) = self
            .surf
            .layout(lang_text(self.c.lang), CHIP_PX, 600, 4096.0)
        {
            self.surf.draw_centred(chip, &l, pal.text, 1.0);
        }
        let lit = self.plate(Zone::Copy, r.copy);
        if let Some(l) = self.surf.layout(tr(Key::CapCopy), HEADER_PX, 400, 4096.0) {
            let rgb = if lit { pal.text } else { pal.grey };
            self.surf.draw_centred(r.copy, &l, rgb, 1.0);
        }
        // Pause while it runs, play while it is held; dimmed when there is
        // no session to pause.
        let icon = if self.c.status == CardStatus::Paused {
            Icon::Play
        } else {
            Icon::Pause
        };
        if self.can_pause() {
            let lit = self.plate(Zone::Pause, r.pause);
            let rgb = if lit { pal.text } else { pal.grey };
            self.surf.icon(icon, r.pause, 16.0, rgb, 1.0);
        } else {
            self.surf.icon(icon, r.pause, 16.0, pal.grey, 0.35);
        }
        let lit = self.plate(Zone::Close, r.close);
        let rgb = if lit { pal.text } else { pal.grey };
        self.surf.icon(Icon::X, r.close, 16.0, rgb, 1.0);
    }

    fn paint_grip(&self, g: RectF, alpha: f32) {
        for off in [4.0f32, 8.0] {
            unsafe {
                self.surf.brush.SetColor(&color(self.pal.text, alpha));
                self.surf.rt.DrawLine(
                    Vector2::new(g.right() - 3.0, g.bottom() - 3.0 - off),
                    Vector2::new(g.right() - 3.0 - off, g.bottom() - 3.0),
                    &self.surf.brush,
                    1.5,
                    None,
                );
            }
        }
    }

    fn paint_body(&mut self, r: &Rects, now: Instant) {
        match self.view() {
            View::Rows => self.paint_rows(r.body, now),
            View::Message(text) => self.paint_message(r.body, &text),
            View::Prompt(text, button) => self.paint_prompt(r, text, button),
            View::Progress(percent) => self.paint_progress(r.body, percent),
        }
    }

    /// The lines on the body's bottom; what is cut off above (and, scrolled
    /// back, below) fades out instead of ending in a hard edge.
    fn paint_rows(&mut self, body: RectF, now: Instant) {
        let back = self.back.at(now);
        let tops = layout::bottom_stack(&self.heights, layout::LINE_GAP, body.h, back);
        let above = tops.first().map_or(0.0, |t| (-t).max(0.0));
        let top_fade = above.min(layout::EDGE_FADE) / body.h.max(1.0);
        let bottom_fade = back.clamp(0.0, layout::EDGE_FADE) / body.h.max(1.0);
        let opacity = self.surf.vertical_gradient(
            body.y,
            body.bottom(),
            &[
                (0.0, if top_fade > 0.0 { 0.0 } else { 1.0 }),
                (top_fade.min(0.5), 1.0),
                (1.0 - bottom_fade.min(0.5), 1.0),
                (1.0, if bottom_fade > 0.0 { 0.0 } else { 1.0 }),
            ],
        );
        let keep = self.surf.push_layer(body, opacity);
        let text_x = self.text_x();
        let pal = self.pal;
        for (row, top) in self.finals.iter().chain(&self.tail).zip(&tops) {
            let y = body.y + top;
            if y + row.h < body.y || y > body.bottom() {
                continue;
            }
            if let (Some(who), Some(labels)) = (row.who, self.labels.as_ref()) {
                let (label, rgb) = match who {
                    Speaker::Me => (&labels.me, pal.me),
                    Speaker::Others => (&labels.others, pal.others),
                };
                // The label sits on the first line's baseline.
                let dy = row.baseline - labels.baseline;
                self.surf.draw_text((layout::PAD, y + dy), label, rgb, 1.0);
            }
            self.surf.draw_text((text_x, y), &row.text, row.rgb, 1.0);
        }
        self.surf.pop_layer(keep);
    }

    /// One centred sentence.
    fn paint_message(&mut self, body: RectF, text: &str) {
        let width = (body.w - 2.0 * layout::PAD).max(1.0);
        let Some(l) = self.surf.layout(text, MESSAGE_PX, 400, width) else {
            return;
        };
        unsafe {
            let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
        }
        let keep = self.surf.push_layer(body, None);
        let y = body.y + ((body.h - Surface::height_of(&l)) / 2.0).max(0.0);
        self.surf
            .draw_text((layout::PAD, y), &l, self.pal.grey, 1.0);
        self.surf.pop_layer(keep);
    }

    /// Why captions wait, over the one button that gets them going.
    fn paint_prompt(&mut self, r: &Rects, text: &str, button: &str) {
        let pal = self.pal;
        let width = (r.body.w - 2.0 * layout::PAD).max(1.0);
        if let Some(l) = self.surf.one_line(text, MESSAGE_PX, 400, width) {
            unsafe {
                let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            }
            let y = (r.action.y - 10.0 - Surface::height_of(&l)).max(r.body.y);
            self.surf.draw_text((layout::PAD, y), &l, pal.text, 1.0);
        }
        // The card's one filled shape: white with the card's own colour on it.
        let lit = self.hover_zone == Some(Zone::Action);
        self.surf
            .fill_round(r.action, 8.0, pal.text, if lit { 0.88 } else { 1.0 });
        if let Some(l) = self.surf.one_line(button, 13.0, 600, r.action.w - 16.0) {
            unsafe {
                let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            }
            let y = r.action.y + (r.action.h - Surface::height_of(&l)) / 2.0;
            self.surf.draw_text((r.action.x + 8.0, y), &l, pal.bg, 1.0);
        }
    }

    /// The model download: the percent as text and as a bar.
    fn paint_progress(&mut self, body: RectF, percent: u8) {
        let pal = self.pal;
        let width = (body.w - 2.0 * layout::PAD).max(1.0);
        let text = format!("{} {percent}%", tr(Key::CapDownloading));
        let bar_w = width.min(200.0);
        let Some(l) = self.surf.one_line(&text, MESSAGE_PX, 400, width) else {
            return;
        };
        unsafe {
            let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
        }
        let text_h = Surface::height_of(&l);
        let (gap, bar_h) = (10.0, 4.0);
        let y = body.y + ((body.h - text_h - gap - bar_h) / 2.0).max(0.0);
        self.surf.draw_text((layout::PAD, y), &l, pal.text, 1.0);
        let track = RectF::new((body.w - bar_w) / 2.0, y + text_h + gap, bar_w, bar_h);
        self.surf.fill_round(track, bar_h / 2.0, pal.text, 0.20);
        if percent > 0 {
            let fill = (bar_w * f32::from(percent) / 100.0).max(bar_h);
            self.surf.fill_round(
                RectF::new(track.x, track.y, fill, bar_h),
                bar_h / 2.0,
                pal.text,
                1.0,
            );
        }
    }

    /// Pushes the drawn DIB to the window.
    fn present(&self) {
        let size = self.size_px();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&POINT {
                    x: self.origin.0,
                    y: self.origin.1,
                }),
                Some(&SIZE {
                    cx: size.0,
                    cy: size.1,
                }),
                Some(self.surf.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    // -------------------------------------------------------------- input

    fn to_dip(&self, x: i32, y: i32) -> (f32, f32) {
        let k = self.scale();
        (x as f32 / k, y as f32 / k)
    }

    fn zone(&self, x: f32, y: f32) -> Option<Zone> {
        layout::zone_at(&self.rects(), (x, y), self.action_shown())
    }

    fn pointer_for(&self, zone: Option<Zone>) -> Pointer {
        match zone {
            Some(Zone::Grip) => Pointer::Grip,
            Some(Zone::LangChip | Zone::Copy | Zone::Close | Zone::Action) => Pointer::Hand,
            Some(Zone::Pause) if self.can_pause() => Pointer::Hand,
            _ => Pointer::Arrow,
        }
    }

    fn set_hover(&mut self, on: bool, now: Instant) {
        if self.hovering == on {
            return;
        }
        self.hovering = on;
        self.hover
            .go(if on { 1.0 } else { 0.0 }, HOVER_MS, self.animations, now);
    }

    /// Asks Windows for `WM_MOUSELEAVE` (once; it is one-shot).
    fn arm_tracking(&mut self) {
        if self.tracking {
            return;
        }
        let mut t = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        if unsafe { TrackMouseEvent(&mut t) }.is_ok() {
            self.tracking = true;
        }
    }

    fn pointer_move(&mut self, x: i32, y: i32, fx: &mut Vec<Fx>) {
        let now = Instant::now();
        self.arm_tracking();
        if let Some(press) = &self.press {
            // A press that moves is a drag (or a resize from the grip).
            let mut cursor = POINT::default();
            if unsafe { GetCursorPos(&mut cursor) }.is_err() {
                return;
            }
            let (dx, dy) = (cursor.x - press.start.0, cursor.y - press.start.1);
            let thr = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
            let started = press.dragging || is_drag(dx, dy, thr.0, thr.1);
            let (zone, origin, size_dip) = (press.zone, press.origin, press.size_dip);
            if started && matches!(zone, Some(Zone::Header | Zone::Body | Zone::Grip)) {
                if let Some(p) = &mut self.press {
                    p.dragging = true;
                }
                if zone == Some(Zone::Grip) {
                    let k = self.scale();
                    let size =
                        layout::clamp_size(size_dip.0 + dx as f32 / k, size_dip.1 + dy as f32 / k);
                    self.resize_to(size, now);
                } else {
                    self.origin = (origin.0 + dx, origin.1 + dy);
                    self.present();
                }
            }
            return;
        }
        let (dx, dy) = self.to_dip(x, y);
        let was_moving = self.hover.moving(now);
        self.set_hover(true, now);
        let zone = self.zone(dx, dy);
        let changed = zone != self.hover_zone;
        self.hover_zone = zone;
        fx.push(Fx::Cursor(self.pointer_for(zone)));
        self.sync_clock(now);
        if changed || was_moving || self.hover.moving(now) {
            self.render(now);
        }
    }

    /// A new size (dip): the DIB and the wrapping follow.
    fn resize_to(&mut self, size_dip: (f32, f32), now: Instant) {
        let wrap_changed = (size_dip.0 - self.size_dip.0).abs() > 0.5;
        self.size_dip = size_dip;
        if wrap_changed {
            self.build_finals();
        }
        // Also keeps the scroll inside the new body.
        self.build_tail();
        self.render(now);
    }

    fn pointer_leave(&mut self) {
        self.tracking = false;
        if self.press.is_some() {
            return;
        }
        let now = Instant::now();
        self.set_hover(false, now);
        self.hover_zone = None;
        self.sync_clock(now);
        self.render(now);
    }

    fn button_down(&mut self, x: i32, y: i32, fx: &mut Vec<Fx>) {
        let (dx, dy) = self.to_dip(x, y);
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        self.press = Some(Press {
            zone: self.zone(dx, dy),
            start: (cursor.x, cursor.y),
            origin: self.origin,
            size_dip: self.size_dip,
            dragging: false,
        });
        fx.push(Fx::Capture(true));
    }

    fn button_up(&mut self, x: i32, y: i32, fx: &mut Vec<Fx>, post: &mut Vec<(Msg, isize)>) {
        let Some(press) = self.press.take() else {
            return;
        };
        fx.push(Fx::Capture(false));
        if press.dragging {
            self.finish_move(post);
        } else {
            // A press and a release on the same part is a click.
            let (dx, dy) = self.to_dip(x, y);
            if let Some(zone) = self.zone(dx, dy).filter(|z| Some(*z) == press.zone) {
                self.click(zone, post);
            }
        }
        self.after_press();
    }

    /// What a click on `zone` asks the island for.
    fn click(&self, zone: Zone, post: &mut Vec<(Msg, isize)>) {
        let msg = match zone {
            Zone::Close => Msg::Close,
            Zone::Copy => Msg::Copy,
            Zone::LangChip => Msg::Lang,
            Zone::Action => Msg::Action,
            Zone::Pause if self.can_pause() => {
                if self.c.status == CardStatus::Paused {
                    Msg::Resume
                } else {
                    Msg::Pause
                }
            }
            Zone::Pause | Zone::Header | Zone::Body | Zone::Grip => return,
        };
        post.push((msg, 0));
    }

    /// Capture was taken away mid-press (Alt+Tab, the Win key, a UAC prompt):
    /// a drag or resize that already moved the window is finished and saved,
    /// as a button release would. Our own `ReleaseCapture` in `button_up`
    /// lands here with the press already gone: nothing happens twice.
    fn capture_lost(&mut self, post: &mut Vec<(Msg, isize)>) {
        let Some(press) = self.press.take() else {
            return;
        };
        if press.dragging {
            self.finish_move(post);
        }
        self.after_press();
    }

    /// The press ended: the pointer may be outside the card now (`hovering`
    /// stayed true, `WM_MOUSELEAVE` was ignored while pressed) or inside
    /// with its tracking used up.
    fn after_press(&mut self) {
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        if !point_in((cursor.x, cursor.y), self.origin, self.size_px()) {
            self.pointer_leave();
            return;
        }
        let now = Instant::now();
        self.tracking = false;
        self.arm_tracking();
        let (x, y) = self.to_dip(cursor.x - self.origin.0, cursor.y - self.origin.1);
        let zone = self.zone(x, y);
        if zone != self.hover_zone {
            self.hover_zone = zone;
            self.render(now);
        }
    }

    /// A drag or resize ended: the monitor may have changed DPI; save the place.
    fn finish_move(&mut self, post: &mut Vec<(Msg, isize)>) {
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        if dpi >= 96 && dpi != self.dpi {
            self.set_dpi(dpi, None);
        }
        let size = self.size_px();
        self.placed = Some(CardPlace {
            left: self.origin.0,
            top: self.origin.1,
            right: self.origin.0 + size.0,
            bottom: self.origin.1 + size.1,
        });
        post.push((Msg::Placed, 0));
    }

    /// Windows moved the window itself (a monitor unplugged or rearranged, a
    /// display change): later renders keep it where it is now.
    fn sync_origin(&mut self) {
        let mut r = RECT::default();
        if unsafe { GetWindowRect(self.hwnd, &mut r) }.is_ok() {
            self.origin = (r.left, r.top);
        }
    }

    /// The window is gone: stop the timers and the frame clock for good.
    fn destroyed(&mut self) {
        self.gone = true;
        self.press = None;
        self.kill_timers();
        self.clock = None;
    }

    /// The window moved to a monitor of another DPI: the same size in dip.
    fn set_dpi(&mut self, dpi: u32, origin: Option<(i32, i32)>) {
        self.dpi = dpi.max(96);
        if let Some(o) = origin {
            self.origin = o;
        }
        self.render(Instant::now());
    }

    /// Ctrl + wheel asks for the next text size; a plain wheel scrolls back
    /// through the session and holds there for a while.
    fn wheel(&mut self, delta: i16, ctrl: bool, now: Instant, post: &mut Vec<(Msg, isize)>) {
        if ctrl {
            if delta != 0 {
                post.push((Msg::Size, isize::from(delta > 0)));
            }
            return;
        }
        if !matches!(self.view(), View::Rows) {
            return;
        }
        let notches = f32::from(delta) / 120.0;
        let bounds = (0.0, self.back_max());
        // `ManualScroll` counts down for a wheel away from the user; the
        // card's offset grows that way (further back).
        let goal = self
            .manual
            .wheel(self.back.at(now), -notches, bounds, self.elapsed_ms(now));
        self.back.go(goal, WHEEL_MS, self.animations, now);
        self.sync_timers(now);
        self.sync_clock(now);
        self.render(now);
    }

    fn timer(&mut self, id: usize, now: Instant) {
        match id {
            TIMER_CLOCK => {
                self.sync_timers(now);
                self.render(now);
            }
            TIMER_MANUAL => {
                if self.manual.due(self.elapsed_ms(now)) {
                    self.manual.release();
                    self.back.go(0.0, RETURN_MS, self.animations, now);
                    self.sync_clock(now);
                    self.render(now);
                }
                self.sync_timers(now);
            }
            _ => {}
        }
    }

    fn frame(&mut self, now: Instant) {
        if let Some(clock) = &self.clock {
            clock.ack();
        }
        self.render(now);
        self.sync_clock(now);
    }
}

// ------------------------------------------------------------ the procedure

fn lparam_xy(lparam: LPARAM) -> (i32, i32) {
    (
        i32::from(lparam.0 as i16),
        i32::from((lparam.0 >> 16) as i16),
    )
}

/// Tells the island (or, in the demo, the demo) what the user did.
fn emit(island: HWND, demo: bool, msg: Msg, value: isize) {
    if demo {
        super::demo_emit(msg, value);
        return;
    }
    if island.0.is_null() {
        return;
    }
    unsafe {
        let _ = PostMessageW(
            Some(island),
            WM_APP_CAPTION_CARD,
            WPARAM(msg.code()),
            LPARAM(value),
        );
    }
}

/// Runs `f` on the card and sends what it collected, after the borrow.
fn posting(f: impl FnOnce(&mut Card, &mut Vec<Fx>, &mut Vec<(Msg, isize)>)) {
    let mut post = Vec::new();
    let info = with(|c, fx| {
        f(c, fx, &mut post);
        (c.island, c.demo)
    });
    if let Some((island, demo)) = info {
        for (m, v) in post {
            emit(island, demo, m, v);
        }
    }
}

/// Never calls into `App`.
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        // A click works the card but never takes the focus from the meeting.
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            // The client area sets its own cursor on mouse moves.
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                return LRESULT(1);
            }
        }
        WM_MOUSEMOVE => {
            let (x, y) = lparam_xy(lparam);
            with(|c, fx| c.pointer_move(x, y, fx));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            with(|c, _| c.pointer_leave());
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_xy(lparam);
            with(|c, fx| c.button_down(x, y, fx));
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let (x, y) = lparam_xy(lparam);
            posting(|c, fx, post| c.button_up(x, y, fx, post));
            return LRESULT(0);
        }
        WM_CAPTURECHANGED => {
            // A drag or resize cut short (Alt+Tab, the Win key, a UAC prompt)
            // still saves where the card is.
            posting(|c, _, post| c.capture_lost(post));
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            // Only the card scrolls: the wheel never reaches the island.
            let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16;
            let ctrl = wparam.0 & MK_CONTROL != 0;
            posting(|c, _, post| c.wheel(delta, ctrl, Instant::now(), post));
            return LRESULT(0);
        }
        WM_TIMER => {
            with(|c, _| c.timer(wparam.0, Instant::now()));
            return LRESULT(0);
        }
        WM_APP_FRAME => {
            with(|c, _| c.frame(Instant::now()));
            return LRESULT(0);
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xFFFF) as u32;
            let rect = unsafe { &*(lparam.0 as *const RECT) };
            with(|c, _| c.set_dpi(dpi, Some((rect.left, rect.top))));
            return LRESULT(0);
        }
        // Every render passes the stored origin to `UpdateLayeredWindow`, so a
        // move Windows made must be read back or the next render undoes it.
        // A move the card made itself (inside `present`) arrives while the card
        // is borrowed and is skipped: the origin already holds it.
        WM_WINDOWPOSCHANGED => {
            with(|c, _| {
                if c.hwnd == hwnd {
                    c.sync_origin();
                }
            });
        }
        // App closes the card, like the X. DefWindowProc would destroy the
        // window behind App's back.
        WM_CLOSE => {
            posting(|_, _, post| post.push((Msg::Close, 0)));
            return LRESULT(0);
        }
        // `close` takes the card out before it destroys the window, so a card
        // still stored here was destroyed by someone else: App drops it.
        WM_DESTROY => {
            posting(|c, _, post| {
                if c.hwnd == hwnd && !c.gone {
                    c.destroyed();
                    post.push((Msg::Close, 0));
                }
            });
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics_card::layout::Mon;
    use crate::ui::settings_window::Rc;
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongW, IsWindow, IsWindowVisible, SendMessageW,
    };

    fn line(speaker: Option<Speaker>, text: &str, start_ms: u64) -> Line {
        Line {
            speaker,
            text: text.into(),
            start_ms,
            gap: false,
        }
    }

    fn content(status: CardStatus, running: bool) -> CardContent {
        let lines = vec![
            line(Some(Speaker::Others), "one", 0),
            Line {
                speaker: None,
                text: "...".into(),
                start_ms: 0,
                gap: true,
            },
            line(Some(Speaker::Me), "three", 2_000),
        ];
        CardContent {
            status,
            lines: Change::Set(Arc::new(lines)),
            interim: vec![(Some(Speaker::Me), "still hearing".into())],
            labelled: true,
            running_since: running.then(Instant::now),
            lang: TrLang::Id,
            size: TextSize::Medium,
            slow_hint: false,
            palette: [0x232527, 0x1B1D1F, 0xD97757],
            font: UiFont::Sans,
            animations: true,
        }
    }

    fn args() -> OpenArgs {
        let rect = Rc {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let mon = Mon {
            rect,
            work: Rc {
                bottom: 1040,
                ..rect
            },
            dpi: 96,
        };
        OpenArgs {
            island: HWND::default(),
            saved: None,
            monitors: vec![mon],
            notch: Some(mon),
            demo: true,
        }
    }

    /// A timer that exists can be killed (Ok); one that does not, cannot.
    fn timer_set(h: HWND, id: usize) -> bool {
        unsafe { KillTimer(Some(h), id) }.is_ok()
    }

    /// A message-only window standing in for the island (never on screen).
    fn fake_island() -> HWND {
        use windows::Win32::UI::WindowsAndMessaging::{
            HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
        };
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                None,
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .expect("a message-only window")
    }

    /// The card messages posted to `island`, oldest first (removed).
    fn posted(island: HWND) -> Vec<(usize, isize)> {
        use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW};
        let mut out = Vec::new();
        let mut m = MSG::default();
        while unsafe {
            PeekMessageW(
                &mut m,
                Some(island),
                WM_APP_CAPTION_CARD,
                WM_APP_CAPTION_CARD,
                PM_REMOVE,
            )
        }
        .as_bool()
        {
            out.push((m.wParam.0, m.lParam.0));
        }
        out
    }

    fn open_for(island: HWND, c: CardContent) -> HWND {
        let mut a = args();
        a.island = island;
        a.demo = false;
        assert!(open(a, c));
        hwnd().expect("the card exists")
    }

    fn press(dragging: bool) -> Press {
        Press {
            zone: Some(Zone::Header),
            start: (0, 0),
            origin: (0, 0),
            size_dip: (380.0, 220.0),
            dragging,
        }
    }

    #[test]
    fn open_update_close_without_a_window_on_screen() {
        assert!(open(args(), content(CardStatus::Running, true)));
        // A second open does nothing: one card.
        assert!(!open(args(), content(CardStatus::Running, true)));
        let h = hwnd().expect("the card exists");
        assert!(
            !unsafe { IsWindowVisible(h) }.as_bool(),
            "tests never show it"
        );
        // Every status draws, with and without lines, labels and a slow hint.
        for status in [
            CardStatus::Listening,
            CardStatus::Running,
            CardStatus::Paused,
            CardStatus::Battery,
            CardStatus::NeedsTranskrip,
            CardStatus::NeedsModel,
            CardStatus::Downloading(40),
            CardStatus::Failed("no device".into()),
        ] {
            update(content(status.clone(), false));
            let mut bare = content(status, false);
            bare.lines = Change::Set(Arc::new(Vec::new()));
            bare.interim.clear();
            bare.labelled = false;
            bare.slow_hint = true;
            bare.animations = false;
            update(bare);
        }
        update(content(CardStatus::Running, true));
        for size in [TextSize::Small, TextSize::Large] {
            let mut c = content(CardStatus::Running, true);
            c.lines = Change::Keep;
            c.size = size;
            update(c);
        }
        assert_eq!(with(|c, _| c.finals.len()), Some(3), "Keep keeps the lines");
        assert!(take_placed().is_none());
        close();
        assert!(hwnd().is_none());
        assert!(!unsafe { IsWindow(Some(h)) }.as_bool());
        // Closing again is harmless, and it can open again.
        close();
        assert!(open(args(), content(CardStatus::Listening, false)));
        close();
    }

    #[test]
    fn the_slow_hint_stays_until_the_card_opens_again() {
        assert!(open(args(), content(CardStatus::Running, true)));
        assert_eq!(with(|c, _| c.slow), Some(false));
        let mut slow = content(CardStatus::Running, true);
        slow.slow_hint = true;
        update(slow);
        update(content(CardStatus::Running, true));
        assert_eq!(with(|c, _| c.slow), Some(true), "a later snapshot keeps it");
        close();
        assert!(open(args(), content(CardStatus::Running, true)));
        assert_eq!(with(|c, _| c.slow), Some(false));
        close();
    }

    #[test]
    fn the_clock_timer_runs_only_while_running_and_shown() {
        assert!(open(args(), content(CardStatus::Running, true)));
        let h = hwnd().unwrap();
        assert!(timer_set(h, TIMER_CLOCK), "a running session arms it");
        update(content(CardStatus::Running, true));
        assert!(timer_set(h, TIMER_CLOCK), "re-armed by the next snapshot");
        // A fire re-arms it for the next second.
        unsafe { SendMessageW(h, WM_TIMER, Some(WPARAM(TIMER_CLOCK)), None) };
        assert!(timer_set(h, TIMER_CLOCK));
        update(content(CardStatus::Paused, false));
        assert!(!timer_set(h, TIMER_CLOCK), "nothing runs: none");
        unsafe { SendMessageW(h, WM_TIMER, Some(WPARAM(TIMER_CLOCK)), None) };
        assert!(!timer_set(h, TIMER_CLOCK), "a late fire does not re-arm it");
        assert!(!timer_set(h, TIMER_MANUAL));
        // Not shown any more: a snapshot arms nothing.
        update(content(CardStatus::Running, true));
        let _ = with(|c, _| c.shown = false);
        update(content(CardStatus::Running, true));
        assert!(!timer_set(h, TIMER_CLOCK), "not shown: none");
        close();
    }

    #[test]
    fn a_click_never_activates() {
        use windows::Win32::UI::WindowsAndMessaging::{MA_NOACTIVATE, WM_MOUSEACTIVATE};
        assert!(open(args(), content(CardStatus::Running, true)));
        let h = hwnd().unwrap();
        let r = unsafe { SendMessageW(h, WM_MOUSEACTIVATE, None, None) };
        assert_eq!(r.0 as u32, MA_NOACTIVATE);
        let ex = unsafe { GetWindowLongW(h, GWL_EXSTYLE) } as u32;
        for bit in [
            WS_EX_NOACTIVATE,
            WS_EX_TOPMOST,
            WS_EX_TOOLWINDOW,
            WS_EX_LAYERED,
        ] {
            assert_eq!(ex & bit.0, bit.0);
        }
        close();
    }

    #[test]
    fn capture_lost_after_a_drag_posts_one_placed() {
        let island = fake_island();
        let h = open_for(island, content(CardStatus::Running, true));
        let _ = with(|c, _| c.press = Some(press(true)));
        // Alt+Tab, the Win key or a UAC prompt takes the capture.
        unsafe { SendMessageW(h, WM_CAPTURECHANGED, None, None) };
        assert_eq!(posted(island), vec![(Msg::Placed.code(), 0)]);
        assert!(take_placed().is_some());
        // The WM_CAPTURECHANGED of our own ReleaseCapture after a release: the
        // press is gone by then, so nothing more is posted.
        unsafe { SendMessageW(h, WM_CAPTURECHANGED, None, None) };
        assert!(posted(island).is_empty());
        // A click that is not a drag posts nothing either.
        let _ = with(|c, _| c.press = Some(press(false)));
        unsafe { SendMessageW(h, WM_CAPTURECHANGED, None, None) };
        assert!(posted(island).is_empty());
        assert!(take_placed().is_none());
        close();
        unsafe {
            let _ = DestroyWindow(island);
        }
    }

    fn wheel_wparam(delta: i16, ctrl: bool) -> WPARAM {
        WPARAM(((delta as u16 as usize) << 16) | if ctrl { MK_CONTROL } else { 0 })
    }

    #[test]
    fn ctrl_wheel_posts_size_and_plain_wheel_scrolls() {
        let island = fake_island();
        let mut c = content(CardStatus::Running, true);
        let many: Vec<Line> = (0..60)
            .map(|i| line(None, "a line of speech", i * 1_000))
            .collect();
        c.lines = Change::Set(Arc::new(many));
        let h = open_for(island, c);
        // Ctrl + wheel: the text size, never a scroll.
        unsafe { SendMessageW(h, WM_MOUSEWHEEL, Some(wheel_wparam(120, true)), None) };
        unsafe { SendMessageW(h, WM_MOUSEWHEEL, Some(wheel_wparam(-120, true)), None) };
        assert_eq!(
            posted(island),
            vec![(Msg::Size.code(), 1), (Msg::Size.code(), 0)]
        );
        assert_eq!(with(|c, _| c.manual.offset()), Some(None));
        assert!(!timer_set(h, TIMER_MANUAL));
        // A plain wheel away from the user scrolls back and holds for 3 s.
        unsafe { SendMessageW(h, WM_MOUSEWHEEL, Some(wheel_wparam(240, false)), None) };
        assert_eq!(
            with(|c, _| c.manual.offset()),
            Some(Some(2.0 * layout::WHEEL_STEP))
        );
        assert!(
            posted(island).is_empty(),
            "scrolling tells the island nothing"
        );
        assert!(timer_set(h, TIMER_MANUAL), "the return is armed");
        // Towards the user: forward again, never past the newest line.
        unsafe { SendMessageW(h, WM_MOUSEWHEEL, Some(wheel_wparam(-1200, false)), None) };
        assert_eq!(with(|c, _| c.manual.offset()), Some(Some(0.0)));
        // The return: the hold ends and the list rests on the newest line.
        let rest = with(|c, _| {
            c.manual.wheel(0.0, -3.0, (0.0, 9_999.0), 0);
            c.started = Instant::now() - std::time::Duration::from_secs(10);
            c.animations = false;
            c.timer(TIMER_MANUAL, Instant::now());
            (c.manual.offset(), c.back.at(Instant::now()))
        });
        assert_eq!(rest, Some((None, 0.0)));
        close();
        unsafe {
            let _ = DestroyWindow(island);
        }
    }

    #[test]
    fn wm_close_and_a_destroy_behind_app_s_back_ask_the_island_to_close() {
        let island = fake_island();
        let h = open_for(island, content(CardStatus::Running, true));
        unsafe { SendMessageW(h, WM_CLOSE, None, None) };
        assert!(
            unsafe { IsWindow(Some(h)) }.as_bool(),
            "the window lives until App closes it"
        );
        assert_eq!(posted(island), vec![(Msg::Close.code(), 0)]);
        unsafe {
            let _ = DestroyWindow(h);
        }
        assert_eq!(posted(island), vec![(Msg::Close.code(), 0)]);
        // App then drops the stale card; nothing else is posted.
        close();
        assert!(hwnd().is_none());
        assert!(posted(island).is_empty());
        unsafe {
            let _ = DestroyWindow(island);
        }
    }

    #[test]
    fn a_click_posts_what_the_button_does() {
        assert!(open(args(), content(CardStatus::Running, true)));
        let clicks = |status: CardStatus| {
            let mut c = content(status, false);
            c.lines = Change::Keep;
            update(c);
            with(|c, _| {
                let r = c.rects();
                let mut post = Vec::new();
                for b in [r.close, r.copy, r.pause, r.lang_chip, r.action] {
                    let (x, y) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
                    if let Some(z) = c.zone(x, y) {
                        c.click(z, &mut post);
                    }
                }
                post.into_iter().map(|(m, _)| m).collect::<Vec<_>>()
            })
            .unwrap()
        };
        assert_eq!(
            clicks(CardStatus::Running),
            [Msg::Close, Msg::Copy, Msg::Pause, Msg::Lang]
        );
        assert_eq!(
            clicks(CardStatus::Paused),
            [Msg::Close, Msg::Copy, Msg::Resume, Msg::Lang]
        );
        // A status with a button: the body's button answers. Nothing runs
        // there, so the pause button has nothing to pause.
        assert_eq!(
            clicks(CardStatus::NeedsModel),
            [Msg::Close, Msg::Copy, Msg::Lang, Msg::Action]
        );
        assert_eq!(
            clicks(CardStatus::NeedsTranskrip),
            [Msg::Close, Msg::Copy, Msg::Lang, Msg::Action]
        );
        assert_eq!(
            clicks(CardStatus::Battery),
            [Msg::Close, Msg::Copy, Msg::Pause, Msg::Lang, Msg::Action]
        );
        assert_eq!(
            clicks(CardStatus::Downloading(3)),
            [Msg::Close, Msg::Copy, Msg::Lang]
        );
        close();
    }

    #[test]
    fn the_clock_reads_minutes_and_seconds() {
        assert_eq!(clock_text(0), "0:00");
        assert_eq!(clock_text(59), "0:59");
        assert_eq!(clock_text(12 * 60 + 48), "12:48");
        assert_eq!(clock_text(3_600 + 125), "62:05");
    }
}
