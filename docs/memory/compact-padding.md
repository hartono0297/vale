---
tags: [decision, geometry, notch]
date: 2026-10-31
---

# Compact padding: one 16 dip inset

The compact views used uneven side insets (art 6, bars 14, ring 22, robot 6).
They now share one: `COMPACT_INSET` = 16 = `NOTCH_COMPACT_RADIUS`
(`src/ui/geometry.rs`), the same in the Pill style. Content never sits closer
to an end than the Notch's bottom curve reaches.

## Who uses it

- **MediaCompact**: art (or robot) 16 from the left, equalizer bars end 16
  from the right (`compact_layout`, `robot_slot`). `ROBOT_INSET` is gone.
- **PomodoroCompact**: the ring's left edge 16 in (centre `s.x + 16 + 8`);
  with the robot the robot sits at 16 and the ring follows it as before
  (centre `s.x + 58`). The text's right edge stays 18 from the right.
- **MediaLyric with the robot**: robot at 16, line box from 50
  (`lyric_lead(true)` = 16 + 26 + 8). Right padding stays 20.
- **Not used**: Idle (centred group), Notice, NoticeShot, every Panel kind,
  Eyes, `ROBOT_NOTICE`. Idle keeps the user's width exactly.

## Why option A: the shapes grow, the text room does not

Only the shapes changed, so the title and lyric text keep exactly the room
they had at every `NotchSize` (a test recomputes today's layout from the old
constants and compares).

| Shape | Before | Now |
|---|---|---|
| MediaCompact | w | w + 12 (`MEDIA_COMPACT_EXTRA`); `robot_extra` stays 2 |
| PomodoroCompact | w + 20 | w + 22 (`POMODORO_COMPACT_EXTRA`); `robot_extra` 26 -> 34 |
| MediaLyric with robot | W | W + 10 (`LYRIC_ROBOT_EXTRA`); no robot unchanged |
| Idle | w | w |

Text widths: media `w - 62 - bars_w` and pomodoro `w - 38`, both with and
without the robot, as before. The widest island (300 + 12 + 2 = 314 dip) still
fits Settings' preview "screen" (464 dip, with the 48 dip checker margin);
a test in `settings_layout.rs` guards it.

## Gotcha: the lyric clamp

`lyric_width_min` clamps the width the line wanted with the *old* lead (40) to
`[min_w, max_w]` exactly as before and adds the extra 10 *outside* the clamp,
so the box is never narrower at the minimum or maximum. A consequence:
with the robot the notch can reach `max_w + 10`, and `LyricFit::line` in
`app.rs` still caps at `max_w.max(min_w)`, so for a line that already hits the
maximum the box there is 10 narrower than before (it scrolls anyway), while
`refit` / `set_min` give `max_w + 10`. A one-line fix in `LyricFit::line`
(cap + `LYRIC_ROBOT_EXTRA` with the robot) would make them agree.

Related: [[notch-size-and-fill]], [[lyric-notch-width]]. Listed in [[index]].
