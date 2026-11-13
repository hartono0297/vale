//! Shared hover tooltip: one small themed plate that follows what the pointer
//! rests on. `logic` decides when it shows and where it goes; this module
//! owns the window, the one thread timer and the drawing.
//!
//! An owner window calls `hover` on every pointer move over it (`None` when
//! the pointer is over nothing with a tip), `pressed` on any press and `hide`
//! when it hides, moves, resizes or loses the region. The plate never takes
//! the activation or a click (`WS_EX_NOACTIVATE`, `WS_EX_TRANSPARENT`). It is
//! created when it shows and destroyed when it hides; the timer exists only
//! while a tip is waited for or the 300 ms warm window runs. At idle there is
//! no window, no timer and no thread.

use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_CLIP, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{DWRITE_TEXT_METRICS, IDWriteTextLayout};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST, IsWindow, KillTimer,
    MA_NOACTIVATE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetTimer,
    SetWindowPos, ShowWindow, WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCDESTROY, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Interface, PCWSTR, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::arrange_overlay::Canvas;
use crate::capture::Rect;
use crate::config::UiFont;
use crate::hotkey::{HotkeyId, Hotkeys};
use crate::i18n::{self, Key, Lang};
use crate::shot_edit::gfx::{Gfx, color};
use crate::theme;
use crate::ui::geometry::Rect as FRect;
use crate::ui::menu_pop::{Text, monitor_at, paint_plate, register_class, rounded};

pub mod logic;

pub use logic::Side;
use logic::{GAP_DIP, MAX_W_DIP, Machine, Out, TipId, place};

/// Whether the plate is put on screen: always, except in tests.
const ON_SCREEN: bool = !cfg!(test);

const CLASS: PCWSTR = w!("ValeTooltip");

/// From the plate's edge to its text, across and down.
const PAD: f32 = 10.0;
const PAD_Y: f32 = 7.0;
/// Line 1's height and the key plate's inside it.
const LINE1_H: f32 = 18.0;
const KEY_H: f32 = 17.0;
const KEY_PAD: f32 = 6.0;
const KEY_GAP: f32 = 4.0;
/// Between the name and the first key plate.
const NAME_GAP: f32 = 12.0;
/// Between line 1 and the about line.
const LINE_GAP: f32 = 2.0;
const NAME_SIZE: f32 = 12.5;
const ABOUT_SIZE: f32 = 12.0;
const KEY_SIZE: f32 = 10.5;
const RADIUS: f32 = 8.0;
const KEY_RADIUS: f32 = 5.0;
/// The see-through margin round the plate that holds its shadow.
const SHADOW: f32 = 10.0;

// ------------------------------------------------------------------ content

/// A key or gesture written in the key plate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyHint {
    /// A hotkey of Settings, read from the live config at show time; none
    /// when it has no combo.
    Hotkey(HotkeyId),
    /// A fixed key or gesture in the current language.
    Fixed(Key),
    /// A key name that is the same in every language ("Esc").
    Raw(&'static str),
}

/// What a tip says: a name (line 1), what a click does (the about line) and
/// the keys that do the same.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tip {
    pub name: Option<String>,
    pub about: Option<String>,
    pub keys: Vec<KeyHint>,
}

impl Tip {
    pub fn named(key: Key) -> Tip {
        Tip::name_str(i18n::tr(key).to_string())
    }

    pub fn name_str(name: String) -> Tip {
        Tip {
            name: Some(name),
            ..Tip::default()
        }
    }

    /// The short form, for a button that already shows its label: only the
    /// description.
    pub fn short(key: Key) -> Tip {
        Tip::default().about(key)
    }

    pub fn about(self, key: Key) -> Tip {
        self.about_str(i18n::tr(key).to_string())
    }

    pub fn about_str(mut self, about: String) -> Tip {
        self.about = Some(about);
        self
    }

    pub fn key(mut self, key: KeyHint) -> Tip {
        self.keys.push(key);
        self
    }
}

