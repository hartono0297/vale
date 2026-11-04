//! The shelf files shared with the phones: a small set that lives and dies
//! with the session (`Session` holds it). The app shares every file card of
//! the shelf, so the set follows the shelf. No sockets, no clock and no file
//! system here: the caller checks that a path is a regular file before it
//! asks to share it (for a file on the network, right after, off the UI
//! thread), so every rule is tested without a network.
//!
//! Each shared file gets a random 128-bit id from the session's filler (the
//! same one the token comes from). The phone only ever sees ids and names:
//! a path never leaves this module's owner. `rev` goes up on every change,
//! so the page can tell its list is stale.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::http::Method;
use super::session::{Fill, same};
use crate::shelf::{self, ShelfKind};

/// At most the shelf's own cap.
pub const MAX_SHARED: usize = shelf::MAX_ITEMS;

/// One shared file.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub path: PathBuf,
    /// Lower-cased path, trailing slash trimmed (`shelf::key_of`): what a
    /// shelf card is matched by.
    pub key: String,
    /// The path's last part.
    pub name: String,
    pub kind: ShelfKind,
    /// Devices that finished a download of it.
    pub saved_by: Vec<u64>,
    /// Devices with a download of it in flight.
    pub busy: Vec<u64>,
    /// The JPEG thumbnail once made (a few KB), with the `ETag` of the file
    /// it was made from: a file written over in place (same path, same id)
    /// gets a new one.
    thumb: Option<(String, Arc<Vec<u8>>)>,
}

/// The shared files in the order they were shared.
#[derive(Debug, Default)]
pub struct SharedSet {
    entries: Vec<Entry>,
    rev: u64,
}

fn new_id(fill: Fill) -> Option<String> {
    let mut b = [0u8; 16];
    fill(&mut b).then(|| super::base64url(&b))
}

impl SharedSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Goes up on every change (share, unshare, saved, gone).
    pub fn rev(&self) -> u64 {
        self.rev
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn bump(&mut self) {
        self.rev += 1;
    }

    fn position(&self, id: &str) -> Option<usize> {
        // Every entry is compared, so the time does not tell where a
        // matching id sits.
        let mut at = None;
        for (i, e) in self.entries.iter().enumerate() {
            if same(&e.id, id) {
                at = Some(i);
            }
        }
        at
    }

    /// The entry called `id`, if it is still shared.
    pub fn find(&self, id: &str) -> Option<&Entry> {
        self.position(id).map(|i| &self.entries[i])
    }

    pub fn contains(&self, id: &str) -> bool {
        self.position(id).is_some()
    }

    /// Shares `path` (the caller has seen it is a regular file) and returns
    /// its id. A path already shared (by its key) gives the id it has; a
    /// full set, a nameless path or a filler that fails share nothing.
    pub fn share(&mut self, fill: Fill, path: &Path) -> Option<String> {
        let key = shelf::key_of(path);
        if key.is_empty() {
            return None;
        }
        if let Some(e) = self.entries.iter().find(|e| e.key == key) {
            return Some(e.id.clone());
        }
        if self.entries.len() >= MAX_SHARED {
            return None;
        }
        let name = path.file_name()?.to_string_lossy().into_owned();
        // A clash of two random ids is not going to happen; a filler that
        // answers the same twice (a broken one) is refused all the same.
        let mut id = new_id(fill)?;
        if self.contains(&id) {
            id = new_id(fill)?;
            if self.contains(&id) {
                return None;
            }
        }
        self.entries.push(Entry {
            id: id.clone(),
            path: path.to_path_buf(),
            key,
            name,
            kind: shelf::kind_of(path, false),
            saved_by: Vec::new(),
            busy: Vec::new(),
            thumb: None,
        });
        self.bump();
        Some(id)
    }

    /// Stops sharing `id`. `true` if it was shared.
    pub fn unshare(&mut self, id: &str) -> bool {
        let Some(i) = self.position(id) else {
            return false;
        };
        self.entries.remove(i);
        self.bump();
        true
    }

    /// Stops sharing everything. `true` if anything was.
    pub fn unshare_all(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.entries.clear();
        self.bump();
        true
    }

    /// A phone finished downloading `id` (the page shows "Tersimpan" for
    /// it). `true` the first time this phone does; `false` after, or when
    /// the id is not shared.
    pub fn mark_saved(&mut self, id: &str, device: u64) -> bool {
        let Some(i) = self.position(id) else {
            return false;
        };
        let e = &mut self.entries[i];
        if e.saved_by.contains(&device) {
            return false;
        }
        e.saved_by.push(device);
        self.bump();
        true
    }

    /// `device` starts or ends a download of `id` (no `rev` change: the
    /// page reads this when it polls for a download it started).
    pub fn set_busy(&mut self, id: &str, device: u64, on: bool) {
        let Some(i) = self.position(id) else {
            return;
        };
        let busy = &mut self.entries[i].busy;
        if on {
            busy.push(device);
        } else if let Some(at) = busy.iter().position(|d| *d == device) {
            busy.swap_remove(at);
        }
    }

    /// The thumbnail of `id` made from the file as `tag` (`http::etag`)
    /// says it is now: none when it was made from another version.
    pub fn thumb(&self, id: &str, tag: &str) -> Option<Arc<Vec<u8>>> {
        let (made_from, jpeg) = self.find(id)?.thumb.as_ref()?;
        (made_from == tag).then(|| Arc::clone(jpeg))
    }

    /// Keeps `jpeg`, made from the file as `tag`, in place of any older one.
    pub fn set_thumb(&mut self, id: &str, tag: &str, jpeg: Vec<u8>) {
        if let Some(i) = self.position(id) {
            self.entries[i].thumb = Some((tag.to_owned(), Arc::new(jpeg)));
        }
    }
}

