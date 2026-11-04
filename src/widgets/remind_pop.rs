//! "Atur sendiri...": the small entry that sets a sticky note's reminder to a
//! day and a time. A layered popup owned by the note, in the style of the
//! colour popover (`pop.rs`): a day stepper (`< Hari ini >`), an hour and a
//! minute stepper (5 minute steps, the mouse wheel works on all three) and
//! Simpan / Batal. All the stepping is `remind::Entry`; this file only draws
//! and routes the pointer. Esc, Batal, a click elsewhere and deactivation
//! close it; Simpan hands the time to the note (`host::remind_chosen`).

use std::cell::RefCell;

use chrono::{Local, NaiveDateTime};
use windows::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, IDC_ARROW, LoadCursorW, RegisterClassExW,
    SW_SHOW, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
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
use crate::widgets::remind::{self, Entry};

const CLASS: PCWSTR = w!("ValeWidgetRemind");
const W: f32 = 232.0;
const H: f32 = 168.0;
const PAD: f32 = 14.0;
const ROW_H: f32 = 40.0;
const ARROW: f32 = 28.0;

/// What the pointer can act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    DayPrev,
    DayNext,
    HourPrev,
    HourNext,
    MinPrev,
    MinNext,
    Save,
    Cancel,
}

/// A stepper the wheel works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Day,
    Hour,
    Minute,
}

