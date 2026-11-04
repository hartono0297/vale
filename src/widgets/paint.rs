//! Drawing a sticky note into a layered window's canvas, and the geometry the
//! pointer and keyboard code needs to agree with that drawing: the header's
//! buttons, the checkbox of each item, where a click lands in the text and
//! where the caret is.
//!
//! Everything is in dip relative to the note's top-left; the window adds a
//! margin around it (a soft shadow, and above it the room the peeking cat
//! needs) and the canvas's transform scales dip to pixels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_ROUNDED_RECT, ID2D1RenderTarget,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_WEIGHT, DWRITE_HIT_TEST_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    IDWriteTextFormat, IDWriteTextLayout,
};
use windows::core::{BOOL, Interface, Result};
use windows_numerics::{Matrix3x2, Vector2};

use crate::arrange_overlay::Canvas;
use crate::config::UiFont;
use crate::shot_edit::gfx::{Gfx, color};
use crate::theme::{CatAccessory, CatColor};
use crate::ui::cat_style::CatLook;
use crate::ui::fonts::{Face, Role};
use crate::ui::geometry::Rect;
use crate::ui::icons::{Icon, IconSet};
use crate::ui::robot::{Expr, Pose, RobotPainter};
use crate::widgets::checklist::{self, Line};
use crate::widgets::sticky::{ROLLED_H, TextSize, Tones, chip_tones};

// ------------------------------------------------------------------ geometry

/// The strip along the top: the date at rest, the buttons while the pointer
/// is on the note. All of it moves the note.
pub const HEADER_H: f32 = 36.0;
pub const FOOTER_H: f32 = 24.0;
pub const PAD_X: f32 = 12.0;
pub const BTN: f32 = 22.0;
pub const BTN_GAP: f32 = 2.0;
pub const GRIP: f32 = 16.0;
pub const RADIUS: f32 = 10.0;
/// A checklist item's box, and the indent of its text.
pub const BOX: f32 = 14.0;
pub const INDENT: f32 = 22.0;
/// The soft shadow around the note, and the room above it for the cat.
pub const SHADOW: f32 = 8.0;
pub const CAT_ROOM: f32 = 30.0;
/// The cat: its square, and how much of its 64-unit grid is above the note's
/// top edge (the grid row of its eyes' lower edge).
const CAT_SIZE: f32 = 44.0;
const CAT_VISIBLE_ROWS: f32 = 47.0;
/// The accent frame the Settings gallery's highlight draws.
const FRAME_W: f32 = 3.0;
/// How long a reminder shakes its note, in ms, and how far it goes (dip).
pub const WIGGLE_MS: f32 = 600.0;
const WIGGLE_DIP: f32 = 4.0;

/// The note's sideways shake `ms` after a reminder fired: a few quick swings
/// that die away inside the window's margin. Zero before it starts and after
/// `WIGGLE_MS`.
pub fn wiggle_dx(ms: f32) -> f32 {
    if !(0.0..WIGGLE_MS).contains(&ms) {
        return 0.0;
    }
    let t = ms / WIGGLE_MS;
    WIGGLE_DIP * (1.0 - t) * (1.0 - t) * (t * std::f32::consts::TAU * 4.0).sin()
}

/// The header's buttons, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Pin,
    Plus,
    Check,
    Colour,
    More,
    Close,
}

impl Btn {
    pub const ALL: [Btn; 6] = [
        Btn::Pin,
        Btn::Plus,
        Btn::Check,
        Btn::Colour,
        Btn::More,
        Btn::Close,
    ];
}

/// The margin around the note inside its window, in dip: left, top, right,
/// bottom.
pub fn margins() -> (f32, f32, f32, f32) {
    (SHADOW, SHADOW + CAT_ROOM, SHADOW, SHADOW)
}

/// The window's size in dip for a note of `w` x `h`.
pub fn window_dip(w: f32, h: f32) -> (f32, f32) {
    let m = margins();
    (w + m.0 + m.2, h + m.1 + m.3)
}

/// The note's height as shown: a rolled-up note is one header and one line.
pub fn shown_h(h: f32, rolled: bool) -> f32 {
    if rolled { ROLLED_H } else { h }
}

/// A header button's rect, right-aligned in a note `w` wide.
pub fn button_rect(btn: Btn, w: f32) -> Rect {
    let i = Btn::ALL.iter().position(|b| *b == btn).unwrap_or(0) as f32;
    let n = Btn::ALL.len() as f32;
    let right = w - 6.0;
    Rect {
        x: right - (n - i) * BTN - (n - 1.0 - i) * BTN_GAP,
        y: (HEADER_H - BTN) / 2.0,
        w: BTN,
        h: BTN,
    }
}

/// The text area of a note `w` x `h` (not rolled).
pub fn body_rect(w: f32, h: f32) -> Rect {
    Rect {
        x: PAD_X,
        y: HEADER_H,
        w: (w - 2.0 * PAD_X).max(0.0),
        h: (h - HEADER_H - FOOTER_H).max(0.0),
    }
}

/// The resize grip's rect.
pub fn grip_rect(w: f32, h: f32) -> Rect {
    Rect {
        x: w - GRIP,
        y: h - GRIP,
        w: GRIP,
        h: GRIP,
    }
}

/// What is under a point of the note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The header's free part: drags the note, a double click rolls it.
    Header,
    Button(Btn),
    /// The box of the item on this line.
    Check(usize),
    Body,
    Grip,
    /// The footer's text.
    Footer,
    Outside,
}

