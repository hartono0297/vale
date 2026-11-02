//! The overlay's drawing phase ("Langsung di layar"): the region is picked,
//! the frozen screen stays up, and the person draws on it in place with a
//! tool strip beside the selection and an action bar under it.
//!
//! Annotations live in frame pixels: (0, 0) is the frame's top-left, which is
//! also the overlay's client (0, 0), and the view is the identity. The
//! selection is in virtual-screen pixels (the frame's origin added), like the
//! pointer the pure rules (`inline`) work with. The mosaic and the export go
//! through the same painter and the same crop as the editor's, so what is
//! seen is what is copied or saved.
//!
//! Nothing runs at idle: no timer and no worker; a redraw happens when
//! something changed.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_ELLIPSE,
    ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, IDC_ARROW, IDC_CROSS, IDC_IBEAM, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS,
    IDC_SIZENWSE, IDC_SIZEWE, IsWindow, LoadCursorW, SetCursor,
};
use windows::core::PCWSTR;
use windows_numerics::Vector2;

use super::{Phase, State, black, invalidate, with};
use crate::capture::inline::{
    BarLabels, Btn, Cursor, FinishKind, InlineKey, KeyCtx, Layer, PAINT_ORDER, PANEL_GAP, Panel,
    SelHandle, SelectPress, Side, bar_layout, cursor_over, inline_key, label_rect, move_sel,
    place_side, resize_sel, select_press, separate, strip_layout, tip, tooltip_rect, ui_for,
};
use crate::capture::{Rect, drag_rect, is_click, size_label, win};
use crate::i18n::{Key, tr};
use crate::shot_edit::gfx::{Align, Gfx, bitmap, color, rect_f};
use crate::shot_edit::icons::{Glyph, fill_round, glyph, line};
use crate::shot_edit::paint::{Pix, draw_annotations_clipped, draw_selection};
use crate::shot_edit::text::{
    self, TEXT_COMMIT, TextClick, TextEdit, edit_text, text_click_action, text_msg_is_current,
};
use crate::shot_edit::{
    Annotation, COLORS, FRect, Handle, Pt, STROKES, Session, Tool, View, text_size,
};
use crate::ui::geometry::Rect as UiRect;

/// Text of a button, in dip.
const UI_TEXT: f32 = 13.0;

/// What a running drag moves.
pub(super) enum Drag {
    /// A new annotation being drawn; `from` is where the press was (frame px).
    Draw {
        idx: usize,
        from: Pt,
    },
    MoveAnn {
        idx: usize,
        last: Pt,
    },
    ResizeAnn {
        idx: usize,
        handle: Handle,
    },
    /// A new selection from `from` (virtual px), taken once it is a drag.
    NewSel {
        from: (i32, i32),
    },
    /// The whole selection, as it was at the press.
    MoveSel {
        start: Rect,
        from: (i32, i32),
    },
    ResizeSel {
        start: Rect,
        handle: SelHandle,
    },
}

/// The drawing phase's state.
pub(super) struct Edit {
    /// The selection, virtual-screen px, always inside the frame.
    pub(super) sel: Rect,
    pub(super) session: Session,
    drag: Option<Drag>,
    text: Option<TextEdit>,
    hover: Option<Btn>,
    pressed: Option<Btn>,
    /// The "Buang perubahan?" bar replaces the action bar's buttons.
    confirm: bool,
    /// A line at the top of the monitor (the demo's "no clipboard").
    notice: Option<Key>,
    /// DPI of the monitor holding the selection's centre, over 96.
    ui: f32,
    /// That monitor.
    monitor: Rect,
    labels: BarLabels,
    /// Placed, in virtual px.
    strip: Panel,
    bar: Panel,
    pub(super) pix: Pix,
}

impl Edit {
    fn new(sel: Rect) -> Edit {
        Edit {
            sel,
            session: Session::default(),
            drag: None,
            text: None,
            hover: None,
            pressed: None,
            confirm: false,
            notice: None,
            ui: 1.0,
            monitor: sel,
            labels: BarLabels::default(),
            strip: Panel {
                rect: Rect::default(),
                items: Vec::new(),
                seps: Vec::new(),
                text: None,
            },
            bar: Panel {
                rect: Rect::default(),
                items: Vec::new(),
                seps: Vec::new(),
                text: None,
            },
            pix: Pix::default(),
        }
    }

    fn button_at(&self, p: (i32, i32)) -> Option<Btn> {
        self.strip.hit(p).or_else(|| self.bar.hit(p))
    }

    fn over_panel(&self, p: (i32, i32)) -> bool {
        self.strip.rect.contains(p) || self.bar.rect.contains(p)
    }

    /// While the confirm bar asks, only its two buttons answer.
    fn live(&self, btn: Btn) -> bool {
        !self.confirm || matches!(btn, Btn::Keep | Btn::Discard)
    }

    /// Where a press is taken as a grab of a handle, px.
    fn handle_tol(&self) -> i32 {
        (7.0 * self.ui).round() as i32
    }

    fn ann_tol(&self) -> f32 {
        6.0 * self.ui
    }
}

impl Drop for Edit {
    /// An EDIT still open goes with the phase (its font and brush too).
    fn drop(&mut self) {
        if let Some(t) = self.text.take() {
            text::destroy(t);
        }
    }
}

/// What a handler needs from the rest of the state.
struct Env<'a> {
    gfx: &'a Gfx,
    /// The frame in virtual px.
    frame: Rect,
    demo: bool,
}

impl Env<'_> {
    /// A point of the frame (px from its top-left), kept on it.
    fn local(&self, p: (i32, i32)) -> Pt {
        Pt::new(
            (p.0 - self.frame.x).clamp(0, self.frame.w) as f32,
            (p.1 - self.frame.y).clamp(0, self.frame.h) as f32,
        )
    }
}

