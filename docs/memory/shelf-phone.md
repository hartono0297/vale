---
tags: [decision, shelf, phone, network, security, power, download, range, thumbnail, v3.18]
date: 2026-11-04
---

# The shelf and the phone, phase 1: receive from a phone over a QR

The shelf's **HP** pill opens a link: a phone on the same Wi-Fi scans a QR code,
opens a page the island serves, the owner allows it on the notch (a four digit
code on both screens), and the phone's files land in `Downloads\Vale` and on the
[[shelf]] as cards tagged "dari HP". Phase 2 (shelf files to the phone, below) added
routes in `server.rs::route`, a second tab on the page and a notch view. The
agreed mockup is `docs/design/rak-hp.html` (never committed).

## Where it lives

- `src/phone/` has no UI code and is tested without a network: `session.rs`
  (the rules, injected clock and random filler), `http.rs` (head parser with
  hard limits, response writer), `names.rs` (sanitise, number a clash), `ua.rs`,
  `net.rs` (private ranges, adapter choice, Network List Manager, the bound
  check), `qr.rs` (`qrcodegen` 1.8.0, logo rule), `server.rs` (the one thread),
  `page.html` (one file, 14 KB, no CDN, no font), `mod.rs` (public API),
  `debug.rs` (`--phone-debug`, debug builds), `e2e.rs` (loopback test).
- `src/app_phone.rs` (a child of `app`) keeps the `Handle` and what the events
  said; `ui/phone_view.rs` is the data the renderer borrows;
  `panel_layout.rs` has `phone_qr/warn/ask/recv_layout` and `phone_page_layout`;
  `render_panel.rs` paints them. The reducer knows only `PhoneStage`
  (`Qr`, `Approval`, `Receiving`) and the hits, like the cleaner over System.
- English words are short on purpose (the Pill's title and the shelf tag are
  tight): "From phone" (not "Receiving from phone"), "phone" on the card tag,
  "Finding the network...". `every_phone_string_holds_its_box_in_both_fonts` in
  `render_panel.rs` measures every phone string in the box the painter uses
  (the layouts and `phone_chip_text`) in both fonts, both styles and both
  languages; a text that overflows by a pixel in JetBrains Mono fails it.
- Video files (`ShelfKind::Video`, mp4 m4v mov mkv avi webm wmv 3gp) get the
  camcorder `Icon::Video` in the rows and on the shelf cards ([[shelf]]).
- Demo scenes `phone-qr`, `phone-qr-expired`, `phone-public`, `phone-nolan`,
  `phone-approve`, `phone-receive`, `phone-send` (phase 2); the `shelf` scene has two "dari HP" cards.

## Decisions

- **No HTTP crate.** A small HTTP/1.1 handler on `std::net`: it drops a non
  private peer before reading anything, caps the head at 16 KB (431), needs
  `Content-Length` for POST (411), refuses `Transfer-Encoding` (400), takes at
  most 8 connections (dropped over the cap) and uses no pool.
- **Plain HTTP, no HTTPS.** A self-signed certificate gives a red warning page.
  Instead: a 128-bit token in the QR, the owner's approval of each phone with a
  four digit code (unbiased by rejection sampling, `< 60000` of a `u16`), private
  peers only, and Public networks refused (an unknown category counts as Public,
  the safe default). The risk left: someone on the same Wi-Fi who can sniff
  traffic can read the files.
- **Token:** 5 minutes unless a phone was approved with it; then it lives until the
  session ends. Three rejections (Tolak or a 60 s time out) void it
  (`TokenVoid`); "Buat QR baru" starts over. A phone gets a pairing cookie `vp`
  (opens only `GET /` and `GET /api/state`), and on approval its own random
  session cookie `vs` (the next `/api/state` sets it and clears `vp`). Both
  are `HttpOnly; SameSite=Strict; Path=/`.
- **Stop before bind:** `server::run` looks at the stop flag after the adapters
  were read and before `bind`; a Back pressed meanwhile ends the session
  (`SessionEnded`, `ended`) with nothing listening, so no Firewall prompt
  appears afterwards. Tested with `Mode::Fixed` and the flag already set.
