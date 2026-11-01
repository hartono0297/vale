//! Where opening a caught toast goes: the pure rules (no Windows calls).
//!
//! `sources::notif_open` lists windows, reads a browser's tab strip through UI
//! Automation and brings the right window forward; every decision it takes
//! comes from here so it is tested without a desktop. See
//! docs/memory/notification-open.md.

use std::time::Duration;

use crate::model::OpenTarget;
use crate::sources::notifications::is_browser;

// ------------------------------------------------------------- tab titles

/// Decorations a browser appends to a tab's accessible name, lowercase.
/// "Memory usage" is special (it carries a size after it) and handled apart.
const SUFFIXES: [&str; 12] = [
    " - audio playing",
    " - camera or microphone recording",
    " - network error",
    " - pinned",
    " - google chrome",
    " - microsoft edge",
    " - brave",
    " - vivaldi",
    " - opera",
    " - mozilla firefox",
    " \u{2014} mozilla firefox",
    " - memory usage",
];

const MEMORY_USAGE: &str = " - memory usage - ";

/// `name` without the browser's trailing decorations ("Inbox - Memory usage -
/// 120 MB - Google Chrome" -> "Inbox"), however many are stacked. Zero-width
/// spaces (Edge writes "Microsoft\u{200b} Edge") are dropped first.
pub fn strip_tab_suffix(name: &str) -> String {
    let mut text: String = name.chars().filter(|c| *c != '\u{200b}').collect();
    loop {
        text.truncate(text.trim_end().len());
        // ASCII lowercase keeps every byte offset of the original.
        let lower = text.to_ascii_lowercase();
        if let Some(at) = lower.rfind(MEMORY_USAGE) {
            text.truncate(at);
            continue;
        }
        match SUFFIXES.iter().find(|s| lower.ends_with(*s)) {
            Some(s) => text.truncate(text.len() - s.len()),
            None => break,
        }
    }
    text
}

// ------------------------------------------------------------- site label

/// Lowercase letters and digits only: "Sedulur Store" -> "sedulurstore".
pub fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The second-level labels that sit in front of a country code ("co.id").
const SECOND_LEVEL: [&str; 14] = [
    "co", "com", "net", "org", "ac", "or", "go", "gov", "edu", "sch", "web", "biz", "mil", "ne",
];

/// The registrable label of a site, squashed: "web.telegram.org" ->
/// "telegram", "dashboard.sedulurstore.co.id" -> "sedulurstore", an IPv4 or a
/// label shorter than 3 characters -> None.
pub fn site_label(site: &str) -> Option<String> {
    let text = site.trim().to_ascii_lowercase();
    let text = text.split_once("://").map_or(text.as_str(), |(_, rest)| rest);
    let host = text.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next()?.trim_matches('.');
    let labels: Vec<&str> = host.split('.').collect();
    if labels.iter().any(|l| l.is_empty()) {
        return None;
    }
    if labels.len() == 4 && labels.iter().all(|l| l.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let at = match labels.as_slice() {
        [_] => 0,
        [.., second, last] if labels.len() >= 3 && last.len() == 2 && SECOND_LEVEL.contains(second) => {
            labels.len() - 3
        }
        _ => labels.len() - 2,
    };
    let label = squash(labels[at]);
    (label.chars().count() >= 3).then_some(label)
}

// ---------------------------------------------------------------- scoring

const SCORE_SITE: u32 = 1000;
const SCORE_TITLE: u32 = 500;
const SCORE_WORDS_HALF: u32 = 100;
const SCORE_WORDS_FEW: u32 = 10;
/// A notification title shorter than this is too common to prove a tab.
const MIN_TITLE: usize = 3;

/// How well a tab's name fits the toast: 0 = not at all. The site's label in
/// the tab name counts most, then the whole notification title, then the
/// title's words of 4+ letters (at least one; half or more counts more). The
/// body is not used: it is too long and too often generic.
pub fn score_tab(tab_title: &str, target: &OpenTarget) -> u32 {
    let tab = squash(&strip_tab_suffix(tab_title));
    if tab.is_empty() {
        return 0;
    }
    let mut score = 0;
    if let Some(label) = target.site.as_deref().and_then(site_label)
        && tab.contains(&label)
    {
        score += SCORE_SITE;
    }
    let title = squash(&target.title);
    if title.chars().count() >= MIN_TITLE && tab.contains(&title) {
        score += SCORE_TITLE;
    }
    if score == 0 {
        let mut words: Vec<String> = target
            .title
            .split(|c: char| !c.is_alphanumeric())
            .map(squash)
            .filter(|w| w.chars().count() >= 4)
            .collect();
        words.sort();
        words.dedup();
        let found = words.iter().filter(|w| tab.contains(w.as_str())).count() as u32;
        if found > 0 {
            score += if found as usize * 2 >= words.len() {
                SCORE_WORDS_HALF
            } else {
                SCORE_WORDS_FEW
            } + found;
        }
    }
    score
}

/// One browser window's tab strip, as UI Automation read it.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowTabs {
    pub tabs: Vec<String>,
    pub active: Option<usize>,
}