/// Something with a tip: `id` tells it from the owner's other targets (see
/// `id_of`), `anchor` is where it is on screen, px. The id names the control,
/// never its state: a button that turns from Play into Pause keeps its id,
/// and `hover` repaints a shown tip whose content changed.
#[derive(Clone)]
pub struct Target {
    pub id: u64,
    pub anchor: Rect,
    pub side: Side,
    pub tip: Tip,
}

/// A stable id for a target built from a hit enum or any hashable value.
pub fn id_of<T: Hash>(t: &T) -> u64 {
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

/// `r` (dip from a window's top-left) on screen, px, for a window at `origin`
/// (px) and `scale`: for windows that keep their own screen origin.
pub fn anchor_px(origin: (i32, i32), r: FRect, scale: f32) -> Rect {
    Rect::new(
        origin.0 + (r.x * scale).round() as i32,
        origin.1 + (r.y * scale).round() as i32,
        (r.w * scale).round() as i32,
        (r.h * scale).round() as i32,
    )
}

/// A tip target before its rect is put on screen: a window builds it inside
/// its own borrow and maps it with `on_screen` once that has ended
/// (`ClientToScreen` can send messages back to the window).
pub struct TipSpec {
    pub id: u64,
    /// Client dip.
    pub rect: FRect,
    pub scale: f32,
    pub tip: Tip,
}

/// `spec` on screen, below its control (above when there is no room).
pub fn on_screen(hwnd: HWND, spec: TipSpec) -> Target {
    Target {
        id: spec.id,
        anchor: to_screen(hwnd, spec.rect, spec.scale),
        side: Side::Auto,
        tip: spec.tip,
    }
}

/// `r` (client dip) on screen, px.
pub fn to_screen(hwnd: HWND, r: FRect, scale: f32) -> Rect {
    let mut pt = POINT {
        x: (r.x * scale).round() as i32,
        y: (r.y * scale).round() as i32,
    };
    let _ = unsafe { ClientToScreen(hwnd, &mut pt) };
    Rect::new(
        pt.x,
        pt.y,
        (r.w * scale).round() as i32,
        (r.h * scale).round() as i32,
    )
}

// ------------------------------------------------------------------ pure

/// The key plate texts of `keys`: a hotkey with no combo is left out.
fn resolve_keys(keys: &[KeyHint], hk: &Hotkeys, lang: Lang) -> Vec<String> {
    keys.iter()
        .filter_map(|k| match k {
            KeyHint::Hotkey(id) => hk.get(*id).map(|h| h.to_string()),
            KeyHint::Fixed(key) => Some(i18n::text(lang, *key).to_string()),
            KeyHint::Raw(s) => Some((*s).to_string()),
        })
        .collect()
}

/// The plate's width, dip: the wider of line 1 and the about line, plus the
/// padding, at most `MAX_W_DIP`.
fn plate_w(line1_w: f32, about_w: f32) -> f32 {
    (line1_w.max(about_w) + 2.0 * PAD).min(MAX_W_DIP)
}

/// A `w_dip` by `h_dip` plate in px at `scale`.
fn plate_px(w_dip: f32, h_dip: f32, scale: f32) -> (i32, i32) {
    (
        (w_dip * scale).round() as i32,
        (h_dip * scale).round() as i32,
    )
}

// ------------------------------------------------------------------ look

/// A tip measured and ready to draw, dip.
struct Look {
    w: f32,
    h: f32,
    name: Option<String>,
    /// How wide the name may be before it is cut.
    name_room: f32,
    /// Each key plate's text and its width.
    keys: Vec<(String, f32)>,
    keys_w: f32,
    about: Option<IDWriteTextLayout>,
    about_y: f32,
    /// The about line is the only text (short form): it is in the text colour.
    about_is_main: bool,
}

fn layout_height(l: &IDWriteTextLayout) -> f32 {
    let mut m = DWRITE_TEXT_METRICS::default();
    unsafe { l.GetMetrics(&mut m) }.map_or(0.0, |()| m.height.ceil())
}

/// `tip` measured with `text`; `None` when it has nothing to say.
fn compose(text: &Text, tip: &Tip, keys: Vec<String>) -> Option<Look> {
    if tip.name.is_none() && tip.about.is_none() {
        return None;
    }
    let keys: Vec<(String, f32)> = keys
        .into_iter()
        .map(|k| {
            let w = (text.width(&k, KEY_SIZE, 600) + 2.0 * KEY_PAD).ceil();
            (k, w)
        })
        .collect();
    let keys_w =
        keys.iter().map(|(_, w)| w).sum::<f32>() + KEY_GAP * keys.len().saturating_sub(1) as f32;
    let key_gap = if keys.is_empty() { 0.0 } else { NAME_GAP };
    let about_full = tip
        .about
        .as_deref()
        .map_or(0.0, |a| text.width(a, ABOUT_SIZE, 400).ceil());
    if let Some(name) = &tip.name {
        let name_full = text.width(name, NAME_SIZE, 600).ceil();
        let w = plate_w(name_full + key_gap + keys_w, about_full);
        let about = tip
            .about
            .as_deref()
            .and_then(|a| text.block(a, ABOUT_SIZE, 400, w - 2.0 * PAD).ok());
        let about_h = about.as_ref().map_or(0.0, |l| LINE_GAP + layout_height(l));
        return Some(Look {
            w,
            h: 2.0 * PAD_Y + LINE1_H + about_h,
            name: Some(name.clone()),
            name_room: w - 2.0 * PAD - key_gap - keys_w,
            keys,
            keys_w,
            about,
            about_y: PAD_Y + LINE1_H + LINE_GAP,
            about_is_main: false,
        });
    }
    // Short form: the description is line 1, wrapped beside the key plates.
    let w = plate_w(about_full + key_gap + keys_w, 0.0);
    let room = w - 2.0 * PAD - key_gap - keys_w;
    let about = tip
        .about
        .as_deref()
        .and_then(|a| text.block(a, ABOUT_SIZE, 400, room).ok());
    let about_h = about.as_ref().map_or(0.0, layout_height);
    Some(Look {
        w,
        h: 2.0 * PAD_Y + about_h.max(LINE1_H),
        name: None,
        name_room: 0.0,
        keys,
        keys_w,
        about,
        about_y: PAD_Y + ((LINE1_H - about_h).max(0.0) / 2.0).floor(),
        about_is_main: true,
    })
}

/// Draws the plate (shadow, fill, hairline, text) `SHADOW` dip inside the
/// canvas.
fn paint(c: &Canvas, text: &Text, look: &Look, scale: f32) {
    let (rt, brush, _) = c.parts();
    let Ok(rt) = rt.cast::<ID2D1RenderTarget>() else {
        return;
    };
    let pal = theme::current_window();
    let set = |rgb: u32, a: f32| unsafe { brush.SetColor(&color(rgb, a)) };
    unsafe { rt.SetTransform(&Matrix3x2::scale(scale, scale)) };
    let plate = FRect {
        x: SHADOW,
        y: SHADOW,
        w: look.w,
        h: look.h,
    };
    paint_plate(&rt, brush, plate, RADIUS, &pal);
    unsafe {
        rt.PushAxisAlignedClip(
            &D2D_RECT_F {
                left: plate.x,
                top: plate.y,
                right: plate.x + plate.w,
                bottom: plate.y + plate.h,
            },
            D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        );
    }
    let x0 = SHADOW + PAD;
    let y0 = SHADOW + PAD_Y;
    if let Some(name) = &look.name
        && let Ok(l) = text.line(name, NAME_SIZE, 600, look.name_room, LINE1_H)
    {
        set(pal.text, 1.0);
        unsafe { rt.DrawTextLayout(Vector2::new(x0, y0), &l, brush, D2D1_DRAW_TEXT_OPTIONS_CLIP) };
    }
    if let Some(about) = &look.about {
        let ink = if look.about_is_main {
            pal.text
        } else {
            pal.secondary
        };
        set(ink, 1.0);
        unsafe {
            rt.DrawTextLayout(
                Vector2::new(x0, SHADOW + look.about_y),
                about,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
            )
        };
    }
    // The key plates, right-aligned on line 1.
    let mut x = SHADOW + look.w - PAD - look.keys_w;
    let y = y0 + (LINE1_H - KEY_H) / 2.0;
    for (k, kw) in &look.keys {
        let r = FRect {
            x,
            y,
            w: *kw,
            h: KEY_H,
        };
        set(pal.button, 1.0);
        unsafe { rt.FillRoundedRectangle(&rounded(r, KEY_RADIUS), brush) };
        if let Ok(l) = text.line(k, KEY_SIZE, 600, kw - 2.0 * KEY_PAD + 2.0, KEY_H) {
            set(pal.text, 1.0);
            unsafe {
                rt.DrawTextLayout(
                    Vector2::new(x + KEY_PAD, y),
                    &l,
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                )
            };
        }
        x += kw + KEY_GAP;
    }
    unsafe {
        rt.PopAxisAlignedClip();
        rt.SetTransform(&Matrix3x2::identity());
    }
}

// ------------------------------------------------------------------ state

struct State {
    machine: Machine,
    /// The newest target seen for the id the machine waits on or shows.
    pending: Option<(TipId, Target)>,
    /// What the plate on screen says, and for which target.
    shown: Option<(TipId, Tip)>,
    /// The plate on screen.
    window: Option<HWND>,
    /// The thread timer's id; 0 when none runs.
    timer: usize,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        machine: Machine::new(),
        pending: None,
        shown: None,
        window: None,
        timer: 0,
    });
    static HOTKEYS: Cell<Option<Hotkeys>> = const { Cell::new(None) };
    static FONT: Cell<UiFont> = const { Cell::new(UiFont::Sans) };
}

