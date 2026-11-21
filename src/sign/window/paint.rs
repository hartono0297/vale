//! Drawing the signing window with Direct2D, in the order a frame is built:
//! title row, tools, banner, rail, stage (page, items, selection), footer,
//! the item bar, popovers, the keyboard ring. Colours come from the window
//! palette; the page, its items and the ink stay paper and ink in both
//! themes.

use std::sync::Arc;

use windows::Win32::Foundation::D2DERR_RECREATE_TARGET;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_SIZE_F, D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_ROUNDED_RECT, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
    ID2D1Bitmap, ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFactory,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::core::w;
use windows_numerics::{Matrix3x2, Vector2};

use super::layout::{self, Hit, Tool};
use super::window::{
    self, Ask, Drag, Phase, Pop, Tone, View, Win, float_bar, popover, rail, shown_item,
    ui_of, view,
};
use crate::i18n::{Key, tr};
use crate::shot_edit::gfx::{Align, bitmap, color, rect_f};
use crate::shot_edit::icons::{fill, fill_round};
use crate::sign::{ItemKind, Mark, Quarter, raster};
use crate::theme;
use crate::ui::geometry::Rect;

/// Ink blue `#1F3A8A`, the colour of placed text and checks (as the PDF
/// writer and the image writer draw them).
pub(super) const INK: u32 = 0x1F3A8A;
/// The stage behind the page: a step darker than the window, so the white
/// page reads as the one lifted object.
const STAGE_DARK: u32 = 0x15110E;
const STAGE_LIGHT: u32 = 0xECE5DB;
/// The paper of the picker tiles and the maker pad.
pub(super) const PAPER: u32 = 0xFFFDF9;
const RETRIES: u8 = 3;

pub(super) fn weight_of(hit: Hit) -> i32 {
    match hit {
        Hit::Save | Hit::SaveAnyway | Hit::Discard | Hit::OpenPassword => 600,
        Hit::Cancel | Hit::Keep | Hit::NotNow | Hit::CloseWindow | Hit::FormatImage | Hit::FormatPdf => 500,
        _ => 400,
    }
}

fn rgba(packed: u32) -> (u32, f32) {
    (packed >> 8, (packed & 0xFF) as f32 / 255.0)
}

fn neg(q: Quarter) -> Quarter {
    Quarter::from_degrees(-q.degrees())
}

/// A counter-clockwise visual turn by `q` about `(cx, cy)` (y down).
pub(super) fn turn_matrix(q: Quarter, cx: f32, cy: f32) -> Matrix3x2 {
    let (sin, cos) = match q {
        Quarter::R0 => (0.0, 1.0),
        Quarter::R90 => (-1.0, 0.0),
        Quarter::R180 => (0.0, -1.0),
        Quarter::R270 => (1.0, 0.0),
    };
    Matrix3x2 { M11: cos, M12: sin, M21: -sin, M22: cos, M31: cx, M32: cy }
}

fn odd(q: Quarter) -> bool {
    matches!(q, Quarter::R90 | Quarter::R270)
}

fn inflate(r: Rect, by: f32) -> Rect {
    Rect { x: r.x - by, y: r.y - by, w: r.w + 2.0 * by, h: r.h + 2.0 * by }
}

fn stroke_round(rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush, r: Rect, radius: f32, width: f32) {
    unsafe {
        rt.DrawRoundedRectangle(
            &D2D1_ROUNDED_RECT { rect: rect_f(r.x, r.y, r.w, r.h), radiusX: radius, radiusY: radius },
            brush,
            width,
            None,
        );
    }
}

// ---- ink text ----

