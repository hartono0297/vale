//! The colour popover of a sticky note: a small layered popup owned by the
//! note, below its colour button, clamped to the work area. Six swatches, the
//! text size (S, M, L), the font (Vale, Tangan, Mono), the paper's ruling, the
//! decoration, "Tembus pandang", "Sematkan di atas" and "Kucing ngintip". It closes on
//! Esc, on a click outside or when it is deactivated, and it follows the
//! Windows palette of Vale's window theme (`theme::current_window`, Terminal
//! included).

use std::cell::RefCell;

use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, IDC_ARROW,
    LoadCursorW, RegisterClassExW, SW_SHOW, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Interface, PCWSTR, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::arrange_overlay::{Canvas, take_foreground};
use crate::capture::Rect;
use crate::config::UiFont;
use crate::i18n::{Key, Lang, text};
use crate::shot_edit::gfx::color;
use crate::theme;
use crate::ui::geometry::Rect as FRect;
use crate::widgets::host;
use crate::widgets::paint::Res;
use crate::widgets::sticky::{Deco, NoteFont, OPACITIES, Paper, Pattern, TextSize};

const CLASS: PCWSTR = w!("ValeWidgetPop");
const W: f32 = 292.0;
const PAD: f32 = 14.0;
const SWATCH: f32 = 24.0;
const ROW_H: f32 = 34.0;
/// The rows under the swatches: the five segmented ones, then the switches.
const ROWS: usize = 7;
const H: f32 = PAD + SWATCH + 10.0 + ROWS as f32 * ROW_H + 6.0;

/// What a pick in the popover does to its note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopCmd {
    Paper(Paper),
    Size(TextSize),
    Font(NoteFont),
    Pattern(Pattern),
    Deco(Deco),
    Opacity(u8),
    Pin(bool),
    Peek(bool),
}

/// What the popover shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PopState {
    pub paper: Paper,
    pub size: TextSize,
    pub font: NoteFont,
    pub pattern: Pattern,
    pub deco: Deco,
    pub opacity: u8,
    pub pinned: bool,
    pub peek: bool,
    pub lang: Lang,
    /// Vale's UI font (the popover's own text).
    pub ui: UiFont,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Swatch(Paper),
    Size(TextSize),
    Font(NoteFont),
    Pattern(Pattern),
    Deco(Deco),
    Opacity(u8),
    Pin,
    Peek,
}

struct Pop {
    hwnd: HWND,
    note: HWND,
    origin: (i32, i32),
    scale: f32,
    state: PopState,
    res: Res,
    canvas: Option<Canvas>,
    hover: Option<Item>,
    /// `close` is running (Esc, a command of the note, a new popover): the
    /// deactivation that follows is ours, not the user's going elsewhere.
    closing: bool,
    /// Deactivated in favour of a window that is not the note: the user went
    /// to another app, so the note must not take the keyboard back.
    away: bool,
}

