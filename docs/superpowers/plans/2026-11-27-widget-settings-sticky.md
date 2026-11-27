# Widget settings: Catatan tempel page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings → Widget shows only "Tampilkan widget" and the gallery; clicking the Catatan tempel card opens a Catatan tempel page (inside the Widget section) holding the other six settings.

**Architecture:** Same pattern as Susun's template editor on Fitur (`arrange_edit`): the Settings window keeps a `sticky_page: bool`, passes it to the layout `State`, and `widgets_page` draws either the gallery page or the Catatan tempel page. No new `Section`, no data or config change.

**Tech Stack:** Rust 2024, Win32 + Direct2D, layout tests in `src/ui/settings_layout.rs`.

**Spec:** `docs/superpowers/specs/2026-11-27-widget-settings-sticky-design.md` (mockup `docs/design/widget-settings-rapi.html`)

## Global Constraints

- Labels and descriptions of every setting stay as they are (`Key::Widgets*`); `WidgetPrefs` and the `widgets` config key do not change.
- Indonesian and English for every new string (`src/i18n.rs`, `Key => "id", "en";`).
- Spacing follows the existing builder (`titled`, `b.card`, `ROW_H`, `wrapped_row_h`); no new constants unless a size is new.
- Test: `cargo test --locked`. On this PC the drive Z: tests always fail and timing tests can fail under load; rerun those alone before calling anything red.
- Files touched: `src/i18n.rs`, `src/ui/settings_layout.rs`, `src/ui/settings_controls.rs` (Task 1); `src/ui/settings.rs` (Tasks 1 and 2). The parallel session "Sticky note: maskot ikut pilihan user" may edit the `WidgetsPeek` row in `settings_layout.rs` / `i18n.rs`; keep whatever name it lands, put the row in the Tampilan card.

## Review Focus

1. Opening the page while the gallery page is scrolled: the page starts at the top (scroll reset to 0) — Task 2.
2. Leaving the Widget section and coming back, or reopening Settings: the gallery page shows, not the Catatan tempel page — Task 2.
3. At 20 notes the (+) on the Catatan tempel page is off and the reason (`Key::StickyMax`) shows in the warning tone — Task 1.
4. The template menu from the page's (+) opens under that (+): `template_menu_after` finds the `IconButton { hit: Hit::WidgetAdd }` rect, so the page must have exactly one such button — Task 1.
5. The card is clickable and shows its hover frame with 0 notes too; with 0 notes the click posts no `WidgetHighlight` — Tasks 1 and 2.

---

### Task 1: Layout — gallery page and Catatan tempel page

**Files:**
- Modify: `src/i18n.rs` (keys near `GroupWidgetsGallery`, ~line 1345)
- Modify: `src/ui/settings_layout.rs` (`Hit` enum ~line 495, `State` ~line 1577, `widgets_page`/`widget_tile` ~4270-4420, tests ~12315-12470 and `SCROLLING` ~7283, `states()` helper)
- Modify: `src/ui/settings_controls.rs:1212` (hover no longer gated on `t.active`)
- Modify: `src/ui/settings.rs:1875` (only add `sticky_page: false` to the `State` literal so it compiles; Task 2 wires it)

**Interfaces:**
- Produces: `State.sticky_page: bool` (layout state, after `sticky_count`); `Hit::WidgetBack`; i18n keys `GroupStickyPlace` ("Posisi", "Position"), `GroupStickyLook` ("Tampilan", "Look"), `GroupStickyNew` ("Catatan baru", "New notes").
- The gallery card always pushes `Control::Area { hit: Hit::WidgetCard, rect }` (any count). Drop `WidgetTile.active` if nothing reads it after the hover change.

