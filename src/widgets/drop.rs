//! Dropping onto a sticky note: each note window is an OLE drop target (like
//! the island's own, `shelf_ole`) that adds what was dropped as new lines at
//! the end of the note.
//!
//! What it takes, in this order: files (CF_HDROP, each full path on a line,
//! at most `MAX_PATHS`), a link (`UniformResourceLocatorW`, then the ANSI
//! `UniformResourceLocator`, one line), text (CF_UNICODETEXT). Anything else
//! is `DROPEFFECT_NONE`, with no highlight. The answer is link for files when
//! offered, else copy; copy for a link or text. Never move: a move would make
//! the source app delete what was dragged.
//!
//! The callbacks reach the host's state only through `host::drop_hover` and
//! `host::drop_into`, which use its `try_borrow_mut`, and never borrow the
//! island: the shelf's own `DoDragDrop` may be pumping this thread. A drop
//! target costs nothing while idle (no timer, no listener).
//!
//! The pure parts (`drop_lines`, `append_lines`, `pick`, `effect_for`) are
//! tested without a window.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HGLOBAL, HWND, POINTL, S_OK};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{
    CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget,
    IDropTarget_Impl, OleInitialize, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::core::{Ref, implement, w};

use crate::shelf_ole::{choose_effect, read_paths_max};
use crate::widgets::cliptext::{cap, normalize};
use crate::widgets::host;

/// The most paths one drop adds.
pub const MAX_PATHS: usize = 32;

/// The most bytes read from a dropped text: the block is cut to `CLIP_MAX`
/// chars anyway, and a drag can carry a whole book.
const MAX_READ_BYTES: usize = 512 * 1024;

/// What a drop carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropKind {
    Files(Vec<PathBuf>),
    Url(String),
    Text(String),
}

