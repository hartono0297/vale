//! Downloads the engine (with the Silero VAD model) or one speech model in
//! the background: resumable across runs (`<file>.part` plus an HTTP Range
//! request), cancellable, SHA-256 checked before anything is put in place.
//! The engine's `.tar.gz` is unpacked into a staging folder and renamed to
//! `engine\<version>` only once it is whole and its `contract.json` matches.
//!
//! The HTTP side is `google::http::Http::download_resume` (the self-update's
//! WinRT client). The thread exists only while a download runs.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::E_ABORT;
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
};
use windows::Win32::Security::Cryptography::{
    BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALG_HANDLE, BCryptCreateHash, BCryptDestroyHash,
    BCryptFinishHash, BCryptHashData,
};

use super::catalog::{self, Asset, ENGINE, Model};
use super::store::{self, Store};
use crate::google::http::Http;

/// A frozen connection fails after this long without a byte.
const STALL: Duration = Duration::from_secs(30);
/// The unpacked engine is about 56 MB; anything far past that is not it.
const ENGINE_UNPACKED_CAP: usize = 256 * 1024 * 1024;

/// What to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// The native package and the VAD model.
    Engine,
    Model(&'static Model),
    /// The engine and the VAD when missing, then the model: one run, one
    /// `Progress`.
    Setup(&'static Model),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    /// No answer, a timeout or a broken transfer: try again later.
    NoNetwork,
    /// The server answered with this status (404 when an asset was pulled).
    Status(u16),
    Cancelled,
    /// The finished file does not match the catalog (it was deleted).
    Checksum(&'static str),
    /// The disk has no room for the file (ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL).
    DiskFull,
    /// The file could not be written or moved (no access, a write fault).
    Disk(String),
    /// The engine package is not what the catalog describes.
    Archive(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::NoNetwork => write!(f, "no network"),
            DownloadError::Status(code) => write!(f, "server answered {code}"),
            DownloadError::Cancelled => write!(f, "cancelled"),
            DownloadError::Checksum(file) => write!(f, "{file}: SHA-256 does not match"),
            DownloadError::DiskFull => write!(f, "disk full"),
            DownloadError::Disk(why) => write!(f, "disk: {why}"),
            DownloadError::Archive(why) => write!(f, "engine package: {why}"),
        }
    }
}

impl From<io::Error> for DownloadError {
    fn from(error: io::Error) -> Self {
        match error.raw_os_error() {
            Some(112 | 39) => DownloadError::DiskFull,
            _ => DownloadError::Disk(error.to_string()),
        }
    }
}

/// Bytes done out of the bytes this download needs (installed parts are
/// not counted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

impl Progress {
    /// Tenths of a percent, 0 to 1000.
    pub fn permille(self) -> u32 {
        if self.total == 0 {
            return 1000;
        }
        (self.done.min(self.total) * 1000 / self.total) as u32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Sent when the permille changes.
    Progress(Progress),
    Finished(Result<(), DownloadError>),
}

/// A running download. Dropping it does not stop it; `cancel` does.
pub struct Download {
    cancel: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Download {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    pub fn join(self) {
        let _ = self.thread.join();
    }
}

/// Starts `item` on its own thread; `on_event` runs on that thread and gets
/// `Finished` last.
pub fn start(
    store: Store,
    item: Item,
    mut on_event: impl FnMut(Event) + Send + 'static,
) -> io::Result<Download> {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let thread = std::thread::Builder::new()
        .name("transcribe-download".into())
        .spawn(move || {
            // A download is never urgent: low CPU and I/O priority.
            unsafe {
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
            }
            let result = run(&store, item, &flag, &mut on_event);
            on_event(Event::Finished(result));
        })?;
    Ok(Download { cancel, thread })
}

/// Every asset `item` names, installed or not, in download order.
pub fn assets_of(item: Item) -> Vec<Asset> {
    match item {
        Item::Engine => vec![ENGINE.asset, catalog::VAD],
        Item::Model(model) => vec![model.asset],
        Item::Setup(model) => vec![ENGINE.asset, catalog::VAD, model.asset],
    }
}

/// What `item` still needs, in download order.
pub fn missing(store: &Store, item: Item) -> Vec<Asset> {
    assets_of(item)
        .into_iter()
        .filter(|asset| {
            if *asset == ENGINE.asset {
                !store.engine_installed()
            } else if *asset == catalog::VAD {
                !store.vad_installed()
            } else {
                !catalog::MODELS
                    .iter()
                    .any(|m| m.asset == *asset && store.model_installed(m))
            }
        })
        .collect()
}

/// Downloads, checks and installs `item` on the calling thread.
pub fn run(
    store: &Store,
    item: Item,
    cancel: &AtomicBool,
    on_event: &mut impl FnMut(Event),
) -> Result<(), DownloadError> {
    let assets = missing(store, item);
    let total: u64 = assets.iter().map(|a| a.size).sum();
    let http = Http::new()
        .map_err(|_| DownloadError::NoNetwork)?
        .with_timeout(STALL);
    let mut before = 0;
    let mut last = None;
    for asset in assets {
        let part = fetch(&http, store, &asset, cancel, &mut |done| {
            let progress = Progress {
                done: before + done,
                total,
            };
            if last != Some(progress.permille()) {
                last = Some(progress.permille());
                on_event(Event::Progress(progress));
            }
        })?;
        if asset == ENGINE.asset {
            install_engine(store, &part)?;
            let _ = fs::remove_file(&part);
        } else {
            let target = if asset == catalog::VAD {
                store.vad_path()
            } else {
                store.models_dir().join(asset.file)
            };
            fs::create_dir_all(store.models_dir())?;
            let _ = fs::remove_file(&target);
            fs::rename(&part, &target)?;
        }
        before += asset.size;
    }
    Ok(())
}

/// What to do with a partial file of `len` bytes for a file of `size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Fresh,
    /// Ask for the bytes from this offset on.
    Resume(u64),
    /// Everything is there: only the checksum is left.
    Verify,
}

pub fn plan(len: u64, size: u64) -> Plan {
    match len {
        0 => Plan::Fresh,
        n if n < size => Plan::Resume(n),
        n if n == size => Plan::Verify,
        _ => Plan::Fresh,
    }
}

/// Downloads `asset` into its `.part` file and checks it; the verified part's
/// path. `progress` gets the bytes of this asset on disk.
fn fetch(
    http: &Http,
    store: &Store,
    asset: &Asset,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64),
) -> Result<PathBuf, DownloadError> {
    let part = store.part_path(asset);
    fs::create_dir_all(store.downloads_dir())?;
    let agent = concat!("Vale/", env!("CARGO_PKG_VERSION"));
    let headers = [("User-Agent", agent)];
    let mut fresh_retry = true;
    loop {
        let len = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let from = match plan(len, asset.size) {
            Plan::Verify => break,
            Plan::Resume(from) => from,
            Plan::Fresh => {
                fs::File::create(&part)?;
                0
            }
        };
        progress(from);
        let status = http
            .download_resume(asset.url, &headers, &part, from, asset.size, cancel, |done, _| {
                progress(done)
            })
            .map_err(|e| classify(e.code()))?;
        match status {
            200 | 206 => {
                let len = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                if len != asset.size {
                    return Err(DownloadError::NoNetwork);
                }
                break;
            }
            // The part does not fit the file any more: once more from zero.
            416 if fresh_retry => {
                fresh_retry = false;
                fs::remove_file(&part)?;
            }
            other => return Err(DownloadError::Status(other)),
        }
    }
    let expected = catalog::parse_sha256(asset.sha256).expect("catalog hashes are valid");
    if sha256_file(&part, cancel)? != expected {
        let _ = fs::remove_file(&part);
        return Err(DownloadError::Checksum(asset.file));
    }
    Ok(part)
}

/// A failed transfer's HRESULT: cancelled, a disk problem, or the network.
fn classify(code: windows::core::HRESULT) -> DownloadError {
    const DISK: [u32; 3] = [
        0x8007_0005, // ERROR_ACCESS_DENIED
        0x8007_001D, // ERROR_WRITE_FAULT
        0x8007_0020, // ERROR_SHARING_VIOLATION
    ];
    if code == E_ABORT {
        DownloadError::Cancelled
    } else if [0x8007_0070_u32, 0x8007_0027].contains(&(code.0 as u32)) {
        // ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL
        DownloadError::DiskFull
    } else if DISK.contains(&(code.0 as u32)) {
        DownloadError::Disk(format!("0x{:08X}", code.0 as u32))
    } else {
        DownloadError::NoNetwork
    }
}

/// SHA-256 over a byte stream (BCrypt, one hash object per value).
pub struct Sha256(BCRYPT_HASH_HANDLE);

impl Sha256 {
    pub fn new() -> Self {
        let mut handle = BCRYPT_HASH_HANDLE::default();
        // The SHA-256 pseudo-handle needs no provider and does not fail on
        // Windows 10+ (see `google::oauth::sha256`).
        let status =
            unsafe { BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &mut handle, None, None, 0) };
        debug_assert!(status.is_ok(), "BCryptCreateHash failed: {status:?}");
        Sha256(handle)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let status = unsafe { BCryptHashData(self.0, bytes, 0) };
        debug_assert!(status.is_ok(), "BCryptHashData failed: {status:?}");
    }

    pub fn finish(self) -> [u8; 32] {
        let mut digest = [0u8; 32];
        let status = unsafe { BCryptFinishHash(self.0, &mut digest, 0) };
        debug_assert!(status.is_ok(), "BCryptFinishHash failed: {status:?}");
        digest
    }
}

