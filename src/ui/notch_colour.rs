//! Every colour decision of "Warna notch", as pure functions: the tint of a
//! coat, the gradient's bottom colour, the face fill and the ink palettes that
//! keep text (4.5:1) and marks (3:1) readable on them. The renderer only
//! paints what these return; nothing here owns a timer or draws.

use super::cat_style::CatLook;
use super::mascot::{mix, octopus_rgb};
use super::geometry::ShapeKind;
use super::peek_face::peek_face;
use crate::config::NotchStyle;
use crate::model::View;
use crate::theme::{CatColor, FalkColor, Mascot, Palette};

/// Where the gradient leaves the surface colour (0 and this stop are both the
/// surface, so the top stays near-black like the bezel).
pub const TOP_STOP: f32 = 0.28;
/// The largest blend toward the tint on closed views and notices.
pub const CLOSED_T: f32 = 0.45;
/// The largest blend toward the tint on the open panel.
pub const PANEL_T: f32 = 0.32;
/// The island's cream text.
pub const CREAM: u32 = 0xF4EDE4;
/// The dark ink used on light fills.
pub const INK: u32 = 0x17120F;

/// WCAG 2.x contrast ratio between two colours (1.0 to 21.0).
pub fn contrast(a: u32, b: u32) -> f32 {
    let lum = |c: u32| {
        let ch = |sh: u32| {
            let v = ((c >> sh) & 0xFF) as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
    };
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The style actually drawn: Face needs the mascot, so with it off the notch
/// draws Gradient (the stored value is untouched).
pub fn effective_style(style: NotchStyle, robot_on: bool) -> NotchStyle {
    match style {
        NotchStyle::Face if !robot_on => NotchStyle::Gradient,
        s => s,
    }
}

/// The coat's notch colour ("Warna dari"); the accent with the mascot off.
pub fn tint(look: CatLook, robot_on: bool, accent: u32) -> u32 {
    if !robot_on {
        return accent;
    }
    match look.mascot {
        Mascot::Cat => match look.color {
            CatColor::Terracotta | CatColor::Calico => 0xE2683C,
            CatColor::White => 0xF6F1EA,
            CatColor::Black => 0x8E7CB8,
        },
        Mascot::Falk => match look.falk {
            FalkColor::Lutino => 0xF6D04A,
            FalkColor::Grey => 0xF2D35A,
            FalkColor::WhiteFace => 0x9A959C,
        },
        Mascot::Octopus => octopus_rgb(look.octopus),
    }
}

/// The gradient's bottom colour: `surface` blended toward `tint` by the
/// largest `t` (steps of 0.01) up to `max_t` that keeps cream text at 5:1,
/// the caption colour at 4.5:1 and, when given, the accent at 3:1. `t = 0`
/// (the surface itself) is the floor.
pub fn bottom(surface: u32, tint: u32, max_t: f32, accent: Option<u32>) -> u32 {
    let top = (max_t * 100.0).round() as i32;
    for n in (1..=top).rev() {
        let b = mix(surface, tint, n as f32 / 100.0);
        let ok = contrast(CREAM, b) >= 5.0
            && contrast(mix(CREAM, b, 0.15), b) >= 4.5
            && accent.is_none_or(|a| contrast(a, b) >= 3.0);
        if ok {
            return b;
        }
    }
    surface
}

/// The face colour Jadi muka fills the notch with: what the shrunk pill uses.
pub fn face_fill(look: CatLook, surface: u32) -> u32 {
    peek_face(look, surface).fill
}

/// `base` on a face-coloured notch: the face is the surface, text and icons
/// take the dark ink where it holds 4.5:1 (else cream), the caption colour is
/// held at 4.5:1 and colours under 3:1 fall back to the text colour.
pub fn face_palette(base: Palette, fill: u32) -> Palette {
    let text = if contrast(INK, fill) >= 4.5 { INK } else { CREAM };
    let secondary = (0..=30)
        .rev()
        .map(|n| mix(text, fill, n as f32 / 100.0))
        .find(|c| contrast(*c, fill) >= 4.5)
        .unwrap_or(text);
    let keep = |c: u32| if contrast(c, fill) >= 3.0 { c } else { text };
    Palette {
        surface: fill,
        text,
        on_accent: text,
        secondary,
        accent: keep(base.accent),
        sage: keep(base.sage),
        ochre: keep(base.ochre),
        plum: keep(base.plum),
        ..base
    }
}

/// `base` on a gradient's bottom colour: the accent becomes the text colour
/// where it would be under 3:1 against `bottom`.
pub fn gradient_palette(base: Palette, bottom: u32) -> Palette {
    Palette {
        accent: if contrast(base.accent, bottom) >= 3.0 {
            base.accent
        } else {
            base.text
        },
        ..base
    }
}

/// The shape kind a closed compact view is laid out as on the face notch
/// (the caption view and the screen countdown take the REC view's), `None`
/// for the views Jadi muka leaves to Gradasi (notices, the panel, hidden).
pub fn face_kind(view: &View) -> Option<ShapeKind> {
    match view {
        View::Idle => Some(ShapeKind::Idle),
        View::MediaCompact => Some(ShapeKind::MediaCompact),
        View::MediaLyric => Some(ShapeKind::MediaLyric),
        View::PomodoroCompact => Some(ShapeKind::PomodoroCompact),
        View::RecordCompact | View::CaptionCompact | View::ScreenCountdown(_) => {
            Some(ShapeKind::RecordCompact)
        }
        View::Hidden | View::Panel(_) | View::Notice(_) => None,
    }
}

/// Whether `view` is drawn as the face under `style` (the effective one):
/// Jadi muka on a closed compact view.
pub fn shows_face(style: NotchStyle, view: &View) -> bool {
    style == NotchStyle::Face && face_kind(view).is_some()
}

/// The closed content's palette while the fill turns between the face
/// (`share` 1) and Gradasi with the bottom colour `closed` (0): the face's
/// ink (`face_palette`) until cream holds 4.5:1 on the blended bottom
/// (`mix(closed, face, share)`), Gradasi's (`gradient_palette`) from there.
pub fn crossfade_palette(base: Palette, face: u32, closed: u32, share: f32) -> Palette {
    if share >= 1.0 {
        return face_palette(base, face);
    }
    if share <= 0.0 || contrast(CREAM, mix(closed, face, share)) >= 4.5 {
        gradient_palette(base, closed)
    } else {
        face_palette(base, face)
    }
}

/// A non-text mark (a REC level meter) on `fill`: as it is where it holds
/// 3:1, else in `ink` (the face's text colour).
pub fn readable_mark(mark: u32, fill: u32, ink: u32) -> u32 {
    if contrast(mark, fill) >= 3.0 { mark } else { ink }
}

/// Whether a mark (the REC dot) needs a ring in the text colour on `fill`.
pub fn needs_ring(mark: u32, fill: u32) -> bool {
    contrast(mark, fill) < 3.0
}

/// Whether the head needs the dark halo behind it: under 3:1 against `fill`
/// and no rim of its own.
pub fn needs_halo(head: u32, fill: u32, has_rim: bool) -> bool {
    !has_rim && contrast(head, fill) < 3.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Accent, Coat, OctopusColor, Surface, island};

    fn looks() -> Vec<CatLook> {
        let mut v = Vec::new();
        for m in Mascot::ALL {
            for c in Coat::all_of(*m) {
                let mut l = CatLook {
                    mascot: *m,
                    ..CatLook::default()
                };
                match c {
                    Coat::Cat(x) => l.color = x,
                    Coat::Falk(x) => l.falk = x,
                    Coat::Octopus(x) => l.octopus = x,
                }
                v.push(l);
            }
        }
        v
    }

    fn surfaces() -> Vec<Surface> {
        Surface::ALL
            .iter()
            .copied()
            .filter(|s| *s != Surface::Auto)
            .collect()
    }

    fn cat(color: CatColor) -> CatLook {
        CatLook {
            mascot: Mascot::Cat,
            color,
            ..CatLook::default()
        }
    }

    #[test]
    fn there_are_eleven_coats_and_six_surfaces() {
        assert_eq!(looks().len(), 11);
        assert_eq!(surfaces().len(), 6);
    }

    #[test]
    fn the_tint_table_matches_the_spec() {
        let a = 0xD9A441;
        let on = |l: CatLook| tint(l, true, a);
        assert_eq!(on(cat(CatColor::Terracotta)), 0xE2683C);
        assert_eq!(on(cat(CatColor::Calico)), 0xE2683C);
        assert_eq!(on(cat(CatColor::White)), 0xF6F1EA);
        assert_eq!(on(cat(CatColor::Black)), 0x8E7CB8);
        let falk = |f| CatLook {
            mascot: Mascot::Falk,
            falk: f,
            ..CatLook::default()
        };
        assert_eq!(on(falk(FalkColor::Lutino)), 0xF6D04A);
        assert_eq!(on(falk(FalkColor::Grey)), 0xF2D35A); // Kuning
        assert_eq!(on(falk(FalkColor::WhiteFace)), 0x9A959C);
        for o in OctopusColor::ALL {
            let l = CatLook {
                mascot: Mascot::Octopus,
                octopus: *o,
                ..CatLook::default()
            };
            assert_eq!(on(l), octopus_rgb(*o));
        }
        for l in looks() {
            assert_eq!(tint(l, false, a), a);
        }
    }

    #[test]
    fn every_gradient_bottom_keeps_cream_and_caption_readable() {
        for l in looks() {
            for s in surfaces() {
                for t in [CLOSED_T, PANEL_T] {
                    let b = bottom(s.rgb(), tint(l, true, 0xD9A441), t, None);
                    assert!(contrast(CREAM, b) >= 5.0, "{l:?} {s:?} {t}");
                    assert!(
                        contrast(mix(CREAM, b, 0.15), b) >= 4.5,
                        "{l:?} {s:?} {t}"
                    );
                }
            }
        }
    }

    #[test]
    fn notice_and_panel_bottoms_keep_every_accent_at_3() {
        for l in looks() {
            for s in surfaces() {
                for a in Accent::ALL {
                    for t in [CLOSED_T, PANEL_T] {
                        let b = bottom(s.rgb(), tint(l, true, a.rgb()), t, Some(a.rgb()));
                        assert!(contrast(a.rgb(), b) >= 3.0, "{l:?} {s:?} {a:?} {t}");
                    }
                }
            }
        }
    }

    #[test]
    fn pale_coats_blend_less() {
        let s = Surface::Espresso.rgb();
        let at = |c| bottom(s, tint(cat(c), true, 0), CLOSED_T, None);
        let (white, terra) = (at(CatColor::White), at(CatColor::Terracotta));
        // The blend `t` actually used, read off the red channel (the surface
        // and every tint differ there). Luminance would not do: the white
        // coat's bottom is a lighter grey than terracotta's darker orange
        // even though it blends less.
        let used = |b: u32, tint: u32| {
            let r = |c: u32| ((c >> 16) & 0xFF) as f32;
            (r(b) - r(s)) / (r(tint) - r(s))
        };
        let (tw, tt) = (used(white, 0xF6F1EA), used(terra, 0xE2683C));
        assert!(tw < tt, "t {tw} vs {tt} ({white:06X} vs {terra:06X})");
    }

    #[test]
    fn t_zero_is_the_surface_and_a_cap_blends() {
        let s = Surface::Night.rgb();
        assert_eq!(bottom(s, 0xE2683C, 0.0, None), s);
        assert_ne!(bottom(s, 0xE2683C, CLOSED_T, None), s);
    }

    #[test]
    fn face_ink_is_readable_on_every_face() {
        let base = island(Accent::Terracotta, Surface::Espresso);
        for l in looks() {
            let fill = face_fill(l, Surface::Espresso.rgb());
            let p = face_palette(base, fill);
            assert_eq!(p.surface, fill);
            assert!(contrast(p.text, fill) >= 4.5, "{l:?} text");
            assert!(contrast(p.secondary, fill) >= 4.5, "{l:?} secondary");
            for (n, c) in [
                ("accent", p.accent),
                ("sage", p.sage),
                ("ochre", p.ochre),
                ("plum", p.plum),
            ] {
                assert!(contrast(c, fill) >= 3.0 || c == p.text, "{l:?} {n}");
            }
        }
        let black = face_fill(cat(CatColor::Black), Surface::Espresso.rgb());
        assert_eq!(face_palette(base, black).text, CREAM);
        let red = CatLook {
            mascot: Mascot::Octopus,
            octopus: OctopusColor::Red,
            ..CatLook::default()
        };
        let fill = face_fill(red, Surface::Espresso.rgb());
        assert_eq!(fill, 0xD9505F);
        assert_eq!(face_palette(base, fill).text, INK);
    }

    #[test]
    fn gradient_palette_swaps_only_a_weak_accent() {
        let base = island(Accent::Terracotta, Surface::Espresso);
        let weak = gradient_palette(base, base.accent);
        assert_eq!(weak.accent, base.text);
        assert_eq!(
            Palette {
                accent: base.accent,
                ..weak
            },
            base
        );
        assert_eq!(gradient_palette(base, base.surface), base);
    }

    #[test]
    fn ring_and_halo_rules() {
        assert!(needs_ring(0xE2683C, 0xE2683C));
        assert!(!needs_ring(0xE2683C, 0x17120F));
        assert!(needs_halo(0xE2683C, 0xE2683C, false));
        assert!(!needs_halo(0xE2683C, 0xE2683C, true));
        assert!(!needs_halo(0xE2683C, 0x17120F, false));
    }

    #[test]
    fn only_the_closed_compact_views_show_the_face() {
        use crate::model::{Notice, NoticeKind, Page, View};
        use crate::ui::geometry::ShapeKind;
        let faces = [
            (View::Idle, ShapeKind::Idle),
            (View::MediaCompact, ShapeKind::MediaCompact),
            (View::MediaLyric, ShapeKind::MediaLyric),
            (View::PomodoroCompact, ShapeKind::PomodoroCompact),
            (View::RecordCompact, ShapeKind::RecordCompact),
            (View::CaptionCompact, ShapeKind::RecordCompact),
            (View::ScreenCountdown(3), ShapeKind::RecordCompact),
        ];
        for (view, kind) in &faces {
            assert!(shows_face(NotchStyle::Face, view), "{view:?}");
            assert_eq!(face_kind(view), Some(*kind), "{view:?}");
            for style in [NotchStyle::Plain, NotchStyle::Gradient] {
                assert!(!shows_face(style, view), "{style:?} {view:?}");
            }
        }
        let notice = View::Notice(Notice {
            key: "n".into(),
            source: "Vale".into(),
            title: "T".into(),
            body: "B".into(),
            kind: NoticeKind::ClaudeWaiting { session_id: "s".into() },
        });
        for view in [notice, View::Panel(Page::ALL[0]), View::Hidden] {
            assert!(!shows_face(NotchStyle::Face, &view), "{view:?}");
            assert_eq!(face_kind(&view), None, "{view:?}");
        }
    }

    #[test]
    fn the_ink_turns_cream_once_cream_reads_on_the_blended_fill() {
        let base = island(Accent::Terracotta, Surface::Espresso);
        let lutino = CatLook {
            mascot: Mascot::Falk,
            falk: FalkColor::Lutino,
            ..CatLook::default()
        };
        let face = face_fill(lutino, base.surface);
        let closed = bottom(base.surface, tint(lutino, true, base.accent), CLOSED_T, None);
        // All face: the face's dark ink; all Gradasi: Gradasi's cream.
        assert_eq!(crossfade_palette(base, face, closed, 1.0), face_palette(base, face));
        assert_eq!(crossfade_palette(base, face, closed, 0.0), gradient_palette(base, closed));
        // In between, cream exactly where it holds 4.5:1 on the blend.
        let mut seen = (false, false);
        for n in 1..100 {
            let share = n as f32 / 100.0;
            let p = crossfade_palette(base, face, closed, share);
            let reads = contrast(CREAM, mix(closed, face, share)) >= 4.5;
            assert_eq!(p.text == CREAM, reads, "{share}");
            if reads {
                seen.0 = true;
            } else {
                seen.1 = true;
                assert_eq!(p, face_palette(base, face), "{share}");
            }
        }
        assert!(seen.0 && seen.1, "the switch happens inside the morph");
        // A dark face keeps the face's palette all the way (its ink is cream).
        let black = face_fill(cat(CatColor::Black), base.surface);
        assert_eq!(crossfade_palette(base, black, closed, 1.0).text, CREAM);
    }

    #[test]
    fn face_with_the_mascot_off_draws_gradient() {
        assert_eq!(
            effective_style(NotchStyle::Face, false),
            NotchStyle::Gradient
        );
        assert_eq!(effective_style(NotchStyle::Face, true), NotchStyle::Face);
        for s in [NotchStyle::Plain, NotchStyle::Gradient] {
            assert_eq!(effective_style(s, false), s);
            assert_eq!(effective_style(s, true), s);
        }
    }
}
