---
tags: [decision, shelf, phone, zip]
date: 2026-11-26
---

# Shelf folders on the phone page: a zip built while streaming

Spec: `docs/superpowers/specs/2026-11-26-shelf-folder-zip-design.md`. Extends [[shelf-phone]].

- **Bug:** a folder card never showed on the link page's *Dari laptop* tab. The cause sat in
  two places: `sync_plan` in `app_phone.rs` dropped `ShelfKind::Folder` cards, and
  `Handle::share` / `check_shared` took regular files only. Both had to change; tests that
  call `handle.share` directly passed while the app still filtered folders (caught by the
  final review).
- **Why a zip:** a browser cannot save a folder; the File System Access API needs HTTPS and
  the link is plain HTTP. Owner picked one zip over many single downloads (2026-11-26).
- **`phone/zip_stream.rs`:** `plan(root)` walks once (DirEntry metadata, no per-file open;
  links/junctions skipped; `MAX_FILES` 20,000 → `Planned::TooMany`), entries sorted, no root
  prefix, empty dirs kept. Stored (method 0), flags bit 3 (data descriptor, CRC known only
  after reading) + bit 11 (UTF-8). `len()` is exact up front (one `layout()` shared with
  `write`), so the response has a Content-Length. ZIP64 per entry from `zip64_at`
  (0xFFFFFFFF, tests lower it). Own CRC-32 (no new crate). Checked with `tar.exe` in tests.
- **Server:** decides per request: regular file → the old path untouched; plain folder →
  `<name>.zip`, `kind:"folder"`, `application/zip`, `Accept-Ranges: none`, no ETag, Range
  ignored (always 200, no resume); view/thumb 404; too many → listed `big:true`, download 500.
  A file inside that vanishes, is locked or changes size cuts the stream short (never a wrong
  byte). A shared file replaced by a folder of the same path is now served as a zip.
- **Cost:** `/api/files` walks each shared folder on every call (the page only refetches on
  `rev` change and while a Save waits); a big network folder can still be slow.
- **Not done:** a folder dropped on the page from a laptop is still skipped (page.html drop).
- Not verified live with a real phone or Windows "Extract all" (UTF-8 names) at merge time.
