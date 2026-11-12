# Drag an image card out of the Clipboard page

Agreed with the owner in chat on 2026-11-12.

## Intent

The owner wants to drag an image from Vale's clipboard history straight onto
anything that takes an image: a website's "drag & drop / upload image" box,
WhatsApp (Desktop or Web), an Explorer folder, Discord, Telegram. Today the
only way is click (copy) and paste, and many upload boxes take no paste.

## Goals

- An **image** card on the Clipboard page can be dragged out and dropped onto
  any target that accepts files.
- Text, link, code, colour and file cards do not change (owner: "gambar aja").

## Decided

- **Click still copies.** A drag starts only when a card is pressed and the
  pointer moves more than `DRAG_START_DIP` (6 dip, the shelf's threshold)
  with the primary button down, while the Clipboard page is the open panel.
  The click that would have copied the card is dropped. Pin, plain-text and
  delete buttons never start a drag.
- **A real PNG file is handed over.** At drag start Vale writes the image to a
  temporary PNG file and drags it the way the shelf drags a file (the shell's
  own data object for that path, `CF_HDROP` and friends). Only files are
  accepted by most upload boxes; a bare bitmap is not.
- **File name** reads like the card: `Screenshot 2026-11-12 14.03.12.png` for
  a screenshot, else `Gambar 2026-11-12 14.03.12.png` (English: `Image ...`),
  the time being the card's copy time (`Clip::at`, local). Each card gets its
  own folder (`%TEMP%\Vale\drag\<card id>\`) so two cards copied in the same
  second never clash.
- **Copy only.** The drag offers `DROPEFFECT_COPY` only, never link or move,
  so a folder drop makes a real file, not a shortcut to a file that will
  vanish.
- **Thumbnail-only cards** (restored after a restart, or whose original was
  dropped by the budget) can be dragged; the small picture is what goes. After
  a drop that was accepted the notch says so with the same body as the copy
  notice ("Screenshot · pratinjau kecil, aslinya tidak tersimpan").
- **Packed originals** are unpacked on the UI thread for the drag, as for a
  paste. A drag does not move the card or change the history in any way.
- **Temp file lifetime.** A file is removed `KEEP` (10 minutes) after the drag
  that last used it, or when Vale exits, whichever comes first. At startup the
  whole `%TEMP%\Vale\drag` folder is removed (a crash may have left files).
  When "Hapus saat layar dikunci" is on, the lock removes them too. Deleting a
  card removes its file, and "Hapus riwayat" removes the files of the cards it
  clears; a pinned card keeps its file until its 10 minutes run out. Dragging the
  same card again within the 10 minutes reuses its file (no new encode) and
  restarts its 10 minutes; if its original came back or was dropped since, the
  file is written again in the same place.
- **Idle power.** One one-shot timer (`TIMER_CLIP_DRAG`, id 55), set only while
  a temp file exists, for the nearest expiry. No repeating timer.
- **Panel.** It stays open during the drag; after it, the shelf's
  after-drag rules apply (the swallowed release is dropped, a pointer that
  ended off the island leaves it).

## Must not change

Click, pin, delete, copy as plain text, the history file format (DICB v1, no
originals written), the privacy rules in `clipboard-privacy`, the shelf's own
drag out (its effects stay copy or link).

## Left to the builder

- The drag image (default cursors as the shelf, or a picture of the card).
- How a 32 bpp DIB whose alpha bytes are all 0 is written: it must come out
  opaque (most clipboard bitmaps carry no alpha). A DIB with real alpha keeps
  it.

## Done

Merged into `main`, `cargo test --locked` green there, memory and README
updated, `main` pushed, worktree cleaned. The owner checks by hand: drag to
WhatsApp Desktop, WhatsApp Web, a website upload box, an Explorer folder.
