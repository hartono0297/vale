//! The recorder's settings, kept in config.json under `recording`. Read
//! leniently like `widgets`: a missing key or a wrong type gives that
//! field's default, a non-object gives all of them, and the rest of the file
//! always loads. The screen recording's settings (`video_fps`,
//! `video_quality`, `cursor_highlight`, `click_ripples`) live in the same
//! section. `folder` is the one recordings root: audio goes to its `Audio`
//! subfolder and screen recordings to its `Video` subfolder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::model::RecSources;

/// The subfolder of the root that audio recordings go to.
pub const AUDIO_SUB: &str = "Audio";
/// The subfolder of the root that screen recordings go to.
pub const VIDEO_SUB: &str = "Video";

/// How large a screen recording is: the shorter side of the picture is capped
/// at this many pixels, a smaller screen is never scaled up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VideoQuality {
    /// The screen's own size.
    #[default]
    Original,
    P1440,
    P1080,
    P720,
}

impl VideoQuality {
    pub const ALL: [VideoQuality; 4] = [
        VideoQuality::Original,
        VideoQuality::P1440,
        VideoQuality::P1080,
        VideoQuality::P720,
    ];

    /// The cap on the picture's shorter side; `None` keeps the screen's size.
    pub fn max_short(self) -> Option<u32> {
        match self {
            VideoQuality::Original => None,
            VideoQuality::P1440 => Some(1440),
            VideoQuality::P1080 => Some(1080),
            VideoQuality::P720 => Some(720),
        }
    }

