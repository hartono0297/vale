//! The floating live-caption card: the last lines of speech, as text. A
//! separate always-on-top window in the lyrics card's family that never takes
//! the focus (a click on it leaves the meeting or the video active).
//!
//! - `layout`: pure rules (the header parts, zones, the bottom stack, the
//!   place, the colours).
//! - `window`: the Win32 window, Direct2D drawing, input.
//!
//! The island talks to it through the functions here; the card tells the
//! island what the user did with `WM_APP_CAPTION_CARD` (`Msg`). The card's
//! window procedure never calls into `App`, and caption text never leaves the
//! card (no log, no print).

#[cfg(debug_assertions)]
pub mod demo;
// Until the island opens the card (the demo does, in debug builds).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub mod layout;
// Until the island opens the card (the demo does, in debug builds).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
mod window;

use std::sync::Arc;
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

use crate::captions::control::CardStatus;
use crate::captions::lines::Line;
use crate::captions::prefs::TextSize;
use crate::config::{CardPlace, UiFont};
pub use crate::lyrics_card::{Change, Mon};
use crate::transcribe::output::Speaker;
use crate::transcribe::prefs::TrLang;

/// Posted to the island by the card: `wparam` is `Msg::code`, `lparam` the
/// value (`Size`: 1 bigger, 0 smaller).
pub const WM_APP_CAPTION_CARD: u32 = WM_APP + 87;

/// What the user did in the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    /// The X.
    Close = 1,
    /// "Salin": copy the session's lines.
    Copy = 2,
    Pause = 3,
    Resume = 4,
    /// The language chip: the next language.
    Lang = 5,
    /// The status button in the body (download, turn on Transkrip, keep
    /// going on battery); the island knows which from the status it sent.
    Action = 6,
    /// A drag or resize ended; `take_placed` has the rect to save.
    Placed = 7,
    /// Ctrl + wheel: the text one step bigger (`lparam` 1) or smaller (0).
    Size = 8,
}

impl Msg {
    pub fn code(self) -> usize {
        self as usize
    }

    // Until the island reads the card's messages.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_code(code: usize) -> Option<Msg> {
        Some(match code {
            1 => Msg::Close,
            2 => Msg::Copy,
            3 => Msg::Pause,
            4 => Msg::Resume,
            5 => Msg::Lang,
            6 => Msg::Action,
            7 => Msg::Placed,
            8 => Msg::Size,
            _ => return None,
        })
    }
}

/// One snapshot of what the card shows. The final lines are `Set` only when
/// they changed; the grey lines come with every snapshot.
// Until the island opens the card (the demo does, in debug builds).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[derive(Clone)]
pub struct CardContent {
    pub status: CardStatus,
    pub lines: Change<Arc<Vec<Line>>>,
    /// The line each channel is still hearing, in channel order.
    pub interim: Vec<(Option<Speaker>, String)>,
    /// Two sources: lines carry "Saya" / "Peserta".
    pub labelled: bool,
    /// When the running session began; `None` while nothing runs (the header
    /// clock and its timer stop).
    pub running_since: Option<Instant>,
    pub lang: TrLang,
    pub size: TextSize,
    /// The engine fell behind: the hint stays until the card opens again.
    pub slow_hint: bool,
    /// The dark window palette's `[card, bg, accent]`.
    pub palette: [u32; 3],
    pub font: UiFont,
    /// Windows animations are on (off: values jump).
    pub animations: bool,
}

