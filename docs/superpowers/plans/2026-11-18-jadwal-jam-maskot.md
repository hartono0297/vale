# Mascot stories on the hour Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings explains every "Animasi maskot otomatis" preset, and a new "Tiap jam" row makes the mascot play a story on round clock hours (every 1, 2, 3, 4 or 6 hours).

**Architecture:** A pure core in `src/ui/play/trigger.rs` (hour arithmetic, constants), one gate tweak in `gate.rs` (an hour story skips the cooldown), one new one-shot timer in `app.rs` that replaces the clock-bound `check_hour`, and a new Settings row plus text in `settings_layout.rs`, `settings.rs`, `i18n.rs`. Config key `mascot_hours`.

**Tech Stack:** Rust 2024, Win32 + Direct2D, chrono (`Local`, `Timelike`), serde. Windows 11 x64 only.

**Spec:** `docs/superpowers/specs/2026-11-18-jadwal-jam-maskot-design.md`. Mockup: `docs/design/maskot-jadwal-jam.html` (explain list B, hours option 1).

## Global Constraints

- Test with `cargo test --locked`; unit tests live inside `src/**` as `#[cfg(test)]` modules.
- `mascot_hours` values: `0` off, `1`, `2`, `3`, `4`, `6`; default `1`; any other value or type reads as `1`.
- Hours are round clock hours counted from 00:00 local time (every 2 hours is 02:00, 04:00 ...).
- Window after a scheduled hour: `HOUR_WINDOW` = 10 minutes; retry every `HOUR_RETRY` = 30 seconds; a missed hour is never made up (sleep, lock, wake, clock jump).
- An hour story skips the cooldown, and still needs the presence rule (idle 15 s to 5 min), the transient gates and every permanent gate. Preset `Mati` never plays an hour story.
- One timer at a time, armed only while wanted; nothing runs at idle ([[bata-idle-power-budget]]).
- Texts exist in Indonesian and English. No em dashes in any text. Keep `Key` order and the `Key => "id", "en";` macro style of `src/i18n.rs`.
- Commits: conventional style (`feat:`, `test:`), each ending with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Surgical: remove only what this change makes unused (`check_hour`, `clock_hour`, `hour_rolled` and its test); mention any other dead code instead of deleting it.

## Review Focus

- Hour arithmetic at the edges: 23:59:59 with step 2 and 6, exactly on a boundary (strictly the next one), step 0, a step that does not divide 24 clamps at 24:00. Pinned in Task 2.
- The timer fires while another story already runs: that is not "started", it retries inside the window. Pinned in Task 3 (`hour_wait`).
- The setting changes (step to Mati, preset to Mati, mascot off) while a timer is armed or a window is open: timer killed, no play. Pinned in Task 3 (`hour_timer_wanted`).
- An old `config.json` with no key, or with `"mascot_hours":"x"`, `7`, `-1`, `null`: reads as 1 and the app starts. Pinned in Task 1.
- Settings text that is longer in one language: the six-item segmented row and the five-line list must stay inside the card in both languages. Pinned in Task 4.

---

### Task 1: Config key `mascot_hours`

**Files:**
- Modify: `src/config.rs` (field next to `mascot_play` at ~237, `Default` at ~848, the merge block at ~1004, the test literal at ~1416, a lenient reader next to `fn mascot_play` at ~526)
- Modify: `src/config_transfer.rs` (key list at ~64, after `"mascot_play"`)

**Interfaces:**
- Produces: `Config::mascot_hours: u8` (`pub`), values 0, 1, 2, 3, 4, 6.

- [ ] **Step 1: Write the failing tests** in `src/config.rs` tests, beside `mascot_play_defaults_to_sesekali_and_reads_leniently`:

```rust
#[test]
fn mascot_hours_defaults_to_one_and_reads_leniently() {
    assert_eq!(Config::default().mascot_hours, 1);
    for (json, want) in [
        (r#"{}"#, 1u8),
        (r#"{"mascot_hours":0}"#, 0),
        (r#"{"mascot_hours":3}"#, 3),
        (r#"{"mascot_hours":6}"#, 6),
        (r#"{"mascot_hours":5}"#, 1),
        (r#"{"mascot_hours":7}"#, 1),
        (r#"{"mascot_hours":-1}"#, 1),
        (r#"{"mascot_hours":"2"}"#, 1),
        (r#"{"mascot_hours":null}"#, 1),
    ] {
        assert_eq!(parse(json).mascot_hours, want, "{json}");
    }
}
```

