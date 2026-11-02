//! `--hook` side of "click a Claude notice, go to its session": where does
//! this hook run? Walks the process ancestors (one Toolhelp snapshot), finds
//! the first one that owns a real top-level window and is not a shell or
//! console helper, and falls back to the CLI's (else the parent's) console
//! window. Prints nothing, never panics, launches nothing. See docs/memory/claude-notice-open.md.

use std::collections::HashMap;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, FreeConsole, GetConsoleCP, GetConsoleTitleW,
    GetConsoleWindow,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOTOWNER, GW_OWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetWindow,
    GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;

use super::origin::{ClaudeOrigin, clamp_title, sanitize_host_session_id};
use crate::notif::claude_open::contains_whole_word;

/// How many ancestors are looked at.
const MAX_DEPTH: usize = 16;

/// One process of the chain, nearest ancestor first.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcEntry {
    pub pid: u32,
    /// Lowercase image file name.
    pub exe: String,
    /// Owns a visible, unowned, titled top-level window.
    pub has_window: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HostPick {
    /// Index into the chain.
    pub index: usize,
    pub cli_direct: bool,
}

/// One row of the process table.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcRow {
    pub pid: u32,
    pub ppid: u32,
    pub exe: String,
}

/// The claude CLI itself: its own binary, or the node / bun runtime running it.
fn is_cli(exe: &str) -> bool {
    matches!(exe, "claude.exe" | "node.exe" | "bun.exe")
}

/// Shells: a process of one between the CLI and a host means a terminal tab.
fn is_shell(exe: &str) -> bool {
    matches!(
        exe,
        "cmd.exe"
            | "powershell.exe"
            | "pwsh.exe"
            | "bash.exe"
            | "sh.exe"
            | "zsh.exe"
            | "fish.exe"
            | "nu.exe"
            | "wsl.exe"
    )
}

/// Never the host: runtimes, shells and console helpers. (A `claude.exe` after
/// the CLI entry is not here: with a window it is Claude Desktop.)
fn is_skipped(exe: &str) -> bool {
    is_shell(exe)
        || matches!(
            exe,
            "node.exe"
                | "bun.exe"
                | "wslhost.exe"
                | "conhost.exe"
                | "openconsole.exe"
                | "git-bash.exe"
        )
}

/// The host of a hook, from its ancestors (`chain[0]` is the hook's parent):
/// the first entry after the CLI (from the start when no CLI is found) that
/// has a window and is not skipped. `cli_direct`: between the CLI and the host
/// there is only the host's own exe (a VS Code extension host, Claude Desktop),
/// no shell; false without a CLI entry.
pub fn choose_host(chain: &[ProcEntry]) -> Option<HostPick> {
    let cli = chain.iter().position(|e| is_cli(&e.exe));
    let start = cli.map_or(0, |c| c + 1);
    let index =
        (start..chain.len()).find(|&i| chain[i].has_window && !is_skipped(&chain[i].exe))?;
    let cli_direct = cli.is_some_and(|c| {
        chain[c + 1..index]
            .iter()
            .all(|e| e.exe == chain[index].exe && !is_shell(&e.exe))
    });
    Some(HostPick { index, cli_direct })
}

/// The ancestors of `pid`, nearest first, from a process table: stops at a
/// missing parent, a cycle, pid 0 or `MAX_DEPTH`.
pub fn ancestor_chain(table: &[ProcRow], pid: u32) -> Vec<ProcRow> {
    let by_pid: HashMap<u32, &ProcRow> = table.iter().map(|r| (r.pid, r)).collect();
    let mut chain: Vec<ProcRow> = Vec::new();
    let mut seen = vec![pid];
    let mut next = by_pid.get(&pid).map(|r| r.ppid);
    while let Some(parent) = next {
        if chain.len() >= MAX_DEPTH || parent == 0 || seen.contains(&parent) {
            break;
        }
        let Some(row) = by_pid.get(&parent) else {
            break;
        };
        seen.push(parent);
        chain.push((*row).clone());
        next = Some(row.ppid);
    }
    chain
}

// -------------------------------------------------------------------- Win32

