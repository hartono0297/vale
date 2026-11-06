//! The history itself: newest first, capped, with read marks. Pure data.

use crate::sources::notifications::is_muted;

pub const MIN_CAP: u32 = 10;
pub const MAX_CAP: u32 = 200;
pub const DEFAULT_CAP: u32 = 50;
pub const CAP_STEP: u32 = 10;
/// Title and body are cut to this many characters when an entry is made.
pub const MAX_TEXT: usize = 1024;

/// "Jumlah maksimal": any number kept in `MIN_CAP..=MAX_CAP`, snapped to the
/// nearest step of 10.
pub fn clamp_cap(n: u32) -> u32 {
    let snapped = n.saturating_add(CAP_STEP / 2) / CAP_STEP * CAP_STEP;
    snapped.clamp(MIN_CAP, MAX_CAP)
}

/// One caught toast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifEntry {
    pub id: u64,
    /// Unix seconds when Vale caught it.
    pub at: i64,
    pub read: bool,
    /// The app's display name.
    pub source: String,
    /// The app's AppUserModelID, when Windows told it.
    pub app_id: Option<String>,
    pub title: String,
    pub body: String,
    /// The web site of a browser notification, when Windows gave it.
    pub site: Option<String>,
}

impl NotifEntry {
    /// What keys this entry's logo: see `logo_key`.
    pub fn logo_key(&self) -> String {
        logo_key(self.app_id.as_deref(), &self.source)
    }
}

/// The logo key of a toast: its AppUserModelID when it has one, else the
/// app's display name.
pub fn logo_key(app_id: Option<&str>, source: &str) -> String {
    match app_id {
        Some(id) if !id.is_empty() => id.to_owned(),
        _ => source.to_owned(),
    }
}

fn cut(text: &str) -> String {
    match text.char_indices().nth(MAX_TEXT) {
        Some((i, _)) => text[..i].to_owned(),
        None => text.to_owned(),
    }
}

/// Where a fresh toast goes: `(notch, history)`. A muted app's toast goes
/// nowhere; any other reaches the history when recording is on and not
/// paused, and the notch unless it is `quiet` (caught by the first read after
/// the poller's pause).
pub fn route_toast(
    source: &str,
    muted: &[String],
    record_on: bool,
    paused: bool,
    quiet: bool,
) -> (bool, bool) {
    if is_muted(source, muted) {
        return (false, false);
    }
    (!quiet, record_on && !paused)
}

/// The entries, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifHistory {
    cap: usize,
    items: Vec<NotifEntry>,
    next_id: u64,
}

impl Default for NotifHistory {
    fn default() -> Self {
        Self::new(DEFAULT_CAP)
    }
}

impl NotifHistory {
    pub fn new(cap: u32) -> Self {
        NotifHistory {
            cap: clamp_cap(cap) as usize,
            items: Vec::new(),
            next_id: 1,
        }
    }

    /// Restored entries (newest first) with the id counter they were saved
    /// with; the cap trims the oldest, and the counter never falls behind an
    /// id in use.
    pub fn from_parts(cap: u32, next_id: u64, items: Vec<NotifEntry>) -> Self {
        let mut h = NotifHistory::new(cap);
        let highest = items.iter().map(|e| e.id).max().unwrap_or(0);
        h.next_id = next_id.max(highest.saturating_add(1)).max(1);
        h.items = items;
        h.enforce();
        h
    }

    fn enforce(&mut self) {
        self.items.truncate(self.cap);
    }

    pub fn cap(&self) -> u32 {
        self.cap as u32
    }

    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Changes the cap (clamped, stepped) and drops the oldest over it.
    pub fn set_cap(&mut self, cap: u32) {
        self.cap = clamp_cap(cap) as usize;
        self.enforce();
    }

    /// A new unread entry at the front; the oldest go when the cap is over.
    /// Title, body and site are cut to `MAX_TEXT` characters. Returns its id.
    pub fn push(
        &mut self,
        at: i64,
        source: &str,
        app_id: Option<&str>,
        site: Option<&str>,
        title: &str,
        body: &str,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.insert(
            0,
            NotifEntry {
                id,
                at,
                read: false,
                source: source.to_owned(),
                app_id: app_id.filter(|a| !a.is_empty()).map(str::to_owned),
                title: cut(title),
                body: cut(body),
                site: site.filter(|s| !s.is_empty()).map(cut),
            },
        );
        self.enforce();
        id
    }

