use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, HWND_BOTTOM, HWND_TOPMOST, IDC_ARROW, LoadCursorW, RegisterClassExW,
    GWL_EXSTYLE, GetWindowLongPtrW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WNDCLASSEXW, WNDPROC,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};
use windows::core::{Error, Result, w};

use super::app_icon;
use super::geometry::{TALL_WINDOW_H, WINDOW_H, WINDOW_W};
use crate::arrange::Rect;
use crate::notch_monitor::Display;

/// The island's layered popup: a transparent canvas at the top-center of the
/// chosen monitor (the primary by default), `WINDOW_H` tall, or `TALL_WINDOW_H` while the enlarged
/// CLI panel needs it. Content and per-pixel alpha come from
/// `Renderer::draw` via `UpdateLayeredWindow`, which also applies a new size.
///
/// It never activates on a click (`WS_EX_NOACTIVATE`, and `MA_NOACTIVATE`
/// in the window procedure); only the CLI input activates it on purpose.
pub struct IslandWindow {
    hwnd: HWND,
    scale: f32,
    origin: POINT,
    size: SIZE,
    tall: bool,
    /// Above every window (the default); false while "Di belakang jendela"
    /// has put it at the bottom of the window order.
    front: bool,
    /// The island's monitor's width in logical px (the lyric notch's cap).
    screen_w: f32,
    /// `WS_EX_TRANSPARENT` is set: every click falls through to what is below.
    click_through: bool,
}

impl IslandWindow {
    /// Creates the island on `target` (the chosen monitor; the primary when
    /// `None`).
    pub fn create(wnd_proc: WNDPROC, target: Option<&Display>) -> Result<Self> {
        // Also named in `capture::win::KEPT_OUT` (kept out of screenshots).
        let class_name = w!("ValeWindow");
        unsafe {
            let instance = GetModuleHandleW(None)?.into();
            // The island has no taskbar button, but Task Manager and other
            // shell surfaces ask the window's class for an icon.
            let (icon, icon_small) = app_icon::class_icons(instance);
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: wnd_proc,
                hInstance: instance,
                hIcon: icon,
                hIconSm: icon_small,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: class_name,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                let error = Error::from_thread();
                if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                    return Err(error);
                }
            }
            let Placement {
                origin,
                size,
                scale,
                screen_w,
            } = placement(target, false);
            let ex_style: WINDOW_EX_STYLE =
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!("Vale"),
                WS_POPUP,
                origin.x,
                origin.y,
                size.cx,
                size.cy,
                None,
                None,
                Some(instance),
                None,
            )?;
            // A layered window stays invisible until its first UpdateLayeredWindow.
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            Ok(Self {
                hwnd,
                scale,
                origin,
                size,
                tall: false,
                front: true,
                screen_w,
                click_through: false,
            })
        }
    }

    /// Grows the canvas to `TALL_WINDOW_H` or back; the next draw resizes
    /// the window. True when it changed.
    pub fn set_tall(&mut self, tall: bool) -> bool {
        if self.tall == tall {
            return false;
        }
        self.tall = tall;
        self.size.cy = (height(tall) * self.scale).round() as i32;
        true
    }

    /// Makes every pixel of the island pass clicks to the window below
    /// (`WS_EX_TRANSPARENT`), or lets the alpha rule govern again. The style
    /// is written only when this changes.
    pub fn set_click_through(&mut self, on: bool) {
        if self.click_through == on {
            return;
        }
        self.click_through = on;
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
            let ex = if on {
                ex | WS_EX_TRANSPARENT.0
            } else {
                ex & !WS_EX_TRANSPARENT.0
            };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex as isize);
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// Physical pixels per logical pixel.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The island's monitor's width in logical px, as of the last placement.
    pub fn screen_w(&self) -> f32 {
        self.screen_w
    }

    /// Puts the island above every window, or at the bottom of the window
    /// order ("Di belakang jendela"). Never activates it and never moves or
    /// resizes it. True when the order changed.
    pub fn set_front(&mut self, front: bool) -> bool {
        if self.front == front {
            return false;
        }
        self.front = front;
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(order(front)),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        true
    }

    /// The window's top-left corner on the screen, physical pixels.
    pub fn screen_origin(&self) -> (i32, i32) {
        (self.origin.x, self.origin.y)
    }

    /// Re-centers on `target` (the chosen monitor; the primary when `None`) at
    /// its current DPI. Call on `WM_DPICHANGED`, `WM_DISPLAYCHANGE`, a monitor
    /// hot-plug and when the choice changes, then redraw.
    pub fn reposition(&mut self, target: Option<&Display>) {
        let Placement {
            origin,
            size,
            scale,
            screen_w,
        } = placement(target, self.tall);
        self.origin = origin;
        self.size = size;
        self.scale = scale;
        self.screen_w = screen_w;
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(order(self.front)),
                origin.x,
                origin.y,
                size.cx,
                size.cy,
                SWP_NOACTIVATE,
            );
        }
    }

    /// A point in the window's logical pixels, in screen coordinates (physical pixels).
    pub fn to_screen(&self, x: f32, y: f32) -> POINT {
        POINT {
            x: self.origin.x + (x * self.scale).round() as i32,
            y: self.origin.y + (y * self.scale).round() as i32,
        }
    }

    /// Top-left corner in screen coordinates (physical pixels).
    pub(super) fn origin(&self) -> POINT {
        self.origin
    }

    /// Window size in physical pixels.
    pub(super) fn size(&self) -> SIZE {
        self.size
    }
}

