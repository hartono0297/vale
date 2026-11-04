//! The settings window: a normal, single-instance window on the island's UI
//! thread, drawn with Direct2D (`settings_controls`) from a pure layout
//! (`settings_layout`). Only the two Google credential fields are native
//! EDIT controls.
//!
//! The window never changes settings itself: each click posts an `Action`
//! to its owner (the island) with `WM_APP_SETTINGS_ACTION`, and the owner
//! applies and saves it, then pushes the new `Model` back with `update`.
//! Controls animate when the model changes, so a change made elsewhere (the
//! tray menu's style items) animates the same way. Opening links, the data
//! folder and the Claude Code hook buttons need no island state and run
//! here.
//!
//! Nothing redraws while idle: hover and press repaint on change only, and
//! animations run a `FrameClock` only while they last.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Once;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
    DEFAULT_CHARSET, DeleteObject, EndPaint, EnumDisplayMonitors, FW_NORMAL, GetDC,
    GetMonitorInfoW, GetTextMetricsW, HBRUSH, HDC, HFONT, HGDIOBJ, HMONITOR, InvalidateRect,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
    MonitorFromRect, OUT_DEFAULT_PRECIS, PAINTSTRUCT, ReleaseDC, SelectObject, SetBkColor,
    SetTextColor, TEXTMETRICW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    EM_SETCUEBANNER, EM_SETLIMITTEXT, EM_SETSEL, SetWindowTheme, WM_MOUSELEAVE,
};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_A, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN, VK_SHIFT, VK_TAB,
    VK_UP,
};
use windows::Win32::UI::Shell::{
    DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, EN_CHANGE, EN_KILLFOCUS,
    EN_SETFOCUS, ES_AUTOHSCROLL, ES_PASSWORD, GetClientRect, GetDlgCtrlID, GetWindowPlacement,
    GetWindowTextLengthW, GetWindowTextW, HICON, HMENU, HTCLIENT, ICON_BIG, ICON_SMALL, IDC_ARROW,
    IDC_HAND, IDC_SIZEALL, IMAGE_ICON, IsIconic, IsWindow, KillTimer, LR_DEFAULTCOLOR, LoadCursorW,
    LoadImageW, MINMAXINFO, MoveWindow, PostMessageW, RegisterClassExW, SIZE_MAXIMIZED,
    SIZE_RESTORED, SM_CXICON, SM_CXSMICON, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED,
    SW_SHOWMINIMIZED, SW_SHOWNA, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW,
    SetCursor, SetForegroundWindow, SetTimer, SetWindowPos, SetWindowTextW, ShowWindow,
    WINDOW_EX_STYLE, WINDOW_STYLE, WINDOWPLACEMENT, WM_ACTIVATE, WM_APP, WM_CAPTURECHANGED,
    WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
    WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WM_SETFONT, WM_SETICON, WM_SIZE,
    WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW, WPF_RESTORETOMAXIMIZED, WS_CAPTION, WS_CHILD,
    WS_CLIPCHILDREN, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP,
    WS_THICKFRAME,
};
use windows::core::{Error, HSTRING, PCWSTR, Result, w};

use super::frame_clock::{FrameClock, WM_APP_FRAME};
use super::hotkey_rec::{RecHook, WM_APP_HOTKEY_REC, WM_APP_HOTKEY_REC_DONE};
use super::robot::{Motion, sleep_step};
use super::settings_controls::{Gfx, Scene, ease};
use super::settings_layout::{
    CLIENT_H, CLIENT_W, CanvasPress, CatFace, Control, DragView, EditView, Field, GUIDE_URL,
    GoogleState, Group, Hit, Layout, List, MonitorRow, REPO_URL, ROW_H, RejectMsg, ReorderRow,
    Section, SliderId, State, TileForm, Tone, UpdateRow, battery_step, canvas_drag, canvas_press,
    clip_items_step, disk_warn_step, drag_scroll_speed, drop_index, layout_in, monitor_choice,
    notif_items_step, ram_warn_step, scroll_by, shot_secs_step, slider_value,
};
use super::settings_window::{Rc, clamp_min, mostly_visible, workspace_to_screen};
use crate::arrange::{self, ArrangeSettings, CustomTemplate, Node, SavedArrangement};
use crate::calendar::CalStatus;
use crate::claude::install::{self, InstallOutcome, UninstallOutcome};
use crate::cli::Shell;
use crate::cli_folders::CliStart;
use crate::config::{
    AfterCapture, ClipboardSettings, Config, NotchFill, NotchMonitor, NotifSettings, OpenMode,
    ScreenshotSettings, ShotIgnore, ShotTool, Style, ToastStyle, UiFont, WindowPlace,
};
use crate::google::client;
use crate::hotkey::{Hotkey, HotkeyId, Hotkeys, Mods, Reject, Step, check, record_step, used_by};
use crate::i18n::{Key, Lang, text};
use crate::model::PomoField;
use crate::pages::PanelPages;
use crate::pomodoro::PomoSettings;
use crate::sources::battery::battery_present;
use crate::theme::{self, Accent, CatAccessory, CatColor, Surface, WindowPalette, WindowTheme};
use crate::tiles::{CustomKind, CustomProblem, CustomTile, LABEL_MAX, ShortcutTiles, TARGET_MAX};
use crate::widgets::prefs::{FullscreenRule, WidgetLayer, WidgetPrefs};
use crate::widgets::templates::Template;

/// Posted to the owner for each change: `lparam` is a `Box<Action>` raw
/// pointer the owner frees (`take_action`). WM_APP + 1..7 are taken.
pub const WM_APP_SETTINGS_ACTION: u32 = WM_APP + 8;
/// Enter in a credential field, posted by the field to the window.
const WM_APP_SUBMIT: u32 = WM_APP + 9;
/// The section fade ended: the credential fields may show.
const WM_APP_PLACE: u32 = WM_APP + 10;
/// From the own-shortcut form's fields: `wparam` 0 adds the tile (Enter), 1
/// closes the form (Esc), 2 gives its Nama field the keyboard.
const WM_APP_FORM: u32 = WM_APP + 12;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// The window's normal rectangle (screen coordinates) and whether it is
    /// maximized, after a move, a resize or on closing.
    WindowPlace(WindowPlace),
    Style(Style),
    Accent(Accent),
    Surface(Surface),
    /// "Font" (Tampilan → Island): DM Sans or JetBrains Mono.
    Font(UiFont),
    /// Settings → Tampilan → Notch. The sliders post `commit: false` while
    /// the thumb moves (the island follows live, nothing is saved) and
    /// `commit: true` once on release (saved).
    NotchSize {
        w: u32,
        h: u32,
        commit: bool,
    },
    NotchOpacity {
        pct: u8,
        commit: bool,
    },
    NotchFill(NotchFill),
    /// "Monitor": the screen the notch shows on (Tampilan → Notch).
    NotchMonitor(NotchMonitor),
    /// "Tandai": the island numbers every screen for two seconds.
    NotchIdentify,
    /// "Ukuran bawaan": back to 190 × 34.
    NotchReset,
    WindowMode(WindowTheme),
    Lang(Lang),
    Autostart(bool),
    IdleClock(bool),
    EqFollowAudio(bool),
    StayBehind(bool),
    Lyrics(bool),
    Shell(Shell),
    OpenMode(OpenMode),
    /// Settings -> Widget: the five settings, the gallery's (+) and a click
    /// on its active card.
    WidgetsShow(bool),
    WidgetsLayer(WidgetLayer),
    WidgetsFullscreen(FullscreenRule),
    WidgetsLock(bool),
    WidgetsPeek(bool),
    /// "Miring sedikit".
    WidgetsTilt(bool),
    /// "Isi dari clipboard".
    WidgetsClipFill(bool),
    /// The gallery card's (+): a note, from the template chosen in its menu.
    WidgetAdd(Template),
    WidgetHighlight,
    CliStart(CliStart),
    Robot(bool),
    RobotMotion(Motion),
    RobotSleep(u32),
    Greeting(bool),
    /// Settings → Kucing: the coat, the accessory, "Ikuti kursor" and
    /// "Mengecil jadi mata".
    CatColor(CatColor),
    CatAccessory(CatAccessory),
    CatFollow(bool),
    NotchPeek(bool),
    /// "Joget ikut musik".
    CatDance(bool),
    /// Settings → Clipboard.
    ClipEnabled(bool),
    ClipItems(u32),
    ClipPersist(bool),
    ClipClearOnLock(bool),
    /// Settings → Fitur → Perangkat: the plug-in and safe-to-remove notices.
    DeviceNotices(bool),
    /// Settings → Tombol pintas: a row's new combo (`None`: off), every row
    /// back to its default, and the recorder starting (`true`: the island
    /// lets go of its hotkeys) or ending (`false`: it takes them again). A
    /// `Hotkey` is always posted before the `HotkeyRecording(false)` that
    /// ends the recording, and that one waits until the recorder's hook has
    /// let go of every key it swallowed (`Resume`).
    Hotkey(HotkeyId, Option<Hotkey>),
    HotkeysResetAll,
    HotkeyRecording(bool),
    /// Settings -> Fitur -> Susun: the gap (`commit` false while
    /// the thumb moves), and the whole lists after an edit.
    ArrangeGap {
        gap: u32,
        commit: bool,
    },
    ArrangeTemplates(Vec<CustomTemplate>),
    ArrangeSaved(Vec<SavedArrangement>),
    ClipNotice(bool),
    ClipFeedbackAll(bool),
    /// "Hapus riwayat": every entry that is not pinned.
    ClipClear,
    /// Settings → Fitur → Notifikasi.
    NotifRecord(bool),
    NotifItems(u32),
    NotifPersist(bool),
    /// "Notifikasi saat notch dipakai".
    NotifToastStyle(ToastStyle),
    /// A name for the excluded apps, and one taken out of them (by name: the
    /// island's list may have changed by hand since the window drew it).
    NotifMutedAdd(String),
    NotifMutedRemove(String),
    /// "Hapus riwayat notifikasi".
    NotifClear,
    /// Settings → Clipboard → Screenshot.
    ShotTool(ShotTool),
    ShotAfterCapture(AfterCapture),
    ShotPreview(bool),
    ShotAutoCopy(bool),
    ShotOnIgnore(ShotIgnore),
    ShotSecs(u32),
    BatteryThreshold(u8),
    /// "Peringatan disk": GB free.
    DiskWarn(u32),
    /// "Peringatan RAM": percent used.
    RamWarn(u8),
    /// "Lindungi cache Cargo & Autodesk di Mole".
    MoleProtect(bool),
    /// "Halaman panel": the whole new order and on/off list.
    PanelPages(PanelPages),
    /// "Pintasan": the whole new tile list.
    ShortcutTiles(ShortcutTiles),
    DndHelp,
    Pomo(PomoField, i32),
    SaveGoogle {
        id: String,
        secret: String,
    },
    ConnectGoogle,
    DisconnectGoogle,
    /// Tentang → Pembaruan: "Cek sekarang".
    CheckUpdates,
    /// Tentang → Pembaruan: "Update", on a known newer version.
    InstallUpdate,
    /// "Cek pembaruan otomatis".
    UpdateAuto(bool),
    /// The window closed.
    Closed,
}

/// "Halaman panel": the pages after a click on a page's "Sembunyikan" /
/// "Tampilkan", its up or down button (among the pages shown) or "Kembalikan
/// bawaan", or `None` when it changes nothing (or is not such a click).
fn page_action(pages: &PanelPages, hit: Hit) -> Option<PanelPages> {
    let mut next = pages.clone();
    let changed = match hit {
        Hit::PageOn(p) => next.toggle(p),
        Hit::PageMove(p, d) => next.move_shown(p, d),
        Hit::PageReset => {
            next = PanelPages::default();
            next != *pages
        }
        _ => false,
    };
    changed.then_some(next)
}

/// "Pintasan": the tiles after a click on a tile's "Sembunyikan" / "+
/// Tambah", its up or down button (among the tiles shown), "Hapus" or
/// "Kembalikan bawaan", or `None` when it changes nothing (or is not such a
/// click).
fn tile_action(tiles: &ShortcutTiles, hit: Hit) -> Option<ShortcutTiles> {
    let mut next = tiles.clone();
    let changed = match hit {
        Hit::TileOn(i) => next.toggle(i),
        Hit::TileMove(i, d) => next.move_shown(i, d),
        Hit::TileRemove(i) => next.remove(i),
        Hit::TileReset => {
            next.reset();
            next != *tiles
        }
        _ => false,
    };
    changed.then_some(next)
}

/// What "Tambah" in the form does: the tiles with the typed one at the end
/// and whether it is on (it is not while twelve show already), or the message
/// that says what is wrong with what was typed.
fn add_tile_to(
    tiles: &ShortcutTiles,
    kind: CustomKind,
    name: &str,
    target: &str,
) -> std::result::Result<(ShortcutTiles, bool), Key> {
    let tile = CustomTile::check(name, kind, target).map_err(|problem| match problem {
        CustomProblem::Label => Key::TileErrName,
        CustomProblem::Target if kind == CustomKind::Url => Key::TileErrUrl,
        CustomProblem::Target => Key::TileErrPath,
    })?;
    let mut next = tiles.clone();
    let on = next.add_own(tile).ok_or(Key::TileOwnFull)?;
    Ok((next, on))
}

/// The pages after a drag dropped the page shown at place `from` on the
/// place `to` among the pages shown.
fn page_drop(pages: &PanelPages, from: usize, to: usize) -> Option<PanelPages> {
    let shown = pages.visible();
    let (page, target) = (*shown.get(from)?, *shown.get(to)?);
    let mut next = pages.clone();
    next.move_to(page, next.position(target)).then_some(next)
}

/// The tiles after a drag dropped the tile shown at place `from` on the
/// place `to` among the tiles shown.
fn tile_drop(tiles: &ShortcutTiles, from: usize, to: usize) -> Option<ShortcutTiles> {
    let on: Vec<usize> = (0..tiles.entries().len())
        .filter(|i| tiles.entries()[*i].1)
        .collect();
    let mut next = tiles.clone();
    next.move_to(*on.get(from)?, *on.get(to)?).then_some(next)
}

/// Takes ownership of a `WM_APP_SETTINGS_ACTION`'s `lparam`.
///
/// # Safety
/// `lparam` must come from a `WM_APP_SETTINGS_ACTION` message, taken once.
pub unsafe fn take_action(lparam: LPARAM) -> Option<Action> {
    let ptr = lparam.0 as *mut Action;
    (!ptr.is_null()).then(|| *unsafe { Box::from_raw(ptr) })
}

/// What the window shows, as the owner has it.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub style: Style,
    pub accent: Accent,
    pub surface: Surface,
    /// The UI typeface of the notch, the panel and this window.
    pub font: UiFont,
    /// The closed island's size and background.
    pub notch_width: u32,
    pub notch_height: u32,
    pub notch_fill: NotchFill,
    pub notch_opacity: u8,
    /// The screen the notch shows on, and the screens connected now (the
    /// island fills `monitors`; `from_config` leaves it empty).
    pub notch_monitor: NotchMonitor,
    pub monitors: Vec<MonitorRow>,
    pub window: WindowTheme,
    pub lang: Lang,
    pub autostart: bool,
    pub idle_clock: bool,
    pub eq_follow_audio: bool,
    pub stay_behind: bool,
    pub lyrics: bool,
    pub shell: Shell,
    pub cli_start: CliStart,
    pub open_mode: OpenMode,
    pub device_notices: bool,
    /// The six hotkeys, and by `HotkeyId::index` whether the one set could
    /// not be registered (another app has it, or the PrtSc hook failed).
    pub hotkeys: Hotkeys,
    pub hotkey_failed: [bool; HotkeyId::ALL.len()],
    /// The widgets' settings, and how many sticky notes are on the desktop.
    pub widgets: WidgetPrefs,
    pub sticky_count: usize,
    /// Susun's gap, templates and saved arrangements.
    pub arrange: ArrangeSettings,
    pub robot: bool,
    pub robot_motion: Motion,
    pub robot_sleep: u32,
    pub greeting: bool,
    pub cat_color: CatColor,
    pub cat_accessory: CatAccessory,
    pub cat_follow: bool,
    pub notch_peek: bool,
    pub cat_dance: bool,
    pub battery_threshold: u8,
    pub disk_warn_gb: u32,
    pub ram_warn_percent: u8,
    pub mole_protect: bool,
    pub pomodoro: PomoSettings,
    /// A pomodoro phase runs or is paused.
    pub pomodoro_locked: bool,
    pub calendar: CalStatus,
    pub google_client_id: String,
    pub google_client_secret: String,
    pub clip: ClipboardSettings,
    /// Settings → Fitur → Notifikasi and the excluded apps.
    pub notif: NotifSettings,
    pub muted: Vec<String>,
    pub shot: ScreenshotSettings,
    /// "Halaman panel".
    pub pages: PanelPages,
    /// "Pintasan".
    pub tiles: ShortcutTiles,
    /// Where the window was last. Only `open` reads it, for a new window;
    /// `update` ignores it.
    pub window_place: Option<WindowPlace>,
    /// "Cek pembaruan otomatis", and the Pembaruan row's state (the island's:
    /// a check running, an update known, a download).
    pub update_auto: bool,
    pub update_row: UpdateRow,
}

impl Model {
    pub fn from_config(cfg: &Config) -> Self {
        Model {
            style: cfg.style,
            accent: cfg.accent,
            surface: cfg.surface,
            font: cfg.font,
            notch_width: cfg.notch_width,
            notch_height: cfg.notch_height,
            notch_fill: cfg.notch_fill,
            notch_opacity: cfg.notch_opacity,
            notch_monitor: cfg.notch_monitor.clone(),
            monitors: Vec::new(),
            window: cfg.window_theme,
            lang: cfg.language,
            autostart: cfg.start_with_windows,
            idle_clock: cfg.idle_clock,
            eq_follow_audio: cfg.eq_follow_audio,
            stay_behind: cfg.stay_behind,
            lyrics: cfg.lyrics,
            shell: cfg.cli_shell,
            cli_start: cfg.cli_start,
            open_mode: cfg.open_mode,
            device_notices: cfg.device_notices,
            hotkeys: cfg.hotkeys,
            hotkey_failed: [false; HotkeyId::ALL.len()],
            widgets: cfg.widgets,
            sticky_count: 0,
            arrange: cfg.arrange.clone(),
            robot: cfg.robot,
            robot_motion: cfg.robot_animation,
            robot_sleep: cfg.robot_sleep_minutes,
            greeting: cfg.morning_greeting,
            cat_color: cfg.cat_color,
            cat_accessory: cfg.cat_accessory,
            cat_follow: cfg.cat_follow_pointer,
            notch_peek: cfg.notch_peek,
            cat_dance: cfg.cat_dance,
            battery_threshold: cfg.battery_threshold,
            disk_warn_gb: cfg.disk_warn_gb,
            ram_warn_percent: cfg.ram_warn_percent,
            mole_protect: cfg.mole_protect_defaults,
            pomodoro: cfg.pomodoro,
            pomodoro_locked: false,
            calendar: CalStatus::NotConfigured,
            google_client_id: cfg.google_client_id.clone(),
            google_client_secret: cfg.google_client_secret.clone(),
            clip: cfg.clipboard,
            notif: cfg.notifications,
            muted: cfg.muted_apps.clone(),
            shot: cfg.screenshot,
            pages: PanelPages::from_saved(&cfg.panel_pages),
            tiles: ShortcutTiles::from_saved(&cfg.shortcut_tiles),
            window_place: cfg.settings_window,
            update_auto: cfg.update_check,
            update_row: UpdateRow::Idle,
        }
    }
}

