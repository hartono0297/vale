use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::io;
use std::path::{Path, PathBuf};

use crate::arrange::ArrangeSettings;
use crate::clean::CleanMode;
use crate::cli::Shell;
use crate::cli_folders::CliStart;
use crate::greeting::GreetMark;
use crate::i18n::Lang;
use crate::pages::PageEntry;
use crate::pomodoro::PomoSettings;
use crate::theme::{Accent, CatAccessory, CatColor, Surface, WindowTheme};
use crate::tiles::TileEntry;
use crate::ui::geometry::{
    NOTCH_H_DEFAULT, NOTCH_H_MAX, NOTCH_H_MIN, NOTCH_OPACITY_DEFAULT, NOTCH_W_DEFAULT, NOTCH_W_MAX,
    NOTCH_W_MIN,
};
use crate::ui::robot::{Motion, clamp_sleep};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Pill,
    Notch,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub style: Style,
    pub battery_threshold: u8,
    pub start_with_windows: bool,
    /// Apps whose toasts never reach the notch or the history (a case-
    /// insensitive part of the app's name). Owned by the file; the settings
    /// window and the Notifikasi page edit it through `save_merged_with`.
    #[serde(deserialize_with = "muted_apps")]
    pub muted_apps: Vec<String>,
    pub onboarding_done: bool,
    pub pomodoro: PomoSettings,
    /// OAuth desktop-client credentials for Google Calendar; empty until the user sets them.
    pub google_client_id: String,
    pub google_client_secret: String,
    /// UI language, "id" or "en".
    pub language: Lang,
    pub accent: Accent,
    /// The island's fill; the island itself is always dark.
    pub surface: Surface,
    /// The closed island's width in logical px, kept in `NOTCH_W_MIN..=NOTCH_W_MAX`
    /// (Settings -> Tampilan -> Notch).
    #[serde(deserialize_with = "notch_width")]
    pub notch_width: u32,
    /// The closed island's height, kept in `NOTCH_H_MIN..=NOTCH_H_MAX`.
    #[serde(deserialize_with = "notch_height")]
    pub notch_height: u32,
    /// "Isi notch": a solid black closed island or a see-through one (a typo
    /// reads as Solid). The open panel is always solid.
    pub notch_fill: NotchFill,
    /// How opaque the see-through fill is, in percent (0..=100, default 30).
    #[serde(deserialize_with = "notch_opacity")]
    pub notch_opacity: u8,
    /// Editors and settings window: follow Windows, or always dark / light.
    pub window_theme: WindowTheme,
    /// The idle island shows the local time instead of an empty shape.
    pub idle_clock: bool,
    /// Experimental: the island sits below ordinary windows and comes up when
    /// it has something to show or the pointer touches the top edge over it.
    pub stay_behind: bool,
    /// While music plays, the compact island shows the current synced lyric
    /// line (looked up at lrclib.net) instead of the artist.
    pub lyrics: bool,
    /// The shell the CLI page runs commands in.
    pub cli_shell: Shell,
    /// Where the CLI page starts when it opens: the last active Explorer
    /// window's folder, or the last folder (by name; a typo is Explorer).
    pub cli_start: CliStart,
    /// "Mode buka": how the pointer opens the panel (by name; a typo is
    /// Hover).
    pub open_mode: OpenMode,
    /// Swipe, sideways wheel and middle button on the island while music
    /// plays (`gestures`): on unless switched off here.
    pub gestures: bool,
    /// The wheel over the compact music notch (and the Music page's cover)
    /// turns the volume 2 % a notch (`scroll_volume`): on unless off here.
    pub scroll_volume: bool,
    /// Bata, the robot, beside the clock and on the island's own notices.
    pub robot: bool,
    /// How much Bata moves: Full, Saver or Off (by name; a typo is Full).
    pub robot_animation: Motion,
    /// Bata falls asleep after this many minutes without activity (1–60).
    #[serde(deserialize_with = "sleep_minutes")]
    pub robot_sleep_minutes: u32,
    /// The cat's greeting by time of day: once per period per day, with a
    /// summary of the events and reminders that fit the period.
    pub morning_greeting: bool,
    /// The day and period the greeting last showed, "2026-10-31 evening". A
    /// plain old "2026-10-27" reads as that day's Morning; an unreadable
    /// value reads as none.
    #[serde(deserialize_with = "crate::greeting::lenient_mark")]
    pub last_greeting: Option<GreetMark>,
    /// Hidden, set by hand: write the media diagnosis log (`media_log`),
    /// like `--media-log`. Not in the settings window, and not written to
    /// the file while off.
    #[serde(skip_serializing_if = "is_false")]
    pub media_log: bool,
    /// The low-disk notice: a fixed drive with less than this many GB free
    /// (`DISK_WARN_RANGE`). Owned by the file like the battery threshold.
    #[serde(deserialize_with = "disk_warn")]
    pub disk_warn_gb: u32,
    /// The RAM notice: used memory above this percent for a minute
    /// (`RAM_WARN_RANGE`).
    #[serde(deserialize_with = "ram_warn")]
    pub ram_warn_percent: u8,
    /// The cleaner's mode last used: "Own" (Pilih sendiri, the default) or
    /// "Mole" (by name; a typo is Own).
    pub clean_mode: CleanMode,
    /// Before Mole's first preview, add the Cargo and Autodesk caches to
    /// Mole's whitelist (Settings → Fitur → Pembersih).
    pub mole_protect_defaults: bool,
    /// That was done once: an entry the user removes later is never added
    /// back.
    pub mole_defaults_added: bool,
    /// The clipboard history and its page.
    pub clipboard: ClipboardSettings,
    /// The screenshot preview in the notch (Settings → Clipboard, its
    /// second card).
    pub screenshot: ScreenshotSettings,
    /// "Halaman panel": the pages' order and which the wheel shows. Empty
    /// means the default (every page in `Page::ALL`); `pages::PanelPages` fills in the rest.
    #[serde(deserialize_with = "panel_pages")]
    pub panel_pages: Vec<PageEntry>,
    /// "Pintasan": the tiles' order, which show, and the custom ones. Empty
    /// means the eight built-ins; `tiles::ShortcutTiles` checks and fills in.
    #[serde(deserialize_with = "shortcut_tiles")]
    pub shortcut_tiles: Vec<TileEntry>,
    /// A notice when a flash drive or disk is plugged in, and when Vale's
    /// Eject says it is safe to remove (Settings → Fitur → Perangkat). On
    /// unless switched off here.
    pub device_notices: bool,
    /// "Warna" (Settings -> Kucing): the notch cat's coat. A bad value reads
    /// as Terracotta.
    #[serde(deserialize_with = "cat_color")]
    pub cat_color: CatColor,
    /// "Aksesori": what the cat wears. A bad value reads as None.
    #[serde(deserialize_with = "cat_accessory")]
    pub cat_accessory: CatAccessory,
    /// "Ikuti kursor": the idle cat looks toward the pointer (default on).
    #[serde(deserialize_with = "default_on")]
    pub cat_follow_pointer: bool,
    /// "Joget ikut musik": the cat nods to the beat while music plays
    /// (default on).
    #[serde(deserialize_with = "default_on")]
    pub cat_dance: bool,
    /// "Mengecil jadi mata": the notch shrinks to the cat's eyes near the
    /// pointer over a window (default on).
    #[serde(deserialize_with = "default_on")]
    pub notch_peek: bool,
    /// "Equalizer ikut irama": the equalizer bars beside the song title
    /// follow the real sound (default on).
    #[serde(deserialize_with = "default_on")]
    pub eq_follow_audio: bool,
    /// The Notifikasi page's history (Settings → Fitur → Notifikasi).
    #[serde(deserialize_with = "notif_settings")]
    pub notifications: NotifSettings,
    /// Susun (Arrange): the hotkey, the gap, the user's templates and the
    /// saved arrangements ([`crate::arrange`]).
    #[serde(deserialize_with = "crate::arrange::lenient_settings")]
    pub arrange: ArrangeSettings,
    /// "Cek pembaruan otomatis" (Settings → Tentang): ask GitHub for a newer
    /// release about once a day (default on). A manual check ignores it.
    #[serde(deserialize_with = "default_on")]
    pub update_check: bool,
    /// "Tombol pintas": the global hotkeys (Settings -> Tombol pintas).
    /// Read leniently (`Hotkeys`'s deserializer never fails); an old file
    /// without the key gets the old presets as combos (`migrate_hotkeys`).
    /// Owned by the island, so a Settings choice sticks across saves.
    pub hotkeys: crate::hotkey::Hotkeys,
    /// No longer used: it once remembered the last version an automatic check
    /// offered. Every check that finds a newer release offers it now ("Nanti"
    /// only clears the notice). Kept so an old `config.json` with the key
    /// still loads and `merge_owned` keeps working; nothing reads or writes it.
    #[serde(deserialize_with = "lenient_string")]
    pub update_offered: String,
    /// Where the Settings window was last (screen coordinates), so it opens
    /// there again. Not part of `merge_owned`: it comes from the file on every
    /// save and only `save_config_with` writes it, so a hand edit survives.
    #[serde(deserialize_with = "window_place")]
    pub settings_window: Option<WindowPlace>,
    /// "Font" (Settings -> Tampilan -> Island): the typeface of the notch, the
    /// panel and Settings. Owned by the island. A bad value reads as Sans.
    #[serde(deserialize_with = "ui_font")]
    pub font: UiFont,
    /// "Monitor" (Settings -> Tampilan -> Notch, tray): the screen the island
    /// shows on. Owned by the island; a bad value reads as Primary.
    pub notch_monitor: NotchMonitor,
    /// Where the floating lyrics card was last (screen coordinates, physical
    /// px), so it opens there again. Owned by the island (`merge_owned`); a
    /// malformed value reads as None and never fails the rest of the file.
    #[serde(deserialize_with = "card_place")]
    pub lyrics_card: Option<CardPlace>,
    /// "Widget" (Settings -> Widget): where the desktop widgets sit and what
    /// they do. Owned by the island; read leniently, field by field.
    pub widgets: crate::widgets::prefs::WidgetPrefs,
    /// "Rekaman" (Settings -> Fitur): the sources, the folder and the one-time
    /// consent notice. Owned by the island; read leniently, field by field.
    pub recording: crate::recorder::prefs::RecordingPrefs,
}

/// The lyrics card's rectangle in screen coordinates, physical pixels.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardPlace {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// `lyrics_card` read leniently, like `settings_window`.
fn card_place<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CardPlace>, D::Error> {
    Ok(card_place_from_json(serde_json::Value::deserialize(d)?))
}

/// A `CardPlace` from a JSON value, or None when it is not well formed (a
/// missing or non-number edge, a width or height of 0 or less, a coordinate
/// beyond +-100000).
pub fn card_place_from_json(v: serde_json::Value) -> Option<CardPlace> {
    let p = serde_json::from_value::<CardPlace>(v).ok()?;
    let sane = [p.left, p.top, p.right, p.bottom]
        .iter()
        .all(|c| (-WINDOW_COORD_MAX..=WINDOW_COORD_MAX).contains(c))
        && p.right > p.left
        && p.bottom > p.top;
    sane.then_some(p)
}

/// Which screen the island shows on. `Primary` follows whichever monitor
/// Windows calls primary; `Chosen` names one by its device path (stable across
/// reboots and replugs, unlike the `\\.\DISPLAYn` name) with the friendly name
/// kept only to show while it is unplugged.
///
/// On disk: the string "primary", or `{"path": "...", "name": "..."}`. Reading
/// never fails: anything else is `Primary`, a missing or non-string `name` is "".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NotchMonitor {
    #[default]
    Primary,
    Chosen { path: String, name: String },
}

impl Serialize for NotchMonitor {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            NotchMonitor::Primary => s.serialize_str("primary"),
            NotchMonitor::Chosen { path, name } => {
                let mut map = s.serialize_map(Some(2))?;
                map.serialize_entry("path", path)?;
                map.serialize_entry("name", name)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for NotchMonitor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(NotchMonitor::from_value(&v))
    }
}

impl NotchMonitor {
    fn from_value(v: &serde_json::Value) -> NotchMonitor {
        let Some(obj) = v.as_object() else {
            return NotchMonitor::Primary;
        };
        match obj.get("path").and_then(|p| p.as_str()) {
            Some(path) if !path.is_empty() => NotchMonitor::Chosen {
                path: path.to_string(),
                name: obj
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_string(),
            },
            _ => NotchMonitor::Primary,
        }
    }
}

/// The Settings window's normal (restore) rectangle in screen coordinates,
/// physical pixels, and whether it was maximized.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPlace {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub maximized: bool,
}

/// No coordinate of a saved window is believable beyond this.
const WINDOW_COORD_MAX: i32 = 100_000;

/// `settings_window` read leniently: anything that is not a well-formed
/// place (a width or height of 0 or less, a coordinate beyond +-100000) is
/// None and never fails the rest of the file.
fn window_place<'de, D: Deserializer<'de>>(d: D) -> Result<Option<WindowPlace>, D::Error> {
    Ok(place_from_json(serde_json::Value::deserialize(d)?))
}

/// A `WindowPlace` from a JSON value, or None when it is not well formed
/// (see `window_place`).
pub fn place_from_json(v: serde_json::Value) -> Option<WindowPlace> {
    let p = serde_json::from_value::<WindowPlace>(v).ok()?;
    let sane = [p.left, p.top, p.right, p.bottom]
        .iter()
        .all(|c| (-WINDOW_COORD_MAX..=WINDOW_COORD_MAX).contains(c))
        && p.right > p.left
        && p.bottom > p.top;
    sane.then_some(p)
}

/// The apps muted until the owner says otherwise.
pub fn default_muted_apps() -> Vec<String> {
    vec!["Claude".into(), "Orca".into()]
}

/// The longest name Settings or the Notifikasi page adds to `muted_apps`.
pub const MUTED_NAME_MAX: usize = 64;

/// `muted_apps` read leniently: anything that is not a list is the default
/// (Claude, Orca); items that are not strings go; each name is trimmed;
/// empty names and repeats (any case) go.
fn muted_apps<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let Some(items) = v.as_array() else {
        return Ok(default_muted_apps());
    };
    let mut out: Vec<String> = Vec::new();
    for name in items.iter().filter_map(serde_json::Value::as_str) {
        let name = name.trim();
        if !name.is_empty() && !out.iter().any(|m| m.eq_ignore_ascii_case(name)) {
            out.push(name.to_owned());
        }
    }
    Ok(out)
}

