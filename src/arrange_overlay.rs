//! Susun's two overlays, both layered windows drawn with
//! `UpdateLayeredWindow` from a premultiplied DIB (like `ui/render.rs`).
//!
//! - The **outline**: click-through (`WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`),
//!   shows the tiles of the template under the pointer, dashed, on the target
//!   monitor's work area.
//! - The **pick overlay**: covers the whole virtual screen and takes clicks.
//!   A layered window passes clicks through pixels whose alpha is 0, so the
//!   whole canvas is cleared to alpha 1/255 (invisible, but it counts as
//!   there). It draws every tile of the layout (the current one filled, the
//!   chosen ones with the app's icon and name, the rest dashed), lights the
//!   window under the pointer, and posts what the user did to the island
//!   (`WM_APP_ARRANGE`): a window clicked, Backspace, Esc or the foreground
//!   lost.
//!
//! One pick overlay at a time. Nothing runs at idle: no timer, no polling;
//! it redraws only when what it shows changes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_CAP_STYLE_FLAT, D2D1_DASH_STYLE_DASH,
    D2D1_LINE_JOIN_MITER, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, ID2D1Bitmap, ID2D1DCRenderTarget,
    ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HDC, HGDIOBJ,
    SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_BACK, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, GA_ROOTOWNER, GetAncestor,
    GetForegroundWindow, GetWindowThreadProcessId, HWND_TOPMOST, IDC_ARROW, IsWindow, LoadCursorW,
    PostMessageW, RegisterClassExW, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetForegroundWindow, SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WM_ACTIVATE,
    WM_ACTIVATEAPP, WM_APP, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_RBUTTONUP, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Error, PCWSTR, w};

use crate::arrange::Rect;
use crate::clipboard::image::Thumb;
use crate::shot_edit::gfx::{Align, Gfx, bitmap, color, rect_f};

/// Posted to the island by the pick overlay. `wparam` is one of the `MSG_*`
/// codes; `lparam` is the window clicked for `MSG_PICK`.
pub const WM_APP_ARRANGE: u32 = WM_APP + 61;
pub const MSG_PICK: usize = 1;
pub const MSG_BACK: usize = 2;
pub const MSG_CANCEL: usize = 3;

const CLASS_PICK: PCWSTR = w!("ValeArrangePick");
const CLASS_OUTLINE: PCWSTR = w!("ValeArrangeOutline");

/// One tile as the pick overlay draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum TileState {
    /// Not reached yet: dashed.
    Waiting,
    /// The one being picked: a 2 px border and a 20 % fill.
    Current,
    /// A window was chosen: its icon and name.
    Filled {
        name: String,
        key: String,
        icon: Option<Thumb>,
    },
    /// Skipped: dashed, dimmer.
    Skipped,
}

/// What the pick overlay needs to start.
pub struct PickSpec {
    /// The island, which stays above the overlay and gets its messages.
    pub island: HWND,
    /// The tiles in virtual-screen px, in pick order.
    pub tiles: Vec<Rect>,
    pub accent: u32,
    /// The windows that may be picked.
    pub allowed: Vec<isize>,
    /// The windows on screen, frontmost first, for the hit test.
    pub windows: Vec<(isize, Rect)>,
    /// The virtual screen the overlay covers.
    pub area: Rect,
    /// `--demo-arrange`: click-through, takes no foreground, posts nothing.
    pub demo: bool,
}

// ------------------------------------------------------------------ canvas

/// A DIB the size of a window, a software Direct2D target bound to it and
/// the way to push it to a layered window.
struct Canvas {
    gfx: Gfx,
    rt: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    dashed: ID2D1StrokeStyle,
    dc: HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    previous: HGDIOBJ,
    size: (i32, i32),
    bitmaps: HashMap<String, ID2D1Bitmap>,
}

