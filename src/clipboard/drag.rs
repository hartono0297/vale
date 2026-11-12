//! Dragging an image card out of the island: the file name, the pixels a PNG
//! is written from, and the bookkeeping of the temporary files. Pure, tested
//! without the clipboard or the disk; the PNG writing lives elsewhere.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::NaiveDateTime;
use windows::Win32::Graphics::Imaging::{
    GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat32bppBGRA,
};

use super::image::dib_info;
use super::pack;

/// How long a dragged-out temp file stays after its last drag.
pub const KEEP: Duration = Duration::from_secs(600);
/// How long to wait before trying again to remove a file that was in use.
pub const RETRY: Duration = Duration::from_secs(60);

/// `"{what} {YYYY-MM-DD} {HH.MM.SS}.png"`: no colons, which a file name cannot hold.
pub fn file_name(what: &str, at: NaiveDateTime) -> String {
    format!("{what} {}.png", at.format("%Y-%m-%d %H.%M.%S"))
}

/// Pixels ready for a PNG encoder: top-down rows without padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PngPixels {
    pub w: u32,
    pub h: u32,
    /// True: 4 bytes per pixel, BGRA. False: 3 bytes per pixel, BGR.
    pub bgra: bool,
    pub stride: u32,
    pub rows: Vec<u8>,
}

/// The pixels of a packed DIB; `None` when its header is not one read.
/// A 32 bpp DIB whose alpha bytes are all 0 comes out opaque (most clipboard
/// bitmaps carry no real alpha); real alpha is kept.
pub fn png_pixels(dib: &[u8]) -> Option<PngPixels> {
    let info = dib_info(dib)?;
    let bytes_pp = (info.bpp / 8) as usize;
    let row_len = info.w as usize * bytes_pp;
    let mut rows = Vec::with_capacity(row_len * info.h as usize);
    for y in 0..info.h as usize {
        let src = if info.top_down {
            y
        } else {
            info.h as usize - 1 - y
        };
        let at = info.offset + src * info.stride;
        rows.extend_from_slice(&dib[at..at + row_len]);
    }
    let bgra = info.bpp == 32;
    if bgra && rows.chunks_exact(4).all(|p| p[3] == 0) {
        rows.chunks_exact_mut(4).for_each(|p| p[3] = 255);
    }
    Some(PngPixels {
        w: info.w,
        h: info.h,
        bgra,
        stride: row_len as u32,
        rows,
    })
}

/// Where dragged-out files live: `%TEMP%\Vale\drag`.
pub fn root() -> PathBuf {
    std::env::temp_dir().join("Vale").join("drag")
}

/// `root\<id>\name`: one folder per card, so the file keeps its readable name.
pub fn card_path(root: &Path, id: u64, name: &str) -> PathBuf {
    root.join(id.to_string()).join(name)
}

/// Write the packed DIB `dib` as a PNG at `path`, creating its folder. False
/// on any failure, with no file left at `path`. Needs COM on this thread.
pub fn write_png(dib: &[u8], path: &Path) -> bool {
    let Some(p) = png_pixels(dib) else {
        return false;
    };
    let format = if p.bgra {
        GUID_WICPixelFormat32bppBGRA
    } else {
        GUID_WICPixelFormat24bppBGR
    };
    let Some(png) = pack::encode_png(p.w, p.h, format, p.stride, &p.rows) else {
        return false;
    };
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_err()
    {
        return false;
    }
    if std::fs::write(path, png).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(path);
    false
}

/// Remove the file and, if that leaves it empty, its card folder. True when
/// the file is gone afterwards (also when it never existed).
pub fn remove(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return false,
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_dir(dir); // only succeeds when empty
    }
    !path.exists()
}

/// Delete everything under `root`; errors are ignored.
pub fn sweep(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
}

type Entry = (u64, PathBuf, Instant);

/// The temp files made for dragged cards, each with the moment it may go.
#[derive(Default)]
pub struct DragFiles {
    files: Vec<Entry>,
}

impl DragFiles {
    /// Note a drag of card `id` whose file is at `path`, and restart its
    /// clock. True when the card already had a file (reuse that one).
    pub fn touch(&mut self, id: u64, path: PathBuf, now: Instant) -> bool {
        let due = now + KEEP;
        if let Some(f) = self.files.iter_mut().find(|f| f.0 == id) {
            f.2 = due;
            true
        } else {
            self.files.push((id, path, due));
            false
        }
    }

    pub fn path(&self, id: u64) -> Option<&Path> {
        self.files.iter().find(|f| f.0 == id).map(|f| f.1.as_path())
    }

    /// Remove and return the entries whose time has come.
    pub fn expire(&mut self, now: Instant) -> Vec<(u64, PathBuf)> {
        self.drain_where(|f| f.2 <= now)
    }

    /// Remove and return the entries of cards no longer in `live`.
    pub fn retain_ids(&mut self, live: &[u64]) -> Vec<(u64, PathBuf)> {
        self.drain_where(|f| !live.contains(&f.0))
    }

