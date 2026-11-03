use chrono::{Datelike, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

use crate::config::backup_path;
use crate::i18n::{self, Key, Lang, text};
use crate::widgets::sticky::{self, Sticky};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Note {
    pub id: String,
    pub text: String,
    /// Unix seconds of the last edit.
    pub updated: i64,
    /// Present while the note is stuck to the desktop (`widgets`). Read
    /// leniently: a malformed one never makes the file unreadable.
    #[serde(
        default,
        deserialize_with = "sticky::deserialize_lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub sticky: Option<Sticky>,
}

/// "hari ini 09.12", "kemarin", "23 Okt", or "23 Okt 2025" in another year.
/// The Notes page and the desktop note's footer share it.
pub fn note_time_label(lang: Lang, updated: NaiveDateTime, today: NaiveDate) -> String {
    let d = updated.date();
    if d == today {
        format!("{} {}", text(lang, Key::Today), i18n::time_hm(lang, updated))
    } else if Some(d) == today.pred_opt() {
        text(lang, Key::Yesterday).into()
    } else if d.year() == today.year() {
        i18n::short_date(lang, d)
    } else {
        format!("{} {}", i18n::short_date(lang, d), d.year())
    }
}

/// Plain-text notes kept in a local JSON file. Changes stay in memory until `save`.
pub struct NotesStore {
    path: PathBuf,
    /// Newest `updated` first.
    notes: Vec<Note>,
    counter: u64,
    /// The file existed but could not be read or parsed. It is moved aside
    /// (`notes.json.bak`) before the first save instead of being overwritten.
    keep_old: bool,
}

/// The notes in a file read as `read`, and whether the file must be kept
/// aside: it exists but is unreadable or not a notes list. A missing file is
/// simply empty.
fn load(read: io::Result<String>) -> (Vec<Note>, bool) {
    match read {
        Ok(text) => {
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            match serde_json::from_str(text) {
                Ok(notes) => (notes, false),
                Err(_) => (Vec::new(), true),
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => (Vec::new(), false),
        Err(_) => (Vec::new(), true),
    }
}

impl NotesStore {
    /// A missing, unreadable or malformed file yields an empty store. The file itself
    /// is left untouched until the next `save`, which first renames an unreadable or
    /// malformed one to `notes.json.bak`.
    pub fn open(path: PathBuf) -> Self {
        let (mut notes, keep_old) = load(std::fs::read_to_string(&path));
        notes.sort_by_key(|n| std::cmp::Reverse(n.updated));
        NotesStore {
            path,
            notes,
            counter: 0,
            keep_old,
        }
    }

    /// `%APPDATA%\Vale\notes.json`, next to the config file.
    pub fn default_path() -> PathBuf {
        crate::config::path().with_file_name("notes.json")
    }

    /// True while an existing notes file that could not be read or parsed
    /// waits to be moved aside by the next `save`.
    pub fn load_failed(&self) -> bool {
        self.keep_old
    }

    pub fn list(&self) -> &[Note] {
        &self.notes
    }

    pub fn get(&self, id: &str) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    /// Creates or edits a note and returns its id. Text that is empty after trimming
    /// deletes the note instead (or creates nothing) and returns `None`. An unknown
    /// `id` is treated as a new note and gets a fresh id.
    pub fn upsert(&mut self, id: Option<&str>, text: &str, now: i64) -> Option<String> {
        let existing = id.and_then(|id| self.notes.iter().position(|n| n.id == id));
        if text.trim().is_empty() {
            if let Some(i) = existing {
                self.notes.remove(i);
            }
            return None;
        }
        let mut note = match existing {
            Some(i) => self.notes.remove(i),
            None => Note {
                id: self.next_id(now),
                text: String::new(),
                updated: now,
                sticky: None,
            },
        };
        note.text = text.to_string();
        note.updated = now;
        let id = note.id.clone();
        self.place_by_time(note);
        Some(id)
    }

    /// Puts `note` ahead of every note with the same or an older timestamp.
    fn place_by_time(&mut self, note: Note) {
        let at = self.notes.partition_point(|n| n.updated > note.updated);
        self.notes.insert(at, note);
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.notes.len();
        self.notes.retain(|n| n.id != id);
        self.notes.len() != before
    }

    /// The desktop window's edit: the text of note `id`. A note on the
    /// desktop keeps an empty text (a fresh one starts empty); any other note
    /// follows `upsert`, where empty text deletes it. False when the note is
    /// gone (a plain note emptied is deleted, and the call still counts).
    /// Unchanged text touches nothing.
    pub fn set_text(&mut self, id: &str, text: &str, now: i64) -> bool {
        let Some(i) = self.notes.iter().position(|n| n.id == id) else {
            return false;
        };
        if self.notes[i].text == text {
            return true;
        }
        if self.notes[i].sticky.is_none() {
            self.upsert(Some(id), text, now);
            return true;
        }
        let mut note = self.notes.remove(i);
        note.text = text.to_owned();
        note.updated = now;
        self.place_by_time(note);
        true
    }

    /// Puts note `id` on the desktop (`Some`) or takes it off (`None`; an
    /// empty note is deleted then, since nothing is left of it). Putting one
    /// more on a full desktop is refused. True when something changed.
    pub fn set_sticky(&mut self, id: &str, value: Option<Sticky>) -> bool {
        let count = self.sticky_count();
        let Some(i) = self.notes.iter().position(|n| n.id == id) else {
            return false;
        };
        let was = self.notes[i].sticky.is_some();
        match value {
            Some(s) => {
                if !was && !sticky::can_add(count) {
                    return false;
                }
                if self.notes[i].sticky.as_ref() == Some(&s) {
                    return false;
                }
                self.notes[i].sticky = Some(s);
                true
            }
            None if !was => false,
            None => {
                if self.notes[i].text.trim().is_empty() {
                    self.notes.remove(i);
                } else {
                    self.notes[i].sticky = None;
                }
                true
            }
        }
    }

    /// A new, empty note on the desktop; `None` on a full desktop.
    pub fn create_sticky(&mut self, sticky: Sticky, now: i64) -> Option<String> {
        if !sticky::can_add(self.sticky_count()) {
            return None;
        }
        let note = Note {
            id: self.next_id(now),
            text: String::new(),
            updated: now,
            sticky: Some(sticky),
        };
        let id = note.id.clone();
        self.place_by_time(note);
        Some(id)
    }

    /// How many notes are on the desktop.
    pub fn sticky_count(&self) -> usize {
        self.notes.iter().filter(|n| n.sticky.is_some()).count()
    }

    /// Writes pretty JSON to a sibling `.tmp` file, then renames it over the real
    /// file so a crash mid-write never leaves a truncated store. A file that could
    /// not be loaded is renamed to a backup first; if that fails, nothing is written.
    pub fn save(&mut self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if self.keep_old {
            match std::fs::rename(&self.path, backup_path(&self.path)) {
                Ok(()) => {}
                // Gone meanwhile: nothing left to keep.
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            self.keep_old = false;
        }
        let json = serde_json::to_string_pretty(&self.notes).map_err(io::Error::other)?;
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)
    }

    /// `n{now}-{counter}`, skipping ids already present: a store reopened within
    /// the same second restarts its counter at zero.
    fn next_id(&mut self, now: i64) -> String {
        loop {
            let id = format!("n{now}-{}", self.counter);
            self.counter += 1;
            if self.get(&id).is_none() {
                return id;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh directory under the system temp dir; removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static SEQ: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "vale-notes-{name}-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join("notes.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_file_is_empty() {
        let tmp = TempDir::new("missing");
        let store = NotesStore::open(tmp.file());
        assert!(store.list().is_empty());
        assert!(!store.load_failed(), "a missing file is no failure");
        assert!(!tmp.file().exists());
    }

    #[test]
    fn upsert_new_then_edit_keeps_id() {
        let tmp = TempDir::new("edit");
        let mut store = NotesStore::open(tmp.file());
        let id = store.upsert(None, "buy milk", 100).unwrap();
        assert!(id.starts_with("n100-"), "id was {id}");
        let again = store.upsert(Some(&id), "buy oat milk", 200).unwrap();
        assert_eq!(again, id);
        assert_eq!(store.list().len(), 1);
        let note = store.get(&id).unwrap();
        assert_eq!(note.text, "buy oat milk");
        assert_eq!(note.updated, 200);
        // Many new notes in the same second still get distinct ids.
        let a = store.upsert(None, "a", 300).unwrap();
        let b = store.upsert(None, "b", 300).unwrap();
        let c = store.upsert(None, "c", 300).unwrap();
        assert!(a != b && b != c && a != c);
        assert_eq!(store.list().len(), 4);
    }

    #[test]
    fn list_is_newest_first() {
        let tmp = TempDir::new("order");
        let mut store = NotesStore::open(tmp.file());
        let old = store.upsert(None, "old", 10).unwrap();
        let new = store.upsert(None, "new", 30).unwrap();
        let mid = store.upsert(None, "mid", 20).unwrap();
        let ids: Vec<&str> = store.list().iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec![new.as_str(), mid.as_str(), old.as_str()]);
        // Editing moves a note to the top.
        store.upsert(Some(&old), "old, edited", 40);
        assert_eq!(store.list()[0].id, old);
    }

    #[test]
    fn empty_text_deletes_existing() {
        let tmp = TempDir::new("delete");
        let mut store = NotesStore::open(tmp.file());
        let id = store.upsert(None, "temporary", 1).unwrap();
        assert_eq!(store.upsert(Some(&id), "   \n\t ", 2), None);
        assert!(store.get(&id).is_none());
        assert!(store.list().is_empty());
        // Explicit delete reports whether anything was removed.
        let other = store.upsert(None, "keep", 3).unwrap();
        assert!(store.delete(&other));
        assert!(!store.delete(&other));
    }

    #[test]
    fn empty_text_creates_nothing() {
        let tmp = TempDir::new("nothing");
        let mut store = NotesStore::open(tmp.file());
        assert_eq!(store.upsert(None, "", 1), None);
        assert_eq!(store.upsert(None, "  \r\n ", 1), None);
        assert_eq!(store.upsert(Some("n0-unknown"), " ", 1), None);
        assert!(store.list().is_empty());
    }

    #[test]
    fn save_and_reopen_round_trip() {
        let tmp = TempDir::new("roundtrip");
        let mut store = NotesStore::open(tmp.file());
        let a = store.upsert(None, "first", 5).unwrap();
        let b = store
            .upsert(None, "second line\nwith \"quotes\"", 9)
            .unwrap();
        store.save().unwrap();
        assert!(!tmp.file().with_extension("json.tmp").exists());
        assert!(!tmp.file().with_extension("tmp").exists());

        let reopened = NotesStore::open(tmp.file());
        assert_eq!(reopened.list(), store.list());
        assert_eq!(reopened.get(&a).unwrap().text, "first");
        assert_eq!(reopened.get(&b).unwrap().updated, 9);
        // A reopened store never reuses an id from the same second.
        let mut reopened = reopened;
        let c = reopened.upsert(None, "third", 5).unwrap();
        assert!(c != a && c != b, "id {c} collided");
        reopened.delete(&c);
        // Saving again overwrites the existing file.
        reopened.delete(&a);
        reopened.save().unwrap();
        assert_eq!(NotesStore::open(tmp.file()).list().len(), 1);
    }

    #[test]
    fn corrupt_file_is_empty_not_panic() {
        let tmp = TempDir::new("corrupt");
        std::fs::write(tmp.file(), "{not json at all").unwrap();
        let mut store = NotesStore::open(tmp.file());
        assert!(store.list().is_empty());
        assert!(store.load_failed(), "reported, so the island can say so");
        // The corrupt file is left alone until an explicit save.
        store.upsert(None, "fresh", 1);
        assert_eq!(
            std::fs::read_to_string(tmp.file()).unwrap(),
            "{not json at all"
        );
        store.save().unwrap();
        assert!(!store.load_failed(), "moved aside");
        assert_eq!(NotesStore::open(tmp.file()).list().len(), 1);
        // ...which keeps the corrupt file as a backup instead of losing it.
        let bak = tmp.0.join("notes.json.bak");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "{not json at all");
        // Only once: the next save just replaces the good file.
        store.upsert(None, "second", 2);
        store.save().unwrap();
        assert!(!tmp.0.join("notes.json.bak2").exists());
        assert_eq!(NotesStore::open(tmp.file()).list().len(), 2);
    }

    #[test]
    fn load_decides_what_to_keep() {
        let missing = io::Error::from(io::ErrorKind::NotFound);
        assert_eq!(load(Err(missing)), (vec![], false));
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(load(Err(denied)), (vec![], true), "unreadable is kept");
        assert_eq!(load(Ok("{not json".into())), (vec![], true));
        assert_eq!(load(Ok(String::new())), (vec![], true));
        assert_eq!(load(Ok("\u{feff}[]".into())), (vec![], false));
        let one = r#"[{"id":"n1-0","text":"a","updated":1}]"#;
        let (notes, keep) = load(Ok(one.into()));
        assert_eq!((notes.len(), keep), (1, false));
    }

    #[test]
    fn existing_backups_are_not_overwritten() {
        let tmp = TempDir::new("backups");
        std::fs::write(tmp.0.join("notes.json.bak"), "older").unwrap();
        std::fs::write(tmp.file(), "garbage").unwrap();
        let mut store = NotesStore::open(tmp.file());
        store.save().unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.0.join("notes.json.bak")).unwrap(),
            "older"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.0.join("notes.json.bak2")).unwrap(),
            "garbage"
        );
    }

    fn stuck(x: f32) -> Sticky {
        Sticky {
            rect: crate::widgets::sticky::DipRect::new(x, 16.0, 240.0, 208.0),
            ..Sticky::default()
        }
    }

    #[test]
    fn a_sticky_note_round_trips_through_the_file() {
        let tmp = TempDir::new("sticky-roundtrip");
        let mut store = NotesStore::open(tmp.file());
        let plain = store.upsert(None, "plain", 5).unwrap();
        let id = store.create_sticky(stuck(24.0), 9).unwrap();
        store.set_text(&id, "Belanja\n- [ ] bata", 10);
        store.save().unwrap();
        let reopened = NotesStore::open(tmp.file());
        assert_eq!(reopened.list(), store.list());
        let note = reopened.get(&id).unwrap();
        assert_eq!(note.sticky, Some(stuck(24.0)));
        assert_eq!(note.text, "Belanja\n- [ ] bata");
        assert_eq!(reopened.get(&plain).unwrap().sticky, None);
        // A note off the desktop writes no sticky key at all.
        let json = std::fs::read_to_string(tmp.file()).unwrap();
        assert_eq!(json.matches("\"sticky\"").count(), 1, "{json}");
    }

    #[test]
    fn an_old_file_without_sticky_loads_unchanged_and_saves_back_without_it() {
        let old = r#"[
  {"id": "n100-0", "text": "beli kopi", "updated": 100},
  {"id": "n50-0", "text": "telp Pak Budi\njam 3", "updated": 50}
]"#;
        let tmp = TempDir::new("old-file");
        std::fs::write(tmp.file(), old).unwrap();
        let mut store = NotesStore::open(tmp.file());
        assert!(!store.load_failed());
        assert_eq!(store.list().len(), 2);
        assert!(store.list().iter().all(|n| n.sticky.is_none()));
        assert_eq!(store.list()[0].text, "beli kopi");
        store.save().unwrap();
        let saved = std::fs::read_to_string(tmp.file()).unwrap();
        assert!(!saved.contains("sticky"), "{saved}");
        let again = NotesStore::open(tmp.file());
        assert_eq!(again.list(), store.list());
    }

    #[test]
    fn a_malformed_sticky_keeps_the_file_loading() {
        let bad = r#"[
  {"id": "n1-0", "text": "a", "updated": 3, "sticky": "oops"},
  {"id": "n2-0", "text": "b", "updated": 2, "sticky": {"paper": 9, "rect": {"w": 3, "h": 100000}, "pinned": "x"}},
  {"id": "n3-0", "text": "c", "updated": 1, "sticky": null}
]"#;
        let tmp = TempDir::new("bad-sticky");
        std::fs::write(tmp.file(), bad).unwrap();
        let store = NotesStore::open(tmp.file());
        assert!(!store.load_failed(), "the file is still good");
        assert_eq!(store.list().len(), 3);
        assert_eq!(store.get("n1-0").unwrap().sticky, None);
        let s = store.get("n2-0").unwrap().sticky.clone().unwrap();
        assert_eq!(s.paper, crate::widgets::sticky::Paper::Krem);
        assert_eq!(s.rect.w, crate::widgets::sticky::MIN_W, "clamped to the minimum");
        assert_eq!(s.rect.h, crate::widgets::sticky::MAX_H);
        assert!(!s.pinned);
        assert_eq!(store.get("n3-0").unwrap().sticky, None);
    }

    #[test]
    fn an_empty_sticky_note_survives_set_text_but_a_plain_one_does_not() {
        let tmp = TempDir::new("empty-sticky");
        let mut store = NotesStore::open(tmp.file());
        let id = store.create_sticky(stuck(0.0), 1).unwrap();
        assert_eq!(store.get(&id).unwrap().text, "", "a fresh note starts empty");
        assert!(store.set_text(&id, "hello", 2));
        assert!(store.set_text(&id, "", 3));
        assert_eq!(store.get(&id).unwrap().text, "");
        assert_eq!(store.get(&id).unwrap().updated, 3);
        // The Notes editor's empty save still deletes it.
        assert_eq!(store.upsert(Some(&id), "  ", 4), None);
        assert!(store.get(&id).is_none());
        // A plain note emptied by set_text is deleted, as upsert does.
        let plain = store.upsert(None, "x", 5).unwrap();
        assert!(store.set_text(&plain, "", 6));
        assert!(store.get(&plain).is_none());
        assert!(!store.set_text("n0-missing", "x", 7));
    }

    #[test]
    fn unsticking_keeps_a_note_with_text_and_drops_an_empty_one() {
        let tmp = TempDir::new("unstick");
        let mut store = NotesStore::open(tmp.file());
        let kept = store.create_sticky(stuck(0.0), 1).unwrap();
        store.set_text(&kept, "keep me", 2);
        let empty = store.create_sticky(stuck(8.0), 3).unwrap();
        assert!(store.set_sticky(&kept, None));
        assert_eq!(store.get(&kept).unwrap().text, "keep me");
        assert_eq!(store.get(&kept).unwrap().sticky, None);
        assert!(store.set_sticky(&empty, None));
        assert!(store.get(&empty).is_none(), "nothing left of an empty note");
        assert!(!store.set_sticky(&kept, None), "already off");
        // And it can go back on.
        assert!(store.set_sticky(&kept, Some(stuck(16.0))));
        assert_eq!(store.sticky_count(), 1);
    }

    #[test]
    fn at_most_twenty_notes_are_on_the_desktop() {
        let tmp = TempDir::new("cap");
        let mut store = NotesStore::open(tmp.file());
        for i in 0..20 {
            assert!(store.create_sticky(stuck(i as f32), 1).is_some(), "{i}");
        }
        assert_eq!(store.sticky_count(), 20);
        assert_eq!(store.create_sticky(stuck(0.0), 2), None);
        let plain = store.upsert(None, "plain", 3).unwrap();
        assert!(!store.set_sticky(&plain, Some(stuck(0.0))), "the 21st is refused");
        assert_eq!(store.get(&plain).unwrap().sticky, None);
        // Changing one that is on is fine at the cap.
        let first = store.list().iter().find_map(|n| n.sticky.as_ref().map(|_| n.id.clone())).unwrap();
        assert!(store.set_sticky(&first, Some(stuck(99.0))));
        // Removing one makes room again.
        assert!(store.set_sticky(&first, None));
        assert!(store.set_sticky(&plain, Some(stuck(0.0))));
    }

    #[test]
    fn moving_a_sticky_does_not_change_the_edit_time() {
        let tmp = TempDir::new("move");
        let mut store = NotesStore::open(tmp.file());
        let id = store.create_sticky(stuck(0.0), 1).unwrap();
        store.set_text(&id, "t", 5);
        store.set_sticky(&id, Some(stuck(40.0)));
        assert_eq!(store.get(&id).unwrap().updated, 5);
    }

    #[test]
    fn the_time_label_words_the_day() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        let at = |d: NaiveDate, h, m| d.and_hms_opt(h, m, 0).unwrap();
        assert_eq!(note_time_label(Lang::Id, at(today, 9, 12), today), "hari ini 09.12");
        assert_eq!(note_time_label(Lang::En, at(today, 9, 12), today), "Today 09:12");
        let yesterday = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
        assert_eq!(note_time_label(Lang::Id, at(yesterday, 23, 0), today), "kemarin");
    }

    #[test]
    fn good_file_is_never_backed_up() {
        let tmp = TempDir::new("nobackup");
        let mut store = NotesStore::open(tmp.file());
        store.upsert(None, "a", 1);
        store.save().unwrap();
        let mut store = NotesStore::open(tmp.file());
        store.upsert(None, "b", 2);
        store.save().unwrap();
        assert!(!tmp.0.join("notes.json.bak").exists());
        assert_eq!(NotesStore::open(tmp.file()).list().len(), 2);
    }
}
