//! A sticky note's reminder ("Ingatkan aku"): the time maths, pure and tested
//! without a window. The wall clock is the local one (`NaiveDateTime`); a
//! reminder is stored as unix seconds (`Sticky::remind_at`).
//!
//! The island arms exactly one one-shot timer (`TIMER_STICKY_REMIND`, 47) for
//! the nearest unfired reminder; nothing polls. A reminder whose time passed
//! while the PC was off or asleep is due the moment the island looks again
//! (`due`).

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};

use crate::i18n::{self, Key, Lang, text};
use crate::notes::Note;
use crate::widgets::sticky::Sticky;

/// The longest wait `SetTimer` takes (`USER_TIMER_MAXIMUM`), in ms. A longer
/// reminder is armed for this long and re-armed when it fires.
pub const USER_TIMER_MAXIMUM: u32 = 0x7FFF_FFFF;
/// The shortest wait `SetTimer` honours (`USER_TIMER_MINIMUM`), in ms.
pub const USER_TIMER_MINIMUM: u32 = 10;
/// "Tunda 10 mnt".
pub const SNOOZE_SECS: i64 = 10 * 60;

/// The quick choices of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// 15 mnt.
    In15,
    /// 1 jam.
    In1h,
    /// 17.00 today, or tomorrow when 17.00 has passed.
    Evening,
    /// besok 09.00.
    Tomorrow9,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::In15, Preset::In1h, Preset::Evening, Preset::Tomorrow9];

    pub fn label_key(self) -> Key {
        match self {
            Preset::In15 => Key::StickyRemind15,
            Preset::In1h => Key::StickyRemind1h,
            Preset::Evening => Key::StickyRemindEvening,
            Preset::Tomorrow9 => Key::StickyRemindTomorrow,
        }
    }

    /// When this choice reminds, from the local time `now`. 17.00 exactly
    /// counts as passed.
    pub fn at(self, now: NaiveDateTime) -> NaiveDateTime {
        match self {
            Preset::In15 => now + Duration::minutes(15),
            Preset::In1h => now + Duration::hours(1),
            Preset::Evening => {
                let five = now.date().and_time(hm(17, 0));
                if now >= five {
                    five + Duration::days(1)
                } else {
                    five
                }
            }
            Preset::Tomorrow9 => (now.date() + Duration::days(1)).and_time(hm(9, 0)),
        }
    }
}

fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).expect("a valid time of day")
}

/// A local wall-clock time as unix seconds. A time that does not exist (the
/// hour a clock jumps over) is read an hour later; one that happens twice, the
/// first time.
pub fn to_unix(local: NaiveDateTime) -> i64 {
    Local
        .from_local_datetime(&local)
        .earliest()
        .or_else(|| {
            Local
                .from_local_datetime(&(local + Duration::hours(1)))
                .earliest()
        })
        .map_or_else(|| local.and_utc().timestamp(), |t| t.timestamp())
}

/// Unix seconds as the local wall-clock time.
pub fn from_unix(ts: i64) -> NaiveDateTime {
    DateTime::from_timestamp(ts, 0)
        .map(|t| t.with_timezone(&Local).naive_local())
        .unwrap_or_default()
}

/// Snoozed: ten minutes from `now` (unix seconds).
pub fn snooze(now: i64) -> i64 {
    now + SNOOZE_SECS
}

/// The earliest reminder that has not fired yet (unix seconds), over every
/// note on the desktop. `None` when there is none.
pub fn nearest(notes: &[Note]) -> Option<i64> {
    notes
        .iter()
        .filter_map(|n| n.sticky.as_ref())
        .filter(|s| !s.remind_fired)
        .filter_map(|s| s.remind_at)
        .min()
}

