---
tags: [gotcha, island, menu]
date: 2026-11-14
---

# A native popup menu must hold the open panel

Right-clicking in the open panel (shelf or any page) opens the tray menu, and on a Notes card the card menu. Both are modal `TrackPopupMenu` calls: the pointer goes down to the menu, off the island, and in the hover modes the panel collapsed to the small notch under it.

The dropdowns (`menu_pop`, [[menu-pop]]) already held the panel with `Event::MenuOpen`; the two native menus did not. They now run inside `hold_panel_over_app` (`app.rs`), which sends `MenuOpen(true)` before and `MenuOpen(false)` after, so the panel stays while the menu is up and lingers `ACTIVITY_LINGER` after it (a latched panel waits for a click outside).

Any new native popup on the island must use the same wrapper. `MenuOpen(true)` never opens a closed panel, so a right-click on the compact notch is unchanged. Not verified live (no UI automation here): the unit test covers the wrapper with the real reducer.