/// The kind of evidence a score stands for: 3 the site, 2 the whole title, 1
/// words of the title only, 0 none. Scores inside one tier are close calls;
/// a higher tier always wins.
fn tier(score: u32) -> u8 {
    match score {
        0 => 0,
        s if s >= SCORE_SITE => 3,
        s if s >= SCORE_TITLE => 2,
        _ => 1,
    }
}

/// The tab to select: `(window index, tab index)`. `windows` is in Z order
/// (most recently used first). The strongest kind of match anywhere decides
/// (a site match in an older window beats a words-only match in a newer one);
/// the most recent window holding a match of that kind wins, and inside it
/// the active tab if its match is of that kind too, else the first tab with
/// the best score.
pub fn pick_tab(windows: &[WindowTabs], target: &OpenTarget) -> Option<(usize, usize)> {
    let scores: Vec<Vec<u32>> = windows
        .iter()
        .map(|w| w.tabs.iter().map(|t| score_tab(t, target)).collect())
        .collect();
    let top = scores.iter().flatten().map(|s| tier(*s)).max().unwrap_or(0);
    if top == 0 {
        return None;
    }
    let w = scores
        .iter()
        .position(|s| s.iter().any(|s| tier(*s) == top))?;
    let window = &scores[w];
    if let Some(active) = windows[w].active
        && window.get(active).is_some_and(|s| tier(*s) == top)
    {
        return Some((w, active));
    }
    let best = window.iter().max()?;
    let first_best = window.iter().position(|s| s == best)?;
    Some((w, first_best))
}

/// The window to bring forward when no tab matched: the most recent one whose
/// tab strip was read with tabs in it (a PWA, DevTools or picture-in-picture
/// window has none), else the most recent one.
pub fn fallback_window(strips: &[WindowTabs]) -> usize {
    strips.iter().position(|s| !s.tabs.is_empty()).unwrap_or(0)
}

// ------------------------------------------------------------ app kinds

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenKind {
    Browser,
    Desktop,
}

/// A browser toast goes to a tab; anything else to the app's own window.
pub fn decide(target: &OpenTarget) -> OpenKind {
    if is_browser(target.app_id.as_deref(), &target.source) {
        OpenKind::Browser
    } else {
        OpenKind::Desktop
    }
}

/// The process names of the browser a toast came from (lowercase), by the
/// whole words of its AUMID or display name. Empty for a non-browser.
pub fn browser_exe_names(app_id: Option<&str>, source: &str) -> Vec<&'static str> {
    let text = format!("{} {}", app_id.unwrap_or(""), source).to_lowercase();
    let mut out: Vec<&'static str> = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        let names: &[&'static str] = match word {
            "chrome" => &["chrome.exe"],
            "msedge" | "edge" => &["msedge.exe"],
            "brave" => &["brave.exe"],
            "vivaldi" => &["vivaldi.exe"],
            // Opera's own `launcher.exe` exits once opera.exe runs, and the
            // name is too generic to stand for Opera in a process list.
            "opera" => &["opera.exe"],
            "firefox" => &["firefox.exe"],
            _ => &[],
        };
        for name in names {
            if !out.contains(name) {
                out.push(name);
            }
        }
    }
    out
}

