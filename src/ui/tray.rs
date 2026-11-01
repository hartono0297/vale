//! Notification-area icon, its menu, and the autostart registry value.

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, HICON, IDI_APPLICATION,
    LoadIconW, MF_CHECKED, MF_SEPARATOR, MF_STRING, PostMessageW, RegisterWindowMessageW,
    SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP,
    WM_NULL,
};
use windows::core::{Error, HSTRING, PCSTR, Result, w};

use crate::config::{Config, Style};
use crate::i18n::{Key, text, update_menu_install};
use crate::ui::app_icon;

/// Sent by the shell for mouse activity on the tray icon; `lparam` holds the
/// mouse message (legacy notify-icon version).
pub const WM_APP_TRAY: u32 = WM_APP + 2;
const TRAY_ID: u32 = 1;
const TOOLTIP: &str = "Vale";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Vale";
/// The Run value from before the rename; removed whenever autostart is set.
const OLD_RUN_VALUE: &str = "DynamicIsland";

/// The logo icon the tray currently shows, if loaded (null otherwise). Kept
/// alive while the icon is in the notification area and replaced on re-add.
static TRAY_ICON: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCmd {
    Settings,
    Search,
    StylePill,
    StyleNotch,
    /// "Cek pembaruan…": asks GitHub now.
    CheckUpdate,
    /// "Update ke X.Y.Z": in the menu in place of `CheckUpdate` while an
    /// update is known; installs it.
    InstallUpdate,
    Exit,
}

const COMMANDS: [MenuCmd; 7] = [
    MenuCmd::Settings,
    MenuCmd::Search,
    MenuCmd::StylePill,
    MenuCmd::StyleNotch,
    MenuCmd::CheckUpdate,
    MenuCmd::InstallUpdate,
    MenuCmd::Exit,
];

impl MenuCmd {
    /// Menu item id; 0 is what `TrackPopupMenu` returns for "nothing chosen".
    fn id(self) -> usize {
        COMMANDS.iter().position(|&c| c == self).unwrap_or(0) + 1
    }

    fn from_id(id: usize) -> Option<Self> {
        id.checked_sub(1).and_then(|i| COMMANDS.get(i).copied())
    }
}

#[derive(Debug, PartialEq)]
enum MenuItem {
    Entry {
        cmd: MenuCmd,
        label: String,
        checked: bool,
    },
    Separator,
}

/// The menu in `cfg.language`, built each time it opens, so it always
/// shows the current language. Everything else lives in the settings window.
/// `update`: the version (X.Y.Z) of a known, not yet installed update; its
/// item then installs it instead of checking again.
fn menu_items(cfg: &Config, update: Option<&str>) -> Vec<MenuItem> {
    let entry = |cmd, key, checked| MenuItem::Entry {
        cmd,
        label: text(cfg.language, key).to_owned(),
        checked,
    };
    let update_entry = match update {
        Some(version) => MenuItem::Entry {
            cmd: MenuCmd::InstallUpdate,
            label: update_menu_install(cfg.language, version),
            checked: false,
        },
        None => entry(MenuCmd::CheckUpdate, Key::MenuCheckUpdate, false),
    };
    vec![
        entry(MenuCmd::Settings, Key::MenuSettings, false),
        entry(MenuCmd::Search, Key::MenuSearch, false),
        MenuItem::Separator,
        entry(
            MenuCmd::StylePill,
            Key::MenuStylePill,
            cfg.style == Style::Pill,
        ),
        entry(
            MenuCmd::StyleNotch,
            Key::MenuStyleNotch,
            cfg.style == Style::Notch,
        ),
        MenuItem::Separator,
        update_entry,
        entry(MenuCmd::Exit, Key::MenuExit, false),
    ]
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        ..Default::default()
    }
}

/// Adds the tray icon. Call again when `is_taskbar_created` says so: the
/// icon is lost when Explorer restarts or was not up yet at logon.
pub fn add_tray(hwnd: HWND) -> Result<()> {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = WM_APP_TRAY;
    // Small-icon size for the system DPI: 16 px at 100 %, 20 at 125 %, ...
    let logo = app_icon::small_icon_for_dpi(unsafe { GetDpiForSystem() }).ok();
    data.hIcon = match logo {
        Some(icon) => icon,
        None => unsafe { LoadIconW(None, IDI_APPLICATION)? },
    };
    for (dst, src) in data.szTip.iter_mut().zip(TOOLTIP.encode_utf16()) {
        *dst = src;
    }
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.ok();
    let new = logo.map_or(std::ptr::null_mut(), |icon| icon.0);
    destroy_icon(TRAY_ICON.swap(new, Ordering::Relaxed));
    added
}