- **Session end:** Selesai, Back with nothing connected, 10 idle minutes, the
  address leaving the adapters (`BoundCheck`, read at most every 5 s on a new
  connection and on the loop tick), sleep (`PBT_APMSUSPEND`), lock, and a session
  left over on resume. The stop never waits for the thread (`Handle::stop_with`
  and `Drop` do not join; only the test-only `stop()` does), so the UI thread
  never blocks on a Windows network call.
- **Idle rule:** a page load, an upload (each chunk touches the clock) and a poll
  of a phone still waiting for its answer count; polls of an approved phone do
  **not**, so an open but unused phone tab cannot keep the server alive.
- **Files:** `<name>.part` opened with `create_new`, 64 KB chunks, moved to the final
  name with `MoveFileExW` **without** `MOVEFILE_REPLACE_EXISTING` (looping
  `IMG_1 (2).png`). Max 4 GiB a file (413), 1 GiB must stay free (507), a
  cancelled batch gets 409. Cancel, stop or a broken connection deletes the
  `.part`. Names are cut to the last path part, lose control characters and
  `<>:"|?*` and `..`, Windows device names get an `_`, "berkas"/"file" when empty.
  Format and bidi controls are dropped too (`names::is_format`: the whole Cf
  category by explicit ranges, plus U+2028/2029), so a right-to-left override
  cannot make `photo<RLO>gnp.exe` read as `photoexe.png`; the zero width joiner
  goes with them, so emoji ZWJ sequences fall apart (accepted, the name is a
  label). The cap is **150 UTF-16 units**, not characters (an emoji is two), cut
  on a character boundary with a short extension kept, so the name plus
  ` (9999)` and `.part` stays under NTFS's 255 units a component.
- **Shelf landing:** `shelf::phone_landing(len, batch)`: a batch that would pass 12
  cards gives one card for `Downloads\Vale` instead of its files (deduped). Taking
  a card off or "Kosongkan" never deletes a file (tested).
- **Power:** the server thread exists only while a session runs (accept loop sleeps
  200 ms with `park_timeout`, so 5 wake-ups a second during a session only). One
  one-shot timer, `TIMER_PHONE` (48; 42-47 belong to other features), re-armed each second only while the QR
  view with a counting code is on screen. Progress events (at most 6 a second)
  draw only the receiving view and never refresh the island. An idle island is
  unchanged ([[bata-idle-power-budget]]).
- **Stage rules (`app_phone.rs`):** a phone asking sends `PhoneAttention(Approval)`
  (opens the panel on the shelf, latched like a drop, the Hover-mode `DropVisit`
  rule included, not in fullscreen); approval moves to Receiving, a rejection back
  to Qr (to Receiving if a phone is already in); the first upload of a batch sends
  `PhoneAttention(Receiving)` but never over a waiting phone. Stage changes use the
  existing 160 ms `Reveal` ([[panel-linger-and-motion]]) through `SubView.phone`.
- A late `SessionEnded` from an earlier session is ignored: the app only acts on
  it when its current handle `has_ended()`.

## Gotchas

- **`std::fs::rename` overwrites on Windows.** Never use it for the final move.
- **`SameSite=Strict` and reload:** a reload of `/` sends the cookie, but opening the
  link `/s/<token>` from another app (Notes, a chat) is a cross-site navigation and
  does not, so that phone pairs again and the owner must allow it again. The page
  does `history.replaceState` to `/` after landing for this reason.
- `qrcodegen::encode_text` boosts the error correction when it costs no size, so
  the logo rule reads the level that came out, not the one asked for. A typical
  link (`http://192.168.100.200:47399/s/<22>`, 53 bytes) is version 6 at High (41
  modules); the 7 x 7 plate is 2.9 % of it.
- The coverage test of the sources lists U+2028 / U+2029 as "not drawn" (the page's
  boot JSON escapes them; `<`, `>` and `&` too).
- Test spawns, ports: the e2e tests bind `127.0.0.1` port 0 only, write under
  `target/test-tmp/phone-e2e-<pid>`, read no adapters and no Network List
  Manager ([[test-spawns-no-window]], [[clipboard-privacy]] for the same rules).
- The **page cap** is 30 KB; it is 14 KB. All page strings live in one `id`/`en`
  dict, chosen by the boot JSON's `lang`.
- A full shelf (12 cards) cannot get the folder card either; the receiving view
  still lists the files.

## Not verified live

