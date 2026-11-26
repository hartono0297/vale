# Sticky note resize from every edge — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A sticky note resizes from all four sides and corners, and by width from the left/right side while rolled up.

**Architecture:** The hit test (`paint::target_full`) answers `Target::Resize(Edges)` for 6 dip bands at the note's edges. The resize drag carries those `Edges`; a new `snap::snap_resize_edges` moves only those edges, anchoring the opposite ones, and snaps/clamps them. The host maps `Edges` to the matching cursor.

**Tech Stack:** Rust 2024, `windows` crate (Win32), unit tests in `#[cfg(test)]` modules.

**Spec:** `docs/superpowers/specs/2026-11-26-sticky-resize-edges-design.md`

## Global Constraints

- Edge band: `EDGE = 6.0` dip inside the note. Corner reach along a side: `GRIP` (16 dip).
- `Edges` already exists in `src/widgets/snap.rs` (`left, top, right, bottom: bool`, `Edges::BOTTOM_RIGHT`). Do not redefine it.
- Lock (`prefs.lock`) blocks every resize. Rolled: width only, saved `sticky.rect.h` unchanged.
- Test: `cargo test --locked`. On this PC `app::record::tests::a_folder_is_writable_only_when_a_file_can_be_made_in_it` always fails (drive Z:), and timing tests (e.g. `recorder::tests::stop_pushes_the_rest_of_the_timeline`) may fail under load but pass alone. Ignore those two.
- Disk is tight: never start a second `target` dir; use this worktree only.

## Review Focus

1. Dragging the left/top edge past the minimum size: the note must stop at `MIN_W`/`MIN_H` with the right/bottom edge still fixed (not slide). → Task 2 test `left_edge_stops_at_the_minimum_with_the_right_edge_fixed`.
2. Dragging the left/top edge beyond the area edge: x/y stop at `area.x`/`area.y`, right/bottom stays. → Task 2 test `left_and_top_edges_stop_at_the_area`.
3. Rolled note dragged on its right band with vertical motion: height and saved height unchanged. → Task 3 test `a_rolled_note_resizes_its_width_only`.
4. A header button or stack arrow near an edge still gets its click. → Task 1 test `buttons_win_over_the_edge_bands`.
5. A stack resized from its left edge narrower than `stack::MIN_W`: it stops at `stack::MIN_W` with its right edge fixed. → Task 3 test `a_stack_resized_from_the_left_keeps_its_right_edge_at_min_width`.

---

### Task 1: Hit test answers which edges (paint.rs)

**Files:**
- Modify: `src/widgets/paint.rs` (`Target`, `target_full`, its tests)
- Modify: `src/widgets/art.rs` (tests that name `Target::Grip`)
- Modify: `src/widgets/host.rs` (only mechanical: every `Target::Grip` pattern becomes `Target::Resize(_)`, test asserts become `Target::Resize(Edges::BOTTOM_RIGHT)`; no behaviour change)

**Interfaces:**
- Consumes: `crate::widgets::snap::Edges`.
- Produces: `Target::Resize(Edges)` replacing `Target::Grip`; `pub const EDGE: f32 = 6.0;` in `paint.rs`.

Rules for `target_full` (after the `Outside` check, `h` = shown height):
- Not rolled: header buttons (and stack arrows) first, then format-bar buttons (not `BarHit::Bar`), then bands: `left = x < EDGE`, `right = x >= w - EDGE`, `top = y < EDGE`, `bottom = y >= h - EDGE`; a band within `GRIP` of a perpendicular side also takes that side (corner): e.g. `top && x < GRIP` → top-left, `left && y >= h - GRIP` → bottom-left. `grip_rect` stays bottom-right. Any edge set → `Target::Resize(edges)`. Then the existing rules.
- Rolled: header buttons first, then only `x < EDGE` → `Resize(left)`, `x >= w - EDGE` → `Resize(right)`; else `Header`.

