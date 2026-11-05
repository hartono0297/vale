//! Rich text of a sticky note: bold, italic, underline, strike, a highlight
//! and a text colour, kept as a list of runs next to the plain text.
//!
//! The text itself stays plain (`notes.json` `text`); the format is the
//! sticky part's `styles`, a sorted list of non-overlapping runs. Every index
//! is a UTF-16 unit of the `\n` text (what `checklist` and the layout count).
//! Everything here is pure and tested: the window code only calls it.
//!
//! The highlight and the colour are stored by NAME (`"kuning"`, `"merah"`),
//! never as a hex value, so the palette can change later without touching a
//! saved note.

use std::collections::BTreeSet;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::Value;

use crate::i18n::Key;
use crate::widgets::sticky::INK;

/// A highlighter (stabilo) colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hl {
    Kuning,
    Hijau,
    Pink,
    Biru,
}

impl Hl {
    pub const ALL: [Hl; 4] = [Hl::Kuning, Hl::Hijau, Hl::Pink, Hl::Biru];

    pub fn name(self) -> &'static str {
        match self {
            Hl::Kuning => "kuning",
            Hl::Hijau => "hijau",
            Hl::Pink => "pink",
            Hl::Biru => "biru",
        }
    }

    /// Any case, trimmed; a wrong name is `None`.
    pub fn from_name(name: &str) -> Option<Hl> {
        let n = name.trim().to_lowercase();
        Hl::ALL.into_iter().find(|h| h.name() == n)
    }

    pub fn label_key(self) -> Key {
        match self {
            Hl::Kuning => Key::FmtHlKuning,
            Hl::Hijau => Key::FmtHlHijau,
            Hl::Pink => Key::FmtHlPink,
            Hl::Biru => Key::FmtHlBiru,
        }
    }
}

/// A text colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fg {
    Merah,
    Biru,
    Hijau,
}

impl Fg {
    pub const ALL: [Fg; 3] = [Fg::Merah, Fg::Biru, Fg::Hijau];

    pub fn name(self) -> &'static str {
        match self {
            Fg::Merah => "merah",
            Fg::Biru => "biru",
            Fg::Hijau => "hijau",
        }
    }

    pub fn from_name(name: &str) -> Option<Fg> {
        let n = name.trim().to_lowercase();
        Fg::ALL.into_iter().find(|f| f.name() == n)
    }

    pub fn label_key(self) -> Key {
        match self {
            Fg::Merah => Key::FmtFgMerah,
            Fg::Biru => Key::FmtFgBiru,
            Fg::Hijau => Key::FmtFgHijau,
        }
    }
}

/// The highlight fill. Highlighted text is always `INK` on it.
pub fn hl_rgb(h: Hl) -> u32 {
    match h {
        Hl::Kuning => 0xFFE36E,
        Hl::Hijau => 0xB9E6A5,
        Hl::Pink => 0xF7B8CF,
        Hl::Biru => 0xAED4F2,
    }
}

/// The ink of highlighted text: the dark ink on every paper, Gelap included.
pub const HL_INK: u32 = INK;

/// A text colour on a pastel paper, or (`dark_paper`) on Gelap.
///
/// The pastel values start at the mockup's reds and greens but darker: the
/// mockup's #B3261E and #2E6B2F fail 4.5:1 on Terakota and Plum.
pub fn fg_rgb(f: Fg, dark_paper: bool) -> u32 {
    match (f, dark_paper) {
        (Fg::Merah, false) => 0xA11F18,
        (Fg::Biru, false) => 0x1D4E9E,
        (Fg::Hijau, false) => 0x285E29,
        (Fg::Merah, true) => 0xFF8A80,
        (Fg::Biru, true) => 0x8AB4F8,
        (Fg::Hijau, true) => 0x81C995,
    }
}

/// What can be switched on and off by a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attr {
    Bold,
    Italic,
    Underline,
    Strike,
}

/// The look of one stretch of text. `Default` is plain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub b: bool,
    pub i: bool,
    pub u: bool,
    pub x: bool,
    pub hl: Option<Hl>,
    pub fg: Option<Fg>,
}

impl Style {
    pub fn is_plain(&self) -> bool {
        *self == Style::default()
    }

    pub fn has(&self, a: Attr) -> bool {
        match a {
            Attr::Bold => self.b,
            Attr::Italic => self.i,
            Attr::Underline => self.u,
            Attr::Strike => self.x,
        }
    }

    pub fn with(mut self, a: Attr, on: bool) -> Style {
        match a {
            Attr::Bold => self.b = on,
            Attr::Italic => self.i = on,
            Attr::Underline => self.u = on,
            Attr::Strike => self.x = on,
        }
        self
    }
}

/// Units `s..e` (UTF-16, `\n` text) in `style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Run {
    pub s: usize,
    pub e: usize,
    pub style: Style,
}

impl Run {
    /// A run from a JSON value, field by field: `None` when it is not an
    /// object or `s` / `e` are not unsigned integers; a wrong flag is false
    /// and a wrong `hl` / `fg` name drops that attribute.
    pub fn from_value(v: &Value) -> Option<Run> {
        let map = v.as_object()?;
        let s = usize::try_from(map.get("s")?.as_u64()?).ok()?;
        let e = usize::try_from(map.get("e")?.as_u64()?).ok()?;
        let flag = |k: &str| map.get(k).and_then(Value::as_bool).unwrap_or(false);
        Some(Run {
            s,
            e,
            style: Style {
                b: flag("b"),
                i: flag("i"),
                u: flag("u"),
                x: flag("x"),
                hl: map.get("hl").and_then(Value::as_str).and_then(Hl::from_name),
                fg: map.get("fg").and_then(Value::as_str).and_then(Fg::from_name),
            },
        })
    }
}

