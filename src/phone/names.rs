//! File names that arrive from a phone: decoded, cleaned so that they cannot
//! leave the destination folder or name a Windows device, and numbered when
//! the name is taken. Pure.

use crate::i18n::Lang;

/// The longest name kept, in characters.
const MAX_CHARS: usize = 150;

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
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
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
    if name.chars().count() > MAX_CHARS {
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

/// `name` cut to `MAX_CHARS` characters, keeping a short extension.
fn cap(name: &str) -> String {
    let ext_len = name
        .rfind('.')
        .map(|i| name[i..].chars().count())
        .filter(|&n| n > 1 && n <= 20)
        .unwrap_or(0);
    let stem_len = MAX_CHARS - ext_len;
    let chars: Vec<char> = name.chars().collect();
    let mut out: String = chars[..stem_len].iter().collect();
    out.extend(&chars[chars.len() - ext_len..]);
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
        assert_eq!(got.chars().count(), 150);
        assert!(got.ends_with(".jpeg"));
        // No extension: just cut.
        assert_eq!(s(&"y".repeat(300)).chars().count(), 150);
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
