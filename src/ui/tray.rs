//! Notification-area icon, its menu, and the autostart registry value.

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

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
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    HICON, HMENU, IDI_APPLICATION, LoadIconW, MF_BYCOMMAND, MF_CHECKED, MF_GRAYED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, PostMessageW, RegisterWindowMessageW, SetForegroundWindow,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_NULL,
};
use windows::core::{Error, HSTRING, PCSTR, Result, w};

use crate::config::{Config, NotchMonitor, Style};
use crate::i18n::{Key, monitor_entry, text, update_menu_install};
use crate::notch_monitor::{Display, choice_for, display_name, same_path, saved_missing};
use crate::ui::app_icon;
use crate::ui::tray_dot;

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

/// A recording runs: the tray icon carries the red dot and the menu entry
/// stops it. Kept here so a re-added icon (the taskbar was re-created) gets
/// the dot back.
static RECORDING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCmd {
    Settings,
    Search,
    /// "Catatan tempel baru": one empty sticky note on the desktop.
    StickyNote,
    /// "Daftar catatan tempel": opens the sticky note list window.
    StickyList,
    /// "Mulai rekam" / "Hentikan rekam": the audio recorder, like its hotkey.
    Record,
    StylePill,
    StyleNotch,
    /// "Jalan saat Windows mulai": a check item that flips `start_with_windows`.
    Autostart,
    /// "Cek pembaruan…": asks GitHub now.
    CheckUpdate,
    /// "Update ke X.Y.Z": in the menu in place of `CheckUpdate` while an
    /// update is known; installs it.
    InstallUpdate,
    Exit,
}

const COMMANDS: [MenuCmd; 11] = [
    MenuCmd::Settings,
    MenuCmd::Search,
    MenuCmd::StickyNote,
    MenuCmd::StickyList,
    MenuCmd::Record,
    MenuCmd::StylePill,
    MenuCmd::StyleNotch,
    MenuCmd::Autostart,
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

/// What a pick in the menu means: one of the plain commands, or the screen the
/// notch should be on.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuChoice {
    Cmd(MenuCmd),
    Monitor(NotchMonitor),
}

/// The Monitor submenu's ids start here, clear of `MenuCmd::id` (1..=11):
/// 100 is "Monitor utama", 101 + i the display at place i of the list the menu
/// was built from.
const MONITOR_ID_PRIMARY: usize = 100;

/// One radio entry of the Monitor submenu.
#[derive(Debug, PartialEq)]
struct MonitorItem {
    id: usize,
    label: String,
    checked: bool,
    /// Greyed out: a display with no device path, or the saved one that is
    /// not connected.
    enabled: bool,
}

#[derive(Debug, PartialEq)]
enum MenuItem {
    Entry {
        cmd: MenuCmd,
        label: String,
        checked: bool,
    },
    /// An entry that cannot be chosen now; its label says why.
    Disabled {
        cmd: MenuCmd,
        label: String,
    },
    Separator,
    /// A popup of radio entries.
    Submenu {
        label: String,
        items: Vec<MonitorItem>,
    },
}

/// The Monitor submenu from the saved choice and the displays now: "Monitor
/// utama", each display as "N · name" (greyed when it cannot be chosen), and
/// the saved one when it is not connected, greyed and checked.
fn monitor_items(cfg: &Config, displays: &[Display]) -> Vec<MonitorItem> {
    let lang = cfg.language;
    let mut items = vec![MonitorItem {
        id: MONITOR_ID_PRIMARY,
        label: text(lang, Key::MonitorPrimary).to_owned(),
        checked: cfg.notch_monitor == NotchMonitor::Primary,
        enabled: true,
    }];
    for (i, d) in displays.iter().enumerate() {
        let checked = matches!(&cfg.notch_monitor,
            NotchMonitor::Chosen { path, .. } if same_path(&d.path, path));
        items.push(MonitorItem {
            id: MONITOR_ID_PRIMARY + 1 + i,
            label: monitor_entry(d.number, &display_name(d)),
            checked,
            enabled: d.selectable(),
        });
    }
    if let NotchMonitor::Chosen { name, .. } = &cfg.notch_monitor
        && saved_missing(&cfg.notch_monitor, displays)
    {
        let name = if name.trim().is_empty() {
            text(lang, Key::MonitorChoice)
        } else {
            name.as_str()
        };
        items.push(MonitorItem {
            id: MONITOR_ID_PRIMARY + 1 + displays.len(),
            label: format!("{name} ({})", text(lang, Key::MonitorNotConnected)),
            checked: true,
            enabled: false,
        });
    }
    items
}

