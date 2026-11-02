//! Which monitor the island shows on: the list Windows reports and the pure
//! rule that turns the saved choice into one of them. The Win32 side that fills
//! the list is `arrange_win::displays`; nothing here touches the system, so
//! all of it is tested.
//!
//! A monitor is identified by its device path (`DISPLAYCONFIG_TARGET_DEVICE_NAME`),
//! not by `\\.\DISPLAYn`, which Windows renumbers when a screen is replugged. Two
//! screens of the same model have the same friendly name but never the same path.

use crate::arrange::Monitor;
use crate::config::NotchMonitor;
use crate::i18n::{self, Key};

/// One connected display, as `arrange_win::displays` lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Display {
    /// Device name, rect, work area and DPI (physical px).
    pub monitor: Monitor,
    /// The `HMONITOR`, only good while the display stays connected.
    pub hmonitor: isize,
    /// Windows' primary monitor (`MONITORINFOF_PRIMARY`).
    pub primary: bool,
    /// The monitor's device path; empty when Windows would not say. A display
    /// without one is listed but cannot be chosen (nothing to find it by later).
    pub path: String,
    /// The monitor's friendly name ("DELL U2723QE"); empty when unknown.
    pub name: String,
    /// 1-based place in the left to right, then top to bottom order (the same
    /// order as `arrange_win::monitors`).
    pub number: usize,
}

impl Display {
    /// Can be picked in Settings and the tray: it has a path to remember.
    pub fn selectable(&self) -> bool {
        !self.path.is_empty()
    }
}

/// The name to show: the friendly one, or "Monitor N" when there is none.
pub fn display_name(d: &Display) -> String {
    if d.name.trim().is_empty() {
        format!("{} {}", i18n::tr(Key::MonitorNumbered), d.number)
    } else {
        d.name.clone()
    }
}

/// Two device paths name the same monitor (any case; an empty one names none).
pub fn same_path(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && a.to_lowercase() == b.to_lowercase()
}

/// The index of the display with this path, if connected.
pub fn find_path(list: &[Display], path: &str) -> Option<usize> {
    list.iter().position(|d| same_path(&d.path, path))
}

/// The primary display, else the first; `None` for an empty list.
fn primary_of(primaries: impl ExactSizeIterator<Item = bool>) -> Option<usize> {
    if primaries.len() == 0 {
        return None;
    }
    let mut first_primary = None;
    for (i, p) in primaries.enumerate() {
        if p {
            first_primary = Some(i);
            break;
        }
    }
    Some(first_primary.unwrap_or(0))
}

/// Which display the island goes on. Primary follows whatever is primary now.
/// A chosen display that is not connected falls back to the primary and the
/// choice stays as saved, so it comes back when the screen does. `None` only
/// for an empty list (the caller then asks Windows for the primary itself).
pub fn resolve(choice: &NotchMonitor, list: &[Display]) -> Option<usize> {
    resolve_paths(
        choice,
        &list
            .iter()
            .map(|d| (d.path.as_str(), d.primary))
            .collect::<Vec<_>>(),
    )
}

/// `resolve` over just the `(device path, primary)` of each display, for the
/// Settings layout, which only has those.
pub fn resolve_paths(choice: &NotchMonitor, list: &[(&str, bool)]) -> Option<usize> {
    let primary = || primary_of(list.iter().map(|d| d.1));
    match choice {
        NotchMonitor::Primary => primary(),
        NotchMonitor::Chosen { path, .. } => list
            .iter()
            .position(|d| same_path(d.0, path))
            .or_else(primary),
    }
}

/// True when the saved choice is a monitor that is not connected right now.
pub fn saved_missing(choice: &NotchMonitor, list: &[Display]) -> bool {
    match choice {
        NotchMonitor::Primary => false,
        NotchMonitor::Chosen { path, .. } => find_path(list, path).is_none(),
    }
}

/// The choice that picks `d`; `None` when it cannot be chosen.
pub fn choice_for(d: &Display) -> Option<NotchMonitor> {
    d.selectable().then(|| NotchMonitor::Chosen {
        path: d.path.clone(),
        name: d.name.clone(),
    })
}