impl Serialize for Run {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let st = &self.style;
        let mut map = ser.serialize_map(None)?;
        map.serialize_entry("s", &self.s)?;
        map.serialize_entry("e", &self.e)?;
        for (key, on) in [("b", st.b), ("i", st.i), ("u", st.u), ("x", st.x)] {
            if on {
                map.serialize_entry(key, &true)?;
            }
        }
        if let Some(h) = st.hl {
            map.serialize_entry("hl", h.name())?;
        }
        if let Some(f) = st.fg {
            map.serialize_entry("fg", f.name())?;
        }
        map.end()
    }
}

/// Runs from a JSON value leniently: not an array is empty, a bad element is
/// skipped. The result is not normalised.
pub fn runs_from_value(v: &Value) -> Vec<Run> {
    v.as_array()
        .map(|a| a.iter().filter_map(Run::from_value).collect())
        .unwrap_or_default()
}

// ------------------------------------------------------------------ helpers

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn is_high(u: u16) -> bool {
    (0xD800..0xDC00).contains(&u)
}

fn is_low(u: u16) -> bool {
    (0xDC00..0xE000).contains(&u)
}

/// `i` moved back to before the surrogate pair it falls inside, if it does.
fn snap(u: &[u16], i: usize) -> usize {
    if i > 0 && i < u.len() && is_low(u[i]) && is_high(u[i - 1]) {
        i - 1
    } else {
        i
    }
}

/// Clamps to `n` and snaps; swaps so that `a <= b`.
fn range(u: &[u16], a: usize, b: usize) -> (usize, usize) {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    (snap(u, a.min(u.len())), snap(u, b.min(u.len())))
}

/// Per-unit styles of `runs` over `0..n` (later runs win), coalesced back into
/// sorted, merged runs with no plain run.
fn coalesce(per_unit: &[Style]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (i, st) in per_unit.iter().enumerate() {
        if st.is_plain() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.e == i && last.style == *st => last.e = i + 1,
            _ => out.push(Run {
                s: i,
                e: i + 1,
                style: *st,
            }),
        }
    }
    out
}

/// `runs` clamped to `text`, off surrogate pairs, without empty or plain runs,
/// overlaps resolved (a later run wins), sorted, and touching runs of equal
/// style merged.
pub fn normalise(runs: &[Run], text: &str) -> Vec<Run> {
    normalise_units(runs, &units(text))
}

fn normalise_units(runs: &[Run], u: &[u16]) -> Vec<Run> {
    let clean: Vec<Run> = runs
        .iter()
        .filter_map(|r| {
            let (s, e) = range(u, r.s, r.e);
            (s < e && !r.style.is_plain()).then_some(Run { s, e, style: r.style })
        })
        .collect();
    let (Some(lo), Some(hi)) = (
        clean.iter().map(|r| r.s).min(),
        clean.iter().map(|r| r.e).max(),
    ) else {
        return Vec::new();
    };
    let mut per_unit = vec![Style::default(); hi - lo];
    for r in &clean {
        per_unit[r.s - lo..r.e - lo].fill(r.style);
    }
    let mut out = coalesce(&per_unit);
    for r in &mut out {
        r.s += lo;
        r.e += lo;
    }
    out
}

/// The style of the unit at `idx` in normalised `runs`.
fn unit_style(runs: &[Run], idx: usize) -> Style {
    runs.iter()
        .rev()
        .find(|r| r.s <= idx && idx < r.e)
        .map_or_else(Style::default, |r| r.style)
}

/// The style at caret `i`: the character before it, or the one at it when `i`
/// is 0.
pub fn style_at(runs: &[Run], i: usize) -> Style {
    unit_style(runs, i.saturating_sub(1))
}

/// Whether every unit of `a..b` has `attr`. An empty range has none.
pub fn all_have(runs: &[Run], a: usize, b: usize, attr: Attr) -> bool {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    a < b && (a..b).all(|i| unit_style(runs, i).has(attr))
}

/// Applies `f` to the style of every unit of `a..b`.
fn map_range(runs: &[Run], text: &str, a: usize, b: usize, f: impl Fn(Style) -> Style) -> Vec<Run> {
    let u = units(text);
    let base = normalise_units(runs, &u);
    let (a, b) = range(&u, a, b);
    if a >= b {
        return base;
    }
    let mut cuts: BTreeSet<usize> = BTreeSet::from([a, b]);
    for r in &base {
        for p in [r.s, r.e] {
            if p > a && p < b {
                cuts.insert(p);
            }
        }
    }
    let cuts: Vec<usize> = cuts.into_iter().collect();
    let mut out: Vec<Run> = base
        .iter()
        .flat_map(|r| {
            let left = (r.s < a).then(|| Run { s: r.s, e: r.e.min(a), style: r.style });
            let right = (r.e > b).then(|| Run { s: r.s.max(b), e: r.e, style: r.style });
            left.into_iter().chain(right)
        })
        .collect();
    for w in cuts.windows(2) {
        out.push(Run {
            s: w[0],
            e: w[1],
            style: f(unit_style(&base, w[0])),
        });
    }
    normalise_units(&out, &u)
}

/// A button: when the whole range has `attr` it comes off, else it goes on
/// the whole range.
pub fn toggle(runs: &[Run], text: &str, a: usize, b: usize, attr: Attr) -> Vec<Run> {
    let u = units(text);
    let base = normalise_units(runs, &u);
    let (a, b) = range(&u, a, b);
    let on = !all_have(&base, a, b, attr);
    map_range(&base, text, a, b, |s| s.with(attr, on))
}

