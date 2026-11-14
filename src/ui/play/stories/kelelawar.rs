//! The falk's "Kelelawar" (the bat): the head tilts in its slot until it is
//! upside down, drops out of the slot head first, the body coming out from
//! behind the lip, and hangs by its feet from the lip, swinging (a floating
//! Pill tilts a little with it); it spreads its wings wide, crest up, wraps
//! them round its body like a cloak and peeks over one, flaps twice and
//! climbs back up, the head turning upright in its slot. Timeline, easing
//! and offsets from `docs/design/maskot-notch-v3.html`, story `f1`.

use std::f32::consts::TAU;

use super::{Draw, Ease, HEAD_PIVOT, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, island_box, kept, kf, lerp, ring};
use crate::config::Style;
use crate::ui::geometry::{WINDOW_H, WINDOW_W};
use crate::ui::mascot::CrestPose;
use crate::ui::play::art::falk::{BirdPose, WingPose, bird_at, bird_parts};
use crate::ui::play::shape::{Deform, rigid};
use crate::ui::play::trigger::Story;
use crate::ui::robot::{Expr, GRID, Seg};

use Ease::{Io, Lin, O, Ob, Sb};

/// The primaries trail the arm by this much (the mockup's 70 ms).
const LAG: f32 = 0.07;
/// The two strong flaps before it climbs: from 5.35 s, two whole beats at
/// 3.2 Hz (the mockup runs them to 6.0 s, cutting the third mid-beat).
const FLAPS: (f32, f32) = (5.35, 5.35 + 2.0 / 3.2);
/// The figure takes over from the slot head over the first span (its head
/// fading in over the identical slot head) and hands back over the second.
const SHOW_IN: (f32, f32) = (0.1, 0.25);
const SHOW_OUT: (f32, f32) = (7.25, 7.4);
/// Where the head's pivot sits above the feet in the bird (`art::falk`).
const NECK: f32 = 24.0;

/// The head's turn in its slot: slowly over to upside down, a little past
/// it, and back upright at the end with a small overshoot.
fn flip(t: f32) -> f32 {
    kf(t, &[(0.45, 0.0, Lin), (0.95, 195.0, Io), (1.08, 180.0, O), (6.6, 180.0, Io), (7.05, -12.0, Io), (7.25, 0.0, O)])
}

/// How far the bird is out of its slot: 0 in it, 1 hanging from the lip.
fn out_of_slot(t: f32) -> f32 {
    kf(t, &[(1.05, 0.0, Lin), (1.6, 1.0, Io), (5.95, 1.0, Io), (6.55, 0.0, Io)])
}

/// The bird's swing from the lip, degrees: the drop's ring, a slow sway, a
/// jolt as it unwraps.
fn swing(t: f32) -> f32 {
    ring(t, 1.6, 11.0, 0.9, 1.3)
        + 4.0 * (TAU * 0.45 * t).sin() * ((t - 2.0) / 1.0).clamp(0.0, 1.0)
        + kf(t, &[(5.35, 0.0, Lin), (5.6, -6.0, Io), (5.9, 0.0, Io)])
}

/// How far the wings wrap the body as a cloak.
fn wrap(u: f32) -> f32 {
    kf(u, &[(3.7, 0.0, Lin), (4.35, 1.0, Io), (5.15, 1.0, Io), (5.4, 0.0, O)])
}

/// How far the left wing is lowered to peek over.
fn peek(u: f32) -> f32 {
    kf(u, &[(4.55, 0.0, Lin), (4.75, 1.0, O), (5.05, 1.0, Io), (5.15, 0.0, Io)])
}

