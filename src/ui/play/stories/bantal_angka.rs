//! The cat's "Bantal angka" (the digit pillow): the slot head dives into the
//! island's dark and the cat comes out of it in front of the clock and sits
//! on the lip, which sags under it. It yawns, nods off and jerks awake twice,
//! then rests its head on the minute like a pillow, the digit squashed under
//! its cheek, the lip rising and falling with its breath. The minute turns
//! over: the pillow becomes the next minute and pops, the cat startles, stares
//! at it, smiles and goes back to sleep on it, then hops back into the dark.
//! Timeline, easing and offsets from `docs/design/maskot-notch-v3.html`,
//! story `c2`.

use std::f32::consts::{PI, TAU};

use super::{
    DIGIT_X, Draw, Ease, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, bump, island_box, kept, kf,
    lerp, mix_hop, ring, toward_rest,
};
use crate::ui::play::art::cat::{CatPose, Paw, Tail, cat_at};
use crate::ui::play::shape::{Deform, lip_dent};
use crate::ui::play::trigger::Story;
use crate::ui::render::DigitFx;
use crate::ui::robot::{CREAM, Expr, Seg};

use Ease::{I, Io, Lin, O, Sb, Sio};

/// The clock's character the cat sleeps on: the minute's last.
const PILLOW: usize = 4;
/// The cat sits on the lip this far left of the pillow's centre.
const SEAT_OFF: f32 = 12.7;

/// How asleep the cat is, 0 to 1: the mockup's two naps (5.2 to 6.35 s and
/// 7.15 to 7.75 s), eased over 0.15 s at each edge, so the breath, the tail
/// and the z's never step where the mockup switches them.
fn asleep_k(t: f32) -> f32 {
    kf(
        t,
        &[
            (5.2, 0.0, Lin),
            (5.35, 1.0, Sio),
            (6.2, 1.0, Lin),
            (6.35, 0.0, Sio),
            (7.15, 0.0, Lin),
            (7.3, 1.0, Sio),
            (7.6, 1.0, Lin),
            (7.75, 0.0, Sio),
        ],
    )
}