Also add `mascot_hours: 4` next to `mascot_play: MascotPlay::Sapaan` in the non-default test literal (~1416) and assert it survives whatever round trip that test does.

- [ ] **Step 2: Run** `cargo test --locked mascot_hours` → FAIL (no field).

- [ ] **Step 3: Implement** `pub mascot_hours: u8` with `#[serde(default = "...", deserialize_with = "mascot_hours")]`; the reader accepts only the integers 0, 1, 2, 3, 4, 6, anything else is 1 (same shape as `fn mascot_play`). Add it to `Default`, to the merge block beside `mascot_play: mem.mascot_play`, and `"mascot_hours"` to the `config_transfer` list. Use the set `crate::ui::play::trigger::HOUR_STEPS` (Task 2) only if Task 2 is done first; otherwise a local `matches!` and let Task 2 not depend on it.

- [ ] **Step 4: Run** `cargo test --locked` → PASS, including the existing config and transfer tests (fix any key-count assertion in `config_transfer` tests that this adds to).

- [ ] **Step 5: Commit** `feat(config): mascot_hours setting`

---

### Task 2: Hour arithmetic and the gate

**Files:**
- Modify: `src/ui/play/trigger.rs` (constants, `next_hour`, `hours_of`, `wait_range`; `wait_for` uses `wait_range`)
- Modify: `src/ui/play/gate.rs` (`may_start`, its doc comment, tests)

**Interfaces:**
- Produces, all `pub` in `trigger.rs`:
  - `pub const HOUR_STEPS: [u8; 6] = [0, 1, 2, 3, 4, 6];`
  - `pub const HOUR_WINDOW: Duration = Duration::from_secs(10 * 60);`
  - `pub const HOUR_RETRY: Duration = Duration::from_secs(30);`
  - `pub fn next_hour(hour: u32, min: u32, sec: u32, step: u8) -> Option<Duration>`: time from now (local hour, minute, second) to the next multiple of `step` hours strictly after now, clamped to 24:00; `None` when `step` is 0.
  - `pub fn hours_of(step: u8) -> Vec<u32>`: the hours of day (0 to 23) that are multiples of `step`, empty for 0.
  - `pub fn wait_range(preset: MascotPlay) -> Option<(u64, u64)>`: the preset's idle wait in minutes, `None` for Sapaan and Mati (the table now inside `wait_for`).
  - `cooldown(preset)` stays public as is.

- [ ] **Step 1: Write the failing tests** in `trigger.rs` tests:

```rust
#[test]
fn next_hour_counts_to_the_next_multiple_strictly_after_now() {
    let m = |h, mi, s| (h, mi, s);
    let cases = [
        (m(1, 30, 0), 1u8, 30 * 60),
        (m(1, 0, 0), 1, 3600),
        (m(23, 59, 59), 2, 1),
        (m(23, 59, 59), 6, 1),
        (m(18, 0, 0), 6, 6 * 3600),
        (m(17, 59, 59), 6, 1),
        (m(0, 0, 0), 3, 3 * 3600),
        (m(13, 10, 0), 4, 2 * 3600 + 50 * 60),
        (m(21, 0, 0), 5, 3 * 3600), // 5 does not divide 24: clamps at 24:00
    ];
    for ((h, mi, s), step, secs) in cases {
        assert_eq!(next_hour(h, mi, s, step), Some(Duration::from_secs(secs)), "{h}:{mi}:{s} step {step}");
    }
    assert_eq!(next_hour(10, 0, 0, 0), None);
}

#[test]
fn hours_of_lists_the_multiples_from_midnight() {
    assert_eq!(hours_of(0), Vec::<u32>::new());
    assert_eq!(hours_of(1).len(), 24);
    assert_eq!(hours_of(2), vec![0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22]);
    assert_eq!(hours_of(3), vec![0, 3, 6, 9, 12, 15, 18, 21]);
    assert_eq!(hours_of(4), vec![0, 4, 8, 12, 16, 20]);
    assert_eq!(hours_of(6), vec![0, 6, 12, 18]);
}

#[test]
fn wait_range_is_the_table_wait_for_draws_from() {
    assert_eq!(wait_range(MascotPlay::Sering), Some((8, 15)));
    assert_eq!(wait_range(MascotPlay::Sesekali), Some((15, 35)));
    assert_eq!(wait_range(MascotPlay::Jarang), Some((45, 90)));
    assert_eq!(wait_range(MascotPlay::Sapaan), None);
    assert_eq!(wait_range(MascotPlay::Mati), None);
}
```

