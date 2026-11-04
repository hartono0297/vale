//! The note templates of "Catatan tempel": what a new note starts with.
//! Plain text in the note's own format (`\n` breaks, the first line is the
//! title, `- [ ] ` is an item), so the Notes page reads them like anything
//! typed. Pure and tested without a window.

use chrono::{NaiveDate, Weekday};

use crate::i18n::{self, Key, Lang, text};

/// A way to start a note. A left click on a header's (+) is `Kosong`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Template {
    /// Empty: the plain new note.
    #[default]
    Kosong,
    /// A shopping list.
    Belanja,
    /// Today's date as the title, three items.
    HariIni,
    /// A meeting: agenda, decisions, follow-up.
    Rapat,
    /// A title only.
    Ide,
    /// Monday to Friday as items.
    Mingguan,
}

impl Template {
    pub const ALL: [Template; 6] = [
        Template::Kosong,
        Template::Belanja,
        Template::HariIni,
        Template::Rapat,
        Template::Ide,
        Template::Mingguan,
    ];

    /// The menu entry's label key.
    pub fn label_key(self) -> Key {
        match self {
            Template::Kosong => Key::TplKosong,
            Template::Belanja => Key::TplBelanja,
            Template::HariIni => Key::TplHariIni,
            Template::Rapat => Key::TplRapat,
            Template::Ide => Key::TplIde,
            Template::Mingguan => Key::TplMingguan,
        }
    }
}

const ITEM: &str = "- [ ] ";

fn items(n: usize) -> impl Iterator<Item = String> {
    std::iter::repeat_n(ITEM.to_owned(), n)
}

/// The text `t` starts a note with, in `lang`; `today` names the day of "Hari
/// ini". The first line is the title.
pub fn text_of(lang: Lang, t: Template, today: NaiveDate) -> String {
    let lines: Vec<String> = match t {
        Template::Kosong => Vec::new(),
        Template::Belanja => std::iter::once(text(lang, Key::TplBelanja).to_owned())
            .chain(items(3))
            .collect(),
        Template::HariIni => std::iter::once(i18n::day_title_short(lang, today))
            .chain(items(3))
            .collect(),
        Template::Rapat => [
            text(lang, Key::TplRapat),
            text(lang, Key::TplAgenda),
            text(lang, Key::TplDecisions),
            text(lang, Key::TplFollowUp),
        ]
        .into_iter()
        .map(str::to_owned)
        .chain(items(1))
        .collect(),
        Template::Ide => vec![text(lang, Key::TplIde).to_owned()],
        Template::Mingguan => std::iter::once(text(lang, Key::TplMingguan).to_owned())
            .chain(
                [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri]
                    .into_iter()
                    .map(|d| format!("{ITEM}{}", i18n::weekday_long(lang, d))),
            )
            .collect(),
    };
    lines.join("\n")
}