**Page shapes:**
- Gallery page (`sticky_page == false`): one card with the `WidgetsShow` row (no eyebrow, like the mockup), then the existing "Galeri" eyebrow + count + grid with the tile. `l.cards.len() == 2`.
- Catatan tempel page (`sticky_page == true`): a header card of `ROW_H`: at the left an `icon_button(Hit::WidgetBack, Icon::Back)`, beside it `Key::StickyName` as the label with `on_desktop_label(lang, n)` as its description (or `Key::StickyMax` in `Tone::Warn` when `!can_add(n)`), at the right `icon_button(Hit::WidgetAdd, Icon::Plus, can_add(n))`. Then `titled(GroupStickyPlace)` card: Lapisan (segmented), Saat layar penuh (segmented), Kunci posisi (toggle) — reuse today's row code and wrapping. `titled(GroupStickyLook)` card: Miring sedikit, Kucing ngintip. `titled(GroupStickyNew)` card: Isi dari clipboard. `l.cards.len() == 4`. No `WidgetsShow` and no tile on this page.

- [ ] **Step 1: Rewrite the widget layout tests (failing)**

Replace `widget_has_its_settings_and_a_gallery_with_one_card`, `the_gallery_card_is_clickable_once_a_note_is_on_the_desktop`, `the_plus_is_off_at_the_cap_and_the_description_says_why`, `the_widget_controls_show_the_saved_choices`, `the_widget_section_scrolls_at_the_minimum_size_and_not_in_a_tall_window` with:

```rust
fn sticky_page(lang: Lang, count: usize) -> State<'static> {
    State { sticky_page: true, ..widgets_state(lang, count) }
}

#[test]
fn widget_page_has_the_show_switch_and_the_gallery_only() {
    for lang in [Lang::Id, Lang::En] {
        let l = layout_in(Section::Widgets, &widgets_state(lang, 0), &Estimate, TALL);
        assert_eq!(l.cards.len(), 2, "{lang:?}");
        let has = |t: &str| l.texts.iter().any(|x| x.text == t);
        assert!(has(text(lang, Key::WidgetsShow)) && has(text(lang, Key::StickyName)));
        assert!(has(&text(lang, Key::GroupWidgetsGallery).to_uppercase()));
        for key in [Key::WidgetsLayer, Key::WidgetsFullscreen, Key::WidgetsLock, Key::WidgetsPeek, Key::WidgetsTilt, Key::WidgetsClipFill] {
            assert!(!has(text(lang, key)), "{lang:?} {key:?} moved to the sticky page");
        }
        // The card opens the page at any count, 0 included.
        let tile = l.tiles[0].rect;
        assert_eq!(l.hit(tile.x + 20.0, tile.y + 20.0), Some(Hit::WidgetCard));
        // "0 di desktop" stays on the gallery eyebrow's line (keep today's assertion).
    }
}

#[test]
fn sticky_page_has_back_add_and_three_groups() {
    for lang in [Lang::Id, Lang::En] {
        let l = layout_in(Section::Widgets, &sticky_page(lang, 3), &Estimate, TALL);
        assert_eq!(l.cards.len(), 4, "{lang:?}");
        let has = |t: &str| l.texts.iter().any(|x| x.text == t);
        for key in [Key::GroupStickyPlace, Key::GroupStickyLook, Key::GroupStickyNew] {
            assert!(has(&text(lang, key).to_uppercase()), "{lang:?} {key:?}");
        }
        for key in [Key::StickyName, Key::WidgetsLayer, Key::WidgetsFullscreen, Key::WidgetsLock, Key::WidgetsPeek, Key::WidgetsTilt, Key::WidgetsClipFill] {
            assert!(has(text(lang, key)), "{lang:?} {key:?}");
        }
        assert!(has(&crate::widgets::sticky::on_desktop_label(lang, 3)));
        assert!(!has(text(lang, Key::WidgetsShow)) && l.tiles.is_empty());
        let hits: Vec<Hit> = l.hits().into_iter().map(|(h, _)| h).collect();
        for h in [Hit::WidgetBack, Hit::WidgetAdd, Hit::WidgetsLock, Hit::WidgetsPeek, Hit::WidgetsTilt, Hit::WidgetsClipFill] {
            assert!(hits.contains(&h), "{lang:?} {h:?}");
        }
        assert_eq!(hits.iter().filter(|h| **h == Hit::WidgetAdd).count(), 1, "one (+) for the template menu");
        // Back sits left of the title, (+) at the right edge of the header card.
        // Order of the groups: Posisi above Tampilan above Catatan baru.
    }
}
```

