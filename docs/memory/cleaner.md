---
tags: [decision, cleaner, system, licence]
date: 2026-11-02
---

# The cleaner ("Bersihkan", mode "Pilih sendiri")

Run 5 of the v3.1 plan (keputusan-v3.1 decision 4b, option D;
`docs/design/deepclean-opsi-d.html`). Engine in `src/clean.rs`, the panel in
`panel_layout::cleaner_layout` and `render_panel::paint_cleaner`.

- **Seven categories** (`CleanCat`, row order): temp & logs (user folders
  only, files > 7 days), browser caches (Chrome, Edge, Firefox, Brave,
  Opera), app caches (VS Code, Teams, Discord, Spotify, Zoom), developer
  caches (npm, pip, NuGet, Go, Cargo), Autodesk & Adobe, old installers
  (`.exe`/`.msi` at the top of Downloads, > 30 days), Windows Update
  leftovers. The top three are ticked when it opens.
- **Locations are a data table** (`LOCATIONS`), injectable roots (`Roots`),
  so `select(roots, cats, now, running)` is testable on a temp tree under
  `target/clean-tests`. Tests never touch the real Temp, Downloads, AppData
  or browser folders and never call `Roots::known()`.
- **Never follows links**: any reparse point (symlink, junction) is skipped,
  in the path to a location and inside it.
- **Open browsers are skipped** by exe name, without case, checked at the
  scan and again right before deleting; they are named in the row and the
  notice.
- **How files go**: caches permanently, file by file; a locked or denied
  file is counted once, never retried. Installers go to the Recycle Bin via
  `IFileOperation` (`FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE`, plus
  `FOF_WANTNUKEWARNING` so a file too big for the bin asks instead of being
  destroyed). COM is initialised (STA) on the cleaner thread.
- **Admin without elevating the island**: Windows Update runs this exe again
  with `--clean-admin windows-update` through `ShellExecuteExW` verb
  `runas`. The helper (in `main`, not in `opens_windows`, no window) deletes
  only `%WINDIR%\SoftwareDistribution\Download` and exits with the number of
  skipped files (0..=250) or 255. A refused UAC prompt is "skipped".
  `app.manifest` stays asInvoker.
- **Confirm in the button**: the first click arms it ("Yakin? Hapus X") for
  `CLEAN_CONFIRM` (4 s, the reducer's `next_deadline`, so the existing
  one-shot `TIMER_DEADLINE`); the second click inside that sends
  `Effect::CleanDelete`. Any other click, a tick, the panel closing, another
  page taking the panel (the cleaner stays, the button turns back) or the
  timeout disarms it. Reducer tests prove a single click never deletes.
- **Result = measured free space**: `GetDiskFreeSpaceExW` on the affected
  drives before and after, on the worker. The notice (`NoticeKind::CleanDone`,
  Bata Happy, a sage sparkle badge when the robot is off) shows the gain,
  the drive's free space now and what was skipped. The list scans again
  if the cleaner still exists (open or not), asked by the reducer.
- **Sub-view, not a page**: the reducer keeps `cleaner: Option<CleanerUi>`
  over the Sistem page. Since 2026-11-02 it keeps its place, see "Keeps its
  place" below. The panel grows to `ShapeKind::PanelClean` (560 × 352, the
  tall canvas). No page dots. The System page stops sampling while the
  cleaner covers it.
- **Power**: no timers or redraws while it is closed; scanning shows a
  static "memindai…", no spinner. A clean keeps running when the panel
  closes; its notice still comes.
- **Not here (by decision)**: no Recycle Bin category, no automatic or
  scheduled cleaning.
- **Two modes** (Run 6): the header segment switches to "Otomatis · Mole"
  (not while either mode cleans); the mode last used is remembered
  (`clean_mode`). See [[mole-mode]].
- **Done-notice body** is fitted to the 360 px notice by measure
  (`fits`), shorter wordings before dropping parts; see [[mole-mode]].

## Keeps its place (2026-11-02)

The cleaner (either mode) no longer resets when the notch closes or the page
changes. Tests are reducer tests in `state.rs` (`cleaner_*`, `the_wheel_*`,
`a_closed_cleaner_*`, `a_clean_that_ended_*`).

- **The wheel never leaves it.** `can_turn_page()` is false while
  `cleaner_shown()` (the cleaner exists and the view is `Panel(System)`), so
  `Event::Wheel`, `Event::PageFlick` and the dock (`go_to_page`) do nothing
  and spend any saved-up notch (`wheel = 0`). Only Back leaves. The peek over a sticky reminder is unchanged. The plain System page
  (no cleaner) still turns pages. Over a Mole log longer than its box the
  wheel scrolls the log instead, see [[mole-mode]].
- **Kept across everything else.** A close of the panel (hover leave,
  Dismiss, linger), the Clipboard / Devices / Arrange hotkeys, the search and
  its back button, tiles, a notice's "Buka" and the CLI all leave `cleaner`
  alone: mode, ticks, `mole_log`, `mole_opts` and `MolePhase` stay. A Mole
  detect or preview keeps running meanwhile; `leave_mole` runs only when the
  cleaner ends or the mode leaves Mole.
