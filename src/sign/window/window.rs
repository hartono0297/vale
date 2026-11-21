//! The signing window's Win32 side: its state, messages, mouse and keys, and
//! the workers that open and save. Where things sit is `layout` (pure), how
//! they look is `paint`.
//!
//! Threads: `sign-open` (reads the file, asks lopdf, starts the renderer),
//! `sign-office` (converts an Office file first), `sign-save`, and the
//! renderer's own `sign-render`. All end with the window: `Win::drop` sets
//! the cancel flag and joins them. The one timer is the re-render debounce
//! after a resize, one-shot, on this window.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow,
    ScreenToClient, ValidateRect,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN,
    VK_ESCAPE, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetCursorPos,
    IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENWSE, IsWindow,
    KillTimer, LoadCursorW, MINMAXINFO, PostMessageW, PostQuitMessage, RegisterClassExW, SW_HIDE,
    SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, SetCursor, SetForegroundWindow, SetTimer, SetWindowPos,
    ShowWindow, WINDOW_EX_STYLE, WM_APP, WM_CAPTURECHANGED, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLOREDIT, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT,
    WM_SETCURSOR, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW,
};
use windows::core::{Error, HSTRING, PCWSTR, w};

use super::layout::{
    self, Corner, Footer, Hit, Labels, Layout, Popover, RailRow, Scene, Spec, StageMode, Tool,
};
use super::text_edit::{self, EDIT_CANCEL, EDIT_COMMIT, EDIT_TAB, Field, WM_SIGN_EDIT};
use super::{Report, maker, paint};
use crate::i18n::{Key, Lang, lang, tr};
use crate::shot_edit::gfx::{Gfx, Target};
use crate::sign::geom::{self, PageGeom};
use crate::sign::image_out::{self, ImageFormat};
use crate::sign::pdf_write::{self, OpenError, Opened, SaveError};
use crate::sign::render::{Done, Job, Pixels, Renderer};
use crate::sign::session::{Page, Session};
use crate::sign::{Item, ItemKind, Kind, Mark, Quarter, Source, names, office, style, text_draw, winansi};
use crate::theme::{self, WindowPalette};
use crate::ui::geometry::Rect;

const CLASS: PCWSTR = w!("ValeSign");

/// Tests build the real window but never show it or take the focus.
pub(super) const ON_SCREEN: bool = !cfg!(test);

/// Gives `hwnd` the keyboard focus, never in tests (`ON_SCREEN`). Every
/// focus change of the signing window and the maker goes through here.
pub(super) fn take_focus(hwnd: HWND) {
    if ON_SCREEN {
        unsafe {
            let _ = SetFocus(Some(hwnd));
        }
    }
}

/// The renderer finished something (`Renderer::take`).
const WM_RENDERED: u32 = WM_APP + 98;
/// The opening worker left its outcome in the inbox.
const WM_OPENED: u32 = WM_APP + 99;
/// The save worker left its outcome in the inbox.
const WM_SAVED: u32 = WM_APP + 100;
/// The maker finished (`maker::take_result`).
pub(super) const WM_MAKER: u32 = WM_APP + 101;

/// One-shot: the size settled, the page is rendered again at its new width.
const ID_RERENDER: usize = 1;
const RERENDER_MS: u32 = 120;
/// The widest page bitmap asked of the renderer.
const MAX_RENDER_W: u32 = 4096;

pub(super) const UI_TEXT: f32 = 13.0;
const WIN_W: f32 = 1100.0;
const WIN_H: f32 = 760.0;
const MIN_W: f32 = 720.0;
const MIN_H: f32 = 520.0;

// Default sizes in points.
const SIG_BOX: (f32, f32) = (150.0, 50.0);
const INITIALS_BOX: (f32, f32) = (48.0, 24.0);
const DATE_SIZE: (f32, f32) = (110.0, 14.0);
const TEXT_H: f32 = 14.0;
const CHECK_SIZE: f32 = 14.0;
pub(super) use crate::sign::text_draw::TEXT_FILL;
/// The gap between a signature and the date put under it.
const DATE_GAP: f32 = 3.0;
/// Lebih kecil / Lebih besar.
const STEP: f32 = 1.2;

// ---- state ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Loading,
    Converting,
    Password { wrong: bool, busy: bool },
    Ready,
    Saving,
    /// Opening failed; the stage says why and offers Tutup.
    Failed(Key),
}

/// The question the footer is asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ask {
    None,
    Discard,
    Breaks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tone {
    Hint,
    Error,
}

/// A tool waiting for a click on the page.
#[derive(Clone)]
pub(super) struct Armed {
    pub tool: Tool,
    pub mark: Option<Arc<Mark>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Drag {
    /// Page points at the press and now.
    Move { idx: usize, from: (f32, f32), now: (f32, f32) },
    /// The item's view rect at the press, the corner held and the pointer.
    Resize { idx: usize, corner: Corner, start: Rect, press: (f32, f32), now: (f32, f32) },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pop {
    More,
    Picker { initials: bool },
    /// The face menu under the selected item's format bar.
    Fonts,
}

pub(super) struct Shot {
    pub px: Pixels,
    pub bmp: Option<ID2D1Bitmap>,
    pub r#gen: u64,
}

#[cfg(test)]
pub(super) static ENGINES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The window's renderer. Dropping it stops and joins its thread.
pub(super) struct Engine(Option<Renderer>);

impl Engine {
    fn new(r: Renderer) -> Engine {
        #[cfg(test)]
        ENGINES.fetch_add(1, Ordering::SeqCst);
        Engine(Some(r))
    }

    fn request(&self, job: Job) {
        if let Some(r) = &self.0 {
            r.request(job);
        }
    }

    fn take(&self) -> Vec<Done> {
        self.0.as_ref().map(Renderer::take).unwrap_or_default()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        drop(self.0.take());
        #[cfg(test)]
        ENGINES.fetch_sub(1, Ordering::SeqCst);
    }
}

/// An opened document.
pub(super) struct Doc {
    pub session: Session,
    pub engine: Engine,
    /// The PDF (and its lock) when the source is a PDF or Office file.
    pub opened: Option<Arc<Opened>>,
    pub image: bool,
}

pub(super) struct Loaded {
    engine: Engine,
    opened: Option<Opened>,
    geoms: Vec<PageGeom>,
    image: bool,
    outs: Outs,
}

/// Where the copy goes, worked out off the UI thread (the writable probe
/// makes a file in the folder): `(path, fell back to Documents)`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Outs {
    /// In the source's own format (PDF for a PDF or Office file).
    pub own: (PathBuf, bool),
    /// As a PDF, for an image (Simpan sebagai PDF).
    pub pdf: Option<(PathBuf, bool)>,
}

impl Outs {
    pub(super) fn pick(&self, as_pdf: bool) -> &(PathBuf, bool) {
        match &self.pdf {
            Some(p) if as_pdf => p,
            _ => &self.own,
        }
    }
}

enum Outcome {
    Ready(Box<Loaded>),
    Password { bytes: Arc<Vec<u8>>, wrong: bool },
    Failed(Key),
}

enum Saved {
    To(PathBuf),
    /// With the outputs worked out again (the folder may have changed).
    Failed(Key, Outs),
}

#[derive(Default)]
struct Inbox {
    opened: Option<Outcome>,
    saved: Option<Saved>,
}

pub(super) struct Win {
    pub hwnd: HWND,
    owner: HWND,
    pub src: Source,
    pub demo: bool,
    pub gfx: Gfx,
    pub target: Option<Target>,
    pub p: WindowPalette,
    pub dpi: u32,
    pub client: (u32, u32),
    pub labels: Labels,
    pub layout: Layout,
    pub phase: Phase,
    pub ask: Ask,
    pub doc: Option<Doc>,
    /// The page shown (session index; always a live page).
    pub cur: usize,
    pub armed: Option<Armed>,
    pub selected: Option<usize>,
    pub drag: Option<Drag>,
    pub hover: Option<Hit>,
    pub pressed: Option<Hit>,
    /// The keyboard's place among the buttons; drawn as a ring.
    pub focus: Option<Hit>,
    pub pop: Option<Pop>,
    pub note: Option<(String, Tone)>,
    /// Where the copy goes and whether that is the fallback folder.
    pub out: Option<(PathBuf, bool)>,
    outs: Option<Outs>,
    /// Images: save as PDF instead of the image's own format.
    pub as_pdf: bool,
    pub rail_scroll: f32,
    /// The Teks EDIT and the item it edits (`None`: a new one at the page
    /// point).
    pub text: Option<(Field, Option<usize>, (f32, f32))>,
    pub password: Option<Field>,
    pending: Option<Arc<Vec<u8>>>,
    pub maker: Option<HWND>,
    /// The maker was opened from the keyboard: its mark is placed at once.
    maker_kb: bool,
    pub pages: HashMap<usize, Shot>,
    pub thumbs: HashMap<usize, Shot>,
    pub thumbs_asked: HashSet<usize>,
    pub failed: HashSet<usize>,
    asked: Option<(usize, u32)>,
    next_gen: u64,
    pub mark_bmps: HashMap<usize, (Arc<Mark>, ID2D1Bitmap)>,
    wheel: i32,
    cancel: Arc<AtomicBool>,
    open_job: Option<JoinHandle<()>>,
    job_end: Arc<Mutex<JobEnd>>,
    save_job: Option<JoinHandle<()>>,
    job_dir: Option<PathBuf>,
    inbox: Arc<Mutex<Inbox>>,
    /// Lost render targets in a row.
    pub lost: u8,
}

/// Joins `job` when it has already finished; a job still running is left to
/// end by itself (it sees the cancel flag, drops what it made and cleans up
/// after itself), so closing never waits on a slow file. True when it was
/// left running.
pub(super) fn reap(job: Option<JoinHandle<()>>) -> bool {
    match job {
        Some(j) if j.is_finished() => {
            let _ = j.join();
            false
        }
        Some(_) => true,
        None => false,
    }
}

/// Who removes the Office job folder: the opening worker and the window
/// each record their end under this lock, and whichever ends second sweeps,
/// so exactly one of them does, and never while the worker still uses it.
#[derive(Default)]
pub(super) struct JobEnd {
    /// A worker is running (set when one starts).
    running: bool,
    closed: bool,
}

impl JobEnd {
    pub(super) fn started(&mut self) {
        self.running = true;
    }
}

/// The opening worker is about to end. True: the window is already gone, the
/// worker sweeps.
pub(super) fn worker_ended(e: &Mutex<JobEnd>) -> bool {
    let mut e = e.lock().unwrap_or_else(|p| p.into_inner());
    e.running = false;
    e.closed
}

/// The window closes. True: no worker is left, the window sweeps.
pub(super) fn window_closed(e: &Mutex<JobEnd>) -> bool {
    let mut e = e.lock().unwrap_or_else(|p| p.into_inner());
    e.closed = true;
    !e.running
}

impl Drop for Win {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        reap(self.open_job.take());
        // A save is short and its result is the user's file: waited for.
        if let Some(job) = self.save_job.take() {
            let _ = job.join();
        }
        // The document and any renderer the inbox still holds go to a
        // short-lived thread: stopping a renderer joins its thread, which
        // may be mid-page, and the island must not wait for that.
        let left = self.inbox.lock().ok().and_then(|mut i| i.opened.take());
        drop_later((self.doc.take(), left));
        if let Some(t) = self.text.take() {
            text_edit::free_gdi(&t.0);
        }
        if let Some(f) = self.password.take() {
            text_edit::free_gdi(&f);
        }
        // A worker still opening sweeps the Office folder itself when it
        // ends; never under its feet.
        let sweep = window_closed(&self.job_end);
        if let Some(d) = self.job_dir.take()
            && sweep
        {
            office::sweep(&d);
        }
    }
}

thread_local! {
    static WINDOWS: RefCell<HashMap<isize, Win>> = RefCell::new(HashMap::new());
    /// Threads dropping what closed windows left (`drop_later`).
    static REAPERS: RefCell<Vec<JoinHandle<()>>> = const { RefCell::new(Vec::new()) };
}

/// Drops `v` on a short-lived thread, never on the UI thread; inline when no
/// thread can be made. Every value it gets is plain data whose COM and WinRT
/// objects live on their own threads (a renderer's are inside its worker).
pub(super) fn drop_later<T: Send + 'static>(v: T) {
    REAPERS.with(|r| {
        let mut r = r.borrow_mut();
        r.retain(|h| !h.is_finished());
        if let Ok(h) = std::thread::Builder::new().name("sign-reap".into()).spawn(move || drop(v)) {
            r.push(h);
        }
    });
}

/// Waits for every `drop_later` thread (the app is ending).
pub(super) fn join_reapers() {
    let left = REAPERS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    for h in left {
        let _ = h.join();
    }
}

pub(super) fn with<R>(hwnd: HWND, f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    WINDOWS.with(|m| m.try_borrow_mut().ok().and_then(|mut m| m.get_mut(&(hwnd.0 as isize)).map(f)))
}

fn key_down(vk: u16) -> bool {
    unsafe { GetKeyState(i32::from(vk)) < 0 }
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub(super) fn ui_of(w: &Win) -> f32 {
    w.dpi as f32 / 96.0
}

fn same_file(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

// ---- pure rules ----

/// "20 November 2026" in Indonesian or English.
pub(super) fn date_text(l: Lang, year: i32, month: u32, day: u32) -> String {
    const ID: [&str; 12] = [
        "Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober",
        "November", "Desember",
    ];
    const EN: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
        "November", "December",
    ];
    let names = if l == Lang::En { EN } else { ID };
    let m = names.get((month.max(1) - 1) as usize).copied().unwrap_or("");
    format!("{day} {m} {year}")
}

/// "Paraf dipasang di n halaman lain." with the English singular for one.
pub(super) fn all_pages_note(n: usize) -> String {
    if n == 1 {
        tr(Key::SignAllPagesDoneOne).to_owned()
    } else {
        tr(Key::SignAllPagesDone).replace("{n}", &n.to_string())
    }
}

fn today() -> String {
    use chrono::Datelike;
    let now = chrono::Local::now();
    date_text(lang(), now.year(), now.month(), now.day())
}

/// What closing does: nothing to lose closes, changes ask first, a running
/// save is waited for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CloseAction {
    Destroy,
    Ask,
    Wait,
}

pub(super) fn close_action(phase: Phase, dirty: bool) -> CloseAction {
    match phase {
        Phase::Saving => CloseAction::Wait,
        Phase::Ready if dirty => CloseAction::Ask,
        _ => CloseAction::Destroy,
    }
}

/// Saving would invalidate a digital signature already in the PDF: a full
/// rewrite, or a page turned (the revision changes the page itself).
pub(super) fn breaks_signature(signed: bool, incremental: bool, rotated: bool) -> bool {
    signed && (!incremental || rotated)
}

/// The step Esc takes, first that applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EscStep {
    Maker,
    Bar,
    Popover,
    Disarm,
    Deselect,
    Close,
}

pub(super) fn esc_step(maker: bool, ask: Ask, pop: bool, armed: bool, selected: bool) -> EscStep {
    if maker {
        EscStep::Maker
    } else if ask != Ask::None {
        EscStep::Bar
    } else if pop {
        EscStep::Popover
    } else if armed {
        EscStep::Disarm
    } else if selected {
        EscStep::Deselect
    } else {
        EscStep::Close
    }
}

/// The phase an opening outcome leads to.
fn phase_after(o: &Outcome) -> Phase {
    match o {
        Outcome::Ready(_) => Phase::Ready,
        Outcome::Password { wrong, .. } => Phase::Password { wrong: *wrong, busy: false },
        Outcome::Failed(k) => Phase::Failed(*k),
    }
}

