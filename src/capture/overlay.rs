//! The capture overlay: one borderless topmost window over the whole virtual
//! screen, showing the frozen frame dimmed everywhere but the highlight.
//!
//! The pointer snaps to the window under it at once; finer boxes (panels,
//! toolbars) are added when the UI Automation worker answers. A click takes
//! the highlight, a drag draws a rectangle, Enter takes the highlight, a
//! right click or Esc cancels, and so does losing the foreground (Alt+Tab,
//! the Win key, another app). One overlay at a time. It exists only while
//! in use: no timer, nothing at idle, and the frame is freed when it closes.
//!
//! With "Setelah capture" set to "Langsung di layar" (`inline`), the pick
//! does not open the editor: the overlay stays up in its second phase
//! (`Phase::Edit`, in `edit`) and the region is drawn on in place. The pure
//! rules of that phase are in `inline`.

mod edit;

use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use windows::Win32::Foundation::{
    D2DERR_RECREATE_TARGET, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, ID2D1Bitmap};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos,
    GetForegroundWindow, GetWindowThreadProcessId, HWND_TOPMOST, IDC_CROSS, IsWindow, LoadCursorW,
    MSG, PM_REMOVE, PeekMessageW, PostQuitMessage, RegisterClassExW, SW_SHOWNA, SWP_SHOWWINDOW,
    SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow, WM_ACTIVATE, WM_ACTIVATEAPP,
    WM_CAPTURECHANGED, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY, WM_DISPLAYCHANGE,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSEXW, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, w};

use super::inline::FinishKind;
use super::uia::{WM_APP_UIA, take_result};
use super::win::{self, Frozen, TopWindow};
use super::{Candidate, Rect, crop, drag_rect, is_click, pick, size_label};
use crate::i18n::{Key, tr};
use crate::shot_edit::gfx::{Align, Gfx, Target, bitmap, color, rect_f};
use crate::shot_edit::text::WM_APP_TEXT;
use crate::theme;

const CLASS: windows::core::PCWSTR = w!("ValeSnapOverlay");

/// How much of the picture stays visible outside the highlight.
const DIM: f32 = 0.55;

/// Lost render targets made again in a row before the overlay gives up.
const RETRIES: u8 = 3;

/// Which part of the capture the overlay is in.
enum Phase {
    /// Choosing the region: snapping, click, drag.
    Pick,
    /// The region is chosen and stays on screen to draw on (inline mode).
    Edit(Box<edit::Edit>),
}

struct State {
    hwnd: HWND,
    frame: Frozen,
    gfx: Gfx,
    target: Option<Target>,
    picture: Option<ID2D1Bitmap>,
    cands: Vec<Candidate>,
    /// Which capture the UI Automation results belong to.
    id: usize,
    /// Raised when the overlay closes so the worker stops between windows.
    stop: Arc<AtomicBool>,
    /// Pointer in virtual-screen pixels.
    cursor: (i32, i32),
    hover: Option<Rect>,
    down: Option<(i32, i32)>,
    drag: Option<Rect>,
    accent: u32,
    demo: bool,
    /// Lost targets in a row (see `RETRIES`).
    lost: u8,
    /// Set once the overlay holds the foreground: from then on losing it
    /// cancels. Not before, so the brief input join in `take_foreground`
    /// (which can move the activation around) does not end it.
    armed: bool,
    /// "Setelah capture" = inline, latched when this capture started.
    inline: bool,
    /// "Salin otomatis", latched likewise (never in a demo): a Save also
    /// copies.
    auto_copy: bool,
    phase: Phase,
}

thread_local! {
    static OVERLAY: RefCell<Option<State>> = const { RefCell::new(None) };
    /// True from the first trigger until the window is up (the grab takes a
    /// moment): a second trigger is ignored.
    static STARTING: Cell<bool> = const { Cell::new(false) };
    /// The demo's Save runs on a worker after the overlay is gone;
    /// `run_snap` joins it so the file lands before the process ends.
    static DEMO_SAVE: RefCell<Option<std::thread::JoinHandle<bool>>> =
        const { RefCell::new(None) };
}

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

/// The highlight and the size label wear the island's accent, not the
/// window palette's (that one is a darker shade in light mode, for text
/// on a light window).
fn island_accent() -> u32 {
    theme::current().accent.rgb()
}

