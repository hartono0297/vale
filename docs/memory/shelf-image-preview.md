---
tags: [decision, shelf, preview, image, wic]
date: 2026-11-14
---

# Shelf: image thumbnails and the right-click preview

Spec: `docs/superpowers/specs/2026-11-14-shelf-image-preview-design.md`. Builds on
[[shelf]] (the page, cards, stale marking) and shares the WIC decode with the
phone page's thumbnails ([[shelf-phone]]). Code: `src/shelf_thumb.rs` (decode),
`src/shelf_thumbs.rs` (pure cache and preview state), `src/app_shelf.rs`
(worker wiring), the card and preview painting in `src/ui/render_panel.rs`.

## Decisions

- **Right-click, not hover.** The owner picked "Pratinjau gambar" in a card's
  right-click menu (one item, image cards that are not stale only; every other
  right-click is unchanged). A hover preview would also fight the panel's own
  hover-to-open and the card's drag.
- **The preview is a sub-view inside the panel, not a window.** Same idea as
  the phone link's views: back button, the file name as title, the image
  centred, drawn by the same renderer with the existing reveal. A floating
  window (like the lyrics card) would add a window class, placement,
  fullscreen and capture rules and a second place to keep the theme right.
  Limit: the image is as big as the panel, and is never scaled above 1:1.
- **Thumbnails are lazy, on one worker, only while the Shelf page is open.**
  No timer and no file IO on the UI thread ([[bata-idle-power-budget]]). One
  short-lived thread per batch posts one event per result and exits. A batch
  already started keeps decoding after the page closes (up to 12 photos at
  192 px); results for a card that is gone are ignored. Each open of a preview
  starts its own decode (nothing is cached for it).
- **Sizes:** card thumbnail 192 px on the long side (`CARD_SIDE`), preview
  1024 px (`PREVIEW_SIDE`), files over 64 MiB are not decoded (the phone
  thumbnail's cap), and neither are images that declare more than 250 M pixels
  (`MAX_PIXELS`: a small, highly compressed file can ask for gigabytes). EXIF
  orientation is applied. The pixels are premultiplied BGRA (WIC `PBGRA`), which
  is what `thumb_bitmap` declares, so a transparent PNG shows the card colour
  through its clear areas instead of going dark. A card thumbnail is
  cover-cropped into the card's picture box.
- **Network paths get no card thumbnail** (the icon stays, so a dead share
  never makes the page decode slowly); a preview of one is allowed because the
  owner asked for it, and it runs on the worker.
- **A failed decode is remembered** (`Slot::Failed`): the card keeps its icon
  and is not asked again. The preview then says "Tidak bisa menampilkan
  gambar" (HEIC or AVIF without a Windows codec lands here).
- **Cache of 12, keyed by the card key** (the shelf's cap), dropped with the
  card (`retain`). **No size or mtime stamp:** a file rewritten in place keeps
  its old thumbnail until its card is taken off (re-decoding on each page
  open would cost more).
- **Bitmap caches are cleared with the render target**, like `clip_thumbs`
  ([[d2d-bitmaps-vs-pixels]]); the preview's pixels are dropped when the
  preview closes.
- **A drag over the island closes the preview** (`on_shelf_drag(true)` and
  `on_shelf_drop`): only the cards' page draws the drop zone, so files dropped
  on an open preview would land unseen.
- **`MenuOpen` holds the panel while the menu is open.** The menu is modal, so
  the pointer leaves the island and the panel would close under it (`app.rs`,
  next to `open_note_card_menu`).

## Gotchas found in review

- A ghost card (one taken off, fading out) is drawn as its icon, not its
  thumbnail: the ghost has no cache slot any more.
- Both D2D bitmap caches (the card thumbnails and the preview's big bitmap)
  match on the card key AND the very same decode (`Arc::ptr_eq`,
  `shelf_thumbs::bitmap_is_current`). A card taken off and put back keeps its
  key but gets a new decode, so a key-only match would draw the old picture;
  with the check a new decode rebuilds the bitmap and replaces the entry.
- The menu is modal and the pick carries the card's index: if the shelf
  changes while it is open (a phone upload, a card removed), that index can
  point at another card. `shelf_preview_open` only checks that the card at the
  index is still a previewable image, so it can open a different image than
  the one that was clicked. Not fixed (rare, harmless). A card with no path
  ends the preview as `Failed` instead of `Loading` for good.
- The UUID or all-digit file names from a phone are what the phone's browser
  sends; Vale already stores the name it receives. Not changed here.

## Demo scenes (debug builds)

`shelf-thumbs`, `shelf-preview` (a long name), `shelf-preview-loading` and
`shelf-preview-failed`, with generated gradients and no files; Pill and Notch
(`notch:` prefix), `DI_LANG`, `DI_FONT=mono`. `ShelfThumbs::with_slots` exists
for them under `cfg(debug_assertions)`. See [[screenshot-capture-script]] for
the capture: while the PC is locked `CopyFromScreen` returns the lock screen
(620 x 460 of wallpaper), but `PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)` on
the demo window still gives the island (560 x 290, 580 x 290 for the Notch).

## Not verified live

A real right-click on the island, thumbnails of real photos (HEIC from an
iPhone in particular), memory use with twelve 20 MP photos on the shelf, and the
release build (size: see [[exe-size]]). Only the demo scenes were looked at.
