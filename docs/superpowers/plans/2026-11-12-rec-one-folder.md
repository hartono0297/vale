# One recordings folder (Audio and Video subfolders) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Audio and screen recordings share one parent folder by default, each in its own subfolder (`Audio`, `Video`); Settings shows one folder row; existing recordings move into the new subfolders once.

**Architecture:** `recording.folder` becomes the parent ("root"); `dir()` and `video_dir()` derive `root\Audio` and `root\Video`. A one-time startup step (marker `recording.folders_split`) works out the root from the old two keys, renames Vale's recordings (with their companion files) into the subfolders, and rewrites the transcript queue's paths. The Controlled-folder-access fallback moves to `%USERPROFILE%\Vale\Rekaman\{Audio,Video}`. The video folder setting, its Settings row and its picker go away.

**Tech Stack:** Rust 2024, `windows` crate (known folders), `serde_json`, std::fs.

**Spec:** the owner's request ("bisa ga default video dan recording folder jadi 1 aj, bedain per sub folder"); the owner skipped the questions, so the design below is the session's call. Read `docs/memory/recording-audio.md`, `screen-recording.md`, `controlled-folder-access.md`, `transcript.md`, `settings-structure.md` first.

## Design (decided)

- Root default: `Documents\Vale\Rekaman` (the old audio default, `prefs::default_folder()`, unchanged). Subfolders: `Audio`, `Video` (same words in both languages).
- Fallback root (Controlled folder access): `%USERPROFILE%\Vale\Rekaman` (`prefs::fallback_folder()`, unchanged). Fallback dirs: `<fallback root>\Audio`, `<fallback root>\Video`.
- A fall-back recording with no folder chosen sets `recording.folder = <fallback root>` for good (both kinds then follow it). A folder the user picked stays theirs (as today).
- Settings → Fitur: one row "Folder rekaman" / "Recordings folder" in the Rekaman card with Open (opens the root) and Change (picks the root). The "Folder video" row in the Rekam layar card is removed. Description: `<root label> · Audio & Video` in both languages, wrapped at ` · ` only when it does not fit (same rule as Kualitas video).
- The Rekam page's open-folder button opens `Audio` on the Audio tab and `Video` on the Layar tab (unchanged code, new paths).
- Changing the folder is refused while an audio **or** screen recording is busy (Settings snaps back).
- One-time migration (`recording.folders_split` false or missing):
  - New root `R`: the old `folder` when set; otherwise the fallback root when the old `video_folder` equals the old video fallback (`%USERPROFILE%\Vale\Video`, Windows is known to block); otherwise `None` (default).
  - Audio: recordings in `R` itself (`<stem>.m4a`) move to `R\Audio`.
  - Video: when the old `video_folder` is `None`, the old video default (`Videos\Vale`) or the old video fallback (`%USERPROFILE%\Vale\Video`), recordings there (`<stem>.mp4`) move to `R\Video`. A video folder the user picked is left alone (its files stay, no longer listed).
  - "Recording with its companions": for each `<stem>.<ext>` file, every file in the same folder whose name starts with `<stem>.` (sidecar `.vale.json`, cursor log, `.md`, `.srt`, `.transcript.*`, `.part`). Rename only (`std::fs::rename`); a file whose target exists is skipped; any error leaves that file and the rest go on. The target folder is created only when something moves.
  - After the video move, the old video folder is removed only if empty (`std::fs::remove_dir`, never recursive).
  - Queue: `queue.json` entries whose `audio` was moved get the new path (case-insensitive match, `transcribe::queue::same_audio`); the file is rewritten only when something changed.
  - Then: `folder = R`, `video_folder = None`, `folders_split = true`, save config. Runs once, also when nothing moved or a move failed.

## Global Constraints