    /// True when the entry was unread.
    pub fn mark_read(&mut self, id: u64) -> bool {
        match self.items.iter_mut().find(|e| e.id == id) {
            Some(e) if !e.read => {
                e.read = true;
                true
            }
            _ => false,
        }
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|e| e.id != id);
        self.items.len() != before
    }

    /// True when anything was there.
    pub fn clear(&mut self) -> bool {
        let had = !self.items.is_empty();
        self.items.clear();
        had
    }

    /// Removes every entry whose source `is_muted`; how many went.
    pub fn remove_muted(&mut self, muted: &[String]) -> usize {
        let before = self.items.len();
        self.items.retain(|e| !is_muted(&e.source, muted));
        before - self.items.len()
    }

    pub fn get(&self, id: u64) -> Option<&NotifEntry> {
        self.items.iter().find(|e| e.id == id)
    }

    pub fn items(&self) -> &[NotifEntry] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn unread_count(&self) -> usize {
        self.items.iter().filter(|e| !e.read).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(h: &mut NotifHistory, n: i64) -> u64 {
        h.push(
            n,
            "WhatsApp",
            Some("5319.WhatsApp!App"),
            None,
            &format!("t{n}"),
            "b",
        )
    }

    #[test]
    fn newest_first_and_ids_count_up() {
        let mut h = NotifHistory::new(50);
        let a = push(&mut h, 1);
        let b = push(&mut h, 2);
        assert!(b > a);
        assert_eq!(h.items()[0].id, b);
        assert_eq!(h.items()[1].id, a);
        assert!(!h.items()[0].read);
    }

    #[test]
    fn the_cap_drops_the_oldest_first() {
        let mut h = NotifHistory::new(10);
        for n in 0..12 {
            push(&mut h, n);
        }
        assert_eq!(h.len(), 10);
        assert_eq!(h.items()[0].at, 11, "newest kept at the front");
        assert_eq!(h.items()[9].at, 2, "0 and 1 went");
    }

    #[test]
    fn set_cap_clamps_snaps_and_trims() {
        assert_eq!(clamp_cap(0), 10);
        assert_eq!(clamp_cap(7), 10);
        assert_eq!(clamp_cap(14), 10);
        assert_eq!(clamp_cap(15), 20);
        assert_eq!(clamp_cap(999), 200);
        let mut h = NotifHistory::default();
        assert_eq!(h.cap(), 50);
        for n in 0..40 {
            push(&mut h, n);
        }
        h.set_cap(20);
        assert_eq!(h.len(), 20);
        assert_eq!(h.items()[0].at, 39);
        assert_eq!(h.items()[19].at, 20);
        h.set_cap(5);
        assert_eq!(h.cap(), 10);
        assert_eq!(h.len(), 10);
        h.set_cap(200);
        assert_eq!(h.len(), 10, "a larger cap brings nothing back");
    }

    #[test]
    fn read_marks_and_the_unread_count() {
        let mut h = NotifHistory::new(50);
        let a = push(&mut h, 1);
        let b = push(&mut h, 2);
        assert_eq!(h.unread_count(), 2);
        assert!(h.mark_read(a));
        assert!(!h.mark_read(a), "already read");
        assert!(!h.mark_read(999));
        assert_eq!(h.unread_count(), 1);
        assert!(h.get(a).unwrap().read);
        assert!(!h.get(b).unwrap().read);
    }

    #[test]
    fn remove_and_clear() {
        let mut h = NotifHistory::new(50);
        let a = push(&mut h, 1);
        let b = push(&mut h, 2);
        assert!(h.remove(a));
        assert!(!h.remove(a));
        assert_eq!(h.len(), 1);
        assert_eq!(h.items()[0].id, b);
        assert!(h.clear());
        assert!(!h.clear(), "nothing left to clear");
        assert_eq!(h.len(), 0);
        // Ids keep counting after a clear.
        let c = push(&mut h, 3);
        assert!(c > b);
    }

    #[test]
    fn remove_muted_is_the_case_insensitive_substring_rule() {
        let mut h = NotifHistory::new(50);
        for source in ["Claude", "orca agent", "WhatsApp", "My CLAUDE app"] {
            h.push(1, source, None, None, "t", "b");
        }
        let muted = vec!["Claude".to_owned(), "Orca".to_owned()];
        assert_eq!(h.remove_muted(&muted), 3);
        assert_eq!(h.len(), 1);
        assert_eq!(h.items()[0].source, "WhatsApp");
        assert_eq!(h.remove_muted(&muted), 0);
        assert_eq!(h.remove_muted(&[]), 0);
    }

    #[test]
    fn text_is_cut_to_1024_characters_on_a_character_boundary() {
        let mut h = NotifHistory::new(50);
        let long = "é".repeat(2000);
        h.push(1, "App", None, None, &long, &long);
        let e = &h.items()[0];
        assert_eq!(e.title.chars().count(), 1024);
        assert_eq!(e.body.chars().count(), 1024);
        h.push(2, "App", None, None, "short", "");
        assert_eq!(h.items()[0].title, "short");
    }

    #[test]
    fn an_empty_app_id_is_none_and_the_logo_key_falls_back_to_the_source() {
        let mut h = NotifHistory::new(50);
        h.push(1, "Chrome", Some(""), None, "t", "b");
        h.push(2, "Teams", Some("MSTeams!App"), None, "t", "b");
        assert_eq!(h.items()[1].app_id, None);
        assert_eq!(h.items()[1].logo_key(), "Chrome");
        assert_eq!(h.items()[0].logo_key(), "MSTeams!App");
        assert_eq!(logo_key(None, "X"), "X");
    }

    #[test]
    fn site_is_kept_cut_and_empty_means_none() {
        let mut h = NotifHistory::new(50);
        h.push(
            1,
            "Chrome",
            Some("Chrome"),
            Some("web.telegram.org"),
            "t",
            "b",
        );
        assert_eq!(h.items()[0].site.as_deref(), Some("web.telegram.org"));
        h.push(2, "Chrome", Some("Chrome"), Some(""), "t", "b");
        assert_eq!(h.items()[0].site, None);
        h.push(3, "Chrome", Some("Chrome"), None, "t", "b");
        assert_eq!(h.items()[0].site, None);
        let long = "a".repeat(2000);
        h.push(4, "Chrome", None, Some(&long), "t", "b");
        assert_eq!(h.items()[0].site.as_ref().unwrap().chars().count(), 1024);
    }

    #[test]
    fn restoring_keeps_the_counter_ahead_of_every_id_and_applies_the_cap() {
        let items: Vec<NotifEntry> = (1..=15)
            .rev()
            .map(|id| NotifEntry {
                id,
                at: id as i64,
                read: false,
                source: "A".into(),
                app_id: None,
                title: "t".into(),
                body: String::new(),
                site: None,
            })
            .collect();
        let h = NotifHistory::from_parts(10, 3, items);
        assert_eq!(h.len(), 10);
        assert_eq!(h.next_id(), 16, "a stale counter never reuses an id");
        assert_eq!(h.items()[0].id, 15);
    }

    #[test]
    fn route_truth_table() {
        let muted = vec!["Claude".to_owned()];
        // Muted: nowhere, whatever the switches.
        for record in [false, true] {
            for paused in [false, true] {
                for quiet in [false, true] {
                    assert_eq!(
                        route_toast("Claude", &muted, record, paused, quiet),
                        (false, false)
                    );
                }
            }
        }
        let route = |record, paused| route_toast("WhatsApp", &muted, record, paused, false);
        assert_eq!(route(true, false), (true, true));
        assert_eq!(route(true, true), (true, false));
        assert_eq!(route(false, false), (true, false));
        assert_eq!(route(false, true), (true, false));
        assert_eq!(
            route_toast("WhatsApp", &[], true, false, false),
            (true, true)
        );
    }

    #[test]
    fn a_quiet_toast_reaches_the_history_never_the_notch() {
        let muted = vec!["Claude".to_owned()];
        let route = |record, paused| route_toast("WhatsApp", &muted, record, paused, true);
        assert_eq!(route(true, false), (false, true));
        assert_eq!(route(true, true), (false, false));
        assert_eq!(route(false, false), (false, false));
        assert_eq!(route(false, true), (false, false));
        assert_eq!(route_toast("WhatsApp", &[], true, false, true), (false, true));
    }
}
