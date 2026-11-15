---
tags: [spec, shelf, phone, clipboard]
date: 2026-11-15
---

# Copy the phone link (so a laptop can join too)

## Why

Owner's words: "bisa ga bikin user bisa copy url, jadi bisa antar laptop, ga cuma antar hp".
The shelf's phone link only lets you in by scanning the QR, so another laptop can't
practically join. The server already accepts any private peer (Wi-Fi mode) or tailnet
peer (Tailscale mode), phone or laptop. What's missing is a way to get the URL out,
plus a page that's comfortable on a laptop.

## Goals

- A copy icon on the QR view's network card copies the full link, exactly what the QR
  encodes (`Runtime::url(token)`, `http://<addr>:<port>/s/<token>`, as carried by
  `PhoneEvent::Ready.url` and `NewLink.url`), for the token and
  network mode that are live right now.
- Feedback: the icon turns into a check for about 2 seconds, then back.
- Step 1's subtitle becomes "Atau salin link-nya ke laptop" / "Or copy the link to a laptop",
  replacing the camera hint ("iPhone: Kamera · Android: ..."), as in the approved mockup
  (there is no room for a third line above step 2).
- The phone page accepts files dragged from Explorer or Finder anywhere on the page and
  uploads them the same way as the file picker. While files are dragged over the page it
  shows "Lepas untuk kirim" / "Drop to send".

## Non-goals

- No change to the session rules: the token still lives 5 minutes and is renewed by the
  island, a joined device keeps its `vs` until the session ends, same peer checks.
- No desktop layout for the page (it stays one column, max 480 px): the content is one list.
- No new way to share the link (no email, no chat, no share sheet).

## Anti-goals

- The copy must not put the link in Vale's own clipboard history or flash the clipboard
  notice (it is the island's own copy, already skipped by `kinds::is_own_change`).
- No new timer and no continuous redraw at idle ([[bata-idle-power-budget]]).
- Nothing in tests or the demo touches the real clipboard ([[clipboard-privacy]]).

## Decisions (owner's)

- Placement: option A, an icon button at the right end of the network card (chosen from
  three mockups: icon on the card, labelled "Copy link" button on the card, button under
  the QR).
- The copy is a plain `CF_UNICODETEXT` write: Windows clipboard history (Win+V) and
  "Sync across devices" are **not** suppressed, so with cloud sync on the link reaches the
  owner's other PC on its own. Accepted risk: the link passes through Microsoft's cloud
  clipboard and sits in Win+V; the token is dead after 5 minutes.
- Drag and drop on the page: yes; layout unchanged.

## Approaches

One sensible way, the builder's calls inside it:

- **Copy:** the same path as `app_search::copy_search_result` (`win::write` with the text
  formats, record the sequence so the history skips it). Rejected: writing through the
  clipboard history (would add a card), the copy-preview notice (the owner chose the
  in-place check).
- **Check revert:** the QR view already re-arms the one-shot `TIMER_PHONE` every second
  while a counting code is on screen, and the icon only exists then; keep a `copied_until`
  instant and clear it on that tick. Rejected: a new timer.
- **Page drop:** `dragover`/`drop` on the document, reusing the upload queue the file
  input feeds. Rejected: a separate drop zone element (one more box on a phone that never
  drags).

## Rules

- The icon exists (drawn and hit-tested) only on the QR view with a live, counting code.
  Not on "Mencari jaringan…", the dimmed "QR habis" state, the "Tailscale belum jalan"
  card or the receiving view. A copy failure (clipboard busy after the usual retries)
  leaves the icon as it was; no error text.
- A token renewal while the check shows ends the check (the copied link is now stale).
- New hit `Hit::PhoneCopyLink`, effect to the app, the app reads the current URL from the
  running `Handle` (never a cached string), writes it, sets the check.
- Strings fit their boxes in DM Sans and JetBrains Mono, ID and EN, Pill and Notch
  (`every_phone_string_holds_its_box_in_both_fonts` grows to cover the new line). The icon's
  hit box is a square as tall as the card, so it stays easy to hit.
- Page: dropping folders or nothing does nothing; several files make one batch like a
  multi-select; the drop hint uses the existing dictionary (`id`/`en`, same keys tested);
  page stays under 30 KB, no `innerHTML`, no outside address.
- Demo scene `phone-qr` shows the icon; a `phone-qr-copied` scene shows the check.

## Left to the builder

Exact icon glyph (existing `Icon` set, a copy glyph is added if none fits), its size and
inset in the card, the hint's look on the page (reuse the page's card and accent tokens),
test names. README privacy text: add one line that the link can be copied and pasted into
a laptop browser, and that anyone holding a fresh link can join (same as the QR).

## Done

Merged to `main`, `cargo test --locked` green, memory (`shelf-phone`) and README updated,
`main` pushed. Owner checks by hand: copy on Wi-Fi and Tailscale, open in another laptop's
browser, send a file by drag and drop, take a file; Win+V shows the link.
