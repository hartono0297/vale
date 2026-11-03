//! Opt-in diagnosis log for "the music paused by itself": every transport
//! command the island sends (or drops) and every playback status change the
//! watcher sees, in `media.log` beside config.json. Off unless `--media-log`
//! or the hidden `media_log` config field turns it on. Never records a title
//! or artist: the formatters only take the fields they print.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Local, NaiveDateTime};

use crate::config;
use crate::model::{Hit, MediaCommand, Page};

/// The command-line flag that turns the log on for this run.
pub const FLAG: &str = "--media-log";
/// Past this size the file moves to `media.log.1` (replacing the older one).
pub const MAX_BYTES: u64 = 1024 * 1024;

static ENABLED: AtomicBool = AtomicBool::new(false);
/// One writer at a time: the UI, the watcher and command threads all log.
static WRITE: Mutex<()> = Mutex::new(());

/// Whether this run logs: the flag or the config field asks for it.
pub fn requested(flag: bool, config: bool) -> bool {
    flag || config
}

/// Set once at startup.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// `media.log` in the island's data folder (where config.json lives).
pub fn path() -> PathBuf {
    let config = config::path();
    config
        .parent()
        .map_or_else(|| PathBuf::from("media.log"), |dir| dir.join("media.log"))
}

/// `<file>.1`, the one older log kept.
fn rotated(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

/// Appends `line` when the log is on; failures are ignored.
fn write(line: &str) {
    let _guard = WRITE.lock();
    let _ = append_if(enabled(), &path(), line, MAX_BYTES);
}

/// Appends `line` to `path` when `on`, first rotating a file that would grow
/// past `cap`. Off, it touches nothing (the file is never created).
fn append_if(on: bool, path: &Path, line: &str, cap: u64) -> io::Result<()> {
    if !on {
        return Ok(());
    }
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir)?;
    }
    let len = fs::metadata(path).map_or(0, |m| m.len());
    if len > 0 && len + line.len() as u64 + 2 > cap {
        // `rename` replaces an existing `.1` on Windows.
        let _ = fs::rename(path, rotated(path));
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    write!(file, "{line}\r\n")
}

