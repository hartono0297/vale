//! The shelf's thumbnail cache and the large preview's state. Pure: the
//! decoding runs elsewhere (`shelf_thumb`) and its results come in through
//! `set`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::clipboard::image::Thumb;
use crate::shelf::{ShelfCard, previewable};
use crate::shelf_thumb::CARD_SIDE;
use crate::ui::geometry::Rect;

/// What the cache knows about one card's thumbnail.
pub enum Slot {
    Pending,
    Ready(Arc<Thumb>),
    /// Could not be decoded; not asked again.
    Failed,
}

/// Card thumbnails by card key.
#[derive(Default)]
pub struct ShelfThumbs {
    slots: HashMap<String, Slot>,
}

impl ShelfThumbs {
    /// The demo's cache: slots filled without a decode (debug builds only).
    #[cfg(debug_assertions)]
    pub fn with_slots(slots: impl IntoIterator<Item = (String, Slot)>) -> ShelfThumbs {
        ShelfThumbs {
            slots: slots.into_iter().collect(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Slot> {
        self.slots.get(key)
    }

    /// The indices into `cards` that need a decode: previewable, no slot yet
    /// and not on the network (`on_network(index)`). Marks them `Pending`.
    pub fn wanted(
        &mut self,
        cards: &[ShelfCard],
        on_network: impl Fn(usize) -> bool,
    ) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, card) in cards.iter().enumerate() {
            if previewable(card) && !self.slots.contains_key(&card.key) && !on_network(i) {
                self.slots.insert(card.key.clone(), Slot::Pending);
                out.push(i);
            }
        }
        out
    }

    /// A decode finished: only a `Pending` slot takes it (a card that left,
    /// or a slot already settled, ignores it).
    pub fn set(&mut self, key: &str, thumb: Option<Arc<Thumb>>) {
        if let Some(slot @ Slot::Pending) = self.slots.get_mut(key) {
            *slot = match thumb {
                Some(t) => Slot::Ready(t),
                None => Slot::Failed,
            };
        }
    }

    /// Drops the slots of cards no longer on the shelf.
    pub fn retain(&mut self, cards: &[ShelfCard]) {
        self.slots.retain(|key, _| cards.iter().any(|c| &c.key == key));
    }
}

pub enum PreviewState {
    Loading,
    Failed,
    Ready(Arc<Thumb>),
}

/// The large preview of one card.
pub struct Preview {
    pub key: String,
    pub state: PreviewState,
}

impl Preview {
    pub fn new(key: String) -> Preview {
        Preview {
            key,
            state: PreviewState::Loading,
        }
    }

    /// The decode for `key` finished; ignored for another key or once the
    /// preview is no longer loading.
    pub fn set(&mut self, key: &str, thumb: Option<Arc<Thumb>>) {
        if self.key == key && matches!(self.state, PreviewState::Loading) {
            self.state = match thumb {
                Some(t) => PreviewState::Ready(t),
                None => PreviewState::Failed,
            };
        }
    }

