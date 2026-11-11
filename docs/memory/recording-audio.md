---
tags: [decision, recording, audio, power, timers, hotkeys]
date: 2026-11-07
---

# Audio recording (phase 1 of recording)

Records the mic, the PC's audio (WASAPI loopback) or both to an `.m4a` with a
`.vale.json` sidecar. Design and contracts: `docs/superpowers/specs/2026-11-07-recording-design.md`,
plan: `docs/superpowers/plans/2026-11-07-recording-phase1.md`, mockups:
`docs/design/rekaman.html`. Transcript (track B) and screen recording (track C)
build on it. Related: [[controlled-folder-access]], [[media-foundation-dynamic-load]],
[[wasapi-capture-timestamps]], [[bata-idle-power-budget]], [[hotkeys]], [[exe-size]].
Mute, device choice and echo cancellation: [[recording-controls]].
Transcript of a recording (phase 2): [[transcript]]. Screen recording (phase 3)
uses the recorder's mono mix mode (`Recorder::start_mix`, thread `rec-mix`; the
file writer and the mix share one paced `pump`): [[screen-recording]].

## Where it lives

- `src/recorder/`: pure parts (`naming`, `sidecar`, `resample`, `timeline`,
  `level`, `prefs`), the AAC writer (`encode`), the WASAPI capture (`capture`,
  the API track C consumes) and the session (`mod.rs`: `Recorder`,
  `RecEvent`, `RecError`, `Failure`).
- `src/app_record.rs` (child of `app`): start, stop, pause, notices, the
  folder probe and fallback, the Rekam page list, sleep pause, shutdown.
- Notch `View::RecordCompact`, `Page::Record` ("Rekam", after Pomodoro),
  hotkey id 9 (`Ctrl+Alt+R`), tray "Mulai rekam" / "Hentikan rekam" and a red
  dot on the tray icon (`ui/tray_dot.rs`), Settings → Fitur → "Rekaman".
- Demo: `--demo-record`; debug: `--record-test <s> [mic|system|both]`
  (`DI_REC_DIR`), `DI_REC_TIMING` prints start and stop times.

## Decisions

- **Timeline, not "append what arrives".** Each packet is placed at its QPC
  stamp on a 48 kHz timeline (ch0 mic, ch1 system); gaps are zeros, late or
  overlapping samples drop, a paused span is cut out. Within 20 ms packets are
  laid end to end; past that a new run starts; clock drift is slewed one
  sample per packet.
- **Resampler:** polyphase windowed sinc with the exact ratio (160 phases for
  44.1 → 48 kHz), outside the timeline lock.
- **File:** AAC-LC 96 kbps (12000 B/s) for mono and stereo (8000 B/s mono is
  not a rate the MF encoder takes), fragmented MP4 with plain MP4 fallback,
  written as `.m4a.part`, renamed, then the sidecar. Name
  `YYYY-MM-DD HHmm <title>`, a clash (also with a `.part` or a sidecar) adds ` (2)`.
- **Threads only while recording:** `rec-capture` and `rec-writer` live from
  start to stop; start and stop themselves run on `rec-start` / `rec-stop`
  (start ~75-240 ms with the folder probe, stop ~40-50 ms) so the UI thread
  never waits; the list is read on `rec-scan` when the Rekam page shows.
  Every word from those threads carries the session number; an older
  session's word is ignored.
- **Timers:** `TIMER_REC` (60) one-shot to the next whole second for the
  clock; `TIMER_REC_LEVEL` (61) coalescable 120 ms one-shot only while the
  REC view is on screen. Both are killed when it leaves.
- **Not kept:** under 0.5 s is a quiet cancel; longer with no packet from any
  source (PC audio only while nothing played) is removed and says
  "Rekaman tidak disimpan · Tidak ada suara masuk" (`RecError::Silent`).
- **Sleep** pauses the recording and the wake resumes it, unless the user
  paused it themselves. Vale closing or the session ending finalizes it.
- **Consent:** a one-time notice on the first recording, not a dialog.
