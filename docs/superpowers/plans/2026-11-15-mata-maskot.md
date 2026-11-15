# Mascot face in the shrunk notch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The 34 x 14 dip "Mengecil jadi mata" pill shows the chosen mascot's mini face (fill, eyes, marks, edge) instead of terracotta cat eyes.

**Architecture:** A pure table `peek_face(CatLook, surface) -> PeekFace` in a new `src/ui/peek_face.rs`. The existing eyes painter (`RobotPainter::paint_eyes`, `Renderer::paint_eyes_pill`) draws from it, and the look clamp (`eyes_offset_in`) also covers the face marks. The island fill blends from the surface to the face colour with a new eased `peek_mix` (0 = notch, 1 = face), run the same way as the existing `fill` pair in `App`.

**Tech Stack:** Rust 2024, Direct2D through the `windows` crate, unit tests in `#[cfg(test)]` modules, `cargo test --locked`.

**Spec:** `docs/superpowers/specs/2026-11-15-mata-maskot-design.md` (colours, mark geometry and behaviour are there, verbatim; read it first).

## Global Constraints

- No new timer, thread, timer id or frame-clock use at rest (`docs/memory/bata-idle-power-budget.md`). The mix runs only inside the existing 320 ms morph.
- Pill size, place, hit test, peek state machine, blink and look timing are unchanged.
- Colours come from the existing constants (`mascot::falk_ink`, `mascot::octopus_rgb`, `mascot::shade`, `cat_style::{EYES_DARK, AMBER, RIM, PATCH_ORANGE, PATCH_DARK}`, `CatLook::head_rgb`). Never copy hex values where a constant exists. The new beak colour is `0xD9978F`, the falk's own beak (reuse its constant if `mascot.rs` has one).
- Mark geometry is in the eyes' 64-unit grid (eyes at (24, 40) and (40, 40), `EYES_SCALE` 0.8 dip per unit).
- Test command: `cargo test --locked` (whole suite green before every commit). Build in the worktree `.claude/worktrees/mata-maskot`, branch `feat/mata-maskot`.
- Code comments follow the repo style: plain sentences on why, no generic filler.

## Review Focus

1. A falk whose cheeks would leave the pill at the look's extremes: the cheeks must stay whole (the clamp shortens the look) in both styles and every morph shape. Test in Task 1.
2. A coat change while the notch is shrunk, at rest: the dirty-rect path must redraw the whole pill. `Drawn` already compares `cat` and `palette`; Task 3 adds `peek_mix` and a test that a changed `CatLook` gives `Plan::Full`.
3. A restore that starts while the shrink morph is half done: the colour must carry on from the reached mix, not jump. Test in Task 3 (`the_face_mix_carries_on_from_where_a_cut_morph_was`).
4. See-through notch (`fill_alpha` < 1): at mix 1 the pill is opaque, at mix 0 the old alpha. Test in Task 3 (`peek_fill`).
5. Motion Saver / Off and Energy Saver: no blend frames, the colour is at its target at once. Test in Task 3.

---

### Task 1: The face table and the clamp over its marks

**Files:**
- Create: `src/ui/peek_face.rs` (and `mod peek_face;` in `src/ui/mod.rs`)
- Modify: `src/ui/robot.rs` (`eyes_boxes_at`, `eyes_offset_in`, around lines 690-750)
- Modify: `src/ui/render.rs:3143` (the one caller of `eyes_offset_in`), `src/ui/geometry.rs:2574` (`the_shrunk_eyes_stay_inside_the_pill`)

