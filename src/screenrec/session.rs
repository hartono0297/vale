//! The rules of one screen recording session, apart from the windows and the
//! threads: the recorder's options from the settings, what pauses it, and the
//! clock the island shows.

use std::time::{Duration, Instant};

use super::Options;
use crate::model::RecSources;

/// The sidecar's name for the one audio channel: mic and PC audio summed.
pub const MIX_CHANNEL: &str = "mix";

/// The cursor effects drawn into the video, from the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorEffects {
    pub highlight: bool,
    pub ripples: bool,
}

/// The recorder's options for a recording at `fps` (60, anything else is 30)
/// of the window called `title`, with one mono track when a source is on.
pub fn options_for(fps: u32, sources: RecSources, title: &str, effects: CursorEffects) -> Options {
    let audio = sources.mic || sources.system;
    Options {
        fps: if fps == 60 { 60 } else { 30 },
        max_short: None,
        audio_channels: audio.then_some(1),
        channel_names: if audio {
            vec![MIX_CHANNEL.to_owned()]
        } else {
            Vec::new()
        },
        draw_cursor: true,
        cursor_highlight: effects.highlight,
        click_ripples: effects.ripples,
        prefer_hardware: true,
        side_files: true,
        title: title.to_owned(),
        app: None,
    }
}

/// Why a recording pauses on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseWhy {
    /// The PC sleeps or is locked.
    Away,
    /// The recorded window is minimised.
    Minimised,
}

/// What holds a screen recording paused. Each cause is kept apart, so the
/// recording resumes only when the last one ends and the user's own pause
/// outlasts every automatic one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenPause {
    user: bool,
    away: bool,
    minimised: bool,
}

impl ScreenPause {
    pub fn paused(&self) -> bool {
        self.user || self.away || self.minimised
    }

    /// An automatic cause began (`on`) or ended. Returns the new paused state
    /// when that changes it.
    pub fn set(&mut self, why: PauseWhy, on: bool) -> Option<bool> {
        let was = self.paused();
        match why {
            PauseWhy::Away => self.away = on,
            PauseWhy::Minimised => self.minimised = on,
        }
        let now = self.paused();
        (now != was).then_some(now)
    }

    /// The user's pause button. While paused, for any reason, it resumes and
    /// forgets every cause; while running it pauses. Returns the new paused
    /// state.
    pub fn user_toggle(&mut self) -> bool {
        *self = ScreenPause {
            user: !self.paused(),
            ..ScreenPause::default()
        };
        self.user
    }
}

/// The recording's clock for the island: the time since `started` without
/// the pauses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenClock {
    pub started: Instant,
    pub paused_total: Duration,
    pub paused_since: Option<Instant>,
}

impl ScreenClock {
    pub fn new(now: Instant) -> ScreenClock {
        ScreenClock {
            started: now,
            paused_total: Duration::ZERO,
            paused_since: None,
        }
    }

    /// A pause begins; nothing when one is already open.
    pub fn pause(&mut self, now: Instant) {
        self.paused_since.get_or_insert(now);
    }

    /// The open pause ends; nothing when the clock runs.
    pub fn resume(&mut self, now: Instant) {
        if let Some(since) = self.paused_since.take() {
            self.paused_total += now.saturating_duration_since(since);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(mic: bool, system: bool) -> RecSources {
        RecSources { mic, system }
    }

    const ALL: CursorEffects = CursorEffects {
        highlight: true,
        ripples: true,
    };

    #[test]
    fn the_cursor_effects_come_from_the_settings() {
        let off = CursorEffects { highlight: false, ripples: false };
        let o = options_for(30, sources(true, true), "", off);
        assert!(!o.cursor_highlight && !o.click_ripples && o.draw_cursor);
        let on = CursorEffects { highlight: true, ripples: true };
        let o = options_for(30, sources(true, true), "", on);
        assert!(o.cursor_highlight && o.click_ripples);
    }

    #[test]
    fn options_follow_the_prefs() {
        let o = options_for(60, sources(true, false), "Figma", ALL);
        assert_eq!(o.fps, 60);
        assert_eq!(o.audio_channels, Some(1));
        assert_eq!(o.channel_names, ["mix"]);
        assert!(o.draw_cursor && o.prefer_hardware && o.side_files);
        assert_eq!(o.max_short, None);
        assert_eq!(o.app, None);
        assert_eq!(o.title, "Figma");

        let o = options_for(45, sources(false, false), "", ALL);
        assert_eq!(o.fps, 30);
        assert_eq!(o.audio_channels, None);
        assert!(o.channel_names.is_empty());
        assert_eq!(o.title, "");

        // PC audio alone is a track too.
        assert_eq!(options_for(30, sources(false, true), "", ALL).audio_channels, Some(1));
    }

    #[test]
    fn a_minimised_window_pauses_and_its_restore_resumes() {
        let mut p = ScreenPause::default();
        assert!(!p.paused());
        assert_eq!(p.set(PauseWhy::Minimised, true), Some(true));
        assert!(p.paused());
        assert_eq!(p.set(PauseWhy::Minimised, true), None, "told twice changes nothing");
        assert_eq!(p.set(PauseWhy::Minimised, false), Some(false));
        assert!(!p.paused());
    }

    #[test]
    fn a_user_pause_survives_restore_and_wake() {
        let mut p = ScreenPause::default();
        assert!(p.user_toggle());
        assert_eq!(p.set(PauseWhy::Minimised, true), None);
        assert_eq!(p.set(PauseWhy::Minimised, false), None);
        assert_eq!(p.set(PauseWhy::Away, true), None);
        assert_eq!(p.set(PauseWhy::Away, false), None);
        assert!(p.paused(), "only the user ends the user's pause");
        assert!(!p.user_toggle());
        assert!(!p.paused());
    }

    #[test]
    fn sleep_and_minimise_both_must_end_before_resume() {
        let mut p = ScreenPause::default();
        assert_eq!(p.set(PauseWhy::Away, true), Some(true));
        assert_eq!(p.set(PauseWhy::Minimised, true), None);
        assert_eq!(p.set(PauseWhy::Away, false), None);
        assert!(p.paused());
        assert_eq!(p.set(PauseWhy::Minimised, false), Some(false));
    }

    #[test]
    fn the_user_resumes_over_an_automatic_pause() {
        let mut p = ScreenPause::default();
        assert_eq!(p.set(PauseWhy::Minimised, true), Some(true));
        assert!(!p.user_toggle());
        assert!(!p.paused());
        assert_eq!(p.set(PauseWhy::Minimised, false), None);
        assert!(!p.paused());
    }

    #[test]
    fn the_clock_leaves_pauses_out() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut c = ScreenClock::new(t0);
        assert_eq!((c.started, c.paused_total, c.paused_since), (t0, Duration::ZERO, None));

        c.pause(at(10));
        c.pause(at(12));
        assert_eq!(c.paused_since, Some(at(10)), "a second pause keeps the first start");
        c.resume(at(15));
        c.resume(at(40));
        assert_eq!(c.paused_total, Duration::from_secs(5), "a second resume adds nothing");
        assert_eq!(c.paused_since, None);

        c.pause(at(60));
        c.resume(at(63));
        assert_eq!(c.paused_total, Duration::from_secs(8));
        assert_eq!(c.started, t0);
        // 100 s after the start, 92 s were recorded.
        assert_eq!(at(100).duration_since(c.started) - c.paused_total, Duration::from_secs(92));
    }
}
