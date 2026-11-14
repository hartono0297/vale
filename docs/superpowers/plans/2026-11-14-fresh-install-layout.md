# Fresh-install layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Vale install with no config starts as a Notch with Pintasan as the first page, 9 shown pages and 12 tiles; every existing config behaves as before.

**Architecture:** `PanelPages::fresh()` and `ShortcutTiles::fresh()` build the two lists. `Config::fresh()` is `Config::default()` plus Notch and those lists. `config::load()` returns it only when the file is `OnDisk::Missing`. The state starts on the first shown page.

**Tech Stack:** Rust 2024, serde, unit tests inside `src/**` (`cargo test --locked`).

**Spec:** `docs/superpowers/specs/2026-11-14-fresh-install-layout-design.md` (mockup: `docs/design/notch-awal-layout.html`, card A).

## Global Constraints

- `Config::default()`, `PanelPages::default()` and `ShortcutTiles::default()` do not change.
- No new setting, no UI text, no change to `i18n.rs` (shared with other sessions).
- Never run `cargo fmt` on the repo; format new code by hand (note `repo-not-rustfmt-clean`).
- Shown pages, in order (9): Pintasan (`Page::Shortcuts`), Kalender (`Calendar`), Pengingat (`Reminders`), Catatan (`Notes`), Pomodoro, Musik (`Media`), Clipboard, Notifikasi (`Notifications`), Rak file (`Shelf`). Hidden, in this order after them: `Record`, `Mixer`, `System`, `Devices`, `Cli`, `Arrange`.
- Shown tiles, in order (12 = `MAX_SHOWN`): `Go(Calendar)`, `Go(Reminders)`, `Go(Notes)`, `Go(Pomodoro)`, `Go(Media)`, `Clipboard`, `Go(Shelf)`, `Go(Notifications)`, `Screenshot`, `Search`, `Go(Record)`, `Go(Mixer)`. Every other entry of `Shortcut::ALL` follows, off, in `Shortcut::ALL` order.
- Style is `Style::Notch`; size, cat and clock stay at the defaults.
- One test in the whole suite fails on this PC regardless (the `Z:` drive test, `app::record::tests::a_folder_is_writable_only_when_a_file_can_be_made_in_it`); report it as known, use `cargo test --locked -- --skip a_folder_is_writable`, and delete `target/test-tmp` after a full run.

## Review Focus

- A `config.json` that is only whitespace or a BOM gets the fresh layout (Task 2 test).
- A corrupt `config.json` and a read error give `Config::default()`, not the fresh layout (Task 2 test).
- A valid partial file such as `{"style":"Pill"}` keeps Pill and all 15 pages (Task 2 test).
- The fresh lists survive save then parse unchanged: 9 shown pages, 12 shown tiles, no tile dropped by the `MAX_SHOWN` rule (Task 2 test).
- A tile whose page is hidden (Rekam, Mixer) still opens it (Task 3 test).

---

### Task 1: The fresh page and tile lists

**Files:**
- Modify: `src/pages.rs` (add `PanelPages::fresh`, tests in its `mod tests`)
- Modify: `src/tiles.rs` (add `ShortcutTiles::fresh`, tests in its `mod tests`)

**Interfaces:**
- Produces: `PanelPages::fresh() -> PanelPages` and `ShortcutTiles::fresh() -> ShortcutTiles` (both `pub`, no arguments; entries for every page / built-in tile so `from_saved(&x.to_saved())` returns `x`).

- [ ] **Step 1: Write the failing tests**

In `src/pages.rs`: `fresh_shows_nine_pages_pintasan_first` asserts `PanelPages::fresh().visible()` equals the 9 pages of Global Constraints in order; `fresh_lists_every_page_once_with_the_hidden_ones_last` asserts `entries().len() == Page::ALL.len()`, no page repeated, and the last six entries are the hidden pages in the stated order, all off; `fresh_survives_a_save` asserts `PanelPages::from_saved(&PanelPages::fresh().to_saved()) == PanelPages::fresh()`; `default_is_unchanged` asserts `PanelPages::default().visible() == Page::ALL.to_vec()`.

