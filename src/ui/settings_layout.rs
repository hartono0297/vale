//! Layout of the settings window in logical pixels (96 DPI): where each card,
//! text and control of a section goes, and what a click on it means. Pure,
//! so it is tested in both languages; `settings.rs` only draws what it says.
//!
//! Text widths come from a `Measure`: DirectWrite in the window, a
//! deliberately generous estimate in tests. Controls are sized to their
//! labels and pushed to the right edge of their card; row labels take the
//! space left over, so the tests check that the labels fit with the estimate.

use crate::arrange::{ArrangeSettings, Dir, MAX_TEMPLATES, Node};
use crate::calendar::CalStatus;
use crate::claude::install::HookState;
use crate::cli::Shell;
use crate::cli_folders::CliStart;
use crate::clipboard::store::{ITEMS_STEP, MAX_ITEMS, MIN_ITEMS};
use crate::config::{
    ClipboardSettings, DISK_WARN_RANGE, NotchFill, NotifSettings, OpenMode, RAM_WARN_RANGE,
    SHOT_SECS_RANGE, ScreenshotSettings, ShotIgnore, ShotTool, Style, ToastStyle,
};
use crate::hotkey::{Hotkey, HotkeyId, Hotkeys};
use crate::i18n::{
    Key, Lang, text, update_available, update_downloading, update_installed, version_label,
};
use crate::model::{Page, PomoField};
use crate::notif::store::{CAP_STEP, MAX_CAP as NOTIF_MAX, MIN_CAP as NOTIF_MIN};
use crate::pages::PanelPages;
use crate::pomodoro::{self, PomoSettings};
use crate::theme::{Accent, CatAccessory, CatColor, Surface, WindowTheme};
use crate::tiles::{CustomKind, MAX_CUSTOM, MAX_SHOWN, ShortcutTiles, Tile, TileGroup, group};
use crate::ui::geometry::{
    NOTCH_H_DEFAULT, NOTCH_H_MAX, NOTCH_H_MIN, NOTCH_W_DEFAULT, NOTCH_W_MAX, NOTCH_W_MIN,
};
use crate::ui::icons::Icon;
use crate::ui::robot::{Motion, SLEEP_MAX, SLEEP_MIN};

pub use super::geometry::Rect;

/// The smallest client area the window can have (the window's minimum
/// track size); a bigger window lays out against its real client size
/// (`layout_in`).
pub const CLIENT_W: f32 = 720.0;
pub const CLIENT_H: f32 = 520.0;
/// The content column stops growing here and is centred in the space beside
/// the sidebar.
const MAX_CONTENT_W: f32 = 760.0;
pub const SIDEBAR_W: f32 = 184.0;
const PAD: f32 = 24.0;
const CARD_GAP: f32 = 12.0;
pub const ROW_H: f32 = 52.0;

// Studio radius scale (docs/design/studio-ui.html): card 14, control 10
// (segmented track, sidebar item, text field), the segment inside a track 8,
// and chips, buttons, toggles and stepper buttons fully rounded. `FULL` is
// clamped to half the shape's height when drawn.
pub const CARD_RADIUS: f32 = 14.0;
pub const CONTROL_RADIUS: f32 = 10.0;
pub const SEGMENT_RADIUS: f32 = 8.0;
pub const FULL_RADIUS: f32 = 999.0;

/// Horizontal padding inside a card.
const INSET: f32 = 16.0;
/// The column at the minimum client size (what the tests measure against).
#[cfg(test)]
const CONTENT_X: f32 = SIDEBAR_W + PAD;
#[cfg(test)]
const CONTENT_W: f32 = CLIENT_W - CONTENT_X - PAD;
const TITLE_Y: f32 = 20.0;
const TITLE_H: f32 = 32.0;
const FIRST_CARD_Y: f32 = TITLE_Y + TITLE_H + 16.0;
/// A section taller than the window scrolls under the title, from here down.
pub const VIEW_TOP: f32 = TITLE_Y + TITLE_H + 8.0;
/// One wheel notch scrolls one row.
const WHEEL_STEP: f32 = ROW_H;
/// The scrollbar thumb at the content's right edge.
const SCROLLBAR_W: f32 = 3.0;
/// Segmented controls and steppers.
pub const CONTROL_H: f32 = 28.0;
pub const BUTTON_H: f32 = 30.0;
/// Inner padding of a segmented control's track.
const SEG_PAD: f32 = 2.0;
const SEG_TEXT_PAD: f32 = 14.0;
const SEG_MIN_W: f32 = 64.0;
/// Segments share one width unless that makes the control wider than this.
const SEG_EQUAL_MAX: f32 = 280.0;
pub const TOGGLE_W: f32 = 40.0;
pub const TOGGLE_H: f32 = 22.0;
pub const SWATCH: f32 = 22.0;
const SWATCH_GAP: f32 = 10.0;
/// Swatches take clicks a little outside the circle; the selection ring
/// (a 2 px gap and a 1.5 px line) stays inside this margin.
const SWATCH_SLOP: f32 = 4.0;
pub const SWATCH_RING_GAP: f32 = 2.0;
pub const SWATCH_RING_W: f32 = 1.5;
pub const STEP_BUTTON: f32 = 28.0;
const STEP_VALUE_W: f32 = 64.0;
const BUTTON_PAD: f32 = 16.0;
const BUTTON_MIN_W: f32 = 80.0;
/// An icon-only button (the About links).
const ICON_BUTTON: f32 = 32.0;
pub const EXTERNAL_GLYPH: f32 = 16.0;
const LINK_PAD: f32 = 8.0;
/// Space taken by the glyph after a link's label.
pub const LINK_GLYPH_W: f32 = 18.0;
const GAP: f32 = 8.0;
/// A description that wraps: each further line adds this to its row.
const DESC_LINE_H: f32 = 14.0;
const LOGO: f32 = 28.0;
const NAV_X: f32 = 10.0;
const NAV_W: f32 = SIDEBAR_W - 2.0 * NAV_X;
const NAV_Y: f32 = 72.0;
const NAV_H: f32 = 36.0;
const NAV_GAP: f32 = 2.0;
const PREVIEW_H: f32 = 112.0;
const FIELD_ROW_H: f32 = 44.0;
const FIELD_H: f32 = 32.0;
/// The uppercase label above a card (Integrations).
const EYEBROW_H: f32 = 18.0;
const EYEBROW_GAP: f32 = 8.0;
/// A status chip: dot, gap, text, with this padding on each side.
pub const CHIP_H: f32 = 20.0;
const CHIP_PAD: f32 = 8.0;
pub const DOT: f32 = 6.0;
const DOT_GAP: f32 = 5.0;
const ABOUT_LOGO: f32 = 64.0;
const BATTERY_RANGE: (u8, u8, u8) = (5, 50, 5);
/// The first row of a list card: its count and "Kembalikan bawaan".
const HEADER_H: f32 = 48.0;
/// A sub-header inside a card ("Tersembunyi", a group of "Tersedia").
const SUBHEAD_H: f32 = 32.0;
/// The drag handle's column: a row's label starts after it.
const GRIP_W: f32 = 28.0;
/// A page's Dock icon in the pages card, and where it and the name start
/// (from the card's left edge): icon, 10 dip gap, name.
const PAGE_ICON: f32 = 18.0;
const PAGE_ICON_X: f32 = GRIP_W + 6.0;
const PAGE_NAME_X: f32 = PAGE_ICON_X + PAGE_ICON + 10.0;
/// Padding above the own-shortcut form's first row.
const FORM_PAD: f32 = 6.0;

pub const REPO_URL: &str = "https://github.com/hartono0297/vale";
pub const GUIDE_URL: &str = "https://github.com/hartono0297/vale#setup-google-calendar";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// Umum: language, autostart, the warnings.
    General,
    /// Tampilan: the island's look (style, colours, the notch, the window).
    Appearance,
    /// Kucing: the cat's colour, accessory, motion and greeting.
    Cat,
    /// Panel: how it opens, its pages, its shortcut tiles.
    Panel,
    Clipboard,
    /// Tombol pintas: the five global hotkeys, each recorded by pressing it.
    Hotkeys,
    /// Fitur: the pomodoro, music, the CLI, devices, the cleaner.
    Features,
    Integrations,
    About,
}

impl Section {
    pub const ALL: [Section; 9] = [
        Section::General,
        Section::Appearance,
        Section::Cat,
        Section::Panel,
        Section::Clipboard,
        Section::Hotkeys,
        Section::Features,
        Section::Integrations,
        Section::About,
    ];

    pub fn key(self) -> Key {
        match self {
            Section::General => Key::SecGeneral,
            Section::Appearance => Key::SecAppearance,
            Section::Cat => Key::SecCat,
            Section::Panel => Key::SecPanel,
            Section::Clipboard => Key::SecClipboard,
            Section::Hotkeys => Key::SecHotkeys,
            Section::Features => Key::SecFeatures,
            Section::Integrations => Key::SecIntegrations,
            Section::About => Key::SecAbout,
        }
    }

    /// The section's icon in the sidebar.
    pub fn icon(self) -> Icon {
        match self {
            Section::General => Icon::Gear,
            Section::Appearance => Icon::Palette,
            Section::Cat => Icon::Cat,
            Section::Panel => Icon::Grid,
            Section::Clipboard => Icon::Clipboard,
            Section::Hotkeys => Icon::Keyboard,
            Section::Features => Icon::Sparkle,
            Section::Integrations => Icon::Link,
            Section::About => Icon::Info,
        }
    }

    pub fn index(self) -> usize {
        Section::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// "appearance", "general", ... (also accepts the Indonesian names), for
    /// the debug `--settings` mode. "pomodoro" (a section before v3.8) is
    /// Features now.
    #[cfg(any(test, debug_assertions))]
    pub fn from_name(name: &str) -> Option<Section> {
        match name.trim().to_ascii_lowercase().as_str() {
            "general" | "umum" => Some(Section::General),
            "appearance" | "tampilan" => Some(Section::Appearance),
            "cat" | "kucing" => Some(Section::Cat),
            "panel" => Some(Section::Panel),
            "clipboard" => Some(Section::Clipboard),
            // "shortcuts" is not here: the tiles are "Pintasan".
            "hotkeys" | "tombol-pintas" | "tombolpintas" | "shortcut-keys" => {
                Some(Section::Hotkeys)
            }
            "features" | "fitur" | "pomodoro" => Some(Section::Features),
            "integrations" | "integrasi" => Some(Section::Integrations),
            "about" | "tentang" => Some(Section::About),
            _ => None,
        }
    }
}

/// The face the Kucing page's preview shows, chosen with its chips: window
/// state only, never saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CatFace {
    #[default]
    Idle,
    Happy,
    Blink,
    Sleep,
    Wave,
}

impl CatFace {
    pub const ALL: [CatFace; 5] = [
        CatFace::Idle,
        CatFace::Happy,
        CatFace::Blink,
        CatFace::Sleep,
        CatFace::Wave,
    ];

    pub fn key(self) -> Key {
        match self {
            CatFace::Idle => Key::ExprIdle,
            CatFace::Happy => Key::ExprHappy,
            CatFace::Blink => Key::ExprBlink,
            CatFace::Sleep => Key::ExprSleep,
            CatFace::Wave => Key::ExprWave,
        }
    }

    /// The face, and whether its eyes are shut for a blink.
    pub fn expr(self) -> (super::robot::Expr, bool) {
        use super::robot::Expr;
        match self {
            CatFace::Idle => (Expr::Idle, false),
            CatFace::Happy => (Expr::Happy, false),
            CatFace::Blink => (Expr::Idle, true),
            CatFace::Sleep => (Expr::Sleep, false),
            CatFace::Wave => (Expr::Wave, false),
        }
    }
}

/// Text styles; `settings.rs` maps each to a DirectWrite format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Font {
    Brand,
    Nav,
    Title,
    Label,
    Desc,
    Control,
    Value,
    AboutName,
    AboutVersion,
    /// The uppercase label above a card ("GOOGLE CALENDAR").
    Eyebrow,
}

impl Font {
    pub const ALL: [Font; 10] = [
        Font::Brand,
        Font::Nav,
        Font::Title,
        Font::Label,
        Font::Desc,
        Font::Control,
        Font::Value,
        Font::AboutName,
        Font::AboutVersion,
        Font::Eyebrow,
    ];

    /// Studio scale: the display style (DM Sans 600 upright, tight
    /// tracking) at 20 for titles and the About name and 16 for the brand,
    /// DM Sans 12 for text and 10.5 for descriptions.
    pub fn size(self) -> f32 {
        match self {
            Font::Brand => 16.0,
            Font::Title | Font::AboutName => 20.0,
            Font::Nav | Font::Label | Font::Control | Font::Value | Font::AboutVersion => 12.0,
            Font::Desc | Font::Eyebrow => 10.5,
        }
    }

    /// The display style: DM Sans 600 upright with tight (negative)
    /// tracking. The renderer reads this to pick the tracking; the face and
    /// weight are the same DM Sans as the body text.
    pub fn display(self) -> bool {
        matches!(self, Font::Brand | Font::Title | Font::AboutName)
    }

    /// CSS-style weight: 400, 500 or 600 (the weights embedded).
    pub fn weight(self) -> u16 {
        match self {
            Font::Brand | Font::Title | Font::AboutName | Font::Eyebrow => 600,
            Font::Label => 500,
            Font::Nav | Font::Desc | Font::Control | Font::Value | Font::AboutVersion => 400,
        }
    }

    /// Numbers line up (tabular figures): the stepper values.
    pub fn tabular(self) -> bool {
        self == Font::Value
    }
}

pub trait Measure {
    /// Width of `text` on one line in `font`, in logical pixels.
    fn width(&self, text: &str, font: Font) -> f32;
}

/// A generous width estimate (wider than DM Sans or the Segoe UI Variable
/// fallback draw nearly any text, the display style included), so a layout
/// that fits with it also fits on screen.
pub struct Estimate;

impl Measure for Estimate {
    fn width(&self, text: &str, font: Font) -> f32 {
        let per_char = if font == Font::Eyebrow {
            // All capitals.
            0.8
        } else if font.weight() >= 500 {
            // Medium and semibold, the display style included. Measured
            // from DMSans-600.ttf with the display style's -2.5% tracking,
            // the titles and brand take 0.45 to 0.57 em per character, and
            // the Segoe UI Variable Display semibold fallback is about as
            // wide; 0.66 leaves over 15% for wide letters (m, w) and
            // rounding.
            0.66
        } else {
            0.62
        };
        text.chars().count() as f32 * font.size() * per_char
    }
}

/// What a click on a control does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Nav(Section),
    Style(Style),
    Accent(Accent),
    Surface(Surface),
    /// Settings → Kucing: "Warna" (a coat) and "Aksesori" (a tile).
    CatColor(CatColor),
    CatAccessory(CatAccessory),
    /// "Ikuti kursor" and "Mengecil jadi mata".
    CatFollow,
    CatDance,
    NotchPeek,
    /// "Ubah" beside "Latar: ...": goes to Tampilan.
    CatBackdrop,
    /// A chip of the preview: which face it shows (window state).
    CatFace(CatFace),
    /// A slider: the press and the drag are handled by the window (a press
    /// jumps there, a drag follows), not as a click.
    Slider(SliderId),
    /// "Ukuran bawaan": 190 x 34 again.
    NotchReset,
    /// "Isi notch": Solid or Tembus.
    NotchFill(NotchFill),
    WindowMode(WindowTheme),
    Lang(Lang),
    Autostart,
    IdleClock,
    /// "Equalizer ikut irama".
    EqFollowAudio,
    /// "Di belakang jendela".
    StayBehind,
    Lyrics,
    /// "Mode buka": Hover, Hover & tetap or Klik.
    OpenMode(OpenMode),
    /// Settings -> Tombol pintas: a row's field (click to record, again to
    /// cancel), its reset button, and "Kembalikan semua".
    HotkeyField(HotkeyId),
    HotkeyReset(HotkeyId),
    HotkeysResetAll,
    /// "Notifikasi perangkat": the plug-in and safe-to-remove notices.
    DeviceNotices,
    /// Settings -> Fitur -> Susun: "+ Template baru", a
    /// template's "Ubah" and "Hapus", a saved arrangement's "Ganti nama"
    /// and "Hapus" (by place in their lists), and the editor's "Belah"
    /// buttons, "Gabung", "Simpan" and "Batal".
    ArrangeTemplateNew,
    ArrangeTemplateEdit(usize),
    ArrangeTemplateDelete(usize),
    ArrangeSavedRename(usize),
    ArrangeSavedDelete(usize),
    ArrangeSplit(Dir),
    ArrangeMerge,
    ArrangeEditorSave,
    ArrangeEditorCancel,
    /// "Folder awal CLI": Ikuti Explorer or Folder terakhir.
    CliStart(CliStart),
    Shell(Shell),
    /// "Kucing di notch".
    Robot,
    /// "Animasi": Penuh, Hemat, Mati.
    Motion(Motion),
    /// "Tidur setelah", one step down (−1) or up.
    RobotSleep(i32),
    /// "Sapaan".
    Greeting,
    /// Settings → Clipboard: "Riwayat clipboard", "Jumlah item" (−1 / +1),
    /// "Simpan setelah restart", "Hapus saat layar dikunci",
    /// "Notifikasi salinan", "Disalin" for every copy, and "Hapus riwayat".
    ClipEnabled,
    ClipItems(i32),
    ClipPersist,
    ClipClearOnLock,
    ClipNotice,
    ClipFeedbackAll,
    ClipClear,
    /// Settings → Fitur → Notifikasi: "Simpan notifikasi", "Jumlah maksimal"
    /// (−1 / +1), "Simpan setelah restart", the remove button of an excluded
    /// app (its place in the list), "Tambah" and "Hapus riwayat notifikasi".
    NotifRecord,
    NotifItems(i32),
    NotifPersist,
    NotifMutedRemove(usize),
    NotifMutedAdd,
    NotifClear,
    /// Settings → Clipboard → Screenshot: "Preview di notch", "Jika
    /// diabaikan" (Simpan / Hapus) and "Lama tampil" (−1 / +1).
    ShotPreview,
    ShotIgnore(ShotIgnore),
    ShotSecs(i32),
    /// "Alat screenshot" (Vale / Snipping Tool).
    ShotTool(ShotTool),
    /// "Notifikasi saat notch dipakai" (Ringkas / Penuh).
    NotifToastStyle(ToastStyle),
    Battery(i32),
    /// "Peringatan disk" / "Peringatan RAM", one step down (−1) or up.
    DiskWarn(i32),
    RamWarn(i32),
    /// "Lindungi cache Cargo & Autodesk di Mole".
    MoleProtect,
    /// "Halaman panel": a page's "Sembunyikan" / "Tampilkan", and its up
    /// (−1) / down (+1) buttons among the pages shown.
    PageOn(Page),
    PageMove(Page, i32),
    /// The drag handle of a shown page, and "Kembalikan bawaan".
    PageGrip(Page),
    PageReset,
    /// "Pintasan": a tile's "Sembunyikan" / "+ Tambah", its up (−1) / down
    /// (+1) buttons among the tiles shown, and "Hapus" (a custom tile), by
    /// its place in the list.
    TileOn(usize),
    TileMove(usize, i32),
    TileRemove(usize),
    /// The drag handle of a shown tile, and "Kembalikan bawaan".
    TileGrip(usize),
    TileReset,
    /// "+ Pintasan sendiri": opens the form; its Jenis segments, "Pilih…",
    /// "Batal" and "Tambah".
    TileFormOpen,
    TileFormKind(CustomKind),
    TileBrowse,
    TileFormCancel,
    TileFormAdd,
    DndHelp,
    Pomo(PomoField, i32),
    GoogleSave,
    GoogleConnect,
    GoogleDisconnect,
    GoogleGuide,
    /// "Lanjutan: pakai Client ID sendiri", opening or closing the fields.
    GoogleAdvanced,
    HooksInstall,
    HooksRemove,
    Repo,
    /// The Pembaruan row's button: checks, or installs a known update.
    UpdateCheck,
    /// "Cek pembaruan otomatis".
    UpdateAuto,
    DataFolder,
}

/// What the Tentang → Pembaruan row says and offers.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum UpdateRow {
    /// Nothing known: the installed version, and "Cek sekarang".
    #[default]
    Idle,
    /// A check is running.
    Checking,
    /// A newer version (X.Y.Z) is known: "Update".
    Available(String),
    /// The download is at this percent.
    Downloading(u8),
}

/// Which slider (Tampilan -> Notch).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names)]
pub enum SliderId {
    NotchWidth,
    NotchHeight,
    NotchOpacity,
    /// "Jarak antar ubin", 0 to 24 px.
    ArrangeGap,
}

impl SliderId {
    /// The slider's (min, max).
    pub fn range(self) -> (i32, i32) {
        match self {
            SliderId::NotchWidth => (NOTCH_W_MIN as i32, NOTCH_W_MAX as i32),
            SliderId::NotchHeight => (NOTCH_H_MIN as i32, NOTCH_H_MAX as i32),
            SliderId::NotchOpacity => (0, 100),
            SliderId::ArrangeGap => (0, crate::arrange::GAP_MAX as i32),
        }
    }
}

/// The thumb's diameter; the track's hit area is this tall.
pub const SLIDER_THUMB: f32 = 16.0;
/// The slider's track length, and the value label after it.
const SLIDER_W: f32 = 120.0;
const SLIDER_VALUE_W: f32 = 56.0;

/// The value a press at `x` on a slider `track` wide means: the thumb's
/// centre runs from half a thumb in from the track's left to the same from
/// its right, so `x` there gives `min` and `max`; outside it clamps.
pub fn slider_value(track: Rect, x: f32, min: i32, max: i32) -> i32 {
    let span = (track.w - SLIDER_THUMB).max(1.0);
    let t = ((x - track.x - SLIDER_THUMB / 2.0) / span).clamp(0.0, 1.0);
    (min as f32 + t * (max - min) as f32).round() as i32
}

/// Where the thumb's centre is for `value` (the inverse of `slider_value`).
pub fn slider_x(track: Rect, value: i32, min: i32, max: i32) -> f32 {
    let span = (track.w - SLIDER_THUMB).max(1.0);
    let range = (max - min).max(1) as f32;
    let t = ((value - min) as f32 / range).clamp(0.0, 1.0);
    track.x + SLIDER_THUMB / 2.0 + t * span
}

/// Colour role of a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Primary,
    Secondary,
    Good,
    Warn,
    Bad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub rect: Rect,
    pub text: String,
    pub font: Font,
    pub tone: Tone,
    pub align: Align,
}

/// Which segmented control: animations follow it across layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Style,
    /// The Kucing preview's expression chips.
    CatFace,
    NotchFill,
    Window,
    Lang,
    OpenMode,
    CliStart,
    Shell,
    Motion,
    ShotIgnore,
    /// The own-shortcut form's Jenis.
    TileKind,
    ShotTool,
    NotifToastStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub hit: Hit,
    pub rect: Rect,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    /// The second click of "Putuskan": confirm in red.
    Confirm,
    /// Accent text without a fill, followed by an "open" glyph.
    Link,
    /// Like `Link`, but it opens a part of the page in place: a chevron
    /// (right while closed, down while open, the bool) instead of the glyph.
    Disclosure(bool),
    /// Only the "open" glyph; the label names it but is not drawn.
    External,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub hit: Hit,
    pub rect: Rect,
    pub label: String,
    pub kind: ButtonKind,
    pub enabled: bool,
}

/// A native text field the window places over the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    ClientId,
    ClientSecret,
    /// The own-shortcut form's Nama and Target.
    TileName,
    TileTarget,
}

impl Field {
    /// Where the window keeps the field's text.
    pub fn index(self) -> usize {
        match self {
            Field::ClientId => 0,
            Field::ClientSecret => 1,
            Field::TileName => 2,
            Field::TileTarget => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    /// Off (`enabled` false): faded, no clicks.
    Segmented {
        group: Group,
        track: Rect,
        items: Vec<Segment>,
        selected: usize,
        enabled: bool,
    },
    Toggle {
        hit: Hit,
        rect: Rect,
        on: bool,
        enabled: bool,
    },
    /// `−  value  +`; a side is `None` at the end of the range or while locked.
    Stepper {
        minus: (Rect, Option<Hit>),
        plus: (Rect, Option<Hit>),
        value: Text,
        enabled: bool,
    },
    Swatches {
        items: Vec<(Accent, Rect)>,
        selected: Accent,
    },
    /// "Warna dasar": Ikuti kucing first, then the five surfaces. Off
    /// (`enabled` false): faded, no clicks.
    SurfaceSwatches {
        items: Vec<(Surface, Rect)>,
        selected: Surface,
        enabled: bool,
    },
    /// "Warna" of the cat: its four coats.
    CatSwatches {
        items: Vec<(CatColor, Rect)>,
        selected: CatColor,
        enabled: bool,
    },
    /// "Aksesori": a tile for each, the cat wearing it.
    AccessoryTiles {
        items: Vec<(CatAccessory, Rect)>,
        selected: CatAccessory,
        enabled: bool,
    },
    /// A track with a round thumb; `track` is the hit area (the thumb's
    /// height), the line is drawn through its middle. Off (`enabled` false):
    /// faded, no clicks.
    Slider {
        id: SliderId,
        track: Rect,
        value: i32,
        min: i32,
        max: i32,
        enabled: bool,
    },
    Button(Button),
    /// A round button with only an icon (the lists' up and down chevrons),
    /// or with `bare` just the icon (a row's drag handle).
    IconButton {
        hit: Hit,
        rect: Rect,
        icon: Icon,
        enabled: bool,
        bare: bool,
    },
    /// A hotkey row's field: the combo as key caps (empty: "Tidak ada"), or
    /// the recording prompt while `recording`. A click starts or cancels the
    /// recording.
    HotkeyField {
        hit: Hit,
        rect: Rect,
        caps: Vec<String>,
        recording: bool,
    },
    /// Susun's layouts as a miniature: the tiles of `rects` in `rect`. With
    /// `canvas` it is the editor's big one (the selected tile lit, the
    /// dividers draggable by the window's pointer handling, not as hits).
    Mini {
        rect: Rect,
        rects: Vec<crate::arrange::NRect>,
        selected: Option<usize>,
        canvas: bool,
    },
}

impl Control {
    /// Everything clickable in the control, with the area that takes the click.
    pub fn hits(&self) -> Vec<(Hit, Rect)> {
        match self {
            Control::Segmented { enabled: false, .. } | Control::Toggle { enabled: false, .. } => {
                Vec::new()
            }
            Control::Segmented { items, .. } => items.iter().map(|s| (s.hit, s.rect)).collect(),
            Control::Toggle { hit, rect, .. } => vec![(*hit, *rect)],
            Control::Stepper { minus, plus, .. } => [minus, plus]
                .into_iter()
                .filter_map(|(rect, hit)| hit.map(|h| (h, *rect)))
                .collect(),
            Control::Swatches { items, .. } => items
                .iter()
                .map(|(a, r)| (Hit::Accent(*a), inflate(*r, SWATCH_SLOP)))
                .collect(),
            Control::SurfaceSwatches { enabled: false, .. }
            | Control::CatSwatches { enabled: false, .. }
            | Control::AccessoryTiles { enabled: false, .. } => Vec::new(),
            Control::SurfaceSwatches { items, .. } => items
                .iter()
                .map(|(v, r)| (Hit::Surface(*v), inflate(*r, SWATCH_SLOP)))
                .collect(),
            Control::CatSwatches { items, .. } => items
                .iter()
                .map(|(v, r)| (Hit::CatColor(*v), inflate(*r, SWATCH_SLOP)))
                .collect(),
            Control::AccessoryTiles { items, .. } => items
                .iter()
                .map(|(v, r)| (Hit::CatAccessory(*v), *r))
                .collect(),
            Control::Slider {
                id,
                track,
                enabled: true,
                ..
            } => vec![(Hit::Slider(*id), *track)],
            Control::Slider { .. } => Vec::new(),
            Control::Button(b) if b.enabled => vec![(b.hit, b.rect)],
            Control::Button(_) => Vec::new(),
            Control::IconButton {
                hit,
                rect,
                enabled: true,
                ..
            } => vec![(*hit, *rect)],
            Control::IconButton { .. } => Vec::new(),
            Control::HotkeyField { hit, rect, .. } => vec![(*hit, *rect)],
            Control::Mini { .. } => Vec::new(),
        }
    }

    /// Moves the control `dy` down (a scrolled section).
    fn shift(&mut self, dy: f32) {
        let down = |r: &mut Rect| r.y += dy;
        match self {
            Control::Segmented { track, items, .. } => {
                down(track);
                for s in items {
                    down(&mut s.rect);
                }
            }
            Control::Toggle { rect, .. } => down(rect),
            Control::Stepper {
                minus, plus, value, ..
            } => {
                down(&mut minus.0);
                down(&mut plus.0);
                down(&mut value.rect);
            }
            Control::Swatches { items, .. } => {
                for (_, r) in items {
                    down(r);
                }
            }
            Control::SurfaceSwatches { items, .. } => {
                for (_, r) in items {
                    down(r);
                }
            }
            Control::CatSwatches { items, .. } => {
                for (_, r) in items {
                    down(r);
                }
            }
            Control::AccessoryTiles { items, .. } => {
                for (_, r) in items {
                    down(r);
                }
            }
            Control::Slider { track, .. } => down(track),
            Control::Button(b) => down(&mut b.rect),
            Control::IconButton { rect, .. } => down(rect),
            Control::HotkeyField { rect, .. } => down(rect),
            Control::Mini { rect, .. } => down(rect),
        }
    }

    /// The area the control draws in.
    #[cfg(test)]
    pub fn bounds(&self) -> Rect {
        match self {
            Control::Segmented { track, .. } => *track,
            Control::Toggle { rect, .. } => *rect,
            Control::Stepper { minus, plus, .. } => union(minus.0, plus.0),
            Control::Swatches { items, .. } => items
                .iter()
                .map(|(_, r)| inflate(*r, SWATCH_SLOP))
                .reduce(union)
                .unwrap_or_default(),
            Control::SurfaceSwatches { items, .. } => items
                .iter()
                .map(|(_, r)| inflate(*r, SWATCH_SLOP))
                .reduce(union)
                .unwrap_or_default(),
            Control::CatSwatches { items, .. } => items
                .iter()
                .map(|(_, r)| inflate(*r, SWATCH_SLOP))
                .reduce(union)
                .unwrap_or_default(),
            Control::AccessoryTiles { items, .. } => items
                .iter()
                .map(|(_, r)| *r)
                .reduce(union)
                .unwrap_or_default(),
            Control::Slider { track, .. } => *track,
            Control::Button(b) => b.rect,
            Control::IconButton { rect, .. } => *rect,
            Control::HotkeyField { rect, .. } => *rect,
            Control::Mini { rect, .. } => *rect,
        }
    }
}

/// A fully rounded status chip: a dot and a short text in the tone's colour
/// on a faint wash of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Chip {
    pub rect: Rect,
    pub dot: Rect,
    pub label: Text,
}

/// A decorative icon in a row: never a hit target and not in the focus
/// order. `quiet` draws it dimmed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub rect: Rect,
    pub icon: Icon,
    pub tone: Tone,
    pub quiet: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Nav {
    pub section: Section,
    pub pill: Rect,
    pub glyph: Rect,
    pub label: Text,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub logo: Rect,
    pub brand: Option<Text>,
    pub nav: Vec<Nav>,
    pub title: Option<Text>,
    pub cards: Vec<Rect>,
    /// 1 px lines between the rows of a card.
    pub dividers: Vec<Rect>,
    pub texts: Vec<Text>,
    pub controls: Vec<Control>,
    /// Status chips (Integrations).
    pub chips: Vec<Chip>,
    /// Decorative icons (the pages card), drawn under the texts.
    pub glyphs: Vec<Glyph>,
    /// The live island preview (Appearance).
    pub preview: Option<Rect>,
    /// The cat's preview card (Kucing): the cat on its surface and the idle
    /// notch at real size.
    pub cat_preview: Option<Rect>,
    /// The large logo (About).
    pub about_logo: Option<Rect>,
    /// Info glyph before a note (Pomodoro while locked).
    pub note_glyph: Option<Rect>,
    /// Rounded field backgrounds; the native edit sits inside.
    pub fields: Vec<(Field, Rect)>,
    /// Susun's template editor canvas, while the editor is open.
    pub canvas: Option<Rect>,
    /// The accent row's description, which names the swatch under the pointer.
    pub accent_desc: Option<usize>,
    /// A section taller than the window: the area its content scrolls in
    /// (clipped to it, clicks only inside it), how far it can scroll, and
    /// the scroll applied.
    pub viewport: Option<Rect>,
    pub scroll_max: f32,
    pub scroll: f32,
    /// The rows of the lists that can be reordered, each with its drag
    /// handle's hit, in the order they are drawn.
    pub rows: Vec<ReorderRow>,
    /// The row held by a drag, drawn raised over the others.
    pub lifted: Option<Rect>,
}

/// A list the owner reorders: by dragging a handle or with the buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    Pages,
    Tiles,
}

/// One row of a reorderable list: where it is, and the hit of its handle.
#[derive(Clone, Debug, PartialEq)]
pub struct ReorderRow {
    pub list: List,
    pub grip: Hit,
    pub rect: Rect,
}

/// A drag in progress: the row picked up (its place among the rows shown),
/// the place it would drop at, and where its top is (in unscrolled layout
/// pixels, following the pointer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragView {
    pub list: List,
    pub from: usize,
    pub to: usize,
    pub top: f32,
}

/// The own-shortcut form while it is open: the kind chosen and what is
/// wrong with the last try.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileForm {
    pub kind: CustomKind,
    pub error: Option<Key>,
}

/// The row slot a dragged row dropped with its centre at `y` takes: the last
/// row whose top is at or above `y` (the first one when `y` is above them
/// all). `row_tops` are the tops of the rows, in order.
pub fn drop_index(y: f32, row_tops: &[f32]) -> usize {
    row_tops.iter().rposition(|top| *top <= y).unwrap_or(0)
}

/// How near the viewport's top or bottom edge a dragged row's pointer starts
/// to scroll the content.
pub const DRAG_SCROLL_ZONE: f32 = 48.0;
/// The slowest auto-scroll (at the zone's inner boundary), in rows a second.
const DRAG_SCROLL_MIN_ROWS: f32 = 2.0;
/// The fastest (at the edge and beyond it), in rows a second.
const DRAG_SCROLL_MAX_ROWS: f32 = 12.0;

/// The speed, in px a second, at which the content scrolls while a row is
/// dragged with the pointer at client `y` over the viewport `view_top` to
/// `view_bottom`: negative scrolls up. Zero in the middle; inside the zone at
/// either edge it grows from 2 rows a second to 12 at the edge, and past the
/// edge it stays at 12. In a viewport shorter than two zones the nearer edge
/// decides.
pub fn drag_scroll_speed(y: f32, view_top: f32, view_bottom: f32) -> f32 {
    let from_top = y - view_top;
    let from_bottom = view_bottom - y;
    let (d, sign) = if from_top <= from_bottom {
        (from_top, -1.0)
    } else {
        (from_bottom, 1.0)
    };
    if d >= DRAG_SCROLL_ZONE {
        return 0.0;
    }
    let depth = (1.0 - d.max(0.0) / DRAG_SCROLL_ZONE).clamp(0.0, 1.0);
    let rows = DRAG_SCROLL_MIN_ROWS + (DRAG_SCROLL_MAX_ROWS - DRAG_SCROLL_MIN_ROWS) * depth;
    sign * ROW_H * rows
}

/// The rows' places as they are drawn during a drag: the row held leaves
/// its place and takes the one it would drop at.
fn display_order(n: usize, drag: Option<&DragView>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    if let Some(d) = drag
        && d.from < n
    {
        let held = order.remove(d.from);
        order.insert(d.to.min(n - 1), held);
    }
    order
}

impl Layout {
    /// Every clickable area: sidebar rows, then the section's controls (cut
    /// to the viewport of a scrolled section, so nothing hidden takes clicks).
    pub fn hits(&self) -> Vec<(Hit, Rect)> {
        let nav = self.nav.iter().map(|n| (Hit::Nav(n.section), n.pill));
        let view = self.viewport;
        let controls = self
            .controls
            .iter()
            .flat_map(Control::hits)
            .filter_map(move |(h, r)| match view {
                Some(v) => clip(r, v).map(|r| (h, r)),
                None => Some((h, r)),
            });
        nav.chain(controls).collect()
    }

    /// The scrollbar thumb of a scrolled section.
    pub fn scrollbar(&self) -> Option<Rect> {
        let v = self.viewport.filter(|_| self.scroll_max > 0.0)?;
        let content = v.h + self.scroll_max;
        let h = (v.h * v.h / content).max(24.0);
        let y = v.y + (v.h - h) * (self.scroll / self.scroll_max);
        Some(Rect {
            x: v.right() - SCROLLBAR_W - 5.0,
            y: y + 4.0,
            w: SCROLLBAR_W,
            h: h - 8.0,
        })
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits()
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(h, _)| h)
    }

    /// The segmented control of `group`, if this section has one.
    #[cfg(test)]
    pub fn segmented(&self, group: Group) -> Option<(&Rect, &[Segment], usize)> {
        self.controls.iter().find_map(|c| match c {
            Control::Segmented {
                group: g,
                track,
                items,
                selected,
                ..
            } if *g == group => Some((track, items.as_slice(), *selected)),
            _ => None,
        })
    }
}

/// Google Calendar as the settings window shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoogleState {
    pub status: CalStatus,
    /// The fields differ from the saved config.
    pub dirty: bool,
    /// The Client ID field has text.
    pub has_id: bool,
    /// "Putuskan" was clicked once and waits for the second click.
    pub confirm: bool,
    /// This build carries Vale's own Google client: one click signs in.
    pub built_in: bool,
    /// "Lanjutan" is open: the user's own Client ID and secret show.
    pub advanced: bool,
}