fn process_table() -> Vec<ProcRow> {
    let mut rows = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return rows;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let name = &entry.szExeFile;
                let n = name.iter().position(|c| *c == 0).unwrap_or(name.len());
                rows.push(ProcRow {
                    pid: entry.th32ProcessID,
                    ppid: entry.th32ParentProcessID,
                    exe: String::from_utf16_lossy(&name[..n]).to_lowercase(),
                });
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    rows
}

/// Classes of the desktop and taskbar: never an app's window.
const SHELL_CLASSES: [&str; 4] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

struct Scan {
    pids: Vec<u32>,
    found: HashMap<u32, Vec<(i64, String)>>,
}

/// Per pid of `pids`, its top-level windows that are visible, unowned, not a
/// tool window and titled, most recent in Z order first, with their titles.
/// (For another process's window `GetWindowTextW` reads the stored caption
/// and sends no message, so a hung host cannot stall the hook.)
fn windows_of(pids: &[u32]) -> HashMap<u32, Vec<(i64, String)>> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let scan = unsafe { &mut *(lparam.0 as *mut Scan) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if scan.pids.contains(&pid) && unsafe { real_window(hwnd) } {
            let mut text = [0u16; 512];
            let n = unsafe { GetWindowTextW(hwnd, &mut text) }.max(0) as usize;
            let title = String::from_utf16_lossy(&text[..n.min(text.len())]);
            scan.found
                .entry(pid)
                .or_default()
                .push((hwnd.0 as isize as i64, title));
        }
        true.into()
    }
    let mut scan = Scan {
        pids: pids.to_vec(),
        found: HashMap::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM((&raw mut scan) as isize));
    }
    scan.found
}

/// Which of the host's windows (Z order, frontmost first) holds the session
/// when the console does not say: a host such as VS Code runs every window
/// from one process, and its frontmost one is only where the user is now.
/// VS Code titles read "file - folder - Visual Studio Code", so first a
/// window with a " - " part equal to the project folder (a leading dirty dot
/// ignored), then one naming it as a whole word, then the frontmost.
/// Case-insensitive; None for no windows.
pub fn pick_host_window<W: Copy>(wins: &[(W, String)], project: &str) -> Option<W> {
    let project = project.trim().to_lowercase();
    let rank = |title: &str| -> u8 {
        if project.is_empty() {
            return 0;
        }
        let lower = title.to_lowercase();
        if lower
            .split(" - ")
            .any(|part| part.trim().trim_start_matches('\u{25cf}').trim() == project)
        {
            2
        } else if contains_whole_word(&lower, &project) {
            1
        } else {
            0
        }
    };
    let best = wins.iter().map(|(_, t)| rank(t)).max()?;
    wins.iter().find(|(_, t)| rank(t) == best).map(|(w, _)| *w)
}

unsafe fn real_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool()
            || GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid())
            || GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0
            || GetWindowTextLengthW(hwnd) <= 0
        {
            return false;
        }
        let mut name = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut name).max(0) as usize;
        let class = String::from_utf16_lossy(&name[..n.min(name.len())]);
        !SHELL_CLASSES.contains(&class.as_str())
    }
}

/// A console to read, in the order `console_info` tries them.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ConsoleOf {
    /// The CLI's own console: the one the session's terminal shows and titles.
    Cli(u32),
    /// The hook's parent's console (`ATTACH_PARENT_PROCESS`). Claude Code runs
    /// hook commands through a shell that can sit in its own hidden,
    /// windowless console titled with the shell's path, so it is only a
    /// fallback.
    Parent,
}

/// The CLI's console first, then the parent's; the parent alone when it is the
/// CLI itself or no CLI was found.
fn console_order(cli_pid: Option<u32>, parent_pid: Option<u32>) -> Vec<ConsoleOf> {
    match cli_pid {
        Some(cli) if parent_pid != Some(cli) => vec![ConsoleOf::Cli(cli), ConsoleOf::Parent],
        _ => vec![ConsoleOf::Parent],
    }
}

/// A console's default title: just an executable's path ("C:\Program
/// Files\Git\usr\bin\bash.exe", "Administrator: C:\WINDOWS\system32\cmd.exe").
/// Says nothing about the session and would only match a plain shell tab.
fn is_exe_path_title(title: &str) -> bool {
    let t = title.trim();
    let t = t
        .strip_prefix("Administrator:")
        .unwrap_or(t)
        .trim()
        .trim_matches('"');
    t.to_ascii_lowercase().ends_with(".exe")
        && (t.contains(['\\', '/']) || !t.contains(char::is_whitespace))
}

