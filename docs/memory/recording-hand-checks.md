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

## Screen recording, phase 4: zoom and cursor effects (2026-11-11)

Merged on `main` (79c6f68); the agent's live check passed (see
[[screen-recording]]). Test videos from that check: `%USERPROFILE%\Vale\Video\2026-11-11 1818.mp4`
(zoom in, wheel to 3×, back, out) and `2026-11-11 1828.mp4` (both switches off).
For the owner by hand:

- [ ] Play `2026-11-11 1818.mp4`: the zoom in and out feel smooth (about a third
      of a second), the camera follows the cursor without seasickness, and the
      zoom-out happens right away (frame grabs suggested a 2 s delay, believed
      to be the key-frame seek, not the video).
- [ ] A click ripple is visible in the video (no grabbed frame caught one).
- [ ] The yellow disc at 3-4× (it grows with the zoom, about 80 px at 4×): fine,
      or too big?
- [ ] By eye during a recording: the notch says `2×`, then `2,5×` / `3×` after
      `Ctrl+Alt` + scroll, and "Layar" again after zooming out; "dijeda" while
      paused.
- [ ] `Ctrl+Alt` + scroll while zoomed does not also zoom the app under the
      cursor (a browser, Figma); `Ctrl+Alt+Shift` + scroll still reaches it.
- [ ] `Ctrl+Alt+Z` while paused, then resume: the video and the notch agree.
- [ ] Zoom over an app run as administrator: whether the wheel still changes
      the level there (Windows may keep a low-level hook out of elevated windows).
- [ ] Settings → Tombol pintas: turn "Zoom video rekaman" off while zoomed; the
      video zooms out.
- [ ] The two new switches in Settings → Fitur → Rekam layar with the Terminal
      style.

## Video quality (2026-11-12)

The agent recorded at each quality with the debug runner and checked the
Settings row in a demo window ([[recording-quality]]). Still by hand, in the
real app:

- [ ] Record a whole screen at Asli, 1440p, 1080p and 720p; Explorer →
      Properties → Details shows 2560×1440, 2560×1440, 1920×1080, 1280×720.
- [ ] Record a small window (under 720 px high) at 1080p: the video keeps the
      window's own size.
- [ ] Record a dragged area at 720p and at Asli.
- [ ] Zoom (`Ctrl+Alt+Z`) at 720p: the zoomed text is readable.
- [ ] A one-minute recording with movement (scrolling a page, a video) at
      1080p 30 fps: the file is at most about the estimate (± 36 MB).
- [ ] Change the quality while a recording runs: the running video keeps its
      size, the next one takes the new choice.
- [ ] The row with the Terminal style and in English.

## Phase 5 live captions (2026-11-11)

Branch `feat/captions` (54377fa), release build on the owner's PC, one agent
instance at a time. Spec: `docs/superpowers/specs/2026-11-11-live-captions-phase5-design.md`.

Verified by the agent on the real app (the speech was synthetic test speech,
Windows SAPI, not a person):

- `Ctrl+Alt+C` opened the card (380x220, topmost, no-activate, tool window) and
  the foreground window did not change, on and after speech; off removed it.
- Speech played through the speakers (Windows SAPI, English, turbo model on the
  GPU, saved language `id`) became lines: "Listening..." first, then grey and
  final lines. First change in the card 2.7 s after the speech started (about
  0.4 s of that is the speech engine starting), then a change every 1 to 2 s up to
  the last line at 17.5 s. GPU latency is fine.
- A fullscreen browser window (F11, 2560x1440, an animated page): the card is
  the window at its own centre (on top), the hotkey did not move the foreground,
  a click on the card left the browser foreground and fullscreen, the card
  stayed and kept its lines.
- Transkrip off (`transcript.enabled` false, restored after): the card says
  "Captions need Transcripts" with a "Turn on Transcripts" button.
