---
tags: [spec, recording, transcript]
date: 2026-11-08
status: written 2026-11-08 with the owner's answers; waiting for the owner's OK on the whole document
---

# Transcript after the meeting (recording phase 2)

Builds on the approved recording design
(`docs/superpowers/specs/2026-11-07-recording-design.md`, "the 11-07 design"),
its mockups (`docs/design/rekaman.html`, sections 4 and 5c) and track B's core
(branch `worktree-agent-a1c20b057f5c3a485`, 10 commits on top of `5ecff05`:
`src/transcribe/`, the `--transcribe` debug flag). This file says only what
changes or is added; everything else in the 11-07 design stands.

Related notes: [[recording-audio]], [[recording-controls]],
[[controlled-folder-access]], [[media-foundation-dynamic-load]],
[[bata-idle-power-budget]], [[exe-size]], [[open-mode]], [[panel-pages]],
[[settings-structure]], [[lyrics-card]], [[sticky-note-list]].

## Intent

The owner records meetings with Vale (phase 1, on `main`). Phase 2 turns each
recording into text on this PC, with no audio leaving it: who said what and
when, readable in Vale, saved as `.md` and `.srt`, easy to paste into an AI
chat for a summary or into a Vale note. The owner's words: "abis meeting
langsung mulai bikin transkripnya", and there should be a setting for whether
that happens on its own. For everything else the owner said "u atur aja yg
terbaiknya gimana", so the calls marked **(builder's call, made here)** were
made by the design author, not the owner.

## Goals

1. When a recording stops and Transkrip is on with auto on, the transcript
   starts by itself, in the background, and the notch says when it is ready.
2. A switch "Bikin transkrip otomatis setelah rekam" (default on). Off: the
   saved notice offers a **Transkrip** button, and the Rekam page always can.
3. A transcript window to read it, jump the audio to a line, copy it, save it,
   make it a note, or copy it with a summary prompt.
4. Settings → Fitur → Transkrip: turn the feature on, pick and download a
   model, language, the auto switch, space used.
5. Re-transcribe any recording, with another model if wanted.

## Non-goals

- Live captions (phase 5, later, same track).
- Any AI summary inside Vale, any cloud engine, any upload.
- Telling speakers apart beyond the two channels ("Saya" = mic,
  "Peserta" = PC audio). A mic-only or PC-only recording has no labels.
- Editing the transcript text inside Vale (the `.md` opens in any editor).
- Translation. Retention or auto-delete of old audio (11-07 "later" list).
- **Screen recordings (owner, 2026-11-08).** Phase 2 is audio only: no
  `.mp4` input, no transcript state on a "Layar" tab. The queue has a pause
  hook (pause / resume by a reason, so two reasons can hold it at once) that
  the audio recorder uses now and track C's screen recorder will call when it
  is integrated; transcripts of `.mp4` come with that integration.

## Anti-goals (a failure even if it works)

- Anything downloaded without a click. Any audio or text sent anywhere.
- The meeting PC slowed down: transcribing must not compete with a running
  recording or make the call stutter.
- A thread, timer or poll at idle (no recording, no transcription, no
  download): [[bata-idle-power-budget]].
- `vale.exe` growing past a few tens of KB for this phase ([[exe-size]]).
- A transcript made of an empty or silent recording, or a "Saya" line that
  just repeats what "Peserta" said.

## What changed since the 11-07 design (decided)

