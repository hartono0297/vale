//! The clipboard history: newest first, capped, with pins, de-duplication,
//! search and the image budgets. `ingest` turns a `ClipSnapshot` (what the
//! Win32 reader saw, or a test's fake) into an entry. Pure.

use std::path::PathBuf;
use std::sync::Arc;

use super::image::{self, Thumb};
use super::kinds::{ClipKind, detect_kind, should_skip};

/// Default and range of "Jumlah item".
pub const DEFAULT_ITEMS: u32 = 50;
pub const MIN_ITEMS: u32 = 10;
pub const MAX_ITEMS: u32 = 200;
pub const ITEMS_STEP: u32 = 10;
/// A text longer than this (in UTF-8 bytes) is not kept at all: a cut copy
/// pasted back would silently lose its end.
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
/// Bitmaps larger than this are not read.
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
/// All thumbnails together (memory and file); the oldest unpinned image
/// entries go first when a new one would pass it.
pub const THUMB_BUDGET: usize = 4 * 1024 * 1024;
/// Full-size image data, memory only, counted in the bytes it takes (the
/// compressed size once an original is packed, see `Original`); the oldest
/// images lose theirs first and then copy back at thumbnail size. The newest
/// image is outside it.
pub const FULL_BUDGET: usize = 16 * 1024 * 1024;

pub fn clamp_items(n: u32) -> u32 {
    n.clamp(MIN_ITEMS, MAX_ITEMS)
}

/// How a packed original's pixel array is laid out in its PNG (`clipboard::pack`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngLayout {
    /// 32 bpp: `w` x `h` BGRA, stride `w * 4`.
    Bgra { w: u32, h: u32 },
    /// 24 bpp: 8-bit gray, `stride` x `h` (the rows with their padding, so
    /// the bytes come back exactly).
    Gray { stride: u32, h: u32 },
}

/// An original DIB with its pixel array PNG-compressed: `head` (the bytes
/// before the pixels: header, masks, colour table) and `tail` (any bytes
/// after them) verbatim, so `head + pixels + tail` is the DIB again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packed {
    pub head: Vec<u8>,
    pub png: Vec<u8>,
    pub tail: Vec<u8>,
    /// The length of the whole DIB.
    pub raw_len: usize,
    pub layout: PngLayout,
}

impl Packed {
    /// The bytes it takes in memory (what the budget counts).
    pub fn bytes(&self) -> usize {
        self.head.len() + self.png.len() + self.tail.len()
    }
}

/// The original of an image: the packed DIB as the clipboard gave it, or the
/// same PNG-compressed (memory only, never saved).
#[derive(Clone, Debug, PartialEq)]
pub enum Original {
    Raw(Arc<Vec<u8>>),
    Packed(Arc<Packed>),
}

/// Where a raw original is on its way to being packed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PackState {
    /// Not handed to a worker (yet).
    #[default]
    Idle,
    /// A worker has it; its answer is awaited.
    Pending,
    /// The worker could not pack it (or the check failed): stays raw.
    Failed,
}

/// A raw original for a worker to pack.
#[derive(Clone, Debug)]
pub struct PackJob {
    pub id: u64,
    pub fingerprint: u64,
    pub raw: Arc<Vec<u8>>,
}

/// A bitmap entry: the thumbnail (kept and saved) and, while the budget
/// allows, the original DIB (memory only).
#[derive(Clone, Debug, PartialEq)]
pub struct ClipImage {
    pub width: u32,
    pub height: u32,
    pub thumb: Thumb,
    /// `image::fingerprint` of the original, for de-duplication.
    pub fingerprint: u64,
    /// Only bitmap formats were on the clipboard (PrtSc, Snipping Tool).
    pub screenshot: bool,
    pub full: Option<Original>,
    pub pack: PackState,
}

impl ClipImage {
    /// Any original is kept (else it copies back at thumbnail size).
    pub fn has_original(&self) -> bool {
        self.full.is_some()
    }

    /// The original as the clipboard gave it (not packed).
    pub fn raw(&self) -> Option<&Arc<Vec<u8>>> {
        match &self.full {
            Some(Original::Raw(raw)) => Some(raw),
            _ => None,
        }
    }

