//! The real clipboard: the change listener, reading a change into a
//! `ClipSnapshot` and writing an entry back. Only the running island calls
//! these; no test and no demo does (they would read, change or empty the
//! owner's clipboard).
//!
//! The clipboard is held open only while bytes are copied out or in.

use std::path::PathBuf;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, EnumClipboardFormats,
    GetClipboardData, GetClipboardFormatNameW, GetClipboardOwner, GetClipboardSequenceNumber,
    OpenClipboard, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

use super::image;
use super::kinds::{Rule, excluded_rule, is_own_change, should_skip};
use crate::widgets::cliptext::CLIP_MAX;
use super::store::{Clip, ClipData, ClipSnapshot, MAX_IMAGE_BYTES, MAX_TEXT_BYTES, Original};

const CF_TEXT: u32 = 1;
const CF_BITMAP: u32 = 2;
const CF_METAFILEPICT: u32 = 3;
const CF_OEMTEXT: u32 = 7;
const CF_DIB: u32 = 8;
#[cfg(test)]
const CF_PALETTE: u32 = 9;
const CF_UNICODETEXT: u32 = 13;
const CF_ENHMETAFILE: u32 = 14;
const CF_HDROP: u32 = 15;
const CF_DIBV5: u32 = 17;
/// Registered formats start here.
const FIRST_REGISTERED: u32 = 0xC000;

/// `WM_CLIPBOARDUPDATE` comes to `hwnd` from now on.
pub fn add_listener(hwnd: HWND) -> bool {
    unsafe { AddClipboardFormatListener(hwnd) }.is_ok()
}

pub fn remove_listener(hwnd: HWND) {
    unsafe {
        let _ = RemoveClipboardFormatListener(hwnd);
    }
}

/// What `read` found.
pub enum Read {
    /// The island's own write: no entry.
    Own,
    /// Another app holds the clipboard open; try again shortly.
    Busy,
    Snapshot(ClipSnapshot),
}

/// Closes the clipboard when dropped, on every path out.
struct Open;