const ANIM: Duration = Duration::from_millis(120);
const TIMER_CONFIRM: usize = 1;
const TIMER_GOOGLE_NOTE: usize = 2;
const TIMER_HOOK_NOTE: usize = 3;
const TIMER_TILE_NOTE: usize = 4;
/// Scrolls the content while a row is dragged near the top or bottom edge;
/// runs only while it scrolls.
const TIMER_DRAG_SCROLL: usize = 5;
const DRAG_SCROLL_MS: u32 = 16;
/// A tick after a stall scrolls for at most this long.
const DRAG_SCROLL_MAX_DT: Duration = Duration::from_millis(50);
const CONFIRM_MS: u32 = 4000;
const NOTE_MS: u32 = 5000;
const ERROR_NOTE_MS: u32 = 10_000;
const ID_CLIENT_ID: i32 = 101;
const ID_CLIENT_SECRET: i32 = 102;
const ID_TILE_NAME: i32 = 103;
const ID_TILE_TARGET: i32 = 104;
const SUBCLASS_ID: usize = 1;
/// Upward offset the new section's content starts from.
const CONTENT_RISE: f32 = 6.0;

#[derive(Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    start: Instant,
}

impl Tween {
    /// From where `current` is now (or `rest`) to `to`.
    fn toward(current: Option<Tween>, rest: f32, to: f32, now: Instant) -> Tween {
        Tween {
            from: current.map_or(rest, |t| t.value(now)),
            to,
            start: now,
        }
    }

    fn progress(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.start).as_secs_f32() / ANIM.as_secs_f32()
    }

    fn value(&self, now: Instant) -> f32 {
        self.from + (self.to - self.from) * ease(self.progress(now))
    }

    fn running(&self, now: Instant) -> bool {
        self.progress(now) < 1.0
    }
}

#[derive(Default)]
struct Anims {
    segments: Vec<(Group, Tween)>,
    /// Toggle knobs sliding, by the toggle's hit.
    toggles: Vec<(Hit, Tween)>,
    nav: Option<Tween>,
    content: Option<Tween>,
}

impl Anims {
    fn running(&self, now: Instant) -> bool {
        let live = |t: &Option<Tween>| t.is_some_and(|t| t.running(now));
        live(&self.nav)
            || live(&self.content)
            || self.segments.iter().any(|(_, t)| t.running(now))
            || self.toggles.iter().any(|(_, t)| t.running(now))
    }

    fn segment(&self, group: Group) -> Option<Tween> {
        self.segments
            .iter()
            .find(|(g, _)| *g == group)
            .map(|(_, t)| *t)
    }

    fn set_segment(&mut self, group: Group, t: Tween) {
        self.segments.retain(|(g, _)| *g != group);
        self.segments.push((group, t));
    }

    /// Slides the knob of the toggle `hit` from `from` toward `on`.
    fn slide_toggle(&mut self, hit: Hit, from: bool, on: bool, now: Instant) {
        let running = self
            .toggles
            .iter()
            .find(|(h, _)| *h == hit)
            .map(|(_, t)| *t);
        let t = Tween::toward(
            running,
            f32::from(u8::from(from)),
            f32::from(u8::from(on)),
            now,
        );
        self.toggles.retain(|(h, _)| *h != hit);
        self.toggles.push((hit, t));
    }

    /// Drops finished tweens.
    fn prune(&mut self, now: Instant) {
        self.segments.retain(|(_, t)| t.running(now));
        self.toggles.retain(|(_, t)| t.running(now));
        for t in [&mut self.nav, &mut self.content] {
            if t.is_some_and(|t| !t.running(now)) {
                *t = None;
            }
        }
    }
}

/// The two credential fields and the own-shortcut form's two, and the GDI
/// objects they use.
struct Edits {
    id: HWND,
    secret: HWND,
    name: HWND,
    target: HWND,
    font: HFONT,
    /// Text line height of `font`, in physical pixels.
    line_h: i32,
    brush: HBRUSH,
    brush_rgb: u32,
}

impl Edits {
    fn hwnds(&self) -> [HWND; 4] {
        [self.id, self.secret, self.name, self.target]
    }

    /// The field Tab goes to from `edit`: the other of its pair.
    fn partner(&self, edit: HWND) -> HWND {
        if edit == self.id {
            self.secret
        } else if edit == self.secret {
            self.id
        } else if edit == self.name {
            self.target
        } else {
            self.name
        }
    }
}

impl Drop for Edits {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font.0));
            let _ = DeleteObject(HGDIOBJ(self.brush.0));
        }
    }
}

/// A row picked up by its drag handle.
struct Drag {
    list: List,
    from: usize,
    to: usize,
    /// How far under the row's top the pointer took it.
    grab: f32,
    /// The tops of the list's rows, in unscrolled layout pixels.
    tops: Vec<f32>,
    /// The row's top now, in the same pixels.
    top: f32,
    /// The pointer's last client y, in logical px.
    pointer_y: f32,
    /// Set while `TIMER_DRAG_SCROLL` runs: when the last tick (or the start)
    /// was.
    last_tick: Option<Instant>,
}

/// The held row's top and the slot it would drop at, for a pointer at client
/// `y` with the content scrolled by `scroll`: the row follows the pointer
/// (`grab` under its top) but stays between the first and last rows.
fn drag_place(tops: &[f32], grab: f32, y: f32, scroll: f32) -> (f32, usize) {
    let (Some(first), Some(last)) = (tops.first(), tops.last()) else {
        return (0.0, 0);
    };
    let top = (y + scroll - grab).clamp(*first, last.max(*first));
    (top, drop_index(top + ROW_H / 2.0, tops))
}

/// What the template editor is for.
enum EditTarget {
    New,
    Template(usize),
    /// A saved arrangement's name (no canvas).
    Rename(usize),
}

/// The template editor while it is open (Fitur -> Susun).
struct ArrangeEdit {
    target: EditTarget,
    root: Node,
    selected: usize,
    /// A divider in hand, by its path.
    drag: Option<Vec<bool>>,
}

struct Settings {
    hwnd: HWND,
    owner: HWND,
    dpi: u32,
    model: Model,
    section: Section,
    /// How far the section (when it is taller than the window) is scrolled.
    scroll: f32,
    /// The client area in logical px (never below the minimum, and kept
    /// from the last non-minimized size).
    client: (f32, f32),
    /// The place last posted to the owner (or the one the window opened at).
    last_place: Option<WindowPlace>,
    /// The window is maximized (as far as WM_SIZE told).
    maximized: bool,
    layout: Layout,
    gfx: Gfx,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    /// A slider's thumb is being dragged (the pointer is captured).
    slider: Option<SliderId>,
    /// `TME_LEAVE` is armed.
    tracking: bool,
    anims: Anims,
    clock: Option<FrameClock>,
    edits: Option<Edits>,
    /// What the credential fields and the form's fields hold now, by
    /// `Field::index`.
    field_text: [String; 4],
    focused: Option<Field>,
    has_battery: bool,
    /// The face the Kucing preview shows (the chips; not saved).
    cat_face: CatFace,
    hooks: Option<install::HookState>,
    google_note: Option<(String, Tone)>,
    hook_note: Option<(String, Tone)>,
    /// "Putuskan" waits for its second click.
    confirm: bool,
    /// "Lanjutan" under Google is open: the own Client ID fields show.
    google_advanced: bool,
    /// "Hapus riwayat" waits for its second click.
    clip_confirm: bool,
    /// "Hapus riwayat notifikasi" waits for its second click.
    notif_confirm: bool,
    /// The own-shortcut form is open (Pintasan), and the message after the
    /// last tile added.
    tile_form: Option<TileForm>,
    tile_note: Option<Key>,
    /// A row held by its drag handle.
    drag: Option<Drag>,
    /// The template editor (or the rename field) while it is open.
    arrange_edit: Option<ArrangeEdit>,
    /// The hotkey row being recorded, and why the last combo was refused
    /// (shown under that row until the next recording or a section change).
    recording: Option<HotkeyId>,
    hotkey_error: Option<(HotkeyId, RejectMsg)>,
    /// The recorder's keyboard hook. Kept after a combo until it has let go
    /// of the keys (it ends by itself), so `WM_DESTROY` can still stop it.
    rec_hook: Option<RecHook>,
    /// Whether a `HotkeyRecording(false)` waits for that hook to end.
    resume: Resume,
    /// `%APPDATA%\Vale`, or the old folder while a failed migration keeps it.
    data_label: String,
    /// Small and large window icons, owned (not shared) and destroyed with the window.
    icons: [HICON; 2],
    /// `(light, caption colour, caption text)` the title bar was last styled
    /// for; the fields' theme follows `light` alone.
    styled: Option<StyleKey>,
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

/// Runs `f` on the open window's state, unless a handler up the stack holds it.
fn with<R>(f: impl FnOnce(&mut Settings) -> R) -> Option<R> {
    SETTINGS.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        slot.as_mut().map(f)
    })
}

pub fn is_open() -> bool {
    SETTINGS.with(|cell| cell.try_borrow().map(|s| s.is_some()).unwrap_or(true))
}

fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))).0 }
}

fn window_text(hwnd: HWND) -> String {
    let len = usize::try_from(unsafe { GetWindowTextLengthW(hwnd) }).unwrap_or(0);
    let mut buf = vec![0u16; len + 1];
    let copied = usize::try_from(unsafe { GetWindowTextW(hwnd, &mut buf) }).unwrap_or(0);
    String::from_utf16_lossy(buf.get(..copied).unwrap_or(&[]))
}

/// `0xRRGGBB` as GDI's `0x00BBGGRR`.
const fn colorref(rgb: u32) -> COLORREF {
    COLORREF((rgb >> 16) & 0xFF | (rgb & 0xFF00) | (rgb & 0xFF) << 16)
}

fn post_action(owner: HWND, action: Action) {
    let ptr = Box::into_raw(Box::new(action));
    let posted = unsafe {
        PostMessageW(
            Some(owner),
            WM_APP_SETTINGS_ACTION,
            WPARAM(0),
            LPARAM(ptr as isize),
        )
    };
    if posted.is_err() {
        drop(unsafe { Box::from_raw(ptr) });
    }
}

/// `WM_ACTIVATE`'s `wparam` low word when the window loses the foreground.
const WA_INACTIVE: u32 = 0;

const CLASS_NAME: PCWSTR = w!("ValeSettings");

fn register_class(instance: HINSTANCE) -> Result<()> {
    static ONCE: Once = Once::new();
    let mut result = Ok(());
    ONCE.call_once(|| unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            let error = Error::from_thread();
            if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                result = Err(error);
            }
        }
    });
    result
}

/// `assets/icon.ico` at the small or large icon size for `dpi`. Not shared:
/// the window owns it, so the 16 px image is the real 16 px one.
fn load_icon(instance: HINSTANCE, dpi: u32, large: bool) -> HICON {
    let metric = if large { SM_CXICON } else { SM_CXSMICON };
    unsafe {
        let size = GetSystemMetricsForDpi(metric, dpi);
        LoadImageW(
            Some(instance),
            PCWSTR(1 as _),
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
        .map(|h| HICON(h.0))
        .unwrap_or_default()
    }
}

/// Sets the window's icons for `dpi` and destroys the ones they replace.
fn set_icons(hwnd: HWND, dpi: u32) {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return;
    };
    let instance: HINSTANCE = module.into();
    let icons = [
        load_icon(instance, dpi, false),
        load_icon(instance, dpi, true),
    ];
    send(hwnd, WM_SETICON, ICON_SMALL as usize, icons[0].0 as isize);
    send(hwnd, WM_SETICON, ICON_BIG as usize, icons[1].0 as isize);
    let old = with(|st| std::mem::replace(&mut st.icons, icons));
    match old {
        Some(old) => destroy_icons(old),
        // The window is not registered (yet): nobody else will free them.
        None => destroy_icons(icons),
    }
}

fn destroy_icons(icons: [HICON; 2]) {
    for icon in icons {
        if !icon.is_invalid() {
            unsafe {
                let _ = DestroyIcon(icon);
            }
        }
    }
}

fn create_font(dpi: u32, ui: UiFont) -> HFONT {
    let px = (13.0 * dpi as f32 / 96.0).round() as i32;
    unsafe {
        CreateFontW(
            -px,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            super::fonts::gdi_text_face_for(ui),
        )
    }
}

fn line_height(hwnd: HWND, font: HFONT) -> i32 {
    unsafe {
        let hdc = GetDC(Some(hwnd));
        let old = SelectObject(hdc, HGDIOBJ(font.0));
        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        SelectObject(hdc, old);
        ReleaseDC(Some(hwnd), hdc);
        tm.tmHeight.max(1)
    }
}

/// The primary monitor's DPI and work area.
fn primary_monitor() -> (u32, RECT) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dx, mut dy) = (96, 96);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        (dx, info.rcWork)
    }
}

const STYLE: WINDOW_STYLE = WINDOW_STYLE(
    WS_OVERLAPPED.0
        | WS_CAPTION.0
        | WS_SYSMENU.0
        | WS_MINIMIZEBOX.0
        | WS_MAXIMIZEBOX.0
        | WS_THICKFRAME.0
        | WS_CLIPCHILDREN.0,
);

/// The primary monitor's full rectangle and work area.
fn primary_rects() -> (Rc, Rc) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        (rc_of(info.rcMonitor), rc_of(info.rcWork))
    }
}

fn rc_of(r: RECT) -> Rc {
    Rc {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

/// Every monitor's full rectangle.
fn monitor_rects() -> Vec<Rc> {
    unsafe extern "system" fn each(
        _: HMONITOR,
        _: HDC,
        rect: *mut RECT,
        data: LPARAM,
    ) -> windows::core::BOOL {
        let list = unsafe { &mut *(data.0 as *mut Vec<Rc>) };
        if let Some(r) = unsafe { rect.as_ref() } {
            list.push(rc_of(*r));
        }
        true.into()
    }
    let mut list: Vec<Rc> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM((&raw mut list) as isize));
    }
    list
}

/// The window's normal rectangle (screen coordinates) and whether it is
/// maximized, as `GetWindowPlacement` has it.
fn window_place_of(hwnd: HWND) -> Option<WindowPlace> {
    let mut wp = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    unsafe { GetWindowPlacement(hwnd, &mut wp) }.ok()?;
    let maximized = wp.showCmd == SW_SHOWMAXIMIZED.0 as u32
        || (wp.showCmd == SW_SHOWMINIMIZED.0 as u32 && wp.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0);
    let (monitor, work) = primary_rects();
    let r = workspace_to_screen(rc_of(wp.rcNormalPosition), monitor, work);
    Some(WindowPlace {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
        maximized,
    })
}

/// The saved place to open at: its screen rectangle (grown to the minimum
/// outer size at the DPI of the monitor it is on) and whether to maximize,
/// or `None` when most of it is off every screen (a monitor was unplugged).
fn restored_place(place: Option<WindowPlace>) -> Option<(Rc, bool)> {
    let p = place?;
    let r = Rc {
        left: p.left,
        top: p.top,
        right: p.right,
        bottom: p.bottom,
    };
    if !mostly_visible(r, &monitor_rects()) {
        return None;
    }
    let rect = RECT {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    };
    let dpi = unsafe {
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96, 96);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        dx.max(96)
    };
    let (w, h) = outer_size(dpi);
    Some((clamp_min(r, w, h), p.maximized))
}

/// The client area of `hwnd` in logical px; the minimum size when it has
/// none (minimized).
fn client_logical(hwnd: HWND, dpi: u32) -> (f32, f32) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    let s = dpi.max(96) as f32 / 96.0;
    let (w, h) = (
        (rc.right - rc.left) as f32 / s,
        (rc.bottom - rc.top) as f32 / s,
    );
    if w > 0.0 && h > 0.0 {
        (w, h)
    } else {
        (CLIENT_W, CLIENT_H)
    }
}

/// Outer size for the logical client size at `dpi`: the smallest the window
/// can be (`WM_GETMINMAXINFO`) and what it opens at by default.
fn outer_size(dpi: u32) -> (i32, i32) {
    let s = dpi as f32 / 96.0;
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: (CLIENT_W * s).round() as i32,
        bottom: (CLIENT_H * s).round() as i32,
    };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut rc, STYLE, false, WINDOW_EX_STYLE(0), dpi);
    }
    (rc.right - rc.left, rc.bottom - rc.top)
}

/// Opens the window on `section` (Umum by default), or brings the open
/// one forward (restored if minimized) with the model brought up to date.
pub fn open(owner: HWND, model: Model, section: Option<Section>) -> Result<HWND> {
    let existing = SETTINGS.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|s| s.as_ref().map(|st| st.hwnd))
    });
    if let Some(hwnd) = existing {
        if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            update(model);
            if let Some(section) = section {
                set_section(hwnd, section);
            }
            unsafe {
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hwnd);
            }
            return Ok(hwnd);
        }
        SETTINGS.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut() {
                *slot = None;
            }
        });
    }

    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    register_class(instance)?;
    // A saved place that is still on a screen: the window is created
    // right there (so on the right monitor at the right DPI, with no
    // WM_DPICHANGED to rescale it) and shown maximized afterwards if it was.
    // The creation rectangle is the restore rectangle. Otherwise the default:
    // the minimum client size, centred on the primary work area.
    let (x, y, w, h, maximize) = match restored_place(model.window_place) {
        Some((r, maximized)) => (r.left, r.top, r.width(), r.height(), maximized),
        None => {
            let (dpi, work) = primary_monitor();
            let (w, h) = outer_size(dpi);
            let x = work.left + ((work.right - work.left) - w).max(0) / 2;
            let y = work.top + ((work.bottom - work.top) - h).max(0) / 2;
            (x, y, w, h, false)
        }
    };
    let title = text(model.lang, Key::SettingsTitle);
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS_NAME,
            &HSTRING::from(title),
            STYLE,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    // The window may have landed on a monitor with another DPI.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let client = client_logical(hwnd, dpi);
    let opened_at = window_place_of(hwnd).map(|p| WindowPlace {
        maximized: maximize,
        ..p
    });
    let gfx = match Gfx::new(model.font) {
        Ok(gfx) => gfx,
        Err(error) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(error);
        }
    };
    let edits = create_edits(hwnd, instance, dpi, &model);
    let field_text = [
        model.google_client_id.clone(),
        model.google_client_secret.clone(),
        String::new(),
        String::new(),
    ];
    // Someone with their own Client ID sees it straight away.
    let google_advanced = !field_text[0].trim().is_empty();
    let state = Settings {
        hwnd,
        owner,
        dpi,
        model,
        section: section.unwrap_or(Section::General),
        scroll: 0.0,
        client,
        last_place: opened_at,
        maximized: maximize,
        layout: Layout::default(),
        gfx,
        hover: None,
        pressed: None,
        slider: None,
        tracking: false,
        anims: Anims::default(),
        clock: None,
        edits,
        field_text,
        focused: None,
        has_battery: battery_present(),
        cat_face: CatFace::default(),
        hooks: install::hook_state(),
        google_note: None,
        hook_note: None,
        confirm: false,
        google_advanced,
        clip_confirm: false,
        notif_confirm: false,
        tile_form: None,
        tile_note: None,
        drag: None,
        arrange_edit: None,
        recording: None,
        hotkey_error: None,
        rec_hook: None,
        resume: Resume::default(),
        data_label: crate::data_dir::label(&crate::data_dir::dir()),
        icons: [HICON::default(); 2],
        styled: None,
    };
    let stored = SETTINGS.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => {
            *slot = Some(state);
            None
        }
        Err(_) => Some(state),
    });
    if stored.is_some() {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(Error::empty());
    }
    set_icons(hwnd, dpi);
    restyle();
    relayout_and_place();
    set_name_cue();
    unsafe {
        let _ = ShowWindow(hwnd, if maximize { SW_SHOWMAXIMIZED } else { SW_SHOW });
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(hwnd)
}

