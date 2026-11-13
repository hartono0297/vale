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
use std::mem::ManuallyDrop;
use std::rc::Rc;

use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_CAP_STYLE_FLAT,
    D2D1_DASH_STYLE_CUSTOM, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_LINE_JOIN_ROUND, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, ID2D1Bitmap, ID2D1RenderTarget, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_HIT_TEST_METRICS, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTypography,
};
use windows::core::{BOOL, HSTRING, IUnknown, Interface, PCWSTR, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::arrange_overlay::Canvas;
use crate::config::UiFont;
use crate::shot_edit::gfx::{Gfx, color};
use crate::theme::{CatAccessory, CatColor};
use crate::ui::cat_style::CatLook;
use crate::i18n::{Key, Lang, text};
use crate::ui::fonts::{self, Face, Role};
use crate::ui::geometry::Rect;
use crate::ui::icons::{Icon, IconSet};
use crate::ui::robot::{Expr, Pose, RobotPainter};
use crate::widgets::art::{self, Piece};
use crate::widgets::checklist::{self, Line};
use crate::widgets::sticky::{
    Deco, DipRect, NoteFont, Pattern, ROLLED_H, Stamp, TextSize, Tones, chip_tones, tangan_family,
};
use crate::widgets::styles::{self, Hl, Run};
use crate::widgets::toolbar::{self, BAR_BLOCK, BarHit, Tool};
use crate::widgets::tuck::{self, Edge};

// ------------------------------------------------------------------ geometry

/// The strip along the top: the date at rest, the buttons while the pointer
/// is on the note. All of it moves the note.
pub const HEADER_H: f32 = 36.0;
pub const FOOTER_H: f32 = 24.0;
pub const PAD_X: f32 = 12.0;
pub const BTN: f32 = 20.0;
pub const BTN_GAP: f32 = 1.0;
pub const GRIP: f32 = 16.0;
pub const RADIUS: f32 = 10.0;
/// A checklist item's box, and the indent of its text; a bullet's dot has a
/// smaller indent.
pub const BOX: f32 = 14.0;
pub const INDENT: f32 = 22.0;
pub const BULLET_INDENT: f32 = 16.0;
/// The bullet's dot: its diameter, and how much of the body ink it takes.
const BULLET_DOT: f32 = 5.0;
const BULLET_ALPHA: f32 = 0.7;
/// An image's round remove button (dip), its inset from the corner and the
/// corner radius of the picture.
pub const IMG_X: f32 = 20.0;
pub const IMG_X_INSET: f32 = 5.0;
const IMG_RADIUS: f32 = 7.0;
/// The soft shadow around the note, and the room above it for the cat.
pub const SHADOW: f32 = 8.0;
pub const CAT_ROOM: f32 = 30.0;
/// The cat: its square, and how much of its 64-unit grid is above the note's
/// top edge (the grid row of its eyes' lower edge).
pub const CAT_SIZE: f32 = 44.0;
const CAT_VISIBLE_ROWS: f32 = 47.0;
/// The accent frame the Settings gallery's highlight draws.
const FRAME_W: f32 = 3.0;
/// The outline of a note that a drop would land on: 2 dip wide, dashes of 6
/// and gaps of 4, inset 1 dip so it sits inside the note's edge.
pub const DROP_W: f32 = 2.0;
pub const DROP_DASH: f32 = 6.0;
pub const DROP_GAP: f32 = 4.0;
pub const DROP_INSET: f32 = 1.0;

/// The rounded rect (x, y, w, h, radius) the drop outline is stroked along,
/// for a note at (`ox`, `oy`) of `w` x `h` dip: the note's own rounded rect
/// pulled in by `DROP_INSET`, so the 2 dip stroke fills the outer 2 dip.
pub fn drop_frame(ox: f32, oy: f32, w: f32, h: f32) -> (f32, f32, f32, f32, f32) {
    (
        ox + DROP_INSET,
        oy + DROP_INSET,
        w - 2.0 * DROP_INSET,
        h - 2.0 * DROP_INSET,
        RADIUS - DROP_INSET,
    )
}

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Btn {
    /// "Daftar catatan tempel": opens the list window.
    List,
    Pin,
    Plus,
    Check,
    Colour,
    More,
    Close,
    /// A stack's arrows, left of the strip (`stack_strip`): only on a stack.
    Prev,
    Next,
}

impl Btn {
    pub const ALL: [Btn; 7] = [
        Btn::List,
        Btn::Pin,
        Btn::Plus,
        Btn::Check,
        Btn::Colour,
        Btn::More,
        Btn::Close,
    ];
}

/// A stack's arrows are this big (dip), and the counter between them has room
/// for "20 / 20" in tabular figures.
pub const ARROW: f32 = 20.0;
pub const COUNTER_W: f32 = 46.0;

/// Where a stack's strip puts its arrows and its counter: from the strip's
/// left, in the place the date takes on a plain note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackStrip {
    pub prev: Rect,
    pub counter: Rect,
    pub next: Rect,
}

pub fn stack_strip() -> StackStrip {
    let y = (HEADER_H - ARROW) / 2.0;
    let prev = Rect { x: 6.0, y, w: ARROW, h: ARROW };
    let counter = Rect { x: prev.x + ARROW + 2.0, y: 0.0, w: COUNTER_W, h: HEADER_H };
    let next = Rect { x: counter.x + COUNTER_W + 2.0, y, w: ARROW, h: ARROW };
    StackStrip { prev, counter, next }
}

/// The margin around the note inside its window, in dip: left, top, right,
/// bottom.
pub fn margins() -> (f32, f32, f32, f32) {
    (SHADOW, SHADOW + CAT_ROOM, SHADOW, SHADOW)
}

/// The window's size in dip for a note of `w` x `h` with margin `m`.
pub fn window_dip(w: f32, h: f32, m: (f32, f32, f32, f32)) -> (f32, f32) {
    (w + m.0 + m.2, h + m.1 + m.3)
}

/// The note's height as shown: a rolled-up note is one header and one line.
pub fn shown_h(h: f32, rolled: bool) -> f32 {
    if rolled { ROLLED_H } else { h }
}

/// A header button's rect, right-aligned in a note `w` wide (a stack's
/// arrows are at the left).
pub fn button_rect(btn: Btn, w: f32) -> Rect {
    match btn {
        Btn::Prev => return stack_strip().prev,
        Btn::Next => return stack_strip().next,
        _ => {}
    }
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

/// The text area of a note `w` x `h` (not rolled), with the footer under it.
#[cfg_attr(not(test), allow(dead_code))]
pub fn body_rect(w: f32, h: f32) -> Rect {
    body_rect_in(w, h, false)
}

/// The text area of a note `w` x `h` (not rolled): with the format bar
/// showing (`bar`) the 42 dip block takes the place of the 24 dip footer, so
/// the area is shorter; the window never resizes.
pub fn body_rect_in(w: f32, h: f32, bar: bool) -> Rect {
    let under = if bar { BAR_BLOCK } else { FOOTER_H };
    Rect {
        x: PAD_X,
        y: HEADER_H,
        w: (w - 2.0 * PAD_X).max(0.0),
        h: (h - HEADER_H - under).max(0.0),
    }
}

/// The clip of the images: the text area's rows (so they scroll out under the
/// header and the footer like the text) but the grid's own width, 8 dip in from
/// the note's sides, which is wider than the text area. Clipping the images to
/// the text area would cut their outer rounded corners and the hover outline.
pub fn images_clip_in(w: f32, h: f32, bar: bool) -> Rect {
    let area = body_rect_in(w, h, bar);
    Rect {
        x: crate::widgets::images::SIDE,
        y: area.y,
        w: (w - 2.0 * crate::widgets::images::SIDE).max(0.0),
        h: area.h,
    }
}

/// The ✕ of an image at `r` (note dip, from the text's top): a 20 dip round
/// button 5 dip in from the top-right corner.
pub fn image_remove_rect(r: Rect) -> Rect {
    Rect {
        x: r.right() - IMG_X_INSET - IMG_X,
        y: r.y + IMG_X_INSET,
        w: IMG_X,
        h: IMG_X,
    }
}

/// What the pointer needs besides the text to know what is under it: whether
/// the format bar shows, and where the images are (note x, y from the text
/// area's top: they scroll with the text).
#[derive(Clone, Copy)]
pub struct Extras<'a> {
    pub bar: bool,
    pub images: &'a [Rect],
}

impl Extras<'static> {
    #[cfg_attr(not(test), allow(dead_code))]
    pub const NONE: Extras<'static> = Extras { bar: false, images: &[] };
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
    /// The format bar (only while the note has the keyboard): a button, or the
    /// bar's background and its margin (which do nothing).
    Bar(BarHit),
    /// The image at this index.
    Image(usize),
    /// The round ✕ of the image at this index.
    ImageRemove(usize),
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
/// pin included) is `Header`. A `stack` also has the two arrows in the strip's
/// left, shown with the other buttons.
#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::too_many_arguments)]
pub fn target_in(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rolled: bool,
    body: Option<&BodyLayout>,
    scroll: f32,
    buttons_shown: bool,
    stack: bool,
) -> Target {
    target_full(x, y, w, h, rolled, body, scroll, buttons_shown, stack, &Extras::NONE)
}

/// `target_in` with the format bar and the images: a bar button wins over the
/// grip, the grip over the bar's own background; an image's ✕ wins over the
/// image, and both over the text.
#[allow(clippy::too_many_arguments)]
pub fn target_full(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rolled: bool,
    body: Option<&BodyLayout>,
    scroll: f32,
    buttons_shown: bool,
    stack: bool,
    extra: &Extras,
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
            if stack {
                for b in [Btn::Prev, Btn::Next] {
                    if inside(button_rect(b, w), x, y) {
                        return Target::Button(b);
                    }
                }
            }
        }
        return Target::Header;
    }
    if rolled {
        return Target::Header;
    }
    let area = body_rect_in(w, h, extra.bar);
    // The bar block: a button first, then the grip, then the background.
    let bar_hit = (extra.bar && y >= h - BAR_BLOCK)
        .then(|| toolbar::hit(&toolbar::layout(w), x, y - (h - BAR_BLOCK)));
    if let Some(Some(hit)) = bar_hit
        && hit != BarHit::Bar
    {
        return Target::Bar(hit);
    }
    if inside(grip_rect(w, h), x, y) {
        return Target::Grip;
    }
    if extra.bar && y >= h - BAR_BLOCK {
        // The bar's margin is as inert as its background.
        return Target::Bar(BarHit::Bar);
    }
    if y >= area.y + area.h {
        return Target::Footer;
    }
    for (i, r) in extra.images.iter().enumerate() {
        let shown = Rect {
            x: r.x,
            y: area.y + r.y - scroll,
            w: r.w,
            h: r.h,
        };
        if inside(shown, x, y) && y < area.y + area.h {
            if inside(image_remove_rect(shown), x, y) {
                return Target::ImageRemove(i);
            }
            return Target::Image(i);
        }
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

/// `target_in` for a note that is not a stack.
#[cfg(test)]
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
    target_in(x, y, w, h, rolled, body, scroll, buttons_shown, false)
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
    pub(super) icons: IconSet,
    robot: RobotPainter,
    /// Vale's face (the Font setting).
    face: Face,
    /// The faces of Tangan and Mono, made when a note first asks for one.
    extra: RefCell<[Option<Face>; 2]>,
    ui: UiFont,
    /// One format per (font, size, weight).
    formats: RefCell<HashMap<(NoteFont, u32, u16), IDWriteTextFormat>>,
    /// Fixed-width figures, for a stack's counter.
    tabular: IDWriteTypography,
    /// The drop outline's dashes (6 on, 4 off at 2 wide: 3 and 2 stroke
    /// widths); `None` when the factory would not make it (no outline then).
    drop_dash: Option<ID2D1StrokeStyle>,
}

