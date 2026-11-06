---
tags: [design, power, render]
date: 2026-11-06
---

# Round 3: redraw only the notch parts that changed, reuse text layouts

## Intent

Make Vale lighter while music plays, without any visual or feature change.
Rounds 1 and 2 (dirty rect, WIC bitmap target; see
`docs/memory/bata-idle-power-budget.md`) took Vale from 66-75 to 38-43 ms CPU/s
with music playing. The main thread is still ~38 ms/s (user 24, kernel 15).
The owner wants it lighter still; the earlier target was 25 ms/s total.

Why it is still high: on every equalizer tick (30 fps) the island repaints the
**whole notch** (the dirty rect is the whole shape outline: `dirty::painted_bounds`)
and every painter runs in full; the clip only throws pixels away afterwards.
Every text draw builds a new `IDWriteTextLayout` (`Renderer::layout` ->
`fonts::text_layout`), so the title or the lyric line is shaped again 30 times a
second, and a scrolling lyric line also creates a new gradient stop collection
and brush per frame (`fade_brush`). `App::fit_lyric_line` measures the lyric
line through the same path.

## Goals

1. **A. Reuse text layouts.** Text that has not changed is not laid out again.
2. **B. Part-level dirty rect in the two music views.** In `View::MediaCompact`
   and `View::MediaLyric`, a frame repaints only the parts whose pixels change,
   and painters for parts outside the dirty rect do not run their draw calls.
3. Measured with music playing (release, A/B against v5.0.0 and against main
   `ac5dbbc`, 60 s windows, the same scripts as rounds 1-2): Vale total as close
   to 25 ms CPU/s as this approach gets; the main thread clearly below 38 ms/s.

## Non-goals and anti-goals

- No change to what is drawn: pixels identical to a full redraw at every scale,
  hit regions identical, no visual change of any kind. A stale pixel or a
  missing hit region is a failure even if CPU drops.
- The equalizer stays at 30 fps (`EQ_MS = 33`); capture keeps its 15 ms poll.
- No other views (idle, notices, pomodoro, panel, eyes) get part-level dirty
  rects; they keep today's whole-shape dirty rect.
- No new thread, timer, crate or rendering backend (no DirectComposition).

## Constraints

- Rust 2024, Win32 + Direct2D software target (`src/ui/render.rs`), the dirty
  plan logic in `src/ui/dirty.rs`, the EQ skip in `app.rs` (`EqKey`,
  `eq_draw_needed`) stays as is.
- The debug self-check `DI_DIRTY_CHECK=1` (compares each dirty frame with a full
  redraw) must cover part-dirty frames and report 0 mismatches over the full
  `--demo` loop (Pill and Notch, with and without `DI_HALO=1`).
- `cargo test --locked` green, `cargo build --release --locked` no warnings.

## Approaches considered

1. **A only (layout cache).** Small and safe; saves the shaping cost only,
   maybe 5-10 ms/s.
2. **A + B (chosen by the owner).** Biggest saving: on most music frames only the
   bars or the cat change, a small area of the notch.
3. **B only.** Leaves the per-frame text shaping in the frames that do repaint
   text and in `fit_lyric_line`.

## Decided

### A. Layout cache

- A small cache in the renderer (a handful of entries is enough: title, lyric
  line, previous lyric line, rest sign, plus measure calls) keyed by everything
  that changes the layout: the UTF-16 text, the format (by identity), the box
  `w` and `h`, the UI font and whatever tracking/typography `Renderer::layout`
  applies. Cleared whenever the fonts or the UI font change.
- A cached layout must give exactly the result a fresh one gives. Callers that
  mutate a layout (`lyric_line` calls `SetTextAlignment`) must not leak that
  state into the next user of the same entry (set it every time, or make it
  part of the key).
- Used by every `Renderer::layout` caller in the island renderer, so
  `text_width`, `lyric_text_width` (called by `App::fit_lyric_line`) and the
  draws all benefit.
- The lyric fade's gradient stop collection is made once per render target
  (dropped in `drop_target` with the other target-owned objects); only the
  brush's start/end points or opacity change per frame, if that is cheaper and
  pixel-identical.

### B. Part-level dirty rect (music views only)

- The renderer describes each music-view frame as a list of **parts**, each a
  rect in dip (including everything it paints outside its box: text legibility
  halo, bar shadows, the cat's halo and the notes above it, the progress line's
  round ends, the fade) and a **key** of every value that decides its pixels.
  Parts at least: progress line, art or cat slot, title text, EQ bars, lyric
  line (current and fading-out line, fade, both scrolls), toast dot.
- The part list is only trusted when everything else is the same as the last
  presented frame: same `Drawn` fields as `dirty::plan` compares today, same
  view, same shape, no transition (`force_full` false), no toast slot
  animation, content alpha 1, legibility unchanged. Otherwise: today's path
  (whole-shape dirty rect or `Full`).
- When trusted, the dirty area is **several small rects, not one bounding
  rect** (decided by the owner after the builder check: one bounding rect over
  the bars, the cat and the progress line covers most of the notch and saves
  little). Each changed part contributes its own rect (old rect union new rect,
  with the existing safety margin, clamped to the surface); rects that overlap
  or touch are merged. Too many rects, or rects covering most of the shape, may
  fall back to one bounding rect (the threshold is the builder's call).
  Nothing changed: no paint and no present at all (the EQ skip already covers
  most of this).
- **The progress line dirties only its tip:** the span between the old and the
  new end of the fill (plus the anti-aliasing margin), not the whole line, as
  long as the track and the fill's start are unchanged.
- Painting a part-dirty frame: for each rect, an aliased clip on whole device
  pixels (the existing gotcha: an antialiased clip edge blends old and new
  pixels), a clear of that rect, the shape's background fill (clipped) and only
  the parts that intersect it; then the clip is popped. A part that intersects
  no rect skips its draw calls. **Hit regions are still built for every part**,
  drawn or not, so `HitRegions` is identical to a full paint.
- Copy and present: each rect is copied from the WIC bitmap to the DIB.
  `UpdateLayeredWindowIndirect` takes one `prcDirty`, so the present uses the
  bounding rect of all of them (the DIB outside the small rects already holds
  the right pixels).
- `verify_dirty` / `DI_DIRTY_CHECK` compares the whole presented frame with a
  full redraw on multi-rect frames too.
- The pure parts (rect union, key diff, the decision whether part-level dirty
  is trusted) get unit tests in `dirty.rs`.

## Left to the builder

- The data structure for parts and keys, and where it is built (renderer or
  app).
- Exact cache size and eviction.
- Whether a multi-rect frame paints once per rect or once under a combined
  aliased clip, and the fallback threshold to one bounding rect.
- How to make painters skip draws (a "should draw" check against the current
  dirty rect, or separate measure/draw steps), as long as hit regions are
  unchanged.
- Whether the gradient brush is cached or only its stops.

## Done means

- Unit tests for the cache key, the part diff and the trust decision.
- `DI_DIRTY_CHECK=1` over the full `--demo` loop (and `DI_HALO=1`): 0
  mismatches, with part-dirty frames counted separately in its summary so it is
  clear they were exercised (compact media with bars, lyric with the dancing
  cat, long scrolling lyric, lyric line change, toast dot).
- Docs: `bata-idle-power-budget.md` gets a "Part-level dirty rect and the
  layout cache" section; a gotcha note if one comes up; index link.
- After merge the main session measures the release build with music playing
  and reports before/after numbers.
- Manual checks for the owner: the notch at 125/150/200 % scale while music
  plays, hover and click targets in the compact and lyric views.