/// Everything a layout depends on besides the section.
#[derive(Clone, Debug, PartialEq)]
pub struct State<'a> {
    pub lang: Lang,
    pub style: Style,
    pub accent: Accent,
    pub surface: Surface,
    pub window: WindowTheme,
    /// The closed island's size and background (Tampilan -> Notch).
    pub notch_width: u32,
    pub notch_height: u32,
    pub notch_fill: NotchFill,
    pub notch_opacity: u8,
    /// The idle island shows the clock.
    pub idle_clock: bool,
    pub eq_follow_audio: bool,
    /// "Di belakang jendela" (experimental).
    pub stay_behind: bool,
    /// The cat in the notch, how it moves, when it sleeps, the greeting.
    pub robot: bool,
    pub robot_motion: Motion,
    pub robot_sleep: u32,
    pub greeting: bool,
    /// The cat's coat and accessory, "Ikuti kursor" and "Mengecil jadi mata".
    pub cat_color: CatColor,
    pub cat_accessory: CatAccessory,
    pub cat_follow: bool,
    pub notch_peek: bool,
    pub cat_dance: bool,
    /// The face the Kucing preview shows (window state, not saved).
    pub cat_face: CatFace,
    /// How far a section taller than the window is scrolled (clamped).
    pub scroll: f32,
    /// Synced lyrics in the notch.
    pub lyrics: bool,
    /// The CLI page's shell.
    pub shell: Shell,
    /// Where the CLI page starts when it opens.
    pub cli_start: CliStart,
    /// How the pointer opens the panel.
    pub open_mode: OpenMode,
    /// The plug-in and safe-to-remove notices.
    pub device_notices: bool,
    /// The five hotkeys, and by `HotkeyId::index` whether a set one could not
    /// be registered (taken by another app, or the PrtSc hook failed).
    pub hotkeys: Hotkeys,
    pub hotkey_failed: [bool; 5],
    /// The row being recorded, and why the last combo typed was refused
    /// (window state, not saved).
    pub hotkey_recording: Option<HotkeyId>,
    pub hotkey_error: Option<(HotkeyId, RejectMsg)>,
    /// Susun's settings, and the template editor while it is open.
    pub arrange: ArrangeSettings,
    pub arrange_edit: Option<EditView>,
    pub autostart: bool,
    pub battery_threshold: u8,
    pub has_battery: bool,
    /// The System page's warnings: GB free and percent used.
    pub disk_warn_gb: u32,
    pub ram_warn_percent: u8,
    /// Mole's whitelist gets the Cargo and Autodesk caches before its
    /// first preview.
    pub mole_protect: bool,
    pub pomodoro: PomoSettings,
    /// A phase runs or is paused: the steppers are off.
    pub pomodoro_locked: bool,
    pub google: GoogleState,
    /// A short message beside the Google buttons.
    pub google_note: Option<(&'a str, Tone)>,
    /// Settings → Fitur → Notifikasi, the excluded apps (`muted_apps`) and
    /// whether "Hapus riwayat notifikasi" waits for its second click.
    pub notif: NotifSettings,
    pub muted: Vec<String>,
    pub notif_confirm: bool,
    /// "Hapus riwayat" was clicked once and waits for the second click.
    pub clip_confirm: bool,
    /// `None` when Claude Code's settings could not be read.
    pub hooks: Option<HookState>,
    /// The outcome of the last hook button, shown instead of the status.
    pub hook_note: Option<(&'a str, Tone)>,
    /// Settings → Clipboard.
    pub clip: ClipboardSettings,
    /// Settings → Clipboard → Screenshot.
    pub shot: ScreenshotSettings,
    /// "Halaman panel": the pages' order and which the wheel shows.
    pub pages: PanelPages,
    /// "Pintasan": the tiles' order, which show, and the custom ones.
    pub tiles: ShortcutTiles,
    /// A drag on one of the two lists, and the own-shortcut form.
    pub drag: Option<DragView>,
    pub tile_form: Option<TileForm>,
    /// A message after the last tile added (it is off: the page is full).
    pub tile_note: Option<Key>,
    pub version: &'a str,
    /// "Cek pembaruan otomatis", and the Pembaruan row's state.
    pub update_auto: bool,
    pub update_row: UpdateRow,
    /// The data folder in use, as `data_dir::label` names it.
    pub data_dir: &'a str,
}

pub fn accent_key(a: Accent) -> Key {
    match a {
        Accent::Terracotta => Key::AccentTerracotta,
        Accent::Sage => Key::AccentSage,
        Accent::Ochre => Key::AccentOchre,
        Accent::Clay => Key::AccentClay,
        Accent::Olive => Key::AccentOlive,
        Accent::Plum => Key::AccentPlum,
    }
}

pub fn status_key(s: CalStatus) -> (Key, Tone) {
    match s {
        CalStatus::NotConfigured => (Key::StatusNotConfigured, Tone::Secondary),
        CalStatus::NeedsLogin => (Key::StatusNeedsLogin, Tone::Warn),
        CalStatus::Reconnect => (Key::StatusReconnect, Tone::Warn),
        CalStatus::Ok => (Key::StatusConnected, Tone::Good),
        CalStatus::Offline => (Key::StatusOffline, Tone::Secondary),
    }
}

/// The Google card's status chip: `status_key`, but without Vale's client and
/// without a Client ID typed the next step is the user's, not "not set up".
fn google_status_key(g: &GoogleState) -> (Key, Tone) {
    match status_key(g.status) {
        (Key::StatusNotConfigured, tone) if !g.built_in && !g.has_id => {
            (Key::StatusNeedsClientId, tone)
        }
        other => other,
    }
}

/// The value a stepper shows: "25 mnt", "4", "20%".
pub fn pomo_value(lang: Lang, field: PomoField, v: u32) -> String {
    match field {
        PomoField::Sessions => v.to_string(),
        _ => format!("{v} {}", text(lang, Key::MinutesShort)),
    }
}

pub fn pomo_get(p: &PomoSettings, field: PomoField) -> u32 {
    match field {
        PomoField::Focus => p.focus_min,
        PomoField::ShortBreak => p.short_break_min,
        PomoField::LongBreak => p.long_break_min,
        PomoField::Sessions => p.sessions,
    }
}

/// The battery threshold one step up (`dir > 0`) or down, kept in 5–50.
pub fn battery_step(threshold: u8, dir: i32) -> u8 {
    let (min, max, step) = BATTERY_RANGE;
    let stepped = if dir > 0 {
        threshold.saturating_add(step)
    } else {
        threshold.saturating_sub(step)
    };
    // A hand-edited value off the 5 % grid snaps onto it.
    (stepped / step * step).clamp(min, max)
}

/// One step of `(min, max, step)` up (`dir > 0`) or down, snapped onto the
/// step's grid (from `min`) and kept in range.
fn grid_step(v: u32, dir: i32, (min, max, step): (u32, u32, u32)) -> u32 {
    let v = v.clamp(min, max);
    let on_grid = min + (v - min) / step * step;
    let stepped = if dir > 0 {
        on_grid + step
    } else if on_grid < v {
        on_grid
    } else {
        on_grid.saturating_sub(step)
    };
    stepped.clamp(min, max)
}

/// The disk warning one step up or down: 5–100 GB by 5.
pub fn disk_warn_step(gb: u32, dir: i32) -> u32 {
    grid_step(gb, dir, DISK_WARN_RANGE)
}

/// The RAM warning one step up or down: 70–98 %.
pub fn ram_warn_step(percent: u8, dir: i32) -> u8 {
    let (min, max, step) = RAM_WARN_RANGE;
    let range = (u32::from(min), u32::from(max), u32::from(step));
    grid_step(u32::from(percent), dir, range) as u8
}

/// "10 GB": the disk warning stepper's value.
pub fn disk_warn_value(gb: u32) -> String {
    format!("{gb} GB")
}

/// The scroll after `wheel` (raw delta, +120 per notch away from the user,
/// which scrolls up) from `scroll`, kept in 0..=`max`.
pub fn scroll_by(scroll: f32, wheel: i32, max: f32) -> f32 {
    (scroll - wheel as f32 / 120.0 * WHEEL_STEP).clamp(0.0, max.max(0.0))
}

/// The part of `r` inside `v`, if any.
fn clip(r: Rect, v: Rect) -> Option<Rect> {
    let x = r.x.max(v.x);
    let y = r.y.max(v.y);
    let right = r.right().min(v.right());
    let bottom = r.bottom().min(v.bottom());
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        w: right - x,
        h: bottom - y,
    })
}

fn inflate(r: Rect, by: f32) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

#[cfg(test)]
fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        w: a.right().max(b.right()) - x,
        h: a.bottom().max(b.bottom()) - y,
    }
}

struct Builder<'a> {
    m: &'a dyn Measure,
    lang: Lang,
    l: Layout,
    /// Top of the next card.
    y: f32,
    /// The column for this window size.
    g: Geom,
}

/// Where the content column goes for one window size, worked out once per
/// layout. At the minimum client size this is `CONTENT_X` and `CONTENT_W`.
#[derive(Clone, Copy, Debug)]
struct Geom {
    client_w: f32,
    client_h: f32,
    content_x: f32,
    content_w: f32,
    /// Where controls end inside a card.
    right: f32,
}

impl Geom {
    fn new((w, h): (f32, f32)) -> Self {
        let client_w = w.max(CLIENT_W);
        let client_h = h.max(CLIENT_H);
        let avail = client_w - SIDEBAR_W - 2.0 * PAD;
        let content_w = avail.min(MAX_CONTENT_W);
        let content_x = SIDEBAR_W + PAD + ((avail - content_w) / 2.0).round();
        Geom {
            client_w,
            client_h,
            content_x,
            content_w,
            right: content_x + content_w - INSET,
        }
    }
}

impl Builder<'_> {
    fn t(&self, key: Key) -> String {
        text(self.lang, key).to_owned()
    }

    fn w(&self, s: &str, font: Font) -> f32 {
        self.m.width(s, font)
    }

    fn card(&mut self, h: f32) -> Rect {
        let r = Rect {
            x: self.g.content_x,
            y: self.y,
            w: self.g.content_w,
            h,
        };
        self.l.cards.push(r);
        self.y += h + CARD_GAP;
        r
    }

    /// A hairline across the whole card, between two rows.
    fn divider(&mut self, card: Rect, y: f32) {
        self.l.dividers.push(Rect {
            x: card.x + 1.0,
            y: y - 0.5,
            w: card.w - 2.0,
            h: 1.0,
        });
    }

    /// An uppercase label above the next card, in the space it takes.
    fn eyebrow(&mut self, key: Key, w: f32) {
        let rect = Rect {
            x: self.g.content_x + 2.0,
            y: self.y,
            w,
            h: EYEBROW_H,
        };
        let label = text(self.lang, key).to_uppercase();
        self.text(rect, label, Font::Eyebrow, Tone::Secondary, Align::Left);
        self.y += EYEBROW_H + EYEBROW_GAP;
    }

    /// A status chip starting at `x`, centred on `cy`, at most `max_w` wide.
    fn chip(&mut self, x: f32, cy: f32, label: String, tone: Tone, max_w: f32) {
        let text_w = self.w(&label, Font::Desc);
        let w = (2.0 * CHIP_PAD + DOT + DOT_GAP + text_w).min(max_w.max(0.0));
        let rect = Rect {
            x,
            y: cy - CHIP_H / 2.0,
            w,
            h: CHIP_H,
        };
        let dot = Rect {
            x: x + CHIP_PAD,
            y: cy - DOT / 2.0,
            w: DOT,
            h: DOT,
        };
        let text_x = dot.right() + DOT_GAP;
        let label = Text {
            rect: Rect {
                x: text_x,
                y: rect.y,
                w: (rect.right() - CHIP_PAD - text_x).max(0.0),
                h: CHIP_H,
            },
            text: label,
            font: Font::Desc,
            tone,
            align: Align::Left,
        };
        self.l.chips.push(Chip { rect, dot, label });
    }

    fn text(&mut self, rect: Rect, s: String, font: Font, tone: Tone, align: Align) -> usize {
        self.l.texts.push(Text {
            rect,
            text: s,
            font,
            tone,
            align,
        });
        self.l.texts.len() - 1
    }

    /// A row's label and description, left of `control_x`. With `chip`, the
    /// description is a status chip in its tone. Returns the description's
    /// index in `texts` (none for a chip).
    fn row_text(
        &mut self,
        card: Rect,
        y: f32,
        label: String,
        desc: Option<(String, Tone)>,
        chip: bool,
        control_x: f32,
    ) -> Option<usize> {
        let x = card.x + INSET;
        let w = row_text_w(card, control_x);
        let Some((desc, tone)) = desc else {
            let rect = Rect {
                x,
                y: y + 16.0,
                w,
                h: 20.0,
            };
            self.text(rect, label, Font::Label, Tone::Primary, Align::Left);
            return None;
        };
        if chip {
            let label_rect = Rect {
                x,
                y: y + 5.0,
                w,
                h: 20.0,
            };
            self.text(label_rect, label, Font::Label, Tone::Primary, Align::Left);
            self.chip(x, y + 25.0 + CHIP_H / 2.0, desc, tone, w);
            return None;
        }
        let label_rect = Rect {
            x,
            y: y + 7.0,
            w,
            h: 20.0,
        };
        self.text(label_rect, label, Font::Label, Tone::Primary, Align::Left);
        let desc_rect = Rect {
            x,
            y: y + 28.0,
            w,
            h: 16.0,
        };
        Some(self.text(desc_rect, desc, Font::Desc, tone, Align::Left))
    }

    /// `row_text` with a description already broken into `lines` (`wrap`):
    /// each line one `DESC_LINE_H` under the last, in a row
    /// `wrapped_row_h(lines.len())` tall.
    fn row_text_lines(
        &mut self,
        card: Rect,
        y: f32,
        label: String,
        lines: Vec<String>,
        tone: Tone,
        control_x: f32,
    ) {
        let x = card.x + INSET;
        let w = row_text_w(card, control_x);
        let label_rect = Rect {
            x,
            y: y + 7.0,
            w,
            h: 20.0,
        };
        self.text(label_rect, label, Font::Label, Tone::Primary, Align::Left);
        for (i, line) in lines.into_iter().enumerate() {
            let rect = Rect {
                x,
                y: y + 28.0 + i as f32 * DESC_LINE_H,
                w,
                h: 16.0,
            };
            self.text(rect, line, Font::Desc, tone, Align::Left);
        }
    }

    /// `s` broken at spaces into lines no wider than `w` in `font`; a word
    /// wider than `w` keeps a line of its own.
    fn wrap(&self, s: &str, font: Font, w: f32) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        for word in s.split_whitespace() {
            match lines.last_mut() {
                Some(line) if self.w(&format!("{line} {word}"), font) <= w => {
                    line.push(' ');
                    line.push_str(word);
                }
                _ => lines.push(word.to_owned()),
            }
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }

    /// The width a segmented control of `items` takes (see `segmented_if`).
    fn segmented_w(&self, items: &[(Hit, String)]) -> f32 {
        self.segment_widths(items).iter().sum::<f32>() + 2.0 * SEG_PAD
    }

    /// Each segment's width: all the widest one's, unless that runs past
    /// `SEG_EQUAL_MAX`; then each its own.
    fn segment_widths(&self, items: &[(Hit, String)]) -> Vec<f32> {
        let natural: Vec<f32> = items
            .iter()
            .map(|(_, s)| (self.w(s, Font::Control) + 2.0 * SEG_TEXT_PAD).max(SEG_MIN_W))
            .collect();
        let widest = natural.iter().copied().fold(0.0, f32::max);
        let equal = widest * items.len() as f32 + 2.0 * SEG_PAD <= SEG_EQUAL_MAX;
        if equal {
            vec![widest; items.len()]
        } else {
            natural
        }
    }

    /// A segmented control right-aligned at `right`, centered on `cy`.
    fn segmented(
        &mut self,
        group: Group,
        right: f32,
        cy: f32,
        items: Vec<(Hit, String)>,
        selected: usize,
    ) -> f32 {
        self.segmented_if(group, right, cy, items, selected, true)
    }

    /// `segmented`, faded and taking no clicks unless `enabled`.
    fn segmented_if(
        &mut self,
        group: Group,
        right: f32,
        cy: f32,
        items: Vec<(Hit, String)>,
        selected: usize,
        enabled: bool,
    ) -> f32 {
        let widths = self.segment_widths(&items);
        let total = widths.iter().sum::<f32>() + 2.0 * SEG_PAD;
        let track = Rect {
            x: right - total,
            y: cy - CONTROL_H / 2.0,
            w: total,
            h: CONTROL_H,
        };
        let mut x = track.x + SEG_PAD;
        let segs = items
            .into_iter()
            .zip(widths)
            .map(|((hit, label), w)| {
                let rect = Rect {
                    x,
                    y: track.y + SEG_PAD,
                    w,
                    h: CONTROL_H - 2.0 * SEG_PAD,
                };
                x += w;
                Segment { hit, rect, label }
            })
            .collect();
        self.l.controls.push(Control::Segmented {
            group,
            track,
            items: segs,
            selected,
            enabled,
        });
        track.x
    }

    fn toggle(&mut self, hit: Hit, right: f32, cy: f32, on: bool) -> f32 {
        self.toggle_if(hit, right, cy, on, true)
    }

    fn toggle_if(&mut self, hit: Hit, right: f32, cy: f32, on: bool, enabled: bool) -> f32 {
        let rect = Rect {
            x: right - TOGGLE_W,
            y: cy - TOGGLE_H / 2.0,
            w: TOGGLE_W,
            h: TOGGLE_H,
        };
        self.l.controls.push(Control::Toggle {
            hit,
            rect,
            on,
            enabled,
        });
        rect.x
    }

    /// A slider right-aligned at `right` (the value label at its end, centred
    /// on `cy`). Returns the track's left edge.
    fn slider(
        &mut self,
        right: f32,
        cy: f32,
        id: SliderId,
        value: i32,
        label: String,
        enabled: bool,
    ) -> f32 {
        let (min, max) = id.range();
        let value = value.clamp(min, max);
        let value_rect = Rect {
            x: right - SLIDER_VALUE_W,
            y: cy - CONTROL_H / 2.0,
            w: SLIDER_VALUE_W,
            h: CONTROL_H,
        };
        let track = Rect {
            x: value_rect.x - GAP - SLIDER_W,
            y: cy - SLIDER_THUMB / 2.0,
            w: SLIDER_W,
            h: SLIDER_THUMB,
        };
        let tone = if enabled {
            Tone::Primary
        } else {
            Tone::Secondary
        };
        self.text(value_rect, label, Font::Value, tone, Align::Center);
        self.l.controls.push(Control::Slider {
            id,
            track,
            value,
            min,
            max,
            enabled,
        });
        track.x
    }

    #[allow(clippy::too_many_arguments)]
    fn stepper(
        &mut self,
        right: f32,
        cy: f32,
        value: String,
        down: Hit,
        up: Hit,
        can: (bool, bool),
        enabled: bool,
    ) -> f32 {
        let total = 2.0 * STEP_BUTTON + STEP_VALUE_W;
        let y = cy - STEP_BUTTON / 2.0;
        let x = right - total;
        let button = |x| Rect {
            x,
            y,
            w: STEP_BUTTON,
            h: STEP_BUTTON,
        };
        let value = Text {
            rect: Rect {
                x: x + STEP_BUTTON,
                y,
                w: STEP_VALUE_W,
                h: STEP_BUTTON,
            },
            text: value,
            font: Font::Value,
            tone: Tone::Primary,
            align: Align::Center,
        };
        self.l.controls.push(Control::Stepper {
            minus: (button(x), (enabled && can.0).then_some(down)),
            plus: (
                button(right - STEP_BUTTON),
                (enabled && can.1).then_some(up),
            ),
            value,
            enabled,
        });
        x
    }

    /// A round chevron button right-aligned at `right`, centered on `cy`.
    /// Returns its left edge.
    fn icon_button(&mut self, right: f32, cy: f32, hit: Hit, icon: Icon, enabled: bool) -> f32 {
        let rect = Rect {
            x: right - STEP_BUTTON,
            y: cy - STEP_BUTTON / 2.0,
            w: STEP_BUTTON,
            h: STEP_BUTTON,
        };
        self.l.controls.push(Control::IconButton {
            hit,
            rect,
            icon,
            enabled,
            bare: false,
        });
        rect.x
    }

    /// A decorative icon: no control, so no hit region and no focus stop.
    fn glyph(&mut self, rect: Rect, icon: Icon, tone: Tone, quiet: bool) {
        self.l.glyphs.push(Glyph {
            rect,
            icon,
            tone,
            quiet,
        });
    }

    /// A row's drag handle, in the column at the left of `card`.
    fn grip(&mut self, card: Rect, cy: f32, hit: Hit) {
        let rect = Rect {
            x: card.x + 8.0,
            y: cy - STEP_BUTTON / 2.0,
            w: GRIP_W - 4.0,
            h: STEP_BUTTON,
        };
        self.l.controls.push(Control::IconButton {
            hit,
            rect,
            icon: Icon::Grip,
            enabled: true,
            bare: true,
        });
    }

    /// The first row of a list card: `title` (the count) and the button
    /// that puts the list back to its defaults.
    fn list_header(&mut self, card: Rect, title: String, reset: Hit, enabled: bool) {
        let cy = card.y + HEADER_H / 2.0;
        let label = self.t(Key::ListRestore);
        let w = self.button_w(&[&label], ButtonKind::Secondary);
        let item = (reset, label, ButtonKind::Secondary, enabled, w);
        let x = self.buttons(self.g.right, cy, vec![item]);
        let rect = Rect {
            x: card.x + INSET,
            y: cy - 10.0,
            w: row_text_w(card, x),
            h: 20.0,
        };
        self.text(rect, title, Font::Label, Tone::Primary, Align::Left);
    }

    /// A small uppercase title across a card at `y`, with a hairline above
    /// it unless it opens the card.
    fn sub_header(&mut self, card: Rect, y: f32, label: String, line: bool) {
        if line {
            self.divider(card, y);
        }
        let rect = Rect {
            x: card.x + INSET,
            y: y + (SUBHEAD_H - EYEBROW_H) / 2.0 + 2.0,
            w: card.w - 2.0 * INSET,
            h: EYEBROW_H,
        };
        self.text(
            rect,
            label.to_uppercase(),
            Font::Eyebrow,
            Tone::Secondary,
            Align::Left,
        );
    }

    /// A button for a list row: the label with a little padding, narrower
    /// than a button of the cards' rows (which keep `BUTTON_MIN_W`).
    fn compact_w(&self, label: &str) -> f32 {
        (self.w(label, Font::Control) + 24.0).max(56.0)
    }

    fn button_w(&self, labels: &[&str], kind: ButtonKind) -> f32 {
        let widest = labels
            .iter()
            .map(|s| self.w(s, Font::Control))
            .fold(0.0, f32::max);
        match kind {
            ButtonKind::Link | ButtonKind::Disclosure(_) => widest + 2.0 * LINK_PAD + LINK_GLYPH_W,
            ButtonKind::External => ICON_BUTTON,
            _ => (widest + 2.0 * BUTTON_PAD).max(BUTTON_MIN_W),
        }
    }

    /// Buttons right-aligned at `right` in the order given, centered on `cy`.
    /// Each is `(hit, label, kind, enabled, width)`. Returns the left edge.
    fn buttons(
        &mut self,
        right: f32,
        cy: f32,
        items: Vec<(Hit, String, ButtonKind, bool, f32)>,
    ) -> f32 {
        let total: f32 =
            items.iter().map(|i| i.4).sum::<f32>() + GAP * (items.len().max(1) - 1) as f32;
        let mut x = right - total;
        let left = x;
        for (hit, label, kind, enabled, w) in items {
            let h = match kind {
                ButtonKind::Link | ButtonKind::Disclosure(_) => CONTROL_H,
                ButtonKind::External => ICON_BUTTON,
                _ => BUTTON_H,
            };
            self.l.controls.push(Control::Button(Button {
                hit,
                rect: Rect {
                    x,
                    y: cy - h / 2.0,
                    w,
                    h,
                },
                label,
                kind,
                enabled,
            }));
            x += w + GAP;
        }
        left
    }

    fn sidebar(&mut self) {
        self.l.logo = Rect {
            x: NAV_X + 8.0,
            y: 22.0,
            w: LOGO,
            h: LOGO,
        };
        let brand_x = self.l.logo.right() + 8.0;
        self.l.brand = Some(Text {
            // No fill or divider marks the sidebar's edge, so the brand may
            // run into the gutter, up to half a gap before the content
            // column. DM Sans 600 draws "Vale" at 16 px about
            // 40 px wide; the estimate is well inside what the sidebar has.
            rect: Rect {
                x: brand_x,
                y: 22.0,
                w: SIDEBAR_W + PAD - GAP / 2.0 - brand_x,
                h: LOGO,
            },
            text: "Vale".into(),
            font: Font::Brand,
            tone: Tone::Primary,
            align: Align::Left,
        });
        for (i, s) in Section::ALL.into_iter().enumerate() {
            let pill = Rect {
                x: NAV_X,
                y: NAV_Y + i as f32 * (NAV_H + NAV_GAP),
                w: NAV_W,
                h: NAV_H,
            };
            let glyph = Rect {
                x: pill.x + 10.0,
                y: pill.y,
                w: 24.0,
                h: NAV_H,
            };
            let label_x = glyph.right() + 8.0;
            let label = Text {
                rect: Rect {
                    x: label_x,
                    y: pill.y,
                    w: pill.right() - 8.0 - label_x,
                    h: NAV_H,
                },
                text: self.t(s.key()),
                font: Font::Nav,
                tone: Tone::Primary,
                align: Align::Left,
            };
            self.l.nav.push(Nav {
                section: s,
                pill,
                glyph,
                label,
            });
        }
    }
}

/// The whole window for `section` at the minimum client size (what the
/// tests lay out against; the window uses `layout_in`).
#[cfg(test)]
pub fn layout(section: Section, s: &State, m: &dyn Measure) -> Layout {
    layout_in(section, s, m, (CLIENT_W, CLIENT_H))
}

/// The whole window for `section` in a client area of `client` logical
/// pixels (never smaller than `CLIENT_W` x `CLIENT_H`).
pub fn layout_in(section: Section, s: &State, m: &dyn Measure, client: (f32, f32)) -> Layout {
    let g = Geom::new(client);
    let mut b = Builder {
        m,
        lang: s.lang,
        l: Layout::default(),
        y: FIRST_CARD_Y,
        g,
    };
    b.sidebar();
    b.l.title = Some(Text {
        rect: Rect {
            x: b.g.content_x,
            y: TITLE_Y,
            w: b.g.content_w,
            h: TITLE_H,
        },
        text: b.t(section.key()),
        font: Font::Title,
        tone: Tone::Primary,
        align: Align::Left,
    });
    match section {
        Section::General => general(&mut b, s),
        Section::Appearance => appearance(&mut b, s),
        Section::Cat => cat_page(&mut b, s),
        Section::Panel => panel_page(&mut b, s),
        Section::Clipboard => clipboard_page(&mut b, s),
        Section::Hotkeys => hotkeys_page(&mut b, s),
        Section::Features => features_page(&mut b, s),
        Section::Integrations => integrations(&mut b, s),
        Section::About => about(&mut b, s),
    }
    scroll(&mut b.l, s.scroll, g);
    b.l
}

/// A section whose cards run past the window's bottom margin scrolls: its
/// content (not the sidebar or title) moves up by `scroll`, kept in range,
/// inside a viewport from `VIEW_TOP` to the window's bottom.
fn scroll(l: &mut Layout, scroll: f32, g: Geom) {
    // A note under the last card counts too.
    let bottom = l
        .cards
        .iter()
        .map(Rect::bottom)
        .chain(l.texts.iter().map(|t| t.rect.bottom()))
        .fold(0.0, f32::max);
    let max = (bottom - (g.client_h - PAD)).max(0.0);
    if max <= 0.0 {
        return;
    }
    let dy = -scroll.clamp(0.0, max);
    l.viewport = Some(Rect {
        x: SIDEBAR_W,
        y: VIEW_TOP,
        w: g.client_w - SIDEBAR_W,
        h: g.client_h - VIEW_TOP,
    });
    l.scroll_max = max;
    l.scroll = -dy;
    let down = |r: &mut Rect| r.y += dy;
    for r in l.cards.iter_mut().chain(l.dividers.iter_mut()) {
        down(r);
    }
    for t in &mut l.texts {
        down(&mut t.rect);
    }
    for c in &mut l.controls {
        c.shift(dy);
    }
    for c in &mut l.chips {
        down(&mut c.rect);
        down(&mut c.dot);
        down(&mut c.label.rect);
    }
    for (_, r) in &mut l.fields {
        down(r);
    }
    for g in &mut l.glyphs {
        down(&mut g.rect);
    }
    for r in &mut l.rows {
        down(&mut r.rect);
    }
    for r in [
        &mut l.preview,
        &mut l.cat_preview,
        &mut l.about_logo,
        &mut l.note_glyph,
        &mut l.lifted,
    ]
    .into_iter()
    .flatten()
    {
        down(r);
    }
}

fn index_of<T: PartialEq>(all: &[T], v: &T) -> usize {
    all.iter().position(|x| x == v).unwrap_or(0)
}

/// An uppercase title over the card that follows (the whole content width).
fn titled(b: &mut Builder, key: Key) {
    b.eyebrow(key, b.g.content_w - 4.0);
}

/// Tampilan: the preview, then the cards "Island", "Notch" and "Jendela".
fn appearance(b: &mut Builder, s: &State) {
    let preview = b.card(PREVIEW_H);
    b.l.preview = Some(preview);
    titled(b, Key::GroupIsland);
    let card = b.card(5.0 * ROW_H);
    let rows: [f32; 5] = std::array::from_fn(|i| card.y + i as f32 * ROW_H);
    let cy = |y: f32| y + ROW_H / 2.0;

    let styles = [
        (Style::Pill, Key::StylePill),
        (Style::Notch, Key::StyleNotch),
    ];
    let items = styles
        .iter()
        .map(|(v, k)| (Hit::Style(*v), b.t(*k)))
        .collect();
    let selected = index_of(&[Style::Pill, Style::Notch], &s.style);
    let x = b.segmented(Group::Style, b.g.right, cy(rows[0]), items, selected);
    let desc = Some((b.t(Key::IslandStyleDesc), Tone::Secondary));
    b.row_text(card, rows[0], b.t(Key::IslandStyle), desc, false, x);

    let total = Accent::ALL.len() as f32 * SWATCH + (Accent::ALL.len() - 1) as f32 * SWATCH_GAP;
    let x0 = b.g.right - total;
    let items = Accent::ALL
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let r = Rect {
                x: x0 + i as f32 * (SWATCH + SWATCH_GAP),
                y: cy(rows[1]) - SWATCH / 2.0,
                w: SWATCH,
                h: SWATCH,
            };
            (*a, r)
        })
        .collect();
    b.l.controls.push(Control::Swatches {
        items,
        selected: s.accent,
    });
    let desc = Some((b.t(accent_key(s.accent)), Tone::Secondary));
    b.l.accent_desc = b.row_text(
        card,
        rows[1],
        b.t(Key::AccentColor),
        desc,
        false,
        x0 - SWATCH_SLOP,
    );
    b.divider(card, rows[1]);

    // "Warna dasar": Ikuti kucing first, then the five fills, as swatches;
    // the row's description is the chosen name (and what Auto gives now).
    let n = Surface::ALL.len() as f32;
    let total = n * SWATCH + (n - 1.0) * SWATCH_GAP;
    let x0 = b.g.right - total;
    let items = Surface::ALL
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let r = Rect {
                x: x0 + i as f32 * (SWATCH + SWATCH_GAP),
                y: cy(rows[2]) - SWATCH / 2.0,
                w: SWATCH,
                h: SWATCH,
            };
            (*v, r)
        })
        .collect();
    b.l.controls.push(Control::SurfaceSwatches {
        items,
        selected: s.surface,
        enabled: true,
    });
    let name = surface_label(b.lang, s.surface, s.cat_color, s.robot);
    let desc = Some((name, Tone::Secondary));
    b.row_text(
        card,
        rows[2],
        b.t(Key::IslandSurface),
        desc,
        false,
        x0 - SWATCH_SLOP,
    );
    b.divider(card, rows[2]);

    let x = b.toggle(Hit::IdleClock, b.g.right, cy(rows[3]), s.idle_clock);
    let desc = Some((b.t(Key::IdleClockDesc), Tone::Secondary));
    b.row_text(card, rows[3], b.t(Key::IdleClock), desc, false, x);
    b.divider(card, rows[3]);

    let x = b.toggle(
        Hit::EqFollowAudio,
        b.g.right,
        cy(rows[4]),
        s.eq_follow_audio,
    );
    let desc = Some((b.t(Key::EqFollowAudioDesc), Tone::Secondary));
    b.row_text(card, rows[4], b.t(Key::EqFollowAudio), desc, false, x);
    b.divider(card, rows[4]);

    notch_card(b, s);

    titled(b, Key::GroupWindow);
    let card = b.card(ROW_H);
    let modes = [
        (WindowTheme::System, Key::ModeSystem),
        (WindowTheme::Dark, Key::ModeDark),
        (WindowTheme::Light, Key::ModeLight),
    ];
    let items = modes
        .iter()
        .map(|(v, k)| (Hit::WindowMode(*v), b.t(*k)))
        .collect();
    let x = b.segmented(
        Group::Window,
        b.g.right,
        card.y + ROW_H / 2.0,
        items,
        index_of(WindowTheme::ALL, &s.window),
    );
    let desc = Some((b.t(Key::WindowModeDesc), Tone::Secondary));
    b.row_text(card, card.y, b.t(Key::WindowMode), desc, false, x);
}

/// The name of a surface choice: "Espresso", ...; Ikuti kucing says what it
/// gives for the cat now ("Ikuti kucing · Laut").
pub fn surface_label(lang: Lang, surface: Surface, cat: CatColor, robot: bool) -> String {
    let name = |v: Surface| text(lang, surface_key(v));
    match surface {
        Surface::Auto => format!("{} · {}", name(surface), name(surface.resolve(cat, robot))),
        v => name(v).to_owned(),
    }
}

pub fn surface_key(v: Surface) -> Key {
    match v {
        Surface::Auto => Key::SurfaceAuto,
        Surface::Espresso => Key::SurfaceEspresso,
        Surface::Graphite => Key::SurfaceGraphite,
        Surface::Sea => Key::SurfaceSea,
        Surface::Night => Key::SurfaceNight,
        Surface::Plum => Key::SurfacePlum,
        Surface::Terminal => Key::SurfaceTerminal,
    }
}

pub fn cat_color_key(v: CatColor) -> Key {
    match v {
        CatColor::Terracotta => Key::CatColorTerracotta,
        CatColor::Calico => Key::CatColorCalico,
        CatColor::White => Key::CatColorWhite,
        CatColor::Black => Key::CatColorBlack,
    }
}

pub fn accessory_key(v: CatAccessory) -> Key {
    match v {
        CatAccessory::None => Key::AccNone,
        CatAccessory::Glasses => Key::AccGlasses,
        CatAccessory::Bow => Key::AccBow,
        CatAccessory::Nightcap => Key::AccNightcap,
        CatAccessory::Scarf => Key::AccScarf,
        CatAccessory::Flower => Key::AccFlower,
        CatAccessory::Crown => Key::AccCrown,
    }
}

/// "190 px": a size slider's value.
pub fn px_value(px: u32) -> String {
    format!("{px} px")
}

/// "30 %": the opacity slider's value.
pub fn percent_value(pct: u8) -> String {
    format!("{pct} %")
}

/// The closed island's size and background: width and height sliders, a
/// reset to 190 x 34, "Solid" or "Tembus", and the opacity slider, which
/// fades and takes no clicks unless it is Tembus.
fn notch_card(b: &mut Builder, s: &State) {
    titled(b, Key::GroupNotch);
    let card = b.card(5.0 * ROW_H);
    let rows: [f32; 5] = std::array::from_fn(|i| card.y + i as f32 * ROW_H);
    let cy = |y: f32| y + ROW_H / 2.0;

    let x = b.slider(
        b.g.right,
        cy(rows[0]),
        SliderId::NotchWidth,
        s.notch_width as i32,
        px_value(s.notch_width),
        true,
    );
    b.row_text(card, rows[0], b.t(Key::NotchWidth), None, false, x);

    let x = b.slider(
        b.g.right,
        cy(rows[1]),
        SliderId::NotchHeight,
        s.notch_height as i32,
        px_value(s.notch_height),
        true,
    );
    b.row_text(card, rows[1], b.t(Key::NotchHeight), None, false, x);
    b.divider(card, rows[1]);

    let changed = (s.notch_width, s.notch_height) != (NOTCH_W_DEFAULT, NOTCH_H_DEFAULT);
    let reset = b.t(Key::NotchReset);
    let w = b.button_w(&[&reset], ButtonKind::Secondary);
    let x = b.buttons(
        b.g.right,
        cy(rows[2]),
        vec![(Hit::NotchReset, reset, ButtonKind::Secondary, changed, w)],
    );
    b.row_text(card, rows[2], b.t(Key::NotchDefaultSize), None, false, x);
    b.divider(card, rows[2]);

    let items = [
        (NotchFill::Solid, Key::NotchFillSolid),
        (NotchFill::Transparent, Key::NotchFillTransparent),
    ]
    .iter()
    .map(|(v, k)| (Hit::NotchFill(*v), b.t(*k)))
    .collect();
    let x = b.segmented(
        Group::NotchFill,
        b.g.right,
        cy(rows[3]),
        items,
        index_of(NotchFill::ALL, &s.notch_fill),
    );
    let desc = Some((b.t(Key::NotchFillDesc), Tone::Secondary));
    b.row_text(card, rows[3], b.t(Key::NotchFill), desc, false, x);
    b.divider(card, rows[3]);

    let x = b.slider(
        b.g.right,
        cy(rows[4]),
        SliderId::NotchOpacity,
        i32::from(s.notch_opacity),
        percent_value(s.notch_opacity),
        s.notch_fill == NotchFill::Transparent,
    );
    b.row_text(card, rows[4], b.t(Key::NotchOpacity), None, false, x);
    b.divider(card, rows[4]);
}