/// Reads the consoles in `order` (`read` attaches to one and returns its
/// window and title, `None` when it cannot attach) until both are known: the
/// first window found, the first title that is not an executable's path.
fn gather_console<W>(
    order: &[ConsoleOf],
    mut read: impl FnMut(ConsoleOf) -> Option<(Option<W>, Option<String>)>,
) -> (Option<W>, Option<String>) {
    let (mut window, mut title) = (None, None);
    for &target in order {
        if window.is_some() && title.is_some() {
            break;
        }
        let Some((w, t)) = read(target) else {
            continue;
        };
        window = window.or(w);
        title = title.or(t.filter(|t| !is_exe_path_title(t)));
    }
    (window, title)
}

/// What the CLI's console (else the hook's parent's) says: its window (a
/// ConPTY pseudo window resolved to the terminal window that owns it) and its
/// title. Attaches to each console and lets go again; a console this process
/// already had (`--probe-claude-open` attaches to its parent's to print) is
/// left first and attached again at the end. Stdin is already read and
/// nothing is written.
fn console_info(cli_pid: Option<u32>, parent_pid: Option<u32>) -> (Option<HWND>, Option<String>) {
    unsafe {
        // GetConsoleCP fails (0) without a console.
        let had_console = GetConsoleCP() != 0;
        if had_console {
            let _ = FreeConsole();
        }
        let found = gather_console(&console_order(cli_pid, parent_pid), |target| {
            let attached = match target {
                ConsoleOf::Cli(pid) => AttachConsole(pid),
                ConsoleOf::Parent => AttachConsole(ATTACH_PARENT_PROCESS),
            };
            attached.ok()?;
            let console = GetConsoleWindow();
            let window = (!console.is_invalid())
                .then(|| terminal_window(console))
                .flatten();
            let mut buf = [0u16; 1024];
            let n = GetConsoleTitleW(&mut buf) as usize;
            let title = clamp_title(&String::from_utf16_lossy(&buf[..n.min(buf.len())]));
            let _ = FreeConsole();
            Some((window, title))
        });
        if had_console {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        found
    }
}

/// Class of a ConPTY console's stand-in window: hidden, owned by the terminal.
const PSEUDO_CONSOLE_CLASS: &str = "PseudoConsoleWindow";

/// A console window as the window to bring forward: a ConPTY pseudo window is
/// replaced by its root owner (the terminal), which must be a real visible
/// window; a classic console window is itself, only while it is visible (a
/// console started hidden, such as a hook shell's, was never the user's).
unsafe fn terminal_window(console: HWND) -> Option<HWND> {
    unsafe {
        let mut name = [0u16; 64];
        let n = GetClassNameW(console, &mut name).max(0) as usize;
        let class = String::from_utf16_lossy(&name[..n.min(name.len())]);
        let root = if class == PSEUDO_CONSOLE_CLASS {
            let root = GetAncestor(console, GA_ROOTOWNER);
            (!root.is_invalid()).then(|| (root, IsWindowVisible(root).as_bool()))
        } else {
            None
        };
        console_target(console, &class, IsWindowVisible(console).as_bool(), root)
    }
}

/// The pure part of `terminal_window`: `root` is the pseudo window's root
/// owner and whether that is visible.
fn console_target<W: Copy + PartialEq>(
    console: W,
    class: &str,
    console_visible: bool,
    root: Option<(W, bool)>,
) -> Option<W> {
    if class != PSEUDO_CONSOLE_CLASS {
        return console_visible.then_some(console);
    }
    root.filter(|&(r, visible)| r != console && visible)
        .map(|(r, _)| r)
}

/// Facts about where this hook runs, for the session in the `project`
/// folder. Called only for a Done or Waiting event. No panics, nothing printed.
pub fn capture(project: &str) -> ClaudeOrigin {
    let table = process_table();
    let ancestors = ancestor_chain(&table, unsafe { GetCurrentProcessId() });
    let pids: Vec<u32> = ancestors.iter().map(|r| r.pid).collect();
    let windows = windows_of(&pids);
    let chain: Vec<ProcEntry> = ancestors
        .iter()
        .map(|r| ProcEntry {
            pid: r.pid,
            exe: r.exe.clone(),
            // Explorer's windows are folders and the desktop, never a CLI's home.
            has_window: windows.get(&r.pid).is_some_and(|w| !w.is_empty())
                && r.exe != "explorer.exe",
        })
        .collect();
    let mut origin = ClaudeOrigin {
        host_session_id: sanitize_host_session_id(
            std::env::var("CLAUDE_CODE_HOST_SESSION_ID").ok(),
        ),
        ..Default::default()
    };
    if let Some(pick) = choose_host(&chain) {
        let host = &chain[pick.index];
        origin.host_pid = host.pid;
        origin.host_exe = host.exe.clone();
        origin.host_hwnd = windows
            .get(&host.pid)
            .and_then(|w| pick_host_window(w, project))
            .unwrap_or(0);
        origin.cli_direct = pick.cli_direct;
    }
    let cli_pid = chain.iter().find(|e| is_cli(&e.exe)).map(|e| e.pid);
    let parent_pid = chain.first().map(|e| e.pid);
    let (console_window, title) = console_info(cli_pid, parent_pid);
    origin.console_title = title;
    if let Some(hwnd) = console_window {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if origin.host_pid == 0 {
            origin.host_pid = pid;
            origin.host_exe = crate::sources::notif_open::image_name(pid);
            origin.host_hwnd = hwnd.0 as isize as i64;
        } else if pid != 0 && pid == origin.host_pid {
            // The console names the session's own window (a ConPTY tab's
            // terminal window), not just the host's frontmost one.
            origin.host_hwnd = hwnd.0 as isize as i64;
        }
    }
    origin
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `exe` or `exe*` (with a window).
    fn chain(spec: &[&str]) -> Vec<ProcEntry> {
        spec.iter()
            .enumerate()
            .map(|(i, s)| ProcEntry {
                pid: 100 + i as u32,
                exe: s.trim_end_matches('*').to_string(),
                has_window: s.ends_with('*'),
            })
            .collect()
    }

    fn pick(spec: &[&str]) -> Option<(usize, bool)> {
        choose_host(&chain(spec)).map(|p| (p.index, p.cli_direct))
    }

    #[test]
    fn claude_desktop_is_direct() {
        assert_eq!(pick(&["claude.exe", "claude.exe*"]), Some((1, true)));
    }

    #[test]
    fn windows_terminal_goes_through_a_shell() {
        assert_eq!(
            pick(&[
                "claude.exe",
                "pwsh.exe",
                "openconsole.exe",
                "windowsterminal.exe*"
            ]),
            Some((3, false))
        );
    }

    #[test]
    fn vs_code_extension_host_is_direct_the_integrated_terminal_is_not() {
        assert_eq!(
            pick(&["claude.exe", "code.exe", "code.exe*"]),
            Some((2, true))
        );
        assert_eq!(
            pick(&["claude.exe", "pwsh.exe", "code.exe", "code.exe*"]),
            Some((3, false))
        );
    }

    #[test]
    fn orca_terminal() {
        assert_eq!(
            pick(&["claude.exe", "pwsh.exe", "orca.exe*"]),
            Some((2, false))
        );
    }

    #[test]
    fn console_helpers_are_skipped_even_with_a_window() {
        assert_eq!(
            pick(&[
                "claude.exe",
                "cmd.exe",
                "conhost.exe*",
                "openconsole.exe*",
                "explorer.exe*"
            ]),
            Some((4, false))
        );
    }

    #[test]
    fn no_window_anywhere_is_none() {
        assert_eq!(
            pick(&["claude.exe", "pwsh.exe", "windowsterminal.exe"]),
            None
        );
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn the_hooks_own_shell_before_the_cli_is_not_the_host() {
        // The hook command ran through bash / cmd before reaching claude.
        assert_eq!(
            pick(&[
                "bash.exe*",
                "claude.exe",
                "pwsh.exe",
                "windowsterminal.exe*"
            ]),
            Some((3, false))
        );
        assert_eq!(
            pick(&["cmd.exe", "claude.exe", "claude.exe*"]),
            Some((2, true))
        );
        // node.exe as the CLI (npm install), a cmd shim between.
        assert_eq!(
            pick(&["node.exe", "cmd.exe", "pwsh.exe", "windowsterminal.exe*"]),
            Some((3, false))
        );
    }

    #[test]
    fn without_a_cli_entry_the_host_is_searched_from_the_start() {
        assert_eq!(
            pick(&["pwsh.exe", "windowsterminal.exe*"]),
            Some((1, false))
        );
    }

    #[test]
    fn a_shell_between_breaks_direct_even_for_the_same_exe() {
        assert_eq!(
            pick(&["claude.exe", "code.exe", "bash.exe", "code.exe*"]),
            Some((3, false))
        );
    }

    #[test]
    fn the_clis_console_is_read_before_the_parents() {
        use ConsoleOf::{Cli, Parent};
        // Claude Code -> bash (the hook command) -> vale.
        assert_eq!(console_order(Some(30), Some(40)), [Cli(30), Parent]);
        // The CLI started the hook itself: its console is the parent's.
        assert_eq!(console_order(Some(30), Some(30)), [Parent]);
        assert_eq!(console_order(None, Some(40)), [Parent]);
        assert_eq!(console_order(Some(30), None), [Cli(30), Parent]);
    }

    #[test]
    fn executable_paths_are_not_session_titles() {
        for t in [
            r"C:\Program Files\Git\bin\..\usr\bin\bash.exe",
            r"C:\WINDOWS\system32\cmd.exe",
            r"Administrator: C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe",
            r#""C:\Program Files\nodejs\node.exe""#,
            "/usr/bin/bash.exe",
            "pwsh.EXE",
        ] {
            assert!(is_exe_path_title(t), "{t}");
        }
        for t in [
            "✳ Fix the bug",
            "Claude Code",
            "Fix vale.exe startup",
            "api",
        ] {
            assert!(!is_exe_path_title(t), "{t}");
        }
    }

    /// The fake reads: per console, `None` (cannot attach) or what it says.
    fn gather(
        cli: Option<(Option<i64>, Option<&str>)>,
        parent: Option<(Option<i64>, Option<&str>)>,
    ) -> ((Option<i64>, Option<String>), Vec<ConsoleOf>) {
        let mut read = Vec::new();
        let found = gather_console(&console_order(Some(30), Some(40)), |c| {
            read.push(c);
            let r = match c {
                ConsoleOf::Cli(_) => cli,
                ConsoleOf::Parent => parent,
            };
            r.map(|(w, t)| (w, t.map(String::from)))
        });
        (found, read)
    }

    const BASH: &str = r"C:\Program Files\Git\bin\..\usr\bin\bash.exe";

    #[test]
    fn a_hidden_shell_console_does_not_hide_the_session() {
        // The reported case: the hook's bash has a hidden, windowless console
        // titled with its path; the CLI's console is the terminal's tab.
        let (found, read) = gather(
            Some((Some(7), Some("✳ Fix the bug"))),
            Some((None, Some(BASH))),
        );
        assert_eq!(found, (Some(7), Some("✳ Fix the bug".into())));
        assert_eq!(read, [ConsoleOf::Cli(30)], "the parent is not needed");
        // Plain console / default terminal: the CLI's conhost window is the
        // only window anywhere, its title still the shell's.
        let (found, _) = gather(
            Some((Some(7), Some(r"C:\WINDOWS\system32\cmd.exe"))),
            Some((None, Some(BASH))),
        );
        assert_eq!(found, (Some(7), None));
    }

    #[test]
    fn the_parents_console_fills_what_the_clis_lacks() {
        // No CLI console to attach to.
        let (found, read) = gather(None, Some((Some(9), Some("tab"))));
        assert_eq!(found, (Some(9), Some("tab".into())));
        assert_eq!(read, [ConsoleOf::Cli(30), ConsoleOf::Parent]);
        // A windowless CLI console (Claude Desktop) with a title.
        let (found, _) = gather(Some((None, Some("✳ Task"))), Some((Some(9), Some(BASH))));
        assert_eq!(found, (Some(9), Some("✳ Task".into())));
        // Nothing anywhere.
        assert_eq!(gather(None, None).0, (None, None));
    }

    fn wins(titles: &[&str]) -> Vec<(i64, String)> {
        titles
            .iter()
            .enumerate()
            .map(|(i, t)| (i as i64 + 1, t.to_string()))
            .collect()
    }

    #[test]
    fn the_host_window_of_the_project_beats_the_frontmost() {
        // VS Code: the user works in window B (front), the session is in A.
        let w = wins(&[
            "main.rs - shop - Visual Studio Code",
            "lib.rs - api - Visual Studio Code",
        ]);
        assert_eq!(pick_host_window(&w, "api"), Some(2));
        assert_eq!(pick_host_window(&w, "API"), Some(2));
        // A dirty marker and a multi-root workspace title.
        let w = wins(&[
            "main.rs - shop - Visual Studio Code",
            "\u{25cf} lib.rs - api (Workspace) - Visual Studio Code",
        ]);
        assert_eq!(pick_host_window(&w, "api"), Some(2));
    }

    #[test]
    fn the_folder_part_beats_a_file_named_like_the_project() {
        // "api.rs" names "api" as a whole word, but the folder part decides.
        let w = wins(&[
            "api.rs - shop - Visual Studio Code",
            "lib.rs - api - Visual Studio Code",
        ]);
        assert_eq!(pick_host_window(&w, "api"), Some(2));
        // Of two windows of the folder, the frontmost.
        let w = wins(&[
            "x - other - Visual Studio Code",
            "a.rs - api - Visual Studio Code",
            "b.rs - api - Visual Studio Code",
        ]);
        assert_eq!(pick_host_window(&w, "api"), Some(2));
    }

    #[test]
    fn without_a_project_match_the_frontmost_host_window_stays() {
        let w = wins(&["one - rapid - Code", "two - apiary - Code"]);
        assert_eq!(pick_host_window(&w, "api"), Some(1));
        assert_eq!(pick_host_window(&w, ""), Some(1));
        assert_eq!(pick_host_window(&wins(&[]), "api"), None);
    }

    #[test]
    fn a_console_window_counts_only_when_shown() {
        // ConPTY: the stand-in window's visible root owner (the terminal).
        assert_eq!(
            console_target(1, PSEUDO_CONSOLE_CLASS, false, Some((5, true))),
            Some(5)
        );
        assert_eq!(
            console_target(1, PSEUDO_CONSOLE_CLASS, false, Some((5, false))),
            None
        );
        assert_eq!(
            console_target(1, PSEUDO_CONSOLE_CLASS, false, Some((1, true))),
            None
        );
        assert_eq!(console_target(1, PSEUDO_CONSOLE_CLASS, false, None), None);
        // A classic console: itself, only while visible (a hook shell's or a
        // CLI started hidden is never the user's window).
        assert_eq!(console_target(1, "ConsoleWindowClass", true, None), Some(1));
        assert_eq!(console_target(1, "ConsoleWindowClass", false, None), None);
    }

    fn row(pid: u32, ppid: u32, exe: &str) -> ProcRow {
        ProcRow {
            pid,
            ppid,
            exe: exe.into(),
        }
    }

    #[test]
    fn ancestors_follow_parents_and_stop_at_gaps_and_cycles() {
        let table = [
            row(1, 0, "system"),
            row(10, 1, "explorer.exe"),
            row(20, 10, "pwsh.exe"),
            row(30, 20, "claude.exe"),
            row(40, 30, "vale.exe"),
        ];
        let names = |pid| {
            ancestor_chain(&table, pid)
                .into_iter()
                .map(|r| r.exe)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(40),
            ["claude.exe", "pwsh.exe", "explorer.exe", "system"]
        );
        // A parent that is gone ends the chain.
        assert_eq!(names(99), Vec::<String>::new());
        let orphan = [row(5, 777, "a.exe")];
        assert!(ancestor_chain(&orphan, 5).is_empty());
        // A cycle ends it too.
        let cyc = [row(1, 2, "a.exe"), row(2, 1, "b.exe")];
        assert_eq!(ancestor_chain(&cyc, 1).len(), 1);
    }

    #[test]
    fn the_chain_is_depth_limited() {
        let table: Vec<ProcRow> = (1..=40).map(|p| row(p, p + 1, "x.exe")).collect();
        assert_eq!(ancestor_chain(&table, 1).len(), MAX_DEPTH);
    }

    /// Only reads the real process table and windows; prints and asserts
    /// nothing about the machine.
    #[test]
    #[ignore = "reads the real desktop; run with --ignored --nocapture"]
    fn capture_on_this_machine() {
        let t = std::time::Instant::now();
        let origin = capture("dynamic-island");
        println!("{origin:?} in {:?}", t.elapsed());
    }
}
