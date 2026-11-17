//! The mini face the shrunk notch shows: the chosen mascot's face colour, dark
//! eyes and the few small marks that tell the mascots apart. Pure data in the
//! eyes' 64-unit grid (the one `robot::paint_eyes` draws in); nothing here
//! paints or owns a timer. The accessory never changes it.

use super::cat_style::{CatLook, PATCH_DARK, PATCH_ORANGE};
use super::mascot::{FALK_BEAK, falk_ink, octopus_rgb, shade};
use super::robot::Seg;
use crate::theme::{CatColor, FalkColor, Mascot};

/// A small mark on the face, in the eyes' grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PeekMark {
    /// A filled circle in the eyes' grid.
    Dot { cx: f32, cy: f32, r: f32, rgb: u32 },
    /// The falk's beak, the spec's path, in the eyes' grid.
    Beak { rgb: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PeekFace {
    pub fill: u32,
    pub eye: u32,
    /// The 1 dip line on the pill's sides and bottom; `None` for none.
    pub edge: Option<u32>,
    /// Slide with the look and are clamped with the eyes (cheeks, beak).
    pub face_marks: Vec<PeekMark>,
    /// Fixed to the pill and clipped to it (calico patches, octopus spots).
    pub coat_marks: Vec<PeekMark>,
}

/// The falk's beak: `M30.375 43 Q32 42 33.625 43 Q33.5 45.5 32 46.375
/// Q30.5 45.5 30.375 43 Z`.
pub(crate) const BEAK_PATH: [Seg; 5] = [
    Seg::Move(30.375, 43.0),
    Seg::Quad(32.0, 42.0, 33.625, 43.0),
    Seg::Quad(33.5, 45.5, 32.0, 46.375),
    Seg::Quad(30.5, 45.5, 30.375, 43.0),
    Seg::Close,
];

const fn dot(cx: f32, cy: f32, r: f32, rgb: u32) -> PeekMark {
    PeekMark::Dot { cx, cy, r, rgb }
}

/// The island's fill while the notch shrinks to the face and back: the
/// surface colour at `mix` 0, the face colour at 1 (per channel), and the
/// alpha from the see-through `fill_alpha` up to opaque. `mix` is clamped.
pub fn peek_fill(surface: u32, fill_alpha: f32, face_fill: u32, mix: f32) -> (u32, f32) {
    let m = mix.clamp(0.0, 1.0);
    let rgb = super::mascot::mix(surface, face_fill, m);
    (rgb, fill_alpha + (1.0 - fill_alpha) * m)
}

/// Where the face mix (0 = notch surface, 1 = face) starts and heads for in
/// the current morph; the app eases it with the morph's own curve, as it
/// does the see-through fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeekMix {
    pub from: f32,
    pub to: f32,
}

impl PeekMix {
    pub const fn at_rest(mix: f32) -> Self {
        Self { from: mix, to: mix }
    }

    /// The mix at the morph's eased progress `t` (0..=1).
    pub fn at(&self, t: f32) -> f32 {
        self.from + (self.to - self.from) * t
    }

    /// A new morph toward `to`, starting from the mix `reached` now.
    pub fn retarget(&mut self, to: f32, reached: f32) {
        *self = Self { from: reached, to };
    }

    /// Heads for `to` from the eased progress `t` of the morph already
    /// running, which carries on: the mix at `t` stays where it is and the
    /// rest of the morph runs from there to `to`. At the morph's end (or
    /// past it) both ends are `to`.
    pub fn retarget_at(&mut self, to: f32, t: f32) {
        let reached = self.at(t);
        let left = 1.0 - t;
        self.from = if left > 1e-3 && !t.is_nan() {
            (reached - to * t) / left
        } else {
            to
        };
        self.to = to;
    }

    /// No morph: both ends are the target.
    pub fn settle(&mut self) {
        self.from = self.to;
    }
}

