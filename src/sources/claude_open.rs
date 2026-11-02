//! Clicking a Claude notice: go to the Claude Code session it came from.
//!
//! What the hook recorded (`claude::origin`) picks a rule (`notif::claude_open`,
//! pure and tested): Claude Desktop (its window, then the session's row in the
//! sidebar), the VS Code extension (its window, then the extension's own
//! link), a terminal or other host with tabs (its window, then the tab), or
//! just the window. This file lists windows, reads UI Automation and acts, on
//! the `notif-open` worker (`notif_open`), under the same 1 s budget and the
//! same newest-click-wins cancel.
//!
//! It never starts anything: no `claude://` link, no `claude --resume`, no new
//! window, terminal or session, no AUMID launch. A hidden (tray) main window
//! is shown, never relaunched. The foreground rule is the toast one: no input
//! queue attaching, no synthetic keys. Nothing is written to disk or logs; the
//! Claude Desktop session file is read only for its `title`, only at click
//! time. `plan` / `--probe-claude-open` run the same steps without acting.
//! See docs/memory/claude-notice-open.md.

use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation2, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationSelectionItemPattern, TreeScope_Descendants, UIA_ButtonControlTypeId,
    UIA_ControlTypePropertyId, UIA_InvokePatternId, UIA_NamePropertyId, UIA_SelectionItemPatternId,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, SW_SHOW, SW_SHOWNORMAL, ShowWindow,
};
use windows::core::{HSTRING, w};

use super::notif_open::{Clock, SELECT_TIMEOUT_MS, Win, bring_forward, read_tabs, windows};
use crate::claude::hook::{HookKind, parse_hook_input};
use crate::claude::host;
use crate::claude::origin::{self, ClaudeOrigin};
use crate::notif::claude_open::{
    ClaudeRule, WinCand, choose_claude_target, desktop_session_file_ok, hidden_console,
    match_sidebar_row, parse_session_title, pick_claude_tab, resolve_window, tab_fallback_project,
    vscode_uri,
};
use crate::notif::open::WinState;

/// Window class of Electron apps (Claude Desktop): the only windows the exe
/// fallback may pick for the Desktop rule, so a console window of a CLI that
/// is also called claude.exe is never taken for it.
const ELECTRON_CLASS: &str = "Chrome_WidgetWin_1";

/// UIA control types the tab search descends into. Wider than the browser
/// rule's: Windows Terminal (XAML tab view) and Orca (tabs in web content)
/// may nest their tabs in a List, a Document or further panes.
const TAB_CONTAINERS: [i32; 9] = [
    50033, // Pane
    50032, // Window
    50018, // Tab
    50026, // Group
    50021, // ToolBar
    50025, // Custom
    50037, // TitleBar
    50008, // List
    50030, // Document
];

/// Biggest session file read (it is only parsed for its title).
const MAX_SESSION_FILE: u64 = 8 * 1024 * 1024;

/// How long the VS Code link waits for its window to be the foreground one.
const FOREGROUND_WAIT: Duration = Duration::from_millis(300);
const FOREGROUND_POLL: Duration = Duration::from_millis(25);

/// What the worker needs for one click.
pub(crate) struct ClaudeJob {
    session_id: String,
    origin: ClaudeOrigin,
    project: String,
}

/// The job for a clicked notice; None when no origin was recorded for the
/// session (the click then only dismisses).
pub(crate) fn job_for(session_id: String) -> Option<ClaudeJob> {
    let remembered = origin::get(&session_id)?;
    Some(ClaudeJob {
        session_id,
        origin: remembered.origin,
        project: remembered.project,
    })
}

/// Does the job on the worker thread (its MTA apartment is already up).
pub(crate) fn run(job: &ClaudeJob, cancel: &dyn Fn() -> bool) {
    let _ = execute(job, cancel, true);
}

/// What a click would do, described and not done: the same steps with no
/// focus change, no Invoke, no Select and no link.
pub(crate) fn plan(job: &ClaudeJob) -> String {
    let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let text = execute(job, &|| false, false).join("\n");
    if inited {
        unsafe { CoUninitialize() };
    }
    text
}

// ------------------------------------------------------------------- steps