pub fn remove_tray(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
    }
    destroy_icon(TRAY_ICON.swap(std::ptr::null_mut(), Ordering::Relaxed));
}

fn destroy_icon(raw: *mut core::ffi::c_void) {
    if !raw.is_null() {
        unsafe {
            let _ = DestroyIcon(HICON(raw));
        }
    }
}

/// True for the message Explorer broadcasts when the taskbar is (re)created.
pub fn is_taskbar_created(msg: u32) -> bool {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    let registered =
        *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) });
    // 0 means registration failed; never mistake WM_NULL for it.
    registered != 0 && msg == registered
}

/// True when a `WM_APP_TRAY` message is a click that should open the menu.
pub fn is_menu_click(lparam: LPARAM) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{WM_LBUTTONUP, WM_RBUTTONUP};
    matches!(lparam.0 as u32, WM_LBUTTONUP | WM_RBUTTONUP)
}

/// `uxtheme.dll` ordinal of `SetPreferredAppMode(i32) -> i32` (1903+). On 1809
/// the same ordinal is `AllowDarkModeForApp(BOOL)`. Undocumented export, so it
/// is resolved by ordinal: it has no public name.
const ORD_SET_PREFERRED_APP_MODE: u16 = 135;
/// `uxtheme.dll` ordinal of `FlushMenuThemes()`. Undocumented, by ordinal.
const ORD_FLUSH_MENU_THEMES: u16 = 136;
/// `uxtheme.dll` ordinal of `RefreshImmersiveColorPolicyState()`. Undocumented,
/// by ordinal.
const ORD_REFRESH_COLOR_POLICY: u16 = 104;
/// `PreferredAppMode::AllowDark`: menus follow the Windows app mode (2 would
/// be ForceDark). Also reads as TRUE for `AllowDarkModeForApp` on 1809.
const APP_MODE_ALLOW_DARK: i32 = 1;

/// The undocumented uxtheme functions; each is `None` when the export is
/// missing, and then that step is silently skipped.
struct UxTheme {
    set_preferred_app_mode: Option<unsafe extern "system" fn(i32) -> i32>,
    flush_menu_themes: Option<unsafe extern "system" fn()>,
    refresh_color_policy: Option<unsafe extern "system" fn()>,
}

/// An export name that is an ordinal (MAKEINTRESOURCEA).
fn ordinal_name(ordinal: u16) -> PCSTR {
    PCSTR(ordinal as usize as *const u8)
}

/// Loads uxtheme.dll once and resolves the ordinals; `None` when it will not load.
fn ux_theme() -> Option<&'static UxTheme> {
    static UX: OnceLock<Option<UxTheme>> = OnceLock::new();
    UX.get_or_init(|| unsafe {
        let module = LoadLibraryExW(w!("uxtheme.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        // SAFETY: the signatures are the ones these exports have on Windows 10/11.
        Some(UxTheme {
            set_preferred_app_mode: GetProcAddress(
                module,
                ordinal_name(ORD_SET_PREFERRED_APP_MODE),
            )
            .map(|f| std::mem::transmute::<_, unsafe extern "system" fn(i32) -> i32>(f)),
            flush_menu_themes: GetProcAddress(module, ordinal_name(ORD_FLUSH_MENU_THEMES))
                .map(|f| std::mem::transmute::<_, unsafe extern "system" fn()>(f)),
            refresh_color_policy: GetProcAddress(module, ordinal_name(ORD_REFRESH_COLOR_POLICY))
                .map(|f| std::mem::transmute::<_, unsafe extern "system" fn()>(f)),
        })
    })
    .as_ref()
}

/// Makes this process's native menus follow the Windows app mode (light or
/// dark), whatever Vale's own window theme is. Call once before any menu
/// opens. Does nothing when the uxtheme exports are missing: the menu stays light.
pub fn follow_system_menu_theme() {
    let Some(ux) = ux_theme() else { return };
    unsafe {
        if let Some(set_mode) = ux.set_preferred_app_mode {
            set_mode(APP_MODE_ALLOW_DARK);
        }
        if let Some(flush) = ux.flush_menu_themes {
            flush();
        }
    }
}

/// Re-reads the Windows app mode for the next menu; call when Windows
/// switches between light and dark. Silent when the exports are missing.
pub fn refresh_menu_theme() {
    let Some(ux) = ux_theme() else { return };
    unsafe {
        if let Some(refresh) = ux.refresh_color_policy {
            refresh();
        }
        if let Some(flush) = ux.flush_menu_themes {
            flush();
        }
    }
}

/// Shows the menu at the cursor and waits for a choice. Runs a modal message
/// loop, so the caller must not hold any state borrow across it.
pub fn show_menu(hwnd: HWND, cfg: &Config, update: Option<&str>) -> Option<MenuCmd> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for item in menu_items(cfg, update) {
            let _ = match item {
                MenuItem::Entry {
                    cmd,
                    label,
                    checked,
                } => {
                    let flags = if checked {
                        MF_STRING | MF_CHECKED
                    } else {
                        MF_STRING
                    };
                    AppendMenuW(menu, flags, cmd.id(), &HSTRING::from(label))
                }
                MenuItem::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, None),
            };
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Without the foreground the menu would not close on a click elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        // Lets the menu dismiss properly the next time it opens.
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        MenuCmd::from_id(chosen.0 as usize)
    }
}

