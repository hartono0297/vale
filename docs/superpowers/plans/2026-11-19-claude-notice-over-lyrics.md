# Claude notices beside the lyric line: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** With the Ringkas setting, "Claude selesai" and "Claude butuh respon" tuck into the toast slot beside the lyric line (and the compact music view) instead of covering it.

**Architecture:** The reducer in `src/state.rs` already derives a real toast's form (Full, Strip, Slot) from context (`toast_form`, `toast_candidate`, `toast_tucked`, `toast_slot`, peek). Widen "can be tucked" to the two Claude kinds, limited to a music base view. `ClaudeDone` lives in `current`, so it reuses the toast path. `ClaudeWaiting` is sticky (outranks the media in `base_view_with`), so it needs its own small tuck check that feeds the same `toast_slot()`, peek and click. The renderer, geometry and `App` already work from `toast_slot()`; nothing there should change.

**Tech Stack:** Rust 2024, unit tests inside `src/state.rs` (`#[cfg(test)]`).

**Spec:** `docs/superpowers/specs/2026-11-19-claude-notice-over-lyrics-design.md`; background `docs/memory/toast-while-busy.md`.

## Global Constraints

- Only `ToastStyle::Compact` tucks; `Penuh` keeps today's behaviour for every notice.
- Claude notices tuck only when the view under them is `View::MediaLyric` or `View::MediaCompact`. Over a panel, idle, REC, pomodoro, caption, countdown or fullscreen nothing changes.
- The Strip form, the unread dot, the in-place replacement arm and `countdown_admits` stay real-toast only (keep `is_real_toast` there).
- No new timer, window or setting. Do not edit `src/ui/cat_style.rs`, `src/ui/render.rs`, `src/ui/geometry.rs` (another session works there).
- Test names and comments in English, as in the file.

## Review Focus

- A hover opens the panel while a `ClaudeDone` slot shows: the notice goes Full over the panel as it does today, no panic, no flapping between Slot and Full.
- A `ClaudeWaiting` slot and a real toast at once: the toast takes the slot for its time, the waiting slot returns after, and the toast does not latch Full.
- Pointer peeking at the waiting slot, then `ClaudeActivity` clears the sticky: the view returns to the music view with no stale peek.
- `ToastStyle::Full`: both Claude notices cover the lyric exactly as before.
- The fullscreen brief copy of `ClaudeWaiting` (queued while fullscreen) and its sticky twin: one click gives exactly one `Effect::OpenClaudeSession`.

---

### Task 1: `ClaudeDone` tucks beside the music

**Files:**
- Modify: `src/state.rs` (`is_real_toast` neighbourhood ~line 477, `toast_candidate` ~3178, `Event::ToastPointer` ~874, `notice_held` ~1447, `advance` ~2960, `toast_hold_moved` ~3339, `settle_toast` ~3354, `toast_left` ~3306)
- Test: `src/state.rs` tests module, next to the toast tests (~11634+; helpers `busy`, `lyric_ready`, `done`, `secs`)

**Interfaces:**
- Produces: `pub fn is_tuckable(notice: &Notice) -> bool` (real toast, `ClaudeDone` or `ClaudeWaiting`); Task 2 uses it.
- Consumes: existing `toast_form()`, `toast_slot()`, `ToastForm`.

- [ ] **Step 1: Write the failing tests** (in the toast tests area; `busy("lyric", t0)` gives a lyric view, `busy("media", t0)` a music view)
  - `claude_done_over_lyrics_is_a_slot_not_a_cover`: `s.apply(Event::Notice(done("s1")), t0)`; assert `s.view() == View::MediaLyric`, `s.toast_form() == Some(ToastForm::Slot)`, `s.toast_slot().unwrap().title == "Claude is done"`.
  - `claude_done_over_media_is_a_slot`: same with `busy("media", t0)` and `View::MediaCompact`.
  - `claude_done_slot_peeks_and_a_click_opens_its_session`: after the notice, `ToastPointer(true)` gives `View::Notice` with `ToastForm::Full`; `ToastPointer(false)` returns to Slot; `Click(Hit::ToastSlot)` returns `vec![Effect::OpenClaudeSession { session_id: "s1".into() }]` and the view is the music view.
  - `claude_done_slot_ends_after_the_notice_time_without_a_dot`: `Tick` at `t0 + NOTICE_DURATION` clears it; `!s.toast_dot_shown()`.
  - `claude_done_stays_full_over_the_panel_idle_and_with_penuh`: `busy("idle")` and `busy("panel")` give `View::Notice`; with `Event::ToastStyle(ToastStyle::Full)` applied first, `busy("lyric")` gives `View::Notice` too.

