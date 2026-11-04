---
tags: [decision, shelf, phone, network, security, power, download, range, thumbnail, v3.18, simplified, v3.19]
date: 2026-11-04
---

# The shelf and the phone: send and receive files over a QR

**v3.19 simplified this: scan the QR and you are in, no approval, no code, no ticking.
Read "Simplified (v3.19)" at the end first; lines marked superseded below describe
v3.18 (phase 1 and 2) and are kept for the reasons behind the code that remains.**

The shelf's **HP** pill opens a link: a phone on the same Wi-Fi scans a QR code,
opens a page the island serves, ~~the owner allows it on the notch (a four digit
code on both screens)~~ (superseded: it joins at once), and the phone's files land in
`Downloads\Vale` and on the [[shelf]] as cards tagged "dari HP". Phase 2 (shelf files
to the phone, below; since v3.19 all of them) added routes in `server.rs::route`, a
second tab on the page and a notch view. The agreed mockup is `docs/design/rak-hp.html`
(never committed).

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
  `panel_layout.rs` has `phone_qr/warn/recv_layout` and `phone_page_layout`
  (superseded: `phone_ask_layout` and `phone_send_layout` are gone);
  `render_panel.rs` paints them. The reducer knows only `PhoneStage`
  (`Qr`, `Receiving`; superseded: it had `Approval` and `Send` too) and the hits, like
  the cleaner over System.
- English words are short on purpose (the Pill's title and the shelf tag are
  tight): "From phone" (not "Receiving from phone"), "phone" on the card tag,
  "Finding the network...". `every_phone_string_holds_its_box_in_both_fonts` in
  `render_panel.rs` measures every phone string in the box the painter uses
  (the layouts and `phone_chip_text`) in both fonts, both styles and both
  languages; a text that overflows by a pixel in JetBrains Mono fails it.
- Video files (`ShelfKind::Video`, mp4 m4v mov mkv avi webm wmv 3gp) get the
  camcorder `Icon::Video` in the rows and on the shelf cards ([[shelf]]).
- Demo scenes (superseded, see v3.19 for the list now): `phone-qr`, `phone-qr-expired`,
  `phone-public`, `phone-nolan`, ~~`phone-approve`~~, `phone-receive`, ~~`phone-send`~~;
  the `shelf` scene has two "dari HP" cards.

## Decisions

- **No HTTP crate.** A small HTTP/1.1 handler on `std::net`: it drops a non
  private peer before reading anything, caps the head at 16 KB (431), needs
  `Content-Length` for POST (411), refuses `Transfer-Encoding` (400), takes at
  most 8 connections (dropped over the cap) and uses no pool.
- **Plain HTTP, no HTTPS.** A self-signed certificate gives a red warning page.
  Instead: a 128-bit token in the QR, private peers only. **Superseded in v3.19:** the
  owner's approval with a four digit code is gone, and Public networks are allowed with
  a warning (an unknown category still counts as Public). The risk left: someone on the
  same Wi-Fi who can sniff traffic can read the files, and whoever sees the QR can join.
- **Token (superseded in v3.19):** it was 5 minutes unless a phone was approved with
  it, three rejections voided it (`TokenVoid`), and a phone got a pairing cookie `vp`
  then, on approval, its own `vs`. Now: always 5 minutes, renewed by the island, no
  `vp`, a phone gets its own `vs` when it joins. The cookie is
  `HttpOnly; SameSite=Strict; Path=/`.
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
- **Idle rule:** a page load, a join, an upload (each chunk touches the clock) count;
  polls of a joined phone do **not**, so an open but unused phone tab cannot keep the
  server alive. (v3.18 also counted the polls of a phone still waiting for its answer:
  gone with the waiting page.)
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
- **Stage rules (`app_phone.rs`):** the first upload of a batch sends
  `PhoneAttention(Receiving)` (opens the panel on the shelf, latched like a drop, the
  Hover-mode `DropVisit` rule included, not in fullscreen). Superseded: a phone asking
  sent `PhoneAttention(Approval)` and approval moved to Receiving; a join now changes
  nothing on screen but the chip. Stage changes use the existing 160 ms `Reveal`
  ([[panel-linger-and-motion]]) through `SubView.phone`.
- A late `SessionEnded` from an earlier session is ignored: the app only acts on
  it when its current handle `has_ended()`.

## Gotchas

