# Screenshot Rotate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rotate a screenshot 90° left/right in the screenshot editor (annotations turn with it and stay editable, text stays upright, one undo step, window re-fits), plus a rotate button in the inline capture mode that opens the editor already rotated.

**Architecture:** A pure `shot_edit::rotate` module turns pixels, points, rects and annotations. `Session` carries a quarter-turn count inside its undo snapshots so picture and annotations always undo together. The editor window keeps the original picture and derives the shown one from the count; export, save and auto-copy already render "picture + annotations" and need no change. Inline mode hands off through a new `FinishKind` and a new editor entry point that applies the turn as the first undo step.

**Tech Stack:** Rust 2024, `windows` crate (Win32 + Direct2D), unit tests in `#[cfg(test)]` modules.

**Spec:** `docs/superpowers/specs/2026-11-20-screenshot-rotate-design.md`

## Global Constraints

- Windows 11 x64; `cargo test --locked` must pass after every task; no new dependencies.
- Every user-facing string has Indonesian and English (`src/i18n.rs`, `Key => "id", "en";`).
- Shortcuts: Ctrl+R = rotate right, Ctrl+Shift+R = rotate left (editor and inline).
- Editor tooltips: "Putar ke kiri (Ctrl+Shift+R)" / "Rotate left (Ctrl+Shift+R)", "Putar ke kanan (Ctrl+R)" / "Rotate right (Ctrl+R)".
- Inline tooltip: "Putar ke kanan, buka di editor (Ctrl+R)" / "Rotate right, open in editor (Ctrl+R)".
- Existing toolbar/strip items keep their order; new items go after Undo/Redo in their own group.
- Annotations are never baked; text is never drawn rotated.
- Four turns in one direction: pixels identical, annotation coordinates within 1e-3 px.
- No timer or thread at idle (project power rule); rotation runs on the UI thread on demand.
- Worktree `.claude/worktrees/shot-rotate`, branch `claude/shot-rotate`. Do not touch `.claude/worktrees/tip-popover` or the main checkout's uncommitted `AGENTS.md`.
- Never use the Fable model for any subagent.

## Review Focus

1. Undo after drawing on a rotated picture, then undoing the rotation: picture and every annotation return together (Task 2 test `undo_of_a_rotation_turns_doc_and_picture_back_together`).
2. Pixelate after a rotation shows the mosaic of the turned picture, not a stale cache (Task 4 clears `pix`; Task 1 export test `export_of_a_rotated_document_matches_the_rotated_export`).
3. A save running while the user rotates: the file has the old state and the editor stays dirty (Task 2 test `rotate_bumps_the_revision_and_marks_dirty`).
4. Inline rotate on an empty crop behaves like "Buka di editor" on an empty crop (a cancel) (Task 5 test `open_editor_turned_keeps_the_picture_like_open_editor`).
5. Rotation with the close-confirm bar up or a text EDIT open does nothing from the keyboard (Task 4 manual check; the existing `busy`/`confirm` gates in `on_key` run before the Ctrl branch).

---

### Task 1: Pure rotation (`shot_edit::rotate`)

**Files:**
- Create: `src/shot_edit/rotate.rs`
- Modify: `src/shot_edit/mod.rs:14-20` (add `pub mod rotate;`)
- Test: `src/shot_edit/rotate.rs` (`#[cfg(test)] mod tests`), one export test in `src/shot_edit/export.rs` tests

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Turn { Left, Right }`
  - `pub fn rotate_pt(p: Pt, turn: Turn, size: (f32, f32)) -> Pt` — `size` is the picture's (w, h) before the turn. Right: `(h - y, x)`. Left: `(y, w - x)`.
  - `pub fn rotate_rect(r: FRect, turn: Turn, size: (f32, f32)) -> FRect` — both corners turned, `FRect::between`.
  - `pub fn turned_size(size: (u32, u32), turns: u8) -> (u32, u32)` — swaps for odd `turns`.
  - `pub fn rotate_shot(shot: &Shot, turn: Turn) -> Shot` — top-down BGRA; Right: dest `(h-1-y, x)` ← src `(x, y)`; Left: dest `(y, w-1-x)` ← src `(x, y)`; new size `(h, w)`.
  - `pub fn turned(shot: &Shot, turns: u8) -> Shot` — `turns % 4` right turns (0 = clone).
  - `impl Annotation { pub fn rotate(&mut self, turn: Turn, size: (f32, f32)) }` — Rect/Pixelate: `rotate_rect`; Line/Arrow: both ends; Pen/Marker: every point; Text: `(tw, th) = text_extent(text, size)`, centre `pos + (tw/2, th/2)` turned, new `pos = centre' - (tw/2, th/2)`; size, colour, style unchanged.
  - `pub fn rotate_doc(doc: &mut [Annotation], turn: Turn, size: (f32, f32))`.

