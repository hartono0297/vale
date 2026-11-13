---
tags: [spec, settings, config]
date: 2026-11-13
---

# Export and import settings

## Why

The owner wants to move their Vale setup to another PC: export the settings
to one file, import it on the other PC, and Vale looks and behaves the same
without setting everything by hand again.

Owner's words: "gw mau kita bisa import/export config jadi kalau mau pindahin
config ke pc lain gampang". On what goes in: "isi jangan dong, hanya config2 nya
aja, kalau isi sticky notes jangan" and "intiny semua config yang bersifat
umum, yang bersifat private jangan." The owner left the approach to the agent
("u atur yang terbaiknya gimana").

## Goals

- One button exports the general settings to a single readable file.
- One button imports such a file on another PC (or another Windows account),
  replaces the general settings there, and Vale restarts with them.
- Nothing private and nothing that only fits the source PC leaves in the file.
  The target PC keeps its own values for those.

## Non-goals

- No content: no sticky notes, reminders, clipboard history, notification
  history, transcripts, recordings, lyrics cache, transcription models.
- No Google login. The token is DPAPI-sealed to one Windows account; the user
  signs in again on the new PC.
- No sync, no cloud, no automatic transfer. A file the user carries.
- No partial import picker ("only hotkeys"). All general settings or nothing.

## Anti-goals (a failure even if it works)

- The export file holds anything private: the own Google Client ID or secret,
  file paths with the user name, app names from the notification exclusions,
  exe paths and window titles from saved arrangements.
- An import silently loses the target PC's own values (its monitor, folders,
  Google client, exclusions, own tiles).
- A broken or hostile file crashes Vale, half-applies, or leaves a broken
  `config.json`.
- An import cuts a running recording.
- A setting added later lands in (or out of) the export without anyone
  deciding.

## Approaches

1. **Buttons in Settings, Vale restarts after import (chosen).** Export writes
   one `.json` file. Import validates the file, asks once, keeps a backup of
   the current config, applies the general fields, saves, and restarts Vale
   the way the self-updater already does. A restart re-applies everything at
   one point (hotkeys, monitor, theme, font, mascot, autostart, language)
   through the normal startup path, so no setting needs its own "apply
   imported value" code.
2. **Buttons in Settings, applied live without restart.** Smoother, but every
   setting needs its own wiring to the code that applies it, it is easy to
   miss one, and every future setting has to remember it. Rejected.
3. **Command line only** (`vale --export`, `vale --import`). Least work, poor
   to use day to day. Rejected.

The owner handed the choice to the agent; 1 was picked.

## What goes in the file

The rule: a field goes in when it is a preference that makes sense on any PC
and says nothing private. Everything else stays out, and on import the
target keeps its own value for it.

### Exported (general)

Top-level `config.json` keys, same names and shapes as in `config.json`:

- `style`, `language`, `accent`, `surface`, `font`, `window_theme`
- `notch_width`, `notch_height`, `notch_fill`, `notch_opacity`, `idle_clock`,
  `stay_behind`, `open_mode`, `gestures`, `scroll_volume`, `lyrics`,
  `eq_follow_audio`, `notch_peek`
- `robot`, `robot_animation`, `robot_sleep_minutes`, `morning_greeting`,
  `mascot`, `cat_color`, `falk_color`, `octopus_color`, `cat_accessory`,
  `cat_follow_pointer`, `cat_dance`
- `start_with_windows`, `battery_threshold`, `disk_warn_gb`,
  `ram_warn_percent`, `update_check`, `device_notices`
- `pomodoro`, `clipboard`, `screenshot`, `widgets`, `hotkeys`, `panel_pages`
- `cli_shell`, `cli_start`, `clean_mode`, `mole_protect_defaults`
- `notifications` (its own settings: record, max items, persist, paused,
  toast style; the exclusion list is the separate top-level `muted_apps`,
  which stays out)
- `shortcut_tiles`: **built-in tiles only** (their order and whether each is
  shown). Own tiles (with a label, kind and target) are left out.
- `arrange`: `gap` and `templates` only (layouts, no paths). `saved` is left
  out.
- `recording`: `sources`, `echo_cancel`, `video_fps`, `video_quality`,
  `cursor_highlight`, `click_ripples`.
- `transcript`: `enabled`, `model`, `language`, `auto`.
- `captions`: `mic`, `notch`, `on_battery`, `language`, `size`.

### Left out (private, or only fits the source PC)

- `google_client_id`, `google_client_secret` (private)
- `muted_apps` (names the apps the user runs)
- own shortcut tiles, `arrange.saved` (paths, window titles)
- `recording.folder`, `recording.video_folder`, `recording.mic_device`,
  `recording.system_device` (paths with the user name, device ids)