The existing `wait_for` test (~362) must keep passing unchanged.

In `gate.rs` tests add:

```rust
#[test]
fn an_hour_story_ignores_the_cooldown_but_not_presence_or_the_gates() {
    let p = MascotPlay::Sesekali;
    let (input, none) = (Duration::from_secs(60), Duration::ZERO);
    assert!(may_start(&ok_env(), Moment::Hour, p, input, none), "cooldown skipped");
    assert!(!may_start(&ok_env(), Moment::Idle, p, input, none), "idle still cools down");
    assert!(!may_start(&ok_env(), Moment::Greeting, p, input, none), "greeting still cools down");
    assert!(!may_start(&ok_env(), Moment::Hour, p, Duration::from_secs(5), none), "mid-keystroke");
    assert!(!may_start(&ok_env(), Moment::Hour, p, Duration::from_secs(600), none), "away");
    for (name, env) in off_by_one_envs() {
        assert!(!may_start(&env, Moment::Hour, p, input, none), "{name}");
    }
    assert!(!may_start(&ok_env(), Moment::Hour, MascotPlay::Mati, input, none));
    assert!(may_start(&ok_env(), Moment::Hour, MascotPlay::Sapaan, input, none));
}
```

- [ ] **Step 2: Run** `cargo test --locked -p vale trigger:: gate::` (or `cargo test --locked play::`) → FAIL (items missing).

- [ ] **Step 3: Implement** the four items above in `trigger.rs` (`next_hour`: whole seconds, `((hour/step + 1) * step).min(24)` hours minus now) and refactor `wait_for` to read `wait_range`. In `gate.rs::may_start` replace `since_last >= cooldown(preset)` by `(moment == Moment::Hour || since_last >= cooldown(preset))` and update the function's doc comment to say an hour story skips the cooldown as well.

- [ ] **Step 4: Run** `cargo test --locked` → PASS.

- [ ] **Step 5: Commit** `feat(play): hour arithmetic, an hour story skips the cooldown`

---

### Task 3: The hour timer in the app

**Files:**
- Modify: `src/app.rs`: new `const TIMER_MASCOT_HOUR: usize = 36;` with a doc comment like `TIMER_MASCOT_PLAY`'s (check 36 is free with the test that lists every timer id (it sits above `the_mascot_play_timer_has_its_own_id`) and add it there); fields `hour_timer: bool`, `hour_deadline: Option<Instant>` beside `mascot_timer`; `sync_hour_timer`; `on_hour_timer`; the dispatch arm beside `TIMER_MASCOT_PLAY => ...` (~6004); `SettingsAction::MascotHours` handler beside `SettingsAction::MascotPlay` (~7201); `on_clock_jump` (~5818); remove `check_hour` (~2619), its call in the `TIMER_CLOCK` arm (~5963), the `clock_hour` field and its three assignments (~624, ~1139, ~4829, ~5825), `fn hour_rolled` (~9114) and the test `the_hour_moment_fires_when_the_hour_rolls_over_and_not_within_it` (~12387).
- Test: `src/app.rs` tests module.

