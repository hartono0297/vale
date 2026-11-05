//! Stacks of sticky notes ("Tumpuk catatan"): several notes shown as one block
//! of paper, one window, one sheet on top. Every sheet stays an ordinary note
//! in notes.json and on the Notes page; the stack is only three keys on each
//! sheet's sticky part (`stack`, `stack_order`, `stack_top`).
//!
//! Pure, tested without a window. The functions read the notes and answer the
//! new sticky part of each sheet that changes (`Edits`); `apply` writes them to
//! the store. A *unit* is what has a window: a plain note, or a whole stack.

use crate::notes::Note;
use crate::widgets::paint::shown_h;
use crate::widgets::remind;
use crate::widgets::snap;
use crate::widgets::sticky::{DipRect, Sticky};

/// A stack is at least this wide (dip): the counter and both arrows have to
/// fit left of the seven header buttons.
pub const MIN_W: f32 = 260.0;
/// A stack made from a plain note takes this prefix plus that note's id.
pub const ID_PREFIX: &str = "stk-";
/// One wheel notch.
pub const WHEEL_STEP: i32 = 120;

/// The new sticky part of each sheet that changes, by note id.
pub type Edits = Vec<(String, Sticky)>;

/// A plain note, or a stack with its sheets in order.
#[derive(Debug)]
pub struct Unit<'a> {
    /// The stack id; `None` for a plain note.
    pub stack: Option<&'a str>,
    /// The sheets, by `stack_order` and then id (one for a plain note).
    pub sheets: Vec<&'a Note>,
}

fn sticky_of(n: &Note) -> &Sticky {
    n.sticky.as_ref().expect("a sheet has its sticky part")
}

impl Unit<'_> {
    /// The window key: the stack id, else the note's id.
    pub fn key(&self) -> &str {
        self.stack.unwrap_or_else(|| self.sheets[0].id.as_str())
    }

    pub fn is_stack(&self) -> bool {
        self.stack.is_some()
    }

    pub fn ids(&self) -> Vec<String> {
        self.sheets.iter().map(|n| n.id.clone()).collect()
    }

    pub fn has(&self, id: &str) -> bool {
        self.sheets.iter().any(|n| n.id == id)
    }

    pub fn position(&self, id: &str) -> Option<usize> {
        self.sheets.iter().position(|n| n.id == id)
    }

    /// The sheet the stack shows as saved: the first flagged one, else the
    /// first in order.
    pub fn top(&self) -> usize {
        self.sheets
            .iter()
            .position(|n| sticky_of(n).stack_top)
            .unwrap_or(0)
    }
}

/// The units of `notes`: every sticky note is one, or a sheet of one. A stack
/// id that one live sheet carries alone is a plain note. Units come in the
/// order their first sheet appears in `notes`.
pub fn units<'a>(notes: impl IntoIterator<Item = &'a Note>) -> Vec<Unit<'a>> {
    let sticky: Vec<&Note> = notes.into_iter().filter(|n| n.sticky.is_some()).collect();
    fn carried(n: &Note) -> Option<&str> {
        sticky_of(n).stack.as_deref()
    }
    let mut out: Vec<Unit> = Vec::new();
    for &n in &sticky {
        let shared = carried(n)
            .filter(|id| sticky.iter().filter(|m| carried(m) == Some(*id)).count() >= 2);
        match shared {
            Some(id) => match out.iter_mut().find(|u| u.stack == Some(id)) {
                Some(u) => u.sheets.push(n),
                None => out.push(Unit { stack: Some(id), sheets: vec![n] }),
            },
            None => out.push(Unit { stack: None, sheets: vec![n] }),
        }
    }
    for u in out.iter_mut().filter(|u| u.is_stack()) {
        u.sheets
            .sort_by(|a, b| (sticky_of(a).stack_order, &a.id).cmp(&(sticky_of(b).stack_order, &b.id)));
    }
    out
}

/// The unit holding sheet `id`.
pub fn unit_of<'a>(units: &'a [Unit<'a>], id: &str) -> Option<&'a Unit<'a>> {
    units.iter().find(|u| u.has(id))
}

/// The sheet a unit shows at `now`: the one whose reminder is overdue (the
/// earliest if several) comes to the top, else the saved top.
pub fn shown(unit: &Unit, now: i64) -> usize {
    unit.sheets
        .iter()
        .enumerate()
        .filter(|(_, n)| remind::overdue(sticky_of(n), now))
        .min_by_key(|(_, n)| sticky_of(n).remind_at.unwrap_or(i64::MAX))
        .map_or_else(|| unit.top(), |(i, _)| i)
}

fn push_if_changed(out: &mut Edits, note: &Note, new: Sticky) {
    if sticky_of(note) != &new {
        out.push((note.id.clone(), new));
    }
}

