//! The pure part of the inline capture ("Setelah capture" = "Langsung di
//! layar"): after the region is picked the overlay stays up and the person
//! draws on the frozen screen itself, Lightshot style.
//!
//! Nothing here touches Win32 or Direct2D: where the tool strip and the
//! action bar go around the selection, the eight selection handles and how
//! dragging one resizes it, which key does what, which finishes put the
//! picture on the clipboard, and the order a frame is painted in. `overlay`
//! is the window that uses it. Everything is in physical pixels of the
//! virtual screen unless a name says otherwise; a size in dip is scaled by
//! `ui` (the DPI of the monitor holding the selection, over 96).

use super::Rect;
use crate::i18n::{Key, tr};
use crate::shot_edit::Tool;

// ---- the buttons ----

/// What a button of the strip or the bar does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Btn {
    Tool(Tool),
    Color(usize),
    Stroke(usize),
    Undo,
    Redo,
    Copy,
    Save,
    OpenEditor,
    /// "Tutup": the same way out as Esc.
    Close,
    /// The confirm bar's "Buang" and "Batal".
    Discard,
    Keep,
}

/// The tools of the strip, top to bottom.
pub const STRIP_TOOLS: [Tool; 8] = [
    Tool::Select,
    Tool::Pen,
    Tool::Line,
    Tool::Arrow,
    Tool::Rect,
    Tool::Marker,
    Tool::Text,
    Tool::Pixelate,
];

/// What the tooltip beside a hovered button says: its name and shortcut.
/// The confirm bar's own buttons say nothing.
pub fn tip(btn: Btn) -> Option<String> {
    Some(match btn {
        Btn::Tool(t) => {
            let name = match t {
                Tool::Select => Key::EditSelect,
                Tool::Text => Key::EditText,
                Tool::Rect => Key::EditRect,
                Tool::Line => Key::EditLine,
                Tool::Arrow => Key::EditArrow,
                Tool::Pen => Key::EditPen,
                Tool::Marker => Key::EditMarker,
                Tool::Pixelate => Key::EditPixelate,
            };
            format!("{} ({})", tr(name), t.key())
        }
        Btn::Color(_) => tr(Key::EditColor).to_owned(),
        Btn::Stroke(_) => tr(Key::EditStroke).to_owned(),
        Btn::Undo => format!("{} (Ctrl+Z)", tr(Key::EditUndo)),
        Btn::Redo => format!("{} (Ctrl+Y)", tr(Key::EditRedo)),
        Btn::Copy => format!("{} (Ctrl+C)", tr(Key::EditCopy)),
        Btn::Save => format!("{} (Ctrl+S)", tr(Key::EditSave)),
        Btn::OpenEditor => tr(Key::InlineOpenEditor).to_owned(),
        Btn::Close => format!("{} (Esc)", tr(Key::EditClose)),
        Btn::Discard | Btn::Keep => return None,
    })
}

// ---- sizes (dip, scaled by `ui`) ----

/// A strip button (a square).
const BTN: f32 = 30.0;
/// Between two buttons of a group, and between columns.
const GAP: f32 = 2.0;
/// Panel inset.
const PAD: f32 = 6.0;
/// Between the groups of the strip (the separator sits in the middle).
const GROUP_GAP: f32 = 10.0;
/// The bar's height and the text buttons in it.
const BAR_H: f32 = 40.0;
const BAR_BTN_H: f32 = 30.0;
const BAR_GAP: f32 = 4.0;
const TEXT_PAD: f32 = 12.0;
/// Between the selection and the panels beside it.
pub const PANEL_GAP: f32 = 8.0;

fn s(dip: f32, ui: f32) -> i32 {
    (dip * ui).round() as i32
}

/// DPI over 96, never below 1.
pub fn ui_for(dpi: u32) -> f32 {
    dpi.max(96) as f32 / 96.0
}

/// A panel (the tool strip or the action bar) with its buttons.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub rect: Rect,
    pub items: Vec<(Btn, Rect)>,
    /// Hairlines between the groups of the strip.
    pub seps: Vec<Rect>,
    /// The confirm bar's question.
    pub text: Option<Rect>,
}

impl Panel {
    /// The same panel with its top-left at `(x, y)`.
    pub fn moved_to(&self, x: i32, y: i32) -> Panel {
        let (dx, dy) = (x - self.rect.x, y - self.rect.y);
        let shift = |r: Rect| Rect::new(r.x + dx, r.y + dy, r.w, r.h);
        Panel {
            rect: shift(self.rect),
            items: self.items.iter().map(|(b, r)| (*b, shift(*r))).collect(),
            seps: self.seps.iter().map(|r| shift(*r)).collect(),
            text: self.text.map(shift),
        }
    }

    /// The button under `pt`.
    pub fn hit(&self, pt: (i32, i32)) -> Option<Btn> {
        self.items
            .iter()
            .find(|(_, r)| r.contains(pt))
            .map(|(b, _)| *b)
    }

    pub fn rect_of(&self, btn: Btn) -> Option<Rect> {
        self.items.iter().find(|(b, _)| *b == btn).map(|(_, r)| *r)
    }
}

/// The strip's buttons in groups: tools, colours, strokes, undo and redo.
fn strip_groups() -> Vec<Vec<Btn>> {
    vec![
        STRIP_TOOLS.iter().map(|t| Btn::Tool(*t)).collect(),
        (0..crate::shot_edit::COLORS.len())
            .map(Btn::Color)
            .collect(),
        (0..crate::shot_edit::STROKES.len())
            .map(Btn::Stroke)
            .collect(),
        vec![Btn::Undo, Btn::Redo],
    ]
}

/// The tool strip at the origin: one column of buttons (tools, colours,
/// strokes, undo and redo), or two (three, ...) side by side when one column
/// would be taller than `max_h` px. The buttons keep their order, running
/// down each column in turn.
pub fn strip_layout(ui: f32, max_h: i32) -> Panel {
    let (btn, gap, pad) = (s(BTN, ui), s(GAP, ui).max(1), s(PAD, ui));
    let group_gap = s(GROUP_GAP, ui);
    let mut items = Vec::new();
    let mut seps = Vec::new();
    let (mut col, mut y) = (0i32, pad);
    // Bottom of the lowest button of any column.
    let mut deepest = pad;
    let col_x = |c: i32| pad + c * (btn + gap);
    for (gi, group) in strip_groups().into_iter().enumerate() {
        if gi > 0 {
            // `y` already holds the gap after the previous button.
            y += group_gap - gap;
        }
        for (i, b) in group.into_iter().enumerate() {
            // A new column when this button would end below the limit
            // (never an empty column).
            if y + btn + pad > max_h && y > pad {
                col += 1;
                y = pad;
            } else if i == 0 && gi > 0 {
                // The separator sits in the middle of the group gap.
                seps.push(Rect::new(col_x(col), y - group_gap / 2 - 1, btn, 1));
            }
            items.push((b, Rect::new(col_x(col), y, btn, btn)));
            deepest = deepest.max(y + btn);
            y += btn + gap;
        }
    }
    Panel {
        rect: Rect::new(0, 0, col_x(col) + btn + pad, deepest + pad),
        items,
        seps,
        text: None,
    }
}

