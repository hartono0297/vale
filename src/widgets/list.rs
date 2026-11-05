//! The sticky note list ("Daftar catatan tempel"): the rules behind the list
//! window (`list_win`), pure and tested without a window. One row per sticky
//! note (every sheet of a stack has its own), the search, the geometry the
//! drawing and the pointer share, where the window goes on the screen, the
//! keyboard selection, what "show and focus" does to a note, and the undo
//! snapshot of a delete.

use crate::capture::Rect as Px;
use crate::notes::{Note, NotesStore};
use crate::ui::geometry::Rect;
use crate::widgets::host::WidgetEvent;
use crate::widgets::sticky::{Paper, Sticky};
use crate::widgets::{checklist, remind, stack};

// ------------------------------------------------------------------ rows

/// How many characters of a note's body a row's snippet keeps.
pub const SNIPPET_MAX: usize = 120;
/// What joins the lines of a snippet.
const SNIPPET_JOIN: &str = " \u{b7} ";

/// One note of the list, as its row shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub paper: Paper,
    /// The first line without its checklist marker, trimmed; empty for an
    /// empty note (the caller shows "Catatan kosong").
    pub title: String,
    /// The lines after the first, markers removed, joined and capped.
    pub snippet: String,
    /// Unix seconds of the last edit.
    pub updated: i64,
    pub tucked: bool,
    pub rolled: bool,
    pub pinned: bool,
    /// The sheet's 1-based place and the stack's size, for a real stack.
    pub stack: Option<(usize, usize)>,
    /// A reminder is set: `true` once it is overdue (fired, not opened).
    pub reminder: Option<bool>,
    /// Done and total items of a checklist.
    pub progress: Option<(usize, usize)>,
}

/// `text` without a checklist marker at its start.
fn unmarked(line: &str) -> &str {
    line.strip_prefix(checklist::OPEN)
        .or_else(|| line.strip_prefix(checklist::DONE))
        .or_else(|| line.strip_prefix("- [X] "))
        .unwrap_or(line)
}

/// The text after the first line as one line: markers removed, blank lines
/// dropped, the rest joined with " \u{b7} ", whitespace collapsed, capped at
/// `SNIPPET_MAX` characters (cut on a character boundary, never inside a
/// code point).
pub fn snippet_of(text: &str) -> String {
    let text = checklist::normalize(text);
    let lines: Vec<String> = text
        .split('\n')
        .skip(1)
        .map(|l| unmarked(l).split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect();
    let joined = lines.join(SNIPPET_JOIN);
    if joined.chars().count() <= SNIPPET_MAX {
        return joined;
    }
    let cut: String = joined.chars().take(SNIPPET_MAX).collect();
    cut.trim_end().trim_end_matches('\u{b7}').trim_end().to_owned()
}

/// The title of a row: the first line, marker removed, trimmed.
pub fn title_of(text: &str) -> String {
    checklist::title_of(&checklist::normalize(text))
}

/// A reminder worth showing: set, and either still ahead or overdue (a fired
/// one the clock went back before shows nothing). `true` is overdue.
fn reminder_of(s: &Sticky, now: i64) -> Option<bool> {
    s.remind_at?;
    let overdue = remind::overdue(s, now);
    (!s.remind_fired || overdue).then_some(overdue)
}

/// One row per sticky note, newest edit first (ties by id). A stack's sheets
/// each have a row with their place in the stack.
pub fn rows(notes: &[Note], now: i64) -> Vec<Row> {
    let mut out = Vec::new();
    for unit in stack::units(notes) {
        let len = unit.sheets.len();
        for (k, n) in unit.sheets.iter().enumerate() {
            let Some(s) = n.sticky.as_ref() else { continue };
            let (done, total) = checklist::counts(&n.text);
            out.push(Row {
                id: n.id.clone(),
                paper: s.paper,
                title: title_of(&n.text),
                snippet: snippet_of(&n.text),
                updated: n.updated,
                tucked: s.tuck.is_some(),
                rolled: s.rolled,
                pinned: s.pinned,
                stack: unit.is_stack().then_some((k + 1, len)),
                reminder: reminder_of(s, now),
                progress: (total > 0).then_some((done, total)),
            });
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
    out
}

// ------------------------------------------------------------------ search

/// Whether `row` (whose note text is `note_text`) matches `query`: the query
/// is trimmed and compared case-insensitively (`to_lowercase`, so it is
/// Unicode-safe) against the title and the whole body. An empty query matches
/// everything.
pub fn matches(row: &Row, note_text: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    row.title.to_lowercase().contains(&q) || note_text.to_lowercase().contains(&q)
}

/// The rows that match `query`, in their order.
pub fn filter(rows: &[Row], notes: &[Note], query: &str) -> Vec<Row> {
    rows.iter()
        .filter(|r| {
            let text = notes.iter().find(|n| n.id == r.id).map_or("", |n| n.text.as_str());
            matches(r, text, query)
        })
        .cloned()
        .collect()
}

// ------------------------------------------------------------------ geometry

/// The window's size in dip.
pub const WIN_W: f32 = 340.0;
pub const WIN_H: f32 = 460.0;
/// The header: title and count at the left, (+) and the close button at the
/// right.
pub const HEADER_H: f32 = 48.0;
pub const BTN: f32 = 28.0;
pub const BTN_GAP: f32 = 4.0;
/// The search box.
pub const SEARCH_H: f32 = 36.0;
pub const SIDE: f32 = 12.0;
/// The gap between the search box and the first row.
pub const SEARCH_GAP: f32 = 8.0;
pub const ROW_H: f32 = 60.0;
/// The undo strip at the bottom.
pub const STRIP_H: f32 = 40.0;
pub const UNDO_W: f32 = 76.0;
pub const UNDO_H: f32 = 28.0;

/// Where everything is in a window `w` x `h` dip (the origin is the window's
/// top-left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub plus: Rect,
    pub close: Rect,
    pub search: Rect,
    /// Where the rows scroll; the strip, while shown, is not part of it.
    pub rows_view: Rect,
    pub strip: Option<Rect>,
    pub undo_btn: Option<Rect>,
}

/// The layout of a `w` x `h` window, with the undo strip when `strip`.
pub fn layout(w: f32, h: f32, strip: bool) -> Layout {
    let close = Rect {
        x: w - SIDE - BTN,
        y: (HEADER_H - BTN) / 2.0,
        w: BTN,
        h: BTN,
    };
    let plus = Rect { x: close.x - BTN_GAP - BTN, ..close };
    let search = Rect {
        x: SIDE,
        y: HEADER_H,
        w: (w - 2.0 * SIDE).max(0.0),
        h: SEARCH_H,
    };
    let top = HEADER_H + SEARCH_H + SEARCH_GAP;
    let strip_h = if strip { STRIP_H } else { 0.0 };
    let rows_view = Rect {
        x: 0.0,
        y: top,
        w,
        h: (h - top - strip_h).max(0.0),
    };
    let strip_rect = strip.then_some(Rect { x: 0.0, y: h - STRIP_H, w, h: STRIP_H });
    let undo_btn = strip_rect.map(|s| Rect {
        x: s.right() - SIDE - UNDO_W,
        y: s.y + (STRIP_H - UNDO_H) / 2.0,
        w: UNDO_W,
        h: UNDO_H,
    });
    Layout { w, h, plus, close, search, rows_view, strip: strip_rect, undo_btn }
}

/// The window is never shorter than this, even on a short work area.
pub const MIN_H: f32 = 240.0;

/// The scale of a monitor at `dpi` (96 is 1).
pub fn scale_of(dpi: u32) -> f32 {
    dpi.max(96) as f32 / 96.0
}

/// How tall the window is in dip on a work area `work_h` px tall at `scale`:
/// `WIN_H`, or less on a short screen so it stays inside it.
pub fn fit_height(work_h: i32, scale: f32) -> f32 {
    WIN_H.min(work_h as f32 / scale - 2.0 * SIDE).max(MIN_H)
}

/// The window's size in px for `w` x `h` dip at `scale`.
pub fn size_px(w: f32, h: f32, scale: f32) -> (i32, i32) {
    ((w * scale).round() as i32, (h * scale).round() as i32)
}

/// The empty states: the message (no notes, or no match) and, with no notes at
/// all, the "Buat catatan" button under it, centred in the rows' area.
pub fn empty_message(l: &Layout) -> Rect {
    Rect {
        x: SIDE,
        y: l.rows_view.y + l.rows_view.h / 2.0 - 40.0,
        w: (l.w - 2.0 * SIDE).max(0.0),
        h: 24.0,
    }
}

pub const EMPTY_BTN_W: f32 = 140.0;
pub const EMPTY_BTN_H: f32 = 32.0;

pub fn empty_button(l: &Layout) -> Rect {
    let m = empty_message(l);
    Rect {
        x: (l.w - EMPTY_BTN_W) / 2.0,
        y: m.bottom() + 12.0,
        w: EMPTY_BTN_W,
        h: EMPTY_BTN_H,
    }
}

/// What a point of the window is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Plus,
    Close,
    Search,
    Row(usize),
    Undo,
    /// The header outside its buttons: it drags the window.
    Header,
    None,
}

/// The row under window y `y` when scrolled by `scroll` with `n` rows.
pub fn row_at(l: &Layout, y: f32, scroll: f32, n: usize) -> Option<usize> {
    if y < l.rows_view.y || y >= l.rows_view.bottom() {
        return None;
    }
    let i = ((y - l.rows_view.y + scroll) / ROW_H).floor();
    (i >= 0.0 && (i as usize) < n).then_some(i as usize)
}

/// How far the rows can scroll.
pub fn max_scroll(l: &Layout, n: usize) -> f32 {
    (n as f32 * ROW_H - l.rows_view.h).max(0.0)
}

pub fn clamp_scroll(scroll: f32, l: &Layout, n: usize) -> f32 {
    scroll.clamp(0.0, max_scroll(l, n))
}

/// A wheel turn of `delta` (120 per notch, positive is up): one notch is one
/// row.
pub fn wheel(scroll: f32, delta: i32, l: &Layout, n: usize) -> f32 {
    clamp_scroll(scroll - delta as f32 / 120.0 * ROW_H, l, n)
}

/// The scroll that shows row `index` whole, moving as little as it can.
pub fn ensure_visible(scroll: f32, index: usize, l: &Layout) -> f32 {
    let top = index as f32 * ROW_H;
    if top < scroll {
        top
    } else if top + ROW_H > scroll + l.rows_view.h {
        (top + ROW_H - l.rows_view.h).max(0.0)
    } else {
        scroll
    }
}

/// What window point (`x`, `y`) is on.
pub fn hit(l: &Layout, x: f32, y: f32, scroll: f32, n: usize) -> Hit {
    if l.plus.contains(x, y) {
        return Hit::Plus;
    }
    if l.close.contains(x, y) {
        return Hit::Close;
    }
    if l.undo_btn.is_some_and(|r| r.contains(x, y)) {
        return Hit::Undo;
    }
    if l.search.contains(x, y) {
        return Hit::Search;
    }
    if x >= l.rows_view.x
        && x < l.rows_view.right()
        && let Some(i) = row_at(l, y, scroll, n)
    {
        return Hit::Row(i);
    }
    if y >= 0.0 && y < HEADER_H && x >= 0.0 && x < l.w {
        return Hit::Header;
    }
    Hit::None
}

/// The parts of one row, in dip from the row's top-left (`ROW_H` tall).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowGeo {
    /// The hover and selection plate.
    pub card: Rect,
    /// The paper colour's strip at the left.
    pub strip: Rect,
    pub title: Rect,
    pub time: Rect,
    pub snippet: Rect,
    /// The status icons, right-aligned under the time.
    pub icons: Rect,
    /// The stack badge and the checklist count, when the row has either.
    pub meta: Option<Rect>,
}

