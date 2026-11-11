# Live Captions (recording phase 5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Offline live captions of PC audio (and optionally the mic) in a floating, never-activated caption card and an optional one-line notch caption, using the phase 2 engine and model.

**Architecture:** A caption session owns its own `recorder::capture::Capture`; its sink resamples each packet to 16 kHz and sends it over a channel to one low-priority caption worker. The worker places packets on a `recorder::timeline::Timeline` (zero-filling loopback silence), runs VAD + `Chunker` with live parameters through a pure `captions::live::Live` core, and recognizes final and interim chunks with the engine. Events reach `App` through the existing `Event` channel; `App` (`app_captions.rs`) drives a pure control model, the caption card window (`caption_card/`, built like `lyrics_card/`) and the notch line.

**Tech Stack:** Rust 2024, `windows` crate (Win32, Direct2D, DirectWrite), the phase 2 `transcribe-cpp` engine (`src/transcribe/engine.rs`), WASAPI capture (`src/recorder/capture.rs`).

**Spec:** `docs/superpowers/specs/2026-11-11-live-captions-phase5-design.md` (parent: `docs/superpowers/specs/2026-11-07-recording-design.md`). Read `docs/memory/transcript.md`, `docs/memory/lyrics-card.md`, `docs/memory/bata-idle-power-budget.md`, `docs/memory/screen-recording.md` before starting.

## Global Constraints

