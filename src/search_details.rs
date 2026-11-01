//! What a search row learns after it is shown: its icon, a folder's item
//! count and an app's install size. All of it is read on one parked worker
//! thread (`vale-search-details`, COM single-threaded apartment), never on
//! the UI thread: rows show at once and fill in as answers arrive.
//!
//! Power: the worker blocks on its channel, so it costs nothing while idle;
//! it starts the first time the search opens. A request that is still
//! waiting when a newer one arrives is dropped (the worker always jumps to
//! the newest), and an answer for an older request is ignored.
//!
//! The pure parts (`wanted_details`, `Details`, `install_size`,
//! `premultiply`) are tested; the shell and registry reads are not, since
//! tests never touch the owner's shell, files or registry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
    STGM_READ,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    REG_SAM_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, IShellLinkW, SHCreateItemFromParsingName, SIIGBF_ICONONLY, ShellLink,
};
use windows::core::{HSTRING, Interface, PCWSTR, PWSTR, w};

use crate::clipboard::image::Thumb;
use crate::i18n::{self, Lang};
use crate::model::Event;
use crate::search::{Kind, SearchResult};
use crate::search_files;

/// Details kept at most; the cache is emptied when it grows past this.
pub const DETAILS_MAX: usize = 256;
/// The icon is asked for at this many pixels square (shown at 16 dip).
const ICON_PX: i32 = 32;

/// What a row's path is, so the worker knows what to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// A Start Menu shortcut: icon of its target, install size.
    App,
    File,
    /// Icon and item count.
    Folder,
}

/// One row's path to look at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub kind: ItemKind,
}

/// What was found for a path; a field stays `None` when it is not known (or
/// does not apply), and the row simply shows without it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detail {
    /// The shell's icon, premultiplied BGRA.
    pub icon: Option<Thumb>,
    /// Items directly inside a folder, up to `COUNT_CAP + 1`.
    pub count: Option<usize>,
    /// An installed app's size in bytes.
    pub install: Option<u64>,
}

/// A batch of paths to look at; `generation` tells answers apart from those
/// of an older batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub generation: u64,
    pub items: Vec<Item>,
}

/// The shown rows whose details are not known yet, in row order.
pub fn wanted_details(results: &[SearchResult], have: &HashMap<PathBuf, Detail>) -> Vec<Item> {
    results
        .iter()
        .filter_map(|r| {
            let path = r.path.as_ref().filter(|p| !have.contains_key(*p))?;
            let kind = match r.kind {
                Kind::App => ItemKind::App,
                Kind::File => ItemKind::File,
                Kind::Folder => ItemKind::Folder,
                Kind::Setting | Kind::Calc | Kind::Web => return None,
            };
            Some(Item {
                path: path.clone(),
                kind,
            })
        })
        .collect()
}

/// What a row says after its subtitle once its details are known: a
/// folder's item count, an installed app's size. `None` when there is
/// nothing to add.
pub fn row_note(kind: Kind, detail: &Detail, lang: Lang) -> Option<String> {
    match kind {
        Kind::Folder => detail.count.map(|n| i18n::search_folder_items(lang, n)),
        Kind::App => detail
            .install
            .map(|bytes| i18n::search_file_size(lang, bytes)),
        Kind::File | Kind::Setting | Kind::Calc | Kind::Web => None,
    }
}

/// The details known so far, by path, and the generation of the newest
/// request.
#[derive(Default)]
pub struct Details {
    known: HashMap<PathBuf, Detail>,
    generation: u64,
}

impl Details {
    pub fn known(&self) -> &HashMap<PathBuf, Detail> {
        &self.known
    }

