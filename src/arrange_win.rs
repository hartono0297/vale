//! Susun's Win32 side: the monitors, the open windows, the icons and the
//! mover. The process is PerMonitorV2 (app.manifest), so every number here
//! is a physical pixel.
//!
//! Nothing runs until the Susun page is used. Moving windows and reading
//! icons happen on short-lived worker threads, never on the UI thread: a hung
//! app would block a `SetWindowPos` that sends it a message. Nothing here
//! touches Windows' Snap Layouts, the registry, or launches an app.

use std::ffi::c_void;
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetWindow, GetWindowLongW,
    GetWindowPlacement, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsHungAppWindow,
    IsIconic, IsWindow, IsWindowVisible, IsZoomed, SET_WINDOW_POS_FLAGS, SW_RESTORE,
    SW_SHOWMINIMIZED, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SW_SHOWNORMAL, SWP_NOACTIVATE,
    SWP_NOOWNERZORDER, SWP_NOZORDER, SetWindowPlacement, SetWindowPos, ShowWindow, WINDOWPLACEMENT,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

use crate::arrange::{ArrangeEvent, Monitor, OpenWindow, Placement, Rect};
use crate::model::Event;

/// Window classes that are never listed: the desktop and the taskbars.
const SKIPPED_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

// ----------------------------------------------------------------- monitors

/// The monitors, numbered in a stable order: left to right, then top to
/// bottom. `work` excludes the taskbar.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        let ok = unsafe { GetMonitorInfoW(m, (&raw mut info).cast::<MONITORINFO>()) }.as_bool();
        if ok {
            let rect = |r: RECT| Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top);
            let n = info
                .szDevice
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(info.szDevice.len());
            let (mut dx, mut dy) = (96u32, 96u32);
            if unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) }.is_err() {
                dx = 96;
            }
            list.push(Monitor {
                device: String::from_utf16_lossy(&info.szDevice[..n]),
                rect: rect(info.monitorInfo.rcMonitor),
                work: rect(info.monitorInfo.rcWork),
                dpi: dx.max(96),
            });
        }
        true.into()
    }
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM((&raw mut list) as isize));
    }
    list.sort_by_key(|m| (m.rect.x, m.rect.y));
    list
}

/// Which of `monitors` holds `point` (the notch's centre); the first when
/// none does.
pub fn monitor_index_at(monitors: &[Monitor], point: (i32, i32)) -> usize {
    monitors
        .iter()
        .position(|m| m.rect.contains(point))
        .unwrap_or(0)
}

// ------------------------------------------------------------------ windows

unsafe fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut name) }.max(0) as usize;
    String::from_utf16_lossy(&name[..n.min(name.len())])
}

unsafe fn title_of(hwnd: HWND) -> String {
    let mut text = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut text) }.max(0) as usize;
    String::from_utf16_lossy(&text[..n.min(text.len())])
}

/// The full path of the exe of process `pid`; empty when it cannot be read.
fn exe_of(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 1040];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if ok {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        }
    }
}

/// Whether a window with this class and style bits is what Alt+Tab would
/// list: not a tool window, and unowned or flagged as an app window.
pub fn is_alt_tab_style(class: &str, ex_style: u32, owned: bool) -> bool {
    !SKIPPED_CLASSES.contains(&class)
        && ex_style & WS_EX_TOOLWINDOW.0 == 0
        && (!owned || ex_style & WS_EX_APPWINDOW.0 != 0)
}

/// The windows Alt+Tab would list, from every monitor, in z-order (the
/// frontmost first): visible, not cloaked, not a tool window, unowned (or an
/// app window), with a title, not the desktop or a taskbar, not this
/// process's. Minimized windows are listed.
pub fn open_windows() -> Vec<OpenWindow> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<OpenWindow>) };
        if let Some(w) = unsafe { describe(hwnd) } {
            out.push(w);
        }
        true.into()
    }
    let mut out: Vec<OpenWindow> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM((&raw mut out) as isize));
    }
    out
}

unsafe fn describe(hwnd: HWND) -> Option<OpenWindow> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
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
        let class = class_of(hwnd);
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let owned = GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid());
        if !is_alt_tab_style(&class, ex, owned) {
            return None;
        }
        let title = title_of(hwnd);
        if title.trim().is_empty() {
            return None;
        }
        let exe = exe_of(pid);
        Some(OpenWindow {
            hwnd: hwnd.0 as isize,
            title,
            class,
            icon_key: exe.to_lowercase(),
            exe,
            minimized: IsIconic(hwnd).as_bool(),
        })
    }
}

// -------------------------------------------------------------------- icons