/// Counts and states only: caption text must never reach a log or a panic
/// message through `{:?}`.
impl std::fmt::Debug for CardContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = match &self.status {
            CardStatus::Listening => "Listening",
            CardStatus::Running => "Running",
            CardStatus::Paused => "Paused",
            CardStatus::Battery => "Battery",
            CardStatus::NeedsTranskrip => "NeedsTranskrip",
            CardStatus::NeedsModel => "NeedsModel",
            CardStatus::Downloading(_) => "Downloading",
            CardStatus::Failed(_) => "Failed",
        };
        let lines = match &self.lines {
            Change::Keep => None,
            Change::Set(l) => Some(l.len()),
        };
        f.debug_struct("CardContent")
            .field("status", &status)
            .field("lines", &lines)
            .field("interim", &self.interim.len())
            .field("labelled", &self.labelled)
            .field("running", &self.running_since.is_some())
            .field("lang", &self.lang)
            .field("size", &self.size)
            .field("slow_hint", &self.slow_hint)
            .finish()
    }
}

/// Where the card opens.
// Until the island opens the card (the demo does, in debug builds).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub struct OpenArgs {
    /// The island's window, which gets `WM_APP_CAPTION_CARD`.
    pub island: HWND,
    pub saved: Option<CardPlace>,
    pub monitors: Vec<Mon>,
    pub notch: Option<Mon>,
    /// The demo: what the user did goes to the demo, the place to a temp file.
    pub demo: bool,
}

/// Opens the card with `content`. False when it could not (already open, or
/// no window or target). Shown without activating.
// Until the island opens the card (the demo does, in debug builds).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn open(args: OpenArgs, content: CardContent) -> bool {
    window::open(args, content)
}

/// A new snapshot. Does nothing when the card is not open.
// Until the island opens the card (the demo does, in debug builds).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn update(content: CardContent) {
    window::update(content);
}

/// Closes the card and frees its window and Direct2D objects.
// Until the island closes the card.
#[allow(dead_code)]
pub fn close() {
    window::close();
}

/// The island is ending: the same as `close`, called before the process
/// ends so no Direct2D object survives to process detach.
pub fn release() {
    window::close();
}

/// The card's window, while it is open.
// Until the island asks for it.
#[allow(dead_code)]
pub fn hwnd() -> Option<HWND> {
    window::hwnd()
}

/// The rect to save after `Msg::Placed`.
// Until the island opens the card (the demo does, in debug builds).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn take_placed() -> Option<CardPlace> {
    window::take_placed()
}

/// The demo's command sink.
pub(crate) fn demo_emit(msg: Msg, value: isize) {
    #[cfg(debug_assertions)]
    demo::emit(msg, value);
    #[cfg(not(debug_assertions))]
    let _ = (msg, value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_caption_text() {
        use crate::captions::lines::Line;
        let c = CardContent {
            status: CardStatus::Failed("SECRET-REASON".into()),
            lines: Change::Set(Arc::new(vec![Line {
                speaker: Some(Speaker::Me),
                text: "SECRET-FINAL".into(),
                start_ms: 0,
                gap: false,
            }])),
            interim: vec![(None, "SECRET-INTERIM".into())],
            labelled: true,
            running_since: None,
            lang: TrLang::Id,
            size: TextSize::Medium,
            slow_hint: false,
            palette: [0, 0, 0],
            font: UiFont::Sans,
            animations: true,
        };
        let shown = format!("{c:?}");
        assert!(!shown.contains("SECRET"), "{shown}");
        assert!(
            shown.contains("lines: Some(1)") && shown.contains("interim: 1"),
            "{shown}"
        );
    }

    #[test]
    fn msg_codes_round_trip() {
        let all = [
            Msg::Close,
            Msg::Copy,
            Msg::Pause,
            Msg::Resume,
            Msg::Lang,
            Msg::Action,
            Msg::Placed,
            Msg::Size,
        ];
        for (i, m) in all.into_iter().enumerate() {
            assert_eq!(m.code(), i + 1);
            assert_eq!(Msg::from_code(m.code()), Some(m));
        }
        assert_eq!(Msg::from_code(0), None);
        assert_eq!(Msg::from_code(9), None);
        assert_eq!(WM_APP_CAPTION_CARD, WM_APP + 87);
        assert_ne!(WM_APP_CAPTION_CARD, crate::lyrics_card::WM_APP_LYRICS_CARD);
    }
}
