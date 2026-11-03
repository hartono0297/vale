//! The keyboard hook that carries the PrtSc combos: a child of `app`, used by
//! `app_hotkeys`.
//!
//! Windows 11 owns the bare PrtSc key (Snipping Tool, while
//! `PrintScreenKeyForSnippingEnabled` is absent), so `RegisterHotKey` fails on
//! it. A low-level keyboard hook (`WH_KEYBOARD_LL`) sees the key first and can
//! swallow it. Any hotkey row whose combo is PrtSc (bare, or with modifiers)
//! goes through the hook instead of `RegisterHotKey`.
//!
//! The hook lives on its own thread: Windows waits for a low-level hook on
//! every keystroke system-wide and silently removes a slow one, so the busy UI
//! thread must not be the one that answers. The thread exists only while at
//! least one row uses PrtSc (and the hotkeys are not paused) and blocks in
//! `GetMessageW` between keystrokes: no timer, nothing runs while nobody
//! types. The proc does no work: it reads a static table (one `AtomicU8` per
//! row: unassigned, or the modifier bits of that row's PrtSc combo), asks
//! `prtsc_hook_action`, and posts `WM_APP_PRTSC` with the row to the island.
//! Changing a row while the hook runs is just an atomic store.
//!
//! A PrtSc press is taken only when the modifiers held are exactly a row's:
//! with the default row (bare PrtSc), Alt+PrtSc, Win+PrtSc, Ctrl+PrtSc and
//! Shift+PrtSc stay Windows'. Vale writes nothing to the registry.
//!
//! A combo taken with Win or Alt held would leave Windows a bare Win or Alt
//! tap (the PrtSc in between never reached it), which opens Start or the
//! active window's menu bar on release. The proc then sends a mask key (an
//! unassigned vk, down and up) while the modifier is still down, as
//! AutoHotkey's `A_MenuMaskKey` does.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    VK_SNAPSHOT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MSG, PM_NOREMOVE, PeekMessageW,
    PostMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_APP, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::hotkeys::ROWS;
use crate::hotkey::{HotkeyId, Mods};

/// Posted to the island by the hook proc when a PrtSc combo of a row goes
/// down; `wparam` is the row's `HotkeyId::index`.
pub(super) const WM_APP_PRTSC: u32 = WM_APP + 60;

/// The table entry of a row that has no PrtSc combo.
const UNASSIGNED: u8 = 0xFF;

/// One entry per `HotkeyId`: `UNASSIGNED`, or the modifier bits (`Mods::bits`)
/// of that row's PrtSc combo. Written by the island's thread, read by the
/// proc.
static TABLE: [AtomicU8; ROWS] = [const { AtomicU8::new(UNASSIGNED) }; ROWS];

/// The island window the proc posts to (0: none). Set before the thread
/// starts, cleared at shutdown.
static ISLAND: AtomicIsize = AtomicIsize::new(0);
/// A PrtSc key-down was swallowed and its key-up has not come yet. Only the
/// hook thread touches it while the hook runs.
static SWALLOWING: AtomicBool = AtomicBool::new(false);

/// The row's PrtSc combo has these modifiers.
pub(super) fn table_set(id: HotkeyId, mods: Mods) {
    TABLE[id.index()].store(mods.bits(), Ordering::Relaxed);
}

/// The row has no PrtSc combo.
pub(super) fn table_clear(id: HotkeyId) {
    TABLE[id.index()].store(UNASSIGNED, Ordering::Relaxed);
}

/// What the table holds now, as the pure decision wants it.
fn table_snapshot() -> [Option<Mods>; ROWS] {
    std::array::from_fn(|i| {
        let bits = TABLE[i].load(Ordering::Relaxed);
        (bits != UNASSIGNED).then(|| Mods::from_bits(bits))
    })
}

/// What the proc does with one key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HookAction {
    /// Not ours: on to Windows and the other hooks.
    Pass,
    /// Ours, but nothing to start (auto-repeat, the key-up): swallowed.
    Swallow,
    /// A fresh press: swallowed, and the island is told.
    SwallowAndFire,
}

