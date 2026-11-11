//! Watches the recorded window: tells the island when it is minimised
//! (nothing to record, the recording pauses) and when it is back. The hook
//! exists only while a [`WindowWatch`] lives, which is while a window is
//! being recorded.

// Wired into the app with the screen recording's start (track A).
#![cfg_attr(not(test), allow(dead_code))]

use std::cell::RefCell;
use std::ffi::c_void;
use std::marker::PhantomData;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GetWindowThreadProcessId, PostMessageW,
    WINEVENT_OUTOFCONTEXT,
};

/// What an event says about the watched window: `Some(true)` minimised,
/// `Some(false)` restored, `None` for another window or another event.
pub fn watch_event(event: u32, hwnd: isize, target: isize) -> Option<bool> {
    if hwnd != target {
        return None;
    }
    match event {
        EVENT_SYSTEM_MINIMIZESTART => Some(true),
        EVENT_SYSTEM_MINIMIZEEND => Some(false),
        _ => None,
    }
}

/// Posts `msg` to `notify` whenever the watched window is minimised
/// (`wparam` 1) or restored (`wparam` 0), until it drops.
///
/// It belongs to the thread that started it: the hook reports to that
/// thread's message loop.
pub struct WindowWatch {
    hook: HWINEVENTHOOK,
    /// The hook calls back on the starting thread only.
    _own_thread: PhantomData<*const ()>,
}

/// What one live watch looks for and whom it tells.
#[derive(Clone, Copy)]
struct Watched {
    hook: isize,
    target: isize,
    notify: isize,
    msg: u32,
}

thread_local! {
    /// This thread's live watches, found by their hook in the callback.
    static WATCHED: RefCell<Vec<Watched>> = const { RefCell::new(Vec::new()) };
}

impl WindowWatch {
    /// `None` when the window is gone or Windows refused the hook.
    pub fn start(target: isize, notify: HWND, msg: u32) -> Option<WindowWatch> {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(HWND(target as *mut c_void), Some(&mut pid)) };
        if pid == 0 {
            return None;
        }
        // Only the target's process is hooked. It is never Vale's own: the
        // picker leaves Vale's windows out.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_MINIMIZESTART,
                EVENT_SYSTEM_MINIMIZEEND,
                None,
                Some(on_event),
                pid,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            return None;
        }
        WATCHED.with_borrow_mut(|watched| {
            watched.push(Watched {
                hook: hook.0 as isize,
                target,
                notify: notify.0 as isize,
                msg,
            })
        });
        Some(WindowWatch {
            hook,
            _own_thread: PhantomData,
        })
    }
}

impl Drop for WindowWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWinEvent(self.hook);
        }
        let hook = self.hook.0 as isize;
        WATCHED.with_borrow_mut(|watched| watched.retain(|w| w.hook != hook));
    }
}

unsafe extern "system" fn on_event(
    hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    let hook = hook.0 as isize;
    let watch = WATCHED.with_borrow(|watched| watched.iter().find(|w| w.hook == hook).copied());
    let Some(watch) = watch else {
        return;
    };
    if let Some(minimised) = watch_event(event, hwnd.0 as isize, watch.target) {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(watch.notify as *mut c_void)),
                watch.msg,
                WPARAM(minimised as usize),
                LPARAM(0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use windows::Win32::UI::Accessibility::NotifyWinEvent;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, IsWindowVisible, MSG, PM_REMOVE, PeekMessageW,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::w;

    use super::*;

    const MSG_WATCH: u32 = 0x8000 + 91;

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

    /// The `wparam` of the next watch message posted to `notify`, waiting
    /// up to `wait` (the hook calls back while this thread reads messages).
    fn next_report(notify: HWND, wait: Duration) -> Option<usize> {
        let until = Instant::now() + wait;
        loop {
            let mut msg = MSG::default();
            let got = unsafe {
                PeekMessageW(&mut msg, Some(notify), MSG_WATCH, MSG_WATCH, PM_REMOVE).as_bool()
            };
            if got {
                return Some(msg.wParam.0);
            }
            if Instant::now() >= until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The hook itself, with no window on screen: the minimise events are
    /// raised by hand for windows that stay hidden.
    #[test]
    fn the_watch_posts_minimise_and_restore_of_its_window_only() {
        let target = hidden_window();
        let other = hidden_window();
        let notify = hidden_window();
        let watch = WindowWatch::start(target.0 as isize, notify, MSG_WATCH).expect("a watch");

        unsafe { NotifyWinEvent(EVENT_SYSTEM_MINIMIZESTART, other, 0, 0) };
        unsafe { NotifyWinEvent(EVENT_SYSTEM_MINIMIZESTART, target, 0, 0) };
        assert_eq!(next_report(notify, Duration::from_secs(2)), Some(1));
        unsafe { NotifyWinEvent(EVENT_SYSTEM_MINIMIZEEND, target, 0, 0) };
        assert_eq!(next_report(notify, Duration::from_secs(2)), Some(0));
        assert_eq!(
            next_report(notify, Duration::from_millis(100)),
            None,
            "the other window's minimise was not reported"
        );

        drop(watch);
        unsafe { NotifyWinEvent(EVENT_SYSTEM_MINIMIZESTART, target, 0, 0) };
        assert_eq!(
            next_report(notify, Duration::from_millis(200)),
            None,
            "nothing after the drop"
        );
        unsafe {
            for hwnd in [target, other, notify] {
                assert!(!IsWindowVisible(hwnd).as_bool());
                let _ = DestroyWindow(hwnd);
            }
        }
    }

    #[test]
    fn the_watch_reports_only_its_window() {
        assert_eq!(watch_event(0x16, 5, 5), Some(true));
        assert_eq!(watch_event(0x17, 5, 5), Some(false));
        assert_eq!(watch_event(0x16, 5, 6), None, "another window");
        assert_eq!(watch_event(0x17, 5, 6), None, "another window");
        assert_eq!(watch_event(0x8002, 5, 5), None, "another event");
    }

    #[test]
    fn a_window_that_is_gone_starts_no_watch() {
        assert!(WindowWatch::start(0, HWND::default(), 0x8000 + 91).is_none());
    }
}
