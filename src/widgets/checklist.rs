//! Checklists inside a sticky note's text. A note is plain text; a line that
//! starts with `- [ ] ` (open) or `- [x] ` (done, `- [X] ` is read too) is a
//! checklist item, and the first line of a note is its title.
//!
//! All indices here are UTF-16 code units of the text with `\n` line breaks
//! (what the Windows EDIT counts, apart from its `\r\n`: `edit_index` and
//! `model_index` convert). Every function is pure: an edit comes back as an
//! `Edit` (a range to replace, the new text, where the caret goes) for the
//! keyboard code to apply to the EDIT, or for `apply` on a plain string.

/// The marker of an open and of a done item. Both are six UTF-16 units.
pub const OPEN: &str = "- [ ] ";
pub const DONE: &str = "- [x] ";
/// Length of either marker.
pub const MARKER_LEN: usize = 6;
/// A bullet line starts with this: a bullet and a space, two UTF-16 units.
pub const BULLET: &str = "\u{2022} ";
/// Length of the bullet marker.
pub const BULLET_LEN: usize = 2;

/// What a line starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    None,
    /// A checklist item (`- [ ] ` or `- [x] `).
    Item { checked: bool },
    /// A bullet (`\u{2022} `).
    Bullet,
}

/// What the list buttons toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Check,
}

/// One line of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    /// Index of its first unit.
    pub start: usize,
    /// Index just past its last unit (before the `\n`).
    pub end: usize,
    /// 6 for a checklist item, 2 for a bullet, else 0.
    pub marker_len: usize,
    pub checked: bool,
    pub marker: Marker,
}

impl Line {
    /// Where the text after the marker starts.
    pub fn content_start(&self) -> usize {
        self.start + self.marker_len
    }

    /// A checklist item: the only line that has a box and counts.
    pub fn is_item(&self) -> bool {
        matches!(self.marker, Marker::Item { .. })
    }

    pub fn is_bullet(&self) -> bool {
        self.marker == Marker::Bullet
    }

    /// Any marker (a box or a bullet): the text is indented and the marker is
    /// never drawn as text.
    pub fn has_marker(&self) -> bool {
        self.marker_len > 0
    }

    /// A marker with nothing after it.
    pub fn is_empty_marker(&self) -> bool {
        self.has_marker() && self.end <= self.content_start()
    }
}

/// A replacement: units `start..end` of the text become `insert`, and the
/// caret goes to `caret` (an index in the new text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub insert: String,
    pub caret: usize,
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn from_units(u: &[u16]) -> String {
    String::from_utf16_lossy(u)
}

