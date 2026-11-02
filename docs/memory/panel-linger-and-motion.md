---
tags: [decision, panel, animation, power, linger, v3.4]
date: 2026-11-02
---

# Panel linger and panel motion

Two parts of one run. Part A keeps the panel open for a while after the user
did something in it; part B makes every change inside the open panel move
instead of pop, with nothing running at idle. Pure rules live in
`src/ui/motion.rs` (tested); the reducer part in `src/state.rs`; the frame
plumbing in `src/app.rs`; drawing in `src/ui/render.rs` and
`src/ui/render_panel.rs`. See [[open-mode]], [[bata-idle-power-budget]],
[[panel-pages]], [[stay-behind]], [[lyric-notch-width]],
[[studio-media-progress-timer]].

## A. Linger (`state.rs`)

`ACTIVITY_LINGER` = 4 s. `IslandState::linger_until` holds the deadline and
`lingering` mirrors "it is still in the future", re-evaluated at the start of
every `apply` (`panel_open()` has no clock; the "was open" values are read
before it, so the close still cleans up after itself). `panel_open()` also
holds while `lingering`.

Armed, only when the panel is open at that moment (never reopens a closed
one, nothing while shut by a tile, a fullscreen app or `Dismiss`), at the end
of `apply` around the event, not inside `click()`:

| Event | Arms when |
|---|---|
| `EditorOpen(false)` (closed, saved, deleted, cancelled) | the panel was open: the editor was often all that held it |
| `Click` (any hit but a media button the hover guard drops) | the panel was open and showing its content (not covered by a notice) |
| `Wheel` that turned a page | same |
| `CliFocus(false)`, `ClipFocus(false)` | something other than the linger still holds the panel after the event (`panel_held()`, hover). If the input alone held it, letting go closes it at once, as Esc always did, and an earlier click's linger is ended with it, never re-armed |

A new click re-arms (a fresh 4 s). The pointer may come back (hover holds the
panel as usual) and leaving keeps whatever is left.

Ended early by: `collapse()` (tiles, a copy with Enter, the search closing,
the Clipboard hotkey pressed while its page shows, lingering included),
`Dismiss` (a click outside), `Fullscreen(true)`, an `OpenMode` change, and
the panel closing for any reason (at the end of `apply`, `!panel_open()`
clears it). `Collapse::Shut/Left` always wins (`panel_open` needs
`collapsed == No`).

Expiry: `next_deadline()` includes `linger_until`, so the existing
`TIMER_DEADLINE` fires `Event::Tick`, `apply` sees it passed and the panel
closes through the normal morph. No new timer.

**Cost, one:** `outside_click_wanted()` is also true while lingering, so the
existing `TIMER_OUTSIDE` poll runs for those 4 s (it is what turns a click
outside into `Dismiss`). It stops when the linger ends.

**Gotcha:** many older state tests clicked or turned a page and then
`Hover(false)` expecting the panel shut. They use `leave()` now (leave plus a
`Dismiss`); the editor tests assert the linger then a `Tick`.

## B. Transitions

### Audit: what each view did before, what it does now