thread_local! {
    static POP: RefCell<Option<Pop>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Pop) -> R) -> Option<R> {
    POP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

pub fn is_open() -> bool {
    POP.with(|cell| cell.try_borrow().map(|p| p.is_some()).unwrap_or(true))
}

/// Whether the popover is open, and for `note`.
pub fn is_open_for(note: HWND) -> bool {
    POP.with(|cell| {
        cell.try_borrow()
            .map(|p| p.as_ref().is_some_and(|p| p.note == note))
            .unwrap_or(false)
    })
}

/// Whether `h` is the popover of `note`.
pub fn is_popover_of(h: HWND, note: HWND) -> bool {
    POP.with(|cell| {
        cell.try_borrow()
            .map(|p| p.as_ref().is_some_and(|p| p.hwnd == h && p.note == note))
            .unwrap_or(false)
    })
}

// ------------------------------------------------------------------ layout

fn swatch_rect(i: usize) -> FRect {
    let gap = (W - 2.0 * PAD - 6.0 * SWATCH) / 5.0;
    FRect {
        x: PAD + i as f32 * (SWATCH + gap),
        y: PAD,
        w: SWATCH,
        h: SWATCH,
    }
}

fn row_y(n: usize) -> f32 {
    PAD + SWATCH + 10.0 + n as f32 * ROW_H
}

/// The rows: 0 text size, 1 font, 2 paper, 3 decoration, 4 see-through (all
/// segmented), 5 pin on top, 6 the cat.
const ROW_SIZE: usize = 0;
const ROW_FONT: usize = 1;
const ROW_PATTERN: usize = 2;
const ROW_DECO: usize = 3;
const ROW_OPACITY: usize = 4;
const ROW_PIN: usize = 5;
const ROW_PEEK: usize = 6;

/// How wide one segment of a segmented row is, dip.
fn seg_w(row: usize) -> f32 {
    match row {
        ROW_SIZE => 34.0,
        ROW_FONT | ROW_DECO => 62.0,
        ROW_PATTERN => 46.0,
        _ => 38.0,
    }
}

/// How many segments the row has.
fn seg_n(row: usize) -> usize {
    match row {
        ROW_SIZE | ROW_FONT | ROW_DECO => 3,
        _ => 4,
    }
}

/// Segment `i` of segmented row `row`, right-aligned in the row.
fn seg_rect(row: usize, i: usize) -> FRect {
    let (w, n) = (seg_w(row), seg_n(row));
    FRect {
        x: W - PAD - n as f32 * w + i as f32 * w,
        y: row_y(row) + (ROW_H - 24.0) / 2.0,
        w,
        h: 24.0,
    }
}

/// The track behind a segmented row's segments.
fn seg_track(row: usize) -> FRect {
    let first = seg_rect(row, 0);
    FRect {
        x: first.x - 2.0,
        y: first.y - 2.0,
        w: seg_w(row) * seg_n(row) as f32 + 4.0,
        h: 28.0,
    }
}

fn toggle_rect(row: usize) -> FRect {
    FRect {
        x: W - PAD - 36.0,
        y: row_y(row) + (ROW_H - 20.0) / 2.0,
        w: 36.0,
        h: 20.0,
    }
}

fn row_rect(row: usize) -> FRect {
    FRect {
        x: 6.0,
        y: row_y(row),
        w: W - 12.0,
        h: ROW_H,
    }
}

fn inside(r: FRect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// The item of segmented row `row`'s segment `i`.
fn seg_item(row: usize, i: usize) -> Item {
    match row {
        ROW_SIZE => Item::Size(TextSize::ALL[i]),
        ROW_FONT => Item::Font(NoteFont::ALL[i]),
        ROW_PATTERN => Item::Pattern(Pattern::ALL[i]),
        ROW_DECO => Item::Deco(Deco::ALL[i]),
        _ => Item::Opacity(OPACITIES[i]),
    }
}

fn item_at(x: f32, y: f32) -> Option<Item> {
    for (i, p) in Paper::ALL.iter().enumerate() {
        let r = swatch_rect(i);
        let grown = FRect {
            x: r.x - 3.0,
            y: r.y - 3.0,
            w: r.w + 6.0,
            h: r.h + 6.0,
        };
        if inside(grown, x, y) {
            return Some(Item::Swatch(*p));
        }
    }
    for row in ROW_SIZE..=ROW_OPACITY {
        for i in 0..seg_n(row) {
            if inside(seg_rect(row, i), x, y) {
                return Some(seg_item(row, i));
            }
        }
    }
    if inside(row_rect(ROW_PIN), x, y) {
        return Some(Item::Pin);
    }
    if inside(row_rect(ROW_PEEK), x, y) {
        return Some(Item::Peek);
    }
    None
}

/// What `item` shows as the current choice in `st`.
fn is_current(item: Item, st: &PopState) -> bool {
    match item {
        Item::Size(v) => st.size == v,
        Item::Font(v) => st.font == v,
        Item::Pattern(v) => st.pattern == v,
        Item::Deco(v) => st.deco == v,
        Item::Opacity(v) => st.opacity == v,
        _ => false,
    }
}

/// The label of a segment.
fn seg_label(item: Item, lang: Lang) -> String {
    match item {
        Item::Size(v) => v.name().to_owned(),
        Item::Font(v) => text(lang, v.label_key()).to_owned(),
        Item::Pattern(v) => text(lang, v.label_key()).to_owned(),
        Item::Deco(v) => text(lang, v.label_key()).to_owned(),
        Item::Opacity(v) => format!("{v}%"),
        _ => String::new(),
    }
}

/// The label at the left of segmented row `row`.
fn row_label(row: usize) -> Key {
    match row {
        ROW_SIZE => Key::StickyTextSize,
        ROW_FONT => Key::StickyFont,
        ROW_PATTERN => Key::StickyPattern,
        ROW_DECO => Key::StickyDeco,
        _ => Key::StickyOpacity,
    }
}

// ------------------------------------------------------------------ window

fn register() -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(pop_proc),
        hInstance: module.into(),
        hCursor: cursor,
        lpszClassName: CLASS,
        ..Default::default()
    };
    unsafe {
        if RegisterClassExW(&wc) == 0 {
            return windows::core::Error::from_thread().code()
                == ERROR_CLASS_ALREADY_EXISTS.to_hresult();
        }
    }
    true
}