/// Sets (or with `None` clears) the highlight of `a..b`.
pub fn set_hl(runs: &[Run], text: &str, a: usize, b: usize, hl: Option<Hl>) -> Vec<Run> {
    map_range(runs, text, a, b, |mut s| {
        s.hl = hl;
        s
    })
}

/// Sets (or with `None` clears) the text colour of `a..b`.
pub fn set_fg(runs: &[Run], text: &str, a: usize, b: usize, fg: Option<Fg>) -> Vec<Run> {
    map_range(runs, text, a, b, |mut s| {
        s.fg = fg;
        s
    })
}

/// Whether every unit of `a..b` has highlight `hl`.
pub fn all_hl(runs: &[Run], a: usize, b: usize, hl: Hl) -> bool {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    a < b && (a..b).all(|i| unit_style(runs, i).hl == Some(hl))
}

// ------------------------------------------------------------------- rebase

/// Where the text changed: `(start, old_end, new_end)` in units, by common
/// prefix and suffix, never splitting a surrogate pair. `None` when equal.
fn diff(old: &[u16], new: &[u16]) -> Option<(usize, usize, usize)> {
    if old == new {
        return None;
    }
    let max = old.len().min(new.len());
    let mut p = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    // A shared high half with a different low half: back to before the pair.
    if p > 0 && is_high(old[p - 1]) {
        p -= 1;
    }
    let mut sf = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(max - p);
    // A shared suffix that begins with a low half belongs to a pair.
    if sf > 0 && is_low(old[old.len() - sf]) {
        sf -= 1;
    }
    Some((p, old.len() - sf, new.len() - sf))
}

