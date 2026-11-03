//! The file shelf: a temporary place to leave files on the island and take
//! them out again later. It keeps paths only, never copies, and only in
//! memory: a restart empties it. The list and the card data are pure; the
//! OLE drop target and drag-out are in `shelf_ole`.

use std::path::{Path, PathBuf};

use crate::ui::icons::Icon;

/// Files on the shelf at most (three rows of four cards).
pub const MAX_ITEMS: usize = 12;

/// What a card's icon shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShelfKind {
    Folder,
    Image,
    /// Sound and video.
    Media,
    Code,
    Archive,
    /// Everything else.
    Document,
}

/// The icon kind for a path: a folder, or by extension.
pub fn kind_of(path: &Path, is_dir: bool) -> ShelfKind {
    if is_dir {
        return ShelfKind::Folder;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "tif" | "tiff" | "ico"
        | "heic" | "avif" => ShelfKind::Image,
        "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" | "mp4" | "mkv" | "mov" | "avi" | "webm"
        | "wmv" => ShelfKind::Media,
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "c" | "cpp" | "h" | "cs" | "java" | "go"
        | "html" | "css" | "json" | "xml" | "yaml" | "yml" | "toml" | "ps1" | "sh" | "bat"
        | "sql" => ShelfKind::Code,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "cab" => ShelfKind::Archive,
        _ => ShelfKind::Document,
    }
}

/// A card's icon.
pub fn icon(kind: ShelfKind) -> Icon {
    match kind {
        ShelfKind::Folder => Icon::Folder,
        ShelfKind::Image => Icon::Image,
        ShelfKind::Media => Icon::Music,
        ShelfKind::Code => Icon::Code,
        ShelfKind::Archive => Icon::Package,
        ShelfKind::Document => Icon::Note,
    }
}

/// What the file system says about a shelf path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    Missing,
    File,
    Folder,
}

/// `GetDriveTypeW`'s answer for a network drive.
const DRIVE_REMOTE: u32 = 4;

/// Whether `path` is on the network: a UNC path (`\\server\share\…`, and
/// any other `\\` form, which is not worth the risk) or a drive letter that
/// `remote_drive` says is mapped. Such a path is never probed on the UI
/// thread: a share that is gone can block it for many seconds.
pub fn on_network(path: &Path, remote_drive: impl Fn(char) -> bool) -> bool {
    let s = path.as_os_str().to_string_lossy();
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some('\\' | '/'), Some('\\' | '/')) => true,
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic() => remote_drive(letter),
        _ => false,
    }
}

/// Whether drive `letter` is a network drive (asks for its root only, which
/// does not touch the share).
fn drive_is_remote(letter: char) -> bool {
    let root = windows::core::HSTRING::from(format!("{letter}:\\"));
    unsafe { windows::Win32::Storage::FileSystem::GetDriveTypeW(&root) == DRIVE_REMOTE }
}

/// What a network path is taken to be without looking: present, a folder
/// when it has no extension, else a file.
fn unprobed(path: &Path) -> Probe {
    if path.extension().is_some() {
        Probe::File
    } else {
        Probe::Folder
    }
}

/// The real file system's answer (for `Shelf::cards`). A path on the
/// network (`on_network`) is not looked at: it counts as there.
pub fn probe(path: &Path) -> Probe {
    if on_network(path, drive_is_remote) {
        return unprobed(path);
    }
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => Probe::Folder,
        Ok(_) => Probe::File,
        Err(_) => Probe::Missing,
    }
}

/// One card as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShelfCard {
    /// Who the card is across changes: the normalised path (`key_of`), so
    /// the motion can tell a card that stayed from one that came or went.
    pub key: String,
    /// The file's whole name; the page breaks it into lines (`wrap_name`).
    pub name: String,
    pub kind: ShelfKind,
    /// The path is gone: the card is dimmed and cannot be dragged.
    pub stale: bool,
    /// The file came from a phone over the QR link (a small tag says so).
    pub from_phone: bool,
}

/// The name a card shows for `path`: its last part, or the whole path for
/// a drive root ("C:\").
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// `name` cut to `max` characters with an ellipsis, keeping a short
/// extension: "kemang-denah-lantai.pdf" -> "kemang-de….pdf".
pub fn short_name(name: &str, max: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max {
        return name.to_owned();
    }
    let ext_len = name
        .rfind('.')
        .map(|i| name[i..].chars().count())
        .filter(|&n| n > 1 && n <= 6 && n + 2 < max)
        .unwrap_or(0);
    let head = max - ext_len - 1;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - ext_len..]);
    out
}

