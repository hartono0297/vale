---
tags: [spec, clipboard, panel]
date: 2026-11-14
---

# Clipboard: "Hapus semua" button in the panel (design)

Owner's words: "tambahkan opsi/tombol bisa hapus semua clipboard". Settings →
Clipboard already has "Hapus riwayat" (two clicks, pins stay, the file is
rewritten). What was missing is a way to do it from the panel itself. Chosen with
the owner: a button on the Clipboard page, two clicks ("Yakin?"), pins stay.
Everything else is my call, marked as such.

## Intent

Clear the clipboard history without opening Settings.

## Goals

- A "Hapus semua" button in the title row of the Clipboard page, left of the
  search field. First click arms it ("Yakin?", 4 s, `CLEAN_CONFIRM`), the second
  clears. Same pattern and look as the Notifikasi page's "Hapus semua".
- It clears what Settings' "Hapus riwayat" clears: every unpinned entry, memory
  and the `clipboard-history.bin` file, and the dragged-out temp files of the
  cards that are gone. Pinned entries stay.
- English and Indonesian text.

## Non-goals

- An option to clear pinned entries too. Undo. A hotkey.
- Any change to Settings, the lock clear or the history's persistence.

## Anti-goals

- No timer or thread at idle: the arming reuses the state's existing deadline
  list ([[bata-idle-power-budget]]).
- No accidental wipe: one stray click never clears.

## Approaches considered

- **A. Armed button in the title row (picked).** Copies the Notifikasi page: a
  state field with a deadline, a header button measured by the renderer, a click
  reducer arm/confirm. Smallest, no new concepts.
- B. 5 s Undo strip (like the sticky note list). Rejected by the owner: it needs a
  snapshot and a delayed file write.
- C. A button under the card list. Rejected: the list scrolls, the button would
  leave the screen.

## Design

- `Hit::ClipClear`, `ClipAction::ClearUnpinned`. `State.clip_armed:
  Option<Instant>`: first click sets `now + CLEAN_CONFIRM`; a second before it
  expires emits `Effect::Clip(ClipAction::ClearUnpinned)`. Any other click, the
  deadline, a page change or the panel closing disarms it; it is in
  `next_deadline` and `expire_confirm`. The hit counts only while the Clipboard
  page shows (as the other `Clip*` hits).
- `ClipView` gets `clearable` and `armed`. **My call:** `clearable` is true only
  when history is on, the search field is empty and at least one unpinned entry
  exists. With a search typed the button hides, so "semua" never means "only what
  matches" or "more than I see". Pinned-only history hides it too (nothing to do).
- `clipboard_layout` takes the button's width (`None` when hidden), places it
  left of the search field, vertically centred in the title row; the title and
  the search shrink to make room. The renderer measures the width from "Hapus
  semua" and "Yakin?" so arming moves nothing. `regions` adds the button hit;
  painting reuses the Notifikasi look (trash icon and label; armed: soft-red pill
  with "Yakin?").
- The clearing code of Settings' `ClipSetting::Clear` moves into one `App`
  method used by Settings and the new action (no copy of the block).
- i18n: no new key. The button reuses `Key::NotifClearAll` ("Hapus semua" /
  "Clear all") and `Key::NotifSure` ("Yakin?" / "Sure?"), the words the
  Notifikasi page already has.
- Tests: reducer arm / confirm / expire / disarm, layout (button position,
  hidden, search keeps a minimum width), which entries make the button show.
  The clearing itself is `History::clear_unpinned`, already tested (pins stay).

## Left to the builder

Exact gap, the search's minimum width on the narrowest panel, the demo scene
(show the button in the existing clipboard scenes), and the memory note and
README line.
