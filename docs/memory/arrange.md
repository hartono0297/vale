---
tags: [decision, gotcha, arrange, susun, windows, hotkey, config, v3.10]
date: 2026-11-01
---

# Susun (Arrange): the window layouts (v3.10)

Vale arranges open windows into a tile layout. The user picks a template, Vale
walks them through the tiles one at a time ("Ubin 2 dari 3"), then moves and
resizes the windows. Never called "tiles" in code or UI: `tiles.rs` and
`ShortcutTiles` are the Pintasan tiles ([[shortcut-tiles]]); here it is
`arrange` / `Arrange*`, and the slots of a layout are leaves of a tree.

Files: `src/arrange.rs` (pure model, tested), `src/arrange_win.rs` (Win32:
monitors, window list, icons, mover), `src/arrange_overlay.rs` (the two
overlays), `src/app_arrange.rs` (the island's side, a child of `app` like
[[devices]]), `panel_layout::arrange_layout`, `ui/render_arrange.rs`, the card
in `settings_layout.rs`, `--demo-arrange` in `ui/demo.rs`.

## Model (`arrange.rs`)

- `Node` is a binary tree: `Leaf` or `Split { dir, ratio, a, b }`, leaves
  numbered depth first (a before b): that is the pick order. `Dir::Cols` is
  side by side (vertical divider). The six built-ins (`cols2`, `cols3`,
  `main2`, `grid4`, `two_thirds`, `main3`) are `Node`s too (thirds are nested
  splits with ratio 1/3 and 1/2), so one `to_rects` serves all. They are not
  editable or deletable.
- Editor rules in the model: max 6 leaves, a split is refused when a half would
  be under 15 % of the monitor's width (Cols) or height (Rows), `set_ratio`
  snaps the divider's **absolute** position to multiples of 0.05 of the monitor
  and clamps so every tile keeps 15 %; dividers inside the two halves keep
  their absolute places (a rewrite top-down, `refit`), so dragging a big
  divider never squeezes a small tile. `merge` lets the sibling subtree take the
  parent's place. A loaded tree is only used when `is_valid`.
- Gap maths: `tile_rects` takes the edges from the fractions once per position
  (neighbours share the pixel), insets an edge on the work area by the gap and
  an inner edge by half (left/top rounds down, right/bottom up: neighbours are
  exactly `gap` apart, no pixel lost). Gap 0..=24 logical px, default 8, scaled
  by the target monitor's DPI/96 (`scaled_gap`).
- Config `config.arrange` (`ArrangeSettings`: gap, templates, saved; an old
  `hotkey` key inside it is ignored and drops out on the next save)
  reads leniently, like `panel_pages`: a broken template or saved entry (bad
  tree, unknown built-in id, bad name, tile count not matching the layout) drops
  only itself, a non-array is an empty list, gap clamps, past 12 are dropped.
  A saved arrangement stores a built-in by id (`{"builtin":"main2"}`) or a full
  copy of a custom tree (`{"custom":{..}}`), so deleting the template later
  breaks nothing. It is the island's own field (`merge_owned`).
- Matching saved windows (`match_windows`): greedy in slot order, each open
  window once; candidates have the same exe path (case-insensitive), or the
  same class when the saved exe is empty; among them the same class wins, then
  the closest title (`title_similarity`: twice the longest common subsequence of
  the letters and digits, lower case, over both lengths), then the earlier in
  z-order. No candidate: the slot is skipped. `resolve_monitor`: device name,
  else identical rect, else the notch's monitor.
- `PickFlow`: pick, skip ("Lewati ubin"), back (Backspace: one tile back, frees
  its window; nothing at tile 1).

## Win32 (`arrange_win.rs`) and the mover

