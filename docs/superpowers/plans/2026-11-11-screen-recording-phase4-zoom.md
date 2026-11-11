# Screen recording phase 4 (zoom, cursor effects) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `Ctrl+Alt+Z` zooms a screen recording's video in to the cursor and out again, `Ctrl+Alt`+wheel changes the level while zoomed, the notch shows the level, and the video gets a cursor highlight and click ripples (each switchable in Settings).

**Architecture:** The recorder thread already has the zoom camera, the ripples and the cursor log (`src/screenrec/`); phase 4 adds the cursor highlight to the compositor, two recording prefs, hotkey row 11 (registered only while a screen recording runs), an island-side mirror of the zoom state (`ZoomState`) for the notch, and a low-level mouse hook thread that exists only while zoomed.

**Tech Stack:** Rust 2024, `windows` crate (Win32 hooks, `RegisterHotKey`, Direct2D), Windows 11 x64.

**Spec:** `docs/superpowers/specs/2026-11-11-screen-recording-phase4-zoom-design.md` (parent: `docs/superpowers/specs/2026-11-07-recording-design.md`). Phase 3's plan for context: `docs/superpowers/plans/2026-11-11-screen-recording-phase3.md`.

## Global Constraints

- Ids: hotkey row `RecordZoom` = `RegisterHotKey` id **11**; wheel message `WM_APP + 92`. No new timer.
- Idle cost zero: the wheel hook thread runs only while zoomed; hotkey 11 is registered only while a screen recording runs (`ScreenPhase::Recording`).
- Zoom levels: start 2.0 (`zoom::DEFAULT_LEVEL`), step 0.5, range **1.5 to 4.0** for steps.
- Defaults: `cursor_highlight = true`, `click_ripples = true`; they apply to the next recording.
- Every user-facing string in Indonesian and English (`src/i18n.rs` `Key`).
- No new crates. `cargo test --locked` green after every task. Tests spawn no windows ([[test-spawns-no-window]]).
- UI work (Tasks 2, 3) loads `antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-copywriting`, `antislop:antislop-human` first and runs their checklists.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Work in the worktree `D:\GitHub\Dynamic Island\.claude\worktrees\phase4-zoom` (branch `phase4-zoom`).

## Review Focus

1. **Zoom toggled or stepped while paused:** the island's `ZoomState` and the recorder's `ZoomCamera` must agree after the resume (the commands queue). Pinned by `the_mirror_agrees_with_the_camera` (Task 1).
2. **A recording that ends while zoomed** (stop, discard, the window closed, the monitor gone, `screen_idle`): the wheel hook stops, hotkey 11 is let go, the next recording starts unzoomed at 2×. Pinned by the `wanted` test (Task 3) and `ZoomState::default` at start (Task 3); the reviewer checks every end path drops `ScreenState::wheel`.
3. **A step at the edge** (4× up, 1.5× down): nothing is sent to the recorder and the notch does not change. Pinned by `next_level_clamps` and `a_step_at_the_edge_changes_nothing` (Task 1).
4. **Smooth-scroll and reversed wheels** (deltas of 30, a turn back mid-notch): one step per 120 in one direction, a reversal starts over. Pinned by `WheelAcc` tests (Task 4).
5. **Ctrl+Alt+Shift+wheel or with Win:** not swallowed, not a step. Pinned by `zoom_wheel_mods` test (Task 4).

---

### Task 1: Zoom level rules, the mirror, the cursor highlight in the video

Model: Sonnet (GPU drawing code plus pure maths).

**Files:**
- Modify: `src/screenrec/zoom.rs` (constants, `next_level`, `ZoomState`, `ZoomCamera::step`, tests incl. `steps_change_the_level_within_bounds` which today expects `MIN_LEVEL` after stepping down)
- Modify: `src/screenrec/mod.rs` (`Options.cursor_highlight`, `Overlay` construction ~line 762)
- Modify: `src/screenrec/compose.rs` (`Overlay.highlight`, `highlight_radius`, a `glow` brush, `draw_overlay`)
- Modify: `src/screenrec/session.rs` (`CursorEffects`, `options_for`)

