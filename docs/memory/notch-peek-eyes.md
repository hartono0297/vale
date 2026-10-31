---
tags: [decision, notch, pointer, power, motion]
date: 2026-10-31
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
  clock, art, timer or head. Drawn even with the cat off. Static: no blink (the
  blink timer is not armed while shrunk, nor the equalizer timer).
- Colour: the brand terracotta #E2683C (assumption: the espresso #17120F of the
  cat's own eyes would be invisible on the dark notch).

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

Only the pure state machine, the geometry and the `--demo` scenes
(`peek-eyes`, `peek-eyes-plain`) were run; the real pointer approach, the
click-through onto a browser, and the walk over windows with overlays (a game
bar or other always-on-top non-transparent windows count as "a window under")
were not tried by hand. The Notch-style tab shape was not looked at on a clean
screen.
