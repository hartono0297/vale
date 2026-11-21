# Inline Rotate In Place Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In the inline capture mode, the Rotate button and Ctrl+R / Ctrl+Shift+R turn the picked region in place on the frozen screen (a lifted, turned card you keep drawing on), instead of opening the editor.

**Architecture:** Annotations stay in frame px while `turns == 0`; a turn from 0 shifts them into crop px and turns them, a turn back to 0 turns and shifts them back, each as one `Session` step, so undo always restores a matching (doc, turns) pair. The overlay caches the turned crop as a lifted picture, paints it as a card with a scaled `View`, maps the pointer through that view, and gates the selection editing while turned. Output uses the turned crop and the doc as-is.

**Tech Stack:** Rust 2024, `windows` crate (Win32 + Direct2D), unit tests in `#[cfg(test)]` modules.

**Spec:** `docs/superpowers/specs/2026-11-21-inline-rotate-in-place-design.md`

## Global Constraints

- Windows 11 x64; `cargo test --locked` must pass after every task; no new dependencies; no new build warnings (the pre-existing `src/config.rs` `parse` warning aside).
- Ctrl+R = turn right in place, Ctrl+Shift+R = turn left in place (inline); same gates as today (`inline::key_action`: not while typing text, dragging, or with the confirm bar up; no key repeat).
- Rotate button tooltip: "Putar ke kanan (Ctrl+R)" / "Rotate right (Ctrl+R)" (`Key::EditRotateRight`). `Key::InlineRotate` and `FinishKind::OpenEditorTurned` are removed.
- Card display scale `s = min(1, fit)` with a 48 dip margin per side on the selection's monitor; output is always full size.
- Four turns give back the untouched state; a turn is one undo step.
- Never run `cargo fmt` repo-wide (the repo is not fmt-clean); format only touched lines.
- Never use the Fable model.

## Review Focus

1. Draw, then turn, then undo twice: the annotation is back in frame px at its original place and the selection is editable again (Task 1 test `turn_from_zero_and_back_restores_frame_px`, Task 2 manual).
2. A selection on a second monitor with negative coordinates: the card stays on that monitor (Task 1 test `card_stays_on_a_negative_monitor`).
3. A wide selection that turned is taller than the monitor: the card shrinks to fit, the copied picture is full size (Task 1 test `a_card_taller_than_the_monitor_shrinks`, Task 2 manual).
4. Pixelate drawn on the turned card mosaics the turned crop, and Copy equals the card (Task 2 manual + review).
5. Esc with only a turn asks before discarding (Task 2 test on the key context `has_annotations || turned`).

---

### Task 1: Pure rules (session step, card, mapping, shifts)

**Files:**
- Modify: `src/shot_edit/mod.rs` (`Session::rotate`, ~548) and its tests
- Modify: `src/capture/inline.rs` (new pure fns near `place_side` ~277) and its tests

**Interfaces:**
- Produces:
  - `Session::rotate_shifted(&mut self, turn: Turn, size: (u32, u32), pre: (f32, f32), post: (f32, f32)) -> bool` — one undo step: `move_by(pre)` on every annotation, `rotate_doc(turn, size)`, `move_by(post)`, update `turns`; returns true. `Session::rotate(turn, size)` becomes `rotate_shifted(turn, size, (0.0, 0.0), (0.0, 0.0))`.
  - `pub fn turned_card(sel: Rect, turns: u8, monitor: Rect, ui: f32) -> (Rect, f32)` in inline.rs — turned size `(tw, th)` = `(sel.h, sel.w)` for odd `turns`, else `(sel.w, sel.h)`; margin `m = (48.0 * ui).round()`; `s = min(1, (mon.w - 2m)/tw, (mon.h - 2m)/th)`; card `w = (tw*s).round().max(1)`, `h` likewise; centred on `sel.center()`, then x clamped to `[mon.x, mon.x + mon.w - w]`, y likewise. Returns (card, s).
  - `pub fn card_local(p: (i32, i32), card: Rect, scale: f32, size: (u32, u32)) -> Pt` — `((p.0 - card.x) as f32 / scale, (p.1 - card.y) as f32 / scale)` clamped to `0..=size`.
  - `pub fn turn_shifts(sel: Rect, origin: (i32, i32), turns_before: u8, turn: Turn) -> ((f32, f32), (f32, f32), (u32, u32))` — returns `(pre, post, size)`; `off = (sel.x - origin.0, sel.y - origin.1)`; `after = (turns_before + 1 or 3) % 4`; if `turns_before == 0`: `pre = -off`, `post = 0`, `size = (sel.w, sel.h)`; else if `after == 0`: `pre = 0`, `post = +off`, `size` = selection turned by `turns_before`; else `pre = post = 0`, `size` = selection turned by `turns_before`.

