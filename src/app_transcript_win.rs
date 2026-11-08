//! The island's side of the transcript windows (`crate::transcript_win`):
//! what a window shows (the document, and while a job runs the lines so far
//! with the mic's echo dropped), and what its buttons do. A child of `app`,
//! so it works on `App` directly.
//!
//! The windows' procedure never calls into `App`: it queues requests and posts
//! `WM_APP_TRANSCRIPT`, answered by `on_transcript_requests`. The queue's
//! changes and the worker's words reach an open window through
//! `transcript_refresh`; nothing runs while no window is open.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Local;

use super::record::display_title;
use super::transcribe_page::tr_notice;
use super::{App, DEFERRED_TRANSCRIPT_MENU};
use crate::clipboard::win;
use crate::i18n::{Key, Lang, text};
use crate::model::{Event, Notice, NoticeKind, RowTranscript};
use crate::shortcuts;
use crate::transcribe::doc::{self, Doc};
use crate::transcribe::output::{self, Segment};
use crate::transcribe::queue::{eta, percent, same_audio};
use crate::transcribe::{catalog, echo, gpu};
use crate::transcript_win::{self, Request, Status, View};
use crate::ui::menu_pop::Row;
use crate::ui::transcript_menu;

/// The lines a running job sent so far, as the window shows them: in time
/// order, the mic's echo of the meeting dropped.
pub(super) fn live_lines(segments: &[Segment]) -> Vec<Segment> {
    echo::drop_echo(output::merge(segments.iter().cloned()))
}

/// Where a recording's job stands, as the island knows it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Job {
    /// Its worker runs (and was not asked to stop).
    Running {
        progress: Option<(u64, Option<u64>)>,
        elapsed: Duration,
    },
    /// In the queue; a recording holds it, or its model (or the engine) is
    /// not installed.
    Queued { recording: bool, model_missing: bool },
    Failed(String),
    /// Nothing queued, running or failed.
    Idle,
}

/// What the window says about `job`. A recording without a transcript while
/// transcripts are off is `Off`.
pub(super) fn status_of(job: Job, enabled: bool, has_transcript: bool) -> Status {
    match job {
        Job::Running { progress, elapsed } => {
            let (done, total) = progress.unwrap_or((0, None));
            Status::Running {
                pct: percent(done, total).unwrap_or(0),
                eta: eta(elapsed, done, total),
            }
        }
        Job::Queued { recording: true, .. } => Status::Held,
        Job::Queued { model_missing: true, .. } => Status::Waiting,
        Job::Queued { .. } => Status::Queued,
        Job::Failed(why) => Status::Failed(why),
        Job::Idle if !enabled && !has_transcript => Status::Off,
        Job::Idle => Status::None,
    }
}

/// Adds `audio` to the windows to refresh after this batch, once.
pub(super) fn mark_dirty(dirty: &mut Vec<PathBuf>, audio: &Path) {
    if !dirty.iter().any(|p| same_audio(p, audio)) {
        dirty.push(audio.to_path_buf());
    }
}

/// The text a request puts on the clipboard or in a note, from `doc`.
pub(super) fn request_text(req: &Request, doc: &Doc, lang: Lang) -> Option<String> {
    match req {
        Request::Copy => Some(doc::plain_text(doc, lang)),
        Request::CopyPrompt => Some(doc::prompt_text(doc, lang)),
        Request::MakeNote => Some(doc::note_text(doc, lang)),
        _ => None,
    }
}

/// The redo menu's rows: the "Transkrip ulang" rows of a done recording's
/// menu (the window already shows the transcript, so "Lihat" is left out).
fn redo_rows(rows: Vec<Row>) -> Vec<Row> {
    rows.into_iter()
        .filter(|r| matches!(r, Row::Item { detail: Some(_), .. }))
        .collect()
}