    pub fn packed(&self) -> Option<&Arc<Packed>> {
        match &self.full {
            Some(Original::Packed(p)) => Some(p),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipData {
    Text(String),
    Files(Vec<PathBuf>),
    Image(ClipImage),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// Unique for this run (and kept in the file).
    pub id: u64,
    pub kind: ClipKind,
    pub data: ClipData,
    pub pinned: bool,
    /// When it was (last) copied, unix seconds.
    pub at: i64,
}

impl Clip {
    /// The text the card, the search and "copy as plain text" use: the
    /// text itself, paths one per line, nothing for an image.
    pub fn plain_text(&self) -> Option<String> {
        match &self.data {
            ClipData::Text(t) => Some(t.clone()),
            ClipData::Files(paths) => Some(
                paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("\r\n"),
            ),
            ClipData::Image(_) => None,
        }
    }

    pub fn image(&self) -> Option<&ClipImage> {
        match &self.data {
            ClipData::Image(i) => Some(i),
            _ => None,
        }
    }

    /// Same content as `other` (the id, pin and time aside).
    fn same_content(&self, other: &ClipData) -> bool {
        match (&self.data, other) {
            (ClipData::Text(a), ClipData::Text(b)) => a == b,
            (ClipData::Files(a), ClipData::Files(b)) => a == b,
            (ClipData::Image(a), ClipData::Image(b)) => {
                (a.width, a.height, a.fingerprint) == (b.width, b.height, b.fingerprint)
            }
            _ => false,
        }
    }
}

/// What the reader found on the clipboard, closed again by now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipSnapshot {
    /// Registered formats by name, with the DWORD of an excluded one.
    pub formats: Vec<(String, Option<u32>)>,
    /// Only bitmap formats were present.
    pub only_bitmaps: bool,
    pub text: Option<String>,
    pub files: Vec<PathBuf>,
    /// A packed DIB.
    pub dib: Option<Vec<u8>>,
    /// `GetClipboardSequenceNumber()` when it was read (the screenshot
    /// preview's "Hapus" empties only while it is still this one).
    pub sequence: u32,
}

/// What `ingest` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ingested {
    /// Password-manager content: nothing stored, nothing logged.
    Skipped,
    /// Nothing the history keeps (an unknown format, too big, empty).
    Nothing,
    Added {
        id: u64,
        image: bool,
    },
    /// The same content was already there: moved to the front.
    Bumped {
        id: u64,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    /// Newest first.
    items: Vec<Clip>,
    next_id: u64,
    cap: usize,
}

impl History {
    pub fn new(cap: u32) -> Self {
        History {
            items: Vec::new(),
            next_id: 1,
            cap: clamp_items(cap) as usize,
        }
    }

    /// Restored entries (newest first), ids renumbered.
    pub fn from_items(cap: u32, items: Vec<Clip>) -> Self {
        let mut h = History::new(cap);
        for mut clip in items {
            clip.id = h.take_id();
            h.items.push(clip);
        }
        h.enforce();
        h
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn items(&self) -> &[Clip] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: u64) -> Option<&Clip> {
        self.items.iter().find(|c| c.id == id)
    }

    pub fn set_cap(&mut self, cap: u32) {
        self.cap = clamp_items(cap) as usize;
        self.enforce();
    }

    /// Adds `data` at the front, or moves an entry with the same content
    /// there (its pin kept), then applies the cap and the image budgets.
    pub fn push(&mut self, kind: ClipKind, data: ClipData, at: i64) -> Ingested {
        if let Some(i) = self.items.iter().position(|c| c.same_content(&data)) {
            let mut clip = self.items.remove(i);
            clip.at = at;
            // A newer copy of an image brings its raw original back over a
            // packed one or none.
            if let (ClipData::Image(old), ClipData::Image(new)) = (&mut clip.data, data)
                && old.raw().is_none()
            {
                old.full = new.full;
                old.pack = PackState::Idle;
            }
            let id = clip.id;
            self.items.insert(0, clip);
            self.enforce();
            return Ingested::Bumped { id };
        }
        let image = matches!(data, ClipData::Image(_));
        let id = self.take_id();
        self.items.insert(
            0,
            Clip {
                id,
                kind,
                data,
                pinned: false,
                at,
            },
        );
        self.enforce();
        Ingested::Added { id, image }
    }

    /// The island copied entry `id` back: it moves to the front, no
    /// duplicate is made.
    pub fn bump(&mut self, id: u64, at: i64) -> bool {
        let Some(i) = self.items.iter().position(|c| c.id == id) else {
            return false;
        };
        let mut clip = self.items.remove(i);
        clip.at = at;
        self.items.insert(0, clip);
        true
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> bool {
        let Some(c) = self.items.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        let changed = c.pinned != pinned;
        c.pinned = pinned;
        if changed {
            self.enforce();
        }
        changed
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|c| c.id != id);
        self.items.len() != before
    }

    /// Everything not pinned goes (the clear button, a locked screen).
    /// True when something went.
    pub fn clear_unpinned(&mut self) -> bool {
        let before = self.items.len();
        self.items.retain(|c| c.pinned);
        self.items.len() != before
    }

    /// The cap (oldest unpinned first, never a pin and never the newest
    /// entry), then the thumbnail budget (oldest unpinned images go), then
    /// the full-size budget (the oldest images lose their original; never
    /// the newest).
    fn enforce(&mut self) {
        while self.items.len() > self.cap {
            let Some(i) = self.oldest_unpinned(|_| true) else {
                break;
            };
            self.items.remove(i);
        }
        while self.thumb_bytes() > THUMB_BUDGET {
            let Some(i) = self.oldest_unpinned(|c| c.image().is_some()) else {
                break;
            };
            self.items.remove(i);
        }
        // The newest image keeps its original whatever its size or form (a
        // text copied after it changes nothing); the others share the budget,
        // newest first.
        let newest = self.newest_image();
        let mut full = 0usize;
        for (i, clip) in self.items.iter_mut().enumerate() {
            if Some(i) == newest {
                continue;
            }
            let ClipData::Image(img) = &mut clip.data else {
                continue;
            };
            let cost = match (&img.full, img.pack) {
                (None, _) => continue,
                // Waiting for (or about to be handed to) a worker: exempt
                // until it answers, else a 4K picture would be dropped in
                // the very call that makes it older.
                (Some(Original::Raw(_)), PackState::Idle | PackState::Pending) => continue,
                (Some(Original::Raw(raw)), PackState::Failed) => raw.len(),
                (Some(Original::Packed(p)), _) => p.bytes(),
            };
            if full + cost > FULL_BUDGET {
                img.full = None;
                img.pack = PackState::Idle;
            } else {
                full += cost;
            }
        }
    }

    /// The index of the newest image entry (not necessarily entry 0).
    fn newest_image(&self) -> Option<usize> {
        self.items.iter().position(|c| c.image().is_some())
    }

    /// The raw originals of the images that are not the newest and not yet
    /// handed out, to pack on a worker. They are marked `Pending`.
    pub fn pack_jobs(&mut self) -> Vec<PackJob> {
        let newest = self.newest_image();
        let mut jobs = Vec::new();
        for (i, clip) in self.items.iter_mut().enumerate() {
            if Some(i) == newest {
                continue;
            }
            let id = clip.id;
            if let ClipData::Image(img) = &mut clip.data
                && img.pack == PackState::Idle
                && let Some(Original::Raw(raw)) = &img.full
            {
                jobs.push(PackJob {
                    id,
                    fingerprint: img.fingerprint,
                    raw: raw.clone(),
                });
                img.pack = PackState::Pending;
            }
        }
        jobs
    }

    /// A worker's answer for entry `id`. Applied only while the entry still
    /// exists with that fingerprint, still holds its raw original and is
    /// waiting for this answer; if it became the newest image meanwhile it
    /// just stays raw. `Some` replaces the raw original, `None` marks it
    /// `Failed` (stays raw). True when applied.
    pub fn set_packed(&mut self, id: u64, fingerprint: u64, result: Option<Arc<Packed>>) -> bool {
        let newest = self.newest_image();
        let Some(i) = self.items.iter().position(|c| c.id == id) else {
            return false;
        };
        let ClipData::Image(img) = &mut self.items[i].data else {
            return false;
        };
        if img.fingerprint != fingerprint || img.pack != PackState::Pending || img.raw().is_none() {
            return false;
        }
        if newest == Some(i) {
            img.pack = PackState::Idle;
            return false;
        }
        match result {
            Some(p) => {
                img.full = Some(Original::Packed(p));
                img.pack = PackState::Idle;
            }
            None => img.pack = PackState::Failed,
        }
        self.enforce();
        true
    }

    /// Entry `id` was pasted from its packed original: `raw` is that decoded
    /// DIB (it must be as long as the packed one), kept raw as the entry is
    /// now the newest. True when applied.
    pub fn set_raw(&mut self, id: u64, raw: Arc<Vec<u8>>) -> bool {
        let Some(clip) = self.items.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        let ClipData::Image(img) = &mut clip.data else {
            return false;
        };
        if img.packed().is_none_or(|p| p.raw_len != raw.len()) {
            return false;
        }
        img.full = Some(Original::Raw(raw));
        img.pack = PackState::Idle;
        self.enforce();
        true
    }

    /// Entry `id`'s original is unusable (its pack did not decode): only the
    /// thumbnail is left.
    pub fn forget_original(&mut self, id: u64) {
        if let Some(clip) = self.items.iter_mut().find(|c| c.id == id)
            && let ClipData::Image(img) = &mut clip.data
        {
            img.full = None;
            img.pack = PackState::Idle;
        }
    }

    /// The last unpinned entry matching `f`, never the newest one.
    fn oldest_unpinned(&self, f: impl Fn(&Clip) -> bool) -> Option<usize> {
        (1..self.items.len())
            .rev()
            .find(|&i| !self.items[i].pinned && f(&self.items[i]))
    }

    pub fn thumb_bytes(&self) -> usize {
        self.items
            .iter()
            .filter_map(Clip::image)
            .map(|i| i.thumb.bytes())
            .sum()
    }

    /// The entries matching `query` (case-insensitive, over the text, the
    /// paths and the kind's label from `label`), newest first. An empty
    /// query matches all.
    pub fn search<'a>(&'a self, query: &str, label: impl Fn(ClipKind) -> &'a str) -> Vec<&'a Clip> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.items.iter().collect();
        }
        self.items
            .iter()
            .filter(|c| {
                label(c.kind).to_lowercase().contains(&q)
                    || c.plain_text()
                        .is_some_and(|t| t.to_lowercase().contains(&q))
            })
            .collect()
    }
}

