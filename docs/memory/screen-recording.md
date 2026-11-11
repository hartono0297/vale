---
tags: [decision, recording, screen-recording, power, hotkeys, timers]
date: 2026-11-11
---

# Screen recording, basic (recording phase 3)

Records a monitor, a window (followed when moved) or a dragged area to an
`.mp4` with the mic and the PC audio mixed into one mono track. Spec:
`docs/superpowers/specs/2026-11-11-screen-recording-phase3-design.md`, plan:
`docs/superpowers/plans/2026-11-11-screen-recording-phase3.md`, mockups:
`docs/design/rekaman.html` sections 8, 10, 12. Merged on `main` 2026-11-11
(ce67e90, 3733 tests). Related: [[recording-audio]], [[recording-controls]],
[[notch-rec-mute]], [[transcript]], [[controlled-folder-access]],
[[media-foundation-dynamic-load]], [[screenshot-snap]], [[hotkeys]],
[[bata-idle-power-budget]], [[exe-size]], [[exclude-from-capture-in-context]],
[[mp4-silent-audio-track]], [[recording-hand-checks]].

## Where it lives

- `src/screenrec/`: track C's core (Windows Graphics Capture, D3D11 compositor,
  H.264 encoder through `crate::mf`, cursor log, zoom camera kept for phase 4,
  `ScreenRecorder`), plus `files` (names), `session` (options, pause causes,
  clock), `exclude` (Vale out of the video), `watch` (minimise/restore of the
  recorded window), `thumb` (thumbnail decoded from the saved file).
- `src/recorder/mix.rs` + `Recorder::start_mix` (thread `rec-mix`): phase 1's
  capture, resampler, timeline, mute and meters, summed through a soft limiter
  (knee 0.7, tanh above) into mono and pushed into the screen recorder.
- `src/capture/rec_pick.rs` (pure) and `src/capture/overlay/rec.rs`: the picker
  on the screenshot overlay (Layar / Jendela / Area, Mic and Suara PC toggles,
  Rekam). Jendela never picks a Vale window.
- `src/app_screenrec.rs` (child of `app`): picker, countdown, start, stop, pause,
  discard, notices, the Layar list.
- Ids: hotkey row 10 `RecordScreen` (`Ctrl+Alt+Shift+R`), countdown Esc is a raw
  `RegisterHotKey` id 13 for the 3 s only, timer 70 `TIMER_SCREEN_COUNTDOWN`,
  `WM_APP + 90` pick, `WM_APP + 91` window watch. 11, 71-74 and 92-94 stay free
  (11 is phase 4's zoom).
- Settings → Fitur → Rekam layar: 30 / 60 fps (`recording.video_fps`), video
  folder (`recording.video_folder`). Debug: `--screenrec-test <s>` with
  `--pause`, `--no-audio-push`; `--demo-snap rec`; `--demo-record` with
  `DI_REC_TAB=screen|screen-empty`, `DI_REC_LIVE=screen|screen-sure`.

## Decisions

- **One mono AAC track** (96 kbit/s), mic and PC audio summed, not averaged: a
  one-sided passage keeps its level, two loud sources never clip. Sidecar
  `"channels": ["mix"]`, so transcripts have no Saya/Peserta labels; an outside
  `.mp4` without a sidecar is downmixed unlabelled.
- **Crash safety:** written as `<name>.mp4.part`, fragmented MP4 first (plain MP4
  as fallback), renamed at stop, then the sidecar and `<name>.cursor.jsonl`. A
  killed recording leaves a playable `.part` the Layar tab lists as unfinished.
- **Audio timeline across pauses:** the screen recorder keeps every pause span;
  audio captured before a pause that arrives during it (the mix runs ~200 ms
  behind) still lands, trimmed at the pause start; the feed is drained once more
  at stop; stop order is mix first, then video, then the excluder. A loop gap
  over 2 s with no pause open is cut like a pause (a sleep the suspend notice
  missed).
- **Vale stays out of the video** only while a recording runs (screenshots and
  the owner's screen shares still see it): [[exclude-from-capture-in-context]].
- **Pause causes** tracked apart: the user, away (sleep and lock), the recorded
  window minimised; the user's own pause always wins. Wake while locked stays
  paused until the unlock. A closed window or a gone monitor saves with "Jendela
  ditutup, video tersimpan" / "Layar terputus, video tersimpan".
- **One recording at a time:** audio and screen block each other in every phase
  with "Rekaman lain sedang jalan".
- **Countdown** in the notch (hollow red ring, numeral, "Esc untuk batal"); it
  outranks the panel and island notices (they wait), a toast takes the slot
  beside it. The recording starts after the 1, so about the first second of
  speech can be lost if the narrator starts on the 1.
- **Sound that fails** (cannot start, or the last source lost): the video goes
  on and the notice is "Video jalan tanpa suara"; a silent video gets no
  transcript job and no Transkrip pill. Sound errors that race ahead of the
  start are buffered and shown once it succeeded.
- **Files:** `Videos\Vale\YYYY-MM-DD HHmm <window title>.mp4`, fallback
  `%USERPROFILE%\Vale\Video` when Controlled folder access refuses it (the
  owner's PC uses the fallback). A clash with any of the four files adds ` (2)`.
- **The Rekam page** (island Palette, all surfaces dark) uses the raised track
  with a `raised_hi` segment for the Audio / Layar switch and the failed pill's
  10 % red tint for "Yakin buang?" (the 22 % wash fell to 3.80:1 on Plum);
  `segment_fill` / `confirm_fill` are for light windows only.
- **Not in phase 3:** zoom and auto-zoom (hotkey 11), click ripples, webcam,
  editor. Accessible names for the picker's toggles are tooltips only (the
  overlay has no UI Automation provider). A tutorial about Vale itself cannot be
  recorded with Vale.

## Live check (2026-11-11, branch build on the owner's PC)

Passed: monitor (2560x1440, 30.00 fps, no gap > 33.3 ms, mono 48 kHz AAC),
window (follows moves, paused while minimised), area (video = the box), countdown
Esc (no file, Esc reaches other apps after), one at a time, pause from the page,
Buang to the Recycle Bin, kill leaves a playable `.part`, 60 fps, transcript
`.srt`/`.md` beside the video, a transcript job waits for the recording, idle
threads back to the start count, Settings / a new sticky note / the tray menu
not in the frames. Left by hand: [[recording-hand-checks]].
