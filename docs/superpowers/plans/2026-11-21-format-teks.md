# Text formatting in the sign window: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Text and Date items get font family, bold, italic, underline and colour; Check items get colour; PDF keeps real text in the base-14 fonts; screen, image and PDF agree.

**Architecture:** A small style model (`sign::style`) rides on `ItemKind`. One DirectWrite helper (`sign::text_draw`) measures and draws styled text for the screen, the image writer and the non-WinAnsi picture fallback; the PDF writer maps a style to one of the 12 base-14 fonts and uses the same helper to measure the underline. The window gets format buttons in the floating bar, a font menu, Ctrl+B/I/U, and a pending style while a text is being typed.

**Tech Stack:** Rust 2024, `windows` 0.62 (Direct2D, DirectWrite, GDI EDIT), lopdf 0.47. No new crates.

**Spec:** `docs/superpowers/specs/2026-11-21-format-teks-design.md` (mockup `docs/design/tanda-tangan-format.html`).

## Global Constraints

- `vale.exe` at most 6,373,068 bytes (`cargo build --release --locked`); today 6,028,800.
- Colours: Black `0x1C1C1C`, Blue `0x1F3A8A`, Red `0xA61B1B`.
- Faces: Sans "Arial" / Serif "Times New Roman" / Mono "Courier New" on screen and in images.
- PDF base fonts (Type1, `/Encoding /WinAnsiEncoding`, never embedded): Sans Helvetica, Helvetica-Bold, Helvetica-Oblique, Helvetica-BoldOblique; Serif Times-Roman, Times-Bold, Times-Italic, Times-BoldItalic; Mono Courier, Courier-Bold, Courier-Oblique, Courier-BoldOblique.
- Underline (all families, in em of the font size, y up from the baseline): centre at -0.1, thickness 0.05, from the text's start to its advance width, in the item's colour.
- New Text/Date default style: Sans, plain, Black. New Check default: Black. Last-used style kept in memory only.
- Text placement unchanged: font size `TEXT_FILL` (0.8) of the box height, baseline 0.2 of the height above the box bottom, left aligned.
- Never touch `src/shot_edit/`. Tests live in `#[cfg(test)]` modules. `cargo test --locked` green before every commit (run as `rtk bypass cargo test --locked` and check `$LASTEXITCODE`; rtk hides exit codes).

## Review Focus

1. Ctrl+I in a Win32 EDIT arrives as WM_CHAR 0x09 (Tab) and Ctrl+B / Ctrl+U as 0x02 / 0x15: they must toggle the style without inserting a control character or beeping (Task 5 test `format_keys_post_and_swallow`).
2. A Date or a dragged-wider Text box is wider than its text: the PDF underline must end at the text's end, not the box's (Task 3 test `underline_follows_text_not_box`).
3. Text with characters outside WinAnsi (e.g. "Zoë – 名前") saved to PDF: the picture fallback must keep face, bold, italic, underline and colour (Task 2 test `text_mark_uses_style_colour`, Task 5 test `encodable_keeps_style`).
4. One save with many styles on many pages: each base font object is written once per save and shared, resource names never collide with the page's own fonts (Task 3 test `fonts_shared_across_pages`).
5. Restyle near the right page edge: the wider box is pulled back inside the page (Task 4 test `restyle_keeps_box_on_page`).

---

### Task 1: Style model on items

**Files:**
- Create: `src/sign/style.rs`
- Modify: `src/sign/mod.rs` (`pub mod style;`, `ItemKind`), `src/sign/session.rs` (`copy_item`, new `restyle`), every `ItemKind::Text/Date/Check` site in `src/sign/**` (image_out.rs, pdf_write.rs, geom.rs, window/layout.rs, window/paint.rs, window/window.rs, tests).

