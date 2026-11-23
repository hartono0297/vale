//! The signature maker: a child window over the signing window, 400 DIPs
//! wide. Three ways to make a mark (draw with mouse, touchpad, pen or touch;
//! type a name in a handwriting face; a photo of a signature on paper), blue
//! or black ink, "Simpan di PC ini", and the saved marks to reuse or forget.
//! It hands the result to its parent with `WM_MAKER` (`take_result`); the
//! parent destroys it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;

use windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_ELLIPSE, ID2D1Bitmap, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_WRAP};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateRoundRectRgn, EndPaint, InvalidateRect, PAINTSTRUCT, ScreenToClient,
    SetWindowRgn,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_BACK, VK_DELETE, VK_ESCAPE, VK_RETURN,
    VK_SHIFT, VK_SPACE, VK_TAB,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetAncestor, GA_ROOT, IDC_ARROW, IDC_CROSS, IDC_HAND,
    LoadCursorW, PostMessageW, RegisterClassExW, SWP_NOACTIVATE, SWP_NOZORDER, SetCursor,
    SetWindowPos, WINDOW_EX_STYLE, WM_CAPTURECHANGED, WM_COMMAND, WM_CTLCOLOREDIT, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_POINTERDOWN, WM_POINTERUP, WM_POINTERUPDATE, WM_SETCURSOR, WNDCLASSEXW, WS_CHILD,
    WS_CLIPCHILDREN, WS_VISIBLE,
};
use windows::core::{Error, HSTRING, PCWSTR, w};
use windows_numerics::Vector2;

use super::paint::{PAPER, premultiplied};
use super::text_edit::{self, EDIT_CANCEL, EDIT_COMMIT, EDIT_TAB, Field, WM_SIGN_EDIT};
use super::window::{WM_MAKER, take_focus};
use crate::i18n::{Key, tr};
use crate::shot_edit::gfx::{Align, Gfx, Target, bitmap, color, rect_f};
use crate::shot_edit::icons::fill_round;
use crate::sign::Mark;
use crate::sign::hand;
use crate::sign::ink::{self, Ink};
use crate::theme::WindowPalette;
use crate::ui::geometry::Rect;

const CLASS: PCWSTR = w!("ValeSignMaker");

const W: f32 = 400.0;
const PAD: f32 = 16.0;
const TITLE_H: f32 = 22.0;
const TABS_H: f32 = 34.0;
const PAD_H: f32 = 150.0;
const INK_H: f32 = 28.0;
const CHECK_H: f32 = 52.0;
const TILE_H: f32 = 48.0;
const BTN_H: f32 = 34.0;
const FIELD_H: f32 = 36.0;
/// The row of handwriting-font tiles under the name field (Ketik).
const FONT_ROW_H: f32 = 44.0;
const SWATCH: f32 = 18.0;
/// The logical pad `ink::from_strokes` takes.
const INK_PAD: (f32, f32) = (300.0, 90.0);
const INK_STROKE: f32 = 3.2;
/// Most saved marks of a kind (the store keeps four).
const TILES: usize = 4;
const ERROR_ON_PAPER: u32 = 0xC42B1C;
const HINT_ON_PAPER: u32 = 0x6E5F52;
const BASELINE: u32 = 0xC9C0B4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    Draw,
    Type,
    Photo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MHit {
    Tab(Tab),
    Redo,
    PickPhoto,
    Ink(bool),
    Keep,
    Tile(usize),
    Forget(usize),
    Font(usize),
    New,
    Cancel,
    Use,
    Pad,
}

/// Where everything sits in the maker (client px).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct MLayout {
    pub size: (f32, f32),
    pub title: Rect,
    pub tabs: Rect,
    pub pad: Rect,
    /// The name field (Ketik) above the preview inside the pad's place.
    pub field: Rect,
    pub buttons: Vec<(MHit, Rect)>,
    pub check_text: Rect,
}

impl MLayout {
    pub fn rect_of(&self, h: MHit) -> Option<Rect> {
        self.buttons.iter().find(|(x, _)| *x == h).map(|(_, r)| *r)
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<MHit> {
        // A tile's "×" sits on the tile: it is listed first.
        if let Some((h, _)) = self.buttons.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(*h);
        }
        self.pad.contains(x, y).then_some(MHit::Pad)
    }
}

/// Measured widths of the words the layout needs: tabs are equal thirds,
/// so only Ulangi, the photo button, Batal, Pakai.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct MLabels {
    pub redo: f32,
    pub pick: f32,
    pub cancel: f32,
    pub use_: f32,
}

