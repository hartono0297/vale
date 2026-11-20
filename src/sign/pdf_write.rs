//! Opening a PDF for signing and writing the signed copy with lopdf.

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
        Err(lopdf::Error::InvalidPassword) => return Err(OpenError::WrongPassword),
        Err(_) => return Err(OpenError::Broken),
    };
    if doc.is_encrypted() {
        // Loaded, but nothing could be decrypted.
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
    // An incremental update needs an intact cross-reference chain to point
    // back to, and for a locked file the original /Encrypt object.
    let incremental = live == ids.len()
        && o.doc.xref_recovery.is_none()
        && o.doc.xref_start != 0
        && o.doc.encryption_state.as_ref().is_none_or(|s| s.encrypt_object_id().is_some());
    let out = if incremental {
        save_incremental(o, &ids, pages, items)?
    } else {
        save_full(o, &ids, pages, items)?
    };
    match load(&out, o.password.as_deref()) {
        Ok(d) if !d.is_encrypted() && d.get_pages().len() == live => Ok(out),
        Ok(_) => Err(SaveError::Write("the saved file does not read back".into())),
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
    let mut shared = Shared::default();
    for e in edits(ids, pages, items) {
        let read = read_page(inc.get_prev_documents(), e.id)?;
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
    let mut shared = Shared::default();
    for e in edits(ids, pages, items) {
        let read = read_page(&doc, e.id)?;
        let dict = write_page(&mut doc, read, &e, &mut shared)?;
        doc.set_object(e.id, dict);
    }
    let gone: Vec<u32> = (0..ids.len())
        .filter(|&i| pages.get(i).is_some_and(|p| p.deleted))
        .filter_map(|i| u32::try_from(i + 1).ok())
        .collect();
    if !gone.is_empty() {
        doc.delete_pages(&gone);
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
    origin: (f32, f32),
    base: Quarter,
}

fn read_page(doc: &Document, id: ObjectId) -> Result<PageRead, SaveError> {
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
    let mut c: Vec<u8> = b"Q\nq\n".to_vec();
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
            ItemKind::Date(s) | ItemKind::Text(s) => {
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
            ItemKind::Check => {
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

    fn locked(mut doc: Document, lock: Lock, permissions: Permissions) -> Vec<u8> {
        let key = [7u8; 32];
        let v = match lock {
            Lock::Rc4Bits40 => EncryptionVersion::V1 {
                document: &doc,
                owner_password: "pemilik",
                user_password: PW,
                permissions,
            },
            Lock::Aes128 => {
                let cf: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
                EncryptionVersion::V4 {
                    document: &doc,
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
                document: &doc,
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
        let state = EncryptionState::try_from(v).unwrap();
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
        let it = item(ItemKind::Date("20 November 2026".into()), 0, Quarter::R0);
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
        let check = item(ItemKind::Check, 0, Quarter::R0);
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
        let it = item(ItemKind::Text("Ibu Siti".into()), 1, Quarter::R90);
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
        let it = item(ItemKind::Text("x".into()), 0, Quarter::R0);
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
