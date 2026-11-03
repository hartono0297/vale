//! The keyboard of a sticky note: one hidden multi-line EDIT, a child of the
//! note's window, made when the note gets the keyboard and destroyed when it
//! lets go (the project's pattern, `ui/cli_input.rs`). A layered window shows
//! only its own bitmap, never its children, so the EDIT is never drawn; the
//! note draws the text, the selection and the caret itself. The EDIT is there
//! for what a real control gives for free: the IME, paste, copy, cut, undo,
//! word moves and selection.
//!
//! The checklist keys go through `checklist`: a space after a line-start `-`
//! makes a box, Enter continues or ends a list, Backspace at the start of an
//! item's text takes the whole marker, and the caret never rests inside a
//! marker. Those edits are made with `EM_REPLACESEL` (undo allowed), so Ctrl+Z
//! undoes them.
//!
//! Indices outside this file are UTF-16 units of the text with `\n` breaks;
//! the EDIT counts `\r\n` as two, `checklist::edit_index` converts. That
//! holds only while every break in the EDIT is a `\r\n` pair: a plain
//! multi-line EDIT keeps a pasted lone `\n` or `\r` as one unit. So a paste
//! goes in with pairs (`paste`, undo kept), and `text` rewrites any lone break
//! that still got in before anything reads an index.

use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Controls::{EM_GETSEL, EM_LIMITTEXT, EM_REPLACESEL, EM_SETSEL};
use windows::Win32::UI::Input::Ime::{
    CFS_POINT, COMPOSITIONFORM, ImmGetContext, ImmReleaseContext, ImmSetCompositionWindow,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_A, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_SHIFT, VK_UP,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DLGC_WANTALLKEYS, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN, GetParent,
    GetWindowTextLengthW, GetWindowTextW, HMENU, SendMessageW, SetWindowTextW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CHAR, WM_CLEAR, WM_CUT, WM_GETDLGCODE, WM_IME_COMPOSITION,
    WM_IME_STARTCOMPOSITION, WM_KEYDOWN, WM_KILLFOCUS, WM_NCDESTROY, WM_PASTE, WM_SETFOCUS,
    WM_UNDO, WS_CHILD, WS_VISIBLE,
};
use windows::core::{HSTRING, Result, w};

use crate::widgets::checklist::{self, Edit};
use crate::widgets::host;

/// What the EDIT tells the note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditEvent {
    /// The text or the selection may have changed: read both.
    Changed,
    /// Esc: leave editing.
    Escape,
    /// An arrow up or down: the note's layout knows where that goes.
    Vertical { up: bool, extend: bool },
    /// The EDIT lost the keyboard.
    Blur,
}

const SUBCLASS_ID: usize = 7;
/// A note is a note, not a document.
const LIMIT: usize = 20_000;

/// The EDIT's style. `ES_AUTOHSCROLL` turns its own word wrap off: it would
/// wrap with its own font, which is not the drawn layout, so Home and End
/// (and their Shift forms) would stop at line ends the user never sees.
/// Without the wrap they go to the start and end of the line as typed; Up and
/// Down follow the drawn lines (`EditEvent::Vertical`).
fn style() -> WINDOW_STYLE {
    WS_CHILD
        | WS_VISIBLE
        | WINDOW_STYLE((ES_MULTILINE | ES_WANTRETURN | ES_AUTOVSCROLL | ES_AUTOHSCROLL) as u32)
}

/// A hidden EDIT in `parent`'s corner holding `text` (model text, `\n`
/// breaks), `size` px (the note's text area).
pub fn create(parent: HWND, text: &str, size: (i32, i32)) -> Result<HWND> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let style = style();
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            w!(""),
            style,
            0,
            0,
            size.0.max(40),
            size.1.max(40),
            Some(parent),
            Some(HMENU(401 as *mut _)),
            Some(instance.into()),
            None,
        )?
    };
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID, 0);
        SendMessageW(hwnd, EM_LIMITTEXT, Some(WPARAM(LIMIT)), Some(LPARAM(0)));
        let _ = SetWindowTextW(hwnd, &HSTRING::from(checklist::to_edit_text(text)));
    }
    Ok(hwnd)
}

/// The EDIT's own text, breaks as they are.
fn raw_text(edit: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(edit) }.max(0) as usize;
    let mut buf = vec![0u16; len + 1];
    let n = unsafe { GetWindowTextW(edit, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n.min(len)])
}

