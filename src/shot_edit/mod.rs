//! The screenshot editor opened straight after a capture: the pure part
//! (annotations, hit testing, moving and resizing, undo and redo, the view
//! mapping, pixelating). `window` is the window, `paint` draws the
//! annotations (on screen and in the export alike), `export` renders at full
//! resolution, saves and copies, `auto` decides when the picture is copied
//! by itself.
//!
//! Everything lives in image pixels. The picture and its annotations exist
//! only while the window is open. Nothing reaches the disk without Save. The
//! clipboard gets the picture on Copy, and (with "Salin otomatis", on by
//! default) by itself: when the editor opens and after each finished edit,
//! only while the clipboard still holds what this editor wrote.

pub mod auto;
pub mod export;
pub mod gfx;
pub mod paint;
pub mod window;

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// A point in image pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub const fn new(x: f32, y: f32) -> Pt {
        Pt { x, y }
    }
}

/// A rectangle in image pixels, always with a positive size once made by
/// `FRect::between`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl FRect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> FRect {
        FRect { x, y, w, h }
    }

    /// The rectangle with `a` and `b` as opposite corners, in any order.
    pub fn between(a: Pt, b: Pt) -> FRect {
        FRect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            w: (a.x - b.x).abs(),
            h: (a.y - b.y).abs(),
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, p: Pt) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }

    pub fn grow(&self, by: f32) -> FRect {
        FRect {
            x: self.x - by,
            y: self.y - by,
            w: self.w + 2.0 * by,
            h: self.h + 2.0 * by,
        }
    }

    fn corner(&self, h: Handle) -> Pt {
        match h {
            Handle::Nw => Pt::new(self.x, self.y),
            Handle::Ne => Pt::new(self.right(), self.y),
            Handle::Sw => Pt::new(self.x, self.bottom()),
            _ => Pt::new(self.right(), self.bottom()),
        }
    }
}

/// How an outline or a pen looks: colour as `0xRRGGBB`, width in image px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub color: u32,
    pub width: f32,
}

/// One thing drawn over the picture.
#[derive(Clone, Debug, PartialEq)]
pub enum Annotation {
    Rect {
        r: FRect,
        style: Style,
    },
    Arrow {
        a: Pt,
        b: Pt,
        style: Style,
    },
    Pen {
        pts: Vec<Pt>,
        style: Style,
    },
    /// A highlighter: the same stroke, drawn wide and translucent.
    Marker {
        pts: Vec<Pt>,
        style: Style,
    },
    /// `pos` is the top-left of the text; lines are split at '\n'.
    Text {
        pos: Pt,
        text: String,
        size: f32,
        color: u32,
    },
    /// The part of the picture under it is blurred into blocks.
    Pixelate {
        r: FRect,
    },
}

/// The tools of the toolbar; Select is active when the editor opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    #[default]
    Select,
    Text,
    Rect,
    Arrow,
    Pen,
    Marker,
    Pixelate,
}

impl Tool {
    pub const ALL: [Tool; 7] = [
        Tool::Select,
        Tool::Text,
        Tool::Rect,
        Tool::Arrow,
        Tool::Pen,
        Tool::Marker,
        Tool::Pixelate,
    ];

    /// V, T, R, A, P, M, B (either case).
    pub fn from_key(c: char) -> Option<Tool> {
        Some(match c.to_ascii_uppercase() {
            'V' => Tool::Select,
            'T' => Tool::Text,
            'R' => Tool::Rect,
            'A' => Tool::Arrow,
            'P' => Tool::Pen,
            'M' => Tool::Marker,
            'B' => Tool::Pixelate,
            _ => return None,
        })
    }

    pub fn key(self) -> char {
        match self {
            Tool::Select => 'V',
            Tool::Text => 'T',
            Tool::Rect => 'R',
            Tool::Arrow => 'A',
            Tool::Pen => 'P',
            Tool::Marker => 'M',
            Tool::Pixelate => 'B',
        }
    }
}

/// The six colour swatches (`0xRRGGBB`): red first, the default.
pub const COLORS: [u32; 6] = [0xE5484D, 0xF5A524, 0x30A46C, 0x3E63DD, 0x1A1A1A, 0xFFFFFF];

/// The three stroke widths in image px (thin, medium, thick).
pub const STROKES: [f32; 3] = [3.0, 6.0, 12.0];

/// The text size in image px for stroke `i`.
pub fn text_size(i: usize) -> f32 {
    [22.0, 32.0, 48.0][i.min(2)]
}

/// The marker is this many times as wide as the stroke.
pub const MARKER_WIDTH_X: f32 = 3.5;

/// The marker's opacity.
pub const MARKER_ALPHA: f32 = 0.4;

/// What a drag on a selected annotation grabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Nw,
    Ne,
    Sw,
    Se,
    /// An arrow's tail and head.
    Start,
    End,
}

/// A rough size of `text` at `size` px: enough for hit testing and the
/// selection box (the drawing itself is laid out by DirectWrite).
pub fn text_extent(text: &str, size: f32) -> (f32, f32) {
    let lines = text.split('\n').count().max(1);
    let widest = text
        .split('\n')
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .max(1);
    (widest as f32 * size * 0.56, lines as f32 * size * 1.25)
}

