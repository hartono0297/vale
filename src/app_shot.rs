//! The island's side of the screenshot preview (Run 8): a new clipboard
//! image becomes a candidate, a short-lived worker looks for its file in
//! the Screenshots folder, the big notice shows it, and its buttons copy it
//! again, keep it or move it to the Recycle Bin. A child of `app`, so it
//! works on `App` directly. See docs/memory/screenshot-preview.md.
//!
//! Power: no watcher and no polling. One worker lists the folder when an
//! image arrives, and `TIMER_SHOT_RETRY` (a one-shot) runs at most four more
//! within about 5.5 s; `TIMER_SHOT_SHOW` waits for a screen clip's overlay to
//! close at most three times; then nothing runs until the next image. A
//! Simpan or Hapus worker looks a few times a second at most until the
//! screenshot's match window has closed (about 5.5 s after it), then ends.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use super::{App, WM_APP_EVENT, poster};
use crate::clipboard::image::{self, Thumb};
use crate::clipboard::store::ClipImage;
use crate::clipboard::win as clipboard_win;
use crate::config::ShotIgnore;
use crate::i18n::{self, Key, tr};
use crate::model::{Event, Notice, NoticeKind, ShotAction, View};
use crate::screenshot::win::{self, DeleteJob};
use crate::screenshot::{self, ClipCandidate, FileCandidate, FileOutcome};
use crate::ui::render::ShotView;

/// One-shot: look in the Screenshots folder once more (Snipping Tool
/// writes the file a moment after the clipboard). 20–22 are the
/// clipboard's.
pub(super) const TIMER_SHOT_RETRY: usize = 23;
/// One-shot: the preview waits for a fullscreen app to be gone.
pub(super) const TIMER_SHOT_SHOW: usize = 25;
/// The waits before the later looks: they cover the whole match window
/// (`screenshot::AFTER_MS`), about 5.5 s in all.
const RETRY_MS: [u32; 4] = [600, 900, 1500, 2500];
/// The preview comes up while a fullscreen app is reported (a screen clip's
/// overlay counts as one until it closes): look again this often, this many
/// times, before it is taken for a game.
const SHOW_RETRY_MS: u32 = 1200;
const SHOW_TRIES: u8 = 3;
/// The preview's thumbnail, made from the full DIB: its 230 × 136 box at 2×.
const PREVIEW_MAX: (u32, u32) = (460, 272);

/// What the preview draws of a screenshot.
pub(super) struct ShotLook {
    pub id: u64,
    pub thumb: Thumb,
    pub w: u32,
    pub h: u32,
    /// The matched file's size, else the bitmap's.
    pub bytes: u64,
}

/// One screenshot: the clipboard candidate, its file once found.
pub(super) struct ShotState {
    pub look: ShotLook,
    /// Its history card; `None` with the history off.
    pub entry: Option<u64>,
    pub clip: ClipCandidate,
    /// The clipboard sequence number read with it (or after "Salin lagi").
    pub sequence: u32,
    /// The original, for "Salin lagi" (the history's own copy, shared).
    pub dib: Option<Arc<Vec<u8>>>,
    pub file: Option<FileCandidate>,
    /// Folder looks made so far.
    pub checks: u8,
    /// The clipboard held a picture and nothing else (`picture_only`): its
    /// preview does not wait for a file.
    pub flagged: bool,
    /// The capture it belongs to: the id of its first clipboard update (its
    /// own id unless it was merged, `plan_arrival`). The updates of one
    /// capture share one file.
    pub capture: u64,
}

/// How a button answered a capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Answer {
    /// Simpan.
    Kept,
    /// Hapus (or "Jika diabaikan: Hapus").
    Deleted,
}

/// The capture Simpan or Hapus last answered, as of its newest clipboard
/// update: a later update of that same capture is no new screenshot.
#[derive(Clone, Copy, Debug)]
pub(super) struct AnsweredCapture {
    pub clip: ClipCandidate,
    pub entry: Option<u64>,
    pub answer: Answer,
}

/// An update of the capture a button answered (`Shots::answered_again`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AbsorbedUpdate {
    pub answer: Answer,
    /// After Hapus, surely the deleted picture put back
    /// (`screenshot::takes_back`): it goes again. Else the update only
    /// stays quiet.
    pub take_back: bool,
}

/// What a new clipboard image did on the screenshot side
/// (`App::shot_from_image`), for the clipboard's own feedback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShotArrival {
    /// Nothing yet: no preview now (off, Vale's own copy, waiting for its
    /// file or for an overlay). The copy feedback goes as usual.
    Nothing,
    /// The preview went up now (the caller refreshes).
    Raised,
    /// An update of a capture Simpan or Hapus already answered: no preview
    /// and no copy feedback (it would replace "Sudah tersimpan"/"Dihapus").
    Absorbed,
}

impl ShotArrival {
    pub fn raised(self) -> bool {
        self == Self::Raised
    }

    /// Whether the clipboard's "Disalin"/copy notice may follow: not when the
    /// preview took its place, nor for a capture a button answered.
    pub fn copy_feedback_due(self) -> bool {
        self == Self::Nothing
    }
}

/// What Simpan works from (`Shots::keep_capture`).
pub(super) struct KeepPlan {
    /// The file Windows wrote for any update of the capture.
    pub file: Option<FileCandidate>,
    /// The newest update's candidate: its window reaches back to the first.
    pub clip: ClipCandidate,
    pub dib: Option<Arc<Vec<u8>>>,
    /// A waiting update of the same capture that was dropped.
    pub dropped: Option<u64>,
}

/// What Hapus works from (`Shots::delete_capture`).
pub(super) struct DeletePlan {
    /// The shown update's look, for the fade-out.
    pub look: ShotLook,
    /// The file Windows wrote for any update of the capture.
    pub file: Option<FileCandidate>,
    /// The newest update's candidate: its window reaches back to the first.
    pub clip: ClipCandidate,
    /// The clipboard sequence of the newest write of the capture: the
    /// clipboard is emptied only while it still holds that.
    pub sequence: u32,
    /// The history cards of its updates (one, unless they differed).
    pub entries: Vec<u64>,
    /// A waiting update of the same capture that was dropped.
    pub dropped: Option<u64>,
}

/// The screenshots the island knows of.
#[derive(Default)]
pub(super) struct Shots {
    /// The one whose preview was raised (shows or waits its turn).
    pub shown: Option<ShotState>,
    /// A new image without the screenshot flag: it shows only once its file
    /// is found; a browser's "Copy image" never is.
    pub waiting: Option<ShotState>,
    /// The shot whose folder looks still run (the newest only).
    pub checking: Option<u64>,
    pub retry_timer: bool,
    /// Times the preview waited for a fullscreen app to go.
    pub show_tries: u8,
    pub next_id: u64,
    /// The Screenshots folder as the last look found it.
    pub dir: Option<PathBuf>,
    /// The last preview's look, kept so its fade-out still draws it.
    pub faded: Option<ShotLook>,
    /// When the image before the newest arrived (unix ms): files before
    /// the newest's arrival may be that one's (`screenshot::claim_from`).
    pub last_at_ms: Option<i64>,
    /// The files matched to screenshots, newest last (`TAKEN_CAP`): never
    /// matched to another.
    pub taken: Vec<PathBuf>,
    /// When the latest images arrived, shared with the workers: a
    /// screenshot's window ends where the next one's begins.
    pub arrivals: screenshot::Arrivals,
    /// The capture Simpan or Hapus last answered (`keep_capture`,
    /// `delete_capture`).
    pub answered: Option<AnsweredCapture>,
}

/// How many matched files `Shots::taken` remembers.
const TAKEN_CAP: usize = 16;

/// `path` joins the files no other screenshot may match (once).
fn remember_taken(taken: &mut Vec<PathBuf>, path: PathBuf) {
    if !taken.contains(&path) {
        taken.push(path);
        if taken.len() > TAKEN_CAP {
            taken.remove(0);
        }
    }
}

