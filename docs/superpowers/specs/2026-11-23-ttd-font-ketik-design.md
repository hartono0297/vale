# Sign maker: font choice for typed signatures

Date: 2026-11-23. Approved by the owner in the session (fonts and behaviour).

## Why

A colleague asked for what other signing apps have: in the Type tab, a row of
"Aa" tiles in different handwriting styles, so a typed signature does not
always look the same. Today the Type tab uses whatever Windows has (Ink Free,
then Segoe Script, then Segoe Print), so the look also differs between PCs.

## What

- **Four bundled fonts**, one per style, all SIL OFL 1.1, subset to Latin,
  no hinting (about 211 KB together):

  | Tile | Internal family | Source (google/fonts `06bfe0d6`) | Subset |
  |---|---|---|---|
  | 1 | Vale Sign Slant | Dancing Script, variable, instanced at wght 600 | ~57 KB |
  | 2 | Vale Sign Fine | Sacramento Regular | ~57 KB |
  | 3 | Vale Sign Print | Kalam Regular | ~41 KB |
  | 4 | Vale Sign Formal | Great Vibes Regular | ~56 KB |

- **Renamed families.** Dancing Script and Sacramento carry a Reserved Font
  Name, and a subset is a Modified Version under the OFL, so the embedded files
  must not use those names. All four get neutral `Vale Sign *` names (and
  neutral file names) for consistency. The UI never shows font names, only
  "Aa" tiles. THIRD-PARTY-NOTICES credits each source font by name ("based
  on"), which the OFL allows. No weight or style words (Thin, Light, Bold,
  Italic...) in the family names: DirectWrite strips them into the weight or
  style, so the family is no longer found by its name.
- **Type tab layout:** name field, then one row of four equal tiles ("Aa" drawn
  in each font, ink colour of the current swatch), then the preview. The
  chosen tile has the accent (orange) outline. The window grows by the row's
  height in the Type tab only (the maker already resizes per tab).
- **Keyboard:** the tiles are in the Tab order after the name field; Space or
  Enter picks the focused tile; the focus ring is the one the other maker
  buttons use.
- **Preview and result** use the chosen font; changing the font re-renders the
  preview at once, like changing the ink.
- **Default:** tile 1 (Slant). The last font used is remembered while Vale
  runs (process-wide static, not on disk), like the text style in
  [[sign-text-format]].
- **Ink Free / Segoe Script are no longer used** when the bundled fonts load.
  If the bundled collection cannot load (no resource, DirectWrite error), the
  old installed-font chain stays as the fallback, so typing never gives no
  mark.
- **Saved signatures are unaffected:** `signatures.bin` stores PNGs, not text
  or font names; no format change.
- **Licence files:** the four upstream OFL texts are appended to
  `assets/fonts/OFL.txt` by `scripts/make-fonts.ps1`, and listed in
  `THIRD-PARTY-NOTICES.md`.

## Non-goals

- Font names in the UI, saving the font choice to disk, more than four fonts,
  a font choice in the Draw or Photo tabs, or for the Teks item (that is
  [[sign-text-format]]'s Arial / Times / Courier).
- Changing the UI fonts: the eight existing files in `assets/fonts` must come
  out of `make-fonts.ps1` byte-identical.

## Constraints

- Exe size: release build must stay under the Tanda tangan module cap
  (6,373,068 bytes, see `docs/memory/exe-size.md`); expected about +215 KB on
  6,072,320.
- `src/ui/font_coverage.rs` keeps guarding the UI fonts unchanged; the sign
  fonts get their own coverage test (full ASCII and Latin-1, Latin Extended-A
  counts pinned).
- Swash capitals (Great Vibes) reach left of the text origin: the typed mark
  must not be clipped on any side for any of the four fonts.
- Two other sessions edit `src/sign/window/maker.rs` at the same time
  (Start over bug, colour of placed signatures): merge `main` before merging.

## Approaches

1. **Bundle four fonts as RCDATA in their own DirectWrite collection
   (chosen).** Same pipeline as the UI fonts (`make-fonts.ps1`, pinned SHA-256,
   fontTools subset), same in-memory loader; a separate collection keeps the UI
   collection and its tests untouched.
2. Keep installed fonts only (Ink Free, Segoe Script, Segoe Print, plus
   others Windows has): no size cost, but looks differ between PCs and there
   are not four distinct handwriting fonts on every Windows install. Rejected
   by the owner's brief (fonts must be bundled).
3. Bundle unsubset fonts: simpler script, about 1 MB for the four. Rejected on
   size.

## Left to the builder

RCDATA ids (next free: 211-214), the enum name and where it lives, tile height
and spacing (match the maker's 8 px rhythm), the left margin of the typed
bitmap, how the sign fonts appear in `VerifyPy`.
