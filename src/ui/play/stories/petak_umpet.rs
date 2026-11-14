//! The cat's "Petak umpet" (hide and seek): the slot head dives into the
//! island's dark; a bump runs along the lip each time the cat moves behind
//! it; it peeks round the left end, then the right, a paw gripping the edge;
//! peeking from the left again, its own tail tip pokes out at the right end;
//! taking it for another cat it wiggles (the island shakes) and pounces (the
//! bump shoots across, the island rocks); it turns up at the right end
//! biting its own tail, lets go, and the slot head comes back up. Timeline,
//! easing and offsets from `docs/design/maskot-notch-v3.html`, story `c1`.

use std::f32::consts::{PI, TAU};

use super::{Draw, Ease, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, island_box, kept, kf, lerp, rim_pt, ring};
use crate::config::Style;
use crate::ui::geometry::{ROBOT_COMPACT, corner_radius};
use crate::ui::play::art::cat::{CAT_SC, slim_tail, small_paw, tail};
use crate::ui::play::art::tube::{along, catmull_rom, curl, plen};
use crate::ui::play::shape::{Deform, lip_dent, rigid};
use crate::ui::play::trigger::Story;
use crate::ui::robot::{Expr, GRID};

use Ease::{I, Lin, O, Ob, Qi, Sb, Sio};

/// The four peeks: start, end, and the end it peeks round (-1 left, 1 right).
const PEEKS: [(f32, f32, f32); 4] = [(1.6, 2.45, -1.0), (3.0, 3.75, 1.0), (4.12, 5.02, -1.0), (5.32, 6.6, 1.0)];

/// How far a peek from `t0` to `t1` has come round its end, 0 to a little
/// over 1.
fn peek(t: f32, t0: f32, t1: f32) -> f32 {
    kf(t, &[(t0, 0.0, Lin), (t0 + 0.32, 1.0, Ob), (t1 - 0.25, 1.0, Lin), (t1, 0.0, I)])
}

/// The face of peek `n` at `t` in the app's own expressions: the mockup's
/// round eyes are Alert, its squint Focus. Its ear turns and its 'w' mouth
/// have no art here and are left out.
fn peek_face(n: usize, side: f32, t: f32) -> MascotShot {
    let (mut expr, mut look) = (Expr::Idle, -side * 0.6);
    match n {
        1 => expr = Expr::Alert,
        2 if t > 4.45 => (expr, look) = (Expr::Alert, 1.0),
        2 => look = 0.0,
        3 if t < 5.95 => expr = Expr::Focus,
        3 if t < 6.2 => expr = Expr::Alert,
        3 => expr = Expr::Happy,
        _ => {}
    }
    MascotShot { expr, glance: (look * 2.4, 0.0), blink: blinks(t, &[2.1, 5.9]) > 0.25, ..MascotShot::REST }
}

/// `items` (window dip) as one figure in `layer`.
fn figure(layer: Layer, items: Vec<Draw>, alpha: f32) -> Part {
    Part { layer, draw: Draw::Group { at: (0.0, 0.0), rot_deg: 0.0, scale: (1.0, 1.0), pivot: (0.0, 0.0), items }, alpha }
}

