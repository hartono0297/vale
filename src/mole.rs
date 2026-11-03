//! The cleaner's "Otomatis · Mole" mode (keputusan-v3.1 decision 4b,
//! option D): the island runs a Mole for Windows the user installed, as a
//! separate process, and reads what it prints. No Mole code is copied,
//! vendored or linked; Mole is only started, never read.
//!
//! The flow is fixed by types: `clean` takes a `Preview`, which only a
//! finished dry-run (`preview`) makes, for the same options, plus a
//! `Confirmed` from the panel's second click. Changing an option throws the
//! preview away; the panel then asks for a new one.
//!
//! What Mole prints is shown as a log (ANSI codes stripped, its internal
//! table rows left out) and, for the preview, read into four groups. A
//! group without a readable line is `Unreadable` ("lihat log"): the island
//! never guesses a number, and never uses Mole's own summary line. What a
//! clean freed is measured on the disks, before and after
//! (`clean::measure_freed`).
//!
//! Facts about Mole's CLI come from its public docs (the Windows reference
//! of `tw93/mole`); what they do not confirm is a named constant below
//! with a note saying so.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsHandle, AsRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::{AssignProcessToJobObject, TerminateJobObject};
use windows::Win32::System::Threading::{
    CREATE_NEW_CONSOLE, CREATE_NO_WINDOW, GetExitCodeProcess, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{HSTRING, PCWSTR, w};

use crate::clean::{self, AdminOutcome};
use crate::i18n::{self, Key, Lang, text};
use crate::model::{Event, Notice, NoticeKind};

/// Mole for Windows' README (the `windows` branch of tw93/mole), linked
/// from the "Cara memasang" card. The island never installs Mole itself.
pub const README_URL: &str = "https://github.com/tw93/Mole/tree/windows";

/// Where the installer puts the script, under `%LOCALAPPDATA%`. From
/// keputusan-v3.1 (the install on the owner's PC); Mole's docs do not name
/// the folder.
const SCRIPT_REL: &str = "Mole\\mole.ps1";
/// The command on PATH, as the docs call it (`mo clean --dry-run`).
const PATH_NAME: &str = "mo";
/// The extensions a `mo` on PATH may have that the island can start.
const PATH_EXTS: [&str; 3] = [".cmd", ".bat", ".exe"];
/// Windows' own PATHEXT when the variable is unset.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";
/// Read beside the script for the version.
const VERSION_FILE: &str = "VERSION";
/// The release line the island was built and tested against.
const TESTED_LINE: &str = "1.30.";

/// Mole's whitelist for `clean`, under `%USERPROFILE%`: one full path or
/// wildcard per line, `#` comments, no case, a folder protecting what is
/// under it. From Mole's Windows reference (written for releases after
/// 1.30.x; not confirmed for 1.30 itself).
pub const WHITELIST_REL: &str = ".config\\mole\\whitelist.txt";

/// A section header's mark ("▸ User essentials", from the design's log of
/// Mole 1.30.0; not in Mole's docs). `?` and U+FFFD stand in for it when
/// the console's code page could not carry it.
const SECTION_MARKS: [char; 3] = ['\u{25B8}', '?', '\u{FFFD}'];
/// What ends a dry-run item line, "Name (N items, X MB dry)" (the design's
/// capture of Mole 1.30.0; not confirmed by the docs).
const DRY_WORD: &str = "dry";
/// Mole's internal table (keputusan-v3.1 p9: "Size / Removed / Failed"
/// rows are printed too). Its exact shape is not documented.
const TABLE_WORDS: [&str; 3] = ["Size", "Removed", "Failed"];
/// Names an item line may carry that are totals, never items: Mole's
/// summary is known to be wrong (keputusan-v3.1 p9) and is never used.
const SUMMARY_WORDS: [&str; 5] = ["total", "summary", "potential", "reclaimable", "freed"];
/// Mole's sizes are taken as binary (1 KB = 1024 B), like the island's own
/// figures; the docs do not say.
const UNIT_BASE: f64 = 1024.0;

/// Lines of Mole's log the panel keeps; older ones scroll away.
pub const LOG_CAP: usize = 400;

// ---- Detection ----

/// How a found Mole is started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launcher {
    /// `mole.ps1` / `mo.ps1`: through `powershell.exe -File`.
    Script,
    /// `mo.cmd`, `mo.bat`, `mo.exe`: run as it is.
    Direct,
}

/// A Mole found on this PC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub path: PathBuf,
    pub launcher: Launcher,
    /// The `VERSION` file beside it, trimmed; `None` when missing or
    /// unreadable.
    pub version: Option<String>,
}

impl Install {
    /// The release the island was tested against (1.30.x).
    pub fn tested(&self) -> bool {
        tested(self.version.as_deref())
    }
}

/// Only 1.30.x is tested; any other or unknown version shows a warning
/// but may still run.
pub fn tested(version: Option<&str>) -> bool {
    let Some(v) = version else {
        return false;
    };
    let v = v.trim();
    let v = v.strip_prefix(['v', 'V']).unwrap_or(v);
    v.strip_prefix(TESTED_LINE)
        .is_some_and(|patch| !patch.is_empty() && patch.chars().all(|c| c.is_ascii_digit()))
}

/// What detection reads, so tests run without the OS.
pub struct MoleEnv<'a> {
    pub local_app_data: Option<OsString>,
    pub path: Option<OsString>,
    pub pathext: Option<String>,
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub read_to_string: &'a dyn Fn(&Path) -> io::Result<String>,
}

/// `%LOCALAPPDATA%\Mole\mole.ps1` first, then `mo` on PATH (`mo.ps1`, then
/// `.cmd`, `.bat`, `.exe` in PATHEXT's order), the version from `VERSION`
/// beside it.
pub fn detect(env: &MoleEnv) -> Option<Install> {
    let found = env
        .local_app_data
        .as_ref()
        .map(|local| PathBuf::from(local).join(SCRIPT_REL))
        .filter(|p| (env.exists)(p))
        .map(|p| (p, Launcher::Script))
        .or_else(|| on_path(env))?;
    let (path, launcher) = found;
    let version = path
        .parent()
        .map(|dir| dir.join(VERSION_FILE))
        .and_then(|v| (env.read_to_string)(&v).ok())
        .map(|s| s.trim().trim_start_matches('\u{feff}').trim().to_owned())
        .filter(|s| !s.is_empty());
    Some(Install {
        path,
        launcher,
        version,
    })
}

fn on_path(env: &MoleEnv) -> Option<(PathBuf, Launcher)> {
    let path = env.path.as_ref()?;
    let pathext = env.pathext.as_deref().unwrap_or(DEFAULT_PATHEXT);
    let mut names = vec![(format!("{PATH_NAME}.ps1"), Launcher::Script)];
    for ext in pathext.split(';').map(|e| e.trim().to_ascii_lowercase()) {
        if PATH_EXTS.contains(&ext.as_str()) {
            names.push((format!("{PATH_NAME}{ext}"), Launcher::Direct));
        }
    }
    for dir in std::env::split_paths(path).filter(|d| !d.as_os_str().is_empty()) {
        for (name, launcher) in &names {
            let candidate = dir.join(name);
            if (env.exists)(&candidate) {
                return Some((candidate, *launcher));
            }
        }
    }
    None
}

/// Detection on this PC.
pub fn detect_here() -> Option<Install> {
    let exists = |p: &Path| p.is_file();
    let read = |p: &Path| std::fs::read_to_string(p);
    detect(&MoleEnv {
        local_app_data: std::env::var_os("LOCALAPPDATA"),
        path: std::env::var_os("PATH"),
        pathext: std::env::var("PATHEXT").ok(),
        exists: &exists,
        read_to_string: &read,
    })
}

// ---- Invocation ----

/// The two options the panel offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MoleOpts {
    /// "+ Sistem (admin)": `--system`, run elevated after confirmation.
    pub system: bool,
    /// "+ Media game > 90 hari": `--game-media`.
    pub game_media: bool,
}

/// Which of the two options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoleOpt {
    System,
    GameMedia,
}

impl MoleOpts {
    pub fn toggled(self, opt: MoleOpt) -> Self {
        match opt {
            MoleOpt::System => Self {
                system: !self.system,
                ..self
            },
            MoleOpt::GameMedia => Self {
                game_media: !self.game_media,
                ..self
            },
        }
    }

    pub fn get(self, opt: MoleOpt) -> bool {
        match opt {
            MoleOpt::System => self.system,
            MoleOpt::GameMedia => self.game_media,
        }
    }
}

