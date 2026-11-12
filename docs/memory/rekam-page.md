---
tags: [decision, recording, panel, ui]
date: 2026-11-13
---

# The Rekam page, direction B (2026-11-13)

The panel's Rekam page was redesigned: the owner compared three directions
(`docs/design/rekam-ulang.html`) and picked **B**, then approved every state in
`docs/design/rekam-b.html`. Spec `docs/superpowers/specs/2026-11-12-rekam-page-redesign-design.md`,
plan `docs/superpowers/plans/2026-11-12-rekam-page-redesign.md`. Builds on
[[recording-audio]], [[recording-controls]], [[screen-recording]], [[recording-quality]],
[[transcript]], [[live-captions]], [[menu-pop]].

- **Left card, right list.** Card 196 dip (record button, a switch per source with its
  device line, live captions; live: pause / stop (/ trash on a screen recording), clock,
  mute buttons and meters). List 312 dip, grouped by day (`rec_day_label`: Hari ini,
  Kemarin, "8 Nov"), rows of time + length (+ title for videos), one pill shape for every
  transcript state (`rec_pill_look`, min 76, max 112), a Now row while recording, and
  "N lagi di folder" (`RecScan.total`, `rec.total` / `screen.total`) when rows are cut.
- **Sources are two switches**, still `Hit::RecSources(pair)`: the layout computes the pair
  each switch leads to (`RecSourceRow.switch_to`); the last source on gets `None` (dimmed,
  no click), so "no source" is unreachable.
- **Header**: two 22 dip icon buttons (transcribe a file, open folder) replace the text
  links. The panel has **no hover state**, so they have no hover fill; tooltips are the
  `hover-tooltips` work's (not landed when this merged).
- **Layar quality row** opens `src/ui/quality_menu.rs` (a `menu_pop` like `device_menu`,
  typed `Context`, `WM_APP + 95`), saving through `screen_set_quality` / `screen_set_fps`
  as Settings does; refused while recording.
- **Discard confirm has no timer.** "Buang video ini?" with Buang / Simpan
  (`Hit::ScreenDiscardKeep`) replaces the stop block; it used to expire after
  `CLEAN_CONFIRM` (4 s), which put Stop under a late click on Simpan. Now it closes on
  Simpan, any other click, Buang, a status change (pause, mute, zoom) or the end.
- **Missing device** (configured id, endpoint gone) shows "Tidak ditemukan" / "Not found"
  in ochre on the device line, only while idle (`missing_while_idle`); the long
  `RecMicMissing` / `RecSystemMissing` stay for the notch notice (they never fit 98 dip).
- **Strip**: with the waiting strip the top block is 68 (label dropped), 72 while
  recording (stop 52 + clock 20).
- **Demo**: `--demo-record` (debug build only) with `DI_REC_SOURCES`, `DI_REC_MISSING`,
  `DI_REC_PAUSED`, `DI_REC_TR=all|off`, `DI_REC_TAB=audio-empty`, `DI_REC_MENU=quality`.
  `DI_SURFACE` did not switch the Rekam demo's surface; Terminal contrast is pinned by the
  `Surface::ALL` tests instead.
- **Not checked by hand** (see [[recording-hand-checks]]): the quality dropdown in the real
  app, a device unplugged while idle, the page in every surface/font/language combination.