/// Runs `f` on the state; `None` when it is borrowed (a re-entrant call).
fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|cell| cell.try_borrow_mut().ok().map(|mut s| f(&mut s)))
}

/// The hotkeys the tips read at show time (the app's config copy).
pub fn set_hotkeys(h: Hotkeys) {
    HOTKEYS.with(|c| c.set(Some(h)));
}

/// The UI font the plate is drawn in (the app's config copy).
pub fn set_font(font: UiFont) {
    FONT.with(|c| c.set(font));
}

/// The pointer is over `target`, or over nothing with a tip. Call it on every
/// pointer move over `owner`.
pub fn hover(owner: HWND, target: Option<Target>) {
    if let Some((out, repaint)) = feed(owner, target) {
        apply(out);
        if let Some(id) = repaint {
            show_with(id, true);
        }
    }
}

/// What a hover asks for: the machine's answer, and the shown tip to repaint
/// in place when its target now says something else (Play became Pause under
/// a resting pointer). `None` when the state is borrowed.
fn feed(owner: HWND, target: Option<Target>) -> Option<(Out, Option<TipId>)> {
    let key = owner.0 as isize;
    with(|s| {
        let id = target.as_ref().map(|t| TipId {
            owner: key,
            id: t.id,
        });
        // Another window's tip is not this window's to hide.
        if id.is_none() && s.machine.current().is_some_and(|c| c.owner != key) {
            return (Out::default(), None);
        }
        let mut repaint = None;
        if let (Some(id), Some(t)) = (id, target) {
            let says_else = s.shown.as_ref().is_some_and(|(at, tip)| *at == id && *tip != t.tip);
            if says_else && s.machine.shown() == Some(id) {
                repaint = Some(id);
            }
            s.pending = Some((id, t));
        }
        (s.machine.hover(id), repaint)
    })
}

