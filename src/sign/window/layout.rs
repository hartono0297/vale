//! Where everything sits in the signing window and what is under the
//! pointer. Pure: physical pixels in, rectangles out (`ui` is dpi / 96), so
//! it is all tested without a window.

use crate::i18n::Key;
use crate::sign::geom;
use crate::sign::{Item, Quarter};
use crate::ui::geometry::Rect;

// ---- sizes (DIPs, scaled by `ui`) ----

pub const TITLE_H: f32 = 40.0;
pub const TOOLS_H: f32 = 48.0;
pub const BANNER_H: f32 = 34.0;
pub const RAIL_W: f32 = 132.0;
pub const THUMB_W: f32 = 74.0;
pub const FOOT_H: f32 = 56.0;
/// Space kept around the page in the stage.
pub const PAGE_MARGIN: f32 = 24.0;
const EDGE: f32 = 12.0;
const TOOL_H: f32 = 34.0;
const TOOL_GAP: f32 = 4.0;
const ICON: f32 = 16.0;
const ICON_GAP: f32 = 7.0;
const TOOL_PAD: f32 = 11.0;
const DIVIDER: f32 = 13.0;
const MORE_W: f32 = 38.0;
const BTN_H: f32 = 34.0;
const BTN_PAD: f32 = 16.0;
const BTN_GAP: f32 = 8.0;
const SEG_PAD: f32 = 12.0;
/// Rail rows: the gap above the first thumbnail and between rows, and the
/// page number line under each thumbnail.
const RAIL_GAP: f32 = 12.0;
const RAIL_LABEL: f32 = 18.0;
/// The tallest a thumbnail may be, as a share of its width.
const THUMB_MAX_ASPECT: f32 = 1.6;
const FLOAT_H: f32 = 32.0;
const FLOAT_PAD: f32 = 4.0;
const FLOAT_ITEM_PAD: f32 = 9.0;
const FLOAT_GAP: f32 = 10.0;
const HANDLE: f32 = 8.0;
const HANDLE_HIT: f32 = 16.0;
const MENU_ROW: f32 = 36.0;
const MENU_W: f32 = 210.0;
const TILE_W: f32 = 104.0;
const TILE_H: f32 = 52.0;
const POP_PAD: f32 = 8.0;
const FIELD_W: f32 = 240.0;
const FIELD_H: f32 = 34.0;

/// The five things that go on a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Signature,
    Initials,
    Date,
    Text,
    Check,
}

impl Tool {
    pub const ALL: [Tool; 5] = [Tool::Signature, Tool::Initials, Tool::Date, Tool::Text, Tool::Check];
}

/// A corner of an item's rectangle as shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];

    fn left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }

    fn top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }
}

/// Everything a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Tool(Tool),
    Rotate,
    DeletePage,
    /// The "…" button holding the tools that do not fit.
    More,
    /// A page in the rail (session page index).
    Thumb(usize),
    FormatImage,
    FormatPdf,
    Cancel,
    Save,
    /// The close-with-changes bar.
    Discard,
    Keep,
    /// The "breaks the digital signature" bar.
    SaveAnyway,
    NotNow,
    OpenPassword,
    /// "Tutup" under an error that ends the window.
    CloseWindow,
    // The selected item's floating bar.
    Smaller,
    Bigger,
    AddDate,
    AllPages,
    Remove,
    // The saved-signature picker.
    PickMark(usize),
    PickNew,
    /// A popover's own background: takes the click, does nothing.
    Inert,
    Handle(Corner),
    Item(usize),
    Page,
    Stage,
}

/// The words a button shows.
pub fn label_key(hit: Hit) -> Option<Key> {
    Some(match hit {
        Hit::Tool(Tool::Signature) => Key::SignToolSignature,
        Hit::Tool(Tool::Initials) => Key::SignToolInitials,
        Hit::Tool(Tool::Date) => Key::SignToolDate,
        Hit::Tool(Tool::Text) => Key::SignToolText,
        Hit::Tool(Tool::Check) => Key::SignToolCheck,
        Hit::Rotate => Key::SignRotate,
        Hit::DeletePage => Key::SignDeletePage,
        Hit::FormatImage => Key::SignFormatImage,
        Hit::FormatPdf => Key::SignFormatPdf,
        Hit::Cancel | Hit::NotNow => Key::SignCancel,
        Hit::Save => Key::SignSave,
        Hit::Discard => Key::EditDiscard,
        Hit::Keep => Key::EditKeep,
        Hit::SaveAnyway => Key::SignSaveAnyway,
        Hit::OpenPassword => Key::SignOpen,
        Hit::CloseWindow => Key::SignClose,
        Hit::Smaller => Key::SignSmaller,
        Hit::Bigger => Key::SignBigger,
        Hit::AddDate => Key::SignAddDate,
        Hit::AllPages => Key::SignAllPages,
        Hit::Remove => Key::SignRemove,
        Hit::PickNew => Key::SignMakerNew,
        _ => return None,
    })
}

/// Every hit that has a label, for measuring once.
pub const LABELLED: [Hit; 22] = [
    Hit::Tool(Tool::Signature),
    Hit::Tool(Tool::Initials),
    Hit::Tool(Tool::Date),
    Hit::Tool(Tool::Text),
    Hit::Tool(Tool::Check),
    Hit::Rotate,
    Hit::DeletePage,
    Hit::FormatImage,
    Hit::FormatPdf,
    Hit::Cancel,
    Hit::Save,
    Hit::Discard,
    Hit::Keep,
    Hit::SaveAnyway,
    Hit::OpenPassword,
    Hit::CloseWindow,
    Hit::Smaller,
    Hit::Bigger,
    Hit::AddDate,
    Hit::AllPages,
    Hit::Remove,
    Hit::PickNew,
];