/// What makes the data right: a stack id that one sheet carries alone is
/// cleared (so are stray order and top flags on a plain note), exactly one
/// sheet of a stack is shown (the first flagged one, else the first; with
/// `overdue_top` an overdue sheet comes to the top, see `shown`), and the
/// orders become 0..n-1. The stack's saved top moves with it.
pub fn normalise(notes: &[Note], now: i64, overdue_top: bool) -> Edits {
    let mut out = Edits::new();
    for u in units(notes) {
        if !u.is_stack() {
            let n = u.sheets[0];
            let mut s = sticky_of(n).clone();
            s.stack = None;
            s.stack_order = 0;
            s.stack_top = false;
            push_if_changed(&mut out, n, s);
            continue;
        }
        let top = if overdue_top { shown(&u, now) } else { u.top() };
        for (k, n) in u.sheets.iter().enumerate() {
            let mut s = sticky_of(n).clone();
            s.stack_order = k as u32;
            s.stack_top = k == top;
            push_if_changed(&mut out, n, s);
        }
    }
    out
}

/// What every sheet of a stack shares with the one that was moved, resized,
/// pinned, rolled up or tucked.
pub fn share(from: &Sticky, to: &mut Sticky) {
    to.rect = from.rect;
    to.monitor.clone_from(&from.monitor);
    to.pinned = from.pinned;
    to.rolled = from.rolled;
    to.tuck = from.tuck;
}

/// Sheet `id`'s shared fields written to every other sheet of its stack, so
/// dragging the stack moves all of it.
pub fn propagate(notes: &[Note], id: &str) -> Edits {
    let mut out = Edits::new();
    let us = units(notes);
    let Some(u) = unit_of(&us, id).filter(|u| u.is_stack()) else {
        return out;
    };
    let Some(src) = u.sheets.iter().find(|n| n.id == id) else {
        return out;
    };
    for n in u.sheets.iter().filter(|n| n.id != id) {
        let mut s = sticky_of(n).clone();
        share(sticky_of(src), &mut s);
        push_if_changed(&mut out, n, s);
    }
    out
}

/// A stack id for a new stack made on note `base`: `stk-` and its id, with a
/// number added while a sticky note still carries that id (the sheets left
/// behind by a take-out or an unstick keep the old stack's id, and a new stack
/// with the same id would swallow them).
fn fresh_id(notes: &[Note], base: &str) -> String {
    let used = |id: &str| {
        notes
            .iter()
            .filter_map(|n| n.sticky.as_ref())
            .any(|s| s.stack.as_deref() == Some(id))
    };
    let first = format!("{ID_PREFIX}{base}");
    let mut id = first.clone();
    let mut k = 2;
    while used(&id) {
        id = format!("{first}-{k}");
        k += 1;
    }
    id
}

/// The dragged unit (the unit holding `dragged[0]`) dropped on the target unit
/// (holding `target[0]`): one stack. Its id is the target's when it has one,
/// else `stk-` and the target's note id (numbered when that id is taken). The dragged sheets go first, in their
/// own order, then the target's; the sheet shown is the dragged unit's. Every
/// sheet takes the target's rect, monitor, pin, roll and tuck, and the width is
/// at least `MIN_W`. Nothing when either is missing or they are one unit.
pub fn join(notes: &[Note], dragged: &[String], target: &[String]) -> Edits {
    let us = units(notes);
    let find = |ids: &[String]| ids.first().and_then(|id| unit_of(&us, id));
    let (Some(d), Some(t)) = (find(dragged), find(target)) else {
        return Edits::new();
    };
    if d.key() == t.key() {
        return Edits::new();
    }
    let id = t
        .stack
        .map_or_else(|| fresh_id(notes, &t.sheets[0].id), str::to_owned);
    let base = sticky_of(t.sheets[t.top()]).clone();
    let mut rect = base.rect;
    rect.w = rect.w.max(MIN_W);
    let top = d.top();
    let mut out = Edits::new();
    for (k, n) in d.sheets.iter().chain(t.sheets.iter()).enumerate() {
        let mut s = sticky_of(n).clone();
        s.stack = Some(id.clone());
        s.stack_order = k as u32;
        s.stack_top = k == top;
        share(&base, &mut s);
        s.rect = rect;
        push_if_changed(&mut out, n, s);
    }
    out
}

/// Where the shown sheet goes when flipped by `delta`: one step either way,
/// wrapping round a stack of `len`.
pub fn flip_pos(pos: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    (pos as i64 + i64::from(delta)).rem_euclid(len as i64) as usize
}

/// The sheet of `unit` that shows after flipping by `delta`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn flip(unit: &Unit, delta: i32) -> usize {
    flip_pos(unit.top(), unit.sheets.len(), delta)
}

/// Sheet `id` becomes the one its stack shows (the other flags clear).
pub fn show(notes: &[Note], id: &str) -> Edits {
    let mut out = Edits::new();
    let us = units(notes);
    let Some(u) = unit_of(&us, id).filter(|u| u.is_stack()) else {
        return out;
    };
    for n in &u.sheets {
        let mut s = sticky_of(n).clone();
        s.stack_top = n.id == id;
        push_if_changed(&mut out, n, s);
    }
    out
}

