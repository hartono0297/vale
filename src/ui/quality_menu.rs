//! The video quality choice of the Rekam page's Layar tab in Vale's own
//! dropdown (`menu_pop`): the four sizes, a separator, then the two frame
//! rates, the current size and rate checked. It opens under the quality row;
//! a pick is saved like Settings' "Kualitas video" and "Gambar per detik".

use windows::Win32::Foundation::HWND;

use crate::capture::Rect;
use crate::config::UiFont;
use crate::i18n::{Key, Lang, text};
use crate::recorder::prefs::VideoQuality;
use crate::ui::menu_pop::{self, Align, Done, Row};

/// The frame rates, in the order Settings shows them.
const FPS: [u32; 2] = [30, 60];
/// Quality rows have ids `QUALITY_FIRST..`, frame rate rows `FPS_FIRST..`.
const QUALITY_FIRST: usize = 1;
const FPS_FIRST: usize = 10;

/// What a picked row asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPick {
    Quality(VideoQuality),
    Fps(u32),
}

/// What the dropdown hands back with its `Done`.
struct Context;

fn quality_label(lang: Lang, q: VideoQuality) -> String {
    match q.max_short() {
        Some(px) => format!("{px}p"),
        None => text(lang, Key::ScreenSettingsQualityOriginal).to_owned(),
    }
}

/// The rows: every quality, a separator, every frame rate; `current` checked.
fn rows(lang: Lang, current: (VideoQuality, u32)) -> Vec<Row> {
    let mut rows: Vec<Row> = VideoQuality::ALL
        .iter()
        .enumerate()
        .map(|(i, &q)| Row::Item {
            id: QUALITY_FIRST + i,
            label: quality_label(lang, q),
            detail: None,
            checked: q == current.0,
        })
        .collect();
    rows.push(Row::Separator);
    rows.extend(FPS.iter().enumerate().map(|(i, &fps)| Row::Item {
        id: FPS_FIRST + i,
        label: format!("{fps} fps"),
        detail: None,
        checked: fps == current.1,
    }));
    rows
}

/// The pick a row id stands for; `None` for an id of no row.
fn pick_of(id: usize) -> Option<QualityPick> {
    if let Some(i) = id.checked_sub(QUALITY_FIRST)
        && let Some(&q) = VideoQuality::ALL.get(i)
    {
        return Some(QualityPick::Quality(q));
    }
    let i = id.checked_sub(FPS_FIRST)?;
    FPS.get(i).map(|&fps| QualityPick::Fps(fps))
}

/// Opens the dropdown under `anchor` for `owner`, which gets
/// `menu_pop::WM_APP_MENU_DONE` when it goes (read it with `picked`).
/// Returns whether it opened.
pub fn open(
    owner: HWND,
    anchor: Rect,
    align: Align,
    current: (VideoQuality, u32),
    lang: Lang,
    ui: UiFont,
) -> bool {
    menu_pop::open(owner, anchor, align, rows(lang, current), ui, Box::new(Context))
}

/// The pick of a quality dropdown's `Done`; `None` when nothing was picked or
/// the `Done` is another dropdown's.
pub fn picked(done: &Done) -> Option<QualityPick> {
    done.context.downcast_ref::<Context>()?;
    pick_of(done.pick?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_menu_rows_and_picks_map_back() {
        let rows = rows(Lang::En, (VideoQuality::P1080, 30));
        assert_eq!(rows.len(), 7);
        let items: Vec<(usize, bool)> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Item { id, checked, .. } => Some((*id, *checked)),
                Row::Separator => None,
            })
            .collect();
        assert_eq!(items.len(), 6);
        assert!(matches!(rows[4], Row::Separator));
        let checked: Vec<QualityPick> = items
            .iter()
            .filter(|(_, c)| *c)
            .filter_map(|(id, _)| pick_of(*id))
            .collect();
        assert_eq!(
            checked,
            [QualityPick::Quality(VideoQuality::P1080), QualityPick::Fps(30)]
        );
        for (id, _) in &items {
            let done = Done {
                pick: Some(*id),
                context: Box::new(Context),
            };
            assert_eq!(picked(&done), pick_of(*id));
            assert!(picked(&done).is_some());
        }
        let picks: Vec<QualityPick> = items.iter().filter_map(|(id, _)| pick_of(*id)).collect();
        assert_eq!(picks[..4], VideoQuality::ALL.map(QualityPick::Quality));
        assert_eq!(picks[4..], [QualityPick::Fps(30), QualityPick::Fps(60)]);
        let none = Done {
            pick: None,
            context: Box::new(Context),
        };
        assert_eq!(picked(&none), None);
        let other = Done {
            pick: Some(1),
            context: Box::new(5u8),
        };
        assert_eq!(picked(&other), None);
    }
}
