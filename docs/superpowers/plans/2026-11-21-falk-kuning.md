# Falk Kuning and site eyes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the falk's second coat into an all-yellow bird called "Kuning" / "Yellow" in the app and on the site, and put the site hero's falk and octopus eyes where the app has them.

**Architecture:** App: one ink change in `falk_ink` (body = face yellow), the notch tint, the label, and the Settings swatch; wing tones follow from `falk_coat`'s existing `tone()`. Site: the `MASCOTS` data and labels, plus per-mascot eye boxes and a clamped glance in the hero.

**Tech Stack:** Rust 2024 (`cargo test --locked`), static site (HTML/CSS/ES5 JS, `scripts/check-site.py`).

**Spec:** `docs/superpowers/specs/2026-11-21-falk-kuning-design.md`

## Global Constraints

- Lutino and WhiteFace (Muka putih) do not change: their ink, wing tones, tint, surface, swatch and site colours stay byte-identical.
- The stored values stay: config `falk_color` value `Grey`, site storage key `vale-coat-falk` value `grey`.
- Kuning ink: body `0xF2D35A`, face `0xF2D35A`, crest `0xE9CF62`, cheek `0xEC6B3A`. Surface stays `Surface::Night`.
- Labels: ID "Kuning", EN "Yellow".
- No em dash in any new text; antislop skills for the site and Settings visuals (`antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-human`).
- Do not commit `AGENTS.md`; never `git stash`; stage only your own files by path.

## Review Focus

1. An existing config with `falk_color: "Grey"` loads as the Kuning coat (not reset to Lutino). Owner: Task 1.
2. Kuning vs Lutino are still tellable apart in the Settings coat swatches and in the notch. Owner: Task 1.
3. The falk and octopus eyes stay inside the face with the pointer at each viewport corner, during drag and in happy/squint. Owner: Task 2.
4. Reduced motion and no-script on the site: unchanged behaviour, eyes in place. Owner: Task 2.
5. Notch Gradient and Face under Kuning keep the contrast rules (`bottom()` 5:1 cream, 4.5:1 caption; face ink 4.5:1). Owners: Task 1 (app), Task 2 (site).

---

### Task 1: App, the Kuning coat

**Files:** `src/ui/mascot.rs` (`falk_ink`, tests), `src/ui/notch_colour.rs` (`tint`, tests), `src/i18n.rs` (`FalkColorGrey`, test), `src/theme.rs` (comment at `FalkColor`), `src/ui/play/art/falk.rs` (doc table and the Grey == WhiteFace wing test), the Settings swatch code (`src/ui/settings_layout.rs` / `settings_controls.rs`, wherever the falk coat swatch is painted), `src/ui/peek_face.rs` tests if they pin Grey colours.

- [ ] Step 1: failing tests: `falk_ink(FalkColor::Grey)` equals the Kuning ink; `falk_coat(Grey)` differs from `falk_coat(WhiteFace)` and its body is `0xF2D35A`; `falk_coat(WhiteFace)` and `falk_coat(Lutino)` unchanged (pin current values); notch tint of Grey is `0xF2D35A` and `bottom()` keeps 5:1 / 4.5:1; `Key::FalkColorGrey` is ["Kuning", "Yellow"]; a config with `"falk_color": "Grey"` parses to `FalkColor::Grey`.
- [ ] Step 2: `cargo test --locked` → those fail.
- [ ] Step 3: implement; Settings swatch: make Kuning tellable from Lutino (e.g. draw it on its Night surface or two-tone with the face and surface), reuse what the swatch code already supports; describe it in the report.
- [ ] Step 4: `cargo test --locked` → all green. Run `cargo run --release -- --demo` with the falk/Kuning if a demo variable allows (see `src/ui/demo.rs`) and take a screenshot only if the harness allows; otherwise say what the owner should look at.
- [ ] Step 5: commit `feat(mascot): falk coat Grey becomes Kuning, yellow all over`.

### Task 2: Site, Kuning and per-mascot eyes

**Files:** `site/site.js` (`MASCOTS.falk.coats.grey`, the hero eye/glance code around `#cat-eyes`), `site/index.html` (hero svg eye groups), `site/style.css` (eye visibility per mascot), `scripts/check-site.py` if it pins labels.

- [ ] Step 1: failing browser assertions (script in the session scratchpad, not committed): with each mascot picked and the pointer at the four viewport corners and at the picks row, the eye pills' bounding boxes lie inside the face: falk pills within x 84.8-171.2 ± glance, y never below 180 + 0 (above the cheek centre line 178 for the bottom of the pill at rest + clamp), octopus pills' bottom at least 8 above the mantle bottom (184); rest positions equal the spec values; cat eyes unchanged.
- [ ] Step 2: implement: per-mascot eye groups (or one group with per-mascot transform and size) for open, happy and squint; glance scaled by 22.4/26 for falk and octopus and clamped as the spec says; Kuning colours and label "Yellow" in `MASCOTS` (key `grey` kept).
- [ ] Step 3: run the assertions, `python -I scripts/check-site.py` (0 problems), and a quick contrast check of the Kuning notch Gradient/Face values with the site's `notchBottom`/`faceInk`.
- [ ] Step 4: commit `fix(site): each mascot's eyes where the app has them; falk Kuning`.