/// Errors of `pdf_write::open` as the line the stage shows; `None` asks for
/// a password.
pub(super) fn open_error_key(e: &OpenError) -> Option<Key> {
    match e {
        OpenError::NeedsPassword | OpenError::WrongPassword => None,
        OpenError::Broken => Some(Key::SignCannotOpen),
        OpenError::TooBig => Some(Key::SignTooBig),
    }
}

pub(super) fn office_error_key(e: &office::ConvertError) -> Key {
    match e {
        office::ConvertError::NoSuite => Key::SignNoOffice,
        office::ConvertError::Locked => Key::SignOfficeLocked,
        office::ConvertError::Failed | office::ConvertError::TimedOut => Key::SignOfficeFailed,
    }
}

pub(super) fn save_error_key(e: &SaveError) -> Key {
    match e {
        SaveError::CannotKeepLock => Key::SignCannotKeepLock,
        SaveError::Write(_) => Key::SignSaveFailed,
    }
}

/// A mark's size inside a `bw` x `bh` box, keeping its aspect.
pub(super) fn fit_mark(m: &Mark, bw: f32, bh: f32) -> (f32, f32) {
    let a = m.h.max(1) as f32 / m.w.max(1) as f32;
    let h = bw * a;
    if h > bh { (bh / a, bh) } else { (bw, h) }
}

/// The render width for a page shown `shown_w` x `shown_h` px: the width of
/// the page as the renderer draws it (before the user's own turn), clamped.
pub(super) fn render_width(shown_w: f32, shown_h: f32, extra: Quarter) -> u32 {
    let w = if matches!(extra, Quarter::R90 | Quarter::R270) { shown_h } else { shown_w };
    (w.round().max(1.0) as u32).min(MAX_RENDER_W)
}

/// Point-space step of an arrow nudge `(dx, dy)` in view directions (y
/// down) on a page shown turned by `shown` clockwise.
pub(super) fn nudge(shown: Quarter, dx: f32, dy: f32) -> (f32, f32) {
    // View y down is page y up when upright; undo the clockwise turn.
    let (vx, vy) = (dx, -dy);
    match shown {
        Quarter::R0 => (vx, vy),
        Quarter::R90 => (-vy, vx),
        Quarter::R180 => (-vx, -vy),
        Quarter::R270 => (vy, -vx),
    }
}

// ---- opening ----

fn register(instance: HINSTANCE) -> windows::core::Result<()> {
    unsafe {
        let (icon, small) = crate::ui::app_icon::class_icons(instance);
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            hIcon: icon,
            hIconSm: small,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            let error = Error::from_thread();
            if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                return Err(error);
            }
        }
    }
    Ok(())
}

pub(super) fn open(owner: HWND, src: Source, demo: bool) -> bool {
    let existing = WINDOWS.with(|m| {
        m.try_borrow()
            .ok()
            .and_then(|m| m.iter().find(|(_, w)| same_file(&w.src.path, &src.path)).map(|(h, _)| *h))
    });
    if let Some(h) = existing {
        let hwnd = HWND(h as *mut c_void);
        if ON_SCREEN {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
            }
        }
        return true;
    }
    match try_open(owner, src, demo) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("vale: the signing window could not open: {e}");
            false
        }
    }
}

fn try_open(owner: HWND, src: Source, demo: bool) -> windows::core::Result<()> {
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    register(instance)?;
    let (work, dpi) = unsafe {
        let monitor = MonitorFromWindow(owner, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() {
            dx = 96;
        }
        (info.rcWork, dx.max(96))
    };
    let ui = dpi as f32 / 96.0;
    let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;
    let (ww, wh) = ((work.right - work.left) as f32, (work.bottom - work.top) as f32);
    let mut rc = RECT { left: 0, top: 0, right: (WIN_W * ui) as i32, bottom: (WIN_H * ui) as i32 };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WINDOW_EX_STYLE(0), dpi);
    }
    let ow = ((rc.right - rc.left) as f32).min(ww) as i32;
    let oh = ((rc.bottom - rc.top) as f32).min(wh) as i32;
    let x = work.left + ((work.right - work.left) - ow).max(0) / 2;
    let y = work.top + ((work.bottom - work.top) - oh).max(0) / 2;
    let title = src.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            &HSTRING::from(title.as_str()),
            style,
            x,
            y,
            ow,
            oh,
            None,
            None,
            Some(instance),
            None,
        )?
    };
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(e);
        }
    };
    let real_dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let p = theme::current_window();
    let job_dir = (src.kind == Kind::Office).then(|| office::job_dir().ok()).flatten();
    let mut win = Win {
        hwnd,
        owner,
        demo,
        gfx,
        target: None,
        p,
        dpi: real_dpi,
        client: ((client.right - client.left).max(1) as u32, (client.bottom - client.top).max(1) as u32),
        labels: Labels::default(),
        layout: Layout::default(),
        phase: if src.kind == Kind::Office { Phase::Converting } else { Phase::Loading },
        ask: Ask::None,
        doc: None,
        cur: 0,
        armed: None,
        selected: None,
        drag: None,
        hover: None,
        pressed: None,
        focus: None,
        pop: None,
        note: None,
        out: None,
        outs: None,
        as_pdf: false,
        rail_scroll: 0.0,
        text: None,
        password: None,
        pending: None,
        maker: None,
        maker_kb: false,
        pages: HashMap::new(),
        thumbs: HashMap::new(),
        thumbs_asked: HashSet::new(),
        failed: HashSet::new(),
        asked: None,
        next_gen: 0,
        mark_bmps: HashMap::new(),
        wheel: 0,
        cancel: Arc::new(AtomicBool::new(false)),
        open_job: None,
        job_end: Arc::new(Mutex::new(JobEnd::default())),
        save_job: None,
        job_dir,
        inbox: Arc::new(Mutex::new(Inbox::default())),
        lost: 0,
        src,
    };
    win.labels = measure_labels(&win.gfx, real_dpi as f32 / 96.0);
    relayout(&mut win);
    if win.src.kind == Kind::Office && win.job_dir.is_none() {
        win.phase = Phase::Failed(Key::SignOfficeFailed);
        relayout(&mut win);
    } else {
        start_open(&mut win, None);
    }
    let stored = WINDOWS.with(|m| match m.try_borrow_mut() {
        Ok(mut m) => {
            m.insert(hwnd.0 as isize, win);
            true
        }
        Err(_) => false,
    });
    if !stored {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return Err(Error::empty());
    }
    apply_frame(hwnd, &p);
    if ON_SCREEN {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    take_focus(hwnd);
    Ok(())
}

/// Size of the face names in the item bar and the face menu, in DIPs.
pub(super) const FACE_TEXT: f32 = 12.5;

pub(super) fn measure_labels(gfx: &Gfx, ui: f32) -> Labels {
    let mut all: Vec<(Hit, f32)> = layout::LABELLED
        .iter()
        .filter_map(|h| {
            let k = layout::label_key(*h)?;
            Some((*h, gfx.measure(tr(k), UI_TEXT * ui, paint::weight_of(*h)).0))
        })
        .collect();
    // Each face name in its own face.
    all.extend(style::Family::ALL.iter().map(|f| {
        let st = style::TextStyle { family: *f, ..Default::default() };
        (Hit::PickFamily(*f), text_draw::width(&gfx.dwrite, f.face(), &st, FACE_TEXT * ui).ceil())
    }));
    Labels(all)
}

/// Dark or light title bar in the window's colours, rounded corners.
pub(super) fn apply_frame(hwnd: HWND, p: &WindowPalette) {
    let dark = i32::from(!p.light);
    let caption = text_edit::colorref(p.bg).0;
    let text = text_edit::colorref(p.text).0;
    let round = DWMWCP_ROUND.0;
    unsafe {
        let set = |attr, v: *const c_void, n: usize| {
            let _ = DwmSetWindowAttribute(hwnd, attr, v, n as u32);
        };
        set(DWMWA_USE_IMMERSIVE_DARK_MODE, (&raw const dark).cast(), size_of::<i32>());
        set(DWMWA_CAPTION_COLOR, (&raw const caption).cast(), size_of::<u32>());
        set(DWMWA_TEXT_COLOR, (&raw const text).cast(), size_of::<u32>());
        set(DWMWA_WINDOW_CORNER_PREFERENCE, (&raw const round).cast(), size_of::<i32>());
    }
}

pub(super) fn restyle_open() {
    let p = theme::current_window();
    let open: Vec<(HWND, Option<HWND>)> = WINDOWS.with(|m| match m.try_borrow_mut() {
        Ok(mut m) => m
            .iter_mut()
            .map(|(h, w)| {
                w.p = p;
                (HWND(*h as *mut c_void), w.maker)
            })
            .collect(),
        Err(_) => Vec::new(),
    });
    for (hwnd, mk) in open {
        apply_frame(hwnd, &p);
        if let Some(m) = mk {
            maker::restyle(m, p);
        }
        invalidate(hwnd);
    }
}

pub(super) fn release() {
    let open: Vec<HWND> = WINDOWS.with(|m| match m.try_borrow() {
        Ok(m) => m.keys().map(|h| HWND(*h as *mut c_void)).collect(),
        Err(_) => Vec::new(),
    });
    for hwnd in open {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    // Whatever WM_DESTROY could not take (a busy map) is dropped here, which
    // joins its threads.
    let left = WINDOWS.with(|m| match m.try_borrow_mut() {
        Ok(mut m) => std::mem::take(&mut *m),
        Err(_) => HashMap::new(),
    });
    drop(left);
    join_reapers();
}

// ---- workers ----

/// Initialises COM (multithreaded) on a thread this module made, and
/// uninitialises it when dropped, after everything made under it.
struct Com(bool);

impl Com {
    fn init() -> Com {
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn post(hwnd: isize, msg: u32) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut c_void)), msg, WPARAM(0), LPARAM(0));
    }
}

/// Starts the opening worker: the first open, or a password retry.
fn start_open(w: &mut Win, password: Option<String>) {
    if let Some(job) = w.open_job.take() {
        let _ = job.join();
    }
    let hwnd = w.hwnd.0 as isize;
    let src = w.src.clone();
    let inbox = w.inbox.clone();
    let cancel = w.cancel.clone();
    let job_dir = w.job_dir.clone();
    let pending = w.pending.clone();
    let job_end = w.job_end.clone();
    job_end.lock().unwrap_or_else(|p| p.into_inner()).started();
    // Whatever way the worker ends, it records it last; if the window has
    // closed by then, the Office copy goes with the worker.
    let end = move |job_end: &Mutex<JobEnd>, job_dir: &Option<PathBuf>| {
        if worker_ended(job_end)
            && let Some(d) = job_dir
        {
            office::sweep(d);
        }
    };
    let demo = w.demo;
    let name = if src.kind == Kind::Office && pending.is_none() { "sign-office" } else { "sign-open" };
    let spawned = std::thread::Builder::new().name(name.into()).spawn(move || {
        let outcome = {
            let _com = Com::init();
            open_document(&src, demo, pending, password, job_dir.as_deref(), &cancel, hwnd)
        };
        if cancel.load(Ordering::SeqCst) {
            // The window closed without waiting: what was opened (a renderer
            // included) ends here.
            drop(outcome);
        } else {
            if let Ok(mut i) = inbox.lock() {
                i.opened = Some(outcome);
            }
            post(hwnd, WM_OPENED);
        }
        end(&job_end, &job_dir);
    });
    match spawned {
        Ok(h) => w.open_job = Some(h),
        Err(_) => {
            // No worker after all: the window sweeps when it closes.
            worker_ended(&w.job_end);
            w.phase = Phase::Failed(Key::SignCannotOpen);
        }
    }
}

fn read_capped(path: &Path) -> Result<Vec<u8>, Key> {
    let meta = std::fs::metadata(path).map_err(|_| Key::SignCannotOpen)?;
    if meta.len() > pdf_write::MAX_BYTES as u64 {
        return Err(Key::SignTooBig);
    }
    std::fs::read(path).map_err(|_| Key::SignCannotOpen)
}

fn open_document(
    src: &Source,
    demo: bool,
    pending: Option<Arc<Vec<u8>>>,
    password: Option<String>,
    job_dir: Option<&Path>,
    cancel: &AtomicBool,
    hwnd: isize,
) -> Outcome {
    let notify = move || post(hwnd, WM_RENDERED);
    if src.kind == Kind::Image {
        return match Renderer::start_image(src.path.clone(), notify) {
            Ok(r) => {
                let (pw, ph) = r.page_size_dip(0);
                let geoms = vec![image_out::image_geom(pw as u32, ph as u32)];
                let outs = outs_now(&src.path, demo, true);
                Outcome::Ready(Box::new(Loaded { engine: Engine::new(r), opened: None, geoms, image: true, outs }))
            }
            Err(crate::sign::render::RenderOpenError::TooBig) => Outcome::Failed(Key::SignImageTooBig),
            Err(_) => Outcome::Failed(Key::SignCannotOpen),
        };
    }
    let bytes = match pending {
        Some(b) => b,
        None => {
            let pdf = if src.kind == Kind::Office {
                let Some(dir) = job_dir else { return Outcome::Failed(Key::SignOfficeFailed) };
                match office::convert(&src.path, dir, cancel) {
                    Ok(p) => p,
                    Err(e) => return Outcome::Failed(office_error_key(&e)),
                }
            } else {
                src.path.clone()
            };
            if cancel.load(Ordering::SeqCst) {
                return Outcome::Failed(Key::SignCannotOpen);
            }
            match read_capped(&pdf) {
                Ok(b) => Arc::new(b),
                Err(k) => return Outcome::Failed(k),
            }
        }
    };
    let tried = password.is_some();
    let opened = match pdf_write::open((*bytes).clone(), password.as_deref()) {
        Ok(o) => o,
        Err(e) => {
            return match open_error_key(&e) {
                None => Outcome::Password { bytes, wrong: tried },
                Some(k) => Outcome::Failed(k),
            };
        }
    };
    if cancel.load(Ordering::SeqCst) {
        return Outcome::Failed(Key::SignCannotOpen);
    }
    match Renderer::start_pdf(opened.bytes.clone(), password, notify) {
        Ok(r) if r.page_count() as usize == opened.geoms.len() => {
            let geoms = opened.geoms.clone();
            let outs = outs_now(&src.path, demo, false);
            Outcome::Ready(Box::new(Loaded { engine: Engine::new(r), opened: Some(opened), geoms, image: false, outs }))
        }
        Ok(_) => Outcome::Failed(Key::SignCannotOpen),
        Err(crate::sign::render::RenderOpenError::Password) => Outcome::Password { bytes, wrong: tried },
        Err(_) => Outcome::Failed(Key::SignCannotOpen),
    }
}

fn on_opened(hwnd: HWND) {
    enum Field_ {
        Keep,
        Drop(Field),
        Make,
        Clear(HWND),
    }
    // Decided under the borrow; EDITs are made, cleared and destroyed after
    // it (they send messages to this window).
    let field = with(hwnd, |w| {
        let Some(outcome) = w.inbox.lock().ok().and_then(|mut i| i.opened.take()) else { return Field_::Keep };
        w.phase = phase_after(&outcome);
        let field = match outcome {
            Outcome::Ready(l) => {
                let Loaded { engine, opened, geoms, image, outs } = *l;
                w.pending = None;
                w.doc = Some(Doc { session: Session::new(geoms), engine, opened: opened.map(Arc::new), image });
                w.cur = 0;
                w.outs = Some(outs);
                refresh_out(w);
                w.password.take().map_or(Field_::Keep, Field_::Drop)
            }
            Outcome::Password { bytes, .. } => {
                w.pending = Some(bytes);
                match &w.password {
                    None => Field_::Make,
                    Some(f) => Field_::Clear(f.edit),
                }
            }
            Outcome::Failed(_) => w.password.take().map_or(Field_::Keep, Field_::Drop),
        };
        relayout(w);
        want_render(w);
        field
    })
    .unwrap_or(Field_::Keep);
    let focus_field = match field {
        Field_::Keep => None,
        Field_::Drop(f) => {
            text_edit::destroy(f);
            None
        }
        Field_::Make => make_password(hwnd, ""),
        Field_::Clear(e) => {
            text_edit::clear(e);
            Some(e)
        }
    };
    with(hwnd, |w| place_password(w));
    take_focus(focus_field.unwrap_or(hwnd));
    invalidate(hwnd);
}

