//! The octopus's "Penasaran jam" (curious about the clock): the slot head
//! glances at the clock and dives into the island's dark, two eye glints
//! travel right; one long arm comes out from behind the right end, winds
//! round it and reaches up the island's face to the minute's last digit
//! while the mantle peeks out from behind the end; the arm sticks to the
//! digit and pulls it, plop, it lets go, the digit wobbles home, the octopus
//! flushes, startled, and goes home through the dark. Timeline, easing and
//! offsets from `docs/design/maskot-notch-v3.html`, story `o1`.

use std::f32::consts::TAU;

use super::{
    DIGIT_X, Draw, Ease, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, circle_segs, kept, kf, lerp, rim_pt, ring,
    toward_rest,
};
use crate::config::Style;
use crate::ui::geometry::{Rect, ShapeRect, corner_radius};
use crate::ui::play::art::octopus::{arm, mantle, root};
use crate::ui::play::art::tube::{along, catmull_rom, chain, curl, plen};
use crate::ui::play::trigger::Story;
use crate::ui::render::DigitFx;
use crate::ui::robot::{CREAM, Expr};

use Ease::{I, Io, Lin, O, Ob, Sio};

type Pt = (f32, f32);

/// The digit the arm pulls: the minute's last.
const PULLED: usize = 4;
/// The two eye glints in the dark (the mockup's `glints`).
const GLINT_RGB: u32 = 0xE9DCC8;
/// The least the arm's root stays inside the Pill's end: more than half the
/// arm's width there (4.3), so its flat start never shows.
const ROOT_IN: f32 = 3.0;

/// `items` (window dip) as one figure in `layer`.
fn figure(layer: Layer, items: Vec<Draw>, alpha: f32) -> Part {
    Part { layer, draw: Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items }, alpha }
}

/// Keyframes of points, each axis eased alike.
fn kf2<const N: usize>(t: f32, keys: [(f32, Pt, Ease); N]) -> Pt {
    (kf(t, &keys.map(|(k, p, e)| (k, p.0, e))), kf(t, &keys.map(|(k, p, e)| (k, p.1, e))))
}

/// How far the arm has pulled the digit, 0 to 1; 0 again from the plop.
fn pull(t: f32) -> f32 {
    if t < 4.85 { kf(t, &[(4.3, 0.0, Lin), (4.85, 1.0, Sio)]) } else { 0.0 }
}

/// Where the arm leaves the octopus, behind the island's right end: the
/// mockup's 4 in from the end and 7 below the top. On the Pill that lies
/// close to (on a tall one, outside) the half circle, so it is pulled in
/// toward the end's centre until it is `ROOT_IN` inside.
fn root_pt(style: Style, s: ShapeRect) -> Pt {
    let p = (s.x + s.w - 4.0, s.y + 7.0);
    if style != Style::Pill {
        return p;
    }
    let r = corner_radius(style, s);
    let c = (s.x + s.w - r, s.y + s.h / 2.0);
    let (dx, dy) = (p.0 - c.0, p.1 - c.1);
    let (d, most) = (dx.hypot(dy), r - ROOT_IN);
    if d <= most { p } else { (c.0 + dx * most / d, c.1 + dy * most / d) }
}

