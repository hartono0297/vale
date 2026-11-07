---
tags: [widgets, sticky-notes, win32, z-order, decision, gotcha]
date: 2026-11-07
---

# A new sticky note opens on top (the lift)

Spec: `docs/superpowers/specs/2026-11-07-sticky-note-on-top-design.md`. Related:
[[widgets]], [[sticky-note-list]].

Problem: the Desktop layer keeps notes at `HWND_BOTTOM`, so a new note opened
behind every window. Now every new note is **lifted** (`Win::lifted` in
`widgets/host.rs`), and so is a note the sticky note list shows. That covers
the hotkey, the tray, the gallery, the header (+), templates, the Notes card
icon and its menu. A lifted note sits over ordinary windows until the user
activates another window, then goes back to its layer.

## Rules

- `lift(id)` (card icon: foreground, no EDIT) and `bring_front_at(id, caret)`
  (keyboard plus lift). A note is lifted only if afterwards it is the
  foreground window or has the EDIT. Otherwise nothing could end the lift.
- The lift is exclusive: lifting one note ends every other lift.
- It ends on the note's `WM_ACTIVATE` / `WA_INACTIVE` unless the window being
  activated is its own popover, its image dialog or the island
  (`lift_survives`). Esc (end of editing) keeps it.
- A colour or reminder popover that closes because the user clicked elsewhere
  ends it (`pop_closed(note, Some(to))`), and so does the image picker
  returning while another window is foreground. Otherwise the note, already
  inactive, never sees another `WM_ACTIVATE` and stays on top forever.
- When the island itself was active (its text inputs, the tray or card menu)
  and loses activation to a window that is not a lifted note, every lift ends
  (`island_deactivated`).
- No change for pinned notes or the "Di atas jendela" layer: `lift` returns
  false without taking the foreground there.
- A note lifted for no other reason goes directly **below the island** when the
  island is a visible topmost window (`z_place`), so the open panel is not
  covered. The island's own z-order is never touched (stay-behind mode).
  Clicking the lifted note later still raises it above the island (Windows
  raises an activated topmost window); accepted.

## Gotchas

- The island's `WM_ACTIVATE` often arrives synchronously inside
  `SetForegroundWindow` while `App` is borrowed (for example a card menu choice
  lifts a note from inside `with_app`). `with_app` then returns `None`, so the
  deactivation is stored in `DEFERRED_ISLAND_INACTIVE` and replayed by
  `run_deferred`, like the deferred CLI and search keys. A replay is checked
  against the lift state at that time; an early end is the safe failure.
- The Notes card menu goes through `apply_now(Event::Click(Hit::NoteSticky))`,
  not `sticky_toggle` directly, or the panel does not redraw (the panel tracks
  no hover, so nothing else would).

## Notes card (Part 2)

`Icon::StickyNote` (paper with a folded corner) replaces the pin, in the accent
colour while on the desktop. No tooltip, because the panel tracks no hover. A
right-click on a card (`note_card_menu_target`) opens a one-item native menu
(`tray::note_card_menu`, outside the app borrow, `MENU_OPEN`) with "Tempel ke
desktop" / "Lepas dari desktop" (`StickyOnDesktopTip` / `StickyOffDesktopTip`).
A right-click elsewhere opens the tray menu as before. Options A (the list shows
unstuck notes) and B (an undo notice after X) were declined.

Not verified live (tests only): the real z-order against the island, the panel
redraw after the menu, and the deferred island deactivation.
