//! The falk's "Terbang keliling" (a little flight): it looks left and right
//! and chirps in its slot, crouches and drops out of it, the body unfolding
//! below the lip, flaps out, rolls once round a loop under the island (a
//! feather coming loose), glides under the clock glancing up and chirping,
//! swings round behind the right end, comes back low, brakes with its wings
//! flared forward and rises into its slot, the body folding back under the
//! head. The island does nothing. Timeline, easing and path from
//! `docs/design/maskot-notch-v3.html`, story `f3`.

use std::f32::consts::TAU;

use super::{Draw, Ease, HEAD_PIVOT, Layer, MascotShot, Part, Shot, Slot, Stage, blinks, bump, kept, kf, lerp, ring};
use crate::ui::geometry::{Rect, ShapeRect};
use crate::ui::mascot::CrestPose;
use crate::ui::play::art::falk::{BirdPose, WingPose, bird_at, bird_parts, falk_coat, plume};
use crate::ui::play::art::tube::{along, catmull_rom, measure};
use crate::ui::play::state::spread_offsets;
use crate::ui::play::trigger::Story;
use crate::ui::robot::{Expr, GRID, Seg};

use Ease::{Io, Lin, O, Qi, Sb, Sio};

type Pt = (f32, f32);

/// The bird takes the head over from the slot at `OUT` and hands it back at
/// `HOME`; for `HAND` after `OUT` and before `HOME` both show, in the same
/// place, the bird fading in over the slot head (or out from over it).
const OUT: f32 = 1.15;
const HOME: f32 = 7.1;
const HAND: f32 = 0.1;
/// Where `art::falk` puts the head's pivot, above the feet (bird-local dip).
const HEAD_AT: Pt = (0.0, -24.0);
/// The feet below the body's centre, and so the centre below the head's
/// pivot while the bird is upright.
const FEET: f32 = 10.0;
const NECK: f32 = -HEAD_AT.1 - FEET;
/// How far the crouch lowers the slot head before the jump, dip.
const CROUCH_DY: f32 = 2.5;
/// The last blink, home again; the face is idle after it.
const LAST_BLINK: f32 = 7.72;
/// The chirp marks' ink, the mockup's `CREAM`.
const CREAM: u32 = 0xF4EDE4;

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn add(a: Pt, b: Pt) -> Pt {
    (a.0 + b.0, a.1 + b.1)
}

/// `v` turned `deg` clockwise (the mockup's `R`).
fn turn(deg: f32, v: Pt) -> Pt {
    let (sin, cos) = deg.to_radians().sin_cos();
    (v.0 * cos - v.1 * sin, v.0 * sin + v.1 * cos)
}

/// How far along the flight path the bird is at `t`, 0 to 1 (`f3u`).
fn travel(t: f32) -> f32 {
    // The last stretch eases out quadratically (the mockup's `qo`).
    if t > 5.95 {
        let x = clamp01((t - 5.95) / (HOME - 5.95));
        return 0.92 + 0.08 * (1.0 - (1.0 - x) * (1.0 - x));
    }
    kf(t, &[(OUT, 0.0, Lin), (1.8, 0.065, Qi), (3.1, 0.33, Lin), (3.95, 0.43, Lin), (5.95, 0.92, Lin)])
}

/// The flight path of the body's centre (the mockup's `f3path`), one
/// Catmull-Rom curve through points taken from the island's edges and the
/// settled slot, so it holds at any size and in both styles.
struct Flight {
    pts: Vec<Pt>,
    tot: f32,
    /// The stretch it rolls over, as shares of the path: the loop.
    roll: (f32, f32),
}

impl Flight {
    /// Out of the slot under the crouched head, down and round a loop under
    /// the island's left part, a glide under the clock, round behind the
    /// right end (its innermost points inside the end's arc, so on the Pill
    /// the bird is really gone there), back low and up into the slot under
    /// the resting head. `home` is the slot where a play puts it.
    fn new(s: ShapeRect, home: Rect) -> Flight {
        let k = home.w / GRID;
        let (s0, head) = (home.x + HEAD_PIVOT.0 * k, home.y + HEAD_PIVOT.1 * k);
        let (cx, q) = (s.x + s.w / 2.0, s.w / 2.0);
        let (right, mid) = (s.x + s.w, s.y + s.h / 2.0);
        let x = |f: f32| cx + q * f;
        let by = |d: f32| s.y + s.h + d;
        let mut p = vec![(s0, head + CROUCH_DY + NECK), (s0 - 2.5, by(13.0)), (s0 + 3.0, by(27.0)), (s0 + 19.0, by(39.0)), (s0 + 36.0, by(42.5))];
        p.extend((0..=12).map(|i| {
            let a = (90.0 - 30.0 * i as f32).to_radians();
            (s0 + 54.0 + 10.0 * a.cos(), by(33.0) + 10.0 * a.sin())
        }));
        p.extend([
            (x(0.02), by(42.0)),
            (x(0.36), by(34.0)),
            (x(0.66), by(28.0)),
            (x(0.94), by(18.0)),
            (right + 15.0, by(2.0)),
            (right + 9.0, mid + 3.0),
            (right - 4.0, mid + 5.0),
            (right - 1.0, by(6.0)),
            (x(0.8), by(24.0)),
            (x(0.44), by(34.0)),
            (x(0.04), by(37.0)),
            (x(-0.36), by(34.0)),
            (s0 + 18.0, by(28.0)),
            (s0 + 6.0, by(20.0)),
            (s0 + 1.0, by(11.0)),
            (s0, head + NECK),
        ]);
        let pts = catmull_rom(&p, 6);
        let c = measure(&pts);
        let tot = c[c.len() - 1];
        // The loop runs from the 6th point to the 18th, 6 samples a span.
        Flight { roll: (c[5 * 6] / tot, c[17 * 6] / tot), pts, tot }
    }

    /// The body's centre at `t`.
    fn at(&self, t: f32) -> Pt {
        along(&self.pts, self.tot * travel(t), 1)[1]
    }
}