/// One wing's pose at `u`, `side` -1 left, 1 right: folded, spread (a
/// little past open), the two flaps, the cloak crossed over the chest, the
/// left one lowered to peek.
fn wing_pose(u: f32, side: f32) -> WingPose {
    let spread = kf(
        u,
        &[
            (2.45, 0.0, Lin),
            (2.7, -0.15, Io),
            (3.05, 1.0, Sb),
            (3.7, 1.0, Io),
            (4.0, 0.7, Io),
            (5.15, 0.7, Io),
            (5.4, 1.0, O),
            (5.95, 0.5, Io),
            (6.3, 0.0, Io),
        ],
    );
    // The open wings' slow beat goes on under the two strong flaps, so the
    // arm never jumps where they start or end.
    let mut flap = 6.0 * (TAU * 1.4 * u).sin() * spread.clamp(0.0, 1.0);
    if (FLAPS.0..FLAPS.1).contains(&u) {
        flap += 28.0 * (TAU * 3.2 * (u - FLAPS.0)).sin();
    }
    let mut open = WingPose::mix(WingPose::FOLD, WingPose::SPREAD, spread.clamp(-0.2, 1.2));
    open.arm += flap;
    let breath = 3.0 * (TAU * 0.9 * u).sin() * side;
    let cross = WingPose { arm: WingPose::CROSS.arm + breath + if side < 0.0 { -10.0 } else { 0.0 }, ..WingPose::CROSS };
    let lowered = if side < 0.0 { peek(u) } else { 0.0 };
    WingPose::mix(open, WingPose::mix(cross, WingPose::LOW, lowered), wrap(u))
}

/// The bird at `t` with `face` on its head, swinging `swing` degrees (its
/// tail turns against it). The primaries take the pose `LAG` earlier.
fn bird_pose(t: f32, face: MascotShot, swing: f32) -> BirdPose {
    let front = wrap(t) > 0.3;
    BirdPose {
        wl: wing_pose(t, -1.0),
        wr: wing_pose(t, 1.0),
        wl_before: wing_pose(t - LAG, -1.0),
        wr_before: wing_pose(t - LAG, 1.0),
        wl_front: front,
        wr_front: front,
        face,
        tail_rot: -swing * 0.5,
        tail_spread: 0.0,
        tuck: false,
    }
}

/// The head's face at `t` in the app's own expressions, turned by `flip`:
/// the mockup's happy eyes are Happy, its crest the falk's crest (`crestUp`
/// its scale), its look a glance. `swing` is the bird's swing as it hangs.
fn face(t: f32, swing: f32) -> MascotShot {
    let happy = (2.95..3.65).contains(&t) || (6.9..7.5).contains(&t);
    // The quick blinks, and the slow one peeking over the cloak (the
    // mockup's 0.22 s; shut past the 0.25 threshold for about 0.165 s).
    let blink = blinks(t, &[0.3, 2.45, 7.45]).max(1.0 - (t - 4.95).abs() / 0.11);
    // The mockup holds its last crest (1.1) to the end; it settles to rest
    // here.
    let crest_up = kf(
        t,
        &[
            (0.3, 1.0, Lin),
            (0.5, 1.25, O),
            (1.0, 0.75, Io),
            (2.4, 0.7, Io),
            (2.7, 0.55, Io),
            (2.95, 1.05, Ob),
            (3.6, 0.9, Io),
            (4.2, 0.6, Io),
            (4.75, 1.05, Ob),
            (5.4, 0.8, Io),
            (6.6, 0.8, Io),
            (7.1, 1.1, Ob),
            (7.45, 1.0, Io),
        ],
    );
    // One eye aside while it turns, back as it drops (the mockup snaps it
    // back at the drop), and the peek over the cloak.
    let look = kf(t, &[(0.2, 0.0, Lin), (0.5, 1.0, Io), (1.0, 1.0, Io), (1.3, 0.0, Io)]) - 0.6 * peek(t);
    MascotShot {
        rot_deg: flip(t),
        expr: if happy { Expr::Happy } else { Expr::Idle },
        crest: CrestPose { rot_deg: 10.0 + ring(t, 1.0, 12.0, 2.2, 3.0) + swing * 0.8, scale: crest_up },
        glance: (look * 2.4, 0.0),
        blink: blink > 0.25,
        ..MascotShot::REST
    }
}

/// The window from the island's top edge down: the bird's body and tail
/// are cut to it, so on their way down from the slot they never show above
/// a floating Pill (what is behind the island the back layer cuts away).
fn below_top(top: f32) -> Vec<Seg> {
    vec![Seg::Move(0.0, top), Seg::Line(WINDOW_W, top), Seg::Line(WINDOW_W, WINDOW_H), Seg::Line(0.0, WINDOW_H), Seg::Close]
}