/// `list` with `name` added: trimmed, at most `MUTED_NAME_MAX` characters,
/// not empty, and not there already (any case). `None` when nothing is added.
pub fn with_muted(list: &[String], name: &str) -> Option<Vec<String>> {
    let name = name.trim();
    let name = match name.char_indices().nth(MUTED_NAME_MAX) {
        Some((i, _)) => name[..i].trim_end(),
        None => name,
    };
    if name.is_empty() || list.iter().any(|m| m.to_lowercase() == name.to_lowercase()) {
        return None;
    }
    let mut out = list.to_vec();
    out.push(name.to_owned());
    Some(out)
}

/// `list` without `name` (any case, trimmed); `None` when it is not there.
pub fn without_muted(list: &[String], name: &str) -> Option<Vec<String>> {
    let name = name.trim().to_lowercase();
    let at = list.iter().position(|m| m.to_lowercase() == name)?;
    let mut out = list.to_vec();
    out.remove(at);
    Some(out)
}

/// `name` added to `c.muted_apps` (see `with_muted`). Meant as the edit of a
/// `save_merged_with`: there `c.muted_apps` is the file's list, so a name
/// added by hand while Vale runs is kept, not replaced by the list in memory.
pub fn add_muted(c: &mut Config, name: &str) {
    if let Some(list) = with_muted(&c.muted_apps, name) {
        c.muted_apps = list;
    }
}

/// `name` taken out of `c.muted_apps` (see `without_muted`); the
/// `save_merged_with` edit, as for `add_muted`.
pub fn remove_muted(c: &mut Config, name: &str) {
    if let Some(list) = without_muted(&c.muted_apps, name) {
        c.muted_apps = list;
    }
}

/// Settings → Fitur → Notifikasi.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct NotifSettings {
    /// "Simpan notifikasi": the page keeps the toasts it catches.
    #[serde(deserialize_with = "default_on")]
    pub record: bool,
    /// "Jumlah maksimal", kept in 10–200 by 10.
    #[serde(deserialize_with = "notif_items")]
    pub max_items: u32,
    /// "Simpan setelah restart": an encrypted file, else memory only.
    #[serde(deserialize_with = "default_on")]
    pub persist: bool,
    /// "Jeda": nothing new is kept while on (the notch still shows toasts).
    #[serde(deserialize_with = "default_off")]
    pub paused: bool,
    /// "Notifikasi saat notch dipakai": how a Windows toast shows while the
    /// notch is busy (a panel, lyrics, music, a pomodoro).
    #[serde(deserialize_with = "toast_style")]
    pub toast_style: ToastStyle,
}

impl Default for NotifSettings {
    fn default() -> Self {
        NotifSettings {
            record: true,
            max_items: crate::notif::store::DEFAULT_CAP,
            persist: true,
            paused: false,
            toast_style: ToastStyle::Compact,
        }
    }
}

crate::theme::named_enum! {
    /// "Notifikasi saat notch dipakai": Compact (Ringkas, the default) keeps a
    /// real Windows toast from covering a busy notch (a strip in the open
    /// panel, a small logo beside a compact view); Full (Penuh) is the full
    /// takeover as always. Any other name reads as `Compact`.
    ToastStyle { Compact, Full }
}

/// A name or anything else (a number, null): any name that is not a style
/// reads as Compact, so a bad value never makes the whole config invalid.
fn toast_style<'de, D: Deserializer<'de>>(d: D) -> Result<ToastStyle, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str()
        .and_then(ToastStyle::from_name)
        .unwrap_or_default())
}

/// A bool; anything else is off.
fn default_off<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_bool().unwrap_or(false))
}

/// Any number, kept in range and stepped; a non-number is the default.
fn notif_items<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_f64().map_or(crate::notif::store::DEFAULT_CAP, |n| {
        crate::notif::store::clamp_cap(n.clamp(0.0, f64::from(u32::MAX)).round() as u32)
    }))
}

/// `notifications` read leniently: not an object is the default, and a bad
/// field is that field's default (each field's own reader).
fn notif_settings<'de, D: Deserializer<'de>>(d: D) -> Result<NotifSettings, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    // Only an object: serde would also read a list as the fields in order.
    if !v.is_object() {
        return Ok(NotifSettings::default());
    }
    Ok(NotifSettings::deserialize(v).unwrap_or_default())
}

/// A bool; anything else (a string, a number, null) is on, so a bad value
/// never makes the whole config invalid.
fn default_on<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_bool().unwrap_or(true))
}

/// A string; anything else (a number, null) is empty.
fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str().unwrap_or_default().to_owned())
}

/// A cat colour name; anything else (a typo, a number) is Terracotta.
fn cat_color<'de, D: Deserializer<'de>>(d: D) -> Result<CatColor, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str().and_then(CatColor::from_name).unwrap_or_default())
}

/// An accessory name; anything else is None.
fn cat_accessory<'de, D: Deserializer<'de>>(d: D) -> Result<CatAccessory, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str()
        .and_then(CatAccessory::from_name)
        .unwrap_or_default())
}

crate::theme::named_enum! {
    /// "Isi notch": what the closed island's background is. Solid is the
    /// island as always; Transparent lets the desktop show through at
    /// `notch_opacity`. Any other name reads as `Solid`.
    NotchFill { Solid, Transparent }
}

crate::theme::named_enum! {
    /// "Mode buka": Hover opens the panel under the pointer and closes it when
    /// the pointer leaves (the default, as always); HoverStay opens on hover
    /// and keeps it open until a click outside; Click opens on a click on the
    /// island, and keeps it open until a click outside. Any other name reads
    /// as `Hover`.
    OpenMode { Hover, HoverStay, Click }
}

crate::theme::named_enum! {
    /// "Jika diabaikan": what happens to a screenshot's file when its preview
    /// ran its full time without a click. Keep (Simpan) by default; Delete
    /// (Hapus) is an explicit opt-in. Any other name reads as `Keep`.
    ShotIgnore { Keep, Delete }
}

crate::theme::named_enum! {
    /// "Alat screenshot": what the Screenshot tile starts. Vale is its own
    /// region capture with the editor straight after; Windows is Snipping
    /// Tool (`ms-screenclip:`). An old config without it, or any other name,
    /// reads as `Vale`.
    ShotTool { Vale, Windows }
}

crate::theme::named_enum! {
    /// "Setelah capture": what happens after Vale's own capture picks a
    /// region. Editor opens the editor window (as always); Inline keeps the
    /// frozen screen up and lets the person draw on it (Lightshot style).
    /// A missing value or any other name reads as `Editor`.
    AfterCapture { Editor, Inline }
}

crate::theme::named_enum! {
    /// The UI typeface of the notch, panel and Settings. Sans is DM Sans (the
    /// default, as always); Mono is JetBrains Mono. Code text stays DM Mono in
    /// both, and the reminder and screenshot editors stay DM Sans.
    UiFont { Sans, Mono }
}

/// A font name; anything else (a typo, a number, null, an object) is Sans, and
/// the rest of the file is kept.
fn ui_font<'de, D: Deserializer<'de>>(d: D) -> Result<UiFont, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str().and_then(UiFont::from_name).unwrap_or_default())
}

/// "Lama tampil" in seconds: (min, max, step).
pub const SHOT_SECS_RANGE: (u32, u32, u32) = (4, 30, 2);

/// Settings → Clipboard → Screenshot.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct ScreenshotSettings {
    /// "Preview di notch".
    pub preview: bool,
    /// "Jika diabaikan".
    pub on_ignore: ShotIgnore,
    /// "Lama tampil", kept in `SHOT_SECS_RANGE`.
    #[serde(deserialize_with = "shot_secs")]
    pub show_secs: u32,
    /// "Alat screenshot".
    #[serde(deserialize_with = "shot_tool")]
    pub tool: ShotTool,
    /// "Salin otomatis": Vale's own capture is copied to the clipboard when
    /// the editor opens and again after each finished edit. A missing field,
    /// a non-bool or null reads as on.
    #[serde(deserialize_with = "default_on")]
    pub auto_copy: bool,
    /// "Setelah capture": the editor window or drawing on the screen itself.
    #[serde(deserialize_with = "after_capture")]
    pub after_capture: AfterCapture,
}

impl Default for ScreenshotSettings {
    fn default() -> Self {
        ScreenshotSettings {
            preview: true,
            on_ignore: ShotIgnore::Keep,
            show_secs: 8,
            tool: ShotTool::Vale,
            auto_copy: true,
            after_capture: AfterCapture::Editor,
        }
    }
}

/// A name (any case) or anything else (a number, null): a value that is not
/// a mode reads as Editor, so a bad value never invalidates the file.
fn after_capture<'de, D: Deserializer<'de>>(d: D) -> Result<AfterCapture, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str()
        .and_then(AfterCapture::from_name)
        .unwrap_or_default())
}

/// A name or anything else (a number, null): any name that is not a tool
/// reads as Vale, so a bad value never makes the whole config invalid.
fn shot_tool<'de, D: Deserializer<'de>>(d: D) -> Result<ShotTool, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str().and_then(ShotTool::from_name).unwrap_or_default())
}

/// Any number, kept in `SHOT_SECS_RANGE`; a non-number is the default.
fn shot_secs<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let (min, max, _) = SHOT_SECS_RANGE;
    Ok(v.as_f64()
        .map_or(ScreenshotSettings::default().show_secs, |n| {
            n.clamp(f64::from(min), f64::from(max)).round() as u32
        }))
}

/// Settings → Clipboard.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct ClipboardSettings {
    /// "Riwayat clipboard": keep what is copied.
    pub enabled: bool,
    /// "Jumlah item", kept in 10–200 (pins come on top).
    #[serde(deserialize_with = "clip_items")]
    pub max_items: u32,
    /// "Simpan setelah restart": the history goes to an encrypted file; off,
    /// it lives in memory only.
    pub persist: bool,
    /// "Hapus saat layar dikunci": unpinned entries go when the screen locks.
    pub clear_on_lock: bool,
    /// "Disalin" also for copies made in other apps.
    pub copied_feedback_all: bool,
    /// "Notifikasi salinan": a preview in the notch on every copy made in
    /// another app. While it is on it supersedes `copied_feedback_all` for
    /// those copies.
    pub copy_notice: bool,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        ClipboardSettings {
            enabled: true,
            max_items: crate::clipboard::store::DEFAULT_ITEMS,
            persist: true,
            clear_on_lock: false,
            copied_feedback_all: false,
            copy_notice: true,
        }
    }
}

/// Any number, kept in range; a non-number is the default.
fn clip_items<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_f64()
        .map_or(crate::clipboard::store::DEFAULT_ITEMS, |n| {
            crate::clipboard::store::clamp_items(n.clamp(0.0, f64::from(u32::MAX)) as u32)
        }))
}

/// Any number, kept in `NOTCH_W_MIN..=NOTCH_W_MAX`; a non-number is the default.
fn notch_width<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_f64().map_or(NOTCH_W_DEFAULT, |n| {
        n.clamp(f64::from(NOTCH_W_MIN), f64::from(NOTCH_W_MAX))
            .round() as u32
    }))
}

/// Any number, kept in `NOTCH_H_MIN..=NOTCH_H_MAX`; a non-number is the default.
fn notch_height<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_f64().map_or(NOTCH_H_DEFAULT, |n| {
        n.clamp(f64::from(NOTCH_H_MIN), f64::from(NOTCH_H_MAX))
            .round() as u32
    }))
}

/// Any number, kept in 0..=100; a non-number is the default.
fn notch_opacity<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_f64()
        .map_or(NOTCH_OPACITY_DEFAULT, |n| n.clamp(0.0, 100.0).round() as u8))
}

/// The disk limit's (min, max, step) in GB.
pub const DISK_WARN_RANGE: (u32, u32, u32) = (5, 100, 5);
/// The RAM limit's (min, max, step) in percent.
pub const RAM_WARN_RANGE: (u8, u8, u8) = (70, 98, 1);

fn is_false(b: &bool) -> bool {
    !b
}

/// Any number, kept in `DISK_WARN_RANGE`; a non-number is the default.
fn disk_warn<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let (min, max, _) = DISK_WARN_RANGE;
    Ok(v.as_f64().map_or(Config::default().disk_warn_gb, |n| {
        n.clamp(f64::from(min), f64::from(max)).round() as u32
    }))
}

/// Any number, kept in `RAM_WARN_RANGE`; a non-number is the default.
fn ram_warn<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let (min, max, _) = RAM_WARN_RANGE;
    Ok(v.as_f64().map_or(Config::default().ram_warn_percent, |n| {
        n.clamp(f64::from(min), f64::from(max)).round() as u8
    }))
}

/// Any number, kept in 1–60; a non-number is the default.
fn sleep_minutes<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_f64()
        .map_or(Config::default().robot_sleep_minutes, |n| {
            clamp_sleep(n.clamp(0.0, f64::from(u32::MAX)) as u32)
        }))
}

impl Default for Config {
    fn default() -> Self {
        Config {
            style: Style::Pill,
            battery_threshold: 20,
            start_with_windows: true,
            muted_apps: default_muted_apps(),
            settings_window: None,
            onboarding_done: false,
            pomodoro: PomoSettings::default(),
            google_client_id: String::new(),
            google_client_secret: String::new(),
            language: Lang::Id,
            accent: Accent::Terracotta,
            surface: Surface::Auto,
            notch_width: NOTCH_W_DEFAULT,
            notch_height: NOTCH_H_DEFAULT,
            notch_fill: NotchFill::Solid,
            notch_opacity: NOTCH_OPACITY_DEFAULT,
            window_theme: WindowTheme::System,
            idle_clock: true,
            stay_behind: false,
            lyrics: false,
            gestures: true,
            scroll_volume: true,
            cli_shell: Shell::PowerShell,
            cli_start: CliStart::Explorer,
            open_mode: OpenMode::Hover,
            robot: true,
            robot_animation: Motion::Full,
            robot_sleep_minutes: 10,
            morning_greeting: true,
            last_greeting: None,
            media_log: false,
            disk_warn_gb: 10,
            ram_warn_percent: 90,
            clean_mode: CleanMode::Own,
            mole_protect_defaults: true,
            mole_defaults_added: false,
            clipboard: ClipboardSettings::default(),
            screenshot: ScreenshotSettings::default(),
            panel_pages: Vec::new(),
            shortcut_tiles: Vec::new(),
            device_notices: true,
            cat_color: CatColor::Terracotta,
            cat_accessory: CatAccessory::None,
            cat_follow_pointer: true,
            cat_dance: true,
            notch_peek: true,
            eq_follow_audio: true,
            notifications: NotifSettings::default(),
            arrange: ArrangeSettings::default(),
            update_check: true,
            hotkeys: crate::hotkey::Hotkeys::default(),
            update_offered: String::new(),
            font: UiFont::Sans,
            notch_monitor: NotchMonitor::Primary,
            lyrics_card: None,
            widgets: crate::widgets::prefs::WidgetPrefs::default(),
            recording: crate::recorder::prefs::RecordingPrefs::default(),
        }
    }
}