impl Canvas {
    fn new(size: (i32, i32)) -> windows::core::Result<Canvas> {
        let gfx = Gfx::new()?;
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
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e);
                }
            };
            let previous = SelectObject(dc, bitmap.into());
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
                let rt = gfx.d2d.CreateDCRenderTarget(&props)?;
                rt.SetTextAntialiasMode(
                    windows::Win32::Graphics::Direct2D::D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                );
                let brush = rt.CreateSolidColorBrush(&color(0, 1.0), None)?;
                let dashed = gfx.d2d.CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        startCap: D2D1_CAP_STYLE_FLAT,
                        endCap: D2D1_CAP_STYLE_FLAT,
                        dashCap: D2D1_CAP_STYLE_FLAT,
                        lineJoin: D2D1_LINE_JOIN_MITER,
                        miterLimit: 10.0,
                        dashStyle: D2D1_DASH_STYLE_DASH,
                        dashOffset: 0.0,
                    },
                    None,
                )?;
                Ok::<_, Error>((rt, brush, dashed))
            })();
            match made {
                Ok((rt, brush, dashed)) => Ok(Canvas {
                    gfx,
                    rt,
                    brush,
                    dashed,
                    dc,
                    bitmap,
                    previous,
                    size,
                    bitmaps: HashMap::new(),
                }),
                Err(e) => {
                    SelectObject(dc, previous);
                    let _ = DeleteObject(bitmap.into());
                    let _ = DeleteDC(dc);
                    Err(e)
                }
            }
        }
    }

    /// Clears to `base_alpha` (0: see-through and click-through; 1/255: see
    /// through but it takes clicks), runs `f`, and pushes the result to `hwnd`
    /// at `origin`.
    fn paint(
        &self,
        hwnd: HWND,
        origin: (i32, i32),
        base_alpha: f32,
        f: impl FnOnce(&Canvas),
    ) -> windows::core::Result<()> {
        let bounds = RECT {
            left: 0,
            top: 0,
            right: self.size.0,
            bottom: self.size.1,
        };
        unsafe {
            self.rt.BindDC(self.dc, &bounds)?;
            self.rt.BeginDraw();
            self.rt.Clear(Some(&color(0, base_alpha)));
        }
        f(self);
        unsafe { self.rt.EndDraw(None, None)? };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&POINT {
                    x: origin.0,
                    y: origin.1,
                }),
                Some(&SIZE {
                    cx: self.size.0,
                    cy: self.size.1,
                }),
                Some(self.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }

    fn fill(&self, r: Rect, radius: f32, rgb: u32, alpha: f32) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.rt
                .FillRoundedRectangle(&rounded(r, radius, 0.0), &self.brush);
        }
    }

    fn stroke(&self, r: Rect, radius: f32, rgb: u32, alpha: f32, width: f32, dashed: bool) {
        unsafe {
            self.brush.SetColor(&color(rgb, alpha));
            self.rt.DrawRoundedRectangle(
                &rounded(r, radius, width / 2.0),
                &self.brush,
                width,
                if dashed { Some(&self.dashed) } else { None },
            );
        }
    }

    fn text(&self, s: &str, area: (f32, f32, f32, f32), size: f32, rgb: u32, alpha: f32) {
        self.gfx.text_in(
            &self.rt,
            &self.brush,
            s,
            area,
            size,
            500,
            rgb,
            alpha,
            Align::Center,
        );
    }

    /// The icon `key` as a bitmap of this target, made once.
    fn icon(&mut self, key: &str, thumb: &Thumb) -> Option<ID2D1Bitmap> {
        if let Some(b) = self.bitmaps.get(key) {
            return Some(b.clone());
        }
        let b = bitmap(&self.rt, thumb.w, thumb.h, &thumb.bgra).ok()?;
        self.bitmaps.insert(key.to_owned(), b.clone());
        Some(b)
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// `r` as a rounded rectangle, grown inward by `inset` (for a stroke that
/// stays inside).
fn rounded(r: Rect, radius: f32, inset: f32) -> D2D1_ROUNDED_RECT {
    let radius = (radius - inset).max(0.0);
    D2D1_ROUNDED_RECT {
        rect: rect_f(
            r.x as f32 + inset,
            r.y as f32 + inset,
            (r.w as f32 - 2.0 * inset).max(0.0),
            (r.h as f32 - 2.0 * inset).max(0.0),
        ),
        radiusX: radius,
        radiusY: radius,
    }
}

// ------------------------------------------------------------------ windows

fn register(class: PCWSTR) -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(if class == CLASS_PICK {
            pick_proc
        } else {
            outline_proc
        }),
        hInstance: module.into(),
        hCursor: cursor,
        lpszClassName: class,
        ..Default::default()
    };
    unsafe {
        if RegisterClassExW(&wc) == 0 {
            let error = Error::from_thread();
            return error.code() == ERROR_CLASS_ALREADY_EXISTS.to_hresult();
        }
    }
    true
}