/// The notes whose reminder is due at `now` and has not fired: the ones whose
/// time came, and the ones that came while nobody was looking (the PC off or
/// asleep).
pub fn due(notes: &[Note], now: i64) -> Vec<String> {
    notes
        .iter()
        .filter(|n| {
            n.sticky
                .as_ref()
                .is_some_and(|s| !s.remind_fired && s.remind_at.is_some_and(|t| t <= now))
        })
        .map(|n| n.id.clone())
        .collect()
}

/// The fired reminders that are ahead of `now` again (the clock was set back
/// past them), with their time. They have not rung at this time yet.
pub fn fired_in_future(notes: &[Note], now: i64) -> Vec<(String, i64)> {
    notes
        .iter()
        .filter_map(|n| {
            let s = n.sticky.as_ref()?;
            let at = s.remind_at.filter(|t| s.remind_fired && *t > now)?;
            Some((n.id.clone(), at))
        })
        .collect()
}

/// The reminder fired and the note has not been opened since: its chip is in
/// the accent colour and the cat holds a clock.
pub fn overdue(s: &Sticky, now: i64) -> bool {
    s.remind_fired && s.remind_at.is_some_and(|t| t <= now)
}

/// How long the one-shot timer waits (ms) for a reminder at `at`, from `now`
/// (unix seconds): never below `USER_TIMER_MINIMUM`, never above
/// `USER_TIMER_MAXIMUM`. A clamped wait just recomputes when it fires.
pub fn wait_ms(at: i64, now: i64) -> u32 {
    let ms = at.saturating_sub(now).max(0).saturating_mul(1000);
    ms.clamp(i64::from(USER_TIMER_MINIMUM), i64::from(USER_TIMER_MAXIMUM)) as u32
}

/// The header chip's text: "09.30" (id) or "09:30" (en) for today, else the
/// short weekday and the time: "Kam 09.00", "Thu 09:00".
pub fn chip_label(lang: Lang, at: NaiveDateTime, today: NaiveDate) -> String {
    let time = i18n::time_hm(lang, at);
    if at.date() == today {
        time
    } else {
        format!("{} {time}", i18n::day_short(lang, at.date()))
    }
}

// ------------------------------------------------------------------ the entry

/// The "Atur sendiri" entry: a day (0 is today), an hour and a minute in
/// 5-minute steps. All the stepping is here; the popover only draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Days from today.
    pub day: i64,
    pub hour: u32,
    /// 0, 5, ... 55.
    pub minute: u32,
}

/// How far ahead the day stepper goes.
pub const MAX_DAYS: i64 = 365;
/// The minute stepper's step.
pub const MINUTE_STEP: u32 = 5;

impl Entry {
    /// Opens 15 minutes ahead, rounded up to the next 5 minutes.
    pub fn from_now(now: NaiveDateTime) -> Entry {
        let at = now + Duration::minutes(15);
        let total = at.time().hour() * 60 + at.time().minute();
        let rounded = total.div_ceil(MINUTE_STEP) * MINUTE_STEP;
        let day = (at.date() - now.date()).num_days() + i64::from(rounded >= 24 * 60);
        let rounded = rounded % (24 * 60);
        Entry {
            day,
            hour: rounded / 60,
            minute: rounded % 60,
        }
    }

    /// The day moved by `by`: never before today, never past `MAX_DAYS`.
    pub fn step_day(self, by: i64) -> Entry {
        Entry {
            day: (self.day + by).clamp(0, MAX_DAYS),
            ..self
        }
    }

    /// The hour moved by `by`, round the clock (it does not touch the day).
    pub fn step_hour(self, by: i32) -> Entry {
        Entry {
            hour: (self.hour as i32 + by).rem_euclid(24) as u32,
            ..self
        }
    }

    /// The minute moved by `by` steps of 5, round the hour (it does not touch
    /// the hour).
    pub fn step_minute(self, by: i32) -> Entry {
        let steps = (60 / MINUTE_STEP) as i32;
        let now = (self.minute / MINUTE_STEP) as i32;
        Entry {
            minute: ((now + by).rem_euclid(steps) as u32) * MINUTE_STEP,
            ..self
        }
    }

