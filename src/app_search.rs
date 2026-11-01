//! The island's side of the quick search: its hotkey, the Start Menu index
//! on a background thread, the field's keys, the results and running one.
//! A child of `app`, so it works on `App` directly. What a line finds and
//! how results rank is `search.rs`; the reducer only knows the search is
//! open and which row was clicked.
//!
//! Power: nothing runs while the search is closed. The Start Menu is read
//! once, on the first time it opens, on its own thread.

use std::sync::mpsc::Sender;
use std::time::Instant;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, GW_HWNDNEXT, GWL_EXSTYLE, GetClassNameW,
    GetForegroundWindow, GetTopWindow, GetWindow, GetWindowLongW, IsIconic, IsWindowVisible,
    SetForegroundWindow, WINDOW_EX_STYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

use super::{App, WM_APP_EVENT, poster};
use crate::clipboard::win;
use crate::i18n::{self, Key, tr};
use crate::model::{Event, Hit, Notice, NoticeKind, Page, View};
use crate::search::{self, Action, AppEntry, Labels, SearchResult};
use crate::search_details::{self, Detail, Details};
use crate::search_files::{self, FileEntry, Limits};
use crate::shelf::{self, Probe};
use crate::shortcuts;
use crate::ui::cli_input::{CliInput, Key as InputKey};
use crate::ui::editor;

/// Characters the field takes.
pub(super) const QUERY_LIMIT: usize = search::QUERY_MAX;

/// The search's state that is not the reducer's.
#[derive(Default)]
pub(super) struct SearchState {
    /// The Start Menu's apps; `None` until the index thread answered.
    pub apps: Option<Vec<AppEntry>>,
    /// The index thread was started.
    pub indexing: bool,
    /// The files and folders last walked, newest walk; empty until the
    /// first one is done. In memory only.
    pub files: Vec<FileEntry>,
    /// A walk is running, and when the last one started.
    pub walking: bool,
    pub walk_started: Option<Instant>,
    /// At least one walk has finished (the "reading files" hint ends).
    pub walked: bool,
    /// Icons, folder counts and app sizes of the rows shown, and the
    /// worker that reads them (started the first time the search opens).
    pub details: Details,
    pub details_tx: Option<Sender<search_details::Request>>,
    /// The field's text as last searched.
    pub query: String,
    pub results: Vec<SearchResult>,
    /// The highlighted row.
    pub selected: usize,
}

/// `CF_UNICODETEXT`'s bytes for `text`.
fn text_formats(text: &str) -> Vec<(u32, Vec<u8>)> {
    vec![(13, win::utf16z(text))]
}

/// The word under a result row's kind, in the current language.
fn with_labels<T>(f: impl FnOnce(&Labels) -> T) -> T {
    let lang = i18n::lang();
    let web_title = move |q: &str| i18n::search_web_title(lang, q);
    let size = move |bytes: u64| i18n::search_file_size(lang, bytes);
    f(&Labels {
        app: tr(Key::SearchKindApp),
        setting: tr(Key::SearchKindSetting),
        copy: tr(Key::SearchCopy),
        web: tr(Key::SearchKindWeb),
        web_title: &web_title,
        root: &i18n::search_root,
        size: &size,
    })
}

/// Where the highlight goes when the list is made again without a new
/// query (the app index or the file index arrived): the row of the same
/// result as before (compared by what Enter does), else the first.
pub(super) fn keep_selection(old: &[SearchResult], at: usize, new: &[SearchResult]) -> usize {
    old.get(at)
        .and_then(|was| new.iter().position(|r| r.action == was.action))
        .unwrap_or(0)
}

/// Whether a file walk should start now: none is running, and the last
/// one started `FILE_FRESH` ago or never.
pub(super) fn walk_due(last: Option<Instant>, running: bool, now: Instant) -> bool {
    !running && last.is_none_or(|at| now.saturating_duration_since(at) > search_files::FILE_FRESH)
}

/// The highlighted row after an Up or a Down among `n` rows (it stops at
/// both ends).
pub(super) fn move_selection(at: usize, n: usize, up: bool) -> usize {
    if n == 0 {
        0
    } else if up {
        at.saturating_sub(1)
    } else {
        (at + 1).min(n - 1)
    }
}