impl Drop for Sha256 {
    fn drop(&mut self) {
        unsafe {
            let _ = BCryptDestroyHash(self.0);
        }
    }
}

/// The SHA-256 of a file read in 1 MB pieces; stops when `cancel` is set.
pub fn sha256_file(path: &Path, cancel: &AtomicBool) -> Result<[u8; 32], DownloadError> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let n = file.read(&mut buffer)?;
        if n == 0 {
            return Ok(hash.finish());
        }
        hash.update(&buffer[..n]);
    }
}

/// Unpacks the verified engine package into `engine_dir`, through the
/// staging folder.
pub fn install_engine(store: &Store, tarball: &Path) -> Result<(), DownloadError> {
    let packed = fs::read(tarball)?;
    let tar = gunzip(&packed, ENGINE_UNPACKED_CAP).map_err(DownloadError::Archive)?;
    let staging = store.engine_staging_dir();
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let result = untar(&tar, &staging).and_then(|_| {
        if !staging.join(ENGINE.dll).is_file() {
            return Err(DownloadError::Archive(format!("no {}", ENGINE.dll)));
        }
        let contract = fs::read_to_string(staging.join("contract.json"))
            .map_err(|_| DownloadError::Archive("no contract.json".into()))?;
        store::check_contract(&contract).map_err(DownloadError::Archive)
    });
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let target = store.engine_dir();
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::rename(&staging, &target)?;
    Ok(())
}

