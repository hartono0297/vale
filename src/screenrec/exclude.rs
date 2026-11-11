//! Keeps Vale's own windows out of a screen recording: every top-level
//! window of this process is excluded from capture while an [`Excluder`]
//! lives, windows made or shown later included. Nothing here exists, and
//! no hook runs, outside a recording.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::sync::{Mutex, MutexGuard};

use windows::Win32::Foundation::{HMODULE, HWND, LPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW, EnumWindows, GA_ROOT, GetAncestor,
    GetWindowDisplayAffinity, GetWindowThreadProcessId, IsWindow, WDA_NONE, WINEVENT_INCONTEXT,
    WINEVENT_OUTOFCONTEXT,
};
use windows::core::BOOL;

use super::capture::exclude_from_capture;

/// The window itself, not one of its parts.
const OBJID_WINDOW: i32 = 0;
const CHILDID_SELF: i32 = 0;

/// Whether a "made" or "shown" event is one of Vale's own top-level
/// windows: this process, top level, the window itself (`OBJID_WINDOW`,
/// `CHILDID_SELF`) and not one of its parts (a scroll bar, the caret, the
/// cursor).
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
/// does each one made or shown later, on any thread, before it can be on
/// screen (the hook runs inside the call that makes or shows it). Dropping
/// it gives every window it changed its previous affinity back.
///
/// One at a time in the process. It stays on the thread that started it
/// (the island's): a hook is removed by the thread that installed it.
pub struct Excluder {
    hook: HWINEVENTHOOK,
    /// Unhooked on the starting thread only.
    _own_thread: PhantomData<*const ()>,
}

/// The one live excluder's windows, shared with the hook, which runs on
/// whichever thread makes or shows a window.
struct Shared {
    /// An excluder lives. Off, a late callback changes nothing.
    active: bool,
    /// The windows taken out of the capture. Each had no affinity before
    /// (`WDA_NONE`), which is what the drop restores.
    changed: Vec<isize>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    active: false,
    changed: Vec::new(),
});

/// Never held across a Windows call: the hook can run inside one, on the
/// same thread.
fn shared() -> MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

impl Excluder {
    pub fn start() -> Excluder {
        {
            let mut shared = shared();
            debug_assert!(!shared.active, "one excluder at a time");
            shared.active = true;
        }
        // The hook first, then the windows that exist: one made in between
        // is caught by either (excluding twice is harmless).
        let hook = install_hook();
        unsafe extern "system" fn each(hwnd: HWND, _: LPARAM) -> BOOL {
            if window_pid(hwnd) == std::process::id() {
                exclude(hwnd);
            }
            true.into()
        }
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(0));
        }
        Excluder {
            hook,
            _own_thread: PhantomData,
        }
    }

    /// Whether windows made or shown from now on are caught. `false` when
    /// Windows refused the hook: only the windows that existed at the start
    /// are out of the video.
    pub fn hooked(&self) -> bool {
        !self.hook.is_invalid()
    }
}

/// The hook for windows made or shown later. In context (called inside the
/// call that raises the event, on its thread), so a window is out of the
/// capture before it is drawn; the module is Vale's own exe, which is all
/// an in-context hook on the own process needs. No `WINEVENT_SKIPOWNTHREAD`
/// / `SKIPOWNPROCESS`: the windows to catch are Vale's own.
///
/// Should Windows refuse it, an out-of-context hook on "shown" stands in:
/// it reports to the starting thread's message loop, so a new window may
/// be in the video for a frame or more.
fn install_hook() -> HWINEVENTHOOK {
    let pid = std::process::id();
    let module: HMODULE = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_SHOW,
            Some(module),
            Some(on_event),
            pid,
            0,
            WINEVENT_INCONTEXT,
        )
    };
    if !hook.is_invalid() {
        return hook;
    }
    eprintln!("vale: screen recording: no in-context window hook, new windows are excluded late");
    let hook = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_SHOW,
            None,
            Some(on_event),
            pid,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_invalid() {
        eprintln!("vale: screen recording: no window hook, new windows are not excluded");
    }
    hook
}

impl Drop for Excluder {
    fn drop(&mut self) {
        if !self.hook.is_invalid() {
            unsafe {
                let _ = UnhookWinEvent(self.hook);
            }
        }
        // Off and emptied under one lock: a callback still running on
        // another thread finds it off and undoes its own change.
        let changed = {
            let mut shared = shared();
            shared.active = false;
            std::mem::take(&mut shared.changed)
        };
        for hwnd in changed {
            let hwnd = HWND(hwnd as *mut c_void);
            if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
                exclude_from_capture(hwnd, false);
            }
        }
    }
}

fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Takes `hwnd` out of the capture and remembers it, unless it already has
/// an affinity of its own: that one is kept and never touched. Does nothing
/// when no excluder lives.
fn exclude(hwnd: HWND) {
    if !shared().active {
        return;
    }
    let mut before = 0u32;
    if unsafe { GetWindowDisplayAffinity(hwnd, &mut before) }.is_err() || before != WDA_NONE.0 {
        return;
    }
    if !exclude_from_capture(hwnd, true) {
        return;
    }
    let key = hwnd.0 as isize;
    let still_active = {
        let mut shared = shared();
        if shared.active && !shared.changed.contains(&key) {
            shared.changed.push(key);
        }
        shared.active
    };
    if !still_active {
        // The excluder dropped meanwhile and no longer knows this window.
        exclude_from_capture(hwnd, false);
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // The range also holds "destroyed"; and most events are for parts of
    // windows, which are turned away before any call.
    if (event != EVENT_OBJECT_CREATE && event != EVENT_OBJECT_SHOW)
        || id_object != OBJID_WINDOW
        || id_child != CHILDID_SELF
        || hwnd.is_invalid()
    {
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

    use windows::Win32::UI::Accessibility::NotifyWinEvent;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, IsWindowVisible, SW_SHOWNA, ShowWindow, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_POPUP,
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

    /// A window made while excluding that its creation did not catch: back
    /// in the capture, so only a later event can take it out again.
    fn missed_window() -> HWND {
        let hwnd = hidden_window();
        exclude_from_capture(hwnd, false);
        assert_eq!(affinity(hwnd), 0);
        hwnd
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
            let excluder = Excluder::start();
            assert!(excluder.hooked());
            assert_eq!(affinity(hwnd), EXCLUDED);
        }
        assert_eq!(affinity(hwnd), 0, "restored when the excluder drops");
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    /// No message is read between the creation and the check: the hook
    /// runs inside `CreateWindowExW`, before the window can be on screen.
    #[test]
    fn a_window_is_excluded_as_soon_as_it_is_made() {
        let _alone = alone();
        let excluder = Excluder::start();
        let hwnd = hidden_window();
        assert_eq!(affinity(hwnd), EXCLUDED, "right after CreateWindowExW");
        drop(excluder);
        assert_eq!(affinity(hwnd), 0, "restored when the excluder drops");
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    /// The "shown" event, raised by hand for a window that stays hidden,
    /// is handled before `NotifyWinEvent` returns.
    #[test]
    fn a_shown_event_excludes_a_window_made_later() {
        let _alone = alone();
        let excluder = Excluder::start();
        let hwnd = missed_window();
        unsafe { NotifyWinEvent(EVENT_OBJECT_SHOW, hwnd, 0, 0) };
        assert_eq!(affinity(hwnd), EXCLUDED, "right after the event, no pump");
        drop(excluder);
        assert_eq!(affinity(hwnd), 0, "restored when the excluder drops");
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    /// Windows made on another thread (a file dialog's worker) are caught
    /// too, on that thread.
    #[test]
    fn a_window_made_on_another_thread_is_excluded_too() {
        let _alone = alone();
        let excluder = Excluder::start();
        let seen = std::thread::spawn(|| {
            let hwnd = hidden_window();
            let seen = affinity(hwnd);
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            seen
        })
        .join()
        .expect("the window's thread");
        assert_eq!(seen, EXCLUDED);
        drop(excluder);
    }

    #[test]
    fn nothing_is_excluded_once_the_excluder_is_gone() {
        let _alone = alone();
        drop(Excluder::start());
        let hwnd = hidden_window();
        unsafe { NotifyWinEvent(EVENT_OBJECT_SHOW, hwnd, 0, 0) };
        assert_eq!(affinity(hwnd), 0);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    #[test]
    #[ignore = "shows a window (off screen, never activated); run by hand"]
    fn a_window_shown_while_excluding_is_excluded_too() {
        let _alone = alone();
        let excluder = Excluder::start();
        let hwnd = missed_window();
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
        assert_eq!(affinity(hwnd), EXCLUDED, "right after ShowWindow, no pump");
        drop(excluder);
        assert_eq!(affinity(hwnd), 0, "restored when the excluder drops");
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