/// Reads icons off the UI thread: one short-lived worker per batch, started
/// by `request`; each answer is an `ArrangeEvent::Icon`. Nothing waits.
pub fn load_icons(exes: Vec<String>, tx: Sender<Event>, wake: impl Fn() + Send + 'static) {
    if exes.is_empty() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("arrange-icons".into())
        .spawn(move || {
            // The shell's image factory wants an apartment.
            let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
            for exe in exes {
                let thumb = crate::search_details::shell_icon(std::path::Path::new(&exe));
                let key = exe.to_lowercase();
                if tx
                    .send(Event::Arrange(ArrangeEvent::Icon { key, thumb }))
                    .is_err()
                {
                    break;
                }
                wake();
            }
            if com {
                unsafe { CoUninitialize() };
            }
        });
}

// -------------------------------------------------------------------- mover

/// Whether this process is elevated (so it can move an elevated window).
fn own_elevation() -> bool {
    token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

/// Whether the process behind `process` runs elevated; `None` when its token
/// cannot be opened.
fn token_elevated(process: HANDLE) -> Option<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut elevation).cast::<c_void>()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok.then_some(elevation.TokenIsElevated != 0)
    }
}

/// Whether the window's process is elevated; `None` when that cannot be told.
fn window_elevated(hwnd: HWND) -> Option<bool> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let elevated = token_elevated(process);
        let _ = CloseHandle(process);
        elevated
    }
}

/// How much bigger the window's rectangle is than what the eye sees, left,
/// top, right, bottom (the invisible resize borders of Windows 10 and 11).
fn invisible_borders(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let mut outer = RECT::default();
        GetWindowRect(hwnd, &mut outer).ok()?;
        let frame = visible_frame(hwnd)?;
        Some((
            frame.x - outer.left,
            frame.y - outer.top,
            outer.right - frame.right(),
            outer.bottom - frame.bottom(),
        ))
    }
}

/// The frame the eye sees (`DWMWA_EXTENDED_FRAME_BOUNDS`).
pub fn visible_frame(hwnd: HWND) -> Option<Rect> {
    unsafe {
        let mut f = RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut f).cast(),
            size_of::<RECT>() as u32,
        )
        .ok()?;
        Some(Rect::new(f.left, f.top, f.right - f.left, f.bottom - f.top))
    }
}

/// The window's place, for undo. A window in its normal state also keeps the
/// rectangle it really has: when it is snapped (Aero Snap, Snap Layouts),
/// `rcNormalPosition` is where it was before the snap, not where it is.
fn placement_of(hwnd: HWND) -> Option<Placement> {
    unsafe {
        let mut p = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        GetWindowPlacement(hwnd, &mut p).ok()?;
        let normal_state = !(IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool());
        let mut outer = RECT::default();
        let rect = (normal_state && GetWindowRect(hwnd, &mut outer).is_ok()).then_some((
            outer.left,
            outer.top,
            outer.right,
            outer.bottom,
        ));
        Some(Placement {
            rect,
            hwnd: hwnd.0 as isize,
            flags: p.flags.0,
            show: p.showCmd,
            min: (p.ptMinPosition.x, p.ptMinPosition.y),
            max: (p.ptMaxPosition.x, p.ptMaxPosition.y),
            normal: (
                p.rcNormalPosition.left,
                p.rcNormalPosition.top,
                p.rcNormalPosition.right,
                p.rcNormalPosition.bottom,
            ),
        })
    }
}

/// Puts the window's visible frame at `target`: the target grown by the
/// invisible borders. A move to a monitor of another DPI changes the borders
/// (and the app may resize itself on `WM_DPICHANGED`), so the frame is read
/// again and corrected once if it is more than 1 px off. False when
/// `SetWindowPos` fails.
fn place(hwnd: HWND, target: Rect) -> bool {
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    let go = |borders: (i32, i32, i32, i32)| unsafe {
        SetWindowPos(
            hwnd,
            None,
            target.x - borders.0,
            target.y - borders.1,
            target.w + borders.0 + borders.2,
            target.h + borders.1 + borders.3,
            flags,
        )
        .is_ok()
    };
    if !go(invisible_borders(hwnd).unwrap_or((0, 0, 0, 0))) {
        return false;
    }
    if let (Some(frame), Some(borders)) = (visible_frame(hwnd), invisible_borders(hwnd))
        && !frame_close(frame, target)
    {
        go(borders);
    }
    true
}

