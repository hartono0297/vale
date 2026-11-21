//! Opening a PDF for signing and writing the signed copy with lopdf.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use lopdf::{
    dictionary, Dictionary, Document, IncrementalDocument, LoadOptions, Object, ObjectId,
    Permissions, Stream,
};
use miniz_oxide::deflate::compress_to_vec_zlib;

use crate::sign::geom::{self, PageGeom};
use crate::sign::session::Page;
use crate::sign::{winansi, Item, ItemKind, Mark, Quarter};

/// Ink blue `#1F3A8A` as PDF colour components.
const INK: &str = "0.12 0.23 0.54";

/// Larger files are refused before parsing.
pub const MAX_BYTES: usize = 200 * 1024 * 1024;

pub struct Opened {
    /// The parsed document, decrypted when it was locked.
    pub doc: lopdf::Document,
    /// The file as read; an incremental save appends to these bytes.
    pub bytes: Vec<u8>,
    pub geoms: Vec<PageGeom>,
    /// The source is password-locked; the saved copy keeps the same lock.
    pub encrypted: bool,
    /// The lock's permissions forbid changing or annotating the document.
    pub no_changes: bool,
    /// The source carries a digital signature that edits would invalidate.
    pub signed: bool,
    /// The password that opened it, used to check the saved copy.
    pub password: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    NeedsPassword,
    WrongPassword,
    Broken,
    TooBig,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SaveError {
    /// The source was locked and the lock could not be put back.
    CannotKeepLock,
    Write(String),
}

pub fn open(bytes: Vec<u8>, password: Option<&str>) -> Result<Opened, OpenError> {
    if bytes.len() > MAX_BYTES {
        return Err(OpenError::TooBig);
    }
    // lopdf tries the empty user password first on its own, so a file locked
    // only against changes opens without asking.
    let doc = match load(&bytes, password) {
        Ok(doc) => doc,
        Err(lopdf::Error::InvalidPassword) => {
            // A certificate lock has no password that could open it.
            let standard = Document::load_mem(&bytes).is_ok_and(|d| password_lock(&d));
            return Err(if standard { OpenError::WrongPassword } else { OpenError::Broken });
        }
        Err(_) => return Err(OpenError::Broken),
    };
    if doc.is_encrypted() {
        // Loaded, but nothing could be decrypted.
        if !password_lock(&doc) {
            return Err(OpenError::Broken);
        }
        return Err(match password {
            Some(_) => OpenError::WrongPassword,
            None => OpenError::NeedsPassword,
        });
    }
    let ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    // A page listed twice means a looping page tree; its page numbers are
    // not stable enough to edit.
    let unique: std::collections::BTreeSet<ObjectId> = ids.iter().copied().collect();
    if ids.is_empty() || unique.len() != ids.len() {
        return Err(OpenError::Broken);
    }
    let geoms = ids
        .iter()
        .map(|&id| {
            let (_, _, w, h) = page_box(&doc, id);
            PageGeom { w, h, base: page_rotate(&doc, id) }
        })
        .collect();
    let no_changes = doc.encryption_state.as_ref().is_some_and(|s| {
        let p = s.permissions();
        !p.contains(Permissions::MODIFIABLE) || !p.contains(Permissions::ANNOTABLE)
    });
    Ok(Opened {
        encrypted: doc.encryption_state.is_some(),
        signed: is_signed(&doc),
        no_changes,
        geoms,
        doc,
        bytes,
        password: password.map(str::to_owned),
    })
}

fn load(bytes: &[u8], password: Option<&str>) -> lopdf::Result<Document> {
    match password {
        Some(p) => Document::load_mem_with_options(bytes, LoadOptions::with_password(p)),
        None => Document::load_mem(bytes),
    }
}

/// The file is locked with the standard password handler, not a
/// certificate (`/Adobe.PubSec`) or another handler lopdf cannot open.
fn password_lock(doc: &Document) -> bool {
    doc.get_encrypted()
        .map(|e| e.get(b"Filter").and_then(Object::as_name).is_ok_and(|f| f == b"Standard"))
        .unwrap_or(false)
}

/// Follows references, bounded so a reference loop cannot hang.
fn deref<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Object> {
    doc.dereference(o).ok().map(|(_, o)| o)
}

/// A page attribute, inherited through `/Parent` when the page lacks it.
fn inherited<'a>(doc: &'a Document, page: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    let mut d = page;
    for _ in 0..64 {
        if let Ok(v) = d.get(key) {
            return deref(doc, v);
        }
        let parent = d.get(b"Parent").ok()?.as_reference().ok()?;
        d = doc.get_dictionary(parent).ok()?;
    }
    None
}

/// `(x0, y0, x1, y1)` of a PDF rectangle, normalised; `None` when unusable.
fn rect(doc: &Document, o: &Object) -> Option<(f32, f32, f32, f32)> {
    let a = o.as_array().ok()?;
    if a.len() < 4 {
        return None;
    }
    let mut v = [0f32; 4];
    for (slot, item) in v.iter_mut().zip(a) {
        *slot = deref(doc, item)?.as_float().ok()?;
    }
    let [a0, a1, a2, a3] = v;
    let r = (a0.min(a2), a1.min(a3), a0.max(a2), a1.max(a3));
    let ok = [r.0, r.1, r.2, r.3].iter().all(|x| x.is_finite() && x.abs() < 1e6)
        && r.2 - r.0 >= 1.0
        && r.3 - r.1 >= 1.0;
    ok.then_some(r)
}

/// The visible page box as `(x0, y0, w, h)`: `/CropBox` clipped to
/// `/MediaBox`, else `/MediaBox`, else US Letter.
fn page_box(doc: &Document, id: ObjectId) -> (f32, f32, f32, f32) {
    let found = doc.get_dictionary(id).ok().and_then(|page| {
        let media = inherited(doc, page, b"MediaBox").and_then(|o| rect(doc, o));
        let crop = inherited(doc, page, b"CropBox").and_then(|o| rect(doc, o));
        match (crop, media) {
            (Some(c), Some(m)) => {
                let r = (c.0.max(m.0), c.1.max(m.1), c.2.min(m.2), c.3.min(m.3));
                Some(if r.2 - r.0 >= 1.0 && r.3 - r.1 >= 1.0 { r } else { m })
            }
            (c, m) => c.or(m),
        }
    });
    let (x0, y0, x1, y1) = found.unwrap_or((0.0, 0.0, 612.0, 792.0));
    (x0, y0, x1 - x0, y1 - y0)
}

fn page_rotate(doc: &Document, id: ObjectId) -> Quarter {
    let deg = doc
        .get_dictionary(id)
        .ok()
        .and_then(|page| inherited(doc, page, b"Rotate"))
        .and_then(|o| o.as_float().ok())
        .filter(|d| d.is_finite())
        .map(|d| d.round() as i64)
        .unwrap_or(0);
    Quarter::from_degrees(deg)
}

/// An existing digital signature: `/AcroForm /SigFlags` bit 1, or a filled
/// signature field anywhere.
fn is_signed(doc: &Document) -> bool {
    let flags = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .and_then(|o| deref(doc, o))
        .and_then(|o| o.as_dict().ok())
        .and_then(|a| a.get(b"SigFlags").ok())
        .and_then(|o| deref(doc, o))
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0);
    flags & 1 != 0
        || doc.objects.values().any(|o| {
            o.as_dict().is_ok_and(|d| {
                d.has(b"V") && d.get(b"FT").and_then(Object::as_name).is_ok_and(|n| n == b"Sig")
            })
        })
}

pub fn save(o: &Opened, pages: &[Page], items: &[Item]) -> Result<Vec<u8>, SaveError> {
    let ids: Vec<ObjectId> = o.doc.get_pages().into_values().collect();
    let deleted = |i: usize| pages.get(i).is_some_and(|p| p.deleted);
    let live = (0..ids.len()).filter(|&i| !deleted(i)).count();
    if live == 0 {
        return Err(SaveError::Write("no page left".into()));
    }
    let out = if save_is_incremental(o, pages) {
        save_incremental(o, &ids, pages, items)?
    } else {
        save_full(o, &ids, pages, items)?
    };
    check_output(&out, o.password.as_deref(), live, o.encrypted)?;
    Ok(out)
}

/// Whether `save` appends a revision (keeping an existing digital signature
/// valid) instead of rewriting the file: no page deleted, an intact
/// cross-reference chain to point back to and, for a locked file, the
/// original /Encrypt object.
pub fn save_is_incremental(o: &Opened, pages: &[Page]) -> bool {
    !pages.iter().take(o.geoms.len()).any(|p| p.deleted)
        && o.doc.xref_recovery.is_none()
        && o.doc.xref_start != 0
        && o.doc.encryption_state.as_ref().is_none_or(|s| s.encrypt_object_id().is_some())
}

/// The saved file reads back with the same password, still locked when the
/// source was, with the expected number of pages.
fn check_output(
    out: &[u8],
    password: Option<&str>,
    live: usize,
    locked: bool,
) -> Result<(), SaveError> {
    match load(out, password) {
        Ok(d) if d.is_encrypted() => Err(SaveError::Write("the saved file does not open".into())),
        Ok(d) if locked && d.encryption_state.is_none() => Err(SaveError::CannotKeepLock),
        Ok(d) if d.get_pages().len() == live => Ok(()),
        Ok(_) => Err(SaveError::Write("the saved file has the wrong pages".into())),
        Err(e) => Err(SaveError::Write(e.to_string())),
    }
}

/// What one page needs: the extra rotation and the items on it.
struct Edit<'a> {
    id: ObjectId,
    extra: Quarter,
    items: Vec<&'a Item>,
}

fn edits<'a>(ids: &[ObjectId], pages: &[Page], items: &'a [Item]) -> Vec<Edit<'a>> {
    ids.iter()
        .enumerate()
        .filter(|&(i, _)| !pages.get(i).is_some_and(|p| p.deleted))
        .map(|(i, &id)| Edit {
            id,
            extra: pages.get(i).map_or(Quarter::R0, |p| p.extra),
            items: items.iter().filter(|it| it.page == i && drawable(it)).collect(),
        })
        .filter(|e| e.extra != Quarter::R0 || !e.items.is_empty())
        .collect()
}

fn drawable(it: &Item) -> bool {
    [it.x, it.y, it.w, it.h].iter().all(|v| v.is_finite()) && it.w > 0.0 && it.h > 0.0
}

