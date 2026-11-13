//! Export and import of the general settings.
//!
//! The file holds only the preferences that make sense on any PC and say
//! nothing private. Everything else stays out of the file, and an import keeps
//! the target's own value for it. Every `Config` key is classified in
//! `EXPORTED` or `LEFT_OUT`; a test fails when a new key is in neither, so
//! nobody adds a setting without deciding where it goes.

use crate::config::{self, Config};
use crate::i18n::Lang;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime};
use serde_json::{Map, Value};
use std::io;
use std::path::{Path, PathBuf};

/// The flag of the restart after an import: the new Vale waits for the old
/// island's mutex like after an update and says the settings were imported.
pub const IMPORTED_FLAG: &str = "--imported";

/// Starts `exe` as `vale --imported`.
pub fn launch_imported(exe: &Path) -> io::Result<()> {
    std::process::Command::new(exe)
        .arg(IMPORTED_FLAG)
        .spawn()
        .map(|_| ())
}

/// The file format version this build writes and reads.
pub const FORMAT: u64 = 1;
/// A settings file is a few KB; anything bigger is not one.
pub const MAX_BYTES: usize = 1_048_576;

/// Top-level keys that go in the file. The split objects (`recording`,
/// `transcript`, `captions`, `arrange`) and `shortcut_tiles` are filtered
/// further below.
pub const EXPORTED: &[&str] = &[
    "style",
    "language",
    "accent",
    "surface",
    "font",
    "window_theme",
    "notch_width",
    "notch_height",
    "notch_fill",
    "notch_opacity",
    "idle_clock",
    "stay_behind",
    "open_mode",
    "gestures",
    "scroll_volume",
    "lyrics",
    "eq_follow_audio",
    "notch_peek",
    "robot",
    "robot_animation",
    "robot_sleep_minutes",
    "morning_greeting",
    "mascot",
    "cat_color",
    "falk_color",
    "octopus_color",
    "cat_accessory",
    "cat_follow_pointer",
    "cat_dance",
    "start_with_windows",
    "battery_threshold",
    "disk_warn_gb",
    "ram_warn_percent",
    "update_check",
    "device_notices",
    "pomodoro",
    "clipboard",
    "screenshot",
    "widgets",
    "hotkeys",
    "panel_pages",
    "cli_shell",
    "cli_start",
    "clean_mode",
    "mole_protect_defaults",
    "notifications",
    "phone_net",
    "shortcut_tiles",
    "arrange",
    "recording",
    "transcript",
    "captions",
];

/// Top-level keys that never go in the file: private, or fitting one PC only.
/// Only the classification test reads it.
#[allow(dead_code)]
pub const LEFT_OUT: &[&str] = &[
    "google_client_id",
    "google_client_secret",
    "muted_apps",
    "notch_monitor",
    "settings_window",
    "lyrics_card",
    "onboarding_done",
    "last_greeting",
    "update_offered",
    "mole_defaults_added",
    "media_log",
];

pub const RECORDING_KEYS: &[&str] = &[
    "sources",
    "echo_cancel",
    "video_fps",
    "video_quality",
    "cursor_highlight",
    "click_ripples",
];
pub const TRANSCRIPT_KEYS: &[&str] = &["enabled", "model", "language", "auto"];
pub const CAPTIONS_KEYS: &[&str] = &["mic", "notch", "on_battery", "language", "size"];
pub const ARRANGE_KEYS: &[&str] = &["gap", "templates"];

/// The exported sub-keys of a split object, `None` for a plain key.
fn split_keys(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "recording" => Some(RECORDING_KEYS),
        "transcript" => Some(TRANSCRIPT_KEYS),
        "captions" => Some(CAPTIONS_KEYS),
        "arrange" => Some(ARRANGE_KEYS),
        _ => None,
    }
}

/// A tile entry that names a built-in shortcut and carries nothing that could
/// launch something: a file never adds a tile of its own.
fn is_builtin_tile(entry: &Value) -> bool {
    let Some(obj) = entry.as_object() else {
        return false;
    };
    let named = obj
        .get("tile")
        .and_then(Value::as_str)
        .is_some_and(|id| crate::tiles::shortcut_from_name(id).is_some());
    named && !obj.contains_key("kind") && !obj.contains_key("target")
}

