//! Rendering the picture with its annotations at full resolution (Direct2D
//! on a WIC bitmap), saving it as a PNG in the Screenshots folder, and
//! copying it to the clipboard.
//!
//! Nothing here runs until Save or Copy is pressed, or (auto-copy) until the
//! editor opens or a change settles: then one short-lived worker renders.
//! The PNG encoding and the
//! unique file naming are the screenshot preview's own (`screenshot::win`,
//! `screenshot::save_name`), not copies of them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapCacheOnLoad,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::w;

use super::gfx::{Gfx, bitmap, color, rect_f};
use super::paint::{Pix, draw_annotations};
use super::{Annotation, View};
use crate::capture::Shot;
use crate::clipboard::image::Thumb;
use crate::screenshot;

/// `CF_DIB`.
const CF_DIB: u32 = 8;

/// How a save ended, handed back to the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveResult {
    Saved(PathBuf),
    Failed,
}

/// Where finished saves wait for the window to pick them up.
pub type Inbox = Arc<Mutex<Vec<SaveResult>>>;

/// The picture with every annotation drawn in, at its own size: top-down
/// opaque BGRA, `base.w * base.h * 4` bytes. Needs COM on this thread.
pub fn render_bgra(base: &Shot, doc: &[Annotation]) -> Option<Vec<u8>> {
    if base.w == 0 || base.h == 0 || base.bgra.len() != base.w as usize * base.h as usize * 4 {
        return None;
    }
    unsafe {
        let gfx = Gfx::new().ok()?;
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let target = wic
            .CreateBitmap(
                base.w,
                base.h,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapCacheOnLoad,
            )
            .ok()?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let rt = gfx.d2d.CreateWicBitmapRenderTarget(&target, &props).ok()?;
        let brush = rt.CreateSolidColorBrush(&color(0, 1.0), None).ok()?;
        let picture = bitmap(&rt, base.w, base.h, &base.bgra).ok()?;
        rt.BeginDraw();
        rt.Clear(Some(&color(0, 1.0)));
        let all = rect_f(0.0, 0.0, base.w as f32, base.h as f32);
        rt.DrawBitmap(
            &picture,
            Some(&all),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            None,
        );
        let view = View {
            scale: 1.0,
            ox: 0.0,
            oy: 0.0,
        };
        let mut pix = Pix::default();
        draw_annotations(&rt, &brush, &gfx, base, doc, &view, None, &mut pix);
        rt.EndDraw(None, None).ok()?;
        let mut out = vec![0u8; base.w as usize * base.h as usize * 4];
        target
            .CopyPixels(std::ptr::null(), base.w * 4, &mut out)
            .ok()?;
        // Everything is opaque; make sure of it.
        for px in out.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        Some(out)
    }
}

/// `render_bgra`, encoded as a PNG (the preview's WIC encoder).
pub fn render_png(base: &Shot, doc: &[Annotation]) -> Option<Vec<u8>> {
    let bgra = render_bgra(base, doc)?;
    screenshot::win::encode_png(base.w, base.h, &bgra)
}

/// The name for a new file in `dir`: "Screenshot YYYY-MM-DD HHMMSS.png",
/// " (2)" and on while one is there.
pub fn next_path(dir: &Path, now: chrono::NaiveDateTime) -> PathBuf {
    dir.join(screenshot::save_name(now, |n| dir.join(n).exists()))
}

/// Writes `png` as a new file in `dir` (made if missing); never over a file.
/// A name taken since it was chosen is chosen again, a few times.
pub fn write_new(dir: &Path, png: &[u8], now: chrono::NaiveDateTime) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    (0..5).find_map(|_| screenshot::win::write_at(&next_path(dir, now), png))
}