/// Clears `STARTING` on every way out of `start`, a panic included.
struct Starting;

impl Starting {
    fn begin() -> Self {
        STARTING.set(true);
        Starting
    }
}

impl Drop for Starting {
    fn drop(&mut self) {
        STARTING.set(false);
    }
}

/// Takes the value out of `slot` when `owns` says it is the caller's: the
/// one place the overlay's state leaves its slot, so a second close finds
/// nothing and a stale window handle never takes another overlay's state.
fn take_owned<T>(slot: &mut Option<T>, owns: impl FnOnce(&T) -> bool) -> Option<T> {
    if slot.as_ref().is_some_and(owns) {
        slot.take()
    } else {
        None
    }
}

/// The overlay's state, when it is `hwnd`'s.
fn take_state(hwnd: Option<HWND>) -> Option<State> {
    OVERLAY.with(|c| {
        let mut slot = c.try_borrow_mut().ok()?;
        take_owned(&mut slot, |s| hwnd.is_none_or(|h| s.hwnd == h))
    })
}

/// True while an overlay is up or being made.
pub fn active() -> bool {
    STARTING.get() || OVERLAY.with(|c| c.try_borrow().map_or(true, |s| s.is_some()))
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    OVERLAY.with(|c| c.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

/// Freezes the real screen and puts the overlay up. Ignored while one is
/// already up. The caller (the shortcut runner) is outside the app borrow.
pub fn start() {
    if active() {
        return;
    }
    let starting = Starting::begin();
    // Latched for this capture: a change in Settings meanwhile applies to
    // the next one.
    let inline = super::inline_after_capture();
    let up = (|| {
        let bounds = win::virtual_screen();
        let windows = win::top_windows(bounds);
        let frame = win::grab_virtual_screen()?;
        let cands: Vec<Candidate> = windows.iter().map(TopWindow::candidate).collect();
        create(frame, cands, Some(windows), false, inline)
    })();
    drop(starting);
    if up.is_none() {
        eprintln!("vale: the capture overlay could not start");
    }
}

/// The overlay over a made-up frame with fixed candidates: no screen grab,
/// no window list, no UI Automation (the `--demo-snap` mode). `inline` is
/// `--demo-snap inline`.
#[cfg(debug_assertions)]
pub fn start_demo(frame: Frozen, cands: Vec<Candidate>, inline: bool) -> bool {
    if active() {
        return false;
    }
    create(frame, cands, None, true, inline).is_some()
}

/// Waits for the demo's Save to land (`run_snap`, after the message loop).
#[cfg(debug_assertions)]
pub fn join_demo_save() {
    if let Some(handle) = DEMO_SAVE.with(|s| s.borrow_mut().take()) {
        let _ = handle.join();
    }
}

fn register(instance: windows::Win32::Foundation::HINSTANCE) -> windows::core::Result<()> {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            // Double clicks (a text edited in place) arrive as their own
            // message, handled like a press.
            style: CS_DBLCLKS,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_CROSS)?,
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

fn create(
    frame: Frozen,
    cands: Vec<Candidate>,
    for_uia: Option<Vec<TopWindow>>,
    demo: bool,
    inline: bool,
) -> Option<()> {
    let area = frame.rect();
    let gfx = Gfx::new().ok()?;
    let instance = unsafe { GetModuleHandleW(None) }.ok()?.into();
    register(instance).ok()?;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS,
            w!("Vale capture"),
            // The EDIT of a text drawn inline is a child of this window.
            WS_POPUP | WS_CLIPCHILDREN,
            area.x,
            area.y,
            area.w,
            area.h,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .ok()?;
    let mut at = POINT::default();
    let _ = unsafe { GetCursorPos(&mut at) };
    let cursor = if demo {
        (area.x + area.w / 2, area.y + area.h / 2)
    } else {
        (at.x, at.y)
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let hover = pick(&cands, cursor);
    let state = State {
        hwnd,
        frame,
        gfx,
        target: None,
        picture: None,
        cands,
        id,
        stop: stop.clone(),
        cursor,
        hover,
        down: None,
        drag: None,
        accent: island_accent(),
        demo,
        lost: 0,
        armed: false,
        inline,
        // A demo never touches the clipboard.
        auto_copy: crate::shot_edit::auto_copy_setting() && !demo,
        phase: Phase::Pick,
    };
    let stored = OVERLAY.with(|c| match c.try_borrow_mut() {
        Ok(mut slot) if slot.is_none() => {
            *slot = Some(state);
            true
        }
        _ => false,
    });
    if !stored {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return None;
    }
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            area.x,
            area.y,
            area.w,
            area.h,
            SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
    if !take_foreground(hwnd) {
        // Esc and Enter would reach the app underneath (Enter could send a
        // message there): no overlay rather than that.
        eprintln!("vale: the capture overlay could not take the keyboard");
        finish(None);
        return None;
    }
    // Nothing is pumped between the check above and this, so the overlay
    // still holds the foreground.
    with(|s| s.armed = true);
    invalidate(hwnd);
    if let Some(windows) = for_uia {
        // Never waited for: the windows are snappable already.
        super::uia::spawn(hwnd, id, windows, stop);
    }
    Some(())
}

/// Brings the overlay to the foreground with the keyboard focus. Windows
/// may refuse a plain `SetForegroundWindow` (the foreground lock, after
/// other input); then this thread's input is joined to the foreground
/// window's for a moment, which it allows. True when the overlay has it.
fn take_foreground(hwnd: HWND) -> bool {
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() != hwnd {
            let ours = GetCurrentThreadId();
            let theirs = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let joined =
                theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, true).as_bool();
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            if joined {
                let _ = AttachThreadInput(ours, theirs, false);
            }
        }
        let _ = SetFocus(Some(hwnd));
        GetForegroundWindow() == hwnd
    }
}

/// Closes the overlay. `choice` is the rectangle to keep (virtual-screen
/// pixels); it is cropped from the frame, the frame is freed, the window
/// closes and the editor opens at once. `None` cancels: nothing is kept,
/// copied or opened.
///
/// Idempotent: the state leaves its slot first, so a second call (the
/// `WM_ACTIVATE` that `DestroyWindow` itself sends, an Alt+F4 after a click)
/// finds nothing and does nothing.
fn finish(choice: Option<Rect>) {
    let Some(state) = take_state(None) else {
        return;
    };
    state.stop.store(true, Ordering::Relaxed);
    let shot = choice.and_then(|r| {
        crop(state.frame.bgra(), state.frame.stride(), state.frame.rect(), r).map(|s| (s, r))
    });
    let (hwnd, demo) = (state.hwnd, state.demo);
    // The picture, the target and the frame go now (an open text with them).
    drop(state);
    unsafe {
        // Not while the window is already on its way out (closed by the
        // system from outside).
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
    }
    match shot {
        Some((shot, r)) => crate::shot_edit::window::open(shot, r.center(), demo),
        None if demo => unsafe { PostQuitMessage(0) },
        None => {}
    }
}

/// Ends the inline capture as `kind` says (see `inline::finish_copies`): the
/// selection is cropped from the frame, the annotations moved onto the crop,
/// and then Copy and an auto-copying Save put it on the clipboard (on this
/// thread, before the window goes), Save also writes it on a worker, and
/// OpenEditor opens the editor on it. Anything else closes with nothing
/// copied. Idempotent like `finish`.
fn finish_inline(kind: FinishKind) {
    // A text still being typed is part of the picture.
    edit::commit_text_now();
    let Some(mut state) = take_state(None) else {
        return;
    };
    state.stop.store(true, Ordering::Relaxed);
    let (hwnd, demo, auto_copy) = (state.hwnd, state.demo, state.auto_copy);
    let kept = match std::mem::replace(&mut state.phase, Phase::Pick) {
        Phase::Edit(e) => {
            let sel = e.sel;
            let doc = edit::doc_in_crop(&e.session.doc, sel, state.frame.origin);
            crop(
                state.frame.bgra(),
                state.frame.stride(),
                state.frame.rect(),
                sel,
            )
            .map(|shot| (shot, sel, doc))
        }
        Phase::Pick => None,
    };
    // The frame (100+ MB on a big desktop), the target, the picture and the
    // open EDIT go before anything is rendered.
    drop(state);
    let (kind, kept) = match kept {
        Some(k) => (kind, Some(k)),
        // Nothing to keep (an empty crop): the same as a cancel.
        None => (FinishKind::Cancel, None),
    };
    let mut editor = None;
    let mut saved = None;
    if let Some((shot, sel, doc)) = kept {
        if super::inline::finish_copies(kind, auto_copy) && !demo {
            let capture = crate::shot_edit::next_capture_id();
            // Managed: one history card, no notch preview, no copy notice.
            if crate::shot_edit::export::copy(hwnd, &shot, &doc, capture, true).is_none() {
                eprintln!("vale: the inline screenshot could not be copied");
            }
        }
        match kind {
            FinishKind::Save => {
                let dir = demo.then(|| {
                    std::env::current_dir()
                        .unwrap_or_default()
                        .join("target")
                        .join("demo-shots")
                });
                saved = crate::shot_edit::export::save_detached(Arc::new(shot), doc, dir);
                if saved.is_none() {
                    eprintln!("vale: the inline screenshot could not be saved");
                }
            }
            // The overlay closes first below, then the editor opens on the
            // crop (and follows its own rules, auto-copy included).
            FinishKind::OpenEditor => editor = Some((shot, sel, doc)),
            _ => {}
        }
    }
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
    }
    if let Some(handle) = saved {
        if demo {
            DEMO_SAVE.with(|s| *s.borrow_mut() = Some(handle));
        }
        // Otherwise detached: nobody waits.
    }
    if let Some((shot, sel, doc)) = editor {
        crate::shot_edit::window::open_with(shot, sel.center(), demo, doc);
    } else if demo {
        // Nothing else will end the demo's message loop.
        unsafe { PostQuitMessage(0) };
    }
}

