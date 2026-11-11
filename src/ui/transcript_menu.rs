//! The right-click menu of a row on the Rekam page, in Vale's own dropdown
//! (`menu_pop`): what a row can do about its transcript depends on its mark.
//! Nothing yet: "Bikin transkrip". Waiting or running: "Lihat transkrip" and
//! "Batalkan transkrip". Done or failed: "Lihat transkrip" and one "Transkrip
//! ulang" per installed model, the active one first and checked.

use std::path::PathBuf;

use windows::Win32::Foundation::HWND;

use crate::capture::Rect;
use crate::config::UiFont;
use crate::i18n::{Key, Lang, text};
use crate::model::RowTranscript;
use crate::transcribe::catalog::{self, Model};
use crate::ui::menu_pop::{self, Align, Done, Row};

const VIEW: usize = 1;
const MAKE: usize = 2;
const CANCEL: usize = 3;
/// "Transkrip ulang" rows: this plus the model's place in `catalog::MODELS`.
const REDO_FIRST: usize = 10;

/// What the dropdown hands back with its `Done`: the recording it was for.
struct Context {
    audio: PathBuf,
}

/// What a picked row asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuChoice {
    /// Open the transcript window.
    View(PathBuf),
    /// Queue a transcript with the chosen model.
    Transcribe(PathBuf),
    /// Queue it again with this model.
    Redo(PathBuf, &'static Model),
    /// Take it out of the queue (a running job stops).
    Cancel(PathBuf),
}