/// Save on a short-lived worker: render, encode, write, then put the result
/// in `inbox` and ring `hwnd` with `msg`. `dir` is the Screenshots folder
/// unless the demo gives its own. False when the thread could not start.
pub fn spawn_save(
    base: Arc<Shot>,
    doc: Vec<Annotation>,
    dir: Option<PathBuf>,
    inbox: Inbox,
    hwnd: HWND,
    msg: u32,
) -> bool {
    let target = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("shot-edit-save".into())
        .spawn(move || {
            let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let saved = (|| {
                let dir = dir.or_else(screenshot::win::screenshots_dir)?;
                let png = render_png(&base, &doc)?;
                write_new(&dir, &png, chrono::Local::now().naive_local())
            })();
            if inited {
                unsafe { CoUninitialize() };
            }
            let result = saved.map_or(SaveResult::Failed, SaveResult::Saved);
            if let Ok(mut list) = inbox.lock() {
                list.push(result);
            }
            unsafe {
                let _ = PostMessageW(Some(HWND(target as *mut _)), msg, WPARAM(0), LPARAM(0));
            }
        })
        .is_ok()
}

/// The picture with its annotations, rendered and encoded for the
/// clipboard: a packed DIB and a PNG.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub dib: Vec<u8>,
    pub png: Vec<u8>,
}

/// Renders `doc` over `base` and encodes both clipboard formats. Needs COM
/// on this thread (the auto-copy worker, or the UI thread for Copy).
pub fn prepare(base: &Shot, doc: &[Annotation]) -> Option<Prepared> {
    let bgra = render_bgra(base, doc)?;
    let png = screenshot::win::encode_png(base.w, base.h, &bgra)?;
    let dib = crate::clipboard::image::dib_from_thumb(&Thumb {
        w: base.w,
        h: base.h,
        bgra,
    });
    Some(Prepared { dib, png })
}

/// CF_DIB and the registered "PNG", the formats Copy writes.
fn formats(p: &Prepared) -> Vec<(u32, Vec<u8>)> {
    let png_format = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    let mut formats = vec![(CF_DIB, p.dib.clone())];
    if png_format != 0 {
        formats.push((png_format, p.png.clone()));
    }
    formats
}

/// How a guarded write ended (see `clipboard::win::write_when`).
pub use crate::clipboard::win::Wrote as Put;

/// Puts `prepared` on the clipboard, owned by `hwnd`, only while the
/// clipboard's sequence is still `expected` (`None` for the first write of a
/// capture). On success the sequence it left is recorded as the editor's own
/// (`capture`, `managed`), so the screenshot preview skips it and the island
/// treats it by the editor's rules. Runs on the editor's thread, never inside
/// a borrow of the editor: `EmptyClipboard` may send WM_DESTROYCLIPBOARD to
/// another editor's window on this thread.
pub fn put(
    hwnd: HWND,
    prepared: &Prepared,
    expected: Option<u32>,
    capture: u64,
    managed: bool,
) -> Put {
    let result = crate::clipboard::win::write_when(hwnd, &formats(prepared), expected);
    if let Put::Written(seq) = result {
        super::record_own_copy(seq, capture, managed);
    }
    result
}

/// What an auto-copy worker hands back: the picture of document revision
/// `rev`, or `None` when it could not be rendered.
#[derive(Debug)]
pub struct CopyReady {
    pub rev: u64,
    pub prepared: Option<Prepared>,
}

/// Where finished renders wait for the window to pick them up.
pub type CopyInbox = Arc<Mutex<Vec<CopyReady>>>;

/// Renders the picture on a short-lived worker (`shot-edit-copy`) and puts
/// the result in `inbox`, then rings `hwnd` with `msg`. Like `spawn_save`: if
/// the window is gone by then, the Arc is all that is left. False when the
/// thread could not start.
pub fn spawn_auto_copy(
    base: Arc<Shot>,
    doc: Vec<Annotation>,
    rev: u64,
    inbox: CopyInbox,
    hwnd: HWND,
    msg: u32,
) -> bool {
    let target = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("shot-edit-copy".into())
        .spawn(move || {
            let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let prepared = prepare(&base, &doc);
            if inited {
                unsafe { CoUninitialize() };
            }
            if let Ok(mut list) = inbox.lock() {
                list.push(CopyReady { rev, prepared });
            }
            unsafe {
                let _ = PostMessageW(Some(HWND(target as *mut _)), msg, WPARAM(0), LPARAM(0));
            }
        })
        .is_ok()
}

