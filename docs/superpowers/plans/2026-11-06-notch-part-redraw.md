# Notch part redraw and layout cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While music plays, the island repaints only the notch parts that changed (several small rects) and stops re-shaping text that did not change, with pixels identical to a full redraw.

**Architecture:** A small LRU of `IDWriteTextLayout`s behind `Renderer::layout`. In the two music views the renderer describes each frame as a list of parts (id, bounds, key); a pure function in `dirty.rs` diffs it against the last presented frame's list and returns up to four device rects. `Renderer::draw` paints `paint_island` once per rect under an aliased clip, painters skip parts outside the rect, each rect is copied to the DIB, and the present uses their bounding rect.

**Tech Stack:** Rust 2024, `windows` crate (Direct2D software WIC target, DirectWrite), Win32 layered window.

**Spec:** `docs/superpowers/specs/2026-11-06-notch-part-redraw-design.md`

## Global Constraints

- No visual or feature change: pixels identical to a full redraw at every scale; `HitRegions` identical.
- `EQ_MS = 33` and the 15 ms capture poll unchanged; `EqKey` / `eq_draw_needed` unchanged.
- Part-level dirty only in `View::MediaCompact` and `View::MediaLyric`; every other view keeps today's `Plan::Dirty(whole shape)` / `Plan::Full`.
- No new thread, timer, crate or rendering backend.
- `cargo test --locked` green; `cargo build --release --locked` and `cargo build` with 0 warnings.
- `DI_DIRTY_CHECK=1` over the full `--demo` loop (Pill then Notch), plain and with `DI_HALO=1`: 0 `dirty mismatch` lines.
- Worktree under `.claude/worktrees/`; every commit message ends with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A seek (the progress fill end jumps far in one frame) must repaint the whole span between the old and new end, not two separate tips: pinned by `part_plan_progress_seek_covers_the_span` (Task 2).
2. A song change (title, art and bars change in one frame) must give at most 4 rects or fall back to one bounding rect, never drop a changed part: pinned by `part_plan_caps_at_four_rects` (Task 2).
3. A see-through island (`legibility > 0`) draws text and bar shadows 1 dip outside their boxes; part bounds must include them: pinned by `music_parts_bounds_include_shadows` (Task 3).
4. The dancing cat's nod, sway, notes and halo reach outside its slot; the cat part's bounds must hold all of it (clipped to the shape): pinned by `cat_part_holds_every_dance_pose` (Task 3) and the demo self-check.
5. Anything that is not a part (volume plate, toast slot, a fade, a morph, eyes, a view change) must disable part mode for this frame and the next: pinned by `part_mode_off_when_not_trusted` (Task 3).

---

### Task 1: Layout cache and fade stops cache

**Files:**
- Create: `src/ui/layout_cache.rs` (pure LRU, generic, unit tested)
- Modify: `src/ui/mod.rs` (add `pub mod layout_cache;`)
- Modify: `src/ui/render.rs` (`Renderer` fields, `Renderer::layout` ~line 3262, `set_ui_font` ~line 1020, `drop_target` ~line 1095, `fade_brush` ~line 2469)

**Interfaces:**
- Produces: `pub struct LruCache<K: PartialEq, V: Clone> { .. }` with `pub fn new(capacity: usize) -> Self`, `pub fn get_or_insert_with<E>(&mut self, key: K, make: impl FnOnce() -> Result<V, E>) -> Result<V, E>` (hit moves the entry to the front; a miss inserts at the front and evicts the last beyond `capacity`; a failed `make` inserts nothing), `pub fn clear(&mut self)`, `pub fn len(&self) -> usize`.
- Produces: `LAYOUT_CACHE_SLOTS: usize = 16` in `render.rs`.

- [ ] **Step 1: Write the failing tests** in `src/ui/layout_cache.rs`:

```rust
#[test] fn hit_returns_the_cached_value_without_calling_make() {
    let mut c = LruCache::new(2);
    assert_eq!(c.get_or_insert_with::<()>(1, || Ok(10)), Ok(10));
    assert_eq!(c.get_or_insert_with::<()>(1, || panic!("make called on a hit")), Ok(10));
}
#[test] fn evicts_the_least_recently_used() {
    let mut c = LruCache::new(2);
    c.get_or_insert_with::<()>(1, || Ok(1)).unwrap();
    c.get_or_insert_with::<()>(2, || Ok(2)).unwrap();
    c.get_or_insert_with::<()>(1, || Ok(0)).unwrap(); // 1 is now most recent
    c.get_or_insert_with::<()>(3, || Ok(3)).unwrap(); // evicts 2
    assert_eq!(c.len(), 2);
    assert_eq!(c.get_or_insert_with::<()>(2, || Ok(22)), Ok(22));
}
#[test] fn a_failed_make_caches_nothing() {
    let mut c: LruCache<u8, u8> = LruCache::new(2);
    assert_eq!(c.get_or_insert_with(1, || Err("no")), Err("no"));
    assert_eq!(c.len(), 0);
}
#[test] fn clear_empties_it() {
    let mut c = LruCache::new(2);
    c.get_or_insert_with::<()>(1, || Ok(1)).unwrap();
    c.clear();
    assert_eq!(c.len(), 0);
}
```