/// The history's side of a clipboard change: password-manager content is
/// skipped, then files, else text, else a bitmap becomes an entry.
pub fn ingest(history: &mut History, snap: ClipSnapshot, at: i64) -> Ingested {
    match entry_for(snap) {
        Ok((kind, data)) => history.push(kind, data, at),
        Err(not_kept) => not_kept,
    }
}

/// The entry a snapshot becomes, or why it is not kept (`Skipped`:
/// password-manager content; `Nothing`: an unknown format, too big, empty).
fn entry_for(snap: ClipSnapshot) -> Result<(ClipKind, ClipData), Ingested> {
    if should_skip(&snap.formats) {
        return Err(Ingested::Skipped);
    }
    if !snap.files.is_empty() {
        return Ok((ClipKind::Files, ClipData::Files(snap.files)));
    }
    if let Some(text) = snap.text.filter(|t| !t.trim().is_empty()) {
        if text.len() > MAX_TEXT_BYTES {
            return Err(Ingested::Nothing);
        }
        let kind = detect_kind(&text);
        return Ok((kind, ClipData::Text(text)));
    }
    match make_image(snap.dib, snap.only_bitmaps) {
        Some(img) => Ok((ClipKind::Image, ClipData::Image(img))),
        None => Err(Ingested::Nothing),
    }
}

/// `ingest` for an editor's auto-copy of one capture: when `card` (the card
/// this capture made before) is still there and not pinned, it goes before
/// the new picture is pushed, so a capture is one card, with a new id (the
/// thumbnail cache is keyed by id and never shows the old picture) at the
/// front. A pinned card is never changed or removed: the newer picture
/// becomes its own unpinned card. A snapshot that is not kept (skipped, over
/// the size cap) leaves the old card as it is.
pub fn ingest_replacing(
    history: &mut History,
    snap: ClipSnapshot,
    at: i64,
    card: Option<u64>,
) -> Ingested {
    let (kind, data) = match entry_for(snap) {
        Ok(entry) => entry,
        Err(not_kept) => return not_kept,
    };
    if let Some(id) = card
        && history.get(id).is_some_and(|c| !c.pinned)
    {
        history.delete(id);
    }
    history.push(kind, data, at)
}

