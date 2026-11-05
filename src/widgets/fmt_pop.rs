//! The format bar's two popups, one layered window with two modes: the
//! highlighter popover (the Marker button's chevron: four highlights and none,
//! the text colours and automatic) and the overflow menu (the tools that did
//! not fit a narrow note's bar, with icon, label and shortcut).
//!
//! It never takes the activation (`WS_EX_NOACTIVATE`, `MA_NOACTIVATE`, shown
//! with `SW_SHOWNA`): the note's hidden EDIT keeps the keyboard, so nothing
//! changes in its `Blur`. Esc is the EDIT's (`host::edit_event` closes this
//! first, as it does the colour popover). It also closes on a choice, when the
//! note ends editing, when the note window goes and on a click elsewhere on the
//! note. It sits above the bar, clamped to the work area. Tests create it but
//! never show it (`ON_SCREEN`).

use std::cell::RefCell;

use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_TRAILING,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, IDC_ARROW, LoadCursorW, MA_NOACTIVATE,
    RegisterClassExW, SW_SHOWNA, ShowWindow, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONDOWN,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Interface, PCWSTR, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::arrange_overlay::Canvas;
use crate::capture::Rect;
use crate::config::UiFont;
use crate::i18n::{Key, Lang, text};
use crate::shot_edit::gfx::color;
use crate::theme;
use crate::ui::geometry::Rect as FRect;
use crate::ui::icons::Icon;
use crate::widgets::host;
use crate::widgets::paint::Res;
use crate::widgets::styles::{self, Fg, Hl};
use crate::widgets::toolbar::{self, Tool};

/// Whether the popup is put on screen: always, except in tests, which make the
/// window and look at its drawing without ever showing it.
const ON_SCREEN: bool = !cfg!(test);

const CLASS: PCWSTR = w!("ValeWidgetFmt");

/// The popover's width (mockup C) and its parts, dip.
pub const POP_W: f32 = 196.0;
const PAD: f32 = 12.0;
const CELL: f32 = 24.0;
const LABEL_H: f32 = 14.0;
/// The overflow menu's width and row height. Wide enough for the longest label
/// with its shortcut in both languages (a test measures it with real text).
pub const MENU_W: f32 = 208.0;
const ROW_H: f32 = 28.0;
const MENU_PAD: f32 = 6.0;
const GAP: f32 = 6.0;

/// What the popover shows as chosen at the caret or over the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkerState {
    pub hl: Option<Hl>,
    pub fg: Option<Fg>,
    pub dark_paper: bool,
}

/// What is shown.
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Marker(MarkerState),
    /// The tools that did not fit, in order.
    Menu(Vec<Tool>),
}

/// A choice in the popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Hl(Option<Hl>),
    Fg(Option<Fg>),
    Tool(Tool),
}

struct Fmt {
    hwnd: HWND,
    note: HWND,
    origin: (i32, i32),
    scale: f32,
    mode: Mode,
    lang: Lang,
    res: Res,
    canvas: Option<Canvas>,
    hover: Option<Item>,
}