/// How wide the bar's labels are, in px (measured by the caller).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BarLabels {
    pub copy: f32,
    pub save: f32,
    pub editor: f32,
    pub close: f32,
    /// The confirm bar's question, "Buang" and "Batal".
    pub ask: f32,
    pub discard: f32,
    pub keep: f32,
}

/// The action bar at the origin: Salin, Simpan, Buka di editor, Tutup; with
/// `confirm` the question and its Buang and Batal take its place.
pub fn bar_layout(ui: f32, labels: &BarLabels, confirm: bool) -> Panel {
    let (h, bh) = (s(BAR_H, ui), s(BAR_BTN_H, ui));
    let (pad, gap) = (s(PAD, ui), s(BAR_GAP, ui));
    let tp = s(TEXT_PAD, ui);
    let y = (h - bh) / 2;
    let mut x = pad;
    let mut items = Vec::new();
    let mut text = None;
    let mut button = |btn: Btn, label: f32, x: &mut i32| {
        let w = label.ceil() as i32 + 2 * tp;
        items.push((btn, Rect::new(*x, y, w, bh)));
        *x += w + gap;
    };
    if confirm {
        let w = labels.ask.ceil() as i32 + 2 * tp;
        text = Some(Rect::new(x, 0, w, h));
        x += w;
        button(Btn::Discard, labels.discard, &mut x);
        button(Btn::Keep, labels.keep, &mut x);
    } else {
        button(Btn::Copy, labels.copy, &mut x);
        button(Btn::Save, labels.save, &mut x);
        button(Btn::OpenEditor, labels.editor, &mut x);
        button(Btn::Close, labels.close, &mut x);
    }
    Panel {
        rect: Rect::new(0, 0, x - gap + pad, h),
        items,
        seps: Vec::new(),
        text,
    }
}

// ---- placement ----

/// Which side of the selection a panel prefers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Beside the right edge, top aligned (then the left edge).
    Right,
    /// Under the bottom edge, right aligned (then above the top edge).
    Below,
}

/// `r` moved the least that puts all of it on `bounds` (pinned to its
/// top-left when it is bigger).
pub fn clamp_into(r: Rect, bounds: Rect) -> Rect {
    let x = r.x.min(bounds.right() - r.w).max(bounds.x);
    let y = r.y.min(bounds.bottom() - r.h).max(bounds.y);
    Rect::new(x, y, r.w, r.h)
}

/// Where a panel of `size` goes around `sel` on `monitor` (the one holding
/// the selection), `gap` px away.
///
/// Right: beside the right edge from the top of the selection; no room
/// there: beside the left edge; none either: inside the selection's right
/// edge. Below: under the bottom edge, its right edge on the selection's;
/// no room: above the top edge; none either: inside the bottom edge. The
/// result is always fully on `monitor`.
pub fn place_side(sel: Rect, size: (i32, i32), monitor: Rect, side: Side, gap: i32) -> Rect {
    let (w, h) = size;
    let at = match side {
        Side::Right => {
            let right = sel.right() + gap;
            let left = sel.x - gap - w;
            let x = if right + w <= monitor.right() {
                right
            } else if left >= monitor.x {
                left
            } else {
                sel.right() - w
            };
            (x, sel.y)
        }
        Side::Below => {
            let x = sel.right() - w;
            let below = sel.bottom() + gap;
            let above = sel.y - gap - h;
            let y = if below + h <= monitor.bottom() {
                below
            } else if above >= monitor.y {
                above
            } else {
                sel.bottom() - h
            };
            (x, y)
        }
    };
    clamp_into(Rect::new(at.0, at.1, w, h), monitor)
}

/// The bar moved clear of the strip when they would overlap (a narrow
/// selection with the strip flipped to its left, say): to the strip's far
/// side when that fits, else under or above it. Untouched otherwise.
pub fn separate(bar: Rect, strip: Rect, monitor: Rect, gap: i32) -> Rect {
    if bar.intersect(&strip).is_empty() {
        return bar;
    }
    let beside = Rect::new(strip.right() + gap, bar.y, bar.w, bar.h);
    if beside.right() <= monitor.right() {
        return beside;
    }
    let under = Rect::new(bar.x, strip.bottom() + gap, bar.w, bar.h);
    if under.bottom() <= monitor.bottom() {
        return under;
    }
    clamp_into(
        Rect::new(bar.x, strip.y - gap - bar.h, bar.w, bar.h),
        monitor,
    )
}

/// The size label (`size` px) of `sel`: above its top-left corner, `gap`
/// away; no room above on `monitor`: just inside the top-left corner.
pub fn label_rect(sel: Rect, size: (i32, i32), monitor: Rect, gap: i32) -> Rect {
    let (w, h) = size;
    let above = sel.y - gap - h;
    let y = if above >= monitor.y {
        above
    } else {
        sel.y + gap
    };
    clamp_into(Rect::new(sel.x, y, w, h), monitor)
}

/// The tooltip (`size` px) of a hovered button: beside a vertical strip's
/// button (right, else left), under a bar's (below, else above). On `monitor`.
pub fn tooltip_rect(btn: Rect, size: (i32, i32), monitor: Rect, vertical: bool, gap: i32) -> Rect {
    let (w, h) = size;
    let (x, y) = if vertical {
        let right = btn.right() + gap;
        let x = if right + w <= monitor.right() {
            right
        } else {
            btn.x - gap - w
        };
        (x, btn.y + (btn.h - h) / 2)
    } else {
        let below = btn.bottom() + gap;
        let y = if below + h <= monitor.bottom() {
            below
        } else {
            btn.y - gap - h
        };
        (btn.x + (btn.w - w) / 2, y)
    };
    clamp_into(Rect::new(x, y, w, h), monitor)
}

// ---- the selection ----

/// The eight handles of the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelHandle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}

/// What a handle moves along an axis.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    /// The low edge (left, top).
    Low,
    /// The high edge (right, bottom).
    High,
}

impl SelHandle {
    /// Corners first: where they meet an edge's handle (a very small
    /// selection) the corner wins.
    pub const ALL: [SelHandle; 8] = [
        SelHandle::Nw,
        SelHandle::Ne,
        SelHandle::Se,
        SelHandle::Sw,
        SelHandle::N,
        SelHandle::E,
        SelHandle::S,
        SelHandle::W,
    ];

