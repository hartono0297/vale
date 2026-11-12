---
tags: [decision, clipboard, drag-drop, ole, privacy]
date: 2026-11-12
---

# Clipboard: dragging an image card out

An image card on the Clipboard page drags out as a real PNG file onto anything
that takes files (website upload boxes, WhatsApp Desktop/Web, Explorer,
Discord). Text, link, code, colour and file cards do not drag (owner: images
only). Spec `docs/superpowers/specs/2026-11-12-clip-image-drag-design.md`.

- **Why a file.** Most upload boxes take only files (`CF_HDROP`), not a bitmap.
  So the drag writes `%TEMP%\Vale\drag\<card id>\<name>.png` and hands it to
  `shelf_ole::drag_out` (the shell's own data object for that path), offered
  **copy only** (`CLIP_DRAG_EFFECTS`): a folder drop must make a real file, not
  a shortcut to a file that will vanish. The shelf keeps copy or link
  (`SHELF_DRAG_EFFECTS`). `drag_out(path, allowed)` returns the effect, `None`
  when the drag could not start.
- **Name.** `drag::drag_name`: `"Screenshot YYYY-MM-DD HH.MM.SS.png"` for a
  screenshot card, else `Gambar` / `Image` (`Key::ClipKindImage`), the time
  being `Clip::at` in local time. One folder per card id, so two cards copied
  in the same second never clash (ids are monotonic per run and the startup
  sweep covers a restart).
- **Start.** A press on an image card (`drag_press_target`) and a move past
  6 dip (`past_threshold`, shared with the shelf) with the primary button down,
  only while the Clipboard page is the open panel. Pin, plain-text and delete
  never start one. The click that would copy is cleared only once the file
  exists: a failed write (full or blocked `%TEMP%`) still copies on release.
- **Pixels.** `drag::png_pixels` flips bottom-up DIBs and drops row padding; a
  32 bpp DIB whose alpha bytes are all 0 (most screenshots) comes out opaque,
  real alpha is kept. `pack::encode_png` (the pack encoder, generalised) writes
  it with WIC on the UI thread. Live: a 4K card was on disk well under 300 ms
  after the threshold.
- **Thumbnail-only cards** (restored after a restart, or the budget dropped the
  original) drag their small picture; after an accepted drop the notch shows
  "Ditarik" with the copy notice's "pratinjau kecil" body. An unpack failure
  that falls back to the thumbnail counts as small too (`drag::full_size`).
- **History untouched.** No bump, no `set_raw`, no save.
- **Lifetime (`DragFiles`).** Removed `KEEP` (10 min) after the drag that last
  used it, on exit (`clip_shutdown`, `WM_DESTROY` and `WM_ENDSESSION`), on lock
  when "Hapus saat layar dikunci" is on, when its card is deleted (also through
  the screenshot notice's Delete), and for the cards "Hapus riwayat" clears (a
  pinned card keeps its file until its 10 minutes end). Startup sweeps the whole
  folder. A re-drag within 10 min reuses the file and restarts its clock, unless
  the card's original came back or was dropped since: then it is rewritten in
  place. A failed removal is retried after `RETRY` (60 s) and that entry is never
  reused. Cards pushed out by the cap or replaced by auto-copy keep their file
  until their 10 minutes end.
- **Power.** One one-shot `TIMER_CLIP_DRAG` (55) for the nearest expiry, set
  only while a file exists.
- **Panel.** It closes once the drag is under way (OLE takes the mouse and the
  island sees the pointer leave); this does not get in the way of a drop
  elsewhere, and the after-drag steps are shared with the shelf
  (`drag_out_ended`).

**Verified live (2026-11-12, release build, own APPDATA):** a 4K card dropped on
an Explorer folder became a real `Screenshot ….png` (3840 × 2160, not a
shortcut); a second drag reused the file; a drop on the Claude desktop app
(Chromium/Electron) attached the image; quitting left the folder empty.
**Not verified live:** WhatsApp Desktop, WhatsApp Web, a website upload box,
the "Ditarik" notice, the lock cleanup, the 10 minute timer.

Related: [[clipboard-history]], [[clipboard-privacy]], [[shelf]],
[[bata-idle-power-budget]], [[exe-size]]