/// Puts the picture with its annotations on the clipboard as a PNG and a
/// DIB, owned by `hwnd`, and records the sequence number it left (for
/// `capture`; `managed` is the editor's auto-copy flag) so the notch's
/// screenshot preview skips this one image. `None` when it could not be
/// rendered or the clipboard stayed busy. Needs COM on this thread.
pub fn copy(
    hwnd: HWND,
    base: &Shot,
    doc: &[Annotation],
    capture: u64,
    managed: bool,
) -> Option<u32> {
    let prepared = prepare(base, doc)?;
    let seq = crate::clipboard::win::write(hwnd, &formats(&prepared))?;
    super::record_own_copy(seq, capture, managed);
    Some(seq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shot_edit::{FRect, Style};
    use chrono::NaiveDate;

    fn at() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 29)
            .and_then(|d| d.and_hms_opt(9, 5, 7))
            .expect("valid time")
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vale-shotedit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_new_file_is_named_like_windows_and_never_overwrites() {
        let dir = temp("names");
        let png = b"\x89PNG fake";
        let first = write_new(&dir, png, at()).expect("first");
        assert_eq!(
            first.file_name().and_then(|n| n.to_str()),
            Some("Screenshot 2026-10-29 090507.png")
        );
        let second = write_new(&dir, png, at()).expect("second");
        assert_eq!(
            second.file_name().and_then(|n| n.to_str()),
            Some("Screenshot 2026-10-29 090507 (2).png")
        );
        let third = write_new(&dir, b"other", at()).expect("third");
        assert!(third.ends_with("Screenshot 2026-10-29 090507 (3).png"));
        assert_eq!(
            std::fs::read(&first).unwrap(),
            png,
            "the first is untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_a_failure_not_a_panic() {
        let dir = temp("blocked");
        std::fs::create_dir_all(&dir).unwrap();
        // A file where the folder should be.
        let file = dir.join("not-a-folder");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(write_new(&file.join("Screenshots"), b"png", at()), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_or_short_picture_is_not_rendered() {
        let none = Shot {
            w: 0,
            h: 0,
            bgra: Vec::new(),
        };
        assert!(render_bgra(&none, &[]).is_none());
        let short = Shot {
            w: 4,
            h: 4,
            bgra: vec![0; 10],
        };
        assert!(render_bgra(&short, &[]).is_none());
    }

    #[test]
    fn the_export_draws_the_annotations_into_the_picture() {
        // White 40 x 30 picture with a red filled-looking thick rectangle.
        let base = Shot {
            w: 40,
            h: 30,
            bgra: [255u8, 255, 255, 255].repeat(40 * 30),
        };
        let doc = vec![Annotation::Rect {
            r: FRect::new(5.0, 5.0, 30.0, 20.0),
            style: Style {
                color: 0xE5484D,
                width: 4.0,
            },
        }];
        let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        let plain = render_bgra(&base, &[]);
        let marked = render_bgra(&base, &doc);
        if inited {
            unsafe { CoUninitialize() };
        }
        let (plain, marked) = (plain.expect("plain"), marked.expect("marked"));
        assert_eq!(plain.len(), 40 * 30 * 4);
        assert!(plain.chunks(4).all(|p| p == [255, 255, 255, 255]));
        // A pixel on the rectangle's left edge (x = 5, y = 15) turned red
        // (BGRA: blue 0x4D, green 0x48, red 0xE5); the centre stayed white.
        let at = |x: usize, y: usize| &marked[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4];
        assert_eq!(at(5, 15), [0x4D, 0x48, 0xE5, 255]);
        assert_eq!(at(20, 15), [255, 255, 255, 255]);
    }
}
