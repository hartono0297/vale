//! Shows a saved screenshot in File Explorer: its folder opens with the file
//! selected ("klik untuk buka" on the "Disimpan" notice, `Effect::RevealFile`).
//!
//! Everything runs on a short-lived worker (`shot-reveal`), never on the UI
//! thread: the shell call can take a moment and COM must not run there.
//!
//! - The file exists: `ILCreateFromPathW` + `SHOpenFolderAndSelectItems` with
//!   no items (cidl 0) opens the parent and selects the file; the shell reuses
//!   an Explorer window already on that folder.
//! - The file is gone but its folder is there: the folder opens (`ShellExecuteW`).
//! - Neither: a line on stderr, nothing launched.
//!
//! Best effort afterwards (`follow_up`): for up to `FOLLOW_UP_MS` the worker
//! looks for the Explorer window showing that folder and brings it forward
//! when it is not already in front (`notif_open::bring_forward`: no input
//! queue attaching, no synthetic keys). `App` calls `AllowSetForegroundWindow`
//! first, on the UI thread, right after the click that asked for this. The
//! island's own window stays no-activate.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SW_SHOWNORMAL};
use windows::core::{HSTRING, w};

use super::{explorer, notif_open};

/// How long the worker looks for the Explorer window afterwards.
const FOLLOW_UP_MS: u64 = 1500;
/// How often it looks.
const POLL_MS: u64 = 100;

/// What to do for a file, from what is on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealStep {
    /// Open the parent folder with the file selected.
    SelectFile,
    /// The file is gone: open its folder.
    OpenFolder,
    /// Nothing to show.
    Nothing,
}

/// Pure: the step for a file that exists or not and its folder that does.
pub fn reveal_step(file_exists: bool, dir_exists: bool) -> RevealStep {
    match (file_exists, dir_exists) {
        (true, _) => RevealStep::SelectFile,
        (false, true) => RevealStep::OpenFolder,
        (false, false) => RevealStep::Nothing,
    }
}

/// Reveals `file` in Explorer on a new thread; returns at once.
pub fn reveal(file: PathBuf) {
    let spawned = std::thread::Builder::new()
        .name("shot-reveal".into())
        .spawn(move || run(&file));
    if let Err(error) = spawned {
        eprintln!("vale: cannot start the reveal worker: {error}");
    }
}

fn run(file: &Path) {
    let dir = file.parent().map(Path::to_path_buf);
    let step = reveal_step(
        file.is_file(),
        dir.as_deref().is_some_and(Path::is_dir),
    );
    if step == RevealStep::Nothing {
        eprintln!("vale: nothing to reveal, {} is gone", file.display());
        return;
    }
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let shown = match (step, &dir) {
        (RevealStep::SelectFile, _) => select_file(file) || dir.as_deref().is_some_and(open_folder),
        (_, Some(dir)) => open_folder(dir),
        _ => false,
    };
    if shown && let Some(dir) = &dir {
        follow_up(dir);
    }
    if com {
        unsafe { CoUninitialize() };
    }
}

/// Opens the file's folder and selects it. False when the shell refused.
fn select_file(file: &Path) -> bool {
    let path = HSTRING::from(file.as_os_str());
    unsafe {
        let pidl = ILCreateFromPathW(&path);
        if pidl.is_null() {
            return false;
        }
        let done = SHOpenFolderAndSelectItems(pidl, None, 0);
        ILFree(Some(pidl));
        if let Err(error) = &done {
            eprintln!("vale: cannot select {}: {error}", file.display());
        }
        done.is_ok()
    }
}

/// Opens `dir` in Explorer. False when ShellExecute reported an error
/// (a value of 32 or less).
fn open_folder(dir: &Path) -> bool {
    let path = HSTRING::from(dir.as_os_str());
    let result = unsafe { ShellExecuteW(None, w!("open"), &path, None, None, SW_SHOWNORMAL) };
    result.0 as isize > 32
}

/// Brings the Explorer window showing `dir` forward, when it is not already
/// in front: looks every `POLL_MS` for up to `FOLLOW_UP_MS`, stops at the
/// first match.
fn follow_up(dir: &Path) {
    let wanted = dir.to_string_lossy();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(FOLLOW_UP_MS) {
        let found = explorer::shown_folders()
            .into_iter()
            .find(|(_, folder)| explorer::same_folder(&folder.to_string_lossy(), &wanted));
        if let Some((hwnd, _)) = found {
            if unsafe { GetForegroundWindow() } != hwnd {
                notif_open::bring_forward(hwnd);
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(POLL_MS));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_existing_file_is_selected_in_its_folder() {
        assert_eq!(reveal_step(true, true), RevealStep::SelectFile);
        assert_eq!(reveal_step(true, false), RevealStep::SelectFile);
    }

    #[test]
    fn a_missing_file_opens_its_folder_when_that_is_there() {
        assert_eq!(reveal_step(false, true), RevealStep::OpenFolder);
    }

    #[test]
    fn nothing_is_launched_when_both_are_gone() {
        assert_eq!(reveal_step(false, false), RevealStep::Nothing);
    }
}