pub(super) fn maker_layout(ui: f32, tab: Tab, saved: usize, has_ink: bool, lab: &MLabels) -> MLayout {
    let (wpx, pad) = (W * ui, PAD * ui);
    let inner = wpx - 2.0 * pad;
    let mut y = pad;
    let title = Rect { x: pad, y, w: inner, h: TITLE_H * ui };
    y += title.h + 10.0 * ui;
    let tabs = Rect { x: pad, y, w: inner, h: TABS_H * ui };
    let mut buttons = Vec::new();
    let seg = (inner - 6.0 * ui) / 3.0;
    for (i, t) in [Tab::Draw, Tab::Type, Tab::Photo].into_iter().enumerate() {
        buttons.push((
            MHit::Tab(t),
            Rect { x: tabs.x + 3.0 * ui + i as f32 * seg, y: tabs.y + 3.0 * ui, w: seg, h: tabs.h - 6.0 * ui },
        ));
    }
    y += tabs.h + 12.0 * ui;
    let area_h = if tab == Tab::Type { PAD_H + FONT_ROW_H + 8.0 } else { PAD_H };
    let area = Rect { x: pad, y, w: inner, h: area_h * ui };
    let mut field = Rect::default();
    let pad_rect = match tab {
        Tab::Type => {
            field = Rect { x: area.x, y: area.y, w: area.w, h: FIELD_H * ui };
            let row_h = FONT_ROW_H * ui;
            let gap = 8.0 * ui;
            let fw = (area.w - 3.0 * gap) / 4.0;
            for i in 0..4 {
                buttons.push((
                    MHit::Font(i),
                    Rect { x: area.x + i as f32 * (fw + gap), y: field.bottom() + gap, w: fw, h: row_h },
                ));
            }
            let top = field.h + gap + row_h + gap;
            Rect { x: area.x, y: area.y + top, w: area.w, h: area.h - top }
        }
        _ => area,
    };
    match tab {
        Tab::Draw if has_ink => {
            let w = lab.redo + 20.0 * ui;
            buttons.push((MHit::Redo, Rect { x: area.right() - w - 6.0 * ui, y: area.y + 6.0 * ui, w, h: 28.0 * ui }));
        }
        Tab::Photo => {
            let w = (lab.pick + 32.0 * ui).min(area.w - 24.0 * ui);
            let r = if has_ink {
                Rect { x: area.right() - w - 6.0 * ui, y: area.y + 6.0 * ui, w, h: 30.0 * ui }
            } else {
                Rect { x: area.x + (area.w - w) / 2.0, y: area.y + area.h / 2.0 - 30.0 * ui, w, h: 34.0 * ui }
            };
            buttons.push((MHit::PickPhoto, r));
        }
        _ => {}
    }
    y += area.h + 12.0 * ui;
    // Ink: the word, then two swatches with a generous hit area each.
    let sw = 30.0 * ui;
    let ink_x = pad + 44.0 * ui;
    for (i, blue) in [true, false].into_iter().enumerate() {
        buttons.push((MHit::Ink(blue), Rect { x: ink_x + i as f32 * (sw + 4.0 * ui), y: y + (INK_H * ui - sw) / 2.0, w: sw, h: sw }));
    }
    y += INK_H * ui + 10.0 * ui;
    buttons.push((MHit::Keep, Rect { x: pad - 4.0 * ui, y: y - 2.0 * ui, w: 24.0 * ui, h: 24.0 * ui }));
    let check_text = Rect { x: pad + 26.0 * ui, y, w: inner - 26.0 * ui, h: CHECK_H * ui };
    y += CHECK_H * ui;
    if saved > 0 {
        y += 12.0 * ui;
        let n = saved.min(TILES) + 1;
        let gap = 8.0 * ui;
        let tw = (inner - gap * (n - 1) as f32) / n as f32;
        for i in 0..n {
            let r = Rect { x: pad + i as f32 * (tw + gap), y, w: tw, h: TILE_H * ui };
            if i + 1 == n {
                buttons.push((MHit::New, r));
            } else {
                let x = 20.0 * ui;
                buttons.insert(
                    0,
                    (MHit::Forget(i), Rect { x: r.right() - x - 2.0 * ui, y: r.y + 2.0 * ui, w: x, h: x }),
                );
                buttons.push((MHit::Tile(i), r));
            }
        }
        y += TILE_H * ui;
    }
    y += 16.0 * ui;
    let use_w = lab.use_ + 32.0 * ui;
    let cancel_w = lab.cancel + 32.0 * ui;
    let use_r = Rect { x: wpx - pad - use_w, y, w: use_w, h: BTN_H * ui };
    buttons.push((MHit::Cancel, Rect { x: use_r.x - 8.0 * ui - cancel_w, y, w: cancel_w, h: BTN_H * ui }));
    buttons.push((MHit::Use, use_r));
    y += BTN_H * ui + pad;
    MLayout { size: (wpx, y), title, tabs, pad: pad_rect, field, buttons, check_text }
}

/// Pad px to the ink pad's logical units: one uniform scale that fits the
/// whole pad, so nothing drawn is cut off.
pub(super) fn pad_scale(pad: Rect) -> f32 {
    (INK_PAD.0 / pad.w.max(1.0)).min(INK_PAD.1 / pad.h.max(1.0))
}

pub(super) struct MakerResult {
    pub mark: Option<Mark>,
    pub initials: bool,
}

struct Maker {
    hwnd: HWND,
    parent: HWND,
    initials: bool,
    demo: bool,
    p: WindowPalette,
    ui: f32,
    gfx: Gfx,
    target: Option<Target>,
    labels: MLabels,
    layout: MLayout,
    tab: Tab,
    blue: bool,
    keep: bool,
    strokes: Vec<Vec<(f32, f32)>>,
    drawing: bool,
    /// The pen or finger drawing now (`WM_POINTER*`).
    pointer: Option<u32>,
    name: Option<Field>,
    typed: Option<Arc<Mark>>,
    photo_path: Option<PathBuf>,
    photo: Option<Arc<Mark>>,
    /// Why the last photo picked gave no mark.
    photo_failed: Option<ink::PhotoError>,
    saved: Vec<Arc<Mark>>,
    bmps: HashMap<usize, (Arc<Mark>, ID2D1Bitmap)>,
    hover: Option<MHit>,
    focus: Option<MHit>,
}

impl Drop for Maker {
    fn drop(&mut self) {
        if let Some(f) = self.name.take() {
            text_edit::free_gdi(&f);
        }
    }
}

thread_local! {
    static MAKERS: RefCell<HashMap<isize, Maker>> = RefCell::new(HashMap::new());
    static RESULTS: RefCell<HashMap<isize, MakerResult>> = RefCell::new(HashMap::new());
}

fn with<R>(hwnd: HWND, f: impl FnOnce(&mut Maker) -> R) -> Option<R> {
    MAKERS.with(|m| m.try_borrow_mut().ok().and_then(|mut m| m.get_mut(&(hwnd.0 as isize)).map(f)))
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn register(instance: HINSTANCE) -> windows::core::Result<()> {
    unsafe {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            let error = Error::from_thread();
            if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                return Err(error);
            }
        }
    }
    Ok(())
}

fn ink_of(blue: bool) -> Ink {
    if blue { Ink::Blue } else { Ink::Black }
}

/// Opens the maker over `parent` (centred in its `client` area).
pub(super) fn open(
    parent: HWND,
    initials: bool,
    demo: bool,
    p: WindowPalette,
    ui: f32,
    client: (u32, u32),
) -> Option<HWND> {
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None).ok()? }.into();
    register(instance).ok()?;
    let gfx = Gfx::new().ok()?;
    let m = |k: Key, weight: i32| gfx.measure(tr(k), 13.0 * ui, weight).0;
    let labels = MLabels {
        redo: m(Key::SignMakerRedo, 500),
        pick: m(Key::SignMakerPhotoPick, 500),
        cancel: m(Key::SignCancel, 500),
        use_: m(Key::SignMakerUse, 600),
    };
    let saved = super::saved_marks(initials, demo);
    let layout = maker_layout(ui, Tab::Draw, saved.len(), false, &labels);
    let (w, h) = layout.size;
    let (x, y) = centred(client, (w, h));
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            None,
            WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
            x,
            y,
            w as i32,
            h as i32,
            Some(parent),
            None,
            Some(instance),
            None,
        )
        .ok()?
    };
    round(hwnd, (w, h), ui);
    let mk = Maker {
        hwnd,
        parent,
        initials,
        demo,
        p,
        ui,
        gfx,
        target: None,
        labels,
        layout,
        tab: Tab::Draw,
        blue: true,
        keep: true,
        strokes: Vec::new(),
        drawing: false,
        pointer: None,
        name: None,
        typed: None,
        photo_path: None,
        photo: None,
        photo_failed: None,
        saved,
        bmps: HashMap::new(),
        hover: None,
        focus: None,
    };
    MAKERS.with(|m| m.borrow_mut().insert(hwnd.0 as isize, mk));
    take_focus(hwnd);
    Some(hwnd)
}