**Interfaces:**
- Consumes: `trigger::{next_hour, HOUR_WINDOW, HOUR_RETRY}` (Task 2), `Config::mascot_hours` (Task 1), `SettingsAction::MascotHours(u8)` (Task 4 adds the variant; add the variant with its handler here as `Action::MascotHours(u8)` in `src/ui/settings.rs` if building Task 3 first, then Task 4 only adds the UI).
- Produces (private to `app.rs`):
  - `fn hour_timer_wanted(mascot_on: bool, preset: MascotPlay, step: u8) -> bool`: `mascot_on && step != 0 && preset != MascotPlay::Mati`. The pointer, presence, motion, panels and the cooldown are not read: they are judged when it fires.
  - `enum HourWait { Retry, Next }` and `fn hour_wait(started: bool, window_left: Duration) -> HourWait`: `Retry` only when not `started` and `window_left >= HOUR_RETRY`, else `Next`.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test]
fn the_hour_timer_is_wanted_only_with_a_step_a_live_preset_and_the_mascot_on() {
    use crate::theme::MascotPlay::{Jarang, Mati, Sapaan, Sering, Sesekali};
    for p in [Sering, Sesekali, Jarang, Sapaan] {
        assert!(hour_timer_wanted(true, p, 1), "{p:?}");
        assert!(hour_timer_wanted(true, p, 6), "{p:?}");
        assert!(!hour_timer_wanted(true, p, 0), "{p:?} step off");
        assert!(!hour_timer_wanted(false, p, 1), "{p:?} mascot off");
    }
    assert!(!hour_timer_wanted(true, Mati, 1));
}

#[test]
fn a_busy_hour_retries_inside_the_window_and_gives_up_after_it() {
    let s = |n| Duration::from_secs(n);
    assert!(matches!(hour_wait(false, s(600)), HourWait::Retry));
    assert!(matches!(hour_wait(false, s(30)), HourWait::Retry));
    assert!(matches!(hour_wait(false, s(29)), HourWait::Next));
    assert!(matches!(hour_wait(false, Duration::ZERO), HourWait::Next));
    assert!(matches!(hour_wait(true, s(600)), HourWait::Next), "started: on to the next hour");
}