/// Where Simpan writes a screenshot that has no file: in `dir`, named like
/// Windows names them, never a name on disk (`exists`) or one already
/// handed out (`taken`, a save still running).
fn save_path(
    dir: &std::path::Path,
    now: chrono::NaiveDateTime,
    taken: &[PathBuf],
    exists: impl Fn(&std::path::Path) -> bool,
) -> PathBuf {
    dir.join(screenshot::save_name(now, |n| {
        let p = dir.join(n);
        taken.contains(&p) || exists(&p)
    }))
}

impl Shots {
    fn get_mut(&mut self, id: u64) -> Option<&mut ShotState> {
        [self.shown.as_mut(), self.waiting.as_mut()]
            .into_iter()
            .flatten()
            .find(|s| s.look.id == id)
    }

    /// `file` was found for `shot`: it is the file of every clipboard update
    /// of that capture still without one. An update a merge superseded can
    /// still be shown (its successor waits for the overlay) and its look come
    /// back late; the file then goes to the successor too, never only into
    /// `taken` where the successor could not claim it. The ids that got it.
    fn give_file(&mut self, shot: u64, file: &FileCandidate) -> Vec<u64> {
        let Some(capture) = self.get_mut(shot).map(|s| s.capture) else {
            return Vec::new();
        };
        let mut got = Vec::new();
        for s in [self.shown.as_mut(), self.waiting.as_mut()]
            .into_iter()
            .flatten()
        {
            if s.capture == capture && (s.look.id == shot || s.file.is_none()) {
                s.look.bytes = file.bytes;
                s.file = Some(file.clone());
                got.push(s.look.id);
            }
        }
        remember_taken(&mut self.taken, file.path.clone());
        got
    }

    /// Simpan on the shown screenshot `id` answers for its whole capture: the
    /// file Windows wrote for any of its clipboard updates, else the newest
    /// update's candidate (its window reaches back to the first update, and
    /// that update's arrival does not cut it). A waiting update of the same
    /// capture is dropped (its preview would ask again), and the capture is
    /// remembered (`answered`): a later update of it is no new screenshot.
    fn keep_capture(&mut self, id: u64) -> Option<KeepPlan> {
        let shown = self.shown.as_ref().filter(|s| s.look.id == id)?;
        let sibling = self.waiting.take_if(|w| w.capture == shown.capture);
        let newest = sibling.as_ref().unwrap_or(shown);
        let plan = KeepPlan {
            file: shown
                .file
                .clone()
                .or_else(|| sibling.as_ref().and_then(|s| s.file.clone())),
            clip: newest.clip,
            dib: shown.dib.clone(),
            dropped: sibling.as_ref().map(|s| s.look.id),
        };
        self.answered = Some(AnsweredCapture {
            clip: newest.clip,
            entry: newest.entry.or(shown.entry),
            answer: Answer::Kept,
        });
        Some(plan)
    }

    /// Hapus on the shown screenshot `id`, like Simpan, answers its whole
    /// capture: the shown update goes, and so does a waiting update of the
    /// same capture (its overlay timer would raise a preview of what was
    /// just deleted). The delete works from the file of any update, the
    /// newest update's candidate (the strict match and `AFTER_MS` are
    /// unchanged) and the newest clipboard write. The capture is remembered
    /// (`answered`): a later update of it is no new screenshot and does not
    /// cut the delete worker's wait.
    fn delete_capture(&mut self, id: u64) -> Option<DeletePlan> {
        let shown = self.shown.take_if(|s| s.look.id == id)?;
        let sibling = self.waiting.take_if(|w| w.capture == shown.capture);
        let newest = sibling.as_ref().unwrap_or(&shown);
        let mut entries: Vec<u64> = [shown.entry, newest.entry].into_iter().flatten().collect();
        entries.dedup();
        let plan = DeletePlan {
            file: shown
                .file
                .clone()
                .or_else(|| sibling.as_ref().and_then(|s| s.file.clone())),
            clip: newest.clip,
            // "Salin lagi" on the shown one can be the newest write.
            sequence: shown.sequence.max(newest.sequence),
            entries,
            dropped: sibling.as_ref().map(|s| s.look.id),
            look: ShotLook {
                thumb: shown.look.thumb.clone(),
                ..shown.look
            },
        };
        self.answered = Some(AnsweredCapture {
            clip: newest.clip,
            entry: newest.entry.or(shown.entry),
            answer: Answer::Deleted,
        });
        Some(plan)
    }

    /// A new clipboard image that `plan_arrival` did not merge (`merged`
    /// false) and that is the answered capture's own update again
    /// (`screenshot::same_capture`): Simpan or Hapus already answered it,
    /// so it is no new screenshot, and its arrival must not cut the save or
    /// delete worker's wait. It extends the answered capture (a third
    /// update chains on). The answer it got and, after Hapus, whether it
    /// goes again (the stricter `screenshot::takes_back`), else `None`.
    fn answered_again(
        &mut self,
        merged: bool,
        clip: &ClipCandidate,
        entry: Option<u64>,
    ) -> Option<AbsorbedUpdate> {
        let a = self.answered.as_mut().filter(|_| !merged)?;
        if !screenshot::same_capture(&a.clip, a.entry, clip, entry) {
            return None;
        }
        let take_back = a.answer == Answer::Deleted && screenshot::takes_back(&a.clip, clip);
        a.clip = screenshot::merge_arrival(&a.clip, *clip);
        a.entry = a.entry.or(entry);
        Some(AbsorbedUpdate {
            answer: a.answer,
            take_back,
        })
    }
}

/// What the renderer draws for `view`'s preview.
pub(super) fn shot_view<'a>(shots: &'a Shots, view: &View) -> Option<ShotView<'a>> {
    let View::Notice(Notice {
        kind: NoticeKind::Screenshot { shot, .. },
        ..
    }) = view
    else {
        return None;
    };
    let look = shots
        .shown
        .as_ref()
        .map(|s| &s.look)
        .filter(|l| l.id == *shot)
        .or(shots.faded.as_ref().filter(|l| l.id == *shot))?;
    Some(ShotView {
        id: look.id,
        thumb: &look.thumb,
        w: look.w,
        h: look.h,
        bytes: look.bytes,
    })
}

/// The history card that goes with a screenshot's "Hapus": its own card,
/// never one the user pinned (a PrtSc of an unchanged screen can bring an
/// old pinned card to the front).
pub(super) fn card_to_remove(
    history: &crate::clipboard::History,
    entry: Option<u64>,
) -> Option<u64> {
    entry.filter(|&id| history.get(id).is_some_and(|c| !c.pinned))
}

/// The notice after "Simpan": "Disimpan · Pictures\Screenshots" when Simpan
/// wrote the file (`written`) in `dir`, "Sudah tersimpan · ..." when Windows
/// had already written it; without a file (no picture to write) "Belum ada
/// file · tetap di clipboard", so no one thinks it is on disk.
pub(super) fn kept_notice(has_file: bool, written: bool, dir: Option<PathBuf>) -> Notice {
    let (title, body) = match (has_file, dir) {
        (true, Some(dir)) => (
            if written {
                Key::ShotSaved
            } else {
                Key::ShotAlreadySaved
            },
            screenshot::folder_label(&dir),
        ),
        _ => (Key::ShotNotSaved, tr(Key::ShotOnClipboard).into()),
    };
    Notice {
        key: "shot-saved".into(),
        source: tr(Key::ShotTitle).into(),
        title: tr(title).into(),
        body,
        kind: NoticeKind::ShotSaved,
    }
}

/// "Gagal menyimpan": Simpan could not write the file.
fn save_failed_notice() -> Notice {
    Notice {
        key: "shot-saved".into(),
        source: tr(Key::ShotTitle).into(),
        title: tr(Key::ShotSaveFailed).into(),
        // Windows Security's Controlled folder access blocks a new program
        // from writing in Pictures until it is allowed.
        body: format!(
            "{} · {}",
            tr(Key::ShotOnClipboard),
            tr(Key::ShotMaybeBlocked)
        ),
        kind: NoticeKind::ShotSaved,
    }
}