/// Mole's arguments: `clean [--dry-run] [--system] [--game-media]`.
pub fn clean_args(opts: MoleOpts, dry_run: bool, system: bool) -> Vec<String> {
    let mut args = vec!["clean".to_owned()];
    if dry_run {
        args.push("--dry-run".into());
    }
    if system {
        args.push("--system".into());
    }
    if opts.game_media {
        args.push("--game-media".into());
    }
    args
}

/// A program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl Invocation {
    /// Program then arguments, as a fake runner records them.
    #[cfg(test)]
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.program.to_string_lossy().into_owned())
            .chain(self.args.iter().cloned())
            .collect()
    }
}

/// Windows PowerShell 5.1, from System32 when it is there.
fn powershell_exe() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

/// A script through `powershell.exe -NoProfile -ExecutionPolicy Bypass
/// -File <script> …`; a `.cmd` / `.exe` as it is.
pub fn invocation(install: &Install, mole_args: &[String]) -> Invocation {
    match install.launcher {
        Launcher::Script => {
            let mut args: Vec<String> = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
                .map(String::from)
                .into();
            args.push(install.path.to_string_lossy().into_owned());
            args.extend(mole_args.iter().cloned());
            Invocation {
                program: powershell_exe(),
                args,
            }
        }
        Launcher::Direct => Invocation {
            program: install.path.clone(),
            args: mole_args.to_vec(),
        },
    }
}

// ---- Output ----

/// One line as the panel shows it: CSI sequences (colours, cursor moves,
/// erase), OSC sequences (ended by BEL or ST), other ESC sequences and
/// every other control character removed; of a line overwritten with `\r`,
/// only its last part.
pub fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                // CSI: parameters and intermediates, then a final byte.
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: up to BEL or ESC \.
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // ESC, intermediates (0x20–0x2F), a final byte.
                Some(c) if ('\u{20}'..='\u{2f}').contains(&c) => {
                    while chars
                        .next_if(|c| ('\u{20}'..='\u{2f}').contains(c))
                        .is_some()
                    {}
                    chars.next();
                }
                _ => {}
            },
            // The 8-bit CSI.
            '\u{9b}' => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            '\r' => out.push('\r'),
            '\t' => out.push(' '),
            '\u{feff}' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    match out.rsplit('\r').find(|part| !part.trim().is_empty()) {
        Some(last) => last.trim_end().to_owned(),
        None => String::new(),
    }
}

/// Box-drawing and separator characters: a line of only these (and
/// spaces) is a rule, not text.
fn is_rule_char(c: char) -> bool {
    ('\u{2500}'..='\u{257f}').contains(&c) || matches!(c, '-' | '=' | '_' | '+' | '|' | '*' | ' ')
}

/// Mole's internal table: a header naming Size / Removed / Failed, a row
/// of one of them and a figure ("Removed: 12"), or a row split into cells
/// by `│` / `|`. "Failed to remove …" is a message, not the table.
fn is_table_row(line: &str) -> bool {
    let words: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || c == '\u{2502}' || c == '|' || c == ':')
        .filter(|w| !w.is_empty())
        .collect();
    let named = words.iter().filter(|w| TABLE_WORDS.contains(w)).count();
    let body = line.trim_start();
    let first = TABLE_WORDS.iter().any(|w| {
        body.strip_prefix(w).is_some_and(|rest| {
            let rest = rest.trim_start_matches(':').trim();
            rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_digit())
        })
    });
    let cells = line
        .chars()
        .filter(|c| matches!(c, '\u{2502}' | '\u{2503}' | '|'))
        .count();
    named >= 2 || first || cells >= 2
}

/// The line as the panel's log shows it, or `None` for what it leaves
/// out: empty lines, rules and box drawing, Mole's internal table. Section
/// headers and item lines are kept.
pub fn log_line(raw: &str) -> Option<String> {
    let line = strip_ansi(raw);
    let body = line.trim();
    if body.is_empty() || body.chars().all(is_rule_char) {
        return None;
    }
    if is_section(&line) {
        return Some(line);
    }
    if is_table_row(body) {
        return None;
    }
    Some(line)
}

/// "▸ User essentials".
pub fn is_section(line: &str) -> bool {
    section_name(line).is_some()
}

fn section_name(line: &str) -> Option<&str> {
    let body = line.trim_start();
    let first = body.chars().next()?;
    if !SECTION_MARKS.contains(&first) {
        return None;
    }
    let name = body[first.len_utf8()..].trim();
    // The mark, a space, then a name that starts with a letter.
    (body[first.len_utf8()..].starts_with(' ') && name.starts_with(char::is_alphabetic))
        .then_some(name.trim_end_matches(['\u{2026}', '.']).trim())
}

// ---- Parsing the preview ----

/// The panel's four groups, in the design's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoleGroup {
    Browser,
    Apps,
    Dev,
    TempLogs,
}

impl MoleGroup {
    pub const ALL: [MoleGroup; 4] = [
        MoleGroup::Browser,
        MoleGroup::Apps,
        MoleGroup::Dev,
        MoleGroup::TempLogs,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

/// What a section of Mole's output counts as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Section {
    Group(MoleGroup),
    /// `--system`: counted (as temp & logs) only when ticked.
    System,
    /// `--game-media`: counted (as app caches) only when ticked.
    GameMedia,
    /// A section the island does not know: in the total, in no group.
    Unknown,
}

/// By the words in its header; Mole's section names are not documented
/// beyond the design's log ("User essentials", "Browser caches",
/// "Developer tools").
fn section_of(name: &str) -> Section {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["browser"]) {
        Section::Group(MoleGroup::Browser)
    } else if has(&["developer", "dev tool", "development", "programming"]) {
        Section::Group(MoleGroup::Dev)
    } else if has(&["game"]) {
        Section::GameMedia
    } else if has(&["system"]) {
        Section::System
    } else if has(&["app"]) {
        Section::Group(MoleGroup::Apps)
    } else if has(&["essential", "temp", "log"]) {
        Section::Group(MoleGroup::TempLogs)
    } else {
        Section::Unknown
    }
}

/// One item line read: its name, item count and size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub items: u64,
    pub bytes: u64,
}

/// "Name (N items, X MB dry)": `.` or `,` decimals, B to TB. `None` for
/// anything else, Mole's summary lines included.
pub fn parse_item(line: &str) -> Option<Item> {
    let line = line.trim();
    let inner_end = line.strip_suffix(')')?;
    let open = inner_end.rfind('(')?;
    let name = inner_end[..open].trim();
    let inner = inner_end[open + 1..].trim();
    if name.is_empty() || !name.starts_with(|c: char| c.is_alphanumeric()) {
        return None;
    }
    let lower = name.to_lowercase();
    if SUMMARY_WORDS.iter().any(|w| lower.starts_with(w)) {
        return None;
    }
    let rest = inner.strip_suffix(DRY_WORD)?.trim_end();
    let (count, size) = rest.split_once(", ")?;
    let mut count = count.split_whitespace();
    let n = count.next()?.replace([',', '.'], "");
    let items: u64 = n.parse().ok()?;
    if !matches!(count.next()?, "item" | "items") || count.next().is_some() {
        return None;
    }
    let mut size = size.split_whitespace();
    let number: f64 = size.next()?.replace(',', ".").parse().ok()?;
    let unit = size.next()?;
    if size.next().is_some() || !number.is_finite() || number < 0.0 {
        return None;
    }
    let power = match unit.to_ascii_uppercase().as_str() {
        "B" => 0,
        "KB" => 1,
        "MB" => 2,
        "GB" => 3,
        "TB" => 4,
        _ => return None,
    };
    let bytes = (number * UNIT_BASE.powi(power)).round() as u64;
    Some(Item {
        name: name.to_owned(),
        items,
        bytes,
    })
}

/// A group's size as the panel shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSize {
    Bytes(u64),
    /// No readable line: "lihat log", never a guess.
    Unreadable,
}

/// The preview read into the panel's groups.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewSummary {
    /// By `MoleGroup::index`.
    pub groups: [GroupSize; 4],
    /// Every entry counted (unknown sections too) and how many; `None`
    /// when nothing could be read.
    pub total: Option<(u64, usize)>,
}

impl PreviewSummary {
    #[cfg(test)]
    pub fn unreadable() -> Self {
        Self {
            groups: [GroupSize::Unreadable; 4],
            total: None,
        }
    }
}