- [ ] **Step 1: Write the failing tests** in `rotate.rs`:
  - `a_point_turns_right_and_left`: size (100, 40): `rotate_pt((10,5), Right) == (35, 10)`; `rotate_pt((10,5), Left) == (5, 90)`.
  - `left_undoes_right`: for size (100, 40), `rotate_pt(rotate_pt(p, Right, (100,40)), Left, (40,100)) == p` for p = (0,0), (100,40), (12.5, 7.25).
  - `four_right_turns_give_the_point_back`: fractional p = (12.3, 7.7) on (1080, 1920) back within 1e-3.
  - `a_rect_stays_a_positive_rect`: (10,5,20,10) on (100,40) Right → (25,10,10,20).
  - `the_pixels_turn_right`: 3×2 shot with distinct pixels (B channel = index), Right gives 2×3 with dest (h-1-y, x) mapping checked for all 6; `the_pixels_turn_left` likewise.
  - `four_turns_give_the_same_pixels`: `turned(&s, 4).bgra == s.bgra` and `turned(&rotate_shot(&s, Right), 3) == s`.
  - `text_keeps_its_size_and_turns_its_centre`: Text at (10,10) "ab" size 22 on (200,100) Right: size/text/colour unchanged, centre of new box == `rotate_pt(old centre)` within 1e-3.
  - `every_kind_turns`: Rect, Arrow, Line, Pen (3 pts), Marker, Pixelate each turned Right then Left on the swapped size equal the original within 1e-3.

- [ ] **Step 2: Run** `cargo test --locked rotate` — expected FAIL (module missing).

- [ ] **Step 3: Implement** the interfaces above in `src/shot_edit/rotate.rs`; register `pub mod rotate;` in `mod.rs`.

- [ ] **Step 4: Add the export test** in `export.rs` tests: `export_of_a_rotated_document_matches_the_rotated_export` — a 64×40 `Shot` with a horizontal colour ramp, doc = [Rect (8,6,20,12) width 3 red, Pixelate (30,4,24,24)]; compare `render_bgra(&turned(&base,1), &doc_turned)` with `rotate_shot(&Shot{.. render_bgra(&base,&doc)}, Right)`: same size, at most 0.5 % of pixels differ by more than 8 on any channel.

- [ ] **Step 5: Run** `cargo test --locked rotate` and `cargo test --locked export` — expected PASS.

- [ ] **Step 6: Commit** `feat(shot_edit): pure rotation of pictures and annotations`.

### Task 2: Session carries the turn in its history

**Files:**
- Modify: `src/shot_edit/mod.rs:458-670` (`Session`)
- Test: `src/shot_edit/mod.rs` tests

**Interfaces:**
- Consumes: `rotate::{Turn, rotate_doc}`.
- Produces:
  - `pub fn turns(&self) -> u8` (0..=3, starts 0).
  - `pub fn rotate(&mut self, turn: Turn, size: (u32, u32)) -> bool` — `size` = the picture as currently shown; one undo step (`begin`/`end`); turns the doc and `turns = (turns + 1 or 3) % 4`; keeps `selected`; always true.
  - Undo/redo/pending snapshots hold `(Vec<Annotation>, u8)` (a private `struct Snap { doc, turns }`); `end()` compares both; `undo()`/`redo()` restore both. Public behaviour of every existing method is unchanged.

