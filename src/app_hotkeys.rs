//! The island's side of the global hotkeys (`crate::hotkey`): which combo
//! each row has in effect, registering it with Windows, the PrtSc hook for the
//! combos Windows keeps (`app_prtsc`), the retry of one that is taken, the
//! pause while Settings records a new combo, and what a press does. A child of
//! `app`, so it works on `App` directly.
//!
//! One `sync_hotkeys` brings every row to the config's value: every row that
//! differs from what is registered, or that failed, is unregistered first,
//! then each is registered again (`RegisterHotKey`, or an entry in the hook's
//! table for a PrtSc combo), so two rows can swap combos. The hook thread
//! runs only while some row needs it.
//!
//! A row Windows or another app holds shows red in Settings and is tried again
//! when the tray menu opens or Settings opens (`retry_hotkeys`).

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VK_ESCAPE,
};

use super::App;
use super::prtsc::{self, HookThread};
use crate::hotkey::{self, Hotkey, HotkeyId, Hotkeys};
use crate::model::Event;
use crate::recorder::Source;

/// `RegisterHotKey` ids, one per row.
pub(super) const HOTKEY_CLIP: i32 = 1;
pub(super) const HOTKEY_SEARCH: i32 = 2;
pub(super) const HOTKEY_DEVICES: i32 = 3;
pub(super) const HOTKEY_ARRANGE: i32 = 4;
pub(super) const HOTKEY_SCREENSHOT: i32 = 5;
pub(super) const HOTKEY_LYRICS_CARD: i32 = 6;

/// How many rows there are (the size of the per-row tables).
pub(super) const ROWS: usize = HotkeyId::ALL.len();
/// Id 6 is the lyrics card's (another branch); the sticky note takes 7.
pub(super) const HOTKEY_STICKY: i32 = 7;
/// The sticky note list takes 8.
pub(super) const HOTKEY_STICKY_LIST: i32 = 8;
/// The audio recorder takes 9.
pub(super) const HOTKEY_RECORD: i32 = 9;
/// Mute the recording's mic takes 12 (10 and 11 are the screen recorder's).
pub(super) const HOTKEY_RECORD_MUTE: i32 = 12;
/// The screen recorder takes 10.
pub(super) const HOTKEY_RECORD_SCREEN: i32 = 10;
/// The screen recorder's video zoom takes 11.
pub(super) const HOTKEY_RECORD_ZOOM: i32 = 11;
/// Live captions take 14 (13 is the countdown's Esc, which is no row).
pub(super) const HOTKEY_CAPTIONS: i32 = 14;
/// Esc during the 3-2-1 before a screen recording. Not a row: a raw
/// registration that exists only while the countdown runs.
pub(super) const HOTKEY_COUNTDOWN_ESC: i32 = 13;

/// The `RegisterHotKey` id of a row.
pub(super) fn wm_id(id: HotkeyId) -> i32 {
    match id {
        HotkeyId::Clipboard => HOTKEY_CLIP,
        HotkeyId::Search => HOTKEY_SEARCH,
        HotkeyId::Devices => HOTKEY_DEVICES,
        HotkeyId::Arrange => HOTKEY_ARRANGE,
        HotkeyId::Screenshot => HOTKEY_SCREENSHOT,
        HotkeyId::LyricsCard => HOTKEY_LYRICS_CARD,
        HotkeyId::StickyNote => HOTKEY_STICKY,
        HotkeyId::StickyList => HOTKEY_STICKY_LIST,
        HotkeyId::Record => HOTKEY_RECORD,
        HotkeyId::RecordMute => HOTKEY_RECORD_MUTE,
        HotkeyId::RecordScreen => HOTKEY_RECORD_SCREEN,
        HotkeyId::RecordZoom => HOTKEY_RECORD_ZOOM,
        HotkeyId::Captions => HOTKEY_CAPTIONS,
    }
}

/// Whether a `WM_HOTKEY` id is the countdown's Esc.
pub(super) fn is_countdown_esc(wm: i32) -> bool {
    wm == HOTKEY_COUNTDOWN_ESC
}

/// Whether a recording runs, for `wanted`: the mic mute works on a screen
/// recording as on an audio one.
pub(super) fn any_recording(audio: bool, screen: bool) -> bool {
    audio || screen
}

