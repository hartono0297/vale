//! The cat's "Petak umpet" (hide and seek): the slot head dives into the
//! island's dark; a bump runs along the lip each time the cat moves behind
//! it; it peeks round the left end, then the right, a paw gripping the edge;
//! peeking from the left again, its own tail tip pokes out at the right end;
//! taking it for another cat it wiggles (the island shakes) and pounces (the
//! bump shoots across, the island rocks); it turns up at the right end
//! biting its own tail, lets go, and the slot head comes back up. Timeline,
//! easing and offsets from `docs/design/maskot-notch-v3.html`, story `c1`.

use std::f32::consts::{PI, TAU};

use super::{Draw, Ease, HEAD_PIVOT, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, island_box, kf, lerp, rim_pt, ring};
use crate::config::Style;
use crate::ui::geometry::{ROBOT_COMPACT, corner_radius};
use crate::ui::play::art::cat::{CAT_SC, slim_tail, small_paw, tail};
use crate::ui::play::art::tube::{along, catmull_rom, curl, plen};
use crate::ui::play::shape::{Deform, lip_dent};
use crate::ui::play::trigger::Story;
use crate::ui::robot::{Expr, GRID};

use Ease::{I, Lin, O, Ob, Qi, Sb, Sio};

/// The slot head is back up from the dark here (its soft overshoot crosses
/// rest), and its halo returns over the next 0.1 s.
const RISEN: f32 = 6.95 + 0.4 * (1.0 - 0.9 / 1.9);

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
/// `beat.alpha` eases it home: from 1 to 0.5 the peeks fade and the island
/// settles, to 0.25 the slot head comes back out of the dark where it dived,
/// to 0 it rises home and takes its halo back. Cut short, the story holds
/// where the cut found it, so nothing it had not brought out shows. Exactly
/// at rest at 0 s, at the end and with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, .. } = *stage;
    let a = beat.alpha.clamp(0.0, 1.0);
    let mut out = Shot::rest(style, s);
    if beat.t <= 0.0 || beat.t >= Story::PetakUmpet.secs() || a == 0.0 {
        return out;
    }
    // The slot where the renderer puts it (the clock on or off moves it).
    let Some(slot) = stage.room().robot else {
        return out;
    };
    let t = beat.cut_at.map_or(beat.t, |c| beat.t.min(c));
    let play = ((a - 0.5) * 2.0).clamp(0.0, 1.0);
    let k = slot.w / GRID;
    let (slot_x, slot_y) = (slot.x + slot.w / 2.0, slot.y + HEAD_PIVOT.1 * k);

    // The slot head ducks into the dark and later comes back up, its halo
    // giving way first and returning last: darkening, it is a part (a part
    // has no halo), which the slot head hands over to where the halo is gone.
    let away = kf(t, &[(0.5, 0.0, Lin), (0.85, 1.0, I), (6.95, 1.0, Lin), (7.35, 0.0, Sb)]);
    let duck = away * (2.0 * a).min(1.0);
    let dark = away.max(0.0) * ((a - 0.25) * 4.0).clamp(0.0, 1.0);
    let halo = kf(t, &[(0.4, 1.0, Lin), (0.5, 0.0, Lin), (RISEN, 0.0, Lin), (RISEN + 0.1, 1.0, Lin)]);
    out.halo = lerp(1.0, halo, (4.0 * a).min(1.0));
    let head = MascotShot {
        dy: 4.0 * duck,
        sx: 1.0 - 0.15 * duck,
        sy: 1.0 - 0.15 * duck,
        expr: if (7.0..7.7).contains(&t) { Expr::Happy } else { Expr::Idle },
        blink: blinks(t, &[7.7]) > 0.25,
        ..MascotShot::REST
    };
    if dark > 0.0 {
        out.slot = Slot::Empty;
        if dark < 1.0 {
            let size = slot.w / ROBOT_COMPACT;
            let shot = MascotShot { dy: 0.0, sx: head.sx * size, sy: head.sy * size, ..head };
            out.parts.push(Part { layer: Layer::Dark, draw: Draw::Head { at: (slot_x, slot_y + head.dy), shot }, alpha: 1.0 - dark });
        }
    } else {
        out.slot = Slot::Story(head);
    }

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
    let wiggle = if (4.62..5.0).contains(&t) { 0.8 * (TAU * 14.0 * t).sin() } else { 0.0 };
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
    // below the chin, in front.
    let unit = CAT_SC * ROBOT_COMPACT / GRID;
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
                at: rim_pt(style, s, side, 0.73, 0.4),
                rot_deg: side * 70.0,
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
    use crate::ui::geometry::ROBOT_COMPACT;
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

    /// The slot head as the frame shows it, wherever it is drawn: its pivot,
    /// its dip per grid unit and its opacity. `None` while it is all in the
    /// dark.
    fn slot_head(st: &Stage, shot: &Shot) -> Option<((f32, f32), f32, f32)> {
        let r = st.room().robot.unwrap();
        let k = r.w / GRID;
        match shot.slot {
            Slot::Story(m) => Some(((r.x + 32.0 * k + m.dx, r.y + 40.0 * k + m.dy), k * m.sx, 1.0)),
            Slot::App => Some(((r.x + 32.0 * k, r.y + 40.0 * k), k, 1.0)),
            Slot::Empty => heads(shot)
                .into_iter()
                .find(|h| h.0 == Layer::Dark)
                .map(|(_, at, m, alpha)| (at, m.sx * ROBOT_COMPACT / GRID, alpha)),
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
            // The floating pill is knocked off its height too, a little.
            let dy = at(Style::Pill, w, h, 5.45, 1.0).deform.shift.1;
            assert!(dy.abs() > 0.5 && dy.abs() < 2.0, "{w}x{h}: {dy}");
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
                    let slot_x = st.room().robot.map(|r| r.x + r.w / 2.0).unwrap();
                    // Only the slot head, diving, is drawn in the island's
                    // dark; every peeking head is cut to outside it.
                    for (layer, at, ..) in heads(&shot) {
                        assert!(layer == Layer::Back || (layer == Layer::Dark && (at.0 - slot_x).abs() < 0.01),
                            "{style:?} {w}x{h} t={t}: a head in {layer:?} at {at:?}");
                    }
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
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                let s = shape(style, w, h);
                let edge: Vec<_> = (0..=2000).map(|i| rim(style, s, i as f32 / 2000.0)).collect();
                for (t, side) in [(2.0, -1.0), (3.4, 1.0), (4.6, -1.0), (6.0, 1.0)] {
                    let case = format!("{style:?} {w}x{h} t={t}");
                    let shot = at(style, w, h, t, 1.0);
                    let paws: Vec<_> = shot.parts.iter().filter(|p| p.layer == Layer::Front).collect();
                    assert_eq!(paws.len(), 1, "{case}");
                    let Draw::Group { at: g, .. } = paws[0].draw else { panic!("{case}: {:?}", paws[0].draw) };
                    // Just outside the edge, low on its end's corner.
                    let d = edge.iter().map(|e| (e.x - g.0).hypot(e.y - g.1)).fold(f32::MAX, f32::min);
                    assert!((d - 0.4).abs() < 0.2, "{case}: {d} off the edge");
                    assert!(g.1 > s.y + s.h / 2.0, "{case}: {g:?}");
                    let end = if side < 0.0 { g.0 < s.x + s.h / 2.0 } else { g.0 > s.x + s.w - s.h / 2.0 };
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
                let mut was_part = false;
                let (mut dark, mut t) = (false, 0.0);
                while t <= Story::PetakUmpet.secs() {
                    let st = stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t, cut_at: None }, true);
                    let shot = shot(&st);
                    let now = slot_head(&st, &shot);
                    let part = heads(&shot).iter().any(|h| h.0 == Layer::Dark);
                    // Drawn as a part, the slot is empty; handed back to the
                    // slot, its halo starts from nothing.
                    if part {
                        assert_eq!(shot.slot, Slot::Empty, "{case} t={t}");
                    }
                    if was_part && !part {
                        assert!(shot.halo < 0.15, "{case} t={t}: halo {}", shot.halo);
                    }
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
                    was_part = part;
                    t += 0.01;
                }
                assert!(dark, "{case}: never in the dark");
                // Gone by the first peek, back after the last.
                assert!(slot_head(&stage(style, w, h, Beat { spread: 1.0, alpha: 1.0, t: 2.0, cut_at: None }, true), &at(style, w, h, 2.0, 1.0)).is_none(), "{case}");
                assert!(matches!(at(style, w, h, 7.4, 1.0).slot, Slot::Story(_)), "{case}");
            }
        }
    }

    #[test]
    fn the_slot_heads_halo_gives_way_before_it_dives_and_returns_after() {
        let halo = |t: f32| at(Style::Pill, 190.0, 34.0, t, 1.0).halo;
        assert_eq!(halo(0.3), 1.0);
        assert!(halo(0.45) > 0.3 && halo(0.45) < 0.7);
        assert_eq!(halo(0.5), 0.0);
        assert_eq!(halo(7.6), 1.0);
        assert_eq!(at(Style::Pill, 190.0, 34.0, 0.45, 1.0).slot, Slot::Story(MascotShot::REST));
    }

    /// The largest alpha of any part but the slot head coming home (in the
    /// dark), and how many show.
    fn showing(shot: &Shot) -> (f32, usize) {
        let on: Vec<_> = shot.parts.iter().filter(|p| p.alpha > 0.0 && p.layer != Layer::Dark).collect();
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
        // Cut short mid-peek, the slot head deep in the dark, the spread
        // falling with alpha: the peek fades, the island settles, the slot
        // head fades back in where it dived and rises home, never jumping.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    let mut last: Option<(((f32, f32), f32, f32), f32)> = None;
                    for i in (0..=100).rev() {
                        let a = i as f32 / 100.0;
                        let st = stage(style, w, h, Beat { spread: a, alpha: a, t: 2.0, cut_at: Some(2.0) }, clock);
                        let shot = shot(&st);
                        if a <= 0.5 {
                            assert!(is_rest(style, &shot.deform), "{case} alpha={a}");
                            assert!(shot.parts.iter().all(|p| p.layer == Layer::Dark), "{case} alpha={a}");
                        }
                        let now = slot_head(&st, &shot);
                        let slot_x = st.room().robot.unwrap().x;
                        if let (Some((p, k, o)), Some(((q, j, b), was))) = (now, last) {
                            let own = (p.0 - q.0) - (slot_x - was);
                            assert!(own.abs() < 0.6 && (p.1 - q.1).abs() < 0.6 && (k - j).abs() < 0.02 && (o - b).abs() < 0.1,
                                "{case} alpha={a}: {q:?} {j} {b} to {p:?} {k} {o}");
                        }
                        if a < 0.25 {
                            assert!(now.is_some(), "{case} alpha={a}: no slot head");
                        }
                        last = now.map(|n| (n, slot_x));
                    }
                    let s = shape(style, w, h);
                    let rest = shot(&stage(style, w, h, Beat { spread: 0.0, alpha: 0.0, t: 2.0, cut_at: Some(2.0) }, clock));
                    assert_eq!(rest, Shot::rest(style, s), "{case}");
                }
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