fn arial(
    dwrite: &IDWriteFactory,
    size: f32,
) -> Option<windows::Win32::Graphics::DirectWrite::IDWriteTextFormat> {
    unsafe {
        let f = dwrite
            .CreateTextFormat(
                w!("Arial"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size.max(0.5),
                w!("en-us"),
            )
            .ok()?;
        let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        Some(f)
    }
}

/// Width of `s` in Arial at `size` (any unit: points in, points out).
pub(super) fn arial_width(dwrite: &IDWriteFactory, s: &str, size: f32) -> f32 {
    let wide: Vec<u16> = s.encode_utf16().collect();
    let Some(f) = arial(dwrite, size) else { return s.chars().count() as f32 * size * 0.55 };
    unsafe {
        let Ok(layout) = dwrite.CreateTextLayout(&wide, &f, 1.0e6, size * 4.0) else {
            return s.chars().count() as f32 * size * 0.55;
        };
        let mut m = DWRITE_TEXT_METRICS::default();
        if layout.GetMetrics(&mut m).is_err() {
            return s.chars().count() as f32 * size * 0.55;
        }
        m.widthIncludingTrailingWhitespace
    }
}

/// `s` in Arial filling `TEXT_FILL` of the box height, left aligned, its
/// baseline a fifth of the height above the bottom: where the PDF writer
/// puts Helvetica.
pub(super) fn ink_text(
    rt: &ID2D1RenderTarget,
    dwrite: &IDWriteFactory,
    brush: &ID2D1SolidColorBrush,
    s: &str,
    (x, y, _w, h): (f32, f32, f32, f32),
) {
    let size = h * window::TEXT_FILL;
    let Some(f) = arial(dwrite, size) else { return };
    let wide: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        let Ok(layout) = dwrite.CreateTextLayout(&wide, &f, 1.0e6, size * 4.0) else { return };
        let mut lines = [DWRITE_LINE_METRICS::default()];
        let mut n = 0u32;
        let baseline = if layout.GetLineMetrics(Some(&mut lines), &mut n).is_ok() && n > 0 {
            lines[0].baseline
        } else {
            size * 0.905
        };
        rt.DrawTextLayout(
            Vector2::new(x, y + h * 0.8 - baseline),
            &layout,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }
}

/// `s` drawn as on the page, as a picture the size of a `w_pt` x `h_pt`
/// box (for text the PDF's standard font cannot write). COM must be
/// initialised on the calling thread.
pub(super) fn text_mark(s: &str, w_pt: f32, h_pt: f32) -> Option<Mark> {
    if !(w_pt > 0.0 && h_pt > 0.0) {
        return None;
    }
    // Four pixels a point, less for a huge box.
    let k = 4.0f32.min((30_000_000.0 / (w_pt * h_pt)).sqrt()).max(0.5);
    let (wp, hp) = ((w_pt * k).round().max(1.0) as u32, (h_pt * k).round().max(1.0) as u32);
    let mut drawn = false;
    let px = raster::offscreen(wp, hp, |rt, _, dwrite| unsafe {
        let Ok(brush) = rt.CreateSolidColorBrush(&color(INK, 1.0), None) else { return };
        rt.SetTextAntialiasMode(windows::Win32::Graphics::Direct2D::D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        ink_text(rt, dwrite, &brush, s, (0.0, 0.0, wp as f32, hp as f32));
        drawn = true;
    })?;
    drawn.then(|| raster::to_mark(wp, hp, &px))
}

fn draw_check(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    round: &ID2D1StrokeStyle,
    bw: f32,
    bh: f32,
) {
    // The PDF writer's tick in the unit box (y up): 0.18,0.52 0.42,0.26 0.84,0.8.
    let at = |fx: f32, fy: f32| Vector2::new((fx - 0.5) * bw, (0.5 - fy) * bh);
    let width = (bw.min(bh) * 0.12).max(1.0);
    unsafe {
        rt.DrawLine(at(0.18, 0.52), at(0.42, 0.26), brush, width, round);
        rt.DrawLine(at(0.42, 0.26), at(0.84, 0.8), brush, width, round);
    }
}

/// A mark (straight RGBA) as premultiplied BGRA.
pub(super) fn premultiplied(m: &Mark) -> Vec<u8> {
    let mut out = Vec::with_capacity(m.rgba.len());
    for p in m.rgba.chunks_exact(4) {
        let a = u32::from(p[3]);
        let pm = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
        out.extend_from_slice(&[pm(p[2]), pm(p[1]), pm(p[0]), p[3]]);
    }
    out
}

/// Drops the bitmaps of marks nothing else holds any more (a removed item,
/// a forgotten saved mark, an old typed preview), so the cache follows what
/// can still be drawn.
pub(super) fn evict_unused<B>(cache: &mut std::collections::HashMap<usize, (Arc<Mark>, B)>) {
    cache.retain(|_, (m, _)| Arc::strong_count(m) > 1);
}

fn mark_bitmap(w: &mut Win, rt: &ID2D1RenderTarget, m: &Arc<Mark>) -> Option<ID2D1Bitmap> {
    let key = Arc::as_ptr(m) as usize;
    if let Some((_, b)) = w.mark_bmps.get(&key) {
        return Some(b.clone());
    }
    if m.rgba.len() != m.w as usize * m.h as usize * 4 || m.w == 0 {
        return None;
    }
    let b = bitmap(rt, m.w, m.h, &premultiplied(m)).ok()?;
    w.mark_bmps.insert(key, (m.clone(), b.clone()));
    Some(b)
}

// ---- icons ----

#[derive(Clone, Copy)]
struct Vec2 {
    x: f32,
    y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Icon {
    Signature,
    Initials,
    Date,
    Text,
    Check,
    Rotate,
    Trash,
    More,
}

pub(super) fn icon_of(hit: Hit) -> Option<Icon> {
    Some(match hit {
        Hit::Tool(Tool::Signature) => Icon::Signature,
        Hit::Tool(Tool::Initials) => Icon::Initials,
        Hit::Tool(Tool::Date) => Icon::Date,
        Hit::Tool(Tool::Text) => Icon::Text,
        Hit::Tool(Tool::Check) => Icon::Check,
        Hit::Rotate => Icon::Rotate,
        Hit::DeletePage => Icon::Trash,
        Hit::More => Icon::More,
        _ => return None,
    })
}

#[derive(Clone, Copy)]
enum Seg {
    M(f32, f32),
    L(f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    /// To (x, y) on a circle of radius r: large arc, clockwise.
    A(f32, f32, f32, bool, bool),
}

/// The pictures of the tools, on a 24-unit grid (from the design's SVGs).
fn icon_paths(i: Icon) -> &'static [&'static [Seg]] {
    use Seg::*;
    match i {
        Icon::Signature => &[
            &[M(3.0, 17.0), C(6.0, 11.0, 8.0, 8.0, 10.0, 8.0), C(12.0, 8.0, 9.0, 16.0, 12.0, 16.0),
              C(15.0, 16.0, 15.0, 11.0, 17.0, 11.0), C(19.0, 11.0, 18.0, 15.0, 21.0, 15.0)],
            &[M(3.0, 21.0), L(21.0, 21.0)],
        ],
        Icon::Initials => &[
            &[M(5.0, 15.0), C(7.0, 11.0, 8.0, 9.0, 9.0, 9.0), C(10.0, 9.0, 9.0, 14.0, 11.0, 14.0),
              C(13.0, 14.0, 13.0, 11.0, 14.0, 11.0)],
            &[M(17.0, 17.0), L(20.0, 17.0)],
        ],
        Icon::Date => &[
            &[M(6.0, 5.0), L(18.0, 5.0), A(21.0, 8.0, 3.0, false, true), L(21.0, 18.0),
              A(18.0, 21.0, 3.0, false, true), L(6.0, 21.0), A(3.0, 18.0, 3.0, false, true), L(3.0, 8.0),
              A(6.0, 5.0, 3.0, false, true)],
            &[M(3.0, 10.0), L(21.0, 10.0)],
            &[M(8.0, 3.0), L(8.0, 7.0)],
            &[M(16.0, 3.0), L(16.0, 7.0)],
        ],
        Icon::Text => &[
            &[M(5.0, 6.0), L(5.0, 4.0), L(19.0, 4.0), L(19.0, 6.0)],
            &[M(12.0, 4.0), L(12.0, 20.0)],
            &[M(9.0, 20.0), L(15.0, 20.0)],
        ],
        Icon::Check => &[&[M(5.0, 12.5), L(9.5, 17.0), L(19.0, 7.0)]],
        Icon::Rotate => &[
            &[M(20.0, 11.0), A(17.7, 16.7, 8.0, true, false)],
            &[M(20.0, 4.0), L(20.0, 11.0), L(13.0, 11.0)],
        ],
        Icon::Trash => &[
            &[M(4.0, 7.0), L(20.0, 7.0)],
            &[M(9.0, 7.0), L(9.0, 4.0), L(15.0, 4.0), L(15.0, 7.0)],
            &[M(6.0, 7.0), L(7.0, 20.0), L(17.0, 20.0), L(18.0, 7.0)],
        ],
        Icon::More => &[],
    }
}

/// `icon` drawn in a `size` px square with its top-left at `(x, y)`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_icon(
    rt: &ID2D1RenderTarget,
    d2d: &ID2D1Factory,
    brush: &ID2D1SolidColorBrush,
    round: &ID2D1StrokeStyle,
    icon: Icon,
    (x, y): (f32, f32),
    size: f32,
    rgb: u32,
    alpha: f32,
) {
    let s = size / 24.0;
    let pt = |px: f32, py: f32| Vec2 { x: x + px * s, y: y + py * s };
    unsafe {
        brush.SetColor(&color(rgb, alpha));
        if icon == Icon::More {
            for cx in [6.0, 12.0, 18.0] {
                let c = pt(cx, 12.0);
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2::new(c.x, c.y), radiusX: 1.6 * s, radiusY: 1.6 * s },
                    brush,
                );
            }
            return;
        }
        let width = (1.7 * s).max(1.0);
        for path in icon_paths(icon) {
            let Ok(geo) = d2d.CreatePathGeometry() else { continue };
            let Ok(sink) = geo.Open() else { continue };
            let mut open = false;
            for seg in *path {
                match *seg {
                    Seg::M(a, b) => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        }
                        let p = pt(a, b);
                        sink.BeginFigure(Vector2::new(p.x, p.y), D2D1_FIGURE_BEGIN_HOLLOW);
                        open = true;
                    }
                    Seg::L(a, b) => {
                        let p = pt(a, b);
                        sink.AddLine(Vector2::new(p.x, p.y));
                    }
                    Seg::C(a, b, c, d, e, f) => {
                        let (p1, p2, p3) = (pt(a, b), pt(c, d), pt(e, f));
                        sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                            point1: Vector2::new(p1.x, p1.y),
                            point2: Vector2::new(p2.x, p2.y),
                            point3: Vector2::new(p3.x, p3.y),
                        });
                    }
                    Seg::A(a, b, r, large, cw) => {
                        let p = pt(a, b);
                        sink.AddArc(&D2D1_ARC_SEGMENT {
                            point: Vector2::new(p.x, p.y),
                            size: D2D_SIZE_F { width: r * s, height: r * s },
                            rotationAngle: 0.0,
                            sweepDirection: if cw {
                                D2D1_SWEEP_DIRECTION_CLOCKWISE
                            } else {
                                D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                            },
                            arcSize: if large { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                        });
                    }
                }
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            if sink.Close().is_ok() {
                rt.DrawGeometry(&geo, brush, width, round);
            }
        }
    }
}