- Threads at idle: 23, listening 26 to 27, after speech 32, off +6 s 31, off
  +66 s 26 (likely Windows thread-pool workers decaying plus one AMD driver
  thread: the attribution measured in the thread-count fix round, see
  [[transcript]]; Vale's own threads went 6, 8, 6).
- A real screen recording of the monitor with the card open (13.5 s, a 2560x1440
  file, 6.2 MB, deleted): while it ran the card had `WDA_EXCLUDEFROMCAPTURE` (0x11),
  a screen grab of the card's rectangle showed what lies behind it, and the
  affinity went back to 0 after the stop.
- Not decoded: the `.mp4` frames themselves (no frame grabber on this PC), so
  "the card is not in the video" rests on the display affinity and the grab.

For the owner by hand:

- [ ] Play a screen recording made with the card open (Film & TV or VLC): the
      card is nowhere in the video, and the video keeps its normal pictures.
- [ ] A real fullscreen YouTube video (not F11): the card stays over it, a click
      on the card keeps the video fullscreen, the video keeps playing.
- [ ] Drag the card by its header, resize it by the corner grip, scroll the
      wheel over the lines (the card was never activated: check the cursor
      shapes, the grip on hover and that no click steals focus from the app
      underneath).
- [ ] The header buttons by a real click: pause / resume pauses and resumes the
      lines, `Copy` puts the lines on the clipboard, the `ID` chip shows the
      language, X closes the card and turns captions off.
- [ ] The status strip under the lines, by a real click: with lines on the card
      and no model, "Unduh model" starts the download; with Transkrip off,
      "Nyalakan Transkrip" turns it on and captions continue; on battery,
      "Tetap jalan" runs this session (the card was never activated: the click
      must not take focus from the app underneath).
- [ ] Unplug the charger while captions run: the card shows "Pakai baterai"
      and recognition stops.
- [ ] The same unplug: the notice appears once, not on every 30 s check.
- [ ] Replug the charger: captions resume on their own.
- [ ] On battery, click "Tetap jalan": this session runs on battery; the next
      session on battery stops again.
- [ ] Turn captions on while on battery: the card opens already in "Pakai
      baterai".
- [ ] "Mic juga" in a real call or a voice note: the labels "Saya" and "Peserta"
      are right, and "Saya" lines that only repeat what the speakers just
      played (the mic picking up the PC audio) are dropped (echo).
- [ ] A real Zoom or Teams meeting for 10 minutes: the lines keep up, nothing
      grows slow, and the recording (if one runs) has no dropouts.
- [ ] A headset rate change mid-session (plug or unplug a headset, or switch
      the output device): captions keep going or recover within a few seconds.
- [ ] Sleep and resume, and lock and unlock, while captions run: no burst of old
      lines, no frozen card, captions continue or say why they stopped.
- [ ] A laptop without a GPU (or a forced CPU run): captions still run, with the
      "Slow" hint if the engine falls behind; the turbo model on the CPU took
      about 6 s per grey line in the CLI debug run of the worker task, so try `whisper-small`.
- [ ] Another app already holds `Ctrl+Alt+C` (an IDE or remote-desktop client):
      Settings, Tombol pintas shows the Captions row red ("Sudah dipakai"), the
      chord does nothing for Vale, and the tray entry and Rekam switch still
      turn captions on.
- [ ] Change the chord in Settings to another one: the new chord toggles
      captions and the old one no longer does.
- [ ] A fullscreen game: the chord still toggles captions (expected, as for the
      other hotkeys); note whether the card shows over the game.
- [ ] Reduced motion in Windows (Settings, Accessibility, Visual effects,
      Animation effects off): the card appears, scrolls and fades without
      animation.
- [ ] Notch line: new words appear in the closed notch and vanish 8 s after the
      last one.
- [ ] Notch line: a long line is cut from the left with a leading ellipsis.
- [ ] Notch line during a recording: the REC view stays and carries the line;
      paused shows "dijeda" with no line.
- [ ] Settings, "Caption di notch" off: no line in the notch, the card still
      works.
- [ ] The Rekam page captions switch: flips captions on and off and agrees with
      the card.
- [ ] The tray menu entry for captions: toggles captions, shows its state.
- [ ] The Settings card "Caption langsung" with the Terminal style: readable,
      switches work.
- [ ] The off-state switch track on the dark island surface is faint (about
      1.3:1, shared with "Notifikasi Jeda", a known follow-up): decide whether
      it is acceptable.
- [ ] Turn captions on and off ten times in a row, then watch the Task Manager
      thread count of Vale for two minutes: it settles near the idle number.
- [ ] Start captions while a transcript job is running: the job finishes,
      captions run and nothing crashes; a queued job waits until captions are off.
- [ ] A 30+ minute session reaching the 500-line cap: the card stays smooth
      when new lines arrive and while resizing, and the oldest lines drop.