- The process is PerMonitorV2 (app.manifest): all numbers are physical px.
  `monitors()` (EnumDisplayMonitors + MONITORINFOEXW + GetDpiForMonitor) sorted
  left to right then top to bottom. `open_windows()` is Alt-Tab-like (visible,
  not cloaked, no `WS_EX_TOOLWINDOW`, unowned or `WS_EX_APPWINDOW`, titled, not
  the desktop or a taskbar, not our process); minimized windows are listed.
  `capture::win::top_windows` is **not** changed; the overlay's hit test reuses
  it (visible frames in z-order, frontmost first).
- **Frame compensation.** `SetWindowPos` sets the window rect, which has an
  invisible border of a few px; the target is grown by `GetWindowRect` minus
  `DWMWA_EXTENDED_FRAME_BOUNDS`. A move to a monitor of another DPI changes the
  border (and the app may resize itself on `WM_DPICHANGED`), so both rects are
  read again and the move is corrected once when the visible frame is off by
  more than 1 px.
- **Worker, never the UI thread.** A hung app blocks a `SetWindowPos` (it sends
  messages), so moves and undo run on short-lived threads and report as
  `Event::Arrange(ArrangeEvent::Done { run, moved, skipped, undo })`. Skipped:
  gone window, `IsHungAppWindow`, a target process that is elevated while Vale is
  not (token check; a token that cannot be opened is tried, a failed move counts
  as skipped). Minimized or maximized windows are restored first. Undo is
  `SetWindowPlacement` of the saved placements (rect plus maximized/minimized),
  with the show command made quiet (`SW_SHOWNOACTIVATE` / `SW_SHOWMINNOACTIVE`,
  else undo raises each window in turn and reverses the z-order). **Snap
  gotcha:** a snapped window's `rcNormalPosition` is its place *before* the
  snap, so a normal window also saves its `GetWindowRect` and undo
  `SetWindowPos`es it back there when the placement left it elsewhere.
  "Simpan…" is offered only when at least one tile got a window.
  No animation. Icons are read by `search_details::shell_icon` on a worker, a
  generic icon until ready.
- Never touches Windows Snap Layouts or the registry, never launches anything.
  `live_move_own_test_window` (`#[ignore]`) makes its OWN window and moves only
  that one.

## Overlays (`arrange_overlay.rs`)

- Both are layered windows drawn with `UpdateLayeredWindow` (software D2D target
  on a DIB). **Alpha-0 click-through gotcha:** a layered window passes clicks
  through pixels with alpha 0, so the pick overlay clears its whole canvas to
  alpha 1/255 (invisible, but it takes the click). The hover outline is the
  opposite: `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`, alpha 0.
- The pick overlay covers the virtual screen, takes the foreground for the keys
  (Backspace, Esc) and posts `WM_APP_ARRANGE` (WM_APP + 61) to the island.
  Losing the foreground cancels, armed only after it first holds it (the
  `armed` gotcha of [[screenshot-snap]]). The island is re-raised `HWND_TOPMOST`
  after the overlay shows and after every activation or click (a click raises the
  overlay over it). While the panel is open the stay-behind setting already keeps
  the island on top.
- **Self-fullscreen gotcha:** Vale's own topmost pick overlay (the whole
  virtual screen) is reported as a fullscreen app by
  `SHQueryUserNotificationState` (QUNS_BUSY), the same family as the capture
  overlay in [[screenshot-snap]]. Unhandled, the next `TIMER_SYSTEM` tick
  hides the island, `arrange_page_shown` turns false and `sync_arrange`
  cancels the pick about a second after it began. So `check_fullscreen` does
  not ask while `arrange_overlay::active()` (`fullscreen_event` returns None;
  `App.fullscreen` and `State.fullscreen` stay as they were and never
  disagree, which is why the reducer is not guarded), and `page_shown` ignores
  fullscreen while picking. A real fullscreen app mid-pick takes the
  foreground from the overlay: that posts MSG_CANCEL (armed), `cancel_pick`
  closes the overlay and the next `TIMER_SYSTEM` tick hides the island.
  `on_arrange_msg` does NOT check fullscreen right after the cancel: the
  overlay was destroyed a moment earlier and QUNS can still say BUSY for it,
  which would hide the island on a plain Esc (every cancel goes the same
  way). Up to a second late for a real game is fine. The hover outline is not
  skipped: it never activates and is click-through, and covers only the work
  area; where the work area equals the monitor (auto-hiding taskbar) it is
  made 1 px shorter (`outline_area`) and it is never shown while
  `State.fullscreen()` is true, so a game launched while the pointer rests on
  a card still hides the island.
