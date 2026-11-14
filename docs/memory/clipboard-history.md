---
tags: [decision, clipboard, privacy, dpapi, panel]
date: 2026-11-02
---

# Clipboard history: what it keeps and how

Run 7 of the v3.1 plan (Keputusan #5). Code in `src/clipboard/` (pure:
`kinds`, `image`, `store`, the `persist` format; Win32: `win`) and
`src/app_clip.rs` (the island's side). Privacy rules are in
[[clipboard-privacy]].

- **Kinds.** Text, Link, Code, Color (hex `#RGB`/`#RGBA`/`#RRGGBB`/`#RRGGBBAA`),
  Image (thumbnail + width × height), Files (paths). `detect_kind` is pure:
  a link is one token (`http(s)://`, `www.`, or a bare domain *with a path*),
  so prose with a period or comma never is one; code is a known command
  with arguments, a flag, code marks, or several lines with braces,
  semicolons or indentation. Files beat text, text beats an image.
- **Cap and pins.** Newest first, "Jumlah item" 50 (10–200, step 10).
  Pinned entries are never pushed out by the cap; unpinned ones go oldest
  first, but never the newest entry (so a copy still shows when pins fill the
  cap). An external copy equal to an entry, or a copy from the island, moves
  that entry to the front: no duplicates.
- **Text over 64 KB is not kept at all** (not cut): pasting a cut copy back
  would silently lose its end.
- **Images.** Read from `CF_DIB` (Windows synthesises it from DIBV5/PNG
  apps), at most 64 MB. The thumbnail (≤ 184 × 128, four samples per pixel,
  opaque) is made straight from the DIB on the UI thread: cheap, no full
  decode. All thumbnails together stay under 4 MB (memory and file): the
  oldest unpinned image entries go first. The original DIB is kept in memory
  only, within 16 MB counted in compressed bytes (oldest images lose theirs
  first; see "Originals packed in memory").
  **A restored image (after a restart) copies back at thumbnail size**: the
  file never holds full-size pixels.
  "Screenshot · W × H" when only bitmap formats were on the clipboard,
  else "Gambar · W × H". `clip_image_arrived` hands a new image to the
  screenshot preview ([[screenshot-preview]]).
- **Persistence.** `clipboard-history.bin` next to config.json, DPAPI
  current user (`src/dpapi.rs`, shared with the Google token), plaintext
  wiped. Versioned binary (`DICB`, v1), written to `.bin.tmp` then renamed,
  on a short-lived thread, ordered by a generation counter. Writes coalesce:
  one one-shot `TIMER_CLIP_SAVE` 1.5 s after the last change; a pending save
  is flushed on `WM_DESTROY` and `WM_ENDSESSION`. Missing, foreign or corrupt
  file → empty history, never a crash. "Simpan setelah restart" off: nothing
  is written and the file is deleted.
- **Lock.** "Hapus saat layar dikunci" (default off) clears the unpinned
  entries from memory and the file on `WTS_SESSION_LOCK`. **Pins stay**:
  pinning is an explicit "keep this". The clear button in Settings does the
  same. Turning the history off shows "Riwayat clipboard mati"; the
  entries stay until cleared. The listener stops only when the screenshot
  preview is off too; with the preview on it still looks at new images
  (by the same skip rules) and keeps nothing.
- **Hotkey.** None by default: the `clipboard` row of `config.hotkeys`
  ([[hotkeys]]), recorded in Settings → Tombol pintas → Buka Clipboard (the
  old `clipboard.hotkey` Ctrl+Alt+V / Win+Alt+V presets are migrated once).
  **Ctrl+Shift+V is reserved** (paste as plain text in browsers, VS Code and
  Office) and `check` refuses it. A bad value reads as the default (none).
  `RegisterHotKey` only when set; a failure shows in the row ("Sudah dipakai
  aplikasi lain"). The
  hotkey opens the panel on the Clipboard page *without* the foreground
  (`clip_held`); it closes on a second press, when the pointer visits and
  leaves, or after an 8 s one-shot if it never came.
- **Page.** `Page::Clipboard` sits after Musik, with Sistem
  ([[system-page]]) as its right neighbour and then CLI: the interim order
  of the merge with Runs 4–6, nine pages (`Page::ALL: [Page; 9]`). Run 9
  sets the final order. The
  Pintasan Clipboard tile opens it instead of Win+V
  (`shortcuts::shortcut_action` → `None`). Two columns of 64 px cards,
  scrolled a row per wheel notch over the cards; the selected card gets the
  accent ring and its buttons (pin, copy as plain text, delete), drawn in
  the panel; a right-click selects a card. Clicking a card copies it.
  **Hover selects (3.2.1).** The buttons belong to the *selected* card only,
  and until 3.2.1 only a right-click or the keyboard moved the selection, while
  a left click copies the card and bumps it to the front: so in practice only
  the first card ever showed them. Now `WM_MOUSEMOVE` over a card, or over one
  of its buttons, selects that card (`app_clip::hover_target`, pure and tested;
  `App::clip_hover` redraws only when the selection changes).
  **Hover vs keyboard (3.2.1 review):** Windows sends `WM_MOUSEMOVE` when the
  cards scroll under a still pointer, which stole the keyboard's selection.
  `pointer_moved` ignores a move at the last spot, and after Up/Down or
  PageUp/PageDown (`keyboard_held`) the pointer selects nothing until it
  really moves. A wheel scroll keeps the selection on a card that shows
  (`visible_card`), so Enter never copies one scrolled out of sight.
  Keyboard: [[cli-panel-keyboard-focus]].
- **"Disalin".** `NoticeKind::Copied`, 1.5 s on the notice deadline, a newer
  one replaces it, never over a fullscreen app; Bata Happy
  ([[bata-robot-expressions]]), else the Clipboard badge. Only for copies
  made from the island unless "Disalin untuk semua salinan" is on; never for
  skipped content.
- **Power.** No repeating timer ([[bata-idle-power-budget]]). A change while
  the page is hidden draws nothing (unless a notice shows). Its one-shots are
  timer ids 20–22 (17 and 18 are the System page's).
- **Notices beside the cleaner's.** `Copied` is one more `NoticeKind` next to
  `SystemLow`, `CleanDone`, `MoleDone` and `MoleFailed`; a newer "Disalin"
  replaces only an older "Disalin", never a cleaner or system notice.

## "Hapus semua" in the panel (2026-11-14)

A button in the Clipboard page's title row, left of the search field: first
click arms it ("Yakin?" for `CLEAN_CONFIRM`, 4 s), the second clears every
unpinned entry, same as Settings' "Hapus riwayat". Copy of the Notifikasi
page's armed button: `State.clip_armed`, `Hit::ClipClear`,
`ClipAction::ClearUnpinned`, disarmed by any other click, a page change, the
panel closing or the deadline (no new timer). Design and plan in
`docs/superpowers/specs/2026-11-14-clipboard-clear-all-design.md`.

- **Shared clear.** `App::clip_clear_unpinned` (`app_clip.rs`) holds what
  Settings' clear did (packing, selection and scroll reset, file rewritten,
  dragged-out temp files forgotten); Settings and the button both call it. The
  lock clear is separate and unchanged.
- **When it shows.** `clip_clearable`: history on, search field empty, at
  least one unpinned entry. With a search typed it hides, so "semua" never
  means "only what matches". Armed and then hidden, it comes back as "Yakin?"
  until the deadline (accepted).
- **Layout.** `clipboard_layout(.., clear_w, ..)`: the width is the renderer's
  (max of "Hapus semua" and "Yakin?" plus padding, so arming moves nothing);
  `clip_frame` shrinks the search by it and keeps the title at least 108 dip.
  Text reuses `Key::NotifClearAll` / `Key::NotifSure` (no new key). Demo scenes
  `clipboard` and `clipboard-long` show it, `clipboard-armed` the "Yakin?" pill.
- **Not verified by eye** (the screen was locked during the demo run): the
  one-row fit at 560 px, JetBrains Mono, English. The layout tests cover the
  geometry at button widths 60, 120 and 200, for Pill and Notch with no strip.
- **Left as cleanup:** the paint block and the width measure repeat
  `notif_head` / the Notifikasi paint (a shared helper would remove both
  copies); one `Some(120.0)` case in the layout no-overlap sweep.

## Verify pass (2026-10-28)

- The newest image always keeps its full-size original, even past
  `FULL_BUDGET` (a 4K or 5K screenshot alone passes it); older ones share the
  budget. A card with only its thumbnail left says so when copied ("pratinjau
  kecil, aslinya tidak tersimpan").

- The fingerprint hashes every byte (8 at a time): a one-pixel change is a
  new image. Cards saved by 3.1.0 may not match a re-copy of the same image
  once (old fingerprint).
- With "Simpan setelah restart" off, a history file still on disk is removed
  at startup (`persist::start_items`).
- Text offered but too big to keep (a large Excel range) keeps its bitmap
  out of the history too (`win::wants_image`).
- "Hapus riwayat" needs two clicks, as "Putuskan".

Related: [[clipboard-privacy]], [[settings-appearance-scroll]], [[clipboard-drag-out]]

Listed in [[index]].

## Scrolling (v3.2 Run 2)

The wheel path was wired right; what made it feel dead: a notch over the
cards was always swallowed, even with six cards or fewer (nothing to scroll)
and the page did not turn either; there was no sign the list scrolls; and a
focused search field (a hidden EDIT) took the wheel itself.

- `clip_wheel_step` (pure, `panel_layout.rs`): `None` when everything fits
  (`max_scroll` 0), so the notch turns the page like anywhere on the panel;
  otherwise a row per notch, and an end spends the notch (a long scroll never
  turns the page by accident).
- `ClipLayout::scrollbar`: a 3 px thumb in the right margin, shown only when
  the cards scroll, as tall as the share of rows shown (at least 24 px).
- `cli_input.rs` forwards `WM_MOUSEWHEEL` of the focused EDIT to the island's
  window (the cards, the CLI output and the pages all use it).
- A card click still resets the scroll: the card moves to the front, so the
  top is where it is. Demo scene `clipboard-long` shows the thumb.

See also [[clipboard-notice]]: the preview in the notch on every copy made in another app (v3.5).

## One card per capture (auto-copy, 2026-11-02)

Vale's screenshot editor copies its picture by itself and again after each
finished edit ([[screenshot-snap]], "Auto-copy"). Writes from an editor whose
auto-copy was on (`OwnCopy.managed`) do not take the normal `ingest` path:

- `store::ingest_replacing(history, snap, at, card)` builds the entry by the
  normal rules first (`should_skip`, the size caps, unchanged). Only when the
  snapshot yields an entry does it remove the capture's previous card, if it
  still exists and is **not pinned**, and push the new one. The new card gets a
  new id (the thumbnail cache is keyed by id and never shows a stale picture)
  and goes to the front; `push`'s dedupe against other cards still applies.
- `CaptureCards::card(capture)` is `None` when another capture maps to the
  same card (two pixel-identical captures: the dedupe answered `Bumped` with
  the first one's card). Neither removes a shared card, so two captures keep
  two cards; once one moves on, the other owns it alone again.
- A **pinned** card is never changed or removed: the newer picture becomes a
  new unpinned card. A snapshot that is not kept (over 64 MB, skipped) leaves
  the old card as it is.
- `ClipState.captures` (`store::CaptureCards`, newest 16 captures) maps a
  capture number to its card. `App::keep_capture` updates it, calls
  `schedule_clip_save` and `redraw_clip`. With the history off nothing is kept.
- Not announced: see [[clipboard-notice]]. Windows' own Win+V history still
  records every write; Vale cannot replace those.

## Originals packed in memory (2026-11-06)

`ClipImage.full` is an `Original`: `Raw` (the CF_DIB as the clipboard gave it)
or `Packed` (`store::Packed`: `head` = the DIB bytes before the pixels,
`png` = the pixel array as PNG, `tail` = any bytes after it, `raw_len`, and the
`PngLayout` used). Memory only, as before: `persist` still writes no originals
(DICB v1 unchanged). `FULL_BUDGET` is 16 MB, counted in the bytes an original
takes (a packed one counts head + png + tail).

- **Newest image rule.** The newest *image* entry (the first entry whose data
  is an image, **not necessarily index 0**: a text copied after a screenshot
  leaves it raw and exempt) keeps its `Raw` original whatever its size. This is
  a deliberate change: before, only index 0 was exempt. Every other image is
  packed.
- **Pack state.** `PackState { Idle, Pending, Failed }` per image.
  `History::pack_jobs()` hands out the non-newest `Raw` + `Idle` originals
  (`PackJob { id, fingerprint, raw }`) and marks them `Pending`.
  `set_packed(id, fingerprint, result)` applies an answer only if the entry
  still exists, has that fingerprint, is still `Raw` + `Pending` and is not now
  the newest image (then it goes back to `Idle`, raw). `Some` -> `Packed`;
  `None` -> `Failed` (stays raw). Then `enforce` runs.
- **Budget cost.** Newest image 0; `Raw` + `Pending` 0 (exempt until the worker
  answers, else a 33 MB 4K raw would be dropped at once against 16 MB);
  `Raw` + `Idle` (just made older, not yet handed out) also 0 for the same
  reason, because `push` runs `enforce` before `pack_jobs`; `Packed` its
  compressed bytes; `Failed` its raw length. Eviction as before: walk newest
  first, an original that would pass the budget is dropped (thumbnail only).
- **Byte-exact layout** (`clipboard/pack.rs`, WIC PNG): `dib_info` splits the
  DIB. 32 bpp -> `GUID_WICPixelFormat32bppBGRA`, `w` x `h`, stride `w * 4`
  (PNG is not premultiplied, so colour under alpha 0 survives). 24 bpp ->
  `GUID_WICPixelFormat8bppGray` of width = row stride and height = `h`, which
  carries the row padding too. Decode uses the same format and `CopyPixels`
  into the same layout; head and tail are put back verbatim.
- **Verify on encode.** `pack` decodes its own output and compares it
  byte for byte with the input; a mismatch, a WIC error or a codec that
  would convert the pixel format returns `None`, and the original stays raw
  (`Failed`, counted raw).
- **Decode on paste, keep raw.** `win::formats_for` unpacks a `Packed`
  original on the UI thread (COM is already there); `None` or a failed unpack
  pastes `dib_from_thumb` as for no original. `copy_clip` then moves the entry
  to the front, `set_raw(id, decoded)` keeps the decoded DIB as its `Raw`
  original (it must be as long as `raw_len`, else the original is forgotten:
  `forget_original`) and `clip_schedule_packing` packs the image that is no
  longer the newest. A re-copy of the same image (`push`) also brings a `Raw`
  original back over a `Packed` one or none.
- **Worker and event.** `App::clip_schedule_packing` (app_clip.rs) takes
  `pack_jobs()` after every ingest, `keep_capture`, delete, clear, cap change
  and paste, and spawns one short-lived `clip-pack` thread per batch (never
  the UI thread; `CoInitializeEx` MTA, `CoUninitialize`). Each result goes back
  as `Event::ClipPacked { id, fingerprint, packed }` through `tx` and the
  `WM_APP_EVENT` poster; `on_clip_packed` calls `set_packed`. No redraw, no
  save. A thread that cannot start marks its jobs `Failed`.
- `app_shot` reads `img.raw()` (a new image is the newest, so raw);
  `copied_notice` says "pratinjau kecil" only when `!has_original()`.
