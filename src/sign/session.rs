//! The editing session: pages, placed items and whole-snapshot undo.

use std::sync::Arc;

use crate::sign::geom::{self, PageGeom};
use crate::sign::{Item, ItemKind, Mark, Quarter};

#[derive(Clone)]
pub struct Page {
    pub geom: PageGeom,
    /// Extra clockwise rotation the user added on top of the source's own.
    pub extra: Quarter,
    pub deleted: bool,
}

/// Returned when the last live page would be deleted.
#[derive(Debug, PartialEq, Eq)]
pub struct LastPage;

const MIN_SIDE: f32 = 8.0;

fn copy_item(i: &Item) -> Item {
    let kind = match &i.kind {
        ItemKind::Signature(m) => ItemKind::Signature(m.clone()),
        ItemKind::Initials(m) => ItemKind::Initials(m.clone()),
        ItemKind::Date(s, st) => ItemKind::Date(s.clone(), *st),
        ItemKind::Text(s, st) => ItemKind::Text(s.clone(), *st),
        ItemKind::Check(t) => ItemKind::Check(*t),
    };
    Item { kind, page: i.page, x: i.x, y: i.y, w: i.w, h: i.h, turn: i.turn }
}

struct Snapshot {
    pages: Vec<Page>,
    items: Vec<Item>,
}

