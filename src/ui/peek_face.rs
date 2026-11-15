//! The mini face the shrunk notch shows: the chosen mascot's face colour, dark
//! eyes and the few small marks that tell the mascots apart. Pure data in the
//! eyes' 64-unit grid (the one `robot::paint_eyes` draws in); nothing here
//! paints or owns a timer. The accessory never changes it.

use super::cat_style::{AMBER, CatLook, EYES_DARK, PATCH_DARK, PATCH_ORANGE, RIM};
use super::mascot::{FALK_BEAK, falk_ink, octopus_rgb, shade};
use super::robot::Seg;
use crate::theme::{CatColor, FalkColor, Mascot};

const CALICO_CREAM: u32 = 0xF4EDE4;
const WHITE_CAT: u32 = 0xF6F1EA;
const BLACK_CAT: u32 = 0x1E1B22;
const TERRACOTTA: u32 = 0xE2683C;

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
        Mascot::Cat => match look.color {
            CatColor::Terracotta => plain(TERRACOTTA, EYES_DARK, None),
            CatColor::Calico => PeekFace {
                coat_marks: vec![
                    dot(17.625, 35.0, 4.25, PATCH_ORANGE),
                    dot(47.625, 45.625, 3.75, PATCH_DARK),
                ],
                ..plain(CALICO_CREAM, EYES_DARK, Some(surface))
            },
            CatColor::White => plain(WHITE_CAT, EYES_DARK, Some(surface)),
            CatColor::Black => plain(BLACK_CAT, AMBER, Some(RIM)),
        },
        Mascot::Falk => {
            let ink = falk_ink(look.falk);
            let edge = (look.falk == FalkColor::WhiteFace).then_some(surface);
            let mut face_marks = Vec::new();
            if let Some(cheek) = ink.cheek {
                face_marks.push(dot(18.75, 44.0, 2.375, cheek));
                face_marks.push(dot(45.25, 44.0, 2.375, cheek));
            }
            face_marks.push(PeekMark::Beak { rgb: FALK_BEAK });
            PeekFace { face_marks, ..plain(ink.face, EYES_DARK, edge) }
        }
        Mascot::Octopus => {
            let fill = octopus_rgb(look.octopus);
            let spot = shade(fill);
            PeekFace {
                coat_marks: vec![dot(44.25, 36.5, 1.625, spot), dot(19.0, 35.75, 1.125, spot)],
                ..plain(fill, EYES_DARK, None)
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
}