**Interfaces:**
- Produces, in `crate::ui::peek_face`:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum PeekMark {
      /// A filled circle in the eyes' grid.
      Dot { cx: f32, cy: f32, r: f32, rgb: u32 },
      /// The falk's beak, the spec's path, in the eyes' grid.
      Beak { rgb: u32 },
  }
  #[derive(Clone, Debug, PartialEq)]
  pub struct PeekFace {
      pub fill: u32,
      pub eye: u32,
      /// The 1 dip line on the pill's sides and bottom; `None` for none.
      pub edge: Option<u32>,
      /// Slide with the look and are clamped with the eyes (cheeks, beak).
      pub face_marks: Vec<PeekMark>,
      /// Fixed to the pill and clipped to it (calico patches, octopus spots).
      pub coat_marks: Vec<PeekMark>,
  }
  pub fn peek_face(look: CatLook, surface: u32) -> PeekFace;
  /// Grid boxes (left, top, right, bottom) of `marks`.
  pub fn mark_boxes(marks: &[PeekMark]) -> Vec<(f32, f32, f32, f32)>;
  pub const BEAK_PATH: ... // the spec's beak as `robot::Seg`s, for painting and its box
  ```
- Changes in `crate::ui::robot`:
  `eyes_boxes_at(centre, dx, dy, blink, marks: &[PeekMark])` returns the eyes' boxes plus `mark_boxes(marks)` mapped the same way; `eyes_offset_in(style, shape, dx, dy, marks: &[PeekMark])` tests them all.

- [ ] **Step 1: Write the failing tests** in `peek_face.rs`

  - `every_coat_has_the_approved_face`: for all 11 coats, with `surface = 0x151C2E`, assert `fill`, `eye`, `edge` per the spec's table: Terracotta `(0xE2683C, EYES_DARK, None)`; Calico `(0xF4EDE4, EYES_DARK, Some(surface))`, two coat dots `PATCH_ORANGE` at (17.625, 35) r 4.25 and `PATCH_DARK` at (47.625, 45.625) r 3.75; White `(0xF6F1EA, EYES_DARK, Some(surface))`; Black `(0x1E1B22, AMBER, Some(RIM))`; Falk Lutino / Grey fill = `falk_ink(c).face`, eye `EYES_DARK`, edge `None`, face marks = cheeks `0xEC6B3A` at (18.75, 44) and (45.25, 44) r 2.375 plus `Beak { rgb: 0xD9978F }`; Falk WhiteFace fill `0xF4EFE6`, edge `Some(surface)`, face marks = the beak only; Octopus each coat fill = `octopus_rgb(c)`, eye `EYES_DARK`, edge `None`, coat marks = dots `shade(fill)` at (44.25, 36.5) r 1.625 and (19, 35.75) r 1.125, no face marks.
  - `the_accessory_does_not_change_the_face`: same face for every `CatAccessory`.
  - `the_beak_box_is_the_spec_path`: `mark_boxes(&[Beak..])` is (30.375, 42.0..43.0, 33.625, 46.375) within 0.5 (control points bound it).

- [ ] **Step 2: Extend `the_shrunk_eyes_stay_inside_the_pill`** in `geometry.rs`: loop it over `peek_face(look, 0).face_marks` for a cat, a Lutino falk and a WhiteFace falk; boxes from `eyes_boxes_at(.., &marks)` stay inside every morph shape for every offset. For the cat the first six offsets still come back unchanged. Add `the_falk_looks_a_little_less_far`: in both styles, at the final pill, `eyes_offset_in(style, pill, EYES_LOOK_X_DIP, 0.0, &falk_marks).0` is `> 0.0` and `<= EYES_LOOK_X_DIP`, and the cheeks' boxes are inside.

- [ ] **Step 3: Run them to see them fail** (`cargo test --locked peek_face` and `cargo test --locked shrunk_eyes`; expected: compile errors / missing items).

- [ ] **Step 4: Implement** `peek_face`, `mark_boxes`, the beak path, and the two `robot.rs` signature changes. The render caller passes `&peek_face(frame.cat, self.palette.surface).face_marks` for now. Keep the painter as it is (Task 2 paints).

- [ ] **Step 5: Run `cargo test --locked`.** Expected: all green.

- [ ] **Step 6: Commit** `feat(peek): a face per mascot coat for the shrunk notch, clamped with its marks`

### Task 2: Paint the face

**Files:**
- Modify: `src/ui/robot.rs` (`paint_eyes`, ~line 1308), `src/ui/render.rs` (`paint_eyes_pill`, ~line 3134)
- Test: `src/ui/peek_face.rs` tests (the WIC harness of `mascot.rs` tests, ~line 760, copied as a small helper)

**Interfaces:**
- Consumes: `PeekFace`, `PeekMark`, `BEAK_PATH`, `eyes_offset_in(.., marks)` from Task 1.
- Produces: `RobotPainter::paint_eyes(&self, rt, brush, area: Rect, alpha: f32, blink: bool, face: &PeekFace)`: eyes and blink lines in `face.eye`, then `face.face_marks`, all in the same transform. `RobotPainter::paint_peek_marks(&self, rt, brush, area: Rect, alpha: f32, marks: &[PeekMark])` for the coat marks (same grid mapping, no slide).

- [ ] **Step 1: Write the failing pixel tests** `the_shrunk_face_paints_each_mascot`: render `paint_eyes` into a 64 x 64 bitmap at `k = 1` (area = the whole grid, black clear) and assert with `assert_near`: cat Terracotta eye pixel (24, 40) = `EYES_DARK`; cat Black (24, 40) = `AMBER`; Lutino (24, 40) = `EYES_DARK`, cheek (18, 44) = `0xEC6B3A`, beak (32, 44) = `0xD9978F`; WhiteFace (18, 44) is still black (no cheek); blink on Lutino: (24, 40.6) row has `EYES_DARK`, (24, 36) is black. And `paint_peek_marks` for Calico gives `PATCH_ORANGE` at (17, 35).

- [ ] **Step 2: Run to see it fail** (signature mismatch).

- [ ] **Step 3: Implement.** `paint_eyes` loses `TERRACOTTA` and takes `face`. In `paint_eyes_pill`, compute `let face = peek_face(self.cat.get(), self.palette.surface)` (the frame's look is in `self.cat` during paint; check, else pass `frame.cat`); inside the existing axis clip, also clip to the pill outline (the island geometry the fill used: rounded rect for Pill, `notch_path` for Notch) before drawing the coat marks with `paint_peek_marks` at the *unslid* `eyes_area(eyes_centre(shape))`; then the slid eyes and face marks. The edge, when `Some`, is a 1 dip line inside the same outline (stroke width 2 dip clipped to the outline gives exactly 1 dip inside; the top edge may show), at `alpha`.

- [ ] **Step 4: `cargo test --locked`**, all green.

- [ ] **Step 5: Commit** `feat(peek): paint the mascot's mini face in the shrunk notch`

