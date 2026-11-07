# Sticky note on top Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new sticky note (every way of making one, the Notes card icon and the list's show included) opens over other windows until the user activates another window; the Notes card gets a sticky-paper icon and a one-item right-click menu.

**Architecture:** Reuse `Win::lifted` in `src/widgets/host.rs`. The lift no longer ends in `end_editing`; it ends on the note's `WM_ACTIVATE` / `WA_INACTIVE` (unless the window being activated is its popover, its dialog or the island) and when the island itself loses activation. The island calls the host's lift API from every creation path. Part 2 adds `Icon::StickyNote`, one i18n key and a native popup menu that the island shows outside the app borrow.

**Tech Stack:** Rust 2024, `windows` crate (Win32 layered windows, `TrackPopupMenu`), Direct2D icons from SVG-like paths.

**Spec:** `docs/superpowers/specs/2026-11-07-sticky-note-on-top-design.md`

## Global Constraints

- No change for the "Di atas jendela" layer, pinned notes, the X button (the note stays in Notes, an empty one is deleted), delete, roll up, stacks, tuck.
- No new timer, thread, hook or polling (idle power budget, [[bata-idle-power-budget]]).
- Copy, exact: "Tempel ke desktop" / "Stick to desktop"; "Lepas dari desktop" / "Unstick from desktop" (existing `Key::StickyOffDesktopTip`).
- Tests never show windows ([[test-spawns-no-window]]); host tests use the existing hidden-window helpers (`keyboard_lock`, `ctx`, `note`, `display`).
- `cargo test --locked` green, `cargo build --release --locked` with 0 warnings.
- Worktree under `.claude/worktrees/`; every commit message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. Windows refuses the foreground (another app holds the foreground lock) when the hotkey or the card makes a note: the note must not stay topmost forever. `lift` lifts only a note that is foreground or has the EDIT. Pinned by `lift_needs_the_foreground_or_the_keyboard` (Task 1).
2. Esc while lifted: editing ends, the note stays on top until another window is activated. Pinned by `a_lift_outlives_the_keyboard_and_ends_on_deactivation` (Task 1).
3. Pin while lifted, then deactivate: the note stays topmost (pinned); the Top layer is untouched. Pinned by `a_pinned_note_stays_on_top_when_its_lift_ends` (Task 1).
4. The island's own text input takes activation from a lifted note (click in the panel search), then the user clicks another app: the lift must end then, not stick. Pinned by `the_island_losing_activation_ends_every_lift` (Task 1).
5. A right-click on the Notes page outside a card, or on any other page, still opens the tray menu. Pinned by `note_card_menu_target_only_on_a_notes_card` (Task 3).

---

### Task 1: The lift ends on deactivation, not on the keyboard (host)

**Files:**
- Modify: `src/widgets/host.rs` (`bring_front` ~939, `above_windows` 1238 unchanged, `WM_ACTIVATE` arm ~2115, `end_editing` ~4869, the `bring_front` test ~5690)

**Interfaces:**
- Produces:
  - `pub fn lift(&mut self, id: &str) -> bool` makes the window showing `id` the foreground window (`take_foreground`, no editing). It sets `lifted` and runs `apply_z` only if afterwards `GetForegroundWindow() == hwnd` or `edit.is_some()`. Returns whether it lifted. Unknown id: `false`.
  - `pub fn bring_front_at(&mut self, id: &str, at: Option<usize>)` is `focus_at(id, at)` followed by the lift rule above. `bring_front(id)` becomes `bring_front_at(id, None)`.
  - `pub fn island_deactivated(&mut self, to: HWND)`: clears `lifted` on every window (and runs `apply_z` on each one that changed) unless `to` is a window that is lifted, or a popover of one (`pop`, `remind_pop`, `fmt_pop` `is_popover_of`).
  - `fn lift_survives(to: HWND, owner: HWND, popover: bool, dialog: bool) -> bool` (pure): `popover || dialog || (!to.is_invalid() && to == owner)`.
- `end_editing` no longer touches `lifted`. In `WM_ACTIVATE` `WA_INACTIVE`, after the existing editing logic, the window's `lifted` is cleared (and `apply_z` runs) unless `lift_survives(to, s.owner, <popover check>, dialog)`.

- [ ] **Step 1: Write the failing tests** (in the `host.rs` test module, next to the current `bring_front` test, which this replaces)

