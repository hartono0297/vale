//! The lyrics card's window: a layered popup drawn into a premultiplied DIB
//! by a software Direct2D target and pushed with `UpdateLayeredWindow`
//! (the pattern of `arrange_overlay::Canvas`; the surface is
//! `ui::card_surface`, shared with the caption card), DirectWrite text, WIC
//! art and the pointer, wheel and key handling.
//!
//! State lives in this module's `thread_local!` (`CARD`). The window procedure
//! never calls `app::with_app`: it tells the island what the user did with
//! `PostMessageW(island, WM_APP_LYRICS_CARD, code, value)`. The borrow of
//! `CARD` is never held across `ShowWindow`, `SetWindowPos`, `SetCapture`,
//! `ReleaseCapture` or `DestroyWindow` (they send messages to this window
//! synchronously): such calls are collected as `Fx` and run after the borrow
//! ends, and the procedure uses `try_borrow_mut` everywhere.
//!
//! Power: the card's own one-shot timers (the next line, the manual-scroll
//! return) and a `FrameClock` only while an animation moves. Nothing repeats
//! and nothing polls; hidden by fullscreen, paused, or without lyrics there is
//! no timer at all.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Direct2D::{D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, ID2D1Bitmap};
use windows::Win32::Graphics::DirectWrite::{DWRITE_TEXT_ALIGNMENT_CENTER, IDWriteTextLayout};
use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, GUID_WICPixelFormat32bppPBGRA,
    IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetSystemMetrics, GetWindowRect,
    HTCLIENT,
    IDC_ARROW, IDC_HAND, IDC_SIZENWSE, KillTimer, LoadCursorW, PostMessageW, RegisterClassExW,
    SM_CXDRAG, SM_CYDRAG, SW_HIDE, SW_SHOWNOACTIVATE, SetCursor, SetTimer, ShowWindow, ULW_ALPHA,
    UpdateLayeredWindow, WM_CAPTURECHANGED, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_SETCURSOR,
    WM_TIMER, WM_WINDOWPOSCHANGED, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, Interface, PCWSTR, w};
use windows_numerics::{Matrix3x2, Vector2};

use super::colour;
use super::layout::{self, Rects, Zone};
use super::{CardContent, Change, Msg, OpenArgs, Status, WM_APP_LYRICS_CARD};
use crate::config::CardPlace;
use crate::lyrics::{self, LrcLine};
use crate::model::Timeline;
use crate::shot_edit::gfx::{bitmap, color};
use crate::ui::card_surface::{Surface, Tween, rounded};
use crate::ui::frame_clock::{FrameClock, WM_APP_FRAME};
use crate::config::UiFont;
use crate::ui::icons::Icon;
use crate::ui::tooltip::{self, Side};

/// The window class.
const CLASS: PCWSTR = w!("ValeLyricsCard");
/// The class name, for the test that pins it.
#[cfg(test)]
pub const CLASS_NAME: &str = "ValeLyricsCard";
/// The card's own timer ids (the card has its own id space).
const TIMER_LINE: usize = 1;
const TIMER_MANUAL: usize = 2;
/// The rest glyph of an instrumental gap and the loading mark (U+266A).
const NOTE: &str = "\u{266A}";

// --------------------------------------------------------------------- art

/// The decoded cover: a small straight-alpha picture for the colour and a
/// premultiplied one for the thumbnail.
struct Art {
    small: Vec<u8>,
    small_side: u32,
    thumb: Vec<u8>,
    thumb_side: u32,
}

const SMALL_SIDE: u32 = 16;
const THUMB_SIDE: u32 = 128;

/// `bytes` decoded and scaled to `side` x `side` in `format`.
fn decode_pixels(wic: &IWICImagingFactory, bytes: &[u8], side: u32, premultiplied: bool) -> Option<Vec<u8>> {
    unsafe {
        let stream = wic.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let decoder = wic
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let scaler = wic.CreateBitmapScaler().ok()?;
        scaler
            .Initialize(&frame, side, side, WICBitmapInterpolationModeFant)
            .ok()?;
        let source: IWICBitmapSource = scaler.cast().ok()?;
        let converter = wic.CreateFormatConverter().ok()?;
        let format = if premultiplied {
            &GUID_WICPixelFormat32bppPBGRA
        } else {
            &GUID_WICPixelFormat32bppBGRA
        };
        converter
            .Initialize(
                &source,
                format,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut pixels = vec![0u8; (side * side * 4) as usize];
        converter.CopyPixels(std::ptr::null(), side * 4, &mut pixels).ok()?;
        Some(pixels)
    }
}

fn decode_art(bytes: &[u8]) -> Option<Art> {
    let wic: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.ok()?;
    Some(Art {
        small: decode_pixels(&wic, bytes, SMALL_SIDE, false)?,
        small_side: SMALL_SIDE,
        thumb: decode_pixels(&wic, bytes, THUMB_SIDE, true)?,
        thumb_side: THUMB_SIDE,
    })
}

// ------------------------------------------------------------------- state

type Stops = (u32, u32, u32);

/// Everything the card knows about what it shows.
struct Content {
    status: Status,
    title: String,
    artist: String,
    lines: Arc<Vec<LrcLine>>,
    timeline: Option<Timeline>,
    playing: bool,
    can_seek: bool,
    session_key: u64,
    /// The dominant colour of the art, when there is art.
    art_rgb: Option<u32>,
    palette: Stops,
}

/// The wrapped lines at the current width.
struct LineLayouts {
    items: Vec<IDWriteTextLayout>,
    /// The same lines in the bold face, wrapped at the wrap width divided by
    /// the current line's scale, so the scaled line wraps where the stacked
    /// one does.
    bold: Vec<IDWriteTextLayout>,
    /// Each line's slot: the taller of its plain and its scaled bold form.
    heights: Vec<f32>,
    tops: Vec<f32>,
}

struct Press {
    zone: Zone,
    line: Option<usize>,
    /// Where the pointer was (screen px) and where the window was.
    start: (i32, i32),
    origin: (i32, i32),
    size_dip: (f32, f32),
    dragging: bool,
}

/// A tooltip call, made once the borrow is released.
enum TipOp {
    Hover(Option<tooltip::Target>),
    Pressed,
    Hide,
}

/// What to do once the borrow is released.
enum Fx {
    Show(bool),
    Capture(bool),
    Cursor(&'static str),
}

pub(super) struct Card {
    hwnd: HWND,
    island: HWND,
    // Declared before the surface: released before its factories.
    thumb: Option<ID2D1Bitmap>,
    surf: Surface,
    dpi: u32,
    origin: (i32, i32),
    size_dip: (f32, f32),
    c: Content,
    layouts: Option<LineLayouts>,
    art: Option<Art>,
    font: UiFont,
    animations: bool,
    started: Instant,
    // view
    cur: Option<usize>,
    prev: Option<usize>,
    scroll: Tween,
    focus: Tween,
    manual: layout::ManualScroll,
    stops: Stops,
    stops_from: Stops,
    fade: Tween,
    hover: Tween,
    hovering: bool,
    /// Tooltip calls the card made, run by `with` after its borrow ends.
    tips: Vec<TipOp>,
    hover_zone: Zone,
    hover_line: Option<usize>,
    /// The last pointer position inside the card (client dip), while it is in:
    /// `rehover` finds the line under it again when the list moves.
    last_pt: Option<(f32, f32)>,
    tracking: bool,
    press: Option<Press>,
    hidden: bool,
    /// The window was destroyed by someone other than `close` (App was asked
    /// to close): nothing is drawn or timed on the dead hwnd any more.
    gone: bool,
    clock: Option<FrameClock>,
    placed: Option<CardPlace>,
    demo: bool,
    /// Demo: freeze the position here (`DI_CARD_POS`).
    frozen_ms: Option<u64>,
}

thread_local! {
    static CARD: RefCell<Option<Card>> = const { RefCell::new(None) };
}

fn register() -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(proc),
        hInstance: module.into(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    unsafe {
        if RegisterClassExW(&wc) == 0 {
            return Error::from_thread().code() == ERROR_CLASS_ALREADY_EXISTS.to_hresult();
        }
    }
    true
}

fn create_window(origin: (i32, i32), size: (i32, i32)) -> Option<HWND> {
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS,
            w!("Vale lyrics"),
            WS_POPUP,
            origin.0,
            origin.1,
            size.0,
            size.1,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .ok()
}

fn px(dip: f32, dpi: u32) -> i32 {
    (dip * dpi.max(96) as f32 / 96.0).round() as i32
}

/// Tests make the window but never put it on screen (`test-spawns-no-window`).
const ON_SCREEN: bool = !cfg!(test);

fn run_fx(hwnd: HWND, fx: Vec<Fx>) {
    for f in fx {
        match f {
            Fx::Show(true) => unsafe {
                if ON_SCREEN {
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
            },
            Fx::Show(false) => unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            },
            Fx::Capture(true) => unsafe {
                let _ = SetCapture(hwnd);
            },
            Fx::Capture(false) => unsafe {
                let _ = ReleaseCapture();
            },
            Fx::Cursor(which) => unsafe {
                let id = match which {
                    "grip" => IDC_SIZENWSE,
                    "hand" => IDC_HAND,
                    _ => IDC_ARROW,
                };
                if let Ok(c) = LoadCursorW(None, id) {
                    SetCursor(Some(c));
                }
            },
        }
    }
}

/// Runs `f` on the card (if there is one and it is not borrowed), then the
/// window calls it asked for.
fn with<R>(f: impl FnOnce(&mut Card, &mut Vec<Fx>) -> R) -> Option<R> {
    let mut fx = Vec::new();
    let (out, hwnd, tips) = CARD.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        let card = guard.as_mut()?;
        let hwnd = card.hwnd;
        let out = f(card, &mut fx);
        Some((out, hwnd, std::mem::take(&mut card.tips)))
    })?;
    run_fx(hwnd, fx);
    for op in tips {
        match op {
            TipOp::Hover(t) => tooltip::hover(hwnd, t),
            TipOp::Pressed => tooltip::pressed(hwnd),
            TipOp::Hide => tooltip::hide(hwnd),
        }
    }
    Some(out)
}