- [ ] **Step 1: Write failing tests in `paint.rs` tests**
  - `every_side_and_corner_is_a_resize_target`: for a 240x208 note, not rolled, buttons shown: `(120,2)`→top, `(120,206)`→bottom, `(2,100)`→left, `(238,100)`→right, `(2,2)`→top+left, `(238,2)`→top+right, `(2,206)`→bottom+left, `(237,205)`→bottom+right, `(10,100)`→`Body` (just inside the band), `(120,20)`→`Header`.
  - `a_rolled_note_resizes_only_from_left_and_right`: rolled 240x208: `(2,30)`→left, `(238,30)`→right, `(120,2)`→`Header`, `(120,ROLLED_H-2)`→`Header`.
  - `buttons_win_over_the_edge_bands`: every `button_rect(b, w)` centre and both stack arrows' centres (stack = true) still give `Button`; a format-bar button touching the bottom band (bar shown) gives `Bar(_)` not `Resize`.
  - Update existing `Target::Grip` asserts (paint.rs, art.rs, host.rs) to `Target::Resize(Edges::BOTTOM_RIGHT)`.
- [ ] **Step 2:** `cargo test --locked widgets::paint` → the new tests fail (compile error on `Target::Resize` is fine as the failing state).
- [ ] **Step 3:** Implement in `paint.rs`; rename host.rs patterns mechanically.
- [ ] **Step 4:** `cargo test --locked widgets::` → all pass.
- [ ] **Step 5:** Commit `fix(sticky): hit test finds every resize edge`.

### Task 2: Edge-aware resize snap (snap.rs) — runs in parallel with Task 1

**Files:**
- Modify: `src/widgets/snap.rs` only.

**Interfaces:**
- Produces:
  ```rust
  pub fn snap_resize_edges(
      start: DipRect,      // the note as shown when the drag began (dip, area-relative)
      delta: (f32, f32),   // pointer movement since then, dip
      edges: Edges,
      area: &DipRect,
      others: &[DipRect],
      alt: bool,
      rolled: bool,
  ) -> Snapped
  ```
  `snap_resize(want, area, others, alt)` stays and becomes `snap_resize_edges(want, (0.0, 0.0), Edges::BOTTOM_RIGHT, area, others, alt, false)`; its existing tests must keep passing unchanged.

Rules: a moved left edge goes to `start.x + dx` with `start.right()` fixed; right `start.w + dx`; top `start.y + dy` with `start.bottom()` fixed; bottom `start.h + dy`. Unmoved edges stay. `rolled`: top/bottom ignored, `h = start.h`, min height `ROLLED_H` (`min_h_of`). Size limits as `clamp_size_min`; a moved left/top edge never passes `area.x`/`area.y`, a moved right/bottom never passes `area.right()`/`area.bottom()`; when the size hits a limit the fixed edge stays fixed. Snap (unless `alt`) each moved edge to the nearest candidate within `THRESHOLD`: left `area.x, o.right() + GAP, o.x`; top `area.y, o.bottom() + GAP, o.y`; right/bottom as today; neighbour candidates only when `near(..)` on the other axis, as today. No candidate → the size along that axis on the grid (`grid(w)`, anchored at the fixed edge). Guides from `guides_of`.

- [ ] **Step 1: Failing tests** (AREA as in the existing tests):
  - `left_edge_moves_with_the_right_edge_fixed`: start `(400,100,240,208)`, delta `(-37,0)`, left, no snap target near → `rect.right() == 640.0`, `rect.w == 280.0` (277 to the grid), `rect.y, rect.h` unchanged.
  - `top_edge_moves_with_the_bottom_edge_fixed`: start `(400,300,240,208)`, delta `(0,-50)`, top → `rect.bottom() == 508.0`, `rect.h == 256.0`.
  - `left_edge_stops_at_the_minimum_with_the_right_edge_fixed`: start `(400,100,240,208)`, delta `(+500,0)`, left → `rect.w == MIN_W`, `rect.right() == 640.0`.
  - `left_and_top_edges_stop_at_the_area`: start `(20,20,240,208)`, delta `(-500,-500)`, top+left → `rect.x == AREA.x`, `rect.y == AREA.y`, `rect.right() == 260.0`, `rect.bottom() == 228.0`.
  - `a_left_edge_snaps_to_a_neighbour_with_the_gap`: other `(100,100,200,200)`, start `(320,100,240,208)`, delta `(-6,0)`, left → `rect.x == 312.0` (= other.right()+GAP), right stays 560.
  - `a_rolled_resize_keeps_the_rolled_height`: start `(400,100,240,ROLLED_H)`, delta `(40,90)`, `Edges{right:true,bottom:true,..}`, rolled → `rect.h == ROLLED_H`, `rect.w == 280.0`.
  - `alt_resizes_without_snapping`: left, delta `(-37,0)`, alt → `rect.x == 363.0`.
