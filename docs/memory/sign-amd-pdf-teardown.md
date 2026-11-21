---
tags: [gotcha, sign, tests, windows, gpu]
date: 2026-11-21
---

# Gotcha: Direct2D then Windows.Data.Pdf fail-fasts at exit (AMD)

A process that creates a Direct2D device and then uses Windows.Data.Pdf dies with 0xC0000409 during DLL teardown at exit (RIP in `amdxx64.dll`, stack d3d11, Windows.Data.Pdf). Not a Rust abort, not a COM imbalance. Order matters: PDF first then Direct2D exits clean; Direct2D first then PDF crashes. Any module triggers it (`shot_edit` too).

- **The app is safe:** `app::end_process` drops the D2D holders and ends with `TerminateProcess`, skipping DLL detach. `--demo-sign` must end the same way.
- **Test binary:** `src/sign/render.rs` has a `#[cfg(test)]` `.CRT$XCU` pre-main hook that renders one PDF page on its own thread (15 s wait, one stderr line on failure) so the PDF device exists first.
- **rtk hides the exit code.** Check with `rtk bypass cargo test --locked` from PowerShell and read `$LASTEXITCODE`. Git Bash shows this crash as exit 127.

Related: [[tanda-tangan]], [[test-spawns-no-window]].
