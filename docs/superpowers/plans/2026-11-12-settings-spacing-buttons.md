# Settings spacing and button sizes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One spacing rule and one text-button size across every Settings page, so lists, section titles, notes and buttons line up the same way everywhere.

**Architecture:** All geometry lives in `src/ui/settings_layout.rs` (the pure `Builder`, tested with `Estimate`); the painter in `src/ui/settings_controls.rs` only draws it. Each rule becomes a constant or a `Builder` helper, and every card that broke the rule goes through it. The scroll fade is a new `Layout::top_fade()` rect the painter fills with a background-to-transparent gradient.

**Tech Stack:** Rust 2024, windows-rs Direct2D, unit tests inside `src/**`.

**Spec:** agreed in the session chat (2026-11-12). The owner left the numbers to the agent ("u atur yang terbaiknya gimana"); the rules below are the agreed proposal.

## Global Constraints

- Text buttons (`ButtonKind::Primary | Secondary | Confirm`): height `BUTTON_H` 30, font `Font::Control` 12, width `max(widest label + 2 * BUTTON_PAD (16), BUTTON_MIN_W (80))`. No other width formula: `Builder::compact_w` is deleted.
- Round icon buttons, steppers and segmented controls keep 28 (`STEP_BUTTON`, `CONTROL_H`). Link and External buttons unchanged.
- A text field (`Layout::fields`) is `BUTTON_H` (30) high: `FIELD_H = BUTTON_H`.
- Rows stay 52 (`ROW_H`), label at +7, description at +28, label-only rows centred; card inset 16; gap between two cards 12 (`CARD_GAP`).
- A list inside a card (Susun "Template saya" and "Susunan tersimpan", Notifikasi "Aplikasi dikecualikan"): every item row and the empty-state row is a 52 row with a divider at its top, like the Panel lists.
- A card's footnote (the Sapaan periods line, Live captions' model note): divider above, `NOTE_PAD` 12 above the first line and 12 below the last.
- A block under a row's text (accessory tiles, monitor map and radio list): starts at the row's bottom (+52), 16 of space under it (`BLOCK_PAD`).
- A section title (eyebrow) after other content: `GROUP_GAP` 24 from what is above it to the eyebrow, `EYEBROW_GAP` 8 to its card. The first eyebrow of a page stays at `FIRST_CARD_Y`.
- A scrolled section (`scroll > 0`) fades its top `SCROLL_FADE_H` 16 from the window background to transparent instead of a hard cut under the title.
- All values are DIPs (they scale per monitor DPI). No colour changes: every theme and surface (Terminal included) is untouched apart from geometry.
- `cargo test --locked` must pass; existing tests that pinned old numbers are updated to the new rule, never deleted, and every behaviour they check (hits, actions, which sections scroll) stays.

## Review Focus

- Indonesian labels are longer ("Ganti nama", "Hapus riwayat"): rows with two buttons plus a mini preview (saved arrangements, Panel tiles with Hapus + Sembunyikan) must not overlap their label at the 720 minimum width. Test in Task 1.
- Which sections scroll at 720x520 (`exactly_the_overflowing_sections_get_a_viewport`): Umum, Integrasi and Tentang must still not scroll after the 24 group gap. Test stays in Task 3.
- The Widget gallery count and the Google "Panduan" link sit on the eyebrow's line: they must follow the eyebrow's new y. Test in Task 3.
- An empty list (no templates, no saved arrangements, no muted apps) after the change: the empty text row gets its divider and the card height is unchanged in rows. Test in Task 2.
- The scroll fade must not exist when the section does not scroll or is at scroll 0, so nothing at the top of a page is dimmed when unscrolled. Test in Task 5.

---

### Task 1: One text-button size, fields as tall as buttons

**Files:**
- Modify: `src/ui/settings_layout.rs` (`FIELD_H`, `Builder::compact_w` and its 6 callers: `tiles_card`, `available_card`, `notifications_card`, `arrange_card` x2, `tr_cols`)
- Test: same file, `mod tests`

**Interfaces:**
- Produces: `Builder::button_w(&self, labels: &[&str], ButtonKind::Secondary) -> f32` is the only text-button width; `const FIELD_H: f32 = BUTTON_H;`

