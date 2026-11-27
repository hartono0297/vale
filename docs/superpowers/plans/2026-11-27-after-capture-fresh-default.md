# After capture "On screen" for new installs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A first run of Vale starts with Settings → Clipboard → *After capture* = *On screen* (`AfterCapture::Inline`); existing configs are untouched.

**Architecture:** `Config::fresh()` in `src/config.rs` is the first-run config (used by `load_at` when `config.json` is missing or blank). Set the screenshot field there only.

**Tech Stack:** Rust 2024, serde, `cargo test --locked`.

**Spec:** `docs/superpowers/specs/2026-11-27-after-capture-fresh-default-design.md`

## Global Constraints

- `ScreenshotSettings::default()` and the `after_capture` deserializer stay `Editor`.
- A file without `after_capture`, and a corrupt file, still load `Editor`.
- No UI, i18n or migration change.

## Review Focus

- Old `config.json` written before the setting existed (no `after_capture` field): must stay `Editor`. Covered by the existing `after_capture_is_the_editor_unless_the_file_says_inline` test (do not change it).
- Corrupt `config.json`: `Config::default()`, so `Editor`. Covered by `a_corrupt_file_or_a_read_error_loads_the_plain_defaults`.
- Fresh config saved then reloaded keeps `Inline`. Covered by `fresh_survives_a_save` once `fresh()` changes (it compares to `Config::fresh()`).

---

### Task 1: Fresh config picks On screen

**Files:**
- Modify: `src/config.rs` (`Config::fresh`, around line 1155; its doc comment)
- Test: `src/config.rs` test module (`fresh_is_a_notch_with_the_new_layout`, around line 2777)

**Interfaces:**
- Consumes: `ScreenshotSettings`, `AfterCapture::Inline` (both in `src/config.rs`)
- Produces: `Config::fresh().screenshot.after_capture == AfterCapture::Inline`

- [ ] **Step 1: Write the failing test**

In `fresh_is_a_notch_with_the_new_layout`, add
`assert_eq!(c.screenshot.after_capture, AfterCapture::Inline);`
and in its `rest` struct add `screenshot: ScreenshotSettings::default(),` so "nothing else differs" still holds.
Also add to `no_file_or_a_blank_one_loads_the_fresh_layout`:
`assert_eq!(load_at(&path).screenshot.after_capture, AfterCapture::Inline, "first run draws on screen");` (after the "no file" assertion, before writing the file).

- [ ] **Step 2: Run to see it fail**

Run: `cargo test --locked config::tests::fresh`
Expected: FAIL in `fresh_is_a_notch_with_the_new_layout` (left `Editor`, right `Inline`).

- [ ] **Step 3: Implement**

In `Config::fresh()` add
`screenshot: ScreenshotSettings { after_capture: AfterCapture::Inline, ..ScreenshotSettings::default() },`
and extend the doc comment to say the first run also draws screenshots on the screen.

- [ ] **Step 4: Run the config tests**

Run: `cargo test --locked config::`
Expected: all pass, including `after_capture_is_the_editor_unless_the_file_says_inline`, `a_corrupt_file_or_a_read_error_loads_the_plain_defaults`, `fresh_survives_a_save`.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(shot): new installs draw on the screen after capture"
```