// -------------------------------------------------------------------- API

pub(super) fn open(args: OpenArgs, content: CardContent) -> bool {
    if CARD.with(|c| c.try_borrow().map_or(true, |c| c.is_some())) {
        return false;
    }
    if !register() {
        return false;
    }
    let Some(rc) = layout::place(args.saved, &args.monitors, args.notch) else {
        return false;
    };
    let dpi = args
        .monitors
        .iter()
        .chain(args.notch.iter())
        .max_by_key(|m| {
            let w = (rc.right.min(m.rect.right) - rc.left.max(m.rect.left)).max(0);
            let h = (rc.bottom.min(m.rect.bottom) - rc.top.max(m.rect.top)).max(0);
            i64::from(w) * i64::from(h)
        })
        .map_or(96, |m| m.dpi.max(96));
    let k = dpi as f32 / 96.0;
    let size_dip = layout::clamp_size(rc.width() as f32 / k, rc.height() as f32 / k);
    let size = (px(size_dip.0, dpi), px(size_dip.1, dpi));
    let origin = (rc.left, rc.top);
    let Some(hwnd) = create_window(origin, size) else {
        return false;
    };
    let Ok(surf) = Surface::new(size, content.font) else {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        return false;
    };
    let now = Instant::now();
    let stops = colour::palette_gradient(content.palette[0], content.palette[1], content.palette[2]);
    let mut card = Card {
        hwnd,
        island: args.island,
        thumb: None,
        surf,
        dpi,
        origin,
        size_dip,
        c: Content {
            status: Status::NoMedia,
            title: String::new(),
            artist: String::new(),
            lines: Arc::new(Vec::new()),
            timeline: None,
            playing: false,
            can_seek: false,
            session_key: 0,
            art_rgb: None,
            palette: (content.palette[0], content.palette[1], content.palette[2]),
        },
        layouts: None,
        art: None,
        font: content.font,
        animations: content.animations,
        started: now,
        cur: None,
        prev: None,
        scroll: Tween::at_rest(0.0, now),
        focus: Tween::at_rest(1.0, now),
        manual: layout::ManualScroll::default(),
        stops,
        stops_from: stops,
        fade: Tween::at_rest(1.0, now),
        hover: Tween::at_rest(0.0, now),
        hovering: false,
        tips: Vec::new(),
        hover_zone: Zone::Outside,
        hover_line: None,
        last_pt: None,
        tracking: false,
        press: None,
        // Held back until the card is stored: a frame tick posted to a card
        // nobody can find yet would never be acknowledged.
        hidden: true,
        gone: false,
        clock: None,
        placed: None,
        demo: args.demo,
        frozen_ms: args.frozen_ms,
    };
    let mut fx = Vec::new();
    card.apply(content, now, &mut fx);
    // Jump to where things are: no ease from nothing on the first frame, and
    // no crossfade from the palette to the art (that is for a track change).
    card.scroll = Tween::at_rest(card.scroll_goal(now), now);
    card.stops_from = card.stops;
    card.fade = Tween::at_rest(1.0, now);
    card.hidden = args.hidden;
    card.render(now);
    let hidden = card.hidden;
    CARD.with(|c| *c.borrow_mut() = Some(card));
    if !hidden {
        run_fx(hwnd, vec![Fx::Show(true)]);
    }
    run_fx(hwnd, fx);
    // The timers and the frame clock start now that the window can be found.
    let _ = with(|c, _| {
        let now = Instant::now();
        c.sync_timers(now);
        c.sync_clock(now);
    });
    true
}

pub(super) fn update(content: CardContent) {
    let _ = with(|c, fx| {
        let now = Instant::now();
        c.apply(content, now, fx);
        c.render(now);
    });
}

/// Destroys the window and drops every Direct2D object now.
pub(super) fn close() {
    let card = CARD.with(|c| c.try_borrow_mut().ok().and_then(|mut c| c.take()));
    let Some(mut card) = card else {
        return;
    };
    card.kill_timers();
    card.clock = None;
    let hwnd = card.hwnd;
    tooltip::hide(hwnd);
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    drop(card);
}

pub(super) fn hwnd() -> Option<HWND> {
    CARD.with(|c| c.try_borrow().ok().and_then(|c| c.as_ref().map(|c| c.hwnd)))
}

/// Fullscreen on the card's monitor hides it; true when this changed it.
pub(super) fn set_hidden(hidden: bool) -> bool {
    with(|c, fx| {
        if c.hidden == hidden {
            return false;
        }
        c.hidden = hidden;
        fx.push(Fx::Show(!hidden));
        let now = Instant::now();
        if hidden {
            c.tips.push(TipOp::Hide);
            c.kill_timers();
            c.clock = None;
        } else {
            c.render(now);
            c.sync_timers(now);
            c.sync_clock(now);
        }
        true
    })
    .unwrap_or(false)
}

pub(super) fn shown_key() -> Option<u64> {
    CARD.with(|c| c.try_borrow().ok().and_then(|c| c.as_ref().map(|c| c.c.session_key)))
}

pub(super) fn take_placed() -> Option<CardPlace> {
    CARD.with(|c| c.try_borrow_mut().ok().and_then(|mut c| c.as_mut().and_then(|c| c.placed.take())))
}

// -------------------------------------------------------------- the card

impl Card {
    fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    fn size_px(&self) -> (i32, i32) {
        (px(self.size_dip.0, self.dpi), px(self.size_dip.1, self.dpi))
    }

    fn rects(&self) -> Rects {
        layout::card_rects(self.size_dip.0, self.size_dip.1)
    }

