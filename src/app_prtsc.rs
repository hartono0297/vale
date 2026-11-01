//! The PrtSc key opens Vale's own screenshot (`capture` and the editor after
//! it): the keyboard hook that sees the key, what Settings shows when the
//! hook could not be installed. A child of `app`, so it works on `App`
//! directly.
//!
//! Windows 11 owns the bare key (Snipping Tool, while
//! `PrintScreenKeyForSnippingEnabled` is absent), so `RegisterHotKey` fails.
//! A low-level keyboard hook (`WH_KEYBOARD_LL`) sees the key first and
//! swallows it. The hook lives on its own thread: Windows waits for a
//! low-level hook on every keystroke system-wide and silently removes a slow
//! one, so the busy UI thread must not be the one that answers. The thread
//! is started only while the setting is on and blocks in `GetMessageW`
//! between keystrokes: no timer, nothing runs while nobody types. The proc
//! does no work: it reads two statics, asks `prtsc_hook_action`, and posts
//! `WM_APP_PRTSC` to the island.
//!
//! The key is taken bare: Alt+PrtSc, Win+PrtSc, Ctrl+PrtSc and Shift+PrtSc go
//! to Windows untouched. Vale writes nothing to the registry.
//!
//! A failed install (Settings shows a red note) is tried again only when the
//! tray menu opens or Settings opens (`retry_hotkeys`). A config save or read
//! starts or stops the hook only when the setting itself moved
//! (`follows_config`), so a toggle click is one try and an unrelated save
//! none.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_SNAPSHOT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MSG, PM_NOREMOVE, PeekMessageW,
    PostMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_APP, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::App;
use crate::model::Event;
use crate::ui::settings_layout::PrtscRow;

/// Posted to the island by the hook proc when a bare PrtSc goes down.
pub(super) const WM_APP_PRTSC: u32 = WM_APP + 60;

/// The island window the proc posts to (0: none). Set before the thread
/// starts, cleared at shutdown.
static ISLAND: AtomicIsize = AtomicIsize::new(0);
/// A PrtSc key-down was swallowed and its key-up has not come yet. Only the
/// hook thread touches it while the hook runs.
static SWALLOWING: AtomicBool = AtomicBool::new(false);

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

/// The modifiers held when the key went down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Mods {
    pub alt: bool,
    pub win: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl Mods {
    fn any(self) -> bool {
        self.alt || self.win || self.ctrl || self.shift
    }
}