// ---- the frame ----

pub(super) fn paint(w: &mut Win) {
    let mut ps = PAINTSTRUCT::default();
    unsafe {
        BeginPaint(w.hwnd, &mut ps);
    }
    let drawn = draw(w);
    evict_unused(&mut w.mark_bmps);
    match drawn {
        Ok(()) => w.lost = 0,
        Err(e) if e.code() == D2DERR_RECREATE_TARGET => {
            w.target = None;
            w.mark_bmps.clear();
            for s in w.pages.values_mut().chain(w.thumbs.values_mut()) {
                s.bmp = None;
            }
            w.lost = w.lost.saturating_add(1);
            if w.lost < RETRIES {
                unsafe {
                    let _ = InvalidateRect(Some(w.hwnd), None, false);
                }
            }
        }
        Err(e) => eprintln!("vale: the signing window could not draw: {e}"),
    }
    unsafe {
        let _ = EndPaint(w.hwnd, &ps);
    }
}

fn stage_rgb(p: &theme::WindowPalette) -> u32 {
    if p.light { STAGE_LIGHT } else { STAGE_DARK }
}

fn draw(w: &mut Win) -> windows::core::Result<()> {
    if w.target.is_none() {
        w.target = Some(w.gfx.hwnd_target(w.hwnd, w.client)?);
    }
    let Some(t) = w.target.as_ref() else { return Ok(()) };
    let rt: ID2D1RenderTarget = windows::core::Interface::cast(&t.rt)?;
    let brush = t.brush.clone();
    let p = w.p;
    let ui = ui_of(w);
    unsafe {
        rt.BeginDraw();
        rt.Clear(Some(&color(p.bg, 1.0)));
    }
    let (border, ba) = rgba(p.border);
    let hair = ba.max(0.12);
    let l = w.layout.clone();

    // Title row: the file name and where in the document we are.
    let name = w.src.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let pad = 14.0 * ui;
    let count = page_count_text(w);
    let count_w = if count.is_empty() { 0.0 } else { w.gfx.measure(&count, 12.5 * ui, 400).0 };
    let name_w = w.gfx.measure(&name, 13.5 * ui, 600).0.min((l.title.w - 3.0 * pad - count_w).max(0.0));
    w.gfx.text_in(
        &rt,
        &brush,
        &name,
        (pad, l.title.y, name_w, l.title.h),
        13.5 * ui,
        600,
        p.text,
        1.0,
        Align::Left,
    );
    if !count.is_empty() {
        let x = pad + name_w + 10.0 * ui;
        w.gfx.text_in(
            &rt,
            &brush,
            &count,
            (x, l.title.y, count_w + 2.0, l.title.h),
            12.5 * ui,
            400,
            p.secondary,
            1.0,
            Align::Left,
        );
    }
    fill(&rt, &brush, Rect { x: 0.0, y: l.title.bottom() - 1.0, w: l.title.w, h: 1.0 }, border, hair);

    // Tools row.
    let ready = w.phase == Phase::Ready;
    for (hit, r) in l.buttons.iter().filter(|(h, _)| matches!(h, Hit::Tool(_) | Hit::Rotate | Hit::DeletePage | Hit::More)) {
        let on = match (hit, &w.armed, w.pop) {
            (Hit::Tool(t), Some(a), _) if a.tool == *t => true,
            (Hit::Tool(Tool::Signature), _, Some(Pop::Picker { initials: false })) => true,
            (Hit::Tool(Tool::Initials), _, Some(Pop::Picker { initials: true })) => true,
            (Hit::More, _, Some(Pop::More)) => true,
            _ => false,
        };
        let hot = ready && w.hover == Some(*hit);
        let a = if ready { 1.0 } else { 0.4 };
        let (sel, sa) = rgba(p.selection);
        if on {
            fill_round(&rt, &brush, *r, 9.0 * ui, sel, sa);
        } else if hot {
            fill_round(&rt, &brush, *r, 9.0 * ui, p.hover, 1.0);
        }
        let icon = layout_icon(*hit);
        let ix = if *hit == Hit::More { r.x + (r.w - 16.0 * ui) / 2.0 } else { r.x + 11.0 * ui };
        if let Some(icon) = icon {
            draw_icon(&rt, &w.gfx.d2d, &brush, &w.gfx.round, icon, (
                ix,
                r.y + (r.h - 16.0 * ui) / 2.0,
            ), 16.0 * ui,
                if on { p.accent } else { p.secondary }, a);
        }
        if let Some(k) = layout::label_key(*hit) {
            let tx = r.x + (11.0 + 16.0 + 7.0) * ui;
            w.gfx.text_in(
                &rt,
                &brush,
                tr(k),
                (tx, r.y, r.right() - tx, r.h),
                window::UI_TEXT * ui,
                400,
                p.text,
                a,
                Align::Left,
            );
        }
    }
    if let Some(x) = l.divider {
        fill(
            &rt,
            &brush,
            Rect { x, y: l.tools.y + l.tools.h * 0.28, w: 1.0, h: l.tools.h * 0.44 },
            border,
            ba.max(0.18),
        );
    }
    fill(&rt, &brush, Rect { x: 0.0, y: l.tools.bottom() - 1.0, w: l.tools.w, h: 1.0 }, border, hair);

    if let Some(b) = l.banner {
        fill(&rt, &brush, b, p.card, 1.0);
        w.gfx.text_in(
            &rt,
            &brush,
            tr(Key::SignNoChanges),
            (pad, b.y, b.w - 2.0 * pad, b.h),
            12.5 * ui,
            500,
            p.text,
            1.0,
            Align::Left,
        );
        fill(&rt, &brush, Rect { x: 0.0, y: b.bottom() - 1.0, w: b.w, h: 1.0 }, border, hair);
    }

    // Stage.
    fill(&rt, &brush, l.stage, stage_rgb(&p), 1.0);
    match w.phase {
        Phase::Ready | Phase::Saving => draw_page(w, &rt, &brush)?,
        Phase::Loading | Phase::Converting => {
            let k = if w.phase == Phase::Converting { Key::SignConverting } else { Key::SignLoading };
            w.gfx.text_in(
                &rt,
                &brush,
                tr(k),
                (l.message.x, l.message.y, l.message.w, l.message.h),
                14.0 * ui,
                500,
                p.text,
                1.0,
                Align::Center,
            );
        }
        Phase::Password { wrong, busy } => {
            let (k, rgb) = if wrong { (Key::SignPasswordWrong, p.error) } else { (
                Key::SignPassword,
                p.text,
            ) };
            w.gfx.text_in(
                &rt,
                &brush,
                tr(k),
                (l.message.x, l.message.y, l.message.w, l.message.h),
                14.0 * ui,
                500,
                rgb,
                1.0,
                Align::Center,
            );
            if let Some(f) = l.field {
                fill_round(&rt, &brush, f, 8.0 * ui, p.field, 1.0);
                brush_set(&brush, border, ba.max(0.3));
                stroke_round(&rt, &brush, f, 8.0 * ui, 1.0);
            }
            if busy {
                // The button reads dimmed while the password is tried.
                if let Some(r) = l.rect_of(Hit::OpenPassword) {
                    fill_round(&rt, &brush, r, 10.0 * ui, p.accent, 0.5);
                }
            }
        }
        Phase::Failed(k) => {
            w.gfx.text_in(
                &rt,
                &brush,
                tr(k),
                (l.message.x, l.message.y, l.message.w, l.message.h),
                14.0 * ui,
                500,
                p.text,
                1.0,
                Align::Center,
            );
        }
    }

    // Rail.
    draw_rail(w, &rt, &brush)?;
    fill(&rt, &brush, Rect { x: l.rail.right() - 1.0, y: l.rail.y, w: 1.0, h: l.rail.h }, border, hair);

    // Footer.
    fill(&rt, &brush, l.footer, p.bg, 1.0);
    fill(&rt, &brush, Rect { x: 0.0, y: l.footer.y, w: l.footer.w, h: 1.0 }, border, hair);
    draw_footer_text(w, &rt, &brush);
    for (hit, r) in &l.buttons {
        if matches!(hit, Hit::Tool(_) | Hit::Rotate | Hit::DeletePage | Hit::More) {
            continue;
        }
        if matches!(hit, Hit::FormatImage | Hit::FormatPdf) {
            let on = (*hit == Hit::FormatPdf) == w.as_pdf;
            if on {
                fill_round(&rt, &brush, *r, 8.0 * ui, p.button_hover, 1.0);
            }
            brush_set(&brush, border, ba.max(0.3));
            stroke_round(&rt, &brush, *r, 8.0 * ui, 1.0);
            let k = layout::label_key(*hit).unwrap_or(Key::SignFormatPdf);
            w.gfx.text_in(&rt, &brush, tr(k), (
                r.x,
                r.y,
                r.w,
                r.h,
            ), window::UI_TEXT * ui, if on { 600 } else { 400 },
                if on { p.text } else { p.secondary }, 1.0, Align::Center);
            continue;
        }
        let busy = w.phase == Phase::Saving && matches!(hit, Hit::Save | Hit::Cancel);
        let hot = w.hover == Some(*hit);
        let (bg, fg) = match hit {
            Hit::Save | Hit::SaveAnyway | Hit::OpenPassword => (p.accent, p.on_accent),
            Hit::Discard => (p.error, if p.light { 0xFFFFFF } else { 0x1A1210 }),
            _ if hot => (p.button_hover, p.text),
            _ => (p.button, p.text),
        };
        let a = if busy { 0.5 } else { 1.0 };
        fill_round(&rt, &brush, *r, 10.0 * ui, bg, a);
        if let Some(k) = layout::label_key(*hit) {
            w.gfx.text_in(
                &rt,
                &brush,
                tr(k),
                (r.x, r.y, r.w, r.h),
                window::UI_TEXT * ui,
                weight_of(*hit),
                fg,
                a,
                Align::Center,
            );
        }
    }

    // The item bar and popovers sit over everything else.
    if let Some(v) = view(w).filter(|_| ready) {
        if let Some(bar) = float_bar(w, &v) {
            draw_popover_panel(&rt, &brush, &bar.panel, &p, ui);
            for (hit, r) in &bar.items {
                if w.hover == Some(*hit) {
                    fill_round(&rt, &brush, *r, 7.0 * ui, p.hover, 1.0);
                }
                if let Some(k) = layout::label_key(*hit) {
                    w.gfx.text_in(
                        &rt,
                        &brush,
                        tr(k),
                        (r.x, r.y, r.w, r.h),
                        12.5 * ui,
                        400,
                        p.text,
                        1.0,
                        Align::Center,
                    );
                }
            }
        }
    }
    if let Some(pop) = popover(w) {
        draw_popover_panel(&rt, &brush, &pop.panel, &p, ui);
        let picker = match w.pop {
            Some(Pop::Picker { initials }) => Some(super::saved_marks(initials, w.demo)),
            _ => None,
        };
        for (hit, r) in &pop.items {
            let hot = w.hover == Some(*hit);
            match (*hit, &picker) {
                (Hit::PickMark(i), Some(marks)) => {
                    fill_round(&rt, &brush, *r, 8.0 * ui, PAPER, 1.0);
                    if hot {
                        brush_set(&brush, p.accent, 1.0);
                        stroke_round(&rt, &brush, *r, 8.0 * ui, 2.0 * ui);
                    }
                    if let Some(m) = marks.get(i) {
                        draw_mark_in(w, &rt, m, inflate(*r, -6.0 * ui));
                    }
                }
                (Hit::PickNew, _) => {
                    if hot {
                        fill_round(&rt, &brush, *r, 8.0 * ui, p.hover, 1.0);
                    }
                    brush_set(&brush, p.secondary, 0.8);
                    stroke_round(&rt, &brush, inflate(*r, -0.5), 8.0 * ui, 1.0);
                    w.gfx.text_in(
                        &rt,
                        &brush,
                        tr(Key::SignMakerNew),
                        (r.x, r.y, r.w, r.h),
                        12.5 * ui,
                        500,
                        p.text,
                        1.0,
                        Align::Center,
                    );
                }
                (h, _) => {
                    if hot {
                        fill_round(&rt, &brush, *r, 7.0 * ui, p.hover, 1.0);
                    }
                    if let Some(icon) = icon_of(h) {
                        draw_icon(
                            &rt,
                            &w.gfx.d2d,
                            &brush,
                            &w.gfx.round,
                            icon,
                            (r.x + 10.0 * ui, r.y + (r.h - 16.0 * ui) / 2.0),
                            16.0 * ui,
                            p.secondary,
                            1.0,
                        );
                    }
                    if let Some(k) = layout::label_key(h) {
                        let tx = r.x + 36.0 * ui;
                        w.gfx.text_in(
                            &rt,
                            &brush,
                            tr(k),
                            (tx, r.y, r.right() - tx, r.h),
                            window::UI_TEXT * ui,
                            400,
                            p.text,
                            1.0,
                            Align::Left,
                        );
                    }
                }
            }
        }
    }

    // The keyboard's place.
    if let Some(r) = w.focus.and_then(|f| window::focus_rect(w, f)) {
        brush_set(&brush, p.accent, 1.0);
        stroke_round(&rt, &brush, inflate(r, 2.0 * ui), 11.0 * ui, 2.0 * ui);
    }

    // The maker is up: everything behind it steps back.
    if w.maker.is_some() {
        fill(
            &rt,
            &brush,
            Rect { x: 0.0, y: 0.0, w: w.client.0 as f32, h: w.client.1 as f32 },
            0x000000,
            if p.light { 0.25 } else { 0.45 },
        );
    }
    unsafe { rt.EndDraw(None, None) }
}

