//! The names the quick search finds besides apps: files and folders under the
//! user's Desktop, Documents, Downloads, Pictures, Videos and Music. Our own
//! walker, not Windows Search (indexing may be off or leave folders out, and a
//! walker is testable on a temp tree). Pure over the roots it is given;
//! `default_roots` is the only part that asks Windows.
//!
//! Power: the walk runs on a short-lived thread, only when the search opens
//! (and only if the last walk is older than `FILE_FRESH`); the result lives in
//! memory and is never written to disk. Files are never opened (nothing is
//! downloaded from a cloud folder), only their names, sizes and dates are read
//! from the directory listing.

use std::collections::VecDeque;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Videos, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use windows::core::GUID;

/// Folders below a root the walk reads (Documents\A\B\C\D\file is the
/// deepest file kept); deeper is mostly code trees and archives.
pub const FILE_DEPTH: usize = 5;
/// Entries one root keeps at most, so a huge Documents cannot starve Desktop
/// or Downloads.
pub const FILE_ROOT_MAX: usize = 10_000;
/// Entries all roots keep together (about 6 MB of paths in memory, and a walk
/// of a few hundred ms on an SSD).
pub const FILE_TOTAL_MAX: usize = 30_000;
/// A walk younger than this is not repeated when the search opens again.
pub const FILE_FRESH: Duration = Duration::from_secs(30);
/// A folder's file count stops here ("999+").
pub const COUNT_CAP: usize = 999;

const HIDDEN: u32 = 0x2;
const SYSTEM: u32 = 0x4;
const DIRECTORY: u32 = 0x10;
/// A cloud folder: listing it would make the provider download it.
const RECALL_ON_OPEN: u32 = 0x40000;

/// Folder names that are never walked (compared without case).
const SKIPPED_DIRS: [&str; 3] = ["node_modules", "__pycache__", "$recycle.bin"];

/// Which of the user's folders an entry lives under (its label in a row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootKind {
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Videos,
    Music,
}

/// A folder the walk starts from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRoot {
    pub kind: RootKind,
    pub path: PathBuf,
}

/// How far a walk goes; `Limits::DEFAULT` outside tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub depth: usize,
    pub root_max: usize,
    pub total_max: usize,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        depth: FILE_DEPTH,
        root_max: FILE_ROOT_MAX,
        total_max: FILE_TOTAL_MAX,
    };
}

/// A file or folder the walk found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// The name with its extension.
    pub name: String,
    /// `name` in lower case, so a search does not lower 30,000 names a key.
    pub lower: String,
    pub path: PathBuf,
    pub root: RootKind,
    /// The folder it is in, below its root (`A\B`); `None` at the root.
    pub parent: Option<String>,
    pub is_dir: bool,
    /// Bytes (0 for a folder).
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// `path` for comparing roots: lower case, no trailing separator.
fn key(path: &Path) -> String {
    let s = path.to_string_lossy().to_lowercase();
    s.trim_end_matches(['\\', '/']).to_owned()
}

/// `inner` is `outer` or below it.
fn is_inside(inner: &str, outer: &str) -> bool {
    inner == outer
        || inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with(['\\', '/']))
}

/// The roots without any that lies inside another (Pictures inside a
/// redirected Documents) or repeats one: the outer root walks it once.
pub fn dedupe_roots(roots: &[FileRoot]) -> Vec<FileRoot> {
    let keys: Vec<String> = roots.iter().map(|r| key(&r.path)).collect();
    roots
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            !keys.iter().enumerate().any(|(j, other)| {
                if j == *i {
                    false
                } else if keys[*i] == *other {
                    // An exact repeat keeps its first.
                    j < *i
                } else {
                    is_inside(&keys[*i], other)
                }
            })
        })
        .map(|(_, r)| r.clone())
        .collect()
}

/// The six folders as Windows names them now (a redirected or OneDrive
/// location included); one it cannot name is left out. Never called from a
/// test: it reads the user's real shell folders.
pub fn default_roots() -> Vec<FileRoot> {
    let known = [
        (RootKind::Desktop, FOLDERID_Desktop),
        (RootKind::Documents, FOLDERID_Documents),
        (RootKind::Downloads, FOLDERID_Downloads),
        (RootKind::Pictures, FOLDERID_Pictures),
        (RootKind::Videos, FOLDERID_Videos),
        (RootKind::Music, FOLDERID_Music),
    ];
    known
        .iter()
        .filter_map(|(kind, id)| known_folder(id).map(|path| FileRoot { kind: *kind, path }))
        .collect()
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = raw.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(raw.0 as *const _));
        path.filter(|p| p.is_absolute())
    }
}

