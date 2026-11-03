//! The five global hotkeys as data: a combo (`Hotkey`), the rows that can
//! hold one (`HotkeyId`, `Hotkeys`), their text form in config.json, the rules
//! a combo must meet (`check`, `used_by`), what the recorder makes of one key
//! press (`record_step`) and the migration from the old fixed presets
//! (`from_legacy`). Pure: nothing here touches Windows.
//!
//! The text form is the modifiers in the order Win, Ctrl, Alt, Shift joined
//! with `+`, then the key: "Ctrl+Alt+V", "Win+Shift+F13", "PrtSc". "None" is a
//! row that is off.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

// Virtual-key codes the rules name (the values are fixed by Win32).
const VK_CANCEL: u16 = 0x03;
const VK_BACK: u16 = 0x08;
const VK_TAB: u16 = 0x09;
const VK_RETURN: u16 = 0x0D;
const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12;
const VK_PAUSE: u16 = 0x13;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;
const VK_F4: u16 = 0x73;
const VK_SNAPSHOT: u16 = 0x2C;
const VK_DELETE: u16 = 0x2E;
const VK_LWIN: u16 = 0x5B;
const VK_RWIN: u16 = 0x5C;
const VK_SCROLL: u16 = 0x91;
const VK_LSHIFT: u16 = 0xA0;
const VK_RSHIFT: u16 = 0xA1;
const VK_LCONTROL: u16 = 0xA2;
const VK_RCONTROL: u16 = 0xA3;
const VK_LMENU: u16 = 0xA4;
const VK_RMENU: u16 = 0xA5;

/// `RegisterHotKey` modifier bits (`MOD_ALT` ...), as plain numbers so this
/// file needs no Windows crate; a test checks them against the crate.
const MOD_ALT: u32 = 1;
const MOD_CONTROL: u32 = 2;
const MOD_SHIFT: u32 = 4;
const MOD_WIN: u32 = 8;
const MOD_NOREPEAT: u32 = 0x4000;

/// The modifiers of a combo. Left and right keys are the same flag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Mods {
    /// The `RegisterHotKey` bits (without `MOD_NOREPEAT`); also what the
    /// PrtSc hook's table keeps in an `AtomicU8`.
    pub fn bits(self) -> u8 {
        (if self.alt { MOD_ALT as u8 } else { 0 })
            | (if self.ctrl { MOD_CONTROL as u8 } else { 0 })
            | (if self.shift { MOD_SHIFT as u8 } else { 0 })
            | (if self.win { MOD_WIN as u8 } else { 0 })
    }

    /// The inverse of `bits`.
    pub fn from_bits(bits: u8) -> Mods {
        Mods {
            alt: bits & MOD_ALT as u8 != 0,
            ctrl: bits & MOD_CONTROL as u8 != 0,
            shift: bits & MOD_SHIFT as u8 != 0,
            win: bits & MOD_WIN as u8 != 0,
        }
    }

    /// Whether the combo has Ctrl, Alt or Win (Shift alone is not enough).
    pub fn has_primary(self) -> bool {
        self.ctrl || self.alt || self.win
    }

    /// The flag a modifier key sets, as `bits` would: `None` for any other key.
    pub fn bit_of_vk(vk: u16) -> Option<u8> {
        let m = match vk {
            VK_SHIFT | VK_LSHIFT | VK_RSHIFT => MOD_SHIFT,
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL => MOD_CONTROL,
            VK_MENU | VK_LMENU | VK_RMENU => MOD_ALT,
            VK_LWIN | VK_RWIN => MOD_WIN,
            _ => return None,
        };
        Some(m as u8)
    }
}

/// Whether `vk` is a modifier key (never a combo's key).
pub fn is_modifier_vk(vk: u16) -> bool {
    Mods::bit_of_vk(vk).is_some()
}

/// A combo: modifiers and one non-modifier key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub mods: Mods,
    pub vk: u16,
}

/// Names that are not a letter, digit, F-key or numpad digit.
const NAMED: &[(&str, u16)] = &[
    ("Space", VK_SPACE),
    ("PrtSc", VK_SNAPSHOT),
    ("Pause", VK_PAUSE),
    ("ScrollLock", VK_SCROLL),
    ("Insert", 0x2D),
    ("Delete", VK_DELETE),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Enter", VK_RETURN),
    ("Tab", VK_TAB),
    ("Esc", VK_ESCAPE),
    ("Backspace", VK_BACK),
    ("NumMultiply", 0x6A),
    ("NumAdd", 0x6B),
    ("NumSubtract", 0x6D),
    ("NumDecimal", 0x6E),
    ("NumDivide", 0x6F),
    ("Semicolon", 0xBA),
    ("Equals", 0xBB),
    ("Comma", 0xBC),
    ("Minus", 0xBD),
    ("Period", 0xBE),
    ("Slash", 0xBF),
    ("Backtick", 0xC0),
    ("LBracket", 0xDB),
    ("Backslash", 0xDC),
    ("RBracket", 0xDD),
    ("Quote", 0xDE),
    ("CapsLock", 0x14),
    ("NumLock", 0x90),
];

/// Other spellings the parser takes: (alias, what it means).
const KEY_ALIASES: &[(&str, &str)] = &[
    ("PrintScreen", "PrtSc"),
    ("PrtScn", "PrtSc"),
    ("Del", "Delete"),
    ("Escape", "Esc"),
];

/// The key a combo holds, with VK_CANCEL (what Ctrl+Pause produces) as Pause.
fn normal_vk(vk: u16) -> u16 {
    if vk == VK_CANCEL { VK_PAUSE } else { vk }
}