- Windows 11 x64, Rust 2024; `cargo test --locked` green after every task; unit tests live in `#[cfg(test)]` modules inside `src/**`.
- Tests never show a window or spawn a visible console (`ON_SCREEN = !cfg!(test)` pattern, CREATE_NO_WINDOW; memory note `test-spawns-no-window`).
- Idle cost zero: no thread, timer, capture or loaded model while captions are off **or paused**. While on: the capture thread, one caption worker, and one-shot timers only (the worker's own `recv_timeout` wait of 200 ms is allowed while on).
- Reserved ids: timers **66-69** (65 is `TIMER_TRANSCRIBE`); `WM_APP + 87..89` (85 transcript window, 86 player). New hotkey row id **14** (13 is the countdown Esc).
- Card window procedure never calls `app::with_app`; it posts `WM_APP_CAPTION_CARD` (`WM_APP + 87`). The `RefCell` borrow is never held across `ShowWindow`, `SetCapture`, `ReleaseCapture`, `DestroyWindow` (same rules as `lyrics_card/window.rs`).
- Caption text never reaches logs, `eprintln!`, files or crash output. Only counts and error kinds may be logged.
- No cloud, no new download kind. Downloads only on a click, through `App::transcribe_download`.
- Every new user string in both languages through `i18n.rs` `keys!` (`Name => "id", "en";`). Copy follows the existing casual Indonesian tone ("Caption langsung", "Saya", "Peserta", "Dijeda", "Pakai baterai", "Tetap jalan", "Salin").
- UI work loads the antislop skills (`antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-human`) before writing paint code and checks colour pairs with its `check_contrast` tool.
- Default hotkey for the new row: `Ctrl+Alt+C`. Default prefs: `mic: false`, `notch: false`, `on_battery: false`, `language: None` (follow Transkrip), `size: Medium`, `place: None`.

## Review Focus

1. **PC audio goes silent with no packets** (WASAPI loopback sends nothing in silence): the sentence must still end and become final within ~1 s. Pinned by `live::tests::a_sentence_ends_when_the_pc_goes_silent_without_packets` (Task 2).
2. **Output device changes rate mid-session** (headset plugged in, 48 kHz → 44.1 kHz): no pitch-shifted garbage, no panic. Pinned by `worker::tests::a_rate_change_rebuilds_the_resampler` (Task 5).
3. **Clock jump** (sleep/resume, a stalled worker): no multi-minute chunk and no huge allocation; the delay stays bounded. Pinned by `live::tests::a_clock_jump_is_cut_to_the_backlog_cap` (Task 2).
4. **Captions turned off mid-chunk**: the toggle returns at once (engine cancel), and no event of the old worker changes the new state. Pinned by `app_captions::tests::events_of_an_old_worker_are_ignored` (Task 8).
5. **A recording starts or stops while captions run**: the recording is untouched, a transcript queued meanwhile waits for captions off, and starts when both are off. Pinned by `queue::tests::captions_and_recording_holds_release_in_any_order` (Task 1).

---

### Task 1: Soft hold `Hold::Captions` in the transcript queue

**Files:**
- Modify: `src/transcribe/queue.rs` (enum `Hold` line 16, `set_hold` line 88)
- Modify: `src/app_transcribe.rs` (`transcribe_sync_hold` line 478)

**Interfaces:**
- Produces: `Hold::Captions`; `Hold::preempts(self) -> bool` (false only for `Captions`); `App::transcribe_sync_hold` reads a new `captions_on: bool` argument source: `self.cap.holds_queue()` (Task 8 provides it; until then pass `false` through a private helper `fn captions_hold(&self) -> bool { false }` that Task 8 replaces).

- [ ] **Step 1: Write the failing tests** in `queue.rs` tests:

```rust
#[test]
fn the_captions_hold_never_cancels_a_running_job() {
    let mut q = Queue::new(&[]);
    q.push(job("a.m4a"));
    assert!(q.start_next(|_| true).is_some());
    assert_eq!(q.set_hold(Hold::Captions, true), None, "running job keeps running");
    assert_eq!(q.state(Path::new("a.m4a")), Some(JobState::Running));
    q.push(job("b.m4a"));
    q.cancel(Path::new("a.m4a"));
    assert_eq!(q.start_next(|_| true), None, "held: the next one waits");
}

#[test]
fn captions_and_recording_holds_release_in_any_order() {
    let mut q = Queue::new(&[]);
    q.push(job("a.m4a"));
    q.set_hold(Hold::Captions, true);
    q.set_hold(Hold::Recording, true);
    q.set_hold(Hold::Recording, false);
    assert_eq!(q.start_next(|_| true), None);
    q.set_hold(Hold::Captions, false);
    assert!(q.start_next(|_| true).is_some());
}
```
(Use the file's existing job-builder helper; add one if missing.)

- [ ] **Step 2:** Run `cargo test --locked queue::` → both FAIL (no `Hold::Captions`).
- [ ] **Step 3:** Add `Hold::Captions` (doc: "Live captions are on: starts wait, a running job finishes") and `Hold::preempts`; `set_hold` returns `None` without touching `running` when `!hold.preempts()`. In `transcribe_sync_hold` add `self.tr.queue.set_hold(Hold::Captions, self.captions_hold())` to the set-before-look array and include it in the `on` check that decides `transcribe_try_next`.
- [ ] **Step 4:** `cargo test --locked` → PASS.
- [ ] **Step 5:** Commit `feat(captions): soft queue hold while captions run`.

### Task 2: The live chunking core `captions::live`

**Files:**
- Create: `src/captions/mod.rs` (declares `live`, later `lines`, `control`, `worker`), `src/captions/live.rs`
- Modify: `src/main.rs` (add `mod captions;`), `src/transcribe/chunk.rs` (add `Chunker::open_chunk`)

**Interfaces:**
- Consumes: `transcribe::chunk::{Chunker, ChunkParams, Chunk, Framer, FRAME, FRAME_MS}`, `transcribe::runner::FrameScorer`, `transcribe::output::{Segment, Speaker}`, `transcribe::echo::{settled, drop_echo}`, `recorder::timeline::Timeline`, `transcribe::decode::RATE` (16 000).
- Produces:
  - `Chunker::open_chunk(&self) -> Option<Chunk>` (a copy of the open chunk so far, same `start_ms`).
  - `pub fn live_params() -> ChunkParams` = default with `min_silence_ms: 500`, `max_chunk_ms: 10_000`, `split_window_ms: 3_000`.
  - Constants: `LAG_MS: u64 = 300` (audio newer than this is not taken yet), `INTERIM_EVERY_MS: u64 = 1_500`, `INTERIM_MIN_MS: u64 = 1_000`, `MAX_BACKLOG_MS: u64 = 15_000`, `MAX_TAKE_MS: u64 = 1_000` (per `advance` call).
  - `pub struct Live` with `new(speakers: Vec<Option<Speaker>>) -> Live` (one channel per entry), `put(&mut self, channel: usize, start_frame: i64, samples: &[f32])` (16 kHz frames from the session origin), `advance(&mut self, now_frame: i64, scorers: &mut [Box<dyn FrameScorer + '_>]) -> Result<(), EngineError>`, `next_work(&mut self, now_ms: u64) -> Option<Work>`, `final_done(&mut self, chunk: &Chunk, segments: Vec<Segment>) -> Vec<Out>`, `interim_done(&mut self, chunk: &Chunk, text: String) -> Vec<Out>`, `take_out(&mut self) -> Vec<Out>` (gaps and the slow hint queued by `next_work`).
  - `pub enum Work { Final(Chunk), Interim(Chunk) }`
  - `pub enum Out { Final(Segment), Interim { channel: usize, text: String }, ClearInterim { channel: usize }, Gap, Slow }`

Rules the code must follow:
- `advance` takes at most `MAX_TAKE_MS` per call from the timeline up to `now_frame - LAG_MS`, de-interleaves, frames, scores and chunks every channel; closed chunks go to a FIFO of finals. Called repeatedly by the worker until caught up.
- `next_work`: when the finals FIFO holds more than `MAX_BACKLOG_MS` of audio, drop from its front until it holds at most that, queue one `Out::Gap` per drop run and `Out::Slow` the first time in the session. Then a final if any; else an interim for the channel whose open chunk is ≥ `INTERIM_MIN_MS` long and whose last interim (or chunk open) is ≥ `INTERIM_EVERY_MS` ago, oldest first; else `None`.
- `final_done`: segments of the "Saya" channel are held until `echo::settled(seg, others_settled_ms)` with `others_settled_ms` from the "Peserta" chunker (or `u64::MAX` without one); released ones and any "Peserta" segments pass through `echo::drop_echo` against the "Peserta" finals of the last 30 s; outputs `ClearInterim` for that channel then the `Final`s in start order.
- `interim_done` ignores a result whose chunk is no longer the open one (`start_ms` differs).

- [ ] **Step 1: Write the failing tests** in `live.rs`, with a test scorer `Loud` (prob = 1.0 when the frame RMS > 0.01, else 0.0) and a helper `tone(ms)`/`silence(ms)` making 16 kHz samples:
  - `a_pause_closes_a_final` — put 2 s tone then 1 s silence, advance to 3.3 s → `next_work` is `Final` with `start_ms` ≤ 200 and length 2.0-2.8 s.
  - `a_sentence_ends_when_the_pc_goes_silent_without_packets` — put 2 s tone only (no silence packets), advance with `now_frame` at 3.3 s → a `Final` exists (timeline zeros close it).
  - `interim_comes_every_1500_ms_only_without_finals` — 4 s of continuous tone: `next_work(now_ms)` gives `Interim` at ≥1.0 s, `None` again before +1.5 s, `Interim` after; with a final queued, `Final` comes first.
  - `a_long_stretch_is_split_at_10_s` — 25 s tone → finals each ≤ 10 s.
  - `a_clock_jump_is_cut_to_the_backlog_cap` — queue 40 s of finals (tone/silence pattern), call `next_work` → FIFO ≤ 15 s of audio, `take_out()` == `[Gap, Slow]`; a second overflow gives `[Gap]` only.
  - `an_echo_of_peserta_on_the_mic_is_dropped` — channels `[Some(Me), Some(Others)]`, `final_done` with an Others segment "jadi deadline-nya jumat ya" 1.0-3.0 s and a Me segment with the same words 1.1-3.1 s → only the Others `Final` comes out.
  - `a_real_reply_on_the_mic_stays` — same times, different words → both come out, in start order.
  - `a_stale_interim_is_ignored` — `interim_done` for a chunk whose `start_ms` is not the open chunk's → `vec![]`.
  - In `chunk.rs`: `open_chunk_copies_the_open_speech` — after 10 loud frames `open_chunk()` has 10+pad frames and the same `start_ms` the closed chunk later gets.
- [ ] **Step 2:** `cargo test --locked captions::live chunk::` → FAIL (module missing).
- [ ] **Step 3:** Implement `Chunker::open_chunk` and `captions::live` per the rules above (the `Timeline` is `Timeline::new(16_000, speakers.len())`; de-interleave `take_max` output by channel count).
- [ ] **Step 4:** `cargo test --locked` → PASS.
- [ ] **Step 5:** Commit `feat(captions): live chunking core with interim, backlog cap and echo hold`.

### Task 3: Caption lines and control model

**Files:**
- Create: `src/captions/lines.rs`, `src/captions/control.rs`

**Interfaces:**
- Consumes: `captions::live::Out`, `transcribe::output::{Segment, Speaker}`, `transcribe::prefs::TrLang`.
- Produces:
  - `lines.rs`: `pub const MAX_LINES: usize = 500;` `pub struct Line { pub speaker: Option<Speaker>, pub text: String, pub start_ms: u64, pub gap: bool }`; `pub struct Lines { .. }` with `apply(&mut self, out: Out)`, `finals(&self) -> &VecDeque<Line>`, `interim(&self) -> Vec<(Option<Speaker>, &str)>` (channel order), `labelled(&self) -> bool` (set by `Lines::new(speakers: Vec<Option<Speaker>>)` when 2 channels), `latest(&self) -> Option<&str>` (newest interim else newest final text, for the notch), `copy_text(&self, me: &str, others: &str) -> String`, `generation(&self) -> u64` (bumps on every change, for change detection).
  - `control.rs`: `pub struct Ctl { pub on: bool, pub user_paused: bool, pub on_battery: bool, pub battery_ok: bool, pub session_battery_ok: bool, pub transcript_on: bool, pub engine_ok: bool, pub model_ok: bool, pub downloading: Option<u8>, pub failed: Option<String> }`; `pub enum CardStatus { Listening, Running, Paused, Battery, NeedsTranskrip, NeedsModel, Downloading(u8), Failed(String) }`; `pub fn status(c: &Ctl, has_lines: bool) -> Option<CardStatus>` (`None` when off); `pub fn should_run(c: &Ctl) -> bool`; `pub fn holds_queue(c: &Ctl) -> bool` (= `c.on`); `pub fn next_lang(l: TrLang) -> TrLang` (Id → En → Auto → Id).

Rules: `Lines::apply` inserts a `Final` keeping `start_ms` order (search from the back), drops the oldest beyond `MAX_LINES`, `Gap` pushes a `Line { gap: true, text: "…" }`, `Interim`/`ClearInterim` set/clear the channel's grey text. `copy_text` writes one line per final, `"Saya: "` / `"Peserta: "` prefixes only when labelled, gap lines as `…`. `status` priority: `!on` → None; `!transcript_on` → NeedsTranskrip; `downloading` → Downloading; `!engine_ok || !model_ok` → NeedsModel; `failed` → Failed; `user_paused` → Paused; `on_battery && !battery_ok && !session_battery_ok` → Battery; `has_lines` → Running else Listening. `should_run` is true only for Running/Listening.

- [ ] **Step 1: Write the failing tests:** `finals_stay_in_start_order`, `the_501st_line_drops_the_oldest`, `a_final_clears_its_channels_grey_line`, `copy_labels_only_with_two_sources` (assert exact strings: `"Peserta: Oke.\nSaya: Setuju."` vs `"Oke.\nSetuju."`), `latest_prefers_the_grey_line`, `status_order` (one assertion per rule above, e.g. paused + on battery → `Paused`; needs Transkrip beats downloading), `battery_stops_only_without_either_override`, `nothing_runs_while_paused_or_blocked`, `lang_cycles_id_en_auto`.
- [ ] **Step 2:** `cargo test --locked captions::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test --locked` → PASS.
- [ ] **Step 5:** Commit `feat(captions): caption lines and control model`.

### Task 4: Config `CaptionPrefs`, i18n keys

**Files:**
- Create: `src/captions/prefs.rs`
- Modify: `src/config.rs` (field after `transcript` line 208, `Default` line 781, `merge_owned` line 929, the full-struct test literal ~1371), `src/i18n.rs` (`keys!` table near `TrCardTitle` ~1322)

**Interfaces:**
- Produces: `pub enum TextSize { Small, Medium, Large }` with `body_px(self) -> f32` (15.0 / 18.0 / 22.0) and `step(self, up: bool) -> TextSize` (clamped); `pub struct CaptionPrefs { pub mic: bool, pub notch: bool, pub on_battery: bool, pub language: Option<TrLang>, pub size: TextSize, pub place: Option<CardPlace> }` with lenient `Deserialize` (every field on its own, like `TranscriptPrefs::from_json`; `place` through `config::card_place_from_json`) and a manual `Serialize` (JSON keys `mic`, `notch`, `on_battery`, `language` (`"id"`/`"en"`/`"auto"`/absent), `size` (`"s"`/`"m"`/`"l"`), `place`); `Config.captions: CaptionPrefs` (island-owned: listed in `merge_owned`).
- i18n keys (Indonesian / English): `CapTitle` "Caption langsung" / "Live captions"; `CapMe` "Saya" / "Me"; `CapOthers` "Peserta" / "Others"; `CapListening` "Mendengarkan…" / "Listening…"; `CapPaused` "Dijeda" / "Paused"; `CapBattery` "Pakai baterai, caption dijeda" / "On battery, captions paused"; `CapKeepGoing` "Tetap jalan" / "Keep going"; `CapNeedsTranskrip` "Caption butuh Transkrip" / "Captions need Transcripts"; `CapTurnOnTranskrip` "Nyalakan Transkrip" / "Turn on Transcripts"; `CapNeedsModel` "Model transkrip belum ada" / "No transcript model yet"; `CapDownload` "Unduh model" / "Download model"; `CapDownloading` "Mengunduh model" / "Downloading model"; `CapSlow` "Model ini berat buat caption di PC ini. Coba Whisper small." / "This model is heavy for captions on this PC. Try Whisper small."; `CapFailed` "Caption berhenti" / "Captions stopped"; `CapCopy` "Salin" / "Copy"; `CapSourceLost` "Suara PC terputus" / "PC audio lost" (and `CapMicLost` "Mic terputus" / "Mic lost"); `CapBatteryNotice` "Caption dijeda: pakai baterai" / "Captions paused: on battery"; Settings: `CapCardTitle` "Caption langsung", `CapMicRow` "Mic juga" + desc "Omonganmu ikut muncul sebagai \"Saya\"" / "Your own speech shows as \"Me\""; `CapNotchRow` "Caption di notch" + desc "Kalimat terakhir di notch" / "The last sentence in the notch"; `CapBatteryRow` "Tetap jalan pakai baterai" / "Keep running on battery"; `CapModelNote` "Pakai model Transkrip yang aktif" / "Uses the active Transcripts model"; `TrayCapOn` "Nyalakan caption" / "Turn on captions"; `TrayCapOff` "Matikan caption" / "Turn off captions"; `HotkeyRowCaptions` "Caption langsung" / "Live captions" + `HotkeyRowCaptionsDesc` "Nyala/mati caption langsung" / "Live captions on/off"; `RecCaptionSwitch` "Caption langsung" / "Live captions".

- [ ] **Step 1: Write the failing tests** in `prefs.rs`: `defaults_are_all_off_and_medium`, `round_trips`, `a_bad_field_falls_back_alone` (`{"mic":"yes","size":"xl","language":"id"}` → mic false, size Medium, language Some(Id)), `size_steps_clamp`; in `config.rs`: `captions_are_owned_by_the_island` (merge keeps mem's), `an_old_config_without_captions_loads`.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement and add the keys. **Step 4:** `cargo test --locked` → PASS (`every_key_has_text_in_both_languages`, `no_key_leaks_the_other_language` included).
- [ ] **Step 5:** Commit `feat(captions): caption settings and texts`.

### Task 5: The caption worker thread

**Files:**
- Create: `src/captions/worker.rs`

**Interfaces:**
- Consumes: `Live`, `live_params`, `Work`, `Out` (Task 2); `recorder::capture::{Capture, AudioSink, Source, Sources, ChoiceSet, CaptureError}`; `recorder::resample::Resampler`; `recorder::timeline::qpc_to_frame`; `transcribe::engine::{Engine, Model, Session, VadSession, EngineError}`; `transcribe::runner::{chunk_segments, default_threads}`; `transcribe::chunk::EnergyVad`; `transcribe::prefs::TrLang`.
- Produces:
  - `pub struct Spec { pub engine_dir: PathBuf, pub model_path: PathBuf, pub vad_path: Option<PathBuf>, pub gpu: bool, pub threads: i32, pub mic: bool, pub devices: [Option<String>; 2], pub language: Arc<AtomicU8> }` (`language` encodes `TrLang` as 0 Id, 1 En, 2 Auto; helpers `lang_to_u8`, `lang_from_u8`).
  - `pub enum Event { Started { backend: String }, Out(Out), SourceLost(Source), Failed(String), Ended }`
  - `pub struct Running` with `stop(self)` (sets cancel, signals the channel, joins) and `is_finished(&self) -> bool`.
  - `pub fn spawn(spec: Spec, on_event: impl FnMut(Event) + Send + 'static) -> std::io::Result<Running>`
  - Private sink `Feed` implementing `AudioSink`: per source a `Resampler` rebuilt when `rate` changes, start frame from `qpc_to_frame(qpc, 10_000_000, origin, 16_000)` with `origin` = the session start `qpc_now()`, sends `(channel, start_frame, Vec<f32>)` over `mpsc::Sender`; `lost` sends a lost marker.

Rules: thread named `caption-worker`, `THREAD_PRIORITY_BELOW_NORMAL` (as `runner::spawn`). Order inside the thread: `Engine::load`, `Model::load` (GPU when `spec.gpu`; on `EngineError::Backend` retry once on the CPU, as `runner::run_job`), VAD sessions (or `EnergyVad`) per channel, then `Capture::start(Sources { mic: spec.mic, system: true }, choices, false, Box::new(feed))` (echo cancellation off: captions never change the mic's mode), emit `Started`. Channels: `[System]` or `[Mic, System]`, speakers `[None]` or `[Some(Me), Some(Others)]`. Loop: `recv_timeout(200 ms)` and drain, `live.put`, `live.advance` until caught up to `now - origin`, then `next_work`: a final runs `Session::run(pcm, lang, cancel)` and feeds `chunk_segments` into `final_done`; an interim joins the text into `interim_done`; every `Out` goes out as `Event::Out`. Language read from the atomic per chunk. Stop: cancel flag → engine returns `Cancelled`, capture stopped, model dropped, `Ended` sent. A failure sends `Failed(kind)` with no caption text.

- [ ] **Step 1: Write the failing tests** (pure parts only, no device or engine): `a_rate_change_rebuilds_the_resampler` (feed a 48 kHz packet then a 44.1 kHz packet through `Feed::audio` with a test `Sender`; second block length ≈ `441 * 16000 / 44100` frames and its start frame follows the qpc, not the first rate), `packets_are_placed_by_qpc` (two packets 1 s apart in qpc → start frames 0 and 16 000), `lang_atomic_round_trips`, `channels_follow_the_mic_switch` (a pure `fn layout(mic: bool) -> (Vec<Source>, Vec<Option<Speaker>>)`).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** `cargo test --locked` → PASS.
- [ ] **Step 5:** Add a debug-only CLI `--captions-test <seconds> [--mic]` in `main.rs` (cfg(debug_assertions), `attach_parent_console`) that runs the worker against the installed model and prints each `Final`/`Interim` to the console with its time; run it for 20 s while a video plays and note GPU and CPU latency for the memory note.
- [ ] **Step 6:** Commit `feat(captions): caption worker with its own capture`.

### Task 6: Caption card layout (pure)

**Files:**
- Create: `src/caption_card/mod.rs` (declares `layout`), `src/caption_card/layout.rs`
- Modify: `src/main.rs` (`mod caption_card;`)

**Interfaces:**
- Consumes: `lyrics_card::layout::{RectF, Mon, place, point_in, is_drag, ease_out_cubic, ManualScroll, px}` (reuse, do not copy); `captions::prefs::TextSize`.
- Produces: constants `DEFAULT_W 380.0, DEFAULT_H 220.0, MIN_W 260.0, MIN_H 140.0, MAX_W 760.0, MAX_H 600.0, RADIUS 14.0, PAD 12.0, HEADER_H 34.0, GRIP 16.0, LINE_GAP 8.0, LABEL_GAP 6.0, EDGE_FADE 24.0, WHEEL_STEP 48.0, MANUAL_HOLD_MS 3000`; `pub struct Rects { header, dot, time, title, lang_chip, copy, pause, close, body, grip, action }` (`action` = the status button, e.g. "Unduh model" / "Tetap jalan" / "Nyalakan Transkrip"); `pub fn card_rects(w: f32, h: f32) -> Rects`; `pub fn clamp_size(w, h) -> (f32, f32)`; `pub enum Zone { Header, LangChip, Copy, Pause, Close, Action, Body, Grip }`; `pub fn zone_at(r: &Rects, pt: (f32, f32), action_shown: bool) -> Option<Zone>`; `pub fn bottom_stack(heights: &[f32], gap: f32, body_h: f32, scroll_back: f32) -> Vec<f32>` (tops with the newest line resting on the body's bottom; `scroll_back` ≥ 0 moves older lines into view, clamped by `scroll_back_max`); `pub fn scroll_back_max(heights: &[f32], gap: f32, body_h: f32) -> f32`; `pub fn place_default(...)` = `lyrics_card::layout::place` with this card's default size and the bottom-centre of the notch monitor's work area, 96 dip above its bottom (subtitle position, my call); colours `pub fn palette(dark_card: u32) -> Palette { bg, text: 0xFFFFFF, grey, me, others, gap }` and `pub fn contrast(fg: u32, bg: u32) -> f32`.

- [ ] **Step 1: Write the failing tests:** `the_newest_line_rests_on_the_bottom`, `scroll_back_is_clamped_to_the_oldest_line`, `zones_cover_the_header_buttons` (each rect centre maps to its zone; action only when shown), `sizes_clamp`, `the_default_place_is_bottom_centre_of_the_notch_monitor`, `contrast_numbers` (prints and asserts: text vs bg ≥ 7.0, grey vs bg ≥ 4.5, me and others label colours vs bg ≥ 4.5, light and dark window palettes through `theme::window_for(.., Dark, ..)` and the Terminal surface).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement (load antislop skills first; choose label colours with `check_contrast`). **Step 4:** PASS.
- [ ] **Step 5:** Commit `feat(captions): caption card layout`.

### Task 7: Caption card window, API and demo

**Files:**
- Create: `src/caption_card/window.rs`, `src/caption_card/demo.rs` (cfg(debug_assertions))
- Modify: `src/caption_card/mod.rs`, `src/main.rs` (`--demo-captions [scene]` in the debug arm of `opens_windows` ~105 and the match ~258, like `--demo-lyrics-card`), `src/app.rs` `release_graphics` (~825: add `caption_card::release()`)

**Interfaces:**
- Consumes: Task 6 layout; `captions::{lines::Line, control::CardStatus, prefs::TextSize}`; `transcribe::output::Speaker`; `TrLang`.
- Produces (in `mod.rs`):
  - `pub const WM_APP_CAPTION_CARD: u32 = WM_APP + 87;`
  - `pub enum Msg { Close = 1, Copy = 2, Pause = 3, Resume = 4, Lang = 5, Action = 6, Placed = 7, Size = 8 }` with `code`/`from_code` (lparam of `Size`: 1 up, 0 down).
  - `pub struct CardContent { pub status: CardStatus, pub lines: Change<Arc<Vec<Line>>>, pub interim: Vec<(Option<Speaker>, String)>, pub labelled: bool, pub running_since: Option<Instant>, pub lang: TrLang, pub size: TextSize, pub slow_hint: bool, pub palette: [u32; 3], pub font: UiFont, pub animations: bool }` (`Change` re-exported from `lyrics_card`).
  - `pub struct OpenArgs { pub island: HWND, pub saved: Option<CardPlace>, pub monitors: Vec<Mon>, pub notch: Option<Mon>, pub demo: bool }`
  - `pub fn open(args, content) -> bool`, `update(content)`, `close()`, `release()`, `hwnd() -> Option<HWND>`, `take_placed() -> Option<CardPlace>`.

Rules: class `ValeCaptionCard`, `WS_POPUP`, `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE`, shown with `SW_SHOWNOACTIVATE`, `WM_MOUSEACTIVATE` returns `MA_NOACTIVATE`. No fullscreen hide. Copy the lyrics card's surface, drag/resize (`SetCapture`, `WM_CAPTURECHANGED` → `capture_lost`, `after_press`), DPI and `WM_WINDOWPOSCHANGED` handling. Wheel: plain wheel scrolls back (`ManualScroll`, 3 s return on window timer 2); Ctrl+wheel posts `Size`. Window timers: 1 = header clock, a one-shot re-armed each second **only while** `running_since` is `Some` and the card is shown; 2 = manual-scroll return. Body paint: labels "Saya"/"Peserta" in their colours only when `labelled`; finals white; interim grey; gap lines grey "…"; top fade. Status rows render in the body with the action button when the status has one (NeedsTranskrip → "Nyalakan Transkrip", NeedsModel → "Unduh model", Battery → "Tetap jalan"; Downloading shows the percent). Slow hint: one grey line under the newest line until the next `open`. Reduced motion: values jump.

- [ ] **Step 1: Write the failing tests** (`ON_SCREEN = !cfg!(test)`): `open_update_close_without_a_window_on_screen`, `the_clock_timer_runs_only_while_running_and_shown` (assert with `KillTimer` result as the lyrics card test does), `a_click_never_activates` (`WM_MOUSEACTIVATE` → `MA_NOACTIVATE`), `capture_lost_after_a_drag_posts_one_placed`, `ctrl_wheel_posts_size_and_plain_wheel_scrolls`, `msg_codes_round_trip`.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement window and demo. Demo scenes: `two` (default: Saya/Peserta, one grey line), `one` (no labels, subtitle style), `long` (long wrapped lines, scroll back), `listening`, `needs-transkrip`, `needs-model`, `downloading`, `paused`, `battery`, `gap` (gap line + slow hint), `cjk`; `DI_LANG=en` works; the place goes to `temp_dir()/vale-caption-card-demo.json`.
- [ ] **Step 4:** `cargo test --locked` → PASS. Run `cargo run -- --demo-captions <scene>` for every scene and screenshot each (light and dark, S/M/L, a resize, scroll back and its return); check against the mockup section 3 of `docs/design/rekaman.html` and the antislop checklists.
- [ ] **Step 5:** Commit `feat(captions): caption card window and demo`.

### Task 8: App integration `app_captions.rs`

**Files:**
- Create: `src/app_captions.rs` (a `#[path]` child of `app`, like `app_lyrics_card.rs`)
- Modify: `src/app.rs` (state field `cap: CaptionState`; `Event::Captions(u64, captions::worker::Event)` in `model.rs` Event and its arm in `apply` next to `Event::Transcribe` ~1561, skipped by the redraw count like `Transcribe`; wndproc arm for `WM_APP_CAPTION_CARD` next to `WM_APP_LYRICS_CARD` ~9266; `check_battery` ~5640 calls `self.captions_power()`; `transcribe_set_enabled` and `transcribe_delete` call `self.captions_sync()`; `drop_app`/shutdown stops the worker), `src/app_transcribe.rs` (replace Task 1's `captions_hold` stub with `captions::control::holds_queue(&self.cap.ctl)`)

**Interfaces:**
- Consumes: everything above; `App::transcribe_download(model)`, `App::transcribe_set_enabled(bool)`, `self.tr.store` (`engine_installed`, `model_installed`, `model_path`, `engine_dir`, `vad_installed`, `vad_path`), `TranscriptPrefs::model_for(gpu)`, `gpu::has_hardware_gpu()`, `sources::battery::read_battery()` (on battery = `Some((_, false))`), `clipboard::win::{write, utf16z}` + `self.clip.own_sequence`, the "Disalin" notice (`NoticeKind::Copied`), `notice(..)` from `app_record.rs:185` with `NoticeKind::RecInfo`.
- Produces: `pub(super) struct CaptionState { ctl: Ctl, worker: Option<(u64, Running)>, next_id: u64, lines: Lines, lang: Arc<AtomicU8>, slow: bool, since: Option<Instant>, sent_gen: u64 }`; `App::captions_toggle()`, `captions_set(on: bool)`, `captions_pause(bool)`, `captions_sync()` (recomputes `Ctl` from config/store/battery, starts or stops the worker to match `should_run`, syncs the queue hold via `transcribe_sync_hold`, updates the card, the tray flag and the notch line), `captions_power()`, `on_captions(id, ev)`, `on_caption_card_msg(code, value)`, `captions_on(&self) -> bool`, `caption_line(&self) -> Option<&str>` (for Task 10).

Rules: turning on opens the card (`OpenArgs` from `monitors_of`, saved `config.captions.place`) and runs `captions_sync`; `Spec.devices` comes from the recording prefs' mic and PC-audio device choices, `Spec.language` from `config.captions.language` else `config.transcript.language`. `Msg::Close` = `captions_set(false)`; `Msg::Pause`/`Resume` = `captions_pause(true/false)`. Off stops the worker (`Running::stop`), closes the card, clears `Lines` and `slow`. An event whose id is not the current worker's is ignored. `Failed` sets `ctl.failed`, drops the worker; toggling off and on clears it. `SourceLost` adds a grey line (`CapSourceLost` / `CapMicLost`). Battery: when `captions_sync` moves from Running/Listening to Battery, one `RecInfo` notice `CapBatteryNotice`; back on AC it resumes by itself; `Msg::Action` in Battery sets `session_battery_ok`. `Msg::Action` in NeedsTranskrip calls `transcribe_set_enabled(true)` then, if engine/model missing, `transcribe_download(model_for(gpu))`; in NeedsModel only the download. Download progress and completion (`on_transcribe_download` end) call `captions_sync`. `Msg::Lang` cycles `next_lang`, stores into `config.captions.language`, saves, updates the atomic. `Msg::Size` steps `config.captions.size` and saves. `Msg::Copy` writes `lines.copy_text(..)` as `CF_UNICODETEXT` (13) through `clipboard::win::write`, sets `own_sequence`, shows the "Disalin" notice. `Msg::Placed` saves `config.captions.place`. The card is updated only when `Lines::generation`, status, lang, size or palette changed.

- [ ] **Step 1: Write the failing tests** (pure helpers in `app_captions.rs` tests, no window): `events_of_an_old_worker_are_ignored` (a pure `fn accepts(current: Option<u64>, id: u64) -> bool`), `battery_notice_only_on_the_running_to_battery_edge` (pure `fn battery_edge(before: Option<CardStatus>, after: Option<CardStatus>) -> bool`), `the_action_button_per_status` (pure `fn action_of(status) -> Option<Action>`: NeedsTranskrip → TurnOnTranskrip, NeedsModel → Download, Battery → KeepGoing, others None), `ctl_from_inputs` (pure `fn ctl_of(prev: &Ctl, prefs: &CaptionPrefs, transcript_on, engine_ok, model_ok, downloading, reading: Option<(u8, bool)>) -> Ctl`: desktop `None` reading is not on battery).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** `cargo test --locked` → PASS.
- [ ] **Step 5:** Commit `feat(captions): app side of live captions` (the first run on the real app comes in Task 9, once a hotkey turns it on).

### Task 9: Entry points: hotkey 14, tray, Rekam switch, Settings card

**Files:**
- Modify: `src/hotkey.rs` (`HotkeyId::Captions` appended after `RecordZoom`; `ALL: [HotkeyId; 13]`; `config_key` `"captions"`, default `Ctrl+Alt+C`, `Hotkeys` struct/Default/none/get/set/from_legacy), `src/app_hotkeys.rs` (`HOTKEY_CAPTIONS: i32 = 14`, `wm_id`, `row_of_wm_id`, `on_hotkey` → `captions_toggle`), `src/ui/settings_layout.rs` (`hotkey_row_key`/`_desc_key`; new `captions_card(b, s)` after `transcript_card` at ~3991 with three `b.toggle` rows `Hit::CaptionMic`, `Hit::CaptionNotch`, `Hit::CaptionBattery` and the model note line), `src/ui/settings.rs` (`SettingsAction::CaptionMic(bool)`, `CaptionNotch(bool)`, `CaptionBattery(bool)`, the optimistic mirror ~4586, the click wiring ~2580), `src/app.rs` (`SettingsAction` arms ~6527: save config, then `captions_sync()`; mic change restarts a running worker), `src/ui/tray.rs` (`MenuCmd::Captions` appended to `COMMANDS` (14 entries), item after `RecordScreen` with `TrayCapOn`/`TrayCapOff`, static `CAPTIONS` + `set_captions(hwnd, on)`), `app.rs` `open_menu` ~8536 (`Captions` → `captions_toggle`), `src/ui/panel_layout.rs` (`RecordLayout.caption_switch: Option<(Rect, Rect)>` label + switch using `NOTIF_SWITCH_W/H`, placed between the card and the recent list on both tabs; region `Hit::RecCaptions`), `src/model.rs` (`Hit::RecCaptions`, `Effect::CaptionToggle`, `RecordView.captions_on: bool`), `src/state.rs` (`Hit::RecCaptions` → `Effect::CaptionToggle`), `src/ui/render_panel.rs` (`paint_record` paints the switch like the Notifikasi pause switch).

**Interfaces:**
- Consumes: `App::captions_toggle`, `App::captions_on`, `App::captions_sync`, `CaptionPrefs`.
- Produces: nothing new for later tasks.

- [ ] **Step 1: Update and write the tests first:** every count-bearing test the survey lists moves from 12 to 13 rows (`hotkey.rs` `the_defaults_and_the_row_order`, `the_config_form_...` JSON, `app_hotkeys.rs` `ROWS == 13` and the 13 distinct ids, `settings_layout.rs` `card.h == HEADER_H + 13.0 * ROW_H`, `config.rs` hotkeys literals and JSON asserts; `tray.rs` `COMMANDS` positions and item lists); new: `the_captions_row_is_last_and_registered_always` (`wanted` registers id 14 with or without a recording), `the_captions_card_lists_its_three_switches` (hit list `[CaptionMic, CaptionNotch, CaptionBattery]`), `the_rekam_switch_sits_between_card_and_list_on_both_tabs`, `the_tray_says_turn_off_while_on`.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** `cargo test --locked` → PASS.
- [ ] **Step 5:** Screenshots: `cargo run -- --settings hotkeys`, `--settings` (Fitur, the new card), the Rekam page with captions off and on (use the existing `DI_DEMO_STEP` hooks of `ui/demo.rs`, adding a `record-captions` step if needed), and the tray menu.
- [ ] **Step 6:** First real run: `Stop-Process -Name vale, dynamic-island` then `cargo run --release`, Transkrip on with a model installed, `Ctrl+Alt+C`, play a video: lines appear, the card never takes focus, a fullscreen video keeps the card on top, and the thread count returns to the idle number after off (`(Get-Process vale).Threads.Count` before on and after off).
- [ ] **Step 7:** Commit `feat(captions): hotkey, tray, Rekam switch and Settings card`.

### Task 10: Notch caption line

**Files:**
- Modify: `src/model.rs` (`View::CaptionCompact`; `Event::CaptionLine(Option<String>)`), `src/state.rs` (`caption_line: Option<String>` reduced from `Event::CaptionLine`; `base_view()` ~1265: after `rec.is_some()` → `RecordCompact`, a `caption_line.is_some()` → `CaptionCompact`, before pomodoro, media and idle; every `View::RecordCompact` match site listed below gets the matching `CaptionCompact` handling where it concerns closed compact views), `src/app.rs` (View → `ShapeKind::MediaLyric` for `CaptionCompact` ~3801; `RecFrame.caption: Option<String>` built at ~6859 from `caption_line` when `config.captions.notch`; `TIMER_CAPTION_LINE: usize = 66` one-shot 8 000 ms, re-armed on each new text, posts `Event::CaptionLine(None)`), `src/app_captions.rs` (after each `Out` that changes `Lines::latest`, when `config.captions.notch`: `apply(Event::CaptionLine(latest))` and re-arm the timer; captions off clears it and kills the timer), `src/ui/geometry.rs` (`REC_CAPTION_EXTRA: f32 = 120.0` added to the RecordCompact width while a caption shows), `src/ui/panel_layout.rs` (`record_compact_layout` gets `caption: bool`; the label slot from `time.right() + 4` to the meters holds the caption), `src/ui/render_panel.rs` (`paint_record_compact` draws `rec.caption` cut from the left with a leading ellipsis in `fonts.eleven`, colour as the mockup's `#C9BBAC` role from the palette; new `paint_caption_compact` draws a small "CC" glyph and the line), `src/render.rs` (`View::CaptionCompact` dispatch ~2187).
- Sites to check for `View::RecordCompact` parity: `app.rs:5253, 5268, 6707`; `behind.rs:57`; `state.rs:1997-2005, 2059, 3073, 3146, 3161`; `gestures.rs`; `pointer_watch.rs:115`.

**Interfaces:**
- Consumes: `App::caption_line()` (Task 8), `config.captions.notch`.
- Produces: `View::CaptionCompact`, `Event::CaptionLine`.

Rules: hidden in fullscreen as every notch view (the existing `base_view` order already does it); no repeating timer; the line is the newest grey line else the newest final (`Lines::latest`); a hover opens the panel as the media compact view does; a click opens the Rekam page.

- [ ] **Step 1: Write the failing tests:** `state::tests::a_fresh_caption_line_takes_the_compact_view` (with media playing → `CaptionCompact`; `CaptionLine(None)` → `MediaCompact`), `..._recording_keeps_the_rec_view` (rec + line → `RecordCompact`), `..._fullscreen_hides_it`, `panel_layout::tests::the_rec_caption_slot_is_between_time_and_meters` (and widens by `REC_CAPTION_EXTRA`), `the_caption_text_is_cut_from_the_left` (pure `fn tail_fit(text, max_chars) -> String`).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** `cargo test --locked` → PASS.
- [ ] **Step 5:** Screenshots of the REC view with a caption and the compact caption view (a demo step in `ui/demo.rs`, e.g. `DI_DEMO_STEP=caption-notch` and `rec-caption`), light and dark, with and without the robot.
- [ ] **Step 6:** Commit `feat(captions): one-line caption in the notch`.

### Task 11: Live check, exe size, hand checks

**Files:**
- Modify: `docs/memory/recording-hand-checks.md` (a "Phase 5 live captions" section), `docs/memory/exe-size.md` (the measured entry)

- [ ] **Step 1:** Measure the exe: `cargo build --release --locked` of `main` (`git archive` into a scratch folder, its own `CARGO_TARGET_DIR`) and of the branch; record bytes and delta in `exe-size.md` as earlier entries do.
- [ ] **Step 2:** Live check on the real app: a YouTube video windowed and fullscreen (card on top, click on the card keeps the video fullscreen), "Mic juga" with a call or a voice note (labels, echo dropped), unplug the charger (card says "Pakai baterai", notice once, resumes on replug, "Tetap jalan"), Transkrip off (card offers it), a screen recording with the card open (the card is not in the `.mp4`), thread count at idle before on and after off, GPU latency and (with `DI_GPU=0` or the CPU path the engine exposes, if any) CPU latency.
- [ ] **Step 3:** Write what could not be verified into the hand-checks list: real Zoom/Teams meeting, a headset rate change mid-session, sleep/resume while captions run, a laptop without GPU, the hotkey `Ctrl+Alt+C` against other apps, reduced motion.
- [ ] **Step 4:** Commit `docs(captions): hand checks and exe size`.
