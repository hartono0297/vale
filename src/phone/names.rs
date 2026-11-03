//! File names that arrive from a phone: decoded, cleaned so that they cannot
//! leave the destination folder or name a Windows device, and numbered when
//! the name is taken. Pure.

use crate::i18n::Lang;

/// The longest name kept, in UTF-16 code units (what NTFS counts: a component
/// holds 255). 150 leaves room for a clash suffix like " (9999)" (7) and
/// ".part" (5) with a wide margin.
const MAX_UTF16: usize = 150;

/// Whether `c` is a format or bidi control the name must not carry: the Unicode
/// category Cf (std has no predicate for it, and no crate is added) plus the
/// line and paragraph separators. These draw nothing yet can reorder or hide
/// the text around them, as in `photo<RLO>gnp.exe` shown as `photoexe.png`.
///
/// - U+00AD soft hyphen; U+061C Arabic letter mark; U+180E Mongolian vowel
///   separator
/// - U+0600-0605, U+06DD, U+070F, U+0890-0891, U+08E2 Arabic and Syriac marks
/// - U+200B-U+200F zero width space, joiners and the left/right marks
/// - U+2028, U+2029 line and paragraph separators
/// - U+202A-U+202E embedding and override (RLO is U+202E)
/// - U+2060-U+2064 word joiner and invisible operators, U+2066-U+2069
///   isolates, U+206A-U+206F deprecated controls
/// - U+FEFF byte order mark, U+FFF9-U+FFFB interlinear annotation
/// - U+110BD, U+110CD, U+13430-1343F, U+1BCA0-1BCA3, U+1D173-1D17A,
///   U+E0001, U+E0020-U+E007F (the tag characters)
///
/// The zero width joiner (U+200D) is in the list, so an emoji ZWJ sequence
/// falls apart into its emoji: accepted, the name is only a label.
fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x2028..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// `name` in UTF-16 code units.
fn utf16_len(name: &str) -> usize {
    name.chars().map(char::len_utf16).sum()
}

/// What an empty name becomes.
fn fallback(lang: Lang) -> &'static str {
    match lang {
        Lang::Id => "berkas",
        Lang::En => "file",
    }
}

/// `s` with its `%XX` escapes decoded. A broken escape stays as it is; bytes
/// that are not UTF-8 become U+FFFD (the name is only ever a label).
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Windows device names that cannot be a file name, with any extension.
fn is_reserved(stem: &str) -> bool {
    let up = stem.trim_end_matches([' ', '.']).to_ascii_uppercase();
    match up.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            (up.len() == 4)
                && (up.starts_with("COM") || up.starts_with("LPT"))
                && matches!(up.as_bytes()[3], b'1'..=b'9')
        }
    }
}

/// The name to write for the percent-encoded `raw` from the phone.
pub fn sanitize(raw: &str, lang: Lang) -> String {
    let decoded = percent_decode(raw);
    // Only the part after the last separator: a path from the phone is a name.
    let base = decoded.rsplit(['/', '\\']).next().unwrap_or("");
    let mut name: String = base
        .chars()
        .filter(|&c| {
            !c.is_control()
                && !is_format(c)
                && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')
        })
        .collect();
    while name.contains("..") {
        name = name.replace("..", ".");
    }
    // Windows drops trailing dots and spaces; a leading dot stays (".env").
    let mut name = trim(&name).to_owned();
    // The extension, if any, is the part after the last dot.
    if is_reserved(name.split('.').next().unwrap_or("")) {
        let at = name.find('.').unwrap_or(name.len());
        name.insert(at, '_');
    }
    if utf16_len(&name) > MAX_UTF16 {
        name = cap(&name);
    }
    let name = trim(&name).to_owned();
    if name.is_empty() {
        fallback(lang).to_owned()
    } else {
        name
    }
}

/// `name` without leading spaces and without trailing dots and spaces.
fn trim(name: &str) -> &str {
    name.trim_start_matches(' ').trim_end_matches([' ', '.'])
}

/// `name` cut to `MAX_UTF16` code units, keeping a short extension (2 to 20
/// characters with the dot). The stem is cut on a character boundary, so a
/// surrogate pair is never split.
fn cap(name: &str) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if (2..=20).contains(&name[i..].chars().count()) => name.split_at(i),
        _ => (name, ""),
    };
    let room = MAX_UTF16.saturating_sub(utf16_len(ext));
    let mut used = 0;
    let mut out = String::with_capacity(name.len().min(MAX_UTF16 * 4));
    for c in stem.chars() {
        used += c.len_utf16();
        if used > room {
            break;
        }
        out.push(c);
    }
    out.push_str(ext);
    out
}

