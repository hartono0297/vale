---
tags: [feature, sign, fonts, directwrite, licence]
date: 2026-11-23
---

# Sign: four bundled handwriting fonts for typed signatures

Merged into `main` at f8bb2db (2026-11-23). Spec `docs/superpowers/specs/2026-11-23-ttd-font-ketik-design.md`, plan `docs/superpowers/plans/2026-11-23-ttd-font-ketik.md`. Part of [[tanda-tangan]].

- **What:** the maker's Type tab shows a row of four "Aa" tiles between the name field and the preview; the chosen tile has the accent outline, the preview and the made mark use its font. Tab order: tabs, name field, the four tiles, ink. The window is 52 px taller in the Type tab only.
- **Fonts** (RCDATA 211-214, built by `scripts/make-fonts.ps1`, see [[studio-font-sources]]): Dancing Script (wght 600) as "Vale Sign Slant", Sacramento as "Vale Sign Fine", Kalam as "Vale Sign Print", Great Vibes as "Vale Sign Formal". Latin subset, no hinting, about 212 KB together. Files keep the names `SignSlant/Thin/Print/Formal.ttf`.
- **Why renamed:** a subset is a Modified Version under the OFL, and Dancing Script and Sacramento carry a Reserved Font Name, so the embedded families must not use those names. Name ID 0 (copyright) stays as upstream wrote it; `VerifyPy` fails if a family name record contains a Reserved Font Name. THIRD-PARTY-NOTICES credits each one with "based on".
- **Gotcha: no weight or style words in a family name.** DirectWrite parsed "Vale Sign Thin" as family "Vale Sign" at weight 100, so the font was not found under its own name. The thin one is "Vale Sign Fine".
- **Code:** `ui::fonts::sign_collection` is its own in-memory collection (the UI `Embedded` collection is untouched), cached once like `embedded()`; it works from `raster::offscreen`'s factory because `DWriteCreateFactory(SHARED)` hands out the same factory. `src/sign/hand.rs`: `HandFont`, `ALL` (tile order), `format` (bundled family, else Ink Free, Segoe Script, Segoe Print installed, else nothing), last-used font in a process-wide `Mutex` (default Slant, never on disk; tests take `hand::TEST_LOCK`). `ink::from_text(name, ink, font)`.
- **Typed canvas:** 96 px per character plus `TEXT_PX` (64) left and right, `TEXT_PX / 2` above, height `3 * TEXT_PX`, then cropped. 80 px per character clipped 40 wide caps in Sacramento and Great Vibes; `typed_marks_are_never_clipped` holds 40 × "M" and "W" in all four.
- **Tile hover** is BASELINE at 0.5 over the paper tile (the same in both themes); `p.hover` read mid-grey in dark and invisible in light.
- **Unchanged:** saved signatures are PNGs in `signatures.bin` ([[sign-signature-store]]), so old ones still load and carry no font.
- **Size:** +218,624 B ([[exe-size]]).
- **Not done:** font names in the UI, more fonts, saving the choice to disk, a font for the Draw or Photo tabs. With the bundled collection missing, all four tiles show the same installed font (accepted fallback).
