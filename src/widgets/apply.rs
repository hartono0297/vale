//! What the user did to a note on the desktop (`WidgetEvent`), applied to the
//! notes. Shared by the island and the `--demo-widgets` scene, so both treat a
//! keystroke, a drag or an x the same way; pure apart from the store.

use crate::config::NotchMonitor;
use crate::notch_monitor::{self, Display};
use crate::notes::{Note, NotesStore};
use crate::widgets::host::WidgetEvent;
use crate::widgets::sticky::{DipRect, Sticky};
use crate::widgets::templates::Template;
use crate::widgets::{paint, place, snap, stack};

/// What applying a batch of events asks of the owner.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// Something that must be saved changed.
    pub changed: bool,
    /// A note came off the desktop or was deleted: the windows need a sync.
    pub structural: bool,
    /// Something eases: the frame clock must run.
    pub clock: bool,
    /// The notes whose (+) was pressed (a right click chose a template).
    pub beside: Vec<(String, Template)>,
    /// "Kucing ngintip" was switched from a note's popover.
    pub cat_peek: Option<bool>,
    /// A reminder was set or cleared: the timer is armed again and the
    /// notices are settled.
    pub reminders: bool,
    /// Notes that came off the desktop or went: their notices go too.
    pub gone: Vec<String>,
    /// Sheets to take out of their stack (the owner places them beside it,
    /// `stack_out`, since that needs the displays).
    pub unstack: Vec<String>,
    /// "Kirim ke HP" was chosen on this note (the last one wins): the island
    /// shows it on the phone link.
    pub send_phone: Option<String>,
}

/// Edits note `id`'s sticky part. False when it has none or nothing changed.
fn edit_sticky(notes: &mut NotesStore, id: &str, f: impl FnOnce(&mut Sticky)) -> bool {
    let Some(mut s) = notes.get(id).and_then(|n| n.sticky.clone()) else {
        return false;
    };
    f(&mut s);
    notes.set_sticky(id, Some(s))
}

/// The sticky parts in `edits` written to the store. True when one changed.
pub fn write_edits(notes: &mut NotesStore, edits: stack::Edits) -> bool {
    let mut changed = false;
    for (id, s) in edits {
        changed |= notes.set_sticky(&id, Some(s));
    }
    changed
}

/// What a sheet shares with its stack (place, size, pin, roll, tuck) written
/// to every other sheet of it.
fn spread(notes: &mut NotesStore, id: &str) -> bool {
    let edits = stack::propagate(notes.list(), id);
    write_edits(notes, edits)
}

/// The stacks made right (`stack::normalise`): one sheet shown, orders 0..n-1,
/// a stack id one sheet carries alone cleared. `overdue_top` brings a sheet
/// whose reminder is overdue to the top (at startup and when one fires).
/// True when something changed.
pub fn settle(notes: &mut NotesStore, now: i64, overdue_top: bool) -> bool {
    let edits = stack::normalise(notes.list(), now, overdue_top);
    write_edits(notes, edits)
}

/// Sheet `id` taken out of its stack and placed beside it, on the display the
/// stack is on. True when something changed.
pub fn stack_out(notes: &mut NotesStore, displays: &[Display], id: &str) -> bool {
    let Some(sticky) = notes.get(id).and_then(|n| n.sticky.as_ref()) else {
        return false;
    };
    let area = place::resolve(&sticky.monitor, displays)
        .and_then(|(i, _)| displays.get(i))
        .map_or(DipRect::new(0.0, 0.0, 1920.0, 1040.0), place::snap_area);
    let edits = stack::take_out(notes.list(), id, &area);
    write_edits(notes, edits)
}

/// Note `id`'s reminder set to `at` (unix seconds; `None` clears it), not
/// fired. `fired` marks it as gone off (the island's timer). False when the
/// note has no sticky part or nothing changed.
pub fn set_reminder(notes: &mut NotesStore, id: &str, at: Option<i64>, fired: bool) -> bool {
    edit_sticky(notes, id, |s| {
        s.remind_at = at;
        s.remind_fired = fired && at.is_some();
    })
}

