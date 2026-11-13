//! Theme settings (accent colour, island surface, window mode) and the
//! colours they give the island renderer, the editors and the settings
//! window. The island itself is always dark; only its surface changes.
//! Windows (editors, settings) follow the window mode, where "System" reads
//! the Windows app mode (`AppsUseLightTheme`).
//!
//! Colours are `0xRRGGBB` unless a field says otherwise.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::Config;
use crate::pomodoro::Phase;

/// Declares a settings enum stored in config.json by its variant name. An
/// unknown name (a typo while editing by hand) reads as the default instead of
/// making the whole config invalid. Optional `aliases` map names an older
/// version wrote onto current variants; they are read, never written.
macro_rules! named_enum {
    (
        $(#[$doc:meta])* $name:ident { $default:ident $(, $other:ident)* $(,)? }
        $(aliases { $($alias:literal => $target:ident),* $(,)? })?
    ) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub enum $name {
            #[default]
            $default,
            $($other,)*
        }

        impl $name {
            pub const ALL: &[$name] = &[$name::$default $(, $name::$other)*];

            /// Position in `ALL`.
            #[allow(dead_code)]
            fn index(self) -> u32 {
                Self::ALL.iter().position(|v| *v == self).unwrap_or(0) as u32
            }

            /// `ALL[i]`, or the default past its end.
            #[allow(dead_code)]
            fn from_index(i: u32) -> Self {
                Self::ALL.get(i as usize).copied().unwrap_or_default()
            }

            pub fn name(self) -> &'static str {
                match self {
                    $name::$default => stringify!($default),
                    $($name::$other => stringify!($other),)*
                }
            }

            /// Older names and the variants they now read as.
            const ALIASES: &[(&str, $name)] = &[$($(($alias, $name::$target),)*)?];

            /// Case-insensitive; a current name first, then an alias.
            pub fn from_name(name: &str) -> Option<Self> {
                let name = name.trim();
                Self::ALL
                    .iter()
                    .copied()
                    .find(|v| v.name().eq_ignore_ascii_case(name))
                    .or_else(|| {
                        Self::ALIASES
                            .iter()
                            .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
                            .map(|(_, v)| *v)
                    })
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.name())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let name = String::deserialize(d)?;
                Ok(Self::from_name(&name).unwrap_or_default())
            }
        }
    };
}

/// For settings enums elsewhere (the robot's motion policy).
pub(crate) use named_enum;

named_enum! {
    /// Accent colour of the island and the windows.
    Accent { Terracotta, Sage, Ochre, Clay, Olive, Plum }
    aliases {
        "Blue" => Terracotta,
        "Green" => Sage,
        "Orange" => Ochre,
        "Pink" => Clay,
        "Purple" => Plum,
        "Red" => Terracotta,
    }
}

named_enum! {
    /// The island's fill. `Auto` follows the cat's colour
    /// (`Surface::resolve`) and never reaches the palette.
    Surface { Auto, Espresso, Graphite, Sea, Night, Plum, Terminal }
    aliases { "Black" => Espresso }
}

named_enum! {
    /// The notch cat's coat.
    CatColor { Terracotta, Calico, White, Black }
}

named_enum! {
    /// What the notch cat wears.
    CatAccessory { None, Glasses, Bow, Nightcap, Scarf, Flower, Crown, Headphone, Sailor, Bowtie }
}

named_enum! {
    /// Which mascot lives in the notch.
    Mascot { Cat, Falk, Octopus }
}

named_enum! {
    /// How often the mascot plays a short animation on its own. `Sering`
    /// plays often, `Sesekali` (the default) now and then, `Jarang` rarely,
    /// `Sapaan` only with the greeting and on the hour (never on the idle
    /// timer), `Mati` nothing automatic (the menu item still plays one).
    MascotPlay { Sesekali, Sering, Jarang, Sapaan, Mati }
}

named_enum! {
    /// The falk's coat.
    FalkColor { Lutino, Grey, WhiteFace }
}

named_enum! {
    /// The octopus's coat.
    OctopusColor { Coral, Red, Purple, Blue }
}

/// A mascot together with its own coat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coat {
    Cat(CatColor),
    Falk(FalkColor),
    Octopus(OctopusColor),
}

impl Coat {
    /// The chosen mascot's own coat.
    pub fn of(cfg: &Config) -> Coat {
        Coat::from_parts(cfg.mascot, cfg.cat_color, cfg.falk_color, cfg.octopus_color)
    }

    /// `mascot`'s coat out of the three kept side by side.
    pub fn from_parts(
        mascot: Mascot,
        cat: CatColor,
        falk: FalkColor,
        octopus: OctopusColor,
    ) -> Coat {
        match mascot {
            Mascot::Cat => Coat::Cat(cat),
            Mascot::Falk => Coat::Falk(falk),
            Mascot::Octopus => Coat::Octopus(octopus),
        }
    }

    /// Every coat `mascot` can have, in the order Settings shows them.
    pub fn all_of(mascot: Mascot) -> Vec<Coat> {
        match mascot {
            Mascot::Cat => CatColor::ALL.iter().map(|c| Coat::Cat(*c)).collect(),
            Mascot::Falk => FalkColor::ALL.iter().map(|c| Coat::Falk(*c)).collect(),
            Mascot::Octopus => OctopusColor::ALL.iter().map(|c| Coat::Octopus(*c)).collect(),
        }
    }
}

named_enum! {
    /// Light or dark editors and settings window; `System` follows Windows.
    WindowTheme { System, Dark, Light }
}

impl Accent {
    /// The accent on the island and in dark windows.
    pub fn rgb(self) -> u32 {
        match self {
            Accent::Terracotta => 0xE2683C,
            Accent::Sage => 0x9DB59A,
            Accent::Ochre => 0xD9A441,
            Accent::Clay => 0xC9826B,
            Accent::Olive => 0xA3A55A,
            Accent::Plum => 0xB07A9A,
        }
    }

    /// Text and marks drawn on `rgb()`: espresso ink, which reads on every
    /// one of these mid-light earth tones.
    pub fn ink(self) -> u32 {
        0x17120F
    }