fn centred(client: (u32, u32), size: (f32, f32)) -> (i32, i32) {
    (((client.0 as f32 - size.0) / 2.0).max(0.0) as i32, ((client.1 as f32 - size.1) / 2.0).max(0.0) as i32)
}

fn round(hwnd: HWND, (w, h): (f32, f32), ui: f32) {
    let r = (16.0 * ui) as i32;
    unsafe {
        let rgn = CreateRoundRectRgn(0, 0, w as i32 + 1, h as i32 + 1, r, r);
        // The window owns the region from here on.
        let _ = SetWindowRgn(hwnd, Some(rgn), true);
    }
}

/// Keeps the maker centred when the signing window changes size.
pub(super) fn place(hwnd: HWND, client: (u32, u32), _ui: f32) {
    let size = with(hwnd, |m| m.layout.size);
    if let Some(size) = size {
        let (x, y) = centred(client, size);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
            );
        }
    }
}

pub(super) fn restyle(hwnd: HWND, p: WindowPalette) {
    with(hwnd, |m| m.p = p);
    invalidate(hwnd);
}

pub(super) fn take_result(parent: HWND) -> Option<MakerResult> {
    RESULTS.with(|r| r.borrow_mut().remove(&(parent.0 as isize)))
}

/// The layout after a change of tab, ink or saved marks; the window is
/// resized when its height changed.
fn relayout(hwnd: HWND) {
    let plan = with(hwnd, |m| {
        let has = match m.tab {
            Tab::Draw => !m.strokes.is_empty(),
            Tab::Type => false,
            Tab::Photo => m.photo.is_some(),
        };
        let old = m.layout.size;
        m.layout = maker_layout(m.ui, m.tab, m.saved.len(), has, &m.labels);
        if let Some(f) = &m.name {
            let r = m.layout.field;
            let inset = (6.0 * m.ui).round();
            text_edit::move_to(
                f.edit,
                (r.x + inset) as i32,
                (r.y + inset) as i32,
                (r.w - 2.0 * inset) as i32,
                (r.h - 2.0 * inset) as i32,
            );
        }
        (old != m.layout.size).then_some((m.layout.size, m.ui, m.parent))
    })
    .flatten();
    if let Some((size, ui, parent)) = plan {
        let mut rc = windows::Win32::Foundation::RECT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(parent, &mut rc);
            let (x, y) = centred(((rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32), size);
            let _ = SetWindowPos(
                hwnd,
                None,
                x,
                y,
                size.0 as i32,
                size.1 as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        round(hwnd, size, ui);
        with(hwnd, |m| {
            if let Some(t) = &m.target {
                t.resize(size.0 as u32, size.1 as u32);
            }
        });
    }
    invalidate(hwnd);
}

// ---- actions ----

fn finish(hwnd: HWND, mark: Option<Mark>, keep: bool) {
    let Some((parent, initials, demo)) = with(hwnd, |m| (m.parent, m.initials, m.demo)) else { return };
    if keep && let Some(mk) = &mark {
        super::keep_mark(mk, initials, demo);
    }
    RESULTS.with(|r| r.borrow_mut().insert(parent.0 as isize, MakerResult { mark, initials }));
    unsafe {
        let _ = PostMessageW(Some(parent), WM_MAKER, WPARAM(0), LPARAM(0));
    }
}

/// The mark made now, if there is one to use.
fn current_mark(m: &Maker) -> Option<Mark> {
    let ink = ink_of(m.blue);
    match m.tab {
        Tab::Draw => {
            let s = pad_scale(m.layout.pad);
            let logical: Vec<Vec<(f32, f32)>> = m
                .strokes
                .iter()
                .map(|st| st.iter().map(|(x, y)| ((x - m.layout.pad.x) * s, (y - m.layout.pad.y) * s)).collect())
                .collect();
            ink::from_strokes(&logical, ink)
        }
        Tab::Type => m.typed.as_deref().cloned(),
        Tab::Photo => m.photo.as_deref().cloned(),
    }
}

fn can_use(m: &Maker) -> bool {
    match m.tab {
        Tab::Draw => !m.strokes.is_empty(),
        Tab::Type => m.typed.is_some(),
        Tab::Photo => m.photo.is_some(),
    }
}

fn retype(m: &mut Maker) {
    let name = m.name.as_ref().map(|f| text_edit::text(f.edit)).unwrap_or_default();
    m.typed = ink::from_text(&name, ink_of(m.blue), hand::last()).map(Arc::new);
}

fn act(hwnd: HWND, hit: MHit) {
    match hit {
        MHit::Tab(t) => set_tab(hwnd, t),
        MHit::Redo => {
            with(hwnd, |m| m.strokes.clear());
            relayout(hwnd);
        }
        MHit::PickPhoto => pick_photo(hwnd),
        MHit::Font(_) => {}
        MHit::Ink(blue) => {
            with(hwnd, |m| {
                m.blue = blue;
                if m.tab == Tab::Type {
                    retype(m);
                }
                if let Some(p) = m.photo_path.clone() {
                    m.photo = ink::from_photo(&p, ink_of(blue)).ok().map(Arc::new);
                }
            });
        }
        MHit::Keep => {
            with(hwnd, |m| m.keep = !m.keep);
        }
        MHit::Tile(i) => {
            let mark = with(hwnd, |m| m.saved.get(i).map(|a| (**a).clone())).flatten();
            if mark.is_some() {
                finish(hwnd, mark, false);
            }
        }
        MHit::Forget(i) => {
            with(hwnd, |m| {
                super::forget_mark(m.initials, i, m.demo);
                m.saved = super::saved_marks(m.initials, m.demo);
                m.focus = None;
            });
            relayout(hwnd);
        }
        MHit::New => {
            with(hwnd, |m| {
                m.strokes.clear();
                m.photo = None;
                m.photo_path = None;
                if let Some(f) = &m.name {
                    text_edit::clear(f.edit);
                }
                m.typed = None;
            });
            relayout(hwnd);
        }
        MHit::Cancel => finish(hwnd, None, false),
        MHit::Use => {
            let made = with(hwnd, |m| can_use(m).then(|| (current_mark(m), m.keep))).flatten();
            if let Some((Some(mark), keep)) = made {
                finish(hwnd, Some(mark), keep);
            }
        }
        MHit::Pad => {}
    }
    invalidate(hwnd);
}

fn set_tab(hwnd: HWND, t: Tab) {
    let need = with(hwnd, |m| {
        m.tab = t;
        t == Tab::Type && m.name.is_none()
    })
    .unwrap_or(false);
    if need && let Some(edit) = make_name(hwnd, "") {
        take_focus(edit);
    }
    show_name(hwnd);
    relayout(hwnd);
}

/// The name field (Ketik), made outside the maker's borrow (making an EDIT
/// sends messages to its parent), holding `initial`. Returns its HWND.
fn make_name(hwnd: HWND, initial: &str) -> Option<HWND> {
    let (r, ui, p) = with(hwnd, |m| (m.layout.field, m.ui, m.p))?;
    let inset = (6.0 * ui).round();
    let spec = text_edit::Spec {
        rect: (
            (r.x + inset) as i32,
            (r.y + inset) as i32,
            (r.w - 2.0 * inset) as i32,
            (r.h - 2.0 * inset) as i32,
        ),
        initial,
        px: 15.0 * ui,
        face: crate::ui::fonts::gdi_text_face(),
        color: p.text,
        back: p.field,
        password: false,
        commit_on_blur: false,
        bold: false,
        italic: false,
        underline: false,
        format_keys: false,
    };
    let f = text_edit::create(hwnd, &spec)?;
    let edit = f.edit;
    let mut slot = Some(f);
    with(hwnd, |m| m.name = slot.take());
    if let Some(f) = slot {
        text_edit::destroy(f);
        return None;
    }
    Some(edit)
}

fn show_name(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOW, ShowWindow};
    let show = with(hwnd, |m| m.name.as_ref().map(|f| (f.edit, m.tab == Tab::Type))).flatten();
    if let Some((edit, on)) = show {
        unsafe {
            let _ = ShowWindow(edit, if on { SW_SHOW } else { SW_HIDE });
        }
    }
}

/// The signing window moved to a monitor with another scale: new sizes, the
/// drawing kept on the pad, the name field remade at the new font size.
pub(super) fn rescale(hwnd: HWND, ui: f32) {
    let plan = with(hwnd, |m| {
        if (m.ui - ui).abs() < f32::EPSILON {
            return None;
        }
        m.ui = ui;
        let mw = |k: Key, weight: i32| m.gfx.measure(tr(k), 13.0 * ui, weight).0;
        m.labels = MLabels {
            redo: mw(Key::SignMakerRedo, 500),
            pick: mw(Key::SignMakerPhotoPick, 500),
            cancel: mw(Key::SignCancel, 500),
            use_: mw(Key::SignMakerUse, 600),
        };
        m.bmps.clear();
        Some((m.layout.pad, m.name.take()))
    })
    .flatten();
    let Some((old_pad, name)) = plan else { return };
    let typed = name.map(|f| {
        let t = text_edit::text(f.edit);
        text_edit::destroy(f);
        t
    });
    relayout(hwnd);
    with(hwnd, |m| {
        let new = m.layout.pad;
        let k = new.w / old_pad.w.max(1.0);
        for s in &mut m.strokes {
            for p in s.iter_mut() {
                *p = (new.x + (p.0 - old_pad.x) * k, new.y + (p.1 - old_pad.y) * k);
            }
        }
    });
    if let Some(t) = typed {
        make_name(hwnd, &t);
        show_name(hwnd);
    }
    invalidate(hwnd);
}

/// The system's file dialog, images only. Runs modal on this thread.
fn pick_photo(hwnd: HWND) {
    let owner = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let path = unsafe {
        (|| -> Option<PathBuf> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let options = dialog.GetOptions().ok()?;
            let _ = dialog.SetOptions(options | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST);
            let name = HSTRING::from(tr(Key::SignMakerPhoto));
            let filters = [COMDLG_FILTERSPEC {
                pszName: PCWSTR(name.as_ptr()),
                pszSpec: w!("*.jpg;*.jpeg;*.png;*.bmp;*.gif;*.tif;*.tiff;*.heic;*.webp"),
            }];
            let _ = dialog.SetFileTypes(&filters);
            let _ = dialog.SetTitle(&HSTRING::from(tr(Key::SignMakerPhotoPick)));
            dialog.Show(Some(owner)).ok()?;
            let item = dialog.GetResult().ok()?;
            let raw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let p = raw.to_string().ok().map(PathBuf::from);
            CoTaskMemFree(Some(raw.0 as *const c_void));
            p
        })()
    };
    let Some(path) = path else { return };
    with(hwnd, |m| {
        let made = ink::from_photo(&path, ink_of(m.blue));
        m.photo_failed = made.as_ref().err().copied();
        m.photo = made.ok().map(Arc::new);
        m.photo_path = m.photo.is_some().then_some(path);
    });
    relayout(hwnd);
}

