//! The island's side of the self-update (`update.rs`): the automatic check on
//! a timer, the manual one, the notices, the install, and what the tray and
//! Settings show. A child of `app`, so it works on `App` directly.
//!
//! Power: one UI timer (`TIMER_UPDATE`), first 30 s after the start, then
//! every hour; a tick checks only when the setting is on and 24 h of wall
//! clock have passed since the last automatic check of this run, so a PC that
//! slept does not stretch the interval. Never in a debug build.

use std::time::{Duration, SystemTime};

use windows::Win32::UI::WindowsAndMessaging::SetTimer;

use super::{App, DEFERRED_SHUTDOWN, Start, WM_APP_EVENT, poster, shortcuts};
use crate::i18n::{self, Key, Lang};
use crate::model::{Event, Notice, NoticeKind};
use crate::ui::settings_layout::UpdateRow;
use crate::update::{self, Release, UpdateError, UpdateEvent};

/// Next free `TIMER_` id of the island (the highest in use was 34).
pub(super) const TIMER_UPDATE: usize = 35;
/// The first automatic check, after the start.
const FIRST_CHECK_MS: u32 = 30_000;
/// Then the timer ticks every hour.
const TICK_MS: u32 = 3_600_000;
/// A tick checks only when this much wall-clock time has passed.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Default)]
pub(super) struct UpdateState {
    /// The newer release a check found, until it is installed.
    known: Option<Release>,
    /// A check is running (at most one).
    checking: bool,
    /// A manual check was asked for while another ran: its answer reports.
    manual_waiting: bool,
    /// The download's whole percent while one runs (at most one).
    downloading: Option<u8>,
    /// When the last automatic check of this run started.
    last_auto: Option<SystemTime>,
}

impl UpdateState {
    /// The version (X.Y.Z) of the known, not yet installed update.
    pub(super) fn known_version(&self) -> Option<String> {
        self.known.as_ref().map(Release::version)
    }

    /// What Tentang's Pembaruan row shows.
    pub(super) fn row(&self) -> UpdateRow {
        if let Some(pct) = self.downloading {
            UpdateRow::Downloading(pct)
        } else if self.checking {
            UpdateRow::Checking
        } else if let Some(version) = self.known_version() {
            UpdateRow::Available(version)
        } else {
            UpdateRow::Idle
        }
    }
}

/// An automatic check is due: the setting is on and a day has passed since
/// the last one (or there was none). A clock set back counts as due rather
/// than waiting for the old time to come round.
fn auto_check_due(enabled: bool, last: Option<SystemTime>, now: SystemTime) -> bool {
    enabled
        && last.is_none_or(|last| {
            now.duration_since(last)
                .map_or(true, |passed| passed >= CHECK_EVERY)
        })
}

pub(super) fn notice(key: &str, title: String, body: String, kind: NoticeKind) -> Notice {
    Notice {
        key: key.into(),
        source: "Vale".into(),
        title,
        body,
        kind,
    }
}

/// "Vale X.Y.Z tersedia" with the "Nanti" and "Update" chips. Sticky: it stays
/// until a chip is clicked, an install starts, a newer offer replaces it or a
/// check finds no newer release.
fn available_notice(lang: Lang, version: &str) -> Notice {
    notice(
        "update-available",
        i18n::update_available(lang, version),
        i18n::update_installed(lang, update::CURRENT),
        NoticeKind::UpdateAvailable {
            version: version.into(),
        },
    )
}

/// "Mengunduh pembaruan… NN%". One key, so a newer one is the same notice.
fn progress_notice(lang: Lang, version: &str, pct: u8) -> Notice {
    notice(
        "update-progress",
        i18n::update_downloading(lang, pct),
        format!("Vale {version}"),
        NoticeKind::UpdateProgress { pct },
    )
}

/// "Update gagal: <alasan>" with the chip to the releases page.
fn failed_notice(lang: Lang, error: UpdateError, version: Option<&str>) -> Notice {
    notice(
        "update-failed",
        i18n::update_failed(lang, error.reason(lang)),
        match version {
            Some(version) => format!("Vale {version}"),
            None => i18n::update_installed(lang, update::CURRENT),
        },
        NoticeKind::UpdateFailed,
    )
}

/// News with nothing to click: up to date, updated, off in debug builds.
fn info_notice(title: String, body: String) -> Notice {
    notice("update-info", title, body, NoticeKind::UpdateInfo)
}

fn up_to_date_notice(lang: Lang) -> Notice {
    info_notice(
        i18n::update_up_to_date(lang, update::CURRENT),
        "GitHub".into(),
    )
}

fn debug_off_notice(lang: Lang) -> Notice {
    info_notice(
        i18n::text(lang, Key::UpdateDebugOff).into(),
        i18n::update_installed(lang, update::CURRENT),
    )
}

/// "Diperbarui ke X.Y.Z" after a restart by the updater; `old` is the version
/// it replaced.
fn updated_notice(lang: Lang, old: &str) -> Notice {
    info_notice(
        i18n::update_done(lang, update::CURRENT),
        format!("{old} \u{2192} {}", update::CURRENT),
    )
}

