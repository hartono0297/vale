---
tags: [decision, shortcuts, settings, config, v3.2, v3.4]
date: 2026-10-29
---

# Shortcut tiles you choose (v3.2 Run 7)

`src/tiles.rs` (`ShortcutTiles`, like `pages::PanelPages`): the Pintasan page
shows the tiles in `config.shortcut_tiles`, in that order, up to twelve (four
per row, up to three rows, `panel_layout::shortcuts_layout` wraps). Empty =
the default tiles (the nine built-ins, Search being the ninth since 3.2.1,
plus Vale Settings and the Shelf since 3.4), so old configs read as before.
Settings → Panel (v3.8; it was Tampilan) → **Pintasan** (laid out as in the 2026-10-29 section
below): the last tile shown cannot be hidden, and none can be switched on
while twelve show (`toggle` and `set_on` refuse; the off rows say "Halaman
penuh (maks. 12)" with a disabled "+ Tambah"). A list edited by hand can
still have more than twelve on: the rows past the twelfth say "Tidak tampil
(maks. 12)" (`beyond_limit`). Tile names in the file
(built-ins, `Custom`, the `kind`) are read in any case and written back in
the canonical spelling.

- **Built-in** entry: `{"tile":"Lock","on":false}`; each built-in is listed
  once, one missing from the file rejoins right after the one before it in
  `Shortcut::ALL`.
- **Custom** entry: `{"tile":"Custom","label","kind","target"}`; `kind` is
  `App`, `Folder` or `Url`. `CustomTile::new` is the one gate: label 1–14
  chars, target ≤ 500, no control characters; a `Url` must be http(s) with a
  host or `mailto:` (no `file:`, `javascript:`, `ms-*:`). An entry that fails
  is dropped when read, never kept to fail at click time. Max 8 custom. An
  absent `on` reads as shown.
- Click path: `Hit::Shortcut(s)` (built-in, as before) and
  `Hit::CustomTile(i)` (`i` = place among the tiles shown) →
  `Effect::RunCustom` → `Action::Custom` → `ShellExecute "open"` on the
  target, after the panel collapses like any tile. `Action` is no longer
  `Copy` (it owns a string).
- The settings scroll now counts text under the last card (the note under
  the Pintasan card), not just card bottoms.

**Not done (decision for the owner):** the "Perintah" kind (a shell command
per tile: a security surface, the plan wanted a first-run confirmation). The
add form is done, see "Page tiles, Vale Settings and the new Settings card"
below.

Related: [[open-mode]].

**3.2.1:** a built-in the file does not name (the new "Search" tile) rejoins
right after the built-in before it in `Shortcut::ALL`, and comes in *off*
when the page already shows twelve, so an upgrade never pushes an own tile
off it (`from_saved`); it can be switched on in Settings once another tile is
switched off (the twelve-tile limit above).

## Page tiles, Vale Settings and the new Settings card (2026-10-29)

- **New built-ins** in `Shortcut::ALL` (20 since v3.5, in this order): the nine,
  `ValeSettings`, then `Go(Calendar|Pomodoro|Reminders|Notes|Media|Mixer|Shelf|
  System|Cli)`. No Go tile for Search, Shortcuts or Clipboard (Clipboard keeps
  its own tile, which already opens its page). Names in the file are
  `"ValeSettings"` and `"Go"` + `pages::page_name` (`"GoShelf"`), read in any
  case, written canonically (`tiles::shortcut_from_name`).
- **`Shortcut::default_on()`**: the nine, ValeSettings and Go(Shelf) (v3.5 adds Go(Devices): 12 shown on a fresh install, so the page is full; 11 shown
  on a fresh install). A built-in the file does not name joins after its
  predecessor in ALL, on only when `default_on` **and** the page has room
  (the 12 rule is unchanged). An old nine-tile config therefore gets
  ValeSettings and the Shelf on (11 shown); a full twelve keeps its twelve.
- **Gotcha, all off:** a file whose tiles are all off is made whole *before*
  the missing built-ins join (it turns the default-on built-ins on). Judged
  after, the new tiles would count as shown and the file would stay empty.
- **Reducer:** `Hit::Shortcut(Go(p))` sets `page = p` like the Clipboard tile
  (no collapse; a page that is switched off opens for that visit only, the
  close path `leave_hidden_page` moves off it). `Hit::Shortcut(ValeSettings)`
  is `collapse_for(Effect::OpenSettings)`; `App` answers with the existing
  `open_settings(hwnd)`, as the last arm of `perform`. **Gotcha:** `perform`
  runs inside `with_app`, and `open_settings` calls `with_app` itself (it
  finds the app borrowed and returns), so the arm only sets
  `DEFERRED_OPEN_SETTINGS` and `run_deferred` opens the window once the
  handler returns. `shortcut_action` returns `None` for both, like
  Clipboard and Search.
- **Icons:** each Go tile uses its page's icon (`shortcuts::page_icon`);
  ValeSettings has its own `Icon::Sliders` so it is not the Windows gear.
  New in `icons.rs`, drawn by hand (not from the design file): `Sliders`,
  `Chip` (System), `Grip` (drag handle), `ChevU`. Labels reuse the `Page*`
  keys, except the Shelf ("Rak", `ShortcutGoShelf`: "Rak berkas" is too long
  under a tile).
- **Settings card (Appearance → Pintasan):** a header "Tampil di halaman:
  n/12" with *Kembalikan bawaan*; the tiles that are on, each with a drag
  handle, up/down (among the tiles shown; `ShortcutTiles::move_shown`),
  *Sembunyikan* and *Hapus* on an own tile; then *Tersedia* grouped
  Halaman (Go tiles and Clipboard), Windows, Vale (Settings and Search) and
  Milik sendiri, each row with *+ Tambah* (disabled with "Halaman penuh (maks.
  12)" while 12 show); then *+ Pintasan sendiri*. `TileOn(i)` is both hide and
  add (`toggle`); `reset()` puts the built-ins back and keeps own tiles after
  them, on only while there is room; `add_own` returns whether it came in on.
- **Own-tile form** (inline in the Tersedia card): Nama and Target are native
  EDITs created with the two Google ones (`Edits` has four hwnds, ids
  103/104, same subclass, colours and destruction; Esc closes the form, Enter
  adds, Tab swaps within the pair, `WM_APP_FORM` carries those). Jenis is a
  segment (Aplikasi | Folder | Web). *Pilih…* runs `IFileOpenDialog`
  (`FOS_FORCEFILESYSTEM`, `FOS_PICKFOLDERS` for a folder) from `run_after`,
  with the window state released (the dialog pumps messages); the COM
  apartment is the island's (the `--settings` demo initialises its own).
  `CustomTile::check` says which part failed; the message sits under the
  form, no message box. Adding off (page full) leaves a note for a few
  seconds. Settings still posts only `Action::ShortcutTiles`.
- **Native fields do not clip:** the form's EDITs hide unless their whole
  field rect is inside the scroll viewport, and the wheel re-places them.
- **Drag:** press on the grip (`SetCapture`), the row follows the pointer on
  a raised plate, the others shift (`display_order`), release commits through
  the pure `drop_index(y, row_tops)`, Esc or a lost capture cancels. The
  Settings model is updated at once (optimistic) and the owner confirms.
- **Edge auto-scroll (since 2026-11):** while a row (Halaman panel or
  Pintasan) is held, the content scrolls when the pointer is within 48 px of
  the viewport's top or bottom edge or beyond it. `drag_scroll_speed(y, top,
  bottom)` gives px/s: 0 outside the zone, 2 rows/s (ROW_H 52) at the zone's
  inner boundary, growing linearly to 12 rows/s at the edge and beyond; in a
  viewport shorter than two zones the nearer edge wins. `TIMER_DRAG_SCROLL`
  (id 5, 16 ms) runs only while it scrolls: `Drag::last_tick` is `Some` while
  it is set (it is not set again on every pointer move, which would restart
  its interval), the tick uses dt clamped to 50 ms, and it stops when the
  speed is 0 or scroll hits 0 / `scroll_max`. `end_drag` (release, Esc,
  `WM_CAPTURECHANGED`) always kills it. The row place comes from the pure
  `drag_place(tops, grab, y, scroll)`, re-run after each scroll change, so the
  held row stays under the pointer. The mouse wheel now scrolls during a drag
  too (it used to be ignored). The held row is still clamped to its own list.
- Not verified by hand: typing into the form's fields and the file dialog
  (the logic around them is tested; the layout and the drag were checked in
  the `--settings` demo).

Related: [[panel-pages]], [[studio-icons]].

**v3.10:** `Shortcut::Go(Page::Arrange)` (name `"GoArrange"`) sits after `Go(Cli)` in
`Shortcut::ALL` (22 built-ins) and is **off by default** (`default_on` stays false for
it): an old config joins it off, right after Cli, and the default tile count is
unchanged ([[arrange]]). "Tiles" in code and UI still means these Pintasan tiles; Susun
calls its slots leaves and its overlay `arrange`.