/// The row a `WM_HOTKEY` id belongs to.
pub(super) fn row_of_wm_id(wm: i32) -> Option<HotkeyId> {
    HotkeyId::ALL.into_iter().find(|id| wm_id(*id) == wm)
}

/// What a sync does to one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowPlan {
    /// Registered as wanted and not failed: nothing.
    Keep,
    /// Wanted off: unregister whatever it had.
    Clear,
    /// Register the combo with Windows.
    Register(Hotkey),
    /// Put the PrtSc combo in the hook's table.
    Hook(Hotkey),
}

/// The step for one row from what is wanted, what is in effect and whether it
/// failed last time. Pure.
pub(super) fn plan_row(want: Option<Hotkey>, applied: Option<Hotkey>, failed: bool) -> RowPlan {
    if want == applied && !failed {
        return RowPlan::Keep;
    }
    match want {
        None => RowPlan::Clear,
        Some(hk) if hotkey::uses_hook(hk) => RowPlan::Hook(hk),
        Some(hk) => RowPlan::Register(hk),
    }
}

/// The combos the rows want now: the config's, except that a row that only
/// makes sense while a recording runs (the mic mute) or while a screen
/// recording runs (the video zoom) is off otherwise, so its combo is not
/// taken from other apps at idle. Pure.
pub(super) fn wanted(config: &Hotkeys, recording: bool, screen: bool) -> Hotkeys {
    let mut want = *config;
    if !recording {
        want.set(HotkeyId::RecordMute, None);
    }
    if !screen {
        want.set(HotkeyId::RecordZoom, None);
    }
    want
}

/// The video is zoomed but the zoom row has no combo any more (switched off
/// in Settings): the zoom has to go out, or nothing could undo it. Pure.
pub(super) fn zoom_out_needed(row: Option<Hotkey>, zoomed: bool) -> bool {
    zoomed && row.is_none()
}

/// What the island keeps about the hotkeys.
#[derive(Default)]
pub(super) struct HotkeyState {
    /// What each row has in effect (registered or in the hook's table).
    applied: [Option<Hotkey>; ROWS],
    /// A row whose combo could not be registered or hooked.
    failed: [bool; ROWS],
    /// The PrtSc hook thread, while a row needs it.
    hook: Option<HookThread>,
    /// Settings is recording: nothing is registered and no sync does anything.
    paused: bool,
    /// The first sync ran (before it a config read leaves the hotkeys alone).
    started: bool,
}

impl HotkeyState {
    /// Whether any row's combo in effect needs the PrtSc hook.
    fn needs_hook(&self) -> bool {
        self.applied
            .iter()
            .flatten()
            .any(|hk| hotkey::uses_hook(*hk))
    }

    /// The rows in effect that are failed, by row index.
    pub(super) fn failed_flags(&self) -> [bool; ROWS] {
        self.failed
    }

    /// Whether a row is failed.
    pub(super) fn any_failed(&self) -> bool {
        self.failed.iter().any(|f| *f)
    }
}

/// What a sync asks of Windows and the PrtSc hook's table; tests use a fake
/// that refuses a combo another id holds, as `RegisterHotKey` does.
pub(super) trait HotkeyOps {
    /// Lets go of whatever the row had: its `RegisterHotKey` id and its entry
    /// in the hook's table.
    fn release(&mut self, id: HotkeyId);
    /// Registers the combo under the row's id; false when it is taken.
    fn register(&mut self, id: HotkeyId, hk: Hotkey) -> bool;
    /// Puts the PrtSc combo in the hook's table.
    fn hook(&mut self, id: HotkeyId, hk: Hotkey);
}

/// The real thing, on the island's window.
struct WinOps(HWND);

impl HotkeyOps for WinOps {
    fn release(&mut self, id: HotkeyId) {
        unsafe {
            let _ = UnregisterHotKey(Some(self.0), wm_id(id));
        }
        prtsc::table_clear(id);
    }

    fn register(&mut self, id: HotkeyId, hk: Hotkey) -> bool {
        unsafe {
            RegisterHotKey(
                Some(self.0),
                wm_id(id),
                HOT_KEY_MODIFIERS(hotkey::register_mods(hk)),
                u32::from(hotkey::register_vk(hk)),
            )
        }
        .is_ok()
    }

    fn hook(&mut self, id: HotkeyId, hk: Hotkey) {
        prtsc::table_set(id, hk.mods);
    }
}

