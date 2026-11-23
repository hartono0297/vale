//! The handwriting fonts a typed signature is set in: four bundled faces,
//! with the installed Ink Free / Segoe Script / Segoe Print as a fallback.

use std::sync::Mutex;

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory, IDWriteTextFormat,
};
use windows::core::{HSTRING, w};

use crate::ui::fonts::{has_family, sign_collection, system_has};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandFont {
    Slant,
    Thin,
    Print,
    Formal,
}

/// In tile order.
pub const ALL: [HandFont; 4] = [HandFont::Slant, HandFont::Thin, HandFont::Print, HandFont::Formal];

const INSTALLED: [&str; 3] = ["Ink Free", "Segoe Script", "Segoe Print"];

impl HandFont {
    /// The family name inside the bundled font file.
    pub fn family(self) -> &'static str {
        match self {
            HandFont::Slant => "Vale Sign Slant",
            HandFont::Thin => "Vale Sign Fine",
            HandFont::Print => "Vale Sign Print",
            HandFont::Formal => "Vale Sign Formal",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Choice {
    Embedded(&'static str),
    Installed(&'static str),
    None,
}

/// The bundled family when it loaded, else the first installed fallback.
fn choose(font: HandFont, embedded: bool, installed: impl Fn(&str) -> bool) -> Choice {
    if embedded {
        return Choice::Embedded(font.family());
    }
    INSTALLED
        .into_iter()
        .find(|name| installed(name))
        .map_or(Choice::None, Choice::Installed)
}

/// A text format for `font` at `size` px: normal weight and style, no wrap.
pub fn format(dwrite: &IDWriteFactory, font: HandFont, size: f32) -> Option<IDWriteTextFormat> {
    let collection = sign_collection(dwrite);
    let embedded = collection.is_some_and(|c| has_family(c, font.family()));
    let (family, collection) = match choose(font, embedded, |n| system_has(dwrite, n)) {
        Choice::Embedded(name) => (name, collection),
        Choice::Installed(name) => (name, None),
        Choice::None => return None,
    };
    unsafe {
        let format = dwrite
            .CreateTextFormat(
                &HSTRING::from(family),
                collection,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )
            .ok()?;
        let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        Some(format)
    }
}

/// Tests that set or depend on the last font take this lock (the static is
/// process-wide and tests run in parallel).
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

static LAST: Mutex<HandFont> = Mutex::new(HandFont::Slant);

/// The font last chosen in the signature maker; it starts as `Slant`.
pub fn last() -> HandFont {
    *LAST.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_last(f: HandFont) {
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = f;
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::DirectWrite::{DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory};

    #[test]
    fn every_hand_font_loads_from_the_exe() {
        let dwrite: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("factory") };
        for f in ALL {
            let collection = sign_collection(&dwrite).expect("collection");
            assert!(has_family(collection, f.family()), "{f:?}");
            assert!(format(&dwrite, f, 64.0).is_some(), "{f:?}");
        }
    }

    #[test]
    fn falls_back_to_installed_when_not_embedded() {
        for f in ALL {
            assert_eq!(
                choose(f, false, |n| n == "Segoe Script"),
                Choice::Installed("Segoe Script")
            );
            assert_eq!(choose(f, true, |_| false), Choice::Embedded(f.family()));
            assert_eq!(choose(f, false, |_| false), Choice::None);
        }
        assert_eq!(choose(HandFont::Slant, false, |_| true), Choice::Installed("Ink Free"));
    }

    #[test]
    fn the_last_font_starts_slanted_and_round_trips() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(last(), HandFont::Slant);
        set_last(HandFont::Formal);
        assert_eq!(last(), HandFont::Formal);
        set_last(HandFont::Slant);
        assert_eq!(last(), HandFont::Slant);
    }
}