fn inside(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

fn grown(r: Rect, by: f32) -> Rect {
    Rect {
        x: r.x - by,
        y: r.y - by,
        w: r.w + 2.0 * by,
        h: r.h + 2.0 * by,
    }
}

/// What a point (dip from the note's top-left) of a note `w` x `h` is on.
/// `body` is the text layout (for the item boxes) and `scroll` how far it is
/// scrolled. `buttons_shown` is whether the header's buttons are visible: a
/// hidden button takes no press, so the whole strip (full width, the pinned
/// pin included) is `Header`.
#[allow(clippy::too_many_arguments)]
pub fn target(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rolled: bool,
    body: Option<&BodyLayout>,
    scroll: f32,
    buttons_shown: bool,
) -> Target {
    let h = shown_h(h, rolled);
    if x < 0.0 || y < 0.0 || x >= w || y >= h {
        return Target::Outside;
    }
    if y < HEADER_H {
        if buttons_shown {
            for b in Btn::ALL {
                if inside(button_rect(b, w), x, y) {
                    return Target::Button(b);
                }
            }
        }
        return Target::Header;
    }
    if rolled {
        return Target::Header;
    }
    if inside(grip_rect(w, h), x, y) {
        return Target::Grip;
    }
    let area = body_rect(w, h);
    if y >= area.y + area.h {
        return Target::Footer;
    }
    if let Some(b) = body {
        for (i, l) in b.lines.iter().enumerate() {
            if l.raw.is_item() {
                let r = grown(box_rect(l, area, scroll), 4.0);
                if inside(r, x, y) {
                    return Target::Check(i);
                }
            }
        }
    }
    Target::Body
}

/// An item's checkbox, in the note's dip.
pub fn box_rect(l: &BodyLine, area: Rect, scroll: f32) -> Rect {
    Rect {
        x: area.x,
        y: area.y + l.y - scroll + (l.first_h - BOX) / 2.0,
        w: BOX,
        h: BOX,
    }
}

// ------------------------------------------------------------------ resources

/// What every widget window shares: the factories, the icons, the cat and the
/// font (Vale's Font setting).
pub struct Res {
    pub gfx: Rc<Gfx>,
    icons: IconSet,
    robot: RobotPainter,
    face: Face,
    ui: UiFont,
    formats: RefCell<HashMap<(u32, u16), IDWriteTextFormat>>,
}

impl Res {
    pub fn new(ui: UiFont) -> Result<Res> {
        Res::with_gfx(Rc::new(Gfx::new()?), ui)
    }

    pub fn with_gfx(gfx: Rc<Gfx>, ui: UiFont) -> Result<Res> {
        let icons = IconSet::new(&gfx.d2d)?;
        let robot = RobotPainter::new(&gfx.d2d)?;
        let face = Face::for_role_in(&gfx.dwrite, Role::Text, ui);
        Ok(Res {
            gfx,
            icons,
            robot,
            face,
            ui,
            formats: RefCell::new(HashMap::new()),
        })
    }

    /// Vale's Font setting changed.
    pub fn set_font(&mut self, ui: UiFont) {
        if ui != self.ui {
            self.ui = ui;
            self.face = Face::for_role_in(&self.gfx.dwrite, Role::Text, ui);
            self.formats.borrow_mut().clear();
        }
    }

    fn format(&self, size: f32, weight: u16) -> Result<IDWriteTextFormat> {
        let key = (size.to_bits(), weight);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        let f = self
            .face
            .format(&self.gfx.dwrite, DWRITE_FONT_WEIGHT(i32::from(weight)), size)?;
        self.formats.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    /// `text` wrapped at `width` dip, drawn at `size` dip.
    pub fn layout(&self, text: &str, size: f32, weight: u16, width: f32) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let format = self.format(size, weight)?;
        unsafe {
            self.gfx
                .dwrite
                .CreateTextLayout(&wide, &format, width.max(1.0), 100_000.0)
        }
    }
}

// ------------------------------------------------------------------ body text

/// One line of the note's text, laid out.
pub struct BodyLine {
    pub raw: Line,
    /// Top, from the top of the text (unscrolled), and height.
    pub y: f32,
    pub h: f32,
    /// The first row's height (where an item's box centres).
    pub first_h: f32,
    /// Where its text starts, from the body's left.
    pub x: f32,
    pub title: bool,
    pub layout: Option<IDWriteTextLayout>,
    /// The line's text after its marker, as UTF-16 units (what `layout` was
    /// made from): a hit is settled against it.
    content: Vec<u16>,
}

pub struct BodyLayout {
    pub lines: Vec<BodyLine>,
    pub height: f32,
}

/// The text of a note laid out for a body `width` wide.
pub fn build_body(res: &Res, text: &str, size: TextSize, width: f32) -> BodyLayout {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut lines = Vec::new();
    let mut y = 0.0;
    for (i, raw) in checklist::lines(text).into_iter().enumerate() {
        let title = i == 0;
        let (fs, weight, min_h) = if title {
            (size.title(), 600, size.line() + 2.0)
        } else {
            (size.body(), 400, size.line())
        };
        let x = if raw.is_item() { INDENT } else { 0.0 };
        let content = String::from_utf16_lossy(&units[raw.content_start()..raw.end]);
        let layout = res.layout(&content, fs, weight, width - x).ok();
        let h = layout
            .as_ref()
            .and_then(|l| {
                let mut m = DWRITE_TEXT_METRICS::default();
                unsafe { l.GetMetrics(&mut m) }.ok().map(|_| m.height)
            })
            .unwrap_or(min_h)
            .max(min_h);
        let first_h = layout
            .as_ref()
            .and_then(|l| {
                let (mut x0, mut y0) = (0.0f32, 0.0f32);
                let mut m = DWRITE_HIT_TEST_METRICS::default();
                unsafe { l.HitTestTextPosition(0, false, &mut x0, &mut y0, &mut m) }
                    .ok()
                    .map(|_| m.height)
            })
            .unwrap_or(min_h)
            .max(min_h.min(h));
        if raw.checked
            && let Some(l) = &layout
            && content.encode_utf16().count() > 0
        {
            let range = DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: content.encode_utf16().count() as u32,
            };
            unsafe {
                let _ = l.SetStrikethrough(true, range);
            }
        }
        lines.push(BodyLine {
            raw,
            y,
            h,
            first_h: first_h.min(h),
            x,
            title,
            layout,
            content: units[raw.content_start()..raw.end].to_vec(),
        });
        y += h + 1.0;
    }
    BodyLayout {
        lines,
        height: (y - 1.0).max(0.0),
    }
}

