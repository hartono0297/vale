---
tags: [design, screenshot, editor]
date: 2026-11-20
---

# Screenshot rotate (editor + inline mode)

## Intent

The owner wants a screenshot to be rotatable while it is being taken and
edited: "ketika lagi screenshot, ada tambahan bisa di rotate". Picked from
mockups: **A** (two rotate buttons in the editor toolbar) **+ C** (a rotate
button in the "Langsung di layar" inline mode that hands off to the editor
already rotated).

## Goals

- Rotate the picture 90° left or right in the screenshot editor
  (`src/shot_edit/`), any number of times.
- Everything drawn on it (rect, line, arrow, pen, marker, pixelate, text)
  turns with the picture and stays editable (not baked).
- Text annotations stay upright: their box moves with the picture, the
  letters are not turned.
- One rotation = one undo step (Ctrl+Z / Ctrl+Y undo and redo it like any
  other edit).
- The editor window re-fits to the new shape (a portrait shot turned
  landscape gets a landscape window, no big empty margins).
- Copy, Save, "Salin otomatis" and the close flush all give the rotated
  picture with its annotations, exactly as seen.
- Inline mode gets a rotate button: it opens the editor with the selection
  rotated once to the right (the region on the frozen screen cannot turn in
  place).

## Non-goals

- Free-angle rotation (blank corners, blurred text; screenshots need 90°).
- Mirror / flip (option B, not picked).
- Rotating from Clipboard history, the shelf (rak berkas) or the screenshot
  notice: none of them can open the editor today; that is its own feature.
- Rotating the selection on the frozen screen in inline mode.

## Anti-goals (a failure even if it works)

- Rotating bakes the annotations into the pixels or loses their editability.
- Text ends up sideways or upside down.
- An undo leaves the picture and the annotations out of step (picture turned
  back, arrows still turned, or the reverse).
- The saved or copied file differs from what the canvas shows.
- The editor or inline mode behaves differently when rotate is never used
  (existing tests stay green, toolbar order of the existing items unchanged).

## Behaviour

### Editor (option A)

- Two icon buttons after Undo/Redo, in their own group (a separator before
  them): **Putar ke kiri** (counter-clockwise icon) and **Putar ke kanan**
  (clockwise icon). Tooltips: "Putar ke kiri (Ctrl+Shift+R)" / "Putar ke
  kanan (Ctrl+R)"; English "Rotate left (Ctrl+Shift+R)" / "Rotate right
  (Ctrl+R)".
- Keys: Ctrl+R = right, Ctrl+Shift+R = left. Same gates as the other Ctrl
  keys: ignored while a text EDIT is open or a drag is in progress, and while
  the close-confirm bar is up.
- A toolbar click while a text EDIT is open commits the text first (the same
  as every other toolbar button), then rotates.
- The selected annotation stays selected after a rotation.
- A rotation marks the document changed: the close confirm asks, and
  "Salin otomatis" copies the rotated result after its usual debounce.
- Four rotations in one direction give back the original: pixels exactly,
  annotation coordinates within 1e-3 px (f32 maths on fractional pointer
  coordinates; agent's call). Undo is exact anyway, it restores a snapshot.
- Text that a turn pushes partly off the picture (a wide label on a narrow
  picture) is left where the turn puts it; canvas and export clip it the same
  way and the user can drag it back. No clamping (agent's call: clamping
  breaks the four-turns rule and cannot fit text wider than the picture).
- Window after a rotation (or an undo/redo that changes the turn): a normal
  window re-fits with the open-time rules, even if the user had resized it
  (the old size was for the other shape). A maximized window stays maximized
  and only the scale is re-fitted to the client area (agent's call).

### Inline mode (option C)

- One icon button in the strip, after Undo/Redo, in its own group: **Putar
  dan buka di editor** (clockwise icon). Tooltip "Putar ke kanan, buka di
  editor (Ctrl+R)" / "Rotate right, open in editor (Ctrl+R)".
- Ctrl+R in inline mode does the same; Ctrl+Shift+R does it to the left.
  Same gates as the other inline keys (`inline::key_action`: not while typing
  text, dragging, or with the confirm bar up; no key repeat).
- It is "Buka di editor" plus one rotation: the editor opens on the cropped
  selection with the inline annotations, then applies the rotation as its
  first undo step, so Ctrl+Z in the editor shows the unrotated shot.

## How it is built (agent's call; the owner talked outcomes)

### Approaches considered

1. **Rotate for real, history carries the turn** (picked). The editor keeps
   the original picture and a quarter-turn count (0-3). The shown picture is
   the original turned by that count (cached; rebuilt only when the count
   changes). A rotation turns every annotation's coordinates in image px. Each
   undo snapshot holds the turn count with the annotation list, so picture and
   annotations always undo together. Export, auto-copy and save keep working
   unchanged because they already render "picture + annotations".
2. **Keep everything unturned, rotate only the view.** Draw and export through
   a rotation transform and map the pointer back. Rejected: every tool, hit
   test, handle, text EDIT placement and the export would need the transform;
   many places to get wrong.
3. **Bake annotations, then rotate pixels.** Rejected: loses editability
   (anti-goal).

### Shape of the change

- **Pure, tested:** rotate a `capture::Shot` (top-down BGRA) by a quarter
  turn left/right; turn a point and a rect for an image of size w x h;
  `Annotation` gets a rotate method over all seven variants (Rect and
  Pixelate turn their rect; Line, Arrow turn both ends; Pen, Marker turn
  every point; Text keeps its size and moves its top-left so its box's centre
  lands where the turned centre is, using `text_extent`).
- **Session:** a `turns` field, part of the undo/redo snapshots and of the
  "changed" comparison; a `rotate(dir)` that is one begin/end step (turns the
  doc and the count together) and bumps the revision so auto-copy sees it.
- **Editor window:** keeps the original `Arc<Shot>` and the current turned
  `Arc<Shot>`; after rotate/undo/redo, if the count changed: rebuild the
  turned picture, drop the D2D picture and the pixelate cache, re-fit the
  window (the same `open_scale`/`client_size` rules as at open, kept centred
  on where the window is now and inside the monitor's work area), recompute
  the view, `note_change` for auto-copy. Workers already holding the old
  `Arc` finish with it; the revision check keeps "clean" honest.
- **Toolbar:** `Hit::RotateLeft`, `Hit::RotateRight`, a fourth separator,
  two new glyphs in `shot_edit::icons`. The layout test is updated for the
  new group; existing items keep their order. `min_client_w` grows with it.
- **Inline:** `Btn::Rotate` in the strip after Undo/Redo, `InlineKey::RotateRight`
  / `RotateLeft`; `finish_inline` gets the rotation for the OpenEditor
  handoff and `shot_edit::window::open_with` takes an initial rotation it
  applies as the first step. Strip layout tests updated.
- **Strings:** new i18n keys, Indonesian and English.
- **Demos:** `--demo-shot-editor` and `--demo-snap inline` show the new
  buttons (no new demo flag).

## Left to the builder

- Icon drawing details (line art matching the existing glyphs).
- Exact cache strategy for the turned picture beyond "original + current".
- Whether the window re-fit animates (it should not; a plain `SetWindowPos`).
- Test names and helpers, following the existing `#[cfg(test)]` style.
