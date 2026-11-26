# Shelf folders on the phone page, as a zip (design)

Date: 2026-11-26. Small change. Approved by the owner in chat.

## Why

A folder dropped on the shelf never reached the phone/laptop link page:
`phone::Handle::share` (src/phone/mod.rs) and `server::check_shared` accept
only a regular file, so the folder card is skipped. The owner: if a folder
can be dropped on the shelf, it must be shareable too.

## What

- A folder card on the shelf shows on the page's "Dari laptop / From laptop"
  tab as `<folder>.zip`, folder icon, total size, its own Save, and is part of
  "Simpan semua / Save all".
- Save downloads one `.zip` that Vale builds while it sends (stored, no
  compression, no temp file on disk). Every subfolder is inside, entries have
  no root prefix (Windows "Extract all" makes the `<folder>` folder itself),
  empty folders are kept, names are UTF-8, ZIP64 when sizes/offsets/count need
  it.
- Links, symlinks and junctions inside are skipped (same rule as files today);
  a folder that is itself a link is not shared.
- A folder with more than 20,000 files is listed with "too many files" and no
  Save (it is not in Save all).
- A file inside that is gone, locked or changed size while streaming: the
  connection is cut short and the browser shows a failed download.
- No resume: a folder download is always a full 200 (`Accept-Ranges: none`).

## Not changing

- Regular file downloads (Range, ETag, thumbnails) stay as they are.
- A folder dropped on the page from the laptop stays skipped (page.html drop).
- Nothing on the island UI changes.

## Approaches

1. Zip built while streaming (picked): one download, structure kept, no disk
   use, no new crate. No resume.
2. Every file inside listed separately: structure lost, many downloads.
3. Zip to a temp file when dropped: resume works, but disk use and wait.

A browser cannot save a folder as a folder: the File System Access API needs
HTTPS and the link is plain HTTP on the LAN.

## Done

Folder "DA" on the shelf shows as `DA.zip`, Save gives a zip Windows opens
with the full content, `cargo test --locked` green (known Z: drive test aside).