/// The old `Run\DynamicIsland` command starts the island's old exe.
fn is_old_island(command: &str) -> bool {
    const EXE: &str = "dynamic-island.exe";
    let command = command.to_lowercase();
    command.match_indices(EXE).any(|(at, _)| {
        command[..at]
            .chars()
            .next_back()
            .is_none_or(|c| matches!(c, '\\' | '/' | '"'))
    })
}

/// The text of `HKCU\...\Run\<name>` in the open `key`; `None` when absent.
unsafe fn read_run_value(key: HKEY, name: &str) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            &HSTRING::from(name),
            None,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = (size as usize / 2).min(buf.len());
    let text = String::from_utf16_lossy(&buf[..len]);
    Some(text.trim_end_matches('\0').to_string())
}

/// `"<exe>"`, the command the Run value holds.
fn run_command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Writes or deletes `HKCU\...\Run\Vale` for the current exe, and deletes
/// the old `Run\DynamicIsland` when it starts the old exe. Debug
/// builds leave the registry alone, so `cargo run` never points autostart at
/// `target\debug`.
pub fn set_autostart(enabled: bool) -> Result<()> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_QUERY_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
        // The old value starts a Dynamic Island that is no longer there;
        // under that name another app's entry is left alone.
        if read_run_value(key, OLD_RUN_VALUE).is_some_and(|c| is_old_island(&c)) {
            let _ = RegDeleteValueW(key, &HSTRING::from(OLD_RUN_VALUE));
        }
        let result = if enabled {
            write_run_value(key)
        } else {
            match RegDeleteValueW(key, &HSTRING::from(RUN_VALUE)) {
                ERROR_FILE_NOT_FOUND => Ok(()),
                status => status.ok(),
            }
        };
        let _ = RegCloseKey(key);
        result
    }
}

