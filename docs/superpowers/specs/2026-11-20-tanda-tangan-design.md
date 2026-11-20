# Tanda tangan: sign and lightly edit documents

Date: 2026-11-20. Owner request: "gw mau tambahin fitur ... Modul FileEdit + esign !
coba kita brainstroming dlu". Asked which files and what for: "file apapun, menurut u
terbaiknya gimana, kasih gw visualnya dlu". After the mockup: "Pas, lanjut". After the
builder check: same password on protected PDFs, warn but allow on "no changes" PDFs,
and the office suite choice "u atur terbaiknya gimana".

Mockup (open in a browser): `docs/design/tanda-tangan.html`. It shows the shelf menu,
the signing window, the signature maker, the notch notice, the file table and the
three signature levels. Names and document content in it are examples.

## Intent

The owner wants to sign documents on the PC without a web service: open a file, put a
signature, initials, a date or a short text on it, fix a page that is sideways or not
needed, and get a new file to send on. "Any file" in practice means PDFs, images and
Office documents. Everything stays local, like the rest of Vale (the README promises no
data leaves the PC).

## Goals

1. **Entry from the file shelf.** A shelf card of a signable kind gets **Tanda tangani**
   in its right-click menu. Files reach the shelf the ways they do today (drag onto the
   notch, phone link, drag from Explorer).
2. **A signing window** of its own (like the screenshot editor, not a panel page):
   page rail on the left, one page large in the middle, tools on top, output name and
   Batal / Simpan at the bottom.
