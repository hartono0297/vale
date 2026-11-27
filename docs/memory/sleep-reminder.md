---
tags: [decision, mascot, timer, power, greeting]
date: 2026-11-27
---

# Pengingat tidur (sleep reminder)

Owner request: a notice at the late hours telling them to go to sleep, from
the mascot. Spec `docs/superpowers/specs/2026-11-27-pengingat-tidur-design.md`,
mockup `docs/design/maskot-pengingat-tidur.html`. Pure rules in
`src/sleep_reminder.rs`; wiring in `app.rs`.

- Hours 01, 02, 03, 04 only, a notice of 6 s (`NoticeKind::SleepReminder
  { hour }`, texts `i18n::sleep_reminder_title/body`). The face escalates by
  hour: Drowsy, Yawn, Doze (the cat's ears, the falk's crest and the octopus's
  curl follow, see [[bata-robot-expressions]], [[maskot-falk-gurita]]).
- `Config::sleep_reminder` (default on, `merge_owned`, exported). Settings →
  Maskot → Sapaan card, own row ([[settings-structure]]).
- **Own one-shot `TIMER_SLEEP_REMINDER` (37)**, independent of "Tiap jam",
  motion and Energy Saver. Why: the hour story's timer only lives while a story
  may play ([[mascot-hour-schedule]]), so the reminder could not ride on it.
  It is armed at startup too (`sync_mascot_timer` at the end of init; the first
  version only armed on the first `refresh()`).
- Gates never skip at once: each waits inside a 10 min window, retried every
  30 s (`WINDOW`, `RETRY`, `LAST_TRY`). Sticky notices never block. An open
  panel blocks until 9:30 past the hour.
- "Done" means actually shown (a view check) or dropped, kept in `sleep_done`
  as (date, hour). A wake or clock jump inside an unfinished window retries at
  once: the final review found the first version lost the hour after a wake.
- With the reminder on, the hour story skips 01 to 04 when it arms
  (`story_wait`, `hour_arm_ms` `skip_night`), so the two never collide.
- The falk yawns with its beak (`Pose::yawn`); the mouth is not drawn in the
  Jadi muka face (`face_marks` filters it). The face sheet is now seven faces
  per row.
- Declined by the owner: a "Cukup malam ini" snooze chip.
- Demo: `DI_SLEEP_HOUR`, scene `bata-pengingat-tidur`.
- Gotcha: the build target dir went to C: because D: was full
  ([[parallel-worktrees-shared-target]]).

Related: [[greeting-periods]], [[bata-idle-power-budget]]. Listed in [[index]].