impl App {
    /// `WM_HOTKEY`: the search opens (or closes). The Start Menu is read the
    /// first time.
    pub(super) fn on_search_hotkey(&mut self) {
        self.start_search_index();
        self.apply_now(Event::SearchHotkey);
    }

    /// The tray's "Cari…": the search opens, or its field takes the
    /// keyboard again when it is open already. `before` had the foreground
    /// when the menu opened; it is where Esc hands the keyboard back,
    /// unless that was the taskbar (a click on the tray icon activates it)
    /// or the island itself.
    pub(super) fn on_search_menu(&mut self, before: HWND) {
        if let Some(prev) = window_to_return_to(before, self.hwnd) {
            self.cli_prev = Some(prev);
        }
        self.start_search_index();
        self.apply_now(Event::SearchOpen);
    }

    /// Reads the Start Menu on its own thread, once.
    fn start_search_index(&mut self) {
        if self.search.apps.is_some() || self.search.indexing {
            return;
        }
        self.search.indexing = true;
        let tx = self.tx.clone();
        let wake = poster(self.hwnd, WM_APP_EVENT);
        let spawned = std::thread::Builder::new()
            .name("vale-search-index".into())
            .spawn(move || {
                let apps = search::index_start_menu();
                if tx.send(Event::SearchIndex(apps)).is_ok() {
                    wake();
                }
            });
        if spawned.is_err() {
            self.search.indexing = false;
        }
    }

    /// Walks the user's folders for file and folder names, on a short-lived
    /// thread, when the search opens and the last walk is older than
    /// `FILE_FRESH` (the old list answers meanwhile). Nothing runs while the
    /// search is closed.
    fn start_search_files(&mut self) {
        let now = Instant::now();
        if !walk_due(self.search.walk_started, self.search.walking, now) {
            return;
        }
        self.search.walking = true;
        self.search.walk_started = Some(now);
        let tx = self.tx.clone();
        let wake = poster(self.hwnd, WM_APP_EVENT);
        let spawned = std::thread::Builder::new()
            .name("vale-search-files".into())
            .spawn(move || {
                // Reading thousands of directories is not urgent: stay out of
                // the way of whatever the user is doing.
                unsafe {
                    let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
                }
                let files = search_files::walk(&search_files::default_roots(), Limits::DEFAULT);
                if tx.send(Event::SearchFiles(files)).is_ok() {
                    wake();
                }
            });
        if spawned.is_err() {
            self.search.walking = false;
        }
    }

    /// Starts the details worker the first time the search opens; it then
    /// sleeps on its channel until a request comes.
    fn start_search_details(&mut self) {
        if self.search.details_tx.is_none() {
            let wake = poster(self.hwnd, WM_APP_EVENT);
            self.search.details_tx = search_details::spawn(self.tx.clone(), wake);
        }
    }

    /// Asks the worker for the icon, count or size of the rows shown that
    /// have none yet. The rows are already on screen; answers redraw them.
    fn request_search_details(&mut self) {
        let Some(tx) = &self.search.details_tx else {
            return;
        };
        if let Some(request) = self.search.details.request(&self.search.results)
            && tx.send(request).is_err()
        {
            // The worker is gone: rows keep their glyphs.
            self.search.details_tx = None;
        }
    }

    /// An answer from the details worker: kept (and drawn) unless it is for
    /// an older request. Never changes the order of the rows.
    pub(super) fn on_search_detail(
        &mut self,
        generation: u64,
        key: std::path::PathBuf,
        detail: Detail,
    ) {
        if self.search.details.accept(generation, key, detail) {
            self.redraw_search();
        }
    }

    /// The index arrived: what is typed is searched again, the highlight
    /// staying on the same result.
    pub(super) fn on_search_index(&mut self, apps: Vec<AppEntry>) {
        self.search.apps = Some(apps);
        self.search.indexing = false;
        self.search_again(true);
        self.redraw_search();
    }

    /// A walk finished: the new names replace the old ones, and what is typed
    /// is searched again.
    pub(super) fn on_search_files(&mut self, files: Vec<FileEntry>) {
        self.search.files = files;
        self.search.walking = false;
        self.search.walked = true;
        self.search_again(true);
        self.redraw_search();
    }