    fn x_edge(self) -> Option<Edge> {
        match self {
            SelHandle::Nw | SelHandle::Sw | SelHandle::W => Some(Edge::Low),
            SelHandle::Ne | SelHandle::Se | SelHandle::E => Some(Edge::High),
            SelHandle::N | SelHandle::S => None,
        }
    }

    fn y_edge(self) -> Option<Edge> {
        match self {
            SelHandle::Nw | SelHandle::Ne | SelHandle::N => Some(Edge::Low),
            SelHandle::Sw | SelHandle::Se | SelHandle::S => Some(Edge::High),
            SelHandle::E | SelHandle::W => None,
        }
    }

    /// Where the handle is drawn and grabbed: a corner, or the middle of
    /// an edge.
    pub fn point(self, sel: Rect) -> (i32, i32) {
        let x = match self.x_edge() {
            Some(Edge::Low) => sel.x,
            Some(Edge::High) => sel.right(),
            None => sel.x + sel.w / 2,
        };
        let y = match self.y_edge() {
            Some(Edge::Low) => sel.y,
            Some(Edge::High) => sel.bottom(),
            None => sel.y + sel.h / 2,
        };
        (x, y)
    }

    pub fn cursor(self) -> Cursor {
        match self {
            SelHandle::Nw | SelHandle::Se => Cursor::SizeNwse,
            SelHandle::Ne | SelHandle::Sw => Cursor::SizeNesw,
            SelHandle::N | SelHandle::S => Cursor::SizeNs,
            SelHandle::E | SelHandle::W => Cursor::SizeWe,
        }
    }
}

/// The handle of `sel` within `tol` px of `pt` (the nearest; a corner on a
/// tie).
pub fn handle_at(sel: Rect, pt: (i32, i32), tol: i32) -> Option<SelHandle> {
    SelHandle::ALL
        .into_iter()
        .map(|h| {
            let p = h.point(sel);
            (h, (p.0 - pt.0).abs().max((p.1 - pt.1).abs()))
        })
        .filter(|(_, d)| *d <= tol)
        .min_by_key(|(_, d)| *d)
        .map(|(h, _)| h)
}

/// `sel` (as it was when the drag began) with `handle` dragged to `to`: the
/// grabbed edges follow the pointer (kept inside `bounds`), the opposite ones
/// stay. Dragged past them the selection flips over, never inverted, and it
/// is at least 1 × 1.
pub fn resize_sel(sel: Rect, handle: SelHandle, to: (i32, i32), bounds: Rect) -> Rect {
    let tx = to.0.clamp(bounds.x, bounds.right().max(bounds.x));
    let ty = to.1.clamp(bounds.y, bounds.bottom().max(bounds.y));
    let axis = |lo: i32, hi: i32, edge: Option<Edge>, t: i32, b_lo: i32, b_hi: i32| {
        let (a, b) = match edge {
            Some(Edge::Low) => (t, hi),
            Some(Edge::High) => (lo, t),
            None => (lo, hi),
        };
        let (mut a, mut b) = if a <= b { (a, b) } else { (b, a) };
        if b - a < 1 {
            if b < b_hi {
                b = a + 1;
            } else if a > b_lo {
                a = b - 1;
            }
        }
        (a, b)
    };
    let (x0, x1) = axis(
        sel.x,
        sel.right(),
        handle.x_edge(),
        tx,
        bounds.x,
        bounds.right(),
    );
    let (y0, y1) = axis(
        sel.y,
        sel.bottom(),
        handle.y_edge(),
        ty,
        bounds.y,
        bounds.bottom(),
    );
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// `sel` (as it was when the drag began) moved by `(dx, dy)`: the size
/// stays, the place is kept inside `bounds`.
pub fn move_sel(sel: Rect, dx: i32, dy: i32, bounds: Rect) -> Rect {
    clamp_into(Rect::new(sel.x + dx, sel.y + dy, sel.w, sel.h), bounds)
}

/// What a press with the Select tool grabs, in the order the rules ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectPress {
    /// A handle of the selected annotation.
    AnnotationHandle,
    /// The topmost annotation under the pointer.
    Annotation,
    /// A handle of the selection.
    SelHandle(SelHandle),
    /// Inside the selection: the whole selection moves (the annotations
    /// stay where they are on screen; only the crop changes).
    MoveSel,
    /// Outside, nothing drawn yet: a drag makes a new selection.
    NewSel,
    /// Outside with annotations: nothing.
    Nothing,
}

/// The decision of a Select press. `ann_handle` and `ann_hit` say whether the
/// selected annotation has a handle under the pointer and whether any
/// annotation does (the caller asks the session; only inside the selection
/// is anything visible).
pub fn select_press(
    sel: Rect,
    pt: (i32, i32),
    tol: i32,
    ann_handle: bool,
    ann_hit: bool,
    has_annotations: bool,
) -> SelectPress {
    if ann_handle {
        SelectPress::AnnotationHandle
    } else if ann_hit {
        SelectPress::Annotation
    } else if let Some(h) = handle_at(sel, pt, tol) {
        SelectPress::SelHandle(h)
    } else if sel.contains(pt) {
        SelectPress::MoveSel
    } else if !has_annotations {
        SelectPress::NewSel
    } else {
        SelectPress::Nothing
    }
}

/// The pointer shapes the overlay uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Cross,
    IBeam,
    SizeAll,
    SizeNwse,
    SizeNesw,
    SizeNs,
    SizeWe,
}

/// The pointer over the picture (not a button): size cursors over a
/// selection handle, the move cursor over the selection with Select, an I
/// beam with Text, a cross to draw.
pub fn cursor_over(sel: Rect, pt: (i32, i32), tol: i32, tool: Tool) -> Cursor {
    if tool == Tool::Select {
        if let Some(h) = handle_at(sel, pt, tol) {
            return h.cursor();
        }
        return if sel.contains(pt) {
            Cursor::SizeAll
        } else {
            Cursor::Arrow
        };
    }
    // A drawing tool still resizes by a handle's cursor? No: it draws.
    if !sel.contains(pt) {
        return Cursor::Arrow;
    }
    match tool {
        Tool::Text => Cursor::IBeam,
        _ => Cursor::Cross,
    }
}

// ---- keys ----

/// What the overlay knows when a key arrives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyCtx {
    /// A text EDIT is open: it has the keys.
    pub editing_text: bool,
    /// The "Buang perubahan?" bar is up.
    pub confirm: bool,
    /// Something is drawn.
    pub has_annotations: bool,
    /// A drag is running.
    pub dragging: bool,
}

/// What a key asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InlineKey {
    /// Not ours (typing goes to the EDIT, a drag is running, an unknown key).
    Ignore,
    /// Enter, Ctrl+C: copy and close.
    Copy,
    /// Ctrl+S: save and close.
    Save,
    /// Esc with something drawn: the confirm bar.
    AskDiscard,
    /// Esc with nothing drawn: close, copy nothing.
    Cancel,
    /// Enter on the confirm bar.
    Discard,
    /// Esc on the confirm bar.
    Keep,
    Undo,
    Redo,
    /// Delete the selected annotation.
    Delete,
    Tool(Tool),
}

