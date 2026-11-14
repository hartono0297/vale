//! The octopus's "Bungee": in its slot it shuffles to the lip, looks down
//! and gathers itself, then jumps; one arm stays hooked on the lip under the
//! slot and stretches like a rope (the longer, the thinner) while the others
//! stream up behind the fall; it bounces on the rope (squashed at the bottom,
//! stretched on the way up), throws every arm wide ("ta-da"), crouches a
//! little, is pulled back up into the slot, squashes and stands. A floating
//! pill is dragged down by the weight on the rope and springs back; a notch,
//! welded to the screen's edge, does nothing. Timeline, easing and offsets
//! from `docs/design/maskot-notch-v3.html`, story `o3`.

use std::f32::consts::{PI, TAU};

use super::{Draw, Ease, HEAD_PIVOT, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, island_box, kept, kf, lerp, ring};
use crate::config::Style;
use crate::ui::play::art::octopus::{MANTLE_PIVOT, arm, mantle, root};
use crate::ui::play::art::tube::{catmull_rom, chain, curl, plen};
use crate::ui::play::shape::{Deform, rigid};
use crate::ui::play::trigger::Story;
use crate::ui::robot::{Expr, GRID};

use Ease::{I, Io, Lin, O, Ob, Sio};

/// The figure (the mantle and its long arms, in front) fades in over the
/// slot head from `TAKE` and has taken over when it jumps at `JUMP`.
const TAKE: f32 = 0.88;
const JUMP: f32 = 1.0;
/// Pulled back up, the figure is in the slot at `HOME` and has faded out over
/// the slot head by `GONE`.
const HOME: f32 = 5.6;
const GONE: f32 = 5.72;
/// The bounce on the rope starts at the bottom of the drop.
const CATCH: f32 = 1.5;
/// The rope reverses the fall at the catch in one instant: what the speed
/// drives (the free arms streaming up, the stretch) swings over from the
/// fall's end to the bounce over this long, never flipping in a frame.
const SETTLE: f32 = 0.25;
/// How squashed the slot head gathers itself before the jump, and how
/// stretched it is pulled back in.
const CROUCH: f32 = 0.86;
const PULLED: f32 = 1.25;
/// The holding arm's length before it stretches, dip.
const ROPE: f32 = 22.0;
/// The free arms' spread, left to right (the mockup's `O`).
const SPREAD: [f32; 5] = [-1.0, -0.45, 0.15, 0.6, 1.0];

/// The holding arm's width, as a share of an arm's, at `stretch` (its
/// length over `ROPE`): thinner the more it stretches, never under 0.7.
fn rope_width(stretch: f32) -> f32 {
    (1.0 / stretch.max(1.0).powf(0.3)).clamp(0.7, 1.0)
}

/// The slot head at `t`: it shuffles to the slot's lip looking down and
/// gathers itself for the jump; pulled back in, it lands stretched, squashes,
/// overshoots and stands, happy. At rest while the figure is out.
fn slot_pose(t: f32) -> MascotShot {
    let blink = blinks(t, &[0.35, 7.0]) > 0.25;
    if t <= JUMP {
        let shuffle = kf(t, &[(0.5, 0.0, Lin), (0.85, 1.0, Io)]);
        let sy = kf(t, &[(0.7, 1.0, Lin), (0.95, CROUCH, O)]);
        return MascotShot { dy: 2.0 * shuffle, sx: 1.0 / sy.sqrt(), sy, glance: (0.0, 2.2 * shuffle), blink, ..MascotShot::REST };
    }
    if t < HOME {
        return MascotShot::REST;
    }
    let sy = kf(t, &[(5.62, PULLED, Lin), (5.75, 0.82, O), (6.1, 1.05, Io), (6.3, 1.0, Io)]);
    let expr = if (6.0..6.8).contains(&t) { Expr::Happy } else { Expr::Idle };
    MascotShot { dy: ring(t, 5.62, -2.5, 2.5, 5.0), sx: 1.0 / sy.sqrt(), sy, expr, blink, ..MascotShot::REST }
}