    fn elapsed_ms(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.started).as_millis() as u64
    }

    fn visible(&self) -> bool {
        !self.hidden && !self.gone
    }

    /// The position the card shows at `now` (frozen in the demo).
    fn position(&self, now: Instant) -> u64 {
        if let Some(ms) = self.frozen_ms {
            return ms;
        }
        self.c
            .timeline
            .map_or(0, |t| lyrics::position_at(&t, self.c.playing, now))
    }

    /// The current line, by the notch's own rule.
    fn current_line(&self, now: Instant) -> Option<usize> {
        if self.c.status != Status::Synced {
            return None;
        }
        let pos = self.position(now);
        lyrics::current_line(&self.c.lines, lyrics::lyric_position(pos)).map(|(i, _)| i)
    }

    /// Where the list wants to be: held by the wheel, or on the current line.
    fn scroll_goal(&self, now: Instant) -> f32 {
        if let Some(o) = self.manual.offset() {
            return o;
        }
        let body_h = self.rects().body.h;
        match &self.layouts {
            Some(l) => layout::scroll_target(&l.tops, &l.heights, self.current_line(now), body_h),
            None => 0.0,
        }
    }

    fn bounds(&self) -> (f32, f32) {
        let body_h = self.rects().body.h;
        match &self.layouts {
            Some(l) => layout::scroll_bounds(&l.tops, &l.heights, body_h),
            None => (0.0, 0.0),
        }
    }

    /// Takes a new snapshot of what is playing.
    fn apply(&mut self, content: CardContent, now: Instant, fx: &mut Vec<Fx>) {
        let _ = fx;
        let mut relayout = false;
        if content.font != self.font {
            self.font = content.font;
            self.surf.set_font(content.font);
            relayout = true;
        }
        self.animations = content.animations;
        if let Change::Set(lines) = content.lines {
            self.c.lines = lines;
            relayout = true;
            self.manual.release();
            self.cur = None;
            self.prev = None;
        }
        if let Change::Set(art) = content.art {
            self.set_art(art);
        }
        let palette = (content.palette[0], content.palette[1], content.palette[2]);
        let same_track = self.c.session_key == content.session_key
            && self.c.title == content.title
            && self.c.artist == content.artist;
        self.c.status = content.status;
        self.c.title = content.title;
        self.c.artist = content.artist;
        self.c.timeline = content.timeline;
        self.c.playing = content.playing;
        self.c.can_seek = content.can_seek;
        // The transport buttons go with the session: their tip too.
        if !self.has_media() {
            self.tips.push(TipOp::Hide);
        }
        self.c.session_key = content.session_key;
        self.c.palette = palette;
        if !same_track {
            self.manual.release();
        }
        if relayout || self.layouts.is_none() {
            self.build_layouts();
        }
        self.retarget_colours(now);
        self.follow(now);
    }

    fn set_art(&mut self, bytes: Option<Vec<u8>>) {
        self.art = bytes.as_deref().and_then(decode_art);
        self.c.art_rgb = self
            .art
            .as_ref()
            .and_then(|a| colour::dominant_rgb(&a.small, a.small_side, a.small_side));
        self.thumb = self.art.as_ref().and_then(|a| {
            bitmap(&self.surf.rt, a.thumb_side, a.thumb_side, &a.thumb).ok()
        });
    }

    /// The gradient the art (or, without art, the palette) gives; a change
    /// crossfades.
    fn retarget_colours(&mut self, now: Instant) {
        let want = match self.c.art_rgb {
            Some(rgb) => colour::card_gradient(rgb),
            None => colour::palette_gradient(self.c.palette.0, self.c.palette.1, self.c.palette.2),
        };
        if want == self.stops {
            return;
        }
        // From what shows now.
        self.stops_from = self.shown_stops(now);
        self.stops = want;
        self.fade.go(1.0, layout::FADE_MS, false, now);
        self.fade = if self.animations {
            Tween {
                from: 0.0,
                to: 1.0,
                start: now,
                ms: layout::FADE_MS,
            }
        } else {
            Tween::at_rest(1.0, now)
        };
    }

    fn shown_stops(&self, now: Instant) -> Stops {
        let t = if self.fade.ms == 0 { 1.0 } else { self.fade.t(now) };
        if t >= 1.0 {
            return self.stops;
        }
        (
            colour::lerp(self.stops_from.0, self.stops.0, t),
            colour::lerp(self.stops_from.1, self.stops.1, t),
            colour::lerp(self.stops_from.2, self.stops.2, t),
        )
    }

    /// Wraps every line to the card's width and stacks them.
    fn build_layouts(&mut self) {
        let width = layout::line_width(self.size_dip.0);
        let mut items = Vec::new();
        let mut bold = Vec::new();
        let mut heights = Vec::new();
        for line in self.c.lines.iter() {
            let text = if line.text.is_empty() { NOTE } else { line.text.as_str() };
            let plain = self.surf.layout(text, layout::SIZE_OTHER, 400, width);
            let strong = self
                .surf
                .layout(text, layout::SIZE_OTHER, 600, width / layout::CURRENT_SCALE);
            let (Some(plain), Some(strong)) = (plain, strong) else {
                self.layouts = None;
                return;
            };
            let slot =
                Surface::height_of(&plain).max(Surface::height_of(&strong) * layout::CURRENT_SCALE);
            heights.push(slot);
            items.push(plain);
            bold.push(strong);
        }
        let tops = layout::stack(&heights, layout::LINE_GAP);
        self.layouts = Some(LineLayouts {
            items,
            bold,
            heights,
            tops,
        });
        let bounds = self.bounds();
        self.manual.clamp(bounds);
    }

    /// The current line and the scroll follow the song.
    fn follow(&mut self, now: Instant) {
        let cur = self.current_line(now);
        if cur != self.cur {
            self.prev = self.cur;
            self.cur = cur;
            self.focus.go(1.0, 0, false, now);
            self.focus = if self.animations && self.prev.is_some() {
                Tween {
                    from: 0.0,
                    to: 1.0,
                    start: now,
                    ms: layout::SCROLL_MS,
                }
            } else {
                Tween::at_rest(1.0, now)
            };
        }
        let goal = self.scroll_goal(now);
        if (goal - self.scroll.to).abs() > 0.01 {
            self.scroll.go(goal, layout::SCROLL_MS, self.animations, now);
        }
        // The list moves under a resting pointer: the highlight follows.
        self.rehover(now);
        self.sync_timers(now);
        self.sync_clock(now);
    }

    fn moving(&self, now: Instant) -> bool {
        self.scroll.moving(now) || self.focus.moving(now) || self.fade.moving(now) || self.hover.moving(now)
    }

    fn sync_clock(&mut self, now: Instant) {
        let want = self.visible() && self.moving(now);
        if want && self.clock.is_none() {
            self.clock = Some(FrameClock::start(self.hwnd.0 as isize));
        } else if !want {
            self.clock = None;
        }
    }

    fn kill_timers(&self) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_LINE);
            let _ = KillTimer(Some(self.hwnd), TIMER_MANUAL);
        }
    }

    /// Whether the extrapolated position has reached the end of the track
    /// (`position_at` stops there): nothing moves any more, so no line timer
    /// is armed; the next snapshot re-arms it through `follow`.
    fn at_track_end(&self, now: Instant) -> bool {
        self.c
            .timeline
            .is_some_and(|t| t.end_ms > 0 && lyrics::position_at(&t, true, now) >= t.end_ms)
    }

    /// The one-shot timers: the next line (only while visible and playing
    /// with synced lyrics) and the manual-scroll return (only while held).
    fn sync_timers(&mut self, now: Instant) {
        let line = self.visible()
            && self.c.playing
            && self.c.status == Status::Synced
            && !self.c.lines.is_empty()
            && self.c.timeline.is_some()
            && self.frozen_ms.is_none()
            && !self.at_track_end(now);
        unsafe {
            if line {
                let t = self.c.timeline.unwrap_or_else(|| Timeline {
                    position_ms: 0,
                    end_ms: 0,
                    updated: now,
                    rate: 1.0,
                });
                let pos = lyrics::lyric_position(lyrics::position_at(&t, true, now));
                let wait = lyrics::wait_for(&self.c.lines, pos, t.rate, lyrics::lyric_reach(t.end_ms));
                SetTimer(Some(self.hwnd), TIMER_LINE, layout::timer_ms(wait), None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_LINE);
            }
            let hold = if self.visible() {
                self.manual.remaining_ms(self.elapsed_ms(now))
            } else {
                None
            };
            match hold {
                Some(ms) => {
                    SetTimer(Some(self.hwnd), TIMER_MANUAL, ms, None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_MANUAL);
                }
            }
        }
    }

    // ------------------------------------------------------------ drawing

    fn render(&mut self, now: Instant) {
        if !self.visible() {
            return;
        }
        let size = self.size_px();
        if self.surf.resize(size).is_err() {
            return;
        }
        let k = self.scale();
        let r = self.rects();
        let (w, h) = self.size_dip;
        let stops = self.shown_stops(now);
        let hover = self.hover.at(now);
        let bounds = RECT {
            left: 0,
            top: 0,
            right: size.0,
            bottom: size.1,
        };
        {
            let rt = &self.surf.rt;
            unsafe {
                if rt.BindDC(self.surf.dc, &bounds).is_err() {
                    return;
                }
                rt.SetDpi(96.0 * k, 96.0 * k);
                rt.BeginDraw();
                rt.Clear(Some(&color(0, 0.0)));
            }
        }
        // The card: a rounded gradient.
        let card = layout::RectF::new(0.0, 0.0, w, h);
        if let Some(g) = self.surf.vertical_gradient(
            0.0,
            h,
            &[(0.0, stops.0, 1.0), (1.0, stops.1, 1.0)],
        ) {
            unsafe {
                self.surf
                    .rt
                    .FillRoundedRectangle(&rounded(card, layout::RADIUS), &g);
            }
        }
        self.paint_body(&r, now);
        self.paint_header(&r, hover);
        self.paint_footer(&r, stops, hover, now);
        let drawn = unsafe { self.surf.rt.EndDraw(None, None) };
        if drawn.is_ok() {
            self.present();
        }
    }

    fn paint_header(&mut self, r: &Rects, hover: f32) {
        // The thumbnail, rounded; a plain plate without art.
        if let Some(bmp) = self.thumb.clone() {
            unsafe {
                if let Ok(brush) = self.surf.rt.CreateBitmapBrush(&bmp, None, None) {
                    brush.SetInterpolationMode(D2D1_BITMAP_INTERPOLATION_MODE_LINEAR);
                    let side = self.art.as_ref().map_or(THUMB_SIDE, |a| a.thumb_side) as f32;
                    let s = r.thumb.w / side;
                    brush.SetTransform(&Matrix3x2 {
                        M11: s,
                        M12: 0.0,
                        M21: 0.0,
                        M22: s,
                        M31: r.thumb.x,
                        M32: r.thumb.y,
                    });
                    self.surf
                        .rt
                        .FillRoundedRectangle(&rounded(r.thumb, layout::THUMB_RADIUS), &brush);
                }
            }
        } else {
            self.surf
                .fill_round(r.thumb, layout::THUMB_RADIUS, 0xFFFFFF, 0.10);
            self.surf.icon(Icon::Music, r.thumb, 16.0, 0xFFFFFF, 0.55);
        }
        let (title, artist) = (self.c.title.clone(), self.c.artist.clone());
        if !title.is_empty()
            && let Some(l) = self.surf.one_line(&title, 13.0, 600, r.title.w)
        {
            self.surf.draw_text((r.title.x, r.title.y), &l, 0xFFFFFF, 1.0);
        }
        if !artist.is_empty()
            && let Some(l) = self.surf.one_line(&artist, 12.0, 400, r.artist.w)
        {
            self.surf.draw_text((r.artist.x, r.artist.y), &l, 0xFFFFFF, 0.60);
        }
        // The close button: an icon, brighter under the pointer.
        let lit = self.hover_zone == Zone::Close;
        if lit {
            self.surf.fill_round(r.close, layout::CLOSE / 2.0, 0xFFFFFF, 0.12);
        }
        self.surf
            .icon(Icon::X, r.close, 14.0, 0xFFFFFF, if lit { 1.0 } else { 0.70 });
        let _ = hover;
    }

    fn paint_footer(&mut self, r: &Rects, stops: Stops, hover: f32, now: Instant) {
        let bar = r.bar;
        self.surf.fill_round(bar, bar.h / 2.0, 0xFFFFFF, 0.20);
        if let Some(t) = self.c.timeline {
            let end = t.end_ms;
            let pos = if self.frozen_ms.is_some() {
                self.position(now)
            } else {
                lyrics::position_at(&t, self.c.playing, now)
            };
            let fill = layout::progress(pos, end);
            if fill > 0.0 {
                let f = layout::RectF::new(bar.x, bar.y, (bar.w * fill).max(bar.h), bar.h);
                self.surf.fill_round(f, bar.h / 2.0, stops.2, 1.0);
            }
        }
        if hover <= 0.001 {
            return;
        }
        // Prev, play/pause, next fade in above the bar; with no session there
        // is nothing to control, so they are not drawn (nor hit, `zone_at`).
        if self.has_media() {
            let play = if self.c.playing { Icon::Pause } else { Icon::Play };
            for (i, icon) in [Icon::Prev, play, Icon::Next].into_iter().enumerate() {
                let c = r.controls[i];
                let zone = [Zone::Prev, Zone::PlayPause, Zone::Next][i];
                if self.hover_zone == zone {
                    self.surf.fill_round(c, c.w / 2.0, 0xFFFFFF, 0.14 * hover);
                }
                self.surf.icon(icon, c, 18.0, 0xFFFFFF, 0.92 * hover);
            }
        }
        // The grip: two short diagonal strokes in the corner.
        self.paint_grip(r.grip, 0.45 * hover);
    }

    fn paint_grip(&self, g: layout::RectF, alpha: f32) {
        for off in [4.0f32, 8.0] {
            unsafe {
                self.surf.brush.SetColor(&color(0xFFFFFF, alpha));
                self.surf.rt.DrawLine(
                    Vector2::new(g.right() - 3.0, g.bottom() - 3.0 - off),
                    Vector2::new(g.right() - 3.0 - off, g.bottom() - 3.0),
                    &self.surf.brush,
                    1.5,
                    None,
                );
            }
        }
    }

    /// The lines (or a message) in the body, faded at its top and bottom.
    fn paint_body(&mut self, r: &Rects, now: Instant) {
        let body = r.body;
        let fade = (layout::EDGE_FADE / body.h.max(1.0)).min(0.5);
        let opacity = self.surf.vertical_gradient(
            body.y,
            body.bottom(),
            &[
                (0.0, 0, 0.0),
                (fade, 0, 1.0),
                (1.0 - fade, 0, 1.0),
                (1.0, 0, 0.0),
            ],
        );
        let status = self.c.status;
        let scroll = self.scroll.at(now);
        let focus = self.focus.at(now);
        let (cur, prev) = (self.cur, self.prev);
        let hover_line = if self.c.can_seek { self.hover_line } else { None };
        let width = layout::line_width(self.size_dip.0);
        let keep = self.surf.push_layer(body, opacity);
        let message = match status {
            Status::Synced => None,
            Status::Pending => Some(crate::i18n::tr(crate::i18n::Key::LyricsCardSearching)),
            Status::Missing => Some(crate::i18n::tr(crate::i18n::Key::LyricsCardNone)),
            Status::NoMedia => Some(crate::i18n::tr(crate::i18n::Key::LyricsCardNoMedia)),
        };
        if let Some(msg) = message {
            paint_message(&mut self.surf, body, msg, status == Status::Pending, width);
        } else if let Some(l) = self.layouts.as_mut() {
            let big = layout::CURRENT_SCALE;
            for i in 0..l.items.len() {
                let top = body.y + l.tops[i] - scroll;
                let h = l.heights[i];
                if top + h * big < body.y - 40.0 || top > body.bottom() + 40.0 {
                    continue;
                }
                let e = if Some(i) == cur {
                    focus
                } else if Some(i) == prev {
                    1.0 - focus
                } else {
                    0.0
                };
                let s = 1.0 + (big - 1.0) * e;
                if hover_line == Some(i) {
                    let hr = layout::RectF::new(
                        layout::PAD - 8.0,
                        top - 5.0,
                        body.w - 2.0 * (layout::PAD - 8.0),
                        // `h` is the slot, which already holds the scaled line.
                        h + 10.0,
                    );
                    self.surf.fill_round(hr, 10.0, 0xFFFFFF, 0.08);
                }
                let alpha = 0.40 + 0.60 * e;
                let (x, cy) = (layout::PAD, top + h / 2.0);
                // Bold from halfway through the ease; every line is centred
                // in its slot.
                let drawn = if e >= 0.5 { &l.bold[i] } else { &l.items[i] };
                let dh = Surface::height_of(drawn);
                unsafe {
                    self.surf.rt.SetTransform(&Matrix3x2 {
                        M11: s,
                        M12: 0.0,
                        M21: 0.0,
                        M22: s,
                        M31: x - s * x,
                        M32: cy - s * cy,
                    });
                }
                self.surf.draw_text((x, cy - dh / 2.0), drawn, 0xFFFFFF, alpha);
                unsafe { self.surf.rt.SetTransform(&Matrix3x2::identity()) };
            }
        }
        self.surf.pop_layer(keep);
    }

    /// Pushes the drawn DIB to the window.
    fn present(&self) {
        let size = self.size_px();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&POINT {
                    x: self.origin.0,
                    y: self.origin.1,
                }),
                Some(&SIZE {
                    cx: size.0,
                    cy: size.1,
                }),
                Some(self.surf.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    // -------------------------------------------------------------- input

    fn to_dip(&self, x: i32, y: i32) -> (f32, f32) {
        let k = self.scale();
        (x as f32 / k, y as f32 / k)
    }

    fn zone(&self, x: f32, y: f32) -> Zone {
        let (w, h) = self.size_dip;
        layout::zone_at(&self.rects(), w, h, x, y, self.hovering, self.has_media())
    }

    /// The tip of the control under the pointer, anchored on screen (the card
    /// is exactly its window, so the origin plus the dip rect is enough).
    fn tip_target(&self, zone: Zone) -> Option<tooltip::Target> {
        let r = self.rects();
        let rect = match zone {
            Zone::Close => r.close,
            Zone::Prev => r.controls[0],
            Zone::PlayPause => r.controls[1],
            Zone::Next => r.controls[2],
            Zone::Grip => r.grip,
            _ => return None,
        };
        let tip = layout::zone_tip(zone, self.c.playing)?;
        Some(tooltip::Target {
            id: layout::zone_tip_id(zone),
            anchor: tooltip::anchor_px(self.origin, rect.shared(), self.scale()),
            side: Side::Auto,
            tip,
        })
    }

    /// A session to send prev, play/pause and next to.
    fn has_media(&self) -> bool {
        self.c.status != Status::NoMedia
    }

    fn line_at(&self, y: f32, now: Instant) -> Option<usize> {
        let l = self.layouts.as_ref()?;
        if self.c.status != Status::Synced {
            return None;
        }
        let r = self.rects();
        layout::hit_line(&l.tops, &l.heights, self.scroll.at(now), r.body.y, r.body.h, y)
    }

    fn set_hover(&mut self, on: bool, now: Instant) {
        if self.hovering == on {
            return;
        }
        self.hovering = on;
        self.hover
            .go(if on { 1.0 } else { 0.0 }, layout::HOVER_MS, self.animations, now);
    }

    /// Asks Windows for `WM_MOUSELEAVE` (once; it is one-shot).
    fn arm_tracking(&mut self) {
        if self.tracking {
            return;
        }
        let mut t = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        if unsafe { TrackMouseEvent(&mut t) }.is_ok() {
            self.tracking = true;
        }
    }

    /// The zone and the line under the resting pointer, found again after the
    /// list moved (the follow, the wheel, the manual return, the ease). Not
    /// while a press is held (a drag or a click in progress keeps what it
    /// had); without a pointer inside there is no highlight.
    fn rehover(&mut self, now: Instant) {
        if self.press.is_some() {
            return;
        }
        match self.last_pt {
            Some((x, y)) if self.hovering => {
                let zone = self.zone(x, y);
                self.hover_line = if zone == Zone::Body { self.line_at(y, now) } else { None };
                self.hover_zone = zone;
            }
            _ => self.hover_line = None,
        }
    }

    fn pointer_move(&mut self, x: i32, y: i32, fx: &mut Vec<Fx>, post: &mut Vec<(Msg, isize)>) {
        let now = Instant::now();
        self.arm_tracking();
        if let Some(press) = &self.press {
            // A press that moves is a drag (or a resize from the grip).
            let mut cursor = POINT::default();
            if unsafe { GetCursorPos(&mut cursor) }.is_err() {
                return;
            }
            let (dx, dy) = (cursor.x - press.start.0, cursor.y - press.start.1);
            let thr = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
            let started = press.dragging || layout::is_drag(dx, dy, thr.0, thr.1);
            let (zone, origin, size_dip) = (press.zone, press.origin, press.size_dip);
            if started && matches!(zone, Zone::Header | Zone::Body | Zone::Grip) {
                if let Some(p) = &mut self.press {
                    if !p.dragging {
                        self.tips.push(TipOp::Hide);
                    }
                    p.dragging = true;
                }
                if zone == Zone::Grip {
                    let k = self.scale();
                    let (nw, nh) = layout::resized(size_dip, dx as f32 / k, dy as f32 / k);
                    self.resize_to((nw, nh), now);
                } else {
                    self.origin = (origin.0 + dx, origin.1 + dy);
                    self.present();
                }
            }
            return;
        }
        let (dx, dy) = self.to_dip(x, y);
        self.last_pt = Some((dx, dy));
        self.set_hover(true, now);
        let zone = self.zone(dx, dy);
        let line = if zone == Zone::Body { self.line_at(dy, now) } else { None };
        let changed = zone != self.hover_zone || line != self.hover_line;
        self.hover_zone = zone;
        self.hover_line = line;
        let cursor = match zone {
            Zone::Grip => "grip",
            Zone::Close | Zone::Prev | Zone::PlayPause | Zone::Next => "hand",
            Zone::Body if line.is_some() && self.c.can_seek => "hand",
            _ => "arrow",
        };
        fx.push(Fx::Cursor(cursor));
        let t = self.tip_target(zone);
        self.tips.push(TipOp::Hover(t));
        let _ = post;
        if changed || self.moving(now) {
            self.sync_clock(now);
            self.render(now);
        } else {
            self.sync_clock(now);
            if self.hover.moving(now) {
                self.render(now);
            }
        }
    }

    /// A new size (dip): the DIB, the wrapping and the scroll follow.
    fn resize_to(&mut self, size_dip: (f32, f32), now: Instant) {
        let wrap_changed = (layout::line_width(size_dip.0) - layout::line_width(self.size_dip.0)).abs() > 0.5;
        self.size_dip = size_dip;
        if wrap_changed {
            self.build_layouts();
        }
        let goal = self.scroll_goal(now);
        self.scroll = Tween::at_rest(goal, now);
        self.render(now);
    }

    fn pointer_leave(&mut self) {
        self.tracking = false;
        if self.press.is_some() {
            return;
        }
        self.tips.push(TipOp::Hover(None));
        let now = Instant::now();
        self.last_pt = None;
        self.set_hover(false, now);
        self.hover_zone = Zone::Outside;
        self.hover_line = None;
        self.sync_clock(now);
        self.render(now);
    }

    fn button_down(&mut self, x: i32, y: i32, fx: &mut Vec<Fx>) {
        self.tips.push(TipOp::Pressed);
        let now = Instant::now();
        let (dx, dy) = self.to_dip(x, y);
        // The controls and the grip only exist while the pointer is in.
        self.hovering = true;
        let zone = self.zone(dx, dy);
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        let line = if zone == Zone::Body { self.line_at(dy, now) } else { None };
        self.press = Some(Press {
            zone,
            line,
            start: (cursor.x, cursor.y),
            origin: self.origin,
            size_dip: self.size_dip,
            dragging: false,
        });
        fx.push(Fx::Capture(true));
    }

    fn button_up(&mut self, x: i32, y: i32, fx: &mut Vec<Fx>, post: &mut Vec<(Msg, isize)>) {
        let Some(press) = self.press.take() else {
            return;
        };
        fx.push(Fx::Capture(false));
        self.release(&press, x, y, post);
        self.after_press();
    }

    /// Capture was taken away mid-press (Alt+Tab, the Win key, a UAC prompt):
    /// a drag or resize that already moved the window is finished and saved,
    /// as a button release would. Our own `ReleaseCapture` in `button_up`
    /// lands here with the press already gone: nothing happens twice.
    fn capture_lost(&mut self, post: &mut Vec<(Msg, isize)>) {
        let Some(press) = self.press.take() else {
            return;
        };
        if press.dragging {
            self.finish_move(post);
        }
        self.after_press();
    }

    /// The press ended: the pointer may be outside the card now (`hovering`
    /// stayed true, `WM_MOUSELEAVE` was ignored while pressed) or inside
    /// with its tracking used up.
    fn after_press(&mut self) {
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        let size = self.size_px();
        if !layout::point_in((cursor.x, cursor.y), self.origin, size) {
            self.pointer_leave();
            return;
        }
        let now = Instant::now();
        let (before_zone, before_line) = (self.hover_zone, self.hover_line);
        self.tracking = false;
        self.arm_tracking();
        self.last_pt = Some(self.to_dip(cursor.x - self.origin.0, cursor.y - self.origin.1));
        self.rehover(now);
        if (self.hover_zone, self.hover_line) != (before_zone, before_line) {
            self.render(now);
        }
    }

    /// What a release does: ends a drag, or clicks what was pressed.
    fn release(&mut self, press: &Press, x: i32, y: i32, post: &mut Vec<(Msg, isize)>) {
        let now = Instant::now();
        if press.dragging {
            self.finish_move(post);
            return;
        }
        let (dx, dy) = self.to_dip(x, y);
        let zone = self.zone(dx, dy);
        if zone != press.zone {
            return;
        }
        match zone {
            Zone::Close => post.push((Msg::Close, 0)),
            Zone::Prev => post.push((Msg::Prev, 0)),
            Zone::PlayPause => post.push((Msg::PlayPause, 0)),
            Zone::Next => post.push((Msg::Next, 0)),
            Zone::Body => {
                // A press and release without movement over a line seeks, when
                // the session allows it.
                let line = self.line_at(dy, now);
                if self.c.can_seek
                    && line.is_some()
                    && line == press.line
                    && let Some(ms) = line.and_then(|i| layout::seek_ms(&self.c.lines, i))
                {
                    post.push((Msg::Seek, ms as isize));
                }
            }
            _ => {}
        }
    }

    /// A drag or resize ended: the monitor may have changed DPI; save the place.
    fn finish_move(&mut self, post: &mut Vec<(Msg, isize)>) {
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        if dpi >= 96 && dpi != self.dpi && !self.demo_dpi_fixed() {
            self.set_dpi(dpi, None);
        }
        let size = self.size_px();
        self.placed = Some(CardPlace {
            left: self.origin.0,
            top: self.origin.1,
            right: self.origin.0 + size.0,
            bottom: self.origin.1 + size.1,
        });
        post.push((Msg::Placed, 0));
    }

    fn demo_dpi_fixed(&self) -> bool {
        false
    }

    /// Windows moved the window itself (a monitor unplugged or rearranged, a
    /// display change): later renders keep it where it is now.
    fn sync_origin(&mut self) {
        let mut r = RECT::default();
        if unsafe { GetWindowRect(self.hwnd, &mut r) }.is_ok() {
            self.origin = (r.left, r.top);
        }
    }

    /// The window is gone: stop the timers and the frame clock for good.
    fn destroyed(&mut self) {
        self.tips.push(TipOp::Hide);
        self.gone = true;
        self.press = None;
        self.last_pt = None;
        self.kill_timers();
        self.clock = None;
    }

    /// The window moved to a monitor of another DPI: the same size in dip.
    fn set_dpi(&mut self, dpi: u32, origin: Option<(i32, i32)>) {
        self.dpi = dpi.max(96);
        if let Some(o) = origin {
            self.origin = o;
        }
        let now = Instant::now();
        self.render(now);
    }

    fn wheel(&mut self, delta: i16, now: Instant) {
        self.tips.push(TipOp::Hide);
        if self.c.status != Status::Synced || self.layouts.is_none() {
            return;
        }
        let bounds = self.bounds();
        let base = self.scroll.at(now);
        let notches = f32::from(delta) / 120.0;
        let goal = self.manual.wheel(base, notches, bounds, self.elapsed_ms(now));
        self.scroll.go(goal, 120, self.animations, now);
        self.rehover(now);
        self.sync_timers(now);
        self.sync_clock(now);
        self.render(now);
    }

    fn timer(&mut self, id: usize, now: Instant) {
        match id {
            TIMER_LINE => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_LINE);
                }
                self.follow(now);
                self.render(now);
            }
            TIMER_MANUAL => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_MANUAL);
                }
                if self.manual.due(self.elapsed_ms(now)) {
                    self.manual.release();
                    self.follow(now);
                    self.render(now);
                } else {
                    self.sync_timers(now);
                }
            }
            _ => {}
        }
    }

    fn frame(&mut self, now: Instant) {
        if let Some(clock) = &self.clock {
            clock.ack();
        }
        if self.scroll.moving(now) {
            self.rehover(now);
        }
        self.render(now);
        self.sync_clock(now);
    }
}

