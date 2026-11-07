---
tags: [spec, sticky-note, draft]
date: 2026-11-07
status: draft, brainstorming almost done (one open question)
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
- On the Notes card, the pin is `Hit::NoteSticky`, with the tooltip
  `StickyOffDesktopTip` "Lepas dari desktop".

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
- Tooltip: "Tempel ke desktop" when it is off, "Lepas dari desktop" when it
  is on.
- Add the same action to the card's right-click menu.

## Open question (ask the user before planning)

Add one of these, or neither?

- **A:** the sticky note list also shows notes that were taken off the desktop.
- **B:** after X, show a notice "Dilepas dari desktop · masih ada di Notes"
  with undo.
- **Neither:** ship Part 1 and Part 2 only.

## Must not change

- The X button keeps the note in Notes (an empty note is still deleted).
- Delete, roll up, layers and pinning behave as they do today, apart from
  Part 1.
