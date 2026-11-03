---
tags: [gotcha, tests, windows]
date: 2026-11-03
---

# Tests open no window and spawn no console

`cargo test` must never put anything on screen: no Windows Terminal, cmd or
powershell window and no Vale window.

## Why it happened
`main.rs` has `#![windows_subsystem = "windows"]`, so the test binary has no
console. On Windows 11 every console child it spawns without
`CREATE_NO_WINDOW` gets a console of its own, which Windows Terminal hosts in a
new window (class `CASCADIA_HOSTING_WINDOW_CLASS`). Do not switch the test
binary to the console subsystem: with no console from cargo it would open one
itself.

## Rules
- A test that spawns a process uses `.creation_flags(CREATE_NO_WINDOW.0)`
  (`CommandExt`). Done in the `make_link` helpers (`mklink /J` fallback) of
  `clean.rs` and `search_files.rs`, and in the mole test
  `the_elevated_step_fails_when_the_script_fails` (real `powershell.exe`).
  `cli.rs` `spawn` and mole's `RealRunner` already used it.
  `open_whitelist` keeps `CREATE_NEW_CONSOLE` on purpose; no test reaches it
  (they use a fake runner).
- Test windows are created but never shown: `ui/editor.rs` has
  `const ON_SCREEN: bool = !cfg!(test)`, which gates `ShowWindow(SW_SHOW)` in
  `open_editor` and the `SetForegroundWindow` / `SwitchToThisWindow` block in
  `bring_to_front` (`focus_last` still runs). In the app it is the constant
  `true`. The tests `close_open_editor_returns_the_note_without_posting` and
  `reminder_editor_opens_without_windows_date_controls` assert
  `!IsWindowVisible(editor)`.
- Other tests use hidden or `HWND_MESSAGE` windows; `arrange_win`'s on-screen
  test is `#[ignore]`.
- A new test that needs a window or a child process follows the same rules.

Related: [[mole-mode]], [[cleaner]].