fn create_edits(parent: HWND, instance: HINSTANCE, dpi: u32, model: &Model) -> Option<Edits> {
    let font = create_font(dpi, model.font);
    let make = |id: i32, text: &str, extra: i32, cue: PCWSTR, limit: usize| -> Option<HWND> {
        let style = WS_CHILD | WS_TABSTOP | WINDOW_STYLE((ES_AUTOHSCROLL | extra) as u32);
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                &HSTRING::from(text),
                style,
                0,
                0,
                0,
                0,
                Some(parent),
                Some(HMENU(id as isize as *mut c_void)),
                Some(instance),
                None,
            )
            .ok()?
        };
        send(hwnd, WM_SETFONT, font.0 as usize, 0);
        send(hwnd, EM_SETCUEBANNER, 0, cue.0 as isize);
        if limit > 0 {
            send(hwnd, EM_SETLIMITTEXT, limit, 0);
        }
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID, 0);
        }
        Some(hwnd)
    };
    let id = make(
        ID_CLIENT_ID,
        &model.google_client_id,
        0,
        w!("…apps.googleusercontent.com"),
        0,
    );
    let secret = make(
        ID_CLIENT_SECRET,
        &model.google_client_secret,
        ES_PASSWORD,
        w!("GOCSPX-…"),
        0,
    );
    let name = make(ID_TILE_NAME, "", 0, w!("Gmail"), LABEL_MAX);
    let target = make(ID_TILE_TARGET, "", 0, cue_for(CustomKind::App), TARGET_MAX);
    match (id, secret, name, target) {
        (Some(id), Some(secret), Some(name), Some(target)) => Some(Edits {
            id,
            secret,
            name,
            target,
            font,
            line_h: line_height(parent, font),
            brush: unsafe { CreateSolidBrush(colorref(0)) },
            brush_rgb: 0,
        }),
        _ => {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
            None
        }
    }
}

/// The hint a Target field shows while it is empty, by what the tile opens.
fn cue_for(kind: CustomKind) -> PCWSTR {
    match kind {
        CustomKind::App => w!("C:\\Program Files\\…\\app.exe"),
        CustomKind::Folder => w!("D:\\Proyek"),
        CustomKind::Url => w!("https://…"),
    }
}

/// Brings the window up to `model`; animates what changed.
pub fn update(model: Model) {
    let changed = with(|st| {
        let mut model = model;
        // Only `open` uses the saved place.
        model.window_place = st.model.window_place;
        // The thumb in hand is where the pointer put it; an echo of an
        // earlier step must not pull it back.
        if st.slider.is_some() {
            model.notch_width = st.model.notch_width;
            model.notch_height = st.model.notch_height;
            model.notch_opacity = st.model.notch_opacity;
        }
        if st.model == model {
            return None;
        }
        Some(st.on_model(model))
    })
    .flatten();
    let Some(changes) = changed else {
        return;
    };
    if let Some((hwnd, lang)) = changes.title {
        unsafe {
            let _ = SetWindowTextW(hwnd, &HSTRING::from(text(lang, Key::SettingsTitle)));
        }
        set_name_cue();
    }
    for (edit, value) in changes.fields {
        unsafe {
            let _ = SetWindowTextW(edit, &HSTRING::from(value.as_str()));
        }
    }
    if let Some((hwnds, font, old)) = changes.edit_font {
        for edit in hwnds {
            send(edit, WM_SETFONT, font.0 as usize, 1);
        }
        unsafe {
            let _ = DeleteObject(HGDIOBJ(old.0));
        }
    }
    restyle();
    relayout_and_place();
}

/// What the title bar is styled from: `(light, caption colour, caption text)`.
type StyleKey = (bool, u32, u32);

/// `(title bar changed, light changed)` between the style last applied and
/// the palette `p`. The title bar follows the palette's bg and text, so it
/// also changes when the base colour moves between two dark palettes (warm to
/// Terminal's neutral); the fields' theme only when light flips.
fn style_changes(prev: Option<StyleKey>, p: &WindowPalette) -> (bool, bool) {
    let now = (p.light, p.bg, p.text);
    (
        prev != Some(now),
        prev.map(|(light, ..)| light) != Some(p.light),
    )
}

/// Restyles for the window palette in effect (Windows switched between
/// light and dark, or the window mode, accent or base colour changed).
pub fn restyle() {
    let Some((hwnd, edits, p, caption_changed, light_changed)) = with(|st| {
        let p = theme::current_window();
        let (caption_changed, changed) = style_changes(st.styled, &p);
        st.styled = Some((p.light, p.bg, p.text));
        if let Some(e) = &mut st.edits
            && e.brush_rgb != p.field
        {
            let old =
                std::mem::replace(&mut e.brush, unsafe { CreateSolidBrush(colorref(p.field)) });
            e.brush_rgb = p.field;
            unsafe {
                let _ = DeleteObject(HGDIOBJ(old.0));
            }
        }
        let edits = st.edits.as_ref().map(Edits::hwnds);
        (st.hwnd, edits, p, caption_changed, changed)
    }) else {
        return;
    };
    if caption_changed {
        apply_mode(hwnd, &p);
    }
    if light_changed {
        for edit in edits.into_iter().flatten() {
            let theme = if p.light {
                w!("Explorer")
            } else {
                w!("DarkMode_Explorer")
            };
            unsafe {
                let _ = SetWindowTheme(edit, theme, None);
            }
        }
    }
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
        for edit in edits.into_iter().flatten() {
            let _ = InvalidateRect(Some(edit), None, true);
        }
    }
}

/// Dark or light title bar, coloured like the window so it reads as one surface.
fn apply_mode(hwnd: HWND, p: &WindowPalette) {
    let dark = i32::from(!p.light);
    let caption = colorref(p.bg).0;
    let text = colorref(p.text).0;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        );
        // Windows 11 only; older systems keep the standard caption.
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &caption as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            &text as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
    }
}

