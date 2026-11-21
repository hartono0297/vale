# Text formatting in the sign window: design

Date: 2026-11-21. Owner request (2026-11-21, while testing bc3a7af): "mungkin
untuk edit, bisa ditambahkan pilihan font, bold italic dll". Owner left every
how-to choice to the builder ("u atur terbaiknya gimana", "ya atur ja yang
terbaiknya gimana") after reading the approach summary below.

Mockup: `docs/design/tanda-tangan-format.html`.
Builds on: `docs/superpowers/specs/2026-11-20-tanda-tangan-design.md` and the
code in `src/sign/`.

## Intent

The owner types fill-in text onto documents (names, dates, numbers, notes) and
wants it to look right on the page: a serif face to match a formal letter, bold
for a name, a colour that is not always blue. The sign window today writes
every Text and Date item in one face (Arial on screen, Helvetica in the PDF),
always ink blue (`pdf_write::INK`, `paint::INK`), and the check mark is always
blue too.

## Goals

- Text and Date items get: font family (3), bold, italic, underline, colour (3).
- Check items get: colour.
- PDF output keeps text as real text (searchable, copyable, sharp at any zoom).
- Image output (JPG/PNG) and the screen look the same as the PDF.
- Ctrl+B, Ctrl+I, Ctrl+U.
- Each format change is one undo step.

## Non-goals

- Fonts other than the three below; embedding fonts in PDFs.
- Alignment, line spacing, multi-line text (items stay one line).
- Strikethrough, highlight, free colour picker.
- Changing the typed-signature font in the signature maker (signatures and
  initials are pictures with their own ink choice; possible follow-up).
- Remembering the last style across Vale restarts.

## Anti-goals (a failure even if it works)

- Any PDF where formatted text becomes a picture when it could be real text.
- Screen and saved file disagreeing on face, weight, slant, colour or width.
- `vale.exe` growing past the cap: the whole Tanda tangan module (this change
  included) stays within +1,363,148 bytes over the 5,009,920 baseline, i.e.
  `vale.exe` at most 6,373,068 bytes. Today it is 6,028,800.
- Breaking any existing sign behaviour (rotation, all-pages initials, deleted
  pages, encrypted PDFs, non-WinAnsi fallback, undo).

## Approaches considered

1. **The three standard PDF families** (picked). Sans = Arial on screen /
   Helvetica in PDF; Serif = Times New Roman / Times; Mono = Courier New /
   Courier. Each with bold, italic, bold-italic: the 12 base-14 fonts every PDF
   reader has, so nothing is embedded. Real text, zero font bytes, small code.
   Trade-off: only three faces.
2. Any installed Windows font, subset-embedded in the PDF. Most faithful, but
   needs a font subsetter crate (likely most or all of the ~344 KB left under
   the cap), TrueType/CID font dictionaries, ToUnicode maps and respect for
   fonts whose licence forbids embedding. Rejected: cost and size.
3. Any installed Windows font, drawn as a picture in the PDF. Cheap, but the
   text stops being searchable/copyable and files grow. Rejected: it gives up
   what makes the PDF useful (anti-goal 1).

## The design

### Model

- A text style value: family (Sans | Serif | Mono), bold, italic, underline,
  colour. Colour is one of Black | Blue | Red:
  - Black `#1C1C1C` (same as `Ink::Black`), Blue `#1F3A8A` (today's ink,
    `Ink::Blue`), Red `#A61B1B`.
- `ItemKind::Text` and `ItemKind::Date` carry the string and a style.
  `ItemKind::Check` carries a colour. Names and exact shape are the builder's.
- Default style for a new Text/Date: Sans, not bold/italic/underline, Black.
  Default colour for a new Check: Black.
- "Last used": every format change sets the style new items of that kind
  start with (Text and Date share one last style; Check has its own last
  colour, which a colour change on text does not touch). Kept in memory for
  the life of the Vale process (all sign windows), never written to disk.
- The Date that the signature bar's "Tambah tanggal" button adds uses the
  last Text/Date style too.

### Screen

- Faces: Sans → "Arial", Serif → "Times New Roman", Mono → "Courier New",
  via DirectWrite with weight 700/400 and style italic/normal. Underline: the
  DirectWrite layout's underline (or a drawn line at the font's underline
  position and thickness), in the item's colour.
- The selected item's floating bar (`window::float_hits`, `layout::float_bar`):
  - Text/Date: `[family ▾] | B I U | ●black ●blue ●red | Kecilkan Besarkan Hapus`
    (plus whatever the bar already has for that kind, e.g. "Semua halaman" does
    not apply here).
  - Check: `●black ●blue ●red | Kecilkan Besarkan Hapus`.
  - Signature/Initials: unchanged.
  - B, I, U and the swatches show their on state (accent tint for B/I/U, a ring
    for the current swatch), as in the mockup.
  - The family button shows the current face's name drawn in that face.