    /// The request for the rows in `results` that still lack details, or
    /// `None` when there is nothing to ask. Each request starts a new
    /// generation. A cache grown past `DETAILS_MAX` is emptied first, so the
    /// rows on screen are asked for again.
    pub fn request(&mut self, results: &[SearchResult]) -> Option<Request> {
        if self.known.len() > DETAILS_MAX {
            self.known.clear();
        }
        let items = wanted_details(results, &self.known);
        if items.is_empty() {
            return None;
        }
        self.generation += 1;
        Some(Request {
            generation: self.generation,
            items,
        })
    }

    /// An answer from the worker: kept unless it belongs to an older
    /// generation. Returns whether it was kept (the search then redraws).
    pub fn accept(&mut self, generation: u64, key: PathBuf, detail: Detail) -> bool {
        if generation < self.generation {
            return false;
        }
        self.known.insert(key, detail);
        true
    }
}

/// One entry of the Uninstall registry keys, the three fields that matter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UninstallEntry {
    pub install_location: Option<String>,
    pub display_icon: Option<String>,
    /// `EstimatedSize`, in KB.
    pub size_kb: Option<u64>,
}

/// An app's install size in bytes: the first entry (with a size) whose
/// `InstallLocation` holds `target`, or whose `DisplayIcon` is `target`
/// itself. Paths compare without case and without a trailing backslash; a
/// `DisplayIcon` may be quoted and end in `,0`. No match, or no size
/// (`EstimatedSize` 0 is "unknown"): `None`.
pub fn install_size(entries: &[UninstallEntry], target: &str) -> Option<u64> {
    let target = normal(target);
    if target.is_empty() {
        return None;
    }
    // The entry whose icon is the target wins; else the most specific
    // (longest) install location holding it.
    let mut best: Option<(usize, u64)> = None;
    for entry in entries {
        let Some(kb) = entry.size_kb.filter(|kb| *kb > 0) else {
            continue;
        };
        let by_icon = entry
            .display_icon
            .as_deref()
            .map(icon_file)
            .is_some_and(|icon| !icon.is_empty() && normal(&icon) == target);
        let by_location = entry.install_location.as_deref().map(normal).filter(|loc| {
            !too_broad(loc)
                && target
                    .strip_prefix(loc.as_str())
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('\\'))
        });
        let rank = match (by_icon, by_location) {
            (true, _) => usize::MAX,
            (false, Some(loc)) => loc.len(),
            (false, None) => continue,
        };
        if best.is_none_or(|(r, _)| rank > r) {
            best = Some((rank, kb * 1024));
        }
    }
    best.map(|(_, bytes)| bytes)
}

/// `path` for comparing: no quotes or spaces around it, lower case, no
/// trailing backslash.
fn normal(path: &str) -> String {
    path.trim()
        .trim_matches('"')
        .to_lowercase()
        .trim_end_matches('\\')
        .to_owned()
}

/// The file in a `DisplayIcon` value: `"C:\a\b.exe",0` or `C:\a\b.exe,1`
/// (an icon index after a comma) or the bare path.
fn icon_file(value: &str) -> String {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('"') {
        return rest.split('"').next().unwrap_or_default().to_owned();
    }
    match value.rsplit_once(',') {
        Some((file, index))
            if index
                .trim()
                .trim_start_matches('-')
                .bytes()
                .all(|b| b.is_ascii_digit()) =>
        {
            file.to_owned()
        }
        _ => value.to_owned(),
    }
}

/// A location that names no single app: a drive root, or the folders that
/// hold all of them.
fn too_broad(location: &str) -> bool {
    let parts: Vec<&str> = location.split('\\').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [] | [_] => true,
        [_, name] => [
            "program files",
            "program files (x86)",
            "windows",
            "users",
            "programdata",
        ]
        .contains(name),
        _ => false,
    }
}

/// BGRA pixels with straight alpha made premultiplied, as Direct2D wants
/// them. A picture whose alpha is zero everywhere (a bitmap without an alpha
/// channel) is made opaque instead of invisible.
pub fn premultiply(bgra: &mut [u8]) {
    // Whole pixels only: a ragged tail is left alone.
    let pixels = bgra.as_chunks_mut::<4>().0;
    if pixels.iter().all(|px| px[3] == 0) {
        for px in pixels {
            px[3] = 255;
        }
        return;
    }
    for px in pixels {
        let a = u32::from(px[3]);
        for c in &mut px[..3] {
            *c = ((u32::from(*c) * a + 127) / 255) as u8;
        }
    }
}

