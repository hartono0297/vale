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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::{DateTime, Local, NaiveDate};
use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_MENU,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, GWLP_HWNDPARENT, GWLP_USERDATA, GetCaretBlinkTime, GetCursorPos, GetShellWindow,
    GetWindowLongPtrW, HWND_BOTTOM, HWND_TOPMOST, IDC_ARROW, IDC_HAND, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENWSE,
    KillTimer, LoadCursorW, MA_ACTIVATE, MA_NOACTIVATE, MF_CHECKED, MF_POPUP, MF_SEPARATOR,
    MF_GRAYED, MF_STRING, PostMessageW, RegisterClassExW, SW_HIDE, SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SetCursor, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WINDOWPOS, WM_ACTIVATE, WM_APP,
    WM_CAPTURECHANGED, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_RBUTTONUP, WM_SETCURSOR, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WM_WINDOWPOSCHANGING, WNDCLASSEXW, WS_EX_LAYERED,
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
use crate::widgets::art;
use crate::widgets::checklist::{self, Edit};
use crate::widgets::drop;
use crate::widgets::mood::{self, Mood};
use crate::widgets::edit::{self, EditEvent};
use crate::widgets::clip_io;
use crate::widgets::fmt_pop;
use crate::widgets::richclip;
use crate::ui::geometry::Rect as DipR;
use crate::widgets::image_io;
use crate::widgets::images;
use crate::widgets::paint::{
    self, BarScene, BodyLayout, Btn, CatPose, Extras, GuideLine, HEADER_H, ImgCache, ImgScene, Res,
    Scene, Target, shown_h, window_dip,
};
use crate::widgets::toolbar::{self, BarHit, Tool};
use crate::widgets::place;
use crate::widgets::pop::{self, PopCmd, PopState};
use crate::widgets::stack::{self, Existing};
use crate::widgets::tuck::{self, Edge};
use crate::widgets::remind::{self, Preset};
use crate::widgets::remind_pop;
use crate::widgets::prefs::{FullscreenRule, WidgetLayer, WidgetPrefs};
use crate::widgets::snap::{Axis, Snapped, snap_move_for, snap_resize};
use crate::widgets::sticky::{
    self, Deco, DipRect, NoteFont, Paper, Pattern, Stamp, Sticky, TextSize,
};
use crate::widgets::styles::{self, Attr, Hl, Run, Style};
use crate::widgets::templates::Template;

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
    /// The note's formatting is now these runs (over the note's text; the
    /// owner normalises them). Pushed right after a `Text` when the text
    /// changed, and alone for a style-only change: it wins over the store's
    /// own rebase of the text edit.
    Styles { id: String, runs: Vec<Run> },
    /// The note's images are now these file names (the owner keeps only valid
    /// names, at most four).
    Images { id: String, images: Vec<String> },
    /// The note was moved or resized: its monitor and rect (dip).
    Placed {
        id: String,
        monitor: String,
        rect: DipRect,
    },
    Paper { id: String, paper: Paper },
    Size { id: String, size: TextSize },
    Font { id: String, font: NoteFont },
    Pattern { id: String, pattern: Pattern },
    Deco { id: String, deco: Deco },
    Stamp { id: String, stamp: Stamp },
    /// "Tembus pandang": 100, 80, 60 or 40 percent while the pointer is away.
    Opacity { id: String, percent: u8 },
    Pinned { id: String, on: bool },
    Rolled { id: String, on: bool },
    /// The note's reminder was set (unix seconds) or cleared (`None`): "Ingatkan
    /// aku", the chip's menu, or opening an overdue note.
    Remind { id: String, at: Option<i64> },
    /// "Kucing ngintip" was switched from a note's popover.
    CatPeek(bool),
    /// The (+) of this note's header: a left click is `Template::Kosong`, a
    /// right click chose one.
    NewBeside { id: String, template: Template },
    /// "Lepas dari desktop" (the x): the note stays unless it is empty.
    Unstick { id: String },
    /// "Hapus catatan".
    Delete { id: String },
    /// "Kirim ke HP": the island shows this note on the phone link.
    SendToPhone { id: String },
    /// The note was used: the cat goes to the one touched last.
    Touched { id: String, at: i64 },
    /// A note was dropped on another with Shift held: the units of these
    /// sheets (in order) become one stack. Structural.
    Stack { dragged: Vec<String>, target: Vec<String> },
    /// The stack now shows this sheet (a flip: the wheel, the arrows).
    StackShow { id: String },
    /// "Keluarkan dari tumpukan": this sheet leaves its stack and is placed
    /// beside it. Structural.
    StackOut { id: String },
    /// The note was tucked to an edge, or brought out (`None`).
    Tuck { id: String, edge: Option<Edge> },
    /// The list button of this note's header: the owner opens the sticky note
    /// list beside it.
    OpenList { id: String },
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

/// What a pop looked like when it was last drawn: the note's scale, the note's
/// and the tab's opacity.
#[derive(Clone, Copy, PartialEq)]
struct PopDrawn {
    scale: f32,
    note: f32,
    tab: f32,
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
    /// Where the pointer was last (desktop px).
    last: (i32, i32),
    /// The edge a release here would tuck to.
    cue: Option<Edge>,
    /// The unit a release here (Shift held) would stack on.
    target: Option<HWND>,
}

/// The stack a window shows: its id, its sheets' note ids in order, and the
/// place of the shown one.
#[derive(Clone, Debug, PartialEq)]
struct StackView {
    id: String,
    ids: Vec<String>,
    pos: usize,
}

/// What the window knows of one sheet of its stack: the shown sheet's data
/// here is stale (the window's own fields are live).
#[derive(Clone, Debug)]
struct Sheet {
    id: String,
    sticky: Sticky,
    text: String,
    updated: i64,
}

struct Win {
    /// The note id of the sheet this window shows.
    id: String,
    hwnd: HWND,
    sticky: Sticky,
    text: String,
    updated: i64,
    /// The note's rect on the desktop (px), the monitor's scale and its place
    /// in the display list.
    note: Rect,
    /// The note as shown (`place::Placement::view`): dip from its display's
    /// placement area, the size clamped to that area. The window, the text
    /// layout, the hit-testing and the EDIT use this size; `sticky.rect` is
    /// the saved one, which may be larger than this display.
    view: DipRect,
    scale: f32,
    display: usize,
    canvas: Option<Canvas>,
    body: Option<BodyLayout>,
    /// What the laid-out text was made from: the text, the font, the size,
    /// the width, a hash of the formatting and the image block's height. (The
    /// text colours are drawing effects made at each draw, so neither the paper
    /// nor the canvas is part of it.)
    body_key: Option<(String, NoteFont, TextSize, i32, u64, i32)>,
    scroll: f32,
    header: Ease,
    hot: Option<Btn>,
    tracking: bool,
    pressed: Option<Btn>,
    /// Where the press on `pressed` was (desktop px): a move of 4 px or more
    /// from here turns the press into a drag of the note.
    press_at: (i32, i32),
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
    /// A reminder fired: the note sits above other windows until it is
    /// touched, snoozed, opened or its reminder cleared.
    raised: bool,
    /// The sticky note list brought this note to the front (`bring_front`):
    /// it sits above other windows, even on the desktop layer, until it loses
    /// the keyboard.
    lifted: bool,
    /// When the reminder's shake began.
    wiggle: Option<Instant>,
    /// When the confetti began, and its seed.
    confetti: Option<(Instant, u64)>,
    /// "Miring sedikit" is on (mirrors the prefs): the note leans.
    tilt: bool,
    /// The pointer is over the note, and where along the cat it points (a
    /// step, see `mood::look_step`).
    pointer_in: bool,
    look: i8,
    /// The mood the cat was last drawn in on this note.
    mood: Mood,
    /// The window's opacity, 0 to 1: the note's see-through level while the
    /// pointer is away, full while it is over, has the keyboard, is raised or
    /// dragged.
    alpha: Ease,
    /// The opacity (0..=255) the window was last shown at.
    alpha_shown: u8,
    /// Hidden by a fullscreen app on its monitor.
    away: bool,
    shown: bool,
    /// Something a note takes is being dragged over it (the accent dashed
    /// outline): set on an accepted `DragEnter`, cleared on a leave or a drop.
    drop_hover: bool,
    dirty: bool,
    /// Set for a stack: the window shows one sheet of it.
    stack: Option<StackView>,
    /// Every sheet of the stack in order (empty for a plain note).
    sheets: Vec<Sheet>,
    /// How far a tucked note is out: progress 0 only the tab, 1 the whole note
    /// (settled at 1 for a note that is not tucked), and its scale and opacity
    /// on the way. `pop_drawn` is what was last drawn.
    pop: tuck::Pop,
    pop_drawn: PopDrawn,
    /// The edge the note was last tucked to, so an untuck can still animate
    /// (and be drawn) after `sticky.tuck` is none.
    pop_edge: Option<Edge>,
    /// The extra window margin the last frame was drawn with (`tuck::room`
    /// while the note is tucked or popping): the window's real size.
    room: (f32, f32, f32, f32),
    /// The pointer left a note that is out: it pops back into its tab at this
    /// time plus `tuck::GRACE_MS`.
    grace: Option<Instant>,
    /// Wheel delta not yet worth a flip.
    wheel: i32,
    /// A release here would tuck to this edge (drawn as an accent bar).
    cue: Option<Edge>,
    /// A dragged note would stack on this one (drawn with the highlight frame).
    targeted: bool,
    /// A style chosen with a collapsed caret, for the text typed next (cleared
    /// when the caret moves without a text change).
    pending: Option<Style>,
    /// The bar button under the pointer (its tooltip shows), and the one a
    /// press began on.
    hot_bar: Option<BarHit>,
    bar_press: Option<BarHit>,
    /// The image under the pointer, and a press on an image or its remove
    /// button (index, remove, desktop px where it began).
    hot_image: Option<usize>,
    image_press: Option<(usize, bool, (i32, i32))>,
    /// The size of each image file by name (`None`: unreadable), and where
    /// the images go (note dip, from the text's top) with the height of the
    /// block they make above the text.
    img_sizes: HashMap<String, Option<(u32, u32)>>,
    img_rects: Vec<DipR>,
    img_top: f32,
    /// The images' bitmaps (they belong to the canvas).
    img: RefCell<ImgCache>,
    /// A file dialog of this note is open: it taking the keyboard is not the
    /// end of the editing.
    dialog: bool,
    /// The bar is drawn without the keyboard (`DI_WIDGETS_BAR`, debug builds):
    /// a look at the drawing that takes no focus.
    preview: bool,
}

impl Win {
    /// The format bar shows while the note has the keyboard, is not rolled up
    /// and is not tucked in (a tucked note that is not all the way out has no
    /// room for it).
    fn bar_shown(&self, now: Instant) -> bool {
        (self.edit.is_some() || self.preview)
            && !self.sticky.rolled
            && !(self.tuck_edge(now).is_some() && self.pop.progress(now) < 1.0)
    }

    /// The text area as it is now: shorter by the bar while it shows (the
    /// window never resizes). Every user of the area goes through this.
    fn body_area(&self) -> DipR {
        paint::body_rect_in(self.view.w, self.view.h, self.bar_shown(Instant::now()))
    }

    /// What the lean follows: the stack's id (so a flip does not change it),
    /// else the note's. It is also the key a unit's window goes by.
    fn key(&self) -> &str {
        self.stack.as_ref().map_or(self.id.as_str(), |s| s.id.as_str())
    }

    /// The angle the note is drawn at: leaning while "Miring sedikit" is on,
    /// straight while it has the keyboard, while it is tucked and while it
    /// pops (an untuck that animates does not lean mid-scale).
    fn lean(&self) -> f32 {
        if self.tilt
            && self.edit.is_none()
            && self.sticky.tuck.is_none()
            && !self.pop.running(Instant::now())
        {
            art::tilt_deg(self.key())
        } else {
            0.0
        }
    }

    /// The window's margin round the note (grown for the lean, whether or not
    /// the note has the keyboard, so the window keeps its size), plus the
    /// room a tucked note needs for its pop (`room`, set by `draw`).
    fn margins(&self) -> (f32, f32, f32, f32) {
        let deg = if self.tilt { art::tilt_deg(self.key()) } else { 0.0 };
        let b = art::margins_for(self.view.w, shown_h(self.view.h, self.sticky.rolled), deg);
        (
            b.0 + self.room.0,
            b.1 + self.room.1,
            b.2 + self.room.2,
            b.3 + self.room.3,
        )
    }

    /// The edge the note is tucked to, or still popping from after an untuck.
    fn tuck_edge(&self, now: Instant) -> Option<Edge> {
        self.sticky
            .tuck
            .or_else(|| self.pop_edge.filter(|_| self.pop.running(now)))
    }

    /// The room the window should have now (what `draw` will set).
    fn room_now(&self, now: Instant) -> (f32, f32, f32, f32) {
        self.tuck_edge(now).map_or((0.0, 0.0, 0.0, 0.0), |edge| {
            tuck::room(self.view.w, shown_h(self.view.h, self.sticky.rolled), edge, self.stack_edges())
        })
    }

    /// How many sheet edges peek out under a stack's paper (0 for a note).
    fn stack_edges(&self) -> usize {
        self.sheet_count().saturating_sub(1).min(2)
    }

    /// The pop as it is now.
    fn pop_now(&self, now: Instant) -> PopDrawn {
        PopDrawn {
            scale: self.pop.scale(now),
            note: self.pop.note_alpha(now),
            tab: self.pop.tab_alpha(now),
        }
    }

    /// Whether this window shows note `id` (as its sheet or one hidden under it).
    fn has(&self, id: &str) -> bool {
        self.id == id || self.stack.as_ref().is_some_and(|s| s.ids.iter().any(|i| i == id))
    }

    /// The note ids of the unit, in order.
    fn unit_ids(&self) -> Vec<String> {
        self.stack
            .as_ref()
            .map_or_else(|| vec![self.id.clone()], |s| s.ids.clone())
    }

    /// How many sheets (notes) the window stands for.
    fn sheet_count(&self) -> usize {
        self.stack.as_ref().map_or(1, |s| s.ids.len())
    }

    /// Every sheet's sticky part, the shown one live.
    fn stickies(&self) -> Vec<(&str, &Sticky)> {
        if self.sheets.is_empty() {
            return vec![(self.id.as_str(), &self.sticky)];
        }
        self.sheets
            .iter()
            .map(|sh| {
                if sh.id == self.id {
                    (self.id.as_str(), &self.sticky)
                } else {
                    (sh.id.as_str(), &sh.sticky)
                }
            })
            .collect()
    }

    /// A reminder of the unit is overdue (on any sheet).
    fn any_overdue(&self, unix: i64) -> bool {
        self.stickies().iter().any(|(_, s)| remind::overdue(s, unix))
    }

    /// The tab is what shows: tucked (or popping) and not (all the way) out.
    fn tab_showing(&self, now: Instant) -> bool {
        self.tuck_edge(now).is_some() && self.pop.progress(now) < 0.999
    }

    /// The tab's rect in the note's dip, while a tab shows.
    fn tab_dip(&self, now: Instant) -> Option<DipRect> {
        let edge = self.tuck_edge(now)?;
        if self.pop.progress(now) >= 0.999 {
            return None;
        }
        let out = DipRect::new(0.0, 0.0, self.view.w, shown_h(self.view.h, self.sticky.rolled));
        Some(tuck::tab_rect(&out, edge))
    }

    /// The desktop point (px) is on the tab and the tab is drawn (visible
    /// enough to be pressed): a press there moves the note.
    fn on_tab(&self, sx: i32, sy: i32, now: Instant) -> bool {
        self.pop.tab_alpha(now) > 0.0
            && self.tab_dip(now).is_some_and(|tab| {
                let (x, y) = (
                    (sx - self.note.x) as f32 / self.scale,
                    (sy - self.note.y) as f32 / self.scale,
                );
                x >= tab.x && x < tab.right() && y >= tab.y && y < tab.bottom()
            })
    }

    /// The window's top-left on the desktop (px).
    fn origin(&self) -> (i32, i32) {
        let m = self.margins();
        (
            self.note.x - (m.0 * self.scale).round() as i32,
            self.note.y - (m.1 * self.scale).round() as i32,
        )
    }
}