    /// The value in config.json.
    pub fn key(self) -> &'static str {
        match self {
            VideoQuality::Original => "original",
            VideoQuality::P1440 => "1440p",
            VideoQuality::P1080 => "1080p",
            VideoQuality::P720 => "720p",
        }
    }

    pub fn from_key(s: &str) -> Option<VideoQuality> {
        VideoQuality::ALL.into_iter().find(|q| q.key() == s)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingPrefs {
    /// What the next recording takes (the Rekam page and Settings).
    pub sources: RecSources,
    /// The recordings root, with an `Audio` and a `Video` subfolder; `None` is
    /// `Documents\Vale\Rekaman`.
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
    /// Legacy: where screen recordings used to go. Only the one-time migration
    /// reads it; it is `None` after that.
    pub video_folder: Option<PathBuf>,
    /// The screen recording's frame rate: 30 or 60.
    pub video_fps: u32,
    /// The screen recording's size cap.
    pub video_quality: VideoQuality,
    /// A soft yellow circle around the cursor in a screen recording.
    pub cursor_highlight: bool,
    /// A ring that spreads from every click in a screen recording.
    pub click_ripples: bool,
    /// The one-time move into the `Audio` and `Video` subfolders has run.
    pub folders_split: bool,
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
            video_quality: VideoQuality::Original,
            cursor_highlight: true,
            click_ripples: true,
            folders_split: false,
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
            video_quality: map
                .get("video_quality")
                .and_then(Value::as_str)
                .and_then(VideoQuality::from_key)
                .unwrap_or_default(),
            cursor_highlight: map
                .get("cursor_highlight")
                .and_then(Value::as_bool)
                .unwrap_or(d.cursor_highlight),
            click_ripples: map
                .get("click_ripples")
                .and_then(Value::as_bool)
                .unwrap_or(d.click_ripples),
            folders_split: map
                .get("folders_split")
                .and_then(Value::as_bool)
                .unwrap_or(d.folders_split),
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

    /// The recordings root.
    pub fn root(&self) -> PathBuf {
        self.folder.clone().unwrap_or_else(default_folder)
    }

    /// The folder audio recordings go to now.
    pub fn dir(&self) -> PathBuf {
        self.root().join(AUDIO_SUB)
    }

    /// The folder screen recordings go to now.
    pub fn video_dir(&self) -> PathBuf {
        self.root().join(VIDEO_SUB)
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

/// `<fallback root>\Audio`.
pub fn fallback_audio_dir() -> Option<PathBuf> {
    fallback_folder().map(|p| p.join(AUDIO_SUB))
}

/// `<fallback root>\Video`.
pub fn fallback_video_dir() -> Option<PathBuf> {
    fallback_folder().map(|p| p.join(VIDEO_SUB))
}

/// `Videos\Vale`: where screen recordings went before the one root; next to
/// the exe's current directory when Windows cannot say where Videos is.
pub fn legacy_video_default() -> PathBuf {
    use windows::Win32::UI::Shell::FOLDERID_Videos;
    let base = known_folder(&FOLDERID_Videos).unwrap_or_else(|| PathBuf::from("."));
    base.join("Vale")
}

/// `%USERPROFILE%\Vale\Video`: where a screen recording went before the one
/// root when Windows would not let Vale write in `Videos`.
pub fn legacy_video_fallback() -> Option<PathBuf> {
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

/// The folders the migration compares the old settings against.
#[derive(Clone, Debug)]
pub struct Known {
    /// The default root, `Documents\Vale\Rekaman`.
    pub default_root: PathBuf,
    /// The fallback root, `%USERPROFILE%\Vale\Rekaman`.
    pub fallback_root: Option<PathBuf>,
    /// The old screen recording default, `Videos\Vale`.
    pub video_default: PathBuf,
    /// The old screen recording fallback, `%USERPROFILE%\Vale\Video`.
    pub video_fallback: Option<PathBuf>,
}

impl Known {
    /// The real folders of this PC.
    pub fn system() -> Known {
        Known {
            default_root: default_folder(),
            fallback_root: fallback_folder(),
            video_default: legacy_video_default(),
            video_fallback: legacy_video_fallback(),
        }
    }
}

/// One folder of recordings to rename into a subfolder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: PathBuf,
    pub to: PathBuf,
    /// The extension of the recordings in `from` (`m4a` or `mp4`).
    pub ext: &'static str,
}

/// What the one-time migration does: the root to store (`None` keeps the
/// default) and the folders to move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitPlan {
    pub root: Option<PathBuf>,
    pub moves: Vec<Move>,
}

/// Works out the migration from the old `folder` and `video_folder`. The root
/// is the old folder; else the fallback root when the old video folder was the
/// video fallback (Windows is known to block the default); else the default.
/// Audio in the root moves to `Audio`. Screen recordings move to `Video` only
/// from the old video folder when it was unset, the old default or the old
/// fallback; a folder the user picked stays where it is.
pub fn split_plan(
    folder: Option<&Path>,
    video_folder: Option<&Path>,
    known: &Known,
) -> SplitPlan {
    use crate::transcribe::queue::same_audio;
    let video_was_fallback = match (video_folder, known.video_fallback.as_deref()) {
        (Some(v), Some(f)) => same_audio(v, f),
        _ => false,
    };
    let root = match folder {
        Some(f) => Some(f.to_path_buf()),
        None if video_was_fallback => known.fallback_root.clone(),
        None => None,
    };
    let eff = root.clone().unwrap_or_else(|| known.default_root.clone());
    let mut moves = vec![Move {
        from: eff.clone(),
        to: eff.join(AUDIO_SUB),
        ext: "m4a",
    }];
    let video_from = match video_folder {
        None => Some(known.video_default.clone()),
        Some(v) if video_was_fallback || same_audio(v, &known.video_default) => {
            Some(v.to_path_buf())
        }
        Some(_) => None,
    };
    if let Some(from) = video_from {
        moves.push(Move {
            from,
            to: eff.join(VIDEO_SUB),
            ext: "mp4",
        });
    }
    SplitPlan { root, moves }
}

/// How a folder is named in Settings: its parent and itself.
pub fn folder_label(dir: &Path) -> String {
    crate::screenshot::folder_label(dir)
}

impl Serialize for RecordingPrefs {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(12))?;
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
        map.serialize_entry("video_quality", self.video_quality.key())?;
        map.serialize_entry("cursor_highlight", &self.cursor_highlight)?;
        map.serialize_entry("click_ripples", &self.click_ripples)?;
        map.serialize_entry("folders_split", &self.folders_split)?;
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
        assert!(d.root().ends_with(r"Vale\Rekaman"));
        assert!(d.dir().ends_with(r"Vale\Rekaman\Audio"));
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
        assert_eq!(p.dir(), PathBuf::from(r"D:\Audio\Audio"));
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
    fn the_cursor_effects_are_on_by_default_and_read_alone() {
        let d = RecordingPrefs::default();
        assert!(d.cursor_highlight && d.click_ripples);
        let p = RecordingPrefs::from_json(&json!({"cursor_highlight": false, "click_ripples": false}));
        assert!(!p.cursor_highlight && !p.click_ripples);
        let p = RecordingPrefs::from_json(&json!({"cursor_highlight": "no", "click_ripples": 1}));
        assert!(p.cursor_highlight && p.click_ripples, "a wrong type reads as the default");
        let p = RecordingPrefs::from_json(&json!({"click_ripples": false}));
        assert!(p.cursor_highlight && !p.click_ripples);
        let off = RecordingPrefs {
            cursor_highlight: false,
            click_ripples: false,
            ..RecordingPrefs::default()
        };
        let v = serde_json::to_value(&off).unwrap();
        assert_eq!(v["cursor_highlight"], json!(false));
        assert_eq!(RecordingPrefs::from_json(&v), off);
    }

    #[test]
    fn video_defaults() {
        let d = RecordingPrefs::default();
        assert_eq!(d.video_folder, None);
        assert_eq!(d.video_fps, 30);
        assert!(d.video_dir().ends_with(r"Vale\Rekaman\Video"));
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
    fn video_quality_reads_leniently_and_round_trips() {
        for (v, q) in [
            (json!("original"), VideoQuality::Original),
            (json!("1440p"), VideoQuality::P1440),
            (json!("1080p"), VideoQuality::P1080),
            (json!("720p"), VideoQuality::P720),
            (json!("4k"), VideoQuality::Original),
            (json!(1080), VideoQuality::Original),
            (json!(null), VideoQuality::Original),
        ] {
            let p = RecordingPrefs::from_json(&json!({"video_quality": v, "video_fps": 60}));
            assert_eq!(p.video_quality, q);
            assert_eq!(p.video_fps, 60, "the other keys still load");
        }
        assert_eq!(RecordingPrefs::from_json(&json!({})).video_quality, VideoQuality::Original);
        for q in VideoQuality::ALL {
            let p = RecordingPrefs { video_quality: q, ..RecordingPrefs::default() };
            let back: RecordingPrefs = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
            assert_eq!(back, p);
        }
        assert_eq!(VideoQuality::ALL.map(VideoQuality::max_short), [None, Some(1440), Some(1080), Some(720)]);
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
    fn the_legacy_video_folders_and_the_fallback_dirs() {
        let f = legacy_video_fallback().unwrap();
        assert!(f.ends_with(r"Vale\Video"), "{}", f.display());
        let videos = legacy_video_default();
        assert!(videos.ends_with("Vale"), "{}", videos.display());
        assert!(!f.starts_with(videos.parent().unwrap()), "not under Videos: {}", f.display());
        assert!(fallback_audio_dir().unwrap().ends_with(r"Vale\Rekaman\Audio"));
        assert!(fallback_video_dir().unwrap().ends_with(r"Vale\Rekaman\Video"));
    }

    fn known() -> Known {
        Known {
            default_root: PathBuf::from(r"C:\U\Documents\Vale\Rekaman"),
            fallback_root: Some(PathBuf::from(r"C:\U\Vale\Rekaman")),
            video_default: PathBuf::from(r"C:\U\Videos\Vale"),
            video_fallback: Some(PathBuf::from(r"C:\U\Vale\Video")),
        }
    }

    fn mv(from: &str, to: &str, ext: &'static str) -> Move {
        Move { from: PathBuf::from(from), to: PathBuf::from(to), ext }
    }

    #[test]
    fn dirs_are_subfolders_of_one_root() {
        let d = RecordingPrefs::default();
        assert!(d.dir().ends_with(r"Vale\Rekaman\Audio"));
        assert!(d.video_dir().ends_with(r"Vale\Rekaman\Video"));
        let p = RecordingPrefs { folder: Some(PathBuf::from(r"D:\R")), ..d };
        assert_eq!(p.dir(), PathBuf::from(r"D:\R\Audio"));
        assert_eq!(p.video_dir(), PathBuf::from(r"D:\R\Video"));
    }

    #[test]
    fn fresh_install_plans_default_root() {
        let plan = split_plan(None, None, &known());
        assert_eq!(plan.root, None);
        assert_eq!(
            plan.moves,
            vec![
                mv(r"C:\U\Documents\Vale\Rekaman", r"C:\U\Documents\Vale\Rekaman\Audio", "m4a"),
                mv(r"C:\U\Videos\Vale", r"C:\U\Documents\Vale\Rekaman\Video", "mp4"),
            ]
        );
    }

    #[test]
    fn fallback_paths_keep_the_fallback_root() {
        let plan = split_plan(
            Some(Path::new(r"C:\U\Vale\Rekaman")),
            Some(Path::new(r"C:\U\Vale\Video")),
            &known(),
        );
        assert_eq!(plan.root, Some(PathBuf::from(r"C:\U\Vale\Rekaman")));
        assert_eq!(
            plan.moves,
            vec![
                mv(r"C:\U\Vale\Rekaman", r"C:\U\Vale\Rekaman\Audio", "m4a"),
                mv(r"C:\U\Vale\Video", r"C:\U\Vale\Rekaman\Video", "mp4"),
            ]
        );
    }

    #[test]
    fn blocked_video_only_moves_the_root_to_the_fallback() {
        let plan = split_plan(None, Some(Path::new(r"C:\U\Vale\Video")), &known());
        assert_eq!(plan.root, Some(PathBuf::from(r"C:\U\Vale\Rekaman")));
        assert!(plan.moves.contains(&mv(r"C:\U\Vale\Video", r"C:\U\Vale\Rekaman\Video", "mp4")));
    }

    #[test]
    fn a_picked_video_folder_is_left_alone() {
        let plan = split_plan(Some(Path::new(r"D:\R")), Some(Path::new(r"E:\Vid")), &known());
        assert_eq!(plan.root, Some(PathBuf::from(r"D:\R")));
        assert_eq!(plan.moves, vec![mv(r"D:\R", r"D:\R\Audio", "m4a")]);
    }

    #[test]
    fn case_differences_still_match_the_legacy_paths() {
        let plan = split_plan(None, Some(Path::new(r"c:\u\VALE\video")), &known());
        assert_eq!(plan.root, Some(PathBuf::from(r"C:\U\Vale\Rekaman")));
        let plan = split_plan(None, Some(Path::new(r"c:\u\videos\vale")), &known());
        assert_eq!(plan.root, None);
        assert_eq!(plan.moves.len(), 2);
    }

    #[test]
    fn folders_split_reads_leniently() {
        let get = |v: Value| RecordingPrefs::from_json(&v).folders_split;
        assert!(!get(json!({})));
        assert!(!get(json!({"folders_split": 7})));
        assert!(get(json!({"folders_split": true})));
        let p = RecordingPrefs { folders_split: true, ..RecordingPrefs::default() };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["folders_split"], json!(true));
        assert_eq!(RecordingPrefs::from_json(&v), p);
    }
}
