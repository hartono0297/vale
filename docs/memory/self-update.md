---
tags: [decision, gotcha, update, tray, settings, v3.11]
date: 2026-11-01
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
3. **Offer**: notice "Vale X.Y.Z tersedia" with the chip "Update"
   (`NoticeKind::UpdateAvailable`). An automatic check offers a version
   **once** (`should_offer`; `config.update_offered` is saved at once); a
   manual check always offers. While an update is known the tray reads
   "Update ke X.Y.Z" and the Tentang row has "Update".
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
7. **New process**: waits (up to 60 s) for the single-instance mutex, which the
   old island releases itself at the end of its exit (see "Exit crash and the
   mutex hand-over"), shows "Diperbarui ke X.Y.Z", deletes `vale.exe.old` (`cleanup_leftovers`, background thread,
   retries for about 5 s). Every normal start of a release build runs the
   same cleanup.

Assets, exact names (from `.github/workflows/release.yml`, see
[[install-script-follows-latest-release]]): `vale.exe`, `vale.exe.sha256`,
`vale-vX.Y.Z.zip`. Renaming them breaks the updater (`AssetMissing`).

## Gotchas

- **The mutex handle outlives the old process.** `CreateMutexW` returns a
  handle even when the mutex exists (`ERROR_ALREADY_EXISTS`). A start that
  waits (`--updated`, up to 60 s (`UPDATE_CLAIM_WAIT`, `claim_wait`), polling
  every 200 ms) must `CloseHandle` every handle of a failed try, for both names
  (`Local\Vale`, `Local\DynamicIsland`), before retrying, including the handle
  of a name that was free (else the retry sees its own mutex). A normal start
  still quits at once. The old island closes its own handles at the end of its
  teardown (`release_single_instance`, handles kept by `try_claim` in
  `INSTANCE_MUTEXES`), so the wait normally ends within a second. Tests:
  `claim_names` and `releasing_the_mutexes_lets_the_next_island_claim_them` in
  `app.rs`.
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

## Exit crash and the mutex hand-over (v3.15.1)

**Symptom**: in v3.15.0 Vale did not come back after "Update". The old exe was
already swapped (`vale.exe.old` stayed behind), the new `--updated` process
started, waited 10 s for the mutex and quit silently.

**Root cause** (crash dumps in `%LOCALAPPDATA%\CrashDumps`): the OLD process
crashed on every exit with 0xc000000d in ntdll, raised on a threadpool thread
and called from `D3D10Warp.dll` during the DLL detach / FLS cleanup in
`LdrShutdownProcess`. The render targets are
`D2D1_RENDER_TARGET_TYPE_SOFTWARE` (WARP), and they were still alive: `App`
(it owns `Renderer`) sits in the `APP` thread-local in `app.rs`, which only
the TLS destructors dropped, inside `LdrShutdownProcess`. Windows Error
Reporting then kept the dying process, and with it the `Local\Vale` and
`Local\DynamicIsland` mutexes, for about 12 s. The new process waited 10 s
(`UPDATE_CLAIM_WAIT`), found the mutex still taken and ended.

**Fix** (`app::end_process`, after the `GetMessageW` loop; every exit path,
tray Quit, the update's `shutdown`, `WM_CLOSE`, ends in that loop):
1. `drop_app`: `App` leaves `APP` and is dropped explicitly (Renderer, CLI
   inputs, window, stores). Config, notes and reminders are saved at each
   change, the clipboard and notification history in `WM_DESTROY`, so nothing
   is left to save at the end.
2. `release_graphics`: `release()` in every module that keeps D2D/DWrite
   objects in a thread-local: `settings`, `editor` (and its popover),
   `arrange_overlay` (outline and pick), `capture::overlay`, `identify_overlay`,
   `shot_edit::window`. Each destroys its window if open and empties its slot.
   `fonts.rs`'s DirectWrite loader is not WARP and stays.
3. `stop_pipe_server`: the Claude hook pipe thread stops (`PipeServer::stop`:
   flag, a wake-up connection of its own, bounded join of about 1 s). A hook
   line that arrived before the stop is still read.
4. `release_single_instance`: the island closes its own mutex handles.
5. `TerminateProcess(GetCurrentProcess(), 0)` instead of returning. Belt and
   braces: the crash is inside D3D10Warp's own threadpool cleanup, so it can
   still happen on the DLL detach with none of our objects left; terminating
   skips the detach, so an exit can never hang on WER. The OS closes the
   handles of a terminated process at once; the CLI's job object still kills
   its children. A watchdog thread ends the process after 5 s if a drop blocks.

**New process**: waits 60 s instead of 10 (`claim_wait`: `Some` -> 60 s,
`None` -> 0). The Claude pipe's first instance
(`FILE_FLAG_FIRST_PIPE_INSTANCE`) is retried for 60 s (`first_instance_retry`,
250 ms steps) while the old process still holds `\\.\pipe\vale`.

Not checked live in the build run (the owner's island was running): start the
release exe, quit from the tray, and compare the Application Error events for
`vale.exe` and the files in `%LOCALAPPDATA%\CrashDumps` before and after: none
new.

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
set back counts as due. The 24 h memory is per run (not saved); only the
offered version is saved, so a restart may check again but never re-offers
the same version.

## Notices

`UpdateAvailable` and `UpdateFailed` have actions (hover does not open the
panel over them; `Effect::InstallUpdate`, `Effect::OpenReleases`).
`UpdateProgress` replaces a queued or current one in place (no new entrance:
`refresh` does not count a progress-to-progress change as a view swap), lasts
15 s from its last update so a stalled download fades; an `UpdateFailed`
removes it. A manual check that fails reuses `UpdateFailed` ("Update gagal:
tidak ada koneksi").

Related: [[open-mode]] (notices with a button), [[settings-structure]],
[[tray-dark-menu]].