impl App {
    /// After the island is up: the timer of the automatic check (release
    /// builds only), the notice of an update that just restarted Vale, and
    /// the removal of what the last swap left beside the exe.
    pub(super) fn update_start(&mut self, start: &Start) {
        match start {
            Start::Updated(old) => {
                eprintln!("vale: updated from {old} to {}", update::CURRENT);
                self.apply_now(Event::Notice(updated_notice(self.config.language, old)));
            }
            Start::Imported => {
                let title = i18n::text(self.config.language, Key::SettingsImported).into();
                self.apply_now(Event::Notice(info_notice(title, String::new())));
            }
            Start::Normal => {}
        }
        if cfg!(debug_assertions) {
            return;
        }
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_UPDATE, FIRST_CHECK_MS, None);
        }
        // Best effort, off the UI thread: the old process may still be ending.
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        {
            std::thread::spawn(move || update::cleanup_leftovers(&dir));
        }
    }

    /// `TIMER_UPDATE`: the first tick comes 30 s after the start, the rest
    /// every hour; the 24 h rule decides which one checks.
    pub(super) fn on_update_timer(&mut self) {
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_UPDATE, TICK_MS, None);
        }
        if cfg!(debug_assertions) {
            return;
        }
        let now = SystemTime::now();
        if auto_check_due(self.config.update_check, self.update.last_auto, now)
            && self.update_check(false)
        {
            self.update.last_auto = Some(now);
        }
    }

    /// The tray item and "Cek sekarang": checks and always says what it
    /// found. A debug build only says it is off.
    pub(super) fn update_manual_check(&mut self) {
        if cfg!(debug_assertions) {
            self.apply_now(Event::Notice(debug_off_notice(self.config.language)));
            return;
        }
        self.update_check(true);
    }

    /// Starts a check on a worker; false when one runs or a download does.
    /// A manual ask while a check runs makes that check report.
    fn update_check(&mut self, manual: bool) -> bool {
        if self.update.downloading.is_some() {
            return false;
        }
        if self.update.checking {
            self.update.manual_waiting |= manual;
            return false;
        }
        self.update.checking = true;
        update::check(manual, self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        self.sync_settings();
        true
    }

    /// The notice's chip, the tray's "Update ke X.Y.Z" and Settings' "Update":
    /// downloads and verifies the known release on a worker. One at a time.
    pub(super) fn update_install(&mut self) {
        if self.update.downloading.is_some() {
            return;
        }
        let Some(release) = self.update.known.clone() else {
            return;
        };
        self.update.downloading = Some(0);
        update::install(release.clone(), self.tx.clone(), poster(self.hwnd, WM_APP_EVENT));
        let notice = progress_notice(self.config.language, &release.version(), 0);
        self.apply_now(Event::Notice(notice));
        self.sync_settings();
    }

    /// The failure notice's chip: the release page, as a last resort.
    pub(super) fn update_open_releases(&mut self) {
        self.queue_shortcut(shortcuts::Action::Open(update::RELEASES_LATEST_URL));
    }

    /// What a worker reported.
    pub(super) fn on_update_event(&mut self, event: UpdateEvent) {
        let lang = self.config.language;
        match event {
            UpdateEvent::Checked { manual, result } => {
                self.update.checking = false;
                let manual = manual | std::mem::take(&mut self.update.manual_waiting);
                self.on_checked(manual, result);
            }
            UpdateEvent::Progress(pct) => {
                // A late percent after a failure or the swap is not shown.
                if let Some(shown) = &mut self.update.downloading {
                    *shown = pct;
                    let version = self.update.known_version().unwrap_or_default();
                    self.apply_now(Event::Notice(progress_notice(lang, &version, pct)));
                }
            }
            UpdateEvent::Downloaded { release, file } => self.on_downloaded(&release, &file),
            UpdateEvent::Failed { release, error } => {
                self.update.downloading = None;
                let notice = failed_notice(lang, error, Some(&release.version()));
                self.apply_now(Event::Notice(notice));
            }
        }
        self.sync_settings();
    }

    fn on_checked(&mut self, manual: bool, result: Result<Release, UpdateError>) {
        let lang = self.config.language;
        match result {
            Ok(release) if update::is_newer(&release.tag, update::CURRENT) => {
                // Every check that finds a newer release offers it (the
                // sticky notice keeps one slot, so a repeat replaces it).
                // "Nanti" only clears the notice; the next check asks again.
                let version = release.version();
                self.update.known = Some(release);
                // A check that lands while an install runs (the daily one was
                // in flight when the tray's item was clicked) offers nothing:
                // "Update" on it would do nothing.
                if self.update.downloading.is_none() {
                    self.apply_now(Event::Notice(available_notice(lang, &version)));
                }
            }
            Ok(_) => {
                // No newer release (the offered one was pulled): the offer
                // goes with it, or its "Update" would have nothing to install.
                self.update.known = None;
                self.apply_now(Event::UpdateWithdrawn);
                if manual {
                    self.apply_now(Event::Notice(up_to_date_notice(lang)));
                }
            }
            Err(error) if manual => {
                self.apply_now(Event::Notice(failed_notice(lang, error, None)));
            }
            // An automatic check fails silently.
            Err(error) => eprintln!("vale: update check failed: {error:?}"),
        }
    }

    /// The download is verified: the config is saved, the new exe swapped in
    /// and started, and the island closes (the tray icon goes with its
    /// window). A failure rolls back and the old island keeps running.
    fn on_downloaded(&mut self, release: &Release, file: &std::path::Path) {
        let lang = self.config.language;
        self.save_config();
        let swapped = std::env::current_exe()
            .map_err(|_| UpdateError::NotWritable)
            .and_then(|exe| {
                update::swap_and_launch(&exe, file, |path| {
                    update::launch_updated(path, update::CURRENT)
                })
            });
        match swapped {
            // `shutdown` destroys the window, which needs the app unborrowed.
            Ok(()) => DEFERRED_SHUTDOWN.set(true),
            Err(error) => {
                self.update.downloading = None;
                let notice = failed_notice(lang, error, Some(&release.version()));
                self.apply_now(Event::Notice(notice));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    const DAY: u64 = 24 * 60 * 60;

    #[test]
    fn the_first_tick_checks_and_a_day_must_pass_before_the_next() {
        let start = 1_000_000;
        assert!(auto_check_due(true, None, at(start)));
        // The hourly ticks after a check do nothing for 24 h.
        for hours in 1..24 {
            assert!(
                !auto_check_due(true, Some(at(start)), at(start + hours * 3600)),
                "{hours} h"
            );
        }
        assert!(auto_check_due(true, Some(at(start)), at(start + DAY)));
        // A PC asleep for two days checks on the first tick after waking.
        assert!(auto_check_due(true, Some(at(start)), at(start + 2 * DAY + 5)));
    }

    #[test]
    fn the_setting_off_never_checks_and_a_clock_set_back_does_not_stall() {
        assert!(!auto_check_due(false, None, at(5)));
        assert!(!auto_check_due(false, Some(at(5)), at(5 + 3 * DAY)));
        assert!(auto_check_due(true, Some(at(DAY * 10)), at(DAY)));
    }

    #[test]
    fn the_row_follows_the_state_download_first() {
        let release = Release {
            tag: "v3.11.0".into(),
            html_url: String::new(),
            exe_url: String::new(),
            sha_url: String::new(),
        };
        let mut state = UpdateState::default();
        assert_eq!(state.row(), UpdateRow::Idle);
        state.checking = true;
        assert_eq!(state.row(), UpdateRow::Checking);
        state.checking = false;
        state.known = Some(release);
        assert_eq!(state.row(), UpdateRow::Available("3.11.0".into()));
        assert_eq!(state.known_version().as_deref(), Some("3.11.0"));
        state.downloading = Some(42);
        assert_eq!(state.row(), UpdateRow::Downloading(42));
    }

    #[test]
    fn the_notices_say_what_the_brief_says() {
        let n = available_notice(Lang::Id, "3.11.0");
        assert_eq!(n.title, "Vale 3.11.0 tersedia");
        assert_eq!(
            n.kind,
            NoticeKind::UpdateAvailable {
                version: "3.11.0".into()
            }
        );
        assert_eq!(
            available_notice(Lang::En, "3.11.0").title,
            "Vale 3.11.0 is available"
        );
        assert_eq!(
            progress_notice(Lang::Id, "3.11.0", 42).title,
            "Mengunduh pembaruan\u{2026} 42%"
        );
        assert_eq!(
            progress_notice(Lang::En, "3.11.0", 7).title,
            "Downloading update\u{2026} 7%"
        );
        // The same key at every percent: one notice, replaced.
        assert_eq!(
            progress_notice(Lang::Id, "3.11.0", 1).key,
            progress_notice(Lang::Id, "3.11.0", 2).key
        );
        let failed = failed_notice(Lang::Id, UpdateError::ChecksumMismatch, None);
        assert_eq!(failed.title, "Update gagal: checksum tidak cocok");
        assert_eq!(failed.kind, NoticeKind::UpdateFailed);
        assert_eq!(
            failed_notice(Lang::En, UpdateError::NoNetwork, Some("3.11.0")).title,
            "Update failed: no connection"
        );
        let current = update::CURRENT;
        assert_eq!(
            up_to_date_notice(Lang::En).title,
            format!("Vale is up to date ({current})")
        );
        assert_eq!(
            up_to_date_notice(Lang::Id).title,
            format!("Vale sudah versi terbaru ({current})")
        );
        assert_eq!(
            updated_notice(Lang::Id, "3.10.0").title,
            format!("Diperbarui ke {current}")
        );
        assert_eq!(
            debug_off_notice(Lang::En).title,
            "Update check is off in debug builds"
        );
        assert_eq!(
            debug_off_notice(Lang::Id).title,
            "Cek pembaruan mati di build debug"
        );
    }
}
