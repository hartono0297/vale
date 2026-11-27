use std::time::Duration;

use crate::ui::play::trigger::next_hour;

/// How long after the hour the reminder may still show.
pub const WINDOW: Duration = Duration::from_secs(600);
/// Wait between tries while a gate blocks the reminder.
pub const RETRY: Duration = Duration::from_secs(30);
/// Latest point in the window at which a panel-open block still retries.
pub const LAST_TRY: Duration = Duration::from_secs(570);
/// Extra delay added to each timer so it fires just after the hour.
pub const ARM_MARGIN_MS: u32 = 1000;
/// Idle time after which the user counts as away.
pub const AWAY_AFTER: Duration = Duration::from_secs(600);

/// Whether `hour` is one of the reminder hours (01:00 to 04:00).
pub fn is_reminder_hour(hour: u32) -> bool {
    (1..=4).contains(&hour)
}

/// Milliseconds until the next reminder hour strictly after the given time, plus `ARM_MARGIN_MS`.
pub fn next_ms(hour: u32, min: u32, sec: u32) -> u32 {
    let now = hour * 3600 + min * 60 + sec;
    let target = (1..=4)
        .map(|h| h * 3600)
        .find(|&t| t > now)
        .unwrap_or(25 * 3600);
    (target - now) * 1000 + ARM_MARGIN_MS
}

/// The reminder hour and time since it started, when inside the window.
pub fn in_window(hour: u32, min: u32, sec: u32) -> Option<(u8, Duration)> {
    if !is_reminder_hour(hour) {
        return None;
    }
    let since = Duration::from_secs(u64::from(min * 60 + sec));
    (since < WINDOW).then_some((hour as u8, since))
}

/// Which screen and activity conditions currently hold.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gates {
    pub screen_away: bool,
    pub fullscreen: bool,
    pub recording: bool,
    pub idle_for: Duration,
    pub menu_open: bool,
    pub timed_notice: bool,
    pub panel_open: bool,
}

/// What to do with the reminder right now.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Show,
    Retry,
    Drop,
}

/// Decide whether to show, retry later or drop the reminder.
pub fn decide(g: &Gates, since_hour: Duration) -> Step {
    if since_hour >= WINDOW {
        return Step::Drop;
    }
    let blocked = g.screen_away
        || g.fullscreen
        || g.recording
        || g.idle_for >= AWAY_AFTER
        || g.menu_open
        || g.timed_notice
        || (g.panel_open && since_hour < LAST_TRY);
    if !blocked {
        Step::Show
    } else if since_hour + RETRY < WINDOW {
        Step::Retry
    } else {
        Step::Drop
    }
}

/// Wait until the next story hour, skipping reminder hours when `skip_night` is set.
pub fn story_wait(hour: u32, min: u32, sec: u32, step: u8, skip_night: bool) -> Option<Duration> {
    let mut wait = next_hour(hour, min, sec, step)?;
    if !skip_night {
        return Some(wait);
    }
    let now = hour * 3600 + min * 60 + sec;
    let step_secs = u64::from(step) * 3600;
    loop {
        let target = (u64::from(now) + wait.as_secs()) / 3600 % 24;
        if !is_reminder_hour(target as u32) {
            break;
        }
        wait += Duration::from_secs(step_secs);
    }
    Some(wait)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn clear() -> Gates {
        Gates::default()
    }

    #[test]
    fn next_ms_points_to_the_next_reminder_hour() {
        assert_eq!(next_ms(0, 0, 0), 3_601_000);
        assert_eq!(next_ms(0, 59, 59), 2_000);
        assert_eq!(next_ms(1, 0, 0), 3_601_000);
        assert_eq!(next_ms(3, 30, 0), 1_801_000);
        assert_eq!(next_ms(4, 0, 0), 75_601_000);
        assert_eq!(next_ms(23, 0, 0), 7_201_000);
    }

    #[test]
    fn in_window_only_inside_ten_minutes_of_a_reminder_hour() {
        assert_eq!(in_window(1, 0, 1), Some((1, secs(1))));
        assert_eq!(in_window(2, 9, 59), Some((2, secs(599))));
        assert_eq!(in_window(2, 10, 0), None);
        assert_eq!(in_window(0, 5, 0), None);
        assert_eq!(in_window(5, 0, 1), None);
        assert_eq!(in_window(4, 3, 0), Some((4, secs(180))));
    }

    #[test]
    fn decide_shows_when_clear() {
        assert_eq!(decide(&clear(), secs(60)), Step::Show);
    }

    #[test]
    fn decide_retries_while_any_gate_blocks_with_room_left() {
        let blocks = [
            Gates { screen_away: true, ..clear() },
            Gates { fullscreen: true, ..clear() },
            Gates { recording: true, ..clear() },
            Gates { menu_open: true, ..clear() },
            Gates { timed_notice: true, ..clear() },
            Gates { idle_for: AWAY_AFTER, ..clear() },
        ];
        for g in &blocks {
            assert_eq!(decide(g, secs(60)), Step::Retry, "{g:?}");
            assert_eq!(decide(g, secs(575)), Step::Drop, "{g:?}");
        }
    }

    #[test]
    fn decide_panel_open_blocks_only_before_last_try() {
        let g = Gates { panel_open: true, ..clear() };
        assert_eq!(decide(&g, secs(60)), Step::Retry);
        assert_eq!(decide(&g, secs(570)), Step::Show);
    }

    #[test]
    fn decide_drops_at_the_end_of_the_window() {
        assert_eq!(decide(&clear(), WINDOW), Step::Drop);
    }

    #[test]
    fn decide_short_idle_does_not_block() {
        let g = Gates { idle_for: secs(599), ..clear() };
        assert_eq!(decide(&g, secs(60)), Step::Show);
    }

    #[test]
    fn story_wait_steps_over_the_night_when_asked() {
        assert_eq!(story_wait(0, 30, 0, 1, true), Some(secs(4 * 3600 + 30 * 60)));
        assert_eq!(story_wait(0, 30, 0, 1, false), Some(secs(30 * 60)));
        assert_eq!(story_wait(0, 30, 0, 2, true), Some(secs(5 * 3600 + 30 * 60)));
        assert_eq!(story_wait(22, 0, 0, 2, true), Some(secs(2 * 3600)));
        assert_eq!(story_wait(4, 10, 0, 1, true), Some(secs(50 * 60)));
        assert_eq!(story_wait(4, 10, 0, 0, true), None);
    }
}