/// The lines a drop adds. Text goes through the clip clean-up (`cliptext`)
/// with its blank lines removed; a link is one trimmed line; each path is
/// trimmed, at most `MAX_PATHS` of them. Empty means nothing happens.
pub fn drop_lines(kind: DropKind) -> Vec<String> {
    match kind {
        DropKind::Files(paths) => paths
            .iter()
            .map(|p| p.to_string_lossy().trim().to_owned())
            .filter(|p| !p.is_empty())
            .take(MAX_PATHS)
            .collect(),
        DropKind::Url(url) => {
            let line = url
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(|l| l.chars().filter(|c| !c.is_control()).collect::<String>())
                .unwrap_or_default();
            let line = line.trim();
            if line.is_empty() {
                Vec::new()
            } else {
                vec![line.to_owned()]
            }
        }
        DropKind::Text(text) => normalize(&text)
            .split('\n')
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

/// `text` with `lines` added at the end. An empty note becomes just the
/// lines joined with LF; otherwise `text`, an LF (none when it already ends
/// with one), then the lines. The added block is cut to `CLIP_MAX` chars.
pub fn append_lines(text: &str, lines: &[String]) -> String {
    let block = cap(&lines.join("\n"));
    if block.is_empty() {
        return text.to_owned();
    }
    if text.is_empty() {
        return block;
    }
    let sep = if text.ends_with('\n') { "" } else { "\n" };
    format!("{text}{sep}{block}")
}

/// A format a note takes, in priority order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    Files,
    UrlW,
    UrlA,
    Text,
}

impl Offer {
    pub const PRIORITY: [Offer; 4] = [Offer::Files, Offer::UrlW, Offer::UrlA, Offer::Text];
}

/// The first format in priority order that `has` says the source offers.
pub fn pick(has: impl Fn(Offer) -> bool) -> Option<Offer> {
    Offer::PRIORITY.into_iter().find(|o| has(*o))
}

/// The effect to answer, from the ones the source `offered`: link for files
/// when offered, else copy; copy for a link or text. Never move.
pub fn effect_for(offer: Offer, offered: u32) -> u32 {
    match offer {
        Offer::Files => choose_effect(offered),
        Offer::UrlW | Offer::UrlA | Offer::Text => {
            if offered & DROPEFFECT_COPY.0 != 0 {
                DROPEFFECT_COPY.0
            } else {
                DROPEFFECT_NONE.0
            }
        }
    }
}

// ------------------------------------------------------------------ OLE

fn url_format(offer: Offer) -> u32 {
    match offer {
        Offer::UrlW => unsafe { RegisterClipboardFormatW(w!("UniformResourceLocatorW")) },
        _ => unsafe { RegisterClipboardFormatW(w!("UniformResourceLocator")) },
    }
}

fn format_id(offer: Offer) -> u32 {
    match offer {
        Offer::Files => u32::from(CF_HDROP.0),
        Offer::Text => u32::from(CF_UNICODETEXT.0),
        Offer::UrlW | Offer::UrlA => url_format(offer),
    }
}

fn formatetc(id: u32) -> FORMATETC {
    FORMATETC {
        // Registered formats are 0xC000 and up: they fit a u16.
        cfFormat: id as u16,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn offers(data: &IDataObject, offer: Offer) -> bool {
    let id = format_id(offer);
    id != 0 && unsafe { data.QueryGetData(&formatetc(id)) } == S_OK
}

/// The first of `Offer::PRIORITY` that `data` has.
fn offered_by(data: &IDataObject) -> Option<Offer> {
    pick(|o| offers(data, o))
}

/// The bytes of an HGLOBAL format, at most `MAX_READ_BYTES`.
fn read_bytes(data: &IDataObject, id: u32) -> Option<Vec<u8>> {
    let mut medium = unsafe { data.GetData(&formatetc(id)) }.ok()?;
    let mut out = None;
    unsafe {
        if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            let mem: HGLOBAL = medium.u.hGlobal;
            if !mem.is_invalid() {
                let size = GlobalSize(mem).min(MAX_READ_BYTES);
                let ptr = GlobalLock(mem) as *const u8;
                if !ptr.is_null() {
                    out = Some(std::slice::from_raw_parts(ptr, size).to_vec());
                    let _ = GlobalUnlock(mem);
                }
            }
        }
        ReleaseStgMedium(&mut medium);
    }
    out
}

fn wide_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn ansi_text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// What `data` carries, in the format `offer`.
fn read(data: &IDataObject, offer: Offer) -> Option<DropKind> {
    match offer {
        Offer::Files => Some(DropKind::Files(read_paths_max(data, MAX_PATHS))),
        Offer::UrlW => read_bytes(data, format_id(offer)).map(|b| DropKind::Url(wide_text(&b))),
        Offer::UrlA => read_bytes(data, format_id(offer)).map(|b| DropKind::Url(ansi_text(&b))),
        Offer::Text => read_bytes(data, format_id(offer)).map(|b| DropKind::Text(wide_text(&b))),
    }
}

#[implement(IDropTarget)]
struct NoteDrop {
    hwnd: HWND,
    /// The drag in progress carries something a note takes.
    accepting: Cell<Option<Offer>>,
}

impl NoteDrop {
    fn effect(&self, effect: *mut DROPEFFECT) {
        if effect.is_null() {
            return;
        }
        unsafe {
            *effect = match self.accepting.get() {
                Some(offer) => DROPEFFECT(effect_for(offer, (*effect).0)),
                None => DROPEFFECT_NONE,
            };
        }
    }
}

impl Drop for NoteDrop {
    fn drop(&mut self) {
        #[cfg(test)]
        LIVE.with(|n| n.set(n.get().saturating_sub(1)));
    }
}

impl IDropTarget_Impl for NoteDrop_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let offer = data.as_ref().and_then(offered_by);
        self.accepting.set(offer);
        self.effect(effect);
        // A source that offers nothing we can answer gets no highlight.
        let answered = !effect.is_null() && unsafe { (*effect).0 } != DROPEFFECT_NONE.0;
        if offer.is_some() && answered {
            host::drop_hover(self.hwnd, true);
        } else {
            self.accepting.set(None);
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.effect(effect);
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.accepting.set(None);
        host::drop_hover(self.hwnd, false);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let offer = data.as_ref().and_then(offered_by).or(self.accepting.get());
        self.accepting.set(offer);
        self.effect(effect);
        self.accepting.set(None);
        host::drop_hover(self.hwnd, false);
        let lines = match (data.as_ref(), offer) {
            (Some(data), Some(offer)) => read(data, offer).map(drop_lines).unwrap_or_default(),
            _ => Vec::new(),
        };
        if lines.is_empty() {
            if !effect.is_null() {
                unsafe { *effect = DROPEFFECT_NONE };
            }
        } else {
            host::drop_into(self.hwnd, &lines);
        }
        Ok(())
    }
}

/// Said once: OLE would not start on a thread, or a window would not take
/// drops. Never fatal; the note just takes no drops.
fn log_once(what: &str, error: &windows::core::Error) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        eprintln!("vale: no drops onto notes ({what}): {error}");
    }
}

