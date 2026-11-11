---
tags: [spec, recording, captions, transcript]
date: 2026-11-11
status: approved by the user 2026-11-11
---

# Live captions (recording phase 5)

Brainstormed with the user on 2026-11-11. Parent design:
`docs/superpowers/specs/2026-11-07-recording-design.md` (phase 5, track B),
mockup section 3 of `docs/design/rekaman.html`. Builds on [[transcript]]
(phase 2), [[recording-audio]], [[recording-controls]], [[lyrics-card]],
[[screen-recording]], [[bata-idle-power-budget]], [[hotkeys]].

## Intent

Read what is being said, as it is said, on this PC and offline. The user
named two moments: **meetings** (the most common) and **videos without
subtitles**. Captions are a reading aid for the moment; the accurate,
saved text is still the phase 2 transcript made from the recording after
it ends.

## Decided with the user

- Captions have **their own on/off toggle** and **do not need a recording**.
  They work during a meeting (recorded or not) and while watching a video.
- **Fullscreen:** while captions are on, the caption card **stays visible over
  a fullscreen window** (unlike the lyrics card). Whether it shows is the
  user's choice through the toggle / X. The notch line hides with the notch
  as usual.
- Everything else below the user handed to me ("u atur yang terbaiknya");
  those are marked *(my call)*.

## Goals

1. A floating caption card (same family as the lyrics card) with the last few
   lines, the line being spoken shown grey until it is final.
2. Optional one-line caption in the notch.
3. Offline, using the phase 2 engine and installed model. No cloud, no new
   download kind; the README privacy promise is unchanged.
4. Zero idle cost when captions are off.

## Non-goals

- Saving captions to a file. Captions live in memory for the session and are
  dropped when captions turn off (*my call*: the recording's transcript is the
  saved text). A "Salin" button copies what is in memory.
- Translation, speaker diarization beyond "Saya" / "Peserta", a caption
  history window, burning captions into the screen recording.
- Changing how the phase 2 queue or the recording itself works, except the
  one hold rule below.

## Anti-goals (a failure even if it works)

- Any thread, timer, poll, capture or loaded model while captions are off.
- Captions disturbing a running recording (dropouts, device switches, echo
  cancellation changes) or the recording's files.
- Caption text written to disk, logs or crash output.
- The card appearing in Vale's own screen recordings.

## Behaviour

### Turning captions on and off *(my call)*

- A switch "Caption langsung" on the Rekam page (as in the mockup), a tray
  item, and a new global hotkey row "Caption langsung", default
  `Ctrl+Alt+C`, **hotkey id 14** (appended to `HotkeyId::ALL` after
  `RecordZoom`; 13 is the countdown Esc).
- The X on the card turns captions off (not just hides the card). The card
  **never takes focus** (`WS_EX_NOACTIVATE`): a click on it over a fullscreen
  video must not end the fullscreen check or minimise a game. So there is no
  Esc; the X, the hotkey, the tray and the Rekam switch close it. The wheel
  still reaches it (Windows sends the wheel to the window under the pointer).
- On-state is for this run only: captions are never on at Vale start.
- The card's ⏸ button pauses: capture and worker stop, the card and its lines
  stay, the header says "Dijeda"; ▶ resumes.

### Audio source *(my call)*

- Captions open **their own** `recorder::capture::Capture` (shared-mode
  WASAPI), independent of any recording. Windows allows several shared
  captures of one device, so a running recording is not touched.
- Default source: **PC audio only** (loopback of the default output, or the
  device the recording prefs chose for "Suara PC").
- Setting "Mic juga" (off by default): adds the mic (the recording prefs' mic
  choice). With both, lines are labelled "Saya" (mic) and "Peserta" (PC audio)
  and the phase 2 echo filter (`echo::drop_echo`) applies between finals.
  With one source there are no labels (subtitle style).
- Device switches and loss follow the recording's rules (fall back to the
  default; a lost source shows a line in the card, captions keep the other).

### Recognition

- Engine and model: the phase 2 engine and the **active installed Transkrip
  model** (turbo on a GPU, small without). If the engine or no model is
  installed, the card says so with a button that starts the same `Setup`
  download as Transkrip (only on a click, like phase 2).
- Pipeline per source: capture (48 kHz) → `resample` to 16 kHz mono →
  VAD (Silero when installed, `EnergyVad` otherwise) → `Chunker` with live
  parameters → engine `Session::run` on one low-priority worker thread.
- Live chunk parameters *(my call, builder may tune with measurements)*:
  `min_silence_ms` about 500, `max_chunk_ms` about 10 000; everything else as
  phase 2.
- **Interim (grey) text:** while a chunk is open, the worker re-runs the audio
  of the open chunk so far about every 1.5 s, **only when it has no final
  chunk waiting**. On a slow CPU interim text is skipped and only finals show.
  A final replaces the grey line. Target delay: 2-4 s after a sentence ends.
- **Language** *(my call)*: starts from the Transkrip language setting
  (default Indonesian; Whisper set to Indonesian still writes English words
  that slip in). The card has a small chip that cycles ID / EN / Auto for this
  caption session; the choice is remembered as the caption language
  (`config` key, own, not the Transkrip one).
- **Transcript jobs while captions are on** *(my call)*: a new
  `Hold::Captions` that only **blocks starts**: a queued job waits ("Antre")
  while captions are on (paused included) and starts when captions turn off.
  A job **already running** when captions turn on is **not cancelled**: it
  finishes, and for that overlap two models are loaded (the existing holds
  cancel and restart; this one must not, so it needs a "soft" hold path in
  `Queue`). Pause does not release the hold, so pause / resume never restarts
  a job. The caption worker loads its own `Model` at start and drops it at
  stop.