fn execute(job: &ClaudeJob, cancel: &dyn Fn() -> bool, act: bool) -> Vec<String> {
    let started = Instant::now();
    let mut log = Vec::new();
    let o = &job.origin;
    let rule = choose_claude_target(o);
    log.push(format!("rule: {rule:?}"));
    if rule == ClaudeRule::NoHost {
        log.push("nothing: no host recorded".into());
        return log;
    }
    let all = windows();
    if cancel() {
        return log;
    }
    let Some(win) = find_window(o, rule == ClaudeRule::Desktop, &all) else {
        log.push("nothing: no window of the host found (never launched)".into());
        return log;
    };
    log.push(format!(
        "window {:#x} of {} ({:?}, \"{}\")",
        win.hwnd,
        win.exe,
        win.state(),
        win.title
    ));
    let step = Step {
        job,
        win,
        cancel,
        started,
        act,
    };
    match rule {
        ClaudeRule::Desktop => step.desktop(&mut log),
        ClaudeRule::VsCodeUri { scheme } => step.vscode(scheme, &mut log),
        ClaudeRule::Tabs => step.tabs(&mut log),
        ClaudeRule::WindowOnly => step.front(&mut log),
        ClaudeRule::NoHost => {}
    }
    log
}

/// The recorded window while it is there, else the main window of the host's
/// exe. For the Desktop rule the exe fallback only looks at Electron windows.
/// A hidden console window is never taken (an older hook may have recorded
/// one): showing it would open a blank window the user never had.
fn find_window<'a>(o: &ClaudeOrigin, desktop: bool, all: &'a [Win]) -> Option<&'a Win> {
    let cand = |w: &Win| WinCand {
        hwnd: w.hwnd as i64,
        pid: w.pid,
        exe: w.exe.clone(),
        cand: w.cand(),
    };
    let all: Vec<&Win> = all
        .iter()
        .filter(|w| !hidden_console(&w.class, w.state()))
        .collect();
    let cands: Vec<WinCand> = all.iter().map(|w| cand(w)).collect();
    if let Some(i) = resolve_window(o.host_hwnd, o.host_pid, "", &cands) {
        return Some(all[i]);
    }
    let exe = if desktop && o.host_exe.is_empty() {
        "claude.exe"
    } else {
        o.host_exe.as_str()
    };
    let pool: Vec<&Win> = all
        .into_iter()
        .filter(|w| !desktop || w.class == ELECTRON_CLASS)
        .collect();
    let cands: Vec<WinCand> = pool.iter().map(|w| cand(w)).collect();
    resolve_window(0, 0, exe, &cands).map(|i| pool[i])
}

struct Step<'a> {
    job: &'a ClaudeJob,
    win: &'a Win,
    cancel: &'a dyn Fn() -> bool,
    started: Instant,
    act: bool,
}

impl Step<'_> {
    /// Shows a hidden window, restores a minimized one, brings it forward.
    fn front(&self, log: &mut Vec<String>) {
        log.push("bring forward".into());
        if !self.act {
            return;
        }
        if self.win.state() == WinState::Hidden {
            unsafe {
                let _ = ShowWindow(self.win.handle(), SW_SHOW);
            }
        }
        bring_forward(self.win.handle());
    }

    fn uia(&self) -> Option<IUIAutomation2> {
        unsafe {
            CoCreateInstance::<_, IUIAutomation2>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
        }
        .ok()
    }

    /// Claude Desktop: the window, then the session's sidebar row.
    fn desktop(&self, log: &mut Vec<String>) {
        self.front(log);
        let id = self
            .job
            .origin
            .host_session_id
            .as_deref()
            .filter(|id| desktop_session_file_ok(id));
        let Some(title) = id.and_then(session_title) else {
            log.push("no session title found: window only".into());
            return;
        };
        log.push(format!("session title: \"{title}\""));
        if (self.cancel)() {
            return;
        }
        let Some(uia) = self.uia() else {
            log.push("no UI Automation: window only".into());
            return;
        };
        let clock = Clock {
            uia: &uia,
            started: self.started,
            cancel: self.cancel,
        };
        let Some(rows) = (unsafe { read_buttons(&clock, self.win.handle()) }) else {
            log.push("out of time before the sidebar was read: window only".into());
            return;
        };
        let names: Vec<String> = rows.iter().map(|(n, _)| n.clone()).collect();
        log.push(format!("{} buttons read", names.len()));
        let Some(i) = match_sidebar_row(&names, &title) else {
            log.push("no single sidebar row matches the title: window only".into());
            return;
        };
        log.push(format!("sidebar row \"{}\" -> invoke", names[i]));
        if !self.act || (self.cancel)() {
            return;
        }
        unsafe {
            reset_timeouts(&uia);
            if let Ok(pattern) = rows[i]
                .1
                .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
            {
                let _ = pattern.Invoke();
            }
        }
    }

    /// VS Code: its window, and once that is really the foreground one, the
    /// extension's link (it acts in the focused window).
    fn vscode(&self, scheme: &str, log: &mut Vec<String>) {
        self.front(log);
        let uri = vscode_uri(scheme, &self.job.session_id);
        log.push(format!("open {uri} once the window is in front"));
        if !self.act {
            return;
        }
        if !wait_foreground(self.win.handle(), self.cancel) {
            return;
        }
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(uri),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }

    /// A terminal or another host: its window, then the tab of the session.
    fn tabs(&self, log: &mut Vec<String>) {
        self.front(log);
        if (self.cancel)() {
            return;
        }
        let Some(uia) = self.uia() else {
            log.push("no UI Automation: window only".into());
            return;
        };
        let clock = Clock {
            uia: &uia,
            started: self.started,
            cancel: self.cancel,
        };
        let Some((tabs, elements)) =
            (unsafe { read_tabs(&clock, self.win.handle(), &TAB_CONTAINERS) })
        else {
            log.push("out of time before the tabs were read: window only".into());
            return;
        };
        log.push(format!("{} tabs read", tabs.tabs.len()));
        // An editor's tabs include its open files: there only the console
        // title may pick.
        let picked = pick_claude_tab(
            &tabs.tabs,
            self.job.origin.console_title.as_deref(),
            tab_fallback_project(&self.job.origin.host_exe, &self.job.project),
        );
        let Some(i) = picked else {
            log.push("no single tab matches: current tab left".into());
            return;
        };
        log.push(format!("tab {} \"{}\" -> select", i + 1, tabs.tabs[i]));
        if !self.act || (self.cancel)() {
            return;
        }
        unsafe {
            reset_timeouts(&uia);
            if let Ok(item) = elements[i].GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                UIA_SelectionItemPatternId,
            ) {
                let _ = item.Select();
            }
        }
    }
}

