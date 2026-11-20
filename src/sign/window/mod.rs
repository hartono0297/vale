//! The signing window. Stub until the editor lands.

use std::path::PathBuf;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

use crate::sign::Source;

pub const WM_APP_SIGN: u32 = WM_APP + 96;

pub enum Report {
    Saved(PathBuf),
}

pub fn open(_owner: HWND, _src: Source, _demo: bool) -> bool {
    false
}

pub fn release() {}

pub fn restyle_open() {}

#[cfg(debug_assertions)]
pub fn run_demo() {}

pub fn take_reports() -> Vec<Report> {
    Vec::new()
}
