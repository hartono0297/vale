//! The folders open in File Explorer, the last active window first, read
//! once on request on a short-lived background thread (COM never runs on
//! the UI thread). `IShellWindows` lists every shell browser; each one's
//! active view gives its folder (`IShellBrowser` → `IShellView` →
//! `IFolderView` → `IPersistFolder2` → a PIDL → a path). Virtual locations
//! (Home, This PC, Recycle Bin, Gallery) have no file-system path and are
//! left out. Kept thin; what it returns is used by `cli_folders`.
//!
//! - Windows are ordered by z-order (`EnumWindows` goes top first), which
//!   is the order they were last active in.
//! - Windows 11 tabs: every tab of a window is its own item with the same
//!   top-level HWND. The tab on show is the one whose `ShellTabWindowClass`
//!   window (`IShellBrowser::GetWindow`) is visible; it goes first.
//! - Only File Explorer (`CabinetWClass`), not other shell browsers.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, IServiceProvider,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Shell::{
    GPFIDL_DEFAULT, IFolderView, IPersistFolder2, IShellBrowser, IShellWindows, IWebBrowserApp,
    SHGetPathFromIDListEx, SID_STopLevelBrowser, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, IsWindowVisible};
use windows::core::{BOOL, Interface};

use crate::model::Event;

/// One Explorer tab: its window, whether it is the one on show, its folder.
struct Tab {
    top: HWND,
    shown: bool,
    path: PathBuf,
}

/// Reads the folders on a new thread and sends them as
/// `Event::ExplorerFolders { request, .. }`, then calls `wake`. Any error
/// gives an empty list. False when the thread could not start (no event
/// follows).
pub fn read_folders(request: u64, tx: Sender<Event>, wake: impl Fn() + Send + 'static) -> bool {
    let spawned = std::thread::Builder::new()
        .name("explorer-folders".into())
        .spawn(move || {
            let folders = unsafe {
                let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
                // Every COM object is dropped inside, before the uninit.
                let folders = open_folders().unwrap_or_default();
                if com {
                    CoUninitialize();
                }
                folders
            };
            if tx.send(Event::ExplorerFolders { request, folders }).is_ok() {
                wake();
            }
        });
    match spawned {
        Ok(_) => true,
        Err(error) => {
            eprintln!("vale: cannot read Explorer folders: {error}");
            false
        }
    }
}

/// Each File Explorer window's shown tab: its top-level window and folder
/// (`sources::reveal` looks for the window of a folder it just opened).
/// Runs on the calling thread, which sets COM up itself; call it from a
/// worker, never the UI thread. Any error gives an empty list.
pub fn shown_folders() -> Vec<(HWND, PathBuf)> {
    unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        // Every COM object is dropped inside, before the uninit.
        let tabs = tabs().unwrap_or_default();
        if com {
            CoUninitialize();
        }
        tabs.into_iter()
            .filter(|t| t.shown)
            .map(|t| (t.top, t.path))
            .collect()
    }
}

/// True when `a` and `b` name the same folder: case does not matter (NTFS)
/// and a trailing separator does not either. Pure text; no disk access.
pub fn same_folder(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let t = s.replace('/', "\\");
        // "C:\" keeps its separator: it is the drive's root.
        let keep = if t.len() > 3 {
            t.trim_end_matches('\\')
        } else {
            &t
        };
        keep.to_lowercase()
    };
    norm(a) == norm(b)
}

/// The folders, the last active window first, each window's shown tab
/// before its other tabs.
unsafe fn open_folders() -> windows::core::Result<Vec<PathBuf>> {
    Ok(unsafe { tabs() }?.into_iter().map(|t| t.path).collect())
}

/// Every Explorer tab, the last active window first, each window's shown
/// tab before its other tabs.
unsafe fn tabs() -> windows::core::Result<Vec<Tab>> {
    let windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)? };
    let count = unsafe { windows.Count()? };
    let mut tabs = Vec::new();
    for i in 0..count {
        if let Some(tab) = unsafe { tab_at(&windows, i) } {
            tabs.push(tab);
        }
    }
    let order = z_order();
    let rank = |top: HWND| order.iter().position(|h| *h == top).unwrap_or(usize::MAX);
    // Stable: tabs of one window keep their own order after the shown one.
    tabs.sort_by_key(|t| (rank(t.top), !t.shown));
    Ok(tabs)
}

/// Item `i`, if it is a File Explorer tab showing a file-system folder.
unsafe fn tab_at(windows: &IShellWindows, i: i32) -> Option<Tab> {
    let mut index = VARIANT::default();
    unsafe {
        let v = &mut index.Anonymous.Anonymous;
        v.vt = VT_I4;
        v.Anonymous.lVal = i;
    }
    let item = unsafe { windows.Item(&index) }.ok()?;
    let browser: IWebBrowserApp = item.cast().ok()?;
    let top = HWND(unsafe { browser.HWND() }.ok()?.0 as *mut _);
    if !is_explorer(top) {
        return None;
    }
    let services: IServiceProvider = browser.cast().ok()?;
    let shell: IShellBrowser = unsafe { services.QueryService(&SID_STopLevelBrowser) }.ok()?;
    let shown =
        unsafe { shell.GetWindow() }.is_ok_and(|tab| unsafe { IsWindowVisible(tab) }.as_bool());
    let view = unsafe { shell.QueryActiveShellView() }.ok()?;
    let folder: IFolderView = view.cast().ok()?;
    let persist: IPersistFolder2 = unsafe { folder.GetFolder() }.ok()?;
    let pidl = unsafe { persist.GetCurFolder() }.ok()?;
    // Long paths too (past MAX_PATH): the Ex form takes any buffer.
    let mut buf = vec![0u16; 32_768];
    let ok = unsafe { SHGetPathFromIDListEx(pidl, &mut buf, GPFIDL_DEFAULT) }.as_bool();
    unsafe { CoTaskMemFree(Some(pidl as *const _)) };
    if !ok {
        // A virtual location: Home, This PC, Recycle Bin, Gallery.
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let path = PathBuf::from(String::from_utf16_lossy(&buf[..len]));
    path.is_dir().then_some(Tab { top, shown, path })
}

/// A File Explorer window (not another shell browser).
fn is_explorer(hwnd: HWND) -> bool {
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut class) };
    n > 0 && String::from_utf16_lossy(&class[..n as usize]) == "CabinetWClass"
}

/// Top-level windows, the topmost (last active) first.
fn z_order() -> Vec<HWND> {
    unsafe extern "system" fn add(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        list.push(hwnd);
        BOOL(1)
    }
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(add), LPARAM(&mut list as *mut Vec<HWND> as isize));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::same_folder;

    #[test]
    fn same_folder_ignores_case_and_a_trailing_separator() {
        assert!(same_folder(
            r"C:\Users\vale\Pictures\Screenshots",
            r"c:\users\VALE\pictures\screenshots\"
        ));
        assert!(same_folder(r"D:\Shots\", r"D:\Shots"));
        assert!(same_folder("D:/Shots", r"D:\Shots"));
        assert!(same_folder(r"C:\", r"c:\"));
        assert!(!same_folder(r"C:\Users\vale", r"C:\Users\vale\Pictures"));
        assert!(!same_folder(r"C:\Shots", r"C:\Shots2"));
    }

    /// By hand, with Explorer windows open:
    /// `cargo test print_open_folders -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn print_open_folders() {
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(super::read_folders(1, tx, || {}));
        let event = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the read answers");
        println!("{event:?}");
    }
}
