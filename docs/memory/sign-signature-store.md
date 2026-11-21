---
tags: [decision, sign, privacy, dpapi]
date: 2026-11-21
---

# Sign: saved signatures

- Up to 4 signatures and 4 initials, each deletable, kept in `%APPDATA%\Vale\signatures.bin`, PNGs with alpha, sealed with DPAPI through `src/dpapi.rs` (same as [[clipboard-history]]): another Windows account or a plain file read gets nothing.
- The in-memory store is cleared when the last signing window closes.
- `--demo-sign` never writes the file.
- A failed write can leave `signatures.bin.tmp` behind (known minor).
- Typed signatures use Ink Free, then Segoe Script (installed with Windows, nothing embedded); photo signatures use a luminance threshold to alpha and crop to the ink.

Part of [[tanda-tangan]]; photo size limits in [[sign-image-cap]].
