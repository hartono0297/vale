//! `clipboard-history.bin`: the history sealed with DPAPI (current user),
//! in a small versioned binary format, written atomically (temp file and
//! rename). Thumbnails are stored, full-size images never are. A file that
//! cannot be read, opened or parsed loads as an empty history.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use windows::core::w;

use super::image::Thumb;
use super::kinds::ClipKind;
use super::store::{Clip, ClipData, ClipImage, PackState};
use crate::dpapi;

const MAGIC: &[u8; 4] = b"DICB";
const VERSION: u16 = 1;
/// Refuses absurd counts and lengths in a damaged file before allocating.
const MAX_COUNT: u32 = 10_000;
const MAX_FIELD: u32 = 16 * 1024 * 1024;

/// Next to config.json.
pub fn path() -> PathBuf {
    crate::config::path().with_file_name("clipboard-history.bin")
}

/// The plaintext of `items` (newest first).
pub fn encode(items: &[Clip]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(MAGIC);
    out.extend(VERSION.to_le_bytes());
    out.extend((items.len() as u32).to_le_bytes());
    let put_str = |out: &mut Vec<u8>, s: &str| {
        out.extend((s.len() as u32).to_le_bytes());
        out.extend(s.as_bytes());
    };
    for c in items {
        out.push(c.kind.tag());
        out.push(u8::from(c.pinned));
        out.extend(c.at.to_le_bytes());
        match &c.data {
            ClipData::Text(t) => {
                out.push(0);
                put_str(&mut out, t);
            }
            ClipData::Files(paths) => {
                out.push(1);
                out.extend((paths.len() as u32).to_le_bytes());
                for p in paths {
                    put_str(&mut out, &p.to_string_lossy());
                }
            }
            ClipData::Image(img) => {
                out.push(2);
                out.extend(img.width.to_le_bytes());
                out.extend(img.height.to_le_bytes());
                out.extend(img.fingerprint.to_le_bytes());
                out.push(u8::from(img.screenshot));
                out.extend(img.thumb.w.to_le_bytes());
                out.extend(img.thumb.h.to_le_bytes());
                out.extend((img.thumb.bgra.len() as u32).to_le_bytes());
                out.extend(&img.thumb.bgra);
            }
        }
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

/// The entries in `plain`, or `None` when it is not a history this reads.
pub fn decode(plain: &[u8]) -> Option<Vec<Clip>> {
    let mut r = Reader { b: plain, i: 0 };
    if r.take(4)? != MAGIC || r.u16()? != VERSION {
        return None;
    }
    let count = r.u32()?;
    if count > MAX_COUNT {
        return None;
    }
    let mut items = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let kind = ClipKind::from_tag(r.u8()?)?;
        let pinned = r.u8()? != 0;
        let at = r.u64()? as i64;
        let data = match r.u8()? {
            0 => ClipData::Text(r.string()?),
            1 => {
                let n = r.u32()?;
                if n > MAX_COUNT {
                    return None;
                }
                let paths = (0..n)
                    .map(|_| r.string().map(PathBuf::from))
                    .collect::<Option<Vec<_>>>()?;
                ClipData::Files(paths)
            }
            2 => {
                let (width, height) = (r.u32()?, r.u32()?);
                let fingerprint = r.u64()?;
                let screenshot = r.u8()? != 0;
                let (w, h) = (r.u32()?, r.u32()?);
                let n = r.len()?;
                if n as u64 != u64::from(w) * u64::from(h) * 4 {
                    return None;
                }
                let bgra = r.take(n)?.to_vec();
                ClipData::Image(ClipImage {
                    width,
                    height,
                    thumb: Thumb { w, h, bgra },
                    fingerprint,
                    screenshot,
                    full: None,
                    pack: PackState::Idle,
                })
            }
            _ => return None,
        };
        let fits = match &data {
            ClipData::Text(_) => matches!(
                kind,
                ClipKind::Text | ClipKind::Link | ClipKind::Code | ClipKind::Color
            ),
            ClipData::Files(_) => kind == ClipKind::Files,
            ClipData::Image(_) => kind == ClipKind::Image,
        };
        if !fits {
            return None;
        }
        items.push(Clip {
            id: 0,
            kind,
            data,
            pinned,
            at,
        });
    }
    (r.i == plain.len()).then_some(items)
}

/// Seals `items` and writes them to `path` through a temp file.
pub fn save_to(path: &Path, items: &[Clip]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut plain = encode(items);
    let sealed = dpapi::protect(&plain, w!("Vale clipboard history"));
    dpapi::wipe(&mut plain);
    let tmp = path.with_extension("bin.tmp");
    std::fs::write(&tmp, sealed?)?;
    std::fs::rename(&tmp, path)
}

/// The history in `path`; empty when the file is missing, not ours, or
/// damaged (never an error: the island starts with no history then).
pub fn load_from(path: &Path) -> Vec<Clip> {
    let Ok(sealed) = std::fs::read(path) else {
        return Vec::new();
    };
    let Ok(mut plain) = dpapi::unprotect(&sealed) else {
        return Vec::new();
    };
    let items = decode(&plain).unwrap_or_default();
    dpapi::wipe(&mut plain);
    items
}

/// The history at startup: the file's when it is kept, else none, and a
/// file left behind (the removal never ran) goes now, as "Hanya di memori"
/// promises.
pub fn start_items(path: &Path, persist: bool) -> Vec<Clip> {
    if persist {
        load_from(path)
    } else {
        run_write(path, Write::Remove);
        Vec::new()
    }
}

/// Writes happen off the UI thread, one at a time and in order: each save
/// carries a generation, and a save older than the newest asked for is
/// dropped (the newer one writes the newer state).
static LATEST: AtomicU64 = AtomicU64::new(0);
static WRITING: Mutex<()> = Mutex::new(());

/// What a background write does.
pub enum Write {
    Save(Vec<Clip>),
    /// "Hanya di memori": the file goes.
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
        .name("clipboard-save".into())
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
        Write::Save(items) => {
            let _ = save_to(path, &items);
        }
        Write::Remove => {
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(path.with_extension("bin.tmp"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::image::tests::dib24;
    use super::super::store::{ClipSnapshot, History, ingest};
    use super::*;

    /// A fresh folder under target/, never %APPDATA%.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("clipboard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> History {
        let mut h = History::new(50);
        let snap = |s: &str| ClipSnapshot {
            text: Some(s.into()),
            ..ClipSnapshot::default()
        };
        ingest(&mut h, snap("rahasia-teks-biasa"), 100);
        ingest(&mut h, snap("github.com/tw93/mole"), 200);
        let files = ClipSnapshot {
            files: vec![PathBuf::from(r"D:\a b\c.pdf"), PathBuf::from(r"E:\x")],
            ..ClipSnapshot::default()
        };
        ingest(&mut h, files, 300);
        let img = ClipSnapshot {
            dib: Some(dib24(64, 40)),
            only_bitmaps: true,
            ..ClipSnapshot::default()
        };
        ingest(&mut h, img, 400);
        let id = h.items()[2].id;
        h.set_pinned(id, true);
        h
    }

    /// Without the memory-only parts (ids are renumbered, full images dropped).
    fn comparable(items: &[Clip]) -> Vec<Clip> {
        items
            .iter()
            .cloned()
            .map(|mut c| {
                c.id = 0;
                if let ClipData::Image(img) = &mut c.data {
                    img.full = None;
                    img.pack = PackState::Idle;
                }
                c
            })
            .collect()
    }

    #[test]
    fn encode_decode_round_trip() {
        let h = sample();
        let plain = encode(h.items());
        assert_eq!(decode(&plain).unwrap(), comparable(h.items()));
        assert_eq!(decode(&encode(&[])).unwrap(), Vec::new());
    }

    #[test]
    fn encrypted_file_round_trip_in_a_temp_folder() {
        let dir = temp_dir("round-trip");
        let path = dir.join("clipboard-history.bin");
        let h = sample();
        save_to(&path, h.items()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let needle = b"rahasia-teks-biasa";
        assert!(
            !bytes.windows(needle.len()).any(|w| w == needle),
            "history stored in plain text"
        );
        assert!(!dir.join("clipboard-history.bin.tmp").exists());
        let loaded = load_from(&path);
        assert_eq!(loaded, comparable(h.items()));
        let restored = History::from_items(50, loaded);
        assert_eq!(restored.len(), 4);
        assert!(restored.items()[2].pinned);
        run_write(&path, Write::Remove);
        assert!(!path.exists());
        assert!(load_from(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn memory_only_at_startup_removes_a_file_left_behind() {
        // "Simpan setelah restart" was turned off but the removal never ran
        // (quit at once, or config.json edited while closed).
        let dir = temp_dir("memory-only");
        let path = dir.join("clipboard-history.bin");
        save_to(&path, sample().items()).unwrap();
        std::fs::write(path.with_extension("bin.tmp"), b"half").unwrap();
        assert!(start_items(&path, false).is_empty());
        assert!(!path.exists() && !path.with_extension("bin.tmp").exists());
        // Kept: loaded as before.
        save_to(&path, sample().items()).unwrap();
        assert_eq!(start_items(&path, true).len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_files_load_as_empty() {
        let dir = temp_dir("corrupt");
        let path = dir.join("clipboard-history.bin");
        // Not DPAPI at all.
        std::fs::write(&path, b"garbage").unwrap();
        assert!(load_from(&path).is_empty());
        // Sealed, but not a history.
        let sealed = dpapi::protect(b"DICB\x09\x00", w!("t")).unwrap();
        std::fs::write(&path, sealed).unwrap();
        assert!(load_from(&path).is_empty());
        // A sealed history cut short.
        let mut plain = encode(sample().items());
        plain.truncate(plain.len() - 3);
        let sealed = dpapi::protect(&plain, w!("t")).unwrap();
        std::fs::write(&path, sealed).unwrap();
        assert!(load_from(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decode_rejects_damage() {
        let plain = encode(sample().items());
        assert!(decode(&plain[..plain.len() - 1]).is_none());
        let mut extra = plain.clone();
        extra.push(0);
        assert!(decode(&extra).is_none());
        let mut version = plain.clone();
        version[4] = 2;
        assert!(decode(&version).is_none());
        let mut kind = plain.clone();
        // The first entry is the image: say it is a text kind.
        kind[10] = ClipKind::Text.tag();
        assert!(decode(&kind).is_none());
        assert!(decode(b"").is_none());
    }
}