/// Measured label widths (physical px at the window's text size).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Labels(pub Vec<(Hit, f32)>);

impl Labels {
    pub fn w(&self, hit: Hit) -> f32 {
        let hit = if hit == Hit::NotNow { Hit::Cancel } else { hit };
        self.0.iter().find(|(h, _)| *h == hit).map_or(0.0, |(_, w)| *w)
    }
}

/// What the footer holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Footer {
    /// The output name, Batal and Simpan; `formats` adds Gambar / PDF.
    Normal { formats: bool },
    /// Nothing to save yet (opening, password): Batal only.
    CancelOnly,
    /// An error ended it: the stage's Tutup is the only button.
    Empty,
    /// "Buang perubahan?" with Buang and Batal.
    AskDiscard,
    /// "The digital signature will no longer be valid" with Simpan tetap and Batal.
    AskBreaks,
}

/// What the stage shows instead of a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageMode {
    Page,
    /// One line (loading, converting).
    Message,
    /// The line, a password field and Buka.
    Password,
    /// The line and Tutup.
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub cw: f32,
    pub ch: f32,
    pub ui: f32,
    pub banner: bool,
    pub footer: Footer,
    pub stage: StageMode,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub title: Rect,
    pub tools: Rect,
    pub banner: Option<Rect>,
    pub rail: Rect,
    pub stage: Rect,
    pub footer: Rect,
    /// The footer's text box (output name or question), left of the buttons.
    pub foot_text: Rect,
    /// Every button of the chrome: tools row, footer, stage.
    pub buttons: Vec<(Hit, Rect)>,
    /// Tools that did not fit, in order: they live in the "…" menu.
    pub overflow: Vec<Hit>,
    /// x of the line between the item tools and the page tools.
    pub divider: Option<f32>,
    /// The stage's message line.
    pub message: Rect,
    /// Where the password EDIT goes.
    pub field: Option<Rect>,
}

impl Layout {
    pub fn rect_of(&self, hit: Hit) -> Option<Rect> {
        self.buttons.iter().find(|(h, _)| *h == hit).map(|(_, r)| *r)
    }
}

fn tool_w(label: f32, ui: f32) -> f32 {
    (TOOL_PAD * 2.0 + ICON + ICON_GAP) * ui + label
}

fn button_w(label: f32, ui: f32) -> f32 {
    label + 2.0 * BTN_PAD * ui
}

/// The tools row in order: the five item tools, then Putar and Hapus
/// halaman after a divider.
const ROW: [Hit; 7] = [
    Hit::Tool(Tool::Signature),
    Hit::Tool(Tool::Initials),
    Hit::Tool(Tool::Date),
    Hit::Tool(Tool::Text),
    Hit::Tool(Tool::Check),
    Hit::Rotate,
    Hit::DeletePage,
];

