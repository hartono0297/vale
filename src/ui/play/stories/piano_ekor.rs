//! The cat's "Piano ekor" (the tail piano): the slot head ducks into the
//! dark and the cat comes out of it beside the clock; its tail rises in an
//! arc over its head and taps the clock's digits one by one like piano keys
//! (each digit dips, the lip under it lights up, a note floats off), the
//! head nodding along with its eyes shut in delight. A sweep of the tail
//! from left to right makes every digit jump in a wave and lights the edge
//! all the way round the right end. The last note is too hard: the "1"
//! falls over, the cat winces, its tail stands the digit up again, the cat
//! bows and goes back into the dark. Timeline, easing and offsets from
//! `docs/design/maskot-notch-v3.html`, story `c3`.

use std::f32::consts::TAU;

use super::syal_ekor::mix_hop;
use super::{Draw, Ease, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, bump, island_box, kept, kf, lerp, ring, toward_rest};
use crate::config::Style;
use crate::ui::geometry::{ShapeRect, corner_radius};
use crate::ui::play::art::cat::{CatPose, Paw, cat_at, slim_tail, tail};
use crate::ui::play::art::tube::{along, catmull_rom, plen};
use crate::ui::play::shape::{Deform, lip_dent};
use crate::ui::play::trigger::Story;
use crate::ui::render::DigitFx;
use crate::ui::robot::{CREAM, Expr, Seg};

use Ease::{I, Io, Lin, O, Sb, Sio};

/// The clock's character centres from the clock box's centre (DM Sans at
/// 16, `%H:%M`), as the mockup measures them: where the tail taps and the
/// lip lights. The characters themselves are placed by DirectWrite.
const DIGIT_X: [f32; 5] = [-17.2, -7.6, 0.0, 7.6, 17.2];
/// The notes the tail plays: when, and on which character.
const NOTES: [(f32, usize); 8] = [(2.05, 0), (2.35, 1), (2.65, 3), (2.95, 4), (3.4, 3), (3.7, 1), (4.0, 0), (4.3, 1)];
/// The tail's sweep from left to right: its start and end.
const SWEEP: (f32, f32) = (4.62, 5.05);
/// The last note, too hard: it knocks the minute's last digit over.
const LAST: f32 = 5.6;
/// The lit lip and edge, the mockup's `#F6D04A`.
const LIGHT: u32 = 0xF6D04A;

/// How far into the sweep's press the tail and the digits are. The mockup
/// switches it on and off at once; here it eases over 60 ms each way, so
/// neither the tail's tip nor a digit jumps.
fn sweeping(t: f32) -> f32 {
    kf(t, &[(SWEEP.0, 0.0, Lin), (SWEEP.0 + 0.06, 1.0, Sio), (SWEEP.1 - 0.06, 1.0, Lin), (SWEEP.1, 0.0, Sio)])
}

/// Where the tail's tip hovers (x): over each key just before its note,
/// along the clock on the sweep, over the fallen digit while it stands it
/// up, then over the colon as it draws back.
fn tip_x(t: f32, dig: &[f32; 5], right: f32) -> f32 {
    let mut keys = vec![(1.6, dig[0], Lin)];
    keys.extend(NOTES.iter().map(|&(tn, i)| (tn - 0.1, dig[i], Sio)));
    keys.extend([
        (SWEEP.0, dig[0] - 4.0, Sio),
        (SWEEP.1, right - 9.0, Lin),
        (LAST - 0.25, dig[4], Sio),
        (6.25, dig[4], Io),
        (6.45, dig[4] + 3.0, Sio),
        (7.0, dig[2], Sio),
    ]);
    kf(t, &keys)
}

/// How low the tip dips, 0 to 1.4: each note, the sweep's press, the hard
/// last note, the push that stands the digit up.
fn tap(t: f32) -> f32 {
    NOTES
        .iter()
        .map(|&(tn, _)| bump(t, tn - 0.08, 0.18))
        .fold(0.0, f32::max)
        .max(0.7 * sweeping(t))
        .max(1.4 * bump(t, LAST - 0.06, 0.18))
        .max(0.6 * bump(t, 6.3, 0.3))
}

/// A floating note (the mockup's `noteMark`) from `at`, `k` of the way up
/// its 7 dip rise.
fn note(at: (f32, f32), k: f32) -> Draw {
    let head = Draw::Group {
        at: (0.0, 0.0),
        rot_deg: -20.0,
        scale: (1.0, 1.0),
        pivot: (0.0, 0.0),
        items: vec![Draw::Ellipse { c: (0.0, 0.0), r: (1.4, 1.0), rgb: CREAM }],
    };
    let stem = vec![Seg::Move(1.2, -0.2), Seg::Line(1.2, -5.2), Seg::Quad(2.8, -4.6, 3.6, -3.2)];
    Draw::Group {
        at: (at.0, at.1 - 7.0 * k),
        rot_deg: 0.0,
        scale: (1.0, 1.0),
        pivot: (0.0, 0.0),
        items: vec![head, Draw::Stroke { path: stem, rgb: CREAM, width: 0.7 }],
    }
}

/// How far the tail's centre line keeps inside the island's edge: half
/// its widest (6.4 x 0.42, 5 % more with its fluff), the Black cat's rim
/// 0.8 past that, and a little to spare.
const TAIL_PAD: f32 = 2.4;

