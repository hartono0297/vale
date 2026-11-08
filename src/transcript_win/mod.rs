//! The transcript window: one recording's transcript to read, copy, save as a
//! note or play. One normal top-level window per recording (class
//! `ValeTranscript`, in the taskbar), drawn with Direct2D in the window palette
//! of the theme in effect (light, warm dark or the Terminal neutral) and the UI
//! font.
//!
//! The island hands it a `View` (the document, the job's status, whether a
//! redo is possible, the language and font) with `open` and `update`. The
//! window procedure never calls into the island: what the user asks for is
//! queued as a `Request` and `WM_APP_TRANSCRIPT` is posted to the owner once
//! per batch, which reads them with `take_requests` (the sticky note list's
//! pattern).
//!
//! The rules (buttons per status, frame, line geometry, follow band, seek and
//! placement maths) are pure and live in `layout.rs`; `window.rs` draws and
//! routes.
//!
//! Power: nothing runs while no window is open. An open window reacts to
//! messages only: no timer and no poll.

use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

use crate::config::{CardPlace, UiFont};
use crate::i18n::Lang;
use crate::transcribe::doc::Doc;

pub mod layout;
mod window;
#[cfg(debug_assertions)]
pub mod demo;

/// Posted to the owner when the window queued requests (`take_requests`).
pub const WM_APP_TRANSCRIPT: u32 = WM_APP + 85;

/// Where the recording's transcript job stands, as the window says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// No job: the transcript (or its absence) is all there is.
    None,
    /// In the queue behind another job.
    Queued,
    /// In the queue, its model not there yet.
    Waiting,
    /// In the queue, held while a recording runs.
    Held,
    Running {
        pct: u8,
        eta: Option<Duration>,
    },
    /// The job failed, for this reason (in plain words).
    Failed(String),
    /// Transcripts are off and this recording has none.
    Off,
    /// The audio cannot be played (the player of Task 12 sets it).
    #[allow(dead_code)]
    PlayerError,
}

/// What a window shows. While a job runs, `doc.segments` are the lines so far.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub doc: Doc,
    pub status: Status,
    /// "Transkrip ulang" is offered (transcripts on, a model installed).
    pub can_redo: bool,
    pub lang: Lang,
    pub font: UiFont,
}

/// What the user asked a window for, for the island.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Copy,
    CopyPrompt,
    OpenMd,
    OpenFolder,
    MakeNote,
    /// The redo menu, under the button (screen px).
    Redo { anchor: crate::capture::Rect },
    Cancel,
    Retry,
    Settings,
    /// The window closed.
    Closed,
    /// The window was moved or sized (screen px).
    Placed(CardPlace),
}

/// Opens the window of `view.doc.audio`, or updates the one already open for
/// that recording and brings it forward. `place` is where the last window was;
/// a second window opens 24 px down and right of the newest one. Requests go to
/// `owner`. False when no window could be made.
pub fn open(owner: HWND, view: View, place: Option<CardPlace>) -> bool {
    window::open(owner, view, place)
}

/// A new view for the open window of `audio`; nothing when none is open.
pub fn update(audio: &Path, view: View) {
    window::update(audio, view);
}

pub fn is_open(audio: &Path) -> bool {
    window::is_open(audio)
}

/// The recordings whose windows are open.
pub fn open_audios() -> Vec<PathBuf> {
    window::open_audios()
}

/// What the windows asked for since the last call, oldest first.
pub fn take_requests() -> Vec<(PathBuf, Request)> {
    window::take_requests()
}

/// The theme changed: every window takes the palette in effect.
pub fn restyle() {
    window::restyle();
}

/// Closes every window.
pub fn close_all() {
    window::close_all();
}

/// The island is ending: the windows close and their Direct2D objects go now.
pub fn release() {
    window::release();
}