/// The text position a `HitTestPoint` answer puts the caret at: the start of
/// the hit cluster, or its end when the hit is on the trailing side. The end
/// is `textPosition + length`: `length` is the cluster's length in UTF-16
/// units, 2 for an emoji (a surrogate pair), 2 or more for a base letter with
/// combining marks. Adding 1 puts the caret inside the cluster, and a typed
/// character or a delete there splits the pair.
fn hit_index(text_position: u32, length: u32, trailing: bool) -> usize {
    text_position as usize + if trailing { length as usize } else { 0 }
}

/// A unit that attaches to the one before it: a combining mark, a variation
/// selector or a zero width joiner. These are the common ranges, not the whole
/// of Unicode; any other index stays a legal one (both sides are whole code
/// points), the caret just may rest between a letter and a rarer mark.
fn is_extend(unit: u16) -> bool {
    matches!(
        unit,
        0x0300..=0x036F
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x200D
            | 0x20D0..=0x20FF
            | 0xFE00..=0xFE0F
            | 0xFE20..=0xFE2F
    )
}

/// `pos` (UTF-16 units into `content`) moved forward to the next place a caret
/// may rest: never between the halves of a surrogate pair, never between a
/// letter and the combining marks after it. DirectWrite's hit metrics can
/// name a one-unit cluster for a letter whose mark is a unit of its own, so
/// `hit_index` alone is not enough there.
fn settle_index(content: &[u16], pos: usize) -> usize {
    let mut pos = pos.min(content.len());
    let low = |u: u16| (0xDC00..0xE000).contains(&u);
    let high = |u: u16| (0xD800..0xDC00).contains(&u);
    loop {
        let inside = pos > 0 && pos < content.len();
        let in_pair = inside && high(content[pos - 1]) && low(content[pos]);
        let before_mark = inside && is_extend(content[pos]);
        if in_pair || before_mark {
            pos += 1;
        } else {
            return pos;
        }
    }
}

impl BodyLayout {
    /// The line holding model index `index`.
    fn line_of(&self, index: usize) -> Option<&BodyLine> {
        self.lines
            .iter()
            .rev()
            .find(|l| l.raw.start <= index)
            .or_else(|| self.lines.first())
    }

    /// The caret at model index `index`: x, y, height, from the text's top-left
    /// (unscrolled). A caret inside an item's marker is at the text.
    pub fn caret(&self, index: usize) -> Option<(f32, f32, f32)> {
        let line = self.line_of(index)?;
        let at = index.clamp(line.raw.content_start(), line.raw.end.max(line.raw.content_start()));
        let pos = (at - line.raw.content_start()) as u32;
        match &line.layout {
            Some(l) => {
                let (mut x, mut y) = (0.0f32, 0.0f32);
                let mut m = DWRITE_HIT_TEST_METRICS::default();
                unsafe { l.HitTestTextPosition(pos, false, &mut x, &mut y, &mut m) }.ok()?;
                Some((line.x + x, line.y + y, m.height.max(1.0)))
            }
            None => Some((line.x, line.y, line.h)),
        }
    }

    /// The model index at a point of the text (from its top-left, unscrolled).
    pub fn index_at(&self, x: f32, y: f32) -> usize {
        let Some(first) = self.lines.first() else {
            return 0;
        };
        let line = self
            .lines
            .iter()
            .rev()
            .find(|l| l.y <= y)
            .unwrap_or(first);
        let end = line.raw.end.max(line.raw.content_start());
        let Some(l) = &line.layout else {
            return end;
        };
        let (mut trailing, mut inside) = (BOOL(0), BOOL(0));
        let mut m = DWRITE_HIT_TEST_METRICS::default();
        let hit = unsafe {
            l.HitTestPoint(
                (x - line.x).max(0.0),
                (y - line.y).max(0.0),
                &mut trailing,
                &mut inside,
                &mut m,
            )
        };
        if hit.is_err() {
            return end;
        }
        let pos = hit_index(m.textPosition, m.length, trailing.as_bool());
        let pos = settle_index(&line.content, pos);
        (line.raw.content_start() + pos).min(end)
    }

    /// The rectangles that show a selection of model indices `start..end`.
    pub fn selection(&self, start: usize, end: usize) -> Vec<Rect> {
        let mut out = Vec::new();
        for line in &self.lines {
            let (cs, ce) = (line.raw.content_start(), line.raw.end);
            let (s, e) = (start.max(cs), end.min(ce));
            let Some(l) = &line.layout else { continue };
            if s >= e {
                // An empty line inside the selection still shows a stub.
                if start <= line.raw.end && end > line.raw.end && ce == cs {
                    out.push(Rect { x: line.x, y: line.y, w: 4.0, h: line.first_h });
                }
                continue;
            }
            let mut count = 0u32;
            let _ = unsafe {
                l.HitTestTextRange(
                    (s - cs) as u32,
                    (e - s) as u32,
                    0.0,
                    0.0,
                    None,
                    &mut count,
                )
            };
            let mut metrics = vec![DWRITE_HIT_TEST_METRICS::default(); count as usize];
            let ok = unsafe {
                l.HitTestTextRange(
                    (s - cs) as u32,
                    (e - s) as u32,
                    0.0,
                    0.0,
                    Some(&mut metrics),
                    &mut count,
                )
            };
            if ok.is_err() {
                continue;
            }
            for m in metrics.iter().take(count as usize) {
                out.push(Rect {
                    x: line.x + m.left,
                    y: line.y + m.top,
                    w: m.width,
                    h: m.height,
                });
            }
        }
        out
    }
}

// ------------------------------------------------------------------ drawing