In `src/tiles.rs`: `fresh_shows_the_twelve_main_tiles_in_order` asserts the shown built-ins (filter `on`) equal the 12 tiles of Global Constraints in order and number `MAX_SHOWN`; `fresh_lists_every_built_in_once` asserts each `Shortcut::ALL` entry appears exactly once and the off ones follow in `Shortcut::ALL` order; `fresh_survives_a_save` asserts `ShortcutTiles::from_saved(&ShortcutTiles::fresh().to_saved()) == ShortcutTiles::fresh()`; `default_is_unchanged` asserts `ShortcutTiles::default()` still shows 11 tiles (the existing `the_default_lists_every_built_in_and_shows_the_default_ones` already holds this; do not duplicate it if it covers the count). Use whatever accessor the existing tests use to read `entries` (`entries()` or the field, same module).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked fresh_ -- --skip a_folder_is_writable`
Expected: compile error, `fresh` is not defined.

- [ ] **Step 3: Implement `PanelPages::fresh()` and `ShortcutTiles::fresh()`**

`PanelPages::fresh` builds `entries` from two const arrays (the 9 on, then the 6 off). `ShortcutTiles::fresh` builds the 12 on from a const array of `Shortcut`, then every other `Shortcut::ALL` entry off. Each gets a doc line saying it is the first-run layout and that `default()` is unchanged on purpose.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --locked fresh_ -- --skip a_folder_is_writable`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/pages.rs src/tiles.rs
git commit -m "feat(layout): fresh page and tile lists for a first run"
```

### Task 2: `Config::fresh()` and the first-run load

**Files:**
- Modify: `src/config.rs` (add `Config::fresh`, change `load`, tests in its `mod tests`)

**Interfaces:**
- Consumes: `PanelPages::fresh()`, `ShortcutTiles::fresh()` (Task 1; `to_saved()` gives `Vec<PageEntry>` / `Vec<TileEntry>`).
- Produces: `Config::fresh() -> Config`; `fn load_at(path: &Path) -> Config` (private or `pub(crate)`) which `load()` calls with `path()`.

- [ ] **Step 1: Write the failing tests**

`fresh_is_a_notch_with_the_new_layout`: `Config::fresh()` has `style == Style::Notch`, `PanelPages::from_saved(&c.panel_pages).visible()` equals `PanelPages::fresh().visible()`, shown tiles equal `ShortcutTiles::fresh()`'s, and `Config { style: Style::Pill, panel_pages: vec![], shortcut_tiles: vec![], ..Config::fresh() } == Config::default()` (nothing else differs). `defaults_match_spec` (existing) must still pass untouched.

`load_at` tests, each writing a file into a temp dir (use the helper the existing config tests use for temp paths; else `std::env::temp_dir()` with a unique name, removed at the end): no file gives `Config::fresh()`; a file with only `"  \n"` gives fresh; a file with only a BOM gives fresh; `{"style": ` (corrupt) gives `Config::default()`; `{"style":"Pill"}` gives `Config { style: Style::Pill, ..Config::default() }` with `visible().len() == 15`; a directory at the path (read error) gives `Config::default()`; and `fresh_survives_a_save`: `Config::fresh().save_to(path)` then `load_at(path) == Config::fresh()`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked config::tests -- --skip a_folder_is_writable`
Expected: compile error (`fresh`, `load_at` missing).

- [ ] **Step 3: Implement**

`Config::fresh()`: `Config { style: Style::Notch, panel_pages: PanelPages::fresh().to_saved(), shortcut_tiles: ShortcutTiles::fresh().to_saved(), ..Config::default() }`. `load_at(path)` is a `match read_at(path)`: `Missing` gives `Config::fresh()`, `Valid(c)` gives `c`, `Invalid` gives `Config::default()` (what `parse` did before). `load()` becomes `load_at(&path())`. Do not touch `parse`, `try_parse` or `save_merged_*`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --locked config -- --skip a_folder_is_writable`
Expected: PASS, including every existing config test.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(layout): a first run with no config starts on the fresh layout"
```

### Task 3: The panel starts on the first shown page

**Files:**
- Modify: `src/state.rs` (new `pub fn show_first_page(&mut self)` near `set_pages`, line ~2823; tests in its `mod tests`, helper `st()`)
- Modify: `src/app.rs` (call it right after the `Event::PanelPages(pages)` startup apply, line ~746)

**Interfaces:**
- Consumes: `PanelPages::fresh()`, `Event::PanelPages`.
- Produces: `State::show_first_page(&mut self)`: sets `self.page` and `self.search_prev` to `self.pages.visible()[0]`.

- [ ] **Step 1: Write the failing tests**

`start_page_is_the_first_shown_page`: `st()`, apply `Event::PanelPages(PanelPages::fresh())`, call `show_first_page()`, then `s.apply(Event::Hover(true), t0)` and assert `s.view() == View::Panel(Page::Shortcuts)`. `start_page_with_the_default_list_is_calendar`: same with `PanelPages::default()` gives `Page::Calendar`. `search_returns_to_the_first_page`: after `show_first_page()` with the fresh list, `search_prev` is `Page::Shortcuts` (the module may read the private field, as line 9718 does). `a_tile_for_a_hidden_page_still_opens_it`: with the fresh pages, panel open, `Event::Click(Hit::Shortcut(Shortcut::Go(Page::Mixer)))` leaves `s.page() == Page::Mixer`; mirror `a_go_tile_opens_a_page_switched_off_for_that_visit_only` (line ~6296) for the setup.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --locked start_page -- --skip a_folder_is_writable`
Expected: compile error, `show_first_page` missing.

- [ ] **Step 3: Implement `show_first_page` and call it from `app.rs`**

One method, no event. In `app.rs` call it as `app.state.show_first_page()` right after `app.apply(Event::PanelPages(pages), now)` in the startup block; do not call it on later page changes from Settings.

- [ ] **Step 4: Run to verify they pass, then the whole suite**

Run: `cargo test --locked -- --skip a_folder_is_writable`
Expected: PASS. Then delete `target/test-tmp`.

- [ ] **Step 5: Commit**

```bash
git add src/state.rs src/app.rs
git commit -m "feat(layout): the panel starts on the first shown page"
```

### Task 4: Docs

**Files:**
- Create: `docs/memory/fresh-install-layout.md` (frontmatter `tags`, `date: 2026-11-14`; the decision, why `Default` is untouched, the three seeds, `load_at`, the startup page, links to [[panel-pages]], [[shortcut-tiles]], [[config-parse-default-fallback]], [[whats-new]])
- Modify: `docs/memory/index.md` (one line under Decisions)
- Modify: `README.md` (one sentence in "Why Vale" or Install: a new install starts as a notch with the Shortcuts page first; everything else is in Settings -> Panel)

- [ ] **Step 1: Write the note, the index line and the README sentence** (plain wording, no new claims beyond the spec; say what is not verified live: the first run on a clean profile).

- [ ] **Step 2: Commit**

```bash
git add docs/memory/fresh-install-layout.md docs/memory/index.md README.md docs/design/notch-awal-layout.html docs/superpowers
git commit -m "docs: fresh-install layout note, mockup, spec and plan"
```

(AGENTS.md step 5 later folds the memory and README update into the "docs: update memory and README" commit on `main`; if this task already did it, say so there instead of duplicating.)

## Verification before merge

- `cargo test --locked -- --skip a_folder_is_writable` green; report the skipped Z: test as the known one.
- A clean-profile look is optional and slow (release build is 1.5 h+): if wanted, run the debug build with `APPDATA` pointed at an empty folder and confirm Notch, Pintasan first, 12 tiles; otherwise say it was not checked live.
