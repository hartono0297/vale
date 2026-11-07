//! The files of a screen recording: the video's name (the contract in the
//! recording spec: `YYYY-MM-DD HHmm <title>.mp4`, ` (2)` on a clash) and the
//! `.vale.json` sidecar with its `"cursor_log"` file name.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// `YYYY-MM-DD HHmm` plus ` <title>` when there is one. Characters Windows
/// refuses in a file name become spaces; the title is cut at 60 characters.
pub fn file_stem(local: chrono::NaiveDateTime, title: &str) -> String {
    let date = local.format("%Y-%m-%d %H%M").to_string();
    let clean: String = title
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .take(60)
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = clean.trim_end_matches('.');
    if clean.is_empty() {
        date
    } else {
        format!("{date} {clean}")
    }
}

/// `dir\<stem>.<ext>`, or `dir\<stem> (2).<ext>` and up while `taken` says
/// the name is in use.
pub fn unique_path(dir: &Path, stem: &str, ext: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !taken(p))
        .unwrap_or(first)
}

/// The sidecar next to `video`: same name, `.vale.json`.
pub fn sidecar_path(video: &Path) -> PathBuf {
    video.with_extension("vale.json")
}

/// The cursor log next to `video`.
pub fn cursor_log_path(video: &Path) -> PathBuf {
    video.with_extension(super::cursor_log::EXTENSION)
}

/// What the screen recorder knows about a finished recording.
#[derive(Clone, Debug, PartialEq)]
pub struct SidecarInfo {
    pub title: String,
    /// RFC 3339 local time.
    pub started: String,
    pub duration_ms: u64,
    /// Index = audio channel ("mic", "system"); empty without audio.
    pub channels: Vec<String>,
    pub app: Option<String>,
    /// The cursor log's file name (no folder), if one was written.
    pub cursor_log: Option<String>,
}

/// The sidecar's JSON: `existing` (a sidecar already there) with the keys
/// this recorder owns set, every other key kept as it was.
pub fn sidecar_json(existing: Option<&str>, info: &SidecarInfo) -> String {
    let mut map: Map<String, Value> = existing
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();
    map.insert("v".into(), 1.into());
    map.insert("kind".into(), "screen".into());
    map.insert("title".into(), info.title.clone().into());
    map.insert("started".into(), info.started.clone().into());
    map.insert("duration_ms".into(), info.duration_ms.into());
    map.insert(
        "channels".into(),
        Value::Array(info.channels.iter().cloned().map(Value::from).collect()),
    );
    map.insert(
        "app".into(),
        info.app.clone().map_or(Value::Null, Value::from),
    );
    match &info.cursor_log {
        Some(name) => {
            map.insert("cursor_log".into(), name.clone().into());
        }
        None => {
            map.remove("cursor_log");
        }
    }
    serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
}

/// Writes (or rewrites) the sidecar of `video`.
pub fn write_sidecar(video: &Path, info: &SidecarInfo) -> std::io::Result<()> {
    let path = sidecar_path(video);
    let existing = std::fs::read_to_string(&path).ok();
    std::fs::write(&path, sidecar_json(existing.as_deref(), info))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u32, m: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 11, 7)
            .unwrap()
            .and_hms_opt(h, m, 5)
            .unwrap()
    }

    #[test]
    fn the_stem_is_the_date_and_the_title() {
        assert_eq!(file_stem(at(14, 30), ""), "2026-11-07 1430");
        assert_eq!(file_stem(at(9, 5), "Zoom"), "2026-11-07 0905 Zoom");
        assert_eq!(
            file_stem(at(9, 5), "a/b: c?  d."),
            "2026-11-07 0905 a b c d"
        );
        assert_eq!(file_stem(at(9, 5), "  :: "), "2026-11-07 0905");
    }

    #[test]
    fn a_clash_adds_a_number() {
        let dir = Path::new(r"C:\v");
        let taken = [
            dir.join("2026-11-07 1430.mp4"),
            dir.join("2026-11-07 1430 (2).mp4"),
        ];
        let p = unique_path(dir, "2026-11-07 1430", "mp4", |p| {
            taken.iter().any(|t| t == p)
        });
        assert_eq!(p, dir.join("2026-11-07 1430 (3).mp4"));
        let p = unique_path(dir, "x", "mp4", |_| false);
        assert_eq!(p, dir.join("x.mp4"));
    }

    #[test]
    fn the_side_files_share_the_name() {
        let v = Path::new(r"C:\v\2026-11-07 1430 Zoom.mp4");
        assert_eq!(
            sidecar_path(v),
            Path::new(r"C:\v\2026-11-07 1430 Zoom.vale.json")
        );
        assert_eq!(
            cursor_log_path(v),
            Path::new(r"C:\v\2026-11-07 1430 Zoom.cursor.jsonl")
        );
    }

    fn info() -> SidecarInfo {
        SidecarInfo {
            title: String::new(),
            started: "2026-11-07T14:30:05+07:00".into(),
            duration_ms: 4200,
            channels: vec!["mic".into(), "system".into()],
            app: None,
            cursor_log: Some("2026-11-07 1430.cursor.jsonl".into()),
        }
    }

    #[test]
    fn the_sidecar_has_the_contract_keys() {
        let v: Value = serde_json::from_str(&sidecar_json(None, &info())).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["kind"], "screen");
        assert_eq!(v["duration_ms"], 4200);
        assert_eq!(v["channels"][1], "system");
        assert_eq!(v["app"], Value::Null);
        assert_eq!(v["cursor_log"], "2026-11-07 1430.cursor.jsonl");
    }

    #[test]
    fn a_rewrite_keeps_unknown_keys() {
        let old = r#"{"v":1,"kind":"screen","transcript":{"model":"x"},"duration_ms":1}"#;
        let mut i = info();
        i.cursor_log = None;
        let v: Value = serde_json::from_str(&sidecar_json(Some(old), &i)).unwrap();
        assert_eq!(v["transcript"]["model"], "x");
        assert_eq!(v["duration_ms"], 4200);
        assert!(v.get("cursor_log").is_none());
        // A broken file is replaced.
        let v: Value = serde_json::from_str(&sidecar_json(Some("{oops"), &i)).unwrap();
        assert_eq!(v["kind"], "screen");
    }
}