impl Annotation {
    /// The box around it (stroke included), for the selection outline and
    /// for hit testing a text.
    pub fn bounds(&self) -> FRect {
        match self {
            Annotation::Rect { r, style } => r.grow(style.width / 2.0),
            Annotation::Pixelate { r } => *r,
            Annotation::Arrow { a, b, style } => {
                FRect::between(*a, *b).grow(style.width * 1.5 + 4.0)
            }
            Annotation::Pen { pts, style } => points_bounds(pts, style.width / 2.0),
            Annotation::Marker { pts, style } => {
                points_bounds(pts, style.width * MARKER_WIDTH_X / 2.0)
            }
            Annotation::Text {
                pos, text, size, ..
            } => {
                let (w, h) = text_extent(text, *size);
                FRect::new(pos.x, pos.y, w, h)
            }
        }
    }

    /// True when `p` is on it, within `tol` image px. An outline is hit on
    /// its line (so what is inside stays selectable), a pixelated area and
    /// a text anywhere inside.
    pub fn hit(&self, p: Pt, tol: f32) -> bool {
        match self {
            Annotation::Rect { r, style } => {
                let reach = tol + style.width / 2.0;
                r.grow(reach).contains(p) && !r.grow(-reach).contains(p)
            }
            Annotation::Pixelate { r } => r.grow(tol).contains(p),
            Annotation::Arrow { a, b, style } => dist_to_segment(p, *a, *b) <= tol + style.width,
            Annotation::Pen { pts, style } => near_polyline(pts, p, tol + style.width / 2.0),
            Annotation::Marker { pts, style } => {
                near_polyline(pts, p, tol + style.width * MARKER_WIDTH_X / 2.0)
            }
            Annotation::Text { .. } => self.bounds().grow(tol).contains(p),
        }
    }

    /// The handle of a selected annotation under `p`, if any.
    pub fn handle_at(&self, p: Pt, tol: f32) -> Option<Handle> {
        let near = |q: Pt| (p.x - q.x).abs() <= tol && (p.y - q.y).abs() <= tol;
        match self {
            Annotation::Rect { r, .. } | Annotation::Pixelate { r } => {
                [Handle::Nw, Handle::Ne, Handle::Sw, Handle::Se]
                    .into_iter()
                    .find(|h| near(r.corner(*h)))
            }
            Annotation::Arrow { a, b, .. } => {
                if near(*b) {
                    Some(Handle::End)
                } else if near(*a) {
                    Some(Handle::Start)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Where the handles of this annotation are.
    pub fn handles(&self) -> Vec<(Handle, Pt)> {
        match self {
            Annotation::Rect { r, .. } | Annotation::Pixelate { r } => {
                [Handle::Nw, Handle::Ne, Handle::Sw, Handle::Se]
                    .into_iter()
                    .map(|h| (h, r.corner(h)))
                    .collect()
            }
            Annotation::Arrow { a, b, .. } => vec![(Handle::Start, *a), (Handle::End, *b)],
            _ => Vec::new(),
        }
    }

    /// Moves it by `(dx, dy)`.
    pub fn move_by(&mut self, dx: f32, dy: f32) {
        let shift = |p: &mut Pt| {
            p.x += dx;
            p.y += dy;
        };
        match self {
            Annotation::Rect { r, .. } | Annotation::Pixelate { r } => {
                r.x += dx;
                r.y += dy;
            }
            Annotation::Arrow { a, b, .. } => {
                shift(a);
                shift(b);
            }
            Annotation::Pen { pts, .. } | Annotation::Marker { pts, .. } => {
                pts.iter_mut().for_each(shift);
            }
            Annotation::Text { pos, .. } => shift(pos),
        }
    }

    /// Drags `handle` to `to`; a rectangle keeps the opposite corner where
    /// it is (and may flip over it).
    pub fn resize(&mut self, handle: Handle, to: Pt) {
        match self {
            Annotation::Rect { r, .. } | Annotation::Pixelate { r } => {
                let anchor = r.corner(match handle {
                    Handle::Nw => Handle::Se,
                    Handle::Ne => Handle::Sw,
                    Handle::Sw => Handle::Ne,
                    _ => Handle::Nw,
                });
                *r = FRect::between(anchor, to);
            }
            Annotation::Arrow { a, b, .. } => match handle {
                Handle::Start => *a = to,
                Handle::End => *b = to,
                _ => {}
            },
            _ => {}
        }
    }

    /// A new colour (a pixelated area has none).
    pub fn set_color(&mut self, c: u32) {
        match self {
            Annotation::Rect { style, .. }
            | Annotation::Arrow { style, .. }
            | Annotation::Pen { style, .. }
            | Annotation::Marker { style, .. } => style.color = c,
            Annotation::Text { color, .. } => *color = c,
            Annotation::Pixelate { .. } => {}
        }
    }

    /// A new stroke: the width of a line, the size of a text.
    pub fn set_stroke(&mut self, width: f32, text: f32) {
        match self {
            Annotation::Rect { style, .. }
            | Annotation::Arrow { style, .. }
            | Annotation::Pen { style, .. }
            | Annotation::Marker { style, .. } => style.width = width,
            Annotation::Text { size, .. } => *size = text,
            Annotation::Pixelate { .. } => {}
        }
    }

    /// Its colour, if it has one.
    pub fn color(&self) -> Option<u32> {
        match self {
            Annotation::Rect { style, .. }
            | Annotation::Arrow { style, .. }
            | Annotation::Pen { style, .. }
            | Annotation::Marker { style, .. } => Some(style.color),
            Annotation::Text { color, .. } => Some(*color),
            Annotation::Pixelate { .. } => None,
        }
    }

    /// Too small to be worth keeping (a click with a drawing tool).
    pub fn is_degenerate(&self) -> bool {
        const MIN: f32 = 3.0;
        match self {
            Annotation::Rect { r, .. } | Annotation::Pixelate { r } => r.w < MIN || r.h < MIN,
            Annotation::Arrow { a, b, .. } => (a.x - b.x).abs() < MIN && (a.y - b.y).abs() < MIN,
            Annotation::Pen { pts, .. } | Annotation::Marker { pts, .. } => pts.len() < 2,
            Annotation::Text { text, .. } => text.trim().is_empty(),
        }
    }
}

fn points_bounds(pts: &[Pt], pad: f32) -> FRect {
    let Some(first) = pts.first() else {
        return FRect::default();
    };
    let (mut lo, mut hi) = (*first, *first);
    for p in pts {
        lo.x = lo.x.min(p.x);
        lo.y = lo.y.min(p.y);
        hi.x = hi.x.max(p.x);
        hi.y = hi.y.max(p.y);
    }
    FRect::between(lo, hi).grow(pad)
}

/// Distance from `p` to the segment `a`–`b`.
pub fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= f32::EPSILON {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.x + t * dx, a.y + t * dy);
    ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt()
}

fn near_polyline(pts: &[Pt], p: Pt, reach: f32) -> bool {
    match pts {
        [] => false,
        [only] => (p.x - only.x).hypot(p.y - only.y) <= reach,
        _ => pts
            .windows(2)
            .any(|s| dist_to_segment(p, s[0], s[1]) <= reach),
    }
}

/// The document and the editor's state around it: tool, colour, selection,
/// undo and redo (whole-document snapshots, taken around each action).
#[derive(Debug)]
pub struct Session {
    pub doc: Vec<Annotation>,
    pub selected: Option<usize>,
    pub tool: Tool,
    pub color: u32,
    /// Index into `STROKES`.
    pub stroke: usize,
    undo: Vec<Vec<Annotation>>,
    redo: Vec<Vec<Annotation>>,
    /// The document as it was when the running action began.
    pending: Option<Vec<Annotation>>,
    /// Anything changed since the last save or copy.
    changed: bool,
    /// Counts the changes, so a save that finishes later can tell whether
    /// the document it wrote is still the one shown.
    revision: u64,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            doc: Vec::new(),
            selected: None,
            tool: Tool::Select,
            color: COLORS[0],
            stroke: 1,
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
            changed: false,
            revision: 0,
        }
    }
}

impl Session {
    /// A session over annotations that are already there (the demo's);
    /// they are not an undoable change.
    pub fn with_doc(doc: Vec<Annotation>) -> Session {
        Session {
            doc,
            ..Session::default()
        }
    }

