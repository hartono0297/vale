# Fresh-install layout (short design)

Date: 2026-11-14. Mockup: `docs/design/notch-awal-layout.html` (card A, revised).

## Intent

Someone who installs Vale for the first time should see a tidy island at once,
with defaults chosen on purpose. Today a new install gets a floating Pill,
all 15 pages in the order they were added, 11 shortcut tiles (the last row
holds 3 of 4) and a Calendar page that is empty without Google.

## What the owner decided

- Closed island: **Notch** (flush with the top edge), 190x34, cat and clock
  (size, cat and clock are today's defaults; only the shape changes).
- **Pintasan is the first page**, and it is the page the panel opens on the
  first time. Its tiles are the main features, in the owner's trust to order.
- Applies to **new installs only** (no config file). Existing users are never
  touched.

## The layout (my call on the order, as the owner allowed)

- Page order, 9 shown: Pintasan, Kalender, Pengingat, Catatan, Pomodoro,
  Musik, Clipboard, Notifikasi, Rak file. Hidden (still reachable from a tile
  or Settings -> Panel): Rekam, Mixer, Sistem, Perangkat, CLI, Susun.
- 12 tiles, 4 columns, 3 full rows: Kalender, Pengingat, Catatan, Pomodoro /
  Musik, Clipboard, Rak file, Notifikasi / Screenshot, Cari, Rekam, Mixer.
  The other built-in tiles stay in the list, off.

## Goals, non-goals

- Goal: a first run that looks deliberate; no change for anyone who already
  has a config.
- Non-goals: a welcome screen, a "starting style" picker, resetting existing
  users, changing `Config::default()`, new settings, new copy (no UI text).
- Anti-goal: a corrupt or unreadable `config.json` must not look like a fresh
  install (it falls back to `Default` today and keeps doing so).

## Approaches

1. **Change `Config::default()`, `PanelPages::default()`, `ShortcutTiles::default()`.**
   Smallest diff, but `Default` is also the fallback for a bad config and what
   `serde(default)` fills in for an old file that lacks a key, so an existing
   user could see their layout change. Rejected.
2. **Seed once at first run (chosen).** `config::load()` already reads the
   file; when it is `Missing` (no file or only whitespace) it returns a
   `Config::fresh()`: `Default` plus Notch, the 9-page list and the 12-tile
   list. `Default` stays as it is. Existing users, invalid files and
   hand-written partial configs behave exactly as before.
3. **A first-run style picker.** More to build and to explain; the owner wants
   one good default. Left out.

## Parts

- `Config::fresh()` in `src/config.rs`; `load()` uses `classify` to tell
  Missing from Invalid (Invalid and read errors still give `Default`).
- `PanelPages` and `ShortcutTiles` get a constructor for the fresh lists (entries
  for every page / tile, so `from_saved` and the Settings cards need no change).
- The panel's first page follows the first shown page: `state.page` starts
  as `Page::Calendar` (`src/state.rs`, also `search_prev`); at startup, after
  `Event::PanelPages`, it must start on the first shown page. Behaviour for
  an existing config whose first page is Calendar is unchanged.
- What's new: a fresh install already stays silent (`!onboarding_done`); the
  seed does not touch `whats_new_seen`.

## Tests (TDD)

- `Config::fresh()` is Notch; `visible()` is the 9 pages in that order; the
  shown tiles are the 12 in that order; `Config::default()` is unchanged.
- `load` from Missing or whitespace gives fresh; a valid file, an Invalid file
  and a file with only `{"style":"Pill"}` do not.
- A fresh config round-trips through save and parse.
- The first page is the first shown page; with the default list it is still
  Calendar.

## Left to the builder

Exact constructor names, where in startup the first page is set, README and
Obsidian note wording, and the existing tests that assert today's defaults
(they keep passing, since `Default` does not change).
