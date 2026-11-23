# Ink colour for placed signatures Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A placed Signature or Initials can switch ink (black, blue, red) from its floating bar.

**Architecture:** `Mark` (straight RGBA bitmap, one colour plus alpha) learns to report its ink and to return a recoloured copy. The window's item bar shows the three `style::Tint` swatches for marks and swaps a new `Arc<Mark>` into the item through `Session::restyle` (one undo step). "Pasang di semua halaman" learns that a same-shape initials in another colour at the spot is the same paraf, and gives it the selected colour instead of stacking a copy.

**Tech Stack:** Rust 2024, Win32 + Direct2D, unit tests inside `src/**`.

**Spec:** `docs/superpowers/specs/2026-11-23-warna-tanda-tangan-design.md`

## Global Constraints

- Colours are exactly `style::Tint::ALL` = Black 0x1C1C1C, Blue 0x1F3A8A, Red 0xA61B1B. No new colours.
- Do not change `ItemKind`, the writers (`pdf_write.rs`, `raster.rs`, `image_out.rs`), the store, `ink.rs` or `window/maker.rs` (parallel sessions edit `ink.rs` and `maker.rs`).
- Recolour only the selected item; saved signatures and the maker stay as they are.
- Picking the colour an item already has changes nothing and adds no undo step.
- Run tests with `cargo test --locked <filter>`. The Z: drive test always fails on this PC; ignore it.

## Review Focus

1. A mark with no opaque pixel, or `rgba` shorter than `w*h*4`: `tint()` returns `None`, `recolored` does not panic. (Task 1 test.)
2. A drawn mark whose edge pixels' RGB drifts after un-premultiplying: `tint()` reads the most opaque pixel, so it still finds the ink. (Task 1 test.)
3. Initials put on all pages, one copy recoloured, then "Pasang di semua halaman" again: no stacked duplicates; the other copies take the new colour. (Task 2 test.)
4. Undo after a recolour brings the old ink back, redo the new one. (Task 3 test.)
5. A Signature's bar still ends with + Date and Remove, an Initials' bar with Pasang di semua halaman and Remove. (Task 3 test.)

---

### Task 1: `Mark` reports and changes its ink

**Model:** haiku (mechanical).

**Files:**
- Modify: `src/sign/mod.rs` (add `impl Mark` after the struct, tests in the existing `tests` module)

**Interfaces:**
- Produces:
  - `impl Mark { pub fn tint(&self) -> Option<style::Tint> }`: the RGB of the pixel with the highest alpha (first one on a tie; alpha must be > 8), matched to the `Tint` in `Tint::ALL` with the smallest squared RGB distance, accepted only if every channel differs by at most 24; else `None`. `None` also when `rgba.len() < w*h*4` or nothing is opaque.
  - `pub fn recolored(&self, t: style::Tint) -> Mark`: same `w`, `h`, every pixel's RGB set to `t.rgb()` (0xRRGGBB), alpha unchanged; works on whatever `rgba` holds (chunks of 4).
  - `pub fn same_shape(&self, other: &Mark) -> bool`: equal `w`, `h`, `rgba.len()` and every alpha byte equal.

- [ ] **Step 1: Write the failing tests** in `src/sign/mod.rs` `tests`:

```rust
#[test]
fn mark_tint_reads_the_most_opaque_pixel() {
    use style::Tint;
    // A faint edge pixel in a slightly off colour, then the solid ink.
    let m = Mark { w: 2, h: 1, rgba: vec![0x40, 0x50, 0x70, 60, 0x1F, 0x3A, 0x8A, 255] };
    assert_eq!(m.tint(), Some(Tint::Blue));
    let near = Mark { w: 1, h: 1, rgba: vec![0x30, 0x24, 0x20, 200] };
    assert_eq!(near.tint(), Some(Tint::Black), "within 24 per channel");
    assert_eq!(Mark { w: 1, h: 1, rgba: vec![0, 200, 0, 255] }.tint(), None, "green is no ink");
    assert_eq!(Mark { w: 1, h: 1, rgba: vec![0x1F, 0x3A, 0x8A, 8] }.tint(), None, "nothing opaque");
    assert_eq!(Mark { w: 2, h: 2, rgba: vec![0; 4] }.tint(), None, "short buffer");
}

#[test]
fn mark_recolored_keeps_alpha() {
    use style::Tint;
    let m = Mark { w: 2, h: 1, rgba: vec![0x1F, 0x3A, 0x8A, 255, 0x1F, 0x3A, 0x8A, 30] };
    let r = m.recolored(Tint::Red);
    assert_eq!(r.rgba, vec![0xA6, 0x1B, 0x1B, 255, 0xA6, 0x1B, 0x1B, 30]);
    assert_eq!((r.w, r.h), (2, 1));
    assert_eq!(r.tint(), Some(Tint::Red));
    assert!(r.same_shape(&m) && m.same_shape(&r));
    assert!(!m.same_shape(&Mark { w: 2, h: 1, rgba: vec![0x1F, 0x3A, 0x8A, 255, 0x1F, 0x3A, 0x8A, 31] }));
    assert!(!m.same_shape(&Mark { w: 1, h: 2, rgba: m.rgba.clone() }));
    assert!(Mark { w: 0, h: 0, rgba: Vec::new() }.recolored(Tint::Blue).rgba.is_empty());
}
```

- [ ] **Step 2: Run** `cargo test --locked mark_` → FAIL (no method `tint` / `recolored` / `same_shape`).
- [ ] **Step 3: Implement** the three methods per Interfaces.
- [ ] **Step 4: Run** `cargo test --locked mark_` → PASS.
- [ ] **Step 5: Commit** `feat(sign): marks report and change their ink`.

### Task 2: "Pasang di semua halaman" carries the colour

**Model:** sonnet. Runs after Task 1, in parallel with Task 3 (different file).

**Files:**
- Modify: `src/sign/session.rs` (`initials_on_all`, `initials_everywhere`, `has_initials_at`; test in `tests`)

**Interfaces:**
- Consumes: `Mark::same_shape(&self, &Mark) -> bool` (Task 1).
- Produces: same public signatures as today. `initials_on_all(i)` now returns pages that got a copy **plus** pages whose same-shape initials at the spot changed colour, all in one undo step. `initials_everywhere(i)` is true only when every other live page has initials at the spot that equal the selected mark (`Arc::ptr_eq` or `==`).

Behaviour: on each other live page, an initials item within half a point of `x, y` whose mark `same_shape`s the selected one counts as "there". If its mark is not equal (`ptr_eq` or `==`), its kind becomes `ItemKind::Initials(mark.clone())` (the selected Arc). If none is there, a copy is added as today (same fit rule). One `checkpoint()` only when something changes.

- [ ] **Step 1: Write the failing test** in `session.rs` `tests`:

```rust
#[test]
fn initials_on_all_gives_copies_the_new_colour() {
    let mut s = three();
    let blue = Arc::new(Mark { w: 2, h: 1, rgba: vec![0x1F, 0x3A, 0x8A, 255, 0x1F, 0x3A, 0x8A, 40] });
    let i = s.add(item(ItemKind::Initials(blue.clone()), 0, 500.0, 40.0, 48.0, 24.0));
    assert_eq!(s.initials_on_all(i), 2);
    let red = Arc::new(blue.recolored(crate::sign::style::Tint::Red));
    s.items[i].kind = ItemKind::Initials(red.clone());
    assert!(!s.initials_everywhere(i), "the other pages are still blue");
    let before = s.items.len();
    assert_eq!(s.initials_on_all(i), 2, "both copies took the colour");
    assert_eq!(s.items.len(), before, "nothing stacked");
    assert!(s.items.iter().all(|it| matches!(&it.kind, ItemKind::Initials(m) if **m == *red)));
    assert!(s.initials_everywhere(i));
    assert!(s.undo());
    assert!(s.items[1..].iter().all(|it| matches!(&it.kind, ItemKind::Initials(m) if **m == *blue)), "one undo step");
}
```

- [ ] **Step 2: Run** `cargo test --locked initials_` → the new test FAILS (4 items after the second call), the old ones pass.
- [ ] **Step 3: Implement** per Behaviour.
- [ ] **Step 4: Run** `cargo test --locked sign::session` → PASS (old `initials_on_all_twice_makes_no_duplicates` and upright tests included).
- [ ] **Step 5: Commit** `feat(sign): all-pages initials carry the selected colour`.