/// Starts the worker; the sender takes requests. `None` when the thread
/// cannot be made (the rows then keep their glyphs). Answers go to `events`
/// as `Event::SearchDetail`, and `wake` tells the window to read them.
pub fn spawn(events: Sender<Event>, wake: impl Fn() + Send + 'static) -> Option<Sender<Request>> {
    let (tx, rx) = channel::<Request>();
    std::thread::Builder::new()
        .name("vale-search-details".into())
        .spawn(move || run(rx, events, wake))
        .ok()?;
    Some(tx)
}

/// The worker: waits for a request, jumps to the newest one, answers each
/// item as soon as it is read, and looks again for a newer request between
/// items.
fn run(rx: Receiver<Request>, events: Sender<Event>, wake: impl Fn()) {
    // The shell's icon and link objects want a single-threaded apartment.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    // The registry is read once, the first time an app is asked for.
    let mut uninstall: Option<Vec<UninstallEntry>> = None;
    let mut waiting: Option<Request> = None;
    loop {
        let Some(mut request) = waiting.take().or_else(|| rx.recv().ok()) else {
            return;
        };
        while let Ok(newer) = rx.try_recv() {
            request = newer;
        }
        for item in request.items {
            let detail = look(&item, &mut uninstall);
            let answer = Event::SearchDetail {
                generation: request.generation,
                key: item.path,
                detail,
            };
            if events.send(answer).is_err() {
                return;
            }
            wake();
            // A newer request makes the rest of this one stale.
            if let Ok(mut newer) = rx.try_recv() {
                while let Ok(newest) = rx.try_recv() {
                    newer = newest;
                }
                waiting = Some(newer);
                break;
            }
        }
    }
}

/// What the shell and the file system say about one item.
fn look(item: &Item, uninstall: &mut Option<Vec<UninstallEntry>>) -> Detail {
    let mut detail = Detail::default();
    match item.kind {
        ItemKind::App => {
            // The icon of what the shortcut runs (the shortcut's own would
            // carry the arrow); the shortcut itself when that is unknown.
            let target = link_target(&item.path).filter(|t| t.exists());
            detail.icon = shell_icon(target.as_deref().unwrap_or(&item.path));
            if let Some(target) = target.as_deref().and_then(Path::to_str) {
                let entries = uninstall.get_or_insert_with(read_uninstall);
                detail.install = install_size(entries, target);
            }
        }
        ItemKind::File => detail.icon = shell_icon(&item.path),
        ItemKind::Folder => {
            detail.icon = shell_icon(&item.path);
            detail.count = search_files::folder_count(&item.path);
        }
    }
    detail
}

/// The shell's icon for `path` at `ICON_PX`, never a content thumbnail (so a
/// cloud file is not downloaded to draw it).
pub(crate) fn shell_icon(path: &Path) -> Option<Thumb> {
    unsafe {
        let wide = HSTRING::from(path.as_os_str());
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&wide, None::<&windows::Win32::System::Com::IBindCtx>)
                .ok()?;
        let bitmap = factory
            .GetImage(
                SIZE {
                    cx: ICON_PX,
                    cy: ICON_PX,
                },
                SIIGBF_ICONONLY,
            )
            .ok()?;
        let thumb = bitmap_pixels(bitmap);
        let _ = DeleteObject(HGDIOBJ::from(bitmap));
        thumb
    }
}

