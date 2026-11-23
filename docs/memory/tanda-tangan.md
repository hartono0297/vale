---
tags: [feature, sign, shelf, pdf]
date: 2026-11-23
---

# Tanda tangan: sign and lightly edit documents

Merged into `main` at 4211b36 (2026-11-21). Spec `docs/superpowers/specs/2026-11-20-tanda-tangan-design.md`, mockup `docs/design/tanda-tangan.html`.

- **Entry:** right-click a shelf card of pdf, png, jpg, jpeg, doc(x), xls(x), ppt(x) gives "Tanda tangani" (other kinds: no item). See [[shelf]].
- **Window:** its own window (`src/sign/window`), not a panel page. Page rail, one large page, tools on top, output name and Batal / Simpan at the bottom. Dark and light palettes through `window_for`.
- **Can place:** Tanda tangan, Paraf (with "Pasang di semua halaman"), Tanggal, Teks, Centang; select, move, resize, delete, undo / redo. **Page edits:** rotate by 90 degrees, delete a page (never the last). Signature maker (draw, type, photo): [[sign-signature-store]].
- **Files:** PDF opened directly (password asked; saved with the same protection, see [[sign-pdf-writer]]); images as one page, saved as the same format or PDF; Office converted to PDF first ([[sign-office-conversion]]). Image size rule: [[sign-image-cap]]. Coordinates: [[sign-quarter-convention]].
- **Output:** `<name> (ditandatangani).<ext>` next to the source (English `(signed)`), numbered when taken, never overwrites; folder not writable: `Documents\Vale\Ditandatangani`. After saving: a notch notice "Ditandatangani, file, klik untuk buka" (click reveals it) and the new file joins the shelf.
- **Demo:** `--demo-sign` (debug builds only) opens the window on a generated PDF; it ends with `TerminateProcess` ([[sign-amd-pdf-teardown]]).
- **Size:** release 6,028,800 B, +1,018,880 over the 5,009,920 baseline; the owner raised the cap to 1.3 MB for the module ([[exe-size]]).
- **Not in it:** PKI or certified signature (TTE via a PSrE needs online identity checks and breaks the local-only promise; the UI never says "certified" or "legally valid"), form filling, editing existing text, reorder, merge, OCR, a notch drop zone, Explorer context menu. No warning for a PDF that already carries a certifying signature (only delete / rotate are named).
- **Accepted by ruling:** text, dates and checks can be black, blue or red, default black ([[sign-text-format]]); placed signatures and initials too, from their bar ([[sign-mark-ink]]); quitting Vale discards unsaved signing work (like the screenshot editor).
- **Gotcha, buttons on the drawing pad (2026-11-23):** "Start over" sits inside the maker's pad, so a stroke may only start where `MLayout::hit` says `MHit::Pad` (`starts_stroke` in `maker.rs`), never on `pad.contains`. Before this fix a click on it drew a dot and never cleared. A pen or touch tap there falls to `DefWindowProcW` and comes back as a mouse click. See [[sign-signature-store]].
- **Hand checks still open for the owner:** a Sign pick from the shelf menu opens the window in the real app; WPS and LibreOffice conversion (not installed here); an exit check on an AMD GPU after signing a PDF; Excel and PowerPoint password-protected files (only Word was tested live).
