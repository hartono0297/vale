---
tags: [gotcha, tray, theme]
date: 2026-11-01
---

# The tray menu follows the Windows app mode

The native tray menu (`tray::show_menu`, `CreatePopupMenu` + `TrackPopupMenu`)
is dark when Windows apps are in dark mode, light otherwise. It follows
Windows (`AppsUseLightTheme`), not Vale's own "Tema jendela" setting.

- Done through undocumented `uxtheme.dll` exports, resolved by ordinal (they
  have no public name): 135 `SetPreferredAppMode` (on 1809 the same ordinal is
  `AllowDarkModeForApp`), 136 `FlushMenuThemes`, 104
  `RefreshImmersiveColorPolicyState`. Loaded once (`OnceLock`) in `tray.rs`.
- The mode is AllowDark (1), not ForceDark (2): ForceDark would make the menu
  dark even in light mode. 1 also reads as TRUE on 1809.
- `follow_system_menu_theme()` runs once at startup in `app::run`, before the
  island window and the tray icon. `refresh_menu_theme()` runs from
  `on_system_theme_changed` (WM_SETTINGCHANGE "ImmersiveColorSet"), not gated
  on the Vale window theme, so the next menu matches without a restart.
- No `AllowDarkModeForWindow` (133) on the island hwnd: AllowDark + flush is
  enough for popup menus.
- If an ordinal is missing, that step is skipped silently (no log, no error)
  and the menu stays light.
- Side effect: other native menus in the process (for example the right-click
  menu of the Settings edit controls) follow the Windows mode too.

Related: [[logo]] (tray icon), [[settings-structure]] (window theme setting).