/// Width of the time label's box, of the status icons' box and of one icon.
pub const TIME_W: f32 = 88.0;
pub const ICONS_W: f32 = 64.0;
pub const ICON: f32 = 13.0;
pub const ICON_GAP: f32 = 3.0;
/// Where a row's text starts and how far from the window's right it ends.
pub const TEXT_X: f32 = 24.0;
pub const RIGHT_PAD: f32 = 16.0;

/// `meta`: the row has a stack badge or a checklist count, so it is three
/// lines tall; otherwise the two lines are centred.
pub fn row_geo(w: f32, meta: bool) -> RowGeo {
    let right = w - RIGHT_PAD;
    let (title_y, title_h, snippet_y) = if meta { (6.0, 18.0, 24.0) } else { (10.0, 20.0, 31.0) };
    let time = Rect { x: right - TIME_W, y: title_y, w: TIME_W, h: title_h };
    let icons = Rect { x: right - ICONS_W, y: snippet_y, w: ICONS_W, h: 16.0 };
    RowGeo {
        card: Rect { x: 6.0, y: 2.0, w: (w - 12.0).max(0.0), h: ROW_H - 4.0 },
        strip: Rect { x: 12.0, y: 10.0, w: 4.0, h: ROW_H - 20.0 },
        title: Rect { x: TEXT_X, y: title_y, w: (time.x - 8.0 - TEXT_X).max(0.0), h: title_h },
        time,
        snippet: Rect { x: TEXT_X, y: snippet_y, w: (icons.x - 8.0 - TEXT_X).max(0.0), h: 16.0 },
        icons,
        meta: meta.then_some(Rect { x: TEXT_X, y: 41.0, w: (right - TEXT_X).max(0.0), h: 14.0 }),
    }
}

/// Whether the row shows a third line.
pub fn has_meta(r: &Row) -> bool {
    r.stack.is_some() || r.progress.is_some()
}

/// The icons a row shows right-aligned under its time, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Pinned,
    Rolled,
    Tucked,
    /// A reminder; `true` is overdue (drawn in the accent).
    Reminder(bool),
}

pub fn marks(r: &Row) -> Vec<Mark> {
    let mut out = Vec::new();
    if r.pinned {
        out.push(Mark::Pinned);
    }
    if r.rolled {
        out.push(Mark::Rolled);
    }
    if r.tucked {
        out.push(Mark::Tucked);
    }
    if let Some(overdue) = r.reminder {
        out.push(Mark::Reminder(overdue));
    }
    out
}

/// Where mark number `i` of `n` goes inside `icons` (right-aligned).
pub fn mark_rect(icons: Rect, i: usize, n: usize) -> Rect {
    let from_right = (n - 1 - i) as f32;
    Rect {
        x: icons.right() - ICON - from_right * (ICON + ICON_GAP),
        y: icons.y + (icons.h - ICON) / 2.0,
        w: ICON,
        h: ICON,
    }
}

// ------------------------------------------------------------------ search box text

/// How far the search text is scrolled sideways (dip) so the caret at
/// `caret_x` (dip from the text's start) stays inside a field `view_w` wide.
pub fn follow_caret(scroll_x: f32, caret_x: f32, view_w: f32) -> f32 {
    let end = caret_x + 1.5;
    if caret_x < scroll_x {
        caret_x.max(0.0)
    } else if end > scroll_x + view_w {
        (end - view_w).max(0.0)
    } else {
        scroll_x
    }
}

// ------------------------------------------------------------------ placement

fn clamp_in(x: i32, y: i32, size: (i32, i32), work: Px) -> (i32, i32) {
    let x = x.min(work.right() - size.0).max(work.x);
    let y = y.min(work.bottom() - size.1).max(work.y);
    (x, y)
}

