//! The recorder's settings, kept in config.json under `recording`. Read
//! leniently like `widgets`: a missing key or a wrong type gives that
//! field's default, a non-object gives all of them, and the rest of the file
//! always loads. The screen recording's two settings (`video_folder`,
//! `video_fps`) live in the same section.

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
    /// The endpoint id the mic records from; `None` follows the Windows
    /// default. It stays when the device was missing and the default took
    /// over for one recording.
    pub mic_device: Option<String>,
    /// The same for the PC audio (an output endpoint, recorded in loopback).
    pub system_device: Option<String>,
    /// The mic stream asks Windows for echo cancellation (the speech
    /// category), so speaker sound the mic picks up is removed. On by default.
    pub echo_cancel: bool,
    /// Where screen recordings go; `None` is `Videos\Vale`.
    pub video_folder: Option<PathBuf>,
    /// The screen recording's frame rate: 30 or 60.
    pub video_fps: u32,
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
            mic_device: None,
            system_device: None,
            echo_cancel: true,
            video_folder: None,
            video_fps: 30,
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
            folder: folder(map.get("folder")),
            consent_shown: map
                .get("consent_shown")
                .and_then(Value::as_bool)
                .unwrap_or(d.consent_shown),
            mic_device: device(map.get("mic_device")),
            system_device: device(map.get("system_device")),
            echo_cancel: map
                .get("echo_cancel")
                .and_then(Value::as_bool)
                .unwrap_or(d.echo_cancel),
            video_folder: folder(map.get("video_folder")),
            video_fps: match map.get("video_fps").and_then(Value::as_u64) {
                Some(60) => 60,
                _ => d.video_fps,
            },
        }
    }

    /// The endpoint chosen for `source`.
    pub fn device(&self, source: crate::recorder::Source) -> Option<&str> {
        match source {
            crate::recorder::Source::Mic => self.mic_device.as_deref(),
            crate::recorder::Source::System => self.system_device.as_deref(),
        }
    }

    /// Chooses the endpoint for `source` (`None`: follow Windows).
    pub fn set_device(&mut self, source: crate::recorder::Source, id: Option<String>) {
        match source {
            crate::recorder::Source::Mic => self.mic_device = id,
            crate::recorder::Source::System => self.system_device = id,
        }
    }

    /// The folder recordings go to now.
    pub fn dir(&self) -> PathBuf {
        self.folder.clone().unwrap_or_else(default_folder)
    }

    /// The folder screen recordings go to now.
    pub fn video_dir(&self) -> PathBuf {
        self.video_folder.clone().unwrap_or_else(default_video_folder)
    }
}