- [ ] **Step 1: Write the failing tests:**
  - `a_rotation_is_one_undo_step`: doc [Rect], `rotate(Right, (100,40))` → `turns()==1`, `can_undo()`, doc turned; `undo()` → `turns()==0`, doc equals original exactly.
  - `undo_of_a_rotation_turns_doc_and_picture_back_together`: rotate Right, add an Arrow, undo → arrow gone, turns 1; undo → turns 0, doc original; redo twice → turns 1 and the arrow back.
  - `rotate_bumps_the_revision_and_marks_dirty`: rev before < rev after, `dirty()`; `mark_clean_at(old_rev)` keeps it dirty.
  - `left_after_right_is_two_steps_back_to_zero`: Right then Left → turns 0, two undo steps.
  - `rotation_keeps_the_selection`: selected Some(0) stays Some(0).

- [ ] **Step 2: Run** `cargo test --locked shot_edit::tests` — expected FAIL (`rotate`/`turns` missing).

- [ ] **Step 3: Implement** per Interfaces.

- [ ] **Step 4: Run** `cargo test --locked` — expected PASS (all existing Session tests unchanged).

- [ ] **Step 5: Commit** `feat(shot_edit): rotation as an undo step in the session`.

### Task 3: Toolbar items, glyphs and strings (editor + inline strings)

Independent of Tasks 1-2 (different files); may run in parallel with Task 1.

**Files:**
- Modify: `src/shot_edit/window.rs:100-260` (`Hit`, `layout`, `tip`) and its tests (~2313+)
- Modify: `src/shot_edit/icons.rs:101-` (`Glyph`)
- Modify: `src/i18n.rs` near 1120-1125
- Modify: the editor's draw loop arm for Undo/Redo (~window.rs:2091) to draw the two new buttons (always enabled, same hover as Undo/Redo)

**Interfaces:**
- Produces:
  - `Hit::RotateLeft`, `Hit::RotateRight` (appended to the enum).
  - `Glyph::RotateLeft`, `Glyph::RotateRight`: an open circular arc (about 270°) with an arrowhead at its end, counter-clockwise / clockwise, drawn like the other glyphs from lines.
  - i18n keys: `EditRotateLeft => "Putar ke kiri", "Rotate left"`, `EditRotateRight => "Putar ke kanan", "Rotate right"`, `InlineRotate => "Putar ke kanan, buka di editor", "Rotate right, open in editor"`.
  - `layout`: after Redo, a fourth separator, then RotateLeft, RotateRight (BTN size, GAP between); `left_end` = end of RotateRight. Right group unchanged.

- [ ] **Step 1: Update/write tests** in `window.rs` tests:
  - the existing toolbar-order test: `order[..8]` still the tools; `seps.len() == 4`; the items after Redo are `[RotateLeft, RotateRight]`; no overlaps (existing check).
  - `rotate_tips_name_their_shortcuts`: English `tip(Hit::RotateLeft) == Some("Rotate left (Ctrl+Shift+R)")`, `tip(Hit::RotateRight) == Some("Rotate right (Ctrl+R)")` (use the same language setup the existing tip test uses).
  - `min_client_w` test (if any) still holds with the wider left group.

- [ ] **Step 2: Run** `cargo test --locked shot_edit::window` — expected FAIL.

- [ ] **Step 3: Implement** enum arms, layout, tips, glyphs, strings, draw arm. `do_hit` gets placeholder arms `Hit::RotateLeft | Hit::RotateRight => {}` only until Task 4 (Task 4 replaces them).

- [ ] **Step 4: Run** `cargo test --locked` — expected PASS. `cargo run -- --demo-shot-editor` shows the two buttons after Redo (look, no click behaviour yet).

- [ ] **Step 5: Commit** `feat(shot_edit): rotate buttons, glyphs and strings`.

### Task 4: Editor wiring (picture, keys, window re-fit)