/// The contents of a one-member gzip file, at most `cap` bytes.
pub fn gunzip(data: &[u8], cap: usize) -> Result<Vec<u8>, String> {
    const FHCRC: u8 = 2;
    const FEXTRA: u8 = 4;
    const FNAME: u8 = 8;
    const FCOMMENT: u8 = 16;
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
        return Err("not a gzip file".into());
    }
    let flags = data[3];
    let mut at = 10;
    if flags & FEXTRA != 0 {
        let len = *data.get(at).ok_or("truncated")? as usize
            | (*data.get(at + 1).ok_or("truncated")? as usize) << 8;
        at += 2 + len;
    }
    for flag in [FNAME, FCOMMENT] {
        if flags & flag != 0 {
            let end = data[at.min(data.len())..]
                .iter()
                .position(|&b| b == 0)
                .ok_or("truncated")?;
            at += end + 1;
        }
    }
    if flags & FHCRC != 0 {
        at += 2;
    }
    if at + 8 > data.len() {
        return Err("truncated".into());
    }
    let out = miniz_oxide::inflate::decompress_to_vec_with_limit(&data[at..], cap)
        .map_err(|e| format!("inflate: {e:?}"))?;
    let tail = &data[data.len() - 4..];
    let isize = u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]);
    if isize != out.len() as u32 {
        return Err("size does not match the gzip trailer".into());
    }
    Ok(out)
}

