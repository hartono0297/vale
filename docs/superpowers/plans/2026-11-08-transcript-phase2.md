---
tags: [plan, recording, transcript]
date: 2026-11-08
spec: docs/superpowers/specs/2026-11-08-transcript-phase2-design.md
---

# Transcript after the meeting (recording phase 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every kept recording can become a transcript on this PC (queued, paused while recording, kept across restarts), read and played in its own window, copied, saved as a note or copied with a summary prompt, with a Settings card that downloads one of two Whisper models.

**Architecture:** Track B's core (`src/transcribe/`: catalog, store, download, decode, chunk, engine, runner, output) is merged with `main` and trimmed to two models. New pure modules carry the rules (`echo`, `queue`, `doc`, `prefs`, `gpu`); `app_transcribe.rs` (a child of `app`, like `app_record.rs`) owns the queue, the worker, the download and the notices; the Settings card, the Rekam page rows, the row menu (`menu_pop`) and a new transcript window (`src/transcript_win/`, own window, Settings palette, remembered place) are the UI; the player (Media Foundation source reader into a WASAPI render stream) comes last.

**Tech Stack:** Rust 2024, `windows` 0.62 (Win32, Direct2D, DirectWrite, WASAPI, DXGI), Media Foundation loaded at run time, transcribe.cpp 0.4.1 loaded at run time, `miniz_oxide` (track B), `serde_json`, `chrono`.

**Spec:** `docs/superpowers/specs/2026-11-08-transcript-phase2-design.md` (builds on `docs/superpowers/specs/2026-11-07-recording-design.md`; mockups `docs/design/rekaman.html` sections 4 and 5c). Read both specs and the memory notes they link before a task.

**Branch:** `rec-transcript`, worktree `.claude/worktrees/rec-transcript`, made from `worktree-agent-a1c20b057f5c3a485` with `main` merged in (Task 0). This plan never merges into `main` and never pushes: that is the controller's step after the final review.

**Model tiers** (for the controller): *cheap* = mechanical, *standard* = integration, *most capable* = design-heavy. Each task names one.

## Global Constraints

- Windows 11 x64, Rust 2024, one `vale.exe`. Engine and models are downloaded, never bundled; `vale.exe` stays about 4 MB.
- Ids from the 11-07 contract: island timer `TIMER_TRANSCRIBE = 65` (the only one this plan uses; 66-69 stay free), `WM_APP + 85` = `WM_APP_TRANSCRIPT` (transcript window to island), `WM_APP + 86` = `WM_APP_PLAYER` (player thread to its window); 87-89 stay free. No hotkey.
- Nothing at idle: no thread, timer or poll unless a job, a download or playback runs. The settle timer is a one-shot armed at start only when Transkrip is on.
- Media Foundation is only ever loaded with `LoadLibraryExW` through `crate::mf` (Task 1), never imported ([[media-foundation-dynamic-load]]).
- No new crate beyond track B's `miniz_oxide`. One new `windows` feature: `Win32_Graphics_Dxgi` (Task 2).
- No audio or text leaves the PC. Nothing downloads without a click; resuming a download a click started counts as that click.
- Every UI string goes through `src/i18n.rs` with Indonesian and English. Indonesian strings contain no English words (`no_key_leaks_the_other_language`, `no_formatted_text_leaks_the_other_language`). No em dash or en dash in any UI string; the separator is ` · ` (U+00B7). The copy in this plan is fixed: the antislop copywriting pass may propose changes, reported to the controller, not applied.
- UI tasks (7, 9, 10, 11, 12) and copy tasks (5) start by loading the antislop skills with the Skill tool: `antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-copywriting`, and `antislop:antislop-human` for keyboard and contrast. Mode is "during". Every colour pair drawn is checked with antislop-human's `check_contrast` tool in light, warm dark and Terminal dark (`theme::window_for`), and the numbers go in the task's report. Run each loaded skill's checklist before the task's last commit.
- Tests open no window and spawn no console ([[test-spawns-no-window]]: `ON_SCREEN = !cfg!(test)`, `CREATE_NO_WINDOW`); test files go under `target/test-tmp/` (`store::tests::temp_store`).
- Every task ends with `cargo test --locked` green and `cargo build --locked` with 0 warnings, then a commit on `rec-transcript`.
- Labels follow Vale's language when a transcript is written (`output::LABELS_ID` "Saya"/"Peserta", `LABELS_EN` "Me"/"Others").

## Review Focus

1. **A recording that starts while a job runs and then fails to start, stops at once, or is stopped by "no input"**: the queue must resume afterwards. The hold is derived from `RecState::engaged()` on every path, never toggled by hand (Task 4 tests `set_hold` is idempotent; Task 8 calls `transcribe_sync_hold` after `rec_start`, every arm of `rec_started`, and `rec_stopped`).
2. **Vale closed, crashed or the PC shut down mid-job or mid-download**: the running job is in the queue file (running first), a missing or broken queue file loads as empty, the `.part` resumes at settle, no half `.md` exists (Task 4 `the_queue_file_round_trips_with_the_running_job_first`, `a_broken_queue_file_loads_empty`; Task 3 writes atomically or not at all).
3. **The audio moved, renamed or deleted while queued, while the window is open, or between runs**: the job fails or is dropped with a plain reason, the window shows what it has, nothing panics (Task 4 `jobs_whose_audio_is_gone_are_dropped_at_load`; Task 5 `load_of_a_missing_recording_is_an_empty_doc`; Task 8 maps `DecodeError::Open` to "File audio tidak bisa dibaca").
4. **A `.srt` someone edited by hand, or one from another tool**: CRLF, a BOM, missing blank lines, a text that starts with "Saya:" in a one-source recording, bad time lines (Task 5 `parse_srt_*` tests).
5. **A two-hour meeting** (about 2,000 lines): the window measures line heights once per width (cached), lays out and hit-tests only the visible rows, and the live view re-merges at most once per runner event batch (Task 11 `only_visible_rows_are_laid_out`, `line_heights_are_measured_once_per_width`).

---

### Task 0: Branch, worktree, merge `main` (standard)

**Files:** conflicts expected in `src/main.rs`, `Cargo.toml`, `Cargo.lock`.

**Interfaces:**
- Produces: branch `rec-transcript` holding track B's 10 commits plus `main`; every later task starts here.

