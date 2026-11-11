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

The four small issues that live check found (late "Bikin transkrip" notice, Hapus
without confirmation, "0 s" for a file without a sidecar, live "Saya" lines that
later vanish) were fixed on 2026-11-09 (4a6294d, see [[transcript]]); a live
check passed all four. Still open from that check:

- [ ] Look at the red "Yakin?" while hovering and pressing it: the unit test says
      4.5:1 or more, the screenshot sampling read about 3.8-4.1 (anti-aliased
      text), so judge it by eye.
- [ ] The Indonesian "Yakin?" label (only the English "Sure?" was seen).
- [ ] A long (about 5 minute) recording still shows "Bikin transkrip" before
      "Transkrip siap".
- [ ] A live two-source transcript where a "Saya" line really is held back (the
      check never caught a "Saya" candidate on screen).
- [ ] "Transkrip file lain…" with a sidecar-less file (the length was checked
      from the recordings folder instead).

## Screen recording, phase 3 (2026-11-11)

Merged on `main` (ce67e90); the agent's live check passed the automatable lines
(see [[screen-recording]]). Vale's windows are left out of every capture while a
recording runs, so the agent could not see the notch, the panel or the notices
during one. For the owner by hand (test videos kept in `%USERPROFILE%\Vale\Video`:
`2026-11-11 1446`, `2026-11-11 1439 ValeTestWin`, `2026-11-11 1441`):

- [ ] Play them in Film & TV and VLC: narration and PC audio in both ears at
      their level, the cursor visible; the `.srt` shows as subtitles.
- [ ] A real voice; the first words right after the countdown's 1.
- [ ] By eye during a recording: "dijeda" in the notch (page pause and a
      minimised window), "Yakin buang?", "Rekaman lain sedang jalan" for the
      audio hotkey, "Jendela ditutup, video tersimpan" when the window closes.
- [ ] Whether Windows draws a yellow border around the captured area (Vale is a
      packaged app; a border would need the borderless capture capability).
- [ ] Sleep and lock pause the recording; wake or unlock resumes it.
- [ ] Controlled folder access refusing `Videos\Vale`: the fallback notice.
- [ ] A fullscreen video while recording: the notch hides, the tray dot stays.
- [ ] The picker by keyboard only (Tab, Space, Enter, Esc) and in the light
      theme and Terminal style; the Settings card in light and Terminal.
- [ ] Unplug the only mic mid-recording, and the mic refused in Windows
      privacy: "Video jalan tanpa suara" or the mic notice every time.
- [ ] The transcript window on a video (no Saya/Peserta, plays its audio) and the
      "Antre" row while a recording runs.