**Interfaces:**
- Produces (zoom.rs):
  - `pub const MIN_STEP_LEVEL: f64 = 1.5;`
  - `pub fn next_level(level: f64, dir: i32) -> f64` — `level + 0.5 * dir.signum()` clamped to `[MIN_STEP_LEVEL, MAX_LEVEL]`.
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub struct ZoomState { pub on: bool, pub level: f64 }`, `Default` = `{ on: false, level: DEFAULT_LEVEL }`; `pub fn toggle(&mut self)`; `pub fn step(&mut self, dir: i32) -> bool` (true when `level` changed; changes it on or off, like the camera); `pub fn shown(&self) -> Option<f64>` (`Some(level)` while on).
  - `ZoomCamera::step` uses `next_level`.
- Produces (session.rs): `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct CursorEffects { pub highlight: bool, pub ripples: bool }`; `pub fn options_for(fps: u32, sources: RecSources, title: &str, effects: CursorEffects) -> Options` (sets `cursor_highlight` and `click_ripples` from it).
- Produces (mod.rs): `Options.cursor_highlight: bool` (default `true` in `Options::default`).
- Produces (compose.rs): `Overlay.highlight: bool`; `pub fn highlight_radius(scale: f64) -> f32` = `20.0 * scale`.

- [ ] **Step 1: Write the failing tests** in `zoom.rs`'s test module:

```rust
#[test]
fn next_level_clamps() {
    assert!(close(next_level(2.0, 1), 2.5));
    assert!(close(next_level(2.0, -1), 1.5));
    assert!(close(next_level(1.5, -1), 1.5));
    assert!(close(next_level(4.0, 1), 4.0));
    assert!(close(next_level(2.0, 0), 2.0));
    assert!(close(next_level(2.0, 7), 2.5), "one step whatever the size");
}

#[test]
fn a_step_at_the_edge_changes_nothing() {
    let mut z = ZoomState { on: true, level: MAX_LEVEL };
    assert!(!z.step(1));
    assert_eq!(z.shown(), Some(MAX_LEVEL));
    let mut z = ZoomState { on: true, level: MIN_STEP_LEVEL };
    assert!(!z.step(-1));
    assert!(z.step(1));
    assert_eq!(z.shown(), Some(2.0));
}

#[test]
fn the_mirror_starts_off_at_two_and_toggles() {
    let mut z = ZoomState::default();
    assert_eq!(z.shown(), None);
    z.toggle();
    assert_eq!(z.shown(), Some(DEFAULT_LEVEL));
    z.toggle();
    assert_eq!(z.shown(), None);
}

