//! A sticky note's desktop side: its paper, text size, pin, roll-up and where
//! it sits. A note is an ordinary note (`notes::Note`) that also has one of
//! these; taking it off the desktop removes only this part.
//!
//! The data is read leniently (`lenient`): a malformed object never makes
//! notes.json unreadable. A field that is wrong gets its default, a rect is
//! clamped to a usable size, and something that is not an object is no sticky.

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::i18n::{self, Key, Lang, text};
use crate::theme::{Accent, Palette};
use crate::widgets::checklist;

/// At most this many notes on the desktop.
pub const MAX_STICKY: usize = 20;

/// Whether one more note fits, given how many there are.
pub fn can_add(count: usize) -> bool {
    count < MAX_STICKY
}

/// The smallest and the largest a note can be, and the size a new one has
/// (dip).
pub const MIN_W: f32 = 160.0;
pub const MIN_H: f32 = 120.0;
pub const MAX_W: f32 = 640.0;
pub const MAX_H: f32 = 800.0;
pub const DEFAULT_W: f32 = 240.0;
pub const DEFAULT_H: f32 = 208.0;
/// A rolled-up note: one line tall.
pub const ROLLED_H: f32 = 56.0;

/// A rectangle in device-independent pixels. For a sticky it is relative to
/// the top-left of its monitor's placement area.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DipRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl DipRect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> DipRect {
        DipRect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// The overlap area with `other` is not empty.
    pub fn intersects(&self, other: &DipRect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

/// The colour of a note's paper.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Paper {
    #[default]
    Krem,
    Terakota,
    Sage,
    Plum,
    Laut,
    /// Vale's own surface, with an accent strip on top.
    Gelap,
}

impl Paper {
    pub const ALL: [Paper; 6] = [
        Paper::Krem,
        Paper::Terakota,
        Paper::Sage,
        Paper::Plum,
        Paper::Laut,
        Paper::Gelap,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Paper::Krem => "Krem",
            Paper::Terakota => "Terakota",
            Paper::Sage => "Sage",
            Paper::Plum => "Plum",
            Paper::Laut => "Laut",
            Paper::Gelap => "Gelap",
        }
    }

    pub fn from_name(name: &str) -> Option<Paper> {
        let name = name.trim();
        Paper::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(name))
    }

    pub fn label_key(self) -> Key {
        match self {
            Paper::Krem => Key::PaperKrem,
            Paper::Terakota => Key::PaperTerakota,
            Paper::Sage => Key::PaperSage,
            Paper::Plum => Key::PaperPlum,
            Paper::Laut => Key::PaperLaut,
            Paper::Gelap => Key::PaperGelap,
        }
    }

    /// The pastel's fill; `Gelap` has none of its own (the island's surface).
    fn pastel(self) -> Option<u32> {
        match self {
            Paper::Krem => Some(0xF4E7C6),
            Paper::Terakota => Some(0xF3CDBB),
            Paper::Sage => Some(0xD3DFCF),
            Paper::Plum => Some(0xE6CFDC),
            Paper::Laut => Some(0xCCDCE4),
            Paper::Gelap => None,
        }
    }
}

/// The body text size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum TextSize {
    S,
    #[default]
    M,
    L,
}

impl TextSize {
    pub const ALL: [TextSize; 3] = [TextSize::S, TextSize::M, TextSize::L];