// ---- input ----

fn xy(lparam: LPARAM) -> (f32, f32) {
    ((lparam.0 & 0xFFFF) as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as i16 as f32)
}

fn pen_down(hwnd: HWND, x: f32, y: f32) -> bool {
    with(hwnd, |m| {
        if m.tab != Tab::Draw || !m.layout.pad.contains(x, y) {
            return false;
        }
        m.strokes.push(vec![(x, y)]);
        m.drawing = true;
        true
    })
    .unwrap_or(false)
}

fn pen_move(hwnd: HWND, x: f32, y: f32) {
    let drew = with(hwnd, |m| {
        if !m.drawing {
            return false;
        }
        let p = m.layout.pad;
        let pt = (x.clamp(p.x, p.right()), y.clamp(p.y, p.bottom()));
        if let Some(s) = m.strokes.last_mut() {
            s.push(pt);
        }
        true
    })
    .unwrap_or(false);
    if drew {
        invalidate(hwnd);
    }
}

fn pen_up(hwnd: HWND) {
    let first = with(hwnd, |m| {
        let was = m.drawing;
        m.drawing = false;
        m.pointer = None;
        was && m.strokes.len() == 1
    })
    .unwrap_or(false);
    if first {
        // Ulangi appears with the first stroke.
        relayout(hwnd);
    }
    invalidate(hwnd);
}

fn ring(m: &Maker) -> Vec<MHit> {
    ring_of(&m.layout.buttons)
}

fn ring_of(buttons: &[(MHit, Rect)]) -> Vec<MHit> {
    let mut order: Vec<MHit> = buttons
        .iter()
        .map(|(h, _)| *h)
        .filter(|h| !matches!(h, MHit::Forget(_)))
        .collect();
    order.sort_by_key(|h| match h {
        MHit::Tab(_) => 0,
        MHit::Redo | MHit::PickPhoto | MHit::Font(_) => 1,
        MHit::Ink(_) => 2,
        MHit::Keep => 3,
        MHit::Tile(_) | MHit::New => 4,
        _ => 5,
    });
    order
}

