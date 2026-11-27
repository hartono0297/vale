# Pengingat tidur (sleep reminder) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** At 01:00, 02:00, 03:00 and 04:00 the mascot shows a 6 s notice in the notch, getting sleepier each hour, nudging the owner to bed.

**Architecture:** A pure module `src/sleep_reminder.rs` holds the schedule and the gate decision. Three new mascot faces (`Drowsy`, `Yawn`, `Doze`) plus `Sleep` carry the hour's pose, with a one-shot motion in `Brain`. A new `NoticeKind::SleepReminder { hour }` shows it. `app.rs` arms its own one-shot `TIMER_SLEEP_REMINDER`, retries every 30 s inside a 10 min window, and makes the hour story skip hours 1-4.

**Tech Stack:** Rust 2024, Win32 + Direct2D, chrono `Local`.

**Spec:** `docs/superpowers/specs/2026-11-27-pengingat-tidur-design.md` (read it first; the mockup is `docs/design/maskot-pengingat-tidur.html`).

## Global Constraints

- Test command: `cargo test --locked` (focused: `cargo test --locked <name>`). Tests on drive Z: always fail on this PC; timing tests may flake under load, re-run them alone before calling a failure real.
- Disk is tight: every worktree builds into the one shared target dir `CARGO_TARGET_DIR=D:\Test\dynamic-island\target-shared` (set it in every cargo command). Run focused tests per task; the full suite runs once after merging.
- Texts, exact, ID / EN:
  - 01: "Sudah jam 1" / "Yuk mulai siap-siap tidur"; "It's 1 AM" / "Time to start winding down"
  - 02: "Jam 2, masih melek?" / "Tidur yuk, besok lanjut lagi"; "2 AM, still up?" / "Let's sleep, pick it up tomorrow"
  - 03: "Jam 3 pagi" / "Aku sudah ngantuk berat. Tidur yuk"; "3 AM" / "I can't keep my eyes open. Bed?"
  - 04: "Sebentar lagi subuh" / "Tidur dulu, walau sebentar"; "Almost dawn" / "Get some sleep, even a little"
  - Settings row: "Pengingat tidur" / "Sleep reminder"; desc "Jam 01.00 sampai 04.00, maskot ngajak tidur" / "1 to 4 AM, the mascot nudges you to bed"
- Plain ASCII in every new text (`ui::font_coverage`). No em dash.
- Notice shows 6 s. Window 10 min after the hour, retry every 30 s, an open panel stops blocking from 9 min 30 s.
- No timer runs between reminder hours ([[bata-idle-power-budget]]).
- Two writers never touch the same file at the same time; Tasks 1 and 2 run in parallel in separate worktrees, Tasks 3 and 4 run after both are merged.
- Commit trailer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. The PC wakes or the clock jumps to 02:20: nothing shows for 02 (no catch-up), the next one is 03:00. Test: `in_window` and `decide` cases in Task 2.
2. The owner was away and comes back at 01:06: the 01 reminder shows then (idle is a wait, not a skip). Test: `decide` in Task 2.
3. A "Claude butuh respon" sticky notice sits in the notch all night: reminders still show over it. Test: `timed_notice_up` in Task 4.
4. "Tiap jam" every 2 hours from 00:30: next story is 06:00, not 02:00 or 04:00. Test: `story_wait` in Task 2, and `hour_arm_ms` in Task 4.
5. The falk and octopus mid-yawn or mid-nod stay inside the 40 dip notice badge on every notch size. Test: clamp test in Task 1.

---

### Task 1: Three sleepy faces and their motion

Model: opus (new art and motion). Worktree: own branch off this one.

