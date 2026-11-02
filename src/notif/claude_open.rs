//! The decisions behind "click a Claude notice, go to its session", with no
//! Win32: which rule applies to where the hook ran, which Claude Desktop
//! sidebar row or terminal tab is the session, which window to bring forward,
//! and the VS Code link. `sources::claude_open` lists windows and acts. See
//! docs/memory/claude-notice-open.md.

use serde::Deserialize;

use super::open::{MainCand, pick_main_window};
use crate::claude::origin::{ClaudeOrigin, valid_host_session_id};

/// What a click on a Claude notice does, from the hook's origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaudeRule {
    /// Claude Desktop: its window, then the session's row in the sidebar.
    Desktop,
    /// The VS Code extension: its window, then the extension's own link
    /// (`<scheme>://anthropic.claude-code/open?session=<id>`).
    VsCodeUri { scheme: &'static str },
    /// A terminal or another host with tabs: its window, then the tab whose
    /// title is the session's.
    Tabs,
    /// A host window, nothing more to select.
    WindowOnly,
    /// Nothing is known of where the hook ran.
    NoHost,
}

pub fn choose_claude_target(o: &ClaudeOrigin) -> ClaudeRule {
    if o.host_exe == "claude.exe" || (o.host_hwnd == 0 && o.host_session_id.is_some()) {
        return ClaudeRule::Desktop;
    }
    if o.cli_direct {
        match o.host_exe.as_str() {
            "code.exe" => return ClaudeRule::VsCodeUri { scheme: "vscode" },
            "code - insiders.exe" => {
                return ClaudeRule::VsCodeUri {
                    scheme: "vscode-insiders",
                };
            }
            _ => {}
        }
    }
    if !o.host_exe.is_empty() && o.host_hwnd != 0 {
        return ClaudeRule::Tabs;
    }
    if o.host_exe.is_empty() && o.host_hwnd == 0 {
        return ClaudeRule::NoHost;
    }
    ClaudeRule::WindowOnly
}

// ------------------------------------------------------------ Desktop sidebar

/// Rows of the sidebar that are buttons of a session row but not the row.
const ROW_EXCLUDED_PREFIXES: [&str; 3] = [
    "More options for ",
    "Hide nested sessions ",
    "New session in ",
];

/// The sidebar button that is the session titled `title`: its name is the
/// title, or the title and a space and the subtitle. Exactly one candidate
/// decides; none or several (two sessions of one title) is None.
pub fn match_sidebar_row(names: &[String], title: &str) -> Option<usize> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let mut found = None;
    for (i, name) in names.iter().enumerate() {
        let name = name.trim();
        if ROW_EXCLUDED_PREFIXES.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let hit = name == title
            || name
                .strip_prefix(title)
                .is_some_and(|rest| rest.starts_with(' '));
        if hit {
            if found.is_some() {
                return None;
            }
            found = Some(i);
        }
    }
    found
}

/// The `title` of a Claude Desktop session file; nothing else of it is read.
pub fn parse_session_title(json: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Session {
        title: Option<String>,
    }
    let json = json.strip_prefix('\u{feff}').unwrap_or(json);
    let session: Session = serde_json::from_str(json).ok()?;
    session.title.filter(|t| !t.trim().is_empty())
}

/// A Desktop session id that may name a file: a plain identifier, no path
/// separators, no "..".
pub fn desktop_session_file_ok(id: &str) -> bool {
    valid_host_session_id(id) && !id.contains("..")
}

// ----------------------------------------------------------------------- tabs

/// A tab or console title without its status glyph: Claude Code leads the
/// title with one (✳ ✻ ✽ ✶ ✢ ·, a braille spinner while it works). Leading
/// characters that are whitespace, a symbol or punctuation go; the rest is
/// trimmed.
pub fn clean_tab_title(s: &str) -> String {
    s.trim_start_matches(|c: char| {
        // Braille patterns (the spinner) are symbols already; named for the reader.
        c.is_whitespace() || !c.is_alphanumeric() || (0x2800..=0x28ff).contains(&(c as u32))
    })
    .trim()
    .to_string()
}