/// The decision for one key event, from the virtual key, whether it is a
/// key-down, the modifiers held, whether the last press was swallowed and the
/// table of rows. Returns the action, the row to fire and the new
/// "swallowing". Pure: the proc only reads state and calls this.
pub(super) fn prtsc_hook_action(
    vk: u32,
    key_down: bool,
    mods: Mods,
    swallowing: bool,
    table: &[Option<Mods>; ROWS],
) -> (HookAction, Option<usize>, bool) {
    if vk != u32::from(VK_SNAPSHOT.0) {
        return (HookAction::Pass, None, swallowing);
    }
    if key_down {
        if swallowing {
            // Auto-repeat of a press already taken: no second capture.
            (HookAction::Swallow, None, true)
        } else if let Some(row) = table.iter().position(|m| *m == Some(mods)) {
            (HookAction::SwallowAndFire, Some(row), true)
        } else {
            // A combo nobody asked for stays Windows'.
            (HookAction::Pass, None, false)
        }
    } else if swallowing {
        (HookAction::Swallow, None, false)
    } else {
        // A key-up of a press that went to Windows.
        (HookAction::Pass, None, false)
    }
}

/// The key sent to mask a Win or Alt release: unassigned (Microsoft's list),
/// so it does nothing itself.
const MASK_VK: u16 = 0xE8;

/// Whether a swallowed combo with these modifiers needs the mask key: Win or
/// Alt released with no key Windows saw in between opens a menu.
pub(super) fn needs_mask(mods: Mods) -> bool {
    mods.win || mods.alt
}

