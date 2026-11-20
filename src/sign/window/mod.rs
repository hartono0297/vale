//! The signing window: one top-level window per document, opened from the
//! shelf. `window` holds the state and the messages, `layout` where things
//! sit (pure), `paint` the drawing, `maker` the signature maker popover and
//! `text_edit` the native EDITs.
//!
//! Nothing runs while no window is open: each window owns its renderer and
//! its opening, Office and save threads, and they end with it.

mod layout;
mod maker;
mod paint;
mod text_edit;
mod window;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use crate::sign::store::{self, Saved};
use crate::sign::{Mark, Source};

/// Posted to the owner when a report is waiting (`take_reports`).
pub const WM_APP_SIGN: u32 = WM_APP + 96;

#[derive(Debug, PartialEq, Eq)]
pub enum Report {
    /// A signed copy was written here.
    Saved(PathBuf),
}

static REPORTS: Mutex<Vec<Report>> = Mutex::new(Vec::new());

/// Queues `r` for the owner and tells it.
fn report(owner: HWND, r: Report) {
    REPORTS.lock().unwrap_or_else(|e| e.into_inner()).push(r);
    if !owner.is_invalid() {
        unsafe {
            let _ = PostMessageW(Some(owner), WM_APP_SIGN, WPARAM(0), LPARAM(0));
        }
    }
}

/// Opens a signing window for `src` (or brings the one already showing it to
/// the front). `demo` keeps saved signatures in memory only and writes under
/// `target/demo-sign`. False when the window could not be made.
pub fn open(owner: HWND, src: Source, demo: bool) -> bool {
    window::open(owner, src, demo)
}

/// The app is ending: every signing window is destroyed and its threads
/// joined.
pub fn release() {
    window::release();
}

/// Re-reads the window palette (light or dark, accent) for open windows.
pub fn restyle_open() {
    window::restyle_open();
}

pub fn take_reports() -> Vec<Report> {
    std::mem::take(&mut *REPORTS.lock().unwrap_or_else(|e| e.into_inner()))
}

// ---- saved signatures ----

thread_local! {
    /// The saved signatures and initials of this UI thread's windows, and
    /// whether they came from disk (a demo never reads or writes the file).
    static STORE: RefCell<Option<(bool, Saved)>> = const { RefCell::new(None) };
}

fn with_store<R>(demo: bool, f: impl FnOnce(&mut Saved) -> R) -> R {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        let disk = !demo;
        if s.as_ref().is_none_or(|(d, _)| *d != disk) {
            let saved = if disk { store::load_from(&store::path()) } else { Saved::default() };
            *s = Some((disk, saved));
        }
        let (_, saved) = s.as_mut().expect("filled above");
        f(saved)
    })
}

fn persist(demo: bool) {
    if demo {
        return;
    }
    STORE.with(|s| {
        if let Some((true, saved)) = s.borrow().as_ref()
            && let Err(e) = store::save_to(&store::path(), saved)
        {
            eprintln!("vale: the saved signatures could not be written: {e}");
        }
    });
}

/// The saved signatures (or initials), newest first.
fn saved_marks(initials: bool, demo: bool) -> Vec<Arc<Mark>> {
    with_store(demo, |s| {
        let list = if initials { &s.initials } else { &s.signatures };
        list.iter().cloned().map(Arc::new).collect()
    })
}

fn keep_mark(m: &Mark, initials: bool, demo: bool) {
    with_store(demo, |s| store::add(s, m.clone(), initials));
    persist(demo);
}

fn forget_mark(initials: bool, i: usize, demo: bool) {
    with_store(demo, |s| store::remove(s, initials, i));
    persist(demo);
}

// ---- demo ----

