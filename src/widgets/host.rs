//! The windows of the desktop widgets: one layered top-level window per
//! sticky note, drawn into a DIB with a software Direct2D target and pushed
//! with `UpdateLayeredWindow` (`arrange_overlay::Canvas`). Pixels with alpha 0
//! are click-through, so the transparent margin (a soft shadow, and the room
//! for the peeking cat) costs nothing.
//!
//! The host is a component of whoever owns the message loop (the island, or
//! the demo): `sync` brings the windows to the notes, and everything the user
//! does comes back as `WidgetEvent`s, drained after the owner gets
//! `WM_APP_WIDGET`. Window procedures never touch the island's state
//! (`with_app`): they read and write this module's own thread-local state, and
//! every Win32 call that can send messages into our own procedures (focus,
//! destroying, menus) is made with that state released.
//!
//! Z-order: a note on the desktop layer is `HWND_BOTTOM` and owned by the
//! shell's window (Progman), the way Rainmeter keeps its skins visible on
//! Win+D; `WM_WINDOWPOSCHANGING` forces `HWND_BOTTOM` again whenever something
//! asks for another place, so a click that activates the note never raises it.
//! A pinned note (and every note on the "Di atas jendela" layer) is topmost.
//!
//! Nothing runs while idle: no timer, no frame thread. A redraw happens on
//! input or data; the hover fade and the gallery highlight are eased on the
//! owner's frame clock (`moving` / `tick`), and the caret blinks on a timer of
//! the note's own window only while that note has the keyboard.

use std::cell::RefCell;
use std::time::Instant;

use chrono::{DateTime, Local};
use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, GWLP_HWNDPARENT, GWLP_USERDATA, GetCaretBlinkTime, GetCursorPos, GetShellWindow,
    GetWindowLongPtrW, HWND_BOTTOM, HWND_TOPMOST, IDC_ARROW, IDC_HAND, IDC_IBEAM, IDC_SIZENWSE,
    KillTimer, LoadCursorW, MA_ACTIVATE, MA_NOACTIVATE, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, PostMessageW, RegisterClassExW, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SetCursor, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WINDOWPOS, WM_ACTIVATE, WM_APP,
    WM_CAPTURECHANGED, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WM_WINDOWPOSCHANGING, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_POPUP, WA_INACTIVE, CS_DBLCLKS,
};
use windows::core::{PCWSTR, w};

use crate::arrange_overlay::{Canvas, take_foreground};
use crate::capture::Rect;
use crate::config::UiFont;
use crate::i18n::{self, Key, Lang, text};
use crate::notch_monitor::Display;
use crate::notes::Note;
use crate::theme::{self, CatColor};
use crate::widgets::checklist::{self, Edit};
use crate::widgets::edit::{self, EditEvent};
use crate::widgets::paint::{
    self, BodyLayout, Btn, GuideLine, HEADER_H, Res, Scene, Target, margins, shown_h, window_dip,
};
use crate::widgets::place;
use crate::widgets::pop::{self, PopCmd, PopState};
use crate::widgets::prefs::{FullscreenRule, WidgetLayer, WidgetPrefs};
use crate::widgets::snap::{Axis, snap_move, snap_resize};
use crate::widgets::sticky::{self, DipRect, Paper, Sticky, TextSize};

/// Posted to the owner when `WidgetEvent`s wait. One message until `drain`.
pub const WM_APP_WIDGET: u32 = WM_APP + 71;
/// The caret's blink, a timer of the note's own window, alive only while it
/// has the keyboard.
pub const TIMER_WIDGET_CARET: usize = 46;

const CLASS: PCWSTR = w!("ValeWidget");
/// The hover fade of the header.
const FADE_MS: f32 = 120.0;
/// The gallery's highlight frame, in and out.
const HIGHLIGHT_MS: f32 = 600.0;

/// What the user did to a note.
#[derive(Clone, Debug, PartialEq)]
pub enum WidgetEvent {
    /// The text is now this (model text, `\n` breaks).
    Text { id: String, text: String },
    /// The note was moved or resized: its monitor and rect (dip).
    Placed {
        id: String,
        monitor: String,
        rect: DipRect,
    },
    Paper { id: String, paper: Paper },
    Size { id: String, size: TextSize },
    Pinned { id: String, on: bool },
    Rolled { id: String, on: bool },
    /// "Kucing ngintip" was switched from a note's popover.
    CatPeek(bool),
    /// The (+) of this note's header.
    NewBeside { id: String },
    /// "Lepas dari desktop" (the x): the note stays unless it is empty.
    Unstick { id: String },
    /// "Hapus catatan".
    Delete { id: String },
    /// The note was used: the cat goes to the one touched last.
    Touched { id: String, at: i64 },
    /// Something eases: the owner starts its frame clock.
    NeedsClock,
}

/// What `sync` is given besides the notes.
pub struct Ctx<'a> {
    pub notes: &'a [Note],
    pub prefs: WidgetPrefs,
    pub displays: &'a [Display],
    pub lang: Lang,
    pub font: UiFont,
    pub coat: CatColor,
    /// Fades and the highlight move (Motion Full, Windows animations on);
    /// otherwise they are instant.
    pub animate: bool,
}

// ------------------------------------------------------------------ state

#[derive(Clone, Copy)]
struct Ease {
    from: f32,
    to: f32,
    start: Instant,
    ms: f32,
}

impl Ease {
    fn at(v: f32) -> Ease {
        Ease {
            from: v,
            to: v,
            start: Instant::now(),
            ms: 0.0,
        }
    }

    fn value(&self, now: Instant) -> f32 {
        if self.ms <= 0.0 {
            return self.to;
        }
        let t = (now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 / self.ms)
            .clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        self.from + (self.to - self.from) * eased
    }

    fn running(&self, now: Instant) -> bool {
        self.ms > 0.0 && now.saturating_duration_since(self.start).as_secs_f32() * 1000.0 < self.ms
    }

