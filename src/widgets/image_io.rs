//! The Win32 side of a sticky note's images: reading a size, decoding a copy
//! at the size it is drawn, importing a file, a pasted PNG or a pasted bitmap
//! into the note-images folder, the file dialog, and opening a file. The
//! pure rules (names, caps, the grid) are `images`.
//!
//! An import runs on a worker thread (COM on its own, multithreaded) so a big
//! photo never stalls the island. The result goes back to the note window as
//! `WM_NOTE_IMPORT` (`WM_APP + 2`). It is NOTE-LOCAL, like `WM_END_EDIT`
//! (`WM_APP + 1`): the note window's own procedure takes it, so it needs no
//! id the island would have to keep unique (the island's own `WM_APP`
//! messages are above 50). No `WM_APP` message is global for it.
//!
//! The original file is only ever opened for READING (WIC's decoder); the saved
//! copy is a new file written to a `.tmp` and renamed. A source over 25 MiB is
//! refused before it is decoded. EXIF orientation is applied (the same helper
//! as the phone's thumbnails), so the copy is upright and carries no EXIF.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::Foundation::{GENERIC_READ, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_ContainerFormatPng,
    GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat32bppBGRA, GUID_WICPixelFormat32bppPBGRA,
    IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICBitmapTransformOptions,
    WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::PROPBAG2;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    STREAM_SEEK_END, STREAM_SEEK_SET,
};
use windows::Win32::System::Variant::{VARIANT, VT_R4};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};
use windows::core::{HSTRING, Interface, PWSTR, w};

use crate::widgets::images::{self, MAX_SIDE, MAX_SOURCE_BYTES};

/// An import finished: sent to the note window (`lParam` is a `Box<Done>`).
pub const WM_NOTE_IMPORT: u32 = WM_APP + 2;

/// JPEG quality of a saved copy of a JPEG (0 to 1).
const JPEG_QUALITY: f32 = 0.9;

/// Where an image comes from.
#[derive(Clone, Debug)]
pub enum Source {
    /// A file on disk (never written).
    Path(PathBuf),
    /// PNG (or any WIC format) bytes: the clipboard's "PNG".
    Png(Vec<u8>),
    /// A packed DIB: CF_DIBV5 or CF_DIB.
    Dib(Vec<u8>),
}

/// Why an import did not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The source is over 25 MiB.
    TooBig,
    /// It could not be decoded (not an image, a format WIC lacks, damaged).
    Unreadable,
    /// The copy could not be written.
    Io,
}

/// What the worker sends back to the note window.
pub struct Done {
    pub note: String,
    pub result: Result<String, ImportError>,
    /// The folder the file went to (to delete it when no window takes it).
    pub dir: PathBuf,
}

/// The path of image `name` in `dir`, only for a valid name: the one place a
/// name read from the notes file meets a path.
pub fn path_of(dir: &Path, name: &str) -> Option<PathBuf> {
    (images::valid_name(name) && !dir.as_os_str().is_empty()).then(|| dir.join(name))
}

/// COM for the calling thread for as long as this lives (a thread that
/// already has it in the other mode keeps its own).
struct Com(bool);

fn com() -> Com {
    Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn factory() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.ok()
}

/// The size a `w` x `h` image is saved at: the longest side at most `MAX_SIDE`,
/// never enlarged, at least 1 px.
pub fn fit_size(w: u32, h: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= MAX_SIDE || long == 0 {
        return (w.max(1), h.max(1));
    }
    let scale = |side: u32| {
        ((u64::from(side) * u64::from(MAX_SIDE) + u64::from(long) / 2) / u64::from(long)).max(1) as u32
    };
    (scale(w), scale(h))
}

/// The size an image `iw` x `ih` is decoded at to cover a `tw` x `th` cell:
/// the smaller side fills it, never enlarged beyond the file's own size.
pub fn cover_size(iw: u32, ih: u32, tw: u32, th: u32) -> (u32, u32) {
    if iw == 0 || ih == 0 {
        return (1, 1);
    }
    let f = (tw as f32 / iw as f32).max(th as f32 / ih as f32).min(1.0);
    (
        ((iw as f32 * f).round() as u32).max(1),
        ((ih as f32 * f).round() as u32).max(1),
    )
}