- [ ] **Step 2: Run** `cargo test --locked layout_cache` → FAIL (`LruCache` not defined).
- [ ] **Step 3: Implement `LruCache`** as a `Vec<(K, V)>`, most recent first.
- [ ] **Step 4: Run** `cargo test --locked layout_cache` → PASS.
- [ ] **Step 5: Use it in the renderer.**
  - New field `layouts: RefCell<LruCache<LayoutKey, IDWriteTextLayout>>` with `type LayoutKey = (Vec<u16>, usize /* format as_raw() */, u32 /* w bits */, u32 /* h bits */)`, capacity `LAYOUT_CACHE_SLOTS`.
  - `Renderer::layout(&self, text, format, w, h)` keeps its signature and goes through the cache; on every return (hit or miss) it resets the layout's alignment to `format.GetTextAlignment()`, so a caller that changed it (`lyric_line` sets `LEADING`) never leaks that into the next user.
  - `set_ui_font` clears the cache (the formats are rebuilt, and their pointers may be reused).
  - `fade_brush`: cache the `ID2D1GradientStopCollection` per target keyed by `strength.to_bits()` in a `RefCell<Option<(u32, ID2D1GradientStopCollection)>>`; `drop_target` clears it. The brush itself is still made per call.
- [ ] **Step 6: Verify:** `cargo test --locked` green; `cargo build` 0 warnings; debug `vale.exe --demo` with `DI_DIRTY_CHECK=1` for one full loop: 0 mismatches (the cache must not change a pixel).
- [ ] **Step 7: Commit** `perf(render): reuse text layouts and the lyric fade stops between frames`.

### Task 2: Pure part diff in `dirty.rs`

**Files:**
- Modify: `src/ui/dirty.rs` (new types and `part_plan`, tests in its `#[cfg(test)]` module)

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum PartId { ProgressTrack, ProgressTip, Art, Title, Bars, Lyric, ToastDot }`
  - `#[derive(Clone, Debug, PartialEq)] pub enum PartKey { Track([u32; 4]), Tip(u32), Art { art: Option<(usize, Vec<u8>)>, robot: Option<RobotFrame> }, Title(String), Bars(Option<[u32; BAR_COUNT]>), Lyric { text: Option<String>, prev: Option<String>, fade: u32, scroll: [u32; 2] }, Dot(bool) }`. `Bars(None)` means the time-based sine bars: never equal to anything, including another `Bars(None)` (implement `PartialEq` by hand or compare through a helper `keys_equal`).
  - `#[derive(Clone, Debug, PartialEq)] pub struct Part { pub id: PartId, pub bounds: RectDip, pub key: PartKey }`
  - `pub const MAX_RECTS: usize = 4;`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Rects { rects: [RectI; MAX_RECTS], len: usize }` with `pub fn as_slice(&self) -> &[RectI]` and `pub fn bounding(&self) -> RectI`.
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PartPlan { Nothing, Rects(Rects) }`
  - `pub fn part_plan(prev: &[Part], cur: &[Part], scale: f32, size: (i32, i32)) -> Option<PartPlan>`: `None` when the two lists do not have the same ids in the same order (caller falls back to the whole-shape plan). For each part whose key differs or whose bounds differ: `to_device(union_dip(prev.bounds, cur.bounds), scale, margin_px(scale))`, clamped. Then merge rects that overlap or touch (repeat until stable). More than `MAX_RECTS` left: one rect, their union. No changed part: `PartPlan::Nothing`. Empty rects are dropped.
  - `Plan` gains a variant: `Plan::Rects(Rects)` (still `Copy`).