/// Where the caret goes: the end of the first line (UTF-16 units, the
/// indices the notes use), or 0 for an empty text.
pub fn caret_of(text: &str) -> usize {
    text.split('\n').next().map_or(0, |l| l.encode_utf16().count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::NotesStore;
    use chrono::Datelike;
    use crate::widgets::checklist;
    use crate::widgets::sticky::Sticky;

    fn wednesday() -> NaiveDate {
        let d = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        assert_eq!(d.weekday(), Weekday::Wed);
        d
    }

    #[test]
    fn blank_is_empty_and_ide_is_a_title_only() {
        for lang in [Lang::Id, Lang::En] {
            assert_eq!(text_of(lang, Template::Kosong, wednesday()), "");
        }
        assert_eq!(text_of(Lang::Id, Template::Ide, wednesday()), "Ide");
        assert_eq!(text_of(Lang::En, Template::Ide, wednesday()), "Idea");
    }

    #[test]
    fn shopping_is_a_title_and_three_empty_boxes() {
        assert_eq!(
            text_of(Lang::Id, Template::Belanja, wednesday()),
            "Belanja\n- [ ] \n- [ ] \n- [ ] "
        );
        assert_eq!(
            text_of(Lang::En, Template::Belanja, wednesday()),
            "Shopping\n- [ ] \n- [ ] \n- [ ] "
        );
    }

    #[test]
    fn today_is_the_long_date_and_three_boxes() {
        assert_eq!(
            text_of(Lang::Id, Template::HariIni, wednesday()),
            "Rabu, 4 Nov\n- [ ] \n- [ ] \n- [ ] "
        );
        assert_eq!(
            text_of(Lang::En, Template::HariIni, wednesday()),
            "Wednesday, 4 Nov\n- [ ] \n- [ ] \n- [ ] "
        );
        // Another day of the week and month.
        let friday = NaiveDate::from_ymd_opt(2026, 12, 25).unwrap();
        assert!(text_of(Lang::Id, Template::HariIni, friday).starts_with("Jumat, 25 Des\n"));
    }

    #[test]
    fn a_meeting_has_its_three_lines_and_a_box() {
        assert_eq!(
            text_of(Lang::Id, Template::Rapat, wednesday()),
            "Rapat\nAgenda\nKeputusan\nTindak lanjut\n- [ ] "
        );
        assert_eq!(
            text_of(Lang::En, Template::Rapat, wednesday()),
            "Meeting\nAgenda\nDecisions\nFollow-up\n- [ ] "
        );
    }

    #[test]
    fn the_week_is_monday_to_friday_as_boxes() {
        assert_eq!(
            text_of(Lang::Id, Template::Mingguan, wednesday()),
            "Mingguan\n- [ ] Senin\n- [ ] Selasa\n- [ ] Rabu\n- [ ] Kamis\n- [ ] Jumat"
        );
        assert_eq!(
            text_of(Lang::En, Template::Mingguan, wednesday()),
            "This week\n- [ ] Monday\n- [ ] Tuesday\n- [ ] Wednesday\n- [ ] Thursday\n- [ ] Friday"
        );
    }

    #[test]
    fn every_template_reads_back_as_a_title_and_boxes() {
        for lang in [Lang::Id, Lang::En] {
            for t in Template::ALL {
                let s = text_of(lang, t, wednesday());
                if t == Template::Kosong {
                    assert!(s.is_empty());
                    continue;
                }
                assert!(!checklist::title_of(&s).is_empty(), "{t:?} {lang:?}");
                let (done, total) = checklist::counts(&s);
                assert_eq!(done, 0);
                let want = match t {
                    Template::Belanja | Template::HariIni => 3,
                    Template::Rapat => 1,
                    Template::Mingguan => 5,
                    _ => 0,
                };
                assert_eq!(total, want, "{t:?} {lang:?}");
                assert!(!s.contains('\r'), "model text has \\n breaks only");
            }
        }
    }

    #[test]
    fn the_caret_goes_to_the_end_of_the_first_line() {
        assert_eq!(caret_of(""), 0);
        assert_eq!(caret_of("Ide"), 3);
        assert_eq!(caret_of("Belanja\n- [ ] \n- [ ] "), 7);
        assert_eq!(caret_of("Rabu, 4 Nov\n- [ ] "), 11);
        // UTF-16 units: an emoji is two.
        assert_eq!(caret_of("a\u{1F600}\nb"), 3);
    }

    #[test]
    fn each_template_survives_the_store_save_and_load() {
        let dir = std::env::temp_dir().join(format!("vale-templates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("notes.json");
        let mut store = NotesStore::open(path.clone());
        let mut made = Vec::new();
        for lang in [Lang::Id, Lang::En] {
            for t in Template::ALL {
                let s = text_of(lang, t, wednesday());
                let id = store.create_sticky(Sticky::default(), 100).expect("room");
                // A desktop note keeps even an empty text.
                assert!(store.set_text(&id, &s, 100));
                made.push((id, s));
            }
            // Free the desktop for the next language (the cap is 20).
            for (id, _) in std::mem::take(&mut made) {
                let _ = store.set_sticky(&id, None);
            }
        }
        // Once more, kept: save, load, compare.
        for t in Template::ALL {
            let s = text_of(Lang::Id, t, wednesday());
            let id = store.create_sticky(Sticky::default(), 200).expect("room");
            store.set_text(&id, &s, 200);
            made.push((id, s));
        }
        store.save().expect("saved");
        let back = NotesStore::open(path);
        for (id, s) in &made {
            let note = back.get(id).expect("the note is back");
            assert_eq!(&note.text, s);
            assert!(note.sticky.is_some());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