    /// The accent in light windows: a darker shade that holds 3:1 against
    /// the light background and cards.
    pub fn light_rgb(self) -> u32 {
        match self {
            Accent::Terracotta => 0xC9542B,
            Accent::Sage => 0x6C7D6A,
            Accent::Ochre => 0x96712D,
            Accent::Clay => 0xA36957,
            Accent::Olive => 0x7A7C44,
            Accent::Plum => 0x996A86,
        }
    }

    /// Text and marks drawn on `light_rgb()`.
    pub fn light_ink(self) -> u32 {
        0xFFFBF5
    }
}

impl Surface {
    /// The island's fill. `Auto` is resolved first (`resolve`); it reads as
    /// Espresso here so a stray one can never draw a wrong colour.
    pub fn rgb(self) -> u32 {
        match self {
            Surface::Auto | Surface::Espresso => 0x17120F,
            Surface::Graphite => 0x221B17,
            Surface::Sea => 0x132321,
            Surface::Night => 0x151C2E,
            Surface::Plum => 0x2A2140,
            Surface::Terminal => 0x1B1D1F,
        }
    }

    /// The concrete surface: `Auto` follows the mascot's coat (cat: Terracotta
    /// -> Espresso, Calico -> Sea, White -> Night, Black -> Plum; falk: Lutino
    /// -> Espresso, Grey -> Night, WhiteFace -> Plum; octopus: Coral ->
    /// Espresso, Red -> Graphite, Purple -> Night, Blue -> Sea; the mascot off
    /// -> Espresso), every other value is itself.
    pub fn resolve(self, coat: Coat, robot_on: bool) -> Surface {
        match self {
            Surface::Auto if !robot_on => Surface::Espresso,
            Surface::Auto => match coat {
                Coat::Cat(CatColor::Terracotta) => Surface::Espresso,
                Coat::Cat(CatColor::Calico) => Surface::Sea,
                Coat::Cat(CatColor::White) => Surface::Night,
                Coat::Cat(CatColor::Black) => Surface::Plum,
                Coat::Falk(FalkColor::Lutino) => Surface::Espresso,
                Coat::Falk(FalkColor::Grey) => Surface::Night,
                Coat::Falk(FalkColor::WhiteFace) => Surface::Plum,
                Coat::Octopus(OctopusColor::Coral) => Surface::Espresso,
                Coat::Octopus(OctopusColor::Red) => Surface::Graphite,
                Coat::Octopus(OctopusColor::Purple) => Surface::Night,
                Coat::Octopus(OctopusColor::Blue) => Surface::Sea,
            },
            other => other,
        }
    }

    /// The caption colour on this surface and on its raised step: tinted to
    /// the surface, at least 4.5:1 on both (a test checks every surface).
    fn secondary(self) -> u32 {
        match self {
            Surface::Auto | Surface::Espresso | Surface::Graphite => 0xA89A8B,
            Surface::Sea => 0xA3B8B2,
            Surface::Night => 0xA7B1C8,
            Surface::Plum => 0xB9ACD0,
            Surface::Terminal => 0xA9ADB2,
        }
    }
}

/// The three theme settings together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub accent: Accent,
    /// Always concrete: `Theme::of` resolves `Auto`.
    pub surface: Surface,
    pub window: WindowTheme,
}

/// The default settings, with the default surface resolved (Espresso).
impl Default for Theme {
    fn default() -> Self {
        Theme {
            accent: Accent::default(),
            surface: Surface::default().resolve(Coat::Cat(CatColor::default()), true),
            window: WindowTheme::default(),
        }
    }
}

impl Theme {
    pub fn of(cfg: &Config) -> Self {
        Theme {
            accent: cfg.accent,
            surface: cfg.surface.resolve(Coat::of(cfg), cfg.robot),
            window: cfg.window_theme,
        }
    }
}

/// Island colours. Panel rows, tracks and cards sit a step above the surface,
/// so they lighten with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The island's fill.
    pub surface: u32,
    /// Note rows, the view selector track, the waiting strip.
    pub raised: u32,
    /// The active segment of the view selector.
    pub raised_hi: u32,
    /// Ring tracks, stepper and secondary buttons, dividers.
    pub track: u32,
    /// Week view day columns.
    pub column: u32,
    /// Album art placeholder tile.
    pub placeholder: u32,
    /// Inactive page dots.
    pub dot_off: u32,
    pub text: u32,
    /// Today, the selected day, event dot fallback, equalizer, primary
    /// buttons, the active page dot.
    pub accent: u32,
    /// Text and marks on an accent fill.
    pub on_accent: u32,
    /// Captions, times, hour labels, inactive segments.
    pub secondary: u32,
    /// Status colours, fixed whatever the accent: sage for breaks, event
    /// dots and all-day events; ochre for battery and "Terlewat"; plum for
    /// a third event colour.
    pub sage: u32,
    pub ochre: u32,
    pub plum: u32,
}

impl Palette {
    /// The pomodoro's focus phase: the accent.
    pub fn focus(&self) -> u32 {
        self.accent
    }

    /// The pomodoro's breaks: sage.
    pub fn rest(&self) -> u32 {
        self.sage
    }

    /// A pomodoro phase's colour: `focus` or `rest`.
    pub fn phase(&self, phase: Phase) -> u32 {
        match phase {
            Phase::Focus => self.focus(),
            Phase::ShortBreak | Phase::LongBreak => self.rest(),
        }
    }
}

/// Corner radii of the Studio scale, in logical px. A chip is fully rounded
/// (half its height) and has no constant.
pub mod radius {
    /// The open panel.
    pub const PANEL: f32 = 30.0;
    /// The island's notices, both styles.
    pub const NOTICE: f32 = 26.0;
    /// The media page's album cover.
    pub const COVER: f32 = 22.0;
    /// Shortcut tiles.
    pub const TILE: f32 = 18.0;
    /// Cards: the notes page's.
    pub const CARD: f32 = 14.0;
    /// A notice's badge.
    pub const BADGE: f32 = 13.0;
    /// Week view day columns.
    pub const COLUMN: f32 = 12.0;
    /// Controls: the view selector's track, buttons.
    pub const CONTROL: f32 = 10.0;
    /// Day view event blocks.
    pub const BLOCK: f32 = 9.0;
    /// A segment inside a control, the all-day band.
    pub const SEGMENT: f32 = 8.0;
    /// Week view event chips.
    pub const EVENT_CHIP: f32 = 7.0;
    /// The compact album art.
    pub const ART: f32 = 6.0;
}

