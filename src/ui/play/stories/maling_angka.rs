//! The falk's "Maling angka": the minute's last digit wiggles like a bug,
//! the falk tilts its head at it, scoots over inside the island, taps it
//! with its beak, bites it and carries it home like a twig. The clock
//! regrows the digit, the crest shoots up in surprise, the stolen one drops
//! and falls away, the crest droops and the falk whistles as if nothing
//! happened. Timeline, easing and offsets from `docs/design/maskot-notch-v3.html`,
//! story `f2`.

use std::f32::consts::{PI, TAU};

use super::{Ease, MascotShot, Prop, Shot, blinks, bump, kf, lerp, ring, toward_rest};
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
/// The head's pivot on its grid.
const PIVOT: (f32, f32) = (32.0, 40.0);

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
    let (Some(slot), Some(clock)) = (room.robot, room.clock) else {
        return out;
    };
    let k = slot.w / GRID;
    let rest = (slot.x + PIVOT.0 * k, slot.y + PIVOT.1 * k);
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
    let startle = kf(t, &[(4.95, 0.0, Lin), (5.05, 1.0, O), (5.5, 1.0, Io), (6.7, 0.0, Sio)]);
    // Leaning at the digit while it is over there.
    let lean = if go > 0.98 && t < 3.75 {
        -18.0 * kf(t, &[(2.05, 0.0, Lin), (2.2, 1.0, Sio), (3.5, 1.0, Io), (3.75, 0.0, Io)])
    } else {
        0.0
    };
    let rot = 32.0 * curious + lean - 14.0 * tap
        + 6.0 * (TAU * 3.0 * t).sin() * scoot
        + ring(t, 5.0, -10.0, 3.0, 3.0)
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
        + ring(t, 3.6, 10.0, 2.4, 3.0)
        + ring(t, 5.03, 14.0, 2.4, 2.5)
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
    let squash = 1.0 + ring(t, 5.03, 0.08, 3.0, 5.0) - 0.05 * hop * scoot;
    let size = lerp(1.0, AWAY_SCALE, go);
    let (sx, sy) = (size / squash.sqrt(), size * squash);
    // The beak's tip, turned and scaled with the head.
    let (sin, cos) = rot.to_radians().sin_cos();
    let (bx, by) = ((BEAK.0 - PIVOT.0) * k * sx, (BEAK.1 - PIVOT.1) * k * sy);
    let beak = (x + bx * cos - by * sin, y + bx * sin + by * cos);

    out.mascot = Some(MascotShot {
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
    });
    if whistle > 0.0 {
        out.props.push(Prop::Whistle {
            at: (x + 8.0, y - 5.0),
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
                rot + 80.0 + 420.0 * drop,
            )
        } else {
            (beak, rot + 80.0)
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
        if t < 4.7 {
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
    out.digits[STOLEN] = toward_rest(fx, a);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::ui::geometry::{WINDOW_H, WINDOW_W};
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
        assert!((half.digits[4].dx - full.digits[4].dx * 0.5).abs() < 1e-3);
        assert!((half.digits[4].alpha - 0.5).abs() < 1e-3, "the digit fades back in at home");
        let gone = shot(Style::Notch, s, Beat { spread: 0.0, alpha: 0.0, t: 4.0 });
        assert_eq!(gone, crate::ui::play::stories::Shot::rest(Style::Notch, s));
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
        assert!(pose.blink);
        assert_eq!((pose.glance_x, pose.head_dx_dip), (3.4, 3.0), "the app's look stays under the story's");
    }
}