/// The window's top-left (px) beside note `note` on `work`: to its right with
/// `gap` px between, else to its left, else the right one pushed inside; its
/// top lines up with the note's. Always inside `work`.
pub fn place_beside(note: Px, work: Px, size: (i32, i32), gap: i32) -> (i32, i32) {
    let right = note.right() + gap;
    let left = note.x - gap - size.0;
    let x = if right + size.0 <= work.right() {
        right
    } else if left >= work.x {
        left
    } else {
        right
    };
    clamp_in(x, note.y, size, work)
}

/// The window's top-left (px) at the top right of `work`, `margin` px in.
pub fn place_default(work: Px, size: (i32, i32), margin: i32) -> (i32, i32) {
    clamp_in(work.right() - size.0 - margin, work.y + margin, size, work)
}

// ------------------------------------------------------------------ keyboard selection

/// The selection after `delta` rows of Up (-1) or Down (+1) with `n` rows:
/// nothing selected goes to the first on Down and the last on Up; otherwise
/// it moves and stops at the ends. No rows, no selection.
pub fn move_selection(sel: Option<usize>, delta: i32, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    match sel {
        None if delta > 0 => Some(0),
        None if delta < 0 => Some(n - 1),
        None => None,
        Some(i) => Some((i as i64 + i64::from(delta)).clamp(0, n as i64 - 1) as usize),
    }
}

/// Typing in the search box starts over: nothing is selected.
pub fn selection_after_typing(_before: Option<usize>) -> Option<usize> {
    None
}

/// A selection the rows no longer reach (the list got shorter) is dropped.
pub fn settle_selection(sel: Option<usize>, n: usize) -> Option<usize> {
    sel.filter(|i| *i < n)
}

/// The row Enter opens: the selected one, else the first visible.
pub fn enter_target(sel: Option<usize>, n: usize) -> Option<usize> {
    settle_selection(sel, n).or((n > 0).then_some(0))
}

// ------------------------------------------------------------------ show and focus

/// What "show and focus" changes in note `id` before it is brought to the
/// front: a tucked note comes out, a rolled one unrolls, a hidden sheet of a
/// stack comes to the top. Nothing for what is already so.
pub fn show_events(notes: &[Note], id: &str) -> Vec<WidgetEvent> {
    let mut out = Vec::new();
    let Some(s) = notes.iter().find(|n| n.id == id).and_then(|n| n.sticky.as_ref()) else {
        return out;
    };
    if s.tuck.is_some() {
        out.push(WidgetEvent::Tuck { id: id.to_owned(), edge: None });
    }
    if s.rolled {
        out.push(WidgetEvent::Rolled { id: id.to_owned(), on: false });
    }
    let units = stack::units(notes);
    let hidden = stack::unit_of(&units, id)
        .is_some_and(|u| u.is_stack() && u.sheets[u.top()].id != id);
    if hidden {
        out.push(WidgetEvent::StackShow { id: id.to_owned() });
    }
    out
}

// ------------------------------------------------------------------ undo

/// What a delete took away, kept until it is committed (the strip's 5 s, the
/// next delete, or the window closing): the note as it was, and the sticky
/// part of every other sheet of its stack as it was before the rest closed up.
#[derive(Clone, Debug, PartialEq)]
pub struct Undo {
    pub note: Note,
    pub mates: Vec<(String, Sticky)>,
}

impl Undo {
    /// The snapshot of note `id`, taken before it is deleted.
    pub fn capture(notes: &[Note], id: &str) -> Option<Undo> {
        let note = notes.iter().find(|n| n.id == id)?.clone();
        let units = stack::units(notes);
        let mates = stack::unit_of(&units, id)
            .filter(|u| u.is_stack())
            .map(|u| {
                u.sheets
                    .iter()
                    .filter(|n| n.id != id)
                    .filter_map(|n| n.sticky.as_ref().map(|s| (n.id.clone(), s.clone())))
                    .collect()
            })
            .unwrap_or_default();
        Some(Undo { note, mates })
    }