/// `surface` with every channel raised by `d`: how the coloured surfaces get
/// their raised steps (Graphite lifts Espresso the same way).
fn lift(surface: u32, d: u32) -> u32 {
    let ch = |shift: u32| (((surface >> shift) & 0xFF) + d).min(0xFF) << shift;
    ch(16) | ch(8) | ch(0)
}

/// The island's colours for a concrete surface (`Auto` reads as Espresso;
/// resolve it with `Surface::resolve` first).
pub fn island(accent: Accent, surface: Surface) -> Palette {
    // Warm browns over espresso; graphite lifts each a step; the coloured
    // surfaces lift their own colour the same way.
    let (raised, raised_hi, track, column, placeholder, dot_off) = match surface {
        Surface::Auto | Surface::Espresso => {
            (0x241D18, 0x3A2F27, 0x332A23, 0x1D1713, 0x332A23, 0x4A3E35)
        }
        Surface::Graphite => (0x2E2620, 0x463A31, 0x3D332B, 0x272019, 0x3D332B, 0x574A40),
        Surface::Sea | Surface::Night | Surface::Plum | Surface::Terminal => {
            let s = surface.rgb();
            (
                lift(s, 12),
                lift(s, 34),
                lift(s, 26),
                lift(s, 5),
                lift(s, 26),
                lift(s, 50),
            )
        }
    };
    Palette {
        surface: surface.rgb(),
        raised,
        raised_hi,
        track,
        column,
        placeholder,
        dot_off,
        text: 0xF4EDE4,
        accent: accent.rgb(),
        on_accent: accent.ink(),
        secondary: surface.secondary(),
        sage: Accent::Sage.rgb(),
        ochre: Accent::Ochre.rgb(),
        // The third event colour is lifted on the Plum surface, where #B07A9A
        // falls to 4.4:1, and on Terminal, where it holds 4.9:1 on the fill
        // but only 4.2:1 on the raised step.
        plum: if matches!(surface, Surface::Plum | Surface::Terminal) {
            0xC893B4
        } else {
            Accent::Plum.rgb()
        },
    }
}

/// The island's palette for the config: `Auto` is resolved against the cat.
pub fn island_palette(cfg: &Config) -> Palette {
    island(cfg.accent, cfg.surface.resolve(Coat::of(cfg), cfg.robot))
}

/// The CLI page's terminal (`.term` in docs/design/pengingat-cli.html),
/// whatever the surface: background and text.
pub const TERMINAL_BG: u32 = 0x0F0C0A;
pub const TERMINAL_TEXT: u32 = 0xE6DED3;
/// The line over the input (`.inp`), and between commands (`.sep`).
pub const TERMINAL_RULE: u32 = 0x221B16;
pub const TERMINAL_SEP: u32 = 0x2A221C;
/// The input's placeholder and completion (`.ghost`) and its hint.
pub const TERMINAL_GHOST: u32 = 0x5E5248;
pub const TERMINAL_HINT: u32 = 0x6E6155;
/// Error lines and a failed command's chip on the island: soft red.
pub const SOFT_RED: u32 = 0xFF8A73;
/// The recording red: the REC dot, the Rekam page's start button and the
/// tray's dot. A true red, so it reads as "recording" and not as an error.
pub const REC_RED: u32 = 0xE5484D;

/// Colours of ordinary windows (editors, settings, the screenshot editor).
///
/// Light is one palette. Dark is warm brown, except when the island surface is
/// `Surface::Terminal`: then `window_for` gives a neutral dark built on the
/// island's #1B1D1F, so the windows match the island's flat near-black instead
/// of sitting on a brown that the island no longer has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPalette {
    pub light: bool,
    pub bg: u32,
    pub card: u32,
    /// `0xRRGGBBAA`: a hairline at low alpha over `bg` or `card`.
    pub border: u32,
    pub text: u32,
    pub secondary: u32,
    pub accent: u32,
    pub on_accent: u32,
    /// Text field fill (editors).
    pub field: u32,
    /// Secondary button fill, and its hover.
    pub button: u32,
    pub button_hover: u32,
    /// Error messages.
    pub error: u32,
    /// Popover fill (the editors' calendar and time list).
    pub popover: u32,
    /// Hover fill of rows, days and quiet buttons (the design's `--hov`).
    pub hover: u32,
    /// `0xRRGGBBAA`: the accent at low alpha, for the picked row, the active
    /// shortcut pill and the glow around a focused chip (the design's `--sel`).
    pub selection: u32,
    /// Placeholders and days outside the shown month (the design's `--tx3`).
    pub tertiary: u32,
}

/// The warm window palette (what every surface but Terminal uses). The app
/// itself calls `window_for`; this is the reference the tests compare it with.
#[cfg(test)]
pub fn window(accent: Accent, mode: WindowTheme, system_light: bool) -> WindowPalette {
    window_for(accent, Surface::Espresso, mode, system_light)
}

