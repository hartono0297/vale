---
tags: [decision, recording, audio, hotkeys, wasapi]
date: 2026-11-08
---

# Recording controls: mute, device choice, echo cancellation

Added on top of [[recording-audio]] (plan:
`docs/superpowers/plans/2026-11-07-recording-controls.md`).

- **Mute per source** (`Recorder::set_muted`): a muted source's samples are
  zeroed after resampling and before the timeline, so timing, channels and
  duration stay; its meter reads 0. Every recording starts unmuted; nothing
  goes in the sidecar. The notch shows a crossed mic or speaker instead of
  that source's bars; the Rekam page shows a round mute button per recorded
  source; the tray has "Bisukan mic" while recording.
- **Mute hotkey** id 12 (`HotkeyId::RecordMute`, `Ctrl+Alt+M`, mic only) is
  registered **only while a recording runs** (`wanted(&Hotkeys, recording)`
  in `sync_hotkeys`, called on start and stop), so the combo stays free for
  other apps at idle. See [[hotkeys]].
- **Device choice** `recording.mic_device` / `system_device` (endpoint ids,
  `None` = follow Windows). Picked from a native popup menu
  (`tray::device_menu`) in Settings → Fitur → Rekaman and on the Rekam page
  (click the device name), also while recording: `Capture::set_device`
  updates a shared `ChoiceSet` (ids plus a generation per source, so picking
  the same id again ends a fall-back) and wakes the capture thread. A chosen
  device that is missing or lost falls back to the default for that recording
  with a notice naming both; an automatic default-follow switch says "Mic
  pindah"; a hand pick is silent but re-reads the names. Endpoint names are
  read only when a page opens, a recording starts or ends, or a switch is
  reported: no cache, no watcher.
- **Echo cancellation** (`recording.echo_cancel`, default on): the mic opens
  in the **speech** category (`AudioCategory_Speech` through
  `IAudioClient2::SetClientProperties`) and
  `IAcousticEchoCancellationControl::SetEchoCancellationRenderEndpoint` points
  at the PC audio being recorded (re-pointed when that device switches).
  **Not the communications category**: it can trigger Windows'
  "communications activity" ducking (other apps turned down 80 % by default,
  and the meeting audio recorded quieter), and `SetDuckingPreference` cannot
  stop that from a capture stream (it fails with
  AUDCLNT_E_WRONG_ENDPOINT_TYPE). On the owner's PC (Realtek, Windows 11
  26200) both categories cut speaker sound in the mic by ~95 % (mic RMS 0.0014
  / 0.0002 speech, 0.0004 / 0.0012 communications, 0.024 / 0.020 off). A driver
  that rejects the category gets a plain stream. The transcript (track B)
  still drops "Saya" segments that repeat "Peserta" as a backup.
- Not checked by hand: a real voice on the mic with echo cancellation on
  (that it keeps the voice), plugging a headset mid-recording, the dark
  device menu.
