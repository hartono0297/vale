---
tags: [decision, sign, pdf, lopdf, encryption]
date: 2026-11-21
---

# Sign: how the PDF is written

`lopdf` 0.47, default features off (no `image` crate); rendering is Windows.Data.Pdf (0 KB). Part of [[tanda-tangan]].

- **Incremental save** is the normal path: original bytes and the lock stay, another party's digital signature stays verifiable. RC4, AES-128 and AES-256 are tested.
- **Full rewrite only when a page is deleted** (also when the xref had to be repaired). Re-encrypts with the source's `encryption_state`; if that cannot be kept the save is refused (`CannotKeepLock`), never silently unprotected. `save_is_incremental` is exposed so the window can warn.
- **"Deleted page is really gone"** covers content streams, widgets and fields (AcroForm), XObjects through per-page resources (shared ones only if no kept page uses them), outlines and dests to it.
- **Known limits:** `StructTreeRoot` is dropped on delete, so the whole document loses its accessibility tags (tell the owner); `/Pattern`, `/Shading`, `/ExtGState` of a deleted page, XFA datasets and fonts of deleted pages may survive; documents over 256 MiB decoded keep deleted-page images in later undecodable pages; name-tree `/Limits` are not recomputed.
- **Decode budgets:** 64 MiB per page, 256 MiB per save (crafted-PDF OOM guard).
- **Fuzz:** 118k mutations, 0 panics.
- "No changes" PDFs (MODIFY or ANNOTATE permission missing, opens without password) get a warning only; saving stays allowed. PubSec encryption is refused as Broken.

Direction conventions: [[sign-quarter-convention]].
