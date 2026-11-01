//! Opening a caught toast: go to where it came from.
//!
//! A browser toast goes to the tab that raised it (any profile's window); a
//! desktop app's toast brings that app's own window forward, even one hidden
//! in the tray. One routine serves the notch click and the Notifikasi page's
//! Open. Every decision is in `notif::open` (pure, tested); this file lists
//! windows, reads a browser's tab strip through UI Automation, and acts.
//!
//! `open` returns at once: the work runs on one long-lived worker thread (its
//! own COM apartment), the search capped at `SEARCH_BUDGET`, a newer click
//! superseding an older one. It never starts a browser when one of its windows
//! exists (no new window, no profile picker), and never synthesizes keys or
//! attaches input queues. See docs/memory/notification-open.md.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HWND, LPARAM};
use windows::Win32::Storage::EnhancedStorage::{PKEY_AppUserModel_ID, PKEY_Link_TargetParsingPath};
use windows::Win32::System::Com::StructuredStorage::PropVariantToString;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, IBindCtx,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation2, IUIAutomationElement, IUIAutomationSelectionItemPattern,
    TreeScope_Children, UIA_ControlTypePropertyId, UIA_NamePropertyId,
    UIA_SelectionItemIsSelectedPropertyId, UIA_SelectionItemPatternId, UIA_TabItemControlTypeId,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, BringWindowToTop, EnumWindows, GW_OWNER, GWL_EXSTYLE,
    GetClassNameW, GetWindow, GetWindowLongW, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, SW_RESTORE, SW_SHOW,
    SetForegroundWindow, ShowWindow, SwitchToThisWindow, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR};

use crate::model::OpenTarget;
use crate::notif::open::{
    self as rules, DesktopAction, MainCand, OpenKind, WinState, WindowTabs, browser_exe_names,
    decide, desktop_action, exe_stem, exe_stems_for, pick_tab, start_target_exe,
};

/// Time for the search (window list and tab strips); no call starts after it.
const SEARCH_BUDGET: Duration = Duration::from_millis(1000);
/// How long a relaunched, tray-hidden app gets to show its own window.
const WAKE_WAIT: Duration = Duration::from_millis(2000);
const WAKE_POLL: Duration = Duration::from_millis(100);

/// How deep the tab strip is looked for in a browser window's UIA tree.
const MAX_DEPTH: usize = 8;
/// Elements looked at in one browser window.
const MAX_VISITED: usize = 800;

/// Which click is the newest: an older job stops at its next step.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static WORKER: Mutex<Option<Sender<(u64, OpenTarget)>>> = Mutex::new(None);

/// Goes to where `target` came from. Returns at once (a send to the worker);
/// call from any thread, the UI thread included.
pub fn open(target: OpenTarget) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let mut slot = WORKER.lock().unwrap_or_else(|p| p.into_inner());
    let target = match slot.as_ref() {
        Some(tx) => match tx.send((generation, target)) {
            Ok(()) => return,
            // The worker is gone: start another with the job.
            Err(mpsc::SendError((_, target))) => target,
        },
        None => target,
    };
    let (tx, rx) = mpsc::channel();
    let started = std::thread::Builder::new()
        .name("notif-open".into())
        .spawn(move || worker(rx))
        .is_ok();
    if started && tx.send((generation, target)).is_ok() {
        *slot = Some(tx);
    }
}

fn worker(jobs: Receiver<(u64, OpenTarget)>) {
    let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    while let Ok(mut job) = jobs.recv() {
        // Only the newest click matters.
        while let Ok(newer) = jobs.try_recv() {
            job = newer;
        }
        let (generation, target) = job;
        let cancel = move || GENERATION.load(Ordering::SeqCst) != generation;
        let plan = search(&target, &cancel);
        if !cancel() {
            execute(plan, &target, &cancel);
        }
    }
    if inited {
        unsafe { CoUninitialize() };
    }
}

/// What opening `target` would do, described and not done: the same search as
/// `open`, with no selection, no foreground change and no launch.
pub fn plan(target: &OpenTarget) -> String {
    let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let text = describe(&search(target, &|| false));
    if inited {
        unsafe { CoUninitialize() };
    }
    text
}