**Files:**
- Modify: `src/ui/robot.rs` (`Expr`, `Expr::ALL`, `blinks`, `looks`, `key`, `Line`, `Part`, `face`, `Pose`, `Brain`, the painter's `Part` match), `src/ui/mascot.rs` (`crest_pose`, `arms`, `beak_open`), `src/ui/cat_style.rs` and `src/ui/geometry.rs` (exhaustive `Part` matches, clamp test), `src/i18n.rs` (three `Key::Expr*`).
- Test: in those files' `#[cfg(test)]` modules.

**Interfaces:**
- Produces: `Expr::Drowsy`, `Expr::Yawn`, `Expr::Doze` (appended after `Typing`; `Expr::ALL: [Expr; 14]`). Task 3 maps hours 1/2/3/4 to `Drowsy`/`Yawn`/`Doze`/`Sleep`.
- Produces: `Brain::observe` starts the one-shot itself when a fresh notice token arrives with one of the three faces; callers change nothing.

- [ ] **Step 1: Write the failing tests**
  - `robot.rs`: `the_sleepy_faces_are_listed_and_named`: `Expr::ALL.len() == 14`; `Drowsy.blinks()`; `!Yawn.blinks() && !Doze.blinks()`; none of the three `looks()`; `key()` gives `ExprDrowsy`, `ExprYawn`, `ExprDoze`.
  - `robot.rs`: `the_sleepy_faces_have_their_marks`: `face(Drowsy, false).parts` holds two pills of `open(5.0, 5.6, 2.5)` plus `Part::Stroke(Line::Lids)`; `face(Yawn, false).parts` holds `Line::Sleep` and one `Part::Mouth`; `face(Doze, false).parts` holds `Line::Sleep` and no `Line::Zz`; `face(Drowsy, true)` shows `Line::Blink`.
  - `robot.rs`: `the_yawn_opens_then_closes`: with `observe(Some(Yawn), Some(1), t0, true)`, `pose(..).yawn` is `Some(0.0)` at 0 ms, `Some(1.0)` at 1 800 ms, `Some(0.0)` at 3 200 ms; `moving` is true at 5 900 ms and false at 6 100 ms; with `moves == false` `pose(..).yawn == None` (still pose = open mouth).
  - `robot.rs`: `the_doze_sinks_and_jerks_back_twice`: `nod_dip` rises from 0 at 0 ms to its max at 2 400 ms, is 0 again at 2 700 ms, max again at 5 400 ms; `tilt_deg` follows the same shape.
  - `robot.rs`: `the_drowsy_face_blinks_slowly`: `pose(..).blink` true at 1 300 ms and 3 100 ms, false at 500 ms and 2 000 ms.
  - `mascot.rs`: `the_falk_crest_droops_by_the_hour`: `crest_pose` rot/scale `Drowsy (30, 0.92)`, `Yawn (38, 0.9)`, `Doze (50, 0.85)`, `Sleep (62, 0.8)` unchanged; `beak_open(Yawn, &Pose::default())` true; `beak_open(Yawn, &Pose{yawn: Some(0.1), ..})` false.
  - `mascot.rs`: `the_octopus_curls_more_each_hour`: arm curl for `Drowsy` 1.2, `Yawn` 1.2, `Doze` 1.5, `Sleep` 1.8.
  - `geometry.rs`: extend `every_mascot_stays_in_the_notice_badge` with the three faces, each at rest, at `yawn: Some(1.0)` and at the doze's deepest `nod_dip`/`tilt_deg`.

- [ ] **Step 2: Run them, expect compile errors / FAIL**

Run: `cargo test --locked sleepy` then the four other names. Expected: FAIL (variants missing).

- [ ] **Step 3: Implement**
  - `Line::Lids`: one level stroke over each eye, `M L-3.6 Y h7.2` and `M R-3.6 Y h7.2`, width 2.0, espresso.
  - `Part::Mouth { cx: f32, cy: f32, r: f32 }`: espresso ellipse, cat at (32, 48.5), r 3.6; scaled vertically by `pose.yawn.unwrap_or(1.0)` when painted (0 = not drawn). It rides the eyes' transform, so the octopus's `eye_lift` moves it. Not drawn for the falk (its beak opens instead).
  - `face()`: `Drowsy` = `open(5.0, 5.6, 2.5)` + `Lids`, ears droop 2, bob 0.5, tilt 0; `Yawn` = `Sleep` eyes + `Mouth`, ears droop 2, bob 0, tilt -4; `Doze` = `Sleep` eyes, ears droop 4, bob 1.5, tilt 6.
  - `Pose::yawn: Option<f32>` (None = still pose, mouth fully open).
  - `Brain`: field `drowse: Option<Instant>`; `DROWSE_MS = 6000`; started in `observe` when `moves && fresh` and the face is one of the three; cleared when the face is none of them. `pose()`:
    - Drowsy: `blink` true in [1200, 1440) and [3000, 3240) ms.
    - Yawn: `yawn = Some(a)`, a = 0 until 600, eases up to 1 at 1400, holds to 2400, eases to 0 at 3000, 0 after.
    - Doze: two 3000 ms cycles: sink eases 0 to 1 over 2400 ms, jerks back to 0 over 200 ms, rests; `nod_dip = 2.5 * s`, `tilt_deg = 6.0 * s`.
    - `moving()`: `within(self.drowse, DROWSE_MS)` for every mascot.
  - `mascot.rs`: crest and curl values from Step 1; `beak_open`: `Yawn` true when `pose.yawn.map_or(true, |a| a > 0.3)`.
  - i18n: `ExprDrowsy => "Sayu", "Drowsy"; ExprYawn => "Menguap", "Yawning"; ExprDoze => "Terkantuk", "Dozing"`.
  - Every other exhaustive `match` on `Expr`, `Part` or `Line` the compiler names: treat the new ones like their nearest (`Drowsy` like `Sad`, `Yawn`/`Doze` like `Sleep`, `Mouth` like a `Dot`).
  - If a clamp test fails, shrink the motion (nod first), never the badge; record any tweak in the commit message.

- [ ] **Step 4: Run the Step 1 tests plus `cargo test --locked mascot` and `cargo test --locked robot`, expect PASS**

- [ ] **Step 5: Commit** `feat(mascot): drowsy, yawn and doze faces`

### Task 2: The schedule and gate rules (pure)

Model: haiku (pure functions, fully specified). Worktree: own branch off this one, parallel with Task 1.

**Files:**
- Create: `src/sleep_reminder.rs` (with its `#[cfg(test)] mod tests`)
- Modify: `src/main.rs` (add `mod sleep_reminder;` in alphabetical order)

**Interfaces:**
- Consumes: `crate::ui::play::trigger::next_hour(hour: u32, min: u32, sec: u32, step: u8) -> Option<Duration>` (target `((hour/step + 1) * step).min(24)`, `None` for step 0).
- Produces (all `pub`):
  - `const WINDOW: Duration = 600 s; const RETRY: Duration = 30 s; const LAST_TRY: Duration = 570 s; const ARM_MARGIN_MS: u32 = 1000; const AWAY_AFTER: Duration = 600 s;`
  - `fn is_reminder_hour(hour: u32) -> bool` (1..=4)
  - `fn next_ms(hour: u32, min: u32, sec: u32) -> u32`: ms to the next hh:00:00 with hh in 1..=4, strictly after now, plus `ARM_MARGIN_MS`.
  - `fn in_window(hour: u32, min: u32, sec: u32) -> Option<(u8, Duration)>`: `Some((hour, since_hour))` when the hour is a reminder hour and `since_hour < WINDOW`.
  - `struct Gates { screen_away: bool, fullscreen: bool, recording: bool, idle_for: Duration, menu_open: bool, timed_notice: bool, panel_open: bool }` (`Clone, Copy, Debug, Default`)
  - `enum Step { Show, Retry, Drop }` (`Debug, PartialEq, Eq`)
  - `fn decide(g: &Gates, since_hour: Duration) -> Step`
  - `fn story_wait(hour: u32, min: u32, sec: u32, step: u8, skip_night: bool) -> Option<Duration>`: `next_hour`'s wait, stepped on while the target hour (mod 24) is a reminder hour and `skip_night`.

- [ ] **Step 1: Write the failing tests**
  - `next_ms`: (0,0,0) → 3 601 000; (0,59,59) → 2 000; (1,0,0) → 3 601 000 (to 02:00); (3,30,0) → 1 801 000; (4,0,0) → 21 h + 1 s = 75 601 000; (23,0,0) → 2 h + 1 s.
  - `in_window`: (1,0,1) → Some((1, 1 s)); (2,9,59) → Some((2, 599 s)); (2,10,0) → None; (0,5,0) → None; (5,0,1) → None; (4,3,0) → Some((4, 180 s)).
  - `decide`: all clear → Show; each of `screen_away`, `fullscreen`, `recording`, `menu_open`, `timed_notice`, `idle_for = AWAY_AFTER` alone at 60 s → Retry; the same at 575 s → Drop (no room for another 30 s); `panel_open` at 60 s → Retry, at 570 s → Show; anything at `WINDOW` → Drop; `idle_for` 599 s at 60 s → Show.
  - `story_wait`: (0,30,0, step 1, true) → wait to 05:00 (4 h 30 min); (0,30,0, 1, false) → 30 min; (0,30,0, 2, true) → to 06:00; (22,0,0, 2, true) → to 00:00 (2 h); (4,10,0, 1, true) → 50 min (05:00); step 0 → None.

- [ ] **Step 2: Run `cargo test --locked sleep_reminder`, expect FAIL**

- [ ] **Step 3: Implement the functions above**

`decide`: `since_hour >= WINDOW` → Drop; blocked = any flag, `idle_for >= AWAY_AFTER`, or `panel_open && since_hour < LAST_TRY`; blocked → `Retry` if `since_hour + RETRY < WINDOW` else `Drop`; otherwise `Show`.

- [ ] **Step 4: Run `cargo test --locked sleep_reminder`, expect PASS** (an unused-code warning is fine until Task 4)

- [ ] **Step 5: Commit** `feat(sleep): schedule and gate rules for the sleep reminder`

### Task 3: The notice, its texts, the config and the Settings row

Model: sonnet. Runs after Tasks 1 and 2 are merged into this branch.

**Files:**
- Modify: `src/model.rs` (`NoticeKind`), `src/state.rs` (duration, reducer, every `Greeting` arm), `src/ui/geometry.rs` (`badge_style`), `src/ui/robot.rs` (`NoticeSort`, `choose_expr`), `src/ui/demo.rs`, `src/i18n.rs`, `src/config.rs`, `src/config_transfer.rs`, `src/ui/settings.rs`, `src/ui/settings_layout.rs`, `src/app.rs` (only the new `SettingsAction` arm).

**Interfaces:**
- Consumes: `Expr::Drowsy/Yawn/Doze` (Task 1).
- Produces: `NoticeKind::SleepReminder { hour: u8 }`; `i18n::sleep_reminder_title(lang: Lang, hour: u8) -> &'static str`; `i18n::sleep_reminder_body(lang: Lang, hour: u8) -> &'static str` (hour outside 1..=3 reads as 4); `Key::SleepReminder`, `Key::SleepReminderDesc`; `Config::sleep_reminder: bool` (default `true`); `Hit::SleepReminder`; `settings::Action::SleepReminder(bool)`; in `app.rs` the arm `SettingsAction::SleepReminder(on) => { self.config.sleep_reminder = on; self.save_config(); }` (Task 4 adds the timer sync to it).

- [ ] **Step 1: Write the failing tests**
  - `i18n.rs`: `the_sleep_reminder_texts_in_both_languages`: the eight titles/bodies of Global Constraints, exact; add the eight texts and the two keys to `no_formatted_text_leaks_the_other_language`.
  - `robot.rs`: `NoticeSort::of(&SleepReminder{hour})` then `choose_expr` gives `Drowsy`, `Yawn`, `Doze`, `Sleep` for 1, 2, 3, 4.
  - `state.rs`: `a_sleep_reminder_shows_six_seconds` (`duration_of` = 6 s); `a_sleep_reminder_is_dropped_in_fullscreen` (mirror the Greeting test at ~3856); `a_sleep_reminder_shows_over_a_sticky_notice` (a `ClaudeWaiting` notice up, then a SleepReminder: `view()` shows the reminder, the sticky is back after it expires).
  - `config.rs`: `sleep_reminder` defaults to true, survives `merge_owned`; `config_transfer` lists it in `EXPORTED` (`every_config_key_is_classified` passes).
  - `settings_layout.rs`: update `gerak_and_sapaan_rows_follow_their_settings` and `robot_rows_follow_the_setting`: the Sapaan card has a second toggle row `Hit::SleepReminder` labelled `Key::SleepReminder` with `Key::SleepReminderDesc`, below the Sapaan row and above the periods note; no hit while the robot is off.

- [ ] **Step 2: Run the named tests, expect FAIL**

- [ ] **Step 3: Implement**
  - Everywhere `NoticeKind::Greeting` is matched or tested, give `SleepReminder` the same behaviour (fullscreen drop, close ✕, click), except: duration `SLEEP_REMINDER_DURATION = 6 s`; `NoticeSort::SleepReminder(u8)` with the face by hour; `greeting_ended` stays Greeting-only (no story after a reminder).
  - `badge_style`: the Greeting's tint; use a moon icon if `Icon` has one, else `Icon::Sparkle`.
  - Demo: `DemoNotice::SleepReminder` with hour from `DI_SLEEP_HOUR` (1..=4, default 2).
  - Config field with `#[serde(default = ...)]` true, `merge_owned`, `EXPORTED`.
  - Settings: the row inside the Sapaan card, built with `toggle_if(Hit::SleepReminder, ..., s.sleep_reminder, on)` and `row_text`, same pattern as the Sapaan row; model field, `from_config`, model→state, reducer, click, `slide_toggle`.

- [ ] **Step 4: Run the Step 1 tests and `cargo test --locked settings` and `cargo test --locked notice`, expect PASS**

- [ ] **Step 5: Commit** `feat(sleep): sleep reminder notice, texts and Settings row`

### Task 4: The timer, the gates and the hour story skip

Model: sonnet. After Task 3.

**Files:**
- Modify: `src/app.rs`, `src/state.rs` (one accessor).

**Interfaces:**
- Consumes: everything Task 2 and Task 3 produce.
- Produces: `const TIMER_SLEEP_REMINDER: usize = 37;` field `sleep_timer: bool`; `fn sync_sleep_timer(&mut self)`, `fn on_sleep_timer(&mut self)`; `State::timed_notice_up(&self) -> bool` (a timed notice, not a sticky one, is current); `hour_arm_ms(hour, min, sec, step, skip_night: bool) -> Option<u32>`.

- [ ] **Step 1: Write the failing tests**
  - `app.rs`: add `TIMER_SLEEP_REMINDER` to the unique-ids list and `assert_eq!(TIMER_SLEEP_REMINDER, 37)`.
  - `app.rs`: `hour_arm_ms(0, 30, 0, 1, true) == Some(16_201_000)` (05:00 + 1 s); `hour_arm_ms(0, 30, 0, 1, false) == Some(1_801_000)`; `hour_arm_ms(0, 30, 0, 2, true) == Some(19_801_000)`.
  - `state.rs`: `timed_notice_up` false with nothing, true with a Greeting up, false with only a `ClaudeWaiting` sticky up.

- [ ] **Step 2: Run them, expect FAIL**

- [ ] **Step 3: Implement**
  - `sync_sleep_timer`: wanted = `config.sleep_reminder && config.robot`; arm `SetTimer(TIMER_SLEEP_REMINDER, sleep_reminder::next_ms(now local))` when wanted and not set, kill when set and not wanted (the `timer_change` pattern of `sync_hour_timer`). Call it right after `self.sync_hour_timer()` in `sync_mascot_timer`; in `on_clock_jump` kill it (`sleep_timer = false`) and sync, like the hour timer.
  - `on_sleep_timer` (dispatched from `on_timer`): kill; ignore if `!self.sleep_timer` (stale tick); set false. `in_window(local now)`: `None` → `sync_sleep_timer()`. `Some((hour, since))` → build `Gates` (`screen_away()`, fullscreen, `hotkeys::any_recording(...)`, `idle_for` = the same last-input measure `play_env` uses, `MENU_OPEN`, `state.timed_notice_up()`, panel = `View::Panel(_)`) and `decide`:
    - `Show`: `apply_now(Event::Notice(Notice { key: "sleep-reminder", source: tr(Key::SourceVale), title: sleep_reminder_title(lang, hour), body: sleep_reminder_body(lang, hour), kind: SleepReminder { hour } }))`; if `state.view()` now shows it, `sync_sleep_timer()`; else treat as Retry.
    - `Retry`: `SetTimer(..., RETRY ms)`, `sleep_timer = true`.
    - `Drop`: `sync_sleep_timer()`.
  - `SettingsAction::SleepReminder` arm: after saving, `self.stop_hour_timer(); self.refresh(now);` (re-arms both timers with the new rule).
  - `hour_arm_ms` uses `sleep_reminder::story_wait(..., self.config.sleep_reminder && self.config.robot)` in place of `next_hour`, keeping `HOUR_ARM_MARGIN`.

- [ ] **Step 4: Run the Step 1 tests and `cargo test --locked hour` and `cargo test --locked sleep`, expect PASS; `cargo build --release --locked` has no new warnings**

- [ ] **Step 5: Commit** `feat(sleep): sleep reminder timer and hour story skip`

### After the tasks

- Whole-branch review (opus), full `cargo test --locked` on `main` after the merge, then docs (memory notes: new `sleep-reminder.md`; update `mascot-hour-schedule.md`, `greeting-periods.md`, `bata-robot-expressions.md`, `settings-structure.md`, index; README if it lists greeting features) per AGENTS.md step 5.
- Owner checks by hand: `--demo` with `DI_SLEEP_HOUR=1..4` for each mascot; the Settings row.