/// The drawing phase's state with the rest of what it needs, while the
/// overlay is in that phase.
fn ed<R>(f: impl FnOnce(&mut Edit, &Env) -> R) -> Option<R> {
    with(|s| {
        let State {
            phase,
            gfx,
            frame,
            demo,
            ..
        } = s;
        let Phase::Edit(e) = phase else { return None };
        let env = Env {
            gfx,
            frame: frame.rect(),
            demo: *demo,
        };
        Some(f(e, &env))
    })
    .flatten()
}

fn measure_labels(gfx: &Gfx, ui: f32) -> BarLabels {
    let w = |k: Key| gfx.measure(tr(k), UI_TEXT * ui, 500).0;
    BarLabels {
        copy: w(Key::EditCopy),
        save: w(Key::EditSave),
        editor: w(Key::InlineOpenEditor),
        close: w(Key::EditClose),
        ask: gfx.measure(tr(Key::EditDiscardAsk), UI_TEXT * ui, 600).0,
        discard: w(Key::EditDiscard),
        keep: w(Key::EditKeep),
    }
}

/// Places the strip and the bar around the selection (and takes the DPI of
/// its monitor, so a selection moved to another one is sized for it).
fn relayout(e: &mut Edit, gfx: &Gfx) {
    let (monitor, dpi) = win::monitor_at(e.sel.center());
    let ui = ui_for(dpi);
    if ui != e.ui || e.labels == BarLabels::default() {
        e.ui = ui;
        e.labels = measure_labels(gfx, ui);
    }
    e.monitor = monitor;
    let gap = (PANEL_GAP * ui).round() as i32;
    let margin = (16.0 * ui).round() as i32;
    let strip = strip_layout(ui, monitor.h - 2 * margin);
    let strip_at = place_side(
        e.sel,
        (strip.rect.w, strip.rect.h),
        monitor,
        Side::Right,
        gap,
    );
    let bar = bar_layout(ui, &e.labels, e.confirm);
    let bar_at = place_side(e.sel, (bar.rect.w, bar.rect.h), monitor, Side::Below, gap);
    let bar_at = separate(bar_at, strip_at, monitor, gap);
    e.strip = strip.moved_to(strip_at.x, strip_at.y);
    e.bar = bar.moved_to(bar_at.x, bar_at.y);
}

/// The pick is made: the overlay goes on in its drawing phase with `r`.
pub(super) fn enter(hwnd: HWND, r: Rect) {
    with(|s| {
        s.down = None;
        s.drag = None;
        s.hover = None;
        let mut e = Edit::new(r);
        relayout(&mut e, &s.gfx);
        s.phase = Phase::Edit(Box::new(e));
    });
    invalidate(hwnd);
}

/// The document moved onto the crop of `sel` (virtual px) cut from a frame
/// at `origin`: annotations partly outside it stay as they are.
pub(super) fn doc_in_crop(doc: &[Annotation], sel: Rect, origin: (i32, i32)) -> Vec<Annotation> {
    let (dx, dy) = ((sel.x - origin.0) as f32, (sel.y - origin.1) as f32);
    doc.iter()
        .cloned()
        .map(|mut a| {
            a.move_by(-dx, -dy);
            a
        })
        .collect()
}

// ---- text ----

/// Takes the typed text into the picture (blank is dropped) and removes the
/// EDIT. True when an EDIT was open.
fn commit_text(hwnd: HWND) -> bool {
    finish_text(hwnd, true)
}

fn cancel_text(hwnd: HWND) {
    finish_text(hwnd, false);
}

/// `commit_text` for whoever has no window handle at hand.
pub(super) fn commit_text_now() {
    if let Some(hwnd) = with(|s| s.hwnd) {
        commit_text(hwnd);
    }
}

fn finish_text(hwnd: HWND, keep: bool) -> bool {
    let Some(Some(t)) = ed(|e, _| e.text.take()) else {
        return false;
    };
    let typed = edit_text(t.edit);
    let (index, pos) = (t.index, t.pos);
    text::destroy(t);
    ed(|e, _| {
        if keep {
            match index {
                Some(i) => {
                    e.session.set_text(i, &typed);
                }
                None => {
                    // In the colour and size it was typed in; blank is dropped.
                    e.session.add_text(pos, &typed);
                }
            }
        }
    });
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = SetFocus(Some(hwnd));
        }
    }
    invalidate(hwnd);
    true
}

/// Puts a native EDIT at `pos` (frame px): new text, or `index`'s text.
fn begin_text(hwnd: HWND, pos: Pt, index: Option<usize>) {
    commit_text(hwnd);
    struct Plan {
        pos: Pt,
        initial: String,
        size: f32,
        color: u32,
        ui: f32,
    }
    let plan = ed(|e, _| {
        let (initial, size, color, pos) = match index.and_then(|i| e.session.doc.get(i)) {
            Some(Annotation::Text {
                text,
                size,
                color,
                pos,
            }) => (text.clone(), *size, *color, *pos),
            _ => (
                String::new(),
                text_size(e.session.stroke),
                e.session.color,
                pos,
            ),
        };
        Plan {
            pos,
            initial,
            size,
            color,
            ui: e.ui,
        }
    });
    let Some(plan) = plan else { return };
    // The view is the identity: a text px is a screen px.
    let px = plan.size.round().max(8.0 * plan.ui);
    let Some(mut t) = text::create(
        hwnd,
        (plan.pos.x, plan.pos.y),
        &plan.initial,
        px,
        plan.color,
        plan.ui,
    ) else {
        return;
    };
    t.index = index;
    t.pos = plan.pos;
    t.size = plan.size;
    let edit = t.edit;
    let mut slot = Some(t);
    ed(|e, _| e.text = slot.take());
    if let Some(t) = slot {
        // The phase is gone meanwhile: take the EDIT back down.
        text::destroy(t);
        return;
    }
    fit_edit(hwnd);
    unsafe {
        let _ = SetFocus(Some(edit));
    }
    invalidate(hwnd);
}