    /// Puts the note back with its own id, text, time and sticky part. Refused
    /// when the id exists or the desktop is full. Each mate still on the
    /// desktop gets its `stack`, `stack_order` and `stack_top` back and
    /// nothing else; when the note rejoins a stack it takes that stack's
    /// current shared fields (place, size, monitor, pin, roll, tuck), so the
    /// stack is one window again. The caller runs `apply::settle` after.
    pub fn restore(self, store: &mut NotesStore) -> bool {
        let Undo { note, mates } = self;
        let id = note.id.clone();
        let own_stack = note.sticky.as_ref().and_then(|s| s.stack.clone());
        if !store.restore(note) {
            return false;
        }
        let mut joined: Option<Sticky> = None;
        for (mate, kept) in mates {
            let Some(mut now) = store.get(&mate).and_then(|n| n.sticky.clone()) else {
                continue;
            };
            now.stack.clone_from(&kept.stack);
            now.stack_order = kept.stack_order;
            now.stack_top = kept.stack_top;
            store.set_sticky(&mate, Some(now.clone()));
            if own_stack.is_some() && kept.stack == own_stack && joined.is_none() {
                joined = Some(now);
            }
        }
        if let Some(from) = joined
            && let Some(mut mine) = store.get(&id).and_then(|n| n.sticky.clone())
        {
            stack::share(&from, &mut mine);
            store.set_sticky(&id, Some(mine));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::apply;
    use crate::widgets::sticky::{DipRect, MAX_STICKY};
    use crate::widgets::tuck::Edge;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn note(id: &str, text: &str, updated: i64, f: impl FnOnce(&mut Sticky)) -> Note {
        let mut s = Sticky::default();
        f(&mut s);
        Note { id: id.into(), text: text.into(), updated, sticky: Some(s) }
    }

    fn plain(id: &str, text: &str) -> Note {
        Note { id: id.into(), text: text.into(), updated: 1, sticky: None }
    }

    // ------------------------------------------------------------ rows

    #[test]
    fn a_row_has_the_title_the_snippet_and_the_flags_of_its_note() {
        let notes = [note("a", "Belanja\n- [x] bata\n- [ ] kaca\nbiasa", 50, |s| {
            s.paper = Paper::Laut;
            s.pinned = true;
        })];
        let r = &rows(&notes, 100)[0];
        assert_eq!(r.id, "a");
        assert_eq!(r.paper, Paper::Laut);
        assert_eq!(r.title, "Belanja");
        assert_eq!(r.snippet, "bata \u{b7} kaca \u{b7} biasa");
        assert_eq!(r.updated, 50);
        assert!(r.pinned && !r.rolled && !r.tucked);
        assert_eq!((r.stack, r.reminder, r.progress), (None, None, Some((1, 2))));
    }

    #[test]
    fn only_sticky_notes_have_rows() {
        let notes = [plain("p", "plain"), note("a", "x", 1, |_| {}), plain("q", "plain too")];
        let r = rows(&notes, 0);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "a");
    }

    #[test]
    fn the_title_has_no_marker_and_an_empty_note_has_an_empty_one() {
        assert_eq!(title_of("- [ ] Beli kopi\nx"), "Beli kopi");
        assert_eq!(title_of("- [x] Selesai"), "Selesai");
        assert_eq!(title_of("- [X] Selesai"), "Selesai");
        assert_eq!(title_of("  spasi  \nx"), "spasi");
        assert_eq!(title_of(""), "");
        assert_eq!(title_of("\n\nbaris tiga"), "", "the first line is the title, even blank");
        assert_eq!(title_of("Judul\r\nisi"), "Judul", "a CRLF text reads the same");
    }

    #[test]
    fn the_snippet_drops_markers_blank_lines_and_extra_blanks() {
        assert_eq!(snippet_of("Judul"), "");
        assert_eq!(snippet_of("Judul\n\n  \n"), "");
        assert_eq!(snippet_of("J\n- [x] satu\n\n- [ ]   dua   tiga \n- [X] empat"), "satu \u{b7} dua tiga \u{b7} empat");
        // A marker alone leaves nothing; "- [ ]" without its space is text.
        assert_eq!(snippet_of("J\n- [ ] \nbiasa"), "biasa");
        assert_eq!(snippet_of("J\n- [ ]tanpa spasi"), "- [ ]tanpa spasi");
        assert_eq!(snippet_of("J\r\nbaris\r\ndua"), "baris \u{b7} dua");
        assert_eq!(snippet_of("J\n\tbertab\tdi\ttengah"), "bertab di tengah");
    }

    #[test]
    fn the_snippet_is_capped_on_a_character_boundary_emoji_included() {
        let long = format!("J\n{}", "a".repeat(300));
        assert_eq!(snippet_of(&long).chars().count(), SNIPPET_MAX);
        // 200 emoji (two UTF-16 units each, four bytes): cut after 120 of them.
        let emoji = format!("J\n{}", "\u{1F600}".repeat(200));
        let s = snippet_of(&emoji);
        assert_eq!(s.chars().count(), SNIPPET_MAX);
        assert!(s.chars().all(|c| c == '\u{1F600}'), "never half an emoji");
        // The cut never leaves a dangling joiner or blank.
        let many = format!("J\n{}", "kata ".repeat(40));
        let s = snippet_of(&many);
        assert!(s.chars().count() <= SNIPPET_MAX);
        assert_eq!(s, s.trim());
        let lines = format!("J\n{}", (0..60).map(|i| format!("b{i}")).collect::<Vec<_>>().join("\n"));
        let s = snippet_of(&lines);
        assert!(s.chars().count() <= SNIPPET_MAX && !s.ends_with('\u{b7}') && !s.ends_with(' '), "{s:?}");
        // A snippet under the cap is whole.
        assert_eq!(snippet_of("J\nsingkat"), "singkat");
    }

    #[test]
    fn rows_are_newest_first_and_ties_go_by_id() {
        let notes = [
            note("c", "c", 10, |_| {}),
            note("a", "a", 30, |_| {}),
            note("b", "b", 30, |_| {}),
            note("d", "d", 20, |_| {}),
        ];
        let ids: Vec<String> = rows(&notes, 0).into_iter().map(|r| r.id).collect();
        assert_eq!(ids, ["a", "b", "d", "c"]);
    }

    #[test]
    fn the_status_flags_follow_the_sticky_part() {
        let notes = [
            note("rolled", "x", 5, |s| s.rolled = true),
            note("tucked", "x", 4, |s| s.tuck = Some(Edge::Right)),
            note("pinned", "x", 3, |s| s.pinned = true),
            note("plain", "x", 2, |_| {}),
        ];
        let r = rows(&notes, 0);
        let by = |id: &str| r.iter().find(|r| r.id == id).unwrap();
        assert!(by("rolled").rolled && !by("rolled").tucked && !by("rolled").pinned);
        assert!(by("tucked").tucked && !by("tucked").rolled);
        assert!(by("pinned").pinned);
        let p = by("plain");
        assert!(!p.rolled && !p.tucked && !p.pinned && p.stack.is_none() && p.progress.is_none());
    }

    #[test]
    fn a_reminder_shows_while_ahead_and_overdue_but_not_a_fired_one_the_clock_went_back_on() {
        let now = 1_000;
        let notes = [
            note("ahead", "x", 4, |s| s.remind_at = Some(now + 60)),
            note("overdue", "x", 3, |s| {
                s.remind_at = Some(now - 60);
                s.remind_fired = true;
            }),
            note("back", "x", 2, |s| {
                s.remind_at = Some(now + 60);
                s.remind_fired = true;
            }),
            note("none", "x", 1, |_| {}),
            // Past its time but not fired yet: still a reminder, not overdue.
            note("due", "x", 0, |s| s.remind_at = Some(now - 5)),
        ];
        let r = rows(&notes, now);
        let by = |id: &str| r.iter().find(|r| r.id == id).unwrap().reminder;
        assert_eq!(by("ahead"), Some(false));
        assert_eq!(by("overdue"), Some(true));
        assert_eq!(by("back"), None);
        assert_eq!(by("none"), None);
        assert_eq!(by("due"), Some(false));
    }

    #[test]
    fn progress_is_done_over_total_only_when_there_are_items() {
        let notes = [
            note("a", "L\n- [x] a\n- [ ] b\n- [x] c", 3, |_| {}),
            note("b", "L\n- [ ] a", 2, |_| {}),
            note("c", "L\nno items", 1, |_| {}),
        ];
        let r = rows(&notes, 0);
        assert_eq!(r[0].progress, Some((2, 3)));
        assert_eq!(r[1].progress, Some((0, 1)));
        assert_eq!(r[2].progress, None);
    }

    fn sheet(id: &str, order: u32, stk: &str, updated: i64) -> Note {
        note(id, &format!("lembar {id}"), updated, |s| {
            s.stack = Some(stk.into());
            s.stack_order = order;
            s.stack_top = order == 0;
        })
    }

    #[test]
    fn every_sheet_of_a_stack_has_a_row_with_its_place_by_stack_order() {
        // Listed out of order, with different edit times.
        let notes = [
            sheet("c", 2, "stk-x", 30),
            sheet("a", 0, "stk-x", 10),
            note("solo", "solo", 20, |_| {}),
            sheet("b", 1, "stk-x", 40),
        ];
        let r = rows(&notes, 0);
        assert_eq!(r.len(), 4, "a row per sheet");
        let by = |id: &str| r.iter().find(|r| r.id == id).unwrap().stack;
        assert_eq!(by("a"), Some((1, 3)));
        assert_eq!(by("b"), Some((2, 3)));
        assert_eq!(by("c"), Some((3, 3)));
        assert_eq!(by("solo"), None);
        // The list itself is by edit time, not by place in the stack.
        let ids: Vec<&str> = r.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["b", "c", "solo", "a"]);
    }

    #[test]
    fn a_stack_id_one_live_sheet_carries_alone_counts_as_no_stack() {
        let notes = [sheet("a", 0, "stk-gone", 10), plain("p", "plain")];
        assert_eq!(rows(&notes, 0)[0].stack, None);
    }

    // ------------------------------------------------------------ search

    #[test]
    fn an_empty_or_blank_query_matches_everything() {
        let notes = [note("a", "Belanja\nbata", 1, |_| {}), note("b", "", 2, |_| {})];
        let all = rows(&notes, 0);
        for q in ["", "   ", "\t "] {
            assert_eq!(filter(&all, &notes, q).len(), 2, "{q:?}");
        }
    }

    #[test]
    fn search_is_case_insensitive_trimmed_and_looks_in_the_title_and_the_body() {
        let notes = [
            note("a", "Belanja Material\nbata ekspos", 3, |_| {}),
            note("b", "Telp Pak Budi\nsoal atap", 2, |_| {}),
            note("c", "Ide\n- [ ] BATA merah", 1, |_| {}),
        ];
        let all = rows(&notes, 0);
        let ids = |q: &str| -> Vec<String> { filter(&all, &notes, q).into_iter().map(|r| r.id).collect() };
        assert_eq!(ids("belanja"), ["a"], "title, any case");
        assert_eq!(ids("  BATA  "), ["a", "c"], "body, trimmed");
        assert_eq!(ids("atap"), ["b"]);
        assert_eq!(ids("zzz"), Vec::<String>::new());
        // The body beyond the snippet's cap is still searched.
        let far = [note("f", &format!("J\n{}\nkata-rahasia", "x ".repeat(300)), 1, |_| {})];
        let all = rows(&far, 0);
        assert_eq!(filter(&all, &far, "kata-rahasia").len(), 1);
        // A checklist marker is text the body search sees.
        assert_eq!(ids("[ ]").len(), 1);
    }