/// Every entry under `roots`, breadth first per root so the shallow (likely)
/// items are kept before a cap bites. A root inside another is walked by the
/// outer one only (`dedupe_roots`).
pub fn walk(roots: &[FileRoot], limits: Limits) -> Vec<FileEntry> {
    let roots = dedupe_roots(roots);
    let root_keys: Vec<String> = roots.iter().map(|r| key(&r.path)).collect();
    let mut out = Vec::new();
    for root in &roots {
        if out.len() >= limits.total_max {
            break;
        }
        let room = limits.root_max.min(limits.total_max - out.len());
        walk_root(root, &root_keys, limits.depth, room, &mut out);
    }
    out
}

/// One root, at most `room` entries.
fn walk_root(
    root: &FileRoot,
    root_keys: &[String],
    depth: usize,
    room: usize,
    out: &mut Vec<FileEntry>,
) {
    let start = out.len();
    // Folders still to read, with their level (the root is 0) and their path
    // below the root.
    let mut queue: VecDeque<(PathBuf, usize, Option<String>)> = VecDeque::new();
    queue.push_back((root.path.clone(), 0, None));
    while let Some((dir, level, parent)) = queue.pop_front() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        // A folder lists in name order, so a cap cuts the same entries every
        // time.
        let mut children: Vec<(String, std::fs::Metadata, PathBuf)> = read
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                Some((name, e.metadata().ok()?, e.path()))
            })
            .collect();
        children.sort_by_cached_key(|(name, _, _)| name.to_lowercase());
        for (name, meta, path) in children {
            let attrs = meta.file_attributes();
            let is_dir = attrs & DIRECTORY != 0;
            let visit = visit(&name, attrs, meta.file_type().is_symlink());
            if visit == Visit::Skip {
                continue;
            }
            if out.len() - start >= room {
                return;
            }
            let lower = name.to_lowercase();
            // Folders to read later: not a cloud folder (listing it would
            // download it), not another root, not below the depth.
            if visit == Visit::Enter && level + 1 < depth && !root_keys.contains(&key(&path)) {
                let below = match &parent {
                    Some(p) => format!("{p}\\{name}"),
                    None => name.clone(),
                };
                queue.push_back((path.clone(), level + 1, Some(below)));
            }
            out.push(FileEntry {
                name,
                lower,
                path,
                root: root.kind,
                parent: parent.clone(),
                is_dir,
                size: if is_dir { 0 } else { meta.file_size() },
                modified: meta.modified().ok(),
            });
        }
    }
}

/// What the walk does with one entry it lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Visit {
    /// Not kept, not entered.
    Skip,
    /// Kept; a file, or a folder that is not read.
    List,
    /// A folder kept and read later (depth and roots allowing).
    Enter,
}

/// The walk's choice for an entry named `name` with `attrs`; `link` is a
/// symlink or junction (a name-surrogate reparse point, what
/// `FileType::is_symlink` says on Windows).
fn visit(name: &str, attrs: u32, link: bool) -> Visit {
    let is_dir = attrs & DIRECTORY != 0;
    // Only a link leads elsewhere. Other reparse points (OneDrive's cloud
    // tags) hold their own items: a listed cloud folder is read like any.
    if link || skipped(name, attrs, is_dir) {
        Visit::Skip
    } else if is_dir && attrs & RECALL_ON_OPEN == 0 {
        Visit::Enter
    } else {
        Visit::List
    }
}

/// An entry the search never lists: hidden or system, a dot name, a heavy
/// folder, the Recycle Bin, or a shortcut (apps come from the Start Menu).
fn skipped(name: &str, attrs: u32, is_dir: bool) -> bool {
    if attrs & (HIDDEN | SYSTEM) != 0 || name.starts_with('.') {
        return true;
    }
    if is_dir {
        return SKIPPED_DIRS.iter().any(|d| name.eq_ignore_ascii_case(d));
    }
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
}