/// An alignment line shown while a note is dragged: a vertical line at `pos`
/// when `vertical`, else a horizontal one (dip from the note's top-left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuideLine {
    pub vertical: bool,
    pub pos: f32,
}

/// Everything one frame of a note shows.
pub struct Scene<'a> {
    pub w: f32,
    pub h: f32,
    pub scale: f32,
    pub tones: Tones,
    pub rolled: bool,
    pub pinned: bool,
    pub header: f32,
    pub hot: Option<Btn>,
    pub plus_enabled: bool,
    pub date: String,
    pub footer: String,
    pub rolled_note: String,
    pub title: String,
    pub body: Option<&'a BodyLayout>,
    pub scroll: f32,
    /// A selection (model indices) and whether the caret is lit.
    pub selection: Option<(usize, usize)>,
    pub caret_on: bool,
    pub guides: &'a [GuideLine],
    /// The whole note is shifted sideways by this (the reminder's shake), dip.
    pub dx: f32,
    /// The reminder chip: its text, and whether it is overdue.
    pub chip: Option<(String, bool)>,
    /// The gallery's highlight frame, 0 to 1.
    pub highlight: f32,
    /// The cat above the note, and its coat.
    pub peek: Option<Expr>,
    pub cat: CatLook,
    pub frame_accent: u32,
}

fn rr(x: f32, y: f32, w: f32, h: f32, r: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
        radiusX: r,
        radiusY: r,
    }
}