#[test]
fn the_mirror_agrees_with_the_camera() {
    // The island applies each press to its ZoomState at once; the recorder
    // thread applies the same commands later, in order (also after a pause).
    let mut cam = ZoomCamera::new(HD, 1920, 1080);
    let mut z = ZoomState::default();
    let presses = [0, 1, 1, 1, 1, 1, 0, -1, 0, -1, -1, -1, -1, 0, 1];
    for (i, &p) in presses.iter().enumerate() {
        let t = i as f64 * 1000.0;
        if p == 0 {
            z.toggle();
            cam.toggle(t, (100.0, 100.0));
        } else if z.step(p) {
            cam.step(p, t);
        }
        assert_eq!(cam.is_on(), z.on, "press {i}");
        assert!(close(cam.level(), z.level), "press {i}");
    }
}
```

Change `steps_change_the_level_within_bounds` to expect `MIN_STEP_LEVEL` after stepping down. In `session.rs` tests, update the `options_for` calls to pass `CursorEffects { highlight: true, ripples: true }` and add:

```rust
#[test]
fn the_cursor_effects_come_from_the_settings() {
    let off = CursorEffects { highlight: false, ripples: false };
    let o = options_for(30, sources(true, true), "", off);
    assert!(!o.cursor_highlight && !o.click_ripples && o.draw_cursor);
    let on = CursorEffects { highlight: true, ripples: true };
    let o = options_for(30, sources(true, true), "", on);
    assert!(o.cursor_highlight && o.click_ripples);
}
```

(The existing `assert!(!o.click_ripples)` goes.) In `compose.rs` tests:

```rust
#[test]
fn the_highlight_follows_the_cursor_size() {
    assert_eq!(highlight_radius(1.0), 20.0);
    assert_eq!(highlight_radius(2.0), 40.0);
    // Wider than a fresh ripple, so the ripple starts inside it.
    assert!(highlight_radius(1.0) > ripple_at(0.0, 1.0).unwrap().0);
}
```

- [ ] **Step 2: Run** `cargo test --locked screenrec::` — expect compile errors / failures for the new names.

- [ ] **Step 3: Implement.** `zoom.rs`: the constant, `next_level`, `ZoomState` (pure, next to `ZoomCamera`), `ZoomCamera::step` through `next_level`. `session.rs`: `CursorEffects`, the new `options_for` parameter. `mod.rs`: `Options.cursor_highlight`, and the `Overlay` built per frame gets `highlight: self.options.cursor_highlight`. `compose.rs`: `Overlay.highlight`; a third brush `glow` made in `Compositor::new` beside `fill`/`ring`, colour `#FFD54A` (r 1.0, g 0.835, b 0.29); in `draw_overlay`, when `overlay.highlight` and the cursor image is known, fill a circle of `highlight_radius(scale)` at the cursor point with `glow` at opacity 0.35 **before** the ripples and the cursor bitmap (paint order: highlight, ripples, cursor). The early return must still happen when there is nothing to draw. Fix the callers of `options_for` in `src/app_screenrec.rs` (~line 506) temporarily with `CursorEffects { highlight: true, ripples: true }`; Task 2 wires the settings.

- [ ] **Step 4: Run** `cargo test --locked` — all pass. Then `cargo run -- --screenrec-test 6 --zoom` from the worktree (debug build; writes to the scratch folder it prints) and look at one frame of the result for the yellow disc and a ripple on a click.

- [ ] **Step 5: Commit** `feat(screenrec): cursor highlight, zoom levels 1.5-4 and the island's zoom mirror`.

---

### Task 2: Settings switches for the highlight and the ripples

Model: Sonnet. Load the antislop skills named in Global Constraints first.

**Files:**
- Modify: `src/recorder/prefs.rs` (two fields, lenient read, serialize, tests)
- Modify: `src/ui/settings_layout.rs` (`screen_card` 2 → 4 rows, `Hit` variants, layout test near line 9240)
- Modify: `src/ui/settings.rs` (`Action` variants, hit handling, model apply; copy the `RecEchoCancel` path at lines 203, 2564, 4570)
- Modify: `src/app.rs` (~line 6512: the two actions save the config like `VideoFps`)
- Modify: `src/app_screenrec.rs` (the start job carries `CursorEffects` from `self.config.recording`, ~line 761 and 506)
- Modify: `src/i18n.rs` (four keys plus their entries in the key-list test near line 3999)

**Interfaces:**
- Consumes: `session::CursorEffects`, `options_for(.., effects)` (Task 1).
- Produces: `RecordingPrefs.cursor_highlight: bool`, `RecordingPrefs.click_ripples: bool` (JSON keys `"cursor_highlight"`, `"click_ripples"`, both default `true`, a non-bool value reads as the default); `Hit::CursorHighlight`, `Hit::ClickRipples`; `Action::CursorHighlight(bool)`, `Action::ClickRipples(bool)`.

Copy (Indonesian, English):
- `ScreenSettingsHighlight` "Sorot kursor", "Highlight the cursor"
- `ScreenSettingsHighlightDesc` "Lingkaran kuning lembut di kursor dalam video", "A soft yellow circle around the cursor in the video"
- `ScreenSettingsRipples` "Riak klik", "Click ripples"
- `ScreenSettingsRipplesDesc` "Lingkaran yang melebar di video tiap kamu klik", "A ring spreads in the video on every click"

