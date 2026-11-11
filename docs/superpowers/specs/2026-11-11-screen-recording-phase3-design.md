---
tags: [spec, recording, screen-recording]
date: 2026-11-11
status: draft, waiting for the owner's OK
---

# Screen recording, basic (recording phase 3)

Builds on the approved recording design
(`docs/superpowers/specs/2026-11-07-recording-design.md`, "the 11-07 design",
phases 3 and 4, track C), its mockups (`docs/design/rekaman.html` sections 8,
10, 12), track C's core (branch `worktree-agent-a2b34453550627f70`, 4 commits
on top of `5ecff05`: `src/screenrec/` capture, compositor, zoom camera, cursor
log, MP4 encoder, recorder, debug `--screenrec-test`), and what phases 1 and 2
put on `main` (recorder, mute, devices, echo cancellation, transcript queue
with its `Hold::ScreenRecording`, `crate::mf`). This file says what changes or
is added; everything else in the 11-07 design stands.

Related notes: [[recording-audio]], [[recording-controls]], [[notch-rec-mute]],
[[transcript]], [[controlled-folder-access]], [[media-foundation-dynamic-load]],
[[screenshot-snap]], [[hotkeys]], [[bata-idle-power-budget]], [[exe-size]].

## Intent

The owner wants to record the screen so they can make tutorials for other
people ("siapa tau nanti ada yang butuh tutorial, jadi gw bisa rekamin"). The
viewer is someone else watching an `.mp4`: the picture must be clear, the
narration audible in both ears, the cursor visible, and Vale itself must not
appear. The owner approved the flow below on 2026-11-11 ("ga ada, lanjutkan")
and left every other choice to the design author ("u atur yang terbaiknya
gimana"); those are marked **(builder's call, made here)**.

## Goals

1. Start from `Ctrl+Alt+Shift+R` (hotkey id 10), the Rekam page's "Layar" tab
   or the tray; pick **Layar** (one monitor), **Jendela** (one window, followed
   when moved) or **Area** (a dragged box) with the screenshot tool's picker
   (snaps to windows and boxes); choose Mic and Suara PC on the picker's small
   toolbar; **Rekam**.
2. A 3, 2, 1 countdown in the notch; Esc cancels.
3. While recording: the notch shows the red dot, the time, "Layar" and the mic
   level group (click to mute, as in phase 1); hovering the notch offers
   pause, stop and discard. No Vale window (notch, panel, Settings, transcript
   window, sticky notes, lyrics card) appears in the video; the cursor does.
4. Stop with the same hotkey, the notch or the tray. A notice with a small
   thumbnail of the video: "Video tersimpan · <length> · klik untuk buka".
5. The `.mp4` lands in `Videos\Vale\`, with the same Controlled-folder-access
   fallback as audio.
6. If transcripts are on with auto on, the video is transcribed too and its
   `.srt` sits next to the `.mp4` under the same name, so VLC and Film & TV
   show it as subtitles.

## Non-goals

- Zoom (manual `Ctrl+Alt+Z`, auto-zoom on click), cursor highlight and click
  ripples: phase 4. Track C's zoom camera and cursor log stay in the code; the
  zoom hotkey (id 11) is not registered in phase 3. **(builder's call, made
  here:** keep writing the cursor log sidecar, it is cheap and phase 4's editor
  needs it.)
- Webcam overlay, a video editor, trimming, GIF export, live captions.
- Recording audio and screen at the same time (one recording at a time).
- Uploading or sharing anything.

## Anti-goals

- Vale's own windows visible in a video.
- A video whose narration sits in one ear.
- Any thread, capture, timer or poll while not recording ([[bata-idle-power-budget]]).
- Dropped or stuttering frames on an ordinary laptop at the default settings.
- The owner's screen changing in any way because of the recording (no
  magnifier, no resolution change).

## Decisions (builder's calls, made here)

- **Video:** 30 fps by default, 60 fps in Settings; resolution = the captured
  area at its pixel size (track C's minimum video size rule stays); H.264 with
  the hardware encoder when present, software fallback; bitrate from track
  C's `plan::bitrate`.
- **Audio in the video:** one AAC stereo track, mic and PC audio **mixed** into
  both ears (unlike the meeting `.m4a`, which keeps them on separate
  channels). Mute, the device choice and echo cancellation from phase 1 apply
  unchanged, through the recorder's capture API that track C consumes.
- **Transcript of a video:** the existing queue takes `.mp4` jobs (decode the
  audio track); with the mixed track there are no Saya/Peserta labels. Writes
  `<stem>.srt` and `<stem>.md` next to the video by phase 2's naming rules; the
  window and player work on `.mp4` too (audio only in the window).
- **Holds:** a screen recording sets `Hold::ScreenRecording` (the queue already
  has it); an audio recording and a screen recording cannot run at once (the
  second start says "Rekaman lain sedang jalan").
- **Settings → Fitur → Rekam layar:** video folder (Buka / Ganti), 30 / 60 fps,
  and the hotkey row in Tombol pintas like the other hotkeys.
- **Multi-monitor:** "Layar" records the monitor clicked in the picker; the
  notch keeps its own monitor rule.
- **Rekam page:** an "Audio" / "Layar" switch at the top; "Layar" lists the 5
  newest videos like the audio list (length, transcript pill, right-click
  menu, "Buka folder", "Transkrip file lain…" accepting `.mp4` too).
- **Files:** `Videos\Vale\YYYY-MM-DD HHmm <title>.mp4` (title from the
  recorded window or empty), written as `.mp4.part` and renamed at stop, plus
  the `.vale.json` sidecar (`kind: "screen"`, `cursor_log`), per the 11-07
  contract. A crash leaves a `.part` the Rekam page lists as unfinished.
- **Minimised or closed window:** the recording pauses while the window is
  minimised and stops (saving what it has) when the window closes, with a
  notice saying why.
- **Fullscreen apps:** recording continues; the notch hides as in phase 1; the
  tray dot stays.
- **One Media Foundation loader:** track C's own `screenrec::mf` is replaced by
  `crate::mf` when the branch is integrated.
- **The audio track is mono AAC** (mic and PC audio mixed with a soft limiter,
  not a plain mean, so a one-sided passage keeps its level); players put mono
  in both ears. The mix reuses phase 1's per-source path (resampler, timeline,
  mute, meters) and feeds track C's `AudioInput`. The sidecar names the
  channel `"mix"`, so the transcript never labels it Saya/Peserta; an outside
  `.mp4` without a sidecar is also transcribed unlabelled (downmixed).
- **Crash safety:** the video is fragmented MP4 like phase 1's `.m4a`, so a
  `.part` left by a crash still plays.
- **Exclusion from capture is on only while a screen recording runs**, so the
  screenshot tool and the owner's own screen shares still see Vale.
- **Sleep or lock** pauses the screen recording like phase 1; the wake resumes
  it unless the owner paused it.
- **Click ripples are off in phase 3** (`click_ripples: false`).
- **Video fallback folder:** `%USERPROFILE%\Vale\Video` when Windows refuses
  `Videos\Vale`, and the default moves there, as for audio.
- **Esc during the countdown** uses a temporary registration that exists only
  for the 3 seconds.
- **Consequence the owner should know:** because no Vale window is ever in a
  video, a tutorial about Vale itself cannot be recorded with Vale. (From the
  11-07 design.)

## Constraints

- Windows 11 x64, Rust 2024, one `vale.exe`, no new crate. Windows Graphics
  Capture, D3D11, Media Foundation (loaded at run time only).
- Ids from the 11-07 contract: hotkey 10 (11 stays reserved for phase 4),
  timers 70-74, `WM_APP + 90..94`.
- `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` on every Vale window while
  a screen recording runs (or always; builder's choice, measured for side
  effects).
- i18n Id/En with the leak tests; no em or en dash; antislop skills for UI and
  copy; contrast checked in light, warm dark and Terminal.
- Merge `main` into track C's branch first (it predates phases 1 and 2).

## Left to the builder

- Whether exclusion from capture is set always or only while recording.
- The countdown's look in the notch.
- How the thumbnail is made (first frame after 1 s, or a frame at stop).
- The exact mixing (sum with a limiter, or mean), as long as a one-sided
  passage is not 6 dB quieter than recorded.
- Task order.

## Done means

- `cargo test --locked` green on `main`, 0 warnings, exe size measured.
- Live check on the owner's PC: record a monitor, a window (moved while
  recording) and an area, each with mic and PC audio; the video plays in both
  ears with the cursor visible and no Vale window in it; pause and resume;
  discard; the countdown and Esc; the saved notice with its thumbnail opens the
  file; the `.srt` shows as subtitles in a player; a transcript job pauses
  while recording; idle threads after stop.