pub fn layout(s: &Spec, labels: &Labels) -> Layout {
    let ui = s.ui;
    let (cw, ch) = (s.cw.max(1.0), s.ch.max(1.0));
    let title = Rect { x: 0.0, y: 0.0, w: cw, h: TITLE_H * ui };
    let tools = Rect { x: 0.0, y: title.bottom(), w: cw, h: TOOLS_H * ui };
    let banner = s.banner.then(|| Rect { x: 0.0, y: tools.bottom(), w: cw, h: BANNER_H * ui });
    let top = banner.map_or(tools.bottom(), |b| b.bottom());
    let foot_h = FOOT_H * ui;
    let footer = Rect { x: 0.0, y: (ch - foot_h).max(top), w: cw, h: foot_h };
    let body_h = (footer.y - top).max(0.0);
    let rail = Rect { x: 0.0, y: top, w: (RAIL_W * ui).min(cw), h: body_h };
    let stage = Rect { x: rail.right(), y: top, w: (cw - rail.w).max(0.0), h: body_h };
    let mut buttons = Vec::new();

    // Tools row: as many as fit, in order; the rest go to "…".
    let cy = tools.y + tools.h / 2.0;
    let at = |x: f32, w: f32, h: f32| Rect { x, y: cy - h / 2.0, w, h };
    let widths: Vec<f32> = ROW.iter().map(|h| tool_w(labels.w(*h), ui)).collect();
    let limit = cw - EDGE * ui;
    let full: f32 = EDGE * ui + widths.iter().sum::<f32>() + TOOL_GAP * ui * 6.0 + DIVIDER * ui;
    let mut overflow = Vec::new();
    let mut divider = None;
    let mut x = EDGE * ui;
    let room = if full <= limit { limit } else { limit - MORE_W * ui - TOOL_GAP * ui };
    for (i, hit) in ROW.iter().enumerate() {
        if i == 5 {
            if x + DIVIDER * ui + widths[i] > room {
                overflow.extend_from_slice(&ROW[i..]);
                break;
            }
            divider = Some(x + (DIVIDER * ui) / 2.0 - TOOL_GAP * ui / 2.0);
            x += DIVIDER * ui;
        }
        if !overflow.is_empty() || x + widths[i] > room {
            overflow.push(*hit);
            continue;
        }
        buttons.push((*hit, at(x, widths[i], TOOL_H * ui)));
        x += widths[i] + TOOL_GAP * ui;
    }
    if !overflow.is_empty() {
        buttons.push((Hit::More, at(x, MORE_W * ui, TOOL_H * ui)));
    }

    // Footer: buttons from the right edge in.
    let fy = footer.y + (footer.h - BTN_H * ui) / 2.0;
    let fbtn = |x: f32, w: f32| Rect { x, y: fy, w, h: BTN_H * ui };
    let mut right = cw - EDGE * ui;
    let mut push_right = |hit: Hit, buttons: &mut Vec<(Hit, Rect)>| {
        let w = button_w(labels.w(hit), ui);
        buttons.push((hit, fbtn(right - w, w)));
        right -= w + BTN_GAP * ui;
    };
    match s.footer {
        Footer::Normal { formats } => {
            push_right(Hit::Save, &mut buttons);
            push_right(Hit::Cancel, &mut buttons);
            if formats {
                right -= 8.0 * ui;
                let pdf = labels.w(Hit::FormatPdf) + 2.0 * SEG_PAD * ui;
                let img = labels.w(Hit::FormatImage) + 2.0 * SEG_PAD * ui;
                buttons.push((Hit::FormatPdf, fbtn(right - pdf, pdf)));
                buttons.push((Hit::FormatImage, fbtn(right - pdf - img, img)));
                right -= pdf + img + BTN_GAP * ui;
            }
        }
        Footer::CancelOnly => push_right(Hit::Cancel, &mut buttons),
        Footer::Empty => {}
        Footer::AskDiscard => {
            push_right(Hit::Keep, &mut buttons);
            push_right(Hit::Discard, &mut buttons);
        }
        Footer::AskBreaks => {
            push_right(Hit::NotNow, &mut buttons);
            push_right(Hit::SaveAnyway, &mut buttons);
        }
    }
    let foot_text = Rect {
        x: EDGE * ui + 2.0 * ui,
        y: footer.y,
        w: (right - EDGE * ui - 2.0 * ui).max(0.0),
        h: footer.h,
    };

    // Stage contents when there is no page.
    let line_h = 24.0 * ui;
    let mid = stage.y + stage.h / 2.0;
    let message = Rect {
        x: stage.x + PAGE_MARGIN * ui,
        y: mid - line_h - 8.0 * ui,
        w: (stage.w - 2.0 * PAGE_MARGIN * ui).max(0.0),
        h: line_h,
    };
    let mut field = None;
    match s.stage {
        StageMode::Password => {
            let bw = button_w(labels.w(Hit::OpenPassword), ui);
            let fw = (FIELD_W * ui).min((stage.w - bw - 2.0 * PAGE_MARGIN * ui - BTN_GAP * ui).max(40.0 * ui));
            let total = fw + BTN_GAP * ui + bw;
            let x0 = stage.x + (stage.w - total) / 2.0;
            let y0 = mid + 4.0 * ui;
            field = Some(Rect { x: x0, y: y0, w: fw, h: FIELD_H * ui });
            buttons.push((Hit::OpenPassword, Rect { x: x0 + fw + BTN_GAP * ui, y: y0, w: bw, h: FIELD_H * ui }));
        }
        StageMode::Error => {
            let bw = button_w(labels.w(Hit::CloseWindow), ui);
            buttons.push((
                Hit::CloseWindow,
                Rect { x: stage.x + (stage.w - bw) / 2.0, y: mid + 4.0 * ui, w: bw, h: BTN_H * ui },
            ));
        }
        StageMode::Page | StageMode::Message => {}
    }
    Layout { title, tools, banner, rail, stage, footer, foot_text, buttons, overflow, divider, message, field }
}

/// The page at the largest scale that shows it whole inside the stage with
/// `PAGE_MARGIN` around it: its rect and the scale (px per point).
pub fn fit_page(stage: Rect, shown: (f32, f32), ui: f32) -> (Rect, f32) {
    let m = PAGE_MARGIN * ui;
    let (sw, sh) = (shown.0.max(1.0), shown.1.max(1.0));
    let scale = ((stage.w - 2.0 * m) / sw).min((stage.h - 2.0 * m) / sh).max(0.01);
    let (w, h) = (sw * scale, sh * scale);
    (Rect { x: stage.x + (stage.w - w) / 2.0, y: stage.y + (stage.h - h) / 2.0, w, h }, scale)
}

/// One row of the rail: the thumbnail and the page number line under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailRow {
    pub page: usize,
    pub thumb: Rect,
    pub label: Rect,
}

/// The rail's rows for `pages` (session index and shown size), scrolled up
/// by `scroll` px.
pub fn rail_rows(rail: Rect, pages: &[(usize, (f32, f32))], ui: f32, scroll: f32) -> Vec<RailRow> {
    let tw = THUMB_W * ui;
    let x = rail.x + (rail.w - tw) / 2.0;
    let mut y = rail.y + RAIL_GAP * ui - scroll;
    pages
        .iter()
        .map(|&(page, (sw, sh))| {
            let th = (tw * sh.max(1.0) / sw.max(1.0)).min(tw * THUMB_MAX_ASPECT).max(8.0 * ui);
            let thumb = Rect { x, y, w: tw, h: th };
            let label = Rect { x: rail.x, y: y + th + 2.0 * ui, w: rail.w, h: RAIL_LABEL * ui };
            y += th + 2.0 * ui + RAIL_LABEL * ui + RAIL_GAP * ui;
            RailRow { page, thumb, label }
        })
        .collect()
}

