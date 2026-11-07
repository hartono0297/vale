//! The Studio icon set: 24-unit vector icons with a 1.5 px round stroke, an
//! optional duotone accent fill and solid ink dots. The shapes are copied
//! verbatim from `icon()` in `docs/design/studio-ui.html`.
//!
//! The data and the path parser are plain Rust; `IconSet` turns them into
//! Direct2D geometries once per factory and draws them at any size.

use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_F, D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_CAP_STYLE_ROUND,
    D2D1_DASH_STYLE_SOLID, D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, ID2D1Factory,
    ID2D1GeometrySink, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush,
    ID2D1StrokeStyle,
};
use windows::core::{Error, Result};
use windows_numerics::{Matrix3x2, Vector2};

use super::geometry::Rect;

/// The design grid every icon is drawn on.
pub const GRID: f32 = 24.0;
/// Stroke width in DIPs, whatever size the icon is drawn at.
pub const STROKE_W: f32 = 1.5;
/// Opacity of the duotone accent fill.
pub const DUO_ALPHA: f32 = 0.38;
/// Radius of a solid ink dot.
const DOT_R: f32 = 1.1;
/// Default corner radius of a rect (`ST.r` in the design).
const RECT_R: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Camera,
    Clipboard,
    Emoji,
    Folder,
    Calc,
    Activity,
    Gear,
    Lock,
    Calendar,
    Timer,
    Bell,
    Note,
    Music,
    Terminal,
    Grid,
    Palette,
    Link,
    Info,
    Play,
    Pause,
    Reset,
    Skip,
    Prev,
    Next,
    Plus,
    ChevL,
    ChevR,
    Check,
    Ring,
    Repeat,
    Trash,
    Quote,
    Battery,
    Sparkle,
    Mail,
    Ext,
    X,
    Min,
    Max,
    Clock,
    /// The editors' date chip: a down chevron (`down` in editor-acara.html).
    ChevD,
    /// The CLI page's enlarge and shrink buttons (`expand`, `shrink` in
    /// pengingat-cli.html).
    Expand,
    Shrink,
    /// The CLI folder menu's favourites (`star` in keputusan-v3.1.html):
    /// outline to star a folder, with the duotone fill once it is one.
    Star,
    /// The cleaner's rows (the `I` map in deepclean-opsi-d.html: `temp`,
    /// `globe`, `box`, `code`, `image`, `dl`, `win`). Line only, as there.
    TempFile,
    Globe,
    Package,
    Code,
    Image,
    Download,
    Windows,
    /// The Clipboard page (`search`, `pin`, `text`, `color` in
    /// keputusan-v3.1.html; `code` and `image` are the cleaner's above):
    /// line only, no duotone.
    Search,
    Pin,
    Text,
    Swatch,
    /// The screenshot preview's "Salin lagi" (`copy` in keputusan-v3.1.html;
    /// its "Simpan" is `save`, the same path as `Download`). Line only.
    Copy,
    /// The Mixer rows' speaker, and its muted twin. Line only.
    Volume,
    VolumeMute,
    /// The panel header's Settings button (two sliders), so it is not the
    /// Windows Settings gear. Line only.
    Sliders,
    /// The System page's tile (a chip with pins). Line only.
    Chip,
    /// The drag handle of the Settings lists (six dots).
    Grip,
    /// The up chevron of the Settings lists' order buttons.
    ChevU,
    /// The Devices page (`usb`, `hdd`, `sd`, `eject` in rencana-v3.5.html,
    /// page 03): a flash drive, an external disk, a memory card and the eject
    /// triangle. Line only; the design's zero-length "dots" are `Dot`s.
    Usb,
    Hdd,
    Sd,
    Eject,
    /// The quick search's back button: an arrow to the left. Line only.
    Back,
    /// The Kucing section and the base-colour swatch that follows the cat: the
    /// cat head as an outline (the notch cat's dome, two ears, two eyes).
    Cat,
    /// The Notifikasi page: an inbox tray, an open-top box whose front edge
    /// dips in the middle. Duotone like the Bell, which it must not be mistaken for.
    Inbox,
    /// The Susun page: a rounded screen frame split by a vertical and a
    /// horizontal line into a 2x2 layout (Pintasan's `Grid` is four tiles).
    Layout,
    /// The Tombol pintas section: a keyboard, a rounded body with a row of
    /// key dots and a space bar.
    Keyboard,
    /// The phone link: a phone, rounded body with its home bar. Duotone.
    Phone,
    /// A video file on the shelf and in the phone's receiving rows: a
    /// camcorder, a rounded body with a lens triangle on its right. Duotone.
    Video,
    /// The Widget section: four rounded cards of different sizes.
    Widgets,
    /// The sticky note stamps' glyphs (line only): a plain check, an
    /// exclamation mark, a heart and a cup with steam.
    Tick,
    Mark,
    Heart,
    Cup,
    /// A sticky paper with a folded corner: the Notes card's desktop toggle.
    StickyNote,
}

impl Icon {
    /// Every icon, in declaration order (`ALL[i] as usize == i`).
    pub const ALL: [Icon; 79] = [
        Icon::Camera,
        Icon::Clipboard,
        Icon::Emoji,
        Icon::Folder,
        Icon::Calc,
        Icon::Activity,
        Icon::Gear,
        Icon::Lock,
        Icon::Calendar,
        Icon::Timer,
        Icon::Bell,
        Icon::Note,
        Icon::Music,
        Icon::Terminal,
        Icon::Grid,
        Icon::Palette,
        Icon::Link,
        Icon::Info,
        Icon::Play,
        Icon::Pause,
        Icon::Reset,
        Icon::Skip,
        Icon::Prev,
        Icon::Next,
        Icon::Plus,
        Icon::ChevL,
        Icon::ChevR,
        Icon::Check,
        Icon::Ring,
        Icon::Repeat,
        Icon::Trash,
        Icon::Quote,
        Icon::Battery,
        Icon::Sparkle,
        Icon::Mail,
        Icon::Ext,
        Icon::X,
        Icon::Min,
        Icon::Max,
        Icon::Clock,
        Icon::ChevD,
        Icon::Expand,
        Icon::Shrink,
        Icon::Star,
        Icon::TempFile,
        Icon::Globe,
        Icon::Package,
        Icon::Code,
        Icon::Image,
        Icon::Download,
        Icon::Windows,
        Icon::Search,
        Icon::Pin,
        Icon::Text,
        Icon::Swatch,
        Icon::Copy,
        Icon::Volume,
        Icon::VolumeMute,
        Icon::Sliders,
        Icon::Chip,
        Icon::Grip,
        Icon::ChevU,
        Icon::Usb,
        Icon::Hdd,
        Icon::Sd,
        Icon::Eject,
        Icon::Back,
        Icon::Cat,
        Icon::Inbox,
        Icon::Layout,
        Icon::Keyboard,
        Icon::Phone,
        Icon::Video,
        Icon::Widgets,
        Icon::Tick,
        Icon::Mark,
        Icon::Heart,
        Icon::Cup,
        Icon::StickyNote,
    ];