/// Reads a dry-run's lines (stripped or not). An exact duplicate entry
/// (same section, name, count and size) counts once; the System and game
/// media sections only with their option ticked; lines before any header
/// count as an unknown section.
pub fn parse_preview<S: AsRef<str>>(lines: &[S], opts: MoleOpts) -> PreviewSummary {
    let mut section = Section::Unknown;
    let mut seen: HashSet<(Section, String, u64, u64)> = HashSet::new();
    let mut groups: [Option<u64>; 4] = [None; 4];
    let (mut total, mut entries) = (0u64, 0usize);
    for raw in lines {
        let line = strip_ansi(raw.as_ref());
        if let Some(name) = section_name(&line) {
            section = section_of(name);
            continue;
        }
        let Some(item) = parse_item(&line) else {
            continue;
        };
        let group = match section {
            Section::Group(g) => Some(g),
            Section::System if opts.system => Some(MoleGroup::TempLogs),
            Section::GameMedia if opts.game_media => Some(MoleGroup::Apps),
            Section::System | Section::GameMedia => continue,
            Section::Unknown => None,
        };
        if !seen.insert((section, item.name.clone(), item.items, item.bytes)) {
            continue;
        }
        if let Some(g) = group {
            let slot = &mut groups[g.index()];
            *slot = Some(slot.unwrap_or(0).saturating_add(item.bytes));
        }
        total = total.saturating_add(item.bytes);
        entries += 1;
    }
    PreviewSummary {
        groups: groups.map(|g| g.map_or(GroupSize::Unreadable, GroupSize::Bytes)),
        total: (entries > 0).then_some((total, entries)),
    }
}

/// Drive letters a line names as a path ("D:\Games\…").
fn drives_named(line: &str, into: &mut Vec<char>) {
    let chars: Vec<char> = line.chars().collect();
    for w in chars.windows(3) {
        if w[0].is_ascii_alphabetic() && w[1] == ':' && w[2] == '\\' {
            let letter = w[0].to_ascii_uppercase();
            if !into.contains(&letter) {
                into.push(letter);
            }
        }
    }
}

// ---- Running ----

/// Why a run gave nothing usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunFailure {
    /// It could not start.
    Start(String),
    /// It ended with this code (`None`: killed or unknown).
    Exit(Option<i32>),
}

/// How the elevated part went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Elevated {
    /// UAC was refused: skipped, not failed.
    Refused,
    Failed,
    /// It ran: its exit code and the lines it wrote to its log file.
    Done {
        code: u32,
        lines: Vec<String>,
    },
}

/// Starts Mole. The real one runs it hidden; the fake one in the tests
/// records every argv and plays back canned lines.
pub trait MoleRunner {
    /// Runs `inv` hidden, stdin null, stdout and stderr merged; each batch
    /// of raw lines read goes to `lines`. The exit code, `None` when killed.
    fn run(
        &mut self,
        inv: &Invocation,
        preview: bool,
        lines: &mut dyn FnMut(Vec<String>),
    ) -> io::Result<Option<i32>>;

    /// Runs `inv` elevated (UAC), hidden, its output to a log file read
    /// back afterwards.
    fn run_elevated(&mut self, inv: &Invocation) -> Elevated;
}

/// A finished dry-run: only `preview` makes one, so `clean` can only follow
/// a dry-run that ended well, with the options it ran with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    opts: MoleOpts,
    summary: PreviewSummary,
    drives: Vec<char>,
}

impl Preview {
    pub fn opts(&self) -> MoleOpts {
        self.opts
    }

    pub fn summary(&self) -> &PreviewSummary {
        &self.summary
    }
}

/// The panel's second click on "Yakin? Hapus permanen". Made only where
/// the reducer asked for the clean (`Effect::MoleClean`).
#[derive(Debug)]
pub struct Confirmed(());

impl Confirmed {
    pub fn second_click() -> Self {
        Confirmed(())
    }
}

/// The dry-run: Mole lists what it would remove and removes nothing. The
/// log's lines go to `log` as they come.
pub fn run_preview(
    runner: &mut dyn MoleRunner,
    install: &Install,
    opts: MoleOpts,
    log: &mut dyn FnMut(Vec<String>),
) -> Result<Preview, RunFailure> {
    let inv = invocation(install, &clean_args(opts, true, opts.system));
    let mut raw = Vec::new();
    let code = runner
        .run(&inv, true, &mut |batch| {
            let shown: Vec<String> = batch.iter().filter_map(|l| log_line(l)).collect();
            raw.extend(batch);
            if !shown.is_empty() {
                log(shown);
            }
        })
        .map_err(|e| RunFailure::Start(e.to_string()))?;
    if code != Some(0) {
        return Err(RunFailure::Exit(code));
    }
    let mut drives = Vec::new();
    for line in &raw {
        drives_named(line, &mut drives);
    }
    Ok(Preview {
        opts,
        summary: parse_preview(&raw, opts),
        drives,
    })
}

/// The clean was refused before anything ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stale;

/// The confirmed clean: the user's part hidden (`clean [--game-media]`),
/// then with `--system` ticked the system part elevated. `Stale` (and
/// nothing run) when the options are not the preview's.
pub fn run_clean(
    runner: &mut dyn MoleRunner,
    install: &Install,
    preview: &Preview,
    opts: MoleOpts,
    _confirmed: Confirmed,
    log: &mut dyn FnMut(Vec<String>),
) -> Result<CleanRun, Stale> {
    if preview.opts != opts {
        return Err(Stale);
    }
    let inv = invocation(install, &clean_args(opts, false, false));
    let user = runner.run(&inv, false, &mut |batch| {
        let shown: Vec<String> = batch.iter().filter_map(|l| log_line(l)).collect();
        if !shown.is_empty() {
            log(shown);
        }
    });
    let exit = match user {
        Ok(code) => code,
        Err(e) => {
            log(vec![e.to_string()]);
            None
        }
    };
    let admin = if opts.system && exit == Some(0) {
        let inv = invocation(install, &clean_args(opts, false, true));
        match runner.run_elevated(&inv) {
            Elevated::Refused => AdminOutcome::Cancelled,
            Elevated::Failed => AdminOutcome::Failed,
            Elevated::Done { code, lines } => {
                let shown: Vec<String> = lines.iter().filter_map(|l| log_line(l)).collect();
                if !shown.is_empty() {
                    log(shown);
                }
                if code == 0 {
                    AdminOutcome::Done { skipped: 0 }
                } else {
                    AdminOutcome::Failed
                }
            }
        }
    } else {
        AdminOutcome::NotAsked
    };
    Ok(CleanRun { exit, admin })
}

/// How the clean's processes ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanRun {
    /// The user part's exit code; `None` when it was killed or could not
    /// start.
    pub exit: Option<i32>,
    pub admin: AdminOutcome,
}

impl CleanRun {
    /// Everything asked for ran and ended well; a refused UAC prompt only
    /// skips the system part.
    pub fn ok(&self) -> bool {
        self.exit == Some(0) && self.admin != AdminOutcome::Failed
    }
}

/// What the island tells after a clean: the free space it measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoleReport {
    pub run: CleanRun,
    pub freed: u64,
    /// The drive that gained most: its letter, free space before and after.
    pub drive: Option<(char, u64, u64)>,
}

/// The drives a clean is measured on: the system drive and any a preview
/// line named.
pub fn drives_to_measure(system: char, preview: &Preview) -> Vec<char> {
    let mut letters = vec![system.to_ascii_uppercase()];
    for d in &preview.drives {
        if !letters.contains(d) {
            letters.push(*d);
        }
    }
    letters
}

/// Before and after per drive into a report (`clean::measure_freed`).
pub fn report(run: CleanRun, drives: &[(char, u64, u64)]) -> MoleReport {
    let (freed, best) = clean::measure_freed(drives);
    let drive = best.and_then(|(letter, after)| {
        drives
            .iter()
            .find(|d| d.0 == letter)
            .map(|d| (letter, d.1, after))
    });
    MoleReport { run, freed, drive }
}

// ---- Notices ----

/// "Mole selesai · 2,4 GB dibebaskan" (Bata happy) or, failed or stopped,
/// "Mole gagal · kode 1" (Bata low, stays until clicked). `fits(title,
/// text)` says whether a title (`true`) or a body fits the notice as drawn
/// (`Renderer::notice_text_fits`); a shorter wording is chosen, or a
/// lower-priority part left out, when it would not.
pub fn done_notice(lang: Lang, report: &MoleReport, fits: &dyn Fn(bool, &str) -> bool) -> Notice {
    let letter = report.drive.map_or('C', |d| d.0);
    let admin_skipped = report.run.admin == AdminOutcome::Cancelled;
    if report.run.ok() {
        Notice {
            key: "mole-done".into(),
            source: text(lang, Key::SystemClean).into(),
            title: i18n::mole_done_title(lang, report.freed, &|s| fits(true, s)),
            body: i18n::mole_done_body(lang, letter, admin_skipped, &|s| fits(false, s)),
            kind: NoticeKind::MoleDone,
        }
    } else {
        Notice {
            key: "mole-failed".into(),
            source: text(lang, Key::SystemClean).into(),
            title: i18n::mole_failed_title(lang, report.run.exit),
            body: i18n::mole_failed_body(lang, report.freed),
            kind: NoticeKind::MoleFailed,
        }
    }
}