fn on_key(hwnd: HWND, vk: u16) {
    let shift = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
    match vk {
        v if v == VK_ESCAPE.0 => act(hwnd, MHit::Cancel),
        v if v == VK_TAB.0 => {
            let edit = with(hwnd, |m| {
                let order = ring(m);
                let at = m.focus.and_then(|f| order.iter().position(|h| *h == f));
                let n = order.len().max(1);
                let next = match (at, shift) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                };
                m.focus = order.get(next).copied();
                // From the tabs into the name field when typing.
                (m.tab == Tab::Type && m.focus == Some(MHit::Ink(true)) && !shift)
                    .then(|| m.name.as_ref().map(|f| f.edit))
                    .flatten()
            })
            .flatten();
            if let Some(e) = edit {
                with(hwnd, |m| m.focus = None);
                take_focus(e);
            }
            invalidate(hwnd);
        }
        v if v == VK_RETURN.0 || v == VK_SPACE.0 => {
            let f = with(hwnd, |m| m.focus).flatten();
            match f {
                Some(h) => act(hwnd, h),
                None if v == VK_RETURN.0 => act(hwnd, MHit::Use),
                None => {}
            }
        }
        v if v == VK_DELETE.0 || v == VK_BACK.0 => {
            if let Some(Some(MHit::Tile(i))) = with(hwnd, |m| m.focus) {
                act(hwnd, MHit::Forget(i));
            }
        }
        _ => {}
    }
}

// ---- drawing ----

fn mark_bitmap(m: &mut Maker, rt: &ID2D1RenderTarget, mk: &Arc<Mark>) -> Option<ID2D1Bitmap> {
    let key = Arc::as_ptr(mk) as usize;
    if let Some((_, b)) = m.bmps.get(&key) {
        return Some(b.clone());
    }
    if mk.w == 0 || mk.rgba.len() != mk.w as usize * mk.h as usize * 4 {
        return None;
    }
    let b = bitmap(rt, mk.w, mk.h, &premultiplied(mk)).ok()?;
    m.bmps.insert(key, (mk.clone(), b.clone()));
    Some(b)
}

fn draw_fitted(m: &mut Maker, rt: &ID2D1RenderTarget, mk: &Arc<Mark>, r: Rect) {
    let Some(b) = mark_bitmap(m, rt, mk) else { return };
    let (mw, mh) = (mk.w.max(1) as f32, mk.h.max(1) as f32);
    let s = (r.w / mw).min(r.h / mh);
    let (dw, dh) = (mw * s, mh * s);
    unsafe {
        rt.DrawBitmap(
            &b,
            Some(&rect_f(r.x + (r.w - dw) / 2.0, r.y + (r.h - dh) / 2.0, dw, dh)),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            None,
        );
    }
}

