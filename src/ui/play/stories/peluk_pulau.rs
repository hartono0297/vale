//! The octopus's "Peluk pulau" (hug the island): the slot head sinks into
//! the island's dark and its eyes glint across to the middle; the mantle
//! comes down from behind the lip, small, and puffs up under the island; its
//! arms creep under the island to both ends and round them onto its face; it
//! hugs (the island squeezed narrower and taller, the octopus flushed), lets
//! go (the island springs back, the arms unwind from their tips), goes back
//! up behind the lip, its eyes glint home and the slot head rises. Timeline,
//! easing and offsets from `docs/design/maskot-notch-v3.html`, story `o2`.

use std::f32::consts::TAU;

use super::{Draw, Ease, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, island_box, kept, kf, lerp, rim_pt, ring};
use crate::config::Style;
use crate::ui::geometry::{ShapeRect, WINDOW_H, WINDOW_W};
use crate::ui::play::art::octopus::{arm, mantle, root};
use crate::ui::play::art::tube::{along, catmull_rom, chain, curl, plen};
use crate::ui::play::shape::Deform;
use crate::ui::play::trigger::Story;
use crate::ui::robot::{Expr, Seg};

use Ease::{I, Io, Lin, O, Ob, Sio};

/// The eye glints moving through the dark: the mockup's `glints`.
const GLINT: u32 = 0xE9DCC8;

/// Where the long arm's tip rests on the rim (`rim_pt`'s `u`): the Notch's
/// straight side, above the Pill's middle.
pub const TIP_U: f32 = 0.25;

/// `items` (window dip) as one figure in `layer`.
fn figure(layer: Layer, items: Vec<Draw>, alpha: f32) -> Part {
    Part { layer, draw: Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items }, alpha }
}

/// The two eye glints at `x`, `y` in the dark, at `op`.
fn glints(x: f32, y: f32, op: f32) -> [Part; 2] {
    [-2.6, 2.6].map(|d| Part { layer: Layer::Dark, draw: Draw::Ellipse { c: (x + d, y), r: (1.15, 1.7), rgb: GLINT }, alpha: op * 0.9 })
}

/// The long arm's centre line on side `side` (-1 left, 1 right) from its
/// root (the mockup's `D2`): down under the island, along under the lip
/// (`sag` below it), round the end `3.4` off the rim and up to `TIP_U`.
pub fn long_arm_path(style: Style, s: ShapeRect, side: f32, from: (f32, f32), sag: f32) -> Vec<(f32, f32)> {
    let (cx, half, by) = (s.x + s.w / 2.0, s.w / 2.0, s.y + s.h);
    let x = |dx: f32, y: f32| (cx + dx * side, y);
    let rm = |u: f32| rim_pt(style, s, side, u, 3.4);
    catmull_rom(
        &[
            from,
            x(-half * 0.16, by + 20.0),
            x(-half * 0.3, by + 9.0 + sag),
            x(-half * 0.55, by + 4.0 + sag),
            x(-half * 0.78, by + 3.2 + sag * 0.5),
            rm(0.98),
            rm(0.78),
            rm(0.55),
            rm(0.36),
            rm(TIP_U),
        ],
        8,
    )
}