/// The written name of a key: "V", "7", "F13", "Num5", a name from `NAMED`, or
/// "VK_" plus two uppercase hex digits.
pub fn key_name(vk: u16) -> String {
    let vk = normal_vk(vk);
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        _ => match NAMED.iter().find(|(_, v)| *v == vk) {
            Some((name, _)) => (*name).to_owned(),
            None => format!("VK_{:02X}", vk & 0xFF),
        },
    }
}

/// The key a name stands for (any case, aliases included); a modifier is not
/// a key.
fn vk_from_name(name: &str) -> Option<u16> {
    let lower = name.to_ascii_lowercase();
    let canon = KEY_ALIASES
        .iter()
        .find(|(a, _)| a.eq_ignore_ascii_case(&lower))
        .map_or(lower.as_str(), |(_, to)| *to)
        .to_ascii_lowercase();
    let bytes = canon.as_bytes();
    if bytes.len() == 1 {
        let c = bytes[0].to_ascii_uppercase();
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(u16::from(c));
        }
    }
    if let Some(n) = canon.strip_prefix('f')
        && !n.starts_with('0')
        && n.bytes().all(|b| b.is_ascii_digit())
        && let Ok(n @ 1..=24) = n.parse::<u16>()
    {
        return Some(0x6F + n);
    }
    if let Some(n) = canon.strip_prefix("num")
        && n.len() == 1
        && let Some(d) = n.chars().next().and_then(|c| c.to_digit(10))
    {
        return Some(0x60 + d as u16);
    }
    if let Some(hex) = canon.strip_prefix("vk_")
        && hex.len() == 2
        && let Ok(v) = u16::from_str_radix(hex, 16)
    {
        let v = normal_vk(v);
        return (!is_modifier_vk(v)).then_some(v);
    }
    NAMED
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(&canon))
        .map(|(_, v)| *v)
}

/// A modifier word (any case, with its aliases).
fn modifier_from_name(name: &str) -> Option<fn(&mut Mods)> {
    match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(|m| m.ctrl = true),
        "alt" => Some(|m| m.alt = true),
        "shift" => Some(|m| m.shift = true),
        "win" | "windows" => Some(|m| m.win = true),
        _ => None,
    }
}

impl Hotkey {
    /// The parts for key caps: modifiers in the canonical order, then the key.
    pub fn caps(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(5);
        if self.mods.win {
            out.push("Win".to_owned());
        }
        if self.mods.ctrl {
            out.push("Ctrl".to_owned());
        }
        if self.mods.alt {
            out.push("Alt".to_owned());
        }
        if self.mods.shift {
            out.push("Shift".to_owned());
        }
        out.push(key_name(self.vk));
        out
    }
}

/// The `RegisterHotKey` modifiers of a combo with `MOD_NOREPEAT`.
pub fn register_mods(hk: Hotkey) -> u32 {
    u32::from(hk.mods.bits()) | MOD_NOREPEAT
}

/// The `RegisterHotKey` key of a combo. With Ctrl held the Pause key sends
/// `VK_CANCEL` (Ctrl+Break), not `VK_PAUSE`, so Ctrl+Pause is registered on
/// that code; the combo is still stored and shown as Pause.
pub fn register_vk(hk: Hotkey) -> u16 {
    if hk.mods.ctrl && hk.vk == VK_PAUSE {
        VK_CANCEL
    } else {
        hk.vk
    }
}

/// Whether the low-level hook (not `RegisterHotKey`) has to carry it:
/// Windows owns the PrtSc key.
pub fn uses_hook(hk: Hotkey) -> bool {
    hk.vk == VK_SNAPSHOT
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.caps().join("+"))
    }
}

/// Text that is not a combo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError;

/// A combo from its text (any case, any modifier order, blanks ignored).
/// `Ok(None)` is a row that is off ("", "none", "off"). A key that is only a
/// modifier ("Ctrl", "Win") or a text with two keys is an error.
pub fn parse(text: &str) -> Result<Option<Hotkey>, ParseError> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("none") || text.eq_ignore_ascii_case("off") {
        return Ok(None);
    }
    let mut mods = Mods::default();
    let mut key: Option<u16> = None;
    for part in text.split('+') {
        let part = part.trim();
        if part.is_empty() {
            return Err(ParseError);
        }
        if let Some(set) = modifier_from_name(part) {
            set(&mut mods);
        } else if key.is_none() {
            key = Some(vk_from_name(part).ok_or(ParseError)?);
        } else {
            return Err(ParseError);
        }
    }
    key.map(|vk| Some(Hotkey { mods, vk })).ok_or(ParseError)
}

/// A row's text: the combo, or "None".
pub fn format_opt(hk: Option<Hotkey>) -> String {
    hk.map_or_else(|| "None".to_owned(), |h| h.to_string())
}

/// Why a combo is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    /// Windows keeps it (or it is only a modifier).
    Reserved,
    /// Needs Ctrl, Alt or Win.
    NeedsModifier,
}

/// The combos Windows or the common apps keep: (modifiers, key).
fn reserved(hk: Hotkey) -> bool {
    let m = hk.mods;
    let (c, a, s, w) = (m.ctrl, m.alt, m.shift, m.win);
    match hk.vk {
        VK_TAB => a && !c && !w,
        VK_F4 | VK_ESCAPE | VK_SPACE if a && !c && !s && !w => true,
        VK_ESCAPE => (c && !a && !s && !w) || (c && s && !a && !w),
        VK_DELETE => c && a && !s && !w,
        0x4C => w && !c && !a && !s,
        0x56 => c && s && !a && !w,
        _ => false,
    }
}

