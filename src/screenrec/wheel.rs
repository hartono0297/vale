//! Ctrl+Alt + mouse wheel changes the zoom level while the video is zoomed.
//! A low-level mouse hook on its own thread; it exists only while a
//! [`WheelHook`] lives, which is while the recording is zoomed.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostMessageW,
    PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL, WM_APP, WM_MOUSEWHEEL,
    WM_QUIT,
};

/// Posted to the island: `wparam` is the signed step count (`isize as
/// usize`), positive for wheel up (closer).
pub const WM_APP_ZOOM_WHEEL: u32 = WM_APP + 92;

/// One wheel notch.
const NOTCH: i32 = 120;

/// Adds up wheel deltas (smooth wheels send fractions of a notch).
#[derive(Default)]
pub struct WheelAcc {
    acc: i32,
}

impl WheelAcc {
    /// Whole notches so far, sign = direction; the rest is kept. A delta of
    /// the other sign drops the leftover first.
    pub fn push(&mut self, delta: i32) -> i32 {
        if (self.acc > 0 && delta < 0) || (self.acc < 0 && delta > 0) {
            self.acc = 0;
        }
        self.acc += delta;
        let steps = self.acc / NOTCH;
        self.acc -= steps * NOTCH;
        steps
    }
}

/// Whether the held modifiers are exactly Ctrl+Alt.
pub fn zoom_wheel_mods(ctrl: bool, alt: bool, shift: bool, win: bool) -> bool {
    ctrl && alt && !shift && !win
}

/// The island's window, for the hook proc.
static ISLAND: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    /// The hook thread's accumulator.
    static ACC: RefCell<WheelAcc> = const { RefCell::new(WheelAcc { acc: 0 }) };
}

/// Whether a key is down right now.
fn held(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
}

/// The hook proc. Runs for every mouse event in the system: read the
/// modifiers, accumulate, post, return. Nothing else.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // HC_ACTION (0): a real mouse message.
    if code == 0 && wparam.0 as u32 == WM_MOUSEWHEEL {
        let mods = zoom_wheel_mods(
            held(VK_CONTROL.0),
            held(VK_MENU.0),
            held(VK_SHIFT.0),
            held(VK_LWIN.0) || held(VK_RWIN.0),
        );
        if mods {
            let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            let delta = i32::from((info.mouseData >> 16) as u16 as i16);
            let steps = ACC.with(|a| a.borrow_mut().push(delta));
            let hwnd = ISLAND.load(Ordering::Relaxed);
            if steps != 0 && hwnd != 0 {
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(hwnd as *mut _)),
                        WM_APP_ZOOM_WHEEL,
                        WPARAM(steps as isize as usize),
                        LPARAM(0),
                    );
                }
            }
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The running hook thread. Dropping it stops the thread.
pub struct WheelHook {
    thread_id: u32,
    handle: Option<JoinHandle<()>>,
}

impl WheelHook {
    /// Starts the thread and waits once for it to say whether the hook went
    /// in. `None`: `SetWindowsHookExW` failed (or the thread did not start).
    pub fn start(hwnd: HWND) -> Option<WheelHook> {
        ISLAND.store(hwnd.0 as isize, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<Option<u32>>();
        let handle = std::thread::Builder::new()
            .name("vale-zoom-wheel".into())
            .spawn(move || hook_thread_main(tx))
            .ok();
        let started = handle.and_then(|handle| match rx.recv() {
            Ok(Some(thread_id)) => Some(WheelHook {
                thread_id,
                handle: Some(handle),
            }),
            _ => {
                let _ = handle.join();
                None
            }
        });
        if started.is_none() {
            ISLAND.store(0, Ordering::Relaxed);
        }
        started
    }

    /// Drops the hook (Drop sends `WM_QUIT` and joins the thread).
    pub fn stop(self) {}
}

impl Drop for WheelHook {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = handle.join();
            ISLAND.store(0, Ordering::Relaxed);
        }
    }
}

/// The hook thread: install, report, then block in the message loop until
/// `WM_QUIT`.
fn hook_thread_main(tx: mpsc::Sender<Option<u32>>) {
    let hook = unsafe {
        let module = GetModuleHandleW(None).ok();
        SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), module.map(Into::into), 0)
    };
    let Ok(hook) = hook else {
        let _ = tx.send(None);
        return;
    };
    // Make the thread's message queue exist before a stop can post to it.
    let mut msg = MSG::default();
    unsafe {
        let _ = PeekMessageW(&mut msg, None, WM_QUIT, WM_QUIT, PM_NOREMOVE);
    }
    let _ = tx.send(Some(unsafe { GetCurrentThreadId() }));
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {}
    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_notch_is_one_step() {
        let mut w = WheelAcc::default();
        assert_eq!(w.push(120), 1);
        assert_eq!(w.push(-120), -1);
        assert_eq!(w.push(240), 2);
    }

    #[test]
    fn smooth_wheels_add_up_to_a_notch() {
        let mut w = WheelAcc::default();
        assert_eq!(w.push(30), 0);
        assert_eq!(w.push(30), 0);
        assert_eq!(w.push(30), 0);
        assert_eq!(w.push(30), 1);
        assert_eq!(w.push(90), 0);
    }

    #[test]
    fn a_turn_back_starts_over() {
        let mut w = WheelAcc::default();
        assert_eq!(w.push(90), 0);
        assert_eq!(w.push(-60), 0);
        assert_eq!(w.push(-60), -1);
    }

    #[test]
    fn only_ctrl_alt_exactly() {
        assert!(zoom_wheel_mods(true, true, false, false));
        assert!(!zoom_wheel_mods(true, false, false, false));
        assert!(!zoom_wheel_mods(false, true, false, false));
        assert!(!zoom_wheel_mods(true, true, true, false));
        assert!(!zoom_wheel_mods(true, true, false, true));
    }

    #[test]
    fn the_message_id_is_track_cs() {
        assert_eq!(WM_APP_ZOOM_WHEEL, WM_APP + 92);
    }
}
