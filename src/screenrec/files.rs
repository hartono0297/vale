//! The files of a screen recording: the video (`<stem>.mp4`, written as
//! `<stem>.mp4.part` until it is finished), the `.vale.json` sidecar with its
//! `"cursor_log"` file name and the cursor log. The stem is
//! [`crate::recorder::naming::file_stem`]; a clash adds ` (2)`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// The four files one recording named `stem` uses in `dir`: the video, the
/// video while it is written, the sidecar and the cursor log.
pub fn files_of(dir: &Path, stem: &str) -> [PathBuf; 4] {
    [
        dir.join(format!("{stem}.mp4")),
        dir.join(format!("{stem}.mp4.part")),
        dir.join(format!("{stem}.vale.json")),
        dir.join(format!("{stem}.{}", super::cursor_log::EXTENSION)),
    ]
}

/// `stem`, or `stem (2)`, `(3)`, ... for the first stem none of whose files
/// ([`files_of`]) `exists` reports as taken: a sidecar or an unfinished
/// `.part` left by another recording claims its name too.
pub fn unique_stem(dir: &Path, stem: &str, exists: impl Fn(&Path) -> bool) -> String {
    let free = |s: &str| !files_of(dir, s).iter().any(|p| exists(p));
    if free(stem) {
        return stem.to_string();
    }
    (2u32..)
        .map(|n| format!("{stem} ({n})"))
        .find(|s| free(s))
        .unwrap_or_else(|| stem.to_string())
}

/// The file the video is written to until it is finished: `<video>.part`.
pub fn part_of(video: &Path) -> PathBuf {
    let mut name = video.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Those of the four files of `video` that exist, the `.part` included.
pub fn existing_files(video: &Path) -> Vec<PathBuf> {
    let dir = video.parent().unwrap_or(Path::new(""));
    let stem = video
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    files_of(dir, &stem)
        .into_iter()
        .filter(|p| p.exists())
        .collect()
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

    #[test]
    fn the_files_of_a_video_are_four() {
        let f = files_of(Path::new(r"C:\v"), "2026-11-11 1430");
        assert_eq!(
            f,
            [
                PathBuf::from(r"C:\v\2026-11-11 1430.mp4"),
                PathBuf::from(r"C:\v\2026-11-11 1430.mp4.part"),
                PathBuf::from(r"C:\v\2026-11-11 1430.vale.json"),
                PathBuf::from(r"C:\v\2026-11-11 1430.cursor.jsonl"),
            ]
        );
    }

    #[test]
    fn a_clash_with_any_file_takes_the_next_number() {
        let dir = Path::new(r"C:\v");
        let only = |name: &'static str| move |p: &Path| p == dir.join(name);
        assert_eq!(unique_stem(dir, "x", |_| false), "x");
        assert_eq!(unique_stem(dir, "x", only("x.mp4.part")), "x (2)");
        assert_eq!(unique_stem(dir, "x", only("x.cursor.jsonl")), "x (2)");
        let taken = ["x.mp4", "x (2).vale.json"];
        let both = |p: &Path| taken.iter().any(|t| p == dir.join(t));
        assert_eq!(unique_stem(dir, "x", both), "x (3)");
    }

    #[test]
    fn existing_files_lists_only_what_is_there() {
        let dir = std::env::temp_dir()
            .join("vale-test-files")
            .join(format!("{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("x.mp4");
        std::fs::write(&video, b"v").unwrap();
        std::fs::write(dir.join("x.vale.json"), b"{}").unwrap();
        std::fs::write(dir.join("x.txt"), b"t").unwrap();
        assert_eq!(
            existing_files(&video),
            vec![video.clone(), dir.join("x.vale.json")]
        );
        std::fs::write(dir.join("x.mp4.part"), b"p").unwrap();
        assert_eq!(existing_files(&video).len(), 3);
        assert_eq!(part_of(&video), dir.join("x.mp4.part"));
        let _ = std::fs::remove_dir_all(&dir);
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
    fn the_sidecar_names_the_mix_channel() {
        let mut i = info();
        i.channels = vec!["mix".into()];
        i.cursor_log = Some("a.cursor.jsonl".into());
        let old = r#"{"transcript":{"model":"x"}}"#;
        let v: Value = serde_json::from_str(&sidecar_json(Some(old), &i)).unwrap();
        assert_eq!(v["kind"], "screen");
        assert_eq!(v["channels"], serde_json::json!(["mix"]));
        assert_eq!(v["cursor_log"], "a.cursor.jsonl");
        assert_eq!(v["transcript"]["model"], "x");
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