impl Res {
    pub fn new(ui: UiFont) -> Result<Res> {
        Res::with_gfx(Rc::new(Gfx::new()?), ui)
    }

    pub fn with_gfx(gfx: Rc<Gfx>, ui: UiFont) -> Result<Res> {
        let icons = IconSet::new(&gfx.d2d)?;
        let robot = RobotPainter::new(&gfx.d2d)?;
        let face = Face::for_role_in(&gfx.dwrite, Role::Text, ui);
        let tabular = fonts::tabular_figures(&gfx.dwrite)?;
        let dashes = [DROP_DASH / DROP_W, DROP_GAP / DROP_W];
        let drop_dash = unsafe {
            gfx.d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_FLAT,
                    endCap: D2D1_CAP_STYLE_FLAT,
                    dashCap: D2D1_CAP_STYLE_FLAT,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_CUSTOM,
                    dashOffset: 0.0,
                },
                Some(&dashes),
            )
        }
        .ok();
        Ok(Res {
            gfx,
            icons,
            robot,
            face,
            extra: RefCell::new([None, None]),
            ui,
            formats: RefCell::new(HashMap::new()),
            tabular,
            drop_dash,
        })
    }

    /// Vale's Font setting changed.
    pub fn set_font(&mut self, ui: UiFont) {
        if ui != self.ui {
            self.ui = ui;
            self.face = Face::for_role_in(&self.gfx.dwrite, Role::Text, ui);
            // Mono and the Tangan fallback follow the UI font too.
            *self.extra.borrow_mut() = [None, None];
            self.formats.borrow_mut().clear();
        }
    }

    /// The face of Tangan or Mono: Ink Free, else Segoe Print, else Vale's
    /// own face; the bundled DM Mono.
    fn make_face(&self, font: NoteFont) -> Face {
        let dwrite = &self.gfx.dwrite;
        match font {
            NoteFont::Vale => Face::for_role_in(dwrite, Role::Text, self.ui),
            NoteFont::Mono => Face::for_role_in(dwrite, Role::Mono, self.ui),
            NoteFont::Tangan => match tangan_family(|name| fonts::system_has(dwrite, name)) {
                Some(family) => Face {
                    family: HSTRING::from(family),
                    collection: None,
                    style: DWRITE_FONT_STYLE_NORMAL,
                },
                None => Face::for_role_in(dwrite, Role::Text, self.ui),
            },
        }
    }

    fn format(&self, font: NoteFont, size: f32, weight: u16) -> Result<IDWriteTextFormat> {
        let key = (font, size.to_bits(), weight);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        let w = DWRITE_FONT_WEIGHT(i32::from(weight));
        let f = match font {
            NoteFont::Vale => self.face.format(&self.gfx.dwrite, w, size)?,
            other => {
                let slot = usize::from(other == NoteFont::Mono);
                let mut extra = self.extra.borrow_mut();
                let face = extra[slot].get_or_insert_with(|| self.make_face(other));
                face.format(&self.gfx.dwrite, w, size)?
            }
        };
        self.formats.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    /// `text` in Georgia (a serif every Windows has), for the bar's B, I and A.
    /// Falls back to the layout's default face when the family is missing.
    fn serif(&self, text: &str, size: f32, weight: u16, italic: bool) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let format = self.gfx.dwrite.CreateTextFormat(
                w!("Georgia"),
                None,
                DWRITE_FONT_WEIGHT(i32::from(weight)),
                if italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL },
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                PCWSTR(w!("en-us").0),
            )?;
            self.gfx.dwrite.CreateTextLayout(&wide, &format, 100.0, 100.0)
        }
    }

    /// `layout` with tabular figures on its digits: a counter does not jitter.
    pub fn layout_tabular(&self, text: &str, size: f32, weight: u16, width: f32) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let format = self.format(NoteFont::Vale, size, weight)?;
        fonts::text_layout(&self.gfx.dwrite, &wide, &format, width.max(1.0), 100_000.0, &self.tabular)
    }

    /// `text` wrapped at `width` dip, drawn at `size` dip in Vale's face.
    pub fn layout(&self, text: &str, size: f32, weight: u16, width: f32) -> Result<IDWriteTextLayout> {
        self.layout_in(NoteFont::Vale, text, size, weight, width)
    }

    /// `layout` in a note's own font.
    pub fn layout_in(
        &self,
        font: NoteFont,
        text: &str,
        size: f32,
        weight: u16,
        width: f32,
    ) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let format = self.format(font, size, weight)?;
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

/// The text of a note laid out for a body `width` wide, in the note's `font`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn build_body(res: &Res, text: &str, font: NoteFont, size: TextSize, width: f32) -> BodyLayout {
    build_body_with(res, text, font, size, width, 0.0, &[])
}

/// Where a line's text starts from the body's left: a box and a bullet indent
/// it, the marker itself is never drawn as text.
fn indent_of(raw: &Line) -> f32 {
    match raw.marker {
        checklist::Marker::Item { .. } => INDENT,
        checklist::Marker::Bullet => BULLET_INDENT,
        checklist::Marker::None => 0.0,
    }
}

/// The weight a bold stretch takes: the body's 400 becomes 600, the title's 600
/// becomes 700.
fn bold_weight(base: u16) -> u16 {
    if base >= 600 { 700 } else { 600 }
}

/// Sets the runs' bold, italic, underline and strike on `layout` (a line's
/// text), `runs` relative to it. A done item keeps its whole-line strike.
fn style_layout(layout: &IDWriteTextLayout, runs: &[Run], base_weight: u16) {
    for r in runs {
        let range = DWRITE_TEXT_RANGE {
            startPosition: r.s as u32,
            length: (r.e - r.s) as u32,
        };
        let st = r.style;
        unsafe {
            if st.b {
                let _ = layout.SetFontWeight(DWRITE_FONT_WEIGHT(i32::from(bold_weight(base_weight))), range);
            }
            if st.i {
                let _ = layout.SetFontStyle(DWRITE_FONT_STYLE_ITALIC, range);
            }
            if st.u {
                let _ = layout.SetUnderline(true, range);
            }
            if st.x {
                let _ = layout.SetStrikethrough(true, range);
            }
        }
    }
}

/// `build_body`, with the text starting `top` dip down (the image block above
/// it) and the note's formatting `runs` (over the whole text). Carets, hits,
/// selections and boxes all come from the lines' own `y`, so they follow `top`;
/// `height` includes it. A hit above the first line is the first line.
pub fn build_body_with(
    res: &Res,
    text: &str,
    font: NoteFont,
    size: TextSize,
    width: f32,
    top: f32,
    runs: &[Run],
) -> BodyLayout {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut lines = Vec::new();
    let mut y = top;
    for (i, raw) in checklist::lines(text).into_iter().enumerate() {
        let title = i == 0;
        let (fs, weight, min_h) = if title {
            (size.title(), 600, size.line() + 2.0)
        } else {
            (size.body(), 400, size.line())
        };
        let x = indent_of(&raw);
        let content = String::from_utf16_lossy(&units[raw.content_start()..raw.end]);
        let layout = res.layout_in(font, &content, fs, weight, width - x).ok();
        // The formatting first: a bold word is wider, so it can wrap.
        if let Some(l) = &layout {
            let line_runs = styles::slice(runs, raw.content_start(), raw.end);
            style_layout(l, &line_runs, weight);
        }
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
        height: (y - 1.0).max(top),
    }
}

