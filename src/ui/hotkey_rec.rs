//! The recorder of Settings -> Tombol pintas: a low-level keyboard hook on its
//! own thread (`vale-hotkey-rec`) that exists only while a field is recording.
//!
//! While it runs it swallows every key it sees go down, so Win combos, PrtSc,
//! Alt+Tab and the like reach neither Windows nor another app. It tracks the
//! modifiers itself from the events it sees (`GetAsyncKeyState` does not know
//! about swallowed keys): a set of 256 keys whose key-down it swallowed
//! (`KeySet`). A key that was already down when the hook went in (held while
//! the user clicked the field) still sends auto-repeat key-downs; Windows saw
//! that key go down, so the recorder passes its repeats and its key-up and
//! only counts it as a modifier (it tells such a key by `GetAsyncKeyState`,
//! which swallowed keys never change). On the first key that is not a bare
//! modifier it posts the vk and the modifiers once (`WM_APP_HOTKEY_REC` to
//! the Settings window; the decision is `hotkey::record_step`, shared with
//! the window's fallback), then
//! it drains: only the key-ups of keys in the set (and the auto-repeat of
//! those keys) are still swallowed, everything else passes. With the set
//! empty it unhooks and the thread ends, so the hook is never left installed.
//! The thread's end is posted to the window (`WM_APP_HOTKEY_REC_DONE`): only
//! then does the island get its hotkeys back, so a key still held from the
//! recording (and its auto-repeat) never reaches the island's PrtSc hook.
//!
//! Two timers on the hook thread keep that true: draining ends after
//! `DRAIN_MS` at most, and a recording that got no combo in `LISTEN_MS` posts
//! a cancel (vk 0) and drains.
//!
//! The proc allocates nothing, takes no lock and does not log: it reads and
//! writes a few atomics (only this thread touches them while the hook runs).
//! The pure part (`RecState`) is tested; the thread is the same shape as
//! `app_prtsc`'s.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, KillTimer, MSG, PM_NOREMOVE, PeekMessageW,
    PostMessageW, PostQuitMessage, PostThreadMessageW, SetTimer, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_TIMER,
};

use crate::hotkey::{Mods, Step, record_step};

/// Posted to the Settings window by the hook: `wparam` is the vk of the key
/// that ended the listening (0: it gave up waiting), `lparam` the modifier
/// bits (`Mods::bits`) held with it.
pub const WM_APP_HOTKEY_REC: u32 = WM_APP + 13;

/// Posted to the Settings window when the hook thread has ended (drained,
/// timed out or stopped): the recorder holds no key any more, so the island
/// can take its hotkeys back.
pub const WM_APP_HOTKEY_REC_DONE: u32 = WM_APP + 14;

/// A recording that gets no combo in this long is cancelled.
const LISTEN_MS: u32 = 20_000;
/// Draining (swallowing the key-ups of what was pressed) ends after this long
/// at most.
const DRAIN_MS: u32 = 2_000;

/// Every modifier key code (Shift, Ctrl, Alt and their left and right keys,
/// both Win keys).
const MODIFIER_VKS: [u16; 11] = [
    0x10, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C,
];

/// The keys whose key-down the recorder swallowed and whose key-up it has not
/// seen: 256 bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeySet([u64; 4]);

impl KeySet {
    fn slot(vk: u16) -> (usize, u64) {
        let vk = usize::from(vk & 0xFF);
        (vk / 64, 1u64 << (vk % 64))
    }

    /// Adds `vk`; true when it was not in the set.
    pub fn insert(&mut self, vk: u16) -> bool {
        let (i, bit) = Self::slot(vk);
        let was = self.0[i] & bit != 0;
        self.0[i] |= bit;
        !was
    }

    /// Takes `vk` out; true when it was in.
    pub fn remove(&mut self, vk: u16) -> bool {
        let (i, bit) = Self::slot(vk);
        let was = self.0[i] & bit != 0;
        self.0[i] &= !bit;
        was
    }

    pub fn contains(&self, vk: u16) -> bool {
        let (i, bit) = Self::slot(vk);
        self.0[i] & bit != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }

    /// The modifiers among the keys held (left and right alike).
    pub fn mods(&self) -> Mods {
        let mut bits = 0u8;
        for vk in MODIFIER_VKS {
            if self.contains(vk)
                && let Some(bit) = Mods::bit_of_vk(vk)
            {
                bits |= bit;
            }
        }
        Mods::from_bits(bits)
    }
}

