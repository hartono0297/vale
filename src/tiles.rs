//! Which tiles the Pintasan page shows, in what order: the "Pintasan" setting.
//! The built-in tiles (the original nine and a jump to each panel page) are
//! always listed once each (one that is off is hidden,
//! not lost); custom tiles (an app, a folder, a web address) are the owner's
//! own and can be removed. At most twelve show: a tile cannot be switched on
//! while twelve already are, and a built-in a saved list does not name
//! rejoins on only when it is on by default and the page has room. Names in
//! the file are read without regard to case.

use serde::{Deserialize, Serialize};

use crate::model::{Page, Shortcut};
use crate::pages::{page_from_name, page_name};

/// Tiles shown at once: four columns, three rows.
pub const MAX_SHOWN: usize = 12;
/// Custom tiles kept in the list.
pub const MAX_CUSTOM: usize = 8;
/// A custom tile's label, in characters (it sits under an icon).
pub const LABEL_MAX: usize = 14;
/// A custom tile's target (a path or an address), in characters.
pub const TARGET_MAX: usize = 500;

/// The `tile` name a custom entry carries in `config.json`.
const CUSTOM: &str = "Custom";

/// What a custom tile opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomKind {
    /// A program or a file, by its path.
    App,
    /// A folder, by its path.
    Folder,
    /// A web address (http, https) or a mail link (mailto).
    Url,
}

impl CustomKind {
    pub const ALL: [CustomKind; 3] = [CustomKind::App, CustomKind::Folder, CustomKind::Url];

    fn name(self) -> &'static str {
        match self {
            CustomKind::App => "App",
            CustomKind::Folder => "Folder",
            CustomKind::Url => "Url",
        }
    }

    fn from_name(name: &str) -> Option<CustomKind> {
        Self::ALL
            .into_iter()
            .find(|k| k.name().eq_ignore_ascii_case(name.trim()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomTile {
    pub label: String,
    pub kind: CustomKind,
    pub target: String,
}

/// Which part of a typed tile `CustomTile::check` refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomProblem {
    Label,
    Target,
}

impl CustomTile {
    /// A tile from what the owner typed: both parts trimmed, within their
    /// limits and free of control characters, a web address with a safe
    /// scheme, and an app or folder as an absolute local path. `None` when
    /// any of that fails: nothing is cut off to make it pass.
    pub fn new(label: &str, kind: CustomKind, target: &str) -> Option<CustomTile> {
        Self::check(label, kind, target).ok()
    }

    /// `new`, saying which part failed (the label is judged first), for the
    /// Settings form's message under the fields.
    pub fn check(label: &str, kind: CustomKind, target: &str) -> Result<CustomTile, CustomProblem> {
        let (label, target) = (label.trim(), target.trim());
        let clean = |s: &str, max: usize| {
            !s.is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
        };
        if !clean(label, LABEL_MAX) {
            return Err(CustomProblem::Label);
        }
        let valid = clean(target, TARGET_MAX)
            && match kind {
                CustomKind::Url => safe_url(target),
                CustomKind::App | CustomKind::Folder => local_path(target),
            };
        if !valid {
            return Err(CustomProblem::Target);
        }
        Ok(CustomTile {
            label: label.to_owned(),
            kind,
            target: target.to_owned(),
        })
    }
}

/// An address the shell may open from a tile: http(s) with a host, or a
/// mail link without a `?` query (which could fill in a body, copies or an
/// attachment). Anything else (file:, javascript:, ms-*: handlers, bare
/// words) is not a link this page launches.
fn safe_url(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    let host = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    match host {
        Some(rest) => rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric()),
        None => lower
            .strip_prefix("mailto:")
            .is_some_and(|rest| !rest.is_empty() && !rest.contains('?')),
    }
}

/// A path an app or folder tile may open: absolute and local, a drive
/// letter then `:\` ("C:\Program Files\…"), with no other `:` (no stream
/// name, no `scheme:` further on). Relative paths (looked up wherever the
/// shell looks), UNC shares (`\\server\…`), `\\?\` forms and addresses are
/// refused.
fn local_path(target: &str) -> bool {
    let b = target.as_bytes();
    b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/')
        && !target[2..].contains(':')
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tile {
    Builtin(Shortcut),
    Custom(CustomTile),
}

/// One tile in `config.json`: a built-in by name (`{"tile":"Lock","on":false}`)
/// or a custom one (`{"tile":"Custom","label":"Gmail","kind":"Url","target":
/// "https://mail.google.com"}`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileEntry {
    pub tile: String,
    /// Absent means shown, for an entry written by hand.
    #[serde(default = "shown")]
    pub on: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
}

fn shown() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutTiles {
    entries: Vec<(Tile, bool)>,
}

impl Default for ShortcutTiles {
    fn default() -> Self {
        ShortcutTiles {
            entries: Shortcut::ALL
                .iter()
                .map(|s| (Tile::Builtin(*s), s.default_on()))
                .collect(),
        }
    }
}

/// Where the Settings list files a tile that is off, under "Tersedia".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileGroup {
    /// The jump to a page (and the Clipboard tile, which opens its page).
    Pages,
    /// The built-ins that run a Windows tool.
    Windows,
    /// Vale's own: its quick search.
    Vale,
    /// The owner's own tiles.
    Own,
}

impl TileGroup {
    pub const ALL: [TileGroup; 4] = [
        TileGroup::Pages,
        TileGroup::Windows,
        TileGroup::Vale,
        TileGroup::Own,
    ];
}

/// The group a tile is listed under.
pub fn group(tile: &Tile) -> TileGroup {
    match tile {
        Tile::Custom(_) => TileGroup::Own,
        Tile::Builtin(Shortcut::Go(_) | Shortcut::Clipboard) => TileGroup::Pages,
        Tile::Builtin(Shortcut::Search) => TileGroup::Vale,
        Tile::Builtin(_) => TileGroup::Windows,
    }
}

/// The built-in's name in the file ("Lock", "GoShelf").
pub(crate) fn shortcut_name(s: Shortcut) -> String {
    match s {
        Shortcut::Screenshot => "Screenshot".into(),
        Shortcut::Clipboard => "Clipboard".into(),
        Shortcut::Search => "Search".into(),
        Shortcut::Emoji => "Emoji".into(),
        Shortcut::Explorer => "Explorer".into(),
        Shortcut::Calculator => "Calculator".into(),
        Shortcut::TaskManager => "TaskManager".into(),
        Shortcut::WindowsSettings => "WindowsSettings".into(),
        Shortcut::Lock => "Lock".into(),
        Shortcut::Go(page) => format!("Go{}", page_name(page)),
    }
}