impl App {
    /// Opens the transcript window of `audio`, or brings it forward.
    pub(super) fn open_transcript(&mut self, audio: PathBuf) {
        let view = self.transcript_view(&audio);
        transcript_win::open(self.hwnd, view, self.config.transcript.window);
    }

    /// What the window of `audio` shows now.
    pub(super) fn transcript_view(&self, audio: &Path) -> View {
        let lang = self.config.language;
        let fallback = display_title(audio, lang, Local::now().date_naive());
        let tr = &self.tr;
        let store = &tr.store;
        let enabled = self.config.transcript.enabled;
        let worker = tr
            .worker
            .as_ref()
            .filter(|w| same_audio(&w.audio, audio) && !w.stopping);
        // A running job's lines are the worker's: its `.srt` (an older run's,
        // if any) is not read.
        let doc = match worker {
            Some(w) => {
                let mut d = doc::load_meta(audio, &fallback);
                d.segments = live_lines(&w.segments);
                d.two_channel |= d.segments.iter().any(|s| s.speaker.is_some());
                d
            }
            None => doc::load(audio, &fallback),
        };
        let job = if let Some(w) = worker {
            Job::Running {
                progress: w.progress,
                elapsed: w.started.elapsed(),
            }
        } else if tr.queue.state(audio).is_some() {
            let model = tr
                .queue
                .jobs()
                .find(|j| same_audio(&j.audio, audio))
                .and_then(|j| catalog::model(&j.model));
            Job::Queued {
                recording: self.rec.engaged(),
                model_missing: !store.engine_installed()
                    || model.is_none_or(|m| !store.model_installed(m)),
            }
        } else if let Some(why) = tr
            .failed
            .iter()
            .find(|(p, _)| same_audio(p, audio))
            .map(|(_, why)| why.clone())
        {
            Job::Failed(why)
        } else {
            Job::Idle
        };
        let status = status_of(job, enabled, doc.has_transcript);
        View {
            doc,
            status,
            can_redo: enabled && store.engine_installed() && !store.installed_models().is_empty(),
            lang,
            font: self.config.font,
        }
    }

    /// The open window of `audio` follows the job and the queue.
    pub(super) fn transcript_refresh(&mut self, audio: &Path) {
        if transcript_win::is_open(audio) {
            transcript_win::update(audio, self.transcript_view(audio));
        }
    }

    /// The live job of `audio` sent a word: its open window follows once the
    /// event batch is read (`transcript_flush_live`).
    pub(super) fn transcript_mark_live(&mut self, audio: &Path) {
        if transcript_win::is_open(audio) {
            mark_dirty(&mut self.tr.live_dirty, audio);
        }
    }

    /// After an event batch: each window whose live job sent words follows
    /// once.
    pub(super) fn transcript_flush_live(&mut self) {
        for audio in std::mem::take(&mut self.tr.live_dirty) {
            self.transcript_refresh(&audio);
        }
    }

    /// Every open window follows (the queue, a hold, the language or font).
    pub(super) fn transcript_refresh_all(&mut self) {
        for audio in transcript_win::open_audios() {
            self.transcript_refresh(&audio);
        }
    }

    /// `WM_APP_TRANSCRIPT`: what the windows asked for.
    pub(super) fn on_transcript_requests(&mut self) {
        for (audio, req) in transcript_win::take_requests() {
            match req {
                Request::Copy | Request::CopyPrompt | Request::MakeNote => {
                    self.transcript_text_request(&audio, &req);
                }
                Request::OpenMd => {
                    // `<stem>.md`, or Vale's `.transcript.md` beside the user's own.
                    let md = doc::load_meta(&audio, "").md;
                    self.queue_shortcut(shortcuts::Action::OpenPath(md));
                }
                Request::OpenFolder => self.rec_reveal(audio),
                Request::Redo { anchor } => self.transcript_redo_menu(audio, anchor),
                Request::Cancel => self.transcribe_cancel(&audio),
                Request::Retry => self.transcribe_queue(audio, None),
                Request::Settings => self.open_transcript_settings(),
                // The window is gone; the island keeps nothing of it.
                Request::Closed => {}
                Request::Placed(place) => {
                    if self.config.transcript.window != Some(place) {
                        self.config.transcript.window = Some(place);
                        self.save_config();
                    }
                }
            }
        }
    }