/// What the proc does with one key event, and what it tells the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecEvent {
    /// On to Windows and the other hooks (otherwise swallowed).
    pub pass: bool,
    /// The vk and modifiers to post: set once per recording.
    pub post: Option<(u16, Mods)>,
    /// Draining is over: unhook and end the thread.
    pub done: bool,
}

const SWALLOW: RecEvent = RecEvent {
    pass: false,
    post: None,
    done: false,
};
const PASS: RecEvent = RecEvent {
    pass: true,
    post: None,
    done: false,
};

/// The recorder's bookkeeping: the keys held and whether it already posted
/// (and so only drains now). Pure; the proc keeps it in atomics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecState {
    /// The keys whose key-down it swallowed.
    pub keys: KeySet,
    /// The keys Windows already had down when the recorder first saw them:
    /// passed, never swallowed, but their modifiers count.
    pub before: KeySet,
    pub draining: bool,
}

impl RecState {
    /// One key event, as the hook saw it; `os_down`: Windows has the key
    /// down already (read before this event counts, so only a key it saw go
    /// down earlier, held from before the recording). Listening: such a key's
    /// repeats pass and it goes in `before`, where its modifier counts and
    /// nothing else happens. Every other key-down is swallowed and enters the
    /// set (auto-repeat of a key in the set does nothing more); the first one
    /// `record_step` does not ignore is posted and starts the drain. A key-up
    /// is swallowed only when its key is in the set, otherwise it passes
    /// (Windows saw that key go down: swallowing it would leave it held).
    /// Draining: only the key-ups of keys in the set, and their auto-repeat,
    /// are swallowed; the last one out says `done`.
    pub fn event(&mut self, vk: u16, down: bool, os_down: bool) -> RecEvent {
        let vk = vk & 0xFF;
        if down {
            let held = self.keys.contains(vk);
            if self.draining {
                return if held { SWALLOW } else { PASS };
            }
            if !held && os_down {
                self.before.insert(vk);
                return PASS;
            }
            self.keys.insert(vk);
            if held {
                return SWALLOW;
            }
            let mods = Mods::from_bits(self.keys.mods().bits() | self.before.mods().bits());
            match record_step(vk, mods) {
                Step::Ignore => SWALLOW,
                _ => {
                    self.draining = true;
                    RecEvent {
                        pass: false,
                        post: Some((vk, mods)),
                        done: false,
                    }
                }
            }
        } else if self.keys.remove(vk) {
            RecEvent {
                pass: false,
                post: None,
                done: self.draining && self.keys.is_empty(),
            }
        } else {
            self.before.remove(vk);
            PASS
        }
    }

    /// The listening ran out of time: post a cancel (vk 0) once and drain. A
    /// recording that already posted does nothing.
    pub fn give_up(&mut self) -> RecEvent {
        if self.draining {
            return RecEvent {
                done: false,
                ..SWALLOW
            };
        }
        self.draining = true;
        RecEvent {
            pass: false,
            post: Some((0, Mods::default())),
            done: self.keys.is_empty(),
        }
    }
}

// ---- The hook's shared state (touched only by the hook thread while it runs).

static KEYS: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static BEFORE: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static DRAINING: AtomicBool = AtomicBool::new(false);
/// The Settings window the proc posts to (0: none).
static TARGET: AtomicIsize = AtomicIsize::new(0);
/// The thread timers' ids (0: none).
static LISTEN_TIMER: AtomicUsize = AtomicUsize::new(0);
static DRAIN_TIMER: AtomicUsize = AtomicUsize::new(0);

fn load() -> RecState {
    RecState {
        keys: KeySet(std::array::from_fn(|i| KEYS[i].load(Ordering::Relaxed))),
        before: KeySet(std::array::from_fn(|i| BEFORE[i].load(Ordering::Relaxed))),
        draining: DRAINING.load(Ordering::Relaxed),
    }
}

fn store(s: &RecState) {
    for (slot, bits) in KEYS.iter().zip(s.keys.0) {
        slot.store(bits, Ordering::Relaxed);
    }
    for (slot, bits) in BEFORE.iter().zip(s.before.0) {
        slot.store(bits, Ordering::Relaxed);
    }
    DRAINING.store(s.draining, Ordering::Relaxed);
}

fn reset() {
    store(&RecState::default());
    LISTEN_TIMER.store(0, Ordering::Relaxed);
    DRAIN_TIMER.store(0, Ordering::Relaxed);
}

