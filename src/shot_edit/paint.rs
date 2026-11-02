//! Drawing the annotations. The canvas and the export both go through
//! `draw_annotations`, so what is saved is what was seen: the canvas passes
//! its `View` (scale and offset), the export the identity at full size.

use std::collections::HashMap;

use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_OPEN};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, ID2D1Bitmap, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::core::Result;
use windows_numerics::Vector2;

use super::gfx::{Gfx, color, rect_f};
use super::{
    Annotation, FRect, MARKER_ALPHA, MARKER_WIDTH_X, Pt, View, clip_to_image, pixel_block, pixelate,
};
use crate::capture::Shot;

/// Weight of annotation text (a medium DM Sans).
const TEXT_WEIGHT: i32 = 500;

/// The mosaic bitmaps made so far, by the region they cover. Valid for one
/// render target and one picture; dropped when either goes.
#[derive(Default)]
pub struct Pix {
    /// By region (x, y, w, h) and the mosaic's block size.
    map: HashMap<(u32, u32, u32, u32, u32), ID2D1Bitmap>,
}

/// A part of the picture (x, y, w, h in whole pixels).
pub type Region = (u32, u32, u32, u32);

/// Where a `Pixelate` of `r` shows and how coarse its blocks are.
///
/// Without `clip` it is `r` cut to the picture (`w` × `h`) and a block that
/// follows the picture's size, as in the editor and the export. With `clip`
/// (the inline capture: the picture is the whole frozen screen, the part
/// that will be kept is the selection) it is `r` cut to the clip and then to
/// the picture, and the block follows the clip's size. Either way the mosaic
/// of what is seen in the overlay is cell for cell the one the export of the
/// cropped selection makes (the crop is the clip, and `r` moved by its
/// corner): the cells are aligned to the region's corner, which is the same
/// pixel in both.
pub fn pixel_region(r: FRect, w: u32, h: u32, clip: Option<Region>) -> Option<(Region, u32)> {
    let Some((cx, cy, cw, ch)) = clip else {
        return Some((clip_to_image(r, w, h)?, pixel_block(w, h)));
    };
    let moved = FRect::new(r.x - cx as f32, r.y - cy as f32, r.w, r.h);
    let (x, y, rw, rh) = clip_to_image(moved, cw, ch)?;
    let (x, y) = (x + cx, y + cy);
    // The clip itself may reach past the picture.
    let (rw, rh) = (rw.min(w.saturating_sub(x)), rh.min(h.saturating_sub(y)));
    (rw > 0 && rh > 0).then_some(((x, y, rw, rh), pixel_block(cw, ch)))
}

impl Pix {
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Mosaics kept at most; a drag makes a new one each step.
    const KEEP: usize = 24;

    fn get(
        &mut self,
        rt: &ID2D1RenderTarget,
        base: &Shot,
        region: Region,
        block: u32,
    ) -> Option<ID2D1Bitmap> {
        let key = (region.0, region.1, region.2, region.3, block);
        if let Some(b) = self.map.get(&key) {
            return Some(b.clone());
        }
        let px = pixelate(&base.bgra, base.w, base.h, region, block)?;
        let bitmap = super::gfx::bitmap(rt, region.2, region.3, &px).ok()?;
        if self.map.len() >= Self::KEEP {
            self.map.clear();
        }
        self.map.insert(key, bitmap.clone());
        Some(bitmap)
    }
}

/// Draws `doc` over a picture already drawn: each annotation placed by
/// `view`. `skip` leaves one out (the text being edited in a native EDIT).
#[allow(clippy::too_many_arguments)]
pub fn draw_annotations(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    gfx: &Gfx,
    base: &Shot,
    doc: &[Annotation],
    view: &View,
    skip: Option<usize>,
    pix: &mut Pix,
) {
    draw_annotations_clipped(rt, brush, gfx, base, doc, view, skip, pix, None);
}

/// `draw_annotations` where only `clip` (x, y, w, h of `base`) will be kept:
/// a mosaic is limited to it and sized by it (see `pixel_region`). The
/// caller sets the render target's own clip for everything else.
#[allow(clippy::too_many_arguments)]
pub fn draw_annotations_clipped(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    gfx: &Gfx,
    base: &Shot,
    doc: &[Annotation],
    view: &View,
    skip: Option<usize>,
    pix: &mut Pix,
    clip: Option<Region>,
) {
    for (i, ann) in doc.iter().enumerate() {
        if skip == Some(i) {
            continue;
        }
        let _ = draw_one(rt, brush, gfx, base, ann, view, pix, clip);
    }
}

fn v(view: &View, p: Pt) -> Vector2 {
    let (x, y) = view.to_view(p);
    Vector2::new(x, y)
}

