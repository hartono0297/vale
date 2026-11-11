//! The transcript settings, kept in config.json under `transcript`. Read
//! leniently like `RecordingPrefs`: a missing key or a wrong type gives that
//! field's default, a non-object gives all of them, and the rest of the file
//! always loads. Owned by the island (`config::merge_owned`).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::config::{CardPlace, card_place_from_json};
use crate::transcribe::catalog::{self, Model};

/// The language a transcript is written in (JSON `"id"`, `"en"`, `"auto"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrLang {
    Id,
    En,
    Auto,
}

impl TrLang {
    /// The engine's language code; `None` lets the engine detect it.
    pub fn code(self) -> Option<&'static str> {
        match self {
            TrLang::Id => Some("id"),
            TrLang::En => Some("en"),
            TrLang::Auto => None,
        }
    }

    /// The JSON value in config.json.
    fn json_key(self) -> &'static str {
        match self {
            TrLang::Id => "id",
            TrLang::En => "en",
            TrLang::Auto => "auto",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptPrefs {
    /// Transkrip (Settings): transcribe the recordings made from now on.
    pub enabled: bool,
    /// The model key (`catalog::MODELS`); `None` or an unknown key uses the
    /// default for this PC (see `model_for`).
    pub model: Option<String>,
    pub language: TrLang,
    /// Transcribe each recording when it stops.
    pub auto: bool,
    /// The one-time offer to turn the feature on was shown.
    pub setup_offered: bool,
    /// Where the transcript window was last (screen coordinates).
    pub window: Option<CardPlace>,
}

impl Default for TranscriptPrefs {
    fn default() -> Self {
        TranscriptPrefs {
            enabled: false,
            model: None,
            language: TrLang::Id,
            auto: true,
            setup_offered: false,
            window: None,
        }
    }
}

impl TranscriptPrefs {
    /// Every field read on its own from a JSON value.
    pub fn from_json(v: &Value) -> TranscriptPrefs {
        let d = TranscriptPrefs::default();
        let Some(map) = v.as_object() else {
            return d;
        };
        let flag =
            |key: &str, default: bool| map.get(key).and_then(Value::as_bool).unwrap_or(default);
        TranscriptPrefs {
            enabled: flag("enabled", d.enabled),
            model: map
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            language: match map.get("language").and_then(Value::as_str) {
                Some("en") => TrLang::En,
                Some("auto") => TrLang::Auto,
                _ => TrLang::Id,
            },
            auto: flag("auto", d.auto),
            setup_offered: flag("setup_offered", d.setup_offered),
            window: map.get("window").cloned().and_then(card_place_from_json),
        }
    }

    /// The model to use on this PC: the chosen key when the catalog has it,
    /// else the default for `gpu`.
    pub fn model_for(&self, gpu: bool) -> &'static Model {
        self.model
            .as_deref()
            .and_then(catalog::model)
            .unwrap_or_else(|| catalog::default_model(gpu))
    }
}

impl Serialize for TranscriptPrefs {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(6))?;
        map.serialize_entry("enabled", &self.enabled)?;
        map.serialize_entry("model", &self.model)?;
        map.serialize_entry("language", self.language.json_key())?;
        map.serialize_entry("auto", &self.auto)?;
        map.serialize_entry("setup_offered", &self.setup_offered)?;
        map.serialize_entry("window", &self.window)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for TranscriptPrefs {
    /// Never an error: see the module note.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(TranscriptPrefs::from_json(&Value::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::catalog::MODELS;
    use serde_json::json;

    #[test]
    fn transcript_defaults() {
        let d = TranscriptPrefs::default();
        assert!(!d.enabled, "off until the owner turns it on");
        assert!(d.auto, "auto is on");
        assert_eq!(d.language, TrLang::Id, "Indonesian");
        assert!(!d.setup_offered, "nothing offered yet");
        assert_eq!((d.model.clone(), d.window), (None, None));
    }

    #[test]
    fn a_bad_transcript_section_reads_as_defaults() {
        for bad in [json!("x"), json!(7), json!(null)] {
            assert_eq!(
                TranscriptPrefs::from_json(&bad),
                TranscriptPrefs::default(),
                "{bad}"
            );
        }
        let p = TranscriptPrefs::from_json(&json!({"enabled": "yes", "language": "xx", "auto": 3}));
        assert_eq!(p, TranscriptPrefs::default());
    }

    #[test]
    fn an_old_model_key_falls_back_to_the_default() {
        let p = TranscriptPrefs::from_json(&json!({"model": "qwen3-asr-0.6b"}));
        assert_eq!(p.model_for(true).key, "whisper-large-v3-turbo");
        assert_eq!(p.model_for(false).key, "whisper-small");
        let p = TranscriptPrefs::from_json(&json!({"model": "whisper-small"}));
        assert_eq!(
            p.model_for(true).key,
            "whisper-small",
            "a known key is kept"
        );
        assert_eq!(
            TranscriptPrefs::default().model_for(true).key,
            MODELS[0].key
        );
    }

    #[test]
    fn transcript_prefs_round_trip() {
        let p = TranscriptPrefs {
            enabled: true,
            model: Some("whisper-small".into()),
            language: TrLang::Auto,
            auto: false,
            setup_offered: true,
            window: Some(CardPlace {
                left: 10,
                top: 20,
                right: 400,
                bottom: 300,
            }),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(TranscriptPrefs::from_json(&v), p);
        assert_eq!(v["language"], json!("auto"));
        let v = serde_json::to_value(TranscriptPrefs::default()).unwrap();
        assert_eq!(v["language"], json!("id"));
        assert_eq!(v["model"], json!(null));
        assert_eq!(v["window"], json!(null));
        assert_eq!(TrLang::Id.code(), Some("id"));
        assert_eq!(TrLang::En.code(), Some("en"));
        assert_eq!(TrLang::Auto.code(), None);
    }
}