/// A folder from JSON: an absolute path, anything else is the default one.
fn folder(v: Option<&Value>) -> Option<PathBuf> {
    v.and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// A device id from JSON: a non-empty string, anything else is no choice.
fn device(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
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

/// `Videos\Vale`; next to the exe's current directory when Windows cannot say
/// where Videos is.
pub fn default_video_folder() -> PathBuf {
    use windows::Win32::UI::Shell::FOLDERID_Videos;
    let base = known_folder(&FOLDERID_Videos).unwrap_or_else(|| PathBuf::from("."));
    base.join("Vale")
}

/// `%USERPROFILE%\Vale\Video`: where a screen recording goes when Windows
/// will not let Vale write in the chosen folder (see [`fallback_folder`]).
pub fn video_fallback_folder() -> Option<PathBuf> {
    use windows::Win32::UI::Shell::FOLDERID_Profile;
    known_folder(&FOLDERID_Profile).map(|p| p.join("Vale").join("Video"))
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
        let mut map = s.serialize_map(Some(8))?;
        map.serialize_entry(
            "sources",
            &serde_json::json!({"mic": self.sources.mic, "system": self.sources.system}),
        )?;
        map.serialize_entry("folder", &self.folder.as_ref().map(|p| p.to_string_lossy()))?;
        map.serialize_entry("consent_shown", &self.consent_shown)?;
        map.serialize_entry("mic_device", &self.mic_device)?;
        map.serialize_entry("system_device", &self.system_device)?;
        map.serialize_entry("echo_cancel", &self.echo_cancel)?;
        map.serialize_entry(
            "video_folder",
            &self.video_folder.as_ref().map(|p| p.to_string_lossy()),
        )?;
        map.serialize_entry("video_fps", &self.video_fps)?;
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
            mic_device: Some("{0.0.1.00000000}.{abc}".into()),
            system_device: Some("{0.0.0.00000000}.{def}".into()),
            echo_cancel: false,
            ..RecordingPrefs::default()
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(RecordingPrefs::from_json(&v), p);
        let none = serde_json::to_value(RecordingPrefs::default()).unwrap();
        assert_eq!(none["folder"], json!(null));
        assert_eq!(none["mic_device"], json!(null));
        assert_eq!(none["system_device"], json!(null));
    }

    #[test]
    fn the_device_choices_read_alone_and_follow_windows_when_missing_or_wrong() {
        let d = RecordingPrefs::default();
        assert_eq!((d.mic_device.clone(), d.system_device.clone()), (None, None));
        // An old config has neither key.
        let p = RecordingPrefs::from_json(&json!({"consent_shown": true}));
        assert_eq!((p.mic_device, p.system_device), (None, None));
        // A wrong type or an empty id is no choice, and the other field is read alone.
        let p = RecordingPrefs::from_json(&json!({"mic_device": 4, "system_device": "{id}"}));
        assert_eq!(p.mic_device, None);
        assert_eq!(p.system_device.as_deref(), Some("{id}"));
        let p = RecordingPrefs::from_json(&json!({"mic_device": "  ", "system_device": null}));
        assert_eq!((p.mic_device, p.system_device), (None, None));
        // The accessors pick the field of the source.
        let mut p = RecordingPrefs::default();
        p.set_device(crate::recorder::Source::System, Some("{x}".into()));
        assert_eq!(p.device(crate::recorder::Source::System), Some("{x}"));
        assert_eq!(p.device(crate::recorder::Source::Mic), None);
        p.set_device(crate::recorder::Source::System, None);
        assert_eq!(p.system_device, None);
    }

    #[test]
    fn echo_cancel_is_on_by_default_and_read_alone() {
        assert!(RecordingPrefs::default().echo_cancel);
        // An old config has no key.
        assert!(RecordingPrefs::from_json(&json!({"consent_shown": true})).echo_cancel);
        // A wrong type keeps the default; a real flag is kept.
        assert!(RecordingPrefs::from_json(&json!({"echo_cancel": "no"})).echo_cancel);
        let p = RecordingPrefs::from_json(&json!({"echo_cancel": false, "consent_shown": true}));
        assert!(!p.echo_cancel && p.consent_shown);
        let v = serde_json::to_value(RecordingPrefs::default()).unwrap();
        assert_eq!(v["echo_cancel"], json!(true));
    }

    #[test]
    fn video_defaults() {
        let d = RecordingPrefs::default();
        assert_eq!(d.video_folder, None);
        assert_eq!(d.video_fps, 30);
        assert_eq!(d.video_dir(), default_video_folder());
    }

    #[test]
    fn a_bad_video_setting_reads_as_its_default() {
        for bad in [json!({"video_fps": 45}), json!({"video_fps": "60"})] {
            assert_eq!(RecordingPrefs::from_json(&bad).video_fps, 30, "{bad}");
        }
        let p = RecordingPrefs::from_json(&json!({"video_folder": "rel\\x"}));
        assert_eq!(p.video_folder, None);
        let p = RecordingPrefs::from_json(&json!({"video_folder": 7, "video_fps": 60}));
        assert_eq!(p.video_folder, None, "a wrong-typed folder reads as the default");
        assert_eq!(p.video_fps, 60, "and the other key is read alone");
    }

    #[test]
    fn video_prefs_round_trip() {
        let p = RecordingPrefs {
            video_folder: Some(PathBuf::from(r"C:\v")),
            video_fps: 60,
            ..RecordingPrefs::default()
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["video_fps"], json!(60));
        assert_eq!(v["video_folder"], json!(r"C:\v"));
        assert_eq!(RecordingPrefs::from_json(&v), p);
        assert_eq!(p.video_dir(), PathBuf::from(r"C:\v"));
        let none = serde_json::to_value(RecordingPrefs::default()).unwrap();
        assert_eq!(none["video_folder"], json!(null));
        assert_eq!(none["video_fps"], json!(30));
    }

    #[test]
    fn an_old_recording_section_loads_with_video_defaults() {
        let p = RecordingPrefs::from_json(&json!({
            "sources": {"mic": true, "system": false},
            "folder": r"D:\Audio",
            "consent_shown": true,
            "mic_device": "{id}",
            "system_device": null,
            "echo_cancel": false
        }));
        assert_eq!(p.folder, Some(PathBuf::from(r"D:\Audio")));
        assert!(p.consent_shown && !p.echo_cancel);
        assert_eq!(p.video_folder, None);
        assert_eq!(p.video_fps, 30);
    }

    #[test]
    fn the_video_fallback_sits_in_the_profile() {
        let f = video_fallback_folder().unwrap();
        assert!(f.ends_with(r"Vale\Video"), "{}", f.display());
        assert!(f.is_absolute());
        let videos = default_video_folder();
        assert!(videos.ends_with("Vale"), "{}", videos.display());
        assert!(!f.starts_with(videos.parent().unwrap()), "not under Videos: {}", f.display());
    }
}
