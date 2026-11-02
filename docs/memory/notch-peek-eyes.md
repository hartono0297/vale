---
tags: [decision, notch, pointer, power, motion]
date: 2026-11-02
---

# The notch shrinks to the cat's eyes

With "Di belakang jendela" **off**, the closed notch gets out of the way of what
is under it. While the pointer is near it and an ordinary window lies under it
(a browser's tab strip, a window's title bar), the notch shrinks to a small pill
holding only the cat's two open eyes, so the tabs and the close button under it
can be clicked. It comes back when the pointer rests on the eyes. Since v3.8
it has a setting, **"Mengecil jadi mata"** (`notch_peek`, Settings → Kucing →
Gerak, on by default): off, `peek_eligible` is false (through
`pointer_watch::peek_allowed(notch_peek, stay_behind)`), a shrunk notch comes
back at once and the poll may stop ([[bata-idle-power-budget]]). The row does
not depend on the cat (the eyes show with the cat off) and is faded, with a
note, while "Di belakang jendela" is on. The eyes pill stays terracotta
whatever the cat's coat ([[cat-colours-and-surfaces]]). Related:
[[stay-behind]] (the other way out of the way, which turns this off),
[[notch-monitor]] (the notch's monitor, which the zone and the poll follow),
[[bata-idle-power-budget]] (the shared poll), [[bata-robot-expressions]] (the
eyes), [[open-mode]] (the hover that comes back after a restore),
[[cat-looks-at-pointer]] (the other user of the same poll).

## When it may act (`App::peek_eligible`)

`config.stay_behind` off, not fullscreen, not locked or display off
(`screen_away`), and the state view is a closed compact view: Idle,
MediaCompact, MediaLyric, PomodoroCompact (`pointer_watch::peek_view`).
Notices, the panel and Hidden are never shrunk (assumption: a notice has to be
seen). Anything else ends the peek at once (`peek_guard`, run at the top of
`refresh`), so a notice or a panel that appears is drawn whole.

## The state machine (`src/pointer_watch.rs`, pure, tested)

`Peek { Normal | Shrunk { over_since } | Restored }`, `step(PeekInputs, now)`:

- Normal, in the zone, a window under, not over the normal notch -> Shrunk.
- Normal and the pointer over the normal notch -> Restored when it was resting
  there: a notch the pointer sits on is never pulled away. **Fast entry** (v3.9
  fix): a pointer that *arrives* shrinks it even when it lands straight on the
  notch between two polls. `Peek` keeps a private `outside` flag (the previous
  eligible reading saw the pointer outside the zone; `reset()` and any reading
  that is not eligible clear it, so "unknown" counts as resting), and
  `PeekInputs::moved` says the screen position differs from the previous peek
  reading. `arrived = outside && moved`; in Normal + in zone + over the notch,
  `arrived && window_under && !dragging` -> Shrunk, else Restored. No window
  under, a held button, a stationary cursor (a synthesized `WM_MOUSEMOVE`) or no
  earlier outside reading all keep today's Restored. `arriving()` (Normal and
  `outside`) tells `App::peek_tick` to read the window under the notch for an
  over-the-notch reading (`candidate`).
- Shrunk and the pointer leaves the zone -> Normal.
- Shrunk and over the drawn eyes continuously for `PEEK_DWELL` (500 ms) ->
  Restored. `over_since` resets whenever the pointer is off the eyes.
- Restored and the pointer leaves the zone -> Normal (it can shrink again on
  the next approach). A Restored notch does not shrink again while the pointer
  stays in the zone.
- **The `WM_MOUSEMOVE` trigger.** The island's first `WM_MOUSEMOVE` used to
  call `begin_hover` before the poll had stepped the peek. `App::on_mouse_move`
  now, right after the shrunk early return and before the `on_island` check,
  steps the peek once (`GetCursorPos`, `peek_tick`) when `peek_eligible()`, the
  state is Normal and `hover.needs_arming()`; if that shrank the notch it
  returns without starting the hover. `peek_last_pt` (an App field, cleared with
  the peek in `peek_guard` and where the poll stops) feeds `moved`.
- A click on the eyes restores at once (assumption: the request only names the
  dwell). That click is swallowed: it never opens the panel.