impl Open {
    fn new(hwnd: HWND) -> Option<Open> {
        unsafe { OpenClipboard(Some(hwnd)) }.ok().map(|()| Open)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Reads the clipboard after a `WM_CLIPBOARDUPDATE`. Password-manager
/// content returns its formats only: nothing of it is copied out.
pub fn read(hwnd: HWND, own_sequence: Option<u32>) -> Read {
    let sequence = unsafe { GetClipboardSequenceNumber() };
    let owner = unsafe { GetClipboardOwner() }.ok();
    if is_own_change(sequence, own_sequence, owner == Some(hwnd)) {
        return Read::Own;
    }
    read_open(hwnd, sequence, false)
}

/// Reads what the clipboard holds right now, once (a note's "Isi dari
/// clipboard"): `read` without the own-copy check, since a copy the island
/// made itself is as good a text as any. `Busy` when another app holds the
/// clipboard open; nothing is retried.
pub fn read_now(hwnd: HWND) -> Read {
    read_open(hwnd, unsafe { GetClipboardSequenceNumber() }, true)
}

/// `once` is the single read for a note's prefill: no picture is read, and a
/// text over the history's cap is read as a bounded prefix (the prefill cuts
/// it to `CLIP_MAX` itself) instead of being left out.
fn read_open(hwnd: HWND, sequence: u32, once: bool) -> Read {
    let Some(open) = Open::new(hwnd) else {
        return Read::Busy;
    };
    let mut ids = Vec::new();
    let mut f = 0;
    loop {
        f = unsafe { EnumClipboardFormats(f) };
        if f == 0 {
            break;
        }
        ids.push(f);
    }
    let mut snap = ClipSnapshot {
        sequence,
        ..ClipSnapshot::default()
    };
    let mut names = Vec::new();
    for &id in ids.iter().filter(|&&id| id >= FIRST_REGISTERED) {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClipboardFormatNameW(id, &mut buf) }.max(0) as usize;
        let name = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
        let dword = match excluded_rule(&name) {
            Some(Rule::ZeroDword) => read_dword(id),
            _ => None,
        };
        names.push(name.clone());
        snap.formats.push((name, dword));
    }
    if should_skip(&snap.formats) {
        drop(open);
        return Read::Snapshot(snap);
    }
    snap.only_bitmaps = picture_only(&ids, &names);
    if ids.contains(&CF_HDROP) {
        snap.files = read_files();
    }
    if snap.files.is_empty() && ids.contains(&CF_UNICODETEXT) {
        snap.text = read_text(once);
    }
    let image = wants_image(
        !snap.files.is_empty(),
        ids.contains(&CF_UNICODETEXT),
        snap.text.as_deref(),
    );
    if !once && image && ids.contains(&CF_DIB) {
        snap.dib = read_global(CF_DIB, MAX_IMAGE_BYTES);
    }
    drop(open);
    Read::Snapshot(snap)
}

/// Registered formats that mean a copy is more than a picture: a browser's
/// or an Office program's "copy image" (it carries markup, rich text or an
/// embedded object), a file drop, a shell item list.
const NOT_A_SCREENSHOT: &[&str] = &[
    "HTML Format",
    "text/html",
    "Rich Text Format",
    "FileGroupDescriptorW",
    "FileGroupDescriptor",
    "FileContents",
    "Shell IDList Array",
    "Csv",
    "Biff12",
    "Biff8",
    "Biff5",
    "Link",
    "Embed Source",
    "Link Source",
    "Object Descriptor",
    "Link Source Descriptor",
];

/// The copy is a picture and nothing else: a screenshot tool offers bitmaps
/// (and "PNG"), whatever else it registers, so this is a deny-list of what a
/// screenshot never carries rather than an allow-list of what it does (which
/// let Snipping Tool's extra formats turn the preview off). A bitmap with
/// text, files or a vector picture beside it is some other program's copy.
pub(crate) fn picture_only(ids: &[u32], names: &[String]) -> bool {
    ids.iter()
        .any(|id| matches!(*id, CF_BITMAP | CF_DIB | CF_DIBV5))
        && !ids.iter().any(|id| {
            matches!(
                *id,
                CF_TEXT | CF_OEMTEXT | CF_UNICODETEXT | CF_HDROP | CF_METAFILEPICT | CF_ENHMETAFILE
            )
        })
        && !names
            .iter()
            .any(|n| NOT_A_SCREENSHOT.iter().any(|d| n.eq_ignore_ascii_case(d)))
}

/// The bitmap is the copy only without files and without text: a text that
/// was offered but not read (over the history's cap, a large Excel range)
/// keeps the bitmap of it out too. Whitespace beside a bitmap does not count.
fn wants_image(has_files: bool, text_offered: bool, text: Option<&str>) -> bool {
    let text_counts = match text {
        Some(t) => !t.trim().is_empty(),
        None => text_offered,
    };
    !has_files && !text_counts
}

/// The bytes of format `f` (the clipboard is open), when not over `max`.
fn read_global(f: u32, max: usize) -> Option<Vec<u8>> {
    read_global_with(f, max, false)
}

/// The first `max` bytes of format `f` (the clipboard is open): a longer
/// block is cut, not left out.
fn read_global_prefix(f: u32, max: usize) -> Option<Vec<u8>> {
    read_global_with(f, max, true)
}

fn read_global_with(f: u32, max: usize, cut: bool) -> Option<Vec<u8>> {
    let handle = unsafe { GetClipboardData(f) }.ok()?;
    let mem = HGLOBAL(handle.0);
    let size = unsafe { GlobalSize(mem) };
    if size == 0 || (size > max && !cut) {
        return None;
    }
    let ptr = unsafe { GlobalLock(mem) } as *const u8;
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, size.min(max)) }.to_vec();
    unsafe {
        let _ = GlobalUnlock(mem);
    }
    Some(bytes)
}