/// The face for `look`. `surface` is the island's surface colour, the edge
/// line of the pale coats (it keeps them from melting into a white title bar).
pub fn peek_face(look: CatLook, surface: u32) -> PeekFace {
    let plain = |fill: u32, eye: u32, edge: Option<u32>| PeekFace {
        fill,
        eye,
        edge,
        face_marks: Vec::new(),
        coat_marks: Vec::new(),
    };
    match look.mascot {
        Mascot::Cat => {
            let (head, eye) = (look.head_rgb(), look.eye_rgb());
            match look.color {
                CatColor::Terracotta => plain(head, eye, None),
                CatColor::Calico => PeekFace {
                    coat_marks: vec![
                        dot(17.625, 35.0, 4.25, PATCH_ORANGE),
                        dot(47.625, 45.625, 3.75, PATCH_DARK),
                    ],
                    ..plain(head, eye, Some(surface))
                },
                CatColor::White => plain(head, eye, Some(surface)),
                CatColor::Black => plain(head, eye, look.rim()),
            }
        }
        Mascot::Falk => {
            let ink = falk_ink(look.falk);
            let edge = (look.falk == FalkColor::WhiteFace).then_some(surface);
            let mut face_marks = Vec::new();
            if let Some(cheek) = ink.cheek {
                face_marks.push(dot(18.75, 44.0, 2.375, cheek));
                face_marks.push(dot(45.25, 44.0, 2.375, cheek));
            }
            face_marks.push(PeekMark::Beak { rgb: FALK_BEAK });
            PeekFace { face_marks, ..plain(ink.face, look.eye_rgb(), edge) }
        }
        Mascot::Octopus => {
            let fill = octopus_rgb(look.octopus);
            let spot = shade(fill);
            PeekFace {
                coat_marks: vec![dot(44.25, 36.5, 1.625, spot), dot(19.0, 35.75, 1.125, spot)],
                ..plain(fill, look.eye_rgb(), None)
            }
        }
    }
}