/// The window palette for an island `surface`. Light (`Light`, or `System`
/// with a light Windows) is the same whatever the surface. Dark is the warm
/// brown, except on `Surface::Terminal`, where it is a neutral dark on
/// #1B1D1F: the island's fill lifted by the same steps the warm palette takes
/// above its own bg (card +8, popover +10, field +12, button +17, hover
/// button +26), equal on every channel. The text stays #F4EDE4, the island's
/// text on every surface, Terminal included; `secondary` is the island's
/// Terminal grey. `Surface::Auto` (never in a `Theme`) reads as warm.
pub fn window_for(
    accent: Accent,
    surface: Surface,
    mode: WindowTheme,
    system_light: bool,
) -> WindowPalette {
    let light = match mode {
        WindowTheme::System => system_light,
        WindowTheme::Dark => false,
        WindowTheme::Light => true,
    };
    if light {
        WindowPalette {
            light,
            bg: 0xF4EEE6,
            card: 0xFFFBF5,
            border: 0x221A141A,
            text: 0x221A14,
            // The design's #7A6B5D falls just short of 4.5:1 on bg.
            secondary: 0x786A5C,
            accent: accent.light_rgb(),
            on_accent: accent.light_ink(),
            field: 0xFFFFFF,
            button: 0xEFE6DA,
            button_hover: 0xEADFD1,
            error: 0xC42B1C,
            popover: 0xFFFBF5,
            hover: 0xEFE6DA,
            // 12 % of the accent.
            selection: accent.light_rgb() << 8 | 0x1F,
            tertiary: 0xA89A8B,
        }
    } else if surface == Surface::Terminal {
        WindowPalette {
            light,
            bg: 0x1B1D1F,
            card: 0x232527,
            border: 0xF4EDE414,
            text: 0xF4EDE4,
            secondary: 0xA9ADB2,
            accent: accent.rgb(),
            on_accent: accent.ink(),
            field: 0x27292B,
            button: 0x2C2E30,
            button_hover: 0x353739,
            error: 0xEF7A6E,
            popover: 0x252729,
            hover: 0x2C2E30,
            // 16 % of the accent.
            selection: accent.rgb() << 8 | 0x29,
            tertiary: 0x646769,
        }
    } else {
        WindowPalette {
            light,
            bg: 0x1E1814,
            card: 0x272019,
            border: 0xF4EDE414,
            text: 0xF4EDE4,
            secondary: 0xA89A8B,
            accent: accent.rgb(),
            on_accent: accent.ink(),
            field: 0x2C241D,
            button: 0x33291F,
            button_hover: 0x3D3227,
            error: 0xEF7A6E,
            popover: 0x2A221C,
            hover: 0x33291F,
            // 16 % of the accent.
            selection: accent.rgb() << 8 | 0x29,
            tertiary: 0x6E6155,
        }
    }
}

/// The theme in effect, for drawing code that has no `Config` (the editors):
/// set by the app, read on the UI thread.
static CURRENT: AtomicU32 = AtomicU32::new(0);
static SYSTEM_LIGHT: AtomicBool = AtomicBool::new(false);

fn pack(t: Theme) -> u32 {
    t.accent.index() | t.surface.index() << 8 | t.window.index() << 16
}

fn unpack(v: u32) -> Theme {
    Theme {
        accent: Accent::from_index(v & 0xFF),
        surface: Surface::from_index((v >> 8) & 0xFF),
        window: WindowTheme::from_index((v >> 16) & 0xFF),
    }
}

/// Makes `t` the theme in effect. True when it changed.
pub fn set_current(t: Theme) -> bool {
    CURRENT.swap(pack(t), Ordering::Relaxed) != pack(t)
}

pub fn current() -> Theme {
    unpack(CURRENT.load(Ordering::Relaxed))
}

/// Records whether Windows apps are in light mode. True when it changed.
pub fn set_system_light(light: bool) -> bool {
    SYSTEM_LIGHT.swap(light, Ordering::Relaxed) != light
}

/// The window palette in effect.
pub fn current_window() -> WindowPalette {
    let t = current();
    window_for(
        t.accent,
        t.surface,
        t.window,
        SYSTEM_LIGHT.load(Ordering::Relaxed),
    )
}

/// The theme a demo or debug mode runs with: `DI_WINDOW`, `DI_SURFACE` (unset
/// or `auto` gives the default's Espresso) and `DI_ACCENT`; with none of them
/// set it is `Theme::default()`.
#[cfg(debug_assertions)]
pub fn theme_from_env() -> Theme {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    Theme {
        accent: Accent::from_name(&var("DI_ACCENT")).unwrap_or_default(),
        surface: Surface::from_name(&var("DI_SURFACE"))
            .unwrap_or_default()
            .resolve(Coat::Cat(CatColor::default()), true),
        window: WindowTheme::from_name(&var("DI_WINDOW")).unwrap_or_default(),
    }
}

