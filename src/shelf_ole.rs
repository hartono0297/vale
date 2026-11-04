//! The file shelf's OLE side: the island window as a drop target for files,
//! and a card dragged back out. Nothing here keeps state of its own: a drag
//! over the island and a drop become `Event`s on the app's channel (the
//! reducer opens the shelf, `App` keeps the paths), and a drag out is one
//! blocking `DoDragDrop` that `App` starts outside its borrow.
//!
//! Files are never moved or deleted: only copy and link are offered, and a
//! drop answers with link (the shelf refers to the file, it does not copy
//! it) or copy when link is not on offer.

use std::cell::Cell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, POINTL, S_OK,
};
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IBindCtx, IDataObject, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::GlobalSize;
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_NONE, DoDragDrop,
    IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop,
    ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{
    BHID_DataObject, DROPFILES, DragQueryFileW, HDROP, IShellItem, SHCreateItemFromParsingName,
};
use windows::core::{BOOL, HRESULT, HSTRING, Ref, implement};

use crate::model::Event;
use crate::shelf::MAX_ITEMS;

/// The effect a drop answers with, from the ones the source offers: link,
/// else copy, else none. Never move.
pub fn choose_effect(offered: u32) -> u32 {
    if offered & DROPEFFECT_LINK.0 != 0 {
        DROPEFFECT_LINK.0
    } else if offered & DROPEFFECT_COPY.0 != 0 {
        DROPEFFECT_COPY.0
    } else {
        DROPEFFECT_NONE.0
    }
}

thread_local! {
    /// `drag_out` is running on this (the UI) thread: the drag that enters
    /// the island now is its own.
    static DRAGGING_OUT: Cell<bool> = const { Cell::new(false) };
}

/// Sets `DRAGGING_OUT` while it lives (also on an early return or a panic).
struct OwnDrag;

impl OwnDrag {
    fn start() -> Self {
        DRAGGING_OUT.with(|d| d.set(true));
        OwnDrag
    }
}

impl Drop for OwnDrag {
    fn drop(&mut self) {
        DRAGGING_OUT.with(|d| d.set(false));
    }
}

/// A drag carrying files (`files`) is taken by the shelf, unless it is the
/// island's own drag out.
fn takes_drag(files: bool) -> bool {
    files && !DRAGGING_OUT.with(Cell::get)
}

/// Whether `data` can give a file list.
fn has_files(data: &IDataObject) -> bool {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let answer = unsafe { data.QueryGetData(&format) };
    answer == S_OK
}

/// The paths in `data`'s file list (empty when it has none), at most
/// `shelf::MAX_ITEMS` of them: the shelf keeps no more. A medium that is not
/// an HGLOBAL holding at least a `DROPFILES` header is not read.
fn read_paths(data: &IDataObject) -> Vec<PathBuf> {
    read_paths_max(data, MAX_ITEMS)
}

/// `read_paths` with another limit (a note takes fewer paths than the shelf).
pub(crate) fn read_paths_max(data: &IDataObject, max: usize) -> Vec<PathBuf> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let Ok(mut medium) = (unsafe { data.GetData(&format) }) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    unsafe {
        let hglobal = (medium.tymed == TYMED_HGLOBAL.0 as u32).then(|| medium.u.hGlobal);
        let readable =
            hglobal.is_some_and(|h| !h.is_invalid() && GlobalSize(h) >= size_of::<DROPFILES>());
        if !readable {
            ReleaseStgMedium(&mut medium);
            return paths;
        }
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None).min(max as u32);
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            if len == 0 {
                continue;
            }
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(hdrop, i, Some(&mut buf)) as usize;
            paths.push(PathBuf::from(OsString::from_wide(&buf[..got.min(len)])));
        }
        ReleaseStgMedium(&mut medium);
    }
    paths
}

#[implement(IDropTarget)]
struct DropTarget {
    tx: Sender<Event>,
    wake: Box<dyn Fn()>,
    /// The drag in progress carries files.
    accepting: Cell<bool>,
}

impl DropTarget {
    fn send(&self, event: Event) {
        if self.tx.send(event).is_ok() {
            (self.wake)();
        }
    }

