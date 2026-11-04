---
tags: [decision, screenshot, clipboard, privacy, power]
date: 2026-10-28
---

# Screenshot preview in the notch

Run 8 of the v3.1 plan (keputusan #6). Code: `src/screenshot/` (pure:
matching, PNG/JPEG headers, folder listing; Win32: `win`) and
`src/app_shot.rs` (the island's side). The Pintasan Screenshot tile ran
`ms-screenclip:` (now Vale's own capture by default, see [[screenshot-snap]]).

- **Detection.** A new clipboard image from the history's listener
  ([[clipboard-history]], `clip_image_arrived`) becomes a candidate. With
  only bitmap formats on the clipboard (PrtSc, some Snipping Tool paths) the
  preview shows at once; any other image shows only once its file is found
  in the Screenshots folder (Win+Shift+S with auto-save, PrtSc to Snipping
  Tool). A browser's "Copy image" never matches a file, so it never shows.
  The folder comes from `FOLDERID_Screenshots` (follows OneDrive), never a
  guessed path.
- **Matching** (`screenshot::pick_match`, pure, tested): a file counts from
  3 s before to 5 s after the clipboard image, with the very same width and
  height, and an 8 × 8 average hash (`thumb_hash`) within 10 bits of the
  clipboard thumbnail's. The file side is decoded by WIC at the clipboard
  thumbnail's size (`thumb_size`) so both hashes see alike pixels. A file
  whose hash could not be read (still being written) is allowed on time and
  size but loses to a hash-confirmed one; then the closest in time wins.
- **No polling.** One short-lived worker lists the folder when an image
  arrives; `TIMER_SHOT_RETRY` (23, one-shot) looks twice more (1.0 s, 1.8 s
  later). Then nothing runs until the next image ([[bata-idle-power-budget]]).
- **One card.** The screenshot is the history's image card; the file is
  never ingested, so file and clipboard never make two cards. PrtSc twice on
  an unchanged screen bumps the card and previews it again.
- **The notice.** `NoticeKind::Screenshot` on `ShapeKind::NoticeShot`
  (440 × 156, radius 26): the 230 × 136 thumbnail (drawn whole), Bata happy
  or the Camera badge, "1280 × 720 · 245 KB" (the file's size once found,
  else the bitmap's) and "sudah disalin", then Salin lagi / Simpan / Hapus.
  It shows `show_secs` (8 by default) from when it becomes current, never
  expires under the pointer, and gets 2 s more after the pointer leaves. A
  newer screenshot replaces it (and a "Disalin"); never over a fullscreen
  app. "Disimpan · Pictures\Screenshots" (happy) and "Dihapus · clipboard
  dikosongkan" (Bata at rest) follow the buttons.
- **Clicks** use the press-release rule ([[media-click-guards]]) through
  `shot_regions`; the thumbnail opens the file in its default app
  (`ShellExecuteW`), the island never takes the foreground.
- **Nothing is deleted without Hapus.** Only `Hit::ShotDelete` gives
  `ShotAction::Delete`. "Jika diabaikan: Hapus" (opt-in; Simpan is the
  default) gives `ShotAction::DeleteIgnored` only when the preview ran its
  full time with no visit, no open and no other click, and `App` deletes
  then only if the setting is still Hapus (`delete_when_ignored`). Expiry,
  Body click, a newer shot or a game always keep the file.
- **Hapus** moves the file to the Recycle Bin through the cleaner's
  `IFileOperation` recycle on an STA worker (no confirmed file yet: it waits
  for one, see the 3.2.1 fixes below), removes the history card, and empties the clipboard
  only while `GetClipboardSequenceNumber` is still the one read with the
  image (checked again with the clipboard open, `should_empty`).
- **Settings → Clipboard**, a second card under an eyebrow "Screenshot":
  Preview di notch (on), Jika diabaikan (Simpan | Hapus, Simpan), Lama tampil
  (4–30 s, step 2, default 8). The section now scrolls
  ([[settings-appearance-scroll]]). Old config.json without `screenshot`
  loads the defaults. The clipboard listener runs while the history **or**
  the preview is on; with the history off only images are looked at and
  nothing is kept.
- **Tests and demo** never touch the real clipboard, folder or files: the
  matching runs on fakes, the folder test on `target/test-tmp/`, the demo
  (`notch:screenshot`, `-saved`, `-deleted`) draws a fixed picture with
  fixed figures ([[clipboard-privacy]], [[screenshot-capture-script]]).

## Verify pass (2026-10-28)

- **Never another screenshot's file.** `ClipCandidate::from_ms`
  (`claim_from`): when an image arrived shortly before, files before this
  one's arrival belong to that one. `Shots::taken` (last 16) lists files
  already matched; they are never matched again. Before this, a retake within
  a few seconds could match the earlier file, and Hapus recycled it.
- Simpan without a file (PrtSc with no auto-save) says "Belum ada file · tetap
  di clipboard", not "Disimpan".

- Hapus never removes a pinned history card (`card_to_remove`).
- A new image gives the shown screenshot one last file look before it takes
  over the retries (one retry slot only).

Related: [[bata-robot-expressions]], [[system-page]]

Listed in [[index]].

## v3.2 Run 3: the preview always shows

Why it could stay silent in 3.1.0 (read from the code; the Windows side was
not seen live):

1. `only_bitmaps` was an allow-list: every registered format name had to be
   exactly "PNG". Any extra format a screenshot tool registers turned the
   instant preview off, and the preview then waited for a FILE; Win+Shift+S
   often writes none.
2. A screen clip's overlay is a fullscreen window: `SHQueryUserNotificationState`
   says BUSY until it closes, and the island's own poll is up to a second
   behind. `shot_from_image` dropped the image when the cached flag said
   fullscreen, with no retry (and the reducer drops screenshot notices too
   while fullscreen).
3. The file looks stopped at about 2.8 s, short of the 5 s match window.

Now:

- `picture_only` (`clipboard/win.rs`) is a deny-list: a bitmap and none of
  text, files, a metafile, or HTML / rich text / Office / shell-list formats
  (`NOT_A_SCREENSHOT`). A browser's "Copy image" carries HTML Format, so it is
  still no screenshot.
- `fullscreen_now` asks Windows again before trusting the cached flag;
  `try_show_waiting` waits `SHOW_RETRY_MS` (1.2 s) up to `SHOW_TRIES` (3)
  times on `TIMER_SHOT_SHOW` (25), and gives up only for a game or a
  presentation. A flagged picture without a file is never dropped.
- `RETRY_MS` = 0.6, 0.9, 1.5, 2.5 s after the first look: the looks cover the
  whole match window.
- Simpan with no file writes one: `spawn_save` encodes the clipboard DIB at
  full size as a PNG through WIC on a worker, in the Screenshots folder (made
  if missing), `Screenshot YYYY-MM-DD HHMMSS.png`, " (2)"... never over
  another file (`save_name`). The path joins `taken`, so no later screenshot
  matches it. Failure says "Gagal menyimpan". Without the full picture (a
  history restored after a restart) it still says nothing was written.
- Must be verified live: the exact clipboard formats and owner of Win+Shift+S,
  PrtSc, Win+PrtSc and `ms-screenclip:`; whether Win+PrtSc touches the
  clipboard at all (the only trigger is `WM_CLIPBOARDUPDATE`; there is no
  folder watcher); whether Pictures is OneDrive-redirected (its files can look
  like reparse points, which `list_folder` skips).

## v3.2.1 review fixes: waiting for the file, safer deletes (2026-10-29)

- **Simpan and Hapus wait for Snipping Tool's file.** Clicked before the file
  arrived, they used to conclude "no file" at once (and the preview closing
  dropped the in-flight look). Now their workers keep looking themselves
  (`screenshot::wait_for`, every `LOOK_EVERY_MS` 300 ms) until the window has
  closed (+ `SETTLE_MS` 500 ms). Simpan takes the file found instead of
  writing a duplicate; only without one does it write. The notice can come up
  to ~5.5 s after the click.
- **Deletes need a confirmed hash** (`confirmed`, `pick_confirmed`,
  `find_confirmed_in`): both hashes read and within `HASH_MAX_DIST`. The lax
  rule (unknown hash allowed) is kept only for the preview's own match (the
  size line, the thumbnail opening it). The delete worker re-hashes the
  matched file before recycling; a file already gone reports "tidak ada file"
  (`delete_outcome`), not Recycled.
- **Simpan's name is chosen on the UI thread** (`save_path`, also skipping
  names a running save holds) and joins `taken` before the worker starts, so
  the next screenshot cannot claim it.
- **"Jika diabaikan: Hapus" needs a matched file** (`delete_when_ignored(..,
  has_file)`): an image-editor copy with no file keeps its clipboard and card.
- **Windows end at the next image** (`ClipCandidate::to_ms`, `window_end`,
  `Arrivals` shared with the workers): an earlier screenshot's last look (or
  its waiting Simpan/Hapus) never claims a file written after the next image
  arrived; a look that set out before is checked again on return.