/// `p` kept `pad` inside the island's upper half: under its top edge and,
/// on the Pill, inside its half circles (pulled toward the end's centre), so
/// a tail about it never shows in the gap above the island. The mockup ties
/// the arc to the flat top only; the sweep takes the tip into the right end.
fn keep_in(style: Style, s: ShapeRect, p: (f32, f32), pad: f32) -> (f32, f32) {
    let (x, y) = (p.0, p.1.max(s.y + pad));
    if style != Style::Pill {
        return (x, y);
    }
    let (r, mid, right) = (corner_radius(style, s), s.y + s.h / 2.0, s.x + s.w);
    let cx = if x < s.x + r {
        s.x + r
    } else if x > right - r {
        right - r
    } else {
        return (x, y);
    };
    let (dx, dy) = (x - cx, y - mid);
    let (d, m) = (dx.hypot(dy), r - pad);
    if dy >= 0.0 || d <= m { (x, y) } else { (cx + dx * m / d, mid + dy * m / d) }
}

/// `items` (window dip) as one figure in `layer`.
fn figure(layer: Layer, items: Vec<Draw>, alpha: f32) -> Part {
    Part { layer, draw: Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items }, alpha }
}

/// One moment of the story on `stage`, the cat in the coat of its look.
/// `beat.alpha` eases it home, paced by the wind-down's time (`kept`): over
/// its first 0.58 the cat goes back into the dark, everything it drew fades
/// and the clock and the island settle; from 0.3 to 0.88 the slot head
/// comes back out of the dark where it ducked, and from 0.6 it rises home.
/// Cut short, the story holds where the cut found it, so nothing it had not
/// brought out shows. Exactly at rest at 0 s, at the end and with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, .. } = *stage;
    let (t, a) = (beat.cut_at.map_or(beat.t, |c| beat.t.min(c)), beat.alpha.clamp(0.0, 1.0));
    let mut out = Shot::rest(style, s);
    if t <= 0.0 || t >= Story::PianoEkor.secs() || a == 0.0 {
        return out;
    }
    // The clock where the renderer puts it: the story plays on its digits.
    let Some(clock) = stage.room().clock else {
        return out;
    };
    let play = kept(a, 0.0, 0.58);

    // The slot head ducks into the dark and later comes back up, fading with
    // its halo as one.
    let away = kf(t, &[(0.45, 0.0, Lin), (0.8, 1.0, I), (7.85, 1.0, Lin), (8.25, 0.0, Sb)]);
    let duck = away * kept(a, 0.6, 1.0);
    let dark = away.max(0.0) * kept(a, 0.3, 0.88);
    out.slot = Slot::Story(MascotShot {
        dy: 4.0 * duck,
        sx: 1.0 - 0.15 * duck,
        sy: 1.0 - 0.15 * duck,
        alpha: 1.0 - dark,
        ..MascotShot::REST
    });

    let dig: [f32; 5] = std::array::from_fn(|i| clock.x + clock.w / 2.0 + DIGIT_X[i]);
    let (top, bottom, right) = (s.y, s.y + s.h, s.x + s.w);
    // The cat sits 16 dip left of the first digit, coming out of the dark.
    let x = dig[0] - 16.0;
    let hop = mix_hop(t, 1.0, 7.5);
    let dk = lerp(1.0, hop.dk, play);
    let out_of_dark = 1.0 - dk;
    let tip_x = tip_x(t, &dig, right);

    // The clock: each key dips under its note, every character jumps as the
    // sweep runs over it, the last one falls over and is stood up again.
    let fall = kf(t, &[(LAST, 0.0, Lin), (5.95, 1.0, O), (6.3, 1.0, Io), (6.65, 0.0, Sb)]);
    let (sweep_from, sweep_len) = (dig[0] - 4.0, right - 9.0 - (dig[0] - 4.0));
    let mut glow = [0.0_f32; 5];
    for (i, &cx) in dig.iter().enumerate() {
        let mut dy = 0.0;
        for &(tn, j) in &NOTES {
            if j == i {
                dy += 2.4 * bump(t, tn - 0.04, 0.22);
                glow[i] = glow[i].max(bump(t, tn - 0.04, 0.5));
            }
        }
        dy -= 3.0 * (-((tip_x - cx) / 5.0).powi(2)).exp() * sweeping(t);
        let lit_at = SWEEP.0 + (cx - sweep_from) / sweep_len * (SWEEP.1 - SWEEP.0);
        glow[i] = glow[i].max(bump(t, lit_at, 0.45));
        let mut rot_deg = 0.0;
        if i == 4 {
            dy += 2.0 * bump(t, LAST - 0.04, 0.2) + 3.2 * fall;
            rot_deg = 92.0 * fall;
            glow[i] = glow[i].max(bump(t, LAST - 0.04, 0.5));
        }
        out.digits[i] = toward_rest(DigitFx { dy, rot_deg, ..DigitFx::IDENTITY }, play);
    }

    // The island: the lip pressed down where the cat sits, and a small
    // bounce after the sweep. The mockup scales the island's height by
    // `1 + nb`; `Deform` has no height scale, so a broad dent moves the lip
    // by as much (the Notch's whole height, the Pill's half about its
    // centre). The ring has died to under 0.005 dip by 6.6 s and stops there.
    let nb = if (SWEEP.1..6.6).contains(&t) { ring(t, SWEEP.1, 0.03, 2.4, 3.5) } else { 0.0 };
    let reach = if style == Style::Pill { s.h / 2.0 } else { s.h };
    out.deform = Deform {
        dents: vec![
            lip_dent(style, s, x, 11.0, 1.8 * out_of_dark),
            lip_dent(style, s, s.x + s.w / 2.0, s.w / 2.0, reach * nb * play),
        ],
        ..Deform::default()
    };
    out.island = island_box(style, s, &out.deform);
    if play <= 0.0 {
        return out;
    }

    // The cat, inside (under the clock). Its face in the app's own
    // expressions: the mockup's happy eyes are Happy, its squint Focus, its
    // closed eyes a blink; its ear turns have no art here and are left out.
    if out_of_dark > 0.0 {
        let playing = (1.95..5.2).contains(&t);
        // The head's nodding sway eases in and out (the mockup starts and
        // stops it at once, a jump of up to 6 degrees).
        let groove = kf(t, &[(1.95, 0.0, Lin), (2.1, 1.0, Sio), (5.05, 1.0, Lin), (5.2, 0.0, Sio)]);
        let wince = kf(t, &[(5.62, 0.0, Lin), (5.72, 1.0, O), (6.2, 1.0, Io), (6.4, 0.0, Io)]);
        let bow = kf(t, &[(6.7, 0.0, Lin), (6.95, 1.0, Sio), (7.15, 1.0, Io), (7.35, 0.0, Sio)]);
        let nod = if playing { NOTES.iter().map(|&(tn, _)| bump(t, tn - 0.05, 0.2)).fold(0.0, f32::max) } else { 0.0 };
        let expr = if bow > 0.4 {
            Expr::Idle
        } else if wince > 0.4 {
            Expr::Focus
        } else if playing {
            Expr::Happy
        } else {
            Expr::Idle
        };
        let blink = bow > 0.4 || blinks(t, &[1.6, 7.6]) > 0.25;
        let look_x = if playing || wince > 0.0 { 2.4 } else { 0.0 };
        let face = MascotShot { expr, glance: (look_x, 0.0), blink, ..MascotShot::REST };
        let turn = 6.0 * (TAU * 1.65 * t).sin() * groove - 10.0 * wince + 14.0 * bow;
        let pose = CatPose {
            dk,
            rise: None,
            head: (0.0, 1.2 * nod + 5.0 * bow, turn),
            face,
            paws: [Paw::default(); 2],
            tail: None,
        };
        let sq = hop.sq - 0.03 * nod;
        out.parts.push(Part {
            layer: Layer::Inside,
            draw: cat_at((x, bottom + 1.8 * out_of_dark + hop.y), 4.0 * bow, hop.sc / sq.sqrt(), hop.sc * sq, &pose, look),
            alpha: out_of_dark,
        });
    }

    // The tail, in front: up from behind the cat in an arc whose height is
    // tied to the island's top edge (so on the Pill it stays inside), its tip
    // dipping onto the keys. Its width grows over the first fifth of its
    // reach and shrinks over the last (the mockup draws it whole at once).
    let ext = kf(t, &[(1.35, 0.0, Lin), (1.85, 1.0, Sio), (7.05, 1.0, Lin), (7.45, 0.0, Sio)]);
    if ext > 0.01 {
        let hi = top + 4.5;
        let tip = (tip_x, hi + 4.5 * tap(t));
        let (k1, k2) = ((x + 15.0, bottom - 21.0), (tip.0 - 7.0, hi - 3.0));
        let d = catmull_rom(
            &[
                (x + 5.0, bottom - 1.4),
                (x + 11.5, bottom - 9.0),
                k1,
                (lerp(k1.0, k2.0, 0.5), lerp(k1.1, k2.1, 0.5)),
                (tip.0 - 6.0, hi - 2.9),
                (tip.0 - 1.5, hi - 1.9),
                tip,
            ],
            6,
        );
        let line: Vec<_> = along(&d, plen(&d) * ext, 40).into_iter().map(|p| keep_in(style, s, p, TAIL_PAD)).collect();
        // It also fades in and out over 0.15 s as it leaves and rejoins
        // the cat, so it never pops in a frame.
        let shown = kf(t, &[(1.38, 0.0, Lin), (1.53, 1.0, Lin), (7.27, 1.0, Lin), (7.42, 0.0, Lin)]);
        out.parts.push(figure(Layer::Front, tail(&line, &slim_tail(0.42 * (ext / 0.2).min(1.0)), look), shown * play));
    }

    // The lit lip under each key, the notes floating off, and the sweep's
    // lit edge: along the lip and round the right end's arc, one formula for
    // both styles (the island's own corner: 16 on the Notch, h/2 on the Pill).
    for (i, g) in glow.iter().enumerate() {
        if *g > 0.02 {
            out.parts.push(Part {
                layer: Layer::Front,
                draw: Draw::Ellipse { c: (dig[i], bottom - 2.6), r: (4.4, 1.3), rgb: LIGHT },
                alpha: 0.55 * g * play,
            });
        }
    }
    for (tn, j) in NOTES.iter().copied().chain([(LAST, 4)]) {
        let k = (t - tn) / 0.7;
        if k > 0.0 && k < 1.0 {
            // Fading in over 0.12 s (the mockup shows it at once), a
            // frame's fade at most.
            let alpha = (1.0 - k).min(k / 0.17) * play;
            out.parts.push(Part { layer: Layer::Front, draw: note((dig[j] + 3.0, top + 7.0), k), alpha });
        }
    }
    let lit = bump(t, SWEEP.1 - 0.1, 0.6);
    if lit > 0.02 {
        let r = corner_radius(style, s);
        let (cx, cy, big) = (right - r, bottom - r, r + 0.4);
        // The quarter's cubic control arm.
        let k = 0.552_284_8 * big;
        let path = vec![
            Seg::Move(dig[0] - 10.0, bottom + 0.4),
            Seg::Line(cx, bottom + 0.4),
            Seg::Cubic(cx + k, cy + big, cx + big, cy + k, cx + big, cy),
        ];
        out.parts.push(Part { layer: Layer::Front, draw: Draw::Stroke { path, rgb: LIGHT, width: 1.6 }, alpha: 0.5 * lit * play });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::theme::{CatColor, Mascot};
    use crate::ui::cat_style::CatLook;
    use crate::ui::geometry::corner_radius;
    use crate::ui::play::state::{Beat, Play};
    use crate::ui::play::stories::{Draw, FRAME_FADE, Layer, MascotShot, Part, Slot, wind_down_frames};
    use crate::ui::play::testing::shape;
    use crate::ui::play::trigger::Story;
    use crate::ui::robot::{Expr, GRID, Seg};
    use std::time::{Duration, Instant};

    crate::ui::play::stories::story_contract_tests!(Story::PianoEkor, shot);

    const SIZES: [(f32, f32); 3] = crate::ui::play::stories::CONTRACT_SIZES;
    /// The clock's character centres from its box's centre, as the mockup
    /// measures them.
    const CHARS: [f32; 5] = [-17.2, -7.6, 0.0, 7.6, 17.2];
    /// The glow's colour, the mockup's `#F6D04A`.
    const LIGHT: u32 = 0xF6D04A;

    /// The idle island `w` x `h` with the cat and the clock.
    fn stage(style: Style, w: f32, h: f32, beat: Beat) -> Stage {
        let look = CatLook { mascot: Mascot::Cat, ..CatLook::default() };
        Stage::idle(style, shape(style, w, h), beat, look, true)
    }

    /// At `t`, playing (the island spread open).
    fn at(style: Style, w: f32, h: f32, t: f32) -> Shot {
        shot(&stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }))
    }

    /// The clock's character centres (x) where the renderer puts them.
    fn chars(style: Style, w: f32, h: f32) -> [f32; 5] {
        let c = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t: 3.0, cut_at: None }).room().clock.unwrap();
        std::array::from_fn(|i| c.x + c.w / 2.0 + CHARS[i])
    }

    /// The cat: its part, and the head it carries.
    fn cat(shot: &Shot) -> Option<(&Part, MascotShot)> {
        shot.parts.iter().find_map(|p| match &p.draw {
            Draw::Group { items, .. } => items.iter().find_map(|d| match d {
                Draw::Head { shot, .. } => Some((p, *shot)),
                _ => None,
            }),
            _ => None,
        })
    }

    /// The tail: the figure without a head, in front.
    fn tail_part(shot: &Shot) -> Option<&Part> {
        shot.parts.iter().find(|p| {
            p.layer == Layer::Front
                && matches!(&p.draw, Draw::Group { items, .. } if items.len() > 2 && !items.iter().any(|d| matches!(d, Draw::Head { .. })))
        })
    }

    /// The lit lip under the digits: (centre, alpha) of each glow.
    fn glows(shot: &Shot) -> Vec<((f32, f32), f32)> {
        shot.parts
            .iter()
            .filter_map(|p| match p.draw {
                Draw::Ellipse { c, rgb, .. } if rgb == LIGHT => Some((c, p.alpha)),
                _ => None,
            })
            .collect()
    }

    /// The sweep's lit edge: its path and alpha.
    fn sweep(shot: &Shot) -> Option<(Vec<Seg>, f32)> {
        shot.parts.iter().find_map(|p| match &p.draw {
            Draw::Stroke { path, rgb, .. } if *rgb == LIGHT => Some((path.clone(), p.alpha)),
            _ => None,
        })
    }

    #[test]
    fn each_digit_drops_once_in_order() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                // When each character starts to dip (down, or up for the
                // sweep's jump) past 1 dip, sampled every 5 ms.
                let starts = |from: f32, to: f32, up: bool| {
                    let mut out = vec![Vec::new(); 5];
                    let mut was = [false; 5];
                    let mut t = from;
                    while t <= to {
                        let shot = at(style, w, h, t);
                        for (i, fx) in shot.digits.iter().enumerate() {
                            let on = if up { fx.dy < -1.0 } else { fx.dy > 1.0 };
                            if on && !was[i] {
                                out[i].push(t);
                            }
                            was[i] = on;
                        }
                        t += 0.005;
                    }
                    out
                };
                // Up the keys: the hours' two digits, then the minutes', each
                // pressed once, in order; the colon is no key.
                let keys = starts(1.9, 3.2, false);
                assert!(keys[2].is_empty(), "{case}: the colon dipped at {:?}", keys[2]);
                let order: Vec<f32> = [0, 1, 3, 4].iter().map(|&i| {
                    assert_eq!(keys[i].len(), 1, "{case}: digit {i} dipped at {:?}", keys[i]);
                    keys[i][0]
                }).collect();
                assert!(order.windows(2).all(|p| p[0] < p[1]), "{case}: {order:?}");
                // Each lights the lip under itself as it dips.
                let x = chars(style, w, h);
                let bottom = shape(style, w, h).y + h;
                for (tn, i) in [(2.05, 0), (2.35, 1), (2.65, 3), (2.95, 4)] {
                    let lit = glows(&at(style, w, h, tn + 0.07));
                    assert!(lit.iter().any(|(c, a)| (c.0 - x[i]).abs() < 0.01 && (c.1 - (bottom - 2.6)).abs() < 0.01 && *a > 0.3),
                        "{case}: digit {i} at {tn}: {lit:?}");
                }
                assert!(glows(&at(style, w, h, 1.9)).is_empty(), "{case}");
                // The sweep: every character jumps once, left to right.
                let wave = starts(4.55, 5.3, true);
                let order: Vec<f32> = (0..5).map(|i| {
                    assert_eq!(wave[i].len(), 1, "{case}: character {i} jumped at {:?}", wave[i]);
                    wave[i][0]
                }).collect();
                assert!(order.windows(2).all(|p| p[0] < p[1]), "{case}: {order:?}");
            }
        }
    }

    #[test]
    fn no_digit_moves_with_a_jump() {
        // The mockup switches the sweep's press on at once; here every
        // character eases in and out of it.
        for style in [Style::Notch, Style::Pill] {
            let mut last = at(style, 190.0, 34.0, 0.0).digits;
            let mut t = 0.0;
            while t <= Story::PianoEkor.secs() {
                let now = at(style, 190.0, 34.0, t).digits;
                for i in 0..5 {
                    assert!((now[i].dy - last[i].dy).abs() < 0.6 && (now[i].rot_deg - last[i].rot_deg).abs() < 12.0,
                        "{style:?} t={t} character {i}: {:?} to {:?}", last[i], now[i]);
                }
                last = now;
                t += 0.005;
            }
        }
    }

    #[test]
    fn the_sweep_follows_the_right_ends_radius() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let s = shape(style, w, h);
                // One formula: the island's own corner, 16 on the Notch, a
                // half height on the Pill.
                let r = corner_radius(style, s);
                assert_eq!(r, if style == Style::Notch { 16.0 } else { h / 2.0 }, "{case}");
                let (right, bottom) = (s.x + s.w, s.y + s.h);
                let (path, alpha) = sweep(&at(style, w, h, 5.25)).unwrap_or_else(|| panic!("{case}: no sweep"));
                assert!(alpha > 0.4 && alpha <= 0.5, "{case}: {alpha}");
                let x = chars(style, w, h);
                // Along the lip from left of the first digit...
                let [Seg::Move(x0, y0), Seg::Line(x1, y1), Seg::Cubic(ax, ay, bx, by, ex, ey)] = path[..] else {
                    panic!("{case}: {path:?}");
                };
                assert!((x0 - (x[0] - 10.0)).abs() < 1e-3 && (y0 - (bottom + 0.4)).abs() < 1e-3, "{case}: {path:?}");
                assert!((x1 - (right - r)).abs() < 1e-3 && (y1 - y0).abs() < 1e-3, "{case}: {path:?}");
                // ...then round the right end's arc, 0.4 outside it, up to its side.
                let c = (right - r, bottom - r);
                for i in 0..=20 {
                    let t = i as f32 / 20.0;
                    let u = 1.0 - t;
                    let p = (
                        u * u * u * x1 + 3.0 * u * u * t * ax + 3.0 * u * t * t * bx + t * t * t * ex,
                        u * u * u * y1 + 3.0 * u * u * t * ay + 3.0 * u * t * t * by + t * t * t * ey,
                    );
                    let d = (p.0 - c.0).hypot(p.1 - c.1);
                    assert!((d - (r + 0.4)).abs() < 0.01 * r, "{case}: {p:?} is {d} from the corner's centre");
                }
                assert!((ex - (right + 0.4)).abs() < 1e-3 && (ey - (bottom - r)).abs() < 1e-3, "{case}: {path:?}");
                // Only at the end of the sweep.
                assert!(sweep(&at(style, w, h, 4.9)).is_none() && sweep(&at(style, w, h, 5.6)).is_none(), "{case}");
            }
        }
    }

    /// The points `path` passes through: its ends and its curves sampled
    /// (not their control points, which `bounds` counts).
    fn samples(path: &[Seg]) -> Vec<(f32, f32)> {
        let (mut out, mut at) = (Vec::new(), (0.0, 0.0));
        for seg in path {
            let n = 16;
            let pts: Vec<(f32, f32)> = match *seg {
                Seg::Move(x, y) | Seg::Line(x, y) => vec![(x, y)],
                Seg::Quad(ax, ay, x, y) => (1..=n).map(|i| {
                    let (t, u) = (i as f32 / n as f32, 1.0 - i as f32 / n as f32);
                    (u * u * at.0 + 2.0 * u * t * ax + t * t * x, u * u * at.1 + 2.0 * u * t * ay + t * t * y)
                }).collect(),
                Seg::Cubic(ax, ay, bx, by, x, y) => (1..=n).map(|i| {
                    let (t, u) = (i as f32 / n as f32, 1.0 - i as f32 / n as f32);
                    (
                        u * u * u * at.0 + 3.0 * u * u * t * ax + 3.0 * u * t * t * bx + t * t * t * x,
                        u * u * u * at.1 + 3.0 * u * u * t * ay + 3.0 * u * t * t * by + t * t * t * y,
                    )
                }).collect(),
                Seg::Close => vec![],
            };
            for q in pts {
                out.push(q);
                at = q;
            }
        }
        out
    }

    /// The tail's ink as (point, how far its ink reaches round it): its
    /// fills' outlines, and its strokes (the Black cat's rim) with half their
    /// width. The tail's group leaves the window's dip as they are.
    fn ink(d: &Draw, out: &mut Vec<((f32, f32), f32)>) {
        match d {
            Draw::Fill { path, .. } | Draw::Tint { path, .. } => out.extend(samples(path).into_iter().map(|q| (q, 0.0))),
            Draw::Stroke { path, width, .. } => out.extend(samples(path).into_iter().map(|q| (q, width / 2.0))),
            Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items } => items.iter().for_each(|i| ink(i, out)),
            other => panic!("unexpected in the tail: {other:?}"),
        }
    }

    /// How far inside the upper half of the resting island (x, y) is, dip
    /// (negative outside): under the top edge, inside the Pill's half
    /// circles, inside the Notch's straight sides.
    fn depth_inside(style: Style, s: crate::ui::geometry::ShapeRect, (x, y): (f32, f32)) -> f32 {
        let (r, right, mid) = (corner_radius(style, s), s.x + s.w, s.y + s.h / 2.0);
        if style == Style::Pill && x < s.x + r {
            r - (x - (s.x + r)).hypot(y - mid)
        } else if style == Style::Pill && x > right - r {
            r - (x - (right - r)).hypot(y - mid)
        } else {
            (y - s.y).min(x - s.x).min(right - x)
        }
    }

    #[test]
    fn the_tail_stays_inside_the_island_round_its_ends() {
        // Its arc is tied to the island's edge, the Pill's rounded ends
        // included (the sweep takes the tip into the right one), so on the
        // Pill it never shows in the gap above it: every sampled point of its
        // ink in the island's upper half (below it the tail leaves the cat on
        // the lip), the Black cat's rim counted, in every coat.
        for color in [CatColor::Terracotta, CatColor::Calico, CatColor::White, CatColor::Black] {
            let look = CatLook { mascot: Mascot::Cat, color, ..CatLook::default() };
            for style in [Style::Notch, Style::Pill] {
                for (w, h) in SIZES {
                    let s = shape(style, w, h);
                    let mut seen = false;
                    let mut worst = (f32::MAX, 0.0, (0.0, 0.0));
                    let mut t = 1.3;
                    while t < 7.5 {
                        let st = Stage { look, ..stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }) };
                        if let Some(p) = tail_part(&shot(&st)) {
                            let mut pts = Vec::new();
                            ink(&p.draw, &mut pts);
                            for (q, reach) in pts.into_iter().filter(|(q, _)| q.1 < s.y + s.h / 2.0) {
                                let d = depth_inside(style, s, q) - reach;
                                if d < worst.0 {
                                    worst = (d, t, q);
                                }
                            }
                            seen = true;
                        }
                        t += 0.02;
                    }
                    assert!(seen, "{color:?} {style:?} {w}x{h}: no tail");
                    assert!(worst.0 >= -0.05, "{color:?} {style:?} {w}x{h}: ink {} dip outside at t={} {:?}", -worst.0, worst.1, worst.2);
                }
            }
        }
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                // It reaches out over the clock while it plays, and is in
                // before and after.
                let b = tail_part(&at(style, w, h, 3.0)).unwrap().draw.bounds();
                assert!(b.x + b.w > chars(style, w, h)[3], "{style:?} {w}x{h}: {b:?}");
                assert!(tail_part(&at(style, w, h, 1.3)).is_none() && tail_part(&at(style, w, h, 7.5)).is_none());
            }
        }
    }

    /// Which part `p` is, the same from frame to frame: the cat, the tail,
    /// the glow under a character, the note over one, the sweep's edge.
    fn which(p: &Part) -> String {
        match &p.draw {
            Draw::Group { items, .. } if items.iter().any(|d| matches!(d, Draw::Head { .. })) => "cat".into(),
            Draw::Group { at, items, .. } if items.len() == 2 => format!("note {:.2}", at.0),
            Draw::Group { .. } => "tail".into(),
            Draw::Ellipse { c, .. } => format!("glow {:.2}", c.0),
            Draw::Stroke { .. } => "sweep".into(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn no_part_pops_in_or_out_between_frames() {
        // Played through at 60 Hz: every part (one gone counts as 0) and
        // the halo change by a frame's fade at most.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let mut last: Option<Shot> = None;
                let mut frame = 0;
                loop {
                    let t = frame as f32 / 60.0;
                    if t > Story::PianoEkor.secs() {
                        break;
                    }
                    let now = at(style, w, h, t);
                    if let Some(was) = &last {
                        let alpha = |shot: &Shot, k: &str| shot.parts.iter().filter(|p| which(p) == k).map(|p| p.alpha).sum::<f32>();
                        for p in was.parts.iter().chain(&now.parts) {
                            let k = which(p);
                            let (a, b) = (alpha(was, &k), alpha(&now, &k));
                            assert!((a - b).abs() <= FRAME_FADE + 1e-4, "{style:?} {w}x{h} t={t}: {k} from {a} to {b}");
                        }
                        assert_eq!(now.halo, 1.0, "{style:?} {w}x{h} t={t}");
                    }
                    last = Some(now);
                    frame += 1;
                }
            }
        }
    }

    /// The notes floating off: where each is (window dip) and its alpha.
    fn notes(shot: &Shot) -> Vec<((f32, f32), f32)> {
        shot.parts
            .iter()
            .filter_map(|p| match &p.draw {
                Draw::Group { at, items, .. } if items.len() == 2 => Some((*at, p.alpha)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_notes_float_off_the_keys_they_play() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let (x, top) = (chars(style, w, h), shape(style, w, h).y);
                // None before the first key.
                assert!(notes(&at(style, w, h, 2.0)).is_empty(), "{case}");
                // Halfway up its 7 dip rise, the first note over digit 0; the
                // second just leaving digit 1, fading in.
                let n = notes(&at(style, w, h, 2.4));
                assert_eq!(n.len(), 2, "{case}: {n:?}");
                assert!((n[0].0.0 - (x[0] + 3.0)).abs() < 1e-3 && (n[0].0.1 - (top + 7.0 - 3.5)).abs() < 1e-3 && (n[0].1 - 0.5).abs() < 1e-3, "{case}: {n:?}");
                assert!((n[1].0.0 - (x[1] + 3.0)).abs() < 1e-3 && n[1].1 > 0.0 && n[1].1 < 0.5, "{case}: {n:?}");
                // Three at most at once on the climb; the hard last note alone
                // over the minute's last digit; gone 0.7 s after it.
                assert_eq!(notes(&at(style, w, h, 3.0)).len(), 3, "{case}");
                let last = notes(&at(style, w, h, 5.95));
                assert_eq!(last.len(), 1, "{case}: {last:?}");
                assert!((last[0].0.0 - (x[4] + 3.0)).abs() < 1e-3, "{case}: {last:?}");
                assert!(notes(&at(style, w, h, 6.31)).is_empty(), "{case}");
            }
        }
    }

    #[test]
    fn the_later_keys_are_played_in_order() {
        // After the climb, back down: 3.4, 3.7, 4.0 and 4.3 s on the
        // characters 3, 1, 0 and 1, each a single dip.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let mut dips = Vec::new();
                let mut was = [false; 5];
                let mut t = 3.25;
                while t <= 4.6 {
                    let shot = at(style, w, h, t);
                    for (i, fx) in shot.digits.iter().enumerate() {
                        let on = fx.dy > 1.0;
                        if on && !was[i] {
                            dips.push((t, i));
                        }
                        was[i] = on;
                    }
                    t += 0.005;
                }
                let order: Vec<usize> = dips.iter().map(|d| d.1).collect();
                assert_eq!(order, vec![3, 1, 0, 1], "{style:?} {w}x{h}: {dips:?}");
                for ((t, _), tn) in dips.iter().zip([3.4, 3.7, 4.0, 4.3]) {
                    assert!((t - tn).abs() < 0.06, "{style:?} {w}x{h}: {dips:?}");
                }
            }
        }
    }

    #[test]
    fn the_slot_head_and_the_cat_never_show_more_than_one_mascot() {
        // Their alphas sum to 1 at most: while it plays, and through real
        // wind-downs cut during the hop out of the dark, the play and the hop
        // back.
        let both = |shot: &Shot| {
            let head = match shot.slot {
                Slot::Story(m) => m.alpha,
                Slot::App => 1.0,
                Slot::Empty => 0.0,
            };
            head + cat(shot).map_or(0.0, |(p, _)| p.alpha)
        };
        for style in [Style::Notch, Style::Pill] {
            let mut t = 0.0;
            while t <= Story::PianoEkor.secs() {
                let sum = both(&at(style, 190.0, 34.0, t));
                assert!(sum <= 1.0 + 1e-4, "{style:?} t={t}: {sum}");
                t += 0.005;
            }
            for cut in [1.2, 3.0, 7.7] {
                for phase in [0.0, 8.0] {
                    for (ms, beat, shot) in wind_down_frames(cut, phase, |beat| shot(&stage(style, 190.0, 34.0, beat))) {
                        let sum = both(&shot);
                        assert!(sum <= 1.0 + 1e-4, "{style:?} cut {cut} +{ms:.1} ms (alpha {}): {sum}", beat.alpha);
                    }
                }
            }
        }
    }

    #[test]
    fn the_cat_comes_out_of_the_dark_beside_the_clock() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let shot = at(style, w, h, 3.0);
                let (part, _) = cat(&shot).unwrap_or_else(|| panic!("{case}: no cat"));
                assert_eq!((part.layer, part.alpha), (Layer::Inside, 1.0), "{case}");
                // Seated on the lip 16 dip left of the first digit.
                let Draw::Group { at: seat, .. } = part.draw else { unreachable!() };
                assert!((seat.0 - (chars(style, w, h)[0] - 16.0)).abs() < 1e-3, "{case}: {seat:?}");
                assert!(seat.1 > shape(style, w, h).y + h, "{case}: {seat:?}");
                // Out of the dark and back into it: none before and after.
                for t in [0.9, 1.05, 7.95, 8.3] {
                    assert!(cat(&at(style, w, h, t)).is_none(), "{case} t={t}");
                }
                let half = cat(&at(style, w, h, 1.2)).map_or(0.0, |(p, _)| p.alpha);
                assert!(half > 0.0 && half < 1.0, "{case}: {half}");
            }
        }
    }

    /// The slot head as the frame shows it: its pivot, its dip per grid unit
    /// and its opacity. `None` while it is all in the dark.
    fn slot_head(st: &Stage, shot: &Shot) -> Option<((f32, f32), f32, f32)> {
        let r = st.room().robot.unwrap();
        let k = r.w / GRID;
        match shot.slot {
            Slot::Story(m) if m.alpha > 0.0 => Some(((r.x + 32.0 * k + m.dx, r.y + 40.0 * k + m.dy), k * m.sx, m.alpha)),
            Slot::App => Some(((r.x + 32.0 * k, r.y + 40.0 * k), k, 1.0)),
            _ => None,
        }
    }

    #[test]
    fn the_slot_head_ducks_into_the_dark_and_comes_back_without_a_jump() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let mut last: Option<((f32, f32), f32, f32)> = None;
                let (mut dark, mut t) = (false, 0.0);
                while t <= Story::PianoEkor.secs() {
                    let st = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None });
                    let shot = shot(&st);
                    let now = slot_head(&st, &shot);
                    if let (Some((p, k, a)), Some((q, j, b))) = (now, last) {
                        assert!((p.0 - q.0).abs() < 0.5 && (p.1 - q.1).abs() < 0.5 && (k - j).abs() < 0.02 && (a - b).abs() < 0.12,
                            "{case} t={t}: {q:?} {j} {b} to {p:?} {k} {a}");
                    }
                    if now.is_none() {
                        assert!(last.is_none_or(|l| l.2 < 0.12), "{case} t={t}: {last:?}");
                        dark = true;
                    } else if last.is_none() && t > 0.0 {
                        assert!(now.unwrap().2 < 0.12, "{case} t={t}: {now:?}");
                    }
                    last = now;
                    t += 0.01;
                }
                assert!(dark, "{case}: never in the dark");
                // In the dark while the cat plays, home by the end.
                assert!(slot_head(&stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t: 3.0, cut_at: None }), &at(style, w, h, 3.0)).is_none(), "{case}");
                assert!(matches!(at(style, w, h, 8.3).slot, Slot::Story(m) if m.alpha == 1.0), "{case}");
                // The see-through halo goes with the head, never apart.
                assert_eq!(at(style, w, h, 3.0).halo, 1.0, "{case}");
            }
        }
    }

    #[test]
    fn the_last_note_knocks_the_one_over_and_the_tail_stands_it_up() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let d = at(style, w, h, 6.1).digits;
                assert!(d[4].rot_deg > 85.0 && d[4].dy > 3.0, "{case}: {:?}", d[4]);
                assert!(d[..4].iter().all(|f| f.rot_deg == 0.0), "{case}: {d:?}");
                let up = at(style, w, h, 6.9).digits[4];
                assert!(up.rot_deg.abs() < 1.0 && up.dy.abs() < 0.2, "{case}: {up:?}");
                // A note for the hard key too, and the lip lit under it.
                let x = chars(style, w, h)[4];
                assert!(glows(&at(style, w, h, 5.7)).iter().any(|(c, a)| (c.0 - x).abs() < 0.01 && *a > 0.3), "{case}");
            }
        }
    }

    #[test]
    fn the_faces_are_the_mockups_in_the_apps_expressions() {
        // Happy while it plays (eyes shut in delight), the wince a squint
        // (Focus), the bow with the eyes closed (a blink), looking at the
        // clock meanwhile.
        let face = |t: f32| cat(&at(Style::Notch, 190.0, 34.0, t)).unwrap().1;
        assert_eq!((face(1.5).expr, face(1.5).glance.0), (Expr::Idle, 0.0));
        assert!(face(1.6).blink && !face(1.7).blink);
        assert_eq!(face(3.0).expr, Expr::Happy);
        assert!(face(3.0).glance.0 > 2.0);
        assert_eq!(face(5.9).expr, Expr::Focus);
        assert!(face(7.0).blink && face(7.0).expr == Expr::Idle);
        assert_eq!((face(7.5).expr, face(7.5).glance.0), (Expr::Idle, 0.0));
    }

    #[test]
    fn a_cut_before_the_cat_is_out_never_brings_it_out() {
        for cut in [0.5, 0.9, 1.07] {
            let t0 = Instant::now();
            let mut play = Play::start(Story::PianoEkor, t0);
            let at_cut = t0 + Duration::from_secs_f32(cut);
            play.end(at_cut);
            for ms in 0..=220 {
                let beat = play.beat(at_cut + Duration::from_millis(ms));
                let shot = shot(&stage(Style::Notch, 190.0, 34.0, beat));
                assert!(shot.parts.is_empty(), "cut {cut} +{ms} ms (t={}): {:?}", beat.t, shot.parts);
            }
        }
    }

    #[test]
    fn a_wind_down_brings_the_digit_and_the_cat_home() {
        // Cut short with the "1" lying down, a real wind-down in 60 Hz
        // frames: the digit stands up steadily, the slot head comes back out
        // of the dark a frame's fade at a time, and it ends exactly at rest.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let frames = wind_down_frames(6.0, 0.0, |beat| shot(&stage(style, w, h, beat)));
                let mut rot = f32::MAX;
                let mut seen = 0.0_f32;
                for (ms, beat, shot) in &frames {
                    let now = shot.digits[4].rot_deg;
                    assert!(now <= rot + 1e-4, "{case} +{ms:.1} ms (alpha {}): {rot} to {now}", beat.alpha);
                    rot = now;
                    let head = match shot.slot {
                        Slot::Story(m) => m.alpha,
                        _ => 1.0,
                    };
                    assert!(head - seen <= FRAME_FADE + 1e-4, "{case} +{ms:.1} ms: the slot head from {seen} to {head}");
                    seen = head;
                }
                let (_, beat, end) = frames.last().unwrap();
                assert_eq!(beat.alpha, 0.0, "{case}");
                assert_eq!(*end, Shot::rest(style, shape(style, w, h)), "{case}");
            }
        }
    }

    #[test]
    fn the_cat_and_its_tail_wear_the_coat() {
        for color in [CatColor::Terracotta, CatColor::Calico, CatColor::White, CatColor::Black] {
            let look = CatLook { mascot: Mascot::Cat, color, ..CatLook::default() };
            let st = Stage { look, ..stage(Style::Pill, 190.0, 34.0, Beat { spread: 1.0, alpha: 1.0, t: 3.0, cut_at: None }) };
            let shot = shot(&st);
            let Draw::Group { items, .. } = &tail_part(&shot).expect("the tail").draw else { unreachable!() };
            assert!(items.iter().any(|d| matches!(d, Draw::Fill { rgb, .. } if *rgb == look.head_rgb())), "{color:?}");
            assert!(cat(&shot).is_some(), "{color:?}");
        }
    }
}