/// The decision for one key event, from the virtual key, whether it is a
/// key-down, the modifiers and whether the last press was swallowed. Returns
/// the action and the new "swallowing". Pure: the proc only reads state and
/// calls this.
pub(super) fn prtsc_hook_action(
    vk: u32,
    key_down: bool,
    mods: Mods,
    swallowing: bool,
) -> (HookAction, bool) {
    if vk != u32::from(VK_SNAPSHOT.0) {
        return (HookAction::Pass, swallowing);
    }
    if key_down {
        if swallowing {
            // Auto-repeat of a press already taken: no second capture.
            (HookAction::Swallow, true)
        } else if mods.any() {
            (HookAction::Pass, false)
        } else {
            (HookAction::SwallowAndFire, true)
        }
    } else if swallowing {
        (HookAction::Swallow, false)
    } else {
        // A key-up of a press that went to Windows.
        (HookAction::Pass, false)
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
                let (action, now) = prtsc_hook_action(kb.vkCode, down, mods, was);
                SWALLOWING.store(now, Ordering::Relaxed);
                match action {
                    HookAction::Pass => {}
                    HookAction::Swallow => return LRESULT(1),
                    HookAction::SwallowAndFire => {
                        let hwnd = ISLAND.load(Ordering::Relaxed);
                        if hwnd != 0 {
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(hwnd as *mut _)),
                                    WM_APP_PRTSC,
                                    WPARAM(0),
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
struct HookThread {
    thread_id: u32,
    handle: JoinHandle<()>,
}

impl HookThread {
    /// Starts the thread and waits once for it to say whether the hook went
    /// in. `None`: `SetWindowsHookExW` failed (or the thread did not start).
    fn start(hwnd: HWND) -> Option<HookThread> {
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
    fn stop(self) {
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

/// What the PrtSc hook holds: the running thread and whether the last
/// install failed.
#[derive(Default)]
pub(super) struct PrtscState {
    hook: Option<HookThread>,
    pub failed: bool,
}

impl PrtscState {
    /// Whether the hook is installed.
    pub fn installed(&self) -> bool {
        self.hook.is_some()
    }

    /// The Settings row, from what is kept.
    fn row(&self, on: bool) -> RowState {
        prtsc_row(on, self.failed)
    }
}

/// What the Settings row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowState {
    /// The toggle is off: no hook, nothing to say.
    Off,
    /// On and the hook is installed.
    Active,
    /// On, but `SetWindowsHookExW` failed.
    Failed,
}

/// The row's state from the toggle and whether the last install failed.
pub(super) fn prtsc_row(on: bool, failed: bool) -> RowState {
    if !on {
        RowState::Off
    } else if failed {
        RowState::Failed
    } else {
        RowState::Active
    }
}

/// What Settings draws for a row state.
pub(super) fn ui_row(row: RowState) -> PrtscRow {
    match row {
        RowState::Off | RowState::Active => PrtscRow::Plain,
        RowState::Failed => PrtscRow::Failed,
    }
}

/// Whether a config save or read has to touch the hook: only when the
/// setting no longer matches what is installed or was last tried (turned on
/// and never tried, turned off while installed or failed). A failed hook that
/// is still wanted waits for `retry_hotkeys`.
pub(super) fn follows_config(want: bool, installed: bool, failed: bool) -> bool {
    want != (installed || failed)
}

/// What a sync has to do to the hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Plan {
    Nothing,
    Install,
    Uninstall,
}

/// The step from the setting and whether the hook is installed. A failed
/// hook is tried again (`Install`) only while it is wanted; one that is
/// installed is left alone.
pub(super) fn plan(want: bool, installed: bool) -> Plan {
    match (want, installed) {
        (true, false) => Plan::Install,
        (false, true) => Plan::Uninstall,
        _ => Plan::Nothing,
    }
}

impl App {
    /// Installs or removes the PrtSc hook as the setting says. A failure
    /// shows in Settings and is tried again on the next sync, tray menu or
    /// Settings opening.
    pub(super) fn sync_prtsc_hook(&mut self) {
        let want = self.config.screenshot.print_screen;
        match plan(want, self.prtsc.installed()) {
            Plan::Nothing => {}
            Plan::Uninstall => {
                if let Some(hook) = self.prtsc.hook.take() {
                    hook.stop();
                }
            }
            Plan::Install => {
                self.prtsc.hook = HookThread::start(self.hwnd);
                self.prtsc.failed = !self.prtsc.installed();
            }
        }
        if !want {
            self.prtsc.failed = false;
        }
    }

    /// `WM_APP_PRTSC`: Vale's own capture, whatever "Alat screenshot" says
    /// (that choice is for the tile). Starts through the shortcut timer like
    /// the tile, after an open panel or the search has collapsed, so it is
    /// not in the picture. One overlay at a time: a press while it is up (or
    /// while it is starting) does nothing.
    pub(super) fn on_prtsc_hotkey(&mut self) {
        if !self.config.screenshot.print_screen || crate::capture::overlay::active() {
            return;
        }
        self.apply_now(Event::PrintScreen);
    }

    /// The Settings toggle: kept and saved; the hook follows from
    /// `adopt_config` (`prtsc_follow_config`), one try per click.
    pub(super) fn set_print_screen(&mut self, on: bool) {
        self.config.screenshot.print_screen = on;
        self.save_config();
    }

    /// A config save or read (the toggle, a hand edit): the hook follows a
    /// changed setting. Not a retry of a failed hook (`retry_hotkeys` is).
    pub(super) fn prtsc_follow_config(&mut self) {
        let want = self.config.screenshot.print_screen;
        if follows_config(want, self.prtsc.installed(), self.prtsc.failed) {
            self.sync_prtsc_hook();
        }
    }

    /// The row's state for Settings.
    pub(super) fn prtsc_model(&self) -> PrtscRow {
        ui_row(self.prtsc.row(self.config.screenshot.print_screen))
    }

    /// `WM_DESTROY`: the hook goes.
    pub(super) fn prtsc_shutdown(&mut self) {
        ISLAND.store(0, Ordering::Relaxed);
        if let Some(hook) = self.prtsc.hook.take() {
            hook.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNAP: u32 = 0x2C;
    const NONE: Mods = Mods {
        alt: false,
        win: false,
        ctrl: false,
        shift: false,
    };

    #[test]
    fn the_hotkey_ids_of_the_other_keys_stay_distinct() {
        assert_eq!(super::super::clip::HOTKEY_CLIP, 1);
        assert_eq!(super::super::HOTKEY_SEARCH, 2);
        assert_eq!(super::super::HOTKEY_DEVICES, 3);
        assert_eq!(super::super::HOTKEY_ARRANGE, 4);
    }

    #[test]
    fn the_message_id_collides_with_no_other_app_message() {
        assert_eq!(WM_APP_PRTSC, WM_APP + 60);
        // Every `WM_APP + n` the crate uses.
        let taken = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 20, 21, 22, 41, 50, 51, 61,
        ];
        for n in taken {
            assert_ne!(WM_APP_PRTSC, WM_APP + n);
        }
    }

    #[test]
    fn a_bare_press_fires_once_and_is_swallowed() {
        assert_eq!(
            prtsc_hook_action(SNAP, true, NONE, false),
            (HookAction::SwallowAndFire, true)
        );
    }

    #[test]
    fn auto_repeat_is_swallowed_without_a_second_fire() {
        assert_eq!(
            prtsc_hook_action(SNAP, true, NONE, true),
            (HookAction::Swallow, true)
        );
        // Even if a modifier went down meanwhile: the press is still ours.
        let shift = Mods {
            shift: true,
            ..NONE
        };
        assert_eq!(
            prtsc_hook_action(SNAP, true, shift, true),
            (HookAction::Swallow, true)
        );
    }

    #[test]
    fn the_key_up_after_a_swallowed_press_is_swallowed_and_clears() {
        assert_eq!(
            prtsc_hook_action(SNAP, false, NONE, true),
            (HookAction::Swallow, false)
        );
        // The whole press: down, repeat, repeat, up, then a new press fires.
        let mut sw = false;
        let mut fired = 0;
        for (down, expect_swallow) in [(true, true), (true, true), (true, true), (false, true)] {
            let (action, now) = prtsc_hook_action(SNAP, down, NONE, sw);
            fired += usize::from(action == HookAction::SwallowAndFire);
            assert_eq!(action != HookAction::Pass, expect_swallow);
            sw = now;
        }
        assert_eq!(fired, 1);
        assert!(!sw);
        assert_eq!(
            prtsc_hook_action(SNAP, true, NONE, sw),
            (HookAction::SwallowAndFire, true)
        );
    }

    #[test]
    fn each_modifier_sends_the_key_to_windows() {
        let cases = [
            Mods { alt: true, ..NONE },
            Mods { win: true, ..NONE },
            Mods { ctrl: true, ..NONE },
            Mods {
                shift: true,
                ..NONE
            },
        ];
        for mods in cases {
            assert_eq!(
                prtsc_hook_action(SNAP, true, mods, false),
                (HookAction::Pass, false),
                "{mods:?}"
            );
        }
    }

    #[test]
    fn a_key_up_that_was_not_swallowed_passes() {
        for mods in [NONE, Mods { alt: true, ..NONE }] {
            assert_eq!(
                prtsc_hook_action(SNAP, false, mods, false),
                (HookAction::Pass, false)
            );
        }
    }

    #[test]
    fn other_keys_pass_and_leave_the_swallowing_alone() {
        for vk in [0x41, 0x1B, 0x2D, 0x5B, 0x70] {
            for down in [true, false] {
                for sw in [false, true] {
                    assert_eq!(
                        prtsc_hook_action(vk, down, NONE, sw),
                        (HookAction::Pass, sw),
                        "vk={vk:#x} down={down} sw={sw}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_row_follows_the_toggle_and_the_install() {
        for failed in [false, true] {
            assert_eq!(prtsc_row(false, failed), RowState::Off);
        }
        assert_eq!(prtsc_row(true, false), RowState::Active);
        assert_eq!(prtsc_row(true, true), RowState::Failed);
        assert_eq!(ui_row(RowState::Off), PrtscRow::Plain);
        assert_eq!(ui_row(RowState::Active), PrtscRow::Plain);
        assert_eq!(ui_row(RowState::Failed), PrtscRow::Failed);
        let mut s = PrtscState::default();
        assert_eq!(s.row(true), RowState::Active);
        s.failed = true;
        assert_eq!(s.row(true), RowState::Failed);
        assert_eq!(s.row(false), RowState::Off);
        assert!(!s.installed());
    }

    #[test]
    fn a_config_save_touches_the_hook_only_when_the_setting_moved() {
        // Wanted and installed, or wanted and failed: an unrelated save (or
        // the toggle's own save, already followed) neither installs again nor
        // stops anything. A failed hook waits for `retry_hotkeys`.
        assert!(!follows_config(true, true, false));
        assert!(!follows_config(true, false, true));
        // Turned on (never tried, or removed): one try.
        assert!(follows_config(true, false, false));
        // Turned off while installed: removed; while failed: the failure clears.
        assert!(follows_config(false, true, false));
        assert!(follows_config(false, false, true));
        // Off and nothing installed: nothing.
        assert!(!follows_config(false, false, false));
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

    #[test]
    fn the_hook_is_planned_from_the_setting_and_what_is_installed() {
        // Off: nothing installed, nothing to do; installed: remove.
        assert_eq!(plan(false, false), Plan::Nothing);
        assert_eq!(plan(false, true), Plan::Uninstall);
        // On: not installed (never tried, failed, or removed): install, which
        // is also the retry of a failed one; installed: leave.
        assert_eq!(plan(true, false), Plan::Install);
        assert_eq!(plan(true, true), Plan::Nothing);
    }

    #[test]
    fn a_retry_happens_only_for_a_failed_hook_that_is_still_wanted() {
        // `retry_hotkeys` calls the sync only while `failed`; the plan then
        // installs only while wanted, and `follows_config` stays quiet.
        let (want, installed, failed) = (true, false, true);
        assert!(!follows_config(want, installed, failed));
        assert_eq!(plan(want, installed), Plan::Install);
        // Wanted off meanwhile: no retry install.
        assert_eq!(plan(false, installed), Plan::Nothing);
    }
}
