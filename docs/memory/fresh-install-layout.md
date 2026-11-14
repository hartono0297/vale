---
tags: [decision, config, layout]
date: 2026-11-14
---

# Fresh-install layout

A first run starts as a Notch (`Style::Notch`) on the Shortcuts page, with the
main features as tiles. Everything stays editable in Settings → Panel. Spec and
plan: `docs/superpowers/specs|plans/2026-11-14-fresh-install-layout*`, mockup
`docs/design/notch-awal-layout.html`.

## Why `Default` is untouched

`Config::default()` is also what [[config-parse-default-fallback]] returns for a
corrupt file and what every test and `..Config::default()` builds on. Changing it
would hand a broken-file user a new layout, so the new layout is a separate
`Config::fresh()` used only on a true first run.

## The three seeds

- `PanelPages::fresh()` ([[panel-pages]]): 9 shown pages (Pintasan, Kalender,
  Pengingat, Catatan, Pomodoro, Musik, Clipboard, Notifikasi, Rak file) and 6
  hidden (Rekam, Mixer, Sistem, Perangkat, CLI, Susun).
- `ShortcutTiles::fresh()` ([[shortcut-tiles]]): 12 tiles, three rows of four
  (Kalender, Pengingat, Catatan, Pomodoro / Musik, Clipboard, Rak file,
  Notifikasi / Screenshot, Cari, Rekam, Mixer). Rekam and Mixer are hidden pages
  that still open from their tile.
- `Config::fresh()`: `Style::Notch` plus the two lists, the rest default.

## When it applies

`config::load()` calls the private `load_at(path)`: a missing, empty,
whitespace-only or BOM-only `config.json` gives `Config::fresh()`; a corrupt or
unreadable file still gives `Config::default()`.
There is no "first run" flag: once saved, it is an ordinary config. [[whats-new]]
stays silent on a fresh install.

## The startup page

`State::show_first_page()` runs once at startup after the `PanelPages` apply in
`app.rs`, so the panel opens on the first shown page. With the default 15-page
list that is still Calendar. Small effect on existing users: a config whose first
shown page is not Calendar now starts on that page.

## Tests that hold it

`fresh_*` in `pages.rs` and `tiles.rs` (counts, order, every item once, survives
a save); `config.rs`: `fresh_is_a_notch_with_the_new_layout`,
`no_file_or_a_blank_one_loads_the_fresh_layout`, `classify_tells_missing_invalid_and_valid_apart`,
`fresh_survives_a_save`; `state.rs`: `start_page_is_the_first_shown_page`.

## Not verified live

A first run on a clean profile was not run (a release build takes 1.5 h+). The
unit tests cover the lists and the load path; the real window with an empty
`%APPDATA%` still needs a look: Notch, Pintasan first, 12 tiles.
