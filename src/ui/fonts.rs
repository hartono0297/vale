//! The Studio typefaces (DM Sans, DM Mono, JetBrains Mono), embedded in the exe as RCDATA
//! resources and loaded from memory: for DirectWrite as a custom font
//! collection, for the remaining GDI windows as process-private fonts. When a
//! face is missing (no resource, a load error), every caller falls back to the
//! Segoe / Cascadia families the app used before.
//!
//! Display text (headings, the big clock, the song title) is DM Sans 600
//! upright, tracked tighter by `DISPLAY_TRACKING_EM` (see `apply_tracking`).
//!
//! The UI typeface is an option (`UiFont`): Sans is the DM Sans above, Mono
//! sets the text and display roles in JetBrains Mono, with no tracking (a
//! monospace cell is designed spacing). Code stays DM Mono in both.

use std::sync::OnceLock;

use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_TEXT_RANGE,
    IDWriteFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteFontCollection1,
    IDWriteInMemoryFontFileLoader, IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1,
    IDWriteTypography,
};
use windows::Win32::Graphics::Gdi::{
    AddFontMemResourceEx, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DEFAULT_QUALITY,
    DeleteObject, FW_NORMAL, GetDC, GetTextFaceW, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, ReleaseDC,
    SelectObject,
};
use windows::Win32::System::LibraryLoader::{
    FindResourceW, LoadResource, LockResource, SizeofResource,
};
use windows::core::{BOOL, HSTRING, IUnknown, Interface, PCWSTR, Result, w};

use crate::config::UiFont;

pub const DM_SANS: &str = "DM Sans";
pub const DM_MONO: &str = "DM Mono";
pub const JETBRAINS_MONO: &str = "JetBrains Mono";

/// RCDATA ids in `app.rc`: DM Sans 400/500/600 (203-205), DM Mono 400/500
/// (206-207), JetBrains Mono 400/500/600 (208-210).
pub const FONT_RESOURCE_IDS: [u16; 8] = [203, 204, 205, 206, 207, 208, 209, 210];

/// Display text is tracked this much tighter, in em (the design's
/// `letter-spacing: -.025em`).
pub const DISPLAY_TRACKING_EM: f32 = -0.025;

/// The display tracking for `ui`, in em: Sans is tighter by
/// `DISPLAY_TRACKING_EM`; Mono has none, because negative tracking cramps
/// monospace cells. Draw and measure both read it, and apply nothing at 0.
pub fn display_tracking_em(ui: UiFont) -> f32 {
    match ui {
        UiFont::Sans => DISPLAY_TRACKING_EM,
        UiFont::Mono => 0.0,
    }
}

/// `RT_RCDATA` (`MAKEINTRESOURCE(10)`).
const RT_RCDATA: PCWSTR = PCWSTR(10 as _);

const TEXT_FALLBACK: &[&str] = &["Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI"];
const DISPLAY_FALLBACK: &[&str] = &["Segoe UI Variable Display", "Segoe UI Variable", "Segoe UI"];
const MONO_FALLBACK: &[&str] = &["Cascadia Mono", "Consolas"];

/// What a piece of text is set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Body text and captions: DM Sans.
    Text,
    /// Headings, the big clock and the song title: DM Sans 600 upright,
    /// tracked by `DISPLAY_TRACKING_EM` (16 px and up only).
    Display,
    /// Code: DM Mono.
    Mono,
}

/// Which embedded families resolved in the custom collection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    pub dm_sans: bool,
    pub dm_mono: bool,
    pub jetbrains_mono: bool,
}

/// The family a role is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub family: &'static str,
    /// Look the family up in the embedded collection (else the system one).
    pub embedded: bool,
}

/// The embedded family for `role` when it loaded, else the first of the
/// fallback chain that `installed` reports (the chain's last name when none
/// is, leaving the rest to DirectWrite's own font fallback).
pub fn choose(role: Role, loaded: Loaded, installed: impl Fn(&str) -> bool) -> Choice {
    choose_in(role, UiFont::Sans, loaded, installed)
}