/// The choice a Monitor id of a menu built from `displays` makes; `None` for
/// an id that is not one, or a display that cannot be chosen.
fn monitor_choice(id: usize, displays: &[Display]) -> Option<NotchMonitor> {
    if id == MONITOR_ID_PRIMARY {
        return Some(NotchMonitor::Primary);
    }
    let i = id.checked_sub(MONITOR_ID_PRIMARY + 1)?;
    choice_for(displays.get(i)?)
}

/// The menu in `cfg.language`, built each time it opens, so it always
/// shows the current language. Everything else lives in the settings window.
/// `update`: the version (X.Y.Z) of a known, not yet installed update; its
/// item then installs it instead of checking again. `displays`: the screens
/// connected now (the Monitor submenu).
fn menu_items(cfg: &Config, update: Option<&str>, displays: &[Display]) -> Vec<MenuItem> {
    menu_items_with(
        cfg,
        update,
        displays,
        crate::widgets::sticky_count(),
        RECORDING.load(Ordering::Relaxed),
    )
}

/// `menu_items` with the number of sticky notes on the desktop (the new-note
/// entry is greyed, with the reason, at the cap) and whether a recording runs
/// (the record entry then stops it).
fn menu_items_with(
    cfg: &Config,
    update: Option<&str>,
    displays: &[Display],
    stickies: usize,
    recording: bool,
) -> Vec<MenuItem> {
    let sticky = if crate::widgets::sticky::can_add(stickies) {
        MenuItem::Entry {
            cmd: MenuCmd::StickyNote,
            label: text(cfg.language, Key::StickyNew).to_owned(),
            checked: false,
        }
    } else {
        MenuItem::Disabled {
            cmd: MenuCmd::StickyNote,
            label: text(cfg.language, Key::StickyNewMax).to_owned(),
        }
    };
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
        sticky,
        entry(MenuCmd::StickyList, Key::StickyList, false),
        entry(
            MenuCmd::Record,
            if recording { Key::TrayRecStop } else { Key::TrayRecStart },
            false,
        ),
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
        MenuItem::Submenu {
            label: text(cfg.language, Key::MonitorChoice).to_owned(),
            items: monitor_items(cfg, displays),
        },
        MenuItem::Separator,
        // Built from the config each time the menu opens, so a change made in
        // Settings shows up here without any extra sync.
        entry(
            MenuCmd::Autostart,
            Key::Autostart,
            cfg.start_with_windows,
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
    let logo = tray_icon();
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

/// The icon the tray shows now: the logo at the small-icon size, with the
/// recording dot while one runs (the plain logo if the dot cannot be made).
fn tray_icon() -> Option<HICON> {
    let logo = app_icon::small_icon_for_dpi(unsafe { GetDpiForSystem() }).ok()?;
    if !RECORDING.load(Ordering::Relaxed) {
        return Some(logo);
    }
    match tray_dot::with_rec_dot(logo) {
        Some(dotted) => {
            destroy_icon(logo.0);
            Some(dotted)
        }
        None => Some(logo),
    }
}

/// Starts or ends the recording state of the tray: swaps the icon in place
/// (`NIM_MODIFY`) and flips the menu's entry. The old icon is destroyed after
/// the shell has the new one.
pub fn set_recording(hwnd: HWND, on: bool) {
    if RECORDING.swap(on, Ordering::Relaxed) == on {
        return;
    }
    let Some(icon) = tray_icon() else { return };
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_ICON;
    data.hIcon = icon;
    if unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool() {
        destroy_icon(TRAY_ICON.swap(icon.0, Ordering::Relaxed));
    } else {
        // The icon is not in the notification area (yet): `add_tray` builds
        // it from the flag when the taskbar comes back.
        destroy_icon(icon.0);
    }
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
/// loop, so the caller must not hold any state borrow across it. `displays`
/// are the screens now, for the Monitor submenu; a pick there comes back as
/// the choice it makes (resolved against this same list).
pub fn show_menu(
    hwnd: HWND,
    cfg: &Config,
    update: Option<&str>,
    displays: &[Display],
) -> Option<MenuChoice> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for item in menu_items(cfg, update, displays) {
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
                MenuItem::Disabled { cmd, label } => AppendMenuW(
                    menu,
                    MF_STRING | MF_GRAYED,
                    cmd.id(),
                    &HSTRING::from(label),
                ),
                MenuItem::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, None),
                MenuItem::Submenu { label, items } => match CreatePopupMenu() {
                    Ok(sub) => {
                        append_radio_items(sub, &items);
                        // The popup's handle stands in for the item id.
                        AppendMenuW(menu, MF_POPUP, sub.0 as usize, &HSTRING::from(label))
                    }
                    Err(e) => Err(e),
                },
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
        // Destroying the menu destroys its submenus.
        let _ = DestroyMenu(menu);
        let id = chosen.0 as usize;
        if id >= MONITOR_ID_PRIMARY {
            return monitor_choice(id, displays).map(MenuChoice::Monitor);
        }
        MenuCmd::from_id(id).map(MenuChoice::Cmd)
    }
}

/// Shows a one-item menu (`label`) at the cursor; true when it was chosen.
/// Modal like `show_menu`: the caller must not hold any state borrow.
pub fn note_card_menu(hwnd: HWND, label: &str) -> bool {
    const CMD: usize = 1;
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return false;
        };
        let _ = AppendMenuW(menu, MF_STRING, CMD, &HSTRING::from(label));
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
        chosen.0 as usize == CMD
    }
}

