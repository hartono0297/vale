---
tags: [spec, shelf, preview, image]
date: 2026-11-14
---

# Shelf: image preview (design)

Owner's words: "setiap file di shelf, bisa preview image". Chosen with the owner:
right-click a card, then "Pratinjau" (not hover). Everything else is my call, marked
as such; the owner delegated it ("u atur terbaiknya").

## Intent

See what an image file on the shelf is without opening another app. Two parts:

1. **Right-click an image card -> "Pratinjau gambar" / "Preview image"**: the shelf
   page switches to a preview of that image, with a back button.
2. **A small thumbnail on the card** in place of the type icon (my call: it is the
   cheap way to tell images apart at a glance, and the data is already made for 1).

## Not in this change (the owner dropped the first one)

- The phone's file names. A UUID-like or all-digit name (for example
  `17945515658641754357379552135884.jpg`) is what the phone's browser gives; Vale
  already stores the name it receives (`phone/server.rs::upload`, `names::sanitize`).
  Nothing to fix in Vale.
- A hover preview, a floating preview window, zoom or pan, video or PDF preview.
- Anything for non-image cards: they keep today's right-click (the tray menu).

## Anti-goals

- No cost at idle: no timer, no thread while the shelf page is closed, no decode
  per frame ([[bata-idle-power-budget]]).
- No decoding or file IO on the UI thread. A network or slow path never blocks it.
- No full-size image held in memory after the preview closes.

## Approaches considered

- **A. A sub-view inside the panel (picked).** Like the phone's QR view: the
  shelf page gets a preview state with a back button, drawn by the same renderer,
  with the existing 160 ms reveal. No new window, no new surface rules (fullscreen
  hiding, multi-monitor, capture exclusion, timers). Limit: the image is as large
  as the panel, not larger.
- B. A floating preview window (like the lyrics card). Larger and resizable, but a
  new window class, lifetime, placement, fullscreen and capture rules, and a second
  place to keep the theme right. Too heavy for "see what it is".
- C. Open the file in Windows' default viewer. Zero UI, but the card click already
  does that; it is not a preview.

## Design

**Menu.** `on_right_click` (`src/app.rs`) gets a new `RightClick::ShelfMenu(key)`
beside `NoteMenu` and `RecRowMenu`, only when the hit is a shelf card on the Shelf
page whose kind is Image and which is not stale. One item through the same one-item
menu helper the note card uses (`tray::note_card_menu`, generalised or copied by
the builder, whichever is smaller). Every other right-click is unchanged. A pick
applies the same event as a click would, so the panel redraws.

**Preview state.** The shelf page has a preview state holding the card's key. While
it is on, the panel is held open like other sub-views and the shelf cards do not
react. Back button (and a click on the image) returns to the cards; page change,
collapse, a card removed or gone stale also end it. The title shows the file name.

**Decoding (worker, lazy).**
- Format: whatever WIC decodes (jpg, png, gif first frame, bmp, tiff, webp where the
  codec exists). HEIC and AVIF need codecs Windows may not have: then the preview
  shows "Tidak bisa menampilkan gambar" and the card keeps its icon. Files over
  64 MB are not decoded (same cap as the phone thumbnail).
- EXIF orientation applied; reuse the WIC code of `phone/thumb.rs` rather than a
  second copy (the builder moves the shared part out if needed).
- One short-lived worker thread per request batch, started only when the Shelf page
  is open and an Image card has no thumbnail yet, or when a preview is asked for.
  It posts one app event per result and exits. No timer.
- Card thumbnail: at most 192 px on the long side, BGRA pixels in a small map keyed
  by the shelf key, at most 12 entries (the shelf's cap), kept while the card stays
  and dropped with it (no size/mtime stamp: a file rewritten in place keeps its old
  thumbnail until its card is taken off; simpler, and re-decoding on every page open
  would cost more). **Network paths
  (`shelf::is_network`) get no card thumbnail** (icon stays); a preview of one is
  allowed because the owner asked and it runs on the worker.
- Preview image: at most 1024 px long side, one at a time, dropped on close.
- Renderer: D2D bitmaps made from those pixels on first draw and cached per target,
  cleared when the target is rebuilt, exactly like `clip_thumbs`
  ([[d2d-bitmaps-vs-pixels]]).
- Until a thumbnail arrives the card draws its icon; no loading flicker. The preview
  shows "Memuat..." until the pixels arrive.

**Text** (both languages, `src/i18n.rs`; the fit tests must hold in both fonts):
menu item, "Tidak bisa menampilkan gambar", "Memuat...", the back button's tooltip if
the page has one. Shared-file care: `i18n.rs`, the right-click handler (another
session is fixing a right-click bug there) and the panel render code.

**Tests (pure where possible).** Which cards are previewable (kind, stale, folder);
thumbnail cache rules (key change, cap, drop with the card, network skipped); fit
(contain) maths for the preview box; the new strings fit in both fonts; the reducer
ends the preview on the events above; right-click target selection like
`note_card_menu_target`. WIC decode is tested on a tiny generated PNG in the test
temp folder.

**Memory and docs.** A note `shelf-image-preview` in `docs/memory/`, a line in the
index and in [[shelf]]; README and guide mention the right-click. Exe size recorded.

## Left to the builder

Exact paddings and the preview box size inside the panel (follow the other
sub-views), the Esc key if the panel already handles keys, the name of the new
event, and whether the menu helper is generalised or copied.

## Not verified live (to say in the final report)

A real right-click on the island, thumbnails of real photos (HEIC from an iPhone
in particular), a large photo's memory use.
