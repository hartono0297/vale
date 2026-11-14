//! The falk's "Maling angka": the minute's last digit wiggles like a bug,
//! the falk tilts its head at it, scoots over inside the island, taps it
//! with its beak, bites it and carries it home like a twig. The clock
//! regrows the digit, the crest shoots up in surprise, the stolen one drops
//! and falls away, the crest droops and the falk whistles as if nothing
//! happened. Timeline, easing and offsets from `docs/design/maskot-notch-v3.html`,
//! story `f2`.

use std::f32::consts::{PI, TAU};

use super::{Ease, HEAD_PIVOT, MascotShot, Prop, Shot, blinks, bump, kf, lerp, ring, toward_rest};
use crate::config::Style;
use crate::ui::geometry::{ShapeRect, idle_layout_lead};
use crate::ui::mascot::CrestPose;
use crate::ui::play::state::{Beat, spread_offsets};
use crate::ui::play::trigger::Story;
use crate::ui::render::DigitFx;
use crate::ui::robot::{Expr, GRID};

use Ease::{Io, Lin, O, Sb, Sio};

/// The clock's character centres from the clock box's centre (DM Sans at
/// 16, `%H:%M`), as the mockup measures them. Only where the falk aims and
/// where the stolen digit hangs from the beak; the clock's own characters
/// are placed by DirectWrite.
const DIGIT_X: [f32; 5] = [-17.2, -7.6, 0.0, 7.6, 17.2];
/// The digit the falk steals: the minute's last.
const STOLEN: usize = 4;
/// The head stops this far right of the stolen digit's centre: close enough
/// to peck it, inside the island's right end.
const AIM: f32 = 4.5;
/// The head shrinks to this while it is out of its slot.
const AWAY_SCALE: f32 = 0.86;
/// The crest at rest (`crest_pose` of the idle face).
const CREST_REST: CrestPose = CrestPose { rot_deg: 10.0, scale: 1.0 };
/// The beak's tip on the head's grid; the stolen digit hangs from it.
const BEAK: (f32, f32) = (32.0, 49.5);

/// The head's resting pivot (`HEAD_PIVOT`) in the slot of `s` with the
/// island `spread` open, and the head grid's scale (dip per unit).
fn head_pivot(s: ShapeRect, spread: f32) -> Option<((f32, f32), f32)> {
    let slot = spread_offsets(idle_layout_lead(s, true, true, 0.0), s, spread).robot?;
    let k = slot.w / GRID;
    Some(((slot.x + HEAD_PIVOT.0 * k, slot.y + HEAD_PIVOT.1 * k), k))
}

/// The beak's tip of the head `m` draws, from its resting pivot `rest` on
/// a grid of scale `k`.
fn beak_at(m: &MascotShot, rest: (f32, f32), k: f32) -> (f32, f32) {
    let (sin, cos) = m.rot_deg.to_radians().sin_cos();
    let (bx, by) = ((BEAK.0 - HEAD_PIVOT.0) * k * m.sx, (BEAK.1 - HEAD_PIVOT.1) * k * m.sy);
    (rest.0 + m.dx + bx * cos - by * sin, rest.1 + m.dy + bx * sin + by * cos)
}