/// `choose` for the UI typeface `ui`. Sans is exactly `choose`. With Mono the
/// text and display roles give JetBrains Mono when it loaded, else DM Sans
/// when that loaded, else the Segoe chain; code (`Role::Mono`) is DM Mono in
/// both.
pub fn choose_in(
    role: Role,
    ui: UiFont,
    loaded: Loaded,
    installed: impl Fn(&str) -> bool,
) -> Choice {
    let (family, ok, fallback) = match role {
        Role::Text | Role::Display => {
            let fallback = if role == Role::Text {
                TEXT_FALLBACK
            } else {
                DISPLAY_FALLBACK
            };
            if ui == UiFont::Mono && loaded.jetbrains_mono {
                (JETBRAINS_MONO, true, fallback)
            } else {
                (DM_SANS, loaded.dm_sans, fallback)
            }
        }
        Role::Mono => (DM_MONO, loaded.dm_mono, MONO_FALLBACK),
    };
    if ok {
        return Choice {
            family,
            embedded: true,
        };
    }
    let last = fallback[fallback.len() - 1];
    Choice {
        family: fallback
            .iter()
            .copied()
            .find(|f| installed(f))
            .unwrap_or(last),
        embedded: false,
    }
}

// ---- Resources ----

/// The bytes of RCDATA resource `id` in the exe, `None` when it is absent.
fn resource(id: u16) -> Option<&'static [u8]> {
    unsafe {
        let info = FindResourceW(None, PCWSTR(id as usize as *const u16), RT_RCDATA);
        if info.is_invalid() {
            return None;
        }
        let size = SizeofResource(None, info);
        let handle = LoadResource(None, info).ok()?;
        let data = LockResource(handle);
        if data.is_null() || size == 0 {
            return None;
        }
        // The exe's resources stay mapped for the life of the process.
        Some(std::slice::from_raw_parts(data.cast::<u8>(), size as usize))
    }
}

/// The resources among `ids` that exist; missing ones are skipped.
fn resources(ids: &[u16]) -> Vec<&'static [u8]> {
    ids.iter().filter_map(|&id| resource(id)).collect()
}

/// The embedded font files, read once.
fn blobs() -> &'static [&'static [u8]] {
    static BLOBS: OnceLock<Vec<&'static [u8]>> = OnceLock::new();
    BLOBS.get_or_init(|| resources(&FONT_RESOURCE_IDS))
}

// ---- DirectWrite ----

/// DirectWrite objects made on the shared factory, kept for the whole process.
/// The shared factory and the immutable collections it hands out are
/// thread-safe; in practice they are only used from the UI thread.
struct Shared<T>(T);
// SAFETY: see above; the wrapped objects are never mutated after creation.
unsafe impl<T> Send for Shared<T> {}
unsafe impl<T> Sync for Shared<T> {}

/// The in-memory loader, registered once on the shared factory and never
/// unregistered: text formats made from its fonts may live until exit.
fn loader(factory: &IDWriteFactory5) -> Option<&'static IDWriteInMemoryFontFileLoader> {
    static LOADER: OnceLock<Shared<Option<IDWriteInMemoryFontFileLoader>>> = OnceLock::new();
    let loader = LOADER.get_or_init(|| unsafe {
        let made = factory
            .CreateInMemoryFontFileLoader()
            .ok()
            .filter(|loader| factory.RegisterFontFileLoader(loader).is_ok());
        Shared(made)
    });
    loader.0.as_ref()
}

/// A collection of the fonts in `files` (raw OpenType bytes, copied by
/// DirectWrite), `None` when not one of them could be added.
fn build_collection(dwrite: &IDWriteFactory, files: &[&[u8]]) -> Option<IDWriteFontCollection1> {
    let factory: IDWriteFactory5 = dwrite.cast().ok()?;
    let loader = loader(&factory)?;
    unsafe {
        let builder = factory.CreateFontSetBuilder().ok()?;
        let mut added = 0;
        for file in files {
            let Ok(len) = u32::try_from(file.len()) else {
                continue;
            };
            let Ok(reference) = loader.CreateInMemoryFontFileReference(
                dwrite,
                file.as_ptr().cast(),
                len,
                None::<&IUnknown>,
            ) else {
                continue;
            };
            // DWRITE_E_FILEFORMAT for anything that is not an OpenType file.
            if builder.AddFontFile(&reference).is_ok() {
                added += 1;
            }
        }
        if added == 0 {
            return None;
        }
        let set = builder.CreateFontSet().ok()?;
        factory.CreateFontCollectionFromFontSet(&set).ok()
    }
}