A real iPhone (Safari) and an Android (Chrome) scanning the QR on one Wi-Fi; the
Windows Firewall prompt on the first listen; a Public network refusal; two phones in
one session; PC sleep in the middle of a transfer; a 1 GB+ video; Batal in the
middle of an upload; reloading the phone page after approval; a Wi-Fi with client
isolation (the QR view cannot know nobody connects). What was run: the loopback
end-to-end test, `--phone-debug` with a browser (pairing, auto approval, three
uploads, reload keeps the session, "connection lost" after the server stops) and
the demo scenes by eye.

## Phase 2: send the ticked shelf files to the phone

The owner ticks shelf cards in **Kirim ke HP** (`PhoneStage::Send`, reached from the QR
or the receiving view, left with Selesai or Back to the same one, never ends the session;
`resume_stage()` never returns it). The phone's second tab, **Dari laptop**, lists
only those files. Agreed with the owner: nothing else of the PC is visible.

- **Where:** `phone/share.rs` (the `SharedSet` held inside `Session`, ids, the taken rule,
  `RangeAns`), `phone/files.rs` (path look and open), `phone/thumb.rs` (WIC),
  `http.rs` (`parse_range`, `answer_range`, `content_disposition`, `mime_for`,
  `write_file_head`), `server.rs` (`file_list`, `download`, `thumbnail`, `Routed`).
  Notch side: `app_phone.rs` keeps a mirror of the set keyed by the shelf card key
  (`Mirror { key, id, size, taken }`), pruned after every shelf change, cleared with the
  session; `phone_send_layout` / `paint_phone_send`; demo scene `phone-send`.
