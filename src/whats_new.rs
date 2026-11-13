//! "What's new": what each release changed, written by hand for the people
//! who use Vale, one entry per version in both languages. Shown after an
//! update (`app_update`) and in Settings → Tentang. A test fails when the
//! version in `Cargo.toml` has no entry, so a release cannot skip it.

use crate::i18n::Lang;
use crate::update::parse_version;

/// One release: 1-5 short points in each language.
pub struct Entry {
    pub version: &'static str,
    pub id: &'static [&'static str],
    pub en: &'static [&'static str],
}

/// Newest first. Add the entry for a version before bumping it.
pub static ENTRIES: &[Entry] = &[Entry {
    version: "5.3.1",
    id: &[
        "Pindah ke PC lain: ekspor dan impor pengaturan di Pengaturan → Tentang.",
        "Link HP bisa lewat Tailscale: pilih Wi-Fi atau Tailscale di tampilan QR.",
    ],
    en: &[
        "Move to another PC: export and import your settings in Settings → About.",
        "The phone link can run over Tailscale: pick Wi-Fi or Tailscale on the QR view.",
    ],
}];

pub fn points(entry: &Entry, lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Id => entry.id,
        Lang::En => entry.en,
    }
}

/// The entries newer than `seen` up to `current`, newest first. When `seen`
/// is not a version (empty, or edited by hand) only `current`'s own entry.
pub fn since(seen: &str, current: &str) -> Vec<&'static Entry> {
    since_in(ENTRIES, seen, current)
}

fn since_in<'a>(entries: &'a [Entry], seen: &str, current: &str) -> Vec<&'a Entry> {
    let Some(current) = parse_version(current) else {
        return Vec::new();
    };
    match parse_version(seen) {
        Some(seen) => entries
            .iter()
            .filter(|e| matches!(parse_version(e.version), Some(v) if v > seen && v <= current))
            .collect(),
        None => entries
            .iter()
            .filter(|e| parse_version(e.version) == Some(current))
            .collect(),
    }
}

/// The newest `n` entries (the Settings card).
pub fn latest(n: usize) -> &'static [Entry] {
    &ENTRIES[..n.min(ENTRIES.len())]
}

/// What a start does about What's new.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartAction {
    /// Nothing: the version was already seen.
    None,
    /// Remember the running version, show nothing (a fresh install or a
    /// downgrade).
    Store,
    /// Remember it and show the notice.
    Show,
}

/// `fresh`: the first run, before onboarding ended. A `seen` that is not a
/// version on an existing install is an upgrade from a release without this
/// feature.
pub fn start_action(seen: &str, current: &str, fresh: bool) -> StartAction {
    let Some(now) = parse_version(current) else {
        return StartAction::None;
    };
    match parse_version(seen) {
        Some(was) if was == now => StartAction::None,
        Some(was) if was > now => StartAction::Store,
        Some(_) => StartAction::Show,
        Option::None if seen.is_empty() && fresh => StartAction::Store,
        Option::None => StartAction::Show,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn entry(version: &'static str) -> Entry {
        Entry {
            version,
            id: &["x"],
            en: &["x"],
        }
    }

    static FIXTURE: [Entry; 4] = [entry("5.3.4"), entry("5.3.3"), entry("5.3.2"), entry("5.3.1")];
    static GAP: [Entry; 2] = [entry("5.3.4"), entry("5.3.2")];

    fn versions(found: Vec<&Entry>) -> Vec<&str> {
        found.into_iter().map(|e| e.version).collect()
    }

    #[test]
    fn table_has_an_entry_for_the_running_version() {
        assert!(
            ENTRIES.iter().any(|e| e.version == env!("CARGO_PKG_VERSION")),
            "write the What's new entry in src/whats_new.rs before bumping the version"
        );
    }

    #[test]
    fn table_is_well_formed() {
        let mut last = None;
        for e in ENTRIES {
            let v = crate::update::parse_version(e.version)
                .unwrap_or_else(|| panic!("{} does not parse", e.version));
            if let Some(last) = last {
                assert!(v < last, "{} is not older than the entry before it", e.version);
            }
            last = Some(v);
            for points in [e.id, e.en] {
                assert!((1..=5).contains(&points.len()), "{}: 1-5 points", e.version);
                for p in points {
                    assert!(!p.trim().is_empty(), "{}: empty point", e.version);
                    assert!(p.chars().count() <= 90, "{}: point too long: {p}", e.version);
                }
            }
        }
    }

    #[test]
    fn since_lists_the_range_newest_first() {
        assert_eq!(
            versions(since_in(&FIXTURE, "5.3.1", "5.3.4")),
            ["5.3.4", "5.3.3", "5.3.2"]
        );
        assert!(since_in(&FIXTURE, "5.3.4", "5.3.4").is_empty());
        assert_eq!(versions(since_in(&GAP, "5.3.1", "5.3.4")), ["5.3.4", "5.3.2"]);
        assert!(since_in(&GAP, "5.3.4", "5.4.0").is_empty());
    }

    #[test]
    fn since_with_unknown_seen_gives_only_the_current_entry() {
        for seen in ["", "abc", "5.3"] {
            assert_eq!(versions(since_in(&FIXTURE, seen, "5.3.3")), ["5.3.3"], "{seen:?}");
            assert!(since_in(&GAP, seen, "5.3.3").is_empty(), "{seen:?}");
        }
    }

    #[test]
    fn since_ahead_of_current_is_empty() {
        assert!(since_in(&FIXTURE, "6.0.0", "5.3.4").is_empty());
    }

    #[test]
    fn start_action_rules() {
        use StartAction::{None, Show, Store};
        let cases = [
            ("5.3.1", "5.3.1", false, None),
            // A fresh install that quit before onboarding ended: stays silent.
            ("5.3.1", "5.3.1", true, None),
            ("", "5.3.1", true, Store),
            ("", "5.3.1", false, Show),
            ("abc", "5.3.1", false, Show),
            ("5.3.0", "5.3.1", false, Show),
            ("5.3.0", "5.3.1", true, Show),
            ("6.0.0", "5.3.1", false, Store),
            ("5.3.0", "not-a-version", false, None),
        ];
        for (seen, current, fresh, want) in cases {
            assert_eq!(start_action(seen, current, fresh), want, "{seen:?} {current:?} {fresh}");
        }
    }

    #[test]
    fn latest_clamps() {
        assert!(latest(0).is_empty());
        assert_eq!(latest(99).len(), ENTRIES.len());
    }
}