/// Window opacity 0 to 1 as the 0..=255 `UpdateLayeredWindow` takes.
fn alpha_u8(w: &Win, now: Instant) -> u8 {
    (w.alpha.value(now).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Where the opacity is heading: full while the pointer is over the note, it
/// has the keyboard, a reminder raised it or it is being dragged; else its
/// see-through level.
fn alpha_goal(w: &Win) -> f32 {
    let full = w.pointer_in || w.edit.is_some() || w.raised || w.drag.is_some();
    if full {
        1.0
    } else {
        f32::from(w.sticky.opacity) / 100.0
    }
}

/// Note `i`'s opacity follows its state: eased on the frame clock, instant
/// when nothing animates. Only the layered window's alpha moves (`present`),
/// never a redraw.
fn settle_alpha(s: &mut State, i: usize) {
    let (now, animate) = (Instant::now(), s.animate);
    let w = &mut s.wins[i];
    let to = alpha_goal(w);
    if (w.alpha.to - to).abs() < f32::EPSILON {
        return;
    }
    w.alpha.toward(to, now, animate);
    if animate {
        push(s, WidgetEvent::NeedsClock);
    } else {
        present_alpha(s, i);
    }
}

/// Pushes the note's current opacity to its window, without drawing.
fn present_alpha(s: &mut State, i: usize) {
    let w = &mut s.wins[i];
    let a = alpha_u8(w, Instant::now());
    if a == w.alpha_shown {
        return;
    }
    let origin = w.origin();
    if let Some(canvas) = &w.canvas
        && canvas.present(w.hwnd, origin, a).is_ok()
    {
        w.alpha_shown = a;
    }
}

/// Something keeps a tucked note out: the pointer is over it, something is
/// being dragged over it to be dropped, it has the keyboard, it is being
/// dragged, a reminder raised it, or one of its popovers is open.
fn keep_out(w: &Win) -> bool {
    w.pointer_in
        || w.drop_hover
        || w.edit.is_some()
        || w.drag.is_some()
        || w.raised
        || pop::is_open_for(w.hwnd)
        || remind_pop::is_open_for(w.hwnd)
        || fmt_pop::is_open_for(w.hwnd)
}

/// A tucked note follows what keeps it out. While something does it pops out
/// (300 ms on the frame clock, instant when nothing animates) and the grace is
/// off; once nothing does, the grace starts, and the pop back into the tab
/// (200 ms) happens when it ends (`WidgetHost::tick`). The grace counts on the
/// frame clock too, even without animation. Draws what changed at once when
/// the pop is instant.
fn tuck_settle(s: &mut State, i: usize) {
    let (now, animate) = (Instant::now(), s.animate);
    let w = &mut s.wins[i];
    let Some(edge) = w.sticky.tuck else {
        return;
    };
    w.pop_edge = Some(edge);
    let mut clock = false;
    if keep_out(w) {
        w.grace = None;
        if w.pop.to < 1.0 {
            w.pop.toward(1.0, now, animate);
            w.dirty = true;
            clock = animate;
        }
    } else if w.pop.to > 0.0 && w.grace.is_none() {
        w.grace = Some(now);
        clock = true;
    }
    if clock {
        push(s, WidgetEvent::NeedsClock);
    }
    if s.wins[i].dirty && !s.animate {
        draw(s, i);
    }
}

/// The note is all the way out at once, and nothing is waiting to pull it back.
fn pop_out_now(w: &mut Win) {
    w.pop = tuck::Pop::at(1.0);
    w.grace = None;
    w.dirty = true;
}

/// How many notes (sheets) the windows stand for: what the cap counts.
fn sheet_total(wins: &[Win]) -> usize {
    wins.iter().map(Win::sheet_count).sum()
}

/// Shift is held (read on every move, like Alt). The tests set it.
fn shift_down() -> bool {
    #[cfg(test)]
    if let Some(v) = TEST_SHIFT.with(std::cell::Cell::get) {
        return v;
    }
    (unsafe { GetKeyState(i32::from(VK_SHIFT.0)) }) < 0
}

#[cfg(test)]
thread_local! {
    static TEST_SHIFT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// What the cat looks like on note `w` at `unix`. On a stack the cat may sit
/// on a hidden sheet, so the touch that counts is the latest of any sheet.
fn win_mood(w: &Win, unix: i64) -> Mood {
    let touched = w.stickies().iter().map(|(_, s)| s.touched).max().unwrap_or(0);
    mood::mood(
        w.any_overdue(unix),
        checklist::all_done(&w.text),
        touched,
        unix,
        w.pointer_in,
    )
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
    /// The local date the 1 s tick last saw: the notes' labels (a
    /// reminder's chip, the footer's "Hari ini") are drawn against it.
    day: Option<NaiveDate>,
    /// Where the images live (`NotesStore::images_dir`); empty until the owner
    /// says (`WidgetHost::set_images_dir`): nothing loads then.
    images_dir: PathBuf,
    /// The highlight the Marker button and Ctrl+Shift+H apply: Kuning at
    /// first, then the last one chosen. In memory per host, never saved.
    last_hl: Hl,
    /// The notes with an image import in flight (one entry per import).
    in_flight: Vec<String>,
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

/// Whether the local date moved on from `last`, and remember `today`. The
/// first look only remembers it: the notes were just drawn against it.
fn day_turned(last: &mut Option<NaiveDate>, today: NaiveDate) -> bool {
    let turned = last.is_some_and(|d| d != today);
    *last = Some(today);
    turned
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

/// The two-click delete goes back to its first click (the footer follows).
fn disarm_delete(s: &mut State, i: usize) {
    if s.wins[i].confirm_delete {
        s.wins[i].confirm_delete = false;
        s.wins[i].dirty = true;
    }
}

/// The note was used: now it is the one touched last.
fn touch(s: &mut State, i: usize) {
    // Any change to the note is an "other action": the delete is not armed.
    disarm_delete(s, i);
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
    // The note with an overdue reminder (the earliest), else the one touched
    // last.
    // Every sheet of a stack counts: the cat sits on the window holding it.
    let owner = mood::cat_owner(s.wins.iter().flat_map(Win::stickies), unix_now()).map(str::to_owned);
    if owner == s.peek {
        return;
    }
    let before = s.peek.take();
    s.peek = owner.clone();
    for w in &mut s.wins {
        if before.as_deref().is_some_and(|id| w.has(id)) || owner.as_deref().is_some_and(|id| w.has(id)) {
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
                day: None,
                images_dir: PathBuf::new(),
                last_hl: Hl::Kuning,
                in_flight: Vec::new(),
            });
        });
        WidgetHost { _private: () }
    }

    /// Brings the windows to `ctx`: one per note that has a sticky part,
    /// created, moved, styled and hidden or shown as the settings say.
    pub fn sync(&mut self, ctx: &Ctx) {
        sync(ctx);
    }

    /// `DI_WIDGETS_BAR` (debug builds): draws note `id`'s format bar without the
    /// keyboard or the foreground; `marker` and `more` also show the popup
    /// (with `SW_SHOWNA`: no activation).
    #[cfg(debug_assertions)]
    pub fn preview_bar(&mut self, id: &str, mode: &str) {
        let hwnd = with(|s| {
            let i = s.wins.iter().position(|w| w.has(id))?;
            s.wins[i].preview = true;
            s.wins[i].dirty = true;
            Some(s.wins[i].hwnd)
        })
        .flatten();
        flush_dirty();
        let Some(hwnd) = hwnd else { return };
        match mode {
            "marker" => open_marker_pop(hwnd),
            "more" => open_overflow(hwnd),
            _ => {}
        }
    }

    /// Where the notes' images live (`NotesStore::images_dir`). Until the owner
    /// says, no image is loaded or imported.
    pub fn set_images_dir(&mut self, dir: PathBuf) {
        let _ = with(|s| {
            if s.images_dir != dir {
                s.images_dir = dir;
                for w in &mut s.wins {
                    *w.img.borrow_mut() = ImgCache::default();
                    w.img_sizes.clear();
                    w.dirty = true;
                }
            }
        });
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

    /// Gives note `id` the keyboard (a fresh note), the caret at the end.
    pub fn focus(&mut self, id: &str) {
        self.focus_at(id, None);
    }

    /// Gives note `id` the keyboard with the caret at model index `at` (the
    /// end of the text when `None`): a template's caret is the end of its
    /// first line.
    pub fn focus_at(&mut self, id: &str, at: Option<usize>) {
        let hwnd = with(|s| s.wins.iter().find(|w| w.has(id)).map(|w| (w.hwnd, w.id != id))).flatten();
        if let Some((hwnd, other_sheet)) = hwnd {
            // A sheet hidden in a stack comes to the top first.
            if other_sheet {
                end_editing(hwnd);
                let _ = with(|s| {
                    let i = idx_of(s, hwnd)?;
                    let to = s.wins[i].stack.as_ref()?.ids.iter().position(|x| x == id)?;
                    show_sheet(s, i, to);
                    Some(())
                });
                flush_dirty();
            }
            take_foreground(hwnd);
            begin_editing(hwnd, at);
        }
    }

    /// The note's window on the desktop (px), the note itself without the
    /// margin: what the sticky note list sits beside. `None` when no window
    /// shows note `id`.
    pub fn note_rect_px(&self, id: &str) -> Option<Rect> {
        with(|s| s.wins.iter().find(|w| w.has(id)).map(|w| w.note)).flatten()
    }

    /// Gives note `id` the keyboard and puts it over other windows, even on
    /// the desktop layer (the sticky note list's "show and focus"). It goes
    /// back to its layer when it loses the keyboard (`end_editing`).
    pub fn bring_front(&mut self, id: &str) {
        self.focus(id);
        let i = with(|s| {
            let i = s.wins.iter().position(|w| w.has(id))?;
            // Only a note that did get the keyboard: nothing would end it.
            if s.wins[i].edit.is_none() {
                return None;
            }
            s.wins[i].lifted = true;
            Some(i)
        })
        .flatten();
        if let Some(i) = i {
            apply_z(i);
        }
    }

    /// The reminders of notes `ids` went off: each note sits above other
    /// windows (even on the desktop layer) until it is touched, snoozed,
    /// opened or its reminder cleared, and shakes for 600 ms unless the owner
    /// does not animate (Motion not Full, Energy Saver, Windows animations
    /// off). The island's notice is the owner's.
    pub fn raise(&mut self, ids: &[String]) {
        let raised: Vec<usize> = with(|s| {
            let (animate, now) = (s.animate, Instant::now());
            let mut out = Vec::new();
            for (i, w) in s.wins.iter_mut().enumerate() {
                if ids.iter().any(|id| w.has(id)) {
                    w.raised = true;
                    if animate {
                        w.wiggle = Some(now);
                    }
                    w.dirty = true;
                    out.push(i);
                }
            }
            if animate && !out.is_empty() {
                push(s, WidgetEvent::NeedsClock);
            }
            for i in &out {
                settle_alpha(s, *i);
                // A tucked note (or stack) pops out and stays out.
                tuck_settle(s, *i);
            }
            // The overdue note gets the cat (with a clock).
            refresh_peek(s);
            out
        })
        .unwrap_or_default();
        for i in raised {
            apply_z(i);
        }
        flush_dirty();
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
                    || w
                        .wiggle
                        .is_some_and(|t| elapsed_ms(t, now) < paint::WIGGLE_MS)
                    || w
                        .confetti
                        .is_some_and(|(t, _)| elapsed_ms(t, now) < art::CONFETTI_MS)
                    || w.alpha.running(now)
                    // The tuck: the grace and the pop run on the frame clock.
                    || w.grace.is_some()
                    || w.pop.running(now)
            })
        })
        .unwrap_or(false)
    }

    /// One frame of the easing: the notes that move are drawn.
    pub fn tick(&mut self, now: Instant) {
        let _ = with(|s| {
            let animate = s.animate;
            for w in &mut s.wins {
                // A note that was out and lost what kept it out pops back into its
                // tab once the grace is over; re-entering cancelled it.
                if let Some(t) = w.grace
                    && elapsed_ms(t, now) >= tuck::GRACE_MS
                {
                    w.grace = None;
                    if w.sticky.tuck.is_some() && !keep_out(w) {
                        w.pop.toward(0.0, now, animate);
                    }
                }
                // Only a change is drawn (nothing during the grace), so the
                // pop's last frame is drawn too, and the window gives its extra
                // room back once the pop is over.
                if w.pop_now(now) != w.pop_drawn || w.room_now(now) != w.room {
                    w.dirty = true;
                }
                let fading = w.header.running(now);
                let lit = match w.highlight {
                    Some(t) if elapsed_ms(t, now) >= HIGHLIGHT_MS => {
                        w.highlight = None;
                        true
                    }
                    Some(_) => true,
                    None => false,
                };
                let shaking = match w.wiggle {
                    Some(t) if elapsed_ms(t, now) >= paint::WIGGLE_MS => {
                        w.wiggle = None;
                        true
                    }
                    Some(_) => true,
                    None => false,
                };
                let sprinkling = match w.confetti {
                    Some((t, _)) if elapsed_ms(t, now) >= art::CONFETTI_MS => {
                        w.confetti = None;
                        true
                    }
                    Some(_) => true,
                    None => false,
                };
                if fading || lit || shaking || sprinkling {
                    w.dirty = true;
                }
            }
            // The see-through fade only moves the window's alpha.
            for i in 0..s.wins.len() {
                if !s.wins[i].dirty {
                    present_alpha(s, i);
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

    /// The 1 s system tick, next to the fullscreen check: the cat's note is
    /// drawn again only when its mood changed (asleep after three days), and
    /// every note once when the local date turns (a reminder's chip gains its
    /// day, "Hari ini 10.00" becomes "Kemarin"). Nothing else moves; no timer
    /// of its own.
    pub fn mood_tick(&mut self, unix: i64) {
        let _ = with(|s| {
            if day_turned(&mut s.day, remind::from_unix(unix).date()) {
                for w in &mut s.wins {
                    w.dirty = true;
                }
            }
            if !s.prefs.cat_peek {
                return;
            }
            let Some(i) = s
                .peek
                .as_deref()
                .and_then(|id| s.wins.iter().position(|w| w.has(id)))
            else {
                return;
            };
            if win_mood(&s.wins[i], unix) != s.wins[i].mood {
                s.wins[i].dirty = true;
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

    /// How many notes there are on the desktop (every sheet of a stack counts).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn count(&self) -> usize {
        with(|s| sheet_total(&s.wins)).unwrap_or(0)
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
    w.sticky.pinned || s.prefs.layer == WidgetLayer::Top || w.raised || w.lifted
}

// ------------------------------------------------------------------ sync

fn sync(ctx: &Ctx) {
    // 1. What the notes are: one unit each, a plain note or a whole stack (the
    // cap counts sheets, so the first twenty), against the windows there are.
    let units = stack::units(
        ctx.notes
            .iter()
            .filter(|n| n.sticky.is_some())
            .take(sticky::MAX_STICKY),
    );
    let matched: Vec<Option<HWND>> = with(|s| {
        let existing: Vec<Existing> = s
            .wins
            .iter()
            .map(|w| Existing {
                key: w.key(),
                shown: w.id.as_str(),
                ids: w.stack.as_ref().map_or_else(
                    || vec![w.id.as_str()],
                    |st| st.ids.iter().map(String::as_str).collect(),
                ),
            })
            .collect();
        stack::match_units(&existing, &units)
            .into_iter()
            .map(|m| m.map(|e| s.wins[e].hwnd))
            .collect()
    })
    .unwrap_or_else(|| vec![None; units.len()]);
    let keep: Vec<HWND> = matched.iter().flatten().copied().collect();
    let gone: Vec<HWND> = with(|s| {
        s.wins
            .iter()
            .map(|w| w.hwnd)
            .filter(|h| !keep.contains(h))
            .collect()
    })
    .unwrap_or_default();
    for hwnd in gone {
        end_editing(hwnd);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    // A window that is to show another sheet than it does stops editing first.
    let swap: Vec<HWND> = with(|s| {
        units
            .iter()
            .zip(&matched)
            .filter_map(|(u, m)| {
                let hwnd = (*m)?;
                let top = &u.sheets[u.top()].id;
                s.wins
                    .iter()
                    .find(|w| w.hwnd == hwnd && w.id != *top)
                    .map(|w| w.hwnd)
            })
            .collect()
    })
    .unwrap_or_default();
    for hwnd in swap {
        end_editing(hwnd);
    }
    let mut assigned = matched;
    let mut fresh = vec![false; units.len()];
    for (ui, slot) in assigned.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = create_window();
            fresh[ui] = slot.is_some();
            if let Some(hwnd) = *slot {
                drop::register(hwnd);
            }
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
        for (ui, unit) in units.iter().enumerate() {
            let Some(hwnd) = assigned[ui] else { continue };
            if fresh[ui] {
                let top = unit.sheets[unit.top()];
                s.wins.push(blank_win(hwnd, top, s.prefs.tilt));
            }
            let Some(i) = s.wins.iter().position(|w| w.hwnd == hwnd) else {
                continue;
            };
            update_win(s, i, unit, prefs_changed, ctx.prefs.tilt);
        }
        for i in 0..s.wins.len() {
            settle_alpha(s, i);
            tuck_settle(s, i);
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

/// A window for a note that has none yet: nothing is placed or drawn until
/// `update_win` and `relayout` have run.
fn blank_win(hwnd: HWND, note: &Note, tilt: bool) -> Win {
    let sticky = note.sticky.clone().unwrap_or_default();
    let opacity = sticky.opacity;
    let out = if sticky.tuck.is_some() { 0.0 } else { 1.0 };
    let sticky_tuck = sticky.tuck;
    Win {
        id: note.id.clone(),
        hwnd,
        sticky,
        text: note.text.clone(),
        updated: note.updated,
        note: Rect::default(),
        view: DipRect::new(0.0, 0.0, 0.0, 0.0),
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
        press_at: (0, 0),
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
        raised: false,
        lifted: false,
        wiggle: None,
        confetti: None,
        tilt,
        pointer_in: false,
        look: 0,
        mood: Mood::Idle,
        alpha: Ease::at(f32::from(opacity) / 100.0),
        alpha_shown: 255,
        away: false,
        shown: false,
        drop_hover: false,
        dirty: true,
        stack: None,
        sheets: Vec::new(),
        pop: tuck::Pop::at(out),
        pop_drawn: PopDrawn {
            scale: 1.0,
            note: 1.0,
            tab: 0.0,
        },
        pop_edge: sticky_tuck,
        room: (0.0, 0.0, 0.0, 0.0),
        grace: None,
        wheel: 0,
        cue: None,
        targeted: false,
        pending: None,
        hot_bar: None,
        bar_press: None,
        hot_image: None,
        image_press: None,
        img_sizes: HashMap::new(),
        img_rects: Vec::new(),
        img_top: 0.0,
        img: RefCell::new(ImgCache::default()),
        dialog: false,
        preview: false,
    }
}

/// What a note's view of its text starts from again when another sheet takes
/// the window.
fn reset_sheet_view(w: &mut Win) {
    w.body_key = None;
    w.scroll = 0.0;
    w.sel = (0, 0);
    w.anchor = 0;
    w.follow_caret = false;
    w.selecting = false;
    w.footer_msg = None;
    w.confirm_delete = false;
    w.pressed = None;
    w.pending = None;
    w.hot_bar = None;
    w.hot_image = None;
    w.bar_press = None;
    w.image_press = None;
    w.img_sizes.clear();
    *w.img.borrow_mut() = ImgCache::default();
    w.dirty = true;
}

/// The window shows `note` (another sheet of its stack): its id, sticky part,
/// text and edit time are swapped in. The editing ended before.
fn load_sheet(w: &mut Win, note: &Note) {
    w.id = note.id.clone();
    w.sticky = note.sticky.clone().unwrap_or_default();
    w.text = note.text.clone();
    w.updated = note.updated;
    reset_sheet_view(w);
}

/// Window `i` shows sheet `to` of its stack (a flip): the sheet it showed
/// keeps its live data in the window's copy, the new one takes the window.
/// Editing ended before. The window is never destroyed or remade. The lean
/// follows the stack, so it does not change. Tells the owner.
fn show_sheet(s: &mut State, i: usize, to: usize) {
    let w = &mut s.wins[i];
    let Some(pos) = w.stack.as_ref().map(|st| st.pos) else {
        return;
    };
    if pos == to || to >= w.sheets.len() {
        return;
    }
    let live = Sticky {
        stack_top: false,
        ..w.sticky.clone()
    };
    let (text, updated) = (w.text.clone(), w.updated);
    if let Some(sheet) = w.sheets.get_mut(pos) {
        *sheet = Sheet {
            id: sheet.id.clone(),
            sticky: live.clone(),
            text,
            updated,
        };
    }
    let next = w.sheets[to].clone();
    // Place, size, pin, roll and tuck are the stack's, the rest the sheet's.
    let mut sticky = Sticky {
        stack_top: true,
        ..next.sticky
    };
    stack::share(&live, &mut sticky);
    w.id = next.id;
    w.sticky = sticky;
    w.text = next.text;
    w.updated = next.updated;
    if let Some(view) = &mut w.stack {
        view.pos = to;
    }
    reset_sheet_view(w);
    let id = w.id.clone();
    push(s, WidgetEvent::StackShow { id });
    settle_alpha(s, i);
}

/// Brings window `i` to `unit` as the notes are now.
fn update_win(s: &mut State, i: usize, unit: &stack::Unit, prefs_changed: bool, tilt: bool) {
    let note = unit.sheets[unit.top()];
    let sticky = note.sticky.clone().unwrap_or_default();
    let fired = unit
        .sheets
        .iter()
        .any(|n| n.sticky.as_ref().is_some_and(|st| st.remind_fired));
    let w = &mut s.wins[i];
    // The unit: a stack's sheets and the place of the one shown.
    let view = unit.is_stack().then(|| StackView {
        id: unit.key().to_owned(),
        ids: unit.ids(),
        pos: unit.top(),
    });
    if w.stack != view {
        w.stack = view;
        w.dirty = true;
    }
    w.sheets = if unit.is_stack() {
        unit.sheets
            .iter()
            .map(|n| Sheet {
                id: n.id.clone(),
                sticky: n.sticky.clone().unwrap_or_default(),
                text: n.text.clone(),
                updated: n.updated,
            })
            .collect()
    } else {
        Vec::new()
    };
    let was_tucked = w.sticky.tuck.is_some();
    // Another sheet shows now (a flip the owner applied, an overdue sheet
    // coming to the top, one taken out).
    if w.id != note.id {
        load_sheet(w, note);
    }
    let moving = w.drag.is_some();
    // While the note has the keyboard its formatting is live in the window (the
    // EDIT's text is the truth, and the runs follow it): the store has them
    // after the owner applied the batch, and the same again.
    let sticky = if w.edit.is_some() && w.id == note.id {
        Sticky { styles: w.sticky.styles.clone(), ..sticky }
    } else {
        sticky
    };
    if w.sticky.images != sticky.images {
        let mut cache = w.img.borrow_mut();
        *cache = ImgCache::default();
        w.img_sizes.clear();
        w.dirty = true;
    }
    if w.sticky != sticky && !moving {
        w.sticky = sticky;
        w.dirty = true;
    } else if moving {
        // The drag owns the place (and the tuck); the rest still follows.
        let rect = w.sticky.rect;
        let monitor = w.sticky.monitor.clone();
        let tuck = w.sticky.tuck;
        if w.sticky != sticky {
            w.sticky = Sticky {
                rect,
                monitor,
                tuck,
                ..sticky
            };
            w.dirty = true;
        }
    }
    // Tucked or brought out by the owner's data (a stack mate, a restart):
    // a note that just got tucked is out until the pointer leaves it.
    if w.sticky.tuck.is_some() != was_tucked {
        w.pop = tuck::Pop::at(1.0);
        w.pop_edge = w.sticky.tuck.or(w.pop_edge);
        w.grace = None;
        w.dirty = true;
    }
    // A raised note comes back down once its reminder is no longer
    // fired (snoozed, cleared, opened).
    if w.raised && !fired {
        w.raised = false;
        w.wiggle = None;
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
    if w.tilt != tilt {
        // The window's margin changes with the lean: a new window size.
        w.tilt = tilt;
        w.canvas = None;
        w.dirty = true;
    }
    if !moving {
        relayout(s, i);
    }
}

/// The note's rect on the desktop from its saved place and the displays now;
/// the window follows (position and size are the canvas's).
fn relayout(s: &mut State, i: usize) {
    let w = &mut s.wins[i];
    let Some(p) = place::place(&w.sticky, &s.displays) else {
        return;
    };
    if w.note != p.rect || w.view != p.view || w.scale != p.scale || w.display != p.display {
        w.note = p.rect;
        w.view = p.view;
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

/// The window's size (px) for a note shown as `view` at scale `k` with margin `m`.
fn canvas_px(view: &DipRect, rolled: bool, k: f32, m: (f32, f32, f32, f32)) -> (i32, i32) {
    let (mw, mh) = window_dip(view.w, shown_h(view.h, rolled), m);
    ((mw * k).round() as i32, (mh * k).round() as i32)
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
        images_dir,
        last_hl,
        ..
    } = s;
    let Some(res) = res.as_ref() else { return };
    let can_add_more = sticky::can_add(sheet_total(wins));
    let w = &mut wins[i];
    let k = w.scale;
    let now = Instant::now();
    // A tucked note keeps room for the overshoot of its pop (the window grows
    // while it is tucked or popping and gives the room back after).
    w.room = w.room_now(now);
    let m = w.margins();
    let size = canvas_px(&w.view, w.sticky.rolled, k, m);
    if w.canvas.as_ref().is_none_or(|c| c.size() != size) {
        w.canvas = Canvas::with_gfx(res.gfx.clone(), size).ok();
        // The bitmaps belong to the render target that made them.
        *w.img.borrow_mut() = ImgCache::default();
    }
    let origin = w.origin();
    // The text, laid out for the width it is drawn at, in the note's font,
    // below the images and with the note's formatting.
    let (nw, nh) = (w.view.w, w.view.h);
    let bar_on = w.bar_shown(now);
    let area = paint::body_rect_in(nw, nh, bar_on);
    refresh_images(w, images_dir);
    let key = (
        w.text.clone(),
        w.sticky.font,
        w.sticky.text_size,
        (area.w * 100.0) as i32,
        styles_hash(&w.sticky.styles),
        (w.img_top * 100.0) as i32,
    );
    if w.body_key.as_ref() != Some(&key) {
        w.body = Some(paint::build_body_with(
            res,
            &w.text,
            w.sticky.font,
            w.sticky.text_size,
            area.w,
            w.img_top,
            &w.sticky.styles,
        ));
        w.body_key = Some(key);
    }
    let img_keys = prepare_images(w, images_dir, k);
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
    let island = theme::island(theme::current().accent, theme::current().surface);
    let tones = sticky::tones(w.sticky.paper, &island, theme::current().accent);
    let today = Local::now().date_naive();
    let updated = DateTime::from_timestamp(w.updated, 0)
        .map(|t| t.with_timezone(&Local).naive_local())
        .unwrap_or_default();
    let message = match w.footer_msg {
        Some(key) => Some(text(*lang, key).to_owned()),
        None if w.confirm_delete => Some(text(*lang, Key::StickyDeleteAgain).to_owned()),
        None => None,
    };
    let footer = message
        .clone()
        .unwrap_or_else(|| sticky::footer_label(*lang, &w.text, updated, today));
    let bar = bar_on.then(|| BarScene {
        hot: w.hot_bar,
        on: tool_states(w),
        marker_hl: *last_hl,
        msg: message,
    });
    // The pop: the cat shows only on a note that is all the way out.
    let edge = w.tuck_edge(now);
    let popped = w.pop_now(now);
    let progress_now = w.pop.progress(now);
    w.pop_drawn = popped;
    let peek_on = prefs.cat_peek
        && peek.as_deref().is_some_and(|id| w.has(id))
        && (edge.is_none() || (progress_now >= 1.0 && !w.pop.running(now)));
    let cat_mood = win_mood(w, unix_now());
    if peek_on {
        w.mood = cat_mood;
    }
    let progress = {
        let (done, total) = checklist::counts(&w.text);
        (total > 0).then_some((done, total))
    };
    let (win_w, win_h) = window_dip(nw, shown_h(nh, w.sticky.rolled), m);
    let confetti = w.confetti.map_or_else(Vec::new, |(t, seed)| {
        art::confetti(seed, elapsed_ms(t, now), win_w, win_h)
    });
    let highlight = w
        .highlight
        .map(|t| {
            let p = (elapsed_ms(t, now) / HIGHLIGHT_MS).clamp(0.0, 1.0);
            (p * std::f32::consts::PI).sin()
        })
        .unwrap_or(0.0);
    // A note a dragged one would stack on keeps the frame lit.
    let highlight = if w.targeted { 1.0 } else { highlight };
    let title = checklist::title_of(&w.text);
    let unix = unix_now();
    let chip = w.sticky.remind_at.map(|at| {
        (
            remind::chip_label(*lang, remind::from_unix(at), today),
            remind::overdue(&w.sticky, unix),
        )
    });
    let dx = w
        .wiggle
        .map_or(0.0, |t| paint::wiggle_dx(elapsed_ms(t, now)));
    // A stack: "2 / 5", and the next two sheets' paper peeking out underneath.
    let stack = w.stack.as_ref().map(|st| {
        let len = st.ids.len();
        let accent = theme::current().accent;
        let edges = (1..=2usize.min(len.saturating_sub(1)))
            .filter_map(|k| w.sheets.get((st.pos + k) % len))
            .map(|sh| sticky::tones(sh.sticky.paper, &island, accent).fill)
            .collect();
        paint::StackScene {
            label: stack::counter_label(st.pos, len),
            edges,
        }
    });
    let tuck_scene = edge.map(|edge| paint::TuckScene {
        edge,
        progress: progress_now,
        scale: popped.scale,
        note_alpha: popped.note,
        tab_alpha: popped.tab,
        overdue: w.any_overdue(unix),
    });
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
        text_size: w.sticky.text_size,
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
        margins: m,
        font: w.sticky.font,
        pattern: w.sticky.pattern,
        deco: w.sticky.deco,
        stamp: w.sticky.stamp,
        lang: *lang,
        tilt: w.lean(),
        progress,
        confetti: &confetti,
        cat_pose: CatPose {
            pose: if cat_mood == Mood::Looking {
                mood::look_pose(w.look)
            } else {
                Default::default()
            },
            clock: cat_mood == Mood::Clock,
            zzz: cat_mood == Mood::Asleep,
        },
        dx,
        chip,
        highlight,
        drop_hover: w.drop_hover,
        peek: peek_on.then(|| mood::expr(cat_mood)),
        cat: paint::peek_look(*coat),
        frame_accent: tones.accent,
        stack,
        tuck: tuck_scene,
        cue: w.cue,
        bar,
        styles: &w.sticky.styles,
        dark_paper: w.sticky.paper == Paper::Gelap,
        images: (!w.sticky.rolled && !w.img_rects.is_empty()).then(|| ImgScene {
            rects: &w.img_rects,
            keys: &img_keys,
            hot: w.hot_image,
            cache: &w.img,
        }),
    };
    let alpha = alpha_u8(w, now);
    if let Some(canvas) = &w.canvas
        && canvas
            .paint_with(w.hwnd, origin, 0.0, alpha, |c| paint::draw(c, res, &scene))
            .is_ok()
    {
        w.alpha_shown = alpha;
    }
}

// ------------------------------------------------------------------ messages

/// In the Controls feature of `windows`; the value is fixed by Win32.
const WM_MOUSELEAVE: u32 = 0x02A3;
/// `WM_SYSCOMMAND`'s menu key (Alt released alone).
const SC_KEYMENU: u32 = 0xF100;

/// Private to the note windows: leave editing (posted from inside the EDIT's
/// own handlers, which must not destroy it themselves).
const WM_END_EDIT: u32 = WM_APP + 1;
use crate::widgets::image_io::WM_NOTE_IMPORT;

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
        // Alt moves a note: its release must not open a menu mode or beep.
        WM_SYSKEYDOWN | WM_SYSKEYUP if (wparam.0 & 0xFFFF) as u16 == VK_MENU.0 => {
            return LRESULT(0);
        }
        WM_SYSCOMMAND if (wparam.0 & 0xFFF0) as u32 == SC_KEYMENU => return LRESULT(0),
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
            // A right click on the header's (+) offers the templates; on the
            // disabled one (the note cap) it is the plain context menu.
            let on_plus = with(|s| {
                let i = idx_of(s, hwnd)?;
                Some(opens_templates(target_at(&s.wins[i], pt.x, pt.y), sheet_total(&s.wins)))
            })
            .flatten()
            .unwrap_or(false);
            // A right click on the Marker button opens its popover too.
            let on_marker = with(|s| {
                let i = idx_of(s, hwnd)?;
                Some(matches!(
                    target_at(&s.wins[i], pt.x, pt.y),
                    Target::Bar(BarHit::Tool(Tool::Marker) | BarHit::MarkerMenu)
                ))
            })
            .flatten()
            .unwrap_or(false);
            if on_marker {
                open_marker_pop(hwnd);
            } else if on_plus {
                plus_menu(hwnd, pt);
            } else {
                context_menu(hwnd, pt);
            }
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            let raw = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32;
            // The message carries the pointer in screen coordinates.
            mouse_wheel(hwnd, lo(lparam), hi(lparam), raw);
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
                let dialog =
                    with(|s| idx_of(s, hwnd).is_some_and(|i| s.wins[i].dialog)).unwrap_or(false);
                if !pop::is_popover_of(to, hwnd)
                    && !remind_pop::is_popover_of(to, hwnd)
                    && !fmt_pop::is_popover_of(to, hwnd)
                    && !dialog
                {
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
        WM_NOTE_IMPORT => {
            import_done(hwnd, lparam);
            return LRESULT(0);
        }
        WM_DESTROY => {
            // Every way a note window goes passes here: its drop target is
            // revoked with it, and its popup goes.
            drop::revoke(hwnd);
            if fmt_pop::is_open_for(hwnd) {
                fmt_pop::close();
            }
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_WIDGET_CARET);
            }
            // Results posted to this window that it will never pump: their
            // files go, and the pending count they held.
            let orphans = image_io::claim_window(hwnd);
            let _ = with(|s| {
                for done in &orphans {
                    if let Some(at) = s.in_flight.iter().position(|n| *n == done.note) {
                        s.in_flight.remove(at);
                    }
                }
                s.wins.retain(|w| w.hwnd != hwnd);
                refresh_peek(s);
            });
            for done in &orphans {
                image_io::discard(done);
            }
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
    // An Alt press moves the note: no editing starts.
    let alt = alt_down();
    let text = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        // A press on a tab moves the note; it takes no keyboard.
        Some(
            !w.on_tab(pt.x, pt.y, Instant::now())
                && matches!(target_at(w, pt.x, pt.y), Target::Body | Target::Footer),
        )
    })
    .flatten()
    .unwrap_or(false);
    LRESULT((if text && !alt { MA_ACTIVATE } else { MA_NOACTIVATE }) as isize)
}

/// Whether the header's buttons are visible enough to take a press: a hidden
/// button never eats one (the strip underneath drags the note).
fn buttons_shown(w: &Win, now: Instant) -> bool {
    w.header.value(now) >= 0.5
}

/// What is under a point of the desktop (px) on this note.
fn target_at(w: &Win, sx: i32, sy: i32) -> Target {
    target_at_in(w, sx, sy, Instant::now())
}

/// `target_at` at the moment `now` (the pop's scale depends on it).
fn target_at_in(w: &Win, sx: i32, sy: i32, now: Instant) -> Target {
    let (px, py) = (
        (sx - w.note.x) as f32 / w.scale,
        (sy - w.note.y) as f32 / w.scale,
    );
    // The tab counts as part of the note while it shows, all the way to the
    // end of the pop (the hit keeper holds the pointer there).
    if let Some(tab) = w.tab_dip(now)
        && px >= tab.x
        && px < tab.right()
        && py >= tab.y
        && py < tab.bottom()
    {
        return Target::Header;
    }
    let (px, py) = pop_unscale(w, (px, py), now);
    // Only the tab is drawn at progress 0: nothing else is the note.
    if w.tuck_edge(now).is_some() && w.pop.progress(now) <= 0.0 {
        return Target::Outside;
    }
    // A leaning note is hit through the inverse rotation; a tucked or popping
    // one through the inverse of its scale about the tab.
    let (x, y) = art::unrotate(
        px,
        py,
        w.view.w,
        shown_h(w.view.h, w.sticky.rolled),
        w.lean(),
    );
    paint::target_full(
        x,
        y,
        w.view.w,
        w.view.h,
        w.sticky.rolled,
        w.body.as_ref(),
        w.scroll,
        buttons_shown(w, now),
        w.stack.is_some(),
        &Extras {
            bar: w.bar_shown(now),
            images: &w.img_rects,
        },
    )
}

/// A note point as drawn (dip from the note's top-left) taken back through the
/// pop's scale about the tab: the point of the note it shows.
fn pop_unscale(w: &Win, pt: (f32, f32), now: Instant) -> (f32, f32) {
    match w.tuck_edge(now) {
        Some(edge) => {
            let h = shown_h(w.view.h, w.sticky.rolled);
            tuck::unscale(pt, tuck::origin(edge, w.view.w, h), w.pop.scale(now))
        }
        None => pt,
    }
}

/// Alt is held: a press anywhere on the note moves it.
fn alt_down() -> bool {
    (unsafe { GetKeyState(i32::from(VK_MENU.0)) }) < 0
}

/// A client point (px) as dip from the note's top-left.
fn client_to_dip(w: &Win, x: i32, y: i32) -> (f32, f32) {
    let m = w.margins();
    let (x, y) = pop_unscale(
        w,
        (x as f32 / w.scale - m.0, y as f32 / w.scale - m.1),
        Instant::now(),
    );
    art::unrotate(
        x,
        y,
        w.view.w,
        shown_h(w.view.h, w.sticky.rolled),
        w.lean(),
    )
}

fn set_cursor(hwnd: HWND) -> bool {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let id = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        if let Some(d) = &w.drag {
            return Some(if d.kind == DragKind::Move {
                IDC_SIZEALL
            } else {
                IDC_SIZENWSE
            });
        }
        if w.on_tab(pt.x, pt.y, Instant::now()) {
            return Some(if s.prefs.lock { IDC_ARROW } else { IDC_SIZEALL });
        }
        match target_at(w, pt.x, pt.y) {
            Target::Body => Some(IDC_IBEAM),
            Target::Button(_) | Target::Check(_) => Some(IDC_HAND),
            Target::Bar(BarHit::Bar) => Some(IDC_ARROW),
            Target::Bar(_) | Target::Image(_) | Target::ImageRemove(_) => Some(IDC_HAND),
            Target::Header if !s.prefs.lock => Some(IDC_SIZEALL),
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

/// A client point of note window `hwnd` (px) on the desktop. The messages
/// carry the point, so it holds whatever moved the window meanwhile.
fn screen_point(hwnd: HWND, x: i32, y: i32) -> POINT {
    with(|s| {
        let w = &s.wins[idx_of(s, hwnd)?];
        let (ox, oy) = w.origin();
        Some(POINT { x: ox + x, y: oy + y })
    })
    .flatten()
    .unwrap_or_else(|| {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        pt
    })
}

fn mouse_move(hwnd: HWND, x: i32, y: i32) {
    let pt = screen_point(hwnd, x, y);
    let alt = alt_down();
    let shift = shift_down();
    let follow = with(|s| {
        let i = idx_of(s, hwnd)?;
        let now = Instant::now();
        let animate = s.animate;
        let lang_cap = !sticky::can_add(sheet_total(&s.wins));
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
        // A press on a button that then moves 4 px or more is a drag of the
        // note from where the press was, not a click (Kunci posisi blocks it,
        // as it blocks every move).
        if s.wins[i].drag.is_none()
            && s.wins[i].pressed.is_some()
            && !s.prefs.lock
            && (pt.x - s.wins[i].press_at.0).abs() + (pt.y - s.wins[i].press_at.1).abs() >= 4
        {
            s.wins[i].pressed = None;
            start_move(s, i, s.wins[i].press_at);
        }
        if s.wins[i].drag.is_some() {
            drag_move(s, i, (pt.x, pt.y), alt, shift);
            return None;
        }
        let hot_target = target_at(&s.wins[i], pt.x, pt.y);
        let cat_here = s.prefs.cat_peek && s.peek.as_deref().is_some_and(|id| s.wins[i].has(id));
        let w = &mut s.wins[i];
        let hot = match hot_target {
            Target::Button(b) => Some(b),
            _ => None,
        };
        // The bar's hot button (its tooltip, at once, no timer) and the image
        // under the pointer redraw only when they change.
        let hot_bar = match hot_target {
            Target::Bar(h) if h != BarHit::Bar => Some(h),
            _ => None,
        };
        let hot_image = match hot_target {
            Target::Image(n) | Target::ImageRemove(n) => Some(n),
            _ => None,
        };
        let inside = !matches!(hot_target, Target::Outside);
        let mut changed = false;
        // The cat looks along the pointer while it is over this note; the
        // picture is redrawn only when the pointer enters or the step changes.
        let pointer_note_x = (pt.x - w.note.x) as f32 / w.scale;
        let look = mood::look_step(
            pointer_note_x,
            art::cat_left(w.view.w) + paint::CAT_SIZE / 2.0,
        );
        let look_moved = w.look != look;
        w.look = look;
        let entered = w.pointer_in != inside;
        w.pointer_in = inside;
        if cat_here {
            let now_mood = win_mood(w, unix_now());
            if now_mood != w.mood || (now_mood == Mood::Looking && look_moved) {
                changed = true;
            }
        }
        if w.hot != hot {
            w.hot = hot;
            changed = true;
        }
        if w.hot_bar != hot_bar {
            w.hot_bar = hot_bar;
            changed = true;
        }
        if w.hot_image != hot_image {
            w.hot_image = hot_image;
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
        if entered {
            settle_alpha(s, i);
        }
        // The pointer on a tucked note keeps it out (and brings it out).
        tuck_settle(s, i);
        if s.wins[i].dirty {
            changed = true;
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
        w.hot_bar = None;
        w.hot_image = None;
        w.footer_msg = None;
        w.pointer_in = false;
        w.look = 0;
        if w.drag.is_none() {
            w.header.toward(0.0, Instant::now(), animate);
            if animate {
                push(s, WidgetEvent::NeedsClock);
            }
        }
        s.wins[i].dirty = true;
        settle_alpha(s, i);
        // A tucked note that is out waits the grace, then pops back into its tab.
        tuck_settle(s, i);
        draw(s, i);
        Some(())
    });
}

enum Down {
    Nothing,
    Capture,
    Edit(usize),
}

/// A raised note (a reminder fired) goes back to its layer.
fn lower(hwnd: HWND) {
    let i = with(|s| {
        let i = idx_of(s, hwnd)?;
        if !s.wins[i].raised {
            return None;
        }
        s.wins[i].raised = false;
        s.wins[i].wiggle = None;
        s.wins[i].dirty = true;
        settle_alpha(s, i);
        tuck_settle(s, i);
        Some(i)
    })
    .flatten();
    if let Some(i) = i {
        apply_z(i);
    }
}

fn left_down(hwnd: HWND, x: i32, y: i32) {
    // Any press on a note is "touching" it: a reminder's raise ends.
    lower(hwnd);
    let pt = screen_point(hwnd, x, y);
    // A click on a note outside the popover closes it, also where the note
    // does not take the activation (the header, the grip, a box). The colour
    // button toggles it on its release instead.
    let on_colour = with(|s| {
        let i = idx_of(s, hwnd)?;
        Some(target_at(&s.wins[i], pt.x, pt.y) == Target::Button(Btn::Colour))
    })
    .flatten()
    .unwrap_or(false);
    if !on_colour && pop::is_open() {
        pop::close();
    }
    // A click elsewhere on the note closes the format popup; its own two
    // buttons toggle it on release.
    let on_fmt = with(|s| {
        let i = idx_of(s, hwnd)?;
        Some(matches!(
            target_at(&s.wins[i], pt.x, pt.y),
            Target::Bar(BarHit::MarkerMenu | BarHit::More)
        ))
    })
    .flatten()
    .unwrap_or(false);
    if !on_fmt && fmt_pop::is_open() {
        fmt_pop::close();
    }
    if remind_pop::is_open() {
        remind_pop::close();
    }
    let act = with(|s| {
        let i = idx_of(s, hwnd)?;
        let lock = s.prefs.lock;
        let now = Instant::now();
        let t = target_at_in(&s.wins[i], pt.x, pt.y, now);
        // Where the press lands on the note, through the pop's scale, before
        // the pop is finished.
        let dip = client_to_dip(&s.wins[i], x, y);
        // Any press on the note is the next other action: the footer follows.
        disarm_delete(s, i);
        // A press on the tab of a tucked note: it comes out at once and moves.
        if s.wins[i].on_tab(pt.x, pt.y, now) && t != Target::Outside {
            if lock {
                return Some(Down::Nothing);
            }
            pop_out_now(&mut s.wins[i]);
            start_move(s, i, (pt.x, pt.y));
            return Some(Down::Capture);
        }
        // A press elsewhere on a note that is part way: the pop is finished
        // at once and the press is handled as on a note that is out.
        if s.wins[i].tab_showing(now) && t != Target::Outside {
            pop_out_now(&mut s.wins[i]);
        }
        // Alt + press anywhere on the note moves it.
        if alt_down() && t != Target::Outside {
            if lock {
                return Some(Down::Nothing);
            }
            start_move(s, i, (pt.x, pt.y));
            return Some(Down::Capture);
        }
        match t {
            Target::Button(b) => {
                s.wins[i].pressed = Some(b);
                s.wins[i].press_at = (pt.x, pt.y);
                Some(Down::Capture)
            }
            Target::Check(line) => {
                toggle_line(s, i, line);
                Some(Down::Nothing)
            }
            // A bar press never ends editing, never moves the caret or the
            // selection and never starts a drag: the action fires on release.
            Target::Bar(BarHit::Bar) => Some(Down::Nothing),
            Target::Bar(hit) => {
                s.wins[i].bar_press = Some(hit);
                Some(Down::Capture)
            }
            Target::Image(n) | Target::ImageRemove(n) => {
                let remove = matches!(t, Target::ImageRemove(_));
                s.wins[i].image_press = Some((n, remove, (pt.x, pt.y)));
                Some(Down::Capture)
            }
            Target::Header | Target::Grip if lock => Some(Down::Nothing),
            Target::Header => {
                start_move(s, i, (pt.x, pt.y));
                Some(Down::Capture)
            }
            Target::Grip => {
                begin_drag(s, i, DragKind::Resize, (pt.x, pt.y));
                Some(Down::Capture)
            }
            Target::Body | Target::Footer => {
                let w = &s.wins[i];
                let (dx, dy) = dip;
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

/// A drag of note `i` that began with the pointer at `from` (desktop px).
fn begin_drag(s: &mut State, i: usize, kind: DragKind, from: (i32, i32)) {
    let w = &mut s.wins[i];
    // A dragged note is out at once, however it was pressed.
    if w.tuck_edge(Instant::now()).is_some() && (w.pop.to < 1.0 || w.pop.running(Instant::now())) {
        pop_out_now(w);
    }
    w.drag = Some(Drag {
        kind,
        cursor: from,
        note: w.note,
        dip: DipRect {
            h: shown_h(w.view.h, w.sticky.rolled),
            ..w.view
        },
        moved: false,
        lit: Vec::new(),
        last: from,
        cue: None,
        target: None,
    });
    touch(s, i);
    settle_alpha(s, i);
}

/// The note is moved from `from`.
fn start_move(s: &mut State, i: usize, from: (i32, i32)) {
    begin_drag(s, i, DragKind::Move, from);
}

fn left_up(hwnd: HWND, x: i32, y: i32) {
    let pt = screen_point(hwnd, x, y);
    let (press, bar, image) = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &mut s.wins[i];
        w.selecting = false;
        let pressed = w.pressed.take();
        let bar_pressed = w.bar_press.take();
        let image_pressed = w.image_press.take();
        if w.drag.is_some() {
            finish_drag(s, i, true);
            return None;
        }
        let at = target_at(&s.wins[i], pt.x, pt.y);
        let over = match at {
            Target::Button(b) => Some(b),
            _ => None,
        };
        // A bar button fires when released over the same one.
        let bar = bar_pressed.filter(|p| at == Target::Bar(*p));
        // A click on an image: released over the same one without moving 4 px.
        let image = image_pressed.and_then(|(n, remove, from)| {
            let still = (pt.x - from.0).abs() + (pt.y - from.1).abs() < 4;
            let same = if remove {
                at == Target::ImageRemove(n)
            } else {
                at == Target::Image(n)
            };
            (still && same).then_some((n, remove))
        });
        Some((pressed.filter(|p| Some(*p) == over), bar, image))
    })
    .flatten()
    .unwrap_or((None, None, None));
    unsafe {
        let _ = ReleaseCapture();
    }
    if let Some(b) = press {
        button(hwnd, b);
    }
    if let Some(hit) = bar {
        bar_action(hwnd, hit);
    }
    if let Some((n, remove)) = image {
        if remove {
            remove_image(hwnd, n);
        } else {
            open_image(hwnd, n);
        }
    }
}

fn double_click(hwnd: HWND, x: i32, y: i32) {
    let pt = screen_point(hwnd, x, y);
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
        left_down(hwnd, x, y);
    }
}

// ------------------------------------------------------------------ actions

fn toggle_line(s: &mut State, i: usize, line: usize) {
    let Some(e) = checklist::toggle(&s.wins[i].text, line) else {
        return;
    };
    let was_done = checklist::all_done(&s.wins[i].text);
    apply_edit(s, i, &e);
    // The tick that finishes the list: paper confetti for 0.8 s, unless
    // nothing animates.
    if !was_done && checklist::all_done(&s.wins[i].text) && s.animate {
        let seed = art_seed(&s.wins[i].id, unix_now());
        s.wins[i].confetti = Some((Instant::now(), seed));
        push(s, WidgetEvent::NeedsClock);
    }
    touch(s, i);
}

/// A seed for one burst of confetti: the note and the moment.
fn art_seed(id: &str, unix: i64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ unix as u64;
    for b in id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// `e` applied to note `i`'s text: through the EDIT while it has the keyboard
/// (so it can be undone), else straight to the text.
fn apply_edit(s: &mut State, i: usize, e: &Edit) {
    apply_edit_marks(s, i, e, None);
}

/// `apply_edit` for an edit that only changed list markers (`marks` are the
/// toggle's `(line start, old marker length, new marker length)`): the
/// formatting moves by those, not by the single region the edit replaces.
fn apply_edit_marks(s: &mut State, i: usize, e: &Edit, marks: Option<&[(usize, usize, usize)]>) {
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
    text_changed_marks(s, i, &before, marks);
}

/// A drag that a note takes is over (or has left) window `hwnd`: its dashed
/// outline shows. A tucked note pops out while it is over (`keep_out`), so
/// the drop lands in the note, never in a tab. Called from the drop target's callbacks, which reach the
/// state only through `with` (a busy state just skips the redraw).
pub(super) fn drop_hover(hwnd: HWND, on: bool) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        if s.wins[i].drop_hover != on {
            s.wins[i].drop_hover = on;
            s.wins[i].dirty = true;
            // A tucked note comes out under a drag like under a hover, and
            // goes back after the grace once the drag leaves or drops (the
            // same rule as every other thing that keeps it out; no timer).
            tuck_settle(s, i);
            draw(s, i);
        }
        Some(())
    });
}

/// `lines` dropped onto note window `hwnd`: they go at the end of its text.
/// While the note has the keyboard they go in through its EDIT (the selection
/// moved to the end, `EM_REPLACESEL`, so Ctrl+Z undoes them); otherwise the
/// text is set. Either way `text_changed` draws and announces it. A rolled
/// note stays rolled and a locked one takes drops (Kunci posisi locks the
/// place only). True when text was added.
pub(super) fn drop_into(hwnd: HWND, lines: &[String]) -> bool {
    with(|s| {
        let i = idx_of(s, hwnd)?;
        let before = s.wins[i].text.clone();
        let after = drop::append_lines(&before, lines);
        let insert = after.strip_prefix(before.as_str())?.to_owned();
        if insert.is_empty() {
            return None;
        }
        let end = before.encode_utf16().count();
        let caret = after.encode_utf16().count();
        // First, so the draw of the new text is the last one.
        touch(s, i);
        match s.wins[i].edit {
            Some(_) => {
                let e = Edit {
                    start: end,
                    end,
                    insert,
                    caret,
                };
                apply_edit(s, i, &e);
            }
            None => {
                let w = &mut s.wins[i];
                w.text = after;
                w.sel = (caret, caret);
                w.anchor = caret;
                text_changed(s, i, &before);
            }
        }
        Some(())
    })
    .flatten()
    .is_some()
}

/// The text of note `i` changed from `before`: tell the owner, redraw.
fn text_changed(s: &mut State, i: usize, before: &str) {
    text_changed_marks(s, i, before, None);
}

/// `text_changed`; with `marks` the formatting follows the marker changes of
/// a list toggle (`styles::rebase_marks`) instead of the prefix/suffix diff.
fn text_changed_marks(
    s: &mut State,
    i: usize,
    before: &str,
    marks: Option<&[(usize, usize, usize)]>,
) {
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
    // The formatting follows the edit: typed text takes the style at the
    // change (or the one chosen with a collapsed caret), deleted text takes its
    // runs with it. The owner is told the text first, then the runs, which win
    // over the store's own rebase of the same edit.
    let had = !w.sticky.styles.is_empty();
    w.sticky.styles = match marks {
        Some(spans) => styles::rebase_marks(before, &w.text, &w.sticky.styles, spans),
        None => styles::rebase_with(before, &w.text, &w.sticky.styles, w.pending),
    };
    let has = !w.sticky.styles.is_empty();
    let (id, t) = (w.id.clone(), w.text.clone());
    let runs = w.sticky.styles.clone();
    push(s, WidgetEvent::Text { id: id.clone(), text: t });
    if had || has {
        push(s, WidgetEvent::Styles { id, runs });
    }
    draw(s, i);
}

// ------------------------------------------------------------------ format

/// A hash of the formatting: the laid-out text is cached on it.
fn styles_hash(runs: &[Run]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    runs.hash(&mut h);
    h.finish()
}

/// The place of the images (and the block above the text) for the note's
/// size and the images' aspects, which are read from the files once.
fn refresh_images(w: &mut Win, dir: &Path) {
    let names = w.sticky.images.clone();
    for n in &names {
        if !w.img_sizes.contains_key(n) {
            w.img_sizes.insert(n.clone(), image_io::size_of(dir, n));
        }
    }
    w.img_sizes.retain(|k, _| names.contains(k));
    let aspects: Vec<f32> = names
        .iter()
        .map(|n| {
            w.img_sizes
                .get(n)
                .copied()
                .flatten()
                .map_or(4.0 / 3.0, |(a, b)| a as f32 / b.max(1) as f32)
        })
        .collect();
    let (rects, top) = images::grid(names.len(), w.view.w, w.view.h, &aspects);
    w.img_rects = rects;
    w.img_top = top;
}

/// The drawn size of each image in pixels with its name, and the pixels of
/// the ones the canvas has no bitmap for yet decoded (scaled to cover the
/// cell at this DPI, never enlarged). A file that fails is tried once.
fn prepare_images(w: &Win, dir: &Path, k: f32) -> Vec<paint::ImgKey> {
    let mut cache = w.img.borrow_mut();
    let mut keys = Vec::new();
    for (name, r) in w.sticky.images.iter().zip(&w.img_rects) {
        let key: paint::ImgKey = (name.clone(), (r.w * k).ceil() as u32, (r.h * k).ceil() as u32);
        if !cache.bitmaps.contains_key(&key)
            && !cache.fresh.contains_key(&key)
            && !cache.failed.contains(&key)
        {
            match image_io::decode_cover(dir, name, key.1, key.2) {
                Some(px) => {
                    cache.fresh.insert(key.clone(), px);
                }
                None => {
                    cache.failed.insert(key.clone());
                }
            }
        }
        keys.push(key);
    }
    keys
}

fn ordered((a, b): (usize, usize)) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// Which tools are lit at the caret or over the selection, in `Tool::ALL`
/// order: the attributes of the whole selection, or of the style the next
/// typed letter takes; the list kinds of every line it touches.
fn tool_states(w: &Win) -> [bool; 8] {
    let (a, b) = ordered(w.sel);
    let runs = &w.sticky.styles;
    let caret_style = || w.pending.unwrap_or_else(|| styles::style_at(runs, a));
    let attr = |at: Attr| {
        if a != b {
            styles::all_have(runs, a, b, at)
        } else {
            caret_style().has(at)
        }
    };
    let lines = checklist::lines(&w.text);
    // The same lines the list buttons act on (a selection that ends at the
    // start of a line does not touch it).
    let (first, last) = checklist::touched(&lines, a, b);
    let touched = &lines[first..=last.max(first)];
    let hl = if a != b {
        (a..b).all(|i| styles::style_at(runs, i + 1).hl.is_some())
    } else {
        caret_style().hl.is_some()
    };
    [
        attr(Attr::Bold),
        attr(Attr::Italic),
        attr(Attr::Underline),
        attr(Attr::Strike),
        touched.iter().all(|l| l.is_bullet()),
        touched.iter().all(|l| l.is_item()),
        hl,
        false,
    ]
}

/// A bar button was released (or its shortcut pressed).
fn bar_action(hwnd: HWND, hit: BarHit) {
    match hit {
        BarHit::Tool(t) => format_tool(hwnd, t),
        BarHit::MarkerMenu => open_marker_pop(hwnd),
        BarHit::More => open_overflow(hwnd),
        BarHit::Bar => {}
    }
}

/// What a tool does to the note that has the keyboard (the formatting
/// shortcuts, `EditEvent::Format`, call it too).
pub(crate) fn format_tool(hwnd: HWND, tool: Tool) {
    // A choice made in a popover ends it.
    crate::widgets::fmt_pop::close();
    match tool {
        Tool::Bold => toggle_attr(hwnd, Attr::Bold),
        Tool::Italic => toggle_attr(hwnd, Attr::Italic),
        Tool::Underline => toggle_attr(hwnd, Attr::Underline),
        Tool::Strike => toggle_attr(hwnd, Attr::Strike),
        Tool::Bullet => toggle_list_tool(hwnd, checklist::ListKind::Bullet),
        Tool::Check => toggle_list_tool(hwnd, checklist::ListKind::Check),
        Tool::Marker => apply_marker(hwnd, MarkerChoice::Last),
        Tool::Image => pick_images(hwnd),
    }
}

/// Pushes the window's formatting for note `i` and draws.
fn styles_changed(s: &mut State, i: usize) {
    let w = &mut s.wins[i];
    w.body_key = None;
    w.dirty = true;
    let (id, runs) = (w.id.clone(), w.sticky.styles.clone());
    push(s, WidgetEvent::Styles { id, runs });
    touch(s, i);
    draw(s, i);
}

/// B, I, U or strike: over a selection it toggles the whole range (all of it
/// has it: off; else on); with a collapsed caret it is the style of the next
/// typed text (`Win::pending`). Not part of the EDIT's undo.
fn toggle_attr(hwnd: HWND, attr: Attr) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &mut s.wins[i];
        w.edit?;
        let (a, b) = ordered(w.sel);
        if a != b {
            w.sticky.styles = styles::toggle(&w.sticky.styles, &w.text, a, b, attr);
            styles_changed(s, i);
        } else {
            let cur = w.pending.unwrap_or_else(|| styles::style_at(&w.sticky.styles, a));
            w.pending = Some(cur.with(attr, !cur.has(attr)));
            w.dirty = true;
            draw(s, i);
        }
        Some(())
    });
}

/// What the Marker button, Ctrl+Shift+H and the popover choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkerChoice {
    /// The last used highlight, toggled.
    Last,
    /// A highlight (remembered as the last), or none.
    Hl(Option<Hl>),
    /// A text colour, or automatic.
    Fg(Option<styles::Fg>),
}

fn apply_marker(hwnd: HWND, choice: MarkerChoice) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        let last = s.last_hl;
        let w = &mut s.wins[i];
        w.edit?;
        let (a, b) = ordered(w.sel);
        let mut remember = None;
        let apply: Box<dyn Fn(Style) -> Style> = match choice {
            MarkerChoice::Last => {
                let on = if a != b {
                    styles::all_hl(&w.sticky.styles, a, b, last)
                } else {
                    w.pending
                        .unwrap_or_else(|| styles::style_at(&w.sticky.styles, a))
                        .hl
                        == Some(last)
                };
                Box::new(move |st| Style { hl: (!on).then_some(last), ..st })
            }
            MarkerChoice::Hl(hl) => {
                remember = hl;
                Box::new(move |st| Style { hl, ..st })
            }
            MarkerChoice::Fg(fg) => Box::new(move |st| Style { fg, ..st }),
        };
        if a != b {
            w.sticky.styles = match (choice, apply(Style::default())) {
                (MarkerChoice::Fg(fg), _) => styles::set_fg(&w.sticky.styles, &w.text, a, b, fg),
                (_, st) => styles::set_hl(&w.sticky.styles, &w.text, a, b, st.hl),
            };
            if let Some(h) = remember {
                s.last_hl = h;
            }
            styles_changed(s, i);
        } else {
            let cur = w.pending.unwrap_or_else(|| styles::style_at(&w.sticky.styles, a));
            w.pending = Some(apply(cur));
            if let Some(h) = remember {
                s.last_hl = h;
            }
            s.wins[i].dirty = true;
            draw(s, i);
        }
        Some(())
    });
}

/// The bullet and checklist buttons, Ctrl+Shift+8 and 9, and the header's
/// checklist button: every line the selection touches (a collapsed caret, its
/// line; a note without the keyboard, the last line).
fn toggle_list_in(s: &mut State, i: usize, kind: checklist::ListKind) {
    let w = &s.wins[i];
    let t = w.text.clone();
    let (a, b) = if w.edit.is_some() {
        w.sel
    } else {
        let end = t.encode_utf16().count();
        (end, end)
    };
    let plan = checklist::toggle_list_plan(&t, a, b, kind);
    let (na, nb) = plan.sel;
    apply_edit_marks(s, i, &plan.edit, Some(&plan.spans));
    if a != b
        && let Some(edit_hwnd) = s.wins[i].edit
    {
        let now_text = s.wins[i].text.clone();
        edit::set_selection(edit_hwnd, &now_text, na, nb);
        s.wins[i].sel = (na, nb);
        s.wins[i].anchor = na;
        s.wins[i].dirty = true;
        draw(s, i);
    }
}

fn toggle_list_tool(hwnd: HWND, kind: checklist::ListKind) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        s.wins[i].edit?;
        toggle_list_in(s, i, kind);
        touch(s, i);
        Some(())
    });
}

// ------------------------------------------------------------------ popups

/// The bar button's rect on the desktop (px) and the work area it must stay
/// in, for a popup of note `hwnd`: the button is the marker's, the More
/// button's, or (when the bar is not shown) none.
fn bar_anchor(hwnd: HWND, tool: Option<Tool>) -> Option<(Rect, Rect, f32, UiFont)> {
    with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        if !w.bar_shown(Instant::now()) {
            return None;
        }
        let layout = toolbar::layout(w.view.w);
        let r = match tool {
            Some(t) => toolbar::rect_of(&layout, t).or(layout.more)?,
            None => layout.more?,
        };
        let top = w.view.h - toolbar::BAR_BLOCK;
        let anchor = Rect::new(
            w.note.x + (r.x * w.scale).round() as i32,
            w.note.y + ((top + r.y) * w.scale).round() as i32,
            (r.w * w.scale).round() as i32,
            (r.h * w.scale).round() as i32,
        );
        let area = s.displays.get(w.display).map_or(w.note, place::area_px);
        Some((anchor, area, w.scale, s.font))
    })
    .flatten()
}

/// The highlighter popover, above the Marker button (above More when the
/// marker moved into the overflow menu). Toggles: a second press closes it.
fn open_marker_pop(hwnd: HWND) {
    if fmt_pop::is_open_for(hwnd) {
        fmt_pop::close();
        return;
    }
    let Some((anchor, area, scale, ui)) = bar_anchor(hwnd, Some(Tool::Marker)) else {
        return;
    };
    let state = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        let (a, b) = ordered(w.sel);
        let st = w.pending.unwrap_or_else(|| {
            if a != b {
                styles::style_at(&w.sticky.styles, a + 1)
            } else {
                styles::style_at(&w.sticky.styles, a)
            }
        });
        Some((
            fmt_pop::MarkerState {
                hl: st.hl,
                fg: st.fg,
                dark_paper: w.sticky.paper == Paper::Gelap,
            },
            s.lang,
        ))
    })
    .flatten();
    if let Some((state, lang)) = state {
        fmt_pop::open(hwnd, anchor, area, scale, fmt_pop::Mode::Marker(state), lang, ui);
    }
}