const VK_RETURN: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_DELETE: u16 = 0x2E;

/// The key `vk` (virtual-key code) with its modifiers.
///
/// A text EDIT or a running drag takes every key. The confirm bar answers
/// Enter (discard) and Esc (keep) only. Otherwise: Enter or Ctrl+C copy,
/// Ctrl+S save, Esc asks (or cancels when nothing is drawn), Ctrl+Z undo,
/// Ctrl+Y or Ctrl+Shift+Z redo, Delete, and V T R L A P M B (no Ctrl or
/// Alt) pick a tool.
pub fn inline_key(vk: u16, ctrl: bool, shift: bool, alt: bool, ctx: KeyCtx) -> InlineKey {
    if ctx.editing_text || ctx.dragging {
        return InlineKey::Ignore;
    }
    if ctx.confirm {
        return match vk {
            VK_RETURN => InlineKey::Discard,
            VK_ESCAPE => InlineKey::Keep,
            _ => InlineKey::Ignore,
        };
    }
    if alt {
        return InlineKey::Ignore;
    }
    if ctrl {
        return match vk {
            0x43 => InlineKey::Copy,
            0x53 => InlineKey::Save,
            0x5A if shift => InlineKey::Redo,
            0x5A => InlineKey::Undo,
            0x59 => InlineKey::Redo,
            _ => InlineKey::Ignore,
        };
    }
    match vk {
        VK_RETURN => InlineKey::Copy,
        VK_ESCAPE if ctx.has_annotations => InlineKey::AskDiscard,
        VK_ESCAPE => InlineKey::Cancel,
        VK_DELETE => InlineKey::Delete,
        0x41..=0x5A => {
            Tool::from_key(char::from(vk as u8)).map_or(InlineKey::Ignore, InlineKey::Tool)
        }
        _ => InlineKey::Ignore,
    }
}

// ---- finishing ----

/// How the overlay ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishKind {
    Copy,
    Save,
    /// Esc with nothing drawn, focus lost, display change, a draw failure.
    Cancel,
    /// "Tutup" and a right click end up as Cancel or the confirm bar; the
    /// kind exists for the rule below.
    Close,
    /// The confirm bar's "Buang".
    Discard,
    OpenEditor,
}

/// Whether ending as `kind` puts the picture on the clipboard: Copy always,
/// Save only with "Salin otomatis" (latched when the capture started),
/// nothing else. "Salin otomatis" never copies at the pick or on a timer in
/// the inline mode; only these finishes do.
pub fn finish_copies(kind: FinishKind, auto_copy: bool) -> bool {
    match kind {
        FinishKind::Copy => true,
        FinishKind::Save => auto_copy,
        FinishKind::Cancel | FinishKind::Close | FinishKind::Discard | FinishKind::OpenEditor => {
            false
        }
    }
}

// ---- painting ----

/// The stages of one frame. Direct2D paints in call order, so a panel's
/// background has to come before the buttons on it ([[shot-editor-paint-order]]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Frame,
    Dim,
    Annotations,
    Border,
    Label,
    Handles,
    StripBack,
    BarBack,
    StripButtons,
    BarButtons,
    BarText,
    Tooltip,
    Notice,
}

pub const PAINT_ORDER: [Layer; 13] = [
    Layer::Frame,
    Layer::Dim,
    Layer::Annotations,
    Layer::Border,
    Layer::Label,
    Layer::Handles,
    Layer::StripBack,
    Layer::BarBack,
    Layer::StripButtons,
    Layer::BarButtons,
    Layer::BarText,
    Layer::Tooltip,
    Layer::Notice,
];

#[cfg(test)]
mod tests {
    use super::*;

    const MON: Rect = Rect::new(0, 0, 1920, 1080);

    fn overlaps(a: Rect, b: Rect) -> bool {
        !a.intersect(&b).is_empty()
    }

    fn inside(r: Rect, bounds: Rect) -> bool {
        r.x >= bounds.x
            && r.y >= bounds.y
            && r.right() <= bounds.right()
            && r.bottom() <= bounds.bottom()
    }

    // ---- the strip and the bar ----

    #[test]
    fn the_strip_holds_nineteen_buttons_in_order_in_one_column() {
        for ui in [1.0, 1.25, 1.5, 2.0] {
            let p = strip_layout(ui, 100_000);
            let order: Vec<Btn> = p.items.iter().map(|(b, _)| *b).collect();
            assert_eq!(order.len(), 8 + 6 + 3 + 2);
            let tools: Vec<Btn> = STRIP_TOOLS.iter().map(|t| Btn::Tool(*t)).collect();
            assert_eq!(&order[..8], tools.as_slice());
            assert_eq!(order[8], Btn::Color(0));
            assert_eq!(order[13], Btn::Color(5));
            assert_eq!(order[14], Btn::Stroke(0));
            assert_eq!(&order[17..], [Btn::Undo, Btn::Redo]);
            // One column: the same x, strictly going down, nothing touching.
            let xs: Vec<i32> = p.items.iter().map(|(_, r)| r.x).collect();
            assert!(xs.windows(2).all(|w| w[0] == w[1]), "{ui}");
            for pair in p.items.windows(2) {
                assert!(pair[0].1.bottom() <= pair[1].1.y, "{ui}: {pair:?}");
            }
            for (b, r) in &p.items {
                assert!(inside(*r, p.rect), "{b:?} {r:?} in {:?}", p.rect);
                assert!(r.w >= 20 && r.h >= 20, "{b:?} is a button");
            }
            assert_eq!(p.seps.len(), 3, "three groups apart");
            for sep in &p.seps {
                assert!(inside(*sep, p.rect));
                assert!(p.items.iter().all(|(_, r)| !overlaps(*r, *sep)), "{sep:?}");
            }
        }
    }

    #[test]
    fn the_strip_goes_to_two_columns_when_one_is_too_tall() {
        let one = strip_layout(1.0, 100_000);
        assert!(one.rect.h > 400, "{:?}", one.rect);
        let max_h = 400;
        let two = strip_layout(1.0, max_h);
        let mut xs: Vec<i32> = two.items.iter().map(|(_, r)| r.x).collect();
        xs.dedup();
        assert_eq!(xs.len(), 2, "two columns: {xs:?}");
        assert!(two.rect.h <= max_h, "{:?}", two.rect);
        assert!(two.rect.w > one.rect.w);
        // The order still runs down the first column, then the second.
        let order: Vec<Btn> = two.items.iter().map(|(b, _)| *b).collect();
        let want: Vec<Btn> = one.items.iter().map(|(b, _)| *b).collect();
        assert_eq!(order, want);
        for (i, (_, a)) in two.items.iter().enumerate() {
            assert!(inside(*a, two.rect), "{a:?}");
            for (_, b) in &two.items[i + 1..] {
                assert!(!overlaps(*a, *b), "{a:?} / {b:?}");
            }
        }
        // A limit that already fits leaves one column.
        let fits = strip_layout(1.0, one.rect.h);
        assert_eq!(fits, one);
        // Absurdly low: still every button once, no empty column.
        let tiny = strip_layout(1.0, 10);
        assert_eq!(tiny.items.len(), 19);
        assert!(tiny.items.iter().all(|(_, r)| r.w > 0));
    }