/// `panel_pages` read entry by entry: anything that is not a list, and any
/// entry without a string `page` and a bool `on`, is dropped instead of
/// making the whole file invalid.
fn panel_pages<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PageEntry>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    let list = value.as_array().map(Vec::as_slice).unwrap_or_default();
    Ok(list
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect())
}

/// `shortcut_tiles` read entry by entry, like `panel_pages`: an entry that
/// is not an object is dropped (the others stay). The retired "ValeSettings"
/// tile (any case) is dropped too, so a config read and saved again for any
/// other setting never writes it back.
fn shortcut_tiles<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<TileEntry>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    let list = value.as_array().map(Vec::as_slice).unwrap_or_default();
    Ok(list
        .iter()
        .filter(|v| v.is_object())
        .filter_map(|v| serde_json::from_value::<TileEntry>(v.clone()).ok())
        .filter(|e| !e.tile.trim().eq_ignore_ascii_case("ValeSettings"))
        .collect())
}

/// Missing or malformed config falls back to defaults rather than failing startup.
/// A leading UTF-8 BOM (Notepad and PowerShell 5.1 may write one) is skipped.
pub fn parse(text: &str) -> Config {
    try_parse(text).unwrap_or_default()
}

/// `None` when `text` is not a config.
pub fn try_parse(text: &str) -> Option<Config> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut value: serde_json::Value = serde_json::from_str(text).ok()?;
    migrate_hotkeys(&mut value);
    serde_json::from_value(value).ok()
}

/// A file without a `hotkeys` key is from before the recordable hotkeys: the
/// old presets (`quick_search_hotkey`, 3.2.0's `search_hotkey`,
/// `devices_hotkey`, `clipboard.hotkey`, `arrange.hotkey`,
/// `screenshot.print_screen`) become the new rows. The old keys are not read
/// again and drop out on the next save.
fn migrate_hotkeys(value: &mut serde_json::Value) {
    if value.get("hotkeys").is_some() {
        return;
    }
    let migrated = crate::hotkey::from_legacy(value);
    if let Some(map) = value.as_object_mut()
        && let Ok(json) = serde_json::to_value(migrated)
    {
        map.insert("hotkeys".into(), json);
    }
}

/// config.json as found on disk.
#[derive(Debug, PartialEq)]
pub enum OnDisk {
    /// No file, or nothing in it but whitespace (or a BOM): nothing to keep.
    Missing,
    /// Text that is not a config (a typo while editing by hand), or a file
    /// that cannot be read.
    Invalid,
    Valid(Config),
}