    /// The icon's primitives on the 24-unit grid, in drawing order.
    pub fn parts(self) -> &'static [Part] {
        match self {
            Icon::Camera => &[
                Part::Fill("M3 9h4l1.6-2.5h6.8L17 9h4v10H3z"),
                Part::Path(
                    "M4 8.5h3l1.5-2.2h7L17 8.5h3a1 1 0 0 1 1 1v8.5a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9.5a1 1 0 0 1 1-1z",
                ),
                Part::Circle(12.0, 13.5, 3.3),
            ],
            Icon::Clipboard => &[
                Part::FillRect(5.0, 5.0, 14.0, 16.0, RECT_R),
                Part::Rect(5.0, 5.0, 14.0, 16.0, RECT_R),
                Part::Rect(9.0, 3.0, 6.0, 4.0, 1.5),
                Part::Path("M9 12h6M9 16h4"),
            ],
            Icon::Emoji => &[
                Part::FillCircle(12.0, 12.0, 9.0),
                Part::Circle(12.0, 12.0, 9.0),
                Part::Path("M8.5 14.5c1.8 2 5.2 2 7 0"),
                Part::Dot(9.0, 10.0),
                Part::Dot(15.0, 10.0),
            ],
            Icon::Folder => &[
                Part::Fill("M3 7h6l2 2h10v10H3z"),
                Part::Path(
                    "M3 7a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z",
                ),
                Part::Path("M3 11h18"),
            ],
            Icon::Calc => &[
                Part::FillRect(5.0, 3.0, 14.0, 18.0, RECT_R),
                Part::Rect(5.0, 3.0, 14.0, 18.0, RECT_R),
                Part::Rect(8.0, 6.0, 8.0, 4.0, 1.0),
                Part::Dot(8.5, 14.0),
                Part::Dot(12.0, 14.0),
                Part::Dot(15.5, 14.0),
                Part::Dot(8.5, 17.5),
                Part::Dot(12.0, 17.5),
                Part::Dot(15.5, 17.5),
            ],
            Icon::Activity => &[
                Part::FillRect(3.0, 4.0, 18.0, 16.0, RECT_R),
                Part::Rect(3.0, 4.0, 18.0, 16.0, RECT_R),
                Part::Path("M6 13h3l2-4 2 7 2-3h3"),
            ],
            Icon::Gear => &[
                Part::FillCircle(12.0, 12.0, 6.5),
                Part::Circle(12.0, 12.0, 6.5),
                Part::Circle(12.0, 12.0, 2.6),
                Part::Path(
                    "M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1",
                ),
            ],
            Icon::Lock => &[
                Part::FillRect(5.0, 11.0, 14.0, 10.0, RECT_R),
                Part::Rect(5.0, 11.0, 14.0, 10.0, RECT_R),
                Part::Path("M8 11V8a4 4 0 0 1 8 0v3"),
                Part::Dot(12.0, 16.0),
            ],
            Icon::Calendar => &[
                Part::FillRect(3.0, 5.0, 18.0, 16.0, RECT_R),
                Part::Rect(3.0, 5.0, 18.0, 16.0, RECT_R),
                Part::Path("M3 10h18M8 3v4M16 3v4"),
                Part::Dot(8.0, 14.0),
                Part::Dot(12.0, 14.0),
                Part::Dot(16.0, 14.0),
            ],
            Icon::Timer => &[
                Part::FillCircle(12.0, 13.0, 8.0),
                Part::Circle(12.0, 13.0, 8.0),
                Part::Path("M12 13V9M10 2.5h4M18.5 6l1.2-1.2"),
            ],
            Icon::Bell => &[
                Part::Fill("M6 16v-5a6 6 0 0 1 12 0v5l1.5 2h-15z"),
                Part::Path("M6 16v-5a6 6 0 0 1 12 0v5l1.5 2h-15z"),
                Part::Path("M10 21h4"),
            ],
            Icon::Note => &[
                Part::Fill("M5 3.5h10l4 4V20.5H5z"),
                Part::Path("M5 3.5h10l4 4V20.5H5z"),
                Part::Path("M15 3.5v4h4M8 12h8M8 16h5"),
            ],
            Icon::Music => &[
                Part::FillCircle(6.5, 18.0, 2.5),
                Part::Path("M9 18V6l11-2v12"),
                Part::Circle(6.5, 18.0, 2.5),
                Part::Circle(17.5, 16.0, 2.5),
                Part::Path("M9 9.5l11-2"),
            ],
            Icon::Terminal => &[
                Part::FillRect(3.0, 5.0, 18.0, 14.0, RECT_R),
                Part::Rect(3.0, 5.0, 18.0, 14.0, RECT_R),
                Part::Path("M7 10l3 2.5L7 15M12.5 15H17"),
            ],
            Icon::Grid => &[
                Part::FillRect(4.0, 4.0, 7.0, 7.0, 2.0),
                Part::Rect(4.0, 4.0, 7.0, 7.0, 2.0),
                Part::Rect(13.0, 4.0, 7.0, 7.0, 2.0),
                Part::Rect(4.0, 13.0, 7.0, 7.0, 2.0),
                Part::Rect(13.0, 13.0, 7.0, 7.0, 2.0),
            ],
            Icon::Palette => &[
                Part::FillCircle(12.0, 12.0, 9.0),
                Part::Path(
                    "M12 3a9 9 0 0 0 0 18c1.3 0 1.8-1 1.4-2-.5-1.2.3-2.4 1.6-2.4H17a4 4 0 0 0 4-4c0-5.2-4-9.6-9-9.6z",
                ),
                Part::Dot(7.5, 11.0),
                Part::Dot(10.0, 7.5),
                Part::Dot(14.5, 7.5),
            ],
            Icon::Link => &[Part::Path(
                "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1.2 1.2M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1.2-1.2",
            )],
            Icon::Info => &[
                Part::FillCircle(12.0, 12.0, 9.0),
                Part::Circle(12.0, 12.0, 9.0),
                Part::Path("M12 11v6"),
                Part::Dot(12.0, 7.8),
            ],
            Icon::Play => &[Part::Path("M8 5.5v13l10.5-6.5z")],
            Icon::Pause => &[
                Part::Rect(6.5, 5.0, 4.0, 14.0, 1.2),
                Part::Rect(13.5, 5.0, 4.0, 14.0, 1.2),
            ],
            Icon::Reset => &[Part::Path("M4.5 12a7.5 7.5 0 1 0 2.2-5.3M4.5 4.5v3.7h3.7")],
            Icon::Skip => &[Part::Path("M5.5 6v12l9-6z"), Part::Path("M18.5 6v12")],
            Icon::Prev => &[Part::Path("M5.5 6v12"), Part::Path("M18.5 6v12l-9-6z")],
            Icon::Next => &[Part::Path("M18.5 6v12"), Part::Path("M5.5 6v12l9-6z")],
            Icon::Plus => &[Part::Path("M12 5v14M5 12h14")],
            Icon::ChevL => &[Part::Path("M14.5 6l-6 6 6 6")],
            Icon::ChevR => &[Part::Path("M9.5 6l6 6-6 6")],
            Icon::Check => &[
                Part::FillCircle(12.0, 12.0, 8.5),
                Part::Circle(12.0, 12.0, 8.5),
                Part::Path("M8.3 12.3l2.6 2.6L15.8 10"),
            ],
            Icon::Ring => &[Part::Circle(12.0, 12.0, 8.5)],
            Icon::Repeat => &[Part::Path(
                "M17 3.5l3 3-3 3M4 11.5V10a3.5 3.5 0 0 1 3.5-3.5H20M7 20.5l-3-3 3-3M20 12.5V14a3.5 3.5 0 0 1-3.5 3.5H4",
            )],
            Icon::Trash => &[
                Part::Fill("M6.5 7l1 13h9l1-13z"),
                Part::Path("M4.5 7h15M9.5 7V4.5h5V7"),
                Part::Path("M6.5 7l1 13h9l1-13z"),
            ],
            Icon::Quote => &[
                Part::Path("M5 18c3-1 4.5-3.5 4.5-7V7H5v5h4.5"),
                Part::Path("M14.5 18c3-1 4.5-3.5 4.5-7V7h-4.5v5H19"),
            ],
            Icon::Battery => &[
                Part::FillRect(3.0, 7.5, 16.0, 9.0, 2.5),
                Part::Rect(3.0, 7.5, 16.0, 9.0, 2.5),
                Part::Path("M21.5 10.5v3M6.5 10.5v3"),
            ],
            Icon::Sparkle => &[
                Part::Fill("M12 3l2.2 6.8L21 12l-6.8 2.2L12 21l-2.2-6.8L3 12l6.8-2.2z"),
                Part::Path("M12 3l2.2 6.8L21 12l-6.8 2.2L12 21l-2.2-6.8L3 12l6.8-2.2z"),
            ],
            Icon::Mail => &[
                Part::FillRect(3.0, 5.5, 18.0, 13.0, RECT_R),
                Part::Rect(3.0, 5.5, 18.0, 13.0, RECT_R),
                Part::Path("M3.5 7l8.5 6 8.5-6"),
            ],
            Icon::Ext => &[Part::Path(
                "M14 4.5h5.5V10M19.5 4.5L11 13M17 14v4.5a1 1 0 0 1-1 1H5.5a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1H10",
            )],
            Icon::X => &[Part::Path("M6 6l12 12M18 6L6 18")],
            Icon::Min => &[Part::Path("M5 12h14")],
            Icon::Max => &[Part::Rect(5.5, 5.5, 13.0, 13.0, 1.0)],
            Icon::Clock => &[
                Part::FillCircle(12.0, 12.0, 9.0),
                Part::Circle(12.0, 12.0, 9.0),
                Part::Path("M12 7.5V12l3 2"),
            ],
            Icon::ChevD => &[Part::Path("M6 9l6 6 6-6")],
            Icon::Expand => &[Part::Path("M8 4H4v4M16 4h4v4M8 20H4v-4M16 20h4v-4")],
            Icon::Shrink => &[Part::Path("M4 8h4V4M20 8h-4V4M4 16h4v4M20 16h-4v4")],
            Icon::Star => &[
                Part::Fill("M12 4l2.4 5 5.4.6-4 3.7 1.1 5.3L12 16l-4.9 2.6 1.1-5.3-4-3.7 5.4-.6z"),
                Part::Path("M12 4l2.4 5 5.4.6-4 3.7 1.1 5.3L12 16l-4.9 2.6 1.1-5.3-4-3.7 5.4-.6z"),
            ],
            Icon::TempFile => &[Part::Path("M7 4h7l4 4v12H7z"), Part::Path("M14 4v4h4")],
            Icon::Globe => &[
                Part::Circle(12.0, 12.0, 8.5),
                Part::Path("M3.5 12h17M12 3.5c3 3.3 3 13.7 0 17M12 3.5c-3 3.3-3 13.7 0 17"),
            ],
            Icon::Package => &[
                Part::Path("M4 8l8-4 8 4v8l-8 4-8-4z"),
                Part::Path("M4 8l8 4 8-4M12 12v8"),
            ],
            Icon::Code => &[Part::Path("M9 8l-4 4 4 4M15 8l4 4-4 4")],
            Icon::Image => &[
                Part::Rect(3.0, 5.0, 18.0, 14.0, 3.0),
                Part::Circle(9.0, 10.0, 1.6),
                Part::Path("M21 16l-5-5-8 8"),
            ],
            Icon::Download => &[Part::Path("M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19h14")],
            Icon::Windows => &[
                Part::Rect(4.0, 4.0, 7.0, 7.0, 1.0),
                Part::Rect(13.0, 4.0, 7.0, 7.0, 1.0),
                Part::Rect(4.0, 13.0, 7.0, 7.0, 1.0),
                Part::Rect(13.0, 13.0, 7.0, 7.0, 1.0),
            ],
            Icon::Search => &[Part::Circle(11.0, 11.0, 6.0), Part::Path("M16 16l4 4")],
            Icon::Pin => &[Part::Path("M9 4h6l-1 6 3 3H7l3-3zM12 13v7")],
            Icon::Text => &[Part::Path("M5 6h14M5 11h14M5 16h9")],
            Icon::Swatch => &[Part::Circle(12.0, 12.0, 8.5), Part::Path("M12 3.5v17")],
            Icon::Volume => &[
                Part::Path("M4 9.5h3.5L12 6v12l-4.5-3.5H4z"),
                Part::Path("M15.5 9.2a4 4 0 0 1 0 5.6M18.2 6.6a8 8 0 0 1 0 10.8"),
            ],
            Icon::VolumeMute => &[
                Part::Path("M4 9.5h3.5L12 6v12l-4.5-3.5H4z"),
                Part::Path("M16 9.5l5 5M21 9.5l-5 5"),
            ],
            Icon::Copy => &[
                Part::Rect(8.0, 8.0, 11.0, 11.0, 2.5),
                Part::Path("M16 8V6a1 1 0 0 0-1-1H6a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h2"),
            ],
            Icon::Sliders => &[
                Part::Path("M4 7h8M18 7h2M4 17h2M12 17h8"),
                Part::Circle(15.0, 7.0, 2.6),
                Part::Circle(9.0, 17.0, 2.6),
            ],
            Icon::Chip => &[
                Part::Rect(7.0, 7.0, 10.0, 10.0, 2.0),
                Part::Path(
                    "M10 3.5V7M14 3.5V7M10 17v3.5M14 17v3.5M3.5 10H7M3.5 14H7M17 10h3.5M17 14h3.5",
                ),
            ],
            Icon::Grip => &[
                Part::Dot(9.0, 6.0),
                Part::Dot(15.0, 6.0),
                Part::Dot(9.0, 12.0),
                Part::Dot(15.0, 12.0),
                Part::Dot(9.0, 18.0),
                Part::Dot(15.0, 18.0),
            ],
            Icon::ChevU => &[Part::Path("M6 15l6-6 6 6")],
            Icon::Usb => &[
                Part::Path("M9 3h6v5H9zM7 8h10v8a5 5 0 0 1-10 0z"),
                Part::Dot(11.0, 5.0),
                Part::Dot(13.0, 5.0),
            ],
            Icon::Hdd => &[
                Part::Path("M4 6h16v12H4zM4 13h16"),
                Part::Dot(16.5, 15.5),
                Part::Dot(13.5, 15.5),
            ],
            Icon::Sd => &[Part::Path("M7 3h8l3 3v15H7zM10 3v3M12.5 3v3M15 3v2")],
            Icon::Eject => &[Part::Path("M12 5l7 8H5zM5 17h14")],
            Icon::Back => &[Part::Path("M19 12H5M11 6l-6 6 6 6")],
            // The cat head: a flat base, a dome between two pointed ears, two eyes.
            Icon::Cat => &[
                Part::Fill("M4.5 19.5v-8.2L5.8 4.5l4.3 3.2a9 9 0 0 1 3.8 0l4.3-3.2 1.3 6.8v8.2z"),
                Part::Path("M4.5 19.5v-8.2L5.8 4.5l4.3 3.2a9 9 0 0 1 3.8 0l4.3-3.2 1.3 6.8v8.2z"),
                Part::Dot(9.0, 13.5),
                Part::Dot(15.0, 13.5),
            ],
            // The tray: two raised sides, a body, and a front edge that dips
            // in the middle.
            Icon::Inbox => &[
                Part::Fill(
                    "M3 13l3.2-7.5h11.6L21 13v5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18z",
                ),
                Part::Path(
                    "M3 13l3.2-7.5h11.6L21 13v5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18z",
                ),
                Part::Path("M3 13h4.5l1.2 2.5h6.6l1.2-2.5H21"),
            ],
            Icon::Layout => &[
                Part::FillRect(3.0, 4.5, 18.0, 15.0, 3.0),
                Part::Rect(3.0, 4.5, 18.0, 15.0, 3.0),
                Part::Path("M12 4.5v15M3 12h18"),
            ],
            Icon::Keyboard => &[
                Part::FillRect(3.0, 6.0, 18.0, 12.0, 3.0),
                Part::Rect(3.0, 6.0, 18.0, 12.0, 3.0),
                Part::Dot(7.0, 10.0),
                Part::Dot(10.3, 10.0),
                Part::Dot(13.7, 10.0),
                Part::Dot(17.0, 10.0),
                Part::Path("M8 14.2h8"),
            ],
            Icon::Phone => &[
                Part::FillRect(6.0, 2.0, 12.0, 20.0, 3.0),
                Part::Rect(6.0, 2.0, 12.0, 20.0, 3.0),
                Part::Path("M11 18h2"),
            ],
            Icon::Video => &[
                Part::FillRect(3.0, 6.0, 12.0, 12.0, 3.0),
                Part::Rect(3.0, 6.0, 12.0, 12.0, 3.0),
                Part::Path("M15 10.5l5.5-3v9L15 13.5"),
            ],
            // Four rounded cards of different sizes: a tall one and a small
            // one on top, a short one and a tall one below.
            Icon::Tick => &[Part::Path("M5.5 12.5l4.5 4.5 8.5-9.5")],
            Icon::Mark => &[Part::Path("M12 5v9.5"), Part::Dot(12.0, 19.0)],
            Icon::Heart => &[Part::Path(
                "M12 19.5C12 19.5 4.5 15 4.5 9.6C4.5 7.2 6.3 5.6 8.3 5.6C9.9 5.6 11.4 6.5 12 8C12.6 6.5 14.1 5.6 15.7 5.6C17.7 5.6 19.5 7.2 19.5 9.6C19.5 15 12 19.5 12 19.5Z",
            )],
            Icon::Cup => &[
                Part::Path("M5 10h11v4a5 5 0 0 1-5 5h-1a5 5 0 0 1-5-5z"),
                Part::Path("M16 11.5h1.4a2.4 2.4 0 0 1 0 4.8H16"),
                Part::Path("M8.5 3.5v3M12 3.5v3"),
            ],
            Icon::StickyNote => &[
                Part::Fill("M5 4h14v10l-6 6H5z"),
                Part::Path("M5 4h14v10l-6 6H5z"),
                Part::Path("M13 20v-6h6"),
            ],
            Icon::Widgets => &[
                Part::FillRect(3.0, 3.0, 8.0, 11.0, 2.0),
                Part::Rect(3.0, 3.0, 8.0, 11.0, 2.0),
                Part::Rect(14.0, 3.0, 7.0, 6.0, 2.0),
                Part::Rect(3.0, 17.0, 8.0, 4.0, 1.6),
                Part::FillRect(14.0, 12.0, 7.0, 9.0, 2.0),
                Part::Rect(14.0, 12.0, 7.0, 9.0, 2.0),
            ],
        }
    }
}

