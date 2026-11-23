---
tags: [gotcha, window, z-order]
date: 2026-11-23
---

# The notch puts itself back on top

User report (2026-11-23): after another app went fullscreen or was maximised,
that app's window sat over the notch until the user minimised it.

- **Why nothing repaired it:** the island gets `HWND_TOPMOST` at creation and
  again only in `reposition()` (DPI, display, monitor change).
  `IslandWindow::set_front(true)` returns early while its cached `front` is
  already true, so it can never repair an island that another window got
  above.
- **What put the other window above was not captured.** It could not be
  repeated on demand; a plain maximise, borderless fullscreen and
  `MarkFullscreenWindow` in a test window all left the island on top. The
  repair below works whatever the trigger, except a window in a higher z-band
  (uiAccess, shell surfaces), which no `SetWindowPos` can beat.
- **Repair:** on the existing 1 s `TIMER_SYSTEM` tick, right after
  `check_fullscreen`, `App::keep_on_top` walks the z-order from the top down
  to the island (`sources::fullscreen::island_covered`) and calls
  `IslandWindow::raise()` (`HWND_TOPMOST`, no activate, `front` cache
  untouched) when a window above it:
  - is a candidate (`is_candidate`): visible, not minimised, not cloaked, not
    Vale's own, not shell, not a tool window (menus, tooltips), not
    click-through;
  - overlaps the **drawn** notch (`renderer.island_px()` + window origin), not
    the 620x330 canvas, so windows over transparent pixels never cause a
    raise;
  - does not cover the whole monitor: fullscreen stays with the fullscreen
    rule, which hides the notch.
- Skipped while stay-behind has lowered the island (`front == false`) or a
  fullscreen app is reported (`should_raise`). No new timer, thread or hook.
- **Known limits:** a raise puts the island above Vale's own topmost windows
  too (notes on the top layer, lyrics card, tooltip); after the drawn-rect
  rule this needs a foreign window over the notch first. Another app that
  re-asserts topmost over the notch swaps with it once a second. A topmost
  toolbar at top centre that is not a tool window (screen-share bars) would
  now sit under the notch; not checked live.

Related: [[stay-behind]], [[notch-monitor]], [[bata-idle-power-budget]].