    fn toward(&mut self, to: f32, now: Instant, animate: bool) {
        if (self.to - to).abs() < f32::EPSILON {
            return;
        }
        self.from = self.value(now);
        self.to = to;
        self.start = now;
        self.ms = if animate { FADE_MS } else { 0.0 };
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DragKind {
    Move,
    Resize,
}

struct Drag {
    kind: DragKind,
    cursor: (i32, i32),
    /// The note's rect (px) when the drag began, and its dip rect.
    note: Rect,
    dip: DipRect,
    moved: bool,
    /// Windows whose guide lines the drag lit.
    lit: Vec<usize>,
}

struct Win {
    id: String,
    hwnd: HWND,
    sticky: Sticky,
    text: String,
    updated: i64,
    /// The note's rect on the desktop (px), the monitor's scale and its place
    /// in the display list.
    note: Rect,
    scale: f32,
    display: usize,
    canvas: Option<Canvas>,
    body: Option<BodyLayout>,
    body_key: Option<(String, TextSize, i32)>,
    scroll: f32,
    header: Ease,
    hot: Option<Btn>,
    tracking: bool,
    pressed: Option<Btn>,
    edit: Option<HWND>,
    sel: (usize, usize),
    /// Where the selection started (a drag or a shift move extends from it).
    anchor: usize,
    caret_on: bool,
    /// Scroll to show the caret at the next frame.
    follow_caret: bool,
    selecting: bool,
    drag: Option<Drag>,
    guides: Vec<GuideLine>,
    highlight: Option<Instant>,
    footer_msg: Option<Key>,
    confirm_delete: bool,
    /// Hidden by a fullscreen app on its monitor.
    away: bool,
    shown: bool,
    dirty: bool,
}

struct State {
    owner: HWND,
    res: Option<Res>,
    font: UiFont,
    lang: Lang,
    coat: CatColor,
    animate: bool,
    prefs: WidgetPrefs,
    displays: Vec<Display>,
    wins: Vec<Win>,
    events: Vec<WidgetEvent>,
    posted: bool,
    /// The note the cat sits on.
    peek: Option<String>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

fn idx_of(s: &State, hwnd: HWND) -> Option<usize> {
    s.wins.iter().position(|w| w.hwnd == hwnd)
}

fn unix_now() -> i64 {
    Local::now().timestamp()
}

fn push(s: &mut State, ev: WidgetEvent) {
    s.events.push(ev);
    if !s.posted {
        s.posted = true;
        unsafe {
            let _ = PostMessageW(Some(s.owner), WM_APP_WIDGET, WPARAM(0), LPARAM(0));
        }
    }
}

/// The note was used: now it is the one touched last.
fn touch(s: &mut State, i: usize) {
    let at = unix_now();
    let id = s.wins[i].id.clone();
    s.wins[i].sticky.touched = at;
    if !matches!(s.events.last(), Some(WidgetEvent::Touched { id: last, .. }) if *last == id) {
        push(s, WidgetEvent::Touched { id: id.clone(), at });
    } else if let Some(WidgetEvent::Touched { at: slot, .. }) = s.events.last_mut() {
        *slot = at;
    }
    refresh_peek(s);
}

/// The note with the largest `touched` gets the cat (the first one on a tie);
/// the notes it leaves and joins are redrawn.
fn refresh_peek(s: &mut State) {
    let owner = s
        .wins
        .iter()
        .filter(|w| w.sticky.touched > 0)
        .max_by_key(|w| w.sticky.touched)
        .map(|w| w.id.clone());
    if owner == s.peek {
        return;
    }
    let before = s.peek.take();
    s.peek = owner.clone();
    for w in &mut s.wins {
        if Some(&w.id) == before.as_ref() || Some(&w.id) == owner.as_ref() {
            w.dirty = true;
        }
    }
}

// ------------------------------------------------------------------ the facade

/// The desktop widgets of one message loop. The state is thread-local: one
/// host per thread, so the window procedures can find it.
pub struct WidgetHost {
    _private: (),
}

impl WidgetHost {
    /// A host whose events are announced to `owner` with `WM_APP_WIDGET`.
    pub fn new(owner: HWND) -> WidgetHost {
        STATE.with(|cell| {
            *cell.borrow_mut() = Some(State {
                owner,
                res: None,
                font: UiFont::Sans,
                lang: Lang::Id,
                coat: CatColor::Terracotta,
                animate: true,
                prefs: WidgetPrefs::default(),
                displays: Vec::new(),
                wins: Vec::new(),
                events: Vec::new(),
                posted: false,
                peek: None,
            });
        });
        WidgetHost { _private: () }
    }

    /// Brings the windows to `ctx`: one per note that has a sticky part,
    /// created, moved, styled and hidden or shown as the settings say.
    pub fn sync(&mut self, ctx: &Ctx) {
        sync(ctx);
    }

    /// The theme, language or font changed (or Windows said so): every note
    /// is drawn again.
    pub fn restyle(&mut self) {
        let _ = with(|s| {
            for w in &mut s.wins {
                w.body_key = None;
                w.dirty = true;
            }
        });
        flush_dirty();
    }

    /// The events the user made since the last call.
    pub fn drain(&mut self) -> Vec<WidgetEvent> {
        with(|s| {
            s.posted = false;
            std::mem::take(&mut s.events)
        })
        .unwrap_or_default()
    }

    /// Gives note `id` the keyboard (a fresh note).
    pub fn focus(&mut self, id: &str) {
        let hwnd = with(|s| s.wins.iter().find(|w| w.id == id).map(|w| w.hwnd)).flatten();
        if let Some(hwnd) = hwnd {
            take_foreground(hwnd);
            begin_editing(hwnd, None);
        }
    }

    /// The gallery card was clicked: every note flashes an accent frame.
    pub fn highlight(&mut self) {
        let animate = with(|s| s.animate).unwrap_or(false);
        let _ = with(|s| {
            let now = Instant::now();
            for w in &mut s.wins {
                w.highlight = Some(now);
                w.dirty = true;
            }
            if animate {
                push(s, WidgetEvent::NeedsClock);
            }
        });
        if !animate {
            // Without motion a single frame of the frame is no use: skip it.
            let _ = with(|s| {
                for w in &mut s.wins {
                    w.highlight = None;
                }
            });
        }
        flush_dirty();
    }

    /// Whether anything eases now (the owner keeps its frame clock running).
    pub fn moving(&self, now: Instant) -> bool {
        with(|s| {
            s.wins.iter().any(|w| {
                w.header.running(now)
                    || w
                        .highlight
                        .is_some_and(|t| elapsed_ms(t, now) < HIGHLIGHT_MS)
            })
        })
        .unwrap_or(false)
    }

    /// One frame of the easing: the notes that move are drawn.
    pub fn tick(&mut self, now: Instant) {
        let _ = with(|s| {
            for w in &mut s.wins {
                let fading = w.header.running(now);
                let lit = match w.highlight {
                    Some(t) if elapsed_ms(t, now) >= HIGHLIGHT_MS => {
                        w.highlight = None;
                        true
                    }
                    Some(_) => true,
                    None => false,
                };
                if fading || lit {
                    w.dirty = true;
                }
            }
        });
        flush_dirty();
    }

    /// Every window is shown again over the others it should be under
    /// (Explorer restarted: the shell window the notes are owned by is new).
    pub fn reapply_z(&mut self) {
        let hwnds: Vec<usize> = with(|s| (0..s.wins.len()).collect()).unwrap_or_default();
        for i in hwnds {
            apply_z(i);
        }
    }

    /// The monitors changed (hot-plug, resolution, DPI): every note is placed
    /// again from its saved monitor and rect.
    pub fn displays_changed(&mut self, displays: &[Display]) {
        let _ = with(|s| {
            s.displays = displays.to_vec();
            for i in 0..s.wins.len() {
                relayout(s, i);
            }
        });
        flush_dirty();
    }

    /// The 1 s system tick: with "Saat layar penuh: Sembunyikan", the notes
    /// that sit above windows (pinned, or the whole "Di atas jendela" layer)
    /// hide while a fullscreen app covers their monitor. `is_fullscreen` is
    /// asked once per such monitor, with one of its notes. A desktop-layer
    /// note that is not pinned is never touched; with nothing eligible this
    /// does nothing at all.
    pub fn fullscreen_tick(&mut self, mut is_fullscreen: impl FnMut(HWND) -> bool) {
        let probes: Vec<(usize, HWND)> = with(|s| {
            if !s.prefs.show || s.prefs.fullscreen != FullscreenRule::Hide {
                return Vec::new();
            }
            let mut seen: Vec<(usize, HWND)> = Vec::new();
            for w in &s.wins {
                if above_windows(s, w) && !seen.iter().any(|(d, _)| *d == w.display) {
                    seen.push((w.display, w.hwnd));
                }
            }
            seen
        })
        .unwrap_or_default();
        let answers: Vec<(usize, bool)> = probes
            .into_iter()
            .map(|(d, hwnd)| (d, is_fullscreen(hwnd)))
            .collect();
        let changes: Vec<(HWND, bool)> = with(|s| {
            let mut out = Vec::new();
            for i in 0..s.wins.len() {
                let eligible = above_windows(s, &s.wins[i]);
                let away = eligible
                    && answers
                        .iter()
                        .any(|(d, fs)| *d == s.wins[i].display && *fs);
                if s.wins[i].away != away {
                    s.wins[i].away = away;
                    out.push((s.wins[i].hwnd, away));
                }
            }
            out
        })
        .unwrap_or_default();
        for (hwnd, away) in changes {
            if away {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            } else if with(|s| s.prefs.show).unwrap_or(false) {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_SHOWNA);
                }
            }
        }
    }

    /// How many windows there are.
    pub fn count(&self) -> usize {
        with(|s| s.wins.len()).unwrap_or(0)
    }

    /// A message the owner got and the host cares about (the shell restarted).
    pub fn on_taskbar_created(&mut self) {
        self.reapply_z();
    }

    /// Destroys every window and lets go of the Direct2D objects. Before the
    /// process ends (the software targets must not outlive the loop).
    pub fn shutdown(&mut self) {
        pop::close();
        let hwnds: Vec<HWND> = with(|s| s.wins.iter().map(|w| w.hwnd).collect()).unwrap_or_default();
        for h in hwnds {
            unsafe {
                let _ = DestroyWindow(h);
            }
        }
        STATE.with(|cell| {
            if let Ok(mut guard) = cell.try_borrow_mut() {
                *guard = None;
            }
        });
    }
}

fn elapsed_ms(t: Instant, now: Instant) -> f32 {
    now.saturating_duration_since(t).as_secs_f32() * 1000.0
}

/// A note that sits above ordinary windows.
fn above_windows(s: &State, w: &Win) -> bool {
    w.sticky.pinned || s.prefs.layer == WidgetLayer::Top
}

// ------------------------------------------------------------------ sync

fn sync(ctx: &Ctx) {
    // 1. What the notes are, against the windows there are.
    let wanted: Vec<&Note> = ctx
        .notes
        .iter()
        .filter(|n| n.sticky.is_some())
        .take(sticky::MAX_STICKY)
        .collect();
    let gone: Vec<HWND> = with(|s| {
        s.wins
            .iter()
            .filter(|w| !wanted.iter().any(|n| n.id == w.id))
            .map(|w| w.hwnd)
            .collect()
    })
    .unwrap_or_default();
    for hwnd in gone {
        end_editing(hwnd);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    let missing: Vec<&Note> = with(|s| {
        wanted
            .iter()
            .copied()
            .filter(|n| !s.wins.iter().any(|w| w.id == n.id))
            .collect()
    })
    .unwrap_or_default();
    let mut created: Vec<(String, HWND)> = Vec::new();
    for n in missing {
        if let Some(hwnd) = create_window() {
            created.push((n.id.clone(), hwnd));
        }
    }
    let _ = with(|s| {
        s.displays = ctx.displays.to_vec();
        s.lang = ctx.lang;
        s.coat = ctx.coat;
        s.animate = ctx.animate;
        let prefs_changed = s.prefs != ctx.prefs;
        s.prefs = ctx.prefs;
        if s.font != ctx.font {
            s.font = ctx.font;
            if let Some(res) = &mut s.res {
                res.set_font(ctx.font);
            }
            for w in &mut s.wins {
                w.body_key = None;
                w.dirty = true;
            }
        }
        for (id, hwnd) in created {
            let Some(note) = wanted.iter().find(|n| n.id == id) else {
                continue;
            };
            let sticky = note.sticky.clone().unwrap_or_default();
            s.wins.push(Win {
                id,
                hwnd,
                sticky,
                text: note.text.clone(),
                updated: note.updated,
                note: Rect::default(),
                scale: 1.0,
                display: 0,
                canvas: None,
                body: None,
                body_key: None,
                scroll: 0.0,
                header: Ease::at(0.0),
                hot: None,
                tracking: false,
                pressed: None,
                edit: None,
                sel: (0, 0),
                anchor: 0,
                caret_on: true,
                follow_caret: false,
                selecting: false,
                drag: None,
                guides: Vec::new(),
                highlight: None,
                footer_msg: None,
                confirm_delete: false,
                away: false,
                shown: false,
                dirty: true,
            });
        }
        for note in &wanted {
            let Some(i) = s.wins.iter().position(|w| w.id == note.id) else {
                continue;
            };
            let sticky = note.sticky.clone().unwrap_or_default();
            let w = &mut s.wins[i];
            let moving = w.drag.is_some();
            if w.sticky != sticky && !moving {
                w.sticky = sticky;
                w.dirty = true;
            } else if moving {
                // The drag owns the place; the rest still follows.
                let rect = w.sticky.rect;
                let monitor = w.sticky.monitor.clone();
                if w.sticky != sticky {
                    w.sticky = Sticky {
                        rect,
                        monitor,
                        ..sticky
                    };
                    w.dirty = true;
                }
            }
            // While the note has the keyboard the EDIT is the truth.
            if w.edit.is_none() && w.text != note.text {
                w.text = note.text.clone();
                w.dirty = true;
            }
            if w.updated != note.updated {
                w.updated = note.updated;
                w.dirty = true;
            }
            if prefs_changed {
                w.dirty = true;
            }
            if !moving {
                relayout(s, i);
            }
        }
        refresh_peek(s);
    });
    // 2. Layers, visibility and the first frame.
    let n = with(|s| s.wins.len()).unwrap_or(0);
    for i in 0..n {
        apply_z(i);
        apply_visibility(i);
    }
    flush_dirty();
}

/// The note's rect on the desktop from its saved place and the displays now;
/// the window follows (position and size are the canvas's).
fn relayout(s: &mut State, i: usize) {
    let w = &mut s.wins[i];
    let mut shown = w.sticky.clone();
    shown.rect.h = shown_h(shown.rect.h, shown.rolled);
    let Some(p) = place::place(&shown, &s.displays) else {
        return;
    };
    if w.note != p.rect || w.scale != p.scale || w.display != p.display {
        w.note = p.rect;
        w.scale = p.scale;
        w.display = p.display;
        w.canvas = None;
        w.body_key = None;
        w.dirty = true;
    }
}

fn apply_visibility(i: usize) {
    let todo = with(|s| {
        let show = s.prefs.show && !s.wins[i].away;
        let w = &mut s.wins[i];
        if w.shown == show {
            return None;
        }
        w.shown = show;
        Some((w.hwnd, show))
    })
    .flatten();
    if let Some((hwnd, show)) = todo {
        unsafe {
            let _ = ShowWindow(hwnd, if show { SW_SHOWNA } else { SW_HIDE });
        }
    }
}

/// The layer: bottom (owned by the shell's window) or topmost. The mode goes
/// in the window's user data for `WM_WINDOWPOSCHANGING`, which cannot borrow
/// the state.
fn apply_z(i: usize) {
    let todo = with(|s| {
        let w = &s.wins[i];
        (w.hwnd, above_windows(s, w))
    });
    let Some((hwnd, top)) = todo else { return };
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, if top { 2 } else { 1 });
        let owner = if top { 0 } else { GetShellWindow().0 as isize };
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner);
        let _ = SetWindowPos(
            hwnd,
            Some(if top { HWND_TOPMOST } else { HWND_BOTTOM }),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

fn register_class() -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_DBLCLKS,
        lpfnWndProc: Some(wnd_proc),
        hInstance: module.into(),
        hCursor: cursor,
        lpszClassName: CLASS,
        ..Default::default()
    };
    unsafe {
        if RegisterClassExW(&wc) == 0 {
            return windows::core::Error::from_thread().code() == ERROR_CLASS_ALREADY_EXISTS.to_hresult();
        }
    }
    true
}