// ---- Whitelist ----

/// Mole's whitelist file under the user's profile.
pub fn whitelist_path(profile: &Path) -> PathBuf {
    profile.join(WHITELIST_REL)
}

/// The default protections: the Cargo caches and Autodesk's folders, so a
/// Mole clean does not make Rust builds and AutoCAD fetch them again.
pub fn default_entries(profile: &Path, local: &Path, roaming: &Path) -> Vec<String> {
    [
        profile.join(".cargo\\registry"),
        profile.join(".cargo\\git"),
        local.join("Autodesk"),
        roaming.join("Autodesk"),
    ]
    .iter()
    .map(|p| p.to_string_lossy().into_owned())
    .collect()
}

fn norm(entry: &str) -> String {
    entry.trim().trim_end_matches(['\\', '/']).to_lowercase()
}

/// The entries of `wanted` not in `existing` already (without case or a
/// trailing slash; comments do not count).
pub fn missing_entries(existing: &str, wanted: &[String]) -> Vec<String> {
    let have: HashSet<String> = existing
        .lines()
        .map(|l| l.trim_start_matches('\u{feff}'))
        .filter(|l| !l.trim_start().starts_with('#'))
        .map(norm)
        .collect();
    wanted
        .iter()
        .filter(|w| !have.contains(&norm(w)))
        .cloned()
        .collect()
}

/// Appends the entries of `wanted` that are missing, keeping every byte
/// already there; never rewrites or removes a line. Written to a temporary
/// file beside it and moved over it. A new file is UTF-8 with a BOM (as
/// Mole's docs ask for Windows PowerShell 5.1). Returns how many were added.
pub fn append_defaults(path: &Path, wanted: &[String]) -> io::Result<usize> {
    let existing = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    // A file with a UTF-16LE BOM (PowerShell 5.1's Out-File, Notepad's
    // "Unicode") is read and extended as UTF-16LE.
    let utf16 = existing
        .as_deref()
        .is_some_and(|b| b.starts_with(&[0xFF, 0xFE]));
    let text = match existing.as_deref() {
        Some(b) if utf16 => String::from_utf16_lossy(
            &b[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect::<Vec<u16>>(),
        ),
        Some(b) => String::from_utf8_lossy(b).into_owned(),
        None => String::new(),
    };
    let missing = missing_entries(&text, wanted);
    if missing.is_empty() {
        return Ok(0);
    }
    let encode = |s: &str| -> Vec<u8> {
        if utf16 {
            s.encode_utf16().flat_map(u16::to_le_bytes).collect()
        } else {
            s.as_bytes().to_vec()
        }
    };
    let mut out = existing
        .clone()
        .unwrap_or_else(|| "\u{feff}".as_bytes().to_vec());
    let body = text.trim_start_matches('\u{feff}');
    if !body.is_empty() && !body.ends_with('\n') {
        out.extend(encode("\r\n"));
    }
    for entry in &missing {
        out.extend(encode(entry));
        out.extend(encode("\r\n"));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("txt.di-tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&out)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(missing.len())
}

/// What adding the default protections before preview `run` tells the
/// island: done, or a line in the preview's log saying the folders are not
/// protected (the preview still runs; nothing was promised to Mole).
fn defaults_event(result: &io::Result<usize>, run: u64) -> MoleEvent {
    match result {
        Ok(_) => MoleEvent::DefaultsAdded,
        Err(_) => MoleEvent::Lines {
            run,
            lines: vec![i18n::tr(Key::MoleProtectFailed).into()],
        },
    }
}

/// The default protections for this user, appended to Mole's whitelist.
fn add_defaults_here() -> io::Result<usize> {
    let var = |name| std::env::var_os(name).map(PathBuf::from);
    let (Some(profile), Some(local), Some(roaming)) =
        (var("USERPROFILE"), var("LOCALAPPDATA"), var("APPDATA"))
    else {
        return Err(io::Error::new(io::ErrorKind::NotFound, "profile folders"));
    };
    let wanted = default_entries(&profile, &local, &roaming);
    append_defaults(&whitelist_path(&profile), &wanted)
}

/// "Lindungi folder…": `clean --whitelist` in a visible window, since the
/// user answers it there (Mole opens the list in Notepad and waits).
pub fn open_whitelist(install: &Install) -> io::Result<()> {
    let inv = invocation(install, &["clean".to_owned(), "--whitelist".to_owned()]);
    Command::new(&inv.program)
        .args(&inv.args)
        .creation_flags(CREATE_NEW_CONSOLE.0)
        .spawn()
        .map(drop)
}

// ---- The real runner ----

/// The job of the run in progress and whether it is a preview, so the
/// panel can stop a preview from the UI thread.
type Current = Arc<Mutex<Option<(Arc<OwnedHandle>, bool)>>>;

/// Runs Mole hidden in a Job Object (`cli::kill_on_close_job`): quitting
/// the island ends it with everything it started.
pub struct RealRunner {
    current: Current,
}

impl MoleRunner for RealRunner {
    fn run(
        &mut self,
        inv: &Invocation,
        preview: bool,
        lines: &mut dyn FnMut(Vec<String>),
    ) -> io::Result<Option<i32>> {
        let (mut reader, writer) = io::pipe()?;
        let mut command = Command::new(&inv.program);
        command
            .args(&inv.args)
            .stdin(Stdio::null())
            .stdout(writer.try_clone()?)
            .stderr(writer)
            .creation_flags(CREATE_NO_WINDOW.0);
        let mut child = command.spawn()?;
        drop(command);
        let process = child.as_handle().try_clone_to_owned()?;
        let job = crate::cli::kill_on_close_job()
            .filter(|job| unsafe { AssignProcessToJobObject(raw(job), raw(&process)).is_ok() });
        let job = job.map(Arc::new);
        if let Some(job) = &job
            && let Ok(mut slot) = self.current.lock()
        {
            *slot = Some((job.clone(), preview));
        }
        let mut partial: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut batch = Vec::new();
                    for &b in &buf[..n] {
                        if b == b'\n' {
                            batch.push(String::from_utf8_lossy(&partial).into_owned());
                            partial.clear();
                        } else {
                            partial.push(b);
                        }
                    }
                    if !batch.is_empty() {
                        lines(batch);
                    }
                }
            }
        }
        if !partial.is_empty() {
            lines(vec![String::from_utf8_lossy(&partial).into_owned()]);
        }
        let code = child.wait().ok().and_then(|s| s.code());
        if let Ok(mut slot) = self.current.lock() {
            *slot = None;
        }
        Ok(code)
    }

    fn run_elevated(&mut self, inv: &Invocation) -> Elevated {
        run_elevated(inv)
    }
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// How long the worker waits for the elevated part.
const ELEVATED_WAIT_MS: u32 = 60 * 60 * 1000;

/// PowerShell's single-quoted string.
fn ps_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        // PowerShell takes U+2018..U+201B as single quotes too; each is
        // escaped by doubling, like '.
        if matches!(c, '\'' | '\u{2018}'..='\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// The elevated PowerShell's command: `call` (already quoted) with all its
/// output to `log`, then its exit code, and 1 for a script that failed
/// without one.
fn elevated_script(call: &str, log: &str) -> String {
    // `$LASTEXITCODE` alone is stale or empty after a script that ends
    // without `exit`, and 0 after one that only wrote errors (with every
    // stream redirected `$?` stays true). An exit code the script set is
    // its word; without one, new entries in `$Error` or a throw fail it.
    format!(
        "$global:LASTEXITCODE = $null; $errors = $Error.Count; try {{ & {call} *> {}; if ($null -ne $LASTEXITCODE) {{ exit [int]$LASTEXITCODE }}; if ($Error.Count -gt $errors) {{ exit 1 }}; exit 0 }} catch {{ exit 1 }}",
        ps_quote(log)
    )
}

/// `inv` elevated through `ShellExecuteExW` verb `runas` (UAC), hidden:
/// PowerShell runs it with all its output sent to a temporary log file,
/// read back once it ends. The island itself is never elevated.
fn run_elevated(inv: &Invocation) -> Elevated {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let log = std::env::temp_dir().join(format!("vale-mole-{}-{n}.log", std::process::id()));
    let (program, args) = match inv.args.iter().position(|a| a == "-File") {
        // A script: call it directly, the rest after it.
        Some(i) if i + 1 < inv.args.len() => (inv.args[i + 1].clone(), &inv.args[i + 2..]),
        _ => (inv.program.to_string_lossy().into_owned(), &inv.args[..]),
    };
    let call: Vec<String> = std::iter::once(ps_quote(&program))
        .chain(args.iter().map(|a| ps_quote(a)))
        .collect();
    let script = elevated_script(&call.join(" "), &log.to_string_lossy());
    let params = HSTRING::from(format!(
        "-NoProfile -ExecutionPolicy Bypass -Command \"{script}\""
    ));
    let file = HSTRING::from(powershell_exe().as_os_str());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        return if e.code() == ERROR_CANCELLED.to_hresult() {
            Elevated::Refused
        } else {
            Elevated::Failed
        };
    }
    if info.hProcess.is_invalid() {
        return Elevated::Failed;
    }
    let code = unsafe {
        let mut code = 0u32;
        let ended = WaitForSingleObject(info.hProcess, ELEVATED_WAIT_MS) == WAIT_OBJECT_0;
        let read = ended && GetExitCodeProcess(info.hProcess, &mut code).is_ok();
        let _ = CloseHandle(info.hProcess);
        read.then_some(code)
    };
    let lines = std::fs::read(&log)
        .map(|b| decode_log(&b))
        .unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    match code {
        Some(code) => Elevated::Done { code, lines },
        None => Elevated::Failed,
    }
}

/// PowerShell 5.1 writes `*>` files as UTF-16 with a BOM; anything else is
/// read as UTF-8.
pub fn decode_log(bytes: &[u8]) -> Vec<String> {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        String::from_utf8_lossy(rest).into_owned()
    };
    text.lines().map(str::to_owned).collect()
}