    #[test]
    fn search_lowercases_unicode_on_both_sides() {
        let notes = [
            note("a", "ÉCOLE\nStra\u{df}e", 3, |_| {}),
            note("b", "Ünï\u{c7}ode \u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}", 2, |_| {}),
            note("c", "\u{1F600} senyum", 1, |_| {}),
        ];
        let all = rows(&notes, 0);
        let hits = |q: &str| filter(&all, &notes, q).len();
        assert_eq!(hits("école"), 1);
        assert_eq!(hits("ÉCOLE"), 1);
        assert_eq!(hits("ünïçode"), 1);
        assert_eq!(hits("привет"), 1, "Cyrillic");
        assert_eq!(hits("ПРИВЕТ"), 1);
        assert_eq!(hits("\u{1F600}"), 1, "an emoji");
        assert_eq!(hits("senyum"), 1);
    }

    // ------------------------------------------------------------ geometry

    #[test]
    fn the_layout_puts_the_header_search_rows_and_strip_where_the_brief_says() {
        let l = layout(WIN_W, WIN_H, false);
        assert_eq!((l.close.w, l.close.h, l.plus.w, l.plus.h), (28.0, 28.0, 28.0, 28.0));
        assert_eq!(l.close.right(), WIN_W - 12.0, "right-aligned");
        assert_eq!(l.plus.right() + 4.0, l.close.x, "a 4 dip gap");
        assert!(l.close.y >= 0.0 && l.close.bottom() <= HEADER_H);
        assert_eq!(l.search, Rect { x: 12.0, y: 48.0, w: WIN_W - 24.0, h: 36.0 });
        assert_eq!(l.rows_view.y, 48.0 + 36.0 + 8.0);
        assert_eq!(l.rows_view.bottom(), WIN_H, "no strip: rows go to the bottom");
        assert!(l.strip.is_none() && l.undo_btn.is_none());
        // The strip takes 40 dip off the rows.
        let s = layout(WIN_W, WIN_H, true);
        assert_eq!(s.strip, Some(Rect { x: 0.0, y: WIN_H - 40.0, w: WIN_W, h: 40.0 }));
        assert_eq!(s.rows_view.bottom(), WIN_H - 40.0);
        let u = s.undo_btn.unwrap();
        let strip = s.strip.unwrap();
        assert!(u.x >= strip.x && u.right() <= strip.right() && u.y >= strip.y && u.bottom() <= strip.bottom());
        // Nothing overlaps.
        let rects = [s.plus, s.close, s.search, s.rows_view, strip];
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let apart = a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn a_row_at_a_y_and_the_scroll() {
        let l = layout(WIN_W, WIN_H, false);
        let top = l.rows_view.y;
        assert_eq!(row_at(&l, top - 1.0, 0.0, 5), None, "above the rows");
        assert_eq!(row_at(&l, top, 0.0, 5), Some(0));
        assert_eq!(row_at(&l, top + 59.9, 0.0, 5), Some(0));
        assert_eq!(row_at(&l, top + 60.0, 0.0, 5), Some(1));
        assert_eq!(row_at(&l, top + 4.0 * 60.0 + 1.0, 0.0, 5), Some(4));
        assert_eq!(row_at(&l, top + 5.0 * 60.0 + 1.0, 0.0, 5), None, "past the last");
        // Scrolled by one and a half rows.
        assert_eq!(row_at(&l, top, 90.0, 10), Some(1));
        assert_eq!(row_at(&l, top + 30.0, 90.0, 10), Some(2));
        assert_eq!(row_at(&l, l.rows_view.bottom(), 0.0, 100), None, "below the view");
        // The strip hides the rows under it.
        let s = layout(WIN_W, WIN_H, true);
        assert_eq!(row_at(&s, s.strip.unwrap().y + 1.0, 0.0, 100), None);
    }

    #[test]
    fn scrolling_is_clamped_and_a_notch_is_one_row() {
        let l = layout(WIN_W, WIN_H, false);
        assert_eq!(max_scroll(&l, 0), 0.0);
        assert_eq!(max_scroll(&l, 3), 0.0, "everything fits");
        let view = l.rows_view.h;
        assert_eq!(max_scroll(&l, 10), 10.0 * 60.0 - view);
        assert_eq!(clamp_scroll(-5.0, &l, 10), 0.0);
        assert_eq!(clamp_scroll(9_999.0, &l, 10), max_scroll(&l, 10));
        // Down (a negative delta) scrolls by a row; up goes back.
        assert_eq!(wheel(0.0, -120, &l, 10), 60.0);
        assert_eq!(wheel(60.0, -120, &l, 10), 120.0);
        assert_eq!(wheel(120.0, 120, &l, 10), 60.0);
        assert_eq!(wheel(0.0, 120, &l, 10), 0.0, "not above the top");
        assert_eq!(wheel(0.0, -120 * 50, &l, 10), max_scroll(&l, 10), "not below the end");
        assert_eq!(wheel(0.0, -120, &l, 3), 0.0, "nothing to scroll");
        // A smaller step (a precision wheel) is a part of a row.
        assert_eq!(wheel(0.0, -60, &l, 10), 30.0);
        // The strip makes the view shorter, so there is more to scroll.
        let s = layout(WIN_W, WIN_H, true);
        assert_eq!(max_scroll(&s, 10), max_scroll(&l, 10) + 40.0);
    }

    #[test]
    fn ensure_visible_moves_as_little_as_it_can() {
        let l = layout(WIN_W, WIN_H, false);
        let view = l.rows_view.h;
        assert_eq!(ensure_visible(0.0, 0, &l), 0.0, "already shown");
        assert_eq!(ensure_visible(0.0, 2, &l), 0.0);
        // Below the view: the row's bottom lines up with the view's bottom.
        let last_in = (view / ROW_H).floor() as usize;
        assert_eq!(ensure_visible(0.0, last_in, &l), (last_in + 1) as f32 * ROW_H - view);
        // Above the view: its top lines up with the top.
        assert_eq!(ensure_visible(300.0, 2, &l), 120.0);
        assert_eq!(ensure_visible(300.0, 0, &l), 0.0);
        // The result shows the row whole.
        for index in 0..30 {
            for start in [0.0, 100.0, 777.0] {
                let s = ensure_visible(start, index, &l);
                assert!(s <= index as f32 * ROW_H && (index + 1) as f32 * ROW_H <= s + view + 0.001, "{index} from {start}");
            }
        }
    }

    #[test]
    fn a_point_is_classified() {
        let l = layout(WIN_W, WIN_H, true);
        let c = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (x, y) = c(l.plus);
        assert_eq!(hit(&l, x, y, 0.0, 5), Hit::Plus);
        let (x, y) = c(l.close);
        assert_eq!(hit(&l, x, y, 0.0, 5), Hit::Close);
        let (x, y) = c(l.search);
        assert_eq!(hit(&l, x, y, 0.0, 5), Hit::Search);
        let (x, y) = c(l.undo_btn.unwrap());
        assert_eq!(hit(&l, x, y, 0.0, 5), Hit::Undo);
        let top = l.rows_view.y;
        assert_eq!(hit(&l, 100.0, top + 10.0, 0.0, 5), Hit::Row(0));
        assert_eq!(hit(&l, 100.0, top + 70.0, 0.0, 5), Hit::Row(1));
        assert_eq!(hit(&l, 100.0, top + 70.0, 60.0, 5), Hit::Row(2));
        // The header outside its buttons drags.
        assert_eq!(hit(&l, 20.0, 10.0, 0.0, 5), Hit::Header);
        assert_eq!(hit(&l, l.plus.x - 2.0, 24.0, 0.0, 5), Hit::Header);
        // Past the last row, the strip's free part and outside: nothing.
        assert_eq!(hit(&l, 100.0, top + 5.0 * 60.0 + 1.0, 0.0, 5), Hit::None);
        assert_eq!(hit(&l, 20.0, l.strip.unwrap().y + 5.0, 0.0, 5), Hit::None);
        assert_eq!(hit(&l, -1.0, top + 10.0, 0.0, 5), Hit::None);
        assert_eq!(hit(&l, WIN_W + 1.0, top + 10.0, 0.0, 5), Hit::None);
        assert_eq!(hit(&l, 100.0, -1.0, 0.0, 5), Hit::None);
        // Without the strip the undo button is not there.
        let plain = layout(WIN_W, WIN_H, false);
        assert_eq!(hit(&plain, x, y, 0.0, 100), Hit::Row(((y - plain.rows_view.y) / ROW_H) as usize));
    }

    #[test]
    fn the_window_scales_with_the_monitors_dpi_and_stays_on_a_short_screen() {
        assert_eq!(scale_of(96), 1.0);
        assert_eq!(scale_of(144), 1.5);
        assert_eq!(scale_of(192), 2.0);
        assert_eq!(scale_of(0), 1.0, "a failed query reads as 96");
        assert_eq!(size_px(WIN_W, WIN_H, 1.0), (340, 460));
        assert_eq!(size_px(WIN_W, WIN_H, 1.5), (510, 690));
        assert_eq!(size_px(WIN_W, WIN_H, 1.25), (425, 575));
        // A tall screen keeps the full height; a short one shrinks it, never
        // below the minimum.
        assert_eq!(fit_height(1040, 1.0), WIN_H);
        assert_eq!(fit_height(1040, 2.0), WIN_H);
        assert_eq!(fit_height(400, 1.0), 400.0 - 24.0);
        assert_eq!(fit_height(200, 1.0), MIN_H);
        // The smaller window still lays out.
        let l = layout(WIN_W, fit_height(400, 1.0), false);
        assert!(l.rows_view.h > ROW_H);
    }

    #[test]
    fn the_empty_message_and_button_are_centred_under_the_search_box() {
        for strip in [false, true] {
            let l = layout(WIN_W, WIN_H, strip);
            let m = empty_message(&l);
            let b = empty_button(&l);
            assert!(m.y >= l.rows_view.y && b.bottom() <= l.rows_view.bottom(), "inside the rows' area");
            assert!(m.bottom() <= b.y, "the button is under the message");
            assert_eq!(b.x + b.w / 2.0, WIN_W / 2.0, "centred");
            assert_eq!((b.w, b.h), (EMPTY_BTN_W, EMPTY_BTN_H));
        }
    }

    #[test]
    fn a_rows_parts_fit_inside_it_and_do_not_overlap() {
        for meta in [false, true] {
            for w in [300.0, WIN_W, 400.0] {
                let g = row_geo(w, meta);
                let mut parts = vec![g.title, g.time, g.snippet, g.icons];
                if let Some(m) = g.meta {
                    parts.push(m);
                }
                for p in &parts {
                    assert!(p.x >= g.card.x && p.right() <= g.card.right(), "{p:?} at {w}");
                    assert!(p.y >= g.card.y && p.bottom() <= g.card.bottom(), "{p:?} at {w}");
                }
                assert!(g.strip.right() <= g.title.x && g.strip.y >= g.card.y && g.strip.bottom() <= g.card.bottom());
                for (i, a) in parts.iter().enumerate() {
                    for b in &parts[i + 1..] {
                        let apart = a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y;
                        assert!(apart, "{a:?} overlaps {b:?} (meta {meta}, w {w})");
                    }
                }
                assert!(g.title.w > 150.0, "the title keeps room at {w}");
                assert_eq!(g.meta.is_some(), meta);
            }
        }
    }

    #[test]
    fn marks_come_in_a_fixed_order_and_sit_right_aligned() {
        let mut r = rows(&[note("a", "x", 1, |s| {
            s.pinned = true;
            s.rolled = true;
            s.tuck = Some(Edge::Left);
            s.remind_at = Some(5);
            s.remind_fired = true;
        })], 10)
        .remove(0);
        assert_eq!(marks(&r), [Mark::Pinned, Mark::Rolled, Mark::Tucked, Mark::Reminder(true)]);
        r.pinned = false;
        r.reminder = Some(false);
        assert_eq!(marks(&r), [Mark::Rolled, Mark::Tucked, Mark::Reminder(false)]);
        r.rolled = false;
        r.tucked = false;
        r.reminder = None;
        assert!(marks(&r).is_empty());
        let icons = row_geo(WIN_W, false).icons;
        let rects: Vec<Rect> = (0..4).map(|i| mark_rect(icons, i, 4)).collect();
        assert_eq!(rects[3].right(), icons.right(), "the last is flush right");
        assert!(rects[0].x >= icons.x, "four fit the box: {rects:?}");
        for w in rects.windows(2) {
            assert!(w[0].right() <= w[1].x, "no overlap");
        }
        assert_eq!(mark_rect(icons, 0, 1).right(), icons.right());
    }

    #[test]
    fn the_search_text_scrolls_to_keep_the_caret_in_view() {
        assert_eq!(follow_caret(0.0, 10.0, 100.0), 0.0);
        assert_eq!(follow_caret(0.0, 99.0, 100.0), 0.5, "just past the edge");
        assert_eq!(follow_caret(0.0, 250.0, 100.0), 151.5);
        assert_eq!(follow_caret(150.0, 120.0, 100.0), 120.0, "the caret went back left");
        assert_eq!(follow_caret(150.0, 0.0, 100.0), 0.0);
        assert_eq!(follow_caret(20.0, 60.0, 100.0), 20.0, "inside: unchanged");
    }

    // ------------------------------------------------------------ placement

    const WORK: Px = Px::new(0, 0, 1920, 1040);

    #[test]
    fn the_list_goes_right_of_the_note_with_the_gap_and_the_notes_top() {
        let note = Px::new(200, 300, 240, 208);
        assert_eq!(place_beside(note, WORK, (340, 460), 8), (448, 300));
    }

    #[test]
    fn the_list_goes_left_when_the_right_has_no_room() {
        let note = Px::new(1500, 300, 240, 208);
        // Right would end at 1748 + 340 = 2088 > 1920.
        assert_eq!(place_beside(note, WORK, (340, 460), 8), (1500 - 8 - 340, 300));
        // Exactly fitting on the right still goes right.
        let edge = Px::new(1920 - 340 - 8 - 240, 10, 240, 208);
        assert_eq!(place_beside(edge, WORK, (340, 460), 8).0, edge.right() + 8);
    }

    #[test]
    fn the_list_is_pushed_inside_when_neither_side_has_room_or_it_is_too_low() {
        let wide = Px::new(100, 100, 1700, 800);
        let (x, y) = place_beside(wide, WORK, (340, 460), 8);
        assert!(x >= WORK.x && x + 340 <= WORK.right(), "{x}");
        assert_eq!(y, 100);
        // Too low: its bottom stops at the work area's bottom.
        let low = Px::new(200, 900, 240, 208);
        assert_eq!(place_beside(low, WORK, (340, 460), 8), (448, 1040 - 460));
        // A note above the work area is pulled down to it.
        let high = Px::new(200, -50, 240, 208);
        assert_eq!(place_beside(high, WORK, (340, 460), 8).1, 0);
        // A work area on a second monitor: the origin is not zero.
        let second = Px::new(1920, 0, 1920, 1040);
        let note = Px::new(3500, 100, 240, 208);
        let (x, _) = place_beside(note, second, (340, 460), 8);
        assert_eq!(x, 3500 - 8 - 340);
        let note = Px::new(1900, 100, 240, 208);
        let (x, _) = place_beside(note, second, (340, 460), 8);
        assert!(x >= 1920 && x + 340 <= 3840, "{x}");
    }

    #[test]
    fn the_list_is_always_inside_the_work_area() {
        for x in [-500, 0, 700, 1700, 3000] {
            for y in [-500, 0, 500, 1000, 3000] {
                for w in [240, 1200] {
                    let (px, py) = place_beside(Px::new(x, y, w, 208), WORK, (340, 460), 8);
                    assert!(px >= 0 && py >= 0 && px + 340 <= 1920 && py + 460 <= 1040, "{x},{y},{w}: {px},{py}");
                }
            }
        }
    }

    #[test]
    fn the_default_place_is_the_top_right_of_the_work_area() {
        assert_eq!(place_default(WORK, (340, 460), 24), (1920 - 340 - 24, 24));
        let second = Px::new(1920, 0, 2560, 1400);
        assert_eq!(place_default(second, (510, 690), 36), (1920 + 2560 - 510 - 36, 36));
        // A work area smaller than the window: the window sticks to the
        // top-left rather than going outside it.
        let tiny = Px::new(100, 50, 200, 300);
        assert_eq!(place_default(tiny, (340, 460), 24), (100, 50));
    }

    // ------------------------------------------------------------ selection

    #[test]
    fn selection_starts_at_the_first_going_down_and_the_last_going_up() {
        assert_eq!(move_selection(None, 1, 5), Some(0));
        assert_eq!(move_selection(None, -1, 5), Some(4));
        assert_eq!(move_selection(None, 0, 5), None);
        assert_eq!(move_selection(Some(2), 1, 5), Some(3));
        assert_eq!(move_selection(Some(2), -1, 5), Some(1));
        assert_eq!(move_selection(Some(4), 1, 5), Some(4), "stops at the end");
        assert_eq!(move_selection(Some(0), -1, 5), Some(0), "stops at the start");
        assert_eq!(move_selection(Some(1), 8, 5), Some(4));
        assert_eq!(move_selection(Some(1), -8, 5), Some(0));
        assert_eq!(move_selection(None, 1, 0), None);
        assert_eq!(move_selection(Some(3), 1, 0), None);
        assert_eq!(move_selection(Some(7), -1, 3), Some(2), "a stale index comes back in");
    }

    #[test]
    fn typing_clears_the_selection_and_a_shorter_list_drops_a_stale_one() {
        assert_eq!(selection_after_typing(Some(3)), None);
        assert_eq!(selection_after_typing(None), None);
        assert_eq!(settle_selection(Some(2), 3), Some(2));
        assert_eq!(settle_selection(Some(3), 3), None);
        assert_eq!(settle_selection(None, 3), None);
        assert_eq!(settle_selection(Some(0), 0), None);
    }

    #[test]
    fn enter_opens_the_selected_row_else_the_first() {
        assert_eq!(enter_target(Some(2), 5), Some(2));
        assert_eq!(enter_target(None, 5), Some(0));
        assert_eq!(enter_target(None, 0), None);
        assert_eq!(enter_target(Some(9), 5), Some(0), "a stale selection is not used");
    }

    // ------------------------------------------------------------ show and focus

    #[test]
    fn show_comes_with_the_events_that_make_the_note_visible() {
        let notes = [
            note("tucked", "x", 1, |s| s.tuck = Some(Edge::Right)),
            note("rolled", "x", 1, |s| s.rolled = true),
            note("both", "x", 1, |s| {
                s.tuck = Some(Edge::Left);
                s.rolled = true;
            }),
            note("open", "x", 1, |_| {}),
        ];
        assert_eq!(show_events(&notes, "tucked"), [WidgetEvent::Tuck { id: "tucked".into(), edge: None }]);
        assert_eq!(show_events(&notes, "rolled"), [WidgetEvent::Rolled { id: "rolled".into(), on: false }]);
        assert_eq!(
            show_events(&notes, "both"),
            [
                WidgetEvent::Tuck { id: "both".into(), edge: None },
                WidgetEvent::Rolled { id: "both".into(), on: false },
            ]
        );
        assert!(show_events(&notes, "open").is_empty(), "already so");
        assert!(show_events(&notes, "missing").is_empty());
    }

    #[test]
    fn a_hidden_sheet_of_a_stack_is_shown_and_the_shown_one_is_left_alone() {
        let notes = [sheet("a", 0, "stk-x", 1), sheet("b", 1, "stk-x", 2), sheet("c", 2, "stk-x", 3)];
        assert!(show_events(&notes, "a").is_empty(), "the shown sheet");
        assert_eq!(show_events(&notes, "c"), [WidgetEvent::StackShow { id: "c".into() }]);
        // A lone stack id is a plain note: nothing to flip.
        let lone = [sheet("a", 1, "stk-gone", 1)];
        assert!(show_events(&lone, "a").is_empty());
        // Tucked and rolled stack, hidden sheet: all three.
        let mut tucked = notes.clone();
        for n in &mut tucked {
            let s = n.sticky.as_mut().unwrap();
            s.tuck = Some(Edge::Right);
            s.rolled = true;
        }
        assert_eq!(show_events(&tucked, "b").len(), 3);
    }

    #[test]
    fn show_events_applied_make_the_note_visible() {
        let dir = tmp("show");
        let mut store = NotesStore::open(dir.join("notes.json"));
        let id = store.create_sticky(Sticky::default(), 1).unwrap();
        store.set_sticky(&id, Some(Sticky { tuck: Some(Edge::Right), rolled: true, ..Sticky::default() }));
        let events = show_events(store.list(), &id);
        let out = apply::apply(&mut store, events, 5);
        assert!(out.changed);
        let s = store.get(&id).unwrap().sticky.clone().unwrap();
        assert_eq!((s.tuck, s.rolled), (None, false));
        assert!(show_events(store.list(), &id).is_empty());
    }

    // ------------------------------------------------------------ undo

    fn tmp(name: &str) -> PathBuf {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "vale-list-{name}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A store with a stack of three sheets, a plain note and a plain one off
    /// the desktop. Returns the ids of the sheets in order.
    fn stack_store(name: &str) -> (NotesStore, PathBuf, [String; 3]) {
        let dir = tmp(name);
        let mut store = NotesStore::open(dir.join("notes.json"));
        let mut ids = Vec::new();
        for k in 0..3u32 {
            let id = store
                .create_sticky(
                    Sticky {
                        rect: DipRect::new(40.0, 50.0, 280.0, 200.0),
                        monitor: r"\\?\DISPLAY#X".into(),
                        stack: Some("stk-s".into()),
                        stack_order: k,
                        stack_top: k == 1,
                        ..Sticky::default()
                    },
                    10 + i64::from(k),
                )
                .unwrap();
            store.set_text(&id, &format!("sheet {k}"), 10 + i64::from(k));
            ids.push(id);
        }
        let solo = store.create_sticky(Sticky::default(), 5).unwrap();
        store.set_text(&solo, "solo", 5);
        store.upsert(None, "off the desktop", 4);
        let ids: [String; 3] = ids.try_into().unwrap();
        (store, dir, ids)
    }

    #[test]
    fn capture_keeps_the_note_and_the_sticky_parts_of_its_stack_mates() {
        let (store, _dir, ids) = stack_store("capture");
        let u = Undo::capture(store.list(), &ids[1]).unwrap();
        assert_eq!(&u.note, store.get(&ids[1]).unwrap());
        let mate_ids: Vec<&str> = u.mates.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(mate_ids, [ids[0].as_str(), ids[2].as_str()], "in stack order, without the note");
        assert_eq!(u.mates[0].1, store.get(&ids[0]).unwrap().sticky.clone().unwrap());
        // A plain note and a missing one.
        let solo = store.list().iter().find(|n| n.text == "solo").unwrap().id.clone();
        assert!(Undo::capture(store.list(), &solo).unwrap().mates.is_empty());
        assert_eq!(Undo::capture(store.list(), "nope"), None);
    }

    #[test]
    fn a_deleted_plain_note_comes_back_exactly_as_it_was() {
        let (mut store, dir, _ids) = stack_store("plain");
        let solo = store.list().iter().find(|n| n.text == "solo").unwrap().id.clone();
        store.save().unwrap();
        let before = std::fs::read_to_string(dir.join("notes.json")).unwrap();
        let undo = Undo::capture(store.list(), &solo).unwrap();
        apply::apply(&mut store, vec![WidgetEvent::Delete { id: solo.clone() }], 99);
        assert!(store.get(&solo).is_none());
        assert!(undo.restore(&mut store));
        apply::settle(&mut store, 99, false);
        store.save().unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("notes.json")).unwrap(), before, "the same keys, the same file");
    }