- **Models: two, not four.** Measured on the owner's 5 minute recording
  (312.6 s, both channels, before echo cancellation existed):

  | Model | Size | GPU (Vulkan) | CPU | Quality by eye |
  |---|---|---|---|---|
  | Whisper large-v3-turbo | 536 MB | 34.7 s (9x) | not run (slow) | best: "Jadi setelah dicek, sebetulnya ini pasar saham sedang reality check" |
  | Whisper small | 172 MB | 26.7 s (12x) | 99.1 s (3.2x) | good, drops some words |
  | Qwen3-ASR 0.6B | 590 MB | 31.0 s (10x) | 192 s (1.6x) | worst: "realiti ceklah", "hampat", "staloh" |

  Settings lists **Whisper large-v3-turbo** and **Whisper small** only. Both
  Qwen3 entries leave the shipped catalog (the code that handles a model
  without timestamps may stay if it is cheap to keep; builder's choice).
- **Default model by hardware (builder's call, made here).** With a hardware
  GPU (any adapter Vulkan can use, an iGPU counts): Whisper turbo. Without
  one: Whisper small. The first-run offer names that model; the owner can pick
  the other before downloading. How "has a GPU" is told before the engine is
  downloaded (DXGI hardware adapter, or asking the engine once it is there) is
  left to the builder; a wrong guess only changes which model is offered.
- **Language: Indonesian by default**, not auto. Auto-detect picked the wrong
  language on short clips. Choices: Indonesia, Inggris, Otomatis.
- **Auto switch** (goal 2), new.
- **Echo duplicates.** Echo cancellation is now on by default (phase 1), so
  the mic should rarely hear the speakers. As a backup, a "Saya" segment is
  dropped when it overlaps a "Peserta" segment in time and its words mostly
  match (in the test above almost every line was doubled). The match rule and
  threshold are the builder's; it must keep a real reply such as "Boleh, jadi
  deadline-nya Jumat ya" that overlaps someone else talking.

## Behaviour

### Settings → Fitur → Transkrip

Follows mockup 5c with the two-model list. Rows:

1. **Transkrip** switch, off by default. Off: nothing downloaded, nothing
   runs; recording works as before.
2. Turning it on the first time when nothing is installed shows, in the card
   itself, "Unduh Whisper large-v3-turbo (555 MB, termasuk mesin 19 MB)?"
   with **Unduh** and **Pilih model lain** (sizes are the real sums from
   `catalog.rs`: model + engine + VAD). **(builder's call, made here:** inline
   in the card, not a separate dialog.)
3. The model list: name, one-line note, size, and per row **Unduh** /
   progress with **Batal** / **Terpasang** with **Hapus**. A radio picks the
   active model; picking one not installed turns its button into Unduh. The
   model in use by a running transcription cannot be deleted.
4. **Bahasa**: Indonesia (default), Inggris, Otomatis.
5. **Bikin transkrip otomatis setelah rekam** (default on). Greyed out while
   Transkrip is off or no model is installed.
6. A line with where it runs ("Pakai GPU" or "Pakai CPU") and the space used,
   with **Buka folder**.

Downloads run in the background, resume after Vale restarts, are SHA-256
checked before use, and can be cancelled (track B's `download.rs`). When one
finishes or fails while Settings is closed, a notice says so. A failed
download says why in plain words (no network, disk full, checksum).

### After a recording stops

- **Auto on, model installed:** the job is queued at once. The notch shows a
  short notice "Bikin transkrip · <judul>" when it starts and
  "Transkrip siap · <judul> · klik untuk buka" when done; the click opens the
  transcript window. Progress (percent, time left) lives on the Rekam page row
  and in the transcript window, not as a permanent notch view **(builder's
  call, made here:** no new notch view, no timer for it).
- **Auto off:** the existing "Rekaman disimpan" notice gets a **Transkrip**
  button that queues the job.
- **Transkrip off or no model:** once only (remembered in config), the saved
  notice adds "Bikin transkrip? Nyalakan di Pengaturan" with a button that
  opens Settings at the Transkrip card. Never again after that.
- Recordings that phase 1 does not keep (under 0.5 s, silent) never get a
  transcript.

### The queue

- One job at a time, first in first out, on a below-normal worker (track B's
  `runner.rs`). Thread count as the runner picks it.
- **A recording running pauses the queue (builder's call, made here).** If a
  new recording starts while a transcript is being made, that job stops and
  goes back to the head of the queue; it starts again when the recording
  stops. Restarting a job from the beginning is acceptable; resuming mid-file
  is the builder's choice.
- **Vale closing or the PC shutting down** loses nothing: unfinished jobs are
  remembered (where is the builder's choice: config, a small queue file under
  the transcribe store, or the sidecar) and run again on the next start,
  after Vale has settled (not during startup).
- Battery: runs anyway **(builder's call, made here)**.
- Cancel: from the Rekam page row or the transcript window. A cancelled job
  leaves no half-written `.md` / `.srt` (track B writes atomically).
- Re-transcribe replaces the old `.md` / `.srt` and the sidecar's
  `"transcript"` object. It can pick another installed model.
- **The model is still downloading at Stop (builder's call, made here):** the
  job is queued and starts when the download finishes; a failed or cancelled
  download leaves it queued with the Rekam row saying "Menunggu model".
- **Transkrip turned off with jobs queued (builder's call, made here):** the
  running job is cancelled and the queue is cleared; nothing is deleted.
- An interrupted download resumes by itself on the next start (5c: "nyambung
  lagi kalau Vale ditutup").

### Rekam page

Each row of the recent list gets a transcript state on the right:

- none: a **Transkrip** button (queues the job; disabled while Transkrip is
  off, with the reason on hover or as a small note);
- queued: "Antre";
- running: percent;
- done: **Lihat** (opens the window). A right-click menu (or the window's
  own button) offers **Transkrip ulang**.

Clicking the row itself keeps its phase 1 behaviour.

**Older recordings (owner, 2026-11-08):** the page keeps its 5 newest rows.
Next to "Buka folder" a **Transkrip file lain…** button opens the Windows
file picker (`.m4a`, `.wav`), starting in the recording folder. The picked
file opens in the transcript window: its transcript if it has one, or a
queued job if not. This also covers recordings left in a folder the owner
switched away from and everything recorded before phase 2.

### Transcript window

Follows mockup section 4. A window of its own, themed like Vale's other
windows (light and dark), remembered size and place like the lyrics card.

- Header: title, app, date and time, length, model, language.
- Buttons: **Salin** (plain text with times and labels), **Buka .md**,
  **Buka folder**, **Jadikan catatan**, **Salin + prompt ringkasan**,
  **Transkrip ulang**.
- A player bar: play / pause, position, length, a seek bar. **Clicking a line
  jumps the audio to that line's start** and plays. The playing line is
  marked and kept in view while playing. Audio is played with Media
  Foundation loaded at run time ([[media-foundation-dynamic-load]]); the
  exact API is the builder's.
- Lines: time, label (Saya / Peserta, or none for a one-source recording),
  text. Scrolls with the wheel and the keyboard.
- While the job is still running, the window shows the lines so far and the
  progress; it fills in as chunks finish (the runner already reports each
  segment).
- Empty result (speech too unclear): "Tidak ada ucapan yang terbaca" instead
  of an empty list. A failed job: the reason and **Coba lagi**.
- One window per recording; opening the same one again brings it forward.
  Closing it stops playback.

**Salin + prompt ringkasan** copies this prompt followed by the transcript:

> Ringkas transkrip meeting di bawah ini. Tulis: keputusan yang diambil,
> action item beserta siapa yang mengerjakan dan tenggatnya kalau disebut,
> dan pertanyaan yang belum terjawab. "Saya" adalah orang yang merekam,
> "Peserta" adalah orang lain di meeting.

**Jadikan catatan** makes a Notes card: title, date and time, then the
transcript. Over 20 lines: the first 20 lines and the `.md` path.

### Files

As the 11-07 contract: `<name>.md` and `<name>.srt` next to the audio, and the
sidecar's `"transcript"` object (model, engine, language, segments, when).
Labels follow Vale's language (Saya / Peserta, Me / Others).

## Constraints

- Windows 11 x64, Rust 2024, single exe. Engine and models are downloaded,
  never bundled; `vale.exe` stays about 4 MB.
- Ids from the 11-07 contract: timers 65-69, `WM_APP + 85..89`. No hotkey.
- Nothing at idle; worker thread only while a job runs, download thread only
  while downloading; the player's thread only while the window plays.
- Recording folder may be the CFA fallback (`%USERPROFILE%\Vale\Rekaman`);
  the transcript is written next to the audio wherever that is.
- The engine package needs the VC++ runtime; track B's `missing_runtime`
  check must turn into a clear message, not a crash.
- Merge track B's branch with `main` first (it predates phase 1 controls).

## Left to the builder

- How a GPU is detected for the default model offer.
- Where the queue is remembered across restarts.
- The duplicate-match rule and threshold (with tests from the run2254 data:
  `C:\Users\Hartono\AppData\Local\Temp\claude\5eba95a85cda8679\22da1dcd-3975-4cd2-8724-c06cbbf23248\scratchpad\asr\run2254\`).
- The player API, the transcript window's layout details and its hit
  regions, as long as mockup section 4 is recognisable.
- Whether the Qwen3 code path stays.
- Splitting the work into tasks and the order (the player can come last).
- Where the window reads the timed segments from (the sidecar keeps only a
  count): parse the `.srt`, or keep a segments file beside it.
- The player plays both channels mixed into both ears **(builder's call,
  made here)**, so a mic-left / PC-right file does not sound one-sided.

## Done means

- `cargo test --locked` green on `main`, no warnings.
- Live check on the owner's PC: a short real recording is transcribed by
  itself after Stop with the notices above; the window opens from the notice
  and the Rekam page; clicking a line plays from there; Salin, Salin + prompt,
  Jadikan catatan, Buka .md work; auto off shows the Transkrip button; a
  recording started mid-transcript pauses the job and it finishes afterwards;
  quitting Vale mid-job and starting it again finishes the job.
- Settings card: download with progress, cancel, resume after restart,
  delete, switch model, language.
- Light and dark theme checked on the window and the card.
- Exe size measured and noted in [[exe-size]].
