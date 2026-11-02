//! "baru saja", "5 mnt", "14.05", "Kemarin", "28 Okt": how old an entry is,
//! worked out when it is drawn (nothing ticks).

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};

use crate::i18n::{self, Key, Lang, text};

/// How long ago `at` (unix seconds) was, as `now` sees it. Under a minute, or
/// in the future: "baru saja". Under an hour: minutes. The same local day:
/// the time. The day before: "Kemarin". Older: "28 Okt", with the year when
/// it is not `now`'s.
pub fn relative_time(at: i64, now: DateTime<Local>, lang: Lang) -> String {
    let age = now.timestamp() - at;
    if age < 60 {
        return text(lang, Key::NotifJustNow).to_owned();
    }
    if age < 3600 {
        return i18n::duration_label(lang, age / 60);
    }
    let Some(then) = Local.timestamp_opt(at, 0).single() else {
        return text(lang, Key::NotifJustNow).to_owned();
    };
    let (day, today) = (then.date_naive(), now.date_naive());
    if day == today {
        return i18n::time_hm(lang, then.naive_local());
    }
    if today.pred_opt() == Some(day) {
        return text(lang, Key::NotifYesterday).to_owned();
    }
    let date = i18n::short_date(lang, day);
    if day.year() == today.year() {
        date
    } else {
        format!("{date} {}", day.year())
    }
}

/// The longest line each of `relative_time`'s rules writes in `lang`, with
/// `year` standing for a past year: "baru saja", "59 mnt", "23.59",
/// "Kemarin", and the 28th of every month with the year ("28 Okt 2025"). The
/// Notifikasi time column is as wide as the widest of these.
pub fn longest_samples(lang: Lang, year: i32) -> Vec<String> {
    let mut out = vec![
        text(lang, Key::NotifJustNow).to_owned(),
        i18n::duration_label(lang, 59),
        i18n::time_hm(
            lang,
            NaiveDate::from_ymd_opt(2000, 1, 1)
                .and_then(|d| d.and_hms_opt(23, 59, 0))
                .expect("a valid time"),
        ),
        text(lang, Key::NotifYesterday).to_owned(),
    ];
    out.extend((1..=12).filter_map(|month| {
        let day = NaiveDate::from_ymd_opt(year, month, 28)?;
        Some(format!("{} {year}", i18n::short_date(lang, day)))
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, m, d, h, min, s)
            .single()
            .expect("a time that exists")
    }

    fn at(t: DateTime<Local>) -> i64 {
        t.timestamp()
    }

    #[test]
    fn under_a_minute_and_the_future_are_just_now() {
        let now = local(2026, 10, 31, 14, 30, 0);
        for secs in [0, 1, 59] {
            assert_eq!(relative_time(at(now) - secs, now, Lang::Id), "baru saja");
            assert_eq!(relative_time(at(now) - secs, now, Lang::En), "just now");
        }
        assert_eq!(relative_time(at(now) + 500, now, Lang::Id), "baru saja");
        assert_eq!(
            relative_time(at(now) + 86_400 * 3, now, Lang::En),
            "just now"
        );
    }

    #[test]
    fn under_an_hour_counts_minutes() {
        let now = local(2026, 10, 31, 14, 30, 0);
        assert_eq!(relative_time(at(now) - 60, now, Lang::Id), "1 mnt");
        assert_eq!(relative_time(at(now) - 5 * 60 - 40, now, Lang::Id), "5 mnt");
        assert_eq!(
            relative_time(at(now) - 59 * 60 - 59, now, Lang::En),
            "59 min"
        );
    }

    #[test]
    fn the_same_day_reads_the_time_in_each_languages_separator() {
        let now = local(2026, 10, 31, 14, 30, 0);
        let then = local(2026, 10, 31, 9, 5, 0);
        assert_eq!(relative_time(at(then), now, Lang::Id), "09.05");
        assert_eq!(relative_time(at(then), now, Lang::En), "09:05");
        // An hour and a bit ago, still today.
        let t = local(2026, 10, 31, 13, 0, 0);
        assert_eq!(relative_time(at(t), now, Lang::En), "13:00");
    }

    #[test]
    fn the_day_before_is_kemarin_whatever_the_hour() {
        let now = local(2026, 11, 1, 0, 30, 0);
        let late = local(2026, 10, 31, 23, 50, 0);
        // 40 minutes: the minutes rule wins while it applies.
        assert_eq!(relative_time(at(late), now, Lang::Id), "40 mnt");
        let evening = local(2026, 10, 31, 20, 0, 0);
        assert_eq!(relative_time(at(evening), now, Lang::Id), "Kemarin");
        assert_eq!(relative_time(at(evening), now, Lang::En), "Yesterday");
        let morning = local(2026, 10, 31, 6, 0, 0);
        assert_eq!(relative_time(at(morning), now, Lang::En), "Yesterday");
    }

    #[test]
    fn older_days_are_a_short_date_with_the_year_only_when_it_differs() {
        let now = local(2026, 10, 31, 14, 30, 0);
        let earlier = local(2026, 10, 28, 9, 0, 0);
        assert_eq!(relative_time(at(earlier), now, Lang::Id), "28 Okt");
        assert_eq!(relative_time(at(earlier), now, Lang::En), "28 Oct");
        let may = local(2026, 5, 2, 9, 0, 0);
        assert_eq!(relative_time(at(may), now, Lang::Id), "2 Mei");
        assert_eq!(relative_time(at(may), now, Lang::En), "2 May");
        let last_year = local(2025, 12, 30, 9, 0, 0);
        assert_eq!(relative_time(at(last_year), now, Lang::Id), "30 Des 2025");
        assert_eq!(relative_time(at(last_year), now, Lang::En), "30 Dec 2025");
        // New Year: Dec 31 is yesterday on Jan 1, not a dated line.
        let jan = local(2027, 1, 1, 10, 0, 0);
        let dec31 = local(2026, 12, 31, 10, 0, 0);
        assert_eq!(relative_time(at(dec31), jan, Lang::En), "Yesterday");
        let dec30 = local(2026, 12, 30, 10, 0, 0);
        assert_eq!(relative_time(at(dec30), jan, Lang::En), "30 Dec 2026");
    }

    #[test]
    fn longest_samples_cover_every_rule_and_the_dated_line_with_a_year() {
        let now = local(2026, 10, 31, 14, 30, 0);
        for lang in [Lang::Id, Lang::En] {
            let samples = longest_samples(lang, 2025);
            assert_eq!(samples.len(), 16);
            let last_year = local(2025, 10, 28, 9, 0, 0);
            assert!(samples.contains(&relative_time(at(last_year), now, lang)));
            assert!(samples.contains(&relative_time(at(now) - 59 * 60, now, lang)));
            assert!(samples.contains(&relative_time(at(now) - 10, now, lang)));
            let evening = local(2026, 10, 30, 20, 0, 0);
            assert!(samples.contains(&relative_time(at(evening), now, lang)));
        }
        assert!(longest_samples(Lang::Id, 2025).contains(&"28 Okt 2025".to_owned()));
        assert!(longest_samples(Lang::En, 2025).contains(&"28 Oct 2025".to_owned()));
        assert!(longest_samples(Lang::Id, 2025).contains(&"23.59".to_owned()));
    }
}