- Windows 11 only; `cargo test --locked` must stay green; tests never show windows (`docs/memory/test-spawns-no-window.md`).
- No new timers, threads or hotkeys. The migration runs synchronously at startup, before `transcribe_init()`.
- Never overwrite, never delete a recording, never `remove_dir_all`.
- Config stays lenient (`RecordingPrefs::from_json`): a missing or wrong-typed `folders_split` reads as `false`.
- UI copy (Indonesian first, English second) goes through `i18n.rs` keys; Settings work loads `antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-copywriting`.
- Work in `.claude/worktrees/rec-one-folder` on branch `feat/rec-one-folder`. Do not touch `AGENTS.md`.

## Review Focus

1. Owner's real config (`folder = C:\Users\X\Vale\Rekaman`, `video_folder = C:\Users\X\Vale\Video`): audio ends in `...\Rekaman\Audio`, video in `...\Rekaman\Video`, empty `...\Vale\Video` removed, queued jobs keep running. (Task 2 test `owner_layout_moves_both_and_drops_the_empty_video_folder`, Task 1 test `fallback_paths_keep_the_fallback_root`.)
2. A target name already taken in the subfolder: the source stays, nothing is overwritten. (Task 2 test `a_taken_name_is_never_overwritten`.)
3. One file cannot be moved (locked, another drive): the others still move, migration is marked done, no panic. (Task 2 test `a_failed_rename_leaves_that_file_and_moves_the_rest`.)
4. Second start: nothing runs again (marker), files dropped later in the root stay put. (Task 3 test `split_runs_once`.)
5. Folder change while a screen recording runs: refused. (Task 3 test `folder_change_is_refused_while_a_screen_recording_is_busy` if `App` is testable there; otherwise a manual check in the report.)

---

### Task 1: Root model and migration plan in prefs

**Files:**
- Modify: `src/recorder/prefs.rs`

**Interfaces:**
- Produces:
  - `pub const AUDIO_SUB: &str = "Audio"; pub const VIDEO_SUB: &str = "Video";`
  - `RecordingPrefs::root(&self) -> PathBuf` (`folder` or `default_folder()`), `dir()` = `root().join(AUDIO_SUB)`, `video_dir()` = `root().join(VIDEO_SUB)`.
  - field `pub folders_split: bool` (key `"folders_split"`, default `false`, serialized). `video_folder` stays as a field, documented as legacy (read only by the migration, `None` after it).
  - `pub fn fallback_audio_dir() -> Option<PathBuf>`, `pub fn fallback_video_dir() -> Option<PathBuf>` (fallback root joined with the sub). Remove `video_fallback_folder()` and `default_video_folder()`; add `pub fn legacy_video_default() -> PathBuf` (`Videos\Vale`) and `pub fn legacy_video_fallback() -> Option<PathBuf>` (`%USERPROFILE%\Vale\Video`).
  - `pub struct Known { pub default_root: PathBuf, pub fallback_root: Option<PathBuf>, pub video_default: PathBuf, pub video_fallback: Option<PathBuf> }` with `Known::system() -> Known`.
  - `pub struct Move { pub from: PathBuf, pub to: PathBuf, pub ext: &'static str }`
  - `pub struct SplitPlan { pub root: Option<PathBuf>, pub moves: Vec<Move> }`
  - `pub fn split_plan(folder: Option<&Path>, video_folder: Option<&Path>, known: &Known) -> SplitPlan` (rules in Design; path comparisons case-insensitive like `same_audio`).