/// One primitive of an icon. Names follow the design's helpers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Part {
    /// `F(d)`: a duotone fill path.
    Fill(&'static str),
    /// `RF`: a duotone fill rect (x, y, w, h, rx).
    FillRect(f32, f32, f32, f32, f32),
    /// `CF`: a duotone fill circle (cx, cy, r).
    FillCircle(f32, f32, f32),
    /// `R`: a stroked rect (x, y, w, h, rx).
    Rect(f32, f32, f32, f32, f32),
    /// `C`: a stroked circle (cx, cy, r).
    Circle(f32, f32, f32),
    /// `D`: a solid ink dot of radius 1.1.
    Dot(f32, f32),
    /// `P(d)`: a stroked path.
    Path(&'static str),
}

/// Which pass draws a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Accent fill under the strokes, only with duotone on.
    Duo,
    /// Ink outline.
    Stroke,
    /// Solid ink fill on top.
    Dots,
}

impl Part {
    pub fn layer(self) -> Layer {
        match self {
            Part::Fill(_) | Part::FillRect(..) | Part::FillCircle(..) => Layer::Duo,
            Part::Rect(..) | Part::Circle(..) | Part::Path(_) => Layer::Stroke,
            Part::Dot(..) => Layer::Dots,
        }
    }

    /// The part as absolute path segments. Rects and circles are closed.
    pub fn outline(self) -> std::result::Result<Vec<Seg>, PathError> {
        Ok(match self {
            Part::Fill(d) | Part::Path(d) => parse_path(d)?,
            Part::FillRect(x, y, w, h, r) | Part::Rect(x, y, w, h, r) => {
                rect_outline(x, y, w, h, r)
            }
            Part::FillCircle(cx, cy, r) | Part::Circle(cx, cy, r) => circle_outline(cx, cy, r),
            Part::Dot(x, y) => circle_outline(x, y, DOT_R),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

const fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

/// One absolute path segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Move(Pt),
    Line(Pt),
    /// Two control points, then the end point.
    Cubic(Pt, Pt, Pt),
    /// SVG elliptical arc: `sweep` true is the positive (clockwise on screen) direction.
    Arc {
        to: Pt,
        rx: f32,
        ry: f32,
        rotation: f32,
        large: bool,
        sweep: bool,
    },
    Close,
}

/// Line segments and quarter arcs; the radius is clamped as SVG does.
fn rect_outline(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Seg> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let (right, bottom) = (x + w, y + h);
    let corner = |to: Pt| Seg::Arc {
        to,
        rx: r,
        ry: r,
        rotation: 0.0,
        large: false,
        sweep: true,
    };
    let mut segs = vec![Seg::Move(pt(x + r, y)), Seg::Line(pt(right - r, y))];
    let turn = |segs: &mut Vec<Seg>, to: Pt| {
        if r > 0.0 {
            segs.push(corner(to));
        }
    };
    turn(&mut segs, pt(right, y + r));
    segs.push(Seg::Line(pt(right, bottom - r)));
    turn(&mut segs, pt(right - r, bottom));
    segs.push(Seg::Line(pt(x + r, bottom)));
    turn(&mut segs, pt(x, bottom - r));
    segs.push(Seg::Line(pt(x, y + r)));
    turn(&mut segs, pt(x + r, y));
    segs.push(Seg::Close);
    segs
}

/// Two half arcs, closed.
fn circle_outline(cx: f32, cy: f32, r: f32) -> Vec<Seg> {
    let half = |to: Pt| Seg::Arc {
        to,
        rx: r,
        ry: r,
        rotation: 0.0,
        large: false,
        sweep: true,
    };
    vec![
        Seg::Move(pt(cx + r, cy)),
        half(pt(cx - r, cy)),
        half(pt(cx + r, cy)),
        Seg::Close,
    ]
}

/// Where and why path data failed to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathError {
    /// Byte offset into the path data.
    pub at: usize,
    pub what: &'static str,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.what, self.at)
    }
}

/// Reads SVG path data with M, L, H, V, C, A and Z (absolute and relative),
/// implicit repetition and compact numbers (`1.8-1`, `.5`, `-.5`).
pub fn parse_path(d: &str) -> std::result::Result<Vec<Seg>, PathError> {
    let mut s = Scanner {
        b: d.as_bytes(),
        i: 0,
    };
    let mut segs = Vec::new();
    let mut cur = pt(0.0, 0.0);
    let mut start = cur;
    let mut cmd: Option<u8> = None;
    loop {
        s.skip_separators();
        let Some(c) = s.peek() else { break };
        let explicit = c.is_ascii_alphabetic();
        if explicit {
            s.i += 1;
            cmd = Some(c);
        }
        let Some(c) = cmd else {
            return Err(s.error("path data must start with a command"));
        };
        if !explicit && matches!(c, b'Z' | b'z') {
            return Err(s.error("unexpected number after Z"));
        }
        let rel = c.is_ascii_lowercase();
        let base = if rel { cur } else { pt(0.0, 0.0) };
        match c.to_ascii_uppercase() {
            b'M' => {
                let p = s.point(base)?;
                segs.push(Seg::Move(p));
                cur = p;
                start = p;
                // Further pairs are line-tos.
                cmd = Some(if rel { b'l' } else { b'L' });
            }
            b'L' => {
                let p = s.point(base)?;
                segs.push(Seg::Line(p));
                cur = p;
            }
            b'H' => {
                let x = s.number()? + base.x;
                cur = pt(x, cur.y);
                segs.push(Seg::Line(cur));
            }
            b'V' => {
                let y = s.number()? + base.y;
                cur = pt(cur.x, y);
                segs.push(Seg::Line(cur));
            }
            b'C' => {
                let c1 = s.point(base)?;
                let c2 = s.point(base)?;
                let p = s.point(base)?;
                segs.push(Seg::Cubic(c1, c2, p));
                cur = p;
            }
            b'A' => {
                let rx = s.number()?;
                let ry = s.number()?;
                let rotation = s.number()?;
                let large = s.flag()?;
                let sweep = s.flag()?;
                let to = s.point(base)?;
                segs.push(Seg::Arc {
                    to,
                    rx,
                    ry,
                    rotation,
                    large,
                    sweep,
                });
                cur = to;
            }
            b'Z' => {
                segs.push(Seg::Close);
                cur = start;
            }
            _ => return Err(s.error("unsupported command")),
        }
    }
    Ok(segs)
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn error(&self, what: &'static str) -> PathError {
        PathError { at: self.i, what }
    }

    fn skip_separators(&mut self) {
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_whitespace() || c == b',')
        {
            self.i += 1;
        }
    }

