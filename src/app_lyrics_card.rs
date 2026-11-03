//! The island's side of the floating lyrics card (`crate::lyrics_card`): what
//! is open for this run, the snapshot it is sent (only what changed), and what
//! the user's clicks in the card do. A child of `app`, so it works on `App`
//! directly.
//!
//! The card is open for this run only and is never reopened at startup. It
//! follows the island's shown session; the media-session ranking and the
//! lyrics lookup are untouched (the card only reads `LyricsState`). The
//! card's window procedure never calls into `App`: it posts
//! `WM_APP_LYRICS_CARD` and `on_lyrics_card_msg` answers it.
//!
//! Power: `sync_lyrics_card` runs at the end of `refresh` and after a lyrics
//! answer, builds one small snapshot and calls the card only when something
//! changed. The 1 s `TIMER_SYSTEM` tick adds one `is_fullscreen` check, and
//! only while the card is open.

use std::sync::Arc;
use std::time::Instant;

use super::App;
use crate::arrange_overlay;
use crate::lyrics::{self, CardStatus};
use crate::lyrics_card::{
    self as card, CardContent, Change, Msg, OpenArgs, Req, Status, card_next,
};
use crate::model::{Effect, MediaCommand, Timeline};
use crate::notch_monitor;
use crate::sources::fullscreen::is_fullscreen;
use crate::sources::media;

/// What the card was last sent, apart from the heavy parts (the art and the
/// lines, which have their own keys).
#[derive(Clone, Debug, PartialEq)]
struct Sent {
    status: Status,
    title: String,
    artist: String,
    timeline: Option<Timeline>,
    playing: bool,
    can_seek: bool,
    session_key: u64,
    palette: [u32; 3],
    font: crate::config::UiFont,
    animations: bool,
}

#[derive(Default)]
pub(super) struct LyricsCardState {
    /// Open for this run only.
    pub(super) open: bool,
    last: Option<Sent>,
    /// The lyrics key whose lines the card holds (`None`: none).
    lines_key: Option<String>,
    /// A cheap fingerprint of the art the card holds (outer `None`: never sent).
    art_sig: Option<Option<(usize, u64)>>,
    /// Hidden because a fullscreen app covers the card's monitor.
    hidden: bool,
}

/// Length and 64 sampled bytes: enough to tell one cover from another without
/// reading all of it at every refresh.
fn art_sig(art: Option<&[u8]>) -> Option<(usize, u64)> {
    let bytes = art?;
    let step = (bytes.len() / 64).max(1);
    let hash = bytes.iter().step_by(step).fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    Some((bytes.len(), hash))
}

fn status_of(s: CardStatus) -> Status {
    match s {
        CardStatus::Pending => Status::Pending,
        CardStatus::Synced => Status::Synced,
        CardStatus::Missing => Status::Missing,
    }
}

impl App {
    /// The hotkey: nothing while lyrics are off.
    pub(super) fn on_lyrics_card_hotkey(&mut self) {
        if !self.state.lyrics_on() {
            return;
        }
        self.toggle_lyrics_card();
    }

    /// The hotkey and the Musik page's button: opens or closes the card
    /// (`card_next` decides; with lyrics off nothing happens).
    pub(super) fn toggle_lyrics_card(&mut self) {
        let open = self.lyrics_card.open;
        let next = card_next(open, self.state.lyrics_on(), Req::Toggle);
        if next == open {
            return;
        }
        if next {
            self.open_lyrics_card();
        } else {
            self.close_lyrics_card();
        }
    }

    fn open_lyrics_card(&mut self) {
        let notch = notch_monitor::resolve(&self.config.notch_monitor, &self.displays);
        let (monitors, notch) = card::monitors_of(&self.displays, notch);
        let args = OpenArgs {
            island: self.hwnd,
            saved: self.config.lyrics_card,
            monitors,
            notch,
            hidden: self.state.fullscreen(),
            demo: false,
            frozen_ms: None,
        };
        let content = self.card_content(true);
        if card::open(args, content) {
            self.lyrics_card.open = true;
            self.lyrics_card.hidden = self.state.fullscreen();
            self.lyrics_card.last = None;
            // The Musik page's button lights up.
            self.draw(Instant::now());
        }
    }

    /// Closes the card (the X, Esc, the hotkey again, lyrics off, exit).
    pub(super) fn close_lyrics_card(&mut self) {
        if !self.lyrics_card.open {
            return;
        }
        card::close();
        self.lyrics_card = LyricsCardState::default();
        self.draw(Instant::now());
    }

    /// Lyrics were switched off by any path: an open card closes.
    pub(super) fn lyrics_card_lyrics_off(&mut self) {
        if self.lyrics_card.open && !card_next(true, false, Req::LyricsOff) {
            self.close_lyrics_card();
        }
    }

