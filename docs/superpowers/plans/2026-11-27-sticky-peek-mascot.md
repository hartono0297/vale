# Sticky note peek follows the chosen mascot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The mascot peeking over a sticky note is the mascot picked in Settings -> Maskot, in its own coat, instead of always the cat.

**Architecture:** The widget host's `Ctx` carries a whole `CatLook` (built with the existing `CatLook::of(&config)`) in place of the cat colour; the host keeps it in `State` and `paint::peek_look` turns it into the peek's look by dropping the accessory. `RobotPainter::paint` already draws every mascot from a `CatLook`.

**Tech Stack:** Rust 2024, Win32 + Direct2D (`windows` crate). Tests: `cargo test --locked`.

**Spec:** `docs/superpowers/specs/2026-11-27-sticky-peek-mascot-design.md` (visual: `docs/design/sticky-peek-maskot.html`)

## Global Constraints

- Touch only `src/widgets/paint.rs`, `src/widgets/host.rs`, `src/widgets/demo.rs`, `src/app_widgets.rs`. No Settings files (a parallel session reworks them).
- Geometry, clip, clock, "z", moods and their order stay exactly as they are.
- The peek never draws an accessory.
- Known flaky on this PC: `app::record::tests::a_folder_is_writable_only_when_a_file_can_be_made_in_it` (drive Z:) always fails; timing tests may fail when the machine is busy and pass alone.

## Review Focus

- Switching the mascot (or a coat) in Settings while notes are on the desktop: the peek changes at once. Path: config adopted -> `widgets_follow_config` -> `widgets_sync` (new look) -> `widgets_restyle` (redraw). Check by reading; no new test.
- A falk or octopus with an accessory set in config: the peek still has none (test in Task 1).
- The falk's and octopus's coats come through, not only the mascot (test in Task 1).

---

### Task 1: Pass the chosen mascot's look to the sticky note peek

**Files:**
- Modify: `src/widgets/paint.rs` (`peek_look` near line 2454, its test near line 2840, the `CatColor` import if unused)
- Modify: `src/widgets/host.rs` (`Ctx.coat` line 174, `State.coat` line 707, the initial state line 825, `sync` line 1398, the destructure line 1868, `cat: paint::peek_look(*coat)` line 2059, the test Ctx near line 5304, the `CatColor` import line 60 if unused)
- Modify: `src/widgets/demo.rs` (its `Ctx` near line 486, the `CatColor` import line 49 if unused)
- Modify: `src/app_widgets.rs:131`

**Interfaces:**
- Produces: `pub fn peek_look(look: CatLook) -> CatLook` (in `widgets::paint`); `Ctx { .., pub look: CatLook, .. }` replacing `pub coat: CatColor`; `State.look: CatLook` replacing `State.coat`.
- Consumes: `crate::ui::cat_style::CatLook` (`Copy`, `Default`, `PartialEq`) and `CatLook::of(&Config)`.

- [ ] **Step 1: Replace the test `the_peek_has_the_coat_and_no_accessory` in `src/widgets/paint.rs` with**

```rust
#[test]
fn the_peek_is_the_chosen_mascot_in_its_coat_with_no_accessory() {
    use crate::theme::{FalkColor, Mascot, OctopusColor};
    let chosen = CatLook {
        mascot: Mascot::Falk,
        color: CatColor::Calico,
        falk: FalkColor::WhiteFace,
        octopus: OctopusColor::Blue,
        accessory: CatAccessory::Bow,
    };
    assert_eq!(peek_look(chosen), CatLook { accessory: CatAccessory::None, ..chosen });
    let octopus = CatLook { mascot: Mascot::Octopus, ..chosen };
    assert_eq!(peek_look(octopus).mascot, Mascot::Octopus);
    assert_eq!(peek_look(octopus).octopus, OctopusColor::Blue);
    assert_eq!(peek_look(octopus).accessory, CatAccessory::None);
}
```

(Use the accessory variant names that exist in `theme::CatAccessory`; `Bow` is used in `ui/render.rs` tests.)

- [ ] **Step 2: Run it and see it fail to compile** (`peek_look` takes a `CatColor`)

Run: `cargo test --locked the_peek_is_the_chosen_mascot`
Expected: compile error, mismatched types.

- [ ] **Step 3: Change `peek_look` to `pub fn peek_look(look: CatLook) -> CatLook`**, returning `look` with `accessory: CatAccessory::None`; update its doc comment ("The chosen mascot in its coat, no accessory ..."). Then make the host carry the look: rename `Ctx.coat: CatColor` to `Ctx.look: CatLook` (doc: "The chosen mascot and its coat, for the peek."), `State.coat` to `State.look` (initial `CatLook::default()`), `s.look = ctx.look;` in `sync`, `cat: paint::peek_look(*look)` in the scene; the test and demo `Ctx` use `look: CatLook::default()`; `app_widgets.rs` passes `look: CatLook::of(config)`. Remove imports this leaves unused.

- [ ] **Step 4: Run the widget tests**

Run: `cargo test --locked widgets::`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/widgets/paint.rs src/widgets/host.rs src/widgets/demo.rs src/app_widgets.rs
git commit -m "fix(sticky): the peek follows the chosen mascot and its coat"
```
