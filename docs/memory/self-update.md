---
tags: [decision, gotcha, update, tray, settings, v3.11]
date: 2026-11-03
---

# Self-update: check, download, verify, swap, restart

"Cek pembaruan…" in the tray ([[tray-dark-menu]] has the menu code) and
Settings → Tentang → Pembaruan ([[settings-structure]]) find a newer release
and install it in place. No GitHub page, no reinstall. Code: `src/update.rs`
(pure logic and workers), `src/app_update.rs` (the island's side: timer,
notices, state), `google::http::Http::download_to` (streaming download),
`google::oauth::sha256` (the one BCrypt helper, shared with PKCE).

## Flow

1. **Check** (worker thread): GET
   `https://api.github.com/repos/hartono0297/vale/releases/latest`
   with `User-Agent: Vale/<version>` and `Accept: application/vnd.github+json`,
   20 s timeout, body capped at 1 MB. `releases/latest` skips drafts and
   pre-releases. A non-2xx answer (403 rate limit, 404) is `BadRelease`; a
   transport error or timeout is `NoNetwork`.
2. **Compare**: `parse_version` takes `X.Y.Z` with one optional `v`; anything
   else (pre-release suffix, two parts) is `None` and never counts as newer.
3. **Offer**: a **sticky** notice "Vale X.Y.Z tersedia" with two buttons,
   "Update" and "Nanti" (`NoticeKind::UpdateAvailable`, see Notices). **Every**
   check that finds a newer release offers it (`on_checked`, automatic or
   manual): nothing is remembered between checks, so "Nanti" is followed by
   the same offer at the next automatic check. While an update is known the
   tray reads "Update ke X.Y.Z" and the Tentang row has "Update"; both stay
   after "Nanti" (`update.known` stays Some).
4. **Download** (worker): `vale.exe.sha256` into memory (cap 1 KB), then
   `vale.exe` to `<exe dir>\vale.exe.download` (same volume, cap 64 MB, stall
   timeout 30 s that restarts with each chunk). Progress notice
   `UpdateProgress` only when the whole percent changes. Redirects to
   `objects.githubusercontent.com` are followed by `HttpClient`.
5. **Verify**: SHA-256 of the file against the `.sha256` ("<hex>  vale.exe",
   no newline); mismatch deletes the temp file (`ChecksumMismatch`).
6. **Swap** (UI thread, `swap_and_launch`): `vale.exe` -> `vale.exe.old`, the
   download -> `vale.exe` (3 tries, 200 ms apart, antivirus may hold it), start
   `vale.exe --updated <old version>`, then the island closes through
   `shutdown` (tray icon removed by `WM_DESTROY`, config saved before).
7. **New process**: waits for the single-instance mutex, shows "Diperbarui ke
   X.Y.Z", deletes `vale.exe.old` (`cleanup_leftovers`, background thread,
   retries for about 5 s). Every normal start of a release build runs the
   same cleanup.

Assets, exact names (from `.github/workflows/release.yml`, see
[[install-script-follows-latest-release]]): `vale.exe`, `vale.exe.sha256`,
`vale-vX.Y.Z.zip`. Renaming them breaks the updater (`AssetMissing`).

## Gotchas

- **The mutex handle outlives the old process.** `CreateMutexW` returns a
  handle even when the mutex exists (`ERROR_ALREADY_EXISTS`). A start that
  waits (`--updated`, up to 10 s, polling every 200 ms) must `CloseHandle`
  every handle of a failed try, for both names (`Local\Vale`,
  `Local\DynamicIsland`), before retrying, including the handle of a name
  that was free (else the retry sees its own mutex). A normal start still
  quits at once. Test: `claim_names` in `app.rs`.
- **A running exe can be renamed, not overwritten or deleted** (NTFS). That is
  why the swap renames first, and why `.old` can only go once the old process
  is gone.
- **Rollback**: if the second rename or the launch fails, `.old` goes back to
  `vale.exe`, the temp file is deleted, the old island keeps running and shows
  "Update gagal: <alasan>" with the chip "Lihat rilis"
  (`releases/latest`). A launch failure is reported as `NotWritable`
  ("folder tidak bisa ditulis"): the enum has no better word for an exe that
  will not start.
- **`shutdown` cannot run inside the `App` borrow**: `DestroyWindow` ->
  `WM_DESTROY` calls `with_app`, which would fail and skip the cleanup. The
  swap sets `DEFERRED_SHUTDOWN`; `run_deferred` calls `shutdown` after the
  handler returns.
- **`classify`** maps a failed transfer's HRESULT: disk full / access denied /
  write fault -> `NotWritable`, `E_FAIL` (a body past the cap) -> `BadRelease`,
  the rest (WinHTTP 12xxx, `ERROR_TIMEOUT`) -> `NoNetwork`.

## Never in debug builds, never in the demos

`cfg!(debug_assertions)`: no timer, and a manual check only says "Cek
pembaruan mati di build debug". The `--demo*` and `--settings` modes never
reach `app::run`, so they never check; their update notices and the Tentang
row (`DI_UPDATE=available|checking|downloading`, made-up 9.9.9) touch no
network.

## Timer and wall clock