/// Opens the popover below `anchor` (the colour button, px) for `note`,
/// right-aligned to it and kept inside `area` (the placement area, px).
pub fn open(note: HWND, anchor: Rect, area: Rect, scale: f32, state: PopState) {
    close();
    if !register() {
        return;
    }
    let Ok(res) = Res::new(state.ui) else { return };
    let (w, h) = ((W * scale).round() as i32, (H * scale).round() as i32);
    let gap = (4.0 * scale).round() as i32;
    let x = (anchor.right() - w).clamp(area.x, (area.right() - w).max(area.x));
    let below = anchor.bottom() + gap;
    let y = if below + h <= area.bottom() {
        below
    } else {
        (anchor.y - gap - h).max(area.y)
    };
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return;
    };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            CLASS,
            w!("Vale sticky note colours"),
            WS_POPUP,
            x,
            y,
            w,
            h,
            Some(note),
            None,
            Some(module.into()),
            None,
        )
    };
    let Ok(hwnd) = hwnd else { return };
    let canvas = Canvas::with_gfx(res.gfx.clone(), (w, h)).ok();
    POP.with(|cell| {
        *cell.borrow_mut() = Some(Pop {
            hwnd,
            note,
            origin: (x, y),
            scale,
            state,
            res,
            canvas,
            hover: None,
            closing: false,
            away: false,
        });
    });
    redraw();
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    take_foreground(hwnd);
}

