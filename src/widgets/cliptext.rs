//! The text a new sticky note starts with when "Isi dari clipboard" is on, and
//! the clean-up a dropped text goes through. Pure and tested without a window
//! or the real clipboard (clipboard-privacy: no test reads it).
//!
//! The first line of the result is the note's title, since the checklist
//! rules already treat the first line as the title.

use crate::clipboard::kinds::should_skip;
use crate::clipboard::store::ClipSnapshot;

/// The longest text a prefill (or one drop's block) puts in a note, in chars.
pub const CLIP_MAX: usize = 2000;

/// A cut at a line break is looked for in this many chars before the cap.
const CUT_WINDOW: usize = 200;

/// A tab becomes this many spaces.
const TAB: &str = "    ";

/// The most blank lines in a row that are kept.
const MAX_BLANK_RUN: usize = 2;

/// `raw` cleaned for a note, without the cap: CRLF and CR are LF, control
/// characters are gone (a tab is four spaces), every line loses its trailing
/// whitespace, blank lines at either end and the outer whitespace go, and a
/// run of more than two blank lines is two.
pub fn normalize(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                text.push('\n');
            }
            '\n' => text.push('\n'),
            '\t' => text.push_str(TAB),
            c if c.is_control() => {}
            c => text.push(c),
        }
    }
    let mut out: Vec<&str> = Vec::new();
    let mut blank_run = 0;
    for line in text.split('\n') {
        let line = line.trim_end();
        if line.is_empty() {
            blank_run += 1;
            // Blank lines before the first text are dropped as they come.
            if out.is_empty() || blank_run > MAX_BLANK_RUN {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push(line);
    }
    out.join("\n").trim().to_owned()
}

/// `text` cut to `CLIP_MAX` chars: at the last line break in the final
/// `CUT_WINDOW` chars before the cap when there is one, else at the cap
/// (always a char boundary). No ellipsis. Trailing whitespace is trimmed.
pub fn cap(text: &str) -> String {
    match text.char_indices().nth(CLIP_MAX) {
        Some((end, _)) => cut_before(text, end),
        None => text.to_owned(),
    }
}

/// `text` cut at or before byte `end` (a char boundary), the rule `cap`
/// uses: at the last line break in the final `CUT_WINDOW` chars before `end`
/// when there is one, else at `end`. Trailing whitespace is trimmed.
pub fn cut_before(text: &str, end: usize) -> String {
    let head = &text[..end];
    let window_start = head
        .char_indices()
        .rev()
        .nth(CUT_WINDOW - 1)
        .map_or(0, |(i, _)| i);
    let cut = head[window_start..]
        .rfind('\n')
        .map_or(end, |i| window_start + i);
    text[..cut].trim_end().to_owned()
}

/// What a new note starts with for the clipboard text `raw`, or `None` when
/// nothing is left after the clean-up.
pub fn prefill(raw: &str) -> Option<String> {
    let text = cap(&normalize(raw));
    (!text.is_empty()).then_some(text)
}

/// The prefill for what `win::read` found. The excluded formats (a password
/// manager's copy) give nothing, and so do files and pictures: only Unicode
/// text starts a note.
pub fn from_snapshot(snap: &ClipSnapshot) -> Option<String> {
    if should_skip(&snap.formats) || !snap.files.is_empty() {
        return None;
    }
    prefill(snap.text.as_deref()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn line_breaks_become_lf() {
        assert_eq!(prefill("a\r\nb\rc\nd").as_deref(), Some("a\nb\nc\nd"));
        assert_eq!(prefill("a\r\n\r\nb").as_deref(), Some("a\n\nb"));
    }

    #[test]
    fn control_characters_go_and_a_tab_is_four_spaces() {
        assert_eq!(
            prefill("a\u{0}b\u{7}c\u{1b}[0m\u{7f}d").as_deref(),
            Some("abc[0md")
        );
        assert_eq!(prefill("x\n\ty").as_deref(), Some("x\n    y"));
        assert_eq!(prefill("a\tb").as_deref(), Some("a    b"));
    }

    #[test]
    fn trailing_whitespace_and_outer_blank_lines_are_trimmed() {
        assert_eq!(prefill("  \n\n  hi  \nthere \t \n\n \n").as_deref(), Some("hi\nthere"));
        // Indentation inside the text stays.
        assert_eq!(prefill("a\n  b").as_deref(), Some("a\n  b"));
    }

    #[test]
    fn runs_of_blank_lines_collapse_to_two() {
        assert_eq!(prefill("a\n\n\n\n\nb").as_deref(), Some("a\n\n\nb"));
        assert_eq!(prefill("a\n\n\nb").as_deref(), Some("a\n\n\nb"));
        assert_eq!(prefill("a\n\nb").as_deref(), Some("a\n\nb"));
        // A line of spaces counts as blank.
        assert_eq!(prefill("a\n \n\t\n  \n\nb").as_deref(), Some("a\n\n\nb"));
    }

    #[test]
    fn empty_after_cleaning_is_none() {
        for raw in ["", "   ", "\r\n\r\n", "\t\n \u{0}\u{7}", "\n\n\n"] {
            assert_eq!(prefill(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn the_demo_sample_is_an_address_and_a_line() {
        assert_eq!(
            prefill("Jl. Kenanga No. 12, Bandung 40115\r\nKirim sebelum Jumat\r\n\r\n\r\n\r\n  ").as_deref(),
            Some("Jl. Kenanga No. 12, Bandung 40115\nKirim sebelum Jumat")
        );
    }

    #[test]
    fn text_up_to_the_cap_is_kept_whole() {
        let t = "a".repeat(CLIP_MAX);
        assert_eq!(prefill(&t).as_deref(), Some(t.as_str()));
    }

    #[test]
    fn the_cap_cuts_at_the_last_line_break_of_the_final_200_chars() {
        // Lines of 99 chars + LF = 100 per line; 25 lines is 2500 chars.
        let line = "x".repeat(99);
        let raw = vec![line.as_str(); 25].join("\n");
        let got = prefill(&raw).unwrap();
        // The window is chars 1800..2000, whose last LF is at index 1999
        // (the 20th line ends there): 20 whole lines, no trailing LF.
        assert_eq!(got, vec![line.as_str(); 20].join("\n"));
        assert!(got.chars().count() <= CLIP_MAX);
        assert!(!got.ends_with('\n') && !got.ends_with('…'));
    }

    #[test]
    fn the_cap_without_a_line_break_in_the_window_cuts_at_the_cap() {
        // A line break exists, but before the window (index 100).
        let raw = format!("{}\n{}", "a".repeat(100), "b".repeat(3000));
        let got = prefill(&raw).unwrap();
        assert_eq!(got.chars().count(), CLIP_MAX);
        assert!(got.starts_with(&"a".repeat(100)));
        assert!(got.ends_with('b'));
    }

    #[test]
    fn the_cap_cuts_on_a_char_boundary_with_emoji() {
        // Each emoji is four bytes (two UTF-16 units) and one char.
        let raw = "\u{1F600}".repeat(CLIP_MAX + 50);
        let got = prefill(&raw).unwrap();
        assert_eq!(got.chars().count(), CLIP_MAX);
        assert!(got.chars().all(|c| c == '\u{1F600}'));
        // And with a line break in the window, after multi-byte chars.
        let raw = format!(
            "{}\n{}",
            "\u{00e9}".repeat(1900),
            "\u{1F600}".repeat(400)
        );
        let got = prefill(&raw).unwrap();
        assert_eq!(got, "\u{00e9}".repeat(1900));
    }

    #[test]
    fn the_cut_text_loses_its_trailing_whitespace() {
        let raw = format!("{}   \n{}", "a".repeat(1950), "b".repeat(500));
        let got = prefill(&raw).unwrap();
        assert_eq!(got, "a".repeat(1950));
    }

    fn snap(text: Option<&str>) -> ClipSnapshot {
        ClipSnapshot {
            text: text.map(str::to_owned),
            ..ClipSnapshot::default()
        }
    }

    #[test]
    fn a_snapshot_gives_its_cleaned_text() {
        assert_eq!(from_snapshot(&snap(Some("hello\r\n"))).as_deref(), Some("hello"));
        assert_eq!(from_snapshot(&snap(Some("  \n"))), None);
        assert_eq!(from_snapshot(&snap(None)), None, "an image or nothing");
    }

    #[test]
    fn files_and_excluded_copies_give_no_prefill() {
        let mut s = snap(Some("secret"));
        s.files = vec![PathBuf::from("C:\\a.txt")];
        assert_eq!(from_snapshot(&s), None);
        for formats in [
            vec![("ExcludeClipboardContentFromMonitorProcessing".to_owned(), None)],
            vec![("Clipboard Viewer Ignore".to_owned(), None)],
            vec![("CanIncludeInClipboardHistory".to_owned(), Some(0))],
            vec![("CanUploadToCloudClipboard".to_owned(), Some(0))],
            vec![("CanIncludeInClipboardHistory".to_owned(), None)],
        ] {
            let mut s = snap(Some("secret"));
            s.formats = formats.clone();
            assert_eq!(from_snapshot(&s), None, "{formats:?}");
        }
        // A DWORD of 1 does not exclude.
        let mut s = snap(Some("ok"));
        s.formats = vec![("CanIncludeInClipboardHistory".to_owned(), Some(1))];
        assert_eq!(from_snapshot(&s).as_deref(), Some("ok"));
    }
}
