//! Guards the Latin subset built by `scripts/make-fonts.ps1`: the embedded
//! fonts must keep every glyph the UI draws from them. Pure test code; it reads
//! the cmap of the TTF bytes and scans the sources for non-ASCII characters.
//! See the note `docs/memory/exe-size.md` before trimming a font further.
//! JetBrains Mono (the UI font option) is held to the same rules, plus: no
//! ligature features left in its GSUB.

use std::collections::BTreeSet;
use std::path::Path;

const FONTS: [(&str, &[u8]); 8] = [
    (
        "DMSans-400",
        include_bytes!("../../assets/fonts/DMSans-400.ttf"),
    ),
    (
        "DMSans-500",
        include_bytes!("../../assets/fonts/DMSans-500.ttf"),
    ),
    (
        "DMSans-600",
        include_bytes!("../../assets/fonts/DMSans-600.ttf"),
    ),
    (
        "DMMono-400",
        include_bytes!("../../assets/fonts/DMMono-400.ttf"),
    ),
    (
        "DMMono-500",
        include_bytes!("../../assets/fonts/DMMono-500.ttf"),
    ),
    (
        "JetBrainsMono-400",
        include_bytes!("../../assets/fonts/JetBrainsMono-400.ttf"),
    ),
    (
        "JetBrainsMono-500",
        include_bytes!("../../assets/fonts/JetBrainsMono-500.ttf"),
    ),
    (
        "JetBrainsMono-600",
        include_bytes!("../../assets/fonts/JetBrainsMono-600.ttf"),
    ),
];

/// Characters the UI draws that are NOT in the subset. They have always come
/// from DirectWrite's system fallback (Segoe UI Symbol / Segoe UI Emoji), so
/// they are listed here to make that a decision, not an accident. Do not add
/// to it to silence a failure after trimming a font.
const SYSTEM_FALLBACK: &[char] = &[
    '\u{2197}', // ↗ model.rs, render_panel.rs
    '\u{21B3}', // ↳ CLI notices (i18n.rs)
    '\u{2264}', // ≤ render.rs
    '\u{25B8}', // ▸ Mole log
    '\u{266A}', // ♪ geometry.rs
    '\u{25CF}', // ● demo only
    '\u{2713}', // ✓ demo only; DM Sans has no check mark
    '\u{2500}', '\u{2502}', '\u{2503}', '\u{257F}', // box drawing, Mole log
];

/// What JetBrains Mono lacks of what the UI draws, which has always come from
/// DirectWrite's system fallback (Segoe UI Symbol) when the font is chosen.
/// It has the check mark that DM Sans lacks, and the same arrows, quotes and
/// signs as DM Sans; the rest of `SYSTEM_FALLBACK` is missing here too. Do not
/// add to it to silence a real gap: `jetbrains_mono_fallback_list_is_exactly_what_it_lacks`
/// fails for a character that is in the font.
const JBM_SYSTEM_FALLBACK: &[char] = &[
    '\u{2197}', // ↗
    '\u{21B3}', // ↳
    '\u{2264}', // ≤
    '\u{25B8}', // ▸
    '\u{266A}', // ♪
    '\u{25CF}', // ● demo only
    '\u{2500}', '\u{2502}', '\u{2503}', '\u{257F}', // box drawing, Mole log
];

/// Non-ASCII code points in the sources that are never drawn: BOM and
/// replacement-character checks, zero-width joiner, ESC/CSI controls, and the
/// end of the PowerShell quote range `ps_quote` escapes (mole.rs), and the
/// zero-width space Edge writes inside a tab's name (`notif::open`), and the
/// line and paragraph separators the phone page's boot JSON escapes.
const NOT_DRAWN: &[char] = &[
    '\u{FEFF}', '\u{FFFD}', '\u{200D}', '\u{200B}', '\u{9B}', '\u{201B}', '\u{2028}', '\u{2029}',
];