### Task 3: The fill turns into the face colour with the morph

**Files:**
- Modify: `src/app.rs` (a `peek_mix: (f32, f32)` field beside `fill`, set in `refresh` where `eyes_swap` is known ~line 4173 and settled like `fill` in `sync_fill` / the morph's end; `Frame` built ~line 7766)
- Modify: `src/ui/render.rs` (`Frame::peek_mix: f32` beside `eyes` ~line 274; the fill in `paint_island` ~line 2486; the test frame builder ~line 5658; `signature`)
- Modify: `src/ui/dirty.rs` (`Drawn::peek_mix` and its line in `plan_is_full_when_any_signature_field_differs`)
- Test: `src/ui/peek_face.rs`, `src/app.rs` tests (next to `an_eyes_morph_takes_320_ms_and_never_overshoots`, ~line 10944), `src/ui/dirty.rs` tests

**Interfaces:**
- Consumes: `peek_face` from Task 1.
- Produces: `pub fn peek_fill(surface: u32, fill_alpha: f32, face_fill: u32, mix: f32) -> (u32, f32)` in `peek_face.rs`: the colour linearly mixed per channel from `surface` (mix 0) to `face_fill` (mix 1), the alpha `fill_alpha + (1 - fill_alpha) * mix`; `mix` clamped to 0..1. `App::peek_mix_at(&self, now: Instant) -> f32`, eased exactly like `fill_at` (`self.anim.curve.ease(self.anim.progress(now))`, then `from + (to - from) * t`).

- [ ] **Step 1: Write the failing tests.**
  - `peek_fill` (peek_face.rs): `peek_fill(0x17120F, 1.0, 0xF6D04A, 0.0) == (0x17120F, 1.0)`; at 1.0 `== (0xF6D04A, 1.0)`; `peek_fill(s, 0.4, f, 1.0).1 == 1.0`; `peek_fill(s, 0.4, f, 0.0).1 == 0.4`; mix 0.5 is the channel midpoint (rounded); mix -1 and 2 clamp.
  - app.rs `the_face_mix_follows_the_eyes_morph`: shrinking with Motion Full, `peek_mix_at` is 0 at the morph's start, strictly between at 160 ms, 1 at 320 ms and after; restoring goes back to 0 the same way.
  - app.rs `the_face_mix_carries_on_from_where_a_cut_morph_was`: restore at 160 ms into a shrink; the mix right after equals the mix just before (within 1e-3) and ends at 0.
  - app.rs `without_motion_the_face_mix_is_there_at_once`: Motion Off (and Energy Saver) shrink: `peek_mix_at(now) == 1.0` straight away; restore: `0.0`.
  - dirty.rs: add `Box::new(|d| d.peek_mix = 0.5)` to the changed list, and assert a changed `cat.mascot` gives `Plan::Full` (already covered by `d.cat.color`; add `d.cat.mascot = Mascot::Falk`).
  If the app test harness cannot drive `refresh` this way, test the pair's arithmetic through a small pure `struct PeekMix { from, to }` with `at(eased_t)` and `retarget(to, reached)` in `peek_face.rs`, and keep the App wiring thin.

- [ ] **Step 2: Run them to see them fail.**

- [ ] **Step 3: Implement.** On `eyes_swap` with a morph: `peek_mix = (peek_mix_at(now), if self.eyes {1.0} else {0.0})`; when the morph is settled (`Anim::settled` / not running) both ends are the target. `Frame::peek_mix = self.peek_mix_at(now)`. In `paint_island`, the fill colour and alpha come from `peek_fill(self.palette.surface, frame.fill_alpha, face.fill, frame.peek_mix)` with `face = peek_face(frame.cat, self.palette.surface)`; at mix 0 this is exactly today's call. A play never runs while shrunk (`playing(.., frame.eyes)`), so the played fill can keep its colour, but use the same `peek_fill` for it anyway (mix is 0 there). `Drawn` records `peek_mix`.

- [ ] **Step 4: `cargo test --locked`**, all green.

- [ ] **Step 5: Demo check.** Build release and run `--demo` held on `DI_DEMO_STEP=peek-eyes` with `DI_MASCOT=falk`, `octopus`, `cat` (and one pale coat, e.g. `DI_FALK_COLOR` white face) and look at the pill; record what was looked at. If the `peek-eyes*` scenes do not take `DI_MASCOT`, make them (`src/ui/demo.rs` ~line 2479) and say so in the commit.

- [ ] **Step 6: Commit** `feat(peek): the shrunk notch's fill turns into the mascot's face colour`