fn read_dword(f: u32) -> Option<u32> {
    let bytes = read_global(f, 64)?;
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

/// A prefill reads this many UTF-16 units of a long text: well beyond what
/// `CLIP_MAX` chars need, even when the clean-up drops blank lines and
/// control characters.
const PREFILL_UNITS: usize = 16 * CLIP_MAX;

/// The text up to its NUL, at most `max_units` UTF-16 units. A cut in the
/// middle of a surrogate pair gives U+FFFD at the end, far past the point
/// where the prefill cuts the text.
fn wide_prefix(bytes: &[u8], max_units: usize) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take(max_units)
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// `prefix` reads a bounded part of any length of text (the note prefill);
/// otherwise a text the history would not keep gives `None`.
fn read_text(prefix: bool) -> Option<String> {
    if prefix {
        let bytes = read_global_prefix(CF_UNICODETEXT, 2 * PREFILL_UNITS)?;
        return Some(wide_prefix(&bytes, PREFILL_UNITS));
    }
    // UTF-16 takes at most twice the UTF-8 cap for the text the history keeps.
    let bytes = read_global(CF_UNICODETEXT, 2 * MAX_TEXT_BYTES + 2)?;
    Some(wide_prefix(&bytes, usize::MAX))
}

fn read_files() -> Vec<PathBuf> {
    let Ok(handle) = (unsafe { GetClipboardData(CF_HDROP) }) else {
        return Vec::new();
    };
    let drop = HDROP(handle.0);
    let count = unsafe { DragQueryFileW(drop, u32::MAX, None) };
    (0..count.min(10_000))
        .filter_map(|i| {
            let len = unsafe { DragQueryFileW(drop, i, None) } as usize;
            let mut buf = vec![0u16; len + 1];
            let n = unsafe { DragQueryFileW(drop, i, Some(&mut buf)) } as usize;
            (n > 0).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..n.min(len)])))
        })
        .collect()
}

/// What goes on the clipboard for `clip`: its own form, or with `plain`
/// only text (paths one per line; nothing for an image).
pub fn formats_for(clip: &Clip, plain: bool) -> Vec<(u32, Vec<u8>)> {
    match (&clip.data, plain) {
        (ClipData::Image(_), true) => Vec::new(),
        (ClipData::Image(img), false) => {
            let dib = match &img.full {
                Some(Original::Raw(raw)) => raw.as_ref().clone(),
                // Decoded here, on the UI thread, which already has COM.
                // A pack that does not decode pastes small like none.
                Some(Original::Packed(packed)) => {
                    super::pack::unpack(packed).unwrap_or_else(|| image::dib_from_thumb(&img.thumb))
                }
                // Restored from the file: the thumbnail is all there is.
                None => image::dib_from_thumb(&img.thumb),
            };
            vec![(CF_DIB, dib)]
        }
        (ClipData::Files(paths), false) => vec![(CF_HDROP, drop_files(paths))],
        _ => clip
            .plain_text()
            .map(|t| vec![(CF_UNICODETEXT, utf16z(&t))])
            .unwrap_or_default(),
    }
}

/// `text` as NUL-terminated UTF-16LE bytes.
pub fn utf16z(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// A `CF_HDROP` block: `DROPFILES` (wide names after its 20 bytes), each
/// path NUL-terminated, and one more NUL.
pub fn drop_files(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(20u32.to_le_bytes()); // pFiles
    out.extend([0u8; 8]); // pt
    out.extend(0u32.to_le_bytes()); // fNC
    out.extend(1u32.to_le_bytes()); // fWide
    for p in paths {
        out.extend(utf16z(&p.to_string_lossy()));
    }
    out.extend([0u8, 0]);
    out
}

/// Puts `formats` on the clipboard as the island. Returns the sequence
/// number after closing, which marks the `WM_CLIPBOARDUPDATE` this causes
/// as the island's own; `None` if it could not. The bytes are borrowed or
/// owned (`B`), so a caller holding a large picture need not copy it.
pub fn write<B: AsRef<[u8]>>(hwnd: HWND, formats: &[(u32, B)]) -> Option<u32> {
    if formats.is_empty() {
        return None;
    }
    // Another app may hold it for a moment; the click waits a little.
    let open = (0..4).find_map(|i| {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        Open::new(hwnd)
    })?;
    unsafe { EmptyClipboard() }.ok()?;
    let mut any = false;
    for (f, bytes) in formats {
        any |= set(*f, bytes.as_ref());
    }
    drop(open);
    any.then(|| unsafe { GetClipboardSequenceNumber() })
}

/// How `write_when` ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrote {
    /// The formats are on the clipboard; the sequence number after closing.
    Written(u32),
    /// `expected` was given and the clipboard has changed since: nothing was
    /// touched.
    NotOurs,
    /// The clipboard stayed held by another app: nothing was touched, try
    /// again shortly.
    Busy,
    /// The clipboard was emptied but none of the formats could be set
    /// (memory ran out): it is empty now, at this sequence number, which is
    /// the writer's own change, not somebody else's copy.
    Emptied(u32),
}