/// The page a "GoShelf" name jumps to (any case), whether or not it has a tile.
fn go_page(name: &str) -> Option<Page> {
    let head = name.get(..2)?;
    head.eq_ignore_ascii_case("go")
        .then(|| page_from_name(&name[2..]))
        .flatten()
}

/// A built-in by its name in the file, in any case ("lock" is `Lock`,
/// "goshelf" is `Go(Shelf)`). `Go` takes only a page that has a tile in
/// `Shortcut::ALL`.
pub fn shortcut_from_name(name: &str) -> Option<Shortcut> {
    let name = name.trim();
    if let Some(page) = go_page(name) {
        return Some(Shortcut::Go(page)).filter(|s| Shortcut::ALL.contains(s));
    }
    Shortcut::ALL
        .into_iter()
        .find(|s| shortcut_name(*s).eq_ignore_ascii_case(name))
}

/// The entry names a custom tile ("Custom", in any case).
fn is_custom_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(CUSTOM)
}

/// The tiles a first run shows, in order: `MAX_SHOWN` of them.
const FRESH_SHOWN: [Shortcut; MAX_SHOWN] = [
    Shortcut::Go(Page::Calendar),
    Shortcut::Go(Page::Reminders),
    Shortcut::Go(Page::Notes),
    Shortcut::Go(Page::Pomodoro),
    Shortcut::Go(Page::Media),
    Shortcut::Clipboard,
    Shortcut::Go(Page::Shelf),
    Shortcut::Go(Page::Notifications),
    Shortcut::Screenshot,
    Shortcut::Search,
    Shortcut::Go(Page::Record),
    Shortcut::Go(Page::Mixer),
];

impl ShortcutTiles {
    /// The first-run layout: the twelve main tiles shown, every other
    /// built-in listed after them in `Shortcut::ALL` order and off.
    /// `default()` is unchanged on purpose.
    pub fn fresh() -> Self {
        let on = FRESH_SHOWN.iter().map(|s| (Tile::Builtin(*s), true));
        let off = Shortcut::ALL
            .iter()
            .filter(|s| !FRESH_SHOWN.contains(s))
            .map(|s| (Tile::Builtin(*s), false));
        ShortcutTiles {
            entries: on.chain(off).collect(),
        }
    }

    /// The saved list made whole: unknown names, repeated built-ins and
    /// custom entries that fail `CustomTile::new` are dropped (so are custom
    /// entries past `MAX_CUSTOM`), a built-in the file does not name (an
    /// older config) joins right after the one before it in `Shortcut::ALL`
    /// (on only when it is on by default and the page has room), and a list
    /// that shows nothing shows the built-ins that are on by default.
    pub fn from_saved(saved: &[TileEntry]) -> Self {
        let mut entries: Vec<(Tile, bool)> = Vec::new();
        let mut customs = 0;
        for e in saved {
            if is_custom_name(&e.tile) {
                let tile = CustomKind::from_name(&e.kind)
                    .and_then(|kind| CustomTile::new(&e.label, kind, &e.target));
                if let Some(tile) = tile
                    && customs < MAX_CUSTOM
                {
                    customs += 1;
                    entries.push((Tile::Custom(tile), e.on));
                }
            } else if let Some(s) = shortcut_from_name(&e.tile)
                && !entries.iter().any(|(t, _)| *t == Tile::Builtin(s))
            {
                entries.push((Tile::Builtin(s), e.on));
            }
        }
        // A file that shows nothing is made whole before the missing tiles
        // join: they would otherwise count as shown.
        if entries.iter().all(|(_, on)| !on) {
            for (tile, on) in &mut entries {
                if matches!(tile, Tile::Builtin(s) if s.default_on()) {
                    *on = true;
                }
            }
        }
        for (i, s) in Shortcut::ALL.iter().enumerate() {
            let tile = Tile::Builtin(*s);
            if entries.iter().any(|(t, _)| *t == tile) {
                continue;
            }
            let after = Shortcut::ALL[..i]
                .iter()
                .rev()
                .find_map(|prev| entries.iter().position(|(t, _)| *t == Tile::Builtin(*prev)));
            // A tile the file never named comes in shown only when it is on
            // by default and the page is not full: it must not push the
            // owner's own tile off it.
            let room = entries.iter().filter(|(_, on)| *on).count() < MAX_SHOWN;
            entries.insert(after.map_or(0, |a| a + 1), (tile, s.default_on() && room));
        }
        if entries.iter().all(|(_, on)| !on) {
            for (tile, on) in &mut entries {
                if matches!(tile, Tile::Builtin(s) if s.default_on()) {
                    *on = true;
                }
            }
        }
        ShortcutTiles { entries }
    }

    pub fn to_saved(&self) -> Vec<TileEntry> {
        self.entries
            .iter()
            .map(|(tile, on)| match tile {
                Tile::Builtin(s) => TileEntry {
                    tile: shortcut_name(*s),
                    on: *on,
                    ..TileEntry::default()
                },
                Tile::Custom(c) => TileEntry {
                    tile: CUSTOM.to_owned(),
                    on: *on,
                    label: c.label.clone(),
                    kind: c.kind.name().to_owned(),
                    target: c.target.clone(),
                },
            })
            .collect()
    }

    pub fn entries(&self) -> &[(Tile, bool)] {
        &self.entries
    }

    /// The tiles the page shows, in order, at most `MAX_SHOWN`.
    pub fn visible(&self) -> Vec<Tile> {
        self.entries
            .iter()
            .filter(|(_, on)| *on)
            .map(|(t, _)| t.clone())
            .take(MAX_SHOWN)
            .collect()
    }

    pub fn shown(&self) -> usize {
        self.entries.iter().filter(|(_, on)| *on).count()
    }

    pub fn customs(&self) -> usize {
        self.entries
            .iter()
            .filter(|(t, _)| matches!(t, Tile::Custom(_)))
            .count()
    }

    /// Twelve tiles show already: no other can be switched on.
    pub fn full(&self) -> bool {
        self.shown() >= MAX_SHOWN
    }

    /// Entry `i` is switched on but past the twelfth, so the page does not
    /// show it (a list edited by hand can have more on than fit).
    pub fn beyond_limit(&self, i: usize) -> bool {
        let before = |i: usize| self.entries[..i].iter().filter(|(_, on)| *on).count();
        matches!(self.entries.get(i), Some((_, true))) && before(i) >= MAX_SHOWN
    }

    /// Flips entry `i`. The last tile still shown stays on, and a tile stays
    /// off while twelve show already.
    pub fn toggle(&mut self, i: usize) -> bool {
        match self.entries.get(i) {
            Some((_, on)) => self.set_on(i, !on),
            None => false,
        }
    }

