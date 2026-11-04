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
use crate::widgets::tuck::Edge;

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
/// A rolled-up note: the 36 dip strip and one 28 dip line.
pub const ROLLED_H: f32 = 64.0;

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

fn is_false(b: &bool) -> bool {
    !*b
}

/// Which typeface a note's text is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
pub enum NoteFont {
    /// Vale's own font (the Font setting).
    #[default]
    Vale,
    /// Handwriting: Ink Free, else Segoe Print, else Vale's font.
    Tangan,
    /// The bundled DM Mono.
    Mono,
}

impl NoteFont {
    pub const ALL: [NoteFont; 3] = [NoteFont::Vale, NoteFont::Tangan, NoteFont::Mono];

    pub fn name(self) -> &'static str {
        match self {
            NoteFont::Vale => "Vale",
            NoteFont::Tangan => "Tangan",
            NoteFont::Mono => "Mono",
        }
    }

    pub fn from_name(name: &str) -> Option<NoteFont> {
        let name = name.trim();
        NoteFont::ALL
            .into_iter()
            .find(|f| f.name().eq_ignore_ascii_case(name))
    }

    pub fn label_key(self) -> Key {
        match self {
            NoteFont::Vale => Key::FontVale,
            NoteFont::Tangan => Key::FontTangan,
            NoteFont::Mono => Key::FontMono,
        }
    }
}

/// The handwriting family to use: Ink Free when `has` says the system has it,
/// else Segoe Print, else none (Vale's own face stands in).
pub fn tangan_family(has: impl Fn(&str) -> bool) -> Option<&'static str> {
    ["Ink Free", "Segoe Print"].into_iter().find(|f| has(f))
}

/// The faint ruling behind the text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Pattern {
    #[default]
    Polos,
    Garis,
    Kotak,
    Titik,
}

impl Pattern {
    pub const ALL: [Pattern; 4] = [Pattern::Polos, Pattern::Garis, Pattern::Kotak, Pattern::Titik];

    pub fn name(self) -> &'static str {
        match self {
            Pattern::Polos => "Polos",
            Pattern::Garis => "Garis",
            Pattern::Kotak => "Kotak",
            Pattern::Titik => "Titik",
        }
    }

    pub fn from_name(name: &str) -> Option<Pattern> {
        let name = name.trim();
        Pattern::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(name))
    }

    pub fn label_key(self) -> Key {
        match self {
            Pattern::Polos => Key::PatternPolos,
            Pattern::Garis => Key::PatternGaris,
            Pattern::Kotak => Key::PatternKotak,
            Pattern::Titik => Key::PatternTitik,
        }
    }
}

/// What holds a note to the wall, drawn in the window's top margin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Deco {
    #[default]
    None,
    /// A striped washi tape strip across the top.
    Selotip,
    /// A round push pin.
    Paku,
}

impl Deco {
    pub const ALL: [Deco; 3] = [Deco::None, Deco::Selotip, Deco::Paku];

    pub fn name(self) -> &'static str {
        match self {
            Deco::None => "None",
            Deco::Selotip => "Selotip",
            Deco::Paku => "Paku",
        }
    }

    pub fn from_name(name: &str) -> Option<Deco> {
        let name = name.trim();
        Deco::ALL
            .into_iter()
            .find(|d| d.name().eq_ignore_ascii_case(name))
    }

    pub fn label_key(self) -> Key {
        match self {
            Deco::None => Key::DecoNone,
            Deco::Selotip => Key::DecoSelotip,
            Deco::Paku => Key::DecoPaku,
        }
    }
}

/// An optional round stamp in the corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Stamp {
    #[default]
    None,
    Penting,
    Beres,
    Mendesak,
    Suka,
    Nanti,
    Ide,
}

