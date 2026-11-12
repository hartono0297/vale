---
tags: [decision, recording, screen-recording, settings]
date: 2026-11-12
---

# Screen recording quality: a cap on the shorter side, with a size estimate

Settings → Fitur → Rekam layar starts with **Kualitas video** / Video quality:
Asli (Original) · 1440p · 1080p · 720p, default Asli (the video is the recorded
thing's own size, as before). Config `recording.video_quality`: `"original"`,
`"1440p"`, `"1080p"`, `"720p"`, anything else reads as original
(`VideoQuality` in `recorder/prefs.rs`). Spec
`docs/superpowers/specs/2026-11-12-recording-quality-design.md`, plan
`docs/superpowers/plans/2026-11-12-recording-quality.md`. Related:
[[screen-recording]], [[settings-structure]], [[exe-size]],
[[recording-hand-checks]].

## Decisions

- **"Quality" is resolution only.** The bitrate rule (`plan::bitrate`, 0.08 bit
  per pixel per frame) is unchanged, so 1080p is as sharp per pixel as before.
  The owner asked for "pilihan quality" plus a size estimate and left the rest
  to the builder.
- **The cap is on the shorter side** (`plan::output_size(w, h, max_short)`,
  `Options::max_short`, renamed from the unused `max_height`): 1080p on
  3440×1440 is 2580×1080, on a portrait 1440×2560 screen 1080×1920. Never
  scaled up: a 900×600 window at 1080p stays 900×600. The encoder cap
  (4096×2304) and the 64 px minimum still apply.
- **Snapshotted at start** (`StartJob.quality` → `session::options_for`) and
  refused while a recording is busy, exactly like 30/60 fps
  (`screen_set_quality` mirrors `screen_set_fps`). The refusal is silent for
  both controls; a "busy" state in Settings is a possible follow-up.
- **The estimate** is the row's description, `2560×1440 · ± 64 MB/menit`
  (`i18n::video_estimate_line`, `plan::estimate_mib_per_minute`): what the
  primary monitor (else the first in Settings' monitor list, else 1920×1080)
  records at the chosen quality and fps, video bitrate plus the 96 kbit/s
  audio, MB = 1,048,576 bytes, rounded. It is a ceiling: the encoder is
  unconstrained VBR, and a mostly still screen came out far smaller in the
  live check (3 s of a quiet desktop: 0.16 MB at 720p, 0.4 MB at 2560×1440).
- **Fit:** when the line is wider than its box it wraps at " · " into
  `{w}×{h}` and `± {n} MB/{unit}`, never between number and unit. With the
  real font it is one line in both languages, 3-digit figures included; only
  the pessimistic test measure wraps the English 3-digit case.

## Live check (2026-11-12, branch build on the owner's PC)

`vale --screenrec-test <s> --quality <q>` (debug builds) on the 2560×1440
primary: 720p → 1280×720, 1080p → 1920×1080, original → 2560×1440, 1440p at
60 fps → 2560×1440 at 60.02 fps; a 900×600 region at 1080p stayed 900×600, a
600×1200 region at 720p stayed 600×1200. Settings demo screenshots in both
languages (30/60 fps, Asli and 720p): the estimate follows the choice and fits.
The Settings demo (`--settings`) is a debug-build mode only; the release exe
says "unknown argument".