/// Makes the password EDIT (outside the window's borrow) on the laid-out
/// field, holding `initial`, and stores it. Returns its HWND.
fn make_password(hwnd: HWND, initial: &str) -> Option<HWND> {
    let (f, ui, p) = with(hwnd, |w| (w.layout.field.unwrap_or_default(), ui_of(w), w.p))?;
    let spec = text_edit::Spec {
        rect: (f.x as i32, f.y as i32, f.w as i32, f.h as i32),
        initial,
        px: 14.0 * ui,
        face: crate::ui::fonts::gdi_text_face(),
        color: p.text,
        back: p.field,
        password: true,
        commit_on_blur: false,
    };
    let Some(made) = text_edit::create(hwnd, &spec) else {
        eprintln!("vale: the password field could not be made");
        return None;
    };
    let edit = made.edit;
    let mut slot = Some(made);
    with(hwnd, |w| {
        w.password = slot.take();
        place_password(w);
    });
    if let Some(f) = slot {
        text_edit::destroy(f);
        return None;
    }
    Some(edit)
}

/// Keeps the password EDIT on its laid-out field (inset for the border).
fn place_password(w: &Win) {
    if let (Some(f), Some(r)) = (&w.password, w.layout.field) {
        let ui = ui_of(w);
        let inset = (5.0 * ui).round();
        text_edit::move_to(
            f.edit,
            (r.x + inset) as i32,
            (r.y + inset) as i32,
            (r.w - 2.0 * inset) as i32,
            (r.h - 2.0 * inset) as i32,
        );
    }
}

fn submit_password(hwnd: HWND) {
    with(hwnd, |w| {
        if !matches!(w.phase, Phase::Password { busy: false, .. }) {
            return;
        }
        let Some(f) = &w.password else { return };
        let typed = text_edit::text(f.edit);
        w.phase = Phase::Password { wrong: false, busy: true };
        start_open(w, Some(typed));
    });
    invalidate(hwnd);
}

// ---- layout and view ----

pub(super) fn relayout(w: &mut Win) {
    let image = w.doc.as_ref().is_some_and(|d| d.image);
    let ready = matches!(w.phase, Phase::Ready | Phase::Saving);
    let footer = match (w.phase, w.ask) {
        (Phase::Ready | Phase::Saving, Ask::Discard) => Footer::AskDiscard,
        (Phase::Ready | Phase::Saving, Ask::Breaks) => Footer::AskBreaks,
        (Phase::Ready | Phase::Saving, Ask::None) => Footer::Normal { formats: image },
        (Phase::Failed(_), _) => Footer::Empty,
        _ => Footer::CancelOnly,
    };
    let stage = match w.phase {
        Phase::Ready | Phase::Saving => StageMode::Page,
        Phase::Loading | Phase::Converting => StageMode::Message,
        Phase::Password { .. } => StageMode::Password,
        Phase::Failed(_) => StageMode::Error,
    };
    let banner = ready && w.doc.as_ref().and_then(|d| d.opened.as_ref()).is_some_and(|o| o.no_changes);
    let spec = Spec { cw: w.client.0 as f32, ch: w.client.1 as f32, ui: ui_of(w), banner, footer, stage };
    w.layout = layout::layout(&spec, &w.labels);
}

fn is_tool_row(h: Hit) -> bool {
    matches!(h, Hit::Tool(_) | Hit::Rotate | Hit::DeletePage | Hit::More)
}

/// The page as shown: its rect, scale (px per point), geometry and the
/// user's turn.
#[derive(Clone, Copy)]
pub(super) struct View {
    pub page: Rect,
    pub scale: f32,
    pub g: PageGeom,
    pub extra: Quarter,
}

impl View {
    pub fn to_page(&self, x: f32, y: f32) -> (f32, f32) {
        geom::view_to_page(self.g, self.extra, self.scale, (x - self.page.x, y - self.page.y))
    }

    pub fn item_rect(&self, it: &Item) -> Rect {
        let (x, y, w, h) = geom::item_view_rect(it, self.g, self.extra, self.scale);
        Rect { x: x + self.page.x, y: y + self.page.y, w, h }
    }

    pub fn shown(&self) -> Quarter {
        self.g.base.add(self.extra)
    }
}

pub(super) fn view(w: &Win) -> Option<View> {
    let doc = w.doc.as_ref()?;
    let pg = doc.session.pages.get(w.cur)?;
    let shown = geom::shown_size(pg.geom, pg.extra);
    let (page, scale) = layout::fit_page(w.layout.stage, shown, ui_of(w));
    Some(View { page, scale, g: pg.geom, extra: pg.extra })
}

/// Item `i` as it is drawn now: with a drag in progress applied.
pub(super) fn shown_item(w: &Win, v: &View, i: usize) -> Option<Item> {
    let it = w.doc.as_ref()?.session.items.get(i)?;
    let mut it = it.clone();
    match w.drag {
        Some(Drag::Move { idx, from, now }) if idx == i => {
            let (bw, bh) = geom::occupied(&it);
            let (x, y) = layout::clamp_into(
                it.x + now.0 - from.0,
                it.y + now.1 - from.1,
                bw,
                bh,
                v.g.w,
                v.g.h,
            );
            it.x = x;
            it.y = y;
        }
        Some(Drag::Resize { idx, corner, start, press, now }) if idx == i => {
            let f = layout::drag_factor(start, corner, press, now);
            let (nw, nh) = (it.w * f, it.h * f);
            resize_in_place(&mut it, v, start, corner, nw, nh);
        }
        _ => {}
    }
    Some(it)
}

/// `it` at `nw` x `nh`, held by the corner opposite `corner` of `start`.
fn resize_in_place(it: &mut Item, v: &View, start: Rect, corner: Corner, nw: f32, nh: f32) {
    let max = v.g.w.min(v.g.h).max(8.0);
    let f = (nw / it.w.max(0.01)).max(8.0 / it.w.max(0.01)).max(8.0 / it.h.max(0.01));
    let f = f.min(max / it.w.max(0.01)).min(max / it.h.max(0.01));
    it.w *= f;
    it.h *= f;
    let _ = nh;
    place_at_anchor(it, v, start, corner);
}

fn place_at_anchor(it: &mut Item, v: &View, start: Rect, corner: Corner) {
    let r = v.item_rect(it);
    let held = layout::anchored(layout::anchor_of(start, corner), corner, r.w, r.h);
    let a = v.to_page(held.x, held.y);
    let b = v.to_page(held.right(), held.bottom());
    let (bw, bh) = geom::occupied(it);
    let (x, y) = layout::clamp_into(a.0.min(b.0), a.1.min(b.1), bw, bh, v.g.w, v.g.h);
    it.x = x;
    it.y = y;
}

/// The items of the shown page with their view rects, in drawing order.
pub(super) fn page_items(w: &Win, v: &View) -> Vec<(usize, Rect)> {
    let Some(doc) = &w.doc else { return Vec::new() };
    let editing = w.text.as_ref().and_then(|t| t.1);
    (0..doc.session.items.len())
        .filter(|&i| doc.session.items[i].page == w.cur && Some(i) != editing)
        .filter_map(|i| shown_item(w, v, i).map(|it| (i, v.item_rect(&it))))
        .collect()
}

pub(super) fn float_hits(kind: &ItemKind) -> Vec<Hit> {
    let mut hits = Vec::new();
    match kind {
        ItemKind::Text(..) | ItemKind::Date(..) => {
            hits.extend([Hit::Family, Hit::Bold, Hit::Italic, Hit::Underline]);
            hits.extend(style::Tint::ALL.map(Hit::Tint));
        }
        ItemKind::Check(_) => hits.extend(style::Tint::ALL.map(Hit::Tint)),
        _ => {}
    }
    hits.extend([Hit::Smaller, Hit::Bigger]);
    match kind {
        ItemKind::Signature(_) => hits.push(Hit::AddDate),
        ItemKind::Initials(_) => hits.push(Hit::AllPages),
        _ => {}
    }
    hits.push(Hit::Remove);
    hits
}

pub(super) fn float_bar(w: &Win, v: &View) -> Option<Popover> {
    let i = w.selected?;
    if w.drag.is_some() || w.text.is_some() || w.phase != Phase::Ready {
        return None;
    }
    let it = shown_item(w, v, i)?;
    let r = v.item_rect(&it);
    Some(layout::float_bar(r, w.layout.stage, &float_hits(&it.kind), &w.labels, ui_of(w)))
}

pub(super) fn popover(w: &Win) -> Option<Popover> {
    let ui = ui_of(w);
    let cw = w.client.0 as f32;
    match w.pop? {
        Pop::More => Some(layout::more_menu(w.layout.rect_of(Hit::More)?, &w.layout.overflow, cw, ui)),
        Pop::Picker { initials } => {
            let tool = Hit::Tool(if initials { Tool::Initials } else { Tool::Signature });
            let anchor = w.layout.rect_of(tool).or_else(|| w.layout.rect_of(Hit::More))?;
            let n = super::saved_count(initials, w.demo);
            Some(layout::picker(anchor, n, cw, ui))
        }
        Pop::Fonts => {
            let v = view(w)?;
            let bar = float_bar(w, &v)?;
            let it = shown_item(w, &v, w.selected?)?;
            let family = bar.items.iter().find(|(h, _)| *h == Hit::Family)?.1;
            Some(layout::font_menu(family, bar.panel, v.item_rect(&it), w.layout.stage, ui))
        }
    }
}

pub(super) fn rail(w: &Win) -> Vec<RailRow> {
    let Some(doc) = &w.doc else { return Vec::new() };
    let pages: Vec<(usize, (f32, f32))> = doc
        .session
        .live_pages()
        .into_iter()
        .map(|i| {
            let pg = &doc.session.pages[i];
            (i, geom::shown_size(pg.geom, pg.extra))
        })
        .collect();
    layout::rail_rows(w.layout.rail, &pages, ui_of(w), w.rail_scroll)
}

fn handle_rects(w: &Win, v: &View) -> Vec<(Corner, Rect)> {
    let Some(i) = w.selected else { return Vec::new() };
    if w.text.is_some() {
        return Vec::new();
    }
    let Some(it) = shown_item(w, v, i) else { return Vec::new() };
    let ui = ui_of(w);
    let o = v.item_rect(&it);
    let o = Rect { x: o.x - 3.0 * ui, y: o.y - 3.0 * ui, w: o.w + 6.0 * ui, h: o.h + 6.0 * ui };
    layout::handles(o, ui).iter().map(|(c, _, hit)| (*c, *hit)).collect()
}

fn hit_at(w: &Win, x: f32, y: f32) -> Option<Hit> {
    let v = view(w).filter(|_| matches!(w.phase, Phase::Ready | Phase::Saving));
    let pop = popover(w);
    let bar = v.as_ref().and_then(|v| float_bar(w, v));
    let handles = v.as_ref().map(|v| handle_rects(w, v)).unwrap_or_default();
    let items = v.as_ref().map(|v| page_items(w, v)).unwrap_or_default();
    let rows = rail(w);
    let scene = Scene {
        layout: &w.layout,
        popover: pop.as_ref(),
        float: bar.as_ref(),
        handles: &handles,
        items: &items,
        page: v.map(|v| v.page),
        rail: &rows,
    };
    // Nothing to place or turn before the document is open: the tools row
    // shows, dimmed, and takes no clicks.
    let ready = matches!(w.phase, Phase::Ready | Phase::Saving);
    layout::hit_test(&scene, x, y).filter(|h| ready || !is_tool_row(*h))
}

/// Keyboard order: the open popover's entries, else the chrome, with the
/// item bar after the tools row when an item is selected.
fn focus_ring(w: &Win) -> Vec<Hit> {
    if let Some(p) = popover(w) {
        return p.items.iter().map(|(h, _)| *h).collect();
    }
    let ready = w.phase == Phase::Ready;
    let mut order: Vec<Hit> =
        layout::focus_order(&w.layout).into_iter().filter(|h| ready || !is_tool_row(*h)).collect();
    if let Some(bar) = view(w).and_then(|v| float_bar(w, &v)) {
        let at = order.iter().position(|h| !is_tool_row(*h)).unwrap_or(order.len());
        for (i, (h, _)) in bar.items.iter().enumerate() {
            order.insert(at + i, *h);
        }
    }
    order
}

/// Where a focused hit is drawn.
pub(super) fn focus_rect(w: &Win, hit: Hit) -> Option<Rect> {
    if let Some(r) = popover(w).and_then(|p| p.items.iter().find(|(h, _)| *h == hit).map(|(_, r)| *r)) {
        return Some(r);
    }
    if let Some(r) = view(w).and_then(|v| float_bar(w, &v)).and_then(|b| {
        b.items.iter().find(|(h, _)| *h == hit).map(|(_, r)| *r)
    }) {
        return Some(r);
    }
    w.layout.rect_of(hit)
}

// ---- rendering requests ----