- [ ] **Step 1: Write the failing tests**
  - mod.rs `rotate_shifted_moves_turns_and_moves_back_in_one_step`: doc [Rect (110,60,20,10)], `rotate_shifted(Right, (100,40), (-100.0,-50.0), (0.0,0.0))` → Rect (20,10,10,20) (crop px (10,10,20,10) on 100×40 turned right: corners (10,10)->(30,10), (30,20)->(20,30)), `turns()==1`, one `undo()` restores the original doc and turns 0.
  - inline.rs `card_fits_and_is_centred`: sel (100,100,400,200), turns 1, MON 1920×1080, ui 1 → card (200,0,200,400), s 1.0 (centred on (300,200)).
  - `a_card_taller_than_the_monitor_shrinks`: sel (0,500,1800,300), turns 1, MON → th 1800 > 1080-96 → s = 984/1800, card h = 984, inside the monitor.
  - `card_stays_on_a_negative_monitor`: monitor (-1920,0,1920,1080), sel (-300,900,600,100), turns 1 → card fully inside the monitor.
  - `even_turns_keep_the_selection_size`: turns 2 → card size == sel size, s 1.
  - `card_local_maps_back_to_turned_crop_px`: card (200,0,200,400), s 0.5, size (400,800): p (300,100) → (200,200); p outside clamps to 0/size.
  - `turn_shifts_from_zero_lifts_into_crop_px`, `turn_shifts_back_to_zero_puts_into_frame_px`, `turn_shifts_between_turns_do_not_move`: values per the Interfaces formulas with sel (110,60,100,40), origin (10,10).
  - `turn_from_zero_and_back_restores_frame_px`: Session with a Rect in frame px; apply Right with `turn_shifts(sel, origin, 0, Right)`, then Left with `turn_shifts(sel, origin, 1, Left)` → doc equals the original within 1e-3, turns 0.

- [ ] **Step 2: Run** `cargo test --locked rotate_shifted card_ turn_shifts turn_from_zero` (or the module filters) — expected FAIL (missing fns).
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Run** `cargo test --locked` — expected PASS.
- [ ] **Step 5: Commit** `feat(capture): pure rules for turning the inline selection in place`.

### Task 2: Overlay glue (lifted card, input, gates, output)

**Files:**
- Modify: `src/capture/overlay/edit.rs` (Edit state ~88, `Env::local` ~183, `relayout` ~228, `press` ~479, `on_move` ~632, `do_btn` ~789, key dispatch ~946, key context ~892, `draw` ~1191)
- Modify: `src/capture/overlay.rs` (`inline_picture` ~501, `finish_inline` ~521-600)
- Modify: `src/capture/inline.rs` (`tip` for `Btn::Rotate`, remove `FinishKind::OpenEditorTurned` and its tests/arms, `PAINT_ORDER` if a layer is added)
- Modify: `src/i18n.rs` (remove `InlineRotate`)