/// What drives the wings at one moment, each 0 to 1: the flapping's
/// strength, the glide, the landing flare, how far the wings have opened
/// after the jump and folded on the way in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Drive {
    power: f32,
    glide: f32,
    flare: f32,
    open: f32,
    fold: f32,
}

/// Both wings' pose at `tt` (the mockup's `pose`).
fn wing_pose(tt: f32, d: Drive) -> WingPose {
    let th = TAU * 4.4 * tt;
    let up = (-th.cos()).max(0.0) * d.power;
    let swing = 34.0 * (th + 0.35 * th.sin()).sin() * d.power;
    let flap = WingPose { arm: -14.0 + swing - d.glide * 9.0, hand: 10.0 + 14.0 * up, ext: 1.0 - 0.2 * up, fan: 1.0 - 0.38 * up, sweep: 92.0 };
    let flare = WingPose { arm: WingPose::FLARE.arm + 7.0 * (TAU * 9.0 * tt).sin(), ..WingPose::FLARE };
    let p = WingPose::mix(flap, flare, d.flare);
    let p = WingPose::mix(WingPose::FOLD, p, d.open);
    WingPose::mix(p, WingPose::FOLD, d.fold)
}

/// The crest's height through the story (`crestUp`), back to 1 by 7.85 so
/// the story ends at rest.
fn crest_up(t: f32) -> f32 {
    kf(t, &[(0.75, 1.0, Lin), (0.95, 1.2, Sio), (1.05, 0.65, Sio), (1.4, 0.7, Io), (6.6, 0.75, Io), (7.15, 1.3, Sb), (7.6, 1.1, Io), (7.85, 1.0, Sio)])
}

/// The flying bird at one moment.
struct Flying {
    /// The body's centre, on the path.
    centre: Pt,
    /// The feet's point and the bird's turn (`birdG`).
    org: Pt,
    rot: f32,
    /// How far the body has unfolded below the head, 0.12 to 1.
    g: f32,
    // The tests check the wings against it.
    #[cfg_attr(not(test), allow(dead_code))]
    drive: Drive,
    pose: BirdPose,
    chirp: f32,
}

impl Flying {
    /// The head's pivot, window dip.
    fn head(&self) -> Pt {
        add(self.org, turn(self.rot, HEAD_AT))
    }
}

/// The bird at `t` (between `OUT` and `HOME`) on `fl` round the island `s`.
/// At `OUT` and `HOME` its head is exactly the slot head's (crouched, then
/// at rest): no bob, crest wobble or lean yet (or any more), the crest
/// turned as the slot's, the head scaled as the slot's.
fn flying(t: f32, fl: &Flight, s: ShapeRect) -> Flying {
    let u = travel(t);
    let (centre, before) = (fl.at(t), fl.at(t - 0.04));
    let vx = (centre.0 - before.0) / 0.04;
    let (cx, q) = (s.x + s.w / 2.0, s.w / 2.0);
    let (g0, g1) = (cx + 0.12 * q, cx + 0.88 * q);
    let glide = if centre.0 > g0 && centre.0 < g1 && t < 4.5 { clamp01((centre.0 - g0) / 12.0) * clamp01((g1 - centre.0) / 12.0) } else { 0.0 };
    let flare = Sio.at(clamp01((t - 6.35) / 0.35)) * (1.0 - Sio.at(clamp01((t - 6.85) / 0.25)));
    let fold = Sio.at(clamp01((t - 6.8) / 0.3));
    let open = O.at(clamp01((t - 1.25) / 0.25));
    let drive = Drive { power: (1.0 - glide) * (1.0 - flare) * (1.0 - fold), glide, flare, open, fold };
    let th = TAU * 4.4 * t;
    let bob = -1.7 * (th - 0.8).sin() * drive.power * open;
    let roll = clamp01((u - fl.roll.0) / (fl.roll.1 - fl.roll.0));
    let lean = (vx * 0.11).clamp(-15.0, 15.0) * (1.0 - flare) * (1.0 - fold);
    let rot = lean - 360.0 * Sio.at(roll);
    let g = kf(t, &[(OUT, 0.12, Lin), (1.45, 1.0, O), (6.82, 1.0, Lin), (HOME, 0.12, Sio)]);
    let chirp = bump(t, 3.5, 0.22) + bump(t, 3.78, 0.2);
    // The crest leaves the slot's crouch (-4) and comes home to rest (10).
    let flying_crest = -12.0 - vx.abs() * 0.05 + 8.0 * flare;
    let crest_rot = lerp(lerp(-4.0, flying_crest, O.at(clamp01((t - OUT) / 0.3))), 10.0, fold);
    let hsy = kf(t, &[(OUT, 0.84, Lin), (1.3, 1.1, O), (1.6, 1.0, Io)]);
    let face = MascotShot {
        sx: 1.0 / hsy.sqrt(),
        sy: hsy,
        expr: if (roll > 0.05 && roll < 0.95) || chirp > 0.1 { Expr::Happy } else { Expr::Idle },
        crest: CrestPose { rot_deg: crest_rot, scale: crest_up(t) + 0.12 * (th - 1.6).sin() * drive.power * open },
        glance: (
            (vx / 70.0).clamp(-1.0, 1.0) * (1.0 - flare) * (1.0 - fold) * 2.4,
            if (3.3..3.95).contains(&t) { -2.2 } else { 0.0 },
        ),
        beak_open: chirp > 0.2,
        ..MascotShot::REST
    };
    let (now, then) = (wing_pose(t, drive), wing_pose(t - 0.06, drive));
    let pose = BirdPose {
        wl: now,
        wr: now,
        wl_before: then,
        wr_before: then,
        wl_front: false,
        wr_front: false,
        face,
        tail_rot: -lean * 0.35 + 7.0 * (th - 1.3).sin() * drive.power,
        tail_spread: 14.0 * flare,
        tuck: fold < 0.5 && flare < 0.3,
    };
    let org = add(add(centre, (0.0, bob)), turn(rot, (0.0, FEET)));
    Flying { centre, org, rot, g, drive, pose, chirp }
}