/// What a request asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    File,
    View,
    Thumb,
}

/// What the `Range` header (and `If-Range`) asked of a file of `len` bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeAns {
    /// The whole body (no range, one that is ignored, or a failed `If-Range`).
    Full,
    /// `start..=end_incl`. `open_ended`: it was asked as `bytes=N-` (a resume
    /// to the end), not as a closed `a-b` or a suffix `-n`.
    Partial {
        start: u64,
        end_incl: u64,
        open_ended: bool,
    },
    /// 416.
    Unsatisfiable,
}

/// Whether a finished request makes the file "saved" for that phone (the
/// page shows "Tersimpan"): only a GET of the file or the view that wrote every byte of
/// either the whole body (200) or a resume `bytes=N-` that ran to the last
/// byte. Never a HEAD, a suffix or a closed range, or a thumbnail. A zero
/// length file served in full counts.
pub fn counts_as_taken(
    route: Route,
    method: Method,
    range: RangeAns,
    len: u64,
    all_written: bool,
) -> bool {
    if !all_written || method != Method::Get || route == Route::Thumb {
        return false;
    }
    match range {
        RangeAns::Full => true,
        RangeAns::Partial {
            end_incl,
            open_ended,
            ..
        } => open_ended && end_incl.checked_add(1) == Some(len),
        RangeAns::Unsatisfiable => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEED: AtomicU64 = AtomicU64::new(7);

    /// Not random: a counter run through a mixer, so each call differs.
    fn fake(buf: &mut [u8]) -> bool {
        for b in buf.iter_mut() {
            let mut x = SEED.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
            x ^= x >> 30;
            x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x ^= x >> 27;
            *b = (x >> 11) as u8;
        }
        true
    }

    fn none(_: &mut [u8]) -> bool {
        false
    }

    fn same_bytes(buf: &mut [u8]) -> bool {
        buf.fill(3);
        true
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn an_id_is_128_bits_distinct_and_found_only_while_shared() {
        let mut set = SharedSet::new();
        let a = set.share(fake, &p(r"C:\x\a.pdf")).expect("id");
        let b = set.share(fake, &p(r"C:\x\b.png")).expect("id");
        assert_ne!(a, b);
        // 16 bytes in base64url without padding.
        assert_eq!(a.len(), 22);
        assert!(a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
        assert_eq!(set.find(&a).expect("a").name, "a.pdf");
        assert!(set.find("").is_none());
        assert!(set.find(&a[..21]).is_none(), "a prefix is no id");
        assert!(set.find(&format!("{a}A")).is_none());
        // Same length, one character off.
        let mut off = a.clone().into_bytes();
        off[0] = if off[0] == b'A' { b'B' } else { b'A' };
        assert!(set.find(std::str::from_utf8(&off).expect("ascii")).is_none());
        assert!(set.unshare(&a));
        assert!(set.find(&a).is_none(), "a revoked id is not found");
        assert!(!set.unshare(&a), "and not revoked twice");
        assert!(set.find(&b).is_some());
    }

    #[test]
    fn nothing_is_shared_when_the_filler_fails_or_repeats() {
        let mut set = SharedSet::new();
        assert_eq!(set.share(none, &p(r"C:\x\a.pdf")), None);
        assert!(set.is_empty());
        assert_eq!(set.rev(), 0);
        // A filler that never changes cannot make two ids.
        assert!(set.share(same_bytes, &p(r"C:\x\a.pdf")).is_some());
        assert_eq!(set.share(same_bytes, &p(r"C:\x\b.pdf")), None);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn the_same_path_gives_the_same_id_in_any_letter_case() {
        let mut set = SharedSet::new();
        let a = set.share(fake, &p(r"C:\Docs\Denah.PDF")).expect("id");
        let rev = set.rev();
        assert_eq!(set.share(fake, &p(r"c:\docs\denah.pdf")), Some(a.clone()));
        assert_eq!(set.share(fake, &p(r"C:\Docs\Denah.PDF\")), Some(a));
        assert_eq!(set.len(), 1);
        assert_eq!(set.rev(), rev, "nothing changed");
        assert_eq!(set.entries()[0].key, r"c:\docs\denah.pdf");
    }

    #[test]
    fn at_most_twelve_files_and_a_nameless_path_is_refused() {
        let mut set = SharedSet::new();
        for i in 0..MAX_SHARED {
            assert!(set.share(fake, &p(&format!(r"C:\x\{i}.txt"))).is_some());
        }
        assert_eq!(set.share(fake, &p(r"C:\x\one-more.txt")), None);
        assert_eq!(set.len(), MAX_SHARED);
        // The full set still answers for a file it has.
        assert!(set.share(fake, &p(r"C:\x\0.txt")).is_some());
        let mut empty = SharedSet::new();
        assert_eq!(empty.share(fake, &p("")), None);
        assert_eq!(empty.share(fake, &p(r"C:\")), None, "a drive root has no name");
    }

    #[test]
    fn the_rev_goes_up_on_every_change_and_only_then() {
        let mut set = SharedSet::new();
        let mut rev = set.rev();
        let mut step = |set: &SharedSet, changed: bool| {
            assert_eq!(set.rev() > rev, changed);
            rev = set.rev();
        };
        let a = set.share(fake, &p(r"C:\x\a.pdf")).expect("id");
        step(&set, true);
        set.set_busy(&a, 1, true);
        step(&set, false);
        assert!(set.mark_saved(&a, 1));
        step(&set, true);
        assert!(!set.mark_saved(&a, 1));
        step(&set, false);
        assert!(set.mark_saved(&a, 2), "a second phone");
        step(&set, true);
        assert!(set.unshare_all());
        step(&set, true);
        assert!(!set.unshare_all());
        step(&set, false);
        assert!(!set.mark_saved(&a, 1));
        step(&set, false);
    }

    #[test]
    fn saved_is_per_device() {
        let mut set = SharedSet::new();
        let a = set.share(fake, &p(r"C:\x\a.pdf")).expect("id");
        assert!(set.find(&a).expect("a").saved_by.is_empty());
        assert!(set.mark_saved(&a, 4));
        assert!(!set.mark_saved(&a, 4));
        assert!(set.mark_saved(&a, 5));
        assert!(!set.mark_saved("nope", 4), "not shared");
        let e = set.find(&a).expect("a");
        assert_eq!(e.saved_by, [4, 5]);
    }

    #[test]
    fn busy_is_kept_per_device_and_cleared() {
        let mut set = SharedSet::new();
        let a = set.share(fake, &p(r"C:\x\a.pdf")).expect("id");
        set.set_busy(&a, 1, true);
        set.set_busy(&a, 2, true);
        assert_eq!(set.find(&a).expect("a").busy.len(), 2);
        set.set_busy(&a, 1, false);
        assert_eq!(set.find(&a).expect("a").busy, [2]);
        set.set_busy("nope", 2, false);
        set.unshare(&a);
        set.set_busy(&a, 2, true);
        assert!(set.is_empty(), "a revoked id is not made again");
    }

    #[test]
    fn unsharing_all_empties_the_set_and_a_thumb_goes_with_its_file() {
        let mut set = SharedSet::new();
        let a = set.share(fake, &p(r"C:\x\a.png")).expect("id");
        set.share(fake, &p(r"C:\x\b.png")).expect("id");
        assert!(set.thumb(&a, "1-1").is_none());
        set.set_thumb(&a, "1-1", vec![0xFF, 0xD8, 1]);
        assert_eq!(set.thumb(&a, "1-1").expect("thumb").as_slice(), [0xFF, 0xD8, 1]);
        // The file was written over (another size or time): the old picture
        // is not given for it, and the new one takes its place.
        assert!(set.thumb(&a, "2-5").is_none(), "made from another version");
        set.set_thumb(&a, "2-5", vec![0xFF, 0xD8, 2]);
        assert_eq!(set.thumb(&a, "2-5").expect("thumb").as_slice(), [0xFF, 0xD8, 2]);
        assert!(set.thumb(&a, "1-1").is_none());
        set.unshare(&a);
        assert!(set.thumb(&a, "2-5").is_none());
        assert!(set.unshare_all());
        assert!(set.is_empty());
    }

    #[test]
    fn the_taken_rule_counts_a_whole_get_and_a_resume_to_the_end_only() {
        use Method::{Get, Head};
        use Route::{File, Thumb, View};
        let part = |start, end_incl, open_ended| RangeAns::Partial {
            start,
            end_incl,
            open_ended,
        };
        let taken = counts_as_taken;
        // A full body, either route.
        assert!(taken(File, Get, RangeAns::Full, 100, true));
        assert!(taken(View, Get, RangeAns::Full, 100, true));
        // Not all of it written (a dropped connection, a card taken off).
        assert!(!taken(File, Get, RangeAns::Full, 100, false));
        // Never a HEAD or a thumbnail.
        assert!(!taken(File, Head, RangeAns::Full, 100, true));
        assert!(!taken(Thumb, Get, RangeAns::Full, 100, true));
        // A zero length file served in full.
        assert!(taken(File, Get, RangeAns::Full, 0, true));
        // A resume `bytes=N-` to the last byte counts (also from 0).
        assert!(taken(File, Get, part(40, 99, true), 100, true));
        assert!(taken(File, Get, part(0, 99, true), 100, true));
        // A closed range does not, even one that reaches the end.
        assert!(!taken(File, Get, part(40, 99, false), 100, true));
        assert!(!taken(File, Get, part(10, 19, false), 100, true));
        // A suffix range does not.
        assert!(!taken(File, Get, part(90, 99, false), 100, true));
        // An open range that stopped short of the end is not a whole file.
        assert!(!taken(File, Get, part(40, 60, true), 100, true));
        assert!(!taken(File, Get, RangeAns::Unsatisfiable, 100, true));
        assert!(!taken(File, Get, part(0, u64::MAX, true), 100, true));
    }
}