/// Whether the window must be placed again after a re-enumeration: the display
/// it should be on differs from the one it is on in rect, dpi or path.
pub fn moved(current: Option<&Display>, wanted: Option<&Display>) -> bool {
    match (current, wanted) {
        (None, None) => false,
        (Some(a), Some(b)) => {
            a.monitor.rect != b.monitor.rect || a.monitor.dpi != b.monitor.dpi || a.path != b.path
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrange::Rect;

    fn disp(n: usize, x: i32, primary: bool, path: &str, name: &str) -> Display {
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
            name: name.into(),
            number: n,
        }
    }

    fn three() -> Vec<Display> {
        vec![
            disp(1, -1920, false, r"\\?\DISPLAY#AAA#1", "DELL U2415"),
            disp(2, 0, true, r"\\?\DISPLAY#BBB#2", "LG 27UL"),
            disp(3, 1920, false, r"\\?\DISPLAY#CCC#3", "DELL U2415"),
        ]
    }

    fn chosen(path: &str) -> NotchMonitor {
        NotchMonitor::Chosen {
            path: path.into(),
            name: "x".into(),
        }
    }

    #[test]
    fn primary_is_the_default() {
        assert_eq!(NotchMonitor::default(), NotchMonitor::Primary);
        assert_eq!(resolve(&NotchMonitor::default(), &three()), Some(1));
    }

    #[test]
    fn a_list_without_a_primary_flag_gives_the_first() {
        let mut l = three();
        l[1].primary = false;
        assert_eq!(resolve(&NotchMonitor::Primary, &l), Some(0));
    }

    #[test]
    fn the_chosen_monitor_is_used_when_present() {
        let l = three();
        assert_eq!(resolve(&chosen(r"\\?\DISPLAY#CCC#3"), &l), Some(2));
        assert!(!saved_missing(&chosen(r"\\?\DISPLAY#CCC#3"), &l));
        // The path matches in any case.
        assert_eq!(resolve(&chosen(r"\\?\display#ccc#3"), &l), Some(2));
    }

    #[test]
    fn a_missing_chosen_monitor_falls_back_to_primary_and_stays_saved() {
        let l = three();
        let choice = chosen(r"\\?\DISPLAY#GONE#9");
        assert_eq!(resolve(&choice, &l), Some(1));
        assert!(saved_missing(&choice, &l));
        // Resolving never changes the choice (it is borrowed, but be explicit).
        assert_eq!(choice, chosen(r"\\?\DISPLAY#GONE#9"));
    }

    #[test]
    fn the_chosen_monitor_comes_back_and_is_used_again() {
        let choice = chosen(r"\\?\DISPLAY#CCC#3");
        let mut l = three();
        let gone = l.pop().unwrap();
        assert_eq!(resolve(&choice, &l), Some(1));
        assert!(saved_missing(&choice, &l));
        l.push(gone);
        assert_eq!(resolve(&choice, &l), Some(2));
        assert!(!saved_missing(&choice, &l));
    }

    #[test]
    fn primary_follows_a_primary_change() {
        let mut l = three();
        assert_eq!(resolve(&NotchMonitor::Primary, &l), Some(1));
        l[1].primary = false;
        l[2].primary = true;
        assert_eq!(resolve(&NotchMonitor::Primary, &l), Some(2));
    }

    #[test]
    fn two_displays_with_the_same_name_resolve_by_path() {
        let l = three();
        assert_eq!(l[0].name, l[2].name);
        assert_eq!(resolve(&chosen(r"\\?\DISPLAY#AAA#1"), &l), Some(0));
        assert_eq!(resolve(&chosen(r"\\?\DISPLAY#CCC#3"), &l), Some(2));
    }

    #[test]
    fn a_displays_without_a_path_never_matches_an_empty_saved_path() {
        let mut l = three();
        l[0].path.clear();
        assert_eq!(find_path(&l, ""), None);
        assert_eq!(resolve(&chosen(""), &l), Some(1));
        assert!(!l[0].selectable());
        assert_eq!(choice_for(&l[0]), None);
        assert_eq!(
            choice_for(&l[1]),
            Some(NotchMonitor::Chosen {
                path: r"\\?\DISPLAY#BBB#2".into(),
                name: "LG 27UL".into()
            })
        );
    }

    #[test]
    fn garbage_config_resolves_to_primary() {
        let c: crate::config::Config =
            serde_json::from_str(r#"{"notch_monitor":{"path":42}}"#).unwrap();
        assert_eq!(resolve(&c.notch_monitor, &three()), Some(1));
        let c: crate::config::Config = serde_json::from_str(r#"{"notch_monitor":"???"}"#).unwrap();
        assert_eq!(resolve(&c.notch_monitor, &three()), Some(1));
    }

    #[test]
    fn an_empty_list_resolves_to_nothing() {
        assert_eq!(resolve(&NotchMonitor::Primary, &[]), None);
        assert_eq!(resolve(&chosen("p"), &[]), None);
    }

    #[test]
    fn the_name_falls_back_to_the_number() {
        let mut d = disp(2, 0, true, "p", "LG 27UL");
        assert_eq!(display_name(&d), "LG 27UL");
        d.name.clear();
        assert_eq!(display_name(&d), "Monitor 2");
        d.name = "  ".into();
        assert_eq!(display_name(&d), "Monitor 2");
    }

    #[test]
    fn moved_compares_rect_dpi_and_path() {
        let l = three();
        assert!(!moved(Some(&l[1]), Some(&l[1].clone())));
        assert!(moved(Some(&l[1]), Some(&l[2])));
        let mut dpi = l[1].clone();
        dpi.monitor.dpi = 144;
        assert!(moved(Some(&l[1]), Some(&dpi)));
        let mut path = l[1].clone();
        path.path = "other".into();
        assert!(moved(Some(&l[1]), Some(&path)));
        assert!(moved(None, Some(&l[0])));
        assert!(moved(Some(&l[0]), None));
        assert!(!moved(None, None));
    }
}
