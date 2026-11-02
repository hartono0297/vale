---
tags: [decision, claude, hooks, notifications, win32, uia, focus]
date: 2026-11-02
---

# Clicking a Claude notice goes to its session

A click on "Claude selesai" (`NoticeKind::ClaudeDone`) or "Claude butuh respon"
(`ClaudeWaiting`) brings the user to the Claude Code session it came from:
Claude Desktop's Code tab, Windows Terminal, the VS Code extension, Orca, any
other host. The notice is dismissed exactly as before; the new part is
`Effect::OpenClaudeSession { session_id }`, emitted by `state::dismiss_visible_notice`
(Done) and `state::dismiss_sticky` (Waiting, so the Body, Strip and ToastSlot
paths all emit it). `Event::ClaudeActivity` still clears a waiting notice with no
effect. `NoticeKind` did not change shape; the origin lives in a side map.

**Never launches anything.** No `claude://`, no `claude-cli://open`, no
`claude --resume`, no `--desktop --resume`, no new window, terminal or session,
no AUMID launch, no wake. Nothing found means nothing happens (the click only
dismisses). A hidden (tray) main window is `SW_SHOW`n, never relaunched.

## Capture (`vale --hook`, `claude/host.rs`)

Only for Done and Waiting. Activity does no extra work. `parse_hook_input`
stays pure (origin `None`); `run_hook` calls `host::capture()` and sets
`HookMessage.origin` (`claude/origin.rs`: `ClaudeOrigin`, every field
`#[serde(default)]`, so an older or newer line still parses). It prints nothing,
exits 0, has no unwrap on OS results (release is `panic = abort`). About 55 ms in
a debug build, measured with the ignored `capture_on_this_machine` test.

1. One Toolhelp snapshot; the ancestors of the hook (max 16, stops at a gap or
   a cycle; pid reuse is not detected).
