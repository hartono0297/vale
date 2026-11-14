---
tags: [screenshots, convention]
date: 2026-10-27
---

# Screenshot capture script

`scripts/capture-screenshots.ps1` regenerates `assets/screenshots/*.png` from the debug build (`DI_LANG=en`) via the demo modes in `src/ui/demo.rs`. It copies each window over black then white and derives alpha from the difference, so rounded shapes stay transparent. Options: `-Only`, `-Out`, `-Window dark|light`.

- Calls SetProcessDPIAware first, see [[studio-editor-frame]].
- Demo data depends on the clock, see [[demo-data-time-of-day]].
- `lyric-long` freezes the long lyric line mid-scroll (`DI_LYRIC_SCROLL`), see [[lyric-notch-width]].
- `system` and `system-warning` (`notch:bata-sistem`) use fixed figures, never this machine's; see [[system-page]].
- Demo titles (events, notes, reminders) stay Indonesian even with `DI_LANG=en`.
- `cleaner`, `cleaner-confirm` and `cleaner-done` show the cleaner with the design's fixed sizes; the demo never scans or deletes, see [[cleaner]].
- `cleaner-mole`, `-confirm`, `-log`, `-result`, `-missing` and `cleaner-mole-done` (`notch:mole-done`) show the Mole mode with fixed data; Mole is never looked for or run, see [[mole-mode]].
- Clipboard shots: `clipboard` (the design's six cards of 48), `clipboard-search`
  ("denah" typed), `clipboard-copied` (`notch:clipboard-copied`, Bata Happy) and
  `settings-clipboard`. Their data and "now" are fixed (`demo_clips`), and the
  demo never touches the real clipboard, see [[clipboard-privacy]]. The scenes
  `clipboard-empty` and `clipboard-off` exist in `--demo` but are not shot.
- Screenshot preview: `screenshot`, `screenshot-saved`, `screenshot-deleted`
  (`notch:screenshot…`) with a drawn picture and fixed 1280 × 720 · 245 KB,
  and `settings-screenshot` (Clipboard scrolled to its end); never a real
  capture, clipboard or folder, see [[screenshot-preview]].
- The capture copies the screen behind the window too when the backdrop
  does not cover it: look at the PNGs and delete any that show your desktop.
- On a locked PC `CopyFromScreen` returns the lock screen, so every shot is the
  same 620 × 460 wallpaper. `PrintWindow(hwnd, hdc, 2)` on the demo window
  still draws the island (560 × 290; 580 × 290 in Notch), see
  [[shelf-image-preview]].
- **Quit the installed Vale first** (`Stop-Process -Name vale`, start it again
  after): while it runs, every island shot came out 620 × 460 with the
  wallpaper in it (2026-10-29). Right sizes are 560 × 290 for a page,
  384 × 76 for a notice.