/// `diff`, slid left towards `hint` (where the host knows the change starts,
/// at most) as far as it can go and still be a change of the same size. The
/// prefix/suffix diff alone cannot tell which of two equal letters at a style
/// boundary was typed or deleted: in "hel|lo" a typed `l` is the same change
/// as one typed after it ("hell|lo"), and the diff picks the later one. A
/// `hint` past the minimal start, or one the change cannot slide to, changes
/// nothing; one before it only slides within the stretch of equal letters, so
/// a stale hint never makes the change bigger.
fn diff_at(old: &[u16], new: &[u16], hint: usize) -> Option<(usize, usize, usize)> {
    let (p, old_end, new_end) = diff(old, new)?;
    if hint >= p {
        return Some((p, old_end, new_end));
    }
    // The change slides left by `k` while the `k` units before its end in
    // `old` equal the `k` units before its end in `new`; never past 0.
    let slide = old[..old_end]
        .iter()
        .rev()
        .zip(new[..new_end].iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(p);
    let mid_pair = |u: &[u16], i: usize| i > 0 && i < u.len() && is_low(u[i]) && is_high(u[i - 1]);
    // Never inside a surrogate pair (at the start or at either end).
    let mut q = hint.max(p - slide);
    while q < p && {
        let k = p - q;
        mid_pair(old, q) || mid_pair(old, old_end - k) || mid_pair(new, new_end - k)
    } {
        q += 1;
    }
    let k = p - q;
    Some((q, old_end - k, new_end - k))
}

/// `base` (normalised runs of `old`) moved onto the units `nu` across the
/// change `(start, old_end, new_end)`: runs before it stay, runs after it
/// shift, deleted units take their runs with them, and inserted units take
/// `pending` (or the style at the change).
fn move_runs(base: &[Run], nu: &[u16], (p, old_end, new_end): (usize, usize, usize), pending: Option<Style>) -> Vec<Run> {
    let inserted = new_end - p;
    let shift = |i: usize| i + new_end - old_end;
    let mut out: Vec<Run> = Vec::new();
    for r in base {
        if r.s < p {
            out.push(Run { s: r.s, e: r.e.min(p), style: r.style });
        }
        if r.e > old_end {
            out.push(Run {
                s: shift(r.s.max(old_end)),
                e: shift(r.e),
                style: r.style,
            });
        }
    }
    if inserted > 0 {
        let style = pending.unwrap_or_else(|| style_at(base, p));
        out.push(Run { s: p, e: p + inserted, style });
    }
    // The inserted run is last, so it wins over nothing (nothing overlaps it).
    normalise_units(&out, nu)
}

/// `runs` of `old` moved onto `new`; typed text takes the style at the change.
/// Where the change is ambiguous it is taken as late as it can be: for an
/// editor that knows where it happened, `rebase_at` or `rebase_edit`.
pub fn rebase(old: &str, new: &str, runs: &[Run]) -> Vec<Run> {
    rebase_with(old, new, runs, None)
}

/// As `rebase`, but inserted text takes `pending` when it is given (a style
/// chosen with a collapsed caret, before typing).
pub fn rebase_with(old: &str, new: &str, runs: &[Run], pending: Option<Style>) -> Vec<Run> {
    let (ou, nu) = (units(old), units(new));
    let base = normalise_units(runs, &ou);
    match diff(&ou, &nu) {
        Some(change) => move_runs(&base, &nu, change, pending),
        None => normalise_units(&base, &nu),
    }
}

/// As `rebase_with`, for a change known to start at `hint` or after it (the
/// caret or selection start before the change, or the caret after a delete):
/// of the equally small ways to read the change, the one closest to `hint`.
pub fn rebase_at(old: &str, new: &str, runs: &[Run], pending: Option<Style>, hint: usize) -> Vec<Run> {
    let (ou, nu) = (units(old), units(new));
    let base = normalise_units(runs, &ou);
    match diff_at(&ou, &nu, hint) {
        Some(change) => move_runs(&base, &nu, change, pending),
        None => normalise_units(&base, &nu),
    }
}

/// As `rebase_with`, for a known edit: `old[start..end]` replaced by
/// `inserted` units. When `new` is not what that edit makes of `old` (an
/// EDIT that cut the text short, say), as `rebase_at` with `start`.
pub fn rebase_edit(
    old: &str,
    new: &str,
    runs: &[Run],
    pending: Option<Style>,
    (start, end, inserted): (usize, usize, usize),
) -> Vec<Run> {
    let (ou, nu) = (units(old), units(new));
    let base = normalise_units(runs, &ou);
    let exact = start <= end
        && end <= ou.len()
        && start + inserted <= nu.len()
        && nu.len() - (start + inserted) == ou.len() - end
        && ou[..start] == nu[..start]
        && ou[end..] == nu[start + inserted..];
    if exact {
        if start == end && inserted == 0 {
            return normalise_units(&base, &nu);
        }
        return move_runs(&base, &nu, (start, end, start + inserted), pending);
    }
    match diff_at(&ou, &nu, start) {
        Some(change) => move_runs(&base, &nu, change, pending),
        None => normalise_units(&base, &nu),
    }
}

/// `runs` of `old` moved onto `new` when only list markers changed: `spans`
/// are `(line start, old marker length, new marker length)` in `old`, in
/// order (what `checklist::toggle_list_plan` returns). Every unit outside the
/// markers keeps its style and moves by the markers before it, so the
/// formatting of the lines between the first and the last touched line
/// survives (a common-prefix diff would see them as replaced). A dropped
/// marker takes its runs with it; a new one takes the style of the line's
/// first letter (plain on an empty line); a marker that stays is untouched.
pub fn rebase_marks(old: &str, new: &str, runs: &[Run], spans: &[(usize, usize, usize)]) -> Vec<Run> {
    let (ou, nu) = (units(old), units(new));
    let base = normalise_units(runs, &ou);
    let mut per_old = vec![Style::default(); ou.len()];
    for r in &base {
        per_old[r.s..r.e].fill(r.style);
    }
    let mut out: Vec<Style> = Vec::with_capacity(nu.len());
    let mut pos = 0;
    for &(start, old_len, new_len) in spans {
        let start = start.max(pos).min(ou.len());
        out.extend_from_slice(&per_old[pos..start]);
        let content = start + old_len;
        if old_len == new_len {
            // The marker stays as it is.
            let end = content.min(ou.len());
            out.extend_from_slice(&per_old[start..end]);
        } else {
            let st = match ou.get(content) {
                Some(&c) if c != u16::from(b'\n') => per_old[content],
                _ => Style::default(),
            };
            out.extend(std::iter::repeat_n(st, new_len));
        }
        pos = content.min(ou.len());
    }
    out.extend_from_slice(&per_old[pos..]);
    // Whatever the lengths came to, the result is clamped to the new text.
    out.resize(nu.len(), Style::default());
    normalise_units(&coalesce(&out), &nu)
}

// ------------------------------------------------------------- copy, paste

/// The runs of `a..b`, relative to the start of the cut-out.
pub fn slice(runs: &[Run], a: usize, b: usize) -> Vec<Run> {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    runs.iter()
        .filter_map(|r| {
            let (s, e) = (r.s.max(a), r.e.min(b));
            (s < e).then(|| Run { s: s - a, e: e - a, style: r.style })
        })
        .collect()
}

/// `runs` (of the text AFTER a paste) with the pasted stretch `at..at+len`
/// given exactly `pasted`'s styles (relative to the paste, plain where it has
/// no run). The runs outside the stretch are untouched.
pub fn insert_runs(runs: &[Run], at: usize, pasted: &[Run], len: usize) -> Vec<Run> {
    let end = at + len;
    let mut out: Vec<Run> = runs
        .iter()
        .flat_map(|r| {
            let left = (r.s < at).then(|| Run { s: r.s, e: r.e.min(at), style: r.style });
            let right = (r.e > end).then(|| Run { s: r.s.max(end), e: r.e, style: r.style });
            left.into_iter().chain(right)
        })
        .collect();
    out.extend(pasted.iter().filter_map(|r| {
        let e = r.e.min(len);
        (r.s < e).then(|| Run { s: at + r.s, e: at + e, style: r.style })
    }));
    // The inputs are sorted and the stretch is cleared, so nothing overlaps;
    // `normalise` still wants the text, so sort and merge here.
    out.sort_by_key(|r| r.s);
    let mut merged: Vec<Run> = Vec::new();
    for r in out {
        if r.style.is_plain() || r.s >= r.e {
            continue;
        }
        match merged.last_mut() {
            Some(last) if last.e == r.s && last.style == r.style => last.e = r.e,
            _ => merged.push(r),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Accent, Surface, island};
    use crate::widgets::sticky::{Paper, contrast, tones};
    use serde_json::json;

    const BOLD: Style = Style { b: true, i: false, u: false, x: false, hl: None, fg: None };
    const ITAL: Style = Style { b: false, i: true, u: false, x: false, hl: None, fg: None };

    fn run(s: usize, e: usize, style: Style) -> Run {
        Run { s, e, style }
    }

    fn utf16(s: &str) -> usize {
        s.encode_utf16().count()
    }

    #[test]
    fn normalise_clamps_and_drops_empty_and_plain_runs() {
        let runs = [
            run(0, 3, BOLD),
            run(5, 5, BOLD),
            run(2, 2, ITAL),
            run(1, 4, Style::default()),
            run(8, 40, ITAL),
        ];
        assert_eq!(
            normalise(&runs, "0123456789"),
            [run(0, 3, BOLD), run(8, 10, ITAL)]
        );
        assert!(normalise(&[run(30, 40, BOLD)], "abc").is_empty());
        assert!(normalise(&[], "abc").is_empty());
    }

    #[test]
    fn rebase_marks_moves_every_run_by_the_markers_before_it() {
        // "one\ntwo\nthree" with "two" bold; a bullet on every line.
        let old = "one\ntwo\nthree";
        let new = "\u{2022} one\n\u{2022} two\n\u{2022} three";
        let spans = [(0, 0, 2), (4, 0, 2), (8, 0, 2)];
        // The marker takes the style of its line's first letter.
        assert_eq!(
            rebase_marks(old, new, &[run(4, 7, BOLD)], &spans),
            [run(6, 11, BOLD)]
        );
        // A plain line's marker stays plain; a bold first line is not spread.
        assert_eq!(
            rebase_marks(old, new, &[run(0, 3, BOLD)], &spans),
            [run(0, 5, BOLD)]
        );
        // Taking the markers off moves the runs back and drops the markers' own.
        let spans = [(0, 2, 0), (6, 2, 0), (12, 2, 0)];
        assert_eq!(
            rebase_marks(new, old, &[run(6, 11, BOLD), run(0, 1, ITAL)], &spans),
            [run(4, 7, BOLD)]
        );
        // A marker that stays keeps its runs; a run across lines survives whole.
        let spans = [(0, 2, 2), (6, 2, 2)];
        assert_eq!(
            rebase_marks(new, new, &[run(0, 14, BOLD)], &spans),
            [run(0, 14, BOLD)]
        );
        // No runs, no spans.
        assert!(rebase_marks(old, old, &[], &[]).is_empty());
        assert_eq!(rebase_marks(old, old, &[run(1, 2, BOLD)], &[]), [run(1, 2, BOLD)]);
    }

    #[test]
    fn normalise_resolves_overlaps_with_the_later_run_winning() {
        let runs = [run(0, 6, BOLD), run(3, 9, ITAL)];
        assert_eq!(
            normalise(&runs, "0123456789"),
            [run(0, 3, BOLD), run(3, 9, ITAL)]
        );
        // The later run inside an earlier one splits it.
        let runs = [run(0, 9, BOLD), run(3, 5, ITAL)];
        assert_eq!(
            normalise(&runs, "0123456789"),
            [run(0, 3, BOLD), run(3, 5, ITAL), run(5, 9, BOLD)]
        );
        // Unsorted input comes out sorted.
        assert_eq!(
            normalise(&[run(6, 8, ITAL), run(0, 2, BOLD)], "0123456789"),
            [run(0, 2, BOLD), run(6, 8, ITAL)]
        );
    }

    #[test]
    fn normalise_merges_touching_runs_of_equal_style_only() {
        let t = "0123456789";
        assert_eq!(
            normalise(&[run(0, 3, BOLD), run(3, 6, BOLD), run(6, 8, ITAL)], t),
            [run(0, 6, BOLD), run(6, 8, ITAL)]
        );
        // A gap keeps them apart.
        assert_eq!(
            normalise(&[run(0, 3, BOLD), run(4, 6, BOLD)], t),
            [run(0, 3, BOLD), run(4, 6, BOLD)]
        );
        // Overlapping equal runs become one.
        assert_eq!(normalise(&[run(0, 5, BOLD), run(3, 8, BOLD)], t), [run(0, 8, BOLD)]);
    }

    #[test]
    fn normalise_keeps_runs_off_surrogate_pairs() {
        // a, the pair (units 1 and 2), b
        let t = "a\u{1F600}b";
        // Inside the pair at both ends: both go back to before it.
        assert_eq!(normalise(&[run(2, 3, BOLD)], t), [run(1, 3, BOLD)], "the start goes back, the end is whole");
        assert!(normalise(&[run(2, 2, BOLD)], t).is_empty());
        assert_eq!(normalise(&[run(0, 2, BOLD)], t), [run(0, 1, BOLD)]);
        assert_eq!(normalise(&[run(2, 4, BOLD)], t), [run(1, 4, BOLD)]);
        assert_eq!(normalise(&[run(1, 3, BOLD)], t), [run(1, 3, BOLD)]);
    }

    #[test]
    fn style_at_reads_the_character_before_the_caret() {
        let runs = [run(2, 5, BOLD)];
        assert_eq!(style_at(&runs, 2), Style::default(), "before the run starts");
        assert_eq!(style_at(&runs, 3), BOLD);
        assert_eq!(style_at(&runs, 5), BOLD, "at the end of the run");
        assert_eq!(style_at(&runs, 6), Style::default());
        let first = [run(0, 2, ITAL)];
        assert_eq!(style_at(&first, 0), ITAL, "at 0 it is the first character");
        assert_eq!(style_at(&[], 0), Style::default());
    }

    #[test]
    fn all_have_needs_the_whole_range() {
        let runs = [run(2, 6, BOLD)];
        assert!(all_have(&runs, 2, 6, Attr::Bold));
        assert!(all_have(&runs, 3, 5, Attr::Bold));
        assert!(!all_have(&runs, 1, 6, Attr::Bold));
        assert!(!all_have(&runs, 2, 7, Attr::Bold));
        assert!(!all_have(&runs, 2, 6, Attr::Italic));
        assert!(!all_have(&runs, 3, 3, Attr::Bold), "an empty range has nothing");
        assert!(all_have(&runs, 6, 2, Attr::Bold), "reversed ends are fine");
    }

    #[test]
    fn toggle_applies_to_the_whole_range_then_removes() {
        let t = "0123456789";
        let on = toggle(&[], t, 2, 6, Attr::Bold);
        assert_eq!(on, [run(2, 6, BOLD)]);
        let off = toggle(&on, t, 2, 6, Attr::Bold);
        assert!(off.is_empty());
        // Removing from the middle splits the run.
        assert_eq!(
            toggle(&on, t, 3, 5, Attr::Bold),
            [run(2, 3, BOLD), run(5, 6, BOLD)]
        );
    }

    #[test]
    fn toggle_on_a_partial_selection_makes_it_all_bold() {
        let t = "0123456789";
        let runs = [run(2, 4, BOLD), run(6, 8, ITAL)];
        // 2..8 is only partly bold: it becomes all bold, italic kept.
        let out = toggle(&runs, t, 2, 8, Attr::Bold);
        assert!(all_have(&out, 2, 8, Attr::Bold));
        assert!(all_have(&out, 6, 8, Attr::Italic));
        assert!(!all_have(&out, 4, 6, Attr::Italic));
        // Doing it again removes bold everywhere, keeping the italic.
        let back = toggle(&out, t, 2, 8, Attr::Bold);
        assert_eq!(back, [run(6, 8, ITAL)]);
        // An empty selection changes nothing.
        assert_eq!(toggle(&runs, t, 5, 5, Attr::Bold), runs);
    }

    #[test]
    fn other_attributes_are_kept_when_one_toggles() {
        let t = "0123456789";
        let mut s = BOLD;
        s.fg = Some(Fg::Merah);
        let runs = [run(0, 4, s)];
        let out = toggle(&runs, t, 1, 3, Attr::Strike);
        assert_eq!(out[0], run(0, 1, s));
        let mut mid = s;
        mid.x = true;
        assert_eq!(out[1], run(1, 3, mid));
        assert_eq!(out[2], run(3, 4, s));
    }

    #[test]
    fn highlight_and_colour_set_and_clear() {
        let t = "0123456789";
        let hl = set_hl(&[], t, 1, 4, Some(Hl::Pink));
        assert_eq!(hl.len(), 1);
        assert_eq!(hl[0].style.hl, Some(Hl::Pink));
        let both = set_fg(&hl, t, 2, 6, Some(Fg::Biru));
        assert_eq!(style_at(&both, 2).hl, Some(Hl::Pink));
        assert_eq!(style_at(&both, 2).fg, None);
        assert_eq!(style_at(&both, 3).fg, Some(Fg::Biru));
        assert_eq!(style_at(&both, 3).hl, Some(Hl::Pink));
        assert_eq!(style_at(&both, 6).fg, Some(Fg::Biru));
        assert_eq!(style_at(&both, 6).hl, None);
        let cleared = set_hl(&both, t, 0, 10, None);
        assert!(cleared.iter().all(|r| r.style.hl.is_none()));
        let cleared = set_fg(&cleared, t, 0, 10, None);
        assert!(cleared.is_empty());
        assert!(all_hl(&hl, 1, 4, Hl::Pink));
        assert!(!all_hl(&hl, 0, 4, Hl::Pink));
        assert!(!all_hl(&hl, 1, 4, Hl::Kuning));
    }

    #[test]
    fn toggle_never_splits_an_emoji() {
        let t = "a\u{1F600}b";
        // A selection ending between the halves.
        let out = toggle(&[], t, 0, 2, Attr::Bold);
        assert_eq!(out, [run(0, 1, BOLD)], "the end snaps back to before the pair");
        let out = toggle(&[], t, 0, 3, Attr::Bold);
        assert_eq!(out, [run(0, 3, BOLD)]);
    }

    // ----------------------------------------------------------------- rebase

    #[test]
    fn typing_inside_a_run_grows_it() {
        let old = "abcdef";
        let new = "abcXdef";
        let runs = [run(2, 5, BOLD)];
        assert_eq!(rebase(old, new, &runs), [run(2, 6, BOLD)]);
    }

    #[test]
    fn typing_at_a_run_start_stays_outside_and_at_its_end_extends_it() {
        let runs = [run(2, 4, BOLD)];
        // At the start (index 2): the character before is plain.
        assert_eq!(rebase("abcdef", "abXcdef", &runs), [run(3, 5, BOLD)]);
        // At the end (index 4): the character before is bold.
        assert_eq!(rebase("abcdef", "abcdXef", &runs), [run(2, 5, BOLD)]);
    }

    #[test]
    fn typing_at_zero_takes_the_first_characters_style() {
        let runs = [run(0, 3, BOLD)];
        assert_eq!(rebase("abc", "Xabc", &runs), [run(0, 4, BOLD)]);
        assert!(rebase("abc", "Xabc", &[run(1, 3, BOLD)]).iter().all(|r| r.s >= 2));
    }

    #[test]
    fn a_pending_style_overrides_what_typing_inherits() {
        let runs = [run(0, 3, BOLD)];
        // Typing at the end of bold with a plain pending style.
        let out = rebase_with("abc", "abcX", &runs, Some(Style::default()));
        assert_eq!(out, [run(0, 3, BOLD)]);
        // Typing in plain text with pending italic.
        let out = rebase_with("abc", "abcdX", &[], Some(ITAL));
        assert_eq!(out, [run(3, 5, ITAL)]);
        // Pending does nothing for a pure delete.
        let out = rebase_with("abcd", "abc", &[run(0, 4, BOLD)], Some(ITAL));
        assert_eq!(out, [run(0, 3, BOLD)]);
    }

    #[test]
    fn a_letter_typed_before_its_twin_at_a_run_start_stays_plain() {
        // "hello" with "lo" bold; a plain `l` typed at "hel|lo".
        let runs = [run(3, 5, BOLD)];
        assert_eq!(style_at(&runs, 3), Style::default());
        // The blind diff takes it as typed after the bold `l`.
        assert_eq!(rebase("hello", "helllo", &runs), [run(3, 6, BOLD)]);
        // The host knows the caret was at 3.
        assert_eq!(rebase_at("hello", "helllo", &runs, None, 3), [run(4, 6, BOLD)]);
        assert_eq!(rebase_edit("hello", "helllo", &runs, None, (3, 3, 1)), [run(4, 6, BOLD)]);
        // A pending style lands on the typed letter, not one further.
        assert_eq!(
            rebase_at("hello", "helllo", &runs, Some(ITAL), 3),
            [run(3, 4, ITAL), run(4, 6, BOLD)]
        );
    }

    #[test]
    fn deleting_a_letter_before_its_twin_at_a_run_start_keeps_the_twin_bold() {
        // Backspace on the plain `l` at index 2 ("hel|lo", caret 3 -> 2).
        let runs = [run(3, 5, BOLD)];
        assert_eq!(rebase("hello", "helo", &runs), [run(3, 4, BOLD)]);
        assert_eq!(rebase_at("hello", "helo", &runs, None, 2), [run(2, 4, BOLD)]);
        assert_eq!(rebase_edit("hello", "helo", &runs, None, (2, 3, 0)), [run(2, 4, BOLD)]);
    }

    #[test]
    fn a_paste_that_starts_with_the_next_letter_takes_the_style_at_the_caret() {
        // "lx" pasted at "hel|lo" with "lo" bold: the pasted text is plain.
        let runs = [run(3, 5, BOLD)];
        assert_eq!(rebase_at("hello", "hellxlo", &runs, None, 3), [run(5, 7, BOLD)]);
        assert_eq!(rebase_edit("hello", "hellxlo", &runs, None, (3, 3, 2)), [run(5, 7, BOLD)]);
    }

    #[test]
    fn a_hint_never_makes_the_change_bigger() {
        let runs = [run(1, 3, BOLD)];
        // `X` typed at 3 of "abcdef": a hint of 1 (stale) cannot slide it.
        assert_eq!(rebase_at("abcdef", "abcXdef", &runs, None, 1), rebase("abcdef", "abcXdef", &runs));
        // A hint past the change changes nothing either.
        assert_eq!(rebase_at("abcdef", "abcXdef", &runs, None, 6), rebase("abcdef", "abcXdef", &runs));
        // Equal texts.
        assert_eq!(rebase_at("abc", "abc", &runs, None, 0), runs);
        // An edit that does not match the texts falls back to the hint.
        assert_eq!(
            rebase_edit("hello", "helllo", &[run(3, 5, BOLD)], None, (3, 3, 4)),
            [run(4, 6, BOLD)]
        );
    }

    #[test]
    fn a_hint_never_splits_a_surrogate_pair() {
        let old = "a\u{1F600}\u{1F600}b";
        let new = "a\u{1F600}\u{1F600}\u{1F600}b";
        let runs = [run(3, 5, BOLD)];
        for hint in 0..6 {
            let out = rebase_at(old, new, &runs, None, hint);
            let u = units(new);
            assert!(out.iter().all(|r| snap(&u, r.s) == r.s && snap(&u, r.e) == r.e), "{hint}: {out:?}");
        }
        // At the first pair's end (1+2 = 3): the new pair is plain.
        assert_eq!(rebase_at(old, new, &runs, None, 3), [run(5, 7, BOLD)]);
    }

    #[test]
    fn deleting_across_runs_drops_the_deleted_units() {
        let old = "0123456789";
        let runs = [run(1, 4, BOLD), run(6, 9, ITAL)];
        // Delete 3..7.
        let out = rebase(old, "012789", &runs);
        assert_eq!(out, [run(1, 3, BOLD), run(3, 5, ITAL)]);
        // Delete everything.
        assert!(rebase(old, "", &runs).is_empty());
        // Delete a whole run.
        assert!(rebase(old, "0", &runs).is_empty());
    }

    #[test]
    fn a_replace_takes_the_style_before_the_change() {
        let old = "abcdef";
        let runs = [run(0, 3, BOLD)];
        // "cd" -> "XYZ": the char before the change is bold.
        let out = rebase(old, "abXYZef", &runs);
        assert_eq!(out, [run(0, 5, BOLD)]);
    }

    #[test]
    fn runs_after_a_change_shift() {
        let runs = [run(5, 8, ITAL)];
        assert_eq!(rebase("0123456789", "01XX23456789", &runs), [run(7, 10, ITAL)]);
        assert_eq!(rebase("0123456789", "03456789", &runs), [run(3, 6, ITAL)]);
        assert_eq!(rebase("same", "same", &[run(0, 2, BOLD)]), [run(0, 2, BOLD)]);
    }

    #[test]
    fn rebase_over_emoji_never_splits_a_pair() {
        let old = "a\u{1F600}b";
        let runs = [run(0, 1, BOLD), run(3, 4, ITAL)];
        // Replace the emoji by another one that shares the high half.
        let new = "a\u{1F601}b";
        assert_eq!(utf16(new), 4);
        let out = rebase(old, new, &runs);
        assert_eq!(out, [run(0, 3, BOLD), run(3, 4, ITAL)], "the new emoji takes the style before it");
        // Insert an emoji before "b".
        let out = rebase("ab", "a\u{1F600}b", &[run(0, 1, BOLD)]);
        assert_eq!(out, [run(0, 3, BOLD)], "inherits the bold before it");
        // Delete the emoji.
        let out = rebase(old, "ab", &runs);
        assert_eq!(out, [run(0, 1, BOLD), run(1, 2, ITAL)]);
        // Every end of the result is on a boundary.
        let nu: Vec<u16> = new.encode_utf16().collect();
        for r in rebase(old, new, &runs) {
            assert_eq!(snap(&nu, r.s), r.s);
            assert_eq!(snap(&nu, r.e), r.e);
        }
    }

    #[test]
    fn crlf_text_is_mapped_through_normalize_first() {
        use crate::widgets::checklist::normalize;
        let old = normalize("ab\r\ncd");
        let new = normalize("ab\r\nXcd");
        assert_eq!(old, "ab\ncd");
        let out = rebase(&old, &new, &[run(3, 5, BOLD)]);
        assert_eq!(out, [run(4, 6, BOLD)]);
    }

    // ------------------------------------------------------------ copy, paste

    #[test]
    fn slice_is_relative_to_the_cut() {
        let runs = [run(1, 4, BOLD), run(6, 9, ITAL)];
        assert_eq!(slice(&runs, 2, 7), [run(0, 2, BOLD), run(4, 5, ITAL)]);
        assert!(slice(&runs, 4, 6).is_empty());
        assert_eq!(slice(&runs, 7, 2), slice(&runs, 2, 7));
    }

    #[test]
    fn slice_then_insert_round_trips() {
        let src = "aa bold bb";
        let runs = [run(3, 7, BOLD)];
        let cut = slice(&runs, 3, 7);
        assert_eq!(cut, [run(0, 4, BOLD)]);
        // Paste "bold" into "xyz" after the x: the rebase makes the text plain
        // there, and the overlay gives it the copied style.
        let after = "xboldyz";
        let rebased = rebase("xyz", after, &[]);
        let out = insert_runs(&rebased, 1, &cut, 4);
        assert_eq!(out, [run(1, 5, BOLD)]);
        // The source text is untouched by the slice.
        assert_eq!(runs, [run(3, 7, BOLD)]);
        let _ = src;
    }

    #[test]
    fn insert_runs_replaces_the_inherited_style_of_the_pasted_stretch() {
        // Pasting into bold text: the rebase made the pasted stretch bold, the
        // overlay (plain text copied) takes that away.
        let after = "abPASTEcd";
        let rebased = rebase("abcd", after, &[run(0, 4, BOLD)]);
        assert_eq!(rebased, [run(0, 9, BOLD)]);
        let out = insert_runs(&rebased, 2, &[], 5);
        assert_eq!(out, [run(0, 2, BOLD), run(7, 9, BOLD)]);
        // With a copied italic in the middle.
        let out = insert_runs(&rebased, 2, &[run(1, 3, ITAL)], 5);
        assert_eq!(out, [run(0, 2, BOLD), run(3, 5, ITAL), run(7, 9, BOLD)]);
    }

    // ------------------------------------------------------------------ serde

    #[test]
    fn a_run_is_written_with_names_and_only_the_set_flags() {
        let r = run(
            3,
            8,
            Style { b: true, i: false, u: false, x: true, hl: Some(Hl::Kuning), fg: Some(Fg::Merah) },
        );
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"s":3,"e":8,"b":true,"x":true,"hl":"kuning","fg":"merah"}"#
        );
        assert_eq!(
            serde_json::to_string(&run(0, 1, ITAL)).unwrap(),
            r#"{"s":0,"e":1,"i":true}"#
        );
    }

    #[test]
    fn a_run_reads_leniently() {
        let v = json!({"s": 1, "e": 4, "b": true, "hl": "PINK", "fg": "ungu", "u": "yes"});
        let r = Run::from_value(&v).unwrap();
        assert_eq!((r.s, r.e), (1, 4));
        assert!(r.style.b && !r.style.u, "a wrong flag is false");
        assert_eq!(r.style.hl, Some(Hl::Pink));
        assert_eq!(r.style.fg, None, "a bad name drops that attribute");
        assert!(Run::from_value(&json!({"s": -1, "e": 4})).is_none());
        assert!(Run::from_value(&json!({"s": 1.5, "e": 4})).is_none());
        assert!(Run::from_value(&json!({"s": 1})).is_none());
        assert!(Run::from_value(&json!("x")).is_none());
        let list = runs_from_value(&json!([{"s":0,"e":2,"b":true}, 5, {"s":"a","e":2}, {"s":3,"e":4,"i":true}]));
        assert_eq!(list.len(), 2);
        assert!(runs_from_value(&json!({"s":0})).is_empty());
    }

    // ---------------------------------------------------------------- colours

    #[test]
    fn names_round_trip() {
        for h in Hl::ALL {
            assert_eq!(Hl::from_name(h.name()), Some(h));
        }
        for f in Fg::ALL {
            assert_eq!(Fg::from_name(f.name()), Some(f));
        }
        assert_eq!(Hl::from_name(" Kuning "), Some(Hl::Kuning));
        assert_eq!(Hl::from_name("#FFE36E"), None);
    }

    #[test]
    fn highlight_colours_are_the_specified_ones() {
        assert_eq!(hl_rgb(Hl::Kuning), 0xFFE36E);
        assert_eq!(hl_rgb(Hl::Hijau), 0xB9E6A5);
        assert_eq!(hl_rgb(Hl::Pink), 0xF7B8CF);
        assert_eq!(hl_rgb(Hl::Biru), 0xAED4F2);
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
    fn every_text_colour_reads_on_every_paper_and_its_ruling() {
        for surface in SURFACES {
            for accent in Accent::ALL {
                let isl = island(*accent, surface);
                for paper in Paper::ALL {
                    let t = tones(paper, &isl, *accent);
                    let dark = paper == Paper::Gelap;
                    for f in Fg::ALL {
                        let rgb = fg_rgb(f, dark);
                        for (what, bg) in [("fill", t.fill), ("ruling", t.line)] {
                            let c = contrast(rgb, bg);
                            assert!(
                                c >= 4.5,
                                "{f:?} on {paper:?} {what} ({surface:?}/{accent:?}): {c:.2}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn highlighted_text_reads_on_every_highlight() {
        for h in Hl::ALL {
            let c = contrast(HL_INK, hl_rgb(h));
            assert!(c >= 4.5, "{h:?}: {c:.2}");
        }
        assert_eq!(HL_INK, INK, "the dark ink, even on Gelap");
    }
}