fn builtin_tiles(list: &Value) -> Vec<Value> {
    let Some(list) = list.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter(|e| is_builtin_tile(e))
        .map(|e| {
            let mut kept = Map::new();
            kept.insert("tile".into(), e["tile"].clone());
            if let Some(on) = e.get("on") {
                kept.insert("on".into(), on.clone());
            }
            Value::Object(kept)
        })
        .collect()
}

fn config_object(cfg: &Config) -> Map<String, Value> {
    match serde_json::to_value(cfg) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// The general settings of `cfg`, as they go in the file.
pub fn general_settings(cfg: &Config) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in config_object(cfg) {
        if !EXPORTED.contains(&key.as_str()) {
            continue;
        }
        let value = match (split_keys(&key), &value) {
            (Some(keys), Value::Object(obj)) => Value::Object(
                obj.iter()
                    .filter(|(k, _)| keys.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ),
            _ if key == "shortcut_tiles" => Value::Array(builtin_tiles(&value)),
            _ => value,
        };
        out.insert(key, value);
    }
    out
}

/// The settings file for `cfg`, pretty printed so it can be read by eye.
pub fn export_json(cfg: &Config, app_version: &str, exported: DateTime<FixedOffset>) -> String {
    let mut file = Map::new();
    file.insert("vale_settings".into(), FORMAT.into());
    file.insert("app_version".into(), app_version.into());
    file.insert("exported".into(), exported.to_rfc3339().into());
    file.insert("settings".into(), Value::Object(general_settings(cfg)));
    serde_json::to_string_pretty(&Value::Object(file)).unwrap_or_default()
}

/// A checked settings file, ready to apply.
#[derive(Debug, Clone, PartialEq)]
pub struct Incoming {
    pub app_version: String,
    /// The day it was exported, in the exporter's own time zone.
    pub exported: Option<NaiveDate>,
    pub settings: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Too large, not UTF-8, not JSON, or not shaped like a settings file.
    NotVale,
    /// Written by a Vale with a newer file format.
    Newer { app_version: String },
}

/// Checks `bytes` and returns the settings in it. Never panics on any input.
pub fn read_incoming(bytes: &[u8]) -> Result<Incoming, ImportError> {
    if bytes.len() > MAX_BYTES {
        return Err(ImportError::NotVale);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ImportError::NotVale)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Ok(Value::Object(file)) = serde_json::from_str::<Value>(text) else {
        return Err(ImportError::NotVale);
    };
    let format = file
        .get("vale_settings")
        .and_then(Value::as_u64)
        .ok_or(ImportError::NotVale)?;
    let app_version = file
        .get("app_version")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    // A newer format may reshape `settings`, so the version is judged first.
    if format > FORMAT {
        return Err(ImportError::Newer { app_version });
    }
    let Some(Value::Object(settings)) = file.get("settings") else {
        return Err(ImportError::NotVale);
    };
    let exported = file
        .get("exported")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.naive_local().date());
    Ok(Incoming {
        app_version,
        exported,
        settings: settings.clone(),
    })
}

/// `{key: value}` read alone as a config, with the key as it came back. A
/// field with a lenient reader turns a malformed value into its default, so
/// the value only counts when it comes back as it went in; `None` otherwise.
fn read_back(key: &str, value: &Value) -> Option<Value> {
    let mut one = Map::new();
    one.insert(key.into(), value.clone());
    let cfg = config::try_parse(&Value::Object(one).to_string())?;
    config_object(&cfg).remove(key)
}

/// Whether a top-level `value` is a good value for `key`.
fn reads_alone(key: &str, value: &Value) -> bool {
    read_back(key, value).as_ref() == Some(value)
}

/// Whether a split object's `value` is a good value for `parent.sub`.
fn sub_reads_alone(parent: &str, sub: &str, value: &Value) -> bool {
    let mut inner = Map::new();
    inner.insert(sub.into(), value.clone());
    read_back(parent, &Value::Object(inner)).is_some_and(|back| back.get(sub) == Some(value))
}