impl HotkeyState {
    /// Brings every row to `want`, in two passes: first every row that
    /// changes lets go of what it had, then the new combos go in. One pass
    /// would fail when rows swap combos: `RegisterHotKey` refuses a combo
    /// that is still registered, even under another id of the same window,
    /// and the row that holds it may come later.
    fn apply(&mut self, want: &Hotkeys, ops: &mut impl HotkeyOps) {
        let plans = HotkeyId::ALL.map(|id| {
            let i = id.index();
            (id, plan_row(want.get(id), self.applied[i], self.failed[i]))
        });
        for (id, plan) in plans {
            if plan != RowPlan::Keep {
                ops.release(id);
            }
        }
        for (id, plan) in plans {
            let i = id.index();
            if plan == RowPlan::Keep {
                continue;
            }
            self.applied[i] = want.get(id);
            self.failed[i] = match plan {
                RowPlan::Keep | RowPlan::Clear => false,
                RowPlan::Hook(hk) => {
                    ops.hook(id, hk);
                    false
                }
                RowPlan::Register(hk) => !ops.register(id, hk),
            };
        }
    }
}

impl App {
    /// Brings every row to the config's value; one that failed is tried
    /// again. Does nothing while Settings records.
    pub(super) fn sync_hotkeys(&mut self) {
        self.hotkeys.started = true;
        if self.hotkeys.paused {
            return;
        }
        let recording = any_recording(self.rec.active(), self.screen.recording());
        let want = wanted(&self.config.hotkeys, recording, self.screen.recording());
        self.hotkeys.apply(&want, &mut WinOps(self.hwnd));
        self.sync_hotkey_hook();
        if zoom_out_needed(self.config.hotkeys.get(HotkeyId::RecordZoom), self.screen_zoomed()) {
            self.screen_toggle_zoom();
        }
    }

    /// Starts the PrtSc hook thread when a row needs it and none runs, stops
    /// it when none does. A failed install marks every PrtSc row failed.
    fn sync_hotkey_hook(&mut self) {
        let need = self.hotkeys.needs_hook();
        if need && self.hotkeys.hook.is_none() {
            self.hotkeys.hook = HookThread::start(self.hwnd);
            if self.hotkeys.hook.is_none() {
                for id in HotkeyId::ALL {
                    if self.hotkeys.applied[id.index()].is_some_and(hotkey::uses_hook) {
                        self.hotkeys.failed[id.index()] = true;
                    }
                }
            }
        } else if !need && let Some(hook) = self.hotkeys.hook.take() {
            hook.stop();
        }
    }

    /// Tries every failed row again (the tray menu or Settings opening); a
    /// row that works is left alone.
    pub(super) fn retry_hotkeys(&mut self) {
        if self.hotkeys.any_failed() {
            self.sync_hotkeys();
        }
    }

    /// Settings starts recording a combo: every registration and the hook go,
    /// so a combo typed meanwhile reaches the recorder and fires nothing.
    /// Nothing is marked failed.
    pub(super) fn pause_hotkeys(&mut self) {
        if self.hotkeys.paused {
            return;
        }
        self.hotkeys.paused = true;
        self.unregister_all_hotkeys();
    }

    /// Recording is over: back to the config, with a full sync.
    pub(super) fn resume_hotkeys(&mut self) {
        if !self.hotkeys.paused {
            return;
        }
        self.hotkeys.paused = false;
        self.sync_hotkeys();
        // The pause took the countdown's Esc too: back while it still counts.
        if self.screen.wants_esc() {
            self.register_countdown_esc();
        }
    }