- **When recognition falls behind** *(my call)*: the delay stays bounded.
  When the final chunks waiting exceed about 15 s of audio, the oldest waiting
  ones are skipped and the card shows a "…" gap line; the first time it
  happens in a session the card adds a hint that the active model is heavy for
  captions on this PC (suggesting Whisper small). The model is never switched
  silently.
- **Needs Transkrip** *(my call)*: captions use the Transkrip feature's engine
  and model, so they need Transkrip on (`TranscriptPrefs::enabled`). With it
  off, the card shows "Caption butuh Transkrip" and a button that turns it on
  (and offers the `Setup` download when the engine or model is missing, only
  on that click). Turning Transkrip off, or deleting the active model, while
  captions run stops them and the card shows the reason. Turning captions on
  before the store has settled (the 30 s after start) loads the store at once
  for captions; it does not start queued jobs early.

### Battery *(design rule, setting my call)*

- On battery (DC) recognition does not run. Pulling the charger works like a
  pause: capture and worker stop, the card stays open with its lines and says
  "Pakai baterai", and captions resume on their own when the charger is back.
  Turning captions on while on battery opens the card in that state. The card
  has a "Tetap jalan" button that runs this session on battery anyway. Setting
  "Tetap jalan pakai baterai" (off) lifts the rule for good.
- Use the existing power path (`PBT_APMPOWERSTATUSCHANGE` in `app.rs`,
  `sources::battery`); no new poll.

### The caption card

- Its own layered, always-on-top, toolwindow (no taskbar button), built like
  the lyrics card: drag, resize from the bottom-right grip, place saved in
  config, per-monitor placement and DPI, `SW_SHOWNOACTIVATE` on open, never
  activated (see above). Reuse `lyrics_card` pieces where they fit; the builder decides
  how much to share.
- Look follows the mockup: header with a status dot, elapsed time of the
  caption session, "Caption langsung · <language>", then "Salin", ⏸ / ▶, ✕.
  Body: the newest lines at the bottom, older ones scroll up and fade at the
  top; the grey interim line last. Labels "Saya" / "Peserta" in two colours
  only when two sources run. Background: Vale's dark window palette (no art
  gradient). Contrast checked like the lyrics card (white text ≥ 7:1, grey
  interim ≥ 4.5:1).
- **Text size** *(my call)*: three steps (S / M / L), changed with
  Ctrl + wheel over the card, remembered. Default M.
- Wheel without Ctrl scrolls back through the session; it returns to the
  newest line after 3 s without input (as the lyrics card).
- **Fullscreen: stays visible** (decided with the user), always on top.
- **Kept out of screen recordings**: the existing in-context exclusion
  (`screenrec::exclude`) covers every top-level window of the process; the
  builder confirms the card is covered and adds a test or check.
- "Salin" copies the session's lines as plain text, one per line, with
  "Saya: " / "Peserta: " when labelled. Through Vale's own-copy path so the
  clipboard notice / history rules treat it as Vale's copy.
- Memory cap: the newest ~500 final lines; older ones are dropped.
- States shown in the body: listening (no speech yet), needs model / engine
  (with the download button), downloading (percent), paused, on battery,
  source lost.

### Notch line *(my call)*

- Setting "Caption di notch" (off by default).
- While recording, the REC notch view adds the latest caption text after the
  time (mockup section 1, "…jadi deadline-nya Jumat ya"), cut from the left
  with an ellipsis.
- Without a recording, captions on and a final or interim line newer than
  8 s: a compact caption view (small CC glyph + the line) takes the place of
  the idle or media compact view; after 8 s without new text it returns. The
  8 s end is a one-shot timer, never a repeat.
- Hidden with the notch in fullscreen, as every notch view.

### Settings *(my call)*

Settings → Fitur, a "Caption langsung" card (next to Transkrip): Mic juga,
Caption di notch, Tetap jalan pakai baterai, and a line that says captions use
the Transkrip model. The hotkey row lives in Tombol pintas like the others.

## Constraints

- Rust 2024, Windows 11 x64, `cargo test --locked` green; unit tests inside
  `src/**`. Tests never show windows or spawn visible consoles
  ([[test-spawns-no-window]]).
- Idle: no thread, timer, capture or model while captions are off (or paused).
  Threads only while on: the capture thread and one caption worker. Timers
  only one-shots. The engine DLLs may stay loaded after a session (as after a
  transcript job).
- Reserved ids for track B: **timers 66-69** (65 is `TIMER_TRANSCRIBE`),
  **`WM_APP + 87..89`** (85 transcript window, 86 player). The card keeps its
  own window-local timer ids like the lyrics card.
- The window procedure never calls `app::with_app`; it posts messages, like
  the lyrics card.
- Caption text never reaches logs (`media_log`-style rules: no user text).
- Measure the exe size before and after ([[exe-size]]).
- A debug demo: `--demo-captions [scene]` with fake lines (labels, interim,
  long text, needs-model, needs-Transkrip, paused, battery, gap), no real
  audio or model.
- i18n: every new string in both languages (`i18n.rs`).

## Left to the builder

- Module layout (suggested: `src/captions/` for the pure pipeline and card
  layout, `src/app_captions.rs` for the App side).
- How much of `lyrics_card` is shared vs copied.
- Exact interim cadence and chunk parameters, after measuring on GPU and CPU.
- The exact visual of the compact notch caption view (within the notch
  conventions, checked with screenshots).

## Done means

- Phase 5 merged into `main`, `cargo test --locked` green on `main`.
- Memory notes (`docs/memory/`) and README updated, `main` pushed, worktree
  cleaned.
- A list of manual checks added to `docs/memory/recording-hand-checks.md`
  (real meeting, real video fullscreen, battery unplug, GPU and CPU latency,
  mic + PC labels and echo, card not in a screen recording).