/// Closes the window (island shutdown).
pub fn close() {
    let hwnd = SETTINGS.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|s| s.as_ref().map(|st| st.hwnd))
    });
    if let Some(hwnd) = hwnd {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

/// The island is ending: closes the window and drops what is left in its
/// slot (the Direct2D objects) now, not in the thread-local destructors during
/// process exit.
pub fn release() {
    close();
    let left = SETTINGS.with(|cell| cell.try_borrow_mut().ok().and_then(|mut s| s.take()));
    if let Some(st) = left {
        destroy_icons(st.icons);
        drop(st);
    }
}

/// What `on_model` leaves to do once the state is released.
struct ModelChanges {
    title: Option<(HWND, Lang)>,
    fields: Vec<(HWND, String)>,
    /// The typeface changed: the fields, their new GDI font and the old one
    /// to delete once they use the new.
    edit_font: Option<(Vec<HWND>, HFONT, HFONT)>,
}

/// Where each credential field goes: hidden, or at a client rect.
type Placement = (HWND, Option<RECT>);

/// Recomputes the layout and moves the native fields to match.
fn relayout_and_place() {
    let Some(placements) = with(|st| {
        st.relayout();
        st.placements()
    }) else {
        return;
    };
    place(placements);
}

fn place(placements: Vec<Placement>) {
    for (edit, rect) in placements {
        unsafe {
            match rect {
                Some(rc) => {
                    let _ = MoveWindow(
                        edit,
                        rc.left,
                        rc.top,
                        rc.right - rc.left,
                        rc.bottom - rc.top,
                        true,
                    );
                    let _ = ShowWindow(edit, SW_SHOWNA);
                }
                None => {
                    let _ = ShowWindow(edit, SW_HIDE);
                }
            }
        }
    }
}

impl Settings {
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    fn state(&self) -> State<'_> {
        let m = &self.model;
        let id = self.field_text[0].trim();
        let secret = self.field_text[1].trim();
        State {
            lang: m.lang,
            style: m.style,
            accent: m.accent,
            surface: m.surface,
            font: m.font,
            notch_width: m.notch_width,
            notch_height: m.notch_height,
            notch_fill: m.notch_fill,
            notch_opacity: m.notch_opacity,
            notch_monitor: m.notch_monitor.clone(),
            monitors: &m.monitors,
            window: m.window,
            autostart: m.autostart,
            idle_clock: m.idle_clock,
            eq_follow_audio: m.eq_follow_audio,
            stay_behind: m.stay_behind,
            robot: m.robot,
            robot_motion: m.robot_motion,
            robot_sleep: m.robot_sleep,
            greeting: m.greeting,
            cat_color: m.cat_color,
            cat_accessory: m.cat_accessory,
            cat_follow: m.cat_follow,
            notch_peek: m.notch_peek,
            cat_dance: m.cat_dance,
            cat_face: self.cat_face,
            scroll: self.scroll,
            lyrics: m.lyrics,
            shell: m.shell,
            cli_start: m.cli_start,
            open_mode: m.open_mode,
            device_notices: m.device_notices,
            hotkeys: m.hotkeys,
            hotkey_failed: m.hotkey_failed,
            widgets: m.widgets,
            sticky_count: m.sticky_count,
            hotkey_recording: self.recording,
            hotkey_error: self.hotkey_error,
            arrange: m.arrange.clone(),
            arrange_edit: self.arrange_edit.as_ref().map(|e| EditView {
                rename: matches!(e.target, EditTarget::Rename(_)),
                root: e.root.clone(),
                selected: e.selected,
                name_ok: arrange::clean_name(&self.field_text[2]).is_some(),
            }),
            battery_threshold: m.battery_threshold,
            has_battery: self.has_battery,
            disk_warn_gb: m.disk_warn_gb,
            ram_warn_percent: m.ram_warn_percent,
            mole_protect: m.mole_protect,
            pomodoro: m.pomodoro,
            pomodoro_locked: m.pomodoro_locked,
            google: GoogleState {
                status: m.calendar,
                dirty: id != m.google_client_id.trim() || secret != m.google_client_secret.trim(),
                has_id: !id.is_empty(),
                confirm: self.confirm,
                built_in: client::has_built_in(),
                advanced: self.google_advanced,
            },
            google_note: self.google_note.as_ref().map(|(s, t)| (s.as_str(), *t)),
            clip_confirm: self.clip_confirm,
            notif: m.notif,
            muted: m.muted.clone(),
            notif_confirm: self.notif_confirm,
            hooks: self.hooks,
            hook_note: self.hook_note.as_ref().map(|(s, t)| (s.as_str(), *t)),
            clip: m.clip,
            shot: m.shot,
            pages: m.pages.clone(),
            tiles: m.tiles.clone(),
            drag: self.drag.as_ref().map(|d| DragView {
                list: d.list,
                from: d.from,
                to: d.to,
                top: d.top,
            }),
            tile_form: self.tile_form,
            tile_note: self.tile_note,
            version: env!("CARGO_PKG_VERSION"),
            update_auto: m.update_auto,
            update_row: m.update_row.clone(),
            data_dir: &self.data_label,
        }
    }

    fn relayout(&mut self) {
        let l = layout_in(self.section, &self.state(), &self.gfx, self.client);
        // The layout keeps the scroll in range (a language change can move it).
        self.scroll = l.scroll;
        self.layout = l;
        self.invalidate();
    }

    /// The wheel over a section taller than the window scrolls it.
    fn on_wheel(&mut self, delta: i32) {
        let scroll = scroll_by(self.scroll, delta, self.layout.scroll_max);
        if scroll != self.scroll {
            self.scroll = scroll;
            // What was under the pointer moved away.
            self.hover = None;
            // A row in hand stays under the pointer: it takes the place the
            // pointer now is over.
            self.drag_follow();
            self.relayout();
            self.sync_drag_timer();
        }
    }

    /// The window's size in logical px, read again (after a DPI change the
    /// pixel size may be the same while the logical one is not).
    fn read_client(&mut self) {
        self.client = client_logical(self.hwnd, self.dpi);
    }

    /// Posts the window's place to the owner when it changed.
    fn post_place(&mut self) {
        let Some(p) = window_place_of(self.hwnd) else {
            return;
        };
        if self.last_place != Some(p) {
            self.last_place = Some(p);
            post_action(self.owner, Action::WindowPlace(p));
        }
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// The fields show on Integrations once its content has faded in.
    fn placements(&self) -> Vec<Placement> {
        let Some(e) = &self.edits else {
            return Vec::new();
        };
        let fading = self
            .anims
            .content
            .is_some_and(|t| t.running(Instant::now()));
        let s = self.scale();
        let view = self.layout.viewport;
        [
            (Field::ClientId, e.id),
            (Field::ClientSecret, e.secret),
            (Field::TileName, e.name),
            (Field::TileTarget, e.target),
        ]
        .into_iter()
        .map(|(field, edit)| {
            let rect = self
                .layout
                .fields
                .iter()
                .find(|(f, _)| *f == field)
                // A field scrolled partly under the title or past the
                // bottom hides: a native control is not clipped.
                .filter(|(_, r)| view.is_none_or(|v| r.y >= v.y && r.bottom() <= v.bottom()))
                .filter(|_| !fading)
                .map(|(_, r)| {
                    let px = |v: f32| (v * s).round() as i32;
                    let left = px(r.x + 10.0);
                    let right = px(r.right() - 10.0);
                    let mid = px(r.y + r.h / 2.0);
                    RECT {
                        left,
                        top: mid - e.line_h / 2,
                        right,
                        bottom: mid - e.line_h / 2 + e.line_h,
                    }
                });
            (edit, rect)
        })
        .collect()
    }

    fn on_model(&mut self, model: Model) -> ModelChanges {
        let now = Instant::now();
        let old = std::mem::replace(&mut self.model, model);
        let new = &self.model;
        let index = |group: Group, m: &Model| -> f32 {
            match group {
                Group::Style => usize::from(m.style == Style::Notch) as f32,
                // The chips are window state: the layout draws the choice.
                Group::CatFace => 0.0,
                Group::Window => WindowTheme::ALL
                    .iter()
                    .position(|w| *w == m.window)
                    .unwrap_or(0) as f32,
                Group::Lang => usize::from(m.lang == Lang::En) as f32,
                Group::Shell => usize::from(m.shell == Shell::Cmd) as f32,
                Group::CliStart => usize::from(m.cli_start == CliStart::Last) as f32,
                Group::OpenMode => OpenMode::ALL
                    .iter()
                    .position(|v| *v == m.open_mode)
                    .unwrap_or(0) as f32,
                Group::Motion => Motion::ALL
                    .iter()
                    .position(|v| *v == m.robot_motion)
                    .unwrap_or(0) as f32,
                Group::ShotIgnore => usize::from(m.shot.on_ignore == ShotIgnore::Delete) as f32,
                // The form's own choice, not the model's.
                Group::TileKind => 0.0,
                Group::ShotTool => usize::from(m.shot.tool == ShotTool::Windows) as f32,
                Group::ShotAfterCapture => {
                    usize::from(m.shot.after_capture == AfterCapture::Inline) as f32
                }
                Group::NotifToastStyle => {
                    usize::from(m.notif.toast_style == ToastStyle::Full) as f32
                }
                Group::NotchFill => usize::from(m.notch_fill == NotchFill::Transparent) as f32,
                Group::Font => UiFont::ALL.iter().position(|v| *v == m.font).unwrap_or(0) as f32,
                Group::WidgetLayer => usize::from(m.widgets.layer == WidgetLayer::Top) as f32,
                Group::WidgetFullscreen => {
                    usize::from(m.widgets.fullscreen == FullscreenRule::Keep) as f32
                }
            }
        };
        for group in [
            Group::Style,
            Group::NotchFill,
            Group::Font,
            Group::Window,
            Group::Lang,
            Group::OpenMode,
            Group::CliStart,
            Group::Shell,
            Group::Motion,
            Group::ShotIgnore,
            Group::ShotTool,
            Group::ShotAfterCapture,
            Group::NotifToastStyle,
            Group::WidgetLayer,
            Group::WidgetFullscreen,
        ] {
            let (from, to) = (index(group, &old), index(group, new));
            if from != to {
                let t = Tween::toward(self.anims.segment(group), from, to, now);
                self.anims.set_segment(group, t);
            }
        }
        if old.autostart != new.autostart {
            self.anims
                .slide_toggle(Hit::Autostart, old.autostart, new.autostart, now);
        }
        if old.idle_clock != new.idle_clock {
            self.anims
                .slide_toggle(Hit::IdleClock, old.idle_clock, new.idle_clock, now);
        }
        if old.eq_follow_audio != new.eq_follow_audio {
            self.anims.slide_toggle(
                Hit::EqFollowAudio,
                old.eq_follow_audio,
                new.eq_follow_audio,
                now,
            );
        }
        if old.stay_behind != new.stay_behind {
            self.anims
                .slide_toggle(Hit::StayBehind, old.stay_behind, new.stay_behind, now);
        }
        for (hit, was, is) in [
            (Hit::WidgetsShow, old.widgets.show, new.widgets.show),
            (Hit::WidgetsLock, old.widgets.lock, new.widgets.lock),
            (Hit::WidgetsPeek, old.widgets.cat_peek, new.widgets.cat_peek),
            (Hit::WidgetsTilt, old.widgets.tilt, new.widgets.tilt),
            (Hit::WidgetsClipFill, old.widgets.clip_fill, new.widgets.clip_fill),
        ] {
            if was != is {
                self.anims.slide_toggle(hit, was, is, now);
            }
        }
        if old.update_auto != new.update_auto {
            self.anims
                .slide_toggle(Hit::UpdateAuto, old.update_auto, new.update_auto, now);
        }
        if old.lyrics != new.lyrics {
            self.anims
                .slide_toggle(Hit::Lyrics, old.lyrics, new.lyrics, now);
        }
        if old.robot != new.robot {
            self.anims
                .slide_toggle(Hit::Robot, old.robot, new.robot, now);
        }
        if old.cat_follow != new.cat_follow {
            self.anims
                .slide_toggle(Hit::CatFollow, old.cat_follow, new.cat_follow, now);
        }
        if old.notch_peek != new.notch_peek {
            self.anims
                .slide_toggle(Hit::NotchPeek, old.notch_peek, new.notch_peek, now);
        }
        if old.cat_dance != new.cat_dance {
            self.anims
                .slide_toggle(Hit::CatDance, old.cat_dance, new.cat_dance, now);
        }
        if old.greeting != new.greeting {
            self.anims
                .slide_toggle(Hit::Greeting, old.greeting, new.greeting, now);
        }
        if old.mole_protect != new.mole_protect {
            self.anims
                .slide_toggle(Hit::MoleProtect, old.mole_protect, new.mole_protect, now);
        }
        let (oc, nc) = (old.clip, new.clip);
        for (hit, from, to) in [
            (Hit::ClipEnabled, oc.enabled, nc.enabled),
            (Hit::ClipPersist, oc.persist, nc.persist),
            (Hit::ClipClearOnLock, oc.clear_on_lock, nc.clear_on_lock),
            (Hit::ClipNotice, oc.copy_notice, nc.copy_notice),
            (
                Hit::ClipFeedbackAll,
                oc.copied_feedback_all,
                nc.copied_feedback_all,
            ),
            (Hit::NotifRecord, old.notif.record, new.notif.record),
            (Hit::NotifPersist, old.notif.persist, new.notif.persist),
            (Hit::ShotPreview, old.shot.preview, new.shot.preview),
            (Hit::ShotAutoCopy, old.shot.auto_copy, new.shot.auto_copy),
        ] {
            if from != to {
                self.anims.slide_toggle(hit, from, to, now);
            }
        }
        let mut changes = ModelChanges {
            title: None,
            fields: Vec::new(),
            edit_font: None,
        };
        if old.font != new.font {
            // Measure and draw in the new typeface (the width cache is
            // dropped with the formats), and give the native fields the new
            // GDI face.
            if let Err(error) = self.gfx.set_font(new.font) {
                eprintln!("vale: settings font change failed: {error}");
            }
            if let Some(e) = &mut self.edits {
                let font = create_font(self.dpi, new.font);
                let old_font = std::mem::replace(&mut e.font, font);
                e.line_h = line_height(self.hwnd, font);
                changes.edit_font = Some((e.hwnds().to_vec(), font, old_font));
            }
        }
        if old.lang != new.lang {
            changes.title = Some((self.hwnd, new.lang));
            // Messages in the old language would linger; they are short-lived anyway.
            self.google_note = None;
            self.hook_note = None;
        }
        // A credential changed elsewhere (config.json edited by hand and picked
        // up): refresh a field the user has not touched.
        if let Some(e) = &self.edits {
            let pairs = [
                (0, e.id, &old.google_client_id, &new.google_client_id),
                (
                    1,
                    e.secret,
                    &old.google_client_secret,
                    &new.google_client_secret,
                ),
            ];
            for (i, edit, before, after) in pairs {
                if before != after && self.field_text[i] == *before {
                    self.field_text[i] = after.clone();
                    changes.fields.push((edit, after.clone()));
                }
            }
        }
        self.start_clock();
        changes
    }

    fn start_clock(&mut self) {
        if self.clock.is_none() && self.anims.running(Instant::now()) {
            self.clock = Some(FrameClock::start(self.hwnd.0 as isize));
        }
    }

    fn render(&mut self) {
        let now = Instant::now();
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let size = (
            (rc.right - rc.left).max(1) as u32,
            (rc.bottom - rc.top).max(1) as u32,
        );
        let content = self.anims.content.map_or(1.0, |t| t.value(now));
        let p = theme::current_window();
        let scene = Scene {
            layout: &self.layout,
            p,
            island: theme::island(
                self.model.accent,
                self.model
                    .surface
                    .resolve(self.model.cat_color, self.model.robot),
            ),
            style: self.model.style,
            notch: super::geometry::NotchSize::from_px(
                self.model.notch_width,
                self.model.notch_height,
            ),
            notch_alpha: super::notch_fill::fill_alpha(
                self.model.notch_fill,
                self.model.notch_opacity,
            ),
            robot: self.model.robot,
            cat: super::cat_style::CatLook {
                color: self.model.cat_color,
                accessory: self.model.cat_accessory,
            },
            cat_face: self.cat_face,
            lang: self.model.lang,
            hover: self.hover,
            pressed: self.pressed,
            nav_pos: self
                .anims
                .nav
                .map_or(self.section.index() as f32, |t| t.value(now)),
            section: self.section,
            content_alpha: content,
            content_dy: (1.0 - content) * CONTENT_RISE,
            segments: self
                .anims
                .segments
                .iter()
                .map(|(g, t)| (*g, t.value(now)))
                .collect(),
            toggles: self
                .anims
                .toggles
                .iter()
                .map(|(h, t)| (*h, t.value(now)))
                .collect(),
            focused: self.focused,
        };
        let dpi = self.dpi as f32;
        if let Err(error) = self.gfx.paint(self.hwnd, size, dpi, &scene) {
            eprintln!("vale: settings draw failed: {error}");
        }
        let content_was_running = self.anims.content.is_some();
        self.anims.prune(now);
        if !self.anims.running(now) {
            // Dropping joins the frame thread, which wakes within one frame.
            self.clock = None;
        }
        if content_was_running && self.anims.content.is_none() {
            // The fade ended: the native fields may show now.
            unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_APP_PLACE, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// A mouse message's position, in logical pixels.
    fn point(&self, lparam: LPARAM) -> (f32, f32) {
        let x = (lparam.0 & 0xFFFF) as i16 as f32 / self.scale();
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / self.scale();
        (x, y)
    }

    fn hit_at(&self, lparam: LPARAM) -> Option<Hit> {
        let (x, y) = self.point(lparam);
        self.layout.hit(x, y)
    }

    /// Where slider `id`'s track is, as laid out now.
    fn slider_track(&self, id: SliderId) -> Option<super::settings_layout::Rect> {
        self.layout.controls.iter().find_map(|c| match c {
            Control::Slider { id: i, track, .. } if *i == id => Some(*track),
            _ => None,
        })
    }

    /// The value slider `id` has in the model.
    fn slider_current(&self, id: SliderId) -> i32 {
        match id {
            SliderId::NotchWidth => self.model.notch_width as i32,
            SliderId::NotchHeight => self.model.notch_height as i32,
            SliderId::NotchOpacity => i32::from(self.model.notch_opacity),
            SliderId::ArrangeGap => self.model.arrange.gap as i32,
        }
    }

    /// The action for slider `id`'s value as the model has it now.
    fn slider_action(&self, id: SliderId, commit: bool) -> Action {
        match id {
            SliderId::NotchWidth | SliderId::NotchHeight => Action::NotchSize {
                w: self.model.notch_width,
                h: self.model.notch_height,
                commit,
            },
            SliderId::NotchOpacity => Action::NotchOpacity {
                pct: self.model.notch_opacity,
                commit,
            },
            SliderId::ArrangeGap => Action::ArrangeGap {
                gap: self.model.arrange.gap,
                commit,
            },
        }
    }

    /// The thumb of slider `id` follows the pointer at `x`: the model and
    /// the layout take the value at once (the thumb never waits for the
    /// island). `Some` only when the whole number changed, so a drag posts
    /// one live action per step.
    fn slider_move(&mut self, id: SliderId, x: f32) -> Option<Action> {
        let track = self.slider_track(id)?;
        let (min, max) = id.range();
        let value = slider_value(track, x, min, max);
        if value == self.slider_current(id) {
            return None;
        }
        match id {
            SliderId::NotchWidth => self.model.notch_width = value as u32,
            SliderId::NotchHeight => self.model.notch_height = value as u32,
            SliderId::NotchOpacity => self.model.notch_opacity = value as u8,
            SliderId::ArrangeGap => self.model.arrange.gap = value as u32,
        }
        self.relayout();
        Some(self.slider_action(id, false))
    }

    /// The pointer went down on the drag handle `hit` at `y`: its row is in
    /// hand.
    fn begin_drag(&mut self, hit: Hit, y: f32) {
        let list = match hit {
            Hit::PageGrip(_) => List::Pages,
            Hit::TileGrip(_) => List::Tiles,
            _ => return,
        };
        let rows: Vec<&ReorderRow> = self.layout.rows.iter().filter(|r| r.list == list).collect();
        let Some(from) = rows.iter().position(|r| r.grip == hit) else {
            return;
        };
        let tops: Vec<f32> = rows.iter().map(|r| r.rect.y + self.scroll).collect();
        let grab = y + self.scroll - tops[from];
        self.drag = Some(Drag {
            list,
            from,
            to: from,
            grab,
            top: tops[from],
            tops,
            pointer_y: y,
            last_tick: None,
        });
        self.hover = None;
    }

    /// The pointer moved to `y` with a row in hand: it follows, and the
    /// others make room.
    fn drag_to(&mut self, y: f32) {
        let Some(d) = &mut self.drag else {
            return;
        };
        d.pointer_y = y;
        self.drag_follow();
        self.relayout();
        self.sync_drag_timer();
    }

    /// The row in hand takes its place for the pointer's last y and the
    /// scroll now.
    fn drag_follow(&mut self) {
        let scroll = self.scroll;
        if let Some(d) = &mut self.drag {
            (d.top, d.to) = drag_place(&d.tops, d.grab, d.pointer_y, scroll);
        }
    }

    /// How fast the content should scroll for the row in hand, px a second
    /// (negative up): zero unless the pointer is near the viewport's edge,
    /// the content can scroll that way and the section scrolls at all.
    fn drag_scroll_now(&self) -> f32 {
        let (Some(d), Some(v)) = (&self.drag, self.layout.viewport) else {
            return 0.0;
        };
        let speed = drag_scroll_speed(d.pointer_y, v.y, v.bottom());
        let room = if speed < 0.0 {
            self.scroll > 0.0
        } else {
            self.scroll < self.layout.scroll_max
        };
        if room { speed } else { 0.0 }
    }

    /// `TIMER_DRAG_SCROLL` runs exactly while the row in hand scrolls the
    /// content: it starts when the pointer is in a scroll zone with room to
    /// scroll, and stops when it leaves the zone or the content reaches its
    /// end. It is set once per run (setting it again would restart its
    /// interval on every pointer move).
    fn sync_drag_timer(&mut self) {
        let wanted = self.drag_scroll_now() != 0.0;
        let hwnd = self.hwnd;
        let Some(d) = &mut self.drag else {
            return;
        };
        match (wanted, d.last_tick.is_some()) {
            (true, false) => {
                d.last_tick = Some(Instant::now());
                unsafe {
                    SetTimer(Some(hwnd), TIMER_DRAG_SCROLL, DRAG_SCROLL_MS, None);
                }
            }
            (false, true) => {
                d.last_tick = None;
                unsafe {
                    let _ = KillTimer(Some(hwnd), TIMER_DRAG_SCROLL);
                }
            }
            _ => {}
        }
    }

    /// One tick of the drag auto-scroll: scrolls by the speed over the time
    /// since the last tick, moves the row to the place under the pointer and
    /// stops the timer when there is nothing left to scroll. The caller
    /// relayouts (and places the native fields).
    fn drag_tick(&mut self) {
        let speed = self.drag_scroll_now();
        let now = Instant::now();
        let Some(d) = &mut self.drag else {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_DRAG_SCROLL);
            }
            return;
        };
        let dt = d
            .last_tick
            .map_or(Duration::ZERO, |t| now.saturating_duration_since(t))
            .min(DRAG_SCROLL_MAX_DT);
        d.last_tick = Some(now);
        let max = self.layout.scroll_max.max(0.0);
        let scroll = (self.scroll + speed * dt.as_secs_f32()).clamp(0.0, max);
        if scroll != self.scroll {
            self.scroll = scroll;
            self.hover = None;
            self.drag_follow();
        }
        self.sync_drag_timer();
    }

    /// The row in hand is let go: put at its place (`commit`) or back where
    /// it was (Esc, the capture lost). The list shows the new order at once;
    /// the owner confirms it.
    fn end_drag(&mut self, commit: bool) -> Vec<After> {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_DRAG_SCROLL);
        }
        let Some(d) = self.drag.take() else {
            return Vec::new();
        };
        let mut after = Vec::new();
        if commit && d.from != d.to {
            match d.list {
                List::Pages => {
                    if let Some(pages) = page_drop(&self.model.pages, d.from, d.to) {
                        self.model.pages = pages.clone();
                        after.push(After::Post(Action::PanelPages(pages)));
                    }
                }
                List::Tiles => {
                    if let Some(tiles) = tile_drop(&self.model.tiles, d.from, d.to) {
                        self.model.tiles = tiles.clone();
                        after.push(After::Post(Action::ShortcutTiles(tiles)));
                    }
                }
            }
        }
        self.relayout();
        after
    }

    /// "Tambah" in the form: the tile it describes joins the list (on when
    /// the page has room), or the form says what is wrong.
    fn add_tile(&mut self) -> Vec<After> {
        let Some(form) = self.tile_form else {
            return Vec::new();
        };
        let fail = |st: &mut Settings, key: Key| {
            st.tile_form = Some(TileForm {
                error: Some(key),
                ..form
            });
            Vec::new()
        };
        let (name, target) = (&self.field_text[2], &self.field_text[3]);
        let (tiles, on) = match add_tile_to(&self.model.tiles, form.kind, name, target) {
            Ok(added) => added,
            Err(key) => return fail(self, key),
        };
        self.tile_form = None;
        self.tile_note = (!on).then_some(Key::TileAddedOff);
        if !on {
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_TILE_NOTE, ERROR_NOTE_MS, None);
            }
        }
        self.model.tiles = tiles.clone();
        vec![After::Post(Action::ShortcutTiles(tiles)), After::ResetForm]
    }

    fn set_note(&mut self, google: bool, note: Option<(String, Tone)>) {
        let error = note.as_ref().is_some_and(|(_, t)| *t == Tone::Bad);
        let (slot, timer) = if google {
            (&mut self.google_note, TIMER_GOOGLE_NOTE)
        } else {
            (&mut self.hook_note, TIMER_HOOK_NOTE)
        };
        *slot = note;
        let ms = if error { ERROR_NOTE_MS } else { NOTE_MS };
        unsafe {
            SetTimer(Some(self.hwnd), timer, ms, None);
        }
    }

    /// The gallery (+): the template menu is opened under the button once the
    /// state is released. No button in the layout (it scrolled away) is no menu.
    fn template_menu_after(&self) -> Vec<After> {
        let rect = self.layout.controls.iter().find_map(|c| match c {
            Control::IconButton { hit: Hit::WidgetAdd, rect, .. } => Some(*rect),
            _ => None,
        });
        let Some(rect) = rect else { return Vec::new() };
        let k = self.scale();
        let mut at = windows::Win32::Foundation::POINT {
            x: (rect.x * k).round() as i32,
            y: ((rect.y + rect.h) * k).round() as i32 + 2,
        };
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(self.hwnd, &mut at);
        }
        vec![After::TemplateMenu(at.x, at.y, self.model.lang)]
    }

    /// A click on `hit`. Returns what must happen once the state is released.
    fn on_click(&mut self, hit: Hit) -> Vec<After> {
        if hit != Hit::GoogleDisconnect && self.confirm {
            self.confirm = false;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_CONFIRM);
            }
        }
        if hit != Hit::ClipClear && self.clip_confirm {
            self.clip_confirm = false;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_CONFIRM);
            }
        }
        if hit != Hit::NotifClear && self.notif_confirm {
            self.notif_confirm = false;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_CONFIRM);
            }
        }
        let m = self.model.clone();
        let lang = m.lang;
        let post = |a: Action| vec![After::Post(a)];
        let after = match hit {
            Hit::Nav(section) => vec![After::Section(section)],
            Hit::Style(v) if v != m.style => post(Action::Style(v)),
            Hit::Accent(v) if v != m.accent => post(Action::Accent(v)),
            Hit::Surface(v) if v != m.surface => post(Action::Surface(v)),
            Hit::WindowMode(v) if v != m.window => post(Action::WindowMode(v)),
            Hit::Lang(v) if v != m.lang => post(Action::Lang(v)),
            Hit::Autostart => post(Action::Autostart(!m.autostart)),
            Hit::IdleClock => post(Action::IdleClock(!m.idle_clock)),
            Hit::EqFollowAudio => post(Action::EqFollowAudio(!m.eq_follow_audio)),
            Hit::StayBehind => post(Action::StayBehind(!m.stay_behind)),
            Hit::Lyrics => post(Action::Lyrics(!m.lyrics)),
            Hit::Shell(v) if v != m.shell => post(Action::Shell(v)),
            Hit::CliStart(v) if v != m.cli_start => post(Action::CliStart(v)),
            Hit::OpenMode(v) if v != m.open_mode => post(Action::OpenMode(v)),
            Hit::WidgetsShow => post(Action::WidgetsShow(!m.widgets.show)),
            Hit::WidgetsLayer(v) if v != m.widgets.layer => post(Action::WidgetsLayer(v)),
            Hit::WidgetsFullscreen(v) if v != m.widgets.fullscreen => {
                post(Action::WidgetsFullscreen(v))
            }
            Hit::WidgetsLock => post(Action::WidgetsLock(!m.widgets.lock)),
            Hit::WidgetsPeek => post(Action::WidgetsPeek(!m.widgets.cat_peek)),
            Hit::WidgetsTilt => post(Action::WidgetsTilt(!m.widgets.tilt)),
            Hit::WidgetsClipFill => post(Action::WidgetsClipFill(!m.widgets.clip_fill)),
            // At the cap the (+) is off and takes no click; a stray one adds nothing.
            // It opens the template menu under the button (`After::TemplateMenu`),
            // which posts `WidgetAdd` with the choice.
            Hit::WidgetAdd => self.template_menu_after(),
            Hit::WidgetCard => post(Action::WidgetHighlight),
            Hit::Robot => post(Action::Robot(!m.robot)),
            Hit::Motion(v) if v != m.robot_motion => post(Action::RobotMotion(v)),
            Hit::RobotSleep(dir) => {
                let v = sleep_step(m.robot_sleep, dir);
                if v == m.robot_sleep {
                    Vec::new()
                } else {
                    post(Action::RobotSleep(v))
                }
            }
            Hit::Greeting => post(Action::Greeting(!m.greeting)),
            Hit::CatColor(v) if v != m.cat_color => post(Action::CatColor(v)),
            Hit::CatAccessory(v) if v != m.cat_accessory => post(Action::CatAccessory(v)),
            Hit::CatFollow => post(Action::CatFollow(!m.cat_follow)),
            Hit::NotchPeek => post(Action::NotchPeek(!m.notch_peek)),
            Hit::CatDance => post(Action::CatDance(!m.cat_dance)),
            // The base color is picked in Tampilan; no second picker here.
            Hit::CatBackdrop => vec![After::Section(Section::Appearance)],
            // The preview's face: window state only, nothing is saved.
            Hit::CatFace(face) => {
                self.cat_face = face;
                Vec::new()
            }
            Hit::ClipEnabled => post(Action::ClipEnabled(!m.clip.enabled)),
            Hit::ClipItems(dir) => {
                let n = clip_items_step(m.clip.max_items, dir);
                if n == m.clip.max_items {
                    Vec::new()
                } else {
                    post(Action::ClipItems(n))
                }
            }
            Hit::ClipPersist => post(Action::ClipPersist(!m.clip.persist)),
            Hit::ClipClearOnLock => post(Action::ClipClearOnLock(!m.clip.clear_on_lock)),
            Hit::DeviceNotices => post(Action::DeviceNotices(!m.device_notices)),
            // A field: the first click records, the next on the same field
            // cancels.
            Hit::HotkeyField(id) => {
                if self.recording == Some(id) {
                    self.cancel_recording();
                } else {
                    self.start_recording(id);
                }
                Vec::new()
            }
            // The row's default; refused here (red text) when another row
            // has that combo, as the island would refuse it.
            Hit::HotkeyReset(id) => match id.default() {
                Some(hk) if used_by(&m.hotkeys, id, hk).is_some() => {
                    let other = used_by(&m.hotkeys, id, hk);
                    self.hotkey_error = other.map(|o| (id, RejectMsg::UsedBy(o)));
                    Vec::new()
                }
                value => {
                    self.hotkey_error = None;
                    post(Action::Hotkey(id, value))
                }
            },
            Hit::HotkeysResetAll => {
                self.hotkey_error = None;
                post(Action::HotkeysResetAll)
            }
            Hit::ClipNotice => post(Action::ClipNotice(!m.clip.copy_notice)),
            Hit::ClipFeedbackAll => post(Action::ClipFeedbackAll(!m.clip.copied_feedback_all)),
            Hit::ClipClear if self.clip_confirm => {
                self.clip_confirm = false;
                post(Action::ClipClear)
            }
            Hit::ClipClear => {
                self.clip_confirm = true;
                unsafe {
                    SetTimer(Some(self.hwnd), TIMER_CONFIRM, CONFIRM_MS, None);
                }
                Vec::new()
            }
            Hit::NotifRecord => post(Action::NotifRecord(!m.notif.record)),
            Hit::NotifItems(dir) => {
                let n = notif_items_step(m.notif.max_items, dir);
                if n == m.notif.max_items {
                    Vec::new()
                } else {
                    post(Action::NotifItems(n))
                }
            }
            Hit::NotifPersist => post(Action::NotifPersist(!m.notif.persist)),
            Hit::NotifMutedRemove(i) => match m.muted.get(i) {
                Some(name) => post(Action::NotifMutedRemove(name.clone())),
                None => Vec::new(),
            },
            // The typed name goes to the island (which trims, cuts, skips a
            // repeat) and the field empties for the next one.
            Hit::NotifMutedAdd => {
                let name = self.field_text[2].trim().to_owned();
                if name.is_empty() {
                    Vec::new()
                } else {
                    vec![After::Post(Action::NotifMutedAdd(name)), After::ResetForm]
                }
            }
            Hit::NotifClear if self.notif_confirm => {
                self.notif_confirm = false;
                post(Action::NotifClear)
            }
            Hit::NotifClear => {
                self.notif_confirm = true;
                unsafe {
                    SetTimer(Some(self.hwnd), TIMER_CONFIRM, CONFIRM_MS, None);
                }
                Vec::new()
            }
            Hit::ShotPreview => post(Action::ShotPreview(!m.shot.preview)),
            Hit::ShotAutoCopy => post(Action::ShotAutoCopy(!m.shot.auto_copy)),
            Hit::ShotTool(v) if v != m.shot.tool => post(Action::ShotTool(v)),
            Hit::ShotAfterCapture(v) if v != m.shot.after_capture => {
                post(Action::ShotAfterCapture(v))
            }
            Hit::NotifToastStyle(v) if v != m.notif.toast_style => {
                post(Action::NotifToastStyle(v))
            }
            Hit::ShotIgnore(v) if v != m.shot.on_ignore => post(Action::ShotOnIgnore(v)),
            Hit::ShotSecs(dir) => {
                let n = shot_secs_step(m.shot.show_secs, dir);
                if n == m.shot.show_secs {
                    Vec::new()
                } else {
                    post(Action::ShotSecs(n))
                }
            }
            Hit::Battery(dir) => {
                let t = battery_step(m.battery_threshold, dir);
                if t == m.battery_threshold {
                    Vec::new()
                } else {
                    post(Action::BatteryThreshold(t))
                }
            }
            Hit::DiskWarn(dir) => {
                let v = disk_warn_step(m.disk_warn_gb, dir);
                if v == m.disk_warn_gb {
                    Vec::new()
                } else {
                    post(Action::DiskWarn(v))
                }
            }
            Hit::RamWarn(dir) => {
                let v = ram_warn_step(m.ram_warn_percent, dir);
                if v == m.ram_warn_percent {
                    Vec::new()
                } else {
                    post(Action::RamWarn(v))
                }
            }
            Hit::MoleProtect => post(Action::MoleProtect(!m.mole_protect)),
            Hit::PageOn(_) | Hit::PageMove(..) | Hit::PageReset => {
                match page_action(&m.pages, hit) {
                    Some(pages) => post(Action::PanelPages(pages)),
                    None => Vec::new(),
                }
            }
            Hit::TileOn(_) | Hit::TileMove(..) | Hit::TileRemove(_) | Hit::TileReset => {
                match tile_action(&m.tiles, hit) {
                    Some(tiles) => post(Action::ShortcutTiles(tiles)),
                    None => Vec::new(),
                }
            }
            // The handles act on a press and a drag, not on a click; so do
            // the sliders.
            Hit::PageGrip(_) | Hit::TileGrip(_) | Hit::Slider(_) => Vec::new(),
            Hit::NotchReset => post(Action::NotchReset),
            Hit::NotchMonitor(pick) => {
                match monitor_choice(pick, &m.monitors, &m.notch_monitor) {
                    Some(choice) => post(Action::NotchMonitor(choice)),
                    None => Vec::new(),
                }
            }
            Hit::NotchIdentify => post(Action::NotchIdentify),
            Hit::NotchFill(v) if v != m.notch_fill => post(Action::NotchFill(v)),
            Hit::UiFont(v) if v != m.font => post(Action::Font(v)),
            Hit::ArrangeTemplateNew => {
                if m.arrange.templates.len() >= arrange::MAX_TEMPLATES {
                    Vec::new()
                } else {
                    self.arrange_edit = Some(ArrangeEdit {
                        target: EditTarget::New,
                        root: Node::Leaf,
                        selected: 0,
                        drag: None,
                    });
                    vec![After::SetName(String::new()), After::FocusName]
                }
            }
            Hit::ArrangeTemplateEdit(i) => match m.arrange.templates.get(i) {
                Some(t) => {
                    self.arrange_edit = Some(ArrangeEdit {
                        target: EditTarget::Template(i),
                        root: t.root.clone(),
                        selected: 0,
                        drag: None,
                    });
                    vec![After::SetName(t.name.clone()), After::FocusName]
                }
                None => Vec::new(),
            },
            Hit::ArrangeTemplateDelete(i) => {
                let mut list = m.arrange.templates.clone();
                if i < list.len() {
                    list.remove(i);
                    post(Action::ArrangeTemplates(list))
                } else {
                    Vec::new()
                }
            }
            Hit::ArrangeSavedRename(i) => match m.arrange.saved.get(i) {
                Some(s) => {
                    self.arrange_edit = Some(ArrangeEdit {
                        target: EditTarget::Rename(i),
                        root: Node::Leaf,
                        selected: 0,
                        drag: None,
                    });
                    vec![After::SetName(s.name.clone()), After::FocusName]
                }
                None => Vec::new(),
            },
            Hit::ArrangeSavedDelete(i) => {
                let mut list = m.arrange.saved.clone();
                if i < list.len() {
                    list.remove(i);
                    post(Action::ArrangeSaved(list))
                } else {
                    Vec::new()
                }
            }
            Hit::ArrangeSplit(dir) => {
                if let Some(e) = &mut self.arrange_edit {
                    e.root.split(e.selected, dir);
                }
                Vec::new()
            }
            Hit::ArrangeMerge => {
                if let Some(e) = &mut self.arrange_edit
                    && e.root.merge(e.selected)
                {
                    e.selected = e.selected.min(e.root.leaves() - 1);
                }
                Vec::new()
            }
            Hit::ArrangeEditorCancel => {
                self.arrange_edit = None;
                vec![After::ResetForm]
            }
            Hit::ArrangeEditorSave => self.arrange_editor_save(),
            Hit::TileFormOpen => {
                self.tile_form = Some(TileForm {
                    kind: CustomKind::App,
                    error: None,
                });
                self.tile_note = None;
                vec![After::ResetForm, After::FocusName]
            }
            Hit::TileFormCancel => {
                self.tile_form = None;
                vec![After::ResetForm]
            }
            Hit::TileFormKind(kind) => {
                if let Some(form) = &mut self.tile_form {
                    *form = TileForm { kind, error: None };
                }
                vec![After::Cue(kind)]
            }
            Hit::TileBrowse => vec![After::Browse],
            Hit::TileFormAdd => self.add_tile(),
            Hit::DndHelp => post(Action::DndHelp),
            Hit::Pomo(field, dir) => post(Action::Pomo(field, dir)),
            Hit::GoogleSave => {
                self.set_note(true, Some((text(lang, Key::Saved).into(), Tone::Good)));
                post(self.save_action())
            }
            Hit::GoogleConnect => {
                let mut after = Vec::new();
                if self.state().google.dirty {
                    after.push(After::Post(self.save_action()));
                }
                after.push(After::Post(Action::ConnectGoogle));
                let note = text(lang, Key::CheckBrowser).into();
                self.set_note(true, Some((note, Tone::Secondary)));
                after
            }
            Hit::GoogleDisconnect if self.confirm => {
                self.confirm = false;
                let note = text(lang, Key::Disconnected).into();
                self.set_note(true, Some((note, Tone::Secondary)));
                post(Action::DisconnectGoogle)
            }
            Hit::GoogleDisconnect => {
                self.confirm = true;
                unsafe {
                    SetTimer(Some(self.hwnd), TIMER_CONFIRM, CONFIRM_MS, None);
                }
                Vec::new()
            }
            Hit::GoogleAdvanced => {
                self.google_advanced = !self.google_advanced;
                Vec::new()
            }
            Hit::GoogleGuide => vec![After::Open(GUIDE_URL.into())],
            Hit::Repo => vec![After::Open(REPO_URL.into())],
            // Check, or install what a check found; a running one waits.
            Hit::UpdateCheck => match m.update_row {
                UpdateRow::Idle => post(Action::CheckUpdates),
                UpdateRow::Available(_) => post(Action::InstallUpdate),
                UpdateRow::Checking | UpdateRow::Downloading(_) => Vec::new(),
            },
            Hit::UpdateAuto => post(Action::UpdateAuto(!m.update_auto)),
            Hit::DataFolder => vec![After::OpenDataFolder],
            Hit::HooksInstall => {
                let note = match install::install_for_current_exe() {
                    InstallOutcome::Installed => (text(lang, Key::HooksOn).into(), Tone::Good),
                    InstallOutcome::AlreadyInstalled => {
                        (text(lang, Key::HooksAlready).into(), Tone::Good)
                    }
                    InstallOutcome::Error(e) => {
                        (format!("{}: {e}", text(lang, Key::Failed)), Tone::Bad)
                    }
                };
                self.hooks = install::hook_state();
                self.set_note(false, Some(note));
                Vec::new()
            }
            Hit::HooksRemove => {
                let note = match install::uninstall_for_current_user() {
                    UninstallOutcome::Removed => {
                        (text(lang, Key::HooksRemoved).into(), Tone::Secondary)
                    }
                    UninstallOutcome::NothingToRemove => {
                        (text(lang, Key::HooksNone).into(), Tone::Secondary)
                    }
                    UninstallOutcome::Error(e) => {
                        (format!("{}: {e}", text(lang, Key::Failed)), Tone::Bad)
                    }
                };
                self.hooks = install::hook_state();
                self.set_note(false, Some(note));
                Vec::new()
            }
            // The current choice again: nothing to change.
            Hit::Style(_)
            | Hit::Accent(_)
            | Hit::Surface(_)
            | Hit::CatColor(_)
            | Hit::CatAccessory(_)
            | Hit::NotchFill(_)
            | Hit::UiFont(_)
            | Hit::WindowMode(_)
            | Hit::Lang(_)
            | Hit::Shell(_)
            | Hit::CliStart(_)
            | Hit::OpenMode(_)
            | Hit::WidgetsLayer(_)
            | Hit::WidgetsFullscreen(_)
            | Hit::Motion(_)
            | Hit::ShotIgnore(_)
            | Hit::ShotTool(_)
            | Hit::ShotAfterCapture(_)
            | Hit::NotifToastStyle(_) => Vec::new(),
        };
        self.relayout();
        after
    }

    /// The press at `lparam` is on the template editor's canvas: a divider
    /// is taken (the drag follows), a tile is selected. False elsewhere.
    fn canvas_down(&mut self, lparam: LPARAM) -> bool {
        let (x, y) = self.point(lparam);
        let Some(canvas) = self.layout.canvas else {
            return false;
        };
        let Some(e) = &mut self.arrange_edit else {
            return false;
        };
        match canvas_press(&e.root, canvas, x, y) {
            Some(CanvasPress::Divider(path)) => e.drag = Some(path),
            Some(CanvasPress::Tile(i)) => e.selected = i,
            None => return false,
        }
        self.relayout();
        true
    }

    /// A divider in hand follows the pointer (snapped and clamped by the
    /// model).
    fn canvas_move(&mut self, lparam: LPARAM) {
        let (x, y) = self.point(lparam);
        let Some(canvas) = self.layout.canvas else {
            return;
        };
        let Some(e) = &mut self.arrange_edit else {
            return;
        };
        let Some(path) = e.drag.clone() else {
            return;
        };
        if canvas_drag(&mut e.root, canvas, &path, x, y) {
            self.relayout();
        }
    }

    /// The button went up: a divider in hand is let go. True when it was.
    fn canvas_up(&mut self) -> bool {
        self.arrange_edit
            .as_mut()
            .is_some_and(|e| e.drag.take().is_some())
    }

    /// "Simpan" of the editor: the template (or the new name of a saved
    /// arrangement) goes to the island and the editor closes. With no valid
    /// name nothing happens.
    fn arrange_editor_save(&mut self) -> Vec<After> {
        let Some(e) = self.arrange_edit.take() else {
            return Vec::new();
        };
        let Some(name) = arrange::clean_name(&self.field_text[2]) else {
            self.arrange_edit = Some(e);
            return Vec::new();
        };
        let a = &self.model.arrange;
        let action = match &e.target {
            EditTarget::New => {
                let id = arrange::new_template_id(&a.templates);
                match CustomTemplate::new(&id, &name, e.root.clone()) {
                    Some(t) if a.templates.len() < arrange::MAX_TEMPLATES => {
                        let mut list = a.templates.clone();
                        list.push(t);
                        Action::ArrangeTemplates(list)
                    }
                    _ => {
                        self.arrange_edit = Some(e);
                        return Vec::new();
                    }
                }
            }
            EditTarget::Template(i) => {
                let mut list = a.templates.clone();
                let Some(old) = list.get(*i) else {
                    return vec![After::ResetForm];
                };
                match CustomTemplate::new(&old.id, &name, e.root.clone()) {
                    Some(t) => list[*i] = t,
                    None => {
                        self.arrange_edit = Some(e);
                        return Vec::new();
                    }
                }
                Action::ArrangeTemplates(list)
            }
            EditTarget::Rename(i) => {
                let mut list = a.saved.clone();
                if let Some(s) = list.get_mut(*i) {
                    s.name = name;
                }
                Action::ArrangeSaved(list)
            }
        };
        vec![After::Post(action), After::ResetForm]
    }

    fn save_action(&self) -> Action {
        Action::SaveGoogle {
            id: self.field_text[0].trim().to_owned(),
            secret: self.field_text[1].trim().to_owned(),
        }
    }

    fn switch_section(&mut self, section: Section) -> bool {
        if section == self.section {
            return false;
        }
        let now = Instant::now();
        let from = self.section.index() as f32;
        self.anims.nav = Some(Tween::toward(
            self.anims.nav,
            from,
            section.index() as f32,
            now,
        ));
        self.anims.content = Some(Tween {
            from: 0.0,
            to: 1.0,
            start: now,
        });
        // Susun's template editor lives on Fitur and borrows the shortcut
        // form's Nama field: leaving Fitur closes it, as Batal would.
        if self.section == Section::Features {
            self.arrange_edit = None;
        }
        // A recording stops, and a refusal's red text goes, with the section.
        self.cancel_recording();
        self.hotkey_error = None;
        self.section = section;
        self.scroll = 0.0;
        self.focused = None;
        self.hover = None;
        self.start_clock();
        true
    }

    // ---- Recording a hotkey ----

    /// A click on a row's field: the island lets go of its hotkeys, the
    /// recorder's keyboard hook goes in (or, if it cannot, the window's own
    /// key messages record), and the field asks for keys.
    fn start_recording(&mut self, id: HotkeyId) {
        self.stop_rec_hook();
        self.resume.start();
        self.hotkey_error = None;
        self.recording = Some(id);
        post_action(self.owner, Action::HotkeyRecording(true));
        self.rec_hook = RecHook::start(self.hwnd);
    }

    /// Stops the recorder's hook now (a finished one is only joined).
    fn stop_rec_hook(&mut self) {
        if let Some(hook) = self.rec_hook.take() {
            hook.stop();
        }
    }

    /// The recording ends with nothing changed, by the user's click, a
    /// section change or the window losing the foreground: the hook goes at
    /// once and the island takes its hotkeys back. Nothing when none runs.
    /// The field's recording look is part of the layout, so the layout is
    /// built again: the island's answer does not do it (its model is the same).
    fn cancel_recording(&mut self) {
        self.stop_rec_hook();
        if self.recording.take().is_some() {
            if self.resume.end(false) {
                post_action(self.owner, Action::HotkeyRecording(false));
            }
            self.relayout();
        } else if self.resume.hook_done(false) {
            // A finished recording whose hook was still draining.
            post_action(self.owner, Action::HotkeyRecording(false));
        }
    }

    /// Whether the recorder's hook thread still runs (listening or draining).
    fn rec_hook_running(&self) -> bool {
        self.rec_hook.as_ref().is_some_and(RecHook::running)
    }

    /// The recorder's hook thread ended (`WM_APP_HOTKEY_REC_DONE`): a
    /// recording that ended while it drained gives the island its hotkeys
    /// back now.
    fn on_rec_hook_done(&mut self) {
        if self.resume.hook_done(self.rec_hook_running()) {
            post_action(self.owner, Action::HotkeyRecording(false));
        }
    }

    /// A key pressed while recording, from the hook (`WM_APP_HOTKEY_REC`) or
    /// the window's own key messages (the fallback): the `Hotkey` it makes, if
    /// any, goes to the owner before `HotkeyRecording(false)`. The hook is left
    /// to let go of the held keys by itself, and `HotkeyRecording(false)` waits
    /// for that: while it drains, a held key's auto-repeat must not reach the
    /// island's PrtSc hook, which would be newer and so see it first.
    fn on_rec_key(&mut self, vk: u16, mods: Mods) {
        let Some(id) = self.recording else {
            return;
        };
        // vk 0: the hook gave up waiting.
        let step = if vk == 0 {
            Step::Cancel
        } else {
            record_step(vk, mods)
        };
        let RecOutcome::End { post, error } = rec_outcome(&self.model.hotkeys, id, step) else {
            return;
        };
        if let Some(msg) = error {
            self.hotkey_error = Some((id, msg));
        }
        if let Some(value) = post {
            post_action(self.owner, Action::Hotkey(id, value));
        }
        self.recording = None;
        if self.resume.end(self.rec_hook_running()) {
            post_action(self.owner, Action::HotkeyRecording(false));
        }
        self.relayout();
    }

    /// The fallback while the recorder's hook could not go in: a key message
    /// of the window, with the modifiers read from the keyboard state.
    fn on_fallback_key(&mut self, vk: u16) {
        let mods = Mods {
            ctrl: key_down(VK_CONTROL.0),
            alt: key_down(VK_MENU.0),
            shift: key_down(VK_SHIFT.0),
            win: key_down(VK_LWIN.0) || key_down(VK_RWIN.0),
        };
        self.on_rec_key(vk, mods);
    }
}