Card order: fps, Sorot kursor, Riak klik, folder (dividers between rows as today).

- [ ] **Step 1: Failing tests.** In `prefs.rs`: defaults are `true`/`true`; `{"cursor_highlight": false, "click_ripples": false}` reads both false; `{"cursor_highlight": "no", "click_ripples": 1}` reads both `true`; a round trip through serialize/`from_json` keeps `false`. In `settings_layout.rs`: the Rekam layar card has the toggles `Hit::CursorHighlight` and `Hit::ClickRipples` in rows 2 and 3, below `Group::VideoFps` and above `Hit::VideoFolderOpen` (extend the test at ~9240). In `settings.rs`: a click on `Hit::CursorHighlight` with the pref on posts `Action::CursorHighlight(false)`, and the same for ripples.
- [ ] **Step 2: Run** `cargo test --locked` — the new tests fail.
- [ ] **Step 3: Implement** as listed under Files.
- [ ] **Step 4: Run** `cargo test --locked` — pass. `cargo run -- --settings` and look at Fitur → Rekam layar in the dark and the light theme (screenshot both); toggle each switch and check `config.json`'s `recording` section.
- [ ] **Step 5: Commit** `feat(screenrec): Settings switches for the cursor highlight and the click ripples`.

---

### Task 3: Hotkey 11 toggles the zoom; the notch shows the level

Model: Sonnet. Load the antislop skills first (notch text).

