---
tags: [design, mascot, settings]
date: 2026-11-18
---

# Mascot stories on the hour, and what each preset means

Mockup: `docs/design/maskot-jadwal-jam.html` (explain list B, hours option 1).
Memory: [[mascot-play]].

## Intent

Owner request (2026-11-18): the "Animasi maskot otomatis" setting does not say
what Often / Sometimes / Rarely mean, and they want the mascot to play on the
hour (1:00, 2:00, 3:00, or every 2 hours), as the user sets it.

Today an "hour moment" already exists but is hidden and flaky: `check_hour`
runs from `TIMER_CLOCK`, which only ticks while the idle clock is on screen, and
the hour is simply skipped when the user is typing, a panel is open or the
cooldown is on. The user cannot see it or change it.

## Goals

- Settings explains every preset in plain words (how often, the least gap, when
  it starts), Indonesian and English.
- A new row "Tiap jam" / "On the hour": Off, 1, 2, 3, 4 or 6 hours, on round
  clock hours counted from 00:00 (every 2 hours is 02:00, 04:00 ...). A live
  sentence under it lists the hours.
- Default is every 1 hour, so users who have the app see no change.
- The hour plays even when it was busy at the exact minute (see Rules).

## Non-goals

- No per-hour picker, no from/to range, no offset (every 2 hours starting at
  an odd hour). The presence rule already keeps it off when nobody is there.
- No change to the idle timer, the stories, the greeting or "Mainkan maskot".
- No catching up after sleep.

## Anti-goals

- A timer running at idle ([[bata-idle-power-budget]]).
- The hour depending on the idle clock being on screen.

## Approaches

1. **Own wall-clock one-shot (chosen).** One timer set to the next scheduled
   hour, retried every 30 s inside a 10 minute window, then set for the next
   hour. Works with the clock off or a panel open. One timer at a time.
2. Keep `check_hour` on `TIMER_CLOCK` and add the step. Smallest diff, but it
   keeps the flaw: no clock on screen, no hour.
3. Fold the hour into the idle timer's random wait. Not "on the hour".

## Design

- **Config**: `mascot_hours` (u8: 0 off, 1, 2, 3, 4, 6; default 1). A bad value
  reads as 1. Added to `config_transfer` next to `mascot_play`. Not
  user-visible anywhere else.
- **Pure core** (`src/ui/play/trigger.rs`): `next_hour(now: NaiveTime-like
  hour/min/sec, step) -> Duration` to the next multiple of `step` hours after
  now; `HOUR_WINDOW` = 10 min; `HOUR_RETRY` = 30 s. Unit-tested at the edges
  (23:59:59 with 2 and 6, exactly on the hour, step 0).
- **Gate** (`gate.rs`): `Moment::Hour` is allowed by the same preset rule as
  now (Mati never; the others and Sapaan yes). The hour skips the cooldown (it is
  the owner's schedule) and still needs the presence rule (idle 15 s to 5 min),
  the transient gates and every permanent gate. A story that ends re-arms the
  cooldown for idle stories as before. Tests: Hour with a cooldown running
  starts; Hour while typing waits; `off_by_one_envs` stays complete.
- **Timer** (`app.rs`): a new one-shot `TIMER_MASCOT_HOUR`, wanted only while
  `mascot_hours != 0`, the preset is not Mati and the mascot is on with a ready
  story (not the pointer, presence or cooldown: judged on fire). Fire:
  - first fire of an hour opens the window (`hour_deadline = now + 10 min`);
  - `try_play(Moment::Hour)`; if it did not start and the deadline is not past,
    set the timer 30 s ahead; otherwise set it for `next_hour`;
  - after a start, set it for `next_hour`.
  The wake, a clock or zone change (`on_clock_jump`) and a settings change
  recompute it from the new wall clock and open no window: a missed hour is
  skipped. `check_hour`, `clock_hour` and `hour_rolled` go away, they are replaced.
- **Settings** (`settings_layout.rs`, `settings.rs`, `i18n.rs`): the preset row
  loses `MascotPlayDesc` for a short line plus a five-line list (one per
  preset, the active one highlighted, "name: when. gap"), then the shared rule
  line. A second row "Tiap jam" with a segmented control (Mati / 1 / 2 / 3 / 4 /
  6 jam) and the sentence "Jam 02.00, 04.00 ...". The row is dimmed with the
  preset row when the mascot is off, has no ready story, or the preset is
  Mati. New `Hit`/`Action`/`SettingsAction::MascotHours`, `Group::MascotHours`.
  Strings in both languages; the layout test for the card's rows is updated.
- **Texts**: Sering 8 to 15 min (gap 6), Sesekali 15 to 35 (12, default),
  Jarang 45 to 90 (40), Hanya sapaan: no random stories, plays after the
  greeting and on the hours, Mati: never alone, right-click still works. Read
  from the same constants as `trigger::wait_for` and `cooldown` so the numbers
  cannot drift.

## Rules (what happens at the hour)

| At the hour | Result |
|---|---|
| Owner idle 15 s to 5 min, nothing open | plays now |
| Typing, a panel or notice open, menu, pointer on the island | waits, tries every 30 s for 10 min, then skips to the next hour |
| Owner away more than 5 min | no story; if they come back inside the window it plays once they pause 15 s |
| Sleep, lock, fullscreen, recording, Energy Saver, motion not Full | skipped, never made up |
| Another story is running | waits inside the window |

## Left to the builder

Names of the constants and keys beyond those above, the exact pixel heights of
the new card rows (reuse `wrapped_row_h`), and where the timer id sits in the
`TIMER_*` table (`the_mascot_play_timer_has_its_own_id` checks uniqueness).

## Verify

`cargo test --locked` (new unit tests above, config lenient read and transfer,
settings layout for both languages). Not checkable here: a real story at a real
hour on screen; the owner checks one at the next hour.