thread_local! {
    static FMT: RefCell<Option<Fmt>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Fmt) -> R) -> Option<R> {
    FMT.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

/// Whether one of the popups is open.
pub fn is_open() -> bool {
    FMT.with(|cell| cell.try_borrow().map(|p| p.is_some()).unwrap_or(true))
}

/// Whether it is open for `note`.
pub fn is_open_for(note: HWND) -> bool {
    FMT.with(|cell| {
        cell.try_borrow()
            .map(|p| p.as_ref().is_some_and(|p| p.note == note))
            .unwrap_or(false)
    })
}

/// Whether `h` is the popup of `note`.
pub fn is_popover_of(h: HWND, note: HWND) -> bool {
    FMT.with(|cell| {
        cell.try_borrow()
            .map(|p| p.as_ref().is_some_and(|p| p.hwnd == h && p.note == note))
            .unwrap_or(false)
    })
}

// ------------------------------------------------------------------ layout

/// The highlight cells of the popover: four colours and "none", from the left.
fn hl_cell(i: usize) -> FRect {
    let gap = (POP_W - 2.0 * PAD - 5.0 * CELL) / 4.0;
    FRect {
        x: PAD + i as f32 * (CELL + gap),
        y: PAD + LABEL_H + 4.0,
        w: CELL,
        h: CELL,
    }
}

/// The text colour cells: automatic, then the three colours.
fn fg_cell(i: usize) -> FRect {
    let gap = (POP_W - 2.0 * PAD - 4.0 * CELL) / 3.0;
    FRect {
        x: PAD + i as f32 * (CELL + gap),
        y: fg_label_y() + LABEL_H + 4.0,
        w: CELL,
        h: CELL,
    }
}

fn fg_label_y() -> f32 {
    PAD + LABEL_H + 4.0 + CELL + 14.0
}

/// The popover's height.
pub fn pop_h() -> f32 {
    fg_label_y() + LABEL_H + 4.0 + CELL + PAD
}

/// The overflow menu's height for `n` tools.
pub fn menu_h(n: usize) -> f32 {
    2.0 * MENU_PAD + n as f32 * ROW_H
}

fn menu_row(i: usize) -> FRect {
    FRect {
        x: MENU_PAD,
        y: MENU_PAD + i as f32 * ROW_H,
        w: MENU_W - 2.0 * MENU_PAD,
        h: ROW_H,
    }
}

fn inside(r: FRect, x: f32, y: f32) -> bool {
    r.contains(x, y)
}

/// The size of a mode's plate, dip.
pub fn plate_size(mode: &Mode) -> (f32, f32) {
    match mode {
        Mode::Marker(_) => (POP_W, pop_h()),
        Mode::Menu(tools) => (MENU_W, menu_h(tools.len())),
    }
}

/// What the point (dip from the plate's top-left) is on.
pub fn item_at(mode: &Mode, x: f32, y: f32) -> Option<Item> {
    match mode {
        Mode::Marker(_) => {
            let grown = |r: FRect| FRect { x: r.x - 3.0, y: r.y - 3.0, w: r.w + 6.0, h: r.h + 6.0 };
            for (i, hl) in Hl::ALL.iter().enumerate() {
                if inside(grown(hl_cell(i)), x, y) {
                    return Some(Item::Hl(Some(*hl)));
                }
            }
            if inside(grown(hl_cell(4)), x, y) {
                return Some(Item::Hl(None));
            }
            if inside(grown(fg_cell(0)), x, y) {
                return Some(Item::Fg(None));
            }
            for (i, f) in Fg::ALL.iter().enumerate() {
                if inside(grown(fg_cell(i + 1)), x, y) {
                    return Some(Item::Fg(Some(*f)));
                }
            }
            None
        }
        Mode::Menu(tools) => tools
            .iter()
            .enumerate()
            .find(|(i, _)| inside(menu_row(*i), x, y))
            .map(|(_, t)| Item::Tool(*t)),
    }
}

/// Where a plate `w` x `h` (px) goes for a button at `anchor` (px): centred on
/// it, `gap` above it (below when there is no room above), kept inside
/// `area`.
pub fn place(anchor: Rect, area: Rect, w: i32, h: i32, gap: i32) -> (i32, i32) {
    let cx = anchor.x + anchor.w / 2;
    let x = (cx - w / 2).clamp(area.x, (area.right() - w).max(area.x));
    let above = anchor.y - gap - h;
    let y = if above >= area.y {
        above
    } else {
        (anchor.bottom() + gap).min((area.bottom() - h).max(area.y))
    };
    (x, y)
}

// ------------------------------------------------------------------ window

fn register() -> bool {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return false;
    };
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(fmt_proc),
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

/// Opens `mode` above `anchor` (the bar button, desktop px) for `note`,
/// clamped inside `area` (the placement area, px). `scale` is the note's
/// monitor scale.
#[allow(clippy::too_many_arguments)]
pub fn open(note: HWND, anchor: Rect, area: Rect, scale: f32, mode: Mode, lang: Lang, ui: UiFont) {
    close();
    if !register() {
        return;
    }
    let Ok(res) = Res::new(ui) else { return };
    let (dw, dh) = plate_size(&mode);
    let (w, h) = ((dw * scale).round() as i32, (dh * scale).round() as i32);
    let (x, y) = place(anchor, area, w, h, (GAP * scale).round() as i32);
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return;
    };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            CLASS,
            w!("Vale sticky note format"),
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
    FMT.with(|cell| {
        *cell.borrow_mut() = Some(Fmt {
            hwnd,
            note,
            origin: (x, y),
            scale,
            mode,
            lang,
            res,
            canvas,
            hover: None,
        });
    });
    redraw();
    if ON_SCREEN {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
    }
}

