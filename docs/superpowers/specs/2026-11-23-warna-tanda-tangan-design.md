# Ink colour for placed signatures and initials (short design)

Date: 2026-11-23. Request (owner): "bikin bisa ubah warna juga pada signature nya".
The owner left the open choices to the builder ("u atur terbaiknya").

## Intent

A signature or initials already placed on the page can change ink from its
floating bar (today: Smaller, Bigger, + Date / Pasang di semua halaman,
Remove), without making the signature again.

## Behaviour

- The bar of a selected Signature or Initials gets the same three swatches
  the Text, Date and Check bars have: black, blue, red (`style::Tint`), before
  Smaller / Bigger. The swatch of the mark's current ink has the accent ring.
- Picking a swatch recolours only the selected item: every pixel takes the
  new RGB, alpha stays, so the shape and anti-aliased edges are unchanged.
  Works the same for drawn, typed and photo marks (all are one colour plus
  alpha).
- One undo step (`Session::restyle`). Picking the colour it already has does
  nothing and adds no undo step.
- Saved output (PDF, JPG, PNG) uses the new colour, because writers read the
  item's mark.
- Initials placed on every page: only the selected copy changes (same as
  Smaller / Bigger). Builder's call, for predictability. Pressing "Pasang di
  semua halaman" on it afterwards gives the copies at that spot the selected
  colour (same shape, other ink counts as the same paraf) instead of stacking
  a second copy on top; pages without one get a copy as before. One undo step.

## Not changing

- Saved signatures in `signatures.bin` and the signature maker (still blue or
  black ink).
- No "last used ink" for marks; no new colours; text, date and check colour
  behaviour stays as it is.

## Approaches

1. **Recolour the mark bitmap (chosen).** `Mark` gains `tint()` (the ink of
   its most opaque pixel, matched to the nearest `Tint` within a small
   tolerance) and `recolored(Tint)`. The item keeps `ItemKind::Signature(Arc<Mark>)`;
   a recolour puts a new `Arc<Mark>` in it. No change to `ItemKind`, the
   writers, the paint cache or the store, so it stays clear of the files the
   parallel sessions (`ttd-font-ketik`, `ttd-start-over`) edit.
2. Store a `Tint` beside the mark in `ItemKind` and tint at draw / write
   time. Rejected: touches every match on `ItemKind` (session, paint, raster,
   pdf_write, image_out) for no user-visible gain.
3. Keep the original strokes and re-render. Rejected: typed and photo marks
   have no strokes, and placed items only hold the bitmap.

## Done when

- A test that fails first, then passes: `Mark::recolored` / `Mark::tint`, and
  the bar and recolour wiring in `window.rs`.
- `cargo test --locked` green on `main` (except the Z: drive test that always
  fails on this PC).
