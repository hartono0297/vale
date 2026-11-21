//! Turning a screenshot and its annotations by 90 degrees.

use super::{Annotation, FRect, Pt, text_extent};
use crate::capture::Shot;

/// Which way to turn the picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    Left,
    Right,
}

/// `p` after the turn; `size` is the picture's (w, h) before it.
pub fn rotate_pt(p: Pt, turn: Turn, size: (f32, f32)) -> Pt {
    let (w, h) = size;
    match turn {
        Turn::Right => Pt::new(h - p.y, p.x),
        Turn::Left => Pt::new(p.y, w - p.x),
    }
}

/// `r` after the turn, both corners turned.
pub fn rotate_rect(r: FRect, turn: Turn, size: (f32, f32)) -> FRect {
    let a = rotate_pt(Pt::new(r.x, r.y), turn, size);
    let b = rotate_pt(Pt::new(r.x + r.w, r.y + r.h), turn, size);
    FRect::between(a, b)
}

/// The pixels turned a quarter; the new size is (h, w).
pub fn rotate_shot(shot: &Shot, turn: Turn) -> Shot {
    let (w, h) = (shot.w as usize, shot.h as usize);
    let mut bgra = vec![0u8; shot.bgra.len()];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = match turn {
                Turn::Right => (h - 1 - y, x),
                Turn::Left => (y, w - 1 - x),
            };
            let s = (y * w + x) * 4;
            let d = (dy * h + dx) * 4;
            bgra[d..d + 4].copy_from_slice(&shot.bgra[s..s + 4]);
        }
    }
    Shot {
        w: shot.h,
        h: shot.w,
        bgra,
    }
}

/// `turns % 4` right turns (0 is a copy). One or three turns cost one pass
/// (three is a single left turn), two cost two; no extra copy is made.
pub fn turned(shot: &Shot, turns: u8) -> Shot {
    match turns % 4 {
        0 => shot.clone(),
        1 => rotate_shot(shot, Turn::Right),
        2 => rotate_shot(&rotate_shot(shot, Turn::Right), Turn::Right),
        _ => rotate_shot(shot, Turn::Left),
    }
}

impl Annotation {
    /// Turns it with the picture; `size` is the picture's (w, h) before the
    /// turn. Size, colour and style stay as they are.
    pub fn rotate(&mut self, turn: Turn, size: (f32, f32)) {
        match self {
            Annotation::Rect { r, .. } | Annotation::Pixelate { r } => {
                *r = rotate_rect(*r, turn, size);
            }
            Annotation::Arrow { a, b, .. } | Annotation::Line { a, b, .. } => {
                *a = rotate_pt(*a, turn, size);
                *b = rotate_pt(*b, turn, size);
            }
            Annotation::Pen { pts, .. } | Annotation::Marker { pts, .. } => {
                for p in pts {
                    *p = rotate_pt(*p, turn, size);
                }
            }
            Annotation::Text {
                pos,
                text,
                size: font,
                ..
            } => {
                let (tw, th) = text_extent(text, *font);
                let centre = Pt::new(pos.x + tw / 2.0, pos.y + th / 2.0);
                let c = rotate_pt(centre, turn, size);
                *pos = Pt::new(c.x - tw / 2.0, c.y - th / 2.0);
            }
        }
    }
}