    /// The reducer opened the search: a clean field takes the keyboard (the
    /// island activates). When it cannot have it, the search closes again.
    pub(super) fn focus_search(&mut self) -> Option<Event> {
        // The search takes the field from Susun's name.
        self.end_naming();
        // Every way in (hotkey, tray, tile) reads the Start Menu once and
        // looks at the user's folders again when the last look is old.
        self.start_search_index();
        self.start_search_files();
        self.start_search_details();
        if editor::focus_open_editor() {
            return Some(Event::SearchClose);
        }
        // The field could not be made when the island started: no search.
        let Some(input) = self.search_input.as_ref() else {
            return Some(Event::SearchClose);
        };
        input.set_text("");
        self.search.query.clear();
        self.search.results.clear();
        self.search.selected = 0;
        unsafe {
            let fg = GetForegroundWindow();
            if fg != self.hwnd {
                self.cli_prev = (!fg.is_invalid()).then_some(fg);
                let _ = SetForegroundWindow(self.hwnd);
            }
        }
        let ok = input.focus() && unsafe { GetForegroundWindow() } == self.hwnd;
        if ok {
            None
        } else {
            self.return_keyboard();
            Some(Event::SearchClose)
        }
    }

    /// The search closed: the keyboard goes back and the field is cleared.
    pub(super) fn release_search(&mut self) {
        self.return_keyboard();
        if let Some(input) = &self.search_input
            && !input.text().is_empty()
        {
            input.set_text("");
        }
        self.search.query.clear();
        self.search.results.clear();
        self.search.selected = 0;
    }

    /// The field has the keyboard for real.
    fn search_has_keyboard(&self) -> bool {
        self.search_input.as_ref().is_some_and(CliInput::has_focus)
            && unsafe { GetForegroundWindow() } == self.hwnd
    }

    /// A key the field hands over.
    pub(super) fn on_search_key(&mut self, key: InputKey) {
        // The same hidden field takes Susun's name while that is up.
        if self.arrange_naming() {
            return self.on_arrange_name_key(key);
        }
        match key {
            // Enter is a click on the highlighted row: the reducer closes the
            // search and asks for it to be run.
            InputKey::Enter => {
                let at = self.search.selected;
                if self.search.results.get(at).is_some() {
                    self.apply_now(Event::Click(Hit::SearchResult(at)));
                }
            }
            InputKey::Up | InputKey::Down => {
                let to = move_selection(
                    self.search.selected,
                    self.search.results.len(),
                    key == InputKey::Up,
                );
                if to != self.search.selected {
                    self.search.selected = to;
                    self.redraw_search();
                }
            }
            InputKey::Escape => self.apply_now(Event::SearchClose),
            // Another window took the keyboard (a click outside, Alt+Tab):
            // no foreground to hand back.
            InputKey::Blur => {
                if self.state.search_open() && !self.search_has_keyboard() {
                    self.cli_prev = None;
                    self.apply_now(Event::SearchClose);
                }
            }
            InputKey::Tab
            | InputKey::Stop
            | InputKey::PageUp
            | InputKey::PageDown
            | InputKey::Delete => {}
        }
    }

    /// The field's text or selection changed.
    pub(super) fn on_search_changed(&mut self) {
        if self.arrange_naming() {
            return self.on_arrange_name_changed();
        }
        let typed = self
            .search_input
            .as_ref()
            .map(CliInput::text)
            .unwrap_or_default();
        if typed != self.search.query {
            self.search.query = typed;
            self.search_again(false);
        }
        self.redraw_search();
    }

    /// Searches again for what is typed. A new query puts the highlight on
    /// the first row; an index that arrived (`keep`) leaves it on the same
    /// result.
    fn search_again(&mut self, keep: bool) {
        let lang = i18n::lang();
        let apps = self.search.apps.as_deref().unwrap_or(&[]);
        let files = &self.search.files;
        let results =
            with_labels(|labels| search::search(&self.search.query, apps, files, lang, labels));
        self.search.selected = if keep {
            keep_selection(&self.search.results, self.search.selected, &results)
        } else {
            0
        };
        self.search.results = results;
        self.request_search_details();
    }

    fn redraw_search(&mut self) {
        if self.state.view() == View::Panel(Page::Search) && !self.animating() {
            self.draw(Instant::now());
        }
    }