    #[test]
    fn the_bar_has_its_four_buttons_or_the_confirm_question() {
        let labels = BarLabels {
            copy: 30.0,
            save: 40.0,
            editor: 90.0,
            close: 34.0,
            ask: 110.0,
            discard: 36.0,
            keep: 34.0,
        };
        for ui in [1.0, 1.5, 2.0] {
            let bar = bar_layout(ui, &labels, false);
            let order: Vec<Btn> = bar.items.iter().map(|(b, _)| *b).collect();
            assert_eq!(order, [Btn::Copy, Btn::Save, Btn::OpenEditor, Btn::Close]);
            assert!(bar.text.is_none());
            let mut last = 0;
            for (b, r) in &bar.items {
                assert!(inside(*r, bar.rect), "{b:?}");
                assert!(r.x >= last, "left to right without touching");
                last = r.right();
            }
            // The text buttons are wider than their label.
            assert!(bar.rect_of(Btn::OpenEditor).unwrap().w > 90);
            let ask = bar_layout(ui, &labels, true);
            let order: Vec<Btn> = ask.items.iter().map(|(b, _)| *b).collect();
            assert_eq!(order, [Btn::Discard, Btn::Keep]);
            let q = ask.text.expect("the question");
            assert!(q.right() <= ask.items[0].1.x, "the question before Buang");
            assert!(inside(q, ask.rect));
            assert!(ask.items.iter().all(|(_, r)| inside(*r, ask.rect)));
        }
    }

    #[test]
    fn a_panel_moves_with_all_its_parts_and_finds_its_buttons() {
        let p = strip_layout(1.0, 100_000);
        let moved = p.moved_to(500, 300);
        assert_eq!((moved.rect.x, moved.rect.y), (500, 300));
        assert_eq!(moved.rect.w, p.rect.w);
        let (b, r) = moved.items[3];
        assert_eq!(r.x - 500, p.items[3].1.x);
        assert_eq!(r.y - 300, p.items[3].1.y);
        assert_eq!(moved.hit((r.x + 2, r.y + 2)), Some(b));
        assert_eq!(moved.hit((r.x - 400, r.y)), None);
        assert_eq!(moved.rect_of(b), Some(r));
    }

    // ---- placement ----

    fn strip() -> (i32, i32) {
        let p = strip_layout(1.0, 1000);
        (p.rect.w, p.rect.h)
    }

    fn bar() -> (i32, i32) {
        let p = bar_layout(
            1.0,
            &BarLabels {
                copy: 30.0,
                save: 40.0,
                editor: 90.0,
                close: 34.0,
                ..Default::default()
            },
            false,
        );
        (p.rect.w, p.rect.h)
    }

    #[test]
    fn the_strip_goes_right_of_the_selection_and_the_bar_under_it() {
        let sel = Rect::new(400, 200, 600, 400);
        let (sw, sh) = strip();
        let r = place_side(sel, (sw, sh), MON, Side::Right, 8);
        assert_eq!((r.x, r.y), (sel.right() + 8, sel.y));
        assert_eq!((r.w, r.h), (sw, sh));
        let (bw, bh) = bar();
        let b = place_side(sel, (bw, bh), MON, Side::Below, 8);
        assert_eq!(b.right(), sel.right(), "aligned to the right edge");
        assert_eq!(b.y, sel.bottom() + 8);
        // Not touching in the normal case.
        assert!(!overlaps(r, b), "{r:?} / {b:?}");
        assert!(!overlaps(r, sel) && !overlaps(b, sel));
        assert!(inside(r, MON) && inside(b, MON));
    }

    #[test]
    fn a_panel_flips_to_the_other_side_at_the_edges_of_the_monitor() {
        let (sw, sh) = (50, 300);
        // No room right: left of the selection.
        let sel = Rect::new(1500, 100, 400, 300);
        let r = place_side(sel, (sw, sh), MON, Side::Right, 8);
        assert_eq!((r.x, r.y), (sel.x - 8 - sw, 100));
        // No room below: above.
        let (bw, bh) = (300, 40);
        let sel = Rect::new(600, 900, 400, 160);
        let b = place_side(sel, (bw, bh), MON, Side::Below, 8);
        assert_eq!((b.right(), b.y), (sel.right(), sel.y - 8 - bh));
        // Room on the preferred side: it stays there.
        let sel = Rect::new(100, 100, 400, 300);
        assert_eq!(
            place_side(sel, (sw, sh), MON, Side::Right, 8).x,
            sel.right() + 8
        );
        assert_eq!(
            place_side(sel, (bw, bh), MON, Side::Below, 8).y,
            sel.bottom() + 8
        );
    }

    #[test]
    fn a_selection_that_fills_the_monitor_gets_its_panels_inside() {
        let sel = MON;
        let (sw, sh) = strip();
        let r = place_side(sel, (sw, sh), MON, Side::Right, 8);
        assert_eq!(r.right(), sel.right(), "inside the right edge");
        assert_eq!(r.y, sel.y);
        assert!(inside(r, MON));
        let (bw, bh) = bar();
        let b = place_side(sel, (bw, bh), MON, Side::Below, 8);
        assert_eq!(b.bottom(), sel.bottom(), "inside the bottom edge");
        assert_eq!(b.right(), sel.right());
        assert!(inside(b, MON));
        // A panel taller than the monitor is pinned to its top.
        let tall = place_side(sel, (50, 5000), MON, Side::Right, 8);
        assert_eq!(tall.y, MON.y);
    }

    #[test]
    fn panels_of_a_selection_on_a_second_monitor_stay_on_it() {
        // A monitor to the left of the primary: negative coordinates.
        let left = Rect::new(-1920, 0, 1920, 1080);
        let (sw, sh) = strip();
        let (bw, bh) = bar();
        for sel in [
            Rect::new(-1500, 200, 600, 400),
            Rect::new(-300, 100, 280, 300),
            Rect::new(-1900, 1000, 100, 80),
            Rect::new(-1920, 0, 1920, 1080),
        ] {
            let r = place_side(sel, (sw, sh), left, Side::Right, 8);
            let b = place_side(sel, (bw, bh), left, Side::Below, 8);
            assert!(inside(r, left), "{sel:?}: {r:?}");
            assert!(inside(b, left), "{sel:?}: {b:?}");
        }
        // Beside the right edge when it fits, like on the primary.
        let sel = Rect::new(-1500, 200, 600, 400);
        assert_eq!(
            place_side(sel, (sw, sh), left, Side::Right, 8).x,
            sel.right() + 8
        );
        // The right edge of this monitor is x = 0: no room right of this one.
        let sel = Rect::new(-700, 200, 690, 400);
        assert_eq!(
            place_side(sel, (sw, sh), left, Side::Right, 8).x,
            sel.x - 8 - sw
        );
    }