fn want_render(w: &mut Win) {
    if !matches!(w.phase, Phase::Ready | Phase::Saving) {
        return;
    }
    let Some(v) = view(w) else { return };
    let width = render_width(v.page.w, v.page.h, v.extra);
    if w.asked == Some((w.cur, width)) {
        return;
    }
    w.next_gen += 1;
    let r#gen = w.next_gen;
    if let Some(doc) = &w.doc {
        doc.engine.request(Job::Page { page: w.cur as u32, width_px: width, r#gen });
    }
    w.asked = Some((w.cur, width));
    // Only the page shown and its neighbours keep their pixels.
    let cur = w.cur;
    w.pages.retain(|p, _| p.abs_diff(cur) <= 1);
}

/// Thumbnails the rail shows and has not asked for yet.
pub(super) fn want_thumbs(w: &mut Win, rows: &[RailRow]) {
    let Some(doc) = &w.doc else { return };
    for r in rows {
        if r.thumb.bottom() < w.layout.rail.y || r.thumb.y > w.layout.rail.bottom() {
            continue;
        }
        if w.thumbs_asked.insert(r.page) {
            let extra = doc.session.pages.get(r.page).map_or(Quarter::R0, |p| p.extra);
            let width = render_width(r.thumb.w, r.thumb.h, extra);
            doc.engine.request(Job::Thumb { page: r.page as u32, width_px: width });
        }
    }
}

fn on_rendered(hwnd: HWND) {
    with(hwnd, |w| {
        let Some(doc) = &w.doc else { return };
        for done in doc.engine.take() {
            match done {
                Done::Page { page, r#gen, px } => {
                    let page = page as usize;
                    if w.pages.get(&page).is_none_or(|s| s.r#gen <= r#gen) {
                        w.pages.insert(page, Shot { px, bmp: None, r#gen });
                        w.failed.remove(&page);
                    }
                }
                Done::Thumb { page, px } => {
                    w.thumbs.insert(page as usize, Shot { px, bmp: None, r#gen: 0 });
                }
                Done::Failed { page } => {
                    w.failed.insert(page as usize);
                }
            }
        }
    });
    invalidate(hwnd);
}

// ---- output ----

fn out_ext(w: &Win) -> String {
    let image = w.doc.as_ref().is_some_and(|d| d.image);
    if image && !w.as_pdf {
        w.src.path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "png".into())
    } else {
        "pdf".into()
    }
}

fn demo_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("target").join("demo-sign")
}

/// The folder and naming a save uses: the source's own folder, or for the
/// demo `target/demo-sign`.
fn naming(src: &Path, demo: bool) -> (PathBuf, PathBuf) {
    if demo {
        let dir = demo_dir();
        let _ = std::fs::create_dir_all(&dir);
        let name = src.file_name().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("Dokumen.pdf"));
        (dir.join(name), dir)
    } else {
        (src.to_path_buf(), names::fallback_dir())
    }
}

/// The outputs of `src` for its own format and, for an image, PDF: one
/// writable probe of the folder serves both.
fn outs_for(
    src: &Path,
    demo: bool,
    image: bool,
    l: Lang,
    writable: impl Fn(&Path) -> bool,
    exists: impl Fn(&Path) -> bool,
) -> Outs {
    let (named, fallback) = naming(src, demo);
    let probed = std::cell::Cell::new(None);
    let writable = |d: &Path| match probed.get() {
        Some(ok) => ok,
        None => {
            let ok = writable(d);
            probed.set(Some(ok));
            ok
        }
    };
    let out = |ext: &str| {
        let path = names::output_path(&named, ext, l, &exists, &writable, &fallback);
        let fell_back = path.parent() != named.parent();
        (path, fell_back)
    };
    let own_ext = if image {
        src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "png".into())
    } else {
        "pdf".into()
    };
    let own = out(&own_ext);
    let pdf = (image && own_ext != "pdf").then(|| out("pdf"));
    Outs { own, pdf }
}

/// `outs_for` against the disk. Off the UI thread only: it probes the folder.
fn outs_now(src: &Path, demo: bool, image: bool) -> Outs {
    outs_for(src, demo, image, lang(), names::dir_is_writable, |p| p.exists())
}

/// Shows the output for the chosen format from what the worker worked out;
/// nothing touches the disk here.
fn refresh_out(w: &mut Win) {
    let image = w.doc.as_ref().is_some_and(|d| d.image);
    w.out = w.outs.as_ref().map(|o| o.pick(image && w.as_pdf).clone());
}

// ---- actions ----

fn say(w: &mut Win, k: Key, tone: Tone) {
    w.note = Some((tr(k).to_owned(), tone));
}

fn go_to(w: &mut Win, page: usize) {
    let Some(doc) = &w.doc else { return };
    if page >= doc.session.pages.len() || doc.session.pages[page].deleted {
        return;
    }
    if page != w.cur {
        w.selected = None;
        close_stale_fonts(w);
    }
    w.cur = page;
    let rows = rail(w);
    w.rail_scroll = layout::rail_scroll_to(&rows, w.layout.rail, page, ui_of(w), w.rail_scroll);
    want_render(w);
}

fn step_page(w: &mut Win, by: i32) {
    let Some(doc) = &w.doc else { return };
    let live = doc.session.live_pages();
    let Some(at) = live.iter().position(|&p| p == w.cur) else { return };
    let next = (at as i32 + by).clamp(0, live.len() as i32 - 1) as usize;
    go_to(w, live[next]);
}

/// Adds `it` on the shown page and selects it.
fn add_item(w: &mut Win, it: Item) {
    let Some(doc) = &mut w.doc else { return };
    let i = doc.session.add(it);
    w.selected = Some(i);
    w.armed = None;
}

/// A new item of `tool` (and `mark`) centred on the page point `at`.
fn place(w: &mut Win, v: &View, tool: Tool, mark: Option<Arc<Mark>>, at: (f32, f32)) {
    let (kind, (iw, ih)) = match (tool, mark) {
        (Tool::Signature, Some(m)) => {
            let s = fit_mark(&m, SIG_BOX.0, SIG_BOX.1);
            (ItemKind::Signature(m), s)
        }
        (Tool::Initials, Some(m)) => {
            let s = fit_mark(&m, INITIALS_BOX.0, INITIALS_BOX.1);
            (ItemKind::Initials(m), s)
        }
        (Tool::Date, _) => (ItemKind::Date(today(), style::last_text()), DATE_SIZE),
        (Tool::Check, _) => (ItemKind::Check(style::last_check()), (CHECK_SIZE, CHECK_SIZE)),
        _ => return,
    };
    let turn = geom::upright_turn(v.g, v.extra);
    let (x, y) = layout::place_centred(at, iw, ih, turn, v.g.w, v.g.h);
    add_item(w, Item { kind, page: w.cur, x, y, w: iw, h: ih, turn });
}

/// The page point at the centre of the shown page.
fn page_centre(v: &View) -> (f32, f32) {
    v.to_page(v.page.x + v.page.w / 2.0, v.page.y + v.page.h / 2.0)
}

/// A tool chosen (`kb`: from the keyboard, so it is placed at the page centre
/// at once instead of waiting for a click).
fn choose_tool(hwnd: HWND, tool: Tool, kb: bool) {
    enum Next {
        Maker(bool),
        Text((f32, f32)),
        Nothing,
    }
    let next = with(hwnd, |w| {
        let Some(v) = view(w) else { return Next::Nothing };
        match tool {
            Tool::Signature | Tool::Initials => {
                let initials = tool == Tool::Initials;
                if super::saved_count(initials, w.demo) == 0 {
                    Next::Maker(initials)
                } else {
                    w.pop = Some(Pop::Picker { initials });
                    w.focus = kb.then_some(Hit::PickMark(0));
                    Next::Nothing
                }
            }
            Tool::Text if kb => Next::Text((v.page.x + v.page.w / 2.0 - 40.0, v.page.y + v.page.h / 2.0)),
            _ if kb => {
                place(w, &v, tool, None, page_centre(&v));
                Next::Nothing
            }
            _ => {
                let again = w.armed.as_ref().is_some_and(|a| a.tool == tool);
                w.armed = (!again).then_some(Armed { tool, mark: None });
                w.selected = None;
                Next::Nothing
            }
        }
    });
    match next {
        Some(Next::Maker(initials)) => open_maker(hwnd, initials, kb),
        Some(Next::Text(at)) => begin_text(hwnd, at, None),
        _ => {}
    }
}

fn open_maker(hwnd: HWND, initials: bool, kb: bool) {
    let plan = with(hwnd, |w| {
        if w.maker.is_some() {
            return None;
        }
        w.pop = None;
        w.armed = None;
        w.maker_kb = kb;
        Some((w.p, ui_of(w), w.demo, w.client))
    })
    .flatten();
    let Some((p, ui, demo, client)) = plan else { return };
    if let Some(m) = maker::open(hwnd, initials, demo, p, ui, client) {
        with(hwnd, |w| w.maker = Some(m));
    }
    invalidate(hwnd);
}

fn on_maker(hwnd: HWND) {
    let Some(res) = maker::take_result(hwnd) else { return };
    let mk = with(hwnd, |w| w.maker.take()).flatten();
    if let Some(m) = mk {
        unsafe {
            let _ = DestroyWindow(m);
        }
    }
    with(hwnd, |w| {
        if let Some(mark) = res.mark {
            let tool = if res.initials { Tool::Initials } else { Tool::Signature };
            arm_mark(w, tool, Arc::new(mark));
        }
    });
    take_focus(hwnd);
    invalidate(hwnd);
}

/// A mark picked (saved tile or the maker): placed at once from the
/// keyboard, else waiting for a click on the page.
fn arm_mark(w: &mut Win, tool: Tool, mark: Arc<Mark>) {
    w.pop = None;
    if w.maker_kb || matches!(w.focus, Some(Hit::PickMark(_))) {
        w.maker_kb = false;
        w.focus = None;
        if let Some(v) = view(w) {
            place(w, &v, tool, Some(mark), page_centre(&v));
        }
        return;
    }
    w.armed = Some(Armed { tool, mark: Some(mark) });
    w.selected = None;
    say(w, Key::SignPlaceHint, Tone::Hint);
}

fn act(hwnd: HWND, hit: Hit, kb: bool) {
    let ready = with(hwnd, |w| {
        w.note = None;
        w.phase == Phase::Ready
    })
    .unwrap_or(false);
    match hit {
        Hit::Tool(t) if ready => {
            with(hwnd, |w| w.pop = None);
            choose_tool(hwnd, t, kb);
        }
        Hit::More if ready => {
            with(hwnd, |w| {
                w.pop = if w.pop == Some(Pop::More) { None } else { Some(Pop::More) };
                if kb {
                    w.focus = w.layout.overflow.first().copied();
                }
            });
        }
        Hit::Rotate if ready => {
            with(hwnd, |w| {
                w.pop = None;
                let cur = w.cur;
                if let Some(d) = &mut w.doc {
                    d.session.rotate_page(cur);
                }
                want_render(w);
            });
        }
        Hit::DeletePage if ready => {
            with(hwnd, |w| {
                w.pop = None;
                let cur = w.cur;
                let Some(d) = &mut w.doc else { return };
                let live = d.session.live_pages();
                let at = live.iter().position(|&p| p == cur).unwrap_or(0);
                match d.session.delete_page(cur) {
                    Ok(()) => {
                        let next = live.get(at + 1).or_else(|| at.checked_sub(1).and_then(|i| live.get(i)));
                        w.selected = None;
                        if let Some(&n) = next {
                            go_to(w, n);
                        }
                    }
                    Err(_) => say(w, Key::SignLastPage, Tone::Error),
                }
            });
        }
        Hit::Thumb(p) if ready => {
            with(hwnd, |w| go_to(w, p));
        }
        Hit::FormatImage | Hit::FormatPdf if ready => {
            with(hwnd, |w| {
                w.as_pdf = hit == Hit::FormatPdf;
                refresh_out(w);
            });
        }
        Hit::Cancel => request_close(hwnd),
        Hit::Save => do_save(hwnd, false),
        Hit::Discard | Hit::CloseWindow => destroy(hwnd),
        Hit::Keep | Hit::NotNow => {
            with(hwnd, |w| {
                w.ask = Ask::None;
                w.focus = None;
                relayout(w);
            });
        }
        Hit::SaveAnyway => do_save(hwnd, true),
        Hit::OpenPassword => submit_password(hwnd),
        Hit::Smaller | Hit::Bigger if ready => {
            with(hwnd, |w| scale_selected(w, if hit == Hit::Bigger { STEP } else { 1.0 / STEP }));
        }
        Hit::Family if ready => {
            with(hwnd, |w| {
                w.pop = if w.pop == Some(Pop::Fonts) { None } else { Some(Pop::Fonts) };
                if kb && w.pop.is_some() {
                    w.focus = selected_style(w).map(|st| Hit::PickFamily(st.family));
                }
            });
        }
        Hit::Bold | Hit::Italic | Hit::Underline if ready => {
            with(hwnd, |w| {
                w.pop = None;
                toggle_selected(w, hit);
            });
        }
        Hit::Tint(t) if ready => {
            with(hwnd, |w| {
                w.pop = None;
                tint_selected(w, t);
            });
        }
        Hit::PickFamily(f) if ready => {
            with(hwnd, |w| {
                w.pop = None;
                restyle_selected(w, |st| st.family = f);
                if kb {
                    w.focus = Some(Hit::Family);
                }
            });
        }
        Hit::AddDate if ready => with(hwnd, add_date_under).unwrap_or(()),
        Hit::AllPages if ready => {
            with(hwnd, |w| {
                let Some(i) = w.selected else { return };
                let Some(d) = &mut w.doc else { return };
                let n = d.session.initials_on_all(i);
                if n == 0 && d.session.initials_everywhere(i) {
                    say(w, Key::SignAllPagesAlready, Tone::Hint);
                } else if n == 0 {
                    say(w, Key::SignAllPagesNone, Tone::Hint);
                } else {
                    w.note = Some((all_pages_note(n), Tone::Hint));
                }
            });
        }
        Hit::Remove if ready => remove_selected(hwnd),
        Hit::PickMark(i) => {
            with(hwnd, |w| {
                let Some(Pop::Picker { initials }) = w.pop else { return };
                let marks = super::saved_marks(initials, w.demo);
                if let Some(m) = marks.get(i) {
                    let tool = if initials { Tool::Initials } else { Tool::Signature };
                    arm_mark(w, tool, m.clone());
                }
            });
        }
        Hit::PickNew => {
            let initials = with(hwnd, |w| match w.pop {
                Some(Pop::Picker { initials }) => Some(initials),
                _ => None,
            })
            .flatten();
            if let Some(initials) = initials {
                open_maker(hwnd, initials, kb);
            }
        }
        _ => {}
    }
    with(hwnd, |w| {
        close_stale_fonts(w);
        relayout(w);
    });
    invalidate(hwnd);
}

/// The face menu belongs to the selected item: it goes when nothing is
/// selected any more.
fn close_stale_fonts(w: &mut Win) {
    if w.pop == Some(Pop::Fonts) && w.selected.is_none() {
        w.pop = None;
    }
}

/// The text style of the selected Text or Date.
pub(super) fn selected_style(w: &Win) -> Option<style::TextStyle> {
    match &w.doc.as_ref()?.session.items.get(w.selected?)?.kind {
        ItemKind::Text(_, st) | ItemKind::Date(_, st) => Some(*st),
        _ => None,
    }
}

/// The colour of the selected Text, Date or Check.
pub(super) fn selected_tint(w: &Win) -> Option<style::Tint> {
    match &w.doc.as_ref()?.session.items.get(w.selected?)?.kind {
        ItemKind::Text(_, st) | ItemKind::Date(_, st) => Some(st.tint),
        ItemKind::Check(t) => Some(*t),
        _ => None,
    }
}

/// Applies `f` to the style of the selected Text or Date as one undo step.
/// A change of face, weight or slant measures the text again: the box keeps
/// its height and its left edge in its own upright frame, takes the measured
/// width (plus the 2 pt the editor adds) and is pulled back onto the page.
/// Underline and colour leave the size alone. Other kinds ignore it. The
/// style becomes the one new Text and Date items start from.
pub(super) fn restyle_selected(w: &mut Win, f: impl FnOnce(&mut style::TextStyle)) {
    let Some(i) = w.selected else { return };
    let Some(v) = view(w) else { return };
    let Some(old) = w.doc.as_ref().and_then(|d| d.session.items.get(i)).cloned() else { return };
    let (s, was, date) = match &old.kind {
        ItemKind::Text(s, st) => (s.clone(), *st, false),
        ItemKind::Date(s, st) => (s.clone(), *st, true),
        _ => return,
    };
    let mut st = was;
    f(&mut st);
    style::set_last_text(st);
    if st == was {
        return;
    }
    let nw = if was.changes_width(&st) {
        text_draw::width(&w.gfx.dwrite, &s, &st, old.h * TEXT_FILL) + 2.0
    } else {
        old.w
    };
    let kind = if date { ItemKind::Date(s, st) } else { ItemKind::Text(s, st) };
    let Some(d) = &mut w.doc else { return };
    d.session.restyle(i, kind, nw);
    if let Some(it) = d.session.items.get_mut(i) {
        // The width grows from the left edge of the upright frame, which is
        // the item's origin: for a half or three quarter turn that corner
        // moves with the width.
        let grown = it.w - old.w;
        match it.turn {
            Quarter::R180 => it.x -= grown,
            Quarter::R270 => it.y -= grown,
            _ => {}
        }
        let (bw, bh) = geom::occupied(it);
        (it.x, it.y) = layout::clamp_into(it.x, it.y, bw, bh, v.g.w, v.g.h);
    }
}

/// A colour for the selected item: a Check's own (as one undo step), or the
/// style of a Text or Date.
pub(super) fn tint_selected(w: &mut Win, t: style::Tint) {
    let Some(i) = w.selected else { return };
    let Some(d) = &mut w.doc else { return };
    let Some(it) = d.session.items.get(i) else { return };
    match it.kind {
        ItemKind::Check(old) => {
            style::set_last_check(t);
            if old != t {
                let width = it.w;
                d.session.restyle(i, ItemKind::Check(t), width);
            }
        }
        ItemKind::Text(..) | ItemKind::Date(..) => restyle_selected(w, |st| st.tint = t),
        _ => {}
    }
}

fn toggle_selected(w: &mut Win, hit: Hit) {
    restyle_selected(w, |st| match hit {
        Hit::Bold => st.bold = !st.bold,
        Hit::Italic => st.italic = !st.italic,
        Hit::Underline => st.underline = !st.underline,
        _ => {}
    });
}

/// Ctrl+B, Ctrl+I and Ctrl+U on the selected item, when no text is being
/// typed.
fn toggle_by_key(hwnd: HWND, vk: u16) {
    with(hwnd, |w| {
        if w.text.is_some() {
            return;
        }
        let hit = match vk {
            0x42 => Hit::Bold,
            0x49 => Hit::Italic,
            _ => Hit::Underline,
        };
        toggle_selected(w, hit);
    });
    invalidate(hwnd);
}

fn scale_selected(w: &mut Win, by: f32) {
    let Some(i) = w.selected else { return };
    let Some(v) = view(w) else { return };
    let Some(d) = &mut w.doc else { return };
    let Some(old) = d.session.items.get(i).cloned() else { return };
    let before = v.item_rect(&old);
    d.session.resize_item(i, old.w * by, old.h * by);
    // Grows and shrinks about its centre, kept on the page.
    let Some(it) = d.session.items.get_mut(i) else { return };
    let after = v.item_rect(it);
    let (cx, cy) = (before.x + before.w / 2.0, before.y + before.h / 2.0);
    let a = v.to_page(cx - after.w / 2.0, cy - after.h / 2.0);
    let b = v.to_page(cx + after.w / 2.0, cy + after.h / 2.0);
    let (bw, bh) = geom::occupied(it);
    let (x, y) = layout::clamp_into(a.0.min(b.0), a.1.min(b.1), bw, bh, v.g.w, v.g.h);
    it.x = x;
    it.y = y;
}

fn add_date_under(w: &mut Win) {
    let Some(i) = w.selected else { return };
    let Some(v) = view(w) else { return };
    let Some(d) = &w.doc else { return };
    let Some(sig) = d.session.items.get(i).cloned() else { return };
    let (x, y) = layout::below(&sig, DATE_SIZE.0, DATE_SIZE.1, DATE_GAP);
    let mut it = Item {
        kind: ItemKind::Date(today(), style::last_text()),
        page: sig.page,
        x,
        y,
        w: DATE_SIZE.0,
        h: DATE_SIZE.1,
        turn: sig.turn,
    };
    let (bw, bh) = geom::occupied(&it);
    (it.x, it.y) = layout::clamp_into(it.x, it.y, bw, bh, v.g.w, v.g.h);
    add_item(w, it);
}

fn remove_selected(hwnd: HWND) {
    with(hwnd, |w| {
        let Some(i) = w.selected.take() else { return };
        if let Some(d) = &mut w.doc {
            d.session.remove_item(i);
        }
        close_stale_fonts(w);
    });
}

/// Keys that change the document act only on an open document with no bar
/// or maker in front of it.
fn can_edit(w: &Win) -> bool {
    w.phase == Phase::Ready && w.ask == Ask::None && w.maker.is_none()
}

/// The maker goes away as if Batal was pressed.
fn close_maker(hwnd: HWND) {
    let mk = with(hwnd, |w| w.maker.take()).flatten();
    if let Some(m) = mk {
        unsafe {
            let _ = DestroyWindow(m);
        }
        take_focus(hwnd);
    }
    invalidate(hwnd);
}

fn undo_redo(hwnd: HWND, redo: bool) {
    with(hwnd, |w| {
        let Some(d) = &mut w.doc else { return };
        let done = if redo { d.session.redo() } else { d.session.undo() };
        if done {
            w.selected = None;
            if d.session.pages.get(w.cur).is_none_or(|p| p.deleted) {
                let first = d.session.live_pages().first().copied().unwrap_or(0);
                w.cur = first;
            }
            w.asked = None;
            want_render(w);
        }
        close_stale_fonts(w);
    });
    invalidate(hwnd);
}

fn nudge_selected(hwnd: HWND, dx: f32, dy: f32) {
    with(hwnd, |w| {
        let Some(i) = w.selected else { return };
        let Some(v) = view(w) else { return };
        let Some(d) = &mut w.doc else { return };
        let Some(it) = d.session.items.get(i) else { return };
        let (px, py) = nudge(v.shown(), dx, dy);
        let (bw, bh) = geom::occupied(it);
        let (x, y) = layout::clamp_into(it.x + px, it.y + py, bw, bh, v.g.w, v.g.h);
        let (mx, my) = (x - it.x, y - it.y);
        if mx != 0.0 || my != 0.0 {
            d.session.move_item(i, mx, my);
        }
    });
    invalidate(hwnd);
}

// ---- text ----

/// Opens the Teks EDIT at client point `at` (new), or over item `index`.
fn begin_text(hwnd: HWND, at: (f32, f32), index: Option<usize>) {
    open_text(hwnd, at, index, None, true);
}

/// The Teks EDIT at client point `at` (new) or over item `index`, holding
/// `typed` when given (else the item's text, or nothing). Returns its HWND.
fn open_text(hwnd: HWND, at: (f32, f32), index: Option<usize>, typed: Option<String>, focus: bool) -> Option<HWND> {
    commit_text(hwnd);
    let plan = with(hwnd, |w| {
        let v = view(w)?;
        let (initial, h_pt, rect) = match index.and_then(|i| w.doc.as_ref()?.session.items.get(i)) {
            Some(it) => {
                let ItemKind::Text(s, _) = &it.kind else { return None };
                (s.clone(), it.h, v.item_rect(it))
            }
            None => {
                let h = TEXT_H * v.scale;
                (String::new(), TEXT_H, Rect { x: at.0, y: at.1 - h / 2.0, w: 0.0, h })
            }
        };
        w.armed = None;
        w.selected = None;
        Some((initial, h_pt * v.scale, rect, v.to_page(at.0, at.1), ui_of(w)))
    })
    .flatten();
    let (initial, h_px, rect, page_at, ui) = plan?;
    let initial = typed.unwrap_or(initial);
    let px = (h_px * TEXT_FILL).max(8.0 * ui);
    let spec = text_edit::Spec {
        rect: (rect.x as i32, rect.y as i32, (rect.w + px * 4.0).max(160.0 * ui) as i32, (px * 1.5) as i32),
        initial: &initial,
        px,
        face: w!("Arial"),
        color: paint::INK,
        back: 0xFFFFFF,
        password: false,
        commit_on_blur: true,
    };
    let f = text_edit::create(hwnd, &spec)?;
    let edit = f.edit;
    let mut slot = Some(f);
    with(hwnd, |w| {
        if let Some(f) = slot.take() {
            w.text = Some((f, index, page_at));
            say(w, Key::SignTextHint, Tone::Hint);
        }
    });
    if let Some(f) = slot {
        text_edit::destroy(f);
        return None;
    }
    fit_text(hwnd);
    if focus {
        take_focus(edit);
    }
    invalidate(hwnd);
    Some(edit)
}

/// The Teks EDIT made again where its text sits on the page as shown now
/// (after a scale change): same text, same caret, the focus kept.
fn rebuild_text(hwnd: HWND) {
    let Some(Some((f, index, page_at))) = with(hwnd, |w| w.text.take()) else { return };
    let typed = text_edit::text(f.edit);
    let caret = text_edit::selection(f.edit);
    let had_focus = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() } == f.edit;
    // Unhooked before it goes, so losing the focus commits nothing.
    text_edit::destroy(f);
    let at = with(hwnd, |w| {
        view(w).map(|v| {
            let p = geom::page_to_view(v.g, v.extra, v.scale, page_at);
            (p.0 + v.page.x, p.1 + v.page.y)
        })
    })
    .flatten()
    .unwrap_or_default();
    if let Some(edit) = open_text(hwnd, at, index, Some(typed), had_focus) {
        text_edit::select(edit, caret);
    }
}

/// After a DPI change: the EDITs at the new font size (what was typed, the
/// caret and the focus kept) and the maker at the new scale.
fn rescale_fields(hwnd: HWND) {
    rebuild_text(hwnd);
    let pw = with(hwnd, |w| w.password.take()).flatten();
    if let Some(old) = pw {
        let typed = text_edit::text(old.edit);
        let had_focus = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() } == old.edit;
        text_edit::destroy(old);
        if let Some(e) = make_password(hwnd, &typed)
            && had_focus
        {
            take_focus(e);
        }
    }
    let mk = with(hwnd, |w| w.maker.map(|m| (m, ui_of(w), w.client))).flatten();
    if let Some((m, ui, client)) = mk {
        maker::rescale(m, ui);
        maker::place(m, client, ui);
    }
}