### Task 3: Swatches in the bar of a signature or initials

**Model:** sonnet. Runs after Task 1, in parallel with Task 2.

**Files:**
- Modify: `src/sign/window/window.rs`: `float_hits` (~l.1241), `selected_tint` (~l.1829), `tint_selected` (~l.1890), test `format_controls_behave` (~l.3382). Paint (`paint.rs` `Hit::Tint`) already draws swatches and the ring from `selected_tint`; no change there.

**Interfaces:**
- Consumes: `Mark::tint`, `Mark::recolored` (Task 1); `Session::restyle(i, kind, w)` (existing, one undo step).
- Produces: `float_hits(&ItemKind::Signature(_))` = `[Tint(Black), Tint(Blue), Tint(Red), Smaller, Bigger, AddDate, Remove]`; Initials the same with `AllPages` in place of `AddDate`. `selected_tint` returns `mark.tint()` for Signature / Initials. `tint_selected` on a Signature / Initials whose `mark.tint() != Some(t)` calls `restyle(i, <same variant>(Arc::new(mark.recolored(t))), it.w)`; no `set_last_*` call.

- [ ] **Step 1: Write the failing test**: extend `format_controls_behave` (after the Check part):

```rust
// A signature and initials get the inks too; a recolour is one undo step,
// keeps the shape, and the same ink again adds nothing.
let ink = Arc::new(Mark { w: 2, h: 1, rgba: vec![0x1F, 0x3A, 0x8A, 255, 0x1F, 0x3A, 0x8A, 40] });
let mut sig = all_tints.to_vec();
sig.extend([Hit::Smaller, Hit::Bigger, Hit::AddDate, Hit::Remove]);
assert_eq!(float_hits(&ItemKind::Signature(ink.clone())), sig);
let mut ini = all_tints.to_vec();
ini.extend([Hit::Smaller, Hit::Bigger, Hit::AllPages, Hit::Remove]);
assert_eq!(float_hits(&ItemKind::Initials(ink.clone())), ini);
let s = add(ItemKind::Signature(ink.clone()), 60.0, 40.0);
assert_eq!(with(hwnd, |w| selected_tint(w)).flatten(), Some(Tint::Blue));
let steps = undo_steps(hwnd);
with(hwnd, |w| tint_selected(w, Tint::Red));
let ItemKind::Signature(red) = item(hwnd, s).kind else { panic!("still a signature") };
assert_eq!(red.tint(), Some(Tint::Red));
assert!(red.same_shape(&ink));
assert_eq!(item(hwnd, s).w, 40.0);
assert_eq!(undo_steps(hwnd), steps + 1);
with(hwnd, |w| tint_selected(w, Tint::Red));
assert_eq!(undo_steps(hwnd), steps + 1, "same ink: no step");
with(hwnd, |w| w.doc.as_mut().map(|d| d.session.undo()));
assert_eq!(item(hwnd, s).kind, ItemKind::Signature(ink.clone()));
with(hwnd, |w| w.doc.as_mut().map(|d| d.session.redo()));
assert_eq!(with(hwnd, |w| selected_tint(w)).flatten(), Some(Tint::Red));
```

Adjust the exact `undo_steps` / undo-redo calls to how the existing Check part of this test does them (`undo_steps` already undoes and redoes everything; use the same pattern the test uses for the text's "one undo" check). Add `Mark` / `Arc` imports to the test if missing.

- [ ] **Step 2: Run** `cargo test --locked opens_hidden_and_closes` → FAIL on the `float_hits(Signature)` assert.
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Run** `cargo test --locked sign::window` → PASS. Then `cargo build --release --locked`.
- [ ] **Step 5: Commit** `feat(sign): ink swatches on a placed signature or initials`.

### After the tasks

Whole-branch review (opus), then `git merge main` into this branch (siblings `ttd-font-ketik`, `ttd-start-over` may have landed), resolve conflicts, `cargo test --locked`, then the AGENTS.md finish flow.