/// Appends a revision to the original bytes; the source's lock carries over.
fn save_incremental(
    o: &Opened,
    ids: &[ObjectId],
    pages: &[Page],
    items: &[Item],
) -> Result<Vec<u8>, SaveError> {
    let mut inc = IncrementalDocument::create_from(o.bytes.clone(), o.doc.clone());
    let (mut shared, mut scans) = (Shared::default(), Scans::new());
    for e in edits(ids, pages, items) {
        let read = read_page(inc.get_prev_documents(), e.id, &mut scans)?;
        // The changed page goes into the new revision under its old id.
        let dict = write_page(&mut inc.new_document, read, &e, &mut shared)?;
        inc.new_document.set_object(e.id, dict);
    }
    let mut out = Vec::new();
    inc.save_to(&mut out).map_err(write_err)?;
    Ok(out)
}

/// Rewrites the whole file so deleted pages are really gone, then puts the
/// source's lock back.
fn save_full(
    o: &Opened,
    ids: &[ObjectId],
    pages: &[Page],
    items: &[Item],
) -> Result<Vec<u8>, SaveError> {
    let mut doc = o.doc.clone();
    let state = doc.encryption_state.take();
    let (mut shared, mut scans) = (Shared::default(), Scans::new());
    for e in edits(ids, pages, items) {
        let read = read_page(&doc, e.id, &mut scans)?;
        let dict = write_page(&mut doc, read, &e, &mut shared)?;
        doc.set_object(e.id, dict);
    }
    let gone: Vec<usize> =
        (0..ids.len()).filter(|&i| pages.get(i).is_some_and(|p| p.deleted)).collect();
    if !gone.is_empty() {
        let dead: BTreeSet<ObjectId> = gone.iter().filter_map(|&i| ids.get(i).copied()).collect();
        let numbers: Vec<u32> = gone.iter().filter_map(|&i| u32::try_from(i + 1).ok()).collect();
        // Links into the deleted pages go first, while their references
        // still say where they point.
        drop_links_to(&mut doc, &dead);
        drop_annotations_of(&mut doc, &dead);
        doc.delete_pages(&numbers);
        own_resources(&mut doc, &mut scans);
        // Tags can quote the deleted pages' text (/ActualText, /Alt).
        if let Ok(catalog) = doc.catalog_mut() {
            catalog.remove(b"StructTreeRoot");
            catalog.remove(b"MarkInfo");
        }
    }
    // Drops everything only the deleted pages used, their content included.
    doc.prune_objects();
    if let Some(state) = state {
        doc.encrypt(&state).map_err(|_| SaveError::CannotKeepLock)?;
    }
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(write_err)?;
    Ok(out)
}

/// A destination or GoTo action that lands on a deleted page; `names` are
/// the named destinations already removed for pointing there.
fn goes_to(
    doc: &Document,
    o: &Object,
    dead: &BTreeSet<ObjectId>,
    names: &BTreeSet<Vec<u8>>,
    depth: u8,
) -> bool {
    if depth > 8 {
        return false;
    }
    match deref(doc, o) {
        Some(Object::Array(a)) => {
            a.first().and_then(|f| f.as_reference().ok()).is_some_and(|r| dead.contains(&r))
        }
        Some(Object::Name(n)) => names.contains(n),
        Some(Object::String(s, _)) => names.contains(s),
        Some(Object::Dictionary(d)) => {
            let goto = d.get(b"S").and_then(Object::as_name).map_or(true, |s| s == b"GoTo");
            goto && d.get(b"D").is_ok_and(|x| goes_to(doc, x, dead, names, depth + 1))
        }
        _ => false,
    }
}

/// /Dest or /A of an outline item or a link lands on a deleted page.
fn entry_goes_to(
    doc: &Document,
    d: &Dictionary,
    dead: &BTreeSet<ObjectId>,
    names: &BTreeSet<Vec<u8>>,
) -> bool {
    [b"Dest".as_slice(), b"A"]
        .iter()
        .any(|k| d.get(k).is_ok_and(|x| goes_to(doc, x, dead, names, 0)))
}

/// Removes what points into the deleted pages: named destinations,
/// /OpenAction, outline items (their children move up a level) and link
/// annotations on the pages that stay.
fn drop_links_to(doc: &mut Document, dead: &BTreeSet<ObjectId>) {
    let names = drop_named_dests(doc, dead);
    let open = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"OpenAction").ok())
        .is_some_and(|o| goes_to(doc, o, dead, &names, 0));
    if open && let Ok(c) = doc.catalog_mut() {
        c.remove(b"OpenAction");
    }
    drop_outline_items(doc, dead, &names);
    let live: Vec<ObjectId> =
        doc.get_pages().into_values().filter(|id| !dead.contains(id)).collect();
    for p in live {
        let list = doc
            .get_dictionary(p)
            .ok()
            .and_then(|d| d.get(b"Annots").ok())
            .and_then(|o| deref(doc, o))
            .and_then(|o| o.as_array().ok())
            .cloned();
        let Some(list) = list else { continue };
        let kept: Vec<Object> = list
            .iter()
            .filter(|a| {
                !deref(doc, a)
                    .and_then(|o| o.as_dict().ok())
                    .is_some_and(|d| entry_goes_to(doc, d, dead, &names))
            })
            .cloned()
            .collect();
        if kept.len() != list.len()
            && let Ok(d) = doc.get_dictionary_mut(p)
        {
            d.set("Annots", kept);
        }
    }
}

/// A catalog entry that is a dictionary, with its object id when indirect.
fn catalog_dict(doc: &Document, key: &[u8]) -> Option<(Option<ObjectId>, Dictionary)> {
    let o = doc.catalog().ok()?.get(key).ok()?;
    let d = deref(doc, o)?.as_dict().ok()?.clone();
    Some((o.as_reference().ok(), d))
}

fn put_catalog_dict(doc: &mut Document, key: &[u8], at: Option<ObjectId>, d: Dictionary) {
    match at {
        Some(id) => doc.set_object(id, d),
        None => {
            if let Ok(c) = doc.catalog_mut() {
                c.set(key.to_vec(), d);
            }
        }
    }
}

/// Drops named destinations on deleted pages (catalog /Dests and the
/// /Names /Dests tree) and returns their names.
fn drop_named_dests(doc: &mut Document, dead: &BTreeSet<ObjectId>) -> BTreeSet<Vec<u8>> {
    let mut removed = BTreeSet::new();
    let none = BTreeSet::new();
    if let Some((at, mut dests)) = catalog_dict(doc, b"Dests") {
        let gone: Vec<Vec<u8>> = dests
            .iter()
            .filter(|(_, v)| goes_to(doc, v, dead, &none, 0))
            .map(|(k, _)| k.clone())
            .collect();
        if !gone.is_empty() {
            for k in &gone {
                dests.remove(k);
            }
            removed.extend(gone);
            put_catalog_dict(doc, b"Dests", at, dests);
        }
    }
    if let Some((at, mut names)) = catalog_dict(doc, b"Names")
        && let Ok(tree) = names.get(b"Dests").cloned()
    {
        let mut seen = BTreeSet::new();
        if let Some(direct) = scrub_name_tree(doc, &tree, dead, &mut removed, &mut seen, 0) {
            names.set("Dests", direct);
            put_catalog_dict(doc, b"Names", at, names);
        }
    }
    removed
}

/// Removes name-tree pairs whose destination is on a deleted page. An
/// indirect node is rewritten in place; a direct one is returned for the
/// caller to store.
fn scrub_name_tree(
    doc: &mut Document,
    node: &Object,
    dead: &BTreeSet<ObjectId>,
    removed: &mut BTreeSet<Vec<u8>>,
    seen: &mut BTreeSet<ObjectId>,
    depth: u32,
) -> Option<Dictionary> {
    let id = node.as_reference().ok();
    if depth > 32 || id.is_some_and(|id| !seen.insert(id)) {
        return None;
    }
    let mut d = deref(doc, node)?.as_dict().ok()?.clone();
    let kids = d
        .get(b"Kids")
        .ok()
        .and_then(|k| deref(doc, k))
        .and_then(|k| k.as_array().ok())
        .cloned();
    if let Some(kids) = kids {
        let mut new_kids = Vec::with_capacity(kids.len());
        for k in kids {
            match scrub_name_tree(doc, &k, dead, removed, seen, depth + 1) {
                Some(direct) if k.as_reference().is_err() => {
                    new_kids.push(Object::Dictionary(direct))
                }
                _ => new_kids.push(k),
            }
        }
        d.set("Kids", new_kids);
    }
    let pairs = d
        .get(b"Names")
        .ok()
        .and_then(|n| deref(doc, n))
        .and_then(|n| n.as_array().ok())
        .cloned();
    if let Some(pairs) = pairs {
        let none = BTreeSet::new();
        let mut kept = Vec::with_capacity(pairs.len());
        for pair in pairs.chunks(2) {
            match pair {
                [k, v] if goes_to(doc, v, dead, &none, 0) => {
                    if let Ok(name) = k.as_str() {
                        removed.insert(name.to_vec());
                    }
                }
                _ => kept.extend(pair.iter().cloned()),
            }
        }
        d.set("Names", kept);
    }
    match id {
        Some(id) => {
            doc.set_object(id, d);
            None
        }
        None => Some(d),
    }
}

/// Removes outline items that land on a deleted page; their children take
/// their place.
fn drop_outline_items(doc: &mut Document, dead: &BTreeSet<ObjectId>, names: &BTreeSet<Vec<u8>>) {
    let root = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Outlines").ok())
        .and_then(|o| o.as_reference().ok());
    let Some(root) = root else { return };
    let mut seen = BTreeSet::from([root]);
    let (kids, visible) = outline_level(doc, root, dead, names, &mut seen, 0);
    relink(doc, root, &kids, visible, true);
}

/// The items to keep under `parent` and how many lines they show.
fn outline_level(
    doc: &mut Document,
    parent: ObjectId,
    dead: &BTreeSet<ObjectId>,
    names: &BTreeSet<Vec<u8>>,
    seen: &mut BTreeSet<ObjectId>,
    depth: u32,
) -> (Vec<ObjectId>, i64) {
    let (mut out, mut visible) = (Vec::new(), 0i64);
    let mut cur = doc
        .get_dictionary(parent)
        .ok()
        .and_then(|d| d.get(b"First").ok())
        .and_then(|o| o.as_reference().ok());
    while let Some(id) = cur {
        if !seen.insert(id) {
            break;
        }
        let Ok(d) = doc.get_dictionary(id) else { break };
        cur = d.get(b"Next").and_then(Object::as_reference).ok();
        let gone = entry_goes_to(doc, d, dead, names);
        let open = d.get(b"Count").and_then(Object::as_i64).map_or(true, |c| c >= 0);
        if depth > 64 {
            // Too deep to rebuild: kept as it is.
            out.push(id);
            visible += 1;
            continue;
        }
        let (kids, shown) = outline_level(doc, id, dead, names, seen, depth + 1);
        if gone {
            out.extend(kids);
            visible += shown;
        } else {
            relink(doc, id, &kids, shown, open);
            out.push(id);
            visible += 1 + if open { shown } else { 0 };
        }
    }
    (out, visible)
}