/// The sheets left in `u` once `gone` leaves: orders renumbered, and the next
/// sheet in order becomes the shown one when the one that left was (a sheet
/// that was not shown keeps the top). A stack left with one sheet is a plain
/// note.
fn remaining(u: &Unit, gone: &str) -> Edits {
    let mut out = Edits::new();
    let Some(at) = u.position(gone) else {
        return out;
    };
    let left: Vec<&Note> = u.sheets.iter().copied().filter(|n| n.id != gone).collect();
    let single = left.len() < 2;
    let was_top = u.top() == at;
    // The next one in order: the sheet that took its place, else the first.
    let new_top = if was_top {
        Some(if at < left.len() { at } else { 0 })
    } else {
        left.iter().position(|n| sticky_of(n).stack_top)
    };
    for (k, n) in left.iter().enumerate() {
        let mut s = sticky_of(n).clone();
        if single {
            s.stack = None;
            s.stack_order = 0;
            s.stack_top = false;
        } else {
            s.stack_order = k as u32;
            s.stack_top = Some(k) == new_top;
        }
        push_if_changed(&mut out, n, s);
    }
    out
}

/// What the rest of a stack becomes when sheet `id` is deleted or unstuck (a
/// plain note answers nothing). Applied before the sheet goes.
pub fn leave(notes: &[Note], id: &str) -> Edits {
    let us = units(notes);
    match unit_of(&us, id).filter(|u| u.is_stack()) {
        Some(u) => remaining(u, id),
        None => Edits::new(),
    }
}

/// Sheet `id` taken out of its stack: it is a note of its own, placed beside
/// the stack (right, else left, else below: the (+) rule) on `area` (dip, at
/// the origin; its size is kept), out in the open. The rest renumber and the
/// next sheet in order shows if it was the shown one. Nothing for a note that
/// is not in a stack.
pub fn take_out(notes: &[Note], id: &str, area: &DipRect) -> Edits {
    let us = units(notes);
    let Some(u) = unit_of(&us, id).filter(|u| u.is_stack()) else {
        return Edits::new();
    };
    let Some(n) = u.sheets.iter().find(|n| n.id == id) else {
        return Edits::new();
    };
    let mut out = remaining(u, id);
    let shared = sticky_of(u.sheets[u.top()]);
    let source = DipRect {
        h: shown_h(shared.rect.h, shared.rolled),
        ..shared.rect
    };
    let mut s = sticky_of(n).clone();
    // The size it had in the stack (a rolled-up sheet keeps its unrolled
    // height in `rect.h`), not the default one.
    s.rect = snap::place_beside_sized(&source, area, s.rect.w, s.rect.h);
    s.stack = None;
    s.stack_order = 0;
    s.stack_top = false;
    s.tuck = None;
    push_if_changed(&mut out, n, s);
    out
}

/// "1 / 3": the shown sheet's place (0-based `pos`) and the stack's size.
pub fn counter_label(pos: usize, len: usize) -> String {
    format!("{} / {}", pos + 1, len)
}

/// The wheel over a stack's strip: `acc` is what was left of earlier notches,
/// `delta` what this message carries (+120 a notch up). Returns how far to flip
/// (down is next, so a notch down is +1, a notch up is -1) and the new
/// remainder. A notch is `WHEEL_STEP`; a free-spinning wheel's small deltas add
/// up.
pub fn wheel_flips(acc: i32, delta: i32) -> (i32, i32) {
    let total = acc + delta;
    let notches = total / WHEEL_STEP;
    (-notches, total - notches * WHEEL_STEP)
}

/// A window the host already has: the unit it shows (its key), the sheet it
/// shows, and every sheet it knows.
#[derive(Debug)]
pub struct Existing<'a> {
    pub key: &'a str,
    pub shown: &'a str,
    pub ids: Vec<&'a str>,
}

