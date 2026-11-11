---
tags: [plan, recording, screen-recording]
date: 2026-11-11
spec: docs/superpowers/specs/2026-11-11-screen-recording-phase3-design.md
---

# Screen recording, basic (recording phase 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record a monitor, a window or a dragged area to an `.mp4` with the mic and the PC audio mixed into one mono track, started from a hotkey, the Rekam page or the tray through the screenshot picker and a 3-2-1 countdown, with Vale's own windows left out of the video, a screen view in the notch, a saved notice with a thumbnail, a "Layar" list on the Rekam page and transcripts of videos.

**Architecture:** Track C's core (`src/screenrec/`: capture, compositor, encoder, cursor log, zoom, `ScreenRecorder`) is merged with `main` and moved onto `crate::mf`, a crash-safe `.mp4.part` and a pause-aware clock. The audio comes from phase 1's recorder in a new mix mode (`Recorder::start_mix`: same capture, resampler, timeline, mute and meters, a soft-limited mono mix pushed into track C's `AudioInput`). Pure rules first (`recorder::mix`, `screenrec::session`, `screenrec::files`, `capture::rec_pick`); then the island: `src/app_screenrec.rs` (a child of `app`, like `app_record.rs`) owns the picker, countdown, start, stop, pause, discard, exclusion from capture, notices and the video list; the notch reuses `View::RecordCompact` with `RecStatus.kind = Screen`.

**Tech Stack:** Rust 2024, `windows` 0.62 (Win32, WinRT Graphics Capture, Direct3D 11, Direct2D, WASAPI), Media Foundation loaded at run time (`crate::mf`), `serde_json`, `chrono`. No new crate.

**Spec:** `docs/superpowers/specs/2026-11-11-screen-recording-phase3-design.md` (builds on `docs/superpowers/specs/2026-11-07-recording-design.md`; mockups `docs/design/rekaman.html` sections 8, 10, 12). Read both specs and the memory notes they link ([[recording-audio]], [[recording-controls]], [[notch-rec-mute]], [[transcript]], [[controlled-folder-access]], [[media-foundation-dynamic-load]], [[wasapi-capture-timestamps]], [[screenshot-snap]], [[hotkeys]], [[bata-idle-power-budget]], [[exe-size]], [[menu-pop]], [[light-accent-contrast]], [[test-spawns-no-window]]) before a task.

**Branch:** `screen-rec`, worktree `.claude/worktrees/screen-rec`, made from `worktree-agent-a2b34453550627f70` with `main` merged in (Task 0). This plan never merges into `main` and never pushes: that is the controller's step after the final review.

**Model tiers** (for the controller): *cheap* = mechanical, *standard* = integration, *most capable* = design-heavy. Each task names one.

## Global Constraints

- Windows 11 x64, Rust 2024, one `vale.exe`, no new crate. Windows Graphics Capture, D3D11, Media Foundation loaded only through `crate::mf` (`LoadLibraryExW`, never imported: [[media-foundation-dynamic-load]]).
- Ids from the 11-07 contract: hotkey **10** = `HotkeyId::RecordScreen` (default `Ctrl+Alt+Shift+R`); hotkey 11 stays reserved for phase 4 and is not registered. Timer **70** = `TIMER_SCREEN_COUNTDOWN`; 71-74 stay free. `WM_APP + 90` = `WM_APP_SCREEN_PICK` (picker to island), `WM_APP + 91` = `WM_APP_SCREEN_WATCH` (window watch to island); 92-94 stay free. The countdown's Esc is a raw `RegisterHotKey` id **13** (`HOTKEY_COUNTDOWN_ESC`, not a Settings row), registered only for the 3 s of the countdown.
- Idle cost zero ([[bata-idle-power-budget]]): no thread, capture, hook, timer or poll unless the picker is up, the countdown runs or a screen recording runs. Threads of a recording: `vale-screenrec`, `rec-capture`, `rec-mix`, and the short-lived `screen-start` / `screen-stop`.
- One recording at a time: starting audio while a screen recording is picked, counting down, starting, running or stopping (or the other way round) does nothing but the notice "Rekaman lain sedang jalan".
- Files: `<video folder>\YYYY-MM-DD HHmm <title>.mp4` (title = the recorded window's title, else empty), written as `<name>.mp4.part` and renamed at stop, then `<name>.vale.json` (`"kind": "screen"`, `"channels": ["mix"]` or `[]`, `"cursor_log"`) and `<name>.cursor.jsonl`. A clash with any of the four adds ` (2)`. Video folder `Videos\Vale` (config `recording.video_folder`), fallback `%USERPROFILE%\Vale\Video` with the default moving there ([[controlled-folder-access]]).
- Video: 30 fps default, 60 in Settings (`recording.video_fps`), the captured area at its pixel size (`plan::output_size`), H.264 hardware first then software, `plan::bitrate`. Fragmented MP4 first, plain MP4 as the fallback (like phase 1's `Container`). Audio: one **mono** AAC track (96 kbit/s), mic and PC audio summed through a soft limiter; no track when both toggles are off. Click ripples off (`click_ripples: false`), cursor drawn.
- `WDA_EXCLUDEFROMCAPTURE` on every top-level window of Vale's process only while a screen recording runs (set before the capture starts, kept on new windows by a WinEvent hook, restored after the recording thread ended).
- Every UI string goes through `src/i18n.rs` with Indonesian and English. Indonesian strings hold no English word (`no_key_leaks_the_other_language`, `no_formatted_text_leaks_the_other_language`; add every new formatted text to the latter). No em dash or en dash in any UI string; the separator is ` · ` (U+00B7). The copy in this plan is fixed; when a leak test rejects an Indonesian word, rephrase it in Indonesian and say so in the task report. The antislop copywriting pass may propose changes, reported to the controller, not applied.
- UI and copy tasks (8, 9, 10, 11, 12, 13, 14) start by loading, with the Skill tool, `antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-copywriting` and `antislop:antislop-human` (mode "during"). Every colour pair drawn is checked with antislop-human's `check_contrast` tool in light, warm dark and Terminal dark (`theme::window_for`; island surfaces for notch work), numbers in the task report. Accent fills that carry text on a light window use `primary_fill` / `segment_fill` ([[light-accent-contrast]]). Run each loaded skill's checklist before the task's last commit.
- Tests open no window on screen and spawn no console ([[test-spawns-no-window]]); test files go under `target/test-tmp/`.
- Every task ends with `cargo test --locked` green and `cargo build --locked` with 0 warnings, then a commit on `screen-rec`.

## Review Focus

1. **Pause and resume**: the mix writer stays about 200 ms behind, so the audio of the last moment before a pause reaches the screen recorder after the pause began. A person expects no word cut at a pause. The screen recorder's clock keeps every pause span and maps any QPC outside them (Task 2 `a_qpc_from_before_an_open_pause_still_maps`, `the_timeline_cuts_each_pause_once`).
2. **A one-sided passage** (only the narrator talks, the PC is silent, or the reverse) must keep its level, and two loud sources must not clip (Task 3 `a_one_sided_passage_keeps_its_level`, `soft_limit_bends_above_the_knee_and_never_clips`).
3. **The hotkey, the tray or the page pressed at the wrong moment**: during the picker, the countdown, a start or a stop, or for the other kind of recording. Nothing doubles, the countdown cancels, the other kind says "Rekaman lain sedang jalan" (Task 12 `the_toggle_does_what_the_phase_says`, `another_recording_blocks_the_start`).
4. **A Vale window that appears mid-recording** (Settings opened, a new sticky note, the transcript window, the device dropdown, the lyrics card) must stay out of the video (Task 5 `only_top_level_windows_of_this_process_count`, `a_window_shown_while_excluding_is_excluded_too`).
5. **Several monitors**: an area dragged across two monitors records only what the overlay showed, a monitor unplugged or a window closed mid-recording saves what was recorded with a notice saying why (Task 11 `an_area_is_kept_on_the_monitor_it_started_on`; Task 12 `the_saved_notice_names_the_length_and_why_it_ended`).

---

### Task 0: Branch, worktree, merge `main` (standard)

**Files:** conflicts expected in `src/main.rs`, `Cargo.toml`.

**Interfaces:**
- Produces: branch `screen-rec` holding track C's 4 commits plus `main`; every later task starts here.

- [ ] **Step 1:** From the main checkout (never switch its branch, never stash or reset, leave untracked `docs/design/` files alone): `git worktree add .claude/worktrees/screen-rec -b screen-rec worktree-agent-a2b34453550627f70`.
- [ ] **Step 2:** In the worktree: `git merge main`. Resolve: `main.rs` keeps every `mod` line (`mod screenrec;` beside `mod screenshot;`) and both sides' debug flags (`--screenrec-test` next to `--settings`); `Cargo.toml` keeps one `Win32_Graphics_Dxgi` and one `Win32_Media_MediaFoundation` and adds track C's `Win32_Graphics_Direct3D`, `Win32_Graphics_Direct3D11`, `Win32_System_Performance`, `Win32_System_WinRT_Direct3D11`, `Win32_System_WinRT_Graphics_Capture`, `Graphics`, `Graphics_Capture`, `Graphics_DirectX`, `Graphics_DirectX_Direct3D11`. Track C adds no crate: `Cargo.lock` is `main`'s (`git diff main -- Cargo.lock` is empty).
- [ ] **Step 3:** `cargo test --locked`: all pass. `cargo build --locked`: 0 warnings. `cargo run -- --screenrec-test 3` writes a playable `.mp4` into `%TEMP%\vale-screenrec\` and its own check prints success.
- [ ] **Step 4:** Commit the merge: `merge: main into the screen recording core`.

### Task 1: One Media Foundation loader, one QPC clock (cheap)

**Files:**
- Delete: `src/screenrec/mf.rs`
- Modify: `src/mf.rs`, `src/screenrec/{mod.rs, encode.rs, debug.rs}`, `src/recorder/capture.rs` (drop the `#[allow(dead_code)]` on `qpc_hns`)

**Interfaces:**
- Consumes: `crate::mf::{mf, MfSession, created}`, `crate::recorder::capture::qpc_hns`.
- Produces: `crate::mf::Mf` gains `pub create_dxgi_device_manager` (`MFCreateDXGIDeviceManager`, mfplat) and `pub create_video_sample_allocator_ex` (`MFCreateVideoSampleAllocatorEx`, mfplat) with their documented signatures; `screenrec::Recording` holds an `MfSession` (last field, dropped last) and maps `MfSession::start() == None` to `Error::NoMediaFoundation`; `encode.rs` takes `&'static crate::mf::Mf` and builds its objects with `created`. `screenrec::qpc_hns` is gone: everything uses `recorder::capture::qpc_hns`.

- [ ] **Step 1: Test** in `src/mf.rs`: `fn the_video_entry_points_load()` asserts `mf().is_some()` (the two new pointers resolve, or `mf()` would be `None`).
- [ ] **Step 2:** `cargo test --locked mf::`: fails to compile (no field).
- [ ] **Step 3:** Add the two pointers; port `encode.rs`, `mod.rs` and `debug.rs` to `crate::mf`; delete `screenrec/mf.rs`. No behaviour change; track C's tests and `--screenrec-test 3` are the guard.
- [ ] **Step 4:** `cargo test --locked` green, 0 warnings; `rg "LoadLibraryExW\(w!\(\"mf" src` finds only `src/mf.rs`; `--screenrec-test 3` still passes its check.
- [ ] **Step 5:** Commit `refactor(screenrec): one Media Foundation loader and one QPC clock`.

### Task 2: Crash-safe MP4, the file names, pause spans (standard)

**Files:**
- Modify: `src/screenrec/{files.rs, plan.rs, encode.rs, mod.rs}`

**Interfaces:**
- Consumes: `crate::recorder::naming::file_stem`, `crate::recorder::Container`.
- Produces:
  - `files::files_of(dir: &Path, stem: &str) -> [PathBuf; 4]` = `<stem>.mp4`, `<stem>.mp4.part`, `<stem>.vale.json`, `<stem>.cursor.jsonl`; `files::unique_stem(dir: &Path, stem: &str, exists: impl Fn(&Path) -> bool) -> String` (` (2)`, ` (3)`... while any of the four exists); `files::part_of(video: &Path) -> PathBuf` (`<video>.part`); `files::existing_files(video: &Path) -> Vec<PathBuf>` (those of the four that exist, the `.part` included). `files::file_stem` and `files::unique_path` are removed (the stem is `recorder::naming::file_stem`).
  - `plan::Timeline` keeps every ended pause span and the open one: `media_time(qpc) -> Option<i64>` is `None` before the start, at or after an open pause's start, and inside an ended span; else the time with the spans ended before `qpc` cut out. `pause`, `resume`, `is_paused` unchanged.
  - `Encoder::create(.., container: Container)` and `Encoder::container(&self) -> Container`; `ScreenRecorder::start(target, path, options, on_end)` takes the final `.mp4` path, writes `part_of(path)` trying `Container::Fragmented` then `Container::Mpeg4`, and at the end renames the part to `path` before writing the sidecar; a failed rename keeps the `.part` and returns `Error::Io`. `Summary.container: Container`. The recording loop drains the audio feed while paused too and keeps every chunk whose `media_time` is `Some` (the `resumed_at` rule goes).

- [ ] **Step 1: Tests:**
  - `files`: `the_files_of_a_video_are_four` (`files_of(Path::new(r"C:\v"), "2026-11-11 1430")` gives `C:\v\2026-11-11 1430.mp4`, `.mp4.part`, `.vale.json`, `.cursor.jsonl` in that order); `a_clash_with_any_file_takes_the_next_number` (`exists` true only for `x.mp4.part` gives `"x (2)"`; for `x.cursor.jsonl` too); `existing_files_lists_only_what_is_there` (temp dir with `.mp4` and sidecar only: those two; an unrelated `x.txt` never listed); `the_sidecar_names_the_mix_channel` (`sidecar_json(None, &info)` with `channels: vec!["mix".into()]`, `cursor_log: Some("a.cursor.jsonl")` parses to `"kind": "screen"`, `"channels": ["mix"]`, `"cursor_log": "a.cursor.jsonl"`, and an existing `"transcript"` key is kept on rewrite). Replace the tests of the removed `file_stem` / `unique_path`.
  - `plan`: `the_timeline_cuts_each_pause_once` (start 0, freq 10_000_000: pause at 10 s, resume at 15 s; `media_time(12 s) == None`, `media_time(20 s) == Some(150_000_000)`, `media_time(8 s) == Some(80_000_000)` after the resume); `a_qpc_from_before_an_open_pause_still_maps` (paused at 10 s: `media_time(9.9 s) == Some(99_000_000)`, `media_time(10 s) == None`); `two_pauses_add_up` (pauses 10-15 s and 20-22 s: `media_time(30 s) == Some(230_000_000)`, `media_time(18 s) == Some(130_000_000)`).
- [ ] **Step 2:** Run; they fail.
- [ ] **Step 3:** Implement. The rename and the container order run on the recording thread in `Recording::run`'s finish.
- [ ] **Step 4:** `cargo test --locked` green, 0 warnings. `cargo run -- --screenrec-test 3`: the check passes, the folder holds `.mp4`, `.vale.json`, `.cursor.jsonl` and no `.part`; the debug output names the container (`Fragmented` expected).
- [ ] **Step 5:** Commit `feat(screenrec): fragmented MP4 written as .part, pause spans in the clock`.

### Task 3: The recorder's mix mode (standard)

**Files:**
- Create: `src/recorder/mix.rs`
- Modify: `src/recorder/mod.rs`, `src/screenrec/audio.rs`

**Interfaces:**
- Consumes: `Lanes`, `Placer`, `Sink`, `Muted`, `SharedLevels`, `capture::Capture`, the `start_with` fake-feed pattern of the phase 1 tests; `screenrec::audio::{AudioInput, SAMPLE_RATE}`.
- Produces:
  - `mix::KNEE: f32 = 0.7`; `mix::soft_limit(x: f32) -> f32`: `x` for `|x| <= KNEE`, else `sign(x) * (KNEE + (1 - KNEE) * tanh((|x| - KNEE) / (1 - KNEE)))`; `mix::mix_mono(frames: &[f32], channels: usize) -> Vec<f32>` (each frame's channels **summed**, then `soft_limit`).
  - `mix::MixClock { origin: i64, raw: u64 }` with `new(origin_hns: i64) -> MixClock`, `stamp(&self) -> i64` (`origin + raw * 10_000_000 / 48_000`, i128 maths), `advance(&mut self, frames: u64)`, `skip(&mut self, frames: u64)`.
  - `pub trait MixSink: Send { fn mix(&mut self, mono: &[f32], qpc_hns: i64); }`; `pub struct MixOptions { pub sources: Sources, pub devices: [Option<String>; 2], pub echo_cancel: bool }`.
  - `Recorder::start_mix(opts: MixOptions, sink: Box<dyn MixSink>, events: Box<dyn Fn(RecEvent) + Send>) -> Result<Recorder, RecError>`: the same capture, lanes, placer, mute and meters as `start`, but the writer thread is `rec-mix`: every 50 ms it takes the timeline up to 200 ms before now (a second at a time), mixes it with `mix_mono` and calls `sink.mix(mono, clock.stamp())` then `clock.advance(n)`; a tick that finds a sleep gap (`Placer::bridge` now returns `bool`, true when it cut) calls `clock.skip(now - last)` and pushes nothing for it. No file, no sidecar, no `Saved` and no `Silent`; `SourceLost`, `MicDenied`, `DeviceSwitched` and `NoInput` are reported as in phase 1. On a mix recorder `pause` / `resume` do nothing (the screen recorder drops paused audio), `path()` is empty, `stop()` returns `Ok(RecResult { path: PathBuf::new(), duration })`.
  - `screenrec::audio::Forward(pub Arc<dyn AudioInput>)` implementing `MixSink`: `push_audio(mono, 1, SAMPLE_RATE, qpc_hns)`.

- [ ] **Step 1: Tests:**
  - `mix`: `soft_limit_leaves_the_normal_range_alone` (`soft_limit(0.5) == 0.5`, `soft_limit(-0.7) == -0.7`, `soft_limit(0.0) == 0.0`); `soft_limit_bends_above_the_knee_and_never_clips` (`(soft_limit(1.0) - 0.9285).abs() < 1e-3`, `soft_limit(2.0) < 1.0`, `soft_limit(8.0) <= 1.0`, strictly rising over -4.0..4.0 in steps of 0.01, `soft_limit(-x) == -soft_limit(x)`); `a_one_sided_passage_keeps_its_level` (`mix_mono(&[0.5, 0.0, 0.5, 0.0], 2) == [0.5, 0.5]`, `mix_mono(&[0.0, 0.3], 2) == [0.3]`); `both_sides_add_up_under_the_limit` (`mix_mono(&[0.5, 0.5], 2) == [soft_limit(1.0)]`); `mono_passes_through` (`mix_mono(&[0.2, -0.4], 1) == [0.2, -0.4]`); `the_mix_clock_counts_taken_frames_and_skipped_sleep` (`new(1_000)`, `advance(48_000)`: `stamp() == 10_001_000`; `skip(96_000)`: `stamp() == 30_001_000`).
  - `recorder` (fake feed): `a_mix_recording_pushes_mono_stamped_from_its_origin` (mic packets of 0.5 at 48 kHz, PC audio silent: every sample after the first 10 ms is within 0.01 of 0.5, the first stamp is the origin, each next stamp is the previous plus its frames in 100 ns); `a_muted_source_is_left_out_of_the_mix` (mic 0.5 muted, PC 0.25: the mix reads 0.25); `a_mix_recording_writes_no_file` (the temp dir stays empty after start and stop); `a_mix_recorder_ignores_pause` (pause, push, resume: the samples pushed during the "pause" still reach the sink).
  - `screenrec::audio`: `forward_pushes_mono_at_48k` (a fake `AudioInput` records `(len, channels, rate, qpc)`: `(480, 1, 48_000, 7)`).
- [ ] **Step 2:** Run; they fail.
- [ ] **Step 3:** Implement. `write_main` and the new `mix_main` share the tick and the take; keep phase 1's behaviour byte for byte (its tests are the guard).
- [ ] **Step 4:** `cargo test --locked` green, 0 warnings.
- [ ] **Step 5:** Commit `feat(recorder): a mono mix mode that feeds the screen recorder`.

### Task 4: Video settings and the session rules (cheap)

**Files:**
- Create: `src/screenrec/session.rs`
- Modify: `src/recorder/prefs.rs`, `src/screenrec/mod.rs`

**Interfaces:**
- Consumes: `screenrec::Options`, `crate::model::RecSources`.
- Produces:
  - `RecordingPrefs.video_folder: Option<PathBuf>` (key `"video_folder"`, absolute paths only), `RecordingPrefs.video_fps: u32` (key `"video_fps"`, 60 reads 60, anything else 30), `RecordingPrefs::video_dir(&self) -> PathBuf`, `prefs::default_video_folder() -> PathBuf` (`FOLDERID_Videos\Vale`), `prefs::video_fallback_folder() -> Option<PathBuf>` (`FOLDERID_Profile\Vale\Video`). `Serialize` writes both keys.
  - `session::MIX_CHANNEL: &str = "mix"`; `session::options_for(fps: u32, sources: RecSources, title: &str) -> Options` (fps 60 or 30; `audio_channels: Some(1)` and `channel_names: ["mix"]` when a source is on, else `None` and `[]`; `click_ripples: false`, `draw_cursor: true`, `prefer_hardware: true`, `side_files: true`, `max_height: None`, `app: None`, `title`).
  - `session::PauseWhy { Away, Minimised }`; `session::ScreenPause` with `paused(&self) -> bool`, `set(&mut self, why: PauseWhy, on: bool) -> Option<bool>` (the new paused state when it changes), `user_toggle(&mut self) -> bool` (paused: every flag clears, it resumes; running: the user's pause; returns the new state). `Away` is a sleep or a lock.
  - `session::ScreenClock { started: Instant, paused_total: Duration, paused_since: Option<Instant> }` with `new(now)`, `pause(now)`, `resume(now)` (idempotent).

- [ ] **Step 1: Tests:** `prefs`: `video_defaults` (`video_folder == None`, `video_fps == 30`); `a_bad_video_setting_reads_as_its_default` (`{"video_fps": 45}`, `{"video_fps": "60"}` give 30, `{"video_folder": "rel\\x"}` gives `None`); `video_prefs_round_trip` (60 and `C:\v`); `an_old_recording_section_loads_with_video_defaults`; `the_video_fallback_sits_in_the_profile` (`video_fallback_folder().unwrap().ends_with(r"Vale\Video")`, `default_video_folder().ends_with("Vale")`). `session`: `options_follow_the_prefs` (`options_for(60, mic only, "Figma")`: fps 60, `audio_channels == Some(1)`, `channel_names == ["mix"]`, `!click_ripples`, title `"Figma"`; `options_for(45, neither, "")`: fps 30, `audio_channels == None`, `channel_names` empty); `a_minimised_window_pauses_and_its_restore_resumes` (`set(Minimised, true) == Some(true)`, `set(Minimised, false) == Some(false)`); `a_user_pause_survives_restore_and_wake`; `sleep_and_minimise_both_must_end_before_resume` (`set(Away, true)`, `set(Minimised, true) == None`, `set(Away, false) == None`, `set(Minimised, false) == Some(false)`); `the_user_resumes_over_an_automatic_pause` (minimised, `user_toggle() == false`, then `set(Minimised, false) == None`); `the_clock_leaves_pauses_out`.
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings.
- [ ] **Step 5:** Commit `feat(screenrec): video settings, options and pause rules`.

### Task 5: Out of the video: exclusion and the window watch (standard)

**Files:**
- Create: `src/screenrec/exclude.rs`, `src/screenrec/watch.rs`
- Modify: `src/screenrec/mod.rs`, `Cargo.toml` (only if `SetWinEventHook` needs a feature not yet on: `Win32_UI_Accessibility`)

**Interfaces:**
- Consumes: `capture::exclude_from_capture`.
- Produces:
  - `exclude::counts(window_pid: u32, own_pid: u32, top_level: bool, id_object: i32, id_child: i32) -> bool` (own process, top level, `OBJID_WINDOW` (0), `CHILDID_SELF` (0)).
  - `exclude::Excluder` with `start() -> Excluder`: every top-level window of this process (`EnumWindows` + pid) gets `WDA_EXCLUDEFROMCAPTURE` with its previous affinity remembered, and one `SetWinEventHook(EVENT_OBJECT_SHOW, ..., own pid, 0, WINEVENT_OUTOFCONTEXT)` excludes each window shown later (no `WINEVENT_SKIPOWNTHREAD` / `SKIPOWNPROCESS`: Vale's windows live on the island's thread). `Drop` unhooks and restores every remembered affinity on windows that still exist. Lives on the island's thread.
  - `watch::watch_event(event: u32, hwnd: isize, target: isize) -> Option<bool>` (`EVENT_SYSTEM_MINIMIZESTART` 0x16 gives `Some(true)`, `EVENT_SYSTEM_MINIMIZEEND` 0x17 `Some(false)`, any other window or event `None`).
  - `watch::WindowWatch` with `start(target: isize, notify: HWND, msg: u32) -> Option<WindowWatch>`: one out-of-context hook on the target's process for those two events, posting `msg` with `wparam` 1 (minimised) or 0 (restored). `Drop` unhooks.

- [ ] **Step 1: Tests:** `only_top_level_windows_of_this_process_count` (`counts(1, 1, true, 0, 0)`; not another pid, not a child, not `id_object` -4); `the_watch_reports_only_its_window` (as above, `watch_event(0x16, 5, 6) == None`); `the_excluder_marks_and_restores_an_own_window` (a hidden `WS_POPUP` of this process: inside `Excluder::start()` its `GetWindowDisplayAffinity` is `WDA_EXCLUDEFROMCAPTURE` (0x11), after the drop it is 0 again; if Windows refuses the affinity on a window never shown, mark it `#[ignore]` with that reason and say so in the report); `a_window_shown_while_excluding_is_excluded_too` (`#[ignore]`, run by hand: a second hidden window shown with `SW_SHOWNA` off-screen after `start`, messages pumped for 200 ms, affinity 0x11).
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; run both ignored tests by hand once and note the result.
- [ ] **Step 5:** Commit `feat(screenrec): keep Vale out of the video, watch the recorded window`.

### Task 6: The thumbnail of a saved video (standard)

**Files:**
- Create: `src/screenrec/thumb.rs`

**Interfaces:**
- Consumes: `crate::mf::{mf, MfSession, created}`, `crate::clipboard::image::Thumb`.
- Produces: `thumb::THUMB_MAX: u32 = 120`; `thumb::thumb_at(duration_ms: u64) -> u64` (`min(1_000, duration_ms / 2)`); `thumb::fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32)` (aspect kept, rounded, at least 1, never larger than the source); `thumb::scale(bgra: &[u8], w: u32, h: u32, stride: usize, out_w: u32, out_h: u32) -> Vec<u8>` (area average, alpha set to 255); `thumb::grab(video: &Path, at_ms: u64) -> Option<Thumb>` (source reader with `MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING`, only the first video stream selected, output `MFVideoFormat_RGB32`, seek to `at_ms`, the first sample, the default stride's sign honoured, fitted into `THUMB_MAX` x `THUMB_MAX`; `None` on any failure).

- [ ] **Step 1: Tests:** `thumb_time_is_one_second_or_the_middle` (`thumb_at(60_000) == 1_000`, `thumb_at(1_200) == 600`, `thumb_at(0) == 0`); `fit_keeps_the_aspect_and_never_grows` (`fit(1920, 1080, 120, 120) == (120, 68)`, `fit(1080, 1920, 120, 120) == (68, 120)`, `fit(64, 48, 120, 120) == (64, 48)`, `fit(4000, 2, 120, 120) == (120, 1)`); `scale_makes_an_opaque_picture_of_that_size` (4 x 2 in, 2 x 1 out: 8 bytes, every 4th byte 255, a 2 x 2 block of 0 and 200 averages to 100); `#[ignore] grabs_a_frame_from_a_real_video` (`DI_THUMB_VIDEO=<path>`: `Some`, width or height 120).
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; the ignored test passes on a `--screenrec-test` video.
- [ ] **Step 5:** Commit `feat(screenrec): a thumbnail from the saved video`.

### Task 7: Transcripts of videos (cheap)

**Files:**
- Modify: `src/transcribe/{output.rs, decode.rs, runner.rs}`, `src/ui/settings.rs` (`pick_audio` filter), `src/i18n.rs` (`TrFilter`)

**Interfaces:**
- Consumes: `output::speakers`, `Decoder`.
- Produces: `output::channel_plan(audio: &Path, channels: usize, names: Option<&[String]>) -> (bool /* downmix */, Vec<Option<Speaker>>)`: an `.mp4` (any case) whose sidecar names no channels, with 2 or more channels, is downmixed to one unlabelled channel; everything else is `(false, speakers(channels, names))`. `decode::mix_planes(planes: &[Vec<f32>]) -> Vec<f32>` (the mean per sample); `Decoder::downmix(&mut self)` (from then `channels() == 1` and `next_block` gives the mean). The runner uses `channel_plan` before building scorers. The file dialog filter is `"*.m4a;*.wav;*.mp4"` (`pub const RECORDING_FILTER`), `TrFilter` "Rekaman (*.m4a, *.wav, *.mp4)" / "Recordings (*.m4a, *.wav, *.mp4)".

- [ ] **Step 1: Tests:** `a_video_without_a_sidecar_is_downmixed_unlabelled` (`channel_plan(Path::new("a.mp4"), 2, None) == (true, vec![None])`, same for `"A.MP4"`); `a_vale_video_reads_its_mix_channel` (`channel_plan("a.mp4", 1, Some(&["mix".into()])) == (false, vec![None])`); `an_m4a_keeps_its_two_labels` (`channel_plan("a.m4a", 2, None) == (false, vec![Some(Me), Some(Others)])`); `downmix_averages_the_channels` (`mix_planes(&[vec![1.0, 0.0], vec![0.0, 1.0]]) == vec![0.5, 0.5]`); `the_file_filter_takes_videos` (`RECORDING_FILTER == "*.m4a;*.wav;*.mp4"`).
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; `cargo run -- --transcribe <a --screenrec-test video>` (debug CLI) writes `<stem>.srt` with no "Saya"/"Peserta" labels.
- [ ] **Step 5:** Commit `feat(transcribe): transcripts of videos, unlabelled`.

### Task 8: Model, state and notices (standard; copy: load antislop first)

**Files:**
- Create: `src/app_screenrec.rs` (`#[path = "app_screenrec.rs"] mod screen_page;` in `app.rs`) holding for now `ScreenPhase`, `ScreenState` (below) and empty `App` methods `screen_toggle`, `screen_discard`, `screen_open_folder`, `screen_pick_folder`, each with a one-line doc; Tasks 9 and 12 fill them.
- Modify: `src/model.rs`, `src/state.rs`, `src/ui/geometry.rs`, `src/ui/robot.rs`, `src/ui/demo.rs`, `src/app.rs` (`perform` arms, field `screen: ScreenState`), `src/app_record.rs` (`saved_notice` passes `thumb: None`; `folder_blocked_notice(lang, dir: PathBuf, video: bool)`, phase 1 passes `false`), `src/app_transcribe.rs` (`rec_pick_file(kind: RecKind)`, the kind used from Task 13; `rec_list()` where it read `rec_recent()`), `src/i18n.rs`

**Interfaces:**
- Produces in `app_screenrec.rs`: `pub(super) enum ScreenPhase { Idle, Picking, Countdown(u8), Starting, Recording, Stopping }` (Default `Idle`) with `busy(self) -> bool` (not `Idle`), `engaged(self) -> bool` (`Countdown`, `Starting`, `Recording`, `Stopping`: the transcript hold), `recording(self) -> bool`; `#[derive(Default)] pub(super) struct ScreenState { phase: ScreenPhase, thumb: Option<(u64, Thumb)>, .. }` with `busy()`, `engaged()`, `recording()` (Task 12 adds the rest).
- Produces in `model.rs`:
  - `pub enum RecKind { Audio, Screen }` (Default `Audio`); `RecStatus.kind: RecKind`.
  - `pub enum ScreenNote { Started, Ended, Stopped, Audio(crate::recorder::RecEvent) }`; `Event::Screen(u64, ScreenNote)` (the reducer ignores it; `App` counts it as a background event like `Event::Rec`).
  - `Event::ScreenCountdown(Option<u8>)`, `Event::VideoRecent(Vec<RecEntry>)`, `Event::VideoMarks(Vec<RecEntry>)`.
  - `View::ScreenCountdown(u8)`.
  - `NoticeKind::RecSaved { path, offer, thumb: Option<u64> }` (every existing pattern gains `..` or `thumb`); `NoticeKind::RecFolderBlocked { dir, video: bool }`.
  - `Hit::RecTab(RecKind)`, `Hit::ScreenDiscard`.
  - `Effect::ScreenToggle`, `Effect::ScreenDiscard`, `Effect::VideoOpenFolder`, `Effect::VideoPickFolder`, and `Effect::RecPickFile(RecKind)` (was unit).
- `state.rs` (`IslandState`): fields `rec_tab: RecKind`, `rec_videos: Vec<RecEntry>`, `screen_countdown: Option<u8>`, `screen_discard_armed: bool`; getters `rec_tab()`, `rec_list() -> &[RecEntry]` (the tab's list), `screen_countdown()`, `screen_discard_armed()`.
  - `base_view`: after the fullscreen check and before the panel, `screen_countdown` Some gives `View::ScreenCountdown(n)`; while it shows, hover and clicks open and latch nothing (`panel_held` false).
  - `Event::RecStatus(Some(s))` sets `rec_tab = s.kind`; `Hit::RecTab(k)` on the Rekam page only while `rec` is `None`.
  - Rekam page hits by tab: `RecToggle` gives `Effect::RecToggle` (Audio) or `collapse_for(Effect::ScreenToggle)` (Screen, idle) / `Effect::ScreenToggle` (Screen, recording); `RecFolder` gives `RecOpenFolder` or `collapse_for(VideoOpenFolder)`; `RecPickFile` gives `collapse_for(RecPickFile(tab))`; `RecOpen(i)` and `RecTranscript(i)` act on `rec_list()`; `ScreenDiscard` while a screen recording shows: the first click arms (no effect), the second gives `Effect::ScreenDiscard`; leaving the page, closing the panel or any `RecStatus` change disarms.
  - `RecFolderBlocked { video: true }` gives `Effect::VideoPickFolder`, `false` `RecPickFolder`.
- i18n (Id / En), exact: `RecBusyTitle` "Rekaman lain sedang jalan" / "Another recording is running"; `RecBusyBody` "Hentikan dulu, lalu mulai lagi" / "Stop it first, then start again"; `ScreenSavedTitle` "Video tersimpan" / "Video saved"; `ScreenWindowClosedTitle` "Jendela ditutup, video tersimpan" / "Window closed, video saved"; `ScreenDisplayGoneTitle` "Layar terputus, video tersimpan" / "Display disconnected, video saved"; `ScreenDiscardedTitle` "Video dibuang" / "Video discarded"; `ScreenFailedTitle` "Rekam layar gagal" / "Screen recording failed"; `ScreenWhyUnsupported` "Windows ini tidak bisa merekam layar" / "This Windows cannot record the screen"; `ScreenWhyGone` "Jendela atau layarnya sudah tidak ada" / "The window or screen is gone"; `ScreenWhyMinimized` "Jendelanya sedang diminimalkan. Buka dulu, lalu coba lagi" / "The window is minimised. Restore it, then try again"; `ScreenWhyEncoder` "Pengode video tidak tersedia di PC ini" / "No video encoder is available on this PC"; `ScreenWhyGpu` "Kartu grafis tidak bisa dipakai untuk merekam" / "The graphics card cannot be used to record"; `ScreenWhyWrite` "File video tidak bisa ditulis" / "The video file cannot be written"; `ScreenWhyOther` "Ada yang tidak beres saat merekam layar" / "Something went wrong while recording the screen"; `ScreenLabel` "Layar" / "Screen"; `ScreenCountdownEsc` "Esc untuk batal" / "Esc to cancel"; `RecTabAudio` "Audio" / "Audio"; `RecTabScreen` "Layar" / "Screen"; `ScreenIdleHint` "Pilih layar, jendela atau area sesudah ini" / "You pick a screen, window or area next"; `ScreenDiscard` "Buang" / "Discard"; `ScreenDiscardSure` "Yakin buang?" / "Discard it?"; `ScreenNoVideos` "Belum ada video" / "No videos yet".
- Demo scenes in `ui/demo.rs`: `screen-rec`, `screen-rec-paused`, `screen-countdown`, `video-saved` (made-up data; the thumbnail is a generated gradient).

- [ ] **Step 1: Tests** (`app_screenrec`): `the_hold_follows_the_screen_phase` (`engaged`, `busy`, `recording` for all six phases). (state): `a_screen_recording_shows_the_rec_view_with_its_kind`; `the_countdown_shows_over_the_rec_view_and_the_panel` (`ScreenCountdown(Some(3))` gives `View::ScreenCountdown(3)` with the panel open too; `None` gives the view before); `hover_and_clicks_on_the_countdown_open_nothing`; `the_tab_follows_the_running_recording_and_is_locked_meanwhile`; `the_page_buttons_act_by_tab` (a table over both tabs for `RecToggle`, `RecFolder`, `RecPickFile`); `rows_act_on_the_shown_list` (`VideoRecent` with one video, Screen tab: `RecOpen(0)` gives `RecReveal(video)`, `RecTranscript(0)` on `RowTranscript::None` gives `TranscribeQueue(video)`); `discard_takes_two_clicks`; `leaving_the_page_disarms_discard`; `the_saved_video_notice_reveals_and_offers_like_audio` (`RecSaved { thumb: Some(7), offer: Transcribe }`: body click `RevealFile`, chip `TranscribeQueue`); `a_blocked_video_folder_opens_the_video_picker`. Existing `RecSaved` / `RecStatus` constructions gain the new fields.
- [ ] **Step 2:** Fail. **Step 3:** Implement (the `perform` arms call the Task 8 stubs and the existing `rec_*` methods). **Step 4:** Green, 0 warnings; the leak tests pass with the new keys; antislop copywriting checklist run (report only).
- [ ] **Step 5:** Commit `feat(screenrec): the screen recording in the island's model and state`.

### Task 9: Hotkey row 10, the countdown's Esc, the tray item (cheap; copy: load antislop first)

**Files:**
- Modify: `src/hotkey.rs`, `src/app_hotkeys.rs`, `src/ui/settings_layout.rs` (row label and description maps), `src/ui/tray.rs`, `src/app.rs` (`MenuCmd::RecordScreen` arm), `src/shortcuts.rs`, `src/i18n.rs`

**Interfaces:**
- Consumes: Task 8's `screen_toggle` stub.
- Produces:
  - `HotkeyId::RecordScreen` appended last (`index() == 10`, config key `"record_screen"`, default `Ctrl+Alt+Shift+R`), `Hotkeys.record_screen`; a press calls `self.screen_toggle()`.
  - `app_hotkeys::HOTKEY_RECORD_SCREEN: i32 = 10`, `HOTKEY_COUNTDOWN_ESC: i32 = 13`; `pub(super) fn is_countdown_esc(wm: i32) -> bool`; `App::register_countdown_esc(&mut self) -> bool` and `App::unregister_countdown_esc(&mut self)` (`RegisterHotKey(hwnd, 13, MOD_NOREPEAT, VK_ESCAPE)`); `WM_HOTKEY` 13 calls `self.screen_cancel_countdown()` (a stub here, filled in Task 12). `wanted(config, recording)` is called with `recording = self.rec.active() || self.screen.recording()` (the mic mute works on a screen recording too).
  - `tray::MenuCmd::RecordScreen` right after `RecordMute` in `COMMANDS`; the entry comes right after the audio entry (and the mute entry while recording); `tray::set_screen_recording(hwnd: HWND, on: bool)` (the red dot shows while either kind records).
  - `crate::capture::WM_APP_SCREEN_PICK: u32 = WM_APP + 90` (in `src/capture/mod.rs`, so the overlay and the island share it).
  - `shortcuts::Action::ScreenPick { owner: isize, mic: bool, system: bool }` with the delay of `Action::Capture` (`ANIM_MS + 60`); `run` calls `crate::capture::overlay::start_rec_pick(owner, mic, system)` (a stub in `capture/overlay.rs` that posts `WM_APP_SCREEN_PICK` with nothing picked, filled in Task 11; the island's arm comes in Task 12).
- i18n: `HotkeyRowRecordScreen` "Rekam layar" / "Record the screen"; `HotkeyRowRecordScreenDesc` "Pilih layar, jendela atau area, lalu rekam" / "Pick a screen, window or area, then record"; `TrayScreenStart` "Rekam layar" / "Record the screen"; `TrayScreenStop` "Hentikan rekam layar" / "Stop screen recording".

- [ ] **Step 1: Tests:** `hotkey`: `the_record_screen_row_is_appended_with_its_default` (index 10, key, `Some(hk("Ctrl+Alt+Shift+R"))`); `an_old_config_gets_the_record_screen_default`. `app_hotkeys`: `wm_id(HotkeyId::RecordScreen) == 10`; `the_countdown_esc_is_no_row` (`row_of_wm_id(13) == None`, `is_countdown_esc(13)`, no row uses 11 or 13). `tray`: `the_screen_entry_follows_the_screen_recording` (idle: "Rekam layar" after "Mulai rekam"; screen on: "Hentikan rekam layar", the mic entry shown, the audio entry still "Mulai rekam"). `shortcuts`: `the_screen_pick_waits_like_the_capture`. The settings hotkey list tests (every row labelled, fits) stay green with 11 rows.
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; `--settings` screenshot of Tombol pintas in light and dark.
- [ ] **Step 5:** Commit `feat(screenrec): hotkey row, countdown Esc and tray entry`.

### Task 10: The notch: screen view, countdown, the thumbnail notice (standard; UI: load antislop first)

**Files:**
- Modify: `src/ui/panel_layout.rs` (`screen_compact_sources`), `src/ui/render_panel.rs` (`paint_record_compact`, new `paint_screen_countdown`), `src/ui/render.rs` (`RecFrame.screen`, `Frame.video_notice`, `paint_video_notice_thumb`, the view match), `src/ui/geometry.rs` (shape of `View::ScreenCountdown` = the REC view's), `src/app.rs` (frame fields: `RecFrame.screen` from `RecStatus.kind`, `video_notice` from `self.screen.thumb`), `src/ui/demo.rs`

**Interfaces:**
- Consumes: Task 8's model.
- Produces:
  - `RecFrame.screen: bool`. With it, the label slot shows `ScreenLabel` 6 dip after the clock's real width (as "dijeda" does) and only the mic group is drawn and registered: `panel_layout::screen_compact_sources(s: RecSources) -> RecSources` (mic only), passed to `rec_mute_regions`. Paused: "dijeda" as in phase 1, no regions.
  - `paint_screen_countdown(shape, n: u8)`: the REC layout; a 2 dip `REC_RED` ring in the dot's place (hollow, the halo kept), the numeral `n` in the clock slot (title font), `ScreenCountdownEsc` in the label slot (eleven font, secondary ink, ellipsis when it does not fit).
  - `pub struct VideoThumbView<'a> { pub id: u64, pub thumb: &'a Thumb }`; `Frame.video_notice: Option<VideoThumbView<'a>>`; a `RecSaved { thumb: Some(id) }` notice whose id matches draws the thumbnail cover-cropped into the badge spot, as `paint_clip_notice_thumb` does, with a one-entry bitmap cache of its own. `pub fn video_thumb_for<'a>(kind: &NoticeKind, view: Option<VideoThumbView<'a>>) -> Option<VideoThumbView<'a>>`.

- [ ] **Step 1: Tests:** `the_screen_view_keeps_only_the_mic_group` (`screen_compact_sources(BOTH) == mic only`; regions of a both-source screen recording: one, `RecMuteCompact(Mic)`, over the far-right meter); `the_screen_label_fits_beside_the_clock_in_both_fonts` (modelled on `sticky_reminder_chips_hold_their_words_in_both_fonts`: `"Layar"` / `"Screen"` after `"59:59"` fit `l.label` at the default notch size, robot on and off); `the_countdown_hint_fits_or_ellipsizes`; `video_thumb_view_matches_only_its_notice` (id 7 view on `RecSaved { thumb: Some(7) }`: `Some`; on `Some(8)` or `None` or `ClipPreview`: `None`).
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; `--demo` screenshots of `screen-rec`, `screen-rec-paused`, `screen-countdown` (n = 3) and `video-saved` on every island surface (`DI_SURFACE`), robot on and off; contrast numbers for the "Layar" label, the numeral, the hint and the ring against each surface.
- [ ] **Step 5:** Commit `feat(screenrec): the notch's screen view, countdown and video notice`.

### Task 11: The picker (most capable; UI: load antislop first)

**Files:**
- Create: `src/capture/rec_pick.rs`
- Modify: `src/capture/{mod.rs, win.rs, overlay.rs, demo.rs}`, `src/main.rs` (`--demo-snap rec`), `src/i18n.rs`

**Interfaces:**
- Consumes: `capture::{Candidate, Rect, pick, is_click, drag_rect, size_label}`, `win::{TopWindow, top_windows, grab_virtual_screen}`, `inline::{place_side, Side, ui_for}`, `crate::model::RecSources`.
- Produces:
  - `win::monitors() -> Vec<(isize, Rect)>` (`EnumDisplayMonitors`, physical px).
  - `rec_pick::PickMode { Screen, Window, Area }` (default `Window`); `PickTarget { Monitor(isize), Window(isize), Area(Rect) }`; `Picked { pub target: PickTarget, pub mic: bool, pub system: bool, pub title: String }` (`title`: the window's text for `Window`, else empty).
  - `rec_pick::RecPick` with `new(sources: RecSources) -> RecPick` (toggles from the recording prefs; changes are for this recording only, never saved), `mode`, `mic`, `system`, `sel: Option<(Rect, PickTarget)>`, `focus: Option<BarBtn>`; `hover(&self, pt, cands: &[Candidate], windows: &[TopWindow], monitors: &[(isize, Rect)]) -> Option<(Rect, PickTarget)>` (Screen: the monitor under `pt`; Window: the frontmost top-level window under `pt`, never an inner box; Area: `capture::pick`, as `Area(rect)`); `area_on_monitor(a: (i32, i32), b: (i32, i32), monitors: &[(isize, Rect)]) -> Rect` (the drag clamped to the monitor where it began); `MIN_AREA: i32 = 32`; `can_record(&self) -> bool` (a selection, an area at least `MIN_AREA` each side); `set_mode(&mut self, m)` clears the selection.
  - `rec_pick::BarBtn { Screen, Window, Area, Mic, System, Record }`; `rec_pick::RecBar { pub items: Vec<(BarBtn, Rect)> }` with `hit(&self, pt) -> Option<BarBtn>`; `bar_layout(ui: f32, widths: &[f32; 4]) -> RecBar` (a three-way segment, a gap, two square toggles, a gap, Rekam; heights 32 x `ui`); `place_bar(sel: Option<Rect>, size: (i32, i32), monitor: Rect) -> Rect` (under the selection by `place_side`, else centred 48 px above the monitor's bottom).
  - `rec_pick::Key { Record, Cancel, Toggle(BarBtn), Focus(BarBtn), None }`; `RecPick::key(&mut self, vk: u16, shift: bool) -> Key` (Enter: Record when `can_record` and nothing focused, else activates the focused control; Space activates the focused control; Tab / Shift+Tab move focus through the six controls and wrap; Esc: Cancel).
  - `rec_pick::take_picked() -> Option<Picked>` (a thread-local slot); `rec_pick::PickReport` (posts `WM_APP_SCREEN_PICK` to its owner exactly once: on `send(picked)` or on drop with nothing).
  - `overlay::start_rec_pick(owner: isize, mic: bool, system: bool)`: the same frozen frame, dim and highlight as the screenshot pick (UIA boxes only in Area); a click selects, a drag in Area selects a clamped area, Rekam or Enter finishes with the selection, Esc, right click or losing the foreground cancels. The bar is drawn in `theme::current_window()` colours (toggles show `Icon::Mic` / `Icon::MicOff` and `Icon::Volume` / `Icon::VolumeMute`, never colour alone; the selected segment on `segment_fill`; Rekam on `primary_fill` with a red dot; 2 dip accent focus ring), the hint pill above it per mode. Every way out, a failed start included, reports through `PickReport`.
  - `--demo-snap rec` (debug builds): the made-up scene with the bar; the pick is printed to stderr, nothing records.
- i18n: `ScreenPickScreen` "Layar" / "Screen"; `ScreenPickWindow` "Jendela" / "Window"; `ScreenPickArea` "Area" / "Area"; `ScreenPickRecord` "Rekam" / "Record"; mic and PC audio use `RecSourceMic` / `RecSourceSystem` as tooltips and accessible names; hints `ScreenPickHintScreen` "Klik layar yang mau direkam" / "Click the screen to record", `ScreenPickHintWindow` "Klik jendela yang mau direkam" / "Click the window to record", `ScreenPickHintArea` "Tarik kotak, atau klik jendela atau bagiannya" / "Drag a box, or click a window or a part of it".

- [ ] **Step 1: Tests** (`rec_pick`, pure): `layar_picks_the_monitor_under_the_pointer` (monitors `(1, 0,0,1920,1080)` and `(2, 1920,0,2560,1440)`: `pt (2000, 50)` gives `Monitor(2)` and its rect); `jendela_picks_the_frontmost_window_not_a_box` (a depth-1 box under the pointer is ignored); `area_snaps_like_the_screenshot_tool` (equals `pick(cands, pt)`); `an_area_is_kept_on_the_monitor_it_started_on` (drag (100, 100) to (2500, 900): `Rect::new(100, 100, 1820, 800)`); `a_tiny_area_cannot_be_recorded` (31 x 200 false, 32 x 32 true); `switching_mode_clears_the_selection`; `toggles_start_from_the_recording_prefs`; `the_bar_sits_under_the_selection_inside_the_monitor` (and on a selection at the monitor's bottom it flips above); `the_bar_hits_each_control`; `tab_moves_focus_through_the_bar_and_wraps`; `enter_records_only_with_a_selection`; `space_flips_the_focused_toggle`; `the_pick_report_posts_once` (a message-only window counts `WM_APP_SCREEN_PICK`: `send` then drop gives 1; drop alone gives 1). `bar_labels_fit_in_both_languages` (measured widths through `bar_layout` at `ui_for(96)` and `ui_for(144)`: no two rects overlap).
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; `--demo-snap rec` screenshots in each mode, with and without a selection, light and dark window theme and Terminal dark, a keyboard-only walk (Tab, Space, Enter, Esc) written in the report; contrast numbers for segment text on `segment_fill`, Rekam text on `primary_fill`, toggle icons on the bar, the hint pill and the focus ring.
- [ ] **Step 5:** Commit `feat(capture): pick a screen, window or area to record`.

### Task 12: The island controller (most capable; copy: load antislop first)

**Files:**
- Modify: `src/app_screenrec.rs` (fills Task 8's stubs), `src/app.rs` (state field `screen: ScreenState`, `on_timer` arm `TIMER_SCREEN_COUNTDOWN`, `WM_APP_SCREEN_PICK` / `WM_APP_SCREEN_WATCH` arms, `Event::Screen` arm, `WM_POWERBROADCAST` suspend/resume and `WTS_SESSION_LOCK` / `UNLOCK` call the screen side, both shutdown paths call `screen_shutdown` before `rec_shutdown`), `src/app_record.rs` (`rec_toggle` busy check, `rec_toggle_mute`, `rec_pause` and `rec_levels` route to the screen recording when one runs), `src/app_transcribe.rs` (`transcribe_sync_hold` also sets `Hold::ScreenRecording`), `src/app_hotkeys.rs` (Task 9's `screen_cancel_countdown` call)

**Interfaces:**
- Consumes: Tasks 1-11: `ScreenRecorder`, `Target`, `Options`, `Summary`, `EndReason`, `Error`, `files::{files_of, unique_stem, part_of, existing_files}`, `session::{options_for, ScreenPause, PauseWhy, ScreenClock}`, `exclude::Excluder`, `watch::WindowWatch`, `thumb::{grab, thumb_at}`, `audio::Forward`, `Recorder::start_mix`, `MixOptions`, `recorder::naming::file_stem`, `prefs::{video_fallback_folder}`, `app_record::{choose_dir, probe_writable, fallback_notice, folder_blocked_notice, mic_denied_notice, source_lost_notice, device_switched_notice}`, `rec_pick::{take_picked, Picked, PickTarget}`, `clean::recycle`, `transcribe_saved`, `transcribe_sync_hold`.
- Produces (in `app_screenrec.rs`):
  - `pub(super) const TIMER_SCREEN_COUNTDOWN: usize = 70;` `pub(super) const COUNTDOWN_FROM: u8 = 3;` `pub(super) const COUNTDOWN_STEP_MS: u32 = 1_000;` `pub(super) const WM_APP_SCREEN_WATCH: u32 = WM_APP + 91;` (`WM_APP_SCREEN_PICK` is Task 9's, in `capture`).
  - `ScreenState` grows to (Task 8's phase and thumb, then: session number, `recorder: Option<ScreenRecorder>`, `audio: Option<Recorder>`, `picked: Option<Picked>`, `sources: RecSources`, `clock: ScreenClock`, `pause: ScreenPause`, `excluder: Option<Excluder>`, `watch: Option<WindowWatch>`, `esc: bool`, `next_thumb: u64`, `videos: Vec<RecEntry>`, `page_shown: bool`, the start / stop `JoinHandle`s).
  - Pure: `toggle_of(phase: ScreenPhase) -> ScreenToggle` (`Idle` Pick, `Countdown(_)` CancelCountdown, `Recording` Stop, else Nothing); `busy_with_other(kind: RecKind, audio_busy: bool, screen_busy: bool) -> bool`; `busy_notice(lang) -> Notice` (`RecBusyTitle` / `RecBusyBody`, `RecInfo`); `pub(crate) enum Ended { Stopped, WindowClosed, DisplayGone }`, `ended_of(reason: EndReason, window: bool) -> Ended`; `video_saved_notice(lang, path: PathBuf, duration: Duration, ended: Ended, offer: TranscriptOffer, thumb: Option<u64>) -> Notice` (title by `Ended`, body `"{rec_duration} · {RecClickToOpen}"`, or `"{rec_duration} · {TrWantOne}"` with `Setup`, as `saved_notice`); `screen_failure_notice(lang, e: &screenrec::Error, part: Option<PathBuf>) -> Notice` (`ScreenFailedTitle` over the reason; `RecFailed { reveal: part }`); `discarded_notice(lang) -> Notice`; `target_of(t: &PickTarget) -> screenrec::Target`.
  - `App` methods: `screen_toggle` (by `toggle_of`; Pick first checks `busy_with_other(Screen, self.rec.engaged(), ..)`, then `queue_shortcut(Action::ScreenPick { owner, mic, system })` from `config.recording.sources`, phase `Picking`); `screen_pick_done(&mut self, picked: Option<Picked>)` (`None`: back to `Idle`; else the countdown: phase `Countdown(3)`, `Event::ScreenCountdown(Some(3))`, `register_countdown_esc`, the one-shot timer); `screen_countdown_tick` (3 to 2 to 1, then `screen_start`); `screen_cancel_countdown` (timer killed, Esc unregistered, `ScreenCountdown(None)`, `Idle`, no notice); `screen_start` (Esc unregistered, `Excluder::start()`, phase `Starting`, then the `screen-start` thread: `choose_dir(video_dir, video_fallback_folder, probe_writable)`, `unique_stem(dir, file_stem(now, title))`, `ScreenRecorder::start(target_of(..), &files[0], options_for(..), on_end)` where `on_end` posts `ScreenNote::Ended`, then `Recorder::start_mix(.., Box::new(Forward(input)), events)` when a source is on (events post `ScreenNote::Audio`), then posts `Started`); `screen_started` (joins: an `Err` from the screen recorder drops the excluder and shows `screen_failure_notice` or, for a refused folder, `folder_blocked_notice(.., video: true)`; an audio error keeps the video going and shows `mic_denied_notice` or the phase 1 failure notice; success: `ScreenClock::new`, `RecStatus { kind: Screen, .. }`, `tray::set_screen_recording(true)`, `sync_hotkeys`, a `WindowWatch` for a window target, `screen_fell_back(used)` like `rec_fell_back` on `recording.video_folder`); `screen_stop(&mut self, discard: bool)` (phase `Stopping`, `RecStatus(None)`, tray and hotkeys off, the `screen-stop` thread: audio `stop`, screen `stop`, then on success `grab(path, thumb_at(duration))`, or with `discard` `clean::recycle(&existing_files(path))`; posts `Stopped`); `screen_stopped` (joins; drops the excluder and the watch; `Summary.frames == 0`: the files are recycled, no notice; discard: `discarded_notice`; saved: `offer = transcribe_saved(path)`, the thumbnail stored under a new id, `video_saved_notice`; an error: `screen_failure_notice` with the `.part` when it exists; then `screen_rescan` when the page shows); `screen_pause` (user: `ScreenPause::user_toggle`), `screen_on_away(&mut self, on: bool)` (sleep and lock), `screen_on_window(&mut self, minimised: bool)`, each applying `pause` / `resume` to the screen recorder and the clock and re-sending the status; `on_screen_note(&mut self, session: u64, note: ScreenNote)` (older sessions ignored; `Ended` calls `screen_stop(false)`); `screen_toggle_mute(&mut self, source: Source)`; `screen_levels(&self) -> (f32, f32)`; `screen_discard`; `screen_rescan` (the `rec-scan` pattern on `video_dir`, `Event::VideoRecent`); `screen_follow_page(&mut self, under: Option<Page>)`; `screen_open_folder`; `screen_pick_folder`; `screen_set_folder(&mut self, dir: Option<PathBuf>)` and `screen_set_fps(&mut self, fps: u32)` (refused while busy, probed like `rec_set_folder`); `screen_shutdown` (countdown cancelled, a start joined and stopped, a recording stopped and finalized, no notice).
  - `rec_toggle` first checks `busy_with_other(Audio, .., self.screen.busy())` and shows `busy_notice`. `transcribe_sync_hold` sets `Hold::ScreenRecording` to `self.screen.engaged()` with the same cancel of a returned job as `Recording`, and is called after every phase change.

- [ ] **Step 1: Tests** (pure, in `app_screenrec.rs`): `the_toggle_does_what_the_phase_says` (all six phases); `another_recording_blocks_the_start` (`busy_with_other(Screen, true, false)`, `busy_with_other(Audio, false, true)`, `!busy_with_other(Screen, false, true)`, `!busy_with_other(Audio, true, false)`; `busy_notice(Id)` title "Rekaman lain sedang jalan", body "Hentikan dulu, lalu mulai lagi"); `the_saved_notice_names_the_length_and_why_it_ended` (Id: "Video tersimpan", body `format!("{} · klik untuk buka", rec_duration(Id, 197 s))`; `WindowClosed` "Jendela ditutup, video tersimpan"; `DisplayGone` "Layar terputus, video tersimpan"; En titles; `Setup` body ends "Bikin transkrip?"; kind `RecSaved { thumb: Some(7), .. }`); `ended_of_maps_the_reason` (`(TargetClosed, true)` WindowClosed, `(TargetClosed, false)` DisplayGone, `Stopped` Stopped); `every_screen_error_reads_in_both_languages` (each `Error` variant to its key; `NoMediaFoundation` gives "Media Foundation tidak ada di Windows ini"; a `part` gives `RecFailed { reveal: Some(part) }`); `the_target_of_a_pick` (`Monitor(5)`, `Window(9)`, `Area(Rect::new(10, 20, 300, 200))` to `Target::Region { x: 10, y: 20, w: 300, h: 200 }`). Add the formatted notices to the i18n leak test.
- [ ] **Step 2:** Fail. **Step 3:** Implement and wire. **Step 4:** Green, 0 warnings. By hand with the debug build: hotkey, pick a window, countdown, record 10 s with mic and PC audio, stop: the notice with its thumbnail, the `.mp4` beside its sidecar and cursor log; Esc during the countdown; the audio hotkey during a screen recording says "Rekaman lain sedang jalan"; minimise and restore the recorded window; close it: "Jendela ditutup, video tersimpan".
- [ ] **Step 5:** Commit `feat(screenrec): start, stop, pause and discard a screen recording from the island`.

### Task 13: The Rekam page's "Audio" / "Layar" switch (standard; UI: load antislop first)

**Files:**
- Modify: `src/ui/panel_layout.rs` (`record_layout`, `RecordHead`, `RecordLayout`, regions), `src/ui/render_panel.rs` (`paint_record`), `src/ui/render.rs` (`RecordView`), `src/app_record.rs` (`scan_recent`, `listed`, `stem_of`), `src/app.rs` (`RecordView` fields, `rec_row_menu_target` on `rec_list`, the row menu target list, `Effect::RecPickFile(kind)`), `src/app_screenrec.rs` (`screen_follow_page` from `refresh`), `src/app_transcribe.rs` (`transcribe_marks_changed` also re-sends `Event::VideoMarks(self.transcribe_marks(self.screen.videos.clone()))` while the page shows; `rec_pick_file(kind)` starts in `video_dir` for Screen), `src/ui/demo.rs`

**Interfaces:**
- Consumes: Task 8 state (`rec_tab`, `rec_list`, `screen_discard_armed`, `Hit::{RecTab, ScreenDiscard}`), Task 12 `screen_rescan`.
- Produces:
  - `scan_recent(dir, kind: RecKind, lang, today, limit, skip, probe) -> Vec<RecEntry>`: Audio lists `.m4a` / `.m4a.part` as today; Screen lists `.mp4` / `.mp4.part` (unfinished). `stem_of` strips `.mp4.part` and `.mp4` too, so `display_title` and the sidecar read work for videos.
  - `RecordHead.tabs_w: [f32; 2]` (measured "Audio", "Layar"); `record_layout(s, strip, head, live, chosen, rows, pills, tab: RecKind) -> RecordLayout`; `RecordLayout.tabs: [(RecKind, Rect); 2]` (a two-way segmented control in the title's place, the page title stays in the dots and dock), `RecordLayout.discard: Option<Rect>` (beside pause, only while a screen recording runs), `RecordLayout.hint: Option<Rect>` (`ScreenIdleHint` in the source control's place on the idle Layar tab; no idle device lines there). Regions: `RecTab(k)` per segment while idle, `ScreenDiscard` on `discard`.
  - `RecordView.tab: RecKind`, `RecordView.discard_armed: bool`; Buang shows `ScreenDiscard`, armed `ScreenDiscardSure` on the confirm fill (`confirm_fill`, red text); the empty Layar list says `ScreenNoVideos`. Rows, pills, the row menu, "Buka folder" and "Transkrip file lain…" work on the tab's list and folder.
  - Demo: `--demo-record` with `DI_REC_TAB=screen` and `DI_REC_LIVE=screen` fills the Layar tab idle and recording.

- [ ] **Step 1: Tests:** panel_layout: `the_tab_switch_takes_the_title_place_and_both_links_still_fit` (both languages, default panel size: the long "Transkrip file lain…" form fits); `the_screen_live_block_has_pause_stop_and_discard` (no overlaps, all inside the card); `the_audio_live_block_has_no_discard`; `the_idle_screen_tab_shows_the_hint_not_the_source_control` (no `RecSources` regions, no device lines); `tabs_take_clicks_only_while_idle`. app_record: `the_video_list_is_the_newest_five_mp4s` (six `.mp4`, an `.m4a` and a `.txt`: five videos, newest first); `a_video_part_left_by_a_crash_is_listed_unfinished` (and the running one's `.part` skipped); `stem_of_handles_videos` (`"a b.mp4.part"` and `"a b.MP4"` give `"a b"`); `a_video_s_sidecar_gives_its_title_and_length`. app: `right_clicks_on_a_video_row_open_its_menu` (Screen tab, `rec_row_menu_target` gives the row of `rec_list`).
- [ ] **Step 2:** Fail. **Step 3:** Implement. **Step 4:** Green, 0 warnings; `--demo-record` screenshots of both tabs idle and recording, Buang armed, in light and dark (the island panel surfaces); contrast numbers for the tab segments, the hint, Buang and "Yakin buang?".
- [ ] **Step 5:** Commit `feat(screenrec): the Layar tab of the Rekam page`.

### Task 14: Settings → Fitur → Rekam layar, and the wrap-up (standard; UI: load antislop first)

**Files:**
- Modify: `src/ui/settings_layout.rs` (`screen_card` after `recording_card`, `Hit` variants), `src/ui/settings.rs` (`Action` variants, `on_click`, `After::PickVideoFolder`, `pick_video_folder`, `Model` apply), `src/app.rs` (`on_settings_action` arms), `src/screenrec/mod.rs` (remove `#![cfg_attr(not(debug_assertions), allow(dead_code, unused_imports))]` and fix what is left), `src/i18n.rs`

**Interfaces:**
- Consumes: `RecordingPrefs.{video_folder, video_fps, video_dir}`, Task 12's `screen_set_folder` / `screen_set_fps`.
- Produces: settings `Hit::VideoFps(u32)`, `Hit::VideoFolderOpen`, `Hit::VideoFolderChange`; `Action::VideoFolder(Option<PathBuf>)`, `Action::VideoFps(u32)`; `After::PickVideoFolder`; `settings::pick_video_folder(owner: HWND)` (like `pick_rec_folder`, title `ScreenSettingsPickTitle`, posts `Action::VideoFolder`); `pub const VIDEO_DEFAULT_FOLDER: &str = r"Videos\Vale";`. The card, two rows: "Gambar per detik" with a `30` / `60` segmented control and its description; "Folder video" with the folder label and **Buka** / **Ganti** (`RecSettingsOpen`, `RecSettingsChange`).
- i18n: `ScreenSettingsTitle` "Rekam layar" / "Screen recording"; `ScreenSettingsFps` "Gambar per detik" / "Frames per second"; `ScreenSettingsFpsDesc` "60 lebih halus, filenya lebih besar" / "60 is smoother, the files are bigger"; `ScreenSettingsFolder` "Folder video" / "Video folder"; `ScreenSettingsPickTitle` "Pilih folder video" / "Choose the video folder".

- [ ] **Step 1: Tests:** `the_screen_card_has_fps_and_folder` (hits `VideoFps(30)`, `VideoFps(60)`, `VideoFolderOpen`, `VideoFolderChange`; the default label `Videos\Vale`; the 60 segment selected for `video_fps: 60`); `fps_clicks_change_only_a_new_value` (`VideoFps(60)` with 30 gives `Action::VideoFps(60)`, with 60 nothing); `the_folder_buttons_open_and_pick` (Buka gives `After::OpenFolder(video_dir)`, Ganti `After::PickVideoFolder`). The existing `exactly_the_overflowing_sections_get_a_viewport`, `everything_fits_in_the_window_height` and `every_card_of_every_section_has_a_title_but_the_previews` stay green.
- [ ] **Step 2:** Fail. **Step 3:** Implement; remove the dead-code allowance of `screenrec` and delete or `#[cfg(debug_assertions)]`-gate what only `debug.rs` uses (the zoom and the cursor log stay: phase 4 and the sidecar use them).
- [ ] **Step 4:** `cargo test --locked` green; `cargo build --locked` and `cargo build --release --locked` with 0 warnings. `--settings features` screenshots in light, dark and `DI_SURFACE=terminal`; contrast numbers for the segment and the buttons.
- [ ] **Step 5:** Commit `feat(screenrec): Settings card for the frame rate and the video folder`.

---

## Live checks for the controller (owner's PC, after the final review, before merging)

From the spec's "Done means". Use the release build of the branch; stop the installed Vale first (`Stop-Process -Name vale, dynamic-island`).

- [ ] `cargo test --locked` green and `cargo build --release --locked` with no warnings on the branch.
- [ ] Exe size: `vale.exe` of the branch against `main`'s (4,299,776 bytes at 4a6294d), both built in folders of the same path length; both numbers for [[exe-size]].
- [ ] Idle: before any recording and again 10 s after a stop, Vale's thread count in Process Explorer is what it was at start; no `vale-screenrec`, `rec-capture`, `rec-mix`, `screen-start` or `screen-stop` thread, no timer 70 (and nothing new in an ETW or Process Explorer CPU view at idle).
- [ ] Record a **monitor**, a **window** (moved across the screen and to the other monitor while recording) and an **area**, each with mic and PC audio on: each `.mp4` plays in Film & TV and VLC, the narration and the PC audio in both ears at their level, the cursor visible, no click ripples, no Vale window (notch, panel, Settings opened mid-recording, a sticky note created mid-recording, the transcript window, the lyrics card, the device dropdown) in the video.
- [ ] Pause and resume from the page and the notch: no gap in the video, no word cut at the pause; the notch shows "dijeda".
- [ ] Buang: "Yakin buang?", then "Video dibuang" and the files are in the Recycle Bin.
- [ ] The countdown 3, 2, 1 in the notch; Esc cancels it and nothing is recorded; `Esc` works normally in other apps before and after.
- [ ] The saved notice "Video tersimpan · <length> · klik untuk buka" with its thumbnail; a click shows the file in Explorer.
- [ ] Transcripts on with auto on: the `.srt` lands beside the `.mp4` under the same name and shows as subtitles in VLC and Film & TV; the transcript window opens the video's lines (no Saya/Peserta) and plays its audio.
- [ ] A transcript job pauses while a screen recording runs (the row says "Antre", the window "Dijeda selama merekam") and finishes after the stop.
- [ ] One at a time: the audio hotkey during a screen recording, and the screen hotkey during an audio recording, say "Rekaman lain sedang jalan".
- [ ] Minimise the recorded window: the notch shows "dijeda"; restore: it goes on. Close it: "Jendela ditutup, video tersimpan".
- [ ] Sleep and lock pause the recording; the wake or unlock resumes it unless it was paused by hand.
- [ ] Controlled folder access on: the video goes to `%USERPROFILE%\Vale\Video` with the fallback notice, and Settings shows the new folder.
- [ ] Kill Vale mid-recording (Task Manager): the `.mp4.part` plays, and the Layar tab lists it as "belum selesai".
- [ ] A fullscreen video while recording: the notch hides, the tray dot stays, the recording goes on.
- [ ] The picker by keyboard only (Tab, Space, Enter, Esc) and with a mouse in light and dark; the Settings card in light, dark and Terminal.
- [ ] 60 fps in Settings: a 60 fps video; the default 30 fps on the owner's laptop drops no frames (`Summary.dropped` in the debug log, or a frame count in MediaInfo).

## Decisions this plan made where the spec left room

- **Mono, not stereo.** The spec's Decisions say both "one AAC stereo track" and "the audio track is mono AAC"; the later, more specific line wins: one mono AAC track at 96 kbit/s (track C's `aac_bytes_per_second(1)`), which players put in both ears.
- **Mix:** a sum of the two sources through a stateless soft limiter (knee 0.7, `tanh` above it), not a mean: a one-sided passage keeps its exact level, two loud sources never clip.
- **Audio path:** phase 1's recorder gets a mix mode (`Recorder::start_mix`, thread `rec-mix`) that pushes stamped mono into track C's `AudioInput`; the recorder itself never pauses in that mode; the screen recorder's clock keeps every pause span so audio arriving late still lands, and paused audio is dropped by its time.
- **Exclusion from capture:** only while a screen recording runs (set just before the capture starts, kept on windows shown later by a WinEvent hook, restored after the recording thread ends), so screenshots and the owner's own screen shares still see Vale.
- **Countdown look:** the REC layout with a hollow red ring, the numeral in the clock's place and "Esc untuk batal" in the label's place; it outranks the panel for its 3 s and opens nothing on hover. The recording starts after the 1 (not warmed up during the countdown).
- **Esc:** raw hotkey id 13, outside the contract's table (11 stays phase 4's), registered only for the countdown. The screen hotkey during the countdown cancels it too.
- **Thumbnail:** decoded from the saved file at `min(1 s, half the length)` on the stop thread (120 px box), so it needs nothing from the GPU pipeline.
- **"Hover offers pause, stop and discard":** the REC view's hover already opens the Rekam page; the Layar tab's live block holds Jeda / Lanjut, Berhenti and Buang (Buang takes two clicks and sends the files to the Recycle Bin). No new hover strip on the notch.
- **Notch:** "Layar" after the clock and only the mic group (the spec's words); PC audio mute stays on the Rekam page.
- **Picker:** opens on Jendela (as in mockup 8); a click or a drag selects, Rekam or Enter starts; the Mic / Suara PC toggles start from `recording.sources` and are not saved back; both off records a video with no audio track. An area is clamped to the monitor where the drag began. The picker lives in `src/capture/` (the screenshot overlay), not `src/screenshot/`.
- **Rekam page:** the "Audio" / "Layar" switch takes the title's place; it follows the running recording's kind and is locked while one runs.
- **Pause causes:** user, away (sleep **and** lock: phase 1 pauses only on sleep, the screen recording also on lock, since the lock screen would otherwise be recorded) and a minimised window, all tracked apart so the user's own pause always wins.
- **Ends:** a window closed says "Jendela ditutup, video tersimpan", a monitor gone "Layar terputus, video tersimpan"; a recording with no frame is removed quietly; when the audio cannot start the video records on silent with the phase 1 notice.
- **Files:** the stem rule is phase 1's `naming::file_stem` (track C's own copy goes); the sidecar's `"app"` is `null`; the title is the recorded window's text.
- **Transcripts:** an `.mp4` without a sidecar is downmixed and unlabelled; the "Transkrip file lain…" dialog takes `.mp4` in both tabs.