- `notch_monitor` (a device path of one PC)
- `settings_window`, `lyrics_card`, `transcript.window`, `captions.place`
  (window places on one PC's screens)
- `onboarding_done`, `last_greeting`, `update_offered`, `mole_defaults_added`,
  `recording.consent_shown`, `transcript.setup_offered`, `media_log`
  (bookkeeping of one install)

### Every field is classified

A test lists every key `Config::default()` serialises (and every key of the
nested structs that are split: `recording`, `transcript`, `captions`,
`arrange`) and fails when one is neither in the exported list nor in the
left-out list. A new setting added later (the Tailscale phone link work is
adding some now) fails the test until someone decides where it goes.

## The file

Plain JSON, UTF-8, pretty printed, so it can be read and checked by eye.

```json
{
  "vale_settings": 1,
  "app_version": "5.3.0",
  "exported": "2026-11-13T14:05:00+07:00",
  "settings": { "...": "the exported fields only" }
}
```

- `vale_settings` is the file format version. This change writes 1.
- Default name `Vale-pengaturan-YYYY-MM-DD.json` (English UI:
  `Vale-settings-YYYY-MM-DD.json`), default folder Documents. Extension
  `.json`; the open dialog filters on `*.json` with "All files" as a second
  choice.

## Import rules

Reading:

- Larger than 1 MB, not UTF-8, not JSON, or no numeric `vale_settings` key:
  refused, "Ini bukan file pengaturan Vale." / "This is not a Vale settings
  file."
- `vale_settings` greater than 1: refused, "File ini dari Vale yang lebih baru
  (versi {app_version}). Perbarui Vale dulu." / "This file comes from a newer
  Vale ({app_version}). Update Vale first."
- `settings` missing or not an object: same as "not a Vale settings file".

Applying (pure function, unit tested):

- Start from the target's current config. For each **exported** key that is
  present in the file, take the file's value; a key missing from the file
  keeps the target's value. Keys not on the exported list are ignored even if
  someone wrote them into the file by hand (the own Google client in an
  edited file never gets in).
- Every value goes through the same lenient readers `config.json` already
  uses (`config::parse` on the merged JSON), so a bad value reads as that
  field's default instead of failing the whole file, exactly like a hand edit
  of `config.json` does today.
- Split objects merge field by field: `recording`, `transcript`, `captions`
  take only the exported fields and keep the target's folders, devices and
  window places; `arrange` takes `gap` and `templates` and keeps the target's
  `saved`.
- `shortcut_tiles`: the file's built-in entries in the file's order, then the
  target's own tiles appended after them, each keeping its shown/hidden flag.
- Unknown keys (from a newer Vale with the same format version) are ignored.

## Flow

### Where

Settings → Tentang, a new titled card **"Pindah ke PC lain"** / "Move to
another PC" under "Tautan", next to the data folder row it relates to. Two
rows:

- **Ekspor pengaturan** / Export settings. Description: "Simpan pengaturan
  umum ke satu file" / "Save the general settings to one file". Button:
  "Ekspor…" / "Export…".
- **Impor pengaturan** / Import settings. Description: "Ganti pengaturan di PC
  ini dengan isi file" / "Replace this PC's settings with a file". Button:
  "Impor…" / "Import…".

Tentang does not scroll today. If the card makes it overflow at the 720x520
minimum, it scrolls like the other long sections and the scroll tests are
updated (builder's call how; the layout rules in `settings-spacing` hold).

### Export

1. The save dialog opens with the default name and folder.
2. The island saves its config first, then the file is written from the
   island's current config through the export filter (write to `.tmp`, then
   rename, like `Config::save_to`).
3. The export row's description shows the result for this Settings session:
   "Tersimpan: Vale-pengaturan-2026-11-13.json" / "Saved: …", or "Gagal
   menyimpan" / "Could not save" on a write error. Cancelling the dialog
   changes nothing.

### Import

1. The open dialog picks a file.
2. The file is read and checked (rules above). A refusal shows its message in
   the import row's description in the error tone; nothing else changes.
3. A good file turns the import row into a confirmation, no timer: the
   description reads "Dari Vale {app_version}, {date}. Pengaturan umum diganti,
   lalu Vale mulai ulang." / "From Vale {app_version}, {date}. The general
   settings are replaced, then Vale restarts." with two buttons, "Impor &
   mulai ulang" / "Import & restart" (primary) and "Batal" / "Cancel".
   Leaving the section or closing Settings cancels it.
4. While an audio or screen recording runs, confirming is refused with the
   existing busy notice wording pattern: "Hentikan rekaman dulu" / "Stop the
   recording first", and nothing changes.
5. On confirm, the island:
   - copies the current `config.json` to
     `config-sebelum-impor-YYYYMMDD-HHMMSS.json` in the data folder (fixed
     name `config-before-import-…` regardless of language is fine too;
     builder's call, one name only);
   - applies the file (rules above) onto its in-memory config, so the
     teardown save writes the imported config and not the old one;
   - saves, starts a new Vale with a restart flag, and shuts down the same way
     the updater does (`DEFERRED_SHUTDOWN`, single-instance mutex released last).
6. The new Vale waits for the mutex like `--updated` does, starts normally
   (autostart registry, hotkeys, monitor, theme all come from the new config),
   and shows a notch notice: "Pengaturan diimpor" / "Settings imported".

If starting the new process fails, Vale does not shut down: the imported
config is already saved, and the notice says "Pengaturan diimpor. Mulai ulang
Vale supaya semuanya berlaku." / "Settings imported. Restart Vale to apply
everything."

## Constraints

- Rust 2024, Windows 11 only. File dialogs through the same `IFileOpenDialog`
  (and `IFileSaveDialog`) code style `src/ui/settings.rs` already uses.
- Tests never touch `%APPDATA%` or the real `config.json`; file tests use
  `target/test-tmp/` (see `clipboard-privacy`, `test-spawns-no-window`).
- No new timer; the confirmation needs none.
- Both languages for every string (`i18n`), no em dashes, labels follow the
  existing Settings voice.
- The Tailscale phone-link session may add config keys on `main` before this
  merges. The classification test will catch them at merge time; classify
  them then (a phone pairing secret or address is private, a plain on/off is
  general).

## Calls settled after the builder check (agent's calls)

- **How the import reaches disk.** `merge_owned` takes only the island-owned
  fields from memory; file-owned ones (`battery_threshold`, `disk_warn_gb`,
  `ram_warn_percent`, `gestures`, `scroll_volume`, ...) come from disk. So the
  import does not edit memory and hope: it takes the merged truth
  (`save_merged(mem).config`), applies the file onto that, writes the result
  with `Config::save_to`, and then sets the island's in-memory config to the
  same value. The teardown save then merges identical memory and disk and
  changes nothing. A test checks that a file-owned field from the file
  survives the restart sequence's saves.
- **What export reads.** The merged truth too (`SaveOutcome.config` of a save
  made just before), so a hand edit on disk and unsaved island state both
  land in the file.
- **Transcription model.** `transcript.model`, `enabled` and `auto` are
  exported; `setup_offered` stays the target's. Without the model on the
  target, the existing rule already handles it (`offer_for`): nothing is
  queued, nothing downloads by itself, and the one-time "Pengaturan" offer
  shows if the target has not used it.
- **Exclusions.** `muted_apps` is always the target's own list, no union.
- **Tiles from a file.** Only entries whose `tile` id is a known built-in
  shortcut are taken; any other entry is dropped, even with no label or
  target. A file never adds a tile that launches something.
- **Restart wait.** The restart flag shares the updater's wait
  (`UPDATE_CLAIM_WAIT`, 60 s) and its behaviour. The old island releases its
  mutex last in a teardown capped at 5 s, so the wait running out means
  something else holds Vale's mutex; quitting quietly then leaves that other
  Vale running, which is right.

## Left to the builder

- Module layout (a new `src/config_transfer.rs` or similar for the pure
  export filter, file format, import validation and merge; the dialogs and
  Settings rows in the UI code; the restart in the island).
- The restart flag's name and whether it shares `claim_wait` with `--updated`.
- Exact backup file name (one fixed name).
- How the Settings window gets the island's current config for export
  (`SettingsAction` round trip or a save-then-read), as long as unsaved island
  state is included.
- Whether Tentang scrolls after the new card, and the test updates that
  follow.

## Done means

- Unit tests: classification covers every key; export leaves out every
  private field; import keeps the target's private and PC-only fields;
  missing keys keep target values; hand-added private keys are ignored; bad
  values read as defaults; format version too new is refused; not-JSON,
  too large and missing `settings` are refused; tiles and arrange merge as
  specified; round trip export → import on a fresh config gives the same
  general settings.
- `cargo test --locked` green on `main` after the merge.
- Hand checks for the owner: export on this PC and open the file (nothing
  private in it); import on another PC or another Windows account; Vale
  restarts with the new look, hotkeys and mascot; the backup file exists;
  the other PC's monitor, folders and Google client are unchanged; a recording
  running blocks the import.
