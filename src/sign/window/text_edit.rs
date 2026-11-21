//! Native one-line EDITs of the signing window: the Teks item typed on the
//! page, the password field, and the maker's "Ketik" name field (IME and
//! paste work, which a drawn box could not give). Copied from the pattern of
//! `shot_edit/text.rs`.
//!
//! The subclass never destroys anything itself: it posts `WM_SIGN_EDIT` to
//! the parent with the EDIT's own HWND in `lparam` (`EDIT_COMMIT` or
//! `EDIT_CANCEL` in `wparam`), so the EDIT is never destroyed inside its own
//! proc, and a post from an EDIT that is already gone can be told from the
//! open one (`msg_is_current`).

use std::ffi::c_void;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET,
    DeleteObject, FW_BOLD, FW_NORMAL, HBRUSH, HDC, HFONT, HGDIOBJ, InvalidateRect,
    OUT_DEFAULT_PRECIS, SetBkColor, SetTextColor,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{EM_GETSEL, EM_SETMARGINS, EM_SETSEL};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_ESCAPE, VK_MENU, VK_RETURN, VK_TAB};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, EN_CHANGE, ES_AUTOHSCROLL, ES_PASSWORD, GetParent,
    GetWindowTextLengthW, GetWindowTextW, HMENU, MoveWindow, PostMessageW, SendMessageW,
    SetWindowTextW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CHAR, WM_KEYDOWN, WM_KILLFOCUS,
    WM_NCDESTROY, WM_SETFONT, WS_CHILD, WS_VISIBLE,
};
use windows::core::{HSTRING, PCWSTR, w};

/// An EDIT finished: `wparam` is `EDIT_COMMIT` or `EDIT_CANCEL`, `lparam` the
/// EDIT that sent it.
pub const WM_SIGN_EDIT: u32 = WM_APP + 97;
pub const EDIT_COMMIT: usize = 1;
pub const EDIT_CANCEL: usize = 2;
/// Tab pressed in the EDIT: the parent moves the keyboard on.
pub const EDIT_TAB: usize = 3;
/// Ctrl+B, Ctrl+I, Ctrl+U in a field made with `format_keys`.
pub const EDIT_BOLD: usize = 4;
pub const EDIT_ITALIC: usize = 5;
pub const EDIT_UNDERLINE: usize = 6;

const EDIT_ID: isize = 7101;
const SUBCLASS_ID: usize = 1;
/// Subclass data: losing the focus commits (the Teks EDIT), else it does
/// nothing (password and name fields keep what was typed).
const COMMIT_ON_BLUR: usize = 1;
/// Subclass data: Ctrl+B / I / U post a format code.
const FORMAT_KEYS: usize = 2;

/// A native EDIT and the GDI objects it draws with.
pub struct Field {
    pub edit: HWND,
    font: HFONT,
    brush: HBRUSH,
    pub color: u32,
    pub back: u32,
    /// Font height in px the field was made (or last restyled) with.
    pub px: f32,
}

/// How to make a field.
pub struct Spec<'a> {
    /// x, y, w, h in the parent's client px.
    pub rect: (i32, i32, i32, i32),
    pub initial: &'a str,
    /// Font height in px.
    pub px: f32,
    pub face: PCWSTR,
    pub color: u32,
    pub back: u32,
    pub password: bool,
    pub commit_on_blur: bool,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// Ctrl+B / I / U post `EDIT_BOLD` / `EDIT_ITALIC` / `EDIT_UNDERLINE`.
    pub format_keys: bool,
}

/// `0xRRGGBB` as a GDI `COLORREF`.
pub fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | (rgb >> 16))
}

/// Makes the EDIT as a child of `parent`, caret at the end. The caller
/// stores it and gives it the focus. `None` when it could not be made.
pub fn create(parent: HWND, s: &Spec) -> Option<Field> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let font = make_font(s.face, s.px, s.bold, s.italic, s.underline);
        let mut style = WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
        if s.password {
            style |= WINDOW_STYLE(ES_PASSWORD as u32);
        }
        let (x, y, cw, ch) = s.rect;
        let Ok(edit) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            &HSTRING::from(s.initial),
            style,
            x,
            y,
            cw.max(8),
            ch.max(8),
            Some(parent),
            Some(HMENU(EDIT_ID as *mut c_void)),
            Some(instance.into()),
            None,
        ) else {
            let _ = DeleteObject(HGDIOBJ(font.0));
            return None;
        };
        SendMessageW(edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        let margin = (s.px * 0.25).round().max(2.0) as usize;
        SendMessageW(edit, EM_SETMARGINS, Some(WPARAM(3)), Some(LPARAM((margin | margin << 16) as isize)));
        let mut data = if s.commit_on_blur { COMMIT_ON_BLUR } else { 0 };
        if s.format_keys {
            data |= FORMAT_KEYS;
        }
        let _ = SetWindowSubclass(edit, Some(edit_proc), SUBCLASS_ID, data);
        let brush = CreateSolidBrush(colorref(s.back));
        let len = GetWindowTextLengthW(edit);
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(len as usize)), Some(LPARAM(len as isize)));
        Some(Field { edit, font, brush, color: s.color, back: s.back, px: s.px })
    }
}

