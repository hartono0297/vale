---
tags: [spec, recording, transcript, screen-recording, draft]
date: 2026-11-07
status: approved by the user 2026-11-07, split into three tracks
---

# Recording: audio, transcript, screen recording with zoom

Brainstormed with the user on 2026-11-07. Mockups and the comparison tables:
`docs/design/rekaman.html` (open it in a browser; the section numbers below
refer to it). The user said "oke u atur fase pengerjaannya, kerjakan sesuai yg
u plan kan, bagi ke beberapa session". Every default in the decision table
(section 7) stands unless this file says otherwise.

Related notes: [[bata-idle-power-budget]], [[eq-follow-audio]], [[hotkeys]],
[[lyrics-card]], [[exe-size]], [[screenshot-snap]], [[settings-structure]],
[[panel-pages]], [[clipboard-privacy]].

## Settled decisions

- **Sources.** Mic, system audio (WASAPI loopback), or both (default). Both
  are kept as separate channels so the transcript can say "Saya" (mic) and
  "Peserta" (system) without diarization.
- **Engine.** Offline, transcribe.cpp (the engine inside Handy, MIT,
  `handy-computer/transcribe.cpp`, Rust crate `transcribe-cpp`). Pinned
  version. No cloud. The README privacy promise does not change.
- **Models, chosen in Settings, downloaded only on a click** (section 5c):
  Qwen3-ASR 0.6B (default, ~590 MB Q4_K_M), Whisper small (~172 MB), Whisper
  large-v3-turbo (~536 MB), Qwen3-ASR 1.7B (~2.2 GB). Several may be
  installed, one is active. SHA-256 checked. Vulkan (any GPU or iGPU) used
  when present, CPU otherwise. The default model is final only after the
  comparison on a real meeting recording (track B, step 1).
- **Language.** Auto-detect, with a forced Indonesian or English option.
- **Install size.** `vale.exe` stays ~3.9 MB for everyone; the engine and the
  model come only when the user turns Transkrip on.
- **Zoom** means zoom in the video only (the user's screen does not change).
  A real screen magnifier stays in the "later" list (section 11).
- **Summary.** "Salin + prompt ringkasan" only: no AI in Vale.
- **Consent.** A one-time reminder dialog before the first recording; the red
  indicator cannot be turned off.
- **Fullscreen.** The notch and the caption card hide; recording continues; the
  tray icon keeps a red dot.
- **Vale's own windows never appear in a screen recording**
  (`SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`).
- **Idle cost zero.** No audio thread, capture, timer or poll while not
  recording or transcribing ([[bata-idle-power-budget]]).
- **No release** unless the user asks.

## Phases

1. Audio recording with the notch indicator, hotkey, tray, Rekam page.
2. Transcript after the meeting (Settings model picker, transcript window,
   `.md` / `.srt`, copy, make a note, copy with the summary prompt).
3. Screen recording, basic (screen / window / region, the same audio, pause,
   MP4, Vale excluded from capture).
4. Zoom (hotkey `Ctrl+Alt+Z` first, auto-zoom on click next) and the cursor
   highlight / click ripple.
5. Live captions (caption card like the lyrics card, optional one-line notch
   caption). Off by default, stops on battery.
6. Extras from section 11: only what the user picks later. Not started now.

## Tracks (parallel sessions)

Three sessions build at once, each in its own worktree under
`.claude/worktrees/`, each following AGENTS.md "Every code change goes through
Superpowers" (brainstorm is done: this file is the approved design; each
session writes its own plan, implements test-first with reviews, merges
locally into `main`). **Merging into `main` takes turns** (see "Merge rules").

| Track | Session | Phases | Owns |
|---|---|---|---|
| A | the session that wrote this file | 1 | `src/recorder/`, `src/app_record.rs`, the Rekam page, the recording notch views, hotkey id 9, the tray items, Settings "Rekaman" card, `config.recording` |
| B | "Transkrip" session | 2, later 5 | `src/transcribe/`, Settings "Transkrip" card, the transcript window, the transcript notice |
| C | "Rekam layar" session | 3, 4 | `src/screenrec/`, hotkeys 10 and 11, the screen-recording toolbar, the screen notch view |

### Before track A is merged (B and C)

B and C build their **core** only: new modules under their own folder, their
`mod` line in `main.rs`, their `windows` features in `Cargo.toml`, unit tests,
and a debug-only CLI flag to exercise the core by hand. They do **not** touch
`app.rs`, `config.rs`, `i18n.rs`, Settings, the panel or the notch views until
A is in `main`. Then they merge `main` into their branch and integrate.

- **B core:** engine packaging decision (crate compiled in vs a runtime-loaded
  native library downloaded with the model; measure the exe and the CI build
  either way, keep `vale.exe` small), the model catalog (variant, quant, URL,
  size, SHA-256, pinned), the downloader (background, resumable, cancel,
  SHA-256, uses the existing HTTP path of the self-update if it fits), the
  decoder (`.m4a` → 16 kHz mono f32 per channel, Media Foundation Source
  Reader), VAD chunking (each chunk keeps its start time: SRT and
  click-to-seek come from the chunks, Qwen3-ASR has no timestamps), the runner
  on a low-priority worker thread with progress and cancel, the `.md` and
  `.srt` writers, and the comparison of Qwen3-ASR 0.6B, Whisper
  large-v3-turbo and Whisper small on real audio (ask the user for a 5 minute
  meeting recording; report accuracy by eye and the time each took). Debug CLI:
  `--transcribe <file> [--model <variant>]`.