- [ ] **Step 1: Write the failing tests** (in `prefs.rs` tests, with a fake `Known` of `C:\U\Documents\Vale\Rekaman`, `C:\U\Vale\Rekaman`, `C:\U\Videos\Vale`, `C:\U\Vale\Video`)
  - `dirs_are_subfolders_of_one_root`: default prefs → `dir()` ends with `Vale\Rekaman\Audio`, `video_dir()` ends with `Vale\Rekaman\Video`; `folder = D:\R` → `D:\R\Audio`, `D:\R\Video`.
  - `fresh_install_plans_default_root`: `split_plan(None, None, &k)` → `root == None`, moves = `[Move{from: default_root, to: default_root\Audio, ext:"m4a"}, Move{from: C:\U\Videos\Vale, to: default_root\Video, ext:"mp4"}]`.
  - `fallback_paths_keep_the_fallback_root`: `split_plan(Some(C:\U\Vale\Rekaman), Some(C:\U\Vale\Video), &k)` → root `Some(C:\U\Vale\Rekaman)`, moves to `C:\U\Vale\Rekaman\Audio` and from `C:\U\Vale\Video` to `C:\U\Vale\Rekaman\Video`.
  - `blocked_video_only_moves_the_root_to_the_fallback`: `split_plan(None, Some(C:\U\Vale\Video), &k)` → root `Some(C:\U\Vale\Rekaman)`.
  - `a_picked_video_folder_is_left_alone`: `split_plan(Some(D:\R), Some(E:\Vid), &k)` → root `Some(D:\R)`, exactly one move (audio, `D:\R` → `D:\R\Audio`).
  - `case_differences_still_match_the_legacy_paths`: `video_folder = c:\u\VALE\video` treated as the fallback.
  - `folders_split_reads_leniently`: `from_json({})` → false; `{"folders_split": 7}` → false; `{"folders_split": true}` → true; serialize round-trips it.
  - Update existing tests that use `default_video_folder` / `video_fallback_folder` / `unset.video_dir()` to the new names and paths.

- [ ] **Step 2: Run them, expect compile errors / failures**
  Run: `cargo test --locked recorder::prefs` → FAIL (missing items).

- [ ] **Step 3: Implement the items above in `src/recorder/prefs.rs`**; fix the module comment and field docs. Callers elsewhere that used the removed functions get the minimal rename needed to compile (`app_screenrec.rs` start uses `fallback_video_dir()`); behaviour wiring is Task 3.

- [ ] **Step 4: Run** `cargo test --locked` → PASS (all, not just prefs).

- [ ] **Step 5: Commit** `feat(rec): one recordings root with Audio and Video subfolders`

### Task 2: Moving recordings and rewriting the queue

**Files:**
- Create: `src/recorder/split.rs` (add `pub mod split;` in `src/recorder/mod.rs`)

**Interfaces:**
- Consumes: `prefs::{Move, SplitPlan}`, `crate::transcribe::queue::same_audio`.
- Produces:
  - `pub fn move_recordings(m: &Move) -> Vec<(PathBuf, PathBuf)>`: the `(old, new)` paths of each main recording file moved (`<stem>.<ext>`). Companions move with it (Design). Non-recursive (files directly in `from`). `to` created only when a first rename is about to happen. Errors are printed with `eprintln!("vale: ...")` and skipped.
  - `pub fn apply(plan: &SplitPlan) -> Vec<(PathBuf, PathBuf)>`: runs every move, then `remove_dir` on each video `from` that is not the root and is empty; returns all moved pairs.
  - `pub fn rewrite_queue(text: &str, moved: &[(PathBuf, PathBuf)]) -> Option<String>`: the queue JSON (`{"v":1,"jobs":[{"audio":..,"model":..,"tries":..}]}`) with moved `audio` paths replaced; `None` when nothing matched or the text is not that JSON. Other fields and entries untouched.