/// Height of everything in the rail, unscrolled.
pub fn rail_content_h(rows: &[RailRow], rail: Rect, ui: f32, scroll: f32) -> f32 {
    rows.last().map_or(0.0, |r| r.label.bottom() + scroll + RAIL_GAP * ui - rail.y)
}

/// The scroll that keeps the row of `current` wholly in view, changing
/// `scroll` as little as possible, within `0..=content - rail height`.
pub fn rail_scroll_to(rows: &[RailRow], rail: Rect, current: usize, ui: f32, scroll: f32) -> f32 {
    let max = (rail_content_h(rows, rail, ui, scroll) - rail.h).max(0.0);
    let Some(row) = rows.iter().find(|r| r.page == current) else {
        return scroll.clamp(0.0, max);
    };
    let pad = RAIL_GAP * ui;
    let top = row.thumb.y + scroll - rail.y - pad;
    let bottom = row.label.bottom() + scroll - rail.y + pad;
    let mut s = scroll;
    if top < s {
        s = top;
    } else if bottom > s + rail.h {
        s = bottom - rail.h;
    }
    s.clamp(0.0, max)
}

/// A floating group of buttons: the item bar, the "…" menu, the picker.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Popover {
    pub panel: Rect,
    pub items: Vec<(Hit, Rect)>,
}

impl Popover {
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if !self.panel.contains(x, y) {
            return None;
        }
        Some(self.items.iter().find(|(_, r)| r.contains(x, y)).map_or(Hit::Inert, |(h, _)| *h))
    }
}

/// The bar over a selected item (under it when there is no room above),
/// kept inside the stage.
pub fn float_bar(item: Rect, stage: Rect, hits: &[Hit], labels: &Labels, ui: f32) -> Popover {
    let pad = FLOAT_PAD * ui;
    let h = FLOAT_H * ui;
    let widths: Vec<f32> = hits.iter().map(|hit| labels.w(*hit) + 2.0 * FLOAT_ITEM_PAD * ui).collect();
    let w = widths.iter().sum::<f32>() + pad * 2.0 + 2.0 * ui * (hits.len().saturating_sub(1)) as f32;
    let ph = h + 2.0 * pad;
    let above = item.y - FLOAT_GAP * ui - ph;
    let y = if above >= stage.y + 4.0 * ui {
        above
    } else {
        (item.bottom() + FLOAT_GAP * ui).min(stage.bottom() - ph - 4.0 * ui).max(stage.y)
    };
    let x = item.x.min(stage.right() - w - 4.0 * ui).max(stage.x + 4.0 * ui);
    let panel = Rect { x, y, w, h: ph };
    let mut items = Vec::new();
    let mut bx = x + pad;
    for (hit, bw) in hits.iter().zip(widths) {
        items.push((*hit, Rect { x: bx, y: y + pad, w: bw, h }));
        bx += bw + 2.0 * ui;
    }
    Popover { panel, items }
}

/// The tools that did not fit, as a menu under the "…" button, kept inside
/// the window.
pub fn more_menu(more: Rect, entries: &[Hit], cw: f32, ui: f32) -> Popover {
    let pad = POP_PAD * ui / 2.0;
    let w = MENU_W * ui;
    let x = (more.right() - w).max(4.0 * ui).min((cw - w - 4.0 * ui).max(0.0));
    let y = more.bottom() + 4.0 * ui;
    let items = entries
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            (*hit, Rect { x: x + pad, y: y + pad + i as f32 * MENU_ROW * ui, w: w - 2.0 * pad, h: MENU_ROW * ui })
        })
        .collect();
    let panel = Rect { x, y, w, h: entries.len() as f32 * MENU_ROW * ui + 2.0 * pad };
    Popover { panel, items }
}

/// The saved-signature picker under its tool button: one tile per saved
/// mark, then "+ Baru".
pub fn picker(anchor: Rect, marks: usize, cw: f32, ui: f32) -> Popover {
    let pad = POP_PAD * ui;
    let (tw, th, gap) = (TILE_W * ui, TILE_H * ui, 6.0 * ui);
    let n = marks + 1;
    let w = pad * 2.0 + tw * n as f32 + gap * (n - 1) as f32;
    let x = anchor.x.min((cw - w - 4.0 * ui).max(0.0)).max(4.0 * ui);
    let y = anchor.bottom() + 4.0 * ui;
    let mut items = Vec::new();
    for i in 0..n {
        let r = Rect { x: x + pad + i as f32 * (tw + gap), y: y + pad, w: tw, h: th };
        items.push((if i < marks { Hit::PickMark(i) } else { Hit::PickNew }, r));
    }
    Popover { panel: Rect { x, y, w, h: th + 2.0 * pad }, items }
}

/// The four handles of a selected item: the drawn square and its larger
/// hit area, both centred on the corner.
pub fn handles(item: Rect, ui: f32) -> [(Corner, Rect, Rect); 4] {
    Corner::ALL.map(|c| {
        let cx = if c.left() { item.x } else { item.right() };
        let cy = if c.top() { item.y } else { item.bottom() };
        let sq = |s: f32| Rect { x: cx - s * ui / 2.0, y: cy - s * ui / 2.0, w: s * ui, h: s * ui };
        (c, sq(HANDLE), sq(HANDLE_HIT))
    })
}