fn relink(doc: &mut Document, parent: ObjectId, kids: &[ObjectId], visible: i64, open: bool) {
    for (i, &k) in kids.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|j| kids.get(j)).copied();
        let next = kids.get(i + 1).copied();
        if let Ok(d) = doc.get_dictionary_mut(k) {
            d.set("Parent", Object::Reference(parent));
            match prev {
                Some(p) => d.set("Prev", Object::Reference(p)),
                None => {
                    d.remove(b"Prev");
                }
            }
            match next {
                Some(n) => d.set("Next", Object::Reference(n)),
                None => {
                    d.remove(b"Next");
                }
            }
        }
    }
    if let Ok(d) = doc.get_dictionary_mut(parent) {
        match (kids.first(), kids.last()) {
            (Some(&first), Some(&last)) => {
                d.set("First", Object::Reference(first));
                d.set("Last", Object::Reference(last));
                d.set("Count", if open { visible } else { -visible });
            }
            _ => {
                d.remove(b"First");
                d.remove(b"Last");
                d.remove(b"Count");
            }
        }
    }
}

/// Annotation ids a page lists in /Annots.
fn annots_of(doc: &Document, page: ObjectId) -> Vec<ObjectId> {
    doc.get_dictionary(page)
        .ok()
        .and_then(|d| d.get(b"Annots").ok())
        .and_then(|o| deref(doc, o))
        .and_then(|o| o.as_array().ok())
        .into_iter()
        .flatten()
        .filter_map(|o| o.as_reference().ok())
        .collect()
}

/// Removes the deleted pages' annotations everywhere they are listed (form
/// /Fields, parents' /Kids, /CO), then form fields left without widgets, so
/// their values and appearances go too. An annotation a remaining page also
/// lists stays.
fn drop_annotations_of(doc: &mut Document, dead: &BTreeSet<ObjectId>) {
    let mut annots: BTreeSet<ObjectId> = dead.iter().flat_map(|&p| annots_of(doc, p)).collect();
    // Widgets that name a deleted page without being in its /Annots.
    for (&id, o) in &doc.objects {
        let on_dead = o.as_dict().is_ok_and(|d| {
            d.has(b"Rect")
                && d.get(b"P").and_then(Object::as_reference).is_ok_and(|p| dead.contains(&p))
        });
        if on_dead {
            annots.insert(id);
        }
    }
    for page in doc.get_pages().into_values().filter(|p| !dead.contains(p)) {
        for a in annots_of(doc, page) {
            annots.remove(&a);
        }
    }
    let mut parents: Vec<ObjectId> = annots
        .iter()
        .filter_map(|&a| doc.get_dictionary(a).ok()?.get(b"Parent").ok()?.as_reference().ok())
        .collect();
    for &a in &annots {
        doc.delete_object(a);
    }
    let mut budget = 100_000usize;
    while let Some(p) = parents.pop() {
        budget = budget.saturating_sub(1);
        if budget == 0 {
            break;
        }
        let (empty, up) = match doc.get_dictionary(p) {
            Ok(d) => (
                d.get(b"Kids")
                    .ok()
                    .and_then(|k| deref(doc, k))
                    .and_then(|k| k.as_array().ok())
                    .is_some_and(|k| k.is_empty()),
                d.get(b"Parent").and_then(Object::as_reference).ok(),
            ),
            Err(_) => continue,
        };
        if empty {
            parents.extend(up);
            doc.delete_object(p);
        }
    }
}

/// Gives every page its own /Resources holding only the XObjects its
/// content draws, and drops the page tree's shared ones, so an image only a
/// deleted page drew is no longer reachable.
fn own_resources(doc: &mut Document, scans: &mut Scans) {
    let ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for id in ids {
        let Ok(page) = doc.get_dictionary(id) else { continue };
        let mut res = inherited(doc, page, b"Resources")
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default();
        let xobjects = res
            .get(b"XObject")
            .ok()
            .and_then(|o| deref(doc, o))
            .and_then(|o| o.as_dict().ok())
            .cloned();
        // Content that cannot be decoded keeps every XObject.
        if let Some(mut xobjects) = xobjects
            && let Some(used) = scans.drawn(doc, id, &xobjects)
        {
            let unused: Vec<Vec<u8>> =
                xobjects.iter().map(|(k, _)| k.clone()).filter(|k| !used.contains(k)).collect();
            for k in unused {
                xobjects.remove(&k);
            }
            res.set("XObject", xobjects);
        }
        if let Ok(page) = doc.get_dictionary_mut(id) {
            page.set("Resources", res);
        }
    }
    for o in doc.objects.values_mut() {
        if let Object::Dictionary(d) = o
            && d.has_type(b"Pages")
        {
            d.remove(b"Resources");
        }
    }
}

/// Most decoded content one page may cost.
const PAGE_BUDGET: usize = 64 << 20;
/// Most decoded content one save may cost, all pages together.
const SAVE_BUDGET: usize = 256 << 20;

/// What one content stream does, so pages sharing it decode it once.
#[derive(Clone, Default, Debug, PartialEq)]
struct StreamScan {
    /// `Q` with no `q` of its own in this stream (they close earlier ones).
    pops: usize,
    /// `q` this stream leaves open.
    pushes: usize,
    /// Names drawn with `Do`.
    used: BTreeSet<Vec<u8>>,
    /// The first operator is a `Do` whose name ended the previous stream.
    leading_do: bool,
    has_op: bool,
    /// A name after the last operator, for a `Do` in the next stream.
    trailing: Option<Vec<u8>>,
}

/// One page's content as a whole; `drawn` is `None` when it could not all be
/// decoded within budget.
struct PageScan {
    unclosed: usize,
    drawn: Option<BTreeSet<Vec<u8>>>,
}

/// Decodes content streams for one save under a total budget, caching each
/// stream's result.
struct Scans {
    cache: BTreeMap<ObjectId, Option<StreamScan>>,
    left: usize,
    /// Bytes decoded so far.
    decoded: usize,
}

impl Scans {
    fn new() -> Scans {
        Scans { cache: BTreeMap::new(), left: SAVE_BUDGET, decoded: 0 }
    }

    /// Decodes `s` within what the page and the save have left.
    fn decode(&mut self, s: &Stream, page_left: &mut usize) -> Option<Vec<u8>> {
        let data = s.decompressed_content_with_limit((*page_left).min(self.left)).ok()?;
        *page_left = page_left.saturating_sub(data.len());
        self.left = self.left.saturating_sub(data.len());
        self.decoded += data.len();
        Some(data)
    }

    fn stream(&mut self, doc: &Document, sid: ObjectId, page_left: &mut usize) -> Option<StreamScan> {
        if let Some(hit) = self.cache.get(&sid) {
            return hit.clone();
        }
        let scan = doc
            .get_object(sid)
            .and_then(Object::as_stream)
            .ok()
            .and_then(|s| self.decode(s, page_left))
            .and_then(|data| scan_stream(&data));
        self.cache.insert(sid, scan.clone());
        scan
    }

    fn page(&mut self, doc: &Document, id: ObjectId) -> PageScan {
        let undecodable = PageScan { unclosed: 0, drawn: None };
        let mut page_left = PAGE_BUDGET;
        let (mut depth, mut carried, mut used) = (0usize, None::<Vec<u8>>, BTreeSet::new());
        for sid in doc.get_page_contents(id) {
            // A missing content object draws nothing.
            if doc.get_object(sid).and_then(Object::as_stream).is_err() {
                continue;
            }
            let Some(s) = self.stream(doc, sid, &mut page_left) else { return undecodable };
            depth = depth.saturating_sub(s.pops) + s.pushes;
            if s.leading_do
                && let Some(n) = carried.take()
            {
                used.insert(n);
            }
            used.extend(s.used);
            carried = if s.has_op { s.trailing } else { s.trailing.or(carried) };
        }
        PageScan { unclosed: depth, drawn: Some(used) }
    }

    /// XObject names the page draws, and those drawn by the Form XObjects it
    /// draws that use the page's resources (no /Resources of their own);
    /// `None` when any of that cannot be decoded.
    fn drawn(
        &mut self,
        doc: &Document,
        id: ObjectId,
        xobjects: &Dictionary,
    ) -> Option<BTreeSet<Vec<u8>>> {
        let mut used = self.page(doc, id).drawn?;
        let mut page_left = PAGE_BUDGET;
        let mut todo: Vec<Vec<u8>> = used.iter().cloned().collect();
        let mut seen = BTreeSet::new();
        while let Some(name) = todo.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(Object::Reference(fid)) = xobjects.get(&name).ok() else { continue };
            let Ok(form) = doc.get_object(*fid).and_then(Object::as_stream) else { continue };
            let is_form =
                form.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|s| s == b"Form");
            if !is_form || form.dict.has(b"Resources") {
                continue;
            }
            for n in self.stream(doc, *fid, &mut page_left)?.used {
                if used.insert(n.clone()) {
                    todo.push(n);
                }
            }
        }
        Some(used)
    }
}

/// Scans one decoded content stream; `None` when inline image data has no
/// end, so later operators cannot be found.
fn scan_stream(c: &[u8]) -> Option<StreamScan> {
    let mut s = StreamScan::default();
    let (mut depth, mut last) = (0usize, None::<Vec<u8>>);
    let complete = scan_content(c, |t| match t {
        Tok::Name(n) => last = Some(n),
        Tok::Op(op) => {
            match op {
                b"q" => depth += 1,
                b"Q" if depth > 0 => depth -= 1,
                b"Q" => s.pops += 1,
                b"Do" => match last.take() {
                    Some(n) => {
                        s.used.insert(n);
                    }
                    None if !s.has_op => s.leading_do = true,
                    None => {}
                },
                _ => {}
            }
            s.has_op = true;
            last = None;
        }
    });
    s.pushes = depth;
    s.trailing = last;
    complete.then_some(s)
}

enum Tok<'a> {
    Name(Vec<u8>),
    Op(&'a [u8]),
}