/// A fresh timeout for the Invoke / Select: the budget checks leave the
/// client with whatever was over, down to 1 ms.
unsafe fn reset_timeouts(uia: &IUIAutomation2) {
    unsafe {
        let _ = uia.SetConnectionTimeout(SELECT_TIMEOUT_MS);
        let _ = uia.SetTransactionTimeout(SELECT_TIMEOUT_MS);
    }
}

fn wait_foreground(hwnd: HWND, cancel: &dyn Fn() -> bool) -> bool {
    let started = Instant::now();
    loop {
        if unsafe { GetForegroundWindow() } == hwnd {
            return true;
        }
        if started.elapsed() >= FOREGROUND_WAIT || cancel() {
            return false;
        }
        std::thread::sleep(FOREGROUND_POLL);
    }
}

/// Every button under the window with its name, in one cached call under the
/// clock. None when the clock ran out first.
unsafe fn read_buttons(clock: &Clock, hwnd: HWND) -> Option<Vec<(String, IUIAutomationElement)>> {
    let uia = clock.uia;
    unsafe {
        if !clock.go() {
            return None;
        }
        let root = uia.ElementFromHandle(hwnd).ok()?;
        let cache = uia.CreateCacheRequest().ok()?;
        cache.AddProperty(UIA_NamePropertyId).ok()?;
        let buttons = uia
            .CreatePropertyCondition(
                UIA_ControlTypePropertyId,
                &VARIANT::from(UIA_ButtonControlTypeId.0),
            )
            .ok()?;
        if !clock.go() {
            return None;
        }
        let found = root
            .FindAllBuildCache(TreeScope_Descendants, &buttons, &cache)
            .ok()?;
        let mut rows = Vec::new();
        for i in 0..found.Length().unwrap_or(0).max(0) {
            let Ok(el) = found.GetElement(i) else {
                continue;
            };
            let name = el.CachedName().map(|n| n.to_string()).unwrap_or_default();
            rows.push((name, el));
        }
        Some(rows)
    }
}

// ----------------------------------------------------- Desktop session file

/// The `title` of Claude Desktop's session `id`, read at click time from its
/// private storage; nothing else of the file is kept.
fn session_title(id: &str) -> Option<String> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let file = find_session_file(&PathBuf::from(local).join("Claude-Data"), id)?;
    if std::fs::metadata(&file).ok()?.len() > MAX_SESSION_FILE {
        return None;
    }
    parse_session_title(&std::fs::read_to_string(file).ok()?)
}

/// `<root>\*\claude-code-sessions\*\*\<id>.json`, walked three levels with
/// no glob. `id` must be a plain identifier.
fn find_session_file(root: &Path, id: &str) -> Option<PathBuf> {
    if !desktop_session_file_ok(id) {
        return None;
    }
    let dirs = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
    };
    let name = format!("{id}.json");
    for data in dirs(root) {
        for account in dirs(&data.join("claude-code-sessions")) {
            for org in dirs(&account) {
                let file = org.join(&name);
                if file.is_file() {
                    return Some(file);
                }
            }
        }
    }
    None
}