fn create_window() -> Option<HWND> {
    if !register_class() {
        return None;
    }
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            CLASS,
            w!("Vale sticky note"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .ok()
}

// ------------------------------------------------------------------ drawing

/// Draws every note that changed.
fn flush_dirty() {
    let _ = with(|s| {
        for i in 0..s.wins.len() {
            if s.wins[i].dirty {
                draw(s, i);
            }
        }
    });
}

fn draw(s: &mut State, i: usize) {
    s.wins[i].dirty = false;
    if s.wins[i].note.is_empty() {
        return;
    }
    if s.res.is_none() {
        s.res = Res::new(s.font).ok();
    }
    let State {
        res,
        wins,
        prefs,
        lang,
        coat,
        peek,
        ..
    } = s;
    let Some(res) = res.as_ref() else { return };
    let can_add_more = sticky::can_add(wins.len());
    let w = &mut wins[i];
    let k = w.scale;
    let (mw, mh) = window_dip(w.sticky.rect.w, shown_h(w.sticky.rect.h, w.sticky.rolled));
    let size = ((mw * k).round() as i32, (mh * k).round() as i32);
    if w.canvas.as_ref().is_none_or(|c| c.size() != size) {
        w.canvas = Canvas::with_gfx(res.gfx.clone(), size).ok();
    }
    let m = margins();
    let origin = (
        w.note.x - (m.0 * k).round() as i32,
        w.note.y - (m.1 * k).round() as i32,
    );
    // The text, laid out for the width it is drawn at.
    let (nw, nh) = (w.sticky.rect.w, w.sticky.rect.h);
    let area = paint::body_rect(nw, nh);
    let key = (w.text.clone(), w.sticky.text_size, (area.w * 100.0) as i32);
    if w.body_key.as_ref() != Some(&key) {
        w.body = Some(paint::build_body(res, &w.text, w.sticky.text_size, area.w));
        w.body_key = Some(key);
    }
    let body_h = w.body.as_ref().map_or(0.0, |b| b.height);
    let max_scroll = (body_h - area.h).max(0.0);
    if w.follow_caret {
        w.follow_caret = false;
        if let Some((_, cy, ch)) = w.body.as_ref().and_then(|b| b.caret(w.sel.1)) {
            if cy < w.scroll {
                w.scroll = cy;
            } else if cy + ch > w.scroll + area.h {
                w.scroll = cy + ch - area.h;
            }
        }
    }
    w.scroll = w.scroll.clamp(0.0, max_scroll);
    let now = Instant::now();
    let island = theme::island(theme::current().accent, theme::current().surface);
    let tones = sticky::tones(w.sticky.paper, &island, theme::current().accent);
    let today = Local::now().date_naive();
    let updated = DateTime::from_timestamp(w.updated, 0)
        .map(|t| t.with_timezone(&Local).naive_local())
        .unwrap_or_default();
    let footer = match w.footer_msg {
        Some(key) => text(*lang, key).to_owned(),
        None if w.confirm_delete => text(*lang, Key::StickyDeleteAgain).to_owned(),
        None => sticky::footer_label(*lang, &w.text, updated, today),
    };
    let peek_on = prefs.cat_peek && peek.as_deref() == Some(w.id.as_str());
    let highlight = w
        .highlight
        .map(|t| {
            let p = (elapsed_ms(t, now) / HIGHLIGHT_MS).clamp(0.0, 1.0);
            (p * std::f32::consts::PI).sin()
        })
        .unwrap_or(0.0);
    let title = checklist::title_of(&w.text);
    let scene = Scene {
        w: nw,
        h: nh,
        scale: k,
        tones,
        rolled: w.sticky.rolled,
        pinned: w.sticky.pinned,
        header: w.header.value(now),
        hot: w.hot,
        plus_enabled: can_add_more,
        date: i18n::short_date(*lang, updated.date()),
        footer,
        rolled_note: sticky::rolled_label(*lang, &w.text),
        title: if title.is_empty() {
            text(*lang, Key::StickyEmpty).to_owned()
        } else {
            title
        },
        body: w.body.as_ref(),
        scroll: w.scroll,
        selection: w.edit.map(|_| w.sel),
        caret_on: w.caret_on && w.edit.is_some(),
        guides: &w.guides,
        highlight,
        peek: peek_on.then(|| paint::peek_face(&w.text)),
        cat: paint::peek_look(*coat),
        frame_accent: tones.accent,
    };
    if let Some(canvas) = &w.canvas {
        let _ = canvas.paint_with(w.hwnd, origin, 0.0, 255, |c| paint::draw(c, res, &scene));
    }
}

// ------------------------------------------------------------------ messages

/// In the Controls feature of `windows`; the value is fixed by Win32.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// Private to the note windows: leave editing (posted from inside the EDIT's
/// own handlers, which must not destroy it themselves).
const WM_END_EDIT: u32 = WM_APP + 1;

fn lo(l: LPARAM) -> i32 {
    (l.0 & 0xFFFF) as i16 as i32
}

fn hi(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xFFFF) as i16 as i32
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // Whatever asks for another place in the z-order, a note on the
        // desktop layer stays at the bottom.
        WM_WINDOWPOSCHANGING => {
            let mode = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            let wp = lparam.0 as *mut WINDOWPOS;
            if mode == 1 && !wp.is_null() {
                let wp = unsafe { &mut *wp };
                if wp.flags.0 & SWP_NOZORDER.0 == 0 {
                    wp.hwndInsertAfter = HWND_BOTTOM;
                }
            }
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEACTIVATE => return mouse_activate(hwnd),
        WM_SETCURSOR => {
            if set_cursor(hwnd) {
                return LRESULT(1);
            }
        }
        WM_MOUSEMOVE => {
            mouse_move(hwnd, lo(lparam), hi(lparam));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            mouse_leave(hwnd);
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            left_down(hwnd, lo(lparam), hi(lparam));
            return LRESULT(0);
        }
        WM_LBUTTONDBLCLK => {
            double_click(hwnd, lo(lparam), hi(lparam));
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            left_up(hwnd, lo(lparam), hi(lparam));
            return LRESULT(0);
        }
        WM_CAPTURECHANGED => {
            let _ = with(|s| {
                if let Some(i) = idx_of(s, hwnd) {
                    s.wins[i].selecting = false;
                    s.wins[i].pressed = None;
                    if s.wins[i].drag.is_some() {
                        end_drag(s, i);
                    }
                }
            });
            return LRESULT(0);
        }
        WM_RBUTTONUP => {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            }
            context_menu(hwnd, pt);
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            let _ = with(|s| {
                if let Some(i) = idx_of(s, hwnd) {
                    let line = s.wins[i].sticky.text_size.line();
                    s.wins[i].scroll -= delta * 3.0 * line;
                    s.wins[i].dirty = true;
                    draw(s, i);
                }
            });
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == TIMER_WIDGET_CARET => {
            let _ = with(|s| {
                if let Some(i) = idx_of(s, hwnd) {
                    s.wins[i].caret_on = !s.wins[i].caret_on;
                    s.wins[i].dirty = true;
                    draw(s, i);
                }
            });
            return LRESULT(0);
        }
        WM_END_EDIT => {
            end_editing(hwnd);
            return LRESULT(0);
        }
        WM_ACTIVATE => {
            // Losing the keyboard to anything but this note's own popover
            // ends the editing.
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                let to = HWND(lparam.0 as *mut _);
                if !pop::is_popover_of(to, hwnd) {
                    end_editing(hwnd);
                }
            }
            return LRESULT(0);
        }
        WM_DPICHANGED => {
            // The notes size themselves in dip from the monitor's DPI: place
            // them again, ignoring the rectangle Windows suggests.
            let displays = crate::arrange_win::displays();
            let _ = with(|s| {
                s.displays = displays;
                for i in 0..s.wins.len() {
                    relayout(s, i);
                }
            });
            flush_dirty();
            return LRESULT(0);
        }
        WM_DESTROY => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_WIDGET_CARET);
            }
            let _ = with(|s| {
                s.wins.retain(|w| w.hwnd != hwnd);
                refresh_peek(s);
            });
            flush_dirty();
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A click on the text takes the keyboard; every other click (the header,
/// a box, the grip) leaves the user's own app active.
fn mouse_activate(hwnd: HWND) -> LRESULT {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let text = with(|s| {
        let i = idx_of(s, hwnd)?;
        Some(matches!(
            target_at(&s.wins[i], pt.x, pt.y),
            Target::Body | Target::Footer
        ))
    })
    .flatten()
    .unwrap_or(false);
    LRESULT((if text { MA_ACTIVATE } else { MA_NOACTIVATE }) as isize)
}

/// What is under a point of the desktop (px) on this note.
fn target_at(w: &Win, sx: i32, sy: i32) -> Target {
    let x = (sx - w.note.x) as f32 / w.scale;
    let y = (sy - w.note.y) as f32 / w.scale;
    paint::target(
        x,
        y,
        w.sticky.rect.w,
        w.sticky.rect.h,
        w.sticky.rolled,
        w.body.as_ref(),
        w.scroll,
    )
}

/// A client point (px) as dip from the note's top-left.
fn client_to_dip(w: &Win, x: i32, y: i32) -> (f32, f32) {
    let m = margins();
    (x as f32 / w.scale - m.0, y as f32 / w.scale - m.1)
}

fn set_cursor(hwnd: HWND) -> bool {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let id = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        if w.drag.is_some() {
            return Some(IDC_SIZENWSE);
        }
        match target_at(w, pt.x, pt.y) {
            Target::Body => Some(IDC_IBEAM),
            Target::Button(_) | Target::Check(_) => Some(IDC_HAND),
            Target::Grip if !s.prefs.lock => Some(IDC_SIZENWSE),
            _ => Some(IDC_ARROW),
        }
    })
    .flatten();
    let Some(id) = id else { return false };
    unsafe {
        if let Ok(c) = LoadCursorW(None, id) {
            SetCursor(Some(c));
            return true;
        }
    }
    false
}