`TIMER_UPDATE` (35): first tick 30 s after the start, then every hour. A tick
checks when `config.update_check` is on and at least 24 h of **wall clock**
(`SystemTime`) passed since the last automatic check of this run
(`auto_check_due`), so a PC that slept does not stretch the interval; a clock
set back counts as due. The 24 h memory is per run (not saved). There is no
once-per-version rule any more (`should_offer` is gone and nothing reads or
writes `config.update_offered`; the field stays in `Config` with a serde
default, marked unused, so an old `config.json` still loads and `merge_owned`
works): the offer comes at **every** automatic check while a newer release
exists, that is, the 24 h check of this run (`TIMER_UPDATE`, `auto_check_due`)
and the check 30 s after each start, and at every manual check. A manual
check while the same offer shows replaces it in place.

## Notices

**The offer is a sticky notice** (like a reminder or Claude waiting). In the
`Event::Notice` arm it goes to `sticky` with its own key, `Sticky::Update`: one
slot for any version, so a newer or repeated offer replaces the old one and
there is never more than one. No expiry (`UPDATE_NOTICE_DURATION` is for
`UpdateFailed` only). The newest sticky notice shows: on the closed island as
the notch notice, over an open panel as the strip. Claude waiting, reminders
and a failed Mole run keep working beside it (the newest shows, the others
come back once it is cleared); a tucked real toast tucks over it as over any
sticky notice ([[toast-while-busy]]).

- **Two buttons**, both in the notch notice and in the strip: "Update"
  (accent, `Key::UpdateButton`, `Hit::UpdateNow`) and "Nanti" / "Later"
  (raised fill, normal ink, `Key::UpdateLater`, `Hit::UpdateLater`).
  `Hit::UpdateNow` clears the offer and pushes `Effect::InstallUpdate`
  (`App::update_install`, unchanged); `Hit::UpdateLater` clears it and does
  nothing else. Both act only while the offer is what shows (`update_offer_shown`:
  the notice, or the strip, not a toast's strip).
- **A click elsewhere installs nothing and dismisses nothing.** `Hit::Body`
  over the notice: nothing in Hover mode; in Click mode it opens the panel
  like for other sticky notices ([[open-mode]]), the offer in the strip.
  `Hit::Strip` outside the chips: nothing. (`dismiss_sticky` never clears an
  offer; only the chips, `UpdateProgress`, a replacement or
  `Event::UpdateWithdrawn` do.)
- **Withdrawn**: a check that finds no newer release (the offered one was
  pulled) forgets `known` and sends `Event::UpdateWithdrawn`, which clears
  `Sticky::Update`, so no "Update" chip is left that would install nothing.
- **Notch geometry** (`geometry.rs`): `notice_actions_layout(shape, widths)`
  lays a row of chips at the right (6 dip apart, "Nanti" `LATER_CHIP_W` 48 then
  "Update" `UPDATE_CHIP_W` 56) and the title and body stop short of the first;
  `notice_chip_widths(kind)` gives the widths (one for the other notices,
  two for the offer; `notice_action` is `None` for the offer). The renderer
  (`paint_update_offer`) registers `update_offer_regions` like `shot_regions`.
  The same 360 x 76 shape in Pill and Notch; DM Sans keeps the title and body
  on one line in both languages (a test, also for "10.11.12"), JetBrains Mono
  trims with an ellipsis like every notice.
- **Strip** (`panel_layout`): dot, "Vale X.Y.Z tersedia" and two small chips at
  the right (`update_strip_parts`: 18 dip tall, "Nanti" 44, "Update" 52).
  `regions(s, layout, strip: impl Into<StripKind>)` takes `StripKind::{None,
  Plain, UpdateOffer}` (a bare `bool` still means None/Plain); for
  `UpdateOffer` both chip rects come **before** `Hit::Strip`, so they win.
- **Hover**: the first visit after the offer appeared is kept off the panel
  (the buttons are for it), exactly the reminder rule (`reminder_seen`,
  generalised by `sticky_has_buttons` to Reminder and UpdateAvailable); later
  visits open the panel with the offer in the strip. A new offer after
  "Nanti" gets its own first visit (`clear_sticky(Update)` resets
  `reminder_seen`). Claude waiting and a failed Mole run stay non-blocking.
- **Fullscreen**: the offer is hidden while a fullscreen app is in front
  (`base_view` is `Hidden`) and shows again after. **Unlike reminders no brief
  copy is queued** (an offer is not urgent; assumption of this change).
- **Install from elsewhere** (tray "Update ke X.Y.Z", Tentang "Update"): the
  arrival of an `UpdateProgress` notice clears `Sticky::Update`. An
  `UpdateFailed` works as before (queued, 8 s, chip "Lihat rilis", a click
  opens the releases page, `Hit::Body`) and does not bring the offer back.

`UpdateFailed` has an action (hover does not open the panel over it;
`Effect::OpenReleases`). `UpdateProgress` replaces a queued or current one in
place (no new entrance: `refresh` does not count a progress-to-progress change
as a view swap), lasts 15 s from its last update so a stalled download fades;
an `UpdateFailed` removes it. A manual check that fails reuses `UpdateFailed`
("Update gagal: tidak ada koneksi").

Demo: `update-available` (the sticky notch form, made-up 9.9.9) and
`update-available-strip` (Calendar panel, the offer as the strip, both chips).

Related: [[open-mode]] (notices with a button), [[settings-structure]],
[[tray-dark-menu]].
