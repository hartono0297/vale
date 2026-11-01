---
tags: [decision, settings, layout, window, config]
date: 2026-11-01
---

# Settings window: resizable, remembered

- **Minimum size:** 720x520 client (`CLIENT_W` / `CLIENT_H`), enforced with
  `WM_GETMINMAXINFO` (`ptMinTrackSize = outer_size(GetDpiForWindow)`). It
  arrives before the state exists, so it never uses `with()`. The style has
  `WS_THICKFRAME | WS_MAXIMIZEBOX`; maximizing uses the monitor work area.
- **Layout:** `settings_layout::layout_in(section, state, measure, client)`.
  A private `Geom` (client size, column x and width, `right`) is computed once
  and kept in `Builder::g`. The content column is
  `min(client_w - SIDEBAR_W - 2*PAD, MAX_CONTENT_W = 760)`, centred in the
  space beside the sidebar. At 720 it is exactly the old 208 / 488. The sidebar
  never moves (the brand width uses the fixed `SIDEBAR_W + PAD`). The
  viewport and scrollbar follow the client size ([[settings-appearance-scroll]]).
  `layout()` (min size) is test-only; every old test runs at the minimum.
  `State` has no size field; `Settings::client` (logical DIPs) is set from
  `WM_SIZE` (not while minimized) and re-read after a DPI change.
- **Native fields:** `relayout_and_place()` runs after every resize, so the
  EDITs follow `layout.fields` and still hide unless fully inside the
  viewport or while the content fades.
- **Remembered place:** config key `settings_window`
  (`config::WindowPlace`: left, top, right, bottom, maximized; screen
  coordinates, physical px). Read leniently (`place_from_json`: any bad
  value, size <= 0 or coordinate beyond +-100000 is None, the rest of the file
  loads). It is **not** in `merge_owned`: it comes from disk on every save and
  is written only by `Action::WindowPlace` -> `save_config_with`, so hand
  edits survive. `Model::window_place` is read only by `open()`; `update`
  ignores it.
- **Saving:** `Settings::post_place` (GetWindowPlacement: `rcNormalPosition`
  is in *workspace* coordinates, so `workspace_to_screen` adds the primary
  work-area offset; maximized = SW_SHOWMAXIMIZED, or minimized with
  WPF_RESTORETOMAXIMIZED) posts the action only when it differs from
  `last_place`. Called on `WM_EXITSIZEMOVE`, on a maximize state change in
  `WM_SIZE`, and on `WM_CLOSE`. Pure helpers in `ui/settings_window.rs`
  (`workspace_to_screen`, `mostly_visible`, `clamp_min`).
- **Restoring:** the window is **created at the saved screen rect** (so on the
  right monitor at that monitor's DPI) and then shown with
  `ShowWindow(SW_SHOWMAXIMIZED)` when it was maximized. Not
  `SetWindowPlacement`: onto a monitor with another DPI that would fire
  `WM_DPICHANGED` and `on_dpi_changed` would rescale the saved size. The
  creation rect becomes the restore rect. The rect must be `mostly_visible`
  (half its area on one monitor, `EnumDisplayMonitors`) and is grown to the
  minimum outer size at that monitor's DPI; otherwise (unplugged monitor) the
  old path: minimum size centred on the primary work area, not maximized.
- **Demo:** `--settings` is `#[cfg(debug_assertions)]`: run it with
  `cargo run -- --settings panel`; in release the flag is ignored and the real
  island starts. The demo keeps the place in
  `std::env::temp_dir()/vale-settings-demo-window.json` (never the real config): read at
  start, written on `Action::WindowPlace`. Close and reopen it, or write an
  off-screen rect into the file, to check restore and fallback.
- Checked in the debug demo through Win32 messages: 720x520 look unchanged,
  1500x900 centred column and scrollbar at the right edge, drag auto-scroll
  to the end and drop, maximize saved, reopen maximized, off-screen and tiny
  rects.

Related: [[settings-structure]], [[shortcut-tiles]], [[panel-pages]]