**Interfaces:**
- Consumes: Task 1 `Session::rotate_shifted`, `turned_card`, `card_local`, `turn_shifts`; `shot_edit::rotate::{Turn, turned}`; `capture::crop`.
- Produces / behaviour:
  - `Edit` gains a lifted cache: `lifted: Option<Lifted>` with `struct Lifted { turns: u8, shot: Arc<Shot>, bitmap: Option<ID2D1Bitmap> }` (turned crop of `sel`), rebuilt whenever `session.turns()` differs from `lifted.turns` and dropped at 0; the bitmap is dropped with the target like `s.picture`. Also drop `pix` (mosaic cache) on any turns change.
  - `fn turn_in_place(e: &mut Edit, frame: &Frozen, turn: Turn)`: commit open text first (as other strip buttons do), `turn_shifts(e.sel, frame.origin, e.session.turns(), turn)` → `session.rotate_shifted(..)`, then sync the lifted cache and `relayout`. `Btn::Rotate` → `turn_in_place(Right)`; `InlineKey::Rotate(t)` → `turn_in_place(t)`. Undo/redo also sync the cache and relayout when turns changed.
  - Turned state (`session.turns() != 0`): `(card, s) = turned_card(e.sel, turns, monitor, ui)`; draw the frame, the dim over everything except the card, the lifted bitmap into the card, annotations via `draw_annotations_clipped(.., &lifted.shot, doc, &View { scale: s, ox: card.x - origin.0, oy: card.y - origin.1 }, .., clip = card in client px)`, the selected annotation's handles with the same view, the border around the card, the size label with the turned size, no selection handles. Strip and bar placed around `card` (same `place_side`/`separate` calls with `card` instead of `sel`).
  - Pointer in turned state: annotation coordinates come from `card_local(p, card, s, lifted size)`; selection drags (`NewSel`, `MoveSel`, `ResizeSel`, handle hits) are not started; a press outside the card and outside the strip/bar does nothing. Hit tolerances scale by `1/s`.
  - Text EDIT in turned state: placed at the card-mapped point and sized with the text size × `s` (builder's call on details).
  - Key context: `has_annotations: !doc.is_empty() || session.turns() != 0`.
  - `inline_picture()` when turned: `(sel, lifted shot clone (or turned(crop(sel), turns)), doc.clone())` — no `doc_in_crop` shift. Untouched otherwise.
  - Editor handoff (`OpenEditor`) passes that picture and doc; `open_turned(.., None)`.
  - Remove `FinishKind::OpenEditorTurned` (+ `keeps_picture`/`finish_copies` arms and tests) and `Key::InlineRotate`; `tip(Btn::Rotate)` = `format!("{} (Ctrl+R)", tr(Key::EditRotateRight))`.

- [ ] **Step 1: Update/write tests** (inline.rs): `rotate_tip_names_its_shortcut` → English "Rotate right (Ctrl+R)"; FinishKind tests without `OpenEditorTurned`; key-context test that `has_annotations` (or the AskDiscard outcome) is true for a turn-only state if the context is built by a testable fn (otherwise assert through `key_action` with `has_annotations: true` and cover the edit.rs wiring by review).
- [ ] **Step 2: Run** `cargo test --locked capture::inline` — expected FAIL.
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Run** `cargo test --locked` — expected PASS; `cargo check --locked` shows no new warning.
- [ ] **Step 5: Manual check** with `cargo run -- --demo-snap inline`: pick a region, draw an arrow and a pixelate, Ctrl+R → card turned in place with the arrow turned and the strip/bar beside the card; draw a rect on the card; Ctrl+Z twice → back to the plain selection with handles; Ctrl+Shift+R → turned left; a wide region whose turn is taller than the monitor shows a smaller card; Copy (demo: inert, footer says so) / Save writes a turned file in `target/demo-shots/`; "Buka di editor" from turned opens the editor on the turned picture. Capture PNGs with PrintWindow; report what was and wasn't driven.
- [ ] **Step 6: Commit** `feat(capture): turn the inline selection in place`.

## Execution order

Task 1 then Task 2 (Task 2 consumes Task 1's functions and touches inline.rs too).
