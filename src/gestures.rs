//! Pointer gestures on the island while music plays: a swipe left or right
//! with the left button, a sideways wheel (a touchpad's two-finger scroll, a
//! tilt wheel) and the middle button. Plain window messages: no global hook.
//! The evaluators here are pure; `App` feeds them and decides where they
//! count (the music views, with a session).

use std::time::{Duration, Instant};

use crate::model::{Page, View};

/// A swipe's shortest way, in logical pixels.
pub const SWIPE_MIN_DIP: f32 = 56.0;
/// A swipe's longest time from press to release.
const SWIPE_MAX: Duration = Duration::from_millis(700);
/// A sideways wheel's delta that counts as one flick (one notch).
const HWHEEL_STEP: i32 = 120;
/// Deltas in the same direction at most this far apart are one gesture; a
/// longer gap, or a delta the other way, starts a new one.
const HWHEEL_NEW_GESTURE_GAP: Duration = Duration::from_millis(150);
/// After a flick, a new gesture in the same direction waits until this long
/// after the flick before it may fire. Counted from the flick only; later
/// deltas never push it out.
const HWHEEL_COOLDOWN: Duration = Duration::from_millis(500);

/// Which way a gesture asks the music to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Prev,
    Next,
}

/// A press with the left button and what it turns into on release.
#[derive(Default)]
pub struct Swipe {
    down: Option<(i32, i32, Instant)>,
}

impl Swipe {
    pub fn press(&mut self, x: i32, y: i32, now: Instant) {
        self.down = Some((x, y, now));
    }

    pub fn cancel(&mut self) {
        self.down = None;
    }

    /// The swipe the release ends, if it was one: far enough sideways
    /// (`min_px`), mostly sideways (twice as far across as up or down) and
    /// quick. Left is the next song, right the one before. Every release
    /// ends the press.
    pub fn release(&mut self, x: i32, y: i32, now: Instant, min_px: i32) -> Option<Dir> {
        let (x0, y0, t0) = self.down.take()?;
        let (dx, dy) = (x - x0, y - y0);
        let quick = now.saturating_duration_since(t0) <= SWIPE_MAX;
        (quick && dx.abs() >= min_px && dx.abs() >= 2 * dy.abs())
            .then(|| if dx < 0 { Dir::Next } else { Dir::Prev })
    }
}

/// The sideways wheel's deltas, added up until they make one flick.
#[derive(Default)]
pub struct HWheel {
    /// Sum of the current gesture's deltas.
    sum: i32,
    /// Sign of the current gesture (0: none yet).
    sign: i32,
    /// When the last delta came.
    last: Option<Instant>,
    /// The current gesture has fired; the rest of it (momentum) is ignored.
    spent: bool,
    /// Sign of the last flick, and when a same-direction gesture may fire
    /// again. Set only by a flick, never extended by later deltas.
    last_flick: i32,
    quiet_until: Option<Instant>,
}

impl HWheel {
    /// One `WM_MOUSEHWHEEL` delta (positive: scrolled right). Scrolling
    /// right is the next song or page, left the one before.
    ///
    /// A gesture is a run of deltas the same way with gaps of at most
    /// `HWHEEL_NEW_GESTURE_GAP`; the other way, or a longer gap, starts a new
    /// one (sum reset, not spent). Within a gesture the deltas add up and,
    /// at `HWHEEL_STEP`, it fires once and is spent: the rest of it, however
    /// long (a touchpad's momentum tail), returns `None`.
    ///
    /// A flick sets the quiet time to `HWHEEL_COOLDOWN` after it, and nothing
    /// extends it. A new gesture the same way as the last flick that reaches
    /// the step before then keeps its sum and fires on its first delta at or
    /// after the quiet time, if it is still going. One the other way fires at
    /// once. A delta of 0 is ignored.
    pub fn push(&mut self, delta: i32, now: Instant) -> Option<Dir> {
        if delta == 0 {
            return None;
        }
        let sign = delta.signum();
        let new_gesture = sign != self.sign
            || self
                .last
                .is_none_or(|t| now.saturating_duration_since(t) > HWHEEL_NEW_GESTURE_GAP);
        if new_gesture {
            self.sum = 0;
            self.sign = sign;
            self.spent = false;
        }
        self.last = Some(now);
        if self.spent {
            return None;
        }
        self.sum += delta;
        if self.sum.abs() < HWHEEL_STEP {
            return None;
        }
        let waiting = sign == self.last_flick && self.quiet_until.is_some_and(|t| now < t);
        if waiting {
            return None;
        }
        self.spent = true;
        self.last_flick = sign;
        self.quiet_until = Some(now + HWHEEL_COOLDOWN);
        Some(if sign > 0 { Dir::Next } else { Dir::Prev })
    }
}

