# Jadi muka accessories follow the face Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In Jadi muka every worn accessory piece moves with the face's look slide and nod, clamped as one group.

**Architecture:** `face_pose_in` (robot.rs) already clamps the face's slide and checks the glasses; extend its fit check to every piece `face_pieces` returns for the view, and make `paint_face_pieces` translate every piece (not only `FaceAnchor::Eyes`) by the same slide plus nod that `paint_face` uses.

**Tech Stack:** Rust 2024, Direct2D, `cargo test --locked`.

**Spec:** `docs/superpowers/specs/2026-11-18-face-accessories-follow-design.md`

## Global Constraints

- Plain and Gradasi unchanged; no existing Plain/Gradasi test expectation changes.
- Pieces and marks inside the island shape for both styles, sizes 160..=300 x 28..=44, every Face view, every expression, look in -1..=1, nod at 0 and max; left of the clock / content.
- Blink moves eyes only. Shrink-to-pill behaviour unchanged.
- No new timer, thread or clock. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Never the Fable model.

## Review Focus

1. The hat brim at the top edge with a downward look: the group slide must be cut, not the brim pushed out of the shape. Covered by the Task 1 clamp test.
2. Earcups at the shape's ends with a sideways look: same. Covered by the clamp test.
3. The narrow views (12 dip wider in Face) with the headphone and the unread toast dot (`face_toast_dot`): the dot must still clear the moved earcup. Test in Task 1.

---

### Task 1: One offset and one clamp for the face and its pieces

**Files:**
- Modify: `src/ui/robot.rs` (`face_pose_in`, `face_slide`, `paint_face`)
- Modify: `src/ui/cat_style.rs` (`paint_face_pieces`, `FacePiece` placement by anchor)
- Modify: `src/ui/geometry.rs` (face clamp tests), `src/ui/render.rs` only if `face_toast_dot` must account for the moved earcup

**Interfaces:**
- Consumes: `face_pieces(accessory, kind, shape, eyes, unit)`, `face_wears`, `face_pose_in(..)`, `face_slide(pose, scale)`, `paint_face`, `paint_face_pieces`.
- Produces: `face_pose_in` returns a pose whose slide keeps face marks and every worn piece inside; `paint_face_pieces` applies that slide and the nod to every piece.

- [ ] **Step 1: Write the failing tests:**
  - `every_worn_piece_moves_with_the_eyes`: for each accessory and Face view, with a look slide and a nod, every piece's drawn box moves by exactly the eyes' offset (compare at rest vs posed).
  - `the_face_and_its_pieces_stay_inside_when_they_move`: the loop of `every_face_accessory_stays_inside_the_notch` extended with look `v` in -1..=1 step 0.25 and nod 0 / max, clamped through `face_pose_in`: every piece box and every mark inside `shape_contains` and left of the clock.
  - `the_toast_dot_clears_the_moving_earcup` (if the dot is affected).
- [ ] **Step 2:** run, fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** `cargo test --locked` green.
- [ ] **Step 5: Commit** `feat(notch-colour): accessories move with the face in Jadi muka`.