/// Grows the EDIT with its text.
pub(super) fn fit_edit(hwnd: HWND) {
    let plan = ed(|e, _| e.text.as_ref().map(|t| (t.edit, t.size, e.ui))).flatten();
    let Some((edit, px, ui)) = plan else { return };
    text::fit(hwnd, edit, px, ui, |s| {
        ed(|_, env| env.gfx.measure(s, px, 400)).unwrap_or((0.0, 0.0))
    });
}

/// `WM_CTLCOLOREDIT`: the open text's colours (None: not ours).
pub(super) fn ctl_color(wparam: WPARAM) -> Option<LRESULT> {
    ed(|e, _| e.text.as_ref().map(|t| text::ctl_color(t, wparam))).flatten()
}

/// `WM_APP_TEXT` from the EDIT: commit (1) or cancel (2), unless it came from
/// an EDIT that is already gone.
pub(super) fn on_text_msg(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let current = ed(|e, _| e.text.as_ref().map(|t| t.edit.0 as isize)).flatten();
    if !text_msg_is_current(current, lparam.0) {
        return;
    }
    if wparam.0 == TEXT_COMMIT {
        commit_text(hwnd);
    } else {
        cancel_text(hwnd);
    }
}

// ---- mouse ----

/// A mouse message's position in client px.
fn client_pt(lparam: LPARAM) -> (i32, i32) {
    (
        i32::from(lparam.0 as i16),
        i32::from((lparam.0 >> 16) as i16),
    )
}

enum Down {
    /// A button took the press; it acts on release over the same one.
    Pressed,
    /// Something is being dragged.
    Dragging,
    /// A text goes here (frame px), or this annotation's text is edited.
    Text(Pt, Option<usize>),
    /// Nothing.
    Idle,
}

pub(super) fn on_down(hwnd: HWND, lparam: LPARAM, double: bool) {
    let local = client_pt(lparam);
    let edit_was_open = commit_text(hwnd);
    unsafe {
        let _ = SetCapture(hwnd);
    }
    let down = ed(|e, env| {
        e.notice = None;
        let p = (env.frame.x + local.0, env.frame.y + local.1);
        if let Some(btn) = e.button_at(p) {
            e.pressed = Some(btn);
            return Down::Pressed;
        }
        // The panels' own background and the confirm bar: nothing under
        // them is drawn on.
        if e.confirm || e.over_panel(p) {
            return Down::Idle;
        }
        press(e, env, p, double, edit_was_open)
    })
    .unwrap_or(Down::Idle);
    match down {
        Down::Text(pos, index) => {
            unsafe {
                let _ = ReleaseCapture();
            }
            begin_text(hwnd, pos, index);
        }
        Down::Idle => {
            unsafe {
                let _ = ReleaseCapture();
            }
            invalidate(hwnd);
        }
        Down::Pressed | Down::Dragging => invalidate(hwnd),
    }
}

/// A press on the picture with the current tool.
fn press(e: &mut Edit, env: &Env, p: (i32, i32), double: bool, edit_was_open: bool) -> Down {
    let pl = env.local(p);
    let inside = e.sel.contains(p);
    match e.session.tool {
        Tool::Select => {
            let tol = e.ann_tol();
            let handle = e.session.selected.and_then(|i| {
                e.session
                    .doc
                    .get(i)
                    .and_then(|a| a.handle_at(pl, tol))
                    .map(|h| (i, h))
            });
            // Only inside the selection is anything seen, so hit.
            let hit = if inside { e.session.hit(pl, tol) } else { None };
            let press = select_press(
                e.sel,
                p,
                e.handle_tol(),
                handle.is_some(),
                hit.is_some(),
                !e.session.doc.is_empty(),
            );
            match press {
                SelectPress::AnnotationHandle => {
                    let (idx, handle) = handle.expect("said so");
                    e.session.begin();
                    e.drag = Some(Drag::ResizeAnn { idx, handle });
                    Down::Dragging
                }
                SelectPress::Annotation => {
                    let i = hit.expect("said so");
                    e.session.selected = Some(i);
                    if let Some(c) = e.session.doc[i].color() {
                        e.session.color = c;
                    }
                    if double && matches!(e.session.doc[i], Annotation::Text { .. }) {
                        return Down::Text(pl, Some(i));
                    }
                    e.session.begin();
                    e.drag = Some(Drag::MoveAnn { idx: i, last: pl });
                    Down::Dragging
                }
                SelectPress::SelHandle(handle) => {
                    e.session.selected = None;
                    e.drag = Some(Drag::ResizeSel {
                        start: e.sel,
                        handle,
                    });
                    Down::Dragging
                }
                SelectPress::MoveSel => {
                    e.session.selected = None;
                    e.drag = Some(Drag::MoveSel {
                        start: e.sel,
                        from: p,
                    });
                    Down::Dragging
                }
                SelectPress::NewSel => {
                    e.session.selected = None;
                    e.drag = Some(Drag::NewSel { from: p });
                    Down::Dragging
                }
                SelectPress::Nothing => {
                    e.session.selected = None;
                    Down::Idle
                }
            }
        }
        Tool::Text => match text_click_action(edit_was_open, Tool::Text) {
            // A text goes inside the selection (outside it is cut away).
            TextClick::Place if inside => Down::Text(pl, None),
            // The click only finished the open text.
            _ => Down::Idle,
        },
        tool => {
            // A drawing tool starts on a press inside the selection; the
            // stroke may go on outside it (it is clipped).
            if !inside {
                return Down::Idle;
            }
            let style = e.session.style();
            let ann = match tool {
                Tool::Rect => Annotation::Rect {
                    r: FRect::new(pl.x, pl.y, 0.0, 0.0),
                    style,
                },
                Tool::Line => Annotation::Line {
                    a: pl,
                    b: pl,
                    style,
                },
                Tool::Arrow => Annotation::Arrow {
                    a: pl,
                    b: pl,
                    style,
                },
                Tool::Pen => Annotation::Pen {
                    pts: vec![pl],
                    style,
                },
                Tool::Marker => Annotation::Marker {
                    pts: vec![pl],
                    style,
                },
                Tool::Pixelate | Tool::Select | Tool::Text => Annotation::Pixelate {
                    r: FRect::new(pl.x, pl.y, 0.0, 0.0),
                },
            };
            e.session.begin();
            e.session.doc.push(ann);
            e.session.selected = None;
            e.drag = Some(Drag::Draw {
                idx: e.session.doc.len() - 1,
                from: pl,
            });
            Down::Dragging
        }
    }
}