    fn number(&mut self) -> std::result::Result<f32, PathError> {
        self.skip_separators();
        let begin = self.i;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.i += 1;
        }
        let mut digits = self.digits();
        if self.peek() == Some(b'.') {
            self.i += 1;
            digits += self.digits();
        }
        if digits == 0 {
            self.i = begin;
            return Err(self.error("expected a number"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let mark = self.i;
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                self.i = mark;
            }
        }
        let text = std::str::from_utf8(&self.b[begin..self.i]).expect("ASCII number");
        text.parse().map_err(|_| PathError {
            at: begin,
            what: "bad number",
        })
    }

    fn digits(&mut self) -> usize {
        let from = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        self.i - from
    }

    /// An arc flag: a single `0` or `1`, which may run into what follows.
    fn flag(&mut self) -> std::result::Result<bool, PathError> {
        self.skip_separators();
        let flag = match self.peek() {
            Some(b'0') => false,
            Some(b'1') => true,
            _ => return Err(self.error("expected an arc flag")),
        };
        self.i += 1;
        Ok(flag)
    }

    fn point(&mut self, base: Pt) -> std::result::Result<Pt, PathError> {
        let x = self.number()?;
        let y = self.number()?;
        Ok(pt(base.x + x, base.y + y))
    }
}

// ---- Direct2D ----