/// The exe to start the browser with, from an `App Paths` value or a Start
/// entry's target: the whole path (a bare file name is not on the search path
/// `Command` uses, so it would never start), quotes trimmed, when its file
/// name is one of the browser's `names`, or it is Opera's `launcher.exe` in
/// an Opera folder ("...\\Programs\\Opera GX\\launcher.exe"). Whether the
/// file exists is the caller's check.
pub fn browser_launch_path(path: &str, names: &[&str]) -> Option<String> {
    let path = path.trim().trim_matches('"');
    let mut parts = path.rsplit(['\\', '/']);
    let file = parts.next()?.to_lowercase();
    let dir = parts.next().filter(|d| !d.is_empty())?;
    let ok = names.contains(&file.as_str())
        || (file == "launcher.exe"
            && names.contains(&"opera.exe")
            && squash(dir).starts_with("opera"));
    ok.then(|| path.to_string())
}

/// Stems that name no app: an updater or a shell around many ("Update.exe"
/// must never stand for Discord).
const GENERIC_STEMS: [&str; 14] = [
    "update", "updater", "app", "desktop", "launcher", "electron", "com", "squirrel", "org",
    "net", "microsoft", "windows", "setup", "main",
];

fn usable_stem(stem: &str) -> bool {
    stem.chars().count() >= 3 && !GENERIC_STEMS.contains(&stem)
}

/// Process-name candidates (squashed, no ".exe") for a desktop app: its
/// display name, then every meaningful dotted piece of its AUMID, last first
/// ("com.squirrel.Discord.Discord" -> discord; "Telegram.TelegramDesktop" ->
/// telegramdesktop, telegram). Generic stems are left out.
pub fn exe_stems_for(app_id: Option<&str>, source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: String| {
        if usable_stem(&s) && !out.contains(&s) {
            out.push(s);
        }
    };
    add(squash(source));
    if let Some(id) = app_id {
        let base = id.split('!').next().unwrap_or(id);
        for piece in base.rsplit('.') {
            add(squash(piece.split('_').next().unwrap_or(piece)));
        }
    }
    out
}

/// The squashed file stem of an exe path or name ("C:\\x\\Discord.exe" ->
/// "discord").
pub fn exe_stem(path: &str) -> String {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let stem = name
        .rsplit_once('.')
        .filter(|(_, ext)| ext.eq_ignore_ascii_case("exe"))
        .map_or(name, |(stem, _)| stem);
    squash(stem)
}

/// The exe file name (lowercase) of a Start entry's target, unless it is an
/// updater or launcher stub: "...\\Discord\\Update.exe" says nothing about
/// which process holds the window.
pub fn start_target_exe(path: &str) -> Option<String> {
    let name = path.rsplit(['\\', '/']).next()?.to_lowercase();
    (name.ends_with(".exe") && usable_stem(&exe_stem(&name))).then_some(name)
}

// ----------------------------------------------------------- main window

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WinState {
    Visible,
    Minimized,
    Hidden,
}

/// A window of the app being looked for, as the platform side saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MainCand {
    pub state: WinState,
    pub owned: bool,
    pub tool_window: bool,
    pub has_title: bool,
    pub w: i32,
    pub h: i32,
}

/// The app's main window: unowned, titled, not a tool window, and not empty
/// when shown. Visible beats minimized beats hidden; the earlier (frontmost)
/// wins a tie.
pub fn pick_main_window(cands: &[MainCand]) -> Option<usize> {
    cands
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            !c.owned
                && c.has_title
                && !c.tool_window
                && (c.state != WinState::Visible || (c.w > 0 && c.h > 0))
        })
        .min_by_key(|(i, c)| (c.state, *i))
        .map(|(i, _)| i)
}

/// What to do for a desktop app, from the windows found for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopAction {
    /// Restore if minimized, bring forward.
    Focus(usize),
    /// A hidden main window and an AUMID: launch by AUMID so the single
    /// instance shows itself, then show this window if nothing appeared.
    WakeByLaunch(usize),
    /// A hidden main window and no AUMID: show it ourselves.
    Show(usize),
    Launch,
    /// No window, no AUMID: no error, no launch.
    Nothing,
}

pub fn desktop_action(cands: &[MainCand], has_aumid: bool) -> DesktopAction {
    match pick_main_window(cands) {
        Some(i) if cands[i].state != WinState::Hidden => DesktopAction::Focus(i),
        Some(i) if has_aumid => DesktopAction::WakeByLaunch(i),
        Some(i) => DesktopAction::Show(i),
        None if has_aumid => DesktopAction::Launch,
        None => DesktopAction::Nothing,
    }
}