fn has_family(collection: &IDWriteFontCollection, name: &str) -> bool {
    let (mut index, mut exists) = (0u32, BOOL(0));
    let found = unsafe { collection.FindFamilyName(&HSTRING::from(name), &mut index, &mut exists) };
    found.is_ok() && exists.as_bool()
}

/// The embedded collection and which families resolved in it.
pub struct Embedded {
    collection: Option<IDWriteFontCollection>,
    pub loaded: Loaded,
}

impl Embedded {
    fn build(dwrite: &IDWriteFactory, files: &[&[u8]]) -> Self {
        let collection = build_collection(dwrite, files).and_then(|c| c.cast().ok());
        let has = |name| collection.as_ref().is_some_and(|c| has_family(c, name));
        let loaded = Loaded {
            dm_sans: has(DM_SANS),
            dm_mono: has(DM_MONO),
            jetbrains_mono: has(JETBRAINS_MONO),
        };
        Self { collection, loaded }
    }

    pub fn collection(&self) -> Option<&IDWriteFontCollection> {
        self.collection.as_ref()
    }
}

/// The embedded fonts on the shared factory `dwrite`, loaded on first use.
pub fn embedded(dwrite: &IDWriteFactory) -> &'static Embedded {
    static EMBEDDED: OnceLock<Shared<Embedded>> = OnceLock::new();
    &EMBEDDED
        .get_or_init(|| Shared(Embedded::build(dwrite, blobs())))
        .0
}

/// `name` is in the system font collection.
pub fn system_has(dwrite: &IDWriteFactory, name: &str) -> bool {
    unsafe {
        let mut collection = None;
        if dwrite
            .GetSystemFontCollection(&mut collection, false)
            .is_err()
        {
            return false;
        }
        collection.is_some_and(|c| has_family(&c, name))
    }
}

/// A family ready for `CreateTextFormat`.
pub struct Face {
    pub family: HSTRING,
    /// `None`: the system collection.
    pub collection: Option<IDWriteFontCollection>,
    /// Every role is upright: `DWRITE_FONT_STYLE_NORMAL`.
    pub style: DWRITE_FONT_STYLE,
}

impl Face {
    /// The face `role` is drawn in, embedded or fallback.
    pub fn for_role(dwrite: &IDWriteFactory, role: Role) -> Face {
        Face::for_role_in(dwrite, role, UiFont::Sans)
    }

    /// The face `role` is drawn in for the UI typeface `ui` (`choose_in`).
    pub fn for_role_in(dwrite: &IDWriteFactory, role: Role, ui: UiFont) -> Face {
        let fonts = embedded(dwrite);
        let choice = choose_in(role, ui, fonts.loaded, |name| system_has(dwrite, name));
        Face {
            family: HSTRING::from(choice.family),
            collection: if choice.embedded {
                fonts.collection().cloned()
            } else {
                None
            },
            style: DWRITE_FONT_STYLE_NORMAL,
        }
    }

    pub fn format(
        &self,
        dwrite: &IDWriteFactory,
        weight: DWRITE_FONT_WEIGHT,
        size: f32,
    ) -> Result<IDWriteTextFormat> {
        unsafe {
            dwrite.CreateTextFormat(
                &self.family,
                self.collection.as_ref(),
                weight,
                self.style,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )
        }
    }
}

/// Tabular (fixed-width) figures, so times and counters do not jitter.
/// Made once per renderer and shared by every layout.
pub fn tabular_figures(dwrite: &IDWriteFactory) -> Result<IDWriteTypography> {
    unsafe {
        let typography = dwrite.CreateTypography()?;
        typography.AddFontFeature(DWRITE_FONT_FEATURE {
            nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
            parameter: 1,
        })?;
        Ok(typography)
    }
}

