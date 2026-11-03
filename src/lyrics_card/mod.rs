//! The floating "open lyrics" card: a separate always-on-top window that shows
//! the synced lyrics of the session the island shows, like the lyrics card of
//! a music app. See `docs/memory/lyrics-card.md`.
//!
//! - `layout`: pure rules (line stacking, scroll target, hit-testing, seek
//!   time, manual-scroll hold, placement, toggle rules).
//! - `colour`: pure colour maths (dominant colour, the gradient, contrast).
//! - `window`: the Win32 window, Direct2D drawing, input.
//!
//! The island talks to it through the functions here; the card tells the
//! island what the user did with `WM_APP_LYRICS_CARD` (`Msg`). The card's
//! window procedure never calls into `App`.

pub mod colour;
#[cfg(debug_assertions)]
pub mod demo;
pub mod layout;
mod window;

use std::sync::Arc;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

use crate::config::{CardPlace, UiFont};
use crate::lyrics::LrcLine;
use crate::model::Timeline;
use crate::notch_monitor::Display;
use crate::ui::settings_window::Rc;

pub use layout::{Mon, Req, card_next};

/// Posted to the island by the card: `wparam` is `Msg::code`, `lparam` the
/// value (the position in ms for `Seek`).
pub const WM_APP_LYRICS_CARD: u32 = WM_APP + 70;

/// What the user did in the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    /// The X or Esc.
    Close,
    /// A click on a line: seek to `lparam` ms.
    Seek,
    Prev,
    PlayPause,
    Next,
    /// A drag or resize ended; `take_placed` has the rect to save.
    Placed,
}

impl Msg {
    pub fn code(self) -> usize {
        match self {
            Msg::Close => 1,
            Msg::Seek => 2,
            Msg::Prev => 3,
            Msg::PlayPause => 4,
            Msg::Next => 5,
            Msg::Placed => 6,
        }
    }

    pub fn from_code(code: usize) -> Option<Msg> {
        Some(match code {
            1 => Msg::Close,
            2 => Msg::Seek,
            3 => Msg::Prev,
            4 => Msg::PlayPause,
            5 => Msg::Next,
            6 => Msg::Placed,
            _ => return None,
        })
    }
}

/// What the card says about the lyrics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Synced lyrics with words: the lines scroll.
    Synced,
    /// Looking (or waiting to retry): "Mencari lirik".
    Pending,
    /// None, instrumental, plain only, or gave up.
    Missing,
    /// No session: "Nggak ada lagu yang diputer".
    NoMedia,
}

/// A part of the snapshot that is only sent when it changes.
#[derive(Clone, Debug)]
pub enum Change<T> {
    Keep,
    Set(T),
}

/// One snapshot of what the card shows. The art bytes and the lines are
/// `Set` only when they changed.
#[derive(Clone, Debug)]
pub struct CardContent {
    pub status: Status,
    pub title: String,
    pub artist: String,
    pub art: Change<Option<Vec<u8>>>,
    pub lines: Change<Arc<Vec<LrcLine>>>,
    pub timeline: Option<Timeline>,
    pub playing: bool,
    pub can_seek: bool,
    /// The shown session's key: a seek is only sent to the same one.
    pub session_key: u64,
    /// The dark window palette's `[card, bg, accent]`, the gradient without art.
    pub palette: [u32; 3],
    pub font: UiFont,
    /// Windows animations are on (off: values jump).
    pub animations: bool,
}

/// Where and how the card opens.
pub struct OpenArgs {
    /// The island's window, which gets `WM_APP_LYRICS_CARD`.
    pub island: HWND,
    pub saved: Option<CardPlace>,
    pub monitors: Vec<Mon>,
    pub notch: Option<Mon>,
    /// Open hidden (fullscreen is up).
    pub hidden: bool,
    /// The demo: commands are printed, the place goes to a temp file.
    pub demo: bool,
    /// The demo freezes the position here.
    pub frozen_ms: Option<u64>,
}

impl Mon {
    /// A monitor as the card places itself on it.
    pub fn of(d: &Display) -> Mon {
        let rc = |r: crate::arrange::Rect| Rc {
            left: r.x,
            top: r.y,
            right: r.x + r.w,
            bottom: r.y + r.h,
        };
        Mon {
            rect: rc(d.monitor.rect),
            work: rc(d.monitor.work),
            dpi: d.monitor.dpi.max(96),
        }
    }
}

/// The monitors and the notch's one, for `OpenArgs`.
pub fn monitors_of(displays: &[Display], notch: Option<usize>) -> (Vec<Mon>, Option<Mon>) {
    (
        displays.iter().map(Mon::of).collect(),
        notch.and_then(|i| displays.get(i)).map(Mon::of),
    )
}

/// The card's gradient without art: the dark window palette's two surfaces
/// and its accent (Terminal's neutral family follows `theme::window_for`).
pub fn fallback_palette() -> [u32; 3] {
    let t = crate::theme::current();
    let p = crate::theme::window_for(t.accent, t.surface, crate::theme::WindowTheme::Dark, false);
    [p.card, p.bg, p.accent]
}

/// Opens the card with `content`. False when it could not (already open, or
/// no window or target). Shown without activating.
pub fn open(args: OpenArgs, content: CardContent) -> bool {
    window::open(args, content)
}

/// A new snapshot. Does nothing when the card is not open.
pub fn update(content: CardContent) {
    window::update(content);
}

/// Closes the card and frees its window and Direct2D objects.
pub fn close() {
    window::close();
}

/// The island is ending: the same as `close`, called before the process
/// ends so no Direct2D object survives to process detach.
pub fn release() {
    window::close();
}

/// The card's window, while it is open.
pub fn hwnd() -> Option<HWND> {
    window::hwnd()
}

/// Fullscreen on the card's monitor hides it (and stops its timers); true
/// when this changed anything.
pub fn set_hidden(hidden: bool) -> bool {
    window::set_hidden(hidden)
}

/// The session key of what the card shows now.
pub fn shown_key() -> Option<u64> {
    window::shown_key()
}

/// The rect to save after `Msg::Placed`.
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
    fn the_message_codes_round_trip_and_the_id_is_free() {
        for m in [
            Msg::Close,
            Msg::Seek,
            Msg::Prev,
            Msg::PlayPause,
            Msg::Next,
            Msg::Placed,
        ] {
            assert_eq!(Msg::from_code(m.code()), Some(m));
        }
        assert_eq!(Msg::from_code(0), None);
        assert_eq!(Msg::from_code(7), None);
        assert_eq!(WM_APP_LYRICS_CARD, WM_APP + 70);
    }

    #[test]
    fn the_window_class_is_the_documented_one() {
        assert_eq!(window::CLASS_NAME, "ValeLyricsCard");
    }
}