- [ ] **Step 2:** `cargo test --locked widgets::snap` → new tests fail.
- [ ] **Step 3:** Implement `snap_resize_edges`; rewrite `snap_resize` as the wrapper.
- [ ] **Step 4:** `cargo test --locked widgets::snap` → all pass (old and new).
- [ ] **Step 5:** Commit `fix(sticky): resize snap moves any edge`.

If the build fails in a file you do not own (Task 1 is editing paint/art/host at the same time), wait a minute and rerun; do not edit those files.

### Task 3: Wire the drag and cursor (host.rs) — after Tasks 1 and 2

**Files:**
- Modify: `src/widgets/host.rs`; `src/widgets/snap.rs` only to mark `snap_resize` `#[cfg(test)]` if it becomes test-only.

**Interfaces:**
- Consumes: `Target::Resize(Edges)`, `snap_resize_edges(...)`.
- Produces: `DragKind::Resize(Edges)`; `fn resize_cursor(e: Edges) -> PCWSTR`.

Rules:
- `DragKind::Resize` carries `Edges`; `Target::Resize(e)` (not locked) begins `DragKind::Resize(e)`; locked → `Down::Nothing`.
- `resize_cursor`: left/right only → `IDC_SIZEWE`; top/bottom only → `IDC_SIZENS`; top+left or bottom+right → `IDC_SIZENWSE`; top+right or bottom+left → `IDC_SIZENESW`. Used for hover over `Target::Resize(e)` (unless locked) and during a resize drag.
- `drag_move` resize: convert the pointer delta (px) to dip with the display scale, call `snap_resize_edges(start_dip, delta, edges, &area, &others, alt, rolled)`. The stack minimum keeps the fixed edge: with `edges.left` grow leftwards (`x = right - stack::MIN_W`, not below `area.x`), else as today.
- Rolled: `w.sticky.rect.w` and `x`/`y` update; `w.sticky.rect.h` stays; `w.view.h` keeps the unrolled view height (only `w`, `x`, `y` of the view change).
- The existing `resized` helper and its test at host.rs ~5880 move to `snap_resize_edges` (or the helper is removed if unused).

- [ ] **Step 1: Failing tests** (follow the existing pattern of `resizing_a_note_with_photos_…`: `begin_drag` + `drag_move` + `finish_drag` inside `with`):
  - `a_note_resized_from_the_left_keeps_its_right_edge`: begin `DragKind::Resize(Edges{left:true,..})`, move `(-80px, 0)` with alt → `view.right()` unchanged, `view.w` grew by `80/scale`.
  - `a_note_resized_from_the_top_keeps_its_bottom_edge`: same for top.
  - `a_rolled_note_resizes_its_width_only`: rolled note, `Resize(right)`, move `(+60px, +90px)` → `sticky.rect.h` and `view.h` unchanged, `sticky.rect.w` grew.
  - `a_stack_resized_from_the_left_keeps_its_right_edge_at_min_width` (if a stack fixture exists nearby; else a unit test on the clamp helper you extract).
  - `resize_cursor_matches_the_edges`: the four mappings above.
- [ ] **Step 2:** `cargo test --locked widgets::host` → fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test --locked` → all pass except the two known ones.
- [ ] **Step 5:** Commit `fix(sticky): resize from every edge and while rolled up`.