    pub fn style(&self) -> Style {
        Style {
            color: self.color,
            width: STROKES[self.stroke.min(2)],
        }
    }

    /// Starts one action (a drag, a text): everything until `end` is one
    /// undo step.
    pub fn begin(&mut self) {
        if self.pending.is_none() {
            self.pending = Some(self.doc.clone());
        }
    }

    /// Ends the action. An action that changed nothing leaves no step.
    /// True when the document changed.
    pub fn end(&mut self) -> bool {
        let Some(before) = self.pending.take() else {
            return false;
        };
        if before == self.doc {
            return false;
        }
        self.undo.push(before);
        self.redo.clear();
        self.touch();
        true
    }

    /// Adds `ann` as one step and returns its index. A degenerate one (an
    /// empty text, a click with the rectangle tool) is dropped: `None`.
    pub fn add(&mut self, ann: Annotation) -> Option<usize> {
        if ann.is_degenerate() {
            return None;
        }
        self.begin();
        self.doc.push(ann);
        self.end();
        Some(self.doc.len() - 1)
    }

    /// A text at `pos` in the current colour and size; an empty or blank
    /// one is dropped.
    pub fn add_text(&mut self, pos: Pt, text: &str) -> Option<usize> {
        self.add(Annotation::Text {
            pos,
            text: text.to_owned(),
            size: text_size(self.stroke),
            color: self.color,
        })
    }

    /// Replaces the text of annotation `i` (one step); a blank text removes
    /// it. True when something changed.
    pub fn set_text(&mut self, i: usize, new: &str) -> bool {
        let blank = new.trim().is_empty();
        if !matches!(self.doc.get(i), Some(Annotation::Text { .. })) {
            return false;
        }
        self.begin();
        if blank {
            self.doc.remove(i);
            self.selected = None;
        } else if let Some(Annotation::Text { text, .. }) = self.doc.get_mut(i) {
            *text = new.to_owned();
        }
        self.end()
    }

    /// Picks `c` as the colour; the selected annotation takes it too (one
    /// step). True when the document changed.
    pub fn pick_color(&mut self, c: u32) -> bool {
        self.color = c;
        self.restyle(|a| a.set_color(c))
    }

    /// Picks stroke `i`; the selected annotation takes it too (one step).
    pub fn pick_stroke(&mut self, i: usize) -> bool {
        self.stroke = i.min(2);
        let (width, size) = (STROKES[self.stroke], text_size(self.stroke));
        self.restyle(|a| a.set_stroke(width, size))
    }

    fn restyle(&mut self, f: impl FnOnce(&mut Annotation)) -> bool {
        let Some(i) = self.selected.filter(|i| *i < self.doc.len()) else {
            return false;
        };
        self.begin();
        f(&mut self.doc[i]);
        self.end()
    }