/// One moment of the story on `stage`, the falk in the coat of its look.
/// `beat.alpha` eases it home, paced by the wind-down's time (`kept`): over
/// its first 0.58 the island settles; from 0.15 to 0.73 the figure fades out
/// as the slot head fades back in its place, upright and idle, one
/// crossfade, so the falk never shows twice. Cut short,
/// the story holds where the cut found it, so nothing it had not brought out
/// shows. Exactly at rest at 0 s, at the end and with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, .. } = *stage;
    let (t, a) = (beat.cut_at.map_or(beat.t, |c| beat.t.min(c)), beat.alpha.clamp(0.0, 1.0));
    let mut out = Shot::rest(style, s);
    if t <= 0.0 || t >= Story::Kelelawar.secs() || a == 0.0 {
        return out;
    }
    // The slot where the renderer puts it (the clock on or off moves it).
    let Some(slot) = stage.room().robot else {
        return out;
    };
    let k = slot.w / GRID;
    let pivot = (slot.x + HEAD_PIVOT.0 * k, slot.y + HEAD_PIVOT.1 * k);
    let (out_k, swing) = (out_of_slot(t), swing(t));
    let f = face(t, swing * out_k);

    // The figure (its head over the slot head, then the bird) and the slot
    // head under it; wound down, the one crossfades into the other.
    let show = kf(t, &[(SHOW_IN.0, 0.0, Lin), (SHOW_IN.1, 1.0, Lin), (SHOW_OUT.0, 1.0, Lin), (SHOW_OUT.1, 0.0, Lin)]);
    let fig = kept(a, 0.15, 0.73);
    let figure = show * fig;
    let hidden = show >= 1.0;
    let slot_alpha = if hidden { 1.0 - fig } else { 1.0 };
    // Upright in its slot; its face eases to rest through the wind-down.
    // Coming back from hiding it is the resting head, so it never snaps
    // from a happy or blinking face to rest.
    let ease = kept(a, 0.4, 1.0);
    out.halo = 1.0 - figure;
    out.slot = if slot_alpha <= 0.0 {
        Slot::Empty
    } else if hidden {
        Slot::Story(MascotShot { alpha: slot_alpha, ..MascotShot::REST })
    } else {
        Slot::Story(MascotShot {
            rot_deg: 0.0,
            crest: CrestPose {
                rot_deg: lerp(MascotShot::REST.crest.rot_deg, f.crest.rot_deg, ease),
                scale: lerp(MascotShot::REST.crest.scale, f.crest.scale, ease),
            },
            glance: (f.glance.0 * ease, f.glance.1 * ease),
            alpha: slot_alpha,
            ..f
        })
    };

    // A floating Pill tilts about its centre with the bird's swing (the
    // clock rides on it); a notch is welded to the screen edge.
    if style == Style::Pill {
        out.deform = Deform { tilt_deg: swing * out_k * 0.06 * kept(a, 0.0, 0.58), ..Deform::default() };
        out.island = island_box(style, s, &out.deform);
    }
    if figure <= 0.0 {
        return out;
    }

    // In its slot: the head alone, in front, turning.
    if out_k <= 0.001 {
        out.parts.push(Part { layer: Layer::Front, draw: Draw::Head { at: pivot, shot: f }, alpha: figure });
        return out;
    }

    // Out: the bird upside down from its feet, which travel from where the
    // head sat in the slot to the lip 6 right of the slot's centre, on the
    // island as it tilts. The tail, body and wings behind the island; the
    // feet, the head and the cloak in front.
    let m = rigid(style, s, &out.deform);
    let (gx, gy) = (pivot.0 + 6.0, s.y + s.h + 0.4);
    let grip = (gx * m.M11 + gy * m.M21 + m.M31, gx * m.M12 + gy * m.M22 + m.M32);
    let at = (lerp(pivot.0, grip.0, out_k), lerp(pivot.1 - NECK, grip.1, out_k));
    let r = 180.0 + swing * out_k;
    let sq = 1.0 + ring(t, 1.6, -0.09, 3.0, 6.0);
    // The bird turns the head by its own 180; the face keeps the rest of the turn.
    let pose = bird_pose(t, MascotShot { rot_deg: f.rot_deg - 180.0, ..f }, swing);
    let b = bird_parts(&pose, look.falk);
    let mut back = vec![b.tail];
    back.extend(b.body);
    out.parts.push(Part {
        layer: Layer::Back,
        draw: Draw::Clipped { clip: below_top(s.y), items: vec![bird_at(at, r, 1.0, sq, back)] },
        alpha: figure,
    });
    // The toes show as they reach the lip (the mockup's 3 dip above it),
    // over the last 2 dip of the way rather than at once.
    let toes = ((at.1 - (grip.1 - 3.4)) / 2.0).clamp(0.0, 1.0);
    if toes > 0.0 {
        out.parts.push(Part { layer: Layer::Front, draw: bird_at(at, r, 1.0, sq, b.feet), alpha: figure * toes });
    }
    let mut front = vec![b.head];
    front.extend(b.wings_front);
    out.parts.push(Part { layer: Layer::Front, draw: bird_at(at, r, 1.0, sq, front), alpha: figure });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::theme::{FalkColor, Mascot};
    use crate::ui::cat_style::CatLook;
    use crate::ui::geometry::corner_radius;
    use crate::ui::play::art::falk::falk_coat;
    use crate::ui::play::shape::{Deform, is_rest, rigid};
    use crate::ui::play::state::Beat;
    use crate::ui::play::stories::{Draw, FRAME_FADE, Layer, Slot, wind_down_frames};
    use crate::ui::play::testing::shape;
    use crate::ui::play::trigger::Story;
    use crate::ui::render::DigitFx;
    use crate::ui::robot::{Expr, GRID};
    use windows_numerics::Matrix3x2;

    crate::ui::play::stories::story_contract_tests!(Story::Kelelawar, shot);

    const SIZES: [(f32, f32); 3] = crate::ui::play::stories::CONTRACT_SIZES;

    /// The idle island `w` x `h` with the falk, the clock on or off.
    fn stage(style: Style, w: f32, h: f32, beat: Beat, clock: bool) -> Stage {
        let look = CatLook { mascot: Mascot::Falk, ..CatLook::default() };
        Stage::idle(style, shape(style, w, h), beat, look, clock)
    }

    fn playing(t: f32) -> Beat {
        Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }
    }

    /// At `t`, playing, with the clock.
    fn at(style: Style, w: f32, h: f32, t: f32) -> Shot {
        shot(&stage(style, w, h, playing(t), true))
    }

    /// The slot head's resting pivot on `st`.
    fn rest_pivot(st: &Stage) -> (f32, f32) {
        let r = st.room().robot.unwrap();
        let k = r.w / GRID;
        (r.x + 32.0 * k, r.y + 40.0 * k)
    }

    /// `d` holds the falk's head, as the bird's head group does.
    fn has_head(d: &Draw) -> bool {
        match d {
            Draw::Head { .. } => true,
            Draw::Group { items, .. } | Draw::Clipped { items, .. } => items.iter().any(has_head),
            _ => false,
        }
    }

    /// The bird's head part in front: its place, turn and y scale.
    fn bird_place(shot: &Shot) -> Option<((f32, f32), f32, f32)> {
        shot.parts.iter().filter(|p| p.layer == Layer::Front).find_map(|p| match &p.draw {
            Draw::Group { at, rot_deg, scale, items, .. } if items.iter().any(has_head) => Some((*at, *rot_deg, scale.1)),
            _ => None,
        })
    }

    /// The feet: a group of toes alone, in front.
    fn feet(shot: &Shot) -> Vec<&crate::ui::play::stories::Part> {
        shot.parts
            .iter()
            .filter(|p| p.layer == Layer::Front)
            .filter(|p| matches!(&p.draw, Draw::Group { items, .. } if !items.is_empty() && items.iter().all(|i| matches!(i, Draw::Stroke { .. }))))
            .collect()
    }

    /// The face on the head inside the bird.
    fn bird_face(d: &Draw) -> Option<MascotShot> {
        match d {
            Draw::Head { shot, .. } => Some(*shot),
            Draw::Group { items, .. } | Draw::Clipped { items, .. } => items.iter().find_map(bird_face),
            _ => None,
        }
    }

    /// The head the shot shows in front, wherever it is: its pivot and its
    /// whole turn (the bird's and its own).
    fn front_head(shot: &Shot) -> Option<((f32, f32), f32)> {
        for p in shot.parts.iter().filter(|p| p.layer == Layer::Front) {
            match &p.draw {
                Draw::Head { at, shot } => return Some((*at, shot.rot_deg)),
                d @ Draw::Group { at, rot_deg, scale, items, .. } if items.iter().any(has_head) => {
                    let face = bird_face(d).unwrap();
                    let (sin, cos) = rot_deg.to_radians().sin_cos();
                    let y = -24.0 * scale.1;
                    return Some(((at.0 - y * sin, at.1 + y * cos), rot_deg + face.rot_deg));
                }
                _ => {}
            }
        }
        None
    }

    /// `p` back on the island before its rigid motion `m` (a turn and a shift).
    fn local(m: Matrix3x2, p: (f32, f32)) -> (f32, f32) {
        let (x, y) = (p.0 - m.M31, p.1 - m.M32);
        (x * m.M11 + y * m.M12, -x * m.M12 + y * m.M11)
    }

    #[test]
    fn only_the_pill_tilts_with_the_swing() {
        for (w, h) in SIZES {
            let mut most = 0.0f32;
            let mut t = 0.0;
            while t < Story::Kelelawar.secs() {
                let case = format!("{w}x{h} t={t}");
                // The Notch hangs from the screen edge: nothing moves it.
                assert_eq!(at(Style::Notch, w, h, t).deform, Deform::default(), "{case}");
                // The Pill tilts about its centre with the bird's swing x
                // 0.06 degrees, nothing else; the clock rides on it.
                let pill = at(Style::Pill, w, h, t);
                let d = &pill.deform;
                assert!(Deform { tilt_deg: 0.0, ..d.clone() } == Deform::default(), "{case}: {d:?}");
                let swing = bird_place(&pill).map_or(0.0, |(_, r, _)| r - 180.0);
                assert!((d.tilt_deg - swing * 0.06).abs() < 2e-4, "{case}: tilt {} for a swing of {swing}", d.tilt_deg);
                assert_eq!(pill.digits, [DigitFx::IDENTITY; 5], "{case}");
                most = most.max(d.tilt_deg.abs());
                t += 0.02;
            }
            assert!(most > 0.3 && most < 1.2, "{w}x{h}: {most}");
        }
    }

    #[test]
    fn the_feet_stay_on_the_lip() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let r = corner_radius(style, s);
                for clock in [true, false] {
                    let mut t = 1.6;
                    while t <= 5.95 {
                        let case = format!("{style:?} {w}x{h} clock={clock} t={t}");
                        let st = stage(style, w, h, playing(t), clock);
                        let shot = shot(&st);
                        let toes = feet(&shot);
                        assert_eq!(toes.len(), 1, "{case}");
                        assert_eq!(toes[0].alpha, 1.0, "{case}");
                        let Draw::Group { at: g, .. } = toes[0].draw else { unreachable!() };
                        // On the island's straight lip as it tilts, 6 right of
                        // the slot's centre, the same on both styles.
                        let (lx, ly) = local(rigid(style, s, &shot.deform), g);
                        assert!((ly - (s.y + s.h + 0.4)).abs() < 1e-3, "{case}: {ly}");
                        assert!((lx - (rest_pivot(&st).0 + 6.0)).abs() < 1e-3, "{case}: {lx}");
                        assert!(lx - 4.3 >= s.x + r && lx + 4.3 <= s.x + s.w - r, "{case}: {lx}");
                        // The bird hangs from them.
                        assert_eq!(bird_place(&shot).unwrap().0, g, "{case}");
                        t += 0.02;
                    }
                    // None before they reach the lip, nor once they leave it.
                    for t in [0.5, 1.2, 6.5, 7.0] {
                        assert!(feet(&shot(&stage(style, w, h, playing(t), clock))).is_empty(), "{style:?} {w}x{h} t={t}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_slot_head_hands_over_to_the_figure_and_back_in_place() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                for clock in [true, false] {
                    let mut alpha = Vec::new();
                    for t in [0.15, 0.2, 7.3, 7.35] {
                        let case = format!("{style:?} {w}x{h} clock={clock} t={t}");
                        let st = stage(style, w, h, playing(t), clock);
                        let shot = shot(&st);
                        let Slot::Story(m) = shot.slot else { panic!("{case}: {:?}", shot.slot) };
                        // The slot head in its place, the figure's identical
                        // head fading over it, its halo going with it.
                        assert_eq!((m.dx, m.dy, m.alpha), (0.0, 0.0, 1.0), "{case}");
                        assert_eq!(shot.parts.len(), 1, "{case}");
                        let p = &shot.parts[0];
                        let Draw::Head { at: pivot, shot: head } = p.draw else { panic!("{case}: {:?}", p.draw) };
                        assert_eq!(p.layer, Layer::Front, "{case}");
                        let rest = rest_pivot(&st);
                        assert!((pivot.0 - rest.0).abs() < 1e-3 && (pivot.1 - rest.1).abs() < 1e-3, "{case}: {pivot:?} {rest:?}");
                        assert_eq!(head, m, "{case}");
                        assert!(p.alpha > 0.0 && p.alpha < 1.0, "{case}: {}", p.alpha);
                        assert!((shot.halo - (1.0 - p.alpha)).abs() < 1e-6, "{case}: {}", shot.halo);
                        alpha.push(p.alpha);
                    }
                    assert!(alpha[0] < alpha[1] && alpha[2] > alpha[3], "{style:?} {w}x{h}: {alpha:?}");
                    // Between, the slot is empty: the head is the story's.
                    for t in [0.3, 1.0, 3.0, 6.0, 7.0] {
                        assert_eq!(shot(&stage(style, w, h, playing(t), clock)).slot, Slot::Empty, "{style:?} {w}x{h} t={t}");
                    }
                    // After, the slot head alone.
                    let end = shot(&stage(style, w, h, playing(7.5), clock));
                    assert!(matches!(end.slot, Slot::Story(m) if m.alpha == 1.0) && end.parts.is_empty() && end.halo == 1.0, "{style:?} {w}x{h}");
                }
            }
        }
    }

    #[test]
    fn the_head_turns_upside_down_and_drops_head_first_without_a_jump() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let st = stage(style, w, h, playing(0.0), true);
                let rest = rest_pivot(&st);
                for (from, to) in [(0.3, 1.7), (5.8, 7.2)] {
                    let mut last: Option<((f32, f32), f32)> = None;
                    let mut t = from;
                    while t <= to {
                        let case = format!("{style:?} {w}x{h} t={t}");
                        let now = front_head(&at(style, w, h, t)).unwrap_or_else(|| panic!("{case}: no head"));
                        if let Some((p, r)) = last {
                            assert!((now.0.0 - p.0).hypot(now.0.1 - p.1) < 1.5, "{case}: {p:?} to {:?}", now.0);
                            assert!((now.1 - r).abs() < 8.0, "{case}: turned {r} to {}", now.1);
                        }
                        last = Some(now);
                        t += 0.005;
                    }
                }
                // Upside down in its slot before it drops; head first below
                // the lip once it hangs.
                let (p, r) = front_head(&at(style, w, h, 1.0)).unwrap();
                assert!((p.0 - rest.0).abs() < 1e-3 && (p.1 - rest.1).abs() < 1e-3 && (r - 180.0).abs() < 20.0, "{style:?} {w}x{h}: {p:?} {r}");
                let (p, _) = front_head(&at(style, w, h, 1.6)).unwrap();
                assert!(p.1 > s.y + s.h + 20.0, "{style:?} {w}x{h}: {p:?}");
            }
        }
    }

    #[test]
    fn the_body_never_shows_above_the_island() {
        // It comes out from behind the lip; on its way down from the slot
        // the body and the tail are cut, not left over a floating Pill.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let mut t = 0.0;
                while t < Story::Kelelawar.secs() {
                    for p in at(style, w, h, t).parts.iter().filter(|p| p.layer == Layer::Back) {
                        let b = p.draw.bounds();
                        assert!(b.y >= s.y - 1e-3, "{style:?} {w}x{h} t={t}: {b:?}");
                    }
                    t += 0.02;
                }
                assert!(at(style, w, h, 1.1).parts.iter().any(|p| p.layer == Layer::Back), "{style:?} {w}x{h}");
            }
        }
    }

    #[test]
    fn the_primaries_trail_the_arm_by_70_ms() {
        let face = MascotShot::REST;
        for t in [3.0, 4.2, 5.5] {
            let p = bird_pose(t, face, 0.0);
            assert_eq!((p.wl, p.wr), (wing_pose(t, -1.0), wing_pose(t, 1.0)), "t={t}");
            assert_eq!((p.wl_before, p.wr_before), (wing_pose(t - 0.07, -1.0), wing_pose(t - 0.07, 1.0)), "t={t}");
            assert_ne!(p.wl, p.wl_before, "t={t}");
        }
        // Two whole strong flaps, the arm continuous where they start and end.
        assert!(((FLAPS.1 - FLAPS.0) * 3.2 - 2.0).abs() < 1e-5, "{FLAPS:?}");
        for edge in [FLAPS.0, FLAPS.1] {
            for side in [-1.0, 1.0] {
                let (a, b) = (wing_pose(edge - 1e-4, side).arm, wing_pose(edge + 1e-4, side).arm);
                assert!((a - b).abs() < 0.2, "edge {edge} side {side}: {a} to {b}");
            }
        }
        // Wrapped at 4.6 s: the right wing crossed over the chest (`CROSS`, 186).
        let arm = wing_pose(4.6, 1.0).arm;
        assert!((arm - 186.0).abs() < 4.0, "{arm}");
    }

    #[test]
    fn the_wings_open_then_wrap_the_body_like_a_cloak() {
        for style in [Style::Notch, Style::Pill] {
            let width = |t: f32| {
                let shot = at(style, 190.0, 34.0, t);
                shot.parts.iter().filter(|p| p.layer == Layer::Back).map(|p| p.draw.bounds().w).fold(0.0, f32::max)
            };
            // Wide open at 3.3 s, folded while it hangs.
            assert!(width(3.3) > width(2.2) + 30.0, "{style:?}: {} {}", width(3.3), width(2.2));
            // Wrapped in front of the head at 4.6 s, behind it while open.
            let front = |t: f32| {
                let shot = at(style, 190.0, 34.0, t);
                shot.parts.iter().filter(|p| p.layer == Layer::Front).find_map(|p| match &p.draw {
                    Draw::Group { items, .. } if items.iter().any(has_head) => Some(items.len()),
                    _ => None,
                })
            };
            assert_eq!(front(3.3), Some(1), "{style:?}");
            assert!(front(4.6).unwrap() > 1, "{style:?}");
        }
    }

    #[test]
    fn the_faces_are_the_mockups_in_the_apps_expressions() {
        let face = |t: f32| {
            let s = at(Style::Notch, 190.0, 34.0, t);
            let figure = s.parts.iter().filter(|p| p.layer == Layer::Front).find_map(|p| bird_face(&p.draw));
            figure.or(match s.slot {
                Slot::Story(m) => Some(m),
                _ => None,
            })
            .unwrap_or_else(|| panic!("t={t}: no head"))
        };
        // Happy with its wings open and climbing home; idle hanging.
        assert_eq!(face(3.3).expr, Expr::Happy);
        assert_eq!(face(7.0).expr, Expr::Happy);
        assert_eq!(face(2.2).expr, Expr::Idle);
        // The slow blink peeking over the cloak (shut about 0.165 s), the quick ones.
        assert!(face(5.02).blink && face(2.45).blink && !face(2.2).blink);
        // The crest: flat while it hangs, up as the wings open.
        assert!(face(2.7).crest.scale < 0.6 && face(2.95).crest.scale > 1.0, "{} {}", face(2.7).crest.scale, face(2.95).crest.scale);
        // A sideways look as it turns, back by the time it hangs.
        assert!(face(0.8).glance.0 > 2.0 && face(1.6).glance.0 == 0.0);
        // Back at rest before the end.
        let end = face(7.55);
        assert_eq!(MascotShot { blink: false, ..end }, MascotShot::REST, "{end:?}");
    }

    #[test]
    fn a_wind_down_brings_the_falk_home_through_its_slot() {
        // Cut hanging, or turning in the slot: the slot head comes back in
        // its place, upright, idle and open-eyed, as the figure fades, one
        // crossfade (never the falk twice), each by at most a frame's fade;
        // the island settles; then rest.
        for style in [Style::Notch, Style::Pill] {
            // Turning, hanging, happy with its wings open, blinking in the cloak.
            for cut in [0.7, 0.8, 3.0, 3.3, 4.6, 4.95] {
                let frames = wind_down_frames(cut, 0.0, |beat| shot(&stage(style, 190.0, 34.0, beat, true)));
                let mut last = 0.0;
                for (ms, beat, shot) in &frames {
                    let case = format!("{style:?} cut {cut} +{ms:.1} ms (alpha {})", beat.alpha);
                    let slot = match shot.slot {
                        Slot::Story(m) => {
                            assert_eq!((m.dx, m.dy, m.rot_deg), (0.0, 0.0, 0.0), "{case}");
                            m.alpha
                        }
                        Slot::Empty => 0.0,
                        Slot::App => 1.0,
                    };
                    assert!((slot - last).abs() <= FRAME_FADE + 1e-4, "{case}: the slot head from {last} to {slot}");
                    last = slot;
                    let figure = shot.parts.iter().filter(|p| p.layer == Layer::Front && has_head(&p.draw)).map(|p| p.alpha).fold(0.0, f32::max);
                    assert!(slot + figure <= 1.0 + 1e-4, "{case}: the slot head {slot} and the figure {figure}");
                    if let Slot::Story(m) = shot.slot {
                        assert!(m.expr == Expr::Idle && !m.blink, "{case}: {m:?}");
                    }
                    if beat.alpha < 0.25 {
                        assert!(is_rest(style, &shot.deform), "{case}: {:?}", shot.deform);
                    }
                }
                let (_, beat, end) = frames.last().unwrap();
                assert_eq!(beat.alpha, 0.0);
                assert_eq!(*end, Shot::rest(style, shape(style, 190.0, 34.0)), "{style:?} cut {cut}");
            }
        }
    }

    #[test]
    fn the_bird_wears_the_falk_coat() {
        fn fills(d: &Draw, out: &mut Vec<u32>) {
            match d {
                Draw::Fill { rgb, .. } | Draw::Ellipse { rgb, .. } => out.push(*rgb),
                Draw::Group { items, .. } | Draw::Clipped { items, .. } => items.iter().for_each(|i| fills(i, out)),
                _ => {}
            }
        }
        for coat in FalkColor::ALL.iter().copied() {
            let look = CatLook { mascot: Mascot::Falk, falk: coat, ..CatLook::default() };
            let st = Stage { look, ..stage(Style::Pill, 190.0, 34.0, playing(3.3), true) };
            let mut rgb = Vec::new();
            shot(&st).parts.iter().filter(|p| p.layer == Layer::Back).for_each(|p| fills(&p.draw, &mut rgb));
            let c = falk_coat(coat);
            assert!(rgb.contains(&c.body) && rgb.contains(&c.prim), "{coat:?}: {rgb:x?}");
        }
    }

    #[test]
    fn a_cut_holds_the_story_where_it_was() {
        // Cut just before the wings open, they never open in the wind-down.
        let frames = wind_down_frames(2.6, 0.0, |beat| shot(&stage(Style::Pill, 190.0, 34.0, beat, true)));
        let open = frames[0].2.parts.iter().filter(|p| p.layer == Layer::Back).map(|p| p.draw.bounds().w).fold(0.0, f32::max);
        for (ms, _, shot) in &frames {
            for p in shot.parts.iter().filter(|p| p.layer == Layer::Back) {
                assert!(p.draw.bounds().w <= open + 1e-3, "+{ms} ms: {}", p.draw.bounds().w);
            }
        }
    }
}