/// The text position a `HitTestPoint` answer puts the caret at: the start of
/// the hit cluster, or its end when the hit is on the trailing side. The end
/// is `textPosition + length`: `length` is the cluster's length in UTF-16
/// units, 2 for an emoji (a surrogate pair), 2 or more for a base letter with
/// combining marks. Adding 1 puts the caret inside the cluster, and a typed
/// character or a delete there splits the pair.
pub(crate) fn hit_index(text_position: u32, length: u32, trailing: bool) -> usize {
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
pub(crate) fn settle_index(content: &[u16], pos: usize) -> usize {
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
        self.ranges(start, end, true, true)
    }

    /// The rectangles of model indices `start..end`, line by line (what a
    /// highlight is drawn behind). With `stubs` an empty line inside the range
    /// shows a 4 dip stub, as a selection does.
    pub fn ranges(&self, start: usize, end: usize, stubs: bool, done: bool) -> Vec<Rect> {
        let mut out = Vec::new();
        for line in &self.lines {
            if line.raw.checked && !done {
                continue;
            }
            let (cs, ce) = (line.raw.content_start(), line.raw.end);
            let (s, e) = (start.max(cs), end.min(ce));
            let Some(l) = &line.layout else { continue };
            if s >= e {
                // An empty line inside the selection still shows a stub.
                if stubs && start <= line.raw.end && end > line.raw.end && ce == cs {
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

/// How the cat above a note is drawn besides its face.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CatPose {
    pub pose: Pose,
    /// A small alarm clock beside it (an overdue reminder).
    pub clock: bool,
    /// A small "z" (asleep).
    pub zzz: bool,
}

/// What a stack adds to its top sheet: the counter ("2 / 5") and the fills of
/// the next sheets in order (up to two) that peek out under the paper.
#[derive(Clone, Debug, PartialEq)]
pub struct StackScene {
    pub label: String,
    pub edges: Vec<u32>,
}

/// A tucked note: its edge, how far out it is (`progress`, 0 only the tab
/// shows, 1 the whole note), the note's scale about the tab's centre and its
/// and the tab's opacity (`tuck::Pop`), and whether its reminder is overdue
/// (the tab's dot).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TuckScene {
    pub edge: Edge,
    pub progress: f32,
    pub scale: f32,
    pub note_alpha: f32,
    pub tab_alpha: f32,
    pub overdue: bool,
}

/// How far a stack's paper edges peek out under the note, each (dip).
pub const STACK_STEP: f32 = 3.0;

/// The format bar of one frame: the hot button (its tooltip shows), which
/// tools are "on" at the caret or selection (in `Tool::ALL` order), the colour
/// under the marker glyph (the last highlight), and a footer message (an image
/// refusal, "Klik lagi untuk hapus") that is drawn above the bar's left end.
#[derive(Clone, Debug, PartialEq)]
pub struct BarScene {
    pub hot: Option<BarHit>,
    pub on: [bool; 8],
    pub marker_hl: Hl,
    pub msg: Option<String>,
}

/// An image's drawn size in pixels, with its file name: what a bitmap is
/// cached by (a resize or another DPI makes a new one).
pub type ImgKey = (String, u32, u32);

/// Decoded premultiplied BGRA pixels (width, height, bytes), shared so a frame
/// can hold them while the cache is borrowed.
pub type Pixels = Rc<(u32, u32, Vec<u8>)>;

/// The images of one window. The pixels (`decoded`) are CPU memory and outlive
/// a render target; the bitmaps are made from them on the canvas that draws
/// them and belong to that render target, so `drop_bitmaps` (a recreated
/// canvas) clears only those. The whole cache is reset when the images change
/// or the folder moves; it goes with the window.
#[derive(Default)]
pub struct ImgCache {
    pub bitmaps: HashMap<ImgKey, ID2D1Bitmap>,
    /// Decoded pixels per size, kept while the name is in the note.
    pub decoded: HashMap<ImgKey, Pixels>,
    /// Sizes that failed to decode (tried once, drawn as a quiet tile).
    pub failed: std::collections::HashSet<ImgKey>,
}

impl ImgCache {
    /// The canvas was recreated: its bitmaps are gone, the pixels stay.
    pub fn drop_bitmaps(&mut self) {
        self.bitmaps.clear();
    }

    /// Every size of `name` with pixels or a bitmap (sorted: a stable answer).
    pub fn keys_of(&self, name: &str) -> Vec<ImgKey> {
        let mut keys: Vec<ImgKey> = self
            .decoded
            .keys()
            .chain(self.bitmaps.keys())
            .filter(|k| k.0 == name)
            .cloned()
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// A size of `name` failed to decode.
    pub fn failed_name(&self, name: &str) -> bool {
        self.failed.iter().any(|k| k.0 == name)
    }

    /// Forgets what belongs to a name that is no longer in the note.
    pub fn retain_names(&mut self, names: &[String]) {
        let keep = |k: &ImgKey| names.contains(&k.0);
        self.decoded.retain(|k, _| keep(k));
        self.bitmaps.retain(|k, _| keep(k));
        self.failed.retain(keep);
    }

    /// The exact size is there: the other sizes of its name are not needed.
    pub fn keep_only(&mut self, key: &ImgKey) {
        let keep = |k: &ImgKey| k.0 != key.0 || k == key;
        self.decoded.retain(|k, _| keep(k));
        self.bitmaps.retain(|k, _| keep(k));
        self.failed.retain(keep);
    }
}

/// What to do for one image this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImgPlan {
    /// Draw with this size (the exact one, or the nearest while moving).
    Use(ImgKey),
    /// Decode the exact size now.
    Decode,
    /// Nothing to draw with and nothing to try (a quiet tile).
    Skip,
}

/// The decision of `prepare_images`, pure so a test can drive it: `want` is the
/// exact size, `have` the sizes of this name already decoded or with a bitmap,
/// `failed` whether decoding already failed (for `want` when settled, for any
/// size of the name while `moving`), `moving` a resize drag or a tuck pop.
/// While moving the nearest size stands in (smallest area difference, a tie to
/// the larger one) and a decode happens only when there is nothing at all, so
/// never one per pointer move.
pub fn image_plan(want: &ImgKey, have: &[ImgKey], failed: bool, moving: bool) -> ImgPlan {
    if have.contains(want) {
        return ImgPlan::Use(want.clone());
    }
    if moving {
        let area = |k: &ImgKey| u64::from(k.1) * u64::from(k.2);
        let target = area(want);
        let nearest = have
            .iter()
            .filter(|k| k.0 == want.0)
            .min_by_key(|k| (area(k).abs_diff(target), std::cmp::Reverse(area(k))));
        if let Some(k) = nearest {
            return ImgPlan::Use(k.clone());
        }
    }
    if failed { ImgPlan::Skip } else { ImgPlan::Decode }
}

/// The images of one frame: where each goes (note x, y from the text's top),
/// its key, the hovered one, and the cache.
pub struct ImgScene<'a> {
    pub rects: &'a [Rect],
    pub keys: &'a [ImgKey],
    pub hot: Option<usize>,
    pub cache: &'a RefCell<ImgCache>,
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
    /// The body text size (the ruling's line pitch follows it).
    pub text_size: TextSize,
    pub rolled_note: String,
    pub title: String,
    pub body: Option<&'a BodyLayout>,
    pub scroll: f32,
    /// A selection (model indices) and whether the caret is lit.
    pub selection: Option<(usize, usize)>,
    pub caret_on: bool,
    pub guides: &'a [GuideLine],
    /// The window's margin round the note (left, top, right, bottom, dip):
    /// `art::margins_for` for this note.
    pub margins: (f32, f32, f32, f32),
    /// The note's own font (the rolled title), pattern, decoration and stamp.
    pub font: NoteFont,
    pub pattern: Pattern,
    pub deco: Deco,
    pub stamp: Stamp,
    pub lang: Lang,
    /// How far the note leans (degrees clockwise): 0 while it has the keyboard.
    pub tilt: f32,
    /// Checklist progress, done and total, when the note has items.
    pub progress: Option<(usize, usize)>,
    /// The confetti of the moment, in window dip.
    pub confetti: &'a [Piece],
    /// The cat's pose, its alarm clock and its "z".
    pub cat_pose: CatPose,
    /// The whole note is shifted sideways by this (the reminder's shake), dip.
    pub dx: f32,
    /// The reminder chip: its text, and whether it is overdue.
    pub chip: Option<(String, bool)>,
    /// The gallery's highlight frame, 0 to 1.
    pub highlight: f32,
    /// Something a note takes is being dragged over it: the dashed outline.
    pub drop_hover: bool,
    /// The cat above the note, and its coat.
    pub peek: Option<Expr>,
    pub cat: CatLook,
    pub frame_accent: u32,
    /// Set for a stack's top sheet.
    pub stack: Option<StackScene>,
    /// Set while the note is tucked to an edge.
    pub tuck: Option<TuckScene>,
    /// A release here would tuck: the edge that gets the accent bar.
    pub cue: Option<Edge>,
    /// The format bar, while the note has the keyboard (it replaces the footer).
    pub bar: Option<BarScene>,
    /// The note's formatting (over its whole text) and whether its paper is
    /// Gelap (the text colours have lighter variants there).
    pub styles: &'a [Run],
    pub dark_paper: bool,
    /// The images above the text.
    pub images: Option<ImgScene<'a>>,
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
    let m = s.margins;
    let h = shown_h(s.h, s.rolled);
    let (ox, oy) = (m.0 + s.dx, m.1);
    // The note leans about its centre; the confetti does not.
    let straight = Matrix3x2::scale(s.scale, s.scale);
    // A tucked note that is popping out or in is scaled about the tab's
    // centre (it does not lean) and, while it is fading, drawn in a layer.
    let pop = s
        .tuck
        .filter(|tk| tk.progress > 0.0 && (tk.scale != 1.0 || tk.note_alpha < 1.0));
    let base = if let Some(tk) = pop {
        let (px, py) = tuck::origin(tk.edge, s.w, h);
        let (cx, cy) = (ox + px, oy + py);
        Matrix3x2 {
            M11: tk.scale * s.scale,
            M12: 0.0,
            M21: 0.0,
            M22: tk.scale * s.scale,
            M31: (cx - cx * tk.scale) * s.scale,
            M32: (cy - cy * tk.scale) * s.scale,
        }
    } else if s.tilt == 0.0 {
        straight
    } else {
        art::tilt_transform(s.tilt, ox + s.w / 2.0, oy + h / 2.0, s.scale)
    };
    // Tucked all the way in: only the tab, in the window's own place (the
    // cat, the tilt and the paper are not drawn).
    if let Some(tk) = s.tuck.filter(|tk| tk.progress <= 0.0) {
        unsafe { rt.SetTransform(&straight) };
        draw_tab(&rt, brush, s, &tk, tk.tab_alpha);
        unsafe { rt.SetTransform(&Matrix3x2::identity()) };
        return;
    }
    unsafe {
        rt.SetTransform(&base);
    }
    // The paper, shadow and content go through a layer while the note fades.
    let layer_open = pop.filter(|tk| tk.note_alpha < 1.0).and_then(|tk| {
        let layer = unsafe { rt.CreateLayer(None) }.ok()?;
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: D2D_RECT_F {
                left: f32::MIN,
                top: f32::MIN,
                right: f32::MAX,
                bottom: f32::MAX,
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: tk.note_alpha,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS_NONE,
        };
        unsafe { rt.PushLayer(&params, &layer) };
        drop(ManuallyDrop::into_inner(params.opacityBrush));
        Some(layer)
    });
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
            s.cat_pose.pose,
            1.0,
            s.cat,
        );
        unsafe { rt.PopAxisAlignedClip() };
        // An overdue reminder: a small alarm clock beside the cat.
        if s.cat_pose.clock {
            let (cx, cy) = art::clock_centre(s.w);
            art::draw_clock(&rt, brush, (ox + cx, oy + cy), t.fill, t.body, t.accent);
        }
        // Asleep: a small "z" over its head.
        if s.cat_pose.zzz {
            for (dx, dy, size, a) in [(CAT_SIZE - 10.0, -26.0, 10.0, 0.9), (CAT_SIZE - 2.0, -30.0, 7.5, 0.6)] {
                if let Ok(l) = res.layout("z", size, 600, 14.0) {
                    unsafe {
                        brush.SetColor(&color(t.secondary, a));
                        rt.DrawTextLayout(
                            Vector2::new(x + dx, oy + dy),
                            &l,
                            brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                        );
                    }
                }
            }
        }
    }

    // A stack: the next sheets' edges peek out under the paper, down and to the
    // right (inside the margin), the farthest first.
    if let Some(st) = &s.stack {
        for (k, fill) in st.edges.iter().enumerate().rev() {
            let off = STACK_STEP * (k as f32 + 1.0);
            set(0x000000, 0.07);
            unsafe {
                rt.FillRoundedRectangle(
                    &rr(ox + off - 1.5, oy + off - 1.5 + 2.0, s.w + 3.0, h + 3.0, RADIUS + 1.5),
                    brush,
                );
            }
            set(*fill, 1.0);
            unsafe {
                rt.FillRoundedRectangle(&rr(ox + off, oy + off, s.w, h, RADIUS), brush);
            }
        }
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
    let line_text = |font: NoteFont, text: &str, x: f32, y: f32, w: f32, hh: f32, size: f32, weight: u16, rgb: u32, a: f32, right: bool| {
        if text.is_empty() || w <= 0.0 {
            return;
        }
        let Ok(layout) = res.layout_in(font, text, size, weight, w) else {
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
            NoteFont::Vale,
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
    if s.header < 1.0 && s.stack.is_none() {
        // The date, and after it "2/5" for a checklist.
        let date_room = (date_end - PAD_X).max(0.0);
        line_text(
            NoteFont::Vale,
            &s.date,
            ox + PAD_X,
            oy,
            date_room,
            HEADER_H,
            10.5,
            400,
            t.secondary,
            (1.0 - s.header) * 0.95,
            false,
        );
        if let Some((done, total)) = s.progress {
            let count = format!("{done}/{total}");
            let dw = text_width(res, &s.date, 10.5, 400);
            let cw = text_width(res, &count, 10.5, 600);
            let x = PAD_X + dw + 8.0;
            if x + cw <= date_end && !s.rolled {
                line_text(
                    NoteFont::Vale,
                    &count,
                    ox + x,
                    oy,
                    cw + 2.0,
                    HEADER_H,
                    10.5,
                    600,
                    if done == total { t.accent } else { t.secondary },
                    (1.0 - s.header) * 0.95,
                    false,
                );
            }
        }
    }
    if let Some(st) = &s.stack {
        // Where the date sits: "1 / 3" always, with the arrows beside it while
        // the other buttons show.
        let strip = stack_strip();
        if let Ok(layout) = res.layout_tabular(&st.label, 10.5, 600, strip.counter.w) {
            unsafe {
                let _ = layout.SetWordWrapping(
                    windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_NO_WRAP,
                );
                let _ = layout.SetMaxHeight(HEADER_H);
                let _ = layout.SetParagraphAlignment(
                    windows::Win32::Graphics::DirectWrite::DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                );
                let _ = layout.SetTextAlignment(
                    windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER,
                );
                brush.SetColor(&color(t.secondary, 0.95));
                rt.DrawTextLayout(
                    Vector2::new(ox + strip.counter.x, oy),
                    &layout,
                    brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
        }
        if s.header > 0.01 {
            for (b, glyph) in [(Btn::Prev, Icon::ChevL), (Btn::Next, Icon::ChevR)] {
                let r = button_rect(b, s.w);
                if s.hot == Some(b) && s.header > 0.3 {
                    set(t.body, 0.10 * s.header);
                    unsafe {
                        rt.FillRoundedRectangle(&rr(ox + r.x, oy + r.y, r.w, r.h, 6.0), brush);
                    }
                }
                icon(glyph, r, t.secondary, s.header);
            }
        }
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
            Btn::List => {
                // A hamburger: three short horizontal lines.
                set(ink, a);
                for dy in [-4.0f32, 0.0, 4.0] {
                    let (cx, cy) = (ox + r.x + r.w / 2.0, oy + r.y + r.h / 2.0 + dy);
                    unsafe {
                        rt.DrawLine(
                            Vector2::new(cx - 5.0, cy),
                            Vector2::new(cx + 5.0, cy),
                            brush,
                            1.6,
                            None,
                        );
                    }
                }
            }
            Btn::Prev | Btn::Next => {}
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
            s.font,
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
            NoteFont::Vale,
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
        // The progress of a checklist, right above the text.
        if let Some((done, total)) = s.progress {
            let r = art::progress_rect(s.w);
            set(t.secondary, 0.22);
            unsafe {
                rt.FillRoundedRectangle(&rr(ox + r.x, oy + r.y, r.w, r.h, r.h / 2.0), brush);
            }
            let full = r.w * art::progress_fraction(done, total);
            if full > 0.0 {
                set(t.accent, 1.0);
                unsafe {
                    rt.FillRoundedRectangle(&rr(ox + r.x, oy + r.y, full, r.h, r.h / 2.0), brush);
                }
            }
        }
        draw_body(&rt, brush, res, s, ox, oy);
        let stamp_h = if s.bar.is_some() { s.h - (BAR_BLOCK - FOOTER_H) } else { s.h };
        art::draw_stamp(&rt, brush, res, s.stamp, s.lang, t, (ox, oy, s.w, stamp_h));
        // The footer, or the format bar in its place, and the resize grip.
        if let Some(bar) = &s.bar {
            draw_bar(&rt, brush, res, s, bar, ox, oy);
        } else {
            let foot = Rect {
                x: PAD_X,
                y: s.h - FOOTER_H,
                w: s.w - PAD_X - GRIP - 4.0,
                h: FOOTER_H,
            };
            line_text(NoteFont::Vale, &s.footer, ox + foot.x, oy + foot.y, foot.w, foot.h, 10.5, 400, t.secondary, 1.0, false);
        }
        set(if s.bar.is_some() { BAR_GLYPH } else { t.secondary }, 0.7);
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

    // The tape or the pin over the top edge.
    art::draw_deco(&rt, brush, s.deco, t, (ox, oy, s.w));

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
    // A drop would land here: the accent dashed outline inside the edge.
    if s.drop_hover
        && let Some(dash) = res.drop_dash.as_ref()
    {
        set(s.frame_accent, 1.0);
        let (x, y, fw, fh, r) = drop_frame(ox, oy, s.w, h);
        unsafe { rt.DrawRoundedRectangle(&rr(x, y, fw, fh, r), brush, DROP_W, dash) };
    }
    // A release here would tuck: an accent bar on that edge.
    if let Some(edge) = s.cue {
        let x = match edge {
            Edge::Left => ox,
            Edge::Right => ox + s.w - tuck::CUE_W,
        };
        set(t.accent, 1.0);
        unsafe {
            rt.FillRoundedRectangle(&rr(x, oy, tuck::CUE_W, h, tuck::CUE_W / 2.0), brush);
        }
    }
    if layer_open.is_some() {
        unsafe { rt.PopLayer() };
    }
    // Popping: the tab is still there, unscaled, fading as the note comes; and
    // over it a nearly clear fill of its rect so the pointer resting where the
    // faded tab was still reaches the window (the hit keeper).
    if let Some(tk) = s.tuck.filter(|tk| tk.progress < 1.0) {
        unsafe { rt.SetTransform(&straight) };
        if tk.tab_alpha > 0.0 {
            draw_tab(&rt, brush, s, &tk, tk.tab_alpha);
        }
        if tk.progress > 0.0 {
            let out = DipRect::new(0.0, 0.0, s.w, h);
            let tab = tuck::tab_rect(&out, tk.edge);
            set(t.fill, tuck::KEEPER_ALPHA);
            unsafe {
                rt.FillRoundedRectangle(&rr(m.0 + tab.x, m.1 + tab.y, tab.w, tab.h, 0.0), brush);
            }
        }
    }
    // The confetti is the window's, not the leaning note's.
    if !s.confetti.is_empty() {
        unsafe { rt.SetTransform(&straight) };
        art::draw_confetti(&rt, brush, s.confetti, &art::confetti_palette(t));
    }
    unsafe { rt.SetTransform(&Matrix3x2::identity()) };
}

/// The tab of a tucked note at its screen edge, in the window's own place (not
/// scaled by the pop): paper, a 1 dip ink line on the inner side, three
/// short grip lines, and an accent dot while the reminder is overdue. `a` is
/// its opacity.
fn draw_tab(
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    s: &Scene,
    tk: &TuckScene,
    a: f32,
) {
    let t = &s.tones;
    let m = s.margins;
    let out = DipRect::new(0.0, 0.0, s.w, shown_h(s.h, s.rolled));
    let tab = tuck::tab_rect(&out, tk.edge);
    let (x, y) = (m.0 + tab.x, m.1 + tab.y);
    // Round on the inner side only: the shape runs on past the screen edge,
    // where the window ends.
    let ext = 12.0;
    let sx = match tk.edge {
        Edge::Left => x - ext,
        Edge::Right => x,
    };
    let sw = tab.w + ext;
    let set = |rgb: u32, alpha: f32| unsafe { brush.SetColor(&color(rgb, alpha)) };
    for (grow, alpha) in [(5.0, 0.035), (3.0, 0.05), (1.5, 0.07)] {
        set(0x000000, alpha * a);
        unsafe {
            rt.FillRoundedRectangle(
                &rr(sx - grow, y - grow + 2.0, sw + 2.0 * grow, tab.h + 2.0 * grow, 7.0 + grow),
                brush,
            );
        }
    }
    set(t.fill, a);
    unsafe {
        rt.FillRoundedRectangle(&rr(sx, y, sw, tab.h, 7.0), brush);
    }
    // The ink line on the inner side.
    let line_x = match tk.edge {
        Edge::Left => x + tab.w - 0.5,
        Edge::Right => x + 0.5,
    };
    set(t.body, 0.5 * a);
    unsafe {
        rt.DrawLine(
            Vector2::new(line_x, y + 7.0),
            Vector2::new(line_x, y + tab.h - 7.0),
            brush,
            1.0,
            None,
        );
    }
    // Three short grip lines.
    let cx = x + tab.w / 2.0;
    let cy = y + tab.h / 2.0;
    set(t.body, 0.55 * a);
    for k in [-1.0f32, 0.0, 1.0] {
        unsafe {
            rt.DrawLine(
                Vector2::new(cx - 3.0, cy + k * 4.0),
                Vector2::new(cx + 3.0, cy + k * 4.0),
                brush,
                1.5,
                None,
            );
        }
    }
    if tk.overdue {
        set(t.accent, a);
        unsafe {
            rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2::new(cx, y + 12.0),
                    radiusX: 3.0,
                    radiusY: 3.0,
                },
                brush,
            );
        }
    }
}

fn draw_body(
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    res: &Res,
    s: &Scene,
    ox: f32,
    oy: f32,
) {
    let Some(body) = s.body else { return };
    let t = &s.tones;
    let area = body_rect_in(s.w, s.h, s.bar.is_some());
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
    // The ruling, under everything and scrolling with the text.
    art::draw_pattern(
        rt,
        brush,
        s.pattern,
        s.text_size,
        (ox + area.x, oy + area.y, area.w, area.h),
        s.scroll,
        t.line,
    );
    // The images, above the text, under their own clip: the grid is wider
    // than the text area (8 dip side padding against the text's 12).
    let clip = images_clip_in(s.w, s.h, s.bar.is_some());
    unsafe {
        rt.PopAxisAlignedClip();
        rt.PushAxisAlignedClip(
            &D2D_RECT_F {
                left: ox + clip.x,
                top: oy + clip.y,
                right: ox + clip.x + clip.w,
                bottom: oy + clip.y + clip.h,
            },
            D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        );
    }
    draw_images(rt, brush, res, s, ox, oy + area.y - s.scroll);
    unsafe {
        rt.PopAxisAlignedClip();
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
    // Highlights, behind the glyphs (and behind the selection). A done item
    // keeps its dimmed look whatever its runs say.
    for r in s.styles {
        let Some(h) = r.style.hl else { continue };
        unsafe { brush.SetColor(&color(styles::hl_rgb(h), 1.0)) };
        for q in body.ranges(r.s, r.e, false, false) {
            unsafe {
                rt.FillRoundedRectangle(&rr(bx + q.x - 1.0, by + q.y, q.w + 2.0, q.h, 2.0), brush);
            }
        }
    }
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
    // Text colours and the highlight's ink are drawing effects, made here:
    // they are brushes of this render target, so they are never cached with
    // the layout (a recreated canvas can not draw with a stale brush).
    let mut brushes: HashMap<u32, windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush> =
        HashMap::new();
    let mut effect = |rgb: u32| -> Option<IUnknown> {
        if let Some(b) = brushes.get(&rgb) {
            return b.cast().ok();
        }
        let b = unsafe { rt.CreateSolidColorBrush(&color(rgb, 1.0), None) }.ok()?;
        brushes.insert(rgb, b.clone());
        b.cast().ok()
    };
    for line in &body.lines {
        let top = by + line.y;
        if top > oy + area.y + area.h || top + line.h < oy + area.y {
            continue;
        }
        if let Some(l) = &line.layout
            && !line.raw.checked
            && !s.styles.is_empty()
        {
            for r in styles::slice(s.styles, line.raw.content_start(), line.raw.end) {
                let rgb = match (r.style.hl, r.style.fg) {
                    (Some(_), _) => Some(styles::HL_INK),
                    (None, Some(f)) => Some(styles::fg_rgb(f, s.dark_paper)),
                    (None, None) => None,
                };
                if let Some(eff) = rgb.and_then(&mut effect) {
                    let range = DWRITE_TEXT_RANGE {
                        startPosition: r.s as u32,
                        length: (r.e - r.s) as u32,
                    };
                    unsafe {
                        let _ = l.SetDrawingEffect(&eff, range);
                    }
                }
            }
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
        } else if line.raw.is_bullet() {
            // A dot where the marker is; the marker's text is never drawn.
            unsafe {
                brush.SetColor(&color(t.body, BULLET_ALPHA));
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2::new(bx + BOX / 2.0 - 2.0, top + line.first_h / 2.0),
                        radiusX: BULLET_DOT / 2.0,
                        radiusY: BULLET_DOT / 2.0,
                    },
                    brush,
                );
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

/// The images above the text: each cover-cropped into a rounded rect (a bitmap
/// brush filling it), scrolling with the text; the hovered one gets the accent
/// outline, the round ✕ and the "Buka" pill. `top` is the y (px-free dip, from
/// the window's top) of the text's first row.
fn draw_images(
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    res: &Res,
    s: &Scene,
    ox: f32,
    top: f32,
) {
    let Some(im) = &s.images else { return };
    let t = &s.tones;
    for (i, r) in im.rects.iter().enumerate() {
        let (x, y) = (ox + r.x, top + r.y);
        let Some(key) = im.keys.get(i) else { continue };
        let bitmap = {
            let mut c = im.cache.borrow_mut();
            if let Some(b) = c.bitmaps.get(key) {
                Some(b.clone())
            } else if let Some(pix) = c.decoded.get(key).cloned() {
                crate::shot_edit::gfx::bitmap(rt, pix.0, pix.1, &pix.2).ok().inspect(|b| {
                    c.bitmaps.insert(key.clone(), b.clone());
                })
            } else {
                None
            }
        };
        let drawn = bitmap.and_then(|bmp| {
            let size = unsafe { bmp.GetPixelSize() };
            if size.width == 0 || size.height == 0 {
                return None;
            }
            let (bw, bh) = (size.width as f32, size.height as f32);
            let k = s.scale;
            // Cover: the smaller side fills the cell, the rest is cropped
            // evenly; one bitmap pixel is one device pixel at this scale.
            let cover = ((r.w * k) / bw).max((r.h * k) / bh);
            let (dw, dh) = (bw * cover / k, bh * cover / k);
            let props = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };
            let place = windows::Win32::Graphics::Direct2D::D2D1_BRUSH_PROPERTIES {
                opacity: 1.0,
                transform: Matrix3x2 {
                    M11: cover / k,
                    M12: 0.0,
                    M21: 0.0,
                    M22: cover / k,
                    M31: x + (r.w - dw) / 2.0,
                    M32: y + (r.h - dh) / 2.0,
                },
            };
            let b = unsafe { rt.CreateBitmapBrush(&bmp, Some(&props), Some(&place)) }.ok()?;
            unsafe { rt.FillRoundedRectangle(&rr(x, y, r.w, r.h, IMG_RADIUS), &b) };
            Some(())
        });
        if drawn.is_none() {
            // A file that is gone or unreadable: a quiet tile with the icon.
            unsafe {
                brush.SetColor(&color(t.secondary, 0.16));
                rt.FillRoundedRectangle(&rr(x, y, r.w, r.h, IMG_RADIUS), brush);
            }
            put_icon(res, rt, brush, Icon::Image, Rect { x: x + r.w / 2.0 - 10.0, y: y + r.h / 2.0 - 10.0, w: 20.0, h: 20.0 }, 14.0, t.secondary, 0.8);
        }
        if im.hot == Some(i) {
            unsafe {
                brush.SetColor(&color(t.accent, 1.0));
                rt.DrawRoundedRectangle(&rr(x + 1.0, y + 1.0, r.w - 2.0, r.h - 2.0, IMG_RADIUS - 1.0), brush, 2.0, None);
            }
            // The round ✕, top right.
            let xr = image_remove_rect(Rect { x, y, w: r.w, h: r.h });
            let (cx, cy) = (xr.x + xr.w / 2.0, xr.y + xr.h / 2.0);
            unsafe {
                brush.SetColor(&color(IMG_SCRIM, 0.72));
                rt.FillEllipse(
                    &D2D1_ELLIPSE { point: Vector2::new(cx, cy), radiusX: IMG_X / 2.0, radiusY: IMG_X / 2.0 },
                    brush,
                );
                brush.SetColor(&color(0xFFFFFF, 1.0));
                for (a, b) in [((-3.5, -3.5), (3.5, 3.5)), ((-3.5, 3.5), (3.5, -3.5))] {
                    rt.DrawLine(
                        Vector2::new(cx + a.0, cy + a.1),
                        Vector2::new(cx + b.0, cy + b.1),
                        brush,
                        1.6,
                        None,
                    );
                }
            }
            // "Buka", bottom left.
            let label = text(s.lang, Key::FmtOpen);
            let tw = text_width(res, label, 10.0, 600);
            let (pw, ph) = (tw + 18.0, 18.0);
            let (px, py) = (x + 6.0, y + r.h - 6.0 - ph);
            if pw + 12.0 <= r.w && ph + 12.0 <= r.h {
                unsafe {
                    brush.SetColor(&color(IMG_SCRIM, 0.72));
                    rt.FillRoundedRectangle(&rr(px, py, pw, ph, ph / 2.0), brush);
                }
                put_text(res, rt, brush, label, Rect { x: px, y: py, w: pw, h: ph }, 10.0, 600, 0xFFFFFF, 1.0, true);
            }
        }
    }
}

/// The overlay colour of an image's ✕ and "Buka" pill (rgba(20,16,14,.72)).
const IMG_SCRIM: u32 = 0x14100E;
/// The format bar's colours: its fill (and on a Gelap note), a dim glyph, a
/// hovered glyph, the tooltip.
const BAR_FILL: u32 = 0x1F1B18;
const BAR_FILL_DARK: u32 = 0x2C2622;
const BAR_GLYPH: u32 = 0x9C918A;
const BAR_GLYPH_HOT: u32 = 0xE9E2DA;
const TIP_FILL: u32 = 0x14110F;

/// An icon in `r` (absolute dip) at `size`, in `ink`.
fn put_icon(
    res: &Res,
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    icon: Icon,
    r: Rect,
    size: f32,
    ink: u32,
    a: f32,
) {
    let _ = res.icons.draw(rt, brush, icon, r, size, ink, a, None);
}

/// `text` in `r` (absolute dip), one line, vertically centred, centred
/// horizontally when `center`, else from the left.
fn put_text(
    res: &Res,
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    text: &str,
    r: Rect,
    size: f32,
    weight: u16,
    rgb: u32,
    a: f32,
    center: bool,
) {
    if text.is_empty() || r.w <= 0.0 {
        return;
    }
    let Ok(layout) = res.layout(text, size, weight, r.w) else {
        return;
    };
    place_layout(rt, brush, &layout, r, rgb, a, center);
}

/// Draws `layout` one line, centred vertically in `r` (and horizontally when
/// `center`).
fn place_layout(
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    layout: &IDWriteTextLayout,
    r: Rect,
    rgb: u32,
    a: f32,
    center: bool,
) {
    use windows::Win32::Graphics::DirectWrite::{
        DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
        DWRITE_WORD_WRAPPING_NO_WRAP,
    };
    unsafe {
        let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let _ = layout.SetMaxWidth(r.w);
        let _ = layout.SetMaxHeight(r.h);
        let _ = layout.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        let _ = layout.SetTextAlignment(if center {
            DWRITE_TEXT_ALIGNMENT_CENTER
        } else {
            DWRITE_TEXT_ALIGNMENT_LEADING
        });
        brush.SetColor(&color(rgb, a));
        rt.DrawTextLayout(
            Vector2::new(r.x, r.y),
            layout,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }
}

/// The glyph of a bar button, centred in `r` (absolute dip), in `ink`.
fn draw_glyph(
    res: &Res,
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    bar: &BarScene,
    tool: Tool,
    r: Rect,
    ink: u32,
) {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    match tool {
        Tool::Bold => {
            if let Ok(l) = res.serif("B", 14.0, 700, false) {
                place_layout(rt, brush, &l, r, ink, 1.0, true);
            }
        }
        Tool::Italic => {
            if let Ok(l) = res.serif("I", 14.0, 400, true) {
                place_layout(rt, brush, &l, r, ink, 1.0, true);
            }
        }
        Tool::Underline => {
            if let Ok(l) = res.layout("U", 12.5, 600, r.w) {
                unsafe {
                    let _ = l.SetUnderline(true, DWRITE_TEXT_RANGE { startPosition: 0, length: 1 });
                }
                place_layout(rt, brush, &l, r, ink, 1.0, true);
            }
        }
        Tool::Strike => {
            if let Ok(l) = res.layout("ab", 12.0, 600, r.w) {
                unsafe {
                    let _ = l.SetStrikethrough(true, DWRITE_TEXT_RANGE { startPosition: 0, length: 2 });
                }
                place_layout(rt, brush, &l, r, ink, 1.0, true);
            }
        }
        Tool::Bullet => unsafe {
            brush.SetColor(&color(ink, 1.0));
            for dy in [-4.5f32, 0.0, 4.5] {
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2::new(cx - 5.5, cy + dy),
                        radiusX: 1.3,
                        radiusY: 1.3,
                    },
                    brush,
                );
                rt.DrawLine(
                    Vector2::new(cx - 2.0, cy + dy),
                    Vector2::new(cx + 6.0, cy + dy),
                    brush,
                    1.5,
                    None,
                );
            }
        },
        Tool::Check => put_icon(res, rt, brush, Icon::Check, r, 14.0, ink, 1.0),
        Tool::Marker => {
            let left = Rect { x: r.x, y: r.y - 1.0, w: toolbar::MARKER_APPLY_W, h: r.h };
            if let Ok(l) = res.serif("A", 13.0, 700, false) {
                place_layout(rt, brush, &l, left, ink, 1.0, true);
            }
            // The last highlight, under the glyph.
            unsafe {
                brush.SetColor(&color(styles::hl_rgb(bar.marker_hl), 1.0));
                rt.FillRoundedRectangle(
                    &rr(r.x + 3.0, r.y + r.h - 6.0, toolbar::MARKER_APPLY_W - 6.0, 3.0, 1.5),
                    brush,
                );
            }
            let chev = Rect {
                x: r.x + toolbar::MARKER_APPLY_W,
                y: r.y,
                w: r.w - toolbar::MARKER_APPLY_W,
                h: r.h,
            };
            put_icon(res, rt, brush, Icon::ChevD, chev, 8.0, ink, 1.0);
        }
        Tool::Image => put_icon(res, rt, brush, Icon::Image, r, 14.0, ink, 1.0),
    }
}

/// A small dark label (the tooltip and the footer message): `label` and a
/// dimmer `hint` after it, in a rounded box `rect` (absolute dip).
fn draw_tip(
    res: &Res,
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    rect: Rect,
    label: &str,
    hint: &str,
) {
    unsafe {
        brush.SetColor(&color(TIP_FILL, 0.96));
        rt.FillRoundedRectangle(&rr(rect.x, rect.y, rect.w, rect.h, 6.0), brush);
    }
    let lw = text_width(res, label, TIP_TEXT, 600);
    let inner = Rect { x: rect.x + 8.0, y: rect.y, w: rect.w - 16.0, h: rect.h };
    put_text(res, rt, brush, label, Rect { w: inner.w.min(lw + 2.0), ..inner }, TIP_TEXT, 600, 0xF4EEE8, 1.0, false);
    if !hint.is_empty() {
        let hx = inner.x + lw + 8.0;
        if hx + 8.0 < inner.x + inner.w {
            put_text(
                res,
                rt,
                brush,
                hint,
                Rect { x: hx, w: inner.x + inner.w - hx, ..inner },
                TIP_TEXT - 0.5,
                400,
                BAR_GLYPH,
                1.0,
                false,
            );
        }
    }
}

/// The tooltip's text size.
const TIP_TEXT: f32 = 10.5;

/// The format bar: the dark strip at the bottom of the note, its buttons (the
/// hovered one tinted, the ones "on" lit) and the footer message above the
/// bar's left end. A hovered button's tooltip is the shared tooltip window.
fn draw_bar(
    rt: &ID2D1RenderTarget,
    brush: &windows::Win32::Graphics::Direct2D::ID2D1SolidColorBrush,
    res: &Res,
    s: &Scene,
    bar: &BarScene,
    ox: f32,
    oy: f32,
) {
    let layout = toolbar::layout(s.w);
    let (bx, by) = (ox, oy + s.h - BAR_BLOCK);
    let fill = if s.dark_paper { BAR_FILL_DARK } else { BAR_FILL };
    let b = layout.bar;
    unsafe {
        brush.SetColor(&color(fill, 1.0));
        rt.FillRoundedRectangle(&rr(bx + b.x, by + b.y, b.w, b.h, toolbar::BAR_RADIUS), brush);
    }
    let abs = |r: Rect| Rect { x: bx + r.x, y: by + r.y, w: r.w, h: r.h };
    let hot_tool = match bar.hot {
        Some(BarHit::Tool(t)) => Some(t),
        Some(BarHit::MarkerMenu) => Some(Tool::Marker),
        _ => None,
    };
    for (i, tool) in Tool::ALL.iter().enumerate() {
        let Some(r) = toolbar::rect_of(&layout, *tool).map(abs) else {
            continue;
        };
        let on = bar.on[i];
        let hot = hot_tool == Some(*tool);
        if on || hot {
            unsafe {
                brush.SetColor(&color(0xFFFFFF, if on { 0.12 } else { 0.07 }));
                rt.FillRoundedRectangle(&rr(r.x, r.y, r.w, r.h, 6.0), brush);
            }
        }
        let ink = if on {
            0xFFFFFF
        } else if hot {
            BAR_GLYPH_HOT
        } else {
            BAR_GLYPH
        };
        draw_glyph(res, rt, brush, bar, *tool, r, ink);
    }
    for sep in &layout.seps {
        let r = abs(*sep);
        unsafe {
            brush.SetColor(&color(0xFFFFFF, 0.12));
            rt.FillRectangle(
                &D2D_RECT_F { left: r.x, top: r.y + 4.0, right: r.x + r.w, bottom: r.y + r.h - 4.0 },
                brush,
            );
        }
    }
    if let Some(more) = layout.more.map(abs) {
        let hot = bar.hot == Some(BarHit::More);
        if hot {
            unsafe {
                brush.SetColor(&color(0xFFFFFF, 0.07));
                rt.FillRoundedRectangle(&rr(more.x, more.y, more.w, more.h, 6.0), brush);
            }
        }
        unsafe {
            brush.SetColor(&color(if hot { BAR_GLYPH_HOT } else { BAR_GLYPH }, 1.0));
            for dx in [-5.0f32, 0.0, 5.0] {
                rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: Vector2::new(more.x + more.w / 2.0 + dx, more.y + more.h / 2.0),
                        radiusX: 1.4,
                        radiusY: 1.4,
                    },
                    brush,
                );
            }
        }
    }
    // A message (an image refusal, the delete's second click): a small label
    // above the bar's left end.
    if let Some(msg) = &bar.msg {
        let w = text_width(res, msg, TIP_TEXT, 600);
        let r = Rect {
            x: 6.0,
            y: s.h - BAR_BLOCK - 4.0 - toolbar::TOOLTIP_H,
            w: (w + 16.0).min(s.w - 12.0),
            h: toolbar::TOOLTIP_H,
        };
        draw_tip(res, rt, brush, Rect { x: ox + r.x, y: oy + r.y, w: r.w, h: r.h }, msg, "");
    }
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

/// The coat the peek uses: the configured coat, no accessory (a hat would be
/// cut by the note's edge).
pub fn peek_look(coat: CatColor) -> CatLook {
    CatLook {
        color: coat,
        accessory: CatAccessory::None,
        ..CatLook::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str, w: u32, h: u32) -> ImgKey {
        (name.to_owned(), w, h)
    }

    #[test]
    fn an_exact_size_is_used_whatever_else_is_there() {
        let want = key("a.png", 200, 150);
        let have = [key("a.png", 100, 75), want.clone()];
        for moving in [false, true] {
            for failed in [false, true] {
                assert_eq!(image_plan(&want, &have, failed, moving), ImgPlan::Use(want.clone()));
            }
        }
    }

    #[test]
    fn while_moving_the_nearest_size_stands_in_and_nothing_is_decoded() {
        let want = key("a.png", 200, 150);
        let have = [key("a.png", 100, 75), key("a.png", 180, 140), key("b.png", 200, 149)];
        assert_eq!(
            image_plan(&want, &have, false, true),
            ImgPlan::Use(key("a.png", 180, 140)),
            "the closest area of the same name"
        );
        // A tie in the area difference goes to the larger one.
        let tie = [key("a.png", 100, 100), key("a.png", 100, 300)];
        assert_eq!(
            image_plan(&key("a.png", 100, 200), &tie, false, true),
            ImgPlan::Use(key("a.png", 100, 300))
        );
        // Even a failed mark does not stop a size that is already there.
        assert_eq!(
            image_plan(&want, &have, true, true),
            ImgPlan::Use(key("a.png", 180, 140))
        );
    }

    #[test]
    fn with_nothing_decoded_a_move_decodes_once_then_stops() {
        let want = key("a.png", 200, 150);
        assert_eq!(image_plan(&want, &[], false, true), ImgPlan::Decode);
        // The sizes of other names are no help.
        let other = [key("b.png", 200, 150)];
        assert_eq!(image_plan(&want, &other, false, true), ImgPlan::Decode);
        // After a failure the name is not tried again on every move.
        assert_eq!(image_plan(&want, &[], true, true), ImgPlan::Skip);
    }

    #[test]
    fn once_settled_the_exact_size_is_decoded_even_when_another_is_there() {
        let want = key("a.png", 200, 150);
        let have = [key("a.png", 100, 75)];
        assert_eq!(image_plan(&want, &have, false, false), ImgPlan::Decode);
        assert_eq!(image_plan(&want, &have, true, false), ImgPlan::Skip, "failed once at this size");
    }

    #[test]
    fn a_recreated_canvas_keeps_the_pixels_and_the_failures() {
        let mut c = ImgCache::default();
        c.decoded.insert(key("a.png", 10, 10), Rc::new((10, 10, vec![0; 400])));
        c.failed.insert(key("b.png", 10, 10));
        c.drop_bitmaps();
        assert_eq!(c.decoded.len(), 1);
        assert!(c.failed.contains(&key("b.png", 10, 10)));
        assert!(c.bitmaps.is_empty());
    }

    #[test]
    fn the_cache_forgets_names_that_left_the_note_and_other_sizes_once_exact() {
        let px = || Rc::new((1, 1, vec![0; 4]));
        let mut c = ImgCache::default();
        for k in [key("a.png", 10, 10), key("a.png", 20, 20), key("b.png", 10, 10)] {
            c.decoded.insert(k, px());
        }
        c.failed.insert(key("gone.png", 5, 5));
        c.retain_names(&["a.png".to_owned(), "b.png".to_owned()]);
        assert_eq!(c.decoded.len(), 3);
        assert!(c.failed.is_empty());
        assert_eq!(c.keys_of("a.png"), [key("a.png", 10, 10), key("a.png", 20, 20)]);
        c.keep_only(&key("a.png", 20, 20));
        assert_eq!(c.keys_of("a.png"), [key("a.png", 20, 20)]);
        assert_eq!(c.keys_of("b.png"), [key("b.png", 10, 10)], "other names untouched");
        c.retain_names(&[]);
        assert!(c.decoded.is_empty());
    }

    #[test]
    fn the_drop_outline_is_two_dip_inset_one_inside_the_rounded_edge() {
        assert_eq!((DROP_W, DROP_DASH, DROP_GAP, DROP_INSET), (2.0, 6.0, 4.0, 1.0));
        let (x, y, w, h, r) = drop_frame(8.0, 38.0, 240.0, 208.0);
        // The stroke is centred on the path: 1 dip either side of it is the
        // outermost 2 dip of the note, never outside it.
        assert_eq!((x - DROP_W / 2.0, y - DROP_W / 2.0), (8.0, 38.0));
        assert_eq!((x + w + DROP_W / 2.0, y + h + DROP_W / 2.0), (248.0, 246.0));
        assert_eq!(r, RADIUS - DROP_INSET);
    }

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
    fn all_seven_buttons_fit_the_strip_without_overlap_at_every_width() {
        assert_eq!(Btn::ALL.len(), 7);
        assert_eq!(Btn::ALL[0], Btn::List, "the list is the leftmost");
        assert_eq!((BTN, BTN_GAP), (20.0, 1.0));
        for w in [160.0f32, 240.0, 260.0, 640.0] {
            let mut last = f32::MIN;
            for b in Btn::ALL {
                let r = button_rect(b, w);
                assert!(r.x >= 0.0 && r.x + r.w <= w, "{b:?} at {w}: {r:?}");
                assert!(r.y >= 0.0 && r.y + r.h <= HEADER_H, "{b:?} at {w}");
                assert!(r.x >= last, "{b:?} overlaps the one before at {w}");
                last = r.x + r.w;
            }
        }
        // At the smallest note the row spans x 8 .. 154.
        assert_eq!(button_rect(Btn::List, 160.0).x, 8.0);
        let close = button_rect(Btn::Close, 160.0);
        assert_eq!(close.x + close.w, 154.0);
    }

    #[test]
    fn the_list_button_takes_the_press_while_shown_and_the_strip_while_hidden() {
        for w in [160.0f32, 240.0, 260.0] {
            let r = button_rect(Btn::List, w);
            let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            assert_eq!(target(x, y, w, 208.0, false, None, 0.0, true), Target::Button(Btn::List));
            assert_eq!(target(x, y, w, 208.0, false, None, 0.0, false), Target::Header);
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
        let first = button_rect(Btn::List, w);
        assert_eq!(target(first.x - 3.0, 10.0, w, 208.0, false, None, 0.0, true), Target::Header);
        assert_eq!(target(20.0, 3.0, w, 208.0, false, None, 0.0, true), Target::Header);
        // Above and under the buttons, still inside the strip: the strip.
        assert_eq!(target(first.x + 5.0, 1.0, w, 208.0, false, None, 0.0, true), Target::Header);
        assert_eq!(
            target(first.x + 5.0, HEADER_H - 1.0, w, 208.0, false, None, 0.0, true),
            Target::Header
        );
    }


    /// How wide `label` is laid out with tabular figures, dip.
    fn counter_width(res: &Res, label: &str) -> f32 {
        let l = res.layout_tabular(label, 10.5, 600, 400.0).expect("a layout");
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { l.GetMetrics(&mut m) }.expect("metrics");
        m.widthIncludingTrailingWhitespace
    }

    #[test]
    fn a_stacks_counter_and_arrows_fit_left_of_the_seven_buttons_at_the_smallest_width() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        let strip = stack_strip();
        assert_eq!((strip.prev.w, strip.prev.h, strip.next.w, strip.next.h), (20.0, 20.0, 20.0, 20.0));
        // The widest counter, in the real font with tabular figures, fits its box.
        for label in ["20 / 20", "10 / 10", "1 / 3", "9 / 20"] {
            let w = counter_width(&res, label);
            assert!(w <= strip.counter.w, "{label} is {w} wide, the box is {}", strip.counter.w);
        }
        assert_eq!(counter_width(&res, "10 / 20"), counter_width(&res, "88 / 88"), "tabular: the figures do not jitter");
        // Left to right: previous, counter, next; none overlapping.
        assert!(strip.prev.x + strip.prev.w <= strip.counter.x);
        assert!(strip.counter.x + strip.counter.w <= strip.next.x);
        assert!(strip.prev.x >= 0.0 && strip.prev.y >= 0.0 && strip.prev.y + strip.prev.h <= HEADER_H);
        // At every width a stack can have, from `stack::MIN_W`: the last arrow
        // ends before the first of the seven right-hand buttons (the list's
        // hamburger) begins.
        let mut w = crate::widgets::stack::MIN_W;
        while w <= crate::widgets::sticky::MAX_W {
            let first = button_rect(Btn::List, w);
            assert!(strip.next.x + strip.next.w < first.x, "{w} wide: next ends at {}, List starts at {}", strip.next.x + strip.next.w, first.x);
            w += 10.0;
        }
        assert_eq!(button_rect(Btn::List, crate::widgets::stack::MIN_W).x, 108.0);
        // And it is the widest the 20 / 20 can be with both arrows: the same
        // layout would not fit at the plain minimum (160), which is why a
        // stack is at least 260.
        let first = button_rect(Btn::List, crate::widgets::sticky::MIN_W);
        assert!(strip.next.x + strip.next.w >= first.x, "a plain minimum note could not hold it");
    }

    #[test]
    fn a_stacks_arrows_take_presses_only_while_shown_and_only_on_a_stack() {
        let (w, h) = (280.0, 208.0);
        let at = |b: Btn| {
            let r = button_rect(b, w);
            (r.x + r.w / 2.0, r.y + r.h / 2.0)
        };
        for b in [Btn::Prev, Btn::Next] {
            let (x, y) = at(b);
            assert_eq!(target_in(x, y, w, h, false, None, 0.0, true, true), Target::Button(b));
            // Hidden: the strip, so it drags.
            assert_eq!(target_in(x, y, w, h, false, None, 0.0, false, true), Target::Header);
            // Not a stack: no arrows there.
            assert_eq!(target_in(x, y, w, h, false, None, 0.0, true, false), Target::Header);
            // Rolled up: the strip is the same.
            assert_eq!(target_in(x, y, w, 208.0, true, None, 0.0, true, true), Target::Button(b));
        }
        // The counter between them is the strip.
        let strip = stack_strip();
        assert_eq!(
            target_in(strip.counter.x + strip.counter.w / 2.0, 18.0, w, h, false, None, 0.0, true, true),
            Target::Header
        );
        // The seven buttons are where they were.
        for b in Btn::ALL {
            let r = button_rect(b, w);
            assert_eq!(
                target_in(r.x + r.w / 2.0, r.y + r.h / 2.0, w, h, false, None, 0.0, true, true),
                Target::Button(b)
            );
        }
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
        let (ww, wh) = window_dip(240.0, 208.0, margins());
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
    fn the_peek_has_the_coat_and_no_accessory() {
        assert_eq!(peek_look(CatColor::Calico).accessory, CatAccessory::None);
        assert_eq!(peek_look(CatColor::Calico).color, CatColor::Calico);
    }

    #[test]
    fn text_is_laid_out_and_clicks_map_back_to_indices() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        let text = "Belanja\n- [ ] bata\n- [x] kaca\nbiasa";
        let body = build_body(&res, text, NoteFont::Vale, TextSize::M, 200.0);
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
        let body = build_body(&res, text, NoteFont::Vale, TextSize::M, 400.0);
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
        let body = build_body(&res, text, NoteFont::Vale, TextSize::M, 400.0);
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
        let body = build_body(&res, text, NoteFont::Vale, TextSize::M, 400.0);
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
        let narrow = build_body(&res, long, NoteFont::Vale, TextSize::M, 120.0);
        let wide = build_body(&res, long, NoteFont::Vale, TextSize::M, 900.0);
        assert!(narrow.height > wide.height * 1.5);
    }

    // ------------------------------------------------- the format bar, images

    fn target_with(x: f32, y: f32, w: f32, h: f32, extra: &Extras) -> Target {
        target_full(x, y, w, h, false, None, 0.0, true, false, extra)
    }

    #[test]
    fn the_bar_takes_the_footers_place_and_shortens_the_text_area() {
        let plain = body_rect_in(240.0, 208.0, false);
        let with_bar = body_rect_in(240.0, 208.0, true);
        assert_eq!(plain, body_rect(240.0, 208.0));
        assert_eq!(plain.h, 208.0 - HEADER_H - FOOTER_H);
        assert_eq!(with_bar.h, 208.0 - HEADER_H - 42.0);
        assert_eq!((with_bar.x, with_bar.y, with_bar.w), (plain.x, plain.y, plain.w));
        assert_eq!(FOOTER_H, 24.0, "the footer keeps its height");
        // Never negative in a tiny note.
        assert_eq!(body_rect_in(160.0, 40.0, true).h, 0.0);
    }

    #[test]
    fn the_images_clip_holds_the_whole_grid_but_only_the_text_rows() {
        for (w, h, bar) in [(240.0, 208.0, false), (240.0, 208.0, true), (320.0, 400.0, true)] {
            let area = body_rect_in(w, h, bar);
            let clip = images_clip_in(w, h, bar);
            assert_eq!((clip.y, clip.h), (area.y, area.h), "bounded by the text rows");
            for n in 1..=4 {
                let (rects, _) = crate::widgets::images::grid(n, w, h, &[]);
                for r in rects {
                    assert!(r.x >= clip.x - 0.01 && r.right() <= clip.right() + 0.01, "{n}: {r:?} in {clip:?}");
                    // The old text-area clip cut the outer edge of the side cells.
                    assert!(r.x < area.x || r.right() > area.right());
                }
            }
        }
    }

    #[test]
    fn the_bar_is_a_target_only_while_it_shows() {
        let (w, h) = (240.0, 208.0);
        let l = toolbar::layout(w);
        let bold = toolbar::rect_of(&l, Tool::Bold).unwrap();
        let (x, y) = (bold.x + 4.0, h - BAR_BLOCK + bold.y + 4.0);
        let bar = Extras { bar: true, images: &[] };
        assert_eq!(target_with(x, y, w, h, &bar), Target::Bar(BarHit::Tool(Tool::Bold)));
        // Without the bar the same point is still the text (the 24 dip footer
        // is lower), and the footer is where it was.
        assert_eq!(target_with(x, y, w, h, &Extras::NONE), Target::Body);
        assert_eq!(target_with(x, h - 10.0, w, h, &Extras::NONE), Target::Footer);
        // The bar's background and margin are inert (never the footer's text
        // press, which would move the caret).
        assert_eq!(target_with(x + 100.0, y, w, h, &bar), Target::Bar(BarHit::Bar));
        assert_eq!(target_with(2.0, h - 2.0, w, h, &bar), Target::Bar(BarHit::Bar));
        // The text area above it is the text.
        assert_eq!(target_with(30.0, HEADER_H + 5.0, w, h, &bar), Target::Body);
        // Just above the block is still the body; the block starts at h - 42.
        assert_eq!(target_with(30.0, h - BAR_BLOCK - 1.0, w, h, &bar), Target::Body);
        assert!(matches!(target_with(30.0, h - BAR_BLOCK + 1.0, w, h, &bar), Target::Bar(_)));
        // A rolled note has no bar and no body.
        let rolled = target_full(x, 60.0, w, h, true, None, 0.0, true, false, &bar);
        assert_eq!(rolled, Target::Header);
    }

    #[test]
    fn a_bar_button_wins_over_the_grip_and_the_grip_over_the_bar_background() {
        let (w, h) = (235.0, 208.0);
        let l = toolbar::layout(w);
        let last = toolbar::rect_of(&l, Tool::Image).unwrap();
        let bar = Extras { bar: true, images: &[] };
        let grip = grip_rect(w, h);
        // The last button reaches into the grip's x range at 235 dip.
        assert!(last.right() + 0.0 > grip.x, "the geometry overlaps the grip");
        let (bx, by) = (grip.x + 1.0, h - BAR_BLOCK + last.y + 4.0);
        assert!(inside(Rect { x: last.x, y: h - BAR_BLOCK + last.y, w: last.w, h: last.h }, bx, by));
        assert_eq!(target_with(bx, by, w, h, &bar), Target::Bar(BarHit::Tool(Tool::Image)));
        // Where the grip is but no button: the grip.
        assert_eq!(target_with(w - 3.0, h - 3.0, w, h, &bar), Target::Grip);
        // The bar's background left of the grip is the bar.
        assert_eq!(target_with(w - 40.0, h - 3.0, w, h, &bar), Target::Bar(BarHit::Bar));
    }

    #[test]
    fn an_image_and_its_x_are_targets_after_the_scroll() {
        let (w, h) = (240.0, 300.0);
        let (rects, _) = crate::widgets::images::grid(2, w, h, &[]);
        let ex = Extras { bar: false, images: &rects };
        let area = body_rect_in(w, h, false);
        let r = rects[0];
        // The middle of the first image, unscrolled.
        let (x, y) = (r.x + r.w / 2.0, area.y + r.y + r.h / 2.0);
        assert_eq!(target_with(x, y, w, h, &ex), Target::Image(0));
        // Its x button wins over the image.
        let xr = image_remove_rect(Rect { x: r.x, y: area.y + r.y, w: r.w, h: r.h });
        assert_eq!(
            target_with(xr.x + xr.w / 2.0, xr.y + xr.h / 2.0, w, h, &ex),
            Target::ImageRemove(0)
        );
        assert_eq!((xr.w, xr.h), (20.0, 20.0));
        assert_eq!(xr.right(), r.right() - 5.0, "5 dip in from the right");
        assert_eq!(xr.y - (area.y + r.y), 5.0, "and from the top");
        // The second one.
        let r1 = rects[1];
        assert_eq!(
            target_with(r1.x + 4.0, area.y + r1.y + r1.h - 4.0, w, h, &ex),
            Target::Image(1)
        );
        // Scrolled 20 dip: the rects move up with the text.
        let scrolled = target_full(x, y - 20.0, w, h, false, None, 20.0, true, false, &ex);
        assert_eq!(scrolled, Target::Image(0));
        let moved_off = target_full(x, y, w, h, false, None, 20.0, true, false, &ex);
        assert_eq!(moved_off, Target::Image(0), "still inside the image, 20 dip lower");
        // Between the two images (the gap) it is the body.
        let gap_x = r.right() + 2.0;
        assert_eq!(target_with(gap_x, area.y + r.y + 10.0, w, h, &ex), Target::Body);
        // Below the block is the text, and an image scrolled out of the area is
        // not hit through the header.
        let far = target_full(x, HEADER_H - 1.0, w, h, false, None, 100.0, true, false, &ex);
        assert!(!matches!(far, Target::Image(_) | Target::ImageRemove(_)));
        // Without images nothing changes.
        assert_eq!(target_with(x, y, w, h, &Extras::NONE), Target::Body);
    }

    #[test]
    fn the_image_block_pushes_the_text_down_and_hits_follow() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        let text = "Title\n\u{2022} one\n- [ ] two";
        let plain = build_body(&res, text, NoteFont::Vale, TextSize::M, 200.0);
        let with = build_body_with(&res, text, NoteFont::Vale, TextSize::M, 200.0, 90.0, &[]);
        assert_eq!(with.lines.len(), plain.lines.len());
        for (a, b) in plain.lines.iter().zip(&with.lines) {
            assert!((b.y - a.y - 90.0).abs() < 0.01, "every line is 90 dip lower");
            assert_eq!(a.h, b.h);
            assert_eq!(a.x, b.x);
        }
        assert!((with.height - plain.height - 90.0).abs() < 0.01, "height includes the block");
        // A hit above the text is the first line; a caret is at its place.
        assert_eq!(with.index_at(5.0, 10.0), plain.index_at(5.0, 0.0));
        let (_, cy, _) = with.caret(0).unwrap();
        let (_, cy0, _) = plain.caret(0).unwrap();
        assert!((cy - cy0 - 90.0).abs() < 0.01);
        // The item's box follows the line.
        let item = &with.lines[2];
        let area = body_rect(240.0, 208.0);
        let r = box_rect(item, area, 0.0);
        let r0 = box_rect(&plain.lines[2], area, 0.0);
        assert!((r.y - r0.y - 90.0).abs() < 0.01);
        // A selection rect follows too.
        let sel = with.selection(0, 3);
        let sel0 = plain.selection(0, 3);
        assert!((sel[0].y - sel0[0].y - 90.0).abs() < 0.01);
        // An empty top is the old layout.
        let same = build_body_with(&res, text, NoteFont::Vale, TextSize::M, 200.0, 0.0, &[]);
        assert_eq!(same.height, plain.height);
    }

    #[test]
    fn a_bullet_line_is_indented_and_its_marker_is_never_a_hit_target() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        let text = "T\n\u{2022} item\n- [ ] box\nplain";
        let body = build_body(&res, text, NoteFont::Vale, TextSize::M, 200.0);
        assert_eq!(body.lines[1].x, BULLET_INDENT);
        assert_eq!(body.lines[2].x, INDENT);
        assert_eq!(body.lines[3].x, 0.0);
        // A bullet has no box to tick: only the checklist line is a `Check`.
        let area = body_rect(240.0, 208.0);
        let bullet = &body.lines[1];
        let r = Rect { x: area.x + 2.0, y: area.y + bullet.y + 3.0, w: 1.0, h: 1.0 };
        assert_eq!(
            target(r.x, r.y, 240.0, 208.0, false, Some(&body), 0.0, true),
            Target::Body
        );
        let item = &body.lines[2];
        let b = box_rect(item, area, 0.0);
        assert_eq!(
            target(b.x + 3.0, b.y + 3.0, 240.0, 208.0, false, Some(&body), 0.0, true),
            Target::Check(2)
        );
        // The caret on a bullet line sits after its marker.
        let (cx, _, _) = body.caret(text.find('i').map_or(0, |_| 4)).unwrap();
        assert!(cx >= BULLET_INDENT - 0.01, "{cx}");
    }

    #[test]
    fn formatting_changes_the_layout_it_is_drawn_from() {
        use crate::widgets::styles::{Run, Style};
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        let text = "Title\nthe quick brown fox jumps over the lazy dog again and again";
        let start = text.find("quick").unwrap();
        let bold = Style { b: true, ..Style::default() };
        let runs = [Run { s: start, e: text.encode_utf16().count(), style: bold }];
        let plain = build_body_with(&res, text, NoteFont::Vale, TextSize::M, 150.0, 0.0, &[]);
        let styled = build_body_with(&res, text, NoteFont::Vale, TextSize::M, 150.0, 0.0, &runs);
        // Bold is wider: the line is at least as tall (it may wrap further) and
        // the caret at its end is further right or lower.
        assert!(styled.lines[1].h >= plain.lines[1].h);
        let end = text.encode_utf16().count();
        let (x0, y0, _) = plain.caret(end).unwrap();
        let (x1, y1, _) = styled.caret(end).unwrap();
        assert!(x1 != x0 || y1 != y0, "the weight reached the layout");
        // A run over the title line is relative to the line: no panic and the
        // title is laid out as before in size.
        let on_title = [Run { s: 0, e: 5, style: bold }];
        let t = build_body_with(&res, text, NoteFont::Vale, TextSize::M, 150.0, 0.0, &on_title);
        assert_eq!(t.lines[0].h, plain.lines[0].h);
        // A run that crosses a bullet marker is clipped to the line's text.
        let b = "T\n\u{2022} abc";
        let crossing = [Run { s: 0, e: 100, style: bold }];
        let body = build_body_with(&res, b, NoteFont::Vale, TextSize::M, 150.0, 0.0, &crossing);
        assert_eq!(body.lines.len(), 2);
    }

    #[test]
    fn every_footer_message_fits_a_default_note_in_both_languages() {
        let res = Res::new(UiFont::Sans).expect("DirectWrite works without a window");
        for lang in [Lang::Id, Lang::En] {
            for key in [Key::FmtImgMax, Key::FmtImgTooBig, Key::FmtImgBad, Key::StickyDeleteAgain] {
                let w = text_width(&res, text(lang, key), TIP_TEXT, 600) + 16.0;
                assert!(w <= 240.0 - 12.0, "{lang:?} {key:?}: {w:.1} in a default note");
            }
        }
    }
}