fn paint(m: &mut Maker) -> windows::core::Result<()> {
    let (w, h) = m.layout.size;
    if m.target.is_none() {
        m.target = Some(m.gfx.hwnd_target(m.hwnd, (w as u32, h as u32))?);
    }
    let Some(t) = m.target.as_ref() else { return Ok(()) };
    let rt: ID2D1RenderTarget = windows::core::Interface::cast(&t.rt)?;
    let brush = t.brush.clone();
    let (p, ui) = (m.p, m.ui);
    let l = m.layout.clone();
    let (b_rgb, b_a) = (p.border >> 8, (p.border & 0xFF) as f32 / 255.0);
    unsafe {
        rt.BeginDraw();
        rt.Clear(Some(&color(p.popover, 1.0)));
        brush.SetColor(&color(b_rgb, b_a.max(0.2)));
        rt.DrawRoundedRectangle(
            &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT { rect: rect_f(
                0.5,
                0.5,
                w - 1.0,
                h - 1.0,
            ), radiusX: 16.0 * ui, radiusY: 16.0 * ui },
            &brush,
            1.0,
            None,
        );
    }
    let title = if m.initials { Key::SignMakerNewInitials } else { Key::SignMakerNewSignature };
    m.gfx.text_in(
        &rt,
        &brush,
        tr(title),
        (l.title.x, l.title.y, l.title.w, l.title.h),
        14.0 * ui,
        600,
        p.text,
        1.0,
        Align::Left,
    );

    // Tabs: a sunken strip with the chosen one raised.
    fill_round(&rt, &brush, l.tabs, 10.0 * ui, p.bg, 1.0);
    for (hit, r) in l.buttons.iter().filter(|(h, _)| matches!(h, MHit::Tab(_))) {
        let MHit::Tab(t) = hit else { continue };
        let on = *t == m.tab;
        if on {
            fill_round(&rt, &brush, *r, 8.0 * ui, p.hover, 1.0);
        } else if m.hover == Some(*hit) {
            fill_round(&rt, &brush, *r, 8.0 * ui, p.hover, 0.5);
        }
        let k = match t {
            Tab::Draw => Key::SignMakerDraw,
            Tab::Type => Key::SignMakerType,
            Tab::Photo => Key::SignMakerPhoto,
        };
        m.gfx.text_in(&rt, &brush, tr(k), (r.x, r.y, r.w, r.h), 13.0 * ui, if on { 600 } else { 400 },
            if on { p.text } else { p.secondary }, 1.0, Align::Center);
    }

    // The paper.
    let pad = l.pad;
    fill_round(&rt, &brush, pad, 10.0 * ui, PAPER, 1.0);
    let ink_rgb = if m.blue { 0x1F3A8A } else { 0x1C1C1C };
    match m.tab {
        Tab::Draw => {
            unsafe {
                brush.SetColor(&color(BASELINE, 1.0));
                let y = pad.bottom() - 28.0 * ui;
                rt.DrawLine(
                    Vector2::new(pad.x + 24.0 * ui, y),
                    Vector2::new(pad.right() - 24.0 * ui, y),
                    &brush,
                    1.0,
                    &m.gfx.dash,
                );
            }
            if m.strokes.is_empty() {
                m.gfx.text_in(&rt, &brush, tr(Key::SignMakerPadHint), (
                    pad.x + 12.0 * ui,
                    pad.y,
                    pad.w - 24.0 * ui,
                    pad.h - 40.0 * ui,
                ),
                    12.5 * ui, 400, HINT_ON_PAPER, 1.0, Align::Center);
            }
            let width = INK_STROKE / pad_scale(pad);
            unsafe {
                brush.SetColor(&color(ink_rgb, 1.0));
                for s in &m.strokes {
                    if s.len() == 1 {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE { point: Vector2::new(
                                s[0].0,
                                s[0].1,
                            ), radiusX: width / 2.0, radiusY: width / 2.0 },
                            &brush,
                        );
                        continue;
                    }
                    let Ok(geo) = m.gfx.d2d.CreatePathGeometry() else { continue };
                    let Ok(sink) = geo.Open() else { continue };
                    sink.BeginFigure(Vector2::new(s[0].0, s[0].1), D2D1_FIGURE_BEGIN_HOLLOW);
                    for (x, y) in &s[1..] {
                        sink.AddLine(Vector2::new(*x, *y));
                    }
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    if sink.Close().is_ok() {
                        rt.DrawGeometry(&geo, &brush, width, &m.gfx.round);
                    }
                }
            }
        }
        Tab::Type => {
            fill_round(&rt, &brush, l.field, 8.0 * ui, p.field, 1.0);
            unsafe {
                brush.SetColor(&color(b_rgb, b_a.max(0.3)));
                rt.DrawRoundedRectangle(
                    &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT { rect: rect_f(
                        l.field.x,
                        l.field.y,
                        l.field.w,
                        l.field.h,
                    ), radiusX: 8.0 * ui, radiusY: 8.0 * ui },
                    &brush,
                    1.0,
                    None,
                );
            }
            match m.typed.clone() {
                Some(mk) => draw_fitted(
                    m,
                    &rt,
                    &mk,
                    Rect { x: pad.x + 16.0 * ui, y: pad.y + 8.0 * ui, w: pad.w - 32.0 * ui, h: pad.h - 16.0 * ui },
                ),
                None => m.gfx.text_in(
                    &rt,
                    &brush,
                    tr(Key::SignMakerTypeHint),
                    (pad.x, pad.y, pad.w, pad.h),
                    12.5 * ui,
                    400,
                    HINT_ON_PAPER,
                    1.0,
                    Align::Center,
                ),
            }
        }
        Tab::Photo => {
            if let Some(mk) = m.photo.clone() {
                draw_fitted(
                    m,
                    &rt,
                    &mk,
                    Rect { x: pad.x + 16.0 * ui, y: pad.y + 40.0 * ui, w: pad.w - 32.0 * ui, h: pad.h - 52.0 * ui },
                );
            } else if let Some(e) = m.photo_failed {
                let why = match e {
                    ink::PhotoError::TooBig => Key::SignMakerPhotoTooBig,
                    ink::PhotoError::Unusable => Key::SignMakerPhotoFailed,
                };
                m.gfx.text_in(&rt, &brush, tr(why), (
                    pad.x + 12.0 * ui,
                    pad.bottom() - 44.0 * ui,
                    pad.w - 24.0 * ui,
                    32.0 * ui,
                ),
                    11.5 * ui, 500, ERROR_ON_PAPER, 1.0, Align::Center);
            }
        }
    }

    // Ink.
    let ink_label = Rect { x: PAD * ui, y: l.rect_of(MHit::Ink(true)).map_or(
        0.0,
        |r| r.y,
    ), w: 40.0 * ui, h: 30.0 * ui };
    m.gfx.text_in(
        &rt,
        &brush,
        tr(Key::SignMakerInk),
        (ink_label.x, ink_label.y, ink_label.w, ink_label.h),
        13.0 * ui,
        400,
        p.secondary,
        1.0,
        Align::Left,
    );
    for blue in [true, false] {
        let Some(r) = l.rect_of(MHit::Ink(blue)) else { continue };
        let c = Vector2::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
        let ell = |rad: f32| D2D1_ELLIPSE { point: c, radiusX: rad, radiusY: rad };
        let rad = SWATCH * ui / 2.0;
        unsafe {
            brush.SetColor(&color(if blue { 0x1F3A8A } else { 0x1C1C1C }, 1.0));
            rt.FillEllipse(&ell(rad), &brush);
            // A light ring, so black ink shows on a dark popover.
            brush.SetColor(&color(p.text, 0.55));
            rt.DrawEllipse(&ell(rad), &brush, 1.5 * ui, None);
            if m.blue == blue {
                brush.SetColor(&color(p.accent, 1.0));
                rt.DrawEllipse(&ell(rad + 4.0 * ui), &brush, 2.0 * ui, None);
            }
        }
    }

    // Keep on this PC.
    if let Some(r) = l.rect_of(MHit::Keep) {
        let b = Rect { x: r.x + 4.0 * ui, y: r.y + 4.0 * ui, w: 16.0 * ui, h: 16.0 * ui };
        if m.keep {
            fill_round(&rt, &brush, b, 4.0 * ui, p.accent, 1.0);
            unsafe {
                brush.SetColor(&color(p.on_accent, 1.0));
                let at = |fx: f32, fy: f32| Vector2::new(b.x + b.w * fx, b.y + b.h * fy);
                rt.DrawLine(at(0.22, 0.52), at(0.42, 0.72), &brush, 2.0 * ui, &m.gfx.round);
                rt.DrawLine(at(0.42, 0.72), at(0.79, 0.3), &brush, 2.0 * ui, &m.gfx.round);
            }
        } else {
            unsafe {
                brush.SetColor(&color(p.secondary, 1.0));
                rt.DrawRoundedRectangle(
                    &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT { rect: rect_f(
                        b.x + 0.75 * ui,
                        b.y + 0.75 * ui,
                        b.w - 1.5 * ui,
                        b.h - 1.5 * ui,
                    ), radiusX: 4.0 * ui, radiusY: 4.0 * ui },
                    &brush,
                    1.5 * ui,
                    None,
                );
            }
        }
    }
    let ct = l.check_text;
    m.gfx.text_in(
        &rt,
        &brush,
        tr(Key::SignMakerKeep),
        (ct.x, ct.y, ct.w, 18.0 * ui),
        13.0 * ui,
        500,
        p.text,
        1.0,
        Align::Left,
    );
    if let Ok(layout) = m.gfx.layout(tr(Key::SignMakerKeepHint), 12.0 * ui, 400) {
        unsafe {
            let _ = layout.SetMaxWidth(ct.w);
            let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            let _ = layout.SetMaxHeight(ct.h - 20.0 * ui);
            let mut mt = DWRITE_TEXT_METRICS::default();
            let _ = layout.GetMetrics(&mut mt);
            brush.SetColor(&color(p.secondary, 1.0));
            rt.DrawTextLayout(
                Vector2::new(ct.x, ct.y + 20.0 * ui),
                &layout,
                &brush,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
        }
    }

    // Saved marks.
    let saved = m.saved.clone();
    for (hit, r) in &l.buttons {
        match hit {
            MHit::Tile(i) => {
                fill_round(&rt, &brush, *r, 8.0 * ui, PAPER, 1.0);
                if m.hover == Some(*hit) {
                    unsafe {
                        brush.SetColor(&color(p.accent, 1.0));
                        rt.DrawRoundedRectangle(
                            &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT { rect: rect_f(
                                r.x + 1.0,
                                r.y + 1.0,
                                r.w - 2.0,
                                r.h - 2.0,
                            ), radiusX: 8.0 * ui, radiusY: 8.0 * ui },
                            &brush,
                            2.0 * ui,
                            None,
                        );
                    }
                }
                if let Some(mk) = saved.get(*i) {
                    draw_fitted(
                        m,
                        &rt,
                        mk,
                        Rect { x: r.x + 8.0 * ui, y: r.y + 6.0 * ui, w: r.w - 30.0 * ui, h: r.h - 12.0 * ui },
                    );
                }
            }
            MHit::New => {
                if m.hover == Some(*hit) {
                    fill_round(&rt, &brush, *r, 8.0 * ui, p.hover, 1.0);
                }
                unsafe {
                    brush.SetColor(&color(p.secondary, 0.8));
                    rt.DrawRoundedRectangle(
                        &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT { rect: rect_f(
                            r.x + 0.5,
                            r.y + 0.5,
                            r.w - 1.0,
                            r.h - 1.0,
                        ), radiusX: 8.0 * ui, radiusY: 8.0 * ui },
                        &brush,
                        1.0,
                        &m.gfx.dash,
                    );
                }
                m.gfx.text_in(
                    &rt,
                    &brush,
                    tr(Key::SignMakerNew),
                    (r.x, r.y, r.w, r.h),
                    12.5 * ui,
                    500,
                    p.text,
                    1.0,
                    Align::Center,
                );
            }
            _ => {}
        }
    }
    // Each tile's "×" (drawn after the tiles it sits on).
    for (hit, r) in &l.buttons {
        if let MHit::Forget(_) = hit {
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            let hot = m.hover == Some(*hit);
            unsafe {
                brush.SetColor(&color(if hot { 0x221A14 } else { 0x6E5F52 }, 1.0));
                let d = 3.5 * ui;
                rt.DrawLine(
                    Vector2::new(cx - d, cy - d),
                    Vector2::new(cx + d, cy + d),
                    &brush,
                    1.6 * ui,
                    &m.gfx.round,
                );
                rt.DrawLine(
                    Vector2::new(cx - d, cy + d),
                    Vector2::new(cx + d, cy - d),
                    &brush,
                    1.6 * ui,
                    &m.gfx.round,
                );
            }
        }
    }

    // Buttons.
    let usable = can_use(m);
    for (hit, r) in &l.buttons {
        let (k, primary) = match hit {
            MHit::Redo => (Key::SignMakerRedo, false),
            MHit::PickPhoto => (Key::SignMakerPhotoPick, false),
            MHit::Cancel => (Key::SignCancel, false),
            MHit::Use => (Key::SignMakerUse, true),
            _ => continue,
        };
        let hot = m.hover == Some(*hit);
        let on_paper = matches!(hit, MHit::Redo | MHit::PickPhoto);
        let a = if *hit == MHit::Use && !usable { 0.45 } else { 1.0 };
        let (bg, fg) = if primary {
            (p.accent, p.on_accent)
        } else if on_paper {
            (if hot { 0xEFE6DA } else { 0xF6F0E8 }, 0x221A14)
        } else if hot {
            (p.button_hover, p.text)
        } else {
            (p.button, p.text)
        };
        fill_round(&rt, &brush, *r, 9.0 * ui, bg, a);
        m.gfx.text_in(
            &rt,
            &brush,
            tr(k),
            (r.x, r.y, r.w, r.h),
            13.0 * ui,
            if primary { 600 } else { 500 },
            fg,
            a,
            Align::Center,
        );
    }

    if let Some(r) = m.focus.and_then(|f| l.rect_of(f)) {
        unsafe {
            brush.SetColor(&color(p.accent, 1.0));
            rt.DrawRoundedRectangle(
                &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT { rect: rect_f(
                    r.x - 2.0 * ui,
                    r.y - 2.0 * ui,
                    r.w + 4.0 * ui,
                    r.h + 4.0 * ui,
                ), radiusX: 10.0 * ui, radiusY: 10.0 * ui },
                &brush,
                2.0 * ui,
                None,
            );
        }
    }
    unsafe { rt.EndDraw(None, None) }
}

