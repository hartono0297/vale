//! The island's side of "Ekspor / Impor pengaturan": the file dialogs, the
//! checked file waiting for the owner's confirmation, and the restart that
//! applies it. The file logic is in `config_transfer`; Settings only draws
//! `TransferView` and posts the actions. A child of `app`, so it works on
//! `App` directly.

use std::io::Read;

use chrono::Local;

use super::{App, DEFERRED_SHUTDOWN, update_page};
use crate::config;
use crate::config_transfer::{self, ImportError, Incoming, MAX_BYTES};
use crate::i18n::{self, Key, Lang};
use crate::model::{Event, NoticeKind};
use crate::ui::settings::{self, TransferView};

/// What the import card has: what it shows and the checked file it holds.
#[derive(Default)]
pub(super) struct Transfer {
    pub(super) view: TransferView,
    pending: Option<Incoming>,
}

/// What "Impor" may do when the owner confirms.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Gate {
    /// No file is waiting: ignore the click.
    Nothing,
    /// A recording is running or being saved.
    Busy,
    Go,
}

pub(super) fn import_gate(has_pending: bool, audio_busy: bool, screen_busy: bool) -> Gate {
    if !has_pending {
        Gate::Nothing
    } else if audio_busy || screen_busy {
        Gate::Busy
    } else {
        Gate::Go
    }
}

/// The card's state for a file that was refused.
pub(super) fn view_for_error(lang: Lang, e: &ImportError) -> TransferView {
    TransferView::Refused(match e {
        ImportError::NotVale => i18n::text(lang, Key::NotValeFile).into(),
        ImportError::Newer { app_version } => i18n::settings_file_newer(lang, app_version),
    })
}

impl App {
    /// "Ekspor": the save dialog, named for today.
    pub(super) fn config_export(&mut self) {
        let name =
            config_transfer::default_export_name(self.config.language, Local::now().date_naive());
        settings::pick_export_path(self.hwnd, name);
    }

    /// The save dialog's path: the settings as they are now go there.
    pub(super) fn config_export_to(&mut self, path: std::path::PathBuf) {
        self.save_config();
        let written =
            config_transfer::write_export(&path, &self.config, Local::now().fixed_offset());
        self.transfer.view = match written {
            Ok(()) => TransferView::Exported(
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            Err(_) => TransferView::ExportFailed,
        };
        self.sync_settings();
    }

    /// "Impor": the open dialog.
    pub(super) fn config_import(&mut self) {
        settings::pick_import_path(self.hwnd);
    }

    /// The open dialog's file: checked, then held until the owner confirms.
    pub(super) fn config_import_from(&mut self, path: std::path::PathBuf) {
        let lang = self.config.language;
        let mut bytes = Vec::new();
        // One byte over the limit is enough to know it is too big.
        let read = std::fs::File::open(&path)
            .and_then(|f| f.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes));
        let checked = match read {
            Ok(_) => config_transfer::read_incoming(&bytes),
            Err(_) => Err(ImportError::NotVale),
        };
        match checked {
            Ok(incoming) => {
                self.transfer.view = TransferView::Pending {
                    app_version: incoming.app_version.clone(),
                    date: incoming.exported,
                };
                self.transfer.pending = Some(incoming);
            }
            Err(error) => {
                self.transfer.pending = None;
                self.transfer.view = view_for_error(lang, &error);
            }
        }
        self.sync_settings();
    }

    pub(super) fn config_import_cancel(&mut self) {
        self.transfer.pending = None;
        self.transfer.view = TransferView::Idle;
        self.sync_settings();
    }

    /// "Impor sekarang": backs up the config, saves the merged one and
    /// restarts Vale so everything takes effect.
    pub(super) fn config_import_confirm(&mut self) {
        let lang = self.config.language;
        match import_gate(
            self.transfer.pending.is_some(),
            self.rec.engaged(),
            self.screen.busy(),
        ) {
            Gate::Nothing => return,
            Gate::Busy => {
                self.transfer.view =
                    TransferView::Refused(i18n::text(lang, Key::StopRecordingFirst).into());
                self.sync_settings();
                return;
            }
            Gate::Go => {}
        }
        let Some(incoming) = self.transfer.pending.take() else {
            return;
        };
        let path = config::path();
        let _ = config_transfer::backup_config(&path, Local::now().naive_local());
        let merged = config_transfer::import_and_save(&path, &self.config, &incoming);
        self.adopt_config(merged);
        self.transfer.view = TransferView::Idle;
        let launched =
            std::env::current_exe().and_then(|exe| config_transfer::launch_imported(&exe));
        match launched {
            // `shutdown` destroys the window, which needs the app unborrowed.
            Ok(()) => DEFERRED_SHUTDOWN.set(true),
            Err(_) => {
                let notice = update_page::notice(
                    "settings-imported",
                    i18n::text(lang, Key::SettingsImportedRestart).into(),
                    "Vale".into(),
                    NoticeKind::UpdateInfo,
                );
                self.apply_now(Event::Notice(notice));
                self.sync_settings();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_gate_refuses_while_recording() {
        assert_eq!(import_gate(true, true, false), Gate::Busy);
        assert_eq!(import_gate(true, false, true), Gate::Busy);
        assert_eq!(import_gate(true, true, true), Gate::Busy);
        assert_eq!(import_gate(false, false, false), Gate::Nothing);
        assert_eq!(import_gate(false, true, false), Gate::Nothing);
        assert_eq!(import_gate(true, false, false), Gate::Go);
    }

    #[test]
    fn view_for_error_uses_the_exact_copy() {
        assert_eq!(
            view_for_error(Lang::Id, &ImportError::NotVale),
            TransferView::Refused("Ini bukan file pengaturan Vale.".into())
        );
        assert_eq!(
            view_for_error(Lang::En, &ImportError::NotVale),
            TransferView::Refused("This is not a Vale settings file.".into())
        );
        let newer = ImportError::Newer {
            app_version: "6.0.0".into(),
        };
        assert_eq!(
            view_for_error(Lang::Id, &newer),
            TransferView::Refused(
                "File ini dari Vale yang lebih baru (versi 6.0.0). Perbarui Vale dulu.".into()
            )
        );
        assert_eq!(
            view_for_error(Lang::En, &newer),
            TransferView::Refused(
                "This file comes from a newer Vale (6.0.0). Update Vale first.".into()
            )
        );
    }

    #[test]
    fn newer_message_has_no_em_dash() {
        for lang in [Lang::Id, Lang::En] {
            assert!(!i18n::settings_file_newer(lang, "6.0.0").contains('\u{2014}'));
            for key in [
                Key::NotValeFile,
                Key::StopRecordingFirst,
                Key::SettingsImportedRestart,
                Key::ConfigFilterJson,
            ] {
                assert!(!i18n::text(lang, key).contains('\u{2014}'));
            }
        }
    }
}