- The panel stays open through the pick step in every open mode
  (`Event::ArrangePick`, part of `panel_held`) and `Dismiss` is ignored, see
  [[open-mode]]. Leaving the Susun page cancels (`sync_arrange`). Nothing runs at
  idle: no timer, no polling; the overlay redraws when its pointer target
  changes ([[bata-idle-power-budget]]).

## Page, notice, name field

- Template step: monitor chips with two or more monitors (default = the notch's
  monitor), six built-in cards then the user's, then "Tersimpan". Hover on a card
  shows its outline on the target monitor's work area; the wheel scrolls only
  while the content overflows ([[panel-pages]], like `App::devices_wheel`).
- After a move, `NoticeKind::Arranged { save }` ("Tersusun · n jendela" plus
  " · k dilewati") for 3 s with "Batalkan" and "Simpan…"; held under the pointer
  and while the name field is up (`Event::ArrangeNaming`). A saved run says "n
  dari m jendela, k dilewati" with Batalkan only. With 12 saved, Simpan… is off
  and the notice says the list is full.
- The name field reuses the quick search's hidden EDIT (`search_input`): the
  search takes it back when it opens (`focus_search` ends the naming). Enter
  saves (1..=24 chars, default "Susunan n"), Esc and losing the keyboard cancel.

## Settings and entry points

- Fitur has a "Susun" card (after Musik, so the tests anchored to the end of
  Fitur still see their rows): gap slider 0-24 px (the first row; the hotkey
  row moved to Tombol pintas, [[hotkeys]]),
  "Template saya" and "Susunan tersimpan". The template editor (16:9 canvas,
  click a tile, "Belah kiri-kanan" / "Belah atas-bawah", "Gabung", drag dividers,
  Nama field, Simpan, Batal) takes the whole Fitur page while it is open: it uses
  the page's one Nama EDIT (`Field::TileName`), so the excluded-apps field is not
  there at the same time. The canvas is not a hit: the window's pointer handling
  uses the pure `canvas_press` / `canvas_drag`.
- `Page::Arrange` sits between Cli and Shortcuts, `Page::ALL` has 14 pages; an
  old saved list gets it right after Cli, shown (`PanelPages::from_saved`).
  `Shortcut::ALL` has 22 with `Go(Arrange)` after `Go(Cli)`, off by default. New
  `Icon::Layout`, label "Susun" / "Arrange".
- Hotkey: the `arrange` row of `config.hotkeys` ([[hotkeys]]), off by default
  (it was Ctrl+Alt+S, the old `arrange.hotkey`, migrated once),
  `RegisterHotKey` id 4 (Clipboard 1, Search 2, Devices 3, Screenshot 5); it
  opens the page at the template step via the shared `hotkey_held` hold, like
  Devices.

Related: [[panel-pages]], [[devices]], [[screenshot-snap]], [[open-mode]],
[[settings-structure]], [[shortcut-tiles]]

**Not verified by hand:** the pick step lasting more than a few seconds on
one monitor, a real fullscreen app mid-pick, a real arrangement of real windows (the live test only
moves its own window; the demo never calls the mover), elevated windows, a move
across monitors of different DPI, the pick step with the keyboard, and typing in
the name field. The logic around them is tested; the layout and the overlays
were checked in `--demo-arrange` and `--settings features`.
