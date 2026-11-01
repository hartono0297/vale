---
tags: [decision, cat, audio, motion, settings]
date: 2026-11-01
---

# The cat dances to the music

Owner request: the notch cat nods to the music, with a floating note ("A +
floating note", strength "Sedang"), and a setting "Joget ikut musik" (Settings
-> Kucing -> Gerak, right after "Ikuti kursor", on by default).

## What and where

Only the cat drawn with `Expr::Music` (the compact media view,
`View::MediaCompact`) and `Expr::Sing` (the lyric view, `View::MediaLyric`),
and only while the media plays and `TIMER_EQ` runs. The hover face (Happy),
paused media, idle, pomodoro, notices, the panel and the Settings preview are
drawn exactly as before (pose untouched, the note at its rest place, alpha 1).

Gate, `robot::dance_on(robot, cat_dance, motion, windows_animations, playing,
eq_timer_on, expr)`, a table in tests: the cat on, the setting on, `Motion`
effective Full (`App::motion()` already folds Energy Saver in, so Saver, Off
and Full-with-Energy-Saver are still), Windows' client-area animations on
(`App::animations`, the same signal the panel motion reads), playing,
`eq_timer` running, and the face one of Music/Sing.

## The nod (all pure, in `src/ui/robot.rs`, "Dance" section)

| Constant | Value | Meaning |
|---|---|---|
| `NOD_MAX_DIP` | 1.5 | the whole cat moves down this far at amount 1 |
| `SQUASH_W`, `SQUASH_H` | 0.04, 0.06 | width x (1 + 0.04 a), height x (1 - 0.06 a), round the head's base centre, grid (32, 54) |
| `BEAT_MS` | 600 | the fallback beat (100 BPM) |
| `NOTE_LOOP_MS` | 1200 | the note's loop |
| `NOTE_RISE_DIP` | 3.0 | at most this far up per loop |
| `NOTE_MARGIN_DIP` | 0.5 | room kept above the note |

- `nod_amount(level)`: 0..=1, NaN or negative is 0.
- **Bass vs fallback.** `App::eq_bass()` is band 0 of `EqShared` (60-150 Hz)
  while `eq_heights` would be live (capture wanted **and** `live`), 0 when
  the output is muted; with no live capture (setting "Equalizer ikut irama"
  off, no device, error) the cat nods to `fallback_beat(elapsed) =
  sin(pi t / 600 ms)^2` instead, 0 on each beat boundary and 1 mid-beat. The
  anchor is `App::dance_anchor`, reset whenever `TIMER_EQ` starts, so a dance
  starts at rest and the note at its place.
- **Note**: `note_phase(elapsed) -> (rise, fade)`: `p = (elapsed mod 1200
  ms) / 1200`, rise eases out (`1 - (1-p)^3`), fade is 0 for p < 0.5 then
  linear to 1; the loop start is the rest place, fully visible. The note's
  alpha is `1 - fade`.