// ---- The worker ----

enum Request {
    Detect,
    Preview {
        run: u64,
        install: Install,
        opts: MoleOpts,
        protect_defaults: bool,
    },
    Clean {
        run: u64,
        install: Install,
        preview: Box<Preview>,
        opts: MoleOpts,
        confirmed: Confirmed,
    },
}

/// The worker's answers.
#[derive(Clone, Debug, PartialEq)]
pub enum MoleEvent {
    Detected(Option<Install>),
    /// The default protections are in Mole's whitelist (added now or there
    /// already): never add them again.
    DefaultsAdded,
    /// Log lines of run `run` (a preview or a clean), as the panel shows them.
    Lines {
        run: u64,
        lines: Vec<String>,
    },
    Previewed {
        run: u64,
        opts: MoleOpts,
        result: Result<Preview, RunFailure>,
    },
    Cleaned {
        run: u64,
        report: MoleReport,
    },
}

/// The Mole thread, parked until the first request, like `clean::Cleaner`.
pub struct MoleWorker {
    requests: Sender<Request>,
    current: Current,
}

impl MoleWorker {
    pub fn spawn(tx: Sender<Event>, wake: impl Fn() + Send + 'static) -> Option<Self> {
        let (requests, rx) = mpsc::channel();
        let current: Current = Arc::new(Mutex::new(None));
        let runner = RealRunner {
            current: current.clone(),
        };
        let spawned = std::thread::Builder::new()
            .name("mole".into())
            .spawn(move || serve(&rx, runner, &tx, wake));
        match spawned {
            Ok(_) => Some(Self { requests, current }),
            Err(error) => {
                eprintln!("vale: no Mole worker: {error}");
                None
            }
        }
    }

    pub fn detect(&self) -> bool {
        self.requests.send(Request::Detect).is_ok()
    }

    pub fn preview(&self, run: u64, install: Install, opts: MoleOpts, protect: bool) -> bool {
        self.requests
            .send(Request::Preview {
                run,
                install,
                opts,
                protect_defaults: protect,
            })
            .is_ok()
    }

    /// The confirmed clean, after `preview` with the same options.
    pub fn clean(
        &self,
        run: u64,
        install: Install,
        preview: Preview,
        opts: MoleOpts,
        confirmed: Confirmed,
    ) -> bool {
        self.requests
            .send(Request::Clean {
                run,
                install,
                preview: Box::new(preview),
                opts,
                confirmed,
            })
            .is_ok()
    }

    /// Ends a running preview (the cleaner closed or an option changed). A
    /// clean is never stopped here.
    pub fn stop_preview(&self) {
        if let Ok(slot) = self.current.lock()
            && let Some((job, true)) = slot.as_ref()
        {
            unsafe {
                let _ = TerminateJobObject(raw(job), 1);
            }
        }
    }
}

fn serve(rx: &Receiver<Request>, mut runner: RealRunner, tx: &Sender<Event>, wake: impl Fn()) {
    let send = |event: MoleEvent| {
        let ok = tx.send(Event::Mole(event)).is_ok();
        wake();
        ok
    };
    while let Ok(request) = rx.recv() {
        let ok = match request {
            Request::Detect => send(MoleEvent::Detected(detect_here())),
            Request::Preview {
                run,
                install,
                opts,
                protect_defaults,
            } => {
                if protect_defaults {
                    send(defaults_event(&add_defaults_here(), run));
                }
                let result = run_preview(&mut runner, &install, opts, &mut |lines| {
                    send(MoleEvent::Lines { run, lines });
                });
                send(MoleEvent::Previewed { run, opts, result })
            }
            Request::Clean {
                run,
                install,
                preview,
                opts,
                confirmed,
            } => {
                let system = system_drive();
                let letters = drives_to_measure(system, &preview);
                let before: Vec<(char, u64)> = letters
                    .iter()
                    .filter_map(|l| clean::drive_free(*l).map(|f| (*l, f)))
                    .collect();
                let outcome = run_clean(
                    &mut runner,
                    &install,
                    &preview,
                    opts,
                    confirmed,
                    &mut |lines| {
                        send(MoleEvent::Lines { run, lines });
                    },
                );
                let run_result = outcome.unwrap_or(CleanRun {
                    exit: None,
                    admin: AdminOutcome::NotAsked,
                });
                let drives: Vec<(char, u64, u64)> = before
                    .iter()
                    .map(|&(l, b)| (l, b, clean::drive_free(l).unwrap_or(b)))
                    .collect();
                send(MoleEvent::Cleaned {
                    run,
                    report: report(run_result, &drives),
                })
            }
        };
        if !ok {
            return;
        }
    }
}