/// `name` as the page shows it: one line cut with an ellipsis, or (`lines`
/// 2) a first line broken after the last separator (space, `-`, `_`, `.`,
/// `(`) in its first `per_line` characters, and the rest cut like
/// `short_name` on the second.
pub fn wrap_name(name: &str, per_line: usize, lines: usize) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    if lines < 2 || chars.len() <= per_line {
        return vec![short_name(name, per_line)];
    }
    let cut = chars[..per_line]
        .iter()
        .rposition(|c| matches!(c, ' ' | '-' | '_' | '.' | '('))
        .map(|i| i + 1)
        .filter(|&i| i >= per_line / 2)
        .unwrap_or(per_line);
    let first: String = chars[..cut].iter().collect();
    let rest: String = chars[cut..].iter().collect();
    vec![
        first.trim_end().to_owned(),
        short_name(rest.trim_start(), per_line),
    ]
}

/// Where a shelf item came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Dropped on the island.
    Drop,
    /// Received from a phone over the QR link.
    Phone,
}

/// How a batch from a phone lands on the shelf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// Each file gets its card as it arrives.
    Files,
    /// The whole batch would not fit: one card for the folder the files
    /// went to, instead of any file card.
    Folder,
}

/// How the files of a phone batch of `batch_count` land on a shelf that
/// holds `shelf_len` cards: as cards of their own if all of them fit under
/// `MAX_ITEMS`, else as one folder card.
pub fn phone_landing(shelf_len: usize, batch_count: usize) -> Landing {
    if shelf_len.saturating_add(batch_count) > MAX_ITEMS {
        Landing::Folder
    } else {
        Landing::Files
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shelf {
    items: Vec<(PathBuf, Origin)>,
}

impl Shelf {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&Path> {
        self.items.get(i).map(|(p, _)| p.as_path())
    }

    /// Puts `paths` on the shelf, newest last: a path already there (the
    /// same name in any letter case, as Windows sees it) and anything past
    /// `MAX_ITEMS` is left out. Returns how many were added.
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        self.add_from(paths, Origin::Drop)
    }

    /// `add`, for files that came from `origin`.
    pub fn add_from(&mut self, paths: impl IntoIterator<Item = PathBuf>, origin: Origin) -> usize {
        let mut added = 0;
        for path in paths {
            if self.items.len() >= MAX_ITEMS {
                break;
            }
            let key = key_of(&path);
            if key.is_empty() || self.items.iter().any(|(p, _)| key_of(p) == key) {
                continue;
            }
            self.items.push((path, origin));
            added += 1;
        }
        added
    }

    pub fn remove(&mut self, i: usize) -> bool {
        if i < self.items.len() {
            self.items.remove(i);
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) -> bool {
        let had = !self.items.is_empty();
        self.items.clear();
        had
    }

    /// The cards, with `probe` telling what each path is now. A missing
    /// path is stale; its kind then comes from its extension alone.
    pub fn cards(&self, probe: impl Fn(&Path) -> Probe) -> Vec<ShelfCard> {
        self.items
            .iter()
            .map(|(p, origin)| {
                let found = probe(p);
                ShelfCard {
                    key: key_of(p),
                    name: display_name(p),
                    kind: kind_of(p, found == Probe::Folder),
                    stale: found == Probe::Missing,
                    from_phone: *origin == Origin::Phone,
                }
            })
            .collect()
    }
}

/// How two paths are compared: Windows ignores letter case.
fn key_of(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn paths_are_added_once_in_any_letter_case_and_up_to_the_cap() {
        let mut s = Shelf::default();
        assert_eq!(
            s.add([p(r"C:\a\Denah.pdf"), p(r"c:\A\denah.PDF"), p(r"C:\b")]),
            2
        );
        // A trailing slash is the same folder.
        assert_eq!(s.add([p(r"C:\b\")]), 0);
        assert_eq!(s.len(), 2);
        assert_eq!(s.add([p("")]), 0, "an empty path is nothing");
        let many: Vec<PathBuf> = (0..20).map(|i| p(&format!(r"C:\f\{i}.txt"))).collect();
        assert_eq!(s.add(many), MAX_ITEMS - 2);
        assert_eq!(s.len(), MAX_ITEMS);
        assert_eq!(s.add([p(r"C:\one-more.txt")]), 0, "full");
        // The first ones stay, in order.
        assert_eq!(s.get(0), Some(Path::new(r"C:\a\Denah.pdf")));
    }

    #[test]
    fn a_card_key_ignores_letter_case_and_a_trailing_slash() {
        let key = |path: &str| {
            let mut s = Shelf::default();
            s.add([p(path)]);
            s.cards(|_| Probe::File)[0].key.clone()
        };
        assert_eq!(key(r"C:\Proyek\Denah"), key(r"c:\proyek\DENAH\"));
        assert_ne!(key(r"C:\Proyek\Denah"), key(r"C:\Proyek\Lain"));
        assert!(!key(r"C:\Proyek\Denah").is_empty());
    }

    #[test]
    fn remove_and_clear() {
        let mut s = Shelf::default();
        assert!(!s.clear());
        s.add([p(r"C:\a.txt"), p(r"C:\b.txt"), p(r"C:\c.txt")]);
        assert!(s.remove(1));
        assert!(!s.remove(5));
        assert_eq!(s.get(1), Some(Path::new(r"C:\c.txt")));
        assert!(s.clear());
        assert!(s.is_empty());
    }

    #[test]
    fn cards_mark_a_missing_path_stale_through_the_injected_probe() {
        let mut s = Shelf::default();
        s.add([
            p(r"C:\x\denah.pdf"),
            p(r"C:\x\foto.PNG"),
            p(r"C:\x\Proyek"),
            p(r"C:\x\hilang.zip"),
        ]);
        let probe = |path: &Path| match path.file_name().unwrap().to_str().unwrap() {
            "Proyek" => Probe::Folder,
            "hilang.zip" => Probe::Missing,
            _ => Probe::File,
        };
        let cards = s.cards(probe);
        let got: Vec<(&str, ShelfKind, bool)> = cards
            .iter()
            .map(|c| (c.name.as_str(), c.kind, c.stale))
            .collect();
        assert_eq!(
            got,
            [
                ("denah.pdf", ShelfKind::Document, false),
                ("foto.PNG", ShelfKind::Image, false),
                ("Proyek", ShelfKind::Folder, false),
                ("hilang.zip", ShelfKind::Archive, true),
            ]
        );
    }

    #[test]
    fn network_paths_are_never_probed() {
        let mapped = |letter: char| letter.eq_ignore_ascii_case(&'z');
        assert!(on_network(Path::new(r"\\server\share\denah.pdf"), mapped));
        assert!(on_network(Path::new(r"\\?\UNC\server\share\a.txt"), mapped));
        assert!(on_network(Path::new("//server/share/a.txt"), mapped));
        assert!(on_network(Path::new(r"Z:\proyek\a.txt"), mapped));
        assert!(on_network(Path::new(r"z:\proyek"), mapped));
        assert!(!on_network(Path::new(r"C:\proyek\a.txt"), mapped));
        assert!(!on_network(Path::new(r"\proyek"), mapped));
        assert!(!on_network(Path::new(""), mapped));
        // Taken as there; a folder when it has no extension.
        assert_eq!(
            probe(Path::new(r"\\vale-test-nowhere\share\a.txt")),
            Probe::File
        );
        assert_eq!(
            probe(Path::new(r"\\vale-test-nowhere\share\Proyek")),
            Probe::Folder
        );
    }

    #[test]
    fn the_icon_follows_the_extension_in_any_case() {
        let kind = |name: &str| kind_of(Path::new(name), false);
        assert_eq!(kind("a.JPG"), ShelfKind::Image);
        assert_eq!(kind("a.mp3"), ShelfKind::Media);
        assert_eq!(kind("a.rs"), ShelfKind::Code);
        assert_eq!(kind("a.7z"), ShelfKind::Archive);
        assert_eq!(kind("a.xlsx"), ShelfKind::Document);
        assert_eq!(kind("noext"), ShelfKind::Document);
        assert_eq!(kind_of(Path::new("a.jpg"), true), ShelfKind::Folder);
    }

    #[test]
    fn a_name_breaks_into_two_lines_at_a_separator_and_the_second_is_cut() {
        // Fits: one line, untouched.
        assert_eq!(wrap_name("denah.pdf", 16, 2), ["denah.pdf"]);
        // Breaks after the last separator in the first 16 characters.
        assert_eq!(
            wrap_name("kemang-denah-lantai-dua.pdf", 16, 2),
            ["kemang-denah-", "lantai-dua.pdf"]
        );
        // No separator: a hard break, the rest cut with the extension kept.
        let long = wrap_name("abcdefghijklmnopqrstuvwxyzabcdefghijklmnop.pdf", 16, 2);
        assert_eq!(long[0], "abcdefghijklmnop");
        assert_eq!(long[1].chars().count(), 16);
        assert!(long[1].ends_with(".pdf") && long[1].contains('…'));
        // A separator too early in the line is not worth a break.
        assert_eq!(
            wrap_name("a-bcdefghijklmnopqrstuvwxyz", 16, 2)[0],
            "a-bcdefghijklmno"
        );
        // One line only: cut like `short_name`.
        assert_eq!(
            wrap_name("kemang-denah-lantai-dua.pdf", 14, 1),
            ["kemang-de….pdf"]
        );
        // Never more than two lines, whatever the name.
        assert!(wrap_name(&"x ".repeat(80), 12, 2).len() <= 2);
    }

    #[test]
    fn names_are_cut_short_keeping_a_short_extension() {
        assert_eq!(short_name("denah.pdf", 14), "denah.pdf");
        let cut = short_name("kemang-denah-lantai-dua.pdf", 14);
        assert_eq!(cut, "kemang-de….pdf");
        assert_eq!(cut.chars().count(), 14);
        // A long extension is not kept; no extension at all.
        let odd = short_name("arsip.sangatpanjangekstensi", 14);
        assert_eq!(odd.chars().count(), 14);
        assert!(odd.ends_with('…'));
        assert_eq!(
            short_name("abcdefghijklmnopqrstuvwxyz", 14),
            "abcdefghijklm…"
        );
        // Multi-byte characters count as one.
        assert_eq!(short_name("ééééééééééééééééé", 14).chars().count(), 14);
        // A drive root has no file name: its path shows.
        assert_eq!(display_name(Path::new(r"C:\")), r"C:\");
    }

    #[test]
    fn a_batch_lands_as_files_unless_it_would_pass_the_cap() {
        assert_eq!(phone_landing(0, 1), Landing::Files);
        assert_eq!(phone_landing(0, MAX_ITEMS), Landing::Files);
        assert_eq!(phone_landing(5, 7), Landing::Files);
        assert_eq!(phone_landing(5, 8), Landing::Folder);
        assert_eq!(phone_landing(0, MAX_ITEMS + 1), Landing::Folder);
        assert_eq!(phone_landing(MAX_ITEMS, 1), Landing::Folder);
        assert_eq!(phone_landing(usize::MAX, usize::MAX), Landing::Folder);
    }

    #[test]
    fn a_folder_card_for_the_phone_folder_is_added_once() {
        let mut s = Shelf::default();
        let folder = p(r"C:\Users\x\Downloads\Vale");
        assert_eq!(s.add_from([folder.clone()], Origin::Phone), 1);
        assert_eq!(s.add_from([p(r"c:\users\X\downloads\vale\")], Origin::Phone), 0);
        let cards = s.cards(|_| Probe::Folder);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].kind, ShelfKind::Folder);
        assert_eq!(cards[0].name, "Vale");
        assert!(cards[0].from_phone);
    }

    #[test]
    fn cards_remember_where_they_came_from() {
        let mut s = Shelf::default();
        s.add([p(r"C:\a\denah.pdf")]);
        s.add_from([p(r"C:\d\IMG_0531.png")], Origin::Phone);
        let cards = s.cards(|_| Probe::File);
        assert_eq!(
            cards.iter().map(|c| c.from_phone).collect::<Vec<_>>(),
            [false, true]
        );
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn taking_a_card_off_or_clearing_the_shelf_never_deletes_the_file() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("shelf-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&a, "a").expect("write");
        std::fs::write(&b, "b").expect("write");
        let mut s = Shelf::default();
        s.add_from([a.clone(), b.clone()], Origin::Phone);
        assert!(s.remove(0));
        assert!(a.exists(), "Remove only forgets the card");
        assert!(s.clear());
        assert!(a.exists() && b.exists(), "Kosongkan keeps every file");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