pub fn classify(read: io::Result<String>) -> OnDisk {
    match read {
        Ok(text) => {
            let body = text.strip_prefix('\u{feff}').unwrap_or(&text);
            if body.trim().is_empty() {
                OnDisk::Missing
            } else {
                try_parse(body).map_or(OnDisk::Invalid, OnDisk::Valid)
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => OnDisk::Missing,
        Err(_) => OnDisk::Invalid,
    }
}

pub fn read_at(path: &Path) -> OnDisk {
    classify(std::fs::read_to_string(path))
}

/// The island's own settings (`mem`: style, autostart, onboarding, pomodoro,
/// language, theme, idle clock, lyrics, CLI shell and start folder, the robot, its greeting
/// and the day it last greeted, the cleaner's mode and Mole's whitelist
/// flags) over everything else as it
/// is on disk (Google client, muted apps, battery threshold, the disk and RAM
/// limits), which the owner may have edited by hand while the island ran.
pub fn merge_owned(disk: Config, mem: &Config) -> Config {
    Config {
        style: mem.style,
        start_with_windows: mem.start_with_windows,
        onboarding_done: mem.onboarding_done,
        pomodoro: mem.pomodoro,
        language: mem.language,
        accent: mem.accent,
        surface: mem.surface,
        notch_width: mem.notch_width,
        notch_height: mem.notch_height,
        notch_fill: mem.notch_fill,
        notch_opacity: mem.notch_opacity,
        window_theme: mem.window_theme,
        idle_clock: mem.idle_clock,
        stay_behind: mem.stay_behind,
        lyrics: mem.lyrics,
        cli_shell: mem.cli_shell,
        cli_start: mem.cli_start,
        open_mode: mem.open_mode,
        robot: mem.robot,
        robot_animation: mem.robot_animation,
        robot_sleep_minutes: mem.robot_sleep_minutes,
        morning_greeting: mem.morning_greeting,
        last_greeting: mem.last_greeting,
        clean_mode: mem.clean_mode,
        mole_protect_defaults: mem.mole_protect_defaults,
        mole_defaults_added: mem.mole_defaults_added,
        clipboard: mem.clipboard,
        screenshot: mem.screenshot,
        panel_pages: mem.panel_pages.clone(),
        shortcut_tiles: mem.shortcut_tiles.clone(),
        device_notices: mem.device_notices,
        cat_color: mem.cat_color,
        cat_accessory: mem.cat_accessory,
        cat_follow_pointer: mem.cat_follow_pointer,
        cat_dance: mem.cat_dance,
        notch_peek: mem.notch_peek,
        eq_follow_audio: mem.eq_follow_audio,
        notifications: mem.notifications,
        arrange: mem.arrange.clone(),
        update_check: mem.update_check,
        hotkeys: mem.hotkeys,
        update_offered: mem.update_offered.clone(),
        font: mem.font,
        notch_monitor: mem.notch_monitor.clone(),
        lyrics_card: mem.lyrics_card,
        widgets: mem.widgets,
        recording: mem.recording.clone(),
        ..disk
    }
}

/// What a save found wrong with the file it replaced.
#[derive(Debug, PartialEq)]
pub enum Repair {
    /// The file was valid or absent.
    None,
    /// An invalid file was moved here before the island's config was written.
    BackedUp(PathBuf),
    /// An invalid file could not be moved aside, so nothing was written; the
    /// next save tries again.
    Blocked,
}

pub struct SaveOutcome {
    /// The config now in effect: the file's hand-edited fields under the
    /// island's own settings, or the island's config when the file had none.
    pub config: Config,
    pub repair: Repair,
}

/// Saves the island's settings `mem` to `path` without losing anything there:
/// a valid file keeps its other fields (`merge_owned`), a missing or empty one
/// gets `mem`, and an invalid one is renamed to a backup (`backup_path`) first.
/// Write errors only happen on a broken profile; the setting still holds for
/// this run, so they are not reported.
pub fn save_merged_at(path: &Path, mem: &Config) -> SaveOutcome {
    save_merged_with_at(path, mem, |_| {})
}

/// `save_merged_at`, plus `edit` applied on top of the merge: for a field that
/// is normally the owner's to edit by hand (the Google client, the battery
/// threshold) but was just set from the settings window. Only what `edit`
/// touches is replaced; every other hand edit in the file stays.
pub fn save_merged_with_at(
    path: &Path,
    mem: &Config,
    edit: impl FnOnce(&mut Config),
) -> SaveOutcome {
    let (mut config, repair) = match read_at(path) {
        OnDisk::Valid(disk) => (merge_owned(disk, mem), Repair::None),
        OnDisk::Missing => (mem.clone(), Repair::None),
        OnDisk::Invalid => {
            let backup = backup_path(path);
            match std::fs::rename(path, &backup) {
                Ok(()) => (mem.clone(), Repair::BackedUp(backup)),
                // Gone meanwhile: nothing left to keep.
                Err(e) if e.kind() == io::ErrorKind::NotFound => (mem.clone(), Repair::None),
                Err(_) => {
                    let mut config = mem.clone();
                    edit(&mut config);
                    return SaveOutcome {
                        config,
                        repair: Repair::Blocked,
                    };
                }
            }
        }
    };
    edit(&mut config);
    let _ = config.save_to(path);
    SaveOutcome { config, repair }
}

pub fn save_merged(mem: &Config) -> SaveOutcome {
    save_merged_at(&path(), mem)
}

pub fn save_merged_with(mem: &Config, edit: impl FnOnce(&mut Config)) -> SaveOutcome {
    save_merged_with_at(&path(), mem, edit)
}

/// `<file>.bak`, or `.bak2`, `.bak3`… when earlier backups exist, so a
/// backup is never overwritten.
pub fn backup_path(path: &Path) -> PathBuf {
    let with = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    let first = with(".bak");
    if !first.exists() {
        return first;
    }
    (2..1000)
        .map(|n| with(&format!(".bak{n}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// `%APPDATA%\Vale\config.json` (`data_dir::dir`); every other data file sits
/// next to it.
pub fn path() -> PathBuf {
    crate::data_dir::dir().join("config.json")
}

pub fn load() -> Config {
    parse(&std::fs::read_to_string(path()).unwrap_or_default())
}

impl Config {
    /// Pretty JSON to a sibling `.tmp` file, then renamed over `path`, so a
    /// crash mid-write never leaves a truncated config.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::greeting::DayPeriod;
    use chrono::NaiveDate;

    fn mark(y: i32, m: u32, d: u32, period: DayPeriod) -> Option<GreetMark> {
        Some(GreetMark {
            date: NaiveDate::from_ymd_opt(y, m, d)?,
            period,
        })
    }

    #[test]
    fn empty_text_is_default() {
        assert_eq!(parse(""), Config::default());
    }

    #[test]
    fn invalid_json_is_default() {
        assert_eq!(parse("{not json"), Config::default());
    }

    #[test]
    fn leading_byte_order_mark_is_ignored() {
        let c = parse("\u{feff}{\"style\":\"Notch\",\"battery_threshold\":30}");
        assert_eq!(c.style, Style::Notch);
        assert_eq!(c.battery_threshold, 30);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let c = parse(r#"{"style":"Notch"}"#);
        assert_eq!(c.style, Style::Notch);
        assert_eq!(
            c,
            Config {
                style: Style::Notch,
                ..Config::default()
            }
        );
    }

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert_eq!(c.style, Style::Pill);
        assert_eq!(c.battery_threshold, 20);
        assert!(c.start_with_windows);
        assert_eq!(c.muted_apps, vec!["Claude".to_string(), "Orca".to_string()]);
        assert!(!c.onboarding_done);
        assert_eq!(c.language, Lang::Id);
        assert_eq!(c.accent, Accent::Terracotta);
        assert_eq!(c.surface, Surface::Auto);
        assert_eq!(c.window_theme, WindowTheme::System);
        assert!(c.idle_clock);
        assert!(!c.lyrics);
        assert_eq!(c.cli_shell, Shell::PowerShell);
        assert!(c.robot);
        assert_eq!(c.robot_animation, Motion::Full);
        assert_eq!(c.robot_sleep_minutes, 10);
        assert!(c.morning_greeting);
        assert_eq!(c.last_greeting, None);
    }

    #[test]
    fn robot_fields_round_trip_and_hand_edits_fall_back() {
        let c = Config {
            robot: false,
            robot_animation: Motion::Saver,
            robot_sleep_minutes: 25,
            morning_greeting: false,
            last_greeting: mark(2026, 10, 27, DayPeriod::Evening),
            ..Config::default()
        };
        let json = serde_json::to_string(&c).unwrap();
        for field in [
            r#""robot_animation":"Saver""#,
            r#""last_greeting":"2026-10-27 evening""#,
            r#""robot_sleep_minutes":25"#,
        ] {
            assert!(json.contains(field), "{json}");
        }
        assert_eq!(parse(&json), c);
        // Typos and out-of-range values: only that field falls back or clamps.
        let hand = parse(
            r#"{"style":"Notch","robot_animation":"mati","robot_sleep_minutes":500,"last_greeting":"kemarin"}"#,
        );
        assert_eq!(
            (
                hand.style,
                hand.robot_animation,
                hand.robot_sleep_minutes,
                hand.last_greeting
            ),
            (Style::Notch, Motion::Full, 60, None)
        );
        let off = parse(r#"{"robot_animation":"off","robot_sleep_minutes":0}"#);
        assert_eq!(
            (off.robot_animation, off.robot_sleep_minutes),
            (Motion::Off, 1)
        );
        let odd = parse(r#"{"robot_sleep_minutes":"ten","last_greeting":null}"#);
        assert_eq!((odd.robot_sleep_minutes, odd.last_greeting), (10, None));
        // The old once-a-day value, a plain date, reads as that morning.
        let old = parse(r#"{"robot_sleep_minutes":20,"last_greeting":"2026-10-27"}"#);
        assert_eq!(old.last_greeting, mark(2026, 10, 27, DayPeriod::Morning));
        assert_eq!(old.robot_sleep_minutes, 20);
        // A bad period or date reads as none and the rest of the file stays.
        let bad = parse(r#"{"robot_sleep_minutes":20,"last_greeting":"2026-10-27 lunch"}"#);
        assert_eq!((bad.last_greeting, bad.robot_sleep_minutes), (None, 20));
    }

    #[test]
    fn media_log_is_hidden_and_off_by_default() {
        assert!(!Config::default().media_log);
        // An older config without the field still parses, with it off.
        let old = parse(r#"{"style":"Notch","lyrics":true}"#);
        assert_eq!(
            (old.style, old.lyrics, old.media_log),
            (Style::Notch, true, false)
        );
        // Off, it is not written; set by hand, it is read and kept.
        let json = serde_json::to_string(&Config::default()).unwrap();
        assert!(!json.contains("media_log"), "{json}");
        let on = parse(r#"{"media_log":true}"#);
        assert!(on.media_log);
        assert!(
            serde_json::to_string(&on)
                .unwrap()
                .contains(r#""media_log":true"#)
        );
        // The island's own saves keep the hand-set value from disk.
        assert!(merge_owned(on, &Config::default()).media_log);
    }

    #[test]
    fn cli_shell_is_a_name_and_a_typo_falls_back() {
        let cmd = Config {
            cli_shell: Shell::Cmd,
            ..Config::default()
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""cli_shell":"cmd""#), "{json}");
        assert_eq!(parse(&json).cli_shell, Shell::Cmd);
        let typo = parse(r#"{"style":"Notch","cli_shell":"zsh"}"#);
        assert_eq!(
            (typo.style, typo.cli_shell),
            (Style::Notch, Shell::PowerShell)
        );
    }

    #[test]
    fn cli_start_reads_old_files_typos_and_round_trips() {
        assert_eq!(Config::default().cli_start, CliStart::Explorer);
        // A config.json from before the field: Explorer.
        let old = parse(r#"{"style":"Notch","cli_shell":"cmd"}"#);
        assert_eq!(
            (old.style, old.cli_shell, old.cli_start),
            (Style::Notch, Shell::Cmd, CliStart::Explorer)
        );
        let last = Config {
            cli_start: CliStart::Last,
            ..Config::default()
        };
        let json = serde_json::to_string(&last).unwrap();
        assert!(json.contains(r#""cli_start":"Last""#), "{json}");
        assert_eq!(parse(&json), last);
        // A typo keeps the rest of the file: only this field falls back.
        let typo = parse(r#"{"style":"Notch","cli_start":"terakhir"}"#);
        assert_eq!(
            (typo.style, typo.cli_start),
            (Style::Notch, CliStart::Explorer)
        );
        // The island's own value wins over a stale copy on disk.
        let disk = Config::default();
        assert_eq!(merge_owned(disk, &last).cli_start, CliStart::Last);
    }

    #[test]
    fn round_trips_through_json() {
        let c = Config {
            gestures: false,
            scroll_volume: false,
            shortcut_tiles: vec![TileEntry {
                tile: "Lock".into(),
                on: false,
                ..TileEntry::default()
            }],
            style: Style::Notch,
            battery_threshold: 35,
            start_with_windows: false,
            muted_apps: vec!["Slack".into()],
            onboarding_done: true,
            pomodoro: PomoSettings {
                focus_min: 50,
                short_break_min: 10,
                long_break_min: 30,
                sessions: 3,
            },
            google_client_id: "id.apps.googleusercontent.com".into(),
            google_client_secret: "secret".into(),
            language: Lang::En,
            accent: Accent::Ochre,
            surface: Surface::Graphite,
            notch_width: 250,
            notch_height: 40,
            notch_fill: NotchFill::Transparent,
            notch_opacity: 65,
            window_theme: WindowTheme::Light,
            idle_clock: false,
            stay_behind: true,
            lyrics: true,
            cli_shell: Shell::Cmd,
            cli_start: CliStart::Last,
            open_mode: OpenMode::Click,
            robot: false,
            robot_animation: Motion::Off,
            robot_sleep_minutes: 5,
            morning_greeting: false,
            last_greeting: mark(2026, 1, 2, DayPeriod::Night),
            media_log: true,
            disk_warn_gb: 25,
            ram_warn_percent: 85,
            clean_mode: CleanMode::Mole,
            mole_protect_defaults: false,
            mole_defaults_added: true,
            clipboard: ClipboardSettings {
                enabled: false,
                max_items: 120,
                persist: false,
                clear_on_lock: true,
                copied_feedback_all: true,
                copy_notice: false,
            },
            screenshot: ScreenshotSettings {
                preview: false,
                on_ignore: ShotIgnore::Delete,
                show_secs: 12,
                tool: ShotTool::Windows,
                auto_copy: false,
                after_capture: AfterCapture::Inline,
            },
            panel_pages: vec![PageEntry {
                page: "Media".into(),
                on: false,
            }],
            device_notices: false,
            cat_color: CatColor::Calico,
            cat_accessory: CatAccessory::Bow,
            cat_follow_pointer: false,
            cat_dance: false,
            notch_peek: false,
            eq_follow_audio: false,
            notifications: NotifSettings {
                record: false,
                max_items: 120,
                persist: false,
                paused: true,
                toast_style: ToastStyle::Full,
            },
            arrange: ArrangeSettings {
                gap: 16,
                ..ArrangeSettings::default()
            },
            update_check: false,
            update_offered: "3.11.0".into(),
            hotkeys: crate::hotkey::Hotkeys {
                search: None,
                clipboard: crate::hotkey::parse("Win+Alt+V").unwrap(),
                devices: crate::hotkey::parse("Ctrl+Alt+D").unwrap(),
                arrange: crate::hotkey::parse("Win+Shift+F13").unwrap(),
                screenshot: crate::hotkey::parse("Ctrl+PrtSc").unwrap(),
                lyrics_card: crate::hotkey::parse("Win+Alt+L").unwrap(),
                sticky_note: crate::hotkey::parse("Ctrl+Alt+N").unwrap(),
                sticky_list: crate::hotkey::parse("Ctrl+Alt+J").unwrap(),
                record: crate::hotkey::parse("Ctrl+Alt+Y").unwrap(),
                record_mute: crate::hotkey::parse("Ctrl+Alt+U").unwrap(),
            },
            settings_window: Some(WindowPlace {
                left: 20,
                top: 30,
                right: 1000,
                bottom: 800,
                maximized: false,
            }),
            font: UiFont::Mono,
            notch_monitor: NotchMonitor::Chosen {
                path: r"\\?\DISPLAY#X#1".into(),
                name: "Dell".into(),
            },
            lyrics_card: Some(CardPlace {
                left: -500,
                top: 72,
                right: -200,
                bottom: 492,
            }),
            widgets: crate::widgets::prefs::WidgetPrefs {
                show: false,
                layer: crate::widgets::prefs::WidgetLayer::Top,
                fullscreen: crate::widgets::prefs::FullscreenRule::Keep,
                lock: true,
                cat_peek: false,
                tilt: true,
                clip_fill: true,
            },
            recording: crate::recorder::prefs::RecordingPrefs {
                sources: crate::model::RecSources {
                    mic: true,
                    system: false,
                },
                folder: Some(std::path::PathBuf::from(r"E:\Rec")),
                consent_shown: true,
                mic_device: Some("{0.0.1.00000000}.{abc}".into()),
                system_device: None,
                echo_cancel: false,
            },
        };
        assert_eq!(parse(&serde_json::to_string(&c).unwrap()), c);
    }

    #[test]
    fn recording_prefs_read_leniently_and_belong_to_the_island() {
        use crate::recorder::prefs::RecordingPrefs;
        assert_eq!(parse(r#"{"style":"Notch"}"#).recording, RecordingPrefs::default());
        for bad in [r#""x""#, "7", "null", "[1]", r#"{"sources":5,"consent_shown":"yes"}"#] {
            let c = parse(&format!(r#"{{"style":"Notch","recording":{bad}}}"#));
            assert_eq!(c.style, Style::Notch, "{bad}");
            assert_eq!(c.recording, RecordingPrefs::default(), "{bad}");
        }
        let c = parse(r#"{"recording":{"consent_shown":true,"folder":"D:\\Audio"}}"#);
        assert!(c.recording.consent_shown && c.recording.sources.mic && c.recording.sources.system);
        assert_eq!(c.recording.folder, Some(std::path::PathBuf::from(r"D:\Audio")));
        let mem = Config {
            recording: RecordingPrefs {
                consent_shown: true,
                ..RecordingPrefs::default()
            },
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert!(merged.recording.consent_shown);
    }

    #[test]
    fn widget_prefs_read_leniently_and_belong_to_the_island() {
        use crate::widgets::prefs::{FullscreenRule, WidgetLayer, WidgetPrefs};
        // An old config has no key: the defaults.
        assert_eq!(parse(r#"{"style":"Notch"}"#).widgets, WidgetPrefs::default());
        assert_eq!(parse(r#"{"style":"Notch"}"#).style, Style::Notch);
        // A bad value never loses the rest of the file.
        for bad in [r#""x""#, "7", "null", "[1]", r#"{"layer":5,"show":"no"}"#] {
            let c = parse(&format!(r#"{{"style":"Notch","widgets":{bad}}}"#));
            assert_eq!(c.style, Style::Notch, "{bad}");
            assert_eq!(c.widgets, WidgetPrefs::default(), "{bad}");
        }
        let c = parse(r#"{"widgets":{"layer":"top","fullscreen":"Keep","lock":true}}"#);
        assert_eq!(c.widgets.layer, WidgetLayer::Top);
        assert_eq!(c.widgets.fullscreen, FullscreenRule::Keep);
        assert!(c.widgets.lock && c.widgets.show && c.widgets.cat_peek);
        // The island's copy wins over the file's.
        let mem = Config {
            widgets: WidgetPrefs {
                show: false,
                ..WidgetPrefs::default()
            },
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert!(!merged.widgets.show);
        assert_eq!(parse(&serde_json::to_string(&merged).unwrap()).widgets, mem.widgets);
    }

    #[test]
    fn update_settings_default_on_read_leniently_and_belong_to_the_island() {
        let d = Config::default();
        assert!(d.update_check);
        assert_eq!(d.update_offered, "");
        // A file from before the setting has neither key.
        let old = parse(r#"{"style":"Notch"}"#);
        assert!(old.update_check);
        assert_eq!(old.update_offered, "");
        // Junk keeps the defaults and does not drop the rest of the file.
        for junk in [r#""yes""#, "null", "7", r#""false""#, "[]"] {
            let c = parse(&format!(
                r#"{{"style":"Notch","update_check":{junk},"update_offered":{junk}}}"#
            ));
            assert!(c.update_check, "{junk}");
            assert_eq!(c.style, Style::Notch);
        }
        assert_eq!(
            parse(r#"{"update_offered":"3.11.0"}"#).update_offered,
            "3.11.0"
        );
        assert!(!parse(r#"{"update_check":false}"#).update_check);
        // The island's copy wins over the file's, both ways.
        let mem = Config {
            update_check: false,
            update_offered: "3.11.0".into(),
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert!(!merged.update_check);
        assert_eq!(merged.update_offered, "3.11.0");
        let merged = merge_owned(mem, &Config::default());
        assert!(merged.update_check);
        assert_eq!(merged.update_offered, "");
    }

    #[test]
    fn screenshot_settings_default_clamp_and_belong_to_the_island() {
        let d = ScreenshotSettings::default();
        assert!(d.preview);
        assert_eq!((d.on_ignore, d.show_secs), (ShotIgnore::Keep, 8));
        // An older config.json without the key loads the defaults.
        let old = parse(r#"{"style":"Notch","clipboard":{"enabled":false}}"#);
        assert_eq!(old.screenshot, ScreenshotSettings::default());
        assert_eq!(old.style, Style::Notch);
        // Bad values fall back or clamp; the rest of the file stays.
        let bad =
            parse(r#"{"style":"Notch","screenshot":{"on_ignore":"Buang","show_secs":"lama"}}"#);
        assert_eq!(bad.style, Style::Notch);
        assert_eq!(bad.screenshot, ScreenshotSettings::default());
        let long = parse(r#"{"screenshot":{"show_secs":600}}"#);
        assert_eq!(long.screenshot.show_secs, 30);
        let short = parse(r#"{"screenshot":{"show_secs":1}}"#);
        assert_eq!(short.screenshot.show_secs, 4);
        let del = parse(r#"{"screenshot":{"on_ignore":"delete","preview":false}}"#);
        assert_eq!(del.screenshot.on_ignore, ShotIgnore::Delete);
        assert!(!del.screenshot.preview);
        // Set in the settings window: the island's own, over the file.
        let mem = Config {
            screenshot: ScreenshotSettings {
                on_ignore: ShotIgnore::Delete,
                ..ScreenshotSettings::default()
            },
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert_eq!(merged.screenshot.on_ignore, ShotIgnore::Delete);
    }

    #[test]
    fn the_screenshot_tool_is_vale_unless_the_file_says_windows() {
        assert_eq!(ScreenshotSettings::default().tool, ShotTool::Vale);
        // An old file without the key, and bad values of any type: Vale,
        // and the rest of the file stays.
        for text in [
            r#"{"style":"Notch","screenshot":{"preview":false}}"#,
            r#"{"style":"Notch","screenshot":{"tool":"Paint"}}"#,
            r#"{"style":"Notch","screenshot":{"tool":7}}"#,
            r#"{"style":"Notch","screenshot":{"tool":null}}"#,
        ] {
            let c = parse(text);
            assert_eq!(c.style, Style::Notch, "{text}");
            assert_eq!(c.screenshot.tool, ShotTool::Vale, "{text}");
        }
        let win = parse(r#"{"screenshot":{"tool":"windows"}}"#);
        assert_eq!(win.screenshot.tool, ShotTool::Windows);
        let back = parse(&serde_json::to_string(&win).unwrap());
        assert_eq!(back.screenshot.tool, ShotTool::Windows);
    }

    #[test]
    fn after_capture_is_the_editor_unless_the_file_says_inline() {
        assert_eq!(
            ScreenshotSettings::default().after_capture,
            AfterCapture::Editor
        );
        // Missing, an unknown name, a number and null: the editor, and the
        // rest of the file stays.
        for text in [
            r#"{"style":"Notch","screenshot":{"preview":false}}"#,
            r#"{"style":"Notch","screenshot":{"after_capture":"Paint"}}"#,
            r#"{"style":"Notch","screenshot":{"after_capture":7}}"#,
            r#"{"style":"Notch","screenshot":{"after_capture":null}}"#,
            r#"{"style":"Notch"}"#,
        ] {
            let c = parse(text);
            assert_eq!(c.style, Style::Notch, "{text}");
            assert_eq!(c.screenshot.after_capture, AfterCapture::Editor, "{text}");
        }
        for text in [
            r#"{"screenshot":{"after_capture":"inline"}}"#,
            r#"{"screenshot":{"after_capture":"Inline"}}"#,
            r#"{"screenshot":{"after_capture":"INLINE"}}"#,
        ] {
            assert_eq!(
                parse(text).screenshot.after_capture,
                AfterCapture::Inline,
                "{text}"
            );
        }
        let inline = parse(r#"{"screenshot":{"after_capture":"inline"}}"#);
        let json = serde_json::to_string(&inline).unwrap();
        assert!(json.contains(r#""after_capture":"Inline""#), "{json}");
        assert_eq!(
            parse(&json).screenshot.after_capture,
            AfterCapture::Inline,
            "round trip"
        );
    }

    #[test]
    fn auto_copy_is_on_unless_the_file_says_false() {
        assert!(ScreenshotSettings::default().auto_copy);
        // An old file without the key, and bad values of any type: on, and
        // the rest of the file stays valid.
        for text in [
            r#"{"style":"Notch","screenshot":{"preview":false}}"#,
            r#"{"style":"Notch","screenshot":{"auto_copy":"x"}}"#,
            r#"{"style":"Notch","screenshot":{"auto_copy":null}}"#,
            r#"{"style":"Notch","screenshot":{"auto_copy":1}}"#,
            r#"{"style":"Notch"}"#,
        ] {
            let c = parse(text);
            assert_eq!(c.style, Style::Notch, "{text}");
            assert!(c.screenshot.auto_copy, "{text}");
        }
        let off = parse(r#"{"style":"Notch","screenshot":{"auto_copy":false}}"#);
        assert!(!off.screenshot.auto_copy);
        assert_eq!(off.style, Style::Notch);
        let back = parse(&serde_json::to_string(&off).unwrap());
        assert!(!back.screenshot.auto_copy, "false round-trips");
    }

    #[test]
    fn panel_pages_default_empty_lenient_and_belong_to_the_island() {
        use crate::pages::PageEntry;
        assert!(Config::default().panel_pages.is_empty());
        // An older config.json without the key loads empty (= all pages).
        let old = parse(r#"{"style":"Notch"}"#);
        assert!(old.panel_pages.is_empty());
        // A bad value never throws the rest of the file away.
        let bad = parse(r#"{"style":"Notch","panel_pages":"semua"}"#);
        assert_eq!(bad.style, Style::Notch);
        assert!(bad.panel_pages.is_empty());
        // Bad entries are dropped, good ones kept.
        let mixed = parse(r#"{"panel_pages":[{"page":"Notes","on":false},7,{"page":"Media"}]}"#);
        assert_eq!(
            mixed.panel_pages,
            vec![PageEntry {
                page: "Notes".into(),
                on: false
            }]
        );
        // Set in the settings window: the island's own, over the file.
        let mem = Config {
            panel_pages: mixed.panel_pages.clone(),
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert_eq!(merged.panel_pages, mixed.panel_pages);
        let json = serde_json::to_string(&merged).unwrap();
        assert!(
            json.contains(r#""panel_pages":[{"page":"Notes","on":false}]"#),
            "{json}"
        );
    }

    #[test]
    fn cleaner_settings_default_and_belong_to_the_island() {
        let c = Config::default();
        assert_eq!(c.clean_mode, CleanMode::Own);
        assert!(c.mole_protect_defaults && !c.mole_defaults_added);
        // An older config without them loads the defaults.
        let old = parse(r#"{"style":"Notch","disk_warn_gb":20}"#);
        assert_eq!(old.clean_mode, CleanMode::Own);
        assert!(old.mole_protect_defaults && !old.mole_defaults_added);
        assert_eq!(old.disk_warn_gb, 20);
        // By name, any case; a typo is the default.
        let mole = parse(r#"{"clean_mode":"mole","mole_defaults_added":true}"#);
        assert_eq!(mole.clean_mode, CleanMode::Mole);
        assert!(mole.mole_defaults_added);
        assert_eq!(
            parse(r#"{"clean_mode":"Robot"}"#).clean_mode,
            CleanMode::Own
        );
        let json = serde_json::to_string(&mole).unwrap();
        assert!(json.contains(r#""clean_mode":"Mole""#), "{json}");
        // The island's own: a save keeps them over what the file says.
        let mem = Config {
            clean_mode: CleanMode::Mole,
            mole_protect_defaults: false,
            mole_defaults_added: true,
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert_eq!(merged.clean_mode, CleanMode::Mole);
        assert!(!merged.mole_protect_defaults && merged.mole_defaults_added);
    }

    #[test]
    fn system_limits_default_round_trip_and_clamp() {
        let c = Config::default();
        assert_eq!((c.disk_warn_gb, c.ram_warn_percent), (10, 90));
        // An older config without the fields loads the defaults.
        let old = parse(r#"{"style":"Notch","battery_threshold":30}"#);
        assert_eq!((old.disk_warn_gb, old.ram_warn_percent), (10, 90));
        let set = Config {
            disk_warn_gb: 40,
            ram_warn_percent: 75,
            ..Config::default()
        };
        let json = serde_json::to_string(&set).unwrap();
        assert!(json.contains(r#""disk_warn_gb":40"#), "{json}");
        assert!(json.contains(r#""ram_warn_percent":75"#), "{json}");
        assert_eq!(parse(&json), set);
        // Out of range clamps; a typo is the default; the rest of the file stays.
        let hand = parse(r#"{"style":"Notch","disk_warn_gb":500,"ram_warn_percent":5}"#);
        assert_eq!(
            (hand.style, hand.disk_warn_gb, hand.ram_warn_percent),
            (Style::Notch, 100, 70)
        );
        let low = parse(r#"{"disk_warn_gb":-3,"ram_warn_percent":120}"#);
        assert_eq!((low.disk_warn_gb, low.ram_warn_percent), (5, 98));
        let typo = parse(r#"{"disk_warn_gb":"sepuluh","ram_warn_percent":null}"#);
        assert_eq!((typo.disk_warn_gb, typo.ram_warn_percent), (10, 90));
        // Owned by the file: a save keeps the value on disk.
        let disk = Config {
            disk_warn_gb: 30,
            ram_warn_percent: 80,
            ..Config::default()
        };
        let merged = merge_owned(disk, &Config::default());
        assert_eq!((merged.disk_warn_gb, merged.ram_warn_percent), (30, 80));
    }

    #[test]
    fn clipboard_defaults_and_hand_edits() {
        let d = ClipboardSettings::default();
        assert!(d.enabled && d.persist && !d.clear_on_lock && !d.copied_feedback_all);
        // The copy notice is on by default, also for a config.json written
        // before it existed (the group or just the field missing).
        assert!(d.copy_notice);
        assert!(parse(r#"{"style":"Notch"}"#).clipboard.copy_notice);
        let old_group = parse(r#"{"clipboard":{"enabled":false,"max_items":30}}"#);
        assert!(old_group.clipboard.copy_notice && !old_group.clipboard.enabled);
        assert!(
            !parse(r#"{"clipboard":{"copy_notice":false}}"#)
                .clipboard
                .copy_notice
        );
        assert_eq!(d.max_items, 50);
        // An older config.json without the group loads the defaults.
        let old = parse(r#"{"style":"Notch","robot":false,"lyrics":true}"#);
        assert_eq!(old.clipboard, ClipboardSettings::default());
        assert_eq!(old.style, Style::Notch);
        // A group with some fields: the others are defaults.
        let some = parse(r#"{"clipboard":{"clear_on_lock":true}}"#);
        assert!(some.clipboard.clear_on_lock && some.clipboard.enabled);
        // The count is clamped; a non-number is the default.
        let big = parse(r#"{"clipboard":{"max_items":5000}}"#);
        assert_eq!(big.clipboard.max_items, 200);
        let small = parse(r#"{"clipboard":{"max_items":1}}"#);
        assert_eq!(small.clipboard.max_items, 10);
        let word = parse(r#"{"clipboard":{"max_items":"banyak"}}"#);
        assert_eq!(word.clipboard.max_items, 50);
        // The island's own settings win over a stale copy on disk.
        let mem = Config {
            clipboard: ClipboardSettings {
                persist: false,
                ..ClipboardSettings::default()
            },
            ..Config::default()
        };
        assert!(!merge_owned(Config::default(), &mem).clipboard.persist);
    }

    #[test]
    fn theme_fields_are_names_and_typos_fall_back() {
        let c = Config {
            accent: Accent::Plum,
            surface: Surface::Graphite,
            window_theme: WindowTheme::Dark,
            ..Config::default()
        };
        let json = serde_json::to_string(&c).unwrap();
        for field in [
            r#""accent":"Plum""#,
            r#""surface":"Graphite""#,
            r#""window_theme":"Dark""#,
        ] {
            assert!(json.contains(field), "{json}");
        }
        let hand = parse(r#"{"accent":"sage","surface":"GRAPHITE","window_theme":"light"}"#);
        assert_eq!(
            (hand.accent, hand.surface, hand.window_theme),
            (Accent::Sage, Surface::Graphite, WindowTheme::Light)
        );
        // A typo keeps the rest of the file: only that field falls back.
        let typo = parse(r#"{"style":"Notch","accent":"teal","surface":"grey","window_theme":1}"#);
        assert_eq!(typo, Config::default(), "a non-string is still invalid");
        let typo = parse(r#"{"style":"Notch","accent":"teal","surface":"grey"}"#);
        assert_eq!(
            (typo.style, typo.accent, typo.surface),
            (Style::Notch, Accent::Terracotta, Surface::Auto)
        );
    }

    #[test]
    fn old_theme_names_load_as_studio_colours() {
        for (old, accent) in [
            ("Blue", Accent::Terracotta),
            ("Purple", Accent::Plum),
            ("Green", Accent::Sage),
            ("Orange", Accent::Ochre),
            ("pink", Accent::Clay),
            ("Red", Accent::Terracotta),
        ] {
            let c = parse(&format!(
                r#"{{"style":"Notch","accent":"{old}","surface":"Black"}}"#
            ));
            assert_eq!(
                (c.style, c.accent, c.surface),
                (Style::Notch, accent, Surface::Espresso),
                "{old}"
            );
            // Saving writes the Studio name, not the old one.
            let json = serde_json::to_string(&c).unwrap();
            let field = format!(r#""accent":"{}""#, accent.name());
            assert!(json.contains(&field), "{json}");
            assert!(json.contains(r#""surface":"Espresso""#), "{json}");
            assert_eq!(parse(&json), c);
        }
        // An old file that only picked a non-default accent keeps it.
        let pink = parse(r#"{"accent":"Pink","surface":"Graphite"}"#);
        assert_eq!(
            (pink.accent, pink.surface),
            (Accent::Clay, Surface::Graphite)
        );
    }

    #[test]
    fn old_file_without_new_fields_loads_defaults() {
        let old = r#"{
            "style": "Notch",
            "battery_threshold": 15,
            "start_with_windows": false,
            "muted_apps": ["Slack"],
            "onboarding_done": true
        }"#;
        let c = parse(old);
        assert_eq!(c.style, Style::Notch);
        assert_eq!(c.battery_threshold, 15);
        assert!(!c.start_with_windows);
        assert_eq!(c.muted_apps, vec!["Slack".to_string()]);
        assert!(c.onboarding_done);
        assert_eq!(c.pomodoro, PomoSettings::default());
        assert_eq!(c.google_client_id, "");
        assert_eq!(c.google_client_secret, "");
        assert_eq!(c.language, Lang::Id);
        assert_eq!(
            (c.accent, c.surface, c.window_theme),
            (Accent::Terracotta, Surface::Auto, WindowTheme::System)
        );
        // The clipboard group came later still: defaults.
        assert_eq!(c.clipboard, ClipboardSettings::default());
        // The robot's fields came later: defaults, and they survive a save.
        assert!(c.robot && c.morning_greeting);
        assert_eq!(
            (c.robot_animation, c.robot_sleep_minutes, c.last_greeting),
            (Motion::Full, 10, None)
        );
        assert_eq!(parse(&serde_json::to_string_pretty(&c).unwrap()), c);
    }

    #[test]
    fn device_notices_default_on_and_belong_to_the_island() {
        let d = Config::default();
        assert!(d.device_notices);
        let old = parse(r#"{"style":"Notch","disk_warn_gb":20}"#);
        assert!(old.device_notices);
        let odd = parse(r#"{"device_notices":false,"style":"Notch"}"#);
        assert_eq!((odd.device_notices, odd.style), (false, Style::Notch));
        let mem = Config {
            device_notices: false,
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert!(!merged.device_notices);
        assert_eq!(parse(&serde_json::to_string_pretty(&mem).unwrap()), mem);
    }

    #[test]
    fn an_old_config_without_notifications_gets_the_defaults() {
        let old = parse(r#"{"style":"Notch","disk_warn_gb":20}"#);
        assert_eq!(old.style, Style::Notch);
        assert_eq!(old.notifications, NotifSettings::default());
        assert_eq!(
            old.notifications,
            NotifSettings {
                record: true,
                max_items: 50,
                persist: true,
                paused: false,
                toast_style: ToastStyle::Compact,
            }
        );
        // A partial object keeps what it names.
        let part = parse(r#"{"notifications":{"paused":true,"max_items":120}}"#);
        assert_eq!(
            part.notifications,
            NotifSettings {
                record: true,
                max_items: 120,
                persist: true,
                paused: true,
                toast_style: ToastStyle::Compact,
            }
        );
    }

    #[test]
    fn a_bad_notification_field_reads_as_its_default_and_keeps_the_file() {
        let c = parse(
            r#"{"style":"Notch","notifications":{"record":"yes","max_items":"many","persist":null,"paused":5}}"#,
        );
        assert_eq!(c.style, Style::Notch, "the rest of the file survives");
        assert_eq!(c.notifications, NotifSettings::default());
        // A whole bad value too.
        for bad in ["\"on\"", "7", "null", "[1,2]", "true"] {
            let c = parse(&format!(r#"{{"style":"Notch","notifications":{bad}}}"#));
            assert_eq!(c.style, Style::Notch, "{bad}");
            assert_eq!(c.notifications, NotifSettings::default(), "{bad}");
        }
        // Numbers are kept in range and stepped.
        let max = |n: &str| {
            parse(&format!(r#"{{"notifications":{{"max_items":{n}}}}}"#))
                .notifications
                .max_items
        };
        assert_eq!(max("3"), 10);
        assert_eq!(max("-40"), 10);
        assert_eq!(max("77"), 80);
        assert_eq!(max("5000"), 200);
        assert_eq!(max("12.4"), 10);
        // A good file round-trips.
        let mem = Config {
            notifications: NotifSettings {
                record: false,
                max_items: 30,
                persist: false,
                paused: true,
                toast_style: ToastStyle::Full,
            },
            ..Config::default()
        };
        assert_eq!(parse(&serde_json::to_string_pretty(&mem).unwrap()), mem);
    }

    #[test]
    fn the_toast_style_reads_compact_unless_the_file_says_full() {
        let style = |v: &str| {
            parse(&format!(r#"{{"notifications":{{"toast_style":{v}}}}}"#))
                .notifications
                .toast_style
        };
        // A config without the field.
        assert_eq!(
            parse(r#"{"notifications":{"paused":true}}"#)
                .notifications
                .toast_style,
            ToastStyle::Compact
        );
        assert_eq!(parse("{}").notifications.toast_style, ToastStyle::Compact);
        assert_eq!(style("\"Full\""), ToastStyle::Full);
        assert_eq!(style("\"Compact\""), ToastStyle::Compact);
        for bad in ["5", "null", "\"x\"", "true", "[1]"] {
            assert_eq!(style(bad), ToastStyle::Compact, "{bad}");
        }
        // A bad style keeps the other fields of the object.
        let c = parse(r#"{"notifications":{"paused":true,"toast_style":9}}"#);
        assert!(c.notifications.paused);
        assert_eq!(NotifSettings::default().toast_style, ToastStyle::Compact);
    }

    #[test]
    fn notification_settings_belong_to_the_island() {
        let mem = Config {
            notifications: NotifSettings {
                paused: true,
                max_items: 100,
                ..NotifSettings::default()
            },
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert!(merged.notifications.paused);
        assert_eq!(merged.notifications.max_items, 100);
        let full = Config {
            notifications: NotifSettings {
                toast_style: ToastStyle::Full,
                ..NotifSettings::default()
            },
            ..Config::default()
        };
        assert_eq!(
            merge_owned(Config::default(), &full)
                .notifications
                .toast_style,
            ToastStyle::Full
        );
    }

    #[test]
    fn settings_window_round_trips_and_reads_leniently() {
        let place = WindowPlace {
            left: -1500,
            top: 40,
            right: -300,
            bottom: 700,
            maximized: true,
        };
        let c = Config {
            settings_window: Some(place),
            ..Config::default()
        };
        let back = parse(&serde_json::to_string(&c).unwrap());
        assert_eq!(back.settings_window, Some(place));
        // An absent key is None.
        assert_eq!(parse(r#"{"style":"Notch"}"#).settings_window, None);
        let win = |json: &str| parse(&format!(r#"{{"style":"Notch","settings_window":{json}}}"#));
        for bad in [
            "\"x\"",
            "7",
            "null",
            "[]",
            "{}",
            r#"{"left":0,"top":0,"right":-10,"bottom":10,"maximized":false}"#,
            r#"{"left":0,"top":0,"right":0,"bottom":10,"maximized":false}"#,
            r#"{"left":0,"top":0,"right":10,"bottom":0,"maximized":false}"#,
            r#"{"left":0,"top":0,"right":100001,"bottom":10,"maximized":false}"#,
            r#"{"left":-100001,"top":0,"right":10,"bottom":10,"maximized":false}"#,
            r#"{"left":0,"top":0,"right":10,"bottom":10}"#,
            r#"{"left":"a","top":0,"right":10,"bottom":10,"maximized":false}"#,
            r#"{"left":-2147483648,"top":0,"right":2147483647,"bottom":10,"maximized":false}"#,
        ] {
            let c = win(bad);
            assert_eq!(c.settings_window, None, "{bad}");
            assert_eq!(c.style, Style::Notch, "{bad}");
        }
        let edge =
            r#"{"left":-100000,"top":-100000,"right":100000,"bottom":100000,"maximized":false}"#;
        assert!(win(edge).settings_window.is_some());
    }

    #[test]
    fn the_lyrics_card_place_round_trips_and_reads_leniently() {
        let place = CardPlace {
            left: -1500,
            top: 40,
            right: -1200,
            bottom: 460,
        };
        let c = Config {
            lyrics_card: Some(place),
            ..Config::default()
        };
        let text = serde_json::to_string(&c).unwrap();
        assert_eq!(parse(&text).lyrics_card, Some(place));
        // An old config without the key, and a default one: None.
        assert_eq!(parse(r#"{"style":"Notch"}"#).lyrics_card, None);
        assert_eq!(Config::default().lyrics_card, None);
        let card = |json: &str| parse(&format!(r#"{{"style":"Notch","lyrics_card":{json}}}"#));
        for bad in [
            "\"x\"",
            "7",
            "null",
            "[]",
            "{}",
            r#"{"left":0,"top":0,"right":-10,"bottom":10}"#,
            r#"{"left":0,"top":0,"right":0,"bottom":10}"#,
            r#"{"left":0,"top":0,"right":10,"bottom":0}"#,
            r#"{"left":0,"top":0,"right":100001,"bottom":10}"#,
            r#"{"left":-100001,"top":0,"right":10,"bottom":10}"#,
            r#"{"left":0,"top":0,"right":10}"#,
            r#"{"left":"a","top":0,"right":10,"bottom":10}"#,
            r#"{"left":-2147483648,"top":0,"right":2147483647,"bottom":10}"#,
        ] {
            let c = card(bad);
            assert_eq!(c.lyrics_card, None, "{bad}");
            assert_eq!(c.style, Style::Notch, "{bad}: the rest of the file loads");
        }
        assert_eq!(
            card(r#"{"left":1,"top":2,"right":301,"bottom":422}"#).lyrics_card,
            Some(CardPlace {
                left: 1,
                top: 2,
                right: 301,
                bottom: 422
            })
        );
    }

    #[test]
    fn the_lyrics_card_place_belongs_to_the_island() {
        let place = CardPlace {
            left: 10,
            top: 10,
            right: 310,
            bottom: 430,
        };
        let mem = Config {
            lyrics_card: Some(place),
            ..Config::default()
        };
        assert_eq!(
            merge_owned(Config::default(), &mem).lyrics_card,
            Some(place)
        );
        // The island's None wins over a stale file value too.
        let disk = Config {
            lyrics_card: Some(place),
            ..Config::default()
        };
        assert_eq!(merge_owned(disk, &Config::default()).lyrics_card, None);
    }

    #[test]
    fn settings_window_is_not_owned_by_the_island() {
        let place = WindowPlace {
            left: 10,
            top: 10,
            right: 900,
            bottom: 700,
            maximized: false,
        };
        let disk = Config {
            settings_window: Some(place),
            ..Config::default()
        };
        let merged = merge_owned(disk, &Config::default());
        assert_eq!(merged.settings_window, Some(place));
    }

    #[test]
    fn muted_apps_read_leniently() {
        let muted = |json: &str| parse(&format!(r#"{{"style":"Notch","muted_apps":{json}}}"#));
        let default = vec!["Claude".to_string(), "Orca".to_string()];
        // Missing: the default; not a list: the default.
        assert_eq!(parse(r#"{"style":"Notch"}"#).muted_apps, default);
        for bad in ["\"Claude\"", "7", "null", "{}", "true"] {
            let c = muted(bad);
            assert_eq!(c.muted_apps, default, "{bad}");
            assert_eq!(c.style, Style::Notch, "{bad}");
        }
        // Items that are not strings go; names are trimmed; empty ones go.
        assert_eq!(
            muted(r#"["  Slack ", 5, null, "", "   ", ["x"], "Teams"]"#).muted_apps,
            vec!["Slack".to_string(), "Teams".to_string()]
        );
        // Repeats in any case go; the first spelling stays.
        assert_eq!(
            muted(r#"["Slack","SLACK","slack ","Teams","teams"]"#).muted_apps,
            vec!["Slack".to_string(), "Teams".to_string()]
        );
        // An explicit empty list means nothing is muted.
        assert!(muted("[]").muted_apps.is_empty());
        assert!(muted(r#"[5, ""]"#).muted_apps.is_empty());
    }

    #[test]
    fn adding_a_muted_app_trims_cuts_and_skips_repeats() {
        let list = vec!["Claude".to_string()];
        assert_eq!(
            with_muted(&list, "  Slack  "),
            Some(vec!["Claude".to_string(), "Slack".to_string()])
        );
        assert_eq!(with_muted(&list, "claude"), None, "any case");
        assert_eq!(with_muted(&list, "   "), None);
        assert_eq!(with_muted(&list, ""), None);
        let long = "x".repeat(100);
        let added = with_muted(&list, &long).expect("added");
        assert_eq!(added[1].chars().count(), MUTED_NAME_MAX);
        assert_eq!(with_muted(&added, &long), None, "the cut name is a repeat");
        assert_eq!(
            without_muted(&added, "CLAUDE "),
            Some(vec!["x".repeat(MUTED_NAME_MAX)]),
            "by name, any case"
        );
        assert_eq!(without_muted(&added, "Slack"), None);
    }

    #[test]
    fn language_is_a_lowercase_code() {
        let en = Config {
            language: Lang::En,
            ..Config::default()
        };
        let json = serde_json::to_string(&en).unwrap();
        assert!(json.contains(r#""language":"en""#), "{json}");
        assert_eq!(parse(r#"{"language":"en"}"#).language, Lang::En);
        assert_eq!(parse(r#"{"language":"id"}"#).language, Lang::Id);
        // A typo keeps the rest of the file: only the language falls back.
        let typo = parse(r#"{"style":"Notch","language":"inggris"}"#);
        assert_eq!((typo.style, typo.language), (Style::Notch, Lang::Id));
    }

    #[test]
    fn a_file_from_before_the_hotkeys_gets_the_old_presets_as_combos() {
        use crate::hotkey::{HotkeyId, Hotkeys, parse as hk};
        let combo = |t: &str| hk(t).unwrap();
        // Nothing named: the defaults.
        assert_eq!(parse(r#"{"style":"Notch"}"#).hotkeys, Hotkeys::default());
        assert_eq!(Config::default().hotkeys, Hotkeys::default());
        // Every old preset at once.
        let c = parse(
            r#"{"style":"Notch","quick_search_hotkey":"WinAltS","devices_hotkey":"CtrlAltD",
                "clipboard":{"hotkey":"WinAltV","enabled":false},"arrange":{"hotkey":"CtrlAltS","gap":4},
                "screenshot":{"print_screen":false,"show_secs":10}}"#,
        );
        assert_eq!(c.style, Style::Notch);
        assert_eq!(c.hotkeys.search, combo("Win+Alt+S"));
        assert_eq!(c.hotkeys.devices, combo("Ctrl+Alt+D"));
        assert_eq!(c.hotkeys.clipboard, combo("Win+Alt+V"));
        assert_eq!(c.hotkeys.arrange, combo("Ctrl+Alt+S"));
        assert_eq!(c.hotkeys.screenshot, None);
        // The lyrics card row did not exist then: it gets its default.
        assert_eq!(c.hotkeys.lyrics_card, combo("Ctrl+Alt+L"));
        // The rest of each group stays.
        assert!(!c.clipboard.enabled);
        assert_eq!((c.arrange.gap, c.screenshot.show_secs), (4, 10));
        // The search follows the new key, then the 3.2.0 one ("Off" was never a choice).
        assert_eq!(
            parse(r#"{"quick_search_hotkey":"Off"}"#).hotkeys.search,
            None
        );
        assert_eq!(
            parse(r#"{"search_hotkey":"Off"}"#).hotkeys.search,
            HotkeyId::Search.default()
        );
        assert_eq!(
            parse(r#"{"search_hotkey":"WinAltS"}"#).hotkeys.search,
            combo("Win+Alt+S")
        );
        assert_eq!(
            parse(r#"{"search_hotkey":"WinAltS","quick_search_hotkey":"Off"}"#)
                .hotkeys
                .search,
            None
        );
        let odd = parse(r#"{"quick_search_hotkey":"Nope","style":"Notch"}"#);
        assert_eq!((odd.hotkeys.search, odd.style), (None, Style::Notch));
    }

    #[test]
    fn a_hotkeys_key_wins_over_the_old_names_and_reads_leniently() {
        use crate::hotkey::{HotkeyId, Hotkeys, parse as hk};
        // Once the file has `hotkeys`, the old keys are not read again.
        let c = parse(
            r#"{"quick_search_hotkey":"WinAltS","devices_hotkey":"CtrlAltD",
                "hotkeys":{"search":"Ctrl+Alt+K","devices":"None"}}"#,
        );
        assert_eq!(c.hotkeys.search, hk("Ctrl+Alt+K").unwrap());
        assert_eq!(c.hotkeys.devices, None);
        assert_eq!(c.hotkeys.screenshot, HotkeyId::Screenshot.default());
        // A `hotkeys` object from before the lyrics card has no such key: default.
        assert_eq!(c.hotkeys.lyrics_card, HotkeyId::LyricsCard.default());
        // A bad shape anywhere never loses the rest of the file.
        for bad in [
            "null",
            "5",
            r#""x""#,
            "[]",
            r#"{"search":7,"clipboard":"Banana"}"#,
        ] {
            let c = parse(&format!(r#"{{"style":"Notch","hotkeys":{bad}}}"#));
            assert_eq!(c.style, Style::Notch, "{bad}");
            assert_eq!(c.hotkeys, Hotkeys::default(), "{bad}");
        }
        // A reserved row and a repeated row: the default or off, the rest kept.
        let c = parse(
            r#"{"style":"Notch","hotkeys":{"search":"Ctrl+Alt+K","clipboard":"Alt+Tab",
                "devices":"Ctrl+Alt+K","arrange":"Win+Alt+S","screenshot":"F13"}}"#,
        );
        assert_eq!(c.style, Style::Notch);
        assert_eq!(c.hotkeys.search, hk("Ctrl+Alt+K").unwrap());
        assert_eq!(c.hotkeys.clipboard, None);
        assert_eq!(c.hotkeys.devices, None);
        assert_eq!(c.hotkeys.arrange, hk("Win+Alt+S").unwrap());
        assert_eq!(c.hotkeys.screenshot, hk("F13").unwrap());
        // It is written as six strings, and the old keys are gone.
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json["hotkeys"]["devices"], serde_json::json!("None"));
        assert_eq!(
            json["hotkeys"]["lyrics_card"],
            serde_json::json!("Ctrl+Alt+L")
        );
        assert!(json.get("quick_search_hotkey").is_none());
        assert!(json.get("devices_hotkey").is_none());
        assert!(json["clipboard"].get("hotkey").is_none());
        assert!(json["screenshot"].get("print_screen").is_none());
        assert!(json["arrange"].get("hotkey").is_none());
        assert_eq!(parse(&json.to_string()), c);
    }

    #[test]
    fn scroll_volume_is_on_unless_switched_off() {
        assert!(Config::default().scroll_volume);
        assert!(parse(r#"{"style":"Notch"}"#).scroll_volume);
        assert!(!parse(r#"{"scroll_volume":false}"#).scroll_volume);
    }

    #[test]
    fn stay_behind_is_off_until_chosen_and_belongs_to_the_island() {
        assert!(!Config::default().stay_behind);
        assert!(!parse(r#"{"style":"Notch"}"#).stay_behind);
        assert!(parse(r#"{"stay_behind":true}"#).stay_behind);
        let mem = Config {
            stay_behind: true,
            ..Config::default()
        };
        assert!(merge_owned(Config::default(), &mem).stay_behind);
    }

    #[test]
    fn gestures_are_on_unless_switched_off() {
        assert!(Config::default().gestures);
        assert!(parse(r#"{"style":"Notch"}"#).gestures);
        assert!(!parse(r#"{"gestures":false}"#).gestures);
    }

    #[test]
    fn shortcut_tiles_default_empty_lenient_and_belong_to_the_island() {
        assert!(Config::default().shortcut_tiles.is_empty());
        assert!(parse(r#"{"style":"Notch"}"#).shortcut_tiles.is_empty());
        assert!(
            parse(r#"{"shortcut_tiles":"semua"}"#)
                .shortcut_tiles
                .is_empty()
        );
        let mixed = parse(
            r#"{"shortcut_tiles":[{"tile":"Lock","on":false},7,{"tile":"Custom","label":"Gmail","kind":"Url","target":"https://mail.google.com"}]}"#,
        );
        assert_eq!(mixed.shortcut_tiles.len(), 2);
        assert!(!mixed.shortcut_tiles[0].on);
        // An entry written by hand without "on" is shown.
        assert!(mixed.shortcut_tiles[1].on);
        let mem = Config {
            shortcut_tiles: mixed.shortcut_tiles.clone(),
            ..Config::default()
        };
        assert_eq!(
            merge_owned(Config::default(), &mem).shortcut_tiles,
            mixed.shortcut_tiles
        );
    }

    #[test]
    fn the_retired_vale_settings_tile_is_dropped_when_read_and_never_written_back() {
        for on in [true, false] {
            let text = format!(
                r#"{{"shortcut_tiles":[{{"tile":"Lock","on":true}},{{"tile":"ValeSettings","on":{on}}},{{"tile":" valesettings ","on":{on}}},{{"tile":"Emoji","on":false}},{{"tile":"GoShelf","on":true}}]}}"#
            );
            let c = try_parse(&text).expect("no error");
            let tiles: Vec<(&str, bool)> = c
                .shortcut_tiles
                .iter()
                .map(|e| (e.tile.as_str(), e.on))
                .collect();
            assert_eq!(
                tiles,
                [("Lock", true), ("Emoji", false), ("GoShelf", true)],
                "on={on}"
            );
            let json = serde_json::to_string(&c).unwrap();
            assert!(!json.to_lowercase().contains("valesettings"), "{json}");
        }
        // No shortcut_tiles at all still reads as the defaults (empty).
        assert!(
            try_parse(r#"{"style":"Notch"}"#)
                .unwrap()
                .shortcut_tiles
                .is_empty()
        );
    }

    #[test]
    fn an_old_file_with_the_removed_offset_key_still_loads_and_drops_the_key() {
        let text = r#"{"style":"Notch","lyrics":true,"lyrics_offset_ms":-300}"#;
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        let old = try_parse(text).expect("an unknown key is ignored, not an error");
        assert_eq!(old.style, Style::Notch);
        assert!(old.lyrics, "the other fields are kept");
        assert_eq!(value["lyrics_offset_ms"], -300);
        let saved = serde_json::to_string_pretty(&old).unwrap();
        assert!(!saved.contains("lyrics_offset_ms"), "{saved}");
        assert!(saved.contains("\"lyrics\": true"), "{saved}");
    }

    #[test]
    fn open_mode_reads_old_files_typos_and_round_trips() {
        assert_eq!(Config::default().open_mode, OpenMode::Hover);
        // A file from before the setting: the pointer behaves as always.
        let old = parse(r#"{"style":"Notch"}"#);
        assert_eq!((old.style, old.open_mode), (Style::Notch, OpenMode::Hover));
        for mode in OpenMode::ALL {
            let c = Config {
                open_mode: *mode,
                ..Config::default()
            };
            let json = serde_json::to_string(&c).unwrap();
            assert_eq!(parse(&json).open_mode, *mode, "{json}");
        }
        let json = serde_json::to_string(&Config {
            open_mode: OpenMode::HoverStay,
            ..Config::default()
        })
        .unwrap();
        assert!(json.contains(r#""open_mode":"HoverStay""#), "{json}");
        // A typo keeps the rest of the file.
        let typo = parse(r#"{"style":"Notch","open_mode":"klik"}"#);
        assert_eq!(
            (typo.style, typo.open_mode),
            (Style::Notch, OpenMode::Hover)
        );
        // The island's own setting: the file's stale value does not win.
        let disk = Config::default();
        let mem = Config {
            open_mode: OpenMode::Click,
            ..Config::default()
        };
        assert_eq!(merge_owned(disk, &mem).open_mode, OpenMode::Click);
    }

    type Owned = (
        Style,
        bool,
        bool,
        PomoSettings,
        Lang,
        Accent,
        Surface,
        WindowTheme,
        bool,
        bool,
        (Shell, CliStart),
        (bool, Motion, u32, bool, Option<GreetMark>),
    );

    fn owned(c: &Config) -> Owned {
        (
            c.style,
            c.start_with_windows,
            c.onboarding_done,
            c.pomodoro,
            c.language,
            c.accent,
            c.surface,
            c.window_theme,
            c.idle_clock,
            c.lyrics,
            (c.cli_shell, c.cli_start),
            (
                c.robot,
                c.robot_animation,
                c.robot_sleep_minutes,
                c.morning_greeting,
                c.last_greeting,
            ),
        )
    }

    /// The island started with an empty client ID; the owner then added one
    /// (and changed other fields) by hand while the island changed its own.
    fn disk_and_mem() -> (Config, Config) {
        let disk = Config {
            google_client_id: "id.apps.googleusercontent.com".into(),
            google_client_secret: "secret".into(),
            muted_apps: vec!["Slack".into()],
            battery_threshold: 35,
            // Stale copies of the island's own settings.
            style: Style::Pill,
            pomodoro: PomoSettings::default(),
            language: Lang::Id,
            accent: Accent::Terracotta,
            surface: Surface::Auto,
            window_theme: WindowTheme::System,
            idle_clock: true,
            lyrics: false,
            cli_shell: Shell::PowerShell,
            ..Config::default()
        };
        let mem = Config {
            style: Style::Notch,
            start_with_windows: false,
            onboarding_done: true,
            pomodoro: PomoSettings {
                focus_min: 20,
                ..PomoSettings::default()
            },
            language: Lang::En,
            accent: Accent::Olive,
            surface: Surface::Graphite,
            window_theme: WindowTheme::Light,
            idle_clock: false,
            lyrics: true,
            cli_shell: Shell::Cmd,
            cli_start: CliStart::Last,
            robot: false,
            robot_animation: Motion::Saver,
            robot_sleep_minutes: 30,
            morning_greeting: false,
            last_greeting: mark(2026, 10, 27, DayPeriod::Midday),
            ..Config::default()
        };
        (disk, mem)
    }

    #[test]
    fn merge_keeps_hand_edits_and_island_settings() {
        let (disk, mem) = disk_and_mem();
        let merged = merge_owned(disk.clone(), &mem);
        assert_eq!(merged.google_client_id, disk.google_client_id);
        assert_eq!(merged.google_client_secret, disk.google_client_secret);
        assert_eq!(merged.muted_apps, disk.muted_apps);
        assert_eq!(merged.battery_threshold, 35);
        assert_eq!(owned(&merged), owned(&mem));
    }

    /// A fresh directory under the system temp dir; removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static SEQ: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "vale-config-{name}-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn on_disk(path: &Path) -> Config {
        try_parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn classify_tells_missing_invalid_and_valid_apart() {
        let missing = io::Error::from(io::ErrorKind::NotFound);
        assert_eq!(classify(Err(missing)), OnDisk::Missing);
        for empty in ["", "  \r\n", "\u{feff}", "\u{feff} \n"] {
            assert_eq!(classify(Ok(empty.into())), OnDisk::Missing, "{empty:?}");
        }
        assert_eq!(
            classify(Ok("{\"google_client_id\": ".into())),
            OnDisk::Invalid
        );
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(classify(Err(denied)), OnDisk::Invalid);
        assert_eq!(
            classify(Ok("\u{feff}{\"style\":\"Notch\"}".into())),
            OnDisk::Valid(Config {
                style: Style::Notch,
                ..Config::default()
            })
        );
    }

    #[test]
    fn save_merges_into_a_valid_file() {
        let tmp = TempDir::new("valid");
        let path = tmp.file("config.json");
        let (disk, mem) = disk_and_mem();
        disk.save_to(&path).unwrap();
        let out = save_merged_at(&path, &mem);
        assert_eq!(out.repair, Repair::None);
        assert_eq!(out.config, merge_owned(disk, &mem));
        assert_eq!(on_disk(&path), out.config);
        assert!(
            !tmp.file("config.json.tmp").exists(),
            "atomic write cleaned up"
        );
    }

    #[test]
    fn a_hotkey_chosen_in_settings_survives_the_save() {
        use crate::hotkey::{HotkeyId, Hotkeys, parse as hk};
        let tmp = TempDir::new("hotkey");
        let path = tmp.file("config.json");
        // The defaults are on disk; the user turns the search off, then picks
        // Win+Alt+S and a PrtSc combo: each choice is what the file keeps.
        Config::default().save_to(&path).unwrap();
        let mut mine = Hotkeys::default();
        mine.set(HotkeyId::Search, None);
        let mut second = Hotkeys::default();
        second.set(HotkeyId::Search, hk("Win+Alt+S").unwrap());
        second.set(HotkeyId::Screenshot, hk("Ctrl+PrtSc").unwrap());
        for choice in [mine, second] {
            let mem = Config {
                hotkeys: choice,
                ..Config::default()
            };
            let out = save_merged_at(&path, &mem);
            assert_eq!(out.config.hotkeys, choice);
            assert_eq!(on_disk(&path).hotkeys, choice);
        }
        // merge_owned keeps the island's choice over the file's.
        let disk = Config::default();
        let mem = Config {
            hotkeys: second,
            ..Config::default()
        };
        assert_eq!(merge_owned(disk, &mem).hotkeys, second);
    }

    #[test]
    fn save_writes_a_missing_or_empty_file() {
        let tmp = TempDir::new("empty");
        let path = tmp.file("config.json");
        let (_, mem) = disk_and_mem();
        let out = save_merged_at(&path, &mem);
        assert_eq!(
            (out.config.clone(), out.repair),
            (mem.clone(), Repair::None)
        );
        assert_eq!(on_disk(&path), mem);
        // A truncated (empty) file is not "invalid": nothing in it to keep.
        for empty in ["", "\u{feff}\r\n"] {
            std::fs::write(&path, empty).unwrap();
            let out = save_merged_at(&path, &mem);
            assert_eq!(out.repair, Repair::None);
            assert_eq!(on_disk(&path), mem);
        }
        assert!(!tmp.file("config.json.bak").exists());
    }

    #[test]
    fn save_backs_up_an_invalid_file_then_writes() {
        let tmp = TempDir::new("invalid");
        let path = tmp.file("config.json");
        let (_, mem) = disk_and_mem();
        let typo = "{\"google_client_id\": \"abc\",, }";
        std::fs::write(&path, typo).unwrap();
        let out = save_merged_at(&path, &mem);
        let bak = tmp.file("config.json.bak");
        assert_eq!(out.repair, Repair::BackedUp(bak.clone()));
        assert_eq!(out.config, mem);
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), typo);
        assert_eq!(on_disk(&path), mem);
        // A second broken file never overwrites the first backup.
        std::fs::write(&path, "garbage").unwrap();
        let out = save_merged_at(&path, &mem);
        let bak2 = tmp.file("config.json.bak2");
        assert_eq!(out.repair, Repair::BackedUp(bak2.clone()));
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), typo);
        assert_eq!(std::fs::read_to_string(&bak2).unwrap(), "garbage");
        assert_eq!(on_disk(&path), mem);
    }

    #[test]
    fn save_writes_nothing_when_the_backup_fails() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = TempDir::new("blocked");
        let path = tmp.file("config.json");
        let (_, mem) = disk_and_mem();
        std::fs::write(&path, "{broken").unwrap();
        // Held open without delete sharing (an editor locking it): it can be
        // read, but not renamed.
        const FILE_SHARE_READ: u32 = 1;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        let out = save_merged_at(&path, &mem);
        assert_eq!(out.repair, Repair::Blocked);
        assert_eq!(out.config, mem, "the setting still holds for this run");
        drop(lock);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        assert!(!tmp.file("config.json.bak").exists());
        // Once the file is free, the next save repairs it.
        let out = save_merged_at(&path, &mem);
        assert!(matches!(out.repair, Repair::BackedUp(_)));
        assert_eq!(on_disk(&path), mem);
    }

    #[test]
    fn credential_save_keeps_other_hand_edits() {
        let tmp = TempDir::new("credentials");
        let path = tmp.file("config.json");
        let (disk, mem) = disk_and_mem();
        disk.save_to(&path).unwrap();
        let out = save_merged_with_at(&path, &mem, |c| {
            c.google_client_id = "new.apps.googleusercontent.com".into();
            c.google_client_secret = "new-secret".into();
        });
        assert_eq!(out.repair, Repair::None);
        let saved = on_disk(&path);
        assert_eq!(saved, out.config);
        assert_eq!(
            (
                saved.google_client_id.as_str(),
                saved.google_client_secret.as_str()
            ),
            ("new.apps.googleusercontent.com", "new-secret")
        );
        // Hand edits the island does not own stay as they were on disk...
        assert_eq!(saved.muted_apps, disk.muted_apps);
        assert_eq!(saved.battery_threshold, 35);
        // ...and the island's own settings come from memory, as in any save.
        assert_eq!(owned(&saved), owned(&mem));
    }

    #[test]
    fn muted_app_edits_keep_names_added_by_hand() {
        let tmp = TempDir::new("muted-edits");
        let path = tmp.file("config.json");
        // Vale started with [Claude, Orca]; Slack was then added by hand.
        let mem = Config {
            muted_apps: vec!["Claude".into(), "Orca".into()],
            ..Config::default()
        };
        let disk = Config {
            muted_apps: vec!["Claude".into(), "Orca".into(), "Slack".into()],
            ..mem.clone()
        };
        disk.save_to(&path).unwrap();
        let out = save_merged_with_at(&path, &mem, |c| add_muted(c, "Microsoft Outlook"));
        let want = ["Claude", "Orca", "Slack", "Microsoft Outlook"];
        assert_eq!(out.config.muted_apps, want);
        assert_eq!(on_disk(&path).muted_apps, want);
        // Orca removed by hand meanwhile: removing Slack (by name) leaves
        // Claude and Outlook, never a name picked by a stale index.
        let mut hand = on_disk(&path);
        hand.muted_apps.retain(|m| m != "Orca");
        hand.save_to(&path).unwrap();
        let out = save_merged_with_at(&path, &out.config, |c| remove_muted(c, "slack"));
        assert_eq!(out.config.muted_apps, ["Claude", "Microsoft Outlook"]);
        assert_eq!(on_disk(&path).muted_apps, ["Claude", "Microsoft Outlook"]);
    }

    #[test]
    fn edit_applies_to_a_missing_file() {
        let tmp = TempDir::new("edit-missing");
        let path = tmp.file("config.json");
        let (_, mem) = disk_and_mem();
        let out = save_merged_with_at(&path, &mem, |c| c.battery_threshold = 45);
        assert_eq!(out.config.battery_threshold, 45);
        assert_eq!(on_disk(&path).battery_threshold, 45);
    }

    #[test]
    fn pomodoro_round_trip() {
        let c = Config {
            pomodoro: PomoSettings {
                focus_min: 45,
                short_break_min: 7,
                long_break_min: 20,
                sessions: 6,
            },
            ..Config::default()
        };
        let back = parse(&serde_json::to_string_pretty(&c).unwrap());
        assert_eq!(back.pomodoro, c.pomodoro);
        assert_eq!(back, c);
        // A partial pomodoro object keeps the missing fields at their defaults.
        let partial = parse(r#"{"pomodoro":{"focus_min":40}}"#);
        assert_eq!(
            partial.pomodoro,
            PomoSettings {
                focus_min: 40,
                ..PomoSettings::default()
            }
        );
    }

    #[test]
    fn notch_defaults_and_old_files() {
        let c = Config::default();
        assert_eq!(
            (c.notch_width, c.notch_height, c.notch_fill, c.notch_opacity),
            (190, 34, NotchFill::Solid, 30)
        );
        // A file from before these fields loads unchanged.
        let old = parse(r#"{"style":"Notch","accent":"Plum"}"#);
        assert_eq!(
            (
                old.notch_width,
                old.notch_height,
                old.notch_fill,
                old.notch_opacity
            ),
            (190, 34, NotchFill::Solid, 30)
        );
    }

    #[test]
    fn notch_values_are_clamped_or_fall_back() {
        let c = parse(r#"{"notch_width":9999,"notch_height":-5,"notch_opacity":250}"#);
        assert_eq!(
            (c.notch_width, c.notch_height, c.notch_opacity),
            (300, 28, 100)
        );
        let c = parse(r#"{"notch_width":10,"notch_height":500,"notch_opacity":-3}"#);
        assert_eq!(
            (c.notch_width, c.notch_height, c.notch_opacity),
            (160, 44, 0)
        );
        let c = parse(r#"{"notch_width":"wide","notch_height":null,"notch_opacity":"x"}"#);
        assert_eq!(
            (c.notch_width, c.notch_height, c.notch_opacity),
            (190, 34, 30)
        );
        let c = parse(r#"{"notch_width":200.6,"notch_height":30.2,"notch_opacity":55.5}"#);
        assert_eq!(
            (c.notch_width, c.notch_height, c.notch_opacity),
            (201, 30, 56)
        );
        assert_eq!(
            parse(r#"{"notch_fill":"Transparent"}"#).notch_fill,
            NotchFill::Transparent
        );
        assert_eq!(
            parse(r#"{"notch_fill":"Glass"}"#).notch_fill,
            NotchFill::Solid
        );
    }

    #[test]
    fn notch_monitor_defaults_to_primary_and_old_files_load() {
        assert_eq!(Config::default().notch_monitor, NotchMonitor::Primary);
        let old = parse(r#"{"style":"Notch","accent":"Plum"}"#);
        assert_eq!(old.notch_monitor, NotchMonitor::Primary);
        let json = serde_json::to_string(&Config::default()).unwrap();
        assert!(json.contains(r#""notch_monitor":"primary""#), "{json}");
    }

    #[test]
    fn notch_monitor_round_trips_both_variants() {
        let chosen = NotchMonitor::Chosen {
            path: r"\\?\DISPLAY#DEL4123#5&abc&0&UID4352#{guid}".into(),
            name: "DELL U2723QE".into(),
        };
        for m in [NotchMonitor::Primary, chosen.clone()] {
            let c = Config {
                notch_monitor: m.clone(),
                ..Config::default()
            };
            let back = parse(&serde_json::to_string_pretty(&c).unwrap());
            assert_eq!(back.notch_monitor, m);
        }
        let json = serde_json::to_value(&chosen).unwrap();
        assert_eq!(json["name"], "DELL U2723QE");
        assert!(json["path"].as_str().unwrap().starts_with(r"\\?\DISPLAY"));
    }

    #[test]
    fn notch_monitor_garbage_reads_as_primary_and_keeps_the_rest() {
        for junk in [
            "null",
            "5",
            "true",
            "[]",
            r#"["path"]"#,
            r#""Primary2""#,
            r#""chosen""#,
            "{}",
            r#"{"path":""}"#,
            r#"{"path":5}"#,
            r#"{"path":null,"name":"x"}"#,
            r#"{"name":"only"}"#,
        ] {
            let c = parse(&format!(r#"{{"style":"Notch","notch_monitor":{junk}}}"#));
            assert_eq!(c.notch_monitor, NotchMonitor::Primary, "{junk}");
            assert_eq!(c.style, Style::Notch, "{junk}");
        }
        let chosen = |name: &str| NotchMonitor::Chosen {
            path: "p".into(),
            name: name.into(),
        };
        let c = parse(r#"{"notch_monitor":{"path":"p","name":7}}"#);
        assert_eq!(c.notch_monitor, chosen(""));
        let c = parse(r#"{"notch_monitor":{"path":"p"}}"#);
        assert_eq!(c.notch_monitor, chosen(""));
    }

    #[test]
    fn merge_owned_keeps_the_notch_monitor() {
        let mem = Config {
            notch_monitor: NotchMonitor::Chosen {
                path: "p".into(),
                name: "n".into(),
            },
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert_eq!(merged.notch_monitor, mem.notch_monitor);
    }

    #[test]
    fn merge_owned_keeps_the_notch_settings() {
        let mem = Config {
            notch_width: 250,
            notch_height: 40,
            notch_fill: NotchFill::Transparent,
            notch_opacity: 70,
            ..Config::default()
        };
        let merged = merge_owned(Config::default(), &mem);
        assert_eq!(
            (
                merged.notch_width,
                merged.notch_height,
                merged.notch_fill,
                merged.notch_opacity
            ),
            (250, 40, NotchFill::Transparent, 70)
        );
        let back = parse(&serde_json::to_string_pretty(&mem).unwrap());
        assert_eq!(back, mem);
    }

    #[test]
    fn cat_fields_default_lenient_and_belong_to_the_island() {
        use crate::theme::{CatAccessory, CatColor};
        let cat = |c: &Config| {
            (
                c.cat_color,
                c.cat_accessory,
                c.cat_follow_pointer,
                c.notch_peek,
            )
        };
        let d = Config::default();
        assert_eq!(
            cat(&d),
            (CatColor::Terracotta, CatAccessory::None, true, true)
        );
        // An old file has none of them.
        let old = parse(r#"{"style":"Notch","robot":false}"#);
        assert_eq!(
            cat(&old),
            (CatColor::Terracotta, CatAccessory::None, true, true)
        );
        // Names are case-insensitive; a typo falls back per field and keeps
        // the rest of the file (a bad value never drops it).
        let hand = parse(r#"{"cat_color":"calico","cat_accessory":"CROWN","notch_peek":false}"#);
        assert_eq!(
            cat(&hand),
            (CatColor::Calico, CatAccessory::Crown, true, false)
        );
        let typo = parse(
            r#"{"style":"Notch","cat_color":"purple","cat_accessory":7,"cat_follow_pointer":"yes","notch_peek":null}"#,
        );
        assert_eq!(typo.style, Style::Notch);
        assert_eq!(
            cat(&typo),
            (CatColor::Terracotta, CatAccessory::None, true, true)
        );
        // Round trip, and the island keeps its own copies over the file's.
        let mem = Config {
            cat_color: CatColor::Black,
            cat_accessory: CatAccessory::Scarf,
            cat_follow_pointer: false,
            notch_peek: false,
            ..Config::default()
        };
        assert_eq!(parse(&serde_json::to_string_pretty(&mem).unwrap()), mem);
        let merged = merge_owned(Config::default(), &mem);
        assert_eq!(
            cat(&merged),
            (CatColor::Black, CatAccessory::Scarf, false, false)
        );
    }

    #[test]
    fn cat_dance_defaults_on_reads_leniently_and_round_trips() {
        assert!(Config::default().cat_dance);
        // A file from before the setting has no key.
        assert!(parse(r#"{"style":"Notch","robot":false}"#).cat_dance);
        // Junk reads as on and does not drop the rest of the file.
        for junk in [r#""yes""#, "null", "7", r#""false""#, "[]"] {
            let c = parse(&format!(r#"{{"style":"Notch","cat_dance":{junk}}}"#));
            assert!(c.cat_dance, "{junk}");
            assert_eq!(c.style, Style::Notch);
        }
        // Only a real false is off.
        assert!(!parse(r#"{"cat_dance":false}"#).cat_dance);
        assert!(parse(r#"{"cat_dance":true}"#).cat_dance);
        let mem = Config {
            cat_dance: false,
            ..Config::default()
        };
        assert_eq!(parse(&serde_json::to_string_pretty(&mem).unwrap()), mem);
        // The island keeps its own copy over the file's.
        assert!(!merge_owned(Config::default(), &mem).cat_dance);
        assert!(merge_owned(mem.clone(), &Config::default()).cat_dance);
    }

    #[test]
    fn eq_follow_audio_defaults_on_reads_leniently_and_round_trips() {
        assert!(Config::default().eq_follow_audio);
        // A file from before the setting has no key.
        assert!(parse(r#"{"style":"Notch","robot":false}"#).eq_follow_audio);
        // Junk keeps the default and does not drop the rest of the file.
        for junk in [r#""yes""#, "null", "7", r#""false""#, "[]"] {
            let c = parse(&format!(r#"{{"style":"Notch","eq_follow_audio":{junk}}}"#));
            assert!(c.eq_follow_audio, "{junk}");
            assert_eq!(c.style, Style::Notch);
        }
        let off = parse(r#"{"eq_follow_audio":false}"#);
        assert!(!off.eq_follow_audio);
        let mem = Config {
            eq_follow_audio: false,
            ..Config::default()
        };
        assert_eq!(parse(&serde_json::to_string_pretty(&mem).unwrap()), mem);
        // The island keeps its own copy over the file's.
        assert!(!merge_owned(Config::default(), &mem).eq_follow_audio);
        assert!(merge_owned(mem.clone(), &Config::default()).eq_follow_audio);
    }

    #[test]
    fn ui_font_defaults_to_sans_reads_leniently_and_belongs_to_the_island() {
        assert_eq!(Config::default().font, UiFont::Sans);
        // An old file without the key loads unchanged.
        let old = parse(r#"{"style":"Notch","accent":"Sage","robot":false}"#);
        assert_eq!(old.font, UiFont::Sans);
        assert_eq!(
            old,
            Config {
                style: Style::Notch,
                accent: Accent::Sage,
                robot: false,
                ..Config::default()
            }
        );
        // Names are case-insensitive and trimmed.
        for name in ["Mono", "mono", " MONO "] {
            let c = parse(&format!(r#"{{"font":"{name}"}}"#));
            assert_eq!(c.font, UiFont::Mono, "{name:?}");
        }
        assert_eq!(parse(r#"{"font":"sans"}"#).font, UiFont::Sans);
        // A typo, a number, null or an object reads Sans and keeps the rest.
        for junk in [r#""xyz""#, "3", "null", "{}", "[]", "true", r#""""#] {
            let c = parse(&format!(
                r#"{{"style":"Notch","accent":"Sage","font":{junk}}}"#
            ));
            assert_eq!(c.font, UiFont::Sans, "{junk}");
            assert_eq!(c.style, Style::Notch, "{junk}");
            assert_eq!(c.accent, Accent::Sage, "{junk}");
        }
        // Saving writes the canonical name, and it round trips.
        let hand = parse(r#"{"font":" MONO "}"#);
        let json = serde_json::to_string(&hand).unwrap();
        assert!(json.contains(r#""font":"Mono""#), "{json}");
        assert_eq!(parse(&json), hand);
        let json = serde_json::to_string(&Config::default()).unwrap();
        assert!(json.contains(r#""font":"Sans""#), "{json}");
        // The island keeps its own copy over the file's.
        let mono = Config {
            font: UiFont::Mono,
            ..Config::default()
        };
        assert_eq!(merge_owned(Config::default(), &mono).font, UiFont::Mono);
        assert_eq!(merge_owned(mono, &Config::default()).font, UiFont::Sans);
    }

    #[test]
    fn surface_reads_auto_unless_the_file_names_one() {
        let old = parse(r#"{"accent":"Sage","surface":"Espresso"}"#);
        assert_eq!(old.surface, Surface::Espresso, "a stored Espresso stays");
        let graphite = parse(r#"{"surface":"Graphite"}"#);
        assert_eq!(graphite.surface, Surface::Graphite);
        assert_eq!(parse(r#"{"surface":"Black"}"#).surface, Surface::Espresso);
        assert_eq!(parse(r#"{"accent":"Sage"}"#).surface, Surface::Auto);
        assert_eq!(parse(r#"{"surface":"mauve"}"#).surface, Surface::Auto);
        assert_eq!(parse(r#"{"surface":"sea"}"#).surface, Surface::Sea);
        assert_eq!(parse(r#"{"surface":"Plum"}"#).surface, Surface::Plum);
        assert_eq!(
            parse(r#"{"surface":"Terminal"}"#).surface,
            Surface::Terminal
        );
        assert_eq!(
            parse(r#"{"surface":"terminal"}"#).surface,
            Surface::Terminal
        );
        let saved = serde_json::to_string(&Config {
            surface: Surface::Terminal,
            ..Config::default()
        })
        .unwrap();
        assert!(saved.contains(r#""surface":"Terminal""#), "{saved}");
        assert_eq!(parse(&saved).surface, Surface::Terminal);
        assert_eq!(parse(r#"{"surface":"Terminals"}"#).surface, Surface::Auto);
        assert_eq!(parse(r#"{"surface":null}"#).surface, Surface::Auto);
        let json = serde_json::to_string(&Config::default()).unwrap();
        assert!(json.contains(r#""surface":"Auto""#), "{json}");
    }
}