/// Grid boxes (left, top, right, bottom) of `marks`.
pub fn mark_boxes(marks: &[PeekMark]) -> Vec<(f32, f32, f32, f32)> {
    marks
        .iter()
        .map(|mark| match *mark {
            PeekMark::Dot { cx, cy, r, .. } => (cx - r, cy - r, cx + r, cy + r),
            // The control points bound a quadratic curve.
            PeekMark::Beak { .. } => {
                let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                let mut add = |x: f32, y: f32| {
                    b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
                };
                for seg in BEAK_PATH {
                    match seg {
                        Seg::Move(x, y) | Seg::Line(x, y) => add(x, y),
                        Seg::Quad(a, c, x, y) => {
                            add(a, c);
                            add(x, y);
                        }
                        Seg::Cubic(a, c, d, e, x, y) => {
                            add(a, c);
                            add(d, e);
                            add(x, y);
                        }
                        Seg::Close => {}
                    }
                }
                b
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{CatAccessory, OctopusColor};
    use crate::ui::cat_style::{AMBER, EYES_DARK, RIM, solid};
    use crate::ui::geometry::Rect;
    use crate::ui::robot::RobotPainter;
    use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, ID2D1SolidColorBrush};

    #[test]
    fn peek_fill_runs_from_the_surface_to_the_face() {
        assert_eq!(peek_fill(0x17120F, 1.0, 0xF6D04A, 0.0), (0x17120F, 1.0));
        assert_eq!(peek_fill(0x17120F, 1.0, 0xF6D04A, 1.0), (0xF6D04A, 1.0));
        assert_eq!(peek_fill(0x17120F, 0.4, 0xF6D04A, 1.0).1, 1.0);
        assert_eq!(peek_fill(0x17120F, 0.4, 0xF6D04A, 0.0).1, 0.4);
        // 0x00 .. 0xFF at the half: 127.5 rounds up.
        assert_eq!(peek_fill(0x000000, 1.0, 0xFFFFFF, 0.5).0, 0x808080);
        assert_eq!(peek_fill(0x17120F, 0.4, 0xF6D04A, -1.0), (0x17120F, 0.4));
        assert_eq!(peek_fill(0x17120F, 0.4, 0xF6D04A, 2.0), (0xF6D04A, 1.0));
    }

    /// A new target mid-morph (the style or the mascot changed while the
    /// island morphs): no jump at that moment, and the target by the end.
    #[test]
    fn a_new_target_mid_morph_carries_on_without_a_jump() {
        for (from, to, next, t) in [
            (0.0, 1.0, 0.0, 0.5),
            (1.0, 1.0, 0.0, 0.3),
            (1.0, 0.0, 1.0, 0.8),
            (0.2, 0.6, 1.0, 0.0),
        ] {
            let mut mix = PeekMix { from, to };
            let now = mix.at(t);
            mix.retarget_at(next, t);
            assert!((mix.at(t) - now).abs() < 1e-5, "{from} {to} {next} {t}");
            assert!((mix.at(1.0) - next).abs() < 1e-5);
            // Between now and the end it stays between the two.
            let later = mix.at((t + 1.0) / 2.0);
            let (lo, hi) = (now.min(next), now.max(next));
            assert!(later >= lo - 1e-5 && later <= hi + 1e-5);
        }
        // At the morph's end: the target, both ends.
        let mut mix = PeekMix { from: 0.0, to: 1.0 };
        mix.retarget_at(0.0, 1.0);
        assert_eq!(mix, PeekMix::at_rest(0.0));
    }

    #[test]
    fn the_face_mix_follows_the_morph_and_carries_on_when_cut() {
        let mut mix = PeekMix::at_rest(0.0);
        mix.retarget(1.0, 0.0);
        assert_eq!(mix.at(0.0), 0.0);
        let mid = mix.at(0.5);
        assert!(mid > 0.0 && mid < 1.0);
        assert_eq!(mix.at(1.0), 1.0);
        // Restored half way: the new morph starts where this one was.
        mix.retarget(0.0, mid);
        assert!((mix.at(0.0) - mid).abs() < 1e-6);
        assert_eq!(mix.at(1.0), 0.0);
        // No motion: both ends are the target at once.
        mix.retarget(1.0, 0.0);
        mix.settle();
        assert_eq!(mix.at(0.0), 1.0);
    }
    const SURFACE: u32 = 0x151C2E;

    fn look(mascot: Mascot) -> CatLook {
        CatLook { mascot, ..CatLook::default() }
    }

    #[test]
    fn every_coat_has_the_approved_face() {
        let cat = |color| peek_face(CatLook { color, ..look(Mascot::Cat) }, SURFACE);
        let eq = |f: &PeekFace, fill, eye, edge| {
            assert_eq!((f.fill, f.eye, f.edge), (fill, eye, edge));
        };
        let f = cat(CatColor::Terracotta);
        eq(&f, 0xE2683C, EYES_DARK, None);
        assert!(f.face_marks.is_empty() && f.coat_marks.is_empty());

        let f = cat(CatColor::Calico);
        eq(&f, 0xF4EDE4, EYES_DARK, Some(SURFACE));
        assert_eq!(
            f.coat_marks,
            vec![dot(17.625, 35.0, 4.25, PATCH_ORANGE), dot(47.625, 45.625, 3.75, PATCH_DARK)]
        );
        assert!(f.face_marks.is_empty());

        let f = cat(CatColor::White);
        eq(&f, 0xF6F1EA, EYES_DARK, Some(SURFACE));
        assert!(f.face_marks.is_empty() && f.coat_marks.is_empty());

        let f = cat(CatColor::Black);
        eq(&f, 0x1E1B22, AMBER, Some(RIM));
        assert!(f.face_marks.is_empty() && f.coat_marks.is_empty());

        for falk in [FalkColor::Lutino, FalkColor::Grey] {
            let f = peek_face(CatLook { falk, ..look(Mascot::Falk) }, SURFACE);
            eq(&f, falk_ink(falk).face, EYES_DARK, None);
            assert_eq!(
                f.face_marks,
                vec![
                    dot(18.75, 44.0, 2.375, 0xEC6B3A),
                    dot(45.25, 44.0, 2.375, 0xEC6B3A),
                    PeekMark::Beak { rgb: 0xD9978F },
                ]
            );
            assert!(f.coat_marks.is_empty());
        }
        let f = peek_face(CatLook { falk: FalkColor::WhiteFace, ..look(Mascot::Falk) }, SURFACE);
        eq(&f, 0xF4EFE6, EYES_DARK, Some(SURFACE));
        assert_eq!(f.face_marks, vec![PeekMark::Beak { rgb: 0xD9978F }]);

        for &octopus in OctopusColor::ALL {
            let f = peek_face(CatLook { octopus, ..look(Mascot::Octopus) }, SURFACE);
            let fill = octopus_rgb(octopus);
            eq(&f, fill, EYES_DARK, None);
            let spot = shade(fill);
            assert_eq!(
                f.coat_marks,
                vec![dot(44.25, 36.5, 1.625, spot), dot(19.0, 35.75, 1.125, spot)]
            );
            assert!(f.face_marks.is_empty());
        }
    }

    #[test]
    fn the_accessory_does_not_change_the_face() {
        for mascot in [Mascot::Cat, Mascot::Falk, Mascot::Octopus] {
            let base = peek_face(look(mascot), SURFACE);
            for &accessory in CatAccessory::ALL {
                let l = CatLook { accessory, ..look(mascot) };
                assert_eq!(peek_face(l, SURFACE), base, "{mascot:?} {accessory:?}");
            }
        }
    }

    #[test]
    fn the_beak_box_is_the_spec_path() {
        let boxes = mark_boxes(&[PeekMark::Beak { rgb: 0 }]);
        let (l, t, r, b) = boxes[0];
        for (got, want) in [(l, 30.375), (t, 42.0), (r, 33.625), (b, 46.375)] {
            assert!((got - want).abs() < 0.5, "{got} {want}");
        }
    }

    /// 64 x 64 px on black, one px per grid unit, by Direct2D's software
    /// rasterizer into a WIC bitmap (the pattern of the `mascot` tests): no
    /// window. `paint` draws into the target; each pixel is `0xRRGGBB`.
    fn render(paint: impl FnOnce(&RobotPainter, &ID2D1RenderTarget, &ID2D1SolidColorBrush)) -> Vec<u32> {
        use windows::Win32::Graphics::Direct2D::Common::{
            D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
        };
        use windows::Win32::Graphics::Direct2D::{
            D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
            D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1CreateFactory, ID2D1Factory,
        };
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
        use windows::Win32::Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
            WICBitmapCacheOnLoad,
        };
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        };
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).expect("factory");
            let painter = RobotPainter::new(&factory).expect("painter");
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .expect("wic");
            let bitmap = wic
                .CreateBitmap(64, 64, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)
                .expect("bitmap");
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let rt = factory.CreateWicBitmapRenderTarget(&bitmap, &props).expect("target");
            let brush = rt.CreateSolidColorBrush(&solid(0, 1.0), None).expect("brush");
            rt.BeginDraw();
            rt.Clear(Some(&solid(0x000000, 1.0)));
            paint(&painter, &rt, &brush);
            rt.EndDraw(None, None).expect("end draw");
            let mut out = vec![0u8; 64 * 64 * 4];
            bitmap.CopyPixels(std::ptr::null(), 64 * 4, &mut out).expect("pixels");
            out.chunks(4)
                .map(|p| (p[2] as u32) << 16 | (p[1] as u32) << 8 | p[0] as u32)
                .collect()
        }
    }

    fn px(img: &[u32], x: u32, y: u32) -> u32 {
        img[(y * 64 + x) as usize]
    }

    #[track_caller]
    fn assert_near(got: u32, want: u32) {
        let off = [16, 8, 0]
            .iter()
            .map(|s| (((got >> s) & 0xFF) as i32 - ((want >> s) & 0xFF) as i32).abs())
            .max()
            .unwrap();
        assert!(off <= 6, "got #{got:06X}, want #{want:06X}");
    }

    /// The eyes and face marks of `look` over the whole grid (k = 1).
    fn eyes_of(look: CatLook, blink: bool) -> Vec<u32> {
        let face = peek_face(look, SURFACE);
        render(|painter, rt, brush| {
            let area = Rect { x: 0.0, y: 0.0, w: 64.0, h: 64.0 };
            painter.paint_eyes(rt, brush, area, 1.0, blink, &face);
        })
    }

    fn falk(falk: FalkColor) -> CatLook {
        CatLook { falk, ..look(Mascot::Falk) }
    }

    #[test]
    fn the_shrunk_face_paints_each_mascot() {
        let cat = |color| CatLook { color, ..look(Mascot::Cat) };
        assert_near(px(&eyes_of(cat(CatColor::Terracotta), false), 24, 40), EYES_DARK);
        assert_near(px(&eyes_of(cat(CatColor::Black), false), 24, 40), AMBER);

        let lutino = eyes_of(falk(FalkColor::Lutino), false);
        assert_near(px(&lutino, 24, 40), EYES_DARK);
        assert_near(px(&lutino, 18, 44), 0xEC6B3A); // a cheek
        assert_near(px(&lutino, 32, 44), 0xD9978F); // the beak
        // Muka putih has no cheeks.
        assert_near(px(&eyes_of(falk(FalkColor::WhiteFace), false), 18, 44), 0x000000);

        // The blink is the two level lines in the eye colour, the eyes gone.
        let blink = eyes_of(falk(FalkColor::Lutino), true);
        assert_near(px(&blink, 24, 40), EYES_DARK);
        assert_near(px(&blink, 24, 36), 0x000000);
    }

    #[test]
    fn the_coat_marks_paint_in_place() {
        let calico = CatLook { color: CatColor::Calico, ..look(Mascot::Cat) };
        let face = peek_face(calico, SURFACE);
        let img = render(|painter, rt, brush| {
            let area = Rect { x: 0.0, y: 0.0, w: 64.0, h: 64.0 };
            painter.paint_peek_marks(rt, brush, area, 1.0, &face.coat_marks);
        });
        assert_near(px(&img, 17, 35), PATCH_ORANGE);
        assert_near(px(&img, 47, 45), PATCH_DARK);
    }
}