/// `name` for the `n`th try: "IMG_1.png", 2 -> "IMG_1 (2).png". `n` 1 is the
/// name itself.
pub fn clash_name(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_owned();
    }
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(raw: &str) -> String {
        sanitize(raw, Lang::En)
    }

    #[test]
    fn paths_are_cut_to_the_last_part() {
        assert_eq!(s("../x"), "x");
        assert_eq!(s("..%2F..%2Fx.txt"), "x.txt");
        assert_eq!(s(r"a\b.txt"), "b.txt");
        assert_eq!(s("C:%5Cwindows%5Csystem32%5Ccmd.exe"), "cmd.exe");
        assert_eq!(s("../evil.txt"), "evil.txt");
    }

    #[test]
    fn reserved_device_names_get_an_underscore() {
        assert_eq!(s("CON.txt"), "CON_.txt");
        assert_eq!(s("nul"), "nul_");
        assert_eq!(s("Com1.tar.gz"), "Com1_.tar.gz");
        assert_eq!(s("lpt9.x"), "lpt9_.x");
        // Not reserved: other digits, longer names.
        assert_eq!(s("COM0.txt"), "COM0.txt");
        assert_eq!(s("CONSOLE.txt"), "CONSOLE.txt");
        assert_eq!(s("COM10"), "COM10");
    }

    #[test]
    fn trailing_dots_and_spaces_and_bad_characters_go() {
        assert_eq!(s("name. "), "name");
        assert_eq!(s("  a.txt  "), "a.txt");
        assert_eq!(s("a<b>c:d\"e|f?g*h.txt"), "abcdefgh.txt");
        assert_eq!(s("tab\there\u{0}.txt"), "tabhere.txt");
        assert_eq!(s("a..b"), "a.b");
        assert!(!s("....").contains(".."));
    }

    #[test]
    fn a_leading_dot_stays() {
        assert_eq!(s(".env"), ".env");
        assert_eq!(s("..env"), ".env");
        assert_eq!(s(".nomedia"), ".nomedia");
        assert_eq!(s(" .profile"), ".profile");
        assert_eq!(s(".env. "), ".env");
        assert_eq!(s("."), "file");
    }

    #[test]
    fn a_long_name_is_cut_and_keeps_its_extension() {
        let long = format!("{}.jpeg", "x".repeat(300));
        let got = s(&long);
        assert_eq!(utf16_len(&got), 150);
        assert!(got.ends_with(".jpeg"));
        // No extension: just cut.
        assert_eq!(utf16_len(&s(&"y".repeat(300))), 150);
        // 300 ASCII characters and a short extension: exactly 150 units.
        let ascii = format!("{}.png", "a".repeat(300));
        let got = s(&ascii);
        assert_eq!(got.encode_utf16().count(), 150);
        assert!(got.ends_with(".png"));
    }

    #[test]
    fn a_short_name_is_unchanged() {
        assert_eq!(s("IMG_2041.HEIC"), "IMG_2041.HEIC");
        assert_eq!(s("foto lokasi \u{e9}.png"), "foto lokasi \u{e9}.png");
    }

    #[test]
    fn astral_names_are_cut_by_utf16_units_and_stay_under_the_ntfs_limit() {
        let name = format!("{}.png", "\u{1F600}".repeat(200));
        let got = s(&name);
        assert!(got.encode_utf16().count() <= 150, "{}", got.encode_utf16().count());
        assert!(got.ends_with(".png"));
        // Whole emoji only: 73 of them (146 units) and the 4 of ".png".
        assert_eq!(got.chars().filter(|&c| c == '\u{1F600}').count(), 73);
        // The clash suffix and the .part marker still fit a path component.
        let worst = format!("{}.part", clash_name(&got, 9999));
        assert!(worst.encode_utf16().count() <= 255);
        // An astral extension of 20 characters leaves a stem of 110 units.
        let wide = format!("{}.{}", "b".repeat(300), "\u{1F600}".repeat(19));
        let got = s(&wide);
        assert!(got.encode_utf16().count() <= 150);
        assert!(got.ends_with('\u{1F600}'));
    }

    #[test]
    fn format_and_bidi_controls_are_dropped() {
        // The right-to-left override that makes "gnp.exe" read as "exe.png".
        let got = s("photo\u{202E}gnp.exe");
        assert_eq!(got, "photognp.exe");
        assert!(got.ends_with(".exe"));
        assert!(!got.contains('\u{202E}'));
        assert_eq!(s("photo%E2%80%AEgnp.exe"), "photognp.exe");
        for c in [
            '\u{200B}', '\u{200C}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{202A}',
            '\u{202B}', '\u{202C}', '\u{202D}', '\u{2060}', '\u{2064}', '\u{2066}',
            '\u{2067}', '\u{2068}', '\u{2069}', '\u{FEFF}', '\u{2028}', '\u{2029}',
            '\u{061C}', '\u{180E}', '\u{00AD}', '\u{E0041}',
        ] {
            assert_eq!(s(&format!("a{c}b.txt")), "ab.txt", "U+{:04X}", u32::from(c));
        }
        assert_eq!(s("a\u{200B}b\u{200D}c\u{FEFF}d\u{2066}e\u{2069}.txt"), "abcde.txt");
        // Only format characters: the fallback, in the language.
        assert_eq!(sanitize("\u{202E}\u{200B}\u{FEFF}", Lang::Id), "berkas");
        assert_eq!(sanitize("%E2%80%AE%E2%80%8B", Lang::En), "file");
        // An emoji ZWJ sequence falls apart (accepted).
        assert_eq!(s("\u{1F468}\u{200D}\u{1F469}.png"), "\u{1F468}\u{1F469}.png");
        // Visible characters and variation selectors stay.
        assert_eq!(s("\u{2764}\u{FE0F}.png"), "\u{2764}\u{FE0F}.png");
    }

    #[test]
    fn an_empty_name_gets_a_word_in_the_language() {
        assert_eq!(sanitize("", Lang::Id), "berkas");
        assert_eq!(sanitize("..", Lang::En), "file");
        assert_eq!(sanitize("%2F", Lang::Id), "berkas");
        assert_eq!(sanitize("???", Lang::En), "file");
    }

    #[test]
    fn percent_decoding_is_utf8_and_forgiving() {
        assert_eq!(s("foto%20lokasi%20%C3%A9.png"), "foto lokasi é.png");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }

    #[test]
    fn a_taken_name_is_numbered_before_the_extension() {
        assert_eq!(clash_name("IMG_1.png", 1), "IMG_1.png");
        assert_eq!(clash_name("IMG_1.png", 2), "IMG_1 (2).png");
        assert_eq!(clash_name("a.tar.gz", 3), "a.tar (3).gz");
        assert_eq!(clash_name("README", 2), "README (2)");
        assert_eq!(clash_name(".env", 2), ".env (2)");
    }
}