/// `text` laid out in a `width` × `height` box with `format` (its alignment,
/// wrapping and trimming) and `typography` on each run of ASCII digits only.
/// The same box `DrawText` would use, but able to carry OpenType features.
///
/// Only the digits get the features: DM Sans's `tnum` also widens the space,
/// so applying it to the whole string opens wide gaps between words. Times
/// and dates keep fixed-width figures because all their digits are covered.
pub fn text_layout(
    dwrite: &IDWriteFactory,
    text: &[u16],
    format: &IDWriteTextFormat,
    width: f32,
    height: f32,
    typography: &IDWriteTypography,
) -> Result<IDWriteTextLayout> {
    unsafe {
        let layout = dwrite.CreateTextLayout(text, format, width.max(0.0), height.max(0.0))?;
        for (start, length) in digit_runs(text) {
            layout.SetTypography(
                typography,
                DWRITE_TEXT_RANGE {
                    startPosition: start,
                    length,
                },
            )?;
        }
        Ok(layout)
    }
}

/// Tracks the first `len` UTF-16 units of `layout` by `DISPLAY_TRACKING_EM`
/// of the font `size` (trailing space after each character, so the line
/// starts where it did). Display text only; both the draw and the measure
/// of a line must call it, or measured widths stop matching what is drawn.
/// The Sans tracking: the editors, which stay DM Sans, call this.
pub fn apply_tracking(layout: &IDWriteTextLayout, len: u32, size: f32) -> Result<()> {
    apply_tracking_in(layout, len, size, UiFont::Sans)
}

/// `apply_tracking` for the UI typeface `ui` (`display_tracking_em`): nothing
/// is applied when it is 0 (Mono).
///
/// Character spacing needs `IDWriteTextLayout1` (Windows 8 and up); where
/// the cast fails the text stays untracked and this returns `Ok`.
pub fn apply_tracking_in(
    layout: &IDWriteTextLayout,
    len: u32,
    size: f32,
    ui: UiFont,
) -> Result<()> {
    let em = display_tracking_em(ui);
    if em == 0.0 {
        return Ok(());
    }
    let Ok(layout) = layout.cast::<IDWriteTextLayout1>() else {
        return Ok(());
    };
    unsafe {
        layout.SetCharacterSpacing(
            0.0,
            size * em,
            0.0,
            DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: len,
            },
        )
    }
}

/// Each run of ASCII digits in `text` as `(start, length)` in UTF-16 units,
/// in order. Lazy, so a layout pays no allocation for it.
pub fn digit_runs(text: &[u16]) -> impl Iterator<Item = (u32, u32)> + '_ {
    let is_digit = |c: &u16| (u16::from(b'0')..=u16::from(b'9')).contains(c);
    let mut i = 0;
    std::iter::from_fn(move || {
        let start = i + text[i..].iter().position(is_digit)?;
        let len = text[start..]
            .iter()
            .position(|c| !is_digit(c))
            .unwrap_or(text.len() - start);
        i = start + len;
        Some((start as u32, len as u32))
    })
}

// ---- GDI ----

/// Which embedded families GDI can use after `register_gdi`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GdiFonts {
    pub dm_sans: bool,
    pub dm_mono: bool,
    pub jetbrains_mono: bool,
}

static GDI: OnceLock<GdiFonts> = OnceLock::new();

/// Adds the embedded fonts to this process's GDI font table. Call once at
/// startup, before any window is created; later calls return the first result.
pub fn register_gdi() -> GdiFonts {
    *GDI.get_or_init(|| {
        let files = blobs();
        let mut installed = 0u32;
        for file in files {
            let Ok(len) = u32::try_from(file.len()) else {
                continue;
            };
            let mut count = 0u32;
            // GDI writes the font count through this pointer (the binding
            // types it const). The handle is never released: the fonts stay
            // for the process.
            let count_out = (&raw mut count).cast_const();
            let handle =
                unsafe { AddFontMemResourceEx(file.as_ptr().cast(), len, None, count_out) };
            if handle.is_invalid() {
                eprintln!("vale: cannot register an embedded font with GDI");
            } else {
                installed += count;
            }
        }
        if installed == 0 {
            return GdiFonts::default();
        }
        let fonts = GdiFonts {
            dm_sans: gdi_has(DM_SANS),
            dm_mono: gdi_has(DM_MONO),
            jetbrains_mono: gdi_has(JETBRAINS_MONO),
        };
        if !fonts.dm_sans || !fonts.dm_mono || !fonts.jetbrains_mono {
            eprintln!("vale: embedded fonts incomplete for GDI: {fonts:?}");
        }
        fonts
    })
}