/// Closes the popup, if one is open.
pub fn close() {
    let hwnd = FMT.with(|cell| {
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

fn fmt_text(
    res: &Res,
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    s: &str,
    r: FRect,
    size: f32,
    weight: u16,
    ink: u32,
    align: u8,
) {
    let Ok(l) = res.layout(s, size, weight, r.w) else { return };
    unsafe {
        let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = l.SetMaxHeight(r.h);
        let _ = l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        match align {
            1 => {
                let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            }
            2 => {
                let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING);
            }
            _ => {}
        }
        brush.SetColor(&color(ink, 1.0));
        rt.DrawTextLayout(Vector2::new(r.x, r.y), &l, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
    }
}

fn redraw() {
    let _ = with(|p| {
        let pal = theme::current_window();
        let k = p.scale;
        let Some(canvas) = &p.canvas else { return };
        let (pw, ph) = plate_size(&p.mode);
        let (lang, hover) = (p.lang, p.hover);
        let (mode, res) = (&p.mode, &p.res);
        let _ = canvas.paint_with(p.hwnd, p.origin, 0.0, 255, |c| {
            let (rt, brush, _) = c.parts();
            let Ok(rt) = rt.cast::<ID2D1RenderTarget>() else {
                return;
            };
            let set = |c: u32, a: f32| unsafe { brush.SetColor(&color(c, a)) };
            let rr = |r: FRect, rad: f32| D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h },
                radiusX: rad,
                radiusY: rad,
            };
            unsafe { rt.SetTransform(&Matrix3x2::scale(k, k)) };
            let whole = FRect { x: 0.5, y: 0.5, w: pw - 1.0, h: ph - 1.0 };
            set(pal.popover, 1.0);
            unsafe { rt.FillRoundedRectangle(&rr(whole, 12.0), brush) };
            set(pal.border >> 8, (pal.border & 0xFF) as f32 / 255.0 * 2.0);
            unsafe { rt.DrawRoundedRectangle(&rr(whole, 12.0), brush, 1.0, None) };
            match mode {
                Mode::Marker(st) => {
                    let label = |key: Key, y: f32| {
                        fmt_text(
                            res,
                            &rt,
                            brush,
                            text(lang, key),
                            FRect { x: PAD, y, w: POP_W - 2.0 * PAD, h: LABEL_H },
                            11.0,
                            600,
                            pal.secondary,
                            0,
                        );
                    };
                    label(Key::FmtHlLabel, PAD);
                    label(Key::FmtFgLabel, fg_label_y());
                    let ring = |r: FRect, on: bool, hot: bool| {
                        let centre = Vector2::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
                        if on || hot {
                            set(if on { pal.accent } else { pal.text }, if on { 1.0 } else { 0.35 });
                            unsafe {
                                rt.DrawEllipse(
                                    &D2D1_ELLIPSE {
                                        point: centre,
                                        radiusX: r.w / 2.0 + 3.0,
                                        radiusY: r.h / 2.0 + 3.0,
                                    },
                                    brush,
                                    if on { 2.0 } else { 1.5 },
                                    None,
                                );
                            }
                        }
                    };
                    for (i, hl) in Hl::ALL.iter().enumerate() {
                        let r = hl_cell(i);
                        set(styles::hl_rgb(*hl), 1.0);
                        unsafe {
                            rt.FillEllipse(
                                &D2D1_ELLIPSE {
                                    point: Vector2::new(r.x + r.w / 2.0, r.y + r.h / 2.0),
                                    radiusX: r.w / 2.0,
                                    radiusY: r.h / 2.0,
                                },
                                brush,
                            );
                        }
                        ring(r, st.hl == Some(*hl), hover == Some(Item::Hl(Some(*hl))));
                    }
                    // "None": an outlined circle with a slash.
                    let r = hl_cell(4);
                    let centre = Vector2::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
                    set(pal.text, 0.45);
                    unsafe {
                        rt.DrawEllipse(
                            &D2D1_ELLIPSE { point: centre, radiusX: r.w / 2.0 - 0.5, radiusY: r.h / 2.0 - 0.5 },
                            brush,
                            1.2,
                            None,
                        );
                        rt.DrawLine(
                            Vector2::new(r.x + 5.0, r.y + r.h - 5.0),
                            Vector2::new(r.x + r.w - 5.0, r.y + 5.0),
                            brush,
                            1.2,
                            None,
                        );
                    }
                    ring(r, st.hl.is_none(), hover == Some(Item::Hl(None)));
                    // The text colours: an "A" in each (automatic is the plain text).
                    let fg_items = [None, Some(Fg::Merah), Some(Fg::Biru), Some(Fg::Hijau)];
                    // Each "A" sits on a chip of the paper's own lightness, so the
                    // colour is seen as it will be (the plate itself may be dark).
                    let (chip, plain) = if st.dark_paper {
                        (0x24201E, 0xEDE7E1)
                    } else {
                        (0xF7EFD9, crate::widgets::sticky::INK)
                    };
                    for (i, f) in fg_items.iter().enumerate() {
                        let r = fg_cell(i);
                        set(chip, 1.0);
                        unsafe { rt.FillRoundedRectangle(&rr(r, 7.0), brush) };
                        let ink = f.map_or(plain, |f| styles::fg_rgb(f, st.dark_paper));
                        fmt_text(res, &rt, brush, "A", r, 13.0, 700, ink, 1);
                        ring_round(&rt, brush, &rr, r, st.fg == *f, hover == Some(Item::Fg(*f)), pal.accent, pal.text);
                    }
                }
                Mode::Menu(tools) => {
                    for (i, t) in tools.iter().enumerate() {
                        let row = menu_row(i);
                        if hover == Some(Item::Tool(*t)) {
                            set(pal.hover, 1.0);
                            unsafe { rt.FillRoundedRectangle(&rr(row, 8.0), brush) };
                        }
                        let icon = match t {
                            Tool::Check => Some(Icon::Check),
                            Tool::Image => Some(Icon::Image),
                            _ => None,
                        };
                        let ir = FRect { x: row.x + 4.0, y: row.y + 3.0, w: 22.0, h: 22.0 };
                        match icon {
                            Some(icon) => {
                                let _ = res.icons.draw(&rt, brush, icon, ir, 14.0, pal.text, 1.0, None);
                            }
                            None => {
                                let glyph = match t {
                                    Tool::Bold => "B",
                                    Tool::Italic => "I",
                                    Tool::Underline => "U",
                                    Tool::Strike => "ab",
                                    Tool::Bullet => "\u{2022}",
                                    Tool::Marker => "A",
                                    _ => "",
                                };
                                fmt_text(res, &rt, brush, glyph, ir, 12.5, 700, pal.text, 1);
                            }
                        }
                        fmt_text(
                            res,
                            &rt,
                            brush,
                            text(lang, t.label_key()),
                            FRect { x: row.x + 32.0, y: row.y, w: row.w - 36.0, h: row.h },
                            12.0,
                            500,
                            pal.text,
                            0,
                        );
                        fmt_text(
                            res,
                            &rt,
                            brush,
                            toolbar::shortcut(*t),
                            FRect { x: row.x + 32.0, y: row.y, w: row.w - 40.0, h: row.h },
                            10.5,
                            400,
                            pal.secondary,
                            2,
                        );
                    }
                }
            }
            unsafe { rt.SetTransform(&Matrix3x2::identity()) };
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn ring_round(
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    rr: &dyn Fn(FRect, f32) -> D2D1_ROUNDED_RECT,
    r: FRect,
    on: bool,
    hot: bool,
    accent: u32,
    text_rgb: u32,
) {
    if !(on || hot) {
        return;
    }
    let grown = FRect { x: r.x - 3.0, y: r.y - 3.0, w: r.w + 6.0, h: r.h + 6.0 };
    unsafe {
        brush.SetColor(&color(if on { accent } else { text_rgb }, if on { 1.0 } else { 0.35 }));
        rt.DrawRoundedRectangle(&rr(grown, 10.0), brush, if on { 2.0 } else { 1.5 }, None);
    }
}

fn to_dip(p: &Fmt, x: i32, y: i32) -> (f32, f32) {
    (x as f32 / p.scale, y as f32 / p.scale)
}

unsafe extern "system" fn fmt_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => return LRESULT(1),
        // The note's EDIT keeps the keyboard.
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let (x, y) = (
                (lparam.0 & 0xFFFF) as i16 as i32,
                ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
            );
            let changed = with(|p| {
                let (dx, dy) = to_dip(p, x, y);
                let item = item_at(&p.mode, dx, dy);
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
                item_at(&p.mode, dx, dy).map(|item| (p.note, item))
            })
            .flatten();
            if let Some((note, item)) = pick {
                host::fmt_choice(note, item);
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            let _ = FMT.with(|cell| cell.try_borrow_mut().ok().and_then(|mut p| p.take()));
            return LRESULT(0);
        }
        _ => {}
    }
    let _ = wparam;
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS;

    fn width(res: &Res, s: &str, size: f32, weight: u16) -> f32 {
        let l = res.layout(s, size, weight, 2000.0).expect("layout");
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { l.GetMetrics(&mut m) }.expect("metrics");
        m.widthIncludingTrailingWhitespace
    }

    #[test]
    fn the_popover_keeps_its_cells_inside_the_plate_and_clear_of_each_other() {
        let mut rects: Vec<FRect> = (0..5).map(hl_cell).collect();
        rects.extend((0..4).map(fg_cell));
        let h = pop_h();
        for r in &rects {
            assert!(r.x - 3.0 >= 0.0 && r.y - 3.0 >= 0.0, "{r:?}");
            assert!(r.x + r.w + 3.0 <= POP_W && r.y + r.h + 3.0 <= h, "{r:?}");
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                // The ring is 3 dip: cells keep 6 dip apart.
                let apart = a.right() + 6.0 <= b.x
                    || b.right() + 6.0 <= a.x
                    || a.bottom() + 6.0 <= b.y
                    || b.bottom() + 6.0 <= a.y;
                assert!(apart, "{a:?} and {b:?}");
            }
        }
        assert_eq!(POP_W, 196.0);
    }

    #[test]
    fn a_point_in_a_cell_is_that_choice() {
        let mode = Mode::Marker(MarkerState { hl: None, fg: None, dark_paper: false });
        for (i, hl) in Hl::ALL.iter().enumerate() {
            let r = hl_cell(i);
            assert_eq!(item_at(&mode, r.x + 2.0, r.y + 2.0), Some(Item::Hl(Some(*hl))));
        }
        let none = hl_cell(4);
        assert_eq!(item_at(&mode, none.x + 12.0, none.y + 12.0), Some(Item::Hl(None)));
        let auto = fg_cell(0);
        assert_eq!(item_at(&mode, auto.x + 12.0, auto.y + 12.0), Some(Item::Fg(None)));
        for (i, f) in Fg::ALL.iter().enumerate() {
            let r = fg_cell(i + 1);
            assert_eq!(item_at(&mode, r.x + 12.0, r.y + 12.0), Some(Item::Fg(Some(*f))));
        }
        assert_eq!(item_at(&mode, 1.0, pop_h() - 1.0), None);
        assert_eq!(item_at(&mode, POP_W + 5.0, 20.0), None);
    }

    #[test]
    fn the_menu_has_a_row_per_hidden_tool() {
        let tools = vec![Tool::Strike, Tool::Check, Tool::Marker];
        let mode = Mode::Menu(tools.clone());
        let (w, h) = plate_size(&mode);
        assert_eq!(w, MENU_W);
        assert_eq!(h, menu_h(3));
        for (i, t) in tools.iter().enumerate() {
            let r = menu_row(i);
            assert_eq!(item_at(&mode, r.x + 4.0, r.y + 4.0), Some(Item::Tool(*t)));
            assert!(r.bottom() <= h);
        }
        assert_eq!(item_at(&mode, 1.0, 1.0), None, "the plate's padding");
        assert!(MENU_W >= 168.0);
    }

    #[test]
    fn every_menu_label_and_shortcut_fits_the_menu_in_both_languages() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        for lang in [Lang::Id, Lang::En] {
            let mut widest = 0.0f32;
            for t in Tool::ALL {
                let label = width(&res, text(lang, t.label_key()), 12.0, 500);
                let hint = toolbar::shortcut(t);
                let hint_w = if hint.is_empty() { 0.0 } else { width(&res, hint, 10.5, 400) };
                // icon column, the label, a gap, the shortcut, the right padding.
                let need = 32.0 + label + 12.0 + hint_w + 8.0 + 2.0 * MENU_PAD;
                widest = widest.max(need);
                assert!(
                    need <= MENU_W,
                    "{lang:?} {t:?}: {need:.1} > {MENU_W} ({label:.1} + {hint_w:.1})"
                );
            }
            assert!(widest > 100.0, "the test measured something: {widest}");
        }
    }

    #[test]
    fn the_popover_labels_fit_in_both_languages() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        for lang in [Lang::Id, Lang::En] {
            for key in [Key::FmtHlLabel, Key::FmtFgLabel] {
                let w = width(&res, text(lang, key), 11.0, 600);
                assert!(w <= POP_W - 2.0 * PAD, "{lang:?} {key:?}: {w}");
            }
        }
    }

    #[test]
    fn the_popup_sits_above_its_button_inside_the_work_area() {
        let area = Rect { x: 0, y: 0, w: 1920, h: 1040 };
        let anchor = Rect { x: 800, y: 600, w: 36, h: 36 };
        let (x, y) = place(anchor, area, 196, 120, 6);
        assert_eq!(y, 600 - 6 - 120, "above the button");
        assert_eq!(x, 800 + 18 - 98, "centred on it");
        // Near the left edge it is pulled in; near the top it goes below.
        let (x, _) = place(Rect { x: 4, y: 600, w: 36, h: 36 }, area, 196, 120, 6);
        assert_eq!(x, 0);
        let (x, _) = place(Rect { x: 1900, y: 600, w: 36, h: 36 }, area, 196, 120, 6);
        assert_eq!(x, 1920 - 196);
        let (_, y) = place(Rect { x: 800, y: 20, w: 36, h: 36 }, area, 196, 120, 6);
        assert_eq!(y, 20 + 36 + 6, "no room above");
        // Never outside the area, whatever the anchor.
        let (_, y) = place(Rect { x: 800, y: 1000, w: 36, h: 36 }, area, 196, 2000, 6);
        assert!(y >= 0);
    }

    #[test]
    fn it_is_never_put_on_screen_in_a_test() {
        // A hidden owner, the popup made and looked at, nothing shown.
        use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, WS_POPUP};
        let owner = unsafe {
            CreateWindowExW(
                windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WS_POPUP,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
        }
        .expect("an owner");
        let area = Rect { x: 0, y: 0, w: 1920, h: 1040 };
        open(
            owner,
            Rect { x: 500, y: 500, w: 24, h: 24 },
            area,
            1.0,
            Mode::Marker(MarkerState { hl: Some(Hl::Pink), fg: None, dark_paper: false }),
            Lang::En,
            UiFont::Sans,
        );
        assert!(is_open() && is_open_for(owner));
        let hwnd = with(|p| p.hwnd).expect("open");
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool(), "a test never shows it");
        assert!(is_popover_of(hwnd, owner));
        close();
        assert!(!is_open());
        unsafe {
            let _ = DestroyWindow(owner);
        }
    }
}