The **zone** is the *normal* notch shape of the current view grown by
`PEEK_ZONE_DIP` (50 dip) on the left, right and bottom and unbounded upward;
never taken from the pill, so it does not flap with the shape.
`window_under` is read with `capture::win::top_windows(normal notch in screen
px)` (it skips the desktop, invisible, minimised, cloaked and click-through
windows and Vale's own) **only** in the Normal -> candidate case, and again at
most once a second while the pointer stays in the zone (`UnderCache`); never at
idle.

The poll's "near the top" (`pointer_watch::near_top(pt_x, dy_px, span,
scale)`, which sets the quick or slow pace) also needs the pointer inside the
notch monitor's X span (`monitor_span`: the island's centre plus and minus half
the monitor, `[left, right)` in physical px, from `App::notch_span`). The top
of a monitor beside the notch's is not near, so the poll stays slow there. One
monitor: the span is the whole screen, nothing changes. The zone itself was
already local to the notch.

Extra input, an assumption: a **mouse button held** (a drag may be on its way
to the island, the shelf drop) never *starts* a shrink, so the notch is still
there to drop on. A notch already shrunk stays so.

## Shape and drawing

- `ShapeKind::Eyes`: 34 x 14 dip (`EYES_W`, `EYES_H`), at the compact shapes'
  top (Pill 8, Notch 0), centred like them, no robot widening. Fully rounded in
  both styles (radius 7). In the Notch style the shoulders are
  `shoulder_depth(s)` = `min(12, h / 2)`: 12 for every closed and open shape
  (28 dip and taller), 7 for the eyes, shrinking with the morph's height, so
  the bottom corners keep the other half of the height. With fixed 12 dip
  shoulders `corner_radius` held them to 2 and the eyes hung as a small tab.
  `notch_path`, `corner_radius` and the hit test (`in_shoulder`) all use it.
- `refresh` targets the Eyes shape while the peek is Shrunk (`App::eyes`).
  Shrinking and coming back count as a compact swap: the content fades in with
  the morph (the cat too while the eyes morph runs, `Frame::robot_fades`).
  Motion Full: the eyes' own morph, `motion::EYES_MS` (320 ms, slower than
  `ANIM_MS`) on `MorphCurve::EyesInOut` (ease-in-out cubic, no overshoot;
  `morph_timing(eyes_only, lyric_resize, plain)` picks it when the swap is the
  eyes alone, `eyes_swap && view_same && !hiding`; a view change at the same
  time keeps the view's spring). The content fade (`Transit.content_alpha`,
  `Anim::fading`) and the fill (`fill_at`) follow the same curve (the eased
  progress for the eyes, the linear one for other morphs), so the eyes and the
  shape move together; a restore by click goes through the same refresh. Motion Saver / Off or Energy Saver:
  `Anim::settled`, the new shape at once (a view change at the same time still
  morphs).
- `Frame::eyes`: the renderer draws the fill in the pill and **only**
  `RobotPainter::paint_eyes` (the Pill parts of `face(Expr::Idle)`,
  `eye_pills()`, scaled 0.8 so they are 8 dip tall, centred on the pill). No
  clock, art, timer or head. Drawn even with the cat off. The equalizer timer
  is off while shrunk. The eyes look and blink since v3.11 (next section).
- Colour: the brand terracotta #E2683C (assumption: the espresso #17120F of the
  cat's own eyes would be invisible on the dark notch). The blink's two level
  lines and the open pills are both terracotta, whatever the coat.

## The eyes look and blink

**Look** (`pointer_watch.rs`, pure, tested). `eyes_target(pointer_x, pointer_y,
pill_cx, pill_cy, half_span)` in island dip: `vx = (x - cx) / half_span`
clamped to -1..1; `vy = (y - cy) / PEEK_ZONE_DIP` clamped to 0..1 (down only,
never up); NaN or `half_span <= 0` give 0. `half_span` is the *normal* notch's
half width of the current view (`App::normal_shape`) plus `PEEK_ZONE_DIP`, so
the extreme is reached at the edge of the peek zone, not at the monitor's
edges (the cat's look uses the monitor). Offsets: `dx = EYES_LOOK_X_DIP (4) *
vx`, `dy = EYES_LOOK_DOWN_DIP (1.5) * vy`. `EyesLook` is two `LookEase` (tau
120 ms, `LOOK_EPS` ignored changes), `LookEase::jump_to(v)` is the no-ease
case.

**Gating.** Runs while shrunk (`App::eyes_look_eligible`: peek Shrunk,
`cat_follow_pointer`, peek still eligible) and does *not* need the cat
(`config.robot`) or depend on Motion beyond how it moves. `eyes_motion(follow,
motion_moves, windows_animations)` gives `Still` (toggle off: centred), `Jump`
(Saver, Off, Energy Saver or Windows animations off: `jump_to` the target and
draw once, no frame clock) or `Ease` (Motion Full and `App::animations`: the
frame clock, `ensure_clock` when `set_target` returns true, `animating_at`
counts `eyes_look.moving()`, `animate` advances it before the draw like
`advance_look`). Every shrink and every restore snaps the look to the centre
(`refresh`, `eyes_swap`); `sync_eyes_look` centres it when it stops being
eligible and finishes a running ease when the motion setting stops easing.
`WM_SETTINGCHANGE` (Windows animations) re-syncs it and the blink at once
while shrunk.

**Poll.** Reuses `TIMER_POINTER`, no new timer. While shrunk with the eyes look
allowed, `sync_pointer_watch` passes `follow_on(look_eligible,
eyes_look_eligible)` to `poll_ms`: 33 ms near the top edge, 250 ms far, instead
of the peek-only 80 / 350. `on_pointer_timer` feeds the pointer (island dip, as
`peek_tick` converts it) to `eyes_look_tick` after the peek step. Nothing
changes while the notch is not shrunk.

**Blink** (`ui/robot.rs`, pure, tested). `EyesBlink { Open, Shut { second_follows
}, Gap }`, `next(&mut Rng) -> ms until the next step`, `shut()`.
`EYES_BLINK_MIN_MS` 3000 .. `EYES_BLINK_MAX_MS` 6000 open; `BLINK_MS` (120)
shut; a double blink with probability 1/4 (`rng % 4 == 0`) whose second shut
starts `EYES_DOUBLE_MS` (260) after the first starts (140 ms open between).
Reuses the one-shot `TIMER_BLINK`, re-armed per step by `on_blink` when
`eyes_blink_timer` is set (the cat's 4-7 s schedule otherwise). Armed only
while shrunk, Motion Full (not Energy Saver) and Windows animations on, with or
without the cat; the frame clock does not hold it back (the eyes' ease would
postpone every blink): `on_blink` draws only when not animating, otherwise the
clock draws the shut state. `sync_robot_timers` keeps the cat's `plan.blink &=
!eyes` and `sync_eyes_blink` arms or kills the timer for the eyes (it kills
only if the cat's timer is not armed, so a restore never kills the cat's);
switching either way resets the state to open.

**Drawing.** `Frame::eyes_pose: EyesPose { dx_dip, dy_dip, blink }`.
`paint_eyes_pill(style, shape, alpha, pose)` slides the eyes by the offset
after `robot::eyes_offset_in(style, shape, dx, dy)` clamps it; open = the two
`eye_pills()`, blink = the cat's `Line::Blink` (two level lines, stroke 2.4
grid, round caps) via `paint_eyes(.., blink)`.

**The clamp.** `eyes_offset_in` bisects (down first, then across) for the
largest slide that keeps every corner of both boxes (open pills and blink
lines with their stroke, both tested whatever the current state so a blink
never moves them) inside the current `shape_contains` outline (the main
shape, shoulders excluded, the stricter side), for any shape of the 320 ms
morph. At the final 34 x 14 pill the look's extremes (+-4, 1.5) already fit, so
it is a guard and not a visible limit
(`the_shrunk_eyes_stay_inside_the_pill`: both styles, every notch size, every
compact view's shape morphing to the pill at t = 0..1, offsets up to +-20 / 10,
open and shut).

Demo scenes `peek-eyes-left`, `peek-eyes-right`, `peek-eyes-down` (vx 0.5, vy 1)
and `peek-eyes-blink` (robot on, a fixed pose, no pointer read). The Settings
row "Ikuti kursor" now reads "Kepala dan mata menoleh ke kursor"
(`CatFollowDesc`).

## Clicks and hover

The island is a per-pixel-alpha layered window, so pixels it does not draw pass
clicks to the window below. While shrunk the frame draws only the pill, so
`regions.shape` / `island_px` / `under` / `hit_test` describe the pill (a point
in the old notch but off the pill is `Under::Off`;
`the_shrunk_notch_is_a_small_pill_that_alone_takes_clicks`). The drag margin
(halo) is off while shrunk.

- A press and release on the pill restore the notch (`restore_peek`); nothing
  else runs (no swipe, no Hit::Body click, no shelf press).
- `on_mouse_move` returns before `bata.brain.hover` and the 120 ms dwell;
  `on_mouse_hover` does nothing; the wheel and the sideways wheel / swipes
  (`gestures_active`) do nothing over the eyes.
- A restore by dwell or click starts the hover as if the pointer had just
  arrived (`begin_hover`: the cat happy, `TrackMouseEvent` armed), so Hover mode
  opens after the usual dwell and Click mode on a click.

## Not verified live

Only the pure state machine, the geometry, the look / blink maths and the
unit tests were run; the demo scenes were built but not looked at, and the
feel of the eyes' look and blink with a real pointer, and the blink timer
re-arm across a shrink / restore, were not tried by hand. The real pointer approach, the
click-through onto a browser, and the walk over windows with overlays (a game
bar or other always-on-top non-transparent windows count as "a window under")
were not tried by hand. The Notch-style tab shape was not looked at on a clean
screen.
