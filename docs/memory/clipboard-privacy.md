---
tags: [gotcha, clipboard, privacy, tests]
date: 2026-10-28
---

# Clipboard: what the history must never see or touch

Rules behind [[clipboard-history]].

- **Excluded formats** (`kinds::EXCLUDED_FORMATS`, `should_skip`). Present
  means skip: `ExcludeClipboardContentFromMonitorProcessing`, `Clipboard
  Viewer Ignore`. A DWORD that is 0 (or that cannot be read) means skip:
  `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`. KeePass,
  KeePassXC, 1Password and Bitwarden set one or more of these. The reader
  checks the registered formats' names *before* copying anything out; a
  skipped change stores nothing, shows no "Disalin" and is never logged.
- **The island's own copies.** After its `SetClipboardData` and
  `CloseClipboard` the island records `GetClipboardSequenceNumber()`; the
  `WM_CLIPBOARDUPDATE` with that number, or any while `GetClipboardOwner()`
  is the island, adds nothing (`kinds::is_own_change`). The entry copied
  just moves to the front (`History::bump`). The CLI's hidden EDIT is a
  child window, so a Ctrl+C there is the user's copy and is recorded.
- **Busy clipboard.** `OpenClipboard` fails while another app holds it:
  the read retries with a 60 ms one-shot, at most four times (no repeating
  timer). Writing (a card click) waits 3 × 8 ms at most.
- **Never in tests or the demo.** No test and no demo calls `win::read`,
  `win::write`, `EmptyClipboard` or `AddClipboardFormatListener`: they would
  read, change or empty the owner's real clipboard. Tests feed
  `ClipSnapshot` fakes to the pure `ingest`; the demo builds its cards from
  fixed data (`demo_clips`, a fixed "now"). The file tests use a folder under
  `target/test-tmp/`, never `%APPDATA%`.

Listed in [[index]].

## The one picture on disk unencrypted (2026-11-12)

Dragging an image card out writes it as a PNG to `%TEMP%\Vale\drag\<id>\`
(a drop needs a real file). It goes 10 minutes after its last drag, on exit,
on lock with "Hapus saat layar dikunci", on card delete, and the startup sweep
empties the folder. Tests never call `drag::root()` for disk work; file tests
write under `target/test-tmp/`. Details in [[clipboard-drag-out]].
