//! What the Pintasan page's tiles do. The island collapses first, then the
//! action runs a moment later, once the window the user was in is in front
//! again: Win+. opens at its caret, and a screen clip must not catch the
//! panel. The Clipboard tile is the exception: it opens the island's own
//! Clipboard page (the reducer does that), not Win+V; so do the tiles that
//! jump to a page and the one that opens Vale's own Settings.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_LWIN, VK_OEM_PERIOD,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

use crate::i18n::Key;
use crate::model::{Page, Shortcut};
use crate::tiles::{CustomKind, CustomTile};
use crate::ui::geometry::ANIM_MS;
use crate::ui::icons::Icon;

/// How a shortcut is carried out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// `ShellExecute` "open" on a URI.
    Open(&'static str),
    /// `ShellExecute` "open" on a program, by its path under `%SystemRoot%`,
    /// so a same-named exe in the working directory is never picked.
    System(&'static str),
    /// The Windows key with this key.
    WinKey(VIRTUAL_KEY),
    /// `LockWorkStation`.
    Lock,
    /// Explorer on a drive's root ("C:\"), from the System page.
    Drive(char),
    /// The owner's own tile: `ShellExecute` "open" on its path or address.
    Custom(CustomTile),
    /// The quick search's result: `ShellExecute` "open" on a shortcut file,
    /// an `ms-settings:` page or an address the island built itself.
    Shell(String),
    /// A shelf card: `ShellExecute` "open" on a file or folder path, passed
    /// as it is (no lossy text conversion on the way).
    OpenPath(std::path::PathBuf),
    /// Vale's own region capture (Settings → Clipboard → "Alat screenshot"):
    /// the Screenshot tile with the Vale tool.
    Capture,
}

/// A drive's root as Explorer takes it: "C:\".
pub fn drive_root(letter: char) -> String {
    format!("{letter}:\\")
}

/// `None` for the Clipboard, Cari and page tiles: they open the island's own
/// Clipboard page, quick search and a page (the reducer does that).
pub fn shortcut_action(s: Shortcut) -> Option<Action> {
    Some(match s {
        Shortcut::Screenshot => Action::Open("ms-screenclip:"),
        Shortcut::Clipboard | Shortcut::Search => return None,
        Shortcut::Go(_) => return None,
        Shortcut::Emoji => Action::WinKey(VK_OEM_PERIOD),
        Shortcut::Explorer => Action::System("explorer.exe"),
        Shortcut::Calculator => Action::System(r"System32\calc.exe"),
        Shortcut::TaskManager => Action::System(r"System32\taskmgr.exe"),
        Shortcut::WindowsSettings => Action::Open("ms-settings:"),
        Shortcut::Lock => Action::Lock,
    })
}

/// The icon of a custom tile, by what it opens.
pub fn custom_icon(kind: CustomKind) -> Icon {
    match kind {
        CustomKind::App => Icon::Grid,
        CustomKind::Folder => Icon::Folder,
        CustomKind::Url => Icon::Globe,
    }
}

/// The tile's label.
pub fn label(s: Shortcut) -> Key {
    match s {
        Shortcut::Screenshot => Key::ShortcutScreenshot,
        Shortcut::Clipboard => Key::ShortcutClipboard,
        Shortcut::Search => Key::ShortcutSearch,
        Shortcut::Emoji => Key::ShortcutEmoji,
        Shortcut::Explorer => Key::ShortcutExplorer,
        Shortcut::Calculator => Key::ShortcutCalculator,
        Shortcut::TaskManager => Key::ShortcutTaskManager,
        Shortcut::WindowsSettings => Key::ShortcutSettings,
        Shortcut::Lock => Key::ShortcutLock,
        Shortcut::Go(page) => match page {
            Page::Calendar => Key::PageCalendar,
            Page::Pomodoro => Key::PagePomodoro,
            Page::Reminders => Key::PageReminders,
            Page::Notes => Key::PageNotes,
            Page::Media => Key::PageMedia,
            Page::Mixer => Key::PageMixer,
            // "Rak berkas" is too long under a tile's icon.
            Page::Shelf => Key::ShortcutGoShelf,
            Page::System => Key::PageSystem,
            Page::Devices => Key::PageDevices,
            Page::Cli => Key::PageCli,
            Page::Arrange => Key::PageArrange,
            Page::Clipboard => Key::PageClipboard,
            Page::Notifications => Key::PageNotifications,
            Page::Shortcuts => Key::PageShortcuts,
            Page::Search => Key::ShortcutSearch,
        },
    }
}

/// The icon of the tile that jumps to `page`.
pub(crate) fn page_icon(page: Page) -> Icon {
    match page {
        Page::Calendar => Icon::Calendar,
        Page::Pomodoro => Icon::Timer,
        Page::Reminders => Icon::Bell,
        Page::Notes => Icon::Note,
        Page::Media => Icon::Music,
        Page::Mixer => Icon::Volume,
        Page::Clipboard => Icon::Clipboard,
        Page::Notifications => Icon::Inbox,
        Page::Shelf => Icon::TempFile,
        Page::System => Icon::Chip,
        Page::Devices => Icon::Usb,
        Page::Cli => Icon::Terminal,
        Page::Arrange => Icon::Layout,
        Page::Shortcuts => Icon::Grid,
        Page::Search => Icon::Search,
    }
}

/// The tile's Studio icon.
pub fn icon(s: Shortcut) -> Icon {
    match s {
        Shortcut::Screenshot => Icon::Camera,
        Shortcut::Clipboard => Icon::Clipboard,
        Shortcut::Search => Icon::Search,
        Shortcut::Emoji => Icon::Emoji,
        Shortcut::Explorer => Icon::Folder,
        Shortcut::Calculator => Icon::Calc,
        Shortcut::TaskManager => Icon::Activity,
        Shortcut::WindowsSettings => Icon::Gear,
        Shortcut::Lock => Icon::Lock,
        Shortcut::Go(page) => page_icon(page),
    }
}

/// Milliseconds between the click and the action: long enough for the
/// foreground window to have the keyboard again; a screen clip waits for
/// the panel's collapse to finish so it is not in the picture.
pub fn delay_ms(action: &Action) -> u32 {
    match action {
        Action::Open("ms-screenclip:") | Action::Capture => ANIM_MS as u32 + 60,
        _ => 120,
    }
}

/// The `ShellExecute` verb for a custom tile: "explore" for a folder, so a
/// folder path is only ever shown in Explorer; "open" for the rest.
fn custom_verb(kind: CustomKind) -> PCWSTR {
    match kind {
        CustomKind::Folder => w!("explore"),
        CustomKind::App | CustomKind::Url => w!("open"),
    }
}

/// `rel` under `%SystemRoot%`.
fn system_path(rel: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    std::path::Path::new(&root).join(rel)
}

/// Carries `action` out. May pump messages (the shell), so the caller must
/// not hold the app state.
pub fn run(action: Action) {
    match action {
        Action::Open(target) => unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(target),
                None,
                None,
                SW_SHOWNORMAL,
            );
        },
        Action::System(rel) => unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(system_path(rel).as_os_str()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        },
        Action::Custom(tile) => unsafe {
            // A folder opens in Explorer ("explore": never as a program);
            // an address in the default browser or mail program; an app or
            // file as a double click would.
            ShellExecuteW(
                None,
                custom_verb(tile.kind),
                &HSTRING::from(tile.target.as_str()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        },
        Action::OpenPath(path) => unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(path.as_os_str()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        },
        Action::Shell(target) => unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(target.as_str()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        },
        Action::Drive(letter) => unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(system_path("explorer.exe").as_os_str()),
                &HSTRING::from(drive_root(letter)),
                None,
                SW_SHOWNORMAL,
            );
        },
        Action::WinKey(key) => {
            let input = |vk: VIRTUAL_KEY, up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: vk,
                        wScan: 0,
                        dwFlags: if up {
                            KEYEVENTF_KEYUP
                        } else {
                            KEYBD_EVENT_FLAGS(0)
                        },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            let inputs = [
                input(VK_LWIN, false),
                input(key, false),
                input(key, true),
                input(VK_LWIN, true),
            ];
            unsafe {
                SendInput(&inputs, size_of::<INPUT>() as i32);
            }
        }
        Action::Lock => unsafe {
            let _ = windows::Win32::System::Shutdown::LockWorkStation();
        },
        Action::Capture => crate::capture::overlay::start(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tile_maps_to_its_pinned_action() {
        let got: Vec<(Shortcut, Option<Action>)> = Shortcut::ALL
            .into_iter()
            .map(|s| (s, shortcut_action(s)))
            .collect();
        assert_eq!(
            got,
            [
                (Shortcut::Screenshot, Some(Action::Open("ms-screenclip:"))),
                // The island's own Clipboard page, not Win+V.
                (Shortcut::Clipboard, None),
                // The island's own quick search.
                (Shortcut::Search, None),
                (Shortcut::Emoji, Some(Action::WinKey(VK_OEM_PERIOD))),
                (Shortcut::Explorer, Some(Action::System("explorer.exe"))),
                (
                    Shortcut::Calculator,
                    Some(Action::System(r"System32\calc.exe"))
                ),
                (
                    Shortcut::TaskManager,
                    Some(Action::System(r"System32\taskmgr.exe"))
                ),
                (
                    Shortcut::WindowsSettings,
                    Some(Action::Open("ms-settings:"))
                ),
                (Shortcut::Lock, Some(Action::Lock)),
                // The page tiles: the reducer's.
                (Shortcut::Go(Page::Calendar), None),
                (Shortcut::Go(Page::Pomodoro), None),
                (Shortcut::Go(Page::Reminders), None),
                (Shortcut::Go(Page::Notes), None),
                (Shortcut::Go(Page::Media), None),
                (Shortcut::Go(Page::Mixer), None),
                (Shortcut::Go(Page::Notifications), None),
                (Shortcut::Go(Page::Shelf), None),
                (Shortcut::Go(Page::System), None),
                (Shortcut::Go(Page::Devices), None),
                (Shortcut::Go(Page::Cli), None),
                (Shortcut::Go(Page::Arrange), None),
            ]
        );
    }

    #[test]
    fn page_tiles_have_their_own_icon_and_label() {
        assert_eq!(icon(Shortcut::Go(Page::Calendar)), Icon::Calendar);
        assert_eq!(label(Shortcut::Go(Page::Shelf)), Key::ShortcutGoShelf);
        assert_eq!(icon(Shortcut::Go(Page::Devices)), Icon::Usb);
        assert_eq!(label(Shortcut::Go(Page::Devices)), Key::PageDevices);
        // The Notifikasi tile: the inbox tray, not the Reminders' bell.
        assert_eq!(icon(Shortcut::Go(Page::Notifications)), Icon::Inbox);
        assert_eq!(
            label(Shortcut::Go(Page::Notifications)),
            Key::PageNotifications
        );
        assert_ne!(
            icon(Shortcut::Go(Page::Notifications)),
            icon(Shortcut::Go(Page::Reminders))
        );
    }

    #[test]
    fn the_susun_tile_has_its_own_layout_icon_and_label() {
        assert_eq!(page_icon(Page::Arrange), Icon::Layout);
        assert_ne!(Icon::Layout, Icon::Grid, "not Pintasan's icon");
        assert_eq!(label(Shortcut::Go(Page::Arrange)), Key::PageArrange);
        assert_eq!(icon(Shortcut::Go(Page::Arrange)), Icon::Layout);
        assert_eq!(shortcut_action(Shortcut::Go(Page::Arrange)), None);
    }

    #[test]
    fn tiles_have_distinct_labels_and_icons() {
        for (i, a) in Shortcut::ALL.iter().enumerate() {
            for b in &Shortcut::ALL[i + 1..] {
                assert_ne!(label(*a), label(*b));
                assert_ne!(icon(*a), icon(*b));
            }
        }
    }

    #[test]
    fn vales_own_capture_waits_for_the_collapse_like_the_screen_clip() {
        let clip = shortcut_action(Shortcut::Screenshot).expect("the tile's action");
        assert_eq!(delay_ms(&Action::Capture), delay_ms(&clip));
        assert!(
            u64::from(delay_ms(&Action::Capture)) > ANIM_MS,
            "after the morph"
        );
    }

    #[test]
    fn a_search_result_waits_for_the_collapse_like_a_tile() {
        let a = Action::Shell("ms-settings:display".into());
        assert_eq!(delay_ms(&a), 120);
    }

    #[test]
    fn a_folder_tile_is_only_ever_explored() {
        let verb = |kind| unsafe { custom_verb(kind).to_string().unwrap() };
        assert_eq!(verb(CustomKind::Folder), "explore");
        assert_eq!(verb(CustomKind::App), "open");
        assert_eq!(verb(CustomKind::Url), "open");
        let shelf = Action::OpenPath(r"C:\x\denah.pdf".into());
        assert_eq!(delay_ms(&shelf), 120);
    }

    #[test]
    fn custom_tiles_get_an_icon_by_what_they_open() {
        let icons = CustomKind::ALL.map(custom_icon);
        assert_eq!(icons, [Icon::Grid, Icon::Folder, Icon::Globe]);
    }

    #[test]
    fn actions_wait_for_the_collapse() {
        let screenshot = shortcut_action(Shortcut::Screenshot).expect("an action");
        assert!(
            u64::from(delay_ms(&screenshot)) > ANIM_MS,
            "after the morph"
        );
        for s in Shortcut::ALL {
            let Some(action) = shortcut_action(s) else {
                continue;
            };
            let ms = delay_ms(&action);
            assert!((100..=400).contains(&ms), "{s:?}: {ms}");
        }
        assert_eq!(delay_ms(&Action::Drive('C')), 120);
        let own = CustomTile::new("Gmail", CustomKind::Url, "https://mail.google.com").unwrap();
        assert_eq!(delay_ms(&Action::Custom(own)), 120);
        assert_eq!(drive_root('D'), r"D:\");
    }
}
