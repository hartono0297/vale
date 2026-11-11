//! Keeps Vale's own windows out of a screen recording: every top-level
//! window of this process is excluded from capture while an [`Excluder`]
//! lives, windows shown later included. Nothing here exists, and no hook
//! runs, outside a recording.

// Wired into the app with the screen recording's start (track A).
#![cfg_attr(not(test), allow(dead_code))]

use std::cell::RefCell;
use std::ffi::c_void;
use std::marker::PhantomData;

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_OBJECT_SHOW, EnumWindows, GA_ROOT, GetAncestor, GetWindowDisplayAffinity,
    GetWindowThreadProcessId, IsWindow, WDA_NONE, WINEVENT_OUTOFCONTEXT,
};
use windows::core::BOOL;

use super::capture::exclude_from_capture;

/// The window itself, not one of its parts.
const OBJID_WINDOW: i32 = 0;
const CHILDID_SELF: i32 = 0;

/// Whether a "shown" event is one of Vale's own top-level windows: this
/// process, top level, the window itself (`OBJID_WINDOW`, `CHILDID_SELF`)
/// and not one of its parts (a scroll bar, the caret, the cursor).
pub fn counts(
    window_pid: u32,
    own_pid: u32,
    top_level: bool,
    id_object: i32,
    id_child: i32,
) -> bool {
    window_pid == own_pid && top_level && id_object == OBJID_WINDOW && id_child == CHILDID_SELF
}

/// Vale's windows stay out of every capture while this lives: each
/// top-level window of this process gets `WDA_EXCLUDEFROMCAPTURE`, and so
/// does each one shown later. Dropping it gives every window it changed its
/// previous affinity back.
///
/// It belongs to the thread that started it (the island's, where all of
/// Vale's windows live): the hook reports to that thread's message loop.
/// One at a time per thread.
pub struct Excluder {
    hook: HWINEVENTHOOK,
    /// The hook calls back on the starting thread only.
    _own_thread: PhantomData<*const ()>,
}

impl Excluder {
    pub fn start() -> Excluder {
        unsafe extern "system" fn each(hwnd: HWND, _: LPARAM) -> BOOL {
            if window_pid(hwnd) == std::process::id() {
                exclude(hwnd);
            }
            true.into()
        }
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(0));
        }
        // No WINEVENT_SKIPOWNTHREAD / SKIPOWNPROCESS: the windows to catch
        // are this thread's own.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_SHOW,
                EVENT_OBJECT_SHOW,
                None,
                Some(on_show),
                std::process::id(),
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        Excluder {
            hook,
            _own_thread: PhantomData,
        }
    }
}

impl Drop for Excluder {
    fn drop(&mut self) {
        if !self.hook.is_invalid() {
            unsafe {
                let _ = UnhookWinEvent(self.hook);
            }
        }
        for hwnd in CHANGED.take() {
            let hwnd = HWND(hwnd as *mut c_void);
            if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                exclude_from_capture(hwnd, false);
            }
        }
    }
}

thread_local! {
    /// The windows this thread's excluder took out of the capture. Each had
    /// no affinity before (`WDA_NONE`), which is what the drop restores.
    static CHANGED: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
}

fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Takes `hwnd` out of the capture and remembers it, unless it already has
/// an affinity of its own: that one is kept and never touched.
fn exclude(hwnd: HWND) {
    let mut before = 0u32;
    if unsafe { GetWindowDisplayAffinity(hwnd, &mut before) }.is_err() || before != WDA_NONE.0 {
        return;
    }
    if exclude_from_capture(hwnd, true) {
        CHANGED.with_borrow_mut(|changed| changed.push(hwnd.0 as isize));
    }
}

unsafe extern "system" fn on_show(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if hwnd.is_invalid() {
        return;
    }
    let top_level = unsafe { GetAncestor(hwnd, GA_ROOT) } == hwnd;
    if counts(
        window_pid(hwnd),
        std::process::id(),
        top_level,
        id_object,
        id_child,
    ) {
        exclude(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    use windows::Win32::UI::Accessibility::NotifyWinEvent;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, IsWindowVisible, MSG, PM_REMOVE,
        PeekMessageW, SW_SHOWNA, ShowWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::w;

    use super::*;

    const EXCLUDED: u32 = 0x11;

    /// An excluder takes every window of the process, the other tests'
    /// too, so the tests that start one run one at a time.
    static ONE_EXCLUDER: Mutex<()> = Mutex::new(());

    fn alone() -> MutexGuard<'static, ()> {
        ONE_EXCLUDER.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A hidden popup of this process, far off screen.
    fn hidden_window() -> HWND {
        unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                -32000,
                -32000,
                8,
                8,
                None,
                None,
                None,
                None,
            )
        }
        .expect("a hidden test window")
    }

    fn affinity(hwnd: HWND) -> u32 {
        let mut value = u32::MAX;
        unsafe { GetWindowDisplayAffinity(hwnd, &mut value) }.expect("the window's affinity");
        value
    }

    fn pump(span: Duration) {
        let until = Instant::now() + span;
        while Instant::now() < until {
            let mut msg = MSG::default();
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn only_top_level_windows_of_this_process_count() {
        assert!(counts(1, 1, true, 0, 0));
        assert!(!counts(2, 1, true, 0, 0), "another process");
        assert!(!counts(1, 1, false, 0, 0), "a child window");
        assert!(
            !counts(1, 1, true, -4, 0),
            "a part of the window (the client area)"
        );
        assert!(!counts(1, 1, true, 0, 3), "a child element");
    }

    #[test]
    fn the_excluder_marks_and_restores_an_own_window() {
        let _alone = alone();
        let hwnd = hidden_window();
        assert_eq!(affinity(hwnd), 0);
        {
            let _excluder = Excluder::start();
            assert_eq!(affinity(hwnd), EXCLUDED);
        }
        assert_eq!(affinity(hwnd), 0, "restored when the excluder drops");
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    /// The hook itself, with no window on screen: the "shown" event is
    /// raised by hand for a window that stays hidden.
    #[test]
    fn a_shown_event_excludes_a_window_made_later() {
        let _alone = alone();
        let excluder = Excluder::start();
        let hwnd = hidden_window();
        assert_eq!(affinity(hwnd), 0, "made after the start");
        unsafe { NotifyWinEvent(EVENT_OBJECT_SHOW, hwnd, 0, 0) };
        let until = Instant::now() + Duration::from_secs(2);
        while affinity(hwnd) != EXCLUDED && Instant::now() < until {
            pump(Duration::from_millis(10));
        }
        assert_eq!(affinity(hwnd), EXCLUDED);
        drop(excluder);
        assert_eq!(affinity(hwnd), 0, "restored when the excluder drops");
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    #[test]
    #[ignore = "shows a window (off screen, never activated); run by hand"]
    fn a_window_shown_while_excluding_is_excluded_too() {
        let _alone = alone();
        let excluder = Excluder::start();
        let hwnd = hidden_window();
        assert_eq!(affinity(hwnd), 0, "made after the start, not yet shown");
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
        pump(Duration::from_millis(200));
        assert_eq!(affinity(hwnd), EXCLUDED);
        drop(excluder);
        assert_eq!(affinity(hwnd), 0, "restored when the excluder drops");
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