- [ ] **Step 1: Write the failing tests** `every_text_button_has_one_size` and `fields_are_as_tall_as_buttons`: for every `Section::ALL`, every state of `states()` plus a state with two arrange templates, two saved arrangements, two muted apps and two own tiles (`DI_TILES`-like `TileSettings` with custom entries), at the minimum size and at 1200x900: every `Control::Button` with kind Primary/Secondary/Confirm has `rect.h == BUTTON_H`, `rect.w >= BUTTON_MIN_W`, `rect.w >= Estimate.width(label, Font::Control) + 2.0 * BUTTON_PAD - 0.01`; two Secondary buttons with the same label in one layout have the same width; every `l.fields` rect has `h == BUTTON_H`. Also: no button overlaps a text rect of its row (`rect.x >= text.right()` for texts on the same row band).
- [ ] **Step 2:** `cargo test --locked every_text_button_has_one_size fields_are_as_tall_as_buttons` fails (Delete 56/66 vs 80, fields 32).
- [ ] **Step 3:** Delete `compact_w`; each caller uses `button_w(&[&label], ButtonKind::Secondary)` (`tr_cols`: max of `button_w` over its four labels). Set `FIELD_H = BUTTON_H`.
- [ ] **Step 4:** `cargo test --locked` passes; update any old test that pinned a compact width or a 32 field to the new rule.
- [ ] **Step 5:** Commit `fix(settings): one size for every text button and field`.

### Task 2: Lists inside a card are divided rows

**Files:**
- Modify: `src/ui/settings_layout.rs` (`arrange_card`, `notifications_card`)

- [ ] **Step 1: Failing test** `card_lists_have_a_divider_per_row`: Features layout, both languages, with (a) no templates/saved/muted and (b) two of each. For each item row and each empty-state row (found by its label text: template names, saved names, muted names, `ArrangeNoTemplates`, `ArrangeNoSaved`, `NotifMutedNone`), `l.dividers` has a divider whose y is within 1 of the row's top (text y - 16 for a label-only row). Card heights in rows are unchanged from today (`(1 + 1 + max(t,1) + 1 + max(s,1)) * ROW_H`, `(7 + max(m,1)) * ROW_H`).
- [ ] **Step 2:** It fails (no dividers under the list titles).
- [ ] **Step 3:** Add `b.divider(card, y)` at the top of every item row and the empty row in the templates list, the saved list and the muted list. The existing divider at the "Saved arrangements" title and above the add row stay.
- [ ] **Step 4:** `cargo test --locked` passes.
- [ ] **Step 5:** Commit `fix(settings): divide list rows in Susun and Notifikasi`.

### Task 3: Section titles sit with their card

**Files:**
- Modify: `src/ui/settings_layout.rs` (`GROUP_GAP`, `Builder::eyebrow`, `titled`, `widgets_page` gallery count)

**Interfaces:**
- Produces: `const GROUP_GAP: f32 = 24.0;` `Builder::eyebrow(&mut self, key: Key, w: f32) -> f32` returns the eyebrow's top y.

