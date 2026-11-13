---
tags: [decision, settings, config, privacy]
date: 2026-11-13
---

# Export and import settings (Pindah ke PC lain)

Settings → Tentang has a card **Pindah ke PC lain** / Move to another PC with
*Ekspor…* and *Impor…*. Spec: `docs/superpowers/specs/2026-11-13-settings-export-import-design.md`.
The owner wanted only general settings to move ("hanya config2 nya aja … yang
bersifat private jangan"): no sticky notes, reminders, histories, Google login.

- **File:** plain JSON `{"vale_settings": 1, "app_version", "exported",
  "settings": {…}}`, max 1 MB, default `Vale-pengaturan-YYYY-MM-DD.json` /
  `Vale-settings-…` in Documents. `vale_settings` > 1 is refused ("Perbarui
  Vale dulu").
- **What goes in** is an allowlist in `src/config_transfer.rs` (`EXPORTED`,
  plus per-parent key lists for `recording`, `transcript`, `captions`,
  `arrange`). Left out: Google client ID/secret, `muted_apps`, own tiles,
  saved arrangements, recording folders and devices, `notch_monitor`, window
  places, one-install bookkeeping (`onboarding_done`, `last_greeting`,
  `folders_split`, `setup_offered`, …). `phone_net` is exported.
  `every_config_key_is_classified` fails on any new `Config` key until it is
  put in one of the two lists: **a new setting must be classified**.
- **Tiles:** only built-in ids are taken (`{tile, on}`), never a tile with a
  kind or target, so a file cannot add something that launches. The target's
  own tiles go after them; if more than `MAX_SHOWN` would show, imported
  built-ins are switched off from the end.
- **Merge** (`apply`, `merge_node`): key by key over the target's values,
  recursing into objects (`recording.sources`, `hotkeys` rows). A key missing
  from the file keeps the target's value, an unknown key is ignored, a value
  that does not read back exactly as written keeps the target's value (a
  normalised or clamped number costs only that leaf). An object first tries
  all its keys at once (so swapped hotkeys import), then key by key. The final
  parse is strict: a merged config that still fails refuses the import. Why:
  [[config-parse-default-fallback]].
- **Confirm:** refused while an audio or screen recording runs (the
  confirmation stays, with "Hentikan rekaman dulu"). Then `backup_config`
  (`config-before-import-YYYYMMDD-HHMMSS.json` in the data folder; a failed
  backup stops the import), `import_and_save` (merged truth from
  `save_merged`, file applied, `save_to`; a failed write stops the import and
  restarts nothing), `adopt_config`, `launch_imported` and
  `DEFERRED_SHUTDOWN`. The new process starts with `--imported`
  (`app::Start::Imported`), waits for the mutex like `--updated`, and says
  "Pengaturan diimpor".
- **State:** the island owns `Transfer { view, pending }`; `pending` is set
  exactly when the view is `Pending`. Settings only renders `Model.transfer`
  and posts actions; closing Settings (any section) clears it.
- Tentang scrolls at 720x520 since this card ([[settings-structure]]).
- Demo: `DI_TRANSFER=exported|failed|refused|newer|pending|busy` with
  `--settings tentang` (debug only).

Not checked live: the real dialogs, an import on another PC or account, the
restart. Deferred: a future hotkey row that clashes with a target row the file
lacks also drops swapped rows (needs a shrinking all-at-once check); duplicate
built-in tiles from a hostile file are kept in config.json (hidden on screen).