**Interfaces:**
- Produces (`crate::sign::style`):
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)] pub enum Family { #[default] Sans, Serif, Mono }` with `pub const ALL: [Family; 3]` and `pub fn face(self) -> &'static str` ("Arial", "Times New Roman", "Courier New").
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)] pub enum Tint { #[default] Black, Blue, Red }` with `pub const ALL: [Tint; 3]` and `pub fn rgb(self) -> u32`.
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)] pub struct TextStyle { pub family: Family, pub bold: bool, pub italic: bool, pub underline: bool, pub tint: Tint }` with `pub fn pdf_base_font(&self) -> &'static str` and `pub fn changes_width(&self, other: &TextStyle) -> bool` (family, bold or italic differ).
  - `pub const UNDERLINE_POS: f32 = -0.1; pub const UNDERLINE_THICK: f32 = 0.05;`
  - `pub fn last_text() -> TextStyle`, `pub fn set_last_text(s: TextStyle)`, `pub fn last_check() -> Tint`, `pub fn set_last_check(t: Tint)`: process-wide, `std::sync::Mutex` statics, poison-tolerant.
  - `ItemKind::Text(String, TextStyle)`, `ItemKind::Date(String, TextStyle)`, `ItemKind::Check(Tint)`.
  - `Session::restyle(&mut self, i: usize, kind: ItemKind, w: f32)`: one checkpoint, sets `kind` and `w` clamped like `resize_item` (8 pt to the page's shorter side); out of range does nothing.

- [ ] **Step 1: Write failing tests** in `style.rs`: `base_fonts_cover_all_twelve` (every Family × bold × italic maps to the exact name in Global Constraints, 12 distinct), `tints_are_the_spec_colours` (0x1C1C1C, 0x1F3A8A, 0xA61B1B), `defaults_are_sans_plain_black`, `last_used_round_trips` (set_last_text then last_text equal; set_last_check likewise; restore defaults at the end), `width_changes_only_for_face_weight_slant` (underline/tint flips → false; family/bold/italic → true). In `session.rs`: `restyle_is_one_undo_step` (restyle Text to bold Serif with w 120 → kind and w changed; `undo()` restores old kind and w; one undo only), `restyle_clamps_width` (w 1e6 on a 612×792 page → 612).
- [ ] **Step 2:** `cargo test --locked sign::` fails to compile (types missing).
- [ ] **Step 3:** Implement `style.rs`, change `ItemKind`, add `Session::restyle`, fix every match site. Creation sites in window.rs (Date tool, `add_date_under`, Check tool, new Text in `finish_text`) use `style::last_text()` / `style::last_check()`. Renderers keep today's look in this task (they ignore the style); Tasks 2 and 3 use it.
- [ ] **Step 4:** `rtk bypass cargo test --locked` → all pass, `$LASTEXITCODE` 0.
- [ ] **Step 5:** Commit `feat(sign): text style model on items`.

### Task 2: Styled drawing on screen and in images

Runs in parallel with Task 3 (different files).

**Files:**
- Create: `src/sign/text_draw.rs` (`pub mod text_draw;` in `src/sign/mod.rs`)
- Modify: `src/sign/window/paint.rs` (drop `arial`, `arial_width`, `ink_text` in favour of `text_draw`; `text_mark` takes a style; items and checks drawn in their tint), `src/sign/image_out.rs` (`draw_text` / `draw_check` use `text_draw` and the tint; `TEXT_FILL` comes from `text_draw`), `src/sign/window/window.rs` only where it calls `paint::arial_width` / `paint::text_mark` / `paint::TEXT_FILL` (signature change; `encodable` passes the item's style).

**Interfaces:**
- Consumes: Task 1 `style::*`.
- Produces (`crate::sign::text_draw`):
  - `pub const TEXT_FILL: f32 = 0.8;` (window's `TEXT_FILL` re-exports or uses it).
  - `pub fn format(dwrite: &IDWriteFactory, st: &TextStyle, size: f32) -> Option<IDWriteTextFormat>` (face, weight 700/400, style italic/normal, no wrap).
  - `pub fn width(dwrite: &IDWriteFactory, s: &str, st: &TextStyle, size: f32) -> f32` (advance incl. trailing spaces; fallback `chars * size * 0.55`).
  - `pub fn advance_em(s: &str, st: &TextStyle) -> f32`: advance at size 1 using a process-shared DirectWrite factory (`DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)`, no COM needed), for the PDF writer.
  - `pub fn draw(rt: &ID2D1RenderTarget, dwrite: &IDWriteFactory, brush: &ID2D1SolidColorBrush, s: &str, st: &TextStyle, rect: (f32, f32, f32, f32))`: sets the brush to `st.tint`, draws at the existing baseline rule, and when `st.underline` fills the underline rect (Global Constraints) across the layout's advance width.
  - `pub fn tint_color(t: Tint) -> D2D1_COLOR_F`.
  - `paint::text_mark(s: &str, st: &TextStyle, w_pt: f32, h_pt: f32) -> Option<Mark>`.

- [ ] **Step 1: Failing tests** in `text_draw.rs`: `bold_is_wider_than_regular` (width of "Budi Santoso" Sans bold > regular at 20), `mono_advance_is_fixed` (`advance_em("iiii", mono)` == `advance_em("MMMM", mono)` within 1e-3, ≈ 2.4 = 4 × 0.6), `families_differ` (Serif "Budi" advance ≠ Sans). In `paint.rs` tests: `text_mark_uses_style_colour` (red underlined text mark: some opaque pixels are ≈ 0xA61B1B; a row of opaque pixels exists below the baseline spanning ≥ 80% of the text width). In `image_out.rs` tests: extend the existing composed-image test so a Blue Check and a Red Text land in their colours.
- [ ] **Step 2:** Tests fail.
- [ ] **Step 3:** Implement `text_draw`, switch paint.rs, image_out.rs and the window call sites. `image_out::draw_text` adopts the same baseline rule as the screen (today it centres vertically; the spec wants screen and image identical).
- [ ] **Step 4:** `rtk bypass cargo test --locked` green.
- [ ] **Step 5:** Commit `feat(sign): draw text and checks in their style`.

### Task 3: PDF writer: base-14 fonts, colour, underline

Runs in parallel with Task 2 (different files). Uses `text_draw::advance_em`: if Task 2 is not merged yet, add a private `advance_em` stub with the same signature and switch to Task 2's when merging (the merge step resolves this).

**Files:**
- Modify: `src/sign/pdf_write.rs` only.

**Interfaces:**
- Consumes: Task 1 `TextStyle::pdf_base_font`, `Tint::rgb`, `UNDERLINE_POS`, `UNDERLINE_THICK`; Task 2 `text_draw::advance_em`.
- Produces: no new public API; `save` and `save_is_incremental` keep their signatures.

- [ ] **Step 1: Failing tests:** `each_style_gets_its_base_font` (one page, Text items in Sans bold, Serif italic, Mono bold-italic → page font resources reference fonts with BaseFont Helvetica-Bold, Times-Italic, Courier-BoldOblique, each `/Encoding /WinAnsiEncoding`, `Subtype /Type1`, no `FontFile*`), `fonts_shared_across_pages` (same Serif style on 3 pages + a page that already has a font resource named like ours → exactly one Times-Roman font object in the file; no resource name collision), `text_colour_follows_tint` (Red Text content stream contains `0.651 0.106 0.106 rg` with the existing number formatter; Blue Check contains `RG` with the blue; Black is `0.11 0.11 0.11`), `underline_follows_text_not_box` (underlined Sans "AB" in a box 5× wider than its text: content has one `re f` whose width in the unit box ≈ `advance_em("AB") * 0.8 * h / w`, not 1.0; y from the Global Constraints rule), `encrypted_keeps_styles` (an AES-256 source: incremental save still opens with the password and has the Times-Bold font).
- [ ] **Step 2:** Tests fail.
- [ ] **Step 3:** `Shared.font: Option<ObjectId>` becomes a per-base-font cache (`Vec<(&'static str, ObjectId)>`); per page, one resource name per base font used (`free_name`). Text fill colour from the tint; underline as a filled rect after `ET` in the unit box (baseline y 0.2, size 0.8, horizontal scale as today's `sx`), width from `advance_em`. Check stroke colour from its tint. Remove `INK` if unused.
- [ ] **Step 4:** `rtk bypass cargo test --locked` green (includes the existing writer, fuzz-style and encryption tests).
- [ ] **Step 5:** Commit `feat(sign): PDF text in base-14 fonts with colour and underline`.

### Task 4: Format controls on a selected item

After Tasks 2 and 3 are merged into the feature branch.

**Files:**
- Modify: `src/sign/window/layout.rs` (hits, widths, two-row bar, font menu), `src/sign/window/window.rs` (`float_hits`, `Pop::Fonts`, `act`, shortcuts on a selection, `restyle_selected`), `src/sign/window/paint.rs` (drawing the new bar parts and the menu).

**Interfaces:**
- Consumes: Tasks 1-2 (`style::*`, `Session::restyle`, `text_draw::width`, `text_draw::format`).
- Produces: `Hit::Family`, `Hit::Bold`, `Hit::Italic`, `Hit::Underline`, `Hit::Tint(Tint)`, `Hit::PickFamily(Family)`; `Pop::Fonts`; `fn restyle_selected(w: &mut Win, f: impl FnOnce(&mut TextStyle))` and `fn tint_selected(w: &mut Win, t: Tint)` in window.rs; `layout::float_bar` keeps its signature and wraps to two rows; `layout::font_menu(family_button: Rect, bar: Rect, item: Rect, stage: Rect, ui: f32) -> Popover`.

- [ ] **Step 1: Failing tests** (layout.rs / window.rs test modules): `text_bar_has_format_then_actions` (`float_hits` for Text and Date = Family, Bold, Italic, Underline, Tint×3 in `Tint::ALL` order, Smaller, Bigger, Remove; Check = Tint×3, Smaller, Bigger, Remove; Signature/Initials unchanged), `bar_wraps_when_narrow` (a 360 px stage: two rows, format hits on the upper row, every rect inside the stage; a 968 px stage: one row), `font_menu_opens_away_from_item` (bar above item → menu bottom ≤ bar top; bar below item → menu top ≥ bar bottom; inside the stage; rows PickFamily in `Family::ALL` order), `restyle_keeps_box_on_page` (Text near the right edge of the page made bold Serif: new w = measured + 2, h unchanged, box inside the page, one undo step restores all), `underline_and_tint_keep_size` (w, h unchanged), `last_style_follows_changes` (after Bold on a Text, `style::last_text().bold`; after Red on a Check, `last_check() == Red` and `last_text()` untouched).
- [ ] **Step 2:** Tests fail.
- [ ] **Step 3:** Implement. Labels widths: Family = its face name measured in that face at the bar's 12.5 px + chevron; B/I/U = bar height (square); Tint = 26 px × ui; 9 px × ui gap between groups. Draw B bold, I italic, U underlined; on-state uses the accent tint the tool bar's selected tool uses; swatches are filled circles with a ring when current; the family button text in its own face. Clicks and Ctrl+B / Ctrl+I / Ctrl+U on a selected Text/Date (not while editing; other kinds ignore) call `restyle_selected`; swatches call `tint_selected` for Check, `restyle_selected` for Text/Date. Re-measure only when `changes_width`; Date boxes re-measure the same way. Escape and outside clicks close `Pop::Fonts` like the other popovers.
- [ ] **Step 4:** `rtk bypass cargo test --locked` green; `cargo run --release -- --demo-sign` and check the bar, the menu and a save by eye.
- [ ] **Step 5:** Commit `feat(sign): format bar for text, date and check`.

### Task 5: Formatting while typing

After Task 4.

**Files:**
- Modify: `src/sign/window/text_edit.rs` (`Spec` gains `bold`, `italic`, `underline`, `format_keys`; `restyle`; new notify codes), `src/sign/window/window.rs` (`w.text` holds a pending style; `open_text`, `fit_text`, `finish_text`, the `WM_SIGN_EDIT` handler), `src/sign/window/maker.rs` (passes `format_keys: false` and plain style).

**Interfaces:**
- Consumes: Tasks 1-4.
- Produces: `text_edit::EDIT_BOLD = 4`, `EDIT_ITALIC = 5`, `EDIT_UNDERLINE = 6`; `pub fn restyle(f: &mut Field, face: PCWSTR, px: f32, bold: bool, italic: bool, underline: bool, color: u32)`.

- [ ] **Step 1: Failing tests:** `format_keys_post_and_swallow` (pure helper `fn format_key(vk: u16, ctrl: bool) -> Option<usize>` maps Ctrl+B/I/U to the three codes and nothing else; `fn swallow_char(ch: usize, format_keys: bool) -> bool` is true for 0x02, 0x09, 0x15 when format keys are on, and for 0x09, 0x0D, 0x1B always), `commit_applies_pending_style` (window-level helper used by `finish_text`: same text + new style → kind updated, one undo step; same text + same style → no undo step), `escape_drops_pending_style`, `new_text_takes_pending_style_and_sets_last`, `encodable_keeps_style` (a non-WinAnsi bold red Text becomes a Signature mark whose pixels are red).
- [ ] **Step 2:** Tests fail.
- [ ] **Step 3:** Implement. The EDIT's GDI font follows the pending style (face, FW_BOLD/FW_NORMAL, italic, underline) and its text colour the tint; `fit_text` measures with `text_draw::width` and the style. The Text hint note stays as is.
- [ ] **Step 4:** `rtk bypass cargo test --locked` green; in `--demo-sign` type a text, Ctrl+B / Ctrl+I / Ctrl+U, Enter, undo, Escape.
- [ ] **Step 5:** Commit `feat(sign): Ctrl+B/I/U while typing`.

### Finish (controller)

- Release build size into the Size log below; must be ≤ 6,373,068.
- Whole-branch review, merge into `main`, `cargo test --locked` on `main`, memory + README, push, clean up (AGENTS.md).

## Size log

| Point | vale.exe bytes |
|---|---|
| main bc3a7af | 6,028,800 |