    /// Runs result `i` (the search is closing already). `Some(event)` is
    /// what to show after (the "Disalin" notice of a copied result).
    ///
    /// An app, a settings page or the web opens while the island still has
    /// the foreground, so Windows lets the new window come to the front:
    /// the keyboard is not handed back to the window from before (the
    /// release that follows finds nowhere to go and only clears the field's
    /// focus), and the shell runs on the shortcut timer, outside the app
    /// borrow, a moment later with the island still in front. Every process
    /// may also take the foreground once (`ASFW_ANY`), for a browser or the
    /// Settings app that is already running and is asked through a broker.
    /// Clicking elsewhere meanwhile simply wins.
    pub(super) fn run_search(&mut self, i: usize) -> Option<Event> {
        let result = self.search.results.get(i)?.clone();
        let target = match result.action {
            Action::Launch(path) => path,
            Action::OpenSetting(uri) => uri.to_owned(),
            Action::OpenWeb(url) if search::is_web_url(&url) => url,
            Action::OpenWeb(_) => return None,
            Action::Copy(text) => return self.copy_search_result(text),
            Action::OpenPath(path) => {
                // Gone since the walk (or a drive that went): nothing to open.
                if shelf::probe(&path) == Probe::Missing {
                    return None;
                }
                self.cli_prev = None;
                unsafe {
                    if GetForegroundWindow() == self.hwnd {
                        let _ = AllowSetForegroundWindow(ASFW_ANY);
                    }
                }
                self.queue_shortcut(shortcuts::Action::OpenPath(path));
                return None;
            }
        };
        self.cli_prev = None;
        unsafe {
            if GetForegroundWindow() == self.hwnd {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
            }
        }
        self.queue_shortcut(shortcuts::Action::Shell(target));
        None
    }

    /// A calculation's result goes to the clipboard and "Disalin" shows; the
    /// keyboard goes back to the window from before.
    fn copy_search_result(&mut self, text: String) -> Option<Event> {
        let sequence = win::write(self.hwnd, &text_formats(&text))?;
        // Written here: not a copy for the history to take again.
        self.clip.own_sequence = Some(sequence);
        Some(Event::Notice(Notice {
            key: "search-copied".into(),
            source: tr(Key::SearchTitle).into(),
            title: tr(Key::ClipCopied).into(),
            body: text,
            kind: NoticeKind::Copied,
        }))
    }
}

/// Window classes of the shell's own bars: the taskbar (main and other
/// screens) and the tray overflow. A click on a tray icon activates one of
/// them, so it is never the window to give the keyboard back to.
const SHELL_CLASSES: [&str; 4] = [
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
];

fn is_shell_class(class: &str) -> bool {
    SHELL_CLASSES.iter().any(|c| c.eq_ignore_ascii_case(class))
}