- **`std::fs::rename` overwrites on Windows.** Never use it for the final move.
- **`SameSite=Strict` and reload:** a reload of `/` sends the cookie, but opening the
  link `/s/<token>` from another app (Notes, a chat) is a cross-site navigation and
  does not, so that phone joins again as a new device (v3.18: the owner had to allow it
  again; now it just counts once more in "Tersambung: n HP", and fails if the token has
  run out). The page does `history.replaceState` to `/` after landing for this reason.
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

(v3.18. See the end of the note for v3.19.) A real iPhone (Safari) and an Android
(Chrome) scanning the QR on one Wi-Fi; the Windows Firewall prompt on the first listen;
a Public network refusal (superseded: Public is allowed now); two phones in one session; PC sleep in the middle of a transfer; a 1 GB+ video; Batal in the
middle of an upload; reloading the phone page after approval; a Wi-Fi with client
isolation (the QR view cannot know nobody connects). What was run: the loopback
end-to-end test, `--phone-debug` with a browser (pairing, auto approval, three
uploads, reload keeps the session, "connection lost" after the server stops) and
the demo scenes by eye.

## Phase 2: send the shelf files to the phone

**Superseded in v3.19:** the owner no longer ticks anything. `PhoneStage::Send` and the
Kirim ke HP view are gone; every file card of the shelf is shared (`phone_sync_shared`).
What follows is the phase 2 text for the serving rules, which are unchanged; read "ticked"
as "on the shelf". The phone's second tab, **Dari laptop**, lists the shared files.
Agreed with the owner: nothing else of the PC is visible.

- **Where:** `phone/share.rs` (the `SharedSet` held inside `Session`, ids, the taken rule,
  `RangeAns`), `phone/files.rs` (path look and open), `phone/thumb.rs` (WIC),
  `http.rs` (`parse_range`, `answer_range`, `content_disposition`, `mime_for`,
  `write_file_head`), `server.rs` (`file_list`, `download`, `thumbnail`, `Routed`).
  Notch side: `app_phone.rs` keeps a mirror of the set keyed by the shelf card key
  (v3.19: `Mirror { key, id }`; v3.18 also had `size` and `taken`), synced after every
  shelf change, cleared with the session. Superseded: `phone_send_layout` /
  `paint_phone_send`, demo scene `phone-send`.