/// A segment's label, centred in its rect.
fn seg_text(
    res: &Res,
    rt: ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    s: &str,
    r: FRect,
    ink: u32,
) {
    let Ok(l) = res.layout(s, 11.5, 500, r.w) else { return };
    unsafe {
        let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = l.SetMaxHeight(r.h);
        let _ = l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        let _ = l.SetTextAlignment(windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER);
        brush.SetColor(&color(ink, 1.0));
        rt.DrawTextLayout(Vector2::new(r.x, r.y), &l, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
    }
}

/// The note's state changed: the popover shows the new one.
pub fn update(state: PopState) {
    let _ = with(|p| p.state = state);
    redraw();
}

/// Whether the popover losing the activation to `to` (0 for none) is the
/// user going somewhere else: not the note it belongs to, and not our own
/// `close`.
fn left_for_other(to: isize, note: isize, closing: bool) -> bool {
    !closing && to != note
}

pub fn close() {
    let _ = with(|p| p.closing = true);
    let hwnd = POP.with(|cell| {
        cell.try_borrow()
            .ok()
            .and_then(|p| p.as_ref().map(|p| p.hwnd))
    });
    if let Some(h) = hwnd {
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

fn redraw() {
    let _ = with(|p| {
        let pal = theme::current_window();
        let k = p.scale;
        let Some(canvas) = &p.canvas else { return };
        let island = theme::island(theme::current().accent, theme::current().surface);
        let st = p.state;
        let hover = p.hover;
        let res = &p.res;
        let _ = canvas.paint_with(p.hwnd, p.origin, 0.0, 255, |c| {
            let (rt, brush, _) = c.parts();
            let Ok(rt) = rt.cast::<ID2D1RenderTarget>() else {
                return;
            };
            let set = |c: u32, a: f32| unsafe { brush.SetColor(&color(c, a)) };
            let rr = |r: FRect, rad: f32| D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F {
                    left: r.x,
                    top: r.y,
                    right: r.x + r.w,
                    bottom: r.y + r.h,
                },
                radiusX: rad,
                radiusY: rad,
            };
            unsafe { rt.SetTransform(&Matrix3x2::scale(k, k)) };
            // The plate, with a hairline.
            let whole = FRect { x: 0.5, y: 0.5, w: W - 1.0, h: H - 1.0 };
            set(pal.popover, 1.0);
            unsafe { rt.FillRoundedRectangle(&rr(whole, 12.0), brush) };
            set(pal.border >> 8, (pal.border & 0xFF) as f32 / 255.0 * 2.0);
            unsafe { rt.DrawRoundedRectangle(&rr(whole, 12.0), brush, 1.0, None) };
            let label = |s: &str, x: f32, y: f32, w: f32, h: f32, size: f32, weight: u16, ink: u32, right: bool| {
                let Ok(l) = res.layout(s, size, weight, w) else { return };
                unsafe {
                    let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    let _ = l.SetMaxHeight(h);
                    let _ = l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                    if right {
                        let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING);
                    }
                    brush.SetColor(&color(ink, 1.0));
                    rt.DrawTextLayout(Vector2::new(x, y), &l, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                }
            };
            // Swatches.
            for (i, paper) in Paper::ALL.iter().enumerate() {
                let r = swatch_rect(i);
                let tones = crate::widgets::sticky::tones(*paper, &island, theme::current().accent);
                let centre = Vector2::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
                let dot = D2D1_ELLIPSE { point: centre, radiusX: r.w / 2.0, radiusY: r.h / 2.0 };
                set(tones.fill, 1.0);
                unsafe { rt.FillEllipse(&dot, brush) };
                set(pal.text, 0.22);
                unsafe { rt.DrawEllipse(&dot, brush, 1.0, None) };
                if *paper == Paper::Gelap {
                    set(island.accent, 1.0);
                    unsafe {
                        rt.DrawLine(
                            Vector2::new(r.x + 7.0, r.y + 6.0),
                            Vector2::new(r.x + r.w - 7.0, r.y + 6.0),
                            brush,
                            2.0,
                            None,
                        );
                    }
                }
                if st.paper == *paper {
                    set(pal.accent, 1.0);
                    let ring = D2D1_ELLIPSE { point: centre, radiusX: r.w / 2.0 + 3.0, radiusY: r.h / 2.0 + 3.0 };
                    unsafe { rt.DrawEllipse(&ring, brush, 2.0, None) };
                } else if hover == Some(Item::Swatch(*paper)) {
                    set(pal.text, 0.35);
                    let ring = D2D1_ELLIPSE { point: centre, radiusX: r.w / 2.0 + 3.0, radiusY: r.h / 2.0 + 3.0 };
                    unsafe { rt.DrawEllipse(&ring, brush, 1.5, None) };
                }
            }
            // The five segmented rows: size, font, paper, decoration,
            // see-through. Each has its label at the left.
            for row in ROW_SIZE..=ROW_OPACITY {
                label(text(st.lang, row_label(row)), PAD, row_y(row), W - 2.0 * PAD - seg_track(row).w - 6.0, ROW_H, 12.0, 500, pal.text, false);
                set(pal.button, 1.0);
                unsafe { rt.FillRoundedRectangle(&rr(seg_track(row), 9.0), brush) };
                for i in 0..seg_n(row) {
                    let r = seg_rect(row, i);
                    let item = seg_item(row, i);
                    let on = is_current(item, &st);
                    if on {
                        set(pal.accent, 1.0);
                        unsafe { rt.FillRoundedRectangle(&rr(r, 7.0), brush) };
                    } else if hover == Some(item) {
                        set(pal.button_hover, 1.0);
                        unsafe { rt.FillRoundedRectangle(&rr(r, 7.0), brush) };
                    }
                    let ink = if on { pal.on_accent } else { pal.text };
                    seg_text(res, rt.clone(), brush, &seg_label(item, st.lang), r, ink);
                }
            }
            // The two switches.
            for (row, item, key, on) in [
                (ROW_PIN, Item::Pin, Key::StickyPinTop, st.pinned),
                (ROW_PEEK, Item::Peek, Key::WidgetsPeek, st.peek),
            ] {
                if hover == Some(item) {
                    set(pal.hover, 1.0);
                    unsafe { rt.FillRoundedRectangle(&rr(row_rect(row), 8.0), brush) };
                }
                label(text(st.lang, key), PAD, row_y(row), W - 2.0 * PAD - 44.0, ROW_H, 12.0, 400, pal.text, false);
                let t = toggle_rect(row);
                set(if on { pal.accent } else { pal.button_hover }, 1.0);
                unsafe { rt.FillRoundedRectangle(&rr(t, 10.0), brush) };
                let knob_x = if on { t.x + t.w - 10.0 - 2.0 } else { t.x + 10.0 + 2.0 };
                set(if on { pal.on_accent } else { pal.secondary }, 1.0);
                unsafe {
                    rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: Vector2::new(knob_x, t.y + t.h / 2.0),
                            radiusX: 7.0,
                            radiusY: 7.0,
                        },
                        brush,
                    );
                }
            }
            unsafe { rt.SetTransform(&Matrix3x2::identity()) };
        });
    });
}