// ----------------------------------------------------------------- budget

/// The UI Automation timeout for a call started `elapsed` into `budget`: what
/// is left, in whole milliseconds, at least 1.
pub fn timeout_ms(budget: Duration, elapsed: Duration) -> u32 {
    budget
        .saturating_sub(elapsed)
        .as_millis()
        .clamp(1, u128::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(source: &str, app_id: Option<&str>, site: Option<&str>, title: &str) -> OpenTarget {
        OpenTarget {
            app_id: app_id.map(String::from),
            source: source.into(),
            site: site.map(String::from),
            title: title.into(),
            body: "hello".into(),
        }
    }

    fn tg(title: &str) -> OpenTarget {
        target("Google Chrome", Some("Chrome"), Some("web.telegram.org"), title)
    }

    fn win(tabs: &[&str], active: Option<usize>) -> WindowTabs {
        WindowTabs {
            tabs: tabs.iter().map(|t| t.to_string()).collect(),
            active,
        }
    }

    #[test]
    fn suffixes_come_off() {
        assert_eq!(strip_tab_suffix("Inbox - Google Chrome"), "Inbox");
        assert_eq!(
            strip_tab_suffix("Inbox - Memory usage - 120 MB - Google Chrome"),
            "Inbox"
        );
        assert_eq!(strip_tab_suffix("Lagu - Audio playing - Google Chrome"), "Lagu");
        assert_eq!(strip_tab_suffix("Home - Microsoft\u{200b} Edge"), "Home");
        assert_eq!(strip_tab_suffix("Home - Microsoft Edge"), "Home");
        assert_eq!(
            strip_tab_suffix("Rapat - Camera or microphone recording - Pinned - Brave"),
            "Rapat"
        );
        assert_eq!(strip_tab_suffix("Beranda \u{2014} Mozilla Firefox"), "Beranda");
        assert_eq!(strip_tab_suffix("Beranda - mozilla firefox"), "Beranda");
        assert_eq!(strip_tab_suffix("Oops - Network error"), "Oops");
        assert_eq!(strip_tab_suffix("Plain title"), "Plain title");
        assert_eq!(strip_tab_suffix("Edge cases - Edge"), "Edge cases - Edge");
        assert_eq!(strip_tab_suffix(""), "");
    }

    #[test]
    fn squash_keeps_letters_and_digits_lowercase() {
        assert_eq!(squash("Sedulur Store"), "sedulurstore");
        assert_eq!(squash("(3) Telegram Web!"), "3telegramweb");
        assert_eq!(squash(" - "), "");
    }

    #[test]
    fn site_label_takes_the_registrable_label() {
        assert_eq!(site_label("web.telegram.org").as_deref(), Some("telegram"));
        assert_eq!(site_label("web.whatsapp.com").as_deref(), Some("whatsapp"));
        assert_eq!(
            site_label("dashboard.sedulurstore.id").as_deref(),
            Some("sedulurstore")
        );
        assert_eq!(site_label("mail.google.com").as_deref(), Some("google"));
        assert_eq!(site_label("www.example.co.uk").as_deref(), Some("example"));
        assert_eq!(site_label("app.toko.co.id").as_deref(), Some("toko"));
        assert_eq!(site_label("kampus.ac.id").as_deref(), Some("kampus"));
        assert_eq!(site_label("x.go.id").as_deref(), None);
        assert_eq!(site_label("shop.example.com.au").as_deref(), Some("example"));
        assert_eq!(site_label("localhost:3000").as_deref(), Some("localhost"));
        assert_eq!(site_label("my-shop.com:8443").as_deref(), Some("myshop"));
        assert_eq!(site_label("https://m.facebook.com/x?y").as_deref(), Some("facebook"));
        assert_eq!(site_label("192.168.1.10"), None);
        assert_eq!(site_label("192.168.1.10:8080"), None);
        assert_eq!(site_label("x.com"), None);
        assert_eq!(site_label(""), None);
    }

    #[test]
    fn a_site_match_scores_highest() {
        let t = tg("Ibu");
        let site = score_tab("(3) Telegram Web - Google Chrome", &t);
        let title = score_tab("Chat dengan Ibu - Google Chrome", &t);
        assert!(site > title && title > 0, "{site} {title}");
        assert_eq!(score_tab("Berita hari ini", &t), 0);
        assert_eq!(score_tab("", &t), 0);
    }

    #[test]
    fn the_notification_title_matches_a_tab_without_a_site() {
        let t = target("Google Chrome", Some("Chrome"), None, "Sedulur Store");
        assert!(score_tab("Sedulur Store - Dashboard - Google Chrome", &t) >= SCORE_TITLE);
        assert_eq!(score_tab("Toko lain", &t), 0);
    }

    #[test]
    fn words_of_four_letters_count_and_half_counts_more() {
        let t = target("Google Chrome", Some("Chrome"), None, "Pesanan baru masuk");
        // "pesanan", "baru", "masuk" are 4+ letters; one of three is weak.
        let weak = score_tab("Daftar pesanan - Google Chrome", &t);
        let strong = score_tab("Pesanan baru - Google Chrome", &t);
        assert!(weak > 0 && strong > weak, "{weak} {strong}");
        // Short words alone never match.
        let short = target("Google Chrome", Some("Chrome"), None, "Ada si ibu");
        assert_eq!(score_tab("Ada si itu", &short), 0);
    }

    #[test]
    fn the_browser_suffix_never_makes_a_match() {
        let t = target("Google Chrome", Some("Chrome"), None, "Chrome");
        // The title is "Chrome" but the tab only says it as the suffix.
        assert_eq!(score_tab("Berita - Google Chrome", &t), 0);
    }

    #[test]
    fn the_most_recent_window_with_a_match_wins() {
        let t = tg("Ibu");
        let windows = [
            win(&["Berita", "Docs"], Some(0)),
            win(&["Telegram Web", "Lain"], Some(1)),
            win(&["Telegram Web"], Some(0)),
        ];
        assert_eq!(pick_tab(&windows, &t), Some((1, 0)));
    }

    #[test]
    fn inside_a_window_the_active_tab_wins_else_the_best_then_first() {
        let t = tg("Ibu");
        // Both match; the active one is taken even though it scores lower.
        let w = [win(&["Telegram Web (Ibu)", "Telegram Web"], Some(1))];
        assert_eq!(pick_tab(&w, &t), Some((0, 1)));
        // The active tab does not match: the best score.
        let w = [win(&["Chat Ibu", "Telegram Web", "Lain"], Some(2))];
        assert_eq!(pick_tab(&w, &t), Some((0, 1)));
        // Equal scores: the first.
        let w = [win(&["Lain", "Telegram Web A", "Telegram Web B"], None)];
        assert_eq!(pick_tab(&w, &t), Some((0, 1)));
        // Nothing matches.
        assert_eq!(pick_tab(&[win(&["Lain"], Some(0))], &t), None);
        assert_eq!(pick_tab(&[], &t), None);
        assert_eq!(pick_tab(&[win(&[], None)], &t), None);
    }

    #[test]
    fn a_weak_active_tab_never_beats_a_site_match() {
        // "Pagi" is one 4+ letter word of the title: a words-only match.
        let t = tg("Rapat besok pagi");
        let w = [win(&["Pagi ini - YouTube", "Telegram Web"], Some(0))];
        assert_eq!(pick_tab(&w, &t), Some((0, 1)));
        // A title match on the active tab does not beat a site match either.
        let w = [win(&["Rapat besok pagi - Docs", "Telegram Web"], Some(0))];
        assert_eq!(pick_tab(&w, &t), Some((0, 1)));
        // Same tier: the active tab still wins.
        let w = [win(&["Telegram Web", "Telegram Web (Rapat besok pagi)"], Some(0))];
        assert_eq!(pick_tab(&w, &t), Some((0, 0)));
        // A words-only active tab with nothing stronger anywhere stays chosen.
        let w = [win(&["Rapat - Docs", "Lain"], Some(0))];
        assert_eq!(pick_tab(&w, &t), Some((0, 0)));
    }

    #[test]
    fn a_site_match_in_an_older_window_beats_words_in_a_newer_one() {
        let t = tg("Rapat besok pagi");
        let windows = [
            win(&["Pagi ini - YouTube"], Some(0)),
            win(&["Berita", "Telegram Web"], Some(0)),
        ];
        assert_eq!(pick_tab(&windows, &t), Some((1, 1)));
        // A title match in an older window beats words in a newer one.
        let t = target("Google Chrome", Some("Chrome"), None, "Rapat besok pagi");
        let windows = [
            win(&["Pagi ini - YouTube"], Some(0)),
            win(&["Rapat besok pagi - Docs"], Some(0)),
        ];
        assert_eq!(pick_tab(&windows, &t), Some((1, 0)));
    }

    #[test]
    fn with_no_match_the_window_with_a_tab_strip_comes_forward() {
        // A PWA or DevTools window in front reads no tabs.
        let strips = [win(&[], None), win(&["Berita", "Docs"], Some(0))];
        assert_eq!(fallback_window(&strips), 1);
        assert_eq!(fallback_window(&[win(&["Berita"], Some(0)), win(&[], None)]), 0);
        // Nothing read (no UIA, budget spent): the most recent window.
        assert_eq!(fallback_window(&[win(&[], None)]), 0);
        assert_eq!(fallback_window(&[]), 0);
    }

    #[test]
    fn the_browser_launches_by_its_whole_path() {
        let chrome = ["chrome.exe"];
        let full = "C:\\Users\\a\\AppData\\Local\\Google\\Chrome\\Application\\chrome.exe";
        assert_eq!(browser_launch_path(full, &chrome).as_deref(), Some(full));
        assert_eq!(
            browser_launch_path(&format!("\"{full}\""), &chrome).as_deref(),
            Some(full)
        );
        // A bare name would not start: Command does not read App Paths.
        assert_eq!(browser_launch_path("chrome.exe", &chrome), None);
        // Another program's exe is never the browser.
        assert_eq!(browser_launch_path("C:\\x\\Update.exe", &chrome), None);
    }

    #[test]
    fn opera_launcher_only_counts_inside_an_opera_folder() {
        let opera = browser_exe_names(None, "Opera");
        let gx = "C:\\Users\\a\\AppData\\Local\\Programs\\Opera GX\\launcher.exe";
        assert_eq!(browser_launch_path(gx, &opera).as_deref(), Some(gx));
        let plain = "C:\\Users\\a\\AppData\\Local\\Programs\\Opera\\launcher.exe";
        assert!(browser_launch_path(plain, &opera).is_some());
        assert_eq!(browser_launch_path("C:\\Games\\Foo\\launcher.exe", &opera), None);
        // Not Opera: launcher.exe is never accepted.
        assert_eq!(browser_launch_path(plain, &["chrome.exe"]), None);
        // And no launcher.exe process is ever taken for Opera's window.
        assert!(!opera.contains(&"launcher.exe"));
    }

    #[test]
    fn browsers_go_to_tabs_and_everything_else_to_windows() {
        assert_eq!(decide(&tg("x")), OpenKind::Browser);
        assert_eq!(
            decide(&target("Microsoft Edge", Some("MSEdge"), None, "x")),
            OpenKind::Browser
        );
        assert_eq!(
            decide(&target("Discord", Some("com.squirrel.Discord.Discord"), None, "x")),
            OpenKind::Desktop
        );
        assert_eq!(decide(&target("Knowledge Base", None, None, "x")), OpenKind::Desktop);
    }

    #[test]
    fn browser_exe_names_follow_the_browser() {
        assert_eq!(browser_exe_names(Some("Chrome"), "Google Chrome"), ["chrome.exe"]);
        assert_eq!(browser_exe_names(Some("MSEdge"), "Microsoft Edge"), ["msedge.exe"]);
        assert_eq!(browser_exe_names(None, "Brave"), ["brave.exe"]);
        assert_eq!(browser_exe_names(None, "Vivaldi"), ["vivaldi.exe"]);
        assert_eq!(browser_exe_names(None, "Opera GX"), ["opera.exe"]);
        assert_eq!(browser_exe_names(Some("Firefox"), "Firefox"), ["firefox.exe"]);
        assert!(browser_exe_names(Some("Slack"), "Slack").is_empty());
    }

    #[test]
    fn exe_stems_come_from_the_name_and_the_aumid() {
        assert_eq!(
            exe_stems_for(Some("com.squirrel.Discord.Discord"), "Discord"),
            ["discord"]
        );
        assert_eq!(
            exe_stems_for(Some("Telegram.TelegramDesktop"), "Telegram Desktop"),
            ["telegramdesktop", "telegram"]
        );
        assert_eq!(exe_stems_for(None, "WhatsApp"), ["whatsapp"]);
        assert_eq!(
            exe_stems_for(Some("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"), "WhatsApp"),
            ["whatsapp", "whatsappdesktop", "5319275a"]
        );
    }

    #[test]
    fn generic_stems_are_ignored_so_update_exe_never_matches() {
        let stems = exe_stems_for(Some("com.squirrel.slack.Update"), "Update");
        assert_eq!(stems, ["slack"]);
        assert!(exe_stems_for(Some("Desktop.App"), "App").is_empty());
        assert_eq!(exe_stem("C:\\Users\\a\\Discord\\Update.exe"), "update");
        assert!(!stems.contains(&exe_stem("Update.exe")));
    }

    #[test]
    fn a_launcher_start_entry_is_not_a_process_to_match() {
        assert_eq!(start_target_exe("C:\\Users\\a\\AppData\\Local\\Discord\\Update.exe"), None);
        assert_eq!(start_target_exe("C:\\x\\launcher.exe"), None);
        assert_eq!(start_target_exe("C:\\x\\notes.txt"), None);
        assert_eq!(
            start_target_exe("C:\\Program Files\\Slack\\Slack.exe").as_deref(),
            Some("slack.exe")
        );
        assert_eq!(exe_stem("C:\\x\\Telegram.exe"), "telegram");
    }

    fn cand(state: WinState) -> MainCand {
        MainCand {
            state,
            owned: false,
            tool_window: false,
            has_title: true,
            w: 800,
            h: 600,
        }
    }

    #[test]
    fn the_main_window_is_visible_before_minimized_before_hidden() {
        let list = [
            cand(WinState::Hidden),
            cand(WinState::Minimized),
            cand(WinState::Visible),
            cand(WinState::Visible),
        ];
        assert_eq!(pick_main_window(&list), Some(2));
        assert_eq!(pick_main_window(&list[..2]), Some(1));
        assert_eq!(pick_main_window(&list[..1]), Some(0));
        assert_eq!(pick_main_window(&[]), None);
    }

    #[test]
    fn the_main_window_skips_owned_untitled_tool_and_empty_ones() {
        let owned = MainCand { owned: true, ..cand(WinState::Visible) };
        let untitled = MainCand { has_title: false, ..cand(WinState::Visible) };
        let tool = MainCand { tool_window: true, ..cand(WinState::Visible) };
        let empty = MainCand { w: 0, ..cand(WinState::Visible) };
        assert_eq!(pick_main_window(&[owned, untitled, tool, empty]), None);
        // A minimized window has a small icon-sized rect: size is not asked.
        let iconic = MainCand { w: 0, h: 0, ..cand(WinState::Minimized) };
        assert_eq!(pick_main_window(&[empty, iconic]), Some(1));
    }

    #[test]
    fn desktop_actions() {
        let v = cand(WinState::Visible);
        let m = cand(WinState::Minimized);
        let h = cand(WinState::Hidden);
        assert_eq!(desktop_action(&[v], true), DesktopAction::Focus(0));
        assert_eq!(desktop_action(&[m], false), DesktopAction::Focus(0));
        assert_eq!(desktop_action(&[h], true), DesktopAction::WakeByLaunch(0));
        assert_eq!(desktop_action(&[], true), DesktopAction::Launch);
    }

    #[test]
    fn without_an_aumid_nothing_is_ever_launched() {
        let h = cand(WinState::Hidden);
        assert_eq!(desktop_action(&[h], false), DesktopAction::Show(0));
        assert_eq!(desktop_action(&[], false), DesktopAction::Nothing);
        // The target of a toast with no AUMID is searched by display name.
        let t = target("Discord", None, None, "Halo");
        assert_eq!(decide(&t), OpenKind::Desktop);
        assert_eq!(exe_stems_for(t.app_id.as_deref(), &t.source), ["discord"]);
    }

    #[test]
    fn the_uia_timeout_is_what_is_left() {
        let b = Duration::from_millis(1000);
        assert_eq!(timeout_ms(b, Duration::ZERO), 1000);
        assert_eq!(timeout_ms(b, Duration::from_millis(400)), 600);
        assert_eq!(timeout_ms(b, Duration::from_secs(9)), 1);
    }
}