fn layout_icon(hit: Hit) -> Option<Icon> {
    icon_of(hit)
}

fn brush_set(brush: &ID2D1SolidColorBrush, rgb: u32, a: f32) {
    unsafe { brush.SetColor(&color(rgb, a)) };
}

fn page_count_text(w: &Win) -> String {
    let Some(d) = &w.doc else { return String::new() };
    let live = d.session.live_pages();
    let n = live.iter().position(|&p| p == w.cur).map_or(1, |i| i + 1);
    tr(Key::SignPageOf).replace("{n}", &n.to_string()).replace("{total}", &live.len().to_string())
}

fn draw_popover_panel(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    r: &Rect,
    p: &theme::WindowPalette,
    ui: f32,
) {
    // A short, soft shadow: the popover floats over the page it acts on.
    for (i, a) in [0.10f32, 0.06, 0.03].iter().enumerate() {
        let grow = (i as f32 + 1.0) * 2.0 * ui;
        fill_round(
            rt,
            brush,
            Rect { x: r.x - grow / 2.0, y: r.y + grow / 2.0, w: r.w + grow, h: r.h + grow },
            10.0 * ui + grow,
            0x000000,
            *a,
        );
    }
    fill_round(rt, brush, *r, 10.0 * ui, p.popover, 1.0);
    let (b, ba) = rgba(p.border);
    brush_set(brush, b, ba.max(0.14));
    stroke_round(rt, brush, inflate(*r, -0.5), 10.0 * ui, 1.0);
}

