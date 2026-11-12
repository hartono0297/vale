---
tags: [convention, settings, layout, buttons]
date: 2026-11-12
---

# Settings: one spacing rule, one button size

Every Settings page follows these numbers (DIPs, `src/ui/settings_layout.rs`).
They came from an audit of all ten pages (2026-11-12): list buttons were
narrower than the rest ("Hapus" 60 in a list, 80 in the last row), list
titles floated over a blank row, footnotes and blocks had their own paddings.

| What | Rule | Where |
|---|---|---|
| Text button (Primary, Secondary, Confirm) | 30 high, `Font::Control` 12, label + 16 each side, at least 64 wide | `BUTTON_H`, `BUTTON_PAD`, `BUTTON_MIN_W`, `Builder::button_w` (the only width; `compact_w` is gone) |
| Button that swaps its label or stands in a column | the widest label's width (two-click Hapus/Yakin?, Putuskan, editor split/merge, the Transkrip column) | `button_w(&[a, b])` |
| Round icon button, stepper, segmented | 28 | `STEP_BUTTON`, `CONTROL_H` |
| Text field | as tall as a button (30) | `FIELD_H = BUTTON_H` |
| Row | 52, label +7, description +28, label-only centred; card inset 16 | `ROW_H`, `INSET` |
| Between two cards | 12 | `CARD_GAP` |
| Section title (eyebrow) | 24 under what is above it, 8 to its card; the first one of a page at `FIRST_CARD_Y` | `GROUP_GAP`, `EYEBROW_GAP`, `Builder::eyebrow` (returns its top) |
| List inside a card | every item and the empty text is a 52 row with a divider at its top | `arrange_card`, `notifications_card`, the Panel lists |
| Card footnote | divider, 12, the lines, 12 | `NOTE_PAD`, `note_h` |
| Block under a row's text | starts at the row's bottom, 16 under it | `BLOCK_PAD` (accessory tiles, monitor list) |
| Scrolled section | a fade from the window background at the viewport's top, from under the title down to `FIRST_CARD_Y` (8 high, so a title jumped to is never dimmed), opaque whatever the content fade, only while `scroll > 0` | `SCROLL_FADE_H`, `Layout::top_fade`, `fade_out_down` |

- **Why 64, not 80:** with one formula, the Panel own-tile row in Indonesian
  (Hapus + Sembunyikan + the two arrows) left 142 px for a 150 px note at the
  720 minimum width. 64 also lets the width follow the label (Add < Delete).
- **Hand-set `b.y`** (the pomodoro locked note, Google's "Lanjutan", the About
  header) ends at its content + `CARD_GAP`; `eyebrow` adds the rest of the 24.
  Anything placed on an eyebrow's line uses the y `eyebrow` returns, never
  `b.y` read before it.
- Tests: `every_text_button_has_one_size`, `fields_are_as_tall_as_buttons`,
  `card_lists_have_a_divider_per_row`, `section_titles_sit_with_their_card`,
  `footnotes_and_blocks_share_padding`, `the_top_fades_only_while_scrolled`.
  A new card or row should pass them without changes; if it cannot, the
  rule is what changes, here and in the tests.

Related: [[settings-structure]], [[settings-window-size]], [[settings-appearance-scroll]]