/// Geometries of every icon, built once per factory on the 24-unit grid.
/// Factory resources are device independent, so a lost render target needs
/// no rebuild.
pub struct IconSet {
    factory: ID2D1Factory,
    round: ID2D1StrokeStyle,
    /// Indexed by `Icon as usize`.
    icons: Vec<Layers>,
}

struct Layers {
    duo: Option<ID2D1PathGeometry>,
    stroke: Option<ID2D1PathGeometry>,
    dots: Option<ID2D1PathGeometry>,
}

impl IconSet {
    pub fn new(factory: &ID2D1Factory) -> Result<Self> {
        let round = unsafe {
            factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?
        };
        let icons = Icon::ALL
            .iter()
            .map(|&icon| {
                Ok(Layers {
                    duo: layer_geometry(factory, icon, Layer::Duo)?,
                    stroke: layer_geometry(factory, icon, Layer::Stroke)?,
                    dots: layer_geometry(factory, icon, Layer::Dots)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            factory: factory.clone(),
            round,
            icons,
        })
    }

    /// Draws `icon` as a `size` square centred in `area`: the duotone fill
    /// in `accent` (when given) at `DUO_ALPHA × ink_alpha`, then the stroke
    /// and the dots in `ink`. The stroke stays `STROKE_W` DIPs wide at any
    /// size; the target's DPI and world transform still apply. The brush's
    /// colour is restored afterwards.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        rt: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        icon: Icon,
        area: Rect,
        size: f32,
        ink: u32,
        ink_alpha: f32,
        accent: Option<u32>,
    ) -> Result<()> {
        if size <= 0.0 {
            return Ok(());
        }
        let layers = &self.icons[icon as usize];
        let k = size / GRID;
        let transform = Matrix3x2 {
            M11: k,
            M12: 0.0,
            M21: 0.0,
            M22: k,
            M31: area.x + (area.w - size) / 2.0,
            M32: area.y + (area.h - size) / 2.0,
        };
        let placed = |g: &ID2D1PathGeometry| unsafe {
            self.factory.CreateTransformedGeometry(g, &transform)
        };
        unsafe {
            let previous = brush.GetColor();
            let result = (|| -> Result<()> {
                if let (Some(accent), Some(g)) = (accent, &layers.duo) {
                    brush.SetColor(&color(accent, DUO_ALPHA * ink_alpha));
                    rt.FillGeometry(&placed(g)?, brush, None);
                }
                brush.SetColor(&color(ink, ink_alpha));
                if let Some(g) = &layers.stroke {
                    rt.DrawGeometry(&placed(g)?, brush, STROKE_W, &self.round);
                }
                if let Some(g) = &layers.dots {
                    rt.FillGeometry(&placed(g)?, brush, None);
                }
                Ok(())
            })();
            brush.SetColor(&previous);
            result
        }
    }
}

/// The parts of `icon` drawn in `layer` as one path geometry, `None` if it has none.
fn layer_geometry(
    factory: &ID2D1Factory,
    icon: Icon,
    layer: Layer,
) -> Result<Option<ID2D1PathGeometry>> {
    let parts: Vec<Part> = icon
        .parts()
        .iter()
        .copied()
        .filter(|p| p.layer() == layer)
        .collect();
    if parts.is_empty() {
        return Ok(None);
    }
    unsafe {
        let geometry = factory.CreatePathGeometry()?;
        let sink = geometry.Open()?;
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
        for part in parts {
            let segs = part
                .outline()
                .map_err(|e| Error::new(E_INVALIDARG, format!("{icon:?}: {e}")))?;
            emit(&sink, &segs);
        }
        sink.Close()?;
        Ok(Some(geometry))
    }
}

/// Feeds segments into a sink: each figure ends closed after `Z`, open otherwise.
/// Every figure begins filled, even in the stroke layer (which is never
/// filled): a hollow figure has empty bounds.
fn emit(sink: &ID2D1GeometrySink, segs: &[Seg]) {
    let v = |p: Pt| Vector2::new(p.x, p.y);
    let mut open = false;
    let mut cur = pt(0.0, 0.0);
    let mut start = cur;
    unsafe {
        for seg in segs {
            match *seg {
                Seg::Move(p) => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        open = false;
                    }
                    cur = p;
                    start = p;
                }
                Seg::Close => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        open = false;
                    }
                    cur = start;
                }
                drawn => {
                    if !open {
                        sink.BeginFigure(v(cur), D2D1_FIGURE_BEGIN_FILLED);
                        start = cur;
                        open = true;
                    }
                    match drawn {
                        Seg::Line(p) => {
                            sink.AddLine(v(p));
                            cur = p;
                        }
                        Seg::Cubic(c1, c2, p) => {
                            sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                                point1: v(c1),
                                point2: v(c2),
                                point3: v(p),
                            });
                            cur = p;
                        }
                        Seg::Arc {
                            to,
                            rx,
                            ry,
                            rotation,
                            large,
                            sweep,
                        } => {
                            sink.AddArc(&D2D1_ARC_SEGMENT {
                                point: v(to),
                                size: D2D_SIZE_F {
                                    width: rx,
                                    height: ry,
                                },
                                rotationAngle: rotation,
                                sweepDirection: if sweep {
                                    D2D1_SWEEP_DIRECTION_CLOCKWISE
                                } else {
                                    D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                                },
                                arcSize: if large {
                                    D2D1_ARC_SIZE_LARGE
                                } else {
                                    D2D1_ARC_SIZE_SMALL
                                },
                            });
                            cur = to;
                        }
                        Seg::Move(_) | Seg::Close => unreachable!("handled above"),
                    }
                }
            }
        }
        if open {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
    }
}