    #[test]
    fn a_deleted_sheet_of_a_three_sheet_stack_rejoins_it_with_the_orders_it_had() {
        for victim in 0..3 {
            let (mut store, dir, ids) = stack_store(&format!("stack{victim}"));
            store.save().unwrap();
            let before = std::fs::read_to_string(dir.join("notes.json")).unwrap();
            let undo = Undo::capture(store.list(), &ids[victim]).unwrap();
            let out = apply::apply(&mut store, vec![WidgetEvent::Delete { id: ids[victim].clone() }], 99);
            assert!(out.structural);
            // The rest closed up: two sheets, orders 0 and 1.
            let left: Vec<u32> = ids
                .iter()
                .filter_map(|i| store.get(i))
                .map(|n| n.sticky.as_ref().unwrap().stack_order)
                .collect();
            assert_eq!(left, [0, 1], "victim {victim}");
            // Moving the stack meanwhile: the restored sheet follows it.
            let mate = ids.iter().find(|i| **i != ids[victim]).unwrap().clone();
            apply::apply(
                &mut store,
                vec![
                    WidgetEvent::Placed {
                        id: mate.clone(),
                        monitor: r"\?\DISPLAY#X".into(),
                        rect: DipRect::new(300.0, 120.0, 300.0, 210.0),
                    },
                    WidgetEvent::Pinned { id: mate.clone(), on: true },
                ],
                99,
            );
            assert!(undo.restore(&mut store), "victim {victim}");
            apply::settle(&mut store, 99, false);
            // One stack of three again, orders 0..3, one shown.
            let us = stack::units(store.list());
            let u = us.iter().find(|u| u.is_stack()).expect("a stack");
            assert_eq!(u.sheets.len(), 3);
            let order: Vec<&str> = u.sheets.iter().map(|n| n.id.as_str()).collect();
            assert_eq!(order, [ids[0].as_str(), ids[1].as_str(), ids[2].as_str()], "victim {victim}");
            assert_eq!(u.sheets.iter().filter(|n| n.sticky.as_ref().unwrap().stack_top).count(), 1);
            // Same window: every sheet shares the stack's current place and pin.
            for n in &u.sheets {
                let s = n.sticky.as_ref().unwrap();
                assert_eq!(s.rect, DipRect::new(300.0, 120.0, 300.0, 210.0), "victim {victim}");
                assert!(s.pinned);
            }
            // The text and time came back.
            let back = store.get(&ids[victim]).unwrap();
            assert_eq!(back.text, format!("sheet {victim}"));
            assert_eq!(back.updated, 10 + victim as i64);
            let _ = before;
        }
    }

