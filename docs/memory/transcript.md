---
tags: [decision, recording, transcript, whisper, power, queue]
date: 2026-11-09
---

# Transcript (recording phase 2)

Turns a recording into text on this PC, nothing uploaded. Spec:
`docs/superpowers/specs/2026-11-08-transcript-phase2-design.md`, plan:
`docs/superpowers/plans/2026-11-08-transcript-phase2.md`. Builds on
[[recording-audio]] and [[recording-controls]]. Related: [[bata-idle-power-budget]],
[[exe-size]], [[media-foundation-dynamic-load]], [[recording-hand-checks]],
[[transcript-defender-toast]], [[light-accent-contrast]].

## Where it lives

- `src/transcribe/`: the core (`catalog`, `download`, `store`, `engine`, `decode`,
  `chunk`, `echo`, `output`, `doc`, `queue`, `prefs`, `runner`, `gpu`).
- `src/app_transcribe.rs`: queue, worker, downloads, notices, settle timer.
- `src/app_transcript_win.rs` and `src/transcript_win/` (`window`, `player`,
  `layout`, `demo`): the window and the player. Row menu: `ui/transcript_menu.rs`.
- `src/mf.rs`: the one Media Foundation loader ([[media-foundation-dynamic-load]]).
- Store: `%APPDATA%\Vale\transcribe` (models, engine, VAD, `queue.json`).

## Decisions

- **Two models:** Whisper large-v3-turbo (536 MB) and Whisper small (172 MB);
  both Qwen3 entries left the catalog and the sweep deletes their files. Default
  by hardware: turbo with a hardware GPU (DXGI adapter), small without. The
  offer names the saved choice, else that default (555 MB with the 19 MB engine
  and the VAD). Language default `id`; `en` and `auto` also.
- **Auto switch** (default on), plus a one-time offer on the saved notice when
  Transkrip is off or has no model. Auto off: a "Transkrip" chip on the saved
  notice and a pill on the Rekam row.
- **Queue:** one job at a time. It holds by reason (`Hold`): `Recording` now,
  `ScreenRecording` reserved for track C (a dead variant under
  `allow(dead_code)`), and two reasons can hold at once. A held row says "Antre",
  not "Menunggu model" (that is only for a missing engine or model file).
  Persisted to `queue.json`, loaded at init. A one-shot `TIMER_TRANSCRIBE` (65)
  settles the queue 30 s after start so a boot does not start a job. Each saved
  job carries `tries`, bumped at start; `tries >= 2` at load fails the job
  instead of running it (crash loop guard). An orderly quit resets `tries` to 0
  (`save_queue_at_quit`), so closing Vale mid-job is not counted as a crash.
- **Downloads:** `Item::Setup(model)` is engine + VAD + that model, only what is
  missing. Nothing downloads without a click: a resume continues only parts on
  disk plus the clicked model's empty `.part` marker (`mark_setup`), and the
  sweep runs before it. Batal discards the parts and the marker.
- **Echo filter:** a "Saya" segment is dropped when it overlaps a "Peserta"
  segment in time and its words mostly match (`echo::drop_echo`); a real reply
  that overlaps stays.
- **File names:** plain `<stem>.md` and `<stem>.srt` only when free or Vale's
  own (the `.md` starts with Vale's header); else `<stem>.transcript.md/.srt`,
  also for a picked file with no sidecar. The sidecar records the names used. An
  empty result deletes only files the sidecar names.
- **Window:** a normal taskbar window (not the notch), `WM_APP + 85`, fed by a
  request queue; the live view refreshes once per drain of events, and
  `load_meta` serves the header while a job runs. One window per recording
  (opening it again reuses it). Buttons: copy, copy with a summary prompt, make a
  note (a long one holds the first 20 lines plus the `.md` path), open the
  `.md`, open the folder, transcribe again.
- **Player:** MF reader to WASAPI, downmixed so both ears hear it. The
  `transcript-player` thread exists only while playing; `WM_APP + 86`; window
  timers 1 and 2. Esc or the X button stops the sound.
- **Idle cost:** no thread, timer or engine DLL at idle with Transkrip on (live
  check: 25 to 27 threads at rest). After a job the worker is gone; the engine
  DLLs stay loaded, plus one thread started by the GPU driver.
- **Measured** (5 min of audio): turbo on the GPU 35 s, small on the CPU 99 s;
  a 20 s recording was transcribed 8.5 s after Stop.

## Polish (2026-11-09, plan `docs/superpowers/plans/2026-11-09-transcript-polish.md`)

- The start notice is its own kind, `NoticeKind::TranscriptStarting { audio }`:
  a `TranscriptReady` / `TranscriptFailed` drops a still-queued start notice of
  the same audio (`same_audio`), never one on screen and never another
  recording's. A short recording now reads "Rekaman tersimpan" then "Transkrip
  siap".
- Hapus on a model row takes two clicks: the first shows `Key::ConfirmShort`
  ("Yakin?") in `ButtonKind::Confirm` for `CONFIRM_MS` (Settings' own
  `TIMER_CONFIRM`, only while armed); a click on another control disarms; an
  in-use model is never armed (`transcript_click`, the layout, `armed_after`).
- A recording with no sidecar length gets it from the audio:
  `decode::probe_duration_ms` reads `MF_PD_DURATION` only, on the `rec-scan`
  thread, skipped for unfinished rows and for cloud-only files (offline /
  recall attributes from the directory listing, so OneDrive is never asked to
  download). The window takes sidecar, then the running job's total, then the
  scan.
- Live "Saya" lines are held until "Peserta" is settled at that time:
  `Event::Progress` carries `others_settled_ms` (`Chunker::settled_ms`; no
  "Peserta" segment can still start before it; `u64::MAX` when fully read or
  no "Peserta" channel), and `live_lines` shows a "Saya" line only when
  `end_ms + SLACK_MS < others_settled_ms`. Once shown, `drop_echo` can no longer
  remove it, so the finished file agrees with what was on screen. A CPU restart
  resets the mark to 0.
- Left for later: `Event::RecRecent` has no scan sequence, so a slow older scan
  can overwrite a newer one (the probe makes scans a little longer).