    /// Sets entry `i` on or off. False when nothing changed: it is that
    /// already, it is the last tile shown (stays on), or twelve show
    /// already (stays off).
    pub fn set_on(&mut self, i: usize, on: bool) -> bool {
        let shown = self.shown();
        match self.entries.get_mut(i) {
            Some((_, now)) if *now && !on && shown > 1 => *now = false,
            Some((_, now)) if !*now && on && shown < MAX_SHOWN => *now = true,
            _ => return false,
        }
        true
    }

    /// Moves tile `i` (shown) `delta` places among the tiles that are on
    /// (negative: earlier), stopping at the ends; the tiles that are off in
    /// between keep their places relative to it. False when it did not move
    /// or is off.
    pub fn move_shown(&mut self, i: usize, delta: i32) -> bool {
        let on: Vec<usize> = (0..self.entries.len())
            .filter(|k| self.entries[*k].1)
            .collect();
        let Some(at) = on.iter().position(|k| *k == i) else {
            return false;
        };
        let to = (at as i64 + i64::from(delta)).clamp(0, on.len() as i64 - 1) as usize;
        to != at && self.move_to(i, on[to])
    }

    /// Takes entry `i` out and puts it at place `to` (the place it has once
    /// moved); false when either is out of range or nothing moves.
    pub fn move_to(&mut self, i: usize, to: usize) -> bool {
        if i >= self.entries.len() || to >= self.entries.len() || i == to {
            return false;
        }
        let entry = self.entries.remove(i);
        self.entries.insert(to, entry);
        true
    }

    /// The built-ins back in their default order and on/off, then the own
    /// tiles in their order; an own tile stays on only while there is room.
    pub fn reset(&mut self) {
        let mut next = ShortcutTiles::default().entries;
        let mut shown = next.iter().filter(|(_, on)| *on).count();
        for (tile, on) in std::mem::take(&mut self.entries) {
            if matches!(tile, Tile::Custom(_)) {
                let on = on && shown < MAX_SHOWN;
                shown += usize::from(on);
                next.push((tile, on));
            }
        }
        self.entries = next;
    }

    /// Removes entry `i` when it is a custom tile.
    pub fn remove(&mut self, i: usize) -> bool {
        if matches!(self.entries.get(i), Some((Tile::Custom(_), _))) {
            self.entries.remove(i);
            true
        } else {
            false
        }
    }

    /// Adds a custom tile at the end: `Some(true)` on, `Some(false)` off
    /// because twelve tiles show already, `None` when the list of own tiles
    /// is full.
    pub fn add_own(&mut self, tile: CustomTile) -> Option<bool> {
        if self.customs() >= MAX_CUSTOM {
            return None;
        }
        let on = !self.full();
        self.entries.push((Tile::Custom(tile), on));
        Some(on)
    }

    /// Adds a custom tile at the end, on; false when the list is full.
    #[cfg_attr(not(debug_assertions), allow(dead_code))] // Only the debug demo adds one.
    pub fn add(&mut self, tile: CustomTile) -> bool {
        if self.customs() >= MAX_CUSTOM {
            return false;
        }
        self.entries.push((Tile::Custom(tile), true));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(label: &str, kind: CustomKind, target: &str) -> CustomTile {
        CustomTile::new(label, kind, target).expect("valid")
    }

    fn entry(tile: &str, on: bool) -> TileEntry {
        TileEntry {
            tile: tile.into(),
            on,
            ..TileEntry::default()
        }
    }

    fn names(t: &ShortcutTiles) -> Vec<String> {
        t.entries()
            .iter()
            .map(|(tile, on)| {
                let name = match tile {
                    Tile::Builtin(s) => shortcut_name(*s).to_owned(),
                    Tile::Custom(c) => format!("+{}", c.label),
                };
                if *on { name } else { format!("({name})") }
            })
            .collect()
    }

    /// The original nine built-ins, as an old config names them.
    fn original_nine() -> &'static [Shortcut] {
        &Shortcut::ALL[..9]
    }

    #[test]
    fn the_default_lists_every_built_in_and_shows_the_default_ones() {
        let t = ShortcutTiles::default();
        let all: Vec<Tile> = Shortcut::ALL.iter().map(|s| Tile::Builtin(*s)).collect();
        let listed: Vec<Tile> = t.entries().iter().map(|(x, _)| x.clone()).collect();
        assert_eq!(listed, all);
        // The original nine, the Shelf and Devices.
        let mut on: Vec<Tile> = original_nine().iter().map(|s| Tile::Builtin(*s)).collect();
        on.push(Tile::Builtin(Shortcut::Go(Page::Shelf)));
        on.push(Tile::Builtin(Shortcut::Go(Page::Devices)));
        assert_eq!(t.visible(), on);
        assert_eq!(t.shown(), 11);
        assert_eq!(Shortcut::ALL.len(), 22);
        assert_eq!(listed.len(), 22);
        assert_eq!(ShortcutTiles::from_saved(&[]), t);
    }

    #[test]
    fn only_the_original_nine_the_shelf_and_devices_are_on_by_default() {
        for s in original_nine() {
            assert!(s.default_on(), "{s:?}");
        }
        assert!(Shortcut::Go(Page::Shelf).default_on());
        assert!(Shortcut::Go(Page::Devices).default_on());
        for page in Page::ALL {
            let go = Shortcut::Go(page);
            if Shortcut::ALL.contains(&go) && !matches!(page, Page::Shelf | Page::Devices) {
                assert!(!go.default_on(), "{page:?}");
            }
        }
        assert_eq!(Shortcut::ALL.iter().filter(|s| s.default_on()).count(), 11);
    }

    #[test]
    fn the_notifikasi_tile_is_available_but_off_by_default_and_joins_off_in_an_old_config() {
        let n = Shortcut::Go(Page::Notifications);
        assert!(Shortcut::ALL.contains(&n), "an available tile");
        assert!(!n.default_on());
        assert_eq!(shortcut_name(n), "GoNotifications");
        assert_eq!(shortcut_from_name(" gonotifications "), Some(n));
        assert_eq!(group(&Tile::Builtin(n)), TileGroup::Pages);
        let t = ShortcutTiles::default();
        assert!(
            t.entries()
                .iter()
                .any(|(x, on)| *x == Tile::Builtin(n) && !on)
        );
        assert!(!t.visible().contains(&Tile::Builtin(n)));
        // An older config that never named it: it joins right after
        // Mixer's tile, and off.
        let saved: Vec<TileEntry> = Shortcut::ALL
            .iter()
            .filter(|s| **s != n)
            .map(|s| entry(&shortcut_name(*s), s.default_on()))
            .collect();
        let t = ShortcutTiles::from_saved(&saved);
        let listed = names(&t);
        let at = listed.iter().position(|x| x == "(GoNotifications)");
        let mixer = listed.iter().position(|x| x == "(GoMixer)");
        assert!(at.is_some(), "{listed:?}");
        assert_eq!(at, mixer.map(|m| m + 1));
        assert_eq!(t.shown(), 11, "the page is as it was");
    }