/// One moment of the story on `stage`, the cat in the coat of its look.
/// `beat.alpha` eases it home, paced by the wind-down's time (`kept`): over
/// its first 0.58 the peeks fade and the island settles; from 0.3 to 0.88
/// the slot head comes back out of the dark where it dived, and from 0.6 it
/// rises home. Cut short, the story holds
/// where the cut found it, so nothing it had not brought out shows. Exactly
/// at rest at 0 s, at the end and with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, .. } = *stage;
    let a = beat.alpha.clamp(0.0, 1.0);
    let t = beat.cut_at.map_or(beat.t, |c| beat.t.min(c));
    let mut out = Shot::rest(style, s);
    if t <= 0.0 || t >= Story::PetakUmpet.secs() || a == 0.0 {
        return out;
    }
    // The slot where the renderer puts it (the clock on or off moves it).
    let Some(slot) = stage.room().robot else {
        return out;
    };
    let play = kept(a, 0.0, 0.58);
    let slot_x = slot.x + slot.w / 2.0;

    // The slot head ducks into the dark and later comes back up, fading with
    // its halo as one. Wound down, it comes back out of the dark where it
    // dived, then rises home.
    let away = kf(t, &[(0.5, 0.0, Lin), (0.85, 1.0, I), (6.95, 1.0, Lin), (7.35, 0.0, Sb)]);
    let duck = away * kept(a, 0.6, 1.0);
    let dark = away.max(0.0) * kept(a, 0.3, 0.88);
    out.slot = Slot::Story(MascotShot {
        dy: 4.0 * duck,
        sx: 1.0 - 0.15 * duck,
        sy: 1.0 - 0.15 * duck,
        expr: if (7.0..7.7).contains(&t) { Expr::Happy } else { Expr::Idle },
        blink: blinks(t, &[7.7]) > 0.25,
        alpha: 1.0 - dark,
        ..MascotShot::REST
    });

    // The island: a bump runs along the lip while the cat moves behind it;
    // the wiggle shakes it, the pounce rocks it (a floating pill is knocked
    // off its height too; a notch hangs from the screen edge).
    let r = corner_radius(style, s);
    let (lx, rx) = (s.x + r + 2.0, s.x + s.w - r - 2.0);
    let runs = [(0.85, 1.55, slot_x, lx), (2.4, 3.0, lx, rx), (3.7, 4.1, rx, lx), (5.0, 5.28, lx, rx + 2.0)];
    let mut dents = Vec::new();
    for (t0, t1, x0, x1) in runs {
        if (t0..t1 + 0.08).contains(&t) {
            let u = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let ease = if t0 > 4.9 { Qi } else { Sio };
            let depth = s.h * 0.07 * (PI * (u * 1.1).min(1.0)).sin().max(0.0);
            dents.push(lip_dent(style, s, lerp(x0, x1, ease.at(u)), 9.0, depth * play));
        }
    }
    // The wiggle grows over its first 40 ms, so it starts from still.
    let wiggle = if (4.62..5.0).contains(&t) { 0.8 * ((t - 4.62) / 0.04).min(1.0) * (TAU * 14.0 * t).sin() } else { 0.0 };
    let rock = if t < 5.05 {
        0.0
    } else if t < 5.28 {
        kf(t, &[(5.05, 0.0, Lin), (5.28, -2.4, O)])
    } else {
        ring(t, 5.28, 3.4, 1.4, 2.0) - 2.4 * (-6.0 * (t - 5.28)).exp()
    };
    let lift = if style == Style::Pill && t >= 5.28 { ring(t, 5.28, 1.6, 1.4, 2.4) } else { 0.0 };
    out.deform = Deform { dents, swing_deg: rock * play, shift: (0.0, lift * play), jolt: wiggle * play, ..Deform::default() };
    out.island = island_box(style, s, &out.deform);
    if play <= 0.0 {
        return out;
    }

    // The peeks round the ends: the head is cut to outside the island (what
    // is behind it is clipped away, not covered); its paw grips the rim just
    // below the chin, in front, moving and turning with the island.
    let unit = CAT_SC * ROBOT_COMPACT / GRID;
    let m = rigid(style, s, &out.deform);
    let on_island = |(x, y): (f32, f32)| (x * m.M11 + y * m.M21 + m.M31 + out.deform.jolt, x * m.M12 + y * m.M22 + m.M32);
    let turn = m.M12.atan2(m.M11).to_degrees();
    let mid = s.y + s.h / 2.0;
    let end = |d: f32| s.x + s.w + d;
    let peek_x = |side: f32, k: f32| s.x + s.w / 2.0 + side * lerp(s.w / 2.0 - 14.0, s.w / 2.0 + 4.0, k);
    let mut paws = Vec::new();
    for (n, &(t0, t1, side)) in PEEKS.iter().enumerate() {
        let k = peek(t, t0, t1);
        if k <= 0.01 {
            continue;
        }
        let at = (peek_x(side, k), mid + if n == 3 { 2.0 } else { 0.0 });
        let shake = if n == 2 && t > 4.6 && t < 5.0 { 3.0 * (TAU * 5.0 * t).sin() } else { 0.0 };
        let shot = MascotShot { rot_deg: side * 24.0 * k + shake, sx: CAT_SC, sy: CAT_SC, ..peek_face(n, side, t) };
        out.parts.push(Part { layer: Layer::Back, draw: Draw::Head { at, shot }, alpha: play });
        let grip = ((k - 0.4) / 0.6).clamp(0.0, 1.0);
        if grip > 0.02 {
            let draw = Draw::Group {
                at: on_island(rim_pt(style, s, side, 0.73, 0.4)),
                rot_deg: side * 70.0 + turn,
                scale: (grip * unit, grip * unit),
                pivot: (0.0, 0.0),
                items: vec![small_paw(look)],
            };
            paws.push(Part { layer: Layer::Front, draw, alpha: play });
        }
    }

    // The tail tip pokes out at the right end ("another cat!"), gets yanked
    // in, then turns up in its mouth: over the head, cut to outside the island.
    let tip = kf(t, &[(4.3, 0.0, Lin), (4.7, 1.0, Ob), (5.15, 1.0, Lin), (5.27, 0.0, I)]);
    if tip > 0.01 {
        let d = catmull_rom(&[(end(-10.0), mid + 5.0), (end(0.0), mid + 5.0), (end(6.0), mid + 2.0), (end(8.5), mid - 4.0)], 8);
        let line = curl(&along(&d, plen(&d) * tip, 20), -2.6 + 0.6 * (TAU * 2.4 * t).sin(), 8.0 * tip, 10, 0.0);
        out.parts.push(figure(Layer::Back, tail(&line, &slim_tail(0.4), look), play));
    }
    let bite = kf(t, &[(5.4, 0.0, Lin), (5.7, 1.0, O), (6.12, 1.0, Lin), (6.25, 0.0, I)]);
    if bite > 0.01 {
        let k = peek(t, 5.32, 6.6);
        let turn = (24.0 * k).to_radians();
        let mouth = (peek_x(1.0, k) - 3.4 * turn.sin(), mid + 2.0 + 3.4 * turn.cos());
        let (bottom, b2) = (s.y + s.h, (end(4.0), s.y + s.h - 4.0));
        let toward = (lerp(b2.0, mouth.0, 0.7), lerp(b2.1, mouth.1, 0.7));
        let d = catmull_rom(&[(end(-8.0), bottom - 3.0), (end(0.0), bottom - 2.5), b2, toward, mouth], 8);
        let line = curl(&along(&d, plen(&d) * bite, 20), 1.5, 4.0, 8, 0.0);
        out.parts.push(figure(Layer::Back, tail(&line, &slim_tail(0.42), look), play));
    }
    out.parts.extend(paws);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::theme::{CatColor, Mascot};
    use crate::ui::cat_style::CatLook;
    use crate::ui::play::rim::rim;
    use crate::ui::play::shape::{is_rest, rigid};
    use crate::ui::play::state::{Beat, Play};
    use crate::ui::play::stories::{Draw, Layer, MascotShot, Slot};
    use crate::ui::play::testing::shape;
    use crate::ui::play::trigger::Story;
    use crate::ui::robot::{Expr, GRID};
    use std::time::{Duration, Instant};

    crate::ui::play::stories::story_contract_tests!(Story::PetakUmpet, shot);

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

    /// The heads the shot draws as parts: layer, pivot, shot, alpha.
    fn heads(shot: &Shot) -> Vec<(Layer, (f32, f32), MascotShot, f32)> {
        shot.parts
            .iter()
            .filter_map(|p| match p.draw {
                Draw::Head { at, shot } => Some((p.layer, at, shot, p.alpha)),
                _ => None,
            })
            .collect()
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

    /// The deepest dent's place on the lip (x) and its depth (dip).
    fn bump(shot: &Shot, style: Style, s: crate::ui::geometry::ShapeRect) -> Option<(f32, f32)> {
        shot.deform
            .dents
            .iter()
            .max_by(|a, b| a.depth.total_cmp(&b.depth))
            .map(|d| (rim(style, s, d.u).x, d.depth * s.h))
    }

    #[test]
    fn the_bump_runs_left_to_right_along_the_lip() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let case = format!("{style:?} {w}x{h}");
                // From the left end of the lip to its right end, the lip
                // pushed down, as it hides behind the island; and again on
                // the pounce.
                for (from, to) in [(2.42, 2.98), (5.02, 5.26)] {
                    let mut t = from;
                    let mut last = f32::MIN;
                    let mut xs = Vec::new();
                    while t <= to {
                        let (x, depth) = bump(&at(style, w, h, t, 1.0), style, s).unwrap_or_else(|| panic!("{case} t={t}: no bump"));
                        assert!(x > last, "{case} t={t}: the bump went back from {last} to {x}");
                        assert!(depth >= 0.0, "{case} t={t}: depth {depth}");
                        last = x;
                        xs.push(x);
                        t += 0.02;
                    }
                    assert!(xs[0] < s.x + 0.3 * s.w && last > s.x + 0.7 * s.w, "{case}: from {} to {last}", xs[0]);
                }
                // At its height the lip is pushed down by the mockup's 7 % of the height.
                let (_, depth) = bump(&at(style, w, h, 2.7, 1.0), style, s).unwrap();
                assert!(depth > 0.05 * h, "{case}: {depth}");
                // It runs right to left between those, and not at all before
                // the cat has dived or after the pounce.
                let (a, b) = (bump(&at(style, w, h, 3.75, 1.0), style, s).unwrap(), bump(&at(style, w, h, 3.95, 1.0), style, s).unwrap());
                assert!(b.0 < a.0, "{case}: {a:?} then {b:?}");
                for t in [0.5, 0.8, 1.7, 5.4, 7.0] {
                    assert!(at(style, w, h, t, 1.0).deform.dents.is_empty(), "{case} t={t}");
                }
            }
        }
    }

    #[test]
    fn the_notch_rocks_about_the_screen_edge_and_the_pill_about_its_centre() {
        for (w, h) in SIZES {
            let n = shape(Style::Notch, w, h);
            let p = shape(Style::Pill, w, h);
            let apply = |m: windows_numerics::Matrix3x2, (x, y): (f32, f32)| (x * m.M11 + y * m.M21 + m.M31, x * m.M12 + y * m.M22 + m.M32);
            for t in [5.2, 5.4, 5.8, 6.1] {
                let case = format!("{w}x{h} t={t}");
                let notch = at(Style::Notch, w, h, t, 1.0).deform;
                assert!(notch.swing_deg.abs() > 0.3, "{case}: {notch:?}");
                assert_eq!(notch.shift, (0.0, 0.0), "{case}: the Notch hangs from the screen edge");
                let top = (n.x + n.w / 2.0, n.y);
                let q = apply(rigid(Style::Notch, n, &notch), top);
                assert!((q.0 - top.0).abs() < 1e-3 && (q.1 - top.1).abs() < 1e-3, "{case}: {q:?}");
                let pill = at(Style::Pill, w, h, t, 1.0).deform;
                assert!(pill.swing_deg.abs() > 0.3, "{case}: {pill:?}");
                let mid = (p.x + p.w / 2.0, p.y + p.h / 2.0);
                let q = apply(rigid(Style::Pill, p, &pill), mid);
                assert!((q.0 - mid.0).abs() < 1e-3 && (q.1 - mid.1 - pill.shift.1).abs() < 1e-3, "{case}: {q:?}");
            }
            // The pounce tips it left first (the mockup's -2.4 degrees), and
            // knocks the floating pill down off its height a little.
            for style in [Style::Notch, Style::Pill] {
                let rock = at(style, w, h, 5.25, 1.0).deform.swing_deg;
                assert!(rock < -2.0, "{style:?} {w}x{h}: {rock}");
            }
            let dy = at(Style::Pill, w, h, 5.45, 1.0).deform.shift.1;
            assert!(dy > 0.5 && dy < 2.0, "{w}x{h}: {dy}");
            // The wiggle starts from still, not with a step.
            for style in [Style::Notch, Style::Pill] {
                let jolt = |t: f32| at(style, w, h, t, 1.0).deform.jolt;
                assert_eq!(jolt(4.619), 0.0, "{style:?} {w}x{h}");
                assert!(jolt(4.621).abs() < 0.05, "{style:?} {w}x{h}: {}", jolt(4.621));
                let full = (0..40).map(|i| jolt(4.7 + i as f32 * 0.002).abs()).fold(0.0, f32::max);
                assert!(full > 0.75, "{style:?} {w}x{h}: {full}");
            }
            // Still before the pounce, the island only shakes.
            for style in [Style::Notch, Style::Pill] {
                let d = at(style, w, h, 4.9, 1.0).deform;
                assert_eq!((d.swing_deg, d.shift), (0.0, (0.0, 0.0)), "{style:?} {w}x{h}");
                assert!(d.jolt != 0.0, "{style:?} {w}x{h}: the wiggle");
            }
        }
    }

    #[test]
    fn the_peeking_head_is_never_inside_the_island() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let mut t = 0.0;
                while t < Story::PetakUmpet.secs() {
                    let st = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, true);
                    let shot = shot(&st);
                    // Every peeking head is cut to outside the island; the
                    // slot head dives in its own slot.
                    for (layer, at, ..) in heads(&shot) {
                        assert_eq!(layer, Layer::Back, "{style:?} {w}x{h} t={t}: a head at {at:?}");
                    }
                    assert!(matches!(shot.slot, Slot::Story(_) | Slot::App), "{style:?} {w}x{h} t={t}");
                    t += 0.02;
                }
                // At each peek's height it shows round its end.
                for (t, side) in [(2.0, -1.0), (3.4, 1.0), (4.6, -1.0), (6.0, 1.0)] {
                    let shot = at(style, w, h, t, 1.0);
                    let back: Vec<_> = shot.parts.iter().filter(|p| p.layer == Layer::Back && matches!(p.draw, Draw::Head { .. })).collect();
                    assert_eq!(back.len(), 1, "{style:?} {w}x{h} t={t}");
                    let b = back[0].draw.bounds();
                    let out = if side < 0.0 { s.x - b.x } else { b.x + b.w - (s.x + s.w) };
                    assert!(out > 5.0, "{style:?} {w}x{h} t={t}: only {out} past the end");
                }
            }
        }
    }

    #[test]
    fn its_paw_grips_the_rim_in_front_while_it_peeks() {
        // On the island as it moves: shaken by the wiggle, rocked and (the
        // Pill) lifted by the pounce, the paw stays on its rim, turned with it.
        let times = [(2.0, -1.0), (3.4, 1.0), (4.6, -1.0), (4.7, -1.0), (4.85, -1.0), (4.95, -1.0), (5.4, 1.0), (5.45, 1.0), (5.5, 1.0), (5.6, 1.0), (5.7, 1.0), (5.8, 1.0), (6.0, 1.0)];
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                for (t, side) in times {
                    let case = format!("{style:?} {w}x{h} t={t}");
                    let shot = at(style, w, h, t, 1.0);
                    let m = rigid(style, s, &shot.deform);
                    let jolt = shot.deform.jolt;
                    let moved = |x: f32, y: f32| (x * m.M11 + y * m.M21 + m.M31 + jolt, x * m.M12 + y * m.M22 + m.M32);
                    let edge: Vec<_> = (0..=2000).map(|i| rim(style, s, i as f32 / 2000.0)).map(|p| moved(p.x, p.y)).collect();
                    let paws: Vec<_> = shot.parts.iter().filter(|p| p.layer == Layer::Front).collect();
                    assert_eq!(paws.len(), 1, "{case}");
                    let Draw::Group { at: g, rot_deg, .. } = paws[0].draw else { panic!("{case}: {:?}", paws[0].draw) };
                    // Just outside the moved edge, low on its end's corner.
                    let d = edge.iter().map(|e| (e.0 - g.0).hypot(e.1 - g.1)).fold(f32::MAX, f32::min);
                    assert!((d - 0.4).abs() < 0.2, "{case}: {d} off the edge");
                    let turn = m.M12.atan2(m.M11).to_degrees();
                    assert!((rot_deg - (side * 70.0 + turn)).abs() < 1e-3, "{case}: turned {rot_deg}, the island {turn}");
                    let (lx, ly) = { let i = m; let (x, y) = (g.0 - jolt - i.M31, g.1 - i.M32); (x * i.M11 + y * i.M12, -x * i.M12 + y * i.M11) };
                    assert!(ly > s.y + s.h / 2.0, "{case}: {g:?}");
                    let end = if side < 0.0 { lx < s.x + s.h / 2.0 } else { lx > s.x + s.w - s.h / 2.0 };
                    assert!(end, "{case}: {g:?}");
                }
                // None while the cat hides.
                assert!(at(style, w, h, 1.2, 1.0).parts.iter().all(|p| p.layer != Layer::Front), "{style:?} {w}x{h}");
            }
        }
    }

    #[test]
    fn the_slot_head_dives_into_the_dark_and_comes_back_without_a_jump() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let case = format!("{style:?} {w}x{h}");
                let mut last: Option<((f32, f32), f32, f32)> = None;
                let (mut dark, mut t) = (false, 0.0);
                while t <= Story::PetakUmpet.secs() {
                    let st = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, true);
                    let shot = shot(&st);
                    let now = slot_head(&st, &shot);
                    if let (Some((p, k, a)), Some((q, j, b))) = (now, last) {
                        assert!((p.0 - q.0).abs() < 0.5 && (p.1 - q.1).abs() < 0.5 && (k - j).abs() < 0.02 && (a - b).abs() < 0.12,
                            "{case} t={t}: {q:?} {j} {b} to {p:?} {k} {a}");
                    }
                    if now.is_none() {
                        // All in the dark: it faded out first.
                        assert!(last.is_none_or(|l| l.2 < 0.12), "{case} t={t}: {last:?}");
                        dark = true;
                    } else if last.is_none() && t > 0.0 {
                        assert!(now.unwrap().2 < 0.12, "{case} t={t}: {now:?}");
                    }
                    last = now;
                    t += 0.01;
                }
                assert!(dark, "{case}: never in the dark");
                // Gone by the first peek, back after the last.
                assert!(slot_head(&stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t: 2.0, cut_at: None }, true), &at(style, w, h, 2.0, 1.0)).is_none(), "{case}");
                assert!(matches!(at(style, w, h, 7.4, 1.0).slot, Slot::Story(m) if m.alpha == 1.0), "{case}");
            }
        }
    }

    #[test]
    fn the_slot_head_darkens_with_its_halo_in_its_slot() {
        // The head fades into the dark as the mockup darkens it; its halo
        // fades with it (the shot's alpha), never apart.
        let mut t = 0.0;
        while t < Story::PetakUmpet.secs() {
            let shot = at(Style::Pill, 190.0, 34.0, t, 1.0);
            assert_eq!(shot.halo, 1.0, "t={t}");
            t += 0.05;
        }
        let alpha = |t: f32| match at(Style::Pill, 190.0, 34.0, t, 1.0).slot {
            Slot::Story(m) => m.alpha,
            other => panic!("t={t}: {other:?}"),
        };
        assert_eq!(at(Style::Pill, 190.0, 34.0, 0.45, 1.0).slot, Slot::Story(MascotShot::REST));
        assert!(alpha(0.7) > 0.5 && alpha(0.7) < 1.0, "{}", alpha(0.7));
        assert_eq!((alpha(0.86), alpha(4.0), alpha(7.4)), (0.0, 0.0, 1.0));
    }

    /// The largest alpha of any part, and how many show.
    fn showing(shot: &Shot) -> (f32, usize) {
        let on: Vec<_> = shot.parts.iter().filter(|p| p.alpha > 0.0).collect();
        (on.iter().map(|p| p.alpha).fold(0.0, f32::max), on.len())
    }

    #[test]
    fn a_cut_just_before_something_shows_never_brings_it_out_in_the_wind_down() {
        // Real wind-downs, the story's time running on while alpha falls:
        // cut just before the first peek, the tail tip and the mouthful,
        // nothing new shows.
        for cut_s in [1.55, 2.95, 4.25, 5.36] {
            let t0 = Instant::now();
            let mut play = Play::start(Story::PetakUmpet, t0);
            let cut = t0 + Duration::from_secs_f32(cut_s);
            play.end(cut);
            let mut last = (f32::INFINITY, usize::MAX);
            for ms in 0..=200 {
                let beat = play.beat(cut + Duration::from_millis(ms));
                let now = showing(&shot(&stage(Style::Notch, 190.0, 34.0, beat, true)));
                assert!(now.0 <= last.0 && now.1 <= last.1, "cut {cut_s} +{ms} ms (t={}): {last:?} to {now:?}", beat.t);
                last = now;
            }
        }
    }

    #[test]
    fn a_wind_down_brings_the_cat_home_through_its_slot() {
        // Cut short mid-peek, the slot head deep in the dark, a real
        // wind-down in 60 Hz frames: the peek fades and the island settles,
        // the slot head comes back out of the dark where it dived and rises
        // home, fading at most a frame's fade and moving under 1.5 dip a
        // frame of its own.
        use crate::ui::play::stories::{FRAME_FADE, wind_down_frames};
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    let frames = wind_down_frames(Story::PetakUmpet, 2.0, 0.0, |beat| shot(&stage(style, w, h, beat, clock)));
                    let mut last: Option<(((f32, f32), f32, f32), f32)> = None;
                    for (ms, beat, shot) in &frames {
                        let st = stage(style, w, h, *beat, clock);
                        let case = format!("{case} +{ms:.1} ms (alpha {})", beat.alpha);
                        if beat.alpha <= 0.5 {
                            assert!(is_rest(style, &shot.deform) && shot.parts.is_empty(), "{case}");
                        }
                        let now = slot_head(&st, shot);
                        let slot_x = st.room().robot.unwrap().x;
                        if let (Some((p, k, o)), Some(((q, j, b), was))) = (now, last) {
                            let own = (p.0 - q.0) - (slot_x - was);
                            assert!(own.abs() < 1.5 && (p.1 - q.1).abs() < 1.5 && (k - j).abs() < 0.05 && (o - b).abs() <= FRAME_FADE + 1e-4,
                                "{case}: {q:?} {j} {b} to {p:?} {k} {o}");
                        } else if let Some((_, _, o)) = now {
                            assert!(o <= FRAME_FADE + 1e-4, "{case}: out of the dark at {o}");
                        }
                        last = now.map(|n| (n, slot_x));
                    }
                    let (_, beat, end) = frames.last().unwrap();
                    assert_eq!(beat.alpha, 0.0, "{case}");
                    assert_eq!(*end, Shot::rest(style, shape(style, w, h)), "{case}");
                }
            }
        }
    }

    /// Every 60 Hz frame of a real wind-down cut at `cut_s` seconds.
    fn wind_down(style: Style, cut_s: f32) -> Vec<(f32, Beat, Shot)> {
        let t0 = Instant::now();
        let mut play = Play::start(Story::PetakUmpet, t0);
        let cut = t0 + Duration::from_secs_f32(cut_s);
        play.end(cut);
        let mut out = Vec::new();
        let mut ms = 0.0;
        while ms <= 220.0 {
            let beat = play.beat(cut + Duration::from_secs_f32(ms / 1000.0));
            out.push((ms, beat, shot(&stage(style, 190.0, 34.0, beat, true))));
            ms += 1000.0 / 60.0;
        }
        out
    }

    #[test]
    fn a_cut_just_before_the_wiggle_or_the_pounce_never_starts_it() {
        for style in [Style::Notch, Style::Pill] {
            for (ms, beat, shot) in wind_down(style, 4.55) {
                assert_eq!(shot.deform.jolt, 0.0, "{style:?} +{ms} ms (t={})", beat.t);
            }
            for (ms, beat, shot) in wind_down(style, 4.95) {
                let d = &shot.deform;
                assert!(d.swing_deg == 0.0 && d.shift == (0.0, 0.0) && d.dents.is_empty(), "{style:?} +{ms} ms (t={}): {d:?}", beat.t);
            }
        }
    }

    #[test]
    fn a_cut_near_the_end_eases_home_without_snapping_to_rest() {
        // Cut at 7.65 s the wind-down runs past the story's 7.8 s: the slot
        // head keeps its happy, blinking face to the last frame before rest.
        for (ms, beat, shot) in wind_down(Style::Notch, 7.65) {
            if beat.alpha == 0.0 {
                continue;
            }
            match shot.slot {
                Slot::Story(m) => assert!(m.expr == Expr::Happy && m.blink, "+{ms} ms (t={}): {m:?}", beat.t),
                other => panic!("+{ms} ms (t={}, alpha {}): {other:?}", beat.t, beat.alpha),
            }
        }
    }

    /// The head peeking at `t`.
    fn peeking(t: f32) -> MascotShot {
        heads(&at(Style::Notch, 190.0, 34.0, t, 1.0)).into_iter().find(|h| h.0 == Layer::Back).unwrap().2
    }

    #[test]
    fn the_faces_are_the_mockups_in_the_apps_expressions() {
        // Round eyes at the right end; looking ahead, then round-eyed at its
        // own tail; the squint (Focus) biting it, startled, then happy.
        assert_eq!(peeking(2.2).expr, Expr::Idle);
        assert!(peeking(2.2).glance.0 > 1.0, "looks toward the island");
        assert_eq!(peeking(3.4).expr, Expr::Alert);
        assert_eq!((peeking(4.3).expr, peeking(4.3).glance.0), (Expr::Idle, 0.0));
        assert_eq!((peeking(4.6).expr, peeking(4.6).glance.0), (Expr::Alert, 2.4));
        assert_eq!(peeking(5.7).expr, Expr::Focus);
        assert_eq!(peeking(6.1).expr, Expr::Alert);
        assert_eq!(peeking(6.4).expr, Expr::Happy);
        assert!(peeking(2.1).blink && !peeking(2.3).blink);
        // The slot head comes home happy, and blinks back to idle.
        let slot = |t: f32| match at(Style::Notch, 190.0, 34.0, t, 1.0).slot {
            Slot::Story(m) => m,
            other => panic!("t={t}: {other:?}"),
        };
        assert_eq!(slot(7.4).expr, Expr::Happy);
        assert!(slot(7.7).blink);
        assert_eq!(slot(7.76).expr, Expr::Idle);
    }

    /// The story's own tails: groups in the back layer.
    fn tails(shot: &Shot) -> Vec<&crate::ui::play::stories::Part> {
        shot.parts.iter().filter(|p| p.layer == Layer::Back && matches!(p.draw, Draw::Group { .. })).collect()
    }

    #[test]
    fn its_tail_tip_pokes_out_at_the_right_end_and_turns_up_in_its_mouth() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let case = format!("{style:?} {w}x{h}");
                // The tip: out past the right end, then yanked back in.
                let tip = at(style, w, h, 4.8, 1.0);
                assert_eq!(tails(&tip).len(), 1, "{case}");
                let b = tails(&tip)[0].draw.bounds();
                assert!(b.x + b.w > s.x + s.w + 4.0, "{case}: {b:?}");
                assert!(tails(&at(style, w, h, 4.25, 1.0)).is_empty() && tails(&at(style, w, h, 5.3, 1.0)).is_empty(), "{case}");
                // In its mouth: the tail reaches the peeking head's mouth,
                // drawn over the head.
                let bite = at(style, w, h, 5.9, 1.0);
                let t = tails(&bite);
                assert_eq!(t.len(), 1, "{case}");
                let head = bite.parts.iter().position(|p| matches!(p.draw, Draw::Head { .. })).unwrap();
                let tail = bite.parts.iter().position(|p| std::ptr::eq(p, t[0])).unwrap();
                assert!(tail > head, "{case}: the tail is under the head");
                let (_, pivot, m, _) = heads(&bite)[0];
                let a = m.rot_deg.to_radians();
                let mouth = (pivot.0 - 3.4 * a.sin(), pivot.1 + 3.4 * a.cos());
                let b = t[0].draw.bounds();
                assert!(mouth.0 >= b.x - 1.0 && mouth.0 <= b.x + b.w + 1.0 && mouth.1 >= b.y - 1.0 && mouth.1 <= b.y + b.h + 1.0, "{case}: {mouth:?} {b:?}");
                assert!(tails(&at(style, w, h, 6.3, 1.0)).is_empty(), "{case}");
            }
        }
    }

    #[test]
    fn the_tail_and_the_paw_wear_the_coat() {
        fn fills(d: &Draw, out: &mut Vec<u32>) {
            match d {
                Draw::Fill { rgb, .. } | Draw::Ellipse { rgb, .. } => out.push(*rgb),
                Draw::Group { items, .. } | Draw::Clipped { items, .. } => items.iter().for_each(|i| fills(i, out)),
                _ => {}
            }
        }
        for color in [CatColor::Terracotta, CatColor::Calico, CatColor::White, CatColor::Black] {
            let look = CatLook { mascot: Mascot::Cat, color, ..CatLook::default() };
            for t in [4.8, 5.9] {
                let st = Stage { look, ..stage(Style::Notch, 190.0, 34.0, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, true) };
                let shot = shot(&st);
                for layer in [Layer::Back, Layer::Front] {
                    let mut rgb = Vec::new();
                    shot.parts.iter().filter(|p| p.layer == layer && matches!(p.draw, Draw::Group { .. })).for_each(|p| fills(&p.draw, &mut rgb));
                    assert!(rgb.contains(&look.head_rgb()), "{color:?} t={t} {layer:?}: {rgb:x?}");
                }
            }
        }
    }
}
