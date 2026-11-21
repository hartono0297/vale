---
tags: [design, screenshot, capture, inline, rotate]
date: 2026-11-21
---

# Inline capture: rotate in place (no editor)

## Intent

The owner, testing the rotate feature: "untuk rotate image screenshot, bisa
ga tanpa masuk ke halaman editor?" Today the inline mode's ("Langsung di
layar") Rotate button and Ctrl+R / Ctrl+Shift+R open the editor window
already turned. The owner wants to turn the shot without leaving the frozen
screen. Asked for the best way, they said "atur aja terbaiknya gimana", so
every call below is the agent's.

## Goals

- In inline mode, the strip's Rotate button and Ctrl+R turn the picked
  region 90° right **in place**; Ctrl+Shift+R turns it left. No editor opens.
- The turned picture shows on the frozen screen as a "lifted" card: the crop
  of the selection, turned, centred on the selection's centre, with the
  annotations turned with it. Everything else stays dimmed.
- Drawing keeps working on the turned card (all tools, select, move,
  resize, delete, text). Undo/redo work; a turn is one undo step, so Ctrl+Z
  turns it back.
- Copy, Save (and its auto-copy) give the turned picture with its
  annotations, exactly as shown.
- "Buka di editor" from a turned state opens the editor on the turned
  picture with the turned annotations.
- Four turns give back the untouched selection state.

## Non-goals

- Rotate on the Clipboard history card, the shelf or the screenshot notice
  (possible later step; not now).
- Free angle, flip.
- Changing the editor window's rotate.

## Anti-goals

- The output (copy/save/editor) differs from what the card shows.
- Annotations drift from the picture after a turn, an undo or a redo.
- The selection handles still react while turned, moving a crop the card no
  longer shows.
- Inline mode changes when rotate is never used.

## Behaviour (agent's calls)

- **Card.** Size = selection turned (w and h swap), at display scale
  `s = min(1, fit into the selection's monitor minus a 48 dip margin on each
  side)`; centred on the selection's centre, then moved (not shrunk further)
  to lie fully on that monitor. The output is always full size; only the
  display shrinks.
- **While turned (turns != 0):** the selection handles are hidden and the
  selection cannot be moved, resized or re-drawn; a press outside the card
  does nothing. The size label shows the turned size ("400 × 600"). The
  border goes around the card. The strip and bar are placed around the card
  with the same rules as around a selection. Turning back to 0 (by turns or
  undo) restores the normal selection editing.
- **Esc** with only a turn (no annotations) counts as a change: the confirm
  bar asks, like with annotations.
- **Editor handoff** from a turned state: the editor gets the turned picture
  and turned annotations as its starting point (it can turn further; the
  inline undo history is not carried over, as today).
- **Rotate button tooltip:** "Putar ke kanan (Ctrl+R)" / "Rotate right
  (Ctrl+R)" (the editor's string). The old "open in editor" wording and the
  `OpenEditorTurned` finish go away.

## How it is built (agent's call)

Approaches considered:

1. **Coordinate spaces by turn state (picked).** With `turns == 0` the
   annotations live in frame px as today. A turn from 0 moves them into the
   crop's own px (minus the selection's top-left) and turns them; further
   turns stay in turned-crop px; a turn back to 0 turns them and moves them
   back into frame px. Each of these is one undo step that changes doc and
   turns together (a new `Session` method that shifts, turns, shifts), so an
   undo always restores a matching (doc, turns) pair. Drawing in the turned
   state uses a `View { scale: s, ox, oy }` for the card and maps the
   pointer back through it; pixelate reads from the turned crop.
2. Keep annotations in frame px forever and rotate only at draw/export.
   Rejected: every tool, hit test and handle would need the inverse
   rotation.
3. Close the overlay and show a small viewer window. Rejected: that is the
   editor again, which the owner wants to avoid.

Shape:

- `shot_edit::Session::rotate_shifted(turn, size, pre, post)`: move every
  annotation by `pre`, turn with `size`, move by `post`, one step; existing
  `rotate` = `rotate_shifted(turn, size, (0,0), (0,0))`.
- Pure helpers in `capture/inline.rs`: the card rect and scale for a
  selection, turned size, monitor and ui; the pointer → turned-crop px
  mapping; the shift amounts for a turn from/to 0.
- `capture/overlay/edit.rs`: a cached lifted picture (turned crop `Arc<Shot>`
  + its D2D bitmap, rebuilt when `turns` changes), the turned paint path,
  pointer mapping, the selection gates, relayout around the card, the
  Rotate button/keys turning in place.
- `capture/overlay.rs`: `inline_picture()` returns the turned crop and the
  doc as-is when turned; the editor handoff uses it.

## Left to the builder

- Text EDIT sizing/placement at display scale < 1.
- Exact layer order changes in `PAINT_ORDER` for the card.
- Cache invalidation details beyond "rebuild when turns changes".
