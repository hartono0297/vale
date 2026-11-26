//! A folder as one stored (uncompressed) zip, streamed to any `Write`. The
//! size is known before a byte is sent (`ZipPlan::len`), so a download can
//! announce its Content-Length: every entry is stored, and the CRC, known
//! only after the file is read, goes in a data descriptor after the data.
//! ZIP64 records are used per entry when a size or an offset reaches
//! `zip64_at`, and for the end record when needed.

use super::files::open_shared;
use chrono::{DateTime, Datelike, Local, Timelike};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Files counted (folders do not count) before a folder is refused.
pub const MAX_FILES: usize = 20_000;

const CHUNK: usize = 64 * 1024;
const FLAGS_DIR: u16 = 1 << 11;
const FLAGS_FILE: u16 = (1 << 3) | (1 << 11);
const ZIP64_AT: u64 = 0xFFFF_FFFF;

pub enum Planned {
    Zip(ZipPlan),
    TooMany,
}

struct Entry {
    /// Relative path with `/`; a folder ends with `/`.
    rel: String,
    path: PathBuf,
    size: u64,
    mtime: Option<SystemTime>,
}

impl Entry {
    fn is_dir(&self) -> bool {
        self.rel.ends_with('/')
    }
}

pub struct ZipPlan {
    entries: Vec<Entry>,
    /// A size or an offset from here on is written as ZIP64.
    zip64_at: u64,
}

/// Where everything lands, from the planned sizes.
struct Layout {
    offsets: Vec<u64>,
    zip64: Vec<bool>,
    cd_offset: u64,
    cd_size: u64,
    end64: bool,
    total: u64,
}

