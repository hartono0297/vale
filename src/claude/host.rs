//! `--hook` side of "click a Claude notice, go to its session": where does
//! this hook run? Walks the process ancestors (one Toolhelp snapshot), finds
//! the first one that owns a real top-level window and is not a shell or
//! console helper, and falls back to the parent's console window. Prints
//! nothing, never panics, launches nothing. See docs/memory/claude-notice-open.md.

use std::collections::HashMap;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, FreeConsole, GetConsoleTitleW, GetConsoleWindow,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GA_ROOTOWNER, GW_OWNER, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetWindow,
    GetWindowLongW, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
    WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;

use super::origin::{ClaudeOrigin, clamp_title, sanitize_host_session_id};

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
    found: HashMap<u32, i64>,
}

/// Per pid of `pids`, its first (most recent in Z order) top-level window that
/// is visible, unowned, not a tool window and titled.
fn windows_of(pids: &[u32]) -> HashMap<u32, i64> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let scan = unsafe { &mut *(lparam.0 as *mut Scan) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if scan.pids.contains(&pid)
            && !scan.found.contains_key(&pid)
            && unsafe { real_window(hwnd) }
        {
            scan.found.insert(pid, hwnd.0 as isize as i64);
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

/// What the console of the hook's parent (else of the CLI) says: its window
/// (a ConPTY pseudo window resolved to the terminal window that owns it) and
/// its title. Attaches to that console and lets go again. Stdin is already
/// read and nothing is written.
fn console_info(cli_pid: Option<u32>) -> (Option<HWND>, Option<String>) {
    unsafe {
        let mut attached = AttachConsole(ATTACH_PARENT_PROCESS).is_ok();
        if !attached
            && GetConsoleWindow().is_invalid()
            && let Some(pid) = cli_pid
        {
            attached = AttachConsole(pid).is_ok();
        }
        let console = GetConsoleWindow();
        let window = (!console.is_invalid())
            .then(|| terminal_window(console))
            .flatten();
        let mut buf = [0u16; 1024];
        let n = GetConsoleTitleW(&mut buf) as usize;
        let title = clamp_title(&String::from_utf16_lossy(&buf[..n.min(buf.len())]));
        if attached {
            let _ = FreeConsole();
        }
        (window, title)
    }
}

/// A console window as the window to bring forward: a ConPTY pseudo window is
/// replaced by its root owner (the terminal), which must be a real visible
/// window; a classic console window is itself.
unsafe fn terminal_window(console: HWND) -> Option<HWND> {
    unsafe {
        let mut name = [0u16; 64];
        let n = GetClassNameW(console, &mut name).max(0) as usize;
        let class = String::from_utf16_lossy(&name[..n.min(name.len())]);
        if class != "PseudoConsoleWindow" {
            return Some(console);
        }
        let root = GetAncestor(console, GA_ROOTOWNER);
        (!root.is_invalid() && root != console && IsWindowVisible(root).as_bool()).then_some(root)
    }
}

/// Facts about where this hook runs. Called only for a Done or Waiting event.
/// No panics, nothing printed.
pub fn capture() -> ClaudeOrigin {
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
            has_window: windows.contains_key(&r.pid) && r.exe != "explorer.exe",
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
        origin.host_hwnd = windows.get(&host.pid).copied().unwrap_or(0);
        origin.cli_direct = pick.cli_direct;
    }
    let cli_pid = chain.iter().find(|e| is_cli(&e.exe)).map(|e| e.pid);
    let (console_window, title) = console_info(cli_pid);
    origin.console_title = title;
    if origin.host_pid == 0
        && let Some(hwnd) = console_window
    {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        origin.host_pid = pid;
        origin.host_exe = crate::sources::notif_open::image_name(pid);
        origin.host_hwnd = hwnd.0 as isize as i64;
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
        let origin = capture();
        println!("{origin:?} in {:?}", t.elapsed());
    }
}
