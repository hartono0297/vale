---
tags: [decision, screenshot, editor, capture, rotate]
date: 2026-11-21
---

# Screenshot rotate: quarter turns in the editor and the inline mode

Spec: `docs/superpowers/specs/2026-11-20-screenshot-rotate-design.md`. The
owner asked for it as "ketika lagi screenshot, ada tambahan bisa di rotate".
Buttons in the editor toolbar (Putar ke kiri / kanan, Ctrl+Shift+R / Ctrl+R)
plus one button in the inline mode that opens the editor already turned.

- **Turns live in the undo snapshot.** `Session` keeps `turns` (0-3) next to
  the annotation list in every snapshot, so picture and annotations undo and
  redo together; `Session::rotate` is one begin/end step and bumps the
  revision (auto-copy and the close confirm see it). Annotations are turned
  for real (`Annotation::rotate`, image px), never baked, so they stay
  editable. Rejected: rotating only the view (every tool, hit test, handle,
  text EDIT and the export would need the transform).
- **Text stays upright.** `Annotation::rotate` moves a Text's top-left so its
  box centre lands on the turned centre and keeps its size; the letters are
  not turned.
- **No clamping of text.** A turn can leave a wide label hanging over the
  edge of a narrow picture. Left as is, by choice: clamping breaks "four turns
  give the original" and cannot fit text wider than the picture. Canvas and
  export clip it the same way; the user drags it back.
- **Base is the picture as shown; the original is kept.** The window holds
  `original` (never touched) and `base` (what is shown). `sync_picture` turns
  by the delta `(turns + 4 - shown_turns) % 4` from `base`, one `rotate_shot`
  per quarter (a left turn is one Left, not three Rights); `turns == 0`
  reuses `original` with an `Arc` clone, no copy. Why: one left turn used to
  cost a clone plus three right turns (490 ms on 7680x4320) and undo to 0
  copied the original. Rotations are exact permutations, so the result is
  pixel-identical to turning from `original`.
- **Window re-fit** (`refit_window`, `refit_rect`, `outer_size`): after a turn
  (or an undo/redo that changes it) a normal window re-fits with the open-time
  rules even if the user had resized it, because the old size was for the
  other shape. It keeps its **top-left corner**, not its centre: the rotate
  buttons are at the left of the toolbar, so they stay under the pointer for
  a second click. Then it is clamped into the work area (negative monitors and
  oversized shots covered by tests). A maximized window stays maximized and
  only the scale is re-fitted.
- **Restore and the move loop.** Restoring from maximized re-fits too
  (`refit_on_restore`, `WM_SIZE` `SIZE_RESTORED`), but dragging a maximized
  window by its title bar restores it inside the modal move loop. The re-fit
  waits for `WM_EXITSIZEMOVE` (the flag stays set between `WM_ENTERSIZEMOVE`
  and it). A Win+Left/Right snap from maximized has no move loop and may
  still re-fit; accepted.
- **Key repeat is ignored for the rotate keys only.** Holding Ctrl+R turns
  once (`WM_KEYDOWN` lParam bit 30); undo and redo keep repeating as before.
  Without it a held key did a full turn and resize at the repeat rate.
- **Inline hands off, it does not turn in place.** The selection on the frozen
  screen cannot turn, so `Btn::Rotate` / `InlineKey::Rotate` finish with
  `FinishKind::OpenEditorTurned`: the editor opens on the crop with the inline
  annotations, `window::open_turned` applies the turn as its first undo step
  (Ctrl+Z shows the unturned shot). `FinishKind::keeps_picture()` (Save,
  OpenEditor, OpenEditorTurned) is the one list of finishes that crop the
  picture; an empty crop turns any of them into a Cancel. The turn runs before
  `ShowWindow` (else the editor flashes in the unturned shape) and before the
  first auto-copy.
- **Not done on purpose:** no flip (not picked), no free angle (blank corners
  and blurred text; screenshots need 90 degrees), no rotate from Clipboard
  history, the shelf or the screenshot notice (none can open the editor).

Related: [[screenshot-snap]], [[shot-editor-paint-order]]

Listed in [[index]].
