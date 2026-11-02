//! The editor's icons and the little fills they are drawn with, shared by
//! the editor's toolbar and the inline capture's tool strip so both show the
//! same pictures. Everything is Direct2D on a render target in pixels
//! (`ui` scales the drawing); nothing is kept.

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
};
use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, ID2D1SolidColorBrush};
use windows_numerics::Vector2;

use super::Tool;
use super::gfx::{Align, Gfx, color, rect_f};
use crate::ui::geometry::Rect;

pub fn fill(rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush, r: Rect, rgb: u32, a: f32) {
    unsafe {
        brush.SetColor(&color(rgb, a));
        rt.FillRectangle(&rect_f(r.x, r.y, r.w, r.h), brush);
    }
}

pub fn fill_round(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    r: Rect,
    radius: f32,
    rgb: u32,
    a: f32,
) {
    use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;
    unsafe {
        brush.SetColor(&color(rgb, a));
        rt.FillRoundedRectangle(
            &D2D1_ROUNDED_RECT {
                rect: rect_f(r.x, r.y, r.w, r.h),
                radiusX: radius,
                radiusY: radius,
            },
            brush,
        );
    }
}

pub fn line(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    gfx: &Gfx,
    a: (f32, f32),
    b: (f32, f32),
    w: f32,
) {
    unsafe {
        rt.DrawLine(
            Vector2::new(a.0, a.1),
            Vector2::new(b.0, b.1),
            brush,
            w,
            &gfx.round,
        );
    }
}

/// A path through `pts`, stroked (and closed/filled when asked).
pub fn poly(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    gfx: &Gfx,
    pts: &[(f32, f32)],
    width: f32,
    closed_fill: bool,
) {
    let Some(first) = pts.first() else { return };
    unsafe {
        let Ok(g) = gfx.d2d.CreatePathGeometry() else {
            return;
        };
        let Ok(sink) = g.Open() else { return };
        sink.BeginFigure(Vector2::new(first.0, first.1), D2D1_FIGURE_BEGIN_FILLED);
        for p in &pts[1..] {
            sink.AddLine(Vector2::new(p.0, p.1));
        }
        sink.EndFigure(if closed_fill {
            D2D1_FIGURE_END_CLOSED
        } else {
            D2D1_FIGURE_END_OPEN
        });
        if sink.Close().is_err() {
            return;
        }
        if closed_fill {
            rt.FillGeometry(&g, brush, None);
        } else {
            rt.DrawGeometry(&g, brush, width, &gfx.round);
        }
    }
}

/// What an icon shows: a tool, or undo and redo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Tool(Tool),
    Undo,
    Redo,
}

/// A toolbar icon drawn from a few lines, centred in `r`.
#[allow(clippy::too_many_arguments)]
pub fn glyph(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    gfx: &Gfx,
    g: Glyph,
    r: Rect,
    rgb: u32,
    a: f32,
    ui: f32,
) {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let u = 7.5 * ui;
    let sw = 1.7 * ui;
    unsafe {
        brush.SetColor(&color(rgb, a));
        match g {
            Glyph::Tool(Tool::Select) => poly(
                rt,
                brush,
                gfx,
                &[
                    (cx - u * 0.55, cy - u),
                    (cx - u * 0.55, cy + u * 0.75),
                    (cx - u * 0.1, cy + u * 0.3),
                    (cx + u * 0.25, cy + u),
                    (cx + u * 0.55, cy + u * 0.85),
                    (cx + u * 0.2, cy + u * 0.2),
                    (cx + u * 0.75, cy + u * 0.2),
                ],
                sw,
                true,
            ),
            Glyph::Tool(Tool::Text) => {
                gfx.text_in(
                    rt,
                    brush,
                    "T",
                    (r.x, r.y, r.w, r.h),
                    18.0 * ui,
                    700,
                    rgb,
                    a,
                    Align::Center,
                );
            }
            Glyph::Tool(Tool::Rect) => {
                rt.DrawRectangle(
                    &rect_f(cx - u, cy - u * 0.75, u * 2.0, u * 1.5),
                    brush,
                    sw,
                    &gfx.round,
                );
            }
            Glyph::Tool(Tool::Line) => {
                line(
                    rt,
                    brush,
                    gfx,
                    (cx - u * 0.9, cy + u * 0.9),
                    (cx + u * 0.9, cy - u * 0.9),
                    sw,
                );
            }
            Glyph::Tool(Tool::Arrow) => {
                line(
                    rt,
                    brush,
                    gfx,
                    (cx - u * 0.9, cy + u * 0.9),
                    (cx + u * 0.9, cy - u * 0.9),
                    sw,
                );
                poly(
                    rt,
                    brush,
                    gfx,
                    &[
                        (cx + u * 0.0, cy - u * 0.9),
                        (cx + u * 0.9, cy - u * 0.9),
                        (cx + u * 0.9, cy + u * 0.0),
                    ],
                    sw,
                    false,
                );
            }
            Glyph::Tool(Tool::Pen) => poly(
                rt,
                brush,
                gfx,
                &[
                    (cx - u, cy + u * 0.5),
                    (cx - u * 0.5, cy - u * 0.6),
                    (cx, cy + u * 0.4),
                    (cx + u * 0.5, cy - u * 0.5),
                    (cx + u, cy + u * 0.3),
                ],
                sw,
                false,
            ),
            Glyph::Tool(Tool::Marker) => {
                brush.SetColor(&color(rgb, a * 0.45));
                line(
                    rt,
                    brush,
                    gfx,
                    (cx - u * 0.8, cy + u * 0.2),
                    (cx + u * 0.8, cy - u * 0.5),
                    6.0 * ui,
                );
                brush.SetColor(&color(rgb, a));
                line(
                    rt,
                    brush,
                    gfx,
                    (cx - u * 0.9, cy + u * 0.95),
                    (cx + u * 0.9, cy + u * 0.95),
                    sw,
                );
            }
            Glyph::Tool(Tool::Pixelate) => {
                let cell = u * 0.66;
                let (x0, y0) = (cx - cell * 1.5, cy - cell * 1.5);
                for j in 0..3 {
                    for i in 0..3 {
                        let on = (i + j) % 2 == 0;
                        brush.SetColor(&color(rgb, if on { a } else { a * 0.35 }));
                        rt.FillRectangle(
                            &rect_f(
                                x0 + i as f32 * cell,
                                y0 + j as f32 * cell,
                                cell - 1.0 * ui,
                                cell - 1.0 * ui,
                            ),
                            brush,
                        );
                    }
                }
            }
            Glyph::Undo | Glyph::Redo => {
                let m = if matches!(g, Glyph::Undo) { 1.0 } else { -1.0 };
                let x = |dx: f32| cx - m * dx * u;
                poly(
                    rt,
                    brush,
                    gfx,
                    &[
                        (x(0.9), cy - u * 0.1),
                        (x(-0.2), cy - u * 0.1),
                        (x(-0.9), cy + u * 0.35),
                        (x(-0.9), cy + u * 0.6),
                        (x(-0.5), cy + u * 0.9),
                        (x(0.1), cy + u * 0.9),
                    ],
                    sw,
                    false,
                );
                poly(
                    rt,
                    brush,
                    gfx,
                    &[
                        (x(0.9) + m * u * 0.6, cy - u * 0.75),
                        (x(0.9), cy - u * 0.1),
                        (x(0.9) + m * u * 0.6, cy + u * 0.55),
                    ],
                    sw,
                    false,
                );
            }
        }
    }
}