    /// The snapshot of what the island shows. `all`: the art and the lines go
    /// in whatever was sent before (the card was just opened).
    fn card_content(&mut self, all: bool) -> CardContent {
        let media = self.state.media();
        let timeline = self.state.timeline().copied();
        let query = lyrics::track_query(media, timeline.as_ref());
        let status = match (media, &query) {
            (None, _) => Status::NoMedia,
            // No title, artist or length to look up: nothing to find.
            (Some(_), None) => Status::Missing,
            (Some(_), Some(_)) => status_of(self.lyrics.card_status(query.as_ref())),
        };
        let lines = if status == Status::Synced {
            let key = query.as_ref().map(|q| q.key.clone());
            if all || self.lyrics_card.lines_key != key {
                self.lyrics_card.lines_key = key;
                let held = self.lyrics.lines_for(query.as_ref()).unwrap_or(&[]);
                Change::Set(Arc::new(held.to_vec()))
            } else {
                Change::Keep
            }
        } else if all || self.lyrics_card.lines_key.is_some() {
            self.lyrics_card.lines_key = None;
            Change::Set(Arc::new(Vec::new()))
        } else {
            Change::Keep
        };
        let art_bytes = media.and_then(|m| m.art.as_deref());
        let sig = art_sig(art_bytes);
        let art = if all || self.lyrics_card.art_sig != Some(sig) {
            self.lyrics_card.art_sig = Some(sig);
            Change::Set(art_bytes.map(<[u8]>::to_vec))
        } else {
            Change::Keep
        };
        CardContent {
            status,
            title: media.map(|m| m.title.clone()).unwrap_or_default(),
            artist: media.map(|m| m.artist.clone()).unwrap_or_default(),
            art,
            lines,
            timeline: if media.is_some() { timeline } else { None },
            playing: media.is_some_and(|m| m.playing),
            can_seek: media.is_some_and(|m| m.can_seek),
            session_key: media.map_or(0, |m| m.session_key),
            palette: card::fallback_palette(),
            font: self.config.font,
            animations: self.animations,
        }
    }

    /// At the end of `refresh` and after a lyrics answer: closes the card when
    /// lyrics went off, else sends it what changed (and only then).
    pub(super) fn sync_lyrics_card(&mut self, _now: Instant) {
        if !self.lyrics_card.open {
            return;
        }
        if !self.state.lyrics_on() {
            self.lyrics_card_lyrics_off();
            return;
        }
        let content = self.card_content(false);
        let sent = Sent {
            status: content.status,
            title: content.title.clone(),
            artist: content.artist.clone(),
            timeline: content.timeline,
            playing: content.playing,
            can_seek: content.can_seek,
            session_key: content.session_key,
            palette: content.palette,
            font: content.font,
            animations: content.animations,
        };
        let heavy = matches!(content.art, Change::Set(_)) || matches!(content.lines, Change::Set(_));
        if !heavy && self.lyrics_card.last.as_ref() == Some(&sent) {
            return;
        }
        self.lyrics_card.last = Some(sent);
        card::update(content);
    }

    /// The 1 s system tick: a fullscreen app on the card's own monitor hides
    /// it, and it comes back when that ends. Nothing while the card is closed
    /// or Susun's own overlay (which looks fullscreen to Windows) is up. The
    /// notch, the toasts and the panel are not affected.
    pub(super) fn check_lyrics_card_fullscreen(&mut self) {
        if !self.lyrics_card.open || arrange_overlay::active() {
            return;
        }
        let Some(hwnd) = card::hwnd() else {
            return;
        };
        let hide = is_fullscreen(Some(hwnd));
        if hide != self.lyrics_card.hidden {
            self.lyrics_card.hidden = hide;
            card::set_hidden(hide);
        }
    }

    /// What the user did in the card.
    pub(super) fn on_lyrics_card_msg(&mut self, code: usize, value: isize) {
        let Some(msg) = Msg::from_code(code) else {
            return;
        };
        if !self.lyrics_card.open {
            return;
        }
        match msg {
            Msg::Close => {
                if !card_next(true, self.state.lyrics_on(), Req::Close) {
                    self.close_lyrics_card();
                }
            }
            Msg::Placed => {
                if let Some(place) = card::take_placed() {
                    self.config.lyrics_card = Some(place);
                    self.save_config();
                }
            }
            Msg::Prev => self.card_media(MediaCommand::Prev),
            Msg::Next => self.card_media(MediaCommand::Next),
            // What the island shows decides, like the panel's button.
            Msg::PlayPause => {
                let Some(playing) = self.state.media().map(|m| m.playing) else {
                    return;
                };
                self.card_media(if playing {
                    MediaCommand::Pause
                } else {
                    MediaCommand::Play
                });
            }
            Msg::Seek => {
                let Some(ms) = u64::try_from(value).ok() else {
                    return;
                };
                // Only while the session still allows it and is the one the
                // card showed.
                let Some(m) = self.state.media() else {
                    return;
                };
                if !m.can_seek || card::shown_key() != Some(m.session_key) {
                    return;
                }
                let target = media::Target {
                    key: m.session_key,
                    app: m.source_app.clone(),
                };
                media::send_command(MediaCommand::Seek(ms), target, None);
            }
        }
    }

    /// A transport command from the card: `Effect::Media` exactly like the
    /// panel's, to the shown session.
    fn card_media(&mut self, cmd: MediaCommand) {
        self.media_click = None;
        let _ = self.perform(Effect::Media(cmd), Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics_card::layout;

    #[test]
    fn the_art_fingerprint_tells_covers_apart_cheaply() {
        assert_eq!(art_sig(None), None);
        let a: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let b: Vec<u8> = (0..5000u32).map(|i| (i % 241) as u8).collect();
        assert_eq!(art_sig(Some(&a)), art_sig(Some(&a)));
        assert_ne!(art_sig(Some(&a)), art_sig(Some(&b)));
        // Short, even empty, covers do not panic.
        assert!(art_sig(Some(&[])).is_some());
        assert!(art_sig(Some(&[1])).is_some());
    }

    #[test]
    fn the_lyrics_status_maps_one_to_one() {
        assert_eq!(status_of(CardStatus::Pending), Status::Pending);
        assert_eq!(status_of(CardStatus::Synced), Status::Synced);
        assert_eq!(status_of(CardStatus::Missing), Status::Missing);
    }

    #[test]
    fn a_new_card_state_is_closed_and_remembers_nothing() {
        let s = LyricsCardState::default();
        assert!(!s.open && !s.hidden);
        assert!(s.last.is_none() && s.lines_key.is_none() && s.art_sig.is_none());
        // The placement limits the card uses.
        assert_eq!((layout::MIN_W, layout::MAX_H), (240.0, 800.0));
    }
}