/// The slot head before the jump: a look left and right, a happy chirp, the
/// crouch (lower, squashed, crest laid back), the look drifting back to the
/// middle so the bird takes it over looking ahead.
fn in_slot(t: f32) -> MascotShot {
    let crouch = Sio.at(clamp01((t - 0.8) / 0.35));
    let sy = 1.0 - 0.16 * crouch;
    let look = kf(t, &[(0.1, 0.0, Lin), (0.25, -1.0, Io), (0.45, -1.0, Io), (0.6, 0.6, Io), (0.9, 0.6, Io), (OUT, 0.0, Io)]);
    MascotShot {
        dy: CROUCH_DY * crouch,
        sx: 1.0 / sy.sqrt(),
        sy,
        expr: if (0.45..0.8).contains(&t) { Expr::Happy } else { Expr::Idle },
        crest: CrestPose { rot_deg: 10.0 - 14.0 * crouch, scale: crest_up(t) },
        glance: (look * 2.4, 0.0),
        blink: blinks(t, &[0.15]) > 0.25,
        beak_open: bump(t, 0.5, 0.22) > 0.2,
        ..MascotShot::REST
    }
}

/// The slot head home again: the landing's squash, turn and crest rings,
/// calmed to nothing by 7.85, a happy chirp, a blink, then the idle face.
fn home_again(t: f32) -> MascotShot {
    let calm = 1.0 - Sio.at(clamp01((t - 7.45) / 0.4));
    let sy = 1.0 + ring(t, HOME, -0.1, 2.4, 4.5) * calm;
    MascotShot {
        rot_deg: ring(t, HOME, 3.0, 2.0, 4.0) * calm,
        sx: 1.0 / sy.sqrt(),
        sy,
        expr: if (7.35..LAST_BLINK).contains(&t) { Expr::Happy } else { Expr::Idle },
        crest: CrestPose { rot_deg: 10.0 + ring(t, HOME, 14.0, 2.2, 3.0) * calm, scale: crest_up(t) },
        blink: blinks(t, &[LAST_BLINK]) > 0.25,
        beak_open: bump(t, 7.45, 0.22) > 0.2,
        ..MascotShot::REST
    }
}

/// `m` eased toward the resting head as `e` falls from 1 to 0.
fn homeward(m: MascotShot, e: f32) -> MascotShot {
    if e >= 1.0 {
        return m;
    }
    let r = MascotShot::REST;
    let on = e > 0.5;
    MascotShot {
        dx: m.dx * e,
        dy: m.dy * e,
        rot_deg: m.rot_deg * e,
        sx: lerp(r.sx, m.sx, e),
        sy: lerp(r.sy, m.sy, e),
        expr: if on { m.expr } else { r.expr },
        crest: CrestPose { rot_deg: lerp(r.crest.rot_deg, m.crest.rot_deg, e), scale: lerp(r.crest.scale, m.crest.scale, e) },
        glance: (m.glance.0 * e, m.glance.1 * e),
        blink: m.blink && on,
        beak_open: m.beak_open && on,
        ..m
    }
}

/// The chirp's two small marks beside a head whose pivot is `head`.
fn chirp_marks(head: Pt) -> Draw {
    let (x, y) = (head.0 + 9.0, head.1 - 6.0);
    Draw::Stroke {
        path: vec![
            Seg::Move(x, y),
            Seg::Quad(x + 2.2, y - 1.2, x + 2.6, y - 3.4),
            Seg::Move(x + 1.6, y + 2.6),
            Seg::Quad(x + 4.2, y + 2.2, x + 5.4, y + 0.2),
        ],
        rgb: CREAM,
        width: 0.9,
    }
}

/// A feather let go at `from` at `p0`, drifting down, swaying and turning
/// (the mockup's `drift`), and how much of it shows: 2.4 s, faded in over
/// 0.1 s and out over the last 0.6.
fn feather(t: f32, from: f32, p0: Pt, rgb: u32, rim: u32) -> Option<(Draw, f32)> {
    let d = t - from;
    if !(0.0..=2.4).contains(&d) {
        return None;
    }
    let sway = TAU * 0.85 * d;
    let at = (p0.0 + 6.0 * sway.sin() + 4.0 * d, p0.1 + 9.0 * d + 3.0 * d * d);
    let items = vec![
        Draw::Fill { path: plume((-3.2, 0.0), 0.0, 6.8, 3.2), rgb: rim },
        Draw::Fill { path: plume((-3.0, 0.0), 0.0, 6.2, 2.4), rgb },
        Draw::Stroke { path: vec![Seg::Move(-3.0, 0.0), Seg::Line(2.0, 0.0)], rgb: rim, width: 0.35 },
    ];
    let draw = Draw::Group { at, rot_deg: 70.0 + 40.0 * (sway + 0.8).sin(), scale: (1.0, 1.0), pivot: (0.0, 0.0), items };
    Some((draw, clamp01((2.4 - d) / 0.6) * clamp01(d / 0.1)))
}