/// Applies `events` in order. `unix` is now, for the edit time of a text.
pub fn apply(notes: &mut NotesStore, events: Vec<WidgetEvent>, unix: i64) -> Outcome {
    let mut out = Outcome::default();
    for ev in events {
        match ev {
            WidgetEvent::Text { id, text } => out.changed |= notes.set_text(&id, &text, unix),
            WidgetEvent::Placed { id, monitor, rect } => {
                out.changed |= edit_sticky(notes, &id, |s| {
                    s.monitor = monitor;
                    s.rect = rect;
                });
                out.changed |= spread(notes, &id);
            }
            WidgetEvent::Paper { id, paper } => {
                out.changed |= edit_sticky(notes, &id, |s| s.paper = paper);
            }
            WidgetEvent::Size { id, size } => {
                out.changed |= edit_sticky(notes, &id, |s| s.text_size = size);
            }
            WidgetEvent::Font { id, font } => {
                out.changed |= edit_sticky(notes, &id, |s| s.font = font);
            }
            WidgetEvent::Pattern { id, pattern } => {
                out.changed |= edit_sticky(notes, &id, |s| s.pattern = pattern);
            }
            WidgetEvent::Deco { id, deco } => {
                out.changed |= edit_sticky(notes, &id, |s| s.deco = deco);
            }
            WidgetEvent::Stamp { id, stamp } => {
                out.changed |= edit_sticky(notes, &id, |s| s.stamp = stamp);
            }
            WidgetEvent::Opacity { id, percent } => {
                out.changed |= edit_sticky(notes, &id, |s| {
                    s.opacity = crate::widgets::sticky::snap_opacity(i64::from(percent));
                });
            }
            WidgetEvent::Pinned { id, on } => {
                out.changed |= edit_sticky(notes, &id, |s| s.pinned = on);
                out.changed |= spread(notes, &id);
            }
            WidgetEvent::Rolled { id, on } => {
                out.changed |= edit_sticky(notes, &id, |s| s.rolled = on);
                out.changed |= spread(notes, &id);
            }
            WidgetEvent::Tuck { id, edge } => {
                out.changed |= edit_sticky(notes, &id, |s| s.tuck = edge);
                out.changed |= spread(notes, &id);
            }
            WidgetEvent::Stack { dragged, target } => {
                let done = write_edits(notes, stack::join(notes.list(), &dragged, &target));
                out.changed |= done;
                out.structural |= done;
            }
            WidgetEvent::StackShow { id } => {
                out.changed |= write_edits(notes, stack::show(notes.list(), &id));
            }
            WidgetEvent::StackOut { id } => {
                out.unstack.push(id);
                out.structural = true;
            }
            WidgetEvent::Remind { id, at } => {
                out.changed |= set_reminder(notes, &id, at, false);
                out.reminders = true;
            }
            WidgetEvent::Touched { id, at } => {
                out.changed |= edit_sticky(notes, &id, |s| s.touched = at);
            }
            WidgetEvent::CatPeek(on) => out.cat_peek = Some(on),
            WidgetEvent::NewBeside { id, template } => out.beside.push((id, template)),
            WidgetEvent::Unstick { id } => {
                // The rest of its stack closes up first.
                out.changed |= write_edits(notes, stack::leave(notes.list(), &id));
                let done = notes.set_sticky(&id, None);
                out.changed |= done;
                out.structural |= done;
                if done {
                    out.gone.push(id);
                }
            }
            WidgetEvent::Delete { id } => {
                out.changed |= write_edits(notes, stack::leave(notes.list(), &id));
                let done = notes.delete(&id);
                out.changed |= done;
                out.structural |= done;
                if done {
                    out.gone.push(id);
                }
            }
            WidgetEvent::SendToPhone { id } => out.send_phone = Some(id),
            WidgetEvent::NeedsClock => out.clock = true,
        }
    }
    // A stack left with one sheet is a plain note; one sheet shows.
    out.changed |= settle(notes, unix, false);
    out
}

