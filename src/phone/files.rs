//! The file IO of the files shared with the phone: is a path still a regular
//! file, and open it so that a file in use elsewhere still serves. Nothing
//! here knows about sockets; `server.rs` streams what this opens.
//!
//! The path is looked at every time, not once at ticking: a file can be
//! deleted, replaced by a folder or by a link after it was ticked.
//! `symlink_metadata` is read first and refuses a symlink or a junction
//! (std's `is_symlink` is true for every name-surrogate reparse point), while
//! other reparse points, such as OneDrive's cloud files that Windows 11 puts
//! on Desktop and Documents, are ordinary files and are served (opening one
//! downloads it first). What stays open is the gap between that look and the
//! open: a path swapped for a link in between is followed. The handle that
//! is opened is checked again (`is_file`), which closes the folder case but
//! not the link one.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::time::SystemTime;

/// `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`: a file another
/// program has open (even one that writes to it) is still served.
const SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;

/// A shared file, opened.
#[derive(Debug)]
pub struct Opened {
    pub file: File,
    /// The size now.
    pub len: u64,
    pub mtime: Option<SystemTime>,
}

/// Why a shared file could not be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// Missing, or no longer a regular file (a folder, a link): the id is
    /// revoked and the phone is told 410.
    Gone,
    /// There, but it cannot be opened now (another program holds it with no
    /// sharing, access denied): the id stays, the phone gets an error.
    Busy,
}

fn gone_or_busy(e: &io::Error) -> OpenError {
    if matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    ) {
        OpenError::Gone
    } else {
        OpenError::Busy
    }
}

/// Whether `path` is a regular file right now (not a folder, not a symlink
/// or junction), and its size.
pub fn regular_size(path: &Path) -> Option<u64> {
    regular_file(path).map(|(len, _)| len)
}

/// `regular_size` with the file's modified time.
pub fn regular_file(path: &Path) -> Option<(u64, Option<std::time::SystemTime>)> {
    let m = fs::symlink_metadata(path).ok()?;
    (m.is_file() && !m.file_type().is_symlink()).then(|| (m.len(), m.modified().ok()))
}

/// Looks at `path` and opens it for reading with every share flag.
pub fn open_shared(path: &Path) -> Result<Opened, OpenError> {
    let m = fs::symlink_metadata(path).map_err(|e| gone_or_busy(&e))?;
    if m.file_type().is_symlink() || !m.is_file() {
        return Err(OpenError::Gone);
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(SHARE_ALL)
        .open(path)
        .map_err(|e| gone_or_busy(&e))?;
    let m = file.metadata().map_err(|e| gone_or_busy(&e))?;
    if !m.is_file() {
        return Err(OpenError::Gone);
    }
    Ok(Opened {
        len: m.len(),
        mtime: m.modified().ok(),
        file,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::path::PathBuf;

    /// A fresh folder under `target/test-tmp/` for one test.
    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("test dir");
        dir
    }

    #[test]
    fn a_regular_file_opens_with_its_size_and_time() {
        let dir = temp_dir("phone-files-ok");
        let path = dir.join("a.txt");
        fs::write(&path, b"hello").expect("write");
        assert_eq!(regular_size(&path), Some(5));
        let mut o = open_shared(&path).expect("open");
        assert_eq!(o.len, 5);
        assert!(o.mtime.is_some());
        let mut s = String::new();
        o.file.read_to_string(&mut s).expect("read");
        assert_eq!(s, "hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_or_a_missing_path_is_not_a_shareable_file() {
        let dir = temp_dir("phone-files-no");
        assert_eq!(regular_size(&dir), None, "a folder");
        assert_eq!(open_shared(&dir).unwrap_err(), OpenError::Gone);
        let missing = dir.join("nothing.pdf");
        assert_eq!(regular_size(&missing), None);
        assert_eq!(open_shared(&missing).unwrap_err(), OpenError::Gone);
        // A file that is gone after it was seen.
        let path = dir.join("b.txt");
        fs::write(&path, b"x").expect("write");
        assert!(open_shared(&path).is_ok());
        fs::remove_file(&path).expect("remove");
        assert_eq!(open_shared(&path).unwrap_err(), OpenError::Gone);
        // A file replaced by a folder of the same name.
        fs::create_dir(&path).expect("dir");
        assert_eq!(open_shared(&path).unwrap_err(), OpenError::Gone);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_junction_is_refused_and_a_symlink_when_one_can_be_made() {
        let dir = temp_dir("phone-files-link");
        let target = dir.join("real");
        fs::create_dir(&target).expect("dir");
        let file = dir.join("real.txt");
        fs::write(&file, b"x").expect("write");
        // A junction needs no privilege: `mklink /J`.
        let junction = dir.join("junction");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if made {
            assert_eq!(regular_size(&junction), None);
            assert_eq!(open_shared(&junction).unwrap_err(), OpenError::Gone);
        }
        // A file symlink needs developer mode or elevation: skipped without.
        let link = dir.join("link.txt");
        if std::os::windows::fs::symlink_file(&file, &link).is_ok() {
            assert_eq!(regular_size(&link), None, "a symlink is not shared");
            assert_eq!(open_shared(&link).unwrap_err(), OpenError::Gone);
        } else {
            eprintln!("skipped: no privilege to make a symlink");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_held_open_by_a_writer_still_opens() {
        let dir = temp_dir("phone-files-share");
        let path = dir.join("busy.log");
        // std's own open shares read and write, not delete: our flags take
        // all three, so this one still gets in.
        let mut writer = fs::File::create(&path).expect("create");
        writer.write_all(b"in use").expect("write");
        writer.flush().expect("flush");
        let mut o = open_shared(&path).expect("open while written");
        let mut s = String::new();
        o.file.read_to_string(&mut s).expect("read");
        assert_eq!(s, "in use");
        drop(writer);
        let _ = fs::remove_dir_all(&dir);
    }
}