    /// The time this entry names, from today's date.
    pub fn at(self, today: NaiveDate) -> NaiveDateTime {
        (today + Duration::days(self.day)).and_time(hm(self.hour, self.minute))
    }

    /// Whether Simpan makes sense: the time is still ahead.
    pub fn is_future(self, now: NaiveDateTime) -> bool {
        self.at(now.date()) > now
    }

    /// The day stepper's label: "Hari ini", "Besok", then "Jum 6 Nov".
    pub fn day_label(self, lang: Lang, today: NaiveDate) -> String {
        match self.day {
            0 => text(lang, Key::StickyDayToday).to_owned(),
            1 => text(lang, Key::StickyDayTomorrow).to_owned(),
            n => {
                let d = today + Duration::days(n);
                format!("{} {}", i18n::day_short(lang, d), i18n::short_date(lang, d))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::sticky::Sticky;

    fn dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    fn note(id: &str, at: Option<i64>, fired: bool) -> Note {
        Note {
            id: id.into(),
            text: "x".into(),
            updated: 1,
            sticky: Some(Sticky {
                remind_at: at,
                remind_fired: fired,
                ..Sticky::default()
            }),
        }
    }

    #[test]
    fn the_quick_choices_count_from_now() {
        let now = dt(2026, 11, 4, 10, 20);
        assert_eq!(Preset::In15.at(now), dt(2026, 11, 4, 10, 35));
        assert_eq!(Preset::In1h.at(now), dt(2026, 11, 4, 11, 20));
        assert_eq!(Preset::Evening.at(now), dt(2026, 11, 4, 17, 0));
        assert_eq!(Preset::Tomorrow9.at(now), dt(2026, 11, 5, 9, 0));
    }

    #[test]
    fn five_pm_is_tomorrow_once_it_has_passed_and_exactly_five_counts_as_passed() {
        assert_eq!(Preset::Evening.at(dt(2026, 11, 4, 16, 59)), dt(2026, 11, 4, 17, 0));
        assert_eq!(Preset::Evening.at(dt(2026, 11, 4, 17, 0)), dt(2026, 11, 5, 17, 0));
        assert_eq!(Preset::Evening.at(dt(2026, 11, 4, 17, 1)), dt(2026, 11, 5, 17, 0));
        assert_eq!(Preset::Evening.at(dt(2026, 11, 4, 23, 59)), dt(2026, 11, 5, 17, 0));
        assert_eq!(Preset::Evening.at(dt(2026, 11, 4, 0, 0)), dt(2026, 11, 4, 17, 0));
    }

    #[test]
    fn the_choices_cross_midnight_and_month_ends() {
        let late = dt(2026, 11, 30, 23, 50);
        assert_eq!(Preset::In15.at(late), dt(2026, 12, 1, 0, 5));
        assert_eq!(Preset::In1h.at(late), dt(2026, 12, 1, 0, 50));
        assert_eq!(Preset::Tomorrow9.at(late), dt(2026, 12, 1, 9, 0));
        // "Tomorrow" after midnight is the next calendar day, not 24 h later.
        assert_eq!(Preset::Tomorrow9.at(dt(2026, 12, 31, 0, 5)), dt(2027, 1, 1, 9, 0));
        // The year's end.
        assert_eq!(Preset::Evening.at(dt(2026, 12, 31, 18, 0)), dt(2027, 1, 1, 17, 0));
    }

    #[test]
    fn local_time_round_trips_through_unix_seconds() {
        let at = dt(2026, 7, 15, 12, 30);
        assert_eq!(from_unix(to_unix(at)), at);
        // Midnight too.
        let m = dt(2026, 7, 16, 0, 0);
        assert_eq!(from_unix(to_unix(m)), m);
        // A later time is a later unix time.
        assert!(to_unix(at) < to_unix(m));
    }

    #[test]
    fn snooze_is_ten_minutes_from_now() {
        assert_eq!(snooze(1_000), 1_600);
        assert_eq!(SNOOZE_SECS, 600);
    }

    #[test]
    fn the_nearest_is_the_earliest_unfired_one() {
        let notes = vec![
            note("a", Some(500), false),
            note("b", Some(300), true),
            note("c", Some(400), false),
            note("d", None, false),
            note("e", Some(100), true),
        ];
        assert_eq!(nearest(&notes), Some(400), "a fired one is not waited for");
        assert_eq!(nearest(&[]), None);
        assert_eq!(nearest(&[note("x", None, false), note("y", Some(1), true)]), None);
        // A note off the desktop has no sticky part, so no reminder.
        let plain = Note {
            id: "p".into(),
            text: "x".into(),
            updated: 1,
            sticky: None,
        };
        assert_eq!(nearest(&[plain]), None);
    }

    #[test]
    fn due_covers_the_ones_missed_while_off() {
        let notes = vec![
            note("soon", Some(1_000), false),
            note("missed", Some(10), false),
            note("fired", Some(10), true),
            note("none", None, false),
            note("exact", Some(500), false),
        ];
        assert_eq!(due(&notes, 500), vec!["missed".to_owned(), "exact".to_owned()]);
        assert_eq!(due(&notes, 5), Vec::<String>::new());
        assert_eq!(due(&notes, 2_000), vec!["soon".to_owned(), "missed".to_owned(), "exact".to_owned()]);
    }

    #[test]
    fn a_fired_reminder_the_clock_went_back_past_is_found() {
        let notes = vec![
            note("back", Some(1_000), true),
            note("past", Some(10), true),
            note("unfired", Some(1_000), false),
            note("none", None, false),
        ];
        assert_eq!(fired_in_future(&notes, 500), vec![("back".to_owned(), 1_000)]);
        assert_eq!(fired_in_future(&notes, 1_000), Vec::<(String, i64)>::new());
    }

    #[test]
    fn overdue_is_fired_and_past() {
        let s = |at, fired| Sticky {
            remind_at: at,
            remind_fired: fired,
            ..Sticky::default()
        };
        assert!(overdue(&s(Some(100), true), 100));
        assert!(overdue(&s(Some(100), true), 500));
        assert!(!overdue(&s(Some(100), false), 500), "not fired yet");
        assert!(!overdue(&s(Some(900), true), 500), "snoozed into the future");
        assert!(!overdue(&s(None, true), 500));
    }

    #[test]
    fn the_wait_is_clamped_to_what_a_timer_takes() {
        assert_eq!(wait_ms(1_060, 1_000), 60_000);
        assert_eq!(wait_ms(1_000, 1_000), USER_TIMER_MINIMUM, "due now: the shortest wait");
        assert_eq!(wait_ms(500, 1_000), USER_TIMER_MINIMUM, "already past");
        assert_eq!(wait_ms(1_000 + 40 * 24 * 3600, 1_000), USER_TIMER_MAXIMUM, "40 days away");
        assert_eq!(wait_ms(i64::MAX, 0), USER_TIMER_MAXIMUM);
        assert_eq!(wait_ms(i64::MIN, i64::MAX), USER_TIMER_MINIMUM);
        assert_eq!(USER_TIMER_MAXIMUM, 0x7FFF_FFFF);
    }

    #[test]
    fn the_chip_shows_the_time_today_and_the_day_otherwise() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        assert_eq!(chip_label(Lang::Id, dt(2026, 11, 4, 17, 0), today), "17.00");
        assert_eq!(chip_label(Lang::En, dt(2026, 11, 4, 17, 0), today), "17:00");
        // 5 Nov 2026 is a Thursday.
        assert_eq!(chip_label(Lang::Id, dt(2026, 11, 5, 9, 0), today), "Kam 09.00");
        assert_eq!(chip_label(Lang::En, dt(2026, 11, 5, 9, 0), today), "Thu 09:00");
        // Midnight: 00:05 tomorrow is not today.
        assert_eq!(chip_label(Lang::En, dt(2026, 11, 5, 0, 5), today), "Thu 00:05");
        // A missed reminder from yesterday names its day.
        assert_eq!(chip_label(Lang::Id, dt(2026, 11, 3, 15, 30), today), "Sel 15.30");
    }

    #[test]
    fn the_entry_opens_a_quarter_hour_ahead_on_a_five_minute_step() {
        let e = Entry::from_now(dt(2026, 11, 4, 10, 21));
        assert_eq!((e.day, e.hour, e.minute), (0, 10, 40), "10:36 rounds up to 10:40");
        let e = Entry::from_now(dt(2026, 11, 4, 10, 20));
        assert_eq!((e.day, e.hour, e.minute), (0, 10, 35));
        // Across midnight it is tomorrow.
        let e = Entry::from_now(dt(2026, 11, 4, 23, 50));
        assert_eq!((e.day, e.hour, e.minute), (1, 0, 5));
        let e = Entry::from_now(dt(2026, 11, 4, 23, 46));
        assert_eq!((e.day, e.hour, e.minute), (1, 0, 5), "00:01 rounds up to 00:05");
        let e = Entry::from_now(dt(2026, 11, 4, 23, 44));
        assert_eq!((e.day, e.hour, e.minute), (1, 0, 0), "23:59 rounds up to midnight");
        let e = Entry::from_now(dt(2026, 11, 4, 23, 40));
        assert_eq!((e.day, e.hour, e.minute), (0, 23, 55));
    }

    #[test]
    fn the_steppers_wrap_and_never_go_before_today() {
        let e = Entry { day: 0, hour: 23, minute: 55 };
        assert_eq!(e.step_hour(1).hour, 0);
        assert_eq!(e.step_hour(-24).hour, 23);
        assert_eq!(Entry { hour: 0, ..e }.step_hour(-1).hour, 23);
        assert_eq!(e.step_minute(1).minute, 0, "55 + 5 wraps in the hour");
        assert_eq!(e.step_minute(1).hour, 23, "and does not carry");
        assert_eq!(Entry { minute: 0, ..e }.step_minute(-1).minute, 55);
        assert_eq!(Entry { minute: 7, ..e }.step_minute(0).minute, 5, "snaps to the step");
        assert_eq!(e.step_day(-1).day, 0);
        assert_eq!(e.step_day(3).day, 3);
        assert_eq!(e.step_day(10_000).day, MAX_DAYS);
    }

    #[test]
    fn the_entry_names_a_time_and_a_label() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        let e = Entry { day: 0, hour: 9, minute: 30 };
        assert_eq!(e.at(today), dt(2026, 11, 4, 9, 30));
        assert_eq!(Entry { day: 2, ..e }.at(today), dt(2026, 11, 6, 9, 30));
        assert_eq!(e.day_label(Lang::Id, today), "Hari ini");
        assert_eq!(e.day_label(Lang::En, today), "Today");
        assert_eq!(Entry { day: 1, ..e }.day_label(Lang::Id, today), "Besok");
        assert_eq!(Entry { day: 1, ..e }.day_label(Lang::En, today), "Tomorrow");
        // 6 Nov 2026 is a Friday.
        assert_eq!(Entry { day: 2, ..e }.day_label(Lang::Id, today), "Jum 6 Nov");
        assert_eq!(Entry { day: 2, ..e }.day_label(Lang::En, today), "Fri 6 Nov");
        let now = dt(2026, 11, 4, 9, 30);
        assert!(!e.is_future(now), "the same minute is not ahead");
        assert!(Entry { minute: 35, ..e }.is_future(now));
        assert!(Entry { day: 1, hour: 0, minute: 0 }.is_future(now));
    }
}