- [ ] **Step 1: Failing test** `section_titles_sit_with_their_card`: for every section, both languages, `states()[0]` with `scroll: 0`, at the minimum size: for each card-level eyebrow text (`Font::Eyebrow` whose rect x is `content_x + 2`, so not a `sub_header`), the next card's top is `eyebrow.bottom() + EYEBROW_GAP`, and when something is above it (a card, or any text, ending at `above`), `eyebrow.y - above >= GROUP_GAP - 0.01`; the first eyebrow of a page is at `FIRST_CARD_Y`. Widgets: the gallery count text has the same y as the "GALERI" eyebrow. Integrations: the Google guide link's centre y is the Google eyebrow's centre y.
- [ ] **Step 2:** It fails (gap 12).
- [ ] **Step 3:** In `eyebrow`, before placing it: when `self.y > FIRST_CARD_Y + 0.5`, add `GROUP_GAP - CARD_GAP` (the content above already left `CARD_GAP` or its own spacing; make every caller that sets `b.y` by hand, the pomodoro locked note, the Google "Lanjutan" link, the About header, end at its content's bottom + `CARD_GAP` so the rule gives 24). Return the eyebrow's y; the gallery count uses it.
- [ ] **Step 4:** `cargo test --locked` passes, including `exactly_the_overflowing_sections_get_a_viewport` and `everything_fits_in_the_window_height` unchanged in what they assert (Umum, Integrasi, Tentang still do not scroll; update only pixel numbers that moved).
- [ ] **Step 5:** Commit `fix(settings): 24 above a section title, 8 to its card`.

### Task 4: Footnotes and blocks under a row

**Files:**
- Modify: `src/ui/settings_layout.rs` (`NOTE_PAD`, `BLOCK_PAD`, Kucing `Sapaan` card, `captions_card`, `ACC_ROW_H`, `MONITOR_BLOCK_PAD`)

**Interfaces:**
- Produces: `const NOTE_PAD: f32 = 12.0;` `fn note_h(lines: usize) -> f32` (= `2.0 * NOTE_PAD + 16.0 + (lines - 1) as f32 * DESC_LINE_H`); `const BLOCK_PAD: f32 = 16.0;`

- [ ] **Step 1: Failing test** `footnotes_and_blocks_share_padding`: Kucing (both languages): the first periods line's top is the Sapaan divider + `NOTE_PAD`, the card's bottom is the last line's bottom + `NOTE_PAD`. Features: the same for the Live captions model note. Kucing: the card bottom is the accessory tiles' bottom + `BLOCK_PAD`. Tampilan: the Notch card's monitor block ends `BLOCK_PAD` below the last radio row's text bottom (radio text is centred in `RADIO_H`).
- [ ] **Step 2:** It fails (8 / 12 / 12 / 8).
- [ ] **Step 3:** Sapaan and captions use `note_h` and put line i at `divider + NOTE_PAD + i * DESC_LINE_H`; `ACC_ROW_H = ROW_H + ACC_TILE + BLOCK_PAD`; `MONITOR_BLOCK_PAD` makes the block end `BLOCK_PAD` below the last radio label.
- [ ] **Step 4:** `cargo test --locked` passes (update the Sapaan and accessory tests' numbers).
- [ ] **Step 5:** Commit `fix(settings): one padding for card footnotes and blocks`.

### Task 5: Soft top edge while scrolled

**Files:**
- Modify: `src/ui/settings_layout.rs` (`SCROLL_FADE_H`, `Layout::top_fade`), `src/ui/settings_controls.rs` (`content`, a gradient fill helper next to `gradient_round`)

**Interfaces:**
- Produces: `pub fn top_fade(&self) -> Option<Rect>` on `Layout`: `Some(Rect { x: v.x, y: v.y, w: v.w, h: SCROLL_FADE_H })` when `viewport` is set and `scroll > 0.0`, else `None`.

- [ ] **Step 1: Failing test** `the_top_fades_only_while_scrolled`: Features at the minimum size: scroll 0 gives `None`; scroll 100 gives a 16-high rect at `VIEW_TOP` across the viewport; Integrations (no viewport) gives `None` at any scroll.
- [ ] **Step 2:** It fails (no method).
- [ ] **Step 3:** Add the method and constant. In `content()`, after `PopAxisAlignedClip` and before the scrollbar, fill `top_fade()` with a vertical linear gradient from `color(p.bg, 1.0)` at the top to `color(p.bg, 0.0)` at the bottom (`D2D1_EXTEND_MODE_CLAMP`; on any brush error draw nothing).
- [ ] **Step 4:** `cargo test --locked` passes; `cargo build --locked` clean (no warnings added).
- [ ] **Step 5:** Commit `feat(settings): fade the top edge of a scrolled section`.

### Task 6: Look at it

- [ ] Build debug, capture every section with the scratchpad `shoot-settings.ps1` (English and Indonesian; dark; plus `features` light and `DI_SURFACE=terminal`) and compare with the "before" shots: buttons, list dividers, titles, footnotes, the fade while scrolled (`DI_SETTINGS_SCROLL=300`). Check 150 % DPI by hand only if a 150 % monitor is available; geometry is in DIPs, so say so in the report otherwise.
