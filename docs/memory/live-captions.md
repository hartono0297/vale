---
tags: [decision, recording, captions, transcript, power, window, hotkeys, timers]
date: 2026-11-11
---

# Live captions (recording phase 5)

Offline captions of what the PC plays (and, optionally, the mic) in a
floating caption card and an optional one-line notch caption. Spec:
`docs/superpowers/specs/2026-11-11-live-captions-phase5-design.md`, plan:
`docs/superpowers/plans/2026-11-11-live-captions-phase5.md`, mockup:
`docs/design/rekaman.html` section 3. Merged on `main` 2026-11-11. Builds on
[[transcript]] (engine and model), [[recording-audio]] (capture),
[[lyrics-card]] (card window), [[screen-recording]] (exclusion). Hand checks:
[[recording-hand-checks]]. Size: [[exe-size]].

## Decided with the owner

- Captions have their own toggle and do not need a recording (meetings and
  videos without subtitles).
- While on, the card stays visible over a fullscreen window (unlike the
  lyrics card); the owner closes it with X, the hotkey, the tray or the Rekam
  switch.
- Everything else was handed to the builder ("u atur yang terbaiknya").

## Where it lives

- `src/captions/`: `live.rs` (pure core: `Timeline` at 16 kHz, VAD +
  `Chunker` with live params, interim scheduling, backlog cap, echo hold,
  `skip_to`), `lines.rs` (what the card shows, 500-line cap, copy text),
  `control.rs` (`Ctl`, `CardStatus`, `should_run`, `holds_queue`),
  `prefs.rs` (`CaptionPrefs`, `TextSize`), `worker.rs` (the thread, own
  capture, engine, load gate), `debug.rs` (`--captions-test`, debug builds).
- `src/caption_card/` (`layout.rs`, `window.rs`, `mod.rs`, `demo.rs`) and
  `src/ui/card_surface.rs` (`Surface`, `Tween`, `rounded`, now shared with the
  lyrics card).
- `src/app_captions.rs` (`#[path]` child of `app`): state, toggle, sync,
  battery, card messages, Salin, notch line.

## Decisions (builder's calls)

- **Own capture.** A caption session opens its own shared-mode
  `recorder::capture::Capture` (echo cancellation off), never the
  recording's; a running recording is untouched. PC audio by default; "Mic
  juga" adds the mic and the "Saya" / "Peserta" labels.
- **Engine and model** are the Transkrip ones (`model_for(gpu)`); captions
  need Transkrip on (the card offers "Nyalakan Transkrip" and the download).
  Language: `config.captions.language` else the Transkrip language; the
  card's chip cycles ID / EN / Auto (an atomic read per chunk).
- **Live chunking:** `min_silence_ms` 500, `max_chunk_ms` 10 000,
  `split_window_ms` 3 000; audio newer than 300 ms is not taken; interim
  (grey) every 1.5 s once the open chunk is 1 s long and no final waits.
- **Bounded delay:** finals waiting over 15 s of audio are dropped from the
  front with a "…" gap line; the first time in a session the card adds the
  "model is heavy" hint. A clock jump (sleep, a stalled engine) calls
  `Live::skip_to`; awake time (`QueryUnbiasedInterruptTime`) of the last
  engine run tells a slow model (hint) from a sleep (no hint).
- **Echo:** "Saya" finals wait until "Peserta" is settled at that time, then
  `echo::drop_echo` drops mic lines that only repeat the speakers.
- **Transcript queue:** `Hold::Captions` is a soft hold: queued jobs wait
  while captions are on (paused too); a running job is never cancelled.
- **Battery:** on battery the worker stops, the card keeps its lines with a
  "Pakai baterai" strip and "Tetap jalan" (this session); back on AC it
  resumes; one notice on the running → battery edge. Desktops (no battery
  reading) are never "on battery". Setting "Tetap jalan pakai baterai".
- **Worker lifecycle:** stop from the UI is `Running::cancel` (non-blocking,
  detached thread); events carry a worker id and an old worker's are ignored.
  A process-wide load gate keeps one model load in flight; `cancel` is checked
  after the gate and before the GPU → CPU retry. All captioned sources lost
  for good ends the session with "capture". Line times are rebased across
  worker restarts so lines stay in order.
- **Card:** layered, topmost, `WS_EX_NOACTIVATE` + `MA_NOACTIVATE` (never
  takes focus, so no Esc), no fullscreen hide, bottom-centre of the notch
  monitor 96 dip above the work area's bottom by default, place saved.
  Ctrl+wheel steps the text size (S/M/L 15/18/22 px), plain wheel scrolls
  back with a 3 s return. Status strip under the lines when lines exist,
  centred prompt otherwise. Pause disabled on battery. Dark only.
  `CardContent`'s `Debug` prints counts, never text.
- **Notch line** ("Caption di notch", off by default): the REC view grows by
  `REC_CAPTION_EXTRA` (120 dip) with the latest line cut from the left;
  without a recording `View::CaptionCompact` (REC shape width, CC glyph)
  takes the place of the media or idle compact view, below pomodoro; a
  one-shot `TIMER_CAPTION_LINE` (66) ends it after 8 s without new text; no
  refresh while fullscreen hides the notch.
- **Entry points:** hotkey row 14 "Caption langsung" `Ctrl+Alt+C` (always
  registered), tray item after Rekam layar, a switch on the Rekam page
  (costs one recent-recording row: 5 → 4), Settings → Fitur "Caption
  langsung" card (Mic juga, Caption di notch, Tetap jalan pakai baterai).
- **Ids:** timer 66, `WM_APP + 87` (card → island). Worker events ride the
  existing `Event` channel (`Event::Captions(id, ..)`).

## Gotchas

- `Live` must see `take_out()` after every `next_work` (held "Saya" lines are
  released there), and `now_ms` / `now_frame` share the session-origin clock.
- `Timeline::put` drops writes more than 10 s ahead of the read point; the
  worker advances to `min(start, now)` before `put`.
- Thread count after captions off settles back within about 2 minutes: the
  surplus is Windows thread-pool workers plus the one AMD driver thread
  [[transcript]] already notes; Vale's own threads went 6 → 8 → 6. A plain
  phase 2 recording shows the same pattern.

## Measured

GPU (turbo, Vulkan): grey line about every 1.5 s, first change about 2.7 s
after speech starts. Turbo on the CPU: about 6 s per interim (the hint case).
Exe +88,064 bytes (+1.97 %), see [[exe-size]].

## Left for later

Off-state switch track contrast 1.32:1 (shared `paint_switch`); re-layout of
all finals on every change (time a 500-line session); a redacting `Debug` for
`worker::Event` / `model::Event`; "capture" failure always reads "PC audio
lost"; the battery notice source reads "Rekaman"; no unread-toast dot on the
caption view.