unsafe fn write_run_value(key: HKEY) -> Result<()> {
    let exe = std::env::current_exe().map_err(Error::from)?;
    let bytes: Vec<u8> = run_command(&exe)
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    unsafe { RegSetValueExW(key, &HSTRING::from(RUN_VALUE), None, REG_SZ, Some(&bytes)) }.ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    fn labels(items: &[MenuItem]) -> Vec<&str> {
        items
            .iter()
            .map(|item| match item {
                MenuItem::Entry { label, .. } => label.as_str(),
                MenuItem::Separator => "-",
            })
            .collect()
    }

    fn commands(items: &[MenuItem]) -> Vec<MenuCmd> {
        items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Entry { cmd, .. } => Some(*cmd),
                MenuItem::Separator => None,
            })
            .collect()
    }

    fn checked(items: &[MenuItem]) -> Vec<MenuCmd> {
        items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Entry {
                    cmd, checked: true, ..
                } => Some(*cmd),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn ordinal_name_is_the_ordinal_as_a_pointer() {
        assert_eq!(ordinal_name(135).0 as usize, 135);
        assert_eq!(ordinal_name(136).0 as usize, 136);
    }

    #[test]
    fn menu_theme_calls_do_not_panic() {
        follow_system_menu_theme();
        follow_system_menu_theme();
        refresh_menu_theme();
        refresh_menu_theme();
    }

    #[test]
    fn menu_labels_in_order() {
        assert_eq!(
            labels(&menu_items(&Config::default(), None)),
            vec![
                "Pengaturan…",
                "Cari…",
                "-",
                "Gaya: Pill",
                "Gaya: Notch",
                "-",
                "Cek pembaruan…",
                "Keluar"
            ]
        );
    }

    #[test]
    fn a_known_update_takes_the_place_of_the_check_item() {
        let id = labels(&menu_items(&Config::default(), Some("3.11.0"))).join("|");
        assert_eq!(
            id,
            "Pengaturan…|Cari…|-|Gaya: Pill|Gaya: Notch|-|Update ke 3.11.0|Keluar"
        );
        let en = Config {
            language: Lang::En,
            ..Config::default()
        };
        let en = labels(&menu_items(&en, Some("3.11.0"))).join("|");
        assert_eq!(
            en,
            "Settings…|Search…|-|Style: Pill|Style: Notch|-|Update to 3.11.0|Quit"
        );
        let with = commands(&menu_items(&Config::default(), Some("3.11.0")));
        assert!(with.contains(&MenuCmd::InstallUpdate));
        assert!(!with.contains(&MenuCmd::CheckUpdate));
        let without = commands(&menu_items(&Config::default(), None));
        assert!(without.contains(&MenuCmd::CheckUpdate));
        assert!(!without.contains(&MenuCmd::InstallUpdate));
    }

    #[test]
    fn menu_labels_in_english() {
        let en = Config {
            language: Lang::En,
            ..Config::default()
        };
        assert_eq!(
            labels(&menu_items(&en, None)),
            vec![
                "Settings…",
                "Search…",
                "-",
                "Style: Pill",
                "Style: Notch",
                "-",
                "Check for updates…",
                "Quit"
            ]
        );
    }

    #[test]
    fn settings_comes_first() {
        let items = menu_items(&Config::default(), None);
        assert!(matches!(
            items.first(),
            Some(MenuItem::Entry {
                cmd: MenuCmd::Settings,
                checked: false,
                ..
            })
        ));
    }

    #[test]
    fn checks_follow_config() {
        let pill = Config::default();
        assert_eq!(checked(&menu_items(&pill, None)), vec![MenuCmd::StylePill]);
        let notch = Config {
            style: Style::Notch,
            start_with_windows: false,
            ..Config::default()
        };
        assert_eq!(checked(&menu_items(&notch, None)), vec![MenuCmd::StyleNotch]);
    }

    #[test]
    fn ids_round_trip_and_zero_is_nothing() {
        for cmd in COMMANDS {
            assert_ne!(cmd.id(), 0);
            assert_eq!(MenuCmd::from_id(cmd.id()), Some(cmd));
        }
        assert_eq!(MenuCmd::from_id(0), None);
        assert_eq!(MenuCmd::from_id(COMMANDS.len() + 1), None);
    }

    #[test]
    fn every_command_is_in_the_menu() {
        // The install item only shows once an update is known.
        let mut reachable = commands(&menu_items(&Config::default(), None));
        reachable.extend(commands(&menu_items(&Config::default(), Some("3.11.0"))));
        for cmd in COMMANDS {
            assert!(reachable.contains(&cmd), "{cmd:?}");
        }
        // And Quit stays last, update known or not.
        for update in [None, Some("3.11.0")] {
            let items = commands(&menu_items(&Config::default(), update));
            assert_eq!(items.last(), Some(&MenuCmd::Exit));
        }
    }

    #[test]
    fn run_command_quotes_the_exe() {
        assert_eq!(
            run_command(Path::new(r"D:\Program Files\vale.exe")),
            r#""D:\Program Files\vale.exe""#
        );
    }

    #[test]
    fn only_the_old_islands_autostart_is_removed() {
        let old = r#""C:\Users\x\AppData\Local\Programs\DynamicIsland\dynamic-island.exe""#;
        assert!(is_old_island(old));
        assert!(is_old_island(r"D:\Tools\Dynamic-Island.EXE --quiet"));
        assert!(is_old_island("dynamic-island.exe"));
        // Another app under the same value name keeps its autostart.
        assert!(!is_old_island(r#""C:\Apps\DynamicIsland\island.exe""#));
        assert!(!is_old_island(r"C:\x\my-dynamic-island.exe"));
        assert!(!is_old_island(""));
    }
}