fn mouse_move(hwnd: HWND, x: i32, y: i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let alt = unsafe { GetKeyState(i32::from(VK_MENU.0)) } < 0;
    let follow = with(|s| {
        let i = idx_of(s, hwnd)?;
        let now = Instant::now();
        let animate = s.animate;
        let lang_cap = !sticky::can_add(s.wins.len());
        if !s.wins[i].tracking {
            let mut track = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            if unsafe { TrackMouseEvent(&mut track) }.is_ok() {
                s.wins[i].tracking = true;
            }
        }
        if s.wins[i].drag.is_some() {
            drag_move(s, i, (pt.x, pt.y), alt);
            return None;
        }
        let hot_target = target_at(&s.wins[i], pt.x, pt.y);
        let w = &mut s.wins[i];
        let hot = match hot_target {
            Target::Button(b) => Some(b),
            _ => None,
        };
        let inside = !matches!(hot_target, Target::Outside);
        let mut changed = false;
        if w.hot != hot {
            w.hot = hot;
            changed = true;
        }
        let msg = (hot == Some(Btn::Plus) && lang_cap).then_some(Key::StickyMax);
        if w.footer_msg != msg {
            w.footer_msg = msg;
            changed = true;
        }
        let was = w.header.to;
        w.header.toward(if inside { 1.0 } else { 0.0 }, now, animate);
        if (w.header.to - was).abs() > f32::EPSILON {
            changed = true;
            if animate {
                push(s, WidgetEvent::NeedsClock);
            }
        }
        let w = &mut s.wins[i];
        if w.selecting && w.edit.is_some() {
            let (dx, dy) = client_to_dip(w, x, y);
            if let Some(body) = &w.body {
                let idx = body.index_at(dx - paint::PAD_X, dy - HEADER_H + w.scroll);
                return Some((w.edit?, w.text.clone(), w.anchor, idx));
            }
        }
        if changed {
            w.dirty = true;
            draw(s, i);
        }
        None
    })
    .flatten();
    if let Some((edit_hwnd, t, anchor, idx)) = follow {
        edit::set_selection(edit_hwnd, &t, anchor, idx);
        sync_from_edit(hwnd, edit_hwnd);
    }
}

