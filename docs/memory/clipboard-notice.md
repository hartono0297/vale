---
tags: [decision, clipboard, notices, privacy, power, v3.5]
date: 2026-11-02
---

# Copy notice: a preview in the notch on every copy (v3.5 request 3)

`NoticeKind::ClipPreview { id }` (the history entry's id), shown when the user
copies text (Text, Link, Code, Color), an image or files in **another app**.
Code: `src/clipboard/notice.rs` (pure: the rules and the text lines),
`src/app_clip.rs` (`copy_feedback`, `clip_preview_notice`, `clip_notice_view`),
`src/state.rs` (reducer), `src/ui/render.rs` (`paint_clip_notice_thumb`).
History and privacy: [[clipboard-history]], [[clipboard-privacy]].

## What it shows

- **Text:** the Clipboard badge, or Bata happy with the robot on. Title: the
  first non-empty line; body: the second non-empty line, else "Teks · 42
  karakter" (a link or a colour: the kind's name alone). Lines are trimmed,
  whitespace runs (tabs, CR) become one space, cut to `PREVIEW_CHARS` (120) with
  "…" by `store::preview` (never inside a character); the renderer trims to the
  box on one line (`clip_notice_lines`).
- **Image:** the entry's thumbnail cover-cropped in the 40 px badge spot with
  `radius::BADGE` corners, never Bata (`NoticeSort::ClipImage`, chosen by
  `Notice::key == CLIP_IMAGE_KEY`). Title "Gambar disalin" ("Screenshot" when
  only bitmap formats were on the clipboard), body "W × H". The bitmap is the
  Clipboard page's `clip_thumbs` cache entry; the notice painter trims that
  cache to 8 so copies with the panel never opened do not pile up. An entry
  gone (cap, clear) draws the Clipboard badge.
- **Files:** "Berkas disalin", body `i18n::clip_files`.
- **Chip** "Riwayat" / "History" (`notice_action`). A click opens the panel on
  the Clipboard page like `SystemLow` opens System; in HoverStay and Click the
  panel is latched (`open_by_click`, see [[open-mode]]). With the Clipboard page
  switched off in "Halaman panel" the click only dismisses. `ClipPreview` is in
  `notice_has_action`, so hover does not open the panel over it and its expiry
  waits while the pointer's visit began on it.
- **Duration** `CLIP_NOTICE_DURATION` = 3 s on the notice deadline.
- **Reducer:** only the latest is kept (it replaces a queued or current
  `ClipPreview` or `Copied`); a `Screenshot` replaces a `ClipPreview` as it does
  `Copied`; dropped when `fullscreen`, and queued ones are dropped on
  `Fullscreen(true)`. A newer `Copied` does not replace a `ClipPreview`.

## Skip rules (`clip_notice_wanted`, `NoticeGate`; tested with fakes)

1. The island's own copies: `win::read` answers `Read::Own` first, and the
   copy from a card (`History::bump`) goes through the same path.
2. Private content: `Ingested::Skipped` (`kinds::should_skip`) never shows and
   is never logged. The notice text never goes to a log.
3. The same entry id (identical content dedupes to one id via
   `Ingested::Bumped`) announced less than `CLIP_NOTICE_REPEAT` = 4 s ago; the
   gate records only an announcement, a refused repeat does not extend the wait.
4. The image went to the screenshot preview (`shot`, see [[screenshot-preview]]).
   A screenshot whose file is still awaited shows the ClipPreview first and the
   preview replaces it.
5. The panel shows the Clipboard page (the card appears there).
6. A fullscreen app (the reducer also guards).
7. The setting is off, or "Riwayat clipboard" is off: the notice names a history
   entry, and with only the screenshot preview listening there is none.

## Setting and "Disalin untuk semua salinan"

`ClipboardSettings.copy_notice` (default true; old configs without it read
true). Settings → Clipboard has "Notifikasi salinan" directly above the "Disalin"
row, faded with the history off. **While it is on it supersedes
`copied_feedback_all` for external copies, even when a rule keeps the preview
quiet** (no "Disalin" then); the "Disalin" row is faded with a desc saying so.
Off, `copied_feedback_all` works exactly as before. "Disalin" for a copy made
from the island is unchanged.

## Power

No polling and no new timer: the existing `AddClipboardFormatListener`
(`WM_CLIPBOARDUPDATE`) is the trigger and the duration rides the notice
deadline ([[bata-idle-power-budget]]).

## Tests and demo

No test and no demo touches the real clipboard (`win::read`, `win::write`,
`EmptyClipboard`, `AddClipboardFormatListener`): tests feed `ClipSnapshot` fakes
and pure functions. Demo scenes `clipboard-notice-text`, `-long`, `-image`,
`-files` (each also `...-plain` without Bata) use fixed made-up data.

## Not verified live

- How the chip, the two text lines and a cropped thumbnail look on the real
  island (only a smoke run of the demo scene without a panic); the "Riwayat"
  chip width is the shared 66 px `ACTION_W`.
- A real copy from Word, Excel, a browser and Explorer: that the notice shows
  once per copy and not for the apps that rewrite the clipboard in a loop
  (the 4 s gate is the guard).
- Whether copying from a password manager that sets none of the excluded
  formats shows a notice (it does: nothing can tell it apart).

## Skip rule 8: managed editor copies (auto-copy, 2026-11-02)

A clipboard write of Vale's screenshot editor that opened with "Salin
otomatis" on (`shot_edit::own_copy(seq)` with `managed`) shows nothing: no
ClipPreview, no "Disalin" (`copied_feedback_all` included), no screenshot
preview. The owner did not copy anything; the card is just updated, one per
capture ([[clipboard-history]], [[screenshot-snap]] "Auto-copy"). The route
decision is the pure `app_clip::own_route` (managed -> `Managed`, anything
else -> today's path). An editor that opened with auto-copy off is not
managed: its Copy keeps today's behaviour (the card may still be added; the
preview skips it through the same registry).