/// Which existing window goes with each unit (`None` for a new one). A unit
/// first takes the window with its own key (the stack id, else the note id).
/// One that has none takes a window that is not taken and holds one of its
/// sheets (a note that joined a stack, a stack that came apart), preferring the
/// one showing the sheet the unit shows. Windows left over are not used.
pub fn match_units(existing: &[Existing], units: &[Unit]) -> Vec<Option<usize>> {
    let mut taken = vec![false; existing.len()];
    let mut out: Vec<Option<usize>> = vec![None; units.len()];
    for (u, unit) in units.iter().enumerate() {
        if let Some(e) = (0..existing.len()).find(|e| !taken[*e] && existing[*e].key == unit.key()) {
            taken[e] = true;
            out[u] = Some(e);
        }
    }
    // Then a window that is not taken and holds one of the unit's sheets: the
    // one showing the sheet the unit shows first, any holder after.
    for exact in [true, false] {
        for (u, unit) in units.iter().enumerate() {
            if out[u].is_some() {
                continue;
            }
            let top = unit.sheets[unit.top()].id.as_str();
            let pick = (0..existing.len()).find(|e| {
                !taken[*e]
                    && existing[*e].ids.iter().any(|id| unit.has(id))
                    && (!exact || existing[*e].shown == top)
            });
            if let Some(e) = pick {
                taken[e] = true;
                out[u] = Some(e);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::NotesStore;
    use crate::widgets::sticky::{self, Paper};

    const AREA: DipRect = DipRect::new(0.0, 0.0, 1280.0, 720.0);

    fn sheet(id: &str, stack: Option<&str>, order: u32, top: bool) -> Note {
        Note {
            id: id.to_owned(),
            text: format!("Note {id}"),
            updated: 1,
            sticky: Some(Sticky {
                stack: stack.map(str::to_owned),
                stack_order: order,
                stack_top: top,
                rect: DipRect::new(100.0 + order as f32, 100.0, 240.0, 208.0),
                ..Sticky::default()
            }),
        }
    }

    fn plain(id: &str, x: f32) -> Note {
        let mut n = sheet(id, None, 0, false);
        n.sticky.as_mut().unwrap().rect = DipRect::new(x, 300.0, 240.0, 208.0);
        n
    }

    fn apply_edits(notes: &mut [Note], edits: Edits) {
        for (id, s) in edits {
            notes.iter_mut().find(|n| n.id == id).unwrap().sticky = Some(s);
        }
    }

    fn ids(u: &Unit) -> Vec<String> {
        u.ids()
    }

    #[test]
    fn units_group_sheets_in_order_and_keep_plain_notes_alone() {
        let notes = vec![
            sheet("c", Some("s1"), 2, false),
            plain("p", 600.0),
            sheet("a", Some("s1"), 0, true),
            sheet("b", Some("s1"), 1, false),
            Note { id: "off".into(), text: "t".into(), updated: 1, sticky: None },
        ];
        let us = units(&notes);
        assert_eq!(us.len(), 2, "the stack and the plain note; the unstuck note is none");
        assert_eq!(us[0].stack, Some("s1"));
        assert_eq!(ids(&us[0]), ["a", "b", "c"], "by order");
        assert_eq!(us[0].top(), 0);
        assert_eq!(us[0].key(), "s1");
        assert_eq!((us[1].stack, ids(&us[1])), (None, vec!["p".to_string()]));
        assert_eq!(us[1].key(), "p");
    }

    #[test]
    fn a_tie_in_order_goes_by_id() {
        let notes = vec![
            sheet("z", Some("s"), 0, false),
            sheet("m", Some("s"), 0, false),
            sheet("a", Some("s"), 0, false),
        ];
        assert_eq!(ids(&units(&notes)[0]), ["a", "m", "z"]);
    }

    #[test]
    fn a_stack_id_one_sheet_carries_alone_is_a_plain_note() {
        let notes = vec![sheet("a", Some("s1"), 3, true), plain("p", 600.0)];
        let us = units(&notes);
        assert_eq!(us.len(), 2);
        assert!(us.iter().all(|u| !u.is_stack()));
        // A sheet whose partner is not on the desktop (no sticky part) is alone.
        let mut notes = vec![sheet("a", Some("s1"), 0, true), sheet("b", Some("s1"), 1, false)];
        notes[1].sticky = None;
        assert!(!units(&notes)[0].is_stack());
        // Normalising clears its stack fields.
        let edits = normalise(&notes, 0, false);
        assert_eq!(edits.len(), 1);
        let s = &edits[0].1;
        assert_eq!((s.stack.clone(), s.stack_order, s.stack_top), (None, 0, false));
    }

    #[test]
    fn normalise_shows_exactly_one_sheet_and_numbers_the_orders() {
        // No flag at all: the first in order shows. Two flags: the first flagged.
        let mut notes = vec![
            sheet("a", Some("s"), 5, false),
            sheet("b", Some("s"), 9, true),
            sheet("c", Some("s"), 20, true),
        ];
        let edits = normalise(&notes, 0, false);
        apply_edits(&mut notes, edits);
        let us = units(&notes);
        let tops: Vec<bool> = us[0].sheets.iter().map(|n| n.sticky.as_ref().unwrap().stack_top).collect();
        assert_eq!(tops, [false, true, false], "b was the first flagged");
        let orders: Vec<u32> = us[0].sheets.iter().map(|n| n.sticky.as_ref().unwrap().stack_order).collect();
        assert_eq!(orders, [0, 1, 2]);
        assert!(normalise(&notes, 0, false).is_empty(), "settled: nothing to change");
        let mut none = vec![sheet("a", Some("s"), 1, false), sheet("b", Some("s"), 2, false)];
        let edits = normalise(&none, 0, false);
        apply_edits(&mut none, edits);
        assert!(none[0].sticky.as_ref().unwrap().stack_top);
        assert!(!none[1].sticky.as_ref().unwrap().stack_top);
        // A plain note with stray fields is cleaned; a clean one is left alone.
        let mut stray = vec![plain("p", 0.0)];
        stray[0].sticky.as_mut().unwrap().stack_top = true;
        stray[0].sticky.as_mut().unwrap().stack_order = 4;
        assert_eq!(normalise(&stray, 0, false).len(), 1);
        assert!(normalise(&[plain("q", 0.0)], 0, false).is_empty());
    }

    #[test]
    fn an_overdue_sheet_comes_to_the_top_the_earliest_one_first() {
        let mut notes = vec![
            sheet("a", Some("s"), 0, true),
            sheet("b", Some("s"), 1, false),
            sheet("c", Some("s"), 2, false),
        ];
        let now = 10_000;
        let overdue = |n: &mut Note, at: i64| {
            let s = n.sticky.as_mut().unwrap();
            s.remind_at = Some(at);
            s.remind_fired = true;
        };
        // Nothing overdue: the saved top.
        assert_eq!(shown(&units(&notes)[0], now), 0);
        overdue(&mut notes[2], 9_000);
        assert_eq!(shown(&units(&notes)[0], now), 2);
        // Two: the earliest.
        overdue(&mut notes[1], 5_000);
        assert_eq!(shown(&units(&notes)[0], now), 1);
        // A reminder not fired, or in the future, does not count.
        notes[1].sticky.as_mut().unwrap().remind_fired = false;
        assert_eq!(shown(&units(&notes)[0], now), 2);
        notes[2].sticky.as_mut().unwrap().remind_at = Some(now + 5);
        assert_eq!(shown(&units(&notes)[0], now), 0);
        // Persisted by moving stack_top, only when asked.
        overdue(&mut notes[2], 9_000);
        assert!(normalise(&notes, now, false).iter().all(|(_, s)| s.stack_top == (s.stack_order == 0)));
        let edits = normalise(&notes, now, true);
        apply_edits(&mut notes, edits);
        let tops: Vec<bool> = notes.iter().map(|n| n.sticky.as_ref().unwrap().stack_top).collect();
        assert_eq!(tops, [false, false, true]);
    }

    #[test]
    fn joining_takes_the_targets_stack_rect_and_state_and_the_dragged_sheet_shows() {
        let mut target = plain("t", 500.0);
        {
            let s = target.sticky.as_mut().unwrap();
            s.rect = DipRect::new(500.0, 300.0, 200.0, 180.0);
            s.monitor = "B".into();
            s.pinned = true;
            s.rolled = true;
            s.tuck = None;
            s.paper = Paper::Sage;
        }
        let mut dragged = plain("d", 50.0);
        dragged.sticky.as_mut().unwrap().paper = Paper::Plum;
        let mut notes = vec![dragged, target];
        let edits = join(&notes, &["d".into()], &["t".into()]);
        assert_eq!(edits.len(), 2);
        apply_edits(&mut notes, edits);
        let us = units(&notes);
        assert_eq!(us.len(), 1);
        let u = &us[0];
        assert_eq!(u.stack, Some("stk-t"), "stk- and the target's note id");
        assert_eq!(ids(u), ["d", "t"], "the dragged sheet first");
        assert_eq!(u.top(), 0, "the dragged unit's shown sheet shows");
        for n in &u.sheets {
            let s = n.sticky.as_ref().unwrap();
            assert_eq!(s.rect, DipRect::new(500.0, 300.0, 260.0, 180.0), "the target's, at least MIN_W wide");
            assert_eq!((s.monitor.as_str(), s.pinned, s.rolled, s.tuck), ("B", true, true, None));
        }
        // Their own looks stay.
        assert_eq!(u.sheets[0].sticky.as_ref().unwrap().paper, Paper::Plum);
        assert_eq!(u.sheets[1].sticky.as_ref().unwrap().paper, Paper::Sage);
    }

    #[test]
    fn a_stack_made_on_a_note_taken_out_of_one_gets_a_fresh_id() {
        let mut notes = vec![plain("a", 50.0), plain("x", 500.0), plain("b", 900.0), plain("c", 1300.0)];
        let e = join(&notes, &["a".into()], &["x".into()]);
        apply_edits(&mut notes, e);
        let e = join(&notes, &["b".into()], &["x".into()]);
        apply_edits(&mut notes, e);
        let e = take_out(&notes, "x", &AREA);
        apply_edits(&mut notes, e);
        let e = join(&notes, &["c".into()], &["x".into()]);
        apply_edits(&mut notes, e);
        let us = units(&notes);
        assert_eq!(us.len(), 2, "the old stack and the new one stay apart");
        let old = us.iter().find(|u| u.has("a")).unwrap();
        let new = us.iter().find(|u| u.has("c")).unwrap();
        assert_eq!(ids(old).len(), 2);
        assert!(old.has("b") && !old.has("x"));
        assert_eq!(ids(new), ["c", "x"]);
        assert_ne!(old.key(), new.key());
        assert!(normalise(&notes, 0, false).is_empty(), "both stacks are consistent");
    }

    #[test]
    fn joining_a_stack_to_a_stack_uses_the_targets_id_and_keeps_both_orders() {
        let mut notes = vec![
            sheet("d1", Some("sd"), 0, false),
            sheet("d2", Some("sd"), 1, true),
            sheet("t1", Some("st"), 0, false),
            sheet("t2", Some("st"), 1, true),
        ];
        notes[2].sticky.as_mut().unwrap().rect = DipRect::new(600.0, 40.0, 300.0, 200.0);
        notes[3].sticky.as_mut().unwrap().rect = DipRect::new(600.0, 40.0, 300.0, 200.0);
        let edits = join(&notes, &["d1".into(), "d2".into()], &["t1".into(), "t2".into()]);
        apply_edits(&mut notes, edits);
        let us = units(&notes);
        assert_eq!(us.len(), 1);
        assert_eq!(us[0].stack, Some("st"), "the target's own stack id");
        assert_eq!(ids(&us[0]), ["d1", "d2", "t1", "t2"]);
        assert_eq!(us[0].top(), 1, "the dragged stack's shown sheet, d2");
        assert!(us[0].sheets.iter().all(|n| n.sticky.as_ref().unwrap().rect == DipRect::new(600.0, 40.0, 300.0, 200.0)));
    }

    #[test]
    fn joining_something_missing_or_itself_changes_nothing() {
        let notes = vec![plain("a", 0.0), plain("b", 300.0), sheet("c", Some("s"), 0, true), sheet("d", Some("s"), 1, false)];
        assert!(join(&notes, &["a".into()], &["a".into()]).is_empty());
        assert!(join(&notes, &["c".into()], &["d".into()]).is_empty(), "the same stack");
        assert!(join(&notes, &["a".into()], &["nope".into()]).is_empty());
        assert!(join(&notes, &[], &["b".into()]).is_empty());
        assert!(join(&notes, &["x".into()], &["b".into()]).is_empty());
    }

    #[test]
    fn flipping_wraps_both_ways() {
        assert_eq!(flip_pos(0, 3, 1), 1);
        assert_eq!(flip_pos(2, 3, 1), 0, "past the last: the first");
        assert_eq!(flip_pos(0, 3, -1), 2, "before the first: the last");
        assert_eq!(flip_pos(1, 3, -1), 0);
        assert_eq!(flip_pos(1, 3, 7), 2);
        assert_eq!(flip_pos(1, 3, -7), 0);
        assert_eq!(flip_pos(0, 1, 1), 0);
        assert_eq!(flip_pos(0, 0, 1), 0);
        let notes = vec![sheet("a", Some("s"), 0, false), sheet("b", Some("s"), 1, true), sheet("c", Some("s"), 2, false)];
        let us = units(&notes);
        assert_eq!(flip(&us[0], 1), 2);
        assert_eq!(flip(&us[0], -1), 0);
        // `show` moves the flag, and only inside its own stack.
        let mut notes = notes;
        notes.push(sheet("z", Some("other"), 0, true));
        notes.push(sheet("y", Some("other"), 1, false));
        let edits = show(&notes, "c");
        apply_edits(&mut notes, edits);
        let tops: Vec<(&str, bool)> = notes.iter().map(|n| (n.id.as_str(), n.sticky.as_ref().unwrap().stack_top)).collect();
        assert_eq!(tops, [("a", false), ("b", false), ("c", true), ("z", true), ("y", false)]);
        assert!(show(&notes, "c").is_empty(), "already shown");
        assert!(show(&[plain("p", 0.0)], "p").is_empty(), "a plain note has no stack to show");
    }

    #[test]
    fn the_wheel_accumulates_notches_and_down_is_next() {
        assert_eq!(wheel_flips(0, -120), (1, 0), "a notch down: next");
        assert_eq!(wheel_flips(0, 120), (-1, 0), "a notch up: previous");
        // A free-spinning wheel: small deltas add up to a notch.
        let (mut acc, mut total) = (0, 0);
        for _ in 0..4 {
            let (f, a) = wheel_flips(acc, -30);
            total += f;
            acc = a;
        }
        assert_eq!((total, acc), (1, 0));
        let (f, a) = wheel_flips(0, -50);
        assert_eq!((f, a), (0, -50), "not a notch yet");
        assert_eq!(wheel_flips(a, -70), (1, 0));
        // Two notches in one message flip two; a reversal drains the remainder.
        assert_eq!(wheel_flips(0, -240), (2, 0));
        assert_eq!(wheel_flips(-50, 120), (0, 70));
    }

    #[test]
    fn taking_a_sheet_out_places_it_beside_the_stack_and_the_next_one_shows() {
        let mut notes = vec![
            sheet("a", Some("s"), 0, false),
            sheet("b", Some("s"), 1, true),
            sheet("c", Some("s"), 2, false),
        ];
        for n in &mut notes {
            n.sticky.as_mut().unwrap().rect = DipRect::new(100.0, 100.0, 280.0, 208.0);
            n.sticky.as_mut().unwrap().tuck = None;
        }
        // The shown sheet leaves: to the right (12 dip gap, on the grid); the next in order shows.
        let edits = take_out(&notes, "b", &AREA);
        apply_edits(&mut notes, edits);
        let b = notes[1].sticky.as_ref().unwrap();
        assert_eq!((b.stack.clone(), b.stack_order, b.stack_top, b.tuck), (None, 0, false, None));
        assert_eq!((b.rect.x, b.rect.y), (392.0, 104.0), "right of the stack");
        assert_eq!((b.rect.w, b.rect.h), (280.0, 208.0), "it keeps the size it had");
        let us = units(&notes);
        assert_eq!(us.len(), 2);
        let stack = us.iter().find(|u| u.is_stack()).unwrap();
        assert_eq!(ids(stack), ["a", "c"]);
        let orders: Vec<u32> = stack.sheets.iter().map(|n| n.sticky.as_ref().unwrap().stack_order).collect();
        assert_eq!(orders, [0, 1]);
        assert_eq!(stack.sheets[stack.top()].id, "c", "the sheet after it in order");
    }

    #[test]
    fn taking_out_a_sheet_that_is_not_shown_keeps_the_shown_one() {
        let mut notes = vec![
            sheet("a", Some("s"), 0, true),
            sheet("b", Some("s"), 1, false),
            sheet("c", Some("s"), 2, false),
        ];
        let edits = take_out(&notes, "b", &AREA);
        apply_edits(&mut notes, edits);
        let us = units(&notes);
        let stack = us.iter().find(|u| u.is_stack()).unwrap();
        assert_eq!(ids(stack), ["a", "c"]);
        assert_eq!(stack.sheets[stack.top()].id, "a");
        // The last sheet leaving a stack of three: the first shows.
        let mut notes = vec![
            sheet("a", Some("s"), 0, false),
            sheet("b", Some("s"), 1, false),
            sheet("c", Some("s"), 2, true),
        ];
        let edits = take_out(&notes, "c", &AREA);
        apply_edits(&mut notes, edits);
        let us = units(&notes);
        let stack = us.iter().find(|u| u.is_stack()).unwrap();
        assert_eq!(stack.sheets[stack.top()].id, "a", "wraps to the first");
    }

    #[test]
    fn one_sheet_left_means_a_plain_note() {
        let mut notes = vec![sheet("a", Some("s"), 0, true), sheet("b", Some("s"), 1, false)];
        let edits = take_out(&notes, "a", &AREA);
        apply_edits(&mut notes, edits);
        for n in &notes {
            let s = n.sticky.as_ref().unwrap();
            assert_eq!((s.stack.clone(), s.stack_order, s.stack_top), (None, 0, false), "{}", n.id);
        }
        assert_eq!(units(&notes).len(), 2);
        assert!(units(&notes).iter().all(|u| !u.is_stack()));
        // A plain note has nothing to be taken out of.
        assert!(take_out(&notes, "a", &AREA).is_empty());
        assert!(take_out(&notes, "nope", &AREA).is_empty());
    }

    #[test]
    fn a_sheet_taken_out_of_a_tucked_stack_comes_out_untucked_beside_it() {
        use crate::widgets::tuck::Edge;
        let mut notes = vec![sheet("a", Some("s"), 0, true), sheet("b", Some("s"), 1, false), sheet("c", Some("s"), 2, false)];
        for n in &mut notes {
            let s = n.sticky.as_mut().unwrap();
            s.rect = DipRect::new(1280.0 - 280.0, 100.0, 280.0, 208.0);
            s.tuck = Some(Edge::Right);
        }
        let edits = take_out(&notes, "a", &AREA);
        apply_edits(&mut notes, edits);
        let a = notes[0].sticky.as_ref().unwrap();
        assert_eq!(a.tuck, None);
        assert_eq!(a.rect.x, 712.0, "right does not fit: to its left, on the grid");
        let b = notes[1].sticky.as_ref().unwrap();
        assert_eq!(b.tuck, Some(Edge::Right), "the stack stays tucked");
    }

    #[test]
    fn leaving_the_stack_by_delete_or_unstick_renumbers_and_shows_the_next() {
        let mut notes = vec![
            sheet("a", Some("s"), 0, false),
            sheet("b", Some("s"), 1, true),
            sheet("c", Some("s"), 2, false),
        ];
        let edits = leave(&notes, "b");
        apply_edits(&mut notes, edits);
        notes.retain(|n| n.id != "b");
        let us = units(&notes);
        assert_eq!(us.len(), 1);
        assert_eq!(ids(&us[0]), ["a", "c"]);
        assert_eq!(us[0].sheets[us[0].top()].id, "c", "the next sheet shows");
        // The last two: the one left is plain.
        let edits = leave(&notes, "c");
        apply_edits(&mut notes, edits);
        notes.retain(|n| n.id != "c");
        let s = notes[0].sticky.as_ref().unwrap();
        assert_eq!((s.stack.clone(), s.stack_order, s.stack_top), (None, 0, false));
        // A plain note leaving changes nothing in the others.
        assert!(leave(&[plain("p", 0.0), plain("q", 1.0)], "p").is_empty());
    }

    #[test]
    fn a_move_resize_pin_roll_or_tuck_reaches_every_sheet() {
        use crate::widgets::tuck::Edge;
        let mut notes = vec![
            sheet("a", Some("s"), 0, true),
            sheet("b", Some("s"), 1, false),
            sheet("c", Some("s"), 2, false),
            plain("p", 700.0),
        ];
        {
            let s = notes[0].sticky.as_mut().unwrap();
            s.rect = DipRect::new(40.0, 60.0, 300.0, 220.0);
            s.monitor = "B".into();
            s.pinned = true;
            s.rolled = true;
            s.tuck = Some(Edge::Left);
        }
        let edits = propagate(&notes, "a");
        assert_eq!(edits.len(), 2, "b and c, not a itself, not the plain note");
        apply_edits(&mut notes, edits);
        for id in ["b", "c"] {
            let s = notes.iter().find(|n| n.id == id).unwrap().sticky.as_ref().unwrap();
            assert_eq!(s.rect, DipRect::new(40.0, 60.0, 300.0, 220.0), "{id}");
            assert_eq!((s.monitor.as_str(), s.pinned, s.rolled, s.tuck), ("B", true, true, Some(Edge::Left)), "{id}");
        }
        assert_eq!(notes[3].sticky.as_ref().unwrap().rect.x, 700.0);
        assert!(propagate(&notes, "a").is_empty(), "settled");
        assert!(propagate(&notes, "p").is_empty(), "a plain note has no stack");
        // Their paper and text stay their own.
        notes[1].sticky.as_mut().unwrap().paper = Paper::Laut;
        assert!(propagate(&notes, "a").is_empty());
    }

    #[test]
    fn the_twenty_note_cap_counts_sheets() {
        let dir = std::env::temp_dir().join(format!("vale-stack-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = NotesStore::open(dir.join("notes.json"));
        // A stack of three sheets and seventeen notes: twenty sheets.
        for i in 0..3 {
            let id = store.create_sticky(Sticky::default(), 1 + i).unwrap();
            let mut s = store.get(&id).unwrap().sticky.clone().unwrap();
            s.stack = Some("s".into());
            s.stack_order = i as u32;
            s.stack_top = i == 0;
            assert!(store.set_sticky(&id, Some(s)));
        }
        for i in 0..17 {
            assert!(store.create_sticky(Sticky::default(), 10 + i).is_some(), "{i}");
        }
        assert_eq!(units(store.list()).len(), 18, "18 windows");
        assert_eq!(store.sticky_count(), 20, "20 sheets");
        assert!(!sticky::can_add(store.sticky_count()), "full");
        assert_eq!(store.create_sticky(Sticky::default(), 99), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_counter_label() {
        assert_eq!(counter_label(0, 3), "1 / 3");
        assert_eq!(counter_label(19, 20), "20 / 20");
    }

    #[test]
    fn windows_go_with_units_by_key_and_then_by_a_sheet_they_hold() {
        let notes = vec![
            sheet("a", Some("s"), 0, true),
            sheet("b", Some("s"), 1, false),
            plain("p", 700.0),
        ];
        let us = units(&notes);
        // The stack's window and the plain note's window: each by its key.
        let existing = [
            Existing { key: "p", shown: "p", ids: vec!["p"] },
            Existing { key: "s", shown: "b", ids: vec!["a", "b"] },
        ];
        assert_eq!(match_units(&existing, &us), vec![Some(1), Some(0)]);
        // Two plain windows joined into the stack (key "stk-p"): the stack
        // takes the one showing its top sheet; the other is left over.
        let notes = vec![
            sheet("a", Some("stk-p"), 0, true),
            sheet("p", Some("stk-p"), 1, false),
        ];
        let us = units(&notes);
        let existing = [
            Existing { key: "p", shown: "p", ids: vec!["p"] },
            Existing { key: "a", shown: "a", ids: vec!["a"] },
        ];
        assert_eq!(match_units(&existing, &us), vec![Some(1)]);
        // A stack that came apart: both plain notes find a window or make one.
        let notes = vec![plain("a", 0.0), plain("b", 300.0)];
        let us = units(&notes);
        let existing = [Existing { key: "s", shown: "b", ids: vec!["a", "b"] }];
        assert_eq!(match_units(&existing, &us), vec![None, Some(0)], "the one showing b keeps it");
        // Nothing to go with: new windows.
        assert_eq!(match_units(&[], &us), vec![None, None]);
    }

    #[test]
    fn min_w_is_260() {
        assert_eq!(MIN_W, 260.0);
    }

    #[test]
    fn formatting_and_images_belong_to_their_sheet_and_never_travel_along_a_stack() {
        use crate::widgets::styles::{Run, Style};
        let mark = |n: &mut Note, name: &str| {
            let s = n.sticky.as_mut().unwrap();
            s.styles = vec![Run {
                s: 0,
                e: 4,
                style: Style { b: true, ..Style::default() },
            }];
            s.images = vec![format!("{name}.png")];
        };
        let mut notes = vec![plain("a", 50.0), plain("b", 500.0), plain("c", 900.0)];
        // Only "a" is formatted and has an image.
        mark(&mut notes[0], "a");
        let want = |notes: &[Note], id: &str| {
            let s = notes.iter().find(|n| n.id == id).unwrap().sticky.clone().unwrap();
            (s.styles, s.images)
        };
        let (a_styles, a_images) = want(&notes, "a");
        let check = |notes: &[Note], what: &str| {
            assert_eq!(want(notes, "a"), (a_styles.clone(), a_images.clone()), "{what}: a keeps its own");
            for id in ["b", "c"] {
                let (st, im) = want(notes, id);
                assert!(st.is_empty() && im.is_empty(), "{what}: {id} got something");
            }
        };
        let e = join(&notes, &["a".into()], &["b".into()]);
        apply_edits(&mut notes, e);
        check(&notes, "join");
        let e = join(&notes, &["c".into()], &["b".into()]);
        apply_edits(&mut notes, e);
        check(&notes, "join again");
        let e = propagate(&notes, "a");
        apply_edits(&mut notes, e);
        check(&notes, "propagate");
        let e = show(&notes, "c");
        apply_edits(&mut notes, e);
        check(&notes, "show");
        let e = normalise(&notes, 0, true);
        apply_edits(&mut notes, e);
        check(&notes, "normalise");
        let e = leave(&notes, "c");
        apply_edits(&mut notes, e);
        check(&notes, "leave");
        let e = take_out(&notes, "b", &AREA);
        apply_edits(&mut notes, e);
        check(&notes, "take_out");
        // `share` is field-wise: a sheet's own formatting survives it.
        let mut to = Sticky::default();
        to.images = vec!["own.png".into()];
        share(&Sticky::default(), &mut to);
        assert_eq!(to.images, ["own.png"]);
    }
}