/// `text` with `\r\n` and lone `\r` turned into `\n`.
pub fn normalize(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_owned();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The checklist marker at the start of `line` (`Some(checked)`).
fn item_at(u: &[u16]) -> Option<bool> {
    let at = |i: usize| u.get(i).copied();
    let open = at(0) == Some(u16::from(b'-'))
        && at(1) == Some(u16::from(b' '))
        && at(2) == Some(u16::from(b'['))
        && at(4) == Some(u16::from(b']'))
        && at(5) == Some(u16::from(b' '));
    if !open {
        return None;
    }
    match at(3) {
        Some(c) if c == u16::from(b' ') => Some(false),
        Some(c) if c == u16::from(b'x') || c == u16::from(b'X') => Some(true),
        _ => None,
    }
}

fn lines_of(u: &[u16]) -> Vec<Line> {
    let mut out = Vec::new();
    let mut start = 0;
    loop {
        let end = u[start..]
            .iter()
            .position(|c| *c == u16::from(b'\n'))
            .map_or(u.len(), |p| start + p);
        let row = &u[start..end];
        let (marker, marker_len) = match item_at(row) {
            Some(checked) => (Marker::Item { checked }, MARKER_LEN),
            None if row.first() == Some(&0x2022) && row.get(1) == Some(&u16::from(b' ')) => {
                (Marker::Bullet, BULLET_LEN)
            }
            None => (Marker::None, 0),
        };
        out.push(Line {
            start,
            end,
            marker_len,
            checked: marker == (Marker::Item { checked: true }),
            marker,
        });
        if end >= u.len() {
            break;
        }
        start = end + 1;
    }
    out
}

/// Every line of `text`, in order. Text ending in a break has a last, empty
/// line; the empty text has one empty line.
pub fn lines(text: &str) -> Vec<Line> {
    lines_of(&units(text))
}

/// The line holding `caret`.
pub fn line_at(lines: &[Line], caret: usize) -> usize {
    lines
        .iter()
        .rposition(|l| l.start <= caret)
        .unwrap_or_default()
}

/// How many items are done and how many there are.
pub fn counts(text: &str) -> (usize, usize) {
    let all = lines(text);
    let total = all.iter().filter(|l| l.is_item()).count();
    let done = all.iter().filter(|l| l.checked).count();
    (done, total)
}

/// At least one item, and every one done.
pub fn all_done(text: &str) -> bool {
    let (done, total) = counts(text);
    total > 0 && done == total
}

/// Applies `edit` to `text`.
pub fn apply(text: &str, edit: &Edit) -> String {
    let u = units(text);
    let start = edit.start.min(u.len());
    let end = edit.end.clamp(start, u.len());
    let mut out = u[..start].to_vec();
    out.extend(edit.insert.encode_utf16());
    out.extend_from_slice(&u[end..]);
    from_units(&out)
}

/// Flips the item on line `index` (an open one is done and the other way).
/// `None` for a line that is not an item.
pub fn toggle(text: &str, index: usize) -> Option<Edit> {
    let all = lines(text);
    let line = all.get(index)?;
    if !line.is_item() {
        return None;
    }
    let marker = if line.checked { OPEN } else { DONE };
    // The marker keeps its length, so a caret anywhere stays valid; the edit
    // reports the item's text start for callers that have no caret of their own.
    Some(Edit {
        start: line.start,
        end: line.content_start(),
        insert: marker.to_owned(),
        caret: line.content_start(),
    })
}

/// The lines of `text` a selection `a..b` touches, as indices. A selection
/// that ends exactly at the start of a later line does not touch that line.
fn touched(all: &[Line], a: usize, b: usize) -> (usize, usize) {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    let first = line_at(all, a);
    let mut last = line_at(all, b);
    if b > a && last > first && all[last].start == b {
        last -= 1;
    }
    (first, last)
}

/// A list button (the bar's bullet and checklist buttons, Ctrl+Shift+8 and
/// 9, the header's checklist button) on every line the selection `sel_a..sel_b`
/// touches (a collapsed caret is its line): when all of them already have
/// `kind` it comes off, otherwise `kind` is put on all of them, replacing the
/// other kind. The caret of the `Edit` is where `sel_b` goes; use
/// `toggle_list_sel` for both ends.
pub fn toggle_list(text: &str, sel_a: usize, sel_b: usize, kind: ListKind) -> Edit {
    toggle_list_sel(text, sel_a, sel_b, kind).0
}

/// `toggle_list` and the selection (both ends) in the new text.
pub fn toggle_list_sel(
    text: &str,
    sel_a: usize,
    sel_b: usize,
    kind: ListKind,
) -> (Edit, (usize, usize)) {
    let u = units(text);
    let all = lines_of(&u);
    let (first, last) = touched(&all, sel_a, sel_b);
    let has = |l: &Line| match kind {
        ListKind::Bullet => l.is_bullet(),
        ListKind::Check => l.is_item(),
    };
    let remove = all[first..=last].iter().all(has);
    let new_marker = match kind {
        ListKind::Bullet => BULLET,
        ListKind::Check => OPEN,
    };
    let mut insert: Vec<u16> = Vec::new();
    // Per touched line: where it starts, its old marker's length and the new
    // marker's length.
    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    for (n, l) in all[first..=last].iter().enumerate() {
        if n > 0 {
            insert.push(u16::from(b'\n'));
        }
        let marker: Vec<u16> = if remove {
            Vec::new()
        } else if has(l) {
            u[l.start..l.content_start()].to_vec()
        } else {
            new_marker.encode_utf16().collect()
        };
        spans.push((l.start, l.marker_len, marker.len()));
        insert.extend_from_slice(&marker);
        insert.extend_from_slice(&u[l.content_start()..l.end]);
    }
    let region_start = all[first].start;
    // Where an old index lands: inside a marker it is the text's start, after
    // it the shift of every marker so far.
    let map = |p: usize| -> usize {
        let mut shift: isize = 0;
        for &(start, old_len, new_len) in &spans {
            if p < start {
                break;
            }
            if p < start + old_len {
                return (start as isize + shift) as usize + new_len;
            }
            shift += new_len as isize - old_len as isize;
        }
        (p as isize + shift) as usize
    };
    let caret = map(sel_b.max(region_start));
    let edit = Edit {
        start: region_start,
        end: all[last].end,
        insert: from_units(&insert),
        caret,
    };
    (edit, (map(sel_a.max(region_start)), caret))
}

/// Typing a space at `caret` (an empty selection): right after a `-` that
/// starts a plain line, the `-` becomes a full open marker and the space is
/// swallowed. Call it before the space is inserted.
pub fn autobox(text: &str, caret: usize) -> Option<Edit> {
    let u = units(text);
    let all = lines_of(&u);
    let line = all[line_at(&all, caret)];
    let dash = u.get(line.start).copied() == Some(u16::from(b'-'));
    if line.is_item() || !dash || caret != line.start + 1 {
        return None;
    }
    Some(Edit {
        start: line.start,
        end: caret,
        insert: OPEN.to_owned(),
        caret: line.start + MARKER_LEN,
    })
}

/// Enter at `caret` on an item or a bullet: a non-empty one continues the list
/// (a new open item or bullet starts at the caret), an empty one ends it (its
/// marker goes). `None` on a plain line: the EDIT inserts the break itself.
pub fn enter(text: &str, caret: usize) -> Option<Edit> {
    let all = lines(text);
    let line = all[line_at(&all, caret)];
    if !line.has_marker() {
        return None;
    }
    if line.is_empty_marker() {
        return Some(Edit {
            start: line.start,
            end: line.end,
            insert: String::new(),
            caret: line.start,
        });
    }
    let at = caret.clamp(line.content_start(), line.end);
    let marker = if line.is_bullet() { BULLET } else { OPEN };
    Some(Edit {
        start: at,
        end: at,
        insert: format!("\n{marker}"),
        caret: at + 1 + line.marker_len,
    })
}

/// Backspace with the caret at the start of an item's text: the whole marker
/// goes instead of one unit of it.
pub fn backspace(text: &str, caret: usize) -> Option<Edit> {
    let all = lines(text);
    let line = all[line_at(&all, caret)];
    (line.has_marker() && caret == line.content_start()).then(|| Edit {
        start: line.start,
        end: line.content_start(),
        insert: String::new(),
        caret: line.start,
    })
}

/// A caret the user moved from `prev` to `caret` never rests inside a marker:
/// Home (the line's start) and a move to the right land on the item's text,
/// a step left from the text's start goes to the end of the line before.
pub fn snap_caret(text: &str, caret: usize, prev: usize) -> usize {
    let all = lines(text);
    let line = all[line_at(&all, caret)];
    if !line.has_marker() || caret >= line.content_start() {
        return caret;
    }
    let stepping_left = prev >= line.content_start() && caret != line.start && line.start > 0;
    if stepping_left {
        line.start - 1
    } else {
        line.content_start()
    }
}

/// The text as it is shown: the markers cut out of the lines. Line breaks stay.
/// (The note draws each line on its own, from `lines`; this and the two index
/// maps below are the same rule for a caller that lays out the whole text.)
#[cfg_attr(not(test), allow(dead_code))]
pub fn display_text(text: &str) -> String {
    let u = units(text);
    let mut out: Vec<u16> = Vec::with_capacity(u.len());
    for (i, line) in lines_of(&u).iter().enumerate() {
        if i > 0 {
            out.push(u16::from(b'\n'));
        }
        out.extend_from_slice(&u[line.content_start()..line.end]);
    }
    from_units(&out)
}

/// Where raw index `raw` falls in `display_text`; inside a marker it is the
/// start of the item's text.
#[cfg_attr(not(test), allow(dead_code))]
pub fn raw_to_display(text: &str, raw: usize) -> usize {
    let all = lines(text);
    let removed: usize = all
        .iter()
        .take_while(|l| l.start <= raw)
        .map(|l| l.marker_len.min(raw - l.start))
        .sum();
    raw - removed
}

/// Where display index `disp` falls in the raw text.
#[cfg_attr(not(test), allow(dead_code))]
pub fn display_to_raw(text: &str, disp: usize) -> usize {
    let all = lines(text);
    let mut shown = 0;
    for l in &all {
        let len = l.end - l.content_start();
        if disp <= shown + len {
            return l.content_start() + (disp - shown);
        }
        shown += len + 1;
    }
    all.last().map_or(0, |l| l.end)
}

/// An index of the `\n` text as the EDIT counts it (every break is two units).
pub fn edit_index(text: &str, model: usize) -> usize {
    let u = units(text);
    let breaks = u[..model.min(u.len())]
        .iter()
        .filter(|c| **c == u16::from(b'\n'))
        .count();
    model + breaks
}

/// An index of the EDIT's `\r\n` text as the `\n` text counts it.
pub fn model_index(text: &str, edit: usize) -> usize {
    let mut edit_pos = 0;
    let mut model = 0;
    for c in text.encode_utf16() {
        if edit_pos >= edit {
            break;
        }
        edit_pos += if c == u16::from(b'\n') { 2 } else { 1 };
        model += 1;
    }
    model
}

/// `text` with every break as `\r\n`, for the EDIT.
pub fn to_edit_text(text: &str) -> String {
    normalize(text).replace('\n', "\r\n")
}

/// Whether the EDIT's raw text holds a break that is not a `\r\n` pair. A
/// plain multi-line EDIT keeps a pasted lone `\n` (an LF-only source) or `\r`
/// as one character, and then `edit_index` / `model_index` (every break is
/// two units) are off by one for each of them.
pub fn has_lone_break(raw: &str) -> bool {
    let u = units(raw);
    let (cr, lf) = (u16::from(b'\r'), u16::from(b'\n'));
    u.iter().enumerate().any(|(i, &c)| {
        (c == cr && u.get(i + 1) != Some(&lf)) || (c == lf && (i == 0 || u[i - 1] != cr))
    })
}

/// An index of the EDIT's raw text, whatever its breaks (`\r\n`, a lone `\r`
/// or `\n`), as the model (`normalize`d) text counts it. Inside a `\r\n` pair
/// it is after the break.
pub fn raw_model_index(raw: &str, edit: usize) -> usize {
    let u = units(raw);
    let (cr, lf) = (u16::from(b'\r'), u16::from(b'\n'));
    let (mut i, mut model) = (0, 0);
    while i < u.len() && i < edit {
        let pair = u[i] == cr && u.get(i + 1) == Some(&lf);
        i += if pair { 2 } else { 1 };
        model += 1;
    }
    model
}

/// The words under a note's rolled-up title: the first line without its
/// marker, trimmed.
pub fn title_of(text: &str) -> String {
    let first = text.split('\n').next().unwrap_or_default();
    let first = first
        .strip_prefix(OPEN)
        .or_else(|| first.strip_prefix(DONE))
        .or_else(|| first.strip_prefix("- [X] "))
        .or_else(|| first.strip_prefix(BULLET))
        .unwrap_or(first);
    first.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "Belanja\n- [ ] bata\n- [x] kaca\n- [X] baja\nbiasa";

    #[test]
    fn lines_are_parsed_with_their_markers() {
        let l = lines(LIST);
        assert_eq!(l.len(), 5);
        assert!(!l[0].is_item());
        assert!(l[1].is_item() && !l[1].checked);
        assert!(l[2].checked && l[3].checked, "x and X are both done");
        assert!(!l[4].is_item());
        assert_eq!(l[1].start, 8);
        assert_eq!(l[1].content_start(), 14);
        assert_eq!(l[1].end, 18);
        assert_eq!(lines("").len(), 1);
        assert_eq!(lines("a\n").len(), 2, "a break ends with an empty line");
    }

    #[test]
    fn only_the_exact_marker_counts() {
        for plain in ["- [] a", "-[ ] a", "- [y] a", "- [ ]a", "* [ ] a", " - [ ] a", "- [ ]"] {
            assert!(!lines(plain)[0].is_item(), "{plain:?}");
        }
    }

    #[test]
    fn counts_and_all_done() {
        assert_eq!(counts(LIST), (2, 3));
        assert!(!all_done(LIST));
        assert!(all_done("t\n- [x] a\n- [X] b"));
        assert!(!all_done("no items"), "no items is never all done");
        assert!(!all_done(""));
        assert_eq!(counts("- [ ] a\n- [ ] b"), (0, 2), "the title line may be an item");
    }

    #[test]
    fn toggle_flips_one_item_only() {
        let e = toggle(LIST, 1).unwrap();
        assert_eq!(apply(LIST, &e), "Belanja\n- [x] bata\n- [x] kaca\n- [X] baja\nbiasa");
        let e = toggle(LIST, 3).unwrap();
        assert_eq!(apply(LIST, &e), "Belanja\n- [ ] bata\n- [x] kaca\n- [ ] baja\nbiasa");
        assert_eq!(toggle(LIST, 0), None);
        assert_eq!(toggle(LIST, 4), None);
        assert_eq!(toggle(LIST, 9), None);
    }

    fn toggle_current_line(text: &str, caret: usize) -> Edit {
        toggle_list(text, caret, caret, ListKind::Check)
    }

    #[test]
    fn the_header_button_adds_and_removes_the_marker() {
        let e = toggle_current_line("abc\ndef", 5);
        assert_eq!(apply("abc\ndef", &e), "abc\n- [ ] def");
        assert_eq!(e.caret, 5 + MARKER_LEN);
        let text = "abc\n- [x] def";
        let e = toggle_current_line(text, 12);
        assert_eq!(apply(text, &e), "abc\ndef");
        assert_eq!(e.caret, 6);
        // A caret inside the marker stays on the line.
        let e = toggle_current_line(text, 5);
        assert_eq!(apply(text, &e), "abc\ndef");
        assert_eq!(e.caret, 4);
    }

    #[test]
    fn a_space_after_a_line_start_dash_makes_a_box() {
        let e = autobox("-", 1).unwrap();
        assert_eq!(apply("-", &e), "- [ ] ");
        assert_eq!(e.caret, 6);
        let e = autobox("abc\n-", 5).unwrap();
        assert_eq!(apply("abc\n-", &e), "abc\n- [ ] ");
        assert_eq!(e.caret, 10);
        // A dash with text after it still turns into a box.
        let e = autobox("-milk", 1).unwrap();
        assert_eq!(apply("-milk", &e), "- [ ] milk");
        assert_eq!(autobox("a-", 2), None, "not at the line start");
        assert_eq!(autobox("--", 2), None);
        assert_eq!(autobox("- [ ] x", 1), None, "already an item");
        assert_eq!(autobox("abc", 0), None);
    }

    #[test]
    fn enter_continues_a_list_and_ends_it_on_an_empty_item() {
        let text = "t\n- [ ] milk";
        let e = enter(text, text.len()).unwrap();
        assert_eq!(apply(text, &e), "t\n- [ ] milk\n- [ ] ");
        assert_eq!(e.caret, text.len() + 7);
        // In the middle: the rest moves to the new item.
        let e = enter(text, 11).unwrap();
        assert_eq!(apply(text, &e), "t\n- [ ] mil\n- [ ] k");
        assert_eq!(e.caret, 11 + 7);
        // A done item continues with an open one.
        let done = "- [x] a";
        let e = enter(done, 7).unwrap();
        assert_eq!(apply(done, &e), "- [x] a\n- [ ] ");
        // An empty item ends the list.
        let empty = "t\n- [ ] milk\n- [ ] ";
        let e = enter(empty, empty.len()).unwrap();
        assert_eq!(apply(empty, &e), "t\n- [ ] milk\n");
        assert_eq!(e.caret, 13);
        // Plain lines are the EDIT's.
        assert_eq!(enter("abc", 3), None);
        // A caret inside the marker acts as at the item's text.
        let e = enter("- [ ] a", 2).unwrap();
        assert_eq!(apply("- [ ] a", &e), "- [ ] \n- [ ] a");
    }

    #[test]
    fn backspace_removes_the_whole_marker() {
        let text = "t\n- [ ] milk";
        let e = backspace(text, 8).unwrap();
        assert_eq!(apply(text, &e), "t\nmilk");
        assert_eq!(e.caret, 2);
        assert_eq!(backspace(text, 9), None);
        assert_eq!(backspace("abc", 0), None);
    }

    #[test]
    fn the_caret_never_rests_inside_a_marker() {
        let text = "a\n- [ ] milk";
        // Home: the line start goes to the text.
        assert_eq!(snap_caret(text, 2, 6), 8);
        // A step left from the text jumps to the line before.
        assert_eq!(snap_caret(text, 7, 8), 1);
        // A click or a step right lands on the text.
        assert_eq!(snap_caret(text, 5, 0), 8);
        // Outside markers nothing changes.
        assert_eq!(snap_caret(text, 10, 11), 10);
        assert_eq!(snap_caret(text, 1, 2), 1);
        // On the first line there is no line before.
        assert_eq!(snap_caret("- [ ] a", 3, 6), 6);
    }

    #[test]
    fn display_hides_the_markers_and_maps_both_ways() {
        let text = "Belanja\n- [ ] bata\nbiasa\n- [x] kaca";
        assert_eq!(display_text(text), "Belanja\nbata\nbiasa\nkaca");
        // raw -> display
        assert_eq!(raw_to_display(text, 0), 0);
        assert_eq!(raw_to_display(text, 8), 8, "the marker start is the text start");
        assert_eq!(raw_to_display(text, 10), 8, "inside a marker");
        assert_eq!(raw_to_display(text, 14), 8);
        assert_eq!(raw_to_display(text, 15), 9);
        assert_eq!(raw_to_display(text, text.len()), 23);
        // display -> raw
        assert_eq!(display_to_raw(text, 0), 0);
        assert_eq!(display_to_raw(text, 8), 14);
        assert_eq!(display_to_raw(text, 12), 18);
        assert_eq!(display_to_raw(text, 13), 19);
        assert_eq!(display_to_raw(text, 99), text.len());
        // Round trip over every display index.
        for d in 0..=23 {
            assert_eq!(raw_to_display(text, display_to_raw(text, d)), d, "{d}");
        }
    }

    #[test]
    fn edit_and_model_indices_convert() {
        let text = "ab\ncd\nef";
        assert_eq!(edit_index(text, 0), 0);
        assert_eq!(edit_index(text, 3), 4);
        assert_eq!(edit_index(text, 6), 8);
        assert_eq!(edit_index(text, 8), 10);
        for m in 0..=8 {
            assert_eq!(model_index(text, edit_index(text, m)), m, "{m}");
        }
        // Inside a \r\n pair counts as before the break.
        assert_eq!(model_index(text, 3), 3);
        assert_eq!(to_edit_text("a\nb"), "a\r\nb");
        assert_eq!(to_edit_text("a\r\nb"), "a\r\nb");
        assert_eq!(normalize("a\r\nb\rc"), "a\nb\nc");
    }

    #[test]
    fn a_lone_break_in_the_edit_is_found_and_counted_as_one() {
        // "a\nb" pasted from an LF-only source, the caret after the "b".
        let raw = "a\nb";
        let model = normalize(raw);
        assert!(has_lone_break(raw));
        // The pair-only map is one short: the bug the rewrite prevents.
        assert_eq!(model_index(&model, 3), 2);
        assert_eq!(raw_model_index(raw, 3), 3);
        // After the rewrite the EDIT holds pairs and the maps agree again.
        let fixed = to_edit_text(&model);
        assert!(!has_lone_break(&fixed));
        assert_eq!(edit_index(&model, raw_model_index(raw, 3)), 4);
        assert_eq!(model_index(&model, 4), 3);

        // Mixed breaks: a pair, a lone CR, a lone LF.
        let raw = "x\r\ny\rz\nw";
        assert!(has_lone_break(raw));
        assert_eq!(normalize(raw), "x\ny\nz\nw");
        let expect = [0, 1, 2, 2, 3, 4, 5, 6, 7];
        for (e, m) in expect.iter().enumerate() {
            assert_eq!(raw_model_index(raw, e), *m, "{e}");
        }
        assert_eq!(raw_model_index(raw, 99), 7);
        assert!(has_lone_break("\n"));
        assert!(has_lone_break("a\r"));
        assert!(has_lone_break("\r\r\n"));
        assert!(!has_lone_break("a\r\nb\r\n"));
        assert!(!has_lone_break(""));
    }

    #[test]
    fn indices_count_utf16_units() {
        // A cat emoji is two units; the item's text starts after the marker.
        let text = "\u{1F431}\n- [ ] \u{1F431}x";
        let l = lines(text);
        assert_eq!(l[0].end, 2);
        assert_eq!(l[1].start, 3);
        assert_eq!(l[1].end, 3 + 6 + 3);
        let e = toggle(text, 1).unwrap();
        assert_eq!(apply(text, &e), "\u{1F431}\n- [x] \u{1F431}x");
        let e = enter(text, l[1].end).unwrap();
        assert_eq!(apply(text, &e), "\u{1F431}\n- [ ] \u{1F431}x\n- [ ] ");
    }

    #[test]
    fn the_rolled_title_has_no_marker() {
        assert_eq!(title_of("Belanja\n- [ ] a"), "Belanja");
        assert_eq!(title_of("- [x]  Done thing \nx"), "Done thing");
        assert_eq!(title_of(""), "");
    }

    // ---------------------------------------------------------------- bullets

    #[test]
    fn a_bullet_line_is_a_marker_but_not_a_box() {
        let t = "Title\n\u{2022} one\n\u{2022} \nplain\n- [ ] box";
        let l = lines(t);
        assert!(l[1].is_bullet() && !l[1].is_item() && l[1].has_marker());
        assert_eq!(l[1].marker_len, BULLET_LEN);
        assert_eq!(l[1].content_start(), l[1].start + 2);
        assert!(l[2].is_bullet() && l[2].is_empty_marker());
        assert!(!l[3].has_marker());
        assert!(l[4].is_item() && !l[4].is_bullet());
        // A bullet without its space, or another dot, is text.
        for plain in ["\u{2022}x", "* x", "\u{00b7} x", " \u{2022} x"] {
            assert!(!lines(plain)[0].has_marker(), "{plain:?}");
        }
    }

    #[test]
    fn bullets_are_not_counted_or_boxed() {
        let t = "t\n\u{2022} a\n\u{2022} b\n- [x] c\n- [ ] d";
        assert_eq!(counts(t), (1, 2), "boxes and counts stay checklist only");
        assert!(!all_done(t));
        assert!(all_done("t\n\u{2022} a\n- [x] c"));
        assert!(!all_done("t\n\u{2022} a"), "bullets alone are never all done");
        assert_eq!(title_of("\u{2022} Heading\nx"), "Heading");
    }

    #[test]
    fn enter_continues_a_bullet_and_ends_it_when_empty() {
        let t = "t\n\u{2022} milk";
        let n = t.encode_utf16().count();
        let e = enter(t, n).unwrap();
        assert_eq!(apply(t, &e), "t\n\u{2022} milk\n\u{2022} ");
        assert_eq!(e.caret, n + 3);
        // In the middle the rest moves to the new bullet.
        let e = enter(t, n - 2).unwrap();
        assert_eq!(apply(t, &e), "t\n\u{2022} mi\n\u{2022} lk");
        // An empty bullet loses its marker.
        let t = "t\n\u{2022} ";
        let e = enter(t, 4).unwrap();
        assert_eq!(apply(t, &e), "t\n");
        assert_eq!(e.caret, 2);
        // An emoji on a bullet line: the caret counts units.
        let t = "t\n\u{2022} \u{1F600}";
        let n = t.encode_utf16().count();
        let e = enter(t, n).unwrap();
        assert_eq!(apply(t, &e), "t\n\u{2022} \u{1F600}\n\u{2022} ");
        assert_eq!(e.caret, n + 3);
    }

    #[test]
    fn backspace_at_a_bullets_text_start_takes_the_marker() {
        let t = "t\n\u{2022} abc";
        let e = backspace(t, 4).unwrap();
        assert_eq!(apply(t, &e), "t\nabc");
        assert_eq!(e.caret, 2);
        assert_eq!(backspace(t, 5), None, "inside the text it is the EDIT's");
        assert_eq!(backspace("t\nabc", 2), None);
    }

    #[test]
    fn the_caret_never_rests_in_a_bullet_marker() {
        let t = "ab\n\u{2022} cd";
        // Home on the bullet line lands on its text.
        assert_eq!(snap_caret(t, 3, 6), 5);
        // A step left from the text goes to the end of the line before.
        assert_eq!(snap_caret(t, 4, 5), 2);
        assert_eq!(snap_caret(t, 5, 3), 5);
    }

    #[test]
    fn display_hides_a_bullet_marker_and_maps_both_ways() {
        let t = "ab\n\u{2022} cd\nef";
        assert_eq!(display_text(t), "ab\ncd\nef");
        assert_eq!(raw_to_display(t, 5), 3);
        assert_eq!(raw_to_display(t, 4), 3, "inside the marker is the text start");
        assert_eq!(display_to_raw(t, 3), 5);
        assert_eq!(display_to_raw(t, 5), 7);
        let t = "\u{1F600}\n\u{2022} \u{1F601}x";
        assert_eq!(display_text(t), "\u{1F600}\n\u{1F601}x");
        assert_eq!(display_to_raw(t, 3), 5);
    }

    #[test]
    fn autobox_is_unchanged_by_bullets() {
        let e = autobox("-", 1).unwrap();
        assert_eq!(apply("-", &e), "- [ ] ");
        assert_eq!(autobox("\u{2022} -", 3), None, "not at the line start");
    }

    #[test]
    fn toggle_list_bullets_a_line_and_takes_it_off_again() {
        let t = "abc\ndef";
        let e = toggle_list(t, 5, 5, ListKind::Bullet);
        assert_eq!(apply(t, &e), "abc\n\u{2022} def");
        assert_eq!(e.caret, 7);
        let t2 = "abc\n\u{2022} def";
        let e = toggle_list(t2, 7, 7, ListKind::Bullet);
        assert_eq!(apply(t2, &e), t);
        assert_eq!(e.caret, 5);
    }

    #[test]
    fn toggle_list_converts_the_other_kind() {
        let t = "t\n- [x] done\nplain";
        // Bullet over a checklist line and a plain line: both become bullets.
        let e = toggle_list(t, 2, t.len(), ListKind::Bullet);
        assert_eq!(apply(t, &e), "t\n\u{2022} done\n\u{2022} plain");
        // And back to checks: all bullets convert (not all have checks).
        let u = "\u{2022} a\n\u{2022} b";
        let e = toggle_list(u, 0, u.len(), ListKind::Check);
        assert_eq!(apply(u, &e), "- [ ] a\n- [ ] b");
        // All are checks now: the same call removes them.
        let v = "- [ ] a\n- [x] b";
        let e = toggle_list(v, 0, v.len(), ListKind::Check);
        assert_eq!(apply(v, &e), "a\nb");
    }

    #[test]
    fn toggle_list_over_a_selection_touches_every_line_it_reaches() {
        let t = "one\ntwo\nthree\nfour";
        // From inside "two" to inside "three".
        let (e, (a, b)) = toggle_list_sel(t, 5, 10, ListKind::Bullet);
        assert_eq!(apply(t, &e), "one\n\u{2022} two\n\u{2022} three\nfour");
        assert_eq!((a, b), (7, 14), "the selection moves with its text");
        // A selection that ends at the start of a line does not touch it.
        let e = toggle_list(t, 0, 4, ListKind::Bullet);
        assert_eq!(apply(t, &e), "\u{2022} one\ntwo\nthree\nfour");
        // A reversed selection is the same.
        let e2 = toggle_list(t, 4, 0, ListKind::Bullet);
        assert_eq!(e2.insert, e.insert);
        // Mixed lines: some already have it, the rest get it.
        let m = "\u{2022} a\nb";
        let e = toggle_list(m, 0, m.len(), ListKind::Bullet);
        assert_eq!(apply(m, &e), "\u{2022} a\n\u{2022} b");
    }

    #[test]
    fn toggle_list_on_an_emoji_line_counts_units() {
        let t = "t\n\u{1F600} hi";
        let e = toggle_list(t, 5, 5, ListKind::Bullet);
        assert_eq!(apply(t, &e), "t\n\u{2022} \u{1F600} hi");
        assert_eq!(e.caret, 5 + 2);
        let t = "t\n\u{2022} \u{1F600} hi";
        let e = toggle_list(t, 8, 8, ListKind::Bullet);
        assert_eq!(apply(t, &e), "t\n\u{1F600} hi");
        assert_eq!(e.caret, 6);
        // An empty line takes a marker too (a caret on a blank line).
        let e = toggle_list("a\n", 2, 2, ListKind::Bullet);
        assert_eq!(apply("a\n", &e), "a\n\u{2022} ");
        assert_eq!(e.caret, 4);
    }
}