fn page_key(p: Page) -> Key {
    match p {
        Page::Calendar => Key::PageCalendar,
        Page::Pomodoro => Key::PagePomodoro,
        Page::Reminders => Key::PageReminders,
        Page::Notes => Key::PageNotes,
        Page::Media => Key::PageMedia,
        Page::Mixer => Key::PageMixer,
        Page::Shelf => Key::PageShelf,
        Page::Search => Key::SearchTitle,
        Page::Clipboard => Key::PageClipboard,
        Page::Notifications => Key::PageNotifications,
        Page::System => Key::PageSystem,
        Page::Cli => Key::PageCli,
        Page::Shortcuts => Key::PageShortcuts,
        Page::Devices => Key::PageDevices,
        Page::Arrange => Key::PageArrange,
    }
}

/// "Halaman panel": the pages shown come first, in the owner's order, each
/// with a drag handle, up and down buttons and "Sembunyikan"; under
/// "Tersembunyi" come the others, each with "Tampilkan". The last page still
/// shown cannot be hidden. The header counts them and restores the defaults.
fn pages_card(b: &mut Builder, s: &State) {
    b.eyebrow(Key::PanelPages, b.g.content_w - 4.0);
    let shown = s.pages.visible();
    let hidden: Vec<Page> = s
        .pages
        .entries()
        .iter()
        .filter(|(_, on)| !on)
        .map(|(p, _)| *p)
        .collect();
    let (n, total) = (shown.len(), s.pages.entries().len());
    let extra = if hidden.is_empty() {
        0.0
    } else {
        SUBHEAD_H + hidden.len() as f32 * ROW_H
    };
    let card = b.card(HEADER_H + n as f32 * ROW_H + extra);
    let title = format!("{}: {n}/{total}", b.t(Key::PagesShown));
    let changed = s.pages != PanelPages::default();
    b.list_header(card, title, Hit::PageReset, changed);

    let top = card.y + HEADER_H;
    let drag = s.drag.filter(|d| d.list == List::Pages);
    let hide = b.t(Key::ListHide);
    // `row_text` starts a name INSET right of its card's edge.
    let text_card = Rect {
        x: card.x + PAGE_NAME_X - INSET,
        w: card.w - (PAGE_NAME_X - INSET),
        ..card
    };
    let glyph_at = |cy: f32| Rect {
        x: card.x + PAGE_ICON_X,
        y: cy - PAGE_ICON / 2.0,
        w: PAGE_ICON,
        h: PAGE_ICON,
    };
    for (slot, item) in display_order(n, drag.as_ref()).into_iter().enumerate() {
        let page = shown[item];
        let held = drag.filter(|d| d.from == item);
        let y = held.map_or(top + slot as f32 * ROW_H, |d| d.top);
        let cy = y + ROW_H / 2.0;
        b.divider(card, top + slot as f32 * ROW_H);
        if held.is_some() {
            b.l.lifted = Some(lifted_rect(card, y));
        }
        b.l.rows.push(ReorderRow {
            list: List::Pages,
            grip: Hit::PageGrip(page),
            rect: Rect {
                x: card.x,
                y,
                w: card.w,
                h: ROW_H,
            },
        });
        b.grip(card, cy, Hit::PageGrip(page));
        b.glyph(
            glyph_at(cy),
            crate::shortcuts::page_icon(page),
            Tone::Secondary,
            false,
        );
        let x = b.icon_button(
            b.g.right,
            cy,
            Hit::PageMove(page, 1),
            Icon::ChevD,
            item + 1 < n,
        );
        let x = b.icon_button(x - 4.0, cy, Hit::PageMove(page, -1), Icon::ChevU, item > 0);
        let locked = n == 1;
        let w = b.button_w(&[&hide], ButtonKind::Secondary);
        let button = (
            Hit::PageOn(page),
            hide.clone(),
            ButtonKind::Secondary,
            !locked,
            w,
        );
        let x = b.buttons(x - GAP, cy, vec![button]);
        let desc = locked.then(|| (b.t(Key::PagesLastShown), Tone::Secondary));
        b.row_text(text_card, y, b.t(page_key(page)), desc, false, x);
    }

    if !hidden.is_empty() {
        let y0 = top + n as f32 * ROW_H;
        b.sub_header(card, y0, b.t(Key::PagesHidden), true);
        let show = b.t(Key::ListShow);
        let w = b.button_w(&[&show], ButtonKind::Secondary);
        for (i, page) in hidden.iter().enumerate() {
            let y = y0 + SUBHEAD_H + i as f32 * ROW_H;
            if i > 0 {
                b.divider(card, y);
            }
            let button = (
                Hit::PageOn(*page),
                show.clone(),
                ButtonKind::Secondary,
                true,
                w,
            );
            let x = b.buttons(b.g.right, y + ROW_H / 2.0, vec![button]);
            b.glyph(
                glyph_at(y + ROW_H / 2.0),
                crate::shortcuts::page_icon(*page),
                Tone::Secondary,
                true,
            );
            b.row_text(text_card, y, b.t(page_key(*page)), None, false, x);
        }
    }
}

/// The raised plate under the row a drag holds, with its top at `y`.
fn lifted_rect(card: Rect, y: f32) -> Rect {
    Rect {
        x: card.x + 4.0,
        y,
        w: card.w - 8.0,
        h: ROW_H,
    }
}

/// A custom tile's target as a description: its kind, then the path or
/// address, cut short with "…" to fit `max_w`.
fn tile_target(b: &Builder, lang: Lang, kind: CustomKind, target: &str, max_w: f32) -> String {
    let kind = match kind {
        CustomKind::App => Key::TileKindApp,
        CustomKind::Folder => Key::TileKindFolder,
        CustomKind::Url => Key::TileKindUrl,
    };
    let head = format!("{} · ", text(lang, kind));
    let mut chars: Vec<char> = target.chars().collect();
    loop {
        let shown: String = chars.iter().collect();
        let line = format!("{head}{shown}");
        if chars.len() == target.chars().count() && b.w(&line, Font::Desc) <= max_w {
            return line;
        }
        if chars.is_empty() || b.w(&format!("{line}…"), Font::Desc) <= max_w {
            return format!("{line}…");
        }
        chars.pop();
    }
}

/// "Pintasan": the tiles shown come first, in the owner's order, each with
/// a drag handle, up and down buttons, "Sembunyikan" and (an own tile)
/// "Hapus"; the header counts them against the twelve the page takes and
/// restores the defaults. Under "Tersedia" are the tiles that are off, by
/// group, each with "+ Tambah" (off while twelve show), and the form that
/// makes a tile of the owner's own. The last tile still shown cannot be
/// hidden; a tile on past the twelfth (a list edited by hand) says it does
/// not show.
fn tiles_card(b: &mut Builder, s: &State) {
    b.eyebrow(Key::Shortcuts, b.g.content_w - 4.0);
    let entries = s.tiles.entries();
    let on_idx: Vec<usize> = (0..entries.len()).filter(|i| entries[*i].1).collect();
    let n = on_idx.len();
    let card = b.card(HEADER_H + n as f32 * ROW_H);
    let title = format!("{}: {}/{MAX_SHOWN}", b.t(Key::TilesShown), n.min(MAX_SHOWN));
    let mut defaults = s.tiles.clone();
    defaults.reset();
    b.list_header(card, title, Hit::TileReset, defaults != s.tiles);

    let top = card.y + HEADER_H;
    let drag = s.drag.filter(|d| d.list == List::Tiles);
    let (hide, delete) = (b.t(Key::ListHide), b.t(Key::Delete));
    let text_card = Rect {
        x: card.x + GRIP_W,
        w: card.w - GRIP_W,
        ..card
    };
    for (slot, item) in display_order(n, drag.as_ref()).into_iter().enumerate() {
        let i = on_idx[item];
        let held = drag.filter(|d| d.from == item);
        let y = held.map_or(top + slot as f32 * ROW_H, |d| d.top);
        let cy = y + ROW_H / 2.0;
        b.divider(card, top + slot as f32 * ROW_H);
        if held.is_some() {
            b.l.lifted = Some(lifted_rect(card, y));
        }
        b.l.rows.push(ReorderRow {
            list: List::Tiles,
            grip: Hit::TileGrip(i),
            rect: Rect {
                x: card.x,
                y,
                w: card.w,
                h: ROW_H,
            },
        });
        b.grip(card, cy, Hit::TileGrip(i));
        let x = b.icon_button(
            b.g.right,
            cy,
            Hit::TileMove(i, 1),
            Icon::ChevD,
            item + 1 < n,
        );
        let x = b.icon_button(x - 4.0, cy, Hit::TileMove(i, -1), Icon::ChevU, item > 0);
        let locked = n == 1;
        let w = b.button_w(&[&hide], ButtonKind::Secondary);
        let button = (
            Hit::TileOn(i),
            hide.clone(),
            ButtonKind::Secondary,
            !locked,
            w,
        );
        let mut x = b.buttons(x - GAP, cy, vec![button]);
        let note = if s.tiles.beyond_limit(i) {
            Some(Key::TileNotShown)
        } else if locked {
            Some(Key::TilesLastShown)
        } else {
            None
        };
        let note = note.map(|k| (b.t(k), Tone::Secondary));
        let (label, desc) = match &entries[i].0 {
            Tile::Builtin(sc) => (b.t(crate::shortcuts::label(*sc)), note),
            Tile::Custom(c) => {
                let w = b.compact_w(&delete);
                let item = (
                    Hit::TileRemove(i),
                    delete.clone(),
                    ButtonKind::Secondary,
                    true,
                    w,
                );
                x = b.buttons(x - GAP, cy, vec![item]);
                // Why it does not show matters more than where it points.
                let desc = note.unwrap_or_else(|| {
                    let max_w = row_text_w(text_card, x);
                    (
                        tile_target(b, s.lang, c.kind, &c.target, max_w),
                        Tone::Secondary,
                    )
                });
                (c.label.clone(), Some(desc))
            }
        };
        b.row_text(text_card, y, label, desc, false, x);
    }

    available_card(b, s);
}

/// "Tersedia": the tiles that are off, grouped, and the own-shortcut form.
fn available_card(b: &mut Builder, s: &State) {
    let entries = s.tiles.entries();
    let full = s.tiles.full();
    let groups: Vec<(TileGroup, Vec<usize>)> = TileGroup::ALL
        .iter()
        .map(|g| {
            let items: Vec<usize> = (0..entries.len())
                .filter(|i| !entries[*i].1 && group(&entries[*i].0) == *g)
                .collect();
            (*g, items)
        })
        .filter(|(_, items)| !items.is_empty())
        .collect();
    let list_h: f32 = groups
        .iter()
        .map(|(_, items)| SUBHEAD_H + items.len() as f32 * ROW_H)
        .sum();

    // The row that opens the form, or the form itself: heights first.
    let probe = Rect {
        x: b.g.content_x,
        y: 0.0,
        w: b.g.content_w,
        h: 0.0,
    };
    let can_open = s.tiles.customs() < MAX_CUSTOM;
    let open_label = b.t(Key::TileAddOwn);
    let open_w = b.button_w(&[&open_label], ButtonKind::Secondary);
    let desc_key = if can_open {
        Key::TileOwnDesc
    } else {
        Key::TileOwnFull
    };
    let mut desc_lines = vec![b.t(desc_key)];
    if let Some(k) = s.tile_note.filter(|_| can_open) {
        desc_lines.extend(b.wrap(&b.t(k), Font::Desc, row_text_w(probe, b.g.right - open_w)));
    }
    let error_lines = s
        .tile_form
        .and_then(|f| f.error)
        .map(|k| b.wrap(&b.t(k), Font::Desc, b.g.right - b.g.content_x - INSET))
        .unwrap_or_default();
    let error_h = if error_lines.is_empty() {
        0.0
    } else {
        error_lines.len() as f32 * DESC_LINE_H + 8.0
    };
    let tail_h = match s.tile_form {
        Some(_) => FORM_PAD + 3.0 * FIELD_ROW_H + error_h + ROW_H,
        None => wrapped_row_h(desc_lines.len()),
    };

    b.eyebrow(Key::TilesAvailable, b.g.content_w - 4.0);
    let card = b.card(list_h + tail_h);
    let (add, delete) = (b.t(Key::TileAdd), b.t(Key::Delete));
    let add_w = b.button_w(&[&add], ButtonKind::Secondary);
    let delete_w = b.compact_w(&delete);
    let mut y = card.y;
    for (g, items) in groups {
        let key = match g {
            TileGroup::Pages => Key::TileGroupPages,
            TileGroup::Windows => Key::TileGroupWindows,
            TileGroup::Vale => Key::TileGroupVale,
            TileGroup::Own => Key::TileGroupOwn,
        };
        b.sub_header(card, y, b.t(key), y > card.y);
        y += SUBHEAD_H;
        for (k, i) in items.into_iter().enumerate() {
            let cy = y + ROW_H / 2.0;
            if k > 0 {
                b.divider(card, y);
            }
            let button = (
                Hit::TileOn(i),
                add.clone(),
                ButtonKind::Secondary,
                !full,
                add_w,
            );
            let mut x = b.buttons(b.g.right, cy, vec![button]);
            let note = full.then(|| (b.t(Key::TilePageFull), Tone::Secondary));
            let (label, desc) = match &entries[i].0 {
                Tile::Builtin(sc) => (b.t(crate::shortcuts::label(*sc)), note),
                Tile::Custom(c) => {
                    let item = (
                        Hit::TileRemove(i),
                        delete.clone(),
                        ButtonKind::Secondary,
                        true,
                        delete_w,
                    );
                    x = b.buttons(x - GAP, cy, vec![item]);
                    let desc = note.unwrap_or_else(|| {
                        let max_w = row_text_w(card, x);
                        (
                            tile_target(b, s.lang, c.kind, &c.target, max_w),
                            Tone::Secondary,
                        )
                    });
                    (c.label.clone(), Some(desc))
                }
            };
            b.row_text(card, y, label, desc, false, x);
            y += ROW_H;
        }
    }
    if y > card.y {
        b.divider(card, y);
    }
    match s.tile_form {
        None => {
            let button = (
                Hit::TileFormOpen,
                open_label,
                ButtonKind::Secondary,
                can_open,
                open_w,
            );
            let cy = y + tail_h / 2.0;
            let x = b.buttons(b.g.right, cy, vec![button]);
            let label = b.t(Key::TileGroupOwn);
            b.row_text_lines(card, y, label, desc_lines, Tone::Secondary, x);
        }
        Some(form) => tile_form(b, card, y, form, &error_lines),
    }
}

/// The own-shortcut form, from `y` down: Nama, Jenis, Target (with "Pilih…"
/// for an app or a folder), the message about the last try, "Batal" and
/// "Tambah".
fn tile_form(b: &mut Builder, card: Rect, y: f32, form: TileForm, error: &[String]) {
    let labels = [
        b.t(Key::TileFormName),
        b.t(Key::TileFormKind),
        b.t(Key::TileFormTarget),
    ];
    let label_w = labels
        .iter()
        .map(|l| b.w(l, Font::Label))
        .fold(0.0, f32::max);
    let field_x = card.x + INSET + label_w + 2.0 * GAP;
    let top = y + FORM_PAD;
    let row = |i: usize| top + i as f32 * FIELD_ROW_H;
    for (i, label) in labels.into_iter().enumerate() {
        let rect = Rect {
            x: card.x + INSET,
            y: row(i) + (FIELD_ROW_H - 20.0) / 2.0,
            w: label_w + GAP,
            h: 20.0,
        };
        b.text(rect, label, Font::Label, Tone::Primary, Align::Left);
    }
    let field = |i: usize, right: f32| Rect {
        x: field_x,
        y: row(i) + (FIELD_ROW_H - FIELD_H) / 2.0,
        w: right - field_x,
        h: FIELD_H,
    };
    b.l.fields.push((Field::TileName, field(0, b.g.right)));

    let kinds = [
        (CustomKind::App, Key::TileKindApp),
        (CustomKind::Folder, Key::TileKindFolder),
        (CustomKind::Url, Key::TileKindWeb),
    ];
    let items = kinds
        .iter()
        .map(|(k, key)| (Hit::TileFormKind(*k), b.t(*key)))
        .collect();
    let selected = index_of(&CustomKind::ALL, &form.kind);
    let kind_cy = row(1) + FIELD_ROW_H / 2.0;
    b.segmented(Group::TileKind, b.g.right, kind_cy, items, selected);

    let mut right = b.g.right;
    if form.kind != CustomKind::Url {
        let label = b.t(Key::TileBrowse);
        let w = b.button_w(&[&label], ButtonKind::Secondary);
        let button = (Hit::TileBrowse, label, ButtonKind::Secondary, true, w);
        right = b.buttons(b.g.right, row(2) + FIELD_ROW_H / 2.0, vec![button]) - GAP;
    }
    b.l.fields.push((Field::TileTarget, field(2, right)));

    let mut y = row(3);
    if !error.is_empty() {
        for (i, line) in error.iter().enumerate() {
            let rect = Rect {
                x: card.x + INSET,
                y: y + 2.0 + i as f32 * DESC_LINE_H,
                w: b.g.right - card.x - INSET,
                h: 16.0,
            };
            b.text(rect, line.clone(), Font::Desc, Tone::Bad, Align::Left);
        }
        y += error.len() as f32 * DESC_LINE_H + 8.0;
    }
    let (cancel, add) = (b.t(Key::Cancel), b.t(Key::TileFormAdd));
    let items = vec![
        (
            Hit::TileFormCancel,
            cancel.clone(),
            ButtonKind::Secondary,
            true,
            b.button_w(&[&cancel], ButtonKind::Secondary),
        ),
        (
            Hit::TileFormAdd,
            add.clone(),
            ButtonKind::Primary,
            true,
            b.button_w(&[&add], ButtonKind::Primary),
        ),
    ];
    b.buttons(b.g.right, y + ROW_H / 2.0, items);
}

/// The width left for a row's label and description, left of `control_x`.
fn row_text_w(card: Rect, control_x: f32) -> f32 {
    (control_x - 2.0 * GAP - (card.x + INSET)).max(0.0)
}

/// A row whose description takes `lines` lines.
fn wrapped_row_h(lines: usize) -> f32 {
    ROW_H + lines.saturating_sub(1) as f32 * DESC_LINE_H
}

/// "10 mnt", "10 min": the sleep stepper's value.
pub fn sleep_value(lang: Lang, minutes: u32) -> String {
    format!("{minutes} {}", text(lang, Key::MinutesShort))
}

/// The height of the Kucing preview card: the cat's tile and the idle notch
/// above, the expression chips under them.
pub const CAT_PREVIEW_H: f32 = 158.0;
/// The cat's tile in the preview (the cat about 64 px on it).
pub const CAT_TILE: f32 = 96.0;
/// The tiles of the "Aksesori" row, and the gap between them.
const ACC_TILE: f32 = 54.0;
const ACC_ROW_H: f32 = ROW_H + ACC_TILE + 12.0;

/// Kucing: the preview, then the cards "Kucing", "Gerak" and "Sapaan". While
/// the cat is off the rows that need it fade and take no clicks;
/// "Mengecil jadi mata" does not need it (the eyes show without the cat).
fn cat_page(b: &mut Builder, s: &State) {
    let preview = b.card(CAT_PREVIEW_H);
    b.l.cat_preview = Some(preview);
    let items: Vec<(Hit, String)> = CatFace::ALL
        .iter()
        .map(|f| (Hit::CatFace(*f), b.t(f.key())))
        .collect();
    let selected = index_of(&CatFace::ALL, &s.cat_face);
    b.segmented(
        Group::CatFace,
        b.g.right,
        preview.bottom() - 12.0 - CONTROL_H / 2.0,
        items,
        selected,
    );

    let on = s.robot;
    titled(b, Key::GroupCat);
    let card = b.card(3.0 * ROW_H + ACC_ROW_H);
    let rows = [
        card.y,
        card.y + ROW_H,
        card.y + 2.0 * ROW_H,
        card.y + 3.0 * ROW_H,
    ];
    let cy = |y: f32| y + ROW_H / 2.0;

    let x = b.toggle(Hit::Robot, b.g.right, cy(rows[0]), on);
    let desc = Some((b.t(Key::RobotInNotchDesc), Tone::Secondary));
    b.row_text(card, rows[0], b.t(Key::RobotInNotch), desc, false, x);

    // "Warna": the four coats; the chosen one's name is the description.
    let n = CatColor::ALL.len() as f32;
    let x0 = b.g.right - (n * SWATCH + (n - 1.0) * SWATCH_GAP);
    let items = CatColor::ALL
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let r = Rect {
                x: x0 + i as f32 * (SWATCH + SWATCH_GAP),
                y: cy(rows[1]) - SWATCH / 2.0,
                w: SWATCH,
                h: SWATCH,
            };
            (*v, r)
        })
        .collect();
    b.l.controls.push(Control::CatSwatches {
        items,
        selected: s.cat_color,
        enabled: on,
    });
    let desc = Some((b.t(cat_color_key(s.cat_color)), Tone::Secondary));
    b.row_text(
        card,
        rows[1],
        b.t(Key::CatColorLabel),
        desc,
        false,
        x0 - SWATCH_SLOP,
    );
    b.divider(card, rows[1]);

    // "Latar: <name>" and "Ubah": the picker lives in Tampilan.
    let change = b.t(Key::CatBackdropChange);
    let w = b.button_w(&[&change], ButtonKind::Secondary);
    let item = (Hit::CatBackdrop, change, ButtonKind::Secondary, on, w);
    let x = b.buttons(b.g.right, cy(rows[2]), vec![item]);
    let shown = s.surface.resolve(s.cat_color, s.robot);
    let label = format!(
        "{}: {}",
        b.t(Key::CatBackdrop),
        text(b.lang, surface_key(shown))
    );
    let desc = (s.surface == Surface::Auto).then(|| (b.t(Key::SurfaceAuto), Tone::Secondary));
    b.row_text(card, rows[2], label, desc, false, x);
    b.divider(card, rows[2]);

    // "Aksesori": the chosen one's name, and a tile for each below it.
    let name = b.t(accessory_key(s.cat_accessory));
    b.row_text(
        card,
        rows[3],
        b.t(Key::CatAccessoryLabel),
        Some((name, Tone::Secondary)),
        false,
        b.g.right,
    );
    let n = CatAccessory::ALL.len() as f32;
    let gap = (b.g.right - (card.x + INSET) - n * ACC_TILE) / (n - 1.0);
    let items = CatAccessory::ALL
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let r = Rect {
                x: card.x + INSET + i as f32 * (ACC_TILE + gap),
                y: rows[3] + ROW_H,
                w: ACC_TILE,
                h: ACC_TILE,
            };
            (*v, r)
        })
        .collect();
    b.l.controls.push(Control::AccessoryTiles {
        items,
        selected: s.cat_accessory,
        enabled: on,
    });
    b.divider(card, rows[3]);

    cat_motion_card(b, s);

    titled(b, Key::GroupGreeting);
    let line = crate::i18n::greeting_periods_line(b.lang);
    let periods = b.wrap(&line, Font::Desc, b.g.content_w - 2.0 * INSET);
    let card = b.card(ROW_H + periods.len() as f32 * DESC_LINE_H + 14.0);
    let x = b.toggle_if(Hit::Greeting, b.g.right, cy(card.y), s.greeting, on);
    let desc = Some((b.t(Key::MorningGreetingDesc), Tone::Secondary));
    b.row_text(card, card.y, b.t(Key::CatGreeting), desc, false, x);
    b.divider(card, card.y + ROW_H);
    for (i, line) in periods.into_iter().enumerate() {
        let rect = Rect {
            x: card.x + INSET,
            y: card.y + ROW_H + 8.0 + i as f32 * DESC_LINE_H,
            w: b.g.content_w - 2.0 * INSET,
            h: 16.0,
        };
        b.text(rect, line, Font::Desc, Tone::Secondary, Align::Left);
    }
}

/// "Gerak": Animasi, Tidur setelah, Ikuti kursor and Mengecil jadi mata. The
/// Animasi description wraps beside its three segments (the design's full
/// sentence) and that row grows by a line.
fn cat_motion_card(b: &mut Builder, s: &State) {
    let modes = [
        (Motion::Full, Key::MotionFull),
        (Motion::Saver, Key::MotionSaver),
        (Motion::Off, Key::MotionOff),
    ];
    let items: Vec<(Hit, String)> = modes
        .iter()
        .map(|(v, k)| (Hit::Motion(*v), b.t(*k)))
        .collect();
    let probe = Rect {
        x: b.g.content_x,
        y: 0.0,
        w: b.g.content_w,
        h: 0.0,
    };
    let desc_w = row_text_w(probe, b.g.right - b.segmented_w(&items));
    let motion_desc = b.wrap(&b.t(Key::RobotMotionDesc), Font::Desc, desc_w);
    let motion_h = wrapped_row_h(motion_desc.len());

    titled(b, Key::GroupMotion);
    let card = b.card(4.0 * ROW_H + motion_h);
    let heights = [motion_h, ROW_H, ROW_H, ROW_H, ROW_H];
    let mut rows = [card.y; 5];
    for i in 1..5 {
        rows[i] = rows[i - 1] + heights[i - 1];
    }
    let cy = |i: usize| rows[i] + heights[i] / 2.0;
    let on = s.robot;

    let selected = index_of(Motion::ALL, &s.robot_motion);
    let x = b.segmented_if(Group::Motion, b.g.right, cy(0), items, selected, on);
    let label = b.t(Key::RobotMotion);
    b.row_text_lines(card, rows[0], label, motion_desc, Tone::Secondary, x);

    let m = s.robot_sleep;
    let x = b.stepper(
        b.g.right,
        cy(1),
        sleep_value(b.lang, m),
        Hit::RobotSleep(-1),
        Hit::RobotSleep(1),
        (m > SLEEP_MIN, m < SLEEP_MAX),
        on && s.robot_motion != Motion::Off,
    );
    let desc = Some((b.t(Key::RobotSleepDesc), Tone::Secondary));
    b.row_text(card, rows[1], b.t(Key::RobotSleep), desc, false, x);
    b.divider(card, rows[1]);

    let x = b.toggle_if(Hit::CatFollow, b.g.right, cy(2), s.cat_follow, on);
    let desc = Some((b.t(Key::CatFollowDesc), Tone::Secondary));
    b.row_text(card, rows[2], b.t(Key::CatFollow), desc, false, x);
    b.divider(card, rows[2]);

    let x = b.toggle_if(Hit::CatDance, b.g.right, cy(3), s.cat_dance, on);
    let desc = Some((b.t(Key::CatDanceDesc), Tone::Secondary));
    b.row_text(card, rows[3], b.t(Key::CatDance), desc, false, x);
    b.divider(card, rows[3]);

    // The eyes need stay-behind off, not the cat.
    let x = b.toggle_if(
        Hit::NotchPeek,
        b.g.right,
        cy(4),
        s.notch_peek,
        !s.stay_behind,
    );
    let desc = if s.stay_behind {
        b.t(Key::CatPeekBehind)
    } else {
        b.t(Key::CatPeekDesc)
    };
    b.row_text(
        card,
        rows[4],
        b.t(Key::CatPeek),
        Some((desc, Tone::Secondary)),
        false,
        x,
    );
    b.divider(card, rows[4]);
}

/// Umum: "Dasar" (language, autostart) and "Peringatan" (battery, disk and
/// RAM, and how to turn off Windows' pop-ups). The search hotkey lives in
/// the Tombol pintas section.
fn general(b: &mut Builder, s: &State) {
    titled(b, Key::GroupBasics);
    let card = b.card(2.0 * ROW_H);
    let rows = [card.y, card.y + ROW_H];
    let cy = |y: f32| y + ROW_H / 2.0;

    let items = vec![
        (Hit::Lang(Lang::Id), b.t(Key::LangIndonesian)),
        (Hit::Lang(Lang::En), b.t(Key::LangEnglish)),
    ];
    let selected = usize::from(s.lang == Lang::En);
    let x = b.segmented(Group::Lang, b.g.right, cy(rows[0]), items, selected);
    let desc = Some((b.t(Key::LanguageDesc), Tone::Secondary));
    b.row_text(card, rows[0], b.t(Key::Language), desc, false, x);

    let x = b.toggle(Hit::Autostart, b.g.right, cy(rows[1]), s.autostart);
    let desc = Some((b.t(Key::AutostartDesc), Tone::Secondary));
    b.row_text(card, rows[1], b.t(Key::Autostart), desc, false, x);
    b.divider(card, rows[1]);

    titled(b, Key::GroupAlerts);
    warnings_card(b, s);
}

/// "Peringatan" (its title is `general`'s): the battery, disk and RAM
/// steppers and the Windows pop-ups help. The battery stepper is off on a PC
/// without one.
fn warnings_card(b: &mut Builder, s: &State) {
    let card = b.card(4.0 * ROW_H);
    let rows: [f32; 4] = std::array::from_fn(|i| card.y + i as f32 * ROW_H);
    let cy = |y: f32| y + ROW_H / 2.0;

    let (min, max, _) = BATTERY_RANGE;
    let t = s.battery_threshold;
    let x = b.stepper(
        b.g.right,
        cy(rows[0]),
        format!("{t}%"),
        Hit::Battery(-1),
        Hit::Battery(1),
        (t > min, t < max),
        s.has_battery,
    );
    let desc = if s.has_battery {
        (b.t(Key::BatteryAlertDesc), Tone::Secondary)
    } else {
        (b.t(Key::NoBattery), Tone::Secondary)
    };
    b.row_text(card, rows[0], b.t(Key::BatteryAlert), Some(desc), false, x);

    let (min, max, _) = DISK_WARN_RANGE;
    let gb = s.disk_warn_gb;
    let x = b.stepper(
        b.g.right,
        cy(rows[1]),
        disk_warn_value(gb),
        Hit::DiskWarn(-1),
        Hit::DiskWarn(1),
        (gb > min, gb < max),
        true,
    );
    let desc = Some((b.t(Key::DiskAlertDesc), Tone::Secondary));
    b.row_text(card, rows[1], b.t(Key::DiskAlert), desc, false, x);
    b.divider(card, rows[1]);

    let (min, max, _) = RAM_WARN_RANGE;
    let pct = s.ram_warn_percent;
    let x = b.stepper(
        b.g.right,
        cy(rows[2]),
        format!("{pct}%"),
        Hit::RamWarn(-1),
        Hit::RamWarn(1),
        (pct > min, pct < max),
        true,
    );
    let desc = Some((b.t(Key::RamAlertDesc), Tone::Secondary));
    b.row_text(card, rows[2], b.t(Key::RamAlert), desc, false, x);
    b.divider(card, rows[2]);

    let label = b.t(Key::ShowHow);
    let w = b.button_w(&[&label], ButtonKind::Secondary);
    let item = (Hit::DndHelp, label, ButtonKind::Secondary, true, w);
    let x = b.buttons(b.g.right, cy(rows[3]), vec![item]);
    let desc = Some((b.t(Key::PopupsHelpDesc), Tone::Secondary));
    b.row_text(card, rows[3], b.t(Key::PopupsHelp), desc, false, x);
    b.divider(card, rows[3]);
}

/// Panel: "Perilaku" (Mode buka, Di belakang jendela), then the lists "Halaman
/// panel", "Pintasan" and "Tersedia".
fn panel_page(b: &mut Builder, s: &State) {
    titled(b, Key::GroupBehavior);
    let card = b.card(2.0 * ROW_H);
    let rows = [card.y, card.y + ROW_H];
    let cy = |y: f32| y + ROW_H / 2.0;

    let items = vec![
        (Hit::OpenMode(OpenMode::Hover), b.t(Key::OpenModeHover)),
        (Hit::OpenMode(OpenMode::HoverStay), b.t(Key::OpenModeStay)),
        (Hit::OpenMode(OpenMode::Click), b.t(Key::OpenModeClick)),
    ];
    let selected = index_of(OpenMode::ALL, &s.open_mode);
    let x = b.segmented(Group::OpenMode, b.g.right, cy(rows[0]), items, selected);
    let desc = Some((b.t(Key::OpenModeDesc), Tone::Secondary));
    b.row_text(card, rows[0], b.t(Key::OpenModeSetting), desc, false, x);

    let x = b.toggle(Hit::StayBehind, b.g.right, cy(rows[1]), s.stay_behind);
    let desc = Some((b.t(Key::StayBehindDesc), Tone::Secondary));
    b.row_text(card, rows[1], b.t(Key::StayBehind), desc, false, x);
    b.divider(card, rows[1]);

    pages_card(b, s);
    tiles_card(b, s);
}

/// Fitur: "Pomodoro" (its four steppers and the running note), "Musik",
/// "Notifikasi",
/// "CLI", "Perangkat" and "Pembersih".
fn features_page(b: &mut Builder, s: &State) {
    // The template editor takes the whole page while it is open.
    if let Some(e) = &s.arrange_edit {
        arrange_editor(b, e);
        return;
    }
    titled(b, Key::GroupPomodoro);
    let card = b.card(4.0 * ROW_H);
    let fields = [
        (PomoField::Focus, Key::Focus, Key::FocusDesc),
        (PomoField::ShortBreak, Key::Break, Key::BreakDesc),
        (PomoField::LongBreak, Key::LongBreak, Key::LongBreakDesc),
        (PomoField::Sessions, Key::Sessions, Key::SessionsDesc),
    ];
    for (i, (field, label, desc)) in fields.into_iter().enumerate() {
        let y = card.y + i as f32 * ROW_H;
        let v = pomo_get(&s.pomodoro, field);
        let (min, max, _) = pomodoro::range(field);
        let x = b.stepper(
            b.g.right,
            y + ROW_H / 2.0,
            pomo_value(b.lang, field, v),
            Hit::Pomo(field, -1),
            Hit::Pomo(field, 1),
            (v > min, v < max),
            !s.pomodoro_locked,
        );
        let desc = Some((b.t(desc), Tone::Secondary));
        b.row_text(card, y, b.t(label), desc, false, x);
        if i > 0 {
            b.divider(card, y);
        }
    }
    if s.pomodoro_locked {
        let y = card.bottom() + 12.0;
        b.l.note_glyph = Some(Rect {
            x: b.g.content_x + 4.0,
            y,
            w: 16.0,
            h: 18.0,
        });
        let rect = Rect {
            x: b.g.content_x + 26.0,
            y,
            w: b.g.content_w - 26.0,
            h: 18.0,
        };
        let note = b.t(Key::PomodoroLocked);
        b.text(rect, note, Font::Desc, Tone::Secondary, Align::Left);
        // The next title clears the note.
        b.y = y + 18.0 + 10.0;
    }

    titled(b, Key::GroupMusic);
    let card = b.card(ROW_H);
    // The description doubles as the privacy note.
    let x = b.toggle(Hit::Lyrics, b.g.right, card.y + ROW_H / 2.0, s.lyrics);
    let desc = Some((b.t(Key::LyricsInNotchDesc), Tone::Secondary));
    b.row_text(card, card.y, b.t(Key::LyricsInNotch), desc, false, x);

    arrange_card(b, s);
    notifications_card(b, s);

    titled(b, Key::GroupCli);
    let card = b.card(2.0 * ROW_H);
    let rows = [card.y, card.y + ROW_H];
    let cy = |y: f32| y + ROW_H / 2.0;
    // Just before the shell, as in keputusan-v3.1.html.
    let items = vec![
        (
            Hit::CliStart(CliStart::Explorer),
            b.t(Key::CliStartExplorer),
        ),
        (Hit::CliStart(CliStart::Last), b.t(Key::CliStartLast)),
    ];
    let selected = index_of(&CliStart::ALL, &s.cli_start);
    let x = b.segmented(Group::CliStart, b.g.right, cy(rows[0]), items, selected);
    let desc = Some((b.t(Key::CliStartFolderDesc), Tone::Secondary));
    b.row_text(card, rows[0], b.t(Key::CliStartFolder), desc, false, x);

    let items = vec![
        (Hit::Shell(Shell::PowerShell), b.t(Key::ShellPowerShell)),
        (Hit::Shell(Shell::Cmd), b.t(Key::ShellCmd)),
    ];
    let selected = index_of(&Shell::ALL, &s.shell);
    let x = b.segmented(Group::Shell, b.g.right, cy(rows[1]), items, selected);
    let desc = Some((b.t(Key::ShellCliDesc), Tone::Secondary));
    b.row_text(card, rows[1], b.t(Key::ShellCli), desc, false, x);
    b.divider(card, rows[1]);

    devices_card(b, s);

    titled(b, Key::GroupCleaner);
    let card = b.card(ROW_H);
    let x = b.toggle(
        Hit::MoleProtect,
        b.g.right,
        card.y + ROW_H / 2.0,
        s.mole_protect,
    );
    let desc = Some((b.t(Key::MoleProtectSettingDesc), Tone::Secondary));
    b.row_text(card, card.y, b.t(Key::MoleProtectSetting), desc, false, x);
}

/// "Jumlah maksimal" of the notification history one step (10) up or down,
/// kept in 10–200; a hand-edited value off the grid snaps onto it.
pub fn notif_items_step(n: u32, dir: i32) -> u32 {
    let step = CAP_STEP;
    let stepped = if dir > 0 {
        n.saturating_add(step)
    } else {
        n.saturating_sub(step)
    };
    (stepped / step * step).clamp(NOTIF_MIN, NOTIF_MAX)
}

