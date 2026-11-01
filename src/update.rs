//! Self-update: ask GitHub for the latest release, download `vale.exe` next to
//! the running exe, check it against the release's `vale.exe.sha256`, swap it
//! in and start it. The pure parts (versions, the checksum file, the release
//! JSON, the offer rule, the swap on a folder) are tested; the network parts
//! run on worker threads and answer with `Event::Update`.
//!
//! Windows lets a running exe be renamed (not overwritten or deleted), so the
//! swap is: `vale.exe` -> `vale.exe.old`, the download -> `vale.exe`, start
//! the new one with `--updated <old version>`. The new process waits for the
//! old one's single-instance mutex, then deletes `vale.exe.old`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::Value;

use crate::google::http::Http;
use crate::google::oauth::sha256;
use crate::i18n::{Key, Lang, text};
use crate::model::Event;

/// The running version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// The stable (not draft, not pre-release) latest release of the repository.
const LATEST_API: &str = "https://api.github.com/repos/hartono0297/vale/releases/latest";
/// What the failure notice's chip opens: the release page, as a last resort.
pub const RELEASES_LATEST_URL: &str =
    "https://github.com/hartono0297/vale/releases/latest";
/// Asset names the release workflow publishes.
const EXE_ASSET: &str = "vale.exe";
const SHA_ASSET: &str = "vale.exe.sha256";
/// The release JSON stays under this.
const JSON_CAP: usize = 1 << 20;
/// The exe download stays under this (the release exe is a few MB).
const EXE_CAP: u64 = 64 << 20;
/// The `.sha256` file stays under this.
const SHA_CAP: usize = 1024;
/// A check gives up after this long without an answer.
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// A download gives up after this long without a single new byte.
const DOWNLOAD_STALL: Duration = Duration::from_secs(30);

/// `(major, minor, patch)` of "3.10.0" or "v3.10.0"; anything else (two
/// parts, four, a pre-release suffix, letters) is `None`.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim();
    let s = s
        .strip_prefix('v')
        .or_else(|| s.strip_prefix('V'))
        .unwrap_or(s);
    let mut parts = s.split('.');
    let mut next = || {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse::<u32>().ok()
    };
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// `tag` is a newer version than `current`; false when either is malformed.
pub fn is_newer(tag: &str, current: &str) -> bool {
    match (parse_version(tag), parse_version(current)) {
        (Some(tag), Some(current)) => tag > current,
        _ => false,
    }
}

/// "X.Y.Z" for a tag ("v3.11.0" -> "3.11.0"); `None` when malformed.
pub fn plain_version(tag: &str) -> Option<String> {
    let (a, b, c) = parse_version(tag)?;
    Some(format!("{a}.{b}.{c}"))
}

/// The digest in a `.sha256` file ("<hex>  vale.exe", no newline): its first
/// word, 64 hex characters of either case. Anything else is `None`.
pub fn parse_sha256_file(text: &str) -> Option<[u8; 32]> {
    let word = text.split_whitespace().next()?;
    if word.len() != 64 || !word.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (byte, pair) in out.iter_mut().zip(word.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateError {
    /// The request failed or timed out.
    NoNetwork,
    /// The exe's folder (or the file) cannot be written, renamed or started.
    NotWritable,
    ChecksumMismatch,
    /// The release has no `vale.exe` or `vale.exe.sha256`.
    AssetMissing,
    /// GitHub answered with an error status, or with something unreadable.
    BadRelease,
}

impl UpdateError {
    /// The short reason of "Update gagal: <reason>".
    pub fn reason(self, lang: Lang) -> &'static str {
        text(
            lang,
            match self {
                UpdateError::NoNetwork => Key::UpdateErrNoNetwork,
                UpdateError::NotWritable => Key::UpdateErrNotWritable,
                UpdateError::ChecksumMismatch => Key::UpdateErrChecksum,
                UpdateError::AssetMissing => Key::UpdateErrAsset,
                UpdateError::BadRelease => Key::UpdateErrBadRelease,
            },
        )
    }
}

/// What `releases/latest` says about the newest release.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub html_url: String,
    pub exe_url: String,
    pub sha_url: String,
}

impl Release {
    /// "3.11.0" (the tag without its `v`); the tag itself when malformed.
    pub fn version(&self) -> String {
        plain_version(&self.tag).unwrap_or_else(|| self.tag.clone())
    }
}

