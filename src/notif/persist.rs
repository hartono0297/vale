//! `notification-history.bin`: the history and its logos sealed with DPAPI
//! (current user), in a small versioned binary format, written atomically
//! (temp file and rename). Same shape as `clipboard::persist`. A file that
//! cannot be read, opened or parsed loads as an empty history.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use windows::core::w;

use super::store::NotifEntry;
use crate::clipboard::image::Thumb;
use crate::dpapi;

const MAGIC: &[u8; 4] = b"DINH";
/// Written by `encode`: v2 adds the browser site after each entry's body.
const VERSION: u16 = 2;
/// Still read: no site field.
const VERSION_V1: u16 = 1;
/// Refuse absurd counts and lengths in a damaged file before allocating.
const MAX_COUNT: u32 = 10_000;
const MAX_FIELD: u32 = 4 * 1024 * 1024;
/// A logo is at most this many pixels on a side.
const MAX_LOGO_SIDE: u16 = 64;

/// Next to config.json.
pub fn path() -> PathBuf {
    crate::config::path().with_file_name("notification-history.bin")
}

/// What the file holds: the entries (newest first), the id counter and the
/// logos some entry uses, by key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub next_id: u64,
    pub items: Vec<NotifEntry>,
    pub logos: HashMap<String, Thumb>,
}

impl Saved {
    /// The logos of `logos` that some entry in `items` uses.
    pub fn of(next_id: u64, items: &[NotifEntry], logos: &HashMap<String, Thumb>) -> Saved {
        let used: HashSet<String> = items.iter().map(NotifEntry::logo_key).collect();
        Saved {
            next_id,
            items: items.to_vec(),
            logos: logos
                .iter()
                .filter(|(k, _)| used.contains(*k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u32).to_le_bytes());
    out.extend(s.as_bytes());
}

/// The plaintext of `saved`; only logos that an entry uses are written.
pub fn encode(saved: &Saved) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(MAGIC);
    out.extend(VERSION.to_le_bytes());
    out.extend(saved.next_id.to_le_bytes());
    out.extend((saved.items.len() as u32).to_le_bytes());
    for e in &saved.items {
        out.extend(e.id.to_le_bytes());
        out.extend(e.at.to_le_bytes());
        out.push(u8::from(e.read));
        put_str(&mut out, &e.source);
        match &e.app_id {
            Some(id) => {
                out.push(1);
                put_str(&mut out, id);
            }
            None => out.push(0),
        }
        put_str(&mut out, &e.title);
        put_str(&mut out, &e.body);
        match &e.site {
            Some(site) => {
                out.push(1);
                put_str(&mut out, site);
            }
            None => out.push(0),
        }
    }
    let used: HashSet<String> = saved.items.iter().map(NotifEntry::logo_key).collect();
    let mut keys: Vec<&String> = saved.logos.keys().filter(|k| used.contains(*k)).collect();
    keys.sort();
    out.extend((keys.len() as u32).to_le_bytes());
    for key in keys {
        let logo = &saved.logos[key];
        put_str(&mut out, key);
        out.extend((logo.w as u16).to_le_bytes());
        out.extend((logo.h as u16).to_le_bytes());
        out.extend((logo.bgra.len() as u32).to_le_bytes());
        out.extend(&logo.bgra);
    }
    out
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.i..self.i.checked_add(n)?)?;
        self.i += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn len(&mut self) -> Option<usize> {
        let n = self.u32()?;
        (n <= MAX_FIELD).then_some(n as usize)
    }
    fn string(&mut self) -> Option<String> {
        let n = self.len()?;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

/// The contents of `plain`, or `None` when it is not a history this reads.
pub fn decode(plain: &[u8]) -> Option<Saved> {
    let mut r = Reader { b: plain, i: 0 };
    if r.take(4)? != MAGIC {
        return None;
    }
    let version = r.u16()?;
    if version != VERSION && version != VERSION_V1 {
        return None;
    }
    let next_id = r.u64()?;
    let count = r.u32()?;
    if count > MAX_COUNT {
        return None;
    }
    let mut items = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let id = r.u64()?;
        let at = r.u64()? as i64;
        let read = r.u8()? != 0;
        let source = r.string()?;
        let app_id = match r.u8()? {
            0 => None,
            1 => Some(r.string()?),
            _ => return None,
        };
        let title = r.string()?;
        let body = r.string()?;
        let site = if version == VERSION_V1 {
            None
        } else {
            match r.u8()? {
                0 => None,
                1 => Some(r.string()?),
                _ => return None,
            }
        };
        items.push(NotifEntry {
            id,
            at,
            read,
            source,
            app_id,
            title,
            body,
            site,
        });
    }
    let logo_count = r.u32()?;
    if logo_count > MAX_COUNT {
        return None;
    }
    let mut logos = HashMap::new();
    for _ in 0..logo_count {
        let key = r.string()?;
        let (w, h) = (r.u16()?, r.u16()?);
        let n = r.len()?;
        if w == 0 || h == 0 || w > MAX_LOGO_SIDE || h > MAX_LOGO_SIDE {
            return None;
        }
        if n as u64 != u64::from(w) * u64::from(h) * 4 {
            return None;
        }
        let bgra = r.take(n)?.to_vec();
        logos.insert(
            key,
            Thumb {
                w: u32::from(w),
                h: u32::from(h),
                bgra,
            },
        );
    }
    (r.i == plain.len()).then_some(Saved {
        next_id,
        items,
        logos,
    })
}

/// Seals `saved` and writes it to `path` through a temp file.
pub fn save_to(path: &Path, saved: &Saved) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut plain = encode(saved);
    let sealed = dpapi::protect(&plain, w!("Vale notification history"));
    dpapi::wipe(&mut plain);
    let tmp = path.with_extension("bin.tmp");
    std::fs::write(&tmp, sealed?)?;
    std::fs::rename(&tmp, path)
}

