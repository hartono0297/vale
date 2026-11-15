# A mascot story stops only on a click on the notch: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a running mascot story ends on a mouse press on the notch (or a permanent gate), never on pointer movement, keys or clicks elsewhere.

**Architecture:** the gate stops reading the transient gates (pointer, eyes, menu) for a running play; `advance_play` drops the last-input-tick guess; the island's own button-down handlers end the play, and a left press in the Hover modes delivers the hover that the play held back, so the panel opens. The notch peek and the hover dwell are held off while a play runs.

**Tech Stack:** Rust 2024, `windows` crate, unit tests in `#[cfg(test)]` modules.

**Spec:** `docs/superpowers/specs/2026-11-15-maskot-berhenti-klik-notch-design.md`

## Global Constraints

- Windows 11 x64, `cargo test --locked` must stay green.
- `may_start` (when a story starts) does not change.
- Clicks outside the island keep passing through (`click_through`, unchanged).
- The 200 ms wind-down (`WIND_DOWN`) and the instant stops (`instant_stop`) do not change.

## Review Focus

- A press on the notch in the last frames of a wind-down: ends nothing new, opens the panel once, no double click on a panel hit (the release after a press that opened the panel must not become a `Click`).
- Click mode: the normal press/release still opens the panel, and that panel ends the play through the gate.
- A hover dwell held back during a play opens the panel on the next dwell after the play ends (the hover is disarmed, not spent).
- A "Mainkan maskot" play started while the notch is shrunk to the eyes: the peek is reset, nothing sticks.
- The click-through flag still clears on the frame the play is dropped.

---

### Task 1: the gate ends a running play only on permanent gates

**Files:**
- Modify: `src/ui/play/gate.rs` (`must_stop`, its tests)

**Interfaces:**
- Produces: `pub fn must_stop(env: &Env, playing: Story, shape_changed: bool, clock_jumped: bool) -> bool` (the `moment` parameter goes).

- [ ] **Step 1: change the tests first.**
  - `a_running_play_stops_on_use_a_shape_change_or_a_clock_jump` becomes `a_running_play_stops_on_a_shape_change_or_a_clock_jump`: `pointer_near: true` alone does **not** stop it (`!must_stop`), the rest stays.
  - `every_gate_but_the_clock_stops_a_running_play` becomes `every_permanent_gate_stops_a_running_play`: for each flip, `must_stop == !TRANSIENT.contains(name)`.
  - New `the_pointer_the_eyes_and_the_menu_never_stop_a_running_play`: env with `pointer_near`, `shrunk`, `menu_open` all true, `!must_stop` for every `Story` in `trigger::ALL` whose clock need is met.
  - `the_typing_face_blocks_...`, `an_env_that_may_start_a_manual_play_...`, `for_a_manual_play_must_stop_agrees_...`: drop the moment argument; the Idle assertion in `an_env_that_may_start...` now expects `false`.
- [ ] **Step 2:** `cargo test --locked play::gate` fails to compile / fails.
- [ ] **Step 3:** `must_stop` = `!permanent_ok(env) || shape_changed || clock_jumped || (playing.needs_clock() && !env.clock_on)`; doc comment says the transient gates bind only the start.
- [ ] **Step 4:** fix the callers in `src/app.rs` (`step_play`) so it compiles; `cargo test --locked play::gate` passes.

### Task 2: input no longer ends a play; a press on the notch does

**Files:**
- Modify: `src/app.rs` (`advance_play`, `ActivePlay`, `App::play_input`, `tick_to_keep`, `KEY_GRACE`, `input_ends_play`, `advance_slot`, `step_play`, `on_press`, `on_right_press`, `on_middle_press`, `on_mouse_hover`, `peek_eligible`, the play tests near line 12000-12180)

**Interfaces:**
- Consumes: `must_stop` from Task 1.
- Produces:
  - `#[derive(Debug, PartialEq)] enum PlayPress { Ignore, End, EndAndHover }`
  - `fn play_press(playing: bool, on_island: bool, left: bool, mode: OpenMode) -> PlayPress`: `Ignore` without a play or off the island; `EndAndHover` for a left press in `Hover` / `HoverStay`; `End` otherwise (left in `Click`, right, middle).
  - `fn advance_slot(slot, env, shape, clock_jumped, inside, now) -> Advance` (the `clicked` parameter goes).
  - `fn hover_held(playing: bool) -> bool` = `playing`.

- [ ] **Step 1: tests first.**
  - Delete the `input_ends_play` / `tick_to_keep` tests (≈ lines 12140-12176) and replace with `play_press_ends_only_on_the_island`: table over (playing, on_island, left, mode) → expected `PlayPress`, covering all three modes, right/middle (`left = false`) → `End`, no play → `Ignore`, off island → `Ignore`.
  - `a_manual_play_is_not_killed_by_the_pointer_on_its_first_tick` becomes `the_pointer_never_ends_a_play`: with `pointer_near`, `menu_open` true, neither a Manual nor an Idle play ends by 300 ms (`alpha == 1.0`).
  - The `advance_slot` tests drop the `clicked` argument; the one that used `clicked = true` ends the play with `active.play.end(t)` before the call and checks it winds down and clears `click_through` on the drop frame.
  - `hover_is_held_while_a_play_runs`: `hover_held(true)` and `!hover_held(false)`.
- [ ] **Step 2:** `cargo test --locked` fails.
- [ ] **Step 3: implement.**
  - `advance_play`: drop `last_input_tick`, the tick/seen bookkeeping, `buttons`, `age`, `ends`; keep the one `GetCursorPos` for `near` and `inside`. Remove `App::play_input`, `ActivePlay::started` if unused, `tick_to_keep`, `KEY_GRACE`, `input_ends_play`.
  - `fn end_play_on_press(&mut self, lparam: LPARAM, left: bool) -> PlayPress` on `App`: computes `play_press(self.play.is_some(), self.on_island(lparam), left, self.config.open_mode)`, calls `play.end(Instant::now())` for `End` / `EndAndHover`, returns the decision.
  - `on_press`: first line calls it with `left = true`; on `EndAndHover`: `self.swipe.cancel(); self.click.clear();` then `if let Some(e) = self.hover.on_hover() { self.apply_now(e) }` and return. On `End` fall through (Click mode opens the panel through the normal click).
  - `on_right_press(lparam)` and `on_middle_press`: call it with `left = false` (pass `lparam` to `on_right_press` from `WM_RBUTTONDOWN`).
  - `on_mouse_hover`: first, `if hover_held(self.play.is_some()) { self.hover.disarm(); return; }`.
  - `peek_eligible`: add `&& self.play.is_none()`.
- [ ] **Step 4:** `cargo test --locked` passes, `cargo build --release --locked` builds with no new warnings.
- [ ] **Step 5:** commit `fix(play): a mascot story ends only on a press on the notch`.

### Task 3: live check

- [ ] Run the release build (stop any running `vale`), start "Mainkan maskot" (right-click the notch), inject mouse moves with `SendInput` (relative 0,0 moves and moves to the taskbar), check the story runs to its end; a left press on the notch ends it and opens the panel. Record what was and was not seen.
