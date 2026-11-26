# Sticky note: resize from every side and corner, and while rolled up

## Why

A sticky note can only be resized from its bottom-right grip, and not at all
while rolled up. The user wants to resize it like an ordinary window.

Root cause: `paint::target_full` answers `Target::Grip` only for the 16 dip
square at the bottom-right, and returns `Target::Header` for everything
below the header strip of a rolled note. The resize drag
(`host::drag_move`, `snap::snap_resize`) only knows "top-left stays, size
grows to the bottom-right".

## Behaviour (agreed with the user)

- All four sides and all four corners resize. Sides change one dimension,
  corners both. The edge opposite the dragged one stays where it is.
- The pull zone is a 6 dip band inside the note's edges. A corner is where a
  band is within 16 dip (`GRIP`) of the perpendicular side. The existing
  bottom-right grip square stays a bottom-right corner.
- Header buttons, stack arrows and format-bar buttons win over a band.
- The cursor shows the direction: ↔ `IDC_SIZEWE`, ↕ `IDC_SIZENS`,
  ⤡ `IDC_SIZENWSE` (top-left, bottom-right), ⤢ `IDC_SIZENESW` (top-right,
  bottom-left), also while dragging.
- Snapping (grid, area edges, other notes with the 12 dip gap), Alt to turn
  snapping off, the size limits (`MIN_W`..`MAX_W`, `MIN_H`..`MAX_H`), the
  area clamp and a stack's `stack::MIN_W` all still apply to the moved edges.
- Rolled up: only the left and right bands resize, width only. The rolled
  strip stays `ROLLED_H` tall and the saved (unrolled) height does not
  change, so unrolling shows the old height.
- Lock (`prefs.lock`) still blocks every resize.

## Non-goals

- No Windows native sizing (`WM_NCHITTEST` / `HTLEFT`…): it would bypass the
  snap, the guides and the per-monitor DPI handling.
- No change to other widgets (list window, lyrics card).

## Approaches

1. Custom hit-test bands + an edge-aware resize in the existing drag code
   (chosen: keeps snapping, guides and DPI handling).
2. Native Windows sizing via `WM_NCHITTEST`: rejected, see above.
