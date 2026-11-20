//! The saved signatures and initials, sealed with DPAPI so only this Windows
//! account can read them. Plain form: `VSIG1`, the signature and initials
//! counts (one byte each), then per mark `w`, `h`, `len` (u32 LE) and a PNG.

use std::io;
use std::path::{Path, PathBuf};

use windows::core::w;

use super::Mark;
use super::raster;
use crate::dpapi;

const MAGIC: &[u8; 5] = b"VSIG1";
/// How many signatures and how many initials are kept.
const KEEP: usize = 4;

#[derive(Default)]
pub struct Saved {
    pub signatures: Vec<Mark>,
    pub initials: Vec<Mark>,
}

pub fn path() -> PathBuf {
    crate::data_dir::dir().join("signatures.bin")
}

fn encode(s: &Saved) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(u8::try_from(s.signatures.len().min(KEEP)).ok()?);
    out.push(u8::try_from(s.initials.len().min(KEEP)).ok()?);
    let marks = s
        .signatures
        .iter()
        .take(KEEP)
        .chain(s.initials.iter().take(KEEP));
    for m in marks {
        let png = raster::encode_png_rgba(m)?;
        out.extend_from_slice(&m.w.to_le_bytes());
        out.extend_from_slice(&m.h.to_le_bytes());
        out.extend_from_slice(&u32::try_from(png.len()).ok()?.to_le_bytes());
        out.extend_from_slice(&png);
    }
    Some(out)
}

fn take<'a>(plain: &'a [u8], at: &mut usize, n: usize) -> Option<&'a [u8]> {
    let end = at.checked_add(n)?;
    let part = plain.get(*at..end)?;
    *at = end;
    Some(part)
}

fn take_u32(plain: &[u8], at: &mut usize) -> Option<u32> {
    Some(u32::from_le_bytes(take(plain, at, 4)?.try_into().ok()?))
}

fn decode(plain: &[u8]) -> Option<Saved> {
    let mut at = 0usize;
    if take(plain, &mut at, MAGIC.len())? != MAGIC {
        return None;
    }
    let counts = take(plain, &mut at, 2)?;
    let (n_sig, n_ini) = (usize::from(counts[0]), usize::from(counts[1]));
    if n_sig > KEEP || n_ini > KEEP {
        return None;
    }
    let mut marks = Vec::with_capacity(n_sig + n_ini);
    for _ in 0..n_sig + n_ini {
        let (w, h) = (take_u32(plain, &mut at)?, take_u32(plain, &mut at)?);
        let len = take_u32(plain, &mut at)? as usize;
        let mark = raster::decode_png_rgba(take(plain, &mut at, len)?)?;
        if (mark.w, mark.h) != (w, h) {
            return None;
        }
        marks.push(mark);
    }
    let initials = marks.split_off(n_sig);
    (at == plain.len()).then_some(Saved {
        signatures: marks,
        initials,
    })
}

/// Seals `s` and writes it to `path` through a temp file. Needs COM on the
/// calling thread.
pub fn save_to(path: &Path, s: &Saved) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut plain =
        encode(s).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "cannot encode"))?;
    let sealed = dpapi::protect(&plain, w!("Vale signatures"));
    dpapi::wipe(&mut plain);
    let tmp = path.with_extension("bin.tmp");
    std::fs::write(&tmp, sealed?)?;
    std::fs::rename(&tmp, path)
}

/// What `path` holds; empty when the file is missing, not ours or damaged.
/// Needs COM on the calling thread.
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

/// `m` becomes the newest; a fifth drops the oldest.
pub fn add(s: &mut Saved, m: Mark, initials: bool) {
    let list = if initials {
        &mut s.initials
    } else {
        &mut s.signatures
    };
    list.insert(0, m);
    list.truncate(KEEP);
}

/// Forgets entry `i` (an index past the end does nothing).
pub fn remove(s: &mut Saved, initials: bool, i: usize) {
    let list = if initials {
        &mut s.initials
    } else {
        &mut s.signatures
    };
    if i < list.len() {
        list.remove(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    struct Com;
    impl Com {
        fn new() -> Com {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            Com
        }
    }
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    fn mark(w: u32, h: u32, seed: u8) -> Mark {
        let rgba = (0..(w * h * 4) as usize)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect();
        Mark { w, h, rgba }
    }

    #[test]
    fn store_roundtrip_through_dpapi() {
        let _com = Com::new();
        let dir = crate::phone::files::tests::temp_dir("sign-store");
        let file = dir.join("signatures.bin");
        let s = Saved {
            signatures: vec![mark(30, 10, 1), mark(8, 9, 2)],
            initials: vec![mark(12, 12, 3)],
        };
        save_to(&file, &s).expect("save");
        let bytes = std::fs::read(&file).expect("read");
        assert!(!bytes.windows(5).any(|w| w == b"VSIG1"), "sealed");
        assert!(!dir.join("signatures.bin.tmp").exists());
        let back = load_from(&file);
        assert_eq!(back.signatures.len(), 2);
        assert_eq!(back.initials.len(), 1);
        for (a, b) in s
            .signatures
            .iter()
            .chain(&s.initials)
            .zip(back.signatures.iter().chain(&back.initials))
        {
            assert_eq!((a.w, a.h), (b.w, b.h));
            assert_eq!(a.rgba, b.rgba);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_keeps_four_newest() {
        let mut s = Saved::default();
        for i in 0..5u32 {
            add(&mut s, mark(i + 1, 1, 0), false);
        }
        assert_eq!(s.signatures.len(), 4);
        assert_eq!(s.signatures[0].w, 5, "newest first");
        assert_eq!(s.signatures[3].w, 2, "oldest dropped");
        add(&mut s, mark(9, 1, 0), true);
        assert_eq!(s.initials.len(), 1);
        remove(&mut s, false, 0);
        assert_eq!(s.signatures[0].w, 4);
        remove(&mut s, false, 99);
        assert_eq!(s.signatures.len(), 3);
    }

    #[test]
    fn corrupt_store_loads_empty() {
        let _com = Com::new();
        let dir = crate::phone::files::tests::temp_dir("sign-corrupt");
        let file = dir.join("signatures.bin");
        std::fs::write(&file, b"not a sealed blob at all").expect("write");
        let s = load_from(&file);
        assert!(s.signatures.is_empty() && s.initials.is_empty());
        assert!(load_from(&dir.join("missing.bin")).signatures.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