2. One `EnumWindows` pass for the chain's pids: every window (Z order) that is
   visible, unowned, not `WS_EX_TOOLWINDOW`, titled, not Progman, WorkerW or the
   taskbar, with its title. The host's window is `pick_host_window` (pure,
   tested): VS Code and Windows Terminal run every window from one process and
   the frontmost is only where the user is now, so first a window with a
   " - " part equal to the project folder ("file - folder - Visual Studio
   Code"), then one naming it as a whole word, then the frontmost.
3. `choose_host` (pure, tested): the CLI is the first `claude.exe`, `node.exe` or
   `bun.exe`; the host is the first entry after it with a window that is not in
   the skip list (node, bun, cmd, powershell, pwsh, bash, sh, zsh, fish, nu, wsl,
   wslhost, conhost, openconsole, git-bash; `mintty.exe` is a host; a `claude.exe`
   after the CLI is Claude Desktop). `cli_direct`: only the host's own exe sits
   between the CLI and the host (VS Code extension host, Claude Desktop), no
   shell. `explorer.exe` never counts as having a window in `capture` (its windows
   are folders), so the console fallback applies.
4. Console: `AttachConsole(<CLI pid>)` first, then `ATTACH_PARENT_PROCESS`
   (just the parent when the parent is the CLI), `FreeConsole` after each; the
   first window and the first title that is not an executable's path win
   (`console_order`, `is_exe_path_title`, `gather_console`, pure and tested).
   Claude Code runs hook commands through a shell (Git Bash) that can sit in its
   own hidden, windowless console titled with the shell's path, so the parent's
   console alone gave the wrong title and no window. A console the process
   already had (`--probe-claude-open` attaches to print) is freed first and
   attached again at the end.
5. The console window (`console_target`, pure): a ConPTY `PseudoConsoleWindow`
   becomes its `GA_ROOTOWNER` when that is a visible window; a classic console
   counts only while visible (a hook shell or CLI started hidden is never the
   user's). It becomes the host when there is none, and replaces the host's
   window when it belongs to the host's pid (a WT tab's own window, not the
   one the user is in). The title is cut to 512 chars.
   `CLAUDE_CODE_HOST_SESSION_ID` is kept only if it matches
   `^[A-Za-z0-9_-]{1,100}$`.

The line stays far under the pipe's 64 KB `MAX_LINE` (tested with max fields).

## Side registry

`claude::origin::remember / forget / get`: `Mutex<VecDeque>`, 64 sessions, the
newest replaces the same session. It stores the origin and the project (the cwd
folder name, `HookMessage.project`) because the tab rule matches on it.
`pipe_server::read_event` fills it for Done and Waiting: an origin -> remember; a
line from an older hook binary (none) -> forget, so that click only dismisses.
Activity leaves it. `to_event` is unchanged and pure.

## The rules (`notif/claude_open.rs`, pure; first match wins)

1. **Desktop**: host exe `claude.exe`, or no host window and a host session id.
2. **VS Code**: `code.exe` (scheme `vscode`) or `code - insiders.exe`
   (`vscode-insiders`) and `cli_direct`. `code.exe` with a shell in between (the
   integrated terminal) gets Tabs. Other forks (Cursor, Windsurf) get Tabs.
3. **Tabs**: any other host with a window.
4. **WindowOnly**: an exe with no recorded window. **NoHost**: nothing recorded.

Window: the recorded hwnd while it still exists and belongs to the recorded pid,
else `pick_main_window` over the windows of a process with the same exe name
(for Desktop only Electron windows, so a console window of some `claude.exe` is
never taken for it). A hidden console window (`hidden_console`) is never taken,
so a click never shows a blank console an older hook recorded.

## Execution (`sources/claude_open.rs`, on the `notif-open` worker)

`notif_open` has one job channel, `Job::Toast | Job::Claude`, the same generation
counter and newest-job-wins loop; toast behaviour is unchanged. 1 s budget
through the same `Clock`, cancel checked between steps, the usual foreground rule
(`AllowSetForegroundWindow`, `SetForegroundWindow`, fallback `BringWindowToTop` +
`SwitchToThisWindow`; no `AttachThreadInput`, no keys).

- **Desktop**: window forward, then the session's `title` from
  `%LOCALAPPDATA%\Claude-Data\*\claude-code-sessions\*\*\<host_session_id>.json`
  (walked with `read_dir`, no glob; id validated first). That file is Desktop's
  private, undocumented storage: it is read only at click time, only the `title`
  field is parsed, nothing is kept or logged. Then UIA: every Button under the
  window in one cached `FindAllBuildCache`; `match_sidebar_row` takes a name equal
  to the title or the title and a space (rows read "<title> <subtitle>"),
  skipping "More options for ", "Hide nested sessions ", "New session in ". Only
  exactly one row is `Invoke`d (fresh 1 s timeouts first); zero or several: window
  only.
- **VS Code**: window forward; only once `GetForegroundWindow` is that window
  (up to 300 ms) `ShellExecuteW("open", vscode://anthropic.claude-code/open?session=<id>)`.
  The link acts in the *focused* VS Code window, which is why the window comes
  first. Not in front, no window: nothing.
- **Tabs**: window forward, `read_tabs` with a wider container set than the
  browser rule (also List 50008 and Document 50030: Windows Terminal's XAML
  `TabView` holds a `ListView`, Orca's tabs live in web content), then
  `pick_claude_tab`: cleaned console title equal (status glyphs and braille
  spinner stripped, case-insensitive); two equal titles is a tie and decides
  nothing; none equal -> exactly one tab with the project folder as a whole word
  ("api" is not "rapid"); not for an editor host (`tab_fallback_project`: VS Code
  and forks, JetBrains, Visual Studio, Sublime, Zed), whose UIA tree also holds
  the open files' tabs ("api.rs"), so there only the console title picks. Then
  `SelectionItemPattern.Select()`. No pick: the current tab stays.
- **WindowOnly**: window forward.

## Dry run

`<hook payload> | vale.exe --probe-claude-open` captures the origin exactly as the
hook would (the ancestors of the probe, so in a terminal the terminal is the
host) and prints what a click would do (rule, window, session title found or not,
buttons read and the row that would be invoked, tabs read and the one picked, the
link). No focus, no Invoke, no Select, no ShellExecute. `#[ignore]` tests
`plan_for_claude_*` call `plan` only:
`rtk bypass cargo test plan_for_claude -- --ignored --nocapture --test-threads=1`.

Seen on the author's desktop: a Stop payload piped into the probe from a Claude
Desktop Code tab gave host `claude.exe`, `cli_direct`, rule Desktop, the session
title found in the session file, 132 buttons read, one sidebar row matched.

## Assumptions and unverified

- Rule 1 also applies with no host window when a host session id exists.
- Code - Insiders uses `vscode-insiders`; other forks get Tabs.
- Windows Terminal's tabs: its UIA tree (`Tab` > `List` > ...) exposed no `TabItem`
  on the author's machine when checked, so a WT click may bring the window only
  (the tab is left alone, never a wrong one). Orca's tabs are unverified too.
- Nothing was clicked for real (no Invoke, Select or VS Code link was run); only
  plans, the pure rules and the Desktop dry run were exercised.
- Pid reuse in the ancestor walk, and a window of the recorded exe found by name
  when the recorded one is gone (a wrong VS Code or terminal window is possible).
- Default console titles that are just an exe path (the Git Bash path was seen
  from the hook's shell) are dropped. A probe from a Claude Code shell now gives
  `console title None` under Desktop. Sidebar rows of Desktop's Chat tab read
  "Idle <title>"; if Code-tab rows carry such a prefix, `match_sidebar_row`
  never matches (window only). Not confirmed with the Code tab visible.

Related: [[notification-open]] (same worker, foreground rule and budget), [[toast-while-busy]].