/// One moment of the story on `s`. `beat.alpha` eases the whole of it home
/// and `beat.spread` is where the island has made room (the mascot at its
/// left end, the clock at its right). Exactly at rest at 0 s, at the end
/// and with `alpha` 0.
pub fn shot(style: Style, s: ShapeRect, beat: Beat) -> Shot {
    let (t, a) = (beat.t, beat.alpha.clamp(0.0, 1.0));
    let mut out = Shot::rest(style, s);
    if t <= 0.0 || t >= Story::MalingAngka.secs() || a == 0.0 {
        return out;
    }
    // Where the slot and the clock are once the island has made room.
    let room = spread_offsets(idle_layout_lead(s, true, true, 0.0), s, beat.spread);
    let (Some((rest, k)), Some(clock)) = (head_pivot(s, beat.spread), room.clock) else {
        return out;
    };
    let mid = s.y + s.h / 2.0;
    let home = (clock.x + clock.w / 2.0 + DIGIT_X[STOLEN], mid);

    // The head: over to the minute and back, a hop in each scoot, three taps.
    let go = kf(t, &[(1.0, 0.0, Lin), (2.05, 1.0, Sio), (3.75, 1.0, Io), (4.55, 0.0, Sio)]);
    let scoot = if (1.0..2.05).contains(&t) {
        (PI * (t - 1.0) / 1.05).sin()
    } else if (3.75..4.55).contains(&t) {
        (PI * (t - 3.75) / 0.8).sin()
    } else {
        0.0
    };
    let hop = if scoot > 0.0 {
        (TAU * if t < 3.0 { 3.0 } else { 4.0 } * t).sin().abs()
    } else {
        0.0
    };
    let tap = bump(t, 2.35, 0.22) + bump(t, 2.75, 0.22) + bump(t, 3.3, 0.18);
    // At the minute the pivot sits at the island's middle, as the mockup
    // draws it; in the slot it is the slot's own.
    let x = lerp(rest.0, home.0 + AIM, go) - 4.0 * tap;
    let y = lerp(rest.1, mid, go) - 1.8 * hop * scoot + 1.2 * tap;
    let curious = kf(t, &[(0.35, 0.0, Lin), (0.7, 1.0, Sio), (1.0, 1.0, Io), (1.2, 0.0, Sio)]);
    // The head's and the crest's rings have all but died by 7 s; this takes
    // what is left to exactly nothing before the end.
    let settle = kf(t, &[(7.0, 1.0, Lin), (7.5, 0.0, Sio)]);
    let startle = kf(t, &[(4.95, 0.0, Lin), (5.05, 1.0, O), (5.5, 1.0, Io), (6.7, 0.0, Sio)]);
    // Leaning at the digit while it is over there.
    let lean = if go > 0.98 && t < 3.75 {
        -18.0 * kf(t, &[(2.05, 0.0, Lin), (2.2, 1.0, Sio), (3.5, 1.0, Io), (3.75, 0.0, Io)])
    } else {
        0.0
    };
    let rot = 32.0 * curious + lean - 14.0 * tap
        + 6.0 * (TAU * 3.0 * t).sin() * scoot
        + ring(t, 5.0, -10.0, 3.0, 3.0) * settle
        + kf(t, &[(6.0, 0.0, Lin), (6.3, -8.0, Sio), (7.3, -8.0, Io), (7.6, 0.0, Sio)]);
    // The crest: up with interest, up again with the bite, shot up by the
    // regrow, then a slow droop and back. The mockup holds 0.95 after its
    // last key and cuts its droop off at 7.0 s; both ease into the resting
    // crest here instead.
    let crest_up = kf(
        t,
        &[
            (0.3, 1.0, Lin),
            (0.6, 1.2, Sio),
            (3.5, 1.0, Io),
            (3.6, 1.35, Sb),
            (4.6, 1.15, Io),
            (4.95, 1.05, Io),
            (5.03, 1.75, O),
            (5.5, 1.7, Io),
            (6.7, 0.7, Sio),
            (7.5, 0.95, Sio),
            (7.75, 1.0, Sio),
        ],
    );
    let crest_rot = CREST_REST.rot_deg
        + (ring(t, 3.6, 10.0, 2.4, 3.0) + ring(t, 5.03, 14.0, 2.4, 2.5)) * settle
        + 6.0 * (TAU * 3.0 * t - 1.0).sin() * scoot
        + kf(t, &[(5.5, 0.0, Lin), (6.7, -12.0, Lin), (7.0, -12.0, Io), (7.5, 0.0, Sio)]);
    let expr = if startle > 0.5 && t < 5.6 {
        Expr::Alert
    } else if (3.55..4.9).contains(&t) {
        Expr::Happy
    } else {
        Expr::Idle
    };
    let whistle = if (6.35..7.3).contains(&t) {
        bump(t, 6.4, 0.3).max(bump(t, 6.85, 0.3))
    } else {
        0.0
    };
    // One eye on the digit from the slot, then on it from its right, then
    // the innocent look away.
    let look = if curious > 0.3 {
        1.0
    } else if (t > 6.0 && t < 7.4) || (go > 0.5 && t < 3.75) {
        -1.0
    } else {
        0.0
    };
    let chirp = whistle * 0.5 + 0.4 * bump(t, 3.6, 0.2);
    let squash = 1.0 + ring(t, 5.03, 0.08, 3.0, 5.0) * settle - 0.05 * hop * scoot;
    let size = lerp(1.0, AWAY_SCALE, go);
    let (sx, sy) = (size / squash.sqrt(), size * squash);

    // The head as drawn: eased toward its slot by `a`. Whatever hangs from
    // it (the stolen digit, the whistle) follows this head, not the full one.
    let head = MascotShot {
        dx: (x - rest.0) * a,
        dy: (y - rest.1) * a,
        rot_deg: rot * a,
        sx: lerp(1.0, sx, a),
        sy: lerp(1.0, sy, a),
        expr,
        crest: CrestPose {
            rot_deg: lerp(CREST_REST.rot_deg, crest_rot, a),
            scale: lerp(CREST_REST.scale, crest_up, a),
        },
        glance: (look * 2.4 * a, curious * 0.3 * 2.2 * a),
        blink: blinks(t, &[1.15, 4.75, 7.6]) > 0.25,
        beak_open: chirp > 0.05,
    };
    let beak = beak_at(&head, rest, k);
    let pivot = (rest.0 + head.dx, rest.1 + head.dy);
    if whistle > 0.0 {
        out.props.push(Prop::Whistle {
            at: (pivot.0 + 8.0, pivot.1 - 5.0),
            alpha: whistle.min(1.0) * a,
        });
    }

    // The digit: wiggles like a bug, knocked by each tap, bitten at 3.55 s,
    // carried, regrown by the clock from 4.7 s, dropped at 5.05 s.
    let fx = if t < 3.55 {
        let early = if t < 2.0 { 1.0 } else { 0.0 };
        DigitFx {
            dy: 0.5 * (TAU * 7.0 * t).sin() * early,
            rot_deg: 6.0 * (TAU * 5.0 * t).sin() * 0.6 * early
                + ring(t, 2.45, 10.0, 4.0, 6.0)
                + ring(t, 2.85, -10.0, 4.0, 6.0)
                + ring(t, 3.38, 8.0, 4.0, 6.0),
            ..DigitFx::IDENTITY
        }
    } else {
        let drop = t - 5.05;
        let (at, turn) = if drop > 0.0 {
            (
                (beak.0 + 4.0 * drop, beak.1 + 90.0 * drop * drop),
                head.rot_deg + 80.0 + 420.0 * drop,
            )
        } else {
            (beak, head.rot_deg + 80.0)
        };
        // Gone over its last quarter second of falling, well below the island.
        let fade = ((1.0 - drop) / 0.25).clamp(0.0, 1.0);
        if fade > 0.0 {
            out.props.push(Prop::Digit {
                index: STOLEN,
                at,
                rot_deg: turn,
                alpha: fade * a,
            });
        }
        if t < 4.7 && a < 1.0 {
            // Cut short while carried: the stolen digit fades in the beak
            // and the clock regrows its own where it lives, so only one
            // copy ever moves.
            DigitFx {
                scale: 1.0 - a,
                alpha: 1.0 - a,
                ..DigitFx::IDENTITY
            }
        } else if t < 4.7 {
            // Carried: its place is empty, the prop draws it in the beak.
            DigitFx {
                dx: at.0 - home.0,
                dy: at.1 - home.1,
                rot_deg: turn,
                alpha: 0.0,
                ..DigitFx::IDENTITY
            }
        } else {
            let grow = kf(t, &[(4.7, 0.0, Lin), (5.0, 1.0, Sb)]);
            DigitFx {
                scale: grow,
                alpha: grow.min(1.0),
                ..DigitFx::IDENTITY
            }
        }
    };
    out.digits[STOLEN] = if (3.55..4.7).contains(&t) { fx } else { toward_rest(fx, a) };
    out.mascot = Some(head);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::ui::geometry::{ShapeRect, WINDOW_H, WINDOW_W};
    use crate::ui::mascot::{CrestPose, beak_open, crest_pose};
    use crate::ui::play::shape::is_rest;
    use crate::ui::play::state::Beat;
    use crate::ui::play::stories::Prop;
    use crate::ui::play::testing::shape;
    use crate::ui::play::trigger::Story;
    use crate::ui::render::DigitFx;
    use crate::ui::robot::{Expr, Pose};

    #[test]
    fn the_story_starts_and_ends_at_rest_in_both_styles() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)] {
                let s = shape(style, w, h);
                for t in [0.0, Story::MalingAngka.secs()] {
                    let shot = shot(style, s, Beat { spread: 1.0, alpha: 1.0, t });
                    assert!(is_rest(style, &shot.deform), "{style:?} {w}x{h} t={t}");
                    assert!(shot.digits.iter().all(|d| *d == DigitFx::IDENTITY), "{style:?} {w}x{h} t={t}");
                }
            }
        }
    }

    #[test]
    fn alpha_zero_means_no_deformation_at_any_moment() {
        let s = shape(Style::Pill, 190.0, 34.0);
        for i in 0..=78 {
            let beat = Beat { spread: 1.0, alpha: 0.0, t: i as f32 / 10.0 };
            assert!(is_rest(Style::Pill, &shot(Style::Pill, s, beat).deform), "t={}", beat.t);
        }
    }

    #[test]
    fn the_stolen_digit_is_carried_and_then_returned() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let mid = shot(Style::Notch, s, Beat { spread: 1.0, alpha: 1.0, t: 4.0 });
        assert!(mid.digits.iter().any(|d| d.dx.abs() > 5.0), "a digit has left its place");
        let end = shot(Style::Notch, s, Beat { spread: 1.0, alpha: 1.0, t: 7.6 });
        assert!(end.digits.iter().all(|d| d.dx.abs() < 1.0), "every digit is home before the end");
    }

    #[test]
    fn nothing_leaves_the_window_at_any_size_or_style() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)] {
                let s = shape(style, w, h);
                let mut t = 0.0;
                while t <= Story::MalingAngka.secs() {
                    let shot = shot(style, s, Beat { spread: 1.0, alpha: 1.0, t });
                    let b = shot.bounds();
                    assert!(b.x >= 0.0 && b.y >= 0.0 && b.x + b.w <= WINDOW_W && b.y + b.h <= WINDOW_H,
                        "{style:?} {w}x{h} t={t}: {b:?}");
                    t += 0.04;
                }
            }
        }
    }

    /// The shots every 10 ms of the story.
    fn every_frame(style: Style, w: f32, h: f32, alpha: f32) -> Vec<(f32, Shot)> {
        let s = shape(style, w, h);
        let secs = Story::MalingAngka.secs();
        (0..=(secs * 100.0) as usize)
            .map(|i| {
                let t = (i as f32 / 100.0).min(secs);
                (t, shot(style, s, Beat { spread: 1.0, alpha, t }))
            })
            .collect()
    }

    #[test]
    fn the_island_never_bends() {
        // `f2` plays inside the island and with the clock: no dent, so no
        // top corner pulls in past the Notch's shoulder tuck.
        for style in [Style::Notch, Style::Pill] {
            for (t, shot) in every_frame(style, 190.0, 34.0, 1.0) {
                assert!(is_rest(style, &shot.deform), "{style:?} t={t}");
            }
        }
    }

    #[test]
    fn the_digit_wiggles_is_bitten_carried_dropped_and_regrown() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let at = |t: f32| shot(Style::Notch, s, Beat { spread: 1.0, alpha: 1.0, t });
        assert!(at(0.9).digits[4].rot_deg.abs() > 0.5 || at(0.95).digits[4].rot_deg.abs() > 0.5, "it wiggles");
        assert!(at(2.5).digits[4].rot_deg.abs() > 1.0, "a tap knocks it");
        // Bitten: gone from its place, in the beak instead.
        let held = at(4.0);
        assert_eq!(held.digits[4].alpha, 0.0);
        assert!(held.props.iter().any(|p| matches!(p, Prop::Digit { index: 4, alpha, .. } if *alpha == 1.0)));
        // The clock regrows it with a little overshoot, then it is home.
        assert!(at(4.75).digits[4].scale > 0.3 && at(4.75).digits[4].scale < 1.0);
        assert!(at(5.0).digits[4].scale >= 0.99);
        // The stolen one falls and is gone a second after the drop.
        let falling = |t: f32| at(t).props.iter().find_map(|p| match p {
            Prop::Digit { at, .. } => Some(*at),
            _ => None,
        });
        let (a, b) = (falling(5.2).unwrap(), falling(5.8).unwrap());
        assert!(b.1 > a.1 + 20.0, "it falls: {a:?} then {b:?}");
        assert!(falling(6.2).is_none());
        // The other characters never move.
        for (_, shot) in every_frame(Style::Notch, 190.0, 34.0, 1.0) {
            assert!(shot.digits[..4].iter().all(|d| *d == DigitFx::IDENTITY));
        }
    }

    #[test]
    fn the_falk_goes_to_the_minute_and_comes_back_without_a_jump() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in [(160.0, 28.0), (300.0, 44.0)] {
                let frames = every_frame(style, w, h, 1.0);
                let head = |m: Option<MascotShot>| m.map_or((0.0, 0.0, 0.0), |m| (m.dx, m.dy, m.rot_deg));
                // At the clock it is well to the right of its slot.
                let s = shape(style, w, h);
                let there = shot(style, s, Beat { spread: 1.0, alpha: 1.0, t: 3.0 }).mascot.unwrap();
                assert!(there.dx > 0.5 * w, "{style:?} {w}x{h}: only {}", there.dx);
                for pair in frames.windows(2) {
                    let ((t0, a), (t1, b)) = (&pair[0], &pair[1]);
                    let (p, q) = (head(a.mascot), head(b.mascot));
                    // The scoot is quick, but never a jump in one 10 ms frame.
                    assert!((q.0 - p.0).abs() < 0.02 * w && (q.1 - p.1).abs() < 1.0, "{style:?} {w}x{h} {t0}..{t1}: {p:?} {q:?}");
                    assert!((q.2 - p.2).abs() < 4.0, "{style:?} {w}x{h} {t0}..{t1}: turned {} to {}", p.2, q.2);
                }
            }
        }
    }

    #[test]
    fn the_crest_shoots_up_on_the_regrow_then_droops_and_settles() {
        let s = shape(Style::Pill, 190.0, 34.0);
        let crest = |t: f32| shot(Style::Pill, s, Beat { spread: 1.0, alpha: 1.0, t }).mascot.unwrap().crest;
        assert!(crest(5.2).scale > 1.6, "startled");
        assert!(crest(6.7).scale < 0.8, "drooping");
        let end = crest(7.79);
        assert!((end.scale - 1.0).abs() < 0.01 && (end.rot_deg - 10.0).abs() < 0.2, "the idle crest: {end:?}");
        assert_eq!(shot(Style::Pill, s, Beat { spread: 1.0, alpha: 1.0, t: 5.2 }).mascot.unwrap().expr, Expr::Alert);
    }

    #[test]
    fn a_wind_down_eases_everything_home() {
        let s = shape(Style::Notch, 190.0, 34.0);
        let full = shot(Style::Notch, s, Beat { spread: 1.0, alpha: 1.0, t: 4.0 });
        let half = shot(Style::Notch, s, Beat { spread: 1.0, alpha: 0.5, t: 4.0 });
        let (f, h) = (full.mascot.unwrap(), half.mascot.unwrap());
        assert!((h.rot_deg - f.rot_deg * 0.5).abs() < 1e-3);
        assert!((h.dx - f.dx * 0.5).abs() < 1e-3 && (h.sx - (1.0 + (f.sx - 1.0) * 0.5)).abs() < 1e-3);
        let gone = shot(Style::Notch, s, Beat { spread: 0.0, alpha: 0.0, t: 4.0 });
        assert_eq!(gone, crate::ui::play::stories::Shot::rest(Style::Notch, s));
    }

    /// The beak of the head `m` draws, from the slot of `s` at `spread`.
    fn drawn_beak(s: ShapeRect, spread: f32, m: &MascotShot) -> (f32, f32) {
        let (rest, k) = head_pivot(s, spread).unwrap();
        beak_at(m, rest, k)
    }

    #[test]
    fn a_wind_down_keeps_the_stolen_digit_in_the_drawn_beak_and_regrows_the_clocks_at_home() {
        // Cut short while the digit is carried (4.0 s) or falling (5.3 s):
        // the stolen one stays in the beak of the head as drawn and fades
        // with it; the clock's own digit 4 regrows where it lives, never
        // travelling, so no second copy crosses the island.
        for style in [Style::Notch, Style::Pill] {
            let s = shape(style, 190.0, 34.0);
            for a in [0.25, 0.5, 0.75, 1.0] {
                for t in [4.0, 5.3] {
                    let case = format!("{style:?} alpha={a} t={t}");
                    let shot = shot(style, s, Beat { spread: 1.0, alpha: a, t });
                    let m = shot.mascot.unwrap();
                    let beak = drawn_beak(s, 1.0, &m);
                    let digits: Vec<_> = shot.props.iter().filter_map(|p| match *p {
                        Prop::Digit { index, at, alpha, .. } => Some((index, at, alpha)),
                        _ => None,
                    }).collect();
                    assert_eq!(digits.len(), 1, "{case}: one stolen digit");
                    let (index, at, alpha) = digits[0];
                    assert_eq!(index, 4, "{case}");
                    let drop = (t - 5.05_f32).max(0.0);
                    let want = (beak.0 + 4.0 * drop, beak.1 + 90.0 * drop * drop);
                    assert!((at.0 - want.0).abs() < 1e-3 && (at.1 - want.1).abs() < 1e-3, "{case}: {at:?} is not at the beak {want:?}");
                    assert!((alpha - a).abs() < 1e-3, "{case}: the stolen digit fades with the head");
                    let home = shot.digits[4];
                    if t < 4.7 && a < 1.0 {
                        assert_eq!((home.dx, home.dy, home.rot_deg), (0.0, 0.0, 0.0), "{case}: the clock's digit stays home");
                        assert!((home.scale - (1.0 - a)).abs() < 1e-3 && (home.alpha - (1.0 - a)).abs() < 1e-3, "{case}: {home:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_whistle_follows_the_drawn_head() {
        let s = shape(Style::Pill, 190.0, 34.0);
        let (rest, _) = head_pivot(s, 1.0).unwrap();
        for a in [0.5, 1.0] {
            let shot = shot(Style::Pill, s, Beat { spread: 1.0, alpha: a, t: 6.55 });
            let m = shot.mascot.unwrap();
            let at = shot.props.iter().find_map(|p| match *p {
                Prop::Whistle { at, .. } => Some(at),
                _ => None,
            }).unwrap();
            let head = (rest.0 + m.dx, rest.1 + m.dy);
            assert!((at.0 - (head.0 + 8.0)).abs() < 1e-3 && (at.1 - (head.1 - 5.0)).abs() < 1e-3, "alpha={a}");
        }
    }

    #[test]
    fn the_mascot_is_the_idle_falk_just_after_the_start_and_just_before_the_end() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in [(160.0, 28.0), (190.0, 34.0), (300.0, 44.0)] {
                let s = shape(style, w, h);
                for t in [0.001, Story::MalingAngka.secs() - 0.001] {
                    let case = format!("{style:?} {w}x{h} t={t}");
                    let m = shot(style, s, Beat { spread: 1.0, alpha: 1.0, t }).mascot.unwrap();
                    assert_eq!(m.expr, Expr::Idle, "{case}");
                    assert!((m.crest.rot_deg - 10.0).abs() < 1e-3 && (m.crest.scale - 1.0).abs() < 1e-3, "{case}: {:?}", m.crest);
                    assert!((m.sx - 1.0).abs() < 1e-3 && (m.sy - 1.0).abs() < 1e-3, "{case}");
                    assert_eq!(m.glance, (0.0, 0.0), "{case}");
                    assert!(!m.blink && !m.beak_open, "{case}");
                    assert!(m.dx.abs() < 1e-3 && m.dy.abs() < 1e-3 && m.rot_deg.abs() < 1e-3, "{case}: {m:?}");
                }
            }
        }
    }

    #[test]
    fn the_story_face_overrides_the_crest_and_opens_the_beak() {
        let m = MascotShot {
            dx: 0.0,
            dy: 0.0,
            rot_deg: 0.0,
            sx: 1.0,
            sy: 1.0,
            expr: Expr::Happy,
            crest: CrestPose { rot_deg: 30.0, scale: 1.5 },
            glance: (2.4, 0.0),
            blink: true,
            beak_open: true,
        };
        let base = Pose { glance_x: 1.0, head_dx_dip: 3.0, ..Pose::default() };
        let pose = m.pose(base);
        assert_eq!(crest_pose(Expr::Happy, &pose), m.crest, "the story's crest wins over the face's");
        assert!(beak_open(Expr::Idle, &pose));
        assert!(!beak_open(Expr::Idle, &Pose::default()));
        // The story decides: a startled (Alert) falk keeps its beak shut
        // unless the story opens it; without a story the Alert face opens it.
        let shut = MascotShot { expr: Expr::Alert, beak_open: false, ..m };
        assert!(!beak_open(Expr::Alert, &shut.pose(Pose::default())));
        assert!(beak_open(Expr::Alert, &Pose::default()));
        assert!(pose.blink);
        assert_eq!((pose.glance_x, pose.head_dx_dip), (3.4, 3.0), "the app's look stays under the story's");
    }
}
