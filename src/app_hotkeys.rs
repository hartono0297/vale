//! The island's side of the five global hotkeys (`crate::hotkey`): which combo
//! each row has in effect, registering it with Windows, the PrtSc hook for the
//! combos Windows keeps (`app_prtsc`), the retry of one that is taken, the
//! pause while Settings records a new combo, and what a press does. A child of
//! `app`, so it works on `App` directly.
//!
//! One `sync_hotkeys` brings every row to the config's value: a row that
//! differs from what is registered, or that failed, is unregistered and
//! registered again (`RegisterHotKey`, or an entry in the hook's table for a
//! PrtSc combo). The hook thread runs only while some row needs it.
//!
//! A row Windows or another app holds shows red in Settings and is tried again
//! when the tray menu opens or Settings opens (`retry_hotkeys`).

use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, RegisterHotKey, UnregisterHotKey,
};

use super::App;
use super::prtsc::{self, HookThread};
use crate::hotkey::{self, Hotkey, HotkeyId};
use crate::model::Event;

/// `RegisterHotKey` ids, one per row.
pub(super) const HOTKEY_CLIP: i32 = 1;
pub(super) const HOTKEY_SEARCH: i32 = 2;
pub(super) const HOTKEY_DEVICES: i32 = 3;
pub(super) const HOTKEY_ARRANGE: i32 = 4;
pub(super) const HOTKEY_SCREENSHOT: i32 = 5;

/// The `RegisterHotKey` id of a row.
pub(super) fn wm_id(id: HotkeyId) -> i32 {
    match id {
        HotkeyId::Clipboard => HOTKEY_CLIP,
        HotkeyId::Search => HOTKEY_SEARCH,
        HotkeyId::Devices => HOTKEY_DEVICES,
        HotkeyId::Arrange => HOTKEY_ARRANGE,
        HotkeyId::Screenshot => HOTKEY_SCREENSHOT,
    }
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

/// What the island keeps about the hotkeys.
#[derive(Default)]
pub(super) struct HotkeyState {
    /// What each row has in effect (registered or in the hook's table).
    applied: [Option<Hotkey>; 5],
    /// A row whose combo could not be registered or hooked.
    failed: [bool; 5],
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
        self.applied.iter().flatten().any(|hk| hotkey::uses_hook(*hk))
    }

    /// The rows in effect that are failed, by row index.
    pub(super) fn failed_flags(&self) -> [bool; 5] {
        self.failed
    }

    /// Whether a row is failed.
    pub(super) fn any_failed(&self) -> bool {
        self.failed.iter().any(|f| *f)
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
        let want = self.config.hotkeys;
        for id in HotkeyId::ALL {
            let i = id.index();
            let plan = plan_row(want.get(id), self.hotkeys.applied[i], self.hotkeys.failed[i]);
            if plan == RowPlan::Keep {
                continue;
            }
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), wm_id(id));
            }
            prtsc::table_clear(id);
            self.hotkeys.applied[i] = want.get(id);
            self.hotkeys.failed[i] = false;
            match plan {
                RowPlan::Keep | RowPlan::Clear => {}
                RowPlan::Hook(hk) => prtsc::table_set(id, hk.mods),
                RowPlan::Register(hk) => {
                    let ok = unsafe {
                        RegisterHotKey(
                            Some(self.hwnd),
                            wm_id(id),
                            HOT_KEY_MODIFIERS(hotkey::register_mods(hk)),
                            u32::from(hk.vk),
                        )
                    }
                    .is_ok();
                    self.hotkeys.failed[i] = !ok;
                }
            }
        }
        self.sync_hotkey_hook();
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
    }

    /// Unregisters all five ids, empties the hook's table and stops the hook;
    /// every row reads as not in effect.
    fn unregister_all_hotkeys(&mut self) {
        for id in HotkeyId::ALL {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), wm_id(id));
            }
            prtsc::table_clear(id);
        }
        self.hotkeys.applied = [None; 5];
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
            && (hotkey::check(hk).is_err() || hotkey::used_by(&self.config.hotkeys, id, hk).is_some())
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
    fn the_ids_stay_and_screenshot_is_the_fifth() {
        assert_eq!(HOTKEY_CLIP, 1);
        assert_eq!(HOTKEY_SEARCH, 2);
        assert_eq!(HOTKEY_DEVICES, 3);
        assert_eq!(HOTKEY_ARRANGE, 4);
        assert_eq!(HOTKEY_SCREENSHOT, 5);
        for id in HotkeyId::ALL {
            assert_eq!(row_of_wm_id(wm_id(id)), Some(id));
        }
        assert_eq!(row_of_wm_id(0), None);
        assert_eq!(row_of_wm_id(6), None);
        // The hook's `wparam` is the row index, and the five ids are distinct.
        let mut ids: Vec<i32> = HotkeyId::ALL.iter().map(|i| wm_id(*i)).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5);
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
        assert_eq!(plan_row(Some(v), Some(hk("Win+Alt+V")), false), RowPlan::Register(v));
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
        assert_eq!(s.failed_flags(), [false, false, true, false, false]);
    }

    #[test]
    fn a_new_state_is_not_started_and_not_paused() {
        let s = HotkeyState::default();
        assert!(!s.started && !s.paused);
    }
}
