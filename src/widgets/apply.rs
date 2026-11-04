//! What the user did to a note on the desktop (`WidgetEvent`), applied to the
//! notes. Shared by the island and the `--demo-widgets` scene, so both treat a
//! keystroke, a drag or an x the same way; pure apart from the store.

use crate::config::NotchMonitor;
use crate::notch_monitor::{self, Display};
use crate::notes::{Note, NotesStore};
use crate::widgets::host::WidgetEvent;
use crate::widgets::sticky::{DipRect, Sticky};
use crate::widgets::templates::Template;
use crate::widgets::{paint, place, snap};

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
}

/// Edits note `id`'s sticky part. False when it has none or nothing changed.
fn edit_sticky(notes: &mut NotesStore, id: &str, f: impl FnOnce(&mut Sticky)) -> bool {
    let Some(mut s) = notes.get(id).and_then(|n| n.sticky.clone()) else {
        return false;
    };
    f(&mut s);
    notes.set_sticky(id, Some(s))
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
            }
            WidgetEvent::Paper { id, paper } => {
                out.changed |= edit_sticky(notes, &id, |s| s.paper = paper);
            }
            WidgetEvent::Size { id, size } => {
                out.changed |= edit_sticky(notes, &id, |s| s.text_size = size);
            }
            WidgetEvent::Pinned { id, on } => {
                out.changed |= edit_sticky(notes, &id, |s| s.pinned = on);
            }
            WidgetEvent::Rolled { id, on } => {
                out.changed |= edit_sticky(notes, &id, |s| s.rolled = on);
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
                let done = notes.set_sticky(&id, None);
                out.changed |= done;
                out.structural |= done;
                if done {
                    out.gone.push(id);
                }
            }
            WidgetEvent::Delete { id } => {
                let done = notes.delete(&id);
                out.changed |= done;
                out.structural |= done;
                if done {
                    out.gone.push(id);
                }
            }
            WidgetEvent::NeedsClock => out.clock = true,
        }
    }
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
                WidgetEvent::NeedsClock,
            ],
            1,
        );
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
}