- [ ] **Step 1: Write the failing tests:** `part_plan_nothing_when_keys_and_bounds_are_equal`, `part_plan_one_rect_per_changed_part` (bars and cat changed, far apart → 2 rects, each `to_device` of its bounds with the margin), `part_plan_merges_touching_rects`, `part_plan_progress_seek_covers_the_span` (`ProgressTip` bounds at x=10 then x=150 → one rect from ≤10 to ≥150+w), `part_plan_caps_at_four_rects` (6 far-apart changed parts → one rect equal to their union), `part_plan_none_when_ids_differ`, `sine_bars_always_change` (`Bars(None)` vs `Bars(None)` → a rect), `rects_bounding_is_the_union`.
- [ ] **Step 2: Run** `cargo test --locked dirty::` → FAIL.
- [ ] **Step 3: Implement** the types and `part_plan` as above (reuse `to_device`, `margin_px`, `union`, `clamp`, `union_dip`).
- [ ] **Step 4: Run** `cargo test --locked dirty::` → PASS; whole `cargo test --locked` still green (existing `Plan` matches get a `Plan::Rects` arm only where the compiler asks; `render.rs` treats it like `Full` until Task 4).
- [ ] **Step 5: Commit** `feat(dirty): part list diff into up to four dirty rects`.

### Task 3: Build the part list for the music views

**Files:**
- Modify: `src/ui/render.rs` (new `fn music_parts`, `Renderer::signature`/`Drawn` use, tests in its test module)
- Modify: `src/ui/dirty.rs` only if `Drawn` gains a field (`parts: Option<Vec<Part>>`; then `Drawn` loses `Copy`-free assumptions nowhere: it is already `Clone`)

**Interfaces:**
- Consumes: `PartId`, `PartKey`, `Part` (Task 2).
- Produces: `fn music_parts(style: Style, shape: ShapeRect, frame: &Frame, legible: bool) -> Option<Vec<Part>>` (a free function, pure, testable without Direct2D), and `Drawn::parts: Option<Vec<Part>>` filled by `Renderer::signature`.

Rules for `music_parts` (`None` = part mode off for this frame):
- `None` unless `frame.view` is `MediaCompact` or `MediaLyric`, `!frame.eyes`, `frame.transit.content_alpha == 1.0`, no `transit.reveal/page_fade/leaving`, `frame.page_slide == 0.0`, `frame.strip_motion.is_none()`, `!frame.robot_fades`, `frame.toast_slot_w == 0.0`, `frame.volume.is_none()`, `!frame.full_redraw`.
- Layouts exactly as the painters use them: compact uses `compact_layout_robot` / `compact_layout` and `compact_progress(style, shape, p)`; lyric uses `lyric_layout_robot` / `lyric_layout`.
- Bounds: each part's box grown by `PART_PAD = 2.0` dip on every side, plus 1 dip down when `legible` (text drop shadow, `shadow_bar`), then extended vertically to the whole shape (`shape.y .. shape.y + shape.h`: compact content is clipped to the shape, so nothing paints above or below it). The cat (`Art` with a robot) is grown sideways by the widest the dance, the notes and the halo reach outside its slot: read `robot.rs` (`with_note_lift`, the dance sway/nod, the halo's `grow` 3.0) and name that constant `CAT_REACH`. The lyric part is its line box (fade and rise included because the height is the whole shape). The dot: `toast_dot(shape)` circle as a rect. `ProgressTrack`: the track rect; `ProgressTip`: a `2 * PART_PAD` wide rect centred on `fill.right()` over the track's height.
- Keys: `Track` = track rect bits; `Tip` = `fill.right().to_bits()`; `Art` = `art_key(bytes)` of `media.art` (or `None`) plus the `note_placed` robot frame; `Title` = the label `paint_compact` draws; `Bars` = `frame.eq_heights` filtered by `media.playing` as bits, `None` when the sine bars are drawn; `Lyric` = current text, previous text (only while `fade < 1`), `fade` bits, `[lyric_scroll, lyric_prev_scroll]` bits; `Dot` = whether `paint_island` draws the dot.
- Parts not drawn this frame are left out (no progress → no track/tip; no media → `None` overall).
- `Drawn::parts` is compared in `Renderer::draw` (Task 4), not in `dirty::plan`.

- [ ] **Step 1: Write the failing tests:** `compact_parts_follow_compact_layout` (ids in order `ProgressTrack, ProgressTip, Art, Title, Bars, ToastDot` when all present; `Bars` bounds contain `compact_layout(shape).bars`), `lyric_parts_are_cat_and_line`, `music_parts_bounds_include_shadows` (bars bounds with `legible = true` reach 1 dip lower than with `false`), `cat_part_holds_every_dance_pose` (for every dance pose the robot module exposes, the area `robot.rs` paints for it lies inside the `Art` bounds), `part_mode_off_when_not_trusted` (each condition in the list above alone gives `None`), `tip_key_moves_with_the_fill`.
- [ ] **Step 2: Run** `cargo test --locked render::tests::` → FAIL.
- [ ] **Step 3: Implement** `music_parts` and fill `Drawn::parts` in `Renderer::signature`.
- [ ] **Step 4: Run** `cargo test --locked` → PASS.
- [ ] **Step 5: Commit** `feat(render): describe music-view frames as parts`.

