//! The caption settings, kept in config.json under `captions`. Read leniently
//! like `TranscriptPrefs`: a missing key or a wrong type gives that field's
//! default, a non-object gives all of them, and the rest of the file always
//! loads. Owned by the island (`config::merge_owned`).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::config::{CardPlace, card_place_from_json};
use crate::transcribe::prefs::TrLang;

/// How big the caption text is (JSON `"s"`, `"m"`, `"l"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSize {
    Small,
    Medium,
    Large,
}

impl TextSize {
    /// The body text height in logical pixels.
    pub fn body_px(self) -> f32 {
        match self {
            TextSize::Small => 15.0,
            TextSize::Medium => 18.0,
            TextSize::Large => 22.0,
        }
    }

    /// One size bigger (`up`) or smaller, stopping at the ends.
    pub fn step(self, up: bool) -> TextSize {
        match (self, up) {
            (TextSize::Small, true) => TextSize::Medium,
            (TextSize::Medium, true) => TextSize::Large,
            (TextSize::Large, false) => TextSize::Medium,
            (TextSize::Medium, false) => TextSize::Small,
            (s, _) => s,
        }
    }

    fn json_key(self) -> &'static str {
        match self {
            TextSize::Small => "s",
            TextSize::Medium => "m",
            TextSize::Large => "l",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptionPrefs {
    /// Caption the owner's own speech too, shown as "Me".
    pub mic: bool,
    /// Show the last sentence in the notch.
    pub notch: bool,
    /// Keep captioning while on battery.
    pub on_battery: bool,
    /// The caption language; `None` follows the Transcripts language.
    pub language: Option<TrLang>,
    pub size: TextSize,
    /// Where the caption window was last (screen coordinates).
    pub place: Option<CardPlace>,
}

impl Default for CaptionPrefs {
    fn default() -> Self {
        CaptionPrefs {
            mic: false,
            notch: false,
            on_battery: false,
            language: None,
            size: TextSize::Medium,
            place: None,
        }
    }
}

impl CaptionPrefs {
    /// Every field read on its own from a JSON value.
    pub fn from_json(v: &Value) -> CaptionPrefs {
        let d = CaptionPrefs::default();
        let Some(map) = v.as_object() else {
            return d;
        };
        let flag =
            |key: &str, default: bool| map.get(key).and_then(Value::as_bool).unwrap_or(default);
        CaptionPrefs {
            mic: flag("mic", d.mic),
            notch: flag("notch", d.notch),
            on_battery: flag("on_battery", d.on_battery),
            language: match map.get("language").and_then(Value::as_str) {
                Some("id") => Some(TrLang::Id),
                Some("en") => Some(TrLang::En),
                Some("auto") => Some(TrLang::Auto),
                _ => None,
            },
            size: match map.get("size").and_then(Value::as_str) {
                Some("s") => TextSize::Small,
                Some("l") => TextSize::Large,
                _ => TextSize::Medium,
            },
            place: map.get("place").cloned().and_then(card_place_from_json),
        }
    }
}

impl Serialize for CaptionPrefs {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let language = self.language.map(|l| match l {
            TrLang::Id => "id",
            TrLang::En => "en",
            TrLang::Auto => "auto",
        });
        let mut map = s.serialize_map(Some(6))?;
        map.serialize_entry("mic", &self.mic)?;
        map.serialize_entry("notch", &self.notch)?;
        map.serialize_entry("on_battery", &self.on_battery)?;
        map.serialize_entry("language", &language)?;
        map.serialize_entry("size", self.size.json_key())?;
        map.serialize_entry("place", &self.place)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for CaptionPrefs {
    /// Never an error: see the module note.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(CaptionPrefs::from_json(&Value::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_are_all_off_and_medium() {
        let d = CaptionPrefs::default();
        assert!(!d.mic && !d.notch && !d.on_battery);
        assert_eq!((d.language, d.size, d.place), (None, TextSize::Medium, None));
        for bad in [json!("x"), json!(7), json!(null)] {
            assert_eq!(CaptionPrefs::from_json(&bad), d, "{bad}");
        }
    }

    #[test]
    fn round_trips() {
        let p = CaptionPrefs {
            mic: true,
            notch: true,
            on_battery: true,
            language: Some(TrLang::Auto),
            size: TextSize::Large,
            place: Some(CardPlace {
                left: 10,
                top: 20,
                right: 400,
                bottom: 300,
            }),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(CaptionPrefs::from_json(&v), p);
        assert_eq!(v["language"], json!("auto"));
        assert_eq!(v["size"], json!("l"));
        let v = serde_json::to_value(CaptionPrefs::default()).unwrap();
        assert_eq!(v["language"], json!(null));
        assert_eq!(CaptionPrefs::from_json(&v), CaptionPrefs::default());
        for l in [TrLang::Id, TrLang::En] {
            let p = CaptionPrefs {
                language: Some(l),
                ..CaptionPrefs::default()
            };
            let v = serde_json::to_value(&p).unwrap();
            assert_eq!(CaptionPrefs::from_json(&v), p);
        }
    }

    #[test]
    fn a_bad_field_falls_back_alone() {
        let p = CaptionPrefs::from_json(&json!({"mic":"yes","size":"xl","language":"id"}));
        assert!(!p.mic);
        assert_eq!(p.size, TextSize::Medium);
        assert_eq!(p.language, Some(TrLang::Id));
    }

    #[test]
    fn size_steps_clamp() {
        assert_eq!(TextSize::Small.step(false), TextSize::Small);
        assert_eq!(TextSize::Small.step(true), TextSize::Medium);
        assert_eq!(TextSize::Medium.step(true), TextSize::Large);
        assert_eq!(TextSize::Large.step(true), TextSize::Large);
        assert_eq!(TextSize::Large.step(false), TextSize::Medium);
        assert_eq!(TextSize::Medium.step(false), TextSize::Small);
        assert_eq!(TextSize::Small.body_px(), 15.0);
        assert_eq!(TextSize::Medium.body_px(), 18.0);
        assert_eq!(TextSize::Large.body_px(), 22.0);
    }
}
