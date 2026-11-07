---
tags: [plan, recording, audio]
date: 2026-11-07
spec: docs/superpowers/specs/2026-11-07-recording-design.md
---

# Recording controls: mute, device choice, echo

Asked by the user on 2026-11-07 after the phase 1 merge ("bisa ga bikin kita
bisa mute juga dan bisa switch audio"). The user picked: mute per source,
choose the device, follow device changes automatically, a mute hotkey, and the
speaker echo fix; "sisanya u yang atur terbaiknya gimana". Builds on
[[recording-audio]] (`docs/memory/recording-audio.md`) in branch
`rec-controls`, worktree `.claude/worktrees/rec-controls`. Every task is
test-first and ends green (`cargo test --locked`, 0 build warnings) with a
commit.

## What already exists (do not rebuild)

- `recorder::capture` already follows the Windows default device: every
  `DEVICE_EVERY` (1 s) `serve` compares the default endpoint id with the open
  stream's id and reopens on a change, and a lost device is retried for
  `RETRY_FOR` (2 s) before `sink.lost`. The timeline places packets by QPC, so
  a reopen just continues (a rate change starts a new run).
- Start and stop run off the UI thread (`app_record.rs`), the REC notch view,
  the Rekam page, hotkey id 9, the tray entry and red dot, the Settings
  "Rekaman" card.

## Decisions

- **Mute** keeps the recording and its channels: a muted source's samples
  are written as zeros (the packets still arrive and keep the timeline), its
  meter reads 0 and shows a muted glyph. Both the mic and the PC audio can be
  muted. Every new recording starts unmuted. Nothing about mute goes in the
  sidecar (the transcript sees silence).
- **Mute hotkey** id 12 (`HotkeyId::RecordMute`, config key `record_mute`,
  default `Ctrl+Alt+M`, label "Bisukan mic rekaman" / "Mute recording mic"),
  toggles the **mic** only, and is **registered only while a recording runs**
  (`sync_hotkeys` treats the row as off otherwise; start and stop call it), so
  Ctrl+Alt+M is not taken from other apps at idle. Settings → Tombol pintas
  lists it like the others. Update the spec's reserved-id table (A: 9, 12).
- **Device choice** per source: `recording.mic_device` and
  `recording.system_device`, `Option<String>` endpoint ids, `None` = "Ikuti
  Windows" (the default, and the current behaviour). A chosen device that is
  missing at start, or lost while recording, falls back to the Windows default
  for that recording, with an info notice ("Mic <name> tidak ada, pakai
  <default>"); the choice in config stays. With "Ikuti Windows", an automatic
  switch gives an info notice ("Mic pindah ke <name>") so the user knows.
- **Switching live:** picking a device while recording switches that source
  at once (`Capture::set_device`, shared choice + the capture thread's wake
  event), no restart, no gap beyond the reopen (~50-150 ms of zeros).
- **The picker** is a native popup menu (`TrackPopupMenu`, dark-mode aware as
  in `ui/tray.rs`): "Ikuti Windows (<default name>)", a separator, every
  active endpoint of that flow by friendly name, a check on the current one.
  It opens from the Settings "Rekaman" card (rows "Mic" and "Suara PC dari",
  showing the current name) and from the Rekam page (the device name under
  each meter row, clickable, also while recording). The endpoint list is read
  when the menu opens (no cache, no watcher at idle).
- **Echo:** with `recording.echo_cancel` (default on, Settings switch
  "Peredam gema mic" / "Mic echo cancellation") the mic stream is opened in
  the communications category (`IAudioClient2::SetClientProperties`,
  `AudioCategory_Communications`) so the device's or Windows' echo
  cancellation runs, and on Windows 11 builds that have it,
  `IAcousticEchoCancellationControl::SetEchoCancellationRenderEndpoint` points
  it at the render endpoint being recorded. The session opts out of ducking
  (`IAudioSessionControl2::SetDuckingPreference(TRUE)`) so other apps are not
  turned down. Every one of these calls is best effort: a failure leaves a
  plain stream and is logged once. The loopback stream is never changed.
- **Echo backup in the transcript** (track B, not this branch): a "Saya"
  segment whose text mostly repeats a "Peserta" segment overlapping it in time
  is dropped. Added to track B's integration list.

## Tasks

### T1. Mute (standard model)

- `Recorder::set_muted(source, bool)`, `muted() -> [bool; 2]` on atomics
  shared with the placer; the placer zeros a muted source's samples before
  they go on the timeline and the level meter publishes 0 for it. Tests with
  the fake capture: a mute span writes zeros in that channel only, the other
  channel and the duration are untouched, unmute restores audio.
- `RecStatus` gains `muted: [bool; 2]`; the REC compact view draws a small
  crossed mic (or crossed speaker) instead of that source's bars; demo scene
  `rec-muted`.
- Rekam page: while recording, a toggle button next to each recorded
  source's meter (mic, speaker icons; muted state in `REC_RED`), hits →
  `Effect::RecMute(Source)`.
- Tray: while recording, "Bisukan mic" / "Nyalakan mic" under the stop entry.
- Hotkey id 12 as decided, with `plan_row` tests that the row is off unless
  recording, and `on_hotkey` → mic mute toggle. i18n both languages, leak
  tests green.

### T2. Device choice and live switching (standard model; capture parts need care)

- `recorder::devices`: `list(flow) -> Vec<Endpoint { id, name }>` (active
  endpoints, `PKEY_Device_FriendlyName`), `default_endpoint(flow)`.
- `capture`: `Capture::start(sources, choices, sink)` where `choices` is a
  shared `[Option<String>; 2]`; `serve` opens by id (`GetDevice`) when a
  choice is set, else the default; a missing or lost chosen device falls back
  to the default (reported through a new `AudioSink::switched(source, name,
  fell_back: bool)`); `Capture::set_device(source, Option<String>)` updates
  the choice and wakes the thread. Keep the existing default-follow and retry
  logic and its tests; add tests with a fake device layer where practical
  (the choose-the-endpoint decision as a pure function: wanted, default,
  available ids → which id to open and whether it fell back).
- `Recorder::set_device`, `RecEvent::DeviceSwitched { source, name,
  fell_back }` → info notice (both languages).
- `config.recording.mic_device` / `system_device` (lenient read, round trip
  tests), Settings rows and the Rekam page device labels with the popup menu
  as decided; a pick while recording calls `Recorder::set_device`.

### T3. Echo cancellation (standard model, then a live check)

- `Stream::open` for the mic: the communications category, the AEC render
  endpoint where the interface exists, the ducking opt-out, all best effort
  as decided; `recording.echo_cancel` (default true) passed through
  `RecOptions` and `Capture::start`. Unit tests for the config; the Windows
  calls are checked live.
- Settings switch "Peredam gema mic". Live check: play speech through the
  speakers with nobody talking, record both, compare the mic channel's level
  with the switch on and off (`--record-test` gains `--no-aec`).

### T4. Review and live check

Whole-branch review (most capable model) against this plan. Live: mute and
unmute from the page, the hotkey and the tray (the hotkey is free again after
stop); pick a device while recording; plug or switch a headset while
recording with "Ikuti Windows"; the echo check of T3.