/// A preview that ran its full time untouched: the screenshot to delete,
/// only while "Jika diabaikan" is Hapus now (set back to Simpan meanwhile,
/// it is kept) and only when a file was matched to it (`has_file`): a
/// picture copied from an image editor has none, and its clipboard and card
/// stay. Simpan, the default, never deletes.
pub(super) fn delete_when_ignored(on_ignore: ShotIgnore, id: u64, has_file: bool) -> Option<u64> {
    (on_ignore == ShotIgnore::Delete && has_file).then_some(id)
}

/// The newest screenshot in play, as far as a new arrival cares.
pub(super) struct PrevShot<'a> {
    pub clip: &'a ClipCandidate,
    pub entry: Option<u64>,
    pub file: Option<&'a FileCandidate>,
    pub flagged: bool,
}

/// What a new clipboard image becomes.
#[derive(Debug)]
pub(super) struct Arrival {
    pub clip: ClipCandidate,
    /// The file already matched when this is the same capture again.
    pub file: Option<FileCandidate>,
    /// Its preview does not wait for a file.
    pub flagged: bool,
    /// The same capture as `prev` (`screenshot::same_capture`).
    pub merged: bool,
}

/// A new image (`clip`, history card `entry`, the clipboard's own
/// screenshot flag) after `prev`: a new screenshot as it is, or the same
/// capture again, which carries over the first one's window (a file stamped
/// before this arrival is still its file), its matched file and its flag. A
/// tool that sets the clipboard twice would otherwise leave the shown shot
/// without the file Windows wrote, and Simpan would write a second copy.
pub(super) fn plan_arrival(
    prev: Option<PrevShot<'_>>,
    clip: ClipCandidate,
    entry: Option<u64>,
    flagged: bool,
) -> Arrival {
    match prev {
        Some(p) if screenshot::same_capture(p.clip, p.entry, &clip, entry) => Arrival {
            clip: screenshot::merge_arrival(p.clip, clip),
            file: p.file.cloned(),
            flagged: flagged || p.flagged || p.file.is_some(),
            merged: true,
        },
        _ => Arrival {
            clip,
            file: None,
            flagged,
            merged: false,
        },
    }
}

/// What the preview does about a fullscreen report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShowStep {
    Show,
    /// Try again soon: the overlay of a screen clip may still be closing.
    Later,
    /// It stayed: a game or a presentation. Nothing shows over it.
    GiveUp,
}

/// `tries` waits made so far, `fullscreen` the fresh report.
fn show_step(fullscreen: bool, tries: u8) -> ShowStep {
    if !fullscreen {
        ShowStep::Show
    } else if tries < SHOW_TRIES {
        ShowStep::Later
    } else {
        ShowStep::GiveUp
    }
}

/// A change in Settings → Clipboard → Screenshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShotSetting {
    Tool(crate::config::ShotTool),
    Preview(bool),
    OnIgnore(ShotIgnore),
    Secs(u32),
    /// "Salin otomatis".
    AutoCopy(bool),
}

impl App {
    /// A new clipboard image (`entry` its history card, `None` with the
    /// history off). The flag (only bitmap formats) shows the preview at
    /// once; otherwise it shows only if the folder holds its file. Nothing
    /// with the preview off or over a fullscreen app (then it is simply
    /// kept). `Raised` when the preview went up now (the caller refreshes),
    /// `Absorbed` for an update of a capture a button already answered.
    pub(super) fn shot_from_image(
        &mut self,
        entry: Option<u64>,
        img: &ClipImage,
        sequence: u32,
    ) -> ShotArrival {
        if !self.config.screenshot.preview {
            return ShotArrival::Nothing;
        }
        // The editor's own Copy (Vale's screenshot editor): that one image
        // is no new screenshot, whatever formats it carries.
        if crate::shot_edit::own_copy(sequence).is_some() {
            return ShotArrival::Nothing;
        }
        self.shots.next_id += 1;
        let id = self.shots.next_id;
        let at_ms = screenshot::unix_ms(SystemTime::now());
        let dib = img.full.clone();
        let thumb = dib
            .as_deref()
            .and_then(|d| {
                image::dib_info(d)
                    .map(|i| image::thumbnail_fit(d, &i, PREVIEW_MAX.0, PREVIEW_MAX.1))
            })
            .unwrap_or_else(|| img.thumb.clone());
        let clip = ClipCandidate {
            at_ms,
            from_ms: screenshot::claim_from(at_ms, self.shots.last_at_ms.replace(at_ms)),
            to_ms: at_ms + screenshot::AFTER_MS,
            w: img.width,
            h: img.height,
            hash: screenshot::thumb_hash(&img.thumb.bgra, img.thumb.w, img.thumb.h),
        };
        // The newest screenshot still in play (waiting is newer than shown):
        // is this its own capture again?
        let prev = self.shots.waiting.as_ref().or(self.shots.shown.as_ref());
        let prev_capture = prev.map(|s| s.capture);
        let arrival = plan_arrival(
            prev.map(|s| PrevShot {
                clip: &s.clip,
                entry: s.entry,
                file: s.file.as_ref(),
                flagged: s.flagged,
            }),
            clip,
            entry,
            img.screenshot,
        );
        let merged = arrival.merged;
        // The capture Simpan or Hapus already answered, set on the
        // clipboard again (its preview is closed, so nothing above merged
        // it): no preview, no copy notice over the answer's, and no arrival
        // that would end the worker's wait before Windows' file is stamped.
        // After Hapus the picture goes again, only when it surely is the
        // deleted one put back (`screenshot::takes_back`: strict hashes,
        // within `TAKE_BACK_MS`): off the clipboard (only while it still
        // holds this update) and its card out of the history. A looser
        // match (a retake of an unchanged screen, say) is only kept quiet.
        if let Some(absorbed) = self.shots.answered_again(merged, &clip, entry) {
            if absorbed.take_back {
                if let Some(sequence) = clipboard_win::empty_if_unchanged(self.hwnd, sequence) {
                    self.clip.own_sequence = Some(sequence);
                }
                self.remove_shot_cards(entry.into_iter());
            }
            return ShotArrival::Absorbed;
        }
        let capture = prev_capture.filter(|_| merged).unwrap_or(id);
        let file = arrival.file;
        let state = ShotState {
            look: ShotLook {
                id,
                thumb,
                w: img.width,
                h: img.height,
                bytes: file
                    .as_ref()
                    .map_or_else(|| dib.as_ref().map_or(0, |d| d.len() as u64), |f| f.bytes),
            },
            entry,
            clip: arrival.clip,
            sequence,
            dib,
            file,
            checks: 0,
            flagged: arrival.flagged,
            capture,
        };
        let show_now = state.flagged;
        // From now on the files are the new image's: every earlier one's
        // window ends here (`look_for_shot_file`, the workers).
        self.shots.arrivals.push(at_ms);
        // The shown screenshot's file not found yet: one last look for it
        // now, as the new image takes over the retries (a browser's "Copy
        // image" must not leave the preview without its file). Not for the
        // same capture again: the new candidate's window covers it.
        if !merged
            && let Some(prev) = self.shots.checking
            && self
                .shots
                .shown
                .as_ref()
                .is_some_and(|s| s.look.id == prev && s.file.is_none())
        {
            self.look_for_shot_file(prev);
        }
        self.stop_shot_retry();
        self.stop_shot_show();
        self.shots.waiting = Some(state);
        self.shots.checking = Some(id);
        self.look_for_shot_file(id);
        if show_now && self.try_show_waiting() {
            ShotArrival::Raised
        } else {
            ShotArrival::Nothing
        }
    }

    /// A screen clip's overlay is a fullscreen window: Windows reports a
    /// fullscreen app until it closes, and the island's own poll is up to a
    /// second behind. Asks again now before trusting the flag.
    fn fullscreen_now(&mut self) -> bool {
        if self.state.fullscreen() {
            self.check_fullscreen();
        }
        self.state.fullscreen()
    }