thread_local! {
    /// `OleInitialize` was tried on this thread, and whether it worked.
    static OLE: Cell<Option<bool>> = const { Cell::new(None) };
    /// Drop targets alive on this thread (tests count them to see that every
    /// destroyed window revoked its own).
    #[cfg(test)]
    static LIVE: Cell<usize> = const { Cell::new(0) };
}

/// OLE is up on this thread. On the island's thread COM is already an
/// apartment, so this answers S_FALSE; the demo thread gets OLE from this
/// call. A thread in another COM mode cannot, and is said once.
fn ole_ready() -> bool {
    OLE.with(|state| {
        if let Some(ok) = state.get() {
            return ok;
        }
        let ok = match unsafe { OleInitialize(None) } {
            Ok(()) => true,
            Err(error) => {
                log_once("OleInitialize", &error);
                false
            }
        };
        state.set(Some(ok));
        ok
    })
}

/// Makes note window `hwnd` take drops. Right after the window is created;
/// `revoke` goes in its `WM_DESTROY`.
pub fn register(hwnd: HWND) {
    if !ole_ready() {
        return;
    }
    let target: IDropTarget = NoteDrop {
        hwnd,
        accepting: Cell::new(None),
    }
    .into();
    match unsafe { RegisterDragDrop(hwnd, &target) } {
        Ok(()) => {
            #[cfg(test)]
            LIVE.with(|n| n.set(n.get() + 1));
        }
        Err(error) => log_once("RegisterDragDrop", &error),
    }
}

/// Before the window goes (its `WM_DESTROY`).
pub fn revoke(hwnd: HWND) {
    unsafe {
        let _ = RevokeDragDrop(hwnd);
    }
}