/// One moment of the story on `stage`, the cat in the coat of its look and
/// the swap showing the clock's real next minute (`stage.minute`).
/// `beat.alpha` eases it home, paced by the wind-down's time (`kept`): over
/// its first 0.58 the cat, the pillow and the z's fade and the lip settles,
/// the clock taking its minute back as the pillow goes; from 0.3 the slot
/// head comes back out of the dark and rises home. Cut short, the story
/// holds where the cut found it. Exactly at rest at 0 s, at the end and
/// with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, minute, .. } = *stage;
    let a = beat.alpha.clamp(0.0, 1.0);
    let t = beat.cut_at.map_or(beat.t, |c| beat.t.min(c));
    let mut out = Shot::rest(style, s);
    if t <= 0.0 || t >= Story::BantalAngka.secs() || a == 0.0 {
        return out;
    }
    // The clock where the renderer puts it: the cat sits by its minute.
    let Some(clock) = stage.room().clock else {
        return out;
    };
    let play = kept(a, 0.0, 0.58);
    let pillow_x = clock.x + clock.w / 2.0 + DIGIT_X[PILLOW];
    let x = pillow_x - SEAT_OFF;
    let (top, mid, bottom) = (s.y, s.y + s.h / 2.0, s.y + s.h);

    // The slot head ducks into the dark and later comes back up, fading with
    // its halo as one. Wound down, it comes back out of the dark where it
    // dived, then rises home.
    let away = kf(t, &[(0.45, 0.0, Lin), (0.8, 1.0, I), (8.05, 1.0, Lin), (8.45, 0.0, Sb)]);
    let duck = away * kept(a, 0.6, 1.0);
    let dark = away.max(0.0) * kept(a, 0.3, 0.88);
    out.slot = Slot::Story(MascotShot {
        dy: 4.0 * duck,
        sx: 1.0 - 0.15 * duck,
        sy: 1.0 - 0.15 * duck,
        alpha: 1.0 - dark,
        ..MascotShot::REST
    });

    let hop = mix_hop(t, 1.0, 7.75);
    let out_of_dark = 1.0 - hop.dk;
    let breath = (TAU * 0.5 * t).sin();
    let asleep = (5.2..6.35).contains(&t) || (7.15..7.75).contains(&t);
    let snooze = asleep_k(t);
    // The head nods off and jerks up twice, then rests on the pillow, is
    // jerked up by the swap and goes back down.
    let droop = kf(
        t,
        &[
            (2.45, 0.0, Lin),
            (3.0, 17.0, Sio),
            (3.08, -4.0, O),
            (3.5, 0.0, Sio),
            (3.9, 0.0, Lin),
            (4.55, 21.0, Sio),
            (4.63, -5.0, O),
            (5.0, 0.0, Sio),
            (5.25, 0.0, Lin),
            (5.75, 33.0, Sio),
            (6.35, 33.0, Lin),
            (6.42, -8.0, O),
            (6.7, -2.0, Sio),
            (7.05, -2.0, Lin),
            (7.4, 33.0, Sio),
        ],
    );
    let yawn = kf(t, &[(1.7, 0.0, Lin), (2.05, 1.0, Sio), (2.3, 1.0, Lin), (2.5, 0.0, Sio)]);
    let startle = bump(t, 3.0, 0.3)
        .max(bump(t, 4.55, 0.3))
        .max(kf(t, &[(6.35, 0.0, Lin), (6.42, 1.0, O), (6.8, 1.0, Lin), (7.0, 0.0, Io)]));

    // The island: the lip sags under the cat, rising and falling with its
    // breath while it sleeps.
    let sag = (1.8 * out_of_dark + 0.35 * breath * snooze) * play;
    out.deform = Deform { dents: vec![lip_dent(style, s, x, 12.0, sag)], ..Deform::default() };
    out.island = island_box(style, s, &out.deform);

    // The face in the app's own expressions: the mockup's sleepy and closed
    // eyes are a blink, its round eyes Alert, its squint Focus. Its yawning
    // mouth and its ear turns have no art here and are left out.
    let (expr, shut) = if (7.1..7.3).contains(&t) {
        (Expr::Happy, false)
    } else if (6.8..7.1).contains(&t) {
        (Expr::Focus, false)
    } else if startle > 0.4 {
        (Expr::Alert, false)
    } else {
        (Expr::Idle, yawn > 0.3 || droop > 6.0 || asleep)
    };
    let face = MascotShot { expr, blink: shut || blinks(t, &[1.6]) > 0.25, ..MascotShot::REST };

    // The cat, in front of the clock, faded in as it comes out of the dark
    // (the mockup draws it dark at once, covering the clock in one frame).
    let lean = ((droop - 22.0) / 11.0).clamp(0.0, 1.0);
    if hop.dk < 0.99 {
        let sq = hop.sq + 0.02 * breath * snooze + 0.06 * startle;
        let tail = Tail {
            wag: lerp(18.0 * (TAU * 0.8 * t).sin(), 8.0 * (TAU * 0.4 * t).sin(), snooze),
            puff: 1.0 + if t > 6.3 { 0.7 * startle } else { 0.0 },
            ..Tail::default()
        };
        let pose = CatPose {
            dk: hop.dk,
            rise: None,
            head: (2.0 * lean, 2.0 * lean + 0.5 * breath * snooze, droop),
            face,
            paws: [Paw::default(); 2],
            tail: Some(tail),
        };
        out.parts.push(Part {
            layer: Layer::Front,
            draw: cat_at((x, bottom + sag + hop.y), 3.0 * lean, hop.sc / sq.sqrt(), hop.sc * sq, &pose, look),
            alpha: out_of_dark * play,
        });
    }

    // The pillow: the clock's minute, drawn over the cat in its stead from
    // before the cat comes out until after it has gone, squashed and tipped
    // under its cheek. At the swap the real next minute crosses in over it
    // and pops up with a ring; straightened once the cat has gone, it hands
    // the clock its own minute back.
    let held = kf(t, &[(0.6, 0.0, Lin), (0.9, 1.0, Lin), (8.15, 1.0, Lin), (8.45, 0.0, Lin)]) * play;
    out.digits[PILLOW] = DigitFx { alpha: 1.0 - held, ..DigitFx::IDENTITY };
    if held > 0.0 {
        let leave = kf(t, &[(7.8, 1.0, Lin), (8.1, 0.0, Sio)]);
        let squash = lean * leave;
        let pop = bump(t, 6.36, 0.22);
        let fx = toward_rest(
            DigitFx {
                dx: 1.6 * squash,
                dy: 0.8 * squash - 3.0 * pop,
                rot_deg: 10.0 * squash + ring(t, 6.4, -12.0, 3.0, 4.0) * leave,
                scale: 1.0 + 0.35 * pop,
                scale_x: 1.0 - 0.18 * squash,
                ..DigitFx::IDENTITY
            },
            play,
        );
        let next = minute.and_then(|m| m.to_digit(10)).and_then(|d| char::from_digit((d + 1) % 10, 10));
        let swap = if next.is_some() { kf(t, &[(6.3, 0.0, Lin), (6.42, 1.0, Lin)]) } else { 0.0 };
        let near = (pillow_x, mid);
        for (ch, k) in [(None, 1.0 - swap), (next, swap)] {
            if k > 0.0 {
                let draw = Draw::ClockChar { index: PILLOW, fx: DigitFx { ch, ..fx }, near };
                out.parts.push(Part { layer: Layer::Front, draw, alpha: held * k });
            }
        }
    }

    // The z's while it sleeps, rising up and right from over its head; on
    // the Pill they float out through the gap above it.
    if snooze > 0.0 {
        for i in 0..2 {
            let d = (t - 5.2 + 0.9 * i as f32).rem_euclid(1.8) / 1.8;
            let size = 2.2 + 1.4 * d;
            let (zx, zy) = (x + 14.0 + 10.0 * d, top + 4.0 - 5.0 * d);
            let path = vec![Seg::Move(zx, zy), Seg::Line(zx + size, zy), Seg::Line(zx, zy + size), Seg::Line(zx + size, zy + size)];
            out.parts.push(Part {
                layer: Layer::Front,
                draw: Draw::Stroke { path, rgb: CREAM, width: 0.75 },
                alpha: (PI * d).sin() * 0.8 * snooze * play,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::theme::Mascot;
    use crate::ui::cat_style::CatLook;
    use crate::ui::geometry::ShapeRect;
    use crate::ui::play::rim::rim;
    use crate::ui::play::shape::is_rest;
    use crate::ui::play::state::Beat;
    use crate::ui::play::stories::{DIGIT_X, Draw, Layer, MascotShot, Part, Slot, wind_down_frames};
    use crate::ui::play::testing::shape;
    use crate::ui::play::trigger::Story;
    use crate::ui::render::DigitFx;
    use crate::ui::robot::{CREAM, Expr};

    crate::ui::play::stories::story_contract_tests!(Story::BantalAngka, shot);

    const SIZES: [(f32, f32); 3] = crate::ui::play::stories::CONTRACT_SIZES;

    /// The idle island `w` x `h` with the cat and the clock at 14:01.
    fn stage(style: Style, w: f32, h: f32, beat: Beat) -> Stage {
        let look = CatLook { mascot: Mascot::Cat, ..CatLook::default() };
        Stage::idle(style, shape(style, w, h), beat, look, true)
    }

    /// At `t`, wound down to `alpha` (the island spread open).
    fn at(style: Style, w: f32, h: f32, t: f32, alpha: f32) -> Shot {
        shot(&stage(style, w, h, Beat { spread: 1.0, alpha, t, cut_at: None }))
    }

    /// The default island at `t`, playing.
    fn now(t: f32) -> Shot {
        at(Style::Notch, 190.0, 34.0, t, 1.0)
    }

    /// The cat's seat on the lip: 12.7 left of the minute's centre.
    fn seat_x(style: Style, w: f32, h: f32) -> f32 {
        let st = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t: 3.0, cut_at: None });
        let c = st.room().clock.unwrap();
        c.x + c.w / 2.0 + DIGIT_X[4] - 12.7
    }

    /// The deepest dent's place on the lip (x) and its depth (about dip).
    fn dent(shot: &Shot, style: Style, s: ShapeRect) -> Option<(f32, f32)> {
        shot.deform.dents.iter().max_by(|a, b| a.depth.total_cmp(&b.depth)).map(|d| (rim(style, s, d.u).x, d.depth * s.h))
    }

    /// The pillow's parts: its effect and its alpha.
    fn pillows(shot: &Shot) -> Vec<(DigitFx, f32)> {
        shot.parts
            .iter()
            .filter_map(|p| match p.draw {
                Draw::ClockChar { index: 4, fx, .. } => Some((fx, p.alpha)),
                _ => None,
            })
            .collect()
    }

    /// The seated cat's head, deep in its figure.
    fn face(shot: &Shot) -> Option<MascotShot> {
        fn find(d: &Draw) -> Option<MascotShot> {
            match d {
                Draw::Head { shot, .. } => Some(*shot),
                Draw::Group { items, .. } | Draw::Clipped { items, .. } => items.iter().find_map(find),
                _ => None,
            }
        }
        shot.parts.iter().filter(|p| matches!(p.draw, Draw::Group { .. })).find_map(|p| find(&p.draw))
    }

    #[test]
    fn the_lip_breathes_and_settles() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let case = format!("{style:?} {w}x{h}");
                // Under the cat, pressed down by its weight while it sits.
                let (x, depth) = dent(&at(style, w, h, 1.6, 1.0), style, s).unwrap();
                assert!((x - seat_x(style, w, h)).abs() < 0.5, "{case}: the dent at {x}");
                assert!(depth > 0.5, "{case}: {depth}");
                // Awake it holds still; asleep it rises and falls with the breath.
                let depths = |from: f32, to: f32| {
                    let mut v = Vec::new();
                    let mut t = from;
                    while t <= to {
                        v.push(dent(&at(style, w, h, t, 1.0), style, s).unwrap().1);
                        t += 0.02;
                    }
                    v
                };
                let swing = |v: &[f32]| v.iter().copied().fold(f32::MIN, f32::max) - v.iter().copied().fold(f32::MAX, f32::min);
                assert!(swing(&depths(1.5, 1.68)) < 1e-4, "{case}: it breathes awake");
                assert!(swing(&depths(5.4, 6.2)) > 0.2 * depth, "{case}: it does not breathe asleep");
                assert!(swing(&depths(7.32, 7.6)) > 0.02 * depth, "{case}: not asleep again");
                // No step anywhere, falling asleep and waking included.
                let mut t = 0.0;
                let mut last = 0.0;
                while t < Story::BantalAngka.secs() {
                    let d = dent(&at(style, w, h, t, 1.0), style, s).map_or(0.0, |d| d.1);
                    let most = if (1.0..1.5).contains(&t) || (7.75..8.25).contains(&t) { 0.3 } else { 0.06 };
                    assert!((d - last).abs() < most * depth, "{case} t={t}: from {last} to {d}");
                    last = d;
                    t += 1.0 / 60.0;
                }
                // Gone into the dark, the lip is back at rest well before the end.
                for t in [8.2, 8.4, 8.59] {
                    assert!(is_rest(style, &at(style, w, h, t, 1.0).deform), "{case} t={t}");
                }
            }
        }
    }

    #[test]
    fn the_minute_swap_crosses_two_texts() {
        // Before the swap the pillow is the clock's own minute.
        let one = pillows(&now(6.2));
        assert_eq!(one.len(), 1, "{one:?}");
        assert_eq!((one[0].0.ch, one[0].1), (None, 1.0));
        // At the swap two texts cross in the same place: the clock's own
        // fading out, its real next minute fading in.
        let both = pillows(&now(6.36));
        assert_eq!(both.len(), 2, "{both:?}");
        let ((old, a), (new, b)) = (both[0], both[1]);
        assert_eq!((old.ch, new.ch), (None, Some('2')));
        assert!(a > 0.1 && b > 0.1 && (a + b - 1.0).abs() < 1e-4, "{a} + {b}");
        assert_eq!(DigitFx { ch: None, ..new }, old, "not in the same place");
        // After it, the next minute alone, popped up, with a little overshoot.
        let after = pillows(&now(6.47));
        assert_eq!(after.len(), 1, "{after:?}");
        assert_eq!(after[0].0.ch, Some('2'));
        assert!(after[0].0.scale > 1.3 && after[0].0.dy < -2.0, "{:?}", after[0].0);
        let turn: Vec<f32> = [6.45, 6.55, 6.65, 6.75].iter().map(|&t| pillows(&now(t))[0].0.rot_deg).collect();
        assert!(turn.iter().any(|r| *r < -5.0) && turn.iter().any(|r| *r > 3.0), "no ring: {turn:?}");
        // The real next minute, whatever the clock shows: 9 rolls over to 0.
        let nine = Stage { minute: Some('9'), ..stage(Style::Pill, 190.0, 34.0, Beat { spread: 1.0, alpha: 1.0, t: 6.6, cut_at: None }) };
        assert_eq!(pillows(&shot(&nine))[0].0.ch, Some('0'));
        // Without a minute to read, no swap.
        let blind = |t: f32| Stage { minute: None, ..stage(Style::Notch, 190.0, 34.0, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }) };
        for t in [6.36, 6.6, 7.5] {
            assert!(pillows(&shot(&blind(t))).iter().all(|(fx, _)| fx.ch.is_none()), "t={t}");
        }
    }

    #[test]
    fn the_pillow_stands_in_for_the_clocks_minute_with_no_gap() {
        // The clock's own minute and the pillow add up to one whole digit at
        // every moment, playing or wound down; at rest the clock has it back.
        let check = |shot: &Shot, case: &str| {
            let pillow: f32 = pillows(shot).iter().map(|(_, a)| a).sum();
            let own = shot.digits[4];
            assert!((own.alpha + pillow - 1.0).abs() < 1e-4, "{case}: {} + {pillow}", own.alpha);
            assert_eq!(DigitFx { alpha: 1.0, ..own }, DigitFx::IDENTITY, "{case}");
            for (i, d) in shot.digits.iter().enumerate().filter(|(i, _)| *i != 4) {
                assert_eq!(*d, DigitFx::IDENTITY, "{case}: digit {i}");
            }
        };
        let mut t = 0.0;
        while t < Story::BantalAngka.secs() {
            check(&now(t), &format!("t={t}"));
            t += 0.01;
        }
        // Over the cat from before it comes out until after it has gone.
        assert_eq!(pillows(&now(0.95)).iter().map(|p| p.1).sum::<f32>(), 1.0);
        assert_eq!(pillows(&now(8.12)).iter().map(|p| p.1).sum::<f32>(), 1.0);
        assert!(pillows(&now(8.5)).is_empty() && pillows(&now(0.5)).is_empty());
        // Straight and in its own place again before it hands back.
        let back = pillows(&now(8.14))[0].0;
        assert_eq!(DigitFx { ch: None, ..back }, DigitFx::IDENTITY, "{back:?}");
        for cut in [3.0, 5.9, 6.37, 7.0] {
            for (ms, beat, shot) in wind_down_frames(Story::BantalAngka, cut, 0.0, |beat| shot(&stage(Style::Pill, 190.0, 34.0, beat))) {
                check(&shot, &format!("cut {cut} +{ms:.1} ms (alpha {})", beat.alpha));
            }
        }
    }

    #[test]
    fn the_cat_sits_in_front_of_the_clock_under_its_pillow() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let shot = at(style, w, h, 5.9, 1.0);
                // The cat, the pillow over it, the z's over both, all in front.
                let kinds: Vec<(Layer, u8)> = shot
                    .parts
                    .iter()
                    .map(|p| (p.layer, match p.draw { Draw::Group { .. } => 0, Draw::ClockChar { .. } => 1, Draw::Stroke { .. } => 2, _ => 9 }))
                    .collect();
                assert_eq!(kinds, [(Layer::Front, 0), (Layer::Front, 1), (Layer::Front, 2), (Layer::Front, 2)], "{case}");
                // Its head rests on the pillow: the pillow squashed and tipped.
                assert!(face(&shot).unwrap().blink, "{case}: asleep");
                let p = pillows(&shot)[0].0;
                assert!(p.scale_x < 0.9 && p.rot_deg > 8.0, "{case}: {p:?}");
                // Its slot is dark: the slot head dived.
                assert!(matches!(shot.slot, Slot::Story(m) if m.alpha == 0.0), "{case}");
            }
        }
    }

    #[test]
    fn the_cat_comes_out_of_the_dark_and_goes_back_without_a_pop() {
        let cat = |t: f32| now(t).parts.iter().find(|p| matches!(p.draw, Draw::Group { .. })).map(|p| p.alpha);
        assert!(cat(0.99).is_none() && cat(8.3).is_none());
        let mut last = 0.0;
        let mut t = 0.9;
        while t < 8.4 {
            let a = cat(t).unwrap_or(0.0);
            assert!((a - last).abs() <= 0.15, "t={t}: from {last} to {a}");
            last = a;
            t += 1.0 / 60.0;
        }
        assert_eq!(cat(1.5), Some(1.0));
    }

    #[test]
    fn the_faces_are_the_mockups_in_the_apps_expressions() {
        // Open eyes; the yawn's and the sleep's shut eyes (a blink); startled
        // round eyes; the squint (Focus) at the new minute; happy; asleep.
        let f = |t: f32| face(&now(t)).unwrap_or_else(|| panic!("no cat at {t}"));
        assert!(f(1.5).expr == Expr::Idle && !f(1.5).blink);
        assert!(f(1.6).blink, "the blink");
        for (t, expr, shut) in [
            (2.1, Expr::Idle, true),
            (2.9, Expr::Idle, true),
            (3.12, Expr::Alert, false),
            (3.7, Expr::Idle, false),
            (5.9, Expr::Idle, true),
            (6.5, Expr::Alert, false),
            (6.9, Expr::Focus, false),
            (7.2, Expr::Happy, false),
            (7.5, Expr::Idle, true),
        ] {
            assert_eq!((f(t).expr, f(t).blink), (expr, shut), "t={t}");
        }
    }

    #[test]
    fn the_zzz_float_out_while_it_sleeps() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let case = format!("{style:?} {w}x{h}");
                let zs = |t: f32| -> Vec<Part> { at(style, w, h, t, 1.0).parts.into_iter().filter(|p| matches!(p.draw, Draw::Stroke { .. })).collect() };
                for t in [1.5, 4.0, 6.6, 7.0, 8.0] {
                    assert!(zs(t).is_empty(), "{case} t={t}: z's awake");
                }
                // Cream, up and to the right of the cat's head, out over the
                // top of the island (on the Pill through the gap above it).
                let mut top = f32::MAX;
                let mut t = 5.4;
                while t < 6.2 {
                    for p in zs(t) {
                        let Draw::Stroke { rgb, .. } = p.draw else { unreachable!() };
                        assert_eq!(rgb, CREAM, "{case}");
                        let b = p.draw.bounds();
                        assert!(b.x > seat_x(style, w, h) + 10.0, "{case} t={t}: {b:?}");
                        if p.alpha > 0.2 {
                            top = top.min(b.y);
                        }
                    }
                    t += 0.05;
                }
                assert!(top < s.y + 1.0, "{case}: the z's stay low ({top})");
                // They fade in and out as it falls asleep and wakes: no pop.
                for (a, b) in [(5.19, 5.21), (6.34, 6.36), (7.14, 7.16), (7.74, 7.76)] {
                    for p in zs(a).iter().chain(&zs(b)) {
                        assert!(p.alpha < 0.15, "{case} {a}..{b}: {}", p.alpha);
                    }
                }
            }
        }
    }

    #[test]
    fn the_slot_head_dives_into_the_dark_and_comes_back() {
        let slot = |t: f32| match now(t).slot {
            Slot::Story(m) => m,
            other => panic!("t={t}: {other:?}"),
        };
        assert_eq!(slot(0.4), MascotShot::REST);
        assert!(slot(0.7).alpha > 0.0 && slot(0.7).alpha < 1.0 && slot(0.7).dy > 1.0);
        assert_eq!((slot(0.85).alpha, slot(5.0).alpha, slot(8.0).alpha), (0.0, 0.0, 0.0));
        assert_eq!(slot(8.5), MascotShot::REST);
    }
}