3. **Items to place on a page:** Tanda tangan (signature image), Paraf (initials image,
   with "Pasang di semua halaman"), Tanggal (today's date as text), Teks (free text),
   Centang (check mark). Each item can be selected, moved, resized, deleted. Undo / redo.
4. **Page edits:** rotate a page by 90°, delete a page (never the last one).
5. **Signature maker:** make a signature or initials by drawing (mouse, touchpad, pen),
   typing (name in a handwriting face) or from a photo (white background removed).
   Ink blue or black. Optionally saved on this PC, encrypted to the Windows account
   (DPAPI, like clipboard history), up to 4 signatures and 4 initials, each deletable.
6. **Files:**

   | Input | Behaviour | Output |
   |---|---|---|
   | PDF | Opened directly. Password-protected: ask for the password. | New PDF, protected the same way as the source |
   | Image (JPG, PNG) | Opened as one page | Same format as the input by default, or PDF (choice at save) |
   | Word, Excel, PowerPoint (.doc/.docx, .xls/.xlsx, .ppt/.pptx) | Converted to PDF first with an office suite installed on the PC, offline: Microsoft Office, else WPS Office, else LibreOffice. None of them: a clear message to save it as PDF from the app first. | New PDF |
   | Anything else | No **Tanda tangani** in the menu | |

7. **Protected PDFs (owner's answers).** A PDF with an open password is saved with
   the same password and the same protection, never silently unprotected: if the
   file's encryption cannot be kept, saving is refused with a message. A PDF whose
   author marked it "no changes" (permission flags, opens without a password) gets a
   warning in the window ("Pembuat dokumen ini tidak mengizinkan perubahan"), and
   signing and saving stay allowed.
8. **Saving never overwrites.** The output is `<name> (ditandatangani).<ext>` in the
   source file's folder (English UI: `<name> (signed).<ext>`), numbered if taken. If
   that folder is not writable, fall back to `Documents\Vale\Ditandatangani`.
9. **After saving:** a notch notice "Ditandatangani · <file> · klik untuk buka" (click
   reveals the file in Explorer, like the screenshot notice), and the new file is added
   to the shelf.
10. Indonesian and English strings; dark and light window palettes (`window_for`).

## Non-goals (first version)

- A digital (PKI) signature. Planned as a possible later phase: a self-made certificate
  seal that shows tampering, displayed by Adobe as "validity unknown".
- A certified electronic signature (TTE tersertifikasi, PP 71/2019). It needs a PSrE
  (Privy, Peruri, VIDA, BSrE) with online identity checks, which breaks the local-only
  promise. The UI never calls the result "certified" or "legally valid".
- Editing the existing text of a document, filling AcroForm fields, merging or
  splitting files, reordering pages, compressing, OCR.
- A second drop zone on the notch ("Jatuhkan untuk tanda tangan"): maybe later.
- Opening from Explorer's context menu (it would need registry entries; Vale writes
  none today).

## Anti-goals

- The source file is changed or lost in any way.
- Anything goes over the network, including the Office conversion.
- A cost while the window is closed: no timer, thread or loaded engine at idle
  ([[bata-idle-power-budget]]).
- A saved signature is readable by another Windows account or as a plain file.

## Constraints

- Rust 2024, Win32 + Direct2D, `windows` 0.62; Windows 11 only.
- `vale.exe` size is guarded per feature ([[exe-size]]). Expected cost: about
  0.3 to 0.7 MB for the PDF writer. Measure it; if the module adds more than 1 MB,
  stop and report before going on.
- Do not touch `src/shot_edit/` (another session is changing it). Copy patterns from
  it, do not share its document model.
- WPS and LibreOffice paths can only be verified live on a PC that has them; if not,
  they ship with unit-tested detection and are listed as hand checks for the owner.
- Tests never show windows ([[test-spawns-no-window]]); never run `cargo fmt` on the
  repo ([[repo-not-rustfmt-clean]]).

## Approaches considered

| | Approach | Trade-off | Verdict |
|---|---|---|---|
| A | **Own window**, opened from the shelf menu | Room to place a signature exactly on an A4 page; a new window module | **Picked** (owner: "Pas, lanjut") |
| B | A sub-view in the shelf panel, like the image preview | No new window, but the panel is too small to place items precisely on a page | Rejected |
| C | Sign straight on the screen, like the screenshot "Langsung di layar" | Works only for what is visible; no pages, no file output | Rejected |

Rendering: **Windows.Data.Pdf** (built into Windows, 0 KB) was chosen over pdfium
(best fidelity, but a 5 to 6 MB download), hayro (pure Rust, experimental) and MuPDF
(AGPL). Writing: **lopdf** (reads and changes existing PDFs, incremental save) over
pdf-writer and printpdf (writers only).

## How it is built (decided)

- **Render:** WinRT `Windows.Data.Pdf` (`windows` feature `Data_Pdf`) on a worker
  thread: load from bytes (`InMemoryRandomAccessStream`), render a page to a PNG stream
  at the size needed, decode with WIC into pixels, then a D2D bitmap on the UI thread.
  Thumbnails for the rail are small renders done lazily. Images use WIC directly.
- **Write:** `lopdf` with default features off (no `image` crate), plus whatever its
  encryption support needs to write new objects into an encrypted PDF with the
  source's own handler (RC4, AES-128, AES-256). Check which schemes the chosen lopdf
  version can write before planning; schemes it cannot keep fall under goal 7's
  refusal. For each page with
  items, append a content stream: images as Image XObjects (Flate RGB + SMask for
  alpha), text with the standard Helvetica font in WinAnsiEncoding, the check mark as
  a vector path. Save path (builder-check call): a **full rewrite whenever a page was
  deleted**, so the deleted page is really gone from the file; otherwise an
  **incremental update**, which keeps the original bytes and leaves an existing
  digital signature from another party verifiable. If the PDF already carries a
  digital signature and the save will break it (page deleted or rotated), the window
  says so before saving.
- **Coordinates (builder-check call):** items are stored in PDF points in the page's
  unrotated space, plus the page's rotation as a transform. Rotating a page turns the
  items with the content, so a signature stays on the line it was put on. A new item
  is placed upright in the page as it is shown at that moment. The render scale never
  moves items.
- **Office conversion** (suite order is the builder-check call, the owner left it to
  us): Microsoft Office, then WPS Office (common in Indonesia; its Writer /
  Spreadsheets / Presentation expose Word-compatible COM objects under their own
  ProgIDs, so the same code path with other ProgIDs, best effort), then LibreOffice
  (`soffice --headless --convert-to pdf`, a hidden child process with a timeout).
  Microsoft Office details: COM automation with late binding on a worker thread: Word
  `Documents.Open` read-only then `ExportAsFixedFormat` (PDF), Excel
  `Workbook.ExportAsFixedFormat`, PowerPoint `Presentation.SaveAs` as PDF. Invisible,
  alerts off, macros forced off, a timeout; the PDF goes to `%TEMP%\Vale\sign\` and is
  removed when the window closes (and swept at startup). Vale itself sends nothing;
  what Office does on its own (its telemetry, OneDrive fetching a cloud-only file) is
  outside Vale and is not claimed as verified.
- **Typed signature** uses a handwriting font already installed with Windows (Ink Free,
  then Segoe Script), so nothing is embedded.
- **Photo signature:** WIC decode, luminance threshold to alpha, crop to the ink.
- **Saved signatures:** one DPAPI-sealed file under `%APPDATA%\Vale\` through the
  existing `src/dpapi.rs`, holding PNGs with alpha.
- **Shelf menu:** extend `shelf_menu_target` and the menu builder so a card can have
  more than one item (Tanda tangani for signable kinds, Pratinjau gambar for images).
- **Window behaviour:** Ctrl+Z / Ctrl+Y, Delete removes the selected item, arrows nudge,
  Ctrl+S saves, PgUp / PgDn change page, Esc deselects then closes. Closing with
  changes shows the inline Discard / Keep bar like the screenshot editor.
- **Demo:** `--demo-sign` opens the window on a generated sample PDF (no user data).

## Left to the builder

Module layout and names; thumbnail size and cache; how the page is scaled and
scrolled; stroke smoothing; the date format per language; the exact label strings in
both languages (keep the mockup's Indonesian); what happens to characters outside
WinAnsi in Teks (rasterize, or refuse with a message); timer ids if any are needed.
