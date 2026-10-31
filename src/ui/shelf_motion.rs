//! How the shelf's cards move: a card that arrives fades and grows in with a
//! small rise, one that is taken off shrinks and fades out where it was (a
//! "ghost"), and the ones that stay slide to their new places. Pure state,
//! no drawing and no clock: the app records a change with `on_change`, the
//! renderer asks `card_at` and `ghosts` at the frame's time, and `running`
//! tells `App::animating_at` whether the `FrameClock` is still needed. When
//! the last animation ends `prune` leaves the state empty, so an idle shelf
//! costs nothing.
//!
//! Cards are told apart by `ShelfCard::key` (the normalised path), never by
//! their place in the list. Hit regions never come from here: they use the
//! final layout, and a ghost takes no click.

use std::time::{Duration, Instant};

use super::geometry::Rect;
use super::motion::{ease_in_out_cubic, ease_out_cubic};
use crate::shelf::ShelfCard;

/// A card coming in: opacity, size and rise.
pub const SHELF_ENTER_MS: u64 = 200;
/// A card going out.
pub const SHELF_EXIT_MS: u64 = 180;
/// A card changing slot.
pub const SHELF_MOVE_MS: u64 = 220;
/// Cards dropped together start this far apart ...
pub const SHELF_STAGGER_MS: u64 = 30;
/// ... for at most this many of them (the shelf holds twelve).
pub const SHELF_STAGGER_CARDS: usize = 12;

/// A card starts this big and grows to 1 around its centre.
const ENTER_SCALE: f32 = 0.92;
/// A card starts this many px low.
const ENTER_RISE: f32 = 6.0;
/// A card on its way out ends this big.
const EXIT_SCALE: f32 = 0.85;
/// Rects closer than this are the same place.
const SAME_PLACE: f32 = 0.01;

/// How a card is drawn at one moment: where, the scale around its centre,
/// its opacity, an extra shift down and how many lines its name wraps to
/// (`shelf_card_layout`'s `lines`, which belongs to the layout the rect
/// came from: a ghost keeps the old one, a slide swaps at its midpoint).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn {
    pub rect: Rect,
    pub scale: f32,
    pub alpha: f32,
    pub dy: f32,
    pub lines: usize,
}

impl Drawn {
    fn rest(rect: Rect, lines: usize) -> Self {
        Drawn {
            rect,
            scale: 1.0,
            alpha: 1.0,
            dy: 0.0,
            lines,
        }
    }

    /// Nothing to apply: drawn as it rests.
    pub fn at_rest(&self) -> bool {
        self.scale == 1.0 && self.alpha == 1.0 && self.dy == 0.0
    }
}

/// How far an animation of `ms` that began at `start` has got, 0 to 1 (0
/// before it begins).
pub fn progress(start: Instant, now: Instant, ms: u64) -> f32 {
    let elapsed = now.saturating_duration_since(start).as_secs_f32();
    (elapsed / (ms as f32 / 1000.0)).clamp(0.0, 1.0)
}

/// Whether an animation of `ms` that began at `start` is still going.
fn active(start: Instant, now: Instant, ms: u64) -> bool {
    now < start + Duration::from_millis(ms)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect {
        x: lerp(a.x, b.x, t),
        y: lerp(a.y, b.y, t),
        w: lerp(a.w, b.w, t),
        h: lerp(a.h, b.h, t),
    }
}

fn same_place(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < SAME_PLACE
        && (a.y - b.y).abs() < SAME_PLACE
        && (a.w - b.w).abs() < SAME_PLACE
        && (a.h - b.h).abs() < SAME_PLACE
}

/// Where a sliding card comes from: the rect and name lines it was drawn
/// with, and when the slide began.
#[derive(Clone, Copy, Debug)]
struct Move {
    from: Rect,
    lines: usize,
    start: Instant,
}

/// A card that is on the shelf and animating: coming in (from `enter`, which
/// may be in the future for a staggered one) and/or sliding (`mv`).
#[derive(Clone, Debug)]
struct Live {
    key: String,
    enter: Option<Instant>,
    mv: Option<Move>,
}

impl Live {
    fn running(&self, now: Instant) -> bool {
        self.enter.is_some_and(|s| active(s, now, SHELF_ENTER_MS))
            || self.mv.is_some_and(|m| active(m.start, now, SHELF_MOVE_MS))
    }
}