    /// Unregisters every row's id and the countdown's Esc, empties the hook's
    /// table and stops the hook; every row reads as not in effect.
    fn unregister_all_hotkeys(&mut self) {
        self.unregister_countdown_esc();
        for id in HotkeyId::ALL {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), wm_id(id));
            }
            prtsc::table_clear(id);
        }
        self.hotkeys.applied = [None; ROWS];
        if let Some(hook) = self.hotkeys.hook.take() {
            hook.stop();
        }
    }

    /// `WM_DESTROY`: the hotkeys and the hook go.
    pub(super) fn hotkeys_shutdown(&mut self) {
        self.unregister_all_hotkeys();
        self.hotkeys.started = false;
    }

    /// A config save or read: the rows follow it once the first sync ran.
    pub(super) fn hotkeys_follow_config(&mut self) {
        if self.hotkeys.started {
            self.sync_hotkeys();
        }
    }

    /// Settings sets (or clears) one row. The combo is checked again here and
    /// refused if it is reserved, lacks a modifier or another row has it.
    pub(super) fn set_hotkey(&mut self, id: HotkeyId, value: Option<Hotkey>) {
        if let Some(hk) = value
            && (hotkey::check(hk).is_err()
                || hotkey::used_by(&self.config.hotkeys, id, hk).is_some())
        {
            self.sync_settings();
            return;
        }
        self.config.hotkeys.set(id, value);
        self.save_config();
        self.sync_hotkeys();
        self.sync_settings();
    }

    /// Settings: every row back to its default.
    pub(super) fn reset_all_hotkeys(&mut self) {
        self.config.hotkeys.reset_all();
        self.save_config();
        self.sync_hotkeys();
        self.sync_settings();
    }

    /// `WM_HOTKEY` or the PrtSc hook: what the row does.
    pub(super) fn on_hotkey(&mut self, id: HotkeyId) {
        match id {
            HotkeyId::Clipboard => self.on_clip_hotkey(),
            HotkeyId::Search => self.on_search_hotkey(),
            HotkeyId::Devices => self.on_devices_hotkey(),
            HotkeyId::Arrange => self.on_arrange_hotkey(),
            HotkeyId::Screenshot => self.on_screenshot_hotkey(),
            HotkeyId::LyricsCard => self.on_lyrics_card_hotkey(),
            HotkeyId::StickyNote => self.on_sticky_hotkey(),
            HotkeyId::StickyList => self.open_note_list(None),
            HotkeyId::Record => self.rec_toggle(),
            HotkeyId::RecordMute => self.rec_toggle_mute(Source::Mic),
            HotkeyId::RecordScreen => self.screen_toggle(),
            HotkeyId::RecordZoom => self.screen_toggle_zoom(),
            HotkeyId::Captions => self.captions_toggle(),
        }
    }

    /// The 3-2-1 begins: Esc belongs to Vale until `unregister_countdown_esc`,
    /// whichever app has the keyboard. False when another app holds a bare
    /// Esc hotkey (the screen hotkey still cancels).
    pub(super) fn register_countdown_esc(&mut self) -> bool {
        unsafe {
            RegisterHotKey(
                Some(self.hwnd),
                HOTKEY_COUNTDOWN_ESC,
                MOD_NOREPEAT,
                u32::from(VK_ESCAPE.0),
            )
        }
        .is_ok()
    }

    /// The 3-2-1 is over or cancelled: Esc goes back to the other apps.
    pub(super) fn unregister_countdown_esc(&mut self) {
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_COUNTDOWN_ESC);
        }
    }

    /// The PrtSc hook's message: the row is in `wparam`.
    pub(super) fn on_hook_row(&mut self, row: usize) {
        if let Some(id) = HotkeyId::from_index(row) {
            self.on_hotkey(id);
        }
    }

    /// The screenshot row: Vale's own capture, whatever "Alat screenshot" says
    /// (that choice is for the tile). Starts through the shortcut timer like
    /// the tile, after an open panel or the search has collapsed, so it is
    /// not in the picture. One overlay at a time: a press while it is up (or
    /// while it is starting) does nothing.
    pub(super) fn on_screenshot_hotkey(&mut self) {
        if self.config.hotkeys.screenshot.is_none() || crate::capture::overlay::active() {
            return;
        }
        self.apply_now(Event::PrintScreen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hk(text: &str) -> Hotkey {
        hotkey::parse(text).unwrap().unwrap()
    }

    #[test]
    fn zoom_out_needed_only_when_zoomed_without_a_combo() {
        let combo = Some(hk("Ctrl+Alt+Z"));
        assert!(zoom_out_needed(None, true));
        assert!(!zoom_out_needed(None, false));
        assert!(!zoom_out_needed(combo, true));
        assert!(!zoom_out_needed(combo, false));
    }

    #[test]
    fn the_ids_stay_the_lyrics_card_is_6_the_sticky_note_7_and_the_list_8() {
        assert_eq!(HOTKEY_CLIP, 1);
        assert_eq!(HOTKEY_SEARCH, 2);
        assert_eq!(HOTKEY_DEVICES, 3);
        assert_eq!(HOTKEY_ARRANGE, 4);
        assert_eq!(HOTKEY_SCREENSHOT, 5);
        assert_eq!(HOTKEY_LYRICS_CARD, 6);
        assert_eq!(ROWS, 13);
        assert_eq!(HOTKEY_STICKY, 7, "6 is reserved for the lyrics card");
        assert_eq!(HOTKEY_STICKY_LIST, 8);
        assert_eq!(HOTKEY_RECORD, 9);
        assert_eq!(HOTKEY_RECORD_MUTE, 12, "10 and 11 are the screen recorder's");
        assert_eq!(HOTKEY_RECORD_SCREEN, 10);
        assert_eq!(wm_id(HotkeyId::RecordScreen), 10);
        for id in HotkeyId::ALL {
            assert_eq!(row_of_wm_id(wm_id(id)), Some(id));
        }
        assert_eq!(row_of_wm_id(0), None);
        assert_eq!(row_of_wm_id(10), Some(HotkeyId::RecordScreen));
        assert_eq!(HOTKEY_RECORD_ZOOM, 11);
        assert_eq!(wm_id(HotkeyId::RecordZoom), 11);
        assert_eq!(row_of_wm_id(11), Some(HotkeyId::RecordZoom));
        // The hook's `wparam` is the row index, and the thirteen ids are distinct.
        let mut ids: Vec<i32> = HotkeyId::ALL.iter().map(|i| wm_id(*i)).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 13);
    }

    #[test]
    fn the_captions_row_is_last_and_registered_always() {
        assert_eq!(HOTKEY_CAPTIONS, 14);
        assert_eq!(wm_id(HotkeyId::Captions), 14);
        assert_eq!(row_of_wm_id(14), Some(HotkeyId::Captions));
        assert_eq!(HotkeyId::ALL.last(), Some(&HotkeyId::Captions));
        let c = Some(hk("Ctrl+Alt+C"));
        let cfg = Hotkeys::default();
        for (audio, screen) in [(false, false), (true, false), (false, true), (true, true)] {
            let rec = any_recording(audio, screen);
            assert_eq!(wanted(&cfg, rec, screen).get(HotkeyId::Captions), c);
        }
    }

    #[test]
    fn the_countdown_esc_is_no_row() {
        assert_eq!(HOTKEY_COUNTDOWN_ESC, 13);
        assert_eq!(row_of_wm_id(HOTKEY_COUNTDOWN_ESC), None);
        assert!(is_countdown_esc(13));
        for id in HotkeyId::ALL {
            let wm = wm_id(id);
            assert!(!is_countdown_esc(wm), "{id:?}");
            assert_ne!(wm, 13, "{id:?}");
        }
        assert!(!is_countdown_esc(0));
        assert!(!is_countdown_esc(11), "11 is the zoom row's");
    }

    #[test]
    fn the_screen_row_is_registered_at_idle_and_the_mute_row_with_either_recording() {
        let cfg = Hotkeys::default();
        let r = hk("Ctrl+Alt+Shift+R");
        // The screen row starts a recording, so it is wanted at idle too.
        assert_eq!(wanted(&cfg, false, false).get(HotkeyId::RecordScreen), Some(r));
        assert_eq!(wanted(&cfg, true, true).get(HotkeyId::RecordScreen), Some(r));
        // What `sync_hotkeys` passes: an audio or a screen recording runs. The
        // zoom row is wanted with a screen recording only.
        let m = Some(hk("Ctrl+Alt+M"));
        let z = Some(hk("Ctrl+Alt+Z"));
        for (audio, screen, mute, zoom) in [
            (false, false, None, None),
            (true, false, m, None),
            (false, true, m, z),
            (true, true, m, z),
        ] {
            let want = wanted(&cfg, any_recording(audio, screen), screen);
            assert_eq!(want.get(HotkeyId::RecordMute), mute, "{audio} {screen}");
            assert_eq!(want.get(HotkeyId::RecordZoom), zoom, "{audio} {screen}");
        }
        let mut s = HotkeyState::default();
        let mut ops = FakeOps::default();
        s.apply(&wanted(&cfg, false, false), &mut ops);
        assert_eq!(ops.registered.get(&HotkeyId::RecordScreen), Some(&r));
        assert!(!s.any_failed());
    }

    #[test]
    fn a_row_is_planned_from_what_is_wanted_in_effect_and_failed() {
        let v = hk("Ctrl+Alt+V");
        let snap = hk("PrtSc");
        // Nothing to do while it matches and works.
        assert_eq!(plan_row(None, None, false), RowPlan::Keep);
        assert_eq!(plan_row(Some(v), Some(v), false), RowPlan::Keep);
        assert_eq!(plan_row(Some(snap), Some(snap), false), RowPlan::Keep);
        // A new value: registered, or hooked for PrtSc.
        assert_eq!(plan_row(Some(v), None, false), RowPlan::Register(v));
        assert_eq!(
            plan_row(Some(v), Some(hk("Win+Alt+V")), false),
            RowPlan::Register(v)
        );
        assert_eq!(plan_row(Some(snap), None, false), RowPlan::Hook(snap));
        assert_eq!(
            plan_row(Some(hk("Ctrl+PrtSc")), Some(snap), false),
            RowPlan::Hook(hk("Ctrl+PrtSc"))
        );
        // Turned off.
        assert_eq!(plan_row(None, Some(v), false), RowPlan::Clear);
        assert_eq!(plan_row(None, Some(snap), false), RowPlan::Clear);
        // A failed row is tried again with the same value.
        assert_eq!(plan_row(Some(v), Some(v), true), RowPlan::Register(v));
        assert_eq!(plan_row(Some(snap), Some(snap), true), RowPlan::Hook(snap));
    }

    #[test]
    fn the_hook_is_needed_only_while_a_row_uses_prtsc() {
        let mut s = HotkeyState::default();
        assert!(!s.needs_hook());
        s.applied[HotkeyId::Devices.index()] = Some(hk("Ctrl+Alt+D"));
        assert!(!s.needs_hook());
        s.applied[HotkeyId::Clipboard.index()] = Some(hk("Win+PrtSc"));
        assert!(s.needs_hook());
        s.applied[HotkeyId::Clipboard.index()] = None;
        assert!(!s.needs_hook());
        assert!(!s.any_failed());
        s.failed[2] = true;
        assert!(s.any_failed());
        assert_eq!(
            s.failed_flags(),
            [false, false, true, false, false, false, false, false, false, false, false, false, false]
        );
    }

    /// `RegisterHotKey` as Windows does it: a combo is refused while any id
    /// holds it, the window's own other ids included.
    #[derive(Default)]
    struct FakeOps {
        registered: std::collections::HashMap<HotkeyId, Hotkey>,
        table: [Option<Hotkey>; ROWS],
        taken_elsewhere: Vec<Hotkey>,
    }

    impl HotkeyOps for FakeOps {
        fn release(&mut self, id: HotkeyId) {
            self.registered.remove(&id);
            self.table[id.index()] = None;
        }

        fn register(&mut self, id: HotkeyId, hk: Hotkey) -> bool {
            if self.taken_elsewhere.contains(&hk)
                || self.registered.contains_key(&id)
                || self.registered.values().any(|held| *held == hk)
            {
                return false;
            }
            self.registered.insert(id, hk);
            true
        }

        fn hook(&mut self, id: HotkeyId, hk: Hotkey) {
            self.table[id.index()] = Some(hk);
        }
    }

    fn rows(pairs: &[(HotkeyId, &str)]) -> Hotkeys {
        let mut keys = Hotkeys::default();
        for id in HotkeyId::ALL {
            keys.set(id, None);
        }
        for (id, text) in pairs {
            keys.set(*id, Some(hk(text)));
        }
        keys
    }

    #[test]
    fn two_rows_can_swap_their_combos_in_one_sync() {
        let space = hk("Ctrl+Alt+Space");
        let v = hk("Ctrl+Alt+V");
        let mut s = HotkeyState::default();
        let mut ops = FakeOps::default();
        // Search off, Clipboard on Ctrl+Alt+Space (the reviewer's repro
        // before "Kembalikan semua").
        s.apply(&rows(&[(HotkeyId::Clipboard, "Ctrl+Alt+Space")]), &mut ops);
        assert_eq!(ops.registered.get(&HotkeyId::Clipboard), Some(&space));
        // Back to the defaults: Search (earlier in `ALL`) takes the combo
        // Clipboard still holds until Clipboard is let go of.
        s.apply(&rows(&[(HotkeyId::Search, "Ctrl+Alt+Space")]), &mut ops);
        assert!(!s.any_failed(), "{:?}", s.failed_flags());
        assert_eq!(ops.registered.get(&HotkeyId::Search), Some(&space));
        assert_eq!(ops.registered.get(&HotkeyId::Clipboard), None);
        // A hand edit that swaps two set rows.
        s.apply(
            &rows(&[
                (HotkeyId::Search, "Ctrl+Alt+V"),
                (HotkeyId::Clipboard, "Ctrl+Alt+Space"),
            ]),
            &mut ops,
        );
        s.apply(
            &rows(&[
                (HotkeyId::Search, "Ctrl+Alt+Space"),
                (HotkeyId::Clipboard, "Ctrl+Alt+V"),
            ]),
            &mut ops,
        );
        assert!(!s.any_failed(), "{:?}", s.failed_flags());
        assert_eq!(ops.registered.get(&HotkeyId::Search), Some(&space));
        assert_eq!(ops.registered.get(&HotkeyId::Clipboard), Some(&v));
    }

    #[test]
    fn a_sync_hooks_prtsc_keeps_what_did_not_change_and_marks_what_is_taken() {
        let mut s = HotkeyState::default();
        let mut ops = FakeOps {
            taken_elsewhere: vec![hk("Ctrl+Alt+D")],
            ..FakeOps::default()
        };
        let want = rows(&[
            (HotkeyId::Search, "Ctrl+Alt+Space"),
            (HotkeyId::Devices, "Ctrl+Alt+D"),
            (HotkeyId::Screenshot, "PrtSc"),
        ]);
        s.apply(&want, &mut ops);
        assert!(s.failed_flags()[HotkeyId::Devices.index()]);
        assert_eq!(s.failed.iter().filter(|f| **f).count(), 1);
        assert_eq!(ops.table[HotkeyId::Screenshot.index()], Some(hk("PrtSc")));
        assert!(s.needs_hook());
        // The other app lets go: the retry registers Devices and leaves the
        // rows that work alone.
        ops.taken_elsewhere.clear();
        s.apply(&want, &mut ops);
        assert!(!s.any_failed());
        assert_eq!(ops.registered.len(), 2);
        // Screenshot off: out of the table, the hook no longer needed.
        let off = rows(&[
            (HotkeyId::Search, "Ctrl+Alt+Space"),
            (HotkeyId::Devices, "Ctrl+Alt+D"),
        ]);
        s.apply(&off, &mut ops);
        assert_eq!(ops.table, [None; ROWS]);
        assert!(!s.needs_hook());
        assert_eq!(ops.registered.len(), 2);
    }

    #[test]
    fn the_lyrics_card_row_registers_like_any_other_and_never_uses_the_hook() {
        let mut s = HotkeyState::default();
        let mut ops = FakeOps::default();
        let want = rows(&[
            (HotkeyId::Search, "Ctrl+Alt+Space"),
            (HotkeyId::LyricsCard, "Ctrl+Alt+L"),
        ]);
        s.apply(&want, &mut ops);
        assert!(!s.any_failed());
        assert_eq!(
            ops.registered.get(&HotkeyId::LyricsCard),
            Some(&hk("Ctrl+Alt+L"))
        );
        assert!(!s.needs_hook());
        assert_eq!(
            plan_row(want.get(HotkeyId::LyricsCard), None, false),
            RowPlan::Register(hk("Ctrl+Alt+L"))
        );
        // Taken by another app: only this row fails, the others stay.
        let mut s = HotkeyState::default();
        let mut ops = FakeOps {
            taken_elsewhere: vec![hk("Ctrl+Alt+L")],
            ..FakeOps::default()
        };
        s.apply(&want, &mut ops);
        assert!(s.failed_flags()[HotkeyId::LyricsCard.index()]);
        assert_eq!(s.failed.iter().filter(|f| **f).count(), 1);
        // Off: let go.
        s.apply(&rows(&[(HotkeyId::Search, "Ctrl+Alt+Space")]), &mut ops);
        assert!(!s.any_failed());
        assert_eq!(ops.registered.get(&HotkeyId::LyricsCard), None);
    }

    #[test]
    fn the_mute_row_is_wanted_only_while_a_recording_runs() {
        let cfg = Hotkeys::default();
        assert_eq!(cfg.get(HotkeyId::RecordMute), Some(hk("Ctrl+Alt+M")));
        let idle = wanted(&cfg, false, false);
        assert_eq!(idle.get(HotkeyId::RecordMute), None);
        // Nothing else changes (but the zoom row, which is off without a
        // screen recording).
        for id in HotkeyId::ALL {
            if id != HotkeyId::RecordMute && id != HotkeyId::RecordZoom {
                assert_eq!(idle.get(id), cfg.get(id), "{id:?}");
            }
        }
        assert_eq!(wanted(&cfg, true, true), cfg);
        // An audio recording keeps the zoom row off.
        let audio = wanted(&cfg, true, false);
        assert_eq!(audio.get(HotkeyId::RecordZoom), None);
        assert_eq!(audio.get(HotkeyId::RecordMute), cfg.get(HotkeyId::RecordMute));
        // A mute row the user turned off stays off while recording.
        let mut off = cfg;
        off.set(HotkeyId::RecordMute, None);
        assert_eq!(wanted(&off, true, true).get(HotkeyId::RecordMute), None);
    }

    #[test]
    fn the_mute_combo_is_registered_with_a_recording_and_let_go_of_after_it() {
        let cfg = Hotkeys::default();
        let mut s = HotkeyState::default();
        let mut ops = FakeOps::default();
        let m = hk("Ctrl+Alt+M");
        s.apply(&wanted(&cfg, false, false), &mut ops);
        assert_eq!(ops.registered.get(&HotkeyId::RecordMute), None, "free at idle");
        assert_eq!(s.applied[HotkeyId::RecordMute.index()], None);
        s.apply(&wanted(&cfg, true, false), &mut ops);
        assert_eq!(ops.registered.get(&HotkeyId::RecordMute), Some(&m));
        assert!(!s.any_failed());
        s.apply(&wanted(&cfg, false, false), &mut ops);
        assert_eq!(ops.registered.get(&HotkeyId::RecordMute), None, "free again after the stop");
        // Another app holds the combo: only this row fails, and only while recording.
        let mut s = HotkeyState::default();
        let mut ops = FakeOps {
            taken_elsewhere: vec![m],
            ..FakeOps::default()
        };
        s.apply(&wanted(&cfg, false, false), &mut ops);
        assert!(!s.any_failed());
        s.apply(&wanted(&cfg, true, false), &mut ops);
        assert!(s.failed_flags()[HotkeyId::RecordMute.index()]);
        s.apply(&wanted(&cfg, false, false), &mut ops);
        assert!(!s.any_failed(), "a row that is off is not failed");
    }

    #[test]
    fn the_zoom_combo_is_registered_with_a_screen_recording_and_let_go_of_after_it() {
        let cfg = Hotkeys::default();
        let mut s = HotkeyState::default();
        let mut ops = FakeOps::default();
        let z = hk("Ctrl+Alt+Z");
        s.apply(&wanted(&cfg, false, false), &mut ops);
        assert_eq!(ops.registered.get(&HotkeyId::RecordZoom), None, "free at idle");
        // An audio recording leaves it free too.
        s.apply(&wanted(&cfg, true, false), &mut ops);
        assert_eq!(ops.registered.get(&HotkeyId::RecordZoom), None);
        s.apply(&wanted(&cfg, true, true), &mut ops);
        assert_eq!(ops.registered.get(&HotkeyId::RecordZoom), Some(&z));
        assert!(!s.any_failed());
        s.apply(&wanted(&cfg, false, false), &mut ops);
        assert_eq!(ops.registered.get(&HotkeyId::RecordZoom), None, "free again after the stop");
        // Another app holds the combo: only this row fails, and only while
        // a screen recording runs.
        let mut s = HotkeyState::default();
        let mut ops = FakeOps {
            taken_elsewhere: vec![z],
            ..FakeOps::default()
        };
        s.apply(&wanted(&cfg, false, false), &mut ops);
        assert!(!s.any_failed());
        s.apply(&wanted(&cfg, true, true), &mut ops);
        assert!(s.failed_flags()[HotkeyId::RecordZoom.index()]);
        s.apply(&wanted(&cfg, false, false), &mut ops);
        assert!(!s.any_failed(), "a row that is off is not failed");
    }

    #[test]
    fn a_new_state_is_not_started_and_not_paused() {
        let s = HotkeyState::default();
        assert!(!s.started && !s.paused);
    }
}