/// One moment of the story on `stage`, the octopus in the coat of its look.
/// `beat.alpha` eases it home, paced by the wind-down's time (`kept`): over
/// its first 0.58 the glints, the mantle and its arms fade and the island
/// settles; from 0.3 to 0.88 the slot head comes back out of the dark where
/// it sank, and from 0.6 it rises home. Cut short, the story holds where the
/// cut found it, so nothing it had not brought out shows. Exactly at rest at
/// 0 s, at the end and with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, .. } = *stage;
    let a = beat.alpha.clamp(0.0, 1.0);
    let t = beat.cut_at.map_or(beat.t, |c| beat.t.min(c));
    let mut out = Shot::rest(style, s);
    if t <= 0.0 || t >= Story::PelukPulau.secs() || a == 0.0 {
        return out;
    }
    // The slot where the renderer puts it (the clock on or off moves it).
    let Some(slot) = stage.room().robot else {
        return out;
    };
    let play = kept(a, 0.0, 0.58);
    let (cx, mid, bottom) = (s.x + s.w / 2.0, s.y + s.h / 2.0, s.y + s.h);
    let slot_x = slot.x + slot.w / 2.0;

    // The slot head (the sitting octopus) sinks into the dark toward the
    // slot's foot, shrinking, and later rises back, overshooting, with a
    // squash; it looks down as it goes and comes home happy. The mockup's
    // darkening is its fade, its halo fading with it.
    let dep = kf(t, &[(1.0, 0.0, Lin), (1.3, 1.0, I), (6.95, 1.0, Lin), (7.3, 0.0, Ob)]);
    let sy0 = kf(t, &[(0.7, 1.0, Lin), (1.0, 0.86, O), (1.15, 1.1, Io), (1.3, 1.0, Io), (6.95, 1.0, Lin), (7.15, 1.15, O), (7.3, 0.88, O), (7.6, 1.0, Io)]);
    let ly = kf(t, &[(0.4, 0.0, Lin), (0.6, 1.0, Io), (1.2, 1.0, Io), (7.3, 1.0, Lin), (7.6, 0.0, Io)]);
    let home = kept(a, 0.6, 1.0);
    // Its fade, eased in and out: over the sink, and back over the rise's
    // first 0.25 s (whole by the overshoot). The sink's own ease would take
    // it 0.16 in a frame, the rise's 0.22.
    let fade = kf(t, &[(1.0, 0.0, Lin), (1.3, 1.0, Sio), (6.95, 1.0, Lin), (7.2, 0.0, Sio)]);
    let (sink, dark) = (dep * home, fade * kept(a, 0.3, 0.88));
    let (sc, sq) = (1.0 - 0.45 * sink, lerp(1.0, sy0, home));
    out.slot = Slot::Story(MascotShot {
        dx: 2.0 * sink,
        dy: 4.0 * sink,
        sx: sc / sq.sqrt(),
        sy: sc * sq,
        expr: if (7.3..7.7).contains(&t) { Expr::Happy } else { Expr::Idle },
        glance: (0.0, ly * 2.2 * home),
        alpha: 1.0 - dark,
        ..MascotShot::REST
    });

    // The island: squeezed narrower and taller by the hug, then springing
    // back past rest and settling (the Notch from the screen edge, the Pill
    // about its centre: `Deform::stretch`).
    let hug = kf(t, &[(3.75, 0.0, Lin), (4.15, 1.0, Io), (4.6, 1.0, Io), (4.75, 0.0, O)]);
    let nsq = (hug * 0.03 + ring(t, 4.75, 0.028, 2.6, 4.5)) * play;
    out.deform = Deform { squeeze: nsq, stretch: 2.4 * nsq, ..Deform::default() };
    out.island = island_box(style, s, &out.deform);
    if play <= 0.0 {
        return out;
    }

    // The eyes glint through the dark from the slot to the middle, and back.
    let ga = slot_x + 4.0;
    let fade = |t0: f32, t1: f32| ((t - t0) / 0.1).clamp(0.0, 1.0) * ((t1 - t) / 0.1).clamp(0.0, 1.0);
    if (1.25..1.95).contains(&t) {
        let u = kf(t, &[(1.3, 0.0, Lin), (1.9, 1.0, Sio)]);
        out.parts.extend(glints(lerp(ga, cx, u), mid + 4.5, fade(1.25, 1.95) * play));
    }
    if (6.3..7.0).contains(&t) {
        let u = kf(t, &[(6.3, 0.0, Lin), (6.95, 1.0, Sio)]);
        out.parts.extend(glints(lerp(cx, ga, u), mid + 4.5, fade(6.3, 7.0) * play));
    }

    // The mantle comes down from behind the lip in the middle, small, then
    // puffs up; it goes back up the same way.
    let hide = mid - 5.0;
    let my = kf(t, &[(1.9, hide, Lin), (2.45, bottom + 8.0, O), (2.9, bottom + 16.0, O), (5.9, bottom + 16.0, Io), (6.15, bottom + 8.0, Io), (6.4, hide, I)])
        + ring(t, 2.9, 1.5, 2.0, 5.0);
    if my <= hide + 0.5 {
        return out;
    }
    let k = kf(t, &[(2.45, 1.0, Lin), (2.9, 1.75, Ob), (5.85, 1.75, Io), (6.15, 1.0, I)]);
    let m = (cx, my + hug);
    let msy = kf(t, &[
        (1.9, 1.4, Lin), (2.4, 1.3, Io), (2.55, 0.86, O), (2.9, 1.04, Io), (3.1, 1.0, Io), (3.75, 1.0, Io), (4.15, 0.88, Io),
        (4.6, 0.88, Io), (4.8, 1.06, O), (5.0, 1.0, Io), (5.9, 1.0, Io), (6.1, 0.9, Io), (6.4, 1.35, Io),
    ]);
    let flush = kf(t, &[(3.8, 0.0, Lin), (4.2, 0.8, Io), (5.4, 0.3, Io), (6.5, 0.0, Io)]);
    let sag = lerp(3.4, 0.0, hug) + ring(t, 4.75, 2.0, 2.0, 4.0);
    let e1 = kf(t, &[(2.45, 0.0, Lin), (3.0, 1.0, O), (5.35, 1.0, Io), (5.85, 0.0, I)]);
    let e2 = kf(t, &[(2.55, 0.0, Lin), (3.65, 1.0, Io), (5.0, 1.0, Io), (5.85, 0.0, I)]);
    let cu = lerp(2.4, 3.6, hug) - kf(t, &[(4.95, 0.0, Lin), (5.25, 2.2, Io)]);

    // Per side: the inner arm grabs the lip near the middle, the long arm
    // runs under the island, round its end and curls onto its face, the third
    // dangles. Behind the island, and the inner and long arms again on its
    // face (clipped to it), where their tips curl over it.
    let coat = look.octopus;
    let (half, mut back, mut wrap) = (s.w / 2.0, Vec::new(), Vec::new());
    for side in [-1.0_f32, 1.0] {
        let x = |dx: f32, y: f32| (cx + dx * side, y);
        let (i1, i2, i3) = if side < 0.0 { (1, 0, 2) } else { (4, 5, 3) };
        let at = |i: usize| root(m, i, 0.0, k, k * msy);
        let d1 = catmull_rom(&[at(i1), x(-half * 0.12, bottom + 20.0), x(-half * 0.19, bottom + 10.0), x(-half * 0.23, bottom + 3.4 + sag * 0.3), x(-half * 0.27, bottom + 1.4)], 8);
        let a1 = curl(&along(&d1, plen(&d1) * e1, 20), side * cu * 0.8, 8.0, 10, 0.0);
        let d2 = long_arm_path(style, s, side, at(i2), sag);
        let a2 = curl(&along(&d2, plen(&d2) * e2, 50), side * cu, 10.0, 12, (-side * 6.0 * (TAU * 1.1 * t).sin()).to_radians());
        let sw = 12.0 * (TAU * 0.7 * t + side).sin() - hug * 10.0 * side;
        let r3 = at(i3);
        let a3 = chain(r3.0, r3.1, (90.0 - side * 18.0 + sw).to_radians(), 11.0 * k, 16, |q| (-side * 200.0 * q).to_radians());
        let si = side as i8;
        let (inner, long) = (arm(&a1, coat, si, k * 0.95, flush), arm(&a2, coat, si, k, flush));
        back.push(figure(Layer::Back, arm(&a3, coat, -si, k * 0.85, flush), play));
        back.push(figure(Layer::Back, long.clone(), play));
        back.push(figure(Layer::Back, inner.clone(), play));
        wrap.push(figure(Layer::Wrap(None), long, play));
        wrap.push(figure(Layer::Wrap(None), inner, play));
    }

    // The mantle over its arms: happy in the hug, startled as it lets go.
    // Cut off above the island's top edge: it starts and ends inside the
    // island, where a small Pill would show its crown (or its accessory)
    // above it.
    let expr = if hug > 0.5 {
        Expr::Happy
    } else if (4.75..5.0).contains(&t) {
        Expr::Alert
    } else {
        Expr::Idle
    };
    let face = MascotShot { expr, glance: (0.0, -0.4 * 2.2), blink: blinks(t, &[3.3]) > 0.25, flush, ..MascotShot::REST };
    let head = mantle(m, 2.0 * (TAU * 0.6 * t).sin(), k / msy.sqrt() * (1.0 + hug * 0.08), k * msy, &face);
    let below_top = vec![Seg::Move(0.0, s.y), Seg::Line(WINDOW_W, s.y), Seg::Line(WINDOW_W, WINDOW_H), Seg::Line(0.0, WINDOW_H), Seg::Close];
    back.push(Part { layer: Layer::Back, draw: Draw::Clipped { clip: below_top, items: vec![head] }, alpha: play });
    out.parts.extend(back);
    out.parts.extend(wrap);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Mascot, OctopusColor};
    use crate::ui::cat_style::CatLook;
    use crate::ui::geometry::{corner_radius, shoulder_depth};
    use crate::ui::mascot::{FLUSH_RGB, mix, octopus_rgb};
    use crate::ui::play::shape::is_rest;
    use crate::ui::play::state::{Beat, Play};
    use crate::ui::play::stories::{FRAME_FADE, wind_down_frames};
    use crate::ui::play::testing::shape;
    use std::time::{Duration, Instant};

    crate::ui::play::stories::story_contract_tests!(Story::PelukPulau, shot);

    const SIZES: [(f32, f32); 3] = crate::ui::play::stories::CONTRACT_SIZES;
    const SECS: f32 = 7.8;

    /// The idle island `w` x `h` with the octopus in `coat`, the clock on or off.
    fn stage_in(style: Style, w: f32, h: f32, beat: Beat, clock: bool, coat: OctopusColor) -> Stage {
        let look = CatLook { mascot: Mascot::Octopus, octopus: coat, ..CatLook::default() };
        Stage::idle(style, shape(style, w, h), beat, look, clock)
    }

    fn stage(style: Style, w: f32, h: f32, beat: Beat, clock: bool) -> Stage {
        stage_in(style, w, h, beat, clock, OctopusColor::Coral)
    }

    fn playing(t: f32) -> Beat {
        Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }
    }

    /// At `t`, playing, with the clock.
    fn at(style: Style, w: f32, h: f32, t: f32) -> Shot {
        shot(&stage(style, w, h, playing(t), true))
    }

    fn layer(shot: &Shot, pick: fn(Layer) -> bool) -> Vec<&Part> {
        shot.parts.iter().filter(|p| pick(p.layer)).collect()
    }

    fn back(shot: &Shot) -> Vec<&Part> {
        layer(shot, |l| l == Layer::Back)
    }

    fn wrap(shot: &Shot) -> Vec<&Part> {
        layer(shot, |l| matches!(l, Layer::Wrap(_)))
    }

    fn dark(shot: &Shot) -> Vec<&Part> {
        layer(shot, |l| l == Layer::Dark)
    }

    /// The mantle behind the island: its pivot and shot.
    fn mantle_of(shot: &Shot) -> Option<((f32, f32), MascotShot)> {
        back(shot).into_iter().find_map(|p| match &p.draw {
            Draw::Clipped { items, .. } => match items.first() {
                Some(Draw::Head { at, shot }) => Some((*at, *shot)),
                _ => None,
            },
            _ => None,
        })
    }

    /// The slot head.
    fn slot_head(shot: &Shot) -> MascotShot {
        match shot.slot {
            Slot::Story(m) => m,
            other => panic!("{other:?}"),
        }
    }

    /// The first fill's colour of an arm (its body).
    fn body_rgb(p: &Part) -> Option<u32> {
        match &p.draw {
            Draw::Group { items, .. } => items.iter().find_map(|d| match d {
                Draw::Fill { rgb, .. } => Some(*rgb),
                _ => None,
            }),
            _ => None,
        }
    }

    #[test]
    fn the_hug_squeezes_and_springs_back_to_rest() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let case = format!("{style:?} {w}x{h}");
                for t in [1.0, 3.0, 3.75] {
                    assert!(is_rest(style, &at(style, w, h, t).deform), "{case} t={t}");
                }
                // At its height the mockup's sx 0.97 and sy 1 + 0.03 * 2.4:
                // the Notch grows down from the screen edge, the Pill about
                // its centre.
                for t in [4.15, 4.4, 4.6] {
                    let shot = at(style, w, h, t);
                    let d = &shot.deform;
                    assert!((d.squeeze - 0.03).abs() < 1e-4 && (d.stretch - 0.072).abs() < 1e-4, "{case} t={t}: {d:?}");
                    let b = shot.island;
                    let mid = s.y + s.h / 2.0;
                    if style == Style::Notch {
                        assert!((b.y - s.y).abs() < 1e-3 && (b.y + b.h - (s.y + 1.072 * h)).abs() < 0.05, "{case} t={t}: {b:?}");
                        assert!((b.w - 0.97 * w - 2.0 * shoulder_depth(s)).abs() < 0.05, "{case} t={t}: {b:?}");
                    } else {
                        assert!((b.y - (mid - 0.536 * h)).abs() < 0.05 && (b.y + b.h - (mid + 0.536 * h)).abs() < 0.05, "{case} t={t}: {b:?}");
                        assert!((b.w - 0.97 * w).abs() < 0.05, "{case} t={t}: {b:?}");
                    }
                }
                // Let go, it springs back past rest (wider, shorter), settles,
                // and never steps.
                let (mut least, mut last) = (0.0_f32, 0.0_f32);
                let mut t = 3.5;
                while t < SECS {
                    let d = at(style, w, h, t).deform;
                    assert!((d.stretch - 2.4 * d.squeeze).abs() < 1e-6, "{case} t={t}: {d:?}");
                    // The let-go is quick (the mockup's 0.15 s ease out).
                    assert!((d.squeeze - last).abs() < 0.008, "{case} t={t}: {last} to {}", d.squeeze);
                    if t > 4.75 {
                        least = least.min(d.squeeze);
                    }
                    if t > 6.4 {
                        assert!(d.squeeze.abs() < 1e-4, "{case} t={t}: {d:?}");
                    }
                    last = d.squeeze;
                    t += 0.01;
                }
                assert!(least < -0.005, "{case}: {least}");
            }
        }
    }

    #[test]
    fn four_arms_and_their_tips_on_the_rim() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let mid = s.y + s.h / 2.0;
                for t in [3.7, 4.15, 4.6, 4.95] {
                    let case = format!("{style:?} {w}x{h} t={t}");
                    let shot = at(style, w, h, t);
                    // Per side a dangling, a long and an inner arm behind the
                    // island, then the mantle; the long and inner arms again on
                    // its face.
                    let (b, f) = (back(&shot), wrap(&shot));
                    assert_eq!((b.len(), f.len()), (7, 4), "{case}");
                    assert!(b[..6].iter().all(|p| matches!(p.draw, Draw::Group { .. })), "{case}");
                    assert!(mantle_of(&shot).is_some(), "{case}");
                    for (fi, bi) in [(0, 1), (1, 2), (2, 4), (3, 5)] {
                        assert_eq!(f[fi].draw, b[bi].draw, "{case}: the face's copy {fi}");
                    }
                    // Each long arm runs round its end and up to its tip on the rim.
                    for (fi, side) in [(0, -1.0), (2, 1.0)] {
                        let tip = rim_pt(style, s, side, TIP_U, 3.4);
                        let r = f[fi].draw.bounds();
                        assert!(tip.0 >= r.x - 0.5 && tip.0 <= r.x + r.w + 0.5 && tip.1 >= r.y - 0.5 && tip.1 <= r.y + r.h + 0.5, "{case}: {tip:?} {r:?}");
                        let round = if side < 0.0 { r.x < s.x - 3.0 } else { r.x + r.w > s.x + s.w + 3.0 };
                        assert!(round, "{case} side {side}: {r:?}");
                    }
                }
                // The long arm's line ends on the rim at `TIP_U`: the Notch's
                // straight side, above the Pill's middle.
                for side in [-1.0, 1.0] {
                    let p = long_arm_path(style, s, side, (s.x + s.w / 2.0, s.y + s.h + 20.0), 0.0);
                    let (end, tip) = (*p.last().unwrap(), rim_pt(style, s, side, TIP_U, 3.4));
                    assert!((end.0 - tip.0).abs() < 1e-4 && (end.1 - tip.1).abs() < 1e-4, "{style:?} {w}x{h}");
                    let on_side = if style == Style::Notch { tip.1 < s.y + s.h - corner_radius(style, s) } else { tip.1 < mid };
                    assert!(on_side, "{style:?} {w}x{h}: {tip:?}");
                }
            }
        }
    }

    #[test]
    fn the_figure_comes_and_goes_inside_the_island() {
        // The hand-off: the slot head has sunk out of sight before the
        // mantle shows, and the mantle and its arms first show, and last
        // show, wholly behind the island, so nothing pops.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                for clock in [true, false] {
                    let s = shape(style, w, h);
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    let inside = |shot: &Shot, t: f32| {
                        for p in back(shot) {
                            let r = p.draw.bounds();
                            let top = style == Style::Notch || r.y >= s.y - 1e-3;
                            assert!(r.x >= s.x && r.x + r.w <= s.x + s.w && r.y + r.h <= s.y + s.h && top, "{case} t={t}: {r:?}");
                        }
                    };
                    let (mut shown, mut changes, mut last) = (false, 0, None::<Shot>);
                    let mut t = 0.0;
                    while t <= SECS {
                        let now = shot(&stage(style, w, h, playing(t), clock));
                        let showing = !back(&now).is_empty();
                        if showing {
                            assert_eq!(slot_head(&now).alpha, 0.0, "{case} t={t}");
                        }
                        if showing != shown {
                            changes += 1;
                            inside(if showing { &now } else { last.as_ref().unwrap() }, t);
                        }
                        shown = showing;
                        last = Some(now);
                        t += 0.005;
                    }
                    assert_eq!(changes, 2, "{case}");
                }
            }
        }
    }

    #[test]
    fn the_slot_head_sinks_into_the_dark_and_rises_home_without_a_jump() {
        for style in [Style::Notch, Style::Pill] {
            let mut last = slot_head(&at(style, 190.0, 34.0, 0.001));
            let mut t = 0.01;
            while t < SECS {
                let shot = at(style, 190.0, 34.0, t);
                let m = slot_head(&shot);
                assert_eq!(shot.halo, 1.0, "{style:?} t={t}");
                // The rise is quick (the mockup's 0.35 s overshoot).
                assert!((m.dx - last.dx).abs() < 0.8 && (m.dy - last.dy).abs() < 0.8 && (m.sy - last.sy).abs() < 0.08 && (m.alpha - last.alpha).abs() < 0.15,
                    "{style:?} t={t}: {last:?} to {m:?}");
                last = m;
                t += 0.01;
            }
            // Down toward the slot's foot, shrinking, gone in the dark; home at the end.
            assert_eq!(slot_head(&at(style, 190.0, 34.0, 0.3)), MascotShot::REST);
            let sunk = slot_head(&at(style, 190.0, 34.0, 1.29));
            assert!(sunk.dy > 3.0 && sunk.dx > 1.5 && sunk.sx < 0.7 && sunk.alpha < 0.15, "{sunk:?}");
            for t in [1.3, 4.0, 6.9] {
                assert_eq!(slot_head(&at(style, 190.0, 34.0, t)).alpha, 0.0, "t={t}");
            }
            // It rises past its place (bigger for a moment), then settles.
            let over = slot_head(&at(style, 190.0, 34.0, 7.2));
            assert!(over.sx * over.sy > 1.0 && over.alpha == 1.0, "{over:?}");
            assert_eq!(slot_head(&at(style, 190.0, 34.0, 7.75)), MascotShot::REST);
        }
    }

    #[test]
    fn the_eyes_glint_across_the_dark_between_the_slot_and_the_middle() {
        for style in [Style::Notch, Style::Pill] {
            let st = stage(style, 190.0, 34.0, playing(1.6), true);
            let (s, slot) = (st.shape, st.room().robot.unwrap());
            let (from, to) = (slot.x + slot.w / 2.0 + 4.0, s.x + s.w / 2.0);
            let mid = s.y + s.h / 2.0;
            let x = |t: f32| {
                let shot = at(style, 190.0, 34.0, t);
                let g = dark(&shot);
                assert_eq!(g.len(), 2, "{style:?} t={t}");
                let c: Vec<(f32, f32)> = g.iter().map(|p| match p.draw { Draw::Ellipse { c, .. } => c, ref d => panic!("{d:?}") }).collect();
                assert!(c.iter().all(|c| (c.1 - mid - 4.5).abs() < 1e-4), "{c:?}");
                (c[0].0 + c[1].0) / 2.0
            };
            // Out to the middle, and home again.
            assert!((x(1.3) - from).abs() < 1e-3 && (x(1.9) - to).abs() < 1e-3, "{style:?}");
            assert!(x(1.5) > x(1.4) && x(1.7) > x(1.6), "{style:?}");
            assert!((x(6.3) - to).abs() < 1e-3 && (x(6.95) - from).abs() < 1e-3, "{style:?}");
            for t in [1.2, 2.0, 4.0, 6.25, 7.05] {
                assert!(dark(&at(style, 190.0, 34.0, t)).is_empty(), "{style:?} t={t}");
            }
        }
    }

    #[test]
    fn the_mantle_comes_down_small_puffs_up_and_flushes_in_the_hug() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let case = format!("{style:?} {w}x{h}");
                // Under the island's middle, small, then full size well below it.
                let ((p, small), (q, big)) = (mantle_of(&at(style, w, h, 2.45)).unwrap(), mantle_of(&at(style, w, h, 3.5)).unwrap());
                assert!(small.bare_octopus && big.bare_octopus, "{case}");
                assert!(small.sy < 1.1 && (big.sx - 1.75).abs() < 0.01 && (big.sy - 1.75).abs() < 0.01, "{case}: {small:?} {big:?}");
                assert!((p.0 - (s.x + s.w / 2.0)).abs() < 0.5 && q.1 > s.y + s.h + 16.0, "{case}: {p:?} {q:?}");
                // The hug flushes it, arms and all.
                let shot = at(style, w, h, 4.3);
                let (_, m) = mantle_of(&shot).unwrap();
                assert!(m.flush > 0.7, "{case}: {}", m.flush);
                let rgb = mix(octopus_rgb(OctopusColor::Coral), FLUSH_RGB, m.flush);
                for arm in &back(&shot)[..6] {
                    assert_eq!(body_rgb(arm), Some(rgb), "{case}");
                }
            }
        }
    }

    #[test]
    fn every_coat_colours_the_arms_like_the_head() {
        for coat in OctopusColor::ALL {
            let shot = shot(&stage_in(Style::Notch, 190.0, 34.0, playing(3.5), true, *coat));
            let parts: Vec<&Part> = back(&shot).into_iter().chain(wrap(&shot)).filter(|p| matches!(p.draw, Draw::Group { .. })).collect();
            assert_eq!(parts.len(), 10, "{coat:?}");
            for p in parts {
                assert_eq!(body_rgb(p), Some(octopus_rgb(*coat)), "{coat:?}");
            }
        }
    }

    #[test]
    fn the_faces_are_the_mockups_in_the_apps_expressions() {
        let face = |t: f32| mantle_of(&at(Style::Notch, 190.0, 34.0, t)).unwrap().1;
        // Looking up at the island; happy in the hug, startled letting go.
        assert_eq!(face(3.5).expr, Expr::Idle);
        assert!(face(3.5).glance.0 == 0.0 && (face(3.5).glance.1 + 0.88).abs() < 1e-5, "{:?}", face(3.5).glance);
        assert!(face(3.3).blink && !face(3.5).blink);
        assert_eq!(face(4.3).expr, Expr::Happy);
        assert_eq!(face(4.85).expr, Expr::Alert);
        assert_eq!(face(5.2).expr, Expr::Idle);
        // The slot head looks down as it sinks, comes home happy and looks
        // ahead again before the end.
        let slot = |t: f32| slot_head(&at(Style::Notch, 190.0, 34.0, t));
        assert_eq!(slot(1.0).glance, (0.0, 2.2));
        assert_eq!(slot(7.5).expr, Expr::Happy);
        assert_eq!((slot(7.72).expr, slot(7.65).glance), (Expr::Idle, (0.0, 0.0)));
    }

    /// The largest alpha of any part, and how many show.
    fn showing(shot: &Shot) -> (f32, usize) {
        let on: Vec<_> = shot.parts.iter().filter(|p| p.alpha > 0.0).collect();
        (on.iter().map(|p| p.alpha).fold(0.0, f32::max), on.len())
    }

    #[test]
    fn a_cut_just_before_something_shows_never_brings_it_out_in_the_wind_down() {
        // Cut just before the glints, the mantle, the hug and the glints
        // home: nothing new shows and the island never starts to squeeze.
        for style in [Style::Notch, Style::Pill] {
            for cut_s in [1.2, 1.88, 3.7, 6.25] {
                let t0 = Instant::now();
                let mut play = Play::start(Story::PelukPulau, t0);
                let cut = t0 + Duration::from_secs_f32(cut_s);
                play.end(cut);
                let mut last = (f32::INFINITY, usize::MAX);
                for ms in 0..=200 {
                    let beat = play.beat(cut + Duration::from_millis(ms));
                    let shot = shot(&stage(style, 190.0, 34.0, beat, true));
                    let now = showing(&shot);
                    assert!(now.0 <= last.0 && now.1 <= last.1, "{style:?} cut {cut_s} +{ms} ms (t={}): {last:?} to {now:?}", beat.t);
                    if cut_s == 3.7 {
                        assert!(is_rest(style, &shot.deform), "{style:?} +{ms} ms: {:?}", shot.deform);
                    }
                    last = now;
                }
            }
        }
    }

    #[test]
    fn a_wind_down_brings_the_octopus_home_through_its_slot() {
        // Cut mid-hug, a real wind-down in 60 Hz frames: the figure fades and
        // the island settles, the slot head comes back out of the dark where
        // it sank and rises home, a frame's fade and under 1.5 dip at a time.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    let frames = wind_down_frames(Story::PelukPulau, 4.3, 0.0, |beat| shot(&stage(style, w, h, beat, clock)));
                    let mut last: Option<MascotShot> = None;
                    let mut squeeze = f32::INFINITY;
                    for (ms, beat, shot) in &frames {
                        let case = format!("{case} +{ms:.1} ms (alpha {})", beat.alpha);
                        let Slot::Story(m) = shot.slot else {
                            continue;
                        };
                        if let Some(n) = last {
                            assert!((m.alpha - n.alpha).abs() <= FRAME_FADE + 1e-4 && (m.dy - n.dy).abs() < 1.5 && (m.dx - n.dx).abs() < 1.5, "{case}: {n:?} to {m:?}");
                        }
                        assert!(shot.deform.squeeze <= squeeze, "{case}: {:?}", shot.deform);
                        squeeze = shot.deform.squeeze;
                        last = Some(m);
                    }
                    let (_, beat, end) = frames.last().unwrap();
                    assert_eq!(beat.alpha, 0.0, "{case}");
                    assert_eq!(*end, Shot::rest(style, shape(style, w, h)), "{case}");
                    // Part way, the slot head is back out of the dark.
                    let (_, _, part) = frames.iter().find(|(_, b, _)| b.alpha < 0.3).unwrap();
                    assert!(slot_head(part).alpha > 0.0, "{case}");
                }
            }
        }
    }
}
