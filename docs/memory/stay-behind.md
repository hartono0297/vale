---
tags: [decision, window, experimental, v3.2.1, v3.3, animation]
date: 2026-11-03
---

# Stay behind windows (v3.2.1)

Settings → Umum → **Di belakang jendela** (`config.stay_behind`, default
**off**, island-owned like the other toggles). Experimental: it was left out of
3.2.0 on purpose because it can fight other windows for the order.

- Off: nothing changes, nothing runs. The island is topmost as always.
- On: the island sits at the **bottom** of the window order
  (`IslandWindow::set_front(false)` → `SetWindowPos(HWND_BOTTOM, NOMOVE |
  NOSIZE | NOACTIVATE)`, which also drops the topmost style), so ordinary
  windows cover it. It never takes focus.
- It comes up when (`src/behind.rs`, pure `Behind::decide`, tested):
  - the view must be seen (`view_needs_front`: anything but Idle, the music
    and timer compact views and Hidden: notices, panels, search, previews,
    shelf drags);
  - the pointer is on the 3 px strip at the screen's top edge over the
    *drawn* island plus 12 dip each side (`STRIP_PX`, `STRIP_MARGIN_DIP`,
    `on_strip`, `App::on_raise_strip`; not the 620 px window, which is wider);
  - the pointer is still over the drawn island while it is up.
  It goes back down 250 ms (`LOWER_AFTER`) after none of those holds.
- Cost: one `TIMER_BEHIND` (29) reading `GetCursorPos`, only while the
  setting is on, the screen is not fullscreen, the session not locked and the
  display not off (`App::sync_behind`). 80 ms (`POLL_MS`) with the pointer
  within 150 dip of the top edge *and* inside the notch monitor's X span
  (`pointer_watch::near_top`, [[notch-monitor]]) or while lowering is due;
  350 ms (`SLOW_MS`) otherwise (`behind::poll_ms`, coalescable). Fullscreen, a lock or turning it
  off puts the island back on top and stops the poll. A view change raises it
  at once (`sync_timers`), not at the next poll.

## The rise (v3.3)

Raising used to be one `SetWindowPos(HWND_TOPMOST)`: a covered island popped
in at full opacity in a single frame. Now (`behind::Rise`, `rise_wanted`,
pure and tested; `App::rise`, `rise_frame`):

- **When**: the order goes Bottom → Top **and** the island was covered
  (`App::island_covered`: `WindowFromPoint` at the centre of the drawn island,
  `GetAncestor(GA_ROOT)` is not the island's own window). Read once, at the
  raise, while it is still down. An island that was already visible (nothing
  over it) is raised without a fade, so it never blinks out. Half covered with
  a free centre counts as not covered (its edges pop; accepted).
- **How**: opacity 0 → 1 and 8 dip above its place (`RISE_DROP_DIP`) → 0 over
  180 ms (`RISE_MS`), ease-out cubic. With Motion not Full
  (`self.motion().moves()` false: Saver, Off or Energy Saver) only the fade,
  120 ms (`RISE_SAVER_MS`). `draw` applies it with `rise_frame` (multiplies
  the morph's own opacity, adds the offset to `shape.y`).
- **The first frame** (opacity 0) is drawn *before* `set_front(true)`, so the
  raised window never shows one frame at full opacity.
- **Cost**: no new timer and no poll change. `animating_at` is true only while
  the rise runs, so the frame clock runs for those 180 ms (or 120 ms) and
  stops; `animate` clears the rise after its settled frame. Lowering again,
  or the setting turning off / fullscreen (`sync_behind`), drops it. See
  [[bata-idle-power-budget]].
- **Lowering stays instant** (the window in front simply covers it). Fading
  out first would need an occlusion walk over the z-order, not
  `WindowFromPoint`; left out.
- During the slide the hit regions move with the drawing (a click in those
  frames lands as drawn).

**Limits (not verified live):**
- `HWND_BOTTOM` is the bottom of the *normal* order. Whether the island stays
  visible above the wallpaper and desktop icons, rather than under the desktop
  window, was not tried; if it vanished the strip still raises it
  (`HWND_TOPMOST`).
- A maximised window's title bar is at y = 0: the strip lies over it, so the
  island rises over that title bar while the pointer is there.
- A window covering the top-centre of the screen hides the island until the
  pointer touches the strip.
- Games and fullscreen apps: the existing fullscreen detection hides the
  island as before; the order is reset to top when fullscreen starts. The
  detection is per monitor: a fullscreen app only counts while the foreground
  window is on the notch's monitor (`is_fullscreen(Some(island))`,
  `hides_notch`), so a game on another monitor leaves the island alone. With 2+
  monitors and the foreground on another one, a z-order walk finds a fullscreen
  window that covers the notch's own monitor and hides the island too
  (2026-11-03, [[notch-monitor]]).

Related: [[open-mode]], [[v3-2-handoff]]. With this setting off, the notch can
instead shrink to the cat's eyes over a window ([[notch-peek-eyes]]); with it
on, that peek does not run.
