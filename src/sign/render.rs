//! The page renderer of the signing window: one worker thread turns PDF pages
//! (Windows.Data.Pdf) and images (WIC) into premultiplied BGRA bitmaps, so the
//! UI thread never waits on WinRT. The thread exists only while a `Renderer`
//! does (drop stops and joins it).

use std::collections::VecDeque;
use std::fs::File;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
    WICDecodeMetadataCacheOnDemand, WICRect,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows::core::{HSTRING, Interface};

use crate::phone::thumb::{MAX_BYTES, oriented_source};

/// The most pixels an image may declare (WIC decodes them before scaling).
const MAX_PIXELS: u64 = 250_000_000;

/// The most pixels one rendered page may have, so a huge width request cannot
/// ask for gigabytes.
const MAX_PAGE_PIXELS: u64 = 100_000_000;

/// A rendered bitmap: premultiplied BGRA, top-down, ready for
/// `shot_edit::gfx::bitmap`.
pub struct Pixels {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
}

/// What to render. `gen` (a raw identifier, `r#gen`, in edition 2024) lets the window drop answers for a stale zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Page { page: u32, width_px: u32, r#gen: u64 },
    Thumb { page: u32, width_px: u32 },
}

pub enum Done {
    Page { page: u32, r#gen: u64, px: Pixels },
    Thumb { page: u32, px: Pixels },
    Failed { page: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderOpenError {
    /// Needs a password, or the one given is wrong.
    Password,
    /// Damaged, empty or not a document.
    Broken,
    /// Something else stopped the load.
    Unsupported,
}

#[derive(Default)]
struct Queue {
    pages: VecDeque<Job>,
    thumbs: VecDeque<Job>,
    done: Vec<Done>,
    stop: bool,
    paused: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct Renderer {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    pages: Vec<(f32, f32)>,
    #[cfg(test)]
    alive: Arc<AtomicUsize>,
}

type Opened = Result<Vec<(f32, f32)>, RenderOpenError>;
type Loaded = Result<(Source, Vec<(f32, f32)>), RenderOpenError>;

impl Renderer {
    /// Loads `bytes` on the worker and returns once the load has a result.
    pub fn start_pdf(
        bytes: Vec<u8>,
        password: Option<String>,
        notify: impl Fn() + Send + 'static,
    ) -> Result<Renderer, RenderOpenError> {
        Self::start(notify, move || {
            let doc = load_pdf(&bytes, password.as_deref())?;
            let sizes = page_sizes(&doc).ok_or(RenderOpenError::Broken)?;
            Ok((Source::Pdf(doc), sizes))
        })
    }

    /// Decodes the image at `path` on the worker.
    pub fn start_image(
        path: PathBuf,
        notify: impl Fn() + Send + 'static,
    ) -> Result<Renderer, RenderOpenError> {
        Self::start(notify, move || {
            let px = decode_image(&path)?;
            let size = (px.w as f32, px.h as f32);
            Ok((Source::Image(px), vec![size]))
        })
    }

    fn start(
        notify: impl Fn() + Send + 'static,
        open: impl FnOnce() -> Loaded + Send + 'static,
    ) -> Result<Renderer, RenderOpenError> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
        });
        let alive = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = sync_channel::<Opened>(0);
        let (worker_shared, worker_alive) = (shared.clone(), alive.clone());
        let thread = std::thread::Builder::new()
            .name("sign-render".into())
            .spawn(move || {
                worker_alive.fetch_add(1, Ordering::SeqCst);
                let _count = CountGuard(worker_alive);
                // One init for the thread (RoInitialize also sets up COM in the
                // MTA), undone only if it succeeded. `work` owns every WinRT and
                // COM object and drops them before the guard runs.
                let _apartment = Apartment::enter();
                work(&worker_shared, open, tx, &notify);
            })
            .map_err(|_| RenderOpenError::Unsupported)?;
        let mut renderer = Renderer {
            shared,
            thread: Some(thread),
            pages: Vec::new(),
            #[cfg(test)]
            alive,
        };
        match rx.recv() {
            Ok(Ok(pages)) => {
                renderer.pages = pages;
                Ok(renderer)
            }
            // Dropping `renderer` stops and joins the thread.
            Ok(Err(e)) => Err(e),
            Err(_) => Err(RenderOpenError::Unsupported),
        }
    }

    pub fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }

    /// The page size in 96-DPI DIPs, already rotated; (0, 0) out of range.
    pub fn page_size_dip(&self, page: u32) -> (f32, f32) {
        self.pages.get(page as usize).copied().unwrap_or((0.0, 0.0))
    }

    /// Queues a job. A newer `Page` job for a page replaces its queued older
    /// one (a `Thumb` likewise); pages are served before thumbs.
    pub fn request(&self, job: Job) {
        {
            let mut q = self.shared.lock();
            match job {
                Job::Page { page, .. } => {
                    q.pages
                        .retain(|j| !matches!(j, Job::Page { page: p, .. } if *p == page));
                    q.pages.push_back(job);
                }
                Job::Thumb { page, .. } => {
                    q.thumbs
                        .retain(|j| !matches!(j, Job::Thumb { page: p, .. } if *p == page));
                    q.thumbs.push_back(job);
                }
            }
        }
        self.shared.wake.notify_all();
    }

    /// Everything finished since the last call.
    pub fn take(&self) -> Vec<Done> {
        std::mem::take(&mut self.shared.lock().done)
    }

    #[cfg(test)]
    fn set_paused(&self, paused: bool) {
        self.shared.lock().paused = paused;
        self.shared.wake.notify_all();
    }

    #[cfg(test)]
    fn alive(&self) -> Arc<AtomicUsize> {
        self.alive.clone()
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Apartment(bool);

impl Apartment {
    fn enter() -> Apartment {
        Apartment(unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { RoUninitialize() };
        }
    }
}

struct CountGuard(Arc<AtomicUsize>);

impl Drop for CountGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

enum Source {
    Pdf(PdfDocument),
    Image(Pixels),
}

fn work(
    shared: &Shared,
    open: impl FnOnce() -> Loaded,
    tx: SyncSender<Opened>,
    notify: &dyn Fn(),
) {
    let source = match open() {
        Ok((source, sizes)) => {
            if tx.send(Ok(sizes)).is_err() {
                return;
            }
            source
        }
        Err(e) => {
            let _ = tx.send(Err(e));
            return;
        }
    };
    loop {
        let job = {
            let mut q = shared.lock();
            loop {
                if q.stop {
                    return;
                }
                if !q.paused {
                    // The newest page job is the one the view wants most.
                    if let Some(job) = q.pages.pop_back().or_else(|| q.thumbs.pop_front()) {
                        break job;
                    }
                }
                q = shared.wake.wait(q).unwrap_or_else(|e| e.into_inner());
            }
        };
        let (page, width_px) = match job {
            Job::Page { page, width_px, .. } | Job::Thumb { page, width_px } => (page, width_px),
        };
        let rendered = render(&source, page, width_px);
        {
            let mut q = shared.lock();
            if q.stop {
                return;
            }
            q.done.push(match (rendered, job) {
                (Some(px), Job::Page { r#gen, .. }) => Done::Page { page, r#gen, px },
                (Some(px), Job::Thumb { .. }) => Done::Thumb { page, px },
                (None, _) => Done::Failed { page },
            });
        }
        notify();
    }
}

fn render(source: &Source, page: u32, width_px: u32) -> Option<Pixels> {
    match source {
        Source::Pdf(doc) => render_pdf(doc, page, width_px),
        Source::Image(full) => (page == 0).then(|| scale_image(full, width_px)).flatten(),
    }
}

fn code_to_error(e: &windows::core::Error) -> RenderOpenError {
    match e.code().0 as u32 {
        0x8007_052B => RenderOpenError::Password,
        0x8004_8018 | 0x8004_808E => RenderOpenError::Broken,
        _ => RenderOpenError::Unsupported,
    }
}

fn load_pdf(bytes: &[u8], password: Option<&str>) -> Result<PdfDocument, RenderOpenError> {
    let load = || -> windows::core::Result<PdfDocument> {
        let ms = InMemoryRandomAccessStream::new()?;
        let w = DataWriter::CreateDataWriter(&ms)?;
        w.WriteBytes(bytes)?;
        w.StoreAsync()?.join()?;
        w.FlushAsync()?.join()?;
        w.DetachStream()?;
        ms.Seek(0)?;
        match password {
            Some(p) => PdfDocument::LoadFromStreamWithPasswordAsync(&ms, &HSTRING::from(p))?.join(),
            None => PdfDocument::LoadFromStreamAsync(&ms)?.join(),
        }
    };
    load().map_err(|e| code_to_error(&e))
}

fn page_sizes(doc: &PdfDocument) -> Option<Vec<(f32, f32)>> {
    let count = doc.PageCount().ok()?;
    if count == 0 {
        return None;
    }
    (0..count)
        .map(|i| {
            let size = doc.GetPage(i).ok()?.Size().ok()?;
            Some((size.Width, size.Height))
        })
        .collect()
}

fn render_pdf(doc: &PdfDocument, page: u32, width_px: u32) -> Option<Pixels> {
    let page = doc.GetPage(page).ok()?;
    let size = page.Size().ok()?;
    if width_px == 0 || size.Width <= 0.0 || size.Height <= 0.0 {
        return None;
    }
    let height = (f64::from(width_px) * f64::from(size.Height) / f64::from(size.Width)).ceil();
    if f64::from(width_px) * height > MAX_PAGE_PIXELS as f64 {
        return None;
    }
    let opts = PdfPageRenderOptions::new().ok()?;
    opts.SetDestinationWidth(width_px).ok()?;
    let out = InMemoryRandomAccessStream::new().ok()?;
    page.RenderWithOptionsToStreamAsync(&out, &opts)
        .ok()?
        .join()
        .ok()?;
    let len = u32::try_from(out.Size().ok()?).ok()?;
    let reader = DataReader::CreateDataReader(&out.GetInputStreamAt(0).ok()?).ok()?;
    reader.LoadAsync(len).ok()?.join().ok()?;
    let mut png = vec![0u8; len as usize];
    reader.ReadBytes(&mut png).ok()?;
    let mut px = decode_png(&png)?;
    over_white(&mut px.bgra);
    Some(px)
}

/// Premultiplied pixels laid over white, so a page drawn on a transparent
/// background shows as paper.
fn over_white(bgra: &mut [u8]) {
    for p in bgra.chunks_exact_mut(4) {
        let miss = 255 - p[3];
        if miss != 0 {
            p[0] = p[0].saturating_add(miss);
            p[1] = p[1].saturating_add(miss);
            p[2] = p[2].saturating_add(miss);
            p[3] = 255;
        }
    }
}

fn wic() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() }
}

fn decode_png(png: &[u8]) -> Option<Pixels> {
    unsafe {
        let wic = wic()?;
        let stream = SHCreateMemStream(Some(png))?;
        let decoder = wic
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let source: IWICBitmapSource = frame.cast().ok()?;
        copy_pixels(&wic, &source)
    }
}

/// `source` converted to premultiplied BGRA, at its own size.
unsafe fn copy_pixels(wic: &IWICImagingFactory, source: &IWICBitmapSource) -> Option<Pixels> {
    unsafe {
        let (mut w, mut h) = (0u32, 0u32);
        source.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
            return None;
        }
        let converter = wic.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                source,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let stride = w.checked_mul(4)?;
        let mut bgra = vec![0u8; usize::try_from(stride).ok()?.checked_mul(h as usize)?];
        let rect = WICRect {
            X: 0,
            Y: 0,
            Width: i32::try_from(w).ok()?,
            Height: i32::try_from(h).ok()?,
        };
        converter.CopyPixels(&rect, stride, &mut bgra).ok()?;
        Some(Pixels { w, h, bgra })
    }
}

fn decode_image(path: &std::path::Path) -> Result<Pixels, RenderOpenError> {
    let file: File = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(path)
        .map_err(|_| RenderOpenError::Broken)?;
    let meta = file.metadata().map_err(|_| RenderOpenError::Broken)?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_BYTES {
        return Err(RenderOpenError::Broken);
    }
    unsafe {
        let wic = wic().ok_or(RenderOpenError::Unsupported)?;
        let source = oriented_source(&wic, &file).ok_or(RenderOpenError::Broken)?;
        copy_pixels(&wic, &source).ok_or(RenderOpenError::Unsupported)
    }
}

/// `full` scaled to `width_px` wide (never enlarged).
fn scale_image(full: &Pixels, width_px: u32) -> Option<Pixels> {
    if width_px == 0 {
        return None;
    }
    if width_px >= full.w {
        return Some(Pixels {
            w: full.w,
            h: full.h,
            bgra: full.bgra.clone(),
        });
    }
    let h = ((u64::from(full.h) * u64::from(width_px) + u64::from(full.w) / 2)
        / u64::from(full.w))
    .max(1) as u32;
    unsafe {
        let wic = wic()?;
        let bitmap = wic
            .CreateBitmapFromMemory(
                full.w,
                full.h,
                &GUID_WICPixelFormat32bppPBGRA,
                full.w.checked_mul(4)?,
                &full.bgra,
            )
            .ok()?;
        let scaler = wic.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&bitmap, width_px, h, WICBitmapInterpolationModeFant)
            .ok()?;
        let source: IWICBitmapSource = scaler.cast().ok()?;
        copy_pixels(&wic, &source)
    }
}

