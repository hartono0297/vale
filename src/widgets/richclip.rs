//! The note's own clipboard format: what a copy from a sticky note leaves next
//! to the plain text so that a paste into another note keeps the formatting.
//! Pure (the clipboard calls are in `host`); a paste from anywhere else never
//! brings fonts or colours, it is plain text in the style at the caret.
//!
//! The format is registered as `Vale.NoteRich` and holds UTF-8 JSON:
//! `{"v":1,"text":"..","runs":[{"s":0,"e":3,"b":true},..]}`, the runs relative
//! to the copied text. It is only used when its text is the very text the
//! clipboard also holds as CF_UNICODETEXT: if another app replaced the text
//! afterwards but left ours behind, the runs are ignored.

use serde_json::{Value, json};

use crate::widgets::checklist;
use crate::widgets::styles::{self, Run};

/// The registered clipboard format's name.
pub const FORMAT_NAME: &str = "Vale.NoteRich";
/// A payload bigger than this is not read.
pub const MAX_BYTES: usize = 1024 * 1024;

/// The payload for `text` (model text, `\n` breaks) and its `runs`
/// (relative to it).
pub fn encode(text: &str, runs: &[Run]) -> Vec<u8> {
    let runs = serde_json::to_value(runs).unwrap_or_else(|_| json!([]));
    serde_json::to_vec(&json!({ "v": 1, "text": text, "runs": runs })).unwrap_or_default()
}

/// The runs of a payload, when it is well formed (version 1, under 1 MiB) and
/// its text equals `clip_text` (the clipboard's plain text; `\r\n` and `\r`
/// count as `\n` on both sides). The runs are normalised against the text.
pub fn decode(bytes: &[u8], clip_text: &str) -> Option<Vec<Run>> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return None;
    }
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let map = v.as_object()?;
    if map.get("v")?.as_u64()? != 1 {
        return None;
    }
    let text = checklist::normalize(map.get("text")?.as_str()?);
    if text != checklist::normalize(clip_text) {
        return None;
    }
    let runs = styles::runs_from_value(map.get("runs")?);
    Some(styles::normalise(&runs, &text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::styles::{Fg, Hl, Style};

    fn bold(s: usize, e: usize) -> Run {
        Run { s, e, style: Style { b: true, ..Style::default() } }
    }

    #[test]
    fn a_payload_round_trips() {
        let runs = vec![
            bold(0, 3),
            Run {
                s: 4,
                e: 6,
                style: Style { hl: Some(Hl::Pink), fg: Some(Fg::Biru), ..Style::default() },
            },
        ];
        let bytes = encode("abc de", &runs);
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["v"], 1);
        assert_eq!(json["text"], "abc de");
        assert_eq!(decode(&bytes, "abc de"), Some(runs));
    }

    #[test]
    fn the_clipboard_text_must_be_the_same_text() {
        let bytes = encode("abc", &[bold(0, 3)]);
        assert!(decode(&bytes, "abc").is_some());
        assert_eq!(decode(&bytes, "abd"), None, "another app changed the text");
        assert_eq!(decode(&bytes, ""), None);
        // CRLF on the clipboard side is the same text.
        let bytes = encode("a\nb", &[bold(0, 3)]);
        assert_eq!(decode(&bytes, "a\r\nb"), Some(vec![bold(0, 3)]));
        assert_eq!(decode(&bytes, "a\rb"), Some(vec![bold(0, 3)]));
    }

    #[test]
    fn a_bad_payload_is_nothing() {
        assert_eq!(decode(b"", "x"), None);
        assert_eq!(decode(b"not json", "x"), None);
        assert_eq!(decode(b"[]", "x"), None);
        assert_eq!(decode(br#"{"v":2,"text":"x","runs":[]}"#, "x"), None, "wrong version");
        assert_eq!(decode(br#"{"text":"x","runs":[]}"#, "x"), None, "no version");
        assert_eq!(decode(br#"{"v":1,"runs":[]}"#, "x"), None, "no text");
        assert_eq!(decode(br#"{"v":1,"text":"x"}"#, "x"), None, "no runs");
        assert_eq!(decode(br#"{"v":1,"text":5,"runs":[]}"#, "x"), None);
        assert_eq!(decode(&[0xff, 0xfe, 0x00], "x"), None, "not UTF-8");
    }

    #[test]
    fn a_payload_over_one_mebibyte_is_refused() {
        let big = format!(
            r#"{{"v":1,"text":"x","runs":[],"pad":"{}"}}"#,
            "a".repeat(MAX_BYTES)
        );
        assert!(big.len() > MAX_BYTES);
        assert_eq!(decode(big.as_bytes(), "x"), None);
        let ok = format!(r#"{{"v":1,"text":"x","runs":[],"pad":"{}"}}"#, "a".repeat(1000));
        assert_eq!(decode(ok.as_bytes(), "x"), Some(vec![]));
    }

    #[test]
    fn runs_are_normalised_against_the_text() {
        // Out of range, empty, overlapping and plain runs are cleaned.
        let payload = br#"{"v":1,"text":"abcdef","runs":[
            {"s":0,"e":3,"b":true},{"s":2,"e":99,"i":true},{"s":4,"e":4,"b":true},
            {"s":1,"e":2},{"s":"x","e":2,"b":true},{"s":5,"e":6,"hl":"nope","u":true}]}"#;
        let runs = decode(payload, "abcdef").unwrap();
        assert_eq!(runs.len(), 3);
        assert_eq!((runs[0].s, runs[0].e), (0, 2));
        assert!(runs[0].style.b && !runs[0].style.i);
        assert_eq!((runs[1].s, runs[1].e), (2, 5));
        assert!(runs[1].style.i);
        assert_eq!((runs[2].s, runs[2].e), (5, 6));
    }

    #[test]
    fn emoji_text_survives_and_runs_snap_off_the_pair() {
        let text = "a\u{1F600}b";
        let bytes = encode(text, &[bold(1, 3)]);
        assert_eq!(decode(&bytes, text), Some(vec![bold(1, 3)]));
        let payload = format!(r#"{{"v":1,"text":"{text}","runs":[{{"s":0,"e":2,"b":true}}]}}"#);
        assert_eq!(decode(payload.as_bytes(), text), Some(vec![bold(0, 1)]));
    }
}
