//! File names for recordings: `YYYY-MM-DD HHmm <title>` plus a collision
//! suffix. Pure; the caller supplies the existence check.
// The recorder session (T3) uses these; until it lands they look unused.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;

/// The longest title kept in a file name, in characters.
const MAX_TITLE: usize = 60;

/// Names Windows refuses as a file name (with or without an extension).
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Makes a title safe as part of a Windows file name: invalid characters
/// (`<>:"/\|?*` and control characters) are removed, runs of whitespace
/// collapse to one space, the result is cut to 60 characters and trimmed
/// of spaces and dots, and a reserved device name gets a trailing `_`.
fn clean_title(title: &str) -> String {
    let kept: String = title
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c))
        .collect();
    let collapsed = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = collapsed.chars().take(MAX_TITLE).collect();
    let mut out = cut.trim_matches(|c: char| c == ' ' || c == '.').to_string();
    let base = out.split('.').next().unwrap_or("");
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(base)) {
        out.push('_');
    }
    out
}

/// `YYYY-MM-DD HHmm <title>`; an empty (or fully stripped) title gives just
/// the date and time.
pub fn file_stem(started: NaiveDateTime, title: &str) -> String {
    let stamp = started.format("%Y-%m-%d %H%M").to_string();
    let title = clean_title(title);
    if title.is_empty() {
        stamp
    } else {
        format!("{stamp} {title}")
    }
}

/// `dir/stem.ext`, or `dir/stem (2).ext`, `(3)`, ... for the first name
/// `exists` does not report as taken.
pub fn unique_path(dir: &Path, stem: &str, ext: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !exists(&first) {
        return first;
    }
    let mut n = 2u32;
    loop {
        let p = dir.join(format!("{stem} ({n}).{ext}"));
        if !exists(&p) {
            return p;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 7).unwrap().and_hms_opt(9, 5, 33).unwrap()
    }

    #[test]
    fn stem_has_date_time_and_title() {
        assert_eq!(file_stem(at(), "Zoom"), "2026-11-07 0905 Zoom");
    }

    #[test]
    fn empty_title_gives_just_the_time() {
        assert_eq!(file_stem(at(), ""), "2026-11-07 0905");
        assert_eq!(file_stem(at(), "  \t "), "2026-11-07 0905");
        assert_eq!(file_stem(at(), "<>:?"), "2026-11-07 0905");
    }

    #[test]
    fn invalid_characters_are_removed() {
        assert_eq!(file_stem(at(), "a<b>c:d\"e/f\\g|h?i*j\u{7}k"), "2026-11-07 0905 abcdefghijk");
    }

    #[test]
    fn whitespace_is_collapsed_and_trimmed() {
        assert_eq!(file_stem(at(), "  My \n  call\t\tnow  "), "2026-11-07 0905 My call now");
    }

    #[test]
    fn long_titles_are_cut_to_60_characters() {
        let stem = file_stem(at(), &"x".repeat(100));
        assert_eq!(stem.chars().count(), "2026-11-07 0905 ".len() + 60);
        // Multi-byte characters count as one.
        let stem = file_stem(at(), &"é".repeat(100));
        assert_eq!(stem.chars().filter(|c| *c == 'é').count(), 60);
    }

    #[test]
    fn trailing_dots_and_spaces_are_dropped() {
        assert_eq!(file_stem(at(), "notes. . "), "2026-11-07 0905 notes");
    }

    #[test]
    fn reserved_names_get_a_trailing_underscore() {
        assert_eq!(file_stem(at(), "CON"), "2026-11-07 0905 CON_");
        assert_eq!(file_stem(at(), "nul"), "2026-11-07 0905 nul_");
        assert_eq!(file_stem(at(), "Lpt1.txt"), "2026-11-07 0905 Lpt1.txt_");
        assert_eq!(file_stem(at(), "console"), "2026-11-07 0905 console");
    }

    #[test]
    fn unique_path_adds_a_counter() {
        let dir = Path::new("D:/r");
        let none = |_: &Path| false;
        assert_eq!(unique_path(dir, "a", "m4a", none), dir.join("a.m4a"));
        let taken = [dir.join("a.m4a"), dir.join("a (2).m4a")];
        let exists = |p: &Path| taken.iter().any(|t| t == p);
        assert_eq!(unique_path(dir, "a", "m4a", exists), dir.join("a (3).m4a"));
    }
}