- [ ] **Step 2: Run them and confirm they fail**

  Run: `cargo test --locked claude_done_ -- --nocapture`
  Expected: the slot tests FAIL (view is `View::Notice`); the "stays full" test passes already.

- [ ] **Step 3: Implement**
  - Add `pub fn is_tuckable(notice: &Notice) -> bool` next to `is_real_toast`.
  - `toast_candidate`: use `is_tuckable` for the notice, and for a non-real notice require `base_view()` to be `MediaLyric` or `MediaCompact` (a real toast keeps its full list).
  - Use `is_tuckable` in `Event::ToastPointer`, `notice_held`, `toast_hold_moved` and `settle_toast` (latch and seen). Keep `is_real_toast` in the replacement arm, `countdown_admits`, the strip code and everything in `app.rs` / `render_panel.rs`.
  - The unread dot is for real toasts only: `toast_left` sets the dot only for a real toast, but still resets `toast_full` / `toast_seen` when a `ClaudeDone` leaves `current` (`advance` calls it for any tuckable notice; give it the notice or a `real: bool`).

- [ ] **Step 4: Run the new tests, then the whole suite**

  Run: `cargo test --locked`
  Expected: the new tests pass. Existing tests that assumed a Claude notice covers a music view now fail: update each to the new behaviour (slot) or, where the test is about something else, put the state on Idle or `ToastStyle::Full`. Only the drive Z: test may stay red.

- [ ] **Step 5: Commit**

```bash
git add src/state.rs
git commit -m "fix(state): tuck the Claude done notice beside the music"
```

### Task 2: `ClaudeWaiting` (sticky) tucks beside the music

**Files:**
- Modify: `src/state.rs` (`base_view_with` ~1303, `toast_peeking` ~3199, `toast_slot` ~3246, `Event::ToastPointer`)
- Test: `src/state.rs` tests module

**Interfaces:**
- Consumes: `is_tuckable` from Task 1.
- Produces: private `fn compact_view(&self) -> View` (the part of `base_view_with` after the sticky step: rec, pomodoro, caption, media, idle), `fn sticky_slot_candidate(&self) -> bool`, `fn sticky_peeking(&self) -> bool`, `fn sticky_tucked(&self) -> bool`.

- [ ] **Step 1: Write the failing tests**
  - `claude_waiting_over_lyrics_is_a_slot`: `busy("lyric")`, `Event::Notice(waiting("s1"))`; `view() == View::MediaLyric`, `toast_slot()` is the waiting notice, `toast_form()` is `None` (nothing in `current`), `next_deadline() == None`, `!toast_dot_shown()`.
  - `claude_waiting_slot_peeks_and_a_click_opens_its_session`: `ToastPointer(true)` gives `View::Notice` with the waiting notice; `ToastPointer(false)` gives the lyric view again; `Click(Hit::ToastSlot)` gives `vec![Effect::OpenClaudeSession { session_id: "s1".into() }]` and the sticky is gone.
  - `claude_activity_clears_the_waiting_slot_even_while_peeking`: peek, then `Event::ClaudeActivity { session_id: "s1".into() }`: `view() == View::MediaLyric`, `toast_slot().is_none()`, `!toast_is_peeking()`.
  - `claude_waiting_yields_the_slot_to_a_toast_and_comes_back`: waiting slot, then `toast_notice("t1", "Ibu")`: `toast_slot()` is the toast and `toast_form() == Some(ToastForm::Slot)`; after `Tick` at `t0 + NOTICE_DURATION` the slot is the waiting notice again.
  - `claude_waiting_covers_as_before_with_penuh_idle_pomodoro_and_the_panel`: `ToastStyle::Full` over lyrics gives `View::Notice(waiting)`; `busy("idle")` and `busy("pomodoro")` give `View::Notice` (sticky still outranks); `busy("panel")` keeps the panel with `waiting_strip()` set.
  - `a_fullscreen_copy_of_the_waiting_notice_gives_one_open_effect`: fullscreen on, waiting notice (queued copy and sticky), fullscreen off, `Click(Hit::ToastSlot)`: exactly one `Effect::OpenClaudeSession`, no sticky and no current left.