**Files:**
- Modify: `src/hotkey.rs` (`HotkeyId::RecordZoom` appended after `RecordScreen` in the enum and `ALL`, name `"record_zoom"`, default `Ctrl+Alt+Z`, `Hotkeys.record_zoom`, `get`/`set`, tests)
- Modify: `src/app_hotkeys.rs` (`HOTKEY_RECORD_ZOOM: i32 = 11`, `wm_id`, `wanted`, the press, tests; update the "10 and 11 are the screen recorder's" comments)
- Modify: `src/i18n.rs` (`HotkeyRowRecordZoom` "Zoom video rekaman" / "Zoom the recording video", `HotkeyRowRecordZoomDesc` "Hanya aktif saat merekam layar" / "Active only while recording the screen"; the key-list test)
- Modify: `src/ui/settings_layout.rs` (~line 5236, the row's label and description keys)
- Modify: `src/model.rs` (`RecStatus.zoom: Option<f64>`; every `RecStatus { .. }` builds `zoom: None` except the screen one)
- Modify: `src/app_screenrec.rs` (`ScreenState.zoom: ZoomState`, reset in `screen_started`; `status_of` takes the zoom; `screen_toggle_zoom`, `screen_zoom_step`; drop the `#[cfg_attr(not(debug_assertions), allow(dead_code))]` on `ScreenRecorder::toggle_zoom` / `zoom_step` / `Cmd::ToggleZoom` / `Cmd::ZoomStep` in `src/screenrec/mod.rs`)
- Modify: `src/ui/render.rs` (`RecFrame.zoom: Option<f64>`), `src/app.rs` (~line 6851, `zoom: r.zoom`), `src/ui/demo.rs` (~line 2556), `src/ui/render_panel.rs` (`paint_record_compact`)
- Modify: `src/i18n.rs` or `src/ui/render_panel.rs`: `zoom_label`

**Interfaces:**
- Consumes: `zoom::ZoomState` (Task 1).
- Produces: `pub(super) fn wanted(config: &Hotkeys, recording: bool, screen: bool) -> Hotkeys` (drops `RecordMute` unless `recording`, drops `RecordZoom` unless `screen`); `pub(super) fn screen_toggle_zoom(&mut self)`; `pub(super) fn screen_zoom_step(&mut self, dir: i32)`; `pub fn zoom_label(lang: Lang, level: f64) -> String`.

Behaviour:
- `screen_toggle_zoom`: only in `ScreenPhase::Recording`; `recorder.toggle_zoom()`, `self.screen.zoom.toggle()`, `screen_show_status()`. Task 4 adds the wheel hook here.
- `screen_zoom_step(dir)`: only while recording and `zoom.on`; `if self.screen.zoom.step(dir) { recorder.zoom_step(dir); screen_show_status() }`.
- `sync_hotkeys` passes `self.screen.recording()` as `screen` (it is already called on start and stop).
- Notch (`paint_record_compact`): paused keeps "dijeda"; a screen recording with `rec.zoom == Some(level)` draws `zoom_label(lang, level)` in the `screen_label_slot` instead of `Key::ScreenLabel`.
- `zoom_label`: whole levels without decimals (`"2×"`), halves with the language's separator (`"2,5×"` Indonesian, `"2.5×"` English). `×` is U+00D7.
- Demo: `DI_REC_LIVE=screen-zoom` in `--demo-record` shows a screen recording zoomed at 2.5× (follow how `screen` / `screen-sure` are handled).

- [ ] **Step 1: Failing tests.**
  - `hotkey.rs`: `HotkeyId::RecordZoom.index()` is one after `RecordScreen`; its default formats as `"Ctrl+Alt+Z"`; a config without `record_zoom` reads the default; `"record_zoom": null` (or however a turned-off row is stored, copy the `record_mute` test) reads off.
  - `app_hotkeys.rs`: `wm_id(HotkeyId::RecordZoom) == 11`, `row_of_wm_id(11) == Some(RecordZoom)`; extend `the_screen_row_is_registered_at_idle_and_the_mute_row_with_either_recording` to a table over `(audio, screen)` → `(mute, zoom)`: `(false,false)→(None,None)`, `(true,false)→(Some,None)`, `(false,true)→(Some,Some)`, `(true,true)→(Some,Some)`; a test that registers and lets go of id 11 across start/stop like `the_mute_combo_is_registered_with_a_recording_and_let_go_of_after_it`.
  - `zoom_label(Lang::Id, 2.0) == "2×"`, `(Lang::Id, 2.5) == "2,5×"`, `(Lang::En, 2.5) == "2.5×"`, `(Lang::En, 4.0) == "4×"`, `(Lang::Id, 1.5) == "1,5×"`.
  - `status_of(&clock, sources, muted, Some(2.0)).zoom == Some(2.0)` and `None` passes through.
  - `panel_layout.rs`/`render_panel.rs`: the widest label `"2,5×"` measured with the notch's eleven font fits `screen_label_slot` at the smallest notch size with the mic meter (extend `the_screen_view_keeps_only_the_mic_group` with a width bound of 30 dip if text measurement is not available in tests, and say which in the commit).
- [ ] **Step 2: Run** `cargo test --locked` — fail.
- [ ] **Step 3: Implement.** The `WM_HOTKEY` dispatch (`app_hotkeys.rs` ~line 359) gets `HotkeyId::RecordZoom => self.screen_toggle_zoom()`.
- [ ] **Step 4: Run** `cargo test --locked` — pass. `cargo run -- --demo-record` with `DI_REC_LIVE=screen-zoom` and screenshot the notch (dark island, pill and notch styles). `cargo run -- --settings`: the new row in Tombol pintas.
- [ ] **Step 5: Commit** `feat(screenrec): Ctrl+Alt+Z zooms the recording, the notch shows the level`.

---

### Task 4: Ctrl+Alt+wheel changes the level while zoomed

Model: Sonnet.

**Files:**
- Create: `src/screenrec/wheel.rs` (pure accumulator and modifier rule, the hook thread)
- Modify: `src/screenrec/mod.rs` (`pub mod wheel;`)
- Modify: `src/app_screenrec.rs` (`ScreenState.wheel: Option<WheelHook>`, start/stop, `WM_APP_ZOOM_WHEEL`)
- Modify: `src/app.rs` (wndproc dispatch beside `WM_APP_SCREEN_WATCH` ~line 9348)

**Interfaces:**
- Consumes: `screen_zoom_step(dir)`, `screen_toggle_zoom()` (Task 3).
- Produces: `pub const WM_APP_ZOOM_WHEEL: u32 = WM_APP + 92;` (wparam = signed step count as `isize as usize`); `#[derive(Default)] pub struct WheelAcc { acc: i32 }` with `pub fn push(&mut self, delta: i32) -> i32` (returns whole 120-notches, sign = direction; leftover kept; a delta of the other sign drops the leftover first); `pub fn zoom_wheel_mods(ctrl: bool, alt: bool, shift: bool, win: bool) -> bool` (`ctrl && alt && !shift && !win`); `pub struct WheelHook` with `pub fn start(hwnd: HWND) -> Option<WheelHook>` and `pub fn stop(self)`.

Hook: copy `app_prtsc::HookThread`'s shape (thread `vale-zoom-wheel`, `SetWindowsHookExW(WH_MOUSE_LL, ..)`, message loop, `WM_QUIT` via `PostThreadMessageW` to stop, the island `HWND` in an atomic). The proc, for `WM_MOUSEWHEEL` only, reads the modifiers with `GetAsyncKeyState` (`VK_CONTROL`, `VK_MENU`, `VK_SHIFT`, `VK_LWIN`/`VK_RWIN`); when `zoom_wheel_mods` holds it feeds `HIWORD(mouseData)` as `i16` into a thread-local `WheelAcc`, posts `WM_APP_ZOOM_WHEEL` with any non-zero result, and returns `LRESULT(1)` (swallowed); everything else goes to `CallNextHookEx`. Nothing else in the proc (a slow LL hook is removed by Windows).

App: in `screen_toggle_zoom`, after the toggle, `zoom.on` → `self.screen.wheel = WheelHook::start(self.hwnd)` (a `None` only loses the wheel; zoom still works); off → `take()` and `stop()`. Every end of a recording drops it: `screen_stop`, `screen_idle`, the shutdown path (~line 1312). The message handler calls `screen_zoom_step(1)` / `(-1)` `|n|` times for `n = wparam as isize`; wheel up (positive delta) is +1 (closer).

- [ ] **Step 1: Failing tests** in `wheel.rs`:

```rust
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
```

- [ ] **Step 2: Run** `cargo test --locked screenrec::wheel` — fail.
- [ ] **Step 3: Implement** as above.
- [ ] **Step 4: Run** `cargo test --locked` — pass. `cargo build --release --locked` and note the exe size of `target/release/vale.exe` against 4,461,056 bytes.
- [ ] **Step 5: Commit** `feat(screenrec): Ctrl+Alt+wheel changes the zoom level while zoomed`.

---

### After the tasks (controller)

1. Whole-branch review on Opus 5.5 (`requesting-code-review`), fixes, re-review.
2. Live check on the real app (release build of the branch, installed copy stopped): record an Area for ~20 s with `Ctrl+Alt+Z` in, wheel to 3× and back, out; confirm the MP4 zooms smoothly, the highlight and ripples show, the cursor log has `"z"` lines, `vale-zoom-wheel` exists only while zoomed (thread list), hotkey 11 is free after the stop (another app can register `Ctrl+Alt+Z`), Settings switches off → next video has no disc and no ripple.
3. Merge into `main` (`finishing-a-development-branch`, merge locally), `cargo test --locked` on `main`.
4. Memory: update `docs/memory/screen-recording.md` (phase 4 section, ids 11 and `WM_APP + 92` now used), `docs/memory/hotkeys.md` (row 11), `docs/memory/exe-size.md`, the index line; add phase 4 checks to `docs/memory/recording-hand-checks.md`. README: one line for zoom and cursor effects. Commit "docs: update memory and README", push `main`, clean the worktree.
