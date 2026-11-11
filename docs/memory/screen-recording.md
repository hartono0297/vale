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
  `WM_APP + 90` pick, `WM_APP + 91` window watch. Phase 4 took hotkey 11 (zoom)
  and `WM_APP + 92` (the wheel); 71-74, 93 and 94 stay free.
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
- **Not in phase 3:** zoom (hotkey 11) and the cursor effects came in phase 4
  (below); auto-zoom, webcam and an editor are not built. Accessible names for the picker's toggles are tooltips only (the
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

## Phase 4: zoom and cursor effects (2026-11-11)

Spec `docs/superpowers/specs/2026-11-11-screen-recording-phase4-zoom-design.md`,
plan `docs/superpowers/plans/2026-11-11-screen-recording-phase4-zoom.md`. Merged
on `main` at 79c6f68 (3751 tests). Built for tutorials: the owner talks while
clicking, the viewer must read small things.

- **Zoom by a key, never by a click** (the owner said "zoomnya jangan click"):
  `Ctrl+Alt+Z` (hotkey row 11, only during a screen recording, see [[hotkeys]])
  toggles the zoom in the video; the user's screen never changes. The camera
  (`screenrec/zoom.rs` `ZoomCamera`, from phase 3) eases 350 ms and follows the
  cursor inside the recorded monitor, window or area. Every recording starts
  unzoomed at 2×. Auto-zoom (`zoom::InputEvent`, `AutoZoom`) stays unwired.
- **Level:** while zoomed, `Ctrl+Alt` + wheel steps 0.5 between 1.5× and 4×
  (`zoom::next_level`, `MIN_STEP_LEVEL`). The wheel comes from a low-level mouse
  hook thread `vale-zoom-wheel` (`screenrec/wheel.rs`, `WM_APP + 92`) that runs
  only while zoomed: the proc reads exact Ctrl+Alt (no Shift, no Win),
  accumulates 120 per notch (smooth wheels add up, a reversal starts over),
  posts the steps and swallows the event so the app below does not zoom too.
  `WheelHook` unhooks in `Drop`; every end of a recording drops it.
- **The island's mirror:** `zoom::ZoomState` (on, level) changes on each press
  at once, and the same commands go to the recorder thread, which applies queued
  toggles before steps after a pause; both go through `next_level`, a step that
  does not change the level is never sent, so they always agree (two tests).
- **Notch:** a zoomed screen recording shows the level (`2×`, `2,5×`, English
  `2.5×`; `i18n::zoom_label`) where it says "Layar"; paused still says
  "dijeda". `RecStatus.zoom`, `RecFrame.zoom`. Demo: `--demo` with
  `DI_DEMO_STEP=screen-rec DI_REC_LIVE=screen-zoom`.
- **Cursor highlight:** a soft `#FFD54A` disc (radius 20 × cursor scale, 35 %)
  under the cursor, painted before the ripples and the cursor. It is in source
  pixels, so it grows with the zoom like the cursor.
- **Click ripples** (phase 3's `ripple_at`, off in phase 3) are on now.
- **Settings → Fitur → Rekam layar:** "Sorot kursor" and "Riak klik"
  (`recording.cursor_highlight`, `recording.click_ripples`, default on, lenient
  read), "· Berlaku mulai rekaman berikutnya": the start job snapshots them
  (`session::CursorEffects`), a running recording keeps its own.
- **Gotcha for checking frames:** the thumbnail reader (`thumb.rs`) lands on the
  key frame before the asked time, and the encoder's GOP is 2 s, so a frame
  grabbed at 21.5 s can show 20 s. A zoom-out looked 2 s late that way in the
  live check; it was the reader, not the camera.
- **Not built:** auto-zoom, the zoom editor after recording (the cursor log
  already keeps moves, clicks and `[t,"z",level]`), a box on the user's screen
  showing what is in the video, remembering the level.

Live check (2026-11-11, branch build): the log read zoom 2, 2.5, 3, 2.5, 0 for
in, two wheel notches up, one down, out; the process had one more thread only
while zoomed; another app could register `Ctrl+Alt+Z` before and after the
recording and not during it; frames showed the magnified area following the
cursor and the yellow disc, and no disc with the switch off. Left by hand:
[[recording-hand-checks]].