/// When the island gets its hotkeys back after a recording: once the
/// recorder's hook thread has ended, never while it still drains. Its
/// swallowed keys are down for the user but up for Windows, so the island's
/// PrtSc hook (installed again on resume, and newer, so first in line) would
/// take a held PrtSc's auto-repeat for a fresh press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Resume {
    /// The recording ended while the hook still ran.
    pending: bool,
}

impl Resume {
    /// A recording starts: one still waiting is dropped (the island stays
    /// paused for the new one).
    fn start(&mut self) {
        self.pending = false;
    }

    /// The recording ended; true: post `HotkeyRecording(false)` now.
    fn end(&mut self, hook_running: bool) -> bool {
        self.pending = hook_running;
        !hook_running
    }

    /// The hook thread ended, or was stopped; true: post it now. A stale
    /// message while a newer hook runs, or with nothing waiting, does nothing.
    fn hook_done(&mut self, hook_running: bool) -> bool {
        if self.pending && !hook_running {
            self.pending = false;
            return true;
        }
        false
    }
}

/// What a recorder step does to the recording of row `id`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecOutcome {
    /// A modifier alone: keep listening.
    Listen,
    /// The recording ends. `post`: the `Hotkey` action to send first (a new
    /// combo, or `None` to turn the row off); `error`: why a combo was
    /// refused (shown under the row).
    End {
        post: Option<Option<Hotkey>>,
        error: Option<RejectMsg>,
    },
}

/// The recorder's decision for `step` on row `id`, with the rows as the
/// window has them: Esc ends with nothing, Backspace and Delete clear the
/// row, a combo is checked (reserved, needs a modifier) and compared with the
/// other rows, and only a good one is posted.
fn rec_outcome(keys: &Hotkeys, id: HotkeyId, step: Step) -> RecOutcome {
    let refused = |msg| RecOutcome::End {
        post: None,
        error: Some(msg),
    };
    match step {
        Step::Ignore => RecOutcome::Listen,
        Step::Cancel => RecOutcome::End {
            post: None,
            error: None,
        },
        Step::Clear => RecOutcome::End {
            post: Some(None),
            error: None,
        },
        Step::Combo(hk) => match check(hk) {
            Err(Reject::Reserved) => refused(RejectMsg::Reserved),
            Err(Reject::NeedsModifier) => refused(RejectMsg::NeedsModifier),
            Ok(()) => match used_by(keys, id, hk) {
                Some(other) => refused(RejectMsg::UsedBy(other)),
                None => RecOutcome::End {
                    post: Some(Some(hk)),
                    error: None,
                },
            },
        },
    }
}

/// Side effects of a click, run after the state is released (they may send
/// messages back to this window or pump the queue).
enum After {
    Post(Action),
    Section(Section),
    Open(String),
    OpenDataFolder,
    /// The own-shortcut form's fields go back to empty.
    ResetForm,
    /// The Target field's hint follows the kind chosen.
    Cue(CustomKind),
    /// The form's Nama field takes the keyboard.
    FocusName,
    /// "Pilih…": the file or folder dialog fills the Target field.
    Browse,
    /// The Nama field takes this text (the template editor opening).
    SetName(String),
    /// The sticky note templates, as a native menu at this screen point.
    TemplateMenu(i32, i32, Lang),
}