/// The pixels of a 32-bit bitmap, top-down, premultiplied.
unsafe fn bitmap_pixels(bitmap: HBITMAP) -> Option<Thumb> {
    unsafe {
        let mut info = BITMAP::default();
        let read = GetObjectW(
            HGDIOBJ::from(bitmap),
            size_of::<BITMAP>() as i32,
            Some((&raw mut info).cast()),
        );
        let (w, h) = (info.bmWidth, info.bmHeight);
        if read == 0 || w <= 0 || h <= 0 || w > 256 || h > 256 {
            return None;
        }
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // Negative: top-down rows, as Direct2D reads them.
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            h as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        );
        ReleaseDC(None, dc);
        if lines == 0 {
            return None;
        }
        premultiply(&mut bgra);
        Some(Thumb {
            w: w as u32,
            h: h as u32,
            bgra,
        })
    }
}

/// Where a Start Menu shortcut points, read from the file (the link is not
/// resolved: that may search the disk or the network).
fn link_target(lnk: &Path) -> Option<PathBuf> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        (len > 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len])))
    }
}

/// The Uninstall keys of this machine (64-bit and 32-bit views) and this
/// user: the location, icon and size of each entry. Read only.
fn read_uninstall() -> Vec<UninstallEntry> {
    let sub = w!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall");
    let mut out = Vec::new();
    for (root, view) in [
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
        (HKEY_CURRENT_USER, REG_SAM_FLAGS(0)),
    ] {
        unsafe {
            let mut key = HKEY::default();
            if RegOpenKeyExW(root, sub, None, KEY_READ | view, &mut key).is_err() {
                continue;
            }
            for index in 0.. {
                let mut name = [0u16; 256];
                let mut len = name.len() as u32;
                let status = RegEnumKeyExW(
                    key,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut len,
                    None,
                    None,
                    None,
                    None,
                );
                if status.is_err() {
                    break;
                }
                let child = HSTRING::from_wide(&name[..len as usize]);
                out.push(UninstallEntry {
                    install_location: reg_string(key, &child, w!("InstallLocation")),
                    display_icon: reg_string(key, &child, w!("DisplayIcon")),
                    size_kb: reg_dword(key, &child, w!("EstimatedSize")).map(u64::from),
                });
            }
            let _ = RegCloseKey(key);
        }
    }
    out
}

