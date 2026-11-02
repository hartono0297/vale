//! Where a Claude Code hook ran: the host window and the process chain facts
//! `--hook` captures for a Done or Waiting notice, so a click on the notice
//! can bring the user back to that session. Data and a small process-global
//! registry only; capturing is `host`, acting is `sources::claude_open`, the
//! rules are `notif::claude_open`. See docs/memory/claude-notice-open.md.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

/// Longest console title kept (characters).
pub const MAX_TITLE_CHARS: usize = 512;
/// Most sessions the registry remembers.
const REGISTRY_CAP: usize = 64;

/// What the hook process found out about where it runs. Every field has a
/// default, so a line from an older or newer binary still parses.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ClaudeOrigin {
    /// The host process (0: none found).
    #[serde(default)]
    pub host_pid: u32,
    /// Its image file name, lowercase ("windowsterminal.exe").
    #[serde(default)]
    pub host_exe: String,
    /// Its top-level window (0: none).
    #[serde(default)]
    pub host_hwnd: i64,
    /// No shell between the claude CLI and the host process tree (the VS Code
    /// extension host, Claude Desktop), as opposed to a terminal tab.
    #[serde(default)]
    pub cli_direct: bool,
    /// The console title of the CLI's console, at most `MAX_TITLE_CHARS`.
    #[serde(default)]
    pub console_title: Option<String>,
    /// `CLAUDE_CODE_HOST_SESSION_ID` (Claude Desktop's own session id), only
    /// when it is a plain identifier.
    #[serde(default)]
    pub host_session_id: Option<String>,
}

/// True for a plain identifier: `^[A-Za-z0-9_-]{1,100}$`.
pub fn valid_host_session_id(id: &str) -> bool {
    (1..=100).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `id` when it is a plain identifier, else None.
pub fn sanitize_host_session_id(id: Option<String>) -> Option<String> {
    id.filter(|s| valid_host_session_id(s))
}

/// `title` cut to `MAX_TITLE_CHARS` characters; empty (or blank) -> None.
pub fn clamp_title(title: &str) -> Option<String> {
    let cut: String = title.chars().take(MAX_TITLE_CHARS).collect();
    (!cut.trim().is_empty()).then_some(cut)
}

/// What the registry keeps per session: the origin and the project (the cwd's
/// folder name) the notice was raised for, which the tab rule matches on.
#[derive(Clone, Debug, PartialEq)]
pub struct Remembered {
    pub origin: ClaudeOrigin,
    pub project: String,
}

type Entries = VecDeque<(String, Remembered)>;

static REGISTRY: Mutex<Entries> = Mutex::new(VecDeque::new());

fn registry() -> MutexGuard<'static, Entries> {
    REGISTRY.lock().unwrap_or_else(|p| p.into_inner())
}

/// The newest entry replaces the same session's; the oldest session falls out
/// past `cap`.
fn push_capped(entries: &mut Entries, cap: usize, session_id: &str, value: Remembered) {
    entries.retain(|(id, _)| id != session_id);
    entries.push_back((session_id.to_string(), value));
    while entries.len() > cap {
        entries.pop_front();
    }
}

/// Remembers where `session_id`'s newest notice came from.
pub fn remember(session_id: &str, origin: ClaudeOrigin, project: &str) {
    let value = Remembered {
        origin,
        project: project.to_string(),
    };
    push_capped(&mut registry(), REGISTRY_CAP, session_id, value);
}

/// Drops what is known of `session_id` (an older hook binary sent no origin).
pub fn forget(session_id: &str) {
    registry().retain(|(id, _)| id != session_id);
}

pub fn get(session_id: &str) -> Option<Remembered> {
    registry()
        .iter()
        .find(|(id, _)| id == session_id)
        .map(|(_, r)| r.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(pid: u32) -> ClaudeOrigin {
        ClaudeOrigin {
            host_pid: pid,
            host_exe: "windowsterminal.exe".into(),
            ..Default::default()
        }
    }

    fn value(pid: u32) -> Remembered {
        Remembered {
            origin: origin(pid),
            project: "p".into(),
        }
    }

    #[test]
    fn missing_fields_default() {
        let o: ClaudeOrigin = serde_json::from_str("{}").unwrap();
        assert_eq!(o, ClaudeOrigin::default());
        let o: ClaudeOrigin = serde_json::from_str(r#"{"host_pid":7,"future":1}"#).unwrap();
        assert_eq!(o.host_pid, 7);
    }

    #[test]
    fn session_id_validation() {
        assert!(valid_host_session_id("local_0a1b-2C"));
        assert!(!valid_host_session_id(""));
        assert!(!valid_host_session_id("..\\x"));
        assert!(!valid_host_session_id("a/b"));
        assert!(!valid_host_session_id("a b"));
        assert!(!valid_host_session_id(&"a".repeat(101)));
        assert!(valid_host_session_id(&"a".repeat(100)));
        assert_eq!(sanitize_host_session_id(Some("a/b".into())), None);
        assert_eq!(
            sanitize_host_session_id(Some("ok".into())),
            Some("ok".into())
        );
    }

    #[test]
    fn title_is_clamped_and_blank_is_none() {
        assert_eq!(clamp_title("   "), None);
        assert_eq!(clamp_title(""), None);
        assert_eq!(clamp_title(&"é".repeat(600)).unwrap().chars().count(), 512);
    }

    #[test]
    fn registry_keeps_the_newest_per_session_and_caps() {
        let mut e = Entries::new();
        push_capped(&mut e, 3, "a", value(1));
        push_capped(&mut e, 3, "b", value(2));
        push_capped(&mut e, 3, "a", value(3));
        assert_eq!(e.len(), 2);
        assert_eq!(e.back().unwrap().1.origin.host_pid, 3);
        push_capped(&mut e, 3, "c", value(4));
        push_capped(&mut e, 3, "d", value(5));
        assert_eq!(e.len(), 3);
        assert!(e.iter().all(|(id, _)| id != "b"), "the oldest fell out");
    }

    #[test]
    fn remember_get_forget() {
        remember("origin-test-a", origin(1), "proj");
        remember("origin-test-a", origin(2), "proj2");
        let got = get("origin-test-a").unwrap();
        assert_eq!(got.origin.host_pid, 2);
        assert_eq!(got.project, "proj2");
        forget("origin-test-a");
        assert_eq!(get("origin-test-a"), None);
    }
}