// ------------------------------------------------------------------- probe

/// `--probe-claude-open`: reads a hook payload from stdin, captures the
/// origin exactly as the hook would (this process's ancestors), and prints it
/// with what a click would do. Acts on nothing.
pub fn probe() {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        println!("usage: <hook payload JSON> | vale --probe-claude-open");
        println!(
            r#"example: echo {{"hook_event_name":"Stop","session_id":"x","cwd":"D:\\proj"}} | vale --probe-claude-open"#
        );
        return;
    }
    let mut input = Vec::new();
    if stdin.lock().read_to_end(&mut input).is_err() {
        return;
    }
    let Some(msg) = parse_hook_input(&String::from_utf8_lossy(&input)) else {
        println!("not a hook payload the island uses");
        return;
    };
    let kind = match msg.kind {
        HookKind::Done => "Done",
        HookKind::Waiting => "Waiting",
        HookKind::Activity => "Activity (the hook would capture nothing)",
    };
    println!(
        "event: {kind}, session {}, project \"{}\"",
        msg.session_id, msg.project
    );
    let origin = host::capture(&msg.project);
    println!(
        "origin: host pid {}, exe \"{}\", hwnd {:#x}, cli_direct {}",
        origin.host_pid, origin.host_exe, origin.host_hwnd, origin.cli_direct
    );
    println!(
        "        console title {:?}, host session id {:?}",
        origin.console_title, origin.host_session_id
    );
    let job = ClaudeJob {
        session_id: msg.session_id,
        origin,
        project: msg.project,
    };
    println!("{}", plan(&job));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(origin: ClaudeOrigin, project: &str) -> ClaudeJob {
        ClaudeJob {
            session_id: "plan-test-session".into(),
            origin,
            project: project.into(),
        }
    }

    #[test]
    fn the_session_file_is_found_three_levels_down_only_for_a_plain_id() {
        let root = std::env::temp_dir().join(format!("vale-claude-open-{}", std::process::id()));
        let dir = root
            .join("claude_x")
            .join("claude-code-sessions")
            .join("acct")
            .join("org");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("local_abc.json"), r#"{"title":"Fix login"}"#).unwrap();
        let found = find_session_file(&root, "local_abc").unwrap();
        assert!(found.ends_with("local_abc.json"));
        assert_eq!(find_session_file(&root, "local_missing"), None);
        assert_eq!(find_session_file(&root, "..\\local_abc"), None);
        assert_eq!(find_session_file(&root, "claude_x/../local_abc"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Prints what a click on a Claude Desktop notice would do. Acts on nothing.
    #[test]
    #[ignore = "reads the real desktop; run with --ignored --nocapture"]
    fn plan_for_claude_desktop() {
        let o = ClaudeOrigin {
            host_exe: "claude.exe".into(),
            host_session_id: Some("local_does-not-exist".into()),
            cli_direct: true,
            ..Default::default()
        };
        println!("{}", plan(&job(o, "proj")));
    }

    /// Windows Terminal: its window by name, tabs read, none matches "nothing".
    #[test]
    #[ignore = "reads the real desktop; run with --ignored --nocapture"]
    fn plan_for_claude_in_windows_terminal() {
        let o = ClaudeOrigin {
            host_exe: "windowsterminal.exe".into(),
            host_pid: 1,
            host_hwnd: 1,
            console_title: Some("✳ Claude Code".into()),
            ..Default::default()
        };
        println!("{}", plan(&job(o, "dynamic-island")));
    }

    /// VS Code: the window and the link that would open; nothing is opened.
    #[test]
    #[ignore = "reads the real desktop; run with --ignored --nocapture"]
    fn plan_for_claude_in_vs_code() {
        let o = ClaudeOrigin {
            host_exe: "code.exe".into(),
            host_pid: 1,
            host_hwnd: 1,
            cli_direct: true,
            ..Default::default()
        };
        println!("{}", plan(&job(o, "proj")));
    }

    /// No host recorded: nothing to do, and no launch.
    #[test]
    #[ignore = "reads the real desktop; run with --ignored --nocapture"]
    fn plan_for_claude_without_a_host() {
        let text = plan(&job(ClaudeOrigin::default(), "proj"));
        println!("{text}");
        assert!(text.contains("nothing"), "{text}");
    }
}