/// The view corner a resize keeps still: the one opposite `corner`.
pub fn anchor_of(item: Rect, corner: Corner) -> (f32, f32) {
    (if corner.left() { item.right() } else { item.x }, if corner.top() { item.bottom() } else { item.y })
}

/// How much a corner drag to `pointer` scales an item shown at `item`: the
/// larger of the two sides' factors, so the corner follows the pointer
/// while the aspect stays.
pub fn resize_factor(item: Rect, corner: Corner, pointer: (f32, f32)) -> f32 {
    let (ax, ay) = anchor_of(item, corner);
    let fx = (pointer.0 - ax).abs() / item.w.max(1.0);
    let fy = (pointer.1 - ay).abs() / item.h.max(1.0);
    fx.max(fy).max(0.01)
}

/// The rect of a `w` x `h` view box held at `anchor` by its `corner`'s
/// opposite.
pub fn anchored(anchor: (f32, f32), corner: Corner, w: f32, h: f32) -> Rect {
    Rect {
        x: if corner.left() { anchor.0 - w } else { anchor.0 },
        y: if corner.top() { anchor.1 - h } else { anchor.1 },
        w,
        h,
    }
}

/// What the pointer is over, topmost first: an open popover, the chrome's
/// buttons, the rail, the item bar, the selected item's handles, the items
/// (last drawn first), the page, the stage.
pub struct Scene<'a> {
    pub layout: &'a Layout,
    pub popover: Option<&'a Popover>,
    pub float: Option<&'a Popover>,
    pub handles: &'a [(Corner, Rect)],
    /// Item index and view rect, in drawing order.
    pub items: &'a [(usize, Rect)],
    pub page: Option<Rect>,
    pub rail: &'a [RailRow],
}

pub fn hit_test(s: &Scene, x: f32, y: f32) -> Option<Hit> {
    if let Some(h) = s.popover.and_then(|p| p.hit(x, y)) {
        return Some(h);
    }
    if let Some((h, _)) = s.layout.buttons.iter().find(|(_, r)| r.contains(x, y)) {
        return Some(*h);
    }
    if s.layout.rail.contains(x, y) {
        return s
            .rail
            .iter()
            .find(|r| r.thumb.contains(x, y) || r.label.contains(x, y))
            .map(|r| Hit::Thumb(r.page));
    }
    if !s.layout.stage.contains(x, y) {
        return None;
    }
    if let Some(h) = s.float.and_then(|p| p.hit(x, y)) {
        return Some(h);
    }
    if let Some((c, _)) = s.handles.iter().find(|(_, r)| r.contains(x, y)) {
        return Some(Hit::Handle(*c));
    }
    if let Some((i, _)) = s.items.iter().rev().find(|(_, r)| r.contains(x, y)) {
        return Some(Hit::Item(*i));
    }
    if s.page.is_some_and(|p| p.contains(x, y)) {
        return Some(Hit::Page);
    }
    Some(Hit::Stage)
}

/// The keyboard order of the chrome: tools row (with "…"), stage button,
/// footer, as laid out left to right.
pub fn focus_order(l: &Layout) -> Vec<Hit> {
    let mut out: Vec<Hit> = Vec::new();
    let row = |r: &Rect| (r.y / 4.0).round() as i64;
    let mut all: Vec<(Hit, Rect)> = l.buttons.clone();
    all.sort_by(|a, b| row(&a.1).cmp(&row(&b.1)).then(a.1.x.total_cmp(&b.1.x)));
    out.extend(all.into_iter().map(|(h, _)| h));
    out
}

/// Where a new item of page size `w` x `h` (upright frame) goes when it is
/// placed centred on the page point `at`, turned `turn`: its lower-left
/// corner, kept inside the `pw` x `ph` page.
pub fn place_centred(at: (f32, f32), w: f32, h: f32, turn: Quarter, pw: f32, ph: f32) -> (f32, f32) {
    let (bw, bh) = if matches!(turn, Quarter::R90 | Quarter::R270) { (h, w) } else { (w, h) };
    clamp_into(at.0 - bw / 2.0, at.1 - bh / 2.0, bw, bh, pw, ph)
}

/// `(x, y)` moved the least so a `bw` x `bh` box stays on the page (pinned to
/// the lower-left when it is bigger than the page).
pub fn clamp_into(x: f32, y: f32, bw: f32, bh: f32, pw: f32, ph: f32) -> (f32, f32) {
    (x.min(pw - bw).max(0.0), y.min(ph - bh).max(0.0))
}

