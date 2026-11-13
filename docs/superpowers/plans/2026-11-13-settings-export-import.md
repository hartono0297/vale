# Export and Import Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings → Tentang can export the general settings to one JSON file and import such a file, which replaces the general settings and restarts Vale.

**Architecture:** A pure module `src/config_transfer.rs` owns the field classification, the file format, validation and the merge, plus three small file helpers. The island (`src/app_transfer.rs`) runs the dialogs, holds the pending import, refuses while recording, backs up, saves and restarts through a new `--imported` start flag that reuses the updater's mutex wait. Settings only renders a model field and posts actions.

**Tech Stack:** Rust 2024, `serde_json`, `chrono`, `windows` crate (`IFileOpenDialog`, `IFileSaveDialog`).

**Spec:** `docs/superpowers/specs/2026-11-13-settings-export-import-design.md` (read it; its field lists are the source of truth).

## Global Constraints

- Windows 11 x64 only; `cargo test --locked` must stay green; no new crates.
- Tests never touch `%APPDATA%`, the real `config.json`, or show a window; file tests write under `target/test-tmp/<test name>/`.
- No new timer, no new hotkey id. New `WM_APP` ids: none (reuse `WM_APP_SETTINGS_ACTION`).
- Every string in both languages through `i18n`; no em dash anywhere; the exact copy below is from the spec.
- File format: `{"vale_settings": 1, "app_version": "<update::CURRENT>", "exported": "<RFC 3339 local time>", "settings": {…}}`; max 1 MB.
- Default export names: `Vale-pengaturan-YYYY-MM-DD.json` (id), `Vale-settings-YYYY-MM-DD.json` (en), default folder Documents.
- Backup name (one fixed name, any language): `config-before-import-YYYYMMDD-HHMMSS.json` in `data_dir::dir()`.
- UI and copy work: load `antislop:antislop`, `antislop:antislop-ui`, `antislop:antislop-copywriting`, `antislop:antislop-human` before Task 5 (mode `during`, global preference); Settings spacing follows `docs/memory/settings-spacing.md`.

## Review Focus

1. A hand-made file with a non-built-in tile (`{"tile":"x","kind":"Command","target":"calc"}`) or `google_client_secret` in `settings`: both ignored (Task 1 tests).
2. A file-owned field (`battery_threshold`) in the import file survives the island's later `save_merged` and the teardown save (Task 2 test).
3. A `settings` value of the wrong type (`"notch_width": "wide"`, `"hotkeys": 5`): that field reads as its lenient default, the rest still imports (Task 1 test).
4. Confirm pressed while an audio or screen recording runs: refused with "Hentikan rekaman dulu", nothing written (Task 4 test on the pure gate).
5. Settings closed or section left while a confirmation is pending: the pending import is dropped, a later confirm cannot fire (Task 5 test on state).

---

### Task 1: Classification, file format, validation and merge (pure)

**Files:**
- Create: `src/config_transfer.rs` (with `#[cfg(test)] mod tests`)
- Modify: `src/main.rs` (add `mod config_transfer;`)

**Interfaces:**
- Produces:
  - `pub const FORMAT: u64 = 1;` `pub const MAX_BYTES: usize = 1_048_576;`
  - `pub const EXPORTED: &[&str]` and `pub const LEFT_OUT: &[&str]`: top-level keys exactly as the spec's two lists (`recording`, `transcript`, `captions`, `arrange`, `shortcut_tiles` are in `EXPORTED`, filtered below).
  - `pub const RECORDING_KEYS: &[&str] = &["sources","echo_cancel","video_fps","video_quality","cursor_highlight","click_ripples"];`
  - `pub const TRANSCRIPT_KEYS: &[&str] = &["enabled","model","language","auto"];`
  - `pub const CAPTIONS_KEYS: &[&str] = &["mic","notch","on_battery","language","size"];`
  - `pub const ARRANGE_KEYS: &[&str] = &["gap","templates"];`
  - `pub fn general_settings(cfg: &Config) -> serde_json::Map<String, Value>`: serialises `cfg`, keeps `EXPORTED` keys, filters the split objects to their key lists, keeps only built-in `shortcut_tiles` entries (see below).
  - `pub fn export_json(cfg: &Config, app_version: &str, exported: chrono::DateTime<chrono::FixedOffset>) -> String` (pretty).
  - `pub struct Incoming { pub app_version: String, pub exported: Option<chrono::NaiveDate>, pub settings: serde_json::Map<String, Value> }`
  - `pub enum ImportError { NotVale, Newer { app_version: String } }`
  - `pub fn read_incoming(bytes: &[u8]) -> Result<Incoming, ImportError>`
  - `pub fn apply(target: &Config, incoming: &Incoming) -> Config`