```rust
#[test]
fn lift_survives_only_for_its_popover_dialog_or_the_island() {
    let owner = HWND(0x10 as _); let other = HWND(0x20 as _);
    assert!(lift_survives(owner, owner, false, false));
    assert!(lift_survives(other, owner, true, false));
    assert!(lift_survives(other, owner, false, true));
    assert!(!lift_survives(other, owner, false, false));
    assert!(!lift_survives(HWND::default(), HWND::default(), false, false), "a null target never keeps it");
}

#[test]
fn a_lift_outlives_the_keyboard_and_ends_on_deactivation() {
    // one desktop-layer note n1, synced; bring_front("n1")
    // assert lifted, mode() == 2
    // end_editing(hwnd)                         -> still lifted, mode() == 2
    // SendMessageW(hwnd, WM_ACTIVATE, WPARAM(WA_INACTIVE as usize), LPARAM(owner.0 as isize))
    //                                           -> still lifted (the island)
    // SendMessageW(hwnd, WM_ACTIVATE, WPARAM(WA_INACTIVE as usize), LPARAM(0))
    //                                           -> !lifted, mode() == 1
    // bring_front("nope") lifts nothing
}

#[test]
fn lift_needs_the_foreground_or_the_keyboard() {
    // lift() gives no EDIT, so it lifts exactly when the window got the foreground
    // (a test window may or may not get it; the rule must hold either way):
    // let lifted = host.lift("n1");
    // assert_eq!(lifted, unsafe { GetForegroundWindow() } == hwnd);
    // assert_eq!(with(|s| s.wins[0].lifted).unwrap(), lifted);
    // assert_eq!(mode(), if lifted { 2 } else { 1 });
    // assert!(!host.lift("nope"));
}

#[test]
fn a_pinned_note_stays_on_top_when_its_lift_ends() {
    // note n1 with sticky.pinned = true; bring_front; deactivate to null
    // -> !lifted, above_windows still true, mode() == 2
}

#[test]
fn the_island_losing_activation_ends_every_lift() {
    // two notes; bring_front("n1")
    // host.island_deactivated(hwnd_of_n1) -> n1 still lifted (activation went to it)
    // host.island_deactivated(HWND::default()) -> n1 not lifted, mode() == 1
}
```

