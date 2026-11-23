---
tags: [feature, sign, colour]
date: 2026-11-23
---

# Sign: ink colour of a placed signature or initials

Merged into `main` at cad4cba (2026-11-23). Spec `docs/superpowers/specs/2026-11-23-warna-tanda-tangan-design.md`. Part of [[tanda-tangan]]; text colours in [[sign-text-format]].

- **Controls:** a selected Signature or Initials gets the same three inks as text (black, blue, red, `style::Tint`) in its floating bar, before Smaller / Bigger. Only the selected item changes; saved signatures ([[sign-signature-store]]) and the maker (blue or black) stay as they are.
- **How:** a `Mark` is one colour plus alpha (drawn, typed and photo alike), so `Mark::recolored` sets every pixel's RGB and keeps alpha; the item gets a new `Arc<Mark>` through `Session::restyle` (one undo step, nothing when the ink is already that). `ItemKind`, writers and paint are untouched: PDF / JPG / PNG and the bitmap cache follow the new Arc.
- **Which swatch is ringed:** `Mark::tint` reads the most opaque pixel and matches the nearest `Tint` within 24 per channel, else none.
- **Paraf on every page:** recolouring one copy leaves the others. "Pasang di semua halaman" then treats a same-shape initials at the spot (`Mark::same_shape`: size and alpha equal) as the same paraf and gives it the selected colour instead of stacking a second copy; one undo step. The note still says "dipasang" after a recolour-only run (known minor).