/// `%SystemDrive%`'s letter, C when unknown.
fn system_drive() -> char {
    std::env::var("SystemDrive")
        .ok()
        .and_then(|d| d.chars().next())
        .filter(char::is_ascii_alphabetic)
        .map_or('C', |c| c.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Records every argv; plays back canned lines and exit codes, one
    /// script per run in order.
    #[derive(Default)]
    struct FakeRunner {
        argv: Vec<Vec<String>>,
        elevated: Vec<Vec<String>>,
        scripts: Vec<(Vec<&'static str>, Option<i32>)>,
        elevated_answer: Option<Elevated>,
    }

    impl MoleRunner for FakeRunner {
        fn run(
            &mut self,
            inv: &Invocation,
            _preview: bool,
            lines: &mut dyn FnMut(Vec<String>),
        ) -> io::Result<Option<i32>> {
            self.argv.push(inv.argv());
            let (out, code) = if self.scripts.is_empty() {
                (Vec::new(), Some(0))
            } else {
                self.scripts.remove(0)
            };
            for chunk in out.chunks(3) {
                lines(chunk.iter().map(|s| s.to_string()).collect());
            }
            Ok(code)
        }

        fn run_elevated(&mut self, inv: &Invocation) -> Elevated {
            self.elevated.push(inv.argv());
            self.elevated_answer.clone().unwrap_or(Elevated::Refused)
        }
    }

    fn script_install() -> Install {
        Install {
            path: PathBuf::from(r"C:\Users\x\AppData\Local\Mole\mole.ps1"),
            launcher: Launcher::Script,
            version: Some("1.30.0".into()),
        }
    }

    const MIB: u64 = 1024 * 1024;

    /// A dry-run as Mole 1.30.0 printed it on the owner's PC (keputusan
    /// p8–p9, the design's log), with ANSI colours, an internal table, a
    /// duplicated entry, a broken line, an unknown section and the summary.
    const SAMPLE: &[&str] = &[
        "\u{1b}[1;36mClean Your Windows\u{1b}[0m",
        "Free space: 47,24 GB",
        "",
        "\u{1b}[35m\u{25B8} User essentials\u{1b}[0m",
        "  User temp files (34 items, 61 MB dry)",
        "  Thumbnail cache (12 items, 10,1 MB dry)",
        "  Error reports + memory dumps (8 items, 16.2 MB dry)",
        "\u{25B8} Browser caches",
        "  Chrome CacheStorage (210 items, 922,6 MB dry)",
        "  Chrome Code Cache (88 items, 506,9 MB dry)",
        "  Chrome Cache (140 items, 396,1 MB dry)",
        "  Chrome Cache (140 items, 396,1 MB dry)",
        "  Size        Removed   Failed",
        "  \u{2502} 1.2 GB \u{2502} 0 \u{2502} 0 \u{2502}",
        "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        "\u{25B8} App caches",
        "  VS Code cache (77 items, 401,3 MB dry)",
        "  Discord cache (9 items, broken",
        "\u{25B8} Developer tools",
        "  pip cache (55 items, 90,9 MB dry)",
        "  NuGet HTTP cache (3 items, 20 MB dry)",
        "\u{25B8} Quantum flux",
        "  Mystery cache (1 item, 5 MB dry)",
        "Total (29 items, 2,80 GB dry)",
        "No significant reclaimable space detected",
    ];

    #[test]
    fn parses_a_realistic_dry_run() {
        let s = parse_preview(SAMPLE, MoleOpts::default());
        let mb = |x: f64| (x * MIB as f64).round() as u64;
        let g = |g: MoleGroup| s.groups[g.index()];
        assert_eq!(
            g(MoleGroup::TempLogs),
            GroupSize::Bytes(mb(61.0) + mb(10.1) + mb(16.2))
        );
        // The duplicated Chrome Cache counts once.
        assert_eq!(
            g(MoleGroup::Browser),
            GroupSize::Bytes(mb(922.6) + mb(506.9) + mb(396.1))
        );
        // The broken Discord line is skipped; VS Code is read.
        assert_eq!(g(MoleGroup::Apps), GroupSize::Bytes(mb(401.3)));
        assert_eq!(g(MoleGroup::Dev), GroupSize::Bytes(mb(90.9) + mb(20.0)));
        // The unknown section is in the total, in no group; the summary
        // line ("Total …", the wrong "No significant …") never counts.
        let (total, entries) = s.total.expect("total");
        let groups: u64 = MoleGroup::ALL
            .iter()
            .map(|x| match g(*x) {
                GroupSize::Bytes(b) => b,
                GroupSize::Unreadable => 0,
            })
            .sum();
        assert_eq!(total, groups + mb(5.0));
        assert_eq!(entries, 10);
    }

    #[test]
    fn item_lines_units_and_decimals() {
        let item = |s| parse_item(s).map(|i| (i.items, i.bytes));
        assert_eq!(item("  A (1 item, 512 B dry)"), Some((1, 512)));
        assert_eq!(item("A (2 items, 1,5 KB dry)"), Some((2, 1536)));
        assert_eq!(item("A (2 items, 1.5 KB dry)"), Some((2, 1536)));
        assert_eq!(item("A (3 items, 2 GB dry)"), Some((3, 2 * 1024 * MIB)));
        assert_eq!(
            item("A (3 items, 1 TB dry)"),
            Some((3, 1024 * 1024 * 1024 * 1024))
        );
        assert_eq!(item("Big (1,234 items, 1 MB dry)"), Some((1234, MIB)));
        // Not "dry", not a size, a summary, or broken: not an item.
        assert_eq!(item("A (3 items, 2 GB)"), None);
        assert_eq!(item("A (… 922 MB)"), None);
        assert_eq!(item("A (3 items, 2 XB dry)"), None);
        assert_eq!(item("A (3 items, 2 GB dry"), None);
        assert_eq!(item("Total (29 items, 2,8 GB dry)"), None);
        assert_eq!(item("(3 items, 2 GB dry)"), None);
        assert_eq!(item("A (many items, 2 GB dry)"), None);
    }

    #[test]
    fn options_decide_the_system_and_game_sections() {
        let lines = [
            "\u{25B8} System",
            "  Windows temp (5 items, 100 MB dry)",
            "\u{25B8} Game media",
            "  Replays (2 items, 300 MB dry)",
        ];
        let none = parse_preview(&lines, MoleOpts::default());
        assert_eq!(none.total, None, "neither counted without its option");
        assert_eq!(none, PreviewSummary::unreadable());
        let both = parse_preview(
            &lines,
            MoleOpts {
                system: true,
                game_media: true,
            },
        );
        assert_eq!(
            both.groups[MoleGroup::TempLogs.index()],
            GroupSize::Bytes(100 * MIB)
        );
        assert_eq!(
            both.groups[MoleGroup::Apps.index()],
            GroupSize::Bytes(300 * MIB)
        );
        assert_eq!(both.total, Some((400 * MIB, 2)));
    }

    #[test]
    fn nothing_readable_is_unreadable_never_a_guess() {
        let junk = [
            "Clean Your Windows",
            "\u{25B8} Browser caches",
            "  Chrome CacheStorage (… 922 MB)",
            "  Chrome Code Cache (… 507 MB)",
            "garbage \u{1b}[31m(((",
        ];
        assert_eq!(
            parse_preview(&junk, MoleOpts::default()),
            PreviewSummary::unreadable()
        );
        assert_eq!(
            parse_preview::<&str>(&[], MoleOpts::default()),
            PreviewSummary::unreadable()
        );
        // One readable group, the others "lihat log".
        let one = ["\u{25B8} Developer tools", "  pip cache (1 item, 1 MB dry)"];
        let s = parse_preview(&one, MoleOpts::default());
        assert_eq!(s.groups[MoleGroup::Dev.index()], GroupSize::Bytes(MIB));
        for g in [MoleGroup::Browser, MoleGroup::Apps, MoleGroup::TempLogs] {
            assert_eq!(s.groups[g.index()], GroupSize::Unreadable);
        }
        // A garbled mark still opens its section.
        let garbled = ["? Browser caches", "  Edge (1 item, 2 MB dry)"];
        let s = parse_preview(&garbled, MoleOpts::default());
        assert_eq!(
            s.groups[MoleGroup::Browser.index()],
            GroupSize::Bytes(2 * MIB)
        );
    }

    #[test]
    fn strips_ansi_and_control_characters() {
        assert_eq!(strip_ansi("\u{1b}[1;32mGreen\u{1b}[0m"), "Green");
        assert_eq!(strip_ansi("\u{1b}[2K\u{1b}[1Gline"), "line");
        assert_eq!(strip_ansi("\u{1b}[?25lhidden\u{1b}[?25h"), "hidden");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}after"), "after");
        assert_eq!(strip_ansi("\u{1b}]8;;http://x\u{1b}\\link"), "link");
        assert_eq!(strip_ansi("\u{1b}(Bplain\u{1b}=x"), "plainx");
        assert_eq!(strip_ansi("a\u{7}b\u{8}c\u{0}d"), "abcd");
        assert_eq!(strip_ansi("tab\there"), "tab here");
        assert_eq!(strip_ansi("\u{9b}31mred"), "red");
        // A line overwritten with `\r` keeps its last part.
        assert_eq!(strip_ansi("Scanning 10%\rScanning 55%\rDone"), "Done");
        assert_eq!(strip_ansi("Scanning 10%\rScanning 100%\r"), "Scanning 100%");
        assert_eq!(strip_ansi("text\r\n"), "text");
        assert_eq!(
            strip_ansi("\u{feff}\u{25B8} User essentials"),
            "\u{25B8} User essentials"
        );
    }

    #[test]
    fn the_log_leaves_out_tables_and_rules() {
        assert_eq!(log_line(""), None);
        assert_eq!(log_line("   \u{1b}[0m  "), None);
        assert_eq!(log_line("\u{2500}\u{2500}\u{2500}\u{2500}"), None);
        assert_eq!(log_line("+------+------+"), None);
        assert_eq!(log_line("====="), None);
        assert_eq!(log_line("  Size        Removed   Failed"), None);
        assert_eq!(log_line("  Removed: 12"), None);
        assert_eq!(
            log_line("Failed to remove C:\\x: in use").as_deref(),
            Some("Failed to remove C:\\x: in use")
        );
        assert_eq!(log_line("  \u{2502} 1.2 GB \u{2502} 0 \u{2502}"), None);
        assert_eq!(log_line("| a | b |"), None);
        assert_eq!(
            log_line("\u{1b}[35m\u{25B8} User essentials\u{1b}[0m").as_deref(),
            Some("\u{25B8} User essentials")
        );
        assert_eq!(
            log_line("  User temp files (34 items, 61 MB)").as_deref(),
            Some("  User temp files (34 items, 61 MB)")
        );
        assert_eq!(
            log_line("Free space: 47,24 GB").as_deref(),
            Some("Free space: 47,24 GB")
        );
        assert!(is_section("\u{25B8} Developer tools \u{2026}"));
        assert!(!is_section("  Chrome Cache (1 item, 1 MB dry)"));
        assert!(!is_section("?"));
    }

    fn env_with<'a>(
        local: Option<&str>,
        path: Option<&str>,
        pathext: Option<&str>,
        exists: &'a dyn Fn(&Path) -> bool,
        read: &'a dyn Fn(&Path) -> io::Result<String>,
    ) -> MoleEnv<'a> {
        MoleEnv {
            local_app_data: local.map(OsString::from),
            path: path.map(OsString::from),
            pathext: pathext.map(String::from),
            exists,
            read_to_string: read,
        }
    }

    #[test]
    fn detects_the_script_first_then_mo_on_path() {
        let files: HashMap<PathBuf, &str> = [
            (PathBuf::from(r"C:\L\Mole\mole.ps1"), ""),
            (PathBuf::from(r"C:\L\Mole\VERSION"), " 1.30.0\r\n"),
            (PathBuf::from(r"D:\bin\mo.cmd"), ""),
            (PathBuf::from(r"D:\bin\VERSION"), "1.31.2"),
            (PathBuf::from(r"E:\tools\mo.exe"), ""),
        ]
        .into_iter()
        .collect();
        let exists = |p: &Path| files.contains_key(p);
        let read = |p: &Path| {
            files
                .get(p)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        };
        // The script under %LOCALAPPDATA% wins.
        let env = env_with(Some(r"C:\L"), Some(r"D:\bin"), None, &exists, &read);
        let found = detect(&env).expect("found");
        assert_eq!(found.path, PathBuf::from(r"C:\L\Mole\mole.ps1"));
        assert_eq!(found.launcher, Launcher::Script);
        assert_eq!(found.version.as_deref(), Some("1.30.0"));
        assert!(found.tested());
        // Without it, `mo` on PATH (empty entries passed over).
        let env = env_with(
            Some(r"C:\nope"),
            Some(r";D:\bin;E:\tools"),
            None,
            &exists,
            &read,
        );
        let found = detect(&env).expect("on path");
        assert_eq!(found.path, PathBuf::from(r"D:\bin\mo.cmd"));
        assert_eq!(found.launcher, Launcher::Direct);
        assert_eq!(found.version.as_deref(), Some("1.31.2"));
        assert!(!found.tested());
        // PATHEXT decides which extensions count, in its order.
        let env = env_with(None, Some(r"D:\bin;E:\tools"), Some(".EXE"), &exists, &read);
        let found = detect(&env).expect("exe");
        assert_eq!(found.path, PathBuf::from(r"E:\tools\mo.exe"));
        // A missing VERSION: no version, a warning, still usable.
        assert_eq!(found.version, None);
        assert!(!found.tested());
        // Nothing anywhere.
        let env = env_with(Some(r"C:\nope"), Some(r"F:\x"), None, &exists, &read);
        assert_eq!(detect(&env), None);
        let env = env_with(None, None, None, &exists, &read);
        assert_eq!(detect(&env), None);
        // mo.ps1 on PATH runs through PowerShell.
        let ps: HashMap<PathBuf, &str> =
            [(PathBuf::from(r"G:\m\mo.ps1"), "")].into_iter().collect();
        let exists = |p: &Path| ps.contains_key(p);
        let env = env_with(None, Some(r"G:\m"), None, &exists, &read);
        assert_eq!(detect(&env).expect("ps1").launcher, Launcher::Script);
    }

    #[test]
    fn only_1_30_is_tested() {
        assert!(tested(Some("1.30.0")));
        assert!(tested(Some("1.30.12")));
        assert!(tested(Some("v1.30.1")));
        assert!(!tested(Some("1.30")));
        assert!(!tested(Some("1.3.0")));
        assert!(!tested(Some("1.300.0")));
        assert!(!tested(Some("1.31.0")));
        assert!(!tested(Some("2.30.0")));
        assert!(!tested(Some("1.30.0-beta")));
        assert!(!tested(None));
    }

    #[test]
    fn invocations_are_hidden_powershell_or_direct() {
        let i = invocation(
            &script_install(),
            &clean_args(
                MoleOpts {
                    system: true,
                    game_media: true,
                },
                true,
                true,
            ),
        );
        assert!(
            i.program
                .to_string_lossy()
                .to_lowercase()
                .ends_with("powershell.exe")
        );
        assert_eq!(
            i.args,
            [
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                r"C:\Users\x\AppData\Local\Mole\mole.ps1",
                "clean",
                "--dry-run",
                "--system",
                "--game-media"
            ]
        );
        let direct = Install {
            path: PathBuf::from(r"D:\bin\mo.cmd"),
            launcher: Launcher::Direct,
            version: None,
        };
        let i = invocation(&direct, &clean_args(MoleOpts::default(), false, false));
        assert_eq!(i.argv(), [r"D:\bin\mo.cmd", "clean"]);
    }

    fn sample_ok() -> FakeRunner {
        FakeRunner {
            scripts: vec![(SAMPLE.to_vec(), Some(0))],
            ..FakeRunner::default()
        }
    }

    #[test]
    fn a_clean_needs_a_finished_preview_with_the_same_options() {
        let install = script_install();
        let opts = MoleOpts::default();
        // A dry-run that fails makes no preview: there is nothing to clean with.
        let mut runner = FakeRunner {
            scripts: vec![(SAMPLE.to_vec(), Some(1))],
            ..FakeRunner::default()
        };
        let mut log = Vec::new();
        let failed = run_preview(&mut runner, &install, opts, &mut |l| log.extend(l));
        assert_eq!(failed, Err(RunFailure::Exit(Some(1))));
        assert_eq!(runner.argv.len(), 1);
        assert!(runner.argv[0].contains(&"--dry-run".to_owned()));

        // A finished dry-run: its log is filtered, the options kept.
        let mut runner = sample_ok();
        let mut log = Vec::new();
        let p = run_preview(&mut runner, &install, opts, &mut |l| log.extend(l)).expect("preview");
        assert_eq!(p.opts(), opts);
        assert!(
            log.iter()
                .all(|l| !l.contains('\u{1b}') && !l.trim().is_empty())
        );
        assert!(!log.iter().any(|l| l.contains("Removed")));
        assert_eq!(runner.argv.len(), 1, "a preview only runs the dry-run");

        // Other options than the preview's: refused, nothing runs.
        let other = MoleOpts {
            game_media: true,
            ..opts
        };
        let refused = run_clean(
            &mut runner,
            &install,
            &p,
            other,
            Confirmed::second_click(),
            &mut |_| {},
        );
        assert_eq!(refused, Err(Stale));
        assert_eq!(runner.argv.len(), 1);
        assert!(runner.elevated.is_empty());

        // The preview's options and the confirmation: `clean`, no dry-run.
        let run = run_clean(
            &mut runner,
            &install,
            &p,
            opts,
            Confirmed::second_click(),
            &mut |_| {},
        )
        .expect("clean");
        assert!(run.ok());
        assert_eq!(runner.argv.len(), 2);
        let argv = &runner.argv[1];
        assert!(argv.ends_with(&["clean".to_owned()]), "{argv:?}");
        assert!(!argv.contains(&"--dry-run".to_owned()));
        assert!(runner.elevated.is_empty(), "no system part unticked");
    }

    #[test]
    fn the_system_part_runs_elevated_and_a_refusal_skips_it() {
        let install = script_install();
        let opts = MoleOpts {
            system: true,
            game_media: false,
        };
        let mut runner = sample_ok();
        let p = run_preview(&mut runner, &install, opts, &mut |_| {}).expect("preview");
        assert!(runner.argv[0].contains(&"--system".to_owned()));
        let run = run_clean(
            &mut runner,
            &install,
            &p,
            opts,
            Confirmed::second_click(),
            &mut |_| {},
        )
        .expect("clean");
        // The user part ran hidden without --system, then the elevated one.
        assert!(!runner.argv[1].contains(&"--system".to_owned()));
        assert_eq!(runner.elevated.len(), 1);
        assert!(runner.elevated[0].contains(&"--system".to_owned()));
        assert_eq!(run.admin, AdminOutcome::Cancelled);
        assert!(run.ok(), "a refused UAC prompt skips, it does not fail");

        let mut runner = FakeRunner {
            elevated_answer: Some(Elevated::Done {
                code: 0,
                lines: vec![
                    "\u{25B8} System".into(),
                    "  Windows temp (1 item, 1 MB)".into(),
                ],
            }),
            ..sample_ok()
        };
        let p = run_preview(&mut runner, &install, opts, &mut |_| {}).expect("preview");
        let mut log = Vec::new();
        let run = run_clean(
            &mut runner,
            &install,
            &p,
            opts,
            Confirmed::second_click(),
            &mut |l| log.extend(l),
        )
        .expect("clean");
        assert_eq!(run.admin, AdminOutcome::Done { skipped: 0 });
        assert!(log.iter().any(|l| l.contains("Windows temp")));

        // The user part failing: no elevation at all, and not ok.
        let mut runner = FakeRunner {
            scripts: vec![(SAMPLE.to_vec(), Some(0)), (vec!["boom"], Some(3))],
            ..FakeRunner::default()
        };
        let p = run_preview(&mut runner, &install, opts, &mut |_| {}).expect("preview");
        let run = run_clean(
            &mut runner,
            &install,
            &p,
            opts,
            Confirmed::second_click(),
            &mut |_| {},
        )
        .expect("clean");
        assert_eq!(run.exit, Some(3));
        assert!(runner.elevated.is_empty());
        assert!(!run.ok());
    }

    #[test]
    fn the_result_is_measured_free_space() {
        let p = Preview {
            opts: MoleOpts::default(),
            summary: PreviewSummary::unreadable(),
            drives: vec!['D'],
        };
        assert_eq!(drives_to_measure('c', &p), ['C', 'D']);
        let run = CleanRun {
            exit: Some(0),
            admin: AdminOutcome::NotAsked,
        };
        let r = report(run, &[('C', 100, 350), ('D', 500, 510)]);
        assert_eq!(r.freed, 260);
        assert_eq!(r.drive, Some(('C', 100, 350)));
        let mut found = Vec::new();
        drives_named(r"Removing D:\Games\x and c:\y", &mut found);
        assert_eq!(found, ['D', 'C']);
    }

    #[test]
    fn notices_say_done_or_failed() {
        let gib = 1024 * 1024 * 1024u64;
        let ok = MoleReport {
            run: CleanRun {
                exit: Some(0),
                admin: AdminOutcome::NotAsked,
            },
            freed: gib * 24 / 10,
            drive: Some(('C', gib * 472 / 10, gib * 496 / 10)),
        };
        let n = done_notice(Lang::Id, &ok, &|_, _| true);
        assert_eq!(n.kind, NoticeKind::MoleDone);
        assert_eq!(n.title, "Mole selesai · 2,4 GB dibebaskan");
        assert_eq!(n.body, "Diukur dari sisa disk C:");
        // A narrow notice gets the shorter wordings, never a cut.
        let narrow = |title: bool, s: &str| s.chars().count() <= if title { 20 } else { 12 };
        let n = done_notice(Lang::Id, &ok, &narrow);
        assert_eq!(n.title, "2,4 GB dibebaskan");
        assert_eq!(n.body, "Diukur di C:");
        let failed = MoleReport {
            run: CleanRun {
                exit: Some(1),
                admin: AdminOutcome::NotAsked,
            },
            ..ok.clone()
        };
        let n = done_notice(Lang::En, &failed, &|_, _| true);
        assert_eq!(n.kind, NoticeKind::MoleFailed);
        assert_eq!(n.title, "Mole failed · code 1");
        let stopped = MoleReport {
            run: CleanRun {
                exit: None,
                admin: AdminOutcome::NotAsked,
            },
            ..ok
        };
        assert_eq!(
            done_notice(Lang::Id, &stopped, &|_, _| true).title,
            "Mole berhenti"
        );
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("mole-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn the_whitelist_only_grows() {
        let dir = temp_dir("whitelist");
        let profile = Path::new(r"C:\Users\x");
        let wanted = default_entries(
            profile,
            Path::new(r"C:\Users\x\AppData\Local"),
            Path::new(r"C:\Users\x\AppData\Roaming"),
        );
        assert_eq!(
            wanted,
            [
                r"C:\Users\x\.cargo\registry",
                r"C:\Users\x\.cargo\git",
                r"C:\Users\x\AppData\Local\Autodesk",
                r"C:\Users\x\AppData\Roaming\Autodesk",
            ]
        );
        assert_eq!(
            whitelist_path(profile),
            PathBuf::from(r"C:\Users\x\.config\mole\whitelist.txt")
        );

        // A new file: BOM, every entry.
        let fresh = dir.join("new\\whitelist.txt");
        assert_eq!(append_defaults(&fresh, &wanted).unwrap(), 4);
        let bytes = std::fs::read(&fresh).unwrap();
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        assert_eq!(append_defaults(&fresh, &wanted).unwrap(), 0, "never twice");

        // An existing file without a final newline: kept byte for byte,
        // only what is missing appended (case and a trailing slash ignored).
        let path = dir.join("whitelist.txt");
        let before = "# mine\r\nD:\\Projects\r\nc:\\users\\x\\.CARGO\\registry\\";
        std::fs::write(&path, before).unwrap();
        assert_eq!(append_defaults(&path, &wanted).unwrap(), 3);
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.starts_with(before), "{after:?}");
        assert_eq!(
            after,
            format!(
                "{before}\r\nC:\\Users\\x\\.cargo\\git\r\nC:\\Users\\x\\AppData\\Local\\Autodesk\r\nC:\\Users\\x\\AppData\\Roaming\\Autodesk\r\n"
            )
        );
        // A commented entry does not count as there.
        assert_eq!(
            missing_entries("# C:\\Users\\x\\.cargo\\git\n", &wanted[1..2]),
            &wanted[1..2]
        );
        assert!(!dir.join("whitelist.txt.di-tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_whitelist_that_cannot_be_written_says_so_in_the_log() {
        assert_eq!(defaults_event(&Ok(4), 7), MoleEvent::DefaultsAdded);
        let failed = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        assert_eq!(
            defaults_event(&Err(failed), 7),
            MoleEvent::Lines {
                run: 7,
                lines: vec![text(Lang::Id, Key::MoleProtectFailed).into()]
            }
        );
    }

    #[test]
    fn a_utf16_whitelist_stays_utf16() {
        // Windows PowerShell 5.1's Out-File and Notepad's "Unicode" write
        // UTF-16LE with a BOM; UTF-8 bytes appended to it would be garbage.
        let dir = temp_dir("whitelist-utf16");
        let path = dir.join("whitelist.txt");
        let utf16 = |s: &str| -> Vec<u8> {
            let mut b = vec![0xFF, 0xFE];
            b.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
            b
        };
        let before = "# mine\r\nC:\\Users\\x\\.cargo\\registry\r\n";
        std::fs::write(&path, utf16(before)).unwrap();
        let wanted = [
            r"C:\Users\x\.cargo\registry".to_string(),
            r"C:\Users\x\.cargo\git".to_string(),
        ];
        assert_eq!(
            append_defaults(&path, &wanted).unwrap(),
            1,
            "registry is there"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            utf16(&format!("{before}C:\\Users\\x\\.cargo\\git\r\n"))
        );
        assert_eq!(append_defaults(&path, &wanted).unwrap(), 0, "never twice");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_elevated_step_fails_when_the_script_fails() {
        // A real Windows PowerShell on throwaway scripts in a test folder
        // (nothing elevated, nothing of Mole's).
        let dir = temp_dir("elevated-exit");
        let log = dir.join("out.log");
        let code = |body: &str| {
            let script = dir.join("s.ps1");
            std::fs::write(&script, body).unwrap();
            // As `run_elevated` calls mole.ps1: the script itself.
            let call = ps_quote(&script.to_string_lossy());
            std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
                .arg(elevated_script(&call, &log.to_string_lossy()))
                .creation_flags(CREATE_NO_WINDOW.0)
                .status()
                .unwrap()
                .code()
        };
        assert_eq!(code("Write-Output ok"), Some(0));
        assert_eq!(code("exit 3"), Some(3));
        // An error without `exit` is still a failure, never "done".
        assert_ne!(code("Write-Error 'broken'"), Some(0));
        assert_ne!(code("throw 'broken'"), Some(0));
        // An explicit exit code is the script's own word, errors or not.
        assert_eq!(code("Write-Error 'minor'; exit 0"), Some(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn elevated_logs_are_read_in_either_encoding() {
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "a\r\n\u{25B8} b\r\n"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        assert_eq!(decode_log(&utf16), ["a", "\u{25B8} b"]);
        assert_eq!(decode_log(b"\xEF\xBB\xBFx\ny"), ["x", "y"]);
        assert_eq!(decode_log(b""), Vec::<String>::new());
        assert_eq!(ps_quote("C:\\it's"), "'C:\\it''s'");
        // PowerShell also ends a single-quoted string at U+2018..U+201B.
        assert_eq!(
            ps_quote("C:\\Users\\D\u{2019}Angelo\u{2018}\u{201A}\u{201B}"),
            "'C:\\Users\\D\u{2019}\u{2019}Angelo\u{2018}\u{2018}\u{201A}\u{201A}\u{201B}\u{201B}'"
        );
    }
}