/// 0xRRGGBB plus alpha as a Direct2D colour (straight alpha).
fn color(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    let channel = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
    D2D1_COLOR_F {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(segs: &[Seg]) -> String {
        segs.iter()
            .map(|s| match s {
                Seg::Move(_) => 'M',
                Seg::Line(_) => 'L',
                Seg::Cubic(..) => 'C',
                Seg::Arc { .. } => 'A',
                Seg::Close => 'Z',
            })
            .collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn near(p: Pt, x: f32, y: f32) -> bool {
        close(p.x, x) && close(p.y, y)
    }

    #[test]
    fn all_is_in_declaration_order() {
        for (i, icon) in Icon::ALL.iter().enumerate() {
            assert_eq!(*icon as usize, i, "{icon:?}");
        }
    }

    #[test]
    fn every_icon_parses() {
        for icon in Icon::ALL {
            assert!(!icon.parts().is_empty(), "{icon:?}");
            for part in icon.parts() {
                let segs = part.outline().unwrap_or_else(|e| panic!("{icon:?}: {e}"));
                assert!(matches!(segs.first(), Some(Seg::Move(_))), "{icon:?}");
            }
        }
    }

    #[test]
    fn every_point_lies_on_the_grid() {
        let inside =
            |p: Pt| (-0.01..=GRID + 0.01).contains(&p.x) && (-0.01..=GRID + 0.01).contains(&p.y);
        for icon in Icon::ALL {
            for part in icon.parts() {
                for seg in part.outline().expect("parses") {
                    let points: Vec<Pt> = match seg {
                        Seg::Move(p) | Seg::Line(p) => vec![p],
                        Seg::Cubic(a, b, c) => vec![a, b, c],
                        Seg::Arc { to, .. } => vec![to],
                        Seg::Close => vec![],
                    };
                    for p in points {
                        assert!(inside(p), "{icon:?}: {p:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn implicit_repeat_after_relative_cubic() {
        let segs = parse_path("M12 21c1.3 0 1.8-1 1.4-2-.5-1.2.3-2.4 1.6-2.4").unwrap();
        assert_eq!(kinds(&segs), "MCC");
        let Seg::Cubic(a, b, c) = segs[1] else {
            panic!()
        };
        assert!(near(a, 13.3, 21.0) && near(b, 13.8, 20.0) && near(c, 13.4, 19.0));
        // The repeat is relative to the first curve's end point.
        let Seg::Cubic(a, b, c) = segs[2] else {
            panic!()
        };
        assert!(near(a, 12.9, 17.8) && near(b, 13.7, 16.6) && near(c, 15.0, 16.6));
    }

    #[test]
    fn compact_numbers() {
        let segs = parse_path("M.5 -.5l1.8-1").unwrap();
        assert!(matches!(segs[0], Seg::Move(p) if near(p, 0.5, -0.5)));
        assert!(matches!(segs[1], Seg::Line(p) if near(p, 2.3, -1.5)));
        assert!(matches!(parse_path("M1e1 2").unwrap()[0], Seg::Move(p) if near(p, 10.0, 2.0)));
    }

    #[test]
    fn relative_arc_with_flags() {
        let segs = parse_path("M4 8.5a1 1 0 0 1-1-1A4 4 0 1 0 16 8a2 3 30 1 1 1-1").unwrap();
        assert_eq!(kinds(&segs), "MAAA");
        assert_eq!(
            segs[1],
            Seg::Arc {
                to: pt(3.0, 7.5),
                rx: 1.0,
                ry: 1.0,
                rotation: 0.0,
                large: false,
                sweep: true,
            }
        );
        assert!(
            matches!(segs[2], Seg::Arc { to, large: true, sweep: false, .. } if near(to, 16.0, 8.0))
        );
        assert_eq!(
            segs[3],
            Seg::Arc {
                to: pt(17.0, 7.0),
                rx: 2.0,
                ry: 3.0,
                rotation: 30.0,
                large: true,
                sweep: true,
            }
        );
        // Flags may run into the next number.
        let packed = parse_path("M0 0a1 1 0 011 1").unwrap();
        assert!(
            matches!(packed[1], Seg::Arc { to, large: false, sweep: true, .. } if near(to, 1.0, 1.0))
        );
    }

    #[test]
    fn horizontal_and_vertical() {
        let segs = parse_path("M2 3H10V7h-4v2").unwrap();
        let ends: Vec<Pt> = segs
            .iter()
            .map(|s| match s {
                Seg::Move(p) | Seg::Line(p) => *p,
                _ => panic!(),
            })
            .collect();
        assert_eq!(
            ends,
            [
                pt(2.0, 3.0),
                pt(10.0, 3.0),
                pt(10.0, 7.0),
                pt(6.0, 7.0),
                pt(6.0, 9.0)
            ]
        );
    }

    #[test]
    fn close_returns_to_the_subpath_start() {
        let segs = parse_path("M5 6v12l9-6zl1 1M3 3h1Z").unwrap();
        assert_eq!(kinds(&segs), "MLLZLMLZ");
        // After z, relative commands start from the subpath's first point.
        assert!(matches!(segs[4], Seg::Line(p) if near(p, 6.0, 7.0)));
    }

    #[test]
    fn rejects_bad_data() {
        assert!(parse_path("10 10").is_err());
        assert!(parse_path("M1 2Q3 4 5 6").is_err());
        assert!(parse_path("M0 0a1 1 0 2 1 1 1").is_err());
        assert!(parse_path("M1").is_err());
    }

    #[test]
    fn command_counts_match_the_source() {
        let count = |icon: Icon, i: usize| match icon.parts()[i] {
            Part::Path(d) | Part::Fill(d) => kinds(&parse_path(d).unwrap()),
            other => panic!("{other:?}"),
        };
        // M h l h L h a v a H a V a z
        assert_eq!(count(Icon::Camera, 1), "MLLLLLALALALAZ");
        // M a c c H a c z
        assert_eq!(count(Icon::Palette, 1), "MACCLACZ");
        // M v a v l h z
        assert_eq!(count(Icon::Bell, 0), "MLALLLZ");
        // Eight two-point rays.
        assert_eq!(count(Icon::Gear, 3), "ML".repeat(8));
        // M a l a l M a l a l
        assert_eq!(count(Icon::Link, 0), "MALALMALAL");
        // M h V, M L, M v a H a V a H
        assert_eq!(count(Icon::Ext, 0), "MLLMLMLALALAL");
        assert_eq!(count(Icon::Emoji, 2), "MC");
    }

    #[test]
    fn rects_and_circles_close() {
        let r = Part::Rect(5.0, 5.0, 14.0, 16.0, RECT_R).outline().unwrap();
        assert_eq!(kinds(&r), "MLALALALAZ");
        assert!(matches!(r[0], Seg::Move(p) if near(p, 8.0, 5.0)));
        // A radius beyond half the side is clamped.
        let small = Part::Rect(0.0, 0.0, 4.0, 2.0, 3.0).outline().unwrap();
        assert!(matches!(small[2], Seg::Arc { rx, .. } if close(rx, 1.0)));
        let c = Part::Circle(12.0, 12.0, 9.0).outline().unwrap();
        assert_eq!(kinds(&c), "MAAZ");
        let d = Part::Dot(9.0, 10.0).outline().unwrap();
        assert!(matches!(d[0], Seg::Move(p) if near(p, 10.1, 10.0)));
    }

    #[test]
    fn direct2d_geometries_match_the_grid() {
        use windows::Win32::Graphics::Direct2D::{
            D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory,
        };
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }.expect("factory");
        let set = IconSet::new(&factory).expect("icon set");
        let layers = |icon: Icon| &set.icons[icon as usize];
        let bounds = |g: &ID2D1PathGeometry| unsafe { g.GetBounds(None) }.expect("bounds");
        let ring = bounds(layers(Icon::Ring).stroke.as_ref().unwrap());
        assert!(
            close(ring.left, 3.5)
                && close(ring.top, 3.5)
                && close(ring.right, 20.5)
                && close(ring.bottom, 20.5),
            "{ring:?}"
        );
        // Sweep flag 1 is clockwise: the bell's dome rises to y = 5.
        let bell = bounds(layers(Icon::Bell).duo.as_ref().unwrap());
        assert!(
            (bell.top - 5.0).abs() < 0.05 && close(bell.left, 4.5) && close(bell.bottom, 18.0),
            "{bell:?}"
        );
        // Gear: two circles and eight rays; no dots, Play has no duotone.
        let gear = layers(Icon::Gear);
        assert_eq!(
            unsafe { gear.stroke.as_ref().unwrap().GetFigureCount() }.unwrap(),
            10
        );
        assert!(gear.dots.is_none() && layers(Icon::Play).duo.is_none());
        assert_eq!(
            unsafe { layers(Icon::Calc).dots.as_ref().unwrap().GetFigureCount() }.unwrap(),
            6
        );
    }

    #[test]
    fn cleaner_icons_are_line_only_as_in_the_design() {
        let cleaner = [
            Icon::TempFile,
            Icon::Globe,
            Icon::Package,
            Icon::Code,
            Icon::Image,
            Icon::Download,
            Icon::Windows,
        ];
        for icon in cleaner {
            assert!(
                icon.parts().iter().all(|p| p.layer() == Layer::Stroke),
                "{icon:?}"
            );
        }
        let count = |icon: Icon, i: usize| match icon.parts()[i] {
            Part::Path(d) => kinds(&parse_path(d).unwrap()),
            other => panic!("{other:?}"),
        };
        // M h l v H z, then M v h.
        assert_eq!(count(Icon::TempFile, 0), "MLLLLZ");
        assert_eq!(count(Icon::TempFile, 1), "MLL");
        // M h, then two arcs of curves.
        assert_eq!(count(Icon::Globe, 1), "MLMCMC");
        assert_eq!(count(Icon::Code, 0), "MLLMLL");
        assert_eq!(count(Icon::Download, 0), "MLMLLML");
        assert_eq!(Icon::Windows.parts().len(), 4);
        // The preview's copy: the front sheet, and the back one's corner.
        assert!(
            Icon::Copy
                .parts()
                .iter()
                .all(|p| p.layer() == Layer::Stroke)
        );
        assert_eq!(count(Icon::Copy, 1), "MLALALAL");
    }

    #[test]
    fn the_inbox_is_a_tray_with_a_notch_and_not_the_bell() {
        // M l h L v a h A z, then M h l h l h.
        let count = |icon: Icon, i: usize| match icon.parts()[i] {
            Part::Path(d) | Part::Fill(d) => kinds(&parse_path(d).unwrap()),
            other => panic!("{other:?}"),
        };
        assert_eq!(count(Icon::Inbox, 0), "MLLLLALAZ");
        assert_eq!(count(Icon::Inbox, 1), count(Icon::Inbox, 0));
        assert_eq!(count(Icon::Inbox, 2), "MLLLLL");
        // The same duotone layers as the Bell, but another picture.
        let layers: Vec<Layer> = Icon::Inbox.parts().iter().map(|p| p.layer()).collect();
        assert_eq!(layers, [Layer::Duo, Layer::Stroke, Layer::Stroke]);
        assert_ne!(Icon::Inbox.parts(), Icon::Bell.parts());
        // The front edge dips: its middle is lower (larger y) than its ends.
        let segs = Icon::Inbox.parts()[2].outline().unwrap();
        let ys: Vec<f32> = segs
            .iter()
            .map(|s| match s {
                Seg::Move(p) | Seg::Line(p) => p.y,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(ys.first(), ys.last());
        assert!(ys[2] > ys[0] && ys[3] > ys[0]);
        // And it fills the grid like the other page icons do.
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for part in Icon::Inbox.parts() {
            for seg in part.outline().unwrap() {
                if let Seg::Move(p) | Seg::Line(p) | Seg::Arc { to: p, .. } = seg {
                    lo = lo.min(p.x);
                    hi = hi.max(p.x);
                }
            }
        }
        assert!(lo <= 3.5 && hi >= 20.5, "{lo} {hi}");
    }

    #[test]
    fn parts_sort_into_layers() {
        let layers: Vec<Layer> = Icon::Emoji.parts().iter().map(|p| p.layer()).collect();
        assert_eq!(
            layers,
            [
                Layer::Duo,
                Layer::Stroke,
                Layer::Stroke,
                Layer::Dots,
                Layer::Dots
            ]
        );
    }
}