- **Routes** (all need an approved device's `vs`; anything else, `vp` included, an unknown
  or revoked id: the bare 404): `GET /api/files` (id, name, size read now, kind, `view`,
  `saved` and `busy` for that phone, plus `rev`; never a path), `GET|HEAD
  /api/file/<id>` (attachment), `GET|HEAD /api/view/<id>` (images, inline), `GET
  /api/thumb/<id>`. `/api/state` of an approved phone also carries `rev` and `shared`
  (the tab count and the page's refresh trigger).
- **Ids:** 16 random bytes from the session's own filler (base64url, 22 characters),
  compared in constant time over every entry; a path already shared (by `shelf::key_of`)
  gives its id again; at most 12; a failing filler shares nothing. Revoked on untick, card
  removal, "Kosongkan", session end (the thread calls `unshare_all` in `end()`). Network
  paths (UNC or mapped) cannot be ticked: looking at them could hold the UI thread.
- **The taken rule** (`counts_as_taken`): only a GET of `/api/file` or `/api/view` that wrote
  every byte of the whole body (200) or of a resume `bytes=N-` running to the last byte.
  Never HEAD, a suffix `bytes=-N`, a closed `a-b` (even one reaching the end) or a
  thumbnail. A zero-length file served whole counts. `FileTaken` is sent once per id (any
  phone); `saved` is per device. `RangeAns::Partial` carries `open_ended` for this.
- **Range:** `a-b` (b clamped), `a-`, `-n` (n clamped); start past the end, `a > b`, `-0`,
  any range on an empty file, another unit, non-digits, overflow: 416 with `Content-Range:
  bytes */len`. A **multi-range** (a comma) is ignored and answered 200 in full, which
  RFC 9110 allows. An `If-Range` that matches neither the strong ETag (size and mtime) nor
  the `Last-Modified` text gets the whole file, even when the range was unsatisfiable.
- **SVG is never inline:** `/api/view` and `/api/thumb` answer 404 for `.svg`, and
  `/api/file` sends it as `application/octet-stream` with `attachment` and `nosniff`, so a
  shared SVG cannot run script on the page's origin. `mime_for` simply has no SVG entry.
- **Content-Disposition:** an ASCII fallback (every non-ASCII char, control, `"` and a backslash
  is one `_`) plus `filename*=UTF-8''` percent-encoded by RFC 5987 attr-chars; no CR or LF can
  reach a header (`write_file_head` also drops any value that has one).
- **Thumbnail:** WIC on the connection thread (COM joined multithreaded for the call):
  `CreateDecoderFromFileHandle` on the file already opened with the share flags, frame 0,
  EXIF orientation 3, 6 or 8 through a flip-rotator, Fant scaler to at most 192 px on the
  long side (never enlarged), 24bpp BGR, JPEG at quality 0.8 into a memory stream,
  cached per id in the set (a few KB), sent with `Cache-Control: private, max-age=3600`.
  Files over 64 MB, a HEIC without the codec or anything else: 404 and the page keeps the
  type icon. **Never the full image as a fallback**: a phone tab decoding a dozen 12 MP
  photos can crash. No new `windows` feature was needed.
- **Idle rule:** the start of `/api/file` and `/api/view` and every 64 KB chunk call
  `Session::touch`; `/api/files`, `/api/thumb` and approved `/api/state` polls do not
  (`device_for_view_api` does not touch), so an open page cannot keep the server alive.
  Tested at session level and with a real socket pair (`stream_body`).
- **Serving:** the path is looked at every time: `symlink_metadata` must say a regular file
  and not a symlink (std's `is_symlink` is true for every name-surrogate reparse point:
  symlinks and junctions), then it is opened with `FILE_SHARE_READ | WRITE | DELETE` (a file
  in use elsewhere serves) and the handle's `metadata()` must say `is_file`. Other reparse
  points, **OneDrive cloud files** (Desktop and Documents on Windows 11), are ordinary
  files and are served (opening hydrates them). Missing or no longer regular: **410**, the
  id is revoked, `SharedGone` is sent once, later requests are 404. A file that exists but
  cannot be opened (exclusive lock): 500, the id stays. **Residual race:** a path swapped for
  a link between the `symlink_metadata` look and the open is followed; the opened handle's
  `is_file` closes the folder case only.
- **Streaming:** 64 KB chunks, no whole-file buffering, seek to the range start, the 30 s
  write timeout and the 8 connection cap as for uploads. Between chunks it stops when the
  session stopped or the id is no longer shared (an untick cuts the download: the
  connection is closed short). The device is `busy` for that id while streaming (a guard,
  cleared in every case, after `mark_saved`). No progress events go to the app.
- **Page:** the tab count and refresh come from the same 5 s approved poll (no new timer);
  the list is fetched when the tab opens and when `rev` differs; thumbnails load one at a
  time (the 8 connection cap); Save does a HEAD first (404 or 410: "Tidak dibagikan
  lagi"); iPhone images open `/api/view` in a new tab, everything else is a download link;
  **Simpan semua** goes one file after another through `/api/file`, polling `/api/files`
  every 2 s until saved, and moves on after 3 polls that are neither saved nor busy. The
  hint shows the iPhone or Android part by user agent, both when unknown (iPadOS reports
  as a Mac: `maxTouchPoints > 1`). Tests: the page under 30 KB, `id` and `en` dictionaries
  with the same keys, no `innerHTML` but the logo, no outside address.
- **Notch rules:** files arriving while the owner ticks do not pull the view away (the
  reducer remembers Receiving as the stage to return to); `PhoneAttention(Approval)` over
  Send works as before. A ticked card whose file is gone turns stale and is unshared at
  `SharedGone`; `FileTaken` redraws only the open send view (no refresh). The fit test tags
  ("folder", "hilang", "diambil") may enter the 24 px icon box's empty margin (about 3 px)
  in JetBrains Mono, never the drawing.

### Not verified live (phase 2)

A real iPhone Safari (a photo to Photos by long press, a video to Files, Save all) and an
Android Chrome (Download folder, Gallery) on one Wi-Fi; a 1 GB+ video download and a
resume; an untick while a phone looks (the loopback test cuts a 24 MB download); a file
deleted after ticking; two phones taking one file (tested on loopback); the Firewall
prompt still only on the first listen. Run: `cargo test --locked` (loopback e2e);
`--phone-debug` shares `target/phone-debug/share/` and prints each id (checked in a
browser pane: list, thumbnail, tab switch; no download was clicked); the `phone-send`
scene was drawn in DM Sans and JetBrains Mono, ID and EN, Pill and Notch.

Exe size: +115,712 bytes (3,235,328 to 3,351,040, +3.6 %) on `main` f83dbf0, see
[[exe-size]].

Related: [[shelf]], [[shelf-motion]], [[bata-idle-power-budget]], [[exe-size]],
[[panel-linger-and-motion]].