- `Pose` gained `nod_dip`, `squash`, `note_dy_dip`, `note_fade` (zero is
  today's look; `Pose` derives `Default`) and two carrier fields, `dancing`
  and `note_rise` (the app knows the rise but not the shape's room).
  `Pose::dance(amount, note_rise, note_fade)` sets them.
- `RobotPainter::paint`: the squash comes first (grid space), then the look's
  tilt, bob and scale; the nod is part of the placement, so the Black rim,
  ears, head, calico patches, face marks and every accessory follow. The
  note's marks (`Line::Note` strokes and the cream `Dot`s) use their own
  matrix with the extra `note_dy_dip` (in dip, after the scale) and the
  alpha. The see-through legibility halo in `render.rs::paint_robot` shifts
  `head_rect` by `nod_dip`.

## The note clamp

The compact content is clipped to the shape rect, so the note must never
leave it. `notes_top(expr)` is the grid top of the note marks (Music 2.7,
Sing -7.3: the upper note at (64, 3) sits above the grid; a test checks it
against `mark_bounds`). `note_lift_dip(rest_top_room_dip, rise)`: room =
rest room - margin; with room >= 0 the note rises `rise * min(3, room)`; with
room < 0 it is shifted **down** by the shortfall and does not rise. The
renderer (`render.rs::note_placed`, called from `paint_compact` and
`paint_lyric`, which know the shape and the robot square) takes the room as
`area.y + nod + squashed top * k - shape.y` through
`robot::with_note_lift`, which sets `note_dy_dip` only for a dancing pose;
rest visuals are unchanged.

The 28-30 dip Sing case: the 26 px cat sits 1 dip from the top of a 28 dip
notch, so Sing's upper note is already above the shape at rest (clipped
today). While dancing it is pushed down to 0.5 dip from the top (about 2.5
dip at nod 0, less as the nod goes down) and does not rise. At that size the
lower note moves with it, and the Bow's box comes within 0.3 grid units of
it at the very worst (28 dip, no bass); the Bow's loop itself does not reach.
Taller notches (over 30 dip) rise normally.

`the_dancing_cat_stays_in_every_compact_shape` (`geometry.rs`) walks both
styles, every notch size (15 widths x 9 heights), MediaCompact and
MediaLyric at four widths, nod 0 and 1, rise 0, 0.5 and 1: the head outline
and ear tips (bare and with the Black rim), every accessory box and every
note box (after `note_lift_dip`) stay inside the shape.
`no_accessory_covers_a_faces_cream_extras` also sweeps the note's whole rise.

## TIMER_EQ also runs in the lyric view

`eq_timer_wanted(view_compact, view_lyric, dance_wanted, playing, eyes,
fullscreen, screen_away)`: the lyric view joins the compact one while the
cat dances there (`robot && cat_dance && motion().moves() && animations`).
No new timer id and no new `SetTimer`: it is the same `TIMER_EQ` (33 ms), and
the capture thread follows it as before (`eq_capture_wanted`), so loopback
capture can now run in the lyric view too. Cost: one 33 ms redraw while
playing in the lyric view (it was idle, only the lyric's own scroll and
crossfade clocks ran) plus the capture thread, both only with the cat on,
the setting on, full motion and Windows' animations on; with Energy Saver or
the setting off nothing changes. A change of `cat_dance` (Settings action ->
`refresh`), `robot_animation`, Energy Saver (`check_battery` -> `refresh`)
or the Windows animation setting (`WM_SETTINGCHANGE` now calls `refresh`
when the flag changes) re-runs `sync_timers`, so `TIMER_EQ` starts and stops
at once and the cat is drawn at rest when it stops (`dance_on` needs
`eq_timer`).

## Setting

`Config::cat_dance` (default on, lenient `default_on`, in `merge_owned`):
junk, null or a number read as on; only a real `false` is off; an old file
loads as on. Settings: `Hit::CatDance`, `Action::CatDance`, a slide toggle
like `CatFollow`, row "Joget ikut musik" / "Dance to music" with "Mengangguk
ikut irama saat musik diputar" / "Nods to the beat while music plays"; faded
and unclickable while the cat is off. The Gerak card grew one `ROW_H` with a
divider ([[settings-structure]]). The preview stays still.

## Demo

`--demo-robot`: `bata-musik-joget` (compact media, Music) and
`bata-lirik-joget` (lyric view, Sing), after `bata-lirik-long`: fixed pose
`dance(1.0, 0.5, 0.3)` (the deepest nod, the note half way up, 30 % faded);
they never capture (`eq_heights` stays `None`).

## Not verified live

The build must not touch the owner's audio or the running island. Not
checked: how the nod feels against a real song (bass level, the 600 ms
fallback), the lyric view's redraw cost, and the scenes on screen. The
release exe went from 2,744,832 to 2,748,416 bytes (+3,584, 0.13 %; the
baseline was built from `main` in another folder, see [[exe-size]]).

Related: [[eq-follow-audio]], [[bata-robot-expressions]], [[cat-accessories]],
[[cat-colours-and-surfaces]], [[bata-idle-power-budget]]

Listed in [[index]].