/// Where `SetWindowPos` puts the island in the window order: topmost, or the
/// bottom (which also takes away the topmost style).
fn order(front: bool) -> HWND {
    if front { HWND_TOPMOST } else { HWND_BOTTOM }
}

fn height(tall: bool) -> f32 {
    if tall { TALL_WINDOW_H } else { WINDOW_H }
}

/// Where the window goes and what it learned about the monitor.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    origin: POINT,
    size: SIZE,
    scale: f32,
    /// The monitor's width in logical px.
    screen_w: f32,
}

/// Top-center of the monitor whose rectangle is `rect` (physical px), sized for
/// its `dpi`.
fn place_on(rect: Rect, dpi: u32, tall: bool) -> Placement {
    let scale = dpi.max(96) as f32 / 96.0;
    let size = SIZE {
        cx: (WINDOW_W * scale).round() as i32,
        cy: (height(tall) * scale).round() as i32,
    };
    let origin = POINT {
        x: rect.x + (rect.w - size.cx) / 2,
        y: rect.y,
    };
    Placement {
        origin,
        size,
        scale,
        screen_w: rect.w as f32 / scale,
    }
}

/// The primary monitor's rectangle and DPI, straight from Windows: the target
/// when the display list is empty or the caller has none (the demos, the
/// first draw before any enumeration).
///
/// The DPI is read from the monitor rather than `GetDpiForWindow`: asking the
/// monitor avoids moving the window first, which would send `WM_DPICHANGED`
/// back into the caller's window procedure mid-call.
fn primary_target() -> (Rect, u32) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dpi_x, mut dpi_y) = (96, 96);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_err() {
            dpi_x = 96;
        }
        let rc = info.rcMonitor;
        (
            Rect::new(rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top),
            dpi_x,
        )
    }
}

/// The placement on `target` (a chosen display), or on the primary monitor
/// when there is none.
fn placement(target: Option<&Display>, tall: bool) -> Placement {
    match target {
        Some(d) => place_on(d.monitor.rect, d.monitor.dpi, tall),
        None => {
            let (rect, dpi) = primary_target();
            place_on(rect, dpi, tall)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_island_goes_top_centre_of_the_monitor() {
        let p = place_on(Rect::new(0, 0, 1920, 1080), 96, false);
        assert_eq!((p.size.cx, p.size.cy), (620, 330));
        assert_eq!((p.origin.x, p.origin.y), ((1920 - 620) / 2, 0));
        assert_eq!(p.scale, 1.0);
        assert_eq!(p.screen_w, 1920.0);
    }

    #[test]
    fn a_monitor_off_the_origin_is_centred_where_it_is() {
        let p = place_on(Rect::new(-2560, -200, 2560, 1440), 96, false);
        assert_eq!(p.origin.x, -2560 + (2560 - 620) / 2);
        assert_eq!(p.origin.y, -200);
        let right = place_on(Rect::new(1920, 0, 1920, 1080), 96, false);
        assert_eq!(right.origin.x, 1920 + (1920 - 620) / 2);
    }

    #[test]
    fn size_and_screen_width_follow_the_dpi() {
        let p = place_on(Rect::new(0, 0, 3840, 2160), 192, false);
        assert_eq!((p.size.cx, p.size.cy), (1240, 660));
        assert_eq!(p.scale, 2.0);
        assert_eq!(p.screen_w, 1920.0);
        assert_eq!(p.origin.x, (3840 - 1240) / 2);
        let odd = place_on(Rect::new(0, 0, 2400, 1350), 120, false);
        assert_eq!(odd.scale, 1.25);
        assert_eq!(odd.size.cx, (620.0f32 * 1.25).round() as i32);
        assert_eq!(odd.screen_w, 2400.0 / 1.25);
    }

    #[test]
    fn the_tall_canvas_only_changes_the_height() {
        let a = place_on(Rect::new(0, 0, 1920, 1080), 96, false);
        let b = place_on(Rect::new(0, 0, 1920, 1080), 96, true);
        assert_eq!(b.size.cx, a.size.cx);
        assert_eq!(b.size.cy, 460);
        assert_eq!(b.origin, a.origin);
    }
}