/// What `register_gdi` found (nothing before it ran).
pub fn gdi() -> GdiFonts {
    GDI.get().copied().unwrap_or_default()
}

/// The GDI face for UI text: DM Sans when registered, else Segoe UI.
pub fn gdi_text_face() -> PCWSTR {
    if gdi().dm_sans {
        w!("DM Sans")
    } else {
        w!("Segoe UI")
    }
}

/// The GDI face for UI text in the typeface `ui`: JetBrains Mono for Mono
/// when registered, else what `gdi_text_face` gives (DM Sans, then Segoe UI).
pub fn gdi_text_face_for(ui: UiFont) -> PCWSTR {
    if ui == UiFont::Mono && gdi().jetbrains_mono {
        w!("JetBrains Mono")
    } else {
        gdi_text_face()
    }
}

/// GDI resolves `name` to that face rather than substituting another.
fn gdi_has(name: &str) -> bool {
    unsafe {
        let font = CreateFontW(
            -13,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            DEFAULT_QUALITY,
            0,
            &HSTRING::from(name),
        );
        if font.is_invalid() {
            return false;
        }
        let got = gdi_face(font);
        let _ = DeleteObject(HGDIOBJ(font.0));
        got.eq_ignore_ascii_case(name)
    }
}

/// The face GDI actually picked for `font`.
pub fn gdi_face(font: HFONT) -> String {
    unsafe {
        let dc = GetDC(None);
        let old = SelectObject(dc, HGDIOBJ(font.0));
        let mut name = [0u16; 64];
        let n = GetTextFaceW(dc, Some(&mut name)).max(1) as usize - 1;
        SelectObject(dc, old);
        ReleaseDC(None, dc);
        String::from_utf16_lossy(&name[..n.min(name.len())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::DirectWrite::{
        DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
        DWriteCreateFactory,
    };

    fn factory() -> IDWriteFactory {
        unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("shared factory") }
    }

    #[test]
    fn fallbacks_when_nothing_loaded() {
        let none = Loaded::default();
        let all = |_: &str| true;
        let text = choose(Role::Text, none, all);
        assert_eq!(text.family, "Segoe UI Variable Text");
        assert!(!text.embedded);
        let display = choose(Role::Display, none, all);
        assert_eq!(display.family, "Segoe UI Variable Display");
        assert!(!display.embedded);
        assert_eq!(choose(Role::Mono, none, all).family, "Cascadia Mono");
        // Older systems: the chain moves on, ending on its last name.
        let only_segoe = |f: &str| f == "Segoe UI";
        assert_eq!(choose(Role::Text, none, only_segoe).family, "Segoe UI");
        assert_eq!(choose(Role::Display, none, only_segoe).family, "Segoe UI");
        assert_eq!(choose(Role::Mono, none, |_| false).family, "Consolas");
    }

    #[test]
    fn embedded_names_when_loaded() {
        let loaded = Loaded {
            dm_sans: true,
            dm_mono: true,
            jetbrains_mono: true,
        };
        let pick = |role| choose(role, loaded, |_| true);
        let dm_sans = Choice {
            family: DM_SANS,
            embedded: true,
        };
        assert_eq!(pick(Role::Text), dm_sans);
        // Display is DM Sans too (600, tracked): only weight and spacing differ.
        assert_eq!(pick(Role::Display), dm_sans);
        assert_eq!(pick(Role::Mono).family, DM_MONO);
        // One family missing falls back alone.
        let partial = Loaded {
            dm_sans: false,
            ..loaded
        };
        let text = choose(Role::Text, partial, |_| true);
        assert!(!text.embedded);
        let display = choose(Role::Display, partial, |_| true);
        assert_eq!(display.family, "Segoe UI Variable Display");
        assert!(!display.embedded);
        assert!(choose(Role::Mono, partial, |_| true).embedded);
    }

    #[test]
    fn sans_is_exactly_the_old_choice_and_mono_prefers_jetbrains_mono() {
        let all = Loaded {
            dm_sans: true,
            dm_mono: true,
            jetbrains_mono: true,
        };
        let cases = [
            all,
            Loaded::default(),
            Loaded {
                jetbrains_mono: false,
                ..all
            },
            Loaded {
                dm_sans: false,
                ..all
            },
            Loaded {
                dm_mono: false,
                ..all
            },
        ];
        for loaded in cases {
            for role in [Role::Text, Role::Display, Role::Mono] {
                for installed in [true, false] {
                    let installed = move |_: &str| installed;
                    assert_eq!(
                        choose_in(role, UiFont::Sans, loaded, installed),
                        choose(role, loaded, installed),
                        "{role:?} {loaded:?}"
                    );
                }
            }
        }
        let jbm = Choice {
            family: JETBRAINS_MONO,
            embedded: true,
        };
        let mono = |role, loaded| choose_in(role, UiFont::Mono, loaded, |_| true);
        assert_eq!(mono(Role::Text, all), jbm);
        assert_eq!(mono(Role::Display, all), jbm);
        // Code stays DM Mono in both.
        assert_eq!(mono(Role::Mono, all).family, DM_MONO);
        // JetBrains Mono missing: DM Sans when loaded, else the Segoe chain.
        let no_jbm = Loaded {
            jetbrains_mono: false,
            ..all
        };
        assert_eq!(mono(Role::Text, no_jbm).family, DM_SANS);
        assert_eq!(mono(Role::Display, no_jbm).family, DM_SANS);
        let none = Loaded::default();
        let text = choose_in(Role::Text, UiFont::Mono, none, |_| true);
        assert_eq!(text.family, "Segoe UI Variable Text");
        assert!(!text.embedded);
        let display = choose_in(Role::Display, UiFont::Mono, none, |_| true);
        assert_eq!(display.family, "Segoe UI Variable Display");
        assert_eq!(
            choose_in(Role::Display, UiFont::Mono, none, |f| f == "Segoe UI").family,
            "Segoe UI"
        );
    }

    #[test]
    fn mono_has_no_display_tracking() {
        assert_eq!(display_tracking_em(UiFont::Sans), DISPLAY_TRACKING_EM);
        assert_eq!(display_tracking_em(UiFont::Mono), 0.0);
        let dwrite = factory();
        if !embedded(&dwrite).loaded.jetbrains_mono {
            eprintln!("skipped: JetBrains Mono is not in this binary");
            return;
        }
        let face = Face::for_role_in(&dwrite, Role::Display, UiFont::Mono);
        assert_eq!(face.family.to_string(), JETBRAINS_MONO);
        let size = 20.0;
        let format = face
            .format(&dwrite, DWRITE_FONT_WEIGHT_SEMI_BOLD, size)
            .expect("format");
        let typography = tabular_figures(&dwrite).expect("typography");
        let text: Vec<u16> = "Oktober 2026".encode_utf16().collect();
        let width = |ui: Option<UiFont>| unsafe {
            let layout =
                text_layout(&dwrite, &text, &format, 4096.0, 64.0, &typography).expect("layout");
            if let Some(ui) = ui {
                apply_tracking_in(&layout, text.len() as u32, size, ui).expect("tracking");
            }
            let mut m = Default::default();
            layout.GetMetrics(&mut m).expect("metrics");
            m.widthIncludingTrailingWhitespace
        };
        // Mono applies nothing; 12 cells of 0.6 em.
        assert_eq!(width(None), width(Some(UiFont::Mono)));
        assert!((width(None) - 12.0 * 0.6 * size).abs() < 0.1, "{}", width(None));
        assert!(width(Some(UiFont::Sans)) < width(None));
    }

    #[test]
    fn jetbrains_mono_keeps_spaces_and_draws_no_ligatures() {
        let dwrite = factory();
        if !embedded(&dwrite).loaded.jetbrains_mono {
            eprintln!("skipped: JetBrains Mono is not in this binary");
            return;
        }
        let face = Face::for_role_in(&dwrite, Role::Text, UiFont::Mono);
        assert_eq!(face.family.to_string(), JETBRAINS_MONO);
        let format = face
            .format(&dwrite, DWRITE_FONT_WEIGHT_NORMAL, 13.0)
            .expect("format");
        let typography = tabular_figures(&dwrite).expect("typography");
        let plain = unsafe { dwrite.CreateTypography() }.expect("plain");
        let width = |s: &str, typography: &IDWriteTypography| unsafe {
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout =
                text_layout(&dwrite, &wide, &format, 400.0, 20.0, typography).expect("layout");
            let mut m = Default::default();
            layout.GetMetrics(&mut m).expect("metrics");
            m.widthIncludingTrailingWhitespace
        };
        // Text without digits measures as with no features at all, and a
        // space is one cell like any other character.
        for s in ["Minum vitamin", "a b  c", " "] {
            assert_eq!(width(s, &typography), width(s, &plain), "{s:?}");
        }
        let cell = width("m", &typography);
        assert!((width("a b", &typography) - 3.0 * cell).abs() < 0.01);
        assert!((width(" ", &typography) - cell).abs() < 0.01);
        // Digits are fixed width without the feature.
        assert_eq!(width("1111", &plain), width("0000", &plain));
        assert_eq!(width("12:34", &typography), width("00:00", &typography));
        // No ligatures: "->" is "-" plus ">", "==" and "!=" likewise, and
        // "www" is three cells.
        for (pair, a, b) in [("->", "-", ">"), ("==", "=", "="), ("!=", "!", "="), ("<=", "<", "=")]
        {
            let joined = width(pair, &typography);
            let apart = width(a, &typography) + width(b, &typography);
            assert!((joined - apart).abs() < 0.01, "{pair:?}: {joined} vs {apart}");
        }
        assert!((width("www", &typography) - 3.0 * width("w", &typography)).abs() < 0.01);
    }

    #[test]
    fn tracking_narrows_display_text() {
        let dwrite = factory();
        let face = Face::for_role(&dwrite, Role::Display);
        let size = 20.0;
        let format = face
            .format(&dwrite, DWRITE_FONT_WEIGHT_SEMI_BOLD, size)
            .expect("format");
        let typography = tabular_figures(&dwrite).expect("typography");
        let wide: Vec<u16> = "Oktober 2026".encode_utf16().collect();
        let len = wide.len() as u32;
        let width = |tracked: bool| unsafe {
            let layout =
                text_layout(&dwrite, &wide, &format, 4096.0, 64.0, &typography).expect("layout");
            if tracked {
                apply_tracking(&layout, len, size).expect("tracking");
            }
            let mut m = Default::default();
            layout.GetMetrics(&mut m).expect("metrics");
            m.widthIncludingTrailingWhitespace
        };
        let (plain, tracked) = (width(false), width(true));
        let expected = len as f32 * size * -DISPLAY_TRACKING_EM;
        assert!(
            ((plain - tracked) - expected).abs() < 0.5,
            "plain {plain}, tracked {tracked}, expected {expected} narrower"
        );
    }

    #[test]
    fn garbage_bytes_build_nothing() {
        let dwrite = factory();
        let junk = [0x5Au8; 512];
        assert!(build_collection(&dwrite, &[&junk, &[]]).is_none());
        let fonts = Embedded::build(&dwrite, &[&junk]);
        assert_eq!(fonts.loaded, Loaded::default());
        assert!(fonts.collection().is_none());
        assert!(build_collection(&dwrite, &[]).is_none());
    }

    #[test]
    fn system_font_bytes_resolve_in_the_custom_collection() {
        let path = std::path::Path::new(r"C:\Windows\Fonts\consola.ttf");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipped: {} not readable", path.display());
            return;
        };
        let dwrite = factory();
        // Built twice: the loader is registered once and reused.
        for _ in 0..2 {
            let collection = build_collection(&dwrite, &[&bytes]).expect("collection");
            let collection: IDWriteFontCollection = collection.cast().expect("base interface");
            assert!(has_family(&collection, "Consolas"));
            assert!(!has_family(&collection, DM_SANS));
            let face = Face {
                family: HSTRING::from("Consolas"),
                collection: Some(collection),
                style: DWRITE_FONT_STYLE_NORMAL,
            };
            let format = face
                .format(&dwrite, DWRITE_FONT_WEIGHT_NORMAL, 11.0)
                .expect("format");
            let wide: Vec<u16> = "12:34".encode_utf16().collect();
            let typography = tabular_figures(&dwrite).expect("typography");
            text_layout(&dwrite, &wide, &format, 100.0, 20.0, &typography).expect("layout");
        }
    }

    fn runs(s: &str) -> Vec<(u32, u32)> {
        let wide: Vec<u16> = s.encode_utf16().collect();
        digit_runs(&wide).collect()
    }

    #[test]
    fn digit_runs_cover_only_the_digits() {
        assert!(runs("").is_empty());
        assert!(runs("Minum vitamin").is_empty());
        assert_eq!(runs("14:32"), [(0, 2), (3, 2)]);
        assert_eq!(runs("Monday, 26 October"), [(8, 2)]);
        assert_eq!(runs("7 pm"), [(0, 1)]);
        assert_eq!(runs("Room 12"), [(5, 2)]);
        assert_eq!(runs("2026"), [(0, 4)]);
        assert_eq!(runs("a1b23c"), [(1, 1), (3, 2)]);
        // Offsets are UTF-16 units: "é" is one unit, the emoji two.
        assert_eq!(runs("é 9"), [(2, 1)]);
        assert_eq!(runs("\u{1F345} 25:00"), [(3, 2), (6, 2)]);
        // Non-ASCII digits are not figures DM Sans tabulates.
        assert!(runs("\u{0661}\u{FF12}").is_empty());
    }

    #[test]
    fn tabular_typography_lands_on_digits_only() {
        let dwrite = factory();
        let face = Face::for_role(&dwrite, Role::Text);
        let format = face
            .format(&dwrite, DWRITE_FONT_WEIGHT_NORMAL, 13.0)
            .expect("format");
        let typography = tabular_figures(&dwrite).expect("typography");
        let wide: Vec<u16> = "Mon, 26 Oct".encode_utf16().collect();
        let layout =
            text_layout(&dwrite, &wide, &format, 200.0, 20.0, &typography).expect("layout");
        let at = |pos: u32| unsafe {
            let mut typography = None;
            let mut range = DWRITE_TEXT_RANGE::default();
            layout
                .GetTypography(pos, &mut typography, Some(&mut range))
                .expect("typography query");
            typography.map(|t: IDWriteTypography| (t, range))
        };
        // The space and letters carry no typography; the digits do.
        assert!(at(0).is_none());
        assert!(at(4).is_none());
        let (_, range) = at(5).expect("digits are tabular");
        assert_eq!((range.startPosition, range.length), (5, 2));
        assert!(at(7).is_none());
        // Text without digits measures exactly as with no features at all.
        let width = |layout: IDWriteTextLayout| unsafe {
            let mut m = Default::default();
            layout.GetMetrics(&mut m).expect("metrics");
            m.widthIncludingTrailingWhitespace
        };
        let words: Vec<u16> = "Minum vitamin".encode_utf16().collect();
        let plain = unsafe { dwrite.CreateTypography() }.expect("plain");
        let tab = text_layout(&dwrite, &words, &format, 200.0, 20.0, &typography);
        let none = text_layout(&dwrite, &words, &format, 200.0, 20.0, &plain);
        assert_eq!(width(tab.expect("layout")), width(none.expect("layout")));
    }

    #[test]
    fn missing_resource_ids_are_skipped() {
        assert!(resource(65000).is_none());
        assert!(resources(&[65000, 65001, 1]).is_empty());
        // Whatever this binary embeds, reading the real ids never panics.
        assert!(resources(&FONT_RESOURCE_IDS).len() <= FONT_RESOURCE_IDS.len());
    }
}