impl Stamp {
    pub const ALL: [Stamp; 7] = [
        Stamp::None,
        Stamp::Penting,
        Stamp::Beres,
        Stamp::Mendesak,
        Stamp::Suka,
        Stamp::Nanti,
        Stamp::Ide,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stamp::None => "None",
            Stamp::Penting => "Penting",
            Stamp::Beres => "Beres",
            Stamp::Mendesak => "Mendesak",
            Stamp::Suka => "Suka",
            Stamp::Nanti => "Nanti",
            Stamp::Ide => "Ide",
        }
    }

    pub fn from_name(name: &str) -> Option<Stamp> {
        let name = name.trim();
        Stamp::ALL
            .into_iter()
            .find(|s| s.name().eq_ignore_ascii_case(name))
    }

    pub fn label_key(self) -> Key {
        match self {
            Stamp::None => Key::StampNone,
            Stamp::Penting => Key::StampPenting,
            Stamp::Beres => Key::StampBeres,
            Stamp::Mendesak => Key::StampMendesak,
            Stamp::Suka => Key::StampSuka,
            Stamp::Nanti => Key::StampNanti,
            Stamp::Ide => Key::StampIde,
        }
    }
}

/// The see-through choices while the pointer is away, in percent.
pub const OPACITIES: [u8; 4] = [100, 80, 60, 40];

/// The allowed opacity nearest to `n` (a tie goes to the more opaque one).
pub fn snap_opacity(n: i64) -> u8 {
    // Any file can hold any number: clamped, the distance never overflows.
    let n = n.clamp(0, 100);
    let mut best = OPACITIES[0];
    for o in OPACITIES {
        if (i64::from(o) - n).abs() < (i64::from(best) - n).abs() {
            best = o;
        }
    }
    best
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

fn is_full(v: &u8) -> bool {
    *v == 100
}

fn is_zero(v: &u32) -> bool {
    *v == 0
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
    /// "Ingatkan aku": unix seconds the note reminds at. Not written while
    /// there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remind_at: Option<i64>,
    /// The reminder went off (the note was raised and the island told). Until
    /// the note is opened, or the reminder cleared or snoozed, it is overdue.
    /// Not written while false.
    #[serde(skip_serializing_if = "is_false")]
    pub remind_fired: bool,
    /// The typeface of the text. Not written at the default (Vale).
    #[serde(skip_serializing_if = "is_default")]
    pub font: NoteFont,
    /// The ruling behind the text. Not written at the default (Polos).
    #[serde(skip_serializing_if = "is_default")]
    pub pattern: Pattern,
    /// Tape or a pin. Not written at the default (none).
    #[serde(skip_serializing_if = "is_default")]
    pub deco: Deco,
    /// A stamp. Not written at the default (none).
    #[serde(skip_serializing_if = "is_default")]
    pub stamp: Stamp,
    /// How opaque the note is while the pointer is away: 100, 80, 60 or 40.
    /// Not written at 100.
    #[serde(skip_serializing_if = "is_full")]
    pub opacity: u8,
    /// The stack this sheet belongs to (`widgets::stack`). Not written when
    /// the note is on its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    /// The sheet's place in its stack, 0-based. Not written at 0.
    #[serde(skip_serializing_if = "is_zero")]
    pub stack_order: u32,
    /// The sheet the stack shows. Not written when false.
    #[serde(skip_serializing_if = "is_false")]
    pub stack_top: bool,
    /// The screen edge the note is tucked to (`widgets::tuck`). Not written
    /// while it is out in the open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tuck: Option<Edge>,
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
            remind_at: None,
            remind_fired: false,
            font: NoteFont::Vale,
            pattern: Pattern::Polos,
            deco: Deco::None,
            stamp: Stamp::None,
            opacity: 100,
            stack: None,
            stack_order: 0,
            stack_top: false,
            tuck: None,
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

/// The enum named by string `key` of `map`, or `default`.
fn name_of<T>(
    map: &serde_json::Map<String, Value>,
    key: &str,
    from_name: impl Fn(&str) -> Option<T>,
    default: T,
) -> T {
    map.get(key)
        .and_then(Value::as_str)
        .and_then(from_name)
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
    let remind_at = map.get("remind_at").and_then(Value::as_i64);
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
        remind_at,
        // A fired flag without a time means nothing.
        remind_fired: flag("remind_fired") && remind_at.is_some(),
        font: name_of(map, "font", NoteFont::from_name, d.font),
        pattern: name_of(map, "pattern", Pattern::from_name, d.pattern),
        deco: name_of(map, "deco", Deco::from_name, d.deco),
        stamp: name_of(map, "stamp", Stamp::from_name, d.stamp),
        // Any number snaps to the nearest choice; anything else is full.
        opacity: map
            .get("opacity")
            .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64)))
            .map_or(d.opacity, snap_opacity),
        // A stack id is a non-empty string; a number is that many places
        // (negative is none); the edge is any case and a wrong name is none.
        stack: map
            .get("stack")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        stack_order: map
            .get("stack_order")
            .and_then(Value::as_u64)
            .map_or(0, |n| n.min(u64::from(u32::MAX)) as u32),
        stack_top: flag("stack_top"),
        tuck: map
            .get("tuck")
            .and_then(Value::as_str)
            .and_then(Edge::from_name),
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
    /// The faint ruling of Garis, Kotak and Titik: a hair off the fill, so
    /// every text tone keeps 4.5:1 over it.
    pub line: u32,
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

