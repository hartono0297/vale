//! The Win32 side of the capture: freezing the screen and listing the
//! windows. The process is PerMonitorV2 (app.manifest), so every number
//! here is a physical pixel.
//!
//! Nothing runs until the Screenshot tile starts a capture; nothing is
//! kept after the overlay closes.

use std::ffi::c_void;
use std::sync::Arc;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmFlush, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC,
    GetDIBits, GetMonitorInfoW, HDC, HGDIOBJ, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetSystemMetrics, GetWindowDisplayAffinity,
    GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_HIDE,
    SW_SHOWNOACTIVATE, SetWindowDisplayAffinity, ShowWindow, WDA_EXCLUDEFROMCAPTURE,
    WINDOW_DISPLAY_AFFINITY, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use super::{Candidate, Rect, Shot};

/// The whole desktop at one moment, in memory only.
pub struct Frozen {
    /// Where the frame's top-left pixel sits in the virtual screen.
    pub origin: (i32, i32),
    /// The pixels: `w * h * 4` bytes, top-down, alpha 255. Behind an `Arc`
    /// so the inline capture can hand the same frame to the annotation
    /// painter (a `&Shot`) and to the export without copying 100+ MB.
    pub shot: Arc<Shot>,
}

impl Frozen {
    pub fn new(w: u32, h: u32, origin: (i32, i32), bgra: Vec<u8>) -> Frozen {
        Frozen {
            origin,
            shot: Arc::new(Shot { w, h, bgra }),
        }
    }

    pub fn w(&self) -> u32 {
        self.shot.w
    }

    pub fn h(&self) -> u32 {
        self.shot.h
    }

    pub fn bgra(&self) -> &[u8] {
        &self.shot.bgra
    }

    pub fn rect(&self) -> Rect {
        Rect::new(
            self.origin.0,
            self.origin.1,
            self.shot.w as i32,
            self.shot.h as i32,
        )
    }

    pub fn stride(&self) -> usize {
        self.shot.w as usize * 4
    }
}

/// The virtual screen: every monitor together.
pub fn virtual_screen() -> Rect {
    unsafe {
        Rect::new(
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

/// The monitor holding (or nearest to) `pt`, and its DPI.
pub fn monitor_at(pt: (i32, i32)) -> (Rect, u32) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: pt.0, y: pt.1 }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let rect = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let r = info.rcMonitor;
            Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
        } else {
            virtual_screen()
        };
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        (rect, dx.max(96))
    }
}

/// Every monitor: its `HMONITOR` (as a number) and its rectangle in physical
/// pixels, virtual-screen coordinates.
pub fn monitors() -> Vec<(isize, Rect)> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, rect: *mut RECT, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<(isize, Rect)>) };
        if let Some(r) = unsafe { rect.as_ref() } {
            list.push((
                m.0 as isize,
                Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top),
            ));
        }
        true.into()
    }
    let mut list: Vec<(isize, Rect)> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM((&raw mut list) as isize));
    }
    list
}

/// The text of a window's title bar (empty when it has none or is gone).
pub fn window_title(hwnd: isize) -> String {
    let mut text = [0u16; 256];
    let n = unsafe { GetWindowTextW(HWND(hwnd as *mut c_void), &mut text) }.max(0) as usize;
    String::from_utf16_lossy(&text[..n.min(text.len())])
}

/// The window classes kept out of the frozen frame: the island
/// (`ui::window`), which also wears the halo margin and moves behind other
/// windows for "Di belakang jendela"; neither is a window of its own.
/// Vale's other windows (Settings, the calendar and notes editor, a
/// screenshot editor) stay in the picture like any app's, so they can be
/// captured; they are only left out as snap targets (`top_windows`).
const KEPT_OUT: &[&str] = &["ValeWindow", crate::identify_overlay::CLASS_NAME];

fn is_kept_out(class: &str) -> bool {
    KEPT_OUT.contains(&class)
}

/// The class name of `hwnd` (cut at 32 characters, longer than any of ours).
unsafe fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 32];
    let n = unsafe { GetClassNameW(hwnd, &mut name) }.max(0) as usize;
    String::from_utf16_lossy(&name[..n.min(name.len())])
}

/// The island's windows of this process that are on screen (`KEPT_OUT`).
fn own_windows() -> Vec<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == unsafe { GetCurrentProcessId() }
            && unsafe { IsWindowVisible(hwnd) }.as_bool()
            && is_kept_out(&unsafe { class_of(hwnd) })
        {
            list.push(hwnd);
        }
        true.into()
    }
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM((&raw mut list) as isize));
    }
    list
}