fn mouse_leave(hwnd: HWND) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        let animate = s.animate;
        let w = &mut s.wins[i];
        w.tracking = false;
        w.hot = None;
        w.footer_msg = None;
        if w.drag.is_none() {
            w.header.toward(0.0, Instant::now(), animate);
            if animate {
                push(s, WidgetEvent::NeedsClock);
            }
        }
        s.wins[i].dirty = true;
        draw(s, i);
        Some(())
    });
}

enum Down {
    Nothing,
    Capture,
    Edit(usize),
}

fn left_down(hwnd: HWND, x: i32, y: i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let act = with(|s| {
        let i = idx_of(s, hwnd)?;
        let lock = s.prefs.lock;
        let t = target_at(&s.wins[i], pt.x, pt.y);
        s.wins[i].confirm_delete = false;
        match t {
            Target::Button(b) => {
                s.wins[i].pressed = Some(b);
                Some(Down::Capture)
            }
            Target::Check(line) => {
                toggle_line(s, i, line);
                Some(Down::Nothing)
            }
            Target::Header | Target::Grip if lock => Some(Down::Nothing),
            Target::Header | Target::Grip => {
                let kind = if t == Target::Grip {
                    DragKind::Resize
                } else {
                    DragKind::Move
                };
                let w = &mut s.wins[i];
                w.drag = Some(Drag {
                    kind,
                    cursor: (pt.x, pt.y),
                    note: w.note,
                    dip: DipRect::new(
                        w.sticky.rect.x,
                        w.sticky.rect.y,
                        w.sticky.rect.w,
                        shown_h(w.sticky.rect.h, w.sticky.rolled),
                    ),
                    moved: false,
                    lit: Vec::new(),
                });
                touch(s, i);
                Some(Down::Capture)
            }
            Target::Body | Target::Footer => {
                let w = &s.wins[i];
                let (dx, dy) = client_to_dip(w, x, y);
                let at = w.body.as_ref().map_or(0, |b| {
                    b.index_at(dx - paint::PAD_X, dy - HEADER_H + w.scroll)
                });
                touch(s, i);
                Some(Down::Edit(at))
            }
            Target::Outside => Some(Down::Nothing),
        }
    })
    .flatten();
    match act {
        Some(Down::Capture) => unsafe {
            let _ = SetCapture(hwnd);
        },
        Some(Down::Edit(at)) => {
            begin_editing(hwnd, Some(at));
            let _ = with(|s| {
                if let Some(i) = idx_of(s, hwnd) {
                    s.wins[i].selecting = true;
                    s.wins[i].anchor = at;
                }
            });
            unsafe {
                let _ = SetCapture(hwnd);
            }
        }
        _ => {}
    }
    flush_dirty();
}

fn left_up(hwnd: HWND, _x: i32, _y: i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let press = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &mut s.wins[i];
        w.selecting = false;
        let pressed = w.pressed.take();
        if w.drag.is_some() {
            end_drag(s, i);
            return None;
        }
        let over = match target_at(&s.wins[i], pt.x, pt.y) {
            Target::Button(b) => Some(b),
            _ => None,
        };
        pressed.filter(|p| Some(*p) == over)
    })
    .flatten();
    unsafe {
        let _ = ReleaseCapture();
    }
    if let Some(b) = press {
        button(hwnd, b);
    }
}