**Files:**
- Modify: `src/shot_edit/window.rs` (`Editor`, `open_with`, `try_open`, `do_hit`, `on_key`, a new re-fit helper, tests)

**Interfaces:**
- Consumes: Task 1 `rotate::{Turn, turned}`, Task 2 `Session::{rotate, turns}`, Task 3 `Hit::RotateLeft/RotateRight`.
- Produces:
  - `pub fn open_turned(shot: Shot, anchor: (i32, i32), demo: bool, doc: Vec<Annotation>, turn: Option<Turn>)` — `open_with` becomes `open_turned(.., None)`. With `Some(t)`, right after the editor is stored and before the first auto-copy, it runs the same rotate path as the button (so the turn is undo step 1).
  - `Editor.original: Arc<Shot>` (never changes) and `Editor.shown_turns: u8`; `Editor.base` stays the picture as shown (all existing users of `base` unchanged).
  - `fn sync_picture(hwnd: HWND)` — when `session.turns() != shown_turns`: `base = Arc::new(turned(&original, turns))`, `picture = None`, `pix.clear()`, `shown_turns = turns`, then re-fit (below), `recompute_view`, invalidate. Called after `Hit::Undo`, `Hit::Redo`, `Hit::RotateLeft`, `Hit::RotateRight`.
  - `pub fn refit_rect(center: (i32, i32), size: (i32, i32), work: RECT) -> RECT` (pure): a `size` window centred on `center`, moved (not shrunk) to lie inside `work`; if larger than `work`, pinned to `work`'s top-left.
  - Re-fit: if `IsZoomed(hwnd)`: only `e.scale = fit scale of the turned picture in the current canvas (initial_scale of img into canvas size minus 2*PAD*ui)` then `recompute_view`. Otherwise: `scale = open_scale(img, work, ui)`, `client_size(..)`, `AdjustWindowRectExForDpi`, `refit_rect(current window centre, outer size, monitor work area)`, `SetWindowPos(SWP_NOZORDER | SWP_NOACTIVATE)`; `WM_SIZE` then updates `client`/view as today.
  - `do_hit`: `Hit::RotateRight => session.rotate(Turn::Right, (base.w, base.h))`, Left likewise, then `sync_picture`. (`commit_text` already runs first in `do_hit`.)
  - `on_key` Ctrl branch: `0x52 => do_hit(hwnd, if shift { Hit::RotateLeft } else { Hit::RotateRight })`. The existing `busy` and `confirm` gates stay before it. `WM_KEYDOWN` already calls `note_change`; the toolbar path already calls it on mouse up.

- [ ] **Step 1: Write the failing tests** (pure):
  - `refit_keeps_the_centre_when_it_fits`: centre (500,400), size (300,200), work (0,0,1920,1040) → (350,300,650,500).
  - `refit_moves_inside_the_work_area`: centre (1900,1000), size (400,300), work (0,0,1920,1040) → right == 1920, bottom == 1040, size kept.
  - `refit_pins_an_oversized_window_to_the_top_left`: size (2500,1200), work (0,0,1920,1040) → left 0, top 0.
  - `refit_on_a_negative_monitor`: work (-1920,0,0,1080) keeps it inside.

- [ ] **Step 2: Run** `cargo test --locked refit` — expected FAIL.

- [ ] **Step 3: Implement** per Interfaces. `run_demo` keeps calling `open_with`.

- [ ] **Step 4: Run** `cargo test --locked` — expected PASS.

- [ ] **Step 5: Manual check** with `cargo run -- --demo-shot-editor`: Ctrl+R turns picture and demo annotations clockwise, window becomes the other shape; Ctrl+Shift+R back; Ctrl+Z / Ctrl+Y undo/redo the turn with the window following; draw a pixelate after a turn (mosaic matches the turned picture); text upright; maximize then Ctrl+R stays maximized; with the close bar up Ctrl+R does nothing; Save after a turn writes a turned file in `target/demo-shots/`.

- [ ] **Step 6: Commit** `feat(shot_edit): rotate the picture in the editor`.