    pub fn take_all(&mut self) -> Vec<(u64, PathBuf)> {
        self.drain_where(|_| true)
    }

    /// How long until the nearest deadline; `None` when nothing is kept.
    pub fn next_due(&self, now: Instant) -> Option<Duration> {
        self.files
            .iter()
            .map(|f| f.2.saturating_duration_since(now))
            .min()
    }

    /// Keep a file that could not be removed, to try again after `RETRY`.
    pub fn retry(&mut self, id: u64, path: PathBuf, now: Instant) {
        self.files.push((id, path, now + RETRY));
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    fn drain_where(&mut self, gone: impl Fn(&Entry) -> bool) -> Vec<(u64, PathBuf)> {
        let mut out = Vec::new();
        self.files.retain(|f| {
            if gone(f) {
                out.push((f.0, f.1.clone()));
                false
            } else {
                true
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 12)
            .unwrap()
            .and_hms_opt(14, 3, 12)
            .unwrap()
    }

    /// A DIB with a 40-byte header; `px` is the pixel data as stored.
    fn dib(bpp: u16, w: i32, h: i32, compression: u32, masks: bool, px: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(40u32.to_le_bytes());
        out.extend(w.to_le_bytes());
        out.extend(h.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(bpp.to_le_bytes());
        out.extend(compression.to_le_bytes());
        out.extend((px.len() as u32).to_le_bytes());
        out.extend([0u8; 16]);
        if masks {
            out.extend(0x00FF_0000u32.to_le_bytes());
            out.extend(0x0000_FF00u32.to_le_bytes());
            out.extend(0x0000_00FFu32.to_le_bytes());
        }
        out.extend(px);
        out
    }

    /// 2x2 pixels, stored bottom row first.
    const BOTTOM: [u8; 8] = [1, 2, 3, 255, 4, 5, 6, 255];
    const TOP: [u8; 8] = [9, 10, 11, 255, 12, 13, 14, 255];

    fn px32() -> Vec<u8> {
        [BOTTOM, TOP].concat()
    }

    #[test]
    fn a_screenshot_name_reads_like_the_card() {
        let s = file_name("Screenshot", at());
        assert_eq!(s, "Screenshot 2026-11-12 14.03.12.png");
        let g = file_name("Gambar", at());
        assert!(g.starts_with("Gambar 2026-11-12"));
        assert!(!s.contains(':') && !g.contains(':'));
    }

    #[test]
    fn a_bottom_up_32_bit_dib_comes_out_top_down() {
        let p = png_pixels(&dib(32, 2, 2, 0, false, &px32())).unwrap();
        assert_eq!(&p.rows[..8], &TOP);
        assert_eq!(&p.rows[8..], &BOTTOM);
        assert!(p.bgra);
        assert_eq!((p.w, p.h, p.stride), (2, 2, 8));
    }

    #[test]
    fn alpha_all_zero_comes_out_opaque() {
        let mut px = px32();
        px.chunks_exact_mut(4).for_each(|p| p[3] = 0);
        let p = png_pixels(&dib(32, 2, 2, 0, false, &px)).unwrap();
        assert!(p.rows.chunks_exact(4).all(|p| p[3] == 255));
        assert_eq!(&p.rows[..3], &TOP[..3]);
    }

    #[test]
    fn real_alpha_is_kept() {
        let mut px = px32();
        px[7] = 128; // the bottom row's second pixel
        let p = png_pixels(&dib(32, 2, 2, 0, false, &px)).unwrap();
        // The bottom row is the output's second row.
        assert_eq!(p.rows[8 + 7], 128);
        assert_eq!(p.rows[3], 255);
        assert_eq!(p.rows[8 + 3], 255);
    }

    #[test]
    fn a_24_bit_dib_loses_its_row_padding() {
        // 3 px * 3 bytes = 9, padded to a stride of 12.
        let mut px = Vec::new();
        for r in 0..2u8 {
            px.extend((0..9).map(|i| r * 10 + i));
            px.extend([0xEE; 3]);
        }
        let p = png_pixels(&dib(24, 3, 2, 0, false, &px)).unwrap();
        assert!(!p.bgra);
        assert_eq!(p.stride, 9);
        assert_eq!(p.rows.len(), 18);
        assert_eq!(&p.rows[..9], &(10..19).collect::<Vec<u8>>()[..]);
        assert!(!p.rows.contains(&0xEE));
    }

    #[test]
    fn a_bitfields_dib_reads_past_its_masks() {
        let plain = png_pixels(&dib(32, 2, 2, 0, false, &px32())).unwrap();
        let fields = png_pixels(&dib(32, 2, 2, 3, true, &px32())).unwrap();
        assert_eq!(plain, fields);
    }

    #[test]
    fn garbage_gives_none() {
        assert_eq!(png_pixels(&[1, 2, 3]), None);
    }

    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, IWICImagingFactory, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };

    /// COM for this test thread.
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

    /// A scratch folder under `target/test-tmp`, emptied when dropped.
    struct Tmp(PathBuf);
    impl Tmp {
        fn new(test: &str) -> Tmp {
            let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("test-tmp")
                .join(format!("clip-drag-{test}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn decoded_size(path: &Path) -> (u32, u32) {
        let bytes = std::fs::read(path).unwrap();
        unsafe {
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).unwrap();
            let stream = wic.CreateStream().unwrap();
            stream.InitializeFromMemory(&bytes).unwrap();
            let decoder = wic
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .unwrap();
            let frame = decoder.GetFrame(0).unwrap();
            let (mut w, mut h) = (0, 0);
            frame.GetSize(&mut w, &mut h).unwrap();
            (w, h)
        }
    }

    #[test]
    fn a_written_png_decodes_to_the_same_size() {
        let _com = Com::new();
        let t = Tmp::new("size");
        let p = t.0.join("a.png");
        assert!(write_png(&dib(32, 3, 2, 0, false, &[7; 24]), &p));
        assert_eq!(&std::fs::read(&p).unwrap()[..4], &[0x89, 0x50, 0x4E, 0x47]);
        assert_eq!(decoded_size(&p), (3, 2));
    }

    #[test]
    fn a_24_bit_dib_writes_too() {
        let _com = Com::new();
        let t = Tmp::new("bit24");
        let p = t.0.join("a.png");
        assert!(write_png(&dib(24, 3, 2, 0, false, &[5; 24]), &p));
        assert_eq!(decoded_size(&p), (3, 2));
    }

    #[test]
    fn a_bad_dib_writes_nothing() {
        let _com = Com::new();
        let t = Tmp::new("bad");
        let p = t.0.join("a.png");
        assert!(!write_png(&[0; 8], &p));
        assert!(!p.exists());
    }

    #[test]
    fn an_unwritable_place_writes_nothing() {
        let _com = Com::new();
        let t = Tmp::new("unwritable");
        let file = t.0.join("plain");
        std::fs::write(&file, b"x").unwrap();
        assert!(!write_png(&dib(32, 3, 2, 0, false, &[7; 24]), &file.join("a.png")));
    }

    #[test]
    fn remove_takes_the_card_folder_with_it() {
        let _com = Com::new();
        let t = Tmp::new("remove");
        let p = card_path(&t.0, 9, "a.png");
        assert!(write_png(&dib(32, 3, 2, 0, false, &[7; 24]), &p));
        assert!(remove(&p));
        assert!(!t.0.join("9").exists());
        assert!(remove(&p));
    }

    #[test]
    fn the_root_is_vale_drag_under_temp() {
        // Only the path is computed; nothing is created.
        let r = root();
        assert!(r.ends_with(Path::new("Vale").join("drag")));
        assert!(r.starts_with(std::env::temp_dir()));
    }

    #[test]
    fn sweep_clears_the_root() {
        let _com = Com::new();
        let t = Tmp::new("sweep");
        let root = t.0.join("drag");
        for id in [1, 2] {
            let p = card_path(&root, id, "a.png");
            assert!(write_png(&dib(32, 3, 2, 0, false, &[7; 24]), &p));
        }
        sweep(&root);
        assert!(!root.exists());
    }

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn a_second_drag_reuses_the_file_and_restarts_the_clock() {
        let t0 = Instant::now();
        let p = PathBuf::from("a.png");
        let mut d = DragFiles::default();
        assert!(!d.touch(7, p.clone(), t0));
        assert!(d.touch(7, p.clone(), t0 + 5 * MIN));
        assert!(d.expire(t0 + 11 * MIN).is_empty());
        assert_eq!(d.expire(t0 + 16 * MIN), vec![(7, p)]);
        assert!(d.is_empty());
    }

    #[test]
    fn next_due_is_the_nearest_deadline() {
        let t0 = Instant::now();
        let mut d = DragFiles::default();
        assert_eq!(d.next_due(t0), None);
        d.touch(1, "a.png".into(), t0);
        d.touch(2, "b.png".into(), t0 + 2 * MIN);
        assert_eq!(d.next_due(t0 + MIN), Some(9 * MIN));
    }

    #[test]
    fn a_deleted_card_gives_up_its_file() {
        let t0 = Instant::now();
        let mut d = DragFiles::default();
        for id in 1..=3 {
            d.touch(id, format!("{id}.png").into(), t0);
        }
        let gone = d.retain_ids(&[2]);
        assert_eq!(gone.iter().map(|g| g.0).collect::<Vec<_>>(), vec![1, 3]);
        assert_eq!(d.path(2), Some(Path::new("2.png")));
        assert_eq!(d.path(1), None);
        assert_eq!(d.take_all().len(), 1);
    }

    #[test]
    fn a_failed_removal_is_retried() {
        let t0 = Instant::now();
        let mut d = DragFiles::default();
        d.touch(5, "5.png".into(), t0);
        let later = t0 + 11 * MIN;
        let (id, p) = d.expire(later).pop().unwrap();
        d.retry(id, p, later);
        assert_eq!(d.next_due(later), Some(RETRY));
    }
}