    fn effect_for(&self, effect: *mut DROPEFFECT) {
        unsafe {
            *effect = if self.accepting.get() {
                DROPEFFECT(choose_effect((*effect).0))
            } else {
                DROPEFFECT_NONE
            };
        }
    }
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // The island's own drag out (a card pulled over the island) is not
        // a drop onto the shelf.
        let files = takes_drag(data.as_ref().is_some_and(has_files));
        self.accepting.set(files);
        self.effect_for(effect);
        if files {
            self.send(Event::ShelfDragOver(true));
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.effect_for(effect);
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if self.accepting.replace(false) {
            self.send(Event::ShelfDragOver(false));
        }
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let paths = match data.as_ref() {
            Some(data) if takes_drag(true) => read_paths(data),
            _ => Vec::new(),
        };
        let was_accepting = self.accepting.get();
        self.accepting.set(!paths.is_empty());
        self.effect_for(effect);
        self.accepting.set(false);
        if !paths.is_empty() {
            self.send(Event::ShelfDrop(paths));
        } else if was_accepting {
            // The list came up empty after all: no drop follows, so the drop
            // zone and the panel's hold end as on a leave.
            self.send(Event::ShelfDragOver(false));
        }
        Ok(())
    }
}

/// Makes the island window take dropped files. `tx` gets the events and
/// `wake` runs after each. A window that cannot take drops (OLE would not
/// start) just does not: the shelf then fills no other way, which is said in
/// the log, not to the user.
pub fn register(hwnd: HWND, tx: Sender<Event>, wake: impl Fn() + 'static) {
    unsafe {
        // COM is already up on this thread (apartment); this adds OLE's
        // drag and drop, and answers S_FALSE when it was.
        let _ = OleInitialize(None);
        let target: IDropTarget = DropTarget {
            tx,
            wake: Box::new(wake),
            accepting: Cell::new(false),
        }
        .into();
        if let Err(error) = RegisterDragDrop(hwnd, &target) {
            eprintln!("vale: no file drops: {error}");
        }
    }
}

/// Before the window goes.
pub fn revoke(hwnd: HWND) {
    unsafe {
        let _ = RevokeDragDrop(hwnd);
    }
}

#[implement(IDropSource)]
struct DragSource;

impl IDropSource_Impl for DragSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if !keys.contains(MK_LBUTTON) {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Drags `path` out of the island: the file list the shell gives for it,
/// offered for copy or link, never move. Blocks until the button is let go
/// (it pumps this thread's messages meanwhile), so the caller must not hold
/// the app state. False when the drag could not start.
pub fn drag_out(path: &Path) -> bool {
    unsafe {
        let wide = HSTRING::from(path.as_os_str());
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&wide, None::<&IBindCtx>)
        else {
            return false;
        };
        let Ok(data) = item.BindToHandler::<_, IDataObject>(None::<&IBindCtx>, &BHID_DataObject)
        else {
            return false;
        };
        let source: IDropSource = DragSource.into();
        let mut effect = DROPEFFECT_NONE;
        let allowed = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_LINK.0);
        let _own = OwnDrag::start();
        let _ = DoDragDrop(&data, &source, allowed, &mut effect);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_answers_link_else_copy_and_never_move() {
        let (copy, mv, link) = (DROPEFFECT_COPY.0, 2, DROPEFFECT_LINK.0);
        assert_eq!(choose_effect(copy | mv | link), link);
        assert_eq!(choose_effect(copy | mv), copy);
        assert_eq!(choose_effect(copy), copy);
        assert_eq!(choose_effect(link), link);
        // Only move on offer: nothing.
        assert_eq!(choose_effect(mv), DROPEFFECT_NONE.0);
        assert_eq!(choose_effect(0), DROPEFFECT_NONE.0);
        // The scroll flag in the high bit is not an effect.
        assert_eq!(choose_effect(0x8000_0000 | mv), DROPEFFECT_NONE.0);
    }

    #[test]
    fn the_islands_own_drag_out_is_not_a_drop_onto_the_shelf() {
        assert!(takes_drag(true));
        assert!(!takes_drag(false));
        {
            let _own = OwnDrag::start();
            assert!(!takes_drag(true), "a card pulled over the island");
        }
        // Over once `DoDragDrop` returns.
        assert!(takes_drag(true));
    }
}