    #[test]
    fn restoring_a_stack_sheet_without_changes_in_between_writes_the_same_file() {
        let (mut store, dir, ids) = stack_store("same-file");
        store.save().unwrap();
        let before = std::fs::read_to_string(dir.join("notes.json")).unwrap();
        for victim in 0..3 {
            let undo = Undo::capture(store.list(), &ids[victim]).unwrap();
            apply::apply(&mut store, vec![WidgetEvent::Delete { id: ids[victim].clone() }], 99);
            assert!(undo.restore(&mut store));
            apply::settle(&mut store, 99, false);
            store.save().unwrap();
            assert_eq!(
                std::fs::read_to_string(dir.join("notes.json")).unwrap(),
                before,
                "victim {victim}: delete + undo is the identity"
            );
        }
    }

    #[test]
    fn undo_touches_only_the_stack_keys_of_a_mate() {
        let (mut store, _dir, ids) = stack_store("mate-keys");
        let undo = Undo::capture(store.list(), &ids[0]).unwrap();
        apply::apply(&mut store, vec![WidgetEvent::Delete { id: ids[0].clone() }], 99);
        // A mate gets a new paper while the strip is up: it stays.
        let mut m = store.get(&ids[1]).unwrap().sticky.clone().unwrap();
        m.paper = Paper::Plum;
        m.stamp = crate::widgets::sticky::Stamp::Ide;
        store.set_sticky(&ids[1], Some(m));
        assert!(undo.restore(&mut store));
        let s = store.get(&ids[1]).unwrap().sticky.clone().unwrap();
        assert_eq!((s.paper, s.stamp), (Paper::Plum, crate::widgets::sticky::Stamp::Ide));
        assert_eq!((s.stack.as_deref(), s.stack_order), (Some("stk-s"), 1));
    }