/// `--probe-open`: prints `plan` for a toast described on the command line
/// (`<app name> [AUMID|-] [site|-] [title]`). Acts on nothing.
pub fn probe(args: Vec<String>) {
    let mut args = args.into_iter();
    let Some(source) = args.next() else {
        println!("usage: vale --probe-open <app name> [AUMID|-] [site|-] [title]");
        return;
    };
    let some = |a: Option<String>| a.filter(|s| !s.is_empty() && s != "-");
    let target = OpenTarget {
        app_id: some(args.next()),
        source,
        site: some(args.next()),
        title: args.next().unwrap_or_default(),
        body: String::new(),
    };
    println!("{}", plan(&target));
}

// ------------------------------------------------------------------ windows

/// A top-level window, as listed.
#[derive(Clone, Debug)]
struct Win {
    hwnd: isize,
    /// Image file name, lowercase ("chrome.exe"); empty when unreadable.
    exe: String,
    class: String,
    title: String,
    visible: bool,
    minimized: bool,
    owned: bool,
    tool: bool,
    w: i32,
    h: i32,
}

impl Win {
    fn state(&self) -> WinState {
        if !self.visible {
            WinState::Hidden
        } else if self.minimized {
            WinState::Minimized
        } else {
            WinState::Visible
        }
    }

    fn cand(&self) -> MainCand {
        MainCand {
            state: self.state(),
            owned: self.owned,
            tool_window: self.tool,
            has_title: !self.title.trim().is_empty(),
            w: self.w,
            h: self.h,
        }
    }

    fn handle(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }
}

/// Classes that are never an app's window.
const SKIPPED_CLASSES: [&str; 7] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "IME",
    "MSCTFIME UI",
    "CiceroUIWndFrame",
];

struct Listing {
    out: Vec<Win>,
    own_pid: u32,
    exes: HashMap<u32, String>,
}

/// Every top-level window in Z order (frontmost first), hidden and minimized
/// ones too, without this process's own, IME and zero-size windows.
fn windows() -> Vec<Win> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let listing = unsafe { &mut *(lparam.0 as *mut Listing) };
        if let Some(w) = unsafe { describe_window(hwnd, listing) } {
            listing.out.push(w);
        }
        true.into()
    }
    let mut listing = Listing {
        out: Vec::new(),
        own_pid: unsafe { GetCurrentProcessId() },
        exes: HashMap::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM((&raw mut listing) as isize));
    }
    listing.out
}

unsafe fn describe_window(hwnd: HWND, listing: &mut Listing) -> Option<Win> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == listing.own_pid {
            return None;
        }
        let mut name = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut name).max(0) as usize;
        let class = String::from_utf16_lossy(&name[..n.min(name.len())]);
        if SKIPPED_CLASSES.contains(&class.as_str()) {
            return None;
        }
        let minimized = IsIconic(hwnd).as_bool();
        let mut rect = windows::Win32::Foundation::RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if !minimized && (w <= 0 || h <= 0) {
            return None;
        }
        let mut text = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut text).max(0) as usize;
        let exe = listing
            .exes
            .entry(pid)
            .or_insert_with(|| image_name(pid))
            .clone();
        Some(Win {
            hwnd: hwnd.0 as isize,
            exe,
            class,
            title: String::from_utf16_lossy(&text[..n.min(text.len())]),
            visible: IsWindowVisible(hwnd).as_bool(),
            minimized,
            owned: GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()),
            tool: GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0,
            w,
            h,
        })
    }
}

/// The exe file name of process `pid`, lowercase; empty when unreadable.
fn image_name(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 1040];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().unwrap_or("").to_lowercase()
    }
}

/// The AppUserModelID a window declares. Often empty (Chrome and Discord
/// windows set none), so it is never the only way to find an app.
fn window_aumid(hwnd: HWND) -> Option<String> {
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let value = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let mut buf = [0u16; 512];
        PropVariantToString(&value, &mut buf).ok()?;
        let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        let id = String::from_utf16_lossy(&buf[..n]);
        (!id.is_empty()).then_some(id)
    }
}

