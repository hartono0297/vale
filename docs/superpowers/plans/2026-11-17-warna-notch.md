# Warna notch (Polos, Gradasi, Jadi muka) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Settings choice "Warna notch" (Polos default, Gradasi, Jadi muka) that colours the island from the chosen mascot.

**Architecture:** One pure colour module (`src/ui/notch_colour.rs`) decides every colour (tint per coat, gradient bottom with contrast caps, ink palettes). The renderer's single fill site (`Renderer::paint_island`) paints a solid, a vertical gradient or the face colour from it; a palette override per closed view keeps text and marks readable. Jadi muka swaps the head for a new mark-only face painter plus relocated accessory pieces.

**Tech Stack:** Rust 2024, windows-rs Direct2D, `cargo test --locked`.

**Spec:** `docs/superpowers/specs/2026-11-17-warna-notch-design.md` (read it first). Code maps from the read-only recon, in the session notes folder `C:\Users\Hartono\AppData\Local\Claude-Data\claude_pzs8sxrjxfjjc\side-session-notes\local_deb49b3e-dac1-48ad-a723-b87d80ce176a\`: `warna-notch-recon-1-fill.md` (fill, gradients, dirty rects), `-2-settings.md` (settings recipe), `-3-mascot.md` (head, expressions, accessories, peek, plays), `-4-ink.md` (every colour on the compact notch). Mockups: `docs/design/warna-notch-maskot.html`, `docs/design/notch-muka.html`.

## Global Constraints

- `NotchStyle { Plain, Gradient, Face }`, config key `notch_style`, default `Plain`; a missing key or typo reads as Plain.
- **Plain draws exactly what main draws today.** No existing pixel, palette or signature test may change its expected values.
- Text >= 4.5:1 against the worst fill under it; non-text marks >= 3:1. Cream text is `0xF4EDE4`, dark ink is `0x17120F`.
- Gradient stops: 0.0 and 0.28 = resolved Warna dasar surface, 1.0 = bottom colour. Bottom = surface mixed toward the tint by the largest `t` (step 0.01) up to **0.45** (closed views) / **0.32** (panel), such that cream >= **5:1**, caption `mix(cream, bottom, 0.15)` >= **4.5:1**, and for notices and the panel also accent >= **3:1**.
- Tint table (spec "The tint"): cat Terracotta `E2683C`, Calico `E2683C`, White `F6F1EA`, Black `8E7CB8`; falk Lutino `F6D04A`, Grey `9A959C`, WhiteFace `9A959C`; octopus = `octopus_rgb`. Robot off: the accent.
- Face fill = `peek_face(look, surface).fill`. Face marks at grid unit ~1.28 dip (1.6x the pill's 0.8).
- Face only on closed compact views (Idle, MediaCompact, MediaLyric, PomodoroCompact, record / caption compact, ScreenCountdown). Notices, greeting, panel use Gradient.
- Face with the mascot off is drawn as Gradient (accent tint); the stored value is kept.
- No new timer, thread, window message or continuous clock.
- Nothing drawn outside the island shape.
- Labels: row "Warna notch" / "Notch colour"; segments "Polos | Gradasi | Jadi muka" / "Plain | Gradient | Face"; notes "Nyalakan maskot dulu" / "Turn the mascot on first", "Jadi muka selalu solid" / "Face is always solid"; Polos description "Gelap seperti bezel" / "Dark like the bezel"; others "Gradasi · dari <coat>" / "Gradient · from <coat>".
- Never use the Fable model. Worktrees under `.claude/worktrees/`. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. An old `config.json` (no `notch_style`) after update: the notch must look exactly as before. Pinned in Task 1 (parse) and Task 2 (Plain frame signature equals today's).
2. Gradasi with "Isi notch: Tembus": stops carry `fill_alpha`, the see-through halo/shadow still applies. Test in Task 2.
3. Face stored, then the mascot switched off and on again: drawn Gradient, back to Face, config value untouched. Tests in Task 1 (`effective_style`) and Task 4 (settings note and faded segment).
4. Smallest and largest notch (160x28, 300x44) in Jadi muka with every accessory and full look offsets: everything inside the shape and left of the clock. Clamp test in Task 3.
5. Dark faces (Black cat `1E1B22`) keep cream ink; red octopus `D9505F` keeps dark ink with a caption colour that still holds 4.5:1. Tests in Task 1.

---

## Execution order and parallelism

1. **Task 1** alone on the integration branch `feat/notch-colour` (worktree `.claude/worktrees/notch-colour`).
2. **Tasks 2, 3, 4 in parallel**, each in its own worktree branched from `feat/notch-colour` after Task 1 (`.claude/worktrees/notch-gradient`, `notch-face-paint`, `notch-settings`). They touch disjoint files except `src/app.rs` (Task 2: frame build; Task 4: `on_settings_action`), small hunks. Merge back one at a time: merge the latest integration branch into the task branch, `cargo test --locked`, then merge.
3. **Task 5** after 2, 3 and 4 are merged.
4. Whole-branch review, then merge `feat/notch-colour` into `main`.

---

### Task 1: Config field and the pure colour module

**Files:**
- Modify: `src/config.rs` (enum next to `NotchFill` ~:552, field after `notch_fill` ~:59, `Default` ~:806, `merge_owned` ~:966, tests ~:3030-3170 and round trip ~:1331)
- Modify: `src/config_transfer.rs` (`EXPORTED`, after `"notch_fill"` ~:45)
- Create: `src/ui/notch_colour.rs`; Modify: `src/ui/mod.rs` (`pub mod notch_colour;`)

**Interfaces:**
- Produces (later tasks use exactly these):
  - `crate::config::NotchStyle { Plain, Gradient, Face }` (`named_enum!`, `ALL`, `from_name`).
  - `Config.notch_style: NotchStyle`.
  - In `crate::ui::notch_colour`:
    - `pub const TOP_STOP: f32 = 0.28; pub const CLOSED_T: f32 = 0.45; pub const PANEL_T: f32 = 0.32;`
    - `pub const CREAM: u32 = 0xF4EDE4; pub const INK: u32 = 0x17120F;`
    - `pub fn contrast(a: u32, b: u32) -> f32` (WCAG 2.x).
    - `pub fn effective_style(style: NotchStyle, robot_on: bool) -> NotchStyle` (Face + robot off = Gradient).
    - `pub fn tint(look: CatLook, robot_on: bool, accent: u32) -> u32` (the table; accent when robot off).
    - `pub fn bottom(surface: u32, tint: u32, max_t: f32, accent: Option<u32>) -> u32` (largest `t` rule; `t = 0` gives `surface`).
    - `pub fn face_fill(look: CatLook, surface: u32) -> u32` (= `peek_face(look, surface).fill`).
    - `pub fn face_palette(base: Palette, fill: u32) -> Palette`: `surface = fill`; `text` and `on_accent` = INK if `contrast(INK, fill) >= 4.5` else CREAM; `secondary` = `mix(text, fill, s)` with the largest `s <= 0.30` holding 4.5:1; `accent`, `sage`, `ochre`, `plum` replaced by `text` where under 3:1 against `fill`.
    - `pub fn gradient_palette(base: Palette, bottom: u32) -> Palette`: `accent` replaced by `text` where under 3:1 against `bottom`; everything else unchanged.
    - `pub fn needs_ring(mark: u32, fill: u32) -> bool` (< 3:1, for the REC dot).
    - `pub fn needs_halo(head: u32, fill: u32, has_rim: bool) -> bool` (< 3:1 and no rim).

- [ ] **Step 1: Write the failing tests** in `notch_colour.rs` and `config.rs`:
  - `config`: `a_missing_notch_style_reads_as_plain` (`{}` -> Plain), `a_typo_notch_style_reads_as_plain` (`"Glow"` -> Plain), `notch_style_round_trips` (Face saved and read back), `merge_owned_keeps_notch_style` (copy the notch-fill test). `config_transfer`'s `every_config_key_is_classified` must pass.
  - `the_tint_table_matches_the_spec`: every coat of the table above, and `tint(.., robot_on: false, 0xD9A441) == 0xD9A441`.
  - `every_gradient_bottom_keeps_cream_and_caption_readable`: for all 11 coats x the six concrete surfaces x `CLOSED_T`/`PANEL_T`: `contrast(CREAM, b) >= 5.0` and `contrast(mix(CREAM, b, 0.15), b) >= 4.5`.
  - `notice_and_panel_bottoms_keep_every_accent_at_3`: all coats x surfaces x the six accents with `Some(accent)`: `contrast(accent, b) >= 3.0`.
  - `pale_coats_blend_less`: White cat bottom on Espresso at `CLOSED_T` is strictly closer to the surface than Terracotta's (compare `t` or luminance).
  - `face_ink_is_readable_on_every_face`: every coat's `face_fill`: `contrast(p.text, fill) >= 4.5`, `contrast(p.secondary, fill) >= 4.5`, and every one of `accent/sage/ochre/plum` >= 3.0 or equal to `text`. Explicit: Black cat face keeps `text == CREAM`; red octopus (`D9505F`) gets `text == INK`.
  - `face_with_the_mascot_off_draws_gradient`: `effective_style(Face, false) == Gradient`, `effective_style(Face, true) == Face`, Plain and Gradient unchanged either way.
- [ ] **Step 2:** `cargo test --locked notch_colour` and `cargo test --locked notch_style` fail (missing items).
- [ ] **Step 3: Implement** the enum, config wiring, transfer entry and the module (reuse `crate::ui::mascot::mix`; `contrast` is the WCAG formula, round only for display).
- [ ] **Step 4:** `cargo test --locked` all green.
- [ ] **Step 5: Commit** `feat(notch-colour): NotchStyle setting and the colour rules`.

### Task 2: Gradasi in the renderer

**Files:**
- Modify: `src/ui/render.rs` (`Frame` fields ~:231-278, `paint_island` ~:2449-2520, palette override around the compact/notice content ~:2536-2762, mascot halo in `paint_robot_at` ~:3227, `signature` ~:1424, test helper `with_frame_all` ~:5589)
- Modify: `src/ui/render_panel.rs` (REC dot ~:5444-5465 ring; nothing else)
- Modify: `src/ui/dirty.rs` (`Drawn`, `plan`, `EqKey` + their tests)
- Modify: `src/app.rs` (frame build ~:7738-7790, `EqKey` build ~:7896)
- Modify: `src/ui/demo.rs` (`DI_NOTCH_STYLE` read next to `DI_SURFACE` ~:2964 and the Frame constructor ~:3483; header comments)
- Reference template: vertical gradient brush in `src/ui/card_surface.rs:324`.

**Interfaces:**
- Consumes: Task 1's `NotchStyle`, `notch_colour::{effective_style, tint, bottom, gradient_palette, needs_ring, needs_halo, TOP_STOP, CLOSED_T, PANEL_T}`.
- Produces: `Frame.notch_style: NotchStyle` (already effective), `Frame.notch_tint: u32`; `Renderer::island_fill(&self, frame: &Frame) -> IslandFill` with `pub enum IslandFill { Solid { rgb: u32, alpha: f32 }, Gradient { top: u32, bottom: u32, alpha: f32 } }` (Task 5 adds the Face case's solid fill through the same enum). `Drawn` and `EqKey` carry `notch_style` and `notch_tint`.

- [ ] **Step 1: Write the failing tests:**
  - `dirty.rs`: changing `notch_style` or `notch_tint` alone makes `plan` return `Full`; same for `EqKey`.
  - `render.rs`: `a_plain_frame_fills_exactly_as_before` (Plain: `island_fill` is `Solid { surface, fill_alpha }`, and today's pixel tests stay untouched); `gradient_fill_uses_the_surface_on_top_and_the_capped_bottom` (Gradient Terracotta on Espresso: `top == 0x17120F`, `bottom == bottom(0x17120F, 0xE2683C, CLOSED_T, None)`; panel view uses `PANEL_T` with `Some(accent)`; a notice uses `CLOSED_T` with `Some(accent)`); `gradient_stops_carry_the_see_through_alpha` (Transparent 30 %: `alpha == fill_alpha`); a pixel test with `with_frame_all` on Gradient: the island's top row is the surface and its bottom row is the bottom colour (within 2 per channel).
  - `gradient_marks_switch_to_text_under_3_to_1`: the compact palette under Gradient for an accent equal to the tint has `accent == text`.
- [ ] **Step 2:** run them, they fail.
- [ ] **Step 3: Implement:**
  - `Frame.notch_style`/`notch_tint` set in `app.rs` from `effective_style(config.notch_style, config.robot)` and `tint(CatLook::of(cfg), cfg.robot, palette.accent)`; demo reads `DI_NOTCH_STYLE` with `NotchStyle::from_name`.
  - `paint_island`: Plain keeps the current code path untouched; Gradient creates a vertical linear gradient brush over the shape's y-range (stops at 0, `TOP_STOP`, 1) with `alpha` and fills the same geometry with it. The bottom while morphing between a closed view and the panel is `mix(closed_bottom, panel_bottom, eased morph t)` (use the fill morph's own progress). When `peek_mix > 0`, blend both stops toward the face colour by `peek_mix` (the pill ends solid face as today).
  - Closed compact views under Gradient draw with `gradient_palette(base, closed_bottom)`, restored after the content (the recon's seam: after the Panel early return).
  - Halo: draw the existing dark halo behind the head when `needs_halo(look.head_rgb(), closed_bottom, look.rim().is_some())`, independent of `legible`.
  - REC dot: 1.5 dip ring in `p.text` when `needs_ring(REC_RED, fill_under_it)`.
- [ ] **Step 4:** `cargo test --locked` green; run `cargo run --release -- --demo` with `DI_NOTCH_STYLE=gradient` for scenes idle, media, notice, panel and confirm it draws without errors (no screenshot judgement needed here).
- [ ] **Step 5: Commit** `feat(notch-colour): Gradasi fill, caps and readable marks`.

### Task 3: The face painter and its geometry

**Files:**
- Modify: `src/ui/robot.rs` (new method next to `paint_eyes` ~:1330; reuse the Part loop of `paint` ~:1203-1242, `enter_grid`/`fill_marks` ~:1396-1408, `eyes_offset_in` ~:718)
- Modify: `src/ui/cat_style.rs` (new face pieces next to `pieces` ~:324; leave `pieces` and its tests unchanged)
- Modify: `src/ui/geometry.rs` (face area + clamp tests next to `the_looking_cat_stays_in_the_idle_island` ~:2136)

**Interfaces:**
- Consumes: `CatLook`, `Expr`, `Pose`, `PeekFace` (`peek_face`), `ShapeKind`, `NotchSize`.
- Produces:
  - `pub const FACE_SCALE: f32 = 1.6;` (relative to the pill; grid unit = `0.8 * FACE_SCALE` dip).
  - `geometry::face_area(shape: Rect, clock_left: Option<f32>) -> Rect`: the box the face is centred in, left of the clock with the clock's gap, inside the shape minus shoulders.
  - `RobotPainter::paint_face(&self, rt: &ID2D1RenderTarget, centre: (f32, f32), scale: f32, expr: Expr, pose: &Pose, look: CatLook, alpha: f32) -> Result<()>`: eyes in every expression (as `face(expr, blink)` gives them), cheeks, beak, the z's and the wave heart placed beside the eyes (not above), coat marks (calico patches as large circles clipped to the shape, octopus spots), the look as an eye slide only (clamped like `eyes_offset_in`), the dance as a nod of the whole face. No head, ears, body, laptop or paws; `Expr::Typing` draws the focus eyes looking down.
  - `cat_style::face_pieces(accessory: CatAccessory, shape: Rect, eyes: (f32, f32), unit: f32) -> Vec<FacePiece>`: Glasses on the eyes; Headphone as two earcups on the shape's left/right sides; Bowtie under the eyes; Scarf as a band on the bottom edge with its tail; Bow and Flower in the left corner; Nightcap, Crown, Sailor whole, small, rotated -16 degrees, left of the eyes (mockup row "miring di pojok"); a piece whose main colour is under 3:1 against the face gets a 1 dip edge in the face's eye colour; the crown always has a `#9A6A12` edge. `RobotPainter::paint_face_pieces(..)` paints them.