/// The window the search hands the keyboard back to after the tray menu:
/// `before` (the foreground when the menu opened) when it is an ordinary
/// window; when it is the taskbar or the island, the topmost ordinary
/// window under it in the z-order (the one that had the keyboard before the
/// click, in practice); else none. Ordinary: visible, not minimised, not
/// cloaked (another virtual desktop, a suspended app), not a tool window,
/// not always-on-top and not the shell.
fn window_to_return_to(before: HWND, island: HWND) -> Option<HWND> {
    let ordinary = |hwnd: HWND| unsafe {
        if hwnd.is_invalid() || hwnd == island {
            return false;
        }
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return false;
        }
        let ex = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
        if ex.contains(WS_EX_TOOLWINDOW)
            || ex.contains(WS_EX_TOPMOST)
            || ex.contains(WS_EX_NOACTIVATE)
        {
            return false;
        }
        let mut cloaked = 0u32;
        let read = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        );
        if read.is_ok() && cloaked != 0 {
            return false;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class).max(0) as usize;
        !is_shell_class(&String::from_utf16_lossy(&class[..n]))
    };
    if before.is_invalid() || before == island {
        // The island had it: the search already knows where to go back to.
        return None;
    }
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(before, &mut class) }.max(0) as usize;
    if !is_shell_class(&String::from_utf16_lossy(&class[..n])) {
        return Some(before);
    }
    // At most a few hundred windows: a z-order walk is cheap, and bounded.
    let mut at = unsafe { GetTopWindow(None) }.ok()?;
    for _ in 0..512 {
        if ordinary(at) {
            return Some(at);
        }
        at = unsafe { GetWindow(at, GW_HWNDNEXT) }.ok()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_highlight_stops_at_both_ends() {
        assert_eq!(move_selection(0, 4, true), 0);
        assert_eq!(move_selection(0, 4, false), 1);
        assert_eq!(move_selection(3, 4, false), 3);
        assert_eq!(move_selection(3, 4, true), 2);
        assert_eq!(move_selection(0, 0, false), 0);
        assert_eq!(move_selection(5, 0, true), 0);
    }

    fn result(name: &str) -> SearchResult {
        SearchResult {
            kind: search::Kind::File,
            title: name.into(),
            subtitle: String::new(),
            action: Action::OpenPath(std::path::PathBuf::from(format!(r"C:\Home\{name}"))),
            path: None,
        }
    }

    #[test]
    fn the_highlight_stays_on_the_same_result_when_more_arrive() {
        let names =
            |list: &[&str]| -> Vec<SearchResult> { list.iter().map(|n| result(n)).collect() };
        let old = names(&["a", "b", "c"]);
        // More results came in above and below: "b" moved down.
        let more = names(&["x", "a", "y", "b", "c", "z"]);
        assert_eq!(keep_selection(&old, 1, &more), 3);
        assert_eq!(keep_selection(&old, 0, &more), 1);
        assert_eq!(keep_selection(&old, 2, &more), 4);
        // The same list: where it was.
        assert_eq!(keep_selection(&old, 2, &old), 2);
        // The result is gone: the first row.
        let gone = names(&["x", "a", "c"]);
        assert_eq!(keep_selection(&old, 1, &gone), 0);
        // Nothing before, nothing now, a stale index: the first row.
        assert_eq!(keep_selection(&[], 0, &more), 0);
        assert_eq!(keep_selection(&old, 9, &more), 0);
        assert_eq!(keep_selection(&old, 1, &[]), 0);
    }

    #[test]
    fn a_fresh_or_running_walk_is_not_started_again() {
        let now = Instant::now();
        assert!(walk_due(None, false, now), "never walked");
        assert!(!walk_due(None, true, now), "running");
        let ago = |s: u64| now.checked_sub(Duration::from_secs(s));
        assert!(!walk_due(ago(5), false, now), "five seconds old");
        assert!(!walk_due(ago(29), false, now));
        assert!(walk_due(ago(31), false, now), "stale");
        assert!(!walk_due(ago(31), true, now), "stale but running");
    }

    #[test]
    fn the_taskbar_and_tray_overflow_are_never_returned_to() {
        for class in [
            "Shell_TrayWnd",
            "shell_traywnd",
            "Shell_SecondaryTrayWnd",
            "NotifyIconOverflowWindow",
            "TopLevelWindowForOverflowXamlIsland",
        ] {
            assert!(is_shell_class(class), "{class}");
        }
        for class in [
            "Chrome_WidgetWin_1",
            "Notepad",
            "CabinetWClass",
            "",
            "Shell_Tray",
        ] {
            assert!(!is_shell_class(class), "{class}");
        }
        // The island, or nothing, before the menu: nothing new to remember.
        let island = HWND(std::ptr::dangling_mut());
        assert_eq!(window_to_return_to(island, island), None);
        assert_eq!(window_to_return_to(HWND::default(), island), None);
    }

    #[test]
    fn text_goes_to_the_clipboard_as_unicode() {
        let formats = text_formats("12,5");
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0].0, 13);
        // UTF-16LE, NUL-terminated.
        assert_eq!(formats[0].1, [b'1', 0, b'2', 0, b',', 0, b'5', 0, 0, 0]);
    }

    #[test]
    fn labels_name_the_kinds_in_both_languages() {
        for lang in [i18n::Lang::Id, i18n::Lang::En] {
            i18n::set_lang(lang);
            let apps = [AppEntry {
                name: "Kalkulator".into(),
                path: "x.lnk".into(),
            }];
            let found = with_labels(|l| search::search("kal", &apps, &[], lang, l));
            assert_eq!(found[0].subtitle, tr(Key::SearchKindApp));
            assert_eq!(found.last().unwrap().subtitle, tr(Key::SearchKindWeb));
            assert!(found.last().unwrap().title.contains("kal"));
        }
        i18n::set_lang(i18n::Lang::Id);
    }
}