/// Keeps the island (`KEPT_OUT`) out of a screen grab for as long
/// as it lives: `WDA_EXCLUDEFROMCAPTURE` on each, restored to what it was
/// when this drops. A window that refuses is hidden instead and shown again.
struct OutOfTheShot {
    excluded: Vec<(HWND, u32)>,
    hidden: Vec<HWND>,
}

impl OutOfTheShot {
    fn new() -> OutOfTheShot {
        let mut guard = OutOfTheShot {
            excluded: Vec::new(),
            hidden: Vec::new(),
        };
        for hwnd in own_windows() {
            let mut before = 0u32;
            unsafe {
                let _ = GetWindowDisplayAffinity(hwnd, &mut before);
                if SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_ok() {
                    guard.excluded.push((hwnd, before));
                } else {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    guard.hidden.push(hwnd);
                }
            }
        }
        if !guard.excluded.is_empty() || !guard.hidden.is_empty() {
            // The compositor takes the change on its next frame.
            unsafe {
                let _ = DwmFlush();
            }
            if !guard.hidden.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        }
        guard
    }
}

impl Drop for OutOfTheShot {
    fn drop(&mut self) {
        unsafe {
            for (hwnd, before) in &self.excluded {
                let _ = SetWindowDisplayAffinity(*hwnd, WINDOW_DISPLAY_AFFINITY(*before));
            }
            for hwnd in &self.hidden {
                let _ = ShowWindow(*hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }
}

/// Copies the whole virtual screen into memory (GDI `BitBlt` with
/// `CAPTUREBLT`, so layered windows come along), with the island left out
/// (`KEPT_OUT`). `None` when GDI could not make the bitmap.
pub fn grab_virtual_screen() -> Option<Frozen> {
    let area = virtual_screen();
    if area.is_empty() {
        return None;
    }
    let (w, h) = (area.w as usize, area.h as usize);
    let mut bgra = vec![0u8; w * h * 4];
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, area.w, area.h);
        let mut ok = !mem.is_invalid() && !bitmap.is_invalid();
        if ok {
            let old = SelectObject(mem, HGDIOBJ(bitmap.0));
            {
                let _hidden = OutOfTheShot::new();
                ok = BitBlt(
                    mem,
                    0,
                    0,
                    area.w,
                    area.h,
                    Some(screen),
                    area.x,
                    area.y,
                    SRCCOPY | CAPTUREBLT,
                )
                .is_ok();
            }
            // The bitmap must be out of the DC for GetDIBits.
            SelectObject(mem, old);
            if ok {
                let mut info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: area.w,
                        // Negative: top-down rows.
                        biHeight: -area.h,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let lines = GetDIBits(
                    mem,
                    bitmap,
                    0,
                    area.h as u32,
                    Some(bgra.as_mut_ptr().cast::<c_void>()),
                    &mut info,
                    DIB_RGB_COLORS,
                );
                ok = lines == area.h;
            }
        }
        if !bitmap.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        if !mem.is_invalid() {
            let _ = DeleteDC(mem);
        }
        ReleaseDC(None, screen);
        if !ok {
            return None;
        }
    }
    // The screen has no alpha; make every pixel opaque.
    for px in bgra.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    Some(Frozen::new(
        area.w as u32,
        area.h as u32,
        (area.x, area.y),
        bgra,
    ))
}

/// A top-level window worth snapping to.
#[derive(Clone, Copy, Debug)]
pub struct TopWindow {
    /// Kept as a number so it can go to a worker thread.
    pub hwnd: isize,
    pub rect: Rect,
    /// 0 is the frontmost.
    pub z: u32,
}

impl TopWindow {
    pub fn candidate(&self) -> Candidate {
        Candidate {
            rect: self.rect,
            z: self.z,
            depth: 0,
        }
    }
}

/// The visible top-level windows in z-order, frontmost first. Skipped:
/// invisible, minimised, cloaked (another virtual desktop, a suspended
/// app), zero-size, click-through (`WS_EX_TRANSPARENT`), the desktop
/// itself, and this process's own. Each rect is the visible frame
/// (`DWMWA_EXTENDED_FRAME_BOUNDS`, without the invisible resize border),
/// kept inside the virtual screen.
pub fn top_windows(bounds: Rect) -> Vec<TopWindow> {
    struct Walk {
        bounds: Rect,
        pid: u32,
        out: Vec<TopWindow>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let walk = unsafe { &mut *(lparam.0 as *mut Walk) };
        if let Some(rect) = unsafe { snappable(hwnd, walk.pid, walk.bounds) } {
            let z = walk.out.len() as u32;
            walk.out.push(TopWindow {
                hwnd: hwnd.0 as isize,
                rect,
                z,
            });
        }
        true.into()
    }
    let mut walk = Walk {
        bounds,
        pid: unsafe { GetCurrentProcessId() },
        out: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM((&raw mut walk) as isize));
    }
    walk.out
}

