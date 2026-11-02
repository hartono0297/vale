//! The native EDIT that text annotations are typed in, shared by the editor
//! window and the inline capture overlay (IME and paste work, which a drawn
//! text box could not give).
//!
//! The EDIT is a child of its parent window. Its subclass never destroys
//! anything itself: it posts `WM_APP_TEXT` to the parent with the EDIT's own
//! HWND in `lparam` (1 = commit, 2 = cancel), so the EDIT is never destroyed
//! inside its own proc, and a post from an EDIT that is already gone can be
//! told from the open one (`text_msg_is_current`).

use std::ffi::c_void;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET,
    DeleteObject, FW_NORMAL, HBRUSH, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, ScreenToClient,
    SetBkColor, SetTextColor,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{EM_SETMARGINS, EM_SETSEL};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_ESCAPE, VK_RETURN, VK_SHIFT};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, EN_CHANGE, ES_AUTOVSCROLL, ES_MULTILINE, GetParent,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, HMENU, MoveWindow, PostMessageW,
    SendMessageW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CHAR, WM_KEYDOWN, WM_KILLFOCUS,
    WM_NCDESTROY, WM_SETFONT, WS_CHILD, WS_VISIBLE,
};
use windows::core::{HSTRING, w};

use super::{Pt, Tool};
use crate::ui::fonts;

/// A native text editor finished: `wparam` 1 commits, 2 cancels, `lparam`
/// is the EDIT that sent it. A message from an EDIT that is no longer the
/// open one is ignored.
pub const WM_APP_TEXT: u32 = WM_APP + 50;
/// What `WM_APP_TEXT` carries in `wparam`.
pub const TEXT_COMMIT: usize = 1;
pub const TEXT_CANCEL: usize = 2;

const EDIT_ID: isize = 7001;
const SUBCLASS_ID: usize = 1;

/// A native EDIT over the canvas.
pub struct TextEdit {
    pub edit: HWND,
    font: HFONT,
    brush: HBRUSH,
    /// Replacing this annotation's text; `None` for a new text.
    pub index: Option<usize>,
    /// Where the text goes, in annotation px.
    pub pos: Pt,
    /// Text size in annotation px.
    pub size: f32,
    pub color: u32,
    /// The EDIT's background (a contrast to the text colour).
    pub back: u32,
}

/// `0xRRGGBB` as a GDI `COLORREF`.
pub fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | (rgb >> 16))
}

/// White or near-black, whichever reads against `rgb` (the EDIT's back).
pub fn contrast_back(rgb: u32) -> u32 {
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    let luma = (299 * r + 587 * g + 114 * b) / 1000;
    if luma > 150 { 0x202020 } else { 0xF7F7F7 }
}

fn key_down(vk: u16) -> bool {
    unsafe { GetKeyState(i32::from(vk)) < 0 }
}

/// Makes the EDIT, its font (`px` high) and background brush as a child of
/// `parent` at `client_pos` (client px), holding `initial`, with the caret
/// at the end. The caller stores it, calls `fit` and gives it the focus.
/// `None` when the window could not be made.
pub fn create(
    parent: HWND,
    client_pos: (f32, f32),
    initial: &str,
    px: f32,
    color: u32,
    ui: f32,
) -> Option<TextEdit> {
    let back = contrast_back(color);
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let font = CreateFontW(
            -(px as i32),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            0,
            fonts::gdi_text_face(),
        );
        let style = WS_CHILD | WS_VISIBLE | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL) as u32);
        let Ok(edit) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            &HSTRING::from(initial.replace('\n', "\r\n")),
            style,
            client_pos.0.round() as i32,
            client_pos.1.round() as i32,
            (px * 8.0).max(140.0 * ui) as i32,
            (px * 1.4) as i32,
            Some(parent),
            Some(HMENU(EDIT_ID as *mut c_void)),
            Some(instance.into()),
            None,
        ) else {
            let _ = DeleteObject(HGDIOBJ(font.0));
            return None;
        };
        SendMessageW(
            edit,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        SendMessageW(edit, EM_SETMARGINS, Some(WPARAM(3)), Some(LPARAM(0)));
        let _ = SetWindowSubclass(edit, Some(edit_proc), SUBCLASS_ID, 0);
        let brush = CreateSolidBrush(colorref(back));
        // The caret at the end.
        let len = GetWindowTextLengthW(edit);
        SendMessageW(
            edit,
            EM_SETSEL,
            Some(WPARAM(len as usize)),
            Some(LPARAM(len as isize)),
        );
        Some(TextEdit {
            edit,
            font,
            brush,
            index: None,
            pos: Pt::default(),
            size: px,
            color,
            back,
        })
    }
}

/// The EDIT's text, lines split at '\n'.
pub fn edit_text(edit: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; n + 1];
        let got = GetWindowTextW(edit, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..got]).replace("\r\n", "\n")
    }
}

/// Removes the EDIT and frees its font and brush. Unhooked first: the focus
/// it loses on the way out must not post a commit that would land on the
/// next EDIT.
pub fn destroy(t: TextEdit) {
    unsafe {
        let _ = RemoveWindowSubclass(t.edit, Some(edit_proc), SUBCLASS_ID);
        let _ = DestroyWindow(t.edit);
    }
    free_gdi(&t);
}

/// Frees the font and brush only: the window is going down with its parent
/// (a `WM_DESTROY`), which destroys the child EDIT itself.
pub fn free_gdi(t: &TextEdit) {
    unsafe {
        let _ = DeleteObject(HGDIOBJ(t.font.0));
        let _ = DeleteObject(HGDIOBJ(t.brush.0));
    }
}