/// The long arm's centre line at `t`, root first: out from behind the right
/// end, round it just outside its outline, under the lip and up the
/// island's face to the digit whose centre is at x `d4`. `None` while it is
/// in. The tip curl grows over the arm's first and last stretch, so the arm
/// comes out of the end and goes back into it without a jump.
fn arm_line(style: Style, s: ShapeRect, d4: f32, t: f32) -> Option<Vec<Pt>> {
    let ext = kf(t, &[(2.6, 0.0, Lin), (3.3, 1.0, O), (5.05, 1.0, Io), (5.7, 0.0, I)]);
    if ext <= 0.005 {
        return None;
    }
    let reach = kf(t, &[(3.75, 0.0, Lin), (4.3, 1.0, Io), (4.85, 1.0, Io), (5.0, 0.0, O)]);
    let pull = pull(t);
    let (end, bottom, mid) = (s.x + s.w, s.y + s.h, s.y + s.h / 2.0);
    // Gripping under the lip, then up on the digit. The mockup puts the
    // reach 2 and 10 above the lip of its 34 high island; taken from the
    // clock's middle (where the digit is) it lands on the digit at every
    // height.
    let grip = [(d4 - 2.7, bottom + 1.5), (d4 - 3.8, bottom - 2.4)];
    let up = [(d4 - 1.7, (bottom - 2.0).min(mid + 15.0) + pull * 3.0), (d4 - 0.6, mid + 7.0 + pull * 4.4)];
    let to = |a: Pt, b: Pt| (lerp(a.0, b.0, reach), lerp(a.1, b.1, reach));
    let way = [
        root_pt(style, s),
        rim_pt(style, s, 1.0, 0.30, 4.5),
        rim_pt(style, s, 1.0, 0.50, 3.6),
        rim_pt(style, s, 1.0, 0.70, 3.0),
        rim_pt(style, s, 1.0, 0.92, 2.6),
        (end - 13.0, bottom + 3.0),
        (end - 18.0, bottom + 3.4),
        to(grip[0], up[0]),
        to(grip[1], up[1]),
    ];
    let d = catmull_rom(&way, 8);
    let ang = lerp(2.6, 0.9, reach) + ring(t, 4.88, 3.0, 2.4, 4.0) + 0.5 * (TAU * 1.2 * t).sin();
    Some(curl(&along(&d, plen(&d) * ext, 30), ang, 9.0 * (ext / 0.15).min(1.0), 12, 0.0))
}