/// Lower-left corner (page space) of a `dw` x `dh` item that sits `gap` pt
/// under `sig` in `sig`'s own upright frame, left edges aligned, with the
/// same turn.
pub fn below(sig: &Item, dw: f32, dh: f32, gap: f32) -> (f32, f32) {
    let m = geom::item_matrix(sig);
    let to_page = |u: f32, v: f32| {
        let (s, t) = (u / sig.w.max(0.01), v / sig.h.max(0.01));
        (m[0] * s + m[2] * t + m[4], m[1] * s + m[3] * t + m[5])
    };
    let a = to_page(0.0, -gap - dh);
    let b = to_page(dw, -gap);
    (a.0.min(b.0), a.1.min(b.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, text};
    use crate::sign::ItemKind;

    /// Label widths of the Indonesian copy at about 7 px a character, which
    /// is what 13 px DM Sans measures.
    fn labels(ui: f32) -> Labels {
        Labels(
            LABELLED
                .iter()
                .map(|h| (*h, text(Lang::Id, label_key(*h).unwrap()).chars().count() as f32 * 7.0 * ui))
                .collect(),
        )
    }

    fn spec(cw: f32, ch: f32, ui: f32) -> Spec {
        Spec { cw, ch, ui, banner: false, footer: Footer::Normal { formats: false }, stage: StageMode::Page }
    }

    fn inside(inner: Rect, outer: Rect, margin: f32) -> bool {
        inner.x >= outer.x + margin - 0.01
            && inner.y >= outer.y + margin - 0.01
            && inner.right() <= outer.right() - margin + 0.01
            && inner.bottom() <= outer.bottom() - margin + 0.01
    }

    #[test]
    fn layout_fits_page_in_stage() {
        let l = layout(&spec(1100.0, 760.0, 1.0), &labels(1.0));
        let (page, scale) = fit_page(l.stage, (612.0, 792.0), 1.0);
        assert!(inside(page, l.stage, PAGE_MARGIN), "{page:?} in {:?}", l.stage);
        assert!(((page.w / page.h) - 612.0 / 792.0).abs() < 1e-3);
        assert!((page.w - 612.0 * scale).abs() < 0.01);
        // Tall page: the height is what limits it.
        assert!((page.h - (l.stage.h - 2.0 * PAGE_MARGIN)).abs() < 0.5);
        // The regions stack: title, tools, body (rail | stage), footer.
        assert_eq!(l.tools.y, l.title.bottom());
        assert_eq!(l.stage.y, l.tools.bottom());
        assert_eq!(l.rail.w, RAIL_W);
        assert_eq!(l.stage.x, l.rail.right());
        assert!((l.footer.bottom() - 760.0).abs() < 0.01);
        assert_eq!(l.footer.y, l.stage.bottom());
    }

    #[test]
    fn rotated_page_fits_too() {
        let l = layout(&spec(1100.0, 760.0, 1.0), &labels(1.0));
        let (page, _) = fit_page(l.stage, (792.0, 612.0), 1.0);
        assert!(inside(page, l.stage, PAGE_MARGIN), "{page:?}");
        assert!(((page.w / page.h) - 792.0 / 612.0).abs() < 1e-3);
        // At 2x the same holds in physical px.
        let l2 = layout(&spec(2200.0, 1520.0, 2.0), &labels(2.0));
        let (page2, _) = fit_page(l2.stage, (792.0, 612.0), 2.0);
        assert!(inside(page2, l2.stage, PAGE_MARGIN * 2.0));
        // A banner takes from the stage, never overlaps it.
        let mut s = spec(1100.0, 760.0, 1.0);
        s.banner = true;
        let lb = layout(&s, &labels(1.0));
        let b = lb.banner.unwrap();
        assert_eq!(b.y, lb.tools.bottom());
        assert_eq!(lb.stage.y, b.bottom());
    }

    #[test]
    fn hit_test_order() {
        let l = layout(&spec(1100.0, 760.0, 1.0), &labels(1.0));
        let (page, _) = fit_page(l.stage, (612.0, 792.0), 1.0);
        let item = Rect { x: page.x + 100.0, y: page.y + 300.0, w: 150.0, h: 50.0 };
        let under = Rect { x: page.x + 120.0, y: page.y + 310.0, w: 60.0, h: 30.0 };
        let hs: Vec<(Corner, Rect)> = handles(item, 1.0).iter().map(|(c, _, hit)| (*c, *hit)).collect();
        let bar = float_bar(item, l.stage, &[Hit::Smaller, Hit::Bigger, Hit::Remove], &labels(1.0), 1.0);
        let items = vec![(0, under), (1, item)];
        let scene = Scene {
            layout: &l,
            popover: None,
            float: Some(&bar),
            handles: &hs,
            items: &items,
            page: Some(page),
            rail: &[],
        };
        // A handle wins over the item body it overlaps.
        assert_eq!(hit_test(&scene, item.x + 1.0, item.y + 1.0), Some(Hit::Handle(Corner::TopLeft)));
        assert_eq!(
            hit_test(&scene, item.right() - 1.0, item.bottom() - 1.0),
            Some(Hit::Handle(Corner::BottomRight))
        );
        // The topmost (last drawn) item wins over one under it.
        assert_eq!(hit_test(&scene, item.x + 40.0, item.y + 20.0), Some(Hit::Item(1)));
        // Then the page, then the stage.
        assert_eq!(hit_test(&scene, page.x + 10.0, page.y + 10.0), Some(Hit::Page));
        assert_eq!(hit_test(&scene, l.stage.x + 2.0, l.stage.bottom() - 2.0), Some(Hit::Stage));
        // The bar wins over an item under it.
        let (h, r) = bar.items[0];
        let over = vec![(5, Rect { x: r.x - 20.0, y: r.y - 20.0, w: 200.0, h: 80.0 })];
        let scene2 = Scene { items: &over, ..scene };
        assert_eq!(hit_test(&scene2, r.x + 2.0, r.y + 2.0), Some(h));
        // Its background takes the click but does nothing.
        assert_eq!(hit_test(&scene2, bar.panel.x + 1.0, bar.panel.y + 1.0), Some(Hit::Inert));
        // A chrome button is found; outside everything is nothing.
        let save = l.rect_of(Hit::Save).unwrap();
        assert_eq!(hit_test(&scene, save.x + 2.0, save.y + 2.0), Some(Hit::Save));
        assert_eq!(hit_test(&scene, 2.0, 2.0), None);
        // An open popover wins over everything.
        let menu = more_menu(save, &[Hit::Rotate], 1100.0, 1.0);
        let scene3 = Scene { popover: Some(&menu), ..scene };
        let (mh, mr) = menu.items[0];
        assert_eq!(hit_test(&scene3, mr.x + 3.0, mr.y + 3.0), Some(mh));
    }

    #[test]
    fn rail_scrolls_to_current_page() {
        let l = layout(&spec(1100.0, 760.0, 1.0), &labels(1.0));
        let pages: Vec<(usize, (f32, f32))> = (0..20).map(|i| (i, (612.0, 792.0))).collect();
        let rows = rail_rows(l.rail, &pages, 1.0, 0.0);
        // Rows run down without overlap, thumbnails 74 wide and centred.
        for w in rows.windows(2) {
            assert!(w[1].thumb.y > w[0].label.bottom());
        }
        assert_eq!(rows[0].thumb.w, THUMB_W);
        assert!((rows[0].thumb.x - (l.rail.x + (RAIL_W - THUMB_W) / 2.0)).abs() < 0.01);
        // Page 15 is below the rail: the scroll brings it fully in view.
        let s = rail_scroll_to(&rows, l.rail, 15, 1.0, 0.0);
        let moved = rail_rows(l.rail, &pages, 1.0, s);
        let r = moved.iter().find(|r| r.page == 15).unwrap();
        assert!(r.thumb.y >= l.rail.y && r.label.bottom() <= l.rail.bottom(), "{r:?} in {:?}", l.rail);
        // Back to page 0: scroll returns to the top.
        assert_eq!(rail_scroll_to(&moved, l.rail, 0, 1.0, s), 0.0);
        // A page already in view keeps the scroll.
        assert_eq!(rail_scroll_to(&rows, l.rail, 1, 1.0, 0.0), 0.0);
        // Never past the end.
        let end = rail_scroll_to(&rows, l.rail, 19, 1.0, 0.0);
        let max = rail_content_h(&rows, l.rail, 1.0, 0.0) - l.rail.h;
        assert!(end <= max + 0.01 && end > 0.0);
    }

    #[test]
    fn narrow_window_keeps_tools_reachable() {
        let lab = labels(1.0);
        let wide = layout(&spec(1100.0, 760.0, 1.0), &lab);
        assert!(wide.overflow.is_empty());
        assert!(wide.rect_of(Hit::More).is_none());
        assert!(wide.divider.is_some());
        let narrow = layout(&spec(720.0, 520.0, 1.0), &lab);
        assert!(!narrow.overflow.is_empty(), "at 720 the row is too long for every tool");
        let more = narrow.rect_of(Hit::More).expect("a … button");
        assert!(more.right() <= 720.0 - EDGE + 0.01);
        // Every tool is either a button inside the window or in the menu.
        for hit in ROW {
            match narrow.rect_of(hit) {
                Some(r) => assert!(r.right() <= more.x, "{hit:?} clipped"),
                None => assert!(narrow.overflow.contains(&hit), "{hit:?} lost"),
            }
        }
        // Order is kept: what overflows is the tail.
        let shown = ROW.iter().filter(|h| narrow.rect_of(**h).is_some()).count();
        assert_eq!(&ROW[shown..], narrow.overflow.as_slice());
        // The menu opens inside the window.
        let menu = more_menu(more, &narrow.overflow, 720.0, 1.0);
        assert!(menu.panel.x >= 0.0 && menu.panel.right() <= 720.0);
        assert_eq!(menu.items.len(), narrow.overflow.len());
        // Footer buttons do not overlap the output line.
        let cancel = narrow.rect_of(Hit::Cancel).unwrap();
        assert!(narrow.foot_text.right() <= cancel.x);
    }

    #[test]
    fn footer_modes_hold_their_buttons() {
        let lab = labels(1.0);
        let mut s = spec(1100.0, 760.0, 1.0);
        s.footer = Footer::Normal { formats: true };
        let l = layout(&s, &lab);
        let (img, pdf, cancel, save) = (
            l.rect_of(Hit::FormatImage).unwrap(),
            l.rect_of(Hit::FormatPdf).unwrap(),
            l.rect_of(Hit::Cancel).unwrap(),
            l.rect_of(Hit::Save).unwrap(),
        );
        assert!(img.right() <= pdf.x + 0.01 && pdf.right() < cancel.x && cancel.right() < save.x);
        assert!(save.right() <= 1100.0);
        s.footer = Footer::AskDiscard;
        let l = layout(&s, &lab);
        assert!(l.rect_of(Hit::Discard).unwrap().right() < l.rect_of(Hit::Keep).unwrap().x);
        assert!(l.rect_of(Hit::Save).is_none());
        s.footer = Footer::AskBreaks;
        let l = layout(&s, &lab);
        assert!(l.rect_of(Hit::SaveAnyway).is_some() && l.rect_of(Hit::NotNow).is_some());
        s.footer = Footer::CancelOnly;
        s.stage = StageMode::Password;
        let l = layout(&s, &lab);
        let field = l.field.unwrap();
        let open = l.rect_of(Hit::OpenPassword).unwrap();
        assert!(field.right() < open.x && inside(field, l.stage, 0.0));
        assert!(l.rect_of(Hit::Save).is_none());
        s.stage = StageMode::Error;
        let l = layout(&s, &lab);
        assert!(inside(l.rect_of(Hit::CloseWindow).unwrap(), l.stage, 0.0));
    }

    #[test]
    fn corner_drag_keeps_aspect_and_anchor() {
        let r = Rect { x: 100.0, y: 100.0, w: 150.0, h: 50.0 };
        assert_eq!(anchor_of(r, Corner::BottomRight), (100.0, 100.0));
        assert_eq!(anchor_of(r, Corner::TopLeft), (250.0, 150.0));
        // Dragging the bottom-right corner 150 px right doubles it.
        assert!((resize_factor(r, Corner::BottomRight, (400.0, 150.0)) - 2.0).abs() < 1e-4);
        // The larger side's factor wins.
        assert!((resize_factor(r, Corner::BottomRight, (250.0, 200.0)) - 2.0).abs() < 1e-4);
        let a = anchored(anchor_of(r, Corner::TopLeft), Corner::TopLeft, 300.0, 100.0);
        assert_eq!((a.right(), a.bottom()), (250.0, 150.0));
    }

    #[test]
    fn handles_sit_on_the_corners() {
        let r = Rect { x: 10.0, y: 20.0, w: 100.0, h: 40.0 };
        let hs = handles(r, 1.0);
        for (c, drawn, hit) in hs {
            let (cx, cy) = (drawn.x + drawn.w / 2.0, drawn.y + drawn.h / 2.0);
            let (hx, hy) = (hit.x + hit.w / 2.0, hit.y + hit.h / 2.0);
            assert_eq!((cx, cy), (hx, hy));
            assert!(hit.w > drawn.w);
            let want = (if c.left() { 10.0 } else { 110.0 }, if c.top() { 20.0 } else { 60.0 });
            assert_eq!((cx, cy), want);
        }
    }

    #[test]
    fn float_bar_stays_in_the_stage() {
        let stage = Rect { x: 132.0, y: 88.0, w: 968.0, h: 616.0 };
        let lab = labels(1.0);
        let hits = [Hit::Smaller, Hit::Bigger, Hit::AddDate, Hit::Remove];
        // Room above: above the item.
        let item = Rect { x: 600.0, y: 400.0, w: 150.0, h: 50.0 };
        let bar = float_bar(item, stage, &hits, &lab, 1.0);
        assert!(bar.panel.bottom() <= item.y);
        // At the top of the stage: under it.
        let top = Rect { x: 600.0, y: 95.0, w: 150.0, h: 50.0 };
        let bar = float_bar(top, stage, &hits, &lab, 1.0);
        assert!(bar.panel.y >= top.bottom());
        // At the right edge: pulled in.
        let right = Rect { x: 1050.0, y: 400.0, w: 40.0, h: 20.0 };
        let bar = float_bar(right, stage, &hits, &lab, 1.0);
        assert!(bar.panel.right() <= stage.right());
        assert_eq!(bar.items.len(), 4);
    }

    #[test]
    fn picker_has_a_tile_per_mark_and_new() {
        let anchor = Rect { x: 12.0, y: 47.0, w: 120.0, h: 34.0 };
        let p = picker(anchor, 2, 1100.0, 1.0);
        let hits: Vec<Hit> = p.items.iter().map(|(h, _)| *h).collect();
        assert_eq!(hits, vec![Hit::PickMark(0), Hit::PickMark(1), Hit::PickNew]);
        assert!(p.panel.y >= anchor.bottom());
        for (_, r) in &p.items {
            assert!(inside(*r, p.panel, 0.0));
        }
    }

    #[test]
    fn new_items_land_centred_and_on_the_page() {
        assert_eq!(place_centred((300.0, 400.0), 150.0, 50.0, Quarter::R0, 612.0, 792.0), (225.0, 375.0));
        // Turned a quarter, its page box is 50 x 150.
        assert_eq!(place_centred((300.0, 400.0), 150.0, 50.0, Quarter::R90, 612.0, 792.0), (275.0, 325.0));
        // Near the edge it is pulled in.
        assert_eq!(place_centred((5.0, 790.0), 150.0, 50.0, Quarter::R0, 612.0, 792.0), (0.0, 742.0));
    }

    #[test]
    fn a_date_goes_under_its_signature_in_its_own_frame() {
        let sig = |turn| Item {
            kind: ItemKind::Check,
            page: 0,
            x: 100.0,
            y: 200.0,
            w: 150.0,
            h: 50.0,
            turn,
        };
        // Upright: the date's box is under the signature, left aligned.
        assert_eq!(below(&sig(Quarter::R0), 110.0, 14.0, 4.0), (100.0, 182.0));
        // Turned a quarter (CCW): "under" is towards +x in page space.
        let (x, y) = below(&sig(Quarter::R90), 110.0, 14.0, 4.0);
        assert!((x - 154.0).abs() < 1e-3 && (y - 200.0).abs() < 1e-3, "{x} {y}");
    }

    #[test]
    fn every_labelled_hit_has_words_and_the_tab_order_reads_left_to_right() {
        for h in LABELLED {
            assert!(label_key(h).is_some(), "{h:?}");
        }
        let l = layout(&spec(1100.0, 760.0, 1.0), &labels(1.0));
        let order = focus_order(&l);
        assert_eq!(order.first(), Some(&Hit::Tool(Tool::Signature)));
        assert_eq!(order.last(), Some(&Hit::Save));
        let pos = |h: Hit| order.iter().position(|x| *x == h).unwrap();
        assert!(pos(Hit::Tool(Tool::Check)) < pos(Hit::Rotate));
        assert!(pos(Hit::Cancel) < pos(Hit::Save));
    }
}