/// The overflow menu: the tools that did not fit the bar of a narrow note.
fn open_overflow(hwnd: HWND) {
    if fmt_pop::is_open_for(hwnd) {
        fmt_pop::close();
        return;
    }
    let Some((anchor, area, scale, ui)) = bar_anchor(hwnd, None) else {
        return;
    };
    let info = with(|s| {
        let i = idx_of(s, hwnd)?;
        Some((toolbar::layout(s.wins[i].view.w).hidden, s.lang))
    })
    .flatten();
    if let Some((tools, lang)) = info.filter(|(t, _)| !t.is_empty()) {
        fmt_pop::open(hwnd, anchor, area, scale, fmt_pop::Mode::Menu(tools), lang, ui);
    }
}

/// A choice in the popup (called by its window): it closes, then acts. From
/// the overflow menu the Marker row opens the popover (above the same button).
pub(crate) fn fmt_choice(note: HWND, item: fmt_pop::Item) {
    fmt_pop::close();
    match item {
        fmt_pop::Item::Hl(h) => apply_marker(note, MarkerChoice::Hl(h)),
        fmt_pop::Item::Fg(f) => apply_marker(note, MarkerChoice::Fg(f)),
        fmt_pop::Item::Tool(Tool::Marker) => open_marker_pop(note),
        fmt_pop::Item::Tool(t) => format_tool(note, t),
    }
}

/// Where the owner says the images live (`NotesStore::images_dir`).
fn images_dir_of(s: &State) -> Option<PathBuf> {
    (!s.images_dir.as_os_str().is_empty()).then(|| s.images_dir.clone())
}

/// The bar's Image tool: the system's file dialog (several files), owned by
/// the note. The EDIT keeps the editing while it is up (`Win::dialog`), no
/// borrow is held across it, and the EDIT gets the focus back after.
fn pick_images(hwnd: HWND) {
    let info = with(|s| {
        let i = idx_of(s, hwnd)?;
        s.wins[i].edit?;
        s.wins[i].dialog = true;
        Some(s.lang)
    })
    .flatten();
    let Some(lang) = info else { return };
    let files = image_io::pick_files(
        hwnd,
        text(lang, Key::FmtPickImages),
        text(lang, Key::FmtImagesFilter),
    );
    let edit_hwnd = with(|s| {
        let i = idx_of(s, hwnd)?;
        s.wins[i].dialog = false;
        s.wins[i].edit
    })
    .flatten();
    if let Some(e) = edit_hwnd {
        unsafe {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(e));
        }
    }
    import_files(hwnd, files);
}

/// Image files (from the dialog, a drop or the clipboard's file list) go to
/// the note: as many as fit under the cap (counting the imports in flight)
/// start on the worker thread; the rest are refused with a message.
pub(super) fn import_files(hwnd: HWND, files: Vec<PathBuf>) {
    import_sources(hwnd, files.into_iter().map(image_io::Source::Path).collect());
}

/// `import_files` for any source (a pasted bitmap is one too).
pub(super) fn import_sources(hwnd: HWND, sources: Vec<image_io::Source>) {
    if sources.is_empty() {
        return;
    }
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        let id = s.wins[i].id.clone();
        let Some(dir) = images_dir_of(s) else {
            s.wins[i].footer_msg = Some(Key::FmtImgBad);
            s.wins[i].dirty = true;
            draw(s, i);
            return None;
        };
        let current = s.wins[i].sticky.images.len();
        let pending = s.in_flight.iter().filter(|n| **n == id).count();
        let (take, refused) = images::admit(current, pending, sources.len());
        for src in sources.into_iter().take(take) {
            if image_io::spawn_import(hwnd, id.clone(), src, dir.clone()) {
                s.in_flight.push(id.clone());
            } else {
                s.wins[i].footer_msg = Some(Key::FmtImgBad);
            }
        }
        if refused > 0 {
            s.wins[i].footer_msg = Some(Key::FmtImgMax);
        }
        s.wins[i].dirty = true;
        draw(s, i);
        Some(())
    });
}

/// `WM_NOTE_IMPORT`: an import finished. The file is added to the sheet that
/// has that id (the shown one, or a copy under a stack's top sheet); over the
/// cap, or with no window that has the id any more, it is deleted again.
fn import_done(hwnd: HWND, lparam: LPARAM) {
    // The message carries a token, never a pointer: a result is claimed from
    // the registry once, and a token it does not know (a stray post by another
    // process) is ignored.
    let Some(done) = image_io::claim(lparam.0 as u64) else {
        return;
    };
    let kept = with(|s| {
        if let Some(at) = s.in_flight.iter().position(|n| *n == done.note) {
            s.in_flight.remove(at);
        }
        // This window, else any window that shows the note.
        let i = idx_of(s, hwnd)
            .filter(|i| s.wins[*i].has(&done.note))
            .or_else(|| s.wins.iter().position(|w| w.has(&done.note)))?;
        let name = match &done.result {
            Ok(name) => name.clone(),
            Err(e) => {
                s.wins[i].footer_msg = Some(match e {
                    image_io::ImportError::TooBig => Key::FmtImgTooBig,
                    _ => Key::FmtImgBad,
                });
                s.wins[i].dirty = true;
                draw(s, i);
                return Some(true);
            }
        };
        let w = &mut s.wins[i];
        let list: &mut Vec<String> = if w.id == done.note {
            &mut w.sticky.images
        } else {
            &mut w.sheets.iter_mut().find(|sh| sh.id == done.note)?.sticky.images
        };
        if list.len() >= images::MAX_IMAGES || !images::valid_name(&name) {
            w.footer_msg = Some(Key::FmtImgMax);
            w.dirty = true;
            draw(s, i);
            return None;
        }
        list.push(name);
        let images = list.clone();
        let id = done.note.clone();
        *w.img.borrow_mut() = ImgCache::default();
        w.img_sizes.clear();
        w.body_key = None;
        w.dirty = true;
        push(s, WidgetEvent::Images { id, images });
        touch(s, i);
        draw(s, i);
        Some(true)
    })
    .flatten()
    .unwrap_or(false);
    if !kept {
        // Nobody took it: the copy goes.
        image_io::discard(&done);
    }
}

/// Image files dropped on note window `hwnd` (the drop target calls this).
pub(super) fn drop_images(hwnd: HWND, files: Vec<PathBuf>) {
    import_files(hwnd, files);
}

// ------------------------------------------------------------------ clipboard

/// `a..b` of `text` (UTF-16 units).
fn slice_units(text: &str, a: usize, b: usize) -> String {
    let u: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&u[a.min(u.len())..b.min(u.len())])
}

/// What a copy takes: the selection `a..b`, its text and its runs relative to
/// it; `None` for a collapsed caret.
fn copy_payload(w: &Win) -> Option<(usize, usize, String, Vec<Run>)> {
    let (a, b) = ordered(w.sel);
    (a != b).then(|| {
        (
            a,
            b,
            slice_units(&w.text, a, b),
            styles::slice(&w.sticky.styles, a, b),
        )
    })
}

/// Ctrl+C and Ctrl+X (and Ctrl+Insert, Shift+Delete, WM_COPY, WM_CUT): the
/// selection as plain text (`\r\n` breaks, a bullet and a box as their
/// marker text) plus the note's own `Vale.NoteRich` runs. A cut then deletes
/// the selection through the EDIT (undo works). Never logged.
fn copy_selection(parent: HWND, edit_hwnd: HWND, cut: bool) {
    let picked = with(|s| {
        let i = idx_of(s, parent)?;
        let w = &s.wins[i];
        if w.edit != Some(edit_hwnd) {
            return None;
        }
        copy_payload(w)
    })
    .flatten();
    let Some((a, b, part, runs)) = picked else { return };
    let rich = richclip::encode(&part, &runs);
    let wrote = clip_io::write_note(parent, &checklist::to_edit_text(&part), &rich);
    if cut && wrote {
        let _ = with(|s| {
            let i = idx_of(s, parent)?;
            let e = Edit { start: a, end: b, insert: String::new(), caret: a };
            apply_edit(s, i, &e);
            touch(s, i);
            Some(())
        });
    }
}

/// Ctrl+V, Shift+Insert and WM_PASTE. Text wins (Excel and Word put a bitmap
/// beside their text); else image files; else a bitmap.
fn paste_into(parent: HWND, edit_hwnd: HWND) {
    let current = with(|s| {
        let i = idx_of(s, parent)?;
        Some(s.wins[i].edit == Some(edit_hwnd))
    })
    .flatten()
    .unwrap_or(false);
    if !current {
        return;
    }
    match clip_io::read_paste(parent) {
        clip_io::Paste::Text { text, rich } => {
            let _ = with(|s| {
                let i = idx_of(s, parent)?;
                paste_text(s, i, &text, rich.as_deref());
                touch(s, i);
                Some(())
            });
        }
        clip_io::Paste::Files(files) => {
            let (imgs, _) = images::split_files(
                &files.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            );
            let wanted: Vec<PathBuf> = files
                .into_iter()
                .filter(|p| imgs.iter().any(|i| *i == p.to_string_lossy()))
                .collect();
            import_files(parent, wanted);
        }
        clip_io::Paste::Image(src) => import_sources(parent, vec![src]),
        clip_io::Paste::TooBig => {
            let _ = with(|s| {
                let i = idx_of(s, parent)?;
                s.wins[i].footer_msg = Some(Key::FmtImgTooBig);
                s.wins[i].dirty = true;
                draw(s, i);
                Some(())
            });
        }
        clip_io::Paste::Nothing => {}
    }
}

/// `text` (the clipboard's, any breaks) in place of the selection, through the
/// EDIT (undo works). The pasted text takes the style at the caret (a web or
/// Word paste never brings fonts or colours); a `Vale.NoteRich` payload of
/// the same text then overlays its own runs.
fn paste_text(s: &mut State, i: usize, clip: &str, rich: Option<&[u8]>) {
    let model = checklist::normalize(clip);
    if model.is_empty() {
        return;
    }
    let w = &s.wins[i];
    let (a, b) = ordered(w.sel);
    let before_len = w.text.encode_utf16().count();
    let len = model.encode_utf16().count();
    let e = Edit { start: a, end: b, insert: model.clone(), caret: a + len };
    apply_edit(s, i, &e);
    let w = &mut s.wins[i];
    // The EDIT may have cut the text short (its limit): overlay only when the
    // whole of it went in.
    let grew = w.text.encode_utf16().count() + (b - a) == before_len + len;
    if grew
        && let Some(runs) = rich.and_then(|bytes| richclip::decode(bytes, clip))
    {
        w.sticky.styles = styles::insert_runs(&w.sticky.styles, a, &runs, len);
        styles_changed(s, i);
    }
}