- A tile entry is built-in when `crate::tiles` recognises its `tile` id as a built-in `Shortcut` (use the parser `ShortcutTiles` already uses); everything else is dropped on export and on import.

- [ ] **Step 1: Write the failing tests** in `src/config_transfer.rs`:
  - `every_config_key_is_classified`: keys of `serde_json::to_value(Config::default())` (also with `media_log: true`, which is skipped when false) ⊆ `EXPORTED ∪ LEFT_OUT`, the two lists are disjoint; same for the keys of `recording`/`transcript`/`captions`/`arrange` against their key list plus the spec's left-out nested names (`folder`, `video_folder`, `mic_device`, `system_device`, `consent_shown`, `setup_offered`, `window`, `place`, `saved`).
  - `export_leaves_out_private_fields`: a config with `google_client_id = "id.apps"`, `google_client_secret = "s3cret"`, `muted_apps = ["Orca"]`, an own tile with `target = "C:\\Users\\X\\a.exe"`, a saved arrangement with `exe = "C:\\x.exe"`, `recording.folder = Some("C:\\Users\\X\\Rek")`, `notch_monitor = Chosen{path:"\\\\?\\DISPLAY#X",..}`: the exported string contains none of `"id.apps"`, `"s3cret"`, `"Orca"`, `"a.exe"`, `"x.exe"`, `"Rek"`, `"DISPLAY#X"`, and has `"vale_settings": 1`.
  - `import_keeps_the_targets_private_and_pc_fields`: target has its own Google client, `muted_apps`, own tile, saved arrangement, folders, devices, monitor, window places, `onboarding_done = true`, `transcript.setup_offered = true`; after `apply` with a file from a different config, all those equal the target's.
  - `import_takes_general_fields`: `accent`, `notch_width`, `hotkeys`, `mascot`, `battery_threshold`, `recording.video_fps`, `transcript.model`, `captions.size`, `arrange.gap` from the file win.
  - `missing_keys_keep_target_values`: a file whose `settings` is `{"accent": …}` only changes `accent`.
  - `hand_added_private_keys_are_ignored`: `settings` with `google_client_secret`, `muted_apps`, `notch_monitor`, `recording.folder` changes none of them.
  - `bad_values_read_as_defaults`: `"notch_width": "wide"` gives `Config::default().notch_width`, `"hotkeys": 5` gives default hotkeys, and `accent` from the same file still applies.
  - `tiles_merge_builtins_then_own`: file built-ins in the file's order, then the target's own tiles with their `on`; a file entry with an unknown id or with `kind`/`target` is dropped.
  - `refusals`: `b"not json"` → `NotVale`; `{}` → `NotVale`; `{"vale_settings":"1","settings":{}}` → `NotVale`; `{"vale_settings":1}` → `NotVale`; `{"vale_settings":1,"settings":[]}` → `NotVale`; `MAX_BYTES + 1` bytes → `NotVale`; `{"vale_settings":2,"app_version":"6.0.0","settings":{}}` → `Newer{app_version:"6.0.0"}`; invalid UTF-8 → `NotVale`.
  - `unknown_keys_are_ignored`: `settings` with `"future_thing": true` imports fine.
  - `round_trip`: `apply(&Config::default(), &read_incoming(export_json(&custom, …))?)` equals `custom` on every `EXPORTED` key (compare via `general_settings`).