/// `m` fitted (aspect kept) and centred in `r`.
fn draw_mark_in(w: &mut Win, rt: &ID2D1RenderTarget, m: &Arc<Mark>, r: Rect) {
    let Some(b) = mark_bitmap(w, rt, m) else { return };
    let (mw, mh) = (m.w.max(1) as f32, m.h.max(1) as f32);
    let s = (r.w / mw).min(r.h / mh);
    let (dw, dh) = (mw * s, mh * s);
    unsafe {
        rt.DrawBitmap(
            &b,
            Some(&rect_f(r.x + (r.w - dw) / 2.0, r.y + (r.h - dh) / 2.0, dw, dh)),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            None,
        );
    }
}

/// A stored render as a bitmap of this target, made on first use.
fn shot_bitmap(rt: &ID2D1RenderTarget, s: &mut window::Shot) -> Option<ID2D1Bitmap> {
    if s.bmp.is_none() {
        s.bmp = bitmap(rt, s.px.w, s.px.h, &s.px.bgra).ok();
    }
    s.bmp.clone()
}

/// A render of the page (drawn before the user's turn) into the shown rect
/// `r`, turned by `extra`.
fn draw_turned(rt: &ID2D1RenderTarget, b: &ID2D1Bitmap, r: Rect, extra: Quarter) {
    let (bw, bh) = if odd(extra) { (r.h, r.w) } else { (r.w, r.h) };
    unsafe {
        rt.SetTransform(&turn_matrix(neg(extra), r.x + r.w / 2.0, r.y + r.h / 2.0));
        rt.DrawBitmap(
            b,
            Some(&rect_f(-bw / 2.0, -bh / 2.0, bw, bh)),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            None,
        );
        rt.SetTransform(&Matrix3x2::identity());
    }
}