fn make_font(face: PCWSTR, px: f32, bold: bool, italic: bool, underline: bool) -> HFONT {
    unsafe {
        CreateFontW(
            -(px.round().max(6.0) as i32),
            0,
            0,
            0,
            if bold { FW_BOLD.0 } else { FW_NORMAL.0 } as i32,
            italic as u32,
            underline as u32,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            0,
            face,
        )
    }
}

/// Gives the EDIT a new font and text colour (the pending style changed).
/// The old font goes only after the new one is set.
pub fn restyle(f: &mut Field, face: PCWSTR, px: f32, bold: bool, italic: bool, underline: bool, color: u32) {
    unsafe {
        let font = make_font(face, px, bold, italic, underline);
        SendMessageW(f.edit, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        let margin = (px * 0.25).round().max(2.0) as usize;
        SendMessageW(f.edit, EM_SETMARGINS, Some(WPARAM(3)), Some(LPARAM((margin | margin << 16) as isize)));
        let _ = DeleteObject(HGDIOBJ(f.font.0));
        f.font = font;
        f.color = color;
        f.px = px;
        let _ = InvalidateRect(Some(f.edit), None, true);
    }
}

/// Ctrl+B / Ctrl+I / Ctrl+U as a notify code.
fn format_key(vk: u16, ctrl: bool, alt: bool) -> Option<usize> {
    // AltGr reports Ctrl and Alt together: it types characters, not formats.
    if !ctrl || alt {
        return None;
    }
    match vk {
        0x42 => Some(EDIT_BOLD),
        0x49 => Some(EDIT_ITALIC),
        0x55 => Some(EDIT_UNDERLINE),
        _ => None,
    }
}

/// Characters the EDIT must not insert or beep at: Tab, Enter, Esc, and the
/// Ctrl+B / Ctrl+U control codes (0x02, 0x15) when format keys are on.
fn swallow_char(ch: usize, format_keys: bool) -> bool {
    matches!(ch, 0x09 | 0x0D | 0x1B) || (format_keys && matches!(ch, 0x02 | 0x15))
}

/// The EDIT's text.
pub fn text(edit: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; n + 1];
        let got = GetWindowTextW(edit, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..got.min(n)])
    }
}

/// The caret or selection, as (start, end) characters.
pub fn selection(edit: HWND) -> (usize, usize) {
    let v = unsafe { SendMessageW(edit, EM_GETSEL, None, None) }.0 as usize;
    (v & 0xFFFF, (v >> 16) & 0xFFFF)
}

pub fn select(edit: HWND, (start, end): (usize, usize)) {
    unsafe {
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(start)), Some(LPARAM(end as isize)));
    }
}

/// The height of the EDIT's font in px (tests).
#[cfg(test)]
pub fn font_px(edit: HWND) -> i32 {
    use windows::Win32::Graphics::Gdi::{GetObjectW, LOGFONTW};
    use windows::Win32::UI::WindowsAndMessaging::WM_GETFONT;
    let font = unsafe { SendMessageW(edit, WM_GETFONT, None, None) }.0;
    let mut lf = LOGFONTW::default();
    let got = unsafe {
        GetObjectW(HGDIOBJ(font as *mut c_void), size_of::<LOGFONTW>() as i32, Some((&raw mut lf).cast()))
    };
    if got == 0 { 0 } else { lf.lfHeight.abs() }
}

pub fn clear(edit: HWND) {
    unsafe {
        let _ = SetWindowTextW(edit, w!(""));
    }
}

pub fn move_to(edit: HWND, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let _ = MoveWindow(edit, x, y, w.max(8), h.max(8), true);
    }
}

