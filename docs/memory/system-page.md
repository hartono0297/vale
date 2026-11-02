---
tags: [decision, system, timers, power]
date: 2026-11-02
---

# System page (Sistem) and its disk / RAM warnings

Run 4 of the v3.1 plan (keputusan-v3.1.html, decision 4a). Readings and
the warning logic live in `src/system.rs`; the page in
`panel_layout::system_layout` and `render_panel::paint_system`.

- **Readings run off the UI thread.** `Sampler` is a parked worker thread,
  started on the first request, that answers `SampleKind::Page` (CPU from
  two `GetSystemTimes` readings, RAM, fixed drives, top 3 processes from a
  ToolHelp snapshot, uptime) or `SampleKind::Warn` (RAM and drives only).
  `GetDiskFreeSpaceExW` can block on a spun-down disk, so it never runs on
  the UI thread. Only `DRIVE_FIXED` drives are read; removable and network
  drives are never woken.
- **CPU baseline.** Process and system times are kept between page samples.
  Without one, or with one older than 10 s, the worker takes a baseline and
  waits 300 ms, so a page that opens shows real numbers at once. Processes
  are summed per exe without case ("Chrome" is one row). A reused pid with
  another exe counts 0 %.
- **Page refresh (v3.2.1, smoother and lighter).** `TIMER_SYSTEM_PAGE` is a
  1 s one-shot (`system::LIVE_MS`), armed when a sample arrives and only
  while `system_sample_wanted` (the panel shows `Page::System`).
  `sync_system_page` (called from `refresh`) kills it when the page or panel
  goes and resets `system.requests`. A sample that arrives late re-arms
  nothing. The first request after the page shows, and every 4th after
  (`kind_for_request`), is a full `SampleKind::Page`; the ones between are
  `SampleKind::Live` (CPU since the last reading, memory, uptime, no process
  walk, no drives; merged into the last full sample, skipped by the
  warnings). Measured on a 144-process machine: full 8.3 ms, cheap 27 µs
  (`cargo test sampling_cost -- --ignored --nocapture`), so the page reads
  four times as often for about half the work of the old 2 s walk.
- **Easing.** `system::SysEase` keeps an `Eased` per figure (CPU, RAM, each
  drive's bar): a new reading glides there over 500 ms (cubic ease-out),
  from wherever the figure is. The first reading shows at once. While one
  runs, `animating_at` is true (`system_easing`, only with the page shown)
  so the frame clock draws; when they rest the clock stops, so an idle
  page costs nothing between readings. The same reading again, or one
  less than `EASE_MIN_STEP` (0.002) off, starts no glide (`Eased::retarget`
  returns early), else every unchanged figure would keep the clock busy for
  500 ms a second. The sparkline keeps 60
  one-second points (`HISTORY_LEN`); the demo's `DEMO_CPU` has 60 to match.
- **Reopened page.** When the page shows again and the last reading is
  older than `HISTORY_STALE` (10 s), `SystemState::forget_stale` drops the
  sample, the sparkline and the easing: the page shows dashes until the
  first fresh reading, which shows at once instead of gliding from the last
  session's figures. Within 10 s the old figures stay and glide on.
- **Background warnings.** `TIMER_SYSTEM_WARN` is a one-shot, every 5 min,
  re-armed when its reading comes back. It is sooner only to confirm RAM's
  minute above the limit. A drain that holds only system readings does not
  refresh or redraw the island (`drain_refreshes`) unless a notice was
  raised.
- **Once per crossing** (`WarnMonitor`, like the battery). Disk: per drive,
  as soon as it drops under `disk_warn_gb`, re-armed once it is 1 GB back
  above (hysteresis). RAM: only after staying above `ram_warn_percent` for
  60 s, re-armed at or under the limit. Changing a limit does not re-raise
  a crossing that is already alerted. The notice is `NoticeKind::SystemLow`
  with Bata's Sad ("lesu") face, an ochre `Activity` badge when the robot is
  off, and "Buka". A click opens the panel on the System page.
- **Clicks.** A drive meter opens Explorer on its root
  (`shortcuts::Action::Drive`, ShellExecuteW). A process row opens Task
  Manager. Both collapse the panel first, as the shortcut tiles do.
- **"Bersihkan"** is the accent button (`CLEANER_READY` is true since
  Run 5) and opens the cleaner over this page, see [[cleaner]]. The page
  itself deletes nothing, and it stops sampling while the cleaner covers it
  (`system_sample_wanted(view, cleaner)`). The cleaner now survives a close
  and page changes, so `IslandState::cleaner()` is "cleaner on System" (None
  on any other page): the page samples again as soon as another page shows,
  and stops only while the cleaner is shown on System.
- **Config.** `disk_warn_gb` (5–100 by 5, default 10) and
  `ram_warn_percent` (70–98, default 90). Both are serde defaults, so an
  older config still loads. Like the battery threshold, the file owns them
  (`merge_owned`). Settings → Umum shows them in a second card, so General
  now scrolls like [[settings-appearance-scroll]].
- **Order.** `Page::System` sits between the file shelf ([[shelf]]) and
  the Devices page ([[devices]], v3.5), one of 12 pages in `Page::ALL`; the
  owner reorders them in Settings ([[panel-pages]]). Removable and network
  drives still never show here (only `DRIVE_FIXED`); Devices lists those.

Demo: `system`, `system-low` and `bata-sistem` use fixed figures
([[demo-data-time-of-day]]). Shots `system` and `system-warning` are in
[[screenshot-capture-script]].

Related: [[cleaner]], [[bata-idle-power-budget]], [[bata-robot-expressions]],
[[studio-tabular-figures-word-gaps]], [[media-click-guards]]

Listed in [[index]].