/// Mirrors `UNICODES` in `scripts/make-fonts.ps1`.
const SUBSET_RANGES: &[(u32, u32)] = &[
    (0x0020, 0x007E),
    (0x00A0, 0x00FF),
    (0x0100, 0x017F),
    (0x2010, 0x2027),
    (0x2030, 0x203A),
    (0x20AC, 0x20AC),
    (0x2122, 0x2122),
    (0x2190, 0x2193),
    (0x2212, 0x2212),
    (0x2713, 0x2713),
];

fn be16(d: &[u8], at: usize) -> usize {
    u16::from_be_bytes([d[at], d[at + 1]]) as usize
}

fn be32(d: &[u8], at: usize) -> usize {
    u32::from_be_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]) as usize
}

/// Every code point mapped by the font's Unicode cmap (formats 4 and 12).
fn cmap(font: &[u8]) -> BTreeSet<u32> {
    let tables = be16(font, 4);
    let mut cmap_at = None;
    for i in 0..tables {
        let rec = 12 + 16 * i;
        if &font[rec..rec + 4] == b"cmap" {
            cmap_at = Some(be32(font, rec + 8));
        }
    }
    let base = cmap_at.expect("cmap table");
    let mut set = BTreeSet::new();
    for i in 0..be16(font, base + 2) {
        let rec = base + 4 + 8 * i;
        let (platform, encoding) = (be16(font, rec), be16(font, rec + 2));
        if !(platform == 3 && (encoding == 1 || encoding == 10)) {
            continue;
        }
        let sub = base + be32(font, rec + 4);
        match be16(font, sub) {
            4 => {
                let segs = be16(font, sub + 6) / 2;
                let ends = sub + 14;
                let starts = ends + 2 * segs + 2;
                let deltas = starts + 2 * segs;
                let offsets = deltas + 2 * segs;
                for s in 0..segs {
                    let (end, start) = (be16(font, ends + 2 * s), be16(font, starts + 2 * s));
                    if start == 0xFFFF {
                        continue;
                    }
                    for cp in start..=end {
                        let ro = be16(font, offsets + 2 * s);
                        let gid = if ro == 0 {
                            (cp + be16(font, deltas + 2 * s)) & 0xFFFF
                        } else {
                            let at = offsets + 2 * s + ro + 2 * (cp - start);
                            match be16(font, at) {
                                0 => 0,
                                g => (g + be16(font, deltas + 2 * s)) & 0xFFFF,
                            }
                        };
                        if gid != 0 {
                            set.insert(cp as u32);
                        }
                    }
                }
            }
            12 => {
                for g in 0..be32(font, sub + 12) {
                    let at = sub + 16 + 12 * g;
                    let (start, end, gid) =
                        (be32(font, at), be32(font, at + 4), be32(font, at + 8));
                    for cp in start..=end {
                        if gid + (cp - start) != 0 {
                            set.insert(cp as u32);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    set
}

fn rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

const CFG_TEST: &str = "#[cfg(test)]";

/// Byte length of the item at the start of `s`: up to the `}` that closes its
/// body, or the `;` that ends it at depth 0. Braces and semicolons inside
/// comments, strings and char literals do not count.
fn item_len(s: &str) -> usize {
    let b = s.as_bytes();
    let (mut i, mut depth) = (0, 0usize);
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i += s[i..].find('\n').unwrap_or(s.len() - i);
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += s[i + 2..].find("*/").map_or(s.len() - i, |e| e + 4);
                continue;
            }
            b'r' if (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'))
                && matches!(b.get(i + 1), Some(b'"' | b'#')) =>
            {
                let hashes = b[i + 1..].iter().take_while(|&&c| c == b'#').count();
                if b.get(i + 1 + hashes) == Some(&b'"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    let body = i + 2 + hashes;
                    i = s[body..]
                        .find(&close)
                        .map_or(s.len(), |e| body + e + close.len());
                    continue;
                }
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' => {
                // A char literal ('x', '\n', '\u{..}', or one multi-byte char);
                // anything else is a lifetime.
                if b.get(i + 1) == Some(&b'\\') {
                    i += s[i + 2..].find('\'').map_or(s.len() - i, |e| e + 2);
                } else if let Some(c) = s[i + 1..].chars().next()
                    && s[i + 1 + c.len_utf8()..].starts_with('\'')
                {
                    i += c.len_utf8() + 1;
                }
            }
            b'{' | b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            b';' if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    s.len()
}

/// The source with every `#[cfg(test)]` item cut out (test modules, test-only
/// helpers anywhere in the file), and the names of the modules declared
/// test-only as `#[cfg(test)] mod name;`.
fn live_source(source: &str) -> (String, Vec<String>) {
    let (mut live, mut test_mods) = (String::new(), Vec::new());
    let mut rest = source;
    // The attribute on its own line, so the text in a comment or literal is
    // not taken for it.
    let own_line = |s: &str, at: usize| {
        s[..at]
            .rsplit('\n')
            .next()
            .is_some_and(|l| l.trim().is_empty())
            && s[at + CFG_TEST.len()..].starts_with(['\r', '\n'])
    };
    while let Some((at, _)) = rest
        .match_indices(CFG_TEST)
        .find(|&(at, _)| own_line(rest, at))
    {
        live.push_str(&rest[..at]);
        let item = &rest[at + CFG_TEST.len()..];
        let len = item_len(item);
        let decl = item[..len].trim();
        if let Some(name) = decl.strip_prefix("mod ").and_then(|d| d.strip_suffix(';')) {
            test_mods.push(name.trim().to_string());
        }
        rest = &item[len..];
    }
    live.push_str(rest);
    (live, test_mods)
}

/// Non-ASCII characters written in a source file outside comments and test
/// items: raw in a literal, or as a `\u{..}` escape.
fn drawn_chars(source: &str) -> BTreeSet<char> {
    let mut set = BTreeSet::new();
    let (live, _) = live_source(source);
    for line in live.lines() {
        let code = match line.trim_start() {
            t if t.starts_with("//") => continue,
            _ => line,
        };
        // A trailing `//` comment; a URL inside a literal loses only its tail.
        let code = code.split(" // ").next().unwrap_or(code);
        set.extend(code.chars().filter(|c| !c.is_ascii()));
        let mut rest = code;
        while let Some(i) = rest.find("\\u{") {
            rest = &rest[i + 3..];
            if let Some(end) = rest.find('}')
                && let Some(c) = u32::from_str_radix(&rest[..end], 16)
                    .ok()
                    .and_then(char::from_u32)
                && !c.is_ascii()
            {
                set.insert(c);
            }
        }
    }
    set
}

/// Files of the modules a source file declares as `#[cfg(test)] mod name;`.
fn test_module_files(file: &Path, source: &str) -> Vec<std::path::PathBuf> {
    let dir = file.parent().expect("parent");
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let base = if matches!(stem, "mod" | "main" | "lib") {
        dir.to_path_buf()
    } else {
        dir.join(stem)
    };
    live_source(source)
        .1
        .into_iter()
        .flat_map(|m| [base.join(format!("{m}.rs")), base.join(&m).join("mod.rs")])
        .collect()
}

/// Mapped code points per font inside `SUBSET_RANGES`, as the generated files
/// have them today. The source families do not cover every code point of the
/// ranges (DM Mono has no arrows, DM Sans no check mark), so this pins what the
/// subset keeps: dropping a range or a glyph lowers the count and fails here.
const SUBSET_COUNTS: [(&str, usize); 8] = [
    ("DMSans-400", 325),
    ("DMSans-500", 325),
    ("DMSans-600", 325),
    ("DMMono-400", 316),
    ("DMMono-500", 316),
    ("JetBrainsMono-400", 348),
    ("JetBrainsMono-500", 348),
    ("JetBrainsMono-600", 348),
];

fn in_subset(map: &BTreeSet<u32>) -> usize {
    SUBSET_RANGES
        .iter()
        .map(|&(lo, hi)| map.range(lo..=hi).count())
        .sum()
}

#[test]
fn every_font_keeps_its_latin_subset() {
    for ((name, bytes), (count_name, count)) in FONTS.iter().zip(SUBSET_COUNTS) {
        assert_eq!(*name, count_name);
        let map = cmap(bytes);
        // Plain text: ASCII and Latin-1 (user titles, file names, Indonesian).
        for cp in (0x20..=0x7E).chain(0xA0..=0xFF) {
            if cp != 0xAD {
                assert!(map.contains(&cp), "{name} lost U+{cp:04X}");
            }
        }
        assert_eq!(
            in_subset(&map),
            count,
            "{name}: glyph count in the subset changed"
        );
    }
}

#[test]
fn the_fonts_have_the_symbols_the_ui_draws() {
    // Drawn from the sans: separators, signs, arrows, quotes, units.
    let sans = "\u{B7}\u{D7}\u{F7}\u{B0}\u{B1}\u{A7}\u{2013}\u{2014}\u{2026}\u{203A}\u{2192}\
                \u{2191}\u{2193}\u{2212}\u{2018}\u{2019}\u{201C}\u{201D}\u{2022}\u{20AC}\u{2122}%";
    // Drawn from the mono (terminal log, clipboard text, chips).
    let mono = "\u{B7}\u{D7}\u{2026}\u{203A}\u{2013}\u{2014}\u{2212}%";
    for (name, bytes) in FONTS {
        let map = cmap(bytes);
        // JetBrains Mono sets the UI text in the Mono option: the sans list.
        let want = if name.starts_with("DMMono") { mono } else { sans };
        for c in want.chars() {
            assert!(
                map.contains(&(c as u32)),
                "{name} lacks U+{:04X} {c}",
                c as u32
            );
        }
    }
}

#[test]
fn every_character_the_sources_draw_is_covered() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rs_files(&src, &mut files);
    assert!(files.len() > 50, "found {} source files", files.len());
    // Only the sans is required here. The mono draws terminal and clipboard
    // text (its symbols are checked above); a literal the mono lacks falls
    // back to a system font like any user character does.
    let sans: Vec<BTreeSet<u32>> = FONTS
        .iter()
        .filter(|(n, _)| n.starts_with("DMSans"))
        .map(|(_, b)| cmap(b))
        .collect();
    // JetBrains Mono sets the same UI text in the Mono option.
    let jbm: Vec<BTreeSet<u32>> = FONTS
        .iter()
        .filter(|(n, _)| n.starts_with("JetBrainsMono"))
        .map(|(_, b)| cmap(b))
        .collect();
    let sources: Vec<(std::path::PathBuf, String)> = files
        .into_iter()
        .map(|f| {
            let text = std::fs::read_to_string(&f).expect("read source");
            (f, text)
        })
        .collect();
    let test_only: Vec<std::path::PathBuf> = sources
        .iter()
        .flat_map(|(f, text)| test_module_files(f, text))
        .collect();
    assert!(
        test_only.iter().any(|f| f.ends_with("font_coverage.rs")),
        "test-only modules not found: {test_only:?}"
    );
    let mut missing = Vec::new();
    for (file, text) in &sources {
        if test_only.contains(file) {
            continue;
        }
        for c in drawn_chars(text) {
            if NOT_DRAWN.contains(&c) {
                continue;
            }
            if !SYSTEM_FALLBACK.contains(&c) && sans.iter().any(|map| !map.contains(&(c as u32)))
            {
                missing.push(format!("{c} U+{:04X} in {}", c as u32, file.display()));
            }
            if !JBM_SYSTEM_FALLBACK.contains(&c) && jbm.iter().any(|map| !map.contains(&(c as u32)))
            {
                missing.push(format!(
                    "{c} U+{:04X} in {} (JetBrains Mono)",
                    c as u32,
                    file.display()
                ));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "uncovered glyphs:\n{}",
        missing.join("\n")
    );
    // The scan reaches live code past a test-only helper near the top of a
    // file (i18n.rs `mod current`, mole.rs `argv`).
    let drawn_in = |name: &str| {
        let (_, text) = sources.iter().find(|(f, _)| f.ends_with(name)).expect(name);
        drawn_chars(text)
    };
    assert!(
        drawn_in("i18n.rs").contains(&'\u{21B3}'),
        "i18n.rs not scanned past `mod current`"
    );
    assert!(
        drawn_in("mole.rs").contains(&'\u{201B}'),
        "mole.rs not scanned past `argv`"
    );
}

/// The feature tags in a font's GSUB, from the raw bytes.
fn gsub_features(font: &[u8]) -> Vec<[u8; 4]> {
    let tables = be16(font, 4);
    let Some(gsub) = (0..tables).find_map(|i| {
        let rec = 12 + 16 * i;
        (&font[rec..rec + 4] == b"GSUB").then(|| be32(font, rec + 8))
    }) else {
        return Vec::new();
    };
    // Header: version (4), script list, feature list, lookup list offsets.
    let list = gsub + be16(font, gsub + 6);
    (0..be16(font, list))
        .map(|i| {
            let at = list + 2 + 6 * i;
            [font[at], font[at + 1], font[at + 2], font[at + 3]]
        })
        .collect()
}

/// The programming ligatures are dropped in the subset (`calt`, `liga` and
/// `dlig` are not kept), so "->", "!=", "==" and "www" are drawn literally by
/// DirectWrite and GDI alike. DM Sans keeps its `liga` and `calt`.
#[test]
fn jetbrains_mono_has_no_ligature_features() {
    let dm_sans = gsub_features(FONTS[0].1);
    assert!(
        dm_sans.contains(b"liga") && dm_sans.contains(b"calt"),
        "the GSUB reader finds DM Sans's features: {dm_sans:?}"
    );
    for (name, bytes) in FONTS.iter().filter(|(n, _)| n.starts_with("JetBrainsMono")) {
        let tags = gsub_features(bytes);
        assert!(tags.contains(b"ccmp"), "{name}: GSUB not read: {tags:?}");
        for banned in [b"calt", b"liga", b"dlig"] {
            assert!(
                !tags.contains(banned),
                "{name} still has {}",
                String::from_utf8_lossy(banned)
            );
        }
    }
}

#[test]
fn jetbrains_mono_fallback_list_is_exactly_what_it_lacks() {
    let maps: Vec<BTreeSet<u32>> = FONTS
        .iter()
        .filter(|(n, _)| n.starts_with("JetBrainsMono"))
        .map(|(_, b)| cmap(b))
        .collect();
    assert_eq!(maps.len(), 3);
    for c in JBM_SYSTEM_FALLBACK {
        assert!(
            maps.iter().all(|m| !m.contains(&(*c as u32))),
            "JetBrains Mono has U+{:04X}: take it off the fallback list",
            *c as u32
        );
    }
    // What DM Sans lacks and the system supplies, JetBrains Mono has or is listed.
    for c in SYSTEM_FALLBACK {
        let has = maps.iter().all(|m| m.contains(&(*c as u32)));
        assert!(
            has || JBM_SYSTEM_FALLBACK.contains(c),
            "U+{:04X} is in neither JetBrains Mono nor its fallback list",
            *c as u32
        );
    }
    // The check mark DM Sans lacks is in JetBrains Mono.
    assert!(maps.iter().all(|m| m.contains(&0x2713)));
}

#[test]
fn the_scan_skips_only_test_items() {
    let source = "#[cfg(test)]\nmod current {\n    fn f() -> char { '\u{2265}' }\n    \
                  fn b<'a>(s: &'a str) -> (char, &'a str) { ('{', \"}}\" /* } */) } // }\n}\n\
                  const NOTE: &str = \"#[cfg(test)]\";\n\
                  const LIVE: &str = \"\u{2248}\";\n\
                  #[cfg(test)]\nconst CF: [u8; 2] = [0; 2];\n\
                  fn g() -> char { '\u{2260}' }\n\
                  #[cfg(test)]\nmod tests {\n    const T: char = '\u{221E}';\n}\n\
                  fn h() -> char { '\u{2261}' }\n";
    let found: String = drawn_chars(source).into_iter().collect();
    assert_eq!(found, "\u{2248}\u{2260}\u{2261}");
}