pub struct Session {
    pub pages: Vec<Page>,
    pub items: Vec<Item>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Session {
    pub fn new(geoms: Vec<PageGeom>) -> Session {
        let pages = geoms
            .into_iter()
            .map(|geom| Page { geom, extra: Quarter::R0, deleted: false })
            .collect();
        Session { pages, items: Vec::new(), undo: Vec::new(), redo: Vec::new() }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot { pages: self.pages.clone(), items: self.items.iter().map(copy_item).collect() }
    }

    /// Call before every edit: keeps the state to go back to, drops redo.
    fn checkpoint(&mut self) {
        let snap = self.snapshot();
        self.undo.push(snap);
        self.redo.clear();
    }

    pub fn add(&mut self, item: Item) -> usize {
        self.checkpoint();
        self.items.push(item);
        self.items.len() - 1
    }

    pub fn move_item(&mut self, i: usize, dx: f32, dy: f32) {
        if i >= self.items.len() {
            return;
        }
        self.checkpoint();
        self.items[i].x += dx;
        self.items[i].y += dy;
    }

    /// Marks keep their aspect (driven by `w`); other items take `w` and `h`
    /// as given. Both sides stay between 8 pt and the page's shorter side.
    /// Replace an item's kind and width in one undo step; width is clamped
    /// like `resize_item`. Out of range does nothing.
    pub fn restyle(&mut self, i: usize, kind: ItemKind, w: f32) {
        let Some(it) = self.items.get(i) else { return };
        let max = self
            .pages
            .get(it.page)
            .map(|p| p.geom.w.min(p.geom.h))
            .unwrap_or(f32::MAX)
            .max(MIN_SIDE);
        self.checkpoint();
        self.items[i].kind = kind;
        self.items[i].w = w.clamp(MIN_SIDE, max);
    }

    pub fn resize_item(&mut self, i: usize, w: f32, h: f32) {
        let Some(it) = self.items.get(i) else { return };
        let max = self
            .pages
            .get(it.page)
            .map(|p| p.geom.w.min(p.geom.h))
            .unwrap_or(f32::MAX)
            .max(MIN_SIDE);
        let (ow, oh) = (it.w, it.h);
        let keep = matches!(it.kind, ItemKind::Signature(_) | ItemKind::Initials(_));
        let (nw, nh) = if keep && ow > 0.0 && oh > 0.0 {
            let lo = (MIN_SIDE / ow).max(MIN_SIDE / oh);
            let hi = (max / ow).min(max / oh).max(lo);
            let s = (w / ow).clamp(lo, hi);
            (ow * s, oh * s)
        } else {
            (w.clamp(MIN_SIDE, max), h.clamp(MIN_SIDE, max))
        };
        self.checkpoint();
        self.items[i].w = nw;
        self.items[i].h = nh;
    }

    pub fn remove_item(&mut self, i: usize) {
        if i >= self.items.len() {
            return;
        }
        self.checkpoint();
        self.items.remove(i);
    }

    pub fn rotate_page(&mut self, p: usize) {
        if p >= self.pages.len() {
            return;
        }
        self.checkpoint();
        self.pages[p].extra = self.pages[p].extra.turn_cw();
    }

    pub fn delete_page(&mut self, p: usize) -> Result<(), LastPage> {
        match self.pages.get(p) {
            Some(pg) if !pg.deleted => {}
            _ => return Ok(()),
        }
        if self.live_pages().len() <= 1 {
            return Err(LastPage);
        }
        self.checkpoint();
        self.pages[p].deleted = true;
        self.items.retain(|it| it.page != p);
        Ok(())
    }

    /// Copies an initials item to every other live page at the same spot,
    /// each turned to stand upright on its own page as shown, skipping pages
    /// where it would fall outside the page or that already have the same
    /// initials there (within half a point). Returns how many copies were
    /// made.
    pub fn initials_on_all(&mut self, i: usize) -> usize {
        let Some(src) = self.items.get(i) else { return 0 };
        let ItemKind::Initials(mark) = &src.kind else { return 0 };
        let mark = mark.clone();
        let (from, x, y, w, h) = (src.page, src.x, src.y, src.w, src.h);
        let copies: Vec<Item> = self
            .live_pages()
            .into_iter()
            .filter(|&p| p != from)
            .filter_map(|p| {
                let page = &self.pages[p];
                let turn = geom::upright_turn(page.geom, page.extra);
                let copy = Item { kind: ItemKind::Initials(mark.clone()), page: p, x, y, w, h, turn };
                let (bw, bh) = geom::occupied(&copy);
                let g = page.geom;
                let already = self.has_initials_at(p, &mark, x, y);
                (!already && x >= 0.0 && y >= 0.0 && x + bw <= g.w && y + bh <= g.h).then_some(copy)
            })
            .collect();
        if copies.is_empty() {
            return 0;
        }
        self.checkpoint();
        let n = copies.len();
        self.items.extend(copies);
        n
    }

    /// Every other live page already has item `i`'s initials at its spot
    /// (so "Pasang di semua halaman" has nothing left to add).
    pub fn initials_everywhere(&self, i: usize) -> bool {
        let Some(src) = self.items.get(i) else { return false };
        let ItemKind::Initials(mark) = &src.kind else { return false };
        let others: Vec<usize> = self.live_pages().into_iter().filter(|&p| p != src.page).collect();
        !others.is_empty() && others.iter().all(|&p| self.has_initials_at(p, mark, src.x, src.y))
    }

    /// Page `p` has the same initials (the same `Arc` or an equal mark) at
    /// `x, y`, within half a point.
    fn has_initials_at(&self, p: usize, mark: &Arc<Mark>, x: f32, y: f32) -> bool {
        self.items.iter().any(|it| {
            it.page == p
                && matches!(&it.kind, ItemKind::Initials(m) if Arc::ptr_eq(m, mark) || **m == **mark)
                && (it.x - x).abs() <= 0.5
                && (it.y - y).abs() <= 0.5
        })
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        let cur = self.snapshot();
        self.redo.push(cur);
        self.pages = prev.pages;
        self.items = prev.items;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        let cur = self.snapshot();
        self.undo.push(cur);
        self.pages = next.pages;
        self.items = next.items;
        true
    }

    pub fn dirty(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Something to save: an item placed, a page turned or deleted (an item
    /// added and removed again is nothing).
    pub fn has_changes(&self) -> bool {
        !self.items.is_empty() || self.pages.iter().any(|p| p.deleted || p.extra != Quarter::R0)
    }

    pub fn live_pages(&self) -> Vec<usize> {
        (0..self.pages.len()).filter(|&i| !self.pages[i].deleted).collect()
    }

    pub fn has_rotated(&self) -> bool {
        self.pages.iter().any(|p| !p.deleted && p.extra != Quarter::R0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::geom::PageGeom;
    use crate::sign::{ItemKind, Mark, Quarter};
    use std::sync::Arc;

    fn geom(w: f32, h: f32) -> PageGeom {
        PageGeom { w, h, base: Quarter::R0 }
    }

    fn mark() -> Arc<Mark> {
        Arc::new(Mark { w: 2, h: 1, rgba: vec![0; 8] })
    }

    fn item(kind: ItemKind, page: usize, x: f32, y: f32, w: f32, h: f32) -> Item {
        Item { kind, page, x, y, w, h, turn: Quarter::R0 }
    }

    fn three() -> Session {
        Session::new(vec![geom(612.0, 792.0), geom(612.0, 792.0), geom(612.0, 792.0)])
    }

    #[test]
    fn has_changes_only_with_something_to_save() {
        let mut s = three();
        assert!(!s.has_changes());
        let i = s.add(item(ItemKind::Check(Default::default()), 0, 10.0, 10.0, 20.0, 20.0));
        assert!(s.has_changes());
        s.remove_item(i);
        assert!(s.dirty() && !s.has_changes(), "added then removed: nothing to save");
        s.rotate_page(1);
        assert!(s.has_changes());
        s.undo();
        assert!(!s.has_changes());
        assert_eq!(s.delete_page(2), Ok(()));
        assert!(s.has_changes());
    }

    #[test]
    fn initials_on_all_twice_makes_no_duplicates() {
        let mut s = three();
        let m = mark();
        let i = s.add(item(ItemKind::Initials(m.clone()), 0, 500.0, 40.0, 48.0, 24.0));
        assert!(!s.initials_everywhere(i));
        assert_eq!(s.initials_on_all(i), 2);
        assert!(s.initials_everywhere(i), "the note says it is already there, not that there is no room");
        assert_eq!(s.initials_on_all(i), 0, "every other page already has it");
        assert_eq!(s.items.len(), 3);
        // An equal mark (not the same Arc) within half a point is the same.
        let same = Arc::new((*m).clone());
        s.items[2].kind = ItemKind::Initials(same);
        s.items[2].x += 0.4;
        assert_eq!(s.initials_on_all(i), 0);
        // Moved away on one page: that page gets a copy again.
        s.items[2].x -= 100.0;
        assert_eq!(s.initials_on_all(i), 1);
        assert_eq!(s.items.len(), 4);
    }

    #[test]
    fn rotate_turns_items_with_content() {
        let mut s = three();
        s.add(item(ItemKind::Check(Default::default()), 0, 10.0, 20.0, 30.0, 30.0));
        s.rotate_page(0);
        assert_eq!((s.items[0].x, s.items[0].y, s.items[0].turn), (10.0, 20.0, Quarter::R0));
        assert_eq!(s.pages[0].extra, Quarter::R90);
        assert!(s.has_rotated());
    }

    #[test]
    fn delete_refuses_last_page() {
        let mut s = Session::new(vec![geom(100.0, 100.0)]);
        assert_eq!(s.delete_page(0), Err(LastPage));
        assert_eq!(s.live_pages(), vec![0]);
    }

    #[test]
    fn delete_removes_items_of_that_page() {
        let mut s = three();
        s.add(item(ItemKind::Check(Default::default()), 0, 1.0, 1.0, 20.0, 20.0));
        s.add(item(ItemKind::Check(Default::default()), 1, 1.0, 1.0, 20.0, 20.0));
        assert_eq!(s.delete_page(0), Ok(()));
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].page, 1);
        assert_eq!(s.live_pages(), vec![1, 2]);
        assert!(s.pages[0].deleted);
    }

    #[test]
    fn undo_redo_restores_snapshot() {
        let mut s = three();
        assert!(!s.dirty());
        let i = s.add(item(ItemKind::Check(Default::default()), 0, 10.0, 10.0, 20.0, 20.0));
        s.move_item(i, 5.0, 7.0);
        assert!(s.dirty());
        assert_eq!((s.items[0].x, s.items[0].y), (15.0, 17.0));
        assert!(s.undo());
        assert_eq!((s.items[0].x, s.items[0].y), (10.0, 10.0));
        assert!(s.redo());
        assert_eq!((s.items[0].x, s.items[0].y), (15.0, 17.0));
        assert!(s.undo());
        s.move_item(0, 1.0, 1.0);
        assert!(!s.redo());
        assert!(s.undo() && s.undo());
        assert!(!s.undo());
        assert!(!s.dirty());
    }

    #[test]
    fn initials_on_all_copies_to_live_pages() {
        let mut s = three();
        let i = s.add(item(ItemKind::Initials(mark()), 0, 500.0, 40.0, 60.0, 30.0));
        s.delete_page(1).unwrap();
        s.initials_on_all(i);
        let copies: Vec<&Item> = s.items.iter().filter(|it| it.page != 0).collect();
        assert_eq!(copies.len(), 1);
        assert_eq!((copies[0].page, copies[0].x, copies[0].y), (2, 500.0, 40.0));
    }

    #[test]
    fn initials_on_all_skips_page_where_it_falls_outside() {
        let mut s = Session::new(vec![geom(612.0, 792.0), geom(612.0, 792.0), geom(300.0, 300.0)]);
        let i = s.add(item(ItemKind::Initials(mark()), 0, 500.0, 40.0, 60.0, 30.0));
        s.initials_on_all(i);
        assert!(s.items.iter().any(|it| it.page == 1));
        assert!(!s.items.iter().any(|it| it.page == 2));
    }

    #[test]
    fn initials_copies_stand_upright_on_their_own_page() {
        let mut s = Session::new(vec![
            geom(612.0, 792.0),
            PageGeom { w: 612.0, h: 792.0, base: Quarter::R90 },
            geom(612.0, 792.0),
        ]);
        s.rotate_page(2);
        let i = s.add(item(ItemKind::Initials(mark()), 0, 300.0, 40.0, 60.0, 30.0));
        assert_eq!(s.initials_on_all(i), 2);
        let turn_on = |p: usize| s.items.iter().find(|it| it.page == p).map(|it| it.turn);
        assert_eq!(turn_on(1), Some(Quarter::R90));
        assert_eq!(turn_on(2), Some(Quarter::R90));
        // Upright on page 1 means turned a quarter: its box there is 30 x 60.
        let copy = s.items.iter().find(|it| it.page == 1).unwrap();
        assert_eq!(geom::occupied(copy), (30.0, 60.0));
    }

    #[test]
    fn resize_keeps_aspect_and_limits() {
        let mut s = Session::new(vec![geom(612.0, 792.0)]);
        let i = s.add(item(ItemKind::Signature(mark()), 0, 10.0, 10.0, 100.0, 50.0));
        s.resize_item(i, 200.0, 999.0);
        assert_eq!((s.items[0].w, s.items[0].h), (200.0, 100.0));
        s.resize_item(i, 1.0, 1.0);
        assert_eq!((s.items[0].w, s.items[0].h), (16.0, 8.0));
        s.resize_item(i, 5000.0, 5000.0);
        assert_eq!((s.items[0].w, s.items[0].h), (612.0, 306.0));
        let t = s.add(item(ItemKind::Text("a".into(), Default::default()), 0, 0.0, 0.0, 100.0, 20.0));
        s.resize_item(t, 3.0, 5000.0);
        assert_eq!((s.items[t].w, s.items[t].h), (8.0, 612.0));
    }

    #[test]
    fn restyle_is_one_undo_step() {
        use crate::sign::style::{Family, TextStyle};
        let mut s = three();
        let old = ItemKind::Text("a".into(), TextStyle::default());
        let t = s.add(item(old.clone(), 0, 0.0, 0.0, 100.0, 20.0));
        let new = ItemKind::Text("a".into(), TextStyle { family: Family::Serif, bold: true, ..Default::default() });
        s.restyle(t, new.clone(), 120.0);
        assert_eq!(s.items[t].kind, new);
        assert_eq!(s.items[t].w, 120.0);
        assert!(s.undo());
        assert_eq!(s.items[t].kind, old);
        assert_eq!(s.items[t].w, 100.0);
        assert!(!s.undo() || s.items.is_empty());
    }

    #[test]
    fn restyle_clamps_width() {
        use crate::sign::style::TextStyle;
        let mut s = three();
        let k = ItemKind::Text("a".into(), TextStyle::default());
        let t = s.add(item(k.clone(), 0, 0.0, 0.0, 100.0, 20.0));
        s.restyle(t, k, 1.0e6);
        assert_eq!(s.items[t].w, 612.0);
        s.restyle(99, ItemKind::Check(crate::sign::style::Tint::Black), 5.0);
    }
}