/// A made-up three-page document: a heading, grey lines for text, and two
/// signature boxes on the last page. No user data.
#[cfg(any(test, debug_assertions))]
pub(crate) fn demo_pdf() -> Option<Vec<u8>> {
    use lopdf::{Dictionary, Document, Object, Stream, dictionary};
    let (w, h) = (595.0f32, 842.0f32);
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let bold = doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica-Bold"});
    let font = doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let res = doc.add_object(dictionary! {"Font"=>dictionary!{"F1"=>bold,"F2"=>font}});
    let mut kids = Vec::new();
    for n in 0..3 {
        let mut c = String::new();
        c.push_str(&format!("BT /F1 16 Tf {} 770 Td (Contoh dokumen) Tj ET\n", w / 2.0 - 62.0));
        c.push_str(&format!("BT /F2 10 Tf {} 750 Td (halaman {} dari 3) Tj ET\n", w / 2.0 - 34.0, n + 1));
        c.push_str("0.89 0.87 0.83 rg\n");
        let lines = if n == 2 { 9 } else { 26 };
        let mut y = 712.0;
        for i in 0..lines {
            let len = if i % 5 == 4 { 0.62 } else if i % 7 == 3 { 0.86 } else { 1.0 };
            c.push_str(&format!("72 {y} {} 6 re f\n", (w - 144.0) * len));
            y -= if i % 9 == 8 { 30.0 } else { 20.0 };
        }
        if n == 2 {
            c.push_str("0.33 0.33 0.33 rg BT /F2 10 Tf 72 460 Td (Pihak pertama) Tj ET\n");
            c.push_str("BT /F2 10 Tf 330 460 Td (Pihak kedua) Tj ET\n");
            c.push_str("0.6 0.6 0.6 RG 0.8 w 72 380 m 265 380 l S 330 380 m 523 380 l S\n");
            c.push_str("0.33 0.33 0.33 rg BT /F2 10 Tf 72 366 Td (\\(nama\\)) Tj ET\n");
            c.push_str("BT /F2 10 Tf 330 366 Td (\\(nama\\)) Tj ET\n");
        }
        let cid = doc.add_object(Stream::new(Dictionary::new(), c.into_bytes()));
        let page = dictionary! {"Type"=>"Page","Parent"=>pages_id,"Contents"=>cid,"Resources"=>res};
        kids.push(Object::Reference(doc.add_object(page)));
    }
    let tree = dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>3,
        "MediaBox"=>vec![0.into(),0.into(),Object::Real(w),Object::Real(h)]};
    doc.objects.insert(pages_id, Object::Dictionary(tree));
    let cat = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages_id});
    doc.trailer.set("Root", cat);
    let mut out = Vec::new();
    doc.save_to(&mut out).ok()?;
    Some(out)
}

/// `--demo-sign`: the window on `demo_pdf`, written to a fresh
/// `office::job_dir()`. Saved signatures stay in memory; a save goes to
/// `target/demo-sign/`. Runs its own message loop; the process's loop ends
/// when the window closes. `DI_WINDOW`, `DI_SURFACE` and `DI_ACCENT` pick
/// the theme.
#[cfg(debug_assertions)]
pub fn run_demo() {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG, TranslateMessage};

    crate::theme::set_system_light(crate::theme::read_system_light());
    crate::theme::set_current(crate::theme::theme_from_env());
    // The file dialog, WIC and the signature store need COM on this thread.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let dir = crate::sign::office::job_dir();
    let path = dir.as_ref().ok().map(|d| d.join("Contoh dokumen.pdf"));
    let written = match (&path, demo_pdf()) {
        (Some(p), Some(bytes)) => std::fs::write(p, bytes).is_ok(),
        _ => false,
    };
    if written && let Some(p) = path {
        let src = Source { path: p, kind: crate::sign::Kind::Pdf };
        if open(HWND::default(), src, true) {
            let mut msg = MSG::default();
            unsafe {
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    } else {
        eprintln!("vale: the demo document could not be written");
    }
    release();
    if let Ok(d) = dir {
        crate::sign::office::sweep(&d);
    }
    if com {
        unsafe { CoUninitialize() };
    }
    end_process();
}

/// Ends the demo process without running DLL teardown: on some GPU drivers
/// a process that made a Direct2D device and then used Windows.Data.Pdf
/// fails fast while the driver unloads (the app ends the same way).
#[cfg(debug_assertions)]
fn end_process() -> ! {
    use std::io::Write;
    use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_document_has_three_pages() {
        let bytes = demo_pdf().expect("made");
        let o = crate::sign::pdf_write::open(bytes, None).expect("opens");
        assert_eq!(o.geoms.len(), 3);
        assert_eq!((o.geoms[0].w, o.geoms[0].h), (595.0, 842.0));
        assert!(!o.encrypted && !o.signed && !o.no_changes);
    }

    #[test]
    fn reports_are_taken_once() {
        report(HWND::default(), Report::Saved(PathBuf::from(r"C:\x (ditandatangani).pdf")));
        let got = take_reports();
        assert!(got.contains(&Report::Saved(PathBuf::from(r"C:\x (ditandatangani).pdf"))));
        assert!(!take_reports().contains(&Report::Saved(PathBuf::from(r"C:\x (ditandatangani).pdf"))));
    }

    #[test]
    fn a_demo_store_lives_in_memory() {
        let m = Mark { w: 1, h: 1, rgba: vec![0, 0, 0, 255] };
        keep_mark(&m, false, true);
        assert_eq!(saved_marks(false, true).len(), 1);
        forget_mark(false, 0, true);
        assert!(saved_marks(false, true).is_empty());
    }
}