/// A card that was taken off, shrinking and fading where it was.
#[derive(Clone, Debug)]
pub struct Ghost {
    card: ShelfCard,
    /// How it was drawn when it went (a card taken off mid-way keeps what
    /// it had reached, so it never jumps).
    from: Drawn,
    start: Instant,
}

impl Ghost {
    fn running(&self, now: Instant) -> bool {
        active(self.start, now, SHELF_EXIT_MS)
    }

    fn drawn(&self, now: Instant) -> Drawn {
        let t = ease_in_out_cubic(progress(self.start, now, SHELF_EXIT_MS));
        Drawn {
            rect: self.from.rect,
            scale: lerp(self.from.scale, EXIT_SCALE, t),
            alpha: self.from.alpha * (1.0 - t),
            dy: self.from.dy * (1.0 - t),
            lines: self.from.lines,
        }
    }
}

/// What the shelf is animating. Empty at rest.
#[derive(Clone, Debug, Default)]
pub struct ShelfMotion {
    live: Vec<Live>,
    ghosts: Vec<Ghost>,
}

impl ShelfMotion {
    /// Nothing is animating or waiting to.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty() && self.ghosts.is_empty()
    }

    /// Forgets everything: the page is not on screen, or motion is off.
    pub fn clear(&mut self) {
        self.live.clear();
        self.ghosts.clear();
    }

    /// The shelf went from `old_cards` (laid out at `old_rects`, names on
    /// `old_lines` lines) to `new_cards` (`new_rects`), the lists in page
    /// order; the new line count reaches `card_at` when a frame is drawn. A
    /// key that is new enters (several at once, one after the other), one
    /// that is gone leaves as a ghost at the rect and lines it was drawn
    /// with, and one whose rect changed slides, starting from the rect it is
    /// drawn at now. With `enabled` false (Windows animation effects off)
    /// nothing is recorded and what was is dropped.
    #[allow(clippy::too_many_arguments)]
    pub fn on_change(
        &mut self,
        old_cards: &[ShelfCard],
        old_rects: &[Rect],
        old_lines: usize,
        new_cards: &[ShelfCard],
        new_rects: &[Rect],
        now: Instant,
        enabled: bool,
    ) {
        if !enabled {
            self.clear();
            return;
        }
        // How each old card is drawn right now, before anything changes.
        let before: Vec<Drawn> = old_cards
            .iter()
            .zip(old_rects)
            .map(|(card, rect)| self.card_at(&card.key, *rect, old_lines, now))
            .collect();
        for (card, drawn) in old_cards.iter().zip(&before) {
            if !new_cards.iter().any(|n| n.key == card.key) {
                self.ghosts.push(Ghost {
                    card: card.clone(),
                    from: *drawn,
                    start: now,
                });
            }
        }
        let mut live = Vec::new();
        let mut fresh = 0usize;
        for (card, target) in new_cards.iter().zip(new_rects) {
            match old_cards.iter().position(|o| o.key == card.key) {
                None => {
                    // A ghost of the same file would draw under its return.
                    self.ghosts.retain(|g| g.card.key != card.key);
                    let delay = fresh.min(SHELF_STAGGER_CARDS - 1) as u64 * SHELF_STAGGER_MS;
                    fresh += 1;
                    live.push(Live {
                        key: card.key.clone(),
                        enter: Some(now + Duration::from_millis(delay)),
                        mv: None,
                    });
                }
                Some(j) => {
                    // An entrance still going on carries on.
                    let enter = self
                        .live
                        .iter()
                        .find(|l| l.key == card.key)
                        .and_then(|l| l.enter)
                        .filter(|s| active(*s, now, SHELF_ENTER_MS));
                    let was = before[j];
                    let mv = (!same_place(was.rect, *target)).then_some(Move {
                        from: was.rect,
                        lines: was.lines,
                        start: now,
                    });
                    if enter.is_some() || mv.is_some() {
                        live.push(Live {
                            key: card.key.clone(),
                            enter,
                            mv,
                        });
                    }
                }
            }
        }
        self.live = live;
        self.prune(now);
    }

    /// How the card `key` is drawn at `now`, resting at `rest` with its name
    /// on `lines` lines (its place in the final layout). A slide between two
    /// line counts keeps the old one until it is halfway there, so the name
    /// never wraps in a rect still sized for the other count.
    pub fn card_at(&self, key: &str, rest: Rect, lines: usize, now: Instant) -> Drawn {
        let mut drawn = Drawn::rest(rest, lines);
        let Some(live) = self.live.iter().find(|l| l.key == key) else {
            return drawn;
        };
        if let Some(mv) = live.mv {
            let t = ease_out_cubic(progress(mv.start, now, SHELF_MOVE_MS));
            drawn.rect = lerp_rect(mv.from, rest, t);
            if t < 0.5 {
                drawn.lines = mv.lines;
            }
        }
        if let Some(start) = live.enter {
            let t = ease_out_cubic(progress(start, now, SHELF_ENTER_MS));
            drawn.alpha = t;
            drawn.scale = lerp(ENTER_SCALE, 1.0, t);
            drawn.dy = ENTER_RISE * (1.0 - t);
        }
        drawn
    }

    /// The cards on their way out, with how they are drawn at `now`. Draw
    /// them first, under the live cards; they take no clicks.
    pub fn ghosts(&self, now: Instant) -> impl Iterator<Item = (&ShelfCard, Drawn)> {
        self.ghosts
            .iter()
            .filter(move |g| g.running(now))
            .map(move |g| (&g.card, g.drawn(now)))
    }

    /// How many cards are still on their way out at `now` (the header's
    /// count and the empty hint wait for them).
    pub fn ghost_count(&self, now: Instant) -> usize {
        self.ghosts.iter().filter(|g| g.running(now)).count()
    }

    /// Something still moves at `now` (or waits to start): the frame clock
    /// must run. False once every duration has passed.
    pub fn running(&self, now: Instant) -> bool {
        self.live.iter().any(|l| l.running(now)) || self.ghosts.iter().any(|g| g.running(now))
    }

    /// Drops what has finished, so a shelf at rest holds no state at all.
    pub fn prune(&mut self, now: Instant) {
        self.live.retain(|l| l.running(now));
        self.ghosts.retain(|g| g.running(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shelf::ShelfKind;

    fn card(key: &str) -> ShelfCard {
        ShelfCard {
            key: key.into(),
            name: key.into(),
            kind: ShelfKind::Document,
            stale: false,
        }
    }

    fn cards(keys: &[&str]) -> Vec<ShelfCard> {
        keys.iter().map(|k| card(k)).collect()
    }

    /// A row of 60 x 40 cards, 10 apart.
    fn row(n: usize) -> Vec<Rect> {
        (0..n)
            .map(|i| Rect {
                x: i as f32 * 70.0,
                y: 0.0,
                w: 60.0,
                h: 40.0,
            })
            .collect()
    }

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// `a` becomes `b` at `t0`.
    fn change(a: &[&str], b: &[&str], t0: Instant) -> ShelfMotion {
        let mut m = ShelfMotion::default();
        m.on_change(
            &cards(a),
            &row(a.len()),
            2,
            &cards(b),
            &row(b.len()),
            t0,
            true,
        );
        m
    }

    #[test]
    fn progress_and_easing_hit_their_endpoints() {
        let t0 = Instant::now();
        assert_eq!(progress(t0, t0, 200), 0.0);
        assert_eq!(progress(t0, ms(t0, 100), 200), 0.5);
        assert_eq!(progress(t0, ms(t0, 200), 200), 1.0);
        assert_eq!(progress(t0, ms(t0, 900), 200), 1.0);
        // Before it starts: not begun.
        assert_eq!(progress(ms(t0, 50), t0, 200), 0.0);
        assert_eq!(ease_in_out_cubic(0.0), 0.0);
        assert_eq!(ease_in_out_cubic(1.0), 1.0);
        assert!(close(ease_in_out_cubic(0.5), 0.5));
        assert!(ease_in_out_cubic(0.25) < 0.25 && ease_in_out_cubic(0.75) > 0.75);
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
    }

    #[test]
    fn an_entering_card_fades_grows_and_rises_then_rests() {
        let t0 = Instant::now();
        let m = change(&["a"], &["a", "b"], t0);
        let rest = row(2)[1];
        let start = m.card_at("b", rest, 2, t0);
        assert_eq!(start.rect, rest);
        assert!(close(start.alpha, 0.0));
        assert!(close(start.scale, 0.92));
        assert!(close(start.dy, 6.0));
        let mid = m.card_at("b", rest, 2, ms(t0, 100));
        assert!(mid.alpha > 0.5 && mid.alpha < 1.0, "ease-out is ahead");
        assert!(mid.scale > 0.96 && mid.scale < 1.0);
        assert!(mid.dy > 0.0 && mid.dy < 3.0);
        let end = m.card_at("b", rest, 2, ms(t0, SHELF_ENTER_MS));
        assert!(end.at_rest());
        // The card that was already there does not animate.
        assert!(m.card_at("a", row(2)[0], 2, t0).at_rest());
    }

    #[test]
    fn several_new_cards_start_thirty_ms_apart_and_the_stagger_is_capped() {
        let t0 = Instant::now();
        let m = change(&["a"], &["a", "b", "c", "d"], t0);
        let rects = row(4);
        // Halfway through b's entrance, c has not begun and d neither.
        let at = ms(t0, 20);
        assert!(m.card_at("b", rects[1], 2, at).alpha > 0.0);
        assert_eq!(m.card_at("c", rects[2], 2, at).alpha, 0.0);
        assert_eq!(m.card_at("d", rects[3], 2, at).alpha, 0.0);
        let at = ms(t0, 45);
        assert!(m.card_at("c", rects[2], 2, at).alpha > 0.0);
        assert_eq!(m.card_at("d", rects[3], 2, at).alpha, 0.0);
        // The last one ends 60 ms after the first.
        assert!(m.running(ms(t0, SHELF_ENTER_MS + 59)));
        assert!(!m.running(ms(t0, SHELF_ENTER_MS + 60)));
        // Twelve cards at once: the stagger stops growing at the twelfth.
        let names: Vec<String> = (0..12).map(|i| format!("k{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let m = change(&[], &refs, t0);
        let last = SHELF_STAGGER_MS * (SHELF_STAGGER_CARDS as u64 - 1);
        assert!(m.running(ms(t0, last + SHELF_ENTER_MS - 1)));
        assert!(!m.running(ms(t0, last + SHELF_ENTER_MS)));
    }

    #[test]
    fn a_removed_card_is_a_ghost_until_exactly_the_exit_time() {
        let t0 = Instant::now();
        let m = change(&["a", "b", "c"], &["a", "c"], t0);
        let old = row(3);
        let at = |n: u64| {
            m.ghosts(ms(t0, n))
                .map(|(c, d)| (c.key.clone(), d))
                .collect::<Vec<_>>()
        };
        let first = at(0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0, "b");
        assert_eq!(first[0].1.rect, old[1], "it stays where it was");
        assert!(close(first[0].1.scale, 1.0) && close(first[0].1.alpha, 1.0));
        let mid = at(SHELF_EXIT_MS / 2);
        assert!(close(mid[0].1.alpha, 0.5) && close(mid[0].1.scale, 0.925));
        assert_eq!(at(SHELF_EXIT_MS - 1).len(), 1);
        assert!(at(SHELF_EXIT_MS).is_empty(), "gone exactly at the end");
        assert_eq!(m.ghost_count(ms(t0, SHELF_EXIT_MS - 1)), 1);
        assert_eq!(m.ghost_count(ms(t0, SHELF_EXIT_MS)), 0);
        let end = m.ghosts(ms(t0, SHELF_EXIT_MS - 1)).next().unwrap().1;
        assert!(end.alpha < 0.01 && end.scale < 0.86);
    }

    #[test]
    fn nothing_keeps_the_clock_once_every_duration_has_passed() {
        let t0 = Instant::now();
        // A removal (exit and move), a drop (enter) and a clear.
        for (a, b) in [
            (vec!["a", "b", "c"], vec!["a", "c"]),
            (vec!["a"], vec!["a", "b", "c"]),
            (vec!["a", "b"], vec![]),
        ] {
            let mut m = change(&a, &b, t0);
            assert!(m.running(t0));
            let longest = SHELF_MOVE_MS.max(SHELF_ENTER_MS + 2 * SHELF_STAGGER_MS);
            assert!(!m.running(ms(t0, longest)));
            m.prune(ms(t0, longest));
            assert!(m.is_empty(), "no state left at rest");
        }
        assert!(!ShelfMotion::default().running(t0));
    }

    #[test]
    fn the_cards_that_stay_slide_to_their_new_slot() {
        let t0 = Instant::now();
        let m = change(&["a", "b", "c"], &["a", "c"], t0);
        let (old, new) = (row(3), row(2));
        // c goes from slot 2 to slot 1.
        let start = m.card_at("c", new[1], 2, t0);
        assert_eq!(start.rect, old[2]);
        let mid = m.card_at("c", new[1], 2, ms(t0, 110));
        assert!(mid.rect.x < old[2].x && mid.rect.x > new[1].x);
        assert!(mid.at_rest(), "a move alone does not fade or scale");
        let end = m.card_at("c", new[1], 2, ms(t0, SHELF_MOVE_MS));
        assert_eq!(end.rect, new[1]);
        // The card that did not change slot is not moved.
        assert_eq!(m.card_at("a", new[0], 2, ms(t0, 50)).rect, new[0]);
        // Width and height are interpolated too.
        let mut m = ShelfMotion::default();
        let big = Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 80.0,
        };
        let small = Rect {
            x: 50.0,
            y: 20.0,
            w: 60.0,
            h: 40.0,
        };
        m.on_change(
            &cards(&["a"]),
            &[big],
            2,
            &cards(&["a"]),
            &[small],
            t0,
            true,
        );
        let t = ease_out_cubic(0.5);
        let mid = m.card_at("a", small, 2, ms(t0, SHELF_MOVE_MS / 2)).rect;
        assert!(close(mid.w, 100.0 + (60.0 - 100.0) * t));
        assert!(close(mid.h, 80.0 + (40.0 - 80.0) * t));
        assert!(close(mid.x, 50.0 * t) && close(mid.y, 20.0 * t));
    }

    #[test]
    fn a_move_cut_short_starts_from_the_rect_drawn_now() {
        let t0 = Instant::now();
        let mut m = change(&["a", "b", "c", "d"], &["a", "c", "d"], t0);
        let rects3 = row(3);
        let t1 = ms(t0, 80);
        let drawn = m.card_at("d", rects3[2], 2, t1).rect;
        // d began at slot 3 (x 210) heading for slot 2 (x 140): mid-way now.
        assert!(drawn.x < 210.0 && drawn.x > 140.0);
        // Another removal retargets it: it carries on from where it is drawn.
        m.on_change(
            &cards(&["a", "c", "d"]),
            &rects3,
            2,
            &cards(&["c", "d"]),
            &row(2),
            t1,
            true,
        );
        let again = m.card_at("d", row(2)[1], 2, t1).rect;
        assert!(
            close(again.x, drawn.x) && close(again.y, drawn.y),
            "no jump"
        );
        let end = m.card_at("d", row(2)[1], 2, ms(t1, SHELF_MOVE_MS));
        assert_eq!(end.rect, row(2)[1]);
    }

    #[test]
    fn a_card_removed_while_entering_leaves_from_the_state_it_had() {
        let t0 = Instant::now();
        let mut m = change(&["a"], &["a", "b"], t0);
        let t1 = ms(t0, 60);
        let had = m.card_at("b", row(2)[1], 2, t1);
        m.on_change(
            &cards(&["a", "b"]),
            &row(2),
            2,
            &cards(&["a"]),
            &row(1),
            t1,
            true,
        );
        let (_, ghost) = m.ghosts(t1).next().unwrap();
        assert!(close(ghost.alpha, had.alpha) && close(ghost.scale, had.scale));
        // The entrance itself is over: the card is a ghost now.
        assert!(m.card_at("b", row(2)[1], 2, t1).at_rest());
    }

    #[test]
    fn a_card_that_returns_during_its_exit_replaces_its_ghost() {
        let t0 = Instant::now();
        let mut m = change(&["a", "b"], &["a"], t0);
        let t1 = ms(t0, 50);
        m.on_change(
            &cards(&["a"]),
            &row(1),
            2,
            &cards(&["a", "b"]),
            &row(2),
            t1,
            true,
        );
        assert_eq!(m.ghost_count(t1), 0);
        assert!(m.card_at("b", row(2)[1], 2, t1).alpha < 0.01);
    }

    #[test]
    fn clearing_the_shelf_exits_every_card_at_once() {
        let t0 = Instant::now();
        let m = change(&["a", "b", "c", "d", "e"], &[], t0);
        let old = row(5);
        let ghosts: Vec<(String, Rect)> = m
            .ghosts(ms(t0, 10))
            .map(|(c, d)| (c.key.clone(), d.rect))
            .collect();
        assert_eq!(ghosts.len(), 5);
        for (i, (key, rect)) in ghosts.iter().enumerate() {
            assert_eq!(key, ["a", "b", "c", "d", "e"][i]);
            assert_eq!(*rect, old[i]);
        }
        // All end together.
        assert_eq!(m.ghost_count(ms(t0, SHELF_EXIT_MS - 1)), 5);
        assert_eq!(m.ghost_count(ms(t0, SHELF_EXIT_MS)), 0);
    }

    #[test]
    fn disabled_motion_records_nothing_and_drops_what_was() {
        let t0 = Instant::now();
        let mut m = ShelfMotion::default();
        m.on_change(
            &cards(&["a", "b"]),
            &row(2),
            2,
            &cards(&["a"]),
            &row(1),
            t0,
            false,
        );
        assert!(m.is_empty() && !m.running(t0));
        m.on_change(
            &cards(&["a"]),
            &row(1),
            2,
            &cards(&["a", "b"]),
            &row(2),
            t0,
            true,
        );
        assert!(m.running(t0));
        m.on_change(
            &cards(&["a", "b"]),
            &row(2),
            2,
            &cards(&["a"]),
            &row(1),
            t0,
            false,
        );
        assert!(m.is_empty(), "reduced motion clears");
    }

    #[test]
    fn a_ghost_keeps_the_name_lines_of_the_layout_it_left() {
        // Three rows (one line) become two (two lines): the ghost is laid
        // out in its old, short rect, so it keeps one line to the end.
        let t0 = Instant::now();
        let mut m = ShelfMotion::default();
        m.on_change(
            &cards(&["a", "b", "c"]),
            &row(3),
            1,
            &cards(&["a", "c"]),
            &row(2),
            t0,
            true,
        );
        for n in [0, SHELF_EXIT_MS / 2, SHELF_EXIT_MS - 1] {
            let (_, ghost) = m.ghosts(ms(t0, n)).next().unwrap();
            assert_eq!(ghost.lines, 1);
        }
        // Clearing every card: each ghost keeps the old count too.
        let mut m = ShelfMotion::default();
        m.on_change(&cards(&["a", "b"]), &row(2), 1, &[], &[], t0, true);
        assert!(m.ghosts(t0).all(|(_, d)| d.lines == 1));
    }

    #[test]
    fn a_slide_between_line_counts_swaps_at_its_midpoint() {
        let t0 = Instant::now();
        let mut m = ShelfMotion::default();
        m.on_change(
            &cards(&["a", "b", "c"]),
            &row(3),
            1,
            &cards(&["a", "c"]),
            &row(2),
            t0,
            true,
        );
        let rest = row(2)[1];
        // The first frame still has the old count, in the old rect.
        let first = m.card_at("c", rest, 2, t0);
        assert_eq!((first.rect, first.lines), (row(3)[2], 1));
        // Ease-out passes halfway early: find that moment and check both
        // sides of it.
        let half = (0..=SHELF_MOVE_MS)
            .find(|&n| ease_out_cubic(progress(t0, ms(t0, n), SHELF_MOVE_MS)) >= 0.5)
            .unwrap();
        assert!(half > 0);
        assert_eq!(m.card_at("c", rest, 2, ms(t0, half - 1)).lines, 1);
        assert_eq!(m.card_at("c", rest, 2, ms(t0, half)).lines, 2);
        assert_eq!(m.card_at("c", rest, 2, ms(t0, SHELF_MOVE_MS)).lines, 2);
        // A card that does not move has the final count at once.
        assert_eq!(m.card_at("a", row(2)[0], 2, t0).lines, 2);
        // A card taken off mid-slide leaves with the count it was drawn with.
        let t1 = ms(t0, half - 1);
        m.on_change(
            &cards(&["a", "c"]),
            &row(2),
            2,
            &cards(&["a"]),
            &row(1),
            t1,
            true,
        );
        let (_, ghost) = m.ghosts(t1).next().unwrap();
        assert_eq!(ghost.lines, 1);
    }

    #[test]
    fn an_unchanged_shelf_records_nothing() {
        let t0 = Instant::now();
        let m = change(&["a", "b"], &["a", "b"], t0);
        assert!(m.is_empty() && !m.running(t0));
    }
}
