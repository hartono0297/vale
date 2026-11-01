//! Which panel pages the wheel walks through, and in what order: the
//! "Halaman panel" setting. Every page is always listed exactly once; a page
//! that is off is skipped by the wheel and the page dots, but its own button
//! (the Clipboard tile, a disk warning) still opens it.

use serde::{Deserialize, Serialize};

use crate::model::Page;

/// One page in `config.json`: its name and whether the wheel shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageEntry {
    pub page: String,
    pub on: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelPages {
    entries: Vec<(Page, bool)>,
}

impl Default for PanelPages {
    fn default() -> Self {
        PanelPages {
            entries: Page::ALL.iter().map(|p| (*p, true)).collect(),
        }
    }
}

impl PanelPages {
    /// The saved list made whole: unknown and repeated names are dropped, a
    /// page the file does not name (an older config) joins right after the
    /// page before it in `Page::ALL`, and a list with nothing on shows all.
    pub fn from_saved(saved: &[PageEntry]) -> Self {
        let mut entries: Vec<(Page, bool)> = Vec::new();
        for e in saved {
            if let Some(page) = page_from_name(&e.page)
                && !entries.iter().any(|(p, _)| *p == page)
            {
                entries.push((page, e.on));
            }
        }
        for (i, page) in Page::ALL.iter().enumerate() {
            if entries.iter().any(|(p, _)| p == page) {
                continue;
            }
            let after = Page::ALL[..i]
                .iter()
                .rev()
                .find_map(|prev| entries.iter().position(|(p, _)| p == prev));
            entries.insert(after.map_or(0, |a| a + 1), (*page, true));
        }
        if entries.iter().all(|(_, on)| !on) {
            for e in &mut entries {
                e.1 = true;
            }
        }
        PanelPages { entries }
    }

    pub fn to_saved(&self) -> Vec<PageEntry> {
        self.entries
            .iter()
            .map(|(p, on)| PageEntry {
                page: page_name(*p).to_owned(),
                on: *on,
            })
            .collect()
    }

    pub fn entries(&self) -> &[(Page, bool)] {
        &self.entries
    }

    /// The pages the wheel and the dots show, in order.
    pub fn visible(&self) -> Vec<Page> {
        self.entries
            .iter()
            .filter(|(_, on)| *on)
            .map(|(p, _)| *p)
            .collect()
    }

    pub fn is_on(&self, page: Page) -> bool {
        self.entries.iter().any(|(p, on)| *p == page && *on)
    }

    /// Flips `page`; refused (false) when it is the last page still shown.
    pub fn toggle(&mut self, page: Page) -> bool {
        let i = self.position(page);
        if self.entries[i].1 && self.visible().len() == 1 {
            return false;
        }
        self.entries[i].1 = !self.entries[i].1;
        true
    }