- [ ] **Step 1:** From the main checkout (never switch its branch, never stash or reset): `git worktree add .claude/worktrees/rec-transcript -b rec-transcript worktree-agent-a1c20b057f5c3a485`.
- [ ] **Step 2:** In the worktree: `git merge main`. Resolve: `main.rs` keeps every `mod` line and both sides' debug flags (`--transcribe` next to `--settings`); `Cargo.toml` keeps `miniz_oxide = "0.8"` and one `Win32_Media_MediaFoundation` line; `Cargo.lock` takes `main`'s file plus the `adler2` and `miniz_oxide` entries and the `"miniz_oxide"` dependency line of `vale` (if unsure, take `main`'s lock and run `cargo build` once without `--locked` so only those entries are added; `git diff main -- Cargo.lock` must show only them).
- [ ] **Step 3:** Run `cargo test --locked`. Expected: all pass (track B's engine tests that need the downloaded engine are `#[ignore]` already). Run `cargo build --locked`: 0 warnings.
- [ ] **Step 4:** Commit the merge: `git commit` with message `merge: main into the transcript core`.

### Task 1: One Media Foundation loader (cheap)

**Files:**
- Create: `src/mf.rs`
- Modify: `src/main.rs` (`mod mf;`), `src/recorder/encode.rs` (drop its `Mf`, `mf()`, `created`), `src/transcribe/decode.rs` (drop its `Mf`, `mf()`, `created`, `MfSession`)

**Interfaces:**
- Produces (in `crate::mf`):
  - `pub struct Mf { pub startup, pub shutdown, pub create_attributes, pub create_media_type, pub create_sample, pub create_memory_buffer, pub create_sink_writer, pub create_source_reader }` with the exact fn-pointer types the two files use today.
  - `pub fn mf() -> Option<&'static Mf>` (one `OnceLock`, `mfplat.dll` and `mfreadwrite.dll` from System32).
  - `pub fn created<T: Interface>(call: impl FnOnce(*mut *mut c_void) -> HRESULT) -> windows::core::Result<T>`.
  - `pub struct MfSession` with `pub fn start() -> Option<MfSession>` (COM MTA where possible plus `MFStartup(MF_VERSION, MFSTARTUP_LITE)`, undone on drop exactly as `decode.rs` does today). Callers map `None` to their own error (`EncodeError::NoMediaFoundation`, `DecodeError::NoMediaFoundation`).

- [ ] **Step 1: Test** in `src/mf.rs`: `fn media_foundation_loads_once()` asserts `mf().is_some()` and `std::ptr::eq(mf().unwrap(), mf().unwrap())`; `fn a_session_starts_and_stops()` asserts `MfSession::start().is_some()` twice in a row.
- [ ] **Step 2:** Run `cargo test --locked mf::` and see it fail to compile (no module).
- [ ] **Step 3:** Move the loader into `src/mf.rs`; point `encode.rs` and `decode.rs` at it. No behaviour change; the recorder's and the decoder's existing tests are the guard.
- [ ] **Step 4:** `cargo test --locked`: all pass, 0 warnings. `rg "LoadLibraryExW\(w!\(\"mf" src` finds only `src/mf.rs`.
- [ ] **Step 5:** Commit `refactor: one Media Foundation loader for the recorder and the decoder` (the memory note is the controller's docs step).

### Task 2: Two models, default by GPU, download and store fixes (cheap)

**Files:**
- Create: `src/transcribe/gpu.rs`
- Modify: `src/transcribe/{mod.rs, catalog.rs, download.rs, store.rs, engine.rs, debug.rs}`, `Cargo.toml` (feature `Win32_Graphics_Dxgi`)

**Interfaces:**
- Produces:
  - `catalog::MODELS: [Model; 2]` = `[whisper-large-v3-turbo, whisper-small]` in that order (the Settings order), entries unchanged. `Family::Qwen3Asr` and the runner's no-timestamp path stay (cheap, tested); the Qwen entries go.
  - `catalog::default_model(gpu: bool) -> &'static Model`: turbo with a GPU, small without.
  - `catalog::mb(bytes: u64) -> u64`: nearest megabyte, `(bytes + 500_000) / 1_000_000`.
  - `download::Item::Setup(&'static Model)`: the engine and the VAD when missing, then the model, in one run with one `Progress`. `download::assets_of(item: Item) -> Vec<Asset>` (every asset the item names, installed or not).
  - `download::DownloadError::DiskFull` (ERROR_DISK_FULL 112 and ERROR_HANDLE_DISK_FULL 39, from `classify` and from `From<io::Error>`).
  - `Store::queue_path(&self) -> PathBuf` = `<root>\queue.json`; `Store::discard_parts(&self, assets: &[Asset]) -> io::Result<()>`; `Store::sweep` also removes `models\*.gguf` that no catalog model and not the VAD owns (the owner's Qwen files from the comparison).
  - `gpu::has_hardware_gpu() -> bool` (DXGI: `CreateDXGIFactory1`, `EnumAdapters1`, cached in a `OnceLock`), `gpu::is_hardware(flags: u32, vendor: u32, device: u32) -> bool` (false for `DXGI_ADAPTER_FLAG_SOFTWARE` (2) and for Microsoft Basic Render, vendor `0x1414` device `0x8C`).
  - `engine::runtime_missing(dir: &Path) -> Vec<&'static str>` (public wrapper of `missing_runtime(|n| runtime_present(n, dir))`).

- [ ] **Step 1: Tests.**
  - `catalog`: `the_shipped_catalog_is_whisper_turbo_and_small` (`MODELS.len() == 2`, keys `"whisper-large-v3-turbo"`, `"whisper-small"` in that order, `model("qwen3-asr-0.6b") == None`); `default_model_follows_the_gpu` (`default_model(true).key == "whisper-large-v3-turbo"`, `default_model(false).key == "whisper-small"`); `mb_rounds_to_the_nearest_megabyte` (`mb(turbo.size) == 536`, `mb(small.size) == 172`, `mb(ENGINE.asset.size + VAD.size) == 19`, `mb(turbo + engine + vad) == 555`, `mb(small + engine + vad) == 191`). Replace `keys_are_unique_and_the_default_is_qwen3_small` and `sizes_match_the_design` (two models).
  - `store`: `the_layout_sits_under_the_transcribe_folder` now uses `default_model(true)` and expects `models\whisper-large-v3-turbo-Q4_K_M.gguf`; `sweep_removes_models_no_longer_offered` (a `Qwen3-ASR-0.6B-Q4_K_M.gguf` in `models\` goes; turbo at size and the VAD stay); `discard_parts_removes_only_those_parts`; `queue_path_is_beside_the_models`.
  - `download`: `setup_needs_the_engine_and_the_model` (empty store: `missing(Item::Setup(turbo)) == [ENGINE.asset, VAD, turbo.asset]`; engine and VAD installed: `[turbo.asset]`); `disk_full_is_its_own_error` (`classify(HRESULT(0x80070070u32 as i32)) == DiskFull`, `0x80070027` too, `DownloadError::from(io::Error::from_raw_os_error(112)) == DiskFull`, raw 5 gives `Disk(_)`).
  - `gpu`: `software_adapters_do_not_count` (`!is_hardware(2, 0x10DE, 0x2504)`, `!is_hardware(0, 0x1414, 0x8C)`, `is_hardware(0, 0x10DE, 0x2504)`, `is_hardware(0, 0x8086, 0x46A6)`).
- [ ] **Step 2:** Run them; they fail.
- [ ] **Step 3:** Implement. `debug.rs` uses `default_model(gpu::has_hardware_gpu())`.
- [ ] **Step 4:** `cargo test --locked` green, 0 warnings.
- [ ] **Step 5:** Commit `feat(transcribe): two Whisper models, the default by GPU`.

### Task 3: The echo filter and the runner's last changes (standard)

**Files:**
- Create: `src/transcribe/echo.rs`, `src/transcribe/testdata/run2254-turbo.txt`, `src/transcribe/testdata/run2254-small.txt`
- Modify: `src/transcribe/runner.rs`, `src/transcribe/mod.rs`

**Interfaces:**
- Consumes: `output::{Segment, Speaker, merge}`.
- Produces: `echo::drop_echo(segments: Vec<Segment>) -> Vec<Segment>` (order kept); `runner::transcribe` returns `segments: echo::drop_echo(output::merge(..))`; `write_files` with no segments writes no `.md` / `.srt`, deletes old ones, and still sets the sidecar's `"transcript"` with `"segments": 0`.

The rule (not determined by the signature, so here it is). Only `Speaker::Me` segments can be dropped; `Others` and unlabelled ones pass untouched.
1. Words: lowercase, split on every char that is not `char::is_alphanumeric`, empties dropped.
2. Content words: at least 3 chars and not in `STOP` = `jadi yang itu ini kalau juga udah sudah tapi dan atau gitu nah kan aja saja gue kita kami saya kamu mereka ada untuk dengan dari karena lagi tuh nih sih deh dong bang pada akan bisa mau nya the and that this is to of yes okay oke`.
3. The pool: every word of every `Others` segment with `start_ms <= me.end_ms + SLACK_MS` and `end_ms + SLACK_MS >= me.start_ms`, `SLACK_MS = 1_000`.
4. Two words match when equal, or when both have at least 3 chars and `levenshtein(a, b) * 3 <= max(len a, len b)` (chars).
5. Drop when the share of content words that match a pool word is at least `MATCH_SHARE = 0.5`. A segment with no content words is dropped only when every one of its words matches.

Measured on the whole run2254 output: turbo drops 119 of 120 "Saya" lines, small (GPU) 90 of 98; the reply "Boleh, jadi deadline-nya Jumat ya" placed at every 5 s of the recording is never dropped.

Fixture `run2254-turbo.txt` (the turbo GPU run's segment lines up to 00:27, verbatim from `asr/run2254/turbo-gpu.txt` in the scratchpad):

```text
[00:01 - 00:04] Saya    Coba adres pertanyaan-pertanyaan yang akhir-akhir ini
[00:04 - 00:06] Saya    Beruntun datang gue ya pertanyaan
[00:06 - 00:09] Saya    Kapan harga HP dan laptop turun?
[00:09 - 00:12] Saya    Katanya AI bubble itu udah pecah ya Bang
[00:12 - 00:14] Saya    Kan saham-saham di Amerika turun
[00:14 - 00:16] Saya    Lalu ada yang bilang, boy pot aja
[00:16 - 00:17] Saya    Gak bisa beli HP lagi
[00:17 - 00:19] Saya    Rame-rame gak beli HP itu
[00:19 - 00:21] Saya    Produsnya juga nurunin harga
[00:01 - 00:06] Peserta Coba address pertanyaan-pertanyaan yang akhir-akhir ini beruntun datang ke gue ya, pertanyaan
[00:06 - 00:09] Peserta Kapan harga HP dan laptop turun?
[00:09 - 00:14] Peserta Katanya AI bubble itu udah pecah ya bang, kan saham-saham di Amerika turun
[00:14 - 00:17] Peserta Lalu ada yang bilang, boycott aja, gak usah beli HP lagi
[00:17 - 00:21] Peserta Rame-rame gak beli HP, nanti produsenya juga nurunin harga
[00:21 - 00:23] Peserta Ini bener atau hoax?
[00:23 - 00:27] Peserta Lalu solusinya apa kalau harga gak turun-turun? Itu yang mau gue bahas kali ini ya
[00:21 - 00:23] Saya    ini bener atau hoax?
[00:23 - 00:26] Saya    lalu solusinya apa kalau harga gak turun-turun?
[00:26 - 00:27] Saya    itu yang gue bahas kali ini ya
```

Fixture `run2254-small.txt` (the small GPU run, same span, from `small-gpu.txt`):

```text
[00:01 - 00:06] Saya    Coba adres pertanyaan yang akhirnya ini meruntun dan saya akan bertanya
[00:06 - 00:09] Saya    Kapan harga HP dan laptop turun?
[00:09 - 00:14] Saya    Katanya AI Bubble itu udah pecah ya bang, kan samsang di Amerika turun
[00:14 - 00:17] Saya    Lalu dia bilang, boit pertanyaan, nggak bisa beli HP lagi
[00:17 - 00:21] Saya    Kera-mera nggak beli HP, itu aku perusahaan juga nerunin harga
[00:01 - 00:06] Peserta saya mau coba adres pertanyaan-pertanyaan yang akhirnya ini beruntun dan tanya gue ya pertanyaan
[00:06 - 00:09] Peserta kapan harga HP dan laptop turun?
[00:09 - 00:14] Peserta katanya AI Bubble itu udah pecah ya bang, kan sam-sam di Amerika turun
[00:14 - 00:17] Peserta lalu ada yang bilang boy kot aja, nggak usah beli HP lagi
[00:17 - 00:21] Peserta ramai-ramai nggak beli HP, yang terlalu producenya juga nerunin harga
[00:21 - 00:23] Peserta ini bener atau hoax
[00:23 - 00:27] Peserta lalu solusinya apa, kalau harga nggak turun-turun, itu yang bawa gue bahas kali ini ya
[00:21 - 00:23] Saya    ini bener atau hoax
[00:23 - 00:27] Saya    lalu solusinya apa? kalau harga matur-untur itu yang mau membahas kali ini ya
```

- [ ] **Step 1: Tests** in `echo.rs` (a `fixture(text) -> Vec<Segment>` helper parses `[MM:SS - MM:SS] Saya|Peserta text`):
  - `run2254_turbo_echo_is_dropped`: 12 `Me` in, 0 out; the 7 `Others` come out unchanged and in order.
  - `run2254_small_echo_is_dropped`: 7 `Me` in, 0 out; 7 `Others` unchanged.
  - `a_real_reply_over_someone_talking_stays`: the turbo fixture plus `Me 10_000..13_000 "Boleh, jadi deadline-nya Jumat ya"`; the reply is in the output, every other `Me` is not.
  - `me_without_an_overlapping_other_stays`: `Me 40_000..42_000 "Coba adres pertanyaan"` with `Others` ending at 27_000 stays.
  - `filler_alone_drops_only_when_every_word_matches`: `Me 5_000..5_500 "ya"` over `Others 4_000..7_000 "... gue ya, pertanyaan"` is dropped; `Me 5_000..5_500 "oke"` over the same is kept.
  - `unlabelled_segments_are_untouched`: speaker `None` segments come out as they went in.
  - `near_spellings_match`: `close("adres", "address")`, `close("dimilai", "dinilai")`, `!close("gw", "gue")`, `!close("jumat", "jual")`.
  - `runner`: `the_pipeline_drops_echoed_me_segments` (the existing fake source and recognizer, two channels giving the same text at the same time: one segment out, `Others`); `an_empty_result_leaves_no_transcript_files` (`write_files` on a temp audio path with an old `.md`, `.srt` and a sidecar: both files gone, sidecar `"transcript"."segments" == 0`, other sidecar keys kept).
- [ ] **Step 2:** Run; they fail.
- [ ] **Step 3:** Implement `echo.rs` and the two runner changes.
- [ ] **Step 4:** `cargo test --locked` green, 0 warnings.
- [ ] **Step 5:** Commit `feat(transcribe): drop the mic's echo of the meeting`.

### Task 4: The queue (standard)

**Files:**
- Create: `src/transcribe/queue.rs`

**Interfaces:**
- Produces:
  - `pub enum Hold { Startup, Recording, ScreenRecording }` (track C calls `ScreenRecording` when it integrates).
  - `pub struct QueuedJob { pub audio: PathBuf, pub model: String }` (catalog key).
  - `pub enum JobState { Queued, Running }`.
  - `pub struct Queue` with `new(holds: &[Hold]) -> Queue`, `push(&mut self, job: QueuedJob) -> bool` (false when that audio runs; a waiting one takes the new model and keeps its place), `set_hold(&mut self, hold: Hold, on: bool) -> Option<QueuedJob>` (turning a hold on while a job runs moves it back to the head and returns it, so the caller cancels its thread; idempotent), `held(&self) -> bool`, `start_next(&mut self, ready: impl Fn(&QueuedJob) -> bool) -> Option<QueuedJob>` (only with nothing running, no hold, and a ready head: strict FIFO, a head that is not ready blocks the rest), `finish(&mut self, audio: &Path) -> bool`, `cancel(&mut self, audio: &Path) -> Option<JobState>`, `clear(&mut self) -> Option<QueuedJob>`, `state(&self, audio: &Path) -> Option<JobState>`, `jobs(&self) -> impl Iterator<Item = &QueuedJob>` (running first), `is_empty(&self) -> bool`, `to_json(&self) -> String`, `from_json(text: &str, exists: impl Fn(&Path) -> bool) -> Vec<QueuedJob>`.
  - `same_audio(a: &Path, b: &Path) -> bool` (ASCII case-insensitive on the whole path; every comparison in the queue uses it).
  - `percent(done_ms: u64, total_ms: Option<u64>) -> Option<u8>` (0..=99 while running; `None` without a total).
  - `eta(elapsed: Duration, done_ms: u64, total_ms: Option<u64>) -> Option<Duration>` (`None` before 5 % done or 3 s elapsed; else `elapsed * (total - done) / done`, rounded up to whole seconds).
- File format: `{"v":1,"jobs":[{"audio":"C:\\...\\a.m4a","model":"whisper-small"}]}`, running job first. `from_json` is lenient: not JSON, another `v`, a bad entry are skipped; a job whose audio does not exist is dropped; a repeat of an audio is dropped.

- [ ] **Step 1: Tests:** `jobs_run_one_at_a_time_first_in_first_out`; `a_hold_sends_the_running_job_back_to_the_head` (`set_hold(Recording, true)` returns the running job, `state` is `Queued`, it is first in `jobs()`, `start_next` gives `None` until `set_hold(Recording, false)`, then that job); `two_holds_hold_until_both_are_off` (Recording and ScreenRecording); `holds_are_idempotent` (on twice, off once releases); `a_job_waits_for_startup_to_settle`; `a_head_that_is_not_ready_blocks_the_rest`; `pushing_a_waiting_audio_again_changes_its_model_not_its_place`; `pushing_the_running_audio_is_refused`; `paths_compare_without_case`; `cancel_and_clear_say_what_was_running`; `the_queue_file_round_trips_with_the_running_job_first`; `a_broken_queue_file_loads_empty` (`""`, `"x"`, `{"v":2,...}`, `{"jobs":5}`); `jobs_whose_audio_is_gone_are_dropped_at_load`; `percent_and_eta` (`percent(30_000, Some(60_000)) == Some(50)`, `percent(60_000, Some(60_000)) == Some(99)`, `percent(1, None) == None`, `eta(10 s, 30_000, Some(60_000)) == Some(10 s)`, `eta(10 s, 2_000, Some(60_000)) == None`, `eta(2 s, 30_000, Some(60_000)) == None`).
- [ ] **Step 2:** Run; fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings.
- [ ] **Step 5:** Commit `feat(transcribe): the transcript queue with pause holds`.

### Task 5: The transcript document and its texts (standard; copy: load antislop first)

**Files:**
- Create: `src/transcribe/doc.rs`
- Modify: `src/i18n.rs` (keys below), `src/transcribe/mod.rs`

**Interfaces:**
- Consumes: `output::{Segment, Speaker, Labels, LABELS_ID, LABELS_EN, short_time, paths, sidecar_title}`, `catalog::model`, `i18n::{short_date, time_hm, rec_duration}`.
- Produces:
  - `pub struct Doc { pub audio: PathBuf, pub md: PathBuf, pub title: String, pub app: Option<String>, pub started: Option<NaiveDateTime>, pub duration_ms: Option<u64>, pub model: Option<String>, pub language: Option<String>, pub two_channel: bool, pub has_transcript: bool, pub segments: Vec<Segment> }`.
  - `pub fn load(audio: &Path, fallback_title: &str) -> Doc`: sidecar (title, app, started, duration_ms, channels, `"transcript"` model and language) and `.srt`; never fails. `has_transcript` = the `.srt` exists or the sidecar has `"transcript"`. `two_channel` = the sidecar names two channels, or (no sidecar) the `.srt` has a known label.
  - `pub fn parse_srt(text: &str, two_channel: bool) -> Vec<Segment>`: cues by their `-->` line; a label prefix (`Saya: `, `Peserta: `, `Me: `, `Others: `) is read only when `two_channel`; text lines of a cue joined with one space; BOM, CRLF and missing blank lines tolerated; a cue with a bad time line is skipped.
  - `pub fn labels(lang: Lang) -> Labels`; `pub fn model_name(key: &str) -> String` (catalog name, else the key); `pub fn language_name(lang: Lang, code: &str) -> String` (`"id"` Indonesia / Indonesian, `"en"` Inggris / English, else the code uppercased).
  - `pub fn meta_line(doc: &Doc, lang: Lang) -> String`: the known parts of `[short_date time_hm, rec_duration, model_name, language_name]` joined by ` · `.
  - `pub fn plain_text(doc: &Doc, lang: Lang) -> String`: `"{title}\n{meta}\n\n"`, then one line per segment, `"{short_time} {label}: {text}"` or `"{short_time} {text}"` without a speaker, each ending in `\n`.
  - `pub fn prompt_text(doc: &Doc, lang: Lang) -> String`: `"{prompt}\n\n{plain_text}"`.
  - `pub const NOTE_LINES: usize = 20;` `pub fn note_text(doc: &Doc, lang: Lang) -> String`: `"{title}\n{short_date time_hm}\n\n"` (the date line left out when unknown), the first `NOTE_LINES` segment lines; with more segments, then `"\n\n{TrFullTranscript}: {md path}"`.
  - `pub fn current_line(segments: &[Segment], position_ms: u64) -> Option<usize>`: the last segment whose `start_ms <= position_ms`.
- i18n (Id / En), exact:
  - `TrPrompt`: "Ringkas transkrip meeting di bawah ini. Tulis: keputusan yang diambil, action item beserta siapa yang mengerjakan dan tenggatnya kalau disebut, dan pertanyaan yang belum terjawab. \"Saya\" adalah orang yang merekam, \"Peserta\" adalah orang lain di meeting." / "Summarize the meeting transcript below. Write: the decisions made, the action items with who does each and its deadline if one is mentioned, and the questions still open. \"Me\" is the person who recorded, \"Others\" are the other people in the meeting."
  - `TrFullTranscript`: "Transkrip lengkap" / "Full transcript"; `TrLangId`: "Indonesia" / "Indonesian"; `TrLangEn`: "Inggris" / "English"; `TrLangAuto`: "Otomatis" / "Auto".

- [ ] **Step 1: Tests:** `parse_srt_reads_what_output_writes` (`parse_srt(&output::srt(&segs, LABELS_ID), true) == segs` and the same with `LABELS_EN`); `parse_srt_tolerates_crlf_bom_and_missing_blank_lines`; `parse_srt_reads_labels_only_for_two_channels` (mono: text stays `"Saya: halo"`, speaker `None`); `parse_srt_skips_a_bad_time_line`; `parse_srt_joins_a_cue_s_lines`; `load_of_a_missing_recording_is_an_empty_doc` (title = fallback, no segments, `has_transcript == false`); `load_reads_the_sidecar_and_the_srt` (temp dir with a phase 1 sidecar plus `"transcript"`); `meta_line_in_both_languages` (expected built from `short_date`/`time_hm` + `" · 42 mnt · Whisper large-v3-turbo · Indonesia"`, and `" · 42 min · Whisper large-v3-turbo · Indonesian"`); `plain_text_has_times_and_labels`; `prompt_text_starts_with_the_spec_prompt` (`prompt_text(..).starts_with(text(Lang::Id, Key::TrPrompt))` and the exact Id prompt above); `note_text_keeps_twenty_lines_and_the_md_path` (25 segments: 20 lines, then `"Transkrip lengkap: C:\\r\\a.md"`; 20 segments: no path); `current_line_follows_the_position`. Add `meta_line`, `plain_text`, `note_text` outputs to `no_formatted_text_leaks_the_other_language`.
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings, antislop copywriting checklist run (report only).
- [ ] **Step 5:** Commit `feat(transcribe): the transcript document, copy texts and the summary prompt`.

### Task 6: Transcript settings in config (cheap)

**Files:**
- Create: `src/transcribe/prefs.rs`
- Modify: `src/config.rs` (field, default, `merge_owned`), `src/transcribe/mod.rs`

**Interfaces:**
- Consumes: `catalog::{model, default_model}`, `config::CardPlace` (read with the lenient reader `lyrics_card` uses).
- Produces:
  - `pub enum TrLang { Id, En, Auto }` (JSON `"id"`, `"en"`, `"auto"`; anything else reads `Id`); `TrLang::code(self) -> Option<&'static str>` (`Some("id")`, `Some("en")`, `None`).
  - `pub struct TranscriptPrefs { pub enabled: bool, pub model: Option<String>, pub language: TrLang, pub auto: bool, pub setup_offered: bool, pub window: Option<CardPlace> }`, defaults `false, None, Id, true, false, None`, `from_json` lenient field by field like `RecordingPrefs`, hand-written `Serialize`.
  - `TranscriptPrefs::model_for(&self, gpu: bool) -> &'static Model` (the chosen key when the catalog has it, else `default_model(gpu)`).
  - `Config.transcript: TranscriptPrefs`, key `"transcript"`, island-owned (`merge_owned` takes `mem.transcript`).

- [ ] **Step 1: Tests:** `transcript_defaults` (off, auto on, Indonesian, nothing offered); `a_bad_transcript_section_reads_as_defaults` (`"x"`, `7`, `null`, `{"enabled":"yes","language":"xx","auto":3}`); `an_old_model_key_falls_back_to_the_default` (`"qwen3-asr-0.6b"` gives turbo with a GPU, small without); `transcript_prefs_round_trip`; `merge_owned_keeps_the_transcript_settings`; `an_old_config_without_transcript_loads`.
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings.
- [ ] **Step 5:** Commit `feat(transcribe): transcript settings in config.json`.

### Task 7: Notices, hits and effects (standard; UI and copy: load antislop first)

**Files:**
- Create: `src/app_transcribe.rs` (`#[path = "app_transcribe.rs"] mod transcribe_page;` in `app.rs`), holding for now only empty `pub(super) fn transcribe_queue`, `open_transcript`, `open_transcript_settings`, `rec_pick_file`, `transcript_off_note` on `App`, each with a one-line doc; Tasks 8, 9 and 11 fill them.
- Modify: `src/model.rs`, `src/state.rs`, `src/ui/geometry.rs`, `src/ui/render.rs` (notice chips), `src/ui/render_panel.rs` (tests), `src/ui/robot.rs`, `src/ui/demo.rs`, `src/app_record.rs` (`saved_notice`; `entry_of` sets `transcript: RowTranscript::None` until Task 10), `src/app.rs` (the `perform` arms of the five new effects call those methods), `src/i18n.rs`

**Interfaces:**
- Produces in `model.rs`:
  - `pub enum TranscriptOffer { None, Transcribe, Setup }`; `NoticeKind::RecSaved { path: PathBuf, offer: TranscriptOffer }`.
  - `NoticeKind::TranscriptReady { audio: PathBuf }` (click opens the window), `TranscriptFailed { audio: PathBuf }` (click opens the window), `TranscriptInfo` (click dismisses), `TranscriptSettings` (click opens Settings at the Transkrip card).
  - `pub enum RowTranscript { Off, None, Queued, Waiting, Running(u8), Done, Failed }`; `RecEntry.transcript: RowTranscript`.
  - `Hit::RecTranscribeChip`, `Hit::RecTranscriptSetup`, `Hit::RecTranscript(usize)`, `Hit::RecPickFile`.
  - `Effect::TranscribeQueue(PathBuf)`, `Effect::OpenTranscript(PathBuf)`, `Effect::OpenTranscriptSettings`, `Effect::RecPickFile`, `Effect::TranscriptOff`.
  - `Event::Transcribe(u64, crate::transcribe::runner::Event)` and `Event::TranscribeDownload(u64, crate::transcribe::download::Event)` (job and download ids; the reducer ignores both; `App`'s event loop counts them with `Event::Rec(..)` as background events that draw nothing themselves).
- `state.rs`: the four new kinds queue like `RecSaved`; `notice_has_action` adds `TranscriptReady`, `TranscriptFailed`, `TranscriptSettings`; `dismiss_visible_notice`: `TranscriptReady`/`TranscriptFailed` give `Effect::OpenTranscript(audio)`, `TranscriptSettings` gives `Effect::OpenTranscriptSettings`, `RecSaved { path, .. }` still `RevealFile(path)`. `Hit::RecTranscribeChip` on a shown `RecSaved { offer: Transcribe }` gives `TranscribeQueue(path)` and takes the notice down without a reveal; `Hit::RecTranscriptSetup` on `offer: Setup` gives `OpenTranscriptSettings` the same way; both `LingerTrigger::No`. `Hit::RecTranscript(i)` on the Rekam page by `rec_recent[i].transcript`: `Off` gives `TranscriptOff` (panel stays), `None` gives `TranscribeQueue(path)` (panel stays, the row turns "Antre"), every other mark `collapse_for(OpenTranscript(path))`. `Hit::RecPickFile` gives `collapse_for(RecPickFile)`.
- `geometry.rs`: `TRANSCRIBE_CHIP_W: f32 = 76.0`, `SETUP_CHIP_W: f32 = 84.0`; `notice_chip_widths` for the two offers; `rec_saved_regions(chip: Rect, offer: TranscriptOffer) -> Vec<Region>`; icon tints: `TranscriptReady` sage, `TranscriptFailed` the `RecFailed` tint, the others neutral, with the Notes page icon. `robot.rs`: Ready happy, Failed sad, the rest idle.
- `app_record.rs`: `saved_notice(lang, path, duration, no_input, offer: TranscriptOffer) -> Notice`. `None` and `Transcribe`: body as today; `Setup`: body `"{rec_duration} · {TrWantOne}"`.
- i18n (Id / En): `TrNoticeSource` "Transkrip" / "Transcript"; `TrStarting` "Bikin transkrip" / "Making a transcript"; `TrReady` "Transkrip siap" / "Transcript ready"; `TrFailedTitle` "Transkrip gagal" / "Transcript failed"; `TrChipTranscribe` "Transkrip" / "Transcribe"; `TrChipSettings` "Pengaturan" / "Settings"; `TrWantOne` "Bikin transkrip?" / "Want a transcript?"; `TrOffTitle` "Transkrip belum nyala" / "Transcripts are off"; `TrOffBody` "Nyalakan di Pengaturan" / "Turn them on in Settings"; `TrModelReady` "Model transkrip siap" / "Transcript model ready"; `TrDownloadFailed` "Unduhan model gagal" / "Model download failed"; `TrMadeNote` "Dijadikan catatan" / "Saved as a note".
- Demo scenes in `ui/demo.rs`: `rec-saved-transcribe`, `rec-saved-setup`, `transcript-ready`, `transcript-failed`.

- [ ] **Step 1: Tests:** state: `the_transcribe_chip_queues_the_job_and_closes_the_notice` (`Click(Hit::RecTranscribeChip)` gives `[TranscribeQueue(path)]`, no `RevealFile`, current notice gone), `the_setup_chip_opens_the_transcript_settings`, `the_saved_notice_body_still_shows_the_file`, `offer_chips_do_nothing_on_other_notices`, `the_ready_and_failed_notices_open_the_transcript`, `the_settings_notice_opens_the_card`, `the_row_pill_acts_by_its_mark` (table over all seven marks, as above), `the_pick_file_link_collapses_for_the_picker`; geometry: `rec_saved_chip_widths_follow_the_offer`, `rec_saved_regions_hit_their_chip`; render: `rec_saved_chips_hold_their_words_in_both_fonts` (modelled on `sticky_reminder_chips_hold_their_words_in_both_fonts`, keys `TrChipTranscribe`, `TrChipSettings`); app_record: `saved_notice_bodies_per_offer` (Id Setup body `"42 mnt · Bikin transkrip?"`, En `"42 min · Want a transcript?"`). Existing `RecSaved { path }` matches gain `..`.
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; screenshots of the four demo scenes in light and dark (`--demo-notice <scene>` or the demo's own step, `DI_WINDOW`), contrast numbers in the report.
- [ ] **Step 5:** Commit `feat(transcript): notices, chips and the Rekam row hits`.

### Task 8: The island side: jobs, holds, downloads, notices (standard)

**Files:**
- Modify: `src/app_transcribe.rs` (from Task 7), `src/app.rs` (state field `tr: TranscribeState`, `on_timer` arm `TIMER_TRANSCRIBE`, startup call, both shutdown paths next to `rec_shutdown`, event loop arms for `Event::Transcribe` / `Event::TranscribeDownload`), `src/app_record.rs` (hold calls, `Saved` arm, `display_title`)

**Interfaces:**
- Consumes: Tasks 2-7 (`Queue`, `Hold`, `QueuedJob`, `percent`, `eta`, `echo`, `doc`, `TranscriptPrefs::model_for`, `gpu::has_hardware_gpu`, `Store`, `download::{start, Item::Setup, assets_of, Event, DownloadError}`, `runner::{spawn, Job, Event, RunError, default_threads}`, `engine::runtime_missing`).
- Produces (in `app_transcribe.rs`):
  - `pub(super) const TIMER_TRANSCRIBE: usize = 65;` `pub(super) const SETTLE_MS: u32 = 30_000;`
  - `pub(super) struct TranscribeState` (queue, job and download id counters, `worker: Option<Worker>` with id, audio, `runner::Running`, start `Instant`, last progress, live segments, title, `stopping: bool`; `download: Option<DownloadRun>` with id, model, `download::Download`, last `Progress`, `by_user_cancel: bool`; `failed: HashMap<PathBuf, String>` for this run; `problem: Option<String>` (runtime missing); `settled: bool`; `store: Store`).
  - `App` methods: `transcribe_init(&mut self)` (arms the one-shot only when `config.transcript.enabled`), `transcribe_settle(&mut self)` (sweep on a short `transcribe-sweep` thread, `Queue::from_json` of `store.queue_path()`, resume the first model whose `.part` (or the engine's) exists, release `Hold::Startup`, try the next job), `transcribe_queue(&mut self, audio: PathBuf, model: Option<&'static Model>)`, `transcribe_cancel(&mut self, audio: &Path)`, `transcribe_sync_hold(&mut self)` (`Hold::Recording` = `self.rec.engaged()`; a returned job's thread is cancelled with `stopping = true`), `transcribe_try_next(&mut self)` (no worker thread alive, not held, head ready = engine and that model installed and not being downloaded), `on_transcribe(&mut self, id: u64, event: runner::Event)`, `transcribe_saved(&mut self, path: &Path) -> TranscriptOffer`, `transcribe_set_enabled(&mut self, on: bool)` (off: cancel the job, clear the queue, cancel a download with its parts discarded, delete nothing else; on after start: `settled = true`), `transcribe_download(&mut self, model: &'static Model)`, `transcribe_cancel_download(&mut self)`, `transcribe_delete(&mut self, model: &'static Model)` (refused for the running job's model), `on_transcribe_download(&mut self, id: u64, event: download::Event)`, `transcribe_marks(&self, entries: Vec<RecEntry>) -> Vec<RecEntry>`, `transcribe_shutdown(&mut self)` (cancel and join the worker; the queue file keeps it; cancel the download keeping its parts).
  - Pure: `offer_for(enabled: bool, auto: bool, active_installed: bool, active_downloading: bool, any_installed: bool, setup_offered: bool) -> (bool /*queue*/, TranscriptOffer, bool /*mark offered*/)`; `job_reason(lang: Lang, e: &RunError) -> String`; `download_reason(lang: Lang, e: &DownloadError) -> String`; `mark_of(scanned: RowTranscript, enabled: bool, state: Option<JobState>, waiting_model: bool, pct: Option<u8>, failed: bool) -> RowTranscript`.
- Rules:
  - `offer_for`: on + auto + (active installed or downloading): queue, `None`. On + not auto + any installed: `Transcribe`. Otherwise, when `!setup_offered`: `Setup` and mark offered; else `None`. (Off or no model, shown once only.)
  - The `RecEvent::Saved` arm in `app_record.rs` calls `transcribe_saved` before building `saved_notice`. `rec_start`, every arm of `rec_started`, and `rec_stopped` end with `transcribe_sync_hold()`.
  - A job: `Job { audio, model_path: store.model_path(m), model: Some(m), engine_dir: store.engine_dir(), vad_path: store.vad_installed().then(|| store.vad_path()), language: prefs.language.code(), gpu: gpu::has_hardware_gpu(), threads: runner::default_threads(), labels: doc::labels(config.language) }`. The worker's callback sends `Event::Transcribe(id, ev)` and wakes the island (like `rec_poster`); `Progress` only when the percent changes or 250 ms passed.
  - Notices: `TranscriptInfo` "Bikin transkrip" / title, once per audio per run (not for a job resumed at settle or after a hold); on success `TranscriptReady { audio }` "Transkrip siap" / `"{title} · {RecClickToOpen}"`; on failure `TranscriptFailed { audio }` "Transkrip gagal" / reason. A `Cancelled` from a stop the island asked for says nothing. The title is the Rekam page's: `pub(crate) fn display_title(path: &Path, lang: Lang, today: NaiveDate) -> String` in `app_record.rs`, the title part of `entry_of` moved out (reads the sidecar beside `path`), which `entry_of` then calls.
  - `transcript_off_note` raises a `TranscriptSettings` notice `TrOffTitle` / `TrOffBody`; `transcribe_queue` with Transkrip off does the same instead of queueing.
  - Download end while Settings is closed: `TranscriptSettings` notice, `TrModelReady` / model name, or `TrDownloadFailed` / reason. After an engine install, `engine::runtime_missing(store.engine_dir())` non-empty sets `problem` and the notice body is `TrWhyRuntime`.
  - The queue file is written (`output::write_atomic`) after every queue change and deleted when the queue is empty.
  - Reasons, Id / En, exact: `TrWhyNoNetwork` "Tidak ada jaringan" / "No network"; `TrWhyRefused` "Server menolak unduhan ({code})" / "The server refused the download ({code})"; `TrWhyChecksum` "File unduhan rusak, coba lagi" / "The download came out damaged, try again"; `TrWhyDiskFull` "Disk penuh" / "The disk is full"; `TrWhyDisk` "File tidak bisa ditulis" / "The file cannot be written"; `TrWhyArchive` "Paket mesin tidak cocok" / "The engine package does not match"; `TrWhyRuntime` "Butuh Microsoft Visual C++ Redistributable (x64). Pasang dari situs Microsoft, lalu coba lagi." / "Needs the Microsoft Visual C++ Redistributable (x64). Install it from Microsoft's site, then try again."; `TrWhyAudio` "File audio tidak bisa dibaca" / "The audio file cannot be read"; Media Foundation missing reuses `RecWhyNoMf`; `TrWhyEngine` "Mesin transkrip rusak. Hapus modelnya lalu unduh lagi di Pengaturan" / "The transcript engine is damaged. Delete the model and download it again in Settings" (NotInstalled, BadLibrary, Mismatch, OtherFolder, Model); `TrWhyNoDevice` "Mesin transkrip tidak bisa jalan di PC ini" / "The transcript engine cannot run on this PC" (NoBackend); `TrWhyTooLong` "Rekaman terlalu panjang untuk model ini" / "The recording is too long for this model"; `TrWhyWrite` "Transkrip tidak bisa disimpan di folder ini" / "The transcript cannot be saved in this folder" (Io); `TrWhyOther` "Mesin transkrip gagal. Coba lagi" / "The transcript engine failed. Try again" (Backend, Status).

- [ ] **Step 1: Tests** (pure, in `app_transcribe.rs`): `offer_for_each_case` (a table over the inputs, including "off, offered already" giving `(false, None, false)`); `job_reason_names_each_failure` (each `RunError` variant to its key in Id); `download_reason_in_plain_words` (`DiskFull` "Disk penuh", `NoNetwork` "Tidak ada jaringan", `Status(404)` "Server menolak unduhan (404)", `Checksum(_)` "File unduhan rusak, coba lagi"); `marks_overlay_the_queue` (off with nothing on disk: `Off`; off with a transcript: `Done`; queued; queued whose model is missing: `Waiting`; running 42: `Running(42)`; failed this run: `Failed`). Add the formatted reasons to the i18n leak test.
- [ ] **Step 2:** Fail. **Step 3:** Implement and wire. **Step 4:** Green, 0 warnings. By hand with the debug build and a real downloaded model (the owner's store has both): record 20 s, see the notices, see `.md` / `.srt` beside the audio; start a recording while a job runs and see it resume after Stop; quit mid-job and see it finish after the 30 s settle.
- [ ] **Step 5:** Commit `feat(transcript): queue, worker, holds and downloads on the island`.

### Task 9: Settings → Fitur → Transkrip (most capable; UI and copy: load antislop first)

**Files:**
- Modify: `src/ui/settings_layout.rs` (`transcript_card` after `recording_card`, `State.transcript`, `Hit` variants, `CardId`, `Layout.card_tops`), `src/ui/settings.rs` (`Model.transcript`, `Action` variants, `on_click`, window state `transcript_choose`, `open_card`), `src/app.rs` (`settings_model`, `on_settings_action` arms, an `open_settings_card` like `open_settings` with the same deferral as `DEFERRED_OPEN_SETTINGS`), `src/app_transcribe.rs` (fills the `open_transcript_settings` stub with that deferral), `src/i18n.rs`

**Interfaces:**
- Consumes: Task 8's `transcribe_*` methods, `catalog::{MODELS, mb, ENGINE, VAD}`, `download::Progress`.
- Produces:
  - `pub struct TranscriptView { pub prefs: TranscriptPrefs, pub gpu: bool, pub installed: [bool; 2], pub engine_installed: bool, pub download: Option<(usize, Progress)>, pub download_error: Option<(usize, String)>, pub in_use: Option<usize>, pub used_bytes: u64, pub problem: Option<String> }` (indices into `MODELS`).
  - Settings `Hit`: `TranscriptEnabled`, `TranscriptOfferDownload`, `TranscriptOfferOther`, `TranscriptModel(usize)`, `TranscriptDownload(usize)`, `TranscriptCancel`, `TranscriptDelete(usize)`, `TranscriptLang(TrLang)`, `TranscriptAuto`, `TranscriptFolder`.
  - Settings `Action`: `TranscriptEnabled(bool)`, `TranscriptModel(&'static str)`, `TranscriptDownload(&'static str)`, `TranscriptCancelDownload`, `TranscriptDelete(&'static str)`, `TranscriptLang(TrLang)`, `TranscriptAuto(bool)`, `TranscriptFolder`.
  - `pub enum CardId { Transcript }`; `Layout.card_tops: Vec<(CardId, f32)>`; `settings::open_card(owner: HWND, model: Model, card: CardId) -> Result<HWND>` (opens on Fitur, scrolled so the card's eyebrow sits at the top margin, clamped).
- The card's rows by state:
  - Off: the switch only.
  - On, nothing installed, no download, not choosing: switch; the offer `TrOffer` with **Unduh** (primary) and **Pilih model lain** (secondary); Bahasa; auto (greyed); footer.
  - Otherwise: switch; one row per model (radio, name, "disarankan" tag on `default_model(gpu)`, note, size `"{mb} MB"`, and **Unduh** / a 3 dip progress bar with `"{done} / {total} MB"` and **Batal** / "Terpasang" with **Hapus**; Hapus disabled with the note "Sedang dipakai" for `in_use`; other rows' Unduh disabled while a download runs; a failed download shows its reason under the name in the error colour); Bahasa; auto; footer.
  - Auto greyed while off or nothing installed, with its note. Footer: `TrWhereGpu`/`TrWhereCpu` + `" · "` + `TrUsed(mb)`, and **Buka folder** (Explorer on `store.root()`). `problem` shows as one line in the error colour above the footer.
- Copy, Id / En, exact: `TrCardTitle` "Transkrip" / "Transcripts"; `TrEnable` "Transkrip" / "Transcripts", desc `TrEnableDesc` "Ubah rekaman jadi teks di PC ini. Tidak ada audio yang dikirim." / "Turns recordings into text on this PC. No audio leaves it."; `tr_offer(lang, name, total_mb, engine_mb)` "Unduh {name} ({total} MB, termasuk mesin {engine} MB)?" / "Download {name} ({total} MB, including the {engine} MB engine)?"; `TrDownload` "Unduh" / "Download"; `TrOther` "Pilih model lain" / "Choose another model"; `TrCancel` "Batal" / "Cancel"; `TrInstalled` "Terpasang" / "Installed"; `TrDelete` "Hapus" / "Delete"; `TrInUse` "Sedang dipakai" / "In use"; `TrSuggested` "disarankan" / "suggested"; `TrNoteTurbo` "Paling akurat · cepat dengan GPU, lambat tanpa GPU" / "Most accurate · fast with a GPU, slow without one"; `TrNoteSmall` "Ringan · lebih cepat tanpa GPU, kadang ada kata yang hilang" / "Light · quicker without a GPU, sometimes drops words"; `TrLanguage` "Bahasa" / "Language" (choices `TrLangId`, `TrLangEn`, `TrLangAuto`); `TrAuto` "Bikin transkrip otomatis setelah rekam" / "Transcribe automatically after recording", desc `TrAutoDesc` "Mulai sendiri begitu rekaman berhenti" / "Starts on its own when a recording stops", greyed notes `TrAutoNeedsOn` "Nyalakan Transkrip dulu" / "Turn on Transcripts first" and `TrAutoNeedsModel` "Perlu model terpasang" / "Needs an installed model"; `TrWhereGpu` "Pakai GPU" / "Uses the GPU"; `TrWhereCpu` "Pakai CPU" / "Uses the CPU"; `tr_used(lang, mb)` "Terpakai {mb} MB" / "{mb} MB used"; progress `tr_progress(done_mb, total_mb)` "{done} / {total} MB".
- Debug: `--settings features` with `DI_TRANSCRIPT=off|offer|list|downloading|installed|problem` fills the view without touching the store.

- [ ] **Step 1: Tests** (`settings_layout.rs`, `settings.rs`): `the_transcript_card_rows_follow_its_state` (one test per state above: which hits exist); `the_offer_names_the_real_sizes` (`tr_offer(Id, "Whisper large-v3-turbo", 555, 19) == "Unduh Whisper large-v3-turbo (555 MB, termasuk mesin 19 MB)?"`, small `191`); `delete_is_off_for_the_model_in_use`; `other_downloads_wait_for_the_running_one`; `auto_is_greyed_when_off_or_without_a_model`; `a_download_shows_its_progress_and_cancel` (`"220 / 555 MB"`, `Hit::TranscriptCancel`); `the_transcript_card_can_be_scrolled_to` (`open_card` scroll puts the card top at the margin at 720x520 and is clamped at the end); clicks map to actions (`Hit::TranscriptModel(1)` gives `Action::TranscriptModel("whisper-small")`, the switch toggles, the language segment, Pilih model lain sets `transcript_choose` without an action); the existing `exactly_the_overflowing_sections_get_a_viewport`, `everything_fits_in_the_window_height` and `every_card_of_every_section_has_a_title_but_the_previews` stay green. Formatted texts join the i18n leak test.
- [ ] **Step 2:** Fail. **Step 3:** Implement (the island side of each `Action` calls Task 8; `sync_settings` after each download event, delete and enable). **Step 4:** Green, 0 warnings; screenshots of every `DI_TRANSCRIPT` state in light, dark and `DI_SURFACE=terminal`; contrast numbers (text, secondary, tag, progress fill vs track, error) in the report.
- [ ] **Step 5:** Commit `feat(transcript): Settings card to pick, download and switch models`.

### Task 10: The Rekam page: row states, row menu, another file (standard; UI and copy: load antislop first)

**Files:**
- Create: `src/ui/transcript_menu.rs`
- Modify: `src/ui/panel_layout.rs` (`record_layout`, `RecordHead`, `regions`), `src/ui/render_panel.rs` (`paint_record`), `src/app_record.rs` (`entry_of` marks, `display_title`, `RecState.recent`, re-sending marks), `src/app.rs` (`RightClick::RecRowMenu(usize)`, `rec_row_menu_target`, `open_rec_row_menu`, the `WM_APP_MENU_DONE` arm, `Effect::RecPickFile`, `Action::TranscribeFile`), `src/ui/settings.rs` (`pick_rec_audio`, `Action::TranscribeFile(PathBuf)`), `src/i18n.rs`

**Interfaces:**
- Consumes: `RowTranscript`, `Hit::{RecTranscript, RecPickFile}`, the `rec_pick_file` stub (Task 7), `transcribe_marks`, `transcribe_queue`, `transcribe_cancel`, `display_title` (Task 8), `ui::menu_pop::{open, Row, Align, Done}`.
- Produces:
  - `record_layout(s, strip, head: &RecordHead, live, chosen, rows: usize, pills: &[f32]) -> RecordLayout`; `RecordHead.pick_w: f32`, `RecordLayout.pick: Rect`; `RecordRowLayout.pill` sized from `pills[i]` (measured text + 16, clamped 44..=112 dip), right-aligned, the title and sub lines end 8 dip before it; unfinished rows (`.m4a.part`) get no pill. Regions: `RecPickFile`, then per row the pill (`RecTranscript(i)`) before the row (`RecOpen(i)`).
  - The header: "Buka folder" at the right as today, "Transkrip file lain…" 12 dip left of it; when both do not fit beside the title, the short form `TrPickFileShort` is used.
  - `entry_of` sets `transcript: Done` when `<stem>.srt` exists or the sidecar has `"transcript"`, else `None`.
  - `RecState.recent: Vec<RecEntry>` (the last scan); `Event::RecRecent(self.transcribe_marks(recent))` is sent again on every queue, progress-percent or failure change while the page shows.
  - `transcript_menu::rows(lang: Lang, mark: RowTranscript, installed: &[&'static Model], active: &'static Model) -> Vec<Row>`; `transcript_menu::open(owner: HWND, anchor: Rect, align: Align, audio: PathBuf, rows: Vec<Row>, ui: UiFont) -> bool`; `pub enum MenuChoice { View(PathBuf), Transcribe(PathBuf), Redo(PathBuf, &'static Model), Cancel(PathBuf) }`; `transcript_menu::picked(done: &Done) -> Option<MenuChoice>` (`None` for another dropdown's `Done`, so the device menu path still runs).
  - Rows by mark: `None`: Bikin transkrip. `Off`: no menu (the right-click is swallowed). `Queued`/`Waiting`/`Running`: Lihat transkrip, separator, Batalkan transkrip. `Done`/`Failed`: Lihat transkrip, separator, one "Transkrip ulang" row per installed model with the model name as its detail line, the active model first and checked. Anchor: the row rect in screen px, `Align::End`. The panel gets `MenuOpen(true)` / `(false)` like the device menu.
  - `settings::pick_rec_audio(owner: HWND, start: PathBuf)`: like `pick_rec_folder`, a `rec-pick-audio` thread, `IFileOpenDialog` with the filter `TrFilter` ("*.m4a;*.wav") and `SetFolder` on `start` (`SHCreateItemFromParsingName`), posting `Action::TranscribeFile(path)`. The island's `rec_pick_file` (Task 7 stub) starts the picker on `config.recording.dir()`; `Action::TranscribeFile(path)` queues the job when the file has no transcript and Transkrip is on, then calls `open_transcript(path)` (still empty until Task 11).
- Copy, Id / En: pills `TrPillTranscribe` "Transkrip" / "Transcribe", `TrPillQueued` "Antre" / "Queued", `TrPillWaiting` "Menunggu model" / "Waiting for model", `TrPillDone` "Lihat" / "View", `TrPillFailed` "Gagal" / "Failed", running `"{n}%"`; the `Off` pill is the Transcribe label at the disabled alpha (0.45). Menu `TrMenuView` "Lihat transkrip" / "View transcript", `TrMenuMake` "Bikin transkrip" / "Make a transcript", `TrMenuRedo` "Transkrip ulang" / "Transcribe again", `TrMenuCancel` "Batalkan transkrip" / "Cancel the transcript". Header `TrPickFile` "Transkrip file lain…" / "Transcribe a file…", `TrPickFileShort` "File lain…" / "Other file…". Dialog filter `TrFilter` "Rekaman (*.m4a, *.wav)" / "Recordings (*.m4a, *.wav)".

- [ ] **Step 1: Tests:** panel_layout: `the_transcript_pill_sits_right_and_wins_over_the_row` (replaces the `row.pill` assertion of the existing record layout test with `Hit::RecTranscript(i)`); `the_title_ends_before_a_wide_pill` ("Menunggu model" width); `an_unfinished_row_has_no_pill`; `both_header_links_fit_or_the_short_form_is_used` (measured widths in both languages at the default panel size: the long form fits); app_record: `scan_marks_a_recording_with_an_srt_or_a_sidecar_transcript_done`; transcript_menu: `menu_rows_per_mark` (exact labels and ids for each mark), `picked_maps_each_row` (and `picked` of a device `Done` is `None`); app: `right_clicks_on_a_rec_row_open_its_menu` (`rec_row_menu_target(View::Panel(Page::Record), Some(&Hit::RecOpen(2))) == Some(2)`, the pill too, other pages `None`).
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; `--demo-record` screenshots with every mark (a demo step that sets them) in light and dark; contrast for every pill state.
- [ ] **Step 5:** Commit `feat(transcript): transcript state on the Rekam page, its menu and another file`.

### Task 11: The transcript window (most capable; UI and copy: load antislop first)

**Files:**
- Create: `src/transcript_win/{mod.rs, layout.rs, window.rs, demo.rs}`, `src/app_transcript_win.rs` (`mod transcript_page;` child of `app`)
- Modify: `src/main.rs` (`mod transcript_win;`, `--demo-transcript [scene]`), `src/app.rs` (`WM_APP_TRANSCRIPT` arm, `Effect::OpenTranscript`, `apply_theme` and `on_system_theme_changed` call `transcript_win::restyle()`, `release_graphics` calls `transcript_win::release()`, shutdown calls `transcript_win::close_all()`), `src/i18n.rs`

**Interfaces:**
- Consumes: `doc::{Doc, load, plain_text, prompt_text, note_text, meta_line, current_line}` (Task 5), `echo::drop_echo`, `output::merge`, `queue::{percent, eta, JobState}`, Task 8's state, `transcript_menu::open` (Task 10, anchored under the Redo button), `theme::current_window()`, `clipboard::win::{write, utf16z}` with `self.clip.own_sequence` (as `copy_search_result`), `notes.upsert` + `persist_notes`, `sources::reveal::reveal`, `shortcuts::Action::OpenPath`.
- Produces:
  - `pub const WM_APP_TRANSCRIPT: u32 = WM_APP + 85;`
  - `pub enum Status { None, Queued, Waiting, Held, Running { pct: u8, eta: Option<Duration> }, Failed(String), Off, PlayerError }`.
  - `pub struct View { pub doc: Doc, pub status: Status, pub can_redo: bool, pub lang: Lang, pub font: UiFont }` (while running, `doc.segments` are the live ones: `echo::drop_echo(output::merge(..))` of what the runner sent).
  - `pub enum Request { Copy, CopyPrompt, OpenMd, OpenFolder, MakeNote, Redo { anchor: crate::capture::Rect }, Cancel, Retry, Settings, Closed, Placed(CardPlace) }`.
  - `pub fn open(owner: HWND, view: View, place: Option<CardPlace>) -> bool` (one window per audio, by `same_audio`: an open one is updated and brought forward; a second window opens 24 px down and right of the first), `pub fn update(audio: &Path, view: View)`, `pub fn is_open(audio: &Path) -> bool`, `pub fn take_requests() -> Vec<(PathBuf, Request)>`, `pub fn restyle()`, `pub fn close_all()`, `pub fn release()`.
  - The window procedure never calls `app::with_app`; it queues a `Request` and posts `WM_APP_TRANSCRIPT` to the owner, which drains `take_requests` (the `list_win` pattern).
  - `app_transcript_win.rs`: `open_transcript(&mut self, audio: PathBuf)`, `transcript_view(&self, audio: &Path) -> View`, `on_transcript_requests(&mut self)`, `transcript_refresh(&mut self, audio: &Path)` (called by Task 8 on each runner event for an open window, on finish, on queue changes).
  - `layout.rs` (pure): `DEFAULT_W = 640.0`, `DEFAULT_H = 720.0`, `MIN_W = 480.0`, `MIN_H = 400.0` (dip); `pub enum Btn { Copy, OpenMd, OpenFolder, MakeNote, CopyPrompt, Redo, Cancel, Retry, Settings }`; `pub fn buttons_for(status: &Status, has_lines: bool, can_redo: bool) -> Vec<Btn>`; `pub struct Frame { pub header: Rect, pub buttons: Vec<(Btn, Rect)>, pub play: Rect, pub position: Rect, pub bar: Rect, pub length: Rect, pub status: Option<Rect>, pub list: Rect }`; `pub fn frame(w: f32, h: f32, buttons: &[(Btn, f32)], status: bool) -> Frame` (buttons 30 dip high, 8 dip apart, wrapping to more rows); `pub fn line_tops(heights: &[f32]) -> Vec<f32>`; `pub fn visible(tops: &[f32], heights: &[f32], scroll: f32, list_h: f32) -> Range<usize>`; `pub fn line_at(tops: &[f32], heights: &[f32], scroll: f32, y: f32) -> Option<usize>`; `pub fn follow_scroll(tops: &[f32], heights: &[f32], current: usize, scroll: f32, list_h: f32) -> f32` (keeps the line within the 25 %-60 % band); `pub fn seek_ms(bar: Rect, x: f32, duration_ms: u64) -> u64` (clamped); `pub fn place(saved: Option<CardPlace>, monitors: &[lyrics_card::layout::Mon], fallback: lyrics_card::layout::Mon) -> lyrics_card::layout::Rc` (the half-on-a-monitor rule of `lyrics_card::layout::place`, this window's limits, default centred on the notch monitor's work area).
- Buttons by status, in the spec's order: lines and no job: Salin, Buka .md, Buka folder, Jadikan catatan, **Salin + prompt ringkasan** (primary, accent), Transkrip ulang (only with `can_redo`). Running, Queued, Waiting, Held: Batal (lines so far still listed). Failed: Coba lagi, Buka folder. Empty transcript: Transkrip ulang, Buka folder. Off: Buka Pengaturan.
- Look (follows mockup 4): a normal top-level window (`WS_OVERLAPPEDWINDOW`, class `ValeTranscript`, in the taskbar), title bar coloured through DWM as `ui/settings.rs` does (`DWMWA_CAPTION_COLOR`, `DWMWA_TEXT_COLOR`, `DWMWA_USE_IMMERSIVE_DARK_MODE`), Direct2D drawn in `current_window()` colours, the UI font. Header: title (17 px, 600) and `meta_line` (12.5 px, secondary). Player bar (44 dip): round play/pause (32 dip), position, a 4 dip track with the accent fill and a 12 dip thumb, length. Status line above the list while a job runs, waits or failed. Lines: time (tabular figures, fixed column), a label chip (Saya on the `selection` fill in text colour, Peserta on the `hover` fill in text colour; no chip for one-source recordings), the text wrapped. The playing line: `selection` fill and a 3 dip accent bar at its left. Hover: `hover` fill and the hand cursor.
- Keyboard and focus (antislop-human): Tab / Shift+Tab move through the buttons, play, the seek bar and the list; a 2 dip accent focus ring; Enter or Space activates a focused button; in the list, Up/Down move the focused line, Enter plays from it; PgUp/PgDn/Home/End scroll; Space toggles play when the list or the player has focus; Ctrl+C is Salin; Esc closes. Wheel: 48 dip a notch.
- Place: `config.transcript.window` (screen px) saved on `WM_EXITSIZEMOVE` through `Request::Placed`; DPI from the monitor; `WM_DPICHANGED` uses the suggested rect.
- Messages and copy, Id / En: buttons `TrBtnCopy` "Salin" / "Copy", `TrBtnOpenMd` "Buka .md" / "Open .md", `RecOpenFolder` (exists), `TrBtnNote` "Jadikan catatan" / "Make a note", `TrBtnPrompt` "Salin + prompt ringkasan" / "Copy + summary prompt", `TrMenuRedo`, `TrCancel`, `TrBtnRetry` "Coba lagi" / "Try again", `TrBtnSettings` "Buka Pengaturan" / "Open Settings"; status `TrStatusQueued` "Antre, mulai setelah transkrip lain selesai" / "Queued, starts after the others finish", `TrStatusWaiting` "Menunggu model selesai diunduh" / "Waiting for the model to download", `TrStatusHeld` "Dijeda selama merekam" / "Paused while recording", `tr_running(lang, pct, eta)` "{pct}% · sekitar {rec_duration} lagi" / "{pct}% · about {rec_duration} left" (without an eta: "{pct}%"), `TrEmpty` "Tidak ada ucapan yang terbaca" / "No speech could be made out", `TrOffTitle` (Task 7), `TrPlayerError` "Audio tidak bisa diputar" / "The audio cannot be played"; accessible names `TrPlay` "Putar" / "Play", `TrPause` "Jeda" / "Pause".
- Requests on the island: Copy writes `plain_text` and shows the `Copied` notice ("Disalin", body the title); CopyPrompt writes `prompt_text`, same notice; MakeNote `notes.upsert(None, &note_text, now)`, `persist_notes`, notice `TrMadeNote` / title; OpenMd `queue_shortcut(Action::OpenPath(md))`; OpenFolder `reveal(audio)`; Redo opens the row menu's Done/Failed rows under the button; Cancel `transcribe_cancel`; Retry `transcribe_queue(audio, None)`; Settings opens the card; Closed forgets the window; Placed saves the place.
- Demo `--demo-transcript [lines|running|empty|failed|queued|off|long]` (debug builds): writes a fixture `.srt` and sidecar under `temp_dir()\vale-transcript-demo\`, opens the window with no engine, `DI_WINDOW` and `DI_SURFACE` apply; `long` has 2,000 lines.

- [ ] **Step 1: Tests:** layout: `buttons_for_each_status`; `buttons_wrap_at_the_minimum_width_in_both_languages` (measured widths; no two rects overlap, all inside the window); `line_tops_and_line_at_agree`; `only_visible_rows_are_laid_out` (2,000 lines of 20 dip, list 400 dip: the range has at most 21 rows); `follow_scroll_keeps_the_current_line_in_the_band`; `seek_ms_clamps_to_the_bar`; `place_falls_back_when_the_saved_rect_is_off_screen`. window (no window on screen): `opening_the_same_recording_twice_keeps_one_window`; `closing_posts_closed_once`; `line_heights_are_measured_once_per_width` (a counter of measurements); `the_live_view_drops_echo` (`transcript_view` of a running job with echoed `Me` segments). App: `copy_and_note_texts_come_from_the_doc` (pure helper choosing the text per request).
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; screenshots of every demo scene in light, dark and Terminal dark; keyboard walk-through recorded in the report; contrast numbers for text, secondary, both chips, the playing line, the focus ring, the primary button and the seek fill.
- [ ] **Step 5:** Commit `feat(transcript): the transcript window`.

### Task 12: The player and click-to-seek (most capable)

**Files:**
- Create: `src/transcript_win/player.rs`
- Modify: `src/transcript_win/{window.rs, mod.rs}`, `src/transcribe/mod.rs` (remove `#![allow(dead_code)]`; keep `#[cfg(debug_assertions)]` on what only `debug.rs` uses)

**Interfaces:**
- Consumes: `crate::mf::{mf, MfSession, created}` (Task 1), WASAPI render (`IMMDeviceEnumerator`, `IAudioClient`, `IAudioRenderClient`), `doc::current_line`, `layout::{seek_ms, follow_scroll}`.
- Produces:
  - `pub const WM_APP_PLAYER: u32 = WM_APP + 86;` posted to the window when playback ends (wparam 0) or fails (wparam 1).
  - `pub enum PlayerError { NoMediaFoundation, Open(String), Device(String) }`.
  - `pub struct Player` with `new(audio: PathBuf) -> Player` (no thread), `play_from(&mut self, ms: u64, notify: HWND) -> Result<(), PlayerError>` (stops a running thread, then starts `transcript-player`), `pause(&mut self)` (stops and joins the thread, keeps the position), `playing(&self) -> bool`, `position_ms(&self) -> u64`, `duration_ms(&self) -> Option<u64>`; `Drop` stops and joins.
  - Pure: `pub fn downmix(frames: &[f32], channels: usize, out: &mut Vec<f32>)` (every frame becomes two equal samples, the mean of its channels, so a mic-left / PC-right file plays in both ears); `pub fn played_ms(start_ms: u64, frames: u64, rate: u32) -> u64`.
- The thread: COM MTA and an `MfSession`; a source reader on the file, output PCM float at the file's rate; `SetCurrentPosition(ms * 10_000)`; a shared-mode render stream on the default console endpoint, 2 channels float at the file's rate with `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY`, event driven; position published through an `AtomicU64` (`played_ms` of frames written minus padding); ends at end of stream. One player at a time across every transcript window: starting one stops the others. Closing a window stops its player.
- The window: a click on a line (not a drag) plays from its `start_ms`; a click or drag on the bar seeks (`seek_ms`; while paused it only moves the position); Space toggles; the playing line is marked (`current_line`) and kept in view (`follow_scroll`) unless the wheel or keys moved the list in the last 3 s (own timer id 2, a one-shot per notch); a one-shot own timer id 1 every 200 ms only while playing refreshes the position and the line; `WM_APP_PLAYER` end puts the position back to 0 and shows play; failure shows `Status::PlayerError`.

- [ ] **Step 1: Tests:** `downmix_puts_both_channels_in_both_ears` (`[1.0, 0.0, 0.0, 1.0]` with 2 channels gives `[0.5, 0.5, 0.5, 0.5]`; mono `[0.2]` gives `[0.2, 0.2]`); `played_ms_counts_frames_from_the_start` (`played_ms(1_000, 48_000, 48_000) == 2_000`); `a_click_on_a_line_plays_from_its_start` (window logic with a fake player trait: the requested ms equals the line's `start_ms`); `a_manual_scroll_holds_following_for_three_seconds`; `the_play_timer_runs_only_while_playing` (`KillTimer` returns false when paused, as the lyrics card test does); `#[ignore] plays_a_generated_wav` (writes a 1 s stereo WAV under `target/test-tmp`, plays it on the real device; run by hand).
- [ ] **Step 2:** Fail. **Step 3:** Implement. Remove `#![allow(dead_code)]` from `src/transcribe/mod.rs` and fix what is left.
- [ ] **Step 4:** `cargo test --locked` green, `cargo build --locked` and `cargo build --release --locked` with 0 warnings. By hand: `--demo-transcript lines` with a real `.m4a` copied in (`DI_TRANSCRIPT_AUDIO=<path>`): click lines, drag the bar, Space, close while playing (sound stops, the `transcript-player` thread is gone in Process Explorer).
- [ ] **Step 5:** Commit `feat(transcript): play the recording and jump to a line`.

---

## Live checks for the controller (owner's PC, after the final review, before merging)

From the spec's "Done means". Use the release build of the branch; stop the installed Vale first (`Stop-Process -Name vale, dynamic-island`).

- [ ] `cargo test --locked` green and `cargo build --release --locked` with no warnings on the branch.
- [ ] Exe size: `vale.exe` of the branch against `main`'s, both built in folders of the same path length; note both numbers for [[exe-size]]. The anti-goal is "a few tens of KB": report the real figure even if it is more (track B's core alone adds code).
- [ ] Idle: with Transkrip on and nothing queued, after the 30 s settle, Vale's thread count in Process Explorer is what it is with Transkrip off; no `transcribe*` or `transcript-player` thread. After a job ends, the worker thread is gone (the engine DLL stays loaded: say whether ggml or Vulkan left threads behind).
- [ ] Settings → Fitur → Transkrip: switch on with nothing installed shows the offer with the GPU's model and its real size; Unduh shows progress; Batal stops it; Unduh again, quit Vale mid-download, start again: it resumes after the settle; Hapus frees the space (footer number drops); switch the radio to the other model; change Bahasa. Light, dark and Terminal surface.
- [ ] A short real recording with auto on: "Rekaman tersimpan", then "Bikin transkrip · <judul>", then "Transkrip siap · <judul> · klik untuk buka"; the click opens the window; the Rekam row shows the percent while it runs and Lihat after.
- [ ] Window: clicking a line plays from there in both ears; the playing line is marked and followed; Salin, Salin + prompt ringkasan (paste into a chat: the prompt then the lines), Jadikan catatan (the Notes page has it; 20 lines and the `.md` path for a long one), Buka .md, Buka folder; light and dark; keyboard only (Tab, Enter, Space, arrows, Esc).
- [ ] Auto off: the saved notice has the Transkrip chip and it queues the job; the Rekam row's Transkrip pill does the same.
- [ ] Start a recording while a transcript runs: the job pauses (row "Antre", window "Dijeda selama merekam") and finishes after Stop.
- [ ] Quit Vale mid-job, start it again: after the settle the job runs and finishes; no half `.md` beside the audio meanwhile.
- [ ] Right-click a done row: Transkrip ulang with each installed model replaces the `.md`, `.srt` and the sidecar's `"transcript"`.
- [ ] "Transkrip file lain…": the picker opens in the recording folder; an old `.m4a` without a transcript queues and opens; one with a transcript opens it.
- [ ] Echo: a call on speakers with echo cancellation on: few or no "Saya" lines repeating "Peserta"; a real reply over someone talking stays.

## Decisions this plan made where the spec left room

- GPU: DXGI hardware adapter (not software, not Microsoft Basic Render); the job always allows the GPU and the runner falls back to the CPU on a backend failure.
- Queue file: `%APPDATA%\Vale\transcribe\queue.json` (the store), running job first; a job restarts from the beginning after a hold or a restart.
- Settle: 30 s one-shot after start (`TIMER_TRANSCRIBE` 65), only when Transkrip is on; the store sweep, the queue load and the download resume happen then.
- Duplicate rule: the word-overlap rule of Task 3 (content words, 1 s slack, edit distance a third, share 0.5).
- Qwen3: the entries go, the family enum and the no-timestamp path stay; the sweep deletes Qwen `.gguf` files left in the store.
- Segments for the window come from the `.srt` (labels read only for two-channel recordings); while a job runs, from the runner's events.
- An empty result writes no `.md` / `.srt` (old ones removed) and records `"segments": 0` in the sidecar; the window says "Tidak ada ucapan yang terbaca".
- Settings live in a new island-owned `transcript` section of config.json, not inside `recording` (track A's).
- One download at a time; it covers the engine, the VAD and the model in one run (`Item::Setup`); Batal and switching Transkrip off discard the partial files, a quit keeps them for the resume.
- The Rekam row's "Buka" pill becomes the transcript pill; the row click still shows the file. Cancel and re-transcribe live in the row's right-click menu and the window; the window's Transkrip ulang opens the same menu.
- The disabled pill (Transkrip off) explains itself with a notice on click ("Transkrip belum nyala · Nyalakan di Pengaturan", which opens the card): the panel has no hover tooltips.
- The one-time setup offer's notice body is "<length> · Bikin transkrip?" with a "Pengaturan" chip.
- The window is a normal top-level window in the taskbar (like Settings), not a topmost tool window like the lyrics card; one per recording, one player at a time.
- Playback: Media Foundation source reader into a WASAPI shared render stream, both channels mixed into both ears; the player thread lives only while playing.
- The English summary prompt is a translation of the spec's Indonesian one, with "Me" and "Others".