/// A mouse button went down on `owner`: the tip goes until the pointer leaves
/// the target.
pub fn pressed(owner: HWND) {
    act_on_owner(owner, Machine::press);
}

/// `owner` hid, moved, resized or lost the region the target was in.
pub fn hide(owner: HWND) {
    act_on_owner(owner, Machine::hide);
}

/// Everything goes: the theme or language changed, a capture starts.
pub fn hide_all() {
    let out = with(|s| {
        s.pending = None;
        s.machine.hide()
    });
    if let Some(out) = out {
        apply(out);
    }
}

fn act_on_owner(owner: HWND, f: impl FnOnce(&mut Machine) -> Out) {
    let key = owner.0 as isize;
    let out = with(|s| {
        if s.machine.current().is_some_and(|c| c.owner == key) {
            f(&mut s.machine)
        } else {
            Out::default()
        }
    });
    if let Some(out) = out {
        apply(out);
    }
}

/// Does what the machine asks: stop the timer, end the plate, arm the timer,
/// show a plate.
fn apply(out: Out) {
    if out.kill {
        kill_timer();
    }
    if out.hide {
        destroy_window();
    }
    if let Some(ms) = out.arm_ms {
        arm_timer(ms);
    }
    if let Some(id) = out.show {
        show(id);
    }
}