    /// Its card is still on the shelf and previewable.
    pub fn still_open(&self, cards: &[ShelfCard]) -> bool {
        cards.iter().any(|c| c.key == self.key && previewable(c))
    }
}

/// The decode jobs (key, path, long side, is-preview) for the cards at the
/// `wanted` indices, in order. A card whose path is gone from the list is
/// left out.
pub fn shelf_jobs(
    cards: &[ShelfCard],
    path_of: impl Fn(usize) -> Option<PathBuf>,
    wanted: &[usize],
) -> Vec<(String, PathBuf, u32, bool)> {
    wanted
        .iter()
        .filter_map(|&i| {
            let card = cards.get(i)?;
            Some((card.key.clone(), path_of(i)?, CARD_SIDE, false))
        })
        .collect()
}

/// The largest rect of the picture's ratio inside `bx`, centred, never
/// above 1:1.
pub fn fit_contain(w: f32, h: f32, bx: Rect) -> Rect {
    let scale = (bx.w / w).min(bx.h / h).min(1.0);
    let (fw, fh) = (w * scale, h * scale);
    Rect {
        x: bx.x + (bx.w - fw) / 2.0,
        y: bx.y + (bx.h - fh) / 2.0,
        w: fw,
        h: fh,
    }
}

/// The centred region (x, y, w, h) of a `w` x `h` picture that fills a
/// `box_w` x `box_h` box without distortion.
pub fn cover_source(w: f32, h: f32, box_w: f32, box_h: f32) -> (f32, f32, f32, f32) {
    let scale = (box_w / w).max(box_h / h);
    let (sw, sh) = (box_w / scale, box_h / scale);
    ((w - sw) / 2.0, (h - sh) / 2.0, sw, sh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shelf::{ShelfCard, ShelfKind};
    use crate::ui::geometry::Rect;

    fn card(key: &str, kind: ShelfKind, stale: bool) -> ShelfCard {
        ShelfCard {
            key: key.to_string(),
            name: key.to_string(),
            kind,
            stale,
            from_phone: false,
        }
    }

    fn thumb() -> Arc<Thumb> {
        Arc::new(Thumb {
            w: 1,
            h: 1,
            bgra: vec![0, 0, 0, 255],
        })
    }

    #[test]
    fn only_image_cards_that_are_there_are_previewable() {
        assert!(crate::shelf::previewable(&card("a", ShelfKind::Image, false)));
        assert!(!crate::shelf::previewable(&card("a", ShelfKind::Image, true)));
        assert!(!crate::shelf::previewable(&card("a", ShelfKind::Folder, false)));
        assert!(!crate::shelf::previewable(&card("a", ShelfKind::Video, false)));
        assert!(!crate::shelf::previewable(&card("a", ShelfKind::Document, false)));
    }

    #[test]
    fn wanted_marks_pending_once_and_skips_network_and_non_images() {
        let cards = [
            card("a", ShelfKind::Image, false),
            card("b", ShelfKind::Image, false),
            card("c", ShelfKind::Document, false),
        ];
        let mut thumbs = ShelfThumbs::default();
        assert_eq!(thumbs.wanted(&cards, |i| i == 1), vec![0]);
        assert!(matches!(thumbs.get("a"), Some(Slot::Pending)));
        assert!(thumbs.get("b").is_none());
        assert!(thumbs.get("c").is_none());
        assert_eq!(thumbs.wanted(&cards, |i| i == 1), Vec::<usize>::new());
    }

    #[test]
    fn a_result_fills_a_pending_slot_a_failure_is_remembered_and_never_asked_again() {
        let cards = [
            card("a", ShelfKind::Image, false),
            card("b", ShelfKind::Image, false),
        ];
        let mut thumbs = ShelfThumbs::default();
        thumbs.wanted(&cards, |_| false);
        thumbs.set("a", Some(thumb()));
        thumbs.set("b", None);
        assert!(matches!(thumbs.get("a"), Some(Slot::Ready(_))));
        assert!(matches!(thumbs.get("b"), Some(Slot::Failed)));
        // A second result does not change a slot that is no longer pending.
        thumbs.set("b", Some(thumb()));
        assert!(matches!(thumbs.get("b"), Some(Slot::Failed)));
        assert_eq!(thumbs.wanted(&cards, |_| false), Vec::<usize>::new());
    }

    #[test]
    fn a_result_for_a_card_that_left_is_ignored() {
        let cards = [card("a", ShelfKind::Image, false)];
        let mut thumbs = ShelfThumbs::default();
        thumbs.wanted(&cards, |_| false);
        thumbs.retain(&[]);
        thumbs.set("a", Some(thumb()));
        assert!(thumbs.get("a").is_none());
    }

    #[test]
    fn retain_keeps_only_cards_on_the_shelf() {
        let cards = [
            card("a", ShelfKind::Image, false),
            card("b", ShelfKind::Image, false),
        ];
        let mut thumbs = ShelfThumbs::default();
        thumbs.wanted(&cards, |_| false);
        thumbs.retain(&cards[1..]);
        assert!(thumbs.get("a").is_none());
        assert!(thumbs.get("b").is_some());
    }

    #[test]
    fn preview_takes_only_its_own_result_once() {
        let mut preview = Preview::new("a".to_string());
        assert!(matches!(preview.state, PreviewState::Loading));
        preview.set("other", Some(thumb()));
        assert!(matches!(preview.state, PreviewState::Loading));
        preview.set("a", Some(thumb()));
        assert!(matches!(preview.state, PreviewState::Ready(_)));
        preview.set("a", None);
        assert!(matches!(preview.state, PreviewState::Ready(_)));
        let mut failed = Preview::new("a".to_string());
        failed.set("a", None);
        assert!(matches!(failed.state, PreviewState::Failed));
        failed.set("a", Some(thumb()));
        assert!(matches!(failed.state, PreviewState::Failed));
    }

    #[test]
    fn preview_closes_when_its_card_goes_or_goes_stale() {
        let preview = Preview::new("a".to_string());
        assert!(preview.still_open(&[card("a", ShelfKind::Image, false)]));
        assert!(!preview.still_open(&[card("b", ShelfKind::Image, false)]));
        assert!(!preview.still_open(&[card("a", ShelfKind::Image, true)]));
        assert!(!preview.still_open(&[]));
    }

    #[test]
    fn jobs_pair_each_wanted_index_with_its_path_and_the_card_side() {
        let cards = [
            card("a", ShelfKind::Image, false),
            card("b", ShelfKind::Image, false),
            card("c", ShelfKind::Image, false),
        ];
        let path_of = |i: usize| match i {
            2 => None,
            _ => Some(PathBuf::from(format!("p{i}.png"))),
        };
        let jobs = shelf_jobs(&cards, path_of, &[1, 0, 2]);
        assert_eq!(
            jobs,
            vec![
                ("b".to_string(), PathBuf::from("p1.png"), CARD_SIDE, false),
                ("a".to_string(), PathBuf::from("p0.png"), CARD_SIDE, false),
            ]
        );
    }

    #[test]
    fn fit_contain_keeps_the_ratio_centres_and_never_enlarges() {
        let bx = Rect {
            x: 10.0,
            y: 20.0,
            w: 300.0,
            h: 300.0,
        };
        let small = fit_contain(100.0, 50.0, bx);
        assert_eq!((small.w, small.h), (100.0, 50.0));
        assert_eq!((small.x, small.y), (110.0, 145.0));
        let big = fit_contain(600.0, 300.0, bx);
        assert_eq!((big.w, big.h), (300.0, 150.0));
        assert_eq!((big.x, big.y), (10.0, 95.0));
    }

    #[test]
    fn cover_source_crops_the_long_side() {
        assert_eq!(cover_source(200.0, 100.0, 50.0, 50.0), (50.0, 0.0, 100.0, 100.0));
        assert_eq!(cover_source(100.0, 200.0, 50.0, 50.0), (0.0, 50.0, 100.0, 100.0));
    }
}