| View / change | Before | Now |
|---|---|---|
| Idle, MediaCompact, MediaLyric, PomodoroCompact | content drawn at full alpha at once inside the morphing shape | when it replaces another view and the shape morphs, content fades in with the morph (`content_alpha`, 0 to 1 from 30 % to 100 % of the morph); a swap with no shape change (no morph) stays instant; the lyric notch's width-only resize is excluded (`resize`). Bata is never in the faded layer (`Renderer::robot_later`: drawn at full alpha after it), so he does not blink out, nor does a notice's alert bounce. A morph retargeted mid-fade carries on from the alpha it reached (`Anim::fade_from`, `motion::content_alpha_from`) |
| Notices (including the screenshot preview) | same | same fade in with the morph |
| The strip (sticky notice in the panel) | pops with the page | unchanged: part of the page's content, fades with the panel |
| Panel opens | content faded in with the height | same, now via `motion::panel_fade` (shared), from the height of the view it grows from (a Notice's 76 px, not 34) |
| Panel closes | content gone on the first frame (the drawn view became the compact one) | the panel's content stays and fades out with the shape's height down to the target view's height (compact, Notice, NoticeShot), the new view fades in (`Transit::leaving`, `paint_leaving_panel`); no clicks while it closes. It draws what it showed (`ShownPanel`, below), not what the close reset |
| Page change by the wheel | slides | slides |
| Other page changes (Clipboard tile, page tiles, a disk warning's "Buka", the Mole notice, hotkeys over an open panel, a page switched off while it shows) | `follow_page` already ran after every applied event, but only looked at the view afterwards: a change at the same moment as the panel opening slid from nothing, a change under a notice slid unseen | `follow_transition` compares the panel before and after the event (`motion::transition`): slide only when it showed both before and after |
| A slide that starts during a slide | restarted from +-1: the page on screen jumped | continues the same strip (`SlideStrip`): every page on screen keeps its place, either way |
| Search over a page, and back | instant (no slide to or from it) | cross-fade of the two pages, 160 ms (`PageFade`, `PAGE_FADE_MS`) |
| Calendar Month/Week/Day | instant | `Reveal`: finer range from below, coarser from above |
| Calendar prev/next, "+N lagi" day paging | instant | `Reveal`: later from the right, earlier from the left; paging from the right |
| Notes and reminders paging | instant | `Reveal` from the right |
| Cleaner opening and closing over System | instant | `Reveal` from below, closing from above |
| Mole phases and its log toggle | instant | `Reveal`, fade only (the content changes in place) |
| Cleaner own/Mole switch | instant | `Reveal` from below |
| CLI tall and small | morph with the height fade | unchanged |
| CLI folder menu | instant | the menu alone reveals from below (the terminal keeps showing); closing is instant |
| Clipboard and Search results while typing | instant | instant, on purpose |
| Shelf drag highlight | instant | instant, on purpose |
| Volume plate | pops in and out on its timer | unchanged on purpose: it is feedback for a wheel notch |

### Rules

- `REVEAL_MS` = 160, ease-out cubic, offset `REVEAL_OFFSET` = 8 px, applied
  as an opacity layer over the page content (`paint_page_at`, a second D2D
  layer `layer2` because the panel's mask layer is already pushed). A reveal
  cut short by another starts from the opacity it had. Hit regions follow
  today's `settled` rule: they are registered on the first settled frame of
  the new content (a reveal moves it 8 px at most); only a slide or a page
  cross-fade holds them back (`motion::panel_regions_wanted`).
- **Gotcha, regions after a cross-fade:** `animate` draws the frame at the
  fade's end and then stops the clock, so that frame must register the
  regions. The renderer gets the fade only while `t < 1`
  (`motion::page_fade_frame`); passing `Some(1.0)` left the search, the
  Clipboard or the Shelf page with no click regions until something else
  redrew.
- **Slides are one strip** (`motion::SlideStrip`): the page coming in
  starts `from` widths away and eases to 0; the pages going out sit a whole
  number of widths from it (`trail`, up to `SLIDE_TRAIL` = 5) and move with
  it. A new notch puts the next page one width beyond the page that was
  coming in, so nothing on screen moves or vanishes (before, the page going
  out vanished: two notches 20 ms apart left it covering 75 % of the panel,
  and a reversal after two quick notches drew the page on the wrong side).
  A page that lands in the new page's slot (going back) is dropped, so is
  one that never shows during the new slide. `from` is capped at
  `SLIDE_MAX` = 4 widths: only a long fast spin reaches it, and then the
  strip jumps by the excess. The renderer gets at most two trail pages
  (`Frame::page_trail`, those with |x| < 1).
- **Gotcha, the closing panel's content:** by the time `refresh` starts the
  closing morph, `apply` has already reset what the panel showed
  (`cli_tall`, `cli_menu`, the notes, reminders and day pages; the strip is
  `None` once the view is not the panel; the cleaner is kept since
  2026-11-02, but `cleaner()` and the snapshot only give it on System). `App` takes
  `IslandState::shown_panel()` before each event and keeps it
  (`closed_panel`) when the event takes the panel off screen; `refresh` hands
  it to `leaving`, and `draw` builds the leaving frame from it (page, strip,
  page indices, CLI size and menu, cleaner and Mole stage), and
  `paint_leaving_panel` sizes it with the same `panel_kind`. Without a
  snapshot for that page there is no fade-out, never a reset page.
- **Gotcha, the fade floor is the morph's target, not 34 px:** a panel can
  close into a Notice (76 px) or a NoticeShot (`SHOT_H`, 156 px). With a fixed
  34 px floor the old content was still at about 23 % (Notice) or 68 %
  (NoticeShot) when the morph ended and popped away. `motion::panel_fade(h,
  rest_h, floor_h)` is now zero exactly at `floor_h`, and `motion::fade_floor`
  picks it when `refresh` retargets: the target's height when the panel closes
  into a view (`App::fade_floor_h`, handed to the renderer in
  `Transit::fade_floor_h`), the height of the visible view it grows from when
  it opens, the floor it already had when a panel resizes. Tests in
  `motion.rs` pin 1 at the panel's height, a monotonic fall and 0 at 34, 76
  and `SHOT_H`.
- **Gotcha, a retarget mid-fade keeps the alpha:** with hover, the panel is
  often reopened while its close still runs (pointer out, back in). The
  drawn view is then compact, so "grows from the visible view" would put the
  floor at the shape's height and the content would pop from ~93 % to 0.
  `refresh` passes the alpha the panel content has now (`FadeStart::alpha`:
  a panel still opening, or the `leaving` one still fading, measured against
  its own height, `panel_rest_h`), and `fade_floor` picks the floor that
  keeps it: `carried` on a reopen of the same panel, else the floor solved
  from the alpha (it may be below 0 for a taller panel). A close retargeted
  to another view keeps the alpha too unless the target needs a higher floor
  (the fade must still be 0 when the morph ends). A settled view has no
  alpha and gets the plain rules above.
- **Gotcha, Fullscreen clears the leaving snapshot:** `Fullscreen(true)` takes
  the panel off screen (view Hidden) while `drawn` still says Panel, so the
  snapshot `closed_panel` kept would become `leaving` when the game ends and
  the island returns as Idle, and old panel content showed faintly. `refresh`
  now clears `leaving` and `closed_panel` on a Hidden retarget, and
  `leaving_snapshot` hands one over only when the morph starts from a view
  that was visible (`anim.to_opacity == 1.0`), never from a hidden one. The
  decision is the pure `leaving_on_morph` (and `closed_panel_after` for
  `follow_closed_panel`); app tests walk Panel, Hidden, Idle through them.
- The CLI folder menu's reveal uses `layer2` too, so it is skipped (the menu
  shows at once) while a page cross-fade is running: D2D cannot push one
  layer twice, and `EndDraw` would fail.
- Nothing is longer than the morph (`ANIM_MS` 240 ms): slide 220, reveal 160,
  page fade 160.
- A sub-view switch is detected by comparing `SubView` snapshots before and
  after each applied event (`panel_snap`): calendar view, anchor, day page,
  notes and reminders page, cleaner mode, Mole stage and log, CLI menu.
  Typing never changes one.
- **Reduced motion:** `SPI_GETCLIENTAREAANIMATION` is read at start and on
  `WM_SETTINGCHANGE` (no timer; the flag is read again on any setting
  broadcast). Off: no slide, no cross-fade, no reveal (instant), and the morph
  eases with plain ease-out cubic instead of `ease_out_back` (`Anim::plain`,
  `motion::morph_ease`). The morph itself and the content fades remain.
- Demo: `DI_DEMO_REVEAL=0.5` freezes a reveal (`cal-week-to-day`,
  `cleaner-open`) or the search's page cross-fade (`search-open`, over the
  Music page, as the island does it); `DI_DEMO_T=0.5` on a compact scene shows the
  morph's content fade and the panel fading out.

### Budget

Nothing new at idle and no new repeating timer. `reveal`, `page_fade`,
`slide` and the morph are in `App::animating_at`, so the existing
`FrameClock` starts when a transition begins (`ensure_clock`) and stops when
`animating_at` turns false. Only the linger's `TIMER_OUTSIDE` poll (above) and
one deadline on `TIMER_DEADLINE` are added, both only for the 4 s after an
action. The fade floor adds no timer and no frame. See [[bata-idle-power-budget]]
and [[panel-pages]].

**Not verified live:** the drawing (layers, the closing panel's fade, the
folder menu's reveal) was run in `--demo` only to check that nothing errors;
no screenshot was compared by eye. The owner should look at: a calendar
range switch, the cleaner opening, the search opening and closing, the panel
closing from the Music page, a notice appearing from the panel, and the
folder menu; and turn Windows' animation effects off once.

**2026-11-02:** the cleaner is no longer reset by a close. `ShownPanel.cleaner`
is still the closing panel's snapshot (the cleaner on System, `cleaner()`), so
the fade keeps its size; see [[cleaner]].