/// Sends the mask key down and up (no allocation: two inputs on the stack).
fn send_mask() {
    let input = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(MASK_VK),
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [input(false), input(true)];
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

/// Whether a modifier key is down right now.
fn held(vk: u16) -> bool {
    let state = unsafe { GetAsyncKeyState(i32::from(vk)) };
    state < 0
}

/// The hook proc. Runs on the hook thread for every key in the system: no
/// allocation, no locks, no logging.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // HC_ACTION (0): a real key message; anything else must be passed on.
    if code == 0 {
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if kb.vkCode == u32::from(VK_SNAPSHOT.0) {
            let msg = wparam.0 as u32;
            let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
            let up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);
            if down || up {
                let mods = Mods {
                    alt: kb.flags.0 & LLKHF_ALTDOWN.0 != 0 || held(VK_MENU.0),
                    win: held(VK_LWIN.0) || held(VK_RWIN.0),
                    ctrl: held(VK_CONTROL.0),
                    shift: held(VK_SHIFT.0),
                };
                let was = SWALLOWING.load(Ordering::Relaxed);
                let (action, row, now) =
                    prtsc_hook_action(kb.vkCode, down, mods, was, &table_snapshot());
                SWALLOWING.store(now, Ordering::Relaxed);
                match action {
                    HookAction::Pass => {}
                    HookAction::Swallow => return LRESULT(1),
                    HookAction::SwallowAndFire => {
                        if needs_mask(mods) {
                            send_mask();
                        }
                        let hwnd = ISLAND.load(Ordering::Relaxed);
                        if hwnd != 0 {
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(hwnd as *mut _)),
                                    WM_APP_PRTSC,
                                    WPARAM(row.unwrap_or(0)),
                                    LPARAM(0),
                                );
                            }
                        }
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The running hook thread.
pub(super) struct HookThread {
    thread_id: u32,
    handle: JoinHandle<()>,
}

impl HookThread {
    /// Starts the thread and waits once for it to say whether the hook went
    /// in. `None`: `SetWindowsHookExW` failed (or the thread did not start).
    pub(super) fn start(hwnd: HWND) -> Option<HookThread> {
        ISLAND.store(hwnd.0 as isize, Ordering::Relaxed);
        SWALLOWING.store(false, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<Option<u32>>();
        let handle = std::thread::Builder::new()
            .name("vale-prtsc-hook".into())
            .spawn(move || hook_thread_main(tx))
            .ok();
        let started = handle.and_then(|handle| match rx.recv() {
            Ok(Some(thread_id)) => Some(HookThread { thread_id, handle }),
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

    /// `WM_QUIT` to the thread, which unhooks and ends; waits for it.
    pub(super) fn stop(self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = self.handle.join();
        ISLAND.store(0, Ordering::Relaxed);
        SWALLOWING.store(false, Ordering::Relaxed);
    }
}

/// The hook thread: install, report, then block in the message loop until
/// `WM_QUIT`.
fn hook_thread_main(tx: mpsc::Sender<Option<u32>>) {
    let hook = unsafe {
        let module = GetModuleHandleW(None).ok();
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module.map(Into::into), 0)
    };
    let Ok(hook) = hook else {
        let _ = tx.send(None);
        return;
    };
    // Make the thread's message queue exist before `stop` can post to it.
    let mut msg = MSG::default();
    unsafe {
        let _ = PeekMessageW(&mut msg, None, WM_QUIT, WM_QUIT, PM_NOREMOVE);
    }
    let _ = tx.send(Some(unsafe { GetCurrentThreadId() }));
    // The low-level hook is called from inside this loop.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {}
    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAP: u32 = 0x2C;
    const NONE: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
    };

    fn only(row: usize, mods: Mods) -> [Option<Mods>; ROWS] {
        let mut t = [None; ROWS];
        t[row] = Some(mods);
        t
    }

    fn ctrl() -> Mods {
        Mods { ctrl: true, ..NONE }
    }

    #[test]
    fn the_message_id_collides_with_no_other_app_message() {
        assert_eq!(WM_APP_PRTSC, WM_APP + 60);
        // Every `WM_APP + n` the crate uses.
        let taken = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 20, 21, 22, 41, 50, 51, 61, 70, 71,
        ];
        for n in taken {
            assert_ne!(WM_APP_PRTSC, WM_APP + n);
        }
    }

    #[test]
    fn an_assigned_combo_fires_its_row_once_and_is_swallowed() {
        let bare = only(4, NONE);
        assert_eq!(
            prtsc_hook_action(SNAP, true, NONE, false, &bare),
            (HookAction::SwallowAndFire, Some(4), true)
        );
        // A modified combo on another row: the right row fires.
        let mut table = only(1, ctrl());
        table[4] = Some(NONE);
        assert_eq!(
            prtsc_hook_action(SNAP, true, ctrl(), false, &table),
            (HookAction::SwallowAndFire, Some(1), true)
        );
        assert_eq!(
            prtsc_hook_action(SNAP, true, NONE, false, &table),
            (HookAction::SwallowAndFire, Some(4), true)
        );
    }

    #[test]
    fn the_modifiers_must_match_exactly() {
        let table = only(4, NONE);
        let cases = [
            Mods { alt: true, ..NONE },
            Mods { win: true, ..NONE },
            ctrl(),
            Mods {
                shift: true,
                ..NONE
            },
            Mods {
                ctrl: true,
                alt: true,
                ..NONE
            },
        ];
        for mods in cases {
            assert_eq!(
                prtsc_hook_action(SNAP, true, mods, false, &table),
                (HookAction::Pass, None, false),
                "{mods:?}"
            );
        }
        // And a row with Ctrl+Shift does not take a bare press or Ctrl alone.
        let both = Mods {
            ctrl: true,
            shift: true,
            ..NONE
        };
        let table = only(2, both);
        for mods in [NONE, ctrl()] {
            assert_eq!(
                prtsc_hook_action(SNAP, true, mods, false, &table).0,
                HookAction::Pass
            );
        }
        assert_eq!(
            prtsc_hook_action(SNAP, true, both, false, &table).1,
            Some(2)
        );
    }

    #[test]
    fn an_empty_table_takes_nothing() {
        for mods in [NONE, ctrl()] {
            for down in [true, false] {
                assert_eq!(
                    prtsc_hook_action(SNAP, down, mods, false, &[None; ROWS]),
                    (HookAction::Pass, None, false)
                );
            }
        }
    }

    #[test]
    fn only_win_or_alt_combos_need_the_mask_key() {
        let win = Mods { win: true, ..NONE };
        let alt = Mods { alt: true, ..NONE };
        let shift = Mods {
            shift: true,
            ..NONE
        };
        for mods in [
            win,
            alt,
            Mods { ctrl: true, ..win },
            Mods { shift: true, ..alt },
        ] {
            assert!(needs_mask(mods), "{mods:?}");
        }
        for mods in [
            NONE,
            ctrl(),
            shift,
            Mods {
                shift: true,
                ..ctrl()
            },
        ] {
            assert!(!needs_mask(mods), "{mods:?}");
        }
        // The mask key is one Windows leaves unassigned, and no key a hotkey
        // can use.
        assert_eq!(MASK_VK, 0xE8);
        assert!(!crate::hotkey::is_modifier_vk(MASK_VK));
        assert_ne!(MASK_VK, VK_SNAPSHOT.0);
    }

    #[test]
    fn auto_repeat_is_swallowed_without_a_second_fire() {
        let table = only(4, NONE);
        assert_eq!(
            prtsc_hook_action(SNAP, true, NONE, true, &table),
            (HookAction::Swallow, None, true)
        );
        // Even if a modifier went down meanwhile: the press is still ours.
        assert_eq!(
            prtsc_hook_action(SNAP, true, ctrl(), true, &table),
            (HookAction::Swallow, None, true)
        );
    }

    #[test]
    fn the_key_up_after_a_swallowed_press_is_swallowed_and_clears() {
        let table = only(4, NONE);
        assert_eq!(
            prtsc_hook_action(SNAP, false, NONE, true, &table),
            (HookAction::Swallow, None, false)
        );
        // The whole press: down, repeat, repeat, up, then a new press fires.
        let mut sw = false;
        let mut fired = Vec::new();
        for (down, expect_swallow) in [(true, true), (true, true), (true, true), (false, true)] {
            let (action, row, now) = prtsc_hook_action(SNAP, down, NONE, sw, &table);
            if action == HookAction::SwallowAndFire {
                fired.push(row);
            }
            assert_eq!(action != HookAction::Pass, expect_swallow);
            sw = now;
        }
        assert_eq!(fired, vec![Some(4)]);
        assert!(!sw);
        assert_eq!(
            prtsc_hook_action(SNAP, true, NONE, sw, &table),
            (HookAction::SwallowAndFire, Some(4), true)
        );
    }

    #[test]
    fn a_key_up_that_was_not_swallowed_passes() {
        let table = only(4, NONE);
        for mods in [NONE, Mods { alt: true, ..NONE }] {
            assert_eq!(
                prtsc_hook_action(SNAP, false, mods, false, &table),
                (HookAction::Pass, None, false)
            );
        }
    }

    #[test]
    fn other_keys_pass_and_leave_the_swallowing_alone() {
        let table = only(4, NONE);
        for vk in [0x41, 0x1B, 0x2D, 0x5B, 0x70] {
            for down in [true, false] {
                for sw in [false, true] {
                    assert_eq!(
                        prtsc_hook_action(vk, down, NONE, sw, &table),
                        (HookAction::Pass, None, sw),
                        "vk={vk:#x} down={down} sw={sw}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_table_is_atomic_stores_and_reads_back() {
        // Only the Devices row is used here, and it is cleared at the end.
        table_set(HotkeyId::Devices, ctrl());
        assert_eq!(table_snapshot()[HotkeyId::Devices.index()], Some(ctrl()));
        table_clear(HotkeyId::Devices);
        assert_eq!(table_snapshot()[HotkeyId::Devices.index()], None);
        table_set(HotkeyId::Devices, NONE);
        assert_eq!(table_snapshot()[HotkeyId::Devices.index()], Some(NONE));
        table_clear(HotkeyId::Devices);
    }

    #[test]
    fn the_hook_thread_starts_and_stops_cleanly() {
        // No island window: a press would post nowhere. Install and remove
        // again at once; a machine that refuses the hook just reports None.
        if let Some(hook) = HookThread::start(HWND::default()) {
            hook.stop();
        }
        assert_eq!(ISLAND.load(Ordering::Relaxed), 0);
        assert!(!SWALLOWING.load(Ordering::Relaxed));
    }
}