/// Whether a combo may be a hotkey.
pub fn check(hk: Hotkey) -> Result<(), Reject> {
    if is_modifier_vk(hk.vk) || reserved(hk) {
        return Err(Reject::Reserved);
    }
    if !hk.mods.has_primary() {
        let bare_ok = matches!(hk.vk, VK_SNAPSHOT | VK_PAUSE | VK_SCROLL | 0x7C..=0x87);
        if !bare_ok {
            return Err(Reject::NeedsModifier);
        }
    }
    Ok(())
}

/// The rows that can hold a hotkey, in the order Settings lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HotkeyId {
    Search,
    Clipboard,
    Devices,
    Arrange,
    Screenshot,
    /// The floating lyrics card ("Buka lirik"), appended last so the ids and
    /// the order of the earlier rows never change.
    LyricsCard,
}

impl HotkeyId {
    /// Every row, in Settings order.
    pub const ALL: [HotkeyId; 6] = [
        HotkeyId::Search,
        HotkeyId::Clipboard,
        HotkeyId::Devices,
        HotkeyId::Arrange,
        HotkeyId::Screenshot,
        HotkeyId::LyricsCard,
    ];

    /// The row's key in config.json.
    pub fn config_key(self) -> &'static str {
        match self {
            HotkeyId::Search => "search",
            HotkeyId::Clipboard => "clipboard",
            HotkeyId::Devices => "devices",
            HotkeyId::Arrange => "arrange",
            HotkeyId::Screenshot => "screenshot",
            HotkeyId::LyricsCard => "lyrics_card",
        }
    }

    /// What the row has until the user changes it.
    pub fn default(self) -> Option<Hotkey> {
        let hk = |mods, vk| Some(Hotkey { mods, vk });
        match self {
            HotkeyId::Search => hk(
                Mods {
                    ctrl: true,
                    alt: true,
                    ..Mods::default()
                },
                VK_SPACE,
            ),
            HotkeyId::Screenshot => hk(Mods::default(), VK_SNAPSHOT),
            HotkeyId::LyricsCard => hk(
                Mods {
                    ctrl: true,
                    alt: true,
                    ..Mods::default()
                },
                u16::from(b'L'),
            ),
            _ => None,
        }
    }

    /// The position in `ALL` (0..6).
    pub fn index(self) -> usize {
        self as usize
    }

    /// The row at `index`, if any.
    pub fn from_index(index: usize) -> Option<HotkeyId> {
        HotkeyId::ALL.get(index).copied()
    }
}

/// The rows' values; `None` is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hotkeys {
    pub search: Option<Hotkey>,
    pub clipboard: Option<Hotkey>,
    pub devices: Option<Hotkey>,
    pub arrange: Option<Hotkey>,
    pub screenshot: Option<Hotkey>,
    pub lyrics_card: Option<Hotkey>,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Hotkeys {
            search: HotkeyId::Search.default(),
            clipboard: HotkeyId::Clipboard.default(),
            devices: HotkeyId::Devices.default(),
            arrange: HotkeyId::Arrange.default(),
            screenshot: HotkeyId::Screenshot.default(),
            lyrics_card: HotkeyId::LyricsCard.default(),
        }
    }
}

impl Hotkeys {
    /// Every row off.
    pub fn none() -> Hotkeys {
        Hotkeys {
            search: None,
            clipboard: None,
            devices: None,
            arrange: None,
            screenshot: None,
            lyrics_card: None,
        }
    }

    pub fn get(&self, id: HotkeyId) -> Option<Hotkey> {
        match id {
            HotkeyId::Search => self.search,
            HotkeyId::Clipboard => self.clipboard,
            HotkeyId::Devices => self.devices,
            HotkeyId::Arrange => self.arrange,
            HotkeyId::Screenshot => self.screenshot,
            HotkeyId::LyricsCard => self.lyrics_card,
        }
    }

    pub fn set(&mut self, id: HotkeyId, value: Option<Hotkey>) {
        match id {
            HotkeyId::Search => self.search = value,
            HotkeyId::Clipboard => self.clipboard = value,
            HotkeyId::Devices => self.devices = value,
            HotkeyId::Arrange => self.arrange = value,
            HotkeyId::Screenshot => self.screenshot = value,
            HotkeyId::LyricsCard => self.lyrics_card = value,
        }
    }

    /// One row back to its default (Settings sets the default itself, through
    /// the same checks, so only the tests call this).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn reset(&mut self, id: HotkeyId) {
        self.set(id, id.default());
    }

    /// Every row back to its default.
    pub fn reset_all(&mut self) {
        *self = Hotkeys::default();
    }

    /// Whether a row has its default.
    pub fn is_default(&self, id: HotkeyId) -> bool {
        self.get(id) == id.default()
    }

    /// Whether every row has its default.
    pub fn all_default(&self) -> bool {
        HotkeyId::ALL.iter().all(|id| self.is_default(*id))
    }

    /// The config as lenient JSON: one value per row.
    pub fn from_json(value: &serde_json::Value) -> Hotkeys {
        let Some(map) = value.as_object() else {
            return Hotkeys::default();
        };
        let mut out = Hotkeys::none();
        for id in HotkeyId::ALL {
            let read = match map.get(id.config_key()) {
                None => id.default(),
                Some(serde_json::Value::Null) => None,
                Some(serde_json::Value::String(text)) => match parse(text) {
                    Ok(None) => None,
                    Ok(Some(hk)) if check(hk).is_ok() => Some(hk),
                    _ => id.default(),
                },
                Some(_) => id.default(),
            };
            let taken = read.is_some_and(|hk| {
                HotkeyId::ALL[..id.index()]
                    .iter()
                    .any(|earlier| out.get(*earlier) == Some(hk))
            });
            out.set(id, if taken { None } else { read });
        }
        out
    }
}

