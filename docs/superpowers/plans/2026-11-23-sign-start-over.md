# Sign maker "Start over" fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A click or tap on "Start over" in the New signature window's Draw tab clears the drawing instead of starting a stroke.

**Architecture:** The Redo ("Start over") button sits inside the pad rect. `pen_down` only checks `pad.contains`, and `WM_LBUTTONDOWN` / `WM_POINTERDOWN` call `pen_down` before the button hit test, so the click becomes a dot and `act(MHit::Redo)` never runs. Fix: a stroke starts only where `MLayout::hit` returns `MHit::Pad`, i.e. no button covers the point.

**Tech Stack:** Rust 2024, Win32.

**Spec:** the user's report ("tombol starover nya bug", screenshot 2) and the agreed fix in this session.

## Global Constraints

- Windows 11 x64, `cargo test --locked`.
- Touch only `src/sign/window/maker.rs`.

## Review Focus

- Pen/touch tap on Start over: `WM_POINTERDOWN` must fall through to `DefWindowProcW` so Windows turns it into a click (covered by the same `starts_stroke` check).
- Drawing anywhere else on the pad, including right next to the button, still starts a stroke (test below).
- Type and Photo tabs never start a stroke (test below).

---

### Task 1: Strokes start only on bare pad

**Files:**
- Modify: `src/sign/window/maker.rs` (`pen_down`, new `starts_stroke`, tests module)

**Interfaces:**
- Produces: `fn starts_stroke(layout: &MLayout, tab: Tab, x: f32, y: f32) -> bool`

- [ ] **Step 1: Write the failing test** `start_over_is_not_ink` in the tests module:
  layout `maker_layout(1.0, Tab::Draw, 0, true, &LAB)`; centre of `rect_of(MHit::Redo)` → `starts_stroke` is false; centre of `pad` → true; a point on the pad just left of the Redo rect → true; `Tab::Type` layout pad centre → false.
- [ ] **Step 2: Run** `cargo test --locked start_over_is_not_ink` → FAIL (no `starts_stroke` yet / first assertion).
- [ ] **Step 3: Implement** `starts_stroke` as `tab == Tab::Draw && layout.hit(x, y) == Some(MHit::Pad)`, and use it in `pen_down`.
- [ ] **Step 4: Run** `cargo test --locked start_over_is_not_ink` → PASS, then `cargo test --locked` → all green except the known drive Z: test.
- [ ] **Step 5: Commit** `fix(sign): Start over clears the drawing instead of drawing a dot`.