fn kill_timer() {
    if let Some(id) = with(|s| std::mem::take(&mut s.timer))
        && id != 0
    {
        let _ = unsafe { KillTimer(None, id) };
    }
}

/// One timer, re-armed in place: a new wait replaces the old one.
fn arm_timer(ms: u32) {
    kill_timer();
    if !ON_SCREEN {
        return;
    }
    let id = unsafe { SetTimer(None, 0, ms, Some(timer_proc)) };
    let _ = with(|s| s.timer = id);
}

unsafe extern "system" fn timer_proc(_: HWND, _: u32, _: usize, _: u32) {
    // One-shot: whatever the machine says, this firing is spent.
    kill_timer();
    if let Some(out) = with(|s| s.machine.timer()) {
        apply(out);
    }
}

fn destroy_window() {
    if let Some(Some(hwnd)) = with(|s| s.window.take()) {
        let _ = unsafe { DestroyWindow(hwnd) };
    }
}

// ------------------------------------------------------------------ window

unsafe extern "system" fn tip_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCDESTROY => {
            // The owner went and took the plate with it: nothing is up now.
            let out = with(|s| {
                (s.window == Some(hwnd)).then(|| {
                    s.window = None;
                    s.pending = None;
                    s.machine.hide()
                })
            })
            .flatten();
            if let Some(out) = out {
                apply(out);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Puts the plate for `id` on screen, in place of any that is up.
fn show(id: TipId) {
    show_with(id, false);
}

/// Puts the plate for `id` on screen. `in_place`: the plate up is `id`'s and
/// is painted again where it is, with no hide in between. When it cannot,
/// the machine is reset: it must not think a tip is up.
fn show_with(id: TipId, in_place: bool) {
    let old = with(|s| s.window.take()).flatten();
    let keep = if in_place {
        old
    } else {
        if let Some(hwnd) = old {
            let _ = unsafe { DestroyWindow(hwnd) };
        }
        None
    };
    if try_show(id, keep).is_some() {
        return;
    }
    if let Some(hwnd) = keep
        && unsafe { IsWindow(Some(hwnd)) }.as_bool()
    {
        let _ = unsafe { DestroyWindow(hwnd) };
    }
    let out = with(|s| {
        s.pending = None;
        s.shown = None;
        s.machine.hide()
    });
    if let Some(out) = out {
        apply(out);
    }
}

/// Shows `id`'s newest target: painted into `keep` (the plate already up for
/// it) when given, else into a new window.
fn try_show(id: TipId, keep: Option<HWND>) -> Option<()> {
    let target = with(|s| s.pending.take_if(|(p, _)| *p == id).map(|(_, t)| t))??;
    if !ON_SCREEN {
        let _ = with(|s| s.shown = Some((id, target.tip)));
        return Some(());
    }
    let owner = HWND(id.owner as *mut _);
    if !unsafe { IsWindow(Some(owner)) }.as_bool() || !register_class(CLASS, tip_proc) {
        return None;
    }
    let hk = HOTKEYS.with(Cell::get).unwrap_or_else(Hotkeys::none);
    let keys = resolve_keys(&target.tip.keys, &hk, i18n::lang());
    let Ok(gfx) = Gfx::new() else { return None };
    let gfx = Rc::new(gfx);
    let text = Text::new(gfx.dwrite.clone(), FONT.with(Cell::get));
    let Some(look) = compose(&text, &target.tip, keys) else {
        return None;
    };
    let a = target.anchor;
    let (work, scale) = monitor_at((a.x + a.w / 2, a.y + a.h / 2));
    let (pw, ph) = plate_px(look.w, look.h, scale);
    let gap = (GAP_DIP * scale).round() as i32;
    let plate = place(a, pw, ph, work, gap, target.side);
    let margin = (SHADOW * scale).round() as i32;
    let origin = (plate.x - margin, plate.y - margin);
    let size = (plate.w + 2 * margin, plate.h + 2 * margin);
    let hwnd = match keep {
        Some(hwnd) if unsafe { IsWindow(Some(hwnd)) }.as_bool() => hwnd,
        _ => create_plate(owner, origin, size)?,
    };
    let painted = Canvas::with_gfx(gfx, size).and_then(|canvas| {
        canvas.paint_with(hwnd, origin, 0.0, 255, |c| paint(c, &text, &look, scale))
    });
    if painted.is_err() {
        let _ = unsafe { DestroyWindow(hwnd) };
        return None;
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        );
    }
    let _ = with(|s| {
        s.window = Some(hwnd);
        s.shown = Some((id, target.tip));
    });
    Some(())
}

/// A new, empty plate window for `owner` at `origin`, `size` px.
fn create_plate(owner: HWND, origin: (i32, i32), size: (i32, i32)) -> Option<HWND> {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return None;
    };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS,
            w!("Vale tooltip"),
            WS_POPUP,
            origin.0,
            origin.1,
            size.0,
            size.1,
            Some(owner),
            None,
            Some(module.into()),
            None,
        )
    };
    hwnd.ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::{HotkeyId, Hotkeys, parse};
    use crate::i18n::{Key, Lang};

    #[test]
    fn resolve_keys_reads_live_hotkeys() {
        let keys = [
            KeyHint::Hotkey(HotkeyId::LyricsCard),
            KeyHint::Hotkey(HotkeyId::StickyList),
            KeyHint::Raw("Esc"),
        ];
        let mut hk = Hotkeys::none();
        hk.lyrics_card = parse("Ctrl+Alt+L").unwrap();
        assert_eq!(resolve_keys(&keys, &hk, Lang::En), ["Ctrl+Alt+L", "Esc"]);
        hk.lyrics_card = None;
        assert_eq!(resolve_keys(&keys, &hk, Lang::En), ["Esc"]);
    }

    #[test]
    fn resolve_keys_fixed_follows_language() {
        let keys = [KeyHint::Fixed(Key::TipKeySpace)];
        let hk = Hotkeys::none();
        assert_eq!(resolve_keys(&keys, &hk, Lang::Id), ["Spasi"]);
        assert_eq!(resolve_keys(&keys, &hk, Lang::En), ["Space"]);
    }

    #[test]
    fn plate_w_caps_at_max() {
        assert_eq!(plate_w(40.0, 60.0), 60.0 + 2.0 * PAD);
        assert_eq!(plate_w(500.0, 10.0), MAX_W_DIP);
    }

    #[test]
    fn plate_px_uses_given_scale() {
        assert_eq!(plate_px(200.0, 40.0, 1.5), (300, 60));
    }

    #[test]
    fn id_of_is_stable() {
        assert_eq!(id_of(&3u8), id_of(&3u8));
        assert_ne!(id_of(&3u8), id_of(&4u8));
    }

    fn fake(n: usize) -> HWND {
        HWND(n as *mut _)
    }

    fn target(id: u64) -> Option<Target> {
        Some(Target {
            id,
            anchor: Rect::new(10, 10, 20, 20),
            side: Side::Auto,
            tip: Tip::short(Key::SearchBack),
        })
    }

    fn current() -> Option<TipId> {
        with(|s| s.machine.current()).flatten()
    }

    #[test]
    fn another_owner_reporting_nothing_leaves_the_tip() {
        let (a, b) = (fake(0x1000), fake(0x2000));
        hover(a, target(1));
        let id = current();
        assert_eq!(
            id,
            Some(TipId {
                owner: 0x1000,
                id: 1
            })
        );
        hover(b, None);
        assert_eq!(current(), id);
        hover(a, None);
        assert_eq!(current(), None);
    }

    #[test]
    fn hide_and_pressed_act_only_for_the_tips_owner() {
        let (a, b) = (fake(0x1000), fake(0x2000));
        hover(a, target(1));
        let id = current();
        hide(b);
        pressed(b);
        assert_eq!(current(), id);
        pressed(a);
        assert_eq!(current(), id, "a press keeps the id, blocking until leave");
        hide(a);
        assert_eq!(current(), None);
    }

    fn target_with(id: u64, tip: Tip) -> Option<Target> {
        Some(Target {
            id,
            anchor: Rect::new(10, 10, 20, 20),
            side: Side::Auto,
            tip,
        })
    }

    /// The one-shot timer runs out.
    fn fire() {
        unsafe { timer_proc(HWND::default(), 0, 0, 0) };
    }

    fn shown_tip() -> Option<(TipId, Tip)> {
        with(|s| s.shown.clone()).flatten()
    }

    #[test]
    fn new_content_on_the_shown_target_is_repainted_in_place() {
        let a = fake(0x3000);
        let id = TipId {
            owner: 0x3000,
            id: 1,
        };
        hover(a, target_with(1, Tip::named(Key::TipPlay)));
        fire();
        assert_eq!(shown_tip(), Some((id, Tip::named(Key::TipPlay))));
        // Play turned into Pause under a resting pointer: the same target.
        let (out, repaint) = feed(a, target_with(1, Tip::named(Key::TipPause))).unwrap();
        assert_eq!(out, Out::default(), "no hide and no new wait");
        assert_eq!(repaint, Some(id));
        hover(a, target_with(1, Tip::named(Key::TipPause)));
        assert_eq!(current(), Some(id));
        assert_eq!(shown_tip(), Some((id, Tip::named(Key::TipPause))));
        // The same content again: nothing to do.
        let fed = feed(a, target_with(1, Tip::named(Key::TipPause))).unwrap();
        assert_eq!(fed, (Out::default(), None));
    }

    #[test]
    fn new_content_while_waiting_or_clicked_shows_nothing_early() {
        let a = fake(0x3000);
        hover(a, target_with(1, Tip::named(Key::TipPlay)));
        let fed = feed(a, target_with(1, Tip::named(Key::TipPause))).unwrap();
        assert_eq!(fed, (Out::default(), None), "still waiting");
        fire();
        let shown = shown_tip().map(|(_, t)| t);
        assert_eq!(shown, Some(Tip::named(Key::TipPause)), "the newest content shows");
        // A click flips the button; its tip stays away until the pointer leaves.
        pressed(a);
        let fed = feed(a, target_with(1, Tip::named(Key::TipPlay))).unwrap();
        assert_eq!(fed, (Out::default(), None));
    }

    #[test]
    fn anchor_px_is_the_dip_rect_on_screen() {
        let r = FRect {
            x: 10.0,
            y: 4.0,
            w: 20.0,
            h: 8.0,
        };
        assert_eq!(anchor_px((100, 0), r, 1.5), Rect::new(115, 6, 30, 12));
        assert_eq!(anchor_px((-1920, 0), r, 1.0), Rect::new(-1910, 4, 20, 8));
    }

    #[test]
    fn hide_all_clears_whatever_the_owner() {
        hover(fake(0x1000), target(1));
        assert!(current().is_some());
        hide_all();
        assert_eq!(current(), None);
    }
}