/// How much ink (or text colour on Gelap) the ruling takes.
const RULING_ALPHA: f32 = 0.08;
const RULING_ALPHA_DARK: f32 = 0.10;

/// How much of the ink the secondary text and a done item keep.
const SECONDARY_ALPHA: f32 = 0.82;
const DONE_ALPHA: f32 = 0.74;

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
            // The darker accent of the light windows, a touch toward the ink,
            // reads on every pastel (the Terakota paper is the hardest).
            accent: blend(INK, accent.light_rgb(), 0.22),
            strip: None,
            line: blend(INK, fill, RULING_ALPHA),
        },
        None => Tones {
            fill: island.surface,
            title: island.text,
            body: island.text,
            secondary: island.secondary,
            done: blend(island.text, island.surface, DONE_ALPHA),
            accent: island.accent,
            strip: Some(island.accent),
            line: blend(island.text, island.surface, RULING_ALPHA_DARK),
        },
    }
}

/// WCAG relative luminance of an `0xRRGGBB` colour.
pub fn luminance(rgb: u32) -> f64 {
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

/// WCAG contrast ratio of two opaque colours.
pub fn contrast(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// White or black, whichever reads better on `fill` (at least 4.5:1 on any
/// fill: the two cross over at 4.58:1).
pub fn readable_on(fill: u32) -> u32 {
    if contrast(0xFFFFFF, fill) >= contrast(0x000000, fill) {
        0xFFFFFF
    } else {
        0x000000
    }
}

/// The header's reminder chip, `(fill, ink)`: a faint tint of the text at
/// rest, the paper's accent once the reminder is overdue. The ink holds 4.5:1
/// on its fill on every paper and surface (a test checks).
pub fn chip_tones(t: &Tones, overdue: bool) -> (u32, u32) {
    if overdue {
        (t.accent, readable_on(t.accent))
    } else {
        (blend(t.body, t.fill, 0.12), t.body)
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
    fn the_reminder_chip_reads_on_every_paper_and_surface() {
        for surface in SURFACES {
            for accent in Accent::ALL {
                let island = island(*accent, surface);
                for paper in Paper::ALL {
                    let t = tones(paper, &island, *accent);
                    for overdue in [false, true] {
                        let (fill, ink) = chip_tones(&t, overdue);
                        let c = contrast(ink, fill);
                        assert!(
                            c >= 4.5,
                            "{paper:?} chip (overdue {overdue}) on {surface:?}/{accent:?}: {c:.2}"
                        );
                    }
                    // The overdue chip stands out of the paper.
                    let (fill, _) = chip_tones(&t, true);
                    assert!(contrast(fill, t.fill) >= 3.0, "{paper:?} {surface:?}/{accent:?}");
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
            remind_at: Some(1_700_003_600),
            remind_fired: true,
            font: NoteFont::Tangan,
            pattern: Pattern::Kotak,
            deco: Deco::Paku,
            stamp: Stamp::Mendesak,
            opacity: 60,
            stack: Some("stk-n1700-1".into()),
            stack_order: 2,
            stack_top: true,
            tuck: Some(Edge::Right),
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(lenient(&json), Some(s));
    }

    #[test]
    fn every_new_field_round_trips_on_its_own() {
        for font in NoteFont::ALL {
            let s = Sticky { font, ..Sticky::default() };
            assert_eq!(lenient(&serde_json::to_value(&s).unwrap()), Some(s));
        }
        for pattern in Pattern::ALL {
            let s = Sticky { pattern, ..Sticky::default() };
            assert_eq!(lenient(&serde_json::to_value(&s).unwrap()), Some(s));
        }
        for deco in Deco::ALL {
            let s = Sticky { deco, ..Sticky::default() };
            assert_eq!(lenient(&serde_json::to_value(&s).unwrap()), Some(s));
        }
        for stamp in Stamp::ALL {
            let s = Sticky { stamp, ..Sticky::default() };
            assert_eq!(lenient(&serde_json::to_value(&s).unwrap()), Some(s));
        }
        for opacity in OPACITIES {
            let s = Sticky { opacity, ..Sticky::default() };
            assert_eq!(lenient(&serde_json::to_value(&s).unwrap()), Some(s));
        }
        let s = Sticky { remind_at: Some(42), remind_fired: true, ..Sticky::default() };
        assert_eq!(lenient(&serde_json::to_value(&s).unwrap()), Some(s));
    }

    #[test]
    fn a_default_sticky_writes_no_new_key_and_an_old_file_saves_back_unchanged() {
        let json = serde_json::to_value(Sticky::default()).unwrap();
        let keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        for new in [
            "remind_at", "remind_fired", "font", "pattern", "deco", "stamp", "opacity",
            "stack", "stack_order", "stack_top", "tuck",
        ] {
            assert!(!keys.contains(&new), "{new} written at its default: {keys:?}");
        }
        // The phase-1 keys are all still there.
        for old in ["paper", "text_size", "pinned", "rolled", "rect", "monitor", "touched"] {
            assert!(keys.contains(&old), "{old}");
        }
        // A phase-1 file: loads to the defaults and saves back with the same keys.
        let phase1 = json!({
            "paper": "Sage", "text_size": "L", "pinned": true, "rolled": false,
            "rect": {"x": 8.0, "y": 8.0, "w": 240.0, "h": 208.0},
            "monitor": "", "touched": 5,
        });
        let s = lenient(&phase1).unwrap();
        assert_eq!(s.paper, Paper::Sage);
        assert_eq!(
            (s.font, s.pattern, s.deco, s.stamp, s.opacity, s.remind_at, s.remind_fired),
            (NoteFont::Vale, Pattern::Polos, Deco::None, Stamp::None, 100, None, false)
        );
        assert_eq!(serde_json::to_value(&s).unwrap(), phase1);
        // A set field writes its key.
        let s = Sticky { stamp: Stamp::Beres, opacity: 80, ..s };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["stamp"], "Beres");
        assert_eq!(json["opacity"], 80);
    }

    #[test]
    fn the_new_fields_read_leniently() {
        let s = lenient(&json!({
            "font": 7, "pattern": "Spiral", "deco": null, "stamp": ["x"], "opacity": "half",
        }))
        .unwrap();
        assert_eq!(
            (s.font, s.pattern, s.deco, s.stamp, s.opacity),
            (NoteFont::Vale, Pattern::Polos, Deco::None, Stamp::None, 100)
        );
        // Names are trimmed and any case.
        let s = lenient(&json!({"font": " tangan ", "pattern": "GARIS", "deco": "paku", "stamp": "ide"}))
            .unwrap();
        assert_eq!(
            (s.font, s.pattern, s.deco, s.stamp),
            (NoteFont::Tangan, Pattern::Garis, Deco::Paku, Stamp::Ide)
        );
        // Any other opacity snaps to the nearest allowed one.
        for (given, want) in [(100, 100), (95, 100), (90, 100), (89, 80), (70, 80), (69, 60), (50, 60), (49, 40), (0, 40), (-5, 40), (500, 100)] {
            assert_eq!(snap_opacity(given), want, "{given}");
            assert_eq!(lenient(&json!({"opacity": given})).unwrap().opacity, want, "{given}");
        }
        assert_eq!(lenient(&json!({"opacity": 71.4})).unwrap().opacity, 80);
        // A malformed file never panics, whatever the number.
        for given in [i64::MIN, i64::MAX] {
            assert_eq!(snap_opacity(given), if given < 0 { 40 } else { 100 });
        }
        assert_eq!(lenient(&json!({"opacity": i64::MIN})).unwrap().opacity, 40);
        assert_eq!(lenient(&json!({"opacity": -1e300})).unwrap().opacity, 40);
        assert_eq!(lenient(&json!({"opacity": 1e300})).unwrap().opacity, 100);
    }

    #[test]
    fn the_stack_and_tuck_keys_read_and_write_as_documented() {
        let s = lenient(&json!({
            "stack": "stk-n1", "stack_order": 2, "stack_top": true, "tuck": "right",
        }))
        .unwrap();
        assert_eq!(
            (s.stack.as_deref(), s.stack_order, s.stack_top, s.tuck),
            (Some("stk-n1"), 2, true, Some(Edge::Right))
        );
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["stack"], "stk-n1");
        assert_eq!(json["stack_order"], 2);
        assert_eq!(json["stack_top"], true);
        assert_eq!(json["tuck"], "right");
        // Any case and spaces for the edge, a wrong value is none.
        for (given, want) in [("LEFT", Some(Edge::Left)), (" Right ", Some(Edge::Right)), ("top", None), ("", None)] {
            assert_eq!(lenient(&json!({"tuck": given})).unwrap().tuck, want, "{given:?}");
        }
        assert_eq!(lenient(&json!({"tuck": 3})).unwrap().tuck, None);
        assert_eq!(lenient(&json!({"tuck": null})).unwrap().tuck, None);
        // Wrong types are the defaults.
        let s = lenient(&json!({"stack": 5, "stack_order": "first", "stack_top": "yes"})).unwrap();
        assert_eq!((s.stack, s.stack_order, s.stack_top), (None, 0, false));
        let s = lenient(&json!({"stack": "  ", "stack_order": -3})).unwrap();
        assert_eq!((s.stack, s.stack_order), (None, 0));
        assert_eq!(lenient(&json!({"stack_order": 1e30})).unwrap().stack_order, 0, "a float is not a place");
        assert_eq!(lenient(&json!({"stack_order": u64::MAX})).unwrap().stack_order, u32::MAX);
    }

    #[test]
    fn a_phase_1_and_a_run_a_file_load_to_the_defaults_and_save_back_with_their_old_keys() {
        let phase1 = json!({
            "paper": "Sage", "text_size": "L", "pinned": true, "rolled": false,
            "rect": {"x": 8.0, "y": 8.0, "w": 240.0, "h": 208.0},
            "monitor": "", "touched": 5,
        });
        let run_a = json!({
            "paper": "Plum", "text_size": "M", "pinned": false, "rolled": true,
            "rect": {"x": 16.0, "y": 24.0, "w": 300.0, "h": 200.0},
            "monitor": "DISPLAY#X", "touched": 99,
            "remind_at": 1700003600, "remind_fired": true, "font": "Tangan",
            "pattern": "Kotak", "deco": "Paku", "stamp": "Beres", "opacity": 60,
        });
        for file in [phase1, run_a] {
            let s = lenient(&file).unwrap();
            assert_eq!((s.stack.clone(), s.stack_order, s.stack_top, s.tuck), (None, 0, false, None));
            assert_eq!(serde_json::to_value(&s).unwrap(), file, "saved back with exactly its keys");
        }
    }

    #[test]
    fn handwriting_prefers_ink_free_then_segoe_print() {
        assert_eq!(tangan_family(|n| n == "Ink Free" || n == "Segoe Print"), Some("Ink Free"));
        assert_eq!(tangan_family(|n| n == "Segoe Print"), Some("Segoe Print"));
        assert_eq!(tangan_family(|_| false), None, "Vale's own face stands in");
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
            "monitor": 3, "touched": "now", "remind_at": "soon", "remind_fired": true,
        }))
        .unwrap();
        assert_eq!(s.paper, Paper::Krem);
        assert_eq!(s.text_size, TextSize::M);
        assert!(!s.pinned && !s.rolled);
        assert_eq!(s.rect, DipRect::new(0.0, 0.0, MIN_W, MAX_H), "clamped, place 0");
        assert_eq!(s.monitor, "");
        assert_eq!(s.touched, 0);
        assert_eq!(s.remind_at, None, "a wrong type is none");
        assert!(!s.remind_fired, "and a fired flag with no time is false");
        let s = lenient(&json!({"remind_at": 1.5, "remind_fired": "yes"})).unwrap();
        assert_eq!((s.remind_at, s.remind_fired), (None, false));
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