/// True when `needle` is in `hay` with no letter or digit on either side.
/// Both are compared lowercase.
fn contains_whole_word(hay: &str, needle: &str) -> bool {
    let hay = hay.to_lowercase();
    let needle = needle.to_lowercase();
    if needle.trim().is_empty() {
        return false;
    }
    hay.match_indices(&needle).any(|(at, m)| {
        let before = hay[..at].chars().next_back();
        let after = hay[at + m.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// The tab of the session: the one whose cleaned title equals the console's
/// (case-insensitive); two such tabs are a tie and decide nothing. With none,
/// the one tab naming the project folder as a whole word; again exactly one.
pub fn pick_claude_tab(
    tabs: &[String],
    console_title: Option<&str>,
    project: &str,
) -> Option<usize> {
    let wanted = console_title.map(clean_tab_title).filter(|t| !t.is_empty());
    if let Some(wanted) = wanted {
        let wanted = wanted.to_lowercase();
        let same: Vec<usize> = (0..tabs.len())
            .filter(|&i| clean_tab_title(&tabs[i]).to_lowercase() == wanted)
            .collect();
        match same.as_slice() {
            [one] => return Some(*one),
            [] => {}
            _ => return None,
        }
    }
    let project = project.trim();
    if project.is_empty() {
        return None;
    }
    let named: Vec<usize> = (0..tabs.len())
        .filter(|&i| contains_whole_word(&tabs[i], project))
        .collect();
    match named.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

// ------------------------------------------------------------------- VS Code

/// The Claude Code extension's link to a session: `scheme` is `vscode` or
/// `vscode-insiders`. The id is percent-encoded except ASCII unreserved
/// characters.
pub fn vscode_uri(scheme: &str, session_id: &str) -> String {
    let mut out = format!("{scheme}://anthropic.claude-code/open?session=");
    for b in session_id.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// -------------------------------------------------------------------- window

/// A top-level window, as the platform side listed it.
#[derive(Clone, Debug, PartialEq)]
pub struct WinCand {
    pub hwnd: i64,
    pub pid: u32,
    /// Lowercase image file name.
    pub exe: String,
    pub cand: MainCand,
}

/// The window to bring forward, an index into `wins` (Z order, frontmost
/// first): the recorded window while it still exists and still belongs to the
/// recorded process; else the main window (`pick_main_window`) among the
/// windows of a running process with the recorded exe name, which may be a
/// hidden one (shown, never launched). None when neither exists.
pub fn resolve_window(hwnd: i64, pid: u32, exe: &str, wins: &[WinCand]) -> Option<usize> {
    if hwnd != 0
        && let Some(i) = wins
            .iter()
            .position(|w| w.hwnd == hwnd && w.pid == pid && pid != 0)
    {
        return Some(i);
    }
    if exe.is_empty() {
        return None;
    }
    let same: Vec<usize> = (0..wins.len()).filter(|&i| wins[i].exe == exe).collect();
    let cands: Vec<MainCand> = same.iter().map(|&i| wins[i].cand).collect();
    pick_main_window(&cands).map(|k| same[k])
}

#[cfg(test)]
mod tests {
    use super::super::open::WinState;
    use super::*;

    fn origin(exe: &str, hwnd: i64, direct: bool, session: Option<&str>) -> ClaudeOrigin {
        ClaudeOrigin {
            host_pid: if exe.is_empty() { 0 } else { 5 },
            host_exe: exe.into(),
            host_hwnd: hwnd,
            cli_direct: direct,
            console_title: None,
            host_session_id: session.map(String::from),
        }
    }

    #[test]
    fn rule_desktop() {
        let o = origin("claude.exe", 9, true, Some("local_x"));
        assert_eq!(choose_claude_target(&o), ClaudeRule::Desktop);
        // No window captured but Desktop's own session id is there.
        let o = origin("", 0, false, Some("local_x"));
        assert_eq!(choose_claude_target(&o), ClaudeRule::Desktop);
        // Desktop wins over the other rules.
        let o = origin("code.exe", 0, true, Some("local_x"));
        assert_eq!(choose_claude_target(&o), ClaudeRule::Desktop);
    }

    #[test]
    fn rule_vscode_needs_a_direct_cli() {
        assert_eq!(
            choose_claude_target(&origin("code.exe", 9, true, None)),
            ClaudeRule::VsCodeUri { scheme: "vscode" }
        );
        assert_eq!(
            choose_claude_target(&origin("code - insiders.exe", 9, true, None)),
            ClaudeRule::VsCodeUri {
                scheme: "vscode-insiders"
            }
        );
        // The integrated terminal: a terminal tab.
        assert_eq!(
            choose_claude_target(&origin("code.exe", 9, false, None)),
            ClaudeRule::Tabs
        );
        // Other forks get the tab rule.
        assert_eq!(
            choose_claude_target(&origin("cursor.exe", 9, true, None)),
            ClaudeRule::Tabs
        );
    }

    #[test]
    fn rule_tabs_window_only_and_no_host() {
        assert_eq!(
            choose_claude_target(&origin("windowsterminal.exe", 9, false, None)),
            ClaudeRule::Tabs
        );
        assert_eq!(
            choose_claude_target(&origin("orca.exe", 9, false, None)),
            ClaudeRule::Tabs
        );
        // An exe but no window recorded: find its window by name.
        assert_eq!(
            choose_claude_target(&origin("orca.exe", 0, false, None)),
            ClaudeRule::WindowOnly
        );
        assert_eq!(
            choose_claude_target(&ClaudeOrigin::default()),
            ClaudeRule::NoHost
        );
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn sidebar_row_exact_and_prefix_with_space() {
        let rows = names(&["Other", "Fix login", "Fix login 2h ago"]);
        // Two candidates (exact and prefix): a tie.
        assert_eq!(match_sidebar_row(&rows, "Fix login"), None);
        let rows = names(&["Other", "Fix login"]);
        assert_eq!(match_sidebar_row(&rows, "Fix login"), Some(1));
        let rows = names(&["Other", "Fix login 2h ago"]);
        assert_eq!(match_sidebar_row(&rows, "Fix login"), Some(1));
    }

    #[test]
    fn sidebar_row_needs_a_space_after_the_title() {
        let rows = names(&["Fix login page", "Fix loginX"]);
        assert_eq!(match_sidebar_row(&rows, "Fix login"), Some(0));
        let rows = names(&["Fix loginX"]);
        assert_eq!(match_sidebar_row(&rows, "Fix login"), None);
    }

    #[test]
    fn sidebar_row_excludes_the_helper_buttons() {
        let rows = names(&[
            "More options for Fix login",
            "Hide nested sessions Fix login",
            "New session in Fix login",
            "Fix login 1d",
        ]);
        assert_eq!(match_sidebar_row(&rows, "Fix login"), Some(3));
        let only_helpers = names(&["More options for Fix login", "New session in Fix login"]);
        assert_eq!(match_sidebar_row(&only_helpers, "Fix login"), None);
    }

    #[test]
    fn sidebar_row_duplicates_and_blank_titles_decide_nothing() {
        let rows = names(&["Fix login 1d", "Fix login 3d"]);
        assert_eq!(match_sidebar_row(&rows, "Fix login"), None);
        assert_eq!(match_sidebar_row(&rows, ""), None);
        assert_eq!(match_sidebar_row(&rows, "   "), None);
    }

    #[test]
    fn session_title_is_the_only_field_read() {
        let json = r#"{"cliSessionId":"abc","title":"Fix login","cwd":"D:\\secret","x":[1,2]}"#;
        assert_eq!(parse_session_title(json).as_deref(), Some("Fix login"));
        assert_eq!(parse_session_title(r#"{"title":"  "}"#), None);
        assert_eq!(parse_session_title(r#"{"title":5}"#), None);
        assert_eq!(parse_session_title("{"), None);
        assert_eq!(parse_session_title("{}"), None);
    }

    #[test]
    fn desktop_session_file_id_is_validated() {
        assert!(desktop_session_file_ok("local_0a1b-2c"));
        assert!(!desktop_session_file_ok("..\\x"));
        assert!(!desktop_session_file_ok("a/b"));
        assert!(!desktop_session_file_ok("a..b"));
        assert!(!desktop_session_file_ok(""));
    }

    #[test]
    fn tab_titles_lose_their_status_glyph() {
        assert_eq!(clean_tab_title("✳ Fix login"), "Fix login");
        assert_eq!(clean_tab_title("✻  Fix login  "), "Fix login");
        assert_eq!(clean_tab_title("· ✽ Fix login"), "Fix login");
        assert_eq!(clean_tab_title("⠋ Fix login"), "Fix login");
        assert_eq!(clean_tab_title("\u{2801}\u{28ff} Fix login"), "Fix login");
        assert_eq!(clean_tab_title("  ✢ Claude Code\t"), "Claude Code");
        assert_eq!(clean_tab_title("✳"), "");
        assert_eq!(clean_tab_title("Édition"), "Édition");
    }

    #[test]
    fn tab_by_console_title() {
        let tabs = names(&["PowerShell", "⠂ Fix login", "✳ Other"]);
        assert_eq!(pick_claude_tab(&tabs, Some("✳ fix LOGIN"), "api"), Some(1));
    }

    #[test]
    fn a_title_tie_decides_nothing_and_does_not_fall_through() {
        let tabs = names(&["✳ Claude Code", "⠂ Claude Code", "api server"]);
        assert_eq!(pick_claude_tab(&tabs, Some("Claude Code"), "api"), None);
    }

    #[test]
    fn no_title_match_falls_back_to_the_project_word() {
        let tabs = names(&["PowerShell", "~/work/api (main)", "notes"]);
        assert_eq!(
            pick_claude_tab(&tabs, Some("Something else"), "api"),
            Some(1)
        );
        assert_eq!(pick_claude_tab(&tabs, None, "api"), Some(1));
        assert_eq!(pick_claude_tab(&tabs, Some("   "), "api"), Some(1));
    }

    #[test]
    fn the_project_must_be_a_whole_word() {
        let tabs = names(&["rapid fire", "apiary", "api2"]);
        assert_eq!(pick_claude_tab(&tabs, None, "api"), None);
        let tabs = names(&["rapid fire", "the api tab"]);
        assert_eq!(pick_claude_tab(&tabs, None, "api"), Some(1));
    }

    #[test]
    fn two_project_tabs_decide_nothing() {
        let tabs = names(&["api one", "two - api"]);
        assert_eq!(pick_claude_tab(&tabs, None, "api"), None);
        assert_eq!(pick_claude_tab(&tabs, None, ""), None);
    }

    #[test]
    fn a_project_with_spaces_or_dashes_matches_as_a_phrase() {
        let tabs = names(&["claude Dynamic Island x", "Dynamic notes", "island"]);
        assert_eq!(pick_claude_tab(&tabs, None, "Dynamic Island"), Some(0));
        let tabs = names(&["~/dynamic-island (main)", "dynamic", "island"]);
        assert_eq!(pick_claude_tab(&tabs, None, "dynamic-island"), Some(0));
    }

    #[test]
    fn vscode_link_encoding() {
        assert_eq!(
            vscode_uri("vscode", "0a1b-2C_d.e~f"),
            "vscode://anthropic.claude-code/open?session=0a1b-2C_d.e~f"
        );
        assert_eq!(
            vscode_uri("vscode-insiders", "a b/c"),
            "vscode-insiders://anthropic.claude-code/open?session=a%20b%2Fc"
        );
        assert_eq!(
            vscode_uri("vscode", "é&=?"),
            "vscode://anthropic.claude-code/open?session=%C3%A9%26%3D%3F"
        );
    }

    fn cand(state: WinState) -> MainCand {
        MainCand {
            state,
            owned: false,
            tool_window: false,
            has_title: true,
            iconic: state == WinState::Minimized,
            helper: false,
            w: 800,
            h: 600,
        }
    }

    fn win(hwnd: i64, pid: u32, exe: &str, state: WinState) -> WinCand {
        WinCand {
            hwnd,
            pid,
            exe: exe.into(),
            cand: cand(state),
        }
    }

    #[test]
    fn the_recorded_window_wins_while_it_belongs_to_the_recorded_pid() {
        let wins = [
            win(1, 10, "windowsterminal.exe", WinState::Visible),
            win(2, 11, "windowsterminal.exe", WinState::Visible),
        ];
        assert_eq!(resolve_window(2, 11, "windowsterminal.exe", &wins), Some(1));
        // The handle was reused by another process: not the recorded window.
        assert_eq!(resolve_window(2, 99, "windowsterminal.exe", &wins), Some(0));
    }

    #[test]
    fn without_the_recorded_window_the_main_window_of_the_exe_is_used() {
        let wins = [
            win(1, 20, "chrome.exe", WinState::Visible),
            win(2, 11, "claude.exe", WinState::Hidden),
            win(3, 12, "claude.exe", WinState::Minimized),
        ];
        // A minimized window beats a hidden one.
        assert_eq!(resolve_window(77, 5, "claude.exe", &wins), Some(2));
        let only_hidden = [win(2, 11, "claude.exe", WinState::Hidden)];
        assert_eq!(resolve_window(0, 0, "claude.exe", &only_hidden), Some(0));
    }

    #[test]
    fn no_window_and_no_exe_is_none() {
        let wins = [win(1, 20, "chrome.exe", WinState::Visible)];
        assert_eq!(resolve_window(5, 6, "claude.exe", &wins), None);
        assert_eq!(resolve_window(0, 0, "", &wins), None);
        assert_eq!(resolve_window(1, 0, "", &wins), None);
    }
}