/// What a left click takes: the highlight, else the whole monitor under the
/// pointer.
fn click_target(s: &State) -> Rect {
    s.hover
        .unwrap_or_else(|| win::monitor_at(s.cursor).0.intersect(&s.frame.rect()))
}

/// Recomputes the highlight for the pointer; true when what is drawn changed.
fn moved(s: &mut State, cursor: (i32, i32)) -> bool {
    let (old_hover, old_drag, old_cursor) = (s.hover, s.drag, s.cursor);
    s.cursor = cursor;
    match s.down {
        Some(down) if !is_click(down, cursor) => {
            s.drag = Some(drag_rect(down, cursor, s.frame.rect()));
        }
        _ => {
            s.drag = None;
            s.hover = pick(&s.cands, cursor);
        }
    }
    // The hint follows the pointer to another monitor.
    let monitor_changed = win::monitor_at(old_cursor).0 != win::monitor_at(cursor).0;
    s.hover != old_hover || s.drag != old_drag || monitor_changed
}

/// The overlay lost the foreground: keys now go to another window (Enter
/// could send a message there) while the frozen screen still shows, so it
/// cancels, copying nothing. Ignored until `create` has armed it.
fn deactivated() {
    if with(|s| s.armed) == Some(true) {
        finish(None);
    }
}