/// Draws the note into `c` (cleared to transparent first by the caller).
pub fn draw(c: &Canvas, res: &Res, s: &Scene) {
    let (rt, brush, gfx) = c.parts();
    let rt: ID2D1RenderTarget = match rt.cast() {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let m = margins();
    let h = shown_h(s.h, s.rolled);
    let (ox, oy) = (m.0 + s.dx, m.1);
    unsafe {
        rt.SetTransform(&Matrix3x2::scale(s.scale, s.scale));
    }
    let set = |rgb: u32, a: f32| unsafe { brush.SetColor(&color(rgb, a)) };
    let t = &s.tones;

    // The cat, behind the paper, only the part above its top edge.
    if let Some(expr) = s.peek {
        let k = CAT_SIZE / 64.0;
        let x = (ox + s.w - 28.0 - CAT_SIZE).max(ox + 8.0);
        let top = oy - CAT_VISIBLE_ROWS * k;
        unsafe {
            rt.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: ox,
                    top: oy - CAT_ROOM,
                    right: ox + s.w,
                    bottom: oy + 1.0,
                },
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }
        res.robot.paint(
            &rt,
            brush,
            Rect {
                x,
                y: top,
                w: CAT_SIZE,
                h: CAT_SIZE,
            },
            expr,
            Pose::default(),
            1.0,
            s.cat,
        );
        unsafe { rt.PopAxisAlignedClip() };
    }

    // A soft shadow, then the paper.
    for (grow, alpha) in [(5.0, 0.035), (3.0, 0.05), (1.5, 0.07)] {
        set(0x000000, alpha);
        unsafe {
            rt.FillRoundedRectangle(
                &rr(ox - grow, oy - grow + 2.0, s.w + 2.0 * grow, h + 2.0 * grow, RADIUS + grow),
                brush,
            );
        }
    }
    set(t.fill, 1.0);
    unsafe {
        rt.FillRoundedRectangle(&rr(ox, oy, s.w, h, RADIUS), brush);
    }
    if let Some(strip) = t.strip {
        unsafe {
            rt.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: ox,
                    top: oy,
                    right: ox + s.w,
                    bottom: oy + 3.0,
                },
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }
        set(strip, 1.0);
        unsafe {
            rt.FillRoundedRectangle(&rr(ox, oy, s.w, 3.0 + 2.0 * RADIUS, RADIUS), brush);
            rt.PopAxisAlignedClip();
        }
    }

    // The header: the date at rest, the buttons while the pointer is on it.
    let icon = |icon: Icon, r: Rect, ink: u32, a: f32| {
        let _ = res.icons.draw(
            &rt,
            brush,
            icon,
            Rect {
                x: ox + r.x,
                y: oy + r.y,
                w: r.w,
                h: r.h,
            },
            14.0,
            ink,
            a,
            None,
        );
    };
    let line_text = |text: &str, x: f32, y: f32, w: f32, hh: f32, size: f32, weight: u16, rgb: u32, a: f32, right: bool| {
        if text.is_empty() || w <= 0.0 {
            return;
        }
        let Ok(layout) = res.layout(text, size, weight, w) else {
            return;
        };
        unsafe {
            let _ = layout.SetWordWrapping(
                windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_NO_WRAP,
            );
            let _ = layout.SetMaxHeight(hh);
            let _ = layout.SetParagraphAlignment(
                windows::Win32::Graphics::DirectWrite::DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            );
            if right {
                let _ = layout.SetTextAlignment(
                    windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING,
                );
            }
            brush.SetColor(&color(rgb, a));
            rt.DrawTextLayout(
                Vector2::new(x, y),
                &layout,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    };
    let right_edge = button_rect(Btn::Pin, s.w).x;
    // The reminder chip, right-aligned in the strip, fading with the date.
    let mut date_end = right_edge;
    if let Some((label, overdue)) = &s.chip
        && s.header < 1.0
    {
        let (fill, ink) = chip_tones(t, *overdue);
        let tw = text_width(res, label, CHIP_TEXT, 600);
        let (cw, ch) = (tw + 2.0 * CHIP_PAD + CHIP_ICON + 3.0, CHIP_H);
        let cx = (s.w - PAD_X - cw).max(PAD_X);
        let cy = (HEADER_H - ch) / 2.0;
        let a = 1.0 - s.header;
        set(fill, a);
        unsafe {
            rt.FillRoundedRectangle(&rr(ox + cx, oy + cy, cw, ch, ch / 2.0), brush);
        }
        icon(
            Icon::Bell,
            Rect { x: cx + CHIP_PAD - 1.0, y: cy + (ch - CHIP_ICON - 2.0) / 2.0, w: CHIP_ICON + 2.0, h: CHIP_ICON + 2.0 },
            ink,
            a,
        );
        line_text(
            label,
            ox + cx + CHIP_PAD + CHIP_ICON + 3.0,
            oy + cy,
            tw + 2.0,
            ch,
            CHIP_TEXT,
            600,
            ink,
            a,
            false,
        );
        date_end = date_end.min(cx - 6.0);
    }
    if s.header < 1.0 {
        line_text(
            &s.date,
            ox + PAD_X,
            oy,
            (date_end - PAD_X).max(0.0),
            HEADER_H,
            10.5,
            400,
            t.secondary,
            (1.0 - s.header) * 0.95,
            false,
        );
    }
    for b in Btn::ALL {
        let r = button_rect(b, s.w);
        let pinned_rest = b == Btn::Pin && s.pinned;
        let a = if pinned_rest { 1.0 } else { s.header };
        if a <= 0.01 {
            continue;
        }
        if s.hot == Some(b) && s.header > 0.3 {
            set(t.body, 0.10 * a);
            unsafe {
                rt.FillRoundedRectangle(&rr(ox + r.x, oy + r.y, r.w, r.h, 6.0), brush);
            }
        }
        let disabled = b == Btn::Plus && !s.plus_enabled;
        let ink = if pinned_rest { t.accent } else { t.secondary };
        let a = if disabled { a * 0.4 } else { a };
        match b {
            Btn::Pin => icon(Icon::Pin, r, ink, a),
            Btn::Plus => icon(Icon::Plus, r, ink, a),
            Btn::Check => icon(Icon::Check, r, ink, a),
            Btn::Colour => icon(Icon::Swatch, r, ink, a),
            Btn::Close => icon(Icon::X, r, ink, a),
            Btn::More => {
                set(ink, a);
                for dx in [-4.5f32, 0.0, 4.5] {
                    unsafe {
                        rt.FillEllipse(
                            &D2D1_ELLIPSE {
                                point: Vector2::new(ox + r.x + r.w / 2.0 + dx, oy + r.y + r.h / 2.0),
                                radiusX: 1.4,
                                radiusY: 1.4,
                            },
                            brush,
                        );
                    }
                }
            }
        }
    }

    if s.rolled {
        // One line: the title, and how many lines it hides.
        let note = &s.rolled_note;
        let note_w = if note.is_empty() { 0.0 } else { 64.0 };
        line_text(
            &s.title,
            ox + PAD_X,
            oy + HEADER_H - 4.0,
            (s.w - 2.0 * PAD_X - note_w).max(0.0),
            ROLLED_H - HEADER_H,
            13.5,
            600,
            t.title,
            1.0,
            false,
        );
        line_text(
            note,
            ox + s.w - PAD_X - note_w,
            oy + HEADER_H - 4.0,
            note_w,
            ROLLED_H - HEADER_H,
            10.5,
            400,
            t.secondary,
            1.0,
            true,
        );
    } else {
        draw_body(&rt, brush, s, ox, oy);
        // The footer and the resize grip.
        let foot = Rect {
            x: PAD_X,
            y: s.h - FOOTER_H,
            w: s.w - PAD_X - GRIP - 4.0,
            h: FOOTER_H,
        };
        line_text(&s.footer, ox + foot.x, oy + foot.y, foot.w, foot.h, 10.5, 400, t.secondary, 1.0, false);
        set(t.secondary, 0.7);
        for (dx, dy) in [(0.0, 0.0), (-4.0, 0.0), (0.0, -4.0), (-8.0, 0.0), (-4.0, -4.0), (0.0, -8.0)] {
            unsafe {
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2::new(ox + s.w - 8.0 + dx, oy + s.h - 8.0 + dy),
                        radiusX: 0.9,
                        radiusY: 0.9,
                    },
                    brush,
                );
            }
        }
    }

    // Alignment guides while dragging, and the gallery's highlight.
    if !s.guides.is_empty() {
        set(t.accent, 0.9);
        for g in s.guides {
            let (a, b) = if g.vertical {
                (Vector2::new(ox + g.pos, oy - 6.0), Vector2::new(ox + g.pos, oy + h + 6.0))
            } else {
                (Vector2::new(ox - 6.0, oy + g.pos), Vector2::new(ox + s.w + 6.0, oy + g.pos))
            };
            unsafe { rt.DrawLine(a, b, brush, 1.5, &gfx.dash) };
        }
    }
    if s.highlight > 0.01 {
        set(s.frame_accent, s.highlight.clamp(0.0, 1.0));
        unsafe {
            rt.DrawRoundedRectangle(
                &rr(ox - 1.5, oy - 1.5, s.w + 3.0, h + 3.0, RADIUS + 1.5),
                brush,
                FRAME_W,
                None,
            );
        }
    }
    unsafe { rt.SetTransform(&Matrix3x2::identity()) };
}