/// Turns imported built-in tiles off from the end of the list until at most
/// `MAX_SHOWN` are shown, so the target's own tiles stay visible.
fn trim_shown_tiles(cfg: &mut Config) {
    let mut shown = cfg.shortcut_tiles.iter().filter(|t| t.on).count();
    for t in cfg.shortcut_tiles.iter_mut().rev() {
        if shown <= crate::tiles::MAX_SHOWN {
            break;
        }
        if t.on && crate::tiles::shortcut_from_name(&t.tile).is_some() {
            t.on = false;
            shown -= 1;
        }
    }
}

/// `target` with the general settings of `incoming` laid over it. Keys not in
/// `EXPORTED` are ignored and a key missing from the file keeps the target's
/// value. A value that does not read back as it was written, alone in a config, is
/// skipped too, so the target's value stays. `None` when the merged result still does not read.
pub fn apply(target: &Config, incoming: &Incoming) -> Option<Config> {
    let mut merged = config_object(target);
    for &key in EXPORTED {
        let Some(from_file) = incoming.settings.get(key) else {
            continue;
        };
        if let Some(keys) = split_keys(key) {
            // Only the listed fields of a split object; the target's folders,
            // devices, window places and saved layouts stay.
            if let (Some(Value::Object(mine)), Value::Object(theirs)) =
                (merged.get_mut(key), from_file)
            {
                for &sub in keys {
                    if let Some(v) = theirs.get(sub)
                        && sub_reads_alone(key, sub, v)
                    {
                        mine.insert(sub.into(), v.clone());
                    }
                }
            }
        } else if key == "shortcut_tiles" {
            let Some(mine) = merged.get(key).and_then(Value::as_array) else {
                continue;
            };
            if !from_file.is_array() {
                continue;
            }
            let own: Vec<Value> = mine
                .iter()
                .filter(|e| {
                    e.get("tile")
                        .and_then(Value::as_str)
                        .is_none_or(|id| crate::tiles::shortcut_from_name(id).is_none())
                })
                .cloned()
                .collect();
            let mut tiles = builtin_tiles(from_file);
            tiles.extend(own);
            merged.insert(key.into(), Value::Array(tiles));
        } else if reads_alone(key, from_file) {
            merged.insert(key.into(), from_file.clone());
        }
    }
    let mut cfg = config::try_parse(&Value::Object(merged).to_string())?;
    trim_shown_tiles(&mut cfg);
    Some(cfg)
}

/// Writes the settings file to a sibling `.tmp`, then renames it over `path`,
/// so a failed write never leaves a half file where the user chose to save.
pub fn write_export(path: &Path, cfg: &Config, now: DateTime<FixedOffset>) -> io::Result<()> {
    let json = export_json(cfg, env!("CARGO_PKG_VERSION"), now);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Copies `config_path` to `config-before-import-YYYYMMDD-HHMMSS.json` beside
/// it before an import changes it. A taken name gets `-2`, `-3`, and so on, so
/// an earlier backup is never overwritten. `None` when there is no config yet.
pub fn backup_config(config_path: &Path, now: NaiveDateTime) -> io::Result<Option<PathBuf>> {
    if !config_path.is_file() {
        return Ok(None);
    }
    let dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let stamp = now.format("%Y%m%d-%H%M%S");
    for n in 1u32.. {
        let name = if n == 1 {
            format!("config-before-import-{stamp}.json")
        } else {
            format!("config-before-import-{stamp}-{n}.json")
        };
        let dest = dir.join(name);
        if dest.exists() {
            continue;
        }
        std::fs::copy(config_path, &dest)?;
        return Ok(Some(dest));
    }
    Ok(None)
}

/// Applies `incoming` on top of what is on disk and saves it. The island
/// merges its own fields over the file on every save, so the file must already
/// hold the imported values of the file-owned fields (such as the battery
/// threshold) before the restart, or the next save would bring the old ones back.
/// Why an import stopped before it changed anything the island uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFailure {
    /// The merged settings do not read as a config.
    NotVale,
    /// The merged config could not be written.
    Save,
}