- [ ] **Step 2: Run** `cargo test --locked config_transfer` → FAIL (module missing).
- [ ] **Step 3: Implement.** `apply` builds the merged JSON: `serde_json::to_value(target)`, overwrite each `EXPORTED` key present in `incoming.settings`; for `recording`/`transcript`/`captions`/`arrange` overwrite only their listed sub-keys inside the target's object; for `shortcut_tiles` build built-ins-from-file then target's own; then `config::parse(&merged.to_string())`. If a key's file value is not an object where the target's is, skip it for the split objects. `read_incoming` takes `exported` as the date part of the RFC 3339 string or `None`.
- [ ] **Step 4: Run** `cargo test --locked config_transfer` → PASS; `cargo test --locked` → PASS.
- [ ] **Step 5: Commit** `feat(config): export filter and import merge for settings`.

### Task 2: File helpers (export write, backup, restart-safe save)

**Files:**
- Modify: `src/config_transfer.rs`

**Interfaces:**
- Consumes: Task 1's `export_json`, `apply`, `Incoming`.
- Produces:
  - `pub fn write_export(path: &Path, cfg: &Config, now: DateTime<FixedOffset>) -> io::Result<()>`: writes `<path>.tmp` then renames over `path` (replace an existing file).
  - `pub fn backup_config(config_path: &Path, now: NaiveDateTime) -> io::Result<Option<PathBuf>>`: copies to `config-before-import-YYYYMMDD-HHMMSS.json` beside it (appending `-2`, `-3` … if taken); `Ok(None)` when there is no `config.json`.
  - `pub fn import_and_save(config_path: &Path, mem: &Config, incoming: &Incoming) -> Config`: `let truth = config::save_merged_at(config_path, mem).config; let new = apply(&truth, incoming); let _ = new.save_to(config_path); new`.
  - `pub fn default_export_name(lang: Lang, today: NaiveDate) -> String`.