/// "Notifikasi" (the Notifikasi page): "Simpan notifikasi", "Jumlah
/// maksimal", "Simpan setelah restart", the excluded apps (a row each with
/// its remove button, then a field and "Tambah"), and "Hapus riwayat
/// notifikasi" with two clicks.
fn notifications_card(b: &mut Builder, s: &State) {
    b.eyebrow(Key::SecNotifications, b.g.content_w - 4.0);
    let c = s.notif;
    let shown = s.muted.len().max(1);
    let card = b.card((7 + shown) as f32 * ROW_H);
    // Row 0 is "Notifikasi saat notch dipakai"; the numbers below count
    // from the history rows that follow it.
    let style_y = card.y;
    let row = |i: usize| card.y + (i + 1) as f32 * ROW_H;
    let cy = |y: f32| y + ROW_H / 2.0;
    let on = c.record;

    let items = vec![
        (
            Hit::NotifToastStyle(ToastStyle::Compact),
            b.t(Key::NotifToastCompact),
        ),
        (
            Hit::NotifToastStyle(ToastStyle::Full),
            b.t(Key::NotifToastFull),
        ),
    ];
    let selected = index_of(ToastStyle::ALL, &c.toast_style);
    let x = b.segmented(Group::NotifToastStyle, b.g.right, cy(style_y), items, selected);
    let desc = Some((b.t(Key::NotifToastStyleDesc), Tone::Secondary));
    b.row_text(card, style_y, b.t(Key::NotifToastStyle), desc, false, x);
    b.divider(card, row(0));

    let x = b.toggle(Hit::NotifRecord, b.g.right, cy(row(0)), on);
    let desc = Some((b.t(Key::NotifRecordDesc), Tone::Secondary));
    b.row_text(card, row(0), b.t(Key::NotifRecord), desc, false, x);

    let n = c.max_items;
    let x = b.stepper(
        b.g.right,
        cy(row(1)),
        n.to_string(),
        Hit::NotifItems(-1),
        Hit::NotifItems(1),
        (n > NOTIF_MIN, n < NOTIF_MAX),
        true,
    );
    let desc = Some((b.t(Key::NotifMaxDesc), Tone::Secondary));
    b.row_text(card, row(1), b.t(Key::NotifMax), desc, false, x);
    b.divider(card, row(1));

    let x = b.toggle(Hit::NotifPersist, b.g.right, cy(row(2)), c.persist);
    let desc = Some((b.t(Key::NotifPersistDesc), Tone::Secondary));
    b.row_text(card, row(2), b.t(Key::NotifPersist), desc, false, x);
    b.divider(card, row(2));

    // The excluded apps: the title row, a row each, then the add row.
    let desc = Some((b.t(Key::NotifMutedDesc), Tone::Secondary));
    b.row_text(card, row(3), b.t(Key::NotifMuted), desc, false, b.g.right);
    b.divider(card, row(3));
    let delete = b.t(Key::Delete);
    let delete_w = b.compact_w(&delete);
    if s.muted.is_empty() {
        let rect = Rect {
            x: card.x + INSET,
            y: row(4) + 16.0,
            w: b.g.right - card.x - INSET,
            h: 20.0,
        };
        b.text(
            rect,
            b.t(Key::NotifMutedNone),
            Font::Desc,
            Tone::Secondary,
            Align::Left,
        );
    }
    for (i, name) in s.muted.iter().enumerate() {
        let y = row(4 + i);
        let item = (
            Hit::NotifMutedRemove(i),
            delete.clone(),
            ButtonKind::Secondary,
            true,
            delete_w,
        );
        let x = b.buttons(b.g.right, cy(y), vec![item]);
        b.row_text(card, y, name.clone(), None, false, x);
    }
    let add_y = row(4 + shown);
    b.divider(card, add_y);
    let add = b.t(Key::NotifMutedAdd);
    let w = b.button_w(&[&add], ButtonKind::Secondary);
    let item = (Hit::NotifMutedAdd, add, ButtonKind::Secondary, true, w);
    let x = b.buttons(b.g.right, cy(add_y), vec![item]);
    let field_x = card.x + INSET;
    b.l.fields.push((
        Field::TileName,
        Rect {
            x: field_x,
            y: cy(add_y) - FIELD_H / 2.0,
            w: x - GAP - field_x,
            h: FIELD_H,
        },
    ));

    // Two clicks, as "Hapus riwayat" of the clipboard.
    let clear_y = row(5 + shown);
    b.divider(card, clear_y);
    let (label, sure) = (b.t(Key::ClipClearButton), b.t(Key::ConfirmShort));
    let w = b.button_w(&[&label, &sure], ButtonKind::Secondary);
    let item = if s.notif_confirm {
        (Hit::NotifClear, sure, ButtonKind::Confirm, true, w)
    } else {
        (Hit::NotifClear, label, ButtonKind::Secondary, true, w)
    };
    let x = b.buttons(b.g.right, cy(clear_y), vec![item]);
    let desc = Some((b.t(Key::NotifClearHistoryDesc), Tone::Secondary));
    b.row_text(card, clear_y, b.t(Key::NotifClearHistory), desc, false, x);
}

/// What the Susun card shows of the template editor while it is open (the
/// window keeps the tree and the selection; the layout only draws them).
#[derive(Clone, Debug, PartialEq)]
pub struct EditView {
    /// Renaming a saved arrangement: only the name field.
    pub rename: bool,
    pub root: Node,
    pub selected: usize,
    /// The Nama field holds a name of 1 to 24 characters.
    pub name_ok: bool,
}

impl EditView {
    /// Whether "Belah" in `dir` is allowed for the selected tile.
    pub fn can_split(&self, dir: Dir) -> bool {
        let mut probe = self.root.clone();
        probe.split(self.selected, dir)
    }

    /// "Gabung" needs more than one tile.
    pub fn can_merge(&self) -> bool {
        self.root.leaves() > 1
    }
}

/// How close (px) a press must be to a divider to take it.
const DIVIDER_SLOP: f32 = 6.0;

/// What a press on the editor's canvas takes.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasPress {
    /// A divider, by its path from the root.
    Divider(Vec<bool>),
    /// A tile, by its number.
    Tile(usize),
}

/// The tile or divider at `(x, y)` on `canvas` showing `root`: a divider
/// within `DIVIDER_SLOP` wins over the tile under it.
pub fn canvas_press(root: &Node, canvas: Rect, x: f32, y: f32) -> Option<CanvasPress> {
    if !canvas.contains(x, y) {
        return None;
    }
    let (fx, fy) = ((x - canvas.x) / canvas.w, (y - canvas.y) / canvas.h);
    let near = root
        .dividers()
        .into_iter()
        .filter_map(|d| {
            // Along the divider's own extent, and across it within the slop.
            let (across, along, size_across, lo, hi) = match d.dir {
                Dir::Cols => (fx, fy, canvas.w, d.region.y, d.region.bottom()),
                Dir::Rows => (fy, fx, canvas.h, d.region.x, d.region.right()),
            };
            let dist = (across - d.pos).abs() * size_across;
            (dist <= DIVIDER_SLOP && along >= lo && along <= hi).then_some((dist, d.path))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, path)) = near {
        return Some(CanvasPress::Divider(path));
    }
    root.to_rects()
        .iter()
        .position(|r| fx >= r.x && fx < r.right() && fy >= r.y && fy < r.bottom())
        .map(CanvasPress::Tile)
}

/// Drags the divider at `path` to the pointer at `(x, y)` on `canvas`: the
/// ratio inside its region, snapped and clamped by `Node::set_ratio`.
pub fn canvas_drag(root: &mut Node, canvas: Rect, path: &[bool], x: f32, y: f32) -> bool {
    let Some(d) = root.dividers().into_iter().find(|d| d.path == path) else {
        return false;
    };
    let (pos, start, size) = match d.dir {
        Dir::Cols => ((x - canvas.x) / canvas.w, d.region.x, d.region.w),
        Dir::Rows => ((y - canvas.y) / canvas.h, d.region.y, d.region.h),
    };
    size > 0.0 && root.set_ratio(path, (pos - start) / size)
}

const MINI_W: f32 = 48.0;
const MINI_H: f32 = 27.0;

/// A mini preview control for `node` at `(x, cy)`'s left edge.
fn mini(b: &mut Builder, x: f32, cy: f32, node: &Node) {
    let rect = Rect {
        x,
        y: cy - MINI_H / 2.0,
        w: MINI_W,
        h: MINI_H,
    };
    b.l.controls.push(Control::Mini {
        rect,
        rects: node.to_rects(),
        selected: None,
        canvas: false,
    });
}

/// Fitur -> Susun: the gap, the user's templates and the saved
/// arrangements. (The hotkey is in the Tombol pintas section.)
fn arrange_card(b: &mut Builder, s: &State) {
    b.eyebrow(Key::PageArrange, b.g.content_w - 4.0);
    let a = &s.arrange;
    let rows = 1 + 1 + a.templates.len().max(1) + 1 + a.saved.len().max(1);
    let card = b.card(rows as f32 * ROW_H);
    let row = |i: usize| card.y + i as f32 * ROW_H;
    let cy = |y: f32| y + ROW_H / 2.0;

    let x = b.slider(
        b.g.right,
        cy(row(0)),
        SliderId::ArrangeGap,
        a.gap as i32,
        format!("{} px", a.gap),
        true,
    );
    let desc = Some((b.t(Key::ArrangeGapDesc), Tone::Secondary));
    b.row_text(card, row(0), b.t(Key::ArrangeGap), desc, false, x);

    // My templates: the title row with "+ Template baru", then a row each.
    let full = a.templates.len() >= MAX_TEMPLATES;
    let add = b.t(Key::ArrangeNewTemplate);
    let w = b.button_w(&[&add], ButtonKind::Secondary);
    let item = (
        Hit::ArrangeTemplateNew,
        add,
        ButtonKind::Secondary,
        !full,
        w,
    );
    let x = b.buttons(b.g.right, cy(row(1)), vec![item]);
    let desc = full.then(|| (b.t(Key::ArrangeTemplatesFull), Tone::Secondary));
    b.row_text(card, row(1), b.t(Key::ArrangeMyTemplates), desc, false, x);
    b.divider(card, row(1));
    let (edit, delete) = (b.t(Key::ArrangeEdit), b.t(Key::ArrangeDelete));
    let (edit_w, delete_w) = (b.compact_w(&edit), b.compact_w(&delete));
    if a.templates.is_empty() {
        let rect = Rect {
            x: card.x + INSET,
            y: row(2) + 16.0,
            w: b.g.right - card.x - INSET,
            h: 20.0,
        };
        b.text(
            rect,
            b.t(Key::ArrangeNoTemplates),
            Font::Desc,
            Tone::Secondary,
            Align::Left,
        );
    }
    for (i, t) in a.templates.iter().enumerate() {
        let y = row(2 + i);
        let items = vec![
            (
                Hit::ArrangeTemplateEdit(i),
                edit.clone(),
                ButtonKind::Secondary,
                true,
                edit_w,
            ),
            (
                Hit::ArrangeTemplateDelete(i),
                delete.clone(),
                ButtonKind::Secondary,
                true,
                delete_w,
            ),
        ];
        let x = b.buttons(b.g.right, cy(y), items);
        mini(b, x - GAP - MINI_W, cy(y), &t.root);
        b.row_text(card, y, t.name.clone(), None, false, x - GAP - MINI_W);
    }

    // Saved arrangements: the title row, then a row each.
    let head = row(2 + a.templates.len().max(1));
    b.divider(card, head);
    b.row_text(
        card,
        head,
        b.t(Key::ArrangeSavedList),
        None,
        false,
        b.g.right,
    );
    let rename = b.t(Key::ArrangeRename);
    let rename_w = b.compact_w(&rename);
    if a.saved.is_empty() {
        let rect = Rect {
            x: card.x + INSET,
            y: head + ROW_H + 16.0,
            w: b.g.right - card.x - INSET,
            h: 20.0,
        };
        b.text(
            rect,
            b.t(Key::ArrangeNoSaved),
            Font::Desc,
            Tone::Secondary,
            Align::Left,
        );
    }
    for (i, saved) in a.saved.iter().enumerate() {
        let y = head + (i + 1) as f32 * ROW_H;
        let items = vec![
            (
                Hit::ArrangeSavedRename(i),
                rename.clone(),
                ButtonKind::Secondary,
                true,
                rename_w,
            ),
            (
                Hit::ArrangeSavedDelete(i),
                delete.clone(),
                ButtonKind::Secondary,
                true,
                delete_w,
            ),
        ];
        let x = b.buttons(b.g.right, cy(y), items);
        mini(b, x - GAP - MINI_W, cy(y), &saved.layout.node());
        b.row_text(card, y, saved.name.clone(), None, false, x - GAP - MINI_W);
    }
}

/// The template editor (or the rename field) in place of the Fitur page:
/// a 16:9 canvas with the tiles, the split and merge buttons, the Nama
/// field, Simpan and Batal.
fn arrange_editor(b: &mut Builder, e: &EditView) {
    b.eyebrow(Key::PageArrange, b.g.content_w - 4.0);
    let canvas_w = (b.g.content_w - 2.0 * INSET).min(400.0);
    let canvas_h = (canvas_w * 9.0 / 16.0).round();
    let top_h = if e.rename { 0.0 } else { canvas_h + 2.0 * 12.0 };
    let tool_h = if e.rename { 0.0 } else { ROW_H };
    let card = b.card(top_h + tool_h + 2.0 * ROW_H);
    let cy = |y: f32| y + ROW_H / 2.0;
    let mut y = card.y;
    if !e.rename {
        let canvas = Rect {
            x: card.x + (card.w - canvas_w) / 2.0,
            y: card.y + 12.0,
            w: canvas_w,
            h: canvas_h,
        };
        b.l.canvas = Some(canvas);
        b.l.controls.push(Control::Mini {
            rect: canvas,
            rects: e.root.to_rects(),
            selected: Some(e.selected),
            canvas: true,
        });
        y = card.y + top_h;
        b.divider(card, y);
        let (cols, rows, merge) = (
            b.t(Key::ArrangeSplitCols),
            b.t(Key::ArrangeSplitRows),
            b.t(Key::ArrangeMerge),
        );
        let w = b.button_w(&[&cols, &rows, &merge], ButtonKind::Secondary);
        let items = vec![
            (
                Hit::ArrangeSplit(Dir::Cols),
                cols,
                ButtonKind::Secondary,
                e.can_split(Dir::Cols),
                w,
            ),
            (
                Hit::ArrangeSplit(Dir::Rows),
                rows,
                ButtonKind::Secondary,
                e.can_split(Dir::Rows),
                w,
            ),
            (
                Hit::ArrangeMerge,
                merge,
                ButtonKind::Secondary,
                e.can_merge(),
                w,
            ),
        ];
        b.buttons(b.g.right, cy(y), items);
        y += ROW_H;
        b.divider(card, y);
    }
    // Nama: the field takes the row.
    let label = b.t(Key::ArrangeFieldName);
    let label_w = b.w(&label, Font::Control) + 12.0;
    let label_rect = Rect {
        x: card.x + INSET,
        y: cy(y) - 10.0,
        w: label_w,
        h: 20.0,
    };
    b.text(label_rect, label, Font::Control, Tone::Primary, Align::Left);
    let field_x = label_rect.right() + 4.0;
    b.l.fields.push((
        Field::TileName,
        Rect {
            x: field_x,
            y: cy(y) - FIELD_H / 2.0,
            w: b.g.right - field_x,
            h: FIELD_H,
        },
    ));
    y += ROW_H;
    b.divider(card, y);
    let (save, cancel) = (b.t(Key::ArrangeSaveButton), b.t(Key::ArrangeCancel));
    let (save_w, cancel_w) = (
        b.button_w(&[&save], ButtonKind::Primary),
        b.button_w(&[&cancel], ButtonKind::Secondary),
    );
    let items = vec![
        (
            Hit::ArrangeEditorSave,
            save,
            ButtonKind::Primary,
            e.name_ok,
            save_w,
        ),
        (
            Hit::ArrangeEditorCancel,
            cancel,
            ButtonKind::Secondary,
            true,
            cancel_w,
        ),
    ];
    b.buttons(b.g.right, cy(y), items);
}

/// "Perangkat" (the Devices page): the plug-in and safe-to-remove notices.
/// (The page's hotkey is in the Tombol pintas section.)
fn devices_card(b: &mut Builder, s: &State) {
    b.eyebrow(Key::PageDevices, b.g.content_w - 4.0);
    let card = b.card(ROW_H);
    let cy = card.y + ROW_H / 2.0;

    let x = b.toggle(Hit::DeviceNotices, b.g.right, cy, s.device_notices);
    let desc = Some((b.t(Key::DeviceNoticesDesc), Tone::Secondary));
    b.row_text(card, card.y, b.t(Key::DeviceNotices), desc, false, x);
}

/// "Jumlah item" one step (10) up or down, kept in 10–200; a hand-edited
/// value off the grid snaps onto it.
pub fn clip_items_step(n: u32, dir: i32) -> u32 {
    let stepped = if dir > 0 {
        n.saturating_add(ITEMS_STEP)
    } else {
        n.saturating_sub(ITEMS_STEP)
    };
    (stepped / ITEMS_STEP * ITEMS_STEP).clamp(MIN_ITEMS, MAX_ITEMS)
}

/// Settings → Clipboard after keputusan-v3.1.html §5 (`settingsWin`), with a
/// clear button; its hotkey lives in the Tombol pintas section. With the
/// history off the other rows fade and take no clicks.
fn clipboard_page(b: &mut Builder, s: &State) {
    let c = s.clip;
    titled(b, Key::GroupHistory);
    let card = b.card(7.0 * ROW_H);
    let rows: [f32; 7] = std::array::from_fn(|i| card.y + i as f32 * ROW_H);
    let cy = |y: f32| y + ROW_H / 2.0;
    let on = c.enabled;

    let x = b.toggle(Hit::ClipEnabled, b.g.right, cy(rows[0]), on);
    let desc = Some((b.t(Key::ClipHistoryDesc), Tone::Secondary));
    b.row_text(card, rows[0], b.t(Key::ClipHistory), desc, false, x);

    let n = c.max_items;
    let x = b.stepper(
        b.g.right,
        cy(rows[1]),
        n.to_string(),
        Hit::ClipItems(-1),
        Hit::ClipItems(1),
        (n > MIN_ITEMS, n < MAX_ITEMS),
        on,
    );
    let desc = Some((b.t(Key::ClipItemsDesc), Tone::Secondary));
    b.row_text(card, rows[1], b.t(Key::ClipItems), desc, false, x);
    b.divider(card, rows[1]);

    let x = b.toggle_if(Hit::ClipPersist, b.g.right, cy(rows[2]), c.persist, on);
    let desc = Some((b.t(Key::ClipPersistDesc), Tone::Secondary));
    b.row_text(card, rows[2], b.t(Key::ClipPersist), desc, false, x);
    b.divider(card, rows[2]);

    let x = b.toggle_if(
        Hit::ClipClearOnLock,
        b.g.right,
        cy(rows[3]),
        c.clear_on_lock,
        on,
    );
    let desc = Some((b.t(Key::ClipClearOnLockDesc), Tone::Secondary));
    b.row_text(card, rows[3], b.t(Key::ClipClearOnLock), desc, false, x);
    b.divider(card, rows[3]);

    // The copy preview needs a history entry to show: faded with it off.
    let x = b.toggle_if(Hit::ClipNotice, b.g.right, cy(rows[4]), c.copy_notice, on);
    let desc = Some((b.t(Key::ClipCopyNoticeDesc), Tone::Secondary));
    b.row_text(card, rows[4], b.t(Key::ClipCopyNotice), desc, false, x);
    b.divider(card, rows[4]);

    // While the preview is on it says what "Disalin" would, for the same
    // copies: this row fades and tells so.
    let x = b.toggle_if(
        Hit::ClipFeedbackAll,
        b.g.right,
        cy(rows[5]),
        c.copied_feedback_all,
        on && !c.copy_notice,
    );
    let desc = if c.copy_notice {
        b.t(Key::ClipFeedbackAllReplaced)
    } else {
        b.t(Key::ClipFeedbackAllDesc)
    };
    b.row_text(
        card,
        rows[5],
        b.t(Key::ClipFeedbackAll),
        Some((desc, Tone::Secondary)),
        false,
        x,
    );
    b.divider(card, rows[5]);

    // Two clicks, as "Putuskan": the first arms it for a few seconds.
    let (label, sure) = (b.t(Key::ClipClearButton), b.t(Key::ConfirmShort));
    let w = b.button_w(&[&label, &sure], ButtonKind::Secondary);
    let item = if s.clip_confirm {
        (Hit::ClipClear, sure, ButtonKind::Confirm, true, w)
    } else {
        (Hit::ClipClear, label, ButtonKind::Secondary, true, w)
    };
    let x = b.buttons(b.g.right, cy(rows[6]), vec![item]);
    let desc = Some((b.t(Key::ClipClearDesc), Tone::Secondary));
    b.row_text(card, rows[6], b.t(Key::ClipClear), desc, false, x);
    b.divider(card, rows[6]);

    screenshot_card(b, s);
}

/// The narrowest and widest a hotkey field gets (it is measured from what it
/// shows in between).
pub const HOTKEY_FIELD_MIN_W: f32 = 150.0;
pub const HOTKEY_FIELD_MAX_W: f32 = 260.0;
/// The key caps inside a field: their height, the padding inside one, the
/// gap that holds the "+" between two, and the field's own side padding.
pub const HOTKEY_CAP_H: f32 = 20.0;
pub const HOTKEY_CAP_PAD: f32 = 7.0;
pub const HOTKEY_PLUS_W: f32 = 14.0;
pub const HOTKEY_FIELD_PAD: f32 = 6.0;
/// The room round plain text in a field ("Tidak ada", the recording prompt).
const HOTKEY_TEXT_PAD: f32 = 14.0;

/// Why the combo just typed was refused (window state, shown in red under the
/// row's name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectMsg {
    Reserved,
    NeedsModifier,
    /// Another row already has it.
    UsedBy(HotkeyId),
}

/// The name of a hotkey row.
pub fn hotkey_row_key(id: HotkeyId) -> Key {
    match id {
        HotkeyId::Search => Key::HotkeyRowSearch,
        HotkeyId::Clipboard => Key::HotkeyRowClipboard,
        HotkeyId::Devices => Key::HotkeyRowDevices,
        HotkeyId::Arrange => Key::HotkeyRowArrange,
        HotkeyId::Screenshot => Key::HotkeyRowScreenshot,
    }
}

fn hotkey_row_desc_key(id: HotkeyId) -> Key {
    match id {
        HotkeyId::Search => Key::HotkeyRowSearchDesc,
        HotkeyId::Clipboard => Key::HotkeyRowClipboardDesc,
        HotkeyId::Devices => Key::HotkeyRowDevicesDesc,
        HotkeyId::Arrange => Key::HotkeyRowArrangeDesc,
        HotkeyId::Screenshot => Key::HotkeyRowScreenshotDesc,
    }
}

/// The text of a refused combo in `lang`.
pub fn reject_text(lang: Lang, msg: RejectMsg) -> String {
    match msg {
        RejectMsg::Reserved => text(lang, Key::HotkeyReserved).to_owned(),
        RejectMsg::NeedsModifier => text(lang, Key::HotkeyNeedsModifier).to_owned(),
        RejectMsg::UsedBy(id) => crate::i18n::hotkey_used_by(lang, text(lang, hotkey_row_key(id))),
    }
}

/// How wide the key caps of a combo draw: a cap each (its text and padding),
/// a gap for the "+" between, and the field's side padding.
pub fn hotkey_caps_w(m: &dyn Measure, caps: &[String]) -> f32 {
    let caps_w: f32 = caps
        .iter()
        .map(|c| m.width(c, Font::Control) + 2.0 * HOTKEY_CAP_PAD)
        .sum();
    2.0 * HOTKEY_FIELD_PAD + caps_w + caps.len().saturating_sub(1) as f32 * HOTKEY_PLUS_W
}

/// How much of a row the name and the description keep at least: the longest
/// description measures about this with the generous estimate.
const HOTKEY_TEXT_ROOM: f32 = 210.0;

/// The widest a field may be in a card `card_w` wide: `HOTKEY_FIELD_MAX_W`,
/// less when that would leave the row's text less than `HOTKEY_TEXT_ROOM`
/// (at the minimum window), never below `HOTKEY_FIELD_MIN_W`.
pub fn hotkey_max_w(card_w: f32) -> f32 {
    let fixed = 2.0 * INSET + STEP_BUTTON + 3.0 * GAP;
    (card_w - fixed - HOTKEY_TEXT_ROOM).clamp(HOTKEY_FIELD_MIN_W, HOTKEY_FIELD_MAX_W)
}

/// A hotkey field's width: from what it shows (the caps, "Tidak ada" or the
/// recording prompt), kept between `HOTKEY_FIELD_MIN_W` and `hotkey_max_w`.
fn hotkey_field_w(b: &Builder, caps: &[String], recording: bool) -> f32 {
    let content = if recording {
        b.w(&b.t(Key::HotkeyRecording), Font::Control) + 2.0 * HOTKEY_TEXT_PAD
    } else if caps.is_empty() {
        b.w(&b.t(Key::HotkeyNone), Font::Control) + 2.0 * HOTKEY_TEXT_PAD
    } else {
        hotkey_caps_w(b.m, caps)
    };
    content.clamp(HOTKEY_FIELD_MIN_W, hotkey_max_w(b.g.content_w))
}

/// Tombol pintas: one card, a header (the hint and "Kembalikan semua") and a
/// row for each hotkey: its name and description (red when the last combo was
/// refused, or the one set is taken), the field that records it and its own
/// reset button.
fn hotkeys_page(b: &mut Builder, s: &State) {
    titled(b, Key::GroupHotkeys);
    let card = b.card(HEADER_H + HotkeyId::ALL.len() as f32 * ROW_H);

    // The header, laid out like a list's.
    let cy = card.y + HEADER_H / 2.0;
    let label = b.t(Key::HotkeysResetAll);
    let w = b.button_w(&[&label], ButtonKind::Secondary);
    let item = (
        Hit::HotkeysResetAll,
        label,
        ButtonKind::Secondary,
        !s.hotkeys.all_default(),
        w,
    );
    let x = b.buttons(b.g.right, cy, vec![item]);
    let rect = Rect {
        x: card.x + INSET,
        y: cy - 8.0,
        w: row_text_w(card, x),
        h: 16.0,
    };
    let hint = b.t(Key::HotkeysHint);
    b.text(rect, hint, Font::Desc, Tone::Secondary, Align::Left);

    for (i, id) in HotkeyId::ALL.into_iter().enumerate() {
        let y = card.y + HEADER_H + i as f32 * ROW_H;
        b.divider(card, y);
        let cy = y + ROW_H / 2.0;
        let value = s.hotkeys.get(id);
        let at_default = s.hotkeys.is_default(id);
        let left = b.icon_button(
            b.g.right,
            cy,
            Hit::HotkeyReset(id),
            Icon::Reset,
            !at_default,
        );
        let caps = value.map(|h: Hotkey| h.caps()).unwrap_or_default();
        let recording = s.hotkey_recording == Some(id);
        let w = hotkey_field_w(b, &caps, recording);
        let field = Rect {
            x: left - GAP - w,
            y: cy - CONTROL_H / 2.0,
            w,
            h: CONTROL_H,
        };
        b.l.controls.push(Control::HotkeyField {
            hit: Hit::HotkeyField(id),
            rect: field,
            caps,
            recording,
        });
        let desc = match s.hotkey_error {
            Some((who, msg)) if who == id => (reject_text(b.lang, msg), Tone::Bad),
            _ if value.is_some() && s.hotkey_failed[id.index()] => {
                (b.t(Key::HotkeyTaken), Tone::Bad)
            }
            _ => (b.t(hotkey_row_desc_key(id)), Tone::Secondary),
        };
        b.row_text(card, y, b.t(hotkey_row_key(id)), Some(desc), false, field.x);
    }
}

/// "Lama tampil" one step (2 s) up or down, kept in `SHOT_SECS_RANGE`; a
/// hand-edited value off the grid snaps onto it.
pub fn shot_secs_step(n: u32, dir: i32) -> u32 {
    let (min, max, step) = SHOT_SECS_RANGE;
    let stepped = if dir > 0 {
        n.saturating_add(step)
    } else {
        n.saturating_sub(step)
    };
    (stepped / step * step).clamp(min, max)
}

/// The screenshot preview (keputusan-v3.1.html p6, its `settingsWin`
/// "Screenshot") in a second card under the history's seven rows, named by
/// an eyebrow; the section scrolls to reach it. With the preview off the
/// other two rows fade and take no clicks. Simpan is the default.
fn screenshot_card(b: &mut Builder, s: &State) {
    let c = s.shot;
    b.eyebrow(Key::SecScreenshot, b.g.content_w - 4.0);
    let card = b.card(4.0 * ROW_H);
    let rows: [f32; 4] = std::array::from_fn(|i| card.y + i as f32 * ROW_H);
    let cy = |y: f32| y + ROW_H / 2.0;
    let on = c.preview;

    // The tool comes first and never fades: it decides how a screenshot is
    // taken, the preview only what the notch does with a new one.
    let items = vec![
        (Hit::ShotTool(ShotTool::Vale), b.t(Key::ShotToolVale)),
        (Hit::ShotTool(ShotTool::Windows), b.t(Key::ShotToolWindows)),
    ];
    let selected = index_of(ShotTool::ALL, &c.tool);
    let x = b.segmented(Group::ShotTool, b.g.right, cy(rows[0]), items, selected);
    let desc = Some((b.t(Key::ShotToolDesc), Tone::Secondary));
    b.row_text(card, rows[0], b.t(Key::ShotToolSetting), desc, false, x);

    // (The key that starts Vale's capture is in the Tombol pintas section.)
    let x = b.toggle(Hit::ShotPreview, b.g.right, cy(rows[1]), on);
    b.row_text(card, rows[1], b.t(Key::ShotPreviewSetting), None, false, x);
    b.divider(card, rows[1]);

    let items = vec![
        (Hit::ShotIgnore(ShotIgnore::Keep), b.t(Key::ShotKeep)),
        (Hit::ShotIgnore(ShotIgnore::Delete), b.t(Key::ShotDelete)),
    ];
    let selected = index_of(ShotIgnore::ALL, &c.on_ignore);
    let x = b.segmented_if(
        Group::ShotIgnore,
        b.g.right,
        cy(rows[2]),
        items,
        selected,
        on,
    );
    let desc = Some((b.t(Key::ShotIgnoreDesc), Tone::Secondary));
    b.row_text(card, rows[2], b.t(Key::ShotIgnoreSetting), desc, false, x);
    b.divider(card, rows[2]);

    let (min, max, _) = SHOT_SECS_RANGE;
    let n = c.show_secs;
    let x = b.stepper(
        b.g.right,
        cy(rows[3]),
        crate::i18n::seconds_short(b.lang, n),
        Hit::ShotSecs(-1),
        Hit::ShotSecs(1),
        (n > min, n < max),
        on,
    );
    b.row_text(card, rows[3], b.t(Key::ShotShowFor), None, false, x);
    b.divider(card, rows[3]);
}

fn integrations(b: &mut Builder, s: &State) {
    let g = s.google;
    // Above the card: its name, and the setup guide on the right.
    let guide = b.t(Key::SetupGuide);
    let w = b.button_w(&[&guide], ButtonKind::Link);
    let item = (Hit::GoogleGuide, guide, ButtonKind::Link, true, w);
    let x = b.buttons(
        b.g.content_x + b.g.content_w + LINK_PAD,
        b.y + EYEBROW_H / 2.0,
        vec![item],
    );
    b.eyebrow(Key::GoogleCalendar, x - GAP - b.g.content_x - 2.0);
    // With Vale's own client the card is one button; the user's own Client
    // ID and secret wait under "Lanjutan".
    // An own ID in the field keeps them open: it is the one a login uses.
    let fields = !g.built_in || g.advanced || g.has_id;
    let card = b.card(if fields {
        6.0 + 2.0 * FIELD_ROW_H + ROW_H
    } else {
        ROW_H
    });

    // Client ID and secret: label column, then the field.
    let top = card.y + 6.0;
    if fields {
        let labels = [b.t(Key::ClientId), b.t(Key::ClientSecret)];
        let label_w = labels
            .iter()
            .map(|l| b.w(l, Font::Label))
            .fold(0.0, f32::max);
        let field_x = card.x + INSET + label_w + 2.0 * GAP;
        for (i, (field, label)) in [Field::ClientId, Field::ClientSecret]
            .into_iter()
            .zip(labels)
            .enumerate()
        {
            let y = top + i as f32 * FIELD_ROW_H;
            let rect = Rect {
                x: card.x + INSET,
                y: y + (FIELD_ROW_H - 20.0) / 2.0,
                w: label_w + GAP,
                h: 20.0,
            };
            b.text(rect, label, Font::Label, Tone::Primary, Align::Left);
            b.l.fields.push((
                field,
                Rect {
                    x: field_x,
                    y: y + (FIELD_ROW_H - FIELD_H) / 2.0,
                    w: b.g.right - field_x,
                    h: FIELD_H,
                },
            ));
        }
    }
    let buttons_y = if fields {
        top + 2.0 * FIELD_ROW_H
    } else {
        card.y
    };

    // Buttons, with the status chip (or a short message) on their left.
    let (save, connect) = (b.t(Key::Save), b.t(Key::Connect));
    let sign_in = b.t(Key::SignInGoogle);
    let (disconnect, sure) = (b.t(Key::Disconnect), b.t(Key::ConfirmShort));
    // A login to forget: connected, offline, or one Google stopped accepting.
    let can_disconnect = matches!(
        g.status,
        CalStatus::Ok | CalStatus::Offline | CalStatus::Reconnect
    );
    let needs_login = matches!(g.status, CalStatus::NeedsLogin | CalStatus::Reconnect);
    let kind = |primary: bool| {
        if primary {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        }
    };
    // A client to log in with (Vale's, or a typed Client ID), and not
    // connected already.
    let has_client = g.built_in || (g.has_id && (g.status != CalStatus::NotConfigured || g.dirty));
    let can_connect = has_client && g.status != CalStatus::Ok;
    let mut items = Vec::new();
    if fields {
        items.push((
            Hit::GoogleSave,
            save.clone(),
            kind(g.dirty),
            g.dirty,
            b.button_w(&[&save], ButtonKind::Secondary),
        ));
    }
    let connect = if fields { connect } else { sign_in };
    items.push((
        Hit::GoogleConnect,
        connect.clone(),
        kind(!g.dirty && can_connect && needs_login),
        can_connect,
        b.button_w(&[&connect], ButtonKind::Secondary),
    ));
    items.push((
        Hit::GoogleDisconnect,
        if g.confirm {
            sure.clone()
        } else {
            disconnect.clone()
        },
        if g.confirm {
            ButtonKind::Confirm
        } else {
            ButtonKind::Secondary
        },
        can_disconnect,
        b.button_w(&[&disconnect, &sure], ButtonKind::Secondary),
    ));
    let row_cy = buttons_y + ROW_H / 2.0;
    let x = b.buttons(b.g.right, row_cy, items);
    let left_w = (x - 2.0 * GAP - card.x - INSET).max(0.0);
    match s.google_note {
        // A message after a click stands in for the status for a few seconds.
        Some((note, tone)) => {
            let rect = Rect {
                x: card.x + INSET,
                y: row_cy - 9.0,
                w: left_w,
                h: 18.0,
            };
            b.text(rect, note.to_owned(), Font::Desc, tone, Align::Left);
        }
        None => {
            let (key, tone) = google_status_key(&g);
            b.chip(card.x + INSET, row_cy, b.t(key), tone, left_w);
        }
    }
    if g.built_in && !g.has_id {
        // Under the card, on the left: the way to the user's own client.
        let advanced = b.t(Key::GoogleAdvanced);
        let kind = ButtonKind::Disclosure(g.advanced);
        let w = b.button_w(&[&advanced], kind);
        let top = card.y + card.h + 4.0;
        let item = (Hit::GoogleAdvanced, advanced, kind, true, w);
        b.buttons(b.g.content_x + w, top + CONTROL_H / 2.0, vec![item]);
        b.y = top + CONTROL_H + 4.0;
    }

    // Claude Code hooks.
    b.eyebrow(Key::ClaudeCode, b.g.content_w - 2.0);
    let card = b.card(ROW_H);
    let (install, remove) = (b.t(Key::InstallHooks), b.t(Key::RemoveHooks));
    // Unreadable settings (`None`): both stay on, so a click reports why.
    let complete = s.hooks.is_some_and(|h| h.complete);
    let any = s.hooks.is_none_or(|h| h.any);
    let items = vec![
        (
            Hit::HooksInstall,
            install.clone(),
            kind(!complete),
            !complete,
            b.button_w(&[&install], ButtonKind::Secondary),
        ),
        (
            Hit::HooksRemove,
            remove.clone(),
            ButtonKind::Secondary,
            any,
            b.button_w(&[&remove], ButtonKind::Secondary),
        ),
    ];
    let x = b.buttons(b.g.right, card.y + ROW_H / 2.0, items);
    let desc = match s.hook_note {
        Some((note, tone)) => (note.to_owned(), tone),
        None => match s.hooks {
            Some(h) if h.complete => (b.t(Key::HooksOn), Tone::Good),
            Some(_) => (b.t(Key::HooksOff), Tone::Secondary),
            None => (b.t(Key::HooksUnknown), Tone::Warn),
        },
    };
    b.row_text(card, card.y, b.t(Key::ClaudeCode), Some(desc), true, x);
}