/// True when `hwnd` belongs to the process `pid`. Vale's own windows are
/// never snap targets, and never recording targets either: while a screen
/// recording runs they are excluded from capture, so a recording of one
/// would be empty.
fn owned_by(hwnd: HWND, pid: u32) -> bool {
    let mut owner = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
    owner == pid
}

/// The window's frame inside `bounds`, or `None` when it is not something
/// to snap to.
unsafe fn snappable(hwnd: HWND, own_pid: u32, bounds: Rect) -> Option<Rect> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }
        if owned_by(hwnd, own_pid) {
            return None;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return None;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 != 0 {
            return None;
        }
        if is_desktop_class(&class_of(hwnd)) {
            return None;
        }
        let mut frame = RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame).cast(),
            size_of::<RECT>() as u32,
        )
        .ok()?;
        let rect = Rect::new(
            frame.left,
            frame.top,
            frame.right - frame.left,
            frame.bottom - frame.top,
        )
        .intersect(&bounds);
        (!rect.is_empty()).then_some(rect)
    }
}

/// The desktop's own windows cover the whole screen and would swallow every
/// click on the wallpaper.
fn is_desktop_class(class: &str) -> bool {
    matches!(class, "Progman" | "WorkerW")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_is_not_something_to_snap_to() {
        assert!(is_desktop_class("Progman"));
        assert!(is_desktop_class("WorkerW"));
        assert!(!is_desktop_class("Shell_TrayWnd"));
        assert!(!is_desktop_class("CabinetWClass"));
    }

    /// A message-only window: never on screen.
    fn message_window() -> HWND {
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
        };
        use windows::core::w;
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Catatan rapat"),
                WINDOW_STYLE(0),
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

    #[test]
    fn a_window_of_this_process_is_never_a_target() {
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
        let hwnd = message_window();
        let own = unsafe { GetCurrentProcessId() };
        assert!(owned_by(hwnd, own));
        assert!(!owned_by(hwnd, own.wrapping_add(4)));
        // So the window list (what Jendela picks from) never holds it,
        // whatever else is true of the window.
        let bounds = Rect::new(-10_000, -10_000, 20_000, 20_000);
        assert_eq!(unsafe { snappable(hwnd, own, bounds) }, None);
        let listed = top_windows(bounds);
        assert!(listed.iter().all(|w| {
            !owned_by(HWND(w.hwnd as *mut c_void), own) && w.hwnd != hwnd.0 as isize
        }));
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    #[test]
    fn a_window_title_is_read_as_text() {
        use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
        let hwnd = message_window();
        assert_eq!(window_title(hwnd.0 as isize), "Catatan rapat");
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        assert_eq!(window_title(hwnd.0 as isize), "", "a window that is gone");
    }

    #[test]
    fn every_monitor_is_listed_inside_the_virtual_screen() {
        let screen = virtual_screen();
        for (handle, rect) in monitors() {
            assert_ne!(handle, 0);
            assert!(!rect.is_empty());
            assert_eq!(rect.intersect(&screen), rect, "{rect:?} in {screen:?}");
        }
    }

    #[test]
    fn only_the_island_is_kept_out_of_the_shot() {
        assert!(is_kept_out("ValeWindow"));
        // Vale's own Settings and editors can be captured.
        assert!(!is_kept_out("ValeSettings"));
        assert!(!is_kept_out("ValeEditor"));
        assert!(!is_kept_out("ValeEditorPopover"));
        assert!(!is_kept_out("ValeShotEditor"));
        assert!(!is_kept_out("Notepad"));
    }

    #[test]
    fn a_frozen_frame_knows_its_place() {
        let f = Frozen::new(4, 2, (-10, 5), vec![0; 32]);
        assert_eq!(f.rect(), Rect::new(-10, 5, 4, 2));
        assert_eq!(f.stride(), 16);
        // The painter borrows the same pixels, no copy.
        assert!(std::ptr::eq(
            f.shot.bgra.as_ptr(),
            f.shot.clone().bgra.as_ptr()
        ));
        assert_eq!((f.shot.w, f.shot.h), (4, 2));
    }
}
