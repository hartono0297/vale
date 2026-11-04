//! The cat that peeks over a sticky note, as still pictures: where it sits and
//! how it looks. Pure and tested without a window.
//!
//! One cat at a time. It sits on the note whose reminder is overdue (the
//! earliest one), else on the note touched last. Its mood, in this order:
//! Clock (a reminder is overdue), Happy (every item is done), Asleep (the note
//! was last touched three days ago or more), Looking (the pointer is over the
//! note: the pupils turn toward it), Idle.

use crate::ui::robot::{Expr, Pose};
use crate::widgets::remind;
use crate::widgets::sticky::Sticky;

/// A note untouched this long puts the cat to sleep, in seconds.
pub const ASLEEP_AFTER_SECS: i64 = 3 * 24 * 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    /// Holds a small alarm clock: the reminder is overdue.
    Clock,
    /// Every checklist item is done.
    Happy,
    /// Closed lines and a small "z".
    Asleep,
    /// The pupils turn toward the pointer.
    Looking,
    Idle,
}

/// The cat's mood. `touched` is the note's last touch (unix seconds, 0 when
/// it never was, which is not asleep), `now` the time now.
pub fn mood(overdue: bool, all_done: bool, touched: i64, now: i64, pointer_over: bool) -> Mood {
    if overdue {
        Mood::Clock
    } else if all_done {
        Mood::Happy
    } else if touched > 0 && now.saturating_sub(touched) >= ASLEEP_AFTER_SECS {
        Mood::Asleep
    } else if pointer_over {
        Mood::Looking
    } else {
        Mood::Idle
    }
}

/// The face a mood draws.
pub fn expr(m: Mood) -> Expr {
    match m {
        Mood::Happy => Expr::Happy,
        Mood::Asleep => Expr::Sleep,
        Mood::Clock | Mood::Looking | Mood::Idle => Expr::Idle,
    }
}

/// How far the eyes and the head slide toward the pointer, dip.
const LOOK_EYES: f32 = 3.0;
const LOOK_HEAD: f32 = 1.5;
/// The pointer's distance from the cat (dip) that is a full turn.
const LOOK_RANGE: f32 = 120.0;
/// Steps either side of straight ahead.
const LOOK_STEPS: i32 = 2;

/// Where the pointer is, as a step from -2 (far left) to 2 (far right) of
/// the cat's centre. Quantised, so the cat redraws only when the step
/// changes, not on every mouse move.
pub fn look_step(pointer_x: f32, cat_centre_x: f32) -> i8 {
    let t = ((pointer_x - cat_centre_x) / LOOK_RANGE).clamp(-1.0, 1.0);
    (t * LOOK_STEPS as f32).round() as i8
}

/// The pose a look step gives: pupils and head toward the pointer.
pub fn look_pose(step: i8) -> Pose {
    let f = f32::from(step) / LOOK_STEPS as f32;
    Pose {
        eye_dx_dip: f * LOOK_EYES,
        head_dx_dip: f * LOOK_HEAD,
        ..Pose::default()
    }
}