/// Grows the Teks EDIT with what is typed.
fn fit_text(hwnd: HWND) {
    with(hwnd, |w| {
        let Some((f, index, _)) = &w.text else { return };
        let st = match index.and_then(|i| w.doc.as_ref()?.session.items.get(i)).map(|it| &it.kind) {
            Some(ItemKind::Text(_, st)) => *st,
            _ => style::last_text(),
        };
        let ui = ui_of(w);
        let s = text_edit::text(f.edit);
        let mut r = RECT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(f.edit, &mut r);
        }
        let mut pt = POINT { x: r.left, y: r.top };
        unsafe {
            let _ = ScreenToClient(hwnd, &mut pt);
        }
        let h = r.bottom - r.top;
        let px = h as f32 / 1.5;
        let tw = text_draw::width(&w.gfx.dwrite, &format!("{s}MM"), &st, px);
        let width = tw.max(160.0 * ui) as i32;
        text_edit::move_to(f.edit, pt.x, pt.y, width, h);
    });
}

fn commit_text(hwnd: HWND) -> bool {
    finish_text(hwnd, true)
}

fn finish_text(hwnd: HWND, keep: bool) -> bool {
    let Some(Some((f, index, at))) = with(hwnd, |w| w.text.take()) else { return false };
    let typed = text_edit::text(f.edit).trim().to_owned();
    text_edit::destroy(f);
    with(hwnd, |w| {
        if w.note.as_ref().is_some_and(|(n, _)| n == tr(Key::SignTextHint)) {
            w.note = None;
        }
        if !keep || typed.is_empty() {
            return;
        }
        let Some(v) = view(w) else { return };
        let width = |h: f32, st: &style::TextStyle| {
            text_draw::width(&w.gfx.dwrite, &typed, st, h * TEXT_FILL) + 2.0
        };
        match index {
            Some(i) => {
                let Some(d) = &mut w.doc else { return };
                let Some(old) = d.session.items.get(i).cloned() else { return };
                if matches!(&old.kind, ItemKind::Text(t, _) if *t == typed) {
                    // Nothing changed: no undo step, nothing to save.
                    w.selected = Some(i);
                    return;
                }
                let old_style = match &old.kind {
                    ItemKind::Text(_, st) => *st,
                    _ => style::last_text(),
                };
                let nw = width(old.h, &old_style).max(8.0);
                // One undo step: the resize takes the snapshot, the text
                // changes with it.
                d.session.resize_item(i, nw, old.h);
                if let Some(it) = d.session.items.get_mut(i) {
                    it.kind = ItemKind::Text(typed, old_style);
                    let (bw, bh) = geom::occupied(it);
                    (it.x, it.y) = layout::clamp_into(it.x, it.y, bw, bh, v.g.w, v.g.h);
                }
                w.selected = Some(i);
            }
            None => {
                let turn = geom::upright_turn(v.g, v.extra);
                let iw = width(TEXT_H, &style::last_text()).max(8.0);
                // The EDIT's left edge and middle are where the text sits.
                let (bw, bh) = if matches!(turn, Quarter::R90 | Quarter::R270) { (TEXT_H, iw) } else { (
                    iw,
                    TEXT_H,
                ) };
                let left_mid = at;
                let (cx, cy) = {
                    let p = geom::page_to_view(v.g, v.extra, v.scale, left_mid);
                    v.to_page(p.0 + v.page.x + iw * v.scale / 2.0, p.1 + v.page.y)
                };
                let (x, y) = layout::clamp_into(cx - bw / 2.0, cy - bh / 2.0, bw, bh, v.g.w, v.g.h);
                add_item(w, Item { kind: ItemKind::Text(typed, style::last_text()), page: w.cur, x, y, w: iw, h: TEXT_H, turn });
            }
        }
    });
    if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        take_focus(hwnd);
    }
    invalidate(hwnd);
    true
}

// ---- saving ----

enum SaveJob {
    Pdf(Arc<Opened>),
    Image { path: PathBuf, fmt: ImageFormat },
}

fn do_save(hwnd: HWND, confirmed: bool) {
    commit_text(hwnd);
    let go = with(hwnd, |w| {
        if w.phase != Phase::Ready {
            return false;
        }
        let Some(d) = &w.doc else { return false };
        if !d.session.has_changes() {
            // An unsigned copy is not worth a file.
            w.ask = Ask::None;
            say(w, Key::SignNothingAdded, Tone::Hint);
            relayout(w);
            return false;
        }
        if !confirmed && let Some(o) = &d.opened {
            let incremental = pdf_write::save_is_incremental(o, &d.session.pages);
            if breaks_signature(o.signed, incremental, d.session.has_rotated()) {
                w.ask = Ask::Breaks;
                w.focus = Some(Hit::NotNow);
                relayout(w);
                return false;
            }
        }
        w.ask = Ask::None;
        true
    })
    .unwrap_or(false);
    if go {
        with(hwnd, start_save);
    }
    invalidate(hwnd);
}

fn start_save(w: &mut Win) {
    let Some(d) = &w.doc else { return };
    let job = match &d.opened {
        Some(o) => SaveJob::Pdf(o.clone()),
        None => {
            let ext = out_ext(w);
            let fmt = match ext.as_str() {
                "pdf" => ImageFormat::Pdf,
                "jpg" | "jpeg" => ImageFormat::Jpg,
                _ => ImageFormat::Png,
            };
            SaveJob::Image { path: w.src.path.clone(), fmt }
        }
    };
    let pages: Vec<Page> = d.session.pages.clone();
    let items: Vec<Item> = d.session.items.clone();
    let ext = out_ext(w);
    let (src, fallback) = naming(&w.src.path, w.demo);
    let l = lang();
    let hwnd = w.hwnd.0 as isize;
    let inbox = w.inbox.clone();
    let (src_path, demo, image) = (w.src.path.clone(), w.demo, d.image);
    let spawned = std::thread::Builder::new().name("sign-save".into()).spawn(move || {
        let result = {
            let _com = Com::init();
            let saved = match save_bytes(job, &pages, &items) {
                Ok(bytes) => write_out(&src, &ext, l, &fallback, &bytes),
                Err(k) => Err(k),
            };
            match saved {
                Ok(p) => Saved::To(p),
                Err(k) => Saved::Failed(k, outs_now(&src_path, demo, image)),
            }
        };
        if let Ok(mut i) = inbox.lock() {
            i.saved = Some(result);
        }
        post(hwnd, WM_SAVED);
    });
    match spawned {
        Ok(h) => {
            w.save_job = Some(h);
            w.phase = Phase::Saving;
            w.note = None;
        }
        Err(_) => say(w, Key::SignSaveFailed, Tone::Error),
    }
    relayout(w);
}

fn save_bytes(job: SaveJob, pages: &[Page], items: &[Item]) -> Result<Vec<u8>, Key> {
    match job {
        SaveJob::Pdf(o) => {
            let items = encodable(items)?;
            pdf_write::save(&o, pages, &items).map_err(|e| save_error_key(&e))
        }
        SaveJob::Image { path, fmt } => {
            let page = pages.first().ok_or(Key::SignSaveFailed)?;
            let mine: Vec<Item> = items.iter().filter(|i| i.page == 0).cloned().collect();
            let composed = image_out::compose_file(&path, page, &mine).ok_or(Key::SignSaveFailed)?;
            image_out::save_image(composed, fmt).map_err(|_| Key::SignSaveFailed)
        }
    }
}