- [ ] **Step 1: Write the failing tests** (temp dirs under `std::env::temp_dir()` with the pid in the name, removed at the end, like `app_record.rs` tests)
  - `moves_a_recording_with_its_companions`: `a.m4a`, `a.vale.json`, `a.md`, `a.srt`, `a.transcript.md` + unrelated `notes.txt` and `b.m4a.part` (no `b.m4a`) → all `a.*` in `Audio`, `notes.txt` and `b.m4a.part` stay; returns `[(from\a.m4a, to\a.m4a)]`.
  - `video_moves_the_cursor_log_too`: `x.mp4`, `x.vale.json`, `x.<cursor_log::EXTENSION>` → all in `Video`.
  - `a_taken_name_is_never_overwritten`: `to\a.m4a` already exists with other bytes → source `a.m4a` stays, target bytes unchanged, `a.m4a` not in the result.
  - `a_failed_rename_leaves_that_file_and_moves_the_rest`: hold `a.m4a` open with `share_mode(0)` (`std::os::windows::fs::OpenOptionsExt`) → `a.m4a` stays, `b.m4a` moves, no panic.
  - `nothing_to_move_creates_no_folder`: empty `from` → `to` does not exist.
  - `owner_layout_moves_both_and_drops_the_empty_video_folder`: root `R` with `a.m4a`, sibling `V` with `v.mp4` → `R\Audio\a.m4a`, `R\Video\v.mp4`, `V` gone; with an extra `keep.txt` in `V` → `V` stays.
  - `queue_paths_follow_the_move`: two jobs, one moved (different case in the queue) → that `audio` replaced, other job and `model`/`tries` unchanged; no match → `None`; `"not json"` → `None`.

- [ ] **Step 2: Run** `cargo test --locked recorder::split` → FAIL.

- [ ] **Step 3: Implement** in `src/recorder/split.rs`. Match companions by `file_name` string `starts_with(format!("{stem}."))`, case-insensitive (`to_ascii_lowercase` on both). A companion whose rename fails just stays.

- [ ] **Step 4: Run** `cargo test --locked` → PASS.

- [ ] **Step 5: Commit** `feat(rec): move old recordings into the Audio and Video subfolders`

### Task 3: App wiring (startup migration, fallbacks, one folder setter)

**Files:**
- Modify: `src/app.rs` (call before `app.transcribe_init()`), `src/app_record.rs`, `src/app_screenrec.rs`, `src/model.rs`, `src/state.rs`, `src/ui/geometry.rs` / `src/ui/robot.rs` only if the notice field change needs it.

**Interfaces:**
- Consumes: Task 1 (`root`, `dir`, `video_dir`, `fallback_audio_dir`, `fallback_video_dir`, `fallback_folder`, `Known::system`, `split_plan`, `folders_split`), Task 2 (`split::apply`, `split::rewrite_queue`).
- Produces:
  - `App::rec_split_folders(&mut self)` in `app_record.rs`: if `folders_split` is false, plan from the config, `apply`, rewrite `self.tr.store.queue_path()` when `rewrite_queue` returns `Some`, then set `folder = plan.root`, `video_folder = None`, `folders_split = true`, `save_config()`.
  - `NoticeKind::RecFolderBlocked { dir: PathBuf }` (no `video`); `Effect::VideoPickFolder` removed; the notice's click is `Effect::RecPickFolder` for both kinds.
  - `App::rec_set_folder` refuses while `self.rec.engaged() || self.screen.busy()`, and rescans both lists when their page is shown.