Plus: `sticky_page_plus_is_off_at_the_cap_with_the_reason` (the old cap test on `sticky_page(lang, 20)`); `sticky_page_controls_show_the_saved_choices` (the old saved-choices test on the sticky page; `WidgetsShow` checked on the gallery page); `widget_gallery_page_fits_at_the_minimum_size` (`layout(Section::Widgets, &widgets_state(Lang::Id, 2), &Estimate)` has `viewport.is_none()` and `scroll_max == 0.0`); remove `Section::Widgets` from `SCROLLING` and fix its comment and array length. If the sticky page does not fit at 720x520, assert it scrolls there and not in `TALL` (as the old scroll test did).

- [ ] **Step 2: Run, expect compile errors / failures**

Run: `cargo test --locked --bin vale widget` (if the target name differs, `cargo test --locked widget`)
Expected: FAIL — `sticky_page`, `Hit::WidgetBack`, `Key::GroupSticky*` do not exist.

- [ ] **Step 3: Implement** the keys, `Hit::WidgetBack` (give it no tooltip unless `hit_tip` requires one; if it does, use `Key::SecWidgets`), `State.sticky_page`, the two page builders (split `widgets_page` into `widgets_gallery_page` and `sticky_settings_page`), the always-clickable tile, the hover change, `sticky_page: false` in `states()` and in `settings.rs`'s `State` literal.

- [ ] **Step 4: Run the whole suite**

Run: `cargo test --locked`
Expected: all pass (except the known Z: drive tests). The spacing test around line 13799 (eyebrow line of the gallery count) must still pass.

- [ ] **Step 5: Commit** — `feat(settings): sticky notes settings move to their own page`

### Task 2: Settings window — open, close and reset the page

**Files:**
- Modify: `src/ui/settings.rs` (struct `Settings` ~942 and its init ~1447, `State` literal ~1875, `on_click` ~2772, `switch_section` ~3320)

**Interfaces:**
- Consumes: `State.sticky_page`, `Hit::WidgetBack`, `Hit::WidgetCard` (Task 1).
- Produces: `fn widget_card_after(sticky_count: usize) -> Vec<After>` (free fn in `settings.rs`): `vec![After::Post(Action::WidgetHighlight)]` when `sticky_count > 0`, else empty.

- [ ] **Step 1: Failing test** in `settings.rs`'s `mod tests`:

```rust
#[test]
fn the_card_flashes_the_notes_only_when_there_are_some() {
    assert!(widget_card_after(0).is_empty());
    assert!(matches!(widget_card_after(3).as_slice(), [After::Post(Action::WidgetHighlight)]));
}
```

- [ ] **Step 2: Run** `cargo test --locked the_card_flashes` — expect FAIL (not defined).

- [ ] **Step 3: Implement**
  - `Settings.sticky_page: bool`, `false` at open; pass it into `State`.
  - `Hit::WidgetCard` → `self.sticky_page = true; self.scroll = 0.0;` then `widget_card_after(m.sticky_count)`.
  - `Hit::WidgetBack` → `self.sticky_page = false; self.scroll = 0.0;` `Vec::new()`.
  - `switch_section`: next to the `arrange_edit` reset, `if self.section == Section::Widgets { self.sticky_page = false; }`.
  - A model push (`sync_settings`) does not touch `sticky_page`.

- [ ] **Step 4: Run** `cargo test --locked` — all pass (known Z: exceptions aside).

- [ ] **Step 5: Hand check** (report results, do not claim without doing): `cargo run --release -- --settings widgets` (stop any running `vale` first): gallery page fits; card click opens the page at the top; (+) opens the template menu under it; back returns; switching to Panel and back shows the gallery page.

- [ ] **Step 6: Commit** — `feat(settings): open and close the sticky notes page`

### After both tasks (main session, per AGENTS.md)

Whole-branch review on the most capable model, merge locally into `main`, `cargo test --locked` on `main`, then docs: `README.md` Settings / Sticky notes, `docs/guide.md#settings`, `docs/memory/settings-structure.md` (the Widget section) and `docs/memory/widgets.md`, commit "docs: update memory and README", push `main`, clean up the worktree (delete `target/` first).
