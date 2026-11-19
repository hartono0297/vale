---
tags: [decision, lyrics, geometry, power]
date: 2026-10-27
---

# Lyric notch follows its line

v3.1 decision #1: the lyric notch (`ShapeKind::MediaLyric`) is as wide as
the line on screen instead of a fixed 300 × 34. Math in `src/ui/geometry.rs`
(`lyric_*`), state in `App::lyric_fit` (`LyricFit` in `src/app.rs`).

- **Width** = `ceil(text_w) + 1 + lead + 20`, clamped to
  `[220, lyric_max_w]`. Lead is 20 without the robot, 16 + 26 + 8 = 50 with
  it (since the 16 dip inset, [[compact-padding]]; with the robot the width is
  today's clamped width + 10, so the box is unchanged), so `shape_with_robot` adds nothing for MediaLyric (`robot_extra` 0).
  `lyric_max_w(screen_w) = min(560, 0.45 × screen_w)`, never below 220;
  `IslandWindow::screen_w()` is the primary monitor's logical width,
  refreshed on `WM_DPICHANGED` / `WM_DISPLAYCHANGE`. The "♪" rest and
  empty lines take 220.
- **Hysteresis** (`lyric_target_w`): grows at once; shrinks only when the
  new line wants 40 px or more less. Leaving the lyric view resets it
  (Hidden keeps it). A width-only change morphs in `LYRIC_RESIZE_MS` (180)
  with the same `ease_out_back`; view changes keep `ANIM_MS` (`Anim::ms`).
- **Measure equals draw**: `Renderer::lyric_text_width` goes through
  `text_width` → `layout()` (tabular figures + display tracking), and
  `paint_lyric` draws that same full-width layout, centring it itself. So a
  line that fits is never trimmed with "…" (see [[studio-font-loading]]).
- **Too long for the max**: drawn from the box's left at full width,
  clipped by a layer whose opacity brush fades the last `LYRIC_FADE_W`
  (32 px). Held `LYRIC_HOLD_MS` (1 s), then 30 px/s until its end sits just
  left of the fade. The fade weakens as the hidden part drops below 32 px,
  so a box growing past its line does not snap.
- **Power** ([[bata-idle-power-budget]]): no continuous timer. A fitting
  line arms nothing. A long one arms one one-shot `TIMER_LYRIC_SCROLL` (16)
  at line start + 1 s; then `lyric_scrolling` counts in `animating_at` and
  the `FrameClock` runs only until `lyric_scroll_end_ms`, ending through
  `animate()` (which re-plans the blink). A new line, another view or
  Hidden kills both.
- Demo: lyric scenes are sized like the app. `lyric` (medium), `lyric-short`,
  `lyric-instrumental`, `lyric-long`, `bata-lirik-long`;
  `DI_LYRIC_SCROLL=ms` freezes the long line that long after it showed.

## Jadi muka with the Headphone (2026-11-19)

Report: "notch mode face, lyric masih banyak yang kepotong". Root cause: under
Jadi muka the Headphone's earcups make `face_content_wearing` end the content
`face_content_trim` (about 5.3 dip) short of the right end, so the line's box
was that much narrower than the width `lyric_width_min` sized for the line. A
line that exactly filled its notch (every line wider than the user's notch
width) overflowed by the trim: it drew from the box's left, faded the last 32
px and the last letter was cut. Other accessories never trim, so only the
Headphone showed it.

Fix: `LyricFit.trim` (`set_trim`, `App::lyric_trim`: Face style only) makes the
notch `trim` wider (`width()`); `box_w`/`content_w` and the scroll plan stay as
without it. The demo does the same (`scene_lyric_trim`). Tests:
`lyric_fit_in_face_mode_keeps_the_line_box_for_every_accessory`,
`the_face_content_trim_is_what_the_earcups_take_off_the_content`. A line wider
than the cap still scrolls and fades by design.

Related: [[studio-media-progress-timer]], [[screenshot-capture-script]], [[notch-colour-style]]

Part of [[v3-studio-overview]]. Listed in [[index]].