/// The EDIT's text as model text (`\n` breaks). A lone `\n` or `\r` that got
/// into the EDIT is rewritten as `\r\n` first, the selection kept, so the
/// EDIT and the model count the same again (`selection`, `set_selection` and
/// `apply` rely on it, and they read after this).
pub fn text(edit: HWND) -> String {
    let raw = raw_text(edit);
    let model = checklist::normalize(&raw);
    if checklist::has_lone_break(&raw) {
        let (a, b) = raw_selection(edit);
        let (a, b) = (
            checklist::raw_model_index(&raw, a),
            checklist::raw_model_index(&raw, b),
        );
        unsafe {
            let _ = SetWindowTextW(edit, &HSTRING::from(checklist::to_edit_text(&model)));
        }
        set_selection(edit, &model, a, b);
    }
    model
}

/// The clipboard's text, when it has some (and not far more than a note
/// holds).
fn clipboard_text(edit: HWND) -> Option<String> {
    const CF_UNICODETEXT: u32 = 13;
    unsafe { OpenClipboard(Some(edit)) }.ok()?;
    let read = || {
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.ok()?;
        let mem = HGLOBAL(handle.0);
        let units = unsafe { GlobalSize(mem) } / 2;
        if units == 0 || units > 4 * LIMIT {
            return None;
        }
        let ptr = unsafe { GlobalLock(mem) } as *const u16;
        if ptr.is_null() {
            return None;
        }
        let all = unsafe { std::slice::from_raw_parts(ptr, units) };
        let end = all.iter().position(|&u| u == 0).unwrap_or(units);
        let text = String::from_utf16_lossy(&all[..end]);
        unsafe {
            let _ = GlobalUnlock(mem);
        }
        Some(text)
    };
    let text = read();
    unsafe {
        let _ = CloseClipboard();
    }
    text
}

/// A paste whose text has a lone break (an LF-only source): it goes in with
/// `\r\n` pairs through `EM_REPLACESEL`, so Ctrl+Z still undoes it. False
/// when the EDIT's own paste can do it.
fn paste(edit: HWND) -> bool {
    let Some(clip) = clipboard_text(edit) else {
        return false;
    };
    if !checklist::has_lone_break(&clip) {
        return false;
    }
    let wide: Vec<u16> = checklist::to_edit_text(&clip)
        .encode_utf16()
        .chain([0])
        .collect();
    unsafe {
        SendMessageW(
            edit,
            EM_REPLACESEL,
            Some(WPARAM(1)),
            Some(LPARAM(wide.as_ptr() as isize)),
        );
    }
    true
}

fn raw_selection(edit: HWND) -> (usize, usize) {
    let (mut a, mut b) = (0u32, 0u32);
    unsafe {
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut a as *mut u32 as usize)),
            Some(LPARAM(&mut b as *mut u32 as isize)),
        );
    }
    (a as usize, b as usize)
}

/// The selection as model indices, anchor first (`.0` may be after `.1`).
pub fn selection(edit: HWND, model_text: &str) -> (usize, usize) {
    let (a, b) = raw_selection(edit);
    (
        checklist::model_index(model_text, a),
        checklist::model_index(model_text, b),
    )
}

/// Selects model indices `a` to `b` (a caret when equal).
pub fn set_selection(edit: HWND, model_text: &str, a: usize, b: usize) {
    unsafe {
        SendMessageW(
            edit,
            EM_SETSEL,
            Some(WPARAM(checklist::edit_index(model_text, a))),
            Some(LPARAM(checklist::edit_index(model_text, b) as isize)),
        );
    }
}

/// Makes `edit` (an `Edit` against `model_text`) through the EDIT, so it can
/// be undone, and puts the caret where the edit says.
pub fn apply(edit: HWND, model_text: &str, e: &Edit) {
    let wide: Vec<u16> = checklist::to_edit_text(&e.insert)
        .encode_utf16()
        .chain([0])
        .collect();
    let new_text = checklist::apply(model_text, e);
    unsafe {
        SendMessageW(
            edit,
            EM_SETSEL,
            Some(WPARAM(checklist::edit_index(model_text, e.start))),
            Some(LPARAM(checklist::edit_index(model_text, e.end) as isize)),
        );
        SendMessageW(
            edit,
            EM_REPLACESEL,
            Some(WPARAM(1)),
            Some(LPARAM(wide.as_ptr() as isize)),
        );
    }
    set_selection(edit, &new_text, e.caret, e.caret);
}