fn draw_page(w: &mut Win, rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush) -> windows::core::Result<()> {
    let Some(v) = view(w) else { return Ok(()) };
    let ui = ui_of(w);
    let r = v.page;
    let shadow = if w.p.light { 0.10 } else { 0.28 };
    for (i, a) in [shadow, shadow * 0.6, shadow * 0.3].iter().enumerate() {
        let g = (i as f32 + 1.0) * 3.0 * ui;
        fill_round(
            rt,
            brush,
            Rect { x: r.x - g / 2.0, y: r.y + g / 3.0, w: r.w + g, h: r.h + g },
            3.0 * ui + g,
            0x000000,
            *a,
        );
    }
    // Paper first: images keep their transparency.
    fill(rt, brush, r, 0xFFFFFF, 1.0);
    let cur = w.cur;
    let bmp = w.pages.get_mut(&cur).and_then(|s| shot_bitmap(rt, s));
    let bmp = bmp.or_else(|| w.thumbs.get_mut(&cur).and_then(|s| shot_bitmap(rt, s)));
    unsafe {
        rt.PushAxisAlignedClip(&rect_f(r.x, r.y, r.w, r.h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
    if let Some(b) = &bmp {
        draw_turned(rt, b, r, v.extra);
    }
    draw_items(w, rt, brush, &v, w.cur);
    unsafe {
        rt.PopAxisAlignedClip();
    }
    // The selection: a dashed outline in the accent's paper shade and four
    // handles.
    if let Some(sel) = w.selected.filter(|_| w.text.is_none())
        && let Some(it) = shown_item(w, &v, sel)
        && it.page == w.cur
    {
        let ir = v.item_rect(&it);
        let accent = theme::current().accent.light_rgb();
        brush_set(brush, accent, 1.0);
        let o = inflate(ir, 3.0 * ui);
        unsafe {
            rt.DrawRectangle(&rect_f(o.x, o.y, o.w, o.h), brush, 1.5 * ui, &w.gfx.dash);
        }
        if !matches!(w.drag, Some(Drag::Move { .. })) {
            // On the outline, not the item: a small item stays visible.
            for (_, drawn, _) in layout::handles(o, ui) {
                fill_round(rt, brush, drawn, 2.0 * ui, 0xFFFFFF, 1.0);
                brush_set(brush, accent, 1.0);
                stroke_round(rt, brush, drawn, 2.0 * ui, 1.5 * ui);
            }
        }
    }
    Ok(())
}

/// The items of session page `page` as `v` shows that page.
fn draw_items(w: &mut Win, rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush, v: &View, page: usize) {
    let Some(doc) = &w.doc else { return };
    let editing = w.text.as_ref().and_then(|t| t.1);
    let list: Vec<usize> =
        (0..doc.session.items.len()).filter(|&i| doc.session.items[i].page == page && Some(i) != editing).collect();
    let shown = v.shown();
    for i in list {
        let Some(it) = shown_item(w, v, i) else { continue };
        let ir = v.item_rect(&it);
        let (bw, bh) = (it.w * v.scale, it.h * v.scale);
        let m = turn_matrix(it.turn.add(neg(shown)), ir.x + ir.w / 2.0, ir.y + ir.h / 2.0);
        match &it.kind {
            ItemKind::Signature(mk) | ItemKind::Initials(mk) => {
                if let Some(b) = mark_bitmap(w, rt, mk) {
                    unsafe {
                        rt.SetTransform(&m);
                        rt.DrawBitmap(
                            &b,
                            Some(&rect_f(-bw / 2.0, -bh / 2.0, bw, bh)),
                            1.0,
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            None,
                        );
                    }
                }
            }
            ItemKind::Text(s, _) | ItemKind::Date(s, _) => unsafe {
                rt.SetTransform(&m);
                brush_set(brush, INK, 1.0);
                ink_text(rt, &w.gfx.dwrite, brush, s, (-bw / 2.0, -bh / 2.0, bw, bh));
            },
            ItemKind::Check(_) => unsafe {
                rt.SetTransform(&m);
                brush_set(brush, INK, 1.0);
                draw_check(rt, brush, &w.gfx.round, bw, bh);
            },
        }
        unsafe {
            rt.SetTransform(&Matrix3x2::identity());
        }
    }
}

fn draw_rail(w: &mut Win, rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush) -> windows::core::Result<()> {
    if w.doc.is_none() {
        return Ok(());
    }
    let ui = ui_of(w);
    let rows = rail(w);
    window::want_thumbs(w, &rows);
    let l = w.layout.rail;
    let p = w.p;
    unsafe {
        rt.PushAxisAlignedClip(&rect_f(l.x, l.y, l.w, l.h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
    for (n, row) in rows.iter().enumerate() {
        if row.label.bottom() < l.y || row.thumb.y > l.bottom() {
            continue;
        }
        let on = row.page == w.cur;
        if on {
            brush_set(brush, p.accent, 1.0);
            stroke_round(rt, brush, inflate(row.thumb, 2.0 * ui), 4.0 * ui, 2.0 * ui);
        } else if w.hover == Some(Hit::Thumb(row.page)) {
            brush_set(brush, p.secondary, 0.8);
            stroke_round(rt, brush, inflate(row.thumb, 2.0 * ui), 4.0 * ui, 1.0 * ui);
        }
        fill(rt, brush, row.thumb, 0xFFFFFF, 1.0);
        let extra = w.doc.as_ref().and_then(|d| d.session.pages.get(row.page)).map_or(
            Quarter::R0,
            |pg| pg.extra,
        );
        let b = w.thumbs.get_mut(&row.page).and_then(|s| shot_bitmap(rt, s));
        if let Some(b) = b {
            draw_turned(rt, &b, row.thumb, extra);
        }
        // What is placed shows in the rail too.
        let pg = w.doc.as_ref().and_then(|d| d.session.pages.get(row.page)).map(|pg| pg.geom);
        if let Some(g) = pg {
            let (sw, _) = crate::sign::geom::shown_size(g, extra);
            let tv = View { page: row.thumb, scale: row.thumb.w / sw.max(1.0), g, extra };
            unsafe {
                rt.PushAxisAlignedClip(
                    &rect_f(row.thumb.x, row.thumb.y, row.thumb.w, row.thumb.h),
                    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                );
            }
            draw_items(w, rt, brush, &tv, row.page);
            unsafe {
                rt.PopAxisAlignedClip();
            }
        }
        let label = (n + 1).to_string();
        w.gfx.text_in(rt, brush, &label, (row.label.x, row.label.y, row.label.w, row.label.h), 11.5 * ui,
            if on { 600 } else { 400 }, if on { p.text } else { p.secondary }, 1.0, Align::Center);
    }
    unsafe {
        rt.PopAxisAlignedClip();
    }
    Ok(())
}

fn draw_footer_text(w: &Win, rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush) {
    let ui = ui_of(w);
    let p = w.p;
    let r = w.layout.foot_text;
    let bx = (r.x, r.y, r.w, r.h);
    let size = 12.5 * ui;
    let line = |s: &str, weight: i32, rgb: u32| {
        w.gfx.text_in(rt, brush, s, bx, size, weight, rgb, 1.0, Align::Left);
    };
    match w.ask {
        Ask::Discard => return line(tr(Key::EditDiscardAsk), 600, p.text),
        Ask::Breaks => return line(tr(Key::SignBreaksSignature), 600, p.text),
        Ask::None => {}
    }
    if w.phase == Phase::Saving {
        return line(tr(Key::SignSaving), 500, p.text);
    }
    if let Some((s, tone)) = &w.note {
        return line(s, 500, if *tone == Tone::Error { p.error } else { p.text });
    }
    if w.armed.is_some() {
        return line(tr(Key::SignPlaceHint), 500, p.text);
    }
    if !matches!(w.phase, Phase::Ready) {
        return;
    }
    let Some((path, fell_back)) = &w.out else { return };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tpl = tr(if *fell_back { Key::SignSaveAsFallback } else { Key::SignSaveAs });
    let text = tpl.replace("{name}", &name);
    // The file name stands out by weight; the rest is quieter.
    let start = tpl.find("{name}").map_or(0, |i| text[..i].encode_utf16().count());
    let len = name.encode_utf16().count();
    let Ok(layout) = w.gfx.layout(&text, size, 400) else { return line(&text, 400, p.secondary) };
    unsafe {
        let _ = layout.SetMaxWidth(r.w);
        let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = layout.SetFontWeight(
            DWRITE_FONT_WEIGHT(600),
            DWRITE_TEXT_RANGE { startPosition: start as u32, length: len as u32 },
        );
        let mut m = DWRITE_TEXT_METRICS::default();
        let th = if layout.GetMetrics(&mut m).is_ok() { m.height } else { size * 1.3 };
        rt.PushAxisAlignedClip(&rect_f(r.x, r.y, r.w, r.h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        brush_set(brush, p.secondary, 1.0);
        rt.DrawTextLayout(
            Vector2::new(r.x, r.y + (r.h - th) / 2.0),
            &layout,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        rt.PopAxisAlignedClip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_turn_draws_clockwise_and_an_upright_item_stays_upright() {
        // The renderer's bitmap turned by the user's quarter: x axis goes down.
        let m = turn_matrix(neg(Quarter::R90), 0.0, 0.0);
        let x = (1.0 * m.M11 + 0.0 * m.M21, 1.0 * m.M12 + 0.0 * m.M22);
        assert_eq!(x, (0.0, 1.0));
        // An item placed upright on a page shown at R90 has turn R90: no
        // visual turn.
        assert_eq!(Quarter::R90.add(neg(Quarter::R90)), Quarter::R0);
    }

    #[test]
    fn the_mark_cache_keeps_only_marks_still_held() {
        let held = Arc::new(Mark { w: 1, h: 1, rgba: vec![0; 4] });
        let gone = Arc::new(Mark { w: 1, h: 1, rgba: vec![0; 4] });
        let mut cache = std::collections::HashMap::new();
        cache.insert(Arc::as_ptr(&held) as usize, (held.clone(), ()));
        cache.insert(Arc::as_ptr(&gone) as usize, (gone.clone(), ()));
        drop(gone);
        evict_unused(&mut cache);
        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key(&(Arc::as_ptr(&held) as usize)));
    }

    #[test]
    fn premultiplying_scales_colour_by_alpha() {
        let m = Mark { w: 1, h: 1, rgba: vec![200, 100, 50, 128] };
        assert_eq!(premultiplied(&m), vec![25, 50, 100, 128]);
    }

    #[test]
    fn every_tool_has_a_picture() {
        for h in [Hit::Tool(Tool::Signature), Hit::Tool(Tool::Text), Hit::Rotate, Hit::DeletePage, Hit::More] {
            assert!(icon_of(h).is_some());
        }
        assert!(icon_of(Hit::Save).is_none());
    }
}
