# Notch keeps itself on top — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When another app's window ends up above the island (seen after another app went fullscreen or maximised), the island puts itself back on top within about a second, without a new timer.

**Architecture:** The island sets `HWND_TOPMOST` once at creation and again only on DPI/display/monitor changes; `IslandWindow::set_front(true)` returns early because it caches `front == true`, so nothing ever repairs a window that got above it (the user had to minimise the other app). The fix adds one check to the existing 1 s `TIMER_SYSTEM` tick: walk the z-order from the top down to the island, and if a real window of another process that overlaps the island and does not cover the whole monitor sits above it, call `SetWindowPos(HWND_TOPMOST)` again. The walk reuses `WindowFacts`, `is_candidate`, `intersects`, `covers_monitor` and `facts_of` in `src/sources/fullscreen.rs`.

**Tech Stack:** Rust 2024, `windows` crate (Win32 `SetWindowPos`, `GetTopWindow`, `GetWindow`).

**Spec (decided with the user, who left the details to us):**
- Repair only while the island is meant to be in front: `IslandWindow.front == true` (stay-behind not lowered) and `App.fullscreen == false`.
- A window counts as covering when it is above the island in z-order, `is_candidate` (visible, not minimised, not cloaked, not Vale's own process, not shell, not a tool window such as menus and tooltips, not click-through), its frame intersects the island window's rect, and it does **not** cover the whole monitor (`covers_monitor` false): fullscreen windows stay with the existing fullscreen rule (the notch hides over them).
- Topmost or not does not matter: a foreign app window above the notch is wrong either way.
- Vale's own windows (sticky notes, lyrics card, tooltips, overlays) are never a reason to raise.
- No new timer, thread or hook; at most one `SetWindowPos` per 1 s tick, and none when nothing covers the island.

## Global Constraints

- `cargo test --locked` must pass (except the known drive Z: test on this PC).
- Do not touch `src/sign/**` (other sessions are changing it).
- Unit tests live in the file's own `#[cfg(test)] mod tests`.

## Review Focus

- A foreign context menu or tooltip over the notch: tool windows are skipped, so no raise (test in Task 1).
- A fullscreen video/game above the island: covers the monitor, so no raise (test in Task 1).
- Stay-behind on and lowered (`front == false`): never raised by this check (test in Task 2).
- Windows below the island never count, even when they overlap it (test in Task 1).
- Vale's own topmost windows above the island (notes, lyrics card): own process, no raise (test in Task 1).

---

### Task 1: Detect a window covering the island (`src/sources/fullscreen.rs`)

**Files:**
- Modify: `src/sources/fullscreen.rs` (new pure fn + Win32 wrapper + tests in its `mod tests`)

**Interfaces:**
- Produces: `pub fn covers_island(above: impl IntoIterator<Item = WindowFacts>, island: Rect4, monitor: Rect4) -> bool` (pure) and `pub fn island_covered(island: HWND) -> bool` (Win32).

- [ ] **Step 1: Write the failing tests** in `mod tests`, using the existing `window(rect)` helper and `NOTCH`; island rect `ISLAND = Rect4::new(650, 0, 1270, 330)`:
  - `a_window_above_the_island_covers_it`: `covers_island([window(Rect4::new(-8, -8, 1928, 1040))], ISLAND, NOTCH)` is true (a maximised window's frame).
  - `nothing_above_the_island_is_not_covered`: empty iterator → false.
  - `a_window_beside_the_island_does_not_cover_it`: `window(Rect4::new(0, 400, 600, 900))` → false.
  - `a_fullscreen_window_above_is_left_to_the_fullscreen_rule`: `window(NOTCH)` → false.
  - `non_candidates_above_never_cover`: each of own_process, tool, transparent, cloaked, iconic, !visible, shell set on a maximised-frame window → false.
  - `a_later_covering_window_still_counts`: `[tool menu over ISLAND, window beside, maximised window]` → true.
- [ ] **Step 2: Run** `cargo test --locked covers_island -- --nocapture` (or the module's test names) — expected: compile error, `covers_island` not found.
- [ ] **Step 3: Implement `covers_island`**: `any` over `above` with `is_candidate(w) && intersects(w.rect, island) && !covers_monitor(w.rect, monitor, w.maximised_with_caption)`.
- [ ] **Step 4: Implement `island_covered(island: HWND) -> bool`**: island rect via `GetWindowRect` (false on failure); monitor via `monitor_of(island).and_then(monitor_rect)` (false when `None`); walk `GetTopWindow(None)` / `GW_HWNDNEXT` bounded by `MAX_WALK`, stop **before** the island's own hwnd (`take_while(|h| *h != island)`), map with `facts_of`; return `covers_island(..)`. Refactor `z_order_facts` only if it keeps its behaviour (e.g. share the successors iterator); otherwise write the small walk inline. Doc comment: why (see Architecture), runs once per `TIMER_SYSTEM` tick, reads only the windows above the island.
- [ ] **Step 5: Run** `cargo test --locked fullscreen` — expected: all pass.
- [ ] **Step 6: Commit** `fix(island): detect another app's window above the notch`.

### Task 2: Raise the island again on the 1 s tick (`src/ui/window.rs`, `src/app.rs`)

**Files:**
- Modify: `src/ui/window.rs` (new `raise`), `src/app.rs` (`TIMER_SYSTEM` arm, new `keep_on_top`, pure guard + test in app.rs's tests module)

**Interfaces:**
- Consumes: `crate::sources::fullscreen::island_covered(HWND) -> bool` (check the module path used by `is_fullscreen` in app.rs).
- Produces: `IslandWindow::raise(&self)`; `fn should_raise(front: bool, fullscreen: bool, covered: impl FnOnce() -> bool) -> bool` in app.rs (free fn, next to `fullscreen_event` or similar helpers).

- [ ] **Step 1: Write the failing test** in app.rs tests: `should_raise(true, false, || true)` is true; `should_raise(false, false, || panic!())` is false and never walks (stay-behind lowered); `should_raise(true, true, || panic!())` is false (fullscreen); `should_raise(true, false, || false)` is false.
- [ ] **Step 2: Run** `cargo test --locked should_raise` — expected: compile error.
- [ ] **Step 3: Implement** `should_raise` (`front && !fullscreen && covered()`), `IslandWindow::front(&self) -> bool` if no getter exists, and `IslandWindow::raise(&self)`: `SetWindowPos(hwnd, HWND_TOPMOST, 0,0,0,0, SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)` ignoring the `front` cache (doc: the cache is why `set_front(true)` could not repair it).
- [ ] **Step 4: Wire it**: `fn keep_on_top(&self)` in app.rs: with `self.win`, if `should_raise(win.front(), self.fullscreen, || island_covered(win.hwnd()))` then `win.raise()`. Call it in the `TIMER_SYSTEM` arm right after `self.check_fullscreen()`.
- [ ] **Step 5: Run** `cargo test --locked` — expected: green except the known drive Z: test.
- [ ] **Step 6: Commit** `fix(island): put the notch back on top when another window covers it`.

### Hand check (owner)

Run `cargo run --release`, then in another app: go fullscreen and back, maximise, restore; the notch stays visible on top (any cover lasts at most about 1 s). With "Di belakang jendela" on, the island still sinks as before.
