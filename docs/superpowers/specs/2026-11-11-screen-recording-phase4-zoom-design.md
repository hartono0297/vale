---
tags: [spec, recording, screen-recording, zoom]
date: 2026-11-11
status: user said "coba dulu ja u kerjain" (go ahead, decide the details) on 2026-11-11
---

# Screen recording phase 4: zoom and cursor effects

Phase 4 of [[2026-11-07-recording-design]] (section "Phases", track C). Phase 3
(`docs/superpowers/specs/2026-11-11-screen-recording-phase3-design.md`) is on
`main` at 4e9be78. Mockups: `docs/design/rekaman.html` sections 9 and 10.

## Why

The user records tutorials: they talk while clicking around, and viewers must
read small buttons and text. Zoom in the video (never on the user's screen)
lets them point the camera at what matters; the cursor highlight and click
ripples show where they are and when they click.

## What the user decided

- Zoom by a key, not by clicking: "zoomnya jangan click". **No auto-zoom.**
- "Coba dulu ja u kerjain": the rest is the builder's call (below).
- Recording only part of the screen already exists (phase 3's Area mode
  snaps like the screenshot tool); nothing to add there.

## Behaviour

- **Toggle zoom:** `Ctrl+Alt+Z` (hotkey row `RecordZoom`, `RegisterHotKey` id
  11) zooms the video in to the cursor; pressing it again zooms out. The row
  is registered only while a screen recording runs (like the mic mute row,
  id 12), so the combo is never taken from other apps at idle. It is a
  normal row in Settings → Tombol pintas (recordable, can be turned off).
- **Camera:** the existing `ZoomCamera` (`src/screenrec/zoom.rs`): eases in
  and out over 350 ms, the centre follows the cursor smoothly, the view stays
  inside the recorded area (monitor, window or Area).
- **Level:** every recording starts at 2×. While zoomed, `Ctrl+Alt` + mouse
  wheel changes it in 0.5 steps between 1.5× and 4× (wheel up = closer). The
  wheel event is swallowed so the app under the cursor does not also zoom.
  Not zoomed, the wheel is not watched at all.
- **Notch:** while zoomed and not paused, the screen REC notch shows the level
  (`2×`, `2,5×` in Indonesian, `2.5×` in English) where it says "Layar".
  Paused still shows "dijeda".
- **Cursor highlight:** a soft yellow disc under the cursor in the video. On
  by default, Settings → Fitur → Rekam layar → "Sorot kursor".
- **Click ripples:** the existing ripple (`compose::ripple_at`) on every
  mouse button press. On by default, "Riak klik" in the same card.
- Both switches apply to the next recording (a running one keeps its own).
- **Cursor log:** already written by phase 3 (`<name>.cursor.jsonl`, zoom
  changes as `[t,"z",level]`). Nothing new.

## Non-goals

- Auto-zoom on click or typing (the user said no). The tested but unwired
  `zoom::InputEvent` design hook stays as it is.
- The zoom editor after recording (timeline of zoom points, re-render): a
  later phase; the cursor log already keeps what it needs.
- A box on the user's screen showing what is in the video.
- Remembering the zoom level between recordings.

## Constraints

- Ids from track C's reserve only: hotkey 11, `WM_APP + 92`. No new timer.
- Idle cost zero: the wheel hook thread exists only while zoomed; the hotkey
  only while a screen recording runs.
- No new crates. Indonesian and English for every string.
- Vale's windows stay out of the video (phase 3's excluder).

## Builder's calls (mine, open to change)

Hotkey toggle (not hold), level range 1.5×-4× by wheel, both effects on by
default, yellow highlight, level shown instead of "Layar" in the notch, the
level not remembered.