/// The release in the `releases/latest` JSON. The assets are found by their
/// exact names; a download URL must be https.
pub fn pick_release(json: &str) -> Result<Release, UpdateError> {
    let value: Value = serde_json::from_str(json).map_err(|_| UpdateError::BadRelease)?;
    let tag = value
        .get("tag_name")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .ok_or(UpdateError::BadRelease)?;
    let html_url = value
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or(RELEASES_LATEST_URL);
    let assets = value.get("assets").and_then(Value::as_array);
    let asset = |name: &str| -> Result<String, UpdateError> {
        let url = assets
            .into_iter()
            .flatten()
            .find(|a| a.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|a| a.get("browser_download_url"))
            .and_then(Value::as_str)
            .ok_or(UpdateError::AssetMissing)?;
        if url.starts_with("https://") {
            Ok(url.to_owned())
        } else {
            Err(UpdateError::BadRelease)
        }
    };
    Ok(Release {
        tag: tag.trim().to_owned(),
        html_url: html_url.to_owned(),
        exe_url: asset(EXE_ASSET)?,
        sha_url: asset(SHA_ASSET)?,
    })
}

/// Whether a known `latest` is shown as an offer: newer than `current`, and
/// (unless the user asked) not the version an earlier automatic check offered.
pub fn should_offer(latest: &str, current: &str, last_offered: &str, manual: bool) -> bool {
    is_newer(latest, current)
        && (manual || parse_version(latest).is_none_or(|l| Some(l) != parse_version(last_offered)))
}

/// What a worker tells the island (`Event::Update`); the reducer ignores it,
/// `App` handles it.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateEvent {
    /// A check finished: the latest release GitHub lists, or why not.
    Checked {
        manual: bool,
        result: Result<Release, UpdateError>,
    },
    /// The download is at this whole percent.
    Progress(u8),
    /// The download is complete and verified at `file`, next to the exe.
    Downloaded { release: Release, file: PathBuf },
    /// The download or its verification failed; the temp file is gone.
    Failed {
        release: Release,
        error: UpdateError,
    },
}

fn user_agent() -> String {
    format!("Vale/{CURRENT}")
}

/// The latest release, asked of GitHub. Blocks: worker threads only.
pub fn fetch_latest() -> Result<Release, UpdateError> {
    let http = Http::new()
        .map_err(|_| UpdateError::NoNetwork)?
        .with_timeout(CHECK_TIMEOUT);
    let agent = user_agent();
    let (status, body) = http
        .get_with_headers_capped(
            LATEST_API,
            &[
                ("User-Agent", agent.as_str()),
                ("Accept", "application/vnd.github+json"),
            ],
            JSON_CAP,
        )
        .map_err(|e| classify(e.code().0))?;
    // A rate limit (403), no release yet (404) and the like.
    if !(200..300).contains(&status) {
        return Err(UpdateError::BadRelease);
    }
    pick_release(&body)
}

/// Runs `job` on a worker thread with COM up (the WinRT client needs it), then
/// wakes the island.
fn spawn_worker(name: &str, job: impl FnOnce() + Send + 'static) {
    let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
        unsafe {
            use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        job();
    });
    if let Err(error) = spawned {
        eprintln!("vale: cannot start the {name} thread: {error}");
    }
}

/// Starts a check on a worker; its answer is `UpdateEvent::Checked`.
pub fn check(manual: bool, tx: Sender<Event>, wake: impl Fn() + Send + 'static) {
    spawn_worker("update-check", move || {
        let result = fetch_latest();
        let _ = tx.send(Event::Update(UpdateEvent::Checked { manual, result }));
        wake();
    });
}

/// Starts the download and verification of `release` on a worker: progress
/// events, then `Downloaded` or `Failed`.
pub fn install(release: Release, tx: Sender<Event>, wake: impl Fn() + Send + Copy + 'static) {
    spawn_worker("update-download", move || {
        let send = |event| {
            let _ = tx.send(Event::Update(event));
            wake();
        };
        let outcome = match std::env::current_exe() {
            Ok(exe) => download_verified(&release, &exe, |pct| send(UpdateEvent::Progress(pct))),
            Err(_) => Err(UpdateError::NotWritable),
        };
        send(match outcome {
            Ok(file) => UpdateEvent::Downloaded { release, file },
            Err(error) => UpdateEvent::Failed { release, error },
        });
    });
}