/// Removes the EDIT and frees its font and brush. Unhooked first: the focus
/// it loses on the way out must not post a commit.
pub fn destroy(f: Field) {
    unsafe {
        let _ = RemoveWindowSubclass(f.edit, Some(edit_proc), SUBCLASS_ID);
        let _ = DestroyWindow(f.edit);
    }
    free_gdi(&f);
}

/// Frees the font and brush only: the parent is going down and takes the
/// EDIT with it.
pub fn free_gdi(f: &Field) {
    unsafe {
        let _ = DeleteObject(HGDIOBJ(f.font.0));
        let _ = DeleteObject(HGDIOBJ(f.brush.0));
    }
}

/// The parent's `WM_CTLCOLOREDIT`: text colour on the field's back, and the
/// brush to return.
pub fn ctl_color(f: &Field, wparam: WPARAM) -> LRESULT {
    let hdc = HDC(wparam.0 as *mut c_void);
    unsafe {
        SetTextColor(hdc, colorref(f.color));
        SetBkColor(hdc, colorref(f.back));
    }
    LRESULT(f.brush.0 as isize)
}

/// A `WM_COMMAND`'s notification code says the EDIT's text changed.
pub fn is_change(wparam: WPARAM) -> bool {
    ((wparam.0 >> 16) & 0xFFFF) as u32 == EN_CHANGE
}

/// Whether a `WM_SIGN_EDIT` comes from the EDIT open now.
pub fn msg_is_current(open: Option<isize>, sender: isize) -> bool {
    open == Some(sender)
}

/// Enter commits, Esc cancels, Tab hands the keyboard back; losing the focus
/// commits when the field asks for it.
unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let parent = unsafe { GetParent(hwnd) }.ok();
    let post = |what: usize| {
        if let Some(p) = parent {
            unsafe {
                let _ = PostMessageW(Some(p), WM_SIGN_EDIT, WPARAM(what), LPARAM(hwnd.0 as isize));
            }
        }
    };
    match msg {
        WM_KEYDOWN => {
            let key = (wparam.0 & 0xFFFF) as u16;
            if key == VK_ESCAPE.0 {
                post(EDIT_CANCEL);
                return LRESULT(0);
            }
            if key == VK_RETURN.0 {
                post(EDIT_COMMIT);
                return LRESULT(0);
            }
            if key == VK_TAB.0 {
                post(EDIT_TAB);
                return LRESULT(0);
            }
            if data & FORMAT_KEYS != 0 {
                let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
                let alt = unsafe { GetKeyState(VK_MENU.0 as i32) } < 0;
                if let Some(code) = format_key(key, ctrl, alt) {
                    post(code);
                    return LRESULT(0);
                }
            }
        }
        WM_CHAR => {
            // Enter, Esc, Tab and the format keys: no character, no beep.
            if swallow_char(wparam.0, data & FORMAT_KEYS != 0) {
                return LRESULT(0);
            }
        }
        WM_KILLFOCUS if data & COMMIT_ON_BLUR != 0 => post(EDIT_COMMIT),
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
    fn a_message_from_a_gone_edit_is_ignored() {
        assert!(msg_is_current(Some(0x1234), 0x1234));
        assert!(!msg_is_current(Some(0x5678), 0x1234));
        assert!(!msg_is_current(None, 0x1234));
    }

    #[test]
    fn format_keys_post_and_swallow() {
        assert_eq!(format_key(0x42, true, false), Some(EDIT_BOLD));
        assert_eq!(format_key(0x49, true, false), Some(EDIT_ITALIC));
        assert_eq!(format_key(0x55, true, false), Some(EDIT_UNDERLINE));
        assert_eq!(format_key(0x42, false, false), None);
        assert_eq!(format_key(0x41, true, false), None);
        assert_eq!(format_key(0x55, true, true), None);
        assert_eq!(format_key(0x49, true, true), None);
        for ch in [0x02, 0x09, 0x15] {
            assert!(swallow_char(ch, true));
        }
        assert!(!swallow_char(0x02, false));
        assert!(!swallow_char(0x15, false));
        for ch in [0x09, 0x0D, 0x1B] {
            assert!(swallow_char(ch, false));
            assert!(swallow_char(ch, true));
        }
        assert!(!swallow_char(0x41, true));
    }

    #[test]
    fn colorref_swaps_red_and_blue_and_change_is_en_change() {
        assert_eq!(colorref(0x112233).0, 0x332211);
        assert!(is_change(WPARAM((EN_CHANGE as usize) << 16 | 7101)));
        assert!(!is_change(WPARAM(7101)));
        assert_eq!(WM_SIGN_EDIT, WM_APP + 97);
    }
}