fn white(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0C | 0)
}

fn delim(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

/// Walks a content stream, calling `on` with each name and each operator
/// (numbers included); strings, comments and inline image data are
/// skipped. False when inline image data runs to the end without `EI`.
fn scan_content<'a>(c: &'a [u8], mut on: impl FnMut(Tok<'a>)) -> bool {
    let at = |i: usize| c.get(i).copied();
    let word_end = |mut i: usize| {
        while at(i).is_some_and(|x| !white(x) && !delim(x)) {
            i += 1;
        }
        i
    };
    let mut i = 0;
    while let Some(b) = at(i) {
        match b {
            _ if white(b) => i += 1,
            b'%' => {
                while at(i).is_some_and(|x| x != b'\n' && x != b'\r') {
                    i += 1;
                }
            }
            b'(' => {
                let mut depth = 0usize;
                while let Some(x) = at(i) {
                    i += 1;
                    match x {
                        b'\\' => i += 1,
                        b'(' => depth += 1,
                        b')' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            b'<' if at(i + 1) == Some(b'<') => i += 2,
            b'<' => {
                while at(i).is_some_and(|x| x != b'>') {
                    i += 1;
                }
                i += 1;
            }
            b'/' => {
                let end = word_end(i + 1);
                on(Tok::Name(decode_name(c.get(i + 1..end).unwrap_or_default())));
                i = end;
            }
            _ if delim(b) => i += 1,
            _ => {
                let end = word_end(i);
                let word = c.get(i..end).unwrap_or_default();
                i = end;
                if word == b"ID" {
                    // Inline image data runs to an "EI" between white space.
                    let mut closed = false;
                    while at(i).is_some() {
                        let ei = at(i).is_some_and(white)
                            && at(i + 1) == Some(b'E')
                            && at(i + 2) == Some(b'I')
                            && at(i + 3).is_none_or(|y| white(y) || delim(y));
                        i += 1;
                        if ei {
                            i += 2;
                            closed = true;
                            break;
                        }
                    }
                    if !closed {
                        return false;
                    }
                } else {
                    on(Tok::Op(word));
                }
            }
        }
    }
    true
}

/// A name token with its `#xx` escapes decoded.
fn decode_name(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while let Some(&b) = raw.get(i) {
        let hex = raw
            .get(i + 1..i + 3)
            .filter(|h| h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b, hex) {
            (b'#', Some(v)) => {
                out.push(v);
                i += 3;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn write_err(e: impl std::fmt::Display) -> SaveError {
    SaveError::Write(e.to_string())
}

/// Objects written once and used by every page.
#[derive(Default)]
struct Shared {
    font: Option<ObjectId>,
    open_q: Option<ObjectId>,
    images: Vec<(*const Mark, ObjectId)>,
}

/// A page as it stands, with the resource dictionaries we add to resolved.
struct PageRead {
    dict: Dictionary,
    resources: Dictionary,
    xobjects: Dictionary,
    fonts: Dictionary,
    contents: Vec<ObjectId>,
    /// `q` the page's own content leaves open.
    unclosed: usize,
    origin: (f32, f32),
    base: Quarter,
}

fn read_page(doc: &Document, id: ObjectId, scans: &mut Scans) -> Result<PageRead, SaveError> {
    let dict = doc.get_dictionary(id).map_err(write_err)?.clone();
    let resources = inherited(doc, &dict, b"Resources")
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    let sub = |key: &[u8]| {
        resources
            .get(key)
            .ok()
            .and_then(|o| deref(doc, o))
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default()
    };
    let (xobjects, fonts) = (sub(b"XObject"), sub(b"Font"));
    let (x0, y0, _, _) = page_box(doc, id);
    Ok(PageRead {
        contents: doc.get_page_contents(id),
        unclosed: scans.page(doc, id).unclosed,
        base: page_rotate(doc, id),
        origin: (x0, y0),
        dict,
        resources,
        xobjects,
        fonts,
    })
}

/// A resource name not in `dict` yet: `stem` (when `bare`) or `stem0`,
/// `stem1`, ... in turn.
fn free_name(dict: &Dictionary, stem: &str, bare: bool) -> String {
    if bare && !dict.has(stem.as_bytes()) {
        return stem.to_owned();
    }
    let first = u32::from(bare);
    (first..)
        .map(|n| format!("{stem}{n}"))
        .find(|n| !dict.has(n.as_bytes()))
        .unwrap_or_else(|| stem.to_owned())
}

/// Adds the new objects to `dst` and returns the page dictionary to store.
fn write_page(
    dst: &mut Document,
    mut r: PageRead,
    e: &Edit,
    sh: &mut Shared,
) -> Result<Dictionary, SaveError> {
    if e.extra != Quarter::R0 {
        r.dict.set("Rotate", r.base.add(e.extra).degrees());
    }
    if e.items.is_empty() {
        return Ok(r.dict);
    }
    // Close what the page's own content left open and our leading "q",
    // then start again from the page's initial state.
    let mut c: Vec<u8> = b"\n".to_vec();
    for _ in 0..=r.unclosed {
        c.extend(b"Q\n");
    }
    c.extend(b"q\n");
    if r.origin != (0.0, 0.0) {
        c.extend(format!("1 0 0 1 {} {} cm\n", num(r.origin.0), num(r.origin.1)).bytes());
    }
    let mut named: Vec<(ObjectId, String)> = Vec::new();
    let mut font_name: Option<String> = None;
    for it in &e.items {
        let m = geom::item_matrix(it);
        c.extend(format!("q {} cm\n", m.map(num).join(" ")).bytes());
        match &it.kind {
            ItemKind::Signature(mark) | ItemKind::Initials(mark) => {
                let img = image(dst, mark, sh)?;
                let name = match named.iter().find(|(id, _)| *id == img) {
                    Some((_, n)) => n.clone(),
                    None => {
                        let n = free_name(&r.xobjects, "ValeS", false);
                        r.xobjects.set(n.as_bytes().to_vec(), Object::Reference(img));
                        named.push((img, n.clone()));
                        n
                    }
                };
                c.extend(format!("/{name} Do\n").bytes());
            }
            ItemKind::Date(s, _) | ItemKind::Text(s, _) => {
                let name = match &font_name {
                    Some(n) => n.clone(),
                    None => {
                        let font = match sh.font {
                            Some(f) => f,
                            None => {
                                let f = add(dst, dictionary! {"Type"=>"Font","Subtype"=>"Type1",
                                    "BaseFont"=>"Helvetica","Encoding"=>"WinAnsiEncoding"})?;
                                sh.font = Some(f);
                                f
                            }
                        };
                        let n = free_name(&r.fonts, "ValeF", true);
                        r.fonts.set(n.as_bytes().to_vec(), Object::Reference(font));
                        font_name = Some(n.clone());
                        n
                    }
                };
                // Font size h * 0.8 in points; the unit box is w by h, so
                // the text matrix undoes the horizontal stretch. Ink blue,
                // like text drawn on images.
                let sx = 0.8 * it.h / it.w;
                let text: Vec<u8> = s.chars().map(|ch| winansi::code(ch).unwrap_or(b'?')).collect();
                c.extend(format!("BT /{name} 1 Tf {INK} rg {} 0 0 0.8 0 0.2 Tm ", num(sx)).bytes());
                c.extend(winansi::pdf_string(&text));
                c.extend(b" Tj ET\n");
            }
            ItemKind::Check(_) => {
                let tick = "0.12 w 1 J 1 j 0.18 0.52 m 0.42 0.26 l 0.84 0.8 l S";
                c.extend(format!("{INK} RG {tick}\n").bytes());
            }
        }
        c.extend(b"Q\n");
    }
    c.extend(b"Q\n");
    let open_q = match sh.open_q {
        Some(q) => q,
        None => {
            let q = add(dst, Stream::new(Dictionary::new(), b"q\n".to_vec()))?;
            sh.open_q = Some(q);
            q
        }
    };
    let body = add(dst, Stream::new(Dictionary::new(), c))?;
    let mut contents = vec![Object::Reference(open_q)];
    contents.extend(r.contents.iter().map(|&id| Object::Reference(id)));
    contents.push(Object::Reference(body));
    if !named.is_empty() {
        r.resources.set("XObject", r.xobjects);
    }
    if font_name.is_some() {
        r.resources.set("Font", r.fonts);
    }
    r.dict.set("Resources", r.resources);
    r.dict.set("Contents", contents);
    Ok(r.dict)
}

fn add(dst: &mut Document, o: impl Into<Object>) -> Result<ObjectId, SaveError> {
    dst.try_add_object(o).map_err(write_err)
}

/// The image XObject for a mark: RGB with the alpha as a soft mask, written
/// once per mark however many times it is placed.
fn image(dst: &mut Document, mark: &Arc<Mark>, sh: &mut Shared) -> Result<ObjectId, SaveError> {
    let key = Arc::as_ptr(mark);
    if let Some(&(_, id)) = sh.images.iter().find(|(k, _)| *k == key) {
        return Ok(id);
    }
    let n = (mark.w as usize).checked_mul(mark.h as usize).unwrap_or(0);
    if n == 0 || n.checked_mul(4) != Some(mark.rgba.len()) {
        return Err(SaveError::Write("bad signature image".into()));
    }
    let mut rgb = Vec::with_capacity(n * 3);
    let mut alpha = Vec::with_capacity(n);
    for px in mark.rgba.chunks_exact(4) {
        if let &[r, g, b, a] = px {
            rgb.extend_from_slice(&[r, g, b]);
            alpha.push(a);
        }
    }
    let (w, h) = (i64::from(mark.w), i64::from(mark.h));
    let smask = add(
        dst,
        Stream::new(
            dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>w,"Height"=>h,
            "ColorSpace"=>"DeviceGray","BitsPerComponent"=>8,"Filter"=>"FlateDecode"},
            compress_to_vec_zlib(&alpha, 6),
        ),
    )?;
    let img = add(
        dst,
        Stream::new(
            dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>w,"Height"=>h,
            "ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8,"SMask"=>smask,"Filter"=>"FlateDecode"},
            compress_to_vec_zlib(&rgb, 6),
        ),
    )?;
    sh.images.push((key, img));
    Ok(img)
}

/// A PDF number: at most four decimals, never an exponent.
fn num(v: f32) -> String {
    let v = if v.is_finite() { f64::from(v).clamp(-1e7, 1e7) } else { 0.0 };
    let r = (v * 10_000.0).round() / 10_000.0;
    if r == 0.0 {
        return "0".into();
    }
    let s = format!("{r:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::geom::item_matrix;
    use crate::sign::{ItemKind, Mark, Quarter};
    use lopdf::encryption::crypt_filters::{Aes128CryptFilter, Aes256CryptFilter, CryptFilter};
    use lopdf::{
        dictionary, Dictionary, Document, EncryptionState, EncryptionVersion, LoadOptions, Object,
        ObjectId, Permissions, Stream,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;

    const PW: &str = "rahasia";

    #[derive(Default)]
    struct Spec {
        rotate: Option<i64>,
        crop: Option<[i64; 4]>,
        marker: &'static str,
        xobject_clash: bool,
    }

    fn build(specs: &[Spec], tree_rotate: Option<i64>) -> Document {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(
            dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Courier"},
        );
        let res = doc.add_object(dictionary! {"Font"=>dictionary!{"F1"=>font}});
        let mut kids = vec![];
        for s in specs {
            let c = format!("BT /F1 24 Tf 100 700 Td ({}) Tj ET", s.marker);
            let cid = doc.add_object(Stream::new(Dictionary::new(), c.into_bytes()));
            let mut d = dictionary! {"Type"=>"Page","Parent"=>pages_id,"Contents"=>cid,"Resources"=>res};
            if let Some(r) = s.rotate {
                d.set("Rotate", r);
            }
            if let Some(c) = s.crop {
                d.set("CropBox", c.iter().map(|&v| Object::Integer(v)).collect::<Vec<_>>());
            }
            if s.xobject_clash {
                let img = doc.add_object(Stream::new(
                    dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>1,"Height"=>1,
                    "ColorSpace"=>"DeviceGray","BitsPerComponent"=>8},
                    vec![0],
                ));
                d.set(
                    "Resources",
                    dictionary! {"Font"=>dictionary!{"F1"=>font},"XObject"=>dictionary!{"ValeS0"=>img}},
                );
            }
            kids.push(Object::Reference(doc.add_object(d)));
        }
        let mut tree = dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>specs.len() as i64,
            "MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()]};
        if let Some(r) = tree_rotate {
            tree.set("Rotate", r);
        }
        doc.objects.insert(pages_id, Object::Dictionary(tree));
        let cat = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages_id});
        doc.trailer.set("Root", cat);
        doc.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal("0123456789abcdef"),
                Object::string_literal("0123456789abcdef"),
            ]),
        );
        doc
    }

    fn two_pages() -> Document {
        build(
            &[
                Spec { marker: "VALE-PAGE-ONE", ..Spec::default() },
                Spec { rotate: Some(90), marker: "VALE-PAGE-TWO", ..Spec::default() },
            ],
            None,
        )
    }

    fn bytes_of(doc: &mut Document) -> Vec<u8> {
        let mut v = Vec::new();
        doc.save_to(&mut v).unwrap();
        v
    }

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Lock {
        Rc4Bits40,
        Rc4,
        Aes128,
        Aes256,
    }

    const LOCKS: [Lock; 4] = [Lock::Rc4Bits40, Lock::Rc4, Lock::Aes128, Lock::Aes256];

    fn lock_state(doc: &Document, lock: Lock, permissions: Permissions) -> EncryptionState {
        let key = [7u8; 32];
        let v = match lock {
            Lock::Rc4Bits40 => EncryptionVersion::V1 {
                document: doc,
                owner_password: "pemilik",
                user_password: PW,
                permissions,
            },
            Lock::Aes128 => {
                let cf: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
                EncryptionVersion::V4 {
                    document: doc,
                    encrypt_metadata: true,
                    crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), cf)]),
                    stream_filter: b"StdCF".to_vec(),
                    string_filter: b"StdCF".to_vec(),
                    owner_password: "pemilik",
                    user_password: PW,
                    permissions,
                }
            }
            Lock::Rc4 => EncryptionVersion::V2 {
                document: doc,
                owner_password: "pemilik",
                user_password: PW,
                key_length: 128,
                permissions,
            },
            Lock::Aes256 => {
                let cf: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
                EncryptionVersion::V5 {
                    encrypt_metadata: true,
                    crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), cf)]),
                    file_encryption_key: &key,
                    stream_filter: b"StdCF".to_vec(),
                    string_filter: b"StdCF".to_vec(),
                    owner_password: "pemilik",
                    user_password: PW,
                    permissions,
                }
            }
        };
        EncryptionState::try_from(v).unwrap()
    }

    fn locked(mut doc: Document, lock: Lock, permissions: Permissions) -> Vec<u8> {
        let state = lock_state(&doc, lock, permissions);
        doc.encrypt(&state).unwrap();
        bytes_of(&mut doc)
    }

    fn mark() -> Arc<Mark> {
        let mut rgba = Vec::new();
        for i in 0..8u8 {
            rgba.extend_from_slice(&[0x1F, 0x3A, 0x8A, i * 30]);
        }
        Arc::new(Mark { w: 4, h: 2, rgba })
    }

    fn item(kind: ItemKind, page: usize, turn: Quarter) -> Item {
        Item { kind, page, x: 100.0, y: 200.0, w: 150.0, h: 50.0, turn }
    }

    fn sig(page: usize) -> Item {
        item(ItemKind::Signature(mark()), page, Quarter::R0)
    }

    fn pages_of(o: &Opened) -> Vec<Page> {
        o.geoms.iter().map(|&geom| Page { geom, extra: Quarter::R0, deleted: false }).collect()
    }

    fn geom_tuples(o: &Opened) -> Vec<(f32, f32, Quarter)> {
        o.geoms.iter().map(|g| (g.w, g.h, g.base)).collect()
    }

    fn load(bytes: &[u8], pw: Option<&str>) -> Document {
        match pw {
            Some(p) => Document::load_mem_with_options(bytes, LoadOptions::with_password(p)).unwrap(),
            None => Document::load_mem(bytes).unwrap(),
        }
    }

    fn page_id(doc: &Document, n: u32) -> ObjectId {
        *doc.get_pages().get(&n).unwrap()
    }

    fn resource(doc: &Document, n: u32, kind: &[u8]) -> Dictionary {
        let page = doc.get_dictionary(page_id(doc, n)).unwrap();
        let res = doc.dereference(page.get(b"Resources").unwrap()).unwrap().1;
        let sub = res.as_dict().unwrap().get(kind).unwrap();
        doc.dereference(sub).unwrap().1.as_dict().unwrap().clone()
    }

    fn vale_images(doc: &Document, n: u32) -> Vec<(Vec<u8>, ObjectId)> {
        resource(doc, n, b"XObject")
            .iter()
            .filter(|(k, _)| k.starts_with(b"ValeS"))
            .map(|(k, v)| (k.clone(), v.as_reference().unwrap()))
            .collect()
    }

    fn content(doc: &Document, n: u32) -> String {
        String::from_utf8_lossy(&doc.get_page_content(page_id(doc, n))).into_owned()
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    fn any_stream_has(doc: &Document, needle: &[u8]) -> bool {
        doc.objects.values().any(|o| match o {
            Object::Stream(s) => {
                let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
                contains(&data, needle)
            }
            _ => false,
        })
    }

    #[test]
    fn opens_plain_pdf_geometry() {
        let o = open(bytes_of(&mut two_pages()), None).unwrap();
        assert_eq!(
            geom_tuples(&o),
            vec![(612.0, 792.0, Quarter::R0), (612.0, 792.0, Quarter::R90)]
        );
        assert!(!o.encrypted && !o.no_changes && !o.signed);
    }

    #[test]
    fn inherited_rotate_and_cropbox() {
        let mut doc = build(&[Spec { crop: Some([10, 10, 500, 700]), ..Spec::default() }], Some(270));
        let o = open(bytes_of(&mut doc), None).unwrap();
        assert_eq!(geom_tuples(&o), vec![(490.0, 690.0, Quarter::R270)]);
    }

    #[test]
    fn cropbox_origin_offsets_items() {
        let mut doc = build(&[Spec { crop: Some([10, 20, 500, 700]), ..Spec::default() }], None);
        let o = open(bytes_of(&mut doc), None).unwrap();
        let out = save(&o, &pages_of(&o), &[sig(0)]).unwrap();
        assert!(content(&load(&out, None), 1).contains("1 0 0 1 10 20 cm"));
    }

    #[test]
    fn adds_signature_incrementally_keeps_prefix() {
        let src = bytes_of(&mut two_pages());
        let o = open(src.clone(), None).unwrap();
        let out = save(&o, &pages_of(&o), &[sig(0)]).unwrap();
        assert!(out.len() > src.len() && out.starts_with(&src));
        let doc = load(&out, None);
        let images = vale_images(&doc, 1);
        assert_eq!(images.len(), 1);
        let img = doc.get_object(images[0].1).unwrap().as_stream().unwrap();
        assert!(img.dict.get(b"SMask").unwrap().as_reference().is_ok());
        assert!(content(&doc, 1).contains(&format!("/{} Do", String::from_utf8_lossy(&images[0].0))));
        assert!(content(&doc, 1).contains("VALE-PAGE-ONE"));
        // the page's own font is still there
        assert!(resource(&doc, 1, b"Font").has(b"F1"));
    }

    #[test]
    fn same_mark_twice_writes_one_image() {
        let o = open(bytes_of(&mut two_pages()), None).unwrap();
        let m = mark();
        let a = item(ItemKind::Initials(m.clone()), 0, Quarter::R0);
        let b = item(ItemKind::Initials(m), 1, Quarter::R90);
        let doc = load(&save(&o, &pages_of(&o), &[a, b]).unwrap(), None);
        assert_eq!(vale_images(&doc, 1)[0].1, vale_images(&doc, 2)[0].1);
    }

    #[test]
    fn xobject_names_never_clash() {
        let mut doc = build(&[Spec { xobject_clash: true, ..Spec::default() }], None);
        let o = open(bytes_of(&mut doc), None).unwrap();
        let doc = load(&save(&o, &pages_of(&o), &[sig(0)]).unwrap(), None);
        let names: Vec<Vec<u8>> = vale_images(&doc, 1).into_iter().map(|(k, _)| k).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&b"ValeS0".to_vec()) && names.contains(&b"ValeS1".to_vec()));
        assert!(content(&doc, 1).contains("/ValeS1 Do"));
    }

    #[test]
    fn item_on_rotated_source_page_gets_matching_matrix() {
        let o = open(bytes_of(&mut two_pages()), None).unwrap();
        let it = item(ItemKind::Signature(mark()), 1, Quarter::R90);
        let m = item_matrix(&it);
        let want = format!("{} cm", m.iter().map(|&v| num(v)).collect::<Vec<_>>().join(" "));
        assert_eq!(want, "0 150 -50 0 150 200 cm");
        let doc = load(&save(&o, &pages_of(&o), &[it]).unwrap(), None);
        assert!(content(&doc, 2).contains(&want));
        assert!(!content(&doc, 1).contains(&want));
    }

    #[test]
    fn text_item_uses_helvetica_winansi() {
        let o = open(bytes_of(&mut two_pages()), None).unwrap();
        let it = item(ItemKind::Date("20 November 2026".into(), Default::default()), 0, Quarter::R0);
        let doc = load(&save(&o, &pages_of(&o), &[it]).unwrap(), None);
        let fonts = resource(&doc, 1, b"Font");
        let ours = fonts
            .iter()
            .filter(|(k, _)| k.starts_with(b"ValeF"))
            .map(|(_, v)| doc.dereference(v).unwrap().1.as_dict().unwrap().clone())
            .next()
            .unwrap();
        assert_eq!(ours.get(b"BaseFont").unwrap().as_name().unwrap(), b"Helvetica");
        assert_eq!(ours.get(b"Encoding").unwrap().as_name().unwrap(), b"WinAnsiEncoding");
        let c = content(&doc, 1);
        assert!(c.contains("(20 November 2026) Tj"));
        assert!(c.contains("/ValeF 1 Tf 0.12 0.23 0.54 rg"));
    }

    #[test]
    fn check_item_is_an_ink_blue_stroke() {
        let o = open(bytes_of(&mut two_pages()), None).unwrap();
        let check = item(ItemKind::Check(Default::default()), 0, Quarter::R0);
        let doc = load(&save(&o, &pages_of(&o), &[check]).unwrap(), None);
        let c = content(&doc, 1);
        assert!(c.contains("0.12 0.23 0.54 RG") && c.contains("0.12 w") && c.contains(" S"));
    }

    #[test]
    fn rotate_sets_page_rotate() {
        let o = open(bytes_of(&mut two_pages()), None).unwrap();
        let mut pages = pages_of(&o);
        pages[1].extra = Quarter::R90;
        let doc = load(&save(&o, &pages, &[]).unwrap(), None);
        let p2 = doc.get_dictionary(page_id(&doc, 2)).unwrap();
        assert_eq!(p2.get(b"Rotate").unwrap().as_i64().unwrap(), 180);
        let p1 = doc.get_dictionary(page_id(&doc, 1)).unwrap();
        assert!(p1.get(b"Rotate").is_err());
    }

    #[test]
    fn deleted_page_content_is_absent_after_save() {
        let src = bytes_of(&mut two_pages());
        assert!(contains(&src, b"(VALE-PAGE-TWO) Tj"));
        let o = open(src, None).unwrap();
        let mut pages = pages_of(&o);
        pages[1].deleted = true;
        let out = save(&o, &pages, &[sig(0), sig(1)]).unwrap();
        assert!(!contains(&out, b"VALE-PAGE-TWO"));
        let doc = load(&out, None);
        assert_eq!(doc.get_pages().len(), 1);
        assert!(content(&doc, 1).contains("VALE-PAGE-ONE"));
        assert_eq!(vale_images(&doc, 1).len(), 1);
    }

    #[test]
    fn encrypted_incremental_keeps_password() {
        for lock in LOCKS {
            let src = locked(two_pages(), lock, Permissions::all());
            let o = open(src.clone(), Some(PW)).unwrap();
            assert!(o.encrypted, "{lock:?}");
            let out = save(&o, &pages_of(&o), &[sig(0)]).unwrap();
            assert!(out.starts_with(&src), "{lock:?}");
            assert!(!contains(&out, b"VALE-PAGE-ONE"), "{lock:?}");
            assert!(!contains(&out, b" Do\n"), "{lock:?} new content written in plain text");
            if let Ok(d) = Document::load_mem(&out) {
                assert!(d.is_encrypted() && d.get_pages().is_empty(), "{lock:?}");
            }
            assert!(Document::load_mem_with_options(&out, LoadOptions::with_password("salah")).is_err());
            let doc = load(&out, Some(PW));
            assert!(doc.encryption_state.is_some(), "{lock:?} lock lost");
            assert_eq!(vale_images(&doc, 1).len(), 1, "{lock:?}");
            assert!(content(&doc, 1).contains("VALE-PAGE-ONE"), "{lock:?}");
        }
    }

    #[test]
    fn deleted_page_on_encrypted_pdf_keeps_password() {
        for lock in LOCKS {
            let src = locked(two_pages(), lock, Permissions::all());
            let o = open(src, Some(PW)).unwrap();
            let mut pages = pages_of(&o);
            pages[1].deleted = true;
            let out = save(&o, &pages, &[sig(0)]).unwrap();
            assert!(!contains(&out, b"VALE-PAGE-ONE"), "{lock:?} written in plain text");
            assert!(!contains(&out, b" Do\n"), "{lock:?} new content written in plain text");
            assert_eq!(open(out.clone(), None).err(), Some(OpenError::NeedsPassword), "{lock:?}");
            let doc = load(&out, Some(PW));
            assert!(doc.encryption_state.is_some(), "{lock:?} lock lost");
            assert_eq!(doc.get_pages().len(), 1, "{lock:?}");
            assert!(!any_stream_has(&doc, b"VALE-PAGE-TWO"), "{lock:?}");
            assert!(content(&doc, 1).contains("VALE-PAGE-ONE"), "{lock:?}");
            assert_eq!(vale_images(&doc, 1).len(), 1, "{lock:?}");
        }
    }

    #[test]
    fn wrong_password_is_reported() {
        let src = locked(two_pages(), Lock::Aes256, Permissions::all());
        assert_eq!(open(src, Some("salah")).err(), Some(OpenError::WrongPassword));
    }

    #[test]
    fn needs_password_without_one() {
        let src = locked(two_pages(), Lock::Rc4, Permissions::all());
        assert_eq!(open(src, None).err(), Some(OpenError::NeedsPassword));
    }

    #[test]
    fn permission_flags_set_no_changes() {
        let few = Permissions::PRINTABLE | Permissions::COPYABLE | Permissions::ANNOTABLE;
        let o = open(locked(two_pages(), Lock::Aes256, few), Some(PW)).unwrap();
        assert!(o.no_changes);
        let o = open(locked(two_pages(), Lock::Rc4, few), Some(PW)).unwrap();
        assert!(o.no_changes);
        let o = open(locked(two_pages(), Lock::Aes256, Permissions::all()), Some(PW)).unwrap();
        assert!(!o.no_changes);
    }

    #[test]
    fn detects_existing_signature() {
        let mut doc = two_pages();
        let v = doc.add_object(dictionary! {"Type"=>"Sig","Filter"=>"Adobe.PPKLite"});
        let field = doc.add_object(dictionary! {"FT"=>"Sig","T"=>Object::string_literal("Sig1"),"V"=>v});
        let form = doc.add_object(dictionary! {"Fields"=>vec![Object::Reference(field)],"SigFlags"=>3});
        doc.catalog_mut().unwrap().set("AcroForm", form);
        assert!(open(bytes_of(&mut doc), None).unwrap().signed);

        // an empty signature field is not a signature yet
        let mut doc = two_pages();
        let field = doc.add_object(dictionary! {"FT"=>"Sig","T"=>Object::string_literal("Sig1")});
        let form = doc.add_object(dictionary! {"Fields"=>vec![Object::Reference(field)]});
        doc.catalog_mut().unwrap().set("AcroForm", form);
        assert!(!open(bytes_of(&mut doc), None).unwrap().signed);
    }

    #[test]
    fn writes_into_xref_stream_source() {
        let mut doc = two_pages();
        let mut src = Vec::new();
        doc.save_modern(&mut src).unwrap();
        let o = open(src.clone(), None).unwrap();
        assert_eq!(o.geoms.len(), 2);
        let it = item(ItemKind::Text("Ibu Siti".into(), Default::default()), 1, Quarter::R90);
        let out = save(&o, &pages_of(&o), &[it, sig(0)]).unwrap();
        assert!(out.starts_with(&src));
        let doc = load(&out, None);
        assert!(content(&doc, 2).contains("(Ibu Siti) Tj"));
        assert!(content(&doc, 2).contains("VALE-PAGE-TWO"));
        assert_eq!(vale_images(&doc, 1).len(), 1);
    }

    #[test]
    fn signs_an_already_updated_file_again() {
        let o = open(bytes_of(&mut two_pages()), None).unwrap();
        let once = save(&o, &pages_of(&o), &[sig(0)]).unwrap();
        let o = open(once.clone(), None).unwrap();
        let twice = save(&o, &pages_of(&o), &[sig(1)]).unwrap();
        assert!(twice.starts_with(&once));
        let doc = load(&twice, None);
        assert_eq!((vale_images(&doc, 1).len(), vale_images(&doc, 2).len()), (1, 1));
        let mut pages = pages_of(&o);
        pages[0].deleted = true;
        let doc = load(&save(&o, &pages, &[]).unwrap(), None);
        assert_eq!(doc.get_pages().len(), 1);
        assert!(content(&doc, 1).contains("VALE-PAGE-TWO"));
        assert!(!any_stream_has(&doc, b"VALE-PAGE-ONE"));
    }

    #[test]
    fn odd_page_structures_do_not_panic() {
        let mut doc = two_pages();
        let first = page_id(&doc, 1);
        let tree = doc.get_dictionary(first).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
        let page = doc.get_dictionary_mut(first).unwrap();
        page.set("Contents", 7);
        page.set("Resources", Object::Reference((9999, 0)));
        page.set("MediaBox", vec![Object::Name(b"a".to_vec()), 1.into()]);
        page.set("Rotate", 45);
        let o = open(bytes_of(&mut doc), None).unwrap();
        assert_eq!((o.geoms[0].w, o.geoms[0].h, o.geoms[0].base), (612.0, 792.0, Quarter::R0));
        let it = item(ItemKind::Text("x".into(), Default::default()), 0, Quarter::R0);
        let mut pages = pages_of(&o);
        pages[0].extra = Quarter::R270;
        let out = load(&save(&o, &pages, &[it, sig(0)]).unwrap(), None);
        assert!(content(&out, 1).contains("(x) Tj"));
        let rotate = out.get_dictionary(page_id(&out, 1)).unwrap().get(b"Rotate").unwrap().as_i64();
        assert_eq!(rotate.unwrap(), 270);

        // a page tree that lists itself
        let kids = doc.get_dictionary_mut(tree).unwrap().get_mut(b"Kids").unwrap();
        kids.as_array_mut().unwrap().push(Object::Reference(tree));
        assert_eq!(open(bytes_of(&mut doc), None).err(), Some(OpenError::Broken));
    }

    #[test]
    fn garbage_and_truncated_input_are_errors() {
        assert_eq!(open(b"hello".to_vec(), None).err(), Some(OpenError::Broken));
        assert_eq!(open(Vec::new(), None).err(), Some(OpenError::Broken));
        let mut modern = Vec::new();
        two_pages().save_modern(&mut modern).unwrap();
        let classic = bytes_of(&mut two_pages());
        for src in [modern, classic] {
            for cut in [40, 200, src.len() / 2] {
                let short = src[..src.len() - cut].to_vec();
                match open(short, None) {
                    Ok(o) => {
                        let _ = save(&o, &pages_of(&o), &[sig(0)]);
                    }
                    Err(e) => assert_eq!(e, OpenError::Broken),
                }
            }
        }
    }

    fn strings_have(o: &Object, needle: &[u8]) -> bool {
        match o {
            Object::String(s, _) => contains(s, needle),
            Object::Array(a) => a.iter().any(|x| strings_have(x, needle)),
            Object::Dictionary(d) => d.iter().any(|(_, v)| strings_have(v, needle)),
            Object::Stream(s) => s.dict.iter().any(|(_, v)| strings_have(v, needle)),
            _ => false,
        }
    }

    /// The text is anywhere in the document: a stream or a string.
    fn doc_has(doc: &Document, needle: &[u8]) -> bool {
        any_stream_has(doc, needle) || doc.objects.values().any(|o| strings_have(o, needle))
    }

    fn delete_second(o: &Opened) -> Document {
        let mut pages = pages_of(o);
        pages[1].deleted = true;
        let out = save(o, &pages, &[]).unwrap();
        load(&out, o.password.as_deref())
    }

    fn widget(doc: &mut Document, page: ObjectId, extra: Dictionary) -> ObjectId {
        let mut d = dictionary! {"Type"=>"Annot","Subtype"=>"Widget","P"=>page,
            "Rect"=>vec![0.into(),0.into(),100.into(),20.into()]};
        for (k, v) in extra.iter() {
            d.set(k.clone(), v.clone());
        }
        doc.add_object(d)
    }

    #[test]
    fn deleted_page_form_fields_are_gone() {
        let mut doc = two_pages();
        let (p1, p2) = (page_id(&doc, 1), page_id(&doc, 2));
        let ap = doc.add_object(Stream::new(
            dictionary! {"Type"=>"XObject","Subtype"=>"Form",
                "BBox"=>vec![0.into(),0.into(),100.into(),20.into()]},
            b"BT /F1 9 Tf (VALE-FIELD-TWO) Tj ET".to_vec(),
        ));
        let lone = widget(
            &mut doc,
            p2,
            dictionary! {"FT"=>"Tx","T"=>Object::string_literal("lone"),
                "V"=>Object::string_literal("VALE-FIELD-TWO"),"AP"=>dictionary!{"N"=>ap}},
        );
        let parent = doc.add_object(dictionary! {"FT"=>"Tx","T"=>Object::string_literal("parent"),
            "V"=>Object::string_literal("VALE-PARENT-TWO")});
        let kid = widget(&mut doc, p2, dictionary! {"Parent"=>parent});
        doc.get_dictionary_mut(parent).unwrap().set("Kids", vec![Object::Reference(kid)]);
        let one = widget(
            &mut doc,
            p1,
            dictionary! {"FT"=>"Tx","T"=>Object::string_literal("one"),
                "V"=>Object::string_literal("VALE-FIELD-ONE")},
        );
        doc.get_dictionary_mut(p1).unwrap().set("Annots", vec![Object::Reference(one)]);
        doc.get_dictionary_mut(p2)
            .unwrap()
            .set("Annots", vec![Object::Reference(lone), Object::Reference(kid)]);
        let fields = vec![Object::Reference(lone), Object::Reference(parent), Object::Reference(one)];
        let form = doc.add_object(dictionary! {"Fields"=>fields,"CO"=>vec![Object::Reference(lone)]});
        doc.catalog_mut().unwrap().set("AcroForm", form);
        let src = bytes_of(&mut doc);
        assert!(contains(&src, b"VALE-FIELD-TWO") && contains(&src, b"VALE-PARENT-TWO"));

        let o = open(src, None).unwrap();
        let out = delete_second(&o);
        assert!(!doc_has(&out, b"VALE-FIELD-TWO"));
        assert!(!doc_has(&out, b"VALE-PARENT-TWO"));
        assert!(doc_has(&out, b"VALE-FIELD-ONE"));
        let form = out.catalog().unwrap().get(b"AcroForm").unwrap();
        let form = out.dereference(form).unwrap().1.as_dict().unwrap();
        let fields = out.dereference(form.get(b"Fields").unwrap()).unwrap().1.as_array().unwrap();
        assert_eq!(fields.len(), 1);
    }

    /// Two pages drawing `/Im1` and `/Im2`; the image dictionary sits on the
    /// page tree (`on_tree`) or in one indirect dictionary both pages use.
    fn shared_images(on_tree: bool) -> Document {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let image = |doc: &mut Document, mark: &[u8]| {
            doc.add_object(Stream::new(
                dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>mark.len() as i64,
                    "Height"=>1,"ColorSpace"=>"DeviceGray","BitsPerComponent"=>8},
                mark.to_vec(),
            ))
        };
        let im1 = image(&mut doc, b"VALE-IMG-ONE");
        let im2 = image(&mut doc, b"VALE-IMG-TWO");
        let res = dictionary! {"XObject"=>dictionary!{"Im1"=>im1,"Im2"=>im2}};
        let shared = doc.add_object(res.clone());
        let mut kids = vec![];
        for draw in ["q 100 0 0 10 0 0 cm /Im1 Do Q", "q 100 0 0 10 0 0 cm /Im#32 Do Q"] {
            let c = doc.add_object(Stream::new(Dictionary::new(), draw.as_bytes().to_vec()));
            let mut d = dictionary! {"Type"=>"Page","Parent"=>pages_id,"Contents"=>c};
            if !on_tree {
                d.set("Resources", shared);
            }
            kids.push(Object::Reference(doc.add_object(d)));
        }
        let mut tree = dictionary! {"Type"=>"Pages","Kids"=>kids,"Count"=>2,
            "MediaBox"=>vec![0.into(),0.into(),612.into(),792.into()]};
        if on_tree {
            tree.set("Resources", res);
        }
        doc.objects.insert(pages_id, Object::Dictionary(tree));
        let cat = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages_id});
        doc.trailer.set("Root", cat);
        doc
    }

    #[test]
    fn image_only_the_deleted_page_drew_is_gone() {
        for on_tree in [true, false] {
            let src = bytes_of(&mut shared_images(on_tree));
            assert!(contains(&src, b"VALE-IMG-TWO"));
            let o = open(src, None).unwrap();
            let mut pages = pages_of(&o);
            pages[1].deleted = true;
            let out = save(&o, &pages, &[sig(0)]).unwrap();
            assert!(!contains(&out, b"VALE-IMG-TWO"), "on_tree {on_tree}");
            let doc = load(&out, None);
            assert!(any_stream_has(&doc, b"VALE-IMG-ONE"), "on_tree {on_tree}");
            let xobjects = resource(&doc, 1, b"XObject");
            assert!(xobjects.has(b"Im1") && !xobjects.has(b"Im2"), "on_tree {on_tree}");
            assert_eq!(vale_images(&doc, 1).len(), 1);
        }
    }

    #[test]
    fn check_output_requires_the_lock() {
        let plain = bytes_of(&mut two_pages());
        assert_eq!(check_output(&plain, None, 2, true), Err(SaveError::CannotKeepLock));
        assert_eq!(check_output(&plain, None, 2, false), Ok(()));
        assert!(matches!(check_output(&plain, None, 1, false), Err(SaveError::Write(_))));
    }

    #[test]
    fn unbalanced_q_in_page_is_closed_first() {
        let spec = Spec { marker: "x) Tj ET q 2 0 0 2 0 0 cm BT (y", ..Spec::default() };
        let o = open(bytes_of(&mut build(&[spec], None)), None).unwrap();
        let doc = load(&save(&o, &pages_of(&o), &[sig(0)]).unwrap(), None);
        let ids = doc.get_page_contents(page_id(&doc, 1));
        let stream = |id: &ObjectId| doc.get_object(*id).unwrap().as_stream().unwrap().content.clone();
        assert_eq!(ids.len(), 3);
        assert_eq!(stream(&ids[0]), b"q\n");
        assert!(stream(&ids[2]).starts_with(b"\nQ\nQ\nq\n"));
        assert!(!stream(&ids[2]).starts_with(b"\nQ\nQ\nQ"));
    }

    #[test]
    fn content_scanner_skips_strings_comments_and_images() {
        let c = b"Q q (Q) Tj % Q\nBI /W 1 /H 1 ID \x00 Q Q\x00 EI q Q q <51> Tj /Last";
        let s = scan_stream(c).unwrap();
        assert_eq!((s.pops, s.pushes, s.trailing), (1, 2, Some(b"Last".to_vec())));
        let s = scan_stream(b"/Im#31 Do (/X Do) Tj /Y Do /Z 5 Do").unwrap();
        assert_eq!(s.used, BTreeSet::from([b"Im1".to_vec(), b"Y".to_vec()]));
        // inline image data with no white space before EI: the rest is unknown
        assert_eq!(scan_stream(b"BI /W 1 ID \xffEI /Im1 Do"), None);
    }

    #[test]
    fn incremental_unless_a_page_is_deleted() {
        let o = open(bytes_of(&mut two_pages()), None).unwrap();
        let mut pages = pages_of(&o);
        assert!(save_is_incremental(&o, &pages));
        pages[1].extra = Quarter::R90;
        assert!(save_is_incremental(&o, &pages));
        pages[1].deleted = true;
        assert!(!save_is_incremental(&o, &pages));
        let o = open(locked(two_pages(), Lock::Aes256, Permissions::all()), Some(PW)).unwrap();
        assert!(save_is_incremental(&o, &pages_of(&o)));
    }

    #[test]
    fn links_to_deleted_page_are_removed() {
        let mut doc = two_pages();
        let (p1, p2) = (page_id(&doc, 1), page_id(&doc, 2));
        let fit = |p: ObjectId| vec![Object::Reference(p), Object::Name(b"Fit".to_vec())];
        let root = doc.new_object_id();
        let (o1, o2, child) = (doc.new_object_id(), doc.new_object_id(), doc.new_object_id());
        let title = |t: &str| Object::string_literal(t);
        doc.set_object(o1, dictionary! {"Title"=>title("VALE-OUT-ONE"),"Parent"=>root,
            "Dest"=>fit(p1),"Next"=>o2});
        doc.set_object(o2, dictionary! {"Title"=>title("VALE-OUT-TWO"),"Parent"=>root,
            "Dest"=>fit(p2),"Prev"=>o1,"First"=>child,"Last"=>child,"Count"=>1});
        doc.set_object(child, dictionary! {"Title"=>title("VALE-OUT-CHILD"),"Parent"=>o2,
            "A"=>dictionary!{"S"=>"GoTo","D"=>fit(p1)}});
        doc.set_object(root, dictionary! {"Type"=>"Outlines","First"=>o1,"Last"=>o2,"Count"=>3});
        let names = vec![title("n1"), fit(p1).into(), title("n2"), fit(p2).into()];
        let link = |doc: &mut Document, how: Dictionary| {
            let mut d = dictionary! {"Type"=>"Annot","Subtype"=>"Link",
                "Rect"=>vec![0.into(),0.into(),10.into(),10.into()]};
            for (k, v) in how.iter() {
                d.set(k.clone(), v.clone());
            }
            Object::Reference(doc.add_object(d))
        };
        let to_two = link(&mut doc, dictionary! {"Dest"=>"Two"});
        let to_n2 = link(&mut doc, dictionary! {"A"=>dictionary!{"S"=>"GoTo","D"=>title("n2")}});
        let to_n1 = link(&mut doc, dictionary! {"A"=>dictionary!{"S"=>"GoTo","D"=>title("n1")}});
        doc.get_dictionary_mut(p1).unwrap().set("Annots", vec![to_two, to_n2, to_n1]);
        let catalog = doc.catalog_mut().unwrap();
        catalog.set("Outlines", root);
        catalog.set("Dests", dictionary! {"One"=>fit(p1),"Two"=>fit(p2)});
        catalog.set("Names", dictionary! {"Dests"=>dictionary!{"Names"=>names}});
        catalog.set("OpenAction", dictionary! {"S"=>"GoTo","D"=>fit(p2)});

        let o = open(bytes_of(&mut doc), None).unwrap();
        let out = delete_second(&o);
        assert!(!doc_has(&out, b"VALE-OUT-TWO"));
        let catalog = out.catalog().unwrap();
        assert!(catalog.get(b"OpenAction").is_err());
        let dests = out.dereference(catalog.get(b"Dests").unwrap()).unwrap().1.as_dict().unwrap();
        assert!(dests.has(b"One") && !dests.has(b"Two"));
        let names = out.dereference(catalog.get(b"Names").unwrap()).unwrap().1.as_dict().unwrap();
        let tree = out.dereference(names.get(b"Dests").unwrap()).unwrap().1.as_dict().unwrap();
        assert_eq!(tree.get(b"Names").unwrap().as_array().unwrap().len(), 2);
        let p1 = out.get_dictionary(page_id(&out, 1)).unwrap();
        assert_eq!(p1.get(b"Annots").unwrap().as_array().unwrap().len(), 1);
        // outline: item one, then the child of the removed item moved up
        let root_id = catalog.get(b"Outlines").unwrap().as_reference().unwrap();
        let root = out.get_dictionary(root_id).unwrap();
        let first = out.get_dictionary(root.get(b"First").unwrap().as_reference().unwrap()).unwrap();
        assert_eq!(first.get(b"Title").unwrap().as_str().unwrap(), b"VALE-OUT-ONE");
        let second_id = first.get(b"Next").unwrap().as_reference().unwrap();
        let second = out.get_dictionary(second_id).unwrap();
        assert_eq!(second.get(b"Title").unwrap().as_str().unwrap(), b"VALE-OUT-CHILD");
        assert!(second.get(b"Next").is_err());
        assert_eq!(root.get(b"Last").unwrap().as_reference().unwrap(), second_id);
        assert_eq!(root.get(b"Count").unwrap().as_i64().unwrap(), 2);
    }

    #[test]
    fn encrypted_xref_stream_source_saves_both_ways() {
        for lock in [Lock::Aes256, Lock::Rc4] {
            let mut doc = two_pages();
            let state = lock_state(&doc, lock, Permissions::all());
            doc.encrypt(&state).unwrap();
            let mut src = Vec::new();
            doc.save_modern(&mut src).unwrap();
            let o = open(src.clone(), Some(PW)).unwrap();
            assert!(o.encrypted && o.geoms.len() == 2, "{lock:?}");
            let out = save(&o, &pages_of(&o), &[sig(1)]).unwrap();
            assert!(out.starts_with(&src), "{lock:?}");
            let doc = load(&out, Some(PW));
            assert!(doc.encryption_state.is_some(), "{lock:?}");
            assert_eq!(vale_images(&doc, 2).len(), 1, "{lock:?}");
            assert!(content(&doc, 2).contains("VALE-PAGE-TWO"), "{lock:?}");
            let doc = delete_second(&o);
            assert!(doc.encryption_state.is_some(), "{lock:?}");
            assert_eq!(doc.get_pages().len(), 1, "{lock:?}");
            assert!(!doc_has(&doc, b"VALE-PAGE-TWO"), "{lock:?}");
            assert!(content(&doc, 1).contains("VALE-PAGE-ONE"), "{lock:?}");
        }
    }

    #[test]
    fn certificate_lock_is_broken_not_a_password() {
        let mut doc = two_pages();
        let enc = doc.add_object(dictionary! {"Filter"=>"Adobe.PubSec","SubFilter"=>"adbe.pkcs7.s5",
            "V"=>4,"Length"=>128,"Recipients"=>vec![Object::string_literal("x")]});
        doc.trailer.set("Encrypt", enc);
        let src = bytes_of(&mut doc);
        assert_eq!(open(src.clone(), None).err(), Some(OpenError::Broken));
        assert_eq!(open(src, Some(PW)).err(), Some(OpenError::Broken));
    }

    /// Page 1 also lists 64 times a stream of 8 MiB of zeros, deflated: one
    /// object (`distinct == false`) or 64 copies.
    fn bomb_page(distinct: bool) -> Document {
        let mut doc = two_pages();
        let p1 = page_id(&doc, 1);
        let packed = compress_to_vec_zlib(&vec![0u8; 8 << 20], 6);
        let bomb = |doc: &mut Document| {
            doc.add_object(Stream::new(dictionary! {"Filter"=>"FlateDecode"}, packed.clone()))
        };
        let one = bomb(&mut doc);
        let mut contents: Vec<Object> =
            doc.get_page_contents(p1).into_iter().map(Object::Reference).collect();
        for _ in 0..64 {
            let id = if distinct { bomb(&mut doc) } else { one };
            contents.push(Object::Reference(id));
        }
        doc.get_dictionary_mut(p1).unwrap().set("Contents", contents);
        doc
    }

    #[test]
    fn content_decoding_stays_within_budget() {
        for distinct in [false, true] {
            let o = open(bytes_of(&mut bomb_page(distinct)), None).unwrap();
            let p1 = page_id(&o.doc, 1);
            let mut scans = Scans::new();
            let scan = scans.page(&o.doc, p1);
            assert!(scans.decoded <= PAGE_BUDGET, "distinct {distinct}");
            // one shared stream is decoded once and the page stays readable
            assert_eq!(scan.drawn.is_some(), !distinct);
            if !distinct {
                assert!(scans.decoded < 9 << 20);
            }
            let mut tight = Scans::new();
            tight.left = 10 << 20;
            assert!(tight.page(&o.doc, p1).drawn.is_none() || !distinct);
            assert!(tight.decoded <= 10 << 20);

            let out = save(&o, &pages_of(&o), &[sig(0)]).unwrap();
            let doc = load(&out, None);
            assert_eq!(vale_images(&doc, 1).len(), 1);
            let ids = doc.get_page_contents(page_id(&doc, 1));
            let last = doc.get_object(*ids.last().unwrap()).unwrap().as_stream().unwrap();
            assert!(String::from_utf8_lossy(&last.content).contains("/ValeS0 Do"));
            let doc = delete_second(&o);
            assert_eq!(doc.get_pages().len(), 1);
        }
    }

    #[test]
    fn annotation_a_remaining_page_lists_stays() {
        let mut doc = two_pages();
        let (p1, p2) = (page_id(&doc, 1), page_id(&doc, 2));
        let note = doc.add_object(dictionary! {"Type"=>"Annot","Subtype"=>"Text",
            "Rect"=>vec![0.into(),0.into(),10.into(),10.into()],
            "Contents"=>Object::string_literal("VALE-SHARED"),"P"=>p2});
        for p in [p1, p2] {
            doc.get_dictionary_mut(p).unwrap().set("Annots", vec![Object::Reference(note)]);
        }
        let o = open(bytes_of(&mut doc), None).unwrap();
        let out = delete_second(&o);
        let p1 = out.get_dictionary(page_id(&out, 1)).unwrap();
        assert_eq!(p1.get(b"Annots").unwrap().as_array().unwrap().len(), 1);
        assert!(doc_has(&out, b"VALE-SHARED"));
    }

    #[test]
    fn unreadable_inline_image_keeps_every_xobject() {
        let mut doc = shared_images(true);
        let p1 = page_id(&doc, 1);
        let cid = doc.get_page_contents(p1)[0];
        let c = b"BI /W 1 /H 1 /CS /G /BPC 8 ID \xffEI q 100 0 0 10 0 0 cm /Im1 Do Q";
        doc.set_object(cid, Stream::new(Dictionary::new(), c.to_vec()));
        let o = open(bytes_of(&mut doc), None).unwrap();
        let out = delete_second(&o);
        assert!(resource(&out, 1, b"XObject").has(b"Im1"));
    }

    #[test]
    fn too_big_is_refused_before_parsing() {
        assert_eq!(open(vec![0u8; MAX_BYTES + 1], None).err(), Some(OpenError::TooBig));
    }

    #[test]
    fn number_format() {
        assert_eq!(num(150.0), "150");
        assert_eq!(num(-0.0), "0");
        assert_eq!(num(0.123456), "0.1235");
        assert_eq!(num(f32::NAN), "0");
        assert_eq!(num(-12.5), "-12.5");
    }
}