/// A centred message in the body (with the rest glyph over it while searching).
fn paint_message(surf: &mut Surface, body: layout::RectF, msg: &str, searching: bool, width: f32) {
    let mut y = body.y + body.h * 0.40;
    if searching
        && let Some(n) = surf.layout(NOTE, 30.0, 400, width)
    {
        let h = Surface::height_of(&n);
        unsafe {
            let _ = n.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
        }
        surf.draw_text((layout::PAD, y - h), &n, 0xFFFFFF, 0.5);
        y += 6.0;
    }
    if let Some(l) = surf.layout(msg, 14.0, 400, width) {
        unsafe {
            let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
        }
        surf.draw_text((layout::PAD, y), &l, 0xFFFFFF, 0.60);
    }
}

// ------------------------------------------------------------ the procedure

fn lparam_xy(lparam: LPARAM) -> (i32, i32) {
    (
        i32::from(lparam.0 as i16),
        i32::from((lparam.0 >> 16) as i16),
    )
}

/// Tells the island (or, in the demo, prints) what the user did.
fn emit(card_island: HWND, demo: bool, msg: Msg, value: isize) {
    if demo {
        super::demo_emit(msg, value);
        return;
    }
    if card_island.0.is_null() {
        return;
    }
    unsafe {
        let _ = PostMessageW(
            Some(card_island),
            WM_APP_LYRICS_CARD,
            WPARAM(msg.code()),
            LPARAM(value),
        );
    }
}