fn run_after(hwnd: HWND, owner: HWND, after: Vec<After>) {
    for a in after {
        match a {
            After::Post(action) => post_action(owner, action),
            After::Section(section) => set_section(hwnd, section),
            After::Open(url) => shell_open(&url),
            After::ResetForm => reset_form(),
            After::Cue(kind) => set_cue(kind),
            After::FocusName => unsafe {
                let _ = PostMessageW(Some(hwnd), WM_APP_FORM, WPARAM(2), LPARAM(0));
            },
            After::Browse => browse(hwnd),
            After::SetName(text) => set_name_text(&text),
            After::TemplateMenu(x, y, lang) => {
                let at = windows::Win32::Foundation::POINT { x, y };
                if let Some(t) = crate::widgets::host::template_menu(hwnd, at, lang) {
                    post_action(owner, Action::WidgetAdd(t));
                }
            }
            After::OpenDataFolder => {
                let dir = crate::config::path()
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default();
                let _ = std::fs::create_dir_all(&dir);
                shell_open(&dir.display().to_string());
            }
        }
    }
}

/// The form's two fields, when the window has them.
fn tile_edits() -> Option<(HWND, HWND)> {
    with(|st| st.edits.as_ref().map(|e| (e.name, e.target))).flatten()
}

/// Empties the form's fields (and shows the app hint again).
fn reset_form() {
    if let Some((name, target)) = tile_edits() {
        unsafe {
            let _ = SetWindowTextW(name, &HSTRING::new());
            let _ = SetWindowTextW(target, &HSTRING::new());
        }
    }
    set_cue(CustomKind::App);
    set_name_cue();
}

/// The Nama field's text (the template editor opening), with the editor's
/// hint and length.
fn set_name_text(text: &str) {
    if let Some((name, _)) = tile_edits() {
        unsafe {
            let _ = SetWindowTextW(name, &HSTRING::from(text));
        }
    }
    set_name_cue();
}

/// What the shared Nama field holds: Susun's template name (only while the
/// editor is open on Fitur), an excluded app's name (Fitur otherwise), or an
/// own tile's label (the shortcut form on Panel).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NameRole {
    Template,
    MutedApp,
    TileLabel,
}

fn name_role(section: Section, editing: bool) -> NameRole {
    match (section, editing) {
        (Section::Features, true) => NameRole::Template,
        (Section::Features, false) => NameRole::MutedApp,
        _ => NameRole::TileLabel,
    }
}

/// How many characters the Nama field takes: an excluded app's name while
/// Fitur uses it (`MUTED_NAME_MAX`), a tile label otherwise (`LABEL_MAX`).
fn name_limit(section: Section) -> usize {
    if section == Section::Features {
        crate::config::MUTED_NAME_MAX
    } else {
        LABEL_MAX
    }
}

/// The Nama field's hint and length: "Nama aplikasi" and an app name's
/// length while Fitur uses it for the excluded apps, the form's "Gmail" and a
/// tile label's length otherwise.
fn set_name_cue() {
    let Some((name, _)) = tile_edits() else {
        return;
    };
    let Some((lang, section, editing)) =
        with(|st| (st.model.lang, st.section, st.arrange_edit.is_some()))
    else {
        return;
    };
    if name_role(section, editing) == NameRole::Template {
        // Susun's template editor: a name of up to 24 characters.
        send(name, EM_SETLIMITTEXT, arrange::NAME_MAX, 0);
        let cue = HSTRING::from(text(lang, Key::ArrangeFieldName));
        send(name, EM_SETCUEBANNER, 0, cue.as_ptr() as isize);
        return;
    }
    send(name, EM_SETLIMITTEXT, name_limit(section), 0);
    let cue = if section == Section::Features {
        HSTRING::from(text(lang, Key::NotifMutedHint))
    } else {
        HSTRING::from("Gmail")
    };
    send(name, EM_SETCUEBANNER, 0, cue.as_ptr() as isize);
}

fn set_cue(kind: CustomKind) {
    if let Some((_, target)) = tile_edits() {
        send(target, EM_SETCUEBANNER, 0, cue_for(kind).0 as isize);
    }
}

/// "Pilih…": the file (an app) or folder dialog; what it picks goes in the
/// Target field. Runs with the window's state released: the dialog pumps
/// messages.
fn browse(hwnd: HWND) {
    let Some(kind) = with(|st| st.tile_form.map(|f| f.kind)).flatten() else {
        return;
    };
    let Some(path) = pick_path(hwnd, kind) else {
        return;
    };
    if let Some((_, target)) = tile_edits() {
        unsafe {
            let _ = SetWindowTextW(target, &HSTRING::from(path.as_os_str()));
        }
    }
}

/// The path the owner picks in the system dialog: an exe or any file for an
/// app, a folder for a folder. `None` when it is cancelled or cannot open.
fn pick_path(owner: HWND, kind: CustomKind) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    let lang = crate::i18n::lang();
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let mut options = dialog.GetOptions().ok()?;
        options |= FOS_FORCEFILESYSTEM;
        if kind == CustomKind::Folder {
            options |= FOS_PICKFOLDERS;
        } else {
            let apps = HSTRING::from(text(lang, Key::TileFilterApps));
            let all = HSTRING::from(text(lang, Key::TileFilterAll));
            let filters = [
                COMDLG_FILTERSPEC {
                    pszName: PCWSTR(apps.as_ptr()),
                    pszSpec: w!("*.exe;*.lnk;*.bat;*.cmd"),
                },
                COMDLG_FILTERSPEC {
                    pszName: PCWSTR(all.as_ptr()),
                    pszSpec: w!("*.*"),
                },
            ];
            let _ = dialog.SetFileTypes(&filters);
        }
        dialog.SetOptions(options).ok()?;
        // Cancelled: an error code.
        dialog.Show(Some(owner)).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(name.as_wide()));
        CoTaskMemFree(Some(name.0 as *const c_void));
        Some(path)
    }
}

