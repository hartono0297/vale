---
tags: [decision, recording, files, migration]
date: 2026-11-13
---

# One recordings folder: `Audio` and `Video` subfolders

Since 2026-11-13 (branch `feat/rec-one-folder`) audio and screen recordings
share one parent folder. Before, audio went to `Documents\Vale\Rekaman` and
video to `Videos\Vale`, each with its own Settings row. The owner asked for
one folder with the two kinds apart in subfolders.

- **Model** (`recorder/prefs.rs`): `recording.folder` is the parent ("root",
  `None` = `Documents\Vale\Rekaman`). `dir()` = `root\Audio`, `video_dir()` =
  `root\Video` (`AUDIO_SUB`, `VIDEO_SUB`, same words in both languages).
  `recording.video_folder` is legacy: only the migration reads it, then it is
  `null`.
- **Fallback** ([[controlled-folder-access]]): the fallback root stays
  `%USERPROFILE%\Vale\Rekaman`; recordings fall back to its `Audio` / `Video`.
  A fall-back with no folder chosen sets `recording.folder` to the fallback
  root, so both kinds follow it.
- **Settings → Fitur:** one row "Folder rekaman" in the Rekaman card,
  description `<folder> · Audio & Video` (wraps at ` · ` only when it does
  not fit, `Builder::split_desc`, shared with Kualitas video). Open opens the
  root, Change picks the root. The Rekam layar card has no folder row. A
  folder change is refused while an audio or a screen recording is busy.
  The blocked-folder notice opens this one picker for both kinds.
- **The Rekam page** buttons open `Audio` on the Audio tab and `Video` on the
  Layar tab.

## The one-time move (`recorder/split.rs`, `App::rec_split_folders`)

Runs once at startup, before `transcribe_init()`, marked by
`recording.folders_split` (also set when nothing moved or a move failed).

- New root: the old `folder`; else the fallback root when the old
  `video_folder` was the old video fallback (`%USERPROFILE%\Vale\Video`);
  else the default.
- Audio in the root moves to `root\Audio`. Video moves to `root\Video` from
  the old video default, the old video fallback, or the root itself (one
  folder used for both). A different video folder the user picked is left
  alone (its videos are no longer listed).
- A recording is `<stem>.m4a` / `.mp4`, or a lone `<stem>.<ext>.part`; its
  companions (every `<stem>.*`: sidecar, cursor log, `.md`, `.srt`,
  `.transcript.*`) move with it. In a folder the user picked (not one of
  Vale's own), only Vale-made recordings move: a `<stem>.vale.json` beside
  it, or a stem starting with the `YYYY-MM-DD HHMM` stamp (`Move::vale_only`).
- Renames use `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`: a taken name
  is skipped, never overwritten. A failure (another drive, a locked file,
  Controlled folder access) leaves that file and only goes to stderr; there
  is no notice for skipped files.
- The old video folder is removed only if empty (`remove_dir`); the root is
  never removed.
- `queue.json` paths of moved recordings are rewritten (`rewrite_queue`),
  because `Queue::from_json` drops jobs whose file is gone.

Related: [[recording-audio]], [[screen-recording]], [[transcript]],
[[settings-structure]].