    /// Salin, Salin + prompt ringkasan and Jadikan catatan, with their notice.
    fn transcript_text_request(&mut self, audio: &Path, req: &Request) {
        let view = self.transcript_view(audio);
        let lang = view.lang;
        let Some(body) = request_text(req, &view.doc, lang) else {
            return;
        };
        let title = view.doc.title.clone();
        let notice = if *req == Request::MakeNote {
            self.save_note(None, &body);
            tr_notice(
                lang,
                "tr-note",
                text(lang, Key::TrMadeNote).into(),
                title,
                NoticeKind::TranscriptInfo,
            )
        } else {
            let Some(sequence) = win::write(self.hwnd, &[(13, win::utf16z(&body))]) else {
                return;
            };
            // Written here: not a copy for the history to take again.
            self.clip.own_sequence = Some(sequence);
            Notice {
                key: "tr-copied".into(),
                source: text(lang, Key::TrNoticeSource).into(),
                title: text(lang, Key::ClipCopied).into(),
                body: title,
                kind: NoticeKind::Copied,
            }
        };
        self.apply_now(Event::Notice(notice));
    }

    /// Transkrip ulang: the models' rows under the button, opened once the
    /// handler holding the app returns (the dropdown takes the foreground).
    fn transcript_redo_menu(&mut self, audio: PathBuf, anchor: crate::capture::Rect) {
        let installed: Vec<&'static catalog::Model> = catalog::MODELS
            .iter()
            .filter(|m| self.tr.store.model_installed(m))
            .collect();
        let active = self.config.transcript.model_for(gpu::has_hardware_gpu());
        let rows = redo_rows(transcript_menu::rows(
            self.config.language,
            RowTranscript::Done,
            &installed,
            active,
        ));
        if rows.is_empty() {
            return;
        }
        DEFERRED_TRANSCRIPT_MENU.with_borrow_mut(|m| *m = Some((audio, rows, anchor)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::output::Speaker::{Me, Others};
    use std::path::PathBuf;

    fn seg(start: u64, end: u64, who: crate::transcribe::output::Speaker, text: &str) -> Segment {
        Segment {
            start_ms: start,
            end_ms: end,
            speaker: Some(who),
            text: text.into(),
        }
    }

    #[test]
    fn the_live_view_drops_echo() {
        // As the runner sends them: channel by channel, out of time order,
        // the mic repeating what the meeting said.
        let sent = vec![
            seg(5_000, 9_000, Others, "Can we push the release to next week? QA masih nemu dua bug."),
            seg(0, 4_000, Others, "Oke, kita mulai ya. Agenda hari ini tiga hal."),
            seg(200, 4_100, Me, "Oke kita mulai ya agenda hari ini tiga hal"),
            seg(5_100, 9_100, Me, "can we push the release to next week QA masih nemu dua bug"),
            seg(9_500, 11_000, Me, "Boleh, jadi deadline-nya Jumat ya."),
        ];
        let lines = live_lines(&sent);
        let texts: Vec<(&str, u64)> = lines.iter().map(|s| (s.text.as_str(), s.start_ms)).collect();
        assert_eq!(
            texts,
            [
                ("Oke, kita mulai ya. Agenda hari ini tiga hal.", 0),
                ("Can we push the release to next week? QA masih nemu dua bug.", 5_000),
                ("Boleh, jadi deadline-nya Jumat ya.", 9_500),
            ]
        );
        assert!(live_lines(&[]).is_empty());
    }

    #[test]
    fn a_burst_of_live_words_refreshes_each_window_once() {
        let mut dirty = Vec::new();
        // A batch of segments and progress words for two recordings.
        for audio in [r"C:\r\a.m4a", r"C:\r\a.m4a", r"c:\R\A.M4A", r"C:\r\b.m4a", r"C:\r\a.m4a"] {
            mark_dirty(&mut dirty, Path::new(audio));
        }
        assert_eq!(dirty, [PathBuf::from(r"C:\r\a.m4a"), PathBuf::from(r"C:\r\b.m4a")]);
    }

    #[test]
    fn the_status_follows_the_job() {
        use std::time::Duration;
        let running = |progress| Job::Running { progress, elapsed: Duration::from_secs(60) };
        let queued = |recording, model_missing| Job::Queued { recording, model_missing };
        let failed = Job::Failed("Disk penuh".into());
        let table = [
            // (job, transcripts on, has a transcript) -> status
            (running(Some((30_000, Some(120_000)))), true, false,
                Status::Running { pct: 25, eta: Some(Duration::from_secs(180)) }),
            (running(None), true, true, Status::Running { pct: 0, eta: None }),
            (queued(true, true), true, false, Status::Held),
            (queued(true, false), true, true, Status::Held),
            (queued(false, true), true, false, Status::Waiting),
            (queued(false, false), true, false, Status::Queued),
            (failed.clone(), true, false, Status::Failed("Disk penuh".into())),
            (failed, false, true, Status::Failed("Disk penuh".into())),
            (Job::Idle, false, false, Status::Off),
            (Job::Idle, false, true, Status::None),
            (Job::Idle, true, false, Status::None),
            (Job::Idle, true, true, Status::None),
        ];
        for (job, enabled, has, want) in table {
            assert_eq!(status_of(job.clone(), enabled, has), want, "{job:?} {enabled} {has}");
        }
    }

    #[test]
    fn copy_and_note_texts_come_from_the_doc() {
        let doc = Doc {
            audio: PathBuf::from(r"C:\r\a.m4a"),
            md: PathBuf::from(r"C:\r\a.md"),
            title: "Sync mingguan".into(),
            app: None,
            started: None,
            duration_ms: Some(60_000),
            model: None,
            language: None,
            two_channel: true,
            has_transcript: true,
            segments: (0..25)
                .map(|i| seg(i * 2_000, i * 2_000 + 1_500, if i.is_multiple_of(2) { Others } else { Me }, &format!("baris {i}")))
                .collect(),
        };
        for lang in [Lang::Id, Lang::En] {
            assert_eq!(request_text(&Request::Copy, &doc, lang), Some(doc::plain_text(&doc, lang)));
            assert_eq!(
                request_text(&Request::CopyPrompt, &doc, lang),
                Some(doc::prompt_text(&doc, lang))
            );
            assert_eq!(request_text(&Request::MakeNote, &doc, lang), Some(doc::note_text(&doc, lang)));
        }
        // The prompt comes first, then what Salin copies.
        let prompt = request_text(&Request::CopyPrompt, &doc, Lang::Id).unwrap();
        assert!(prompt.starts_with("Ringkas transkrip meeting"));
        assert!(prompt.ends_with(&doc::plain_text(&doc, Lang::Id)));
        // Every other request has no text.
        for req in [Request::OpenMd, Request::OpenFolder, Request::Cancel, Request::Retry, Request::Settings, Request::Closed] {
            assert_eq!(request_text(&req, &doc, Lang::Id), None, "{req:?}");
        }
    }

    #[test]
    fn the_redo_menu_has_only_the_model_rows() {
        let models = [&catalog::MODELS[0], &catalog::MODELS[1]];
        let rows = redo_rows(transcript_menu::rows(Lang::Id, RowTranscript::Done, &models, models[1]));
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| matches!(r, Row::Item { label, .. } if label == "Transkrip ulang")));
    }
}