/// Running CRC-32/IEEE: start from `0`, feed the rest as it comes.
pub fn crc32(seed: u32, bytes: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut c = !seed;
    for &b in bytes {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// `None` when `root` is not a plain folder or cannot be read.
pub fn plan(root: &Path) -> Option<Planned> {
    plan_with_cap(root, MAX_FILES)
}

fn plan_with_cap(root: &Path, cap: usize) -> Option<Planned> {
    let m = fs::symlink_metadata(root).ok()?;
    if m.file_type().is_symlink() || !m.is_dir() {
        return None;
    }
    let mut entries = Vec::new();
    let mut files = 0usize;
    let mut stack = vec![(root.to_path_buf(), String::new())];
    let mut first = true;
    while let Some((dir, prefix)) = stack.pop() {
        let read = match fs::read_dir(&dir) {
            Ok(r) => r,
            // The root must be readable; a subfolder that is not is skipped.
            Err(_) if first => return None,
            Err(_) => continue,
        };
        first = false;
        for item in read.flatten() {
            let path = item.path();
            // The directory entry carries the type (reparse tag included) from
            // the listing itself: no extra handle per item.
            let Ok(kind) = item.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let Ok(meta) = item.metadata() else { continue };
            let rel = format!("{prefix}{}", item.file_name().to_string_lossy());
            // A name that does not fit the zip's 16-bit length is left out.
            if rel.len() >= usize::from(u16::MAX) {
                continue;
            }
            if kind.is_dir() {
                let rel = format!("{rel}/");
                stack.push((path.clone(), rel.clone()));
                entries.push(Entry { rel, path, size: 0, mtime: meta.modified().ok() });
            } else if kind.is_file() {
                files += 1;
                if files > cap {
                    return Some(Planned::TooMany);
                }
                entries.push(Entry { rel, path, size: meta.len(), mtime: meta.modified().ok() });
            }
        }
    }
    entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    Some(Planned::Zip(ZipPlan { entries, zip64_at: ZIP64_AT }))
}

/// DOS `(time, date)` in local time, never before 1980.
fn dos_time(t: Option<SystemTime>) -> (u16, u16) {
    let Some(t) = t else { return (0, 0x21) };
    let d = DateTime::<Local>::from(t);
    if d.year() < 1980 {
        return (0, 0x21);
    }
    let year = (d.year() - 1980).min(127) as u16;
    let time = (d.hour() as u16) << 11 | (d.minute() as u16) << 5 | (d.second() as u16 / 2);
    let date = year << 9 | (d.month() as u16) << 5 | d.day() as u16;
    (time, date)
}

fn u16le(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn u32le(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn u64le(v: &mut Vec<u8>, x: u64) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn local_header(e: &Entry, zip64: bool) -> Vec<u8> {
    let (time, date) = dos_time(e.mtime);
    let mut v = Vec::new();
    u32le(&mut v, 0x0403_4b50);
    u16le(&mut v, if zip64 { 45 } else { 20 });
    u16le(&mut v, if e.is_dir() { FLAGS_DIR } else { FLAGS_FILE });
    u16le(&mut v, 0);
    u16le(&mut v, time);
    u16le(&mut v, date);
    u32le(&mut v, 0);
    let small = if zip64 { 0xFFFF_FFFF } else { e.size as u32 };
    u32le(&mut v, small);
    u32le(&mut v, small);
    u16le(&mut v, e.rel.len() as u16);
    u16le(&mut v, if zip64 { 20 } else { 0 });
    v.extend_from_slice(e.rel.as_bytes());
    if zip64 {
        u16le(&mut v, 1);
        u16le(&mut v, 16);
        u64le(&mut v, e.size);
        u64le(&mut v, e.size);
    }
    v
}

fn descriptor(e: &Entry, zip64: bool, crc: u32) -> Vec<u8> {
    let mut v = Vec::new();
    u32le(&mut v, 0x0807_4b50);
    u32le(&mut v, crc);
    if zip64 {
        u64le(&mut v, e.size);
        u64le(&mut v, e.size);
    } else {
        u32le(&mut v, e.size as u32);
        u32le(&mut v, e.size as u32);
    }
    v
}

fn central_header(e: &Entry, zip64: bool, crc: u32, offset: u64) -> Vec<u8> {
    let (time, date) = dos_time(e.mtime);
    let mut v = Vec::new();
    u32le(&mut v, 0x0201_4b50);
    u16le(&mut v, 45);
    u16le(&mut v, if zip64 { 45 } else { 20 });
    u16le(&mut v, if e.is_dir() { FLAGS_DIR } else { FLAGS_FILE });
    u16le(&mut v, 0);
    u16le(&mut v, time);
    u16le(&mut v, date);
    u32le(&mut v, crc);
    let small = if zip64 { 0xFFFF_FFFF } else { e.size as u32 };
    u32le(&mut v, small);
    u32le(&mut v, small);
    u16le(&mut v, e.rel.len() as u16);
    u16le(&mut v, if zip64 { 28 } else { 0 });
    u16le(&mut v, 0);
    u16le(&mut v, 0);
    u16le(&mut v, 0);
    u32le(&mut v, if e.is_dir() { 0x10 } else { 0 });
    u32le(&mut v, if zip64 { 0xFFFF_FFFF } else { offset as u32 });
    v.extend_from_slice(e.rel.as_bytes());
    if zip64 {
        u16le(&mut v, 1);
        u16le(&mut v, 24);
        u64le(&mut v, e.size);
        u64le(&mut v, e.size);
        u64le(&mut v, offset);
    }
    v
}

fn end_records(l: &Layout, count: usize) -> Vec<u8> {
    let mut v = Vec::new();
    if l.end64 {
        u32le(&mut v, 0x0606_4b50);
        u64le(&mut v, 44);
        u16le(&mut v, 45);
        u16le(&mut v, 45);
        u32le(&mut v, 0);
        u32le(&mut v, 0);
        u64le(&mut v, count as u64);
        u64le(&mut v, count as u64);
        u64le(&mut v, l.cd_size);
        u64le(&mut v, l.cd_offset);
        u32le(&mut v, 0x0706_4b50);
        u32le(&mut v, 0);
        u64le(&mut v, l.cd_offset + l.cd_size);
        u32le(&mut v, 1);
    }
    u32le(&mut v, 0x0605_4b50);
    u16le(&mut v, 0);
    u16le(&mut v, 0);
    // With the ZIP64 end record present, readers take the real values from it.
    let n = if l.end64 { 0xFFFF } else { count as u16 };
    u16le(&mut v, n);
    u16le(&mut v, n);
    u32le(&mut v, if l.end64 { 0xFFFF_FFFF } else { l.cd_size as u32 });
    u32le(&mut v, if l.end64 { 0xFFFF_FFFF } else { l.cd_offset as u32 });
    u16le(&mut v, 0);
    v
}

impl ZipPlan {
    fn layout(&self) -> Layout {
        let at = self.zip64_at;
        let (mut off, mut cd_size) = (0u64, 0u64);
        let mut offsets = Vec::with_capacity(self.entries.len());
        let mut zip64 = Vec::with_capacity(self.entries.len());
        for e in &self.entries {
            let z = e.size >= at || off >= at;
            offsets.push(off);
            zip64.push(z);
            let name = e.rel.len() as u64;
            off += 30 + name + if z { 20 } else { 0 } + e.size;
            if !e.is_dir() {
                off += if z { 24 } else { 16 };
            }
            cd_size += 46 + name + if z { 28 } else { 0 };
        }
        let end64 = zip64.iter().any(|z| *z)
            || off >= at
            || cd_size >= at
            || self.entries.len() >= 0xFFFF;
        let total = off + cd_size + if end64 { 56 + 20 } else { 0 } + 22;
        Layout { offsets, zip64, cd_offset: off, cd_size, end64, total }
    }

    /// The exact size of the zip `write` produces.
    pub fn len(&self) -> u64 {
        self.layout().total
    }

    /// The latest modified time among the entries.
    pub fn newest(&self) -> Option<SystemTime> {
        self.entries.iter().filter_map(|e| e.mtime).max()
    }

    /// `true` only when all `len()` bytes were written. `keep_going` is asked
    /// before each header and each 64 KiB chunk; `false` stops the write.
    pub fn write(&self, out: &mut dyn Write, keep_going: &mut dyn FnMut() -> bool) -> bool {
        matches!(self.write_all(out, keep_going), Ok(true))
    }

    fn write_all(&self, out: &mut dyn Write, keep_going: &mut dyn FnMut() -> bool) -> io::Result<bool> {
        let l = self.layout();
        let mut written = 0u64;
        let mut crcs = Vec::with_capacity(self.entries.len());
        let mut buf = vec![0u8; CHUNK];
        for (e, &z) in self.entries.iter().zip(&l.zip64) {
            if !keep_going() {
                return Ok(false);
            }
            let head = local_header(e, z);
            out.write_all(&head)?;
            written += head.len() as u64;
            let mut crc = 0u32;
            if !e.is_dir() {
                let Ok(opened) = open_shared(&e.path) else { return Ok(false) };
                if opened.len != e.size {
                    return Ok(false);
                }
                let mut file = opened.file;
                let mut left = e.size;
                while left > 0 {
                    if !keep_going() {
                        return Ok(false);
                    }
                    let want = left.min(CHUNK as u64) as usize;
                    let n = file.read(&mut buf[..want])?;
                    if n == 0 {
                        return Ok(false);
                    }
                    crc = crc32(crc, &buf[..n]);
                    out.write_all(&buf[..n])?;
                    left -= n as u64;
                    written += n as u64;
                }
                let d = descriptor(e, z, crc);
                out.write_all(&d)?;
                written += d.len() as u64;
            }
            crcs.push(crc);
        }
        for (i, e) in self.entries.iter().enumerate() {
            let c = central_header(e, l.zip64[i], crcs[i], l.offsets[i]);
            out.write_all(&c)?;
            written += c.len() as u64;
        }
        let end = end_records(&l, self.entries.len());
        out.write_all(&end)?;
        written += end.len() as u64;
        out.flush()?;
        Ok(written == l.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::files::tests::temp_dir;
    use std::os::windows::process::CommandExt;
    use windows::Win32::System::Threading::CREATE_NO_WINDOW;

    const UNI: &str = "Laporan ü 日本.txt";

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn make_tree(root: &Path) {
        fs::write(root.join("a.txt"), "hello").unwrap();
        fs::create_dir_all(root.join("sub").join("empty")).unwrap();
        fs::write(root.join("sub").join("b.bin"), pattern(100 * 1024)).unwrap();
        fs::write(root.join(UNI), "unicode").unwrap();
    }

    fn zip_of(root: &Path, tweak: impl FnOnce(&mut ZipPlan)) -> (Vec<u8>, u64) {
        let Some(Planned::Zip(mut p)) = plan(root) else { panic!("not planned") };
        tweak(&mut p);
        let mut buf = Vec::new();
        assert!(p.write(&mut buf, &mut || true));
        (buf, p.len())
    }

    fn tar(args: &[&std::ffi::OsStr]) -> bool {
        std::process::Command::new("tar")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW.0)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn assert_extracts_like(root: &Path, buf: &[u8], name: &str) {
        let work = temp_dir(name);
        let zip = work.join("x.zip");
        fs::write(&zip, buf).unwrap();
        let out = work.join("out");
        fs::create_dir_all(&out).unwrap();
        assert!(tar(&["-xf".as_ref(), zip.as_os_str(), "-C".as_ref(), out.as_os_str()]));
        for rel in ["a.txt", "sub/b.bin", UNI] {
            assert_eq!(fs::read(root.join(rel)).unwrap(), fs::read(out.join(rel)).unwrap(), "{rel}");
        }
        assert!(out.join("sub").join("empty").is_dir());
        assert!(!out.join(root.file_name().unwrap()).exists());
    }

    #[test]
    fn crc32_matches_the_check_value() {
        assert_eq!(crc32(0, b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(crc32(0, b"1234"), b"56789"), 0xCBF4_3926);
    }

    #[test]
    fn a_zip_extracts_to_the_same_tree() {
        let root = temp_dir("zip-tree");
        make_tree(&root);
        let (buf, len) = zip_of(&root, |_| {});
        assert_eq!(buf.len() as u64, len);
        assert_extracts_like(&root, &buf, "zip-tree-out");
    }

    #[test]
    fn an_empty_folder_is_a_valid_zip() {
        let root = temp_dir("zip-empty");
        let (buf, len) = zip_of(&root, |_| {});
        assert_eq!(buf.len() as u64, len);
        let zip = temp_dir("zip-empty-out").join("x.zip");
        fs::write(&zip, &buf).unwrap();
        assert!(tar(&["-tf".as_ref(), zip.as_os_str()]));
    }

    #[test]
    fn zip64_records_extract_too() {
        let root = temp_dir("zip-64");
        make_tree(&root);
        let (buf, len) = zip_of(&root, |p| p.zip64_at = 1);
        assert_eq!(buf.len() as u64, len);
        assert_extracts_like(&root, &buf, "zip-64-out");
    }

    #[test]
    fn zip64_and_plain_entries_mix_in_one_archive() {
        let root = temp_dir("zip-mixed");
        make_tree(&root);
        // b.bin (100 KiB) crosses the threshold, the small files do not.
        let (buf, len) = zip_of(&root, |p| p.zip64_at = 50 * 1024);
        assert_eq!(buf.len() as u64, len);
        assert_extracts_like(&root, &buf, "zip-mixed-out");
    }

    #[test]
    fn a_file_that_changes_size_cuts_the_stream() {
        let root = temp_dir("zip-grow");
        make_tree(&root);
        let Some(Planned::Zip(p)) = plan(&root) else { panic!() };
        let mut f = fs::OpenOptions::new().append(true).open(root.join("a.txt")).unwrap();
        f.write_all(b" more").unwrap();
        drop(f);
        assert!(!p.write(&mut Vec::new(), &mut || true));
    }

    #[test]
    fn keep_going_false_stops_the_write() {
        let root = temp_dir("zip-stop");
        make_tree(&root);
        let Some(Planned::Zip(p)) = plan(&root) else { panic!() };
        let mut buf = Vec::new();
        let mut calls = 0;
        assert!(!p.write(&mut buf, &mut || {
            calls += 1;
            calls < 4
        }));
        assert!((buf.len() as u64) < p.len());
    }

    #[test]
    fn links_inside_are_skipped() {
        let root = temp_dir("zip-link");
        make_tree(&root);
        let link = root.join("sub").join("loop");
        let made = std::os::windows::fs::symlink_dir(&root, &link).is_ok()
            || std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&root)
                .creation_flags(CREATE_NO_WINDOW.0)
                .output()
                .is_ok_and(|o| o.status.success());
        if !made || !link.exists() {
            return;
        }
        let Some(Planned::Zip(p)) = plan(&root) else { panic!() };
        assert!(p.entries.iter().all(|e| !e.rel.contains("loop")));
        assert!(p.entries.iter().any(|e| e.rel == "sub/b.bin"));
        let _ = fs::remove_dir(&link);
    }

    #[test]
    fn too_many_files_is_reported() {
        let root = temp_dir("zip-many");
        for n in 0..3 {
            fs::write(root.join(format!("{n}.txt")), "x").unwrap();
        }
        assert!(matches!(plan_with_cap(&root, 2), Some(Planned::TooMany)));
        assert!(matches!(plan_with_cap(&root, 3), Some(Planned::Zip(_))));
    }

    #[test]
    fn a_file_or_missing_path_is_not_planned() {
        let root = temp_dir("zip-none");
        let file = root.join("f.txt");
        fs::write(&file, "x").unwrap();
        assert!(plan(&file).is_none());
        assert!(plan(&root.join("missing")).is_none());
    }
}