/// Never calls into `App`.
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_SETCURSOR => {
            // The client area sets its own cursor on mouse moves.
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT {
                return LRESULT(1);
            }
        }
        WM_MOUSEMOVE => {
            let (x, y) = lparam_xy(lparam);
            let mut post = Vec::new();
            with(|c, fx| c.pointer_move(x, y, fx, &mut post));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            with(|c, _| c.pointer_leave());
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = lparam_xy(lparam);
            with(|c, fx| c.button_down(x, y, fx));
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let (x, y) = lparam_xy(lparam);
            let mut post = Vec::new();
            let info = with(|c, fx| {
                c.button_up(x, y, fx, &mut post);
                (c.island, c.demo)
            });
            if let Some((island, demo)) = info {
                for (m, v) in post {
                    emit(island, demo, m, v);
                }
            }
            return LRESULT(0);
        }
        WM_CAPTURECHANGED => {
            // A drag or resize cut short (Alt+Tab, the Win key, a UAC prompt)
            // still saves where the card is. Posted after the borrow.
            let mut post = Vec::new();
            let info = with(|c, _| {
                c.capture_lost(&mut post);
                (c.island, c.demo)
            });
            if let Some((island, demo)) = info {
                for (m, v) in post {
                    emit(island, demo, m, v);
                }
            }
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            // Only the card scrolls: the wheel never reaches the island.
            let delta = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16;
            with(|c, _| c.wheel(delta, Instant::now()));
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            tooltip::hide(hwnd);
            if wparam.0 as u16 == VK_ESCAPE.0 {
                if let Some((island, demo)) = with(|c, _| (c.island, c.demo)) {
                    emit(island, demo, Msg::Close, 0);
                }
                return LRESULT(0);
            }
        }
        WM_TIMER => {
            with(|c, _| c.timer(wparam.0, Instant::now()));
            return LRESULT(0);
        }
        WM_APP_FRAME => {
            with(|c, _| c.frame(Instant::now()));
            return LRESULT(0);
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xFFFF) as u32;
            let rect = unsafe { &*(lparam.0 as *const RECT) };
            with(|c, _| c.set_dpi(dpi, Some((rect.left, rect.top))));
            return LRESULT(0);
        }
        // Every render passes the stored origin to `UpdateLayeredWindow`, so a
        // move Windows made must be read back or the next render undoes it.
        // A move the card made itself (inside `present`) arrives while the card
        // is borrowed and is skipped: the origin already holds it.
        WM_WINDOWPOSCHANGED => {
            with(|c, _| {
                if c.hwnd == hwnd {
                    c.sync_origin();
                }
            });
        }
        // Alt+F4 on a focused card: App closes it, like Esc and the X.
        // DefWindowProc would destroy the window behind App's back.
        WM_CLOSE => {
            if let Some((island, demo)) = with(|c, _| (c.island, c.demo)) {
                emit(island, demo, Msg::Close, 0);
            }
            return LRESULT(0);
        }
        // `close` takes the card out before it destroys the window, so a card
        // still stored here was destroyed by someone else: App drops it.
        WM_DESTROY => {
            let info = with(|c, _| {
                (c.hwnd == hwnd && !c.gone).then(|| {
                    c.destroyed();
                    (c.island, c.demo)
                })
            })
            .flatten();
            if let Some((island, demo)) = info {
                emit(island, demo, Msg::Close, 0);
            }
            return LRESULT(0);
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics_card::layout::Mon;
    use crate::ui::settings_window::Rc;
    use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    fn content(status: Status, playing: bool) -> CardContent {
        let lines = vec![
            LrcLine {
                at_ms: 1_000,
                text: "one".into(),
            },
            LrcLine {
                at_ms: 5_000,
                text: String::new(),
            },
            LrcLine {
                at_ms: 9_000,
                text: "three".into(),
            },
        ];
        CardContent {
            status,
            title: "Title".into(),
            artist: "Artist".into(),
            art: Change::Set(None),
            lines: Change::Set(Arc::new(lines)),
            timeline: Some(Timeline {
                position_ms: 6_000,
                end_ms: 30_000,
                updated: Instant::now(),
                rate: 1.0,
            }),
            playing,
            can_seek: true,
            session_key: 7,
            palette: [0x232527, 0x1B1D1F, 0xD97757],
            font: UiFont::Sans,
            animations: true,
        }
    }

    fn args() -> OpenArgs {
        let rect = Rc {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let mon = Mon {
            rect,
            work: Rc { bottom: 1040, ..rect },
            dpi: 96,
        };
        OpenArgs {
            island: HWND::default(),
            saved: None,
            monitors: vec![mon],
            notch: Some(mon),
            hidden: false,
            demo: true,
            frozen_ms: None,
        }
    }

    #[test]
    fn the_card_opens_updates_hides_and_closes_without_a_window_on_screen() {
        assert!(open(args(), content(Status::Synced, true)));
        // A second open does nothing: one card.
        assert!(!open(args(), content(Status::Synced, true)));
        let hwnd = hwnd().expect("the card exists");
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool(), "tests never show it");
        assert_eq!(shown_key(), Some(7));
        // Every status draws; the lines are dropped when they stop applying.
        for status in [Status::Pending, Status::Missing, Status::NoMedia, Status::Synced] {
            update(content(status, status == Status::Synced));
        }
        // Fullscreen hides it and brings it back; asking twice changes nothing.
        assert!(set_hidden(true));
        assert!(!set_hidden(true));
        update(content(Status::Synced, true));
        assert!(set_hidden(false));
        assert!(!set_hidden(false));
        // A drag's end saves the place once.
        assert!(take_placed().is_none());
        close();
        assert!(super::hwnd().is_none());
        assert!(!unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)) }.as_bool());
        // Closing again is harmless, and it can open again.
        close();
        assert!(open(args(), content(Status::Missing, false)));
        close();
    }

    #[test]
    fn the_timers_exist_only_while_playing_visible_with_synced_lyrics() {
        use windows::Win32::UI::WindowsAndMessaging::KillTimer;
        // A timer that exists can be killed (Ok); one that does not, cannot.
        let timer_set = |h: HWND, id: usize| unsafe { KillTimer(Some(h), id) }.is_ok();
        assert!(open(args(), content(Status::Synced, true)));
        let h = hwnd().unwrap();
        assert!(timer_set(h, TIMER_LINE), "playing with synced lyrics arms it");
        update(content(Status::Synced, true));
        assert!(timer_set(h, TIMER_LINE), "re-armed by the next snapshot");
        update(content(Status::Synced, false));
        assert!(!timer_set(h, TIMER_LINE), "paused: none");
        update(content(Status::Missing, true));
        assert!(!timer_set(h, TIMER_LINE), "no lyrics: none");
        update(content(Status::Synced, true));
        assert!(set_hidden(true));
        assert!(!timer_set(h, TIMER_LINE), "hidden: none");
        assert!(!timer_set(h, TIMER_MANUAL));
        close();
    }

    #[test]
    fn no_line_timer_runs_once_the_track_has_reached_its_end() {
        use windows::Win32::UI::WindowsAndMessaging::KillTimer;
        let timer_set = |h: HWND, id: usize| unsafe { KillTimer(Some(h), id) }.is_ok();
        assert!(open(args(), content(Status::Synced, true)));
        let h = hwnd().unwrap();
        assert!(timer_set(h, TIMER_LINE), "mid-track: armed");
        let mut at_end = content(Status::Synced, true);
        if let Some(t) = at_end.timeline.as_mut() {
            t.position_ms = t.end_ms;
        }
        update(at_end);
        assert!(!timer_set(h, TIMER_LINE), "at the end: nothing left to move");
        close();
    }

    /// A message-only window standing in for the island (never on screen).
    fn fake_island() -> HWND {
        use windows::Win32::UI::WindowsAndMessaging::{HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE};
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                None,
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .expect("a message-only window")
    }

    /// The card messages posted to `island`, oldest first (removed).
    fn posted(island: HWND) -> Vec<usize> {
        use windows::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE, PeekMessageW};
        let mut out = Vec::new();
        let mut m = MSG::default();
        while unsafe {
            PeekMessageW(&mut m, Some(island), WM_APP_LYRICS_CARD, WM_APP_LYRICS_CARD, PM_REMOVE)
        }
        .as_bool()
        {
            out.push(m.wParam.0);
        }
        out
    }

    #[test]
    fn wm_close_asks_the_island_to_close_instead_of_destroying_the_card() {
        use windows::Win32::UI::WindowsAndMessaging::{IsWindow, SendMessageW};
        let island = fake_island();
        let mut a = args();
        a.island = island;
        a.demo = false;
        assert!(open(a, content(Status::Synced, true)));
        let h = hwnd().unwrap();
        // Alt+F4 on a focused card ends in WM_CLOSE.
        unsafe { SendMessageW(h, WM_CLOSE, None, None) };
        assert!(unsafe { IsWindow(Some(h)) }.as_bool(), "the window lives until App closes it");
        assert_eq!(hwnd(), Some(h));
        assert_eq!(posted(island), vec![Msg::Close.code()], "the same path as Esc and the X");
        // App's own close posts nothing back.
        close();
        assert!(posted(island).is_empty());
        unsafe {
            let _ = DestroyWindow(island);
        }
    }

    #[test]
    fn a_card_destroyed_behind_app_s_back_asks_the_island_to_close() {
        let island = fake_island();
        let mut a = args();
        a.island = island;
        a.demo = false;
        assert!(open(a, content(Status::Synced, true)));
        let h = hwnd().unwrap();
        unsafe {
            let _ = DestroyWindow(h);
        }
        assert_eq!(posted(island), vec![Msg::Close.code()]);
        // App then drops the stale card; nothing else is posted.
        close();
        assert!(hwnd().is_none());
        assert!(posted(island).is_empty());
        unsafe {
            let _ = DestroyWindow(island);
        }
    }

    #[test]
    fn a_move_windows_makes_is_kept_by_the_next_render() {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
        };
        let rect_of = |h: HWND| {
            let mut r = RECT::default();
            unsafe { GetWindowRect(h, &mut r) }.unwrap();
            (r.left, r.top)
        };
        assert!(open(args(), content(Status::Synced, true)));
        let h = hwnd().unwrap();
        assert_ne!(rect_of(h), (100, 200));
        // A monitor was unplugged or rearranged: Windows moves the window.
        unsafe { SetWindowPos(h, None, 100, 200, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) }
            .unwrap();
        // The next snapshot renders: the card stays where Windows put it.
        update(content(Status::Synced, true));
        assert_eq!(rect_of(h), (100, 200));
        // A drag's end saves where it really is.
        let mut post = Vec::new();
        assert!(with(|c, _| c.finish_move(&mut post)).is_some());
        let p = take_placed().unwrap();
        assert_eq!((p.left, p.top), (100, 200));
        close();
    }

    fn press(dragging: bool) -> Press {
        Press {
            zone: Zone::Header,
            line: None,
            start: (0, 0),
            origin: (0, 0),
            size_dip: (300.0, 420.0),
            dragging,
        }
    }

    #[test]
    fn a_capture_lost_mid_drag_saves_the_place_once() {
        assert!(open(args(), content(Status::Synced, true)));
        // A header, body or grip drag that already moved the window: saved.
        let mut post = Vec::new();
        let state = with(|c, _| {
            c.press = Some(press(true));
            c.capture_lost(&mut post);
            (c.press.is_none(), c.placed.is_some())
        })
        .unwrap();
        assert_eq!(state, (true, true));
        assert_eq!(post, vec![(Msg::Placed, 0)], "exactly one Placed");
        assert!(take_placed().is_some());
        // A press that never became a drag: nothing to save.
        let mut post = Vec::new();
        let state = with(|c, _| {
            c.press = Some(press(false));
            c.capture_lost(&mut post);
            (c.press.is_none(), c.placed.is_some())
        })
        .unwrap();
        assert_eq!(state, (true, false));
        assert!(post.is_empty());
        // No press at all (our own ReleaseCapture after a button release).
        let mut post = Vec::new();
        with(|c, _| c.capture_lost(&mut post)).unwrap();
        assert!(post.is_empty());
        assert!(take_placed().is_none());
        close();
    }

    #[test]
    fn wm_capturechanged_posts_placed_for_a_drag_and_never_twice() {
        use windows::Win32::UI::WindowsAndMessaging::SendMessageW;
        let island = fake_island();
        let mut a = args();
        a.island = island;
        a.demo = false;
        assert!(open(a, content(Status::Synced, true)));
        let h = hwnd().unwrap();
        let _ = with(|c, _| c.press = Some(press(true)));
        // Alt+Tab, the Win key or a UAC prompt takes the capture.
        unsafe { SendMessageW(h, WM_CAPTURECHANGED, None, None) };
        assert_eq!(posted(island), vec![Msg::Placed.code()]);
        assert!(take_placed().is_some());
        // The WM_CAPTURECHANGED of our own ReleaseCapture after a release: the
        // press is gone by then, so nothing more is posted.
        unsafe { SendMessageW(h, WM_CAPTURECHANGED, None, None) };
        assert!(posted(island).is_empty());
        // A click that is not a drag posts nothing either.
        let _ = with(|c, _| c.press = Some(press(false)));
        unsafe { SendMessageW(h, WM_CAPTURECHANGED, None, None) };
        assert!(posted(island).is_empty());
        close();
        unsafe {
            let _ = DestroyWindow(island);
        }
    }

    #[test]
    fn the_highlight_follows_the_list_under_a_resting_pointer() {
        assert!(open(args(), content(Status::Synced, true)));
        let now = Instant::now();
        let seen = with(|c, _| {
            let body = c.rects().body;
            let (tops, heights) = {
                let l = c.layouts.as_ref().unwrap();
                (l.tops.clone(), l.heights.clone())
            };
            let y = body.y + body.h / 2.0;
            c.hovering = true;
            c.last_pt = Some((50.0, y));
            let at = |c: &mut Card, line: usize| {
                // Scroll so that `line` sits under the pointer.
                let s = tops[line] + heights[line] / 2.0 - (y - body.y);
                c.scroll = Tween::at_rest(s, now);
                c.rehover(now);
                c.hover_line
            };
            let first = at(c, 0);
            let last = at(c, 2);
            let middle = at(c, 1);
            // The pointer left: no highlight, whatever the list does.
            c.hovering = false;
            let off = at(c, 2);
            // A held press keeps what it had.
            c.hovering = true;
            c.press = Some(press(false));
            c.hover_line = Some(1);
            let pressed = at(c, 2);
            c.press = None;
            // Without a position there is nothing to follow.
            c.last_pt = None;
            let none = at(c, 2);
            (first, last, middle, off, pressed, none, c.hover_zone)
        })
        .unwrap();
        assert_eq!(seen.0, Some(0));
        assert_eq!(seen.1, Some(2));
        assert_eq!(seen.2, Some(1));
        assert_eq!(seen.3, None);
        assert_eq!(seen.4, Some(1));
        assert_eq!(seen.5, None);
        assert_eq!(seen.6, Zone::Body);
        close();
    }

    #[test]
    fn a_tween_eases_and_jumps_when_asked_to() {
        let t0 = Instant::now();
        let mut t = Tween::at_rest(10.0, t0);
        assert!(!t.moving(t0));
        t.go(110.0, 350, true, t0);
        assert!(t.moving(t0));
        assert_eq!(t.at(t0), 10.0);
        assert!(t.at(t0 + std::time::Duration::from_millis(175)) > 60.0);
        assert_eq!(t.at(t0 + std::time::Duration::from_millis(350)), 110.0);
        assert!(!t.moving(t0 + std::time::Duration::from_millis(400)));
        // Reduced motion: at once.
        t.go(5.0, 350, false, t0);
        assert_eq!(t.at(t0), 5.0);
        assert!(!t.moving(t0));
    }
}
