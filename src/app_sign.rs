//! The island's side of the signing window (`crate::sign::window`): opening
//! it from a shelf card, and what a saved document does back here (a notice
//! that shows the file, and a card on the shelf). A child of `app`.
//!
//! The window never calls into `App`: it queues a report and posts
//! `WM_APP_SIGN`, answered by `on_sign_reports`. Nothing runs while no
//! window is open.

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::App;
use crate::i18n::{Key, tr};
use crate::model::{Event, Notice, NoticeKind};
use crate::shelf::{self, Probe};
use crate::sign::window::{self, Report};
use crate::sign::{self, Source};

/// The longest a file name in the notice may be cut to, in turn, until the
/// renderer says the line fits.
const NAME_CUTS: [usize; 4] = [32, 24, 16, 10];

/// "Ditandatangani · {name} · klik untuk buka": the whole name if it fits,
/// else the name cut shorter (the renderer ellipsizes what still does not).
pub(super) fn saved_notice(path: &Path, fits: &dyn Fn(&str) -> bool) -> Notice {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let body = |name: &str| tr(Key::SignNoticeBody).replace("{name}", name);
    let mut text = body(&name);
    if !fits(&text) {
        for max in NAME_CUTS {
            text = body(&shelf::short_name(&name, max));
            if fits(&text) {
                break;
            }
        }
    }
    Notice {
        key: "sign-saved".into(),
        source: tr(Key::SignMenu).into(),
        title: tr(Key::SignDone).into(),
        body: text,
        kind: NoticeKind::ShotSaved { path: Some(path.to_path_buf()) },
    }
}

impl App {
    /// "Tanda tangani" was picked on card `i`: the file the signing window
    /// opens on. The caller opens the window outside the app borrow (it takes
    /// the foreground, which sends messages back here). A file that is gone
    /// only dims its card.
    pub(super) fn sign_source(&mut self, i: usize) -> Option<Source> {
        let path = self.shelf.shelf.get(i).map(PathBuf::from)?;
        let kind = sign::kind_of(&path)?;
        if shelf::probe(&path) == Probe::Missing {
            self.shelf_look();
            return None;
        }
        Some(Source { path, kind })
    }

    /// `WM_APP_SIGN`: what the signing windows reported.
    pub(super) fn on_sign_reports(&mut self, now: Instant) {
        for report in window::take_reports() {
            self.on_sign_report(report, now);
        }
    }

    fn on_sign_report(&mut self, report: Report, now: Instant) {
        match report {
            Report::Saved(path) => {
                let notice = {
                    let kind = NoticeKind::ShotSaved { path: None };
                    let fits = |s: &str| self.renderer.notice_text_fits(&kind, false, s);
                    saved_notice(&path, &fits)
                };
                self.apply(Event::Notice(notice), now);
                self.shelf_add_made(path, now);
            }
        }
    }

    /// At startup: the temp copies an Office conversion left after a crash.
    pub(super) fn sign_start(&mut self) {
        sign::office::sweep(&sign::office::temp_root());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Effect, Hit};
    use crate::state::IslandState;

    const SAVED: &str = r"C:\x\A (ditandatangani).pdf";

    #[test]
    fn a_saved_report_makes_a_reveal_notice() {
        let notice = saved_notice(Path::new(SAVED), &|_| true);
        assert_eq!(notice.title, tr(Key::SignDone));
        assert!(notice.body.contains("A (ditandatangani).pdf"));
        let t0 = Instant::now();
        let mut s = IslandState::new(
            chrono::NaiveDate::from_ymd_opt(2026, 10, 25).unwrap_or_default(),
            Default::default(),
        );
        s.apply(Event::Notice(notice), t0);
        let fx = s.apply(Event::Click(Hit::Body), t0 + std::time::Duration::from_secs(1));
        assert_eq!(fx, vec![Effect::RevealFile(SAVED.into())]);
    }

    #[test]
    fn a_long_name_is_cut_until_the_line_fits() {
        let long = r"C:\x\a-very-long-name-for-a-signed-contract-document.pdf";
        let notice = saved_notice(Path::new(long), &|s| s.chars().count() <= 40);
        assert!(notice.body.chars().count() <= 40, "{}", notice.body);
        assert!(notice.body.contains('\u{2026}'));
    }
}