#[test]
fn the_hour_timer_has_its_own_id() {
    assert_eq!(TIMER_MASCOT_HOUR, 36);
}
```

- [ ] **Step 2: Run** `cargo test --locked hour_` → FAIL.

- [ ] **Step 3: Implement.**
  - `sync_hour_timer(&mut self)`: idempotent. `wanted = hour_timer_wanted(<the same mascot-on value play_env uses for Env::mascot_on>, self.config.mascot_play, self.config.mascot_hours)`; use `timer_change(self.hour_timer, wanted)`; arming sets `TIMER_MASCOT_HOUR` to `next_hour(Local::now().hour(), minute, second, step)` and sets `hour_timer = true`; disarming kills it and clears `hour_timer` and `hour_deadline`. Call it at the very top of `sync_mascot_timer` (before the `pending` early return) so every existing call site (startup, lock, display, settings, `start_play`) keeps it in step; it must never re-set a timer that is already armed.
  - `on_hour_timer(&mut self)`: kill the timer, `hour_timer = false`; `deadline = *self.hour_deadline.get_or_insert(now + HOUR_WINDOW)`; `let was = self.play.is_some(); self.try_play(Moment::Hour, now); let started = !was && self.play.is_some();` then `match hour_wait(started, deadline.saturating_duration_since(now))`: `Retry` sets the timer `HOUR_RETRY` ahead and `hour_timer = true`; `Next` clears `hour_deadline` and arms the timer for `next_hour(...)` from the local wall clock.
  - `on_clock_jump`: add `self.hour_deadline = None;` and kill/clear the hour timer so the next sync re-arms from the new wall clock (no catch-up).
  - `SettingsAction::MascotHours(v)`: set `self.config.mascot_hours = v`, save, kill the timer and clear `hour_timer` and `hour_deadline`, then `self.refresh(now)` (which syncs).
  - Remove the dead hour code listed in Files.

- [ ] **Step 4: Run** `cargo test --locked` → PASS.

- [ ] **Step 5: Commit** `feat(app): mascot stories on the hour from their own timer`

---

### Task 4: Settings rows and texts

**Files:**
- Modify: `src/i18n.rs` (keys and two helper functions)
- Modify: `src/ui/settings_layout.rs` (`Hit`, `Group`, `State`, `mascot_play_card_for`, a `MASCOT_HOURS_ORDER` const; tests)
- Modify: `src/ui/settings.rs` (`Action`, model field, hit handling, group position, group list; tests)

**Interfaces:**
- Consumes: `trigger::{HOUR_STEPS, hours_of, wait_range, cooldown}` (Task 2), `Config::mascot_hours` (Task 1), `SettingsAction::MascotHours` (Task 3).
- Produces:
  - `Hit::MascotHours(u8)`, `Group::MascotHours`, `Action::MascotHours(u8)`, `State::mascot_hours: u8`, model `mascot_hours: u8` initialised from `cfg.mascot_hours`.
  - `pub const MASCOT_HOURS_ORDER: [u8; 6]` (equal to `HOUR_STEPS`).
  - `Key::MascotHours` ("Tiap jam" / "On the hour"), `Key::MascotHoursDesc`, `Key::MascotPlayRule`; `MascotPlayDesc` gets new text.
  - `pub fn mascot_play_info(lang: Lang, preset: MascotPlay) -> String` in `i18n.rs`: the preset's name and what it does, built from `wait_range` and `cooldown` so the minutes cannot drift (pattern of `greeting_periods_line`).
  - `pub fn hours_label(lang: Lang, step: u8) -> String`: "Mati"/"Off" for 0, else "N jam"/"N h".
  - `pub fn hours_line(lang: Lang, step: u8) -> String`: the live sentence, from `hours_of`.

Texts (exact):

| Item | Indonesian | English |
|---|---|---|
| `MascotPlayDesc` | Seberapa sering maskot bermain sendiri di sekitar notch. Klik kanan island untuk memainkannya kapan saja | How often the mascot plays on its own around the notch. Right-click the island to play one any time |
| `MascotPlayRule` | Maskot hanya mulai kalau kamu ada di PC tapi tidak mengetik, tidak ada panel terbuka, dan tidak layar penuh. | The mascot only starts when you are at the PC but not typing, no panel is open and nothing is fullscreen. |
| `MascotHours` | Tiap jam | On the hour |
| `MascotHoursDesc` | Maskot bermain tepat di jam bulat, dihitung dari 00.00 | The mascot plays on round hours, counted from 00:00 |
| info Sering | Sering: tiap 8 sampai 15 menit. Jeda minimal 6 menit. | Often: every 8 to 15 minutes. At least 6 minutes apart. |
| info Sesekali | Sesekali: tiap 15 sampai 35 menit. Jeda minimal 12 menit. | Sometimes: every 15 to 35 minutes. At least 12 minutes apart. |
| info Jarang | Jarang: tiap 45 sampai 90 menit. Jeda minimal 40 menit. | Rarely: every 45 to 90 minutes. At least 40 minutes apart. |
| info Hanya sapaan | Hanya sapaan: tidak ada cerita acak. Bermain setelah sapaan dan pada jam di bawah. | Greeting only: no random stories. Plays after the greeting and on the hours below. |
| info Mati | Mati: tidak pernah bermain sendiri. Klik kanan notch tetap bisa. | Off: never plays on its own. Right-click on the notch still works. |
| `hours_line` step 0 | Mati: maskot tidak bermain di jam tertentu. | Off: no story at set hours. |

`hours_line` otherwise: 8 hours or fewer, all of them: `Jam 00.00, 03.00, 06.00, 09.00, 12.00, 15.00, 18.00, 21.00.` / `At 00:00, 03:00, 06:00, 09:00, 12:00, 15:00, 18:00, 21:00.`; more than 8: the first six then ` dan N jam lagi.` / ` and N more.`, e.g. `Jam 00.00, 01.00, 02.00, 03.00, 04.00, 05.00 dan 18 jam lagi.`

- [ ] **Step 1: Write the failing tests:**
  - `i18n.rs`: `hours_line` asserts the four exact strings above (Id step 3, Id step 1, En step 6, En step 1) plus step 0 in both languages; `hours_label` for 0, 1, 6 in both languages; `mascot_play_info_names_the_minutes_the_timers_use`: for Sering, Sesekali, Jarang, both languages, the text contains each of `wait_range` lo, hi and `cooldown` minutes as decimal numbers (compare with the table above for the literal text).
  - `settings_layout.rs`, beside `mascot_play_row_has_its_own_order`: `mascot_hours_row_has_its_own_order` (segmented group `Group::MascotHours`, items equal `MASCOT_HOURS_ORDER.map(Hit::MascotHours)`, selected index of the state's value, both languages); `mascot_hours_row_is_dimmed_with_preset_mati_a_mascot_with_no_story_or_the_mascot_off` (the `Control::Segmented { group: Group::MascotHours, enabled: false, .. }` in those three cases, `enabled: true` otherwise); `the_play_card_stays_inside_the_card_in_both_languages` (every item of both segmented rows, and every line of the preset list, ends at or before the card's right edge at the narrowest window width the other layout tests use). Update `mascot_play_row_*` tests only where `MascotPlayDesc`'s text or the card height changes.
  - `settings.rs`: clicking `Hit::MascotHours(3)` with the model at 1 posts `Action::MascotHours(3)`, at 3 posts nothing; applying `Action::MascotHours(3)` sets the model field.

- [ ] **Step 2: Run** `cargo test --locked` → FAIL (compile errors for the missing items).

- [ ] **Step 3: Implement.**
  - `i18n.rs`: the three keys, the changed `MascotPlayDesc`, `mascot_play_info`, `hours_label`, `hours_line` (texts above).
  - `settings.rs` / `settings_layout.rs`: wire `MascotHours` wherever `MascotPlay` appears (grep `MascotPlay` in both files: `Hit`, `Group`, the `State` field, the model field and its init from the config, the group-position match, the group list, the hit handler, the no-op hit list, the action applier). Group position uses `MASCOT_HOURS_ORDER`.
  - `mascot_play_card_for`: keep the card, the title `GroupMascotPlay`, the label, the `MascotPlayDesc` lines and the five-item segmented row; under the segmented row add the five `mascot_play_info` lines wrapped to `content_w - 2 * INSET` (`b.wrap`, `Font::Desc`), the active preset's lines in `Tone::Primary`, the others `Tone::Secondary` (no new drawing primitive: this replaces the mockup's filled highlight), then the `MascotPlayRule` lines in `Tone::Secondary`; then `b.divider`, then a second row "Tiap jam": label `MascotHours`, `MascotHoursDesc`, its six-item segmented row (`Hit::MascotHours`, labels `hours_label`), then the `hours_line` text in `Tone::Primary`. Grow the card height from the wrapped line counts (`wrapped_row_h`, `DESC_LINE_H`), as the existing code does. Enabled: first row as today (`s.robot && ready`); hours row `s.robot && ready && s.mascot_play != MascotPlay::Mati`.

- [ ] **Step 4: Run** `cargo test --locked` → PASS.

- [ ] **Step 5: Run the app and look:** `cargo run --release -- --settings maskot`. Expected: the card shows the list and the hours row, the active preset is brighter, choosing "3 jam" changes the sentence, "Mati" dims the hours row. Check Indonesian and English and the light and dark window. Report what was and was not seen.

- [ ] **Step 6: Commit** `feat(settings): explain the mascot presets and add the hour row`

---

### Task 5: Memory and README

**Files:**
- Modify: `docs/memory/mascot-play.md` (the "When" bullet: the hour now has its own timer, `mascot_hours`, the window, no catch-up; remove the claim that the hour moment depends on the idle clock; date)
- Modify: `README.md` / `docs/guide.md` only where they describe the automatic animation (grep `maskot` and `Sesekali`); add one sentence for "Tiap jam"
- Modify: `docs/memory/index.md` only if a new note is added (not needed; this updates `mascot-play`)

- [ ] **Step 1:** Update the files above, keep the note's frontmatter and links.
- [ ] **Step 2:** Run `cargo test --locked` once more → PASS (docs only, but the project's merge flow wants green `main`).
- [ ] **Step 3: Commit** `docs: update memory and README`. (`src/whats_new.rs` is written only at release time.)
