---
tags: [decision, robot, pointer, motion, power]
date: 2026-11-02
---

# The idle cat looks toward the pointer

The cat in the idle notch turns its head toward the pointer, sideways only, with
an eased move. It shares one poll with the notch peek ([[notch-peek-eyes]]);
the cost rules are in [[bata-idle-power-budget]], the faces in
[[bata-robot-expressions]].

## When

`App::look_eligible`: `config.robot` on, `motion().moves()` (Full, and not
Energy Saver), the view is **Idle**, the cat is drawn there (`robot_frame` is
Some), the peek is not shrunk, not fullscreen, not `screen_away`. Idle only
(assumption): in the compact music, lyric and timer views the cat's slot is
tight, so it stays still there. Only a face that looks follows
(`Expr::looks`): of the idle faces Idle, Happy and Sad (the typing face only
glances with its eyes, see the last section). Think and Sleep stay
centred, since their thought dots and z's float beside the right ear and
would slide 4 to 7 dip into the clock; a sleeping cat also stops the look's
ease and frames. `robot_frame` draws them with `Pose::looking_as`, which keeps
their pose centred even mid-ease.

## Maths (`pointer_watch.rs`, tested)

- `look_target(pointer_x, notch_centre_x, monitor_left, monitor_right)` in
  [-1, 1]: the horizontal offset over the distance from the notch centre to
  that side's monitor edge, clamped. Pointers on another monitor clamp to +-1.
  The island sits at the notch's monitor's centre; the poll's quick pace
  also needs the pointer inside that monitor's X span
  (`pointer_watch::near_top`).
- `LookEase { current, target }`: `set_target` ignores a change under
  `LOOK_EPS` (0.02) and returns false (no redraw, no clock); `advance(dt)` is an
  exponential ease with tau 120 ms (`1 - e^(-dt/tau)`, the same at any frame
  rate, never overshoots), snapping within 0.002; `snap_to_centre()` puts
  both at 0 at once.
- `look_hidden(lowered, covered)`: the island is lowered by stay-behind and a
  window lies over it; `covered` is read only when lowered.