/// Text the PDF's Helvetica cannot write (outside WinAnsi) becomes a picture
/// of the text in Arial, the size of its box, so the writer only sees text it
/// can encode.
pub(super) fn encodable(items: &[Item]) -> Result<Vec<Item>, Key> {
    items
        .iter()
        .map(|it| match &it.kind {
            ItemKind::Text(s, st) | ItemKind::Date(s, st) if winansi::encode(s).is_none() => {
                let m = paint::text_mark(s, st, it.w, it.h).ok_or(Key::SignSaveFailed)?;
                Ok(Item { kind: ItemKind::Signature(Arc::new(m)), ..it.clone() })
            }
            _ => Ok(it.clone()),
        })
        .collect()
}

fn write_out(src: &Path, ext: &str, l: Lang, fallback: &Path, bytes: &[u8]) -> Result<PathBuf, Key> {
    // The folder can turn out unwritable (or the name taken) between the
    // check and the write: then the fallback folder, then give up.
    for writable in [true, false] {
        let path = names::output_path(
            src,
            ext,
            l,
            |p| p.exists(),
            |d| writable && names::dir_is_writable(d),
            fallback,
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Some(p) = crate::screenshot::win::write_at(&path, bytes) {
            return Ok(p);
        }
    }
    Err(Key::SignSaveFailed)
}

fn on_saved(hwnd: HWND) {
    let done = with(hwnd, |w| {
        if let Some(job) = w.save_job.take() {
            let _ = job.join();
        }
        let saved = w.inbox.lock().ok().and_then(|mut i| i.saved.take())?;
        match saved {
            Saved::To(p) => Some((w.owner, p)),
            Saved::Failed(k, outs) => {
                w.phase = Phase::Ready;
                say(w, k, Tone::Error);
                w.outs = Some(outs);
                refresh_out(w);
                relayout(w);
                None
            }
        }
    })
    .flatten();
    match done {
        Some((owner, path)) => {
            super::report(owner, Report::Saved(path));
            destroy(hwnd);
        }
        None => invalidate(hwnd),
    }
}

// ---- closing ----

fn destroy(hwnd: HWND) {
    unsafe {
        // Hidden first, so the window is gone the moment it is asked to go.
        let _ = ShowWindow(hwnd, SW_HIDE);
        let _ = DestroyWindow(hwnd);
    }
}

fn request_close(hwnd: HWND) {
    commit_text(hwnd);
    let mk = with(hwnd, |w| w.maker.take()).flatten();
    if let Some(m) = mk {
        unsafe {
            let _ = DestroyWindow(m);
        }
    }
    let action = with(hwnd, |w| {
        let dirty = w.doc.as_ref().is_some_and(|d| d.session.dirty());
        let a = close_action(w.phase, dirty);
        if a == CloseAction::Ask {
            w.ask = Ask::Discard;
            w.focus = Some(Hit::Keep);
            w.pop = None;
            relayout(w);
        }
        a
    });
    match action {
        Some(CloseAction::Destroy) => destroy(hwnd),
        Some(CloseAction::Ask) => {
            take_focus(hwnd);
            invalidate(hwnd);
        }
        _ => {}
    }
}

fn on_escape(hwnd: HWND) {
    let step = with(hwnd, |w| {
        let s = esc_step(w.maker.is_some(), w.ask, w.pop.is_some(), w.armed.is_some(), w.selected.is_some());
        match s {
            EscStep::Maker => {}
            EscStep::Bar => {
                w.ask = Ask::None;
                w.focus = None;
                relayout(w);
            }
            EscStep::Popover => w.pop = None,
            EscStep::Disarm => {
                w.armed = None;
                w.note = None;
            }
            EscStep::Deselect => w.selected = None,
            EscStep::Close => {}
        }
        s
    });
    match step {
        Some(EscStep::Maker) => close_maker(hwnd),
        Some(EscStep::Close) => request_close(hwnd),
        Some(_) => invalidate(hwnd),
        None => {}
    }
}

// ---- mouse ----

fn xy(lparam: LPARAM) -> (f32, f32) {
    ((lparam.0 & 0xFFFF) as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as i16 as f32)
}

fn on_down(hwnd: HWND, lparam: LPARAM, double: bool) {
    let (x, y) = xy(lparam);
    // A click while typing only finishes the text (the click is not a place).
    let typing = with(hwnd, |w| w.text.is_some()).unwrap_or(false);
    if typing {
        commit_text(hwnd);
        let on_stage = with(hwnd, |w| w.layout.stage.contains(x, y)).unwrap_or(false);
        if on_stage {
            return;
        }
    }
    enum Then {
        Nothing,
        Button,
        Text((f32, f32), Option<usize>),
        Capture,
    }
    let then = with(hwnd, |w| {
        w.focus = None;
        if w.maker.is_some() {
            return Then::Nothing;
        }
        let hit = hit_at(w, x, y);
        let from_pop = popover(w).is_some_and(|p| p.panel.contains(x, y));
        if w.pop.is_some() && !from_pop {
            w.pop = None;
            if !matches!(hit, Some(h) if w.layout.buttons.iter().any(|(b, _)| *b == h)) {
                return Then::Nothing;
            }
        }
        if w.ask != Ask::None && !matches!(
            hit,
            Some(Hit::Discard | Hit::Keep | Hit::SaveAnyway | Hit::NotNow),
        ) {
            return Then::Nothing;
        }
        let Some(hit) = hit else { return Then::Nothing };
        let Some(v) = view(w).filter(|_| w.phase == Phase::Ready) else {
            return if is_button(hit) {
                w.pressed = Some(hit);
                Then::Button
            } else {
                Then::Nothing
            };
        };
        match hit {
            Hit::Handle(corner) => {
                let Some(i) = w.selected else { return Then::Nothing };
                let Some(it) = shown_item(w, &v, i) else { return Then::Nothing };
                w.drag = Some(Drag::Resize { idx: i, corner, start: v.item_rect(&it), press: (x, y), now: (x, y) });
                Then::Capture
            }
            Hit::Item(i) if w.armed.is_none() => {
                let is_text = w.doc.as_ref().and_then(|d| d.session.items.get(i)).is_some_and(|it| {
                    matches!(it.kind, ItemKind::Text(..))
                });
                if double && is_text {
                    return Then::Text((x, y), Some(i));
                }
                w.selected = Some(i);
                let at = v.to_page(x, y);
                w.drag = Some(Drag::Move { idx: i, from: at, now: at });
                Then::Capture
            }
            Hit::Item(_) | Hit::Page => {
                let Some(armed) = w.armed.clone() else {
                    w.selected = None;
                    return Then::Nothing;
                };
                if armed.tool == Tool::Text {
                    return Then::Text((x, y), None);
                }
                place(w, &v, armed.tool, armed.mark, v.to_page(x, y));
                w.note = None;
                Then::Nothing
            }
            Hit::Stage => {
                w.selected = None;
                Then::Nothing
            }
            h => {
                w.pressed = Some(h);
                Then::Button
            }
        }
    });
    match then {
        Some(Then::Capture) | Some(Then::Button) => unsafe {
            SetCapture(hwnd);
        },
        Some(Then::Text(at, index)) => begin_text(hwnd, at, index),
        _ => {}
    }
    invalidate(hwnd);
}

fn is_button(h: Hit) -> bool {
    !matches!(h, Hit::Handle(_) | Hit::Item(_) | Hit::Page | Hit::Stage | Hit::Inert)
}

fn on_move(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = xy(lparam);
    let changed = with(hwnd, |w| {
        let mut changed = false;
        let v = view(w);
        match (&mut w.drag, v) {
            (Some(Drag::Move { now, .. }), Some(v)) => {
                *now = v.to_page(x, y);
                changed = true;
            }
            (Some(Drag::Resize { now, .. }), Some(_)) => {
                *now = (x, y);
                changed = true;
            }
            _ => {}
        }
        let hover = if w.drag.is_none() { hit_at(w, x, y).filter(|h| is_button(*h)) } else { None };
        if hover != w.hover {
            w.hover = hover;
            changed = true;
        }
        changed
    })
    .unwrap_or(false);
    if changed {
        invalidate(hwnd);
    }
}

/// Ends a drag with one session edit (one undo step).
fn finish_drag(w: &mut Win) {
    let Some(drag) = w.drag.take() else { return };
    let Some(v) = view(w) else { return };
    match drag {
        Drag::Move { idx, .. } => {
            let Some(moved) = shown_item_with(w, &v, idx, drag) else { return };
            let Some(d) = &mut w.doc else { return };
            let Some(it) = d.session.items.get(idx) else { return };
            let (dx, dy) = (moved.x - it.x, moved.y - it.y);
            if dx.abs() > 0.01 || dy.abs() > 0.01 {
                d.session.move_item(idx, dx, dy);
            }
        }
        Drag::Resize { idx, corner, start, press, now } => {
            let f = layout::drag_factor(start, corner, press, now);
            let Some(d) = &mut w.doc else { return };
            let Some(old) = d.session.items.get(idx).cloned() else { return };
            if (f - 1.0).abs() < 0.005 {
                return;
            }
            d.session.resize_item(idx, old.w * f, old.h * f);
            if let Some(it) = d.session.items.get_mut(idx) {
                place_at_anchor(it, &v, start, corner);
            }
        }
    }
}

fn shown_item_with(w: &mut Win, v: &View, i: usize, drag: Drag) -> Option<Item> {
    w.drag = Some(drag);
    let it = shown_item(w, v, i);
    w.drag = None;
    it
}

fn on_up(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = xy(lparam);
    let press = with(hwnd, |w| {
        finish_drag(w);
        let pressed = w.pressed.take();
        let now = hit_at(w, x, y);
        pressed.filter(|p| Some(*p) == now)
    })
    .flatten();
    unsafe {
        let _ = ReleaseCapture();
    }
    if let Some(hit) = press {
        act(hwnd, hit, false);
    }
    invalidate(hwnd);
}

fn on_wheel(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let mut pt = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
    }
    let (x, y) = (pt.x as f32, pt.y as f32);
    // Text being typed belongs to the page it was started on: it is
    // finished first, and this turn of the wheel does nothing else.
    if with(hwnd, |w| w.text.is_some()) == Some(true) {
        commit_text(hwnd);
        return;
    }
    with(hwnd, |w| {
        if w.phase != Phase::Ready || w.maker.is_some() {
            return;
        }
        if w.layout.rail.contains(x, y) {
            let rows = rail(w);
            let ui = ui_of(w);
            let max = (layout::rail_content_h(&rows, w.layout.rail, ui, w.rail_scroll) - w.layout.rail.h).max(0.0);
            w.rail_scroll = (w.rail_scroll - delta as f32 / 120.0 * 60.0 * ui).clamp(0.0, max);
            return;
        }
        if w.layout.stage.contains(x, y) {
            // The whole page is always in view, so the wheel turns pages.
            w.wheel += delta;
            while w.wheel >= 120 {
                w.wheel -= 120;
                step_page(w, -1);
            }
            while w.wheel <= -120 {
                w.wheel += 120;
                step_page(w, 1);
            }
        }
    });
    invalidate(hwnd);
}

fn cursor_for(w: &Win, x: f32, y: f32) -> PCWSTR {
    if w.maker.is_some() || w.ask != Ask::None {
        return IDC_ARROW;
    }
    match hit_at(w, x, y) {
        Some(Hit::Handle(Corner::TopLeft | Corner::BottomRight)) => IDC_SIZENWSE,
        Some(Hit::Handle(_)) => IDC_SIZENESW,
        Some(Hit::Item(_)) if w.armed.is_none() => IDC_SIZEALL,
        Some(Hit::Item(_) | Hit::Page) if w.armed.as_ref().is_some_and(|a| a.tool == Tool::Text) => IDC_IBEAM,
        Some(Hit::Item(_) | Hit::Page) if w.armed.is_some() => IDC_CROSS,
        Some(h) if is_button(h) => IDC_HAND,
        _ => IDC_ARROW,
    }
}

// ---- keys ----

fn on_key(hwnd: HWND, vk: u16) {
    let ctrl = key_down(VK_CONTROL.0);
    let shift = key_down(VK_SHIFT.0);
    let (ask, focus, editing) = with(hwnd, |w| (w.ask, w.focus, can_edit(w))).unwrap_or((Ask::None, None, false));
    // Enter on a bar takes the safe answer unless the keyboard moved to the
    // other one.
    if ask != Ask::None && vk == VK_RETURN.0 && focus.is_none() {
        act(hwnd, if ask == Ask::Discard { Hit::Keep } else { Hit::NotNow }, true);
        return;
    }
    let edit_key = (ctrl && matches!(vk, 0x53 | 0x59 | 0x5A | 0x42 | 0x49 | 0x55))
        || [VK_DELETE.0, VK_BACK.0, VK_PRIOR.0, VK_NEXT.0, VK_LEFT.0, VK_RIGHT.0, VK_UP.0, VK_DOWN.0].contains(&vk);
    if edit_key && !editing {
        return;
    }
    match vk {
        v if v == VK_ESCAPE.0 => on_escape(hwnd),
        v if v == VK_TAB.0 => {
            with(hwnd, |w| {
                let ring = focus_ring(w);
                if ring.is_empty() {
                    return;
                }
                let at = w.focus.and_then(|f| ring.iter().position(|h| *h == f));
                let n = ring.len();
                let next = match (at, shift) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                };
                w.focus = Some(ring[next]);
            });
            invalidate(hwnd);
        }
        v if v == VK_RETURN.0 || v == VK_SPACE.0 => {
            let f = with(hwnd, |w| w.focus).flatten();
            if let Some(h) = f {
                act(hwnd, h, true);
            }
        }
        0x53 if ctrl => do_save(hwnd, false),
        0x5A if ctrl && shift => undo_redo(hwnd, true),
        0x5A if ctrl => undo_redo(hwnd, false),
        0x59 if ctrl => undo_redo(hwnd, true),
        0x42 | 0x49 | 0x55 if ctrl => toggle_by_key(hwnd, vk),
        v if v == VK_DELETE.0 || v == VK_BACK.0 => {
            remove_selected(hwnd);
            invalidate(hwnd);
        }
        v if v == VK_PRIOR.0 || v == VK_NEXT.0 => {
            with(hwnd, |w| {
                if w.phase == Phase::Ready {
                    step_page(w, if v == VK_PRIOR.0 { -1 } else { 1 });
                }
            });
            invalidate(hwnd);
        }
        v if [VK_LEFT.0, VK_RIGHT.0, VK_UP.0, VK_DOWN.0].contains(&v) => {
            let step = if shift { 10.0 } else { 1.0 };
            let (dx, dy) = match v {
                x if x == VK_LEFT.0 => (-step, 0.0),
                x if x == VK_RIGHT.0 => (step, 0.0),
                x if x == VK_UP.0 => (0.0, -step),
                _ => (0.0, step),
            };
            nudge_selected(hwnd, dx, dy);
        }
        _ => {}
    }
}

// ---- the window procedure ----