fn double_click(hwnd: HWND, _x: i32, _y: i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let on_header = with(|s| {
        let i = idx_of(s, hwnd)?;
        Some(matches!(target_at(&s.wins[i], pt.x, pt.y), Target::Header))
    })
    .flatten()
    .unwrap_or(false);
    if on_header {
        unsafe {
            let _ = ReleaseCapture();
        }
        toggle_rolled(hwnd);
    } else {
        left_down(hwnd, _x, _y);
    }
}

// ------------------------------------------------------------------ actions

fn toggle_line(s: &mut State, i: usize, line: usize) {
    let Some(e) = checklist::toggle(&s.wins[i].text, line) else {
        return;
    };
    apply_edit(s, i, &e);
    touch(s, i);
}

/// `e` applied to note `i`'s text: through the EDIT while it has the keyboard
/// (so it can be undone), else straight to the text.
fn apply_edit(s: &mut State, i: usize, e: &Edit) {
    let w = &mut s.wins[i];
    let before = w.text.clone();
    match w.edit {
        Some(edit_hwnd) => {
            edit::apply(edit_hwnd, &before, e);
            let t = edit::text(edit_hwnd);
            w.sel = edit::selection(edit_hwnd, &t);
            w.text = t;
        }
        None => {
            w.text = checklist::apply(&before, e);
        }
    }
    text_changed(s, i, &before);
}

/// The text of note `i` changed from `before`: tell the owner, redraw.
fn text_changed(s: &mut State, i: usize, before: &str) {
    let w = &mut s.wins[i];
    if w.text == before {
        w.dirty = true;
        draw(s, i);
        return;
    }
    w.updated = unix_now();
    w.caret_on = true;
    w.follow_caret = true;
    w.dirty = true;
    let (id, t) = (w.id.clone(), w.text.clone());
    push(s, WidgetEvent::Text { id, text: t });
    draw(s, i);
}

fn button(hwnd: HWND, b: Btn) {
    match b {
        Btn::Pin => {
            let pinned = with(|s| {
                let i = idx_of(s, hwnd)?;
                let on = !s.wins[i].sticky.pinned;
                s.wins[i].sticky.pinned = on;
                s.wins[i].dirty = true;
                let id = s.wins[i].id.clone();
                push(s, WidgetEvent::Pinned { id, on });
                touch(s, i);
                Some(i)
            })
            .flatten();
            if let Some(i) = pinned {
                apply_z(i);
                flush_dirty();
            }
        }
        Btn::Plus => {
            let _ = with(|s| {
                let i = idx_of(s, hwnd)?;
                if sticky::can_add(s.wins.len()) {
                    let id = s.wins[i].id.clone();
                    push(s, WidgetEvent::NewBeside { id });
                }
                Some(())
            });
        }
        Btn::Check => {
            let _ = with(|s| {
                let i = idx_of(s, hwnd)?;
                let t = s.wins[i].text.clone();
                let caret = if s.wins[i].edit.is_some() {
                    s.wins[i].sel.1
                } else {
                    t.encode_utf16().count()
                };
                let e = checklist::toggle_current_line(&t, caret);
                apply_edit(s, i, &e);
                touch(s, i);
                Some(())
            });
        }
        Btn::Colour => open_popover(hwnd),
        Btn::More => {
            let at = with(|s| {
                let i = idx_of(s, hwnd)?;
                let w = &s.wins[i];
                let r = paint::button_rect(Btn::More, w.sticky.rect.w);
                Some(POINT {
                    x: w.note.x + (r.x * w.scale).round() as i32,
                    y: w.note.y + ((r.y + r.h) * w.scale).round() as i32,
                })
            })
            .flatten();
            if let Some(at) = at {
                context_menu(hwnd, at);
            }
        }
        Btn::Close => {
            let _ = with(|s| {
                let i = idx_of(s, hwnd)?;
                let id = s.wins[i].id.clone();
                push(s, WidgetEvent::Unstick { id });
                Some(())
            });
        }
    }
}

fn toggle_rolled(hwnd: HWND) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        let on = !s.wins[i].sticky.rolled;
        s.wins[i].sticky.rolled = on;
        s.wins[i].scroll = 0.0;
        relayout(s, i);
        s.wins[i].dirty = true;
        let id = s.wins[i].id.clone();
        push(s, WidgetEvent::Rolled { id, on });
        touch(s, i);
        Some(())
    });
    flush_dirty();
}

fn set_paper(hwnd: HWND, paper: Paper) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        s.wins[i].sticky.paper = paper;
        s.wins[i].dirty = true;
        let id = s.wins[i].id.clone();
        push(s, WidgetEvent::Paper { id, paper });
        touch(s, i);
        Some(())
    });
    flush_dirty();
}

fn set_size(hwnd: HWND, size: TextSize) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        s.wins[i].sticky.text_size = size;
        s.wins[i].body_key = None;
        s.wins[i].dirty = true;
        let id = s.wins[i].id.clone();
        push(s, WidgetEvent::Size { id, size });
        touch(s, i);
        Some(())
    });
    flush_dirty();
}

fn set_pinned(hwnd: HWND, on: bool) {
    let i = with(|s| {
        let i = idx_of(s, hwnd)?;
        if s.wins[i].sticky.pinned == on {
            return None;
        }
        s.wins[i].sticky.pinned = on;
        s.wins[i].dirty = true;
        let id = s.wins[i].id.clone();
        push(s, WidgetEvent::Pinned { id, on });
        touch(s, i);
        Some(i)
    })
    .flatten();
    if let Some(i) = i {
        apply_z(i);
        flush_dirty();
    }
}

/// A command of the colour popover, for the note it belongs to.
pub(crate) fn pop_command(note: HWND, cmd: PopCmd) {
    match cmd {
        PopCmd::Paper(p) => set_paper(note, p),
        PopCmd::Size(sz) => set_size(note, sz),
        PopCmd::Pin(on) => set_pinned(note, on),
        PopCmd::Peek(on) => {
            let _ = with(|s| {
                s.prefs.cat_peek = on;
                for w in &mut s.wins {
                    w.dirty = true;
                }
                push(s, WidgetEvent::CatPeek(on));
            });
            flush_dirty();
        }
    }
    if let Some(state) = pop_state(note) {
        pop::update(state);
    }
}

fn pop_state(note: HWND) -> Option<PopState> {
    with(|s| {
        let i = idx_of(s, note)?;
        let w = &s.wins[i];
        Some(PopState {
            paper: w.sticky.paper,
            size: w.sticky.text_size,
            pinned: w.sticky.pinned,
            peek: s.prefs.cat_peek,
            lang: s.lang,
            font: s.font,
        })
    })
    .flatten()
}

fn open_popover(hwnd: HWND) {
    let info = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        let r = paint::button_rect(Btn::Colour, w.sticky.rect.w);
        let anchor = Rect::new(
            w.note.x + (r.x * w.scale).round() as i32,
            w.note.y + (r.y * w.scale).round() as i32,
            (r.w * w.scale).round() as i32,
            (r.h * w.scale).round() as i32,
        );
        let area = s
            .displays
            .get(w.display)
            .map_or(w.note, place::area_px);
        Some((anchor, area, w.scale))
    })
    .flatten();
    let state = pop_state(hwnd);
    if let (Some((anchor, area, scale)), Some(state)) = (info, state) {
        pop::open(hwnd, anchor, area, scale, state);
    }
}

/// The popover closed: a note that was being edited gets its keyboard back.
pub(crate) fn pop_closed(note: HWND) {
    let edit_hwnd = with(|s| {
        let i = idx_of(s, note)?;
        s.wins[i].edit
    })
    .flatten();
    if let Some(e) = edit_hwnd {
        take_foreground(note);
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(e));
        }
    }
}

// ------------------------------------------------------------------ the menu