/// Unpacks a ustar / pax tar into `dest`, dropping the first path component
/// (the package's own folder, which every entry must share). Only regular
/// files and folders are written; a path that would leave `dest` fails the
/// whole archive. The files written, relative to `dest`.
pub fn untar(data: &[u8], dest: &Path) -> Result<Vec<PathBuf>, DownloadError> {
    let bad = |why: &str| DownloadError::Archive(why.to_string());
    let mut written = Vec::new();
    let mut top: Option<String> = None;
    let mut long_name: Option<String> = None;
    let mut at = 0;
    while at + 512 <= data.len() {
        let header = &data[at..at + 512];
        if header.iter().all(|&b| b == 0) {
            break;
        }
        if !tar_checksum_ok(header) {
            return Err(bad("bad header checksum"));
        }
        let size = octal(&header[124..136]).ok_or_else(|| bad("bad size"))? as usize;
        let body_start = at + 512;
        let body_end = body_start
            .checked_add(size)
            .filter(|&end| end <= data.len())
            .ok_or_else(|| bad("truncated"))?;
        let body = &data[body_start..body_end];
        at = body_start + size.div_ceil(512) * 512;
        let kind = header[156];
        match kind {
            // A pax extended header: its `path` names the next entry.
            b'x' => {
                long_name = pax_path(body);
                continue;
            }
            // A GNU long name: the next entry's name.
            b'L' => {
                long_name = Some(c_string(body));
                continue;
            }
            b'g' => continue,
            _ => {}
        }
        let name = long_name.take().unwrap_or_else(|| {
            let prefix = c_string(&header[345..500]);
            let name = c_string(&header[0..100]);
            if &header[257..262] == b"ustar" && !prefix.is_empty() {
                format!("{prefix}/{name}")
            } else {
                name
            }
        });
        let (first, rest) = split_top(&name).ok_or_else(|| bad("unsafe path"))?;
        match &top {
            None => top = Some(first.clone()),
            Some(seen) if *seen != first => return Err(bad("more than one top folder")),
            Some(_) => {}
        }
        if rest.as_os_str().is_empty() {
            continue;
        }
        match kind {
            b'0' | 0 => {
                let path = dest.join(&rest);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&path, body)?;
                written.push(rest);
            }
            b'5' => fs::create_dir_all(dest.join(&rest))?,
            // Links and devices: the package has none, and they are never
            // followed or created.
            _ => {}
        }
    }
    Ok(written)
}

/// `top/rest` of a tar path, `None` when any part could leave the folder.
fn split_top(name: &str) -> Option<(String, PathBuf)> {
    if name.starts_with('/') || name.contains('\\') || name.contains(':') {
        return None;
    }
    let mut parts = name.split('/').filter(|p| !p.is_empty() && *p != ".");
    let first = parts.next()?.to_string();
    let mut rest = PathBuf::new();
    for part in parts {
        if part == ".." {
            return None;
        }
        rest.push(part);
    }
    if first == ".." {
        return None;
    }
    Some((first, rest))
}

fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn octal(field: &[u8]) -> Option<u64> {
    let text = c_string(field);
    let text = text.trim_matches(|c: char| c == ' ' || c == '\0');
    if text.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(text, 8).ok()
}

fn tar_checksum_ok(header: &[u8]) -> bool {
    let Some(stored) = octal(&header[148..156]) else {
        return false;
    };
    let sum: u64 = header
        .iter()
        .enumerate()
        .map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u64 })
        .sum();
    sum == stored
}