- [ ] **Step 1: Write the failing tests** (files under `target/test-tmp/config_transfer_*`): `write_export_replaces_and_leaves_no_tmp`; `backup_copies_and_never_overwrites` (two calls same second → two files); `backup_without_config_is_none`; `import_survives_the_islands_later_saves`: on-disk config with `battery_threshold: 20`, mem default; incoming sets `battery_threshold: 35` and `accent`; `let new = import_and_save(..)`; then `config::save_merged_at(path, &new)` twice (the restart's save and the teardown save) → `config::parse(read(path))` has `battery_threshold == 35` and the new accent; `default_export_name` gives `Vale-pengaturan-2026-11-13.json` / `Vale-settings-2026-11-13.json`.
- [ ] **Step 2: Run** `cargo test --locked config_transfer` → FAIL.
- [ ] **Step 3: Implement** the four functions.
- [ ] **Step 4: Run** `cargo test --locked` → PASS.
- [ ] **Step 5: Commit** `feat(config): export file, backup and restart-safe import save`.

### Task 3: The `--imported` start

**Files:**
- Modify: `src/main.rs` (`opens_windows`, the `match` in `main`), `src/app.rs` (`run`, `claim_wait`), `src/app_update.rs` (start notice), `src/i18n.rs`, `src/config_transfer.rs`

**Interfaces:**
- Produces:
  - `pub const IMPORTED_FLAG: &str = "--imported";` and `pub fn launch_imported(exe: &Path) -> io::Result<()>` (spawns `exe --imported`) in `config_transfer.rs`.
  - `app::run(media_log_flag: bool, start: Start)` with `pub enum Start { Normal, Updated(String), Imported }`; `claim_wait(&Start) -> Duration` is `UPDATE_CLAIM_WAIT` for `Updated` and `Imported`, `ZERO` for `Normal`. Callers in `main.rs` updated.
  - `update_start` takes `&Start`; `Imported` shows `info_notice` titled `Key::SettingsImported` ("Pengaturan diimpor" / "Settings imported"), empty body.

- [ ] **Step 1: Write the failing tests**: in `app.rs` tests `claim_wait_waits_after_an_import` (`Imported` and `Updated(_)` → `UPDATE_CLAIM_WAIT`, `Normal` → zero); in `main.rs` tests (or the nearest test module that sees `opens_windows`) `imported_start_opens_windows`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**; `--imported` behaves exactly like a normal start apart from the wait and the notice.
- [ ] **Step 4: Run** `cargo test --locked` → PASS; `cargo build --release --locked` → builds.
- [ ] **Step 5: Commit** `feat(app): --imported start waits for the old island and says so`.

### Task 4: Island: dialogs, pending import, confirm, restart

**Files:**
- Create: `src/app_transfer.rs` (`impl App` block + pure helpers + tests), registered like the other `app_*.rs` modules
- Modify: `src/ui/settings.rs` (`Action` variants, `Model` field, two dialog helpers), `src/app.rs` (`on_settings_action` arms, `settings_model`, `App` field), `src/i18n.rs`

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces (Settings side, in `src/ui/settings.rs`):
  - `Action::ConfigExport`, `Action::ConfigExportTo(PathBuf)`, `Action::ConfigImport`, `Action::ConfigImportFrom(PathBuf)`, `Action::ConfigImportConfirm`, `Action::ConfigImportCancel`.
  - `pub fn pick_export_path(owner: HWND, default_name: String)`: `IFileSaveDialog` on a short-lived STA thread like `pick_folder_for`, default folder Documents (`FOLDERID_Documents`), file types "JSON (*.json)" then "All files (*.*)", default extension `json`; posts `ConfigExportTo`.
  - `pub fn pick_import_path(owner: HWND)`: `IFileOpenDialog` with the same filters; posts `ConfigImportFrom`.
  - `Model.transfer: TransferView` with `pub enum TransferView { Idle, Exported(String /*file name*/), ExportFailed, Refused(String /*message*/), Pending { app_version: String, date: Option<NaiveDate> } }`.
- Island (`app_transfer.rs`): `App.transfer: Transfer { view: TransferView, pending: Option<Incoming> }`; handlers:
  - `ConfigExport` → `pick_export_path(self.hwnd, default_export_name(lang, today))`.
  - `ConfigExportTo(p)` → `self.save_config()`, `write_export(&p, &self.config, now)`; view `Exported(file name)` or `ExportFailed`; `sync_settings`.
  - `ConfigImport` → `pick_import_path(self.hwnd)`.
  - `ConfigImportFrom(p)` → read at most `MAX_BYTES + 1` bytes, `read_incoming`; `Err(NotVale)` → `Refused(i18n NotValeFile)`, `Err(Newer{v})` → `Refused(i18n newer(v))`, `Ok(inc)` → `pending = Some(inc)`, view `Pending{…}`; unreadable file → `Refused(NotValeFile)`.
  - `ConfigImportCancel` → `pending = None`, view `Idle`.
  - `ConfigImportConfirm` → `import_gate(pending.is_some(), self.rec.engaged(), self.screen.busy())`: `Gate::Nothing` → ignore; `Gate::Busy` → `Refused(i18n StopRecordingFirst)` keep pending; `Gate::Go` → `backup_config(&config::path(), now)`, `let new = import_and_save(&config::path(), &self.config, &inc)`, `self.adopt_config(new)`, then `current_exe()` + `launch_imported`: `Ok` → `DEFERRED_SHUTDOWN.set(true)`; `Err` → notice `SettingsImportedRestart` and view `Idle`.
  - `pub(super) fn import_gate(has_pending: bool, audio_busy: bool, screen_busy: bool) -> Gate` with `enum Gate { Nothing, Busy, Go }`.
- Copy (`i18n`, exact): `NotValeFile` "Ini bukan file pengaturan Vale." / "This is not a Vale settings file."; `fn settings_file_newer(lang, v)` "File ini dari Vale yang lebih baru (versi {v}). Perbarui Vale dulu." / "This file comes from a newer Vale ({v}). Update Vale first."; `StopRecordingFirst` "Hentikan rekaman dulu" / "Stop the recording first"; `SettingsImportedRestart` "Pengaturan diimpor. Mulai ulang Vale supaya semuanya berlaku." / "Settings imported. Restart Vale to apply everything."

- [ ] **Step 1: Write the failing tests** in `app_transfer.rs`: `import_gate_refuses_while_recording` (audio busy → Busy, screen busy → Busy, none pending → Nothing, else Go); `view_for_error` maps `NotVale` and `Newer{"6.0.0"}` to the exact strings in both languages; `newer_message_has_no_em_dash`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** the actions, handlers, dialogs and `settings_model` field.
- [ ] **Step 4: Run** `cargo test --locked` → PASS.
- [ ] **Step 5: Commit** `feat(settings): export and import settings in the island`.

### Task 5: Settings → Tentang card

**Files:**
- Modify: `src/ui/settings_layout.rs` (`about`, `Hit`, layout tests), `src/ui/settings.rs` (click handling, section leave and close), `src/i18n.rs`

**Interfaces:**
- Consumes: Task 4's `Action`s and `Model.transfer`.
- Produces: `Hit::ConfigExport`, `Hit::ConfigImport`, `Hit::ConfigImportConfirm`, `Hit::ConfigImportCancel`.
- Card titled `Key::GroupMoveSettings` "Pindah ke PC lain" / "Move to another PC", under "Tautan", two rows (`ROW_H` each, divider between):
  - Row 1 label "Ekspor pengaturan" / "Export settings"; description by `transfer`: `Exported(n)` → "Tersimpan: {n}" / "Saved: {n}", `ExportFailed` → "Gagal menyimpan" / "Could not save" (error tone), else "Simpan pengaturan umum ke satu file" / "Save the general settings to one file"; button `Secondary` "Ekspor…" / "Export…".
  - Row 2, not pending: label "Impor pengaturan" / "Import settings"; description `Refused(m)` → `m` (error tone) else "Ganti pengaturan di PC ini dengan isi file" / "Replace this PC's settings with a file"; button `Secondary` "Impor…" / "Import…".
  - Row 2, pending: description "Dari Vale {v}, {i18n::short_date(date)}. Pengaturan umum diganti, lalu Vale mulai ulang." / "From Vale {v}, {date}. The general settings are replaced, then Vale restarts." (without date: "Dari Vale {v}." / "From Vale {v}." then the same sentence); buttons "Batal" / "Cancel" (`Secondary`) then "Impor & mulai ulang" / "Import & restart" (primary fill, `primary_fill` per `light-accent-contrast`). The description wraps to two lines if it does not fit; the row grows to fit, like the other wrapping rows.
  - Leaving Tentang or closing Settings with a pending import posts `ConfigImportCancel`.
- [ ] **Step 1: Write the failing tests** in `settings_layout.rs` tests: `about_has_the_move_card_with_two_rows` (hits present, in order, card titled); `pending_import_shows_confirm_and_cancel` (both hits, confirm right-most); `long_confirm_text_fits_in_the_row_at_720` (no text rect wider than the content column at 720x520, both languages); update `exactly_the_overflowing_sections_get_a_viewport` / `everything_fits_in_the_window_height` to the measured result (Tentang scrolls at 720x520 only if it overflows); `every_card_of_every_section_has_a_title_but_the_previews` still passes. In `settings.rs` tests (or a pure helper): leaving the section with a pending import yields `ConfigImportCancel`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** layout, clicks, cancel-on-leave; check contrast of the error tone and the primary button text in light and dark with `check_contrast` (≥ 4.5:1).
- [ ] **Step 4: Run** `cargo test --locked` → PASS; `cargo build --release --locked`. The `--settings` demo is debug-only and has no island, so add `DI_TRANSFER=exported|failed|refused|newer|pending` read only by that demo's model, and screenshot `cargo run -- --settings tentang` with each value, `DI_LANG=id` and `en`, light and dark (existing demo env vars for theme).
- [ ] **Step 5: Commit** `feat(settings): Pindah ke PC lain card in Tentang`.