const CMD_PAPER: usize = 1;
const CMD_PIN: usize = 20;
const CMD_ROLL: usize = 21;
const CMD_UNSTICK: usize = 22;
const CMD_DELETE: usize = 23;

fn context_menu(hwnd: HWND, at: POINT) {
    let data = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        Some((
            w.sticky.paper,
            w.sticky.pinned,
            w.sticky.rolled,
            w.confirm_delete,
            s.lang,
        ))
    })
    .flatten();
    let Some((paper, pinned, rolled, confirm, lang)) = data else {
        return;
    };
    let cmd = unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let Ok(colours) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            return;
        };
        for (n, p) in Paper::ALL.iter().enumerate() {
            let _ = AppendMenuW(
                colours,
                MF_STRING,
                CMD_PAPER + n,
                &windows::core::HSTRING::from(text(lang, p.label_key())),
            );
        }
        let at_paper = Paper::ALL.iter().position(|p| *p == paper).unwrap_or(0);
        let _ = CheckMenuRadioItem(
            colours,
            CMD_PAPER as u32,
            (CMD_PAPER + Paper::ALL.len() - 1) as u32,
            (CMD_PAPER + at_paper) as u32,
            windows::Win32::UI::WindowsAndMessaging::MF_BYCOMMAND.0,
        );
        let h = |key: Key| windows::core::HSTRING::from(text(lang, key));
        let _ = AppendMenuW(menu, MF_POPUP, colours.0 as usize, &h(Key::CatColorLabel));
        let _ = AppendMenuW(
            menu,
            if pinned { MF_STRING | MF_CHECKED } else { MF_STRING },
            CMD_PIN,
            &h(Key::StickyPinTop),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_ROLL,
            &h(if rolled { Key::StickyUnroll } else { Key::StickyRoll }),
        );
        let _ = AppendMenuW(menu, MF_STRING, CMD_UNSTICK, &h(Key::StickyOffDesktopTip));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_DELETE,
            &h(if confirm { Key::StickyDeleteAgain } else { Key::StickyDelete }),
        );
        let _ = MF_GRAYED;
        // Without the foreground the menu would not close on a click elsewhere.
        let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            at.x,
            at.y,
            None,
            hwnd,
            None,
        );
        let _ = PostMessageW(
            Some(hwnd),
            windows::Win32::UI::WindowsAndMessaging::WM_NULL,
            WPARAM(0),
            LPARAM(0),
        );
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    };
    match cmd {
        0 => {}
        c if (CMD_PAPER..CMD_PAPER + Paper::ALL.len()).contains(&c) => {
            set_paper(hwnd, Paper::ALL[c - CMD_PAPER]);
        }
        CMD_PIN => set_pinned(hwnd, !pinned),
        CMD_ROLL => toggle_rolled(hwnd),
        CMD_UNSTICK => {
            let _ = with(|s| {
                let i = idx_of(s, hwnd)?;
                let id = s.wins[i].id.clone();
                push(s, WidgetEvent::Unstick { id });
                Some(())
            });
        }
        CMD_DELETE => {
            let _ = with(|s| {
                let i = idx_of(s, hwnd)?;
                if s.wins[i].confirm_delete {
                    let id = s.wins[i].id.clone();
                    push(s, WidgetEvent::Delete { id });
                } else {
                    // The first click arms it; the next other action disarms.
                    s.wins[i].confirm_delete = true;
                    s.wins[i].dirty = true;
                    draw(s, i);
                }
                Some(())
            });
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ dragging

fn end_drag(s: &mut State, i: usize) {
    let Some(d) = s.wins[i].drag.take() else {
        return;
    };
    for j in d.lit {
        if let Some(w) = s.wins.get_mut(j) {
            w.guides.clear();
            w.dirty = true;
        }
    }
    let w = &mut s.wins[i];
    w.guides.clear();
    w.dirty = true;
    if d.moved {
        let monitor = s
            .displays
            .get(w.display)
            .map(place::path_to_save)
            .unwrap_or_default();
        w.sticky.monitor = monitor.clone();
        let (id, rect) = (w.id.clone(), w.sticky.rect);
        push(
            s,
            WidgetEvent::Placed {
                id,
                monitor,
                rect,
            },
        );
    }
    flush_dirty_in(s);
}

fn flush_dirty_in(s: &mut State) {
    for i in 0..s.wins.len() {
        if s.wins[i].dirty {
            draw(s, i);
        }
    }
}

/// The pointer moved at `cursor` (px) while a note is dragged or resized.
fn drag_move(s: &mut State, i: usize, cursor: (i32, i32), alt: bool) {
    let (kind, start_cursor, start_note, start_dip, moved) = {
        let d = s.wins[i].drag.as_ref().expect("a drag");
        (d.kind, d.cursor, d.note, d.dip, d.moved)
    };
    let (dx, dy) = (cursor.0 - start_cursor.0, cursor.1 - start_cursor.1);
    if !moved && dx.abs() + dy.abs() < 4 {
        return;
    }
    if let Some(d) = s.wins[i].drag.as_mut() {
        d.moved = true;
    }
    let rolled = s.wins[i].sticky.rolled;
    let (di, desired) = match kind {
        DragKind::Move => {
            let desired = Rect::new(start_note.x + dx, start_note.y + dy, start_note.w, start_note.h);
            let centre = (desired.x + desired.w / 2, desired.y + desired.h / 2);
            let Some(di) = place::display_at(&s.displays, centre) else {
                return;
            };
            (di, desired)
        }
        DragKind::Resize => (
            s.wins[i].display,
            Rect::new(start_note.x, start_note.y, start_note.w + dx, start_note.h + dy),
        ),
    };
    let Some(disp) = s.displays.get(di).cloned() else {
        return;
    };
    let scale = place::scale_of(disp.monitor.dpi);
    let area_px = place::area_px(&disp);
    let area = place::snap_area(&disp);
    let rel = place::to_dip(&desired, &area_px, scale);
    // The other notes on that display, as they are shown.
    let mut idxs: Vec<usize> = Vec::new();
    let mut others: Vec<DipRect> = Vec::new();
    for (j, w) in s.wins.iter().enumerate() {
        if j != i && w.display == di {
            idxs.push(j);
            others.push(DipRect {
                h: shown_h(w.sticky.rect.h, w.sticky.rolled),
                ..w.sticky.rect
            });
        }
    }
    let snapped = match kind {
        DragKind::Move => snap_move(
            DipRect::new(rel.x, rel.y, start_dip.w, start_dip.h),
            &area,
            &others,
            alt,
        ),
        DragKind::Resize => {
            let w = &s.wins[i];
            snap_resize(
                DipRect::new(w.sticky.rect.x, w.sticky.rect.y, rel.w, rel.h),
                &area,
                &others,
                alt,
            )
        }
    };
    let rect = snapped.rect;
    let w = &mut s.wins[i];
    let rescaled = w.scale != scale || w.display != di;
    w.sticky.rect.x = rect.x;
    w.sticky.rect.y = rect.y;
    if kind == DragKind::Resize {
        w.sticky.rect.w = rect.w;
        w.sticky.rect.h = rect.h;
    }
    let h_shown = shown_h(w.sticky.rect.h, rolled);
    let px = place::to_px(
        &DipRect::new(rect.x, rect.y, w.sticky.rect.w, h_shown),
        &area_px,
        scale,
    );
    let size_changed = px.w != w.note.w || px.h != w.note.h;
    w.note = px;
    w.scale = scale;
    w.display = di;
    w.sticky.monitor = place::path_to_save(&disp);
    if rescaled || size_changed {
        w.canvas = None;
        w.body_key = None;
        w.dirty = true;
    }
    // Guide lines, in this note and in the ones it lines up with.
    let mut mine: Vec<GuideLine> = Vec::new();
    let mut theirs: Vec<(usize, GuideLine)> = Vec::new();
    for g in &snapped.guides {
        let line_in = |x: f32, y: f32| GuideLine {
            vertical: g.axis == Axis::X,
            pos: if g.axis == Axis::X { g.pos - x } else { g.pos - y },
        };
        mine.push(line_in(rect.x, rect.y));
        if let Some(o) = g.other
            && let (Some(j), Some(r)) = (idxs.get(o), others.get(o))
        {
            theirs.push((*j, line_in(r.x, r.y)));
        }
    }
    let lit_before: Vec<usize> = s.wins[i].drag.as_ref().map(|d| d.lit.clone()).unwrap_or_default();
    for j in &lit_before {
        if let Some(w) = s.wins.get_mut(*j) {
            if !theirs.iter().any(|(k, _)| k == j) && !w.guides.is_empty() {
                w.guides.clear();
                w.dirty = true;
            }
        }
    }
    let mut lit = Vec::new();
    for (j, line) in theirs {
        let w = &mut s.wins[j];
        if !lit.contains(&j) {
            if w.guides.is_empty() || !lit_before.contains(&j) {
                w.guides.clear();
            }
            lit.push(j);
        }
        if !w.guides.contains(&line) {
            w.guides.push(line);
            w.dirty = true;
        }
    }
    if let Some(d) = s.wins[i].drag.as_mut() {
        d.lit = lit;
    }
    if s.wins[i].guides != mine {
        s.wins[i].guides = mine;
        s.wins[i].dirty = true;
    }
    // A move that changed nothing visible is just the window going elsewhere.
    let w = &mut s.wins[i];
    if !w.dirty
        && let Some(canvas) = &w.canvas
    {
        let m = margins();
        let origin = (
            w.note.x - (m.0 * w.scale).round() as i32,
            w.note.y - (m.1 * w.scale).round() as i32,
        );
        let _ = canvas.present(w.hwnd, origin, 255);
    }
    flush_dirty_in(s);
}

// ------------------------------------------------------------------ keyboard

fn blink_ms() -> Option<u32> {
    let t = unsafe { GetCaretBlinkTime() };
    (t != 0 && t != u32::MAX).then_some(t)
}

/// Gives note `hwnd` the keyboard: the hidden EDIT is made, focused, and the
/// caret put at `at` (the end of the text when `None`). Already editing: only
/// the caret moves.
fn begin_editing(hwnd: HWND, at: Option<usize>) {
    let info = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        let area = paint::body_rect(w.sticky.rect.w, w.sticky.rect.h);
        Some((
            w.edit,
            w.text.clone(),
            ((area.w * w.scale) as i32, (area.h * w.scale) as i32),
        ))
    })
    .flatten();
    let Some((existing, t, size)) = info else { return };
    let end = t.encode_utf16().count();
    let at = at.unwrap_or(end).min(end);
    let edit_hwnd = match existing {
        Some(e) => e,
        None => {
            let Ok(e) = edit::create(hwnd, &t, size) else {
                return;
            };
            e
        }
    };
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &mut s.wins[i];
        w.edit = Some(edit_hwnd);
        w.caret_on = true;
        w.sel = (at, at);
        w.anchor = at;
        w.follow_caret = true;
        w.dirty = true;
        Some(())
    });
    unsafe {
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(edit_hwnd));
    }
    edit::set_selection(edit_hwnd, &t, at, at);
    if existing.is_none()
        && let Some(ms) = blink_ms()
    {
        unsafe {
            SetTimer(Some(hwnd), TIMER_WIDGET_CARET, ms, None);
        }
    }
    sync_from_edit(hwnd, edit_hwnd);
}