/// The note the cat sits on, among `notes` as `(id, sticky)` at `now`: the
/// one whose reminder is overdue (the earliest `remind_at`), else the one
/// touched last (the first on a tie), else none.
pub fn cat_owner<'a>(notes: impl Iterator<Item = (&'a str, &'a Sticky)> + Clone, now: i64) -> Option<&'a str> {
    let overdue = notes
        .clone()
        .filter(|(_, s)| remind::overdue(s, now))
        .min_by_key(|(_, s)| s.remind_at.unwrap_or(i64::MAX))
        .map(|(id, _)| id);
    overdue.or_else(|| {
        let mut best: Option<(&str, i64)> = None;
        for (id, s) in notes {
            if s.touched > 0 && best.is_none_or(|(_, t)| s.touched > t) {
                best = Some((id, s.touched));
            }
        }
        best.map(|(id, _)| id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000_000;
    const DAY: i64 = 24 * 3600;

    #[test]
    fn the_whole_priority_table() {
        // Clock beats everything.
        for done in [false, true] {
            for touched in [0, NOW, NOW - 10 * DAY] {
                for over in [false, true] {
                    assert_eq!(mood(true, done, touched, NOW, over), Mood::Clock);
                }
            }
        }
        // Happy beats asleep, looking and idle.
        for touched in [0, NOW, NOW - 10 * DAY] {
            for over in [false, true] {
                assert_eq!(mood(false, true, touched, NOW, over), Mood::Happy);
            }
        }
        // Asleep beats looking and idle.
        for over in [false, true] {
            assert_eq!(mood(false, false, NOW - 3 * DAY, NOW, over), Mood::Asleep);
            assert_eq!(mood(false, false, NOW - 30 * DAY, NOW, over), Mood::Asleep);
        }
        // Looking beats idle.
        assert_eq!(mood(false, false, NOW, NOW, true), Mood::Looking);
        assert_eq!(mood(false, false, NOW - 2 * DAY, NOW, true), Mood::Looking);
        // Idle is the rest.
        assert_eq!(mood(false, false, NOW, NOW, false), Mood::Idle);
        assert_eq!(mood(false, false, NOW - 2 * DAY, NOW, false), Mood::Idle);
    }

    #[test]
    fn three_days_is_the_edge_of_sleep() {
        assert_eq!(mood(false, false, NOW - 3 * DAY + 1, NOW, false), Mood::Idle);
        assert_eq!(mood(false, false, NOW - 3 * DAY, NOW, false), Mood::Asleep);
        // Never touched is not asleep.
        assert_eq!(mood(false, false, 0, NOW, false), Mood::Idle);
        // A touch in the future (the clock moved back) is not asleep either.
        assert_eq!(mood(false, false, NOW + DAY, NOW, false), Mood::Idle);
    }

    #[test]
    fn each_mood_has_its_face() {
        assert_eq!(expr(Mood::Happy), Expr::Happy);
        assert_eq!(expr(Mood::Asleep), Expr::Sleep);
        assert_eq!(expr(Mood::Looking), Expr::Idle);
        assert_eq!(expr(Mood::Clock), Expr::Idle);
        assert_eq!(expr(Mood::Idle), Expr::Idle);
    }

    #[test]
    fn the_look_is_a_few_steps_toward_the_pointer() {
        assert_eq!(look_step(500.0, 500.0), 0);
        assert_eq!(look_step(1_000.0, 500.0), 2, "far right");
        assert_eq!(look_step(-1_000.0, 500.0), -2, "far left");
        assert_eq!(look_step(500.0 + 60.0, 500.0), 1);
        assert_eq!(look_step(500.0 - 60.0, 500.0), -1);
        // Nearby moves stay on one step: no redraw for them.
        assert_eq!(look_step(500.0 + 10.0, 500.0), look_step(500.0 + 20.0, 500.0));
        // The pose: toward the pointer, and straight at step 0.
        assert_eq!(look_pose(0), Pose::default());
        let right = look_pose(2);
        assert!(right.eye_dx_dip > 0.0 && right.head_dx_dip > 0.0);
        assert!(right.eye_dx_dip > right.head_dx_dip, "the eyes move more");
        let left = look_pose(-2);
        assert_eq!(left.eye_dx_dip, -right.eye_dx_dip);
        assert!(look_pose(1).eye_dx_dip < right.eye_dx_dip);
    }

    fn sticky(touched: i64, remind_at: Option<i64>, fired: bool) -> Sticky {
        Sticky {
            touched,
            remind_at,
            remind_fired: fired,
            ..Sticky::default()
        }
    }

    #[test]
    fn the_cat_sits_on_the_overdue_note_first_then_the_last_touched() {
        let notes = [
            ("a", sticky(500, None, false)),
            ("b", sticky(900, None, false)),
            ("c", sticky(100, Some(NOW - 50), true)),
            ("d", sticky(100, Some(NOW - 500), true)),
            ("e", sticky(950, Some(NOW + 500), false)),
        ];
        let iter = || notes.iter().map(|(id, s)| (*id, s));
        // Two are overdue: the earliest remind_at.
        assert_eq!(cat_owner(iter(), NOW), Some("d"));
        // A snoozed or unfired one does not count; the last touched wins.
        let notes2 = [
            ("a", sticky(500, None, false)),
            ("b", sticky(900, None, false)),
            ("e", sticky(950, Some(NOW + 500), false)),
        ];
        assert_eq!(cat_owner(notes2.iter().map(|(i, s)| (*i, s)), NOW), Some("e"));
        // A tie goes to the first.
        let tie = [("x", sticky(7, None, false)), ("y", sticky(7, None, false))];
        assert_eq!(cat_owner(tie.iter().map(|(i, s)| (*i, s)), NOW), Some("x"));
        // Nothing touched, nothing overdue: no cat.
        let none = [("x", sticky(0, None, false))];
        assert_eq!(cat_owner(none.iter().map(|(i, s)| (*i, s)), NOW), None);
        assert_eq!(cat_owner(std::iter::empty(), NOW), None);
        // An overdue note that was never touched still gets the cat.
        let late = [("t", sticky(900, None, false)), ("o", sticky(0, Some(5), true))];
        assert_eq!(cat_owner(late.iter().map(|(i, s)| (*i, s)), NOW), Some("o"));
    }
}