// ------------------------------------------------------------------ images

/// The ✕ of image `n`: it leaves the note (its file goes at the next save,
/// when nothing references it).
fn remove_image(hwnd: HWND, n: usize) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &mut s.wins[i];
        if n >= w.sticky.images.len() {
            return None;
        }
        w.sticky.images.remove(n);
        w.hot_image = None;
        *w.img.borrow_mut() = ImgCache::default();
        w.img_sizes.clear();
        w.body_key = None;
        w.dirty = true;
        let (id, list) = (w.id.clone(), w.sticky.images.clone());
        push(s, WidgetEvent::Images { id, images: list });
        touch(s, i);
        draw(s, i);
        Some(())
    });
}

/// A click on image `n` opens its file in the system's viewer.
fn open_image(hwnd: HWND, n: usize) {
    let path = with(|s| {
        let i = idx_of(s, hwnd)?;
        let name = s.wins[i].sticky.images.get(n)?.clone();
        image_io::path_of(&s.images_dir, &name)
    })
    .flatten();
    if let Some(path) = path {
        image_io::shell_open(&path);
    }
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
                if sticky::can_add(sheet_total(&s.wins)) {
                    let id = s.wins[i].id.clone();
                    push(
                        s,
                        WidgetEvent::NewBeside {
                            id,
                            template: Template::Kosong,
                        },
                    );
                }
                Some(())
            });
        }
        Btn::Check => {
            let _ = with(|s| {
                let i = idx_of(s, hwnd)?;
                toggle_list_in(s, i, checklist::ListKind::Check);
                touch(s, i);
                Some(())
            });
        }
        Btn::Colour => {
            if pop::is_open_for(hwnd) {
                pop::close();
            } else {
                open_popover(hwnd);
            }
        }
        Btn::More => {
            let at = with(|s| {
                let i = idx_of(s, hwnd)?;
                let w = &s.wins[i];
                let r = paint::button_rect(Btn::More, w.view.w);
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
        Btn::Prev => flip_by(hwnd, -1),
        Btn::Next => flip_by(hwnd, 1),
        Btn::List => {
            let _ = with(|s| {
                let i = idx_of(s, hwnd)?;
                let id = s.wins[i].id.clone();
                push(s, WidgetEvent::OpenList { id });
                Some(())
            });
        }
    }
}

/// The wheel at desktop point (`sx`, `sy`): over a stack's strip it flips one
/// sheet per notch (down is next, up is previous, wrapping), over the text it
/// scrolls the text.
fn mouse_wheel(hwnd: HWND, sx: i32, sy: i32, raw: i32) {
    let flips = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &mut s.wins[i];
        // A tucked stack shows only its tab: the wheel there flips nothing.
        if w.tab_showing(Instant::now()) {
            w.wheel = 0;
            return Some(0);
        }
        let on_strip = w.stack.is_some()
            && matches!(target_at(w, sx, sy), Target::Header | Target::Button(_));
        if on_strip {
            let (flips, rest) = stack::wheel_flips(w.wheel, raw);
            w.wheel = rest;
            return Some(flips);
        }
        w.wheel = 0;
        let line = w.sticky.text_size.line();
        w.scroll -= raw as f32 / 120.0 * 3.0 * line;
        w.dirty = true;
        draw(s, i);
        Some(0)
    })
    .flatten()
    .unwrap_or(0);
    if flips != 0 {
        flip_by(hwnd, flips);
    }
}

/// The stack of `hwnd` shows the sheet `delta` places on (wrapping). The
/// editing ends first; the window is kept and retargeted.
fn flip_by(hwnd: HWND, delta: i32) {
    // A popover of this note describes the sheet it is leaving.
    if pop::is_open_for(hwnd) {
        pop::close();
    }
    if remind_pop::is_open_for(hwnd) {
        remind_pop::close();
    }
    end_editing(hwnd);
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        let st = s.wins[i].stack.as_ref()?;
        let to = stack::flip_pos(st.pos, st.ids.len(), delta);
        show_sheet(s, i, to);
        Some(())
    });
    flush_dirty();
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

/// The commands of the template menu.
const CMD_TEMPLATE: usize = 100;

/// The six templates as a native menu at `at` (desktop px), owned by
/// `owner`: the one chosen, or `None` when it was dismissed. Shared by the
/// note header's (+) and Settings' gallery card.
pub fn template_menu(owner: HWND, at: POINT, lang: Lang) -> Option<Template> {
    let chosen = unsafe {
        let menu = CreatePopupMenu().ok()?;
        for (n, t) in Template::ALL.iter().enumerate() {
            let _ = AppendMenuW(
                menu,
                MF_STRING,
                CMD_TEMPLATE + n,
                &windows::core::HSTRING::from(text(lang, t.label_key())),
            );
        }
        // Without the foreground the menu would not close on a click elsewhere.
        let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(owner);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            at.x,
            at.y,
            None,
            owner,
            None,
        );
        let _ = PostMessageW(
            Some(owner),
            windows::Win32::UI::WindowsAndMessaging::WM_NULL,
            WPARAM(0),
            LPARAM(0),
        );
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    };
    (CMD_TEMPLATE..CMD_TEMPLATE + Template::ALL.len())
        .contains(&chosen)
        .then(|| Template::ALL[chosen - CMD_TEMPLATE])
}

/// A right click at `target` offers the templates: only the enabled (+). At
/// the note cap the (+) is drawn disabled and still hits as itself, and that
/// right click is the plain context menu like the rest of the header.
fn opens_templates(target: Target, notes: usize) -> bool {
    target == Target::Button(Btn::Plus) && sticky::can_add(notes)
}

/// A right click on a note's enabled (+) (`opens_templates`): the templates,
/// and the new note beside it.
fn plus_menu(hwnd: HWND, at: POINT) {
    let info = with(|s| {
        let i = idx_of(s, hwnd)?;
        Some((s.wins[i].id.clone(), s.lang))
    })
    .flatten();
    let Some((id, lang)) = info else { return };
    if let Some(template) = template_menu(hwnd, at, lang) {
        let _ = with(|s| push(s, WidgetEvent::NewBeside { id, template }));
    }
}

/// Note `hwnd`'s reminder set to `at` (unix seconds), or cleared. The note
/// is not fired then: a raise ends, the chip is the plain one.
fn set_remind(hwnd: HWND, at: Option<i64>) {
    let i = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &mut s.wins[i];
        w.sticky.remind_at = at;
        w.sticky.remind_fired = false;
        w.raised = false;
        w.wiggle = None;
        w.dirty = true;
        let id = w.id.clone();
        push(s, WidgetEvent::Remind { id, at });
        touch(s, i);
        settle_alpha(s, i);
        refresh_peek(s);
        Some(i)
    })
    .flatten();
    if let Some(i) = i {
        apply_z(i);
        flush_dirty();
    }
}

/// The entry ("Atur sendiri...") chose a time.
pub(crate) fn remind_chosen(note: HWND, at: i64) {
    set_remind(note, Some(at));
}

/// Opens the day and time entry under the colour button.
fn open_remind_entry(hwnd: HWND) {
    let info = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        let r = paint::button_rect(Btn::Colour, w.view.w);
        let anchor = Rect::new(
            w.note.x + (r.x * w.scale).round() as i32,
            w.note.y + (r.y * w.scale).round() as i32,
            (r.w * w.scale).round() as i32,
            (r.h * w.scale).round() as i32,
        );
        let area = s.displays.get(w.display).map_or(w.note, place::area_px);
        Some((anchor, area, w.scale, s.lang, s.font))
    })
    .flatten();
    if let Some((anchor, area, scale, lang, font)) = info {
        remind_pop::open(hwnd, anchor, area, scale, lang, font);
    }
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

/// A change to note `hwnd`'s sticky part, and the event that tells the owner.
/// `new_body` when the text must be laid out again (a font).
fn set_field(
    hwnd: HWND,
    change: impl FnOnce(&mut Sticky),
    event: impl FnOnce(String) -> WidgetEvent,
    new_body: bool,
) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        change(&mut s.wins[i].sticky);
        if new_body {
            s.wins[i].body_key = None;
        }
        s.wins[i].dirty = true;
        let id = s.wins[i].id.clone();
        push(s, event(id));
        touch(s, i);
        settle_alpha(s, i);
        Some(())
    });
    flush_dirty();
}

fn set_font(hwnd: HWND, font: NoteFont) {
    set_field(hwnd, |s| s.font = font, |id| WidgetEvent::Font { id, font }, true);
}

fn set_pattern(hwnd: HWND, pattern: Pattern) {
    set_field(hwnd, |s| s.pattern = pattern, |id| WidgetEvent::Pattern { id, pattern }, false);
}

fn set_deco(hwnd: HWND, deco: Deco) {
    set_field(hwnd, |s| s.deco = deco, |id| WidgetEvent::Deco { id, deco }, false);
}

fn set_stamp(hwnd: HWND, stamp: Stamp) {
    set_field(hwnd, |s| s.stamp = stamp, |id| WidgetEvent::Stamp { id, stamp }, false);
}

fn set_opacity(hwnd: HWND, percent: u8) {
    set_field(
        hwnd,
        |s| s.opacity = percent,
        |id| WidgetEvent::Opacity { id, percent },
        false,
    );
}

/// A command of the colour popover, for the note it belongs to.
pub(crate) fn pop_command(note: HWND, cmd: PopCmd) {
    match cmd {
        PopCmd::Paper(p) => set_paper(note, p),
        PopCmd::Size(sz) => set_size(note, sz),
        PopCmd::Font(f) => set_font(note, f),
        PopCmd::Pattern(p) => set_pattern(note, p),
        PopCmd::Deco(d) => set_deco(note, d),
        PopCmd::Opacity(o) => set_opacity(note, o),
        PopCmd::Pin(on) => set_pinned(note, on),
        PopCmd::Peek(on) => {
            let _ = with(|s| {
                if let Some(i) = idx_of(s, note) {
                    disarm_delete(s, i);
                }
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
            font: w.sticky.font,
            pattern: w.sticky.pattern,
            deco: w.sticky.deco,
            opacity: w.sticky.opacity,
            pinned: w.sticky.pinned,
            peek: s.prefs.cat_peek,
            lang: s.lang,
            ui: s.font,
        })
    })
    .flatten()
}

fn open_popover(hwnd: HWND) {
    let info = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        let r = paint::button_rect(Btn::Colour, w.view.w);
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

/// The popover closed: a note that was being edited gets its keyboard back,
/// unless the user left for another app (`away`): then the editing ends, as
/// it does when the note itself loses the activation.
pub(crate) fn pop_closed(note: HWND, away: bool) {
    // A tucked note the popover held out lets go of it.
    let _ = with(|s| {
        let i = idx_of(s, note)?;
        tuck_settle(s, i);
        Some(())
    });
    if away {
        unsafe {
            let _ = PostMessageW(Some(note), WM_END_EDIT, WPARAM(0), LPARAM(0));
        }
        return;
    }
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
const CMD_STAMP: usize = 50;
const CMD_REMIND: usize = 30;
const CMD_REMIND_CUSTOM: usize = 40;
const CMD_REMIND_CLEAR: usize = 41;
const CMD_PIN: usize = 20;
const CMD_ROLL: usize = 21;
const CMD_UNSTICK: usize = 22;
const CMD_DELETE: usize = 23;
/// Take the sheet out of its stack, and tuck to the edge or bring out (right
/// after "Roll up").
const CMD_UNSTACK: usize = 24;
const CMD_TUCK: usize = 25;
/// "Kirim ke HP": the note this window shows goes to the phone link.
const CMD_SEND_PHONE: usize = 26;

/// What the right-click menu needs to know about a note.
struct MenuInfo {
    paper: Paper,
    pinned: bool,
    rolled: bool,
    confirm: bool,
    lang: Lang,
    has_remind: bool,
    stamp: Stamp,
    in_stack: bool,
    tucked: bool,
    /// The edge "Selipkan ke tepi" would use; `None` greys the item.
    tuck_edge: Option<Edge>,
    lock: bool,
}

/// "Kirim ke HP" on window `hwnd`: the id of the sheet the window shows now
/// (after any flips) goes to the owner, never a stack's first or bottom sheet.
fn send_to_phone(hwnd: HWND) {
    let _ = with(|s| {
        let i = idx_of(s, hwnd)?;
        let id = s.wins[i].id.clone();
        push(s, WidgetEvent::SendToPhone { id });
        Some(())
    });
}

fn context_menu(hwnd: HWND, at: POINT) {
    let data = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &s.wins[i];
        Some(MenuInfo {
            paper: w.sticky.paper,
            pinned: w.sticky.pinned,
            rolled: w.sticky.rolled,
            confirm: w.confirm_delete,
            lang: s.lang,
            has_remind: w.sticky.remind_at.is_some(),
            stamp: w.sticky.stamp,
            in_stack: w.stack.is_some(),
            tucked: w.sticky.tuck.is_some(),
            tuck_edge: menu_edge_of(s, i),
            lock: s.prefs.lock,
        })
    })
    .flatten();
    let Some(MenuInfo {
        paper,
        pinned,
        rolled,
        confirm,
        lang,
        has_remind,
        stamp,
        in_stack,
        tucked,
        tuck_edge,
        lock,
    }) = data
    else {
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
        // "Ingatkan aku": the quick choices, a time of one's own, and off.
        if let Ok(remind_menu) = CreatePopupMenu() {
            for (n, p) in Preset::ALL.iter().enumerate() {
                let _ = AppendMenuW(remind_menu, MF_STRING, CMD_REMIND + n, &h(p.label_key()));
            }
            let _ = AppendMenuW(remind_menu, MF_STRING, CMD_REMIND_CUSTOM, &h(Key::StickyRemindCustom));
            let _ = AppendMenuW(remind_menu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(
                remind_menu,
                if has_remind { MF_STRING } else { MF_STRING | MF_GRAYED },
                CMD_REMIND_CLEAR,
                &h(Key::StickyRemindClear),
            );
            let _ = AppendMenuW(menu, MF_POPUP, remind_menu.0 as usize, &h(Key::StickyRemind));
        }
        // "Cap": none, or one of the six, as radio items.
        if let Ok(stamps) = CreatePopupMenu() {
            for (n, st) in Stamp::ALL.iter().enumerate() {
                let _ = AppendMenuW(stamps, MF_STRING, CMD_STAMP + n, &h(st.label_key()));
            }
            let at_stamp = Stamp::ALL.iter().position(|x| *x == stamp).unwrap_or(0);
            let _ = CheckMenuRadioItem(
                stamps,
                CMD_STAMP as u32,
                (CMD_STAMP + Stamp::ALL.len() - 1) as u32,
                (CMD_STAMP + at_stamp) as u32,
                windows::Win32::UI::WindowsAndMessaging::MF_BYCOMMAND.0,
            );
            let _ = AppendMenuW(menu, MF_POPUP, stamps.0 as usize, &h(Key::StickyStamp));
        }
        let _ = AppendMenuW(menu, MF_STRING, CMD_SEND_PHONE, &h(Key::StickySendPhone));
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
        if in_stack {
            let _ = AppendMenuW(menu, MF_STRING, CMD_UNSTACK, &h(Key::StickyTakeOut));
        }
        // Tuck to the nearer outer edge (greyed with Kunci posisi, and when
        // neither side is an outer edge), or bring the tucked note out.
        let (tuck_label, tuck_flags) = if tucked {
            (Key::StickyUntuck, MF_STRING)
        } else if lock || tuck_edge.is_none() {
            (Key::StickyTuck, MF_STRING | MF_GRAYED)
        } else {
            (Key::StickyTuck, MF_STRING)
        };
        let _ = AppendMenuW(menu, tuck_flags, CMD_TUCK, &h(tuck_label));
        let _ = AppendMenuW(menu, MF_STRING, CMD_UNSTICK, &h(Key::StickyOffDesktopTip));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            CMD_DELETE,
            &h(if confirm { Key::StickyDeleteAgain } else { Key::StickyDelete }),
        );
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
    if cmd != 0 && cmd != CMD_DELETE {
        // Any other choice is the next other action (a dismissed menu is not).
        let _ = with(|s| {
            let i = idx_of(s, hwnd)?;
            disarm_delete(s, i);
            Some(())
        });
        flush_dirty();
    }
    match cmd {
        0 => {}
        c if (CMD_PAPER..CMD_PAPER + Paper::ALL.len()).contains(&c) => {
            set_paper(hwnd, Paper::ALL[c - CMD_PAPER]);
        }
        c if (CMD_STAMP..CMD_STAMP + Stamp::ALL.len()).contains(&c) => {
            set_stamp(hwnd, Stamp::ALL[c - CMD_STAMP]);
        }
        c if (CMD_REMIND..CMD_REMIND + Preset::ALL.len()).contains(&c) => {
            let now = chrono::Local::now().naive_local();
            set_remind(hwnd, Some(remind::to_unix(Preset::ALL[c - CMD_REMIND].at(now))));
        }
        CMD_REMIND_CUSTOM => open_remind_entry(hwnd),
        CMD_REMIND_CLEAR => set_remind(hwnd, None),
        CMD_SEND_PHONE => send_to_phone(hwnd),
        CMD_PIN => set_pinned(hwnd, !pinned),
        CMD_ROLL => toggle_rolled(hwnd),
        CMD_UNSTACK => {
            let _ = with(|s| {
                let i = idx_of(s, hwnd)?;
                let id = s.wins[i].id.clone();
                push(s, WidgetEvent::StackOut { id });
                Some(())
            });
        }
        CMD_TUCK => {
            let _ = with(|s| {
                let i = idx_of(s, hwnd)?;
                let edge = if s.wins[i].sticky.tuck.is_some() {
                    None
                } else {
                    Some(menu_edge_of(s, i)?)
                };
                set_tuck(s, i, edge);
                Some(())
            });
            flush_dirty();
        }
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

// ------------------------------------------------------------------ tucking

/// Which sides of display `di` have another display's work area against them
/// across the physical-px span `span` (an inner edge: the note crosses to the
/// next monitor there and never tucks).
fn adjoin_at(s: &State, di: usize, span: (i32, i32)) -> tuck::Adjoin {
    let Some(d) = s.displays.get(di) else {
        return tuck::Adjoin::default();
    };
    let others: Vec<Rect> = s.displays.iter().map(|o| o.monitor.work).collect();
    tuck::adjoin(&d.monitor.work, &others, span)
}

/// The edge "Selipkan ke tepi" tucks note `i` to: the nearer outer one to its
/// centre on its display; `None` when neither side is an outer edge.
fn menu_edge_of(s: &State, i: usize) -> Option<Edge> {
    let w = &s.wins[i];
    let d = s.displays.get(w.display)?;
    let area = place::area_dip(d);
    let shown = DipRect {
        h: shown_h(w.view.h, w.sticky.rolled),
        ..w.view
    };
    let adj = adjoin_at(s, w.display, (w.note.y, w.note.bottom()));
    tuck::menu_edge(&shown, &area, adj)
}

/// Note `i` is tucked to `edge` (its window stays at the out rect, flush with
/// the edge; it is out until the pointer is away and the grace is over) or
/// brought out (`None`, where it is). Tells the owner.
fn set_tuck(s: &mut State, i: usize, edge: Option<Edge>) {
    let w = &mut s.wins[i];
    match edge {
        Some(e) => {
            let Some(d) = s.displays.get(w.display) else {
                return;
            };
            let shown = DipRect {
                h: shown_h(w.view.h, w.sticky.rolled),
                ..w.view
            };
            let out = tuck::out_rect(&shown, e, &place::area_dip(d));
            w.sticky.rect.x = out.x;
            w.sticky.rect.y = out.y;
            w.sticky.monitor = place::path_to_save(d);
            w.sticky.tuck = Some(e);
            w.pop_edge = Some(e);
            // It stays out until the pointer is away: nothing keeps it now.
            w.pointer_in = false;
            w.dirty = true;
            let (id, monitor, rect) = (w.id.clone(), w.sticky.monitor.clone(), w.sticky.rect);
            push(s, WidgetEvent::Placed { id: id.clone(), monitor, rect });
            push(s, WidgetEvent::Tuck { id, edge });
            relayout(s, i);
        }
        None => {
            // Brought out: it pops out of its tab where it is (instant when
            // nothing animates, nothing at all if it was out already).
            let (now, animate) = (Instant::now(), s.animate);
            w.pop_edge = w.sticky.tuck.or(w.pop_edge);
            w.sticky.tuck = None;
            w.grace = None;
            w.pop.toward(1.0, now, animate);
            w.dirty = true;
            let (id, running) = (w.id.clone(), w.pop.running(now));
            push(s, WidgetEvent::Tuck { id, edge: None });
            if running {
                push(s, WidgetEvent::NeedsClock);
            }
        }
    }
    touch(s, i);
    tuck_settle(s, i);
}

// ------------------------------------------------------------------ dragging

/// The drag ended because the window lost the mouse (not a release): nothing
/// stacks.
fn end_drag(s: &mut State, i: usize) {
    finish_drag(s, i, false);
}

/// The drag of note `i` ends. A `release` of a Move with Shift still down over
/// another unit stacks on it; one pressed against an outer edge tucks; any
/// other tucks no more (a tucked note dragged away comes out where dropped).
fn finish_drag(s: &mut State, i: usize, release: bool) {
    let Some(d) = s.wins[i].drag.take() else {
        return;
    };
    settle_alpha(s, i);
    for j in d.lit {
        if let Some(w) = s.wins.get_mut(j) {
            w.guides.clear();
            w.dirty = true;
        }
    }
    for w in &mut s.wins {
        if w.targeted {
            w.targeted = false;
            w.dirty = true;
        }
    }
    let stack_on = d
        .target
        .filter(|_| release && d.moved && d.kind == DragKind::Move && shift_down())
        .and_then(|h| s.wins.iter().position(|w| w.hwnd == h))
        .filter(|j| *j != i && s.wins[*j].sticky.tuck.is_none());
    let w = &mut s.wins[i];
    w.guides.clear();
    w.cue = None;
    w.dirty = true;
    if let Some(j) = stack_on {
        let (dragged, target) = (w.unit_ids(), s.wins[j].unit_ids());
        push(s, WidgetEvent::Stack { dragged, target });
    } else if d.moved {
        let monitor = s
            .displays
            .get(w.display)
            .map(place::path_to_save)
            .unwrap_or_default();
        w.sticky.monitor = monitor.clone();
        // Only a Move changes the tuck: pressed against an outer edge it
        // tucks, anywhere else it is out in the open.
        let tuck_to = if d.kind == DragKind::Move { d.cue } else { w.sticky.tuck };
        let changed = tuck_to != w.sticky.tuck;
        if let (Some(edge), Some(disp)) = (tuck_to, s.displays.get(w.display)) {
            // Flush with the edge, exactly.
            let shown = DipRect {
                h: shown_h(w.view.h, w.sticky.rolled),
                ..w.view
            };
            let out = tuck::out_rect(&shown, edge, &place::area_dip(disp));
            w.sticky.rect.x = out.x;
            w.sticky.rect.y = out.y;
        }
        w.sticky.tuck = tuck_to;
        w.pop_edge = tuck_to.or(w.pop_edge);
        let (id, rect) = (w.id.clone(), w.sticky.rect);
        push(
            s,
            WidgetEvent::Placed {
                id: id.clone(),
                monitor,
                rect,
            },
        );
        if changed {
            push(s, WidgetEvent::Tuck { id, edge: tuck_to });
        }
        if tuck_to.is_some() {
            relayout(s, i);
        }
    }
    // The pointer is where the drag left it: a note it is over stays out.
    let w = &mut s.wins[i];
    w.pointer_in = target_at(w, d.last.0, d.last.1) != Target::Outside;
    tuck_settle(s, i);
    flush_dirty_in(s);
}

fn flush_dirty_in(s: &mut State) {
    for i in 0..s.wins.len() {
        if s.wins[i].dirty {
            draw(s, i);
        }
    }
}

/// A note of saved rect `saved` dragged so its top-left is at `at` (dip on
/// `area`): where it snaps, and how it is shown there (`Placement::view`: the
/// saved size, clamped to this area).
fn moved_view(
    saved: DipRect,
    rolled: bool,
    at: (f32, f32),
    area: &DipRect,
    others: &[DipRect],
    alt: bool,
) -> (Snapped, DipRect) {
    let want = DipRect::new(at.0, at.1, saved.w, shown_h(saved.h, rolled));
    let snapped = snap_move_for(want, area, others, alt, rolled);
    let r = snapped.rect;
    let view = place::view_on(DipRect { x: r.x, y: r.y, ..saved }, rolled, area);
    (snapped, view)
}

/// A note shown as `start` (when the grip was taken) resized to `size`: its
/// top-left stays where it is shown, which for a note on a fallback monitor
/// is not its saved place.
fn resized(
    start: &DipRect,
    size: (f32, f32),
    area: &DipRect,
    others: &[DipRect],
    alt: bool,
) -> Snapped {
    snap_resize(DipRect::new(start.x, start.y, size.0, size.1), area, others, alt)
}

/// The pointer moved at `cursor` (px) while a note is dragged or resized.
fn drag_move(s: &mut State, i: usize, cursor: (i32, i32), alt: bool, shift: bool) {
    let (kind, start_cursor, start_note, start_dip, moved) = {
        let d = s.wins[i].drag.as_mut().expect("a drag");
        d.last = cursor;
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
        // A tucked note is only a tab: nothing to line up with.
        if j != i && w.display == di && w.sticky.tuck.is_none() {
            idxs.push(j);
            others.push(DipRect {
                h: shown_h(w.view.h, w.sticky.rolled),
                ..w.view
            });
        }
    }
    let (snapped, view) = match kind {
        DragKind::Move => moved_view(
            s.wins[i].sticky.rect,
            rolled,
            (rel.x, rel.y),
            &area,
            &others,
            alt,
        ),
        DragKind::Resize => {
            let mut snapped = resized(&start_dip, (rel.w, rel.h), &area, &others, alt);
            // A stack is never narrower than its counter and arrows need.
            if s.wins[i].stack.is_some() && snapped.rect.w < stack::MIN_W {
                snapped.rect.w = stack::MIN_W.min((area.right() - snapped.rect.x).max(0.0));
            }
            let view = snapped.rect;
            (snapped, view)
        }
    };
    let rect = snapped.rect;
    let w = &mut s.wins[i];
    let rescaled = w.scale != scale || w.display != di;
    // Only a resize changes the saved size: a move onto a smaller display
    // shows the note smaller there and keeps the size it was given.
    w.sticky.rect.x = rect.x;
    w.sticky.rect.y = rect.y;
    if kind == DragKind::Resize {
        w.sticky.rect.w = rect.w;
        w.sticky.rect.h = rect.h;
    }
    let px = place::to_px(
        &DipRect {
            h: shown_h(view.h, rolled),
            ..view
        },
        &area_px,
        scale,
    );
    let size_changed =
        px.w != w.note.w || px.h != w.note.h || view.w != w.view.w || view.h != w.view.h;
    w.view = view;
    w.note = px;
    w.scale = scale;
    w.display = di;
    w.sticky.monitor = place::path_to_save(&disp);
    if rescaled || size_changed {
        w.canvas = None;
        w.body_key = None;
        w.dirty = true;
    }
    if kind == DragKind::Move {
        drag_cues(s, i, cursor, shift, (di, &rect, &area, &area_px, scale));
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
        let origin = w.origin();
        let _ = canvas.present(w.hwnd, origin, alpha_u8(w, Instant::now()));
    }
    flush_dirty_in(s);
}

/// What a release here would do to a Move drag of note `i`: with Shift held and
/// the pointer inside another unit's shown rect (tucked ones are left out) it
/// would stack on it, and that note is lit; else, pressed against an outer edge
/// it would tuck, and the dragged note shows an accent bar on that edge.
/// `on` is the display, the dragged rect (dip), the placement area (dip and
/// px) and the scale.
fn drag_cues(
    s: &mut State,
    i: usize,
    cursor: (i32, i32),
    shift: bool,
    on: (usize, &DipRect, &DipRect, &Rect, f32),
) {
    let (di, rect, area, area_px, scale) = on;
    let target = if shift {
        s.wins
            .iter()
            .enumerate()
            .find(|(j, w)| {
                *j != i
                    && !w.away
                    && w.sticky.tuck.is_none()
                    && !w.note.is_empty()
                    && w.note.contains(cursor)
            })
            .map(|(_, w)| w.hwnd)
    } else {
        None
    };
    let cue = if target.is_none() {
        let span = (
            area_px.y + (rect.y * scale).round() as i32,
            area_px.y + (rect.bottom() * scale).round() as i32,
        );
        let adj = adjoin_at(s, di, span);
        let at = place::to_dip(&Rect::new(cursor.0, cursor.1, 0, 0), area_px, scale);
        tuck::press_edge(rect, area, (at.x, at.y), adj)
    } else {
        None
    };
    let before = s.wins[i].drag.as_ref().and_then(|d| d.target);
    if before != target {
        for w in &mut s.wins {
            let on = Some(w.hwnd) == target;
            if w.targeted != on {
                w.targeted = on;
                w.dirty = true;
            }
        }
    }
    if let Some(d) = s.wins[i].drag.as_mut() {
        d.target = target;
        d.cue = cue;
    }
    if s.wins[i].cue != cue {
        s.wins[i].cue = cue;
        s.wins[i].dirty = true;
    }
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
        // With the bar (it shows as soon as the note has the keyboard).
        let area = paint::body_rect_in(w.view.w, w.view.h, true);
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
        if existing.is_none() {
            w.pending = None;
        }
        w.caret_on = true;
        w.sel = (at, at);
        w.anchor = at;
        w.follow_caret = true;
        w.dirty = true;
        // Opening a note whose reminder went off ends it: no chip, no notice.
        if remind::overdue(&w.sticky, unix_now()) {
            w.sticky.remind_at = None;
            w.sticky.remind_fired = false;
            let id = w.id.clone();
            push(s, WidgetEvent::Remind { id, at: None });
            refresh_peek(s);
        }
        settle_alpha(s, i);
        // A tucked note that gets the keyboard pops out and stays out.
        tuck_settle(s, i);
        Some(())
    });
    lower(hwnd);
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
    if fmt_pop::is_open_for(hwnd) {
        fmt_pop::close();
    }
    let ended = with(|s| {
        let i = idx_of(s, hwnd)?;
        let w = &mut s.wins[i];
        let e = w.edit.take()?;
        w.caret_on = false;
        w.selecting = false;
        w.dirty = true;
        // A note the list brought to the front goes back to its layer.
        let lifted = std::mem::take(&mut w.lifted);
        settle_alpha(s, i);
        // Nothing keeps a tucked note out now: the grace starts.
        tuck_settle(s, i);
        Some((e, lifted.then_some(i)))
    })
    .flatten();
    if let Some((e, lifted)) = ended {
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER_WIDGET_CARET);
            let _ = DestroyWindow(e);
        }
        if let Some(i) = lifted {
            apply_z(i);
        }
    }
    flush_dirty();
}