/// The error for a failed transfer, by its HRESULT: a Win32 disk or access
/// error is the folder, a plain failure (a body past the cap) is the release,
/// everything else (WinHTTP, timeouts) the network.
fn classify(hresult: i32) -> UpdateError {
    const E_FAIL: i32 = 0x8000_4005_u32 as i32;
    let win32 = |code: u32| (0x8007_0000_u32 | code) as i32;
    // Access denied, write fault, handle disk full, disk full.
    let folder = [5, 29, 39, 112].map(win32);
    if hresult == E_FAIL {
        UpdateError::BadRelease
    } else if folder.contains(&hresult) {
        UpdateError::NotWritable
    } else {
        UpdateError::NoNetwork
    }
}

/// `<exe dir>\vale.exe.download`, the same volume as the exe.
fn download_path(exe: &Path) -> PathBuf {
    with_suffix(exe, "download")
}

/// `<exe dir>\vale.exe.old`, where the running exe goes during the swap.
fn old_path(exe: &Path) -> PathBuf {
    with_suffix(exe, "old")
}

/// The exe's path with `.suffix` appended to its file name.
fn with_suffix(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    exe.with_file_name(name)
}

/// Downloads the release's exe next to `exe` and checks it against the
/// release's checksum; the file's path, or why not (the file is deleted then).
/// `progress` gets each new whole percent.
fn download_verified(
    release: &Release,
    exe: &Path,
    mut progress: impl FnMut(u8),
) -> Result<PathBuf, UpdateError> {
    let target = download_path(exe);
    // A folder that cannot hold the file is found out before any traffic.
    std::fs::File::create(&target).map_err(|_| UpdateError::NotWritable)?;
    let result = fetch_and_check(release, &target, &mut progress);
    if result.is_err() {
        let _ = std::fs::remove_file(&target);
    }
    result.map(|()| target)
}

fn fetch_and_check(
    release: &Release,
    target: &Path,
    progress: &mut impl FnMut(u8),
) -> Result<(), UpdateError> {
    let http = Http::new()
        .map_err(|_| UpdateError::NoNetwork)?
        .with_timeout(DOWNLOAD_STALL);
    let agent = user_agent();
    let headers = [("User-Agent", agent.as_str())];
    // The small checksum first: a release without a readable one fails fast.
    let (status, body) = http
        .get_with_headers_capped(&release.sha_url, &headers, SHA_CAP)
        .map_err(|e| classify(e.code().0))?;
    check_status(status)?;
    let expected = parse_sha256_file(&body).ok_or(UpdateError::BadRelease)?;
    let mut last = None;
    let status = http
        .download_to(&release.exe_url, &headers, target, EXE_CAP, |done, total| {
            // Only a changed whole percent is passed on.
            if let Some(pct) = percent(done, total)
                && last != Some(pct)
            {
                last = Some(pct);
                progress(pct);
            }
        })
        .map_err(|e| classify(e.code().0))?;
    check_status(status)?;
    let bytes = std::fs::read(target).map_err(|_| UpdateError::NotWritable)?;
    if sha256(&bytes) == expected {
        Ok(())
    } else {
        Err(UpdateError::ChecksumMismatch)
    }
}

/// A missing asset (404) or any other error status.
fn check_status(status: u16) -> Result<(), UpdateError> {
    match status {
        200..=299 => Ok(()),
        404 => Err(UpdateError::AssetMissing),
        _ => Err(UpdateError::BadRelease),
    }
}

/// Whole percent done (0 to 100); `None` when the total is unknown or 0.
fn percent(done: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|&t| t > 0)?;
    Some((done.min(total) * 100 / total) as u8)
}

