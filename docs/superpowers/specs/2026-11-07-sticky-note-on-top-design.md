---
tags: [spec, sticky-note, draft]
date: 2026-11-07
status: approved, brainstorming done (2026-11-07)
---

# Sticky note: new notes on top, desktop icon on the Notes card

Brainstormed with the user in the main session on 2026-11-07. Not started yet.
Related notes: [[widgets]], [[sticky-note-list]].

## How it works today (for context)

- A sticky note is a note in `notes.json` plus a `sticky` part. A note and a
  sticky note are the same data.
- The default layer is `WidgetLayer::Desktop`: `HWND_BOTTOM`, forced in
  `WM_WINDOWPOSCHANGING`. A new sticky note therefore opens behind the other
  windows, and the user has to minimise apps to see it.
- Pinned notes and notes on the "Di atas jendela" layer are topmost.
- The X button (`Btn::Close`, `src/widgets/host.rs`) sends
  `WidgetEvent::Unstick`. The note stays in Notes, and an empty note is
  deleted. Deleting is two clicks in the context menu, or from the list with a
  5 s undo.
- Roll up (`ROLLED_H`, 64 dip) shrinks the note to its strip plus one line.
- The sticky note list shows only notes that are currently sticky.
- On the Notes card, the pin is `Hit::NoteSticky` (`Icon::Pin`, accent while
  on the desktop). It has no tooltip: the notch panel tracks no hover at all
  ("the panel tracks no hover over it", `render_panel.rs`). A right-click on
  the panel opens the tray menu; the Notes card has no menu of its own.
  `StickyOffDesktopTip` is only the item of the note's own right-click menu.
- `WidgetHost::bring_front` (the list's show) already lifts a note
  (`Win::lifted`, topmost) until it loses the keyboard (`end_editing`).

## Part 1: a new note opens on top until the user clicks away

- Every new sticky note opens temporarily on top of other windows. This covers
  every way of creating one, including from the Notes card, and opening a note
  from the sticky note list.
- It stays on top until the user activates another window: another app,
  another note, or a click outside the notch panel. It then drops back to its
  layer (the Desktop layer by default).
- Pinning it while it is on top keeps it on top.
- No change for the "Di atas jendela" layer, pinned notes or other notes.

## Part 2: a "tempel ke desktop" icon on the Notes card

- Replace the pin icon on the Notes card (`Hit::NoteSticky`) with a
  sticky-paper icon, drawn in the accent colour while the note is on the
  desktop.
- No tooltip (decided 2026-11-07 while planning): the panel tracks no hover, and
  hover tracking for one icon is out of proportion. The words live in the
  right-click menu instead.
- A right-click on a Notes card (its body or the icon) opens a small menu with
  one item: "Tempel ke desktop" / "Stick to desktop" when the note is off,
  "Lepas dari desktop" / "Unstick from desktop" when it is on. It does what a
  click on the icon does. A right-click anywhere else still opens the tray
  menu.

## How Part 1 ends (decided while planning)

- The lift is the existing `Win::lifted` flag. It no longer ends when the note
  loses the keyboard (Esc keeps it on top). It ends when the note's window is
  deactivated, unless the window being activated is one of the note's own
  popovers, its dialog, or the island. Clicks inside the notch panel do not
  activate anything, so they never end it.
- When the island itself was active (its text inputs) and loses activation to
  a window that is not a lifted note, every lift ends.
- A note from the Notes card has text, so it does not get the keyboard. It is
  made the foreground window, so the next click on any other window ends the
  lift.
- A note is lifted only if it is the foreground window or has the keyboard
  after the attempt; otherwise nothing could end the lift, so it stays on its
  layer (as today when Windows refuses the foreground).

## Decided: neither A nor B

The open question was whether to add either of these:

- **A:** the sticky note list also shows notes that were taken off the desktop.
- **B:** after X, show a notice "Dilepas dari desktop · masih ada di Notes"
  with undo.

On 2026-11-07 the user left the choice to us ("u atur yang terbaiknya gimana,
intinya new note dia langsung paling atas"). The answer is **neither**: ship
Part 1 and Part 2 only. Part 1 is what the user cares about. Part 2 already
gives an unstuck note a way back to the desktop from the Notes card. A and B
would add surface for a problem nobody has reported yet.

## Must not change

- The X button keeps the note in Notes (an empty note is still deleted).
- Delete, roll up, layers and pinning behave as they do today, apart from
  Part 1.
