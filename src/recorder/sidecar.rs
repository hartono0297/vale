//! The `<name>.vale.json` file next to a recording. Other tracks add their
//! own keys (`"transcript"`, `"cursor_log"`), so rewriting merges into the
//! existing JSON and keeps every key it does not own.
// The recorder session (T3) uses these; until it lands they look unused.
#![allow(dead_code)]

use chrono::{DateTime, FixedOffset};
use serde_json::{Value, json};

/// The fields the recorder owns.
#[derive(Clone, Debug, PartialEq)]
pub struct Sidecar {
    pub v: u32,
    /// `"audio"` or `"screen"`.
    pub kind: String,
    pub title: String,
    /// Start time with the local UTC offset (RFC 3339 in the file).
    pub started: DateTime<FixedOffset>,
    pub duration_ms: u64,
    /// Index = channel in the audio file: `["mic", "system"]`, `["mic"]`...
    pub channels: Vec<String>,
    /// The app that was using the mic, if known.
    pub app: Option<String>,
}

impl Default for Sidecar {
    fn default() -> Self {
        Self {
            v: 1,
            kind: "audio".into(),
            title: String::new(),
            started: DateTime::UNIX_EPOCH.fixed_offset(),
            duration_ms: 0,
            channels: Vec::new(),
            app: None,
        }
    }
}

impl Sidecar {
    /// The recorder's keys as a JSON object.
    pub fn to_value(&self) -> Value {
        json!({
            "v": self.v,
            "kind": self.kind,
            "title": self.title,
            "started": self.started.to_rfc3339(),
            "duration_ms": self.duration_ms,
            "channels": self.channels,
            "app": self.app,
        })
    }

    /// Reads a sidecar leniently: a missing or wrong-typed field gives its
    /// default, and anything that is not an object gives all defaults.
    pub fn from_value(value: &Value) -> Self {
        let d = Self::default();
        let str_of = |k: &str| value.get(k).and_then(Value::as_str);
        Self {
            v: value.get("v").and_then(Value::as_u64).map_or(d.v, |n| n as u32),
            kind: str_of("kind").map_or(d.kind, str::to_string),
            title: str_of("title").map_or(d.title, str::to_string),
            started: str_of("started")
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .unwrap_or(d.started),
            duration_ms: value.get("duration_ms").and_then(Value::as_u64).unwrap_or(0),
            channels: value
                .get("channels")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
            app: str_of("app").map(str::to_string),
        }
    }

    /// Writes the recorder's keys into `existing`, overwriting them and
    /// keeping every other key. A non-object `existing` is replaced.
    pub fn merge_into(&self, existing: Value) -> Value {
        let mut base = match existing {
            Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        if let Value::Object(own) = self.to_value() {
            for (k, v) in own {
                base.insert(k, v);
            }
        }
        Value::Object(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sidecar {
        Sidecar {
            v: 1,
            kind: "audio".into(),
            title: "Zoom".into(),
            started: DateTime::parse_from_rfc3339("2026-11-07T09:05:33+07:00").unwrap(),
            duration_ms: 61_500,
            channels: vec!["mic".into(), "system".into()],
            app: Some("Zoom.exe".into()),
        }
    }

    #[test]
    fn round_trips() {
        let s = sample();
        assert_eq!(Sidecar::from_value(&s.to_value()), s);
        let none = Sidecar { app: None, ..sample() };
        let v = none.to_value();
        assert!(v["app"].is_null());
        assert_eq!(Sidecar::from_value(&v), none);
    }

    #[test]
    fn started_is_rfc3339_with_the_offset() {
        assert_eq!(sample().to_value()["started"], "2026-11-07T09:05:33+07:00");
    }

    #[test]
    fn merge_keeps_unknown_keys_and_overwrites_known_ones() {
        let existing = json!({
            "v": 0, "title": "old", "duration_ms": 1,
            "transcript": {"lang": "id"}, "cursor_log": "a.cursor.json"
        });
        let merged = sample().merge_into(existing);
        assert_eq!(merged["transcript"], json!({"lang": "id"}));
        assert_eq!(merged["cursor_log"], "a.cursor.json");
        assert_eq!(merged["title"], "Zoom");
        assert_eq!(merged["v"], 1);
        assert_eq!(merged["duration_ms"], 61_500);
        assert_eq!(merged["channels"], json!(["mic", "system"]));
    }

    #[test]
    fn merge_into_a_non_object_starts_fresh() {
        let merged = sample().merge_into(json!([1, 2]));
        assert_eq!(Sidecar::from_value(&merged), sample());
    }

    #[test]
    fn reading_is_lenient() {
        let d = Sidecar::default();
        assert_eq!(Sidecar::from_value(&json!({})), d);
        assert_eq!(Sidecar::from_value(&json!(null)), d);
        assert_eq!(Sidecar::from_value(&json!("x")), d);
        let odd = json!({
            "v": "one", "kind": 3, "title": null, "started": "not a date",
            "duration_ms": -5, "channels": ["mic", 7, "system"], "app": 9
        });
        let s = Sidecar::from_value(&odd);
        assert_eq!(s, Sidecar { channels: vec!["mic".into(), "system".into()], ..d });
    }
}