/// `write`, but only while the clipboard's sequence number is still
/// `expected` (`None`: no condition). The number is checked again with the
/// clipboard open and before `EmptyClipboard`, as `empty_if_unchanged` does,
/// so a copy made by someone else since is never replaced. Opens with the
/// same 4 tries 8 ms apart.
pub fn write_when<B: AsRef<[u8]>>(
    hwnd: HWND,
    formats: &[(u32, B)],
    expected: Option<u32>,
) -> Wrote {
    if formats.is_empty() {
        return Wrote::Busy;
    }
    let Some(open) = (0..4).find_map(|i| {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        Open::new(hwnd)
    }) else {
        return Wrote::Busy;
    };
    if expected.is_some_and(|seq| sequence_now() != seq) {
        return Wrote::NotOurs;
    }
    if unsafe { EmptyClipboard() }.is_err() {
        return Wrote::Busy;
    }
    let mut any = false;
    for (f, bytes) in formats {
        any |= set(*f, bytes.as_ref());
    }
    drop(open);
    if any {
        Wrote::Written(sequence_now())
    } else {
        Wrote::Emptied(sequence_now())
    }
}

/// A packed DIB as the clipboard takes it (the screenshot's "Salin lagi").
pub fn image_formats(dib: Vec<u8>) -> Vec<(u32, Vec<u8>)> {
    vec![(CF_DIB, dib)]
}

/// The clipboard's sequence number now.
pub fn sequence_now() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// The screenshot's "Hapus": empties the clipboard, but only while its
/// sequence number is still `recorded` (`screenshot::should_empty`), checked
/// again with the clipboard open, so a copy made since is never lost.
/// Returns the sequence number after emptying (the island's own change), or
/// `None` when it left the clipboard alone.
pub fn empty_if_unchanged(hwnd: HWND, recorded: u32) -> Option<u32> {
    use crate::screenshot::should_empty;
    if !should_empty(sequence_now(), recorded) {
        return None;
    }
    let open = (0..4).find_map(|i| {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        Open::new(hwnd)
    })?;
    if !should_empty(sequence_now(), recorded) {
        return None;
    }
    unsafe { EmptyClipboard() }.ok()?;
    drop(open);
    Some(sequence_now())
}