- If the bar is wider than the stage allows, it wraps into two rows: format row
  on top, the existing actions under it. (One row when it fits.)
- Family button opens a three-row menu (Arial, Times New Roman, Courier New),
  each row drawn in its own face, current one ticked. It opens on the side of
  the bar away from the item (above the bar when the bar sits above the item,
  below when it sits under), kept inside the stage. Escape or a click outside
  closes it. Keyboard: the existing popover/menu keyboard handling applies.
- Swatches and B/I/U have no text labels; each has a tooltip-style name if the
  window already has a tooltip mechanism (builder checks; adding one is not
  required). New i18n keys only for words that are shown or read.
- Shortcuts: Ctrl+B / Ctrl+I / Ctrl+U toggle bold / italic / underline on the
  selected Text/Date item, and also while that item's text is being edited
  (the Win32 EDIT control's subclass must pass them through to the window
  instead of letting EDIT swallow them). They do nothing for other kinds.
- While editing, the EDIT control's font matches the item's style (family,
  weight, italic, underline) and its text colour matches the item colour.
- Changing family/bold/italic re-measures the text: the box keeps its height
  and its left edge in the item's upright frame, and its width becomes the new
  measured width (same rule the editor already uses when text is typed). If
  the new box would leave the page, it is pulled back inside like a drag is.
  Underline and colour do not change the size.

### PDF (`pdf_write`)

- Font resources: one Type1 font dictionary per variant actually used, with
  `/Encoding /WinAnsiEncoding`, BaseFont from:

  | | regular | bold | italic | bold italic |
  |---|---|---|---|---|
  | Sans | Helvetica | Helvetica-Bold | Helvetica-Oblique | Helvetica-BoldOblique |
  | Serif | Times-Roman | Times-Bold | Times-Italic | Times-BoldItalic |
  | Mono | Courier | Courier-Bold | Courier-Oblique | Courier-BoldOblique |

  Each variant's font object is added once per save and shared by all pages
  (today `sh.font` is one shared object; it becomes one per variant). Resource
  names stay collision-free per page (`free_name`).
- Text keeps today's placement (font size 0.8 of the box height, baseline at
  0.2, horizontal scale undoing the box stretch) with the item's colour as the
  fill colour.
- Underline: a filled rectangle in the item's colour under the text, from the
  text's start to its end, at the font's underline position and thickness
  (base-14 AFM: UnderlinePosition -100, UnderlineThickness 50, per 1000 em,
  for all three families), matching where the screen draws it.
- Check: stroked in its colour.
- Non-WinAnsi text keeps today's fallback (`window::encodable` →
  `paint::text_mark`), but the picture is drawn in the item's face, weight,
  slant, underline and colour.
- Incremental save, encryption, deleted pages and every other writer path
  are unchanged.

### Image output (`image_out`)

- Text/Date/Check are drawn with the same style as the screen (it already
  shares the paint code path; that path takes the style).

### Undo

- Each format change on a selected item (bar click, menu pick, shortcut) is
  one undo step, including the width change it causes.
- While a text is being edited (the floating bar is hidden then, and a new
  Text does not exist in the session until it is committed), Ctrl+B/I/U
  change a pending style held by the edit: the EDIT control's font shows it
  at once, and nothing touches the session. On commit, text and style land
  as one undo step (for a new item, it is added with that style; the
  last-used style becomes it). Escape drops the pending style with the
  typed text. Committing an unchanged text with a changed style is a real
  edit (today `finish_text` returns early when the text is unchanged; that
  check must include the style). Builder-check question, settled by the
  builder (2026-11-21).

## Constraints

- Rust 2024, Windows 11 only; `windows` 0.62; no new crates.
- Exe cap above; record the new size in the plan's Size log.
- Tests in `#[cfg(test)]` modules; `cargo test --locked` green.
- No change to `src/shot_edit/`.

## Calls the builder makes

- Exact type names and where the style type lives (`sign::mod` is the natural
  home since `ItemKind` is there).
- How the underline width is computed in the PDF (the box width is the
  measured text width plus a small pad today; use the measured text advance).
- Whether bar tooltips exist; whether the two-row wrap is ever reached at the
  window's minimum size.
- Exact red; must keep 4.5:1 contrast on the page paper (`#FFFDF9`). `#A61B1B`
  passes.