/// Whether a frame is within 1 px of the target on every side.
pub fn frame_close(frame: Rect, target: Rect) -> bool {
    (frame.x - target.x).abs() <= 1
        && (frame.y - target.y).abs() <= 1
        && (frame.right() - target.right()).abs() <= 1
        && (frame.bottom() - target.bottom()).abs() <= 1
}

/// Brings a minimized or maximized window back to its normal state, so its
/// frame and borders are the plain ones before it is placed. A minimized
/// window that was maximized before (`WPF_RESTORETOMAXIMIZED`) comes back
/// maximized from the first `SW_RESTORE`, so a second one takes it to normal.
/// False when the window is gone.
unsafe fn restore_to_normal(hwnd: HWND) -> bool {
    unsafe {
        for _ in 0..2 {
            if !(IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool()) {
                break;
            }
            if !ShowWindow(hwnd, SW_RESTORE).as_bool() && !IsWindow(Some(hwnd)).as_bool() {
                return false;
            }
        }
        true
    }
}

/// Moves every `(hwnd, rect)`; the synchronous core of `spawn_move`. A window
/// that is gone, hung, or elevated while Vale is not is skipped, as is one
/// that refuses the move. A minimized or maximized window is restored first.
/// Returns the windows moved, those skipped and the places to undo to.
pub fn move_all(jobs: &[(isize, Rect)]) -> (usize, usize, Vec<Placement>) {
    let ours_elevated = own_elevation();
    let (mut moved, mut skipped) = (0, 0);
    let mut undo = Vec::new();
    for &(raw, target) in jobs {
        let hwnd = HWND(raw as *mut c_void);
        unsafe {
            if !IsWindow(Some(hwnd)).as_bool() || IsHungAppWindow(hwnd).as_bool() {
                skipped += 1;
                continue;
            }
            if !ours_elevated && window_elevated(hwnd) == Some(true) {
                skipped += 1;
                continue;
            }
            let saved = placement_of(hwnd);
            if !restore_to_normal(hwnd) {
                skipped += 1;
                continue;
            }
            if place(hwnd, target) {
                moved += 1;
                undo.extend(saved);
            } else {
                skipped += 1;
                // The window may have been restored on the way: put it back.
                if let Some(p) = saved {
                    restore(p);
                }
            }
        }
    }
    (moved, skipped, undo)
}

/// The show command undo uses: the saved state without activating, so
/// undoing several windows never raises them one after another (which would
/// reverse their z-order and move the focus). Maximized has no quiet form.
pub fn quiet_show(show: u32) -> u32 {
    let cmd = show as i32;
    if cmd == SW_SHOWNORMAL.0 {
        SW_SHOWNOACTIVATE.0 as u32
    } else if cmd == SW_SHOWMINIMIZED.0 {
        SW_SHOWMINNOACTIVE.0 as u32
    } else {
        show
    }
}

/// Whether two window rectangles (left, top, right, bottom) are within 1 px
/// on every side.
fn rect_close(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    (a.0 - b.0).abs() <= 1
        && (a.1 - b.1).abs() <= 1
        && (a.2 - b.2).abs() <= 1
        && (a.3 - b.3).abs() <= 1
}

/// `SetWindowPlacement` with a saved place: the rectangle and the
/// maximized or minimized state come back, without activating. A window that
/// was in its normal state is then put at the rectangle it really had, when
/// that is not where the placement left it (a snapped window). False when the
/// window is gone.
pub fn restore(p: Placement) -> bool {
    let hwnd = HWND(p.hwnd as *mut c_void);
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        let placement = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            flags: windows::Win32::UI::WindowsAndMessaging::WINDOWPLACEMENT_FLAGS(p.flags),
            showCmd: quiet_show(p.show),
            ptMinPosition: windows::Win32::Foundation::POINT {
                x: p.min.0,
                y: p.min.1,
            },
            ptMaxPosition: windows::Win32::Foundation::POINT {
                x: p.max.0,
                y: p.max.1,
            },
            rcNormalPosition: RECT {
                left: p.normal.0,
                top: p.normal.1,
                right: p.normal.2,
                bottom: p.normal.3,
            },
        };
        if SetWindowPlacement(hwnd, &placement).is_err() {
            return false;
        }
        if let Some(want) = p.rect
            && !(IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool())
        {
            // A move across monitors of another DPI may make the app resize
            // itself, so the rectangle is read again and corrected once.
            for _ in 0..2 {
                let mut now = RECT::default();
                if GetWindowRect(hwnd, &mut now).is_err()
                    || rect_close((now.left, now.top, now.right, now.bottom), want)
                {
                    break;
                }
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    want.0,
                    want.1,
                    want.2 - want.0,
                    want.3 - want.1,
                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                );
            }
        }
        true
    }
}

