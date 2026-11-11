---
tags: [plan, recording, audio]
date: 2026-11-07
spec: docs/superpowers/specs/2026-11-07-recording-design.md
---

# Recording phase 1: audio recording (track A)

Goal: record the mic, the system audio (WASAPI loopback) or both to an `.m4a`
with a sidecar, with a REC view in the notch, a Rekam page, hotkey id 9, tray
items with a red dot, a saved notice, a Settings card and a demo. Mockups:
`docs/design/rekaman.html` sections 1, 2 and 7. Contracts (file layout, sidecar,
reserved ids) are in the spec and are binding.

Branch `rec-audio`, worktree `.claude/worktrees/rec-audio`. Every task is
test-first and ends green (`cargo test --locked`, 0 warnings) with a commit.

## Decisions taken while planning

- **Timeline, not "append what arrives".** Loopback delivers no packets in
  silence and the mic and the render device run on different clocks. Each
  packet is placed on a shared timeline by its QPC position
  (`IAudioCaptureClient::GetBuffer`'s `u64QPCPosition`); the writer pulls
  10 ms blocks up to `now - 200 ms`; gaps are zeros, late or overlapping
  samples are dropped. Pause drops everything between pause and resume and
  shifts later positions back by the paused span.
- **Resample per source** to 48 kHz and down-mix each source to mono
  (channel 0 = mic, channel 1 = system; mono file with one source).
- **Container.** Try the fragmented MP4 container
  (`MFTranscodeContainerType_FMPEG4`) so a crash leaves a playable file; fall
  back to `MPEG4` if the sink writer refuses it. Extension `.m4a` either way.
  The file is written as `<name>.m4a.part` and renamed on finalize; the sidecar
  is written after the rename.
- **Pause and stop** live on the Rekam page (and the hotkey toggles start and
  stop). No hover buttons on the compact view in this phase; a click on the
  REC view opens the Rekam page.
- **Consent** is a one-time notice on the first start ("Ingat: minta izin
  peserta sebelum merekam" / "Remember to ask everyone before recording"),
  not a modal dialog; `recording.consent_shown` flips to true.
- **Levels.** Two small bars (mic, system) driven by peak meters in atomics,
  redrawn by `TIMER_REC_LEVEL` (61, one-shot re-armed every 120 ms) only while
  the REC compact view is on screen, not fullscreen, display on and Motion is
  not Off. The clock text rides `TIMER_REC` (60, one-shot to the next second,
  like `TIMER_POMO`). Both are killed the moment the view leaves the screen.
- **Mic permission.** `E_ACCESSDENIED` on activation or `Initialize` gives a
  notice whose click opens `ms-settings:privacy-microphone`.
- **No transcript UI** in this phase (track B adds it).

## Tasks

### T1. Pure recorder parts (cheap model)

`src/recorder/` with `mod.rs` declaring the modules. All pure, unit-tested:

- `naming.rs`: `file_stem(started: NaiveDateTime, title: &str) -> String`
  (`YYYY-MM-DD HHmm <title>`, title trimmed, characters invalid in Windows
  names removed, at most 60 characters, empty title gives just the time),
  `unique_path(dir, stem, ext, exists: impl Fn(&Path) -> bool) -> PathBuf`
  (` (2)`, ` (3)`).
- `sidecar.rs`: `Sidecar { v, kind, title, started, duration_ms, channels,
  app }` to and from `serde_json::Value`, `merge_into(existing: Value) ->
  Value` keeping unknown keys (B's `"transcript"`, C's `"cursor_log"`).
- `resample.rs`: `Resampler::new(from_hz, to_hz)` streaming linear-phase
  resampler (windowed sinc or cubic; quality good enough for speech, state
  across calls), `downmix(interleaved, channels) -> mono`.
- `timeline.rs`: `Timeline::new(rate, channels)`, `put(channel, start_frame:
  i64, samples)`, `take(up_to_frame) -> Vec<f32>` interleaved, zero-fill
  gaps, drop late/overlap, `pause_at(frame)` / `resume_at(frame)` that remove
  the paused span, `qpc_to_frame(qpc, qpc_freq, origin)`.
- `level.rs`: peak meter with 300 ms decay, `f32` 0..=1, stored as atomics
  (`AtomicU32` bits) for the UI.

### T2. AAC writer (capable model, Media Foundation)

`src/recorder/encode.rs`: `M4aWriter::create(path, channels: 1|2,
rate: 48000) -> Result<Self, EncodeError>`, `write(interleaved_f32)`,
`finish() -> Result<Duration>`. `MFStartup` once per writer thread, sink
writer from URL with the container attribute (FMPEG4 then MPEG4), input
`MFAudioFormat_Float` 48 kHz, output `MFAudioFormat_AAC` 96 kbps stereo /
96 kbps mono (`MF_MT_AUDIO_AVG_BYTES_PER_SECOND` 12000 for both; 8000 is not a documented rate), sample
times in 100 ns units from the frame count. `EncodeError::NoMediaFoundation`
when `MFStartup` fails (N editions). Test: write 2 s of a 440 Hz tone to
`target/test-tmp/`, then read it back with `IMFSourceReader` and assert the
duration within 50 ms and the channel count. Adds
`Win32_Media_MediaFoundation` to `Cargo.toml`.

### T3. Capture worker and the recorder session (capable model)

- `src/recorder/capture.rs`: `Sources { mic: bool, system: bool }`,
  `trait AudioSink: Send { fn audio(&mut self, source: Source, qpc: u64,
  rate: u32, mono: &[f32]); fn lost(&mut self, source: Source, err: CaptureError); }`,
  `Capture::start(sources, sink) -> Result<Capture, CaptureError>`,
  `Capture::stop(self)`. Thread `rec-capture`, COM MTA, default capture
  endpoint (`eCapture`, `eConsole`) and default render endpoint in loopback,
  event-driven (`AUDCLNT_STREAMFLAGS_EVENTCALLBACK` with a wait on both
  events, 100 ms timeout), `parse_format`/`read_format` reused from
  `eq_audio.rs` (move them to a shared spot only if it stays small), the
  silent flag gives zeros, device loss (`AUDCLNT_E_DEVICE_INVALIDATED`) is
  reported through `lost`, `E_ACCESSDENIED` maps to
  `CaptureError::MicDenied`. This is the API track C consumes.
- `src/recorder/mod.rs`: `Recorder::start(opts: RecOptions { sources, dir,
  title, app }, tx: Sender<Event>, wake) -> Result<Recorder, RecError>`,
  `pause()`, `resume()`, `stop() -> RecResult { path, duration }`,
  `elapsed()`, `levels() -> (f32, f32)`, `paused()`. The writer thread
  `rec-writer` pulls from the timeline every 50 ms, resamples, writes,
  finalizes on stop, renames `.part`, writes the sidecar, then sends
  `Event::Rec(RecEvent::Saved { path, duration })` or `RecEvent::Failed(..)`.
  No thread exists outside a recording.
- Debug CLI (debug builds): `--record-test <seconds> [mic|system|both]`
  writes to the scratch folder given by `DI_REC_DIR` or `target/rec-test/`
  and prints the path, the duration and both peak levels.

### T4. Notch REC view (cheap model, many touch points)

Following the pomodoro compact view everywhere it appears (see the
integration map in the session notes): `View::RecordCompact` chosen in
`base_view` after the sticky notice and before pomodoro; `ShapeKind::RecordCompact`
(width extra about 64 dip for dot, clock and the two bar groups, robot extra like
pomodoro); `paint_record_compact` (red dot `theme::SOFT_RED` or the paused
ochre, `mm:ss` or `h:mm:ss` in tabular figures, "dijeda" label when paused,
two four-bar meters in the mic and system tints, the cat with headphones is
**out of scope**: the existing robot face is drawn as for pomodoro);
`Frame` gets a `rec: Option<RecFrame>`; `TIMER_REC` (60) and
`TIMER_REC_LEVEL` (61) in `sync_timers` and the id test; click opens
`Page::Record`; fullscreen hides it (already by `base_view`); demo scenes
`rec-both`, `rec-mic`, `rec-paused`, `rec-long` (1 h 02 m) and
`--demo-record`.

### T5. Rekam page (cheap model)

`Page::Record` appended to `Page::ALL` (config name `"Record"`, slotted
after `Pomodoro` in `from_saved`), icons `Icon::Mic` and `Icon::Record`,
`page_icon`, `page_key`, label keys. Layout and paint per mockup section 2
minus the transcript toggles: the big round button (start / stop),
pause/resume next to it while recording, the source segmented control
(Mic / Suara PC / Dua-duanya, disabled while recording), the last five
recordings from the folder (newest first: title or time, date and
duration from the sidecar, click reveals the file, "Buka folder" link).
The folder scan runs on demand when the page opens (not on a timer), off the
UI thread if it can take long. Hits and `Effect::RecToggle`,
`Effect::RecPause`, `Effect::RecSources(Sources)`, `Effect::RecReveal(path)`,
`Effect::RecOpenFolder`.

### T6. Hotkey, tray, notices, Settings, config (cheap model)

- `config.recording` (`RecordingPrefs { sources, folder: Option<PathBuf>,
  consent_shown }`, lenient like `widgets`, in `Default` and `merge_owned`).
  Folder default: `FOLDERID_Documents\Vale\Rekaman`, created on first start.
- `HotkeyId::Record` (row 9, `RegisterHotKey` id 9, config key `record`,
  default `Ctrl+Alt+R`), labels "Rekam audio" / "Record audio".
- Tray: "Mulai rekam" / "Hentikan rekam" entry; while recording the tray
  icon gets a red dot (`NIM_MODIFY` with a composited `HICON`, restored on
  stop, re-applied after a taskbar re-create).
- Notices: `RecSaved { path }` ("Rekaman tersimpan · 42 mnt · klik untuk
  buka", click reveals), `RecFailed { reason }`, `RecMicDenied` (click opens
  the Windows mic privacy page), the one-time consent notice.
- Settings → Fitur → "Rekaman" card: sources segmented, folder row with
  "Buka" and "Ganti" (folder picker), the hotkey lives in Tombol pintas.
- i18n for everything, both languages, leak tests green.

### T7. Review and live check

Whole-branch review (most capable model) against the spec and this plan.
Live check by the session: `--demo-record` screenshots, `--record-test 5
both` while audio plays and the mic hears speech (file plays, two
channels, right levels), the real app: hotkey start/stop, pause, the
saved notice, the tray dot, fullscreen hide, idle check (no `rec-*` thread
after stop).