fn set(f: u32, bytes: &[u8]) -> bool {
    let Ok(mem) = (unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) }) else {
        return false;
    };
    let ptr = unsafe { GlobalLock(mem) } as *mut u8;
    if ptr.is_null() {
        unsafe {
            let _ = GlobalFree(Some(mem));
        }
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(mem);
    }
    // On success the clipboard owns the memory; on failure it is ours to free.
    if unsafe { SetClipboardData(f, Some(HANDLE(mem.0))) }.is_ok() {
        true
    } else {
        unsafe {
            let _ = GlobalFree(Some(mem));
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::kinds::ClipKind;

    #[test]
    fn text_too_big_to_keep_is_not_kept_as_a_picture_either() {
        // A large Excel range: text over the cap plus a bitmap of the cells.
        assert!(!wants_image(false, true, None));
        // Plain text: kept as text.
        assert!(!wants_image(false, true, Some("hello")));
        // Only a bitmap, or whitespace beside it: the image.
        assert!(wants_image(false, false, None));
        assert!(wants_image(false, true, Some("  ")));
        // Files win.
        assert!(!wants_image(true, false, None));
    }

    fn utf16_bytes(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn a_long_text_prefix_ends_in_a_split_pair_that_the_prefill_cuts_away() {
        // A copy of a whole document: the pair of the emoji straddles the cut.
        let raw = format!("{}\u{1F600}tail", "a".repeat(PREFILL_UNITS - 1));
        let bytes = utf16_bytes(&raw);
        assert!(bytes.len() > 2 * PREFILL_UNITS);
        let prefix = wide_prefix(&bytes, PREFILL_UNITS);
        assert_eq!(prefix.chars().count(), PREFILL_UNITS);
        assert!(prefix.ends_with('\u{FFFD}'));
        let note = crate::widgets::cliptext::prefill(&prefix).unwrap();
        assert_eq!(note.chars().count(), CLIP_MAX);
        assert!(!note.contains('\u{FFFD}'));
    }

    #[test]
    fn a_text_prefix_stops_at_the_nul_and_keeps_a_short_text_whole() {
        let mut bytes = utf16_bytes("abc");
        bytes.extend([0, 0, b'x', 0]);
        assert_eq!(wide_prefix(&bytes, PREFILL_UNITS), "abc");
        assert_eq!(wide_prefix(&utf16_bytes("h\u{E9}llo"), usize::MAX), "h\u{E9}llo");
    }

    fn clip(data: ClipData, kind: ClipKind) -> Clip {
        Clip {
            id: 1,
            kind,
            data,
            pinned: false,
            at: 0,
        }
    }

    #[test]
    fn formats_follow_the_kind_and_plain_text() {
        let text = clip(ClipData::Text("hé".into()), ClipKind::Text);
        assert_eq!(
            formats_for(&text, false),
            vec![(CF_UNICODETEXT, vec![b'h', 0, 0xE9, 0, 0, 0])]
        );
        assert_eq!(formats_for(&text, true), formats_for(&text, false));
        let files = clip(
            ClipData::Files(vec![PathBuf::from("C:\\a"), PathBuf::from("D:\\b")]),
            ClipKind::Files,
        );
        let own = formats_for(&files, false);
        assert_eq!(own[0].0, CF_HDROP);
        let plain = formats_for(&files, true);
        assert_eq!(plain, vec![(CF_UNICODETEXT, utf16z("C:\\a\r\nD:\\b"))]);
    }

    #[test]
    fn drop_files_layout() {
        let d = drop_files(&[PathBuf::from("C:\\a")]);
        assert_eq!(&d[..4], &20u32.to_le_bytes());
        assert_eq!(&d[16..20], &1u32.to_le_bytes());
        // "C:\a" NUL, then the closing NUL.
        let names: Vec<u16> = d[20..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        assert_eq!(names, [67, 58, 92, 97, 0, 0]);
    }
}

#[cfg(test)]
mod picture_tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_screenshot_tool_with_any_extra_registered_format_still_counts() {
        // The old allow-list wanted every registered name to be exactly "PNG".
        assert!(picture_only(&[CF_DIB, CF_DIBV5], &names(&["PNG"])));
        assert!(picture_only(
            &[CF_BITMAP, CF_DIB, CF_DIBV5],
            &names(&[
                "PNG",
                "image/png",
                "Preferred DropEffect",
                "MSDEVColumnSelect"
            ])
        ));
        assert!(picture_only(&[CF_DIB], &[]));
    }

    #[test]
    fn a_browsers_or_offices_copy_is_not_a_screenshot() {
        assert!(!picture_only(
            &[CF_DIB, CF_DIBV5],
            &names(&["PNG", "HTML Format"])
        ));
        assert!(
            !picture_only(&[CF_DIB], &names(&["text/HTML"])),
            "case-insensitive"
        );
        assert!(!picture_only(&[CF_DIB], &names(&["Rich Text Format"])));
        assert!(!picture_only(&[CF_DIB, CF_ENHMETAFILE], &[]));
        assert!(
            !picture_only(&[CF_DIB, CF_UNICODETEXT], &[]),
            "a picture with text"
        );
        assert!(!picture_only(&[CF_DIB, CF_HDROP], &[]), "files");
        assert!(!picture_only(&[CF_DIB], &names(&["Embed Source", "Link"])));
    }

    #[test]
    fn without_a_bitmap_nothing_is_a_screenshot() {
        assert!(!picture_only(&[], &[]));
        assert!(!picture_only(&[CF_UNICODETEXT], &[]));
        assert!(!picture_only(&[CF_PALETTE], &names(&["PNG"])));
    }
}
