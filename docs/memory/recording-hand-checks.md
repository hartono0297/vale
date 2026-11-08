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