- **Routes** (all need a joined device's `vs`; anything else, an old `vp` included, an
  unknown or revoked id: the bare 404): `GET /api/files` (id, name, size read now, `v` (the
  file's version: its strong ETag, size and mtime, without the quotes), kind, `view`,
  `saved` and `busy` for that phone, plus `rev`; never a path), `GET|HEAD
  /api/file/<id>` (attachment), `GET|HEAD /api/view/<id>` (images, inline), `GET
  /api/thumb/<id>?v=<v>` (the page puts the version in the URL, so a file written over
  in place gets a new thumbnail and the phone's cache never shows the old one). `/api/state` of an approved phone also carries `rev` and `shared`
  (the tab count and the page's refresh trigger).
- **Ids:** 16 random bytes from the session's own filler (base64url, 22 characters),
  compared in constant time over every entry; a path already shared (by `shelf::key_of`)
  gives its id again; at most 12; a failing filler shares nothing. Revoked on untick, card
  removal, "Kosongkan", session end (the thread calls `unshare_all` in `end()`).
- **Network paths (UNC or mapped) can be ticked, but are never looked at on the UI
  thread** (a slow or gone share could hold it for seconds). `Handle::share` sends any
  path where `shelf::is_network` is true to `share_unseen`: the id is made at once with
  `ShareInfo { size: None }`, and a `phone-share-check` thread runs
  `server::check_shared`. Superseded in v3.19: `ShareInfo` and `SharedSized` are gone
  (v3.18 filled in the send footer's size); a file that is not a regular file has its
  id revoked and emits `SharedGone`, and the app puts its key on the loop guard's list
  and looks at the shelf again (the card turns stale if it is a local file). A thread that
  cannot be spawned unshares the id and shares nothing. `shelf::probe` does not look at
  network paths either. Tested by the e2e
  `a_file_on_the_network_is_shared_without_being_looked_at_then_sized_or_revoked`.
- **The taken rule** (`counts_as_taken`): only a GET of `/api/file` or `/api/view` that wrote
  every byte of the whole body (200) or of a resume `bytes=N-` running to the last byte.
  Never HEAD, a suffix `bytes=-N`, a closed `a-b` (even one reaching the end) or a
  thumbnail. A zero-length file served whole counts. `saved` is per device (the page's
  "Tersimpan"); superseded: `FileTaken` (once per id, for the notch's "diambil") is gone.
  `RangeAns::Partial` carries `open_ended` for this.
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
  cached per id in the set with the ETag it was made from (a few KB; a new version is
  made again), sent with `Cache-Control: private, max-age=3600`.
  Files over 64 MB, a HEIC without the codec or anything else: 404 and the page keeps the
  type icon. **Never the full image as a fallback**: a phone tab decoding a dozen 12 MP
  photos can crash. No new `windows` feature was needed.
- **Idle rule:** the start of `/api/file` and `/api/view` and every 64 KB chunk call
  `Session::touch`; `/api/files`, `/api/thumb` and `/api/state` polls do not
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
- **Notch rules (superseded in v3.19, the whole send view is gone):** v3.18 let files arriving
  while the owner ticked not pull the view away, redrew the send view at `FileTaken`, and
  fitted the tags "folder", "hilang" and "diambil" on the send cards (`phone_send_name`,
  `the_taken_tag_never_sits_on_a_cards_name`). Kept: a shared file whose card turns stale is
  unshared by the sync. The Shelf page's "dari HP" tag still meets a one-line name with the
  strip (an older compromise, not changed).

### Not verified live (phase 2, v3.18)

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

## Simplified (v3.19): QR only, whole shelf shared

Owner's decision, in his words: "asalkan punya QR nya uda bisa send atau terima file,
gitu aja ga perlu ribet". Whoever scans the QR is in; nothing is approved, compared or
ticked. This section is the current behaviour; statements in the older sections that
say otherwise are marked **superseded**.

**Removed:** the four-digit code (`random_code`), the owner's approval view and
`PhoneAttention(Approval)`, the waiting and rejected pages of the phone, the pairing
cookie `vp`, rejections / the void token (`MAX_REJECTIONS`, `TokenVoid`, `APPROVE_TTL`,
`MAX_PENDING`), `TokenKept` (a phone no longer keeps the token alive), the Kirim ke HP
tick view (`PhoneStage::Send`, `Hit::PhoneSend` / `PhoneShareToggle` / `PhoneSendDone`,
`phone_send_layout`, `paint_phone_send`, `SendView`, `CardState`), `PhoneEvent::FileTaken`
and the notch's "diambil" tag (the page's own "Tersimpan" per phone stays),
`PhoneEvent::SharedSized` and `ShareInfo.size` (`Handle::share` returns the id), the
`Browser` of the User-Agent, and the Public refusal (`Refusal::Public`,
`net::start_allowed`, `Warn::Public`, `PhoneWarnPublic*`).

**New rules**

- **Join on token.** `Session::join(token, now)`: a valid token (`same`, constant time;
  younger than `TOKEN_TTL` 300 s) makes a device with its own random 16-byte `vs` cookie
  at once; that is activity. The token **always** expires 5 minutes after it was made,
  whoever joined (no "kept"); `new_token()` makes a fresh one (same CSPRNG filler, 16
  bytes, 22 characters). Phones that joined keep their `vs` until the session ends. A
  phone that already has a valid `vs` and opens the link again gets the page and no new
  device, whatever the token. `MAX_DEVICES = 64` is the only limit on how many join; over
  it a valid link is a bare 404. Wrong, expired, replaced token, no random bytes, full:
  the bare 404 (no cookie, no event). `GET /`, `/api/state`, `/api/files`, `/api/upload`
  and the file routes need a valid `vs`; no cookie, an old `vp` or a made-up one: bare
  404. `/api/state` still says `"state":"approved"` (kept as the word the page reads).
- **Idle:** page load, join, upload chunks, the start and every 64 KB chunk of
  `/api/file` and `/api/view` count; `/api/state` polls, `/api/files` and `/api/thumb`
  do not.
- **Auto-rotation.** While the QR view is on screen the code always counts down
  ("berlaku m:ss") and, when it ran out (`TokenExpired`, the 1 s `TIMER_PHONE`, or the QR
  view coming back with a dead code), the app calls `Handle::new_token()` by itself
  (`phone_keep_fresh`). Only if that returns `None` (no random bytes) does the dimmed code
  + "QR habis" + "Buat QR baru" show (`renew_failed`; the `phone-qr-expired` scene).
  `TIMER_PHONE = 48` is still the only timer, one-shot, re-armed each second only while
  the QR view with a counting code is on screen. Phones that joined are not touched by a
  new token. Behind another view an expired token waits and is renewed when the QR view
  shows again.
- **Whole shelf shared, `phone_sync_shared`.** While a session listens (at `Ready`, and at
  every `shelf_look`, which every shelf change ends with: drop, file from a phone, card
  removed, Kosongkan) every card that is a file (not a folder), not stale, is shared once;
  a mirror entry whose card left the shelf or turned stale (or became a folder) is
  unshared. Pure decision: `sync_plan(cards, mirror, skipped)`. Network paths keep the
  phase 2 rule (never looked at on the UI thread: `share_unseen` + `phone-share-check`).
  **Loop guard:** `skipped` holds the keys of cards whose `share` returned `None` or whose
  id came back as `SharedGone`; they are not retried while the card stays on the shelf
  (pruned by `prune_skipped`; a key also goes when its card is stale, so a file that is
  back gets a fresh try). Without it a gone network file would be shared again on every
  sync. Files received from a phone are on the shelf, so they are listed too. The mirror
  is `{key, id}` only; ids are unchanged (16 random bytes, max 12 = the shelf's cap, the
  path never leaves the PC, same path gives the same id).
- **Notch.** The QR view stays while the session runs; `Joined` only redraws (never opens
  the panel or changes the stage) and puts the kind on the chip. Step 2 reads "Kirim atau
  ambil berkas di HP" with "Semua berkas di rak ikut tampil", and once a phone joined
  "Tersambung: Android" / "Tersambung: 2 HP" (`i18n::phone_connected`). The device count
  is one per join, so a phone that opens the link from another app (no `vs` sent:
  SameSite=Strict) counts again. `resume_stage()` is Receiving if any phone joined or rows
  exist, else Qr; `phone_back` ends a session no phone ever joined. The receiving view lost
  its "Kirim ke HP" button, nothing else.
- **Public networks** are allowed: `net::network_trust(category) -> Trust` (Private and
  Domain are Private; Public and unknown are Public). `detect_lan` never refuses for it;
  `public` goes into `ServerConfig`, `Runtime` and `PhoneEvent::Ready`. On the QR view the
  network line reads "<name> · publik" / "public" and the ochre hint box shows
  "Wi-Fi publik — jangan dipakai di kafe/hotel" / "Public Wi-Fi — don't use at a café or
  hotel" instead of the firewall text (it fits in both fonts, both styles; checked by
  `every_phone_string_holds_its_box_in_both_fonts`). Vale does not elevate, run netsh or
  add firewall rules.
- **Firewall gotcha (docs only):** on a Public network Windows Firewall may block the
  incoming connection unless Vale is allowed on public networks; the phone then cannot
  connect and nothing on the notch can tell. It is in the README and the guide, not on the
  notch.

**Residual risk (accepted by the owner's request):** anyone on the same network who can
see the QR while it is on screen (a camera, a screen share, someone looking) can send files
to `Downloads\Vale` and download the whole shelf; plain HTTP on the Wi-Fi can be read by
someone who can sniff it, and a Public network makes that likelier. The limits that
remain: local addresses only, 5-minute token, 64 devices, the shelf's 12 files, no
folders, no paths, nothing outside the shelf, a revoked id stops at once, session end
drops everything.

**Demo scenes** now: `phone-qr`, `phone-qr-joined` (step 2 "Tersambung: Android"),
`phone-qr-expired`, `phone-public` (QR view with the Public warning, "Kenzie_5G ·
publik"), `phone-nolan`, `phone-receive`. `phone-approve` and `phone-send` are gone.
`--phone-debug` has no approval loop: it prints each `Joined`.

**Not verified live (v3.19):** a real iPhone and Android scanning the QR on a Private and
a Public network (and the Firewall behaviour on Public); two real phones at once; the
rotation after five real minutes on the notch (the rules are unit-tested, the loopback test
covers a replaced token); the new demo scenes by eye in DM Sans and JetBrains Mono
(only the fit test measured them). Run: `cargo test --locked` (session rules, network
trust, sync plan, loopback end to end).