/// The rows of a row with `mark`. `installed` are the models on disk and
/// `active` is the one Settings has chosen. `Off` has no menu.
pub fn rows(
    lang: Lang,
    mark: RowTranscript,
    installed: &[&'static Model],
    active: &'static Model,
) -> Vec<Row> {
    let item = |id: usize, key: Key| Row::Item {
        id,
        label: text(lang, key).to_owned(),
        detail: None,
        checked: false,
    };
    match mark {
        RowTranscript::Off => Vec::new(),
        RowTranscript::None => vec![item(MAKE, Key::TrMenuMake)],
        RowTranscript::Queued | RowTranscript::Waiting | RowTranscript::Running(_) => vec![
            item(VIEW, Key::TrMenuView),
            Row::Separator,
            item(CANCEL, Key::TrMenuCancel),
        ],
        RowTranscript::Done | RowTranscript::Failed => {
            let mut rows = vec![item(VIEW, Key::TrMenuView)];
            // The active model first, when it is installed.
            let (first, rest): (Vec<_>, Vec<_>) =
                installed.iter().partition(|m| m.key == active.key);
            let models: Vec<&&'static Model> = first.iter().chain(rest.iter()).collect();
            if !models.is_empty() {
                rows.push(Row::Separator);
            }
            for m in models {
                let Some(place) = catalog::MODELS.iter().position(|c| c.key == m.key) else {
                    continue;
                };
                rows.push(Row::Item {
                    id: REDO_FIRST + place,
                    label: text(lang, Key::TrMenuRedo).to_owned(),
                    detail: Some(m.name.to_owned()),
                    checked: m.key == active.key,
                });
            }
            rows
        }
    }
}

/// Opens the menu of the recording `audio` under `anchor` (screen px) for
/// `owner`, which gets `menu_pop::WM_APP_MENU_DONE` when it goes (read it with
/// `picked`). Returns whether it opened.
pub fn open(
    owner: HWND,
    anchor: Rect,
    align: Align,
    audio: PathBuf,
    rows: Vec<Row>,
    ui: UiFont,
) -> bool {
    menu_pop::open(owner, anchor, align, rows, ui, Box::new(Context { audio }))
}

/// The pick of a transcript menu's `Done`. `None` when nothing was picked or
/// the `Done` is another dropdown's.
pub fn picked(done: &Done) -> Option<MenuChoice> {
    let ctx = done.context.downcast_ref::<Context>()?;
    let audio = ctx.audio.clone();
    match done.pick? {
        VIEW => Some(MenuChoice::View(audio)),
        MAKE => Some(MenuChoice::Transcribe(audio)),
        CANCEL => Some(MenuChoice::Cancel(audio)),
        id => {
            let model = catalog::MODELS.get(id.checked_sub(REDO_FIRST)?)?;
            Some(MenuChoice::Redo(audio, model))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RowTranscript::{Done as Finished, Failed, None as Nothing, Off, Queued, Running, Waiting};

    fn shape(rows: &[Row]) -> Vec<(usize, String, Option<String>, bool)> {
        rows.iter()
            .map(|r| match r {
                Row::Item { id, label, detail, checked } => (*id, label.clone(), detail.clone(), *checked),
                Row::Separator => (0, "-".into(), None, false),
            })
            .collect()
    }

    fn item(id: usize, label: &str) -> (usize, String, Option<String>, bool) {
        (id, label.into(), None, false)
    }

    fn sep() -> (usize, String, Option<String>, bool) {
        (0, "-".into(), None, false)
    }

    #[test]
    fn menu_rows_per_mark() {
        let (large, small) = (&catalog::MODELS[0], &catalog::MODELS[1]);
        let both = [large, small];
        let id = |m: RowTranscript| shape(&rows(Lang::Id, m, &both, small));
        // Nothing yet: make one.
        assert_eq!(id(Nothing), [item(MAKE, "Bikin transkrip")]);
        // Transcripts off: no menu at all.
        assert!(rows(Lang::Id, Off, &both, small).is_empty());
        // In the queue or running: view and cancel.
        for mark in [Queued, Waiting, Running(40)] {
            assert_eq!(
                id(mark),
                [item(VIEW, "Lihat transkrip"), sep(), item(CANCEL, "Batalkan transkrip")],
                "{mark:?}"
            );
        }
        // Done or failed: view, then one row per installed model, the active
        // one (small) first and checked, the model's name as its detail.
        for mark in [Finished, Failed] {
            assert_eq!(
                id(mark),
                [
                    item(VIEW, "Lihat transkrip"),
                    sep(),
                    (REDO_FIRST + 1, "Transkrip ulang".into(), Some(small.name.into()), true),
                    (REDO_FIRST, "Transkrip ulang".into(), Some(large.name.into()), false),
                ],
                "{mark:?}"
            );
        }
        // English, and only the installed models.
        let en = shape(&rows(Lang::En, Finished, &[large], small));
        assert_eq!(
            en,
            [
                item(VIEW, "View transcript"),
                sep(),
                (REDO_FIRST, "Transcribe again".into(), Some(large.name.into()), false),
            ]
        );
        assert_eq!(shape(&rows(Lang::En, Nothing, &both, small)), [item(MAKE, "Make a transcript")]);
        assert_eq!(
            shape(&rows(Lang::En, Queued, &both, small))[2],
            item(CANCEL, "Cancel the transcript")
        );
        // No model installed: just the view row, no dangling separator.
        assert_eq!(shape(&rows(Lang::Id, Finished, &[], small)), [item(VIEW, "Lihat transkrip")]);
    }

    #[test]
    fn picked_maps_each_row() {
        let audio = PathBuf::from(r"C:\r\a.m4a");
        let done = |pick: Option<usize>| Done {
            pick,
            context: Box::new(Context { audio: audio.clone() }),
        };
        assert_eq!(picked(&done(Some(VIEW))), Some(MenuChoice::View(audio.clone())));
        assert_eq!(picked(&done(Some(MAKE))), Some(MenuChoice::Transcribe(audio.clone())));
        assert_eq!(picked(&done(Some(CANCEL))), Some(MenuChoice::Cancel(audio.clone())));
        for (i, m) in catalog::MODELS.iter().enumerate() {
            assert_eq!(
                picked(&done(Some(REDO_FIRST + i))),
                Some(MenuChoice::Redo(audio.clone(), m))
            );
        }
        // Dismissed, an id nobody offered, a model past the catalog.
        assert_eq!(picked(&done(None)), None);
        assert_eq!(picked(&done(Some(0))), None);
        assert_eq!(picked(&done(Some(REDO_FIRST + catalog::MODELS.len()))), None);
        // The device dropdown's `Done` is not ours: its path still runs.
        let device = Done { pick: Some(1), context: Box::new(5u8) };
        assert_eq!(picked(&device), None);
    }
}