/// The history in `path`; empty when the file is missing, not ours, or
/// damaged (never an error: the island starts with no history then).
pub fn load_from(path: &Path) -> Saved {
    let Ok(sealed) = std::fs::read(path) else {
        return Saved::default();
    };
    let Ok(mut plain) = dpapi::unprotect(&sealed) else {
        return Saved::default();
    };
    let saved = decode(&plain).unwrap_or_default();
    dpapi::wipe(&mut plain);
    saved
}

/// The history at startup: the file's when it is kept, else none, and a
/// file left behind (the removal never ran) goes now, as "Simpan setelah
/// restart" off promises.
pub fn start_items(path: &Path, persist: bool) -> Saved {
    if persist {
        load_from(path)
    } else {
        run_write(path, Write::Remove);
        Saved::default()
    }
}

/// Writes happen off the UI thread, one at a time and in order: each save
/// carries a generation, and a save older than the newest asked for is
/// dropped (the newer one writes the newer state).
static LATEST: AtomicU64 = AtomicU64::new(0);
static WRITING: Mutex<()> = Mutex::new(());

/// What a background write does.
pub enum Write {
    Save(Saved),
    /// "Simpan setelah restart" off: the file goes.
    Remove,
}

/// Queues `write` for `path` on a short-lived thread.
pub fn write_in_background(path: PathBuf, write: Write) {
    let generation = LATEST.fetch_add(1, Ordering::SeqCst) + 1;
    let run = move || {
        let _guard = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        if LATEST.load(Ordering::SeqCst) != generation {
            return;
        }
        run_write(&path, write);
    };
    // No thread: nothing is written this time; the next change tries again.
    let _ = std::thread::Builder::new()
        .name("notification-save".into())
        .spawn(run);
}

/// Writes now on this thread (shutdown), after any write still running.
pub fn write_now(path: &Path, write: Write) {
    LATEST.fetch_add(1, Ordering::SeqCst);
    let _guard = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    run_write(path, write);
}