### Task 5: Inline mode rotate button

**Files:**
- Modify: `src/capture/inline.rs` (`Btn`, `strip_groups`, `tip`, `InlineKey`, `key_action`, `FinishKind`, `finish_copies`, tests ~855-870, ~1600-1640)
- Modify: `src/capture/overlay/edit.rs` (`do_btn` ~787, key dispatch ~929-948, strip draw arm ~1105)
- Modify: `src/capture/overlay.rs:517-598` (`finish_inline`)

**Interfaces:**
- Consumes: Task 1 `Turn`; Task 3 `Glyph::RotateRight`, `Key::InlineRotate`; Task 4 `shot_edit::window::open_turned`.
- Produces:
  - `Btn::Rotate`; strip groups gain a fifth group `vec![Btn::Rotate]` after Undo/Redo.
  - `tip(Btn::Rotate) == Some(format!("{} (Ctrl+R)", tr(Key::InlineRotate)))`.
  - `InlineKey::Rotate(Turn)`; `key_action` Ctrl branch: `0x52 => InlineKey::Rotate(if shift { Turn::Left } else { Turn::Right })` (repeat already ignored by `inline_key`).
  - `FinishKind::OpenEditorTurned(Turn)` (`Turn` is `Copy + Eq`, so the derive holds); `finish_copies` → false for it.
  - `impl FinishKind { pub fn keeps_picture(self) -> bool }` — true for Save, OpenEditor, OpenEditorTurned; `finish_inline` uses it in both `matches!` places.
  - `finish_inline`: `OpenEditorTurned(t)` opens like OpenEditor but calls `open_turned(shot, sel.center(), demo, doc, Some(t))`.
  - `do_btn(Btn::Rotate)` → `finish_inline(FinishKind::OpenEditorTurned(Turn::Right))`; `InlineKey::Rotate(t)` → `finish_inline(FinishKind::OpenEditorTurned(t))`.

- [ ] **Step 1: Update/write tests** in `inline.rs`:
  - strip order test: `order[17..19] == [Undo, Redo]`, `order[19] == Btn::Rotate`, `order.len() == 20`.
  - `ctrl_r_rotates_right_and_ctrl_shift_r_left`: `inline_key(0x52, true, false, false, ctx) == InlineKey::Rotate(Turn::Right)`, with shift → Left; with `editing_text`, `dragging` or `confirm` → not Rotate; with `repeat` → Ignore.
  - `open_editor_turned_keeps_the_picture_like_open_editor`: `keeps_picture` true for Save, OpenEditor, OpenEditorTurned(Left/Right); false for Copy, Cancel, Close, Discard; `finish_copies(OpenEditorTurned(Right), true) == false`.
  - `rotate_tip_names_its_shortcut`: English tip == "Rotate right, open in editor (Ctrl+R)".
  - any strip-layout size/column test updated for one more button.

- [ ] **Step 2: Run** `cargo test --locked capture::inline` — expected FAIL.

- [ ] **Step 3: Implement** per Interfaces; draw `Btn::Rotate` in the strip with `Glyph::RotateRight` (same hover and alpha rules as Undo/Redo, always enabled).

- [ ] **Step 4: Run** `cargo test --locked` — expected PASS.

- [ ] **Step 5: Manual check** with `cargo run -- --demo-snap inline`: pick a region, draw an arrow, click the rotate button → editor opens turned right with the arrow turned; Ctrl+Z there shows it unturned; Ctrl+Shift+R in inline opens it turned left.

- [ ] **Step 6: Commit** `feat(capture): rotate in the inline mode opens the editor turned`.

---

## Execution order

Wave 1 in parallel: Task 1 and Task 3 (separate worktrees branched from `claude/shot-rotate`; Task 1 owns `rotate.rs`, `mod.rs` module line, `export.rs` tests; Task 3 owns `window.rs`, `icons.rs`, `i18n.rs`). Merge both into `claude/shot-rotate`, `cargo test --locked`. Then Task 2, Task 4, Task 5 in order on `claude/shot-rotate`.