- [ ] **Step 1: Write the failing tests** (geometry.rs / cat_style.rs):
  - `the_face_stays_inside_every_compact_notch`: for widths 160..=300 step 10, heights 28..=44 step 2, every compact view shape, every expression, look `v` in -1..=1 step 0.25, every mascot: all face mark boxes inside `shape_contains` and left of the clock box.
  - `every_face_accessory_stays_inside_the_notch`: same loop over the ten accessories' `face_pieces` boxes (rotated hats included).
  - `face_pieces_hold_3_to_1_or_have_an_edge`: every accessory x every coat's face fill: each piece's main colour >= 3:1 against the fill or the piece has an edge.
  - `typing_on_a_face_is_the_focus_eyes`: the marks for `Expr::Typing` equal those for `Expr::Focus` with the downward offset, and contain no laptop or paw part.
- [ ] **Step 2:** run, fail.
- [ ] **Step 3: Implement** the three items above. No renderer wiring here (Task 5).
- [ ] **Step 4:** `cargo test --locked` green.
- [ ] **Step 5: Commit** `feat(notch-colour): mark-only face painter and face accessories`.

### Task 4: The Settings row and previews

**Files:**
- Modify: `src/i18n.rs` (keys next to `NotchFill*` ~:504)
- Modify: `src/ui/settings_layout.rs` (`Hit::NotchStyle`, `Group::NotchStyle`, `State.notch_style`, Island card 6 -> 7 rows with Warna notch at index 3 ~:2524-2667, faded segment, faded Isi notch/Opasitas under Face, tests ~:6428, ~:7386-7439, ~:9827-9866, ~:12414-12574)
- Modify: `src/ui/settings.rs` (`Action::NotchStyle`, `Model.notch_style`, `from_config`, `state()`, slide group ~:2033-2058, click ~:2947, repeat arm ~:3121-3143, demo apply ~:4902, demo model `DI_NOTCH_STYLE` ~:5180)
- Modify: `src/ui/settings_controls.rs` (`Scene` gets `notch_style: NotchStyle` and `notch_tint: u32`; `preview` ~:1731 and `cat_preview` ~:1486 fill Gradient with a gradient and Face with the face colour)
- Modify: `src/app.rs` (`on_settings_action` ~:7052: `self.config.notch_style = v; self.save_config();`)