/// The exe a Start entry points at ("shell:AppsFolder\\<AUMID>"), when it
/// names one; an updater or launcher stub is ignored (`start_target_exe`).
fn start_entry_exe(aumid: &str) -> Option<String> {
    unsafe {
        let item: IShellItem2 = SHCreateItemFromParsingName(
            &HSTRING::from(format!("shell:AppsFolder\\{aumid}")),
            None::<&IBindCtx>,
        )
        .ok()?;
        let path = item.GetString(&PKEY_Link_TargetParsingPath).ok()?;
        let text = path.to_string().ok();
        CoTaskMemFree(Some(path.0 as *const _));
        start_target_exe(&text?)
    }
}

// ------------------------------------------------------------------- plans

/// What the search decided; `execute` does it, `describe` words it.
enum Plan {
    /// Select this tab of this browser window, then bring it forward.
    Tab {
        win: Win,
        index: usize,
        name: String,
        element: IUIAutomationElement,
    },
    /// A browser window, tab unchanged.
    Front { win: Win, why: &'static str },
    /// The browser is not running: start it on the site.
    LaunchBrowser { exe: String, url: String },
    /// A desktop app's visible or minimized main window.
    Focus { win: Win },
    /// A tray-hidden main window: launch by AUMID, then show it if nothing
    /// came up.
    Wake { win: Win, aumid: String },
    /// A hidden main window and no AUMID: show it.
    Show { win: Win },
    LaunchAumid { aumid: String, why: &'static str },
    Nothing(&'static str),
}

fn describe(plan: &Plan) -> String {
    match plan {
        Plan::Tab {
            win, index, name, ..
        } => format!(
            "browser {}: window {:#x} tab {} \"{}\"",
            win.exe,
            win.hwnd,
            index + 1,
            name
        ),
        Plan::Front { win, why } => format!(
            "browser {}: window {:#x} forward, tab unchanged ({why})",
            win.exe, win.hwnd
        ),
        Plan::LaunchBrowser { exe, url } => format!("browser not running: start {exe} {url}"),
        Plan::Focus { win } => format!(
            "desktop: {:?} main window of {} {:#x} -> focus",
            win.state(),
            win.exe,
            win.hwnd
        ),
        Plan::Wake { win, aumid } => format!(
            "desktop: hidden main window of {} -> relaunch by AUMID {aumid}, then show",
            win.exe
        ),
        Plan::Show { win } => format!("desktop: hidden main window of {} -> show", win.exe),
        Plan::LaunchAumid { aumid, why } => format!("launch by AUMID {aumid} ({why})"),
        Plan::Nothing(why) => format!("nothing: {why}"),
    }
}

/// Looks, never acts. `cancel` ends the search early with `Nothing`.
fn search(target: &OpenTarget, cancel: &dyn Fn() -> bool) -> Plan {
    let wins = windows();
    if cancel() {
        return Plan::Nothing("superseded");
    }
    match decide(target) {
        OpenKind::Browser => search_browser(target, &wins, cancel),
        OpenKind::Desktop => search_desktop(target, &wins),
    }
}

// ----------------------------------------------------------------- browser

/// The windows that can hold a browser's tab strip.
fn is_browser_window(w: &Win) -> bool {
    !w.owned
        && !w.tool
        && w.visible
        && !w.title.trim().is_empty()
        && (w.class == "Chrome_WidgetWin_1" || w.class == "MozillaWindowClass")
}

fn search_browser(target: &OpenTarget, wins: &[Win], cancel: &dyn Fn() -> bool) -> Plan {
    let names = browser_exe_names(target.app_id.as_deref(), &target.source);
    let mine: Vec<&Win> = wins
        .iter()
        .filter(|w| names.contains(&w.exe.as_str()) && is_browser_window(w))
        .collect();
    let Some(first) = mine.first() else {
        return launch_browser_plan(target, &names);
    };
    let uia = unsafe {
        CoCreateInstance::<_, IUIAutomation2>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
    };
    if let Ok(uia) = uia {
        let clock = Clock {
            uia: &uia,
            started: Instant::now(),
            cancel,
        };
        let mut strips: Vec<WindowTabs> = Vec::new();
        let mut elements: Vec<Vec<IUIAutomationElement>> = Vec::new();
        for w in &mine {
            let Some((tabs, els)) = (unsafe { read_tabs(&clock, w.handle()) }) else {
                break;
            };
            strips.push(tabs);
            elements.push(els);
        }
        if let Some((wi, ti)) = pick_tab(&strips, target) {
            return Plan::Tab {
                win: mine[wi].clone(),
                index: ti,
                name: strips[wi].tabs[ti].clone(),
                element: elements[wi][ti].clone(),
            };
        }
    }
    Plan::Front {
        win: (*first).clone(),
        why: "no tab matched",
    }
}

/// The browser has no window: its site opens in it when both are known, else
/// it starts the way it always did.
fn launch_browser_plan(target: &OpenTarget, names: &[&str]) -> Plan {
    if let Some(site) = target.site.as_deref().filter(|s| !s.trim().is_empty())
        && let Some(exe) = browser_exe_path(target.app_id.as_deref(), names)
    {
        return Plan::LaunchBrowser {
            exe,
            url: format!("https://{}", site.trim()),
        };
    }
    match target.app_id.as_deref().filter(|id| !id.is_empty()) {
        Some(aumid) => Plan::LaunchAumid {
            aumid: aumid.to_string(),
            why: "browser not running, site or exe unknown",
        },
        None => Plan::Nothing("browser not running and no AUMID"),
    }
}

/// The browser's exe: `App Paths`, else its Start entry.
fn browser_exe_path(app_id: Option<&str>, names: &[&str]) -> Option<String> {
    const APP_PATHS: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\";
    for name in names {
        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            if let Some(path) = unsafe { reg_default(root, &format!("{APP_PATHS}{name}")) } {
                let path = path.trim_matches('"').to_string();
                if Path::new(&path).is_file() {
                    return Some(path);
                }
            }
        }
    }
    // The Start entry names the exe by file name only when it is the browser.
    let aumid = app_id?;
    let file = start_entry_exe(aumid)?;
    names.contains(&file.as_str()).then(|| file)
}

/// A string value (the default one) of a registry key, environment variables
/// expanded.
unsafe fn reg_default(root: HKEY, subkey: &str) -> Option<String> {
    let key = HSTRING::from(subkey);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    unsafe {
        let mut size = 0u32;
        if RegGetValueW(root, &key, PCWSTR::null(), flags, None, None, Some(&mut size))
            != ERROR_SUCCESS
        {
            return None;
        }
        let mut buf = vec![0u16; size as usize / 2 + 2];
        let mut size = (buf.len() * 2) as u32;
        if RegGetValueW(
            root,
            &key,
            PCWSTR::null(),
            flags,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        let text = String::from_utf16_lossy(&buf[..n]);
        (!text.is_empty()).then_some(text)
    }
}

/// The budget the tab reads share: before every cross-process call, false
/// when the budget is spent or the click is superseded, else the UI
/// Automation timeouts are set to what is left.
struct Clock<'a> {
    uia: &'a IUIAutomation2,
    started: Instant,
    cancel: &'a dyn Fn() -> bool,
}

impl Clock<'_> {
    fn go(&self) -> bool {
        let elapsed = self.started.elapsed();
        if elapsed >= SEARCH_BUDGET || (self.cancel)() {
            return false;
        }
        let left = rules::timeout_ms(SEARCH_BUDGET, elapsed);
        unsafe {
            self.uia.SetConnectionTimeout(left).is_ok()
                && self.uia.SetTransactionTimeout(left).is_ok()
        }
    }
}

/// UIA control types worth opening on the way to the tab strip: the Document
/// (the page itself) and every leaf are not.
const CONTAINERS: [i32; 7] = [
    50033, // Pane
    50032, // Window
    50018, // Tab
    50026, // Group
    50021, // ToolBar
    50025, // Custom
    50037, // TitleBar
];

/// The tab strip of one browser window: breadth first, one level of one
/// element per call, never the page. The first depth holding tab items ends
/// the walk. None when the clock ran out before the window could be started.
unsafe fn read_tabs(
    clock: &Clock,
    hwnd: HWND,
) -> Option<(WindowTabs, Vec<IUIAutomationElement>)> {
    let uia = clock.uia;
    unsafe {
        if !clock.go() {
            return None;
        }
        let Ok(root) = uia.ElementFromHandle(hwnd) else {
            // Closed since the list was made.
            return Some((
                WindowTabs {
                    tabs: Vec::new(),
                    active: None,
                },
                Vec::new(),
            ));
        };
        let cache = uia.CreateCacheRequest().ok()?;
        cache.AddProperty(UIA_ControlTypePropertyId).ok()?;
        cache.AddProperty(UIA_NamePropertyId).ok()?;
        cache.AddProperty(UIA_SelectionItemIsSelectedPropertyId).ok()?;
        let all = uia.CreateTrueCondition().ok()?;
        let mut names: Vec<String> = Vec::new();
        let mut elements: Vec<IUIAutomationElement> = Vec::new();
        let mut active = None;
        let mut visited = 0usize;
        let mut found_at: Option<usize> = None;
        let mut open: VecDeque<(IUIAutomationElement, usize)> = VecDeque::from([(root, 0)]);
        while let Some((parent, depth)) = open.pop_front() {
            if found_at.is_some_and(|d| depth > d) || !clock.go() {
                break;
            }
            let Ok(children) = parent.FindAllBuildCache(TreeScope_Children, &all, &cache) else {
                continue;
            };
            for i in 0..children.Length().unwrap_or(0).max(0) {
                if visited >= MAX_VISITED {
                    break;
                }
                visited += 1;
                let Ok(child) = children.GetElement(i) else {
                    continue;
                };
                let Ok(kind) = child.CachedControlType() else {
                    continue;
                };
                if kind == UIA_TabItemControlTypeId {
                    found_at = Some(depth);
                    let name = child.CachedName().map(|n| n.to_string()).unwrap_or_default();
                    let selected = child
                        .GetCachedPropertyValue(UIA_SelectionItemIsSelectedPropertyId)
                        .ok()
                        .and_then(|v| bool::try_from(&v).ok())
                        .unwrap_or(false);
                    if selected {
                        active = Some(names.len());
                    }
                    names.push(name);
                    elements.push(child);
                } else if found_at.is_none()
                    && depth < MAX_DEPTH
                    && CONTAINERS.contains(&kind.0)
                {
                    open.push_back((child, depth + 1));
                }
            }
        }
        Some((
            WindowTabs {
                tabs: names,
                active,
            },
            elements,
        ))
    }
}

// ----------------------------------------------------------------- desktop

fn search_desktop(target: &OpenTarget, wins: &[Win]) -> Plan {
    let aumid = target.app_id.as_deref().filter(|id| !id.is_empty());
    let stems = exe_stems_for(aumid, &target.source);
    let start_exe = aumid.and_then(start_entry_exe);
    let mut found: Vec<&Win> = Vec::new();
    for w in wins {
        let by_process = !w.exe.is_empty()
            && (start_exe.as_deref() == Some(w.exe.as_str()) || stems.contains(&exe_stem(&w.exe)));
        let by_aumid = !by_process
            && aumid.is_some_and(|id| {
                !w.owned
                    && !w.tool
                    && window_aumid(w.handle()).is_some_and(|got| got.eq_ignore_ascii_case(id))
            });
        if by_process || by_aumid {
            found.push(w);
        }
    }
    let cands: Vec<MainCand> = found.iter().map(|w| w.cand()).collect();
    match desktop_action(&cands, aumid.is_some()) {
        DesktopAction::Focus(i) => Plan::Focus {
            win: found[i].clone(),
        },
        DesktopAction::WakeByLaunch(i) => Plan::Wake {
            win: found[i].clone(),
            aumid: aumid.unwrap_or_default().to_string(),
        },
        DesktopAction::Show(i) => Plan::Show {
            win: found[i].clone(),
        },
        DesktopAction::Launch => Plan::LaunchAumid {
            aumid: aumid.unwrap_or_default().to_string(),
            why: "no window found",
        },
        DesktopAction::Nothing => Plan::Nothing("no window found and no AUMID"),
    }
}

// --------------------------------------------------------------------- act

fn execute(plan: Plan, target: &OpenTarget, cancel: &dyn Fn() -> bool) {
    match plan {
        Plan::Tab { win, element, .. } => {
            unsafe {
                if let Ok(item) = element
                    .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                        UIA_SelectionItemPatternId,
                    )
                {
                    let _ = item.Select();
                }
            }
            bring_forward(win.handle());
        }
        Plan::Front { win, .. } | Plan::Focus { win } => bring_forward(win.handle()),
        Plan::LaunchBrowser { exe, url } => {
            allow_foreground();
            let _ = Command::new(exe).arg(url).spawn();
        }
        Plan::Wake { win, aumid } => {
            launch_aumid(&aumid);
            let started = Instant::now();
            while started.elapsed() < WAKE_WAIT && !cancel() {
                std::thread::sleep(WAKE_POLL);
                if let Plan::Focus { win: shown } = search_desktop(target, &windows()) {
                    bring_forward(shown.handle());
                    return;
                }
            }
            if !cancel() && unsafe { IsWindow(Some(win.handle())) }.as_bool() {
                unsafe {
                    let _ = ShowWindow(win.handle(), SW_SHOW);
                }
                bring_forward(win.handle());
            }
        }
        Plan::Show { win } => {
            unsafe {
                let _ = ShowWindow(win.handle(), SW_SHOW);
            }
            bring_forward(win.handle());
        }
        Plan::LaunchAumid { aumid, .. } => launch_aumid(&aumid),
        Plan::Nothing(_) => {}
    }
}