    /// Takes `page` out and puts it at place `index` of the order (the
    /// place it has once moved, shown pages and hidden ones counted alike);
    /// false when `index` is out of range or nothing moves.
    pub fn move_to(&mut self, page: Page, index: usize) -> bool {
        let from = self.position(page);
        if index >= self.entries.len() || index == from {
            return false;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(index, entry);
        true
    }

    /// Moves a shown `page` `delta` places among the pages shown (negative:
    /// earlier), stopping at the ends. Hidden pages in between keep their
    /// places relative to it. False when it did not move or is hidden.
    pub fn move_shown(&mut self, page: Page, delta: i32) -> bool {
        let shown = self.visible();
        let Some(at) = shown.iter().position(|p| *p == page) else {
            return false;
        };
        let to = (at as i64 + i64::from(delta)).clamp(0, shown.len() as i64 - 1) as usize;
        to != at && self.move_to(page, self.position(shown[to]))
    }

    /// Where `page` is in the order, shown or not.
    pub fn position(&self, page: Page) -> usize {
        self.entries
            .iter()
            .position(|(p, _)| *p == page)
            .expect("every page is listed")
    }

    /// The page `steps` shown pages after `from` (negative: before),
    /// stopping at the ends. From a page that is off (opened by its own
    /// button) the count starts at its place in the order; with nothing
    /// shown on that side it stays.
    pub fn step(&self, from: Page, steps: i64) -> Page {
        if steps == 0 {
            return from;
        }
        let at = self.position(from);
        let side: Vec<Page> = if steps > 0 {
            self.entries[at + 1..]
                .iter()
                .filter(|(_, on)| *on)
                .map(|(p, _)| *p)
                .collect()
        } else {
            self.entries[..at]
                .iter()
                .rev()
                .filter(|(_, on)| *on)
                .map(|(p, _)| *p)
                .collect()
        };
        let n = steps.unsigned_abs() as usize;
        side.get(n - 1).or(side.last()).copied().unwrap_or(from)
    }
}

/// The page's name in `config.json`.
pub fn page_name(page: Page) -> &'static str {
    match page {
        Page::Calendar => "Calendar",
        Page::Pomodoro => "Pomodoro",
        Page::Reminders => "Reminders",
        Page::Notes => "Notes",
        Page::Media => "Media",
        Page::Mixer => "Mixer",
        Page::Clipboard => "Clipboard",
        Page::Notifications => "Notifications",
        Page::Shelf => "Shelf",
        Page::System => "System",
        Page::Devices => "Devices",
        Page::Cli => "Cli",
        Page::Arrange => "Arrange",
        Page::Shortcuts => "Shortcuts",
        // Not in the order; never saved.
        Page::Search => "Search",
    }
}