/// One moment of the story on `stage`, the falk in the coat of its look.
/// The slot head plays the start and the end; between `OUT` and `HOME` the
/// slot is empty and the bird flies, its body behind the island (clipped to
/// outside it, so it unfolds from and folds back under the lip), its head in
/// front of the island while the bird is on the island's left half and
/// behind it otherwise.
///
/// `beat.alpha` eases it home, paced by the wind-down's time (`kept`). Cut
/// short, the story holds where the cut found it: the bird (and a feather or
/// a chirp) fades out where it is over the wind-down's first 0.6 while,
/// from 0.4, the slot head fades in at rest in its slot (one already in the
/// slot eases home from 0.6 instead). The flight is laid against the settled slot,
/// so the spread closing does not drag the fading bird along. Exactly at
/// rest at 0 s, at the end and with `alpha` 0.
pub fn shot(stage: &Stage) -> Shot {
    let Stage { style, shape: s, beat, look, layout } = *stage;
    let (t, a) = (beat.cut_at.map_or(beat.t, |c| beat.t.min(c)), beat.alpha.clamp(0.0, 1.0));
    let mut out = Shot::rest(style, s);
    if t <= 0.0 || t >= Story::TerbangKeliling.secs() || a == 0.0 {
        return out;
    }
    // The slot where the renderer puts it now, and where a play settles it.
    let (Some(slot), Some(home)) = (stage.room().robot, spread_offsets(layout, s, 1.0).robot) else {
        return out;
    };
    let k = slot.w / GRID;
    let pivot = (slot.x + HEAD_PIVOT.0 * k, slot.y + HEAD_PIVOT.1 * k);
    // The bird fades over the wind-down's first 0.6; an empty slot fades its
    // head back in from 0.4; a slot head already showing goes home from 0.6.
    let (fly, back, stay) = (kept(a, 0.0, 0.6), kept(a, 0.4, 1.0), kept(a, 0.6, 1.0));
    let coat = falk_coat(look.falk);
    let fl = Flight::new(s, home);
    let mut front = Vec::new();

    // A feather from the loop and one from the landing flare, in front; the
    // second is gone before the end.
    for (from, off, rgb) in [(2.8, (0.0, -6.0), coat.prim), (6.75, (6.0, 2.0), coat.sec)] {
        if let Some((draw, op)) = feather(t, from, add(fl.at(from), off), rgb, coat.rim) {
            let end = if from > 6.0 { clamp01((7.85 - t) / 0.4) } else { 1.0 };
            let alpha = op * end * fly;
            if alpha > 0.0 {
                front.push(Part { layer: Layer::Front, draw, alpha });
            }
        }
    }

    let bird = (OUT..=HOME).contains(&t).then(|| flying(t, &fl, s));
    let show = if bird.is_some() { clamp01((t - OUT) / HAND).min(clamp01((HOME - t) / HAND)) } else { 0.0 };
    // The slot head as the story plays it: its own before and after the
    // flight; at each hand-over the bird's head, placed from the slot.
    let played = match &bird {
        _ if t < OUT => Some(in_slot(t)),
        _ if t > HOME => Some(home_again(t)),
        Some(b) if show < 1.0 => {
            let head = b.head();
            Some(MascotShot { dx: head.0 - pivot.0, dy: head.1 - pivot.1, rot_deg: (b.rot + 540.0).rem_euclid(360.0) - 180.0, ..b.pose.face })
        }
        _ => None,
    };
    // Cut inside a hand-over window the slot head (whole) and the fading bird
    // add up past one head: accepted, as they are the same head in the same
    // place, and the slot head only goes home once the bird has gone.
    out.slot = match played {
        Some(m) => Slot::Story(homeward(m, stay)),
        None if back < 1.0 => Slot::Story(MascotShot { alpha: 1.0 - back, ..MascotShot::REST }),
        None => Slot::Empty,
    };
    let shown = show * fly;
    out.halo = 1.0 - shown;
    if bird.is_none() {
        let chirp = bump(t, 0.5, 0.22) + bump(t, 7.45, 0.22);
        if let (Slot::Story(m), true) = (out.slot, chirp > 0.2) {
            front.push(Part { layer: Layer::Front, draw: chirp_marks((pivot.0 + m.dx, pivot.1 + m.dy)), alpha: clamp01(chirp) * fly });
        }
    }

    if let Some(b) = bird.filter(|_| shown > 0.0) {
        let parts = bird_parts(&b.pose, look.falk);
        let mut body = vec![parts.tail];
        body.extend(parts.body);
        body.extend(parts.feet);
        // The body unfolds below the head on the way out and folds back
        // under it on the way in.
        let folded = Draw::Group { at: (0.0, -17.0), rot_deg: 0.0, scale: (lerp(0.75, 1.0, b.g), b.g), pivot: (0.0, -17.0), items: body };
        out.parts.push(Part { layer: Layer::Back, draw: bird_at(b.org, b.rot, 1.0, 1.0, vec![folded]), alpha: shown });
        // One head: in front while the bird is on the island's left half (it
        // leaves and enters its slot there), behind the island otherwise (so
        // it is gone behind the right end). The centre crosses the middle
        // only well under the lip, where neither layer cuts any of the head.
        let head = bird_at(b.org, b.rot, 1.0, 1.0, vec![parts.head]);
        if b.centre.0 < s.x + s.w / 2.0 {
            front.push(Part { layer: Layer::Front, draw: head, alpha: shown });
        } else {
            out.parts.push(Part { layer: Layer::Back, draw: head, alpha: shown });
        }
        if b.chirp > 0.2 {
            out.parts.push(Part { layer: Layer::Back, draw: chirp_marks(b.head()), alpha: clamp01(b.chirp) * shown });
        }
    }
    out.parts.extend(front);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;
    use crate::theme::Mascot;
    use crate::ui::cat_style::CatLook;
    use crate::ui::geometry::ROBOT_COMPACT;
    use crate::ui::play::shape::{Deform, outline};
    use crate::ui::play::state::Beat;
    use crate::ui::play::stories::{CONTRACT_SIZES, wind_down_frames};
    use crate::ui::play::testing::shape;

    crate::ui::play::stories::story_contract_tests!(Story::TerbangKeliling, shot);

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

    fn flight_of(st: &Stage) -> Flight {
        Flight::new(st.shape, st.room().robot.unwrap())
    }

    fn dist(a: Pt, b: Pt) -> f32 {
        (a.0 - b.0).hypot(a.1 - b.1)
    }

    /// `a - b` in degrees, folded into -180..180.
    fn turned(a: f32, b: f32) -> f32 {
        (a - b + 540.0).rem_euclid(360.0) - 180.0
    }

    /// Whether `d` paints a head anywhere in it.
    fn has_head(d: &Draw) -> bool {
        match d {
            Draw::Head { .. } => true,
            Draw::Group { items, .. } | Draw::Clipped { items, .. } => items.iter().any(has_head),
            _ => false,
        }
    }

    /// The bird's head as the shot paints it: its pivot (window dip), its
    /// turn, its face and its alpha.
    fn bird_head(shot: &Shot) -> Option<(Pt, f32, MascotShot, f32)> {
        bird_head_in(shot).map(|(_, p, r, f, a)| (p, r, f, a))
    }

    /// `bird_head` with the layer it is in.
    fn bird_head_in(shot: &Shot) -> Option<(Layer, Pt, f32, MascotShot, f32)> {
        shot.parts.iter().find_map(|p| match &p.draw {
            Draw::Group { at, rot_deg, items, .. } => match items.as_slice() {
                [Draw::Group { at: head_at, items: inner, .. }] => match inner.as_slice() {
                    [Draw::Head { shot, .. }] => Some((p.layer, add(*at, turn(*rot_deg, *head_at)), *rot_deg, *shot, p.alpha)),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
    }

    /// The falk's own ink round its head pivot, as a quad in window dip, for
    /// the bird's head at `pivot` turned `rot` with `face`: the egg and the
    /// crest as its pose has them (`falk_outline`), every accessory's box
    /// (each with the glance's slide where it takes it) and 3 grid units for
    /// strokes and pixels. Tighter than `HEAD_INK`, which holds every
    /// mascot's head with the startled crest.
    fn falk_ink(pivot: Pt, rot: f32, face: &MascotShot) -> [Pt; 4] {
        use crate::theme::CatAccessory;
        use crate::ui::cat_style::accessory_corners;
        use crate::ui::mascot::falk_outline;
        use crate::ui::robot::{Ears, Pose};
        let mut pts = falk_outline(face.expr, &Pose { crest: Some(face.crest), ..Pose::default() });
        for &acc in CatAccessory::ALL {
            for ((x, y), slides) in accessory_corners(Mascot::Falk, acc, Ears::default()) {
                let (gx, gy) = if slides { face.glance } else { (0.0, 0.0) };
                pts.push((x + gx, y + gy));
            }
        }
        let fold = |f: fn(f32, f32) -> f32, i: usize, from: f32| pts.iter().map(|p| if i == 0 { p.0 } else { p.1 }).fold(from, f);
        let (x0, y0, x1, y1) = (fold(f32::min, 0, f32::MAX) - 3.0, fold(f32::min, 1, f32::MAX) - 3.0, fold(f32::max, 0, f32::MIN) + 3.0, fold(f32::max, 1, f32::MIN) + 3.0);
        let unit = ROBOT_COMPACT / GRID;
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| {
            add(pivot, turn(rot, ((x - HEAD_PIVOT.0) * face.sx * unit, (y - HEAD_PIVOT.1) * face.sy * unit)))
        })
    }

    #[test]
    fn the_head_changes_layer_only_clear_of_the_island() {
        // One head, in front of the island near the slot and behind it
        // round the right end; it changes layer only well under the lip,
        // where neither layer would cut any of it.
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    let bottom = shape(style, w, h).y + h;
                    let (mut last, mut switches) = (None, 0);
                    let mut t = OUT + HAND;
                    while t < HOME - HAND {
                        let shot = shot(&stage(style, w, h, playing(t), clock));
                        assert_eq!(shot.parts.iter().filter(|p| has_head(&p.draw)).count(), 1, "{case} t={t}: one head");
                        let (layer, pivot, rot, face, _) = bird_head_in(&shot).unwrap_or_else(|| panic!("{case} t={t}: no head"));
                        assert!(matches!(layer, Layer::Front | Layer::Back), "{case} t={t}: {layer:?}");
                        if let Some((was, quad)) = last {
                            if was != layer {
                                switches += 1;
                                // The island lies above its bottom edge: a head
                                // wholly under it touches none of it.
                                for q in [quad, falk_ink(pivot, rot, &face)] {
                                    let top = q.iter().map(|p: &Pt| p.1).fold(f32::MAX, f32::min);
                                    assert!(top > bottom, "{case} t={t}: {was:?} to {layer:?} with the head's ink {} under the lip", top - bottom);
                                }
                            }
                        }
                        last = Some((layer, falk_ink(pivot, rot, &face)));
                        t += 0.002;
                    }
                    assert!(switches >= 2, "{case}: {switches} switches");
                    // In front while it leaves and comes home, behind round the end.
                    assert_eq!(bird_head_in(&at(style, w, h, 1.3)).unwrap().0, Layer::Front, "{case}");
                    assert_eq!(bird_head_in(&at(style, w, h, 6.6)).unwrap().0, Layer::Front, "{case}");
                }
            }
        }
    }

    #[test]
    fn a_wind_down_shows_one_head_but_at_a_hand_over() {
        // In full flight the slot head and the bird never add up past one
        // head. Cut inside a hand-over window they do (both whole at the
        // cut): accepted, as the two are the same head in the same place
        // until the bird has faded, and only then does the slot head go home.
        let alphas = |shot: &Shot| {
            let slot = match shot.slot {
                Slot::Story(m) => m.alpha,
                _ => 0.0,
            };
            (slot, bird_head(shot).map_or(0.0, |b| b.3))
        };
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                for clock in [true, false] {
                    for cut in [2.9, 4.6, 6.6] {
                        for (ms, beat, shot) in wind_down_frames(cut, 0.0, |beat| shot(&stage(style, w, h, beat, clock))) {
                            let (s, b) = alphas(&shot);
                            assert!(s + b <= 1.0 + 1e-4, "{style:?} {w}x{h} clock={clock} cut {cut} +{ms:.1} ms (alpha {}): {s} + {b}", beat.alpha);
                        }
                    }
                    for cut in [1.2, 7.05] {
                        let case = format!("{style:?} {w}x{h} clock={clock} cut {cut}");
                        let frames = wind_down_frames(cut, 0.0, |beat| shot(&stage(style, w, h, beat, clock)));
                        let (s, b) = alphas(&frames[0].2);
                        assert!(s + b > 1.0, "{case}: the documented exception, {s} + {b}");
                        for (ms, beat, shot) in &frames {
                            if let Some((q, rb, f, a)) = bird_head(shot).filter(|b| b.3 > 0.0) {
                                let st = stage(style, w, h, *beat, clock);
                                let (p, k, r, _) = slot_head(&st, shot).unwrap();
                                let unit = ROBOT_COMPACT / GRID;
                                assert!(dist(p, q) < 1e-3 && (k.1 - unit * f.sy).abs() < 1e-4 && turned(r, rb).abs() < 1e-2,
                                    "{case} +{ms:.1} ms: the slot head at {p:?} while the bird's ({a}) is at {q:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_wind_down_in_the_slot_eases_the_head_home_and_fades_the_chirp() {
        use crate::ui::play::stories::fades_by_frames;
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                for clock in [true, false] {
                    for cut in [0.55, 7.5] {
                        let case = format!("{style:?} {w}x{h} clock={clock} cut {cut}");
                        let frames = wind_down_frames(cut, 0.0, |beat| shot(&stage(style, w, h, beat, clock)));
                        assert!(frames[0].2.parts.iter().any(|p| matches!(p.draw, Draw::Stroke { .. })), "{case}: chirping at the cut");
                        if let Err(why) = fades_by_frames(&frames) {
                            panic!("{case}: {why}");
                        }
                        for (ms, _, shot) in &frames {
                            assert!(matches!(shot.slot, Slot::App) || matches!(shot.slot, Slot::Story(m) if m.alpha == 1.0), "{case} +{ms:.1} ms: {:?}", shot.slot);
                            assert!(bird_head(shot).is_none(), "{case} +{ms:.1} ms");
                        }
                        let (_, _, end) = frames.last().unwrap();
                        assert_eq!(*end, Shot::rest(style, shape(style, w, h)), "{case}");
                    }
                }
            }
        }
    }

    /// The slot head as the renderer places it: its pivot, its dip per grid
    /// unit (x, y), its turn and its alpha. `None` with the slot empty.
    fn slot_head(st: &Stage, shot: &Shot) -> Option<(Pt, Pt, f32, f32)> {
        let r = st.room().robot.unwrap();
        let k = r.w / GRID;
        let pivot = (r.x + HEAD_PIVOT.0 * k, r.y + HEAD_PIVOT.1 * k);
        match shot.slot {
            Slot::Story(m) => Some(((pivot.0 + m.dx, pivot.1 + m.dy), (k * m.sx, k * m.sy), m.rot_deg, m.alpha)),
            Slot::App => Some((pivot, (k, k), 0.0, 1.0)),
            Slot::Empty => None,
        }
    }

    /// Whether `p` lies inside the polygon `poly`.
    fn inside(poly: &[Pt], p: Pt) -> bool {
        let mut inside = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[j]);
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) * (b.0 - a.0) / (b.1 - a.1) {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    #[test]
    fn the_bird_is_hidden_behind_the_right_end() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    let st = stage(style, w, h, playing(0.0), clock);
                    let s = st.shape;
                    let fl = flight_of(&st);
                    let island = outline(style, s, &Deform::default());
                    // The turn's innermost point is inside the end's arc, not
                    // in the gap above a pill.
                    assert!(inside(&island, (s.x + s.w - 4.0, s.y + s.h / 2.0 + 5.0)), "{case}");
                    assert!(fl.pts.iter().any(|p| p.0 > s.x + s.w / 2.0 && inside(&island, *p)), "{case}: never behind the right end");
                    // While the body's centre is there, every piece of the bird
                    // is behind the island, clipped to outside it.
                    let mut hidden = 0;
                    let mut t = 3.0;
                    while t < 6.5 {
                        let b = fl.at(t);
                        if b.0 > s.x + s.w / 2.0 && inside(&island, b) {
                            hidden += 1;
                            let shot = shot(&stage(style, w, h, playing(t), clock));
                            assert_eq!(shot.slot, Slot::Empty, "{case} t={t}");
                            assert!(bird_head(&shot).is_some(), "{case} t={t}: no bird");
                            for p in &shot.parts {
                                assert!(p.layer == Layer::Back || !has_head(&p.draw), "{case} t={t}: a head in {:?}", p.layer);
                            }
                        }
                        t += 0.01;
                    }
                    assert!(hidden > 3, "{case}: behind the end for {hidden} steps");
                    // Where it is deepest in, the head's pivot is inside too.
                    let deep = (s.x + s.w - 4.0, s.y + s.h / 2.0 + 5.0);
                    let (mut best, mut t) = ((f32::MAX, 0.0), 3.0);
                    while t < 6.5 {
                        best = if dist(fl.at(t), deep) < best.0 { (dist(fl.at(t), deep), t) } else { best };
                        t += 0.005;
                    }
                    let head = bird_head(&shot(&stage(style, w, h, playing(best.1), clock))).unwrap().0;
                    assert!(inside(&island, head), "{case} t={}: the head at {head:?}", best.1);
                }
            }
        }
    }

    #[test]
    fn the_path_holds_at_160_and_300_wide() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    let st = stage(style, w, h, playing(0.0), clock);
                    let s = st.shape;
                    let slot = st.room().robot.unwrap();
                    let k = slot.w / GRID;
                    let pivot = (slot.x + HEAD_PIVOT.0 * k, slot.y + HEAD_PIVOT.1 * k);
                    let fl = flight_of(&st);
                    let (first, last) = (fl.pts[0], *fl.pts.last().unwrap());
                    let (bottom, right, mid_x) = (s.y + s.h, s.x + s.w, s.x + s.w / 2.0);
                    // Out of the slot under the crouched head, back in under the
                    // resting one.
                    assert!(dist(first, (pivot.0, pivot.1 + CROUCH_DY + NECK)) < 1e-3, "{case}: {first:?}");
                    assert!(dist(last, (pivot.0, pivot.1 + NECK)) < 1e-3, "{case}: {last:?}");
                    // The loop is under the island, below its lip.
                    assert!(fl.pts[30..=102].iter().all(|p| p.1 > bottom + 10.0), "{case}: the loop");
                    // It passes under the middle (the glide and the way back, and
                    // on a narrow island the loop's top, 23 under the lip), low.
                    let under: Vec<_> = fl.pts.iter().filter(|p| (p.0 - mid_x).abs() < 6.0).collect();
                    assert!(under.len() >= 2 && under.iter().all(|p| p.1 > bottom + 20.0), "{case}: {under:?}");
                    // It swings out past the right end, never further than the
                    // mockup's 15 dip and a little for the curve.
                    let far = fl.pts.iter().map(|p| p.0).fold(f32::MIN, f32::max);
                    assert!(far > right + 9.0 && far < right + 17.0, "{case}: {far}");
                    // Never above the island's top, and longer on a longer island.
                    assert!(fl.pts.iter().all(|p| p.1 > s.y), "{case}");
                    if w > 160.0 {
                        let narrow = flight_of(&stage(style, 160.0, 28.0, playing(0.0), clock));
                        assert!(fl.tot > narrow.tot + (w - 160.0), "{case}: {} against {}", fl.tot, narrow.tot);
                    }
                }
            }
        }
    }

    #[test]
    fn the_head_leaves_its_slot_and_comes_home_without_a_jump() {
        let unit = ROBOT_COMPACT / GRID;
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                for clock in [true, false] {
                    let case = format!("{style:?} {w}x{h} clock={clock}");
                    for (from, to) in [(0.9, 1.5), (6.7, 7.4)] {
                        let mut last: Option<(Pt, Pt)> = None;
                        let mut t: f32 = from;
                        while t <= to {
                            let st = stage(style, w, h, playing(t), clock);
                            let shot = shot(&st);
                            let (slot, bird) = (slot_head(&st, &shot), bird_head(&shot));
                            // Shown together, the two heads are one.
                            if let (Some((p, k, r, a)), Some((q, rb, f, _))) = (slot, bird) {
                                assert!(dist(p, q) < 1e-3 && (k.0 - unit * f.sx).abs() < 1e-4 && (k.1 - unit * f.sy).abs() < 1e-4,
                                    "{case} t={t}: the slot head at {p:?} {k:?}, the bird's at {q:?}");
                                assert!(turned(r, rb).abs() < 1e-2 && a == 1.0, "{case} t={t}: {r} {rb} {a}");
                                assert_eq!(MascotShot { dx: 0.0, dy: 0.0, rot_deg: 0.0, ..shot.head().unwrap() }, MascotShot { rot_deg: 0.0, ..f }, "{case} t={t}");
                            }
                            // The head that shows moves smoothly.
                            let now = bird.filter(|b| b.3 >= 1.0).map(|b| (b.0, (unit * b.2.sx, unit * b.2.sy))).or(slot.map(|s| (s.0, s.1)));
                            let now = now.unwrap_or_else(|| panic!("{case} t={t}: no head shows"));
                            if let Some((p, k)) = last {
                                assert!(dist(now.0, p) < 1.0 && (now.1.0 - k.0).abs() < 0.01 && (now.1.1 - k.1).abs() < 0.01,
                                    "{case} t={t}: from {p:?} {k:?} to {now:?}");
                            }
                            last = Some(now);
                            t += 0.002;
                        }
                    }
                    // The slot is empty while the bird has the head, and the
                    // head shows over the island near its slot, unclipped.
                    assert_eq!(at(style, w, h, 4.0).slot, Slot::Empty, "{case}");
                    assert_eq!(bird_head_in(&at(style, w, h, 1.3)).unwrap().0, Layer::Front, "{case}");
                }
            }
        }
    }

    #[test]
    fn a_wind_down_mid_flight_fades_the_bird_where_it_is_and_the_head_into_its_slot() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                for clock in [true, false] {
                    for cut in [1.2, 2.9, 4.6, 6.6, 7.05] {
                        let case = format!("{style:?} {w}x{h} clock={clock} cut at {cut}");
                        let frames = wind_down_frames(cut, 0.0, |beat| shot(&stage(style, w, h, beat, clock)));
                        let first = bird_head(&frames[0].2).unwrap_or_else(|| panic!("{case}: no bird at the cut"));
                        let (mut bird_alpha, mut slot_alpha) = (first.3, 0.0);
                        for (ms, beat, shot) in &frames {
                            let case = format!("{case} +{ms:.1} ms (alpha {})", beat.alpha);
                            // The bird stays where the cut found it and only fades.
                            if let Some((p, _, _, a)) = bird_head(shot) {
                                assert!(dist(p, first.0) < 1e-3, "{case}: moved from {:?} to {p:?}", first.0);
                                assert!(a <= bird_alpha + 1e-6, "{case}: {bird_alpha} to {a}");
                                bird_alpha = a;
                            } else {
                                bird_alpha = 0.0;
                            }
                            // The slot head comes back only by fading in.
                            if let Slot::Story(m) = shot.slot {
                                assert!(m.alpha + 1e-6 >= slot_alpha, "{case}: {slot_alpha} to {}", m.alpha);
                                slot_alpha = m.alpha;
                            }
                        }
                        let (_, beat, end) = frames.last().unwrap();
                        assert_eq!(beat.alpha, 0.0, "{case}");
                        assert_eq!(*end, Shot::rest(style, shape(style, w, h)), "{case}");
                    }
                }
            }
        }
    }

    #[test]
    fn it_rolls_once_round_the_loop_and_flaps_with_the_tips_trailing() {
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                let case = format!("{style:?} {w}x{h}");
                let st = stage(style, w, h, playing(0.0), true);
                let fl = flight_of(&st);
                // The first moment it is `share` of the way round the loop.
                let when = |share: f32| {
                    let u = lerp(fl.roll.0, fl.roll.1, share);
                    let mut t = OUT;
                    while travel(t) < u {
                        t += 0.001;
                    }
                    t
                };
                let rot = |t: f32| flying(t, &fl, st.shape).rot;
                // Upright into the loop, upside down halfway round, round once
                // out of it (the lean adds up to 15 degrees).
                assert!(rot(when(0.0) - 0.01).abs() <= 15.0, "{case}: {}", rot(when(0.0) - 0.01));
                assert!(turned(rot(when(0.5)), 180.0).abs() <= 16.0, "{case}: {}", rot(when(0.5)));
                assert!((rot(when(1.0) + 0.01) + 360.0).abs() <= 15.0, "{case}: {}", rot(when(1.0) + 0.01));
                // Happy while it rolls.
                assert_eq!(flying(when(0.5), &fl, st.shape).pose.face.expr, Expr::Happy, "{case}");
            }
        }
        // The flapping: the arm swings up and down, the hand bends and the
        // fan closes on the up-stroke; the primaries take the pose 60 ms
        // before.
        let full = Drive { power: 1.0, glide: 0.0, flare: 0.0, open: 1.0, fold: 0.0 };
        let beat: Vec<WingPose> = (0..100).map(|i| wing_pose(1.6 + i as f32 / 100.0 / 4.4, full)).collect();
        let arms = beat.iter().map(|p| p.arm);
        let (lo, hi) = (arms.clone().fold(f32::MAX, f32::min), arms.fold(f32::MIN, f32::max));
        assert!(hi - lo > 60.0, "{lo} to {hi}");
        let top = beat.iter().min_by(|a, b| a.fan.total_cmp(&b.fan)).unwrap();
        assert!((top.fan - 0.62).abs() < 0.01 && (top.hand - 24.0).abs() < 0.2, "{top:?}");
        assert!(beat.iter().any(|p| p.fan == 1.0 && p.hand == 10.0));
        let st = stage(Style::Notch, 190.0, 34.0, playing(0.0), true);
        let fl = flight_of(&st);
        for t in [1.7, 2.4, 3.6, 5.0, 6.5] {
            let b = flying(t, &fl, st.shape);
            assert_eq!(b.pose.wl, wing_pose(t, b.drive), "t={t}");
            assert_eq!(b.pose.wl_before, wing_pose(t - 0.06, b.drive), "t={t}");
            assert_eq!((b.pose.wr, b.pose.wr_before), (b.pose.wl, b.pose.wl_before), "t={t}");
        }
        // Folded in the slot at the jump and the landing, the wings opening
        // after it and the flare braking forward.
        let folded = |p: WingPose| {
            let f = WingPose::FOLD;
            [p.arm - f.arm, p.hand - f.hand, p.ext - f.ext, p.fan - f.fan, p.sweep - f.sweep].iter().all(|d| d.abs() < 1e-4)
        };
        assert!(folded(flying(OUT, &fl, st.shape).pose.wl) && folded(flying(HOME, &fl, st.shape).pose.wl));
        assert!(flying(6.7, &fl, st.shape).drive.flare > 0.9);
    }

    #[test]
    fn the_faces_are_the_mockups_in_the_falks_own_expressions() {
        let slot = |t: f32| match at(Style::Notch, 190.0, 34.0, t).slot {
            Slot::Story(m) => m,
            other => panic!("t={t}: {other:?}"),
        };
        let bird = |t: f32| bird_head(&at(Style::Notch, 190.0, 34.0, t)).unwrap().2;
        // In the slot: a look left and right, a happy chirp, the crouch with
        // the crest laid back.
        assert!(slot(0.35).glance.0 < -2.0 && slot(0.7).glance.0 > 1.0);
        assert!(slot(0.6).expr == Expr::Happy && slot(0.6).beak_open);
        assert!(slot(0.15).blink && !slot(0.3).blink);
        assert!(slot(1.1).crest.rot_deg < 0.0 && slot(1.1).sy < 0.9 && slot(1.1).dy > 2.0);
        // Under the clock: glancing up, chirping happily.
        let up = bird(3.6);
        assert!(up.expr == Expr::Happy && up.beak_open && up.glance.1 < 0.0, "{up:?}");
        assert!(!bird(4.2).beak_open && bird(4.2).glance.1 == 0.0);
        // Home: a happy chirp, a blink, then idle.
        assert!(slot(7.55).expr == Expr::Happy && slot(7.55).beak_open);
        assert!(slot(LAST_BLINK).blink);
        assert_eq!(slot(7.8).expr, Expr::Idle);
        // The chirps show their marks, in front at the slot, behind in flight.
        let marks = |t: f32, layer: Layer| at(Style::Notch, 190.0, 34.0, t).parts.iter().filter(|p| p.layer == layer && matches!(p.draw, Draw::Stroke { .. })).count();
        assert_eq!((marks(0.6, Layer::Front), marks(3.6, Layer::Back), marks(7.55, Layer::Front)), (1, 1, 1));
        assert_eq!((marks(2.0, Layer::Front), marks(2.0, Layer::Back)), (0, 0));
    }

    #[test]
    fn a_feather_comes_loose_on_the_loop_and_one_on_the_flare_each_falling_and_fading() {
        /// The feathers: groups in front with no head, where and how visible.
        fn feathers(shot: &Shot) -> Vec<(Pt, f32)> {
            shot.parts
                .iter()
                .filter(|p| p.layer == Layer::Front && !has_head(&p.draw))
                .filter_map(|p| match p.draw {
                    Draw::Group { at, .. } => Some((at, p.alpha)),
                    _ => None,
                })
                .collect()
        }
        for style in [Style::Notch, Style::Pill] {
            for (w, h) in CONTRACT_SIZES {
                let case = format!("{style:?} {w}x{h}");
                let bottom = shape(style, w, h).y + h;
                assert!(feathers(&at(style, w, h, 2.75)).is_empty(), "{case}");
                let (a, b) = (feathers(&at(style, w, h, 3.2)), feathers(&at(style, w, h, 3.8)));
                assert!(a.len() == 1 && b.len() == 1 && a[0].1 == 1.0, "{case}: {a:?} {b:?}");
                assert!(b[0].0.1 > a[0].0.1 && a[0].0.1 > bottom + 10.0, "{case}: {a:?} {b:?}");
                assert!(feathers(&at(style, w, h, 5.0))[0].1 < 0.5, "{case}: it fades");
                assert!(feathers(&at(style, w, h, 5.3)).is_empty(), "{case}");
                let c = feathers(&at(style, w, h, 7.0));
                assert!(c.len() == 1 && c[0].1 == 1.0, "{case}: {c:?}");
                assert!(feathers(&at(style, w, h, 7.6))[0].1 < 0.7, "{case}: it fades before the end");
                assert!(feathers(&at(style, w, h, 7.86)).is_empty(), "{case}");
            }
        }
    }
}
