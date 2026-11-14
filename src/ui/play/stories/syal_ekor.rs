//! The cat's "Syal ekor" (the tail scarf): the cat hops out of the dark onto
//! the island's lip, its tail creeps under the lip and round the left end and
//! hugs it like a scarf (the end pulled in), the furry tip curls back and
//! tickles its own ear, a sneeze jolts the island, the tail lets go and the
//! cat acts as if nothing happened, then hops back into the dark. Timeline,
//! easing and offsets from `docs/design/maskot-notch-v3.html`, story `c4`.

use std::f32::consts::TAU;

use super::{CROUCH, Draw, Ease, HEAD_PIVOT, Hop, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, bump, kept, kf, lerp, mix_hop, rim_pt};
use crate::config::Style;
use crate::ui::geometry::{ROBOT_COMPACT, ShapeRect, corner_radius};
use crate::ui::play::art::cat::{CAT_SC, CatPose, Paw, SEAT, cat_at, tail};
use crate::ui::play::art::tube::{LimbStyle, Stripes, along, catmull_rom, curl, plen};
use crate::ui::play::shape::{Deform, lip_dent};
use crate::ui::play::trigger::Story;
use crate::ui::robot::{Expr, GRID};

use Ease::{Io, Lin, O, Sio};

/// The slot head has ducked to the crouched cat's head by now; the cat,
/// faded in over it, has taken over by `SHOWN` (its hop starts at 0.46).
const DUCKED: f32 = 0.35;
const SHOWN: f32 = 0.45;
/// The cat, crouched again (its hop back ends at 8.0), fades out over the
/// slot head from `LEAVING`; the slot head is alone by `LEFT` and rises home
/// by the end.
const LEAVING: f32 = 8.0;
const LEFT: f32 = 8.1;
/// How far a crouched cat's head is lowered on its grid (`art::cat`: `14 *
/// (1 - rise)`).
const CROUCH_DROP: f32 = 14.0;

/// The scarf's control points (the mockup's `scarfPath`): from beside the
/// cat under the lip, round the left end `gap` off it, then up the Notch's
/// side, or onto the Pill's face so it reads as a scarf over a shoulder.
fn scarf_controls(style: Style, s: ShapeRect, gap: f32) -> Vec<(f32, f32)> {
    let (left, top, bottom) = (s.x, s.y, s.y + s.h);
    let pill = style == Style::Pill;
    let mut p = vec![(left + 19.0, bottom - 0.8), (left + 15.5, bottom + 1.0)];
    let n = if pill { 10 } else { 6 };
    for k in 0..=n {
        let f = k as f32 / n as f32;
        p.push(rim_pt(style, s, -1.0, if pill { 1.0 - f } else { lerp(1.0, 0.55, f) }, gap));
    }
    if pill {
        let r = corner_radius(style, s);
        p.extend([(left + r + 5.0, top - gap + 0.4), (left + r + 12.0, top - gap + 1.6)]);
    } else {
        p.extend([(left - gap - 0.6, top + 9.0), (left - gap + 0.4, top + 4.5)]);
    }
    p
}

/// How far the tail's curl and width have grown: they grow over the first
/// fifth of the tail leaving the cat and shrink over the last fifth of its
/// way back (the mockup draws them whole at once), so the tail never pops.
fn grow(ext: f32) -> f32 {
    (ext / 0.2).clamp(0.0, 1.0)
}

/// The scarf's drawn centre line at `t`: the path `ext` of the way along
/// (`gap` closing with the `hug`), then its curling tip, which the tickle and
/// the sneeze curl further.
fn scarf_line(style: Style, s: ShapeRect, t: f32, ext: f32, hug: f32, tickle: f32, hatchi: f32) -> Vec<(f32, f32)> {
    let path = catmull_rom(&scarf_controls(style, s, lerp(3.4, 2.3, hug)), 6);
    let wave = 0.5 * (TAU * 0.9 * t).sin();
    let tip = lerp(1.6, 3.2, hug) + 1.4 * tickle + 0.4 * (TAU * 1.3 * t).sin() + 2.0 * hatchi;
    let sway = (10.0 * wave + 20.0 * tickle).to_radians();
    curl(&along(&path, plen(&path) * ext, 34), tip, (15.0 + 4.0 * tickle) * grow(ext), 14, sway)
}