    /// Raises the waiting preview unless a fullscreen app is really there:
    /// then it tries again soon (`SHOW_TRIES` times) and gives up only for a
    /// game or a presentation. True when it went up now.
    fn try_show_waiting(&mut self) -> bool {
        let fullscreen = self.fullscreen_now();
        match show_step(fullscreen, self.shots.show_tries) {
            ShowStep::Show => {
                self.shots.show_tries = 0;
                self.show_waiting_shot();
                true
            }
            ShowStep::Later => {
                self.shots.show_tries += 1;
                unsafe {
                    SetTimer(Some(self.hwnd), TIMER_SHOT_SHOW, SHOW_RETRY_MS, None);
                }
                false
            }
            ShowStep::GiveUp => {
                self.shots.show_tries = 0;
                self.shots.waiting = None;
                false
            }
        }
    }

    fn stop_shot_show(&mut self) {
        self.shots.show_tries = 0;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SHOT_SHOW);
        }
    }

    /// `TIMER_SHOT_SHOW`: the preview that waited for the overlay tries again.
    pub(super) fn on_shot_show_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SHOT_SHOW);
        }
        if self.shots.waiting.is_some() && self.try_show_waiting() {
            self.refresh(Instant::now());
        }
    }

    /// The waiting shot's preview goes up; it replaces the one before.
    fn show_waiting_shot(&mut self) {
        let Some(state) = self.shots.waiting.take() else {
            return;
        };
        if let Some(old) = self.shots.shown.replace(state)
            && self.shots.checking == Some(old.look.id)
        {
            self.stop_shot_retry();
        }
        let id = self.shots.shown.as_ref().map_or(0, |s| s.look.id);
        let cfg = self.config.screenshot;
        let notice = Notice {
            key: format!("shot-{id}"),
            source: tr(Key::ShotTitle).into(),
            title: tr(Key::ShotTitle).into(),
            body: String::new(),
            kind: NoticeKind::Screenshot {
                shot: id,
                secs: cfg.show_secs,
                delete_if_ignored: cfg.on_ignore == ShotIgnore::Delete,
            },
        };
        self.apply(Event::Notice(notice), Instant::now());
    }

    /// One look in the Screenshots folder for `id`, on a short-lived thread.
    fn look_for_shot_file(&mut self, id: u64) {
        let arrivals = self.shots.arrivals.clone();
        let Some(state) = self.shots.get_mut(id) else {
            return;
        };
        state.checks += 1;
        state.clip.to_ms = arrivals.window_end(state.clip.at_ms);
        let clip = state.clip;
        let taken = self.shots.taken.clone();
        if !win::spawn_check(
            id,
            clip,
            taken,
            self.tx.clone(),
            poster(self.hwnd, WM_APP_EVENT),
        ) {
            self.shots.checking = None;
        }
    }

    fn stop_shot_retry(&mut self) {
        if std::mem::take(&mut self.shots.retry_timer) {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_SHOT_RETRY);
            }
        }
    }

    /// `TIMER_SHOT_RETRY`: the next look, for the shot still being looked for.
    pub(super) fn on_shot_timer(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_SHOT_RETRY);
        }
        self.shots.retry_timer = false;
        if let Some(id) = self.shots.checking {
            self.look_for_shot_file(id);
        }
    }

    /// A look came back. A match is this screenshot's file (a waiting one
    /// now shows); none tries again up to twice, then gives up: an image
    /// without the flag and without a file was no screenshot.
    pub(super) fn on_shot_folder(
        &mut self,
        shot: u64,
        dir: Option<PathBuf>,
        found: Option<FileCandidate>,
    ) {
        if dir.is_some() {
            self.shots.dir = dir;
        }
        let looking = self.shots.checking == Some(shot);
        let arrivals = self.shots.arrivals.clone();
        // A look that set out before a newer image arrived: a file from
        // after that arrival is the newer one's.
        let Some((end, checks)) = self
            .shots
            .get_mut(shot)
            .map(|s| (arrivals.window_end(s.clip.at_ms), s.checks))
        else {
            return;
        };
        match found.filter(|f| f.at_ms <= end) {
            Some(file) => {
                // This screenshot's file, and that of a newer update of the
                // same capture still without one.
                let got = self.shots.give_file(shot, &file);
                if self.shots.checking.is_some_and(|c| got.contains(&c)) {
                    self.shots.checking = None;
                    self.stop_shot_retry();
                }
                let waiting = self
                    .shots
                    .waiting
                    .as_ref()
                    .is_some_and(|s| got.contains(&s.look.id));
                if waiting {
                    if self.config.screenshot.preview {
                        self.try_show_waiting();
                    } else {
                        self.shots.waiting = None;
                    }
                }
                // The drain refreshes the island: the size line now says
                // the file's.
            }
            None => {
                if looking && let Some(&ms) = RETRY_MS.get(usize::from(checks) - 1) {
                    unsafe {
                        SetTimer(Some(self.hwnd), TIMER_SHOT_RETRY, ms, None);
                    }
                    self.shots.retry_timer = true;
                    return;
                }
                if looking {
                    self.shots.checking = None;
                }
                // A picture without its file still shows (it waits for the
                // overlay at most); one that is not flagged was no screenshot.
                if self
                    .shots
                    .waiting
                    .as_ref()
                    .is_some_and(|s| s.look.id == shot && !s.flagged)
                {
                    self.shots.waiting = None;
                }
            }
        }
    }

    /// After the reducer: a preview that is gone (expired, replaced, a
    /// button, a game) forgets its screenshot; its look stays for the fade.
    pub(super) fn follow_shot(&mut self) {
        let Some(id) = self.shots.shown.as_ref().map(|s| s.look.id) else {
            return;
        };
        if self.state.has_shot(id) {
            return;
        }
        if let Some(old) = self.shots.shown.take() {
            self.shots.faded = Some(old.look);
        }
        if self.shots.checking == Some(id) {
            self.shots.checking = None;
            self.stop_shot_retry();
        }
    }

    /// A button or the thumbnail, from the reducer (which already closed
    /// the preview for the buttons).
    pub(super) fn perform_shot(&mut self, action: ShotAction) -> Option<Event> {
        let lang = i18n::lang();
        match action {
            ShotAction::Copy(id) => {
                let state = self.shots.shown.as_mut().filter(|s| s.look.id == id)?;
                let dib = state
                    .dib
                    .as_deref()
                    .cloned()
                    .unwrap_or_else(|| image::dib_from_thumb(&state.look.thumb));
                let (w, h, entry) = (state.look.w, state.look.h, state.entry);
                let sequence = clipboard_win::write(self.hwnd, &clipboard_win::image_formats(dib))?;
                state.sequence = sequence;
                self.clip.own_sequence = Some(sequence);
                if let Some(entry) = entry
                    && self
                        .clip
                        .history
                        .bump(entry, chrono::Local::now().timestamp())
                {
                    self.schedule_clip_save();
                }
                Some(Event::Notice(Notice {
                    key: "clip-copied".into(),
                    source: tr(Key::ClipboardTitle).into(),
                    title: tr(Key::ClipCopied).into(),
                    body: i18n::clip_image_meta(lang, true, w, h),
                    kind: NoticeKind::Copied,
                }))
            }
            ShotAction::Keep(id) => {
                // Simpan answers the whole capture: a waiting update of it
                // never raises the preview again, and a later one is no new
                // screenshot (`answered_again`).
                let plan = self.shots.keep_capture(id)?;
                if let Some(dropped) = plan.dropped {
                    self.stop_shot_show();
                    if self.shots.checking == Some(dropped) {
                        self.shots.checking = None;
                        self.stop_shot_retry();
                    }
                }
                let dir = plan
                    .file
                    .as_ref()
                    .and_then(|f| f.path.parent().map(PathBuf::from))
                    .or_else(|| self.shots.dir.clone());
                // Windows' own file: nothing to write, "Sudah tersimpan".
                if plan.file.is_some() {
                    return Some(Event::Notice(kept_notice(true, false, dir)));
                }
                let Some(dib) = plan.dib else {
                    return Some(Event::Notice(kept_notice(false, false, dir)));
                };
                // No file yet: the worker waits for Snipping Tool's (it may
                // still be on its way), else writes one. Its name is chosen
                // here and taken now, so the next screenshot never claims
                // it. The notice follows in `on_shot_save_done`.
                let Some(dir) = dir.or_else(win::screenshots_dir) else {
                    return Some(Event::Notice(save_failed_notice()));
                };
                let clip = plan.clip;
                let now = chrono::Local::now().naive_local();
                let path = save_path(&dir, now, &self.shots.taken, std::path::Path::exists);
                remember_taken(&mut self.shots.taken, path.clone());
                let job = win::SaveJob {
                    shot: id,
                    dib,
                    path,
                    clip,
                    taken: self.shots.taken.clone(),
                    arrivals: self.shots.arrivals.clone(),
                };
                if win::spawn_save(job, self.tx.clone(), poster(self.hwnd, WM_APP_EVENT)) {
                    None
                } else {
                    Some(Event::Notice(save_failed_notice()))
                }
            }
            ShotAction::Open(id) => {
                let state = self.shots.shown.as_ref().filter(|s| s.look.id == id)?;
                // Without a file there is nothing to open.
                if let Some(file) = &state.file {
                    win::open_file(&file.path);
                }
                None
            }
            ShotAction::Delete(id) => self.delete_shot(id),
            ShotAction::DeleteIgnored(id) => {
                // Only a file the strict delete worker could accept: a
                // lax-only match would empty the clipboard and drop the card
                // while the file stays.
                let has_file = self.shots.shown.as_ref().is_some_and(|s| {
                    s.look.id == id
                        && s.file
                            .as_ref()
                            .is_some_and(|f| screenshot::may_confirm(s.clip.hash, f.hash))
                });
                let id = delete_when_ignored(self.config.screenshot.on_ignore, id, has_file)?;
                self.delete_shot(id)
            }
        }
    }

    /// "Hapus": the clipboard is emptied only while it still holds this
    /// image; its history card goes; the file goes to the Recycle Bin on a
    /// worker, only once its hash confirms the picture (none matched or
    /// confirmed yet: the worker waits for it until the match window has
    /// closed). The notice follows in `on_shot_recycled`.
    fn delete_shot(&mut self, id: u64) -> Option<Event> {
        // Hapus answers the whole capture: a waiting update of it never
        // raises a preview of the deleted picture, and a later one is no new
        // screenshot (`answered_again`).
        let plan = self.shots.delete_capture(id)?;
        self.shots.faded = Some(plan.look);
        if plan.dropped.is_some() {
            self.stop_shot_show();
        }
        if self
            .shots
            .checking
            .is_some_and(|c| c == id || Some(c) == plan.dropped)
        {
            self.shots.checking = None;
            self.stop_shot_retry();
        }
        let emptied = match clipboard_win::empty_if_unchanged(self.hwnd, plan.sequence) {
            Some(sequence) => {
                self.clip.own_sequence = Some(sequence);
                true
            }
            None => false,
        };
        self.remove_shot_cards(plan.entries.into_iter());
        let had_file = plan.file.is_some();
        let job = DeleteJob {
            shot: id,
            file: plan.file.map(|f| f.path),
            clip: plan.clip,
            taken: self.shots.taken.clone(),
            arrivals: self.shots.arrivals.clone(),
            emptied,
        };
        if win::spawn_delete(job, self.tx.clone(), poster(self.hwnd, WM_APP_EVENT)) {
            return None;
        }
        Some(Event::ShotRecycled {
            shot: id,
            file: if had_file {
                FileOutcome::Failed
            } else {
                FileOutcome::NoFile
            },
            emptied,
        })
    }

    /// A deleted screenshot's history cards go (never a pinned one).
    fn remove_shot_cards(&mut self, entries: impl Iterator<Item = u64>) {
        let mut removed = false;
        for entry in entries {
            if let Some(entry) = card_to_remove(&self.clip.history, Some(entry))
                && self.clip.history.delete(entry)
            {
                if self.clip.selected == Some(entry) {
                    self.clip.selected = None;
                }
                removed = true;
            }
        }
        if removed {
            self.clamp_clip_scroll();
            self.schedule_clip_save();
            self.redraw_clip();
        }
    }

    /// The save worker is done: "Disimpan" with the folder when it wrote the
    /// file (`written`), "Sudah tersimpan" when Snipping Tool's own was found,
    /// or why not. The file is this screenshot's: no later screenshot matches
    /// it.
    pub(super) fn on_shot_save_done(&mut self, path: Option<PathBuf>, written: bool, now: Instant) {
        let notice = match path {
            Some(path) => {
                let dir = path.parent().map(PathBuf::from);
                if dir.is_some() {
                    self.shots.dir = dir.clone();
                }
                remember_taken(&mut self.shots.taken, path);
                kept_notice(true, written, dir)
            }
            None => save_failed_notice(),
        };
        self.apply(Event::Notice(notice), now);
    }

    /// The delete worker is done: "Dihapus" (Bata at rest) says what went.
    pub(super) fn on_shot_recycled(&mut self, file: FileOutcome, emptied: bool, now: Instant) {
        let (title, body) = i18n::shot_deleted(i18n::lang(), file, emptied);
        let notice = Notice {
            key: "shot-deleted".into(),
            source: tr(Key::ShotTitle).into(),
            title: title.into(),
            body,
            kind: NoticeKind::ShotDeleted,
        };
        self.apply(Event::Notice(notice), now);
    }

    /// Settings → Clipboard → Screenshot. The listener runs while the
    /// history or the preview is on.
    pub(super) fn on_shot_setting(&mut self, change: ShotSetting) {
        let cfg = &mut self.config.screenshot;
        match change {
            ShotSetting::Tool(v) => cfg.tool = v,
            ShotSetting::Preview(on) => cfg.preview = on,
            ShotSetting::OnIgnore(v) => cfg.on_ignore = v,
            ShotSetting::Secs(n) => {
                let (min, max, _) = crate::config::SHOT_SECS_RANGE;
                cfg.show_secs = n.clamp(min, max);
            }
            ShotSetting::AutoCopy(on) => cfg.auto_copy = on,
        }
        self.save_config();
        if matches!(change, ShotSetting::Preview(_)) {
            self.sync_clip_listener();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        RETRY_MS, SHOW_RETRY_MS, SHOW_TRIES, ShowStep, card_to_remove, delete_when_ignored,
        kept_notice, show_step,
    };

    #[test]
    fn the_file_looks_cover_the_whole_match_window() {
        // Snipping Tool may write its file seconds after the clipboard.
        let total: u32 = RETRY_MS.iter().sum();
        assert!(
            i64::from(total) >= crate::screenshot::AFTER_MS,
            "{total} ms"
        );
        assert!(RETRY_MS.iter().all(|&ms| ms >= 500), "no busy looping");
    }

    #[test]
    fn a_fullscreen_report_is_waited_out_before_it_is_taken_for_a_game() {
        assert_eq!(show_step(false, 0), ShowStep::Show);
        assert_eq!(show_step(false, SHOW_TRIES), ShowStep::Show);
        for tries in 0..SHOW_TRIES {
            assert_eq!(show_step(true, tries), ShowStep::Later, "try {tries}");
        }
        assert_eq!(show_step(true, SHOW_TRIES), ShowStep::GiveUp);
        // The waits add up to a few seconds, no more.
        assert!(u32::from(SHOW_TRIES) * SHOW_RETRY_MS <= 5_000);
    }
    use crate::clipboard::History;
    use crate::config::{Config, ShotIgnore};

    #[test]
    fn an_ignored_preview_deletes_only_with_hapus_set_now() {
        // The default is Simpan: an ignored preview keeps its file.
        let default = Config::default().screenshot.on_ignore;
        assert_eq!(default, ShotIgnore::Keep);
        assert_eq!(delete_when_ignored(default, 3, true), None);
        assert_eq!(delete_when_ignored(ShotIgnore::Delete, 3, true), Some(3));
    }

    #[test]
    fn an_ignored_copy_without_a_file_is_never_deleted() {
        // A picture copied from an image editor: no file was matched, so
        // its clipboard and history card stay even with Hapus set.
        assert_eq!(delete_when_ignored(ShotIgnore::Delete, 3, false), None);
        assert_eq!(delete_when_ignored(ShotIgnore::Keep, 3, false), None);
    }

    #[test]
    fn simpan_names_its_file_before_the_worker_runs() {
        use super::{remember_taken, save_path};
        let dir = PathBuf::from(r"C:\Users\x\Pictures\Screenshots");
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 28)
            .and_then(|d| d.and_hms_opt(14, 30, 12))
            .expect("valid time");
        let first = dir.join("Screenshot 2026-10-28 143012.png");
        let mut taken = Vec::new();
        let p = save_path(&dir, at, &taken, |_| false);
        assert_eq!(p, first);
        remember_taken(&mut taken, p.clone());
        remember_taken(&mut taken, p);
        assert_eq!(taken.len(), 1, "once");
        // A second Simpan in the same second, the first not written yet:
        // another name, never the one handed out.
        let second = save_path(&dir, at, &taken, |_| false);
        assert_eq!(second, dir.join("Screenshot 2026-10-28 143012 (2).png"));
        // A file on disk counts as taken too.
        let on_disk = save_path(&dir, at, &[], |p| p == first.as_path());
        assert_eq!(on_disk, second);
        // The list keeps the newest few.
        for i in 0..40 {
            remember_taken(&mut taken, PathBuf::from(format!("{i}.png")));
        }
        assert_eq!(taken.len(), super::TAKEN_CAP);
        assert_eq!(taken.last(), Some(&PathBuf::from("39.png")));
    }
    use crate::clipboard::image::tests::dib24;
    use crate::clipboard::store::{ClipSnapshot, Ingested, ingest};
    use crate::screenshot::{ClipCandidate, FileCandidate, pick_match, thumb_hash};

    #[test]
    fn one_screenshot_by_clipboard_and_file_is_one_card() {
        let mut h = History::new(50);
        let snap = ClipSnapshot {
            dib: Some(dib24(1280, 720)),
            only_bitmaps: true,
            sequence: 7,
            ..ClipSnapshot::default()
        };
        let Ingested::Added { id, image: true } = ingest(&mut h, snap.clone(), 100) else {
            panic!("an image entry");
        };
        // Its file in the folder matches (the same picture's hash), and
        // adds nothing to the history.
        let thumb = &h.items()[0].image().unwrap().thumb;
        let hash = thumb_hash(&thumb.bgra, thumb.w, thumb.h);
        assert!(hash.is_some());
        let clip = ClipCandidate {
            at_ms: 100_000,
            from_ms: 100_000 - crate::screenshot::BEFORE_MS,
            to_ms: 100_000 + crate::screenshot::AFTER_MS,
            w: 1280,
            h: 720,
            hash,
        };
        let file = FileCandidate {
            path: "Screenshot 2026-10-28 101500.png".into(),
            at_ms: 100_700,
            bytes: 250_880,
            w: 1280,
            h: 720,
            hash,
        };
        assert!(pick_match(&clip, std::slice::from_ref(&file)).is_some());
        assert_eq!(h.len(), 1);
        // The same screenshot again (PrtSc twice): the card moves up, no copy.
        assert_eq!(ingest(&mut h, snap, 101), Ingested::Bumped { id });
        assert_eq!(h.len(), 1);
        assert!(h.items()[0].image().is_some_and(|i| i.screenshot));
    }

    #[test]
    fn hapus_keeps_a_pinned_card() {
        use crate::clipboard::store::tests::image;
        use crate::clipboard::store::{Ingested, ingest};
        let mut h = History::new(50);
        let Ingested::Added { id, .. } = ingest(&mut h, image(1280, 720), 0) else {
            panic!("added");
        };
        assert_eq!(card_to_remove(&h, Some(id)), Some(id));
        h.set_pinned(id, true);
        assert_eq!(card_to_remove(&h, Some(id)), None, "pinned on purpose");
        assert_eq!(card_to_remove(&h, None), None);
    }

    #[test]
    fn simpan_without_a_file_does_not_say_saved() {
        let dir = PathBuf::from(r"C:\Users\x\Pictures\Screenshots");
        let saved = kept_notice(true, true, Some(dir.clone()));
        assert_eq!(
            (saved.title.as_str(), saved.body.as_str()),
            ("Disimpan", r"Pictures\Screenshots")
        );
        // A PrtSc without auto-save left no file: it is only on the clipboard.
        for dir in [None, Some(dir)] {
            let n = kept_notice(false, false, dir);
            assert_eq!(
                (n.title.as_str(), n.body.as_str()),
                ("Belum ada file", "tetap di clipboard")
            );
        }
    }

    #[test]
    fn a_file_windows_wrote_says_already_saved_and_only_ours_says_saved() {
        let dir = PathBuf::from(r"C:\Users\x\Pictures\Screenshots");
        let already = kept_notice(true, false, Some(dir.clone()));
        assert_eq!(
            (already.title.as_str(), already.body.as_str()),
            ("Sudah tersimpan", r"Pictures\Screenshots")
        );
        let ours = kept_notice(true, true, Some(dir));
        assert_eq!(ours.title, "Disimpan");
        // Not a file written, nothing to claim: no file, no "saved" at all.
        assert_eq!(kept_notice(false, true, None).title, "Belum ada file");
    }

    fn clip_at(at: i64, from: i64, hash: Option<u64>) -> ClipCandidate {
        ClipCandidate {
            at_ms: at,
            from_ms: from,
            to_ms: at + crate::screenshot::AFTER_MS,
            w: 1280,
            h: 720,
            hash,
        }
    }

    fn file_at(name: &str, at: i64, hash: Option<u64>) -> FileCandidate {
        FileCandidate {
            path: name.into(),
            at_ms: at,
            bytes: 248_527,
            w: 1280,
            h: 720,
            hash,
        }
    }

    #[test]
    fn a_second_clipboard_update_of_one_capture_keeps_the_first_ones_file_and_window() {
        use super::{PrevShot, plan_arrival};
        use crate::screenshot::{SaveOutcome, claim_from, save_decision};
        const T: i64 = 1_800_000_000_000;
        const PIC: u64 = 0x00FF_00FF_F0F0_0F0F;
        let first = clip_at(T, claim_from(T, None), Some(PIC));
        let file = file_at("Screenshot 235523.png", T + 150, Some(PIC));
        // The same picture again 600 ms later (a Bumped entry, same card).
        let at = T + 600;
        let fresh = clip_at(at, claim_from(at, Some(T)), Some(PIC));
        fn prev<'a>(
            clip: &'a ClipCandidate,
            file: Option<&'a FileCandidate>,
            flagged: bool,
        ) -> PrevShot<'a> {
            PrevShot {
                clip,
                entry: Some(7),
                file,
                flagged,
            }
        }
        // The shown shot had matched the file: the second carries it over,
        // and Simpan on it is "already saved", never a write.
        let a = plan_arrival(Some(prev(&first, Some(&file), true)), fresh, Some(7), false);
        assert!(a.merged);
        assert_eq!(
            a.file.as_ref().map(|f| f.path.clone()),
            Some(file.path.clone())
        );
        assert!(a.flagged, "the first was a screenshot: this one shows too");
        // The shot had only been waiting (no file yet): the window now
        // reaches back to the first arrival, so the file is found.
        let b = plan_arrival(Some(prev(&first, None, false)), fresh, Some(7), false);
        assert!(b.merged && b.file.is_none() && !b.flagged);
        assert_eq!(
            save_decision(&b.clip, std::slice::from_ref(&file), &[], None),
            SaveOutcome::AlreadySaved(file.path.clone())
        );
        // A fresh candidate would have missed it (the bug).
        assert_eq!(
            save_decision(&fresh, std::slice::from_ref(&file), &[], None),
            SaveOutcome::Write
        );
    }

    #[test]
    fn another_picture_or_a_later_screenshot_is_a_new_candidate() {
        use super::{PrevShot, plan_arrival};
        use crate::screenshot::claim_from;
        const T: i64 = 1_800_000_000_000;
        const PIC: u64 = 0x00FF_00FF_F0F0_0F0F;
        let first = clip_at(T, claim_from(T, None), Some(PIC));
        let file = file_at("a.png", T + 150, Some(PIC));
        let prev = || PrevShot {
            clip: &first,
            entry: Some(7),
            file: Some(&file),
            flagged: true,
        };
        // Another picture 1 s later: its files start at its own arrival.
        let other = clip_at(T + 1_000, T + 1_000, Some(!PIC));
        let a = plan_arrival(Some(prev()), other, Some(8), false);
        assert!(!a.merged && a.file.is_none() && !a.flagged);
        assert_eq!(a.clip.from_ms, T + 1_000);
        // The same picture long after, or with nothing before: new too.
        let later = clip_at(T + 4_000, T + 4_000, Some(PIC));
        assert!(!plan_arrival(Some(prev()), later, Some(7), true).merged);
        assert!(!plan_arrival(None, other, None, true).merged);
    }

    fn state(id: u64, capture: u64, clip: ClipCandidate, file: Option<FileCandidate>) -> ShotState {
        ShotState {
            look: ShotLook {
                id,
                thumb: crate::clipboard::image::Thumb {
                    w: 1,
                    h: 1,
                    bgra: vec![0; 4],
                },
                w: clip.w,
                h: clip.h,
                bytes: 0,
            },
            entry: Some(7),
            clip,
            sequence: 1,
            dib: Some(std::sync::Arc::new(vec![1, 2, 3])),
            file,
            checks: 1,
            flagged: true,
            capture,
        }
    }

    use super::{ShotLook, ShotState, Shots};

    fn with(shown: Option<ShotState>, waiting: Option<ShotState>) -> Shots {
        Shots {
            shown,
            waiting,
            ..Shots::default()
        }
    }
    use crate::screenshot::{SAVE_AFTER_MS, SaveOutcome, claim_from, save_decision};

    #[test]
    fn simpan_before_the_second_update_still_finds_windows_file() {
        // PrtSc at T (preview up), Simpan at T + 300 (preview closes), the
        // same capture set on the clipboard again at T + 600, Snipping Tool
        // stamps its file at T + 800.
        const T: i64 = 1_800_000_000_000;
        const PIC: u64 = 0x00FF_00FF_F0F0_0F0F;
        let first = clip_at(T, claim_from(T, None), Some(PIC));
        let mut shots = with(Some(state(1, 1, first, None)), None);
        shots.arrivals.push(T);
        let plan = shots.keep_capture(1).expect("the shown shot");
        assert!(plan.file.is_none() && plan.dropped.is_none());
        shots.shown = None; // follow_shot: the preview closed
        let file = file_at("Screenshot 235523.png", T + 800, Some(PIC));
        let save = |arrivals: &crate::screenshot::Arrivals| {
            let clip = ClipCandidate {
                to_ms: arrivals.window_end_after(plan.clip.at_ms, SAVE_AFTER_MS),
                ..plan.clip
            };
            save_decision(&clip, std::slice::from_ref(&file), &[], None)
        };
        // The second update is the kept capture: no new screenshot, and its
        // arrival is never pushed, so the save worker still sees the file.
        let again = clip_at(T + 600, claim_from(T + 600, Some(T)), Some(PIC));
        assert_eq!(
            shots
                .answered_again(false, &again, Some(7))
                .map(|a| a.answer),
            Some(Answer::Kept)
        );
        assert_eq!(
            save(&shots.arrivals),
            SaveOutcome::AlreadySaved(file.path.clone())
        );
        // A third update chains on the second.
        let third = clip_at(T + 2_400, T + 2_400, Some(PIC));
        assert_eq!(
            shots
                .answered_again(false, &third, Some(7))
                .map(|a| a.answer),
            Some(Answer::Kept)
        );
        // The bug: pushed as a fresh arrival, it cut the wait at T + 599,
        // and Simpan wrote a second copy.
        let cut = crate::screenshot::Arrivals::default();
        cut.push(T);
        cut.push(T + 600);
        assert_eq!(save(&cut), SaveOutcome::Write);
        // Another picture, or the same one much later, is a new screenshot.
        let other = clip_at(T + 3_000, T + 3_000, Some(!PIC));
        assert_eq!(shots.answered_again(false, &other, Some(8)), None);
        let later = clip_at(T + 9_000, T + 9_000, Some(PIC));
        assert_eq!(shots.answered_again(false, &later, Some(7)), None);
    }

    #[test]
    fn a_late_look_of_a_merged_update_gives_the_file_to_its_successor() {
        // Update 1 is shown without a file; update 2 (merged, capture 1)
        // waits for the overlay; update 1's look comes back with the file.
        const T: i64 = 1_800_000_000_000;
        const PIC: u64 = 0x00FF_00FF_F0F0_0F0F;
        let first = clip_at(T, claim_from(T, None), Some(PIC));
        let second = crate::screenshot::merge_arrival(
            &first,
            clip_at(T + 600, claim_from(T + 600, Some(T)), Some(PIC)),
        );
        let file = file_at("Screenshot 235523.png", T + 150, Some(PIC));
        let mut shots = with(
            Some(state(1, 1, first, None)),
            Some(state(2, 1, second, None)),
        );
        let got = shots.give_file(1, &file);
        assert_eq!(got, vec![1, 2]);
        assert_eq!(shots.taken, vec![file.path.clone()]);
        let waiting = shots.waiting.as_ref().expect("still waiting");
        assert_eq!(waiting.file.as_ref(), Some(&file), "never lost to taken");
        assert_eq!(waiting.look.bytes, file.bytes);
        // Another capture's shot is never given it.
        let mut other = with(
            Some(state(1, 1, first, None)),
            Some(state(3, 3, clip_at(T + 900, T + 900, Some(!PIC)), None)),
        );
        assert_eq!(other.give_file(1, &file), vec![1]);
        assert!(other.waiting.as_ref().is_some_and(|s| s.file.is_none()));
        // An unknown id changes nothing.
        assert!(other.give_file(9, &file).is_empty());
    }

    #[test]
    fn simpan_answers_the_whole_capture() {
        const T: i64 = 1_800_000_000_000;
        const PIC: u64 = 0x00FF_00FF_F0F0_0F0F;
        let first = clip_at(T, claim_from(T, None), Some(PIC));
        let second = crate::screenshot::merge_arrival(
            &first,
            clip_at(T + 600, claim_from(T + 600, Some(T)), Some(PIC)),
        );
        let file = file_at("Screenshot 235523.png", T + 150, Some(PIC));
        // The waiting update found the file: Simpan on the shown one is
        // "already saved", and the waiting update never shows again.
        let mut shots = with(
            Some(state(1, 1, first, None)),
            Some(state(2, 1, second, Some(file.clone()))),
        );
        let plan = shots.keep_capture(1).expect("shown");
        assert_eq!(plan.file, Some(file.clone()));
        assert_eq!(plan.dropped, Some(2));
        assert!(shots.waiting.is_none());
        // No file yet: the save works from the newest update, whose window
        // reaches back to the first and is not cut by the second arrival.
        let mut shots = with(
            Some(state(1, 1, first, None)),
            Some(state(2, 1, second, None)),
        );
        let plan = shots.keep_capture(1).expect("shown");
        assert!(plan.file.is_none());
        assert_eq!(plan.clip, second);
        assert_eq!(plan.clip.from_ms, first.from_ms);
        assert_eq!(shots.answered.map(|k| k.clip), Some(second));
        // A waiting shot of another capture stays; a wrong id keeps nothing.
        let mut shots = with(
            Some(state(1, 1, first, None)),
            Some(state(3, 3, clip_at(T + 900, T + 900, Some(!PIC)), None)),
        );
        assert_eq!(shots.keep_capture(1).and_then(|p| p.dropped), None);
        assert!(shots.waiting.is_some());
        assert!(shots.keep_capture(5).is_none());
    }

    use super::{AbsorbedUpdate, Answer, ShotArrival};

    #[test]
    fn an_update_after_simpan_says_nothing_over_the_saved_notice() {
        // PrtSc (preview raised, so the copy notice's gate never saw this
        // card), Simpan ("Sudah tersimpan" up), then Snipping Tool's second
        // update of the same capture.
        const T: i64 = 1_800_000_000_000;
        const PIC: u64 = 0x00FF_00FF_F0F0_0F0F;
        let first = clip_at(T, claim_from(T, None), Some(PIC));
        let mut shots = with(Some(state(1, 1, first, None)), None);
        assert!(shots.keep_capture(1).is_some());
        shots.shown = None;
        let again = clip_at(T + 400, claim_from(T + 400, Some(T)), Some(PIC));
        assert_eq!(
            shots
                .answered_again(false, &again, Some(7))
                .map(|a| a.answer),
            Some(Answer::Kept)
        );
        // What `shot_from_image` reports for it: handled, so the clipboard
        // side neither raises nor sends its copy notice (the bug: it said
        // "no preview" and the copy notice replaced "Sudah tersimpan").
        assert!(!ShotArrival::Absorbed.copy_feedback_due());
        assert!(!ShotArrival::Absorbed.raised());
        assert!(!ShotArrival::Raised.copy_feedback_due());
        assert!(ShotArrival::Raised.raised());
        assert!(ShotArrival::Nothing.copy_feedback_due());
        // An update `plan_arrival` merged is the shown shot's, never this.
        assert_eq!(shots.answered_again(true, &again, Some(7)), None);
    }

    #[test]
    fn hapus_answers_the_whole_capture() {
        const T: i64 = 1_800_000_000_000;
        const PIC: u64 = 0x00FF_00FF_F0F0_0F0F;
        let first = clip_at(T, claim_from(T, None), Some(PIC));
        let second = crate::screenshot::merge_arrival(
            &first,
            clip_at(T + 600, claim_from(T + 600, Some(T)), Some(PIC)),
        );
        let file = file_at("Screenshot 235523.png", T + 150, Some(PIC));
        // Shot 1 shown; shot 2 (the same capture) waits for the overlay and
        // holds the newest clipboard write.
        let mut newer = state(2, 1, second, Some(file.clone()));
        newer.sequence = 5;
        let mut shots = with(Some(state(1, 1, first, None)), Some(newer));
        let plan = shots.delete_capture(1).expect("the shown shot");
        assert_eq!(plan.dropped, Some(2), "no preview of the deleted shot");
        assert!(shots.shown.is_none() && shots.waiting.is_none());
        assert_eq!(plan.file, Some(file.clone()));
        assert_eq!(plan.clip, second);
        assert_eq!(plan.sequence, 5, "the clipboard holds the newest write");
        assert_eq!(plan.entries, vec![7]);
        assert_eq!(plan.look.id, 1);
        assert_eq!(shots.answered.map(|a| a.answer), Some(Answer::Deleted));
        // Another capture's waiting shot stays; a wrong id deletes nothing.
        let mut shots = with(
            Some(state(1, 1, first, None)),
            Some(state(3, 3, clip_at(T + 900, T + 900, Some(!PIC)), None)),
        );
        assert!(shots.delete_capture(5).is_none());
        assert!(shots.shown.is_some());
        let plan = shots.delete_capture(1).expect("shown");
        assert_eq!(plan.dropped, None);
        assert!(shots.waiting.as_ref().is_some_and(|s| s.look.id == 3));
    }

    #[test]
    fn an_update_after_hapus_neither_shows_again_nor_cuts_the_delete_wait() {
        use crate::screenshot::{Arrivals, pick_confirmed};
        // PrtSc at T, Hapus at T + 300 (no file yet), the same capture set
        // on the clipboard again at T + 600, Snipping Tool's file at T + 800.
        const T: i64 = 1_800_000_000_000;
        const PIC: u64 = 0x00FF_00FF_F0F0_0F0F;
        let first = clip_at(T, claim_from(T, None), Some(PIC));
        let mut shots = with(Some(state(1, 1, first, None)), None);
        shots.arrivals.push(T);
        let plan = shots.delete_capture(1).expect("the shown shot");
        let file = file_at("Screenshot 235523.png", T + 800, Some(PIC));
        // The delete worker's strict look (`AFTER_MS` window, as of now).
        let recycles = |arrivals: &Arrivals| {
            let clip = ClipCandidate {
                to_ms: arrivals.window_end(plan.clip.at_ms),
                ..plan.clip
            };
            pick_confirmed(&clip, std::slice::from_ref(&file)).is_some()
        };
        let again = clip_at(T + 600, claim_from(T + 600, Some(T)), Some(PIC));
        assert_eq!(
            shots.answered_again(false, &again, Some(7)),
            Some(AbsorbedUpdate {
                answer: Answer::Deleted,
                take_back: true,
            }),
            "no new screenshot, no preview of what was deleted, and it goes again"
        );
        assert!(recycles(&shots.arrivals), "its arrival is never pushed");
        // The bug: a fresh arrival cut the wait at T + 599 and the file
        // stayed.
        let cut = Arrivals::default();
        cut.push(T);
        cut.push(T + 600);
        assert!(!recycles(&cut));
        // Another picture soon after is a new screenshot.
        let other = clip_at(T + 900, T + 900, Some(!PIC));
        assert_eq!(shots.answered_again(false, &other, Some(8)), None);
    }

    #[test]
    fn a_retake_after_hapus_is_kept_quiet_but_never_wiped() {
        // PrtSc at T, Hapus; then an update of the same size whose hash is
        // only within the lax distance (a small change on screen), or the
        // same picture later than `TAKE_BACK_MS`: a deliberate retake may
        // look like that. No preview (absorbed), but the clipboard and its
        // card stay (the bug: both were wiped on the lax rule).
        use crate::screenshot::{LAX_HASH_DIST, TAKE_BACK_MS};
        const T: i64 = 1_800_000_000_000;
        const PIC: u64 = 0x00FF_00FF_F0F0_0F0F;
        let deleted = || {
            let first = clip_at(T, claim_from(T, None), Some(PIC));
            let mut shots = with(Some(state(1, 1, first, None)), None);
            assert!(shots.delete_capture(1).is_some());
            shots
        };
        let off = PIC ^ 0xFFF;
        assert!((PIC ^ off).count_ones() <= LAX_HASH_DIST);
        let retake = clip_at(T + 500, claim_from(T + 500, Some(T)), Some(off));
        assert_eq!(
            deleted().answered_again(false, &retake, Some(8)),
            Some(AbsorbedUpdate {
                answer: Answer::Deleted,
                take_back: false,
            })
        );
        let slow = T + TAKE_BACK_MS + 500;
        let later = clip_at(slow, claim_from(slow, Some(T)), Some(PIC));
        assert_eq!(
            deleted()
                .answered_again(false, &later, Some(8))
                .map(|a| a.take_back),
            Some(false)
        );
        // No hash on the new update: no proof either.
        let blind = clip_at(T + 300, claim_from(T + 300, Some(T)), None);
        assert_eq!(
            deleted()
                .answered_again(false, &blind, Some(7))
                .map(|a| a.take_back),
            Some(false)
        );
        // After Simpan nothing is ever taken back.
        let first = clip_at(T, claim_from(T, None), Some(PIC));
        let mut kept = with(Some(state(1, 1, first, None)), None);
        assert!(kept.keep_capture(1).is_some());
        let again = clip_at(T + 300, claim_from(T + 300, Some(T)), Some(PIC));
        assert_eq!(
            kept.answered_again(false, &again, Some(7)),
            Some(AbsorbedUpdate {
                answer: Answer::Kept,
                take_back: false,
            })
        );
    }
}
