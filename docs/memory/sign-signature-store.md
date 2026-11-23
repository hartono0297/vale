---
tags: [decision, sign, privacy, dpapi]
date: 2026-11-23
---

# Sign: saved signatures

- Up to 4 signatures and 4 initials, each deletable, kept in `%APPDATA%\Vale\signatures.bin`, PNGs with alpha, sealed with DPAPI through `src/dpapi.rs` (same as [[clipboard-history]]): another Windows account or a plain file read gets nothing.
- The in-memory store is cleared when the last signing window closes.
- `--demo-sign` never writes the file.
- A failed write can leave `signatures.bin.tmp` behind (known minor).
- Typed signatures use one of four bundled handwriting fonts ([[sign-typed-fonts]]; before 2026-11-23 Ink Free, then Segoe Script); the store keeps only the PNG, never the font. Photo signatures use a luminance threshold to alpha and crop to the ink.

Part of [[tanda-tangan]]; photo size limits in [[sign-image-cap]].