/// `WM_ACTIVATE` says the window was deactivated: `WA_INACTIVE` (0) in the
/// low word of `wparam`.
fn is_deactivation(wparam: usize) -> bool {
    wparam & 0xFFFF == 0
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// True while the overlay is in its drawing phase.
fn in_edit() -> bool {
    with(|s| matches!(s.phase, Phase::Edit(_))) == Some(true)
}

/// The pointer of a mouse message in virtual-screen pixels.
fn pointer(s: &State, lparam: LPARAM) -> (i32, i32) {
    let origin = s.frame.origin;
    (
        origin.0 + i32::from(lparam.0 as i16),
        origin.1 + i32::from((lparam.0 >> 16) as i16),
    )
}

/// The pick is made: inline mode keeps the overlay and goes on drawing on
/// `r`; otherwise the editor opens on it.
fn picked(hwnd: HWND, r: Rect) {
    if with(|s| s.inline) == Some(true) {
        edit::enter(hwnd, r);
    } else {
        finish(Some(r));
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if in_edit() {
                edit::set_cursor();
            } else {
                unsafe {
                    if let Ok(cross) = LoadCursorW(None, IDC_CROSS) {
                        SetCursor(Some(cross));
                    }
                }
            }
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            if in_edit() {
                edit::on_move(hwnd, wparam, lparam);
                return LRESULT(0);
            }
            const MK_LBUTTON: usize = 0x0001;
            let held = wparam.0 & MK_LBUTTON != 0;
            let changed = with(|s| {
                if !held {
                    // The release went elsewhere (capture lost): no drag.
                    s.down = None;
                }
                let p = pointer(s, lparam);
                moved(s, p)
            });
            if changed == Some(true) {
                invalidate(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            if in_edit() {
                edit::on_down(hwnd, lparam, msg == WM_LBUTTONDBLCLK);
                return LRESULT(0);
            }
            let set = with(|s| {
                let p = pointer(s, lparam);
                s.cursor = p;
                s.down = Some(p);
            });
            if set.is_some() {
                unsafe {
                    let _ = SetCapture(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if in_edit() {
                edit::on_up(hwnd, lparam);
                return LRESULT(0);
            }
            let choice = with(|s| {
                let up = pointer(s, lparam);
                let down = s.down.take()?;
                s.cursor = up;
                if is_click(down, up) {
                    // Pointing at where it was pressed: the highlight.
                    s.hover = pick(&s.cands, up);
                    Some(click_target(s))
                } else {
                    let r = drag_rect(down, up, s.frame.rect());
                    if r.is_empty() {
                        // A straight line takes nothing: back to snapping.
                        s.drag = None;
                        s.hover = pick(&s.cands, up);
                        return None;
                    }
                    Some(r)
                }
            })
            .flatten();
            // Released after the state above is settled, so WM_CAPTURECHANGED
            // finds no press left to cancel.
            unsafe {
                let _ = ReleaseCapture();
            }
            match choice {
                Some(r) => picked(hwnd, r),
                None => invalidate(hwnd),
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            // Capture lost mid-drag (Alt+Tab, Win, another window): the
            // release will not come here, so drop the press and its drag.
            if lparam.0 != hwnd.0 as isize {
                if in_edit() {
                    edit::on_capture_lost(hwnd);
                    return LRESULT(0);
                }
                let changed = with(|s| {
                    let pressed = s.down.take().is_some() || s.drag.is_some();
                    s.drag = None;
                    s.hover = pick(&s.cands, s.cursor);
                    pressed
                });
                if changed == Some(true) {
                    invalidate(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            if in_edit() {
                // The same as Esc.
                edit::esc(hwnd, FinishKind::Cancel);
            } else {
                finish(None);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if in_edit() {
                edit::on_key(hwnd, (wparam.0 & 0xFFFF) as u16);
                return LRESULT(0);
            }
            match wparam.0 as u16 {
                x if x == VK_ESCAPE.0 => finish(None),
                x if x == VK_RETURN.0 => {
                    if let Some(Some(r)) = with(|s| s.hover) {
                        picked(hwnd, r);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_APP_TEXT => {
            edit::on_text_msg(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_COMMAND => {
            // EN_CHANGE of the text EDIT: grow it with its text.
            if crate::shot_edit::text::is_change(wparam) {
                edit::fit_edit(hwnd);
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => match edit::ctl_color(wparam) {
            Some(brush) => brush,
            None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        },
        WM_APP_UIA => {
            // Always taken, so its box is freed even for an older capture.
            let found = take_result(lparam);
            let changed = with(|s| {
                let Some(found) = found else { return false };
                // A late answer after the pick is of no use.
                if wparam.0 != s.id || matches!(s.phase, Phase::Edit(_)) {
                    return false;
                }
                s.cands.extend(found);
                let was = s.hover;
                if s.down.is_none() {
                    s.hover = pick(&s.cands, s.cursor);
                }
                s.hover != was
            });
            if changed == Some(true) {
                invalidate(hwnd);
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            // The frozen frame no longer fits the screen.
            finish(None);
            LRESULT(0)
        }
        WM_ACTIVATE if is_deactivation(wparam.0) => {
            deactivated();
            LRESULT(0)
        }
        WM_ACTIVATEAPP if wparam.0 == 0 => {
            // Another app took the foreground.
            deactivated();
            LRESULT(0)
        }
        WM_CLOSE => {
            // Alt+F4 and the like. In the drawing phase the same way out as
            // Esc (the confirm bar when something is drawn); otherwise a
            // cancel. No DefWindowProc, whose DestroyWindow would come on
            // top of `finish`'s own.
            if in_edit() {
                edit::close(hwnd);
            } else {
                finish(None);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // The window went without `finish` (closed from outside): free
            // the frame (it is over 100 MB on a big desktop) and let
            // `active()` say false. Only this window's own state.
            if let Some(state) = take_state(Some(hwnd)) {
                state.stop.store(true, Ordering::Relaxed);
                let demo = state.demo;
                drop(state);
                if demo {
                    unsafe { PostQuitMessage(0) };
                }
            }
            // Results that were posted but not taken: free their boxes.
            let mut m = MSG::default();
            while unsafe { PeekMessageW(&mut m, Some(hwnd), WM_APP_UIA, WM_APP_UIA, PM_REMOVE) }
                .as_bool()
            {
                drop(take_result(m.lParam));
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    unsafe {
        BeginPaint(hwnd, &mut ps);
    }
    let failed = with(|s| match draw(s) {
        Ok(()) => {
            s.lost = 0;
            false
        }
        Err(err) if err.code() == D2DERR_RECREATE_TARGET && s.lost < RETRIES => {
            // A lost target: made again on the next paint (the mosaics,
            // bitmaps of the old target, with it).
            s.lost += 1;
            s.target = None;
            s.picture = None;
            if let Phase::Edit(e) = &mut s.phase {
                e.pix.clear();
            }
            invalidate(hwnd);
            false
        }
        Err(err) => {
            // Anything else (a frame too big for the device) would fail on
            // every paint: give up rather than repaint in a loop. Nothing
            // is copied.
            eprintln!("vale: the capture overlay could not draw: {err}");
            true
        }
    });
    unsafe {
        let _ = EndPaint(hwnd, &ps);
    }
    if failed == Some(true) {
        finish(None);
    }
}

fn black(a: f32) -> D2D1_COLOR_F {
    color(0, a)
}

fn draw(s: &mut State) -> windows::core::Result<()> {
    if matches!(s.phase, Phase::Edit(_)) {
        return edit::draw(s);
    }
    let (w, h) = (s.frame.w(), s.frame.h());
    if s.target.is_none() {
        s.target = Some(s.gfx.hwnd_target(s.hwnd, (w, h))?);
        s.picture = None;
    }
    let target = s.target.as_ref().expect("made above");
    let rt: &windows::Win32::Graphics::Direct2D::ID2D1RenderTarget = &target.rt;
    if s.picture.is_none() {
        s.picture = Some(bitmap(rt, w, h, s.frame.bgra())?);
    }
    let picture = s.picture.as_ref().expect("made above");
    let brush = &target.brush;
    let origin = s.frame.origin;
    let local = |r: Rect| Rect::new(r.x - origin.0, r.y - origin.1, r.w, r.h);
    let (full_w, full_h) = (w as f32, h as f32);
    let lit = s.drag.or(s.hover).map(local);
    unsafe {
        rt.BeginDraw();
        rt.DrawBitmap(
            picture,
            Some(&rect_f(0.0, 0.0, full_w, full_h)),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            None,
        );
        brush.SetColor(&black(DIM));
        let dim = |x: f32, y: f32, w: f32, h: f32| {
            if w > 0.0 && h > 0.0 {
                rt.FillRectangle(&rect_f(x, y, w, h), brush);
            }
        };
        match lit {
            None => dim(0.0, 0.0, full_w, full_h),
            Some(r) => {
                let (x, y, rw, rh) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
                dim(0.0, 0.0, full_w, y);
                dim(0.0, y + rh, full_w, full_h - y - rh);
                dim(0.0, y, x, rh);
                dim(x + rw, y, full_w - x - rw, rh);
            }
        }
        let (_, dpi) = win::monitor_at(s.cursor);
        let ui = dpi as f32 / 96.0;
        if let Some(r) = lit {
            let (x, y, rw, rh) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
            brush.SetColor(&color(s.accent, 1.0));
            // Just outside the highlight, so the picture inside is untouched.
            rt.DrawRectangle(
                &rect_f(x - 1.0 * ui, y - 1.0 * ui, rw + 2.0 * ui, rh + 2.0 * ui),
                brush,
                2.0 * ui,
                None,
            );
            let label = size_label(s.drag.or(s.hover).unwrap_or_default());
            let lw = s.gfx.measure(&label, 12.0 * ui, 500).0 + 16.0 * ui;
            let lh = 22.0 * ui;
            let below = y + rh + 8.0 * ui;
            let ly = if below + lh <= full_h {
                below
            } else {
                (y + 8.0 * ui).max(0.0)
            };
            let lx = x.min(full_w - lw).max(0.0);
            pill(
                rt,
                brush,
                &s.gfx,
                (lx, ly, lw, lh),
                &label,
                12.0 * ui,
                s.accent,
                ui,
            );
        }
        if s.drag.is_none() {
            let (mon, _) = win::monitor_at(s.cursor);
            let mon = local(mon);
            let hint = tr(Key::SnapHint);
            let size = 13.0 * ui;
            let hw = s.gfx.measure(hint, size, 500).0 + 36.0 * ui;
            let hh = 32.0 * ui;
            let hx = mon.x as f32 + (mon.w as f32 - hw) / 2.0;
            let hy = mon.y as f32 + 24.0 * ui;
            pill(rt, brush, &s.gfx, (hx, hy, hw, hh), hint, size, 0, ui);
        }
        rt.EndDraw(None, None)
    }
}

/// A dark rounded pill with white text (the size label, the hint). `edge`
/// colours its hairline (0 for a plain white one).
#[allow(clippy::too_many_arguments)]
fn pill(
    rt: &windows::Win32::Graphics::Direct2D::ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    gfx: &Gfx,
    (x, y, w, h): (f32, f32, f32, f32),
    text: &str,
    size: f32,
    edge: u32,
    ui: f32,
) {
    use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
    use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;
    let rounded = D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
        radiusX: h / 2.0,
        radiusY: h / 2.0,
    };
    unsafe {
        brush.SetColor(&black(0.82));
        rt.FillRoundedRectangle(&rounded, brush);
        if edge == 0 {
            brush.SetColor(&color(0xFFFFFF, 0.16));
        } else {
            brush.SetColor(&color(edge, 0.9));
        }
        rt.DrawRoundedRectangle(&rounded, brush, 1.0 * ui, None);
    }
    gfx.text_in(
        rt,
        brush,
        text,
        (x, y, w, h),
        size,
        500,
        0xFFFFFF,
        1.0,
        Align::Center,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_overlay_is_up_in_a_test() {
        assert!(!active());
    }

    #[test]
    fn only_wa_inactive_is_a_deactivation() {
        // WA_INACTIVE, with the minimised flag in the high word or not.
        assert!(is_deactivation(0));
        assert!(is_deactivation(0x0001_0000));
        // WA_ACTIVE and WA_CLICKACTIVE.
        assert!(!is_deactivation(1));
        assert!(!is_deactivation(2));
        assert!(!is_deactivation(0x0001_0001));
    }

    #[test]
    fn the_state_leaves_its_slot_once() {
        let mut slot = Some(7);
        assert_eq!(take_owned(&mut slot, |_| true), Some(7));
        // A second close finds nothing: finish is idempotent.
        assert_eq!(take_owned(&mut slot, |_| true), None);
        assert!(slot.is_none());
    }

    #[test]
    fn a_stale_window_never_takes_another_overlays_state() {
        let mut slot = Some(7);
        assert_eq!(take_owned(&mut slot, |s| *s == 8), None);
        assert_eq!(slot, Some(7), "still there for its own window");
        assert_eq!(take_owned(&mut slot, |s| *s == 7), Some(7));
        let mut empty: Option<i32> = None;
        assert_eq!(take_owned(&mut empty, |_| true), None);
    }

    #[test]
    fn starting_clears_on_every_way_out() {
        assert!(!active());
        {
            let _starting = Starting::begin();
            assert!(active(), "a second trigger is ignored meanwhile");
        }
        assert!(!active());
        // Also when the start unwinds.
        let r = std::panic::catch_unwind(|| {
            let _starting = Starting::begin();
            panic!("boom");
        });
        assert!(r.is_err());
        assert!(!active());
    }

    #[test]
    fn the_highlight_is_the_island_accent() {
        assert_eq!(island_accent(), theme::current().accent.rgb());
    }

    #[test]
    fn the_dim_leaves_less_than_half_the_picture() {
        assert!((0.5..0.6).contains(&DIM));
    }

    #[test]
    fn the_text_message_is_the_shared_one() {
        assert_eq!(WM_APP_TEXT, windows::Win32::UI::WindowsAndMessaging::WM_APP + 50);
    }
}
