//! WinAnsiEncoding (Windows code page 1252) for text written with the
//! standard Helvetica font.

/// Unicode code points of cp1252 0x80..=0x9F; 0 marks the five unassigned
/// codes. Numbers, not chars, so the font coverage scan does not take them
/// for UI text.
const HIGH: [u32; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021,
    0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017D, 0,
    0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
];

/// The cp1252 code of one character; control characters have none.
pub fn code(c: char) -> Option<u8> {
    match c as u32 {
        u @ (0x20..=0x7E | 0xA0..=0xFF) => Some(u as u8),
        0..=0x9F => None,
        u => HIGH.iter().position(|&h| h == u).map(|i| 0x80 + i as u8),
    }
}

/// The text as cp1252 bytes, or `None` when a character has no cp1252 code
/// (CJK, emoji, control characters).
pub fn encode(s: &str) -> Option<Vec<u8>> {
    s.chars().map(code).collect()
}

/// A PDF literal string, parentheses included, with `\`, `(` and `)` escaped.
pub fn pdf_string(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    out.push(b'(');
    for &b in bytes {
        if matches!(b, b'\\' | b'(' | b')') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winansi_encodes_indonesian_and_rejects_cjk() {
        let rp = encode("Rp 1.500 – Ibu Siti").unwrap();
        assert_eq!(rp[9], 0x96);
        assert_eq!(&rp[..8], b"Rp 1.500");
        assert_eq!(encode("李"), None);
        assert_eq!(encode("€"), Some(vec![0x80]));
        assert_eq!(encode("é Ÿ ™ —"), Some(vec![0xE9, 0x20, 0x9F, 0x20, 0x99, 0x20, 0x97]));
        assert_eq!(encode("a\nb"), None);
        assert_eq!(encode("\u{81}"), None);
        assert_eq!(encode(""), Some(Vec::new()));
    }

    #[test]
    fn pdf_string_escapes() {
        assert_eq!(pdf_string(b"a(b)c\\d"), b"(a\\(b\\)c\\\\d)".to_vec());
        assert_eq!(pdf_string(&[0x80]), vec![b'(', 0x80, b')']);
    }
}
