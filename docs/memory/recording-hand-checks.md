---
tags: [recording, todo, verification]
date: 2026-11-08
---

# Recording: checks still to do by hand

Phase 1 (audio recording) is merged and pushed (`d44973c`, 3326 tests). The
owner has recorded a few seconds and 5 minutes (a YouTube video) without
trouble. These were never checked by a person on the real app; tick them off
here when done. Related: [[recording-audio]], [[recording-controls]],
[[notch-rec-mute]], [[menu-pop]].

- [ ] The device dropdown with a real mouse (Rekam page and Settings → Fitur →
      Rekaman): open, hover, pick, click outside. Only keyboard and simulated
      clicks in the demo were tested.
- [ ] Focus goes back to the previous window after the dropdown closes.
- [ ] The dropdown in the light theme in the real app (only the demo was seen).
- [ ] The dropdown at a higher DPI / on a second monitor.
- [ ] A real voice on the mic with echo cancellation on: the voice stays clear
      while PC audio plays (only a played WAV was measured).
- [ ] Plug in or pull out a headset mid-recording: the "Mic pindah" notice and
      the switch, and no gap in the file.
- [ ] Sleep while recording: the recording pauses and the file is kept.

## Transcript, phase 2 (2026-11-09)

Merged on `main` (3bf0853); the live check passed 11 of 12 lines, see
[[transcript]]. Still for the owner by hand:

- [ ] Hear the playback from the transcript window in both ears (the meter read
      equal on both channels, but nobody listened).
- [ ] Paste "Salin + prompt ringkasan" into a chat.
- [ ] Open the `.md` with an app associated to `.md` (the test PC had none).
- [ ] The echo check on a real call with a person on speakers.
- [ ] Decide whether to allow `vale.exe` in Controlled folder access
      ([[transcript-defender-toast]]).

Known small issues from the live check, not fixed:

- The "Bikin transkrip" notice can arrive after a short job is already done
  (notices queue behind "Rekaman disimpan").
- Hapus on a model row has no confirmation (one click removes 172 or 536 MB).
- A picked file with no sidecar shows "0 s" on the Rekam row and no meta line in
  the window header.
- The live view can show "Saya" lines that the final result drops as echo.
