---
tags: [feature, sign, pdf, text, directwrite]
date: 2026-11-21
---

# Sign: font, bold, italic, underline and colour for text

Merged into `main` at 962a285 (2026-11-21). Spec `docs/superpowers/specs/2026-11-21-format-teks-design.md`, plan `docs/superpowers/plans/2026-11-21-format-teks.md`, mockup `docs/design/tanda-tangan-format.html`. Part of [[tanda-tangan]].

- **Controls:** a selected Text or Date gets a font button (Arial, Times New Roman, Courier New, rows drawn in their own face), B, I, U and three inks (black, blue, red) in its floating bar. A Check gets the three inks only.
- **Keys:** Ctrl+B / I / U work on a selected text or date and while typing. Enter or a click away applies the text and its style as one undo step; Escape drops both. AltGr (Ctrl+Alt) does not toggle.
- **Defaults:** new items start black (was always blue), then follow the last style used while Vale runs. Not saved to disk. Text and date share one style, the check keeps its own colour.
- **Re-measure:** changing font, bold or italic resizes the box; height and text start stay, also on turned pages, and the box is pulled back inside the page.
- **PDF output:** the text stays real text (searchable, copyable) in the PDF's own Helvetica / Times / Courier, plain, bold, italic, bold-italic. Nothing is embedded. One Type1 font object per base font per save, `rg` for colour, underline as an `re f` rectangle at -0.1 em, 0.05 em thick, across the text. Writer: [[sign-pdf-writer]].
- **Fallback:** text outside WinAnsi still becomes a picture, now in the chosen face, style and colour. JPG / PNG output and the screen draw the same.
- **Size:** +33,280 B ([[exe-size]]), no new crate.
- **Kerning is off everywhere.** DirectWrite kerns by default and PDF `Tj` on base-14 fonts does not. Measure with `IDWriteTypography` and the kern feature set to 0, or PDF text and underline drift (AVATAR: 3.704 vs 4.001 em).
- **Why the screen measure fits the PDF:** Arial, Times New Roman and Courier New are metric-compatible with Helvetica, Times and Courier.
- **Ctrl+I in a Win32 EDIT arrives as `WM_CHAR` 0x09** (Ctrl+B 0x02, Ctrl+U 0x15), so the subclass swallows them.
- **Tests that read or set the last-used style take `style::TEST_LOCK`:** the statics are process-wide and tests run in parallel.
- **Code:** `src/sign/style.rs` (Family, Tint, TextStyle, last-used statics), `src/sign/text_draw.rs` (shared DirectWrite measure and draw, `advance_em` with a process-shared factory), `pdf_write.rs`, `window/{layout,paint,window,text_edit}.rs`.
- **Not done:** other fonts or embedding, alignment, multi-line, strikethrough or highlight, a free colour picker, the typed-signature font in the signature maker.