// ---- the window procedure ----

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            unsafe {
                BeginPaint(hwnd, &mut ps);
            }
            let failed = with(hwnd, |m| {
                let drawn = paint(m);
                super::paint::evict_unused(&mut m.bmps);
                if let Err(e) = drawn {
                    m.target = None;
                    m.bmps.clear();
                    return e.code() == windows::Win32::Foundation::D2DERR_RECREATE_TARGET;
                }
                false
            });
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            if failed == Some(true) {
                invalidate(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = xy(lparam);
            with(hwnd, |m| m.focus = None);
            if pen_down(hwnd, x, y) {
                unsafe {
                    SetCapture(hwnd);
                }
            } else if let Some(Some(h)) = with(hwnd, |m| m.layout.hit(x, y)) {
                act(hwnd, h);
            }
            let typing = with(hwnd, |m| m.tab == Tab::Type).unwrap_or(false);
            if !typing {
                take_focus(hwnd);
            }
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = xy(lparam);
            pen_move(hwnd, x, y);
            let changed = with(hwnd, |m| {
                let h = m.layout.hit(x, y).filter(|h| *h != MHit::Pad);
                let c = h != m.hover;
                m.hover = h;
                c
            });
            if changed == Some(true) {
                invalidate(hwnd);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            pen_up(hwnd);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            if with(hwnd, |m| m.drawing && m.pointer.is_none()).unwrap_or(false) {
                pen_up(hwnd);
            }
            LRESULT(0)
        }
        WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP => {
            // Pen and touch draw on the pad; anywhere else they are left to
            // Windows, which turns them into clicks.
            let id = (wparam.0 & 0xFFFF) as u32;
            let mut pt = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut pt);
            }
            let (x, y) = (pt.x as f32, pt.y as f32);
            let mine = with(hwnd, |m| m.pointer == Some(id)).unwrap_or(false);
            match msg {
                WM_POINTERDOWN => {
                    if pen_down(hwnd, x, y) {
                        with(hwnd, |m| m.pointer = Some(id));
                        return LRESULT(0);
                    }
                }
                WM_POINTERUPDATE if mine => {
                    const IN_CONTACT: usize = 0x0004;
                    if (wparam.0 >> 16) & IN_CONTACT != 0 {
                        pen_move(hwnd, x, y);
                    }
                    return LRESULT(0);
                }
                WM_POINTERUP if mine => {
                    pen_up(hwnd);
                    return LRESULT(0);
                }
                _ => {}
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SETCURSOR => {
            let mut at = POINT::default();
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut at);
                let _ = ScreenToClient(hwnd, &mut at);
            }
            let cur = with(hwnd, |m| match m.layout.hit(at.x as f32, at.y as f32) {
                Some(MHit::Pad) if m.tab == Tab::Draw => IDC_CROSS,
                Some(MHit::Pad) | None => IDC_ARROW,
                Some(_) => IDC_HAND,
            })
            .unwrap_or(IDC_ARROW);
            unsafe {
                if let Ok(c) = LoadCursorW(None, cur) {
                    SetCursor(Some(c));
                }
            }
            LRESULT(1)
        }
        WM_KEYDOWN => {
            on_key(hwnd, (wparam.0 & 0xFFFF) as u16);
            LRESULT(0)
        }
        WM_SIGN_EDIT => {
            let current = with(hwnd, |m| m.name.as_ref().map(|f| f.edit.0 as isize)).flatten();
            if text_edit::msg_is_current(current, lparam.0) {
                match wparam.0 {
                    EDIT_COMMIT => act(hwnd, MHit::Use),
                    EDIT_CANCEL => act(hwnd, MHit::Cancel),
                    EDIT_TAB => {
                        with(hwnd, |m| m.focus = Some(MHit::Ink(true)));
                        take_focus(hwnd);
                        invalidate(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            if text_edit::is_change(wparam) {
                with(hwnd, retype);
                invalidate(hwnd);
            }
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            let brush = with(hwnd, |m| m.name.as_ref().map(|f| text_edit::ctl_color(f, wparam))).flatten();
            match brush {
                Some(b) => b,
                None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        WM_DESTROY => {
            let gone = MAKERS.with(|m| m.try_borrow_mut().ok().and_then(|mut m| m.remove(&(hwnd.0 as isize))));
            drop(gone);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAB: MLabels = MLabels { redo: 42.0, pick: 230.0, cancel: 38.0, use_: 38.0 };

    #[test]
    fn the_type_tab_has_four_font_tiles_between_field_and_preview() {
        let t = maker_layout(1.0, Tab::Type, 0, false, &LAB);
        let f: Vec<Rect> = (0..4).map(|i| t.rect_of(MHit::Font(i)).expect("tile")).collect();
        assert!(t.rect_of(MHit::Font(4)).is_none());
        for r in &f {
            assert_eq!(r.h, FONT_ROW_H);
            assert!(r.y >= t.field.bottom() + 7.99 && r.bottom() <= t.pad.y - 7.99, "{r:?}");
        }
        assert!((f[0].x - t.field.x).abs() < 0.01 && (f[3].right() - t.field.right()).abs() < 0.01);
        for i in 1..4 {
            assert!((f[i].w - f[0].w).abs() < 0.01);
            assert!((f[i].x - f[i - 1].right() - 8.0).abs() < 0.01);
        }
        assert_eq!(t.pad.h, PAD_H - FIELD_H - 8.0, "the preview keeps its height");
        let d = maker_layout(1.0, Tab::Draw, 0, false, &LAB);
        assert!((t.size.1 - d.size.1 - (FONT_ROW_H + 8.0)).abs() < 0.01);
        assert_eq!(t.hit(f[2].x + f[2].w / 2.0, f[2].y + f[2].h / 2.0), Some(MHit::Font(2)));
        for tab in [Tab::Draw, Tab::Photo] {
            let l = maker_layout(1.0, tab, 2, true, &LAB);
            assert!(!l.buttons.iter().any(|(h, _)| matches!(h, MHit::Font(_))));
        }
    }

    #[test]
    fn font_tiles_come_after_the_tabs_and_before_the_ink() {
        let t = maker_layout(1.0, Tab::Type, 1, false, &LAB);
        let order = ring_of(&t.buttons);
        let at = |h| order.iter().position(|x| *x == h).unwrap();
        assert!(at(MHit::Tab(Tab::Photo)) < at(MHit::Font(0)));
        assert_eq!([at(MHit::Font(1)), at(MHit::Font(2)), at(MHit::Font(3))], [at(MHit::Font(0)) + 1, at(MHit::Font(0)) + 2, at(MHit::Font(0)) + 3]);
        assert!(at(MHit::Font(3)) < at(MHit::Ink(true)));
    }

    fn all_inside(l: &MLayout) {
        let outer = Rect { x: 0.0, y: 0.0, w: l.size.0, h: l.size.1 };
        for (h, r) in &l.buttons {
            assert!(
                r.x >= outer.x - 4.01 && r.right() <= outer.right() && r.bottom() <= outer.bottom(),
                "{h:?} {r:?}",
            );
        }
    }

    #[test]
    fn the_maker_is_400_wide_and_holds_its_parts() {
        for tab in [Tab::Draw, Tab::Type, Tab::Photo] {
            for saved in [0, 2, 4] {
                let l = maker_layout(1.0, tab, saved, true, &LAB);
                assert_eq!(l.size.0, 400.0);
                all_inside(&l);
                assert!(l.rect_of(MHit::Use).unwrap().x > l.rect_of(MHit::Cancel).unwrap().right());
                let tiles = l.buttons.iter().filter(|(h, _)| matches!(h, MHit::Tile(_))).count();
                assert_eq!(tiles, saved.min(TILES));
                assert_eq!(l.rect_of(MHit::New).is_some(), saved > 0);
            }
        }
        // The pad is 150 high when drawing; typing puts the field on top.
        let d = maker_layout(1.0, Tab::Draw, 0, false, &LAB);
        assert_eq!(d.pad.h, PAD_H);
        assert!(d.rect_of(MHit::Redo).is_none(), "nothing to start over yet");
        assert!(maker_layout(1.0, Tab::Draw, 0, true, &LAB).rect_of(MHit::Redo).is_some());
        let t = maker_layout(1.0, Tab::Type, 0, false, &LAB);
        assert!(t.field.bottom() <= t.pad.y);
        // At 2x everything doubles.
        let d2 = maker_layout(2.0, Tab::Draw, 0, false, &LAB);
        assert_eq!(d2.size.0, 800.0);
        assert!((d2.size.1 - 2.0 * d.size.1).abs() < 0.5);
    }

    #[test]
    fn a_tiles_cross_wins_over_the_tile() {
        let l = maker_layout(1.0, Tab::Draw, 2, false, &LAB);
        let x = l.rect_of(MHit::Forget(1)).unwrap();
        assert_eq!(l.hit(x.x + x.w / 2.0, x.y + x.h / 2.0), Some(MHit::Forget(1)));
        let t = l.rect_of(MHit::Tile(1)).unwrap();
        assert_eq!(l.hit(t.x + 4.0, t.bottom() - 4.0), Some(MHit::Tile(1)));
        assert_eq!(l.hit(l.pad.x + 40.0, l.pad.y + 70.0), Some(MHit::Pad));
    }

    #[test]
    fn pad_strokes_fit_the_ink_pad_whole() {
        let pad = Rect { x: 16.0, y: 100.0, w: 368.0, h: 150.0 };
        let s = pad_scale(pad);
        assert!(pad.w * s <= INK_PAD.0 + 0.01 && pad.h * s <= INK_PAD.1 + 0.01);
        assert!((pad.h * s - INK_PAD.1).abs() < 0.01, "the height limits it");
    }
}