/// The note lets go of the keyboard.
fn end_editing(hwnd: HWND) {
    let edit_hwnd = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &mut s.wins[i];
        let e = w.edit.take()?;
        w.caret_on = false;
        w.selecting = false;
        w.dirty = true;
        Some(e)
    })
    .flatten();
    if let Some(e) = edit_hwnd {
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER_WIDGET_CARET);
            let _ = DestroyWindow(e);
        }
    }
    flush_dirty();
}

/// What the hidden EDIT tells the note (called from its own procedure).
pub(crate) fn edit_event(parent: HWND, edit_hwnd: HWND, ev: EditEvent) {
    match ev {
        EditEvent::Changed => sync_from_edit(parent, edit_hwnd),
        EditEvent::Escape => {
            if pop::is_open() {
                pop::close();
            } else {
                unsafe {
                    let _ = PostMessageW(Some(parent), WM_END_EDIT, WPARAM(0), LPARAM(0));
                }
            }
        }
        EditEvent::Blur => {
            let current = with(|s| {
                let i = idx_of(s, parent)?;
                Some(s.wins[i].edit == Some(edit_hwnd))
            })
            .flatten()
            .unwrap_or(false);
            // A popover taking the focus is not the end of the editing.
            if current && !pop::is_open() {
                unsafe {
                    let _ = PostMessageW(Some(parent), WM_END_EDIT, WPARAM(0), LPARAM(0));
                }
            }
        }
        EditEvent::Vertical { up, extend } => move_vertical(parent, edit_hwnd, up, extend),
    }
}

/// Reads the EDIT's text and selection into the note and draws it.
fn sync_from_edit(parent: HWND, edit_hwnd: HWND) {
    let t = edit::text(edit_hwnd);
    let sel = edit::selection(edit_hwnd, &t);
    let ime = with(|s| {
        let i = idx_of(s, parent)?;
        if s.wins[i].edit != Some(edit_hwnd) {
            return None;
        }
        let w = &mut s.wins[i];
        let before = std::mem::replace(&mut w.text, t);
        if w.sel != sel && sel.0 == sel.1 {
            w.anchor = sel.0;
        }
        w.sel = sel;
        w.caret_on = true;
        w.follow_caret = true;
        if w.text != before {
            touch(s, i);
        }
        text_changed(s, i, &before);
        let w = &s.wins[i];
        let (cx, cy, _) = w.body.as_ref()?.caret(sel.1)?;
        let m = margins();
        let area = paint::body_rect(w.sticky.rect.w, w.sticky.rect.h);
        Some((
            ((m.0 + area.x + cx) * w.scale) as i32,
            ((m.1 + area.y + cy - w.scroll) * w.scale) as i32,
        ))
    })
    .flatten();
    if let Some((x, y)) = ime {
        edit::place_ime(edit_hwnd, x, y);
    }
}

/// An arrow up or down: the caret goes one visual line, where the drawn text
/// says (the EDIT's own wrapping is not what is on screen).
fn move_vertical(parent: HWND, edit_hwnd: HWND, up: bool, extend: bool) {
    let target = with(|s| {
        let i = idx_of(s, parent)?;
        let w = &s.wins[i];
        let body = w.body.as_ref()?;
        let active = if w.sel.0 == w.anchor { w.sel.1 } else { w.sel.0 };
        let (x, y, h) = body.caret(active)?;
        let want_y = if up { y - h * 0.5 } else { y + h * 1.5 };
        let mut to = body.index_at(x, want_y);
        let end = w.text.encode_utf16().count();
        if to == active {
            to = if up { 0 } else { end };
        }
        Some((w.text.clone(), w.anchor, to, w.sel))
    })
    .flatten();
    let Some((t, anchor, to, _)) = target else { return };
    if extend {
        edit::set_selection(edit_hwnd, &t, anchor, to);
    } else {
        edit::set_selection(edit_hwnd, &t, to, to);
    }
    sync_from_edit(parent, edit_hwnd);
}