/// Ends the drag in progress as a release would: a click with a drawing tool
/// leaves nothing behind, and the step's snapshot is closed. False when there
/// was no drag.
fn finish_drag(e: &mut Edit) -> bool {
    let Some(drag) = e.drag.take() else {
        return false;
    };
    if let Drag::Draw { idx, .. } = drag
        && e.session
            .doc
            .get(idx)
            .is_some_and(Annotation::is_degenerate)
    {
        e.session.doc.remove(idx);
    }
    e.session.end();
    true
}

/// A new selection `r` (when it differs); the panels follow.
fn set_sel(e: &mut Edit, env: &Env, r: Rect) -> bool {
    if r == e.sel || r.is_empty() {
        return false;
    }
    e.sel = r;
    relayout(e, env.gfx);
    true
}

pub(super) fn on_move(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    const MK_LBUTTON: usize = 0x0001;
    let local = client_pt(lparam);
    let held = wparam.0 & MK_LBUTTON != 0;
    let changed = ed(|e, env| {
        let mut changed = false;
        if !held {
            // The release went elsewhere (capture lost): the drag is over.
            e.pressed = None;
            changed |= finish_drag(e);
        }
        let p = (env.frame.x + local.0, env.frame.y + local.1);
        // No tooltip while something is being dragged.
        let hover = if e.drag.is_some() {
            None
        } else {
            e.button_at(p)
        };
        if hover != e.hover {
            e.hover = hover;
            changed = true;
        }
        let Some(drag) = e.drag.as_ref() else {
            return changed;
        };
        let pl = env.local(p);
        let bounds = env.frame;
        match drag {
            Drag::Draw { idx, from } => {
                let (idx, from) = (*idx, *from);
                if let Some(a) = e.session.doc.get_mut(idx) {
                    match a {
                        Annotation::Rect { r, .. } | Annotation::Pixelate { r } => {
                            *r = FRect::between(from, pl);
                        }
                        Annotation::Arrow { b, .. } | Annotation::Line { b, .. } => *b = pl,
                        Annotation::Pen { pts, .. } | Annotation::Marker { pts, .. } => {
                            let far = pts
                                .last()
                                .is_none_or(|l| (l.x - pl.x).hypot(l.y - pl.y) >= 1.5);
                            if far {
                                pts.push(pl);
                            }
                        }
                        Annotation::Text { .. } => {}
                    }
                }
                true
            }
            Drag::MoveAnn { idx, last } => {
                let (idx, last) = (*idx, *last);
                if let Some(a) = e.session.doc.get_mut(idx) {
                    a.move_by(pl.x - last.x, pl.y - last.y);
                }
                e.drag = Some(Drag::MoveAnn { idx, last: pl });
                true
            }
            Drag::ResizeAnn { idx, handle } => {
                let (idx, handle) = (*idx, *handle);
                if let Some(a) = e.session.doc.get_mut(idx) {
                    a.resize(handle, pl);
                }
                true
            }
            Drag::NewSel { from } => {
                let from = *from;
                // A press that has not moved yet is still a click.
                if is_click(from, p) {
                    return changed;
                }
                let r = drag_rect(from, p, bounds);
                set_sel(e, env, r) || changed
            }
            Drag::MoveSel { start, from } => {
                let r = move_sel(*start, p.0 - from.0, p.1 - from.1, bounds);
                set_sel(e, env, r) || changed
            }
            Drag::ResizeSel { start, handle } => {
                let r = resize_sel(*start, *handle, p, bounds);
                set_sel(e, env, r) || changed
            }
        }
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

pub(super) fn on_up(hwnd: HWND, lparam: LPARAM) {
    let local = client_pt(lparam);
    let action = ed(|e, env| {
        let pressed = e.pressed.take();
        if finish_drag(e) {
            return None;
        }
        let p = (env.frame.x + local.0, env.frame.y + local.1);
        let hit = e.button_at(p)?;
        // The same button as the press; with the bar up, only its own.
        (pressed == Some(hit) && e.live(hit)).then_some(hit)
    })
    .flatten();
    // Released after the state above is settled: WM_CAPTURECHANGED finds no
    // drag and no press left to cancel.
    unsafe {
        let _ = ReleaseCapture();
    }
    if let Some(btn) = action {
        do_btn(hwnd, btn);
        return;
    }
    invalidate(hwnd);
}

/// Capture lost mid-drag (Alt+Tab, Win, another window): the release will not
/// come here, so end the drag and the press as a release would.
pub(super) fn on_capture_lost(hwnd: HWND) {
    let changed = ed(|e, _| {
        let pressed = e.pressed.take().is_some();
        finish_drag(e) || pressed
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

// ---- buttons and keys ----

fn do_btn(hwnd: HWND, btn: Btn) {
    commit_text(hwnd);
    match btn {
        Btn::Tool(t) => {
            ed(|e, _| {
                e.session.tool = t;
                if t != Tool::Select {
                    e.session.selected = None;
                }
            });
        }
        Btn::Color(i) => {
            ed(|e, _| e.session.pick_color(COLORS[i.min(COLORS.len() - 1)]));
        }
        Btn::Stroke(i) => {
            ed(|e, _| e.session.pick_stroke(i.min(STROKES.len() - 1)));
        }
        Btn::Undo => {
            ed(|e, _| e.session.undo());
        }
        Btn::Redo => {
            ed(|e, _| e.session.redo());
        }
        Btn::Copy => copy(hwnd),
        Btn::Save => super::finish_inline(FinishKind::Save),
        Btn::OpenEditor => super::finish_inline(FinishKind::OpenEditor),
        Btn::Close => esc(hwnd, FinishKind::Close),
        Btn::Discard => super::finish_inline(FinishKind::Discard),
        Btn::Keep => keep(hwnd),
    }
    invalidate(hwnd);
    unsafe {
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = SetFocus(Some(hwnd));
        }
    }
}

/// Copy and close. A demo touches no clipboard: its line says so and the
/// overlay stays.
fn copy(hwnd: HWND) {
    let demo = ed(|e, env| {
        if env.demo {
            e.notice = Some(Key::EditDemoNoClipboard);
        }
        env.demo
    })
    .unwrap_or(false);
    if demo {
        invalidate(hwnd);
    } else {
        super::finish_inline(FinishKind::Copy);
    }
}

/// The confirm bar goes away: back to drawing.
fn keep(hwnd: HWND) {
    ed(|e, env| {
        e.confirm = false;
        e.hover = None;
        relayout(e, env.gfx);
    });
    invalidate(hwnd);
}

/// Esc with something drawn: the confirm bar asks first.
fn ask(hwnd: HWND) {
    ed(|e, env| {
        e.confirm = true;
        e.hover = None;
        relayout(e, env.gfx);
    });
    // The window, not an EDIT, holds the keys the bar answers.
    unsafe {
        let _ = SetFocus(Some(hwnd));
    }
    invalidate(hwnd);
}

/// The way out: Esc, a right click, "Tutup". First the text being typed,
/// then the confirm bar (back to drawing), then the bar when something is
/// drawn, else the overlay closes with nothing copied.
pub(super) fn esc(hwnd: HWND, closing: FinishKind) {
    enum Step {
        CancelText,
        Keep,
        Ask,
        Cancel,
    }
    let step = ed(|e, _| {
        if e.text.is_some() {
            Step::CancelText
        } else if e.confirm {
            Step::Keep
        } else if !e.session.doc.is_empty() {
            Step::Ask
        } else {
            Step::Cancel
        }
    });
    match step {
        Some(Step::CancelText) => cancel_text(hwnd),
        Some(Step::Keep) => keep(hwnd),
        Some(Step::Ask) => ask(hwnd),
        Some(Step::Cancel) => super::finish_inline(closing),
        None => {}
    }
}

/// `WM_CLOSE` / Alt+F4 in the drawing phase: as Esc, after taking a text
/// being typed into the picture (so the bar knows something is drawn).
pub(super) fn close(hwnd: HWND) {
    commit_text(hwnd);
    esc(hwnd, FinishKind::Cancel);
}

fn key_down(vk: u16) -> bool {
    unsafe { GetKeyState(i32::from(vk)) < 0 }
}

pub(super) fn on_key(hwnd: HWND, vk: u16) {
    let (ctrl, shift, alt) = (
        key_down(VK_CONTROL.0),
        key_down(VK_SHIFT.0),
        key_down(VK_MENU.0),
    );
    let ctx = ed(|e, _| {
        let had_notice = e.notice.take().is_some();
        (
            KeyCtx {
                editing_text: e.text.is_some(),
                confirm: e.confirm,
                has_annotations: !e.session.doc.is_empty(),
                dragging: e.drag.is_some(),
            },
            had_notice,
        )
    });
    let Some((ctx, had_notice)) = ctx else { return };
    match inline_key(vk, ctrl, shift, alt, ctx) {
        InlineKey::Ignore => {
            // The demo's line goes with the next key.
            if had_notice {
                invalidate(hwnd);
            }
        }
        InlineKey::Copy => do_btn(hwnd, Btn::Copy),
        InlineKey::Save => do_btn(hwnd, Btn::Save),
        InlineKey::AskDiscard => ask(hwnd),
        InlineKey::Cancel => super::finish_inline(FinishKind::Cancel),
        InlineKey::Discard => do_btn(hwnd, Btn::Discard),
        InlineKey::Keep => keep(hwnd),
        InlineKey::Undo => do_btn(hwnd, Btn::Undo),
        InlineKey::Redo => do_btn(hwnd, Btn::Redo),
        InlineKey::Delete => {
            ed(|e, _| e.session.delete_selected());
            invalidate(hwnd);
        }
        InlineKey::Tool(t) => do_btn(hwnd, Btn::Tool(t)),
    }
}

// ---- the pointer shape ----

pub(super) fn set_cursor() {
    let shape = ed(|e, _| {
        let mut at = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut at);
        }
        let p = (at.x, at.y);
        if let Some(drag) = &e.drag {
            return match drag {
                Drag::Draw { .. } => Cursor::Cross,
                Drag::MoveAnn { .. } | Drag::MoveSel { .. } => Cursor::SizeAll,
                Drag::ResizeAnn { .. } | Drag::NewSel { .. } => Cursor::Cross,
                Drag::ResizeSel { handle, .. } => handle.cursor(),
            };
        }
        if e.confirm || e.over_panel(p) {
            return Cursor::Arrow;
        }
        cursor_over(e.sel, p, e.handle_tol(), e.session.tool)
    })
    .unwrap_or(Cursor::Arrow);
    let id: PCWSTR = match shape {
        Cursor::Arrow => IDC_ARROW,
        Cursor::Cross => IDC_CROSS,
        Cursor::IBeam => IDC_IBEAM,
        Cursor::SizeAll => IDC_SIZEALL,
        Cursor::SizeNwse => IDC_SIZENWSE,
        Cursor::SizeNesw => IDC_SIZENESW,
        Cursor::SizeNs => IDC_SIZENS,
        Cursor::SizeWe => IDC_SIZEWE,
    };
    unsafe {
        if let Ok(c) = LoadCursorW(None, id) {
            SetCursor(Some(c));
        }
    }
}

// ---- drawing ----

fn ui_rect(r: Rect, origin: (i32, i32)) -> UiRect {
    UiRect {
        x: (r.x - origin.0) as f32,
        y: (r.y - origin.1) as f32,
        w: r.w as f32,
        h: r.h as f32,
    }
}

/// A panel's dark rounded background, as the pills have.
fn panel_back(rt: &ID2D1RenderTarget, brush: &ID2D1SolidColorBrush, r: UiRect, ui: f32) {
    use windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT;
    let rounded = D2D1_ROUNDED_RECT {
        rect: rect_f(r.x, r.y, r.w, r.h),
        radiusX: 10.0 * ui,
        radiusY: 10.0 * ui,
    };
    unsafe {
        brush.SetColor(&black(0.82));
        rt.FillRoundedRectangle(&rounded, brush);
        brush.SetColor(&color(0xFFFFFF, 0.16));
        rt.DrawRoundedRectangle(&rounded, brush, 1.0 * ui, None);
    }
}

/// What a frame is drawn with.
struct Paint<'a> {
    rt: &'a ID2D1RenderTarget,
    brush: &'a ID2D1SolidColorBrush,
    gfx: &'a Gfx,
    accent: u32,
    ui: f32,
    origin: (i32, i32),
}

impl Paint<'_> {
    fn rect(&self, r: Rect) -> UiRect {
        ui_rect(r, self.origin)
    }

    /// A button's background when it is hot (accent, faint) or on (accent).
    fn lit(&self, r: UiRect, on: bool, hot: bool) {
        if on {
            fill_round(self.rt, self.brush, r, 8.0 * self.ui, self.accent, 1.0);
        } else if hot {
            fill_round(self.rt, self.brush, r, 8.0 * self.ui, self.accent, 0.22);
        }
    }

    fn strip_button(&self, btn: Btn, r: UiRect, e: &Edit) {
        let ui = self.ui;
        let hot = e.live(btn) && e.hover == Some(btn);
        let a = if e.live(btn) { 1.0 } else { 0.35 };
        let on_fill = text::contrast_back(self.accent);
        match btn {
            Btn::Tool(tool) => {
                let on = e.session.tool == tool;
                self.lit(r, on, hot);
                let rgb = if on { on_fill } else { 0xF2F2F2 };
                glyph(
                    self.rt,
                    self.brush,
                    self.gfx,
                    Glyph::Tool(tool),
                    r,
                    rgb,
                    a,
                    ui,
                );
            }
            Btn::Color(i) => {
                let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                let on = e.session.color == COLORS[i];
                if hot && !on {
                    self.lit(r, false, true);
                }
                let radius = 9.0 * ui;
                let ell = |rad: f32| D2D1_ELLIPSE {
                    point: Vector2::new(cx, cy),
                    radiusX: rad,
                    radiusY: rad,
                };
                unsafe {
                    self.brush.SetColor(&color(COLORS[i], a));
                    self.rt.FillEllipse(&ell(radius), self.brush);
                    self.brush.SetColor(&color(0xFFFFFF, 0.3 * a));
                    self.rt.DrawEllipse(&ell(radius), self.brush, 1.0, None);
                    if on {
                        self.brush.SetColor(&color(0xFFFFFF, a));
                        self.rt
                            .DrawEllipse(&ell(radius + 3.5 * ui), self.brush, 2.0 * ui, None);
                    }
                }
            }
            Btn::Stroke(i) => {
                let on = e.session.stroke == i;
                self.lit(r, on, hot);
                let wpx = [2.0, 4.0, 7.0][i.min(2)] * ui;
                unsafe {
                    self.brush
                        .SetColor(&color(if on { on_fill } else { 0xF2F2F2 }, a));
                }
                line(
                    self.rt,
                    self.brush,
                    self.gfx,
                    (r.x + r.w * 0.28, r.y + r.h / 2.0),
                    (r.x + r.w * 0.72, r.y + r.h / 2.0),
                    wpx,
                );
            }
            Btn::Undo | Btn::Redo => {
                let can = if btn == Btn::Undo {
                    e.session.can_undo()
                } else {
                    e.session.can_redo()
                };
                if hot && can {
                    self.lit(r, false, true);
                }
                let g = if btn == Btn::Undo {
                    Glyph::Undo
                } else {
                    Glyph::Redo
                };
                let alpha = if can { a } else { 0.3 * a };
                glyph(self.rt, self.brush, self.gfx, g, r, 0xF2F2F2, alpha, ui);
            }
            _ => {}
        }
    }

    fn bar_button(&self, btn: Btn, r: UiRect, e: &Edit, demo: bool) {
        let ui = self.ui;
        let hot = e.live(btn) && e.hover == Some(btn);
        let label = match btn {
            Btn::Copy => tr(Key::EditCopy),
            Btn::Save => tr(Key::EditSave),
            Btn::OpenEditor => tr(Key::InlineOpenEditor),
            Btn::Close => tr(Key::EditClose),
            Btn::Discard => tr(Key::EditDiscard),
            Btn::Keep => tr(Key::EditKeep),
            _ => return,
        };
        // Copy is the main one (Enter); Buang is the danger.
        let (fill, fg, base) = match btn {
            Btn::Copy => (self.accent, text::contrast_back(self.accent), 1.0),
            Btn::Discard => (0xEF7A6E, 0x1A1210, 1.0),
            _ => (0xFFFFFF, 0xF2F2F2, 0.10),
        };
        let mut alpha = 1.0;
        // Copy is inert in the demo.
        if demo && btn == Btn::Copy {
            alpha = 0.5;
        }
        fill_round(self.rt, self.brush, r, 8.0 * ui, fill, base * alpha);
        if hot {
            fill_round(self.rt, self.brush, r, 8.0 * ui, self.accent, 0.22);
        }
        self.gfx.text_in(
            self.rt,
            self.brush,
            label,
            (r.x, r.y, r.w, r.h),
            UI_TEXT * ui,
            500,
            fg,
            alpha,
            Align::Center,
        );
    }

    /// A pill (the label, a tooltip, the notice) at `r`.
    fn pill(&self, r: UiRect, text: &str, size: f32) {
        super::pill(
            self.rt,
            self.brush,
            self.gfx,
            (r.x, r.y, r.w, r.h),
            text,
            size,
            0,
            self.ui,
        );
    }
}

pub(super) fn draw(s: &mut State) -> windows::core::Result<()> {
    let (w, h) = (s.frame.w(), s.frame.h());
    if s.target.is_none() {
        s.target = Some(s.gfx.hwnd_target(s.hwnd, (w, h))?);
        s.picture = None;
    }
    let State {
        target,
        picture,
        gfx,
        frame,
        phase,
        accent,
        demo,
        ..
    } = s;
    let Phase::Edit(e) = phase else {
        return Ok(());
    };
    let e = &mut **e;
    let t = target.as_ref().expect("made above");
    let rt: &ID2D1RenderTarget = &t.rt;
    let brush = &t.brush;
    if picture.is_none() {
        *picture = Some(bitmap(rt, w, h, frame.bgra())?);
    }
    let picture = picture.as_ref().expect("made above");
    let origin = frame.origin;
    let ui = e.ui;
    let p = Paint {
        rt,
        brush,
        gfx,
        accent: *accent,
        ui,
        origin,
    };
    let (full_w, full_h) = (w as f32, h as f32);
    let sel = p.rect(e.sel);
    // The part the picture keeps, in whole frame px.
    let clip = (sel.x as u32, sel.y as u32, sel.w as u32, sel.h as u32);
    let view = View {
        scale: 1.0,
        ox: 0.0,
        oy: 0.0,
    };
    unsafe {
        rt.BeginDraw();
    }
    // Direct2D paints in call order: PAINT_ORDER is the order, and a panel's
    // background always comes before the buttons that live on it.
    for layer in PAINT_ORDER {
        match layer {
            Layer::Frame => unsafe {
                rt.DrawBitmap(
                    picture,
                    Some(&rect_f(0.0, 0.0, full_w, full_h)),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                    None,
                );
            },
            Layer::Dim => unsafe {
                brush.SetColor(&black(super::DIM));
                let dim = |x: f32, y: f32, w: f32, h: f32| {
                    if w > 0.0 && h > 0.0 {
                        rt.FillRectangle(&rect_f(x, y, w, h), brush);
                    }
                };
                dim(0.0, 0.0, full_w, sel.y);
                dim(0.0, sel.bottom(), full_w, full_h - sel.bottom());
                dim(0.0, sel.y, sel.x, sel.h);
                dim(sel.right(), sel.y, full_w - sel.right(), sel.h);
            },
            Layer::Annotations => {
                unsafe {
                    rt.PushAxisAlignedClip(
                        &rect_f(sel.x, sel.y, sel.w, sel.h),
                        D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    );
                }
                let editing = e.text.as_ref().and_then(|t| t.index);
                draw_annotations_clipped(
                    rt,
                    brush,
                    gfx,
                    &frame.shot,
                    &e.session.doc,
                    &view,
                    editing,
                    &mut e.pix,
                    Some(clip),
                );
                if editing.is_none()
                    && let Some(a) = e.session.selected.and_then(|i| e.session.doc.get(i))
                {
                    draw_selection(rt, brush, a, &view, *accent, ui);
                }
                unsafe {
                    rt.PopAxisAlignedClip();
                }
            }
            Layer::Border => unsafe {
                // Just outside the selection: black, with white dashes on it.
                let b = rect_f(sel.x - 0.5, sel.y - 0.5, sel.w + 1.0, sel.h + 1.0);
                brush.SetColor(&color(0, 1.0));
                rt.DrawRectangle(&b, brush, 1.0, None);
                brush.SetColor(&color(0xFFFFFF, 1.0));
                rt.DrawRectangle(&b, brush, 1.0, &gfx.dash);
            },
            Layer::Label => {
                let label = size_label(e.sel);
                let lw = gfx.measure(&label, 12.0 * ui, 500).0 + 16.0 * ui;
                let lh = 22.0 * ui;
                let at = label_rect(
                    e.sel,
                    (lw.ceil() as i32, lh.ceil() as i32),
                    e.monitor,
                    (6.0 * ui).round() as i32,
                );
                p.pill(p.rect(at), &label, 12.0 * ui);
            }
            Layer::Handles => {
                let hs = 3.5 * ui;
                for handle in SelHandle::ALL {
                    let (hx, hy) = handle.point(e.sel);
                    let (x, y) = ((hx - origin.0) as f32, (hy - origin.1) as f32);
                    let r = rect_f(x - hs, y - hs, hs * 2.0, hs * 2.0);
                    unsafe {
                        brush.SetColor(&color(0xFFFFFF, 1.0));
                        rt.FillRectangle(&r, brush);
                        brush.SetColor(&color(0, 0.85));
                        rt.DrawRectangle(&r, brush, 1.0, None);
                    }
                }
            }
            Layer::StripBack => {
                panel_back(rt, brush, p.rect(e.strip.rect), ui);
                for sep in &e.strip.seps {
                    let r = p.rect(*sep);
                    unsafe {
                        brush.SetColor(&color(0xFFFFFF, 0.16));
                        rt.FillRectangle(&rect_f(r.x, r.y, r.w, r.h.max(1.0)), brush);
                    }
                }
            }
            Layer::BarBack => panel_back(rt, brush, p.rect(e.bar.rect), ui),
            Layer::StripButtons => {
                for (btn, r) in &e.strip.items {
                    p.strip_button(*btn, p.rect(*r), e);
                }
            }
            Layer::BarButtons => {
                for (btn, r) in &e.bar.items {
                    p.bar_button(*btn, p.rect(*r), e, *demo);
                }
            }
            Layer::BarText => {
                if let Some(q) = e.bar.text {
                    let r = p.rect(q);
                    gfx.text_in(
                        rt,
                        brush,
                        tr(Key::EditDiscardAsk),
                        (r.x, r.y, r.w, r.h),
                        UI_TEXT * ui,
                        600,
                        0xF2F2F2,
                        1.0,
                        Align::Center,
                    );
                }
            }
            Layer::Tooltip => {
                let hovered = e.hover.filter(|b| e.live(*b));
                if let Some(btn) = hovered
                    && let Some(text) = tip(btn)
                {
                    let vertical = e.strip.items.iter().any(|(b, _)| *b == btn);
                    let panel = if vertical { &e.strip } else { &e.bar };
                    if let Some(r) = panel.rect_of(btn) {
                        let tw = gfx.measure(&text, 12.0 * ui, 500).0 + 18.0 * ui;
                        let th = 24.0 * ui;
                        let at = tooltip_rect(
                            r,
                            (tw.ceil() as i32, th.ceil() as i32),
                            e.monitor,
                            vertical,
                            (6.0 * ui).round() as i32,
                        );
                        p.pill(p.rect(at), &text, 12.0 * ui);
                    }
                }
            }
            Layer::Notice => {
                if let Some(key) = e.notice {
                    let text = tr(key);
                    let size = 13.0 * ui;
                    let nw = gfx.measure(text, size, 500).0 + 36.0 * ui;
                    let nh = 32.0 * ui;
                    let m = p.rect(e.monitor);
                    p.pill(
                        UiRect {
                            x: m.x + (m.w - nw) / 2.0,
                            y: m.y + 24.0 * ui,
                            w: nw,
                            h: nh,
                        },
                        text,
                        size,
                    );
                }
            }
        }
    }
    unsafe { rt.EndDraw(None, None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::inline::{STRIP_TOOLS, handle_at};

    #[test]
    fn the_document_moves_onto_the_crop_by_the_selections_corner() {
        use crate::shot_edit::Style;
        let style = Style {
            color: COLORS[0],
            width: 4.0,
        };
        // A frame whose origin is (-1920, 0); the selection at (-1500, 200).
        let sel = Rect::new(-1500, 200, 400, 300);
        let origin = (-1920, 0);
        let doc = vec![
            Annotation::Rect {
                r: FRect::new(500.0, 250.0, 100.0, 60.0),
                style,
            },
            // Partly outside the selection: kept, just moved.
            Annotation::Line {
                a: Pt::new(400.0, 100.0),
                b: Pt::new(1000.0, 600.0),
                style,
            },
            Annotation::Pen {
                pts: vec![Pt::new(430.0, 210.0), Pt::new(450.0, 230.0)],
                style,
            },
        ];
        let moved = doc_in_crop(&doc, sel, origin);
        // The corner of the selection is the crop's (0, 0): 420, 200.
        assert!(matches!(&moved[0], Annotation::Rect { r, .. }
            if *r == FRect::new(80.0, 50.0, 100.0, 60.0)));
        assert!(matches!(&moved[1], Annotation::Line { a, b, .. }
            if *a == Pt::new(-20.0, -100.0) && *b == Pt::new(580.0, 400.0)));
        assert!(matches!(&moved[2], Annotation::Pen { pts, .. }
            if pts[0] == Pt::new(10.0, 10.0) && pts[1] == Pt::new(30.0, 30.0)));
        // The original is untouched (the overlay may go on).
        assert!(matches!(&doc[0], Annotation::Rect { r, .. } if r.x == 500.0));
        // A selection at the frame's corner moves nothing.
        let same = doc_in_crop(&doc, Rect::new(-1920, 0, 100, 100), origin);
        assert_eq!(same, doc);
    }

    #[test]
    fn the_strip_offers_the_editors_tools() {
        assert_eq!(STRIP_TOOLS.len(), Tool::ALL.len());
        assert!(STRIP_TOOLS.contains(&Tool::Line));
    }

    #[test]
    fn a_pointer_message_reads_signed_client_coordinates() {
        // Negative coordinates (a capture that went off the left edge).
        let lparam = LPARAM((((-5i16 as u16 as isize) << 16) | (-12i16 as u16 as isize)) as isize);
        assert_eq!(client_pt(lparam), (-12, -5));
        assert_eq!(client_pt(LPARAM((7isize << 16) | 9)), (9, 7));
    }

    #[test]
    fn no_edit_phase_exists_in_a_test() {
        assert!(ed(|_, _| ()).is_none());
        assert!(!super::super::in_edit());
        // A handle cannot be found on a selection that is not there.
        assert!(handle_at(Rect::new(0, 0, 10, 10), (100, 100), 6).is_none());
    }
}