/// On `Err` the caller adopts nothing and restarts nothing.
pub fn import_and_save(
    config_path: &Path,
    mem: &Config,
    incoming: &Incoming,
) -> Result<Config, ImportFailure> {
    let truth = config::save_merged_at(config_path, mem).config;
    let new = apply(&truth, incoming).ok_or(ImportFailure::NotVale)?;
    new.save_to(config_path).map_err(|_| ImportFailure::Save)?;
    Ok(new)
}

/// The name the save dialog offers, in the UI language.
pub fn default_export_name(lang: Lang, today: NaiveDate) -> String {
    let word = match lang {
        Lang::Id => "pengaturan",
        Lang::En => "settings",
    };
    format!("Vale-{word}-{}.json", today.format("%Y-%m-%d"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(m) => m,
            _ => panic!("not an object"),
        }
    }

    fn keys(cfg: &Config) -> Vec<String> {
        config_object(cfg).keys().cloned().collect()
    }

    /// A config with a private or PC-only value in every left-out spot.
    fn private_config() -> Config {
        let mut v = Value::Object(config_object(&Config::default()));
        v["google_client_id"] = json!("id.apps");
        v["google_client_secret"] = json!("s3cret");
        v["muted_apps"] = json!(["Orca"]);
        v["onboarding_done"] = json!(true);
        v["update_offered"] = json!("9.9.9");
        v["settings_window"] =
            json!({"left":1,"top":2,"right":900,"bottom":700,"maximized":false});
        v["lyrics_card"] = json!({"left":1,"top":2,"right":300,"bottom":200});
        v["notch_monitor"] = json!({"path":"\\\\?\\DISPLAY#X","name":"Dell"});
        v["shortcut_tiles"] = json!([
            {"tile":"Lock","on":false},
            {"tile":"Custom","label":"Mine","kind":"Exe","target":"C:\\Users\\X\\a.exe","on":true}
        ]);
        v["recording"]["folder"] = json!("C:\\Users\\X\\Rek");
        v["recording"]["video_folder"] = json!("C:\\Users\\X\\Rek\\Video");
        v["recording"]["mic_device"] = json!("{mic-id}");
        v["recording"]["system_device"] = json!("{sys-id}");
        v["recording"]["consent_shown"] = json!(true);
        v["transcript"]["setup_offered"] = json!(true);
        v["transcript"]["window"] = json!({"left":5,"top":5,"right":500,"bottom":400});
        v["captions"]["place"] = json!({"left":7,"top":7,"right":600,"bottom":300});
        let saved = crate::arrange::SavedArrangement::new(
            "Kerja",
            crate::arrange::LayoutRef::BuiltIn(crate::arrange::BuiltIn::Cols2),
            crate::arrange::SavedMonitor {
                device: "\\\\.\\DISPLAY1".into(),
                rect: crate::capture::Rect::new(0, 0, 1920, 1080),
            },
            vec![
                Some(crate::arrange::SavedWindow {
                    exe: "C:\\x.exe".into(),
                    class: "Cls".into(),
                    title: "Secret title".into(),
                }),
                None,
            ],
        )
        .expect("a valid saved arrangement");
        v["arrange"]["saved"] = serde_json::to_value(vec![saved]).unwrap();
        let cfg = config::parse(&v.to_string());
        assert_eq!(cfg.google_client_secret, "s3cret");
        assert_eq!(cfg.arrange.saved.len(), 1);
        assert_eq!(cfg.shortcut_tiles.len(), 2);
        assert!(cfg.recording.folder.is_some());
        cfg
    }

    /// A config whose general settings all differ from the defaults.
    fn general_config() -> Config {
        let mut v = Value::Object(config_object(&Config::default()));
        v["accent"] = json!("Plum");
        v["notch_width"] = json!(250);
        v["mascot"] = json!("Falk");
        v["battery_threshold"] = json!(33);
        v["hotkeys"]["search"] = json!("Ctrl+Alt+B");
        v["recording"]["video_fps"] = json!(15);
        v["transcript"]["model"] = json!("some-model");
        v["captions"]["size"] = json!("l");
        v["arrange"]["gap"] = json!(17);
        v["shortcut_tiles"] = json!([{"tile":"Lock","on":false},{"tile":"Search","on":true}]);
        let cfg = config::parse(&v.to_string());
        let d = Config::default();
        assert_ne!(cfg.accent, d.accent);
        assert_eq!(cfg.notch_width, 250);
        assert_ne!(cfg.mascot, d.mascot);
        assert_eq!(cfg.battery_threshold, 33);
        assert_ne!(cfg.hotkeys, d.hotkeys);
        assert_eq!(cfg.arrange.gap, 17);
        cfg
    }

    fn file_of(cfg: &Config) -> Incoming {
        let when = DateTime::parse_from_rfc3339("2026-11-13T14:05:00+07:00").unwrap();
        read_incoming(export_json(cfg, "5.3.0", when).as_bytes()).expect("a good file")
    }

    #[test]
    fn every_config_key_is_classified() {
        for k in EXPORTED {
            assert!(!LEFT_OUT.contains(k), "{k} is in both lists");
        }
        let with_log = Config {
            media_log: true,
            ..Config::default()
        };
        for cfg in [Config::default(), with_log] {
            for k in keys(&cfg) {
                assert!(
                    EXPORTED.contains(&k.as_str()) || LEFT_OUT.contains(&k.as_str()),
                    "config key {k} is in neither EXPORTED nor LEFT_OUT"
                );
            }
        }
        // Nested names that stay with the target, next to the exported ones.
        let left_out_nested = [
            "folder",
            "video_folder",
            "mic_device",
            "system_device",
            "consent_shown",
            "folders_split",
            "setup_offered",
            "window",
            "place",
            "saved",
        ];
        let all = config_object(&private_config());
        for (parent, list) in [
            ("recording", RECORDING_KEYS),
            ("transcript", TRANSCRIPT_KEYS),
            ("captions", CAPTIONS_KEYS),
            ("arrange", ARRANGE_KEYS),
        ] {
            for k in all[parent].as_object().unwrap().keys() {
                assert!(
                    list.contains(&k.as_str()) || left_out_nested.contains(&k.as_str()),
                    "{parent}.{k} is not classified"
                );
            }
        }
    }

    #[test]
    fn export_leaves_out_private_fields() {
        let when = DateTime::parse_from_rfc3339("2026-11-13T14:05:00+07:00").unwrap();
        let text = export_json(&private_config(), "5.3.0", when);
        for secret in [
            "id.apps",
            "s3cret",
            "Orca",
            "a.exe",
            "x.exe",
            "Rek",
            "DISPLAY#X",
            "Secret title",
            "{mic-id}",
        ] {
            assert!(!text.contains(secret), "export leaks {secret}");
        }
        assert!(text.contains("\"vale_settings\": 1"));
    }

    #[test]
    fn import_keeps_the_targets_private_and_pc_fields() {
        let target = private_config();
        let applied = apply(&target, &file_of(&general_config())).unwrap();
        assert_eq!(applied.google_client_id, "id.apps");
        assert_eq!(applied.google_client_secret, "s3cret");
        assert_eq!(applied.muted_apps, target.muted_apps);
        assert_eq!(applied.arrange.saved, target.arrange.saved);
        assert_eq!(applied.recording.folder, target.recording.folder);
        assert_eq!(applied.recording.video_folder, target.recording.video_folder);
        assert_eq!(applied.recording.mic_device, target.recording.mic_device);
        assert_eq!(
            applied.recording.system_device,
            target.recording.system_device
        );
        assert_eq!(
            applied.recording.consent_shown,
            target.recording.consent_shown
        );
        assert_eq!(applied.notch_monitor, target.notch_monitor);
        assert_eq!(applied.settings_window, target.settings_window);
        assert_eq!(applied.lyrics_card, target.lyrics_card);
        assert_eq!(applied.transcript.window, target.transcript.window);
        assert!(applied.transcript.setup_offered);
        assert_eq!(applied.captions.place, target.captions.place);
        assert!(applied.onboarding_done);
        assert_eq!(applied.update_offered, "9.9.9");
        let own: Vec<_> = applied
            .shortcut_tiles
            .iter()
            .filter(|t| t.tile == "Custom")
            .collect();
        assert_eq!(own.len(), 1);
        // The general fields moved at the same time.
        assert_eq!(applied.notch_width, 250);
    }

    #[test]
    fn import_takes_general_fields() {
        let g = general_config();
        let applied = apply(&private_config(), &file_of(&g)).unwrap();
        assert_eq!(applied.accent, g.accent);
        assert_eq!(applied.notch_width, 250);
        assert_eq!(applied.hotkeys, g.hotkeys);
        assert_eq!(applied.mascot, g.mascot);
        assert_eq!(applied.battery_threshold, 33);
        assert_eq!(applied.recording.video_fps, g.recording.video_fps);
        assert_eq!(applied.transcript.model, g.transcript.model);
        assert_eq!(applied.captions.size, g.captions.size);
        assert_eq!(applied.arrange.gap, 17);
    }

    #[test]
    fn missing_keys_keep_target_values() {
        let target = general_config();
        let incoming = Incoming {
            app_version: "5.3.0".into(),
            exported: None,
            settings: obj(json!({"accent": "Sage"})),
        };
        let applied = apply(&target, &incoming).unwrap();
        let mut expected = target.clone();
        expected.accent = config::parse(r#"{"accent":"Sage"}"#).accent;
        assert_eq!(applied, expected);
        assert_ne!(applied.accent, target.accent);
    }

    #[test]
    fn hand_added_private_keys_are_ignored() {
        let target = private_config();
        let incoming = Incoming {
            app_version: "5.3.0".into(),
            exported: None,
            settings: obj(json!({
                "google_client_id": "evil",
                "google_client_secret": "evil",
                "muted_apps": ["evil"],
                "notch_monitor": {"path": "evil", "name": "evil"},
                "onboarding_done": false,
                "recording": {"folder": "C:\\evil", "mic_device": "evil"},
                "transcript": {"setup_offered": false},
                "captions": {"place": {"left":0,"top":0,"right":1,"bottom":1}},
                "arrange": {"saved": []},
            })),
        };
        assert_eq!(apply(&target, &incoming).unwrap(), target);
    }

    #[test]
    fn bad_values_keep_the_targets() {
        let incoming = Incoming {
            app_version: String::new(),
            exported: None,
            settings: obj(json!({"notch_width": "wide", "hotkeys": 5, "accent": "Plum"})),
        };
        let target = Config {
            notch_width: 280,
            ..general_config()
        };
        let applied = apply(&target, &incoming).unwrap();
        assert_eq!(applied.notch_width, 280);
        assert_eq!(applied.hotkeys, target.hotkeys);
        assert_eq!(applied.accent, target.accent);
    }

    #[test]
    fn bad_values_in_split_fields_keep_the_targets() {
        let incoming = Incoming {
            app_version: String::new(),
            exported: None,
            settings: obj(json!({"recording": {"video_fps": "fast"}, "arrange": {"gap": "x"}})),
        };
        let mut target = general_config();
        target.recording.video_fps = 60;
        let applied = apply(&target, &incoming).unwrap();
        assert_eq!(applied.recording.video_fps, 60);
        assert_eq!(applied.arrange.gap, 17);
    }

    #[test]
    fn tiles_merge_builtins_then_own() {
        let target = private_config();
        let incoming = Incoming {
            app_version: String::new(),
            exported: None,
            settings: obj(json!({"shortcut_tiles": [
                {"tile": "Search", "on": false},
                {"tile": "NoSuchTile"},
                {"tile": "Lock", "kind": "Exe", "target": "C:\\evil.exe"},
                {"tile": "Custom", "label": "x", "kind": "Url", "target": "https://x"},
                {"tile": "Lock", "on": true},
            ]})),
        };
        let applied = apply(&target, &incoming).unwrap();
        let tiles: Vec<(&str, bool)> = applied
            .shortcut_tiles
            .iter()
            .map(|t| (t.tile.as_str(), t.on))
            .collect();
        assert_eq!(tiles, [("Search", false), ("Lock", true), ("Custom", true)]);
        let own = &applied.shortcut_tiles[2];
        assert_eq!(own.target, "C:\\Users\\X\\a.exe");
        assert_eq!(own.label, "Mine");
    }

    #[test]
    fn refusals() {
        let not_vale = Err(ImportError::NotVale);
        assert_eq!(read_incoming(b"not json"), not_vale);
        assert_eq!(read_incoming(b"{}"), not_vale);
        assert_eq!(
            read_incoming(br#"{"vale_settings":"1","settings":{}}"#),
            not_vale
        );
        assert_eq!(read_incoming(br#"{"vale_settings":1}"#), not_vale);
        assert_eq!(
            read_incoming(br#"{"vale_settings":1,"settings":[]}"#),
            not_vale
        );
        assert_eq!(read_incoming(&vec![b' '; MAX_BYTES + 1]), not_vale);
        assert_eq!(read_incoming(&[0xff, 0xfe, 0x00]), not_vale);
        assert_eq!(
            read_incoming(br#"{"vale_settings":2,"app_version":"6.0.0","settings":{}}"#),
            Err(ImportError::Newer {
                app_version: "6.0.0".into()
            })
        );
    }

    #[test]
    fn reads_the_header() {
        let incoming = file_of(&Config::default());
        assert_eq!(incoming.app_version, "5.3.0");
        assert_eq!(incoming.exported, NaiveDate::from_ymd_opt(2026, 11, 13));
        let bare = read_incoming(br#"{"vale_settings":1,"settings":{}}"#).unwrap();
        assert_eq!(bare.exported, None);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let incoming = read_incoming(
            br#"{"vale_settings":1,"settings":{"future_thing":true,"accent":"Plum"}}"#,
        )
        .unwrap();
        let applied = apply(&Config::default(), &incoming).unwrap();
        assert_ne!(applied.accent, Config::default().accent);
    }

    #[test]
    fn round_trip() {
        let custom = general_config();
        let applied = apply(&Config::default(), &file_of(&custom)).unwrap();
        assert_eq!(general_settings(&applied), general_settings(&custom));
    }

    /// A fresh, empty folder under `target/test-tmp/`.
    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("config_transfer_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_export_replaces_and_leaves_no_tmp() {
        let dir = scratch("write_export");
        let path = dir.join("out.json");
        std::fs::write(&path, "old").unwrap();
        let when = DateTime::parse_from_rfc3339("2026-11-13T14:05:00+07:00").unwrap();
        write_export(&path, &general_config(), when).unwrap();
        let back = read_incoming(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back.settings["notch_width"], json!(250));
        assert!(!dir.join("out.json.tmp").exists());
    }

    #[test]
    fn backup_copies_and_never_overwrites() {
        let dir = scratch("backup");
        let cfg = dir.join("config.json");
        std::fs::write(&cfg, "one").unwrap();
        let now = NaiveDate::from_ymd_opt(2026, 11, 13)
            .unwrap()
            .and_hms_opt(14, 5, 9)
            .unwrap();
        let a = backup_config(&cfg, now).unwrap().unwrap();
        std::fs::write(&cfg, "two").unwrap();
        let b = backup_config(&cfg, now).unwrap().unwrap();
        assert_eq!(
            a.file_name().unwrap(),
            "config-before-import-20261113-140509.json"
        );
        assert_eq!(
            b.file_name().unwrap(),
            "config-before-import-20261113-140509-2.json"
        );
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "two");
    }

    #[test]
    fn backup_without_config_is_none() {
        let dir = scratch("backup_none");
        let now = NaiveDate::from_ymd_opt(2026, 11, 13)
            .unwrap()
            .and_hms_opt(1, 2, 3)
            .unwrap();
        assert_eq!(backup_config(&dir.join("config.json"), now).unwrap(), None);
    }

    #[test]
    fn import_survives_the_islands_later_saves() {
        let dir = scratch("survives");
        let path = dir.join("config.json");
        let on_disk = Config {
            battery_threshold: 20,
            ..Config::default()
        };
        on_disk.save_to(&path).unwrap();
        let mem = Config::default();
        let incoming = Incoming {
            app_version: "5.3.0".into(),
            exported: None,
            settings: obj(json!({"battery_threshold": 35, "accent": "Plum"})),
        };
        let new = import_and_save(&path, &mem, &incoming).unwrap();
        // The restart's save and the teardown save both merge from memory.
        config::save_merged_at(&path, &new);
        config::save_merged_at(&path, &new);
        let after = config::parse(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(after.battery_threshold, 35);
        assert_eq!(after.accent, new.accent);
        assert_ne!(after.accent, Config::default().accent);
    }

    /// A target with private and own values, a file with `bad` settings next
    /// to a good `notch_width`: every private value stays, the good one lands.
    fn bad_value_keeps_target(bad: Value) {
        let target = private_config();
        let mut settings = obj(bad);
        settings.insert("notch_width".into(), json!(250));
        if !settings.contains_key("accent") {
            settings.insert("accent".into(), json!("Plum"));
        }
        let incoming = Incoming {
            app_version: String::new(),
            exported: None,
            settings,
        };
        let applied = apply(&target, &incoming).expect("a bad value must not fail the import");
        assert_eq!(applied.google_client_secret, "s3cret");
        assert_eq!(applied.muted_apps, target.muted_apps);
        assert!(applied.shortcut_tiles.iter().any(|t| t.tile == "Custom"));
        assert!(applied.onboarding_done);
        assert_eq!(applied.notch_width, 250);
        assert_ne!(applied.notch_width, target.notch_width);
    }

    #[test]
    fn a_bad_style_keeps_the_targets() {
        bad_value_keeps_target(json!({"style": "Bogus"}));
        bad_value_keeps_target(json!({"style": null}));
    }

    #[test]
    fn a_bad_bool_keeps_the_targets() {
        bad_value_keeps_target(json!({"robot": "yes"}));
    }

    #[test]
    fn a_bad_number_keeps_the_targets() {
        bad_value_keeps_target(json!({"battery_threshold": 300}));
    }

    #[test]
    fn a_bad_enum_keeps_the_targets() {
        bad_value_keeps_target(json!({"accent": 5}));
    }

    #[test]
    fn a_bad_nested_value_keeps_the_targets() {
        bad_value_keeps_target(json!({"pomodoro": {"focus_min": "x"}}));
    }

    #[test]
    fn a_bad_split_value_keeps_the_targets() {
        let mut target = private_config();
        target.recording.video_fps = 60;
        assert_ne!(target.recording.video_fps, Config::default().recording.video_fps);
        let incoming = Incoming {
            app_version: String::new(),
            exported: None,
            settings: obj(json!({"recording": {"video_fps": "fast"}})),
        };
        let applied = apply(&target, &incoming).unwrap();
        assert_eq!(applied.recording.video_fps, target.recording.video_fps);
        assert_eq!(applied.recording.folder, target.recording.folder);
    }

    #[test]
    fn imported_builtin_tiles_never_push_own_tiles_off_the_page() {
        let target = private_config();
        let names: Vec<String> = crate::model::Shortcut::ALL
            .iter()
            .map(|s| crate::tiles::shortcut_name(*s))
            .collect();
        assert!(names.len() > crate::tiles::MAX_SHOWN);
        let all_on: Vec<Value> = names.iter().map(|n| json!({"tile": n, "on": true})).collect();
        let incoming = Incoming {
            app_version: String::new(),
            exported: None,
            settings: obj(json!({"shortcut_tiles": all_on})),
        };
        let applied = apply(&target, &incoming).unwrap();
        let shown = applied.shortcut_tiles.iter().filter(|t| t.on).count();
        assert!(shown <= crate::tiles::MAX_SHOWN);
        assert!(applied.shortcut_tiles.iter().any(|t| t.tile == "Custom" && t.on));
    }

    #[test]
    fn a_failed_write_is_an_error() {
        let dir = scratch("save_fails");
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "a file, not a folder").unwrap();
        let path = blocker.join("config.json");
        let incoming = Incoming {
            app_version: String::new(),
            exported: None,
            settings: obj(json!({"accent": "Plum"})),
        };
        assert_eq!(
            import_and_save(&path, &Config::default(), &incoming),
            Err(ImportFailure::Save)
        );
    }

    #[test]
    fn default_export_names() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        assert_eq!(
            default_export_name(Lang::Id, day),
            "Vale-pengaturan-2026-11-13.json"
        );
        assert_eq!(
            default_export_name(Lang::En, day),
            "Vale-settings-2026-11-13.json"
        );
    }
}