fn on_size(hwnd: HWND) {
    with(hwnd, |w| {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rc);
        }
        w.client = ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32);
        if let Some(t) = &w.target {
            t.resize(w.client.0, w.client.1);
        }
        relayout(w);
        place_password(w);
        if let Some(m) = w.maker {
            maker::place(m, w.client, ui_of(w));
        }
        let rows = rail(w);
        w.rail_scroll = layout::rail_scroll_to(&rows, w.layout.rail, w.cur, ui_of(w), w.rail_scroll);
        unsafe {
            let _ = SetTimer(Some(hwnd), ID_RERENDER, RERENDER_MS, None);
        }
    });
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let painted = with(hwnd, paint::paint);
            if painted.is_none() {
                unsafe {
                    let _ = ValidateRect(Some(hwnd), None);
                }
            }
            LRESULT(0)
        }
        WM_SIZE => {
            on_size(hwnd);
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == ID_RERENDER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), ID_RERENDER);
            }
            with(hwnd, want_render);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
            let ui = dpi as f32 / 96.0;
            let mut rc = RECT { left: 0, top: 0, right: (MIN_W * ui) as i32, bottom: (MIN_H * ui) as i32 };
            unsafe {
                let _ = AdjustWindowRectExForDpi(
                    &mut rc,
                    WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                    false,
                    WINDOW_EX_STYLE(0),
                    dpi,
                );
                if let Some(info) = (lparam.0 as *mut MINMAXINFO).as_mut() {
                    info.ptMinTrackSize.x = rc.right - rc.left;
                    info.ptMinTrackSize.y = rc.bottom - rc.top;
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = ((wparam.0 >> 16) & 0xFFFF) as u32;
            with(hwnd, |w| {
                w.dpi = dpi.max(96);
                w.labels = measure_labels(&w.gfx, ui_of(w));
                w.thumbs_asked.clear();
                w.thumbs.clear();
            });
            unsafe {
                if let Some(s) = (lparam.0 as *const RECT).as_ref() {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        s.left,
                        s.top,
                        s.right - s.left,
                        s.bottom - s.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
            on_size(hwnd);
            // The EDIT fonts and the maker follow the new scale.
            rescale_fields(hwnd);
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            on_down(hwnd, lparam, false);
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            on_down(hwnd, lparam, true);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            on_move(hwnd, lparam);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_up(hwnd, lparam);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            if lparam.0 != hwnd.0 as isize {
                with(hwnd, |w| {
                    finish_drag(w);
                    w.pressed = None;
                });
                invalidate(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            on_wheel(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            const HTCLIENT: isize = 1;
            if lparam.0 & 0xFFFF != HTCLIENT {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }
            let mut at = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut at);
                let _ = ScreenToClient(hwnd, &mut at);
            }
            let cursor = with(hwnd, |w| cursor_for(w, at.x as f32, at.y as f32)).unwrap_or(IDC_ARROW);
            unsafe {
                if let Ok(c) = LoadCursorW(None, cursor) {
                    SetCursor(Some(c));
                }
            }
            LRESULT(1)
        }
        WM_KEYDOWN => {
            on_key(hwnd, (wparam.0 & 0xFFFF) as u16);
            LRESULT(0)
        }
        WM_SIGN_EDIT => {
            let (text, pw) = with(hwnd, |w| {
                (w.text.as_ref().map(|t| t.0.edit.0 as isize), w.password.as_ref().map(|f| f.edit.0 as isize))
            })
            .unwrap_or((None, None));
            if text_edit::msg_is_current(text, lparam.0) {
                match wparam.0 {
                    EDIT_CANCEL => {
                        finish_text(hwnd, false);
                    }
                    _ => {
                        commit_text(hwnd);
                    }
                }
            } else if text_edit::msg_is_current(pw, lparam.0) {
                match wparam.0 {
                    EDIT_COMMIT => submit_password(hwnd),
                    EDIT_CANCEL => request_close(hwnd),
                    EDIT_TAB => {
                        with(hwnd, |w| w.focus = Some(Hit::OpenPassword));
                        take_focus(hwnd);
                        invalidate(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            if text_edit::is_change(wparam) {
                fit_text(hwnd);
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let from = lparam.0;
            let brush = with(hwnd, |w| {
                let f = w
                    .text
                    .as_ref()
                    .map(|t| &t.0)
                    .filter(|f| f.edit.0 as isize == from)
                    .or(w.password.as_ref().filter(|f| f.edit.0 as isize == from))?;
                Some(text_edit::ctl_color(f, wparam))
            })
            .flatten();
            match brush {
                Some(b) => b,
                None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        WM_RENDERED => {
            on_rendered(hwnd);
            LRESULT(0)
        }
        WM_OPENED => {
            on_opened(hwnd);
            LRESULT(0)
        }
        WM_SAVED => {
            on_saved(hwnd);
            LRESULT(0)
        }
        WM_MAKER => {
            on_maker(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            request_close(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                let _ = KillTimer(Some(hwnd), ID_RERENDER);
            }
            let removed = WINDOWS.with(|m| m.try_borrow_mut().ok().and_then(|mut m| m.remove(&(hwnd.0 as isize))));
            if let Some(w) = removed {
                let demo = w.demo;
                // Ends the threads, hands the renderer to a reaper, removes
                // the Office copy.
                drop(w);
                // The last window took the saved signatures out of memory
                // with it.
                if WINDOWS.with(|m| m.try_borrow().is_ok_and(|m| m.is_empty())) {
                    super::forget_store();
                }
                if demo && ON_SCREEN {
                    unsafe { PostQuitMessage(0) };
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_hidden_and_closes() {
        let dir = crate::phone::files::tests::temp_dir("sign-window");
        let path = dir.join("Contoh dokumen.pdf");
        std::fs::write(&path, super::super::demo_pdf().unwrap()).unwrap();
        let src = Source { path: path.clone(), kind: Kind::Pdf };
        assert!(open(HWND::default(), src.clone(), true));
        let hwnd = WINDOWS.with(|m| m.borrow().keys().next().copied()).map(|h| HWND(h as *mut c_void));
        let hwnd = hwnd.expect("a window");
        unsafe {
            assert!(!windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool());
        }
        // The same file again is the same window.
        assert!(open(HWND::default(), src, true));
        assert_eq!(WINDOWS.with(|m| m.borrow().len()), 1);
        // Let the opening worker finish, so a renderer exists to be stopped.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            if with(hwnd, |w| w.inbox.lock().map(|i| i.opened.is_some()).unwrap_or(false)) == Some(true) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        on_opened(hwnd);
        assert_eq!(with(hwnd, |w| w.phase), Some(Phase::Ready));
        assert_eq!(with(hwnd, |w| w.doc.as_ref().map(|d| d.session.pages.len())), Some(Some(3)));
        assert!(ENGINES.load(Ordering::SeqCst) >= 1);
        the_open_window_behaves(hwnd);
        release();
        assert_eq!(WINDOWS.with(|m| m.borrow().len()), 0);
        unsafe {
            assert!(!IsWindow(Some(hwnd)).as_bool());
        }
        assert_eq!(ENGINES.load(Ordering::SeqCst), 0, "no sign-render thread left");
        assert!(!super::super::store_loaded(), "the saved signatures left memory with the last window");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// How many undo steps the session holds (undone and redone again).
    fn undo_steps(hwnd: HWND) -> usize {
        with(hwnd, |w| {
            let s = &mut w.doc.as_mut().expect("open").session;
            let mut n = 0;
            while s.undo() {
                n += 1;
            }
            for _ in 0..n {
                s.redo();
            }
            n
        })
        .unwrap_or(0)
    }

    fn lp(x: f32, y: f32) -> LPARAM {
        let (x, y) = (x.round() as i32 as u32 & 0xFFFF, y.round() as i32 as u32 & 0xFFFF);
        LPARAM((y << 16 | x) as isize)
    }

    /// The window-level rules, on the hidden window of `opens_hidden_and_closes`
    /// (one window per test run keeps the renderer count exact).
    fn the_open_window_behaves(hwnd: HWND) {
        // Simpan with nothing placed writes nothing and says so.
        do_save(hwnd, false);
        assert_eq!(
            with(hwnd, |w| (w.phase, w.note.clone(), w.save_job.is_none())),
            Some((Phase::Ready, Some((tr(Key::SignNothingAdded).to_owned(), Tone::Hint)), true))
        );

        // A date at the page centre, selected.
        with(hwnd, |w| {
            let v = view(w).expect("a page");
            place(w, &v, Tool::Date, None, page_centre(&v));
        });
        let sel = with(hwnd, |w| w.selected).flatten().expect("selected");
        let size = |hwnd| with(
            hwnd,
            |w| w.doc.as_ref().map(|d| (d.session.items[sel].w, d.session.items[sel].h)),
        );
        let before = size(hwnd);
        assert_eq!(undo_steps(hwnd), 1);
        // Pressing each handle and letting go without a move: same size, no
        // undo step.
        let hs = with(hwnd, |w| handle_rects(w, &view(w).expect("page"))).unwrap_or_default();
        assert_eq!(hs.len(), 4);
        for (_, r) in &hs {
            let at = lp(r.x + r.w / 2.0, r.y + r.h / 2.0);
            on_down(hwnd, at, false);
            assert!(matches!(with(hwnd, |w| w.drag).flatten(), Some(Drag::Resize { .. })));
            on_up(hwnd, at);
            assert_eq!(size(hwnd), before);
        }
        assert_eq!(undo_steps(hwnd), 1, "a press without a move is no edit");

        // Re-committing a text unchanged adds nothing.
        let t = with(hwnd, |w| {
            let page = w.cur;
            let d = w.doc.as_mut().expect("open");
            let item = Item { kind: ItemKind::Text("Halo".into(), Default::default()), page, x: 50.0, y: 50.0, w: 30.0, h: 14.0, turn: Quarter::R0 };
            d.session.add(item)
        })
        .expect("added");
        let steps = undo_steps(hwnd);
        begin_text(hwnd, (0.0, 0.0), Some(t));
        assert!(with(hwnd, |w| w.text.is_some()).unwrap_or(false));
        assert!(finish_text(hwnd, true));
        assert_eq!(undo_steps(hwnd), steps, "same text, same document");

        // The picker shows the same saved mark on every paint and hit test:
        // the bitmap cache does not grow.
        super::super::keep_mark(
            &Mark { w: 2, h: 1, rgba: vec![0, 0, 255, 255, 0, 0, 255, 255] },
            false,
            true,
        );
        with(hwnd, |w| w.pop = Some(Pop::Picker { initials: false }));
        with(hwnd, paint::paint);
        let n = with(hwnd, |w| w.mark_bmps.len()).unwrap_or(0);
        assert!(n >= 1, "the tile was drawn");
        for i in 0..20 {
            with(hwnd, |w| hit_at(w, 200.0 + i as f32, 70.0));
            with(hwnd, paint::paint);
        }
        assert_eq!(with(hwnd, |w| w.mark_bmps.len()), Some(n));
        with(hwnd, |w| w.pop = None);

        // Enter on the close bar keeps editing; so does Esc. Delete does
        // nothing while the bar is up.
        request_close(hwnd);
        assert_eq!(with(hwnd, |w| (w.ask, w.focus)), Some((Ask::Discard, Some(Hit::Keep))));
        with(hwnd, |w| w.selected = Some(sel));
        let count = with(hwnd, |w| w.doc.as_ref().map_or(0, |d| d.session.items.len()));
        on_key(hwnd, VK_DELETE.0);
        assert_eq!(with(hwnd, |w| w.doc.as_ref().map_or(0, |d| d.session.items.len())), count);
        on_key(hwnd, VK_RETURN.0);
        assert_eq!(with(hwnd, |w| w.ask), Some(Ask::None));
        request_close(hwnd);
        with(hwnd, |w| w.focus = None);
        on_key(hwnd, VK_RETURN.0);
        assert_eq!(with(hwnd, |w| w.ask), Some(Ask::None));
        unsafe {
            assert!(IsWindow(Some(hwnd)).as_bool());
        }

        // With the maker up, Esc closes the maker, never the window.
        open_maker(hwnd, false, false);
        assert!(with(hwnd, |w| w.maker.is_some()).unwrap_or(false));
        on_key(hwnd, VK_ESCAPE.0);
        assert_eq!(with(hwnd, |w| (w.maker.is_none(), w.ask)), Some((true, Ask::None)));
        unsafe {
            assert!(IsWindow(Some(hwnd)).as_bool());
        }

        // A DPI change while typing: the Teks EDIT comes back at the new
        // font size with the same text and caret, still open.
        let centre = with(hwnd, |w| view(w).map(|v| (v.page.x + v.page.w / 2.0, v.page.y + v.page.h / 2.0)))
            .flatten()
            .expect("page");
        begin_text(hwnd, centre, None);
        let old = with(hwnd, |w| w.text.as_ref().map(|t| t.0.edit)).flatten().expect("an EDIT");
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(old, w!("Rp 1.500"));
        }
        text_edit::select(old, (3, 5));
        let px_before = text_edit::font_px(old);
        let mut r = RECT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut r);
        }
        with(hwnd, |w| w.dpi = 192);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                r.left,
                r.top,
                (r.right - r.left) * 2,
                (r.bottom - r.top) * 2,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        on_size(hwnd);
        rescale_fields(hwnd);
        let new = with(hwnd, |w| w.text.as_ref().map(|t| t.0.edit)).flatten().expect("still typing");
        assert_ne!(new, old, "made again");
        assert_eq!(text_edit::text(new), "Rp 1.500");
        assert_eq!(text_edit::selection(new), (3, 5));
        let px_after = text_edit::font_px(new);
        assert!(px_after as f32 >= px_before as f32 * 1.6, "{px_before} -> {px_after}");
        finish_text(hwnd, false);

        format_controls_behave(hwnd);
    }

    /// The format controls of the item bar, on the open window.
    fn format_controls_behave(hwnd: HWND) {
        use style::{Family, TextStyle, Tint};
        let _lock = style::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        with(hwnd, |w| {
            w.pop = None;
            w.selected = None;
        });
        let page_w = with(hwnd, |w| view(w).map(|v| v.g.w)).flatten().expect("a page");
        let add = |kind: ItemKind, x: f32, w_pt: f32| {
            with(hwnd, |w| {
                let page = w.cur;
                let d = w.doc.as_mut().expect("open");
                let i = d.session.add(Item { kind, page, x, y: 100.0, w: w_pt, h: 14.0, turn: Quarter::R0 });
                w.selected = Some(i);
                i
            })
            .expect("added")
        };
        let item = |hwnd, i: usize| with(hwnd, |w| w.doc.as_ref().map(|d| d.session.items[i].clone())).flatten().expect("item");

        // The bar for each kind: format first, then the actions.
        let t = TextStyle::default();
        let text = ItemKind::Text("Halo dunia semuanya".into(), t);
        let all_tints = [Hit::Tint(Tint::Black), Hit::Tint(Tint::Blue), Hit::Tint(Tint::Red)];
        let mut want = vec![Hit::Family, Hit::Bold, Hit::Italic, Hit::Underline];
        want.extend(all_tints);
        want.extend([Hit::Smaller, Hit::Bigger, Hit::Remove]);
        assert_eq!(float_hits(&text), want);
        assert_eq!(float_hits(&ItemKind::Date("1 Mei 2026".into(), t)), want);
        let mut check = all_tints.to_vec();
        check.extend([Hit::Smaller, Hit::Bigger, Hit::Remove]);
        assert_eq!(float_hits(&ItemKind::Check(Tint::Black)), check);

        // A text at the right edge made bold serif: the width follows the
        // measure, the height stays, the box stays on the page, and one undo
        // brings everything back.
        let i = add(text.clone(), page_w - 40.0, 38.0);
        let before = item(hwnd, i);
        let steps = undo_steps(hwnd);
        with(hwnd, |w| restyle_selected(w, |s| {
            s.bold = true;
            s.family = Family::Serif;
        }));
        let after = item(hwnd, i);
        let st = TextStyle { family: Family::Serif, bold: true, ..t };
        let measured = with(hwnd, |w| text_draw::width(&w.gfx.dwrite, "Halo dunia semuanya", &st, 14.0 * TEXT_FILL)).unwrap();
        assert!((after.w - (measured + 2.0)).abs() < 0.01, "{} vs {}", after.w, measured + 2.0);
        assert_eq!(after.h, before.h);
        assert!(after.x >= 0.0 && after.x + after.w <= page_w + 0.01, "{} {}", after.x, after.w);
        assert!(matches!(&after.kind, ItemKind::Text(_, s) if *s == st));
        assert_eq!(undo_steps(hwnd), steps + 1);
        with(hwnd, |w| {
            w.doc.as_mut().expect("open").session.undo();
        });
        let undone = item(hwnd, i);
        assert_eq!((undone.x, undone.w, undone.h), (before.x, before.w, before.h));
        assert_eq!(undone.kind, before.kind);
        with(hwnd, |w| {
            w.doc.as_mut().expect("open").session.redo();
        });

        // Underline and colour never change the size.
        let sized = item(hwnd, i);
        with(hwnd, |w| restyle_selected(w, |s| s.underline = true));
        with(hwnd, |w| tint_selected(w, Tint::Red));
        let now = item(hwnd, i);
        assert_eq!((now.w, now.h, now.x), (sized.w, sized.h, sized.x));
        assert!(matches!(&now.kind, ItemKind::Text(_, s) if s.underline && s.tint == Tint::Red && s.bold));

        // The last-used style follows the changes: Text and Date share one;
        // a Check has its own colour.
        with(hwnd, |w| restyle_selected(w, |s| s.italic = true));
        assert!(style::last_text().italic && style::last_text().bold);
        let before_text = style::last_text();
        let c = add(ItemKind::Check(Tint::Black), 60.0, 14.0);
        with(hwnd, |w| tint_selected(w, Tint::Red));
        assert_eq!(item(hwnd, c).kind, ItemKind::Check(Tint::Red));
        assert_eq!(style::last_check(), Tint::Red);
        assert_eq!(style::last_text(), before_text, "a check's colour leaves the text style alone");
        // Other kinds ignore the text controls.
        with(hwnd, |w| restyle_selected(w, |s| s.bold = !s.bold));
        assert_eq!(item(hwnd, c).kind, ItemKind::Check(Tint::Red));

        // Ctrl+B on a selected text toggles bold; the face menu opens from
        // its button and a pick sets the face.
        with(hwnd, |w| w.selected = Some(i));
        let bold = |hwnd| matches!(item(hwnd, i).kind, ItemKind::Text(_, s) if s.bold);
        let was = bold(hwnd);
        toggle_by_key(hwnd, 0x42);
        assert_eq!(bold(hwnd), !was);
        act(hwnd, Hit::Family, false);
        assert_eq!(with(hwnd, |w| w.pop), Some(Some(Pop::Fonts)));
        assert!(with(hwnd, |w| popover(w).map(|p| p.items.len())).flatten() == Some(3));
        act(hwnd, Hit::PickFamily(Family::Mono), false);
        assert_eq!(with(hwnd, |w| w.pop), Some(None));
        assert!(matches!(item(hwnd, i).kind, ItemKind::Text(_, s) if s.family == Family::Mono));
        with(hwnd, paint::paint);
        // Esc closes the open menu before anything else.
        act(hwnd, Hit::Family, false);
        on_key(hwnd, VK_ESCAPE.0);
        assert_eq!(with(hwnd, |w| (w.pop, w.selected)), Some((None, Some(i))));
        with(hwnd, |w| w.selected = None);
        with(hwnd, |w| w.pop = None);
    }

    #[test]
    fn an_image_opens_as_one_page_and_saves_with_its_items() {
        let _com = Com::init();
        let dir = crate::phone::files::tests::temp_dir("sign-window-image");
        let path = dir.join("Surat.png");
        let white = Mark { w: 40, h: 20, rgba: vec![255; 40 * 20 * 4] };
        std::fs::write(&path, crate::sign::raster::encode_png_rgba(&white).unwrap()).unwrap();
        let src = Source { path: path.clone(), kind: Kind::Image };
        let cancel = AtomicBool::new(false);
        let Outcome::Ready(l) = open_document(&src, false, None, None, None, &cancel, 0) else {
            panic!("the image did not open");
        };
        assert!(l.image && l.opened.is_none());
        assert_eq!(l.geoms.len(), 1);
        assert_eq!((l.geoms[0].w, l.geoms[0].h), (30.0, 15.0));
        let mut session = Session::new(l.geoms.clone());
        session.add(Item { kind: ItemKind::Check(style::Tint::Blue), page: 0, x: 2.0, y: 2.0, w: 10.0, h: 10.0, turn: Quarter::R0 });
        session.rotate_page(0);
        drop(l);
        let job = SaveJob::Image { path: path.clone(), fmt: ImageFormat::Png };
        let png = save_bytes(job, &session.pages, &session.items).expect("saved");
        let out = crate::sign::raster::decode_png_rgba(&png).expect("a png");
        // Turned a quarter: 20 x 40, and the check is ink on the white.
        assert_eq!((out.w, out.h), (20, 40));
        assert!(out.rgba.chunks_exact(4).any(|p| u16::from(p[2]) > u16::from(p[0]) + 40));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_image_over_64_megapixels_says_so_at_open() {
        let _com = Com::init();
        let dir = crate::phone::files::tests::temp_dir("sign-window-image-big");
        let path = dir.join("Besar.png");
        std::fs::write(&path, crate::sign::render::tests::png_header_only(8001, 8000)).unwrap();
        let src = Source { path, kind: Kind::Image };
        let outcome = open_document(&src, false, None, None, None, &AtomicBool::new(false), 0);
        assert!(matches!(outcome, Outcome::Failed(Key::SignImageTooBig)));
        assert_eq!(phase_after(&outcome), Phase::Failed(Key::SignImageTooBig));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exactly_one_side_sweeps_the_office_copy() {
        // The worker ends first: the window, closing later, sweeps.
        let e = Mutex::new(JobEnd::default());
        e.lock().unwrap().started();
        assert!(!worker_ended(&e));
        assert!(window_closed(&e));
        // The window closes first (the worker had already passed its cancel
        // check and goes on to post): the worker sweeps when it ends.
        let e = Mutex::new(JobEnd::default());
        e.lock().unwrap().started();
        assert!(!window_closed(&e));
        assert!(worker_ended(&e));
        // No worker ever ran: the window sweeps.
        assert!(window_closed(&Mutex::new(JobEnd::default())));
    }

    #[test]
    fn the_focus_is_only_taken_through_the_on_screen_guard() {
        let call = concat!("SetFocus", "(Some(");
        for (name, src, allowed) in [("window.rs", include_str!("window.rs"), 1), ("maker.rs", include_str!("maker.rs"), 0)] {
            assert_eq!(src.matches(call).count(), allowed, "{name}: use take_focus, which checks ON_SCREEN");
        }
    }

    #[test]
    fn output_paths_are_worked_out_once_with_one_probe() {
        let probes = std::cell::Cell::new(0);
        let writable = |_: &Path| {
            probes.set(probes.get() + 1);
            true
        };
        let src = Path::new(r"C:\Kerja\Surat.JPG");
        let o = outs_for(src, false, true, Lang::Id, writable, |_| false);
        assert_eq!(probes.get(), 1, "one probe for both formats");
        assert_eq!(o.own, (PathBuf::from(r"C:\Kerja\Surat (ditandatangani).jpg"), false));
        assert_eq!(o.pdf, Some((PathBuf::from(r"C:\Kerja\Surat (ditandatangani).pdf"), false)));
        assert_eq!(o.pick(true), &o.pdf.clone().unwrap());
        assert_eq!(o.pick(false), &o.own);
        // A PDF has one output; a folder that cannot be written falls back.
        let o = outs_for(Path::new(r"C:\Kerja\Kontrak.pdf"), false, false, Lang::Id, |_| false, |_| false);
        assert!(o.pdf.is_none() && o.own.1, "{o:?}");
        assert_eq!(o.own.0.extension().and_then(|e| e.to_str()), Some("pdf"));
        assert_eq!(o.pick(true), &o.own);
    }

    #[test]
    fn a_closed_document_is_dropped_off_the_ui_thread() {
        struct Slow(Arc<AtomicBool>);
        impl Drop for Slow {
            fn drop(&mut self) {
                std::thread::sleep(std::time::Duration::from_millis(800));
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let done = Arc::new(AtomicBool::new(false));
        let t = std::time::Instant::now();
        drop_later(Slow(done.clone()));
        assert!(t.elapsed() < std::time::Duration::from_millis(300), "the caller did not wait");
        assert!(!done.load(Ordering::SeqCst));
        // Quitting waits for it.
        join_reapers();
        assert!(done.load(Ordering::SeqCst));
    }

    #[test]
    fn closing_never_waits_for_a_slow_opening() {
        let slow = std::thread::spawn(|| std::thread::sleep(std::time::Duration::from_millis(1500)));
        let t = std::time::Instant::now();
        assert!(reap(Some(slow)), "left running");
        assert!(t.elapsed() < std::time::Duration::from_millis(300));
        let quick = std::thread::spawn(|| {});
        while !quick.is_finished() {
            std::thread::yield_now();
        }
        assert!(!reap(Some(quick)), "a finished job is joined");
        assert!(!reap(None));
    }

    #[test]
    fn dates_read_in_each_language() {
        assert_eq!(date_text(Lang::Id, 2026, 11, 20), "20 November 2026");
        assert_eq!(date_text(Lang::Id, 2026, 8, 1), "1 Agustus 2026");
        assert_eq!(date_text(Lang::En, 2026, 8, 1), "1 August 2026");
        assert!(winansi::encode(&date_text(Lang::Id, 2026, 3, 9)).is_some());
    }

    #[test]
    fn the_copies_note_counts_in_good_english() {
        use crate::i18n::text;
        assert_eq!(text(Lang::En, Key::SignAllPagesDoneOne), "Initials put on 1 other page.");
        assert_eq!(
            text(Lang::En, Key::SignAllPagesDone).replace("{n}", "2"),
            "Initials put on 2 other pages.",
        );
        assert_eq!(text(Lang::Id, Key::SignAllPagesDoneOne), "Paraf dipasang di 1 halaman lain.");
    }

    #[test]
    fn closing_asks_only_with_changes_and_waits_for_a_save() {
        assert_eq!(close_action(Phase::Ready, false), CloseAction::Destroy);
        assert_eq!(close_action(Phase::Ready, true), CloseAction::Ask);
        assert_eq!(close_action(Phase::Saving, true), CloseAction::Wait);
        assert_eq!(close_action(Phase::Loading, false), CloseAction::Destroy);
        assert_eq!(close_action(Phase::Failed(Key::SignCannotOpen), false), CloseAction::Destroy);
        assert_eq!(close_action(Phase::Password { wrong: true, busy: false }, false), CloseAction::Destroy);
    }

    #[test]
    fn a_signed_pdf_warns_only_when_the_save_breaks_it() {
        assert!(!breaks_signature(false, false, true));
        assert!(!breaks_signature(true, true, false));
        assert!(breaks_signature(true, false, false));
        assert!(breaks_signature(true, true, true));
    }

    #[test]
    fn esc_goes_bar_popover_tool_selection_then_close() {
        // With the maker up, Esc only ever closes the maker.
        assert_eq!(esc_step(true, Ask::None, false, false, false), EscStep::Maker);
        assert_eq!(esc_step(true, Ask::Discard, true, true, true), EscStep::Maker);
        assert_eq!(esc_step(false, Ask::Discard, true, true, true), EscStep::Bar);
        assert_eq!(esc_step(false, Ask::None, true, true, true), EscStep::Popover);
        assert_eq!(esc_step(false, Ask::None, false, true, true), EscStep::Disarm);
        assert_eq!(esc_step(false, Ask::None, false, false, true), EscStep::Deselect);
        assert_eq!(esc_step(false, Ask::None, false, false, false), EscStep::Close);
    }

    #[test]
    fn opening_outcomes_map_to_phases_and_lines() {
        assert_eq!(open_error_key(&OpenError::NeedsPassword), None);
        assert_eq!(open_error_key(&OpenError::WrongPassword), None);
        assert_eq!(open_error_key(&OpenError::Broken), Some(Key::SignCannotOpen));
        assert_eq!(open_error_key(&OpenError::TooBig), Some(Key::SignTooBig));
        assert_eq!(office_error_key(&office::ConvertError::NoSuite), Key::SignNoOffice);
        assert_eq!(office_error_key(&office::ConvertError::Locked), Key::SignOfficeLocked);
        assert_eq!(office_error_key(&office::ConvertError::TimedOut), Key::SignOfficeFailed);
        assert_eq!(save_error_key(&SaveError::CannotKeepLock), Key::SignCannotKeepLock);
        assert_eq!(save_error_key(&SaveError::Write("x".into())), Key::SignSaveFailed);
        let pw = Outcome::Password { bytes: Arc::new(Vec::new()), wrong: true };
        assert_eq!(phase_after(&pw), Phase::Password { wrong: true, busy: false });
        assert_eq!(phase_after(&Outcome::Failed(Key::SignTooBig)), Phase::Failed(Key::SignTooBig));
    }

    #[test]
    fn marks_fit_their_box_and_renders_follow_the_turn() {
        let wide = Mark { w: 600, h: 100, rgba: Vec::new() };
        assert_eq!(fit_mark(&wide, 150.0, 50.0), (150.0, 25.0));
        let tall = Mark { w: 100, h: 100, rgba: Vec::new() };
        assert_eq!(fit_mark(&tall, 150.0, 50.0), (50.0, 50.0));
        assert_eq!(render_width(600.0, 800.0, Quarter::R0), 600);
        assert_eq!(render_width(800.0, 600.0, Quarter::R90), 600);
        assert_eq!(render_width(9000.0, 100.0, Quarter::R0), MAX_RENDER_W);
    }

    #[test]
    fn arrow_nudges_follow_the_page_as_shown() {
        // Upright: right is +x, down is -y in page space.
        assert_eq!(nudge(Quarter::R0, 1.0, 0.0), (1.0, 0.0));
        assert_eq!(nudge(Quarter::R0, 0.0, 1.0), (0.0, -1.0));
        // Each nudge, mapped through the view, moves the item that way on
        // screen.
        let g = PageGeom { w: 612.0, h: 792.0, base: Quarter::R0 };
        for shown in [Quarter::R0, Quarter::R90, Quarter::R180, Quarter::R270] {
            for (dx, dy) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
                let (px, py) = nudge(shown, dx, dy);
                let a = geom::page_to_view(g, shown, 1.0, (300.0, 300.0));
                let b = geom::page_to_view(g, shown, 1.0, (300.0 + px, 300.0 + py));
                assert!(
                    ((b.0 - a.0) - dx).abs() < 1e-4 && ((b.1 - a.1) - dy).abs() < 1e-4,
                    "{shown:?} {dx} {dy}",
                );
            }
        }
    }

    #[test]
    fn text_outside_winansi_becomes_a_picture_for_the_pdf() {
        let _com = Com::init();
        let item = |s: &str| Item {
            kind: ItemKind::Text(s.into(), Default::default()),
            page: 0,
            x: 10.0,
            y: 10.0,
            w: 80.0,
            h: 14.0,
            turn: Quarter::R90,
        };
        let out = encodable(&[item("Rp 1.500 \u{2013} Bu Siti"), item("\u{65E5}\u{672C}")]).unwrap();
        assert!(matches!(&out[0].kind, ItemKind::Text(s, _) if s.contains('\u{2013}')));
        match &out[1].kind {
            ItemKind::Signature(m) => {
                assert!(m.w > 0 && m.h > 0);
                // The picture keeps the box's aspect, so nothing stretches.
                assert!(((m.w as f32 / m.h as f32) - 80.0 / 14.0).abs() < 0.2);
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(
            (out[1].x, out[1].y, out[1].w, out[1].h, out[1].turn),
            (10.0, 10.0, 80.0, 14.0, Quarter::R90),
        );
    }
}