/// Direct children of `dir` that are not hidden, counted up to `cap + 1`
/// (more than `cap` means "`cap`+"). `None` when the folder cannot be read or
/// is a cloud folder whose listing would download it.
pub fn folder_count_capped(dir: &Path, cap: usize) -> Option<usize> {
    let attrs = std::fs::metadata(dir).ok()?.file_attributes();
    if attrs & RECALL_ON_OPEN != 0 {
        return None;
    }
    let mut n = 0;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let hidden = entry
            .metadata()
            .is_ok_and(|m| m.file_attributes() & (HIDDEN | SYSTEM) != 0);
        if !hidden {
            n += 1;
            if n > cap {
                break;
            }
        }
    }
    Some(n)
}

/// `folder_count_capped` at `COUNT_CAP`.
pub fn folder_count(dir: &Path) -> Option<usize> {
    folder_count_capped(dir, COUNT_CAP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use windows::Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, SetFileAttributesW};
    use windows::core::HSTRING;

    /// A temp tree under `target/` (never a real user folder), removed when
    /// dropped.
    struct TestTree {
        base: PathBuf,
    }

    impl TestTree {
        fn new(name: &str) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let base = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("search-files-tests")
                .join(format!(
                    "{name}-{}-{}",
                    std::process::id(),
                    N.fetch_add(1, Ordering::Relaxed)
                ));
            let _ = fs::remove_dir_all(&base);
            fs::create_dir_all(&base).expect("test tree");
            Self { base }
        }

        /// A file of `len` bytes at `rel` (parents made).
        fn file(&self, rel: &str, len: usize) -> PathBuf {
            let path = self.base.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, vec![7u8; len]).unwrap();
            path
        }

        fn dir(&self, rel: &str) -> PathBuf {
            let path = self.base.join(rel);
            fs::create_dir_all(&path).unwrap();
            path
        }

        fn root(&self, kind: RootKind, rel: &str) -> FileRoot {
            FileRoot {
                kind,
                path: self.dir(rel),
            }
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    fn names(entries: &[FileEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    fn sorted(entries: &[FileEntry]) -> Vec<&str> {
        let mut n = names(entries);
        n.sort_unstable();
        n
    }

    /// Hidden or system, as `attrib +h` would.
    fn set_attributes(path: &Path, attributes: u32) {
        unsafe {
            SetFileAttributesW(
                &HSTRING::from(path.as_os_str()),
                FILE_FLAGS_AND_ATTRIBUTES(attributes),
            )
        }
        .expect("set attributes");
    }

    fn limits(depth: usize, root_max: usize, total_max: usize) -> Limits {
        Limits {
            depth,
            root_max,
            total_max,
        }
    }

    #[test]
    fn the_walk_goes_breadth_first_and_stops_at_its_depth() {
        let t = TestTree::new("depth");
        let root = t.root(RootKind::Documents, "Docs");
        t.file("Docs/top.txt", 1);
        t.file("Docs/A/mid.txt", 1);
        t.file("Docs/A/B/low.txt", 1);
        t.file("Docs/A/B/C/deep.txt", 1);
        // Two folders are read (the root and A): B is listed, not entered.
        let found = walk(std::slice::from_ref(&root), limits(2, 100, 100));
        assert_eq!(names(&found), ["A", "top.txt", "B", "mid.txt"]);
        // One more level reads B too, after everything above it.
        let found = walk(std::slice::from_ref(&root), limits(3, 100, 100));
        assert_eq!(
            names(&found),
            ["A", "top.txt", "B", "mid.txt", "C", "low.txt"]
        );
        // The default depth reads five folders: Docs\A\B\C\D\file is found.
        t.file("Docs/A/B/C/D/file.txt", 1);
        t.file("Docs/A/B/C/D/E/toodeep.txt", 1);
        let found = walk(&[root], Limits::DEFAULT);
        let all = sorted(&found);
        assert!(all.contains(&"file.txt") && all.contains(&"E"));
        assert!(!all.contains(&"toodeep.txt"));
    }

    #[test]
    fn the_walk_caps_entries_per_root_and_in_total() {
        let t = TestTree::new("caps");
        let one = t.root(RootKind::Desktop, "One");
        let two = t.root(RootKind::Downloads, "Two");
        for n in ["a", "b", "c", "d"] {
            t.file(&format!("One/{n}.txt"), 1);
            t.file(&format!("Two/{n}.txt"), 1);
        }
        // A deep file under a folder that sorts first: breadth first keeps
        // the shallow files instead.
        t.file("One/0dir/deep.txt", 1);
        let found = walk(&[one.clone(), two.clone()], limits(5, 3, 100));
        let of = |kind| found.iter().filter(|e| e.root == kind).count();
        assert_eq!((of(RootKind::Desktop), of(RootKind::Downloads)), (3, 3));
        assert_eq!(names(&found)[..3], ["0dir", "a.txt", "b.txt"]);
        assert!(!names(&found).contains(&"deep.txt"));
        // The total stops the second root early.
        let found = walk(&[one, two], limits(5, 4, 6));
        assert_eq!(found.len(), 6);
        assert_eq!(
            found
                .iter()
                .filter(|e| e.root == RootKind::Downloads)
                .count(),
            2
        );
    }

    #[test]
    fn hidden_dot_heavy_and_shortcut_entries_are_skipped() {
        let t = TestTree::new("skips");
        let root = t.root(RootKind::Documents, "Docs");
        t.file("Docs/ok.txt", 1);
        t.dir("Docs/Ok Folder");
        t.file("Docs/.hidden.txt", 1);
        t.file("Docs/.git/config", 1);
        t.file("Docs/node_modules/pkg/index.js", 1);
        t.file("Docs/Node_Modules/x.js", 1);
        t.file("Docs/__pycache__/a.pyc", 1);
        t.file("Docs/$RECYCLE.BIN/junk", 1);
        t.file("Docs/App.lnk", 1);
        t.file("Docs/App2.LNK", 1);
        let hid = t.file("Docs/hid.txt", 1);
        set_attributes(&hid, HIDDEN);
        let sys = t.file("Docs/sys.txt", 1);
        set_attributes(&sys, SYSTEM);
        let hid_dir = t.file("Docs/HidDir/inside.txt", 1);
        set_attributes(hid_dir.parent().unwrap(), HIDDEN | DIRECTORY);
        let found = walk(&[root], Limits::DEFAULT);
        assert_eq!(sorted(&found), ["Ok Folder", "ok.txt"]);
    }

    /// A junction (no privilege needed) or a directory symlink from `link` to
    /// `target`; `false` when neither can be made here.
    fn make_link(link: &Path, target: &Path) -> bool {
        use std::os::windows::process::CommandExt;
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        // No console window of its own: the test binary has none, so a bare
        // `cmd` child would open a Windows Terminal window.
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .creation_flags(CREATE_NO_WINDOW.0)
            .output()
            .is_ok_and(|o| o.status.success())
            && link.exists()
    }

    #[test]
    fn links_are_never_followed() {
        let t = TestTree::new("links");
        let root = t.root(RootKind::Documents, "Docs");
        t.file("Docs/real.txt", 1);
        t.file("Elsewhere/precious.doc", 1);
        let link = t.base.join("Docs").join("Link");
        if !make_link(&link, &t.base.join("Elsewhere")) {
            eprintln!("links_are_never_followed: cannot make links here, skipped");
            return;
        }
        let found = walk(&[root], Limits::DEFAULT);
        assert_eq!(
            names(&found),
            ["real.txt"],
            "neither the link nor what it points at"
        );
    }

    /// A symlink, junction or cloud placeholder.
    const REPARSE_POINT: u32 = 0x400;

    #[test]
    fn cloud_folders_are_walked_but_links_are_not() {
        // A OneDrive folder whose items are already listed locally is a
        // reparse point too (a cloud tag, not a link): it is read like any
        // folder, or a redirected Documents would lose every subfolder.
        assert_eq!(
            visit("Proyek", DIRECTORY | REPARSE_POINT, false),
            Visit::Enter
        );
        // One the provider has not listed yet: kept, not read (that would
        // make it download the listing).
        assert_eq!(
            visit("Arsip", DIRECTORY | REPARSE_POINT | RECALL_ON_OPEN, false),
            Visit::List
        );
        // A symlink or junction is never listed or followed, folder or file.
        assert_eq!(visit("Link", DIRECTORY | REPARSE_POINT, true), Visit::Skip);
        assert_eq!(visit("link.txt", REPARSE_POINT, true), Visit::Skip);
        // An online-only file (a cloud reparse point) is listed.
        assert_eq!(visit("laporan.pdf", REPARSE_POINT, false), Visit::List);
        // Plain folders and files, and the usual skips.
        assert_eq!(visit("Docs", DIRECTORY, false), Visit::Enter);
        assert_eq!(visit("a.txt", 0, false), Visit::List);
        assert_eq!(visit(".git", DIRECTORY, false), Visit::Skip);
        assert_eq!(visit("h.txt", HIDDEN, false), Visit::Skip);
    }

    #[test]
    fn nested_roots_are_walked_once() {
        let t = TestTree::new("nested");
        let docs = t.root(RootKind::Documents, "Docs");
        let pics = t.root(RootKind::Pictures, "Docs/Pictures");
        let music = t.root(RootKind::Music, "Music");
        t.file("Docs/a.txt", 1);
        t.file("Docs/Pictures/p.png", 1);
        t.file("Music/s.mp3", 1);
        let both = [docs.clone(), pics.clone(), music.clone()];
        assert_eq!(dedupe_roots(&both), [docs.clone(), music.clone()]);
        // Whatever the order, and for an exact repeat too.
        assert_eq!(
            dedupe_roots(&[pics.clone(), docs.clone(), docs.clone()]),
            std::slice::from_ref(&docs)
        );
        let found = walk(&both, Limits::DEFAULT);
        assert_eq!(sorted(&found), ["Pictures", "a.txt", "p.png", "s.mp3"]);
        // A name that only starts like a root is not inside it.
        let lookalike = FileRoot {
            kind: RootKind::Videos,
            path: PathBuf::from(format!("{}2", docs.path.display())),
        };
        assert_eq!(dedupe_roots(&[docs.clone(), lookalike.clone()]).len(), 2);
    }

    #[test]
    fn entries_carry_root_parent_size_and_kind() {
        let t = TestTree::new("entries");
        let root = t.root(RootKind::Downloads, "Dl");
        t.file("Dl/a.txt", 5);
        t.file("Dl/Proj/b.md", 7);
        t.file("Dl/Proj/Sub/c.txt", 9);
        let found = walk(&[root], Limits::DEFAULT);
        let by = |name: &str| found.iter().find(|e| e.name == name).unwrap();
        let a = by("a.txt");
        assert_eq!(a.root, RootKind::Downloads);
        assert_eq!((a.size, a.is_dir, a.parent.as_deref()), (5, false, None));
        assert_eq!(a.path, t.base.join("Dl").join("a.txt"));
        assert_eq!(a.lower, "a.txt");
        assert!(a.modified.is_some());
        let proj = by("Proj");
        assert!(proj.is_dir && proj.parent.is_none());
        assert_eq!(proj.lower, "proj");
        assert_eq!(by("b.md").parent.as_deref(), Some("Proj"));
        assert_eq!(by("b.md").size, 7);
        assert_eq!(by("Sub").parent.as_deref(), Some("Proj"));
        assert_eq!(by("c.txt").parent.as_deref(), Some(r"Proj\Sub"));
    }

    #[test]
    fn folder_counts_skip_hidden_and_cap_at_999() {
        let t = TestTree::new("counts");
        t.file("D/a.txt", 1);
        t.file("D/b.txt", 1);
        t.file("D/Sub/inner.txt", 1);
        let hid = t.file("D/hid.txt", 1);
        set_attributes(&hid, HIDDEN);
        let sys = t.file("D/sys.txt", 1);
        set_attributes(&sys, SYSTEM);
        let dir = t.base.join("D");
        // Two files and a folder; the inner file and the hidden ones are not
        // counted.
        assert_eq!(folder_count(&dir), Some(3));
        assert_eq!(folder_count(&t.dir("Empty")), Some(0));
        // The cap: more than it says "more", and stops reading.
        assert_eq!(folder_count_capped(&dir, 2), Some(3));
        assert_eq!(folder_count_capped(&dir, 3), Some(3));
        assert_eq!(folder_count_capped(&dir, 1), Some(2));
        assert_eq!(COUNT_CAP, 999);
        // A folder that is not there.
        assert_eq!(folder_count(&t.base.join("Nope")), None);
    }
}