    /// Removes the selected annotation (one step).
    pub fn delete_selected(&mut self) -> bool {
        let Some(i) = self.selected.take() else {
            return false;
        };
        if i >= self.doc.len() {
            return false;
        }
        self.begin();
        self.doc.remove(i);
        self.end()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(before) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.doc, before));
        self.selected = None;
        self.touch();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(after) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.doc, after));
        self.selected = None;
        self.touch();
        true
    }

    /// The topmost annotation under `p`.
    pub fn hit(&self, p: Pt, tol: f32) -> Option<usize> {
        self.doc.iter().rposition(|a| a.hit(p, tol))
    }

    /// True once anything has changed since the last save or copy: closing
    /// then asks first.
    pub fn dirty(&self) -> bool {
        self.changed
    }

    /// The picture was saved or copied.
    pub fn mark_clean(&mut self) {
        self.changed = false;
    }

    /// The document's change count: taken with a save's snapshot and given
    /// back to `mark_clean_at` when the save lands.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// A save of the document as it was at `revision` finished: clean only
    /// when nothing changed since (a later change is not in the file).
    pub fn mark_clean_at(&mut self, revision: u64) {
        if self.revision == revision {
            self.changed = false;
        }
    }

    fn touch(&mut self) {
        self.changed = true;
        self.revision += 1;
    }
}

/// How the picture sits in the canvas: image px × `scale` + the offset is a
/// canvas pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub scale: f32,
    pub ox: f32,
    pub oy: f32,
}

impl View {
    pub fn to_image(self, x: f32, y: f32) -> Pt {
        Pt::new((x - self.ox) / self.scale, (y - self.oy) / self.scale)
    }

    pub fn to_view(self, p: Pt) -> (f32, f32) {
        (self.ox + p.x * self.scale, self.oy + p.y * self.scale)
    }

    /// The picture centred in a canvas of `w` × `h`.
    pub fn centered(scale: f32, img: (u32, u32), canvas: (f32, f32)) -> View {
        View {
            scale,
            ox: ((canvas.0 - img.0 as f32 * scale) / 2.0).floor(),
            oy: ((canvas.1 - img.1 as f32 * scale) / 2.0).floor(),
        }
    }
}

/// The share of the monitor's work area the picture may take.
pub const FIT_SHARE: f32 = 0.85;

/// 1.0 (100 %) when the picture fits in `FIT_SHARE` of the work area
/// (`avail`, physical px), else the scale that makes it fit.
pub fn initial_scale(img: (u32, u32), avail: (f32, f32)) -> f32 {
    if img.0 == 0 || img.1 == 0 {
        return 1.0;
    }
    let fit_w = avail.0 * FIT_SHARE / img.0 as f32;
    let fit_h = avail.1 * FIT_SHARE / img.1 as f32;
    fit_w.min(fit_h).clamp(0.01, 1.0)
}

/// Block size for pixelating a picture of `w` × `h`: big enough that text
/// cannot be read back, small enough to look like a mosaic.
pub fn pixel_block(w: u32, h: u32) -> u32 {
    (w.max(h) / 120).clamp(8, 32)
}