/// Case-insensitive; `None` for a name no page has.
pub fn page_from_name(name: &str) -> Option<Page> {
    let name = name.trim();
    Page::ALL
        .into_iter()
        .find(|p| page_name(*p).eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(page: &str, on: bool) -> PageEntry {
        PageEntry {
            page: page.to_owned(),
            on,
        }
    }

    #[test]
    fn default_shows_all_fourteen_pages_in_the_design_order() {
        let p = PanelPages::default();
        assert_eq!(p.visible(), Page::ALL.to_vec());
        assert_eq!(
            Page::ALL,
            [
                Page::Calendar,
                Page::Pomodoro,
                Page::Reminders,
                Page::Notes,
                Page::Media,
                Page::Mixer,
                Page::Clipboard,
                Page::Notifications,
                Page::Shelf,
                Page::System,
                Page::Devices,
                Page::Cli,
                Page::Arrange,
                Page::Shortcuts,
            ]
        );
        assert_eq!(Page::ALL.len(), 14);
        assert_eq!(p.visible().len(), 14, "every page shows by default");
        assert_eq!(page_name(Page::Arrange), "Arrange");
        assert_eq!(page_from_name(" arrange "), Some(Page::Arrange));
        assert_eq!(page_name(Page::Devices), "Devices");
        assert_eq!(page_from_name(" devices "), Some(Page::Devices));
        assert_eq!(page_name(Page::Notifications), "Notifications");
        assert_eq!(page_from_name(" notifications "), Some(Page::Notifications));
    }

    #[test]
    fn a_saved_order_without_notifications_gets_it_back_after_clipboard() {
        // A config written before the Notifikasi page: every other page, the
        // owner's order (Shortcuts moved first) and one of them off.
        let mut saved: Vec<PageEntry> = Page::ALL
            .iter()
            .filter(|p| **p != Page::Notifications)
            .map(|p| entry(page_name(*p), true))
            .collect();
        saved.rotate_right(1);
        saved[1].on = false; // Calendar off
        let p = PanelPages::from_saved(&saved);
        assert_eq!(p.entries().len(), Page::ALL.len());
        assert_eq!(
            p.position(Page::Notifications),
            p.position(Page::Clipboard) + 1,
            "right after Clipboard"
        );
        assert!(p.is_on(Page::Notifications), "and shown");
        assert!(!p.is_on(Page::Calendar), "the rest is untouched");
        assert_eq!(p.entries()[0].0, Page::Shortcuts);
        // The default order puts it between Clipboard and the Shelf.
        let plain: Vec<PageEntry> = Page::ALL
            .iter()
            .filter(|p| **p != Page::Notifications)
            .map(|p| entry(page_name(*p), true))
            .collect();
        assert_eq!(PanelPages::from_saved(&plain).visible(), Page::ALL.to_vec());
        // With Clipboard moved to the end, it follows it there.
        let mut moved: Vec<PageEntry> = Page::ALL
            .iter()
            .filter(|p| !matches!(p, Page::Notifications | Page::Clipboard))
            .map(|p| entry(page_name(*p), true))
            .collect();
        moved.push(entry("Clipboard", true));
        let p = PanelPages::from_saved(&moved);
        assert_eq!(p.entries().last().unwrap().0, Page::Notifications);
    }

    #[test]
    fn a_saved_order_without_devices_gets_it_back_after_system() {
        // A config written before the Devices page: every other page, the
        // owner's order (Cli moved first) and one of them off.
        let mut saved: Vec<PageEntry> = Page::ALL
            .iter()
            .filter(|p| **p != Page::Devices)
            .map(|p| entry(page_name(*p), true))
            .collect();
        saved.rotate_right(1); // Shortcuts first
        saved[1].on = false; // Calendar off
        let p = PanelPages::from_saved(&saved);
        assert_eq!(p.entries().len(), Page::ALL.len());
        assert_eq!(
            p.position(Page::Devices),
            p.position(Page::System) + 1,
            "right after System"
        );
        assert!(p.is_on(Page::Devices), "and shown");
        assert!(!p.is_on(Page::Calendar), "the rest is untouched");
        assert_eq!(p.entries()[0].0, Page::Shortcuts);
        // The default order puts it between System and Cli.
        let plain: Vec<PageEntry> = Page::ALL
            .iter()
            .filter(|p| **p != Page::Devices)
            .map(|p| entry(page_name(*p), true))
            .collect();
        assert_eq!(PanelPages::from_saved(&plain).visible(), Page::ALL.to_vec());
    }

    #[test]
    fn a_saved_order_without_arrange_gets_it_back_after_cli() {
        // A config written before Susun: the thirteen pages, the owner's
        // order (Cli moved to the front, Shortcuts last) and one off.
        let mut saved: Vec<PageEntry> = Page::ALL
            .iter()
            .filter(|p| **p != Page::Arrange)
            .map(|p| entry(page_name(*p), true))
            .collect();
        assert_eq!(saved.len(), 13);
        let cli = saved.iter().position(|e| e.page == "Cli").unwrap();
        let moved = saved.remove(cli);
        saved.insert(0, moved);
        saved[1].on = false; // Calendar off
        let p = PanelPages::from_saved(&saved);
        assert_eq!(p.entries().len(), 14);
        assert_eq!(
            p.position(Page::Arrange),
            p.position(Page::Cli) + 1,
            "right after Cli wherever it went"
        );
        assert_eq!(p.entries()[0].0, Page::Cli);
        assert_eq!(p.entries().last().unwrap().0, Page::Shortcuts);
        assert!(p.is_on(Page::Arrange), "and shown");
        assert!(!p.is_on(Page::Calendar), "the rest is untouched");
        // The default order puts it between Cli and Pintasan.
        let plain: Vec<PageEntry> = Page::ALL
            .iter()
            .filter(|p| **p != Page::Arrange)
            .map(|p| entry(page_name(*p), true))
            .collect();
        assert_eq!(PanelPages::from_saved(&plain).visible(), Page::ALL.to_vec());
    }

    #[test]
    fn names_round_trip_and_unknown_names_are_none() {
        for page in Page::ALL {
            assert_eq!(page_from_name(page_name(page)), Some(page));
        }
        assert_eq!(page_from_name(" media "), Some(Page::Media));
        assert_eq!(page_from_name("Weather"), None);
    }

    #[test]
    fn saved_order_and_flags_are_kept() {
        let mut saved: Vec<PageEntry> = Page::ALL
            .iter()
            .rev()
            .map(|p| entry(page_name(*p), true))
            .collect();
        saved[0].on = false; // Shortcuts, now first, is off
        let p = PanelPages::from_saved(&saved);
        let order: Vec<Page> = p.entries().iter().map(|e| e.0).collect();
        let mut reversed = Page::ALL.to_vec();
        reversed.reverse();
        assert_eq!(order, reversed);
        assert!(!p.is_on(Page::Shortcuts));
        assert_eq!(p.visible().first(), Some(&Page::Arrange));
        assert_eq!(PanelPages::from_saved(&p.to_saved()), p);
    }

    #[test]
    fn unknown_and_repeated_names_are_dropped() {
        let saved = [
            entry("Notes", true),
            entry("Weather", true),
            entry("notes", false),
        ];
        let p = PanelPages::from_saved(&saved);
        assert_eq!(p.entries().len(), Page::ALL.len());
        assert!(p.is_on(Page::Notes), "the first Notes entry wins");
        // Calendar was not named, and nothing comes before it by default.
        assert_eq!(p.entries()[0].0, Page::Calendar);
    }

    #[test]
    fn a_page_missing_from_the_file_joins_after_its_default_neighbour() {
        // An older config without Clipboard and System.
        let saved: Vec<PageEntry> = [
            Page::Calendar,
            Page::Pomodoro,
            Page::Reminders,
            Page::Notes,
            Page::Media,
            Page::Cli,
            Page::Shortcuts,
        ]
        .iter()
        .map(|p| entry(page_name(*p), true))
        .collect();
        let p = PanelPages::from_saved(&saved);
        assert_eq!(p.visible(), Page::ALL.to_vec());
    }

    #[test]
    fn a_file_with_every_page_off_shows_them_all() {
        let saved: Vec<PageEntry> = Page::ALL
            .iter()
            .map(|p| entry(page_name(*p), false))
            .collect();
        assert_eq!(PanelPages::from_saved(&saved).visible(), Page::ALL.to_vec());
        assert_eq!(PanelPages::from_saved(&[]), PanelPages::default());
    }

    #[test]
    fn the_last_visible_page_cannot_be_turned_off() {
        let mut p = PanelPages::default();
        for page in &Page::ALL[1..] {
            assert!(p.toggle(*page));
        }
        assert_eq!(p.visible(), vec![Page::Calendar]);
        assert!(!p.toggle(Page::Calendar), "refused");
        assert!(p.is_on(Page::Calendar));
        assert!(p.toggle(Page::Notes), "turning one back on works");
        assert!(p.is_on(Page::Notes));
    }

    #[test]
    fn moving_a_page_swaps_it_with_its_neighbour_and_stops_at_the_ends() {
        let mut p = PanelPages::default();
        assert!(p.move_shown(Page::Media, -1));
        assert_eq!(p.position(Page::Media), 3);
        assert_eq!(p.position(Page::Notes), 4);
        assert!(!p.move_shown(Page::Calendar, -1), "already first");
        assert!(!p.move_shown(Page::Shortcuts, 1), "already last");
        assert!(p.move_shown(Page::Calendar, 1));
        assert_eq!(p.entries()[0].0, Page::Pomodoro);
    }

    #[test]
    fn move_to_puts_a_page_at_a_place_of_the_order() {
        let mut p = PanelPages::default();
        assert!(p.move_to(Page::Calendar, 3));
        assert_eq!(p.position(Page::Calendar), 3);
        assert_eq!(p.entries()[0].0, Page::Pomodoro);
        assert_eq!(p.entries()[2].0, Page::Notes);
        assert!(p.move_to(Page::Calendar, 0), "and back");
        assert_eq!(p.visible(), Page::ALL.to_vec());
        assert!(!p.move_to(Page::Notes, 3), "already there");
        assert!(!p.move_to(Page::Notes, 99), "out of range");
        assert!(p.move_to(Page::Calendar, Page::ALL.len() - 1));
        assert_eq!(p.entries().last().unwrap().0, Page::Calendar);
        // Flags travel with the page.
        let mut p = PanelPages::default();
        p.toggle(Page::Notes);
        p.move_to(Page::Notes, 0);
        assert!(!p.is_on(Page::Notes));
        assert_eq!(p.position(Page::Notes), 0);
    }

    #[test]
    fn move_shown_steps_over_hidden_pages() {
        let mut p = PanelPages::default();
        p.toggle(Page::Reminders);
        // Notes goes up one shown page, past the hidden Reminders.
        assert!(p.move_shown(Page::Notes, -1));
        assert_eq!(p.position(Page::Notes), 1);
        assert_eq!(
            p.visible()[..4],
            [Page::Calendar, Page::Notes, Page::Pomodoro, Page::Media]
        );
        let mut p = PanelPages::default();
        p.toggle(Page::Reminders);
        // Pomodoro down: past the hidden Reminders to Notes' place.
        assert!(p.move_shown(Page::Pomodoro, 1));
        assert_eq!(
            p.visible()[..4],
            [Page::Calendar, Page::Notes, Page::Pomodoro, Page::Media]
        );
        assert!(!p.move_shown(Page::Calendar, -1), "first");
        assert!(!p.move_shown(Page::Shortcuts, 1), "last");
        assert!(!p.move_shown(Page::Reminders, 1), "hidden");
        // The ends clamp: a big step lands on the first.
        assert!(p.move_shown(Page::Shortcuts, -50));
        assert_eq!(p.visible()[0], Page::Shortcuts);
    }

    #[test]
    fn the_wheel_skips_pages_that_are_off_and_stops_at_the_ends() {
        let mut p = PanelPages::default();
        p.toggle(Page::Pomodoro);
        p.toggle(Page::Reminders);
        assert_eq!(p.step(Page::Calendar, 1), Page::Notes);
        assert_eq!(p.step(Page::Notes, -1), Page::Calendar);
        assert_eq!(p.step(Page::Calendar, 2), Page::Media);
        assert_eq!(p.step(Page::Calendar, -3), Page::Calendar);
        assert_eq!(p.step(Page::Cli, 5), Page::Shortcuts);
        assert_eq!(p.step(Page::Media, 0), Page::Media);
    }

    #[test]
    fn from_a_page_that_is_off_the_wheel_goes_to_the_nearest_shown_one() {
        let mut p = PanelPages::default();
        p.toggle(Page::Clipboard);
        p.toggle(Page::System);
        // Opened by its tile while it is off.
        assert_eq!(p.step(Page::Clipboard, 1), Page::Notifications);
        assert_eq!(p.step(Page::Clipboard, -1), Page::Mixer);
        assert_eq!(p.step(Page::System, 2), Page::Cli);
        // Nothing shown past it: it stays.
        p.toggle(Page::Shortcuts);
        p.move_to(Page::System, Page::ALL.len() - 1);
        assert_eq!(p.entries().last().unwrap().0, Page::System);
        assert_eq!(p.step(Page::System, 1), Page::System);
    }

    #[test]
    fn the_wheel_follows_a_custom_order() {
        let mut p = PanelPages::default();
        p.move_to(Page::Shortcuts, 0);
        assert_eq!(p.entries()[0].0, Page::Shortcuts);
        assert_eq!(p.step(Page::Shortcuts, 1), Page::Calendar);
        assert_eq!(p.step(Page::Calendar, -1), Page::Shortcuts);
    }
}