fn shell_open(target: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

fn set_section(hwnd: HWND, section: Section) {
    let was = with(|st| st.section);
    if with(|st| st.switch_section(section)) == Some(true) {
        // Fitur reuses the own-shortcut form's Nama field for the excluded
        // apps: what was typed for one is not the other's.
        if was == Some(Section::Features) || section == Section::Features {
            reset_form();
            set_name_cue();
        }
        // A focused field is about to hide; keep the keyboard on the window.
        unsafe {
            let _ = SetFocus(Some(hwnd));
        }
        relayout_and_place();
    }
}

fn on_mouse_move(hwnd: HWND, lparam: LPARAM) {
    // A divider of the template editor follows the pointer.
    with(|st| st.canvas_move(lparam));
    // A slider's thumb follows the pointer; the island follows live.
    let slid = with(|st| {
        let id = st.slider?;
        let (x, _) = st.point(lparam);
        st.slider_move(id, x).map(|action| (st.owner, action))
    })
    .flatten();
    if let Some((owner, action)) = slid {
        post_action(owner, action);
    }
    let arm = with(|st| {
        if st.drag.is_some() {
            let (_, y) = st.point(lparam);
            st.drag_to(y);
        } else {
            let hit = st.hit_at(lparam);
            if hit != st.hover {
                st.hover = hit;
                st.invalidate();
            }
        }
        !std::mem::replace(&mut st.tracking, true)
    });
    if arm == Some(true) {
        let mut track = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        unsafe {
            let _ = TrackMouseEvent(&mut track);
        }
    }
}

fn on_mouse_leave() {
    with(|st| {
        st.tracking = false;
        if st.hover.take().is_some() {
            st.invalidate();
        }
    });
}

fn on_button_down(hwnd: HWND, lparam: LPARAM) {
    // The template editor's canvas: a divider is taken or a tile selected.
    if with(|st| st.canvas_down(lparam)).unwrap_or(false) {
        unsafe {
            let _ = SetFocus(Some(hwnd));
            SetCapture(hwnd);
        }
        return;
    }
    let hit = with(|st| {
        st.pressed = st.hit_at(lparam);
        // A press anywhere but on the field being recorded ends the
        // recording; the click itself still does what it would.
        if let Some(id) = st.recording
            && st.pressed != Some(Hit::HotkeyField(id))
        {
            st.cancel_recording();
        }
        // A handle takes the row in hand at once.
        if let Some(h @ (Hit::PageGrip(_) | Hit::TileGrip(_))) = st.pressed {
            let (_, y) = st.point(lparam);
            st.begin_drag(h, y);
        }
        st.invalidate();
        st.pressed
    });
    unsafe {
        // A click outside the fields takes the keyboard back from them.
        let _ = SetFocus(Some(hwnd));
        if hit.flatten().is_some() {
            SetCapture(hwnd);
        }
    }
    // A press on a slider jumps the thumb there and starts a drag.
    let jumped = with(|st| {
        let Some(Hit::Slider(id)) = st.pressed else {
            return None;
        };
        st.slider = Some(id);
        let (x, _) = st.point(lparam);
        st.slider_move(id, x).map(|action| (st.owner, action))
    })
    .flatten();
    if let Some((owner, action)) = jumped {
        post_action(owner, action);
    }
}

fn on_button_up(hwnd: HWND, lparam: LPARAM) {
    if with(|st| st.canvas_up()).unwrap_or(false) {
        unsafe {
            let _ = ReleaseCapture();
        }
        return;
    }
    // A slider's thumb lets go here: the value is final, so it is saved.
    let released = with(|st| {
        let id = st.slider.take()?;
        st.pressed = None;
        let (x, _) = st.point(lparam);
        // The last move may have been a step short of where the pointer is.
        st.slider_move(id, x);
        st.invalidate();
        Some((st.owner, st.slider_action(id, true)))
    })
    .flatten();
    if let Some((owner, action)) = released {
        unsafe {
            let _ = ReleaseCapture();
        }
        post_action(owner, action);
        return;
    }
    // A row in hand drops here.
    let dropped = with(|st| {
        st.drag.is_some().then(|| {
            st.pressed = None;
            (st.owner, st.end_drag(true))
        })
    })
    .flatten();
    if let Some((owner, after)) = dropped {
        unsafe {
            let _ = ReleaseCapture();
        }
        run_after(hwnd, owner, after);
        relayout_and_place();
        return;
    }
    let clicked = with(|st| {
        let hit = st.hit_at(lparam);
        let pressed = st.pressed.take();
        st.invalidate();
        (st.owner, hit.filter(|h| Some(*h) == pressed))
    });
    unsafe {
        let _ = ReleaseCapture();
    }
    let Some((owner, Some(hit))) = clicked else {
        return;
    };
    let after = with(|st| st.on_click(hit)).unwrap_or_default();
    run_after(hwnd, owner, after);
    relayout_and_place();
}

/// A recording is on and the recorder's hook is not running (it could not be
/// installed): the window's own key messages record.
fn recording_without_hook() -> bool {
    with(|st| st.recording.is_some() && st.rec_hook.is_none()).unwrap_or(false)
}

fn on_key(hwnd: HWND, key: u16) -> bool {
    // Esc with a row in hand puts it back instead of closing the window.
    if key == VK_ESCAPE.0 && cancel_drag() {
        return true;
    }
    if key == VK_ESCAPE.0 {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        return true;
    }
    let step = if key == VK_DOWN.0 {
        1
    } else if key == VK_UP.0 {
        -1
    } else if key == VK_TAB.0 {
        // Tab from the window goes to the first field, when it shows.
        let first = with(|st| match st.section {
            Section::Integrations => st.edits.as_ref().map(|e| e.id),
            Section::Panel if st.tile_form.is_some() => st.edits.as_ref().map(|e| e.name),
            // The excluded apps' field (the form's Nama input, reused).
            Section::Features => st.edits.as_ref().map(|e| e.name),
            _ => None,
        });
        if let Some(Some(edit)) = first {
            unsafe {
                let _ = SetFocus(Some(edit));
            }
        }
        return true;
    } else {
        return false;
    };
    let next = with(|st| {
        let i = st.section.index() as i32 + step;
        Section::ALL
            .get(i.clamp(0, Section::ALL.len() as i32 - 1) as usize)
            .copied()
    });
    if let Some(Some(section)) = next {
        set_section(hwnd, section);
    }
    true
}

/// Puts a row in hand back; false when none was.
fn cancel_drag() -> bool {
    let held = with(|st| {
        st.pressed = None;
        let held = st.drag.is_some();
        st.end_drag(false);
        held
    });
    if held != Some(true) {
        return false;
    }
    unsafe {
        let _ = ReleaseCapture();
    }
    relayout_and_place();
    true
}

fn on_command(wparam: WPARAM, lparam: LPARAM) {
    let id = (wparam.0 & 0xFFFF) as i32;
    let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
    let field = match id {
        ID_CLIENT_ID => Field::ClientId,
        ID_CLIENT_SECRET => Field::ClientSecret,
        ID_TILE_NAME => Field::TileName,
        ID_TILE_TARGET => Field::TileTarget,
        _ => return,
    };
    let edit = HWND(lparam.0 as *mut c_void);
    match code {
        EN_CHANGE => {
            let value = window_text(edit);
            with(|st| {
                st.field_text[field.index()] = value;
                match field {
                    Field::ClientId | Field::ClientSecret => st.google_note = None,
                    // Typing again clears the message about the last try.
                    Field::TileName | Field::TileTarget => {
                        if let Some(form) = &mut st.tile_form {
                            form.error = None;
                        }
                    }
                }
                st.relayout();
            });
        }
        EN_SETFOCUS => {
            with(|st| {
                st.focused = Some(field);
                st.invalidate();
            });
        }
        EN_KILLFOCUS => {
            with(|st| {
                if st.focused == Some(field) {
                    st.focused = None;
                }
                st.invalidate();
            });
        }
        _ => {}
    }
}

fn on_dpi_changed(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let dpi = (wparam.0 & 0xFFFF) as u32;
    let edits = with(|st| {
        st.dpi = dpi.max(96);
        st.gfx.reset_target();
        let ui = st.model.font;
        st.edits.as_mut().map(|e| {
            let font = create_font(st.dpi, ui);
            let old = std::mem::replace(&mut e.font, font);
            e.line_h = line_height(st.hwnd, font);
            (e.hwnds(), font, old)
        })
    })
    .flatten();
    if let Some((hwnds, font, old)) = edits {
        for edit in hwnds {
            send(edit, WM_SETFONT, font.0 as usize, 1);
        }
        unsafe {
            let _ = DeleteObject(HGDIOBJ(old.0));
        }
    }
    set_icons(hwnd, dpi);
    let suggested = lparam.0 as *const RECT;
    if let Some(rc) = unsafe { suggested.as_ref() } {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                rc.left,
                rc.top,
                rc.right - rc.left,
                rc.bottom - rc.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
    // The same pixel size at another DPI is another logical size.
    with(|st| st.read_client());
    relayout_and_place();
}

fn on_timer(id: usize) {
    if id == TIMER_DRAG_SCROLL {
        // Not killed here: the timer runs until `sync_drag_timer` stops it.
        with(|st| st.drag_tick());
        // The form's native fields follow the content.
        relayout_and_place();
        return;
    }
    with(|st| {
        unsafe {
            let _ = KillTimer(Some(st.hwnd), id);
        }
        match id {
            TIMER_CONFIRM => {
                st.confirm = false;
                st.clip_confirm = false;
                st.notif_confirm = false;
            }
            TIMER_GOOGLE_NOTE => st.google_note = None,
            TIMER_HOOK_NOTE => st.hook_note = None,
            TIMER_TILE_NOTE => st.tile_note = None,
            _ => return,
        }
        st.relayout();
    });
}

fn color_edit(wparam: WPARAM) -> LRESULT {
    let hdc = HDC(wparam.0 as *mut c_void);
    let p = theme::current_window();
    let brush = with(|st| st.edits.as_ref().map(|e| e.brush)).flatten();
    unsafe {
        SetTextColor(hdc, colorref(p.text));
        SetBkColor(hdc, colorref(p.field));
    }
    LRESULT(brush.map_or(0, |b| b.0 as isize))
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            with(Settings::render);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        // Direct2D paints every pixel.
        WM_ERASEBKGND => LRESULT(1),
        // The smallest the window can be: the minimum client size. It comes
        // before the state exists, so nothing here uses `with`. Maximizing
        // uses the default (the monitor's work area).
        WM_GETMINMAXINFO => {
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
            let (w, h) = outer_size(dpi);
            if let Some(info) = unsafe { (lparam.0 as *mut MINMAXINFO).as_mut() } {
                info.ptMinTrackSize = POINT { x: w, y: h };
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let (w, h) = (
                (lparam.0 & 0xFFFF) as u32,
                ((lparam.0 >> 16) & 0xFFFF) as u32,
            );
            let kind = wparam.0 as u32;
            let sized = with(|st| {
                st.gfx.resize(w, h);
                // Minimized has no client area: keep the last size.
                if w > 0 && h > 0 {
                    let s = st.scale();
                    st.client = (w as f32 / s, h as f32 / s);
                    true
                } else {
                    false
                }
            });
            if sized == Some(true) {
                relayout_and_place();
            }
            // Maximized or restored from it: the place is worth saving now
            // (a plain resize is saved when it ends).
            with(|st| {
                let now = match kind {
                    SIZE_MAXIMIZED => true,
                    SIZE_RESTORED => false,
                    _ => st.maximized,
                };
                if now != st.maximized {
                    st.maximized = now;
                    st.post_place();
                }
            });
            LRESULT(0)
        }
        // A move or a resize with the mouse ended.
        WM_EXITSIZEMOVE => {
            with(|st| st.post_place());
            LRESULT(0)
        }
        WM_APP_FRAME => {
            with(|st| {
                if let Some(clock) = &st.clock {
                    clock.ack();
                }
                st.invalidate();
            });
            LRESULT(0)
        }
        WM_APP_PLACE => {
            relayout_and_place();
            LRESULT(0)
        }
        WM_APP_SUBMIT => {
            let save = with(|st| {
                let dirty = st.state().google.dirty;
                if dirty {
                    let after = st.on_click(Hit::GoogleSave);
                    Some((st.owner, after))
                } else {
                    None
                }
            })
            .flatten();
            if let Some((owner, after)) = save {
                run_after(hwnd, owner, after);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            on_mouse_move(hwnd, lparam);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            on_mouse_leave();
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // High word of wParam: signed delta, +120 per notch away from the user.
            let delta = i32::from((wparam.0 >> 16) as u16 as i16);
            with(|st| st.on_wheel(delta));
            // The form's native fields follow the content.
            relayout_and_place();
            LRESULT(0)
        }
        WM_APP_FORM => {
            match wparam.0 {
                0 | 1 => {
                    // On Fitur the field is the excluded apps': Enter adds
                    // the name, Esc empties the field.
                    // Susun's editor: Enter saves, Esc cancels.
                    let role = with(|st| name_role(st.section, st.arrange_edit.is_some()));
                    if role == Some(NameRole::Template) {
                        let hit = if wparam.0 == 0 {
                            Hit::ArrangeEditorSave
                        } else {
                            Hit::ArrangeEditorCancel
                        };
                        if let Some((owner, after)) = with(|st| (st.owner, st.on_click(hit))) {
                            run_after(hwnd, owner, after);
                        }
                        relayout_and_place();
                        return LRESULT(0);
                    }
                    let features = role == Some(NameRole::MutedApp);
                    if features && wparam.0 == 1 {
                        reset_form();
                    } else {
                        let hit = match (features, wparam.0) {
                            (true, _) => Hit::NotifMutedAdd,
                            (false, 0) => Hit::TileFormAdd,
                            (false, _) => Hit::TileFormCancel,
                        };
                        if let Some((owner, after)) = with(|st| (st.owner, st.on_click(hit))) {
                            run_after(hwnd, owner, after);
                        }
                    }
                    relayout_and_place();
                }
                2 => {
                    if let Some((name, _)) = tile_edits() {
                        unsafe {
                            let _ = SetFocus(Some(name));
                        }
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        // The pointer was taken from a row in hand (another window came up).
        WM_CAPTURECHANGED => {
            // A slider's drag cut short keeps the value it reached: saved.
            let cut = with(|st| {
                st.pressed = None;
                st.end_drag(false);
                let id = st.slider.take()?;
                st.invalidate();
                Some((st.owner, st.slider_action(id, true)))
            })
            .flatten();
            if let Some((owner, action)) = cut {
                post_action(owner, action);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            on_button_down(hwnd, lparam);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_button_up(hwnd, lparam);
            LRESULT(0)
        }
        WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == HTCLIENT => {
            let (link, grip) = with(|st| {
                (
                    matches!(st.hover, Some(Hit::GoogleGuide)),
                    st.drag.is_some()
                        || matches!(st.hover, Some(Hit::PageGrip(_) | Hit::TileGrip(_))),
                )
            })
            .unwrap_or_default();
            unsafe {
                let cursor = LoadCursorW(
                    None,
                    if grip {
                        IDC_SIZEALL
                    } else if link {
                        IDC_HAND
                    } else {
                        IDC_ARROW
                    },
                );
                SetCursor(cursor.ok());
            }
            LRESULT(1)
        }
        // A combo the recorder's hook saw (or gave up waiting for).
        WM_APP_HOTKEY_REC => {
            let (vk, mods) = (wparam.0 as u16, Mods::from_bits(lparam.0 as u8));
            with(|st| st.on_rec_key(vk, mods));
            LRESULT(0)
        }
        // The recorder's hook thread ended: it holds no key any more.
        WM_APP_HOTKEY_REC_DONE => {
            with(|st| st.on_rec_hook_done());
            LRESULT(0)
        }
        // The fallback while the hook could not go in: the window's own key
        // messages record (Esc cancels instead of closing the window).
        WM_KEYDOWN | WM_SYSKEYDOWN if recording_without_hook() => {
            let vk = (wparam.0 & 0xFFFF) as u16;
            with(|st| st.on_fallback_key(vk));
            LRESULT(0)
        }
        // Another window came up (or Win+L): a recording in progress ends.
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                with(|st| st.cancel_recording());
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_KEYDOWN if on_key(hwnd, (wparam.0 & 0xFFFF) as u16) => LRESULT(0),
        WM_COMMAND => {
            on_command(wparam, lparam);
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => color_edit(wparam),
        WM_TIMER => {
            on_timer(wparam.0);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            on_dpi_changed(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_CLOSE => {
            with(|st| st.post_place());
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(owner) = with(|st| {
                st.clock = None;
                // The recorder's hook never outlives the window; a recording
                // in progress ends (the island takes its hotkeys back).
                st.cancel_recording();
                st.owner
            }) {
                post_action(owner, Action::Closed);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let state = SETTINGS.with(|cell| cell.try_borrow_mut().ok().and_then(|mut s| s.take()));
            if let Some(st) = state {
                destroy_icons(st.icons);
                drop(st);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn key_down(key: u16) -> bool {
    unsafe { GetKeyState(i32::from(key)) < 0 }
}

/// Keyboard in the credential fields: Esc closes the window, Enter saves,
/// Tab moves to the other field, Ctrl+A selects all.
unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let parent = || unsafe { windows::Win32::UI::WindowsAndMessaging::GetParent(hwnd) }.ok();
    match msg {
        WM_KEYDOWN => {
            let key = (wparam.0 & 0xFFFF) as u16;
            let ctrl = key_down(VK_CONTROL.0);
            // The form's fields: Esc closes the form, Enter adds the tile.
            let id = unsafe { GetDlgCtrlID(hwnd) };
            let in_form = id == ID_TILE_NAME || id == ID_TILE_TARGET;
            if key == VK_ESCAPE.0 {
                if let Some(p) = parent() {
                    let (msg, arg) = if in_form {
                        (WM_APP_FORM, 1)
                    } else {
                        (WM_CLOSE, 0)
                    };
                    unsafe {
                        let _ = PostMessageW(Some(p), msg, WPARAM(arg), LPARAM(0));
                    }
                }
                return LRESULT(0);
            }
            if key == VK_RETURN.0 {
                if let Some(p) = parent() {
                    let msg = if in_form { WM_APP_FORM } else { WM_APP_SUBMIT };
                    unsafe {
                        let _ = PostMessageW(Some(p), msg, WPARAM(0), LPARAM(0));
                    }
                }
                return LRESULT(0);
            }
            if key == VK_TAB.0 {
                let other = with(|st| st.edits.as_ref().map(|e| e.partner(hwnd))).flatten();
                // Shift+Tab goes the same way: there are only two in a pair.
                if let Some(other) = other {
                    unsafe {
                        let _ = SetFocus(Some(other));
                    }
                }
                return LRESULT(0);
            }
            if key == VK_A.0 && ctrl {
                send(hwnd, EM_SETSEL, 0, -1);
                return LRESULT(0);
            }
        }
        WM_CHAR => {
            if matches!(wparam.0, 0x01 | 0x09 | 0x0A | 0x0D | 0x1B) {
                return LRESULT(0);
            }
        }
        WM_NCDESTROY => unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID);
        },
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// `--settings [section] [dark|light|system] [id|en]` (debug builds only):
/// the window on its own, with a hidden owner that applies each action to a
/// demo model and pushes it back, so every control works. Nothing is saved
/// and the island is not involved; the hook buttons and links are real, so
/// leave them alone. `DI_ACCENT`, `DI_SURFACE` (a base colour name such as
/// `terminal`; unset or unknown is Auto, which follows the cat; `terminal`
/// also gives the neutral dark window), `DI_GOOGLE` (`ok`, `needslogin`,
/// `reconnect`, `offline`, `none`), `DI_POMO_LOCKED=1`, `DI_HOTKEYS` (`failed`:
/// every hotkey that is set shows as taken by another app) and `DI_UPDATE`
/// (`available`, `checking`, `downloading`: Tentang's Pembaruan row, with a
/// made-up 9.9.9) and `DI_MONITORS` (`1`, `3`, `3-missing`: made-up screens for the
/// Notch card's Monitor row; unset lists the real ones) set the start.
///
/// The window's place is kept in `vale-settings-demo-window.json` in the temp
/// folder (the real config is never touched): read at the start (a missing
/// or bad file means the default place), written when the window is moved,
/// resized, maximized or closed. Close and reopen the demo to see the place
/// come back, or write an off-screen rectangle into the file to see the
/// fallback to the centre.
#[cfg(debug_assertions)]
pub fn run_demo(args: &[String]) {
    use crate::i18n;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage, WS_EX_TOOLWINDOW,
        WS_POPUP,
    };

    thread_local! {
        static DEMO: RefCell<Option<Model>> = const { RefCell::new(None) };
        /// `DI_HOTKEYS=failed`: every hotkey that is set shows as taken.
        static HOTKEYS_FAILED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// The failed flags for `keys`: every set row when `on`.
    fn demo_failed(keys: &Hotkeys, on: bool) -> [bool; HotkeyId::ALL.len()] {
        std::array::from_fn(|i| {
            on && HotkeyId::from_index(i).is_some_and(|id| keys.get(id).is_some())
        })
    }

    fn place_file() -> std::path::PathBuf {
        std::env::temp_dir().join("vale-settings-demo-window.json")
    }

    fn read_place() -> Option<WindowPlace> {
        let text = std::fs::read_to_string(place_file()).ok()?;
        crate::config::place_from_json(serde_json::from_str(&text).ok()?)
    }

    /// The screens the demo shows. `DI_MONITORS`: `1` (one 1920x1080), `3`
    /// (three side by side, the middle one primary, two with the same friendly
    /// name), `3-missing` (the same, with a saved choice that is not
    /// connected); anything else lists the real displays. The second value is
    /// the saved choice (Primary except for `3-missing`).
    fn demo_monitors(kind: &str) -> (Vec<MonitorRow>, NotchMonitor) {
        let row = |number: usize, x: i32, primary: bool, name: &str, path: &str| MonitorRow {
            number,
            name: name.into(),
            w: 1920,
            h: 1080,
            rect: arrange::Rect::new(x, 0, 1920, 1080),
            primary,
            path: path.into(),
            selectable: !path.is_empty(),
        };
        let three = || {
            vec![
                row(1, -1920, false, "DELL U2415", r"\\?\DISPLAY#DEL4123#demo1#{e6f07b5f}"),
                row(2, 0, true, "LG 27UL850", r"\\?\DISPLAY#GSM5B09#demo2#{e6f07b5f}"),
                row(3, 1920, false, "DELL U2415", r"\\?\DISPLAY#DEL4123#demo3#{e6f07b5f}"),
            ]
        };
        match kind.trim().to_ascii_lowercase().as_str() {
            "1" => (
                vec![row(1, 0, true, "Generic PnP Monitor", r"\\?\DISPLAY#DEMO#1#{e6f07b5f}")],
                NotchMonitor::Primary,
            ),
            "3" => (three(), NotchMonitor::Primary),
            "3-missing" => (
                three(),
                NotchMonitor::Chosen {
                    path: r"\\?\DISPLAY#SAM0F9C#gone#{e6f07b5f}".into(),
                    name: "Samsung Odyssey G7".into(),
                },
            ),
            _ => (
                crate::arrange_win::displays()
                    .iter()
                    .map(MonitorRow::from_display)
                    .collect(),
                NotchMonitor::Primary,
            ),
        }
    }

    fn apply(m: &mut Model, action: Action) {
        match action {
            Action::NotchMonitor(v) => m.notch_monitor = v,
            // The real screens, whatever list the demo shows.
            Action::NotchIdentify => {
                crate::identify_overlay::show(&crate::arrange_win::displays(), true);
            }
            Action::WindowPlace(p) => {
                m.window_place = Some(p);
                if let Ok(json) = serde_json::to_string(&p) {
                    let _ = std::fs::write(place_file(), json);
                }
            }
            Action::Style(v) => m.style = v,
            Action::Accent(v) => m.accent = v,
            Action::Surface(v) => m.surface = v,
            Action::Font(v) => m.font = v,
            Action::NotchSize { w, h, .. } => (m.notch_width, m.notch_height) = (w, h),
            Action::NotchOpacity { pct, .. } => m.notch_opacity = pct,
            Action::NotchFill(v) => m.notch_fill = v,
            Action::NotchReset => {
                m.notch_width = crate::ui::geometry::NOTCH_W_DEFAULT;
                m.notch_height = crate::ui::geometry::NOTCH_H_DEFAULT;
            }
            Action::WindowMode(v) => m.window = v,
            Action::Lang(v) => {
                m.lang = v;
                i18n::set_lang(v);
            }
            Action::Autostart(v) => m.autostart = v,
            Action::IdleClock(v) => m.idle_clock = v,
            Action::EqFollowAudio(v) => m.eq_follow_audio = v,
            Action::StayBehind(v) => m.stay_behind = v,
            Action::Lyrics(v) => m.lyrics = v,
            Action::Shell(v) => m.shell = v,
            Action::CliStart(v) => m.cli_start = v,
            Action::OpenMode(v) => m.open_mode = v,
            Action::WidgetsShow(v) => m.widgets.show = v,
            Action::WidgetsLayer(v) => m.widgets.layer = v,
            Action::WidgetsFullscreen(v) => m.widgets.fullscreen = v,
            Action::WidgetsLock(v) => m.widgets.lock = v,
            Action::WidgetsPeek(v) => m.widgets.cat_peek = v,
            Action::WidgetsTilt(v) => m.widgets.tilt = v,
            Action::WidgetsClipFill(v) => m.widgets.clip_fill = v,
            Action::WidgetAdd(_) => m.sticky_count = (m.sticky_count + 1).min(crate::widgets::sticky::MAX_STICKY),
            Action::WidgetHighlight => {}
            Action::Robot(v) => m.robot = v,
            Action::RobotMotion(v) => m.robot_motion = v,
            Action::RobotSleep(v) => m.robot_sleep = v,
            Action::Greeting(v) => m.greeting = v,
            Action::CatColor(v) => m.cat_color = v,
            Action::CatAccessory(v) => m.cat_accessory = v,
            Action::CatFollow(v) => m.cat_follow = v,
            Action::NotchPeek(v) => m.notch_peek = v,
            Action::CatDance(v) => m.cat_dance = v,
            Action::ClipEnabled(v) => m.clip.enabled = v,
            Action::ClipItems(v) => m.clip.max_items = v,
            Action::ClipPersist(v) => m.clip.persist = v,
            Action::ClipClearOnLock(v) => m.clip.clear_on_lock = v,
            Action::DeviceNotices(v) => m.device_notices = v,
            Action::Hotkey(id, value) => {
                // What the island does: checked again, then set.
                let fine = value.is_none_or(|hk| {
                    crate::hotkey::check(hk).is_ok()
                        && crate::hotkey::used_by(&m.hotkeys, id, hk).is_none()
                });
                if fine {
                    m.hotkeys.set(id, value);
                }
                m.hotkey_failed = demo_failed(&m.hotkeys, HOTKEYS_FAILED.get());
            }
            Action::HotkeysResetAll => {
                m.hotkeys.reset_all();
                m.hotkey_failed = demo_failed(&m.hotkeys, HOTKEYS_FAILED.get());
            }
            Action::HotkeyRecording(_) => {}
            Action::ArrangeGap { gap, .. } => m.arrange.gap = gap,
            Action::ArrangeTemplates(list) => m.arrange.templates = list,
            Action::ArrangeSaved(list) => m.arrange.saved = list,
            Action::ClipNotice(v) => m.clip.copy_notice = v,
            Action::ClipFeedbackAll(v) => m.clip.copied_feedback_all = v,
            Action::ClipClear => {}
            Action::NotifRecord(v) => m.notif.record = v,
            Action::NotifItems(v) => m.notif.max_items = v,
            Action::NotifPersist(v) => m.notif.persist = v,
            Action::NotifMutedAdd(name) => {
                if let Some(list) = crate::config::with_muted(&m.muted, &name) {
                    m.muted = list;
                }
            }
            Action::NotifMutedRemove(name) => {
                if let Some(list) = crate::config::without_muted(&m.muted, &name) {
                    m.muted = list;
                }
            }
            Action::NotifClear => {}
            Action::ShotTool(v) => m.shot.tool = v,
            Action::ShotAfterCapture(v) => m.shot.after_capture = v,
            Action::NotifToastStyle(v) => m.notif.toast_style = v,
            Action::ShotPreview(v) => m.shot.preview = v,
            Action::ShotAutoCopy(v) => m.shot.auto_copy = v,
            Action::ShotOnIgnore(v) => m.shot.on_ignore = v,
            Action::ShotSecs(v) => m.shot.show_secs = v,
            Action::BatteryThreshold(v) => m.battery_threshold = v,
            Action::DiskWarn(v) => m.disk_warn_gb = v,
            Action::RamWarn(v) => m.ram_warn_percent = v,
            Action::MoleProtect(v) => m.mole_protect = v,
            Action::PanelPages(v) => m.pages = v,
            Action::ShortcutTiles(v) => m.tiles = v,
            Action::Pomo(field, dir) => {
                let (min, max, step) = crate::pomodoro::range(field);
                let v = match field {
                    PomoField::Focus => &mut m.pomodoro.focus_min,
                    PomoField::ShortBreak => &mut m.pomodoro.short_break_min,
                    PomoField::LongBreak => &mut m.pomodoro.long_break_min,
                    PomoField::Sessions => &mut m.pomodoro.sessions,
                };
                *v = if dir > 0 {
                    v.saturating_add(step)
                } else {
                    v.saturating_sub(step)
                }
                .clamp(min, max);
            }
            Action::SaveGoogle { id, secret } => {
                if m.calendar == CalStatus::NotConfigured && !id.is_empty() {
                    m.calendar = CalStatus::NeedsLogin;
                }
                m.google_client_id = id;
                m.google_client_secret = secret;
            }
            Action::ConnectGoogle => m.calendar = CalStatus::Ok,
            Action::DisconnectGoogle => m.calendar = CalStatus::NeedsLogin,
            Action::UpdateAuto(v) => m.update_auto = v,
            // No network and no install in the demo; the row state is set by
            // `DI_UPDATE`.
            Action::CheckUpdates | Action::InstallUpdate => {}
            Action::DndHelp | Action::Closed => {}
        }
        theme::set_current(theme::Theme {
            accent: m.accent,
            surface: m.surface.resolve(m.cat_color, m.robot),
            window: m.window,
        });
    }

    unsafe extern "system" fn owner_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_APP_SETTINGS_ACTION {
            if let Some(action) = unsafe { take_action(lparam) } {
                println!("settings-demo: {action:?}");
                if action == Action::Closed {
                    unsafe { PostQuitMessage(0) };
                    return LRESULT(0);
                }
                let model = DEMO.with_borrow_mut(|m| {
                    m.as_mut().map(|m| {
                        apply(m, action);
                        m.clone()
                    })
                });
                if let Some(model) = model {
                    update(model);
                }
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    // The "Pilih…" dialog wants a single-threaded apartment, as the island's.
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
        );
    }
    let section = args.first().and_then(|s| Section::from_name(s));
    let window = args
        .get(1)
        .and_then(|s| WindowTheme::from_name(s))
        .unwrap_or_default();
    let lang = args
        .get(2)
        .and_then(|s| Lang::from_code(s))
        .unwrap_or(i18n::lang());
    i18n::set_lang(lang);
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let accent = Accent::from_name(&env("DI_ACCENT")).unwrap_or_default();
    let calendar = match env("DI_GOOGLE").to_ascii_lowercase().as_str() {
        "none" => CalStatus::NotConfigured,
        "needslogin" => CalStatus::NeedsLogin,
        "reconnect" => CalStatus::Reconnect,
        "offline" => CalStatus::Offline,
        _ => CalStatus::Ok,
    };
    let configured = calendar != CalStatus::NotConfigured;
    HOTKEYS_FAILED.set(env("DI_HOTKEYS") == "failed");
    let mut cfg = Config::default();
    cfg.surface = Surface::from_name(&env("DI_SURFACE")).unwrap_or_default();
    if env("DI_TILES") == "1" {
        // Two own tiles among the built-in ones, as the Pintasan card shows.
        let own = |label: &str, kind: &str, target: &str| crate::tiles::TileEntry {
            tile: "Custom".into(),
            on: true,
            label: label.into(),
            kind: kind.into(),
            target: target.into(),
        };
        cfg.shortcut_tiles = vec![
            own("Gmail", "Url", "https://mail.google.com"),
            own("Proyek", "Folder", "D:/Proyek/rumah-kemang"),
        ];
    }
    // Susun: two templates and two saved arrangements to look at.
    let mut grid = Node::Leaf;
    grid.split(0, arrange::Dir::Cols);
    grid.split(1, arrange::Dir::Rows);
    cfg.arrange.templates = vec![
        CustomTemplate::new("t1", "Gambar + 2", grid).expect("a template"),
        CustomTemplate::new("t2", "Tiga kolom", arrange::BuiltIn::Cols3.node())
            .expect("a template"),
    ];
    let monitor = arrange::SavedMonitor {
        device: r"\\.\DISPLAY1".into(),
        rect: arrange::Rect::new(0, 0, 2560, 1440),
    };
    cfg.arrange.saved = vec![
        SavedArrangement::new(
            "Kerja",
            arrange::LayoutRef::BuiltIn(arrange::BuiltIn::Main2),
            monitor.clone(),
            vec![None, None, None],
        )
        .expect("saved"),
        SavedArrangement::new(
            "Rapat",
            arrange::LayoutRef::BuiltIn(arrange::BuiltIn::Cols2),
            monitor,
            vec![None, None],
        )
        .expect("saved"),
    ];
    // `DI_FONT=mono`: the UI typeface (unset or unknown reads Sans).
    let font = UiFont::from_name(&env("DI_FONT")).unwrap_or_default();
    let (monitors, notch_monitor) = demo_monitors(&env("DI_MONITORS"));
    let model = Model {
        font,
        monitors,
        notch_monitor,
        accent,
        window,
        lang,
        pomodoro_locked: env("DI_POMO_LOCKED") == "1",
        lyrics: env("DI_LYRICS") == "1",
        // Every hotkey that is set, taken by another app (the red note).
        hotkey_failed: demo_failed(&Hotkeys::default(), HOTKEYS_FAILED.get()),
        calendar,
        window_place: read_place(),
        // The Pembaruan row's state, with a made-up version; nothing touches
        // the network.
        update_row: match env("DI_UPDATE").to_ascii_lowercase().as_str() {
            "available" => UpdateRow::Available("9.9.9".into()),
            "checking" => UpdateRow::Checking,
            "downloading" => UpdateRow::Downloading(42),
            _ => UpdateRow::Idle,
        },
        // A build with Vale's own client signs in without one.
        google_client_id: if configured && !client::has_built_in() {
            "123456789012-demo.apps.googleusercontent.com".into()
        } else {
            String::new()
        },
        google_client_secret: if configured && !client::has_built_in() {
            "demo-secret".into()
        } else {
            String::new()
        },
        ..Model::from_config(&cfg)
    };
    theme::set_system_light(theme::read_system_light());
    theme::set_current(theme::Theme {
        accent,
        surface: model.surface.resolve(model.cat_color, model.robot),
        window,
    });
    DEMO.with_borrow_mut(|m| *m = Some(model.clone()));
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let instance: HINSTANCE = module.into();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(owner_proc),
            hInstance: instance,
            lpszClassName: w!("ValeSettingsDemoOwner"),
            ..Default::default()
        };
        RegisterClassExW(&class);
        let Ok(owner) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("ValeSettingsDemoOwner"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        ) else {
            eprintln!("settings-demo: cannot create the owner window");
            return;
        };
        match open(owner, model, section) {
            Ok(hwnd) => {
                println!("settings-demo: hwnd {:#x}", hwnd.0 as usize);
                // `DI_ARRANGE_EDIT=new` opens Susun's template editor,
                // `DI_SETTINGS_SCROLL=<px>` scrolls the section.
                let editor = std::env::var("DI_ARRANGE_EDIT").unwrap_or_default();
                let scroll: f32 = std::env::var("DI_SETTINGS_SCROLL")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                with(|st| {
                    if editor == "new" {
                        let mut root = Node::Leaf;
                        root.split(0, arrange::Dir::Cols);
                        root.split(1, arrange::Dir::Rows);
                        st.arrange_edit = Some(ArrangeEdit {
                            target: EditTarget::New,
                            root,
                            selected: 1,
                            drag: None,
                        });
                    }
                    st.scroll = scroll;
                    st.relayout();
                });
                relayout_and_place();
            }
            Err(error) => {
                eprintln!("settings-demo: open failed: {error}");
                return;
            }
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = DestroyWindow(owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Page;

    fn hk(text: &str) -> Hotkey {
        crate::hotkey::parse(text).unwrap().unwrap()
    }

    #[test]
    fn the_title_bar_restyles_when_the_base_colour_moves_not_only_the_mode() {
        use crate::theme::{Accent, Surface, WindowTheme, window_for};
        let pal = |s, mode| window_for(Accent::Terracotta, s, mode, false);
        let warm = pal(Surface::Espresso, WindowTheme::Dark);
        let neutral = pal(Surface::Terminal, WindowTheme::Dark);
        let light = pal(Surface::Terminal, WindowTheme::Light);
        let key = |p: &WindowPalette| (p.light, p.bg, p.text);
        // First styling: everything applies.
        assert_eq!(style_changes(None, &warm), (true, true));
        // Same palette: nothing.
        assert_eq!(style_changes(Some(key(&warm)), &warm), (false, false));
        // Warm dark to Terminal's dark: the caption moves, the fields' theme not.
        assert_eq!(style_changes(Some(key(&warm)), &neutral), (true, false));
        assert_eq!(style_changes(Some(key(&neutral)), &warm), (true, false));
        // Dark to light: both.
        assert_eq!(style_changes(Some(key(&neutral)), &light), (true, true));
        // An accent change alone leaves the title bar alone.
        let other = window_for(Accent::Sage, Surface::Terminal, WindowTheme::Dark, false);
        assert_eq!(style_changes(Some(key(&neutral)), &other), (false, false));
    }

    #[test]
    fn the_recorders_message_id_is_unique_among_the_windows_messages() {
        assert_eq!(WM_APP_HOTKEY_REC, WM_APP + 13);
        assert_eq!(WM_APP_HOTKEY_REC_DONE, WM_APP + 14);
        let ids = [
            ("settings action", WM_APP_SETTINGS_ACTION),
            ("submit", WM_APP_SUBMIT),
            ("place", WM_APP_PLACE),
            ("form", WM_APP_FORM),
            ("frame clock", WM_APP_FRAME),
            ("recorder", WM_APP_HOTKEY_REC),
            ("recorder done", WM_APP_HOTKEY_REC_DONE),
        ];
        for (i, (name, id)) in ids.iter().enumerate() {
            for (other, other_id) in &ids[i + 1..] {
                assert_ne!(id, other_id, "{name} and {other}");
            }
        }
    }

    #[test]
    fn the_island_gets_its_hotkeys_back_only_once_the_recorder_let_go() {
        // A combo while the hook drains: nothing yet, then its end.
        let mut r = Resume::default();
        assert!(!r.end(true));
        assert!(!r.hook_done(true), "still draining");
        assert!(r.hook_done(false));
        assert!(!r.hook_done(false), "only once");
        // The hook already gone (it ended first, or the fallback): at once.
        let mut r = Resume::default();
        assert!(r.end(false));
        assert!(!r.hook_done(false));
        // A new recording while the old hook drained: the old one's end is
        // stale, the new one's end decides.
        let mut r = Resume::default();
        assert!(!r.end(true));
        r.start();
        assert!(!r.hook_done(false));
        assert!(!r.end(true));
        assert!(r.hook_done(false));
    }

    #[test]
    fn a_recorded_step_ends_the_recording_with_the_action_and_the_message_it_should() {
        let rows = Hotkeys::default();
        let id = HotkeyId::Clipboard;
        let end = |post, error| RecOutcome::End { post, error };
        // Modifiers alone: keep listening.
        assert_eq!(rec_outcome(&rows, id, Step::Ignore), RecOutcome::Listen);
        // Esc: nothing posted, nothing said. Delete or Backspace: off.
        assert_eq!(rec_outcome(&rows, id, Step::Cancel), end(None, None));
        assert_eq!(rec_outcome(&rows, id, Step::Clear), end(Some(None), None));
        // A good combo is posted.
        let good = hk("Ctrl+Alt+V");
        assert_eq!(
            rec_outcome(&rows, id, Step::Combo(good)),
            end(Some(Some(good)), None)
        );
        // F13 bare is fine too.
        assert_eq!(
            rec_outcome(&rows, id, Step::Combo(hk("F13"))),
            end(Some(Some(hk("F13"))), None)
        );
        // Windows' combos, a bare letter and a Shift-only one are refused.
        assert_eq!(
            rec_outcome(&rows, id, Step::Combo(hk("Alt+Tab"))),
            end(None, Some(RejectMsg::Reserved))
        );
        assert_eq!(
            rec_outcome(&rows, id, Step::Combo(hk("Ctrl+Shift+V"))),
            end(None, Some(RejectMsg::Reserved))
        );
        for text in ["K", "Shift+K"] {
            assert_eq!(
                rec_outcome(&rows, id, Step::Combo(hk(text))),
                end(None, Some(RejectMsg::NeedsModifier)),
                "{text}"
            );
        }
        // A combo another row has names that row; the row's own is fine.
        assert_eq!(
            rec_outcome(&rows, id, Step::Combo(hk("Ctrl+Alt+Space"))),
            end(None, Some(RejectMsg::UsedBy(HotkeyId::Search)))
        );
        assert_eq!(
            rec_outcome(&rows, HotkeyId::Search, Step::Combo(hk("Ctrl+Alt+Space"))),
            end(Some(Some(hk("Ctrl+Alt+Space"))), None)
        );
        assert_eq!(
            rec_outcome(&rows, HotkeyId::Devices, Step::Combo(hk("PrtSc"))),
            end(None, Some(RejectMsg::UsedBy(HotkeyId::Screenshot)))
        );
        // Esc, Backspace and Delete with a modifier are combos like the rest.
        assert_eq!(
            rec_outcome(&rows, id, Step::Combo(hk("Ctrl+Delete"))),
            end(Some(Some(hk("Ctrl+Delete"))), None)
        );
        assert_eq!(
            rec_outcome(&rows, id, Step::Combo(hk("Ctrl+Esc"))),
            end(None, Some(RejectMsg::Reserved))
        );
    }

    #[test]
    fn the_name_field_takes_an_app_name_in_fitur_and_a_label_elsewhere() {
        for section in Section::ALL {
            let want = if section == Section::Features {
                crate::config::MUTED_NAME_MAX
            } else {
                LABEL_MAX
            };
            assert_eq!(name_limit(section), want, "{section:?}");
        }
        assert!(name_limit(Section::Features) >= "Microsoft Teams (work or school)".len());
    }

    #[test]
    fn the_name_field_is_the_template_name_only_while_the_editor_is_open_on_fitur() {
        assert_eq!(name_role(Section::Features, true), NameRole::Template);
        assert_eq!(name_role(Section::Features, false), NameRole::MutedApp);
        // An editor state carried to another section never takes over the
        // shortcut form's Enter, Esc, hint or length.
        for section in Section::ALL {
            if section != Section::Features {
                assert_eq!(name_role(section, true), NameRole::TileLabel, "{section:?}");
                assert_eq!(
                    name_role(section, false),
                    NameRole::TileLabel,
                    "{section:?}"
                );
            }
        }
    }

    #[test]
    fn page_clicks_give_the_changed_pages_or_nothing() {
        let pages = PanelPages::default();
        let off = page_action(&pages, Hit::PageOn(Page::Notes)).expect("toggled");
        assert!(!off.is_on(Page::Notes));
        let later = page_action(&pages, Hit::PageMove(Page::Notes, 1)).expect("moved");
        assert_eq!(later.position(Page::Notes), 4);
        // Nothing changes: no action.
        assert_eq!(page_action(&pages, Hit::PageMove(Page::Calendar, -1)), None);
        let mut one = PanelPages::default();
        for p in &Page::ALL[1..] {
            one.toggle(*p);
        }
        assert_eq!(page_action(&one, Hit::PageOn(Page::Calendar)), None);
        assert_eq!(page_action(&pages, Hit::Robot), None);
    }

    #[test]
    fn page_clicks_move_among_the_pages_shown_and_restore_the_defaults() {
        let mut pages = PanelPages::default();
        pages.toggle(Page::Reminders);
        // Pomodoro down: past the hidden Reminders, after Notes.
        let moved = page_action(&pages, Hit::PageMove(Page::Pomodoro, 1)).expect("moved");
        assert_eq!(
            moved.visible()[..4],
            [Page::Calendar, Page::Notes, Page::Pomodoro, Page::Media]
        );
        assert_eq!(page_action(&pages, Hit::PageMove(Page::Calendar, -1)), None);
        // "Tampilkan" on a hidden page, "Kembalikan bawaan" on the changed one.
        let back = page_action(&pages, Hit::PageOn(Page::Reminders)).expect("shown");
        assert_eq!(back, PanelPages::default());
        assert_eq!(
            page_action(&moved, Hit::PageReset),
            Some(PanelPages::default())
        );
        assert_eq!(page_action(&PanelPages::default(), Hit::PageReset), None);
        // The handle is not a click.
        assert_eq!(page_action(&pages, Hit::PageGrip(Page::Notes)), None);
    }

    #[test]
    fn a_drop_puts_the_page_at_the_place_it_was_dropped_on() {
        let mut pages = PanelPages::default();
        pages.toggle(Page::Reminders);
        // Shown: Calendar, Pomodoro, Notes, Media, ... Dropping Calendar on
        // the third place (Notes) puts it after Notes.
        let next = page_drop(&pages, 0, 2).expect("moved");
        assert_eq!(
            next.visible()[..4],
            [Page::Pomodoro, Page::Notes, Page::Calendar, Page::Media]
        );
        // Up: Media dropped on the first place goes before Calendar.
        let next = page_drop(&pages, 3, 0).expect("moved");
        assert_eq!(next.visible()[0], Page::Media);
        assert_eq!(page_drop(&pages, 2, 2), None, "same place");
        assert_eq!(page_drop(&pages, 0, 40), None, "no such place");
        assert_eq!(page_drop(&pages, 40, 0), None);
    }

    #[test]
    fn tile_clicks_change_the_list_or_nothing() {
        let mut tiles = ShortcutTiles::default();
        // The Calendar on too: twelve show, the page is full.
        let calendar = 9;
        assert!(tiles.toggle(calendar));
        // Screenshot hidden, then back.
        let off = tile_action(&tiles, Hit::TileOn(0)).expect("hidden");
        assert_eq!(off.shown(), tiles.shown() - 1);
        assert_eq!(tile_action(&off, Hit::TileOn(0)), Some(tiles.clone()));
        // Down among the tiles shown, never past the last.
        let moved = tile_action(&tiles, Hit::TileMove(0, 1)).expect("moved");
        assert_eq!(moved.visible()[1], tiles.visible()[0]);
        assert_eq!(tile_action(&tiles, Hit::TileMove(0, -1)), None);
        // "+ Tambah" on a tile that is off, refused while twelve show.
        assert_eq!(tile_action(&tiles, Hit::TileOn(10)), None);
        assert_eq!(tile_action(&tiles, Hit::TileOn(12)), None);
        let room = tile_action(&tiles, Hit::TileOn(0)).expect("hidden");
        let added = tile_action(&room, Hit::TileOn(10)).expect("shown");
        assert_eq!(added.shown(), room.shown() + 1);
        // "Hapus" only on an own tile; "Kembalikan bawaan" on a changed list.
        assert_eq!(tile_action(&tiles, Hit::TileRemove(0)), None);
        // Reset: nothing to do on the default, the default back from a change.
        let defaults = ShortcutTiles::default();
        assert_eq!(tile_action(&defaults, Hit::TileReset), None);
        assert_eq!(tile_action(&tiles, Hit::TileReset), Some(defaults.clone()));
        assert_eq!(tile_action(&moved, Hit::TileReset), Some(defaults));
        assert_eq!(tile_action(&tiles, Hit::TileGrip(0)), None);
        assert_eq!(tile_action(&tiles, Hit::Robot), None);
    }

    #[test]
    fn a_drop_puts_the_tile_at_the_place_it_was_dropped_on() {
        let tiles = ShortcutTiles::default();
        let names = |t: &ShortcutTiles| t.visible();
        let before = names(&tiles);
        // Shown places 0..10: Screenshot dropped on the third (Search).
        let next = tile_drop(&tiles, 0, 2).expect("moved");
        let after = names(&next);
        assert_eq!(
            after[..3],
            [before[1].clone(), before[2].clone(), before[0].clone()]
        );
        // The last shown (the Shelf, far down the list of every tile) goes
        // to the front.
        let next = tile_drop(&tiles, before.len() - 1, 0).expect("moved");
        assert_eq!(names(&next)[0], before[before.len() - 1]);
        assert_eq!(tile_drop(&tiles, 3, 3), None);
        assert_eq!(tile_drop(&tiles, 0, 99), None);
    }

    #[test]
    fn adding_an_own_tile_checks_what_was_typed() {
        // Eleven show: the page has room for one own tile.
        let tiles = ShortcutTiles::default();
        let add =
            |t: &ShortcutTiles, kind, name: &str, target: &str| add_tile_to(t, kind, name, target);
        let (next, on) = add(
            &tiles,
            CustomKind::Url,
            " Gmail ",
            "https://mail.google.com",
        )
        .expect("valid");
        assert!(on);
        assert_eq!(next.customs(), 1);
        assert!(next.entries().last().unwrap().1);
        // What is wrong is told per part, and by kind.
        let key = |r: std::result::Result<(ShortcutTiles, bool), Key>| r.map(|_| ()).unwrap_err();
        assert_eq!(
            key(add(&tiles, CustomKind::Url, "", "https://x.co")),
            Key::TileErrName
        );
        assert_eq!(
            key(add(
                &tiles,
                CustomKind::Url,
                "Nama yang terlalu panjang",
                "https://x.co"
            )),
            Key::TileErrName
        );
        assert_eq!(
            key(add(&tiles, CustomKind::Url, "X", "mail.google.com")),
            Key::TileErrUrl
        );
        assert_eq!(
            key(add(&tiles, CustomKind::App, "X", "calc.exe")),
            Key::TileErrPath
        );
        assert_eq!(
            key(add(&tiles, CustomKind::Folder, "X", r"\\server\share")),
            Key::TileErrPath
        );
        // Added off while twelve show; refused when eight own tiles exist.
        let (full, _) = add(&tiles, CustomKind::App, "Satu", r"C:\a.exe").expect("valid");
        assert!(full.full());
        let (_, on) = add(&full, CustomKind::Folder, "Dua", r"D:\Proyek").expect("valid");
        assert!(!on, "the page is full: off");
        let mut many = tiles.clone();
        for i in 0..crate::tiles::MAX_CUSTOM {
            many = add(&many, CustomKind::App, &format!("T{i}"), r"C:\a.exe")
                .expect("valid")
                .0;
        }
        assert_eq!(
            key(add(&many, CustomKind::App, "Lagi", r"C:\a.exe")),
            Key::TileOwnFull
        );
    }

    #[test]
    fn the_held_row_follows_the_pointer_within_the_list() {
        let tops = [100.0, 152.0, 204.0, 256.0];
        // The pointer took the row 10 px under its top (second row).
        let grab = 10.0;
        // Unscrolled: the row's top is the pointer minus the grab.
        assert_eq!(drag_place(&tops, grab, 172.0, 0.0), (162.0, 1));
        // Its middle (top + 26) is what picks the slot.
        assert_eq!(drag_place(&tops, grab, 200.0, 0.0), (190.0, 2));
        // Scrolling moves the content under a still pointer: the row goes
        // down the list with it.
        assert_eq!(drag_place(&tops, grab, 172.0, 52.0), (214.0, 2));
        assert_eq!(drag_place(&tops, grab, 172.0, 104.0), (256.0, 3));
        // Kept between the first and the last row.
        assert_eq!(drag_place(&tops, grab, -500.0, 0.0), (100.0, 0));
        assert_eq!(drag_place(&tops, grab, 9000.0, 0.0), (256.0, 3));
        // One row: it stays.
        assert_eq!(drag_place(&[100.0], grab, 500.0, 0.0), (100.0, 0));
        assert_eq!(drag_place(&[], grab, 500.0, 0.0), (0.0, 0));
    }

    #[test]
    fn tweens_ease_toward_the_target_and_retarget_smoothly() {
        let t0 = Instant::now();
        let t = Tween::toward(None, 0.0, 2.0, t0);
        assert_eq!(t.value(t0), 0.0);
        assert_eq!(t.value(t0 + ANIM), 2.0);
        assert!(t.running(t0 + ANIM / 2));
        assert!(!t.running(t0 + ANIM));
        // Retargeting mid-way starts from where it is, not from the old start.
        let mid = t0 + ANIM / 2;
        let back = Tween::toward(Some(t), 2.0, 0.0, mid);
        assert!((back.value(mid) - t.value(mid)).abs() < 1e-6);
    }

    #[test]
    fn model_from_config_copies_the_settings() {
        let cfg = Config {
            style: Style::Notch,
            accent: Accent::Clay,
            battery_threshold: 35,
            google_client_id: "id".into(),
            idle_clock: false,
            lyrics: true,
            ..Config::default()
        };
        let m = Model::from_config(&cfg);
        assert_eq!(
            (
                m.style,
                m.accent,
                m.battery_threshold,
                m.google_client_id.as_str(),
                m.idle_clock,
                m.lyrics
            ),
            (Style::Notch, Accent::Clay, 35, "id", false, true)
        );
        assert_eq!(m.window_place, None);
        let place = WindowPlace {
            left: 1,
            top: 2,
            right: 900,
            bottom: 700,
            maximized: true,
        };
        let cfg = Config {
            settings_window: Some(place),
            ..Config::default()
        };
        assert_eq!(Model::from_config(&cfg).window_place, Some(place));
    }

    #[test]
    fn actions_round_trip_through_lparam() {
        let action = Action::SaveGoogle {
            id: "a".into(),
            secret: "b".into(),
        };
        let lparam = LPARAM(Box::into_raw(Box::new(action.clone())) as isize);
        assert_eq!(unsafe { take_action(lparam) }, Some(action));
        assert_eq!(unsafe { take_action(LPARAM(0)) }, None);
    }
}
