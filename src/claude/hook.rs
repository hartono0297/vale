use std::fs::{File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::os::windows::fs::OpenOptionsExt;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::ERROR_PIPE_BUSY;
use windows::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows::Win32::System::Pipes::WaitNamedPipeW;
use windows::core::HSTRING;

use super::PIPE_NAME;
use super::origin::ClaudeOrigin;
use crate::i18n::{Key, tr};
use crate::model::{Event, Notice, NoticeKind};

/// How long to wait for a free pipe instance while the server is mid-handoff.
const BUSY_WAIT_MS: u32 = 50;
const OPEN_ATTEMPTS: usize = 3;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum HookKind {
    Done,
    Waiting,
    Activity,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct HookMessage {
    pub kind: HookKind,
    pub session_id: String,
    pub project: String,
    pub message: Option<String>,
    /// Where the hook ran (Done and Waiting only; `host::capture`). Absent in
    /// lines from an older hook binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ClaudeOrigin>,
}

/// The fields of Claude Code's hook stdin payload that the island uses.
#[derive(Deserialize)]
struct HookInput {
    hook_event_name: String,
    session_id: String,
    #[serde(default)]
    cwd: String,
    message: Option<String>,
    /// Absent in Claude Code versions that predate it.
    notification_type: Option<String>,
}

/// True for a `Notification` that waits on the user. Without a type (older
/// Claude Code) every notification is assumed to.
fn needs_user(notification_type: Option<&str>) -> bool {
    matches!(
        notification_type,
        None | Some("permission_prompt" | "idle_prompt" | "elicitation_dialog")
    )
}

pub fn parse_hook_input(json: &str) -> Option<HookMessage> {
    // Windows PowerShell 5.1 prefixes piped stdin with a UTF-8 BOM.
    let json = json.strip_prefix('\u{feff}').unwrap_or(json);
    let input: HookInput = serde_json::from_str(json).ok()?;
    let (kind, message) = match input.hook_event_name.as_str() {
        "Stop" => (HookKind::Done, None),
        "Notification" if needs_user(input.notification_type.as_deref()) => {
            (HookKind::Waiting, input.message)
        }
        // Ending the session clears its waiting notice like any activity.
        "UserPromptSubmit" | "PostToolUse" | "SessionEnd" => (HookKind::Activity, None),
        _ => return None,
    };
    let project = input
        .cwd
        .split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or_default()
        .to_string();
    Some(HookMessage {
        kind,
        session_id: input.session_id,
        project,
        message,
        origin: None,
    })
}

pub fn to_event(msg: HookMessage) -> Event {
    let HookMessage {
        kind,
        session_id,
        project,
        message,
        origin: _,
    } = msg;
    match kind {
        HookKind::Done => Event::Notice(Notice {
            key: format!("claude-done-{session_id}"),
            source: "Claude".to_string(),
            title: tr(Key::ClaudeDone).to_string(),
            body: project,
            kind: NoticeKind::ClaudeDone { session_id },
        }),
        HookKind::Waiting => Event::Notice(Notice {
            key: format!("claude-wait-{session_id}"),
            source: "Claude".to_string(),
            title: tr(Key::ClaudeWaiting).to_string(),
            body: message.unwrap_or(project),
            kind: NoticeKind::ClaudeWaiting { session_id },
        }),
        HookKind::Activity => Event::ClaudeActivity { session_id },
    }
}

/// Hands the message to a running island. False when no island is listening.
pub fn send_to_pipe(msg: &HookMessage) -> bool {
    let Ok(mut line) = serde_json::to_vec(msg) else {
        return false;
    };
    line.push(b'\n');
    send_line(PIPE_NAME, &line)
}

/// Writes `line` to the named pipe. A missing pipe fails at once; a busy one
/// (server between connections) gets a few short waits.
pub(super) fn send_line(pipe_name: &str, line: &[u8]) -> bool {
    for _ in 0..OPEN_ATTEMPTS {
        match open_pipe(pipe_name) {
            Ok(mut pipe) => return pipe.write_all(line).is_ok(),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                let _ = unsafe { WaitNamedPipeW(&HSTRING::from(pipe_name), BUSY_WAIT_MS) };
            }
            Err(_) => return false,
        }
    }
    false
}

/// `CreateFileW` with `OPEN_EXISTING`, and identification-only impersonation
/// so whoever owns the pipe cannot act as this user.
fn open_pipe(pipe_name: &str) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .security_qos_flags(SECURITY_IDENTIFICATION.0)
        .open(pipe_name)
}