/// Fills `menu` with the Monitor entries as a radio group: the checked one
/// gets the radio bullet (`CheckMenuRadioItem` sets `MFT_RADIOCHECK` on it),
/// the entries that cannot be chosen are greyed.
unsafe fn append_radio_items(menu: HMENU, items: &[MonitorItem]) {
    for item in items {
        let flags = if item.enabled {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        unsafe {
            let _ = AppendMenuW(menu, flags, item.id, &HSTRING::from(item.label.as_str()));
        }
    }
    if let (Some(first), Some(last)) = (items.first(), items.last())
        && let Some(on) = items.iter().find(|i| i.checked)
    {
        unsafe {
            let _ = CheckMenuRadioItem(
                menu,
                first.id as u32,
                last.id as u32,
                on.id as u32,
                MF_BYCOMMAND.0,
            );
        }
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
                MenuItem::Entry { label, .. } | MenuItem::Disabled { label, .. } => label.as_str(),
                MenuItem::Separator => "-",
                MenuItem::Submenu { label, .. } => label.as_str(),
            })
            .collect()
    }

    fn commands(items: &[MenuItem]) -> Vec<MenuCmd> {
        items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Entry { cmd, .. } | MenuItem::Disabled { cmd, .. } => Some(*cmd),
                MenuItem::Separator | MenuItem::Submenu { .. } => None,
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
            labels(&menu_items(&Config::default(), None, &[])),
            vec![
                "Pengaturan…",
                "Cari…",
                "Catatan tempel baru",
                "Daftar catatan tempel",
                "Mulai rekam",
                "-",
                "Gaya: Pill",
                "Gaya: Notch",
                "Monitor",
                "-",
                "Jalan saat Windows mulai",
                "-",
                "Cek pembaruan…",
                "Keluar"
            ]
        );
    }

    #[test]
    fn the_new_note_entry_is_greyed_with_the_reason_at_the_cap() {
        let cfg = Config::default();
        let few = menu_items_with(&cfg, None, &[], 19, false);
        assert!(matches!(
            &few[2],
            MenuItem::Entry { cmd: MenuCmd::StickyNote, label, .. } if label == "Catatan tempel baru"
        ));
        let full = menu_items_with(&cfg, None, &[], 20, false);
        assert!(matches!(
            &full[2],
            MenuItem::Disabled { cmd: MenuCmd::StickyNote, label }
                if label == "Catatan tempel baru (maksimal 20)"
        ));
        let en = Config {
            language: Lang::En,
            ..Config::default()
        };
        assert_eq!(
            labels(&menu_items_with(&en, None, &[], 20, false))[2],
            "New sticky note (at most 20)"
        );
    }

    #[test]
    fn the_list_entry_follows_the_new_note_entry_and_is_always_enabled() {
        let pos = COMMANDS.iter().position(|c| *c == MenuCmd::StickyNote).unwrap();
        assert_eq!(COMMANDS[pos + 1], MenuCmd::StickyList, "right after the new note entry");
        for lang in [Lang::Id, Lang::En] {
            let cfg = Config { language: lang, ..Config::default() };
            let want = match lang {
                Lang::Id => "Daftar catatan tempel",
                Lang::En => "Sticky note list",
            };
            // Enabled with few notes, with none and at the cap.
            for n in [0, 5, 19, 20] {
                let items = menu_items_with(&cfg, None, &[], n, false);
                assert!(
                    matches!(&items[3], MenuItem::Entry { cmd: MenuCmd::StickyList, label, checked: false } if label == want),
                    "{lang:?} with {n}: {:?}",
                    items[3]
                );
            }
        }
    }

    #[test]
    fn the_record_entry_follows_the_sticky_list_and_turns_into_stop_while_recording() {
        let pos = COMMANDS.iter().position(|c| *c == MenuCmd::StickyList).unwrap();
        assert_eq!(COMMANDS[pos + 1], MenuCmd::Record);
        for (lang, start, stop) in [
            (Lang::Id, "Mulai rekam", "Hentikan rekam"),
            (Lang::En, "Start recording", "Stop recording"),
        ] {
            let cfg = Config { language: lang, ..Config::default() };
            for (recording, want) in [(false, start), (true, stop)] {
                let items = menu_items_with(&cfg, None, &[], 0, recording);
                assert!(
                    matches!(&items[4], MenuItem::Entry { cmd: MenuCmd::Record, label, checked: false } if label == want),
                    "{lang:?} {recording}: {:?}",
                    items[4]
                );
            }
        }
        // The ids stay unique and clear of the Monitor submenu's.
        let mut ids: Vec<usize> = COMMANDS.iter().map(|c| c.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), COMMANDS.len());
        assert!(ids.iter().all(|id| *id < MONITOR_ID_PRIMARY));
    }

    #[test]
    fn a_known_update_takes_the_place_of_the_check_item() {
        let id = labels(&menu_items(&Config::default(), Some("3.11.0"), &[])).join("|");
        assert_eq!(
            id,
            "Pengaturan…|Cari…|Catatan tempel baru|Daftar catatan tempel|Mulai rekam|-|Gaya: Pill|Gaya: Notch|Monitor|-|Jalan saat Windows mulai|-|Update ke 3.11.0|Keluar"
        );
        let en = Config {
            language: Lang::En,
            ..Config::default()
        };
        let en = labels(&menu_items(&en, Some("3.11.0"), &[])).join("|");
        assert_eq!(
            en,
            "Settings…|Search…|New sticky note|Sticky note list|Start recording|-|Style: Pill|Style: Notch|Monitor|-|Start with Windows|-|Update to 3.11.0|Quit"
        );
        let with = commands(&menu_items(&Config::default(), Some("3.11.0"), &[]));
        assert!(with.contains(&MenuCmd::InstallUpdate));
        assert!(!with.contains(&MenuCmd::CheckUpdate));
        let without = commands(&menu_items(&Config::default(), None, &[]));
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
            labels(&menu_items(&en, None, &[])),
            vec![
                "Settings…",
                "Search…",
                "New sticky note",
                "Sticky note list",
                "Start recording",
                "-",
                "Style: Pill",
                "Style: Notch",
                "Monitor",
                "-",
                "Start with Windows",
                "-",
                "Check for updates…",
                "Quit"
            ]
        );
    }

    #[test]
    fn settings_comes_first() {
        let items = menu_items(&Config::default(), None, &[]);
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
        assert_eq!(
            checked(&menu_items(&pill, None, &[])),
            vec![MenuCmd::StylePill, MenuCmd::Autostart]
        );
        let notch = Config {
            style: Style::Notch,
            start_with_windows: false,
            ..Config::default()
        };
        assert_eq!(checked(&menu_items(&notch, None, &[])), vec![MenuCmd::StyleNotch]);
    }

    #[test]
    fn autostart_entry_follows_start_with_windows() {
        for lang in [Lang::Id, Lang::En] {
            let label = match lang {
                Lang::Id => "Jalan saat Windows mulai",
                Lang::En => "Start with Windows",
            };
            for (style, on) in [
                (Style::Pill, true),
                (Style::Pill, false),
                (Style::Notch, true),
                (Style::Notch, false),
            ] {
                let cfg = Config {
                    language: lang,
                    style,
                    start_with_windows: on,
                    ..Config::default()
                };
                let items = menu_items(&cfg, None, &[]);
                let found = items.iter().any(|item| {
                    matches!(item, MenuItem::Entry { cmd: MenuCmd::Autostart, label: l, checked }
                        if l == label && *checked == on)
                });
                assert!(found, "{lang:?} {style:?} {on}");
                // Independent of the style checks: exactly one style stays checked.
                let marks = checked(&items);
                assert_eq!(marks.contains(&MenuCmd::Autostart), on);
                assert_eq!(marks.contains(&MenuCmd::StylePill), style == Style::Pill);
                assert_eq!(marks.contains(&MenuCmd::StyleNotch), style == Style::Notch);
            }
        }
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
        let mut reachable = commands(&menu_items(&Config::default(), None, &[]));
        reachable.extend(commands(&menu_items(&Config::default(), Some("3.11.0"), &[])));
        for cmd in COMMANDS {
            assert!(reachable.contains(&cmd), "{cmd:?}");
        }
        // And Quit stays last, update known or not.
        for update in [None, Some("3.11.0")] {
            let items = commands(&menu_items(&Config::default(), update, &[]));
            assert_eq!(items.last(), Some(&MenuCmd::Exit));
        }
    }

    fn display(n: usize, primary: bool, path: &str, name: &str) -> Display {
        use crate::arrange::{Monitor, Rect};
        Display {
            monitor: Monitor {
                device: format!(r"\\.\DISPLAY{n}"),
                rect: Rect::new((n as i32 - 1) * 1920, 0, 1920, 1080),
                work: Rect::new((n as i32 - 1) * 1920, 0, 1920, 1040),
                dpi: 96,
            },
            hmonitor: n as isize,
            primary,
            path: path.into(),
            name: name.into(),
            number: n,
        }
    }

    fn two() -> Vec<Display> {
        vec![
            display(1, true, r"\\?\DISPLAY#AAA#1", "DELL U2415"),
            display(2, false, r"\\?\DISPLAY#BBB#2", "LG 27UL"),
        ]
    }

    fn submenu(items: &[MenuItem]) -> (&str, &[MonitorItem]) {
        items
            .iter()
            .find_map(|i| match i {
                MenuItem::Submenu { label, items } => Some((label.as_str(), items.as_slice())),
                _ => None,
            })
            .expect("the Monitor submenu")
    }

    fn chosen(path: &str, name: &str) -> NotchMonitor {
        NotchMonitor::Chosen {
            path: path.into(),
            name: name.into(),
        }
    }

    #[test]
    fn the_monitor_submenu_follows_pill_and_notch() {
        let items = menu_items(&Config::default(), None, &two());
        let at = items
            .iter()
            .position(|i| matches!(i, MenuItem::Submenu { .. }))
            .unwrap();
        assert!(matches!(
            &items[at - 1],
            MenuItem::Entry {
                cmd: MenuCmd::StyleNotch,
                ..
            }
        ));
        assert!(matches!(&items[at + 1], MenuItem::Separator));
        let (label, subs) = submenu(&items);
        assert_eq!(label, "Monitor");
        let labels: Vec<&str> = subs.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Monitor utama", "1 · DELL U2415", "2 · LG 27UL"]
        );
        let ids: Vec<usize> = subs.iter().map(|s| s.id).collect();
        assert_eq!(ids, [100, 101, 102]);
        // No id clashes with a plain command.
        for s in subs {
            assert!(MenuCmd::from_id(s.id).is_none());
        }
    }

    #[test]
    fn the_monitor_submenu_in_english() {
        let en = Config {
            language: Lang::En,
            notch_monitor: chosen(r"\\?\DISPLAY#GONE#9", "Samsung"),
            ..Config::default()
        };
        let items = menu_items(&en, None, &two());
        let (label, subs) = submenu(&items);
        assert_eq!(label, "Monitor");
        let labels: Vec<&str> = subs.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Primary monitor",
                "1 · DELL U2415",
                "2 · LG 27UL",
                "Samsung (not connected)"
            ]
        );
    }

    #[test]
    fn primary_is_checked_by_default() {
        let items = menu_items(&Config::default(), None, &two());
        let (_, subs) = submenu(&items);
        let on: Vec<usize> = subs.iter().filter(|s| s.checked).map(|s| s.id).collect();
        assert_eq!(on, [100]);
        assert!(subs.iter().all(|s| s.enabled));
    }

    #[test]
    fn a_connected_choice_is_the_one_checked() {
        let cfg = Config {
            notch_monitor: chosen(r"\\?\display#bbb#2", "LG 27UL"),
            ..Config::default()
        };
        let items = menu_items(&cfg, None, &two());
        let (_, subs) = submenu(&items);
        let on: Vec<usize> = subs.iter().filter(|s| s.checked).map(|s| s.id).collect();
        assert_eq!(on, [102]);
        assert_eq!(subs.len(), 3);
    }

    #[test]
    fn a_missing_choice_shows_greyed_and_checked() {
        let cfg = Config {
            notch_monitor: chosen(r"\\?\DISPLAY#GONE#9", "Samsung Odyssey"),
            ..Config::default()
        };
        let items = menu_items(&cfg, None, &two());
        let (_, subs) = submenu(&items);
        assert_eq!(subs.len(), 4);
        let last = subs.last().unwrap();
        assert!(last.checked && !last.enabled);
        assert_eq!(last.label, "Samsung Odyssey (tidak terhubung)");
        assert_eq!(subs.iter().filter(|s| s.checked).count(), 1);
        // A saved choice with no name still reads.
        let cfg = Config {
            notch_monitor: chosen(r"\\?\DISPLAY#GONE#9", ""),
            ..Config::default()
        };
        let items = menu_items(&cfg, None, &two());
        assert_eq!(
            submenu(&items).1.last().unwrap().label,
            "Monitor (tidak terhubung)"
        );
    }

    #[test]
    fn a_display_without_a_path_is_greyed() {
        let mut list = two();
        list[1].path.clear();
        let items = menu_items(&Config::default(), None, &list);
        let (_, subs) = submenu(&items);
        assert!(subs[0].enabled && subs[1].enabled && !subs[2].enabled);
        // Choosing it does nothing; a display with a path gives its choice.
        assert_eq!(monitor_choice(102, &list), None);
        assert_eq!(
            monitor_choice(101, &list),
            Some(chosen(r"\\?\DISPLAY#AAA#1", "DELL U2415"))
        );
    }

    #[test]
    fn monitor_ids_map_back_to_choices() {
        let list = two();
        assert_eq!(monitor_choice(100, &list), Some(NotchMonitor::Primary));
        assert_eq!(
            monitor_choice(102, &list),
            Some(chosen(r"\\?\DISPLAY#BBB#2", "LG 27UL"))
        );
        // Past the list (the greyed missing entry's id) and below the range.
        assert_eq!(monitor_choice(103, &list), None);
        assert_eq!(monitor_choice(99, &list), None);
        assert_eq!(monitor_choice(0, &list), None);
    }

    #[test]
    fn the_monitor_submenu_never_changes_the_plain_commands() {
        let with = commands(&menu_items(&Config::default(), None, &two()));
        let without = commands(&menu_items(&Config::default(), None, &[]));
        assert_eq!(with, without);
        // With no display listed there is still "Monitor utama".
        let items = menu_items(&Config::default(), None, &[]);
        assert_eq!(submenu(&items).1.len(), 1);
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