- **`cleaner()` is "cleaner on System".** It returns the cleaner only while
  `page == Page::System`, so `panel_kind` / `PanelClean`, `panel_snap`, the
  frame and `system_sample_wanted` follow it: another page keeps its own
  size, and the System page stops sampling only while the cleaner covers it.
  `cleaner_kept()` (tests) is "exists anywhere". `shown_panel()` gives the
  cleaner only on System.
- **A hidden System page** (off in Halaman panel, opened by its tile) is not
  left on close while the cleaner is kept on it (`leave_hidden_page` is
  skipped); without a cleaner the "off page holds for that visit" rule is as
  before. `set_pages` is unchanged.
- **When it ends:** Back (a Mole preview stops via `leave_mole`), a switch
  to Own (the preview stops), or the seen-result rule. `clean_result:
  Option<bool>`: a clean that completes while the cleaner exists
  (`Event::CleanFinished`, or `MoleFinished` moving Running to Finished)
  marks it unseen; once `cleaner_shown()` it is seen; a seen result whose
  cleaner is no longer on an open System panel ends it, and the next hover
  shows the plain System page. A result that finished while closed (or under
  a notice, since `view()` is the notice) waits until shown. `App` applies
  the notice before `CleanFinished` / `MoleFinished` for that reason. The
  mark clears when another clean starts, on a new preview and when the
  cleaner ends. A scan-only or preview-only cleaner survives any number of
  closes until Back. A failed `start_clean` (`CleanFinished` without a real
  clean) counts as a completed clean. The Mole notice's click
  (`mole_pending`) still opens the cleaner on its log; a finished clean
  opened that way also ends once seen.
- **Stale sizes, no timers.** `CLEAN_SCAN_STALE` = 5 min. The reducer stamps
  `own_scan_at` when it emits `OpenCleaner` and `mole_preview_at` when it
  emits `MolePreview` or `MoleDetect`: the time of the request, not of the
  scan's end. When `cleaner_shown()` goes false to true in an `apply` (panel
  reopened, page returned, "Bersihkan" over a cached result, search back),
  `refresh_if_stale` runs: Own scans only if nothing runs and the stamp is
  old or missing (else `App.clean.scan` is reused); Mole: Unknown detects,
  a stale Previewed previews anew, a stale Missing detects anew, a
  Detecting / Scanning / Running / Finished one is left alone. `Hit::SystemClean`
  only creates the cleaner (a stale Finished result is dropped for a new
  Mole cleaner); this transition asks for the work. `switch_mode` to Own uses
  the same rule.
- **Rescan after a clean** comes from the reducer: `CleanFinished` with a
  cleaner emits `OpenCleaner` and stamps, even with the notch shut (the
  worker, no timer). `App` no longer scans again itself. Without a cleaner
  (Back during the clean) it clears `own_scan_at`, since `App` dropped the
  sizes on `Done`: the next "Bersihkan" scans. A scan the thread cannot take
  returns `Event::CleanScanFailed`, which clears the stamp too.
- **Safety:** with the cleaner on anything but an open System panel the
  button is disarmed (`armed_until = None`); a single click still never
  deletes, and `next_deadline` still holds only `armed_until`.
- **Power:** no new timer, no redraw while closed; only the existing
  workers. Notices (CleanDone, MoleDone, MoleFailed) are unchanged.

## Mole: data only, and which licence

The location list follows Mole for Windows by tw93 (`lib/clean/*.ps1` on
branch `windows`). Gotcha: that branch was **relicensed to GPL-3.0 on
2026-10-11** (commit 5b7695bb). The data is taken from the last MIT commit,
500dfc4c (Mole 1.30.0), and only the data (folders, age rules); the Rust is
the island's own. `NOTICE` carries the MIT text with its copyright line
("Copyright (c) 2025 tw93"); README credits it. Never read or copy Mole's
macOS code. Where the island is narrower than Mole (Teams databases, Zoom
settings, NuGet packages, no system temp) the table says so.

## Verify pass (2026-10-28)

- **Prune only what the clean emptied** (`prune_emptied`): walk up from each
  deleted file to its location folder. Pruning every empty folder under a
  location deleted the user's own empty folders across the whole profile and
  %APPDATA% (the profile-root and Roaming-root `*.log` locations), even an
  empty git `refs`.
- **Temp must be a real temp folder** (`safe_temp`): without TMP/TEMP,
  GetTempPath returns the profile or Windows, and "older than 7 days" there
  would take documents. A Temp root that is not named Temp/Tmp, holds the
  profile, AppData, Downloads or Windows, or is Windows' own, is left out.

- **Only what the scan counted** (`only_scanned`): the worker keeps its last
  scan; the confirm re-selects (browsers checked again) and keeps only files
  the scan listed. New cache files, files that crossed their age line and a
  browser that closed after the scan are left alone.
- `*.log` in the profile and Roaming roots go to the Recycle Bin
  (`Target::recycle`), not gone for good: they sit beside the user's files.
- Spotify: only `Data` (its cache). `Storage` is Spotify's default offline
  download folder (Spotify community answers), so it is not a cache.

Related: [[mole-mode]], [[system-page]], [[bata-robot-expressions]],
[[bata-idle-power-budget]], [[studio-icons]], [[screenshot-capture-script]],
[[studio-tabular-figures-word-gaps]]

Listed in [[index]].