/// The sticky part of a note made now: beside note `beside` (its + was
/// pressed), else near the notch on the notch's monitor, on the first free
/// grid slot. Krem, default size, touched now (so it gets the cat).
pub fn sticky_for(
    notes: &[Note],
    displays: &[Display],
    notch: &NotchMonitor,
    beside: Option<&str>,
    unix: i64,
) -> Sticky {
    let pairs: Vec<(usize, &Sticky)> = notes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| n.sticky.as_ref().map(|s| (i, s)))
        .collect();
    let source = beside.and_then(|id| notes.iter().find(|n| n.id == id)?.sticky.as_ref());
    let display = match source {
        Some(s) => place::resolve(&s.monitor, displays).map(|(i, _)| i),
        None => notch_monitor::resolve(notch, displays),
    };
    let fresh = Sticky {
        touched: unix,
        ..Sticky::default()
    };
    let Some((i, disp)) = display.and_then(|i| displays.get(i).map(|d| (i, d))) else {
        return fresh;
    };
    let area = place::snap_area(disp);
    let rect = match source {
        Some(s) => {
            let shown = DipRect {
                h: paint::shown_h(s.rect.h, s.rolled),
                ..s.rect
            };
            snap::place_beside(&shown, &area)
        }
        None => {
            let others = place::others_on(&pairs, displays, i, usize::MAX);
            snap::place_new(&area, &others)
        }
    };
    Sticky {
        rect,
        monitor: disp.path.clone(),
        ..fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrange::Monitor;
    use crate::capture::Rect;
    use crate::widgets::sticky::{Paper, TextSize};
    use std::path::PathBuf;


    fn display(n: usize, x: i32, primary: bool, path: &str) -> Display {
        Display {
            monitor: Monitor {
                device: format!(r"\\.\DISPLAY{n}"),
                rect: Rect::new(x, 0, 1920, 1080),
                work: Rect::new(x, 0, 1920, 1040),
                dpi: 96,
            },
            hmonitor: n as isize,
            primary,
            path: path.into(),
            name: String::new(),
            number: n,
        }
    }

    fn two() -> Vec<Display> {
        vec![display(1, 0, true, "A"), display(2, 1920, false, "B")]
    }

    #[test]
    fn a_new_note_goes_under_the_notch_on_the_notch_monitor() {
        let d = two();
        let s = sticky_for(&[], &d, &NotchMonitor::Primary, None, 77);
        assert_eq!(s.monitor, "A");
        assert_eq!(s.touched, 77);
        assert_eq!((s.rect.x, s.rect.y), (840.0, 24.0), "centred on 1920, under the notch");
        let chosen = NotchMonitor::Chosen { path: "B".into(), name: String::new() };
        assert_eq!(sticky_for(&[], &d, &chosen, None, 1).monitor, "B");
        // The notch monitor is gone: the primary.
        let gone = NotchMonitor::Chosen { path: "C".into(), name: String::new() };
        assert_eq!(sticky_for(&[], &d, &gone, None, 1).monitor, "A");
        // No display at all: still a sticky, at the default place.
        assert_eq!(sticky_for(&[], &[], &NotchMonitor::Primary, None, 1).rect, Sticky::default().rect);
    }

    #[test]
    fn the_second_note_does_not_land_on_the_first() {
        let d = two();
        let mut notes = NotesStore::open(std::env::temp_dir().join(format!("vale-apply-second-{}", std::process::id())).join("n.json"));
        let first = sticky_for(notes.list(), &d, &NotchMonitor::Primary, None, 1);
        notes.create_sticky(first.clone(), 1).unwrap();
        let second = sticky_for(notes.list(), &d, &NotchMonitor::Primary, None, 2);
        assert!(!second.rect.intersects(&first.rect), "{second:?} on {first:?}");
    }

    #[test]
    fn beside_goes_to_the_right_of_the_source_on_its_own_monitor() {
        let d = two();
        let mut notes = NotesStore::open(std::env::temp_dir().join(format!("vale-apply-beside-{}", std::process::id())).join("n.json"));
        let src = Sticky {
            monitor: "B".into(),
            rect: DipRect::new(100.0, 100.0, 240.0, 208.0),
            ..Sticky::default()
        };
        let id = notes.create_sticky(src, 1).unwrap();
        let s = sticky_for(notes.list(), &d, &NotchMonitor::Primary, Some(&id), 5);
        assert_eq!(s.monitor, "B", "the notch is on A, the note's own monitor wins");
        assert_eq!((s.rect.x, s.rect.y), (352.0, 104.0));
    }

    fn store(name: &str) -> NotesStore {
        let dir = std::env::temp_dir().join(format!("vale-apply-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        NotesStore::open(PathBuf::from(dir).join("notes.json"))
    }

    fn id_of(notes: &mut NotesStore) -> String {
        notes.create_sticky(Sticky::default(), 1).unwrap()
    }

    #[test]
    fn typing_moving_and_colouring_land_in_the_note() {
        let mut notes = store("edits");
        let id = id_of(&mut notes);
        let out = apply(
            &mut notes,
            vec![
                WidgetEvent::Text { id: id.clone(), text: "Belanja\n- [ ] bata".into() },
                WidgetEvent::Placed {
                    id: id.clone(),
                    monitor: "A".into(),
                    rect: DipRect::new(16.0, 24.0, 300.0, 200.0),
                },
                WidgetEvent::Paper { id: id.clone(), paper: Paper::Sage },
                WidgetEvent::Size { id: id.clone(), size: TextSize::L },
                WidgetEvent::Pinned { id: id.clone(), on: true },
                WidgetEvent::Rolled { id: id.clone(), on: true },
                WidgetEvent::Touched { id: id.clone(), at: 99 },
            ],
            50,
        );
        assert!(out.changed && !out.structural && !out.clock);
        let note = notes.get(&id).unwrap();
        assert_eq!(note.text, "Belanja\n- [ ] bata");
        assert_eq!(note.updated, 50);
        let s = note.sticky.as_ref().unwrap();
        assert_eq!(s.monitor, "A");
        assert_eq!(s.rect, DipRect::new(16.0, 24.0, 300.0, 200.0));
        assert_eq!((s.paper, s.text_size), (Paper::Sage, TextSize::L));
        assert!(s.pinned && s.rolled);
        assert_eq!(s.touched, 99);
    }

    #[test]
    fn the_look_of_a_note_is_applied_field_by_field() {
        use crate::widgets::sticky::{Deco, NoteFont, Pattern, Stamp};
        let mut notes = store("looks");
        let id = id_of(&mut notes);
        let out = apply(
            &mut notes,
            vec![
                WidgetEvent::Font { id: id.clone(), font: NoteFont::Tangan },
                WidgetEvent::Pattern { id: id.clone(), pattern: Pattern::Kotak },
                WidgetEvent::Deco { id: id.clone(), deco: Deco::Selotip },
                WidgetEvent::Stamp { id: id.clone(), stamp: Stamp::Beres },
                WidgetEvent::Opacity { id: id.clone(), percent: 60 },
            ],
            1,
        );
        assert!(out.changed && !out.structural && !out.reminders);
        let s = notes.get(&id).unwrap().sticky.clone().unwrap();
        assert_eq!(
            (s.font, s.pattern, s.deco, s.stamp, s.opacity),
            (NoteFont::Tangan, Pattern::Kotak, Deco::Selotip, Stamp::Beres, 60)
        );
        // Setting the same again changes nothing to save.
        let out = apply(&mut notes, vec![WidgetEvent::Stamp { id: id.clone(), stamp: Stamp::Beres }], 1);
        assert!(!out.changed);
        // Back to the defaults; an off-grid opacity snaps.
        apply(
            &mut notes,
            vec![
                WidgetEvent::Font { id: id.clone(), font: NoteFont::Vale },
                WidgetEvent::Stamp { id: id.clone(), stamp: Stamp::None },
                WidgetEvent::Opacity { id: id.clone(), percent: 75 },
            ],
            1,
        );
        let s = notes.get(&id).unwrap().sticky.clone().unwrap();
        assert_eq!((s.font, s.stamp, s.opacity), (NoteFont::Vale, Stamp::None, 80));
        // A note that is gone changes nothing.
        let out = apply(
            &mut notes,
            vec![
                WidgetEvent::Font { id: "n0-nope".into(), font: NoteFont::Mono },
                WidgetEvent::Opacity { id: "n0-nope".into(), percent: 40 },
            ],
            1,
        );
        assert_eq!(out, Outcome::default());
    }

    #[test]
    fn a_reminder_is_set_and_cleared() {
        let mut notes = store("remind");
        let id = id_of(&mut notes);
        let out = apply(
            &mut notes,
            vec![WidgetEvent::Remind { id: id.clone(), at: Some(5_000) }],
            1,
        );
        assert!(out.changed && out.reminders && !out.structural);
        let s = notes.get(&id).unwrap().sticky.clone().unwrap();
        assert_eq!((s.remind_at, s.remind_fired), (Some(5_000), false));
        // The timer marks it fired; a new time (a snooze) un-fires it.
        assert!(set_reminder(&mut notes, &id, Some(5_000), true));
        assert!(notes.get(&id).unwrap().sticky.as_ref().unwrap().remind_fired);
        apply(
            &mut notes,
            vec![WidgetEvent::Remind { id: id.clone(), at: Some(5_600) }],
            1,
        );
        let s = notes.get(&id).unwrap().sticky.clone().unwrap();
        assert_eq!((s.remind_at, s.remind_fired), (Some(5_600), false));
        // Clearing it.
        let out = apply(
            &mut notes,
            vec![WidgetEvent::Remind { id: id.clone(), at: None }],
            1,
        );
        assert!(out.changed && out.reminders);
        let s = notes.get(&id).unwrap().sticky.clone().unwrap();
        assert_eq!((s.remind_at, s.remind_fired), (None, false));
        // Clearing it again changes nothing to save (the owner still re-arms).
        let out = apply(&mut notes, vec![WidgetEvent::Remind { id, at: None }], 1);
        assert!(!out.changed && out.reminders);
        // A note that is gone: nothing changes.
        assert!(!set_reminder(&mut notes, "n0-nope", Some(1), false));
    }


    /// Three sheets of one stack in a store: ids in order.
    fn stack_store(name: &str) -> (NotesStore, [String; 3]) {
        let mut notes = store(name);
        let ids: Vec<String> = (0..3u32)
            .map(|k| {
                let id = notes
                    .create_sticky(
                        Sticky {
                            stack: Some("s1".into()),
                            stack_order: k,
                            stack_top: k == 0,
                            rect: DipRect::new(400.0, 200.0, 280.0, 208.0),
                            monitor: "A".into(),
                            ..Sticky::default()
                        },
                        1 + i64::from(k),
                    )
                    .unwrap();
                notes.set_text(&id, &format!("sheet {k}"), 1 + i64::from(k));
                id
            })
            .collect();
        (notes, [ids[0].clone(), ids[1].clone(), ids[2].clone()])
    }

    fn sticky_of(notes: &NotesStore, id: &str) -> Sticky {
        notes.get(id).unwrap().sticky.clone().unwrap()
    }

    #[test]
    fn a_move_a_pin_a_roll_and_a_tuck_of_one_sheet_reach_the_whole_stack() {
        use crate::widgets::tuck::Edge;
        let (mut notes, ids) = stack_store("stack-propagate");
        let out = apply(
            &mut notes,
            vec![
                WidgetEvent::Placed {
                    id: ids[0].clone(),
                    monitor: "B".into(),
                    rect: DipRect::new(40.0, 56.0, 300.0, 220.0),
                },
                WidgetEvent::Pinned { id: ids[0].clone(), on: true },
                WidgetEvent::Rolled { id: ids[0].clone(), on: true },
                WidgetEvent::Tuck { id: ids[0].clone(), edge: Some(Edge::Left) },
            ],
            9,
        );
        assert!(out.changed && !out.structural);
        for id in &ids {
            let s = sticky_of(&notes, id);
            assert_eq!(s.rect, DipRect::new(40.0, 56.0, 300.0, 220.0), "{id}");
            assert_eq!((s.monitor.as_str(), s.pinned, s.rolled, s.tuck), ("B", true, true, Some(Edge::Left)), "{id}");
        }
        // Brought out again: every sheet is.
        apply(&mut notes, vec![WidgetEvent::Tuck { id: ids[1].clone(), edge: None }], 9);
        assert!(ids.iter().all(|id| sticky_of(&notes, id).tuck.is_none()));
        // A paper is the sheet's own.
        apply(&mut notes, vec![WidgetEvent::Paper { id: ids[2].clone(), paper: Paper::Plum }], 9);
        assert_eq!(sticky_of(&notes, &ids[0]).paper, Paper::Krem);
        assert_eq!(sticky_of(&notes, &ids[2]).paper, Paper::Plum);
    }

    #[test]
    fn dropping_a_note_on_another_is_structural_and_makes_a_stack() {
        let mut notes = store("stack-join");
        let a = notes
            .create_sticky(Sticky { rect: DipRect::new(10.0, 10.0, 240.0, 208.0), ..Sticky::default() }, 1)
            .unwrap();
        let b = notes
            .create_sticky(Sticky { rect: DipRect::new(500.0, 100.0, 200.0, 160.0), ..Sticky::default() }, 2)
            .unwrap();
        let out = apply(
            &mut notes,
            vec![WidgetEvent::Stack { dragged: vec![a.clone()], target: vec![b.clone()] }],
            5,
        );
        assert!(out.changed && out.structural);
        let (sa, sb) = (sticky_of(&notes, &a), sticky_of(&notes, &b));
        assert_eq!(sa.stack.as_deref(), Some(format!("stk-{b}").as_str()));
        assert_eq!(sa.stack, sb.stack);
        assert_eq!((sa.stack_order, sa.stack_top), (0, true), "the dragged sheet first and shown");
        assert_eq!((sb.stack_order, sb.stack_top), (1, false));
        assert_eq!(sa.rect, DipRect::new(500.0, 100.0, 260.0, 160.0), "the target's, at least MIN_W wide");
        assert_eq!(sa.rect, sb.rect);
        // Joining something that is gone changes nothing.
        let out = apply(
            &mut notes,
            vec![WidgetEvent::Stack { dragged: vec![a.clone()], target: vec!["n0-nope".into()] }],
            5,
        );
        assert_eq!(out, Outcome::default());
    }

    #[test]
    fn a_flip_is_saved_as_the_top_and_taking_a_sheet_out_is_left_to_the_owner() {
        let (mut notes, ids) = stack_store("stack-show");
        let out = apply(&mut notes, vec![WidgetEvent::StackShow { id: ids[2].clone() }], 5);
        assert!(out.changed && !out.structural, "the window already shows it");
        let tops: Vec<bool> = ids.iter().map(|id| sticky_of(&notes, id).stack_top).collect();
        assert_eq!(tops, [false, false, true]);
        let out = apply(&mut notes, vec![WidgetEvent::StackOut { id: ids[2].clone() }], 5);
        assert_eq!(out.unstack, vec![ids[2].clone()]);
        assert!(out.structural && !out.changed);
        // The owner places it beside the stack on the displays it knows.
        let d = two();
        assert!(stack_out(&mut notes, &d, &ids[2]));
        let s = sticky_of(&notes, &ids[2]);
        assert_eq!((s.stack.clone(), s.stack_order, s.stack_top, s.tuck), (None, 0, false, None));
        assert_eq!(s.monitor, "A", "stays on its monitor");
        assert_eq!((s.rect.x, s.rect.y, s.rect.w), (696.0, 200.0, 280.0), "to the right of the stack: gap 12, on the grid");
        // The next sheet in order shows now: the first.
        assert!(sticky_of(&notes, &ids[0]).stack_top, "wraps to the first");
        assert!(!sticky_of(&notes, &ids[1]).stack_top);
    }

    #[test]
    fn deleting_or_unsticking_a_sheet_leaves_the_rest_of_the_stack() {
        let (mut notes, ids) = stack_store("stack-leave");
        // The shown sheet is deleted: the next one shows.
        let out = apply(&mut notes, vec![WidgetEvent::Delete { id: ids[0].clone() }], 5);
        assert!(out.changed && out.structural);
        assert!(notes.get(&ids[0]).is_none());
        let (b, c) = (sticky_of(&notes, &ids[1]), sticky_of(&notes, &ids[2]));
        assert_eq!((b.stack.as_deref(), b.stack_order, b.stack_top), (Some("s1"), 0, true));
        assert_eq!((c.stack.as_deref(), c.stack_order, c.stack_top), (Some("s1"), 1, false));
        // Unstick one of the last two: the other is a plain note.
        apply(&mut notes, vec![WidgetEvent::Unstick { id: ids[1].clone() }], 6);
        assert!(notes.get(&ids[1]).is_some_and(|n| n.sticky.is_none()), "its text stays on the Notes page");
        let c = sticky_of(&notes, &ids[2]);
        assert_eq!((c.stack, c.stack_order, c.stack_top), (None, 0, false));
    }

    #[test]
    fn a_lone_sheet_of_an_old_or_edited_file_is_made_plain_by_the_next_apply() {
        let mut notes = store("stack-lone");
        let id = notes
            .create_sticky(Sticky { stack: Some("ghost".into()), stack_order: 3, stack_top: true, ..Sticky::default() }, 1)
            .unwrap();
        // Any event settles it; so does `settle`.
        assert!(settle(&mut notes, 1, false));
        let s = sticky_of(&notes, &id);
        assert_eq!((s.stack, s.stack_order, s.stack_top), (None, 0, false));
        assert!(!settle(&mut notes, 1, false), "settled");
        // A plain note's file is not touched by an apply.
        let out = apply(&mut notes, vec![WidgetEvent::NeedsClock], 1);
        assert!(!out.changed);
    }

    #[test]
    fn an_overdue_sheet_comes_to_the_top_when_asked() {
        let (mut notes, ids) = stack_store("stack-overdue");
        assert!(set_reminder(&mut notes, &ids[2], Some(100), true));
        assert!(!settle(&mut notes, 1_000, false), "not on every sync: a flip away from it stays");
        assert!(sticky_of(&notes, &ids[0]).stack_top);
        assert!(settle(&mut notes, 1_000, true), "at startup and when it fires");
        assert!(sticky_of(&notes, &ids[2]).stack_top);
        assert!(!sticky_of(&notes, &ids[0]).stack_top);
    }

    #[test]
    fn a_new_note_does_not_land_on_a_stack() {
        let (notes, _) = stack_store("stack-new");
        let d = two();
        let s = sticky_for(notes.list(), &d, &NotchMonitor::Primary, None, 9);
        let stack_rect = DipRect::new(400.0, 200.0, 280.0, 208.0);
        assert!(!s.rect.intersects(&stack_rect), "the new note is not on the stack: {:?}", s.rect);
    }

    #[test]
    fn unsticking_and_deleting_are_structural() {
        let mut notes = store("structural");
        let empty = id_of(&mut notes);
        let kept = id_of(&mut notes);
        notes.set_text(&kept, "keep", 2);
        let gone = id_of(&mut notes);
        let out = apply(
            &mut notes,
            vec![
                WidgetEvent::Unstick { id: empty.clone() },
                WidgetEvent::Unstick { id: kept.clone() },
                WidgetEvent::Delete { id: gone.clone() },
            ],
            3,
        );
        assert!(out.changed && out.structural);
        assert_eq!(out.gone, vec![empty.clone(), kept.clone(), gone.clone()], "their notices go");
        assert!(notes.get(&empty).is_none(), "an empty note leaves nothing");
        assert!(notes.get(&kept).is_some_and(|n| n.sticky.is_none()));
        assert!(notes.get(&gone).is_none());
    }

    #[test]
    fn the_rest_is_left_to_the_owner() {
        let mut notes = store("owner");
        let id = id_of(&mut notes);
        let out = apply(
            &mut notes,
            vec![
                WidgetEvent::NewBeside { id: id.clone(), template: Template::Rapat },
                WidgetEvent::CatPeek(false),
                WidgetEvent::SendToPhone { id: id.clone() },
                WidgetEvent::NeedsClock,
            ],
            1,
        );
        assert_eq!(out.send_phone.as_deref(), Some(id.as_str()));
        assert_eq!(out.beside, vec![(id, Template::Rapat)]);
        assert_eq!(out.cat_peek, Some(false));
        assert!(out.clock && !out.changed && !out.structural);
        // An event for a note that is gone changes nothing.
        let out = apply(
            &mut notes,
            vec![
                WidgetEvent::Text { id: "n0-nope".into(), text: "x".into() },
                WidgetEvent::Paper { id: "n0-nope".into(), paper: Paper::Plum },
            ],
            1,
        );
        assert_eq!(out, Outcome::default());
    }

    /// A store with `n` sticky sheets: the first `stacked` of them in stacks of
    /// at most four, the rest plain.
    fn crowded(name: &str, n: usize, stacked: usize) -> NotesStore {
        let mut notes = store(name);
        for k in 0..n {
            let mut s = Sticky::default();
            if k < stacked {
                s.stack = Some(format!("stk{}", k / 4));
                s.stack_order = (k % 4) as u32;
                s.stack_top = k % 4 == 0;
            }
            notes.create_sticky(s, k as i64).expect("room");
        }
        notes
    }

    #[test]
    fn at_twenty_sheets_with_stacks_every_way_to_make_a_note_makes_nothing() {
        use crate::widgets::sticky::{MAX_STICKY, can_add};
        // 20 sheets: 8 of them inside two stacks, 12 plain (14 windows).
        let mut notes = crowded("cap20", MAX_STICKY, 8);
        assert_eq!(notes.sticky_count(), MAX_STICKY, "sheets, not windows, are counted");
        // The count every way to make a note asks (the gallery +, the tray,
        // hotkey 7, the header (+), the Notes page pin and the prefill all ask
        // `can_add(sticky_count())`; the host's (+) asks `sheet_total`, the same
        // number, tested in the host).
        assert!(!can_add(notes.sticky_count()));
        // The store itself refuses, whatever the caller does.
        assert_eq!(notes.create_sticky(Sticky::default(), 99), None);
        assert_eq!(notes.sticky_count(), MAX_STICKY);
        // A prefill is not read at the cap, with the switch on or off.
        assert!(!crate::widgets::cliptext::fill_wanted(true, notes.sticky_count()));
        assert!(!crate::widgets::cliptext::fill_wanted(false, notes.sticky_count()));
        // One sheet fewer: a note fits, and the prefill is wanted only when on.
        let notes = crowded("cap19", MAX_STICKY - 1, 8);
        assert!(crate::widgets::cliptext::fill_wanted(true, notes.sticky_count()));
        assert!(!crate::widgets::cliptext::fill_wanted(false, notes.sticky_count()));
    }

    #[test]
    fn a_prefilled_note_counts_as_one_sheet_like_any_other() {
        use crate::widgets::sticky::{MAX_STICKY, can_add};
        let mut notes = crowded("cap-prefill", MAX_STICKY - 1, 4);
        let filled = crate::widgets::cliptext::prefill("Jl. Kenanga 12\r\nKirim sebelum Jumat").unwrap();
        let id = notes.create_sticky(Sticky::default(), 50).expect("the 20th fits");
        notes.set_text(&id, &filled, 50);
        assert_eq!(notes.sticky_count(), MAX_STICKY, "one sheet, not one per line");
        assert!(!can_add(notes.sticky_count()), "now full");
        assert_eq!(notes.create_sticky(Sticky::default(), 51), None);
        // Taking a sheet out of its stack and off the desktop frees one place.
        let stacked = notes.list().iter().find(|n| n.sticky.as_ref().is_some_and(|s| s.stack.is_some())).unwrap().id.clone();
        assert!(notes.set_sticky(&stacked, None));
        assert_eq!(notes.sticky_count(), MAX_STICKY - 1);
        assert!(can_add(notes.sticky_count()));
    }
}