/// One moment of the story on `stage`, the octopus in the coat of its look.
/// `beat.alpha` eases it home, paced by the wind-down's time (`kept`): over
/// its first 0.58 the arm, the mantle, the glints, the ring and a carried
/// digit fade (the clock's own character coming back as the carried one
/// goes); from 0.3 to 0.88 the slot head comes back out of the dark, and
/// from 0.6 it rises home. Cut short, the story holds where the cut found
/// it, so nothing it had not brought out shows. Exactly at rest at 0 s, at
/// the end and with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, .. } = *stage;
    let a = beat.alpha.clamp(0.0, 1.0);
    let t = beat.cut_at.map_or(beat.t, |c| beat.t.min(c));
    let mut out = Shot::rest(style, s);
    if t <= 0.0 || t >= Story::PenasaranJam.secs() || a == 0.0 {
        return out;
    }
    // The slot and the clock where the renderer puts them.
    let room = stage.room();
    let (Some(slot), Some(clock)) = (room.robot, room.clock) else {
        return out;
    };
    let (play, home) = (kept(a, 0.0, 0.58), kept(a, 0.0, 1.0));
    let (end, mid) = (s.x + s.w, s.y + s.h / 2.0);
    let d4 = clock.x + clock.w / 2.0 + DIGIT_X[PULLED];
    let flush = kf(t, &[(4.85, 0.0, Lin), (4.95, 1.0, O), (5.6, 0.85, Io), (7.2, 0.0, Io)]);

    // The slot head (the sitting octopus) glances at the clock, squashes and
    // sinks into the dark, darkening (fading with its halo) as it goes; it
    // comes back up with a bounce, still flushed, happy.
    let dep = kf(t, &[(1.3, 0.0, Lin), (1.6, 1.0, I), (6.45, 1.0, Io), (6.8, 0.0, Ob)]);
    // Its fade, eased in and out: over the sink, and back over the rise's
    // first 0.25 s (whole by the overshoot). The sink's own ease would take
    // it 0.16 in a frame, the rise's 0.22.
    let fade = kf(t, &[(1.3, 0.0, Lin), (1.6, 1.0, Sio), (6.45, 1.0, Lin), (6.7, 0.0, Sio)]);
    let squash = kf(
        t,
        &[(0.95, 1.0, Lin), (1.25, 0.86, O), (1.4, 1.1, Io), (1.6, 1.0, Io), (6.45, 1.0, Io), (6.65, 1.15, O), (6.82, 0.88, O), (7.1, 1.0, Io)],
    );
    let squash = lerp(1.0, squash, home);
    let glance = kf(t, &[(0.55, 0.0, Lin), (0.75, 1.0, Io), (1.3, 1.0, Io), (1.5, 0.0, Io)]);
    let duck = dep * kept(a, 0.6, 1.0);
    let size = 1.0 - 0.45 * duck;
    out.slot = Slot::Story(MascotShot {
        dx: 2.0 * duck,
        dy: 4.0 * duck,
        sx: size / squash.sqrt(),
        sy: size * squash,
        expr: if (6.8..7.4).contains(&t) { Expr::Happy } else { Expr::Idle },
        glance: (2.4 * glance * home, 0.0),
        blink: blinks(t, &[0.4]) > 0.25,
        flush: flush * home,
        alpha: 1.0 - fade * kept(a, 0.3, 0.88),
        ..MascotShot::REST
    });
    if play <= 0.0 {
        return out;
    }

    // Two eye glints travel through the dark to the right end, and back.
    let (ga, gb) = (slot.x + slot.w / 2.0 + 4.0, end - 5.0);
    let glints = if (1.55..2.75).contains(&t) {
        let u = kf(t, &[(1.6, 0.0, Lin), (2.65, 1.0, Sio)]);
        let fade = ((t - 1.55) / 0.1).clamp(0.0, 1.0) * ((2.75 - t) / 0.12).clamp(0.0, 1.0);
        Some((lerp(ga, gb, u), mid + 5.0 + 0.8 * (TAU * 2.0 * t).sin(), blinks(t, &[2.1]), fade))
    } else if (5.75..6.5).contains(&t) {
        let u = kf(t, &[(5.75, 0.0, Lin), (6.45, 1.0, Sio)]);
        let fade = ((t - 5.75) / 0.1).clamp(0.0, 1.0) * ((6.5 - t) / 0.1).clamp(0.0, 1.0);
        Some((lerp(gb, ga, u), mid + 5.0, 0.0, fade))
    } else {
        None
    };
    if let Some((x, y, shut, fade)) = glints {
        let eye = |dx: f32| Draw::Ellipse { c: (x + dx, y), r: (1.15, 1.7 * (1.0 - 0.85 * shut)), rgb: GLINT_RGB };
        out.parts.push(figure(Layer::Dark, vec![eye(-2.6), eye(2.6)], 0.9 * fade * play));
    }

    // The digit: pulled down and turned while the arm holds it (a prop under
    // the arm, the clock's own character hidden), then let go: plop, it
    // wobbles home in the clock. Cut while carried, the prop fades where it
    // was held and the clock's own comes back at home.
    let pull = pull(t);
    if (4.3..4.85).contains(&t) {
        let draw = Draw::Digit { index: PULLED, at: (d4, mid + 4.5 * pull), rot_deg: 8.0 * pull, home: (d4, mid) };
        out.parts.push(Part { layer: Layer::Inside, draw, alpha: play });
        out.digits[PULLED] = if play < 1.0 {
            DigitFx { scale: 1.0 - play, alpha: 1.0 - play, ..DigitFx::IDENTITY }
        } else {
            DigitFx { alpha: 0.0, ..DigitFx::IDENTITY }
        };
    } else {
        // Its rings have all but died by 6.6 s; this takes what is left to
        // nothing, so the clock is one run again before the end.
        let settle = kf(t, &[(6.6, 1.0, Lin), (7.2, 0.0, Sio)]);
        let fx = DigitFx {
            dy: ring(t, 4.85, -3.2, 3.2, 4.5) * settle,
            rot_deg: ring(t, 4.85, -10.0, 2.6, 4.0) * settle,
            ..DigitFx::IDENTITY
        };
        out.digits[PULLED] = toward_rest(fx, play);
    }
    let p = (t - 4.86) / 0.4;
    if p > 0.0 && p < 1.0 {
        let path = circle_segs((d4, mid + 1.0), 1.5 + 6.0 * O.at(p));
        let draw = Draw::Stroke { path, rgb: CREAM, width: 1.1 * (1.0 - p) };
        out.parts.push(Part { layer: Layer::Front, draw, alpha: (1.0 - p) * play });
    }

    // The arm, drawn twice: behind the island (cut to outside it), and the
    // same arm on the island's face, cut short of the right end so its root
    // stays behind. The mantle peeks out from behind the end with three
    // short arms, fading in and out over the first and last 0.15 s it is
    // drawn, while it is still behind the end.
    let Some(line) = arm_line(style, s, d4, t) else {
        return out;
    };
    let long = arm(&line, look.octopus, 1, 1.0, flush);
    out.parts.push(figure(Layer::Back, long.clone(), play));
    if t > 3.2 && t < 5.55 {
        let show = ((t - 3.2) / 0.15).min(1.0) * ((5.55 - t) / 0.15).min(1.0);
        let (hid, peek, pop) = ((end - 8.0, mid - 1.0), (end + 13.6, mid + 2.0), (end + 15.5, mid + 0.5));
        let at = kf2(t, [(3.2, hid, Lin), (3.75, peek, Ob), (4.85, peek, Io), (4.95, pop, O), (5.05, pop, Io), (5.5, hid, I)]);
        let turn = kf(t, &[(3.2, -5.0, Lin), (3.75, -18.0, Ob), (4.85, -18.0, Io), (4.95, -4.0, O), (5.4, -10.0, Io)])
            + 3.0 * (TAU * 0.9 * t).sin();
        let sq = kf(t, &[(4.85, 1.0, Lin), (4.95, 1.18, O), (5.15, 0.92, Io), (5.3, 1.0, Io)]);
        let look_x = kf(t, &[(3.75, 0.0, Lin), (3.95, -1.0, Io), (4.85, -1.0, Io), (4.95, 0.0, Io)]);
        let splay = kf(t, &[(4.85, 0.0, Lin), (4.95, 1.0, O), (5.3, 0.4, Io)]);
        let mut short = Vec::new();
        for (i, base, len) in [(3, 100.0, 13.0), (4, 78.0, 14.0), (5, 60.0, 11.0)] {
            let r0 = root(at, i, turn, 1.0, 1.0);
            let sway = 10.0 * (TAU * 0.8 * t + i as f32).sin();
            let a0 = (base + turn * 0.5 + sway - splay * 35.0).to_radians();
            let line = chain(r0.0, r0.1, a0, len, 16, |u| (-120.0 * u - splay * 80.0 * u).to_radians());
            short.extend(arm(&line, look.octopus, 1, 0.9, flush));
        }
        out.parts.push(figure(Layer::Back, short, play * show));
        let face = MascotShot {
            expr: if (4.85..5.6).contains(&t) { Expr::Alert } else { Expr::Idle },
            glance: (2.4 * look_x, -0.3 * 2.2),
            blink: blinks(t, &[4.2]) > 0.25,
            flush,
            ..MascotShot::REST
        };
        out.parts.push(Part { layer: Layer::Back, draw: mantle(at, turn, 1.0 / sq.sqrt(), sq, &face), alpha: play * show });
    }
    let cut = Rect { x: s.x - 5.0, y: s.y - 5.0, w: s.w - 7.0, h: s.h + 20.0 };
    out.parts.push(figure(Layer::Wrap(Some(cut)), long, play));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Mascot, OctopusColor};
    use crate::ui::cat_style::CatLook;
    use crate::ui::mascot::{FLUSH_RGB, mix, octopus_rgb};
    use crate::ui::play::state::Beat;
    use crate::ui::play::stories::{CONTRACT_SIZES, Draw, Layer, Part, Slot, rim_pt, wind_down_frames};
    use crate::ui::play::testing::shape;
    use crate::ui::play::trigger::Story;
    use crate::ui::render::DigitFx;
    use crate::ui::robot::Expr;

    crate::ui::play::stories::story_contract_tests!(Story::PenasaranJam, shot);

    /// The idle island `w` x `h` with the octopus (coat `coat`) and the clock.
    fn stage(style: Style, w: f32, h: f32, beat: Beat, coat: OctopusColor) -> Stage {
        let look = CatLook { mascot: Mascot::Octopus, octopus: coat, ..CatLook::default() };
        Stage::idle(style, shape(style, w, h), beat, look, true)
    }

    /// At `t`, wound down to `alpha`, the default coat.
    fn at(style: Style, w: f32, h: f32, t: f32, alpha: f32) -> Shot {
        shot(&stage(style, w, h, Beat { spread: 1.0, alpha, t, cut_at: None }, OctopusColor::default()))
    }

    /// The pulled digit's centre at rest: the clock box's centre plus the
    /// mockup's offset of the minute's last character.
    fn home(style: Style, w: f32, h: f32) -> Pt {
        let st = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t: 0.0, cut_at: None }, OctopusColor::default());
        let c = st.room().clock.unwrap();
        (c.x + c.w / 2.0 + DIGIT_X[PULLED], st.shape.y + st.shape.h / 2.0)
    }

    /// The digit props of a shot: index, centre, turn, alpha, layer.
    fn props(shot: &Shot) -> Vec<(usize, Pt, f32, f32, Layer)> {
        shot.parts
            .iter()
            .filter_map(|p| match p.draw {
                Draw::Digit { index, at, rot_deg, .. } => Some((index, at, rot_deg, p.alpha, p.layer)),
                _ => None,
            })
            .collect()
    }

    /// How much of the pulled digit shows: the clock's own character and
    /// its prop together.
    fn shown(shot: &Shot) -> f32 {
        shot.digits[4].alpha + props(shot).iter().map(|p| p.3).sum::<f32>()
    }

    #[test]
    fn the_pulled_digit_is_drawn_once_at_any_moment() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                let case = format!("{style:?} {w}x{h}");
                let mut t = 0.0;
                while t <= Story::PenasaranJam.secs() {
                    let shot = at(style, w, h, t, 1.0);
                    let p = props(&shot);
                    assert!(p.len() <= 1 && p.iter().all(|d| d.0 == 4), "{case} t={t}: {p:?}");
                    assert!((shown(&shot) - 1.0).abs() < 1e-4, "{case} t={t}: shows {}", shown(&shot));
                    // The prop and the clock's character never both show.
                    if p.len() == 1 {
                        assert_eq!(shot.digits[4].alpha, 0.0, "{case} t={t}");
                    }
                    // The other characters never move.
                    assert!(shot.digits.iter().enumerate().all(|(i, d)| i == 4 || *d == DigitFx::IDENTITY), "{case} t={t}");
                    t += 0.01;
                }
                // Carried while the arm pulls it: down and turned, in the
                // clock's place under the arm on the island's face.
                let (x, y) = home(style, w, h);
                let pulled = props(&at(style, w, h, 4.7, 1.0));
                assert_eq!(pulled.len(), 1, "{case}");
                let (_, c, turn, alpha, layer) = pulled[0];
                assert_eq!((layer, alpha), (Layer::Inside, 1.0), "{case}");
                assert!((c.0 - x).abs() < 1e-3 && c.1 > y + 2.0 && c.1 < y + 4.6 && turn > 3.0, "{case}: {c:?} {turn}");
                // Taken up from its home, where the renderer places it from
                // the clock's own character: no jump as the arm takes it.
                for t in [4.301, 4.7] {
                    let shot = at(style, w, h, t, 1.0);
                    let Some(Draw::Digit { home: from, at: now, .. }) = shot.parts.iter().map(|p| &p.draw).find(|d| matches!(d, Draw::Digit { .. })) else {
                        panic!("{case} t={t}: no prop");
                    };
                    assert!((from.0 - x).abs() < 1e-3 && (from.1 - y).abs() < 1e-3, "{case} t={t}: {from:?}");
                    if t < 4.31 {
                        assert!((now.0 - x).abs() < 1e-3 && (now.1 - y).abs() < 0.01, "{case} t={t}: {now:?}");
                    }
                }
                // Not before the pull or after the plop; then it wobbles home
                // in the clock.
                for t in [4.2, 4.9, 5.2] {
                    assert!(props(&at(style, w, h, t, 1.0)).is_empty(), "{case} t={t}");
                }
                let wobble = at(style, w, h, 4.92, 1.0).digits[4];
                assert!(wobble.dy.abs() > 0.5 && wobble.rot_deg.abs() > 1.0 && wobble.alpha == 1.0, "{case}: {wobble:?}");
                assert_eq!(at(style, w, h, 7.3, 1.0).digits[4], DigitFx::IDENTITY, "{case}: settled before the end");
            }
            // Cut while it is carried: the prop fades where it was held (in
            // the clock's place, which goes home with the island) and the
            // clock's own character comes back at home, the two always adding
            // up to one digit.
            for cut in [4.4, 4.7] {
                let frames = wind_down_frames(Story::PenasaranJam, cut, 0.0, |beat| shot(&stage(style, 190.0, 34.0, beat, OctopusColor::default())));
                let held = props(&frames[0].2)[0].1;
                for (ms, beat, shot) in &frames {
                    let case = format!("{style:?} cut {cut} +{ms:.1} ms (alpha {})", beat.alpha);
                    assert!((shown(shot) - 1.0).abs() < 1e-4, "{case}: shows {}", shown(shot));
                    let c = stage(style, 190.0, 34.0, *beat, OctopusColor::default()).room().clock.unwrap();
                    for p in props(shot) {
                        let x = c.x + c.w / 2.0 + DIGIT_X[PULLED];
                        assert!((p.1.0 - x).abs() < 1e-3 && p.1.1 == held.1, "{case}: the prop at {:?}, the digit's place {x}", p.1);
                    }
                    let d = shot.digits[4];
                    assert_eq!((d.dx, d.dy, d.rot_deg), (0.0, 0.0, 0.0), "{case}: the clock's own stays home");
                }
            }
        }
    }

    /// The shortest distance from `p` to the polyline `line`.
    fn off_line(line: &[Pt], p: Pt) -> f32 {
        line.windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len2 = (dx * dx + dy * dy).max(1e-9);
                let u = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
                (a.0 + u * dx - p.0).hypot(a.1 + u * dy - p.1)
            })
            .fold(f32::MAX, f32::min)
    }

    /// The parts of `layer` that are figures (the arms), as drawn.
    fn figures(shot: &Shot, pass: fn(&Layer) -> bool) -> Vec<&Part> {
        shot.parts.iter().filter(|p| pass(&p.layer) && matches!(p.draw, Draw::Group { .. })).collect()
    }

    #[test]
    fn the_arm_wraps_the_right_end_on_both_styles() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                let case = format!("{style:?} {w}x{h}");
                let s = shape(style, w, h);
                let (d4, mid) = home(style, w, h);
                let r = crate::ui::geometry::corner_radius(style, s);
                for t in [3.4, 4.0, 4.6] {
                    let line = arm_line(style, s, d4, t).unwrap_or_else(|| panic!("{case} t={t}: no arm"));
                    // Round the right end, just outside its outline: the
                    // Notch's straight side and corner, the Pill's half circle.
                    for (u, off) in [(0.30, 4.5), (0.50, 3.6), (0.70, 3.0), (0.92, 2.6)] {
                        let p = rim_pt(style, s, 1.0, u, off);
                        let d = off_line(&line, p);
                        // The line is resampled every few dip, so it cuts the
                        // curve by a little.
                        assert!(d < 0.8, "{case} t={t}: {d} from the rim at u {u}");
                    }
                    // Its root stays behind the island, clear of its edge by
                    // more than half the arm's width at the root (4.3).
                    let root = line[0];
                    let inside = if style == Style::Pill {
                        let c = (s.x + s.w - r, mid);
                        root.0 <= c.0 || r - (root.0 - c.0).hypot(root.1 - c.1) >= 2.5
                    } else {
                        s.x + s.w - root.0 >= 2.5 && root.1 > s.y + 2.5 && root.1 < s.y + s.h - r
                    };
                    assert!(inside, "{case} t={t}: the root {root:?} shows");
                }
                // Reaching in, the tip ends on the digit.
                let line = arm_line(style, s, d4, 4.6).unwrap();
                let tip = *line.last().unwrap();
                assert!((tip.0 - d4).abs() < 10.0 && (tip.1 - mid).abs() < 12.0, "{case}: the tip at {tip:?}, the digit at {d4}");
                // Drawn twice: behind the island, and the same arm on its face
                // cut short of the right end so its root stays behind.
                let shot = at(style, w, h, 4.6, 1.0);
                let back = figures(&shot, |l| *l == Layer::Back);
                let wrap = figures(&shot, |l| matches!(l, Layer::Wrap(_)));
                assert_eq!(wrap.len(), 1, "{case}");
                assert_eq!(back[0].draw, wrap[0].draw, "{case}: the same arm");
                let Layer::Wrap(Some(cut)) = wrap[0].layer else { panic!("{case}: {:?}", wrap[0].layer) };
                assert!((cut.x + cut.w - (s.x + s.w - 12.0)).abs() < 1e-3, "{case}: {cut:?}");
                assert!(cut.x < s.x && cut.y < s.y && cut.y + cut.h > s.y + s.h, "{case}: {cut:?}");
                // In and out of the story, no arm.
                assert!(arm_line(style, s, d4, 2.5).is_none() && arm_line(style, s, d4, 5.75).is_none(), "{case}");
            }
        }
    }

    /// The slot head's alpha, the peeking mantle's (0 when it is not
    /// drawn), the glints' and the arm's length.
    fn figure_alphas(style: Style, w: f32, h: f32, t: f32) -> (f32, f32, f32, f32) {
        let shot = at(style, w, h, t, 1.0);
        let slot = match shot.slot {
            Slot::Story(m) => m.alpha,
            Slot::App => 1.0,
            Slot::Empty => 0.0,
        };
        let mantle = shot.parts.iter().find(|p| matches!(p.draw, Draw::Head { .. })).map_or(0.0, |p| p.alpha);
        let glints = shot.parts.iter().find(|p| p.layer == Layer::Dark).map_or(0.0, |p| p.alpha);
        let (d4, _) = home(style, w, h);
        let arm = arm_line(style, shape(style, w, h), d4, t).map_or(0.0, |l| crate::ui::play::art::tube::plen(&l));
        (slot, mantle, glints, arm)
    }

    #[test]
    fn the_octopus_hands_over_from_its_slot_without_a_pop() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                let case = format!("{style:?} {w}x{h}");
                let mut last = figure_alphas(style, w, h, 0.0);
                let mut t = 0.01;
                while t <= Story::PenasaranJam.secs() {
                    let now = figure_alphas(style, w, h, t);
                    // Never two octopuses: the slot head is in the dark while
                    // the mantle peeks round the end.
                    assert!(now.0 == 0.0 || now.1 == 0.0, "{case} t={t}: {now:?}");
                    // Every figure fades in and out over frames, the arm grows.
                    // The slot head comes up out of the dark at the mockup's
                    // pace (its overshooting return, 0.35 s), the fastest.
                    assert!((now.0 - last.0).abs() <= 0.15 && (now.1 - last.1).abs() <= 0.1 && (now.2 - last.2).abs() <= 0.1, "{case} t={t}: {last:?} to {now:?}");
                    // The arm comes out and goes back in at the mockup's pace.
                    assert!((now.3 - last.3).abs() <= 8.0, "{case} t={t}: the arm from {} to {}", last.3, now.3);
                    last = now;
                    t += 0.01;
                }
                // It dives, the mantle peeks, it comes home.
                assert_eq!(figure_alphas(style, w, h, 2.0).0, 0.0, "{case}");
                assert_eq!(figure_alphas(style, w, h, 4.3).1, 1.0, "{case}");
                assert_eq!(figure_alphas(style, w, h, 7.0).0, 1.0, "{case}");
            }
        }
    }

    #[test]
    fn the_flush_reaches_the_mantle_and_the_arms_in_every_coat() {
        fn bodies(d: &Draw, out: &mut Vec<u32>) {
            match d {
                Draw::Fill { rgb, .. } => out.push(*rgb),
                Draw::Group { items, .. } => items.iter().for_each(|i| bodies(i, out)),
                _ => {}
            }
        }
        for coat in OctopusColor::ALL {
            for (t, flushed) in [(4.4, false), (5.0, true)] {
                let shot = shot(&stage(Style::Pill, 190.0, 34.0, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, *coat));
                let Some(Draw::Head { shot: m, .. }) = shot.parts.iter().map(|p| &p.draw).find(|d| matches!(d, Draw::Head { .. })) else {
                    panic!("{coat:?} t={t}: no mantle");
                };
                assert!(m.bare_octopus, "{coat:?} t={t}: the mantle alone");
                assert!(if flushed { m.flush > 0.9 } else { m.flush == 0.0 }, "{coat:?} t={t}: {}", m.flush);
                let want = mix(octopus_rgb(*coat), FLUSH_RGB, m.flush);
                for p in figures(&shot, |l| *l == Layer::Back) {
                    let mut rgb = Vec::new();
                    bodies(&p.draw, &mut rgb);
                    assert!(!rgb.is_empty() && rgb.iter().all(|c| *c == want), "{coat:?} t={t}: {rgb:x?}, want {want:x}");
                }
            }
        }
    }

    #[test]
    fn the_faces_are_the_mockups_in_the_apps_expressions() {
        let slot = |t: f32| match at(Style::Notch, 190.0, 34.0, t, 1.0).slot {
            Slot::Story(m) => m,
            other => panic!("t={t}: {other:?}"),
        };
        let mantle = |t: f32| {
            at(Style::Notch, 190.0, 34.0, t, 1.0)
                .parts
                .iter()
                .find_map(|p| match p.draw {
                    Draw::Head { shot, .. } => Some(shot),
                    _ => None,
                })
                .unwrap()
        };
        // The glance at the clock from the slot; home happy.
        assert!(slot(1.0).glance.0 > 2.0, "looks right at the clock");
        assert!(slot(0.4).blink && !slot(0.6).blink);
        assert_eq!(slot(7.0).expr, Expr::Happy);
        assert_eq!(slot(7.5).expr, Expr::Idle);
        // Peeking it looks left at the clock; the plop startles it.
        assert!(mantle(4.5).glance.0 < -2.0 && mantle(4.5).expr == Expr::Idle);
        assert_eq!(mantle(5.2).expr, Expr::Alert);
        assert!(mantle(4.2).blink);
    }

    #[test]
    fn the_plop_ring_grows_and_fades_over_the_digit() {
        let ring = |t: f32| {
            at(Style::Notch, 190.0, 34.0, t, 1.0).parts.into_iter().find(|p| matches!(p.draw, Draw::Stroke { .. }))
        };
        assert!(ring(4.85).is_none() && ring(5.27).is_none());
        let (a, b) = (ring(4.95).unwrap(), ring(5.15).unwrap());
        assert_eq!(a.layer, Layer::Front);
        let (Draw::Stroke { width: wa, .. }, Draw::Stroke { width: wb, .. }) = (&a.draw, &b.draw) else { unreachable!() };
        assert!(b.draw.bounds().w > a.draw.bounds().w && b.alpha < a.alpha && wb < wa, "{a:?} {b:?}");
        let (d4, mid) = home(Style::Notch, 190.0, 34.0);
        let c = b.draw.bounds();
        assert!((c.x + c.w / 2.0 - d4).abs() < 1e-3 && (c.y + c.h / 2.0 - mid - 1.0).abs() < 1e-3, "{c:?}");
    }
}