fn to_dip(p: &Pop, x: i32, y: i32) -> (f32, f32) {
    (x as f32 / p.scale, y as f32 / p.scale)
}

unsafe extern "system" fn pop_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEMOVE => {
            let (x, y) = (
                (lparam.0 & 0xFFFF) as i16 as i32,
                ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
            );
            let changed = with(|p| {
                let (dx, dy) = to_dip(p, x, y);
                let item = item_at(dx, dy);
                let changed = p.hover != item;
                p.hover = item;
                changed
            })
            .unwrap_or(false);
            if changed {
                redraw();
            }
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (
                (lparam.0 & 0xFFFF) as i16 as i32,
                ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
            );
            let pick = with(|p| {
                let (dx, dy) = to_dip(p, x, y);
                item_at(dx, dy).map(|item| {
                    let cmd = match item {
                        Item::Swatch(paper) => PopCmd::Paper(paper),
                        Item::Size(s) => PopCmd::Size(s),
                        Item::Font(f) => PopCmd::Font(f),
                        Item::Pattern(p) => PopCmd::Pattern(p),
                        Item::Deco(d) => PopCmd::Deco(d),
                        Item::Opacity(o) => PopCmd::Opacity(o),
                        Item::Pin => PopCmd::Pin(!p.state.pinned),
                        Item::Peek => PopCmd::Peek(!p.state.peek),
                    };
                    (p.note, cmd)
                })
            })
            .flatten();
            if let Some((note, cmd)) = pick {
                host::pop_command(note, cmd);
            }
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            if (wparam.0 & 0xFFFF) as u16 == VK_ESCAPE.0 {
                close();
            }
            return LRESULT(0);
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                let to = lparam.0;
                let _ = with(|p| {
                    if left_for_other(to, p.note.0 as isize, p.closing) {
                        p.away = true;
                    }
                });
                close();
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            let gone = POP.with(|cell| {
                cell.try_borrow_mut()
                    .ok()
                    .and_then(|mut p| p.take().map(|p| (p.note, p.away)))
            });
            if let Some((note, away)) = gone {
                host::pop_closed(note, away);
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
    use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS;

    fn width(res: &Res, s: &str, size: f32, weight: u16) -> f32 {
        let l = res.layout(s, size, weight, 1000.0).expect("layout");
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { l.GetMetrics(&mut m) }.expect("metrics");
        m.widthIncludingTrailingWhitespace
    }

    #[test]
    fn the_popover_holds_its_rows_inside_the_plate_and_clear_of_each_other() {
        let mut rects = Vec::new();
        for i in 0..6 {
            rects.push(swatch_rect(i));
        }
        for row in ROW_SIZE..=ROW_OPACITY {
            for i in 0..seg_n(row) {
                rects.push(seg_rect(row, i));
            }
        }
        rects.push(toggle_rect(ROW_PIN));
        rects.push(toggle_rect(ROW_PEEK));
        for r in &rects {
            assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= W && r.y + r.h <= H, "{r:?}");
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
        // Every row has a hit item; a segment is its own item.
        for row in ROW_SIZE..=ROW_OPACITY {
            for i in 0..seg_n(row) {
                let r = seg_rect(row, i);
                assert_eq!(item_at(r.x + 2.0, r.y + 2.0), Some(seg_item(row, i)));
            }
        }
        assert_eq!(item_at(row_rect(ROW_PIN).x + 4.0, row_y(ROW_PIN) + 4.0), Some(Item::Pin));
        assert_eq!(item_at(row_rect(ROW_PEEK).x + 4.0, row_y(ROW_PEEK) + 4.0), Some(Item::Peek));
        assert!(H > 250.0, "the plate grew for the new rows: {H}");
    }

    #[test]
    fn every_label_fits_its_segment_and_every_row_label_its_room_in_both_languages() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        for lang in [Lang::Id, Lang::En] {
            for row in ROW_SIZE..=ROW_OPACITY {
                for i in 0..seg_n(row) {
                    let item = seg_item(row, i);
                    let label = seg_label(item, lang);
                    let w = width(&res, &label, 11.5, 500);
                    assert!(w + 8.0 <= seg_w(row), "{lang:?} {label:?}: {w} in {}", seg_w(row));
                }
                let label = text(lang, row_label(row));
                let room = W - 2.0 * PAD - seg_track(row).w - 6.0;
                let w = width(&res, label, 12.0, 500);
                assert!(w <= room, "{lang:?} {label:?}: {w} in {room}");
            }
            for key in [Key::StickyPinTop, Key::WidgetsPeek] {
                let w = width(&res, text(lang, key), 12.0, 400);
                assert!(w <= W - 2.0 * PAD - 44.0, "{lang:?} {key:?}: {w}");
            }
        }
    }

    #[test]
    fn the_current_choice_is_the_one_marked() {
        let st = PopState {
            paper: Paper::Krem,
            size: TextSize::L,
            font: NoteFont::Mono,
            pattern: Pattern::Titik,
            deco: Deco::Paku,
            opacity: 40,
            pinned: false,
            peek: true,
            lang: Lang::Id,
            ui: UiFont::Sans,
        };
        for row in ROW_SIZE..=ROW_OPACITY {
            let current: Vec<usize> = (0..seg_n(row))
                .filter(|i| is_current(seg_item(row, *i), &st))
                .collect();
            assert_eq!(current.len(), 1, "row {row}: exactly one is on");
        }
        assert!(is_current(Item::Size(TextSize::L), &st));
        assert!(is_current(Item::Font(NoteFont::Mono), &st));
        assert!(is_current(Item::Opacity(40), &st));
        assert!(!is_current(Item::Opacity(100), &st));
        assert!(!is_current(Item::Pin, &st));
        // The opacity choices are the four allowed ones, in order.
        let vals: Vec<u8> = (0..4)
            .map(|i| match seg_item(ROW_OPACITY, i) {
                Item::Opacity(o) => o,
                _ => 0,
            })
            .collect();
        assert_eq!(vals, [100, 80, 60, 40]);
    }

    #[test]
    fn going_to_another_app_is_not_a_return_to_the_note() {
        // A click into another app (a window that is not the note).
        assert!(left_for_other(0x2222, 0x1111, false));
        // No window at all (the desktop, a lock screen).
        assert!(left_for_other(0, 0x1111, false));
        // A click on the note itself: the editing goes on.
        assert!(!left_for_other(0x1111, 0x1111, false));
        // Esc or a command of ours: close() runs, whoever gets the activation.
        assert!(!left_for_other(0x2222, 0x1111, true));
    }
}
