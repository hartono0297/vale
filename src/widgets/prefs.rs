//! The widgets' own settings (Settings -> Widget -> "Di desktop"), kept in
//! config.json under `widgets`. Read leniently: a missing key, a wrong type or
//! an unknown name gives that field's default, a non-object gives all of
//! them, and the rest of the file always loads.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// Where the widgets sit in the z-order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WidgetLayer {
    /// Behind ordinary windows, like desktop icons.
    #[default]
    Desktop,
    /// Above every window.
    Top,
}

impl WidgetLayer {
    pub const ALL: [WidgetLayer; 2] = [WidgetLayer::Desktop, WidgetLayer::Top];

    pub fn name(self) -> &'static str {
        match self {
            WidgetLayer::Desktop => "Desktop",
            WidgetLayer::Top => "Top",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL
            .into_iter()
            .find(|v| v.name().eq_ignore_ascii_case(name))
    }
}

/// What topmost widgets do while a fullscreen app covers their monitor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FullscreenRule {
    #[default]
    Hide,
    Keep,
}

impl FullscreenRule {
    pub const ALL: [FullscreenRule; 2] = [FullscreenRule::Hide, FullscreenRule::Keep];

    pub fn name(self) -> &'static str {
        match self {
            FullscreenRule::Hide => "Hide",
            FullscreenRule::Keep => "Keep",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL
            .into_iter()
            .find(|v| v.name().eq_ignore_ascii_case(name))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetPrefs {
    /// "Tampilkan widget": every widget shown or hidden (their state is kept).
    pub show: bool,
    pub layer: WidgetLayer,
    pub fullscreen: FullscreenRule,
    /// "Kunci posisi": no drag, no resize.
    pub lock: bool,
    /// "Kucing ngintip": the cat looks over the last touched note.
    pub cat_peek: bool,
    /// "Miring sedikit": every note is drawn leaning a fixed angle within 2
    /// degrees (from its id). Off by default.
    pub tilt: bool,
}

impl Default for WidgetPrefs {
    fn default() -> Self {
        WidgetPrefs {
            show: true,
            layer: WidgetLayer::Desktop,
            fullscreen: FullscreenRule::Hide,
            lock: false,
            cat_peek: true,
            tilt: false,
        }
    }
}

impl WidgetPrefs {
    /// Every field read on its own from a JSON value.
    pub fn from_json(v: &Value) -> WidgetPrefs {
        let d = WidgetPrefs::default();
        let Some(map) = v.as_object() else {
            return d;
        };
        let flag =
            |key: &str, default: bool| map.get(key).and_then(Value::as_bool).unwrap_or(default);
        WidgetPrefs {
            show: flag("show", d.show),
            layer: map
                .get("layer")
                .and_then(Value::as_str)
                .and_then(WidgetLayer::from_name)
                .unwrap_or(d.layer),
            fullscreen: map
                .get("fullscreen")
                .and_then(Value::as_str)
                .and_then(FullscreenRule::from_name)
                .unwrap_or(d.fullscreen),
            lock: flag("lock", d.lock),
            cat_peek: flag("cat_peek", d.cat_peek),
            tilt: flag("tilt", d.tilt),
        }
    }
}

impl Serialize for WidgetPrefs {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(6))?;
        map.serialize_entry("show", &self.show)?;
        map.serialize_entry("layer", self.layer.name())?;
        map.serialize_entry("fullscreen", self.fullscreen.name())?;
        map.serialize_entry("lock", &self.lock)?;
        map.serialize_entry("cat_peek", &self.cat_peek)?;
        map.serialize_entry("tilt", &self.tilt)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for WidgetPrefs {
    /// Never an error: see the module note.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(WidgetPrefs::from_json(&Value::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_are_the_documented_ones() {
        let d = WidgetPrefs::default();
        assert!(d.show && d.cat_peek && !d.lock && !d.tilt, "tilt is off by default");
        assert_eq!(d.layer, WidgetLayer::Desktop);
        assert_eq!(d.fullscreen, FullscreenRule::Hide);
    }

    #[test]
    fn a_non_object_gives_every_default() {
        for v in [json!(null), json!(3), json!("x"), json!([1, 2]), json!(true)] {
            assert_eq!(WidgetPrefs::from_json(&v), WidgetPrefs::default(), "{v}");
        }
    }

    #[test]
    fn each_field_is_read_on_its_own() {
        let v = json!({
            "show": false,
            "layer": " top ",
            "fullscreen": 7,
            "lock": "yes",
            "cat_peek": false,
            "tilt": true,
        });
        let p = WidgetPrefs::from_json(&v);
        assert!(p.tilt);
        assert_eq!(WidgetPrefs::from_json(&json!({"tilt": "yes"})).tilt, false, "a wrong type is off");
        let p = WidgetPrefs::from_json(&v);
        assert!(!p.show, "a good bool is kept");
        assert_eq!(p.layer, WidgetLayer::Top, "names are trimmed, any case");
        assert_eq!(p.fullscreen, FullscreenRule::Hide, "a wrong type is the default");
        assert!(!p.lock, "a wrong type is the default");
        assert!(!p.cat_peek);
        let typo = WidgetPrefs::from_json(&json!({"layer": "Floating", "fullscreen": "Keep"}));
        assert_eq!(typo.layer, WidgetLayer::Desktop);
        assert_eq!(typo.fullscreen, FullscreenRule::Keep);
    }

    #[test]
    fn it_round_trips() {
        let p = WidgetPrefs {
            show: false,
            layer: WidgetLayer::Top,
            fullscreen: FullscreenRule::Keep,
            lock: true,
            cat_peek: false,
            tilt: true,
        };
        let text = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<WidgetPrefs>(&text).unwrap(), p);
        let d = WidgetPrefs::default();
        let text = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<WidgetPrefs>(&text).unwrap(), d);
    }
}