/// `--hook` mode: relays Claude Code's stdin payload to the island. Prints
/// nothing and always exits 0, since hook output can reach Claude's context.
pub fn run_hook() {
    let stdin = io::stdin();
    // Claude Code always pipes the payload; a console stdin would block forever.
    if stdin.is_terminal() {
        return;
    }
    let mut input = Vec::new();
    if stdin.lock().read_to_end(&mut input).is_err() {
        return;
    }
    if let Some(mut msg) = parse_hook_input(&String::from_utf8_lossy(&input)) {
        // Only a notice can be clicked: activity events do no capture work.
        if msg.kind != HookKind::Activity {
            msg.origin = Some(super::host::capture(&msg.project));
        }
        send_to_pipe(&msg);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::model::{Notice, NoticeKind};

    #[test]
    fn ignores_byte_order_mark() {
        // Windows PowerShell 5.1 prefixes piped native stdin with a UTF-8 BOM.
        let m = parse_hook_input(
            "\u{feff}{\"hook_event_name\":\"Stop\",\"session_id\":\"s1\",\"cwd\":\"D:\\\\a\\\\Proj\"}\r\n",
        )
        .unwrap();
        assert_eq!(m.kind, HookKind::Done);
        assert_eq!(m.project, "Proj");
    }

    #[test]
    fn parses_stop() {
        let m = parse_hook_input(
            r#"{"hook_event_name":"Stop","session_id":"s1","cwd":"D:\\GitHub\\Dynamic Island"}"#,
        )
        .unwrap();
        assert_eq!(
            m,
            HookMessage {
                kind: HookKind::Done,
                session_id: "s1".into(),
                project: "Dynamic Island".into(),
                message: None,
                origin: None
            }
        );
    }

    #[test]
    fn parses_notification_with_message() {
        let m = parse_hook_input(
            r#"{"hook_event_name":"Notification","session_id":"s2","cwd":"C:\\work\\api","message":"Claude needs your permission to use Bash"}"#,
        )
        .unwrap();
        assert_eq!(
            m,
            HookMessage {
                kind: HookKind::Waiting,
                session_id: "s2".into(),
                project: "api".into(),
                message: Some("Claude needs your permission to use Bash".into()),
                origin: None
            }
        );
    }

    fn notification(notification_type: Option<&str>) -> Option<HookMessage> {
        let type_field = notification_type
            .map(|t| format!(r#","notification_type":"{t}""#))
            .unwrap_or_default();
        parse_hook_input(&format!(
            r#"{{"hook_event_name":"Notification","session_id":"s","cwd":"C:\\p","message":"m"{type_field}}}"#
        ))
    }

    #[test]
    fn notifications_that_need_the_user_are_waiting() {
        for t in ["permission_prompt", "idle_prompt", "elicitation_dialog"] {
            let m = notification(Some(t)).unwrap_or_else(|| panic!("{t} ignored"));
            assert_eq!(m.kind, HookKind::Waiting, "{t}");
            assert_eq!(m.message.as_deref(), Some("m"), "{t}");
        }
    }

    #[test]
    fn notification_without_type_is_waiting() {
        // Claude Code versions before `notification_type` existed.
        assert_eq!(notification(None).unwrap().kind, HookKind::Waiting);
    }

    #[test]
    fn other_notification_types_are_ignored() {
        for t in [
            "auth_success",
            "elicitation_complete",
            "elicitation_response",
            "plugin_notification",
            "something_new",
        ] {
            assert_eq!(notification(Some(t)), None, "{t}");
        }
    }

    #[test]
    fn activity_events() {
        for name in ["UserPromptSubmit", "PostToolUse", "SessionEnd"] {
            let json = format!(r#"{{"hook_event_name":"{name}","session_id":"s3","cwd":"C:\\p"}}"#);
            let m = parse_hook_input(&json).unwrap();
            assert_eq!(m.kind, HookKind::Activity, "{name}");
            assert_eq!(m.session_id, "s3");
        }
    }

    #[test]
    fn unknown_event_or_bad_json_is_none() {
        assert_eq!(
            parse_hook_input(r#"{"hook_event_name":"PreCompact","session_id":"s","cwd":"C:\\p"}"#),
            None
        );
        assert_eq!(parse_hook_input("{"), None);
    }

    #[test]
    fn forward_slash_cwd() {
        let m =
            parse_hook_input(r#"{"hook_event_name":"Stop","session_id":"s","cwd":"/home/x/proj"}"#)
                .unwrap();
        assert_eq!(m.project, "proj");
    }

    #[test]
    fn done_becomes_notice() {
        let event = to_event(HookMessage {
            kind: HookKind::Done,
            session_id: "s1".into(),
            project: "Dynamic Island".into(),
            message: None,
            origin: None,
        });
        assert_eq!(
            event,
            Event::Notice(Notice {
                key: "claude-done-s1".into(),
                source: "Claude".into(),
                title: "Claude selesai".into(),
                body: "Dynamic Island".into(),
                kind: NoticeKind::ClaudeDone {
                    session_id: "s1".into()
                },
            })
        );
    }

    #[test]
    fn waiting_becomes_notice_with_message_or_project() {
        let with_message = to_event(HookMessage {
            kind: HookKind::Waiting,
            session_id: "s2".into(),
            project: "api".into(),
            message: Some("Claude needs your permission to use Bash".into()),
            origin: None,
        });
        assert_eq!(
            with_message,
            Event::Notice(Notice {
                key: "claude-wait-s2".into(),
                source: "Claude".into(),
                title: "Claude butuh respon".into(),
                body: "Claude needs your permission to use Bash".into(),
                kind: NoticeKind::ClaudeWaiting {
                    session_id: "s2".into()
                },
            })
        );
        let Event::Notice(without_message) = to_event(HookMessage {
            kind: HookKind::Waiting,
            session_id: "s2".into(),
            project: "api".into(),
            message: None,
            origin: None,
        }) else {
            panic!("expected a notice");
        };
        assert_eq!(without_message.body, "api");
    }

    #[test]
    fn activity_becomes_claude_activity() {
        let event = to_event(HookMessage {
            kind: HookKind::Activity,
            session_id: "s3".into(),
            project: "p".into(),
            message: None,
            origin: None,
        });
        assert_eq!(
            event,
            Event::ClaudeActivity {
                session_id: "s3".into()
            }
        );
    }

    fn sample_origin() -> ClaudeOrigin {
        ClaudeOrigin {
            host_pid: 4242,
            host_exe: "windowsterminal.exe".into(),
            host_hwnd: 0x1234,
            cli_direct: false,
            console_title: Some("✳ Fix the bug".into()),
            host_session_id: Some("local_abc-1".into()),
        }
    }

    #[test]
    fn an_older_hook_line_without_origin_parses() {
        let m: HookMessage = serde_json::from_str(
            r#"{"kind":"Waiting","session_id":"s","project":"p","message":"m"}"#,
        )
        .unwrap();
        assert_eq!(m.origin, None);
        assert_eq!(m.kind, HookKind::Waiting);
    }

    #[test]
    fn origin_round_trips_and_is_left_out_when_absent() {
        let msg = HookMessage {
            kind: HookKind::Done,
            session_id: "s".into(),
            project: "p".into(),
            message: None,
            origin: Some(sample_origin()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(serde_json::from_str::<HookMessage>(&json).unwrap(), msg);
        let bare = HookMessage {
            origin: None,
            ..msg
        };
        assert!(!serde_json::to_string(&bare).unwrap().contains("origin"));
    }

    #[test]
    fn parsing_the_hook_input_captures_nothing() {
        let m = parse_hook_input(r#"{"hook_event_name":"Stop","session_id":"s","cwd":"C:\\p"}"#)
            .unwrap();
        assert_eq!(m.origin, None);
    }

    #[test]
    fn a_message_with_max_length_fields_stays_under_the_pipe_limit() {
        // The title is cut to 512 chars; the worst case JSON-escapes each as \u00XX.
        let origin = ClaudeOrigin {
            host_exe: "x".repeat(260),
            console_title: Some("\u{1}".repeat(crate::claude::origin::MAX_TITLE_CHARS)),
            host_session_id: Some("a".repeat(100)),
            ..sample_origin()
        };
        let msg = HookMessage {
            kind: HookKind::Waiting,
            session_id: "s".repeat(200),
            project: "p".repeat(255),
            message: Some("m".repeat(4000)),
            origin: Some(origin),
        };
        let line = serde_json::to_vec(&msg).unwrap();
        assert!(line.len() < 64 * 1024, "{} bytes", line.len());
    }

    #[test]
    fn send_to_missing_pipe_is_silent_and_fast() {
        // A name no test server listens on; the real pipe may be live on this machine.
        let name = format!(r"\\.\pipe\vale-missing-{}", std::process::id());
        let msg = HookMessage {
            kind: HookKind::Done,
            session_id: "s".into(),
            project: "p".into(),
            message: None,
            origin: None,
        };
        let line = serde_json::to_vec(&msg).unwrap();
        let t = Instant::now();
        assert!(!send_line(&name, &line));
        assert!(t.elapsed() < Duration::from_millis(200));
    }
}