fn stamp(at: NaiveDateTime) -> String {
    at.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

/// A click on a transport button, as the island saw it.
#[derive(Clone, Debug, PartialEq)]
pub struct Click {
    pub hit: Hit,
    /// The panel page at the click.
    pub page: Page,
    /// How long ago hover opened the panel; `None` when not opened by hover.
    pub hover_open_ms: Option<u64>,
    /// The hover guard was still on (the click is dropped).
    pub guarded: bool,
}

/// What became of a click.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome<'a> {
    /// Sent to the session with the target id, if one was found.
    Sent {
        cmd: &'a MediaCommand,
        session_found: bool,
    },
    /// Not sent: the hover guard, or nothing playing to command.
    Ignored(&'static str),
}

/// One command line. Takes only what it prints: no title, no artist.
pub fn format_command(at: NaiveDateTime, click: &Click, outcome: Outcome, app: &str) -> String {
    let hover = match click.hover_open_ms {
        None => "none".to_string(),
        Some(ms) if click.guarded => format!("{ms}ms(guard)"),
        Some(ms) => format!("{ms}ms"),
    };
    let (cmd, session) = match outcome {
        Outcome::Sent { cmd, session_found } => (
            format!("{cmd:?}"),
            if session_found { "found" } else { "none" },
        ),
        Outcome::Ignored(why) => (format!("ignored({why})"), "-"),
    };
    let app = if app.is_empty() { "-" } else { app };
    format!(
        "{} command hit={:?} cmd={cmd} page={:?} hover_open={hover} app={app} session={session}",
        stamp(at),
        click.hit,
        click.page,
    )
}

/// One playback status line. Takes only the session's id and status.
pub fn format_status(at: NaiveDateTime, app: &str, status: &str) -> String {
    let app = if app.is_empty() { "-" } else { app };
    format!("{} status app={app} status={status}", stamp(at))
}

/// One choice line: which session the watcher now shows (`-` for none) and
/// how many were live to pick from. The id only: no title, artist or album.
pub fn format_choice(at: NaiveDateTime, app: &str, live: usize) -> String {
    let app = if app.is_empty() { "-" } else { app };
    format!("{} choose app={app} of={live}", stamp(at))
}

pub fn log_choice(app: &str, live: usize) {
    if enabled() {
        write(&format_choice(Local::now().naive_local(), app, live));
    }
}

pub fn log_command(click: &Click, outcome: Outcome, app: &str) {
    if enabled() {
        write(&format_command(
            Local::now().naive_local(),
            click,
            outcome,
            app,
        ));
    }
}

pub fn log_status(app: &str, status: &str) {
    if enabled() {
        write(&format_status(Local::now().naive_local(), app, status));
    }
}

/// The last (session id, status) pair logged, so a status is written when it
/// changes and not on every timeline update.
#[derive(Default)]
pub struct StatusSeen(Option<(String, &'static str)>);

impl StatusSeen {
    /// True (and remembers it) when the pair differs from the last one.
    pub fn changed(&mut self, app: &str, status: &'static str) -> bool {
        if matches!(&self.0, Some((a, s)) if a == app && *s == status) {
            return false;
        }
        self.0 = Some((app.to_string(), status));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaInfo;

    fn at() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 27)
            .unwrap()
            .and_hms_milli_opt(14, 3, 12, 45)
            .unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vale-media-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn lines_never_hold_the_title_or_artist() {
        let media = MediaInfo {
            title: "Bohemian Rhapsody".into(),
            artist: "Queen".into(),
            playing: true,
            art: None,
            source_app: "Chrome".into(),
            session_key: 0,
            can_seek: false,
        };
        let click = Click {
            hit: Hit::PlayPause,
            page: Page::Media,
            hover_open_ms: Some(120),
            guarded: true,
        };
        let cmd = MediaCommand::Pause;
        let lines = [
            format_command(at(), &click, Outcome::Ignored("guard"), &media.source_app),
            format_command(
                at(),
                &click,
                Outcome::Sent {
                    cmd: &cmd,
                    session_found: true,
                },
                &media.source_app,
            ),
            format_status(at(), &media.source_app, "Playing"),
        ];
        for line in &lines {
            assert!(!line.contains(&media.title), "{line}");
            assert!(!line.contains(&media.artist), "{line}");
            assert!(line.contains("app=Chrome"), "{line}");
            assert!(line.starts_with("2026-10-27 14:03:12.045 "), "{line}");
        }
        assert_eq!(
            lines[0],
            "2026-10-27 14:03:12.045 command hit=PlayPause cmd=ignored(guard) page=Media \
             hover_open=120ms(guard) app=Chrome session=-"
        );
        assert!(lines[1].contains("cmd=Pause"));
        assert!(lines[1].contains("session=found"));
        assert_eq!(
            lines[2],
            "2026-10-27 14:03:12.045 status app=Chrome status=Playing"
        );
    }

    #[test]
    fn a_choice_line_holds_the_id_and_the_count_only() {
        assert_eq!(
            format_choice(at(), "Spotify.exe", 2),
            "2026-10-27 14:03:12.045 choose app=Spotify.exe of=2"
        );
        assert_eq!(
            format_choice(at(), "", 0),
            "2026-10-27 14:03:12.045 choose app=- of=0"
        );
    }

    #[test]
    fn a_click_without_hover_open_says_none() {
        let click = Click {
            hit: Hit::Next,
            page: Page::Media,
            hover_open_ms: None,
            guarded: false,
        };
        let line = format_command(
            at(),
            &click,
            Outcome::Sent {
                cmd: &MediaCommand::Next,
                session_found: false,
            },
            "",
        );
        assert!(line.contains("hover_open=none"), "{line}");
        assert!(line.contains("app=- session=none"), "{line}");
    }

    #[test]
    fn a_seek_is_logged_by_its_position_only() {
        let click = Click {
            hit: Hit::Next,
            page: Page::Media,
            hover_open_ms: None,
            guarded: false,
        };
        let line = format_command(
            at(),
            &click,
            Outcome::Sent {
                cmd: &MediaCommand::Seek(95_030),
                session_found: true,
            },
            "Spotify.exe",
        );
        assert!(line.contains("cmd=Seek(95030)"), "{line}");
        assert!(line.contains("session=found"), "{line}");
    }

    #[test]
    fn rotates_before_passing_the_cap() {
        let dir = temp_dir("rotate");
        let path = dir.join("media.log");
        let line = "x".repeat(40);
        // 42 bytes a line with CRLF: two fit under 100, the third rotates.
        append_if(true, &path, &line, 100).unwrap();
        append_if(true, &path, &line, 100).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), 84);
        assert!(!rotated(&path).exists());
        append_if(true, &path, &line, 100).unwrap();
        assert_eq!(fs::metadata(rotated(&path)).unwrap().len(), 84);
        assert_eq!(fs::read_to_string(&path).unwrap(), format!("{line}\r\n"));
        // 47 then 89 bytes; the next line would pass 100: the next rotation
        // replaces the older `.1`.
        append_if(true, &path, "new", 100).unwrap();
        append_if(true, &path, &line, 100).unwrap();
        assert!(fs::metadata(&path).unwrap().len() <= 100);
        let last = "y".repeat(40);
        append_if(true, &path, &last, 100).unwrap();
        assert_eq!(
            fs::read_to_string(rotated(&path)).unwrap(),
            format!("{line}\r\nnew\r\n{line}\r\n")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), format!("{last}\r\n"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_disabled_log_never_creates_the_file() {
        let dir = temp_dir("off");
        let path = dir.join("media.log");
        append_if(false, &path, "line", MAX_BYTES).unwrap();
        assert!(!path.exists());
        assert!(!dir.exists());
    }

    #[test]
    fn the_flag_or_the_config_turns_it_on() {
        assert!(!requested(false, false));
        assert!(requested(true, false));
        assert!(requested(false, true));
        assert!(requested(true, true));
    }

    #[test]
    fn lives_beside_the_config() {
        assert_eq!(path().parent(), config::path().parent());
        assert_eq!(path().file_name().unwrap(), "media.log");
    }

    #[test]
    fn status_is_logged_on_change_only() {
        let mut seen = StatusSeen::default();
        assert!(seen.changed("Chrome", "Playing"));
        assert!(!seen.changed("Chrome", "Playing"), "a timeline tick");
        assert!(seen.changed("Chrome", "Paused"));
        assert!(seen.changed("Spotify.exe", "Paused"), "another session");
        assert!(!seen.changed("Spotify.exe", "Paused"));
    }
}