/// The part of `r` inside a picture of `w` × `h`, in whole pixels
/// (x, y, w, h); `None` when nothing is inside.
pub fn clip_to_image(r: FRect, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let x0 = r.x.floor().max(0.0);
    let y0 = r.y.floor().max(0.0);
    let x1 = r.right().ceil().min(w as f32);
    let y1 = r.bottom().ceil().min(h as f32);
    (x1 > x0 && y1 > y0).then_some((x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

/// The pixels of `region` (x, y, w, h) of a `w` × `h` BGRA picture, each
/// `block` × `block` cell replaced by its average. The cells are aligned to
/// the region's corner, so the mosaic does not shift while it is dragged.
pub fn pixelate(
    bgra: &[u8],
    w: u32,
    h: u32,
    region: (u32, u32, u32, u32),
    block: u32,
) -> Option<Vec<u8>> {
    let (rx, ry, rw, rh) = region;
    if block == 0
        || rw == 0
        || rh == 0
        || rx.checked_add(rw)? > w
        || ry.checked_add(rh)? > h
        || bgra.len() < (w as usize) * (h as usize) * 4
    {
        return None;
    }
    let stride = w as usize * 4;
    let mut out = vec![0u8; rw as usize * rh as usize * 4];
    let mut cy = 0;
    while cy < rh {
        let ch = block.min(rh - cy);
        let mut cx = 0;
        while cx < rw {
            let cw = block.min(rw - cx);
            let mut sum = [0u64; 4];
            for y in 0..ch {
                for x in 0..cw {
                    let at = (ry + cy + y) as usize * stride + (rx + cx + x) as usize * 4;
                    for (s, v) in sum.iter_mut().zip(&bgra[at..at + 4]) {
                        *s += u64::from(*v);
                    }
                }
            }
            let n = u64::from(cw * ch);
            let avg = sum.map(|s| (s / n) as u8);
            for y in 0..ch {
                for x in 0..cw {
                    let at = (cy + y) as usize * rw as usize * 4 + (cx + x) as usize * 4;
                    out[at..at + 4].copy_from_slice(&avg);
                }
            }
            cx += block;
        }
        cy += block;
    }
    Some(out)
}

/// One clipboard write an editor made (Copy, an auto-copy or the flush at
/// close), remembered by the sequence number it left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnCopy {
    pub seq: u32,
    /// The editor (capture) that wrote it: `next_capture_id`.
    pub capture: u64,
    /// That editor's auto-copy was on when it opened: the island keeps one
    /// card per capture and shows nothing for these writes.
    pub managed: bool,
}

/// How many editor writes are remembered; older ones fall out.
const OWN_COPIES: usize = 32;

/// The last `OWN_COPIES` writes, oldest first.
static OWN_COPY_LOG: Mutex<VecDeque<OwnCopy>> = Mutex::new(VecDeque::new());

/// The next capture number: each editor takes one when it opens.
pub fn next_capture_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// `copy` joins `log`; the oldest falls out past `OWN_COPIES`. Sequence 0 is
/// never kept, and a sequence already there is replaced, not doubled.
fn remember(log: &mut VecDeque<OwnCopy>, copy: OwnCopy) {
    if copy.seq == 0 {
        return;
    }
    log.retain(|c| c.seq != copy.seq);
    log.push_back(copy);
    while log.len() > OWN_COPIES {
        log.pop_front();
    }
}

/// The write in `log` that left sequence `seq` (0 never matches).
fn recognise(log: &VecDeque<OwnCopy>, seq: u32) -> Option<OwnCopy> {
    if seq == 0 {
        return None;
    }
    log.iter().find(|c| c.seq == seq).copied()
}

/// Remembers the sequence number an editor's write left on the clipboard.
pub fn record_own_copy(seq: u32, capture: u64, managed: bool) {
    if let Ok(mut log) = OWN_COPY_LOG.lock() {
        remember(
            &mut log,
            OwnCopy {
                seq,
                capture,
                managed,
            },
        );
    }
}

/// The editor write that left sequence `seq`, if it was one: the notch's
/// screenshot preview skips it, and the island treats a managed one apart.
pub fn own_copy(seq: u32) -> Option<OwnCopy> {
    OWN_COPY_LOG
        .lock()
        .ok()
        .and_then(|log| recognise(&log, seq))
}

/// "Salin otomatis", as `App` last saw it in the config (on by default).
static AUTO_COPY: AtomicBool = AtomicBool::new(true);

pub fn set_auto_copy(on: bool) {
    AUTO_COPY.store(on, Ordering::Relaxed);
}

/// What an editor latches when it opens.
pub fn auto_copy_setting() -> bool {
    AUTO_COPY.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Style = Style {
        color: 0xE5484D,
        width: 4.0,
    };

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Annotation {
        Annotation::Rect {
            r: FRect::new(x, y, w, h),
            style: RED,
        }
    }

    fn arrow(a: (f32, f32), b: (f32, f32)) -> Annotation {
        Annotation::Arrow {
            a: Pt::new(a.0, a.1),
            b: Pt::new(b.0, b.1),
            style: RED,
        }
    }

    #[test]
    fn tools_have_their_letters() {
        for t in Tool::ALL {
            assert_eq!(Tool::from_key(t.key()), Some(t));
            assert_eq!(Tool::from_key(t.key().to_ascii_lowercase()), Some(t));
        }
        assert_eq!(Tool::from_key('x'), None);
        assert_eq!(Tool::default(), Tool::Select);
        let letters: String = Tool::ALL.iter().map(|t| t.key()).collect();
        assert_eq!(letters, "VTRAPMB");
    }

    #[test]
    fn a_rectangle_is_hit_on_its_line_not_inside() {
        let a = rect(100.0, 100.0, 200.0, 100.0);
        assert!(a.hit(Pt::new(100.0, 150.0), 4.0), "left edge");
        assert!(
            a.hit(Pt::new(200.0, 203.0), 4.0),
            "bottom edge, within reach"
        );
        assert!(!a.hit(Pt::new(200.0, 150.0), 4.0), "inside stays free");
        assert!(!a.hit(Pt::new(50.0, 50.0), 4.0));
    }

    #[test]
    fn arrows_pens_text_and_pixelate_are_hit() {
        let ar = arrow((0.0, 0.0), (100.0, 0.0));
        assert!(ar.hit(Pt::new(50.0, 5.0), 4.0));
        assert!(!ar.hit(Pt::new(50.0, 30.0), 4.0));
        let pen = Annotation::Pen {
            pts: vec![Pt::new(0.0, 0.0), Pt::new(50.0, 50.0), Pt::new(100.0, 0.0)],
            style: RED,
        };
        assert!(pen.hit(Pt::new(25.0, 26.0), 3.0));
        assert!(!pen.hit(Pt::new(50.0, 0.0), 3.0));
        let text = Annotation::Text {
            pos: Pt::new(10.0, 10.0),
            text: "halo".into(),
            size: 20.0,
            color: 0,
        };
        assert!(text.hit(Pt::new(20.0, 20.0), 2.0));
        assert!(!text.hit(Pt::new(200.0, 20.0), 2.0));
        let px = Annotation::Pixelate {
            r: FRect::new(10.0, 10.0, 80.0, 40.0),
        };
        assert!(px.hit(Pt::new(50.0, 30.0), 2.0), "anywhere inside");
        assert!(!px.hit(Pt::new(200.0, 30.0), 2.0));
    }

    #[test]
    fn the_topmost_annotation_is_hit_first() {
        let mut s = Session::default();
        s.add(rect(0.0, 0.0, 100.0, 100.0));
        s.add(rect(0.0, 0.0, 100.0, 100.0));
        assert_eq!(s.hit(Pt::new(0.0, 50.0), 3.0), Some(1));
        assert_eq!(s.hit(Pt::new(500.0, 500.0), 3.0), None);
    }

    #[test]
    fn moving_shifts_every_kind() {
        let mut a = rect(10.0, 10.0, 20.0, 20.0);
        a.move_by(5.0, -3.0);
        assert_eq!(
            a,
            Annotation::Rect {
                r: FRect::new(15.0, 7.0, 20.0, 20.0),
                style: RED
            }
        );
        let mut p = Annotation::Marker {
            pts: vec![Pt::new(0.0, 0.0), Pt::new(4.0, 4.0)],
            style: RED,
        };
        p.move_by(1.0, 2.0);
        assert!(matches!(&p, Annotation::Marker { pts, .. }
            if pts[0] == Pt::new(1.0, 2.0) && pts[1] == Pt::new(5.0, 6.0)));
        let mut t = Annotation::Text {
            pos: Pt::new(3.0, 3.0),
            text: "x".into(),
            size: 10.0,
            color: 0,
        };
        t.move_by(-3.0, -3.0);
        assert!(matches!(t, Annotation::Text { pos, .. } if pos == Pt::new(0.0, 0.0)));
    }

    #[test]
    fn a_rectangle_resizes_from_a_corner_and_may_flip() {
        let mut a = rect(100.0, 100.0, 100.0, 50.0);
        assert_eq!(a.handle_at(Pt::new(101.0, 99.0), 4.0), Some(Handle::Nw));
        assert_eq!(a.handle_at(Pt::new(200.0, 150.0), 4.0), Some(Handle::Se));
        assert_eq!(a.handle_at(Pt::new(150.0, 125.0), 4.0), None);
        a.resize(Handle::Se, Pt::new(250.0, 200.0));
        assert!(matches!(&a, Annotation::Rect { r, .. }
            if *r == FRect::new(100.0, 100.0, 150.0, 100.0)));
        // Past the opposite corner: the rectangle flips, nothing goes negative.
        a.resize(Handle::Nw, Pt::new(300.0, 250.0));
        assert!(matches!(&a, Annotation::Rect { r, .. }
            if *r == FRect::new(250.0, 200.0, 50.0, 50.0)));
        let mut px = Annotation::Pixelate {
            r: FRect::new(0.0, 0.0, 10.0, 10.0),
        };
        px.resize(Handle::Ne, Pt::new(40.0, -20.0));
        assert!(matches!(&px, Annotation::Pixelate { r }
            if *r == FRect::new(0.0, -20.0, 40.0, 30.0)));
    }

    #[test]
    fn an_arrow_resizes_by_its_ends() {
        let mut a = arrow((10.0, 10.0), (100.0, 50.0));
        assert_eq!(a.handle_at(Pt::new(99.0, 52.0), 5.0), Some(Handle::End));
        assert_eq!(a.handle_at(Pt::new(12.0, 8.0), 5.0), Some(Handle::Start));
        assert_eq!(a.handle_at(Pt::new(55.0, 30.0), 5.0), None);
        a.resize(Handle::End, Pt::new(200.0, 200.0));
        a.resize(Handle::Start, Pt::new(0.0, 0.0));
        assert!(matches!(a, Annotation::Arrow { a, b, .. }
            if a == Pt::new(0.0, 0.0) && b == Pt::new(200.0, 200.0)));
        assert_eq!(a.handles().len(), 2);
        assert!(
            Annotation::Pen {
                pts: vec![],
                style: RED
            }
            .handles()
            .is_empty()
        );
    }

    #[test]
    fn delete_removes_the_selected_one_and_undo_brings_it_back() {
        let mut s = Session::default();
        s.add(rect(0.0, 0.0, 10.0, 10.0));
        s.add(arrow((0.0, 0.0), (50.0, 50.0)));
        assert!(!s.delete_selected(), "nothing selected");
        s.selected = Some(0);
        assert!(s.delete_selected());
        assert_eq!(s.doc.len(), 1);
        assert!(matches!(s.doc[0], Annotation::Arrow { .. }));
        assert_eq!(s.selected, None);
        assert!(s.undo());
        assert_eq!(s.doc.len(), 2);
        assert!(matches!(s.doc[0], Annotation::Rect { .. }));
    }

    #[test]
    fn undo_and_redo_walk_the_steps_and_a_new_step_ends_the_redo() {
        let mut s = Session::default();
        assert!(!s.can_undo() && !s.can_redo());
        assert!(!s.undo() && !s.redo());
        s.add(rect(0.0, 0.0, 10.0, 10.0));
        s.add(rect(20.0, 0.0, 10.0, 10.0));
        s.add(rect(40.0, 0.0, 10.0, 10.0));
        assert!(s.undo() && s.undo());
        assert_eq!(s.doc.len(), 1);
        assert!(s.can_redo());
        assert!(s.redo());
        assert_eq!(s.doc.len(), 2);
        s.add(arrow((0.0, 0.0), (9.0, 9.0)));
        assert!(!s.can_redo(), "a new step drops the redo");
        assert_eq!(s.doc.len(), 3);
        while s.undo() {}
        assert!(s.doc.is_empty());
        while s.redo() {}
        assert_eq!(s.doc.len(), 3);
    }

    #[test]
    fn a_drag_is_one_step_and_a_drag_that_moved_nothing_is_none() {
        let mut s = Session::default();
        s.add(rect(0.0, 0.0, 10.0, 10.0));
        s.begin();
        for _ in 0..5 {
            s.doc[0].move_by(2.0, 0.0);
        }
        assert!(s.end());
        assert!(s.undo());
        assert!(matches!(&s.doc[0], Annotation::Rect { r, .. } if r.x == 0.0));
        s.redo();
        s.begin();
        assert!(!s.end(), "nothing moved");
        // One undo undoes the whole drag, not a piece of it.
        assert!(s.undo());
        assert!(matches!(&s.doc[0], Annotation::Rect { r, .. } if r.x == 0.0));
    }

    #[test]
    fn empty_text_is_dropped() {
        let mut s = Session::default();
        assert_eq!(s.add_text(Pt::new(5.0, 5.0), ""), None);
        assert_eq!(s.add_text(Pt::new(5.0, 5.0), "  \n "), None);
        assert!(s.doc.is_empty() && !s.dirty() && !s.can_undo());
        let i = s.add_text(Pt::new(5.0, 5.0), "Catatan").expect("kept");
        assert_eq!(s.doc.len(), 1);
        // Editing it again; clearing the text removes it.
        assert!(s.set_text(i, "Catatan 2"));
        assert!(matches!(&s.doc[0], Annotation::Text { text, .. } if text == "Catatan 2"));
        assert!(!s.set_text(i, "Catatan 2"), "unchanged is no step");
        assert!(s.set_text(i, "  "));
        assert!(s.doc.is_empty());
        assert!(s.undo());
        assert!(matches!(&s.doc[0], Annotation::Text { text, .. } if text == "Catatan 2"));
    }

    #[test]
    fn a_click_with_a_drawing_tool_leaves_nothing() {
        let mut s = Session::default();
        assert_eq!(s.add(rect(10.0, 10.0, 1.0, 1.0)), None);
        assert_eq!(
            s.add(Annotation::Pen {
                pts: vec![Pt::new(1.0, 1.0)],
                style: RED
            }),
            None
        );
        assert!(s.doc.is_empty());
    }

    #[test]
    fn dirty_follows_changes_until_a_save_or_copy() {
        let mut s = Session::default();
        assert!(!s.dirty(), "a fresh editor is clean");
        s.add(rect(0.0, 0.0, 10.0, 10.0));
        assert!(s.dirty());
        s.mark_clean();
        assert!(!s.dirty());
        s.undo();
        assert!(s.dirty(), "undo is a change");
        s.mark_clean();
        s.redo();
        assert!(s.dirty());
        s.mark_clean();
        s.selected = Some(0);
        s.delete_selected();
        assert!(s.dirty());
        // Choosing a tool or a colour is not a change.
        s.mark_clean();
        s.tool = Tool::Pen;
        s.color = COLORS[3];
        assert!(!s.dirty());
    }

    #[test]
    fn a_save_that_lands_after_a_change_leaves_it_dirty() {
        let mut s = Session::default();
        s.add(rect(0.0, 0.0, 10.0, 10.0));
        // Save pressed: the worker writes this revision.
        let saved = s.revision();
        // An arrow drawn before the save lands.
        s.add(rect(5.0, 5.0, 10.0, 10.0));
        s.mark_clean_at(saved);
        assert!(s.dirty(), "the new change is not in the file");
        // A save of the current document cleans it.
        let saved = s.revision();
        s.mark_clean_at(saved);
        assert!(!s.dirty());
        // Undo after Save pressed is a change too.
        s.undo();
        s.mark_clean_at(saved);
        assert!(s.dirty());
    }

    #[test]
    fn the_demo_document_is_not_a_change() {
        let s = Session::with_doc(vec![rect(0.0, 0.0, 10.0, 10.0)]);
        assert!(!s.dirty() && !s.can_undo());
    }

    #[test]
    fn the_view_maps_both_ways() {
        let v = View::centered(0.5, (1000, 600), (800.0, 500.0));
        assert_eq!((v.ox, v.oy), (150.0, 100.0));
        let (x, y) = v.to_view(Pt::new(200.0, 100.0));
        assert_eq!((x, y), (250.0, 150.0));
        let p = v.to_image(x, y);
        assert_eq!(p, Pt::new(200.0, 100.0));
        // The corners of the picture land on the corners of the drawn image.
        assert_eq!(v.to_view(Pt::new(1000.0, 600.0)), (650.0, 400.0));
        let one = View {
            scale: 1.0,
            ox: 20.0,
            oy: 30.0,
        };
        assert_eq!(one.to_image(120.0, 130.0), Pt::new(100.0, 100.0));
    }

    #[test]
    fn a_picture_shows_at_100_percent_when_it_fits_in_85_percent() {
        let area = (1920.0, 1040.0);
        assert_eq!(initial_scale((800, 600), area), 1.0);
        // 1632 × 884 is the limit.
        assert_eq!(initial_scale((1632, 884), area), 1.0);
        let s = initial_scale((3264, 1768), area);
        assert!((s - 0.5).abs() < 1e-4, "{s}");
        // Tall: the height decides.
        let s = initial_scale((1000, 3536), area);
        assert!((s - 0.25).abs() < 1e-4, "{s}");
        assert_eq!(initial_scale((0, 0), area), 1.0);
        // Never scaled up.
        assert_eq!(initial_scale((10, 10), area), 1.0);
    }

    #[test]
    fn clipping_to_the_image_rounds_outward_and_stays_inside() {
        assert_eq!(
            clip_to_image(FRect::new(-5.0, 2.5, 20.0, 10.2), 100, 50),
            Some((0, 2, 15, 11))
        );
        assert_eq!(
            clip_to_image(FRect::new(90.0, 40.0, 50.0, 50.0), 100, 50),
            Some((90, 40, 10, 10))
        );
        assert_eq!(
            clip_to_image(FRect::new(200.0, 0.0, 5.0, 5.0), 100, 50),
            None
        );
    }

    #[test]
    fn pixelating_averages_each_cell_and_leaves_the_rest() {
        // 4 × 2 picture: left half black, right half white.
        let mut img = Vec::new();
        for _y in 0..2 {
            for x in 0..4 {
                let v = if x < 2 { 0 } else { 200 };
                img.extend([v, v, v, 255]);
            }
        }
        let out = pixelate(&img, 4, 2, (0, 0, 4, 2), 4).expect("pixelated");
        // One cell: the average of all eight pixels.
        assert!(out.chunks(4).all(|p| p == [100, 100, 100, 255]));
        // Cells of 2: each half keeps its own value.
        let out = pixelate(&img, 4, 2, (0, 0, 4, 2), 2).expect("pixelated");
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
        assert_eq!(&out[8..12], &[200, 200, 200, 255]);
        // A region inside the picture reads from it at its place.
        let part = pixelate(&img, 4, 2, (2, 0, 2, 2), 8).expect("pixelated");
        assert_eq!(part.len(), 2 * 2 * 4);
        assert!(part.chunks(4).all(|p| p == [200, 200, 200, 255]));
        assert!(pixelate(&img, 4, 2, (3, 0, 2, 2), 2).is_none(), "outside");
        assert!(pixelate(&img, 4, 2, (0, 0, 4, 2), 0).is_none());
        assert!(
            pixelate(&img[..8], 4, 2, (0, 0, 4, 2), 2).is_none(),
            "short"
        );
    }

    #[test]
    fn the_mosaic_is_coarse_enough() {
        assert_eq!(pixel_block(800, 600), 8);
        assert_eq!(pixel_block(3840, 2160), 32);
        assert_eq!(pixel_block(1920, 1080), 16);
    }

    fn copy(seq: u32, capture: u64) -> OwnCopy {
        OwnCopy {
            seq,
            capture,
            managed: capture % 2 == 0,
        }
    }

    #[test]
    fn every_recorded_sequence_is_an_editors_own_copy() {
        let mut log = VecDeque::new();
        remember(&mut log, copy(42, 1));
        remember(&mut log, copy(50, 2));
        remember(&mut log, copy(60, 1));
        assert_eq!(recognise(&log, 42), Some(copy(42, 1)));
        assert_eq!(recognise(&log, 50), Some(copy(50, 2)));
        assert_eq!(recognise(&log, 60).map(|c| c.capture), Some(1));
        assert!(recognise(&log, 43).is_none());
        assert!(recognise(&log, 50).is_some_and(|c| c.managed));
        assert!(recognise(&log, 42).is_some_and(|c| !c.managed));
    }

    #[test]
    fn sequence_zero_is_never_own() {
        let mut log = VecDeque::new();
        remember(&mut log, copy(0, 1));
        assert!(log.is_empty(), "0 is not kept");
        remember(&mut log, copy(7, 1));
        assert!(recognise(&log, 0).is_none());
    }

    #[test]
    fn the_oldest_write_falls_out_past_the_cap() {
        let mut log = VecDeque::new();
        for seq in 1..=(OWN_COPIES as u32 + 3) {
            remember(&mut log, copy(seq, 1));
        }
        assert_eq!(log.len(), OWN_COPIES);
        assert!(recognise(&log, 1).is_none());
        assert!(recognise(&log, 3).is_none());
        assert!(recognise(&log, 4).is_some());
        assert!(recognise(&log, OWN_COPIES as u32 + 3).is_some());
        // The same sequence recorded twice is one entry.
        remember(&mut log, copy(10, 9));
        assert_eq!(log.len(), OWN_COPIES);
        assert_eq!(recognise(&log, 10).map(|c| c.capture), Some(9));
    }

    #[test]
    fn capture_numbers_are_unique() {
        let (a, b) = (next_capture_id(), next_capture_id());
        assert_ne!(a, b);
    }

    #[test]
    fn a_copy_cleans_at_its_revision_and_never_a_newer_one() {
        let mut s = Session::default();
        s.add(rect(0.0, 0.0, 10.0, 10.0));
        let copied = s.revision();
        s.mark_clean_at(copied);
        assert!(!s.dirty(), "cleaned at the copied revision");
        s.add(rect(5.0, 5.0, 10.0, 10.0));
        s.mark_clean_at(copied);
        assert!(s.dirty(), "the newer revision stays dirty");
    }

    #[test]
    fn a_colour_or_stroke_pick_restyles_the_selection_in_one_step() {
        let mut s = Session::default();
        s.add(rect(0.0, 0.0, 10.0, 10.0));
        // Nothing selected: only the choice changes, no step, not dirty.
        s.mark_clean();
        assert!(!s.pick_color(COLORS[3]));
        assert_eq!(s.color, COLORS[3]);
        assert!(!s.dirty());
        s.selected = Some(0);
        assert!(s.pick_color(COLORS[2]));
        assert!(s.pick_stroke(2));
        assert!(matches!(&s.doc[0], Annotation::Rect { style, .. }
            if style.color == COLORS[2] && style.width == STROKES[2]));
        assert!(!s.pick_color(COLORS[2]), "the same colour is no step");
        assert!(s.undo() && s.undo());
        assert!(matches!(&s.doc[0], Annotation::Rect { style, .. }
            if style.color == RED.color && style.width == RED.width));
        // A pixelated area has no colour to change.
        s.add(Annotation::Pixelate {
            r: FRect::new(0.0, 0.0, 9.0, 9.0),
        });
        s.selected = Some(1);
        assert!(!s.pick_color(COLORS[0]));
        assert_eq!(s.doc[1].color(), None);
    }

    #[test]
    fn strokes_and_sizes_have_three_steps() {
        assert_eq!(STROKES.len(), 3);
        assert!(STROKES.windows(2).all(|w| w[0] < w[1]));
        assert!(text_size(0) < text_size(1) && text_size(1) < text_size(2));
        assert_eq!(text_size(9), text_size(2));
        let s = Session::default();
        assert_eq!(s.style().color, COLORS[0]);
        assert_eq!(s.style().width, STROKES[1]);
    }
}