/// Grows the EDIT with its text. `measure` gives the width and height of
/// a string at the EDIT's font size (`px`).
pub fn fit(parent: HWND, edit: HWND, px: f32, ui: f32, measure: impl FnOnce(&str) -> (f32, f32)) {
    let text = edit_text(edit);
    let (tw, th) = measure(&format!("{text}M"));
    let w = (tw + 12.0 * ui).max(140.0 * ui);
    let h = (th + 6.0 * ui).max(px * 1.4);
    unsafe {
        let mut at = RECT::default();
        let _ = GetWindowRect(edit, &mut at);
        let mut pt = POINT {
            x: at.left,
            y: at.top,
        };
        let _ = ScreenToClient(parent, &mut pt);
        let _ = MoveWindow(edit, pt.x, pt.y, w as i32, h as i32, true);
    }
}

/// The parent's `WM_CTLCOLOREDIT`: the text colour on its contrast back;
/// the brush to return.
pub fn ctl_color(t: &TextEdit, wparam: WPARAM) -> LRESULT {
    let hdc = HDC(wparam.0 as *mut c_void);
    unsafe {
        SetTextColor(hdc, colorref(t.color));
        SetBkColor(hdc, colorref(t.back));
    }
    LRESULT(t.brush.0 as isize)
}

/// A `WM_COMMAND`'s notification code says the EDIT's text changed.
pub fn is_change(wparam: WPARAM) -> bool {
    ((wparam.0 >> 16) & 0xFFFF) as u32 == EN_CHANGE
}

/// What a canvas click does with the Text tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextClick {
    /// Not the Text tool: the click is for something else.
    NotText,
    /// An EDIT was open: the click only finishes it, so the focus goes back
    /// to the window and the keys (Ctrl+C, Ctrl+S, Delete, Esc, V/T/R) work.
    CommitOnly,
    /// No EDIT was open: the click places a new one.
    Place,
}

/// `edit_was_open`: the press committed an open EDIT.
pub fn text_click_action(edit_was_open: bool, tool: Tool) -> TextClick {
    match (tool, edit_was_open) {
        (Tool::Text, true) => TextClick::CommitOnly,
        (Tool::Text, false) => TextClick::Place,
        _ => TextClick::NotText,
    }
}

/// Whether a `WM_APP_TEXT` belongs to the EDIT open now: the post carries the
/// sender's HWND, and one from an EDIT already gone must never close a newer
/// one.
pub fn text_msg_is_current(open_edit: Option<isize>, sender: isize) -> bool {
    open_edit == Some(sender)
}

/// Enter commits, Shift+Enter adds a line, Esc cancels, losing focus commits.
unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let parent = unsafe { GetParent(hwnd) }.ok();
    let post = |what: usize| {
        if let Some(p) = parent {
            unsafe {
                let _ = PostMessageW(Some(p), WM_APP_TEXT, WPARAM(what), LPARAM(hwnd.0 as isize));
            }
        }
    };
    match msg {
        WM_KEYDOWN => {
            let key = (wparam.0 & 0xFFFF) as u16;
            if key == VK_ESCAPE.0 {
                post(TEXT_CANCEL);
                return LRESULT(0);
            }
            if key == VK_RETURN.0 && !key_down(VK_SHIFT.0) {
                post(TEXT_COMMIT);
                return LRESULT(0);
            }
        }
        WM_CHAR => {
            // Enter and Esc: no new line, no beep.
            if wparam.0 == 0x1B || (wparam.0 == 0x0D && !key_down(VK_SHIFT.0)) {
                return LRESULT(0);
            }
        }
        WM_KILLFOCUS => post(TEXT_COMMIT),
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
    fn a_text_click_after_typing_only_commits() {
        // An EDIT was open: the click finishes it and places nothing.
        assert_eq!(text_click_action(true, Tool::Text), TextClick::CommitOnly);
        // No EDIT: a click places a new one.
        assert_eq!(text_click_action(false, Tool::Text), TextClick::Place);
        // Other tools are not the text rule's business.
        for tool in Tool::ALL {
            if tool != Tool::Text {
                assert_eq!(text_click_action(true, tool), TextClick::NotText);
                assert_eq!(text_click_action(false, tool), TextClick::NotText);
            }
        }
    }

    #[test]
    fn a_text_message_from_a_gone_edit_is_ignored() {
        assert!(text_msg_is_current(Some(0x1234), 0x1234));
        // A newer EDIT is open: the old one's post does not touch it.
        assert!(!text_msg_is_current(Some(0x5678), 0x1234));
        // Nothing open: nothing to close.
        assert!(!text_msg_is_current(None, 0x1234));
    }

    #[test]
    fn the_text_background_contrasts_with_the_colour() {
        assert_eq!(contrast_back(0xFFFFFF), 0x202020);
        assert_eq!(contrast_back(0x1A1A1A), 0xF7F7F7);
        assert_eq!(contrast_back(super::super::COLORS[0]), 0xF7F7F7);
        assert_eq!(contrast_back(super::super::COLORS[1]), 0x202020);
    }

    #[test]
    fn colorref_swaps_red_and_blue() {
        assert_eq!(colorref(0x112233).0, 0x332211);
    }

    #[test]
    fn the_text_message_is_wm_app_plus_50_and_the_notification_is_en_change() {
        assert_eq!(WM_APP_TEXT, WM_APP + 50);
        assert!(is_change(WPARAM((EN_CHANGE as usize) << 16 | 7001)));
        assert!(!is_change(WPARAM(7001)));
    }
}