- **C core:** Windows Graphics Capture (monitor, window, region crop), a
  D3D11 compositor (crop + scale for zoom, own cursor drawing, click ripple),
  Media Foundation sink writer to MP4 (H.264, hardware encoder when present,
  software fallback; AAC audio track fed by an interface A will provide), the
  pure zoom-follow maths (target rect, easing, clamps; tested), the sidecar
  cursor/click log (for a later zoom editor), and
  `WDA_EXCLUDEFROMCAPTURE` for Vale's windows. Debug CLI:
  `--screenrec-test <seconds> [--zoom]` writing to the scratch folder.

### After track A is merged

- **B integrates:** Settings → Fitur → Transkrip (section 5c), the transcript
  window (section 4), the progress notice, the Rekam page's per-recording
  "Transkrip" pill and re-transcribe, "Jadikan catatan", "Salin + prompt
  ringkasan", the first-run download dialog. Phase 5 (live captions) comes
  after phase 2 is merged, in the same session.
- **C integrates:** the region picker from the screenshot tool, audio from A's
  capture API, the "Layar" tab of the Rekam page, the screen notch view (REC,
  time, "Layar", zoom level), hotkeys 10/11, the saved notice with a
  thumbnail, then phase 4.

## Contracts between tracks

These are fixed so the tracks can build in parallel. A track that needs to
change one asks the owner session first (SendMessage).

- **Folders.** Audio: `Documents\Vale\Rekaman\` (config
  `recording.folder`, owned by A); when Windows refuses it (Controlled folder
  access) the recording goes to `%USERPROFILE%\Vale\Rekaman` and the default
  moves there (`recorder::prefs::fallback_folder`, see the memory note
  `controlled-folder-access`; C does the same for video). Video: `Videos\Vale\` (config
  `recording.video_folder`, added by C). Names:
  `YYYY-MM-DD HHmm <title>.<ext>`, title from the app using the mic or empty
  (then `YYYY-MM-DD HHmm.<ext>`); a clash adds ` (2)`.
- **Audio file (A writes, B reads).** `.m4a`: AAC-LC in MP4, 48 kHz,
  96 kbps (stereo or mono; the Windows AAC encoder has no lower documented rate). Both sources: channel 0 = mic, channel 1 =
  system. One source: mono. Paused audio is dropped, not kept as silence.
- **Sidecar (A writes, B and C add to it).** `<same name>.vale.json`, UTF-8:
  `{ "v": 1, "kind": "audio" | "screen", "title": string, "started": RFC 3339
  local time, "duration_ms": u64, "channels": ["mic", "system"] (index =
  channel), "app": string | null }`. B owns a `"transcript"` object, C owns a
  `"cursor_log"` file name. Unknown keys are kept on rewrite.
- **Transcript files (B).** `<same name>.md` and `<same name>.srt` next to the
  audio or video.
- **Audio capture API (A provides, C consumes).** `recorder::capture`: start
  with a source set (mic, system, both) and a sink; the sink gets 48 kHz f32
  frames per channel with a QPC timestamp, on the capture thread; stop
  returns cleanly; device loss is reported, not hidden. Exact Rust signatures
  are in A's plan and announced to C when merged.
- **Model and engine store (B).** Under Vale's data dir (`data_dir.rs`), in a
  `transcribe\` folder; B picks the layout.
- **Reserved ids** (to avoid collisions while three branches add them):

  | Kind | A | B | C |
  |---|---|---|---|
  | `RegisterHotKey` id | 9 (record audio, default `Ctrl+Alt+R`) | none | 10 (record screen, default `Ctrl+Alt+Shift+R`), 11 (zoom while recording, default `Ctrl+Alt+Z`, registered only while a screen recording runs) |
  | Timer ids | 60-64 | 65-69 | 70-74 |
  | `WM_APP + n` | 80-84 | 85-89 | 90-94 |

  `HotkeyId::ALL` gets A's row first; C appends after A is merged.
- **Panel page.** A adds one page, "Rekam" (`Page::ALL` grows by one), with
  room for an "Audio" / "Layar" switch that C fills.

## Merge rules (all tracks)

- Merge into `main` one at a time. Before merging, check that no other
  linked session is merging (`list_sessions` with `linked: true`, or ask the
  owner session with SendMessage). Merge `main` into the branch first, run
  `cargo test --locked`, then fast-forward or merge into `main`, run
  `cargo test --locked` on `main` again.
- After a merge, AGENTS.md step 5 (memory notes, README, commit "docs: update
  memory and README", push `main`) and step 6 (clean up the worktree and
  branch). Each track writes its own memory notes; README gets one line per
  shipped phase.
- Measure the exe size per merged track ([[exe-size]]).
- A session never starts another session.