/// The row that already has exactly `hk`, other than `id`.
pub fn used_by(keys: &Hotkeys, id: HotkeyId, hk: Hotkey) -> Option<HotkeyId> {
    HotkeyId::ALL
        .into_iter()
        .find(|other| *other != id && keys.get(*other) == Some(hk))
}

impl Serialize for Hotkeys {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(HotkeyId::ALL.len()))?;
        for id in HotkeyId::ALL {
            map.serialize_entry(id.config_key(), &format_opt(self.get(id)))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Hotkeys {
    /// Never an error: a missing, unreadable, refused or repeated row reads as
    /// described at `Hotkeys::from_json`, so the rest of the file loads.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Hotkeys, D::Error> {
        let value = serde_json::Value::deserialize(d)?;
        Ok(Hotkeys::from_json(&value))
    }
}

/// What one key press means to the recorder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// A modifier key: keep listening for the combo.
    Ignore,
    /// Bare Esc: stop, nothing changes.
    Cancel,
    /// Bare Backspace or Delete: the row goes off.
    Clear,
    /// Anything else: a combo to check and keep.
    Combo(Hotkey),
}

/// The recorder's decision for the key `vk` pressed with `mods` held. Shared
/// by the keyboard hook and the window fallback. Esc, Backspace and Delete
/// with a modifier are ordinary combos.
pub fn record_step(vk: u16, mods: Mods) -> Step {
    let vk = normal_vk(vk);
    if is_modifier_vk(vk) {
        return Step::Ignore;
    }
    let bare = mods == Mods::default();
    match vk {
        VK_ESCAPE if bare => Step::Cancel,
        VK_BACK | VK_DELETE if bare => Step::Clear,
        _ => Step::Combo(Hotkey { mods, vk }),
    }
}