    #[test]
    fn built_in_names_round_trip_in_any_case() {
        for s in Shortcut::ALL {
            assert_eq!(shortcut_from_name(&shortcut_name(s)), Some(s), "{s:?}");
            assert_eq!(
                shortcut_from_name(&shortcut_name(s).to_uppercase()),
                Some(s),
                "{s:?}"
            );
        }
        assert_eq!(shortcut_name(Shortcut::Go(Page::Shelf)), "GoShelf");
        assert_eq!(shortcut_name(Shortcut::Go(Page::Devices)), "GoDevices");
        assert_eq!(shortcut_name(Shortcut::Go(Page::Cli)), "GoCli");
        assert_eq!(shortcut_name(Shortcut::Go(Page::Arrange)), "GoArrange");
        assert_eq!(
            shortcut_from_name(" goshelf "),
            Some(Shortcut::Go(Page::Shelf))
        );
        // The retired Vale Settings tile is no built-in any more.
        for gone in ["ValeSettings", "valesettings", " VALESETTINGS "] {
            assert_eq!(shortcut_from_name(gone), None, "{gone}");
        }
        // No tile for Search, Shortcuts or Clipboard as a Go; nor a bare Go.
        for bad in ["GoSearch", "GoShortcuts", "GoClipboard", "Go", "GoWeather"] {
            assert_eq!(shortcut_from_name(bad), None, "{bad}");
        }
        // Written back canonically.
        let t = ShortcutTiles::from_saved(&[entry("gocli", true)]);
        assert!(t.to_saved().iter().any(|e| e.tile == "GoCli" && e.on));
    }

    #[test]
    fn an_old_nine_tile_config_gets_the_shelf_and_devices_on() {
        let saved: Vec<TileEntry> = original_nine()
            .iter()
            .map(|s| entry(&shortcut_name(*s), true))
            .collect();
        let t = ShortcutTiles::from_saved(&saved);
        assert_eq!(t.shown(), 11);
        let on = |s: Shortcut| {
            t.entries()
                .iter()
                .any(|(x, on)| *x == Tile::Builtin(s) && *on)
        };
        assert!(on(Shortcut::Go(Page::Shelf)));
        assert!(on(Shortcut::Go(Page::Devices)));
        assert!(!on(Shortcut::Go(Page::Calendar)));
        // Each joins after its predecessor in ALL.
        assert_eq!(
            &names(&t)[8..12],
            ["Lock", "(GoCalendar)", "(GoPomodoro)", "(GoReminders)"]
        );
        // The same config with some off keeps those off.
        let mut saved = saved;
        saved[8].on = false;
        let t = ShortcutTiles::from_saved(&saved);
        assert_eq!(t.shown(), 10);
    }

    #[test]
    fn an_old_config_with_ten_shown_gets_devices_and_one_with_twelve_does_not() {
        // The nine and the Shelf: ten shown.
        let mut saved: Vec<TileEntry> = original_nine()
            .iter()
            .map(|s| entry(&shortcut_name(*s), true))
            .collect();
        saved.push(entry("GoShelf", true));
        let t = ShortcutTiles::from_saved(&saved);
        assert_eq!(t.shown(), 11, "room for one more: Devices is on");
        assert!(
            t.visible()
                .contains(&Tile::Builtin(Shortcut::Go(Page::Devices)))
        );
        // Two more own tiles: twelve shown, so Devices joins off.
        for label in ["Gmail", "Drive"] {
            saved.push(TileEntry {
                tile: CUSTOM.into(),
                on: true,
                label: label.into(),
                kind: "Url".into(),
                target: "https://mail.google.com".into(),
            });
        }
        let t = ShortcutTiles::from_saved(&saved);
        assert_eq!(t.shown(), 12);
        assert!(
            !t.visible()
                .contains(&Tile::Builtin(Shortcut::Go(Page::Devices)))
        );
    }

    #[test]
    fn a_full_twelve_tile_config_keeps_its_twelve() {
        // The nine and three own tiles, all on: the page is full.
        let mut saved: Vec<TileEntry> = original_nine()
            .iter()
            .map(|s| entry(&shortcut_name(*s), true))
            .collect();
        for i in 0..3 {
            saved.push(TileEntry {
                tile: CUSTOM.into(),
                on: true,
                label: format!("T{i}"),
                kind: "App".into(),
                target: r"C:\a.exe".into(),
            });
        }
        let t = ShortcutTiles::from_saved(&saved);
        assert_eq!(t.shown(), MAX_SHOWN);
        assert_eq!(t.customs(), 3);
        for s in [Shortcut::Go(Page::Shelf), Shortcut::Go(Page::Devices)] {
            assert!(
                t.entries()
                    .iter()
                    .any(|(x, on)| *x == Tile::Builtin(s) && !on),
                "{s:?} joins off: no room"
            );
        }
        assert_eq!(t.visible().len(), MAX_SHOWN);
    }