- `Pose::looking(v)` (`ui/robot.rs`): `head_dx_dip = 7 v`, `tilt_deg = 6 v`,
  `eye_dx_dip = 4 v` (constants `LOOK_HEAD_DIP`, `LOOK_TILT_DEG`,
  `LOOK_EYE_DIP`). `RobotPainter::paint` slides and tilts the head and ears
  (the tilt adds to the face's own, around the same pivot (32, 40)) and slides
  the face's marks by the extra eye offset, all converted with `k = area.w /
  GRID`.

## The clamp

`the_looking_cat_stays_in_the_idle_island` (geometry.rs) draws every face the
idle view shows (Idle, Happy, Think, Sad, Sleep; open-eyed and blinking) as
`RobotPainter::paint` does: the head outline, both ears turned for the face's
droop with their round strokes, and the box of every face mark (the eyes and
what floats beside the head) slid by the eye offset, all under the face's own
tilt and bob plus the look's tilt and slide (`Pose::looking_as`). Head and ears
must stay inside the idle island, and everything left of the clock box, for
both styles, every notch size (160..300 x 28..44), with or without the clock,
at v = -1..1.

The head slide came down from 8 to 7 dip: at 8 the sad cat's drooped right ear
tip, swung right by the 6 degree tilt, reached about 0.3 dip into the clock
box. The extras need no reduction because Think and Sleep do not look (at
full look they reached 4 to 7 dip into the clock). If the idle layout changes
(a wider clock, a smaller gap) or a face gains a mark, that test says whether
`LOOK_HEAD_DIP` must come down.

## Cost

The poll (`TIMER_POINTER`) is slow far from the top edge (250 ms) and 33 ms
within 150 dip of it. The look itself only runs the `FrameClock` while the ease
moves: a poll that changes the target by 0.02 or more calls `ensure_clock`;
`animate` eases the look before each draw (`advance_look`); `animating_at`
counts `look.moving()`. When the cat stops being eligible, `sync_look` snaps
the look to 0 with no ease. There is no eased return: every way to lose
eligibility either hides the cat (off, another view, the eyes, fullscreen),
leaves nobody to see it (locked, display off), stops motion, or turns to a
face that `Pose::looking_as` draws centred anyway. (An earlier eased-release
branch tested the same conditions as `look_eligible`, so it could never run;
it and `LookEase::release` were removed.)

With "Di belakang jendela" on, the poll keeps running while stay-behind has
lowered the island to `Z::Bottom` (the cat may still be seen on the desktop),
but `on_pointer_timer` sets no target while a window lies over the island
(`App::look_hidden_behind`: `island_covered`, the window at the island's
centre, cached for `UNDER_RECHECK`, 1 s, in `cover_cache`, cleared whenever
the island goes up or down). So a maximized window over the notch costs the
poll's `GetCursorPos` and at most one `WindowFromPoint` a second, never the
frame clock or a draw. The look stays where it was and goes on from there
once the island rises or the window moves away.

v3.8: **"Ikuti kursor"** (`cat_follow_pointer`, Settings → Kucing → Gerak, on
by default) is part of "When": `look_eligible` needs
`pointer_watch::look_allowed(robot, cat_follow_pointer)`; off, the cat stays
centred and the poll may stop ([[bata-idle-power-budget]]). The clamp test
also takes every accessory's box now ([[cat-accessories]]); `LOOK_HEAD_DIP`
stayed 7. Accessories follow the same slide and tilt (the glasses the eyes').

The same toggle also drives the **shrunk notch's eyes** (v3.11, see
[[notch-peek-eyes]]): while the notch is shrunk and "Ikuti kursor" is on, the
eyes slide up to 4 dip across and 1.5 dip down toward the pointer. They do not
need the cat (`config.robot`) and are gated by `pointer_watch::eyes_motion`
(eased on the frame clock with Motion Full and Windows animations, otherwise
they jump); the cat's own look stays off while shrunk. The row's description
reads "Kepala dan mata menoleh ke kursor".

## The typing cat glances (2026-11-01)

Since work running gives `Expr::Typing` instead of Think ([[cat-types-while-working]]),
the cat that "did not follow the cursor after the music ended" (it was Think,
which never looks) does follow, but only with its **eyes**. The head, ears,
body and laptop never move (`Expr::looks()` stays false for Typing).

- **Eligibility** is the same pure rule as for the looking cat:
  `pointer_watch::look_on(allowed, moves, idle_view, shrunk, fullscreen,
  screen_away, expr)`, with `expr.follows()` = `looks() || glances()`.
  `App::look_eligible` calls it: "Ikuti kursor" and the cat on, Motion Full,
  the idle view, not shrunk, not fullscreen, not `screen_away`.
- **Target**: `glance_target(pointer_x, pointer_y, centre_x, monitor_left,
  monitor_right, eyes_y) -> (vx, vy)`: `vx` is `look_target`, `vy` the pointer's
  offset from the eyes' line over `GLANCE_SPAN_Y_DIP` (300 dip), both clamped
  to -1..1, NaN 0. The caller gives `y` in window dip. The pair is eased by a
  second `EyesLook` (`App::glance`, tau 120 ms, `value()`), on the frame clock
  only while it moves (`animating_at`, `advance_glance`).
- **Rest**: `GlanceRest` (from the same `TIMER_POINTER` readings, no timer):
  the first reading only records; a changed position follows the pointer; a
  pointer still for `GLANCE_REST` (1.2 s) is no longer followed, so the target
  goes back to (0, 0) and the eyes return to the screen.
- **Pose**: `Pose::glancing(vx, vy)` sets `glance_x`, `glance_y` in grid units
  from `glance_offset`: `GLANCE_X` 4.0 across, `GLANCE_Y` 1.5 down or up. They
  are added to the face marks' translation only (the glasses ride it too).
  At every extreme, open and blinking, both eyes stay at least 0.5 grid unit
  inside the head outline (`the_glancing_eyes_stay_inside_the_head_at_every_extreme`).
- `sync_look`: not eligible snaps look and glance to the centre and resets
  `GlanceRest`; a Typing face snaps the head look to the centre, any other face
  snaps the glance.

Demo scenes: `bata-look-left`, `bata-look-right` (the idle cat at -1 / +1),
`bata-ketik-lirik-kiri` / `-kanan` (the typing cat glancing, fixed pose).

Not verified live: the feel of the ease with a real pointer, and how the tilt
reads on a see-through notch.