// ------------------------------------------------------------------ reading

/// The pixel size of image `name` in `dir` (cheap: the header only), or
/// `None` when it is missing or unreadable.
pub fn size_of(dir: &Path, name: &str) -> Option<(u32, u32)> {
    let path = path_of(dir, name)?;
    let _com = com();
    let wic = factory()?;
    unsafe {
        let decoder = wic
            .CreateDecoderFromFilename(
                &HSTRING::from(path.as_os_str()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        (w > 0 && h > 0).then_some((w, h))
    }
}

/// Image `name` in `dir` decoded to premultiplied BGRA at the size that
/// covers a `tw` x `th` cell (`cover_size`): `(width, height, pixels)`.
pub fn decode_cover(dir: &Path, name: &str, tw: u32, th: u32) -> Option<(u32, u32, Vec<u8>)> {
    let path = path_of(dir, name)?;
    let _com = com();
    let wic = factory()?;
    unsafe {
        let decoder = wic
            .CreateDecoderFromFilename(
                &HSTRING::from(path.as_os_str()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut iw, mut ih) = (0u32, 0u32);
        frame.GetSize(&mut iw, &mut ih).ok()?;
        let (ow, oh) = cover_size(iw, ih, tw, th);
        let source: IWICBitmapSource = frame.cast().ok()?;
        let source = if (ow, oh) != (iw, ih) {
            let scaler = wic.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&source, ow, oh, WICBitmapInterpolationModeFant)
                .ok()?;
            scaler.cast().ok()?
        } else {
            source
        };
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &source,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut pixels = vec![0u8; (ow as usize) * (oh as usize) * 4];
        converter
            .CopyPixels(std::ptr::null(), ow * 4, &mut pixels)
            .ok()?;
        Some((ow, oh, pixels))
    }
}

// ------------------------------------------------------------------ importing

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A packed DIB as top-down opaque BGRA.
fn dib_to_bgra(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let info = crate::clipboard::image::dib_info(bytes)?;
    let (w, h) = (info.w as usize, info.h as usize);
    let bpp = (info.bpp / 8) as usize;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let src_row = if info.top_down { y } else { h - 1 - y };
        let row = bytes.get(info.offset + src_row * info.stride..)?;
        for x in 0..w {
            let px = row.get(x * bpp..x * bpp + 3)?;
            let o = (y * w + x) * 4;
            out[o..o + 3].copy_from_slice(px);
            out[o + 3] = 255;
        }
    }
    Some((info.w, info.h, out))
}

/// Imports `src` into `dir`: decodes it (the original only read), turns it
/// upright, shrinks it so the longest side is at most 1600 px, and writes a
/// JPEG (quality 0.9) when the source is a JPEG, else a PNG, to a `.tmp` that
/// is then renamed. Returns the new file's name. COM is joined for the call.
pub fn import(src: Source, dir: &Path) -> Result<String, ImportError> {
    let size_ok = |n: u64| n <= MAX_SOURCE_BYTES;
    match &src {
        Source::Path(p) => {
            let len = std::fs::metadata(p).map_err(|_| ImportError::Unreadable)?.len();
            if !size_ok(len) {
                return Err(ImportError::TooBig);
            }
        }
        Source::Png(b) | Source::Dib(b) if !size_ok(b.len() as u64) => return Err(ImportError::TooBig),
        _ => {}
    }
    let _com = com();
    let wic = factory().ok_or(ImportError::Unreadable)?;
    let (jpeg, encoded) = unsafe { encode_copy(&wic, &src) }.ok_or(ImportError::Unreadable)?;
    std::fs::create_dir_all(dir).map_err(|_| ImportError::Io)?;
    let ext = if jpeg { "jpg" } else { "png" };
    let name = images::unique_name(unix_ms(), COUNTER.fetch_add(1, Ordering::Relaxed), ext);
    let path = dir.join(&name);
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = std::fs::write(&tmp, &encoded).and_then(|()| std::fs::rename(&tmp, &path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err(ImportError::Io);
    }
    Ok(name)
}

/// The copy's bytes and whether it is a JPEG.
///
/// # Safety
/// COM must be initialised on the calling thread.
unsafe fn encode_copy(wic: &IWICImagingFactory, src: &Source) -> Option<(bool, Vec<u8>)> {
    unsafe {
        let mut is_jpeg = false;
        let mut source: IWICBitmapSource = match src {
            Source::Dib(bytes) => {
                let (w, h, bgra) = dib_to_bgra(bytes)?;
                wic.CreateBitmapFromMemory(w, h, &GUID_WICPixelFormat32bppBGRA, w * 4, &bgra)
                    .ok()?
                    .cast()
                    .ok()?
            }
            other => {
                let decoder = match other {
                    Source::Path(p) => wic
                        .CreateDecoderFromFilename(
                            &HSTRING::from(p.as_os_str()),
                            None,
                            GENERIC_READ,
                            WICDecodeMetadataCacheOnDemand,
                        )
                        .ok()?,
                    Source::Png(bytes) => {
                        let stream = wic.CreateStream().ok()?;
                        stream.InitializeFromMemory(bytes).ok()?;
                        wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                            .ok()?
                    }
                    Source::Dib(_) => return None,
                };
                is_jpeg = decoder.GetContainerFormat().ok()? == GUID_ContainerFormatJpeg;
                let frame = decoder.GetFrame(0).ok()?;
                let mut source: IWICBitmapSource = frame.cast().ok()?;
                if let Some(turn) = crate::phone::thumb::orientation(&frame) {
                    let rotator = wic.CreateBitmapFlipRotator().ok()?;
                    rotator
                        .Initialize(&source, WICBitmapTransformOptions(turn))
                        .ok()?;
                    source = rotator.cast().ok()?;
                }
                source
            }
        };
        let (w, h) = {
            let (mut w, mut h) = (0u32, 0u32);
            source.GetSize(&mut w, &mut h).ok()?;
            (w, h)
        };
        if w == 0 || h == 0 {
            return None;
        }
        let (tw, th) = fit_size(w, h);
        if (tw, th) != (w, h) {
            let scaler = wic.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&source, tw, th, WICBitmapInterpolationModeFant)
                .ok()?;
            source = scaler.cast().ok()?;
        }
        let pixel_format = if is_jpeg {
            GUID_WICPixelFormat24bppBGR
        } else {
            GUID_WICPixelFormat32bppBGRA
        };
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &source,
                &pixel_format,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let container = if is_jpeg { GUID_ContainerFormatJpeg } else { GUID_ContainerFormatPng };
        let encoder = wic.CreateEncoder(&container, std::ptr::null()).ok()?;
        let stream = SHCreateMemStream(None)?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
        let mut out = None;
        let mut options = None;
        encoder.CreateNewFrame(&mut out, &mut options).ok()?;
        let out = out?;
        if is_jpeg
            && let Some(bag) = &options
        {
            // A failed quality hint leaves WIC's default: not worth failing.
            let mut name: Vec<u16> = "ImageQuality\0".encode_utf16().collect();
            let prop = PROPBAG2 {
                dwType: 1,
                vt: VT_R4,
                pstrName: PWSTR(name.as_mut_ptr()),
                ..Default::default()
            };
            let _ = bag.Write(1, &prop, &VARIANT::from(JPEG_QUALITY));
        }
        out.Initialize(options.as_ref()).ok()?;
        out.SetSize(tw, th).ok()?;
        let mut format = pixel_format;
        out.SetPixelFormat(&mut format).ok()?;
        out.WriteSource(&converter, std::ptr::null()).ok()?;
        out.Commit().ok()?;
        encoder.Commit().ok()?;
        let mut len = 0u64;
        stream.Seek(0, STREAM_SEEK_END, Some(&mut len)).ok()?;
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let mut bytes = vec![0u8; usize::try_from(len).ok()?];
        let mut read = 0u32;
        stream
            .Read(bytes.as_mut_ptr().cast(), u32::try_from(len).ok()?, Some(&mut read))
            .ok()
            .ok()?;
        (read as usize == bytes.len() && !bytes.is_empty()).then_some((is_jpeg, bytes))
    }
}

/// Imports `src` for note `note` on a worker thread and tells window `hwnd`
/// with `WM_NOTE_IMPORT` when it is done. The window may be gone by then: the
/// message fails and the new file is deleted. False when the thread could not
/// start (nothing will arrive).
pub fn spawn_import(hwnd: HWND, note: String, src: Source, dir: PathBuf) -> bool {
    let target = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("note-image".into())
        .spawn(move || {
            let result = import(src, &dir);
            let done = Box::new(Done { note, result, dir });
            let raw = Box::into_raw(done);
            let posted = unsafe {
                PostMessageW(
                    Some(HWND(target as *mut _)),
                    WM_NOTE_IMPORT,
                    WPARAM(0),
                    LPARAM(raw as isize),
                )
            };
            if posted.is_err() {
                // No window to take it: nobody else will, so the file goes.
                let done = unsafe { Box::from_raw(raw) };
                discard(&done);
            }
        })
        .is_ok()
}

/// Deletes the file an import made (a refused or orphaned result).
pub fn discard(done: &Done) {
    if let Ok(name) = &done.result
        && let Some(path) = path_of(&done.dir, name)
    {
        let _ = std::fs::remove_file(path);
    }
}

// ------------------------------------------------------------------ cleaning

/// Deletes the files `names` from `dir`. Only valid names ever meet the disk;
/// anything else is skipped.
pub fn delete_named(dir: &Path, names: &[String]) {
    for name in names {
        if let Some(path) = path_of(dir, name) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The startup sweep: the files of `dir` that nothing in `referenced` names and
/// that are older than a day at `now` (a crash, a note deleted while the
/// island was off) are deleted; returns how many. Only valid names inside the
/// folder: a stray file the owner put there is left alone.
pub fn sweep(dir: &Path, referenced: &std::collections::HashSet<String>, now: i64) -> usize {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    let listing: Vec<(String, i64)> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let secs = e
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some((name, secs as i64))
        })
        .collect();
    let gone = images::orphans(&listing, referenced, now);
    delete_named(dir, &gone);
    gone.len()
}

/// A `w` x `h` gradient PNG at `path`, for the demo (debug builds) and the
/// tests: made up, never a user's file. `seed` tilts the colours.
#[cfg(any(test, debug_assertions))]
pub fn write_gradient(path: &Path, w: u32, h: u32, seed: u8) -> bool {
    let _com = com();
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            bgra.extend_from_slice(&[
                (x * 255 / w.max(1)) as u8 ^ seed,
                (y * 255 / h.max(1)) as u8,
                (255 - x * 255 / w.max(1)) as u8 / 2 + seed / 2,
                255,
            ]);
        }
    }
    let Some(png) = crate::screenshot::win::encode_png(w, h, &bgra) else {
        return false;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(path, png).is_ok()
}