#[allow(clippy::too_many_arguments)]
fn draw_one(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    gfx: &Gfx,
    base: &Shot,
    ann: &Annotation,
    view: &View,
    pix: &mut Pix,
    clip: Option<Region>,
) -> Result<()> {
    unsafe {
        match ann {
            Annotation::Rect { r, style } => {
                brush.SetColor(&color(style.color, 1.0));
                let (x, y) = view.to_view(Pt::new(r.x, r.y));
                rt.DrawRectangle(
                    &rect_f(x, y, r.w * view.scale, r.h * view.scale),
                    brush,
                    style.width * view.scale,
                    &gfx.round,
                );
            }
            Annotation::Arrow { a, b, style } => {
                brush.SetColor(&color(style.color, 1.0));
                let w = style.width * view.scale;
                rt.DrawLine(v(view, *a), v(view, *b), brush, w, &gfx.round);
                let [l, r] = arrow_head(*a, *b, style.width);
                let head = [l, *b, r];
                stroke_path(rt, brush, gfx, view, &head, w)?;
            }
            Annotation::Line { a, b, style } => {
                brush.SetColor(&color(style.color, 1.0));
                rt.DrawLine(
                    v(view, *a),
                    v(view, *b),
                    brush,
                    style.width * view.scale,
                    &gfx.round,
                );
            }
            Annotation::Pen { pts, style } => {
                brush.SetColor(&color(style.color, 1.0));
                stroke_path(rt, brush, gfx, view, pts, style.width * view.scale)?;
            }
            Annotation::Marker { pts, style } => {
                brush.SetColor(&color(style.color, MARKER_ALPHA));
                let w = style.width * MARKER_WIDTH_X * view.scale;
                stroke_path(rt, brush, gfx, view, pts, w)?;
            }
            Annotation::Text {
                pos,
                text,
                size,
                color: rgb,
            } => {
                let (x, y) = view.to_view(*pos);
                gfx.text(
                    rt,
                    brush,
                    text,
                    (x, y),
                    size * view.scale,
                    TEXT_WEIGHT,
                    *rgb,
                    1.0,
                );
            }
            Annotation::Pixelate { r } => {
                let Some((region, block)) = pixel_region(*r, base.w, base.h, clip) else {
                    return Ok(());
                };
                let Some(bitmap) = pix.get(rt, base, region, block) else {
                    return Ok(());
                };
                let (x, y) = view.to_view(Pt::new(region.0 as f32, region.1 as f32));
                let dest = rect_f(
                    x,
                    y,
                    region.2 as f32 * view.scale,
                    region.3 as f32 * view.scale,
                );
                rt.DrawBitmap(
                    &bitmap,
                    Some(&dest),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                    None,
                );
            }
        }
    }
    Ok(())
}

/// The two ends of an arrow's head: back from `b` along the shaft, spread
/// to either side (image px).
pub fn arrow_head(a: Pt, b: Pt, width: f32) -> [Pt; 2] {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = dx.hypot(dy).max(f32::EPSILON);
    let (ux, uy) = (dx / len, dy / len);
    // Never longer than the shaft itself.
    let head = (width * 3.0 + 10.0).min(len);
    let spread = 0.5f32;
    let rot = |sign: f32| {
        let (s, c) = (spread.sin() * sign, spread.cos());
        Pt::new(
            b.x - head * (ux * c - uy * s),
            b.y - head * (ux * s + uy * c),
        )
    };
    [rot(1.0), rot(-1.0)]
}

/// An open path through `pts` stroked `width` px wide (one geometry, so a
/// translucent marker does not darken where it crosses itself).
fn stroke_path(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    gfx: &Gfx,
    view: &View,
    pts: &[Pt],
    width: f32,
) -> Result<()> {
    let Some(first) = pts.first() else {
        return Ok(());
    };
    unsafe {
        if pts.len() == 1 {
            // A dot: a zero-length line with round caps.
            rt.DrawLine(v(view, *first), v(view, *first), brush, width, &gfx.round);
            return Ok(());
        }
        let geometry = gfx.d2d.CreatePathGeometry()?;
        let sink = geometry.Open()?;
        sink.BeginFigure(v(view, *first), D2D1_FIGURE_BEGIN_FILLED);
        for p in &pts[1..] {
            sink.AddLine(v(view, *p));
        }
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
        rt.DrawGeometry(&geometry, brush, width, &gfx.round);
    }
    Ok(())
}