- [ ] **Step 2: Run them and confirm they fail**

  Run: `cargo test --locked claude_waiting_ -- --nocapture`
  Expected: the slot and peek tests FAIL (the view is `View::Notice`).

- [ ] **Step 3: Implement**
  - Move the tail of `base_view_with` (from the `rec` check down to `View::Idle`) into `compact_view()`; `base_view_with` calls it.
  - `sticky_slot_candidate()`: `toast_style == Compact`, `sticky.last()` is a `ClaudeWaiting`, and `compact_view()` is `MediaLyric` or `MediaCompact`.
  - `sticky_peeking()`: `toast_pointer && current.is_none() && sticky_slot_candidate()` (a tucked `current` owns the pointer, so the toast peeks, never the sticky one). `sticky_tucked()`: candidate and not peeking.
  - In `base_view_with`, skip the sticky `View::Notice` step when `sticky_tucked()`.
  - `toast_peeking()` is also true while `sticky_peeking()`; `toast_slot()` returns `sticky.last()` when `toast_form() != Some(Slot)` and `sticky_tucked()`.
  - `Event::ToastPointer`: `toast_pointer = on && (current is tuckable || sticky_slot_candidate())`.
  - `Hit::ToastSlot` needs no change: it calls `dismiss_visible_notice`, which dismisses the newest sticky when `current` is empty.

- [ ] **Step 3b: Run the new tests, then the whole suite**

  Run: `cargo test --locked`
  Expected: new tests pass; fix existing tests that encoded "waiting covers the music view" as in Task 1 (Idle, pomodoro, panel and `ToastStyle::Full` keep the old behaviour). Only the drive Z: test may stay red.

- [ ] **Step 4: Commit**

```bash
git add src/state.rs
git commit -m "fix(state): tuck the sticky Claude waiting notice beside the music"
```

### Task 3: `App` wiring check and a `--demo` look

**Files:**
- Inspect: `src/app.rs` (`refresh` ~4250-4262 `slot_keep`, `toast_pointer_at` ~6609, `Hit::ToastSlot` registration), `src/app_notif.rs` (`toast_badge_of`)
- Modify (only if a scene is cheap): `src/ui/demo.rs` (a Claude variant of `notice-busy-lyric`)

- [ ] **Step 1: Read the `App` paths above.** Confirm every slot read goes through `state.toast_slot()` / `toast_is_peeking()` (so a Claude slot gets the badge, the lyric box and the click region) and that nothing there tests `is_real_toast` for the slot. The badge is the letter tile of `source` ("C"). If a path does test it, make the smallest change.
- [ ] **Step 2: Run the demo.** `cargo run --release -- --demo` (stop installed copies first: `Stop-Process -Name vale, dynamic-island`), open `notice-busy-lyric` to see the real toast slot beside the lyric; if `DemoNotice` can hold a Claude notice, add one scene with the Claude notice over the lyric (same style as `notice-busy-lyric`) and look at it.
- [ ] **Step 3: Full verification.** `cargo test --locked` (only the drive Z: test may fail; a timeout in the google http or lyrics worker tests that passes when run alone is the known load flake) and `cargo build --release --locked`.
- [ ] **Step 4: Commit** any file changed in this task: `git commit -m "feat(demo): a Claude notice beside the lyric"` (skip if none).