    #[test]
    fn a_mate_that_is_gone_is_skipped_and_a_lone_stack_id_is_settled_away() {
        let (mut store, _dir, ids) = stack_store("mate-gone");
        let undo = Undo::capture(store.list(), &ids[0]).unwrap();
        apply::apply(&mut store, vec![WidgetEvent::Delete { id: ids[0].clone() }], 99);
        apply::apply(&mut store, vec![WidgetEvent::Delete { id: ids[1].clone() }], 99);
        // One mate left alone; the other was deleted meanwhile.
        assert!(undo.restore(&mut store));
        apply::settle(&mut store, 99, false);
        assert!(store.get(&ids[0]).is_some() && store.get(&ids[1]).is_none());
        assert!(store.get(&ids[2]).is_some());
        // The two live sheets are a stack of two again (both carry the id).
        let us = stack::units(store.list());
        assert_eq!(us.iter().filter(|u| u.is_stack()).count(), 1);
        // And when every mate is gone the restored sheet is a plain note.
        let (mut store, _dir, ids) = stack_store("alone");
        let undo = Undo::capture(store.list(), &ids[0]).unwrap();
        for id in &ids {
            apply::apply(&mut store, vec![WidgetEvent::Delete { id: id.clone() }], 99);
        }
        assert!(undo.restore(&mut store));
        apply::settle(&mut store, 99, false);
        let s = store.get(&ids[0]).unwrap().sticky.clone().unwrap();
        assert_eq!((s.stack, s.stack_order, s.stack_top), (None, 0, false));
    }

    #[test]
    fn undo_is_refused_for_an_existing_id_and_on_a_full_desktop() {
        let (mut store, _dir, ids) = stack_store("refused");
        let undo = Undo::capture(store.list(), &ids[0]).unwrap();
        // The id is still there: nothing to restore.
        assert!(!undo.clone().restore(&mut store));
        let count = store.list().len();
        apply::apply(&mut store, vec![WidgetEvent::Delete { id: ids[0].clone() }], 99);
        // The desktop fills up meanwhile.
        while store.sticky_count() < MAX_STICKY {
            store.create_sticky(Sticky::default(), 200).unwrap();
        }
        assert!(!undo.clone().restore(&mut store), "full");
        assert!(store.get(&ids[0]).is_none());
        // Mates are untouched by a refused restore.
        assert_eq!(store.get(&ids[1]).unwrap().sticky.as_ref().unwrap().stack_order, 0);
        let _ = count;
    }
}
