---
tags: [decision, ui, popup, theme, recording]
date: 2026-11-08
---

# Vale's own dropdown (`ui/menu_pop.rs`)

A reusable themed popup list, first used for the recording device choice
(`ui/device_menu.rs`) on the Rekam page and in Settings → Fitur → Rekaman. It
replaced the native `TrackPopupMenu` there; the tray's menus stay native.
Related: [[recording-controls]], [[terminal-windows]], [[tray-dark-menu]].

- Rows: `Row::Item { id, label, detail, checked }` (an optional grey second
  line) or `Row::Separator`. Drawn with Direct2D in `theme::current_window()`
  colours: rounded plate, soft shadow, accent check, hover fill, ellipsis.
  Width fits the text in 220..380 dip; placed under the anchor (`Align::Start`
  or `End`) inside the monitor work area, flips above, scrolls if neither
  fits; DPI from the anchor's monitor.
- Keys: Up/Down (skip separators, wrap), Home/End, Enter, Esc. Closes on
  deactivation or a press outside; one open at a time; the window exists only
  while open (nothing at idle).
- The result goes to the owner as `WM_APP_MENU_DONE` (`WM_APP + 81`) with a
  boxed `Done { pick, context }` (`take_done`), so no app borrow is held; a
  `Done` for an owner that is no longer a window is dropped. A dead entry is
  reaped by `is_open_for` / `is_open` / `close`. Focus goes back to the window
  that had it, if the dropdown still has focus.
- The island state has `menu_open` (`Event::MenuOpen`): while open it holds
  the panel like an editor and pauses the click-outside watch; a press on the
  island only closes the dropdown. The Rekam page's dropdown follows the page
  under any notice (`panel_page_under`), so "Mic pindah" does not close it.
- Debug: `DI_REC_MENU=mic|system` opens it by itself in `--settings` and
  `--demo-record`.