/// A string value of `key\sub` (environment variables expanded), if present.
unsafe fn reg_string(key: HKEY, sub: &HSTRING, value: PCWSTR) -> Option<String> {
    unsafe {
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        let mut bytes = 0u32;
        RegGetValueW(key, sub, value, flags, None, None, Some(&mut bytes))
            .ok()
            .ok()?;
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
        RegGetValueW(
            key,
            sub,
            value,
            flags,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// A DWORD value of `key\sub`, if present.
unsafe fn reg_dword(key: HKEY, sub: &HSTRING, value: PCWSTR) -> Option<u32> {
    unsafe {
        let mut data = 0u32;
        let mut bytes = size_of::<u32>() as u32;
        RegGetValueW(
            key,
            sub,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        Some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Action;

    fn row(kind: Kind, path: Option<&str>) -> SearchResult {
        SearchResult {
            kind,
            title: "x".into(),
            subtitle: String::new(),
            action: Action::Copy("x".into()),
            path: path.map(PathBuf::from),
        }
    }

    #[test]
    fn only_shown_rows_without_details_are_asked_for() {
        let results = [
            row(Kind::Calc, None),
            row(Kind::App, Some(r"C:\Menu\Code.lnk")),
            row(Kind::Setting, None),
            row(Kind::Folder, Some(r"C:\Home\Proyek")),
            row(Kind::File, Some(r"C:\Home\a.txt")),
            row(Kind::Web, None),
        ];
        let mut have = HashMap::new();
        let all = wanted_details(&results, &have);
        assert_eq!(
            all,
            [
                Item {
                    path: r"C:\Menu\Code.lnk".into(),
                    kind: ItemKind::App
                },
                Item {
                    path: r"C:\Home\Proyek".into(),
                    kind: ItemKind::Folder
                },
                Item {
                    path: r"C:\Home\a.txt".into(),
                    kind: ItemKind::File
                },
            ]
        );
        // One is known (even an empty answer counts): not asked again.
        have.insert(PathBuf::from(r"C:\Home\Proyek"), Detail::default());
        let rest = wanted_details(&results, &have);
        assert_eq!(rest.len(), 2);
        assert!(rest.iter().all(|i| i.path != Path::new(r"C:\Home\Proyek")));
        // All known: nothing.
        for item in &all {
            have.insert(item.path.clone(), Detail::default());
        }
        assert!(wanted_details(&results, &have).is_empty());
        // A path that is not in the shown rows is not asked for.
        assert!(wanted_details(&[], &HashMap::new()).is_empty());
    }

    #[test]
    fn answers_for_an_older_generation_are_dropped() {
        let results = [row(Kind::File, Some(r"C:\Home\a.txt"))];
        let mut d = Details::default();
        let first = d.request(&results).expect("asks for the one row");
        let more = [
            row(Kind::File, Some(r"C:\Home\a.txt")),
            row(Kind::File, Some(r"C:\Home\b.txt")),
        ];
        let second = d.request(&more).expect("a newer request");
        assert!(second.generation > first.generation);
        let detail = Detail {
            count: Some(3),
            ..Detail::default()
        };
        // The old generation's answer is dropped, the new one's is kept.
        assert!(!d.accept(first.generation, r"C:\Home\a.txt".into(), detail.clone()));
        assert!(d.known().is_empty());
        assert!(d.accept(second.generation, r"C:\Home\a.txt".into(), detail.clone()));
        assert_eq!(d.known().get(Path::new(r"C:\Home\a.txt")), Some(&detail));
        // Only what is still missing is asked next time.
        let third = d.request(&more).unwrap();
        assert_eq!(third.items.len(), 1);
        assert_eq!(third.items[0].path, Path::new(r"C:\Home\b.txt"));
        // Everything known: no request, no new generation.
        assert!(d.accept(third.generation, r"C:\Home\b.txt".into(), Detail::default()));
        assert!(d.request(&more).is_none());
        // The cache is emptied when it grows past its cap, and the rows on
        // screen are asked for again.
        for i in 0..=DETAILS_MAX {
            d.known
                .insert(PathBuf::from(format!(r"C:\Fill\{i}")), Detail::default());
        }
        let again = d.request(&more).expect("asked again after the clear");
        assert_eq!(again.items.len(), 2);
        assert!(d.known().len() <= DETAILS_MAX);
    }

    fn uninstall(location: Option<&str>, icon: Option<&str>, kb: Option<u64>) -> UninstallEntry {
        UninstallEntry {
            install_location: location.map(str::to_owned),
            display_icon: icon.map(str::to_owned),
            size_kb: kb,
        }
    }

    #[test]
    fn install_size_matches_by_location_or_icon() {
        let entries = [
            uninstall(
                Some(r"C:\Program Files\Microsoft VS Code\"),
                None,
                Some(300_000),
            ),
            uninstall(None, Some(r#""C:\Tools\Foo\foo.exe",0"#), Some(1_500)),
            uninstall(
                Some(r"D:\Apps\Bar"),
                Some(r"D:\Apps\Bar\bar.exe,1"),
                Some(2_048),
            ),
            uninstall(Some(r"C:\Apps\Empty"), None, None),
            uninstall(Some(r"C:\Apps\Zero"), None, Some(0)),
            // Nothing to say where it is.
            uninstall(None, None, Some(9)),
            uninstall(Some(""), Some(""), Some(9)),
            // Too broad to mean one app.
            uninstall(Some(r"C:\"), None, Some(7_000_000)),
            uninstall(Some(r"C:\Program Files"), None, Some(7_000_000)),
        ];
        let kb = |n: u64| Some(n * 1024);
        // A target inside the install location, without case and with the
        // trailing backslash on either side.
        assert_eq!(
            install_size(&entries, r"c:\program files\microsoft vs code\Code.exe"),
            kb(300_000)
        );
        // By the exe a DisplayIcon names: quotes and ",0" ignored.
        assert_eq!(install_size(&entries, r"C:\Tools\Foo\FOO.EXE"), kb(1_500));
        assert_eq!(install_size(&entries, r"D:\Apps\Bar\bar.exe"), kb(2_048));
        // A folder that only starts like the location is not inside it.
        assert_eq!(
            install_size(
                &entries,
                r"C:\Program Files\Microsoft VS Code Insiders\x.exe"
            ),
            None
        );
        // The location itself counts as a match.
        assert_eq!(install_size(&entries, r"D:\Apps\Bar"), kb(2_048));
        // No size, a zero size, no entry: nothing to show.
        assert_eq!(install_size(&entries, r"C:\Apps\Empty\e.exe"), None);
        assert_eq!(install_size(&entries, r"C:\Apps\Zero\z.exe"), None);
        assert_eq!(install_size(&entries, r"C:\Other\o.exe"), None);
        // A drive root or Program Files as a location never matches, and
        // an empty target matches nothing.
        assert_eq!(
            install_size(&entries, r"C:\Program Files\Other\o.exe"),
            None
        );
        assert_eq!(install_size(&entries, ""), None);
        assert_eq!(install_size(&[], r"C:\x.exe"), None);
    }

    #[test]
    fn a_row_adds_a_folders_count_or_an_apps_size() {
        let counted = Detail {
            count: Some(12),
            ..Detail::default()
        };
        let sized = Detail {
            install: Some(300 * 1024 * 1024),
            ..Detail::default()
        };
        let both = Detail {
            count: Some(1500),
            install: Some(1),
            icon: None,
        };
        assert_eq!(
            row_note(Kind::Folder, &counted, Lang::En).as_deref(),
            Some("12 items")
        );
        assert_eq!(
            row_note(Kind::Folder, &counted, Lang::Id).as_deref(),
            Some("12 item")
        );
        assert_eq!(
            row_note(Kind::Folder, &both, Lang::En).as_deref(),
            Some("999+ items")
        );
        assert_eq!(
            row_note(Kind::App, &sized, Lang::En).as_deref(),
            Some("300 MB")
        );
        // Only what fits the kind: an app has no count, a folder no install size.
        assert_eq!(row_note(Kind::App, &counted, Lang::En), None);
        assert_eq!(row_note(Kind::Folder, &sized, Lang::En), None);
        // Files, settings and the rest add nothing; nothing known, nothing added.
        for kind in [Kind::File, Kind::Setting, Kind::Calc, Kind::Web] {
            assert_eq!(row_note(kind, &both, Lang::En), None);
        }
        assert_eq!(row_note(Kind::Folder, &Detail::default(), Lang::En), None);
        assert_eq!(row_note(Kind::App, &Detail::default(), Lang::En), None);
    }

    #[test]
    fn icon_pixels_are_premultiplied() {
        // B, G, R, A: half-transparent white, opaque red, clear, quarter blue.
        let mut px = [
            255, 255, 255, 128, //
            0, 0, 200, 255, //
            90, 90, 90, 0, //
            200, 0, 0, 64,
        ];
        premultiply(&mut px);
        assert_eq!(
            px,
            [
                128, 128, 128, 128, //
                0, 0, 200, 255, //
                0, 0, 0, 0, //
                50, 0, 0, 64
            ]
        );
        // No alpha anywhere (a plain bitmap): opaque, colours untouched.
        let mut plain = [10, 20, 30, 0, 40, 50, 60, 0];
        premultiply(&mut plain);
        assert_eq!(plain, [10, 20, 30, 255, 40, 50, 60, 255]);
        // Empty and ragged buffers do not panic.
        premultiply(&mut []);
        premultiply(&mut [1, 2, 3]);
    }
}