/// Tells the Settings window.
fn post((vk, mods): (u16, Mods)) {
    let hwnd = TARGET.load(Ordering::Relaxed);
    if hwnd != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd as *mut _)),
                WM_APP_HOTKEY_REC,
                WPARAM(usize::from(vk)),
                LPARAM(isize::from(mods.bits())),
            );
        }
    }
}

/// Starts the drain's time limit (a thread timer: it reaches this thread's
/// message loop).
fn start_drain_timer() {
    let id = unsafe { SetTimer(None, 0, DRAIN_MS, None) };
    DRAIN_TIMER.store(id, Ordering::Relaxed);
}

/// The hook proc: for every key in the system, while a recording runs.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // HC_ACTION (0): a real key message; anything else must be passed on.
    if code == 0 {
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);
        if down || up {
            let vk = kb.vkCode as u16;
            // A low-level hook runs before the key's async state is updated,
            // and a swallowed key never updates it: down here means Windows
            // saw this key go down before the recording.
            let os_down = down && unsafe { GetAsyncKeyState(i32::from(vk & 0xFF)) } < 0;
            let mut state = load();
            let ev = state.event(vk, down, os_down);
            store(&state);
            if let Some(p) = ev.post {
                post(p);
                start_drain_timer();
            }
            if ev.done {
                unsafe { PostQuitMessage(0) };
            }
            if !ev.pass {
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The running recorder.
pub struct RecHook {
    /// The hook thread's id while it lives (0 once it has ended, so a stop
    /// after the drain posts to no one).
    thread_id: Arc<AtomicU32>,
    handle: JoinHandle<()>,
}

impl RecHook {
    /// Starts the thread and waits once for it to say whether the hook went
    /// in. `None`: it could not (the window then records from its own key
    /// messages).
    pub fn start(hwnd: HWND) -> Option<RecHook> {
        reset();
        TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
        let thread_id = Arc::new(AtomicU32::new(0));
        let (tx, rx) = mpsc::channel::<bool>();
        let alive = Arc::clone(&thread_id);
        let handle = std::thread::Builder::new()
            .name("vale-hotkey-rec".into())
            .spawn(move || hook_thread_main(tx, alive))
            .ok();
        let started = handle.and_then(|handle| match rx.recv() {
            Ok(true) => Some(RecHook { thread_id, handle }),
            _ => {
                let _ = handle.join();
                None
            }
        });
        if started.is_none() {
            TARGET.store(0, Ordering::Relaxed);
        }
        started
    }

    /// Whether the hook thread still runs (listening or draining).
    pub fn running(&self) -> bool {
        self.thread_id.load(Ordering::Relaxed) != 0
    }

    /// Ends the thread now (`WM_QUIT`; nothing if it already ended) and waits
    /// for it.
    pub fn stop(self) {
        let id = self.thread_id.load(Ordering::Relaxed);
        if id != 0 {
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        let _ = self.handle.join();
        TARGET.store(0, Ordering::Relaxed);
    }
}

/// The hook thread: install, report, then the message loop (where the hook is
/// called and the two timers arrive) until `WM_QUIT`, the drain's end or a
/// time limit.
fn hook_thread_main(tx: mpsc::Sender<bool>, alive: Arc<AtomicU32>) {
    let hook = unsafe {
        let module = GetModuleHandleW(None).ok();
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module.map(Into::into), 0)
    };
    let Ok(hook) = hook else {
        let _ = tx.send(false);
        return;
    };
    // Make the thread's message queue exist before `stop` can post to it.
    let mut msg = MSG::default();
    unsafe {
        let _ = PeekMessageW(&mut msg, None, WM_QUIT, WM_QUIT, PM_NOREMOVE);
        LISTEN_TIMER.store(SetTimer(None, 0, LISTEN_MS, None), Ordering::Relaxed);
    }
    alive.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    let _ = tx.send(true);
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        if msg.message != WM_TIMER {
            continue;
        }
        let id = msg.wParam.0;
        if id != 0 && id == LISTEN_TIMER.load(Ordering::Relaxed) {
            unsafe {
                let _ = KillTimer(None, id);
            }
            LISTEN_TIMER.store(0, Ordering::Relaxed);
            let mut state = load();
            let ev = state.give_up();
            store(&state);
            if let Some(p) = ev.post {
                post(p);
                start_drain_timer();
            }
            if ev.done {
                break;
            }
        } else if id != 0 && id == DRAIN_TIMER.load(Ordering::Relaxed) {
            break;
        }
    }
    for timer in [&LISTEN_TIMER, &DRAIN_TIMER] {
        let id = timer.swap(0, Ordering::Relaxed);
        if id != 0 {
            unsafe {
                let _ = KillTimer(None, id);
            }
        }
    }
    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
    // Not running from here on, then the window hears it: a window that
    // checks `running` after this store needs no message, one that checked
    // before gets it.
    alive.store(0, Ordering::Relaxed);
    let hwnd = TARGET.load(Ordering::Relaxed);
    if hwnd != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd as *mut _)),
                WM_APP_HOTKEY_REC_DONE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::is_modifier_vk;

    /// The hook's statics are shared: the tests that touch them take turns.
    static STATICS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn the_modifier_list_is_every_modifier_vk() {
        for vk in 0..=0xFFu16 {
            assert_eq!(MODIFIER_VKS.contains(&vk), is_modifier_vk(vk), "{vk:#x}");
        }
    }

    const CTRL: u16 = 0xA2;
    const LALT: u16 = 0xA4;
    const LWIN: u16 = 0x5B;
    const SHIFT: u16 = 0xA0;
    const V: u16 = 0x56;
    const ESC: u16 = 0x1B;

    #[test]
    fn the_key_set_holds_256_keys_and_derives_the_modifiers() {
        let mut s = KeySet::default();
        assert!(s.is_empty());
        for vk in 0..=255u16 {
            assert!(s.insert(vk), "{vk}");
            assert!(!s.insert(vk), "{vk} twice");
        }
        for vk in 0..=255u16 {
            assert!(s.contains(vk));
        }
        for vk in 0..=255u16 {
            assert!(s.remove(vk), "{vk}");
            assert!(!s.remove(vk));
        }
        assert!(s.is_empty());
        // Modifiers, left and right.
        let mut s = KeySet::default();
        s.insert(0xA3);
        s.insert(0xA5);
        s.insert(0x5C);
        assert_eq!(
            s.mods(),
            Mods {
                ctrl: true,
                alt: true,
                shift: false,
                win: true
            }
        );
        s.remove(0xA3);
        assert!(!s.mods().ctrl);
        // A plain key is not a modifier.
        s.insert(V);
        assert!(s.contains(V));
        assert_eq!(s.mods().bits(), 1 | 8);
    }

    #[test]
    fn a_combo_is_posted_once_with_the_modifiers_the_recorder_tracked_itself() {
        let mut r = RecState::default();
        // Modifiers go down: swallowed, nothing posted yet.
        for vk in [CTRL, LALT] {
            assert_eq!(r.event(vk, true, false), SWALLOW, "{vk:#x}");
        }
        assert!(!r.draining);
        // The key: posted with Ctrl+Alt, and the drain starts.
        let ev = r.event(V, true, false);
        assert!(!ev.pass && !ev.done);
        let (vk, mods) = ev.post.expect("posted");
        assert_eq!(vk, V);
        assert_eq!(mods.bits(), 2 | 1);
        assert!(r.draining);
        // Auto-repeat of the key (and of a held modifier): swallowed, no second post.
        for vk in [V, V, CTRL] {
            assert_eq!(r.event(vk, true, false), SWALLOW);
        }
    }

    #[test]
    fn draining_swallows_only_the_key_ups_of_what_was_held_and_ends_when_none_is() {
        let mut r = RecState::default();
        r.event(LWIN, true, false);
        r.event(V, true, false);
        assert!(r.draining);
        // Other keys pass while draining, down and up.
        assert_eq!(r.event(0x41, true, false), PASS);
        assert_eq!(r.event(0x41, false, false), PASS);
        // The key-ups of the held ones are swallowed; the last says done.
        let first = r.event(V, false, false);
        assert!(!first.pass && !first.done);
        let last = r.event(LWIN, false, false);
        assert!(!last.pass && last.done);
        // After that everything passes.
        assert_eq!(r.event(LWIN, false, false), PASS);
        assert_eq!(r.event(V, true, false), PASS);
    }

    #[test]
    fn a_key_up_of_a_key_never_seen_down_passes() {
        let mut r = RecState::default();
        // Held before the recording began: Windows saw its key-down.
        assert_eq!(r.event(CTRL, false, false), PASS);
        assert_eq!(r.event(V, false, false), PASS);
        assert!(!r.draining);
    }

    #[test]
    fn a_key_held_before_the_recording_is_passed_but_its_modifier_counts() {
        // Ctrl held while the field was clicked: Windows saw it go down, and
        // its auto-repeat reaches the hook. The repeats and the key-up pass
        // (swallowing the key-up would leave Ctrl held for every app).
        let mut r = RecState::default();
        assert_eq!(r.event(CTRL, true, true), PASS);
        assert_eq!(r.event(CTRL, true, true), PASS);
        assert!(r.keys.is_empty() && !r.draining);
        // A key pressed now is posted with Ctrl, and only it is drained.
        let ev = r.event(V, true, false);
        assert_eq!(ev.post.map(|(v, m)| (v, m.bits())), Some((V, 2)));
        assert!(!ev.pass && r.draining);
        assert_eq!(r.event(CTRL, true, true), PASS);
        let up = r.event(V, false, false);
        assert!(!up.pass && up.done);
        assert_eq!(r.event(CTRL, false, false), PASS);

        // Let go of before a key: it no longer counts, the next press of it
        // is the recorder's.
        let mut r = RecState::default();
        assert_eq!(r.event(LALT, true, true), PASS);
        assert_eq!(r.event(LALT, false, false), PASS);
        assert!(r.before.is_empty());
        assert_eq!(r.event(LALT, true, false), SWALLOW);
        assert!(r.keys.contains(LALT));
        let ev = r.event(V, true, false);
        assert_eq!(ev.post.map(|(v, m)| (v, m.bits())), Some((V, 1)));

        // A plain key held from before: passed, never posted.
        let mut r = RecState::default();
        assert_eq!(r.event(V, true, true), PASS);
        assert!(!r.draining);
        assert_eq!(r.event(V, false, false), PASS);
    }

    #[test]
    fn a_modifier_pressed_and_released_alone_keeps_listening() {
        let mut r = RecState::default();
        assert_eq!(r.event(SHIFT, true, false), SWALLOW);
        let up = r.event(SHIFT, false, false);
        assert!(!up.pass && !up.done && up.post.is_none());
        assert!(!r.draining && r.keys.is_empty());
        // Then a combo still works.
        r.event(CTRL, true, false);
        assert!(r.event(V, true, false).post.is_some());
    }

    #[test]
    fn esc_and_backspace_and_delete_are_posted_like_any_key() {
        for vk in [ESC, 0x08, 0x2E] {
            let mut r = RecState::default();
            let ev = r.event(vk, true, false);
            assert_eq!(ev.post, Some((vk, Mods::default())), "{vk:#x}");
            assert!(r.draining);
            // The key-up ends it.
            let up = r.event(vk, false, false);
            assert!(!up.pass && up.done);
        }
        // With a modifier they are combos: posted with it.
        let mut r = RecState::default();
        r.event(CTRL, true, false);
        let ev = r.event(ESC, true, false);
        assert_eq!(ev.post.map(|(v, m)| (v, m.bits())), Some((ESC, 2)));
    }

    #[test]
    fn giving_up_posts_a_cancel_once_and_drains() {
        // Nothing held: done at once.
        let mut r = RecState::default();
        let ev = r.give_up();
        assert_eq!(ev.post, Some((0, Mods::default())));
        assert!(ev.done && !ev.pass);
        assert!(r.draining);
        // Keys held (a modifier): the drain waits for their key-ups.
        let mut r = RecState::default();
        r.event(CTRL, true, false);
        let ev = r.give_up();
        assert!(ev.post.is_some() && !ev.done);
        assert!(r.event(CTRL, false, false).done);
        // After a combo was posted it does nothing.
        let mut r = RecState::default();
        r.event(V, true, false);
        let ev = r.give_up();
        assert_eq!(ev.post, None);
        assert!(!ev.done);
        // And a repeat does not post again.
        let mut r = RecState::default();
        r.give_up();
        assert_eq!(r.give_up().post, None);
    }

    #[test]
    fn the_state_survives_the_atomics() {
        let _turn = STATICS.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        let mut s = RecState::default();
        s.keys.insert(CTRL);
        s.keys.insert(0xFE);
        s.before.insert(0xA3);
        s.draining = true;
        store(&s);
        assert_eq!(load(), s);
        reset();
        assert_eq!(load(), RecState::default());
    }

    #[test]
    fn the_recorder_thread_starts_and_stops_cleanly_with_no_window() {
        let _turn = STATICS.lock().unwrap_or_else(|e| e.into_inner());
        // No window: a combo would post nowhere. A machine that refuses the
        // hook just reports None.
        if let Some(hook) = RecHook::start(HWND::default()) {
            hook.stop();
        }
        assert_eq!(TARGET.load(Ordering::Relaxed), 0);
        // Twice in a row: the statics start clean each time.
        if let Some(hook) = RecHook::start(HWND::default()) {
            assert_eq!(load(), RecState::default());
            hook.stop();
        }
    }
}