- [ ] **Step 1: Write / change the failing tests**
  - `state.rs`: `a_blocked_video_folder_opens_the_video_picker` → `a_blocked_folder_opens_the_one_folder_picker` (a notice made by the screen start's blocked path gives `Effect::RecPickFolder`). Fix the other `RecFolderBlocked { .. video .. }` constructions.
  - `app_record.rs` tests around line 1382-1450 (`choose_dir` with the fallback): paths become `...\Rekaman\Audio` and `C:\Users\a\Vale\Rekaman\Audio`.
  - `split_runs_once`: if `App` can be built in a unit test (check existing app tests in `app.rs` ~11267 for the pattern), test that a config with `folders_split = true` moves nothing; otherwise test a pure helper `fn needs_split(prefs: &RecordingPrefs) -> bool` and say so in the report.

- [ ] **Step 2: Run** `cargo test --locked` → FAIL on the changed tests.

- [ ] **Step 3: Implement**
  - `app.rs`: `app.rec_split_folders();` right before `app.transcribe_init();`.
  - Audio start (`app_record.rs` ~768): fallback is `fallback_audio_dir()`. `rec_fell_back`: when `used != dir()` and `folder` is `None`, set `folder = fallback_folder()` (the root), save, sync settings; the notice stays.
  - Screen start (`app_screenrec.rs` ~513): fallback `fallback_video_dir()`; `StartError::FolderBlocked` notice without `video`. `screen_fell_back`: same as audio, setting `folder`, not `video_folder`.
  - `screen_pick_folder` and the blocked notice click go to `rec_pick_folder`; delete `screen_pick_folder`.
  - `rec_set_folder`: the busy check above; after saving, `rec_rescan()` when `self.rec.page_shown`, `screen_rescan()` when `self.screen.page_shown`.
  - `transcribe` start folder (`app_transcribe.rs` `pick_start`) needs no change; its test `assert_ne!(unset.dir(), unset.video_dir())` must still hold.

- [ ] **Step 4: Run** `cargo test --locked` → PASS; `cargo build --release --locked` → no warnings about dead code from removed paths.

- [ ] **Step 5: Commit** `feat(rec): split the folders once at startup and share one fallback root`

### Task 4: Settings shows one folder row

Load `antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-copywriting` before editing.

**Files:**
- Modify: `src/ui/settings_layout.rs` (`recording_card`, the screen card ~4700-4735, `Hit` enum ~452-460, consts ~4513-4516, tests ~9331-9700), `src/ui/settings.rs` (~394-395, ~2594-2602, `After`, `Action`, `pick_video_folder`, ~4600, tests ~4993-5032), `src/app.rs` (~6565), `src/app_screenrec.rs` (`screen_set_folder` removed), `src/i18n.rs`.

**Interfaces:**
- Consumes: `RecordingPrefs::root()`.
- Produces: no `Hit::VideoFolderOpen` / `VideoFolderChange`, no `Action::VideoFolder`, no `After::PickVideoFolder`, no `pick_video_folder`, no `VIDEO_DEFAULT_FOLDER`, no `Key::ScreenSettingsFolder`. New key `Key::RecSettingsFolderSubs` = `"Audio & Video"` (id and en).

- [ ] **Step 1: Write / change the failing tests**
  - The Rekaman card test (~9331-9363): description text is `"Documents\Vale\Rekaman · Audio & Video"` when unset, `"Video\Kerja · Audio & Video"`-style label for a chosen `D:\Video\Kerja` (use `rec_folder_label`), wrapping at ` · ` into two lines only at a width where one line does not fit (reuse the Kualitas video wrap helper at ~4657).
  - The Rekam layar card test (~9491-9537): no folder row, no `VideoFolder*` hits; the card's row count drops by one.
  - `settings.rs`: `Hit::RecFolderOpen` opens `prefs.root()` (not `dir()`); remove the `VideoFolderOpen` cases (~5027).

- [ ] **Step 2: Run** `cargo test --locked ui::settings` → FAIL.

- [ ] **Step 3: Implement**: remove the video folder row and everything listed under Produces; the folder row's Open opens `root()`; description = `format!("{label} · {}", t(RecSettingsFolderSubs))` with the wrap rule. Keep the row's title key `RecSettingsFolder` ("Folder rekaman" / "Recordings folder").

- [ ] **Step 4: Run** `cargo test --locked` → PASS; `cargo build --release --locked` clean.

- [ ] **Step 5: Look at it**: `cargo run --release -- --settings` (stop installed copies first: `Stop-Process -Name vale, dynamic-island`), open Fitur, check the Rekaman card's folder row and that Rekam layar has no folder row; dark and light. Screenshot to the scratchpad for the report.

- [ ] **Step 6: Commit** `feat(settings): one recordings folder row`