/// The `path` record of a pax header ("<len> path=<value>\n" records).
fn pax_path(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    text.lines().find_map(|line| {
        let (_, record) = line.split_once(' ')?;
        record.strip_prefix("path=").map(str::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcribe::store::tests::{CONTRACT, temp_store};

    /// A ustar header for `name` with `size` bytes of `kind`.
    fn tar_header(name: &str, size: usize, kind: u8) -> [u8; 512] {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
        h[136..147].copy_from_slice(b"00000000000");
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        h
    }

    fn tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, body) in entries {
            let kind = if name.ends_with('/') { b'5' } else { b'0' };
            out.extend_from_slice(&tar_header(name, body.len(), kind));
            out.extend_from_slice(body);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0u8; 1024]);
        out
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x1f, 0x8b, 8, 8, 0, 0, 0, 0, 0, 255];
        out.extend_from_slice(b"bundle.tar\0");
        out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(data, 6));
        out.extend_from_slice(&[0, 0, 0, 0]); // CRC: not checked (SHA-256 is)
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out
    }

    #[test]
    fn the_plan_resumes_a_short_part_and_restarts_a_long_one() {
        assert_eq!(plan(0, 100), Plan::Fresh);
        assert_eq!(plan(40, 100), Plan::Resume(40));
        assert_eq!(plan(100, 100), Plan::Verify);
        assert_eq!(plan(140, 100), Plan::Fresh);
    }

    #[test]
    fn progress_is_counted_in_permille() {
        assert_eq!(Progress { done: 0, total: 200 }.permille(), 0);
        assert_eq!(Progress { done: 1, total: 200 }.permille(), 5);
        assert_eq!(Progress { done: 500, total: 200 }.permille(), 1000);
        assert_eq!(Progress { done: 0, total: 0 }.permille(), 1000);
    }

    #[test]
    fn the_streaming_hash_matches_the_one_shot_hash() {
        let data: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut hash = Sha256::new();
        for piece in data.chunks(65_537) {
            hash.update(piece);
        }
        assert_eq!(hash.finish(), crate::google::oauth::sha256(&data));
        // The empty string's well-known digest.
        let empty = catalog::parse_sha256(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
        assert_eq!(Some(Sha256::new().finish()), empty);
    }

    #[test]
    fn hashing_a_file_stops_when_cancelled() {
        let store = temp_store("hash");
        let path = store.root().join("f.bin");
        fs::write(&path, b"abc").unwrap();
        let go = AtomicBool::new(false);
        let abc = catalog::parse_sha256(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        );
        assert_eq!(sha256_file(&path, &go).ok(), abc);
        let stop = AtomicBool::new(true);
        assert_eq!(sha256_file(&path, &stop), Err(DownloadError::Cancelled));
    }

    #[test]
    fn a_gzip_round_trip_checks_its_trailer() {
        let data = b"transcribe".repeat(1000);
        let packed = gzip(&data);
        assert_eq!(gunzip(&packed, 1 << 20).unwrap(), data);
        assert!(gunzip(&packed, 100).is_err(), "past the cap");
        let mut wrong = packed.clone();
        let n = wrong.len();
        wrong[n - 4] ^= 1;
        assert!(gunzip(&wrong, 1 << 20).is_err());
        assert!(gunzip(b"PK\x03\x04 not gzip at all", 1 << 20).is_err());
    }

    #[test]
    fn untar_strips_the_package_folder() {
        let store = temp_store("untar");
        let dest = store.root().join("out");
        fs::create_dir_all(&dest).unwrap();
        let data = tar(&[
            ("transcribe-native-windows-x86_64-cpu-vulkan/", b""),
            ("transcribe-native-windows-x86_64-cpu-vulkan/transcribe.dll", b"MZ"),
            ("transcribe-native-windows-x86_64-cpu-vulkan/licenses/LICENSE", b"MIT"),
        ]);
        let written = untar(&data, &dest).unwrap();
        assert_eq!(
            written,
            vec![PathBuf::from("transcribe.dll"), PathBuf::from(r"licenses\LICENSE")]
        );
        assert_eq!(fs::read(dest.join("transcribe.dll")).unwrap(), b"MZ");
        assert_eq!(fs::read(dest.join("licenses").join("LICENSE")).unwrap(), b"MIT");
    }

    #[test]
    fn untar_refuses_paths_that_leave_the_folder() {
        let store = temp_store("untar-evil");
        let dest = store.root().join("out");
        fs::create_dir_all(&dest).unwrap();
        for name in ["pkg/../../evil.dll", "/abs/evil.dll", "pkg/C:/evil.dll", "pkg\\..\\evil"] {
            let data = tar(&[(name, b"x")]);
            assert!(untar(&data, &dest).is_err(), "{name}");
        }
        let two_tops = tar(&[("a/x.dll", b"x"), ("b/y.dll", b"y")]);
        assert!(untar(&two_tops, &dest).is_err());
        let mut broken = tar(&[("pkg/x.dll", b"x")]);
        broken[0] ^= 1;
        assert!(untar(&broken, &dest).is_err(), "checksum");
        assert!(!store.root().join("evil.dll").exists());
    }

    #[test]
    fn a_pax_path_names_the_next_entry() {
        let store = temp_store("untar-pax");
        let long = format!("pkg/{}/deep.txt", "d".repeat(120));
        let record = format!(" path={long}\n");
        let record = format!("{}{record}", record.len() + 3);
        let mut data = Vec::new();
        data.extend_from_slice(&tar_header("pkg/PaxHeader", record.len(), b'x'));
        data.extend_from_slice(record.as_bytes());
        data.resize(data.len().div_ceil(512) * 512, 0);
        data.extend_from_slice(&tar(&[("pkg/short-name", b"deep")]));
        let written = untar(&data, store.root()).unwrap();
        assert_eq!(written, vec![PathBuf::from(&long[4..].replace('/', "\\"))]);
    }

    #[test]
    fn the_engine_installs_through_staging_and_checks_its_contract() {
        let store = temp_store("install");
        let package = gzip(&tar(&[
            ("transcribe-native-windows-x86_64-cpu-vulkan/transcribe.dll", b"MZ"),
            ("transcribe-native-windows-x86_64-cpu-vulkan/ggml.dll", b"MZ"),
            (
                "transcribe-native-windows-x86_64-cpu-vulkan/contract.json",
                CONTRACT.as_bytes(),
            ),
        ]));
        let tarball = store.root().join("engine.tar.gz");
        fs::write(&tarball, &package).unwrap();
        install_engine(&store, &tarball).unwrap();
        assert!(store.engine_installed());
        assert!(store.engine_dir().join("ggml.dll").is_file());
        assert!(!store.engine_staging_dir().exists());

        // A package for another version is refused and leaves no staging.
        let other = gzip(&tar(&[
            ("pkg/transcribe.dll", b"MZ"),
            ("pkg/contract.json", CONTRACT.replace("0.4.1", "0.9.0").as_bytes()),
        ]));
        fs::write(&tarball, &other).unwrap();
        assert!(matches!(
            install_engine(&store, &tarball),
            Err(DownloadError::Archive(_))
        ));
        assert!(!store.engine_staging_dir().exists());
        assert!(store.engine_installed(), "the installed engine stays");
    }

    #[test]
    fn missing_lists_only_what_is_not_installed() {
        let store = temp_store("missing");
        assert_eq!(missing(&store, Item::Engine), vec![ENGINE.asset, catalog::VAD]);
        let small = catalog::model("whisper-small").unwrap();
        assert_eq!(missing(&store, Item::Model(small)), vec![small.asset]);
        fs::create_dir_all(store.models_dir()).unwrap();
        let file = fs::File::create(store.model_path(small)).unwrap();
        file.set_len(small.asset.size).unwrap();
        assert!(missing(&store, Item::Model(small)).is_empty());
    }

    #[test]
    fn errors_are_sorted_by_cause() {
        use windows::core::HRESULT;
        assert_eq!(classify(E_ABORT), DownloadError::Cancelled);
        assert!(matches!(classify(HRESULT(0x8007_0005_u32 as i32)), DownloadError::Disk(_)));
        assert_eq!(classify(HRESULT(0x8007_2EE2_u32 as i32)), DownloadError::NoNetwork);
    }

    #[test]
    fn setup_needs_the_engine_and_the_model() {
        let store = temp_store("setup");
        let turbo = catalog::default_model(true);
        assert_eq!(
            missing(&store, Item::Setup(turbo)),
            vec![ENGINE.asset, catalog::VAD, turbo.asset]
        );
        assert_eq!(
            assets_of(Item::Setup(turbo)),
            vec![ENGINE.asset, catalog::VAD, turbo.asset]
        );
        let dir = store.engine_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("contract.json"), CONTRACT).unwrap();
        fs::write(dir.join(ENGINE.dll), b"MZ").unwrap();
        fs::create_dir_all(store.models_dir()).unwrap();
        fs::File::create(store.vad_path()).unwrap().set_len(catalog::VAD.size).unwrap();
        assert_eq!(missing(&store, Item::Setup(turbo)), vec![turbo.asset]);
    }

    #[test]
    fn disk_full_is_its_own_error() {
        use windows::core::HRESULT;
        assert_eq!(classify(HRESULT(0x8007_0070_u32 as i32)), DownloadError::DiskFull);
        assert_eq!(classify(HRESULT(0x8007_0027_u32 as i32)), DownloadError::DiskFull);
        assert_eq!(
            DownloadError::from(io::Error::from_raw_os_error(112)),
            DownloadError::DiskFull
        );
        assert_eq!(
            DownloadError::from(io::Error::from_raw_os_error(39)),
            DownloadError::DiskFull
        );
        assert!(matches!(
            DownloadError::from(io::Error::from_raw_os_error(5)),
            DownloadError::Disk(_)
        ));
    }
}