    /// An old config, as 3.12 and earlier wrote it: every built-in named,
    /// `ValeSettings` among them, in the old order.
    fn old_names() -> Vec<&'static str> {
        vec![
            "Screenshot",
            "Clipboard",
            "Search",
            "Emoji",
            "Explorer",
            "Calculator",
            "TaskManager",
            "WindowsSettings",
            "Lock",
            "ValeSettings",
            "GoCalendar",
            "GoPomodoro",
            "GoReminders",
            "GoNotes",
            "GoMedia",
            "GoMixer",
            "GoNotifications",
            "GoShelf",
            "GoSystem",
            "GoDevices",
            "GoCli",
            "GoArrange",
        ]
    }

    #[test]
    fn a_saved_vale_settings_entry_is_dropped_whatever_its_flag_or_case() {
        for (name, on) in [
            ("ValeSettings", true),
            ("ValeSettings", false),
            ("valesettings", true),
            (" VALESETTINGS ", false),
        ] {
            let saved = [
                entry("Lock", true),
                entry(name, on),
                entry("Emoji", false),
                entry("GoShelf", true),
            ];
            let t = ShortcutTiles::from_saved(&saved);
            let listed = names(&t);
            assert!(
                !listed.iter().any(|n| n.to_lowercase().contains("valesettings")),
                "{name}: {listed:?}"
            );
            // The others keep their order and on/off (the missing ones join).
            let at = |n: &str| listed.iter().position(|x| x == n).unwrap();
            assert!(at("Lock") < at("(Emoji)") && at("(Emoji)") < at("GoShelf"), "{listed:?}");
            assert!(
                t.to_saved()
                    .iter()
                    .all(|e| !e.tile.to_lowercase().contains("valesettings"))
            );
            assert_eq!(t.entries().len(), Shortcut::ALL.len());
        }
    }

    #[test]
    fn the_default_tiles_show_eleven_of_twenty_two_built_ins() {
        let t = ShortcutTiles::default();
        assert_eq!(t.shown(), 11);
        assert_eq!(t.entries().len(), 22);
        assert_eq!(Shortcut::ALL.len(), 22);
    }

    #[test]
    fn a_full_twelve_old_config_with_vale_settings_on_shows_eleven_after_the_drop() {
        // Old default: the nine, ValeSettings, the Shelf and Devices on; every
        // built-in is named, so nothing joins and nothing else is switched on.
        let saved: Vec<TileEntry> = old_names()
            .into_iter()
            .map(|n| {
                let on = n == "ValeSettings"
                    || shortcut_from_name(n).is_some_and(|s| s.default_on());
                entry(n, on)
            })
            .collect();
        assert_eq!(saved.iter().filter(|e| e.on).count(), 12);
        let t = ShortcutTiles::from_saved(&saved);
        assert_eq!(t.shown(), 11);
        let on: Vec<Tile> = t.visible();
        let want: Vec<Tile> = Shortcut::ALL
            .iter()
            .filter(|s| s.default_on())
            .map(|s| Tile::Builtin(*s))
            .collect();
        assert_eq!(on, want, "no other tile is switched on");
    }

    #[test]
    fn a_default_on_built_in_the_old_file_never_named_may_take_the_freed_slot() {
        // The rule (unchanged): a built-in the file never names joins right
        // after its predecessor, shown only if it is on by default AND the
        // page has room. Dropping ValeSettings frees a slot, so Devices, which
        // this old file never named, may come in on.
        let mut saved: Vec<TileEntry> = original_nine()
            .iter()
            .map(|s| entry(&shortcut_name(*s), true))
            .collect();
        saved.push(entry("ValeSettings", true));
        saved.push(entry("GoShelf", true));
        saved.push(TileEntry {
            tile: CUSTOM.into(),
            on: true,
            label: "Gmail".into(),
            kind: "Url".into(),
            target: "https://mail.google.com".into(),
        });
        assert_eq!(saved.iter().filter(|e| e.on).count(), 12, "the page was full");
        let t = ShortcutTiles::from_saved(&saved);
        assert_eq!(t.shown(), 12);
        assert!(
            t.visible()
                .contains(&Tile::Builtin(Shortcut::Go(Page::Devices)))
        );
    }

    #[test]
    fn a_list_whose_only_shown_tile_was_vale_settings_is_made_whole_after_the_drop() {
        let saved = [entry("Lock", false), entry("ValeSettings", true)];
        let t = ShortcutTiles::from_saved(&saved);
        // All off after the drop: the default-on built-ins are switched on.
        assert_eq!(t.shown(), 11);
        let want: Vec<Tile> = Shortcut::ALL
            .iter()
            .filter(|s| s.default_on())
            .map(|s| Tile::Builtin(*s))
            .collect();
        let mut got = t.visible();
        got.sort_by_key(|x| match x {
            Tile::Builtin(s) => Shortcut::ALL.iter().position(|a| a == s),
            Tile::Custom(_) => None,
        });
        assert_eq!(got, want);
        let listed = names(&t);
        assert!(listed.contains(&"Lock".to_string()), "{listed:?}");
    }

    #[test]
    fn a_saved_list_keeps_its_order_and_gets_the_missing_built_ins_back() {
        let saved = [
            entry("Lock", true),
            entry("Emoji", false),
            entry("Bogus", true),
        ];
        let t = ShortcutTiles::from_saved(&saved);
        // Each missing one lands right after the one before it in the
        // default order (Screenshot has none before it: the front).
        assert_eq!(
            names(&t),
            [
                "Screenshot",
                "Clipboard",
                "Search",
                "Lock",
                "(GoCalendar)",
                "(GoPomodoro)",
                "(GoReminders)",
                "(GoNotes)",
                "(GoMedia)",
                "(GoMixer)",
                "(GoNotifications)",
                "GoShelf",
                "(GoSystem)",
                "GoDevices",
                "(GoCli)",
                "(GoArrange)",
                "(GoRecord)",
                "(Emoji)",
                "Explorer",
                "Calculator",
                "TaskManager",
                "WindowsSettings"
            ]
        );
    }

    #[test]
    fn repeats_and_a_list_with_everything_off_are_made_whole() {
        let t = ShortcutTiles::from_saved(&[entry("Lock", true), entry("Lock", false)]);
        assert_eq!(
            t.entries()
                .iter()
                .filter(|(x, _)| *x == Tile::Builtin(Shortcut::Lock))
                .count(),
            1
        );
        // Everything off: the default tiles show again.
        let all_off: Vec<TileEntry> = Shortcut::ALL
            .iter()
            .map(|s| entry(&shortcut_name(*s), false))
            .collect();
        assert_eq!(
            ShortcutTiles::from_saved(&all_off).visible(),
            ShortcutTiles::default().visible()
        );
        // So does an old config with the nine off (the new tiles do not
        // count as shown when it is judged).
        let nine_off: Vec<TileEntry> = original_nine()
            .iter()
            .map(|s| entry(&shortcut_name(*s), false))
            .collect();
        let t = ShortcutTiles::from_saved(&nine_off);
        assert_eq!(t.visible(), ShortcutTiles::default().visible());
    }

    #[test]
    fn custom_tiles_are_validated() {
        let ok = |label, kind, target| CustomTile::new(label, kind, target).is_some();
        assert!(ok("Gmail", CustomKind::Url, "https://mail.google.com"));
        assert!(ok("Surel", CustomKind::Url, "mailto:a@b.co"));
        assert!(ok("Proyek", CustomKind::Folder, r"D:\Proyek"));
        assert!(ok(
            "Editor",
            CustomKind::App,
            r"C:\Program Files\Editor\editor.exe"
        ));
        // Not a link this page launches.
        for bad in [
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "ms-settings:",
            "mail.google.com",
            "https://",
            "http:// x",
            "mailto:",
            "mailto:a@b.co?attach=C:\\secret.txt",
            "mailto:a@b.co?body=x",
        ] {
            assert!(!ok("X", CustomKind::Url, bad), "{bad}");
        }
        // An app or folder is an absolute local path, nothing else.
        assert!(ok("Akar", CustomKind::Folder, r"C:\"));
        assert!(ok("Proyek", CustomKind::Folder, r"d:\Proyek\2026"));
        for kind in [CustomKind::App, CustomKind::Folder] {
            for bad in [
                "a.exe",
                r"Proyek\a.exe",
                r"\Windows\notepad.exe",
                r"\\server\share\a.exe",
                r"\\?\C:\a.exe",
                "C:a.exe",
                "//server/share/a.exe",
                r"C:\a.txt:stream.exe",
                r"C:\x\https://evil",
                "file:///C:/a.exe",
                "https://example.com",
                "shell:startup",
                r"%APPDATA%\a.exe",
                r"1:\a.exe",
            ] {
                assert!(!ok("X", kind, bad), "{kind:?} {bad}");
            }
        }
        // Empty, control characters, too long.
        assert!(!ok("", CustomKind::App, r"C:\a.exe"));
        assert!(!ok("  ", CustomKind::App, r"C:\a.exe"));
        assert!(!ok("X", CustomKind::App, ""));
        assert!(!ok("X\n", CustomKind::App, "C:\\a.exe\0"));
        assert!(!ok(
            "Ini label terlalu panjang",
            CustomKind::App,
            r"C:\a.exe"
        ));
        let long = format!(r"C:\{}", "a".repeat(TARGET_MAX));
        assert!(!ok("X", CustomKind::App, &long));
        // Trimmed.
        let t = CustomTile::new("  Gmail ", CustomKind::Url, " https://x.co ").unwrap();
        assert_eq!(
            (t.label.as_str(), t.target.as_str()),
            ("Gmail", "https://x.co")
        );
    }

    #[test]
    fn custom_entries_round_trip_and_bad_ones_are_dropped() {
        let mut t = ShortcutTiles::default();
        assert!(t.add(custom("Gmail", CustomKind::Url, "https://mail.google.com")));
        assert!(t.add(custom("Proyek", CustomKind::Folder, r"D:\Proyek")));
        let saved = t.to_saved();
        assert_eq!(ShortcutTiles::from_saved(&saved), t);
        let json = serde_json::to_string(&saved[Shortcut::ALL.len()]).unwrap();
        assert_eq!(
            json,
            r#"{"tile":"Custom","on":true,"label":"Gmail","kind":"Url","target":"https://mail.google.com"}"#
        );
        // Hand-edited into nonsense: those entries go, the rest stays.
        let mixed = [
            TileEntry {
                tile: CUSTOM.into(),
                on: true,
                label: "Buruk".into(),
                kind: "Url".into(),
                target: "javascript:1".into(),
            },
            TileEntry {
                tile: CUSTOM.into(),
                on: true,
                label: "Tanpa jenis".into(),
                kind: "Perintah".into(),
                target: "calc".into(),
            },
        ];
        assert_eq!(ShortcutTiles::from_saved(&mixed), ShortcutTiles::default());
    }

    #[test]
    fn the_list_and_the_shown_tiles_have_limits() {
        let mut t = ShortcutTiles::default();
        for i in 0..MAX_CUSTOM {
            assert!(t.add(custom(&format!("T{i}"), CustomKind::App, r"C:\a.exe")));
        }
        assert!(
            !t.add(custom("Lebih", CustomKind::App, r"C:\a.exe")),
            "full"
        );
        // 11 built-in on + 8 own, all on: only the first twelve show.
        assert_eq!(t.entries().len(), Shortcut::ALL.len() + MAX_CUSTOM);
        assert_eq!(t.visible().len(), MAX_SHOWN);
        // More customs in a file than fit: the extra are dropped.
        let many: Vec<TileEntry> = (0..MAX_CUSTOM + 3)
            .map(|i| TileEntry {
                tile: CUSTOM.into(),
                on: true,
                label: format!("T{i}"),
                kind: "App".into(),
                target: r"C:\a.exe".into(),
            })
            .collect();
        assert_eq!(ShortcutTiles::from_saved(&many).customs(), MAX_CUSTOM);
    }

    #[test]
    fn a_new_built_in_never_pushes_an_own_tile_off_a_full_page() {
        // A saved list of 8 built-ins and 4 own tiles, all on (12 shown),
        // from before "Search" existed.
        let mut saved: Vec<TileEntry> = original_nine()
            .iter()
            .filter(|s| shortcut_name(**s) != "Search")
            .map(|s| entry(&shortcut_name(*s), true))
            .collect();
        for i in 0..4 {
            saved.push(TileEntry {
                tile: CUSTOM.into(),
                on: true,
                label: format!("T{i}"),
                kind: "App".into(),
                target: r"C:\a.exe".into(),
            });
        }
        let t = ShortcutTiles::from_saved(&saved);
        assert_eq!(t.entries().len(), Shortcut::ALL.len() + 4);
        assert_eq!(t.visible().len(), MAX_SHOWN);
        assert!(
            t.visible()
                .iter()
                .filter(|x| matches!(x, Tile::Custom(_)))
                .count()
                == 4,
            "every own tile still shows"
        );
        assert!(
            t.entries()
                .iter()
                .any(|(x, on)| *x == Tile::Builtin(Shortcut::Search) && !on)
        );
        // With room, the new ones that are on by default come in shown until
        // the page is full (Search, the Shelf, Devices: 8 + 1 own + 3 = 12);
        // a fresh install shows 11.
        let few = ShortcutTiles::from_saved(&saved[..9]);
        assert_eq!(few.shown(), 12);
        assert!(
            few.visible()
                .contains(&Tile::Builtin(Shortcut::Go(Page::Devices)))
        );
        assert_eq!(ShortcutTiles::default().shown(), 11);
    }

    #[test]
    fn toggle_move_and_remove() {
        let mut t = ShortcutTiles::default();
        let before = t.shown();
        assert!(t.toggle(0));
        assert_eq!(t.shown(), before - 1);
        assert!(t.toggle(0), "back on");
        assert!(!t.toggle(99));
        // The last one shown cannot go off.
        let on: Vec<usize> = (1..t.entries().len())
            .filter(|i| t.entries()[*i].1)
            .collect();
        for i in on {
            assert!(t.toggle(i));
        }
        assert!(!t.toggle(0));
        assert_eq!(t.shown(), 1);

        let mut t = ShortcutTiles::default();
        assert!(t.move_shown(0, 1));
        assert_eq!(&names(&t)[..2], ["Clipboard", "Screenshot"]);
        assert!(!t.move_shown(0, -1), "already first");
        assert!(!t.move_shown(99, -1));

        // Only a custom tile can be removed.
        assert!(!t.remove(0));
        t.add(custom("Gmail", CustomKind::Url, "https://mail.google.com"));
        assert!(t.remove(Shortcut::ALL.len()));
        assert_eq!(t.entries().len(), Shortcut::ALL.len());
    }

    #[test]
    fn the_susun_jump_tile_is_off_by_default_and_an_old_config_joins_it_off() {
        assert_eq!(Shortcut::ALL.len(), 22);
        assert!(!Shortcut::Go(Page::Arrange).default_on());
        // A fresh install: the tile exists but is off; the page keeps its eleven.
        let t = ShortcutTiles::default();
        assert!(
            t.entries()
                .iter()
                .any(|(s, on)| *s == Tile::Builtin(Shortcut::Go(Page::Arrange)) && !on)
        );
        assert_eq!(t.shown(), 11);
        // An old config that never heard of it (the 21 built-ins as saved,
        // 11 of them on) gets it back off, right after Cli, and still shows 11.
        let old: Vec<TileEntry> = ShortcutTiles::default()
            .to_saved()
            .into_iter()
            .filter(|e| e.tile != "GoArrange")
            .collect();
        assert_eq!(old.len(), 21);
        let t = ShortcutTiles::from_saved(&old);
        assert_eq!(t.shown(), 11);
        let names = names(&t);
        let at = names
            .iter()
            .position(|n| n == "(GoArrange)")
            .expect("joined off");
        assert_eq!(names[at - 1], "(GoCli)");
    }

    /// The default tiles with the Calendar switched on too (a full page of
    /// 12), the starting point of the limit tests.
    fn full_default() -> ShortcutTiles {
        let mut t = ShortcutTiles::default();
        assert!(t.toggle(9), "the Calendar tile");
        assert_eq!(t.shown(), MAX_SHOWN);
        t
    }

    /// The full page (12 on) plus `own` custom tiles, all on.
    fn with_own(own: usize) -> ShortcutTiles {
        let mut t = full_default();
        for i in 0..own {
            assert!(t.add(custom(&format!("T{i}"), CustomKind::App, r"C:\a.exe")));
        }
        t
    }

    #[test]
    fn no_tile_goes_on_while_twelve_show() {
        // The full default shows 12: the page is full.
        let mut t = with_own(0);
        assert!(t.full());
        assert!(t.toggle(0), "off is always fine");
        assert!(!t.full());
        assert!(t.toggle(0), "back on: 12 again");
        // A new own tile comes in on, past the twelfth: it cannot be
        // switched off and on again to jump the queue.
        assert!(t.add(custom("Baru", CustomKind::App, r"C:\b.exe")));
        let last = t.entries().len() - 1;
        assert!(t.toggle(last), "off");
        assert!(!t.toggle(last), "full: stays off");
        assert!(!t.entries()[last].1);
        // Room again after one goes off.
        assert!(t.toggle(1));
        assert!(t.toggle(last));
        assert_eq!(t.shown(), MAX_SHOWN);
    }

    #[test]
    fn tiles_on_past_the_twelfth_are_marked() {
        // Hand-added: 12 + 3 on, all three past the limit (the own tiles sit
        // after the 22 built-ins, at 22, 23 and 24).
        let t = with_own(3);
        assert_eq!(t.shown(), 15);
        let beyond: Vec<usize> = (0..t.entries().len())
            .filter(|i| t.beyond_limit(*i))
            .collect();
        assert_eq!(beyond, [22, 23, 24]);
        assert_eq!(t.visible().len(), MAX_SHOWN);
        // An entry that is off is never "beyond"; neither is one that shows.
        let mut t = with_own(3);
        assert!(t.toggle(22));
        assert!(!t.beyond_limit(22));
        assert!(t.beyond_limit(23));
        assert!(!t.beyond_limit(0));
        assert!(!t.beyond_limit(99));
    }

    #[test]
    fn move_to_puts_a_tile_at_a_place_of_the_list() {
        let mut t = ShortcutTiles::default();
        assert!(t.move_to(0, 3));
        assert_eq!(
            &names(&t)[..4],
            ["Clipboard", "Search", "Emoji", "Screenshot"]
        );
        assert!(t.move_to(3, 0), "and back");
        assert_eq!(&names(&t)[..2], ["Screenshot", "Clipboard"]);
        assert!(!t.move_to(2, 2), "already there");
        assert!(!t.move_to(0, 99), "out of range");
        assert!(!t.move_to(99, 0), "out of range");
        let last = t.entries().len() - 1;
        assert!(t.move_to(0, last));
        assert_eq!(names(&t)[last], "Screenshot");
        // A flag travels with its tile.
        let mut t = ShortcutTiles::default();
        assert!(t.toggle(1));
        assert!(t.move_to(1, 5));
        assert_eq!(names(&t)[5], "(Clipboard)");
    }

    #[test]
    fn move_shown_steps_over_tiles_that_are_off() {
        let mut t = ShortcutTiles::default();
        // Index 8 is Lock (on), 9..16 are off Go tiles, 16 is GoShelf,
        // 17 GoSystem (off), 18 GoDevices (on, the last shown).
        assert!(
            t.move_shown(16, -1),
            "past the off Go tiles, onto Lock's place"
        );
        assert_eq!(names(&t)[8], "GoShelf");
        assert_eq!(names(&t)[9], "Lock");
        assert!(!t.move_shown(0, -1), "first");
        let mut t = ShortcutTiles::default();
        assert!(!t.move_shown(12, 1), "an off tile does not move");
        let last_on = 18;
        assert!(!t.move_shown(last_on, 1), "last shown");
        assert!(t.move_shown(0, 50), "clamped to the last shown tile");
        assert_eq!(names(&t)[18], "Screenshot");
    }

    #[test]
    fn set_on_follows_the_same_limits_as_toggle() {
        let mut t = ShortcutTiles::default();
        assert!(!t.set_on(0, true), "already on");
        assert!(t.set_on(0, false));
        assert!(!t.set_on(0, false), "already off");
        assert!(t.set_on(0, true));
        assert!(!t.set_on(99, true));
        // Full: stays off. Last shown: stays on.
        let mut t = with_own(1);
        assert!(t.full());
        assert!(!t.set_on(10, true), "page full");
        let mut t = ShortcutTiles::default();
        let on: Vec<usize> = (1..t.entries().len())
            .filter(|i| t.entries()[*i].1)
            .collect();
        for i in on {
            assert!(t.set_on(i, false));
        }
        assert!(!t.set_on(0, false), "the last one shown");
    }

    #[test]
    fn reset_restores_the_built_ins_and_keeps_the_own_tiles_after_them() {
        let mut t = ShortcutTiles::default();
        t.add_own(custom("Gmail", CustomKind::Url, "https://mail.google.com"));
        t.add_own(custom("Proyek", CustomKind::Folder, r"D:\Proyek"));
        t.move_to(0, 5);
        t.toggle(2);
        t.toggle(10);
        t.move_to(20, 0);
        t.reset();
        let defaults = ShortcutTiles::default();
        assert_eq!(t.entries()[..Shortcut::ALL.len()], *defaults.entries());
        assert_eq!(
            &names(&t)[Shortcut::ALL.len()..],
            ["+Gmail", "(+Proyek)"],
            "own tiles in their order, after the built-ins"
        );
        // 11 built-ins on: one own tile fits, the next does not.
        assert_eq!(t.shown(), MAX_SHOWN);
        assert!(!t.entries()[Shortcut::ALL.len() + 1].1, "no room");
        // An own tile that was off stays off.
        let mut t = ShortcutTiles::default();
        t.add_own(custom("Gmail", CustomKind::Url, "https://mail.google.com"));
        t.toggle(Shortcut::ALL.len());
        t.reset();
        assert_eq!(t.shown(), 11);
        // Nothing to do on the default.
        let mut d = ShortcutTiles::default();
        d.reset();
        assert_eq!(d, ShortcutTiles::default());
    }

    #[test]
    fn add_own_is_on_only_while_there_is_room() {
        let mut t = full_default();
        // A full page shows twelve: the first own tile is off already.
        assert!(t.full());
        assert_eq!(
            t.add_own(custom("Gmail", CustomKind::Url, "https://mail.google.com")),
            Some(false),
            "off: twelve show"
        );
        // One built-in hidden makes room for exactly one.
        t.toggle(0);
        assert_eq!(
            t.add_own(custom("Surel", CustomKind::Url, "https://mail.example.com")),
            Some(true)
        );
        assert!(t.full());
        assert_eq!(
            t.add_own(custom("Proyek", CustomKind::Folder, r"D:\Proyek")),
            Some(false),
            "off: twelve show"
        );
        assert_eq!(t.shown(), MAX_SHOWN);
        assert_eq!(t.customs(), 3);
        // The list of own tiles has a limit too.
        let mut t = ShortcutTiles::default();
        for i in 0..MAX_CUSTOM {
            assert!(
                t.add_own(custom(&format!("T{i}"), CustomKind::App, r"C:\a.exe"))
                    .is_some()
            );
        }
        assert_eq!(
            t.add_own(custom("Lebih", CustomKind::App, r"C:\a.exe")),
            None
        );
    }

    #[test]
    fn check_says_which_part_of_a_typed_tile_failed() {
        let url = CustomKind::Url;
        assert!(CustomTile::check("Gmail", url, "https://mail.google.com").is_ok());
        assert_eq!(
            CustomTile::check("", url, "https://mail.google.com"),
            Err(CustomProblem::Label)
        );
        assert_eq!(
            CustomTile::check("Nama yang terlalu panjang", url, "https://x.co"),
            Err(CustomProblem::Label)
        );
        assert_eq!(
            CustomTile::check("Gmail", url, "mail.google.com"),
            Err(CustomProblem::Target)
        );
        assert_eq!(
            CustomTile::check("X", CustomKind::App, "calc.exe"),
            Err(CustomProblem::Target)
        );
        assert_eq!(
            CustomTile::check("", CustomKind::App, "calc.exe"),
            Err(CustomProblem::Label),
            "the label is judged first"
        );
    }

    #[test]
    fn tiles_are_grouped_for_the_available_list() {
        let g = |s| group(&Tile::Builtin(s));
        assert_eq!(g(Shortcut::Clipboard), TileGroup::Pages);
        assert_eq!(g(Shortcut::Go(Page::Mixer)), TileGroup::Pages);
        assert_eq!(g(Shortcut::Lock), TileGroup::Windows);
        assert_eq!(g(Shortcut::Screenshot), TileGroup::Windows);
        assert_eq!(g(Shortcut::Search), TileGroup::Vale);
        let own = custom("Gmail", CustomKind::Url, "https://mail.google.com");
        assert_eq!(group(&Tile::Custom(own)), TileGroup::Own);
    }

    #[test]
    fn names_are_read_in_any_case() {
        let saved = [
            entry("lock", false),
            entry(" SCREENSHOT ", false),
            TileEntry {
                tile: "custom".into(),
                on: true,
                label: "Gmail".into(),
                kind: "url".into(),
                target: "https://mail.google.com".into(),
            },
        ];
        let t = ShortcutTiles::from_saved(&saved);
        let all = names(&t);
        assert_eq!(all[0], "(Lock)", "the first one named stays first");
        assert!(all.contains(&"(Screenshot)".to_owned()));
        assert_eq!(t.customs(), 1);
        assert!(names(&t).contains(&"+Gmail".to_owned()));
        // Each built-in still once, however it was written.
        let twice = ShortcutTiles::from_saved(&[entry("Lock", true), entry("LOCK", false)]);
        assert_eq!(twice.entries().len(), Shortcut::ALL.len());
        // Written back in the canonical spelling.
        assert_eq!(t.to_saved()[0].tile, "Lock");
        assert_eq!(t.to_saved().last().unwrap().tile, "Custom");
    }

    const WANT_SHOWN: [Shortcut; 12] = [
        Shortcut::Go(Page::Calendar),
        Shortcut::Go(Page::Reminders),
        Shortcut::Go(Page::Notes),
        Shortcut::Go(Page::Pomodoro),
        Shortcut::Go(Page::Media),
        Shortcut::Clipboard,
        Shortcut::Go(Page::Shelf),
        Shortcut::Go(Page::Notifications),
        Shortcut::Screenshot,
        Shortcut::Search,
        Shortcut::Go(Page::Record),
        Shortcut::Go(Page::Mixer),
    ];

    #[test]
    fn fresh_shows_the_twelve_main_tiles_in_order() {
        let t = ShortcutTiles::fresh();
        let shown: Vec<Tile> = t
            .entries()
            .iter()
            .filter(|(_, on)| *on)
            .map(|(x, _)| x.clone())
            .collect();
        let want: Vec<Tile> = WANT_SHOWN.iter().map(|s| Tile::Builtin(*s)).collect();
        assert_eq!(shown, want);
        assert_eq!(t.shown(), MAX_SHOWN);
        assert_eq!(t.visible(), want);
    }

    #[test]
    fn fresh_lists_every_built_in_once() {
        let t = ShortcutTiles::fresh();
        let entries = t.entries();
        assert_eq!(entries.len(), Shortcut::ALL.len());
        for s in Shortcut::ALL {
            let tile = Tile::Builtin(s);
            assert_eq!(entries.iter().filter(|(x, _)| *x == tile).count(), 1, "{s:?}");
        }
        let off: Vec<Tile> = entries
            .iter()
            .filter(|(_, on)| !*on)
            .map(|(x, _)| x.clone())
            .collect();
        let want: Vec<Tile> = Shortcut::ALL
            .iter()
            .filter(|s| !WANT_SHOWN.contains(s))
            .map(|s| Tile::Builtin(*s))
            .collect();
        assert_eq!(off, want);
        assert!(entries[MAX_SHOWN..].iter().all(|(_, on)| !*on));
    }

    #[test]
    fn fresh_survives_a_save() {
        assert_eq!(
            ShortcutTiles::from_saved(&ShortcutTiles::fresh().to_saved()),
            ShortcutTiles::fresh()
        );
    }
}