Each test takes `keyboard_lock()`, builds the host with `WidgetHost::new(owner)` where `owner` is a non-null dummy `HWND` the test never uses as a window, and ends with `host.shutdown()`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --locked widgets::host::tests`
Expected: compile errors (`lift`, `lift_survives`, `island_deactivated` not defined), then FAIL on `end_editing` still dropping the lift.

- [ ] **Step 3: Implement** `lift`, `bring_front_at`, `island_deactivated` and `lift_survives`, and move the clearing from `end_editing` to `WM_ACTIVATE`. Update the doc comments on `lifted` and `bring_front`: the lift now lasts "until another window is activated".

- [ ] **Step 4: Run all host tests**

Run: `cargo test --locked widgets::host`
Expected: all pass, including the existing list and stack tests.

- [ ] **Step 5: Commit** `feat(widgets): a lifted note stays on top until another window is activated`

---

### Task 2: Every new note opens lifted (island and demo wiring)

**Files:**
- Modify: `src/app_widgets.rs` (`new_sticky_with` ~523, `sticky_toggle` ~585)
- Modify: `src/app.rs` (island `WM_ACTIVATE` arm ~8692)
- Modify: `src/widgets/demo.rs` (`make` ~705)

**Interfaces:**
- Consumes: `WidgetHost::bring_front_at`, `lift` and `island_deactivated` from Task 1.

- [ ] **Step 1:** `new_sticky_with`: replace `host.focus_at(&id, caret)` with `host.bring_front_at(&id, caret)`. This covers the hotkey, the tray, the gallery, the header (+), templates and `out.beside`.
- [ ] **Step 2:** `sticky_toggle`: when the note was just stuck (`!on_desktop && done`), call `host.lift(id)` after `widgets_sync()`. Unsticking is unchanged.
- [ ] **Step 3:** In the island's `WM_ACTIVATE` `WA_INACTIVE` branch, call `with_app(|app| app.widgets_island_deactivated(HWND(lparam.0 as _)))`. The new `pub(super) fn widgets_island_deactivated(&mut self, to: HWND)` in `app_widgets.rs` forwards to the host when there is one. Keep the existing `deliver_*_key(Blur)` calls and `DefWindowProcW`.
- [ ] **Step 4:** In `demo.rs` `make`, use `bring_front_at` instead of `focus_at`, so `--demo-widgets` shows the behaviour.
- [ ] **Step 5: Verify**

Run: `cargo build --locked` and `cargo test --locked`
Expected: 0 warnings, all tests pass. `App` has no unit tests (it needs the island window). The live check is in the hand-off list.

- [ ] **Step 6: Commit** `feat(widgets): new sticky notes open on top of other windows`

---

### Task 3: Sticky-paper icon and the Notes card menu

**Files:**
- Modify: `src/ui/icons.rs` (enum, `ALL` list, the path table)
- Modify: `src/ui/render_panel.rs:4169-4172` (the card's pin)
- Modify: `src/i18n.rs` (next to `StickyOffDesktopTip`, line 1046)
- Modify: `src/app.rs` (`on_right_click` ~1763, `WM_RBUTTONUP` ~8715, a new pure fn beside `menu_right_click` ~7524)
- Modify: `src/ui/tray.rs` (a small `note_card_menu` beside `show_menu` ~399)

**Interfaces:**
- Produces:
  - `Icon::StickyNote`, line plus duotone fill like `Icon::Note`: `Part::Fill("M5 4h14v10l-6 6H5z")`, `Part::Path("M5 4h14v10l-6 6H5z")`, `Part::Path("M13 20v-6h6")`. Appended at the end of the enum and of `ALL` (the `all_is_in_declaration_order` test).
  - `Key::StickyOnDesktopTip => "Tempel ke desktop", "Stick to desktop";`
  - `fn note_card_menu_target(view: View, hit: Option<&Hit>) -> Option<String>` (pure, `app.rs`): `Some(id)` for `Hit::NoteOpen(id)` or `Hit::NoteSticky(id)` while `view == View::Panel(Page::Notes)`, else `None`.
  - `pub fn note_card_menu(hwnd: HWND, label: &str) -> bool` (`tray.rs`): one `MF_STRING` item, the same foreground, `TrackPopupMenu` (`TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON` at the cursor), `WM_NULL` and `DestroyMenu` dance as `show_menu`. True when the item was chosen.
  - `on_right_click` returns `enum RightClick { Done, Tray, NoteMenu(String) }` instead of `bool`. `NoteMenu` is checked after `clip_right_click` and before the CLI menu logic. `WM_RBUTTONUP` handles `NoteMenu(id)` outside the app borrow: it reads `on_desktop` and the label (`StickyOffDesktopTip` when on, `StickyOnDesktopTip` when off) inside `with_app`, calls `tray::note_card_menu` outside it (guarded by `MENU_OPEN` like `open_menu`), then `with_app(|app| app.sticky_toggle(&id))` when chosen.

- [ ] **Step 1: Write the failing tests**

```rust
// app.rs tests, beside right_clicks_inside_the_folder_menu_never_open_the_tray_menu
#[test]
fn note_card_menu_target_only_on_a_notes_card() {
    let notes = View::Panel(Page::Notes);
    assert_eq!(note_card_menu_target(notes, Some(&Hit::NoteOpen("n1".into()))), Some("n1".into()));
    assert_eq!(note_card_menu_target(notes, Some(&Hit::NoteSticky("n1".into()))), Some("n1".into()));
    assert_eq!(note_card_menu_target(notes, Some(&Hit::NoteNew)), None);
    assert_eq!(note_card_menu_target(notes, None), None);
    assert_eq!(note_card_menu_target(View::Panel(Page::Clipboard), Some(&Hit::NoteOpen("n1".into()))), None);
}

// i18n.rs tests (or wherever keys are checked in both languages)
// tr_in(Lang::Id, Key::StickyOnDesktopTip) == "Tempel ke desktop"
// tr_in(Lang::En, Key::StickyOnDesktopTip) == "Stick to desktop"
```

The existing `every_icon_parses` and `all_is_in_declaration_order` tests cover the new icon once it is in `ALL`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --locked note_card_menu_target`
Expected: compile error, `note_card_menu_target` not defined.

- [ ] **Step 3: Implement** the icon, the key, the pure fn, `note_card_menu`, the `RightClick` enum and its `WM_RBUTTONUP` handling. In `render_panel.rs`, draw `Icon::StickyNote` instead of `Icon::Pin` (same rect, same 14 dip size, same accent/secondary ink rule).
- [ ] **Step 4: Run all tests**

Run: `cargo test --locked`
Expected: all pass, 0 warnings.

- [ ] **Step 5: Commit** `feat(notes): sticky-paper icon and a right-click menu on Notes cards`

---

## Manual checks (owner, after merge)

1. Hotkey, tray, Settings gallery (+) and a note's header (+), each with a browser maximised: the new note shows over the browser with the caret in it. Esc keeps it on top. A click on the browser sends it behind.
2. Notes card icon on a note that is off the desktop: it shows over other windows. A click on another app sends it behind. Clicking inside the panel does not.
3. Right-click a Notes card: one item, with the right words for its state. A right-click elsewhere on the island still opens the tray menu.
4. Pin a lifted note, click away: it stays on top. The "Di atas jendela" layer behaves as before.
5. Sticky note list "Buka": the note is over other windows, and stays on top after Esc until another window is clicked.