/// Moves the windows on a short-lived worker (a hung app must never block
/// the UI thread) and posts `ArrangeEvent::Done`.
pub fn spawn_move(
    run: u64,
    jobs: Vec<(isize, Rect)>,
    tx: Sender<Event>,
    wake: impl Fn() + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("arrange-move".into())
        .spawn({
            let tx = tx.clone();
            move || {
                let (moved, skipped, undo) = move_all(&jobs);
                let _ = tx.send(Event::Arrange(ArrangeEvent::Done {
                    run,
                    moved,
                    skipped,
                    undo,
                }));
                wake();
            }
        });
    if spawned.is_err() {
        let _ = tx.send(Event::Arrange(ArrangeEvent::Done {
            run,
            moved: 0,
            skipped: 0,
            undo: Vec::new(),
        }));
    }
}

/// "Batalkan": every saved place comes back, on a worker as well.
pub fn spawn_undo(undo: Vec<Placement>) {
    let _ = std::thread::Builder::new()
        .name("arrange-undo".into())
        .spawn(move || {
            for p in undo {
                restore(p);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HINSTANCE;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SW_SHOWNORMAL, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::w;

    #[test]
    fn alt_tab_style_skips_tool_windows_owned_windows_and_the_shell() {
        let none = 0u32;
        assert!(is_alt_tab_style("Notepad", none, false));
        assert!(!is_alt_tab_style("Notepad", WS_EX_TOOLWINDOW.0, false));
        assert!(!is_alt_tab_style("Notepad", none, true), "owned");
        assert!(
            is_alt_tab_style("Notepad", WS_EX_APPWINDOW.0, true),
            "owned but an app window"
        );
        for class in SKIPPED_CLASSES {
            assert!(!is_alt_tab_style(class, none, false), "{class}");
        }
    }

    #[test]
    fn a_frame_within_one_pixel_counts_as_in_place() {
        let t = Rect::new(100, 100, 800, 600);
        assert!(frame_close(t, t));
        assert!(frame_close(Rect::new(101, 99, 799, 601), t));
        assert!(!frame_close(Rect::new(103, 100, 800, 600), t));
        assert!(!frame_close(Rect::new(100, 100, 800, 603), t));
    }

    #[test]
    fn a_point_picks_its_monitor_and_falls_to_the_first() {
        let m = |x: i32| Monitor {
            device: String::new(),
            rect: Rect::new(x, 0, 1000, 800),
            work: Rect::new(x, 0, 1000, 760),
            dpi: 96,
        };
        let list = vec![m(0), m(1000)];
        assert_eq!(monitor_index_at(&list, (1500, 10)), 1);
        assert_eq!(monitor_index_at(&list, (10, 10)), 0);
        assert_eq!(monitor_index_at(&list, (5000, 5000)), 0);
        assert_eq!(monitor_index_at(&[], (0, 0)), 0);
    }

    /// A hidden window of this test's own (never shown: no `WS_VISIBLE`, and
    /// nothing below shows it), at `x, y, w, h`.
    unsafe fn hidden_window(x: i32, y: i32, w: i32, h: i32) -> HWND {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None).expect("module").into();
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Vale arrange hidden test window"),
                WS_OVERLAPPEDWINDOW,
                x,
                y,
                w,
                h,
                None,
                None,
                Some(instance),
                None,
            )
            .expect("a window")
        }
    }

    fn outer(hwnd: HWND) -> (i32, i32, i32, i32) {
        let mut r = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut r) }.expect("rect");
        (r.left, r.top, r.right, r.bottom)
    }

    #[test]
    fn moving_nothing_moves_nothing() {
        let (moved, skipped, undo) = move_all(&[]);
        assert_eq!((moved, skipped, undo.len()), (0, 0, 0));
        // A handle that is no window is skipped, never an error. The handle
        // is one of this test's own windows, destroyed: certainly no live
        // window of anyone else's.
        let stale = unsafe {
            let hwnd = hidden_window(0, 0, 100, 100);
            DestroyWindow(hwnd).expect("destroyed");
            hwnd
        };
        assert!(!unsafe { IsWindow(Some(stale)) }.as_bool());
        let (moved, skipped, undo) = move_all(&[(stale.0 as isize, Rect::new(0, 0, 10, 10))]);
        assert_eq!((moved, skipped, undo.len()), (0, 1, 0));
    }

    #[test]
    fn undo_never_activates_a_window() {
        let q = |c: windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD| {
            quiet_show(c.0 as u32) as i32
        };
        assert_eq!(q(SW_SHOWNORMAL), SW_SHOWNOACTIVATE.0);
        assert_eq!(q(SW_SHOWMINIMIZED), SW_SHOWMINNOACTIVE.0);
        assert_eq!(q(SW_SHOWNOACTIVATE), SW_SHOWNOACTIVATE.0);
        assert_eq!(
            q(windows::Win32::UI::WindowsAndMessaging::SW_SHOWMAXIMIZED),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWMAXIMIZED.0
        );
    }

    /// A normal window keeps the rectangle it really has, and undo puts it
    /// back there even when the placement's restore rectangle is elsewhere
    /// (a snapped window: `rcNormalPosition` is its place before the snap).
    /// Only a hidden window of this test's own is used.
    #[test]
    fn undo_puts_a_snapped_window_where_it_really_was() {
        unsafe {
            let hwnd = hidden_window(210, 230, 400, 300);
            let mut saved = placement_of(hwnd).expect("placement");
            assert_eq!(saved.rect, Some(outer(hwnd)), "the real rectangle is kept");
            // Kept hidden through the undo: never a window on screen.
            saved.show = windows::Win32::UI::WindowsAndMessaging::SW_HIDE.0 as u32;

            // Snapped: the restore rectangle is not where the window is.
            let mut snapped = saved;
            snapped.normal = (20, 30, 520, 430);
            snapped.rect = Some((250, 260, 750, 660));
            assert!(restore(snapped));
            assert!(
                rect_close(outer(hwnd), (250, 260, 750, 660)),
                "{:?}",
                outer(hwnd)
            );
            assert!(!IsWindowVisible(hwnd).as_bool(), "still hidden");

            // Not snapped: the plain placement already puts it in place.
            assert!(restore(saved));
            assert!(
                rect_close(outer(hwnd), saved.rect.unwrap()),
                "{:?}",
                outer(hwnd)
            );

            let _ = DestroyWindow(hwnd);
        }
    }

    /// Creates its own plain top-level window, so no other window is ever
    /// touched. The mover runs on this thread (the window's owner pumps
    /// nothing here, and a cross-thread `SetWindowPos` would wait on it).
    #[test]
    #[ignore = "creates a real window on screen"]
    fn live_move_own_test_window() {
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None).expect("module").into();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Vale arrange test window"),
                WS_OVERLAPPEDWINDOW,
                200,
                200,
                500,
                400,
                None,
                None,
                Some(instance),
                None,
            )
            .expect("a window");
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let raw = hwnd.0 as isize;
            let before = placement_of(hwnd).expect("placement");

            let target = Rect::new(300, 250, 640, 480);
            let (moved, skipped, undo) = move_all(&[(raw, target)]);
            assert_eq!((moved, skipped), (1, 0));
            let frame = visible_frame(hwnd).expect("frame");
            assert!(frame_close(frame, target), "{frame:?} vs {target:?}");

            // Maximized: arranging again restores it first.
            let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_MAXIMIZE);
            assert!(IsZoomed(hwnd).as_bool());
            let target2 = Rect::new(120, 140, 700, 500);
            let (moved, skipped, undo2) = move_all(&[(raw, target2)]);
            assert_eq!((moved, skipped), (1, 0));
            assert!(!IsZoomed(hwnd).as_bool(), "restored first");
            let frame = visible_frame(hwnd).expect("frame");
            assert!(frame_close(frame, target2), "{frame:?} vs {target2:?}");

            // Undo of the second move brings the maximized state back, undo
            // of the first the original rectangle.
            for p in undo2 {
                assert!(restore(p));
            }
            assert!(IsZoomed(hwnd).as_bool(), "maximized again");
            for p in undo {
                assert!(restore(p));
            }
            let after = placement_of(hwnd).expect("placement");
            assert_eq!(after.normal, before.normal);

            // Minimized from maximized: the first SW_RESTORE would bring it
            // back maximized; it must end up normal at the target.
            let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_MAXIMIZE);
            let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE);
            assert!(IsIconic(hwnd).as_bool());
            let target3 = Rect::new(160, 180, 600, 420);
            let (moved, skipped, undo3) = move_all(&[(raw, target3)]);
            assert_eq!((moved, skipped), (1, 0));
            assert!(
                !IsIconic(hwnd).as_bool() && !IsZoomed(hwnd).as_bool(),
                "normal"
            );
            let frame = visible_frame(hwnd).expect("frame");
            assert!(frame_close(frame, target3), "{frame:?} vs {target3:?}");
            for p in undo3 {
                assert!(restore(p));
            }
            assert!(IsIconic(hwnd).as_bool(), "minimized again");

            let _ = DestroyWindow(hwnd);
        }
    }
}
