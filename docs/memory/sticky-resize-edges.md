---
tags: [memory, widgets, sticky, resize]
date: 2026-11-26
---

# Sticky note resizes from every side and corner

Before this, a sticky note resized only from the 16 dip grip at its
bottom-right, and not at all while rolled up (the rolled hit test answered
`Header` for everything below the buttons).

- Hit test (`paint::target_full`): `Target::Resize(Edges)` replaces
  `Target::Grip`. A 6 dip band (`paint::EDGE`) inside each edge resizes; a band
  within `GRIP` (16 dip) of the perpendicular side is a corner; the old grip
  square stays bottom-right. Order: header buttons and stack arrows, then (rolled)
  left/right bands only, then format-bar buttons, then the bands, then the grip
  square, then `Header`.
- Cost of the bands: the strip's outer 6 dip no longer double-click-rolls a
  note or wheel-flips a stack; the body's left 6 dip is a band, but the text
  starts at `PAD_X` 12 so the caret is still placed from x 6.
- Snap (`snap::snap_resize_edges`): moves only the dragged edges, the opposite
  edge stays fixed even at a size limit or the area edge. Left/top snap to
  `area.x/y`, `o.right()/o.bottom() + GAP`, `o.x/y`; with no candidate the size
  is rounded to the grid. `snap_resize` is now a test-only wrapper.
- Drag (`host::drag_move`): `DragKind::Resize(Edges)`, pointer delta px / the
  display scale, from the shown dip rect. A side drag saves only its own
  dimension (a note shown clamped on a small monitor keeps its saved size on the
  other axis). Rolled: x, y and w only; `sticky.rect.h` and `view.h` stay. A
  stack resized from the left stops at `stack::MIN_W` with its right edge fixed.
- Cursors (`resize_cursor`): `IDC_SIZEWE`, `IDC_SIZENS`, `IDC_SIZENWSE`
  (top-left, bottom-right), `IDC_SIZENESW` (top-right, bottom-left), on hover
  and during the drag. Lock blocks every band.
- Known quirk: a tucked note resized from its screen-side edge is re-flushed to
  the screen edge on release (`finish_drag` uses `tuck::out_rect`), so the other
  edge ends up moved.

Related: [[widgets]], [[sticky-note-list]]
