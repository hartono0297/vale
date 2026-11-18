---
tags: [decision, mascot, timer, power]
date: 2026-11-18
---

# Mascot story on the hour

Spec `docs/superpowers/specs/2026-11-18-jadwal-jam-maskot-design.md`. Part of
[[mascot-play]].

- `Config::mascot_hours` (0 off, 1, 2, 3, 4, 6; default 1; a bad value reads as
  1). Hours count from 00:00 local, so every 2 hours is 02:00, 04:00.
- Its own one-shot `TIMER_MASCOT_HOUR`, armed only while the hours are on, the
  preset is not Mati and the durable gates hold (`hour_timer_wanted`: mascot on,
  motion Full, no Energy Saver, Windows animations on, not Jadi muka, a story
  ready). With one failing no timer is set at all; `sync_mascot_timer` calls
  `sync_hour_timer`, so any change of those flags arms or kills it. It does not depend on the idle clock:
  the old `check_hour` on `TIMER_CLOCK` only ran while the clock was on screen,
  so with the clock off there was no hour. It is gone.
- **Arm 1 s past the hour** (`hour_arm_ms` in `app.rs`; `trigger::next_hour` is the pure part). A one-shot can fire a few
  ms early; armed exactly on the hour it would land before it and open a second
  window for the same hour.
- On fire, `hour_skips` sends gates that last (lock or screen off, fullscreen,
  recording, the cat asleep, or a durable gate failing) straight to the next
  hour: no window, no retry. Only the short gates wait: a window of 10 minutes,
  retried every 30 s (typing or no presence, a panel or notice, the menu, the
  pointer, shrunk eyes, the typing face, a changed mascot, a running story).
  The hour skips the cooldown but needs presence and every other gate.
- `on_hour_timer` ignores a tick queued before `stop_hour_timer` (it checks
  `hour_timer`, like `on_typing_timer`).
- No catch-up: sleep, lock or a missed window skips the hour. `on_clock_jump`
  re-arms at once from the new wall clock and opens no window.
- Power: nothing runs at idle between hours ([[bata-idle-power-budget]]).