fn about(b: &mut Builder, s: &State) {
    // The logo with the name, version and tagline beside it.
    let top = FIRST_CARD_Y + 2.0;
    b.l.about_logo = Some(Rect {
        x: b.g.content_x + 2.0,
        y: top,
        w: ABOUT_LOGO,
        h: ABOUT_LOGO,
    });
    let text_x = b.g.content_x + 2.0 + ABOUT_LOGO + 16.0;
    let text_w = b.g.content_x + b.g.content_w - text_x;
    let line = |y, h| Rect {
        x: text_x,
        y,
        w: text_w,
        h,
    };
    b.text(
        line(top + 2.0, 26.0),
        "Vale".into(),
        Font::AboutName,
        Tone::Primary,
        Align::Left,
    );
    let version = version_label(b.lang, s.version);
    b.text(
        line(top + 28.0, 18.0),
        version,
        Font::AboutVersion,
        Tone::Secondary,
        Align::Left,
    );
    let tagline = b.t(Key::AboutTagline);
    b.text(
        line(top + 46.0, 18.0),
        tagline,
        Font::AboutVersion,
        Tone::Secondary,
        Align::Left,
    );
    b.y = top + ABOUT_LOGO + 18.0;
    titled(b, Key::GroupLinks);

    let card = b.card(4.0 * ROW_H);
    let row_y = |i: usize| card.y + i as f32 * ROW_H;
    // The source page opens from a bare "open" glyph; the update row and the
    // data folder have a button.
    let repo = (
        Hit::Repo,
        Key::SourceCode,
        "github.com/hartono0297/vale".to_owned(),
        Key::OpenGithub,
        ButtonKind::External,
    );
    let data = (
        Hit::DataFolder,
        Key::DataFolder,
        s.data_dir.to_owned(),
        Key::OpenDataFolder,
        ButtonKind::Secondary,
    );
    for (i, (hit, label, desc, button, kind)) in [(0, repo), (3, data)] {
        let y = row_y(i);
        let button = b.t(button);
        let w = b.button_w(&[&button], kind);
        let item = (hit, button, kind, true, w);
        // The glyph, not its larger click area, ends at the content edge.
        let right = if kind == ButtonKind::External {
            b.g.right + (ICON_BUTTON - EXTERNAL_GLYPH) / 2.0
        } else {
            b.g.right
        };
        let x = b.buttons(right, y + ROW_H / 2.0, vec![item]);
        b.row_text(card, y, b.t(label), Some((desc, Tone::Secondary)), false, x);
        if i > 0 {
            b.divider(card, y);
        }
    }

    // Pembaruan: what is known about an update, and the button for the next
    // step (check, install, or nothing while one runs).
    let (desc, button, enabled) = update_row_texts(b, s);
    let y = row_y(1);
    let w = b.button_w(&[&button], ButtonKind::Secondary);
    let item = (Hit::UpdateCheck, button, ButtonKind::Secondary, enabled, w);
    let x = b.buttons(b.g.right, y + ROW_H / 2.0, vec![item]);
    b.row_text(
        card,
        y,
        b.t(Key::Updates),
        Some((desc, Tone::Secondary)),
        false,
        x,
    );
    b.divider(card, y);

    let y = row_y(2);
    let x = b.toggle(Hit::UpdateAuto, b.g.right, y + ROW_H / 2.0, s.update_auto);
    let desc = Some((b.t(Key::UpdateAutoDesc), Tone::Secondary));
    b.row_text(card, y, b.t(Key::UpdateAuto), desc, false, x);
    b.divider(card, y);
}