/// How many drop targets of this thread are alive.
#[cfg(test)]
pub fn live_targets() -> usize {
    LIVE.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::cliptext::CLIP_MAX;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn dropped_text_is_cleaned_and_its_blank_lines_go() {
        assert_eq!(
            drop_lines(DropKind::Text("  one\r\n\r\n\r\ntwo \r\n\tthree\n\n".into())),
            s(&["one", "two", "    three"])
        );
        assert_eq!(drop_lines(DropKind::Text("a\u{0}b".into())), s(&["ab"]));
        assert!(drop_lines(DropKind::Text("  \r\n \t ".into())).is_empty());
        assert!(drop_lines(DropKind::Text(String::new())).is_empty());
    }

    #[test]
    fn a_link_is_one_trimmed_line() {
        assert_eq!(
            drop_lines(DropKind::Url("  https://example.com/a?b=1 \r\n".into())),
            s(&["https://example.com/a?b=1"])
        );
        assert_eq!(
            drop_lines(DropKind::Url("\nhttps://a.example\nhttps://b.example".into())),
            s(&["https://a.example"]),
            "the first line only"
        );
        assert!(drop_lines(DropKind::Url("  \u{0}".into())).is_empty());
    }

    #[test]
    fn files_are_their_full_paths_one_per_line_at_most_32() {
        let files = vec![
            PathBuf::from(r"C:\Users\me\Desktop\a.txt"),
            PathBuf::from(r" D:\x y\b.png "),
            PathBuf::new(),
        ];
        assert_eq!(
            drop_lines(DropKind::Files(files)),
            s(&[r"C:\Users\me\Desktop\a.txt", r"D:\x y\b.png"])
        );
        let many: Vec<PathBuf> = (0..50).map(|i| PathBuf::from(format!(r"C:\f{i}.txt"))).collect();
        let lines = drop_lines(DropKind::Files(many));
        assert_eq!(lines.len(), MAX_PATHS);
        assert_eq!(lines[31], r"C:\f31.txt");
        assert!(drop_lines(DropKind::Files(Vec::new())).is_empty());
    }

    #[test]
    fn lines_go_at_the_end_of_the_note() {
        let lines = s(&["a", "b"]);
        assert_eq!(append_lines("", &lines), "a\nb", "an empty note is just the lines");
        assert_eq!(append_lines("Belanja", &lines), "Belanja\na\nb");
        assert_eq!(
            append_lines("Belanja\n", &lines),
            "Belanja\na\nb",
            "no extra break when the text ends with one"
        );
        assert_eq!(append_lines("x\n\n", &lines), "x\n\na\nb");
        assert_eq!(append_lines("keep", &[]), "keep", "nothing to add");
        assert_eq!(append_lines("keep", &s(&[""])), "keep");
    }

    #[test]
    fn the_added_block_is_capped_but_the_note_is_not() {
        let note = "n".repeat(5000);
        let long = vec!["x".repeat(99); 40];
        let got = append_lines(&note, &long);
        let block = &got[note.len() + 1..];
        assert!(block.chars().count() <= CLIP_MAX, "{}", block.chars().count());
        assert!(block.chars().count() > CLIP_MAX - 200);
        assert!(got.starts_with(&note), "what was there stays");
        // A single huge line is cut at the cap.
        let got = append_lines("", &[ "y".repeat(CLIP_MAX + 500) ]);
        assert_eq!(got.chars().count(), CLIP_MAX);
    }

    #[test]
    fn files_win_over_a_link_and_a_link_over_text() {
        let all = |_: Offer| true;
        assert_eq!(pick(all), Some(Offer::Files));
        assert_eq!(pick(|o| o != Offer::Files), Some(Offer::UrlW));
        assert_eq!(pick(|o| matches!(o, Offer::UrlA | Offer::Text)), Some(Offer::UrlA));
        assert_eq!(pick(|o| o == Offer::Text), Some(Offer::Text));
        assert_eq!(pick(|_| false), None, "an image or anything else: no drop");
    }

    #[test]
    fn a_drop_answers_link_for_files_else_copy_and_never_move() {
        let (copy, mv, link) = (DROPEFFECT_COPY.0, 2, 4);
        assert_eq!(effect_for(Offer::Files, copy | mv | link), link);
        assert_eq!(effect_for(Offer::Files, copy | mv), copy);
        for offer in [Offer::UrlW, Offer::UrlA, Offer::Text] {
            assert_eq!(effect_for(offer, copy | mv | link), copy, "{offer:?}");
            assert_eq!(effect_for(offer, copy), copy);
            assert_eq!(effect_for(offer, mv), DROPEFFECT_NONE.0, "move only: refused");
            assert_eq!(effect_for(offer, 0), DROPEFFECT_NONE.0);
        }
        assert_eq!(effect_for(Offer::Files, mv), DROPEFFECT_NONE.0);
    }

    #[test]
    fn utf16_and_ansi_texts_stop_at_their_nul() {
        let bytes: Vec<u8> = "Jl. Kenanga \u{1F600}"
            .encode_utf16()
            .chain([0, u16::from(b'x')])
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(wide_text(&bytes), "Jl. Kenanga \u{1F600}");
        assert_eq!(ansi_text(b"https://a.example\0junk"), "https://a.example");
        assert_eq!(ansi_text(b"abc"), "abc");
    }
}