/// `m` eased toward the head at rest by `k` (1 leaves it, 0 is rest).
fn toward_rest(m: MascotShot, k: f32) -> MascotShot {
    let r = MascotShot::REST;
    MascotShot {
        dy: m.dy * k,
        sx: lerp(r.sx, m.sx, k),
        sy: lerp(r.sy, m.sy, k),
        glance: (m.glance.0 * k, m.glance.1 * k),
        expr: if k > 0.5 { m.expr } else { r.expr },
        blink: m.blink && k > 0.5,
        ..m
    }
}

/// Where the slot head's mantle (grid 32, 30, the point the figure is placed
/// by) is with pose `m`, the slot's pivot at `pivot` and `k` dip per grid
/// unit.
fn mantle_of(pivot: (f32, f32), k: f32, m: &MascotShot) -> (f32, f32) {
    (pivot.0 + m.dx, pivot.1 + m.dy - (HEAD_PIVOT.1 - MANTLE_PIVOT.1) * k * m.sy)
}

/// The figure's mantle height out of the slot (the mockup's `yAt`): from
/// `from`, where it jumps, it drops 50 below the lip `bottom`, bounces on the
/// rope about 38 below it, crouches 4 lower and is pulled up to `to`, where
/// it lands in the slot.
fn drop_y(u: f32, from: f32, to: f32, bottom: f32) -> f32 {
    if u < JUMP {
        return from;
    }
    if u < CATCH {
        let k = (u - JUMP) / (CATCH - JUMP);
        return lerp(from, bottom + 50.0, I.at(k) * 0.85 + 0.15 * k);
    }
    if u < 4.85 {
        let tau = u - CATCH;
        return bottom + 38.0 + 12.0 * (-2.2 * tau).exp() * (TAU * 1.6 * tau).cos();
    }
    if u < 5.05 {
        return bottom + 38.0 + kf(u, &[(4.85, 0.0, Lin), (5.05, 4.0, Io)]);
    }
    kf(u, &[(5.05, bottom + 42.0, Lin), (HOME, to, I)])
}