/// Puts the IME's composition window at (`x`, `y`) in the note window's
/// client pixels (the EDIT sits at the origin, so they are its own).
pub fn place_ime(edit: HWND, x: i32, y: i32) {
    unsafe {
        let imc = ImmGetContext(edit);
        if imc.is_invalid() {
            return;
        }
        let form = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT { x, y },
            rcArea: RECT::default(),
        };
        let _ = ImmSetCompositionWindow(imc, &form);
        let _ = ImmReleaseContext(edit, imc);
    }
}

fn down(vk: u16) -> bool {
    unsafe { GetKeyState(i32::from(vk)) < 0 }
}

fn notify(edit: HWND, ev: EditEvent) {
    if let Ok(parent) = unsafe { GetParent(edit) } {
        host::edit_event(parent, edit, ev);
    }
}

/// A collapsed caret in the text, or `None` while something is selected.
fn caret(edit: HWND) -> Option<(String, usize)> {
    // The text first: it settles any lone break the selection is counted in.
    let t = text(edit);
    let (a, b) = raw_selection(edit);
    if a != b {
        return None;
    }
    let c = checklist::model_index(&t, a);
    Some((t, c))
}

/// After a key that moved the caret: out of a marker it goes.
fn fix_caret(edit: HWND, before: usize) {
    let Some((t, c)) = caret(edit) else { return };
    let snapped = checklist::snap_caret(&t, c, before);
    if snapped != c {
        set_selection(edit, &t, snapped, snapped);
    }
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_GETDLGCODE => {
            let code = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            return LRESULT(code.0 | DLGC_WANTALLKEYS as isize);
        }
        WM_KEYDOWN => {
            let key = (wparam.0 & 0xFFFF) as u16;
            if key == VK_ESCAPE.0 {
                notify(hwnd, EditEvent::Escape);
                return LRESULT(0);
            }
            if key == VK_UP.0 || key == VK_DOWN.0 {
                notify(
                    hwnd,
                    EditEvent::Vertical {
                        up: key == VK_UP.0,
                        extend: down(VK_SHIFT.0),
                    },
                );
                return LRESULT(0);
            }
            if key == VK_A.0 && down(VK_CONTROL.0) {
                unsafe {
                    SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                }
                notify(hwnd, EditEvent::Changed);
                return LRESULT(0);
            }
            let before = caret(hwnd).map(|(_, c)| c);
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            if let Some(before) = before {
                fix_caret(hwnd, before);
            }
            notify(hwnd, EditEvent::Changed);
            return result;
        }
        WM_CHAR => {
            let ch = (wparam.0 & 0xFFFF) as u16;
            // Esc and Ctrl+A are handled on the key down; their characters
            // must not beep or type.
            if ch == 0x1B || ch == 0x01 {
                return LRESULT(0);
            }
            // Ctrl+V goes through our own paste.
            if ch == 0x16 {
                unsafe {
                    SendMessageW(hwnd, WM_PASTE, None, None);
                }
                return LRESULT(0);
            }
            let collapsed = caret(hwnd);
            let edit = collapsed.as_ref().and_then(|(t, c)| match ch {
                0x0D => checklist::enter(t, *c),
                0x08 => checklist::backspace(t, *c),
                0x20 => checklist::autobox(t, *c),
                _ => None,
            });
            if let (Some(e), Some((t, _))) = (edit, collapsed) {
                apply(hwnd, &t, &e);
                notify(hwnd, EditEvent::Changed);
                return LRESULT(0);
            }
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            notify(hwnd, EditEvent::Changed);
            return result;
        }
        WM_PASTE => {
            let result = if paste(hwnd) {
                LRESULT(0)
            } else {
                unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
            };
            notify(hwnd, EditEvent::Changed);
            return result;
        }
        WM_CUT | WM_CLEAR | WM_UNDO => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            notify(hwnd, EditEvent::Changed);
            return result;
        }
        WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            notify(hwnd, EditEvent::Changed);
            return result;
        }
        WM_SETFOCUS => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            notify(hwnd, EditEvent::Changed);
            return result;
        }
        WM_KILLFOCUS => {
            let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            notify(hwnd, EditEvent::Blur);
            return result;
        }
        WM_NCDESTROY => unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(edit_proc), SUBCLASS_ID);
        },
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edit_does_not_wrap_on_its_own_metrics() {
        // Home and End must work on the lines as typed, not on the EDIT's own
        // soft wrap, which differs from the drawn layout.
        let s = style().0;
        assert_ne!(s & ES_AUTOHSCROLL as u32, 0, "no word wrap in the EDIT");
        assert_ne!(s & ES_MULTILINE as u32, 0);
    }
}