/// How many captures keep a card they may replace.
const CAPTURE_CARDS: usize = 16;

/// Which card each capture's auto-copy made (the newest 16 captures).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureCards {
    /// Oldest first.
    list: Vec<(u64, u64)>,
}

impl CaptureCards {
    /// The card `capture` made last, which its next picture replaces. `None`
    /// when another capture points at the same card too (two pixel-identical
    /// captures: the dedupe bumped one card for both): neither may remove
    /// it, so two captures keep two cards.
    pub fn card(&self, capture: u64) -> Option<u64> {
        let id = self
            .list
            .iter()
            .find(|(c, _)| *c == capture)
            .map(|(_, id)| *id)?;
        let shared = self.list.iter().any(|(c, i)| *c != capture && *i == id);
        (!shared).then_some(id)
    }

    /// `capture`'s card is now `card`; the oldest capture falls out past 16.
    pub fn set(&mut self, capture: u64, card: u64) {
        self.list.retain(|(c, _)| *c != capture);
        self.list.push((capture, card));
        if self.list.len() > CAPTURE_CARDS {
            self.list.remove(0);
        }
    }
}

/// The bitmap entry for `dib`, if it is one the history reads.
fn make_image(dib: Option<Vec<u8>>, only_bitmaps: bool) -> Option<ClipImage> {
    let dib = dib.filter(|d| d.len() <= MAX_IMAGE_BYTES)?;
    let info = image::dib_info(&dib)?;
    Some(ClipImage {
        width: info.w,
        height: info.h,
        thumb: image::thumbnail(&dib, &info),
        fingerprint: image::fingerprint(&dib),
        screenshot: only_bitmaps,
        // Kept even past the budget: the newest pastes at full size.
        full: Some(Original::Raw(Arc::new(dib))),
        pack: PackState::Idle,
    })
}

/// With the history off and the screenshot preview on: the image `ingest`
/// would have made of `snap`, by the same rules (password-manager content
/// skipped, files and text first), without any entry.
pub fn image_only(snap: ClipSnapshot) -> Option<ClipImage> {
    if should_skip(&snap.formats)
        || !snap.files.is_empty()
        || snap.text.as_ref().is_some_and(|t| !t.trim().is_empty())
    {
        return None;
    }
    make_image(snap.dib, snap.only_bitmaps)
}