/// What the hidden EDIT tells the note (called from its own procedure).
pub(crate) fn edit_event(parent: HWND, edit_hwnd: HWND, ev: EditEvent) {
    match ev {
        EditEvent::Changed => sync_from_edit(parent, edit_hwnd),
        EditEvent::Escape => {
            if pop::is_open() || remind_pop::is_open() || fmt_pop::is_open() {
                pop::close();
                remind_pop::close();
                fmt_pop::close();
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
            // A popover taking the focus is not the end of the editing, and
            // neither is the file dialog of this note.
            let dialog = with(|s| idx_of(s, parent).is_some_and(|i| s.wins[i].dialog)).unwrap_or(false);
            if current && !pop::is_open() && !remind_pop::is_open() && !dialog {
                unsafe {
                    let _ = PostMessageW(Some(parent), WM_END_EDIT, WPARAM(0), LPARAM(0));
                }
            }
        }
        EditEvent::Vertical { up, extend } => move_vertical(parent, edit_hwnd, up, extend),
        EditEvent::Format(tool) => {
            let current = with(|s| idx_of(s, parent).is_some_and(|i| s.wins[i].edit == Some(edit_hwnd)))
                .unwrap_or(false);
            if current {
                format_tool(parent, tool);
            }
        }
        EditEvent::Copy { cut } => copy_selection(parent, edit_hwnd, cut),
        EditEvent::Paste => paste_into(parent, edit_hwnd),
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
        // A style chosen for the next letter is gone once the caret moves
        // without any typing.
        if w.text == before && w.sel != sel {
            w.pending = None;
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
        let m = w.margins();
        let area = w.body_area();
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
/// says (the EDIT does not wrap; the drawn text does).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrange::Monitor;
    use crate::widgets::paint::margins;
    use crate::widgets::sticky::ROLLED_H;

    fn display(path: &str, x: i32, primary: bool, dpi: u32) -> Display {
        Display {
            monitor: Monitor {
                device: format!(r"\.\{path}"),
                rect: Rect::new(x, 0, 1920, 1080),
                work: Rect::new(x, 0, 1920, 1040),
                dpi,
            },
            hmonitor: 1,
            primary,
            path: path.to_owned(),
            name: String::new(),
            number: 1,
        }
    }

    fn sticky(path: &str, rect: DipRect) -> Sticky {
        Sticky {
            monitor: path.to_owned(),
            rect,
            ..Sticky::default()
        }
    }

    /// Giving a window the keyboard activates it, and the window that loses the
    /// activation ends its editing: tests that do it run one at a time, or one
    /// test's window would take the keyboard from another's.
    fn keyboard_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn ctx<'a>(notes: &'a [Note], displays: &'a [Display]) -> Ctx<'a> {
        Ctx {
            notes,
            // Hidden windows: nothing appears on the tester's desktop.
            prefs: WidgetPrefs {
                show: false,
                ..WidgetPrefs::default()
            },
            displays,
            lang: Lang::Id,
            font: UiFont::Sans,
            coat: CatColor::Terracotta,
            animate: false,
        }
    }

    fn note(id: &str, rect: DipRect) -> Note {
        Note {
            id: id.to_owned(),
            text: "Belanja".into(),
            updated: 1,
            sticky: Some(sticky("A", rect)),
        }
    }

    #[test]
    fn a_new_note_can_be_dragged_straight_away() {
        let _focus = keyboard_lock();
        use windows::Win32::UI::WindowsAndMessaging::{CWP_ALL, ChildWindowFromPointEx};
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(400.0, 200.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        // A fresh note gets the keyboard: the hidden EDIT exists.
        host.focus("n1");
        let (hwnd, edit_hwnd, start, k) = with(|s| {
            let w = &s.wins[0];
            (w.hwnd, w.edit, w.note, w.scale)
        })
        .expect("the host has a window");
        let edit_hwnd = edit_hwnd.expect("the note has the keyboard");
        // A point of the strip that lies under the EDIT's rectangle.
        let m = margins();
        let (x, y) = (((m.0 + 20.0) * k) as i32, ((m.1 + 10.0) * k) as i32);
        let under = unsafe { ChildWindowFromPointEx(hwnd, POINT { x, y }, CWP_ALL) };
        assert_eq!(under, edit_hwnd, "the EDIT really covers this point of the strip");
        // The EDIT lets the press through to the note.
        let answer = unsafe {
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                edit_hwnd,
                windows::Win32::UI::WindowsAndMessaging::WM_NCHITTEST,
                Some(WPARAM(0)),
                Some(LPARAM(((y << 16) | x) as isize)),
            )
        };
        assert_eq!(answer.0, -1, "HTTRANSPARENT");
        let _ = host.drain();
        left_down(hwnd, x, y);
        mouse_move(hwnd, x + 40, y);
        left_up(hwnd, x + 40, y);
        let events = host.drain();
        let placed = events.iter().find_map(|e| match e {
            WidgetEvent::Placed { rect, .. } => Some(*rect),
            _ => None,
        });
        let rect = placed.expect("the drag ended with a Placed event");
        assert!(rect.x > 400.0 + 20.0, "moved right: {rect:?}");
        assert!((rect.y - 200.0).abs() <= 8.0, "kept its height: {rect:?}");
        assert_eq!((rect.w, rect.h), (240.0, 208.0), "a move keeps the size");
        let now = with(|s| s.wins[0].note).unwrap();
        assert!(now.x > start.x, "the window followed");
        host.shutdown();
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|l| (*l).to_owned()).collect()
    }

    #[test]
    fn a_drop_adds_its_lines_to_a_note_without_the_keyboard() {
        let displays = vec![display("A", 0, true, 96)];
        // Locked and rolled: neither blocks a drop.
        let notes = vec![note_with("n1", "Belanja", |s| s.rolled = true)];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.prefs.lock = true;
        host.sync(&c);
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let _ = host.drain();
        assert!(drop_into(hwnd, &lines(&["https://example.com", "C:\\a.txt"])));
        let expected = "Belanja\nhttps://example.com\nC:\\a.txt";
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), expected);
        assert!(with(|s| s.wins[0].sticky.rolled).unwrap(), "a rolled note stays rolled");
        let events = host.drain();
        assert!(
            events.contains(&WidgetEvent::Text {
                id: "n1".into(),
                text: expected.into(),
            }),
            "{events:?}"
        );
        assert!(events.iter().any(|e| matches!(e, WidgetEvent::Touched { id, .. } if id == "n1")));
        assert!(with(|s| !s.wins[0].dirty).unwrap(), "drawn");
        // A second drop goes after the first.
        assert!(drop_into(hwnd, &lines(&["x"])));
        assert_eq!(
            with(|s| s.wins[0].text.clone()).unwrap(),
            "Belanja\nhttps://example.com\nC:\\a.txt\nx"
        );
        // Nothing to add: nothing happens, no event.
        let _ = host.drain();
        assert!(!drop_into(hwnd, &[]));
        assert!(host.drain().is_empty());
        host.shutdown();
    }

    #[test]
    fn a_drop_onto_an_empty_note_is_just_the_lines() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", "", |_| {})];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        assert!(drop_into(hwnd, &lines(&["a", "b"])));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "a\nb");
        host.shutdown();
    }

    #[test]
    fn a_drop_without_the_keyboard_keeps_the_note_within_the_edit_limit() {
        // The EDIT takes at most `edit::LIMIT` units (each break is two), so a
        // note set past it could be shown but no longer typed into.
        let edit_units = |t: &str| t.encode_utf16().count() + t.matches('\n').count();
        let displays = vec![display("A", 0, true, 96)];
        let near = "n".repeat(edit::LIMIT - 500);
        let notes = vec![note_with("n1", &near, |_| {})];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let block = vec!["x".repeat(99); 20];
        assert!(drop_into(hwnd, &block), "there is room for some of it");
        let text = with(|s| s.wins[0].text.clone()).unwrap();
        assert!(text.starts_with(&near), "what was there stays");
        assert!(edit_units(&text) <= edit::LIMIT, "{}", edit_units(&text));
        assert!(edit_units(&text) > edit::LIMIT - 120, "it fills what fits");
        // One long line fills it to the limit exactly.
        assert!(drop_into(hwnd, &lines(&[&"y".repeat(500)])));
        let text = with(|s| s.wins[0].text.clone()).unwrap();
        assert_eq!(edit_units(&text), edit::LIMIT);
        // Full: a drop adds nothing and says so, with no event.
        let _ = host.drain();
        assert!(!drop_into(hwnd, &lines(&["more"])));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), text);
        assert!(host.drain().is_empty());
        host.shutdown();
    }

    #[test]
    fn a_drop_onto_a_note_with_the_keyboard_goes_through_its_edit() {
        let _focus = keyboard_lock();
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", "Belanja", |_| {})];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        host.focus("n1");
        let (hwnd, edit_hwnd) = with(|s| (s.wins[0].hwnd, s.wins[0].edit)).unwrap();
        let edit_hwnd = edit_hwnd.expect("the note has the keyboard");
        // The caret is somewhere else: a drop still goes to the end.
        let _ = with(|s| edit::set_selection(edit_hwnd, &s.wins[0].text, 0, 0));
        let _ = host.drain();
        assert!(drop_into(hwnd, &lines(&["one", "two"])));
        let expected = "Belanja\none\ntwo";
        assert_eq!(edit::text(edit_hwnd), expected, "the EDIT holds it");
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), expected);
        let end = expected.encode_utf16().count();
        assert_eq!(with(|s| s.wins[0].sel).unwrap(), (end, end), "the caret is at the end");
        let events = host.drain();
        assert!(events.contains(&WidgetEvent::Text {
            id: "n1".into(),
            text: expected.into(),
        }));
        // Ctrl+Z takes the drop back: it went in through EM_REPLACESEL.
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SendMessageW(
                edit_hwnd,
                windows::Win32::UI::Controls::EM_UNDO,
                Some(WPARAM(0)),
                Some(LPARAM(0)),
            );
        }
        assert_eq!(edit::text(edit_hwnd), "Belanja");
        host.shutdown();
    }

    #[test]
    fn the_drop_outline_shows_while_a_drag_is_over_and_goes_on_leave() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(400.0, 200.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        assert!(!with(|s| s.wins[0].drop_hover).unwrap());
        drop_hover(hwnd, true);
        assert!(with(|s| s.wins[0].drop_hover).unwrap());
        assert!(with(|s| !s.wins[0].dirty).unwrap(), "drawn with the outline");
        drop_hover(hwnd, false);
        assert!(!with(|s| s.wins[0].drop_hover).unwrap());
        host.shutdown();
    }

    #[test]
    fn every_note_window_takes_drops_and_revokes_when_it_is_destroyed() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![
            note("n1", DipRect::new(400.0, 200.0, 240.0, 208.0)),
            note("n2", DipRect::new(700.0, 200.0, 240.0, 208.0)),
        ];
        let mut host = WidgetHost::new(HWND::default());
        assert_eq!(drop::live_targets(), 0);
        host.sync(&ctx(&notes, &displays));
        assert_eq!(drop::live_targets(), 2, "one target per note window");
        // One note leaves the desktop (`sync` destroys its window): its target
        // is revoked, the other stays.
        host.sync(&ctx(&notes[1..], &displays));
        assert_eq!(drop::live_targets(), 1, "the removed note revoked its target");
        host.shutdown();
        assert_eq!(drop::live_targets(), 0, "every destroyed window revoked");
    }

    #[test]
    fn a_press_on_a_button_that_moves_becomes_a_drag_not_a_click() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(400.0, 200.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let (hwnd, k) = with(|s| (s.wins[0].hwnd, s.wins[0].scale)).unwrap();
        // The buttons are shown (the pointer is over the note).
        let _ = with(|s| s.wins[0].header = Ease::at(1.0));
        let r = paint::button_rect(Btn::Pin, 240.0);
        let m = margins();
        let (x, y) = (
            ((m.0 + r.x + r.w / 2.0) * k) as i32,
            ((m.1 + r.y + r.h / 2.0) * k) as i32,
        );
        let _ = host.drain();
        left_down(hwnd, x, y);
        assert_eq!(with(|s| s.wins[0].pressed).unwrap(), Some(Btn::Pin));
        mouse_move(hwnd, x + 2, y + 1);
        assert_eq!(
            with(|s| s.wins[0].pressed).unwrap(),
            Some(Btn::Pin),
            "3 px is still a click"
        );
        mouse_move(hwnd, x + 30, y);
        assert_eq!(with(|s| s.wins[0].pressed).unwrap(), None, "now it is a drag");
        left_up(hwnd, x + 30, y);
        let events = host.drain();
        assert!(events.iter().any(|e| matches!(e, WidgetEvent::Placed { .. })));
        assert!(
            !events.iter().any(|e| matches!(e, WidgetEvent::Pinned { .. })),
            "no click on release"
        );
        host.shutdown();
    }

    #[test]
    fn a_locked_note_does_not_turn_a_button_press_into_a_drag() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(400.0, 200.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.prefs.lock = true;
        host.sync(&c);
        let (hwnd, k) = with(|s| (s.wins[0].hwnd, s.wins[0].scale)).unwrap();
        let _ = with(|s| s.wins[0].header = Ease::at(1.0));
        let r = paint::button_rect(Btn::Pin, 240.0);
        let m = margins();
        let (x, y) = (
            ((m.0 + r.x + r.w / 2.0) * k) as i32,
            ((m.1 + r.y + r.h / 2.0) * k) as i32,
        );
        let _ = host.drain();
        left_down(hwnd, x, y);
        mouse_move(hwnd, x + 40, y);
        assert!(with(|s| s.wins[0].drag.is_none()).unwrap());
        left_up(hwnd, x + 40, y);
        assert!(
            !host
                .drain()
                .iter()
                .any(|e| matches!(e, WidgetEvent::Placed { .. }))
        );
        host.shutdown();
    }

    fn note_with(id: &str, text: &str, f: impl FnOnce(&mut Sticky)) -> Note {
        let mut s = sticky("A", DipRect::new(400.0, 200.0, 240.0, 208.0));
        f(&mut s);
        Note {
            id: id.to_owned(),
            text: text.to_owned(),
            updated: 1,
            sticky: Some(s),
        }
    }

    /// The client point (px) of note-dip point (`x`, `y`) of window 0, as drawn.
    fn client_of(x: f32, y: f32) -> (i32, i32) {
        with(|s| {
            let w = &s.wins[0];
            let m = w.margins();
            let (nw, nh) = (w.view.w, shown_h(w.view.h, w.sticky.rolled));
            let (rx, ry) = art::rotate_about(x, y, w.lean(), nw / 2.0, nh / 2.0);
            (
                ((m.0 + rx) * w.scale).round() as i32,
                ((m.1 + ry) * w.scale).round() as i32,
            )
        })
        .unwrap()
    }

    #[test]
    fn a_leaning_note_is_hit_through_the_inverse_rotation_and_straight_while_editing() {
        let _focus = keyboard_lock();
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1700-3", "Belanja", |_| {})];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.prefs.tilt = true;
        host.sync(&c);
        let (hwnd, lean) = with(|s| (s.wins[0].hwnd, s.wins[0].lean())).unwrap();
        assert!(lean != 0.0 && lean.abs() <= 2.0, "it leans: {lean}");
        // Where the grip's corner and the strip's end are drawn, the targets are
        // still the grip and the strip.
        let (gx, gy) = client_of(236.0, 204.0);
        let (sx, sy) = client_of(6.0, 8.0);
        let at = |x: i32, y: i32| {
            let pt = screen_point(hwnd, x, y);
            with(|s| target_at(&s.wins[0], pt.x, pt.y)).unwrap()
        };
        assert_eq!(at(gx, gy), Target::Grip);
        assert_eq!(at(sx, sy), Target::Header);
        // The window is bigger than a straight note's by the overhang.
        let (straight, leaning) = with(|s| {
            let w = &s.wins[0];
            (paint::margins(), w.margins())
        })
        .unwrap();
        assert!(leaning.0 > straight.0 && leaning.1 > straight.1);
        // With the keyboard the note is drawn and hit straight, in the same window.
        host.focus("n1700-3");
        assert_eq!(with(|s| s.wins[0].lean()).unwrap(), 0.0);
        assert_eq!(with(|s| s.wins[0].margins()).unwrap(), leaning, "the window keeps its size");
        let (gx, gy) = client_of(236.0, 204.0);
        assert_eq!(at(gx, gy), Target::Grip);
        // Tilt off: straight, the usual margin.
        let mut off = ctx(&notes, &displays);
        off.prefs.tilt = false;
        host.sync(&off);
        assert_eq!(with(|s| s.wins[0].margins()).unwrap(), straight);
        host.shutdown();
    }

    #[test]
    fn the_last_tick_throws_confetti_only_when_things_animate() {
        let displays = vec![display("A", 0, true, 96)];
        for animate in [true, false] {
            let notes = vec![note_with("n1", "Daftar\n- [x] a\n- [ ] b", |_| {})];
            let mut host = WidgetHost::new(HWND::default());
            let mut c = ctx(&notes, &displays);
            c.animate = animate;
            host.sync(&c);
            let _ = with(|s| {
                // The first tick is not the last: nothing yet.
                toggle_line(s, 0, 1);
                assert!(s.wins[0].confetti.is_none(), "an unticking is not a finish");
                toggle_line(s, 0, 1);
                assert!(s.wins[0].confetti.is_none(), "still one open after re-ticking: {}", s.wins[0].text);
            });
            let _ = with(|s| {
                // Tick the open one: now everything is done.
                let text = s.wins[0].text.clone();
                assert!(!checklist::all_done(&text), "{text}");
                toggle_line(s, 0, 2);
                assert!(checklist::all_done(&s.wins[0].text));
                assert_eq!(s.wins[0].confetti.is_some(), animate, "animate {animate}");
            });
            if animate {
                assert!(host.moving(Instant::now()), "the frame clock runs for it");
                // Gone after 0.8 s.
                let later = Instant::now() + std::time::Duration::from_millis(900);
                assert!(!host.moving(later));
                host.tick(later);
                assert!(with(|s| s.wins[0].confetti.is_none()).unwrap());
            } else {
                assert!(!host.moving(Instant::now()), "idle: no clock");
            }
            host.shutdown();
        }
    }

    #[test]
    fn a_see_through_note_is_full_over_the_pointer_the_keyboard_and_a_raise() {
        let _focus = keyboard_lock();
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", "Belanja", |s| s.opacity = 60)];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays)); // animate off: instant
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let shown = || with(|s| (s.wins[0].alpha_shown, alpha_u8(&s.wins[0], Instant::now()))).unwrap();
        assert_eq!(shown(), (153, 153), "60 % while the pointer is away");
        // The pointer comes over the note: full.
        let (x, y) = client_of(100.0, 100.0);
        mouse_move(hwnd, x, y);
        assert_eq!(shown(), (255, 255));
        // And away again: back to 60 %.
        mouse_leave(hwnd);
        assert_eq!(shown(), (153, 153));
        // The keyboard keeps it full.
        host.focus("n1");
        assert_eq!(shown(), (255, 255));
        end_editing(hwnd);
        assert_eq!(shown(), (153, 153));
        // A raise (a reminder fired) too.
        let fired = vec![note_with("n1", "Belanja", |s| {
            s.opacity = 60;
            s.remind_at = Some(5);
            s.remind_fired = true;
        })];
        host.sync(&ctx(&fired, &displays));
        host.raise(&["n1".to_owned()]);
        assert!(with(|s| s.wins[0].raised).unwrap());
        assert_eq!(shown(), (255, 255));
        // Snoozed (no longer fired): lowered, see-through again.
        host.sync(&ctx(&notes, &displays));
        assert!(!with(|s| s.wins[0].raised).unwrap());
        assert_eq!(shown(), (153, 153));
        // An opaque note never changes.
        let plain = vec![note_with("n1", "Belanja", |_| {})];
        host.sync(&ctx(&plain, &displays));
        assert_eq!(shown(), (255, 255));
        host.shutdown();
    }

    #[test]
    fn the_see_through_fade_runs_the_frame_clock_only_while_it_moves() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", "Belanja", |s| s.opacity = 40)];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.animate = true;
        host.sync(&c);
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        assert!(!host.moving(Instant::now()), "at rest: nothing runs");
        let (x, y) = client_of(100.0, 100.0);
        mouse_move(hwnd, x, y);
        assert!(host.moving(Instant::now()), "the fade in runs the clock");
        let later = Instant::now() + std::time::Duration::from_millis(400);
        host.tick(later);
        assert!(!host.moving(later), "and stops");
        assert_eq!(with(|s| alpha_u8(&s.wins[0], later)).unwrap(), 255);
        host.shutdown();
    }

    #[test]
    fn a_raised_note_sits_above_other_windows_until_snoozed_and_wiggles_only_when_animated() {
        let displays = vec![display("A", 0, true, 96)];
        let fired = vec![note_with("n1", "Belanja", |s| {
            s.remind_at = Some(5);
            s.remind_fired = true;
        })];
        for animate in [true, false] {
            let mut host = WidgetHost::new(HWND::default());
            let mut c = ctx(&fired, &displays);
            c.animate = animate;
            host.sync(&c);
            let desktop = with(|s| !above_windows(s, &s.wins[0])).unwrap();
            assert!(desktop, "a desktop-layer note sits at the bottom");
            host.raise(&["n1".to_owned()]);
            assert!(with(|s| above_windows(s, &s.wins[0])).unwrap(), "raised: above");
            assert_eq!(with(|s| s.wins[0].wiggle.is_some()).unwrap(), animate);
            assert_eq!(host.moving(Instant::now()), animate, "the wiggle runs the clock");
            if animate {
                let later = Instant::now() + std::time::Duration::from_millis(700);
                assert!(!host.moving(later), "600 ms and done");
                host.tick(later);
                assert!(with(|s| s.wins[0].wiggle.is_none()).unwrap());
            }
            // A press on the note drops the raise.
            let hwnd = with(|s| s.wins[0].hwnd).unwrap();
            let (x, y) = client_of(100.0, 12.0);
            left_down(hwnd, x, y);
            assert!(!with(|s| s.wins[0].raised).unwrap(), "touched: back down");
            left_up(hwnd, x, y);
            host.shutdown();
        }
    }

    #[test]
    fn hidden_notes_stay_hidden_when_raised_so_the_owner_shows_them_first() {
        // The tester's context has prefs.show = false: a raise sets the flag
        // and the layer, but the window is never shown. A reminder therefore
        // turns the widgets on (App::widgets_reveal) before it raises.
        let displays = vec![display("A", 0, true, 96)];
        let fired = vec![note_with("n1", "Belanja", |s| {
            s.remind_at = Some(5);
            s.remind_fired = true;
        })];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&fired, &displays));
        host.raise(&["n1".to_owned()]);
        assert!(with(|s| s.wins[0].raised).unwrap());
        assert!(!with(|s| s.wins[0].shown).unwrap(), "raised but nobody can see it");
        host.shutdown();
    }

    #[test]
    fn opening_an_overdue_note_clears_its_reminder_and_the_cat_follows_the_overdue_note() {
        let _focus = keyboard_lock();
        let displays = vec![display("A", 0, true, 96)];
        let mut late = note_with("late", "Telat", |s| {
            s.remind_at = Some(100);
            s.remind_fired = true;
            s.touched = 5;
        });
        late.sticky.as_mut().unwrap().rect = DipRect::new(700.0, 200.0, 240.0, 208.0);
        let touched = note_with("touched", "Baru", |s| s.touched = 9_999_999_999);
        let notes = vec![touched, late];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        // The overdue note has the cat even though the other was touched last.
        assert_eq!(with(|s| s.peek.clone()).unwrap().as_deref(), Some("late"));
        let _ = host.drain();
        host.focus("late");
        let events = host.drain();
        assert!(
            events.iter().any(|e| matches!(e, WidgetEvent::Remind { id, at: None } if id == "late")),
            "opening it ends the reminder: {events:?}"
        );
        host.shutdown();
    }

    #[test]
    fn a_note_larger_than_its_display_is_drawn_and_hit_at_the_shown_size() {
        // 640 x 800 dip, shown on 1920 x 1080 at 150 % (area 1280 x 693 dip).
        let d = vec![display("A", 0, true, 144)];
        let s = sticky("A", DipRect::new(400.0, 200.0, 640.0, 800.0));
        let p = place::place(&s, &d).unwrap();
        let area = place::area_px(&d[0]);
        let k = p.scale;
        // The window is made for the view, not the saved size: the note in it
        // ends at the area's bottom, above the taskbar.
        let (_, ch) = canvas_px(&p.view, false, k, margins());
        let m = margins();
        let top = p.rect.y - (m.1 * k).round() as i32;
        let note_bottom = top + ch - (m.3 * k).round() as i32;
        assert!(note_bottom <= area.bottom(), "{note_bottom} > {}", area.bottom());
        assert!(note_bottom >= area.bottom() - 1, "fills the area");
        // The grip is where the shown note ends, inside the area.
        let grip = paint::target(p.view.w - 4.0, p.view.h - 4.0, p.view.w, p.view.h, false, None, 0.0, true);
        assert_eq!(grip, Target::Grip);
        let grip_y = p.rect.y + ((p.view.h - 4.0) * k).round() as i32;
        assert!(grip_y < area.bottom());
        // The text area is the shown one.
        let body = paint::body_rect(p.view.w, p.view.h);
        assert!(body.y + body.h < 693.4 - paint::FOOTER_H + 0.5);
    }

    #[test]
    fn a_move_onto_a_smaller_display_shows_it_smaller_and_keeps_the_saved_size() {
        let area = DipRect::new(0.0, 0.0, 1280.0, 693.333);
        let saved = DipRect::new(10.0, 10.0, 640.0, 800.0);
        let (snapped, view) = moved_view(saved, false, (100.0, 50.0), &area, &[], true);
        assert_eq!(snapped.rect.y, 0.0, "pushed up to fit");
        assert!((snapped.rect.h - 693.333).abs() < 0.01);
        assert_eq!(view, snapped.rect, "the window is what the snap made");
        assert!(view.bottom() <= area.bottom());
        // Rolled up: placed at its shown height, unrolls to the area's height.
        let (snapped, view) = moved_view(saved, true, (100.0, 600.0), &area, &[], true);
        assert_eq!(snapped.rect.h, ROLLED_H);
        assert_eq!((view.x, view.y), (snapped.rect.x, snapped.rect.y));
        assert!((view.h - 693.333).abs() < 0.01);
        // On a display it fits, the size is the saved one.
        let big = DipRect::new(0.0, 0.0, 1920.0, 1040.0);
        let (_, view) = moved_view(saved, false, (100.0, 50.0), &big, &[], true);
        assert_eq!((view.w, view.h), (640.0, 800.0));
    }

    #[test]
    fn a_resize_on_the_fallback_monitor_starts_where_the_note_is_shown() {
        // Saved at x 3000 on a monitor that is gone: shown at the primary's
        // right edge.
        let d = vec![display("A", 0, true, 96)];
        let s = sticky("B", DipRect::new(3000.0, 500.0, 240.0, 208.0));
        let p = place::place(&s, &d).unwrap();
        assert!(p.fallback);
        assert_eq!(p.view.x, 1680.0);
        let area = place::snap_area(&d[0]);
        let r = resized(&p.view, (200.0, 240.0), &area, &[], true).rect;
        assert_eq!((r.x, r.y), (p.view.x, p.view.y), "no jump to the old offset");
        assert_eq!((r.w, r.h), (200.0, 240.0));
    }

    #[test]
    fn a_right_click_on_the_disabled_plus_is_the_context_menu() {
        let plus = Target::Button(Btn::Plus);
        assert!(opens_templates(plus, 1));
        assert!(opens_templates(plus, sticky::MAX_STICKY - 1));
        assert!(!opens_templates(plus, sticky::MAX_STICKY), "at the cap: the plain menu");
        assert!(!opens_templates(Target::Button(Btn::More), 1));
        assert!(!opens_templates(Target::Header, 1));
    }

    #[test]
    fn the_list_button_asks_the_owner_to_open_the_list_and_the_note_tells_where_it_is() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(100.0, 100.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        button(hwnd, Btn::List);
        assert_eq!(host.drain(), vec![WidgetEvent::OpenList { id: "n1".into() }]);
        // The note's own rect (px, without the margin) is what the list sits beside.
        let r = host.note_rect_px("n1").expect("the note has a window");
        assert!(r.w > 0 && r.h > 0, "{r:?}");
        assert!(host.note_rect_px("nope").is_none());
        host.shutdown();
    }

    #[test]
    fn bringing_a_note_to_the_front_lifts_it_over_other_windows_until_it_loses_the_keyboard() {
        let _k = keyboard_lock();
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(100.0, 100.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let mode = || unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
        assert!(with(|s| !above_windows(s, &s.wins[0])).unwrap(), "a desktop-layer note is at the bottom");
        assert_eq!(mode(), 1);
        host.bring_front("nope");
        assert!(with(|s| !s.wins[0].lifted).unwrap(), "no such note: nothing lifts");
        host.bring_front("n1");
        assert!(with(|s| s.wins[0].edit.is_some()).unwrap(), "it has the keyboard");
        assert!(with(|s| s.wins[0].lifted && above_windows(s, &s.wins[0])).unwrap(), "lifted: over other windows");
        assert_eq!(mode(), 2, "the window procedure keeps it topmost");
        // Losing the keyboard puts it back on its layer.
        end_editing(hwnd);
        assert!(with(|s| !s.wins[0].lifted && !above_windows(s, &s.wins[0])).unwrap());
        assert_eq!(mode(), 1, "back at the bottom, owned by the shell again");
        host.shutdown();
    }

    #[test]
    fn a_new_local_day_redraws_once_and_the_first_look_only_remembers() {
        let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
        let mut last = None;
        assert!(!day_turned(&mut last, day(4)), "the first tick: just drawn");
        assert!(!day_turned(&mut last, day(4)));
        assert!(day_turned(&mut last, day(5)), "midnight");
        assert!(!day_turned(&mut last, day(5)), "once a day");
        // The clock set back is a new day for the labels too.
        assert!(day_turned(&mut last, day(4)));
    }

    // ------------------------------------------------------------ stacks and tuck

    use crate::notes::NotesStore;
    use crate::widgets::apply::apply;
    use std::time::Duration;

    /// A notes store (in the temp folder) with one sticky note per entry: its
    /// text and sticky part. Ids are the store's.
    fn store_of(name: &str, stickies: Vec<(&str, Sticky)>) -> NotesStore {
        let dir = std::env::temp_dir().join(format!("vale-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = NotesStore::open(dir.join("notes.json"));
        for (i, (text, s)) in stickies.into_iter().enumerate() {
            let at = 10 + i as i64;
            let id = store.create_sticky(s, at).expect("room");
            store.set_text(&id, text, at);
        }
        store
    }

    fn id_with(store: &NotesStore, text: &str) -> String {
        store
            .list()
            .iter()
            .find(|n| n.text == text)
            .map(|n| n.id.clone())
            .expect("a note with that text")
    }

    /// Three sheets of one stack at (400, 200), 280 wide: "a" shown, "b", "c".
    fn stack_of_three(name: &str) -> NotesStore {
        let sheets = ["a", "b", "c"]
            .into_iter()
            .enumerate()
            .map(|(k, t)| {
                (
                    t,
                    Sticky {
                        stack: Some("s1".into()),
                        stack_order: k as u32,
                        stack_top: k == 0,
                        paper: [Paper::Krem, Paper::Laut, Paper::Plum][k],
                        monitor: "A".into(),
                        rect: DipRect::new(400.0, 200.0, 280.0, 208.0),
                        ..Sticky::default()
                    },
                )
            })
            .collect();
        store_of(name, sheets)
    }

    /// Moves the pointer to the desktop point (`x`, `y`): the message carries
    /// it relative to the window where it is now.
    fn move_to(hwnd: HWND, x: i32, y: i32) {
        let (ox, oy) = with(|s| s.wins[idx_of(s, hwnd).unwrap()].origin()).unwrap();
        mouse_move(hwnd, x - ox, y - oy);
    }

    /// Shift as the test says, until the guard drops (a test thread may be
    /// reused, so it never outlives its test).
    struct ShiftHold;

    impl Drop for ShiftHold {
        fn drop(&mut self) {
            TEST_SHIFT.with(|c| c.set(None));
        }
    }

    fn hold_shift(down: bool) -> ShiftHold {
        set_shift(down);
        ShiftHold
    }

    fn set_shift(down: bool) {
        TEST_SHIFT.with(|c| c.set(Some(down)));
    }

    /// A stand-in for the hidden EDIT (no real focus, which another test's
    /// window could take away): `end_editing` just fails to destroy it.
    fn fake_edit() -> HWND {
        HWND(std::ptr::dangling_mut())
    }

    fn win_of(id: &str) -> usize {
        with(|s| s.wins.iter().position(|w| w.has(id)).expect("a window")).unwrap()
    }

    #[test]
    fn a_plain_drag_over_another_note_does_not_stack() {
        let _shift = hold_shift(false);
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![
            note("n1", DipRect::new(100.0, 100.0, 240.0, 208.0)),
            note("n2", DipRect::new(600.0, 100.0, 240.0, 208.0)),
        ];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let (hwnd, target) = with(|s| (s.wins[0].hwnd, s.wins[1].note)).unwrap();
        let (x, y) = client_of(20.0, 10.0);
        let _ = host.drain();
        left_down(hwnd, x, y);
        // The pointer goes right into the middle of the other note.
        move_to(hwnd, target.x + target.w / 2, target.y + target.h / 2);
        assert!(with(|s| !s.wins[1].targeted).unwrap(), "no highlight without Shift");
        let at = target.x + target.w / 2;
        left_up(hwnd, at, target.y + target.h / 2);
        let events = host.drain();
        assert!(events.iter().any(|e| matches!(e, WidgetEvent::Placed { .. })), "{events:?}");
        assert!(
            !events.iter().any(|e| matches!(e, WidgetEvent::Stack { .. })),
            "a drag never stacks without Shift: {events:?}"
        );
        host.shutdown();
    }

    #[test]
    fn a_shift_release_over_another_note_makes_a_stack() {
        let _shift = hold_shift(true);
        let displays = vec![display("A", 0, true, 96)];
        let mut store = store_of(
            "shift-stack",
            vec![
                ("drag", sticky("A", DipRect::new(100.0, 100.0, 240.0, 208.0))),
                ("target", sticky("A", DipRect::new(600.0, 100.0, 300.0, 208.0))),
            ],
        );
        let (drag_id, target_id) = (id_with(&store, "drag"), id_with(&store, "target"));
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        let (di, ti) = (win_of(&drag_id), win_of(&target_id));
        let (hwnd, dragged_hwnd, target) = with(|s| (s.wins[di].hwnd, s.wins[di].hwnd, s.wins[ti].note)).unwrap();
        let strip = with(|s| {
            let w = &s.wins[di];
            let m = w.margins();
            (((m.0 + 20.0) * w.scale) as i32, ((m.1 + 10.0) * w.scale) as i32)
        })
        .unwrap();
        let _ = host.drain();
        left_down(hwnd, strip.0, strip.1);
        let inside = (target.x + target.w / 2, target.y + target.h / 2);
        move_to(hwnd, inside.0, inside.1);
        assert!(with(|s| s.wins[ti].targeted).unwrap(), "the target is lit while Shift is held over it");
        assert!(with(|s| s.wins[di].cue.is_none()).unwrap());
        // Shift let go over it: no stack, and the light goes.
        set_shift(false);
        move_to(hwnd, inside.0 + 2, inside.1);
        assert!(with(|s| !s.wins[ti].targeted).unwrap());
        set_shift(true);
        move_to(hwnd, inside.0, inside.1);
        assert!(with(|s| s.wins[ti].targeted).unwrap());
        let (ox, oy) = with(|s| s.wins[di].origin()).unwrap();
        left_up(hwnd, inside.0 - ox, inside.1 - oy);
        let events = host.drain();
        assert!(
            events.iter().any(|e| matches!(e,
                WidgetEvent::Stack { dragged, target }
                    if *dragged == vec![drag_id.clone()] && *target == vec![target_id.clone()])),
            "{events:?}"
        );
        assert!(!events.iter().any(|e| matches!(e, WidgetEvent::Placed { .. })), "the join places it");
        assert!(with(|s| s.wins.iter().all(|w| !w.targeted)).unwrap(), "the light is off");
        // The owner applies it; the windows follow: one window, two sheets.
        let out = apply(&mut store, events, 100);
        assert!(out.structural && out.changed);
        host.sync(&ctx(store.list(), &displays));
        let n = with(|s| s.wins.len()).unwrap();
        assert_eq!((n, host.count()), (1, 2), "one window for the stack, two sheets");
        let (hw, ids, pos, w, rect) = with(|s| {
            let w = &s.wins[0];
            let st = w.stack.clone().unwrap();
            (w.hwnd, st.ids, st.pos, w.view.w, w.sticky.rect)
        })
        .unwrap();
        assert_eq!(ids, vec![drag_id.clone(), target_id.clone()], "the dragged sheet first");
        assert_eq!(pos, 0, "and it is the one shown");
        assert_eq!(hw, dragged_hwnd, "the dragged window is the one that stays");
        assert_eq!(w, 300.0, "the target's width");
        assert_eq!((rect.x, rect.y), (600.0, 100.0), "at the target's place");
        host.shutdown();
    }

    #[test]
    fn dragging_a_stack_moves_every_sheet() {
        let _shift = hold_shift(false);
        let displays = vec![display("A", 0, true, 96)];
        let mut store = stack_of_three("stack-drag");
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        assert_eq!(with(|s| s.wins.len()).unwrap(), 1, "one window for three sheets");
        assert_eq!(host.count(), 3);
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let (x, y) = client_of(20.0, 10.0);
        let _ = host.drain();
        left_down(hwnd, x, y);
        mouse_move(hwnd, x + 80, y + 40);
        left_up(hwnd, x + 80, y + 40);
        let events = host.drain();
        let out = apply(&mut store, events, 100);
        assert!(out.changed && !out.structural);
        let rects: Vec<DipRect> = store
            .list()
            .iter()
            .map(|n| n.sticky.as_ref().unwrap().rect)
            .collect();
        assert_eq!(rects.len(), 3);
        assert!(rects.iter().all(|r| *r == rects[0]), "every sheet got the same place: {rects:?}");
        assert!(rects[0].x > 400.0 + 40.0 && rects[0].y > 200.0, "and it moved: {:?}", rects[0]);
        assert_eq!(rects[0].w, 280.0);
        host.shutdown();
    }

    #[test]
    fn the_wheel_on_a_stacks_strip_flips_and_retargets_without_a_new_window() {
        let displays = vec![display("A", 0, true, 96)];
        let store = stack_of_three("stack-wheel");
        let (a, b, c) = (id_with(&store, "a"), id_with(&store, "b"), id_with(&store, "c"));
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let strip = {
            let (x, y) = client_of(150.0, 12.0);
            screen_point(hwnd, x, y)
        };
        let shown = || with(|s| (s.wins[0].id.clone(), s.wins[0].text.clone(), s.wins[0].sticky.paper, s.wins[0].hwnd)).unwrap();
        assert_eq!(shown().0, a);
        let _ = host.drain();
        // One notch down: the next sheet. The same window, new text and paper.
        mouse_wheel(hwnd, strip.x, strip.y, -120);
        assert_eq!((shown().0, shown().1, shown().2), (b.clone(), "b".to_owned(), Paper::Laut));
        assert_eq!(shown().3, hwnd, "the window is retargeted, never remade");
        assert_eq!(with(|s| s.wins.len()).unwrap(), 1);
        assert!(with(|s| s.wins[0].body_key.is_none() || s.wins[0].body_key.as_ref().is_some_and(|k| k.0 == "b")).unwrap());
        let events = host.drain();
        assert!(events.contains(&WidgetEvent::StackShow { id: b.clone() }), "saved as stack_top: {events:?}");
        // Down again, and wrapping past the last.
        mouse_wheel(hwnd, strip.x, strip.y, -120);
        assert_eq!(shown().0, c);
        mouse_wheel(hwnd, strip.x, strip.y, -120);
        assert_eq!(shown().0, a, "wraps to the first");
        // Up is previous, wrapping to the last.
        mouse_wheel(hwnd, strip.x, strip.y, 120);
        assert_eq!(shown().0, c);
        // Small deltas add up to a notch.
        mouse_wheel(hwnd, strip.x, strip.y, 60);
        assert_eq!(shown().0, c, "half a notch");
        mouse_wheel(hwnd, strip.x, strip.y, 60);
        assert_eq!(shown().0, b);
        assert_eq!(shown().3, hwnd);
        // Over the text the wheel scrolls the text instead and flips nothing.
        let body = {
            let (x, y) = client_of(100.0, 100.0);
            screen_point(hwnd, x, y)
        };
        let before = shown().0;
        mouse_wheel(hwnd, body.x, body.y, -120);
        assert_eq!(shown().0, before);
        host.shutdown();
    }

    #[test]
    fn flipping_ends_the_editing_and_keeps_what_was_typed_on_each_sheet() {
        let displays = vec![display("A", 0, true, 96)];
        let store = stack_of_three("stack-edit");
        let (a, b) = (id_with(&store, "a"), id_with(&store, "b"));
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        // The note has the keyboard (a stand-in for the hidden EDIT: no real
        // focus, so another test cannot take it away).
        let _ = with(|s| s.wins[0].edit = Some(fake_edit()));
        // Type into the shown sheet.
        let _ = with(|s| {
            s.wins[0].text = "a, edited".to_owned();
        });
        flip_by(hwnd, 1);
        assert!(with(|s| s.wins[0].edit.is_none()).unwrap(), "editing ended before the swap");
        assert_eq!(with(|s| s.wins[0].id.clone()).unwrap(), b);
        // Back: the sheet it left kept its live text.
        flip_by(hwnd, -1);
        assert_eq!(with(|s| (s.wins[0].id.clone(), s.wins[0].text.clone())).unwrap(), (a, "a, edited".to_owned()));
        host.shutdown();
    }

    #[test]
    fn the_arrows_flip_and_hidden_ones_take_no_press() {
        let displays = vec![display("A", 0, true, 96)];
        let store = stack_of_three("stack-arrows");
        let (a, b) = (id_with(&store, "a"), id_with(&store, "b"));
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let next = paint::button_rect(Btn::Next, 280.0);
        let (x, y) = client_of(next.x + next.w / 2.0, next.y + next.h / 2.0);
        let pt = screen_point(hwnd, x, y);
        // Hidden (the pointer is away): the strip, so a drag, not a flip.
        assert_eq!(with(|s| target_at(&s.wins[0], pt.x, pt.y)).unwrap(), Target::Header);
        let _ = with(|s| s.wins[0].header = Ease::at(1.0));
        assert_eq!(with(|s| target_at(&s.wins[0], pt.x, pt.y)).unwrap(), Target::Button(Btn::Next));
        left_down(hwnd, x, y);
        left_up(hwnd, x, y);
        assert_eq!(with(|s| s.wins[0].id.clone()).unwrap(), b, "› is next");
        let prev = paint::button_rect(Btn::Prev, 280.0);
        let (x, y) = client_of(prev.x + prev.w / 2.0, prev.y + prev.h / 2.0);
        left_down(hwnd, x, y);
        left_up(hwnd, x, y);
        assert_eq!(with(|s| s.wins[0].id.clone()).unwrap(), a, "‹ is previous");
        host.shutdown();
    }

    #[test]
    fn a_stack_leans_by_its_own_id_and_flipping_keeps_the_lean() {
        let displays = vec![display("A", 0, true, 96)];
        let store = stack_of_three("stack-tilt");
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(store.list(), &displays);
        c.prefs.tilt = true;
        host.sync(&c);
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let lean = with(|s| s.wins[0].lean()).unwrap();
        assert_eq!(lean, art::tilt_deg("s1"), "the stack id, not a sheet's");
        flip_by(hwnd, 1);
        flip_by(hwnd, 1);
        assert_eq!(with(|s| s.wins[0].lean()).unwrap(), lean, "a flip does not change it");
        host.shutdown();
    }

    #[test]
    fn the_cap_counts_every_sheet_of_every_stack() {
        let displays = vec![display("A", 0, true, 96)];
        let mut sheets: Vec<(String, Sticky)> = (0..3)
            .map(|k| {
                (
                    format!("s{k}"),
                    Sticky {
                        stack: Some("big".into()),
                        stack_order: k,
                        stack_top: k == 0,
                        monitor: "A".into(),
                        ..Sticky::default()
                    },
                )
            })
            .collect();
        for k in 0..17 {
            sheets.push((format!("p{k}"), sticky("A", DipRect::new(10.0 * k as f32, 0.0, 240.0, 208.0))));
        }
        let store = store_of("stack-cap", sheets.iter().map(|(t, s)| (t.as_str(), s.clone())).collect());
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        assert_eq!(with(|s| s.wins.len()).unwrap(), 18, "18 windows");
        assert_eq!(host.count(), 20, "20 sheets");
        assert!(with(|s| !sticky::can_add(sheet_total(&s.wins))).unwrap(), "full");
        host.shutdown();
    }

    #[test]
    fn a_stack_comes_apart_into_windows_and_a_note_joins_without_losing_its_window() {
        let displays = vec![display("A", 0, true, 96)];
        let mut store = stack_of_three("stack-apart");
        let (a, b) = (id_with(&store, "a"), id_with(&store, "b"));
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        let stack_hwnd = with(|s| s.wins[0].hwnd).unwrap();
        // "a" is taken out: the stack keeps its window (b and c), a gets one.
        let out = apply(&mut store, vec![WidgetEvent::StackOut { id: a.clone() }], 100);
        assert!(out.structural && out.unstack == vec![a.clone()]);
        let displays_for_apply = displays.clone();
        crate::widgets::apply::stack_out(&mut store, &displays_for_apply, &a);
        host.sync(&ctx(store.list(), &displays));
        assert_eq!(with(|s| s.wins.len()).unwrap(), 2);
        assert_eq!(host.count(), 3);
        let stack_win = with(|s| s.wins.iter().find(|w| w.stack.is_some()).map(|w| (w.hwnd, w.sheet_count(), w.id.clone()))).unwrap().unwrap();
        assert_eq!((stack_win.0, stack_win.1), (stack_hwnd, 2), "the stack's window stays");
        assert_ne!(stack_win.2, a);
        let plain = with(|s| s.wins.iter().find(|w| w.stack.is_none()).map(|w| (w.id.clone(), w.sticky.rect))).unwrap().unwrap();
        assert_eq!(plain.0, a);
        assert!(plain.1.x > 400.0 + 280.0, "beside the stack, to its right: {:?}", plain.1);
        // The next sheet in order shows: b.
        assert_eq!(stack_win.2, b);
        host.shutdown();
    }

    #[test]
    fn a_hidden_sheets_id_reaches_its_units_window_for_the_cat_a_raise_and_focus() {
        let _focus = keyboard_lock();
        let displays = vec![display("A", 0, true, 96)];
        let mut sheets: Vec<(&str, Sticky)> = Vec::new();
        for (k, t) in ["a", "b", "c"].into_iter().enumerate() {
            sheets.push((
                t,
                Sticky {
                    stack: Some("s1".into()),
                    stack_order: k as u32,
                    stack_top: k == 0,
                    monitor: "A".into(),
                    rect: DipRect::new(400.0, 200.0, 280.0, 208.0),
                    remind_at: (t == "c").then_some(5),
                    remind_fired: t == "c",
                    ..Sticky::default()
                },
            ));
        }
        let store = store_of("stack-hidden", sheets);
        let (a, c) = (id_with(&store, "a"), id_with(&store, "c"));
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        // "c" is hidden under "a" and overdue: it owns the cat, in this window.
        assert_eq!(with(|s| s.peek.clone()).unwrap().as_deref(), Some(c.as_str()));
        assert!(with(|s| s.wins[0].has(&c) && s.wins[0].id == a).unwrap());
        assert!(with(|s| s.wins[0].any_overdue(1_000)).unwrap());
        assert_eq!(with(|s| win_mood(&s.wins[0], 1_000)).unwrap(), Mood::Clock);
        // A raise by the hidden sheet's id raises the unit's window.
        host.raise(std::slice::from_ref(&c));
        assert!(with(|s| s.wins[0].raised).unwrap());
        // Buka on the hidden sheet brings it to the top and gives it the keyboard.
        host.focus(&c);
        assert_eq!(with(|s| s.wins[0].id.clone()).unwrap(), c, "the hidden sheet came to the top");
        host.shutdown();
    }

    // ------------------------------------------------------------ tuck

    fn tucked_note(id: &str, edge: Edge, y: f32) -> Note {
        Note {
            id: id.to_owned(),
            text: "Nomor penting".into(),
            updated: 1,
            sticky: Some(Sticky {
                monitor: "A".into(),
                rect: DipRect::new(if edge == Edge::Right { 1680.0 } else { 0.0 }, y, 240.0, 208.0),
                tuck: Some(edge),
                ..Sticky::default()
            }),
        }
    }

    /// A client point over the tab of window 0 (tucked right, all the way in).
    fn on_the_tab() -> (i32, i32) {
        with(|s| {
            let w = &s.wins[0];
            let m = w.margins();
            (
                ((m.0 + w.view.w - tuck::TAB_W / 2.0) * w.scale) as i32,
                ((m.1 + 12.0) * w.scale) as i32,
            )
        })
        .unwrap()
    }

    #[test]
    fn a_tucked_note_is_a_tab_at_its_edge_until_the_pointer_comes() {
        let displays = vec![display("A", 0, true, 96)];
        for animate in [true, false] {
            let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
            let mut host = WidgetHost::new(HWND::default());
            let mut c = ctx(&notes, &displays);
            c.animate = animate;
            host.sync(&c);
            let now = Instant::now();
            let (hwnd, win_rect, k) = with(|s| (s.wins[0].hwnd, s.wins[0].note, s.wins[0].scale)).unwrap();
            assert_eq!(win_rect.right(), 1920, "the window stays at the out rect, flush right");
            assert_eq!(with(|s| s.wins[0].pop.progress(now)).unwrap(), 0.0, "only the tab");
            assert!(!host.moving(now), "at rest nothing runs");
            // The pointer comes onto the tab: the note pops out.
            let (x, y) = on_the_tab();
            let _ = host.drain();
            mouse_move(hwnd, x, y);
            assert!(with(|s| s.wins[0].pointer_in).unwrap());
            assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 1.0);
            let _ = k;
            if animate {
                assert!(host.moving(Instant::now()), "the slide runs the frame clock");
                assert!(host.drain().contains(&WidgetEvent::NeedsClock));
                assert!(with(|s| s.wins[0].pop.progress(Instant::now())).unwrap() < 1.0);
            } else {
                assert!(!host.moving(Instant::now()), "instant: no clock");
                assert_eq!(with(|s| s.wins[0].pop.progress(Instant::now())).unwrap(), 1.0);
            }
            // Settled: the clock stops, the note is out.
            let later = Instant::now() + Duration::from_millis(400);
            host.tick(later);
            assert!(!host.moving(later), "settled (animate {animate})");
            assert_eq!(with(|s| s.wins[0].pop.progress(later)).unwrap(), 1.0);
            host.shutdown();
        }
    }

    #[test]
    fn a_note_that_is_out_waits_the_grace_then_shrinks_back_and_re_entering_cancels() {
        let displays = vec![display("A", 0, true, 96)];
        for animate in [true, false] {
            let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
            let mut host = WidgetHost::new(HWND::default());
            let mut c = ctx(&notes, &displays);
            c.animate = animate;
            host.sync(&c);
            let hwnd = with(|s| s.wins[0].hwnd).unwrap();
            let (x, y) = on_the_tab();
            mouse_move(hwnd, x, y);
            let out = Instant::now() + Duration::from_millis(400);
            host.tick(out);
            assert_eq!(with(|s| s.wins[0].pop.progress(out)).unwrap(), 1.0);
            let _ = host.drain();
            // The pointer leaves: nothing moves, the grace runs on the frame clock.
            mouse_leave(hwnd);
            let left = Instant::now();
            assert!(with(|s| s.wins[0].grace.is_some()).unwrap());
            assert!(host.moving(left), "the grace counts (animate {animate})");
            assert!(host.drain().contains(&WidgetEvent::NeedsClock));
            // 100 ms later it is still out, and nothing was drawn for it.
            host.tick(left + Duration::from_millis(100));
            assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 1.0);
            // The pointer comes back in time: the grace is off.
            move_to_client(hwnd, x, y);
            assert!(with(|s| s.wins[0].grace.is_none()).unwrap(), "re-entering cancels it");
            assert!(!host.moving(left + Duration::from_millis(500)), "nothing waits");
            // Leave again, and let it run out.
            mouse_leave(hwnd);
            let t0 = Instant::now();
            host.tick(t0 + Duration::from_millis(250));
            assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 1.0, "300 ms is not over");
            let after = t0 + Duration::from_millis(330);
            host.tick(after);
            assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 0.0, "the grace is over: back in");
            let end = after + Duration::from_millis(400);
            if animate {
                assert!(host.moving(after), "the slide runs");
                host.tick(end);
            } else {
                assert!(!host.moving(after), "reduced motion: the slide is instant, the grace still applied");
            }
            assert!(!host.moving(end));
            assert_eq!(with(|s| s.wins[0].pop.progress(end)).unwrap(), 0.0, "only the tab again");
            host.shutdown();
        }
    }

    /// `mouse_move` for a client point (the window has not moved).
    fn move_to_client(hwnd: HWND, x: i32, y: i32) {
        mouse_move(hwnd, x, y);
    }

    #[test]
    fn a_tucked_note_stays_out_while_it_has_the_keyboard_is_dragged_or_raised() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        // The keyboard (Buka / focus): out, and it stays when the pointer is
        // away. A stand-in for the hidden EDIT: real focus can be taken by
        // another test's window.
        let _ = with(|s| {
            s.wins[0].edit = Some(fake_edit());
            tuck_settle(s, 0);
        });
        assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 1.0, "the keyboard pops it out");
        mouse_leave(hwnd);
        assert!(with(|s| s.wins[0].grace.is_none()).unwrap(), "the keyboard keeps it out");
        end_editing(hwnd);
        assert!(with(|s| s.wins[0].grace.is_some()).unwrap(), "now nothing does: the grace starts");
        // A due reminder (raise) pops it out and keeps it out until it ends.
        let _ = with(|s| {
            s.wins[0].grace = None;
            s.wins[0].pop = tuck::Pop::at(0.0);
        });
        host.raise(&["n1".to_owned()]);
        assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 1.0);
        mouse_leave(hwnd);
        assert!(with(|s| s.wins[0].grace.is_none()).unwrap(), "raised: stays out");
        lower(hwnd);
        assert!(with(|s| s.wins[0].grace.is_some()).unwrap(), "the raise ended: the grace starts");
        host.shutdown();
    }

    #[test]
    fn a_drag_ended_pressing_an_outer_edge_tucks_and_one_ended_elsewhere_untucks() {
        let _shift = hold_shift(false);
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(1300.0, 200.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let (x, y) = client_of(20.0, 10.0);
        let _ = host.drain();
        left_down(hwnd, x, y);
        // Pressed against the right edge of the screen: the accent bar shows.
        move_to(hwnd, 1919, 250);
        assert_eq!(with(|s| s.wins[0].cue).unwrap(), Some(Edge::Right), "the cue");
        // Pulled back off the edge: no cue.
        move_to(hwnd, 1700, 250);
        assert_eq!(with(|s| s.wins[0].cue).unwrap(), None);
        move_to(hwnd, 1919, 250);
        assert_eq!(with(|s| s.wins[0].cue).unwrap(), Some(Edge::Right));
        let (ox, oy) = with(|s| s.wins[0].origin()).unwrap();
        left_up(hwnd, 1919 - ox, 250 - oy);
        let events = host.drain();
        let placed = events.iter().find_map(|e| match e {
            WidgetEvent::Placed { rect, .. } => Some(*rect),
            _ => None,
        });
        let rect = placed.expect("placed");
        assert_eq!(rect.right(), 1920.0, "flush with the edge");
        assert!(
            events.contains(&WidgetEvent::Tuck { id: "n1".into(), edge: Some(Edge::Right) }),
            "{events:?}"
        );
        assert_eq!(with(|s| s.wins[0].sticky.tuck).unwrap(), Some(Edge::Right));
        assert!(with(|s| s.wins[0].cue.is_none()).unwrap());
        // The pointer is on it: it stays out. Away, and after the grace it is a tab.
        mouse_leave(hwnd);
        let later = Instant::now() + Duration::from_millis(400);
        host.tick(later);
        assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 0.0);
        // A press on the tab starts a drag with the note out at once; dropped
        // away from the edge it is out in the open where it was dropped.
        let (tx, ty) = on_the_tab();
        let _ = with(|s| s.wins[0].pop = tuck::Pop::at(0.0));
        let _ = host.drain();
        left_down(hwnd, tx, ty);
        assert!(with(|s| s.wins[0].drag.is_some()).unwrap(), "the press on the tab drags");
        assert_eq!(with(|s| s.wins[0].pop.progress(Instant::now())).unwrap(), 1.0, "shown out at once");
        move_to(hwnd, 900, 400);
        let (ox, oy) = with(|s| s.wins[0].origin()).unwrap();
        left_up(hwnd, 900 - ox, 400 - oy);
        let events = host.drain();
        assert!(
            events.contains(&WidgetEvent::Tuck { id: "n1".into(), edge: None }),
            "{events:?}"
        );
        assert_eq!(with(|s| s.wins[0].sticky.tuck).unwrap(), None);
        let rect = events
            .iter()
            .find_map(|e| match e {
                WidgetEvent::Placed { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("placed where dropped");
        assert!(rect.x < 1500.0, "not at the edge any more: {rect:?}");
        host.shutdown();
    }

    #[test]
    fn tucking_by_a_drag_release_ends_with_the_scale_in_after_the_grace() {
        let _shift = hold_shift(false);
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(1300.0, 200.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.animate = true;
        host.sync(&c);
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let (x, y) = client_of(20.0, 10.0);
        let _ = host.drain();
        left_down(hwnd, x, y);
        move_to(hwnd, 1919, 250);
        assert_eq!(with(|s| s.wins[0].cue).unwrap(), Some(Edge::Right), "the cue");
        let (ox, oy) = with(|s| s.wins[0].origin()).unwrap();
        left_up(hwnd, 1919 - ox, 250 - oy);
        let events = host.drain();
        assert!(
            events.contains(&WidgetEvent::Tuck { id: "n1".into(), edge: Some(Edge::Right) }),
            "{events:?}"
        );
        // Dropped on the edge it is still out (the pointer is on it); once the
        // pointer leaves, the grace runs, and it keeps its edge.
        assert_eq!(with(|s| s.wins[0].sticky.tuck).unwrap(), Some(Edge::Right));
        assert_eq!(with(|s| s.wins[0].pop_edge).unwrap(), Some(Edge::Right));
        assert!(with(|s| s.wins[0].grace.is_none()).unwrap(), "the pointer is on it");
        mouse_leave(hwnd);
        let p = pop_of(0);
        assert_eq!((p.to, p.ms), (1.0, 0.0), "out, nothing running");
        assert!(with(|s| s.wins[0].grace.is_some()).unwrap(), "the grace runs");
        let now = Instant::now();
        assert!(host.moving(now), "the grace runs on the frame clock");
        // After the grace: the 200 ms scale-in toward the tab.
        let after = now + Duration::from_millis(400);
        host.tick(after);
        let p = pop_of(0);
        assert_eq!((p.to, p.ms), (0.0, tuck::IN_MS), "200 ms scale-in");
        assert_eq!(with(|s| s.wins[0].pop_edge).unwrap(), Some(Edge::Right));
        assert!(host.moving(after), "running");
        assert!(p.scale(p.start + Duration::from_millis(100)) < 1.0, "the scale goes down");
        assert!(p.scale(p.start + Duration::from_millis(100)) < p.scale(p.start + Duration::from_millis(50)));
        let end = p.start + Duration::from_millis(300);
        host.tick(end);
        assert!(!host.moving(end));
        assert_eq!(look_of(&p, end), [tuck::MIN_SCALE, 0.0, 1.0], "settles at the tab");
        host.shutdown();
    }

    #[test]
    fn a_tucked_stack_keeps_room_for_its_sheet_edges() {
        let displays = vec![display("A", 0, true, 96)];
        let store = stack_of_three("stack-tuck-room");
        let mut notes = store.list().to_vec();
        for n in &mut notes {
            if let Some(s) = n.sticky.as_mut() {
                s.tuck = Some(Edge::Right);
                s.rect = DipRect::new(1640.0, 200.0, 280.0, 208.0);
            }
        }
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let (room, view, edges) = with(|s| (s.wins[0].room, s.wins[0].view, s.wins[0].stack_edges())).unwrap();
        assert_eq!(edges, 2, "three sheets: two edges peek out");
        assert_eq!(room, tuck::room(view.w, view.h, Edge::Right, 2));
        assert!(room.3 > tuck::room(view.w, view.h, Edge::Right, 0).3, "more room below than a plain note");
        host.shutdown();
    }

    #[test]
    fn a_drag_never_tucks_on_an_inner_edge() {
        let _shift = hold_shift(false);
        // B sits against A's right edge.
        let displays = vec![display("A", 0, true, 96), display("B", 1920, false, 96)];
        let notes = vec![note("n1", DipRect::new(1300.0, 200.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let (x, y) = client_of(20.0, 10.0);
        left_down(hwnd, x, y);
        move_to(hwnd, 1919, 250);
        assert_eq!(with(|s| s.wins[0].cue).unwrap(), None, "the right edge of A is an inner one");
        let (ox, oy) = with(|s| s.wins[0].origin()).unwrap();
        left_up(hwnd, 1919 - ox, 250 - oy);
        let events = host.drain();
        assert!(!events.iter().any(|e| matches!(e, WidgetEvent::Tuck { .. })), "{events:?}");
        assert_eq!(with(|s| s.wins[0].sticky.tuck).unwrap(), None);
        // Its left edge (the primary's left) is outer: it tucks there.
        let (x, y) = client_of(20.0, 10.0);
        left_down(hwnd, x, y);
        move_to(hwnd, 0, 250);
        assert_eq!(with(|s| s.wins[0].cue).unwrap(), Some(Edge::Left));
        let (ox, oy) = with(|s| s.wins[0].origin()).unwrap();
        left_up(hwnd, -ox, 250 - oy);
        assert_eq!(with(|s| s.wins[0].sticky.tuck).unwrap(), Some(Edge::Left));
        host.shutdown();
    }

    #[test]
    fn the_menu_tucks_to_the_nearer_outer_edge_and_brings_a_tucked_note_out() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(1300.0, 200.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        assert_eq!(with(|s| menu_edge_of(s, 0)).unwrap(), Some(Edge::Right), "the nearer side of its centre");
        let _ = host.drain();
        let _ = with(|s| set_tuck(s, 0, Some(Edge::Right)));
        let events = host.drain();
        assert!(events.contains(&WidgetEvent::Tuck { id: "n1".into(), edge: Some(Edge::Right) }));
        let rect = events
            .iter()
            .find_map(|e| match e {
                WidgetEvent::Placed { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("placed at the out rect");
        assert_eq!((rect.x, rect.y), (1680.0, 200.0));
        assert_eq!(with(|s| s.wins[0].note.right()).unwrap(), 1920);
        // Brought out: untucked, in place, out.
        let _ = with(|s| set_tuck(s, 0, None));
        assert_eq!(with(|s| s.wins[0].sticky.tuck).unwrap(), None);
        assert_eq!(with(|s| s.wins[0].pop.progress(Instant::now())).unwrap(), 1.0);
        // Two displays side by side: A's right side is inner, so the left one.
        let both = vec![display("A", 0, true, 96), display("B", 1920, false, 96)];
        host.displays_changed(&both);
        assert_eq!(with(|s| menu_edge_of(s, 0)).unwrap(), Some(Edge::Left));
        host.shutdown();
    }

    #[test]
    fn a_tucked_notes_tab_is_hit_but_the_rest_of_its_window_is_not_the_note() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let (x, y) = on_the_tab();
        let pt = {
            let hwnd = with(|s| s.wins[0].hwnd).unwrap();
            screen_point(hwnd, x, y)
        };
        assert_ne!(with(|s| target_at(&s.wins[0], pt.x, pt.y)).unwrap(), Target::Outside);
        // Where the note would be if it were out (its left part): nothing, the
        // pixels there are transparent and the note is drawn off the edge.
        let far = with(|s| {
            let w = &s.wins[0];
            (w.note.x + 20, w.note.y + 12)
        })
        .unwrap();
        // That point is in the window but the note is shifted 224 dip right: it
        // maps outside of it.
        assert_eq!(with(|s| target_at(&s.wins[0], far.0, far.1)).unwrap(), Target::Outside);
        host.shutdown();
    }

    #[test]
    fn a_tucked_note_is_not_a_snap_neighbour() {
        let _shift = hold_shift(false);
        let displays = vec![display("A", 0, true, 96)];
        // A tab flush left (its out rect is 0..240 wide) and a note to drag.
        let notes = vec![
            tucked_note("t1", Edge::Left, 100.0),
            note("n2", DipRect::new(500.0, 100.0, 240.0, 208.0)),
        ];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let i2 = win_of("n2");
        let hwnd = with(|s| s.wins[i2].hwnd).unwrap();
        let (x, y) = client_of_win(i2, 20.0, 10.0);
        left_down(hwnd, x, y);
        // To x = 255: were the tab a neighbour the note would snap to its
        // right edge + 12 = 252; with none it goes to the grid, 256.
        move_to(hwnd, 255 + 20, 110);
        let rect = with(|s| s.wins[i2].sticky.rect).unwrap();
        assert_eq!(rect.x, 256.0, "no pull from a tab: {rect:?}");
        let (ox, oy) = with(|s| s.wins[i2].origin()).unwrap();
        left_up(hwnd, 275 - ox, 110 - oy);
        host.shutdown();
    }

    /// `client_of` for window `i`.
    fn client_of_win(i: usize, x: f32, y: f32) -> (i32, i32) {
        with(|s| {
            let w = &s.wins[i];
            let m = w.margins();
            (((m.0 + x) * w.scale).round() as i32, ((m.1 + y) * w.scale).round() as i32)
        })
        .unwrap()
    }

    #[test]
    fn the_stack_menu_item_and_the_tuck_item_are_the_documented_commands() {
        assert_eq!((CMD_UNSTACK, CMD_TUCK), (24, 25));
        assert_eq!(CMD_ROLL, 21);
    }

    // ------------------------------------------------ Run B finish: interplay

    #[test]
    fn the_menu_ids_are_distinct_and_the_documented_ones() {
        assert_eq!((CMD_UNSTACK, CMD_TUCK, CMD_SEND_PHONE), (24, 25, 26));
        let all = [
            CMD_PIN,
            CMD_ROLL,
            CMD_UNSTICK,
            CMD_DELETE,
            CMD_UNSTACK,
            CMD_TUCK,
            CMD_SEND_PHONE,
            CMD_REMIND_CUSTOM,
            CMD_REMIND_CLEAR,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // The ranges (paper 1.., remind 30.., stamp 50..) hold none of them.
        let ranges = [
            (CMD_PAPER, CMD_PAPER + Paper::ALL.len()),
            (CMD_REMIND, CMD_REMIND + Preset::ALL.len()),
            (CMD_STAMP, CMD_STAMP + Stamp::ALL.len()),
        ];
        for id in all {
            for (lo, hi) in ranges {
                assert!(!(lo..hi).contains(&id), "{id} in {lo}..{hi}");
            }
        }
    }

    #[test]
    fn a_drop_on_a_tucked_note_brings_it_out_and_appends_to_that_note() {
        let displays = vec![display("A", 0, true, 96)];
        for animate in [true, false] {
            let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
            let mut host = WidgetHost::new(HWND::default());
            let mut c = ctx(&notes, &displays);
            c.animate = animate;
            host.sync(&c);
            let hwnd = with(|s| s.wins[0].hwnd).unwrap();
            assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 0.0, "only the tab");
            // A drag arrives over the tab: the note comes out like under a hover.
            drop_hover(hwnd, true);
            assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 1.0, "out (animate {animate})");
            assert!(with(|s| s.wins[0].grace.is_none()).unwrap());
            // The drop appends to that note.
            let _ = host.drain();
            assert!(drop_into(hwnd, &lines(&["https://example.com"])));
            assert_eq!(
                with(|s| s.wins[0].text.clone()).unwrap(),
                "Nomor penting\nhttps://example.com"
            );
            // The drag is over (a drop clears the outline the same way as a
            // leave): the grace starts, then it is a tab again. Nothing stuck.
            drop_hover(hwnd, false);
            assert!(!with(|s| s.wins[0].drop_hover).unwrap());
            assert!(with(|s| s.wins[0].grace.is_some()).unwrap(), "the grace runs");
            let t0 = Instant::now();
            host.tick(t0 + Duration::from_millis(400));
            assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 0.0, "back in");
            let end = t0 + Duration::from_millis(900);
            host.tick(end);
            assert!(!host.moving(end), "nothing is left running");
            assert_eq!(with(|s| s.wins[0].pop.progress(end)).unwrap(), 0.0);
            host.shutdown();
        }
    }

    #[test]
    fn a_drag_that_leaves_a_tucked_note_without_a_drop_lets_it_shrink_back() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![tucked_note("n1", Edge::Left, 100.0)];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.animate = false;
        host.sync(&c);
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        drop_hover(hwnd, true);
        assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 1.0);
        // Two enters in a row (a second DragEnter) change nothing more.
        drop_hover(hwnd, true);
        drop_hover(hwnd, false);
        let t0 = Instant::now();
        host.tick(t0 + Duration::from_millis(400));
        assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 0.0, "a tab again, no drop needed");
        assert!(!host.moving(t0 + Duration::from_millis(900)));
        host.shutdown();
    }

    #[test]
    fn a_drop_on_a_stack_lands_in_the_shown_sheet_not_the_first() {
        let displays = vec![display("A", 0, true, 96)];
        let mut store = stack_of_three("stack-drop");
        let (a, b, c) = (id_with(&store, "a"), id_with(&store, "b"), id_with(&store, "c"));
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        assert_eq!(with(|s| s.wins[0].id.clone()).unwrap(), a, "sheet 1 is shown");
        // Flip to the third sheet, then drop.
        let _ = with(|s| show_sheet(s, 0, 2));
        assert_eq!(with(|s| s.wins[0].id.clone()).unwrap(), c);
        let _ = host.drain();
        assert!(drop_into(hwnd, &lines(&["C:\\x.txt"])));
        let events = host.drain();
        assert!(
            events.contains(&WidgetEvent::Text {
                id: c.clone(),
                text: "c\nC:\\x.txt".into(),
            }),
            "{events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(e, WidgetEvent::Text { id, .. } if *id == a || *id == b)),
            "no other sheet gets text: {events:?}"
        );
        // Applied: only the shown sheet changed.
        let out = apply(&mut store, events, 100);
        assert!(out.changed);
        let text = |id: &str| store.get(id).unwrap().text.clone();
        assert_eq!((text(&a), text(&b), text(&c)), ("a".into(), "b".into(), "c\nC:\\x.txt".into()));
        // Flip back: sheet c keeps what the drop added in the window's copy.
        let _ = with(|s| show_sheet(s, 0, 0));
        let _ = with(|s| show_sheet(s, 0, 2));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "c\nC:\\x.txt");
        host.shutdown();
    }

    #[test]
    fn send_to_phone_on_a_stack_sends_the_sheet_the_window_shows() {
        let displays = vec![display("A", 0, true, 96)];
        let store = stack_of_three("stack-phone");
        let (a, b) = (id_with(&store, "a"), id_with(&store, "b"));
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let _ = host.drain();
        send_to_phone(hwnd);
        assert_eq!(host.drain(), vec![WidgetEvent::SendToPhone { id: a }], "the shown one at first");
        let _ = with(|s| show_sheet(s, 0, 1));
        let _ = host.drain();
        send_to_phone(hwnd);
        let events = host.drain();
        assert!(events.contains(&WidgetEvent::SendToPhone { id: b.clone() }), "{events:?}");
        let sent: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, WidgetEvent::SendToPhone { .. }))
            .collect();
        assert_eq!(sent.len(), 1);
        host.shutdown();
    }

    #[test]
    fn the_drop_target_is_registered_once_per_window_and_a_stack_change_keeps_it() {
        let displays = vec![display("A", 0, true, 96)];
        let mut store = stack_of_three("stack-register");
        let a = id_with(&store, "a");
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(store.list(), &displays));
        assert_eq!(drop::live_targets(), 1, "one window for the whole stack");
        // A sheet leaves: a second window, a second target; the stack's own
        // window keeps the one it has.
        let stack_hwnd = with(|s| s.wins[0].hwnd).unwrap();
        crate::widgets::apply::stack_out(&mut store, &displays, &a);
        host.sync(&ctx(store.list(), &displays));
        assert_eq!(with(|s| s.wins.len()).unwrap(), 2);
        assert_eq!(drop::live_targets(), 2);
        assert!(with(|s| s.wins.iter().any(|w| w.hwnd == stack_hwnd)).unwrap());
        // Syncing again changes nothing.
        host.sync(&ctx(store.list(), &displays));
        assert_eq!(drop::live_targets(), 2);
        host.shutdown();
        assert_eq!(drop::live_targets(), 0, "every window revoked");
    }

    #[test]
    fn the_tab_and_the_pop_keep_the_frame_clock_off_when_nothing_moves() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![tucked_note("n1", Edge::Left, 100.0), note("n2", DipRect::new(500.0, 100.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.animate = true;
        host.sync(&c);
        assert!(!host.moving(Instant::now()), "a tab at rest and a note at rest cost nothing");
        host.shutdown();
    }

    // ------------------------------------------------ the pop (scale and fade)

    /// Window `i`'s pop, as a copy.
    fn pop_of(i: usize) -> tuck::Pop {
        with(|s| s.wins[i].pop).unwrap()
    }

    /// The scale and both opacities of a pop at `t`.
    fn look_of(p: &tuck::Pop, t: Instant) -> [f32; 3] {
        [p.scale(t), p.note_alpha(t), p.tab_alpha(t)]
    }

    #[test]
    fn hovering_the_tab_grows_the_note_out_of_it() {
        let displays = vec![display("A", 0, true, 96)];
        for animate in [true, false] {
            let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
            let mut host = WidgetHost::new(HWND::default());
            let mut c = ctx(&notes, &displays);
            c.animate = animate;
            host.sync(&c);
            let hwnd = with(|s| s.wins[0].hwnd).unwrap();
            let now = Instant::now();
            let p = pop_of(0);
            assert_eq!(look_of(&p, now), [tuck::MIN_SCALE, 0.0, 1.0], "only the tab");
            let (x, y) = on_the_tab();
            let _ = host.drain();
            mouse_move(hwnd, x, y);
            let p = pop_of(0);
            assert_eq!(p.to, 1.0);
            if animate {
                assert_eq!(p.ms, tuck::OUT_MS, "300 ms");
                assert!(host.moving(Instant::now()), "running on the frame clock");
                assert!(host.drain().contains(&WidgetEvent::NeedsClock));
                // It grows from the tab: bigger and more opaque as it goes.
                let a = look_of(&p, p.start + Duration::from_millis(50));
                let b = look_of(&p, p.start + Duration::from_millis(150));
                assert!(a[0] > tuck::MIN_SCALE && b[0] > a[0] && b[0] < 1.05, "{a:?} {b:?}");
                assert!(b[1] > a[1] && b[2] < a[2], "the note fades in as the tab fades out");
                // Past the end it is whole, and the clock stops.
                let later = p.start + Duration::from_millis(400);
                host.tick(later);
                assert!(!host.moving(later));
                assert_eq!(look_of(&p, later), [1.0, 1.0, 0.0]);
            } else {
                assert!(!host.moving(Instant::now()), "instant: no clock");
                assert_eq!(look_of(&p, Instant::now()), [1.0, 1.0, 0.0]);
            }
            host.shutdown();
        }
    }

    #[test]
    fn a_note_that_leaves_waits_the_grace_then_shrinks_into_its_tab() {
        let displays = vec![display("A", 0, true, 96)];
        for animate in [true, false] {
            let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
            let mut host = WidgetHost::new(HWND::default());
            let mut c = ctx(&notes, &displays);
            c.animate = animate;
            host.sync(&c);
            let hwnd = with(|s| s.wins[0].hwnd).unwrap();
            let (x, y) = on_the_tab();
            mouse_move(hwnd, x, y);
            host.tick(Instant::now() + Duration::from_millis(400));
            mouse_leave(hwnd);
            let left = Instant::now();
            // Nothing is drawn during the grace.
            host.tick(left + Duration::from_millis(100));
            assert!(!with(|s| s.wins[0].dirty).unwrap(), "no frame during the grace");
            assert_eq!(pop_of(0).to, 1.0);
            let after = left + Duration::from_millis(330);
            host.tick(after);
            let p = pop_of(0);
            assert_eq!(p.to, 0.0, "the grace is over");
            assert_eq!(with(|s| s.wins[0].pop_edge).unwrap(), Some(Edge::Right));
            if animate {
                assert_eq!(p.ms, tuck::IN_MS, "200 ms");
                assert!(host.moving(after));
                let a = p.scale(p.start + Duration::from_millis(60));
                let b = p.scale(p.start + Duration::from_millis(140));
                assert!(a < 1.0 && b < a, "shrinking: {a} then {b}");
                let end = p.start + Duration::from_millis(250);
                assert_eq!(look_of(&p, end), [tuck::MIN_SCALE, 0.0, 1.0], "only the tab");
                host.tick(end);
                assert!(!host.moving(end));
            } else {
                assert!(!host.moving(after), "instant");
                assert_eq!(look_of(&p, after), [tuck::MIN_SCALE, 0.0, 1.0]);
            }
            host.shutdown();
        }
    }

    #[test]
    fn re_entering_part_way_reverses_without_a_jump() {
        let displays = vec![display("A", 0, true, 96)];
        for coming_in in [true, false] {
            let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
            let mut host = WidgetHost::new(HWND::default());
            let mut c = ctx(&notes, &displays);
            c.animate = true;
            host.sync(&c);
            let hwnd = with(|s| s.wins[0].hwnd).unwrap();
            let (x, y) = on_the_tab();
            // Half way back in (the pointer comes back onto the tab), or part
            // way out and turned back in because the pointer is gone.
            let start = Instant::now() - Duration::from_millis(100);
            let _ = with(|s| {
                s.wins[0].pop = if coming_in {
                    tuck::Pop { from: 1.0, to: 0.0, from_scale: 1.0, start, ms: tuck::IN_MS }
                } else {
                    tuck::Pop { from: 0.0, to: 1.0, from_scale: tuck::MIN_SCALE, start, ms: tuck::OUT_MS }
                };
            });
            let old = pop_of(0);
            if coming_in {
                mouse_move(hwnd, x, y);
                assert_eq!(pop_of(0).to, 1.0, "back out");
            } else {
                let _ = with(|s| {
                    s.wins[0].pointer_in = false;
                    s.wins[0].grace = Some(Instant::now() - Duration::from_millis(400));
                });
                host.tick(Instant::now());
                assert_eq!(pop_of(0).to, 0.0, "back in");
            }
            // The two pops compared at the moment of the reversal.
            let new = pop_of(0);
            let (before, after) = (look_of(&old, new.start), look_of(&new, new.start));
            for k in 0..3 {
                assert!((before[k] - after[k]).abs() < 1e-3, "{before:?} {after:?}");
            }
            host.shutdown();
        }
    }

    /// The desktop px where window `i`'s note point `pt` is drawn when the pop
    /// is at `at` (the note tucked right).
    fn drawn_px(i: usize, pt: (f32, f32), at: Instant) -> (i32, i32) {
        with(|s| {
            let w = &s.wins[i];
            let h = shown_h(w.view.h, w.sticky.rolled);
            let o = tuck::origin(Edge::Right, w.view.w, h);
            let d = tuck::scale_about(pt, o, w.pop.scale(at));
            (
                w.note.x + (d.0 * w.scale).round() as i32,
                w.note.y + (d.1 * w.scale).round() as i32,
            )
        })
        .unwrap()
    }

    #[test]
    fn a_click_part_way_through_the_pop_lands_on_the_part_it_shows() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.animate = true;
        host.sync(&c);
        // Frozen part way: half out at a scale of 0.6 (the trip is very long).
        let at = Instant::now();
        let part = tuck::Pop { from: 0.5, to: 1.0, from_scale: 0.6, start: at, ms: 1.0e9 };
        let _ = with(|s| s.wins[0].pop = part);
        let t = |pt: (f32, f32)| {
            let (x, y) = drawn_px(0, pt, at);
            with(|s| target_at_in(&s.wins[0], x, y, at)).unwrap()
        };
        assert_eq!(t((100.0, 100.0)), Target::Body);
        assert_eq!(t((20.0, 10.0)), Target::Header);
        assert_eq!(t((237.0, 205.0)), Target::Grip);
        // Left of where the scaled note reaches: not the note.
        let (x, y) = drawn_px(0, (240.0, 100.0), at);
        let left = with(|s| target_at_in(&s.wins[0], x - 150, y, at)).unwrap();
        assert_eq!(left, Target::Outside, "beyond the scaled note's left edge");
        // The tab rect counts as long as the tab shows, though the scaled note
        // does not reach it there (its top is below the tab's top).
        let tab_top = with(|s| (s.wins[0].note.x + 232, s.wins[0].note.y + 3)).unwrap();
        assert_eq!(
            with(|s| target_at_in(&s.wins[0], tab_top.0, tab_top.1, at)).unwrap(),
            Target::Header
        );
        // Client points map back through the inverse scale.
        let (cx, cy) = {
            let (x, y) = drawn_px(0, (100.0, 100.0), at);
            let (ox, oy) = with(|s| s.wins[0].origin()).unwrap();
            (x - ox, y - oy)
        };
        let (dx, dy) = with(|s| client_to_dip(&s.wins[0], cx, cy)).unwrap();
        assert!((dx - 100.0).abs() < 2.0 && (dy - 100.0).abs() < 2.0, "({dx}, {dy})");
        // Only the tab is hit at progress 0, not the rest of the window.
        let _ = with(|s| s.wins[0].pop = tuck::Pop::at(0.0));
        let (x, y) = with(|s| (s.wins[0].note.x + 20, s.wins[0].note.y + 12)).unwrap();
        assert_eq!(with(|s| target_at(&s.wins[0], x, y)).unwrap(), Target::Outside);
        host.shutdown();
    }

    #[test]
    fn a_press_part_way_finishes_the_pop_and_moves_the_note_from_the_header() {
        let _shift = hold_shift(false);
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.animate = true;
        host.sync(&c);
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        // Part way, scale 0.6: a press on the scaled header starts a Move with
        // the note out at once.
        let at = Instant::now();
        let _ = with(|s| {
            s.wins[0].pop = tuck::Pop { from: 0.5, to: 1.0, from_scale: 0.6, start: at, ms: 1.0e9 };
        });
        let (x, y) = drawn_px(0, (20.0, 10.0), at);
        let (ox, oy) = with(|s| s.wins[0].origin()).unwrap();
        left_down(hwnd, x - ox, y - oy);
        assert!(with(|s| s.wins[0].drag.is_some()).unwrap(), "the header drags");
        let p = pop_of(0);
        assert_eq!((p.to, p.ms), (1.0, 0.0), "finished at once");
        assert_eq!(look_of(&p, Instant::now()), [1.0, 1.0, 0.0]);
        left_up(hwnd, x - ox, y - oy);
        host.shutdown();
    }

    #[test]
    fn the_tab_counts_as_the_note_for_the_pointer_while_it_shows() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let tab = with(|s| (s.wins[0].note.x + 230, s.wins[0].note.y + 30)).unwrap();
        let now = Instant::now();
        assert!(with(|s| s.wins[0].on_tab(tab.0, tab.1, now)).unwrap());
        assert!(with(|s| s.wins[0].tab_showing(now)).unwrap());
        // Part way (progress 0.5, the tab is gone from the picture): the tab
        // rect is still part of the note for the pointer, but a press is not
        // "on" the tab.
        let _ = with(|s| {
            s.wins[0].pop = tuck::Pop { from: 0.5, to: 1.0, from_scale: 0.8, start: now, ms: 1.0e9 };
        });
        assert!(!with(|s| s.wins[0].on_tab(tab.0, tab.1, now)).unwrap());
        assert!(with(|s| s.wins[0].tab_showing(now)).unwrap());
        assert_ne!(with(|s| target_at_in(&s.wins[0], tab.0, tab.1, now)).unwrap(), Target::Outside);
        // All the way out: no tab.
        let _ = with(|s| s.wins[0].pop = tuck::Pop::at(1.0));
        assert!(!with(|s| s.wins[0].tab_showing(now)).unwrap());
        assert!(!with(|s| s.wins[0].on_tab(tab.0, tab.1, now)).unwrap());
        host.shutdown();
    }

    #[test]
    fn tucking_through_the_menu_ends_with_the_scale_in_after_the_grace() {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note("n1", DipRect::new(1300.0, 200.0, 240.0, 208.0))];
        let mut host = WidgetHost::new(HWND::default());
        let mut c = ctx(&notes, &displays);
        c.animate = true;
        host.sync(&c);
        let _ = host.drain();
        let _ = with(|s| set_tuck(s, 0, Some(Edge::Right)));
        flush_dirty();
        // It is out (a note that was open) with the grace running, nothing else.
        let p = pop_of(0);
        assert_eq!((p.to, p.ms), (1.0, 0.0));
        assert_eq!(with(|s| s.wins[0].pop_edge).unwrap(), Some(Edge::Right));
        assert!(with(|s| s.wins[0].grace.is_some()).unwrap());
        let now = Instant::now();
        assert!(host.moving(now), "the grace runs on the frame clock");
        let after = now + Duration::from_millis(330);
        host.tick(after);
        let p = pop_of(0);
        assert_eq!((p.to, p.ms), (0.0, tuck::IN_MS), "200 ms scale-in");
        assert!(host.moving(after), "running");
        assert!(p.scale(p.start + Duration::from_millis(100)) < 1.0);
        let end = p.start + Duration::from_millis(300);
        host.tick(end);
        assert!(!host.moving(end));
        assert_eq!(look_of(&p, end), [tuck::MIN_SCALE, 0.0, 1.0], "settles at the tab");
        host.shutdown();
    }

    #[test]
    fn bringing_a_tucked_note_out_pops_it_and_without_animation_it_is_instant() {
        let displays = vec![display("A", 0, true, 96)];
        for animate in [true, false] {
            let notes = vec![tucked_note("n1", Edge::Right, 300.0)];
            let mut host = WidgetHost::new(HWND::default());
            let mut c = ctx(&notes, &displays);
            c.animate = animate;
            host.sync(&c);
            let _ = with(|s| s.wins[0].tilt = true);
            assert_eq!(with(|s| s.wins[0].pop.to).unwrap(), 0.0);
            let _ = host.drain();
            let _ = with(|s| set_tuck(s, 0, None));
            flush_dirty();
            assert_eq!(with(|s| s.wins[0].sticky.tuck).unwrap(), None);
            let now = Instant::now();
            let p = pop_of(0);
            assert_eq!(p.to, 1.0);
            if animate {
                assert!(p.running(now) && host.moving(now), "it pops out");
                assert!(host.drain().contains(&WidgetEvent::NeedsClock));
                // Drawn as the tucked note while it pops: the edge is kept, the
                // window keeps its room, and it does not lean mid-scale.
                assert_eq!(with(|s| s.wins[0].tuck_edge(now)).unwrap(), Some(Edge::Right));
                assert!(with(|s| s.wins[0].room.0 > 0.0).unwrap());
                assert_eq!(with(|s| s.wins[0].lean()).unwrap(), 0.0);
                // Settled: the room is given back, and it leans again.
                std::thread::sleep(Duration::from_millis(340));
                host.tick(Instant::now());
                assert!(!host.moving(Instant::now()));
                assert_eq!(with(|s| s.wins[0].room).unwrap(), (0.0, 0.0, 0.0, 0.0));
                assert_eq!(with(|s| s.wins[0].tuck_edge(Instant::now())).unwrap(), None);
                assert_ne!(with(|s| s.wins[0].lean()).unwrap(), 0.0);
            } else {
                assert!(!p.running(now) && !host.moving(now), "instant");
                assert_eq!(look_of(&p, now), [1.0, 1.0, 0.0]);
                assert_eq!(with(|s| s.wins[0].room).unwrap(), (0.0, 0.0, 0.0, 0.0));
            }
            // A note that is out already: nothing to animate.
            let _ = with(|s| set_tuck(s, 0, Some(Edge::Left)));
            let _ = with(|s| s.wins[0].pop = tuck::Pop::at(1.0));
            let _ = with(|s| set_tuck(s, 0, None));
            assert!(!pop_of(0).running(Instant::now()));
            host.shutdown();
        }
    }

    #[test]
    fn a_tucked_note_has_room_for_its_overshoot_in_the_window() {
        let displays = vec![display("A", 0, true, 96)];
        for edge in [Edge::Right, Edge::Left] {
            let notes = vec![
                tucked_note("n1", edge, 300.0),
                note("n2", DipRect::new(500.0, 100.0, 240.0, 208.0)),
            ];
            let mut host = WidgetHost::new(HWND::default());
            host.sync(&ctx(&notes, &displays));
            let (m, room, size, k, view, bare) = with(|s| {
                let w = &s.wins[0];
                let bare = art::margins_for(w.view.w, w.view.h, 0.0);
                (w.margins(), w.room, w.canvas.as_ref().map(|c| c.size()), w.scale, w.view, bare)
            })
            .unwrap();
            let want = tuck::room(view.w, view.h, edge, 0);
            assert_eq!(room, want);
            assert_eq!(m, (bare.0 + want.0, bare.1 + want.1, bare.2 + want.2, bare.3 + want.3));
            assert!(room.0 + room.2 + room.3 > 0.0, "some room is needed");
            // The paper and its shadow at the peak scale are inside the window.
            let o = tuck::origin(edge, view.w, view.h);
            let (r, h) = (tuck::SHADOW_REACH, view.h);
            let lo = tuck::scale_about((-r.0, -r.1), o, tuck::PEAK_SCALE);
            let hi = tuck::scale_about((view.w + r.2, h + r.3), o, tuck::PEAK_SCALE);
            let (w_px, h_px) = size.expect("drawn");
            assert!((m.0 + lo.0) * k >= 0.0 && (m.1 + lo.1) * k >= 0.0, "{edge:?}");
            assert!(
                ((m.0 + hi.0) * k).ceil() as i32 <= w_px && ((m.1 + hi.1) * k).ceil() as i32 <= h_px,
                "{edge:?}"
            );
            // A note that is not tucked gets none.
            assert_eq!(with(|s| s.wins[1].room).unwrap(), (0.0, 0.0, 0.0, 0.0));
            host.shutdown();
        }
    }

    // ------------------------------------------------ format bar, images, clipboard

    use crate::widgets::toolbar::BAR_BLOCK;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, SendMessageW, TranslateMessage, WM_LBUTTONDOWN,
        WM_LBUTTONUP,
    };

    fn pack(x: i32, y: i32) -> LPARAM {
        LPARAM(((y << 16) | (x & 0xFFFF)) as isize)
    }

    /// A note with the keyboard, its EDIT and the window.
    fn editing(text: &str, extra: impl FnOnce(&mut Sticky)) -> (WidgetHost, HWND, HWND) {
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", text, extra)];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        host.focus("n1");
        let (hwnd, edit_hwnd) = with(|s| (s.wins[0].hwnd, s.wins[0].edit)).unwrap();
        (host, hwnd, edit_hwnd.expect("the note has the keyboard"))
    }

    /// The EDIT's selection set the way a drag would, and the note told.
    fn select(hwnd: HWND, edit_hwnd: HWND, a: usize, b: usize) {
        let text = with(|s| s.wins[0].text.clone()).unwrap();
        edit::set_selection(edit_hwnd, &text, a, b);
        edit_event(hwnd, edit_hwnd, EditEvent::Changed);
    }

    /// The client point of the bar's button `tool` in window 0.
    fn bar_point(tool: Tool) -> (i32, i32) {
        let (w, h) = with(|s| (s.wins[0].view.w, s.wins[0].view.h)).unwrap();
        let r = toolbar::rect_of(&toolbar::layout(w), tool).expect("the tool is on the bar");
        client_of(r.x + 4.0, h - BAR_BLOCK + r.y + 12.0)
    }

    fn target_of(x: i32, y: i32) -> Target {
        with(|s| {
            let w = &s.wins[0];
            let (ox, oy) = w.origin();
            target_at(w, ox + x, oy + y)
        })
        .unwrap()
    }

    fn styles_of() -> Vec<Run> {
        with(|s| s.wins[0].sticky.styles.clone()).unwrap()
    }

    fn click(hwnd: HWND, (x, y): (i32, i32)) {
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(pack(x, y)));
            SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(pack(x, y)));
        }
    }

    #[test]
    fn ctrl_b_on_a_selection_is_bold_in_the_styles_and_the_drain_has_them_after_the_text() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("Belanja", |_| {});
        select(hwnd, edit_hwnd, 0, 3);
        let _ = host.drain();
        // What the EDIT's key handler does for Ctrl+B.
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Bold));
        assert_eq!(styles_of(), [Run { s: 0, e: 3, style: Style { b: true, ..Style::default() } }]);
        let events = host.drain();
        assert!(
            events.iter().any(|e| matches!(e, WidgetEvent::Styles { id, runs } if id == "n1" && runs.len() == 1)),
            "{events:?}"
        );
        // The text did not change, so no Text event comes with it.
        assert!(!events.iter().any(|e| matches!(e, WidgetEvent::Text { .. })), "{events:?}");
        // Again: the whole range has it, so it comes off.
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Bold));
        assert!(styles_of().is_empty());
        // The EDIT and the selection are untouched by a style.
        assert_eq!(with(|s| (s.wins[0].sel, s.wins[0].text.clone())).unwrap(), ((0, 3), "Belanja".to_owned()));
        // Typing then pushes Text and, after it, Styles.
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Italic));
        let _ = host.drain();
        with(|s| {
            let e = Edit { start: 7, end: 7, insert: "!".into(), caret: 8 };
            apply_edit(s, 0, &e);
        });
        let events = host.drain();
        let at = |f: fn(&WidgetEvent) -> bool| events.iter().position(f);
        let text = at(|e| matches!(e, WidgetEvent::Text { .. })).expect("a Text event");
        let styles = at(|e| matches!(e, WidgetEvent::Styles { .. })).expect("a Styles event");
        assert!(text < styles, "Text first, then Styles: {events:?}");
        host.shutdown();
    }

    #[test]
    fn the_bar_shows_only_while_the_note_has_the_keyboard() {
        let _focus = keyboard_lock();
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", "Belanja", |_| {})];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let (w, h) = with(|s| (s.wins[0].view.w, s.wins[0].view.h)).unwrap();
        let bold = toolbar::rect_of(&toolbar::layout(w), Tool::Bold).unwrap();
        let bar_pt = client_of(bold.x + 4.0, h - BAR_BLOCK + bold.y + 12.0);
        let footer_pt = client_of(w / 2.0, h - 10.0);
        // Not editing: no bar, the footer is the footer.
        assert_ne!(target_of(bar_pt.0, bar_pt.1), Target::Bar(BarHit::Tool(Tool::Bold)));
        assert_eq!(target_of(footer_pt.0, footer_pt.1), Target::Footer);
        assert!(!with(|s| s.wins[0].bar_shown(Instant::now())).unwrap());
        // Editing: the bar takes the footer's place.
        host.focus("n1");
        assert_eq!(target_of(bar_pt.0, bar_pt.1), Target::Bar(BarHit::Tool(Tool::Bold)));
        assert!(matches!(target_of(footer_pt.0, footer_pt.1), Target::Bar(_)));
        let area = with(|s| s.wins[0].body_area()).unwrap();
        assert_eq!(area.h, h - HEADER_H - BAR_BLOCK);
        // Esc ends the editing: the footer is back.
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        end_editing(hwnd);
        assert_eq!(target_of(footer_pt.0, footer_pt.1), Target::Footer);
        assert_ne!(target_of(bar_pt.0, bar_pt.1), Target::Bar(BarHit::Tool(Tool::Bold)));
        assert_eq!(with(|s| s.wins[0].body_area().h).unwrap(), h - HEADER_H - paint::FOOTER_H);
        host.shutdown();
    }

    #[test]
    fn a_click_on_a_bar_button_keeps_the_editing_and_the_selection_and_applies_the_style() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("Belanja", |_| {});
        select(hwnd, edit_hwnd, 1, 5);
        let _ = host.drain();
        let before = with(|s| (s.wins[0].edit, s.wins[0].sel, s.wins[0].anchor, s.wins[0].scroll)).unwrap();
        click(hwnd, bar_point(Tool::Bold));
        let after = with(|s| (s.wins[0].edit, s.wins[0].sel, s.wins[0].anchor, s.wins[0].scroll)).unwrap();
        assert!(after.0.is_some(), "still editing");
        assert_eq!(after, before, "the caret and the selection did not move");
        assert_eq!(styles_of(), [Run { s: 1, e: 5, style: Style { b: true, ..Style::default() } }]);
        // A press that moves is never a drag of the note, and the note stays put.
        let placed_before = with(|s| s.wins[0].sticky.rect).unwrap();
        let (x, y) = bar_point(Tool::Italic);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(pack(x, y)));
        }
        mouse_move(hwnd, x + 30, y);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(pack(x + 30, y)));
        }
        assert!(with(|s| s.wins[0].drag.is_none()).unwrap());
        assert_eq!(with(|s| s.wins[0].sticky.rect).unwrap(), placed_before);
        assert!(!host.drain().iter().any(|e| matches!(e, WidgetEvent::Placed { .. })));
        // A click on the bar's background does nothing at all.
        let (w, h) = with(|s| (s.wins[0].view.w, s.wins[0].view.h)).unwrap();
        let layout = toolbar::layout(w);
        let bg = client_of(layout.bar.right() - 3.0, h - BAR_BLOCK + 15.0);
        let styles_before = styles_of();
        click(hwnd, bg);
        assert_eq!(styles_of(), styles_before);
        assert!(with(|s| s.wins[0].edit.is_some()).unwrap());
        host.shutdown();
    }

    #[test]
    fn a_bar_button_fires_only_when_released_over_the_same_one() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("Belanja", |_| {});
        select(hwnd, edit_hwnd, 0, 3);
        let _ = host.drain();
        let (bx, by) = bar_point(Tool::Bold);
        let (ix, iy) = bar_point(Tool::Italic);
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(pack(bx, by)));
            SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(pack(ix, iy)));
        }
        assert!(styles_of().is_empty(), "released over another button: nothing");
        assert!(with(|s| s.wins[0].bar_press.is_none()).unwrap(), "the press is forgotten");
        host.shutdown();
    }

    #[test]
    fn a_style_chosen_with_a_collapsed_caret_is_for_the_next_typed_text() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("Belanja", |_| {});
        select(hwnd, edit_hwnd, 7, 7);
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Bold));
        assert!(styles_of().is_empty(), "nothing is selected: no run yet");
        assert!(with(|s| s.wins[0].pending.is_some_and(|p| p.b)).unwrap());
        assert!(with(|s| tool_states(&s.wins[0])[0]).unwrap(), "the B is lit at the caret");
        // Typed text takes it.
        with(|s| apply_edit(s, 0, &Edit { start: 7, end: 7, insert: " baru".into(), caret: 12 }));
        assert_eq!(styles_of(), [Run { s: 7, e: 12, style: Style { b: true, ..Style::default() } }]);
        // Toggling again with the caret still there turns the next text plain.
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Bold));
        with(|s| apply_edit(s, 0, &Edit { start: 12, end: 12, insert: "!".into(), caret: 13 }));
        assert_eq!(styles_of(), [Run { s: 7, e: 12, style: Style { b: true, ..Style::default() } }]);
        // Moving the caret without typing drops the pending style.
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Italic));
        assert!(with(|s| s.wins[0].pending.is_some()).unwrap());
        select(hwnd, edit_hwnd, 2, 2);
        assert!(with(|s| s.wins[0].pending.is_none()).unwrap());
        host.shutdown();
    }

    #[test]
    fn the_marker_highlights_with_the_last_colour_and_the_popover_choices_are_remembered() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("Belanja", |_| {});
        select(hwnd, edit_hwnd, 0, 4);
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Marker));
        assert_eq!(styles_of().len(), 1);
        assert_eq!(styles_of()[0].style.hl, Some(Hl::Kuning), "Kuning is the first");
        // Again: it comes off.
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Marker));
        assert!(styles_of().is_empty());
        // A colour from the popover is applied and becomes the last one.
        fmt_choice(hwnd, fmt_pop::Item::Hl(Some(Hl::Pink)));
        assert_eq!(styles_of()[0].style.hl, Some(Hl::Pink));
        assert_eq!(with(|s| s.last_hl).unwrap(), Hl::Pink);
        // A text colour keeps the highlight; "none" and "automatic" clear them.
        fmt_choice(hwnd, fmt_pop::Item::Fg(Some(styles::Fg::Merah)));
        assert_eq!(styles_of()[0].style.fg, Some(styles::Fg::Merah));
        assert_eq!(styles_of()[0].style.hl, Some(Hl::Pink));
        fmt_choice(hwnd, fmt_pop::Item::Hl(None));
        fmt_choice(hwnd, fmt_pop::Item::Fg(None));
        assert!(styles_of().is_empty());
        assert_eq!(with(|s| s.last_hl).unwrap(), Hl::Pink, "clearing keeps the last colour");
        host.shutdown();
    }

    #[test]
    fn the_list_buttons_toggle_every_line_of_the_selection_and_keep_it() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("Judul\nsatu\ndua", |_| {});
        select(hwnd, edit_hwnd, 6, 14);
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Bullet));
        let t = with(|s| s.wins[0].text.clone()).unwrap();
        assert_eq!(t, "Judul\n\u{2022} satu\n\u{2022} dua");
        // The selection follows its text.
        assert_eq!(with(|s| s.wins[0].sel).unwrap(), (8, 18));
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Check));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "Judul\n- [ ] satu\n- [ ] dua");
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Check));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "Judul\nsatu\ndua");
        // Ctrl+Z (the EDIT's undo) takes each back.
        unsafe { SendMessageW(edit_hwnd, windows::Win32::UI::WindowsAndMessaging::WM_UNDO, None, None) };
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "Judul\n- [ ] satu\n- [ ] dua");
        host.shutdown();
    }

    #[test]
    fn a_list_toggle_over_several_lines_keeps_the_runs_of_the_lines_between() {
        let _focus = keyboard_lock();
        let bold = Style { b: true, ..Style::default() };
        let (mut host, hwnd, edit_hwnd) = editing("one\ntwo\nthree", |s| {
            s.styles = vec![Run { s: 4, e: 7, style: bold }];
        });
        select(hwnd, edit_hwnd, 0, 13);
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Bullet));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "\u{2022} one\n\u{2022} two\n\u{2022} three");
        // "two" moved with its line (its marker takes the line's style); nothing else turned bold.
        assert_eq!(styles_of(), [Run { s: 6, e: 11, style: bold }]);
        // To a checklist: the other marker kind, the same.
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Check));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "- [ ] one\n- [ ] two\n- [ ] three");
        assert_eq!(styles_of(), [Run { s: 10, e: 19, style: bold }]);
        // Off again.
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Check));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "one\ntwo\nthree");
        assert_eq!(styles_of(), [Run { s: 4, e: 7, style: bold }]);
        host.shutdown();
    }

    #[test]
    fn a_list_toggle_does_not_turn_the_lines_bold_when_the_first_one_is() {
        let _focus = keyboard_lock();
        let bold = Style { b: true, ..Style::default() };
        let (mut host, hwnd, edit_hwnd) = editing("one\ntwo\nthree", |s| {
            s.styles = vec![Run { s: 0, e: 3, style: bold }];
        });
        select(hwnd, edit_hwnd, 0, 13);
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Bullet));
        // The first line's own marker and letters are bold, no more.
        assert_eq!(styles_of(), [Run { s: 0, e: 5, style: bold }]);
        host.shutdown();
    }

    #[test]
    fn the_list_buttons_light_for_the_lines_the_toggle_acts_on() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("\u{2022} a\nb", |_| {});
        // Line 1 whole, selection ending at the start of line 2: only line 1
        // is touched, so Bullet is on and the button would take it off.
        select(hwnd, edit_hwnd, 0, 4);
        let on = with(|s| tool_states(&s.wins[0])).unwrap();
        assert!(on[4], "Bullet is on for the touched line only: {on:?}");
        edit_event(hwnd, edit_hwnd, EditEvent::Format(Tool::Bullet));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "a\nb");
        host.shutdown();
    }

    #[test]
    fn the_runs_follow_an_edit_the_edit_undoes_and_a_paste() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("Belanja", |s| {
            s.styles = vec![Run { s: 3, e: 7, style: Style { b: true, ..Style::default() } }];
        });
        // Typing before the run shifts it.
        with(|s| apply_edit(s, 0, &Edit { start: 0, end: 0, insert: "xx".into(), caret: 2 }));
        assert_eq!(styles_of(), [Run { s: 5, e: 9, style: Style { b: true, ..Style::default() } }]);
        // Undo takes the text back and the run follows.
        unsafe { SendMessageW(edit_hwnd, windows::Win32::UI::WindowsAndMessaging::WM_UNDO, None, None) };
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "Belanja");
        assert_eq!(styles_of(), [Run { s: 3, e: 7, style: Style { b: true, ..Style::default() } }]);
        // A paste (what `paste_into` hands to `paste_text`) takes the style at the caret.
        select(hwnd, edit_hwnd, 5, 5);
        with(|s| paste_text(s, 0, "ABC", None));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "BelanABCja");
        assert_eq!(styles_of(), [Run { s: 3, e: 10, style: Style { b: true, ..Style::default() } }]);
        // A matching Vale.NoteRich payload overlays its own runs on the pasted text.
        let italic = Style { i: true, ..Style::default() };
        let payload = richclip::encode("XYZ", &[Run { s: 1, e: 2, style: italic }]);
        select(hwnd, edit_hwnd, 0, 0);
        with(|s| paste_text(s, 0, "XYZ", Some(&payload)));
        assert_eq!(with(|s| s.wins[0].text.clone()).unwrap(), "XYZBelanABCja");
        let runs = styles_of();
        assert!(runs.contains(&Run { s: 1, e: 2, style: italic }), "{runs:?}");
        assert!(!runs.iter().any(|r| r.s == 0 && r.style.b), "the pasted text is not bold: {runs:?}");
        // A payload for other text is ignored (plain text, the style at the caret).
        select(hwnd, edit_hwnd, 0, 0);
        with(|s| paste_text(s, 0, "QQ", Some(&payload)));
        assert!(!styles_of().iter().any(|r| r.style.i && r.s < 2));
        host.shutdown();
    }

    #[test]
    fn a_selection_is_copied_as_text_and_runs_and_a_cut_removes_it() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("Belanja", |s| {
            s.styles = vec![Run { s: 0, e: 4, style: Style { b: true, ..Style::default() } }];
        });
        select(hwnd, edit_hwnd, 2, 6);
        let (a, b, part, runs) = with(|s| copy_payload(&s.wins[0])).unwrap().expect("a selection");
        assert_eq!((a, b, part.as_str()), (2, 6, "lanj"));
        assert_eq!(runs, [Run { s: 0, e: 2, style: Style { b: true, ..Style::default() } }]);
        // What a paste into another note reads back.
        let bytes = richclip::encode(&part, &runs);
        assert_eq!(richclip::decode(&bytes, &part), Some(runs));
        // A collapsed caret copies nothing.
        select(hwnd, edit_hwnd, 3, 3);
        assert!(with(|s| copy_payload(&s.wins[0])).unwrap().is_none());
        host.shutdown();
    }

    #[test]
    fn a_dropped_png_becomes_an_image_and_a_fifth_is_refused() {
        let _focus = keyboard_lock();
        let dir = std::env::temp_dir().join(format!("vale-host-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::notes::NotesStore::open(dir.join("notes.json"));
        let images_dir = store.images_dir();
        let src = dir.join("pic.png");
        assert!(image_io::write_gradient(&src, 120, 90, 7));
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", "Belanja", |_| {})];
        let mut host = WidgetHost::new(HWND::default());
        host.set_images_dir(images_dir.clone());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let _ = host.drain();
        let wait_for = |n: usize| {
            let deadline = Instant::now() + std::time::Duration::from_secs(5);
            while Instant::now() < deadline {
                let mut msg = MSG::default();
                unsafe {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                if with(|s| s.wins[0].sticky.images.len()).unwrap() >= n {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            false
        };
        drop_images(hwnd, vec![src.clone()]);
        assert_eq!(with(|s| s.in_flight.len()).unwrap(), 1, "one import is in flight");
        assert!(wait_for(1), "the import came back within 5 s");
        assert_eq!(with(|s| s.in_flight.len()).unwrap(), 0);
        let events = host.drain();
        let list = events
            .iter()
            .find_map(|e| match e {
                WidgetEvent::Images { id, images } if id == "n1" => Some(images.clone()),
                _ => None,
            })
            .expect("an Images event");
        assert_eq!(list.len(), 1);
        assert!(images::valid_name(&list[0]));
        assert!(images_dir.join(&list[0]).is_file(), "in the test store's folder");
        assert!(!crate::config::path().starts_with(&images_dir), "never the real data folder");
        // Three more fill it up; the fifth is refused with the message and no file.
        drop_images(hwnd, vec![src.clone(), src.clone(), src.clone()]);
        assert!(wait_for(4));
        assert_eq!(with(|s| s.wins[0].sticky.images.len()).unwrap(), 4);
        let _ = host.drain();
        drop_images(hwnd, vec![src.clone()]);
        assert_eq!(with(|s| s.in_flight.len()).unwrap(), 0, "nothing started for the fifth");
        assert_eq!(with(|s| s.wins[0].footer_msg).unwrap(), Some(Key::FmtImgMax));
        assert!(host.drain().iter().all(|e| !matches!(e, WidgetEvent::Images { .. })));
        let files = std::fs::read_dir(&images_dir).unwrap().filter_map(Result::ok).count();
        assert_eq!(files, 4, "four files, no fifth");
        // Four in flight at once are counted: with two imports pending, only two more fit.
        host.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_import_that_finishes_for_a_note_that_is_gone_deletes_its_file() {
        let _focus = keyboard_lock();
        let dir = std::env::temp_dir().join(format!("vale-host-gone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("note-images");
        let src = dir.join("p.png");
        assert!(image_io::write_gradient(&src, 40, 30, 1));
        let name = image_io::import(image_io::Source::Path(src), &out).expect("imported");
        assert!(out.join(&name).is_file());
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", "x", |_| {})];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        // The result names a note no window has.
        let done = image_io::Done {
            note: "n-nobody".into(),
            result: Ok(name.clone()),
            dir: out.clone(),
        };
        import_done(hwnd, LPARAM(image_io::stash(hwnd, done) as isize));
        assert!(!out.join(&name).exists(), "no window took it: the new file is deleted");
        assert!(with(|s| s.wins[0].sticky.images.is_empty()).unwrap());
        // A message with a token nothing stashed is ignored, never read as a
        // pointer, and a token is good once.
        import_done(hwnd, LPARAM(0));
        import_done(hwnd, LPARAM(0x1234_5678));
        import_done(hwnd, LPARAM(-1));
        let again = image_io::Done { note: "n1".into(), result: Err(image_io::ImportError::Io), dir: out.clone() };
        let token = image_io::stash(hwnd, again);
        import_done(hwnd, LPARAM(token as isize));
        assert!(image_io::claim(token).is_none(), "taken once");
        assert!(with(|s| s.wins[0].sticky.images.is_empty()).unwrap());
        // A foreign WM_NOTE_IMPORT through the window procedure: nothing happens.
        unsafe {
            SendMessageW(hwnd, image_io::WM_NOTE_IMPORT, Some(WPARAM(0)), Some(LPARAM(0xDEAD_BEEF)));
        }
        assert!(with(|s| s.wins[0].sticky.images.is_empty()).unwrap());
        host.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_image_opens_on_a_click_and_the_x_removes_it() {
        let _focus = keyboard_lock();
        let dir = std::env::temp_dir().join(format!("vale-host-click-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = dir.join("note-images");
        assert!(image_io::write_gradient(&out.join("a.png"), 200, 100, 1));
        assert!(image_io::write_gradient(&out.join("b.png"), 200, 100, 2));
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", "Belanja", |s| {
            s.images = vec!["a.png".into(), "b.png".into()];
        })];
        let mut host = WidgetHost::new(HWND::default());
        host.set_images_dir(out.clone());
        host.sync(&ctx(&notes, &displays));
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        let rects = with(|s| s.wins[0].img_rects.clone()).unwrap();
        assert_eq!(rects.len(), 2, "two images, side by side");
        let top = with(|s| s.wins[0].body_area().y).unwrap();
        // The text starts below the block.
        assert!(with(|s| s.wins[0].body.as_ref().unwrap().lines[0].y).unwrap() > 40.0);
        let _ = host.drain();
        image_io::OPENED.with(|o| o.borrow_mut().clear());
        // A click on the first image (not moving) opens its file.
        let r = rects[0];
        let (x, y) = client_of(r.x + r.w / 2.0, top + r.y + r.h / 2.0);
        assert_eq!(target_of(x, y), Target::Image(0));
        click(hwnd, (x, y));
        let opened = image_io::OPENED.with(|o| o.borrow().clone());
        assert_eq!(opened, [out.join("a.png")]);
        // A press that moves 4 px or more is no click.
        image_io::OPENED.with(|o| o.borrow_mut().clear());
        unsafe {
            SendMessageW(hwnd, WM_LBUTTONDOWN, Some(WPARAM(1)), Some(pack(x, y)));
            SendMessageW(hwnd, WM_LBUTTONUP, Some(WPARAM(0)), Some(pack(x + 6, y)));
        }
        assert!(image_io::OPENED.with(|o| o.borrow().is_empty()));
        // The x of the second image removes it (the file is released at save).
        let xr = paint::image_remove_rect(DipR { x: rects[1].x, y: top + rects[1].y, w: rects[1].w, h: rects[1].h });
        let (x, y) = client_of(xr.x + xr.w / 2.0, xr.y + xr.h / 2.0);
        assert_eq!(target_of(x, y), Target::ImageRemove(1));
        click(hwnd, (x, y));
        assert_eq!(with(|s| s.wins[0].sticky.images.clone()).unwrap(), ["a.png"]);
        let events = host.drain();
        assert!(events.contains(&WidgetEvent::Images { id: "n1".into(), images: vec!["a.png".into()] }), "{events:?}");
        assert!(out.join("b.png").exists(), "the file stays until the owner's save");
        host.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rolled_note_never_shows_the_bar_or_images() {
        let _focus = keyboard_lock();
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n1", "Belanja", |s| {
            s.rolled = true;
            s.images = vec!["a.png".into()];
        })];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        host.focus("n1");
        assert!(with(|s| s.wins[0].edit.is_some()).unwrap(), "it has the keyboard");
        assert!(!with(|s| s.wins[0].bar_shown(Instant::now())).unwrap(), "but no bar while rolled up");
        for (x, y) in [(60.0, 50.0), (60.0, 40.0), (120.0, 60.0)] {
            let (px, py) = client_of(x, y);
            assert!(
                !matches!(target_of(px, py), Target::Bar(_) | Target::Image(_) | Target::ImageRemove(_)),
                "{x},{y}"
            );
        }
        host.shutdown();
    }

    #[test]
    fn a_tucked_note_that_is_not_all_the_way_out_has_no_bar() {
        let _focus = keyboard_lock();
        let (mut host, _hwnd, _edit) = editing("Belanja", |_| {});
        assert!(with(|s| s.wins[0].bar_shown(Instant::now())).unwrap());
        with(|s| {
            s.wins[0].sticky.tuck = Some(Edge::Right);
            s.wins[0].pop = tuck::Pop::at(0.5);
        });
        assert!(!with(|s| s.wins[0].bar_shown(Instant::now())).unwrap(), "no room in the tab");
        with(|s| s.wins[0].pop = tuck::Pop::at(1.0));
        assert!(with(|s| s.wins[0].bar_shown(Instant::now())).unwrap(), "all the way out: the bar shows");
        host.shutdown();
    }

    #[test]
    fn a_footer_message_is_cleared_by_the_pointer_leaving_like_every_other() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, _edit) = editing("Belanja", |_| {});
        with(|s| {
            s.wins[0].footer_msg = Some(Key::FmtImgMax);
            s.wins[0].dirty = true;
        });
        assert_eq!(with(|s| s.wins[0].footer_msg).unwrap(), Some(Key::FmtImgMax));
        mouse_leave(hwnd);
        assert_eq!(with(|s| s.wins[0].footer_msg).unwrap(), None);
        host.shutdown();
    }

    #[test]
    fn the_marker_popover_opens_above_its_button_and_closes_on_esc_and_on_a_choice() {
        let _focus = keyboard_lock();
        let (mut host, hwnd, edit_hwnd) = editing("Belanja", |_| {});
        assert!(!fmt_pop::is_open());
        open_marker_pop(hwnd);
        assert!(fmt_pop::is_open_for(hwnd), "the popover is open");
        // The note keeps the keyboard.
        assert!(with(|s| s.wins[0].edit.is_some()).unwrap());
        // Esc closes it first, and the editing goes on.
        edit_event(hwnd, edit_hwnd, EditEvent::Escape);
        assert!(!fmt_pop::is_open());
        assert!(with(|s| s.wins[0].edit.is_some()).unwrap());
        // A second press on the chevron toggles.
        open_marker_pop(hwnd);
        open_marker_pop(hwnd);
        assert!(!fmt_pop::is_open());
        // A choice closes it.
        open_marker_pop(hwnd);
        fmt_choice(hwnd, fmt_pop::Item::Hl(Some(Hl::Biru)));
        assert!(!fmt_pop::is_open());
        // Ending the editing closes it too.
        open_marker_pop(hwnd);
        end_editing(hwnd);
        assert!(!fmt_pop::is_open());
        // A narrow note keeps the Marker in the overflow menu.
        host.shutdown();
        let displays = vec![display("A", 0, true, 96)];
        let notes = vec![note_with("n2", "x", |s| s.rect.w = 180.0)];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        host.focus("n2");
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        open_overflow(hwnd);
        assert!(fmt_pop::is_open_for(hwnd), "the overflow menu opens on a narrow note");
        fmt_pop::close();
        host.shutdown();
        assert!(!fmt_pop::is_open());
    }

    #[test]
    fn formatting_and_images_are_not_shared_between_sheets_of_a_window() {
        let displays = vec![display("A", 0, true, 96)];
        let bold = Style { b: true, ..Style::default() };
        let mk = |id: &str, order: u32, top: bool, images: &[&str], styles: Vec<Run>| {
            let mut n = note_with(id, "sheet text", |s| {
                s.stack = Some("stk".into());
                s.stack_order = order;
                s.stack_top = top;
                s.images = images.iter().map(|i| (*i).to_owned()).collect();
                s.styles = styles;
            });
            n.updated = 1 + i64::from(order);
            n
        };
        let notes = vec![
            mk("a", 0, true, &["a.png"], vec![Run { s: 0, e: 5, style: bold }]),
            mk("b", 1, false, &[], vec![]),
        ];
        let mut host = WidgetHost::new(HWND::default());
        host.sync(&ctx(&notes, &displays));
        assert_eq!(with(|s| s.wins[0].sticky.images.clone()).unwrap(), ["a.png"]);
        let hwnd = with(|s| s.wins[0].hwnd).unwrap();
        // Flip to sheet b: it has neither.
        with(|s| show_sheet(s, 0, 1));
        assert!(with(|s| s.wins[0].sticky.images.is_empty() && s.wins[0].sticky.styles.is_empty()).unwrap());
        // And back: sheet a keeps its own.
        with(|s| show_sheet(s, 0, 0));
        assert_eq!(with(|s| s.wins[0].sticky.images.clone()).unwrap(), ["a.png"]);
        assert_eq!(styles_of().len(), 1);
        let _ = hwnd;
        host.shutdown();
    }
}