/// `AppsUseLightTheme` under HKCU Personalize; light when the value is
/// missing, as Windows itself assumes.
pub fn read_system_light() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::w;
    let mut value: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    status.is_err() || value != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_match_the_pinned_hex_values() {
        let hex: Vec<(Accent, u32)> = Accent::ALL.iter().map(|a| (*a, a.rgb())).collect();
        assert_eq!(
            hex,
            [
                (Accent::Terracotta, 0xE2683C),
                (Accent::Sage, 0x9DB59A),
                (Accent::Ochre, 0xD9A441),
                (Accent::Clay, 0xC9826B),
                (Accent::Olive, 0xA3A55A),
                (Accent::Plum, 0xB07A9A),
            ]
        );
        assert_eq!(Accent::default(), Accent::Terracotta);
        assert_eq!(Surface::default(), Surface::Auto);
        // `Theme.surface` is always concrete, the default's too.
        assert_eq!(Theme::default().surface, Surface::Espresso);
        assert_eq!(Accent::Terracotta.light_rgb(), 0xC9542B);
    }

    fn luminance(rgb: u32) -> f64 {
        let lin = |shift: u32| {
            let c = ((rgb >> shift) & 0xFF) as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(16) + 0.7152 * lin(8) + 0.0722 * lin(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn ink_on_every_accent_is_readable() {
        for a in Accent::ALL {
            let c = contrast(a.rgb(), a.ink());
            assert!(c >= 3.0, "{a:?}: {c:.2}");
            // The espresso ink reads as body text on every accent.
            assert!(c >= 4.5, "{a:?}: {c:.2}");
        }
    }

    #[test]
    fn light_mode_accents_read_on_the_light_window() {
        let p = window(Accent::Terracotta, WindowTheme::Light, false);
        for a in Accent::ALL {
            let (fill, ink) = (a.light_rgb(), a.light_ink());
            for surface in [p.bg, p.card] {
                let c = contrast(fill, surface);
                assert!(c >= 3.0, "{a:?} on {surface:06X}: {c:.2}");
            }
            let c = contrast(fill, ink);
            assert!(c >= 3.0, "{a:?} ink: {c:.2}");
        }
    }

    #[test]
    fn espresso_surface_matches_the_pinned_island_colours() {
        let p = island(Accent::Terracotta, Surface::Espresso);
        assert_eq!(
            p,
            Palette {
                surface: 0x17120F,
                raised: 0x241D18,
                raised_hi: 0x3A2F27,
                track: 0x332A23,
                column: 0x1D1713,
                placeholder: 0x332A23,
                dot_off: 0x4A3E35,
                text: 0xF4EDE4,
                accent: 0xE2683C,
                on_accent: 0x17120F,
                secondary: 0xA89A8B,
                sage: 0x9DB59A,
                ochre: 0xD9A441,
                plum: 0xB07A9A,
            }
        );
        assert_eq!((p.focus(), p.rest()), (0xE2683C, 0x9DB59A));
    }

    #[test]
    fn island_text_and_status_colours_read_on_every_surface() {
        for &surface in Surface::ALL {
            let p = island(Accent::Terracotta, surface);
            assert!(contrast(p.text, p.surface) >= 7.0, "{surface:?}");
            assert!(contrast(p.secondary, p.surface) >= 4.5, "{surface:?}");
            assert!(contrast(p.secondary, p.raised) >= 4.5, "{surface:?}");
            for status in [p.sage, p.ochre, p.plum] {
                assert!(contrast(status, p.surface) >= 4.5, "{status:06X}");
            }
        }
        // Status colours stay put whatever the accent.
        let plum = island(Accent::Plum, Surface::Espresso);
        assert_eq!((plum.sage, plum.ochre), (0x9DB59A, 0xD9A441));
        assert_eq!((plum.focus(), plum.rest()), (0xB07A9A, 0x9DB59A));
    }

    /// Contrast numbers per concrete surface (also in the memory note
    /// `cat-colours-and-surfaces`): text and secondary on the surface and
    /// on `raised`, at least 4.5:1, and the ink on every accent.
    #[test]
    fn every_concrete_surface_keeps_text_and_secondary_readable() {
        let concrete: Vec<Surface> = Surface::ALL
            .iter()
            .copied()
            .filter(|s| *s != Surface::Auto)
            .collect();
        assert_eq!(concrete.len(), 6);
        assert!(concrete.contains(&Surface::Terminal));
        for surface in concrete {
            let p = island(Accent::Terracotta, surface);
            let nums = [
                contrast(p.text, p.surface),
                contrast(p.text, p.raised),
                contrast(p.secondary, p.surface),
                contrast(p.secondary, p.raised),
            ];
            println!(
                "{surface:?} {:06X}: text {:.2}/{:.2} secondary {:06X} {:.2}/{:.2}",
                p.surface, nums[0], nums[1], p.secondary, nums[2], nums[3]
            );
            assert_eq!(p.text, 0xF4EDE4);
            for c in nums {
                assert!(c >= 4.5, "{surface:?}: {c:.2}");
            }
            for a in Accent::ALL {
                let c = contrast(a.rgb(), a.ink());
                assert!(c >= 4.5, "{a:?} on {surface:?}: {c:.2}");
                // The island's accent is never the island's own fill.
                assert_ne!(a.rgb(), p.surface);
            }
            // The status and event colours (sage, ochre, plum) read on the
            // surface and on its raised step.
            // (#B07A9A, the default third colour, is only 4.0-4.3:1 on the
            // raised step of Graphite, Sea and Night, so the raised step is
            // checked for it where it is lifted: Plum and Terminal.)
            let lifted = matches!(surface, Surface::Plum | Surface::Terminal);
            for status in [p.sage, p.ochre, p.plum] {
                let c = contrast(status, p.surface);
                assert!(c >= 4.5, "{surface:?} {status:06X}: {c:.2}");
                if status != p.plum || lifted {
                    let c = contrast(status, p.raised);
                    assert!(c >= 4.5, "{surface:?} {status:06X} raised: {c:.2}");
                }
            }
            // The recording red is a graphic (a dot, a button), 3:1 on the
            // surface and its raised step; its label text keeps 4.5:1.
            for under in [p.surface, p.raised] {
                let c = contrast(REC_RED, under);
                assert!(c >= 3.0, "{surface:?} REC_RED on {under:06X}: {c:.2}");
            }
            let c = contrast(REC_RED, p.surface);
            println!("{surface:?} REC_RED on surface {c:.2}");
            // Raised steps stay a visible step above the surface.
            for step in [p.raised, p.raised_hi, p.track, p.dot_off] {
                assert!(luminance(step) > luminance(p.surface), "{surface:?}");
            }
        }
    }

    /// The notice's ✕ is the text colour on the `raised` disc (`raised_hi`
    /// while the pointer is on it): legible on every island surface, whatever
    /// the see-through fill does behind it (docs/memory/notice-close.md).
    #[test]
    fn the_notice_close_glyph_reads_on_its_disc_on_every_surface() {
        for &surface in Surface::ALL.iter().filter(|s| **s != Surface::Auto) {
            let p = island(Accent::default(), surface);
            let (rest, hot) = (contrast(p.text, p.raised), contrast(p.text, p.raised_hi));
            println!("{surface:?}: text on raised {rest:.2}, on raised_hi {hot:.2}");
            assert!(rest >= 4.5, "{surface:?} raised: {rest:.2}");
            assert!(hot >= 4.5, "{surface:?} raised_hi: {hot:.2}");
        }
        // Auto resolves to one of those.
        let auto = island(Accent::default(), Surface::Auto);
        assert!(contrast(auto.text, auto.raised) >= 4.5);
        assert!(contrast(auto.text, auto.raised_hi) >= 4.5);
    }

    #[test]
    fn each_mascot_has_its_own_coat() {
        use Coat::*;
        let (cat, falk, oct) = (CatColor::Black, FalkColor::Grey, OctopusColor::Blue);
        assert_eq!(Coat::from_parts(Mascot::Cat, cat, falk, oct), Cat(cat));
        assert_eq!(Coat::from_parts(Mascot::Falk, cat, falk, oct), Falk(falk));
        assert_eq!(Coat::from_parts(Mascot::Octopus, cat, falk, oct), Octopus(oct));
        let counts = Mascot::ALL.iter().map(|m| Coat::all_of(*m).len());
        assert_eq!(counts.collect::<Vec<_>>(), [4, 3, 4]);
        for m in Mascot::ALL {
            for c in Coat::all_of(*m) {
                let own = matches!(
                    (m, c),
                    (Mascot::Cat, Cat(_)) | (Mascot::Falk, Falk(_)) | (Mascot::Octopus, Octopus(_))
                );
                assert!(own, "{m:?} {c:?}");
            }
        }
    }

    #[test]
    fn auto_follows_every_mascots_coat() {
        use Coat::*;
        let cases = [
            (Cat(CatColor::Terracotta), Surface::Espresso),
            (Cat(CatColor::Calico), Surface::Sea),
            (Cat(CatColor::White), Surface::Night),
            (Cat(CatColor::Black), Surface::Plum),
            (Falk(FalkColor::Lutino), Surface::Espresso),
            (Falk(FalkColor::Grey), Surface::Night),
            (Falk(FalkColor::WhiteFace), Surface::Plum),
            (Octopus(OctopusColor::Coral), Surface::Espresso),
            (Octopus(OctopusColor::Red), Surface::Graphite),
            (Octopus(OctopusColor::Purple), Surface::Night),
            (Octopus(OctopusColor::Blue), Surface::Sea),
        ];
        for (coat, want) in cases {
            assert_eq!(Surface::Auto.resolve(coat, true), want, "{coat:?}");
            assert_eq!(Surface::Auto.resolve(coat, false), Surface::Espresso);
            assert_ne!(Surface::Auto.resolve(coat, true), Surface::Terminal);
            assert_eq!(Surface::Sea.resolve(coat, true), Surface::Sea);
        }
    }

    #[test]
    fn accessories_keep_their_order_and_names() {
        let names: Vec<_> = CatAccessory::ALL.iter().map(|a| a.name()).collect();
        assert_eq!(
            names,
            ["None", "Glasses", "Bow", "Nightcap", "Scarf", "Flower", "Crown", "Headphone", "Sailor", "Bowtie"]
        );
    }

    #[test]
    fn auto_follows_the_cat_and_never_reaches_the_palette() {
        for (cat, want) in [
            (CatColor::Terracotta, Surface::Espresso),
            (CatColor::Calico, Surface::Sea),
            (CatColor::White, Surface::Night),
            (CatColor::Black, Surface::Plum),
        ] {
            assert_eq!(Surface::Auto.resolve(Coat::Cat(cat), true), want);
            // Auto never gives Terminal, whatever the cat or the robot.
            assert_ne!(Surface::Auto.resolve(Coat::Cat(cat), true), Surface::Terminal);
            assert_ne!(Surface::Auto.resolve(Coat::Cat(cat), false), Surface::Terminal);
            // The cat off: always Espresso.
            assert_eq!(Surface::Auto.resolve(Coat::Cat(cat), false), Surface::Espresso);
            // A chosen surface is itself, whatever the cat.
            for &s in Surface::ALL.iter().filter(|s| **s != Surface::Auto) {
                assert_eq!(s.resolve(Coat::Cat(cat), true), s);
                assert_eq!(s.resolve(Coat::Cat(cat), false), s);
            }
        }
        let cfg = Config {
            cat_color: CatColor::Calico,
            ..Config::default()
        };
        assert_eq!(Theme::of(&cfg).surface, Surface::Sea);
        assert_eq!(
            island_palette(&cfg),
            island(Accent::Terracotta, Surface::Sea)
        );
        let off = Config {
            robot: false,
            ..cfg
        };
        assert_eq!(Theme::of(&off).surface, Surface::Espresso);
        // The coloured surfaces' fills.
        assert_eq!(
            [Surface::Sea, Surface::Night, Surface::Plum].map(|s| s.rgb()),
            [0x132321, 0x151C2E, 0x2A2140]
        );
        assert_eq!(Surface::Terminal.rgb(), 0x1B1D1F);
        assert_eq!(Surface::from_name("terminal"), Some(Surface::Terminal));
        assert_eq!(Surface::from_name("auto"), Some(Surface::Auto));
        assert_eq!(Surface::from_name("Black"), Some(Surface::Espresso));
        assert_eq!(CatColor::default(), CatColor::Terracotta);
        assert_eq!(CatAccessory::default(), CatAccessory::None);
        assert_eq!(CatAccessory::ALL.len(), 10);
    }

    #[test]
    fn graphite_lightens_everything_raised() {
        let espresso = island(Accent::Ochre, Surface::Espresso);
        let graphite = island(Accent::Ochre, Surface::Graphite);
        assert_eq!(graphite.surface, 0x221B17);
        assert_eq!((graphite.accent, graphite.on_accent), (0xD9A441, 0x17120F));
        assert_eq!(graphite.text, espresso.text, "the island stays dark");
        let raised = |p: &Palette| {
            [
                p.raised,
                p.raised_hi,
                p.track,
                p.column,
                p.placeholder,
                p.dot_off,
            ]
        };
        for (e, g) in raised(&espresso).into_iter().zip(raised(&graphite)) {
            assert!(luminance(g) > luminance(e), "{g:06X} vs {e:06X}");
            // Still a visible step above the graphite surface.
            assert!(luminance(g) > luminance(graphite.surface), "{g:06X}");
        }
    }

    #[test]
    fn window_modes_match_the_pinned_hex_values() {
        let dark = window(Accent::Terracotta, WindowTheme::Dark, true);
        assert!(!dark.light);
        assert_eq!(
            (dark.bg, dark.card, dark.border, dark.text, dark.secondary),
            (0x1E1814, 0x272019, 0xF4EDE414, 0xF4EDE4, 0xA89A8B)
        );
        assert_eq!(
            (dark.field, dark.button, dark.button_hover),
            (0x2C241D, 0x33291F, 0x3D3227)
        );
        assert_eq!((dark.accent, dark.on_accent), (0xE2683C, 0x17120F));
        let light = window(Accent::Terracotta, WindowTheme::Light, false);
        assert!(light.light);
        assert_eq!(
            (
                light.bg,
                light.card,
                light.border,
                light.text,
                light.secondary
            ),
            (0xF4EEE6, 0xFFFBF5, 0x221A141A, 0x221A14, 0x786A5C)
        );
        assert_eq!(
            (light.field, light.button, light.button_hover),
            (0xFFFFFF, 0xEFE6DA, 0xEADFD1)
        );
        assert_eq!((light.accent, light.on_accent), (0xC9542B, 0xFFFBF5));
        for p in [dark, light] {
            assert!(contrast(p.text, p.bg) >= 7.0);
            assert!(contrast(p.text, p.field) >= 7.0);
            assert!(contrast(p.secondary, p.bg) >= 4.5);
            assert!(contrast(p.error, p.bg) >= 4.5);
        }
    }

    /// `rgba` (`0xRRGGBBAA`) laid over the opaque `base`.
    fn over(base: u32, rgba: u32) -> u32 {
        let a = (rgba & 0xFF) as f64 / 255.0;
        let rgb = rgba >> 8;
        let ch = |shift: u32| {
            let (b, o) = (
                ((base >> shift) & 0xFF) as f64,
                ((rgb >> shift) & 0xFF) as f64,
            );
            ((b + (o - b) * a).round() as u32) << shift
        };
        ch(16) | ch(8) | ch(0)
    }

    #[test]
    fn editor_tokens_match_the_design_and_read() {
        let dark = window(Accent::Terracotta, WindowTheme::Dark, false);
        assert_eq!(
            (dark.popover, dark.hover, dark.selection, dark.tertiary),
            (0x2A221C, 0x33291F, 0xE2683C29, 0x6E6155)
        );
        let light = window(Accent::Terracotta, WindowTheme::Light, false);
        assert_eq!(
            (light.popover, light.hover, light.selection, light.tertiary),
            (0xFFFBF5, 0xEFE6DA, 0xC9542B1F, 0xA89A8B)
        );
        for a in Accent::ALL {
            for mode in [WindowTheme::Dark, WindowTheme::Light] {
                let p = window(*a, mode, false);
                assert_eq!(p.selection >> 8, p.accent, "{a:?} {mode:?}");
                for surface in [p.popover, p.field, p.bg] {
                    assert!(contrast(p.text, surface) >= 7.0, "{a:?} {mode:?}");
                    assert!(contrast(p.secondary, surface) >= 4.5, "{a:?} {mode:?}");
                    // Placeholders and other-month days: quieter, still visible.
                    let c = contrast(p.tertiary, surface);
                    assert!((2.3..4.5).contains(&c), "{a:?} {mode:?}: {c:.2}");
                }
                // The picked time reads in the accent on its tint.
                let picked = over(p.popover, p.selection);
                let c = contrast(p.accent, picked);
                assert!(c >= 3.0, "{a:?} {mode:?}: {c:.2}");
                // Hover is a visible step off the popover.
                assert_ne!(p.hover, p.popover);
            }
        }
    }

    #[test]
    fn neutral_window_palette_matches_the_pinned_hex_values() {
        let p = window_for(
            Accent::Terracotta,
            Surface::Terminal,
            WindowTheme::Dark,
            false,
        );
        assert_eq!(
            p,
            WindowPalette {
                light: false,
                bg: 0x1B1D1F,
                card: 0x232527,
                border: 0xF4EDE414,
                text: 0xF4EDE4,
                secondary: 0xA9ADB2,
                accent: 0xE2683C,
                on_accent: 0x17120F,
                field: 0x27292B,
                button: 0x2C2E30,
                button_hover: 0x353739,
                error: 0xEF7A6E,
                popover: 0x252729,
                hover: 0x2C2E30,
                selection: 0xE2683C29,
                tertiary: 0x646769,
            }
        );
        // The island's Terminal fill and raised step.
        assert_eq!(p.bg, Surface::Terminal.rgb());
        assert_eq!(
            p.field,
            island(Accent::Terracotta, Surface::Terminal).raised
        );
        assert_eq!(p.secondary, Surface::Terminal.secondary());
    }

    #[test]
    fn terminal_with_dark_or_system_dark_is_neutral_and_light_is_unchanged() {
        for mode in [WindowTheme::Dark, WindowTheme::System] {
            let p = window_for(Accent::Sage, Surface::Terminal, mode, false);
            assert!(!p.light);
            assert_eq!(p.bg, 0x1B1D1F, "{mode:?}");
        }
        let dark = window_for(Accent::Sage, Surface::Terminal, WindowTheme::Dark, true);
        assert_eq!(dark.bg, 0x1B1D1F);
        for (mode, sys) in [
            (WindowTheme::Light, false),
            (WindowTheme::Light, true),
            (WindowTheme::System, true),
        ] {
            let p = window_for(Accent::Sage, Surface::Terminal, mode, sys);
            assert!(p.light);
            assert_eq!(p, window(Accent::Sage, mode, sys), "{mode:?} {sys}");
        }
    }

    #[test]
    fn every_surface_but_terminal_keeps_the_warm_window_palettes() {
        for a in Accent::ALL {
            for s in Surface::ALL.iter().filter(|s| **s != Surface::Terminal) {
                for mode in [WindowTheme::Dark, WindowTheme::Light, WindowTheme::System] {
                    for sys in [false, true] {
                        assert_eq!(
                            window_for(*a, *s, mode, sys),
                            window(*a, mode, sys),
                            "{a:?} {s:?} {mode:?} {sys}"
                        );
                    }
                }
            }
        }
        // The warm dark one stays what it was.
        assert_eq!(
            window_for(Accent::Terracotta, Surface::Plum, WindowTheme::Dark, false).bg,
            0x1E1814
        );
    }

    #[test]
    fn neutral_window_palette_reads_for_every_accent() {
        let warm = window(Accent::Terracotta, WindowTheme::Dark, false);
        let warm_tertiary = contrast(warm.tertiary, warm.bg);
        for a in Accent::ALL {
            let p = window_for(*a, Surface::Terminal, WindowTheme::Dark, false);
            for base in [p.bg, p.card, p.field, p.popover] {
                assert!(contrast(p.text, base) >= 7.0, "{a:?} text on {base:06X}");
                assert!(contrast(p.secondary, base) >= 4.5, "{a:?} {base:06X}");
            }
            assert!(contrast(p.error, p.bg) >= 4.5, "{a:?}");
            assert!(contrast(p.on_accent, p.accent) >= 4.5, "{a:?}");
            assert_eq!(p.selection >> 8, p.accent, "{a:?}");
            // Placeholders: the same quiet class as the warm palette's.
            let c = contrast(p.tertiary, p.bg);
            assert!(
                (c - warm_tertiary).abs() <= 0.15,
                "{c:.2} vs {warm_tertiary:.2}"
            );
            assert!(c < 4.5 && warm_tertiary < 4.5);
            // The picked time reads in the accent on its tint.
            assert!(
                contrast(p.accent, over(p.popover, p.selection)) >= 3.0,
                "{a:?}"
            );
            assert_ne!(p.hover, p.popover);
        }
    }

    #[test]
    fn neutral_window_palette_has_no_warm_tint() {
        let p = window_for(
            Accent::Terracotta,
            Surface::Terminal,
            WindowTheme::Dark,
            false,
        );
        for (name, c) in [
            ("bg", p.bg),
            ("card", p.card),
            ("field", p.field),
            ("popover", p.popover),
            ("button", p.button),
            ("button_hover", p.button_hover),
            ("hover", p.hover),
            ("secondary", p.secondary),
            ("tertiary", p.tertiary),
        ] {
            let (r, g, b) = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
            let spread = r.max(g).max(b) - r.min(g).min(b);
            assert!(spread <= 9, "{name} {c:06X}: spread {spread}");
            assert!(r <= b, "{name} {c:06X} is warm-tinted");
        }
    }

    #[test]
    fn system_follows_the_windows_flag() {
        let sys = |light| window(Accent::Sage, WindowTheme::System, light);
        assert_eq!(sys(true), window(Accent::Sage, WindowTheme::Light, false));
        assert_eq!(sys(false), window(Accent::Sage, WindowTheme::Dark, true));
        let cfg = Config::default();
        assert_eq!(cfg.window_theme, WindowTheme::System);
        let t = Theme::of(&cfg);
        assert!(window(t.accent, t.window, true).light);
        assert!(!window(t.accent, t.window, false).light);
        assert_eq!(
            island_palette(&cfg),
            island(Accent::Terracotta, Surface::Espresso)
        );
    }

    #[test]
    fn names_round_trip_and_unknown_names_are_rejected() {
        for a in Accent::ALL {
            assert_eq!(Accent::from_name(a.name()), Some(*a));
        }
        assert_eq!(Accent::from_name("ochre"), Some(Accent::Ochre));
        assert_eq!(Surface::from_name(" GRAPHITE "), Some(Surface::Graphite));
        assert_eq!(WindowTheme::from_name("light"), Some(WindowTheme::Light));
        assert_eq!(Accent::from_name("teal"), None);
        assert_eq!(Surface::from_name("grey"), None);
        assert_eq!(WindowTheme::from_name("black"), None);
    }

    #[test]
    fn old_names_read_as_their_studio_colours() {
        for (old, new) in [
            ("Blue", Accent::Terracotta),
            ("Green", Accent::Sage),
            ("Orange", Accent::Ochre),
            ("Pink", Accent::Clay),
            ("Purple", Accent::Plum),
            ("Red", Accent::Terracotta),
        ] {
            assert_eq!(Accent::from_name(old), Some(new), "{old}");
            let shouted = format!("  {}  ", old.to_uppercase());
            assert_eq!(Accent::from_name(&shouted), Some(new), "{old}");
        }
        assert_eq!(Surface::from_name("black"), Some(Surface::Espresso));
        // Aliases are read, never written.
        for a in Accent::ALL {
            assert!(Accent::ALIASES.iter().all(|(old, _)| *old != a.name()));
        }
        assert_eq!(
            serde_json::to_string(&Accent::from_name("Blue").unwrap()).unwrap(),
            "\"Terracotta\""
        );
        assert_eq!(
            serde_json::from_str::<Surface>("\"Black\"").unwrap(),
            Surface::Espresso
        );
    }

    #[test]
    fn terminal_surface_is_neutral_and_keeps_its_index_last() {
        // Appended last: the older surfaces keep their packed indices.
        assert_eq!(Surface::Terminal.index(), 6);
        assert_eq!(Surface::Plum.index(), 5);
        assert_eq!(Surface::ALL.last(), Some(&Surface::Terminal));
        let t = Theme {
            accent: Accent::Terracotta,
            surface: Surface::Terminal,
            window: WindowTheme::Dark,
        };
        assert_eq!(unpack(pack(t)), t);
        assert_eq!(unpack(pack(t)).surface, Surface::Terminal);
        // Neutral: every derived colour's channels stay within the source's
        // own spread (0x1B1D1F: 4).
        let spread = |c: u32| {
            let ch = [(c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF];
            ch.iter().max().unwrap() - ch.iter().min().unwrap()
        };
        let p = island(Accent::Terracotta, Surface::Terminal);
        assert_eq!(spread(p.surface), 4);
        for c in [
            p.raised,
            p.raised_hi,
            p.track,
            p.column,
            p.placeholder,
            p.dot_off,
        ] {
            assert!(spread(c) <= 4, "{c:06X}");
        }
        // The caption grey is a touch cooler (spread 9) but still reads grey.
        assert!(spread(p.secondary) <= 9);
        assert_eq!(p.raised, 0x27292B);
        assert_eq!(p.plum, 0xC893B4);
        assert!(contrast(p.secondary, p.surface) >= 4.5);
        assert!(contrast(p.secondary, p.raised) >= 4.5);
    }

    #[test]
    fn theme_packs_losslessly() {
        for &accent in Accent::ALL {
            for &surface in Surface::ALL {
                for &window in WindowTheme::ALL {
                    let t = Theme {
                        accent,
                        surface,
                        window,
                    };
                    assert_eq!(unpack(pack(t)), t);
                }
            }
        }
    }
}