/// Renames the running `exe` to `exe.old` (replacing an older one), moves
/// `new_file` to `exe`, and runs `launch(exe)`. If the move or the launch
/// fails, `.old` goes back to `exe`, the new file is deleted and the error is
/// returned: the old exe is as it was.
pub fn swap_and_launch(
    exe: &Path,
    new_file: &Path,
    launch: impl FnOnce(&Path) -> io::Result<()>,
) -> Result<(), UpdateError> {
    let old = old_path(exe);
    let _ = std::fs::remove_file(&old);
    if std::fs::rename(exe, &old).is_err() {
        let _ = std::fs::remove_file(new_file);
        return Err(UpdateError::NotWritable);
    }
    let rollback = || {
        let _ = std::fs::remove_file(new_file);
        let _ = std::fs::rename(&old, exe);
    };
    // An antivirus scan may hold the fresh file for a moment.
    let mut moved = false;
    for attempt in 0..3 {
        if std::fs::rename(new_file, exe).is_ok() {
            moved = true;
            break;
        }
        if attempt < 2 {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    if !moved {
        rollback();
        return Err(UpdateError::NotWritable);
    }
    if launch(exe).is_err() {
        rollback();
        return Err(UpdateError::NotWritable);
    }
    Ok(())
}

/// Starts the new exe as `vale --updated <old version>`.
pub fn launch_updated(exe: &Path, old_version: &str) -> io::Result<()> {
    std::process::Command::new(exe)
        .arg("--updated")
        .arg(old_version)
        .spawn()
        .map(|_| ())
}

/// Best-effort removal of `vale.exe.old` and a stale `vale.exe.download` in
/// `dir`. The old process may still be ending, so a file that stays is tried
/// again for a few seconds. Runs on a background thread.
pub fn cleanup_leftovers(dir: &Path) {
    cleanup_with(dir, 10, Duration::from_millis(500));
}

fn cleanup_with(dir: &Path, tries: u32, delay: Duration) {
    let exe = dir.join(EXE_ASSET);
    let leftovers = [old_path(&exe), download_path(&exe)];
    for attempt in 0..tries {
        for file in &leftovers {
            let _ = std::fs::remove_file(file);
        }
        if leftovers.iter().all(|f| !f.exists()) {
            return;
        }
        if attempt + 1 < tries {
            std::thread::sleep(delay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn versions_parse_with_or_without_the_v() {
        assert_eq!(parse_version("3.10.0"), Some((3, 10, 0)));
        assert_eq!(parse_version("v3.10.0"), Some((3, 10, 0)));
        assert_eq!(parse_version("V3.10.0"), Some((3, 10, 0)));
        assert_eq!(parse_version("  v1.2.3 \n"), Some((1, 2, 3)));
    }

    #[test]
    fn malformed_versions_are_none() {
        for bad in [
            "",
            "v",
            "v3.10",
            "3.10.0-beta",
            "v3.10.0.1",
            "abc",
            "vv3.10.0",
            "3..0",
            "3.10.x",
            "+3.10.0",
            "3.-1.0",
        ] {
            assert_eq!(parse_version(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn newer_means_strictly_newer_and_well_formed() {
        assert!(is_newer("v3.11.0", "3.10.0"));
        assert!(is_newer("3.10.1", "3.10.0"));
        assert!(is_newer("4.0.0", "3.99.99"));
        // Numbers, not text: 3.10 is above 3.9.
        assert!(is_newer("3.10.0", "3.9.0"));
        assert!(!is_newer("v3.10.0", "3.10.0"));
        assert!(!is_newer("3.9.0", "3.10.0"));
        assert!(!is_newer("3.11.0-beta", "3.10.0"));
        assert!(!is_newer("3.11.0", "garbage"));
        assert!(!is_newer("", ""));
    }

    #[test]
    fn plain_version_drops_the_v() {
        assert_eq!(plain_version("v3.11.0").as_deref(), Some("3.11.0"));
        assert_eq!(plain_version("nightly"), None);
    }

    #[test]
    fn the_sha_file_is_a_64_digit_hex_word() {
        let text = format!("{HEX}  vale.exe");
        let digest = parse_sha256_file(&text).unwrap();
        assert_eq!(digest[0], 0x01);
        assert_eq!(digest[1], 0x23);
        assert_eq!(digest[31], 0xef);
        // Upper case reads the same; a trailing newline is no problem.
        assert_eq!(
            parse_sha256_file(&format!("{}\n", HEX.to_uppercase())),
            Some(digest)
        );
        assert_eq!(parse_sha256_file(HEX), Some(digest));
    }

    #[test]
    fn a_bad_sha_file_is_none() {
        assert_eq!(parse_sha256_file(""), None);
        assert_eq!(parse_sha256_file("   \n"), None);
        assert_eq!(parse_sha256_file(&HEX[..63]), None);
        assert_eq!(parse_sha256_file(&format!("{HEX}0")), None);
        assert_eq!(parse_sha256_file(&format!("{}g", &HEX[..63])), None);
        assert_eq!(parse_sha256_file("vale.exe"), None);
    }

    fn release_json(assets: &str) -> String {
        format!(
            r#"{{"tag_name":"v3.11.0","html_url":"https://github.com/x/y/releases/tag/v3.11.0",
            "draft":false,"assets":[{assets}]}}"#
        )
    }

    const EXE_ASSET_JSON: &str = r#"{"name":"vale.exe","browser_download_url":"https://github.com/x/y/releases/download/v3.11.0/vale.exe"}"#;
    const SHA_ASSET_JSON: &str = r#"{"name":"vale.exe.sha256","browser_download_url":"https://github.com/x/y/releases/download/v3.11.0/vale.exe.sha256"}"#;
    const ZIP_ASSET_JSON: &str = r#"{"name":"vale-v3.11.0.zip","browser_download_url":"https://github.com/x/y/releases/download/v3.11.0/vale-v3.11.0.zip"}"#;

    #[test]
    fn the_release_names_its_exe_and_checksum() {
        let json = release_json(&format!(
            "{ZIP_ASSET_JSON},{SHA_ASSET_JSON},{EXE_ASSET_JSON}"
        ));
        let release = pick_release(&json).unwrap();
        assert_eq!(release.tag, "v3.11.0");
        assert_eq!(release.version(), "3.11.0");
        assert_eq!(
            release.html_url,
            "https://github.com/x/y/releases/tag/v3.11.0"
        );
        assert_eq!(
            release.exe_url,
            "https://github.com/x/y/releases/download/v3.11.0/vale.exe"
        );
        assert_eq!(
            release.sha_url,
            "https://github.com/x/y/releases/download/v3.11.0/vale.exe.sha256"
        );
    }

    #[test]
    fn a_missing_asset_is_asset_missing() {
        let no_exe = release_json(&format!("{ZIP_ASSET_JSON},{SHA_ASSET_JSON}"));
        assert_eq!(pick_release(&no_exe), Err(UpdateError::AssetMissing));
        let no_sha = release_json(&format!("{ZIP_ASSET_JSON},{EXE_ASSET_JSON}"));
        assert_eq!(pick_release(&no_sha), Err(UpdateError::AssetMissing));
        let none = r#"{"tag_name":"v3.11.0"}"#;
        assert_eq!(pick_release(none), Err(UpdateError::AssetMissing));
    }

    #[test]
    fn an_unreadable_release_is_bad_release() {
        assert_eq!(pick_release("not json"), Err(UpdateError::BadRelease));
        assert_eq!(pick_release(""), Err(UpdateError::BadRelease));
        assert_eq!(
            pick_release(r#"{"message":"Not Found"}"#),
            Err(UpdateError::BadRelease)
        );
        // A download over plain http is not trusted.
        let http = release_json(&format!(
            r#"{{"name":"vale.exe","browser_download_url":"http://x/vale.exe"}},{SHA_ASSET_JSON}"#
        ));
        assert_eq!(pick_release(&http), Err(UpdateError::BadRelease));
    }

    #[test]
    fn a_version_is_offered_once_by_the_automatic_check() {
        // New, never offered.
        assert!(should_offer("v3.11.0", "3.10.0", "", false));
        // The same version again: not repeated.
        assert!(!should_offer("v3.11.0", "3.10.0", "3.11.0", false));
        // A newer one after that is offered.
        assert!(should_offer("v3.12.0", "3.10.0", "3.11.0", false));
        // A manual check always offers a newer version.
        assert!(should_offer("v3.11.0", "3.10.0", "3.11.0", true));
        // Never an older or an equal one, whoever asks.
        for manual in [false, true] {
            assert!(!should_offer("v3.10.0", "3.10.0", "", manual));
            assert!(!should_offer("v3.9.0", "3.10.0", "", manual));
            assert!(!should_offer("junk", "3.10.0", "", manual));
        }
    }

    #[test]
    fn reasons_exist_in_both_languages() {
        for error in [
            UpdateError::NoNetwork,
            UpdateError::NotWritable,
            UpdateError::ChecksumMismatch,
            UpdateError::AssetMissing,
            UpdateError::BadRelease,
        ] {
            assert!(!error.reason(Lang::Id).is_empty());
            assert!(!error.reason(Lang::En).is_empty());
            assert_ne!(error.reason(Lang::Id), error.reason(Lang::En));
        }
    }

    #[test]
    fn transfer_errors_are_told_apart() {
        assert_eq!(classify(0x8007_0070_u32 as i32), UpdateError::NotWritable);
        assert_eq!(classify(0x8007_0005_u32 as i32), UpdateError::NotWritable);
        // ERROR_WINHTTP_CANNOT_CONNECT and ERROR_TIMEOUT.
        assert_eq!(classify(0x8007_2efd_u32 as i32), UpdateError::NoNetwork);
        assert_eq!(classify(0x8007_05b4_u32 as i32), UpdateError::NoNetwork);
        assert_eq!(classify(0x8000_4005_u32 as i32), UpdateError::BadRelease);
    }

    #[test]
    fn percent_is_whole_and_capped() {
        assert_eq!(percent(0, Some(200)), Some(0));
        assert_eq!(percent(50, Some(200)), Some(25));
        assert_eq!(percent(999, Some(200)), Some(100));
        assert_eq!(percent(5, None), None);
        assert_eq!(percent(5, Some(0)), None);
    }

    /// A folder of its own under the temp dir, gone when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "vale-update-test-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_swap_puts_the_new_exe_in_place_and_keeps_the_old_as_old() {
        let dir = TempDir::new("swap-ok");
        let exe = dir.0.join("vale.exe");
        let new = dir.0.join("vale.exe.download");
        fs::write(&exe, "old build").unwrap();
        fs::write(&new, "new build").unwrap();
        let mut launched = None;
        swap_and_launch(&exe, &new, |path| {
            launched = Some(path.to_path_buf());
            Ok(())
        })
        .unwrap();
        assert_eq!(launched, Some(exe.clone()));
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new build");
        assert_eq!(
            fs::read_to_string(dir.0.join("vale.exe.old")).unwrap(),
            "old build"
        );
        assert_eq!(files(&dir.0), ["vale.exe", "vale.exe.old"]);
    }

    #[test]
    fn a_failed_launch_rolls_back() {
        let dir = TempDir::new("swap-launch");
        let exe = dir.0.join("vale.exe");
        let new = dir.0.join("vale.exe.download");
        fs::write(&exe, "old build").unwrap();
        fs::write(&new, "new build").unwrap();
        let result = swap_and_launch(&exe, &new, |_| {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "blocked"))
        });
        assert_eq!(result, Err(UpdateError::NotWritable));
        assert_eq!(fs::read_to_string(&exe).unwrap(), "old build");
        assert_eq!(files(&dir.0), ["vale.exe"]);
    }

    #[test]
    fn an_old_file_already_there_is_replaced() {
        let dir = TempDir::new("swap-old");
        let exe = dir.0.join("vale.exe");
        let new = dir.0.join("vale.exe.download");
        fs::write(&exe, "old build").unwrap();
        fs::write(&new, "new build").unwrap();
        fs::write(dir.0.join("vale.exe.old"), "older still").unwrap();
        swap_and_launch(&exe, &new, |_| Ok(())).unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new build");
        assert_eq!(
            fs::read_to_string(dir.0.join("vale.exe.old")).unwrap(),
            "old build"
        );
    }

    #[test]
    fn a_missing_download_rolls_back_too() {
        let dir = TempDir::new("swap-missing");
        let exe = dir.0.join("vale.exe");
        let new = dir.0.join("vale.exe.download");
        fs::write(&exe, "old build").unwrap();
        // The second rename keeps failing: the retries run out, the old exe returns.
        let result = swap_and_launch(&exe, &new, |_| Ok(()));
        assert_eq!(result, Err(UpdateError::NotWritable));
        assert_eq!(fs::read_to_string(&exe).unwrap(), "old build");
        assert_eq!(files(&dir.0), ["vale.exe"]);
    }

    #[test]
    fn cleanup_removes_the_leftovers_and_nothing_else() {
        let dir = TempDir::new("cleanup");
        for name in ["vale.exe", "vale.exe.old", "vale.exe.download", "config.json"] {
            fs::write(dir.0.join(name), name).unwrap();
        }
        cleanup_with(&dir.0, 3, Duration::from_millis(1));
        assert_eq!(files(&dir.0), ["config.json", "vale.exe"]);
        // Nothing to remove is fine.
        cleanup_with(&dir.0, 3, Duration::from_millis(1));
        assert_eq!(files(&dir.0), ["config.json", "vale.exe"]);
    }

    #[test]
    fn the_temp_and_old_names_sit_beside_the_exe() {
        let exe = Path::new(r"C:\Apps\Vale\vale.exe");
        assert_eq!(download_path(exe), Path::new(r"C:\Apps\Vale\vale.exe.download"));
        assert_eq!(old_path(exe), Path::new(r"C:\Apps\Vale\vale.exe.old"));
    }
}