/// The old fixed presets, read from a config root, as the new rows. Old names
/// match in any case; a typo reads as off.
pub fn from_legacy(root: &serde_json::Value) -> Hotkeys {
    let text = |v: Option<&serde_json::Value>| -> Option<String> {
        v.and_then(serde_json::Value::as_str)
            .map(str::to_ascii_lowercase)
    };
    let ctrl_alt = Mods {
        ctrl: true,
        alt: true,
        ..Mods::default()
    };
    let win_alt = Mods {
        win: true,
        alt: true,
        ..Mods::default()
    };
    let combo = |mods, key: u8| {
        Some(Hotkey {
            mods,
            vk: u16::from(key),
        })
    };

    let clipboard = match text(root.pointer("/clipboard/hotkey")).as_deref() {
        Some("ctrlaltv") => combo(ctrl_alt, b'V'),
        Some("winaltv") => combo(win_alt, b'V'),
        _ => None,
    };
    let search_of = |name: &str| match name {
        "ctrlaltspace" => HotkeyId::Search.default(),
        "winalts" => combo(win_alt, b'S'),
        _ => None,
    };
    let search = if let Some(v) = root.get("quick_search_hotkey") {
        v.as_str().and_then(|s| search_of(&s.to_ascii_lowercase()))
    } else if let Some(old) = root
        .get("search_hotkey")
        .and_then(serde_json::Value::as_str)
        .filter(|s| *s != "Off")
    {
        search_of(&old.to_ascii_lowercase())
    } else {
        HotkeyId::Search.default()
    };
    let devices = match text(root.get("devices_hotkey")).as_deref() {
        Some("ctrlaltd") => combo(ctrl_alt, b'D'),
        _ => None,
    };
    let arrange = match text(root.pointer("/arrange/hotkey")).as_deref() {
        Some("ctrlalts") => combo(ctrl_alt, b'S'),
        _ => None,
    };
    let screenshot = match root
        .pointer("/screenshot/print_screen")
        .and_then(serde_json::Value::as_bool)
    {
        Some(false) => None,
        _ => HotkeyId::Screenshot.default(),
    };
    Hotkeys {
        search,
        clipboard,
        devices,
        arrange,
        screenshot,
        lyrics_card: HotkeyId::LyricsCard.default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hk(text: &str) -> Hotkey {
        parse(text).unwrap().unwrap()
    }

    fn mods(ctrl: bool, alt: bool, shift: bool, win: bool) -> Mods {
        Mods {
            ctrl,
            alt,
            shift,
            win,
        }
    }

    #[test]
    fn every_name_round_trips() {
        let mut texts: Vec<String> = vec!["PrtSc".into(), "Ctrl+Alt+Space".into()];
        texts.extend((1..=24).map(|n| format!("Win+Shift+F{n}")));
        texts.extend(('A'..='Z').map(|c| format!("Ctrl+Alt+{c}")));
        texts.extend(('0'..='9').map(|c| format!("Ctrl+{c}")));
        texts.extend((0..=9).map(|n| format!("Alt+Num{n}")));
        texts.extend(NAMED.iter().map(|(n, _)| format!("Ctrl+Alt+{n}")));
        texts.extend(["Win+Alt+S", "Win+Shift+F13", "Win+Ctrl+Alt+Shift+Q"].map(String::from));
        for text in texts {
            let parsed = hk(&text);
            assert_eq!(parsed.to_string(), text, "{text}");
            assert_eq!(hk(&parsed.to_string()), parsed, "{text}");
        }
    }

    #[test]
    fn an_unnamed_key_is_written_vk_and_two_hex_digits() {
        let k = Hotkey {
            mods: mods(true, false, false, false),
            vk: 0xE2,
        };
        assert_eq!(k.to_string(), "Ctrl+VK_E2");
        assert_eq!(hk("ctrl+vk_e2"), k);
        let low = Hotkey {
            mods: mods(false, true, false, false),
            vk: 0x0A,
        };
        assert_eq!(low.to_string(), "Alt+VK_0A");
        assert_eq!(hk(&low.to_string()), low);
        // A modifier vk is never a key.
        assert!(parse("Ctrl+VK_11").is_err());
        assert!(parse("VK_5B").is_err());
    }

    #[test]
    fn modifiers_are_written_in_the_canonical_order_whatever_the_input() {
        let want = "Win+Ctrl+Alt+Shift+K";
        for text in [
            "shift+alt+ctrl+win+k",
            "  Alt + Win + Shift + Control + K ",
            "WINDOWS+CTRL+ALT+SHIFT+K",
            "Ctrl+Shift+Win+Alt+k",
        ] {
            assert_eq!(hk(text).to_string(), want, "{text}");
        }
        assert_eq!(hk("alt+ctrl+v").caps(), ["Ctrl", "Alt", "V"]);
        assert_eq!(hk("prtsc").caps(), ["PrtSc"]);
    }

    #[test]
    fn aliases_and_case_are_accepted() {
        assert_eq!(hk("Control+Del").to_string(), "Ctrl+Delete");
        assert_eq!(hk("Windows+PrintScreen").to_string(), "Win+PrtSc");
        assert_eq!(hk("PrtScn").to_string(), "PrtSc");
        assert_eq!(hk("alt+escape").to_string(), "Alt+Esc");
        assert_eq!(hk("CTRL+ALT+PAGEDOWN").to_string(), "Ctrl+Alt+PageDown");
        assert_eq!(hk("ctrl+f5").vk, 0x74);
        assert_eq!(hk("ctrl+num7").vk, 0x67);
    }

    #[test]
    fn off_is_none_in_every_spelling_and_garbage_is_an_error() {
        for text in ["", "  ", "None", "none", "NONE", "off", "Off", " none "] {
            assert_eq!(parse(text), Ok(None), "{text:?}");
        }
        assert_eq!(format_opt(None), "None");
        for text in [
            "Ctrl", "Win", "Ctrl+", "+V", "Ctrl+V+B", "Nope", "F25", "F0", "F013", "Num10",
            "Ctrl+Alt", "VK_1", "VK_ZZ", "Ctrl++V",
        ] {
            assert!(parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn ctrl_pause_reads_as_pause() {
        assert_eq!(key_name(VK_CANCEL), "Pause");
        assert_eq!(hk("Ctrl+Pause").vk, VK_PAUSE);
        assert_eq!(hk("VK_03").to_string(), "Pause");
        assert_eq!(
            record_step(VK_CANCEL, mods(true, false, false, false)),
            Step::Combo(hk("Ctrl+Pause"))
        );
    }

    #[test]
    fn modifier_vks_are_modifiers_left_and_right() {
        for vk in [
            0x10, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C,
        ] {
            assert!(is_modifier_vk(vk), "{vk:#x}");
        }
        for vk in [0x41, 0x1B, 0x20, 0x2C, 0x70] {
            assert!(!is_modifier_vk(vk), "{vk:#x}");
        }
        assert_eq!(Mods::bit_of_vk(0xA0), Mods::bit_of_vk(0xA1));
        assert_eq!(Mods::bit_of_vk(0x5B), Mods::bit_of_vk(0x5C));
        assert_eq!(Mods::bit_of_vk(0xA2), Some(2));
        assert_eq!(Mods::bit_of_vk(0x41), None);
        for bits in 0..16u8 {
            assert_eq!(Mods::from_bits(bits).bits(), bits);
        }
    }

    #[test]
    fn check_accepts_ordinary_combos() {
        for text in [
            "Ctrl+Alt+Space",
            "Ctrl+Alt+V",
            "Win+Alt+S",
            "Win+V",
            "Alt+F1",
            "Ctrl+Shift+K",
            "Ctrl+Shift+Delete",
            "Alt+Shift+Esc",
            "Ctrl+Space",
            "Win+Shift+F13",
            "PrtSc",
            "F13",
            "F24",
            "Pause",
            "ScrollLock",
            "Ctrl+PrtSc",
            "Alt+PrtSc",
            "Ctrl+Pause",
        ] {
            assert_eq!(check(hk(text)), Ok(()), "{text}");
        }
    }

    #[test]
    fn check_refuses_what_windows_keeps() {
        for text in [
            "Alt+Tab",
            "Alt+Shift+Tab",
            "Alt+F4",
            "Alt+Esc",
            "Alt+Space",
            "Ctrl+Esc",
            "Ctrl+Alt+Delete",
            "Ctrl+Shift+Esc",
            "Win+L",
            "Ctrl+Shift+V",
        ] {
            assert_eq!(check(hk(text)), Err(Reject::Reserved), "{text}");
        }
        // A modifier is never the key: "Win alone" and the like.
        for vk in [0x10, 0x11, 0x12, 0xA0, 0xA5, 0x5B, 0x5C] {
            let k = Hotkey {
                mods: Mods::default(),
                vk,
            };
            assert_eq!(check(k), Err(Reject::Reserved), "{vk:#x}");
        }
    }

    #[test]
    fn check_wants_ctrl_alt_or_win() {
        for text in [
            "Shift+K",
            "K",
            "F1",
            "F12",
            "Space",
            "Shift+F5",
            "Esc",
            "Delete",
            "Shift+PrtSc",
        ] {
            let expect = if text == "Shift+PrtSc" {
                Ok(())
            } else {
                Err(Reject::NeedsModifier)
            };
            assert_eq!(check(hk(text)), expect, "{text}");
        }
        // Shift alone does not count, but the bare exceptions stand even with it.
        assert_eq!(check(hk("Shift+K")), Err(Reject::NeedsModifier));
        assert_eq!(check(hk("F13")), Ok(()));
        assert_eq!(check(hk("PrtSc")), Ok(()));
        // The others stay refused with a primary modifier only when reserved.
        assert_eq!(check(hk("Ctrl+K")), Ok(()));
        assert_eq!(check(hk("Ctrl+F4")), Ok(()));
    }

    #[test]
    fn a_combo_used_by_another_row_is_found() {
        let mut keys = Hotkeys::default();
        let v = hk("Ctrl+Alt+V");
        assert_eq!(used_by(&keys, HotkeyId::Clipboard, v), None);
        keys.set(HotkeyId::Clipboard, Some(v));
        assert_eq!(
            used_by(&keys, HotkeyId::Devices, v),
            Some(HotkeyId::Clipboard)
        );
        // The row itself is not a conflict.
        assert_eq!(used_by(&keys, HotkeyId::Clipboard, v), None);
        let space = hk("Ctrl+Alt+Space");
        assert_eq!(
            used_by(&keys, HotkeyId::Arrange, space),
            Some(HotkeyId::Search)
        );
        assert_eq!(
            used_by(&keys, HotkeyId::Arrange, hk("PrtSc")),
            Some(HotkeyId::Screenshot)
        );
    }

    #[test]
    fn the_defaults_and_the_row_order() {
        assert_eq!(
            HotkeyId::ALL.map(HotkeyId::config_key),
            [
                "search",
                "clipboard",
                "devices",
                "arrange",
                "screenshot",
                "lyrics_card"
            ]
        );
        for (i, id) in HotkeyId::ALL.iter().enumerate() {
            assert_eq!(id.index(), i);
            assert_eq!(HotkeyId::from_index(i), Some(*id));
        }
        assert_eq!(HotkeyId::from_index(6), None);
        assert_eq!(HotkeyId::LyricsCard.index(), 5, "appended last");
        let d = Hotkeys::default();
        assert_eq!(d.search, Some(hk("Ctrl+Alt+Space")));
        assert_eq!(
            d.screenshot,
            Some(Hotkey {
                mods: Mods::default(),
                vk: 0x2C
            })
        );
        assert_eq!((d.clipboard, d.devices, d.arrange), (None, None, None));
        assert_eq!(d.lyrics_card, Some(hk("Ctrl+Alt+L")));
        assert!(d.all_default());
    }

    #[test]
    fn reset_one_row_and_all_rows() {
        let mut keys = Hotkeys::default();
        keys.set(HotkeyId::Search, None);
        keys.set(HotkeyId::Clipboard, Some(hk("Win+Alt+V")));
        keys.set(HotkeyId::Screenshot, Some(hk("F13")));
        assert!(!keys.is_default(HotkeyId::Search));
        assert!(!keys.all_default());
        keys.reset(HotkeyId::Search);
        assert_eq!(keys.search, Some(hk("Ctrl+Alt+Space")));
        assert_eq!(keys.clipboard, Some(hk("Win+Alt+V")), "the others stay");
        keys.reset(HotkeyId::Clipboard);
        assert_eq!(keys.clipboard, None);
        keys.reset_all();
        assert_eq!(keys, Hotkeys::default());
    }

    #[test]
    fn the_config_form_is_six_strings_and_round_trips() {
        let mut keys = Hotkeys::default();
        keys.set(HotkeyId::Clipboard, Some(hk("Win+Alt+V")));
        keys.set(HotkeyId::Screenshot, None);
        let v = serde_json::to_value(keys).unwrap();
        assert_eq!(
            v,
            json!({"search":"Ctrl+Alt+Space","clipboard":"Win+Alt+V","devices":"None",
                   "arrange":"None","screenshot":"None","lyrics_card":"Ctrl+Alt+L"})
        );
        assert_eq!(Hotkeys::from_json(&v), keys);
        let back: Hotkeys = serde_json::from_value(v).unwrap();
        assert_eq!(back, keys);
    }

    #[test]
    fn a_missing_bad_or_refused_row_reads_as_its_default_and_the_rest_stays() {
        let v = json!({
            "search": "Banana",          // does not parse -> default
            "clipboard": "Win+Alt+V",    // fine
            "devices": "Alt+Tab",        // reserved -> default (None)
            "arrange": "Shift+S",        // needs a modifier -> default (None)
            // screenshot missing -> default PrtSc
        });
        let got = Hotkeys::from_json(&v);
        assert_eq!(got.search, HotkeyId::Search.default());
        assert_eq!(got.clipboard, Some(hk("Win+Alt+V")));
        assert_eq!(got.devices, None);
        assert_eq!(got.arrange, None);
        assert_eq!(got.screenshot, HotkeyId::Screenshot.default());
        // A refused row whose default is set falls back to that default.
        let got = Hotkeys::from_json(&json!({"screenshot": "Ctrl+Alt+Delete", "search": 5}));
        assert_eq!(got.screenshot, HotkeyId::Screenshot.default());
        assert_eq!(got.search, HotkeyId::Search.default());
        // null and "None" are a deliberate off.
        let got = Hotkeys::from_json(&json!({"search": null, "screenshot": "None"}));
        assert_eq!((got.search, got.screenshot), (None, None));
    }

    #[test]
    fn a_row_repeating_an_earlier_row_is_off() {
        let v = json!({"search":"Ctrl+Alt+K","clipboard":"ctrl+alt+k","devices":"Win+D",
                       "arrange":"Win+D","screenshot":"PrtSc"});
        let got = Hotkeys::from_json(&v);
        assert_eq!(got.search, Some(hk("Ctrl+Alt+K")));
        assert_eq!(got.clipboard, None);
        assert_eq!(got.devices, Some(hk("Win+D")));
        assert_eq!(got.arrange, None);
        assert_eq!(got.screenshot, Some(hk("PrtSc")));
        // A missing row's default is an earlier row's too.
        let got = Hotkeys::from_json(&json!({"clipboard":"Ctrl+Alt+Space"}));
        assert_eq!(got.clipboard, None);
        assert_eq!(got.search, HotkeyId::Search.default());
    }

    #[test]
    fn the_lyrics_card_row_reads_leniently() {
        let l = Some(hk("Ctrl+Alt+L"));
        // An old config without the key: the default.
        assert_eq!(
            Hotkeys::from_json(&json!({"search": "Ctrl+Alt+K"})).lyrics_card,
            l
        );
        assert_eq!(Hotkeys::from_json(&json!({})).lyrics_card, l);
        // A deliberate off.
        assert_eq!(
            Hotkeys::from_json(&json!({"lyrics_card": "None"})).lyrics_card,
            None
        );
        assert_eq!(
            Hotkeys::from_json(&json!({"lyrics_card": null})).lyrics_card,
            None
        );
        // Garbage, a refused combo and a wrong type: the default.
        for bad in [json!("Banana"), json!("Alt+Tab"), json!("L"), json!(7)] {
            let got = Hotkeys::from_json(&json!({ "lyrics_card": bad }));
            assert_eq!(got.lyrics_card, l, "{bad}");
        }
        // A combo of its own survives.
        let got = Hotkeys::from_json(&json!({"lyrics_card": "Win+Alt+L"}));
        assert_eq!(got.lyrics_card, Some(hk("Win+Alt+L")));
        // A combo an earlier row has reads as off (the earlier row keeps it).
        let got = Hotkeys::from_json(&json!({"devices": "Ctrl+Alt+L"}));
        assert_eq!(got.devices, l);
        assert_eq!(got.lyrics_card, None);
        let got = Hotkeys::from_json(
            &json!({"screenshot": "Ctrl+Alt+L", "lyrics_card": "ctrl+alt+l"}),
        );
        assert_eq!(got.lyrics_card, None);
        // The default does not collide with another row's default.
        let d = Hotkeys::default();
        for id in HotkeyId::ALL {
            if id != HotkeyId::LyricsCard {
                assert_ne!(d.get(id), d.lyrics_card, "{id:?}");
            }
        }
    }

    #[test]
    fn anything_that_is_not_an_object_gives_every_default() {
        for v in [
            json!(null),
            json!(5),
            json!("Ctrl+Alt+V"),
            json!([1, 2]),
            json!(true),
        ] {
            assert_eq!(Hotkeys::from_json(&v), Hotkeys::default(), "{v}");
            let h: Hotkeys = serde_json::from_value(v).unwrap();
            assert_eq!(h, Hotkeys::default());
        }
    }

    #[test]
    fn record_step_reads_one_press() {
        let none = Mods::default();
        let ctrl = mods(true, false, false, false);
        for vk in [0x10, 0x11, 0x12, 0xA0, 0xA3, 0xA4, 0x5B, 0x5C] {
            assert_eq!(record_step(vk, ctrl), Step::Ignore, "{vk:#x}");
            assert_eq!(record_step(vk, none), Step::Ignore, "{vk:#x}");
        }
        assert_eq!(record_step(VK_ESCAPE, none), Step::Cancel);
        assert_eq!(record_step(VK_BACK, none), Step::Clear);
        assert_eq!(record_step(VK_DELETE, none), Step::Clear);
        // With a modifier they are ordinary combos and `check` judges them.
        for vk in [VK_ESCAPE, VK_BACK, VK_DELETE] {
            assert_eq!(
                record_step(vk, ctrl),
                Step::Combo(Hotkey { mods: ctrl, vk })
            );
        }
        assert_eq!(record_step(0x41, ctrl), Step::Combo(hk("Ctrl+A")));
        assert_eq!(record_step(0x2C, none), Step::Combo(hk("PrtSc")));
        let shift = mods(false, false, true, false);
        assert_eq!(
            record_step(VK_ESCAPE, shift),
            Step::Combo(Hotkey {
                mods: shift,
                vk: VK_ESCAPE
            })
        );
    }

    #[test]
    fn register_hotkey_bits() {
        assert_eq!(register_mods(hk("Ctrl+Alt+V")), 2 | 1 | 0x4000);
        assert_eq!(register_mods(hk("Win+Shift+F13")), 8 | 4 | 0x4000);
        assert_eq!(register_mods(hk("PrtSc")), 0x4000);
        assert_eq!(register_mods(hk("Alt+F1")), 1 | 0x4000);
        // Ctrl turns the Pause key into VK_CANCEL: that is the code to register.
        assert_eq!(register_vk(hk("Ctrl+Pause")), VK_CANCEL);
        assert_eq!(register_vk(hk("Ctrl+Alt+Pause")), VK_CANCEL);
        assert_eq!(register_vk(hk("Pause")), VK_PAUSE);
        assert_eq!(register_vk(hk("Alt+Pause")), VK_PAUSE);
        assert_eq!(register_vk(hk("Ctrl+V")), 0x56);
        assert_eq!(hk("Ctrl+Pause").to_string(), "Ctrl+Pause");
        use windows::Win32::UI::Input::KeyboardAndMouse as k;
        assert_eq!(MOD_ALT, k::MOD_ALT.0);
        assert_eq!(MOD_CONTROL, k::MOD_CONTROL.0);
        assert_eq!(MOD_SHIFT, k::MOD_SHIFT.0);
        assert_eq!(MOD_WIN, k::MOD_WIN.0);
        assert_eq!(MOD_NOREPEAT, k::MOD_NOREPEAT.0);
        // The key codes match the crate's too.
        assert_eq!(VK_SNAPSHOT, k::VK_SNAPSHOT.0);
        assert_eq!(VK_SPACE, k::VK_SPACE.0);
        assert_eq!(VK_ESCAPE, k::VK_ESCAPE.0);
        assert_eq!(VK_PAUSE, k::VK_PAUSE.0);
        assert_eq!(VK_SCROLL, k::VK_SCROLL.0);
        assert_eq!(VK_CANCEL, k::VK_CANCEL.0);
        assert_eq!(VK_LWIN, k::VK_LWIN.0);
        assert_eq!(VK_RMENU, k::VK_RMENU.0);
        assert_eq!(VK_F4, k::VK_F4.0);
        assert_eq!(VK_DELETE, k::VK_DELETE.0);
        assert_eq!(VK_BACK, k::VK_BACK.0);
        assert_eq!(VK_TAB, k::VK_TAB.0);
        assert_eq!(VK_RETURN, k::VK_RETURN.0);
        assert!(uses_hook(hk("PrtSc")));
        assert!(uses_hook(hk("Ctrl+PrtSc")));
        assert!(!uses_hook(hk("Ctrl+Alt+V")));
    }

    #[test]
    fn legacy_clipboard_presets() {
        let of = |v: serde_json::Value| from_legacy(&v).clipboard;
        assert_eq!(
            of(json!({"clipboard":{"hotkey":"CtrlAltV"}})),
            Some(hk("Ctrl+Alt+V"))
        );
        assert_eq!(
            of(json!({"clipboard":{"hotkey":"WinAltV"}})),
            Some(hk("Win+Alt+V"))
        );
        assert_eq!(
            of(json!({"clipboard":{"hotkey":"ctrlaltv"}})),
            Some(hk("Ctrl+Alt+V"))
        );
        assert_eq!(of(json!({"clipboard":{"hotkey":"Off"}})), None);
        assert_eq!(of(json!({"clipboard":{"hotkey":"CtrlShiftV"}})), None);
        assert_eq!(of(json!({"clipboard":{"hotkey":5}})), None);
        assert_eq!(of(json!({"clipboard":{}})), None);
        assert_eq!(of(json!({})), None);
    }

    #[test]
    fn legacy_search_follows_the_new_key_then_the_old_one() {
        let of = |v: serde_json::Value| from_legacy(&v).search;
        let space = Some(hk("Ctrl+Alt+Space"));
        assert_eq!(of(json!({})), space);
        assert_eq!(of(json!({"quick_search_hotkey":"CtrlAltSpace"})), space);
        assert_eq!(of(json!({"quick_search_hotkey":"ctrlaltspace"})), space);
        assert_eq!(
            of(json!({"quick_search_hotkey":"WinAltS"})),
            Some(hk("Win+Alt+S"))
        );
        assert_eq!(of(json!({"quick_search_hotkey":"Off"})), None);
        assert_eq!(of(json!({"quick_search_hotkey":"AltSpace"})), None);
        assert_eq!(of(json!({"quick_search_hotkey":"Nope"})), None);
        assert_eq!(of(json!({"quick_search_hotkey":5})), None);
        // The new key wins over the 3.2.0 one.
        assert_eq!(
            of(json!({"search_hotkey":"WinAltS","quick_search_hotkey":"Off"})),
            None
        );
        // Only the 3.2.0 key: "Off" was never a choice there; anything else is.
        assert_eq!(of(json!({"search_hotkey":"Off"})), space);
        assert_eq!(
            of(json!({"search_hotkey":"WinAltS"})),
            Some(hk("Win+Alt+S"))
        );
        assert_eq!(of(json!({"search_hotkey":"CtrlAltSpace"})), space);
        assert_eq!(of(json!({"search_hotkey":"Typo"})), None);
        assert_eq!(of(json!({"search_hotkey":"off"})), None);
        assert_eq!(of(json!({"search_hotkey":7})), space);
    }

    #[test]
    fn legacy_devices_arrange_and_print_screen() {
        let k = |v: serde_json::Value| from_legacy(&v);
        assert_eq!(
            k(json!({"devices_hotkey":"CtrlAltD"})).devices,
            Some(hk("Ctrl+Alt+D"))
        );
        assert_eq!(
            k(json!({"devices_hotkey":"ctrlaltd"})).devices,
            Some(hk("Ctrl+Alt+D"))
        );
        assert_eq!(k(json!({"devices_hotkey":"Off"})).devices, None);
        assert_eq!(k(json!({"devices_hotkey":"WinAltD"})).devices, None);
        assert_eq!(k(json!({})).devices, None);
        assert_eq!(
            k(json!({"arrange":{"hotkey":"CtrlAltS"}})).arrange,
            Some(hk("Ctrl+Alt+S"))
        );
        assert_eq!(
            k(json!({"arrange":{"hotkey":"CTRLALTS"}})).arrange,
            Some(hk("Ctrl+Alt+S"))
        );
        assert_eq!(k(json!({"arrange":{"hotkey":"Off"}})).arrange, None);
        assert_eq!(k(json!({"arrange":{"hotkey":"x"}})).arrange, None);
        assert_eq!(k(json!({"arrange":{}})).arrange, None);
        let snap = Some(hk("PrtSc"));
        assert_eq!(
            k(json!({"screenshot":{"print_screen":false}})).screenshot,
            None
        );
        assert_eq!(
            k(json!({"screenshot":{"print_screen":true}})).screenshot,
            snap
        );
        assert_eq!(k(json!({"screenshot":{}})).screenshot, snap);
        assert_eq!(k(json!({})).screenshot, snap);
        for odd in [json!("no"), json!(0), json!(null), json!([false])] {
            assert_eq!(
                k(json!({"screenshot":{"print_screen":odd}})).screenshot,
                snap
            );
        }
        assert_eq!(from_legacy(&json!({})), Hotkeys::default());
        assert_eq!(from_legacy(&json!(5)), Hotkeys::default());
    }
}