/// The outline and handles of the selected annotation, in `accent`.
pub fn draw_selection(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    ann: &Annotation,
    view: &View,
    accent: u32,
    ui: f32,
) {
    let b: FRect = ann.bounds();
    let (x, y) = view.to_view(Pt::new(b.x, b.y));
    unsafe {
        brush.SetColor(&color(accent, 0.9));
        rt.DrawRectangle(
            &rect_f(x, y, b.w * view.scale, b.h * view.scale),
            brush,
            1.0 * ui,
            None,
        );
        let hs = 4.5 * ui;
        for (_, p) in ann.handles() {
            let (hx, hy) = view.to_view(p);
            let r = rect_f(hx - hs, hy - hs, hs * 2.0, hs * 2.0);
            brush.SetColor(&color(0xFFFFFF, 1.0));
            rt.FillRectangle(&r, brush);
            brush.SetColor(&color(accent, 1.0));
            rt.DrawRectangle(&r, brush, 1.5 * ui, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrow_head_sits_behind_the_tip_on_both_sides() {
        let [l, r] = arrow_head(Pt::new(0.0, 0.0), Pt::new(100.0, 0.0), 4.0);
        // Behind the tip (x < 100), mirrored around the shaft.
        assert!(l.x < 100.0 && r.x < 100.0);
        assert!((l.x - r.x).abs() < 1e-3);
        assert!((l.y + r.y).abs() < 1e-3);
        assert!(l.y.abs() > 1.0);
        // The same head turned by 90 degrees.
        let [l2, r2] = arrow_head(Pt::new(0.0, 0.0), Pt::new(0.0, 100.0), 4.0);
        assert!(l2.y < 100.0 && r2.y < 100.0);
        assert!((l2.y - r2.y).abs() < 1e-3);
    }

    /// A frame of `w` × `h` with a pattern that differs from pixel to pixel.
    fn frame(w: u32, h: u32) -> Shot {
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let v = (x * 7 + y * 13 + (x * y) % 11) as u8;
                bgra.extend([v, v.wrapping_mul(3), v.wrapping_add(x as u8), 255]);
            }
        }
        Shot { w, h, bgra }
    }

    #[test]
    fn without_a_clip_the_mosaic_is_the_editors_own() {
        let r = FRect::new(-5.0, 2.5, 20.0, 10.2);
        assert_eq!(
            pixel_region(r, 100, 50, None),
            Some(((0, 2, 15, 11), pixel_block(100, 50)))
        );
        assert_eq!(
            pixel_region(FRect::new(200.0, 0.0, 5.0, 5.0), 100, 50, None),
            None
        );
    }

    #[test]
    fn a_mosaic_seen_in_the_overlay_is_the_one_the_export_makes() {
        // The frozen screen, and the selection the crop is cut from.
        let screen = frame(2400, 1500);
        let sel = (120u32, 80u32, 300u32, 200u32);
        let crop = crate::capture::crop(
            &screen.bgra,
            screen.w as usize * 4,
            crate::capture::Rect::new(0, 0, 2400, 1500),
            crate::capture::Rect::new(sel.0 as i32, sel.1 as i32, sel.2 as i32, sel.3 as i32),
        )
        .expect("a crop");
        // Pokes out of the selection on the left and the top, and ends
        // inside: and one that pokes out on the right and the bottom.
        for r in [
            FRect::new(60.5, 40.25, 190.0, 150.0),
            FRect::new(330.0, 220.75, 180.0, 160.0),
            FRect::new(150.0, 100.0, 40.0, 40.0),
        ] {
            // The overlay: frame base, clip = selection.
            let (region, block) = pixel_region(r, screen.w, screen.h, Some(sel)).expect("seen");
            let seen = pixelate(&screen.bgra, screen.w, screen.h, region, block).expect("pixels");
            // The export: crop base, no clip, the rectangle moved by the
            // selection's corner (what `Annotation::move_by` does).
            let moved = FRect::new(r.x - sel.0 as f32, r.y - sel.1 as f32, r.w, r.h);
            let (region2, block2) = pixel_region(moved, crop.w, crop.h, None).expect("exported");
            let made = pixelate(&crop.bgra, crop.w, crop.h, region2, block2).expect("pixels");
            assert_eq!(block, block2, "{r:?}");
            assert_eq!(block, pixel_block(sel.2, sel.3), "the clip's size decides");
            assert_eq!(
                (region.0 - sel.0, region.1 - sel.1, region.2, region.3),
                region2,
                "{r:?}"
            );
            assert_eq!(seen, made, "{r:?}");
            // And it stays inside the selection.
            assert!(region.0 >= sel.0 && region.0 + region.2 <= sel.0 + sel.2);
            assert!(region.1 >= sel.1 && region.1 + region.3 <= sel.1 + sel.3);
        }
        // Outside the selection altogether: nothing to draw.
        assert_eq!(
            pixel_region(FRect::new(500.0, 400.0, 50.0, 50.0), 2400, 1500, Some(sel)),
            None
        );
        // Without the clip the block would follow the whole frame and the
        // mosaic would differ: the reason the overlay passes the clip.
        assert_ne!(pixel_block(2400, 1500), pixel_block(sel.2, sel.3));
    }

    #[test]
    fn a_short_arrow_has_a_short_head() {
        let [l, _] = arrow_head(Pt::new(0.0, 0.0), Pt::new(5.0, 0.0), 12.0);
        assert!((l.x - 5.0).abs() <= 5.0 && l.y.abs() <= 5.0);
        // A zero-length arrow does not produce NaN.
        let [l, r] = arrow_head(Pt::new(3.0, 3.0), Pt::new(3.0, 3.0), 4.0);
        assert!(l.x.is_finite() && r.y.is_finite());
    }
}