/// Test processes only: renders one PDF page before `main`, so Windows.Data.Pdf
/// creates its Direct3D device before any test builds a Direct2D target. The
/// other order (a Direct2D WIC target first, a PDF page afterwards, from any
/// module) makes the AMD user-mode driver (amdxx64.dll) fail fast with
/// 0xC0000409 while the process exits, after every test has passed.
#[cfg(test)]
#[used]
#[unsafe(link_section = ".CRT$XCU")]
static PDF_DEVICE_FIRST: extern "C" fn() = {
    extern "C" fn init() {
        if let Ok(r) = Renderer::start_pdf(tests::pdf_bytes(1, None), None, || {}) {
            r.request(Job::Page { page: 0, width_px: 64, r#gen: 0 });
            let start = std::time::Instant::now();
            while r.take().is_empty() && start.elapsed() < std::time::Duration::from_secs(10) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
    init
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::files::tests::temp_dir;
    use crate::phone::thumb::tests::write_gradient_png;
    use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
    use lopdf::{Dictionary, Document, EncryptionState, EncryptionVersion, Object, Permissions, Stream};
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    fn dict(entries: Vec<(&str, Object)>) -> Dictionary {
        let mut d = Dictionary::new();
        for (k, v) in entries {
            d.set(k, v);
        }
        d
    }

    /// A Letter-size PDF whose pages each draw a black square at (100, 100),
    /// 200 points wide.
    pub(super) fn pdf_bytes(pages: usize, password: Option<&str>) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();
        for _ in 0..pages {
            let content = doc.add_object(Stream::new(
                Dictionary::new(),
                b"0 0 0 rg 100 100 200 200 re f".to_vec(),
            ));
            let page = doc.add_object(dict(vec![
                ("Type", "Page".into()),
                ("Parent", pages_id.into()),
                ("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()].into()),
                ("Contents", content.into()),
            ]));
            kids.push(Object::Reference(page));
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dict(vec![
                ("Type", "Pages".into()),
                ("Kids", kids.into()),
                ("Count", (pages as i64).into()),
            ])),
        );
        let catalog = doc.add_object(dict(vec![
            ("Type", "Catalog".into()),
            ("Pages", pages_id.into()),
        ]));
        doc.trailer.set("Root", Object::Reference(catalog));
        doc.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal(vec![1u8; 16]),
                Object::string_literal(vec![2u8; 16]),
            ]),
        );
        if let Some(pw) = password {
            let filter: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
            let state = EncryptionState::try_from(EncryptionVersion::V4 {
                document: &doc,
                encrypt_metadata: true,
                crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
                stream_filter: b"StdCF".to_vec(),
                string_filter: b"StdCF".to_vec(),
                owner_password: "owner",
                user_password: pw,
                permissions: Permissions::all(),
            })
            .expect("encryption state");
            doc.encrypt(&state).expect("encrypt");
        }
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("save");
        out
    }

    fn wait_for(r: &Renderer, count: usize) -> Vec<Done> {
        let end = Instant::now() + Duration::from_secs(10);
        let mut got = Vec::new();
        while got.len() < count && Instant::now() < end {
            got.extend(r.take());
            std::thread::sleep(Duration::from_millis(10));
        }
        got
    }

    #[test]
    fn renders_generated_pdf_page() {
        let r = Renderer::start_pdf(pdf_bytes(2, None), None, || {}).expect("opens");
        assert_eq!(r.page_count(), 2);
        let (w, h) = r.page_size_dip(0);
        assert!((w - 816.0).abs() < 1.0 && (h - 1056.0).abs() < 1.0, "{w} x {h}");
        r.request(Job::Page { page: 0, width_px: 400, r#gen: 7 });
        let done = wait_for(&r, 1);
        let Some(Done::Page { page: 0, r#gen: 7, px }) = done.into_iter().next() else {
            panic!("no page came back");
        };
        assert_eq!(px.w, 400);
        assert!(px.h.abs_diff(517) <= 2, "h = {}", px.h);
        assert_eq!(px.bgra.len(), (px.w * px.h * 4) as usize);
        // The square spans x 100..300, y 100..300 from the bottom of a
        // 612 x 792 page: its centre is black, the corner is paper.
        let at = |x: u32, y: u32| &px.bgra[((y * px.w + x) * 4) as usize..][..4];
        let (cx, cy) = (200 * 400 / 612, (792 - 200) * px.h / 792);
        assert!(at(cx, cy)[..3].iter().all(|&c| c < 40), "{:?}", at(cx, cy));
        assert_eq!(at(2, 2), [255, 255, 255, 255]);
    }

    #[test]
    fn a_thumb_comes_after_a_page() {
        let r = Renderer::start_pdf(pdf_bytes(2, None), None, || {}).expect("opens");
        r.set_paused(true);
        r.request(Job::Thumb { page: 1, width_px: 96 });
        r.request(Job::Page { page: 0, width_px: 200, r#gen: 1 });
        r.set_paused(false);
        let done = wait_for(&r, 2);
        assert!(matches!(done.first(), Some(Done::Page { page: 0, .. })));
        assert!(matches!(done.get(1), Some(Done::Thumb { page: 1, px }) if px.w == 96));
    }

    #[test]
    fn password_pdf_reports_password() {
        let bytes = pdf_bytes(1, Some("rahasia"));
        assert_eq!(
            Renderer::start_pdf(bytes.clone(), None, || {}).err(),
            Some(RenderOpenError::Password)
        );
        assert_eq!(
            Renderer::start_pdf(bytes.clone(), Some("salah".into()), || {}).err(),
            Some(RenderOpenError::Password)
        );
        let r = Renderer::start_pdf(bytes, Some("rahasia".into()), || {}).expect("opens");
        assert_eq!(r.page_count(), 1);
    }

    #[test]
    fn garbage_is_broken() {
        assert_eq!(
            Renderer::start_pdf(b"%PDF-1.4 nonsense".to_vec(), None, || {}).err(),
            Some(RenderOpenError::Broken)
        );
        assert_eq!(
            Renderer::start_pdf(Vec::new(), None, || {}).err(),
            Some(RenderOpenError::Broken)
        );
    }

    #[test]
    fn newer_page_job_replaces_older() {
        let r = Renderer::start_pdf(pdf_bytes(1, None), None, || {}).expect("opens");
        r.set_paused(true);
        r.request(Job::Page { page: 0, width_px: 300, r#gen: 1 });
        r.request(Job::Page { page: 0, width_px: 300, r#gen: 2 });
        r.set_paused(false);
        let mut done = wait_for(&r, 1);
        std::thread::sleep(Duration::from_millis(300));
        done.extend(r.take());
        assert_eq!(done.len(), 1);
        assert!(matches!(done[0], Done::Page { r#gen: 2, .. }));
    }

    #[test]
    fn drop_joins_thread() {
        let r = Renderer::start_pdf(pdf_bytes(1, None), None, || {}).expect("opens");
        let alive = r.alive();
        assert_eq!(alive.load(Ordering::SeqCst), 1);
        drop(r);
        assert_eq!(alive.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn an_image_is_decoded_once_and_scaled_per_job() {
        let dir = temp_dir("sign-render-image");
        let path = dir.join("foto.png");
        write_gradient_png(&path, 640, 480);
        let r = Renderer::start_image(path, || {}).expect("opens");
        assert_eq!(r.page_count(), 1);
        assert_eq!(r.page_size_dip(0), (640.0, 480.0));
        r.request(Job::Page { page: 0, width_px: 320, r#gen: 1 });
        let Some(Done::Page { px, .. }) = wait_for(&r, 1).into_iter().next() else {
            panic!("no page came back");
        };
        assert_eq!((px.w, px.h), (320, 240));
        let bad = dir.join("x.png");
        std::fs::write(&bad, b"not an image").expect("write");
        assert!(Renderer::start_image(bad, || {}).is_err());
        drop(r);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