fn run_write(path: &Path, write: Write) {
    match write {
        // A history that cannot be written only loses the history.
        Write::Save(saved) => {
            let _ = save_to(path, &saved);
        }
        Write::Remove => {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(path.with_extension("bin.tmp"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under target/, never %APPDATA%.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("notif-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(id: u64, source: &str, app_id: Option<&str>) -> NotifEntry {
        NotifEntry {
            site: (id == 7).then(|| "web.telegram.org".to_owned()),
            id,
            at: 1_700_000_000 + id as i64,
            read: id % 2 == 0,
            source: source.into(),
            app_id: app_id.map(str::to_owned),
            title: format!("Judul {id} é"),
            body: if id == 3 {
                String::new()
            } else {
                format!("isi {id}")
            },
        }
    }

    fn logo(side: u32, seed: u8) -> Thumb {
        Thumb {
            w: side,
            h: side,
            bgra: (0..side * side * 4)
                .map(|i| (i as u8).wrapping_add(seed))
                .collect(),
        }
    }

    fn sample() -> Saved {
        let items = vec![
            entry(7, "WhatsApp", Some("5319.WhatsApp!App")),
            entry(6, "Chrome", None),
            entry(3, "Teams", Some("MSTeams!App")),
        ];
        let mut logos = HashMap::new();
        logos.insert("5319.WhatsApp!App".to_owned(), logo(48, 1));
        logos.insert("Chrome".to_owned(), logo(16, 9));
        // Used by no entry: not saved.
        logos.insert("Gone".to_owned(), logo(8, 2));
        Saved::of(8, &items, &logos)
    }

    #[test]
    fn only_logos_an_entry_uses_are_kept() {
        let s = sample();
        assert_eq!(s.logos.len(), 2);
        assert!(!s.logos.contains_key("Gone"));
        // `encode` filters too, even for a hand-built value.
        let mut hand = s.clone();
        hand.logos.insert("Gone".into(), logo(8, 2));
        assert_eq!(decode(&encode(&hand)).unwrap(), s);
    }

    #[test]
    fn encode_decode_round_trip_keeps_next_id_and_logos() {
        let s = sample();
        let back = decode(&encode(&s)).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.next_id, 8);
        assert_eq!(back.items[2].body, "");
        assert_eq!(back.items[1].app_id, None);
        assert_eq!(
            decode(&encode(&Saved::default())).unwrap(),
            Saved::default()
        );
    }

    #[test]
    fn encrypted_file_round_trip_in_a_temp_folder() {
        let dir = temp_dir("round-trip");
        let path = dir.join("notification-history.bin");
        let s = sample();
        save_to(&path, &s).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"Judul "), "sealed");
        assert!(!raw.starts_with(MAGIC), "not the plaintext");
        assert_eq!(load_from(&path), s);
        assert!(!path.with_extension("bin.tmp").exists(), "temp renamed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_foreign_corrupt_truncated_and_wrong_version_files_load_empty() {
        let dir = temp_dir("damaged");
        let path = dir.join("notification-history.bin");
        assert_eq!(load_from(&path), Saved::default(), "missing");

        std::fs::write(&path, b"plain text, not sealed").unwrap();
        assert_eq!(load_from(&path), Saved::default(), "not DPAPI");

        let seal = |plain: &[u8]| dpapi::protect(plain, w!("test")).unwrap();
        // Sealed but with another magic (a clipboard file, say).
        let mut foreign = encode(&sample());
        foreign[..4].copy_from_slice(b"DICB");
        std::fs::write(&path, seal(&foreign)).unwrap();
        assert_eq!(load_from(&path), Saved::default(), "foreign magic");

        let mut wrong = encode(&sample());
        wrong[4..6].copy_from_slice(&3u16.to_le_bytes());
        std::fs::write(&path, seal(&wrong)).unwrap();
        assert_eq!(load_from(&path), Saved::default(), "wrong version");

        let whole = encode(&sample());
        std::fs::write(&path, seal(&whole[..whole.len() - 5])).unwrap();
        assert_eq!(load_from(&path), Saved::default(), "truncated");

        let mut extra = whole.clone();
        extra.push(0);
        std::fs::write(&path, seal(&extra)).unwrap();
        assert_eq!(load_from(&path), Saved::default(), "trailing bytes");

        // A count far beyond what the file holds is refused before allocating.
        let mut huge = whole.clone();
        huge[14..18].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode(&huge), None);

        std::fs::write(&path, seal(&whole)).unwrap();
        assert_eq!(load_from(&path), sample(), "and the intact one loads");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The version 1 layout, as the first release wrote it: no site field.
    fn encode_v1(saved: &Saved) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(MAGIC);
        out.extend(1u16.to_le_bytes());
        out.extend(saved.next_id.to_le_bytes());
        out.extend((saved.items.len() as u32).to_le_bytes());
        for e in &saved.items {
            out.extend(e.id.to_le_bytes());
            out.extend(e.at.to_le_bytes());
            out.push(u8::from(e.read));
            put_str(&mut out, &e.source);
            match &e.app_id {
                Some(id) => {
                    out.push(1);
                    put_str(&mut out, id);
                }
                None => out.push(0),
            }
            put_str(&mut out, &e.title);
            put_str(&mut out, &e.body);
        }
        let used: HashSet<String> = saved.items.iter().map(NotifEntry::logo_key).collect();
        let mut keys: Vec<&String> = saved.logos.keys().filter(|k| used.contains(*k)).collect();
        keys.sort();
        out.extend((keys.len() as u32).to_le_bytes());
        for key in keys {
            let logo = &saved.logos[key];
            put_str(&mut out, key);
            out.extend((logo.w as u16).to_le_bytes());
            out.extend((logo.h as u16).to_le_bytes());
            out.extend((logo.bgra.len() as u32).to_le_bytes());
            out.extend(&logo.bgra);
        }
        out
    }

    #[test]
    fn a_v1_file_still_loads_with_no_site() {
        let s = sample();
        assert!(
            s.items.iter().any(|e| e.site.is_some()),
            "the sample has a site"
        );
        let back = decode(&encode_v1(&s)).unwrap();
        assert_eq!(back.items.len(), 3);
        assert!(back.items.iter().all(|e| e.site.is_none()));
        assert_eq!(back.next_id, s.next_id);
        assert_eq!(back.logos, s.logos);
        let others: Vec<_> = back
            .items
            .iter()
            .map(|e| (e.id, &e.title, &e.body))
            .collect();
        let expect: Vec<_> = s.items.iter().map(|e| (e.id, &e.title, &e.body)).collect();
        assert_eq!(others, expect);
        // A trailing byte after a v1 file is still refused.
        let mut extra = encode_v1(&s);
        extra.push(0);
        assert_eq!(decode(&extra), None);
    }

    #[test]
    fn v2_round_trips_an_entry_with_a_site_and_one_without() {
        let s = sample();
        assert_eq!(s.items[0].site.as_deref(), Some("web.telegram.org"));
        assert_eq!(s.items[1].site, None);
        let plain = encode(&s);
        assert_eq!(&plain[4..6], &2u16.to_le_bytes(), "always written as v2");
        let back = decode(&plain).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.items[0].site.as_deref(), Some("web.telegram.org"));
        assert_eq!(back.items[1].site, None);
    }

    #[test]
    fn a_bad_site_flag_is_refused() {
        let s = Saved::of(2, &[entry(1, "Chrome", None)], &HashMap::new());
        let mut plain = encode(&s);
        // Entry 1 has no site: its flag is the last byte before the logo count (4 bytes).
        let flag = plain.len() - 4 - 1;
        assert_eq!(plain[flag], 0);
        plain[flag] = 2;
        assert_eq!(decode(&plain), None);
        plain[flag] = 1;
        assert_eq!(decode(&plain), None, "a flag of 1 with no string behind it");
    }

    #[test]
    fn a_logo_with_the_wrong_size_or_a_bad_side_is_refused() {
        let mut s = sample();
        s.logos.get_mut("Chrome").unwrap().bgra.pop();
        let plain = encode(&s);
        assert_eq!(decode(&plain), None, "length does not match w x h x 4");

        let mut big = sample();
        big.logos.insert("Chrome".into(), logo(80, 0));
        assert_eq!(decode(&encode(&big)), None, "wider than a logo ever is");
    }

    #[test]
    fn start_items_with_persist_off_deletes_the_file_and_starts_empty() {
        let dir = temp_dir("persist-off");
        let path = dir.join("notification-history.bin");
        save_to(&path, &sample()).unwrap();
        std::fs::write(path.with_extension("bin.tmp"), b"x").unwrap();
        assert_eq!(start_items(&path, false), Saved::default());
        assert!(!path.exists());
        assert!(!path.with_extension("bin.tmp").exists());
        // On: the file's contents come back.
        save_to(&path, &sample()).unwrap();
        assert_eq!(start_items(&path, true), sample());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_now_saves_and_removes() {
        let dir = temp_dir("write-now");
        let path = dir.join("notification-history.bin");
        write_now(&path, Write::Save(sample()));
        assert_eq!(load_from(&path), sample());
        write_now(&path, Write::Remove);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