    #[test]
    fn the_bar_steps_aside_when_it_would_cover_the_strip() {
        let (sw, sh) = (46, 640);
        let (bw, bh) = (320, 40);
        // A narrow selection near the right edge: the strip is on its left
        // and the bar, right aligned, would run under it.
        let sel = Rect::new(1820, 100, 80, 300);
        let strip = place_side(sel, (sw, sh), MON, Side::Right, 8);
        assert!(strip.right() < sel.x);
        let bar = place_side(sel, (bw, bh), MON, Side::Below, 8);
        assert!(overlaps(strip, bar), "the case that needs it");
        let moved = separate(bar, strip, MON, 8);
        assert!(!overlaps(strip, moved), "{strip:?} / {moved:?}");
        assert!(inside(moved, MON));
        // No overlap, no change.
        let sel = Rect::new(400, 200, 600, 400);
        let strip = place_side(sel, (sw, sh), MON, Side::Right, 8);
        let bar = place_side(sel, (bw, bh), MON, Side::Below, 8);
        assert_eq!(separate(bar, strip, MON, 8), bar);
    }

    #[test]
    fn the_size_label_sits_above_the_corner_or_just_inside() {
        let size = (90, 24);
        let sel = Rect::new(300, 200, 500, 300);
        assert_eq!(
            label_rect(sel, size, MON, 6),
            Rect::new(300, 200 - 6 - 24, 90, 24)
        );
        // No room above: inside the corner.
        let top = Rect::new(300, 10, 500, 300);
        assert_eq!(label_rect(top, size, MON, 6), Rect::new(300, 16, 90, 24));
        // A second monitor's top, negative x.
        let left = Rect::new(-1920, 0, 1920, 1080);
        let sel = Rect::new(-1900, 4, 300, 300);
        let l = label_rect(sel, size, left, 6);
        assert_eq!(l.y, 10);
        assert!(inside(l, left));
        // Never off the monitor on the right.
        let right = Rect::new(1900, 400, 20, 20);
        assert!(inside(label_rect(right, size, MON, 6), MON));
    }

    #[test]
    fn a_tooltip_sits_beside_its_button_on_the_monitor() {
        let btn = Rect::new(1000, 300, 30, 30);
        let size = (120, 26);
        let v = tooltip_rect(btn, size, MON, true, 6);
        assert_eq!(v.x, btn.right() + 6);
        assert_eq!(v.y + v.h / 2, btn.y + btn.h / 2, "centred on the button");
        // No room right: left.
        let edge = Rect::new(1880, 300, 30, 30);
        assert_eq!(tooltip_rect(edge, size, MON, true, 6).right(), edge.x - 6);
        // The bar's buttons: under, else above.
        let b = Rect::new(800, 600, 80, 30);
        let h = tooltip_rect(b, size, MON, false, 6);
        assert_eq!(h.y, b.bottom() + 6);
        let low = Rect::new(800, 1040, 80, 30);
        assert_eq!(tooltip_rect(low, size, MON, false, 6).bottom(), low.y - 6);
        assert!(inside(
            tooltip_rect(Rect::new(0, 0, 30, 30), size, MON, false, 6),
            MON
        ));
    }

    // ---- the selection ----

    const BOUNDS: Rect = Rect::new(-100, 0, 2000, 1000);

    #[test]
    fn a_handle_is_found_near_a_corner_or_the_middle_of_an_edge() {
        let sel = Rect::new(100, 100, 200, 100);
        assert_eq!(SelHandle::ALL.len(), 8);
        assert_eq!(handle_at(sel, (100, 100), 6), Some(SelHandle::Nw));
        assert_eq!(handle_at(sel, (303, 98), 6), Some(SelHandle::Ne));
        assert_eq!(handle_at(sel, (300, 200), 6), Some(SelHandle::Se));
        assert_eq!(handle_at(sel, (100, 205), 6), Some(SelHandle::Sw));
        assert_eq!(handle_at(sel, (200, 100), 6), Some(SelHandle::N));
        assert_eq!(handle_at(sel, (301, 150), 6), Some(SelHandle::E));
        assert_eq!(handle_at(sel, (200, 199), 6), Some(SelHandle::S));
        assert_eq!(handle_at(sel, (97, 150), 6), Some(SelHandle::W));
        assert_eq!(
            handle_at(sel, (200, 150), 6),
            None,
            "the middle is the move area"
        );
        assert_eq!(
            handle_at(sel, (100, 130), 6),
            None,
            "an edge away from its handle"
        );
        // A tiny selection: the corner wins where handles meet.
        let tiny = Rect::new(100, 100, 6, 6);
        assert_eq!(handle_at(tiny, (100, 100), 6), Some(SelHandle::Nw));
        assert_eq!(handle_at(tiny, (106, 106), 6), Some(SelHandle::Se));
    }

    #[test]
    fn dragging_a_handle_moves_only_the_grabbed_edges() {
        let sel = Rect::new(200, 200, 300, 200);
        // Corner: two edges.
        assert_eq!(
            resize_sel(sel, SelHandle::Se, (600, 500), BOUNDS),
            Rect::new(200, 200, 400, 300)
        );
        assert_eq!(
            resize_sel(sel, SelHandle::Nw, (150, 100), BOUNDS),
            Rect::new(150, 100, 350, 300)
        );
        // Edge: one edge, the other axis stays whatever the pointer does.
        assert_eq!(
            resize_sel(sel, SelHandle::E, (700, 999), BOUNDS),
            Rect::new(200, 200, 500, 200)
        );
        assert_eq!(
            resize_sel(sel, SelHandle::N, (0, 150), BOUNDS),
            Rect::new(200, 150, 300, 250)
        );
        assert_eq!(
            resize_sel(sel, SelHandle::W, (250, 0), BOUNDS),
            Rect::new(250, 200, 250, 200)
        );
        assert_eq!(
            resize_sel(sel, SelHandle::S, (0, 450), BOUNDS),
            Rect::new(200, 200, 300, 250)
        );
    }