fn draw_body(
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    s: &Scene,
    ox: f32,
    oy: f32,
) {
    let Some(body) = s.body else { return };
    let t = &s.tones;
    let area = body_rect(s.w, s.h);
    unsafe {
        rt.PushAxisAlignedClip(
            &D2D_RECT_F {
                left: ox + area.x,
                top: oy + area.y,
                right: ox + area.x + area.w,
                bottom: oy + area.y + area.h,
            },
            D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        );
    }
    let (bx, by) = (ox + area.x, oy + area.y - s.scroll);
    // The selection under the text.
    if let Some((a, b)) = s.selection
        && a != b
    {
        unsafe { brush.SetColor(&color(t.accent, 0.28)) };
        for r in body.selection(a.min(b), a.max(b)) {
            unsafe {
                rt.FillRectangle(
                    &D2D_RECT_F {
                        left: bx + r.x,
                        top: by + r.y,
                        right: bx + r.x + r.w,
                        bottom: by + r.y + r.h,
                    },
                    brush,
                );
            }
        }
    }
    for line in &body.lines {
        let top = by + line.y;
        if top > oy + area.y + area.h || top + line.h < oy + area.y {
            continue;
        }
        if line.raw.is_item() {
            let r = box_rect(line, area, s.scroll);
            let (x, y) = (ox + r.x, oy + r.y);
            if line.raw.checked {
                unsafe {
                    brush.SetColor(&color(t.accent, 1.0));
                    rt.FillRoundedRectangle(&rr(x, y, BOX, BOX, 4.0), brush);
                    brush.SetColor(&color(t.fill, 1.0));
                    // A check mark of two strokes.
                    let p = |dx: f32, dy: f32| Vector2::new(x + dx, y + dy);
                    rt.DrawLine(p(3.5, 7.2), p(6.0, 9.7), brush, 1.7, None);
                    rt.DrawLine(p(6.0, 9.7), p(10.6, 4.6), brush, 1.7, None);
                }
            } else {
                unsafe {
                    brush.SetColor(&color(t.secondary, 1.0));
                    rt.DrawRoundedRectangle(
                        &rr(x + 0.75, y + 0.75, BOX - 1.5, BOX - 1.5, 3.5),
                        brush,
                        1.5,
                        None,
                    );
                }
            }
        }
        if let Some(l) = &line.layout {
            let ink = if line.raw.checked {
                t.done
            } else if line.title {
                t.title
            } else {
                t.body
            };
            unsafe {
                brush.SetColor(&color(ink, 1.0));
                rt.DrawTextLayout(
                    Vector2::new(bx + line.x, top),
                    l,
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
    }
    if s.caret_on
        && let Some((a, b)) = s.selection
        && a == b
        && let Some((cx, cy, ch)) = body.caret(a)
    {
        unsafe {
            brush.SetColor(&color(t.body, 1.0));
            rt.FillRectangle(
                &D2D_RECT_F {
                    left: bx + cx,
                    top: by + cy,
                    right: bx + cx + 1.5,
                    bottom: by + cy + ch,
                },
                brush,
            );
        }
    }
    unsafe { rt.PopAxisAlignedClip() };
}

/// The reminder chip's text size, padding, height and bell (dip).
const CHIP_TEXT: f32 = 10.0;
const CHIP_PAD: f32 = 7.0;
const CHIP_H: f32 = 18.0;
const CHIP_ICON: f32 = 10.0;

/// How wide `text` is laid out in one line, dip.
fn text_width(res: &Res, text: &str, size: f32, weight: u16) -> f32 {
    res.layout(text, size, weight, 2000.0)
        .ok()
        .and_then(|l| {
            let mut m = DWRITE_TEXT_METRICS::default();
            unsafe { l.GetMetrics(&mut m) }
                .ok()
                .map(|_| m.widthIncludingTrailingWhitespace)
        })
        .unwrap_or(0.0)
}

/// The cat's face above a note: Happy while every item is done, else Idle.
pub fn peek_face(note_text: &str) -> Expr {
    if checklist::all_done(note_text) {
        Expr::Happy
    } else {
        Expr::Idle
    }
}

/// The coat the peek uses: the configured coat, no accessory (a hat would be
/// cut by the note's edge).
pub fn peek_look(coat: CatColor) -> CatLook {
    CatLook {
        color: coat,
        accessory: CatAccessory::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_buttons_fit_inside_the_smallest_note() {
        let w = crate::widgets::sticky::MIN_W;
        let mut last = 0.0;
        for b in Btn::ALL {
            let r = button_rect(b, w);
            assert!(r.x >= PAD_X - 4.0, "{b:?} at {}", r.x);
            assert!(r.x + r.w <= w - 5.0, "{b:?}");
            assert!(r.x >= last, "{b:?} overlaps the one before");
            last = r.x + r.w;
            assert!(r.y >= 0.0 && r.y + r.h <= HEADER_H);
        }
    }

    #[test]
    fn a_point_is_classified() {
        let (w, h) = (240.0, 208.0);
        let t = |x, y| target(x, y, w, h, false, None, 0.0, true);
        assert_eq!(t(20.0, 10.0), Target::Header);
        let close = button_rect(Btn::Close, w);
        assert_eq!(t(close.x + 2.0, close.y + 2.0), Target::Button(Btn::Close));
        assert_eq!(t(100.0, 100.0), Target::Body);
        assert_eq!(t(w - 3.0, h - 3.0), Target::Grip);
        assert_eq!(t(40.0, h - 8.0), Target::Footer);
        assert_eq!(t(-1.0, 5.0), Target::Outside);
        assert_eq!(t(5.0, h + 1.0), Target::Outside);
        // Rolled: one line tall, and all of it drags.
        let r = |x, y| target(x, y, w, h, true, None, 0.0, true);
        assert_eq!(r(20.0, 40.0), Target::Header);
        assert_eq!(r(20.0, 80.0), Target::Outside);
    }

    #[test]
    fn hidden_buttons_take_no_press_and_the_whole_strip_drags() {
        for w in [160.0f32, 240.0, 640.0] {
            let mut x = 0.0;
            while x < w {
                for y in [0.0, 10.0, HEADER_H - 0.5] {
                    assert_eq!(
                        target(x, y, w, 208.0, false, None, 0.0, false),
                        Target::Header,
                        "{w} wide, x {x} y {y}"
                    );
                }
                x += 1.0;
            }
            // Just under the strip is the text.
            assert_eq!(target(20.0, HEADER_H + 1.0, w, 208.0, false, None, 0.0, false), Target::Body);
        }
    }

    #[test]
    fn shown_buttons_take_the_press_and_the_rest_of_the_strip_drags() {
        let w = 240.0;
        for b in Btn::ALL {
            let r = button_rect(b, w);
            assert_eq!(
                target(r.x + r.w / 2.0, r.y + r.h / 2.0, w, 208.0, false, None, 0.0, true),
                Target::Button(b)
            );
        }
        let first = button_rect(Btn::Pin, w);
        assert_eq!(target(first.x - 3.0, 10.0, w, 208.0, false, None, 0.0, true), Target::Header);
        assert_eq!(target(20.0, 3.0, w, 208.0, false, None, 0.0, true), Target::Header);
        // Above and under the buttons, still inside the strip: the strip.
        assert_eq!(target(first.x + 5.0, 1.0, w, 208.0, false, None, 0.0, true), Target::Header);
        assert_eq!(
            target(first.x + 5.0, HEADER_H - 1.0, w, 208.0, false, None, 0.0, true),
            Target::Header
        );
    }

    #[test]
    fn a_rolled_note_is_all_strip_and_the_strip_is_36_dip() {
        assert_eq!(HEADER_H, 36.0);
        assert_eq!(ROLLED_H, 64.0, "the strip and one 28 dip line");
        for shown in [false, true] {
            for y in [2.0, 20.0, 50.0, 63.0] {
                assert_eq!(
                    target(20.0, y, 240.0, 208.0, true, None, 0.0, shown),
                    Target::Header,
                    "y {y}"
                );
            }
        }
    }

    #[test]
    fn the_window_has_room_for_the_shadow_and_the_cat() {
        let (ww, wh) = window_dip(240.0, 208.0);
        assert_eq!(ww, 256.0);
        assert_eq!(wh, 208.0 + 2.0 * SHADOW + CAT_ROOM);
        assert_eq!(shown_h(208.0, true), ROLLED_H);
        assert_eq!(shown_h(208.0, false), 208.0);
    }

    #[test]
    fn the_shake_starts_and_ends_still_and_stays_in_the_margin() {
        assert_eq!(wiggle_dx(0.0), 0.0);
        assert_eq!(wiggle_dx(-5.0), 0.0);
        assert_eq!(wiggle_dx(WIGGLE_MS), 0.0);
        assert_eq!(wiggle_dx(10_000.0), 0.0);
        let mut peak: f32 = 0.0;
        let mut moved = false;
        let mut ms = 0.0;
        while ms < WIGGLE_MS {
            let dx = wiggle_dx(ms);
            peak = peak.max(dx.abs());
            moved |= dx.abs() > 0.5;
            ms += 5.0;
        }
        assert!(moved, "it really moves");
        assert!(peak <= WIGGLE_DIP, "{peak}");
        assert!(peak < SHADOW, "inside the margin, with room for the shadow");
        // It decays: the late swings are smaller than the early ones.
        let early = (0..20).map(|i| wiggle_dx(i as f32 * 5.0).abs()).fold(0.0, f32::max);
        let late = (0..20).map(|i| wiggle_dx(500.0 + i as f32 * 5.0).abs()).fold(0.0, f32::max);
        assert!(late < early);
    }

    #[test]
    fn the_cat_is_happy_only_when_everything_is_done() {
        assert_eq!(peek_face("t\n- [x] a\n- [x] b"), Expr::Happy);
        assert_eq!(peek_face("t\n- [x] a\n- [ ] b"), Expr::Idle);
        assert_eq!(peek_face("plain"), Expr::Idle);
        assert_eq!(peek_look(CatColor::Calico).accessory, CatAccessory::None);
    }

    #[test]
    fn text_is_laid_out_and_clicks_map_back_to_indices() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        let text = "Belanja\n- [ ] bata\n- [x] kaca\nbiasa";
        let body = build_body(&res, text, TextSize::M, 200.0);
        assert_eq!(body.lines.len(), 4);
        assert!(body.lines[0].title);
        assert!(body.lines[1].raw.is_item());
        assert!(body.lines[1].x > 0.0, "items are indented for their box");
        assert!(body.lines.windows(2).all(|w| w[0].y < w[1].y));
        assert!(body.height > 3.0 * TextSize::M.line());
        // The caret of every index is inside the text, and a click on it gives
        // the index back.
        for index in [0, 3, 7, 14, 16, 18, 20, 26, 28, 33, text.len()] {
            let (x, y, ch) = body.caret(index).expect("caret");
            assert!(ch > 5.0, "{index}");
            let back = body.index_at(x + 0.5, y + ch / 2.0);
            let want = index.max(
                body.lines
                    .iter()
                    .find(|l| l.raw.start <= index && index <= l.raw.end)
                    .map_or(0, |l| l.raw.content_start()),
            );
            assert!(back.abs_diff(want) <= 1, "{index}: caret at ({x},{y}) gave {back}");
        }
        // A click above the text is the first line, below it the last.
        assert!(body.index_at(0.0, -20.0) <= 7);
        assert_eq!(body.index_at(500.0, 10_000.0), text.len());
        // A selection of a whole line has a rectangle.
        let rects = body.selection(14, 18);
        assert!(!rects.is_empty() && rects.iter().all(|r| r.w > 0.0));
        // The item boxes are hit targets.
        let area = body_rect(240.0, 208.0);
        let item = box_rect(&body.lines[1], area, 0.0);
        assert_eq!(
            target(item.x + 3.0, item.y + 3.0, 240.0, 208.0, false, Some(&body), 0.0, true),
            Target::Check(1)
        );
        assert_eq!(
            target(item.x + 80.0, item.y + 3.0, 240.0, 208.0, false, Some(&body), 0.0, true),
            Target::Body
        );
    }

    #[test]
    fn a_hit_index_adds_the_cluster_length_when_trailing() {
        // Leading: the cluster's start, whatever its length.
        assert_eq!(hit_index(5, 1, false), 5);
        assert_eq!(hit_index(5, 2, false), 5);
        // Trailing: after the whole cluster. One unit for a plain letter, two
        // for an emoji (surrogate pair) or a base letter with a combining mark.
        assert_eq!(hit_index(5, 1, true), 6);
        assert_eq!(hit_index(5, 2, true), 7);
        assert_eq!(hit_index(0, 3, true), 3);
        // A zero-length answer (the end of an empty line) stays put.
        assert_eq!(hit_index(0, 0, true), 0);
    }

    /// Whether `index` of `text` (UTF-16 units) is between a high and a low
    /// surrogate.
    fn inside_a_pair(text: &str, index: usize) -> bool {
        let u: Vec<u16> = text.encode_utf16().collect();
        index > 0
            && index < u.len()
            && (0xD800..0xDC00).contains(&u[index - 1])
            && (0xDC00..0xE000).contains(&u[index])
    }

    /// Every model index a click along the first line of `text` gives,
    /// sweeping x over the line in 0.25 dip steps (and a little past both ends).
    fn sweep_first_line(body: &BodyLayout) -> Vec<(f32, usize)> {
        let (_, y, h) = body.caret(0).expect("caret");
        let width = body.lines[0].layout.as_ref().map_or(200.0, |l| {
            let mut m = DWRITE_TEXT_METRICS::default();
            unsafe { l.GetMetrics(&mut m) }.ok();
            m.widthIncludingTrailingWhitespace
        });
        let steps = ((width + 8.0) * 4.0) as usize;
        (0..=steps)
            .map(|i| {
                let x = -2.0 + i as f32 * 0.25;
                (x, body.index_at(x, y + h / 2.0))
            })
            .collect()
    }

    #[test]
    fn a_settled_index_skips_halves_of_a_pair_and_combining_marks() {
        let u = |t: &str| t.encode_utf16().collect::<Vec<u16>>();
        let t = u("ab\u{1F600}cd");
        assert_eq!(
            (0..=6).map(|i| settle_index(&t, i)).collect::<Vec<_>>(),
            [0, 1, 2, 4, 4, 5, 6]
        );
        assert_eq!(settle_index(&t, 99), 6, "past the end is the end");
        // e + acute + grave: no rest between the three.
        let m = u("e\u{0301}\u{0300}x");
        assert_eq!(
            (0..=3).map(|i| settle_index(&m, i)).collect::<Vec<_>>(),
            [0, 3, 3, 3]
        );
        // A letter followed by a variation selector, and a joiner.
        let z = u("a\u{FE0F}b\u{200D}c");
        assert_eq!(
            (0..=5).map(|i| settle_index(&z, i)).collect::<Vec<_>>(),
            [0, 2, 2, 4, 4, 5]
        );
        assert_eq!(settle_index(&[], 0), 0);
    }

    #[test]
    fn a_click_on_an_emoji_never_lands_between_its_two_halves() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        let text = "ab\u{1F600}cd";
        let body = build_body(&res, text, TextSize::M, 400.0);
        let (x_before, y, h) = body.caret(2).expect("caret before the emoji");
        let (x_after, _, _) = body.caret(4).expect("caret after the emoji");
        assert!(x_after > x_before + 4.0, "the emoji has a width");
        let mid = y + h / 2.0;
        // The left half goes before the emoji, the right half after it.
        assert_eq!(body.index_at(x_before + (x_after - x_before) * 0.25, mid), 2);
        assert_eq!(body.index_at(x_before + (x_after - x_before) * 0.75, mid), 4);
        // Whatever x, the index is never inside the pair.
        let all = sweep_first_line(&body);
        assert!(all.len() > 20);
        for (x, index) in &all {
            assert!(!inside_a_pair(text, *index), "x {x} gave {index}");
        }
        // Sweeping across the emoji does reach both sides of it.
        assert!(all.iter().any(|(_, i)| *i == 2) && all.iter().any(|(_, i)| *i == 4));
        // The drag-selection path (a later line too) goes through the same
        // function: an emoji on the second line, the third being an item.
        let text = "t\nxy\u{1F600}\n- [ ] \u{1F600}\u{1F601}z";
        let body = build_body(&res, text, TextSize::M, 400.0);
        for (li, line) in body.lines.iter().enumerate() {
            let cs = line.raw.content_start();
            let (_, y, h) = body.caret(cs).expect("caret");
            for i in 0..=160 {
                let index = body.index_at(line.x + i as f32 * 0.5, y + h / 2.0);
                assert!(!inside_a_pair(text, index), "line {li}, step {i} gave {index}");
            }
        }
    }

    #[test]
    fn a_click_on_a_letter_with_a_combining_mark_never_splits_it() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        let text = "cafe\u{0301} x";
        // c0 a1 f2 e3 (U+0301)4 space5 x6
        let body = build_body(&res, text, TextSize::M, 400.0);
        let (x_base, y, h) = body.caret(3).expect("caret before the e");
        let (x_after, _, _) = body.caret(5).expect("caret after the mark");
        assert!(x_after > x_base + 2.0);
        let mid = y + h / 2.0;
        // DirectWrite names a one-unit cluster for the e here (position 3,
        // length 1), so its trailing hit alone would give 4: `settle_index`
        // moves it past the mark.
        // The trailing half of the cluster is after the mark, never between.
        assert_eq!(body.index_at(x_base + (x_after - x_base) * 0.75, mid), 5);
        assert_eq!(body.index_at(x_base + (x_after - x_base) * 0.25, mid), 3);
        for (x, index) in sweep_first_line(&body) {
            assert_ne!(index, 4, "x {x}: between the e and its accent");
        }
    }

    #[test]
    fn wrapping_makes_a_long_line_taller() {
        let res = Res::new(UiFont::Sans).unwrap();
        let long = "a very long line of text that will certainly have to wrap at this width";
        let narrow = build_body(&res, long, TextSize::M, 120.0);
        let wide = build_body(&res, long, TextSize::M, 900.0);
        assert!(narrow.height > wide.height * 1.5);
    }
}