/// Turns every annotation of `doc`.
pub fn rotate_doc(doc: &mut [Annotation], turn: Turn, size: (f32, f32)) {
    for a in doc {
        a.rotate(turn, size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shot_edit::Style;

    fn near(a: Pt, b: Pt) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    fn near_rect(a: FRect, b: FRect) -> bool {
        near(Pt::new(a.x, a.y), Pt::new(b.x, b.y)) && near(Pt::new(a.w, a.h), Pt::new(b.w, b.h))
    }

    fn sample() -> Shot {
        Shot {
            w: 3,
            h: 2,
            bgra: (0..6u8).flat_map(|i| [i, 0, 0, 255]).collect(),
        }
    }

    #[test]
    fn a_point_turns_right_and_left() {
        let size = (100.0, 40.0);
        assert_eq!(
            rotate_pt(Pt::new(10.0, 5.0), Turn::Right, size),
            Pt::new(35.0, 10.0)
        );
        assert_eq!(
            rotate_pt(Pt::new(10.0, 5.0), Turn::Left, size),
            Pt::new(5.0, 90.0)
        );
    }

    #[test]
    fn left_undoes_right() {
        for p in [Pt::new(0.0, 0.0), Pt::new(100.0, 40.0), Pt::new(12.5, 7.25)] {
            let there = rotate_pt(p, Turn::Right, (100.0, 40.0));
            assert_eq!(rotate_pt(there, Turn::Left, (40.0, 100.0)), p);
        }
    }

    #[test]
    fn four_right_turns_give_the_point_back() {
        let p = Pt::new(12.3, 7.7);
        let mut size = (1080.0, 1920.0);
        let mut q = p;
        for _ in 0..4 {
            q = rotate_pt(q, Turn::Right, size);
            size = (size.1, size.0);
        }
        assert!(near(p, q), "{q:?}");
    }

    #[test]
    fn a_rect_stays_a_positive_rect() {
        let r = rotate_rect(
            FRect::new(10.0, 5.0, 20.0, 10.0),
            Turn::Right,
            (100.0, 40.0),
        );
        assert!(near_rect(r, FRect::new(25.0, 10.0, 10.0, 20.0)), "{r:?}");
    }

    #[test]
    fn the_pixels_turn_right() {
        let s = sample();
        let t = rotate_shot(&s, Turn::Right);
        assert_eq!((t.w, t.h), (2, 3));
        for y in 0..2usize {
            for x in 0..3usize {
                let (dx, dy) = (2 - 1 - y, x);
                assert_eq!(t.bgra[(dy * 2 + dx) * 4], s.bgra[(y * 3 + x) * 4]);
            }
        }
    }

    #[test]
    fn the_pixels_turn_left() {
        let s = sample();
        let t = rotate_shot(&s, Turn::Left);
        assert_eq!((t.w, t.h), (2, 3));
        for y in 0..2usize {
            for x in 0..3usize {
                let (dx, dy) = (y, 3 - 1 - x);
                assert_eq!(t.bgra[(dy * 2 + dx) * 4], s.bgra[(y * 3 + x) * 4]);
            }
        }
    }

    #[test]
    fn four_turns_give_the_same_pixels() {
        let s = sample();
        assert_eq!(turned(&s, 4).bgra, s.bgra);
        assert_eq!(turned(&rotate_shot(&s, Turn::Right), 3), s);
        // Three right turns are one left turn, one pass.
        assert_eq!(turned(&s, 3), rotate_shot(&s, Turn::Left));
        assert_eq!(turned(&s, 1), rotate_shot(&s, Turn::Right));
        assert_eq!(
            turned(&s, 2),
            rotate_shot(&rotate_shot(&s, Turn::Right), Turn::Right)
        );
        assert_eq!(turned(&s, 7), turned(&s, 3));
    }

    #[test]
    fn text_keeps_its_size_and_turns_its_centre() {
        let mut t = Annotation::Text {
            pos: Pt::new(10.0, 10.0),
            text: "ab".into(),
            size: 22.0,
            color: 0xE5484D,
        };
        let (tw, th) = text_extent("ab", 22.0);
        let old_c = Pt::new(10.0 + tw / 2.0, 10.0 + th / 2.0);
        t.rotate(Turn::Right, (200.0, 100.0));
        let Annotation::Text {
            pos,
            text,
            size,
            color,
        } = t
        else {
            panic!("still text");
        };
        assert_eq!((text.as_str(), size, color), ("ab", 22.0, 0xE5484D));
        let new_c = Pt::new(pos.x + tw / 2.0, pos.y + th / 2.0);
        assert!(near(new_c, rotate_pt(old_c, Turn::Right, (200.0, 100.0))));
    }

    #[test]
    fn text_right_then_left_is_where_it_was() {
        let start = Pt::new(10.0, 14.0);
        let mut t = Annotation::Text {
            pos: start,
            text: "ab".into(),
            size: 22.0,
            color: 0xE5484D,
        };
        t.rotate(Turn::Right, (200.0, 100.0));
        t.rotate(Turn::Left, (100.0, 200.0));
        let Annotation::Text { pos, .. } = t else {
            panic!("still text");
        };
        assert!(near(pos, start), "{pos:?}");
    }

    #[test]
    fn every_kind_turns() {
        let style = Style {
            color: 0xE5484D,
            width: 3.0,
        };
        let p = Pt::new;
        let r = FRect::new(10.0, 5.0, 20.0, 10.0);
        let docs = vec![
            Annotation::Rect { r, style },
            Annotation::Arrow {
                a: p(1.0, 2.0),
                b: p(30.0, 20.0),
                style,
            },
            Annotation::Line {
                a: p(4.0, 9.0),
                b: p(50.0, 33.0),
                style,
            },
            Annotation::Pen {
                pts: vec![p(1.0, 1.0), p(5.5, 8.25), p(70.0, 30.0)],
                style,
            },
            Annotation::Marker {
                pts: vec![p(3.0, 3.0), p(60.0, 12.0)],
                style,
            },
            Annotation::Pixelate { r },
        ];
        for orig in docs {
            let mut a = orig.clone();
            a.rotate(Turn::Right, (100.0, 40.0));
            assert_ne!(a, orig);
            a.rotate(Turn::Left, (40.0, 100.0));
            let same = match (&a, &orig) {
                (Annotation::Rect { r: x, .. }, Annotation::Rect { r: y, .. })
                | (Annotation::Pixelate { r: x }, Annotation::Pixelate { r: y }) => {
                    near_rect(*x, *y)
                }
                (
                    Annotation::Arrow { a: a1, b: b1, .. },
                    Annotation::Arrow { a: a2, b: b2, .. },
                )
                | (Annotation::Line { a: a1, b: b1, .. }, Annotation::Line { a: a2, b: b2, .. }) => {
                    near(*a1, *a2) && near(*b1, *b2)
                }
                (Annotation::Pen { pts: x, .. }, Annotation::Pen { pts: y, .. })
                | (Annotation::Marker { pts: x, .. }, Annotation::Marker { pts: y, .. }) => {
                    x.len() == y.len() && x.iter().zip(y).all(|(a, b)| near(*a, *b))
                }
                _ => false,
            };
            assert!(same, "{a:?} vs {orig:?}");
        }
    }
}
