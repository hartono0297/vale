//! The clipboard of a sticky note, the Win32 side: writing a copy (plain
//! Unicode text plus the note's own `Vale.NoteRich` payload, `richclip`) and
//! reading what a paste carries, in the order a note decides it.
//!
//! The island's clipboard-history rules ([[clipboard-privacy]]) hold: this
//! only runs for a copy or a paste the owner makes in a note; the text is
//! never logged; an `OpenClipboard` that fails (another app holds it) is
//! simply "nothing". **No test and no demo calls the functions that touch the
//! clipboard here**; `order` (the decision) is pure and tested.

use std::path::PathBuf;

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::core::{HSTRING, w};

use crate::widgets::image_io::Source;
use crate::widgets::images::MAX_SOURCE_BYTES;
use crate::widgets::richclip;

const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;
/// Text longer than this is not pasted whole (a note holds 20 000).
const MAX_TEXT_UNITS: usize = 200_000;
/// A file list longer than this is cut (more than the cap can take anyway).
const MAX_FILES: u32 = 64;

/// What a paste carries, decided in a note's order.
#[derive(Debug)]
pub enum Paste {
    /// Plain text (and the note's own runs when `Vale.NoteRich` came with it).
    Text { text: String, rich: Option<Vec<u8>> },
    /// Files (the caller keeps the images).
    Files(Vec<PathBuf>),
    /// A bitmap, as bytes for the worker.
    Image(Source),
    /// A bitmap over the 25 MiB cap.
    TooBig,
    Nothing,
}

/// What a paste is, from what the clipboard offers: text first (Excel and
/// Word put a bitmap beside their text), then files, then a bitmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Files,
    Bitmap,
    Nothing,
}

pub fn order(text_nonempty: bool, has_files: bool, has_bitmap: bool) -> Kind {
    if text_nonempty {
        Kind::Text
    } else if has_files {
        Kind::Files
    } else if has_bitmap {
        Kind::Bitmap
    } else {
        Kind::Nothing
    }
}

fn rich_format() -> u32 {
    unsafe { RegisterClipboardFormatW(&HSTRING::from(richclip::FORMAT_NAME)) }
}

fn png_format() -> u32 {
    unsafe { RegisterClipboardFormatW(w!("PNG")) }
}

/// The bytes of clipboard format `id`, at most `cap` of them (`None` when it
/// is absent or bigger; `Err(())` for bigger so a bitmap can say so).
fn read_format(id: u32, cap: usize) -> Option<Result<Vec<u8>, ()>> {
    unsafe {
        let handle: HANDLE = GetClipboardData(id).ok()?;
        let mem = HGLOBAL(handle.0);
        let size = GlobalSize(mem);
        if size == 0 {
            return None;
        }
        if size > cap {
            return Some(Err(()));
        }
        let ptr = GlobalLock(mem) as *const u8;
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(mem);
        Some(Ok(bytes))
    }
}

fn read_unicode() -> Option<String> {
    let bytes = read_format(CF_UNICODETEXT, MAX_TEXT_UNITS * 2).and_then(Result::ok)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    Some(String::from_utf16_lossy(&units[..end]))
}

fn read_files() -> Vec<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let mut out = Vec::new();
    unsafe {
        let Ok(handle) = GetClipboardData(CF_HDROP) else {
            return out;
        };
        let drop = HDROP(handle.0);
        let count = DragQueryFileW(drop, u32::MAX, None).min(MAX_FILES);
        for i in 0..count {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let n = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
            out.push(PathBuf::from(std::ffi::OsString::from_wide(&buf[..n.min(len)])));
        }
    }
    out
}

/// What the clipboard carries now, read on the UI thread with a byte cap. A
/// busy clipboard is `Nothing`.
pub fn read_paste(owner: HWND) -> Paste {
    if unsafe { OpenClipboard(Some(owner)) }.is_err() {
        return Paste::Nothing;
    }
    let result = (|| {
        let text = unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) }
            .is_ok()
            .then(read_unicode)
            .flatten()
            .filter(|t| !t.is_empty());
        let has_files = unsafe { IsClipboardFormatAvailable(CF_HDROP) }.is_ok();
        let png = png_format();
        let has_bitmap = [png, CF_DIBV5, CF_DIB]
            .iter()
            .any(|f| unsafe { IsClipboardFormatAvailable(*f) }.is_ok());
        match order(text.is_some(), has_files, has_bitmap) {
            Kind::Text => {
                let text = text.unwrap_or_default();
                let rich = read_format(rich_format(), richclip::MAX_BYTES).and_then(Result::ok);
                Paste::Text { text, rich }
            }
            Kind::Files => Paste::Files(read_files()),
            Kind::Bitmap => {
                let cap = MAX_SOURCE_BYTES as usize;
                for (id, make) in [
                    (png, Source::Png as fn(Vec<u8>) -> Source),
                    (CF_DIBV5, Source::Dib),
                    (CF_DIB, Source::Dib),
                ] {
                    if unsafe { IsClipboardFormatAvailable(id) }.is_err() {
                        continue;
                    }
                    match read_format(id, cap) {
                        Some(Ok(bytes)) => return Paste::Image(make(bytes)),
                        Some(Err(())) => return Paste::TooBig,
                        None => {}
                    }
                }
                Paste::Nothing
            }
            Kind::Nothing => Paste::Nothing,
        }
    })();
    unsafe {
        let _ = CloseClipboard();
    }
    result
}

/// A global memory block holding `bytes` (zero-terminated by the caller when
/// text), for the clipboard to own.
fn global_of(bytes: &[u8]) -> Option<HGLOBAL> {
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).ok()?;
        let ptr = GlobalLock(mem) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(mem));
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(mem);
        Some(mem)
    }
}

/// Puts `text` (`\r\n` breaks) and the note's `rich` payload on the clipboard.
/// False when another app holds it or it could not be filled.
pub fn write_note(owner: HWND, text: &str, rich: &[u8]) -> bool {
    if unsafe { OpenClipboard(Some(owner)) }.is_err() {
        return false;
    }
    let mut ok = false;
    unsafe {
        if EmptyClipboard().is_ok() {
            let mut wide: Vec<u8> = text
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            wide.shrink_to_fit();
            if let Some(mem) = global_of(&wide) {
                if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).is_ok() {
                    ok = true;
                } else {
                    let _ = GlobalFree(Some(mem));
                }
            }
            if ok && let Some(mem) = global_of(rich) {
                // Best effort: the plain text is already there.
                if SetClipboardData(rich_format(), Some(HANDLE(mem.0))).is_err() {
                    let _ = GlobalFree(Some(mem));
                }
            }
        }
        let _ = CloseClipboard();
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_wins_over_a_bitmap_and_files_over_a_bitmap() {
        // Excel and Word put a bitmap beside their text: it is a text paste.
        assert_eq!(order(true, false, true), Kind::Text);
        assert_eq!(order(true, true, true), Kind::Text);
        assert_eq!(order(false, true, true), Kind::Files);
        assert_eq!(order(false, true, false), Kind::Files);
        assert_eq!(order(false, false, true), Kind::Bitmap);
        assert_eq!(order(false, false, false), Kind::Nothing);
    }
}