/// The scarf's width along it: slimming, a puff near the tip, closed at it.
fn scarf_width(s: f32) -> f32 {
    3.4 - 1.0 * s + 1.2 * ((s - 0.78) / 0.22).clamp(0.0, 1.0).powf(1.5) * (1.0 - ((s - 0.97) / 0.03).clamp(0.0, 1.0))
}

/// One moment of the story on `stage`, the cat in the coat of its look.
/// `beat.alpha` eases it home, paced by the wind-down's time (`kept`): over
/// its first 0.17 the cat crouches back and the island settles, to 0.75 the
/// cat fades over the slot head where they hand over, then the slot head
/// rises home. Exactly at rest at 0 s, at the end and with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, .. } = *stage;
    // Cut short, the story holds where the cut found it and eases home from
    // there, so nothing it had not brought out shows.
    let (t, a) = (beat.cut_at.map_or(beat.t, |c| beat.t.min(c)), beat.alpha.clamp(0.0, 1.0));
    let mut out = Shot::rest(style, s);
    let secs = Story::SyalEkor.secs();
    if t <= 0.0 || t >= secs || a == 0.0 {
        return out;
    }
    // The slot where the renderer puts it (the clock on or off moves it).
    let Some(slot) = stage.room().robot else {
        return out;
    };
    // The slot head's grid and the seated cat's, dip per unit.
    let (k, unit) = (slot.w / GRID, CAT_SC * ROBOT_COMPACT / GRID);
    let x = slot.x + slot.w / 2.0;
    let bottom = s.y + s.h;

    // The slot head ducks to the crouched cat's head: the same place and
    // size, so the hand-over shows nothing.
    let crouch_y = bottom - (SEAT.1 - HEAD_PIVOT.1 - CROUCH_DROP) * CROUCH * unit;
    let crouch_k = CROUCH * unit / k;
    // Cut while fading in, the cat fades out from what it showed.
    let (fade, home) = (kept(a, 0.17, 0.75), kept(a, 0.75, 1.0));
    let duck = kf(t, &[(0.0, 0.0, Lin), (DUCKED, 1.0, Sio), (LEFT, 1.0, Lin), (secs, 0.0, Sio)]) * home;
    let show = kf(t, &[(DUCKED, 0.0, Lin), (SHOWN, 1.0, Lin), (LEAVING, 1.0, Lin), (LEFT, 0.0, Lin)])
        * fade;
    out.halo = 1.0 - show;
    out.slot = if show < 1.0 {
        Slot::Story(MascotShot {
            dy: (crouch_y - (slot.y + HEAD_PIVOT.1 * k)) * duck,
            sx: lerp(1.0, crouch_k, duck),
            sy: lerp(1.0, crouch_k, duck),
            blink: blinks(t, &[0.2]) > 0.25,
            ..MascotShot::REST
        })
    } else {
        Slot::Empty
    };
    if show <= 0.0 {
        return out;
    }

    // How much of the story the cat plays: all of it, or eased back to its
    // crouch over the wind-down's first 0.17.
    let f = kept(a, 0.0, 0.17);
    let h = mix_hop(t, 0.48, 7.6);
    let hop = Hop { dk: lerp(1.0, h.dk, f), sc: lerp(CROUCH, h.sc, f), y: h.y * f, sq: lerp(1.0, h.sq, f) };
    let out_of_dark = 1.0 - hop.dk;
    // The scarf stays as long as it was and fades over the wind-down's first
    // 0.58, loosening with the hug: drawn back, it would vanish at once.
    let ext = kf(t, &[(1.35, 0.0, Lin), (3.4, 1.0, Sio), (5.55, 1.0, Io), (5.68, 0.55, O), (6.1, 0.0, Sio)]);
    let hug = f * kf(t, &[(3.45, 0.0, Lin), (4.1, 1.0, Sio), (5.5, 1.0, Io), (5.62, 0.0, O)]);
    let tickle_at = kf(t, &[(4.55, 0.0, Lin), (4.95, 1.0, Sio), (5.35, 1.0, Io), (5.5, 0.0, Io)]);
    let sneeze_at = kf(t, &[(5.3, 0.0, Lin), (5.48, 1.0, Sio), (5.55, 0.0, O)]);
    let hatchi_at = bump(t, 5.53, 0.22);
    let (tickle, sneeze, hatchi) = (tickle_at * f, sneeze_at * f, hatchi_at * f);

    // The island: the lip pressed down where the cat sits (more by the
    // sneeze), the left end pulled in and the whole squeezed by the hug, the
    // sneeze's jolt sideways.
    out.deform = Deform {
        dents: vec![
            lip_dent(style, s, x, 12.0, 1.8 * out_of_dark),
            lip_dent(style, s, s.x + 4.0, 6.0, -1.2 * hug),
            lip_dent(style, s, x, 30.0, 2.5 * hatchi),
        ],
        squeeze: 0.018 * hug,
        bulge_left: -2.2 * hug,
        jolt: 1.6 * hatchi * (TAU * 9.0 * t).sin(),
        ..Deform::default()
    };

    // The scarf, under the cat, inside (under the clock).
    if ext > 0.01 {
        let line = scarf_line(style, s, t, ext, hug, tickle, hatchi);
        let style = LimbStyle {
            wf: Some(scarf_width),
            wm: grow(ext),
            fluff: 0.07,
            side_shade: Some((-1, 0.3)),
            stripes: Some(Stripes { start: 5.5, every: 4.4, w: 1.8, skip_base: 3.0, rgb: 0 }),
            tip: Some((3.6, 0)),
            ..LimbStyle::default()
        };
        out.parts.push(Part {
            layer: Layer::Inside,
            draw: Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items: tail(&line, &style, look) },
            alpha: show.min(kept(a, 0.0, 0.58)),
        });
    }

    // The face in the app's own expressions: the mockup's squint is Focus,
    // its closed eyes a blink; its 'o' mouth and its ear turns have no art
    // here and are left out.
    let expr = if f <= 0.5 {
        Expr::Idle
    } else if (5.9..6.5).contains(&t) {
        Expr::Alert
    } else if hatchi_at > 0.3 {
        Expr::Idle
    } else if tickle_at > 0.3 || sneeze_at > 0.3 {
        Expr::Focus
    } else if (3.9..4.7).contains(&t) {
        Expr::Happy
    } else {
        Expr::Idle
    };
    let slow_blink = (1.0 - (t - 6.8).abs() / 0.21).clamp(0.0, 1.0);
    let blink = f > 0.5 && (hatchi_at > 0.3 || blinks(t, &[0.2]).max(slow_blink) > 0.25);
    let look_x = if (1.6..3.4).contains(&t) { -1.0 } else { 0.0 };
    let look_y = if (1.6..2.6).contains(&t) { 0.8 } else { 0.0 };
    let face = MascotShot { expr, glance: (look_x * 2.4 * f, look_y * 2.2 * f), blink, ..MascotShot::REST };

    let sq = hop.sq - 0.06 * sneeze + 0.12 * hatchi;
    let pose = CatPose {
        dk: hop.dk,
        rise: None,
        head: (0.0, -2.0 * hatchi, -6.0 * tickle + 10.0 * hatchi),
        face,
        paws: [Paw::default(); 2],
        tail: None,
    };
    out.parts.push(Part {
        layer: Layer::Inside,
        draw: cat_at(
            (x, bottom + 1.8 * out_of_dark + hop.y),
            -8.0 * hatchi + 4.0 * tickle,
            hop.sc / sq.sqrt(),
            hop.sc * sq,
            &pose,
            look,
        ),
        alpha: show,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{CatColor, Mascot};
    use crate::ui::cat_style::CatLook;
    use crate::ui::play::state::Beat;
    use crate::ui::play::rim::rim;
    use crate::ui::play::stories::{Draw, Layer, MascotShot, Slot, group_matrix};
    use crate::ui::play::testing::shape;
    use crate::ui::play::trigger::Story;
    use crate::ui::robot::GRID;

    crate::ui::play::stories::story_contract_tests!(Story::SyalEkor, shot);

    const SIZES: [(f32, f32); 3] = crate::ui::play::stories::CONTRACT_SIZES;

    /// The idle island `w` x `h` with the cat, the clock on or off.
    fn stage(style: Style, w: f32, h: f32, beat: Beat, clock: bool) -> Stage {
        let look = CatLook { mascot: Mascot::Cat, ..CatLook::default() };
        Stage::idle(style, shape(style, w, h), beat, look, clock)
    }

    /// At `t`, wound down to `alpha` (the island spread open), with the clock.
    fn at(style: Style, w: f32, h: f32, t: f32, alpha: f32) -> Shot {
        shot(&stage(style, w, h, Beat { spread: 1.0, alpha, t, cut_at: None }, true))
    }

    /// The slot head `m` as the renderer places it on `st`: its pivot (window
    /// dip) and its dip per grid unit.
    fn slot_head(st: &Stage, m: &MascotShot) -> ((f32, f32), f32) {
        let r = st.room().robot.unwrap();
        let k = r.w / GRID;
        ((r.x + 32.0 * k + m.dx, r.y + 40.0 * k + m.dy), k * m.sx)
    }

    /// The figure's head, if the shot draws one: its pivot in window dip,
    /// its dip per grid unit, and the part's alpha.
    fn figure_head(shot: &Shot) -> Option<((f32, f32), f32, f32)> {
        shot.parts.iter().find_map(|p| match &p.draw {
            Draw::Group { at, rot_deg, scale, pivot, items } => items.iter().find_map(|d| match d {
                Draw::Head { at: h, shot } => {
                    let m = group_matrix(*at, *rot_deg, *scale, *pivot);
                    let x = h.0 * m.M11 + h.1 * m.M21 + m.M31;
                    let y = h.0 * m.M12 + h.1 * m.M22 + m.M32;
                    Some(((x, y), scale.0 * shot.sx, p.alpha))
                }
                _ => None,
            }),
            _ => None,
        })
    }

    #[test]
    fn the_slot_head_hands_over_to_the_crouched_cat_without_a_jump() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                for t in [0.36, 0.4, 0.44, 8.01, 8.05, 8.09] {
                    let case = format!("{style:?} {w}x{h} t={t}");
                    let st = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, true);
                    let shot = shot(&st);
                    // Both drawn at once, the figure fading over the slot head,
                    // in the same place at the same size: the head never moves.
                    let Slot::Story(m) = shot.slot else { panic!("{case}: {:?}", shot.slot) };
                    let ((sx, sy), sk) = slot_head(&st, &m);
                    let ((fx, fy), fk, alpha) = figure_head(&shot).expect("the figure");
                    assert!(alpha > 0.0 && alpha < 1.0, "{case}: alpha {alpha}");
                    assert!((sx - fx).abs() < 0.02 && (sy - fy).abs() < 0.02, "{case}: slot {sx},{sy} figure {fx},{fy}");
                    assert!((sk - fk).abs() < 1e-3 && (m.sy - m.sx).abs() < 1e-6, "{case}: slot {sk} figure {fk}");
                    assert_eq!(m.rot_deg, 0.0, "{case}");
                }
                // The figure alone in between, the slot head alone outside.
                assert_eq!(at(style, w, h, 0.46, 1.0).slot, Slot::Empty);
                assert_eq!(at(style, w, h, 7.9, 1.0).slot, Slot::Empty);
                assert!(figure_head(&at(style, w, h, 0.3, 1.0)).is_none());
                assert!(figure_head(&at(style, w, h, 8.15, 1.0)).is_none());
            }
        }
    }

    #[test]
    fn the_slot_head_ducks_and_rises_without_a_jump() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let mut last: Option<((f32, f32), f32)> = None;
                let mut t = 0.0;
                while t <= Story::SyalEkor.secs() {
                    let st = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, true);
                    let shot = shot(&st);
                    let now = match (shot.slot, figure_head(&shot)) {
                        (Slot::Story(m), _) => Some(slot_head(&st, &m)),
                        (Slot::App, _) => Some(slot_head(&st, &MascotShot::REST)),
                        (Slot::Empty, _) => None,
                    };
                    if let (Some((p, k)), Some((q, j))) = (now, last) {
                        assert!((p.0 - q.0).abs() < 0.5 && (p.1 - q.1).abs() < 0.5 && (k - j).abs() < 0.02,
                            "{style:?} {w}x{h} t={t}: {q:?} to {p:?}");
                    }
                    last = now.or(last);
                    t += 0.01;
                }
            }
        }
    }

    #[test]
    fn the_tail_follows_the_rim_round_the_left_end() {
        // Between the samples it takes on the end (`rimPt`), the scarf's
        // centre line keeps the mockup's gap from the island's edge.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                for gap in [3.4, 2.3] {
                    let controls = scarf_controls(style, s, gap);
                    let n = if style == Style::Pill { 10 } else { 6 };
                    let path = crate::ui::play::art::tube::catmull_rom(&controls[1..=n + 3], 6);
                    // Its spans between the end's samples only.
                    let end = &path[6..6 * (n + 1) + 1];
                    let edge: Vec<_> = (0..=2000).map(|i| rim(style, s, i as f32 / 4000.0)).collect();
                    for p in end {
                        let d = edge.iter().map(|e| (e.x - p.0).hypot(e.y - p.1)).fold(f32::MAX, f32::min);
                        assert!((d - gap).abs() < 1.0, "{style:?} {w}x{h} gap {gap}: {p:?} is {d} off the edge");
                    }
                    // It goes round the end: below the lip, then beside or over the end.
                    assert!(end[0].1 > s.y + s.h, "{style:?} {w}x{h}");
                    assert!(end.iter().any(|p| p.0 < s.x - gap + 0.5), "{style:?} {w}x{h}");
                }
            }
        }
    }

    /// The tail's part: a group of the coat's fur, before the cat.
    fn tail_part(shot: &Shot) -> Option<&super::super::Part> {
        shot.parts.iter().find(|p| matches!(&p.draw, Draw::Group { items, .. } if !items.iter().any(|d| matches!(d, Draw::Head { .. }))))
    }

    #[test]
    fn the_tail_hugs_the_left_end_and_the_island_gives() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let hug = at(style, w, h, 4.3, 1.0);
                assert!(hug.deform.squeeze > 0.015 && hug.deform.bulge_left < -2.0, "{case}: {:?}", hug.deform);
                // The tail and then the cat, both inside (under the clock), as
                // the mockup layers them.
                assert_eq!(hug.parts.len(), 2, "{case}");
                assert!(hug.parts.iter().all(|p| p.layer == Layer::Inside && p.alpha == 1.0), "{case}");
                assert!(tail_part(&hug).is_some() && figure_head(&hug).is_some(), "{case}");
                assert!(std::ptr::eq(tail_part(&hug).unwrap(), &hug.parts[0]), "{case}: the tail is under the cat");
                // The tail reaches round the end: left of the island.
                let b = hug.parts[0].draw.bounds();
                assert!(b.x < shape(style, w, h).x - 3.0, "{case}: {b:?}");
                // No tail before it leaves the cat or after it is back.
                assert!(tail_part(&at(style, w, h, 1.3, 1.0)).is_none(), "{case}");
                assert!(tail_part(&at(style, w, h, 6.15, 1.0)).is_none(), "{case}");
            }
        }
    }

    #[test]
    fn the_tail_never_pops_in_or_out() {
        // The tail's box grows from nothing as it leaves the cat and shrinks
        // to nothing as it goes back.
        let size = |t: f32| tail_part(&at(Style::Pill, 190.0, 34.0, t, 1.0)).map_or(0.0, |p| {
            let b = p.draw.bounds();
            b.w.max(b.h)
        });
        let mut t = 1.3;
        let mut last = 0.0;
        while t < 6.2 {
            let now = size(t);
            assert!((now - last).abs() < 3.0, "t={t}: {last} to {now}");
            last = now;
            t += 0.01;
        }
    }

    #[test]
    fn the_sneeze_shifts_the_island_and_returns() {
        // The mockup's `dx`: the island alone jolts, the clock stays (a jolt,
        // never the shift the clock rides).
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let jolt = |t: f32| at(style, w, h, t, 1.0).deform.jolt;
                assert!(jolt(5.64).abs() > 1.2, "{style:?} {w}x{h}: {}", jolt(5.64));
                for t in [5.4, 5.52, 5.76, 6.0] {
                    assert_eq!(jolt(t), 0.0, "{style:?} {w}x{h} t={t}");
                }
                let mut t = 0.0;
                while t < Story::SyalEkor.secs() {
                    let d = at(style, w, h, t, 1.0).deform;
                    let s = shape(style, w, h);
                    assert_eq!(crate::ui::play::shape::rigid(style, s, &d), windows_numerics::Matrix3x2::identity(), "{style:?} t={t}");
                    t += 0.02;
                }
                // The sneeze's own dent pushes the lip down under the cat.
                let d = at(style, w, h, 5.64, 1.0).deform;
                assert!(d.dents.iter().any(|k| k.depth * shape(style, w, h).h > 2.0), "{d:?}");
            }
        }
    }

    #[test]
    fn a_wind_down_eases_the_cat_home_through_its_slot() {
        // Cut short mid-hug, a real wind-down in 60 Hz frames, the clock on
        // or off (the slot sits elsewhere without it): the cat crouches back,
        // fades over the slot head exactly where they hand over, and the slot
        // head rises home, never moving more than 3 dip a frame of its own.
        use crate::ui::play::stories::wind_down_frames;
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    let frames = wind_down_frames(Story::SyalEkor, 4.3, 0.0, |beat| shot(&stage(style, w, h, beat, clock)));
                    let mut last: Option<(((f32, f32), f32), f32)> = None;
                    let mut crossed = false;
                    for (ms, beat, shot) in &frames {
                        let st = stage(style, w, h, *beat, clock);
                        let case = format!("{case} +{ms:.1} ms (alpha {})", beat.alpha);
                        if beat.alpha <= 0.9 {
                            assert!(crate::ui::play::shape::is_rest(style, &shot.deform), "{case}");
                        }
                        // Both drawn: the two heads are one.
                        if let (Some((p, k, _)), Slot::Story(m)) = (figure_head(shot), shot.slot) {
                            let (q, j) = slot_head(&st, &m);
                            assert!((p.0 - q.0).abs() < 0.02 && (p.1 - q.1).abs() < 0.02 && (k - j).abs() < 1e-3, "{case}: figure {p:?} slot {q:?}");
                            crossed = true;
                        }
                        let now = match (figure_head(shot), shot.slot) {
                            (Some((p, k, alpha)), _) if alpha >= 1.0 => Some((p, k)),
                            (_, Slot::Story(m)) => Some(slot_head(&st, &m)),
                            (_, Slot::App) => Some(slot_head(&st, &MascotShot::REST)),
                            (None, Slot::Empty) => panic!("{case}: nothing shows the cat"),
                            (Some((p, k, _)), Slot::Empty) => Some((p, k)),
                        };
                        // The slot moves with the falling spread; the head
                        // moves with it, and on its own never jumps.
                        let slot_x = st.room().robot.unwrap().x;
                        if let (Some((p, k)), Some(((q, j), was))) = (now, last) {
                            let own = (p.0 - q.0) - (slot_x - was);
                            assert!(own.abs() < 3.0 && (p.1 - q.1).abs() < 3.0 && (k - j).abs() < 0.1, "{case}: {q:?} to {p:?}, the slot moved {}", slot_x - was);
                        }
                        last = now.map(|n| (n, slot_x));
                    }
                    assert!(crossed, "{case}: no cross-fade seen");
                    let (_, beat, end) = frames.last().unwrap();
                    assert_eq!(beat.alpha, 0.0, "{case}");
                    assert_eq!(*end, Shot::rest(style, shape(style, w, h)), "{case}");
                }
            }
        }
    }

    #[test]
    fn the_drawn_tail_follows_the_rim_round_the_left_end() {
        // The centre line handed to the tube (resampled and curled), wrapped
        // at the hug: where it goes round the end it keeps the gap.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let line = scarf_line(style, s, 4.3, 1.0, 1.0, 0.0, 0.0);
                let edge: Vec<_> = (0..=2000).map(|i| rim(style, s, i as f32 / 4000.0)).collect();
                let mut round = 0;
                // The resampled path from where it meets the end (its first
                // sample on it; before that it runs under the lip from the
                // cat), to where the curl turns away on purpose.
                let first = scarf_controls(style, s, 2.3)[2];
                let from = (0..=34).min_by(|&a, &b| {
                    let d = |i: usize| (line[i].0 - first.0).hypot(line[i].1 - first.1);
                    d(a).total_cmp(&d(b))
                }).unwrap();
                for p in &line[from..=34] {
                    let e = edge.iter().min_by(|a, b| (a.x - p.0).hypot(a.y - p.1).total_cmp(&(b.x - p.0).hypot(b.y - p.1))).unwrap();
                    if e.nx < -0.2 {
                        let d = (e.x - p.0).hypot(e.y - p.1);
                        assert!((d - 2.3).abs() < 1.0, "{style:?} {w}x{h}: {p:?} is {d} off the edge");
                        round += 1;
                    }
                }
                assert!(round >= 8, "{style:?} {w}x{h}: only {round} points round the end");
            }
        }
    }

    #[test]
    fn the_slot_heads_halo_fades_as_the_cat_fades_in_and_back() {
        // The see-through halo is the slot head's alone: it gives way as the
        // figure (which has none) shows, so it never pops at the hand-over.
        for t in [0.2, 0.35, 0.4, 0.44, 8.01, 8.05, 8.1, 8.3] {
            let shot = at(Style::Pill, 190.0, 34.0, t, 1.0);
            let show = figure_head(&shot).map_or(0.0, |(_, _, a)| a);
            assert!((shot.halo - (1.0 - show)).abs() < 1e-5, "t={t}: halo {} with the figure at {show}", shot.halo);
        }
        assert_eq!(Shot::rest(Style::Pill, shape(Style::Pill, 190.0, 34.0)).halo, 1.0);
    }

    /// Every frame of a real wind-down cut at `cut_s` seconds, at 60 Hz with
    /// the frames `phase_ms` after the cut: (ms into it, stage, shot).
    fn wind_down(cut_s: f32, phase_ms: f32) -> Vec<(f32, Stage, Shot)> {
        use crate::ui::play::state::Play;
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut play = Play::start(Story::SyalEkor, t0);
        let cut = t0 + Duration::from_secs_f32(cut_s);
        play.end(cut);
        let mut out = Vec::new();
        let mut ms = phase_ms;
        while ms <= 220.0 {
            let beat = play.beat(cut + Duration::from_secs_f32(ms / 1000.0));
            let st = stage(Style::Notch, 190.0, 34.0, beat, true);
            out.push((ms, st, shot(&st)));
            ms += 1000.0 / 60.0;
        }
        out
    }

    #[test]
    fn a_cut_before_the_slot_head_has_ducked_never_brings_the_cat_out() {
        // The figure had not begun to show: the slot head goes home alone.
        for cut_s in [0.2, 0.3, 0.34] {
            for (ms, _, shot) in wind_down(cut_s, 0.0) {
                assert!(shot.parts.is_empty(), "cut {cut_s} +{ms} ms: {} parts", shot.parts.len());
                assert!(matches!(shot.slot, Slot::Story(_) | Slot::App), "cut {cut_s} +{ms} ms: {:?}", shot.slot);
            }
        }
    }

    #[test]
    fn a_cut_mid_hand_over_fades_the_cat_out_frame_by_frame() {
        // Cut while the figure fades in over the slot head, the story's time
        // running on past the hand-over: the figure keeps what it showed at
        // the cut and fades from there, never rising, never jumping more than
        // 0.15 in a 60 Hz frame, its halo giving way to it, the two heads one.
        for cut_s in [0.36, 0.40, 0.44, 0.449] {
            for phase in 0..17 {
                let frames = wind_down(cut_s, phase as f32);
                let at_cut = at(Style::Notch, 190.0, 34.0, cut_s, 1.0);
                let mut last = figure_head(&at_cut).map_or(0.0, |(_, _, a)| a);
                for (ms, st, shot) in frames {
                    let case = format!("cut {cut_s} +{ms:.1} ms (alpha {})", st.beat.alpha);
                    let alpha = figure_head(&shot).map_or(0.0, |(_, _, a)| a);
                    assert!(alpha <= last + 1e-6 && last - alpha < 0.15, "{case}: the figure from {last} to {alpha}");
                    assert!((shot.halo - (1.0 - alpha)).abs() < 1e-5, "{case}: halo {} with the figure at {alpha}", shot.halo);
                    if let (Some((p, k, _)), Slot::Story(m)) = (figure_head(&shot), shot.slot) {
                        let ((q0, q1), j) = slot_head(&st, &m);
                        assert!((p.0 - q0).abs() < 0.02 && (p.1 - q1).abs() < 0.02 && (k - j).abs() < 1e-3, "{case}: figure {p:?} slot {:?}", (q0, q1));
                    }
                    last = alpha;
                }
            }
        }
    }

    #[test]
    fn the_cat_and_its_tail_wear_the_coat() {
        for color in [CatColor::Terracotta, CatColor::Calico, CatColor::White, CatColor::Black] {
            let look = CatLook { mascot: Mascot::Cat, color, ..CatLook::default() };
            let st = Stage { look, ..stage(Style::Notch, 190.0, 34.0, Beat { spread: 1.0, alpha: 1.0, t: 4.3, cut_at: None }, true) };
            let shot = shot(&st);
            let tail = tail_part(&shot).expect("the tail");
            let Draw::Group { items, .. } = &tail.draw else { unreachable!() };
            assert!(items.iter().any(|d| matches!(d, Draw::Fill { rgb, .. } if *rgb == look.head_rgb())), "{color:?}");
        }
    }

    #[test]
    fn the_slot_head_and_the_cat_show_one_cat_at_a_time() {
        // The slot head waits crouched where the cat's head is while the cat
        // takes over and hands back, and only then: played, and through real
        // wind-downs cut at the hand-overs, the hops and mid story.
        use crate::ui::play::stories::{one_mascot_at_a_time, wind_down_frames};
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    let mut t = 0.0;
                    while t <= Story::SyalEkor.secs() {
                        let st = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, clock);
                        if let Err(why) = one_mascot_at_a_time(&st, &shot(&st)) {
                            panic!("{case} t={t}: {why}");
                        }
                        t += 0.005;
                    }
                    for cut in [0.4, 0.6, 2.0, 4.8, 5.6, 7.8, 8.05] {
                        for phase in [0.0, 8.0] {
                            for (ms, beat, shot) in wind_down_frames(Story::SyalEkor, cut, phase, |beat| shot(&stage(style, w, h, beat, clock))) {
                                if let Err(why) = one_mascot_at_a_time(&stage(style, w, h, beat, clock), &shot) {
                                    panic!("{case} cut {cut} +{ms:.1} ms (alpha {}): {why}", beat.alpha);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