    #[test]
    fn dragging_past_the_opposite_edge_flips_the_selection() {
        let sel = Rect::new(200, 200, 300, 200);
        // E past the left edge: the left edge is now the right.
        let r = resize_sel(sel, SelHandle::E, (100, 300), BOUNDS);
        assert_eq!(r, Rect::new(100, 200, 100, 200));
        // Se past the top-left corner flips both axes.
        let r = resize_sel(sel, SelHandle::Se, (150, 100), BOUNDS);
        assert_eq!(r, Rect::new(150, 100, 50, 100));
        // Nw past the bottom-right corner.
        let r = resize_sel(sel, SelHandle::Nw, (600, 500), BOUNDS);
        assert_eq!(r, Rect::new(500, 400, 100, 100));
        for h in SelHandle::ALL {
            for to in [
                (-500, -500),
                (5000, 5000),
                (350, 300),
                (200, 200),
                (500, 400),
            ] {
                let r = resize_sel(sel, h, to, BOUNDS);
                assert!(r.w >= 1 && r.h >= 1, "{h:?} {to:?}: {r:?}");
                assert!(inside(r, BOUNDS), "{h:?} {to:?}: {r:?}");
            }
        }
    }

    #[test]
    fn a_resize_is_at_least_one_pixel_and_stays_in_the_bounds() {
        let sel = Rect::new(200, 200, 300, 200);
        // Exactly onto the opposite edge: widened to 1.
        let r = resize_sel(sel, SelHandle::E, (200, 300), BOUNDS);
        assert_eq!((r.x, r.w), (200, 1));
        let r = resize_sel(sel, SelHandle::S, (300, 200), BOUNDS);
        assert_eq!((r.y, r.h), (200, 1));
        // At the far bound the 1 px grows the other way, not out.
        let edge = Rect::new(1890, 100, 10, 50);
        let r = resize_sel(edge, SelHandle::W, (1900, 120), BOUNDS);
        assert_eq!((r.x, r.w), (1899, 1));
        assert!(inside(r, BOUNDS));
        // The pointer far outside is held at the bounds.
        let r = resize_sel(sel, SelHandle::Se, (9999, 9999), BOUNDS);
        assert_eq!((r.right(), r.bottom()), (BOUNDS.right(), BOUNDS.bottom()));
        let r = resize_sel(sel, SelHandle::Nw, (-9999, -9999), BOUNDS);
        assert_eq!((r.x, r.y), (BOUNDS.x, BOUNDS.y));
    }

    #[test]
    fn moving_the_selection_keeps_its_size_and_stays_in_the_bounds() {
        let sel = Rect::new(200, 200, 300, 200);
        assert_eq!(
            move_sel(sel, 50, -20, BOUNDS),
            Rect::new(250, 180, 300, 200)
        );
        assert_eq!(
            move_sel(sel, -9999, -9999, BOUNDS),
            Rect::new(-100, 0, 300, 200)
        );
        assert_eq!(
            move_sel(sel, 9999, 9999, BOUNDS),
            Rect::new(BOUNDS.right() - 300, BOUNDS.bottom() - 200, 300, 200)
        );
        // Bigger than the bounds: pinned to the top-left, size kept.
        let big = Rect::new(0, 0, 3000, 2000);
        let m = move_sel(big, 40, 40, BOUNDS);
        assert_eq!((m.x, m.y, m.w, m.h), (-100, 0, 3000, 2000));
    }

    #[test]
    fn a_select_press_grabs_in_the_documented_order() {
        let sel = Rect::new(100, 100, 300, 200);
        let corner = (100, 100);
        let inside_pt = (250, 200);
        let outside = (600, 600);
        // The selected annotation's handle beats everything.
        assert_eq!(
            select_press(sel, corner, 6, true, true, true),
            SelectPress::AnnotationHandle
        );
        // Then an annotation under the pointer, even over a sel handle.
        assert_eq!(
            select_press(sel, corner, 6, false, true, true),
            SelectPress::Annotation
        );
        // Then a selection handle.
        assert_eq!(
            select_press(sel, corner, 6, false, false, true),
            SelectPress::SelHandle(SelHandle::Nw)
        );
        // Inside: move the selection.
        assert_eq!(
            select_press(sel, inside_pt, 6, false, false, true),
            SelectPress::MoveSel
        );
        // Outside: a new selection while nothing is drawn, else nothing.
        assert_eq!(
            select_press(sel, outside, 6, false, false, false),
            SelectPress::NewSel
        );
        assert_eq!(
            select_press(sel, outside, 6, false, false, true),
            SelectPress::Nothing
        );
    }

    #[test]
    fn the_pointer_shape_follows_what_a_press_would_do() {
        let sel = Rect::new(100, 100, 300, 200);
        assert_eq!(
            cursor_over(sel, (100, 100), 6, Tool::Select),
            Cursor::SizeNwse
        );
        assert_eq!(
            cursor_over(sel, (400, 100), 6, Tool::Select),
            Cursor::SizeNesw
        );
        assert_eq!(
            cursor_over(sel, (250, 100), 6, Tool::Select),
            Cursor::SizeNs
        );
        assert_eq!(
            cursor_over(sel, (400, 200), 6, Tool::Select),
            Cursor::SizeWe
        );
        assert_eq!(
            cursor_over(sel, (250, 200), 6, Tool::Select),
            Cursor::SizeAll
        );
        assert_eq!(cursor_over(sel, (900, 900), 6, Tool::Select), Cursor::Arrow);
        assert_eq!(cursor_over(sel, (250, 200), 6, Tool::Text), Cursor::IBeam);
        for t in [
            Tool::Pen,
            Tool::Line,
            Tool::Arrow,
            Tool::Rect,
            Tool::Marker,
            Tool::Pixelate,
        ] {
            assert_eq!(cursor_over(sel, (250, 200), 6, t), Cursor::Cross, "{t:?}");
            assert_eq!(cursor_over(sel, (900, 900), 6, t), Cursor::Arrow, "{t:?}");
        }
    }

    // ---- keys ----

    fn key(vk: u16, ctrl: bool, shift: bool, ctx: KeyCtx) -> InlineKey {
        inline_key(vk, ctrl, shift, false, ctx)
    }

    const NONE: KeyCtx = KeyCtx {
        editing_text: false,
        confirm: false,
        has_annotations: false,
        dragging: false,
    };

    #[test]
    fn enter_and_ctrl_c_copy_and_ctrl_s_saves() {
        assert_eq!(key(0x0D, false, false, NONE), InlineKey::Copy);
        assert_eq!(key(0x43, true, false, NONE), InlineKey::Copy);
        assert_eq!(key(0x53, true, false, NONE), InlineKey::Save);
        // C and S without Ctrl are not those: S is no tool, C neither.
        assert_eq!(key(0x43, false, false, NONE), InlineKey::Ignore);
        assert_eq!(key(0x53, false, false, NONE), InlineKey::Ignore);
        // Alt+Enter and Alt+letters are the system's.
        assert_eq!(
            inline_key(0x0D, false, false, true, NONE),
            InlineKey::Ignore
        );
        assert_eq!(
            inline_key(0x50, false, false, true, NONE),
            InlineKey::Ignore
        );
    }