/// The Pembaruan row's description, button label and whether the button works.
fn update_row_texts(b: &Builder, s: &State) -> (String, String, bool) {
    match &s.update_row {
        UpdateRow::Idle => (
            update_installed(b.lang, s.version),
            b.t(Key::UpdateCheckNow),
            true,
        ),
        UpdateRow::Checking => (b.t(Key::UpdateChecking), b.t(Key::UpdateCheckNow), false),
        UpdateRow::Available(version) => (
            update_available(b.lang, version),
            b.t(Key::UpdateButton),
            true,
        ),
        UpdateRow::Downloading(pct) => (
            update_downloading(b.lang, *pct),
            b.t(Key::UpdateButton),
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row set, with the longest combos the field has to hold.
    fn long_hotkeys() -> Hotkeys {
        let hk = |t: &str| crate::hotkey::parse(t).unwrap();
        Hotkeys {
            search: hk("Win+Ctrl+Alt+Shift+PageDown"),
            clipboard: hk("Win+Ctrl+Alt+Shift+NumMultiply"),
            devices: hk("Ctrl+Alt+F13"),
            arrange: hk("Win+Alt+S"),
            screenshot: hk("Ctrl+PrtSc"),
        }
    }

    fn states() -> Vec<State<'static>> {
        let mut out = Vec::new();
        for lang in [Lang::Id, Lang::En] {
            for (status, dirty, has_id, confirm, built_in, advanced) in [
                (CalStatus::NotConfigured, false, false, false, false, false),
                (CalStatus::NotConfigured, true, true, false, false, false),
                (CalStatus::NeedsLogin, false, true, false, false, false),
                (CalStatus::Reconnect, true, true, false, false, false),
                (CalStatus::Ok, false, true, false, false, false),
                (CalStatus::Ok, false, true, true, false, false),
                (CalStatus::Offline, false, true, false, false, false),
                // A build with Vale's own Google client.
                (CalStatus::NeedsLogin, false, false, false, true, false),
                (CalStatus::Ok, false, false, true, true, false),
                (CalStatus::Reconnect, true, true, false, true, true),
            ] {
                for (hooks, locked, battery) in [
                    (Some((true, true)), false, true),
                    (Some((false, true)), false, true),
                    (Some((false, false)), true, false),
                    (None, true, true),
                ] {
                    let hooks = hooks.map(|(complete, any)| HookState { complete, any });
                    out.push(State {
                        lang,
                        style: Style::Notch,
                        accent: Accent::Ochre,
                        surface: Surface::Graphite,
                        window: WindowTheme::System,
                        notch_width: if locked { NOTCH_W_MAX } else { NOTCH_W_MIN },
                        notch_height: if locked { NOTCH_H_MAX } else { NOTCH_H_MIN },
                        notch_fill: if locked {
                            NotchFill::Transparent
                        } else {
                            NotchFill::Solid
                        },
                        notch_opacity: if locked { 0 } else { 100 },
                        idle_clock: locked,
                        eq_follow_audio: !locked,
                        stay_behind: !locked,
                        robot: !locked,
                        robot_motion: if locked { Motion::Off } else { Motion::Saver },
                        robot_sleep: if locked { 60 } else { 1 },
                        greeting: locked,
                        cat_color: if locked {
                            CatColor::Black
                        } else {
                            CatColor::Calico
                        },
                        cat_accessory: if locked {
                            CatAccessory::Crown
                        } else {
                            CatAccessory::Glasses
                        },
                        cat_follow: !locked,
                        notch_peek: locked,
                        cat_dance: !locked,
                        cat_face: if locked {
                            CatFace::Sleep
                        } else {
                            CatFace::Idle
                        },
                        scroll: 0.0,
                        lyrics: !locked,
                        shell: if locked {
                            Shell::Cmd
                        } else {
                            Shell::PowerShell
                        },
                        cli_start: if locked {
                            CliStart::Last
                        } else {
                            CliStart::Explorer
                        },
                        open_mode: if locked {
                            OpenMode::Click
                        } else {
                            OpenMode::Hover
                        },
                        device_notices: !locked,
                        // Locked: every row set, the longest combos, taken,
                        // one being recorded and one refused. Otherwise the
                        // defaults.
                        hotkeys: if locked {
                            long_hotkeys()
                        } else {
                            Hotkeys::default()
                        },
                        hotkey_failed: [locked; 5],
                        hotkey_recording: locked.then_some(HotkeyId::Devices),
                        hotkey_error: locked.then_some((
                            HotkeyId::Clipboard,
                            RejectMsg::UsedBy(HotkeyId::Screenshot),
                        )),
                        arrange: ArrangeSettings::default(),
                        arrange_edit: None,
                        autostart: true,
                        battery_threshold: 50,
                        has_battery: battery,
                        disk_warn_gb: if locked { 100 } else { 5 },
                        ram_warn_percent: if locked { 98 } else { 70 },
                        mole_protect: !locked,
                        pomodoro: PomoSettings {
                            focus_min: 120,
                            short_break_min: 30,
                            long_break_min: 60,
                            sessions: 12,
                        },
                        pomodoro_locked: locked,
                        google: GoogleState {
                            status,
                            dirty,
                            has_id,
                            confirm,
                            built_in,
                            advanced,
                        },
                        google_note: None,
                        notif: NotifSettings {
                            record: !locked,
                            max_items: if locked { NOTIF_MAX } else { NOTIF_MIN },
                            persist: locked,
                            paused: false,
                            toast_style: if locked {
                                ToastStyle::Full
                            } else {
                                ToastStyle::Compact
                            },
                        },
                        muted: if locked {
                            vec![
                                "Claude".to_owned(),
                                "Orca".to_owned(),
                                "Microsoft Teams (work or school)".to_owned(),
                            ]
                        } else {
                            Vec::new()
                        },
                        notif_confirm: !locked,
                        clip_confirm: !locked,
                        hooks,
                        hook_note: None,
                        clip: ClipboardSettings {
                            enabled: !locked,
                            max_items: if locked { MAX_ITEMS } else { MIN_ITEMS },
                            ..ClipboardSettings::default()
                        },
                        shot: ScreenshotSettings {
                            preview: !locked,
                            on_ignore: if locked {
                                ShotIgnore::Delete
                            } else {
                                ShotIgnore::Keep
                            },
                            show_secs: if locked { 30 } else { 4 },
                            tool: if locked {
                                ShotTool::Windows
                            } else {
                                ShotTool::Vale
                            },
                        },
                        pages: if locked {
                            let mut p = PanelPages::default();
                            p.toggle(Page::Cli);
                            p.move_to(Page::Shortcuts, 0);
                            p
                        } else {
                            PanelPages::default()
                        },
                        tiles: if locked {
                            let mut t = ShortcutTiles::default();
                            t.toggle(7);
                            let own = crate::tiles::CustomTile::new(
                                "Gmail",
                                CustomKind::Url,
                                "https://mail.google.com",
                            );
                            t.add(own.expect("valid"));
                            // Full, and two past the twelfth (added by
                            // hand): both notes show.
                            for i in 0..5 {
                                let own = crate::tiles::CustomTile::new(
                                    &format!("Proyek {i}"),
                                    CustomKind::Folder,
                                    r"D:\Proyek\sebuah folder dengan nama yang panjang",
                                );
                                t.add(own.expect("valid"));
                            }
                            // One own tile that is off: the page is full.
                            let last = t.entries().len() - 1;
                            t.toggle(last);
                            t
                        } else {
                            ShortcutTiles::default()
                        },
                        drag: None,
                        tile_form: None,
                        tile_note: None,
                        version: "2.1.0",
                        update_auto: true,
                        update_row: UpdateRow::Idle,
                        data_dir: r"%APPDATA%\Vale",
                    });
                }
            }
        }
        // Every fixed message beside the buttons, in both languages.
        let notes = [
            Key::Saved,
            Key::CheckBrowser,
            Key::Disconnected,
            Key::HooksOn,
            Key::HooksAlready,
            Key::HooksRemoved,
            Key::HooksNone,
        ];
        let base = out[0].clone();
        for lang in [Lang::Id, Lang::En] {
            for key in notes {
                out.push(State {
                    lang,
                    google_note: Some((text(lang, key), Tone::Good)),
                    hook_note: Some((text(lang, key), Tone::Good)),
                    ..base.clone()
                });
            }
        }
        // The own-shortcut form open on each kind with each message, the
        // note after a tile added off, and a row in hand on each list: with
        // the default lists and with the long, full ones.
        for base in [out[0].clone(), out[2].clone()] {
            for lang in [Lang::Id, Lang::En] {
                for kind in CustomKind::ALL {
                    for error in [
                        None,
                        Some(Key::TileErrName),
                        Some(Key::TileErrPath),
                        Some(Key::TileErrUrl),
                        Some(Key::TileOwnFull),
                    ] {
                        out.push(State {
                            lang,
                            tile_form: Some(TileForm { kind, error }),
                            ..base.clone()
                        });
                    }
                }
                out.push(State {
                    lang,
                    tile_note: Some(Key::TileAddedOff),
                    ..base.clone()
                });
                for list in [List::Pages, List::Tiles] {
                    out.push(State {
                        lang,
                        drag: Some(held_row(
                            &State {
                                lang,
                                ..base.clone()
                            },
                            list,
                            1,
                            3,
                        )),
                        ..base.clone()
                    });
                }
            }
        }
        // The Tombol pintas rows: each refusal under each row, and each row
        // being recorded.
        for base in [out[0].clone(), out[2].clone()] {
            for lang in [Lang::Id, Lang::En] {
                for id in HotkeyId::ALL {
                    for msg in [
                        RejectMsg::Reserved,
                        RejectMsg::NeedsModifier,
                        RejectMsg::UsedBy(HotkeyId::Clipboard),
                        RejectMsg::UsedBy(HotkeyId::Screenshot),
                    ] {
                        out.push(State {
                            lang,
                            hotkey_recording: None,
                            hotkey_error: Some((id, msg)),
                            ..base.clone()
                        });
                    }
                    out.push(State {
                        lang,
                        hotkey_recording: Some(id),
                        hotkey_error: None,
                        ..base.clone()
                    });
                }
            }
        }
        out
    }

    /// A drag of the row at place `from` of `list`, with the pointer a little
    /// into the place `to` (the layout's own rows say where they are).
    fn held_row(s: &State, list: List, from: usize, to: usize) -> DragView {
        let l = layout(Section::Panel, s, &Estimate);
        let tops: Vec<f32> = l
            .rows
            .iter()
            .filter(|r| r.list == list)
            .map(|r| r.rect.y)
            .collect();
        let top = tops[to] + 8.0;
        assert_eq!(drop_index(top + ROW_H / 2.0, &tops), to);
        DragView {
            list,
            from,
            to,
            top,
        }
    }

    /// Every section and state, each scrolled to the top and to the bottom.
    fn all_layouts() -> Vec<(Section, Lang, Layout)> {
        let mut out = Vec::new();
        for s in states() {
            for section in Section::ALL {
                for scroll in [0.0, f32::MAX] {
                    let s = State {
                        scroll,
                        ..s.clone()
                    };
                    out.push((section, s.lang, layout(section, &s, &Estimate)));
                }
            }
        }
        out
    }

    /// The window, or for a scrolled section its whole content as if the
    /// window were tall enough (at this scroll).
    fn canvas(l: &Layout) -> Rect {
        Rect {
            y: -l.scroll,
            h: CLIENT_H + l.scroll_max,
            ..CLIENT
        }
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x - 0.01
            && inner.y >= outer.y - 0.01
            && inner.right() <= outer.right() + 0.01
            && inner.bottom() <= outer.bottom() + 0.01
    }

    const CLIENT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: CLIENT_W,
        h: CLIENT_H,
    };

    #[test]
    fn hit_regions_never_overlap() {
        for (section, lang, l) in all_layouts() {
            let hits = l.hits();
            for (i, (ha, a)) in hits.iter().enumerate() {
                for (hb, b) in &hits[i + 1..] {
                    assert!(
                        !overlaps(*a, *b),
                        "{section:?} {lang:?}: {ha:?} overlaps {hb:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn controls_and_fields_sit_inside_their_cards() {
        let content = Rect {
            x: CONTENT_X,
            y: 0.0,
            w: CONTENT_W + LINK_PAD,
            h: CLIENT_H,
        };
        for (section, lang, l) in all_layouts() {
            for c in &l.controls {
                let bounds = c.bounds();
                // The setup guide link sits on the line above its card.
                if let Control::Button(b) = c
                    && matches!(b.kind, ButtonKind::Link | ButtonKind::Disclosure(_))
                {
                    assert!(inside(bounds, content), "{section:?} {lang:?}: {c:?}");
                    assert!(l.cards.iter().all(|card| !overlaps(bounds, *card)));
                    continue;
                }
                assert!(
                    l.cards.iter().any(|card| inside(bounds, *card)),
                    "{section:?} {lang:?}: {c:?} outside every card"
                );
            }
            for chip in &l.chips {
                assert!(
                    l.cards.iter().any(|card| inside(chip.rect, *card)),
                    "{section:?} {lang:?}: {chip:?}"
                );
                assert!(inside(chip.dot, chip.rect) && inside(chip.label.rect, chip.rect));
            }
            for (f, r) in &l.fields {
                assert!(
                    l.cards.iter().any(|card| inside(*r, *card)),
                    "{section:?} {lang:?}: {f:?}"
                );
            }
            for g in &l.glyphs {
                assert!(
                    l.cards.iter().any(|card| inside(g.rect, *card)),
                    "{section:?} {lang:?}: {g:?} outside every card"
                );
            }
            for card in &l.cards {
                assert!(inside(*card, canvas(&l)), "{section:?}: card {card:?}");
                assert!(card.x >= SIDEBAR_W + PAD - 0.01);
            }
            for n in &l.nav {
                assert!(n.pill.right() <= SIDEBAR_W, "{n:?}");
            }
        }
    }

    #[test]
    fn controls_do_not_overlap_each_other_or_texts() {
        for (section, lang, l) in all_layouts() {
            let mut boxes: Vec<(String, Rect)> = l
                .controls
                .iter()
                .map(|c| (format!("{c:?}"), c.bounds()))
                .collect();
            boxes.extend(l.fields.iter().map(|(f, r)| (format!("{f:?}"), *r)));
            boxes.extend(l.chips.iter().map(|c| (format!("{c:?}"), c.rect)));
            boxes.extend(l.glyphs.iter().map(|g| (format!("{g:?}"), g.rect)));
            for (i, (na, a)) in boxes.iter().enumerate() {
                for (nb, b) in &boxes[i + 1..] {
                    assert!(!overlaps(*a, *b), "{section:?} {lang:?}: {na} / {nb}");
                }
                for t in &l.texts {
                    assert!(
                        !overlaps(*a, t.rect),
                        "{section:?} {lang:?}: {na} covers {:?}",
                        t.text
                    );
                }
            }
        }
    }

    /// Every text fits its box with the generous estimate, in both languages.
    #[test]
    fn texts_fit_in_both_languages() {
        let m = Estimate;
        for (section, lang, l) in all_layouts() {
            let mut texts: Vec<&Text> = l.texts.iter().collect();
            texts.extend(l.title.iter());
            texts.extend(l.brand.iter());
            texts.extend(l.nav.iter().map(|n| &n.label));
            texts.extend(l.chips.iter().map(|c| &c.label));
            for c in &l.controls {
                if let Control::Stepper { value, .. } = c {
                    texts.push(value);
                }
            }
            for t in texts {
                let w = m.width(&t.text, t.font);
                assert!(
                    w <= t.rect.w + 0.01,
                    "{section:?} {lang:?}: {:?} needs {w:.0} px, has {:.0}",
                    t.text,
                    t.rect.w
                );
                assert!(t.rect.h >= t.font.size(), "{:?} too short", t.text);
                assert!(inside(t.rect, canvas(&l)), "{:?}", t.text);
            }
            for c in &l.controls {
                let labels: Vec<(&str, Rect, f32)> = match c {
                    Control::Segmented { items, .. } => items
                        .iter()
                        .map(|s| (s.label.as_str(), s.rect, 0.0))
                        .collect(),
                    Control::Button(b)
                        if matches!(b.kind, ButtonKind::Link | ButtonKind::Disclosure(_)) =>
                    {
                        vec![(b.label.as_str(), b.rect, LINK_GLYPH_W)]
                    }
                    // The label is not drawn.
                    Control::Button(b) if b.kind == ButtonKind::External => Vec::new(),
                    Control::Button(b) => vec![(b.label.as_str(), b.rect, 0.0)],
                    _ => Vec::new(),
                };
                for (label, rect, extra) in labels {
                    let w = m.width(label, Font::Control) + extra;
                    assert!(w <= rect.w, "{section:?} {lang:?}: {label:?}");
                }
            }
            // The accent description names any swatch under the pointer.
            if let Some(i) = l.accent_desc {
                for a in Accent::ALL {
                    let name = text(lang, accent_key(*a));
                    assert!(m.width(name, Font::Desc) <= l.texts[i].rect.w);
                }
            }
        }
    }

    /// The sections that overflow the window and so scroll, whatever the
    /// state: Tampilan (three cards and the preview), Kucing, Panel (the two
    /// lists), Clipboard (history and screenshot) and Fitur (five cards).
    const SCROLLING: [Section; 5] = [
        Section::Appearance,
        Section::Cat,
        Section::Panel,
        Section::Clipboard,
        Section::Features,
    ];

    /// Every section fits the window's height. `SCROLLING` ones scroll by
    /// exactly what they lack and then the last card meets the bottom margin;
    /// the rest (Umum, Tombol pintas, Integrasi, Tentang) never scroll.
    #[test]
    fn everything_fits_in_the_window_height() {
        for (section, lang, l) in all_layouts() {
            let bottom = l
                .cards
                .iter()
                .map(Rect::bottom)
                .chain(l.texts.iter().map(|t| t.rect.bottom()))
                .fold(0.0, f32::max);
            let unscrolled = bottom + l.scroll;
            assert!(
                unscrolled - l.scroll_max <= CLIENT_H - PAD + 0.01,
                "{section:?} {lang:?}: {bottom}"
            );
            if SCROLLING.contains(&section) {
                assert!(l.scroll_max > 0.0 && l.viewport.is_some(), "{section:?}");
                // Scrolled to the end, the last card ends at the margin.
                if l.scroll == l.scroll_max {
                    assert!(
                        (bottom - (CLIENT_H - PAD)).abs() < 0.01,
                        "{section:?} {lang:?}: {bottom}"
                    );
                }
            } else {
                assert_eq!((l.scroll_max, l.viewport), (0.0, None), "{section:?}");
            }
        }
    }

    #[test]
    fn exactly_the_overflowing_sections_get_a_viewport() {
        let base = states()[0].clone();
        for lang in [Lang::Id, Lang::En] {
            let state = State {
                lang,
                ..base.clone()
            };
            let scrolling: Vec<Section> = Section::ALL
                .into_iter()
                .filter(|s| layout(*s, &state, &Estimate).viewport.is_some())
                .collect();
            assert_eq!(scrolling, SCROLLING, "{lang:?}");
            // Umum, Tombol pintas, Integrasi and Tentang never scroll, whatever
            // the state (Google, a recording, a refused combo, taken hotkeys).
            for s in states() {
                for section in [
                    Section::General,
                    Section::Hotkeys,
                    Section::Integrations,
                    Section::About,
                ] {
                    let l = layout(section, &s, &Estimate);
                    assert!(l.viewport.is_none(), "{section:?}");
                }
            }
        }
    }

    /// The layouts of every section and state at `client`, each scrolled to
    /// the top and to the end.
    fn layouts_in(client: (f32, f32)) -> Vec<(Section, Lang, Layout)> {
        let mut out = Vec::new();
        for s in states() {
            for section in Section::ALL {
                for scroll in [0.0, f32::MAX] {
                    let s = State {
                        scroll,
                        ..s.clone()
                    };
                    out.push((section, s.lang, layout_in(section, &s, &Estimate, client)));
                }
            }
        }
        out
    }

    #[test]
    fn layout_in_at_the_minimum_size_is_the_layout() {
        for s in states() {
            for section in Section::ALL {
                for scroll in [0.0, 100.0, f32::MAX] {
                    let s = State {
                        scroll,
                        ..s.clone()
                    };
                    let want = layout(section, &s, &Estimate);
                    assert_eq!(
                        layout_in(section, &s, &Estimate, (CLIENT_W, CLIENT_H)),
                        want,
                        "{section:?}"
                    );
                    // Smaller than the minimum is the minimum.
                    assert_eq!(layout_in(section, &s, &Estimate, (10.0, 10.0)), want);
                }
            }
        }
    }

    #[test]
    fn the_column_at_the_minimum_size_is_where_it_always_was() {
        let g = Geom::new((CLIENT_W, CLIENT_H));
        assert_eq!(
            (g.content_x, g.content_w, g.right),
            (208.0, 488.0, CONTENT_X + CONTENT_W - INSET)
        );
        assert_eq!((CONTENT_X, CONTENT_W), (208.0, 488.0));
    }

    #[test]
    fn a_wide_window_centres_a_column_capped_at_760() {
        let client = (1400.0, 900.0);
        let g = Geom::new(client);
        let want_x = SIDEBAR_W + PAD + ((1400.0 - SIDEBAR_W - 2.0 * PAD - 760.0) / 2.0).round();
        assert_eq!((g.content_x, g.content_w), (want_x, 760.0));
        // The sidebar depends on the language only.
        let min_of = |lang: Lang| {
            let s = states().into_iter().find(|s| s.lang == lang).unwrap();
            layout(Section::General, &s, &Estimate)
        };
        let min = [min_of(Lang::Id), min_of(Lang::En)];
        for (section, lang, l) in layouts_in(client) {
            let min = &min[usize::from(lang == Lang::En)];
            for card in &l.cards {
                assert_eq!((card.x, card.w), (g.content_x, 760.0), "{section:?}");
            }
            let lo = g.content_x - 0.01;
            let hi = g.content_x + 760.0 + 0.01;
            let within = |r: Rect, what: &str| {
                assert!(
                    r.x >= lo && r.right() <= hi,
                    "{section:?} {lang:?}: {what} {r:?} leaves the column"
                );
            };
            for c in &l.controls {
                let bounds = c.bounds();
                // The setup guide link sits just past the column's edge.
                let link = matches!(
                    c,
                    Control::Button(b) if matches!(b.kind, ButtonKind::Link | ButtonKind::Disclosure(_))
                );
                if link {
                    assert!(bounds.x >= lo && bounds.right() <= hi + LINK_PAD, "{c:?}");
                } else {
                    within(bounds, &format!("{c:?}"));
                }
            }
            for t in &l.texts {
                within(t.rect, &t.text);
            }
            for (f, r) in &l.fields {
                within(*r, &format!("{f:?}"));
            }
            for chip in &l.chips {
                within(chip.rect, "chip");
            }
            for g in &l.glyphs {
                within(g.rect, &format!("{g:?}"));
            }
            for r in l.preview.iter().chain(&l.cat_preview).chain(&l.about_logo) {
                within(*r, "picture");
            }
            // The sidebar does not move; the title follows the column.
            assert_eq!(l.logo, min.logo);
            assert_eq!(l.brand, min.brand);
            assert_eq!(l.nav, min.nav);
            let title = l.title.as_ref().expect("a title");
            assert_eq!((title.rect.x, title.rect.w), (g.content_x, 760.0));
        }
    }

    #[test]
    fn a_window_a_little_wider_has_an_uncapped_column_from_the_same_start() {
        let client = (900.0, 520.0);
        for (section, _, l) in layouts_in(client) {
            for card in &l.cards {
                assert_eq!(
                    (card.x, card.w),
                    (208.0, 900.0 - 184.0 - 48.0),
                    "{section:?}"
                );
            }
            let title = l.title.as_ref().expect("a title");
            assert_eq!((title.rect.x, title.rect.w), (208.0, 668.0));
        }
        // Controls end at the card's inner edge.
        let g = Geom::new(client);
        assert_eq!(g.right, 208.0 + 668.0 - INSET);
    }

    #[test]
    fn a_tall_window_needs_no_viewport() {
        for (section, lang, l) in layouts_in((720.0, 3000.0)) {
            assert_eq!(
                (l.scroll_max, l.scroll, l.viewport),
                (0.0, 0.0, None),
                "{section:?} {lang:?}"
            );
            assert_eq!(l.scrollbar(), None);
        }
    }

    #[test]
    fn a_window_between_scrolls_by_what_it_lacks() {
        let client = (720.0, 700.0);
        let mut scrolled = 0;
        for (section, lang, l) in layouts_in(client) {
            let bottom = l
                .cards
                .iter()
                .map(Rect::bottom)
                .chain(l.texts.iter().map(|t| t.rect.bottom()))
                .fold(0.0, f32::max);
            assert!(
                bottom + l.scroll - l.scroll_max <= client.1 - PAD + 0.01,
                "{section:?} {lang:?}"
            );
            let Some(v) = l.viewport else {
                assert_eq!(l.scroll_max, 0.0, "{section:?}");
                continue;
            };
            scrolled += 1;
            assert_eq!(
                (v.x, v.y, v.w, v.h),
                (
                    SIDEBAR_W,
                    VIEW_TOP,
                    client.0 - SIDEBAR_W,
                    client.1 - VIEW_TOP
                )
            );
            assert!(l.scroll_max > 0.0);
            if l.scroll == l.scroll_max {
                assert!(
                    (bottom - (client.1 - PAD)).abs() < 0.01,
                    "{section:?} {lang:?}: {bottom}"
                );
            }
        }
        assert!(scrolled > 0, "some section still scrolls at 700");
    }

    #[test]
    fn a_taller_window_keeps_the_scroll_in_its_smaller_range() {
        let state = |scroll| State {
            scroll,
            ..states()[0].clone()
        };
        let at = |client: (f32, f32), scroll| {
            layout_in(Section::Features, &state(scroll), &Estimate, client)
        };
        let small = at((CLIENT_W, CLIENT_H), 100_000.0);
        let taller = at((CLIENT_W, 800.0), 100_000.0);
        assert!(small.scroll_max > taller.scroll_max && taller.scroll_max > 0.0);
        assert_eq!(small.scroll, small.scroll_max);
        assert_eq!(taller.scroll, taller.scroll_max);
        // The same distance from the end: 280 px more window, 280 less scroll.
        assert!((small.scroll_max - taller.scroll_max - 280.0).abs() < 0.01);
        assert_eq!(at((CLIENT_W, 5000.0), 100_000.0).scroll, 0.0);
    }

    #[test]
    fn the_scrollbar_sits_at_the_windows_right_edge() {
        for client in [(CLIENT_W, CLIENT_H), (1400.0, 520.0), (1000.0, 600.0)] {
            let s = State {
                scroll: 100.0,
                ..states()[0].clone()
            };
            let l = layout_in(Section::Appearance, &s, &Estimate, client);
            let bar = l.scrollbar().expect("Tampilan scrolls");
            assert_eq!(bar.right() + 5.0, client.0, "{client:?}");
            let v = l.viewport.expect("a viewport");
            assert!(bar.y >= v.y && bar.bottom() <= v.bottom());
        }
    }

    #[test]
    fn hits_stay_clipped_to_the_viewport_in_a_wide_window() {
        for (section, _, l) in layouts_in((1400.0, 600.0)) {
            let Some(v) = l.viewport else {
                continue;
            };
            for (hit, r) in l.hits() {
                if matches!(hit, Hit::Nav(_)) {
                    continue;
                }
                assert!(inside(r, v), "{section:?}: {hit:?} {r:?} leaves {v:?}");
            }
        }
    }

    #[test]
    fn appearance_scrolls_and_clicks_follow() {
        let base = states()[0].clone();
        let window_hits = |l: &Layout| -> Vec<Hit> {
            l.hits()
                .into_iter()
                .map(|(h, _)| h)
                .filter(|h| matches!(h, Hit::WindowMode(_)))
                .collect()
        };
        let top = layout(Section::Appearance, &base, &Estimate);
        let view = top.viewport.expect("viewport");
        assert_eq!(top.scroll, 0.0);
        // At the top the Island card is in view and Jendela is far below:
        // nothing under the window takes a click.
        assert_eq!(window_hits(&top), []);
        let peeking: Vec<Hit> = top
            .hits()
            .into_iter()
            .map(|(h, _)| h)
            .filter(|h| matches!(h, Hit::Slider(_) | Hit::NotchReset | Hit::NotchFill(_)))
            .collect();
        // The Island card's fifth row (the equalizer toggle) now ends the
        // visible part; the Notch card starts below the window.
        assert_eq!(peeking, []);
        assert!(top.hits().iter().any(|(h, _)| *h == Hit::EqFollowAudio));
        for (_, r) in top.hits() {
            assert!(r.bottom() <= CLIENT_H + 0.01, "{r:?}");
        }
        // Scrolled to the end: Jendela's three modes, where they are drawn,
        // and the Island card's rows went up under the title: clipped.
        let end = State {
            scroll: 1e6,
            ..base.clone()
        };
        let l = layout(Section::Appearance, &end, &Estimate);
        assert_eq!(l.scroll, l.scroll_max);
        assert_eq!(window_hits(&l).len(), 3);
        let style_hits: Vec<Rect> = l
            .hits()
            .into_iter()
            .filter(|(h, _)| matches!(h, Hit::Style(_)))
            .map(|(_, r)| r)
            .collect();
        assert!(style_hits.iter().all(|r| r.y >= view.y));
        // The wheel: a notch is a row, kept in range.
        assert_eq!(scroll_by(0.0, -120, 100.0), ROW_H);
        assert_eq!(scroll_by(0.0, 120, 100.0), 0.0);
        assert_eq!(scroll_by(90.0, -240, 100.0), 100.0);
        assert!(l.scrollbar().is_some() && top.scrollbar().is_some());
        // The sections that fit have no scrollbar.
        for section in [Section::Integrations, Section::About] {
            assert!(layout(section, &base, &Estimate).scrollbar().is_none());
        }
    }

    #[test]
    fn the_island_card_has_the_base_color_swatches_and_the_cards_are_titled() {
        for lang in [Lang::Id, Lang::En] {
            for surface in Surface::ALL.iter().copied() {
                let s = State {
                    lang,
                    surface,
                    cat_color: CatColor::Calico,
                    robot: true,
                    ..states()[0].clone()
                };
                let l = layout(Section::Appearance, &s, &Estimate);
                let (items, selected) = l
                    .controls
                    .iter()
                    .find_map(|c| match c {
                        Control::SurfaceSwatches {
                            items, selected, ..
                        } => Some((items.clone(), *selected)),
                        _ => None,
                    })
                    .expect("the base color swatches");
                // Ikuti kucing first, then the six fills.
                let order: Vec<Surface> = items.iter().map(|(v, _)| *v).collect();
                assert_eq!(
                    order,
                    [
                        Surface::Auto,
                        Surface::Espresso,
                        Surface::Graphite,
                        Surface::Sea,
                        Surface::Night,
                        Surface::Plum,
                        Surface::Terminal
                    ]
                );
                assert_eq!(selected, surface);
                // Each takes a click as its own choice; the chosen name is
                // the row's description (Auto says what it gives).
                for (v, r) in &items {
                    assert_eq!(
                        l.hit(r.x + r.w / 2.0, r.y + r.h / 2.0),
                        Some(Hit::Surface(*v)),
                        "{v:?}"
                    );
                }
                let name = surface_label(lang, surface, CatColor::Calico, true);
                assert!(l.texts.iter().any(|t| t.text == name), "{name}");
                // Seven swatches plus the hit slop leave "Warna dasar" and its
                // description room to the left, at the 720x520 minimum.
                assert_eq!(items.len(), 7);
                let first = items.iter().map(|(_, r)| r.x).fold(f32::MAX, f32::min);
                let last = items.iter().map(|(_, r)| r.right()).fold(0.0, f32::max);
                let card = l.cards[1];
                assert!(last + SWATCH_SLOP <= card.right(), "{lang:?} {surface:?}");
                for want in [text(lang, Key::IslandSurface), name.as_str()] {
                    let t = l.texts.iter().find(|t| t.text == want).expect("text");
                    assert!(t.rect.w > 0.0, "{want}");
                    assert!(
                        t.rect.right() <= first - SWATCH_SLOP + 0.01,
                        "{lang:?} {want}: {} vs {}",
                        t.rect.right(),
                        first - SWATCH_SLOP
                    );
                }
                if surface == Surface::Auto {
                    assert_eq!(
                        name,
                        format!(
                            "{} · {}",
                            text(lang, Key::SurfaceAuto),
                            text(lang, Key::SurfaceSea)
                        )
                    );
                }
                // The old segmented picker and the old label are gone.
                assert!(
                    l.texts
                        .iter()
                        .all(|t| t.text != text(lang, Key::IslandSurfaceDesc))
                );
                // Three titled cards under the preview: Island, Notch, Jendela.
                for key in [Key::GroupIsland, Key::GroupNotch, Key::GroupWindow] {
                    assert!(
                        l.texts
                            .iter()
                            .any(|t| t.text == text(lang, key).to_uppercase()),
                        "{lang:?} {key:?}"
                    );
                }
                assert_eq!(l.cards.len(), 4, "preview + 3 cards");
            }
        }
        // Names and the cat colours' keys differ by language.
        assert_eq!(
            surface_label(Lang::Id, Surface::Sea, CatColor::Terracotta, true),
            "Laut"
        );
        assert_eq!(
            surface_label(Lang::En, Surface::Auto, CatColor::Black, false),
            "Match cat · Espresso"
        );
    }

    #[test]
    fn umum_has_basics_and_alerts() {
        for lang in [Lang::Id, Lang::En] {
            let base = State {
                lang,
                disk_warn_gb: 10,
                ram_warn_percent: 90,
                has_battery: true,
                ..states()[0].clone()
            };
            let l = layout(Section::General, &base, &Estimate);
            // "Dasar" and "Peringatan", each under its title.
            assert_eq!(l.cards.len(), 2);
            // Two rows, language and autostart: the search hotkey moved to
            // Tombol pintas, so the card never grows.
            assert_eq!(l.cards[0].h, 2.0 * ROW_H);
            assert_eq!(l.cards[1].h, 4.0 * ROW_H);
            for key in [Key::GroupBasics, Key::GroupAlerts] {
                assert!(
                    l.texts
                        .iter()
                        .any(|t| t.text == text(lang, key).to_uppercase()),
                    "{lang:?} {key:?}"
                );
            }
            // Nothing else moved in: Mode buka, stay-behind, lyrics, CLI,
            // devices and the Mole row live in Panel and Fitur now.
            let hits: Vec<Hit> = l.hits().into_iter().map(|(h, _)| h).collect();
            assert!(!hits.iter().any(|h| matches!(
                h,
                Hit::OpenMode(_)
                    | Hit::StayBehind
                    | Hit::Lyrics
                    | Hit::CliStart(_)
                    | Hit::Shell(_)
                    | Hit::DeviceNotices
                    | Hit::MoleProtect
            )));
            // Dasar: language and autostart, in that order, and no hotkey.
            let order: Vec<Hit> = l
                .controls
                .iter()
                .filter_map(|c| match c {
                    Control::Segmented { items, .. } => items.first().map(|i| i.hit),
                    Control::Toggle { hit, .. } => Some(*hit),
                    _ => None,
                })
                .collect();
            assert_eq!(order, [Hit::Lang(Lang::Id), Hit::Autostart]);
            assert!(
                !hits
                    .iter()
                    .any(|h| matches!(h, Hit::HotkeyField(_) | Hit::HotkeyReset(_)))
            );
            // Peringatan: the three steppers (battery, disk, RAM) and the help button.
            let steppers: Vec<Hit> = l
                .controls
                .iter()
                .filter_map(|c| match c {
                    Control::Stepper { minus, plus, .. } => minus.1.or(plus.1),
                    _ => None,
                })
                .collect();
            assert_eq!(
                steppers,
                [Hit::Battery(-1), Hit::DiskWarn(-1), Hit::RamWarn(-1)]
            );
            assert!(hits.contains(&Hit::DndHelp));
            for key in [
                Key::Language,
                Key::Autostart,
                Key::BatteryAlert,
                Key::DiskAlert,
                Key::RamAlert,
                Key::PopupsHelp,
                Key::ShowHow,
            ] {
                let want = text(lang, key);
                let shown = l.texts.iter().any(|t| t.text == want)
                    || l.controls
                        .iter()
                        .any(|c| matches!(c, Control::Button(b) if b.label == want));
                assert!(shown, "{lang:?} {key:?}");
            }
        }
    }

    #[test]
    fn the_battery_stepper_is_off_on_a_pc_without_one() {
        for lang in [Lang::Id, Lang::En] {
            let battery_hits = |has_battery: bool, threshold: u8| -> Vec<Hit> {
                let s = State {
                    lang,
                    has_battery,
                    battery_threshold: threshold,
                    ..states()[0].clone()
                };
                layout(Section::General, &s, &Estimate)
                    .hits()
                    .into_iter()
                    .map(|(h, _)| h)
                    .filter(|h| matches!(h, Hit::Battery(_)))
                    .collect()
            };
            assert_eq!(battery_hits(true, 20), [Hit::Battery(-1), Hit::Battery(1)]);
            assert_eq!(battery_hits(true, 50), [Hit::Battery(-1)]);
            assert_eq!(battery_hits(false, 20), [], "no battery: no clicks");
            let s = State {
                lang,
                has_battery: false,
                ..states()[0].clone()
            };
            let l = layout(Section::General, &s, &Estimate);
            assert!(l.texts.iter().any(|t| t.text == text(lang, Key::NoBattery)));
        }
    }

    #[test]
    fn clipboard_scrolls_to_the_screenshot_card() {
        let shot_hits = |l: &Layout| -> Vec<Hit> {
            l.hits()
                .into_iter()
                .map(|(h, _)| h)
                .filter(|h| matches!(h, Hit::ShotPreview | Hit::ShotIgnore(_) | Hit::ShotSecs(_)))
                .collect()
        };
        for lang in [Lang::Id, Lang::En] {
            let base = State {
                lang,
                shot: ScreenshotSettings::default(),
                ..states()[0].clone()
            };
            let top = layout(Section::Clipboard, &base, &Estimate);
            assert!(top.viewport.is_some() && top.scrollbar().is_some());
            assert_eq!(top.cards.len(), 2, "the screenshot has its own card");
            for (_, r) in top.hits() {
                assert!(r.bottom() <= CLIENT_H + 0.01, "{r:?}");
            }
            let end = State {
                scroll: 1e6,
                ..base.clone()
            };
            let l = layout(Section::Clipboard, &end, &Estimate);
            assert_eq!(l.scroll, l.scroll_max);
            assert!((l.cards[1].bottom() - (CLIENT_H - PAD)).abs() < 0.01);
            assert_eq!(
                shot_hits(&l),
                [
                    Hit::ShotPreview,
                    Hit::ShotIgnore(ShotIgnore::Keep),
                    Hit::ShotIgnore(ShotIgnore::Delete),
                    Hit::ShotSecs(-1),
                    Hit::ShotSecs(1),
                ]
            );
            // The defaults: Simpan chosen, 8 s.
            let chosen = l.controls.iter().find_map(|c| match c {
                Control::Segmented {
                    group, selected, ..
                } if *group == Group::ShotIgnore => Some(*selected),
                _ => None,
            });
            assert_eq!(chosen, Some(0), "Simpan");
            let secs = l.controls.iter().find_map(|c| match c {
                Control::Stepper { minus, value, .. }
                    if matches!(minus.1, Some(Hit::ShotSecs(_))) =>
                {
                    Some(value.text.clone())
                }
                _ => None,
            });
            assert_eq!(secs, Some(crate::i18n::seconds_short(lang, 8)));
            for key in [
                Key::ShotPreviewSetting,
                Key::ShotIgnoreSetting,
                Key::ShotIgnoreDesc,
                Key::ShotShowFor,
            ] {
                assert!(
                    l.texts.iter().any(|t| t.text == text(lang, key)),
                    "{lang:?} {key:?}"
                );
            }
            assert!(
                l.texts
                    .iter()
                    .any(|t| t.text == text(lang, Key::SecScreenshot).to_uppercase())
            );
            // The preview off: only its toggle takes clicks.
            let off = State {
                shot: ScreenshotSettings {
                    preview: false,
                    ..ScreenshotSettings::default()
                },
                ..end
            };
            let l = layout(Section::Clipboard, &off, &Estimate);
            assert_eq!(shot_hits(&l), [Hit::ShotPreview]);
        }
    }

    #[test]
    fn the_clipboard_cards_lost_their_hotkey_and_prtsc_rows() {
        for lang in [Lang::Id, Lang::En] {
            let s = State {
                lang,
                scroll: 1e6,
                ..states()[0].clone()
            };
            let l = layout(Section::Clipboard, &s, &Estimate);
            // Riwayat: seven rows (the hotkey row is gone); Screenshot: tool,
            // preview, on-ignore, seconds (the PrtSc row is gone).
            assert_eq!(l.cards.len(), 2);
            assert_eq!(l.cards[0].h, 7.0 * ROW_H);
            assert_eq!(l.cards[1].h, 4.0 * ROW_H);
            let hits: Vec<Hit> = l.hits().into_iter().map(|(h, _)| h).collect();
            assert!(
                !hits
                    .iter()
                    .any(|h| matches!(h, Hit::HotkeyField(_) | Hit::HotkeyReset(_)))
            );
            let order: Vec<Hit> = l
                .controls
                .iter()
                .filter_map(|c| match c {
                    Control::Toggle { hit, .. } => Some(*hit),
                    Control::Stepper { minus, plus, .. } => minus.1.or(plus.1),
                    Control::Button(b) => Some(b.hit),
                    Control::Segmented { items, .. } => items.first().map(|i| i.hit),
                    _ => None,
                })
                .collect();
            // The rest keep their order: history rows, then the screenshot's.
            assert_eq!(
                order,
                [
                    Hit::ClipEnabled,
                    Hit::ClipItems(1),
                    Hit::ClipPersist,
                    Hit::ClipClearOnLock,
                    Hit::ClipNotice,
                    Hit::ClipFeedbackAll,
                    Hit::ClipClear,
                    Hit::ShotTool(ShotTool::Vale),
                    Hit::ShotPreview,
                    Hit::ShotIgnore(ShotIgnore::Keep),
                    Hit::ShotSecs(1),
                ],
                "{lang:?}"
            );
            // Every row of both cards is one ROW_H apart (no row grew).
            assert!((l.cards[1].bottom() - (CLIENT_H - PAD)).abs() < 0.01);
        }
    }

    /// The Tombol pintas fields of a layout, in the order they are drawn.
    fn hotkey_fields(l: &Layout) -> Vec<(Hit, Rect, Vec<String>, bool)> {
        l.controls
            .iter()
            .filter_map(|c| match c {
                Control::HotkeyField {
                    hit,
                    rect,
                    caps,
                    recording,
                } => Some((*hit, *rect, caps.clone(), *recording)),
                _ => None,
            })
            .collect()
    }

    fn hotkey_base(lang: Lang) -> State<'static> {
        State {
            lang,
            hotkeys: Hotkeys::default(),
            hotkey_failed: [false; 5],
            hotkey_recording: None,
            hotkey_error: None,
            scroll: 0.0,
            ..states()[0].clone()
        }
    }

    #[test]
    fn tombol_pintas_sits_after_clipboard_with_a_header_and_five_rows() {
        assert_eq!(
            Section::ALL,
            [
                Section::General,
                Section::Appearance,
                Section::Cat,
                Section::Panel,
                Section::Clipboard,
                Section::Hotkeys,
                Section::Features,
                Section::Integrations,
                Section::About,
            ]
        );
        assert_eq!(Section::Hotkeys.icon(), Icon::Keyboard);
        for lang in [Lang::Id, Lang::En] {
            let s = hotkey_base(lang);
            let l = layout(Section::Hotkeys, &s, &Estimate);
            // The section needs no viewport at the minimum size.
            assert!(l.viewport.is_none() && l.scroll_max == 0.0, "{lang:?}");
            // One card of the header and five rows, under its eyebrow.
            assert_eq!(l.cards.len(), 1);
            let card = l.cards[0];
            assert_eq!(card.h, HEADER_H + 5.0 * ROW_H);
            assert!(
                l.texts
                    .iter()
                    .any(|t| t.text == text(lang, Key::GroupHotkeys).to_uppercase()),
                "{lang:?}"
            );
            assert_eq!(l.title.as_ref().unwrap().text, text(lang, Key::SecHotkeys));
            // The hint on the left, "Kembalikan semua" on the right, in the
            // header.
            let header_bottom = card.y + HEADER_H;
            let hint = l
                .texts
                .iter()
                .find(|t| t.text == text(lang, Key::HotkeysHint))
                .expect("the hint");
            assert!(hint.rect.bottom() <= header_bottom && hint.rect.x < card.x + 40.0);
            let reset_all = l
                .controls
                .iter()
                .find_map(|c| match c {
                    Control::Button(b) if b.hit == Hit::HotkeysResetAll => Some(b),
                    _ => None,
                })
                .expect("Kembalikan semua");
            assert_eq!(reset_all.label, text(lang, Key::HotkeysResetAll));
            assert_eq!(reset_all.kind, ButtonKind::Secondary);
            assert!(reset_all.rect.bottom() <= header_bottom);
            assert!(reset_all.rect.x > hint.rect.x, "on the right");
            // All at their defaults: nothing to reset.
            assert!(!reset_all.enabled);
            let fields = hotkey_fields(&l);
            let ids: Vec<Hit> = fields.iter().map(|f| f.0).collect();
            assert_eq!(
                ids,
                HotkeyId::ALL.map(Hit::HotkeyField).to_vec(),
                "the rows are in ALL order"
            );
            for (i, (_, rect, caps, recording)) in fields.iter().enumerate() {
                let row_top = header_bottom + i as f32 * ROW_H;
                assert!(rect.y >= row_top && rect.bottom() <= row_top + ROW_H);
                assert!(!recording);
                assert!((HOTKEY_FIELD_MIN_W..=HOTKEY_FIELD_MAX_W).contains(&rect.w));
                let id = HotkeyId::ALL[i];
                assert_eq!(
                    *caps,
                    s.hotkeys.get(id).map(|h| h.caps()).unwrap_or_default()
                );
                // Its reset button sits to the right of the field, off at the default.
                let reset = l
                    .controls
                    .iter()
                    .find_map(|c| match c {
                        Control::IconButton {
                            hit,
                            rect: r,
                            icon,
                            enabled,
                            ..
                        } if *hit == Hit::HotkeyReset(id) => Some((*r, *icon, *enabled)),
                        _ => None,
                    })
                    .expect("a reset button");
                assert!(reset.0.x >= rect.right() && reset.1 == Icon::Reset);
                assert!(!reset.2);
                // The name and the description.
                let name = text(lang, hotkey_row_key(id));
                let desc = text(lang, hotkey_row_desc_key(id));
                for want in [name, desc] {
                    assert!(
                        l.texts.iter().any(|t| t.text == want
                            && t.rect.y >= row_top
                            && t.rect.bottom() <= row_top + ROW_H),
                        "{lang:?} {want}"
                    );
                }
            }
            // One divider on the top edge of every row, the first under the header.
            for i in 0..5 {
                let y = header_bottom + i as f32 * ROW_H;
                assert!(
                    l.dividers.iter().any(|d| (d.y + 0.5 - y).abs() < 0.01),
                    "{lang:?} row {i}"
                );
            }
        }
    }

    #[test]
    fn a_hotkey_field_shows_its_caps_none_or_the_prompt_and_its_row_follows_the_state() {
        for lang in [Lang::Id, Lang::En] {
            let mut s = hotkey_base(lang);
            s.hotkeys.set(
                HotkeyId::Clipboard,
                crate::hotkey::parse("Win+Alt+V").unwrap(),
            );
            let l = layout(Section::Hotkeys, &s, &Estimate);
            let f = hotkey_fields(&l);
            assert_eq!(f[0].2, ["Ctrl", "Alt", "Space"]);
            assert_eq!(f[1].2, ["Win", "Alt", "V"]);
            assert!(f[2].2.is_empty() && f[3].2.is_empty(), "off: no caps");
            assert_eq!(f[4].2, ["PrtSc"]);
            // A changed row can be reset; the others cannot.
            let enabled =
                |l: &Layout, id: HotkeyId| l.hits().iter().any(|(h, _)| *h == Hit::HotkeyReset(id));
            assert!(enabled(&l, HotkeyId::Clipboard));
            assert!(!enabled(&l, HotkeyId::Search));
            assert!(l.hits().iter().any(|(h, _)| *h == Hit::HotkeysResetAll));
            // Every field takes a click.
            for id in HotkeyId::ALL {
                assert!(l.hits().iter().any(|(h, _)| *h == Hit::HotkeyField(id)));
            }
            // Recording: the flag on that field only.
            let rec = State {
                hotkey_recording: Some(HotkeyId::Devices),
                ..s.clone()
            };
            let f = hotkey_fields(&layout(Section::Hotkeys, &rec, &Estimate));
            assert_eq!(
                f.iter().map(|x| x.3).collect::<Vec<_>>(),
                [false, false, true, false, false]
            );
            // The description: red for a refusal on that row, red for a taken
            // one that is set, otherwise secondary.
            let tone_of =
                |l: &Layout, want: &str| l.texts.iter().find(|t| t.text == want).map(|t| t.tone);
            let desc = |id| text(lang, hotkey_row_desc_key(id));
            assert_eq!(tone_of(&l, desc(HotkeyId::Arrange)), Some(Tone::Secondary));
            let err = State {
                hotkey_error: Some((HotkeyId::Arrange, RejectMsg::UsedBy(HotkeyId::Clipboard))),
                ..s.clone()
            };
            let l = layout(Section::Hotkeys, &err, &Estimate);
            let msg = reject_text(lang, RejectMsg::UsedBy(HotkeyId::Clipboard));
            assert_eq!(tone_of(&l, &msg), Some(Tone::Bad));
            assert!(msg.contains(text(lang, Key::HotkeyRowClipboard)), "{msg}");
            assert_eq!(tone_of(&l, desc(HotkeyId::Arrange)), None, "replaced");
            assert_eq!(tone_of(&l, desc(HotkeyId::Devices)), Some(Tone::Secondary));
            for (msg, key) in [
                (RejectMsg::Reserved, Key::HotkeyReserved),
                (RejectMsg::NeedsModifier, Key::HotkeyNeedsModifier),
            ] {
                let s = State {
                    hotkey_error: Some((HotkeyId::Search, msg)),
                    ..s.clone()
                };
                let l = layout(Section::Hotkeys, &s, &Estimate);
                assert_eq!(tone_of(&l, text(lang, key)), Some(Tone::Bad));
            }
            // A set row taken by another app is red; an off one never is.
            let taken = State {
                hotkey_failed: [true; 5],
                ..s.clone()
            };
            let l = layout(Section::Hotkeys, &taken, &Estimate);
            let reds: Vec<&Text> = l.texts.iter().filter(|t| t.tone == Tone::Bad).collect();
            assert_eq!(reds.len(), 3, "search, clipboard and screenshot are set");
            assert!(reds.iter().all(|t| t.text == text(lang, Key::HotkeyTaken)));
        }
    }

    #[test]
    fn the_hotkey_fields_stay_between_the_widths_and_inside_the_card_for_every_combo() {
        let longest = crate::hotkey::parse("Win+Ctrl+Alt+Shift+NumMultiply").unwrap();
        let shortest = crate::hotkey::parse("F13").unwrap();
        for lang in [Lang::Id, Lang::En] {
            for value in [longest, shortest, None] {
                for recording in [None, Some(HotkeyId::Search)] {
                    let mut s = hotkey_base(lang);
                    s.hotkeys.set(HotkeyId::Search, value);
                    s.hotkey_recording = recording;
                    let l = layout(Section::Hotkeys, &s, &Estimate);
                    let f = &hotkey_fields(&l)[0];
                    assert!(
                        (HOTKEY_FIELD_MIN_W..=hotkey_max_w(l.cards[0].w)).contains(&f.1.w),
                        "{lang:?} {value:?}: {}",
                        f.1.w
                    );
                    assert!(inside(f.1, l.cards[0]));
                    // The label column keeps room: the description fits.
                    for t in &l.texts {
                        let w = Estimate.width(&t.text, t.font);
                        assert!(
                            w <= t.rect.w + 0.01,
                            "{lang:?} {:?} {w} > {}",
                            t.text,
                            t.rect.w
                        );
                    }
                }
            }
        }
        // Content decides the width between the limits: a short combo gets the
        // minimum, the longest the maximum (its caps do not all fit then). A
        // wide window allows 260; the minimum window less, so the row's text
        // keeps its room.
        let w_of = |v, client| {
            let mut s = hotkey_base(Lang::En);
            s.hotkeys.set(HotkeyId::Search, v);
            hotkey_fields(&layout_in(Section::Hotkeys, &s, &Estimate, client))[0]
                .1
                .w
        };
        assert_eq!(w_of(shortest, (CLIENT_W, CLIENT_H)), HOTKEY_FIELD_MIN_W);
        assert_eq!(w_of(longest, (1400.0, 900.0)), HOTKEY_FIELD_MAX_W);
        let at_minimum = w_of(longest, (CLIENT_W, CLIENT_H));
        assert!(at_minimum < HOTKEY_FIELD_MAX_W && at_minimum > HOTKEY_FIELD_MIN_W);
        assert_eq!(at_minimum, hotkey_max_w(CONTENT_W));
        let caps = longest.unwrap().caps();
        assert!(hotkey_caps_w(&Estimate, &caps) > HOTKEY_FIELD_MAX_W);
        let small = crate::hotkey::parse("Ctrl+Alt+V").unwrap().unwrap().caps();
        assert!(hotkey_caps_w(&Estimate, &small) <= HOTKEY_FIELD_MAX_W);
    }

    #[test]
    fn hotkey_hits_and_names() {
        assert_eq!(Section::from_name("hotkeys"), Some(Section::Hotkeys));
        assert_eq!(Section::from_name("Tombol-Pintas"), Some(Section::Hotkeys));
        assert_eq!(Section::from_name("tombolpintas"), Some(Section::Hotkeys));
        assert_eq!(Section::from_name("shortcut-keys"), Some(Section::Hotkeys));
        // "shortcuts" is not taken: the tiles are Pintasan.
        assert_eq!(Section::from_name("shortcuts"), None);
        for lang in [Lang::Id, Lang::En] {
            assert_eq!(
                reject_text(lang, RejectMsg::UsedBy(HotkeyId::Arrange)),
                crate::i18n::hotkey_used_by(lang, text(lang, Key::HotkeyRowArrange))
            );
        }
        assert_eq!(
            reject_text(Lang::Id, RejectMsg::UsedBy(HotkeyId::Devices)),
            "Sudah dipakai Buka Perangkat"
        );
        assert_eq!(
            reject_text(Lang::En, RejectMsg::UsedBy(HotkeyId::Devices)),
            "Already used by Open Devices"
        );
        assert_eq!(
            reject_text(Lang::Id, RejectMsg::Reserved),
            "Dipakai Windows"
        );
        assert_eq!(
            reject_text(Lang::En, RejectMsg::Reserved),
            "Reserved by Windows"
        );
        assert_eq!(
            reject_text(Lang::Id, RejectMsg::NeedsModifier),
            "Tambahkan Ctrl, Alt, atau Win"
        );
        assert_eq!(
            reject_text(Lang::En, RejectMsg::NeedsModifier),
            "Add Ctrl, Alt or Win"
        );
    }

    /// The Clipboard section's toggle for `hit`: (on, enabled), and its row's
    /// y so the order of the rows can be checked.
    fn clip_toggle(l: &Layout, hit: Hit) -> Option<(bool, bool, f32)> {
        l.controls.iter().find_map(|c| match c {
            Control::Toggle {
                hit: h,
                rect,
                on,
                enabled,
            } if *h == hit => Some((*on, *enabled, rect.y)),
            _ => None,
        })
    }

    #[test]
    fn the_copy_notice_row_sits_above_disalin_and_follows_the_history() {
        for lang in [Lang::Id, Lang::En] {
            let state = |clip: ClipboardSettings| State {
                lang,
                clip,
                ..states()[0].clone()
            };
            let defaults = ClipboardSettings::default();
            let l = layout(Section::Clipboard, &state(defaults), &Estimate);
            // Present, on by default, directly above the "Disalin" row.
            let (on, enabled, y) = clip_toggle(&l, Hit::ClipNotice).expect("the row");
            assert!(on && enabled, "{lang:?}");
            let (_, _, feedback_y) = clip_toggle(&l, Hit::ClipFeedbackAll).expect("Disalin row");
            assert!((feedback_y - y - ROW_H).abs() < 0.01, "{lang:?}");
            for key in [Key::ClipCopyNotice, Key::ClipCopyNoticeDesc] {
                assert!(
                    l.texts.iter().any(|t| t.text == text(lang, key)),
                    "{lang:?} {key:?}"
                );
            }
            // The preview says what "Disalin" would: that row fades, with a
            // desc that tells so, and takes no click.
            let (_, feedback_on, _) = clip_toggle(&l, Hit::ClipFeedbackAll).unwrap();
            assert!(!feedback_on, "{lang:?}");
            assert!(
                l.texts
                    .iter()
                    .any(|t| t.text == text(lang, Key::ClipFeedbackAllReplaced))
            );
            assert!(!l.hits().iter().any(|(h, _)| *h == Hit::ClipFeedbackAll));
            assert!(l.hits().iter().any(|(h, _)| *h == Hit::ClipNotice));
            // The preview off: "Disalin" is a choice again.
            let off = ClipboardSettings {
                copy_notice: false,
                ..defaults
            };
            let l = layout(Section::Clipboard, &state(off), &Estimate);
            let (on, enabled, _) = clip_toggle(&l, Hit::ClipNotice).unwrap();
            assert!(!on && enabled);
            assert!(clip_toggle(&l, Hit::ClipFeedbackAll).unwrap().1);
            assert!(
                l.texts
                    .iter()
                    .any(|t| t.text == text(lang, Key::ClipFeedbackAllDesc))
            );
            // The history off: the new row fades and takes no clicks too.
            let history_off = ClipboardSettings {
                enabled: false,
                ..defaults
            };
            let l = layout(Section::Clipboard, &state(history_off), &Estimate);
            let (on, enabled, _) = clip_toggle(&l, Hit::ClipNotice).unwrap();
            assert!(on && !enabled, "{lang:?}: faded, its value kept");
            assert!(!l.hits().iter().any(|(h, _)| *h == Hit::ClipNotice));
            // Only "Riwayat clipboard" itself still answers.
            assert!(l.hits().iter().any(|(h, _)| *h == Hit::ClipEnabled));
        }
    }

    #[test]
    fn shot_seconds_step_by_two_within_range() {
        assert_eq!(shot_secs_step(8, 1), 10);
        assert_eq!(shot_secs_step(8, -1), 6);
        assert_eq!(shot_secs_step(4, -1), 4);
        assert_eq!(shot_secs_step(30, 1), 30);
        // Off the grid: snapped onto it.
        assert_eq!(shot_secs_step(9, 1), 10);
        assert_eq!(shot_secs_step(9, -1), 6);
    }

    #[test]
    fn warning_steps_stay_on_their_grid() {
        assert_eq!(disk_warn_step(10, 1), 15);
        assert_eq!(disk_warn_step(10, -1), 5);
        assert_eq!(disk_warn_step(5, -1), 5);
        assert_eq!(disk_warn_step(100, 1), 100);
        // A hand-set value off the grid snaps onto it.
        assert_eq!(disk_warn_step(12, 1), 15);
        assert_eq!(disk_warn_step(12, -1), 10);
        assert_eq!(disk_warn_step(500, -1), 95);
        assert_eq!(ram_warn_step(90, 1), 91);
        assert_eq!(ram_warn_step(90, -1), 89);
        assert_eq!(ram_warn_step(98, 1), 98);
        assert_eq!(ram_warn_step(70, -1), 70);
        assert_eq!(ram_warn_step(40, 1), 71);
        assert_eq!(disk_warn_value(10), "10 GB");
    }

    #[test]
    fn robot_rows_follow_the_setting() {
        for lang in [Lang::Id, Lang::En] {
            let on = State {
                lang,
                robot: true,
                robot_motion: Motion::Saver,
                robot_sleep: 10,
                greeting: false,
                // The eyes need stay-behind off (see the Kucing test below).
                stay_behind: false,
                ..states()[0].clone()
            };
            let on = State {
                scroll: robot_card_scroll(&on),
                ..on
            };
            let l = layout(Section::Cat, &on, &Estimate);
            let (_, items, selected) = l.segmented(Group::Motion).expect("Animasi");
            assert_eq!(items[selected].hit, Hit::Motion(Motion::Saver));
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            let want: Vec<&str> = [Key::MotionFull, Key::MotionSaver, Key::MotionOff]
                .iter()
                .map(|k| text(lang, *k))
                .collect();
            assert_eq!(labels, want);
            let value = l.controls.iter().find_map(|c| match c {
                Control::Stepper { minus, value, .. } if minus.1 == Some(Hit::RobotSleep(-1)) => {
                    Some(value.text.clone())
                }
                _ => None,
            });
            assert_eq!(value.as_deref(), Some(sleep_value(lang, 10).as_str()));
            // A wrapped description is its lines in order, joined by spaces.
            let shown = l
                .texts
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            for key in [
                Key::RobotInNotch,
                Key::RobotInNotchDesc,
                Key::RobotMotion,
                Key::RobotMotionDesc,
                Key::RobotSleep,
                Key::RobotSleepDesc,
                Key::CatGreeting,
                Key::MorningGreetingDesc,
            ] {
                assert!(shown.contains(text(lang, key)), "{key:?}");
            }
            // Off: only the main toggle still takes clicks.
            let off = State {
                robot: false,
                ..on.clone()
            };
            let hit_list =
                |l: &Layout| -> Vec<Hit> { l.hits().into_iter().map(|(h, _)| h).collect() };
            let l = layout(Section::Cat, &off, &Estimate);
            let hits = hit_list(&l);
            assert!(hits.contains(&Hit::Robot));
            // The cat's own rows fade and take no clicks: the coat, the
            // backdrop's "Ubah" and the accessories...
            assert!(!hits.iter().any(|h| matches!(
                h,
                Hit::CatColor(_) | Hit::CatAccessory(_) | Hit::CatBackdrop
            )));
            // ...and Gerak's and Sapaan's, scrolled into view. "Mengecil jadi
            // mata" does not need the cat (the eyes show without it).
            let end = State {
                scroll: 1e6,
                ..off.clone()
            };
            let l = layout(Section::Cat, &end, &Estimate);
            let hits = hit_list(&l);
            assert!(!hits.iter().any(|h| matches!(
                h,
                Hit::Motion(_)
                    | Hit::RobotSleep(_)
                    | Hit::Greeting
                    | Hit::CatFollow
                    | Hit::CatDance
            )));
            assert!(hits.contains(&Hit::NotchPeek));
            // With it on they all take clicks.
            let l = layout(
                Section::Cat,
                &State {
                    scroll: 1e6,
                    ..on.clone()
                },
                &Estimate,
            );
            let hits = hit_list(&l);
            for hit in [
                Hit::Motion(Motion::Full),
                Hit::Greeting,
                Hit::CatFollow,
                Hit::CatDance,
            ] {
                assert!(hits.contains(&hit), "{hit:?}");
            }
            // Mati: no sleep timer, so no sleep setting.
            let still = State {
                robot_motion: Motion::Off,
                scroll: 1e6,
                ..on.clone()
            };
            let l = layout(Section::Cat, &still, &Estimate);
            assert!(
                !l.hits()
                    .iter()
                    .any(|(h, _)| matches!(h, Hit::RobotSleep(_)))
            );
        }
        assert_eq!(sleep_value(Lang::Id, 10), "10 mnt");
        assert_eq!(sleep_value(Lang::En, 10), "10 min");
    }

    /// The design's full "Hemat: tanpa kedip dan gerak, ekspresi tetap"
    /// wraps beside the segments; its row grows by the extra line and the
    /// rows under it follow, clear of the segmented control.
    #[test]
    fn motion_description_wraps_beside_the_segments() {
        assert_eq!(
            text(Lang::Id, Key::RobotMotionDesc),
            "Hemat: tanpa kedip dan gerak, ekspresi tetap"
        );
        for lang in [Lang::Id, Lang::En] {
            let s = State {
                lang,
                robot: true,
                scroll: 1e6,
                ..states()[0].clone()
            };
            let l = layout(Section::Cat, &s, &Estimate);
            let label = l
                .texts
                .iter()
                .position(|t| t.text == text(lang, Key::RobotMotion))
                .expect("Animasi label");
            let sleep = l
                .texts
                .iter()
                .position(|t| t.text == text(lang, Key::RobotSleep))
                .expect("sleep label");
            let lines = &l.texts[label + 1..sleep];
            assert_eq!(
                lines
                    .iter()
                    .map(|t| t.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                text(lang, Key::RobotMotionDesc)
            );
            let (track, _, _) = l.segmented(Group::Motion).expect("segments");
            for t in lines {
                assert!(t.rect.right() + 2.0 * GAP <= track.x + 0.01, "{:?}", t.text);
            }
            // The sleep row starts below the last line, with the usual gap.
            let last = lines.last().expect("a line");
            let sleep_label = &l.texts[sleep];
            assert!(sleep_label.rect.y - last.rect.bottom() >= 15.0 - 0.01);
            assert_eq!(
                last.rect.y - lines[0].rect.y,
                (lines.len() - 1) as f32 * DESC_LINE_H
            );
        }
        let b = Builder {
            m: &Estimate,
            lang: Lang::Id,
            l: Layout::default(),
            y: 0.0,
            g: Geom::new((CLIENT_W, CLIENT_H)),
        };
        assert_eq!(b.wrap("a b", Font::Desc, 1.0), ["a", "b"]);
        assert_eq!(b.wrap("a b", Font::Desc, 100.0), ["a b"]);
        assert_eq!(b.wrap("", Font::Desc, 100.0), [""]);
        assert_eq!(wrapped_row_h(1), ROW_H);
        assert_eq!(wrapped_row_h(2), ROW_H + DESC_LINE_H);
    }

    #[test]
    fn hits_follow_state() {
        let s = &states()[0];
        let l = layout(Section::Appearance, s, &Estimate);
        let (_, items, selected) = l.segmented(Group::Style).unwrap();
        assert_eq!(items[selected].hit, Hit::Style(Style::Notch));
        let r = items[0].rect;
        assert_eq!(
            l.hit(r.x + r.w / 2.0, r.y + r.h / 2.0),
            Some(Hit::Style(Style::Pill))
        );
        // Nav rows switch sections.
        let nav = &l.nav[7];
        assert_eq!(
            l.hit(nav.pill.x + 4.0, nav.pill.y + 4.0),
            Some(Hit::Nav(Section::Integrations))
        );
        // Locked steppers take no clicks; at the end of a range one side is off.
        let locked = State {
            pomodoro_locked: true,
            ..s.clone()
        };
        let l = layout(Section::Features, &locked, &Estimate);
        assert!(!l.hits().iter().any(|(h, _)| matches!(h, Hit::Pomo(..))));
        assert!(l.note_glyph.is_some());
        let free = State {
            pomodoro_locked: false,
            ..s.clone()
        };
        let l = layout(Section::Features, &free, &Estimate);
        let pomo: Vec<Hit> = l
            .hits()
            .into_iter()
            .map(|(h, _)| h)
            .filter(|h| matches!(h, Hit::Pomo(..)))
            .collect();
        // Every value is at its maximum: only the minus buttons work.
        assert_eq!(pomo.len(), 4);
        assert!(pomo.iter().all(|h| matches!(h, Hit::Pomo(_, -1))));
    }

    #[test]
    fn google_buttons_follow_the_status() {
        let base = states()[0].clone();
        let enabled = |g: GoogleState| -> Vec<Hit> {
            let s = State {
                google: g,
                ..base.clone()
            };
            let l = layout(Section::Integrations, &s, &Estimate);
            l.hits()
                .into_iter()
                .map(|(h, _)| h)
                .filter(|h| {
                    matches!(
                        h,
                        Hit::GoogleSave | Hit::GoogleConnect | Hit::GoogleDisconnect
                    )
                })
                .collect()
        };
        let g = |status, dirty, has_id| GoogleState {
            status,
            dirty,
            has_id,
            confirm: false,
            built_in: false,
            advanced: false,
        };
        assert!(enabled(g(CalStatus::NotConfigured, false, false)).is_empty());
        assert_eq!(
            enabled(g(CalStatus::NotConfigured, true, true)),
            [Hit::GoogleSave, Hit::GoogleConnect]
        );
        assert_eq!(
            enabled(g(CalStatus::NeedsLogin, false, true)),
            [Hit::GoogleConnect]
        );
        // Connected: nothing to connect, only to forget.
        assert_eq!(
            enabled(g(CalStatus::Ok, false, true)),
            [Hit::GoogleDisconnect]
        );
        // A refused login can be redone or forgotten.
        assert_eq!(
            enabled(g(CalStatus::Reconnect, false, true)),
            [Hit::GoogleConnect, Hit::GoogleDisconnect]
        );
        assert_eq!(
            enabled(g(CalStatus::Offline, false, true)),
            [Hit::GoogleConnect, Hit::GoogleDisconnect]
        );
    }

    #[test]
    fn built_in_client_offers_one_click_sign_in() {
        let base = states()[0].clone();
        let at = |google: GoogleState| {
            let s = State {
                google,
                ..base.clone()
            };
            layout(Section::Integrations, &s, &Estimate)
        };
        let button = |l: &Layout, hit: Hit| {
            l.controls.iter().find_map(|c| match c {
                Control::Button(b) if b.hit == hit => Some(b.clone()),
                _ => None,
            })
        };
        let g = GoogleState {
            status: CalStatus::NeedsLogin,
            dirty: false,
            has_id: false,
            confirm: false,
            built_in: true,
            advanced: false,
        };
        // One click: no fields, "Masuk dengan Google" as the main button.
        let simple = at(g);
        assert!(simple.fields.is_empty());
        let sign_in = button(&simple, Hit::GoogleConnect).expect("sign-in button");
        assert_eq!(
            (sign_in.label.as_str(), sign_in.kind, sign_in.enabled),
            ("Masuk dengan Google", ButtonKind::Primary, true)
        );
        assert!(button(&simple, Hit::GoogleSave).is_none());
        // Connected: nothing to sign in to, only to forget.
        let ok = at(GoogleState {
            status: CalStatus::Ok,
            ..g
        });
        assert!(!button(&ok, Hit::GoogleConnect).unwrap().enabled);
        assert!(button(&ok, Hit::GoogleDisconnect).unwrap().enabled);
        // "Lanjutan" opens the own-client fields, as without a built-in client.
        // It opens in place, so a chevron, not the "open" glyph of a link.
        let link = button(&simple, Hit::GoogleAdvanced).expect("advanced link");
        assert_eq!(link.kind, ButtonKind::Disclosure(false));
        let open = at(GoogleState {
            advanced: true,
            ..g
        });
        assert_eq!(open.fields.len(), 2);
        assert!(button(&open, Hit::GoogleSave).is_some());
        assert_eq!(
            button(&open, Hit::GoogleAdvanced).map(|b| b.kind),
            Some(ButtonKind::Disclosure(true)),
            "closes again"
        );
        // Without a built-in client: the fields, and no "Lanjutan".
        let plain = at(GoogleState {
            built_in: false,
            ..g
        });
        assert_eq!(plain.fields.len(), 2);
        assert!(button(&plain, Hit::GoogleAdvanced).is_none());
        // An own Client ID wins over Vale's: while one is filled in, its
        // fields and "Hubungkan" show, never "Masuk dengan Google".
        let own = at(GoogleState { has_id: true, ..g });
        assert_eq!(own.fields.len(), 2);
        assert_eq!(button(&own, Hit::GoogleConnect).unwrap().label, "Hubungkan");
    }

    #[test]
    fn clearing_the_clipboard_history_asks_once_more() {
        let base = states()[0].clone();
        let clear = |clip_confirm: bool| {
            let s = State {
                clip_confirm,
                ..base.clone()
            };
            let l = layout(Section::Clipboard, &s, &Estimate);
            l.controls
                .iter()
                .find_map(|c| match c {
                    Control::Button(b) if b.hit == Hit::ClipClear => {
                        Some((b.label.clone(), b.kind))
                    }
                    _ => None,
                })
                .expect("clear button")
        };
        assert_eq!(clear(false).1, ButtonKind::Secondary);
        // The first click arms it, as "Putuskan" does: "Yakin?" in red.
        assert_eq!(clear(true), ("Yakin?".to_string(), ButtonKind::Confirm));
    }

    #[test]
    fn hook_buttons_follow_what_is_installed() {
        let base = states()[0].clone();
        // Buttons in drawing order (left to right) with whether they take clicks.
        let buttons = |hooks: Option<HookState>| -> Vec<(Hit, bool)> {
            let s = State {
                hooks,
                ..base.clone()
            };
            let l = layout(Section::Integrations, &s, &Estimate);
            let mut out: Vec<(f32, Hit, bool)> = l
                .controls
                .iter()
                .filter_map(|c| match c {
                    Control::Button(b) if matches!(b.hit, Hit::HooksInstall | Hit::HooksRemove) => {
                        Some((b.rect.x, b.hit, b.enabled))
                    }
                    _ => None,
                })
                .collect();
            out.sort_by(|a, b| a.0.total_cmp(&b.0));
            out.into_iter().map(|(_, h, e)| (h, e)).collect()
        };
        let state = |complete, any| Some(HookState { complete, any });
        assert_eq!(
            buttons(state(true, true)),
            [(Hit::HooksInstall, false), (Hit::HooksRemove, true)]
        );
        assert_eq!(
            buttons(state(false, true)),
            [(Hit::HooksInstall, true), (Hit::HooksRemove, true)],
            "a partial install can be completed or removed"
        );
        assert_eq!(
            buttons(state(false, false)),
            [(Hit::HooksInstall, true), (Hit::HooksRemove, false)]
        );
        // Unreadable settings: both stay clickable, so a click says why.
        assert_eq!(
            buttons(None),
            [(Hit::HooksInstall, true), (Hit::HooksRemove, true)]
        );
    }

    /// The scroll that brings the Kucing card (the cat's own rows) to the top
    /// of the viewport.
    fn robot_card_scroll(s: &State) -> f32 {
        let top = State {
            scroll: 0.0,
            ..s.clone()
        };
        let l = layout(Section::Cat, &top, &Estimate);
        let view = l.viewport.expect("viewport");
        let toggle = l
            .controls
            .iter()
            .find_map(|c| match c {
                Control::Toggle {
                    hit: Hit::Robot,
                    rect,
                    ..
                } => Some(*rect),
                _ => None,
            })
            .expect("robot toggle");
        let card_top = toggle.y + toggle.h / 2.0 - ROW_H / 2.0;
        card_top - view.y
    }

    /// Every button and handle of a layout: its hit, its label (empty for an
    /// icon) and whether it takes clicks, in drawing order.
    fn buttons_of(l: &Layout) -> Vec<(Hit, String, bool)> {
        l.controls
            .iter()
            .filter_map(|c| match c {
                Control::Button(b) => Some((b.hit, b.label.clone(), b.enabled)),
                Control::IconButton { hit, enabled, .. } => Some((*hit, String::new(), *enabled)),
                _ => None,
            })
            .collect()
    }

    fn button(all: &[(Hit, String, bool)], hit: Hit) -> &(Hit, String, bool) {
        all.iter()
            .find(|b| b.0 == hit)
            .unwrap_or_else(|| panic!("no {hit:?}"))
    }

    #[test]
    fn panel_pages_card_lists_the_shown_pages_first_with_a_handle_and_buttons() {
        for s in states().iter().take(8) {
            let l = layout(Section::Panel, s, &Estimate);
            let all = buttons_of(&l);
            let shown = s.pages.visible();
            let hidden: Vec<Page> = s
                .pages
                .entries()
                .iter()
                .filter(|(_, on)| !on)
                .map(|(p, _)| *p)
                .collect();
            // A handle per shown page, in its order, and the rows follow it.
            let grips: Vec<Hit> = all
                .iter()
                .map(|b| b.0)
                .filter(|h| matches!(h, Hit::PageGrip(_)))
                .collect();
            let want: Vec<Hit> = shown.iter().map(|p| Hit::PageGrip(*p)).collect();
            assert_eq!(grips, want);
            let rows: Vec<Hit> = l
                .rows
                .iter()
                .filter(|r| r.list == List::Pages)
                .map(|r| r.grip)
                .collect();
            assert_eq!(rows, want);
            // Up (−1) and down (+1) among the pages shown: none past the ends.
            for (i, p) in shown.iter().enumerate() {
                assert_eq!(button(&all, Hit::PageMove(*p, -1)).2, i > 0, "{p:?}");
                assert_eq!(
                    button(&all, Hit::PageMove(*p, 1)).2,
                    i + 1 < shown.len(),
                    "{p:?}"
                );
            }
            // "Sembunyikan" on the shown pages (the last one cannot), then
            // "Tampilkan" on the hidden ones.
            let toggles: Vec<(Hit, bool)> = all
                .iter()
                .filter(|b| matches!(b.0, Hit::PageOn(_)))
                .map(|b| (b.0, b.2))
                .collect();
            let want: Vec<(Hit, bool)> = shown
                .iter()
                .map(|p| (Hit::PageOn(*p), shown.len() > 1))
                .chain(hidden.iter().map(|p| (Hit::PageOn(*p), true)))
                .collect();
            assert_eq!(toggles, want);
            for b in &all {
                if let Hit::PageOn(p) = b.0 {
                    let key = if shown.contains(&p) {
                        Key::ListHide
                    } else {
                        Key::ListShow
                    };
                    assert_eq!(b.1, text(s.lang, key), "{p:?}");
                }
            }
            // "Kembalikan bawaan" is on only when something differs.
            let reset = button(&all, Hit::PageReset);
            assert_eq!(reset.1, text(s.lang, Key::ListRestore));
            assert_eq!(reset.2, s.pages != PanelPages::default());
            // The count, and "Tersembunyi" only with hidden pages.
            let count = format!(
                "{}: {}/{}",
                text(s.lang, Key::PagesShown),
                shown.len(),
                s.pages.entries().len()
            );
            assert!(l.texts.iter().any(|t| t.text == count), "{count}");
            let title = text(s.lang, Key::PagesHidden).to_uppercase();
            assert_eq!(l.texts.iter().any(|t| t.text == title), !hidden.is_empty());
        }
    }

    #[test]
    fn panel_pages_card_draws_each_pages_dock_icon_in_its_row() {
        let mut moved = PanelPages::default();
        moved.toggle(Page::Clipboard);
        moved.toggle(Page::Devices);
        moved.toggle(Page::Cli);
        moved.move_to(Page::Shortcuts, 0);
        let mut cases: Vec<State> = states().into_iter().take(8).collect();
        for lang in [Lang::Id, Lang::En] {
            cases.push(State {
                pages: moved.clone(),
                lang,
                ..states()[0].clone()
            });
        }
        let mut saw_hidden = false;
        for s in &cases {
            let l = layout(Section::Panel, s, &Estimate);
            let shown = s.pages.visible();
            let hidden: Vec<Page> = s
                .pages
                .entries()
                .iter()
                .filter(|(_, on)| !on)
                .map(|(p, _)| *p)
                .collect();
            saw_hidden |= !hidden.is_empty();
            let order: Vec<Page> = shown.iter().chain(hidden.iter()).copied().collect();
            assert_eq!(l.glyphs.len(), s.pages.entries().len());
            let icons: Vec<Icon> = l.glyphs.iter().map(|g| g.icon).collect();
            let want: Vec<Icon> = order
                .iter()
                .map(|p| crate::shortcuts::page_icon(*p))
                .collect();
            assert_eq!(icons, want);
            let x0 = l.glyphs[0].rect.x;
            let mut name_x = None;
            for (i, (g, page)) in l.glyphs.iter().zip(&order).enumerate() {
                let is_shown = i < shown.len();
                assert_eq!(g.quiet, !is_shown, "{page:?}");
                assert_eq!(g.tone, Tone::Secondary);
                assert_eq!(g.rect.x, x0, "one icon column");
                assert_eq!((g.rect.w, g.rect.h), (PAGE_ICON, PAGE_ICON));
                let cy = g.rect.y + g.rect.h / 2.0;
                let name = text(s.lang, page_key(*page));
                let t = l
                    .texts
                    .iter()
                    .find(|t| {
                        t.text == name && (t.rect.y + t.rect.h / 2.0 - cy).abs() < ROW_H / 2.0
                    })
                    .unwrap_or_else(|| panic!("no name for {page:?}"));
                assert!(g.rect.x + g.rect.w <= t.rect.x, "{page:?} icon before name");
                assert_eq!(*name_x.get_or_insert(t.rect.x), t.rect.x, "names line up");
                let card = l
                    .cards
                    .iter()
                    .find(|c| {
                        g.rect.x >= c.x
                            && g.rect.right() <= c.right()
                            && g.rect.y >= c.y
                            && g.rect.bottom() <= c.bottom()
                    })
                    .expect("inside a card");
                assert_eq!(t.rect.x, card.x + PAGE_NAME_X);
                if is_shown {
                    let row = l
                        .rows
                        .iter()
                        .find(|r| r.grip == Hit::PageGrip(*page))
                        .expect("a row per shown page");
                    assert!(g.rect.y >= row.rect.y && g.rect.bottom() <= row.rect.bottom());
                    let grip = l
                        .controls
                        .iter()
                        .find_map(|c| match c {
                            Control::IconButton { hit, rect, .. }
                                if *hit == Hit::PageGrip(*page) =>
                            {
                                Some(*rect)
                            }
                            _ => None,
                        })
                        .expect("a grip");
                    assert!(g.rect.x >= grip.x + grip.w, "right of the handle");
                } else {
                    assert!(g.rect.y >= card.y && g.rect.bottom() <= card.bottom());
                    assert!((cy - (t.rect.y + t.rect.h / 2.0)).abs() <= ROW_H / 2.0);
                }
            }
            // Decoration only: no control carries a glyph's rect or icon.
            for c in &l.controls {
                if let Control::IconButton { icon, rect, .. } = c {
                    assert!(!icons.contains(icon), "{icon:?}");
                    assert!(l.glyphs.iter().all(|g| g.rect != *rect));
                }
            }
        }
        assert!(saw_hidden, "a case with hidden pages");
    }

    #[test]
    fn the_icon_of_a_held_page_rides_the_lifted_row() {
        let base = states()[0].clone();
        let shown = base.pages.visible();
        let icons: Vec<Icon> = shown
            .iter()
            .map(|p| crate::shortcuts::page_icon(*p))
            .collect();
        for (i, a) in icons.iter().enumerate() {
            assert!(!icons[i + 1..].contains(a), "one icon per page");
        }
        let drag = held_row(&base, List::Pages, 1, 3);
        let l = layout(
            Section::Panel,
            &State {
                drag: Some(drag),
                ..base.clone()
            },
            &Estimate,
        );
        let lifted = l.lifted.expect("the held row is raised");
        let held = icons[1];
        let g = l.glyphs.iter().find(|g| g.icon == held).expect("its icon");
        assert!(g.rect.y >= lifted.y && g.rect.bottom() <= lifted.bottom());
    }

    #[test]
    fn the_last_page_still_shown_cannot_be_hidden() {
        let mut pages = PanelPages::default();
        for p in &Page::ALL[1..] {
            pages.toggle(*p);
        }
        for lang in [Lang::Id, Lang::En] {
            let s = State {
                pages: pages.clone(),
                lang,
                ..states()[0].clone()
            };
            let l = layout(Section::Panel, &s, &Estimate);
            let all = buttons_of(&l);
            assert!(!button(&all, Hit::PageOn(Page::Calendar)).2, "locked on");
            for p in &Page::ALL[1..] {
                assert!(button(&all, Hit::PageOn(*p)).2, "{p:?} can come back");
            }
            // The reason sits under its name, and nothing can move.
            let why = text(lang, Key::PagesLastShown);
            assert!(l.texts.iter().any(|t| t.text == why));
            assert!(!button(&all, Hit::PageMove(Page::Calendar, 1)).2);
            assert!(!button(&all, Hit::PageMove(Page::Calendar, -1)).2);
        }
    }

    #[test]
    fn pintasan_card_lists_the_shown_tiles_then_the_rest_under_available() {
        for s in states().iter().take(8) {
            let l = layout(Section::Panel, s, &Estimate);
            let all = buttons_of(&l);
            let entries = s.tiles.entries();
            let on: Vec<usize> = (0..entries.len()).filter(|i| entries[*i].1).collect();
            let off: Vec<usize> = (0..entries.len()).filter(|i| !entries[*i].1).collect();
            let grips: Vec<Hit> = all
                .iter()
                .map(|b| b.0)
                .filter(|h| matches!(h, Hit::TileGrip(_)))
                .collect();
            let want: Vec<Hit> = on.iter().map(|i| Hit::TileGrip(*i)).collect();
            assert_eq!(grips, want);
            // "Hapus" on every own tile, shown or not, and on no other.
            let mut removes: Vec<Hit> = all
                .iter()
                .map(|b| b.0)
                .filter(|h| matches!(h, Hit::TileRemove(_)))
                .collect();
            removes.sort_by_key(|h| match h {
                Hit::TileRemove(i) => *i,
                _ => 0,
            });
            let own: Vec<Hit> = (0..entries.len())
                .filter(|i| matches!(entries[*i].0, Tile::Custom(_)))
                .map(Hit::TileRemove)
                .collect();
            assert_eq!(removes, own);
            // Shown: "Sembunyikan", but not the last one. Off: "+ Tambah",
            // not while twelve show.
            for i in &on {
                let b = button(&all, Hit::TileOn(*i));
                assert_eq!(b.1, text(s.lang, Key::ListHide));
                assert_eq!(b.2, on.len() > 1);
            }
            for i in &off {
                let b = button(&all, Hit::TileOn(*i));
                assert_eq!(b.1, text(s.lang, Key::TileAdd));
                assert_eq!(b.2, !s.tiles.full(), "{i}");
            }
            if s.tiles.full() {
                let full = text(s.lang, Key::TilePageFull);
                assert!(l.texts.iter().any(|t| t.text == full));
            }
            // The count against the twelve the page takes.
            let count = format!("{}: {}/12", text(s.lang, Key::TilesShown), on.len().min(12));
            assert!(l.texts.iter().any(|t| t.text == count), "{count}");
            // "Kembalikan bawaan" only when it would change something.
            let mut reset = s.tiles.clone();
            reset.reset();
            assert_eq!(button(&all, Hit::TileReset).2, reset != s.tiles);
            // The form opens while there is room for another own tile.
            let open = button(&all, Hit::TileFormOpen);
            assert_eq!(open.1, text(s.lang, Key::TileAddOwn));
            assert_eq!(open.2, s.tiles.customs() < MAX_CUSTOM);
            // Each group of the available tiles has its title.
            for g in TileGroup::ALL {
                let has = off.iter().any(|i| group(&entries[*i].0) == g);
                let key = match g {
                    TileGroup::Pages => Key::TileGroupPages,
                    TileGroup::Windows => Key::TileGroupWindows,
                    TileGroup::Vale => Key::TileGroupVale,
                    TileGroup::Own => Key::TileGroupOwn,
                };
                let title = text(s.lang, key).to_uppercase();
                let drawn = l
                    .texts
                    .iter()
                    .any(|t| t.text == title && t.font == Font::Eyebrow);
                assert_eq!(drawn, has, "{g:?}");
            }
        }
    }

    #[test]
    fn the_own_shortcut_form_asks_for_a_path_only_for_an_app_or_a_folder() {
        for lang in [Lang::Id, Lang::En] {
            for kind in CustomKind::ALL {
                let s = State {
                    lang,
                    tile_form: Some(TileForm { kind, error: None }),
                    ..states()[0].clone()
                };
                let l = layout(Section::Panel, &s, &Estimate);
                let all = buttons_of(&l);
                let field = |f: Field| {
                    l.fields
                        .iter()
                        .find(|(x, _)| *x == f)
                        .map(|(_, r)| *r)
                        .unwrap_or_else(|| panic!("{f:?}"))
                };
                let (name, target) = (field(Field::TileName), field(Field::TileTarget));
                assert!(target.y > name.y);
                assert!(target.x == name.x);
                // "Pilih…" takes room from the Target field, not for a web
                // address.
                let browse = all.iter().any(|b| b.0 == Hit::TileBrowse);
                assert_eq!(browse, kind != CustomKind::Url, "{kind:?}");
                if browse {
                    assert!(target.right() < name.right());
                } else {
                    assert_eq!(target.right(), name.right());
                }
                // Jenis: Aplikasi | Folder | Web, the kind chosen selected.
                let (_, items, selected) = l.segmented(Group::TileKind).expect("Jenis");
                let hits: Vec<Hit> = items.iter().map(|i| i.hit).collect();
                let want: Vec<Hit> = CustomKind::ALL
                    .iter()
                    .map(|k| Hit::TileFormKind(*k))
                    .collect();
                assert_eq!(hits, want);
                assert_eq!(items[selected].hit, Hit::TileFormKind(kind));
                assert_eq!(items[2].label, text(lang, Key::TileKindWeb));
                // "Batal" and "Tambah", and no "+ Pintasan sendiri" meanwhile.
                assert_eq!(button(&all, Hit::TileFormCancel).1, text(lang, Key::Cancel));
                assert_eq!(
                    button(&all, Hit::TileFormAdd).1,
                    text(lang, Key::TileFormAdd)
                );
                assert!(all.iter().all(|b| b.0 != Hit::TileFormOpen));
            }
            // Closed: no fields, the button to open it.
            let s = State {
                lang,
                ..states()[0].clone()
            };
            let l = layout(Section::Panel, &s, &Estimate);
            assert!(l.fields.is_empty());
            assert!(buttons_of(&l).iter().any(|b| b.0 == Hit::TileFormOpen));
        }
    }

    #[test]
    fn the_form_says_what_is_wrong_under_its_fields_in_red() {
        for lang in [Lang::Id, Lang::En] {
            let keys = [
                Key::TileErrName,
                Key::TileErrPath,
                Key::TileErrUrl,
                Key::TileOwnFull,
            ];
            for key in keys {
                let ok = State {
                    lang,
                    tile_form: Some(TileForm {
                        kind: CustomKind::App,
                        error: None,
                    }),
                    ..states()[0].clone()
                };
                let bad = State {
                    tile_form: Some(TileForm {
                        kind: CustomKind::App,
                        error: Some(key),
                    }),
                    ..ok.clone()
                };
                let (l0, l1) = (
                    layout(Section::Panel, &ok, &Estimate),
                    layout(Section::Panel, &bad, &Estimate),
                );
                let red: Vec<&Text> = l1.texts.iter().filter(|t| t.tone == Tone::Bad).collect();
                assert!(!red.is_empty(), "{key:?}");
                let joined: Vec<&str> = red.iter().map(|t| t.text.as_str()).collect();
                assert_eq!(joined.join(" "), text(lang, key));
                assert!(l0.texts.iter().all(|t| t.tone != Tone::Bad));
                // The card grows by the message; the fields stay put.
                let (c0, c1) = (l0.cards.last().unwrap(), l1.cards.last().unwrap());
                assert!(c1.h > c0.h);
                assert_eq!(l0.fields, l1.fields);
            }
        }
    }

    #[test]
    fn a_tile_added_off_says_why_beside_its_button() {
        let base = states()[0].clone();
        let l0 = layout(Section::Panel, &base, &Estimate);
        let s = State {
            tile_note: Some(Key::TileAddedOff),
            ..base
        };
        let l = layout(Section::Panel, &s, &Estimate);
        let note = text(s.lang, Key::TileAddedOff);
        let lines: Vec<&str> = l
            .texts
            .iter()
            .map(|t| t.text.as_str())
            .filter(|t| t.chars().count() > 4 && note.contains(t))
            .collect();
        assert_eq!(lines.join(" "), note);
        assert!(l.cards.last().unwrap().h > l0.cards.last().unwrap().h);
    }

    #[test]
    fn a_drag_follows_the_pointer_while_the_other_rows_make_room() {
        for list in [List::Pages, List::Tiles] {
            let base = states()[0].clone();
            let l0 = layout(Section::Panel, &base, &Estimate);
            let rows0: Vec<&ReorderRow> = l0.rows.iter().filter(|r| r.list == list).collect();
            let tops: Vec<f32> = rows0.iter().map(|r| r.rect.y).collect();
            assert!(tops.windows(2).all(|w| w[1] - w[0] == ROW_H));
            assert!(l0.lifted.is_none());

            let drag = held_row(&base, list, 1, 3);
            let l = layout(
                Section::Panel,
                &State {
                    drag: Some(drag),
                    ..base.clone()
                },
                &Estimate,
            );
            let rows: Vec<&ReorderRow> = l.rows.iter().filter(|r| r.list == list).collect();
            assert_eq!(rows.len(), rows0.len());
            let y_of = |hit: Hit| rows.iter().find(|r| r.grip == hit).unwrap().rect.y;
            // The row in hand is under the pointer; the ones it passed move
            // up a place; the rest stay.
            assert_eq!(y_of(rows0[1].grip), drag.top);
            assert_eq!(y_of(rows0[0].grip), tops[0]);
            assert_eq!(y_of(rows0[2].grip), tops[1]);
            assert_eq!(y_of(rows0[3].grip), tops[2]);
            assert_eq!(y_of(rows0[4].grip), tops[4]);
            let lifted = l.lifted.expect("the held row is raised");
            assert_eq!(lifted.y, drag.top);
            assert_eq!(lifted.h, ROW_H);
            // Nothing grows or shrinks.
            assert_eq!(l.cards, l0.cards);
            assert!(buttons_of(&l).len() == buttons_of(&l0).len());
        }
    }

    #[test]
    fn drop_index_takes_the_row_the_pointer_is_in() {
        let tops = [100.0, 152.0, 204.0, 256.0];
        assert_eq!(drop_index(0.0, &tops), 0, "above the list");
        assert_eq!(drop_index(100.0, &tops), 0);
        assert_eq!(drop_index(151.9, &tops), 0);
        assert_eq!(drop_index(152.0, &tops), 1);
        assert_eq!(drop_index(230.0, &tops), 2);
        assert_eq!(drop_index(256.0, &tops), 3);
        assert_eq!(drop_index(900.0, &tops), 3, "below the list");
        assert_eq!(drop_index(10.0, &[]), 0);
    }

    #[test]
    fn drag_scroll_speed_is_zero_in_the_middle_and_grows_toward_the_edges() {
        let (top, bottom) = (60.0, 520.0);
        assert_eq!(drag_scroll_speed(290.0, top, bottom), 0.0);
        assert_eq!(drag_scroll_speed(top + DRAG_SCROLL_ZONE, top, bottom), 0.0);
        assert_eq!(
            drag_scroll_speed(bottom - DRAG_SCROLL_ZONE, top, bottom),
            0.0
        );
        // The slowest speed, just inside the zone: about 2 rows a second.
        let slow = drag_scroll_speed(top + DRAG_SCROLL_ZONE - 0.01, top, bottom);
        assert!((slow / ROW_H + 2.0).abs() < 0.01, "{slow}");
        let slow = drag_scroll_speed(bottom - DRAG_SCROLL_ZONE + 0.01, top, bottom);
        assert!((slow / ROW_H - 2.0).abs() < 0.01, "{slow}");
        // The fastest at the edge and beyond: 12 rows a second.
        for y in [top, top - 1.0, top - 500.0] {
            assert_eq!(drag_scroll_speed(y, top, bottom), -12.0 * ROW_H, "{y}");
        }
        for y in [bottom, bottom + 1.0, bottom + 500.0] {
            assert_eq!(drag_scroll_speed(y, top, bottom), 12.0 * ROW_H, "{y}");
        }
        // The sign: up near the top, down near the bottom.
        assert!(drag_scroll_speed(top + 10.0, top, bottom) < 0.0);
        assert!(drag_scroll_speed(bottom - 10.0, top, bottom) > 0.0);
    }

    #[test]
    fn drag_scroll_speed_is_monotonic_and_symmetric() {
        let (top, bottom) = (60.0, 520.0);
        let mut last = 0.0_f32;
        for i in 0..=480 {
            let d = DRAG_SCROLL_ZONE - 0.01 - i as f32 * 0.1;
            let up = -drag_scroll_speed(top + d, top, bottom);
            let down = drag_scroll_speed(bottom - d, top, bottom);
            assert!((up - down).abs() < 0.001, "symmetric at {d}");
            assert!(up >= last - 0.001, "monotonic at {d}: {up} < {last}");
            last = up;
        }
    }

    #[test]
    fn drag_scroll_speed_in_a_short_viewport_follows_the_nearer_edge() {
        // 60 px tall: every point is inside both zones.
        let (top, bottom) = (100.0, 160.0);
        assert!(drag_scroll_speed(120.0, top, bottom) < 0.0);
        assert!(drag_scroll_speed(140.0, top, bottom) > 0.0);
        assert_eq!(
            drag_scroll_speed(120.0, top, bottom),
            -drag_scroll_speed(140.0, top, bottom)
        );
    }

    #[test]
    fn display_order_moves_the_row_held_to_where_it_would_drop() {
        let drag = |from, to| DragView {
            list: List::Pages,
            from,
            to,
            top: 0.0,
        };
        assert_eq!(display_order(5, None), [0, 1, 2, 3, 4]);
        assert_eq!(display_order(5, Some(&drag(1, 3))), [0, 2, 3, 1, 4]);
        assert_eq!(display_order(5, Some(&drag(3, 0))), [3, 0, 1, 2, 4]);
        assert_eq!(display_order(5, Some(&drag(2, 2))), [0, 1, 2, 3, 4]);
        assert_eq!(
            display_order(5, Some(&drag(0, 9))),
            [1, 2, 3, 4, 0],
            "clamped"
        );
        assert_eq!(
            display_order(3, Some(&drag(7, 0))),
            [0, 1, 2],
            "no such row"
        );
    }

    #[test]
    fn tentang_has_four_rows_the_update_row_and_the_auto_toggle_and_never_scrolls() {
        for lang in [Lang::Id, Lang::En] {
            let base = State {
                lang,
                ..states()[0].clone()
            };
            let l = layout(Section::About, &base, &Estimate);
            assert_eq!(l.scroll, 0.0);
            let all = buttons_of(&l);
            for hit in [Hit::Repo, Hit::UpdateCheck, Hit::DataFolder] {
                assert!(all.iter().any(|b| b.0 == hit), "{hit:?}");
            }
            assert!(
                !all.iter().any(|b| b.0 == Hit::UpdateAuto),
                "the auto row is a toggle, not a button"
            );
            // Four rows: Kode sumber, Pembaruan, the toggle, Folder data.
            let card = l.cards.last().expect("the Tautan card");
            assert_eq!(card.h, 4.0 * ROW_H);
            let toggle = l.controls.iter().find_map(|c| match c {
                Control::Toggle { hit, rect, on, .. } if *hit == Hit::UpdateAuto => {
                    Some((*rect, *on))
                }
                _ => None,
            });
            let (rect, on) = toggle.expect("the auto-update toggle");
            assert_eq!(on, base.update_auto);
            assert!(rect.y > card.y + 2.0 * ROW_H && rect.y < card.y + 3.0 * ROW_H);
            let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            assert_eq!(l.hit(cx, cy), Some(Hit::UpdateAuto));
            // The window is tall enough for the whole section.
            assert!(card.bottom() + CARD_GAP <= CLIENT_H);
        }
    }

    #[test]
    fn the_update_row_says_what_is_known_and_offers_the_next_step() {
        let desc_and_button = |lang, row: UpdateRow| {
            let s = State {
                lang,
                update_row: row,
                ..states()[0].clone()
            };
            let l = layout(Section::About, &s, &Estimate);
            let (_, label, enabled) = button(&buttons_of(&l), Hit::UpdateCheck).clone();
            // A disabled button takes no click: its hit is not listed.
            let clickable = l.hits().iter().any(|(h, _)| *h == Hit::UpdateCheck);
            assert_eq!(clickable, enabled);
            let texts: Vec<String> = l.texts.iter().map(|t| t.text.clone()).collect();
            (texts, label, enabled)
        };
        let has = |texts: &[String], what: &str| texts.iter().any(|t| t == what);
        let (t, label, on) = desc_and_button(Lang::Id, UpdateRow::Idle);
        assert!(has(&t, &format!("Versi terpasang {}", "2.1.0")));
        assert_eq!((label.as_str(), on), ("Cek sekarang", true));
        let (t, label, on) = desc_and_button(Lang::En, UpdateRow::Idle);
        assert!(has(&t, "Installed version 2.1.0"));
        assert_eq!((label.as_str(), on), ("Check now", true));
        let (t, label, on) = desc_and_button(Lang::Id, UpdateRow::Checking);
        assert!(has(&t, "Memeriksa\u{2026}"));
        assert_eq!((label.as_str(), on), ("Cek sekarang", false));
        let (t, _, on) = desc_and_button(Lang::En, UpdateRow::Checking);
        assert!(has(&t, "Checking\u{2026}"));
        assert!(!on);
        let (t, label, on) = desc_and_button(Lang::Id, UpdateRow::Available("9.9.9".into()));
        assert!(has(&t, "Vale 9.9.9 tersedia"));
        assert_eq!((label.as_str(), on), ("Update", true));
        let (t, _, _) = desc_and_button(Lang::En, UpdateRow::Available("9.9.9".into()));
        assert!(has(&t, "Vale 9.9.9 is available"));
        let (t, label, on) = desc_and_button(Lang::Id, UpdateRow::Downloading(42));
        assert!(has(&t, "Mengunduh pembaruan\u{2026} 42%"));
        assert_eq!((label.as_str(), on), ("Update", false));
        let (t, _, on) = desc_and_button(Lang::En, UpdateRow::Downloading(7));
        assert!(has(&t, "Downloading update\u{2026} 7%"));
        assert!(!on);
    }

    #[test]
    fn idle_clock_toggle_shows_the_setting() {
        for on in [true, false] {
            let s = State {
                idle_clock: on,
                ..states()[0].clone()
            };
            let l = layout(Section::Appearance, &s, &Estimate);
            let toggle = l.controls.iter().find_map(|c| match c {
                Control::Toggle { hit, rect, on, .. } if *hit == Hit::IdleClock => {
                    Some((*rect, *on))
                }
                _ => None,
            });
            let (rect, shown) = toggle.expect("idle clock toggle in Appearance");
            assert_eq!(shown, on);
            let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            assert_eq!(l.hit(cx, cy), Some(Hit::IdleClock));
        }
    }

    #[test]
    fn eq_follow_audio_toggle_shows_the_setting() {
        for on in [true, false] {
            let s = State {
                eq_follow_audio: on,
                ..states()[0].clone()
            };
            let l = layout(Section::Appearance, &s, &Estimate);
            let toggle = l.controls.iter().find_map(|c| match c {
                Control::Toggle { hit, rect, on, .. } if *hit == Hit::EqFollowAudio => {
                    Some((*rect, *on))
                }
                _ => None,
            });
            let (rect, shown) = toggle.expect("equalizer toggle in Appearance");
            assert_eq!(shown, on);
            let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            assert_eq!(l.hit(cx, cy), Some(Hit::EqFollowAudio));
            // It sits in the Island card, right under "Jam saat kosong".
            let clock = l.controls.iter().find_map(|c| match c {
                Control::Toggle { hit, rect, .. } if *hit == Hit::IdleClock => Some(*rect),
                _ => None,
            });
            assert!((rect.y - clock.expect("idle clock toggle").y - ROW_H).abs() < 0.01);
        }
    }

    #[test]
    fn lyrics_toggle_in_general_with_privacy_note() {
        for lang in [Lang::Id, Lang::En] {
            for on in [true, false] {
                let s = State {
                    lang,
                    lyrics: on,
                    ..states()[0].clone()
                };
                let l = layout(Section::Features, &s, &Estimate);
                let toggle = l.controls.iter().find_map(|c| match c {
                    Control::Toggle { hit, rect, on, .. } if *hit == Hit::Lyrics => {
                        Some((*rect, *on))
                    }
                    _ => None,
                });
                let (rect, shown) = toggle.expect("lyrics toggle in Fitur");
                assert_eq!(shown, on);
                let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
                assert_eq!(l.hit(cx, cy), Some(Hit::Lyrics));
                for key in [Key::LyricsInNotch, Key::LyricsInNotchDesc] {
                    assert!(
                        l.texts.iter().any(|t| t.text == text(lang, key)),
                        "{lang:?} {key:?}"
                    );
                }
            }
        }
        // Not in Umum or Tampilan.
        let l = layout(Section::Appearance, &states()[0], &Estimate);
        assert!(!l.hits().iter().any(|(h, _)| *h == Hit::Lyrics));
    }

    #[test]
    fn the_cli_card_holds_the_start_folder_just_above_the_shell() {
        for lang in [Lang::Id, Lang::En] {
            for (start, shell) in [
                (CliStart::Explorer, Shell::PowerShell),
                (CliStart::Last, Shell::Cmd),
            ] {
                let s = State {
                    lang,
                    cli_start: start,
                    shell,
                    // Notifikasi sits above it now: scrolled to the end.
                    scroll: f32::MAX,
                    ..states()[0].clone()
                };
                let l = layout(Section::Features, &s, &Estimate);
                let (track, items, selected) =
                    l.segmented(Group::CliStart).expect("Folder awal CLI");
                assert_eq!(items[selected].hit, Hit::CliStart(start));
                let hits: Vec<Hit> = items.iter().map(|i| i.hit).collect();
                assert_eq!(
                    hits,
                    [
                        Hit::CliStart(CliStart::Explorer),
                        Hit::CliStart(CliStart::Last)
                    ]
                );
                let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
                assert_eq!(
                    labels,
                    [
                        text(lang, Key::CliStartExplorer),
                        text(lang, Key::CliStartLast)
                    ]
                );
                let r = items[1].rect;
                assert_eq!(
                    l.hit(r.x + r.w / 2.0, r.y + r.h / 2.0),
                    Some(Hit::CliStart(CliStart::Last))
                );
                // One row above the shell, in a card of their own under "CLI".
                let (shell_track, shell_items, shell_selected) =
                    l.segmented(Group::Shell).expect("Shell CLI");
                assert_eq!(shell_track.y - track.y, ROW_H);
                assert_eq!(shell_items[shell_selected].hit, Hit::Shell(shell));
                let hits: Vec<Hit> = shell_items.iter().map(|i| i.hit).collect();
                assert_eq!(
                    hits,
                    [Hit::Shell(Shell::PowerShell), Hit::Shell(Shell::Cmd)]
                );
                let r = shell_items[1].rect;
                assert_eq!(
                    l.hit(r.x + r.w / 2.0, r.y + r.h / 2.0),
                    Some(Hit::Shell(Shell::Cmd))
                );
                let card = l
                    .cards
                    .iter()
                    .find(|c| c.contains(track.x, track.y))
                    .expect("their card");
                assert_eq!(card.h, 2.0 * ROW_H);
                for key in [
                    Key::CliStartFolder,
                    Key::CliStartFolderDesc,
                    Key::ShellCli,
                    Key::ShellCliDesc,
                ] {
                    assert!(
                        l.texts.iter().any(|t| t.text == text(lang, key)),
                        "{lang:?} {key:?}"
                    );
                }
                assert!(
                    l.texts
                        .iter()
                        .any(|t| t.text == text(lang, Key::GroupCli).to_uppercase())
                );
            }
        }
        // Not in the other sections.
        for section in [Section::Appearance, Section::General, Section::Panel] {
            let l = layout(section, &states()[0], &Estimate);
            assert!(l.segmented(Group::Shell).is_none());
            assert!(l.segmented(Group::CliStart).is_none());
        }
    }

    #[test]
    fn no_divider_sits_on_a_cards_top_edge() {
        for lang in [Lang::Id, Lang::En] {
            for section in Section::ALL {
                let s = State {
                    lang,
                    ..states()[0].clone()
                };
                let l = layout(section, &s, &Estimate);
                for card in &l.cards {
                    for d in &l.dividers {
                        assert!(
                            (d.y + 0.5 - card.y).abs() > 0.01,
                            "{section:?}: a divider on the top edge of the card at y={}",
                            card.y
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_devices_card_is_one_row_with_the_notice_toggle_and_no_divider() {
        for lang in [Lang::Id, Lang::En] {
            for on in [true, false] {
                let s = State {
                    lang,
                    device_notices: on,
                    scroll: 1e6,
                    ..states()[0].clone()
                };
                let l = layout(Section::Features, &s, &Estimate);
                // Its own card under its label, before "Pembersih": one row.
                let card = l.cards[l.cards.len() - 2];
                assert_eq!(card.h, ROW_H);
                assert!(
                    l.texts
                        .iter()
                        .any(|t| t.text == text(lang, Key::PageDevices).to_uppercase())
                );
                for key in [Key::DeviceNotices, Key::DeviceNoticesDesc] {
                    assert!(l.texts.iter().any(|t| t.text == text(lang, key)), "{key:?}");
                }
                let toggle = l.controls.iter().find_map(|c| match c {
                    Control::Toggle {
                        hit: Hit::DeviceNotices,
                        on,
                        rect,
                        ..
                    } => Some((*on, *rect)),
                    _ => None,
                });
                let (shown_on, rect) = toggle.expect("the notices toggle");
                assert_eq!(shown_on, on);
                assert!(rect.y >= card.y && rect.bottom() <= card.bottom());
                assert_eq!(
                    l.hit(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
                    Some(Hit::DeviceNotices)
                );
                // No hotkey control and no divider inside the card.
                assert!(!l.hits().iter().any(|(h, r)| matches!(
                    h,
                    Hit::HotkeyField(_) | Hit::HotkeyReset(_)
                ) && inside(*r, card)));
                let dividers = l
                    .dividers
                    .iter()
                    .filter(|d| d.y >= card.y - 1.0 && d.y <= card.bottom() + 1.0)
                    .count();
                assert_eq!(dividers, 0, "{lang:?}");
            }
        }
    }

    fn susun_state(templates: usize, saved: usize) -> State<'static> {
        let node = crate::arrange::BuiltIn::Main2.node();
        let mut arrange = ArrangeSettings::default();
        for i in 0..templates {
            arrange.templates.push(
                crate::arrange::CustomTemplate::new(
                    &format!("t{i}"),
                    &format!("T{i}"),
                    node.clone(),
                )
                .unwrap(),
            );
        }
        for i in 0..saved {
            arrange.saved.push(
                crate::arrange::SavedArrangement::new(
                    &format!("S{i}"),
                    crate::arrange::LayoutRef::BuiltIn(crate::arrange::BuiltIn::Cols2),
                    crate::arrange::SavedMonitor {
                        device: "D".into(),
                        rect: crate::arrange::Rect::new(0, 0, 10, 10),
                    },
                    vec![None, None],
                )
                .unwrap(),
            );
        }
        State {
            arrange,
            scroll: 0.0,
            ..states()[0].clone()
        }
    }

    #[test]
    fn fitur_has_a_titled_susun_card_that_opens_with_the_gap() {
        for lang in [Lang::Id, Lang::En] {
            for (templates, saved) in [(0, 0), (2, 3)] {
                let mut s = susun_state(templates, saved);
                s.lang = lang;
                s.scroll = 1e6;
                let l = layout(Section::Features, &s, &Estimate);
                assert!(
                    l.texts
                        .iter()
                        .any(|t| t.text == text(lang, Key::PageArrange).to_uppercase()),
                    "the eyebrow"
                );
                // The card: the gap, the templates' title row, the templates,
                // the saved title row and the saved (an empty list keeps one
                // row for its note).
                let rows = 1 + 1 + templates.max(1) + 1 + saved.max(1);
                let card = l
                    .cards
                    .iter()
                    .copied()
                    .find(|c| (c.h - rows as f32 * ROW_H).abs() < 0.01)
                    .expect("the Susun card");
                // The gap row is the first row, with nothing above it: no
                // hotkey control, no divider on its top edge.
                let gap = l
                    .controls
                    .iter()
                    .find_map(|c| match c {
                        Control::Slider {
                            id: SliderId::ArrangeGap,
                            track,
                            ..
                        } => Some(*track),
                        _ => None,
                    })
                    .expect("the gap slider");
                assert!(
                    gap.y >= card.y && gap.bottom() <= card.y + ROW_H,
                    "{lang:?}"
                );
                assert!(
                    l.hits()
                        .iter()
                        .all(|(h, _)| !matches!(h, Hit::HotkeyField(_) | Hit::HotkeyReset(_)))
                );
                assert!(
                    l.dividers.iter().all(|d| (d.y + 0.5 - card.y).abs() > 0.01),
                    "no divider on the card's top edge"
                );
                // The first divider in the card is above the templates' title.
                let first = l
                    .dividers
                    .iter()
                    .filter(|d| d.y > card.y && d.y < card.bottom())
                    .map(|d| d.y + 0.5)
                    .fold(f32::MAX, f32::min);
                assert!((first - (card.y + ROW_H)).abs() < 0.01, "{lang:?}");
            }
        }
        assert_eq!(SliderId::ArrangeGap.range(), (0, 24));
    }

    #[test]
    fn the_gap_slider_shows_pixels_and_follows_the_setting() {
        let mut s = susun_state(0, 0);
        s.arrange.gap = 16;
        s.scroll = 1e6;
        let l = layout(Section::Features, &s, &Estimate);
        assert!(l.texts.iter().any(|t| t.text == "16 px"));
        let value = l.controls.iter().find_map(|c| match c {
            Control::Slider {
                id: SliderId::ArrangeGap,
                value,
                ..
            } => Some(*value),
            _ => None,
        });
        assert_eq!(value, Some(16));
    }

    #[test]
    fn susun_lists_each_template_and_saved_arrangement_with_its_buttons() {
        let mut s = susun_state(2, 3);
        s.scroll = 1e6;
        let l = layout(Section::Features, &s, &Estimate);
        let all = buttons_of(&l);
        for i in 0..2 {
            assert!(button(&all, Hit::ArrangeTemplateEdit(i)).2);
            assert!(button(&all, Hit::ArrangeTemplateDelete(i)).2);
        }
        for i in 0..3 {
            assert!(button(&all, Hit::ArrangeSavedRename(i)).2);
            assert!(button(&all, Hit::ArrangeSavedDelete(i)).2);
        }
        assert!(all.iter().all(|b| b.0 != Hit::ArrangeTemplateEdit(2)));
        // One miniature per row.
        let minis = l
            .controls
            .iter()
            .filter(|c| matches!(c, Control::Mini { canvas: false, .. }))
            .count();
        assert_eq!(minis, 5);
        assert!(button(&all, Hit::ArrangeTemplateNew).2, "room for more");
    }

    #[test]
    fn the_new_template_button_is_off_at_twelve_and_says_so() {
        for lang in [Lang::Id, Lang::En] {
            let mut s = susun_state(12, 0);
            s.lang = lang;
            s.scroll = 1e6;
            let l = layout(Section::Features, &s, &Estimate);
            let all = buttons_of(&l);
            assert!(!button(&all, Hit::ArrangeTemplateNew).2);
            assert!(
                l.texts
                    .iter()
                    .any(|t| t.text == text(lang, Key::ArrangeTemplatesFull))
            );
        }
    }

    #[test]
    fn fitur_with_the_susun_card_still_scrolls_to_its_end_and_fits() {
        for (templates, saved) in [(0, 0), (12, 12)] {
            let mut s = susun_state(templates, saved);
            s.scroll = f32::MAX;
            let l = layout(Section::Features, &s, &Estimate);
            let view = l.viewport.expect("Fitur scrolls");
            assert!(l.scroll_max > 0.0 && l.scroll <= l.scroll_max);
            let last = l.cards.last().expect("a card");
            assert!(last.bottom() <= view.bottom() + 0.5, "the end is reachable");
            for c in &l.controls {
                let b = c.bounds();
                assert!(b.x >= CONTENT_X - 1.0 && b.right() <= CONTENT_X + CONTENT_W + 1.0);
            }
        }
    }

    fn editor_state(
        root: crate::arrange::Node,
        selected: usize,
        name: bool,
        rename: bool,
    ) -> State<'static> {
        State {
            arrange_edit: Some(EditView {
                rename,
                root,
                selected,
                name_ok: name,
            }),
            ..susun_state(1, 1)
        }
    }

    #[test]
    fn the_editor_has_a_sixteen_nine_canvas_and_its_buttons() {
        let one = crate::arrange::Node::Leaf;
        let l = layout(
            Section::Features,
            &editor_state(one, 0, false, false),
            &Estimate,
        );
        let canvas = l.canvas.expect("a canvas");
        assert!((canvas.w / canvas.h - 16.0 / 9.0).abs() < 0.02);
        assert!(l.fields.iter().any(|(f, _)| *f == Field::TileName), "Nama");
        let all = buttons_of(&l);
        // One tile: both splits are fine, merge is not; no name: no Simpan.
        assert!(button(&all, Hit::ArrangeSplit(Dir::Cols)).2);
        assert!(button(&all, Hit::ArrangeSplit(Dir::Rows)).2);
        assert!(!button(&all, Hit::ArrangeMerge).2, "disabled with one tile");
        assert!(!button(&all, Hit::ArrangeEditorSave).2, "no valid name");
        assert!(button(&all, Hit::ArrangeEditorCancel).2);
        // The rest of Fitur is not there while it is open.
        assert!(l.segmented(Group::Shell).is_none());
        // With a name: Simpan is on.
        let l = layout(
            Section::Features,
            &editor_state(crate::arrange::Node::Leaf, 0, true, false),
            &Estimate,
        );
        assert!(button(&buttons_of(&l), Hit::ArrangeEditorSave).2);
    }

    #[test]
    fn the_editor_disables_a_split_the_model_refuses_and_merge_needs_two_tiles() {
        // Six tiles: no split at all.
        let mut full = crate::arrange::Node::Leaf;
        for _ in 0..5 {
            let at = (0..full.leaves())
                .find(|i| full.clone().split(*i, Dir::Rows) || full.clone().split(*i, Dir::Cols));
            let at = at.expect("room for another");
            let dir = if full.clone().split(at, Dir::Rows) {
                Dir::Rows
            } else {
                Dir::Cols
            };
            full.split(at, dir);
        }
        assert_eq!(full.leaves(), 6);
        let l = layout(
            Section::Features,
            &editor_state(full, 0, true, false),
            &Estimate,
        );
        let all = buttons_of(&l);
        assert!(!button(&all, Hit::ArrangeSplit(Dir::Cols)).2);
        assert!(!button(&all, Hit::ArrangeSplit(Dir::Rows)).2);
        assert!(button(&all, Hit::ArrangeMerge).2);
        // A tile 0.25 wide cannot be split side by side, only top to bottom.
        let mut narrow = crate::arrange::Node::Leaf;
        narrow.split(0, Dir::Cols);
        narrow.split(0, Dir::Cols);
        let l = layout(
            Section::Features,
            &editor_state(narrow, 0, true, false),
            &Estimate,
        );
        let all = buttons_of(&l);
        assert!(!button(&all, Hit::ArrangeSplit(Dir::Cols)).2);
        assert!(button(&all, Hit::ArrangeSplit(Dir::Rows)).2);
    }

    #[test]
    fn renaming_shows_only_the_name_field() {
        let l = layout(
            Section::Features,
            &editor_state(crate::arrange::Node::Leaf, 0, true, true),
            &Estimate,
        );
        assert!(l.canvas.is_none());
        assert!(l.fields.iter().any(|(f, _)| *f == Field::TileName));
        let all = buttons_of(&l);
        assert!(
            all.iter()
                .all(|b| !matches!(b.0, Hit::ArrangeSplit(_) | Hit::ArrangeMerge))
        );
        assert!(button(&all, Hit::ArrangeEditorSave).2);
    }

    #[test]
    fn a_press_on_the_canvas_takes_a_divider_near_it_else_the_tile() {
        let canvas = Rect {
            x: 100.0,
            y: 50.0,
            w: 320.0,
            h: 180.0,
        };
        let mut root = crate::arrange::Node::Leaf;
        root.split(0, Dir::Cols);
        // The divider is at the middle: x = 100 + 160.
        assert_eq!(
            canvas_press(&root, canvas, 262.0, 100.0),
            Some(CanvasPress::Divider(vec![]))
        );
        assert_eq!(
            canvas_press(&root, canvas, 150.0, 100.0),
            Some(CanvasPress::Tile(0))
        );
        assert_eq!(
            canvas_press(&root, canvas, 400.0, 200.0),
            Some(CanvasPress::Tile(1))
        );
        assert_eq!(canvas_press(&root, canvas, 50.0, 100.0), None, "outside");
        // A divider inside the right half only spans that half.
        root.split(1, Dir::Rows);
        let mid = 50.0 + 90.0;
        assert_eq!(
            canvas_press(&root, canvas, 350.0, mid + 2.0),
            Some(CanvasPress::Divider(vec![true]))
        );
        assert_eq!(
            canvas_press(&root, canvas, 150.0, mid),
            Some(CanvasPress::Tile(0)),
            "the left half has no such divider"
        );
    }

    #[test]
    fn dragging_a_divider_snaps_and_keeps_every_tile_at_fifteen_percent() {
        let canvas = Rect {
            x: 0.0,
            y: 0.0,
            w: 320.0,
            h: 180.0,
        };
        let mut root = crate::arrange::Node::Leaf;
        root.split(0, Dir::Cols);
        // To 30 % of the width: 96 px.
        assert!(canvas_drag(&mut root, canvas, &[], 96.0, 10.0));
        assert!((root.to_rects()[0].w - 0.3).abs() < 1e-3);
        // To 31 %: snapped to 30 %.
        assert!(canvas_drag(&mut root, canvas, &[], 99.0, 10.0));
        assert!((root.to_rects()[0].w - 0.3).abs() < 1e-3);
        // Past the left edge: stops at 15 %; past the right: at 85 %.
        assert!(canvas_drag(&mut root, canvas, &[], -50.0, 10.0));
        assert!((root.to_rects()[0].w - 0.15).abs() < 1e-3);
        assert!(canvas_drag(&mut root, canvas, &[], 900.0, 10.0));
        assert!((root.to_rects()[1].w - 0.15).abs() < 1e-3);
        assert!(root.is_valid());
        assert!(
            !canvas_drag(&mut root, canvas, &[true], 10.0, 10.0),
            "no such divider"
        );
        // A rows divider follows the pointer's y.
        let mut rows = crate::arrange::Node::Leaf;
        rows.split(0, Dir::Rows);
        assert!(canvas_drag(&mut rows, canvas, &[], 10.0, 45.0));
        assert!((rows.to_rects()[0].h - 0.25).abs() < 1e-3);
    }

    #[test]
    fn studio_radius_scale() {
        assert_eq!(
            (CARD_RADIUS, CONTROL_RADIUS, SEGMENT_RADIUS),
            (14.0, 10.0, 8.0)
        );
        // Fully rounded: wider than half of any chip, button or toggle.
        assert!(FULL_RADIUS >= CHIP_H.max(BUTTON_H).max(TOGGLE_H).max(STEP_BUTTON) / 2.0);
        // The segment and its track keep the same gap all round.
        assert_eq!(CONTROL_RADIUS - SEGMENT_RADIUS, SEG_PAD);
    }

    #[test]
    fn integrations_show_the_status_as_chips() {
        let base = states()[0].clone();
        for status in [
            CalStatus::NotConfigured,
            CalStatus::NeedsLogin,
            CalStatus::Reconnect,
            CalStatus::Ok,
            CalStatus::Offline,
        ] {
            let s = State {
                google: GoogleState {
                    status,
                    ..base.google
                },
                hooks: Some(HookState {
                    complete: true,
                    any: true,
                }),
                ..base.clone()
            };
            let l = layout(Section::Integrations, &s, &Estimate);
            let (key, tone) = google_status_key(&s.google);
            let chips: Vec<(&str, Tone)> = l
                .chips
                .iter()
                .map(|c| (c.label.text.as_str(), c.label.tone))
                .collect();
            assert_eq!(
                chips,
                [
                    (text(s.lang, key), tone),
                    (text(s.lang, Key::HooksOn), Tone::Good)
                ]
            );
            // A message after a click stands in for the Google status.
            let noted = State {
                google_note: Some(("Tersimpan", Tone::Good)),
                ..s.clone()
            };
            let l = layout(Section::Integrations, &noted, &Estimate);
            assert_eq!(l.chips.len(), 1);
            assert!(l.texts.iter().any(|t| t.text == "Tersimpan"));
        }
        // Without Vale's client and without an ID, "not set up" asks for one.
        let bare = GoogleState {
            status: CalStatus::NotConfigured,
            built_in: false,
            has_id: false,
            ..base.google
        };
        assert_eq!(google_status_key(&bare).0, Key::StatusNeedsClientId);
        let with_id = GoogleState {
            has_id: true,
            ..bare
        };
        assert_eq!(google_status_key(&with_id).0, Key::StatusNotConfigured);
        let built_in = GoogleState {
            built_in: true,
            ..bare
        };
        assert_eq!(google_status_key(&built_in).0, Key::StatusNotConfigured);
        // Each card has its uppercase name above it.
        let l = layout(Section::Integrations, &base, &Estimate);
        for key in [Key::GoogleCalendar, Key::ClaudeCode] {
            let name = text(base.lang, key).to_uppercase();
            let label = l.texts.iter().find(|t| t.text == name).expect("eyebrow");
            assert_eq!(label.font, Font::Eyebrow);
            assert!(l.cards.iter().any(|c| c.y > label.rect.bottom()));
        }
    }

    #[test]
    fn battery_steps_by_five_within_range() {
        assert_eq!(battery_step(20, 1), 25);
        assert_eq!(battery_step(20, -1), 15);
        assert_eq!(battery_step(50, 1), 50);
        assert_eq!(battery_step(5, -1), 5);
        // Off-grid hand edits snap.
        assert_eq!(battery_step(22, 1), 25);
        assert_eq!(battery_step(22, -1), 15);
        assert_eq!(battery_step(90, -1), 50);
    }

    #[test]
    fn section_names() {
        assert_eq!(Section::from_name("Integrasi"), Some(Section::Integrations));
        assert_eq!(Section::from_name("about"), Some(Section::About));
        assert_eq!(Section::from_name("x"), None);
        for s in Section::ALL {
            assert_eq!(Section::ALL[s.index()], s);
        }
        // The nine sections, in the sidebar's order.
        assert_eq!(
            Section::ALL,
            [
                Section::General,
                Section::Appearance,
                Section::Cat,
                Section::Panel,
                Section::Clipboard,
                Section::Hotkeys,
                Section::Features,
                Section::Integrations,
                Section::About,
            ]
        );
        for (name, want) in [
            ("umum", Section::General),
            ("general", Section::General),
            ("tampilan", Section::Appearance),
            ("appearance", Section::Appearance),
            ("kucing", Section::Cat),
            ("cat", Section::Cat),
            ("panel", Section::Panel),
            ("clipboard", Section::Clipboard),
            ("hotkeys", Section::Hotkeys),
            ("tombol-pintas", Section::Hotkeys),
            ("fitur", Section::Features),
            ("features", Section::Features),
            // A section before v3.8: Pomodoro lives in Fitur now.
            ("pomodoro", Section::Features),
            ("integrasi", Section::Integrations),
            ("tentang", Section::About),
        ] {
            assert_eq!(Section::from_name(name), Some(want), "{name}");
        }
        // The sidebar names, in both languages.
        for (lang, names) in [
            (
                Lang::Id,
                [
                    "Umum",
                    "Tampilan",
                    "Kucing",
                    "Panel",
                    "Clipboard",
                    "Tombol pintas",
                    "Fitur",
                    "Integrasi",
                    "Tentang",
                ],
            ),
            (
                Lang::En,
                [
                    "General",
                    "Appearance",
                    "Cat",
                    "Panel",
                    "Clipboard",
                    "Hotkeys",
                    "Features",
                    "Integrations",
                    "About",
                ],
            ),
        ] {
            let got: Vec<&str> = Section::ALL.iter().map(|s| text(lang, s.key())).collect();
            assert_eq!(got, names, "{lang:?}");
        }
        // Each has its own icon; the Kucing one is the new cat outline.
        assert_eq!(Section::Cat.icon(), Icon::Cat);
        let mut icons: Vec<Icon> = Section::ALL.iter().map(|s| s.icon()).collect();
        icons.dedup();
        assert_eq!(icons.len(), 9);
    }

    #[test]
    fn kucing_has_the_preview_and_three_titled_cards() {
        for lang in [Lang::Id, Lang::En] {
            for cat_face in CatFace::ALL {
                let s = State {
                    lang,
                    robot: true,
                    cat_face,
                    ..states()[0].clone()
                };
                let l = layout(Section::Cat, &s, &Estimate);
                // The preview, then Kucing, Gerak and Sapaan.
                assert_eq!(l.cards.len(), 4);
                let preview = l.cat_preview.expect("the preview card");
                assert_eq!((preview, l.cards[0]), (l.cards[0], preview));
                assert_eq!(preview.h, CAT_PREVIEW_H);
                assert!(l.preview.is_none(), "Tampilan's own preview is not here");
                for key in [Key::GroupCat, Key::GroupMotion, Key::GroupGreeting] {
                    assert!(
                        l.texts
                            .iter()
                            .any(|t| t.text == text(lang, key).to_uppercase()),
                        "{lang:?} {key:?}"
                    );
                }
                // The chips: Diam, Senang, Kedip, Tidur, Menyapa; the face shown
                // is the selected one, and a click on one is a `CatFace` hit.
                let (track, items, selected) = l.segmented(Group::CatFace).expect("the chips");
                let faces: Vec<Hit> = items.iter().map(|i| i.hit).collect();
                let want: Vec<Hit> = CatFace::ALL.iter().map(|f| Hit::CatFace(*f)).collect();
                assert_eq!(faces, want);
                assert_eq!(items[selected].hit, Hit::CatFace(cat_face));
                let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
                let names: Vec<&str> = CatFace::ALL.iter().map(|f| text(lang, f.key())).collect();
                assert_eq!(labels, names);
                // Under the cat's tile and the notch, inside the preview card.
                assert!(
                    track.y >= preview.y + 12.0 + CAT_TILE && track.bottom() < preview.bottom()
                );
                assert!(track.x >= preview.x && track.right() <= preview.right());
                let r = items[1].rect;
                assert_eq!(
                    l.hit(r.x + r.w / 2.0, r.y + r.h / 2.0),
                    Some(Hit::CatFace(CatFace::Happy))
                );
            }
        }
        // The faces the chips show: Blink is the idle face with its eyes shut.
        assert_eq!(CatFace::Blink.expr(), (crate::ui::robot::Expr::Idle, true));
        assert_eq!(CatFace::Sleep.expr().0, crate::ui::robot::Expr::Sleep);
        assert_eq!(CatFace::Wave.expr().0, crate::ui::robot::Expr::Wave);
    }

    #[test]
    fn the_kucing_card_has_color_backdrop_and_accessory_rows() {
        for lang in [Lang::Id, Lang::En] {
            for (color, accessory) in [
                (CatColor::Terracotta, CatAccessory::None),
                (CatColor::Calico, CatAccessory::Glasses),
                (CatColor::Black, CatAccessory::Crown),
            ] {
                let s = State {
                    lang,
                    robot: true,
                    cat_color: color,
                    cat_accessory: accessory,
                    surface: Surface::Auto,
                    ..states()[0].clone()
                };
                let l = layout(Section::Cat, &s, &Estimate);
                // "Warna": four swatches, the coat's name as the description.
                let (items, selected) = l
                    .controls
                    .iter()
                    .find_map(|c| match c {
                        Control::CatSwatches {
                            items, selected, ..
                        } => Some((items.clone(), *selected)),
                        _ => None,
                    })
                    .expect("coat swatches");
                let order: Vec<CatColor> = items.iter().map(|(v, _)| *v).collect();
                assert_eq!(order, CatColor::ALL);
                assert_eq!(selected, color);
                for (v, r) in &items {
                    assert_eq!(
                        l.hit(r.x + r.w / 2.0, r.y + r.h / 2.0),
                        Some(Hit::CatColor(*v))
                    );
                }
                assert!(
                    l.texts
                        .iter()
                        .any(|t| t.text == text(lang, cat_color_key(color)))
                );
                // "Latar: <name>" with "Ubah", which goes to Tampilan: there is
                // no second surface picker on this page.
                let shown = Surface::Auto.resolve(color, true);
                let line = format!(
                    "{}: {}",
                    text(lang, Key::CatBackdrop),
                    text(lang, surface_key(shown))
                );
                assert!(l.texts.iter().any(|t| t.text == line), "{lang:?} {line}");
                let all = buttons_of(&l);
                let change = button(&all, Hit::CatBackdrop);
                assert_eq!(change.1, text(lang, Key::CatBackdropChange));
                assert!(change.2, "Ubah takes clicks while the cat is on");
                assert!(
                    !l.controls
                        .iter()
                        .any(|c| matches!(c, Control::SurfaceSwatches { .. })),
                    "the base color is picked in Tampilan only"
                );
                // "Aksesori": a tile for each of the seven, Tanpa first, the
                // chosen one marked, in one row inside the card.
                let (tiles, chosen) = l
                    .controls
                    .iter()
                    .find_map(|c| match c {
                        Control::AccessoryTiles {
                            items, selected, ..
                        } => Some((items.clone(), *selected)),
                        _ => None,
                    })
                    .expect("accessory tiles");
                let order: Vec<CatAccessory> = tiles.iter().map(|(v, _)| *v).collect();
                assert_eq!(order, CatAccessory::ALL);
                assert_eq!(order[0], CatAccessory::None);
                assert_eq!(chosen, accessory);
                let card = l.cards[1];
                for (v, r) in &tiles {
                    assert!(inside(*r, card), "{v:?} {r:?}");
                    assert_eq!(r.y, tiles[0].1.y, "one row");
                    assert_eq!(
                        l.hit(r.x + r.w / 2.0, r.y + r.h / 2.0),
                        Some(Hit::CatAccessory(*v))
                    );
                }
                for pair in tiles.windows(2) {
                    assert!(pair[0].1.right() < pair[1].1.x, "apart");
                }
                assert!(
                    l.texts
                        .iter()
                        .any(|t| t.text == text(lang, accessory_key(accessory)))
                );
                // Off: the cat's rows fade and take no clicks.
                let off = State { robot: false, ..s };
                let l = layout(Section::Cat, &off, &Estimate);
                let hits: Vec<Hit> = l.hits().into_iter().map(|(h, _)| h).collect();
                assert!(!hits.iter().any(|h| matches!(
                    h,
                    Hit::CatColor(_) | Hit::CatAccessory(_) | Hit::CatBackdrop
                )));
                assert!(hits.contains(&Hit::Robot));
            }
        }
    }

    #[test]
    fn gerak_and_sapaan_rows_follow_their_settings() {
        for lang in [Lang::Id, Lang::En] {
            for (follow, peek, behind) in [
                (true, true, false),
                (false, false, false),
                (true, false, true),
            ] {
                let s = State {
                    lang,
                    robot: true,
                    cat_follow: follow,
                    notch_peek: peek,
                    stay_behind: behind,
                    greeting: true,
                    scroll: 1e6,
                    ..states()[0].clone()
                };
                let l = layout(Section::Cat, &s, &Estimate);
                let toggle = |hit: Hit| {
                    l.controls.iter().find_map(|c| match c {
                        Control::Toggle {
                            hit: h,
                            on,
                            enabled,
                            ..
                        } if *h == hit => Some((*on, *enabled)),
                        _ => None,
                    })
                };
                assert_eq!(toggle(Hit::CatFollow), Some((follow, true)));
                // "Mengecil jadi mata" is off while "Di belakang jendela" is
                // on, and says so; it keeps its value.
                assert_eq!(toggle(Hit::NotchPeek), Some((peek, !behind)));
                let says = |key: Key| l.texts.iter().any(|t| t.text == text(lang, key));
                assert_eq!(says(Key::CatPeekBehind), behind, "{lang:?}");
                assert_eq!(says(Key::CatPeekDesc), !behind, "{lang:?}");
                assert_eq!(l.hits().iter().any(|(h, _)| *h == Hit::NotchPeek), !behind);
                // Sapaan: the toggle, then the five periods as one wrapped
                // static line under it, inside the last card.
                assert_eq!(toggle(Hit::Greeting), Some((true, true)));
                let card = *l.cards.last().unwrap();
                let periods = crate::i18n::greeting_periods_line(lang);
                let lines: Vec<&Text> = l
                    .texts
                    .iter()
                    .filter(|t| inside(t.rect, card) && t.rect.y >= card.y + ROW_H)
                    .collect();
                assert!(!lines.is_empty());
                assert_eq!(
                    lines
                        .iter()
                        .map(|t| t.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                    periods
                );
                for t in &lines {
                    assert!(Estimate.width(&t.text, t.font) <= t.rect.w, "{:?}", t.text);
                }
            }
        }
        // The five periods and their titles, built from the greeting code.
        assert_eq!(
            crate::i18n::greeting_periods_line(Lang::Id),
            "Pagi 04-10 Selamat pagi · Siang 11-14 Selamat siang · Sore 15-17 Selamat sore · Malam 18-23 Selamat malam · Dini hari 00-03 Masih begadang?"
        );
        assert_eq!(
            crate::i18n::greeting_periods_line(Lang::En),
            "Morning 04-10 Good morning · Noon 11-14 Good afternoon · Afternoon 15-17 Good afternoon · Evening 18-23 Good evening · Late night 00-03 Up late?"
        );
    }

    #[test]
    fn the_notification_card_starts_with_the_toast_style_and_everything_moves_one_row_down() {
        for lang in [Lang::Id, Lang::En] {
            for style in ToastStyle::ALL {
                let mut s = State {
                    lang,
                    ..states()[0].clone()
                };
                s.notif.toast_style = *style;
                let l = layout(Section::Features, &s, &Estimate);
                let (track, items, selected) = l
                    .segmented(Group::NotifToastStyle)
                    .expect("Notifikasi saat notch dipakai");
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].hit, Hit::NotifToastStyle(ToastStyle::Compact));
                assert_eq!(items[1].hit, Hit::NotifToastStyle(ToastStyle::Full));
                assert_eq!(items[selected].hit, Hit::NotifToastStyle(*style));
                // Its label and note are the card's first texts.
                let label = text(lang, Key::NotifToastStyle);
                let row = l
                    .texts
                    .iter()
                    .find(|t| t.text == label)
                    .expect("the row's label");
                let record = l
                    .texts
                    .iter()
                    .find(|t| t.text == text(lang, Key::NotifRecord))
                    .expect("Simpan notifikasi");
                assert!(row.rect.y < record.rect.y, "the new row is first");
                assert!((record.rect.y - row.rect.y - ROW_H).abs() < 0.5, "one row down");
                assert!(track.y > row.rect.y - ROW_H && track.bottom() < record.rect.y + ROW_H);
                // The toggle that was first is still there, one row lower.
                assert!(l.controls.iter().any(|c| matches!(
                    c,
                    Control::Toggle { hit: Hit::NotifRecord, on, .. } if *on == s.notif.record
                )));
                // A divider sits between the new row and the old first.
                assert!(l.dividers.iter().any(|d| {
                    d.y > row.rect.y && d.y < record.rect.y && d.w > 100.0
                }));
            }
        }
    }

    #[test]
    fn panel_holds_the_open_mode_stay_behind_and_the_lists() {
        for lang in [Lang::Id, Lang::En] {
            let s = State {
                lang,
                ..states()[0].clone()
            };
            let l = layout(Section::Panel, &s, &Estimate);
            for key in [
                Key::GroupBehavior,
                Key::PanelPages,
                Key::Shortcuts,
                Key::TilesAvailable,
            ] {
                assert!(
                    l.texts
                        .iter()
                        .any(|t| t.text == text(lang, key).to_uppercase()),
                    "{lang:?} {key:?}"
                );
            }
            let (_, items, selected) = l.segmented(Group::OpenMode).expect("Mode buka");
            assert_eq!(items[selected].hit, Hit::OpenMode(s.open_mode));
            assert!(l.controls.iter().any(|c| matches!(
                c,
                Control::Toggle {
                    hit: Hit::StayBehind,
                    on,
                    ..
                } if *on == s.stay_behind
            )));
            // The two lists' handles live here, not in Tampilan.
            assert!(!l.rows.is_empty());
            let l = layout(Section::Appearance, &s, &Estimate);
            assert!(l.rows.is_empty());
        }
    }

    #[test]
    fn every_card_of_every_section_has_a_title_but_the_previews() {
        // Each card sits under an uppercase eyebrow, so no card is bare; the
        // two previews (Tampilan, Kucing) and Tentang's logo are pictures.
        for lang in [Lang::Id, Lang::En] {
            for section in Section::ALL {
                let s = State {
                    lang,
                    ..states()[0].clone()
                };
                let l = layout(section, &s, &Estimate);
                let eyebrows: Vec<&Text> =
                    l.texts.iter().filter(|t| t.font == Font::Eyebrow).collect();
                for card in &l.cards {
                    if l.preview == Some(*card) || l.cat_preview == Some(*card) {
                        continue;
                    }
                    // A card's title is the eyebrow just above it, or (a list
                    // card's own sub-heading) one inside it.
                    let titled = eyebrows.iter().any(|t| {
                        t.rect.bottom() <= card.y + 0.01
                            && card.y - t.rect.bottom() <= EYEBROW_GAP + 0.01
                    });
                    assert!(titled, "{section:?} {lang:?}: bare card {card:?}");
                }
            }
        }
    }

    fn track() -> Rect {
        Rect {
            x: 100.0,
            y: 50.0,
            w: SLIDER_W,
            h: SLIDER_THUMB,
        }
    }

    #[test]
    fn slider_value_and_x_round_trip() {
        for id in [
            SliderId::NotchWidth,
            SliderId::NotchHeight,
            SliderId::NotchOpacity,
        ] {
            let (min, max) = id.range();
            for v in min..=max {
                let x = slider_x(track(), v, min, max);
                assert_eq!(slider_value(track(), x, min, max), v, "{id:?} {v}");
            }
        }
    }

    #[test]
    fn slider_ends_and_clamping() {
        let t = track();
        let (min, max) = SliderId::NotchWidth.range();
        // The thumb's centre runs half a thumb in from each end.
        assert_eq!(slider_x(t, min, min, max), t.x + SLIDER_THUMB / 2.0);
        assert_eq!(slider_x(t, max, min, max), t.right() - SLIDER_THUMB / 2.0);
        // A press past either end gives the end; the middle gives the middle.
        assert_eq!(slider_value(t, t.x - 50.0, min, max), min);
        assert_eq!(slider_value(t, t.right() + 50.0, min, max), max);
        assert_eq!(slider_value(t, t.x + t.w / 2.0, 0, 100), 50);
        // Values outside the range pin to the ends.
        assert_eq!(slider_x(t, min - 10, min, max), slider_x(t, min, min, max));
        assert_eq!(slider_x(t, max + 10, min, max), slider_x(t, max, min, max));
        // A one-px step of the pointer never moves a value by more than the
        // range allows over the track.
        assert!(slider_value(t, t.x + 60.0, min, max) <= max);
    }

    #[test]
    fn the_notch_card_sits_between_the_first_card_and_batas() {
        for lang in [Lang::Id, Lang::En] {
            let base = State {
                lang,
                notch_width: 190,
                notch_height: 34,
                notch_fill: NotchFill::Solid,
                notch_opacity: 30,
                robot: true,
                ..states()[0].clone()
            };
            let l = layout(Section::Appearance, &base, &Estimate);
            // Preview, the Island card, then Notch, then Jendela.
            let notch = l.cards[2];
            let (window, _, _) = l.segmented(Group::Window).expect("Mode jendela");
            assert!(notch.bottom() < window.y && l.cards[1].bottom() < notch.y);
            assert_eq!(l.cards.len(), 4);
            assert_eq!(notch.h, 5.0 * ROW_H);
            // Both sliders and the opacity one, in the card, with their labels.
            let sliders: Vec<(SliderId, Rect, i32, bool)> = l
                .controls
                .iter()
                .filter_map(|c| match c {
                    Control::Slider {
                        id,
                        track,
                        value,
                        enabled,
                        ..
                    } => Some((*id, *track, *value, *enabled)),
                    _ => None,
                })
                .collect();
            let ids: Vec<SliderId> = sliders.iter().map(|s| s.0).collect();
            assert_eq!(
                ids,
                [
                    SliderId::NotchWidth,
                    SliderId::NotchHeight,
                    SliderId::NotchOpacity
                ]
            );
            for (_, track, _, _) in &sliders {
                assert!(track.y >= notch.y && track.bottom() <= notch.bottom());
                assert!(track.right() < notch.right());
            }
            assert_eq!(
                sliders.iter().map(|s| s.2).collect::<Vec<_>>(),
                [190, 34, 30]
            );
            // The opacity slider is off while the background is solid, and
            // takes no press.
            assert!(!sliders[2].3);
            let hits = l.hits();
            let has = |h: Hit| hits.iter().any(|(x, _)| *x == h);
            assert!(!has(Hit::Slider(SliderId::NotchOpacity)));
            // At the default size Reset is off.
            let all = buttons_of(&l);
            assert!(!button(&all, Hit::NotchReset).2);
            assert_eq!(button(&all, Hit::NotchReset).1, text(lang, Key::NotchReset));
            // The labels read in both languages.
            let labels: Vec<&str> = l.texts.iter().map(|t| t.text.as_str()).collect();
            for key in [
                Key::NotchWidth,
                Key::NotchHeight,
                Key::NotchDefaultSize,
                Key::NotchFill,
                Key::NotchFillDesc,
                Key::NotchOpacity,
            ] {
                assert!(labels.contains(&text(lang, key)), "{lang:?} {key:?}");
            }
            assert!(labels.contains(&"190 px") && labels.contains(&"34 px"));
            assert!(labels.contains(&"30 %"));

            // Transparent with another size: the opacity slider takes
            // presses, Reset too, and the values show.
            let on = State {
                notch_width: 240,
                notch_height: 40,
                notch_fill: NotchFill::Transparent,
                notch_opacity: 0,
                ..base.clone()
            };
            // The opacity row is below the first screenful: scroll to it
            // (the Notch card's top goes just under the title).
            let flat = layout(Section::Appearance, &on, &Estimate);
            let at = State {
                scroll: flat.cards[2].y - VIEW_TOP - 8.0,
                ..on.clone()
            };
            let l = layout(Section::Appearance, &at, &Estimate);
            let controls: Vec<(Hit, bool)> = l
                .controls
                .iter()
                .filter_map(|c| match c {
                    Control::Slider { id, enabled, .. } => Some((Hit::Slider(*id), *enabled)),
                    _ => None,
                })
                .collect();
            assert!(controls.iter().all(|c| matches!(c.0, Hit::Slider(_))));
            assert!(controls.contains(&(Hit::Slider(SliderId::NotchOpacity), true)));
            let all = buttons_of(&l);
            assert!(button(&all, Hit::NotchReset).2);
        }
    }

    #[test]
    fn the_background_choice_follows_the_setting() {
        for fill in NotchFill::ALL {
            let s = State {
                notch_fill: *fill,
                ..states()[0].clone()
            };
            let l = layout(Section::Appearance, &s, &Estimate);
            let selected = l
                .controls
                .iter()
                .find_map(|c| match c {
                    Control::Segmented {
                        group: Group::NotchFill,
                        items,
                        selected,
                        ..
                    } => Some((items.iter().map(|i| i.hit).collect::<Vec<_>>(), *selected)),
                    _ => None,
                })
                .expect("the background segments");
            assert_eq!(
                selected.0,
                [
                    Hit::NotchFill(NotchFill::Solid),
                    Hit::NotchFill(NotchFill::Transparent)
                ]
            );
            assert_eq!(NotchFill::ALL[selected.1], *fill);
        }
    }

    #[test]
    fn labels_of_the_notch_card_differ_by_language() {
        for key in [
            Key::NotchWidth,
            Key::NotchHeight,
            Key::NotchFill,
            Key::NotchFillTransparent,
            Key::NotchOpacity,
        ] {
            assert_ne!(text(Lang::Id, key), text(Lang::En, key), "{key:?}");
        }
        // "Isi notch": Solid | Tembus.
        assert_eq!(text(Lang::Id, Key::NotchFill), "Isi notch");
        assert_eq!(text(Lang::Id, Key::NotchFillSolid), "Solid");
        assert_eq!(text(Lang::Id, Key::NotchFillTransparent), "Tembus");
        assert_eq!(text(Lang::En, Key::NotchFill), "Notch fill");
        assert_eq!(text(Lang::En, Key::NotchFillTransparent), "See-through");
        assert_eq!(text(Lang::Id, Key::NotchOpacity), "Opasitas");
    }

    #[test]
    fn the_widest_island_fits_the_preview_screen() {
        use crate::config::Style;
        use crate::ui::geometry::{NotchSize, ShapeKind, shape_with_robot_sized};
        // The preview's "screen" is the card less 12 px each side
        // (`preview` in settings_controls.rs); a see-through island adds a
        // checker strip 24 px wider than it on each side.
        let screen_w = CONTENT_W - 24.0;
        for style in [Style::Pill, Style::Notch] {
            for kind in [ShapeKind::MediaCompact, ShapeKind::Idle] {
                let widest = NotchSize::from_px(300, 44);
                let w = shape_with_robot_sized(style, widest, kind, true).w;
                assert!(w + 48.0 <= screen_w, "{style:?} {kind:?} {w} in {screen_w}");
            }
        }
    }
}
