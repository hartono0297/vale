//! The recording device choice in Vale's own dropdown (`menu_pop`): "Ikuti
//! Windows" with the default device's name under it, a separator, then every
//! active endpoint by name, the current choice checked. It opens from the
//! Rekam page (under the device name) and from Settings → Fitur → Rekaman
//! (under the row's "Ganti"); the endpoints are read as it opens, never at
//! idle.

use windows::Win32::Foundation::HWND;

use crate::capture::Rect;
use crate::config::UiFont;
use crate::i18n::{Key, Lang, text};
use crate::recorder::Source;
use crate::recorder::devices::{self, Endpoint};
use crate::ui::menu_pop::{self, Align, Done, Row};

/// The "Ikuti Windows" row's id; the endpoints follow from `FIRST`.
const FOLLOW: usize = 1;
const FIRST: usize = 2;

/// What a device dropdown hands back with its `Done`: the source it was for
/// and the endpoints it listed, which its ids index.
struct Context {
    source: Source,
    endpoints: Vec<Endpoint>,
}

/// The rows: "Ikuti Windows" (the default's name as its second line), a
/// separator, then every active endpoint by name. The check is on the current
/// choice: `current` is the endpoint id chosen, `None` for following Windows.
/// A choice that is not among `endpoints` (the device is unplugged) has no
/// check.
fn rows(
    lang: Lang,
    default: Option<&Endpoint>,
    endpoints: &[Endpoint],
    current: Option<&str>,
) -> Vec<Row> {
    let mut rows = vec![Row::Item {
        id: FOLLOW,
        label: text(lang, Key::RecFollowWindows).to_owned(),
        detail: default.map(|d| d.name.clone()),
        checked: current.is_none(),
    }];
    if !endpoints.is_empty() {
        rows.push(Row::Separator);
    }
    for (i, e) in endpoints.iter().enumerate() {
        rows.push(Row::Item {
            id: FIRST + i,
            label: e.name.clone(),
            detail: None,
            checked: current == Some(e.id.as_str()),
        });
    }
    rows
}

/// The choice a row id stands for: `Some(None)` follows Windows,
/// `Some(Some(id))` is an endpoint of the list the rows were built from,
/// `None` is no pick.
fn choice(id: usize, endpoints: &[Endpoint]) -> Option<Option<String>> {
    if id == FOLLOW {
        return Some(None);
    }
    let e = endpoints.get(id.checked_sub(FIRST)?)?;
    Some(Some(e.id.clone()))
}

/// Opens the device dropdown of `source` under `anchor` (screen px, lined up
/// with its `align` edge) for `owner`, which gets `menu_pop::WM_APP_MENU_DONE` when it goes (read it with
/// `picked`). `current` is the endpoint id chosen (`None`: follow Windows).
/// Returns whether it opened.
pub fn open(
    owner: HWND,
    anchor: Rect,
    align: Align,
    source: Source,
    current: Option<&str>,
    lang: Lang,
    ui: UiFont,
) -> bool {
    let endpoints = devices::list(source);
    let default = devices::default_endpoint(source);
    let rows = rows(lang, default.as_ref(), &endpoints, current);
    menu_pop::open(
        owner,
        anchor,
        align,
        rows,
        ui,
        Box::new(Context { source, endpoints }),
    )
}

/// The pick of a device dropdown's `Done`: the source and its new choice
/// (`None` inside follows Windows). `None` when nothing was picked or the
/// `Done` is another dropdown's.
pub fn picked(done: &Done) -> Option<(Source, Option<String>)> {
    let ctx = done.context.downcast_ref::<Context>()?;
    Some((ctx.source, choice(done.pick?, &ctx.endpoints)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: &str, name: &str) -> Endpoint {
        Endpoint {
            id: id.into(),
            name: name.into(),
        }
    }

    fn labels(rows: &[Row]) -> Vec<(String, Option<String>, bool)> {
        rows.iter()
            .map(|r| match r {
                Row::Item {
                    label,
                    detail,
                    checked,
                    ..
                } => (label.clone(), detail.clone(), *checked),
                Row::Separator => ("-".into(), None, false),
            })
            .collect()
    }

    #[test]
    fn the_list_leads_with_follow_windows_then_the_endpoints() {
        let list = [endpoint("a", "Headset (USB)"), endpoint("b", "Mic Q&A")];
        let default = endpoint("b", "Mic Q&A");
        let rows = rows(Lang::Id, Some(&default), &list, None);
        assert_eq!(
            labels(&rows),
            [
                (
                    "Ikuti Windows".to_string(),
                    Some("Mic Q&A".to_string()),
                    true
                ),
                ("-".to_string(), None, false),
                ("Headset (USB)".to_string(), None, false),
                // Drawn as it is: no ampersand doubling as a native menu needs.
                ("Mic Q&A".to_string(), None, false),
            ]
        );
        let rows = super::rows(Lang::En, Some(&default), &list, Some("a"));
        let l = labels(&rows);
        assert_eq!(
            l[0],
            (
                "Follow Windows".to_string(),
                Some("Mic Q&A".to_string()),
                false
            )
        );
        assert!(l[2].2);
        assert!(!l[3].2);
    }

    #[test]
    fn the_list_copes_with_no_default_no_endpoints_and_a_missing_choice() {
        let rows = rows(Lang::En, None, &[], None);
        assert_eq!(labels(&rows), [("Follow Windows".to_string(), None, true)]);
        // The chosen device is unplugged: no check anywhere.
        let list = [endpoint("a", "Headset")];
        let rows = super::rows(Lang::Id, None, &list, Some("gone"));
        assert!(labels(&rows).iter().all(|(_, _, checked)| !checked));
    }

    #[test]
    fn a_row_id_maps_back_to_its_choice() {
        let list = [endpoint("a", "A"), endpoint("b", "B")];
        let rows = rows(Lang::En, None, &list, None);
        let ids: Vec<usize> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Item { id, .. } => Some(*id),
                Row::Separator => None,
            })
            .collect();
        assert_eq!(ids, [1, 2, 3]);
        assert_eq!(choice(1, &list), Some(None));
        assert_eq!(choice(2, &list), Some(Some("a".to_string())));
        assert_eq!(choice(3, &list), Some(Some("b".to_string())));
        // Nothing chosen (0), or an id past the list.
        assert_eq!(choice(0, &list), None);
        assert_eq!(choice(4, &list), None);
    }

    #[test]
    fn a_done_gives_the_source_and_its_pick_and_ignores_other_dropdowns() {
        let ctx = || {
            Box::new(Context {
                source: Source::System,
                endpoints: vec![endpoint("a", "A")],
            })
        };
        let done = Done {
            pick: Some(2),
            context: ctx(),
        };
        assert_eq!(picked(&done), Some((Source::System, Some("a".to_string()))));
        let done = Done {
            pick: Some(FOLLOW),
            context: ctx(),
        };
        assert_eq!(picked(&done), Some((Source::System, None)));
        // Dismissed: no pick.
        let done = Done {
            pick: None,
            context: ctx(),
        };
        assert_eq!(picked(&done), None);
        // Another dropdown's `Done`.
        let other = Done {
            pick: Some(1),
            context: Box::new(5u8),
        };
        assert_eq!(picked(&other), None);
    }
}