    #[test]
    fn esc_asks_only_when_something_is_drawn() {
        assert_eq!(key(0x1B, false, false, NONE), InlineKey::Cancel);
        let drawn = KeyCtx {
            has_annotations: true,
            ..NONE
        };
        assert_eq!(key(0x1B, false, false, drawn), InlineKey::AskDiscard);
    }

    #[test]
    fn undo_redo_delete_and_the_tool_letters() {
        assert_eq!(key(0x5A, true, false, NONE), InlineKey::Undo);
        assert_eq!(key(0x59, true, false, NONE), InlineKey::Redo);
        assert_eq!(key(0x5A, true, true, NONE), InlineKey::Redo);
        assert_eq!(key(0x2E, false, false, NONE), InlineKey::Delete);
        for (c, t) in [
            ('V', Tool::Select),
            ('T', Tool::Text),
            ('R', Tool::Rect),
            ('L', Tool::Line),
            ('A', Tool::Arrow),
            ('P', Tool::Pen),
            ('M', Tool::Marker),
            ('B', Tool::Pixelate),
        ] {
            assert_eq!(key(c as u16, false, false, NONE), InlineKey::Tool(t), "{c}");
            // Shift does not matter for a letter.
            assert_eq!(key(c as u16, false, true, NONE), InlineKey::Tool(t), "{c}");
            // With Ctrl it is a shortcut, never a tool.
            assert!(
                !matches!(key(c as u16, true, false, NONE), InlineKey::Tool(_)),
                "{c}"
            );
        }
        assert_eq!(
            key(0x58, false, false, NONE),
            InlineKey::Ignore,
            "X is nothing"
        );
        assert_eq!(
            key(0x70, false, false, NONE),
            InlineKey::Ignore,
            "F1 is nothing"
        );
    }

    #[test]
    fn a_text_edit_or_a_drag_takes_every_key() {
        let editing = KeyCtx {
            editing_text: true,
            has_annotations: true,
            ..NONE
        };
        // Esc only cancels the text (the EDIT's own job), Enter commits it:
        // the overlay does nothing with them.
        for vk in [0x1B, 0x0D, 0x5A, 0x2E, 0x50] {
            assert_eq!(key(vk, false, false, editing), InlineKey::Ignore, "{vk:#x}");
            assert_eq!(key(vk, true, false, editing), InlineKey::Ignore, "{vk:#x}");
        }
        let dragging = KeyCtx {
            dragging: true,
            ..NONE
        };
        for vk in [0x1B, 0x0D, 0x5A, 0x2E] {
            assert_eq!(key(vk, false, false, dragging), InlineKey::Ignore);
            assert_eq!(key(vk, true, false, dragging), InlineKey::Ignore);
        }
    }

    #[test]
    fn the_confirm_bar_answers_enter_and_esc_only() {
        let bar = KeyCtx {
            confirm: true,
            has_annotations: true,
            ..NONE
        };
        assert_eq!(key(0x0D, false, false, bar), InlineKey::Discard);
        assert_eq!(key(0x1B, false, false, bar), InlineKey::Keep);
        for (vk, ctrl) in [
            (0x43, true),
            (0x53, true),
            (0x5A, true),
            (0x2E, false),
            (0x50, false),
        ] {
            assert_eq!(key(vk, ctrl, false, bar), InlineKey::Ignore, "{vk:#x}");
        }
    }

    // ---- finishing and painting ----

    #[test]
    fn only_copy_and_an_auto_copying_save_touch_the_clipboard() {
        for auto in [false, true] {
            assert!(finish_copies(FinishKind::Copy, auto));
            assert_eq!(finish_copies(FinishKind::Save, auto), auto);
            for kind in [
                FinishKind::Cancel,
                FinishKind::Close,
                FinishKind::Discard,
                FinishKind::OpenEditor,
            ] {
                assert!(!finish_copies(kind, auto), "{kind:?}");
            }
        }
    }

    #[test]
    fn a_panels_background_is_painted_before_its_buttons() {
        let at = |l: Layer| PAINT_ORDER.iter().position(|x| *x == l).unwrap();
        assert!(at(Layer::StripBack) < at(Layer::StripButtons));
        assert!(at(Layer::BarBack) < at(Layer::BarButtons));
        assert!(at(Layer::BarBack) < at(Layer::BarText));
        assert!(at(Layer::Frame) < at(Layer::Dim) && at(Layer::Dim) < at(Layer::Annotations));
        // The tooltip and the notice are over everything.
        assert!(at(Layer::Tooltip) > at(Layer::BarText));
        assert_eq!(PAINT_ORDER.last(), Some(&Layer::Notice));
        // Each stage once.
        for (i, a) in PAINT_ORDER.iter().enumerate() {
            assert!(PAINT_ORDER[i + 1..].iter().all(|b| b != a), "{a:?}");
        }
    }

    #[test]
    fn a_button_names_itself_with_its_shortcut() {
        crate::i18n::set_lang(crate::i18n::Lang::En);
        assert_eq!(tip(Btn::Tool(Tool::Line)).as_deref(), Some("Line (L)"));
        assert_eq!(tip(Btn::Tool(Tool::Pen)).as_deref(), Some("Pen (P)"));
        assert_eq!(tip(Btn::Undo).as_deref(), Some("Undo (Ctrl+Z)"));
        assert_eq!(tip(Btn::Copy).as_deref(), Some("Copy (Ctrl+C)"));
        assert_eq!(tip(Btn::Save).as_deref(), Some("Save (Ctrl+S)"));
        assert_eq!(tip(Btn::Close).as_deref(), Some("Close (Esc)"));
        assert_eq!(tip(Btn::OpenEditor).as_deref(), Some("Open in editor"));
        assert!(tip(Btn::Discard).is_none() && tip(Btn::Keep).is_none());
        for t in STRIP_TOOLS {
            let tip = tip(Btn::Tool(t)).expect("a tip");
            assert!(tip.ends_with(&format!("({})", t.key())), "{tip}");
        }
        crate::i18n::set_lang(crate::i18n::Lang::Id);
        assert_eq!(tip(Btn::OpenEditor).as_deref(), Some("Buka di editor"));
        assert_eq!(tip(Btn::Tool(Tool::Line)).as_deref(), Some("Garis (L)"));
    }

    #[test]
    fn the_strip_has_every_tool_once() {
        let mut all = STRIP_TOOLS.to_vec();
        all.sort_by_key(|t| t.key());
        let mut every = Tool::ALL.to_vec();
        every.sort_by_key(|t| t.key());
        assert_eq!(all, every);
    }

    #[test]
    fn the_ui_scale_follows_the_dpi() {
        assert_eq!(ui_for(96), 1.0);
        assert_eq!(ui_for(144), 1.5);
        assert_eq!(ui_for(0), 1.0, "never below 1");
    }
}
