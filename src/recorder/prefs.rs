//! The recorder's settings, kept in config.json under `recording`. Read
//! leniently like `widgets`: a missing key or a wrong type gives that
//! field's default, a non-object gives all of them, and the rest of the file
//! always loads.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::model::RecSources;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingPrefs {
    /// What the next recording takes (the Rekam page and Settings).
    pub sources: RecSources,
    /// Where recordings go; `None` is `Documents\Vale\Rekaman`.
    pub folder: Option<PathBuf>,
    /// The "ask everyone first" notice was shown once.
    pub consent_shown: bool,
}

impl Default for RecordingPrefs {
    fn default() -> Self {
        RecordingPrefs {
            sources: RecSources {
                mic: true,
                system: true,
            },
            folder: None,
            consent_shown: false,
        }
    }
}

impl RecordingPrefs {
    /// Every field read on its own from a JSON value. A source choice with
    /// neither source on reads as both.
    pub fn from_json(v: &Value) -> RecordingPrefs {
        let d = RecordingPrefs::default();
        let Some(map) = v.as_object() else {
            return d;
        };
        let flag = |src: &Value, key: &str| src.get(key).and_then(Value::as_bool);
        let sources = match map.get("sources") {
            Some(s) => {
                let r = RecSources {
                    mic: flag(s, "mic").unwrap_or(d.sources.mic),
                    system: flag(s, "system").unwrap_or(d.sources.system),
                };
                if r.mic || r.system { r } else { d.sources }
            }
            None => d.sources,
        };
        RecordingPrefs {
            sources,
            folder: map
                .get("folder")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
            consent_shown: map
                .get("consent_shown")
                .and_then(Value::as_bool)
                .unwrap_or(d.consent_shown),
        }
    }

    /// The folder recordings go to now.
    pub fn dir(&self) -> PathBuf {
        self.folder.clone().unwrap_or_else(default_folder)
    }
}

/// `Documents\Vale\Rekaman`; next to the exe's current directory when Windows
/// cannot say where Documents is.
pub fn default_folder() -> PathBuf {
    use windows::Win32::UI::Shell::FOLDERID_Documents;
    let base = known_folder(&FOLDERID_Documents).unwrap_or_else(|| PathBuf::from("."));
    base.join("Vale").join("Rekaman")
}

/// `%USERPROFILE%\Vale\Rekaman`: where a recording goes when Windows will not
/// let Vale write in the chosen folder. Controlled folder access guards
/// Documents, Pictures, Videos, Music, Desktop and Favorites by default,
/// never the profile folder itself.
pub fn fallback_folder() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::FOLDERID_Profile;
    known_folder(&FOLDERID_Profile).map(|p| p.join("Vale").join("Rekaman"))
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    unsafe {
        let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = raw.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(raw.0 as *const _));
        path.filter(|p| p.is_absolute())
    }
}

/// How a folder is named in Settings: its parent and itself.
pub fn folder_label(dir: &Path) -> String {
    crate::screenshot::folder_label(dir)
}

impl Serialize for RecordingPrefs {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(3))?;
        map.serialize_entry(
            "sources",
            &serde_json::json!({"mic": self.sources.mic, "system": self.sources.system}),
        )?;
        map.serialize_entry("folder", &self.folder.as_ref().map(|p| p.to_string_lossy()))?;
        map.serialize_entry("consent_shown", &self.consent_shown)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for RecordingPrefs {
    /// Never an error: see the module note.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(RecordingPrefs::from_json(&Value::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_are_both_sources_the_documents_folder_and_no_consent_yet() {
        let d = RecordingPrefs::default();
        assert!(d.sources.mic && d.sources.system);
        assert_eq!(d.folder, None);
        assert!(!d.consent_shown);
        assert!(d.dir().ends_with(r"Vale\Rekaman"));
    }

    #[test]
    fn the_fallback_is_vale_rekaman_in_the_profile_folder_not_in_documents() {
        let f = fallback_folder().expect("the profile folder is known");
        assert!(f.ends_with(r"Vale\Rekaman"), "{}", f.display());
        assert!(f.is_absolute());
        let docs = default_folder();
        let docs_root = docs.parent().and_then(Path::parent).unwrap();
        assert!(!f.starts_with(docs_root), "not under Documents: {}", f.display());
    }

    #[test]
    fn a_non_object_gives_every_default_and_fields_read_alone() {
        for bad in [json!(5), json!("x"), json!(null), json!([1])] {
            assert_eq!(RecordingPrefs::from_json(&bad), RecordingPrefs::default());
        }
        let p = RecordingPrefs::from_json(&json!({
            "sources": {"mic": false}, "folder": 7, "consent_shown": true
        }));
        assert!(!p.sources.mic && p.sources.system, "a missing flag keeps its default");
        assert_eq!(p.folder, None, "a wrong-typed folder reads as the default");
        assert!(p.consent_shown);
    }

    #[test]
    fn no_source_reads_as_both_and_a_relative_folder_is_refused() {
        let p = RecordingPrefs::from_json(&json!({
            "sources": {"mic": false, "system": false}, "folder": "rekaman"
        }));
        assert_eq!(p.sources, RecordingPrefs::default().sources);
        assert_eq!(p.folder, None);
        let p = RecordingPrefs::from_json(&json!({"folder": r"D:\Audio"}));
        assert_eq!(p.folder, Some(PathBuf::from(r"D:\Audio")));
        assert_eq!(p.dir(), PathBuf::from(r"D:\Audio"));
    }

    #[test]
    fn it_round_trips_through_json() {
        let p = RecordingPrefs {
            sources: RecSources {
                mic: true,
                system: false,
            },
            folder: Some(PathBuf::from(r"E:\Rec")),
            consent_shown: true,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(RecordingPrefs::from_json(&v), p);
        let none = serde_json::to_value(RecordingPrefs::default()).unwrap();
        assert_eq!(none["folder"], json!(null));
    }
}