**Interfaces:**
- Consumes: Task 1 (`NotchStyle`, `notch_colour::{effective_style, tint, bottom, face_fill}`).
- Produces: `Scene.notch_style`, `Scene.notch_tint` (Task 5 switches the previews' head to `paint_face` under Face).

- [ ] **Step 1: Write the failing tests:**
  - `the_island_card_has_warna_notch_after_warna_dasar` (7 rows; index 2 swatches, 3 Warna notch, 4 Font, 5 Jam, 6 EQ; update the existing row-index asserts to these).
  - `the_face_segment_is_faded_without_the_mascot`: with `robot: false` the Face segment takes no click (`hit_test` gives no `Hit::NotchStyle(Face)`) and the row description is the "Nyalakan maskot dulu" key; Plain and Gradient stay clickable.
  - `the_fill_rows_fade_under_face`: with Face (and robot on), Isi notch and Opasitas take no clicks and Isi notch's description is "Jadi muka selalu solid".
  - `notch_style_labels_differ_by_language` (copy `labels_of_the_notch_card_differ_by_language`).
  - `appearance_scrolls_and_clicks_follow`, `everything_fits_in_the_window_height`, `exactly_the_overflowing_sections_get_a_viewport` stay green (adjust only the row expectations that moved).
  - `clicking_a_style_posts_the_action` (settings.rs): a click on Gradient posts `Action::NotchStyle(Gradient)`.
- [ ] **Step 2:** run, fail.
- [ ] **Step 3: Implement.** A disabled single segment: extend the segmented control with a per-item enabled slice (faded like `segmented_if`, no hit for that item); keep `segmented`/`segmented_if` callers unchanged.
- [ ] **Step 4:** `cargo test --locked` green; `cargo run --release -- --settings appearance` with `DI_NOTCH_STYLE=face` opens without errors.
- [ ] **Step 5: Commit** `feat(notch-colour): Warna notch row in Tampilan`.

### Task 5: Jadi muka in the renderer and the app

**Files:**
- Modify: `src/ui/render.rs` (`island_fill` Face case, compact painters Idle ~:2619, Media ~:3280, Lyric ~:3346, the Eyes pill ~:2528-2534, `paint_robot` callers for closed views, palette override `face_palette`)
- Modify: `src/ui/render_panel.rs` (Pomodoro/Record ~:5414, ScreenCountdown ~:5611)
- Modify: `src/app.rs` (`peek_mix` retarget ~:4359 constant 1 under Face; the fill morph between Face and Gradient views; look/dance into the face pose)
- Modify: `src/ui/play/gate.rs` (`permanent_ok` ~:40: no plays while the effective style is Face)
- Modify: `src/ui/settings_controls.rs` (previews draw `paint_face` under Face)
- Modify: `src/ui/dirty.rs` only if a new frame field is needed.

**Interfaces:**
- Consumes: Task 2 (`IslandFill`, `Frame.notch_style/notch_tint`), Task 3 (`paint_face`, `face_pieces`, `face_area`, `FACE_SCALE`), Task 4 (`Scene` fields), Task 1 (`face_fill`, `face_palette`).
- Produces: the finished feature.

- [ ] **Step 1: Write the failing tests:**
  - `a_face_view_fills_with_the_face_colour` (Face + Idle: `island_fill` is `Solid { face_fill, 1.0 }` whatever `fill_alpha`); `face_is_only_for_closed_compact_views` (a notice and the panel under Face give the Gradient fill).
  - `the_face_palette_is_used_on_closed_views_only` (text colour on Idle under Face with a Lutino falk is INK; on a notice it is CREAM).
  - `peek_mix_stays_at_one_under_face` (app/peek test: retarget under Face keeps 1 before and after the shrink).
  - `plays_are_off_under_face` (gate test, with the mascot on).
  - A pixel test: Face + Terracotta Idle has the face colour at the notch's centre-right (not the head) and no head pixels in the old head slot.
- [ ] **Step 2:** run, fail.
- [ ] **Step 3: Implement:** closed compact views under Face call `paint_face` at `face_area(..)` centre with `FACE_SCALE` and `paint_face_pieces`; the Eyes pill reuses the same marks at scale 1 with accessories hidden; the morph Face <-> Gradient view cross-fades the fill (stops blend from the face colour) while marks fade out and the head/content fades in, the palette switches ink when cream reaches 4.5:1 on the blended fill; Typing shows focus eyes.
- [ ] **Step 4:** `cargo test --locked` green; `cargo run --release -- --demo` with `DI_NOTCH_STYLE=face` and `DI_MASCOT` / coat variables for cat, falk, octopus: idle, media, lyric, pomodoro, record, peek-eyes scenes draw without errors.
- [ ] **Step 5: Commit** `feat(notch-colour): Jadi muka face notch`.