### Task 4: Paint, copy and present several rects

**Files:**
- Modify: `src/ui/render.rs` (`draw` ~1044, `paint_on` ~1747, `copy_to_surface` ~1789, `present` ~1154, `verify_dirty` ~1842, `paint_island` ~1976, `paint_compact` ~2303, `paint_lyric` ~2369, toast dot block ~2159)

**Interfaces:**
- Consumes: `dirty::part_plan`, `Plan::Rects`, `Rects`, `Drawn::parts`.
- Produces: `Renderer` field `paint_only: Cell<Option<Rect>>` (dip) and `fn paints(&self, area: Rect) -> bool` (true when `paint_only` is `None` or intersects `area`).

- [ ] **Step 1: Plan selection in `draw`:** after `dirty::plan` returns `Plan::Dirty(_)`, if both `prev.parts` and `cur.parts` are `Some` and `dirty::part_plan` returns `Some(p)`: `PartPlan::Nothing` → skip paint, copy and present, keep `last` as `cur`; `PartPlan::Rects(r)` → `Plan::Rects(r)`. Otherwise keep the plan as is.
- [ ] **Step 2: `paint_on` with `Plan::Rects`:** one `BeginDraw`; for each rect: `PushAxisAlignedClip(to_dip(rect), ALIASED)`, `Clear`, set `paint_only` to the rect in dip, `paint_island(...)`, `PopAxisAlignedClip`; `paint_only` back to `None`; one `EndDraw`. Hit regions: reset before the first rect, and every pass rebuilds the same regions, so keep the last pass's (assert in debug that each pass built equal regions).
- [ ] **Step 3: Culling:** in `paint_compact` guard the progress line (`paints(track)`), the art/cat (`paints(art area grown by CAT_REACH)`), the title (`paints(layout.text)`), the bars (`paints(layout.bars)`); in `paint_lyric` the cat and each `lyric_line`; in `paint_island` the toast dot. The halo fill and the shape fill are always drawn (clipped, cheap). Guards use the same bounds as Task 3's parts (share the helpers so they cannot drift).
- [ ] **Step 4: `copy_to_surface(Plan::Rects)`** copies each rect (same `Lock` + `copy_rect` as one `Dirty` rect); **`present(Plan::Rects)`** uses `prcDirty = rects.bounding()`.
- [ ] **Step 5: Self-check:** `verify_dirty` takes the plan; for `Plan::Rects` it compares as today (whole frame vs a full redraw) and reports the bounding rect in a mismatch line; a second counter prints `vale: dirty check: {n} part frames compared` (first and every 500th), so a run shows part frames were exercised.
- [ ] **Step 6: Verify:**
  - `cargo test --locked` green; `cargo build --release --locked` and `cargo build` 0 warnings.
  - Debug `--demo` with `DI_DIRTY_CHECK=1` for a full loop (Pill then Notch), then again with `DI_HALO=1`: both print the `part frames compared` line and 0 `dirty mismatch` lines. The scenes the loop must have gone through: compact media with bars and progress, lyric with the dancing cat, long scrolling lyric, a lyric line change, toast dot on and off.
  - Temporarily shrink `CAT_REACH` to 0, confirm the self-check reports mismatches on the dancing cat, restore it, confirm 0 again (proves the check can fail).
- [ ] **Step 7: Commit** `perf(render): repaint only the changed parts of the music views`.

### Task 5: Docs

**Files:**
- Modify: `docs/memory/bata-idle-power-budget.md` (new section "Part-level dirty rects and the layout cache": what a part is, when part mode is off, the multi-rect paint, the self-check counter)
- Modify: `docs/memory/index.md` (the bata line mentions it)
- Modify: `docs/memory/exe-size.md` only if the release exe size moves by more than 4 KB

- [ ] **Step 1:** Write the section (decisions and gotchas only, no code walk-through).
- [ ] **Step 2: Commit** `docs: part-level dirty rects and the layout cache`.

After merge, the main session builds the release, measures with music playing (A/B against v5.0.0 and `ac5dbbc`), and reports.