/// A one-line preview for the "Disalin" notice: whitespace runs become one
/// space, at most `max` characters (the renderer trims to the width).
pub fn preview(text: &str, max: usize) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &one[..i]),
        None => one,
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::image::tests::dib24;
    use super::*;

    pub fn text(s: &str) -> ClipSnapshot {
        ClipSnapshot {
            text: Some(s.into()),
            ..ClipSnapshot::default()
        }
    }

    fn texts(h: &History) -> Vec<String> {
        h.items().iter().filter_map(Clip::plain_text).collect()
    }

    #[test]
    fn newest_first_with_a_cap_of_fifty() {
        let mut h = History::new(DEFAULT_ITEMS);
        for i in 0..60 {
            ingest(&mut h, text(&format!("item {i}")), i);
        }
        assert_eq!(h.len(), 50);
        assert_eq!(texts(&h)[0], "item 59");
        assert_eq!(texts(&h)[49], "item 10");
    }

    #[test]
    fn pins_survive_sixty_inserts() {
        let mut h = History::new(50);
        ingest(&mut h, text("keep me"), 0);
        let id = h.items()[0].id;
        assert!(h.set_pinned(id, true));
        for i in 0..60 {
            ingest(&mut h, text(&format!("n{i}")), i + 1);
        }
        assert_eq!(h.len(), 50);
        assert!(h.get(id).is_some_and(|c| c.pinned));
        // The pin keeps its place by age; 49 newest unpinned remain.
        assert_eq!(texts(&h)[0], "n59");
        assert_eq!(texts(&h).last().unwrap(), "keep me");
        assert_eq!(texts(&h)[48], "n11");
    }

    #[test]
    fn pins_over_the_cap_stay_and_the_newest_copy_too() {
        let mut h = History::new(10);
        for i in 0..12 {
            ingest(&mut h, text(&format!("p{i}")), i);
            let id = h.items()[0].id;
            h.set_pinned(id, true);
        }
        assert_eq!(h.len(), 12, "pins are never pushed out");
        ingest(&mut h, text("fresh"), 20);
        assert_eq!(h.len(), 13);
        assert_eq!(texts(&h)[0], "fresh");
        ingest(&mut h, text("fresher"), 21);
        // The older unpinned one goes, the newest stays.
        assert_eq!(h.len(), 13);
        assert_eq!(texts(&h)[0], "fresher");
        assert!(!texts(&h).contains(&"fresh".to_string()));
        // A lower cap applies at once.
        let mut h = History::new(50);
        for i in 0..30 {
            ingest(&mut h, text(&format!("x{i}")), i);
        }
        h.set_cap(10);
        assert_eq!(h.len(), 10);
    }

    #[test]
    fn the_cap_is_clamped() {
        assert_eq!(clamp_items(0), MIN_ITEMS);
        assert_eq!(clamp_items(5000), MAX_ITEMS);
        assert_eq!(clamp_items(50), 50);
    }

    #[test]
    fn same_text_moves_to_the_front_without_a_duplicate() {
        let mut h = History::new(50);
        ingest(&mut h, text("a"), 1);
        ingest(&mut h, text("b"), 2);
        let a = h.items()[1].id;
        h.set_pinned(a, true);
        assert_eq!(ingest(&mut h, text("a"), 3), Ingested::Bumped { id: a });
        assert_eq!(texts(&h), ["a", "b"]);
        assert!(h.items()[0].pinned, "the pin stays");
        assert_eq!(h.items()[0].at, 3);
    }

    #[test]
    fn an_island_copy_bumps_the_entry() {
        let mut h = History::new(50);
        ingest(&mut h, text("a"), 1);
        ingest(&mut h, text("b"), 2);
        let a = h.items()[1].id;
        assert!(h.bump(a, 5));
        assert_eq!(texts(&h), ["a", "b"]);
        assert!(!h.bump(999, 5));
    }

    #[test]
    fn delete_and_clear_unpinned() {
        let mut h = History::new(50);
        for s in ["a", "b", "c"] {
            ingest(&mut h, text(s), 0);
        }
        let b = h.items()[1].id;
        assert!(h.delete(b));
        assert!(!h.delete(b));
        assert_eq!(texts(&h), ["c", "a"]);
        let a = h.items()[1].id;
        h.set_pinned(a, true);
        // The lock (and the clear button) keeps pinned entries.
        assert!(h.clear_unpinned());
        assert_eq!(texts(&h), ["a"]);
        assert!(!h.clear_unpinned());
    }

    #[test]
    fn search_is_case_insensitive_over_text_paths_and_kind() {
        let mut h = History::new(50);
        ingest(&mut h, text("Denah lantai 2"), 1);
        ingest(&mut h, text("#E2683C"), 2);
        let files = ClipSnapshot {
            files: vec![PathBuf::from(r"D:\Proyek\Laporan.pdf")],
            ..ClipSnapshot::default()
        };
        ingest(&mut h, files, 3);
        let label = |k: ClipKind| match k {
            ClipKind::Color => "Warna",
            ClipKind::Files => "File",
            _ => "Teks",
        };
        let found =
            |q: &str| -> Vec<ClipKind> { h.search(q, label).iter().map(|c| c.kind).collect() };
        assert_eq!(found("DENAH"), [ClipKind::Text]);
        assert_eq!(found("laporan"), [ClipKind::Files]);
        assert_eq!(found("warna"), [ClipKind::Color]);
        assert_eq!(found("e268"), [ClipKind::Color]);
        assert_eq!(found("  ").len(), 3);
        assert!(found("tidak ada").is_empty());
    }

    #[test]
    fn files_win_over_text_and_huge_text_is_dropped() {
        let mut h = History::new(50);
        let both = ClipSnapshot {
            text: Some("x".into()),
            files: vec![PathBuf::from(r"C:\a.txt")],
            ..ClipSnapshot::default()
        };
        assert!(matches!(
            ingest(&mut h, both, 0),
            Ingested::Added { image: false, .. }
        ));
        assert_eq!(h.items()[0].kind, ClipKind::Files);
        let huge = "a".repeat(MAX_TEXT_BYTES + 1);
        assert_eq!(ingest(&mut h, text(&huge), 1), Ingested::Nothing);
        assert_eq!(ingest(&mut h, text("   "), 1), Ingested::Nothing);
        assert_eq!(
            ingest(&mut h, ClipSnapshot::default(), 1),
            Ingested::Nothing
        );
    }

    #[test]
    fn skipped_content_stores_nothing() {
        let mut h = History::new(50);
        let secret = ClipSnapshot {
            formats: vec![("CanIncludeInClipboardHistory".into(), Some(0))],
            text: Some("hunter2".into()),
            ..ClipSnapshot::default()
        };
        assert_eq!(ingest(&mut h, secret, 0), Ingested::Skipped);
        assert!(h.is_empty());
    }

    pub fn image(w: u32, h: u32) -> ClipSnapshot {
        ClipSnapshot {
            dib: Some(dib24(w, h)),
            only_bitmaps: true,
            ..ClipSnapshot::default()
        }
    }

    #[test]
    fn images_get_a_thumbnail_and_dedupe() {
        let mut h = History::new(50);
        let added = ingest(&mut h, image(1280, 720), 0);
        assert!(matches!(added, Ingested::Added { image: true, .. }));
        let img = h.items()[0].image().unwrap();
        assert_eq!((img.width, img.height), (1280, 720));
        assert!(img.thumb.w <= image::THUMB_MAX_W && img.thumb.h <= image::THUMB_MAX_H);
        assert!(img.screenshot && img.raw().is_some());
        assert!(matches!(
            ingest(&mut h, image(1280, 720), 1),
            Ingested::Bumped { .. }
        ));
        assert_eq!(h.len(), 1);
        let bad = ClipSnapshot {
            dib: Some(b"not a bitmap".to_vec()),
            ..ClipSnapshot::default()
        };
        assert_eq!(ingest(&mut h, bad, 2), Ingested::Nothing);
    }

    #[test]
    fn image_only_follows_the_ingest_rules() {
        let img = image_only(image(1280, 720)).unwrap();
        assert_eq!((img.width, img.height, img.screenshot), (1280, 720, true));
        // Password-manager content and text or files first: no image.
        let secret = ClipSnapshot {
            formats: vec![("ExcludeClipboardContentFromMonitorProcessing".into(), None)],
            ..image(10, 10)
        };
        assert_eq!(image_only(secret), None);
        let with_text = ClipSnapshot {
            text: Some("x".into()),
            ..image(10, 10)
        };
        assert_eq!(image_only(with_text), None);
        assert_eq!(image_only(ClipSnapshot::default()), None);
    }

    #[test]
    fn thumbnails_stay_inside_their_budget() {
        let mut h = History::new(MAX_ITEMS);
        // ~75 KB thumbnails: 60 of them pass 4 MB.
        for i in 0..60 {
            ingest(&mut h, image(368 + i, 256), i64::from(i));
        }
        assert!(h.thumb_bytes() <= THUMB_BUDGET);
        assert!(h.len() < 60 && h.len() > 40);
        // The newest survive.
        assert_eq!(h.items()[0].image().unwrap().width, 427);
    }

    /// A packed original of `bytes` (a stand-in: the store never decodes it).
    fn fake_packed(bytes: usize, raw_len: usize) -> Arc<Packed> {
        Arc::new(Packed {
            head: Vec::new(),
            png: vec![0; bytes],
            tail: Vec::new(),
            raw_len,
            layout: PngLayout::Gray { stride: 4, h: 1 },
        })
    }

    fn img_of(h: &History, id: u64) -> &ClipImage {
        h.get(id).unwrap().image().unwrap()
    }

    /// Adds an image of `w` x `rows` and returns its id.
    fn add(h: &mut History, w: u32, rows: u32, at: i64) -> u64 {
        id_of(ingest(h, image(w, rows), at))
    }

    /// Hands out the jobs and answers each with a packed original of
    /// `bytes`.
    fn pack_all(h: &mut History, bytes: usize) {
        for job in h.pack_jobs() {
            h.set_packed(job.id, job.fingerprint, Some(fake_packed(bytes, job.raw.len())));
        }
    }

    #[test]
    fn the_newest_image_keeps_its_original_whatever_its_size() {
        // A 4K or 5K screenshot alone can pass the full-size budget; the
        // newest still pastes at full size ("Salin lagi", its card).
        let mut h = History::new(MAX_ITEMS);
        // 24-bit rows: a little over the whole budget each.
        let big = |w: u32| image(w, (FULL_BUDGET / 3 / w as usize) as u32 + 8);
        let first = id_of(ingest(&mut h, big(5120), 0));
        assert!(img_of(&h, first).raw().is_some(), "alone");
        let second = id_of(ingest(&mut h, big(5100), 1));
        assert!(img_of(&h, second).raw().is_some(), "the newest");
        // The older one is handed out and exempt while it waits.
        assert_eq!(h.pack_jobs().len(), 1);
        assert!(img_of(&h, first).has_original());
        // Failing to pack it, it counts raw and gives way.
        assert!(h.set_packed(first, img_of(&h, first).fingerprint, None));
        assert!(!img_of(&h, first).has_original(), "over 16 MB raw");
        assert!(img_of(&h, second).raw().is_some());
    }

    #[test]
    fn the_newest_image_stays_raw_and_exempt_when_a_text_is_newer() {
        let mut h = History::new(MAX_ITEMS);
        let shot = add(&mut h, 4096, 2048, 0);
        ingest(&mut h, text("after the screenshot"), 1);
        assert_eq!(h.items()[0].plain_text().as_deref(), Some("after the screenshot"));
        // Not index 0, but the newest image: no job, original kept.
        assert!(h.pack_jobs().is_empty());
        assert!(img_of(&h, shot).raw().is_some());
        assert!(img_of(&h, shot).raw().unwrap().len() > FULL_BUDGET);
        ingest(&mut h, text("and another"), 2);
        assert!(img_of(&h, shot).has_original());
    }

    #[test]
    fn pack_jobs_hand_out_the_older_images_once() {
        let mut h = History::new(50);
        let a = add(&mut h, 64, 48, 0);
        let b = add(&mut h, 65, 48, 1);
        let c = add(&mut h, 66, 48, 2);
        assert!(h.pack_jobs().iter().all(|j| j.id != c));
        let _ = (a, b);
        // The second call finds both already out.
        assert!(h.pack_jobs().is_empty());
        assert_eq!(img_of(&h, a).pack, PackState::Pending);
        assert_eq!(img_of(&h, b).pack, PackState::Pending);
        assert_eq!(img_of(&h, c).pack, PackState::Idle);
        // Texts and files never are.
        ingest(&mut h, text("t"), 3);
        assert!(h.pack_jobs().is_empty());
    }

    #[test]
    fn the_jobs_carry_what_the_answer_must_match() {
        let mut h = History::new(50);
        let a = add(&mut h, 64, 48, 0);
        add(&mut h, 65, 48, 1);
        let jobs = h.pack_jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, a);
        assert_eq!(jobs[0].fingerprint, img_of(&h, a).fingerprint);
        assert_eq!(&**jobs[0].raw, &**img_of(&h, a).raw().unwrap());
    }

    #[test]
    fn a_waiting_original_is_exempt_from_the_budget() {
        let mut h = History::new(MAX_ITEMS);
        // 25 MB raw, then a newer image makes it older.
        let big = add(&mut h, 4096, 2048, 0);
        add(&mut h, 64, 48, 1);
        // Before and after it is handed out.
        assert!(img_of(&h, big).has_original(), "about to be handed out");
        h.pack_jobs();
        assert_eq!(img_of(&h, big).pack, PackState::Pending);
        assert!(img_of(&h, big).has_original(), "pending");
        // Another change runs the budget again.
        add(&mut h, 65, 48, 2);
        assert!(img_of(&h, big).has_original());
    }

    #[test]
    fn a_packed_original_counts_its_compressed_bytes_and_the_oldest_lose_first() {
        let mut h = History::new(MAX_ITEMS);
        let ids: Vec<u64> = (0..5).map(|i| add(&mut h, 64 + i, 48, i64::from(i))).collect();
        // Four older ones of 6 MB packed each: 16 MB holds two.
        pack_all(&mut h, 6 * 1024 * 1024);
        let has = |i: usize| img_of(&h, ids[i]).has_original();
        assert!(has(4), "the newest, raw");
        assert!(has(3) && has(2), "12 MB");
        assert!(!has(1) && !has(0), "the oldest lost theirs");
        assert!(img_of(&h, ids[3]).packed().is_some());
        // Small ones all fit.
        let mut h = History::new(MAX_ITEMS);
        for i in 0..20 {
            add(&mut h, 64 + i, 48, i64::from(i));
        }
        pack_all(&mut h, 100 * 1024);
        assert!(h.items().iter().filter_map(Clip::image).all(ClipImage::has_original));
    }

    #[test]
    fn a_failed_pack_counts_its_raw_length() {
        let mut h = History::new(MAX_ITEMS);
        let a = add(&mut h, 4096, 2048, 0); // 25 MB raw
        let b = add(&mut h, 64, 48, 1);
        add(&mut h, 65, 48, 2);
        for job in h.pack_jobs() {
            let fp = job.fingerprint;
            h.set_packed(job.id, fp, if job.id == b { None } else { Some(fake_packed(1, 0)) });
        }
        assert_eq!(img_of(&h, b).pack, PackState::Failed);
        assert!(img_of(&h, b).raw().is_some(), "small and raw, it fits");
        assert!(img_of(&h, a).packed().is_some());
        // A failed 25 MB one does not fit.
        let mut h = History::new(MAX_ITEMS);
        let a = add(&mut h, 4096, 2048, 0);
        add(&mut h, 65, 48, 1);
        let job = h.pack_jobs().remove(0);
        assert!(h.set_packed(a, job.fingerprint, None));
        assert!(!img_of(&h, a).has_original());
    }

    #[test]
    fn an_answer_for_a_gone_changed_or_newest_entry_is_ignored() {
        let mut h = History::new(50);
        let a = add(&mut h, 64, 48, 0);
        let b = add(&mut h, 65, 48, 1);
        let c = add(&mut h, 66, 48, 2);
        let jobs = h.pack_jobs();
        let fp = |id: u64| jobs.iter().find(|j| j.id == id).unwrap().fingerprint;
        let packed = || Some(fake_packed(10, 0));
        // Deleted.
        h.delete(a);
        assert!(!h.set_packed(a, fp(a), packed()));
        // A different fingerprint: another picture under that id.
        assert!(!h.set_packed(b, fp(b) ^ 1, packed()));
        assert!(img_of(&h, b).raw().is_some());
        // It became the newest image while the worker packed it.
        h.delete(c);
        assert!(!h.set_packed(b, fp(b), packed()));
        assert!(img_of(&h, b).raw().is_some(), "stays raw");
        assert_eq!(img_of(&h, b).pack, PackState::Idle);
        // An answer nobody waits for.
        assert!(!h.set_packed(b, fp(b), packed()));
        // And when it is older again it is handed out again.
        add(&mut h, 70, 48, 3);
        assert_eq!(h.pack_jobs().len(), 1);
        assert!(h.set_packed(b, fp(b), packed()));
        assert!(img_of(&h, b).packed().is_some());
    }

    #[test]
    fn a_recopy_brings_the_raw_original_back() {
        let mut h = History::new(50);
        let a = add(&mut h, 64, 48, 0);
        add(&mut h, 65, 48, 1);
        pack_all(&mut h, 10);
        assert!(img_of(&h, a).packed().is_some());
        assert_eq!(ingest(&mut h, image(64, 48), 2), Ingested::Bumped { id: a });
        assert!(img_of(&h, a).raw().is_some());
        assert_eq!(img_of(&h, a).pack, PackState::Idle);
        // The one it pushed down is packed next.
        assert_eq!(h.pack_jobs().len(), 1);
        // The same after it lost its original altogether.
        h.forget_original(a);
        assert!(!img_of(&h, a).has_original());
        ingest(&mut h, text("x"), 3);
        ingest(&mut h, image(64, 48), 4);
        assert!(img_of(&h, a).raw().is_some());
    }

    #[test]
    fn a_paste_keeps_the_decoded_original_raw() {
        let mut h = History::new(50);
        let a = add(&mut h, 64, 48, 0);
        let raw = img_of(&h, a).raw().unwrap().clone();
        add(&mut h, 65, 48, 1);
        pack_all(&mut h, 10);
        assert!(img_of(&h, a).packed().is_some());
        // The decoded DIB must be as long as the original was.
        assert!(!h.set_raw(a, Arc::new(vec![0; 5])));
        assert!(h.set_raw(a, raw.clone()));
        assert_eq!(img_of(&h, a).raw(), Some(&raw));
        assert!(!h.set_raw(999, raw.clone()));
        // Raw already, or a text: nothing to set.
        assert!(!h.set_raw(a, raw.clone()));
        let t = id_of(ingest(&mut h, text("t"), 2));
        assert!(!h.set_raw(t, raw));
    }

    fn id_of(i: Ingested) -> u64 {
        match i {
            Ingested::Added { id, .. } | Ingested::Bumped { id } => id,
            other => panic!("expected a card, got {other:?}"),
        }
    }

    #[test]
    fn a_captures_first_copy_adds_a_card() {
        let mut h = History::new(50);
        let first = id_of(ingest_replacing(&mut h, image(64, 48), 1, None));
        assert_eq!(h.len(), 1);
        assert_eq!(h.items()[0].id, first);
    }

    #[test]
    fn a_second_copy_of_the_same_capture_replaces_its_card() {
        let mut h = History::new(50);
        let first = id_of(ingest_replacing(&mut h, image(64, 48), 1, None));
        ingest(&mut h, text("something else"), 2);
        let second = id_of(ingest_replacing(&mut h, image(65, 48), 3, Some(first)));
        assert_eq!(h.len(), 2, "one card per capture");
        assert_ne!(second, first, "a new id, so no stale thumbnail");
        assert!(h.get(first).is_none());
        assert_eq!(h.items()[0].id, second, "at the front");
        assert_eq!(h.items()[0].image().unwrap().width, 65);
    }

    #[test]
    fn a_pinned_card_stays_and_the_newer_picture_is_a_new_unpinned_card() {
        let mut h = History::new(50);
        let first = id_of(ingest_replacing(&mut h, image(64, 48), 1, None));
        h.set_pinned(first, true);
        let second = id_of(ingest_replacing(&mut h, image(65, 48), 2, Some(first)));
        assert_eq!(h.len(), 2);
        assert!(h.get(first).is_some_and(|c| c.pinned));
        assert_eq!(h.get(first).unwrap().image().unwrap().width, 64);
        let newest = h.get(second).unwrap();
        assert!(!newest.pinned);
        assert_eq!(h.items()[0].id, second);
    }

    #[test]
    fn a_snapshot_that_is_not_kept_leaves_the_old_card() {
        let mut h = History::new(50);
        let first = id_of(ingest_replacing(&mut h, image(64, 48), 1, None));
        // Over the 64 MB cap.
        let big = ClipSnapshot {
            dib: Some(vec![0; MAX_IMAGE_BYTES + 1]),
            only_bitmaps: true,
            ..ClipSnapshot::default()
        };
        assert_eq!(
            ingest_replacing(&mut h, big, 2, Some(first)),
            Ingested::Nothing
        );
        // Password-manager content.
        let secret = ClipSnapshot {
            formats: vec![("Clipboard Viewer Ignore".into(), None)],
            ..image(70, 40)
        };
        assert_eq!(
            ingest_replacing(&mut h, secret, 3, Some(first)),
            Ingested::Skipped
        );
        assert_eq!(h.len(), 1);
        assert_eq!(h.items()[0].id, first);
    }

    #[test]
    fn two_captures_give_two_cards() {
        let mut h = History::new(50);
        let a = id_of(ingest_replacing(&mut h, image(64, 48), 1, None));
        let b = id_of(ingest_replacing(&mut h, image(80, 60), 2, None));
        assert_eq!(h.len(), 2);
        let a2 = id_of(ingest_replacing(&mut h, image(66, 48), 3, Some(a)));
        assert_eq!(h.len(), 2, "the second capture's card is not touched");
        assert!(h.get(b).is_some());
        assert!(h.get(a).is_none());
        assert_eq!(h.items()[0].id, a2);
    }

    #[test]
    fn a_missing_old_card_just_adds() {
        let mut h = History::new(50);
        let gone = id_of(ingest_replacing(&mut h, image(64, 48), 1, None));
        h.delete(gone);
        id_of(ingest_replacing(&mut h, image(65, 48), 2, Some(gone)));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn the_capture_map_keeps_the_newest_sixteen() {
        let mut m = CaptureCards::default();
        assert_eq!(m.card(1), None);
        for capture in 1..=18u64 {
            m.set(capture, capture * 10);
        }
        assert_eq!(m.card(1), None);
        assert_eq!(m.card(2), None);
        assert_eq!(m.card(3), Some(30));
        assert_eq!(m.card(18), Some(180));
        // Setting a capture again replaces its card.
        m.set(3, 99);
        assert_eq!(m.card(3), Some(99));
        m.set(19, 190);
        assert_eq!(m.card(4), None);
        assert_eq!(m.card(3), Some(99), "just refreshed, so not the oldest");
    }

    #[test]
    fn a_card_two_captures_share_is_replaced_by_neither() {
        // Two pixel-identical captures: the second copy bumped the first's
        // card, so both point at it.
        let mut h = History::new(50);
        let mut m = CaptureCards::default();
        let shared = id_of(ingest_replacing(&mut h, image(64, 48), 1, m.card(1)));
        m.set(1, shared);
        let bumped = id_of(ingest_replacing(&mut h, image(64, 48), 2, m.card(2)));
        assert_eq!(bumped, shared, "the dedupe bumped the same card");
        m.set(2, shared);
        assert_eq!(
            m.card(1),
            None,
            "shared: not the first capture's to replace"
        );
        // The first capture's edit is its own card; the shared one stays.
        let a2 = id_of(ingest_replacing(&mut h, image(66, 48), 3, m.card(1)));
        m.set(1, a2);
        assert!(h.get(shared).is_some(), "the second capture's card stays");
        assert_eq!(h.len(), 2);
        // The second capture owns it alone now and may replace it.
        assert_eq!(m.card(2), Some(shared));
    }

    #[test]
    fn previews_are_one_short_line() {
        assert_eq!(preview("a\n  b\tc", 10), "a b c");
        assert_eq!(preview("abcdefghij", 4), "abcd…");
    }
}