    pub fn name(self) -> &'static str {
        match self {
            TextSize::S => "S",
            TextSize::M => "M",
            TextSize::L => "L",
        }
    }

    pub fn from_name(name: &str) -> Option<TextSize> {
        let name = name.trim();
        TextSize::ALL
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(name))
    }

    /// Body text, in dip.
    pub fn body(self) -> f32 {
        match self {
            TextSize::S => 12.0,
            TextSize::M => 13.5,
            TextSize::L => 15.5,
        }
    }

    /// The title line, semibold and a little larger.
    pub fn title(self) -> f32 {
        match self {
            TextSize::S => 13.0,
            TextSize::M => 15.0,
            TextSize::L => 17.5,
        }
    }

    /// One line's height.
    pub fn line(self) -> f32 {
        (self.body() * 1.42).round()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Sticky {
    pub paper: Paper,
    pub text_size: TextSize,
    pub pinned: bool,
    pub rolled: bool,
    pub rect: DipRect,
    /// The device path of the monitor it sits on; "" is the primary one.
    pub monitor: String,
    /// Unix seconds of the last touch: the most recent one gets the cat.
    pub touched: i64,
}

impl Default for Sticky {
    fn default() -> Self {
        Sticky {
            paper: Paper::Krem,
            text_size: TextSize::M,
            pinned: false,
            rolled: false,
            rect: DipRect::new(0.0, 0.0, DEFAULT_W, DEFAULT_H),
            monitor: String::new(),
            touched: 0,
        }
    }
}

/// `v` as a finite number, else `default`.
fn number(v: Option<&Value>, default: f32) -> f32 {
    v.and_then(Value::as_f64)
        .map(|n| n as f32)
        .filter(|n| n.is_finite())
        .unwrap_or(default)
}

/// A rect with a size inside `MIN_*..=MAX_*` and a finite place.
fn clamp_size(r: DipRect) -> DipRect {
    DipRect {
        x: if r.x.is_finite() { r.x } else { 0.0 },
        y: if r.y.is_finite() { r.y } else { 0.0 },
        w: r.w.clamp(MIN_W, MAX_W),
        h: r.h.clamp(MIN_H, MAX_H),
    }
}

/// A sticky from a JSON value, field by field. `None` when it is not an object.
pub fn lenient(v: &Value) -> Option<Sticky> {
    let map = v.as_object()?;
    let d = Sticky::default();
    let flag = |key: &str| map.get(key).and_then(Value::as_bool).unwrap_or(false);
    let rect = map.get("rect").and_then(Value::as_object);
    let at = |key: &str, default: f32| number(rect.and_then(|r| r.get(key)), default);
    Some(Sticky {
        paper: map
            .get("paper")
            .and_then(Value::as_str)
            .and_then(Paper::from_name)
            .unwrap_or(d.paper),
        text_size: map
            .get("text_size")
            .and_then(Value::as_str)
            .and_then(TextSize::from_name)
            .unwrap_or(d.text_size),
        pinned: flag("pinned"),
        rolled: flag("rolled"),
        rect: clamp_size(DipRect {
            x: at("x", 0.0),
            y: at("y", 0.0),
            w: at("w", DEFAULT_W),
            h: at("h", DEFAULT_H),
        }),
        monitor: map
            .get("monitor")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        touched: map.get("touched").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// For `Note::sticky`: never an error.
pub fn deserialize_lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Sticky>, D::Error> {
    Ok(lenient(&Value::deserialize(d)?))
}

// ------------------------------------------------------------------ colours

/// The colours a note is drawn in, all opaque.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tones {
    pub fill: u32,
    pub title: u32,
    pub body: u32,
    /// The date, the footer and the header icons at rest.
    pub secondary: u32,
    /// A done item: dimmed and struck through.
    pub done: u32,
    /// Pin, guides, the highlight frame: the accent that reads on this paper.
    pub accent: u32,
    /// The strip along the top of a Gelap note.
    pub strip: Option<u32>,
}

/// The ink of the pastel papers.
pub const INK: u32 = 0x2B211A;

/// `fg` over `bg` at `alpha`, as an opaque colour.
pub fn blend(fg: u32, bg: u32, alpha: f32) -> u32 {
    let ch = |shift: u32| {
        let f = ((fg >> shift) & 0xFF) as f32;
        let b = ((bg >> shift) & 0xFF) as f32;
        ((f * alpha + b * (1.0 - alpha)).round() as u32).min(255) << shift
    };
    ch(16) | ch(8) | ch(0)
}

/// How much of the ink the secondary text and a done item keep.
const SECONDARY_ALPHA: f32 = 0.72;
const DONE_ALPHA: f32 = 0.62;

/// `paper`'s colours under the island palette `island` and the accent. Every
/// text tone holds 4.5:1 on the fill (a test checks every paper on every
/// surface); the accent holds 3:1.
pub fn tones(paper: Paper, island: &Palette, accent: Accent) -> Tones {
    match paper.pastel() {
        Some(fill) => Tones {
            fill,
            title: INK,
            body: INK,
            secondary: blend(INK, fill, SECONDARY_ALPHA),
            done: blend(INK, fill, DONE_ALPHA),
            // The darker accent of the light windows reads on a pastel.
            accent: accent.light_rgb(),
            strip: None,
        },
        None => Tones {
            fill: island.surface,
            title: island.text,
            body: island.text,
            secondary: island.secondary,
            done: blend(island.text, island.surface, DONE_ALPHA),
            accent: island.accent,
            strip: Some(island.accent),
        },
    }
}

// ------------------------------------------------------------------ labels

/// "2 dari 4 · diubah 13.22" for a checklist, else the edit time as the
/// Notes page words it ("hari ini 13.22", "kemarin", "23 Okt").
pub fn footer_label(lang: Lang, note_text: &str, updated: NaiveDateTime, today: NaiveDate) -> String {
    let (done, total) = checklist::counts(note_text);
    if total == 0 {
        return crate::notes::note_time_label(lang, updated, today);
    }
    format!(
        "{done} {} {total} \u{b7} {} {}",
        text(lang, Key::StickyOf),
        text(lang, Key::StickyEdited),
        i18n::time_hm(lang, updated),
    )
}

/// "5 baris" / "1 line": the lines under a rolled-up note's title.
pub fn rolled_label(lang: Lang, note_text: &str) -> String {
    let n = note_text
        .split('\n')
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .count();
    if n == 0 {
        return String::new();
    }
    let word = if n == 1 {
        text(lang, Key::StickyLineOne)
    } else {
        text(lang, Key::StickyLineMany)
    };
    format!("{n} {word}")
}

/// "3 di desktop" / "3 on desktop".
pub fn on_desktop_label(lang: Lang, n: usize) -> String {
    format!("{n} {}", text(lang, Key::StickyOnDesktop))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Surface, WindowTheme, island, window_for};
    use serde_json::json;

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

    const SURFACES: [Surface; 6] = [
        Surface::Espresso,
        Surface::Graphite,
        Surface::Sea,
        Surface::Night,
        Surface::Plum,
        Surface::Terminal,
    ];

    #[test]
    fn every_text_tone_reads_on_every_paper_and_surface() {
        for surface in SURFACES {
            for accent in Accent::ALL {
                let island = island(*accent, surface);
                for paper in Paper::ALL {
                    let t = tones(paper, &island, *accent);
                    for (name, ink) in [
                        ("title", t.title),
                        ("body", t.body),
                        ("secondary", t.secondary),
                        ("done", t.done),
                    ] {
                        let c = contrast(ink, t.fill);
                        assert!(
                            c >= 4.5,
                            "{paper:?} {name} on {surface:?}/{accent:?}: {c:.2}"
                        );
                    }
                    let c = contrast(t.accent, t.fill);
                    assert!(c >= 3.0, "{paper:?} accent on {surface:?}/{accent:?}: {c:.2}");
                }
            }
        }
    }

    #[test]
    fn the_surfaces_popovers_read_in_both_window_modes() {
        // The colour popover and the menus use the window palette; its text
        // must read on its popover fill in light and dark, on every surface.
        for surface in SURFACES {
            for mode in [WindowTheme::Light, WindowTheme::Dark] {
                let p = window_for(Accent::Terracotta, surface, mode, false);
                assert!(contrast(p.text, p.popover) >= 4.5, "{surface:?} {mode:?}");
                assert!(contrast(p.secondary, p.popover) >= 4.5, "{surface:?} {mode:?}");
            }
        }
    }

    #[test]
    fn gelap_follows_the_island_and_has_a_strip() {
        let island = island(Accent::Sage, Surface::Terminal);
        let t = tones(Paper::Gelap, &island, Accent::Sage);
        assert_eq!(t.fill, 0x1B1D1F, "Terminal's flat grey");
        assert_eq!(t.body, 0xF4EDE4);
        assert_eq!(t.strip, Some(Accent::Sage.rgb()));
        let krem = tones(Paper::Krem, &island, Accent::Sage);
        assert_eq!(krem.fill, 0xF4E7C6);
        assert_eq!(krem.title, 0x2B211A);
        assert_eq!(krem.strip, None);
    }

    #[test]
    fn the_pastels_are_the_documented_hex_values() {
        let fills: Vec<u32> = Paper::ALL.iter().filter_map(|p| p.pastel()).collect();
        assert_eq!(fills, [0xF4E7C6, 0xF3CDBB, 0xD3DFCF, 0xE6CFDC, 0xCCDCE4]);
    }

    #[test]
    fn the_cap_is_twenty() {
        assert!(can_add(0));
        assert!(can_add(19));
        assert!(!can_add(20));
        assert!(!can_add(21));
        assert_eq!(MAX_STICKY, 20);
    }

    #[test]
    fn names_round_trip() {
        for p in Paper::ALL {
            assert_eq!(Paper::from_name(p.name()), Some(p));
            assert_eq!(Paper::from_name(&p.name().to_uppercase()), Some(p));
        }
        for s in TextSize::ALL {
            assert_eq!(TextSize::from_name(s.name()), Some(s));
        }
        assert_eq!(Paper::from_name("Pink"), None);
    }

    #[test]
    fn a_sticky_round_trips_through_json() {
        let s = Sticky {
            paper: Paper::Plum,
            text_size: TextSize::L,
            pinned: true,
            rolled: true,
            rect: DipRect::new(16.0, 24.5, 300.0, 200.0),
            monitor: r"\\?\DISPLAY#X".into(),
            touched: 1_700_000_000,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(lenient(&json), Some(s));
    }

    #[test]
    fn a_malformed_sticky_reads_as_defaults_or_none() {
        assert_eq!(lenient(&json!(null)), None);
        assert_eq!(lenient(&json!("x")), None);
        assert_eq!(lenient(&json!([1])), None);
        let d = lenient(&json!({})).unwrap();
        assert_eq!(d, Sticky::default());
        let s = lenient(&json!({
            "paper": 5, "text_size": "XL", "pinned": "yes", "rolled": 1,
            "rect": {"x": "a", "y": null, "w": 1.0, "h": 99999},
            "monitor": 3, "touched": "now",
        }))
        .unwrap();
        assert_eq!(s.paper, Paper::Krem);
        assert_eq!(s.text_size, TextSize::M);
        assert!(!s.pinned && !s.rolled);
        assert_eq!(s.rect, DipRect::new(0.0, 0.0, MIN_W, MAX_H), "clamped, place 0");
        assert_eq!(s.monitor, "");
        assert_eq!(s.touched, 0);
        // A rect that is not an object is the default size.
        let s = lenient(&json!({"rect": [1, 2]})).unwrap();
        assert_eq!(s.rect, Sticky::default().rect);
    }

    #[test]
    fn labels() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        let at = today.and_hms_opt(13, 22, 0).unwrap();
        let list = "Belanja\n- [x] a\n- [ ] b\n- [x] c\n- [ ] d";
        assert_eq!(footer_label(Lang::Id, list, at, today), "2 dari 4 \u{b7} diubah 13.22");
        assert_eq!(footer_label(Lang::En, list, at, today), "2 of 4 \u{b7} edited 13:22");
        assert_eq!(footer_label(Lang::Id, "plain", at, today), "hari ini 13.22");
        assert_eq!(rolled_label(Lang::Id, "T\na\nb\n\nc"), "3 baris");
        assert_eq!(rolled_label(Lang::En, "T\na"), "1 line");
        assert_eq!(rolled_label(Lang::En, "T\na\nb"), "2 lines");
        assert_eq!(rolled_label(Lang::En, "T"), "");
        assert_eq!(on_desktop_label(Lang::Id, 3), "3 di desktop");
        assert_eq!(on_desktop_label(Lang::En, 0), "0 on desktop");
    }

    #[test]
    fn text_sizes_grow() {
        assert!(TextSize::S.body() < TextSize::M.body() && TextSize::M.body() < TextSize::L.body());
        for s in TextSize::ALL {
            assert!(s.title() > s.body());
            assert!(s.line() > s.body());
        }
    }
}