fn allow_foreground() {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
}

/// Starts (or shows) the app with this AppUserModelID the way the Start menu
/// does; a single-instance app shows its existing window.
fn launch_aumid(aumid: &str) {
    allow_foreground();
    let _ = Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{aumid}"))
        .spawn();
}

/// Restores a minimized window and makes it the foreground one: no input
/// queue attaching, no synthetic keys. The island's own window is
/// `MA_NOACTIVATE`, so nothing takes it back.
fn bring_forward(hwnd: HWND) {
    allow_foreground();
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if !SetForegroundWindow(hwnd).as_bool() {
            let _ = BringWindowToTop(hwnd);
            SwitchToThisWindow(hwnd, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(source: &str, app_id: Option<&str>, site: Option<&str>, title: &str) -> OpenTarget {
        OpenTarget {
            app_id: app_id.map(String::from),
            source: source.into(),
            site: site.map(String::from),
            title: title.into(),
            body: "hai".into(),
        }
    }

    /// Prints what a Telegram Web toast from Chrome would do. Acts on nothing.
    #[test]
    #[ignore = "reads the real desktop; run with --ignored --nocapture"]
    fn plan_for_a_telegram_web_toast_in_chrome() {
        let t = target(
            "Google Chrome",
            Some("Chrome"),
            Some("web.telegram.org"),
            "Ibu",
        );
        println!("{}", plan(&t));
    }

    /// Prints what a Discord toast would do. Acts on nothing.
    #[test]
    #[ignore = "reads the real desktop; run with --ignored --nocapture"]
    fn plan_for_a_discord_toast() {
        let t = target(
            "Discord",
            Some("com.squirrel.Discord.Discord"),
            None,
            "Ibu",
        );
        println!("{}", plan(&t));
    }

    /// A toast with no AUMID is searched by display name; with nothing found
    /// it plans nothing (no launch).
    #[test]
    #[ignore = "reads the real desktop; run with --ignored --nocapture"]
    fn plan_for_a_toast_without_an_aumid() {
        let t = target("Vale Test App That Does Not Exist", None, None, "x");
        let text = plan(&t);
        println!("{text}");
        assert!(text.starts_with("nothing"), "{text}");
    }
}