fn create_window(class: PCWSTR, area: Rect, click_through: bool) -> Option<HWND> {
    if !register(class) {
        return None;
    }
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    let mut ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
    if click_through {
        ex |= WS_EX_TRANSPARENT | WS_EX_NOACTIVATE;
    }
    unsafe {
        CreateWindowExW(
            ex,
            class,
            w!("Vale arrange"),
            WS_POPUP,
            area.x,
            area.y,
            area.w,
            area.h,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .ok()
}

/// The accent over a dashed outline and its fill, in the island's colour.
fn paint_tile_outline(c: &Canvas, r: Rect, accent: u32) {
    c.stroke(r, 8.0, accent, 0.95, 2.0, true);
}

// ------------------------------------------------------------------ outline

thread_local! {
    static OUTLINE: RefCell<Option<(HWND, Canvas)>> = const { RefCell::new(None) };
    static PICK: RefCell<Option<Pick>> = const { RefCell::new(None) };
}

unsafe extern "system" fn outline_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Shows `tiles` (virtual-screen px) dashed on `area` (the target monitor's
/// work area), click-through. Replaces what an earlier call showed.
pub fn outline_show(area: Rect, tiles: &[Rect], accent: u32) {
    outline_hide();
    if area.is_empty() {
        return;
    }
    let Some(hwnd) = create_window(CLASS_OUTLINE, area, true) else {
        return;
    };
    let Ok(canvas) = Canvas::new((area.w, area.h)) else {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return;
    };
    let drawn = canvas.paint(hwnd, (area.x, area.y), 0.0, |c| {
        for t in tiles {
            let local = Rect::new(t.x - area.x, t.y - area.y, t.w, t.h);
            c.fill(local, 8.0, accent, 0.08);
            paint_tile_outline(c, local, accent);
        }
    });
    if drawn.is_err() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return;
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
    OUTLINE.with(|o| {
        if let Ok(mut slot) = o.try_borrow_mut() {
            *slot = Some((hwnd, canvas));
        }
    });
}

/// Takes the outline down.
pub fn outline_hide() {
    let taken = OUTLINE.with(|o| o.try_borrow_mut().ok().and_then(|mut s| s.take()));
    if let Some((hwnd, canvas)) = taken {
        drop(canvas);
        unsafe {
            if IsWindow(Some(hwnd)).as_bool() {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

// --------------------------------------------------------------------- pick

struct Pick {
    hwnd: HWND,
    island: HWND,
    canvas: Canvas,
    area: Rect,
    tiles: Vec<Rect>,
    states: Vec<TileState>,
    accent: u32,
    allowed: Vec<isize>,
    windows: Vec<(isize, Rect)>,
    /// The window under the pointer that may be picked, and its frame.
    hover: Option<(isize, Rect)>,
    demo: bool,
    /// Set once the overlay holds the foreground: from then on losing it
    /// cancels. Not before, so the brief input join in `take_foreground`
    /// does not end it (the `armed` gotcha of the capture overlay).
    armed: bool,
}

/// True while a pick overlay is up.
pub fn active() -> bool {
    PICK.with(|p| p.try_borrow().map_or(true, |s| s.is_some()))
}

fn with<R>(f: impl FnOnce(&mut Pick) -> R) -> Option<R> {
    PICK.with(|p| p.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

/// Puts the pick overlay up and takes the foreground for the keys. False when
/// it could not (nothing is left up then). Ignored while one is up.
pub fn start(spec: PickSpec) -> bool {
    if active() {
        return false;
    }
    let Some(hwnd) = create_window(CLASS_PICK, spec.area, spec.demo) else {
        return false;
    };
    let Ok(canvas) = Canvas::new((spec.area.w, spec.area.h)) else {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return false;
    };
    let n = spec.tiles.len();
    let mut states = vec![TileState::Waiting; n];
    if let Some(first) = states.first_mut() {
        *first = TileState::Current;
    }
    let pick = Pick {
        hwnd,
        island: spec.island,
        canvas,
        area: spec.area,
        tiles: spec.tiles,
        states,
        accent: spec.accent,
        allowed: spec.allowed,
        windows: spec.windows,
        hover: None,
        demo: spec.demo,
        armed: false,
    };
    let stored = PICK.with(|p| match p.try_borrow_mut() {
        Ok(mut slot) if slot.is_none() => {
            *slot = Some(pick);
            true
        }
        _ => false,
    });
    if !stored {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return false;
    }
    redraw();
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
    if !spec.demo {
        if !take_foreground(hwnd) {
            // The keys (Backspace, Esc) would go to the app underneath.
            eprintln!("vale: the arrange overlay could not take the keyboard");
            close();
            return false;
        }
        with(|p| p.armed = true);
    }
    raise_island();
    true
}

/// The tiles' states after a pick, a skip or a step back, and the windows
/// that may still be picked.
pub fn update(states: Vec<TileState>, allowed: Vec<isize>) {
    with(|p| {
        if states.len() == p.tiles.len() {
            p.states = states;
        }
        p.allowed = allowed;
        p.hover = None;
    });
    redraw();
}

/// Closes the overlay (no message is posted for this one).
pub fn close() {
    let taken = PICK.with(|p| p.try_borrow_mut().ok().and_then(|mut s| s.take()));
    if let Some(pick) = taken {
        let hwnd = pick.hwnd;
        drop(pick);
        unsafe {
            if IsWindow(Some(hwnd)).as_bool() {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

/// The island goes back above the overlay (the overlay is topmost too, and
/// a click on it raises it).
fn raise_island() {
    if let Some(island) = with(|p| p.island)
        && !island.is_invalid()
    {
        unsafe {
            let _ = SetWindowPos(
                island,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

/// Brings the overlay to the foreground with the keyboard focus. Windows may
/// refuse a plain `SetForegroundWindow` (the foreground lock, after other
/// input); then this thread's input is joined to the foreground window's for
/// a moment, which it allows. True when the overlay has it.
fn take_foreground(hwnd: HWND) -> bool {
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() != hwnd {
            let ours = GetCurrentThreadId();
            let theirs = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let joined =
                theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, true).as_bool();
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            if joined {
                let _ = AttachThreadInput(ours, theirs, false);
            }
        }
        let _ = SetFocus(Some(hwnd));
        GetForegroundWindow() == hwnd
    }
}

fn post(code: usize, data: isize) {
    if let Some((island, demo)) = with(|p| (p.island, p.demo))
        && !demo
    {
        unsafe {
            let _ = PostMessageW(Some(island), WM_APP_ARRANGE, WPARAM(code), LPARAM(data));
        }
    }
}

/// The window of `windows` (frontmost first) under `pt` that may be picked:
/// the frontmost window there, or its root owner when that is one.
fn pickable_at(
    windows: &[(isize, Rect)],
    allowed: &[isize],
    pt: (i32, i32),
    owner_of: impl Fn(isize) -> isize,
) -> Option<(isize, Rect)> {
    let (top, rect) = windows.iter().find(|(_, r)| r.contains(pt))?;
    if allowed.contains(top) {
        return Some((*top, *rect));
    }
    let owner = owner_of(*top);
    if owner != *top && allowed.contains(&owner) {
        let frame = windows
            .iter()
            .find(|(h, _)| *h == owner)
            .map_or(*rect, |(_, r)| *r);
        return Some((owner, frame));
    }
    None
}

fn root_owner(hwnd: isize) -> isize {
    unsafe { GetAncestor(HWND(hwnd as *mut c_void), GA_ROOTOWNER).0 as isize }
}

fn redraw() {
    let drawn = with(|p| {
        let Pick {
            hwnd,
            canvas,
            area,
            tiles,
            states,
            accent,
            hover,
            ..
        } = p;
        let (hwnd, area, accent) = (*hwnd, *area, *accent);
        // Make the icon bitmaps first: painting borrows the canvas shared.
        let mut icons: HashMap<String, ID2D1Bitmap> = HashMap::new();
        for s in states.iter() {
            if let TileState::Filled {
                key,
                icon: Some(thumb),
                ..
            } = s
                && let Some(b) = canvas.icon(key, thumb)
            {
                icons.insert(key.clone(), b);
            }
        }
        canvas.paint(hwnd, (area.x, area.y), 1.0 / 255.0, |c| {
            let local = |r: &Rect| Rect::new(r.x - area.x, r.y - area.y, r.w, r.h);
            if let Some((_, frame)) = hover {
                let f = local(frame);
                c.fill(f, 6.0, accent, 0.12);
                c.stroke(f, 6.0, accent, 0.9, 3.0, false);
            }
            for (tile, state) in tiles.iter().zip(states.iter()) {
                let r = local(tile);
                match state {
                    TileState::Current => {
                        c.fill(r, 8.0, accent, 0.20);
                        c.stroke(r, 8.0, accent, 1.0, 2.0, false);
                    }
                    TileState::Waiting => paint_tile_outline(c, r, accent),
                    TileState::Skipped => c.stroke(r, 8.0, accent, 0.45, 1.5, true),
                    TileState::Filled { name, key, .. } => {
                        c.fill(r, 8.0, 0x000000, 0.45);
                        c.stroke(r, 8.0, accent, 0.9, 1.5, false);
                        let side = 40.0f32.min(r.w as f32 - 16.0).min(r.h as f32 - 40.0);
                        let (cx, cy) =
                            (r.x as f32 + r.w as f32 / 2.0, r.y as f32 + r.h as f32 / 2.0);
                        if let Some(b) = icons.get(key)
                            && side > 8.0
                        {
                            unsafe {
                                c.rt.DrawBitmap(
                                    b,
                                    Some(&rect_f(cx - side / 2.0, cy - side - 2.0, side, side)),
                                    1.0,
                                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                    None,
                                );
                            }
                        }
                        c.text(
                            name,
                            (r.x as f32 + 8.0, cy + 2.0, r.w as f32 - 16.0, 22.0),
                            14.0,
                            0xFFFFFF,
                            1.0,
                        );
                    }
                }
            }
        })
    });
    if let Some(Err(error)) = drawn {
        eprintln!("vale: the arrange overlay could not draw: {error}");
        post(MSG_CANCEL, 0);
    }
}

/// The overlay lost the foreground: keys now go to another window, so it
/// cancels. Ignored until `start` has armed it.
fn deactivated() {
    if with(|p| p.armed) == Some(true) {
        post(MSG_CANCEL, 0);
    }
}

unsafe extern "system" fn pick_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let point = |lparam: LPARAM, p: &Pick| {
        (
            p.area.x + i32::from(lparam.0 as i16),
            p.area.y + i32::from((lparam.0 >> 16) as i16),
        )
    };
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            let changed = with(|p| {
                let at = point(lparam, p);
                let now = pickable_at(&p.windows, &p.allowed, at, root_owner);
                let changed = now != p.hover;
                p.hover = now;
                changed
            });
            if changed == Some(true) {
                redraw();
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let clicked = with(|p| {
                let at = point(lparam, p);
                pickable_at(&p.windows, &p.allowed, at, root_owner).map(|(h, _)| h)
            })
            .flatten();
            raise_island();
            if let Some(h) = clicked {
                post(MSG_PICK, h);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => LRESULT(0),
        WM_KEYDOWN => {
            match wparam.0 as u16 {
                x if x == VK_BACK.0 => post(MSG_BACK, 0),
                x if x == VK_ESCAPE.0 => post(MSG_CANCEL, 0),
                _ => {}
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if wparam.0 & 0xFFFF == 0 {
                deactivated();
            } else {
                // A click on the overlay raised it: the island goes back on top.
                raise_island();
            }
            LRESULT(0)
        }
        WM_ACTIVATEAPP if wparam.0 == 0 => {
            deactivated();
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            post(MSG_CANCEL, 0);
            LRESULT(0)
        }
        WM_CLOSE => {
            // Alt+F4 and the like: the same way out as Esc. No DefWindowProc,
            // whose DestroyWindow would come on top of `close`'s own.
            post(MSG_CANCEL, 0);
            LRESULT(0)
        }
        WM_NCDESTROY | WM_DESTROY => {
            // The window went without `close` (closed from outside): free
            // its state so `active()` says false. Only this window's own.
            PICK.with(|p| {
                if let Ok(mut slot) = p.try_borrow_mut()
                    && slot.as_ref().is_some_and(|s| s.hwnd == hwnd)
                {
                    *slot = None;
                }
            });
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(h: isize, x: i32) -> (isize, Rect) {
        (h, Rect::new(x, 0, 100, 100))
    }

    #[test]
    fn the_frontmost_window_under_the_pointer_is_the_one_picked() {
        // Windows in z-order, frontmost first: 1 covers the left half of 2.
        let windows = vec![win(1, 0), win(2, 50)];
        let allowed = vec![1, 2];
        let none = |h| h;
        assert_eq!(
            pickable_at(&windows, &allowed, (10, 10), none).map(|w| w.0),
            Some(1)
        );
        assert_eq!(
            pickable_at(&windows, &allowed, (60, 10), none).map(|w| w.0),
            Some(1)
        );
        assert_eq!(
            pickable_at(&windows, &allowed, (120, 10), none).map(|w| w.0),
            Some(2)
        );
        assert_eq!(pickable_at(&windows, &allowed, (500, 10), none), None);
    }

    #[test]
    fn a_window_not_in_the_list_or_already_picked_is_ignored() {
        let windows = vec![win(1, 0), win(2, 50)];
        let none = |h| h;
        // 1 (frontmost) is picked already: the click on it picks nothing,
        // even where 2 shows behind it.
        assert_eq!(pickable_at(&windows, &[2], (60, 10), none), None);
        assert_eq!(
            pickable_at(&windows, &[2], (120, 10), none).map(|w| w.0),
            Some(2)
        );
        assert_eq!(pickable_at(&windows, &[], (10, 10), none), None);
    }

    #[test]
    fn a_dialog_of_a_listed_window_counts_as_that_window() {
        let windows = vec![(9, Rect::new(10, 10, 20, 20)), win(1, 0)];
        let owner = |h| if h == 9 { 1 } else { h };
        let hit = pickable_at(&windows, &[1], (15, 15), owner).expect("owner");
        assert_eq!(hit.0, 1);
        assert_eq!(hit.1, Rect::new(0, 0, 100, 100), "its own frame is lit");
    }

    #[test]
    fn the_message_ids_are_distinct_and_clear_of_the_others() {
        assert_eq!(WM_APP_ARRANGE, WM_APP + 61);
        let codes = [MSG_PICK, MSG_BACK, MSG_CANCEL];
        for (i, a) in codes.iter().enumerate() {
            for b in &codes[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