struct Pop {
    hwnd: HWND,
    note: HWND,
    origin: (i32, i32),
    scale: f32,
    entry: Entry,
    now: NaiveDateTime,
    lang: Lang,
    res: Res,
    canvas: Option<Canvas>,
    hover: Option<Item>,
    closing: bool,
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

/// Whether the entry is open, and for `note`.
pub fn is_open_for(note: HWND) -> bool {
    POP.with(|cell| {
        cell.try_borrow()
            .map(|p| p.as_ref().is_some_and(|p| p.note == note))
            .unwrap_or(false)
    })
}

/// Whether `h` is the entry of `note`.
pub fn is_popover_of(h: HWND, note: HWND) -> bool {
    POP.with(|cell| {
        cell.try_borrow()
            .map(|p| p.as_ref().is_some_and(|p| p.hwnd == h && p.note == note))
            .unwrap_or(false)
    })
}

// ------------------------------------------------------------------ layout

fn row_y(n: usize) -> f32 {
    PAD + 22.0 + n as f32 * ROW_H
}

/// The day row spans the width; the hour and minute share the next one.
fn day_prev() -> FRect {
    FRect { x: PAD, y: row_y(0) + 6.0, w: ARROW, h: ARROW }
}

fn day_next() -> FRect {
    FRect { x: W - PAD - ARROW, y: row_y(0) + 6.0, w: ARROW, h: ARROW }
}

fn day_label() -> FRect {
    FRect { x: PAD + ARROW, y: row_y(0), w: W - 2.0 * (PAD + ARROW), h: ROW_H }
}

/// Half a row each: the hour's block, then the minute's.
fn half(i: usize) -> f32 {
    let gap = 12.0;
    let w = (W - 2.0 * PAD - gap) / 2.0;
    PAD + i as f32 * (w + gap)
}

fn half_w() -> f32 {
    (W - 2.0 * PAD - 12.0) / 2.0
}

fn hour_prev() -> FRect {
    FRect { x: half(0), y: row_y(1) + 6.0, w: ARROW, h: ARROW }
}

fn hour_next() -> FRect {
    FRect { x: half(0) + half_w() - ARROW, y: row_y(1) + 6.0, w: ARROW, h: ARROW }
}

fn hour_label() -> FRect {
    FRect { x: half(0) + ARROW, y: row_y(1), w: half_w() - 2.0 * ARROW, h: ROW_H }
}

fn min_prev() -> FRect {
    FRect { x: half(1), y: row_y(1) + 6.0, w: ARROW, h: ARROW }
}

fn min_next() -> FRect {
    FRect { x: half(1) + half_w() - ARROW, y: row_y(1) + 6.0, w: ARROW, h: ARROW }
}

fn min_label() -> FRect {
    FRect { x: half(1) + ARROW, y: row_y(1), w: half_w() - 2.0 * ARROW, h: ROW_H }
}

fn button(i: usize) -> FRect {
    let gap = 8.0;
    let w = (W - 2.0 * PAD - gap) / 2.0;
    FRect { x: PAD + i as f32 * (w + gap), y: H - PAD - 30.0, w, h: 30.0 }
}

fn inside(r: FRect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

fn item_at(x: f32, y: f32) -> Option<Item> {
    [
        (day_prev(), Item::DayPrev),
        (day_next(), Item::DayNext),
        (hour_prev(), Item::HourPrev),
        (hour_next(), Item::HourNext),
        (min_prev(), Item::MinPrev),
        (min_next(), Item::MinNext),
        (button(0), Item::Cancel),
        (button(1), Item::Save),
    ]
    .into_iter()
    .find(|(r, _)| inside(*r, x, y))
    .map(|(_, i)| i)
}

/// The field under a point, for the wheel: its whole block.
fn field_at(x: f32, y: f32) -> Option<Field> {
    if y >= row_y(0) && y < row_y(0) + ROW_H {
        return Some(Field::Day);
    }
    if y >= row_y(1) && y < row_y(1) + ROW_H {
        return Some(if x < half(1) - 6.0 { Field::Hour } else { Field::Minute });
    }
    None
}

/// The entry after `item`.
fn stepped(e: Entry, item: Item) -> Entry {
    match item {
        Item::DayPrev => e.step_day(-1),
        Item::DayNext => e.step_day(1),
        Item::HourPrev => e.step_hour(-1),
        Item::HourNext => e.step_hour(1),
        Item::MinPrev => e.step_minute(-1),
        Item::MinNext => e.step_minute(1),
        Item::Save | Item::Cancel => e,
    }
}

/// The entry after the wheel turned over `field` by `notches` (positive is
/// up: later).
fn wheeled(e: Entry, field: Field, notches: i32) -> Entry {
    match field {
        Field::Day => e.step_day(i64::from(notches)),
        Field::Hour => e.step_hour(notches),
        Field::Minute => e.step_minute(notches),
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

/// Opens the entry below `anchor` (px) for `note`, kept inside `area` (px).
pub fn open(note: HWND, anchor: Rect, area: Rect, scale: f32, lang: Lang, font: UiFont) {
    close();
    if !register() {
        return;
    }
    let Ok(res) = Res::new(font) else { return };
    let now = Local::now().naive_local();
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
            w!("Vale sticky note reminder"),
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
            entry: Entry::from_now(now),
            now,
            lang,
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
        let (entry, now, lang, hover) = (p.entry, p.now, p.lang, p.hover);
        let future = entry.is_future(now);
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
            let whole = FRect { x: 0.5, y: 0.5, w: W - 1.0, h: H - 1.0 };
            set(pal.popover, 1.0);
            unsafe { rt.FillRoundedRectangle(&rr(whole, 12.0), brush) };
            set(pal.border >> 8, (pal.border & 0xFF) as f32 / 255.0 * 2.0);
            unsafe { rt.DrawRoundedRectangle(&rr(whole, 12.0), brush, 1.0, None) };
            let label = |s: &str, r: FRect, size: f32, weight: u16, ink: u32, center: bool| {
                let Ok(l) = res.layout(s, size, weight, r.w) else { return };
                unsafe {
                    let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    let _ = l.SetMaxHeight(r.h);
                    let _ = l.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                    if center {
                        let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                    }
                    brush.SetColor(&color(ink, 1.0));
                    rt.DrawTextLayout(
                        Vector2::new(r.x, r.y),
                        &l,
                        brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
            };
            label(
                text(lang, Key::StickyRemind),
                FRect { x: PAD, y: PAD - 4.0, w: W - 2.0 * PAD, h: 22.0 },
                12.5,
                600,
                pal.text,
                false,
            );
            // The steppers: an arrow either side of the value.
            let arrow = |r: FRect, item: Item, left: bool| {
                if hover == Some(item) {
                    set(pal.button_hover, 1.0);
                } else {
                    set(pal.button, 1.0);
                }
                unsafe { rt.FillRoundedRectangle(&rr(r, 8.0), brush) };
                label(if left { "\u{2039}" } else { "\u{203A}" }, r, 16.0, 500, pal.text, true);
            };
            arrow(day_prev(), Item::DayPrev, true);
            arrow(day_next(), Item::DayNext, false);
            arrow(hour_prev(), Item::HourPrev, true);
            arrow(hour_next(), Item::HourNext, false);
            arrow(min_prev(), Item::MinPrev, true);
            arrow(min_next(), Item::MinNext, false);
            label(&entry.day_label(lang, now.date()), day_label(), 13.0, 600, pal.text, true);
            label(&format!("{:02}", entry.hour), hour_label(), 15.0, 600, pal.text, true);
            label(&format!("{:02}", entry.minute), min_label(), 15.0, 600, pal.text, true);
            // The colon between the hour and the minute.
            label(
                ":",
                FRect { x: half(1) - 12.0, y: row_y(1), w: 12.0, h: ROW_H },
                15.0,
                600,
                pal.secondary,
                true,
            );
            // Batal and Simpan.
            let cancel = button(0);
            set(if hover == Some(Item::Cancel) { pal.button_hover } else { pal.button }, 1.0);
            unsafe { rt.FillRoundedRectangle(&rr(cancel, 9.0), brush) };
            label(text(lang, Key::Cancel), cancel, 12.0, 500, pal.text, true);
            let save = button(1);
            set(pal.accent, if future { 1.0 } else { 0.4 });
            unsafe { rt.FillRoundedRectangle(&rr(save, 9.0), brush) };
            label(text(lang, Key::Save), save, 12.0, 600, pal.on_accent, true);
            unsafe { rt.SetTransform(&Matrix3x2::identity()) };
        });
    });
}

fn to_dip(p: &Pop, x: i32, y: i32) -> (f32, f32) {
    (x as f32 / p.scale, y as f32 / p.scale)
}

fn xy(l: LPARAM) -> (i32, i32) {
    (
        (l.0 & 0xFFFF) as i16 as i32,
        ((l.0 >> 16) & 0xFFFF) as i16 as i32,
    )
}

/// Save: the note gets the time, and the entry closes.
fn save() {
    let chosen = with(|p| p.entry.is_future(p.now).then(|| (p.note, p.entry.at(p.now.date()))))
        .flatten();
    if let Some((note, at)) = chosen {
        close();
        host::remind_chosen(note, remind::to_unix(at));
    }
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
            let (x, y) = xy(lparam);
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
            let (x, y) = xy(lparam);
            let item = with(|p| {
                let (dx, dy) = to_dip(p, x, y);
                item_at(dx, dy)
            })
            .flatten();
            match item {
                Some(Item::Save) => save(),
                Some(Item::Cancel) => close(),
                Some(item) => {
                    let _ = with(|p| p.entry = stepped(p.entry, item));
                    redraw();
                }
                None => {}
            }
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            // The wheel's point is in screen px.
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as i32 / 120;
            let (sx, sy) = xy(lparam);
            let _ = with(|p| {
                let (dx, dy) = to_dip(p, sx - p.origin.0, sy - p.origin.1);
                if let Some(field) = field_at(dx, dy) {
                    p.entry = wheeled(p.entry, field, delta);
                }
            });
            redraw();
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            let key = (wparam.0 & 0xFFFF) as u16;
            if key == VK_ESCAPE.0 {
                close();
            } else if key == VK_RETURN.0 {
                save();
            }
            return LRESULT(0);
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                let to = lparam.0;
                let _ = with(|p| {
                    if !p.closing && to != p.note.0 as isize {
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

    #[test]
    fn every_control_fits_inside_the_plate_and_clear_of_the_others() {
        let rects = [
            day_prev(),
            day_next(),
            hour_prev(),
            hour_next(),
            min_prev(),
            min_next(),
            button(0),
            button(1),
        ];
        for r in rects {
            assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= W && r.y + r.h <= H, "{r:?}");
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
        // The labels sit between their arrows.
        assert!(day_label().w > 60.0 && hour_label().w > 20.0 && min_label().w > 20.0);
    }

    #[test]
    fn a_click_on_an_arrow_steps_that_field_and_the_wheel_works_on_all_three() {
        let e = Entry { day: 0, hour: 9, minute: 30 };
        assert_eq!(stepped(e, Item::DayNext).day, 1);
        assert_eq!(stepped(e, Item::DayPrev).day, 0, "never before today");
        assert_eq!(stepped(e, Item::HourNext).hour, 10);
        assert_eq!(stepped(e, Item::HourPrev).hour, 8);
        assert_eq!(stepped(e, Item::MinNext).minute, 35);
        assert_eq!(stepped(e, Item::MinPrev).minute, 25);
        assert_eq!(stepped(e, Item::Save), e);
        assert_eq!(wheeled(e, Field::Day, 2).day, 2);
        assert_eq!(wheeled(e, Field::Hour, -1).hour, 8);
        assert_eq!(wheeled(e, Field::Minute, 1).minute, 35);
        // The points: an arrow is an item, the label's block is a wheel field.
        let a = day_next();
        assert_eq!(item_at(a.x + 2.0, a.y + 2.0), Some(Item::DayNext));
        let c = button(1);
        assert_eq!(item_at(c.x + 2.0, c.y + 2.0), Some(Item::Save));
        assert_eq!(item_at(2.0, 2.0), None);
        assert_eq!(field_at(W / 2.0, row_y(0) + 5.0), Some(Field::Day));
        assert_eq!(field_at(half(0) + 30.0, row_y(1) + 5.0), Some(Field::Hour));
        assert_eq!(field_at(half(1) + 30.0, row_y(1) + 5.0), Some(Field::Minute));
        assert_eq!(field_at(W / 2.0, H - 5.0), None);
    }
}