## Gotcha: Controlled folder access (found 2026-10-28)

Windows Security's *Controlled folder access* ("Protected folder access
blocked", app `vale.exe`, folder `%userprofile%\Pictures\Screenshots`) blocks a
program it does not know from creating or changing files in Pictures. Vale only
*reads* that folder to match the clipboard picture to Snipping Tool's file
(allowed); the two things that write there are blocked: **Simpan** with no file
(`screenshot::win::spawn_save`, a new PNG) and **Hapus** (`clean::recycle`, a
move to the Recycle Bin). The app cannot ask for the exception; the owner
allows `vale.exe` once. A failed save now says "mungkin diblokir Controlled
folder access" (`Key::ShotMaybeBlocked`); README Troubleshooting has the steps.
A new release may need the exception again (the file is replaced).

See also: [[screenshot-snap]] (the Screenshot tile's own capture and editor).

## Duplicate on Simpan (v3.6)

Evidence (owner, PrtSc to Snipping Tool with auto-save): two identical files in
Pictures\Screenshots, `Screenshot 2026-10-28 235523.png` (248527 B, Snipping
Tool) and `... 235525.png` (248506 B, Vale's `spawn_save`, 2 s later). So the
preview was up, Simpan was clicked about 2 s after the capture, and the
file of the same pixel size existed, yet `wait_for_file` never accepted it.
Read from the code; the Windows side was not seen live.

Causes (each has a pure test):

1. **Two clipboard updates for one capture.** Snipping Tool can set the
   clipboard more than once (history "Bumped" or a second "Added"). Each
   `WM_CLIPBOARDUPDATE` made a NEW `ShotState`, whose `from_ms` is
   `claim_from` = its own arrival (the previous arrival is within `AFTER_MS`).
   The file stamped before the second arrival fell outside its window, the
   first shot's window ended at the second's arrival, and its matched file sat
   in `taken`. The shown (second) shot then never found the file: Simpan wrote
   a second copy; a merely waiting first shot was replaced and dropped (no
   preview). Fix: `screenshot::same_capture` (same size, at most
   `SAME_CAPTURE_MS` 2 s after the newest shown or waiting shot, same history
   card or hashes within the lax distance) and `merge_arrival`: the new shot
   keeps the first one's `from_ms`, matched file and flag
   (`app_shot::plan_arrival`) and shows again. A real PrtSc twice on an
   unchanged screen inside 2 s merges too: Simpan then says "already saved"
   for the first file (no duplicate; the look for a second file continues).
2. **Hash too strict.** Simpan needed `confirmed` hashes (both read, within
   10 bits). An unreadable hash (file still locked, WIC failure) or a
   low-contrast picture (average-hash cells near the mean flip; the clipboard
   thumbnail is a box sampler, the file side WIC Fant) refused the file.
3. **Window.** The wait ended at 5 s after the capture (+0.5 s); a cold
   Snipping Tool can write later. `list_folder` skips a file whose header
   cannot be read (still locked); the 300 ms looks make that only a delay.

New rules:

- **Lax for the preview and for Simpan** (`pick_match`, `save_decision`): same
  size, in the window, hash unreadable or within `LAX_HASH_DIST` (16 of 64
  bits). Ranking: hash within 10 bits, then 11..16, then unknown; then the
  closest in time. The preview never takes a clearly different picture (more
  than 16 bits). Simpan alone (review round 2) also takes the one unclaimed
  same-size file in its window when it is the only one, whatever its hash
  (a low-contrast picture's average hash can read far off; a second copy is
  the worse mistake); two such files and no hash match: it writes. A
  browser "Copy image" (no new file of that size) still never shows.
- **The lone guess waits for the end** (review round 3, `SaveOutcome::Lone`,
  `screenshot::wait_for_save`). It never ends the wait: Simpan clicked
  before Snipping Tool wrote this capture's file took an unrelated same-size
  file on the first look and never saw the right one a moment later. Only
  the guess of the last look (at the deadline) is kept. And it must lie past
  the Simpan window (`SAVE_AFTER_MS`) of the image before this capture
  (`Arrivals::before(from_ms)`): `claim_from` keeps out only that one's
  `AFTER_MS` files, so an unanswered earlier screenshot's late (cold) file at
  +6 s sat in the next capture's window, and a plain PrtSc 8-9 s later
  "already saved" that picture and wrote nothing. Cost: a far-hash file of
  this capture that lands inside the previous image's Simpan window (two
  captures within ~10 s, low-contrast picture) makes Simpan write a copy.
- **Simpan never writes a second copy** when Windows wrote a file for the
  shot: found at click time (`state.file`) or by the save worker's wait
  (`Strictness::Lax`, window `SAVE_AFTER_MS` 7 s, +0.5 s settle, ending
  earlier at the next screenshot's arrival). Notice "Sudah tersimpan ·
  Pictures\Screenshots" / "Already saved" (`Key::ShotAlreadySaved`);
  "Disimpan" only for a PNG Vale wrote (`Event::ShotSaveDone.written`). Cost:
  with no file at all (PrtSc without auto-save) the notice comes up to 7.5 s
  after the capture, was 5.5 s.
- **Deletes stay strict** (`pick_confirmed`, `find_confirmed_in`,
  `Strictness::Confirmed`, window `AFTER_MS`): both hashes read and within 10
  bits; a looser match never recycles a file.
- `claim_from`, `window_end` and `taken` are unchanged for different
  pictures: never another screenshot's file.
- **One capture, one answer** (review round 1). Every `ShotState` has a
  `capture` (the id of the capture's first clipboard update; merged updates
  share it). A file found for one update goes to every update of that capture
  still without one (`Shots::give_file`): a superseded update still shown
  while its successor waits for the overlay could otherwise put the file in
  `taken` where the successor never claims it. Simpan answers the capture
  (`Shots::keep_capture`): a file of any update is "already saved"; else the
  save worker gets the newest update's candidate; a waiting update of it is
  dropped (no second preview). The capture is remembered
  (`Shots::answered`, `Answer::Kept`): an update of it arriving after Simpan
  (preview already closed, so nothing to merge with) is ignored, NOT pushed
  to `arrivals`, so it cannot cut the save worker's wait before Windows' file
  is stamped (`answered_again`). Cost: a real second PrtSc of an unchanged
  screen within `SAME_CAPTURE_MS` of the last update after Simpan or Hapus
  gets no preview.
- **Hapus answers the capture too** (review round 2, `Shots::delete_capture`):
  a waiting update of it is dropped and `TIMER_SHOT_SHOW` stopped (it raised
  a preview of the just-deleted shot); the delete job gets the file of any
  update, the newest update's candidate and the newest clipboard sequence
  (the clipboard holds that write). `Answer::Deleted` is remembered: a later
  update is absorbed, not pushed to `arrivals` (it cut the strict worker's
  `AFTER_MS` wait, the file stayed). The picture it put back goes again
  (clipboard emptied if unchanged, its card removed unless pinned) only on
  stricter proof (review round 3, `screenshot::takes_back`): both hashes
  read and within `HASH_MAX_DIST`, at most `TAKE_BACK_MS` (1 s) after the
  capture's last update. A looser update (lax hash only, no hash, or 1-2 s
  later) is only kept quiet: a deliberate retake of an almost unchanged
  screen right after Hapus was wiped from the clipboard and history on the
  lax rule. Left: a retake strictly equal and within 1 s is still wiped (its
  auto-saved file stays on disk); a tool's re-set later than 1 s leaves the
  deleted picture on the clipboard and in the history (no preview). The
  strict hash rule of deletes is unchanged.
- **An absorbed update says nothing** (review round 2). `shot_from_image`
  returns `ShotArrival` (`Nothing` / `Raised` / `Absorbed`); `read_clipboard`
  sends the copy notice / "Disalin" only for `Nothing`. Before, the absorbed
  update returned "no preview", the copy-notice gate had never seen the card
  (the first update raised the preview), and the copy notice replaced
  "Sudah tersimpan" a few hundred ms after Simpan.
- **"Jika diabaikan: Hapus"** fires only when the matched file is one the
  strict worker may accept (`screenshot::may_confirm`: confirmed, or the
  file's hash not read yet). A lax-only match (11..16 bits) or a clipboard
  without a hash keeps the clipboard, the card and the file.

Still unverified live: which formats and how many updates Snipping Tool really
posts for PrtSc (the merge is read from the symptom, not captured); whether
the 16-bit lax distance is enough for the preview in the owner's low-contrast
cases (Simpan no longer depends on it when one file is in the window); the
file stamp of a cold-start Snipping Tool beyond 7 s.

## The saved notice is clickable (2026-11-04)

"Disimpan" / "Sudah tersimpan" now read "Pictures\Screenshots · klik untuk
buka" and a body click shows the file in Explorer (`NoticeKind::ShotSaved {
path }`, `Effect::RevealFile`, `sources/reveal.rs`). `kept_notice` takes the
saved file (Windows' `plan.file.path` or the worker's path) and the renderer's
`fits`. Simpan's matching, "never a duplicate", `save_path`, `taken` and
Hapus are untouched. Details, the foreground rule and the inline / editor sink
in [[screenshot-snap]] ("The saved notice is clickable").