/// One moment of the story on `stage`, the octopus in the coat of its look.
/// `beat.alpha` eases it home, paced by the wind-down's time (`kept`): over
/// its first 0.58 the figure fades where it is, the slot head fades back in
/// at rest and the pill springs back. Cut short, the story holds where the
/// cut found it. Exactly at rest at 0 s, at the end and with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, .. } = *stage;
    let a = beat.alpha.clamp(0.0, 1.0);
    let t = beat.cut_at.map_or(beat.t, |c| beat.t.min(c));
    let mut out = Shot::rest(style, s);
    if t <= 0.0 || t >= Story::Bungee.secs() || a == 0.0 {
        return out;
    }
    // The slot where the renderer puts it (the clock on or off moves it).
    let Some(slot) = stage.room().robot else {
        return out;
    };
    let play = kept(a, 0.0, 0.58);
    let k = slot.w / GRID;
    let pivot = (slot.x + slot.w / 2.0, slot.y + HEAD_PIVOT.1 * k);
    let bottom = s.y + s.h;

    // The slot head, and the figure crossing over it where they hand over.
    let pose = toward_rest(slot_pose(t), play);
    let show = kf(t, &[(TAKE, 0.0, Lin), (JUMP, 1.0, Lin), (HOME, 1.0, Lin), (GONE, 0.0, Lin)]) * play;
    out.slot = if show < 1.0 { Slot::Story(MascotShot { alpha: 1.0 - show, ..pose }) } else { Slot::Empty };
    if show <= 0.0 {
        return out;
    }

    // Where the figure's mantle is: on the slot head's while they hand over,
    // else on the drop. It sways on the rope (eased out before it is pulled up).
    let out_of_slot = (JUMP..HOME).contains(&t);
    let (from, to) = (mantle_of(pivot, k, &slot_pose(JUMP)).1, mantle_of(pivot, k, &slot_pose(HOME)).1);
    let y_at = |u: f32| if (JUMP..HOME).contains(&u) { drop_y(u, from, to, bottom) } else { mantle_of(pivot, k, &toward_rest(slot_pose(u), play)).1 };
    let tau = t - CATCH;
    let sway = if t > CATCH { kf(t, &[(4.6, 1.0, Lin), (5.0, 0.0, Sio)]) } else { 0.0 };
    let swing = (PI * tau).sin() * sway;
    // Falling (> 0) or rising, dip per second; frozen as it lands, so the
    // arms keep their tuck while the figure fades.
    let speed = |u: f32| (y_at(u) - y_at(u - 0.03)) / 0.03;
    let v = speed(t.min(HOME - 1e-3));
    let fall_end = speed(CATCH - 1e-3);
    let settle = Sio.at(((t - CATCH) / SETTLE).clamp(0.0, 1.0));
    let caught = |live: f32, fall: f32| if t >= CATCH { lerp(fall, live, settle) } else { live };
    let tada = kf(t, &[(3.6, 0.0, Lin), (3.9, 1.0, Ob), (4.6, 1.0, Lin), (4.85, 0.2, Io)]);
    let (m_at, mr, msx, msy) = if out_of_slot {
        // The first impact's squash comes on over 50 ms, with the arms.
        let attack = kf(t, &[(CATCH, 0.0, Lin), (CATCH + 0.05, 1.0, Lin)]);
        let squashed = if t > CATCH && t < 4.85 { (-2.2 * tau).exp() * (TAU * 1.6 * tau).cos().max(0.0).powi(6) * attack } else { 0.0 };
        let pose_sy = kf(t, &[(3.6, 0.0, Lin), (3.9, 0.08, Ob), (4.6, 0.08, Lin), (4.85, -0.1, Io), (5.05, 0.0, Io)]);
        // The crouch it jumped from lets go over its first 0.12 s.
        let release = kf(t, &[(JUMP, CROUCH - 1.0, Lin), (JUMP + 0.12, 0.0, O)]);
        let stretch = |v: f32| (v.abs() / 260.0).clamp(0.0, 0.28);
        let msy = 1.0 + caught(stretch(speed(t)), stretch(fall_end)) - squashed * 0.3 + pose_sy + release;
        let mr = 6.0 * swing * (-0.3 * tau).exp();
        ((pivot.0 + 3.0 * swing, drop_y(t, from, to, bottom)), mr, 1.0 / msy.sqrt(), msy)
    } else {
        (mantle_of(pivot, k, &pose), 0.0, pose.sx, pose.sy)
    };

    // The rope: from its root, out and down under the lip, then up to the lip
    // below the slot, its tip curled over it. Taut, it drags a floating pill
    // down by its tension x 1.4 dip, one damped spring on top.
    let rope = |lip: (f32, f32)| {
        let r0 = root(m_at, 0, mr, 1.0, msy);
        let low = (lip.1 + 6.0).max(lerp(lip.1 + 6.0, m_at.1 - 6.0, 0.5));
        catmull_rom(&[r0, (r0.0 - 5.0, r0.1 - 3.0), (lip.0 - 1.5, low), (lip.0, lip.1 + 3.0), lip], 8)
    };
    let lip = (pivot.0 + 2.0, bottom + 0.4);
    if style == Style::Pill && out_of_slot {
        let tension = ((plen(&rope(lip)) / ROPE - 1.0) / 2.2).clamp(0.0, 1.0);
        out.deform = Deform { shift: (0.0, (tension * 1.4 + ring(t, CATCH, -1.6, 1.6, 2.2)) * play), ..Deform::default() };
        out.island = island_box(style, s, &out.deform);
    }
    // The tip holds the lip as the island moves.
    let m = rigid(style, s, &out.deform);
    let line = rope((lip.0 * m.M11 + lip.1 * m.M21 + m.M31, lip.0 * m.M12 + lip.1 * m.M22 + m.M32));
    let hold = arm(&curl(&line, 3.2, 7.0, 10, 0.0), look.octopus, 1, rope_width(plen(&line) / ROPE), 0.0);

    // The free arms: streaming up behind a fall, tucked while it rises, wide
    // open for the ta-da.
    let streaming = |v: f32| (v / 90.0).clamp(-1.0, 1.0);
    let up = caught(streaming(v), streaming(fall_end));
    let mut arms = Vec::new();
    for (n, o) in SPREAD.iter().enumerate() {
        let i = n + 1;
        let r = root(m_at, i, mr, 1.0, msy);
        let side = if *o < 0.0 { -1.0 } else { 1.0 };
        let ang = 90.0 - o * 38.0 + mr * 0.6 + 7.0 * (TAU * 1.3 * t + i as f32 * 1.7).sin()
            - side * up.clamp(0.0, 1.0) * 125.0
            + side * (-up).clamp(0.0, 1.0) * 12.0
            - o * tada * 62.0;
        let bend = -side * lerp(110.0, 30.0, up.clamp(0.0, 1.0)) - side * tada * 40.0;
        let line = chain(r.0, r.1, ang.to_radians(), lerp(13.0, 15.5, tada), 18, |q| (bend * q.powf(1.5) * 2.0).to_radians());
        arms.extend(arm(&line, look.octopus, -side as i8, 0.95, 0.0));
    }

    // The face in the app's own expressions: the mockup's round eyes are
    // Alert (looking up as it jumps), its arcs Happy.
    let face = if out_of_slot {
        let jumped = t < CATCH;
        let expr = if jumped { Expr::Alert } else if (3.65..4.7).contains(&t) { Expr::Happy } else { Expr::Idle };
        MascotShot { expr, glance: (0.0, if jumped { -2.2 } else { 0.0 }), blink: blinks(t, &[2.9]) > 0.25, ..MascotShot::REST }
    } else {
        pose
    };
    let group = |items: Vec<Draw>| Part {
        layer: Layer::Front,
        draw: Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items },
        alpha: show,
    };
    out.parts.push(group(arms));
    out.parts.push(group(hold));
    out.parts.push(Part { layer: Layer::Front, draw: mantle(m_at, mr, msx, msy, &face), alpha: show });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::theme::Mascot;
    use crate::ui::cat_style::CatLook;
    use crate::ui::play::shape::Deform;
    use crate::ui::play::state::Beat;
    use crate::ui::play::stories::{Draw, Layer, MascotShot, Part, Slot};
    use crate::ui::play::testing::shape;
    use crate::ui::play::trigger::Story;
    use crate::ui::render::DigitFx;
    use crate::ui::robot::{GRID, Seg};

    crate::ui::play::stories::story_contract_tests!(Story::Bungee, shot);

    const SIZES: [(f32, f32); 3] = crate::ui::play::stories::CONTRACT_SIZES;

    /// The idle island `w` x `h` with the octopus, the clock on or off.
    fn stage(style: Style, w: f32, h: f32, t: f32, clock: bool) -> Stage {
        let look = CatLook { mascot: Mascot::Octopus, ..CatLook::default() };
        Stage::idle(style, shape(style, w, h), Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, look, clock)
    }

    /// At `t`, playing, with the clock.
    fn at(style: Style, w: f32, h: f32, t: f32) -> Shot {
        shot(&stage(style, w, h, t, true))
    }

    fn front(shot: &Shot) -> Vec<&Part> {
        shot.parts.iter().filter(|p| p.layer == Layer::Front).collect()
    }

    /// The figure's mantle: its pivot, its shot and its alpha.
    fn figure_head(shot: &Shot) -> Option<((f32, f32), MascotShot, f32)> {
        front(shot).into_iter().find_map(|p| match p.draw {
            Draw::Head { at, shot } => Some((at, shot, p.alpha)),
            _ => None,
        })
    }

    /// How wide the holding arm is where it leaves the mantle: its body's
    /// outline starts on one side of the root and ends on the other.
    fn rope_root_width(shot: &Shot) -> f32 {
        let groups: Vec<_> = front(shot).into_iter().filter(|p| matches!(p.draw, Draw::Group { .. })).collect();
        assert_eq!(groups.len(), 2, "the free arms and the holding arm: {:?}", shot.parts);
        let Draw::Group { items, .. } = &groups[1].draw else { unreachable!() };
        let Some(Draw::Fill { path, .. }) = items.first() else { panic!("no body: {items:?}") };
        let Seg::Move(x0, y0) = path[0] else { panic!("{path:?}") };
        let Some(Seg::Line(x1, y1)) = path.iter().rev().find(|s| !matches!(s, Seg::Close)) else { panic!("{path:?}") };
        (x1 - x0).hypot(y1 - y0)
    }

    /// Each free arm's heading, its root to its tip (radians), as drawn: the
    /// body's outline starts and ends on the two sides of the root and turns
    /// round the tip with its one cubic.
    fn free_arm_headings(shot: &Shot) -> Vec<f32> {
        let Some(p) = front(shot).into_iter().find(|p| matches!(p.draw, Draw::Group { .. })) else { return Vec::new() };
        let Draw::Group { items, .. } = &p.draw else { unreachable!() };
        items
            .iter()
            .filter_map(|d| {
                let Draw::Fill { path, .. } = d else { return None };
                let Seg::Move(x0, y0) = path[0] else { return None };
                let Some(&Seg::Line(x1, y1)) = path.iter().rev().find(|s| !matches!(s, Seg::Close)) else { return None };
                let Some(&Seg::Cubic(.., tx, ty)) = path.iter().find(|s| matches!(s, Seg::Cubic(..))) else { return None };
                Some((ty - (y0 + y1) / 2.0).atan2(tx - (x0 + x1) / 2.0))
            })
            .collect()
    }

    #[test]
    fn the_free_arms_never_flip_from_one_frame_to_the_next() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let (mut last, mut t, mut seen) = (Vec::new(), 0.0, 0);
                while t < Story::Bungee.secs() {
                    let now = free_arm_headings(&at(style, w, h, t));
                    if now.len() == 5 && last.len() == 5 {
                        seen += 1;
                        for (i, (a, b)) in last.iter().zip(&now).enumerate() {
                            let turn = ((b - a + PI).rem_euclid(TAU) - PI).to_degrees();
                            assert!(turn.abs() <= 60.0, "{case} t={t}: arm {i} turned {turn} degrees in a frame");
                        }
                    }
                    last = now;
                    t += 1.0 / 60.0;
                }
                assert!(seen > 250, "{case}: {seen} frames with the arms");
            }
        }
    }

    #[test]
    fn only_the_pill_is_pulled_down() {
        for (w, h) in SIZES {
            for clock in [true, false] {
                let case = format!("{w}x{h} clock={clock}");
                let (mut t, mut deepest) = (0.0, 0.0_f32);
                while t <= Story::Bungee.secs() {
                    // The Notch hangs from the screen's edge: nothing moves it.
                    let notch = shot(&stage(Style::Notch, w, h, t, clock));
                    assert_eq!(notch.deform, Deform::default(), "{case} t={t}");
                    // The Pill only shifts, straight down (or springing back up).
                    let pill = shot(&stage(Style::Pill, w, h, t, clock));
                    let dy = pill.deform.shift.1;
                    assert_eq!(Deform { shift: (0.0, 0.0), ..pill.deform.clone() }, Deform::default(), "{case} t={t}");
                    assert_eq!(pill.deform.shift.0, 0.0, "{case} t={t}");
                    assert!(dy.abs() <= 1.4 + 1.6, "{case} t={t}: {dy}");
                    if !(1.0..5.6).contains(&t) {
                        assert_eq!(dy, 0.0, "{case} t={t}: the octopus is in its slot");
                    }
                    // The clock follows the island's own move, not one of its own.
                    for s in [&notch, &pill] {
                        assert!(s.digits.iter().all(|d| *d == DigitFx::IDENTITY), "{case} t={t}");
                    }
                    deepest = deepest.max(dy);
                    t += 0.02;
                }
                // At the bottom of the drop the taut rope pulls it down by its
                // tension x 1.4 dip; pulled back up, it has sprung back.
                assert!(deepest > 0.8, "{case}: {deepest}");
                let bottom = shot(&stage(Style::Pill, w, h, 1.5, clock)).deform.shift.1;
                assert!(bottom > 0.6 && bottom <= 1.4, "{case}: {bottom}");
                let late = shot(&stage(Style::Pill, w, h, 5.55, clock)).deform.shift.1;
                assert!(late.abs() < 0.5, "{case}: {late}");
            }
        }
    }

    #[test]
    fn the_rope_thins_as_it_stretches() {
        // Its width over a power of its stretch, never under 0.7 of it.
        assert_eq!(rope_width(0.6), 1.0);
        assert_eq!(rope_width(1.0), 1.0);
        assert!((rope_width(2.0) - 2.0_f32.powf(-0.3)).abs() < 1e-6);
        assert_eq!(rope_width(10.0), 0.7);
        let mut last = 1.0;
        for i in 0..40 {
            let w = rope_width(1.0 + i as f32 * 0.1);
            assert!(w <= last, "{i}");
            last = w;
        }
        // In the story: the lower the octopus hangs under the lip, the
        // longer and thinner the holding arm (just under the lip the rope
        // still loops below it, so only from 12 dip down).
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let bottom = shape(style, w, h).y + h;
                let mut hang: Vec<(f32, f32)> = Vec::new();
                let mut t = 1.0;
                while t < 4.85 {
                    let sh = at(style, w, h, t);
                    let (pivot, ..) = figure_head(&sh).unwrap();
                    if pivot.1 > bottom + 12.0 {
                        hang.push((pivot.1, rope_root_width(&sh)));
                    }
                    t += 0.02;
                }
                hang.sort_by(|a, b| a.0.total_cmp(&b.0));
                // The sway and the squash move it a little at one height.
                for (i, p) in hang.iter().enumerate() {
                    for q in hang[i..].iter().filter(|q| q.0 > p.0 + 4.0) {
                        assert!(q.1 <= p.1 + 0.02, "{case}: {p:?} then {q:?}");
                    }
                }
                let (widest, thinnest) = (hang[0].1, hang[hang.len() - 1].1);
                assert!(thinnest < widest - 0.4 && thinnest < 4.3 * 0.8 && thinnest >= 4.3 * 0.7 - 1e-3, "{case}: {widest} to {thinnest}");
            }
        }
    }

    #[test]
    fn the_figure_takes_over_from_the_slot_head_where_it_sits() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    // Fading in over it before the jump and out over it after
                    // the pull back in: the same place, the same size.
                    for t in [0.9, 0.95, 0.99, 5.61, 5.65, 5.7] {
                        let st = stage(style, w, h, t, clock);
                        let sh = shot(&st);
                        let Slot::Story(m) = sh.slot else { panic!("{case} t={t}: {:?}", sh.slot) };
                        let (pivot, head, alpha) = figure_head(&sh).unwrap_or_else(|| panic!("{case} t={t}: no figure"));
                        let r = st.room().robot.unwrap();
                        let k = r.w / GRID;
                        let slot_pivot = (r.x + 32.0 * k + m.dx, r.y + 40.0 * k + m.dy);
                        assert!((pivot.0 - slot_pivot.0).abs() < 1e-3 && (pivot.1 - slot_pivot.1).abs() < 1e-3, "{case} t={t}: {pivot:?} {slot_pivot:?}");
                        assert!((head.sx - m.sx).abs() < 1e-5 && (head.sy - m.sy).abs() < 1e-5 && head.rot_deg == 0.0, "{case} t={t}: {head:?} {m:?}");
                        assert!(head.bare_octopus, "{case} t={t}: the long arms are its own");
                        assert!((alpha + m.alpha - 1.0).abs() < 1e-5, "{case} t={t}: {alpha} + {}", m.alpha);
                    }
                    // Out of the slot in between; frame by frame it never jumps.
                    let mut last: Option<((f32, f32), f32)> = None;
                    let mut t = 0.85;
                    while t < 5.8 {
                        let sh = shot(&stage(style, w, h, t, clock));
                        if (1.0..5.6).contains(&t) {
                            assert_eq!(sh.slot, Slot::Empty, "{case} t={t}");
                        }
                        let now = figure_head(&sh).map(|(p, _, a)| (p, a));
                        if let (Some((p, a)), Some((q, b))) = (now, last) {
                            assert!((p.0 - q.0).hypot(p.1 - q.1) < 7.0 && (a - b).abs() <= 0.15, "{case} t={t}: {q:?} {b} to {p:?} {a}");
                        }
                        last = now;
                        t += 1.0 / 60.0;
                    }
                }
            }
        }
    }

    #[test]
    fn it_drops_far_below_the_island_on_one_arm() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let s = shape(style, w, h);
                let sh = at(style, w, h, 1.5);
                let (pivot, ..) = figure_head(&sh).unwrap_or_else(|| panic!("{case}: no figure"));
                assert!(pivot.1 > s.y + s.h + 40.0, "{case}: {pivot:?}");
                // Its holding arm still reaches up to the lip.
                let b = front(&sh)[1].draw.bounds();
                assert!(b.y < s.y + s.h + 1.0 && b.y + b.h > pivot.1 - 6.0, "{case}: {b:?}");
            }
        }
    }
}