// ------------------------------------------------------------------ the shell

/// Opens `path` with the system's viewer. A test only records it: no test may
/// launch an app on the owner's desktop.
pub fn shell_open(path: &Path) {
    #[cfg(test)]
    {
        OPENED.with(|o| o.borrow_mut().push(path.to_path_buf()));
    }
    #[cfg(not(test))]
    unsafe {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(path.as_os_str()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(test)]
thread_local! {
    /// The paths `shell_open` was asked for (tests only).
    pub static OPENED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The image files the owner picks in the system dialog (several allowed),
/// `png jpg jpeg gif bmp webp`. Empty when it is cancelled or cannot open.
/// Blocks in the dialog's own message loop: the caller must hold no borrow.
pub fn pick_files(owner: HWND, title: &str, filter_name: &str) -> Vec<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FOS_ALLOWMULTISELECT, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    use windows::core::PCWSTR;
    let mut out = Vec::new();
    let _com = com();
    unsafe {
        let Ok(dialog) = CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
        else {
            return out;
        };
        let Ok(mut options) = dialog.GetOptions() else {
            return out;
        };
        options |= FOS_FORCEFILESYSTEM | FOS_ALLOWMULTISELECT;
        let name = HSTRING::from(filter_name);
        let filters = [COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: w!("*.png;*.jpg;*.jpeg;*.gif;*.bmp;*.webp"),
        }];
        let _ = dialog.SetFileTypes(&filters);
        let _ = dialog.SetTitle(&HSTRING::from(title));
        if dialog.SetOptions(options).is_err() || dialog.Show(Some(owner)).is_err() {
            return out;
        }
        let Ok(items) = dialog.GetResults() else {
            return out;
        };
        for i in 0..items.GetCount().unwrap_or(0) {
            let Ok(item) = items.GetItemAt(i) else { continue };
            let Ok(display) = item.GetDisplayName(SIGDN_FILESYSPATH) else {
                continue;
            };
            out.push(PathBuf::from(std::ffi::OsString::from_wide(display.as_wide())));
            CoTaskMemFree(Some(display.0 as *const std::ffi::c_void));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::thumb::tests::write_gradient_png;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vale-image-io-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_saved_size_never_passes_1600_and_keeps_the_shape() {
        assert_eq!(fit_size(800, 600), (800, 600));
        assert_eq!(fit_size(1600, 1000), (1600, 1000));
        assert_eq!(fit_size(3200, 1600), (1600, 800));
        assert_eq!(fit_size(1600, 3200), (800, 1600));
        assert_eq!(fit_size(4000, 3000), (1600, 1200));
        assert_eq!(fit_size(100_000, 10), (1600, 1));
        assert_eq!(fit_size(0, 0), (1, 1));
    }

    #[test]
    fn a_cell_is_covered_never_enlarged() {
        // 800x600 for a 200x100 cell: scaled to 200 wide (the smaller side
        // that still covers).
        assert_eq!(cover_size(800, 600, 200, 100), (200, 150));
        assert_eq!(cover_size(600, 800, 200, 100), (200, 267));
        // A small picture is not enlarged.
        assert_eq!(cover_size(50, 40, 200, 100), (50, 40));
        assert_eq!(cover_size(0, 5, 10, 10), (1, 1));
    }

    #[test]
    fn the_sweep_deletes_only_old_unreferenced_valid_files() {
        let dir = temp("sweep");
        let day = std::time::Duration::from_secs(25 * 3600);
        let write = |name: &str, old: bool| {
            let path = dir.join(name);
            std::fs::write(&path, b"x").unwrap();
            if old {
                let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
                f.set_modified(std::time::SystemTime::now() - day).unwrap();
            }
        };
        write("old-unused.png", true);
        write("old-used.png", true);
        write("fresh-unused.png", false);
        write("old-note.txt", true);
        write("old name.png", true);
        std::fs::create_dir_all(dir.join("sub.png")).unwrap();
        let used: std::collections::HashSet<String> = ["old-used.png".to_owned()].into();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert_eq!(sweep(&dir, &used, now), 1);
        assert!(!dir.join("old-unused.png").exists());
        for kept in ["old-used.png", "fresh-unused.png", "old-note.txt", "old name.png"] {
            assert!(dir.join(kept).exists(), "{kept}");
        }
        assert_eq!(sweep(&dir.join("missing"), &used, now), 0, "no folder, nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_named_only_touches_valid_names_inside_the_folder() {
        let dir = temp("delete");
        let inside = dir.join("images");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::write(inside.join("a.png"), b"x").unwrap();
        std::fs::write(dir.join("outside.png"), b"x").unwrap();
        delete_named(
            &inside,
            &["..\\outside.png".to_owned(), "../outside.png".to_owned(), "a.png".to_owned()],
        );
        assert!(!inside.join("a.png").exists());
        assert!(dir.join("outside.png").exists(), "never outside the folder");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_is_only_joined_when_valid() {
        let dir = Path::new("C:\\data\\note-images");
        assert_eq!(path_of(dir, "a.png"), Some(dir.join("a.png")));
        assert_eq!(path_of(dir, "..\\a.png"), None);
        assert_eq!(path_of(dir, "con.png"), None);
        assert_eq!(path_of(Path::new(""), "a.png"), None, "no folder set");
    }

    #[test]
    fn a_png_is_imported_as_a_copy_and_the_original_is_untouched() {
        let dir = temp("png");
        let src = dir.join("photo.png");
        write_gradient_png(&src, 200, 100);
        let before = std::fs::read(&src).unwrap();
        let out = dir.join("note-images");
        let name = import(Source::Path(src.clone()), &out).expect("imported");
        assert!(images::valid_name(&name), "{name}");
        assert!(name.ends_with(".png"));
        assert!(out.join(&name).is_file());
        assert!(!out.join(format!("{name}.tmp")).exists(), "the tmp was renamed");
        assert_eq!(std::fs::read(&src).unwrap(), before, "the original is never written");
        assert_eq!(size_of(&out, &name), Some((200, 100)));
        // Two imports never collide.
        let again = import(Source::Path(src), &out).unwrap();
        assert_ne!(name, again);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_big_picture_is_shrunk_to_1600() {
        let dir = temp("big");
        let src = dir.join("wide.png");
        write_gradient_png(&src, 2400, 600);
        let out = dir.join("note-images");
        let name = import(Source::Path(src), &out).expect("imported");
        assert_eq!(size_of(&out, &name), Some((1600, 400)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn png_bytes_and_a_dib_are_imported_as_png() {
        let dir = temp("bytes");
        let out = dir.join("note-images");
        // PNG bytes.
        let (w, h) = (30u32, 20u32);
        let bgra: Vec<u8> = (0..w * h).flat_map(|i| [i as u8, 90, 200, 255]).collect();
        let png = {
            let _c = com();
            crate::screenshot::win::encode_png(w, h, &bgra).expect("a png")
        };
        let name = import(Source::Png(png), &out).expect("png bytes");
        assert!(name.ends_with(".png"));
        assert_eq!(size_of(&out, &name), Some((30, 20)));
        // A 24-bit bottom-up DIB.
        let dib = crate::clipboard::image::tests::dib24(16, 12);
        let name = import(Source::Dib(dib), &out).expect("a dib");
        assert!(name.ends_with(".png"));
        assert_eq!(size_of(&out, &name), Some((16, 12)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn something_that_is_not_an_image_is_unreadable_and_a_huge_source_is_refused() {
        let dir = temp("bad");
        let out = dir.join("note-images");
        let txt = dir.join("a.png");
        std::fs::write(&txt, b"this is not a picture").unwrap();
        assert_eq!(import(Source::Path(txt), &out), Err(ImportError::Unreadable));
        assert_eq!(
            import(Source::Path(dir.join("missing.png")), &out),
            Err(ImportError::Unreadable)
        );
        assert_eq!(import(Source::Png(b"nope".to_vec()), &out), Err(ImportError::Unreadable));
        assert_eq!(import(Source::Dib(vec![1, 2, 3]), &out), Err(ImportError::Unreadable));
        let huge = vec![0u8; (MAX_SOURCE_BYTES + 1) as usize];
        assert_eq!(import(Source::Png(huge), &out), Err(ImportError::TooBig));
        // Nothing was written for any of them.
        let left = std::fs::read_dir(&out).map(|d| d.count()).unwrap_or(0);
        assert_eq!(left, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_copy_decodes_at_the_size_of_its_cell() {
        let dir = temp("cover");
        let src = dir.join("p.png");
        write_gradient_png(&src, 400, 300);
        let out = dir.join("note-images");
        let name = import(Source::Path(src), &out).unwrap();
        let (w, h, px) = decode_cover(&out, &name, 100, 50).expect("decoded");
        assert_eq!((w, h), (100, 75));
        assert_eq!(px.len(), 100 * 75 * 4);
        assert!(px.chunks(4).all(|p| p[3] == 255), "opaque");
        assert!(decode_cover(&out, "missing.png", 10, 10).is_none());
        assert!(decode_cover(&out, "../x.png", 10, 10).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