/// What a sideways flick does, by where it lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HRoute {
    /// Turns one panel page (right = next, left = previous).
    Page,
    /// Skips the song (right = next).
    Song,
    Nothing,
}

/// Where a sideways flick goes (`WM_MOUSEHWHEEL`):
///
/// - an open panel's page, the Music page included, on the drawn island (not
///   the faint margin round it): one page, like the vertical wheel. This is
///   navigation, so it does not depend on `config.gestures`. Not on the
///   search (it is not in the order of pages), and not while the CLI folder
///   menu is open;
/// - the compact music notch (song or lyric line), with the gestures on and
///   a session: the song, as ever (the margin was never excluded there);
/// - anything else (a notice, idle, the search): nothing.
///
/// `gestures_media_ok` is `App::gestures_active` (setting, session, view).
pub fn hwheel_route(
    view: &View,
    cli_menu_open: bool,
    gestures_media_ok: bool,
    over_island: bool,
) -> HRoute {
    match view {
        View::Panel(Page::Search) => HRoute::Nothing,
        View::Panel(Page::Cli) if cli_menu_open => HRoute::Nothing,
        View::Panel(_) if over_island => HRoute::Page,
        View::MediaCompact | View::MediaLyric if gestures_media_ok => HRoute::Song,
        _ => HRoute::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_quick_sideways_drag_is_a_swipe() {
        let t0 = Instant::now();
        let mut s = Swipe::default();
        s.press(200, 10, t0);
        assert_eq!(s.release(120, 14, t0 + ms(180), 56), Some(Dir::Next));
        s.press(100, 10, t0);
        assert_eq!(s.release(190, 0, t0 + ms(300), 56), Some(Dir::Prev));
    }

    #[test]
    fn short_slow_or_diagonal_drags_and_releases_without_a_press_are_not() {
        let t0 = Instant::now();
        let mut s = Swipe::default();
        s.press(200, 10, t0);
        assert_eq!(s.release(160, 10, t0 + ms(100), 56), None, "too short");
        s.press(200, 10, t0);
        assert_eq!(s.release(100, 10, t0 + ms(900), 56), None, "too slow");
        s.press(200, 10, t0);
        assert_eq!(s.release(120, 70, t0 + ms(100), 56), None, "diagonal");
        assert_eq!(s.release(100, 10, t0 + ms(100), 56), None, "no press");
        // Every release ends the press; a cancel does too.
        s.press(200, 10, t0);
        s.cancel();
        assert_eq!(s.release(100, 10, t0 + ms(100), 56), None);
        // The bigger the screen scale, the further a swipe has to go.
        s.press(200, 10, t0);
        assert_eq!(s.release(120, 10, t0 + ms(100), 84), None);
    }

    #[test]
    fn a_sideways_wheel_flicks_once_per_scroll() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        // A notch.
        assert_eq!(w.push(120, t0), Some(Dir::Next));
        // A touchpad's momentum right after it: spent.
        for i in 1..20 {
            assert_eq!(w.push(40, t0 + ms(i * 20)), None);
        }
        // Later, small deltas add up; the other way is the other song.
        let t1 = t0 + ms(2_000);
        assert_eq!(w.push(-50, t1), None);
        assert_eq!(w.push(-50, t1 + ms(30)), None);
        assert_eq!(w.push(-50, t1 + ms(60)), Some(Dir::Prev));
    }

    /// (a) A burst then a long continuous tail turns exactly one page.
    #[test]
    fn one_momentum_tail_turns_exactly_one_page() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        let mut fired = 0;
        for i in 0..30 {
            fired += w.push(40, t0 + ms(i * 16)).is_some() as u32;
        }
        let tail = t0 + ms(30 * 16);
        for i in 0..=93 {
            fired += w.push(40, tail + ms(i * 16)).is_some() as u32;
        }
        assert_eq!(fired, 1);
    }

    /// (b) A second swipe after a gap and the quiet time turns another page.
    #[test]
    fn a_second_swipe_after_the_gap_and_quiet_turns_another_page() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        assert_eq!(w.push(120, t0), Some(Dir::Next));
        for i in 1..=10 {
            assert_eq!(w.push(40, t0 + ms(i * 16)), None);
        }
        let t1 = t0 + ms(160) + ms(600);
        assert_eq!(w.push(70, t1), None);
        assert_eq!(w.push(70, t1 + ms(16)), Some(Dir::Next));
    }

    /// (c) The other way turns a page at once, inside the quiet time.
    #[test]
    fn a_direction_change_turns_the_other_way_at_once() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        assert_eq!(w.push(120, t0), Some(Dir::Next));
        assert_eq!(w.push(-120, t0 + ms(100)), Some(Dir::Prev));
        // And back again, still inside the quiet time of the last flick.
        assert_eq!(w.push(120, t0 + ms(200)), Some(Dir::Next));
    }

    /// (d) Continuous deltas for seconds do not block paging for good.
    #[test]
    fn continuous_deltas_never_block_paging_forever() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        let mut fired = 0;
        for i in 0..=187 {
            fired += w.push(40, t0 + ms(i * 16)).is_some() as u32;
        }
        assert_eq!(fired, 1, "3 s of one scroll is one page");
        let t1 = t0 + ms(187 * 16) + ms(200);
        assert_eq!(w.push(70, t1), None);
        assert_eq!(w.push(70, t1 + ms(16)), Some(Dir::Next));
    }

    /// (e) A same-way swipe inside the quiet time waits for it to end.
    #[test]
    fn a_same_way_swipe_inside_the_quiet_time_fires_when_it_ends() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        assert_eq!(w.push(120, t0), Some(Dir::Next));
        // A new swipe 200 ms later (a gap over 150 ms), 40 per 20 ms.
        let mut fired = Vec::new();
        for i in 0..30 {
            let at = ms(200 + i * 20);
            if w.push(40, t0 + at).is_some() {
                fired.push(at);
            }
        }
        assert_eq!(fired, vec![ms(500)], "once, on the first delta at 500 ms");
    }

    /// (f) Small deltas still add up to one flick.
    #[test]
    fn small_deltas_add_up_to_one_flick() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        for i in 0..3 {
            assert_eq!(w.push(30, t0 + ms(i * 20)), None);
        }
        assert_eq!(w.push(30, t0 + ms(60)), Some(Dir::Next));
        assert_eq!(w.push(30, t0 + ms(80)), None, "the tail is spent");
    }

    #[test]
    fn a_zero_delta_is_ignored() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        assert_eq!(w.push(0, t0), None);
        assert_eq!(w.push(60, t0 + ms(10)), None);
        assert_eq!(w.push(0, t0 + ms(400)), None, "does not break the gesture");
        assert_eq!(w.push(60, t0 + ms(420)), None, "gap over 150 ms resets");
    }

    #[test]
    fn slow_drips_of_wheel_never_add_up() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        for i in 0..10 {
            assert_eq!(w.push(40, t0 + ms(i * 400)), None);
        }
    }

    /// A touchpad's two-finger scroll: ~30 small deltas over a second.
    #[test]
    fn a_touchpad_burst_turns_one_page_and_the_next_flick_another() {
        let t0 = Instant::now();
        let mut w = HWheel::default();
        let mut fired = 0;
        for i in 0..30 {
            if w.push(40, t0 + ms(i * 33)).is_some() {
                fired += 1;
            }
        }
        assert_eq!(fired, 1, "one flick, one page");
        // Still for 600 ms after the last delta, then another flick.
        let last = t0 + ms(29 * 33);
        let t1 = last + ms(600);
        assert_eq!(w.push(-60, t1), None);
        assert_eq!(w.push(-60, t1 + ms(30)), Some(Dir::Prev));
    }

    fn route(view: View, menu: bool, gestures: bool, over: bool) -> HRoute {
        hwheel_route(&view, menu, gestures, over)
    }

    #[test]
    fn every_panel_page_but_the_search_turns_on_a_sideways_flick() {
        for page in Page::ALL {
            for gestures in [false, true] {
                assert_eq!(
                    route(View::Panel(page), false, gestures, true),
                    HRoute::Page,
                    "{page:?} gestures {gestures}"
                );
            }
        }
        // Not on the faint margin.
        assert_eq!(
            route(View::Panel(Page::Media), false, true, false),
            HRoute::Nothing
        );
        assert_eq!(
            route(View::Panel(Page::Notes), false, false, false),
            HRoute::Nothing
        );
        // Not on the search, and not over the CLI folder menu.
        assert_eq!(
            route(View::Panel(Page::Search), false, true, true),
            HRoute::Nothing
        );
        assert_eq!(
            route(View::Panel(Page::Cli), true, true, true),
            HRoute::Nothing
        );
        assert_eq!(
            route(View::Panel(Page::Cli), false, true, true),
            HRoute::Page
        );
        // The menu flag only counts on the CLI page.
        assert_eq!(
            route(View::Panel(Page::Notes), true, false, true),
            HRoute::Page
        );
    }

    #[test]
    fn the_compact_music_notch_skips_songs_only_with_gestures_on() {
        for view in [View::MediaCompact, View::MediaLyric] {
            assert_eq!(route(view.clone(), false, true, true), HRoute::Song);
            assert_eq!(route(view.clone(), false, true, false), HRoute::Song);
            assert_eq!(route(view, false, false, true), HRoute::Nothing);
        }
    }

    #[test]
    fn notices_idle_and_the_rest_do_nothing() {
        for view in [
            View::Hidden,
            View::Idle,
            View::PomodoroCompact,
            View::RecordCompact,
        ] {
            assert_eq!(route(view, false, true, true), HRoute::Nothing);
        }
    }
}
