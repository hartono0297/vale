//! Drawing for the settings window: Direct2D on an `ID2D1HwndRenderTarget`
//! (software, so no GPU driver joins the process), DirectWrite text, and
//! every control the layout places. `settings.rs` owns the window and state
//! and hands each frame over as a `Scene`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS, D2D1_LINE_JOIN_ROUND,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_PRESENT_OPTIONS_IMMEDIATELY,
    D2D1_QUADRATIC_BEZIER_SEGMENT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1_SWEEP_DIRECTION,
    D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
    D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE, D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget,
    ID2D1Layer, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout, IDWriteTypography,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::Result;
use windows_numerics::{Matrix3x2, Vector2};

use super::cat_style::CatLook;
use super::fonts::{self, Face, Role};
use super::geometry::{
    BAR_COUNT, BAR_GAP, BAR_W, NotchSize, SHOULDER, ShapeKind, compact_layout,
    compact_layout_robot, idle_layout, shape_with_robot_sized,
};
use super::icons::{Icon, IconSet};
use super::notch_fill::legibility;
use super::robot::{Expr, Pose, RobotPainter};
use super::settings_layout::{
    Align, Button, ButtonKind, CARD_RADIUS, CAT_TILE, CONTROL_RADIUS, CatFace, Chip, Control,
    EXTERNAL_GLYPH, FULL_RADIUS, Field, Font, Group, HOTKEY_CAP_H, HOTKEY_CAP_PAD,
    HOTKEY_FIELD_PAD, HOTKEY_PLUS_W, Hit, LINK_GLYPH_W, Layout, MapItem, Measure, MonitorPick,
    Bar, Rect, SEGMENT_RADIUS, SLIDER_THUMB, SWATCH_RING_GAP, SWATCH_RING_W, Section, Segment, Text, Tone, WidgetTile, accent_key,
    hotkey_caps_w, slider_x,
};
use crate::config::{Style, UiFont};
use crate::i18n::{Key, Lang, text};
use crate::theme;
use crate::theme::{Accent, CatAccessory, CatColor, Palette, Surface, WindowPalette};

/// The Vale logo's fixed brand colours, whatever the accent: the terracotta
/// cat head and its ears, the espresso pill eyes.
const LOGO_HEAD: u32 = 0xE2683C;
const LOGO_EYE: u32 = 0x17120F;
/// The equalizer of the preview, frozen mid-beat (the design's sample
/// heights; the tallest is `BAR_MAX_H`).
const PREVIEW_BARS: [f32; BAR_COUNT] = [5.0, 11.0, 8.0, 13.0, 6.0];
/// A slider rail's thickness.
const SLIDER_RAIL_H: f32 = 4.0;

/// One frame's worth of what to draw.
pub struct Scene<'a> {
    pub layout: &'a Layout,
    pub p: WindowPalette,
    pub island: Palette,
    pub style: Style,
    /// The closed island's size and fill alpha (`notch_fill::fill_alpha`):
    /// the preview draws it that big, see-through when the alpha is below 1.
    pub notch: NotchSize,
    pub notch_alpha: f32,
    /// The cat is on: the preview shows it in the album art's place.
    pub robot: bool,
    /// The cat's colour and accessory in the preview.
    pub cat: CatLook,
    /// The face the Kucing preview shows.
    pub cat_face: CatFace,
    pub lang: Lang,
    pub hover: Option<Hit>,
    pub pressed: Option<Hit>,
    /// The sidebar selection, in section indexes (fractional while it slides).
    pub nav_pos: f32,
    pub section: Section,
    /// Section crossfade: content opacity and its upward offset.
    pub content_alpha: f32,
    pub content_dy: f32,
    /// Segmented selections in segment indexes, while they slide.
    pub segments: Vec<(Group, f32)>,
    /// Toggle knobs while they slide, 0 (off) to 1 (on), by the toggle's hit.
    pub toggles: Vec<(Hit, f32)>,
    pub focused: Option<Field>,
}

/// Device-independent resources plus the render target.
pub struct Gfx {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    /// The UI typeface the formats are made in.
    ui: UiFont,
    formats: HashMap<(Font, Align), IDWriteTextFormat>,
    /// Tabular figures, set on the digits of the fonts that line numbers up
    /// (`Font::tabular`). Only on the digits: DM Sans's `tnum` also widens
    /// the space, which would open up the gaps between words.
    tabular: IDWriteTypography,
    /// No features: every other run.
    plain: IDWriteTypography,
    /// Studio icon geometries, built on `d2d`.
    icons: IconSet,
    /// The cat, for the previews and the accessory tiles.
    robot: RobotPainter,
    /// The preview island's line of text.
    island_text: IDWriteTextFormat,
    widths: RefCell<HashMap<(String, Font), f32>>,
    /// The logo's two ear triangles in its 256-unit design space.
    ears: ID2D1PathGeometry,
    /// The logo's head, a dome on a flat base, in the same space.
    head: ID2D1PathGeometry,
    /// Round joins, for the ears' stroke.
    round: ID2D1StrokeStyle,
    target: Option<Target>,
}

struct Target {
    rt: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
    /// The layer a faded cat is drawn through (`Painter::cat`).
    layer: ID2D1Layer,
}

impl Gfx {
    /// The text formats of every `Font` style (and the preview island's
    /// line) in the UI typeface `ui`: DM Sans (display: 600, tracked) or
    /// JetBrains Mono (display: 600, not tracked) when embedded, else Segoe
    /// UI Variable.
    #[allow(clippy::type_complexity)]
    fn text_formats(
        dwrite: &IDWriteFactory,
        ui: UiFont,
    ) -> Result<(
        HashMap<(Font, Align), IDWriteTextFormat>,
        IDWriteTextFormat,
    )> {
        let text = Face::for_role_in(dwrite, Role::Text, ui);
        let display = Face::for_role_in(dwrite, Role::Display, ui);
        let mut formats = HashMap::new();
        for font in Font::ALL {
            let face = if font.display() { &display } else { &text };
            let weight = DWRITE_FONT_WEIGHT(i32::from(font.weight()));
            for (align, a) in [
                (Align::Left, DWRITE_TEXT_ALIGNMENT_LEADING),
                (Align::Center, DWRITE_TEXT_ALIGNMENT_CENTER),
            ] {
                let format = line_format(dwrite, face, weight, font.size(), a)?;
                formats.insert((font, align), format);
            }
        }
        let island_text = line_format(
            dwrite,
            &text,
            DWRITE_FONT_WEIGHT_NORMAL,
            12.0,
            DWRITE_TEXT_ALIGNMENT_LEADING,
        )?;
        Ok((formats, island_text))
    }

    pub fn new(ui: UiFont) -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let (formats, island_text) = Gfx::text_formats(&dwrite, ui)?;
            Ok(Gfx {
                icons: IconSet::new(&d2d)?,
                robot: RobotPainter::new(&d2d)?,
                island_text,
                ears: ears_geometry(&d2d)?,
                head: head_geometry(&d2d)?,
                round: d2d.CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        lineJoin: D2D1_LINE_JOIN_ROUND,
                        miterLimit: 10.0,
                        ..Default::default()
                    },
                    None,
                )?,
                tabular: fonts::tabular_figures(&dwrite)?,
                plain: dwrite.CreateTypography()?,
                d2d,
                dwrite,
                ui,
                formats,
                widths: RefCell::new(HashMap::new()),
                target: None,
            })
        }
    }

    /// Switches the UI typeface: the formats are rebuilt and the measured
    /// widths forgotten (they belong to the old font), only when `ui`
    /// differs. The caller lays out again.
    pub fn set_font(&mut self, ui: UiFont) -> Result<()> {
        if ui == self.ui {
            return Ok(());
        }
        let (formats, island_text) = Gfx::text_formats(&self.dwrite, ui)?;
        self.formats = formats;
        self.island_text = island_text;
        self.widths.borrow_mut().clear();
        self.ui = ui;
        Ok(())
    }

    /// One line of `text` in a `w` × `h` box in `style`: with `tabular`, its
    /// digits take tabular figures; a display style is tracked. Both the
    /// width cache and `draw_str` lay lines out here, so measured widths
    /// match what is drawn.
    fn line(&self, text: &[u16], style: TextStyle, w: f32, h: f32) -> Result<IDWriteTextLayout> {
        let layout = fonts::text_layout(&self.dwrite, text, style.format, w, h, &self.plain)?;
        if let Some(size) = style.tracked {
            fonts::apply_tracking_in(&layout, text.len() as u32, size, self.ui)?;
        }
        if style.tabular {
            for (start, len) in digit_runs(text) {
                let range = DWRITE_TEXT_RANGE {
                    startPosition: start,
                    length: len,
                };
                unsafe { layout.SetTypography(&self.tabular, range)? };
            }
        }
        Ok(layout)
    }

    /// The window was resized (physical pixels).
    pub fn resize(&self, width: u32, height: u32) {
        if let Some(t) = &self.target {
            unsafe {
                let _ = t.rt.Resize(&D2D_SIZE_U { width, height });
            }
        }
    }

    /// Forgets the target, e.g. after a DPI change; the next frame makes a new one.
    pub fn reset_target(&mut self) {
        self.target = None;
    }

    fn ensure_target(&mut self, hwnd: HWND, size: (u32, u32), dpi: f32) -> Result<&Target> {
        if self.target.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: dpi,
                dpiY: dpi,
                ..Default::default()
            };
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: D2D_SIZE_U {
                    width: size.0,
                    height: size.1,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };
            unsafe {
                let rt = self.d2d.CreateHwndRenderTarget(&props, &hwnd_props)?;
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
                let brush = rt.CreateSolidColorBrush(&color(0, 1.0), None)?;
                let layer = rt.CreateLayer(None)?;
                self.target = Some(Target { rt, brush, layer });
            }
        }
        Ok(self.target.as_ref().expect("target just made"))
    }

    /// Draws one frame. A lost target is dropped and remade on the next one.
    pub fn paint(&mut self, hwnd: HWND, size: (u32, u32), dpi: f32, scene: &Scene) -> Result<()> {
        self.ensure_target(hwnd, size, dpi)?;
        let target = self.target.as_ref().expect("ensured");
        let rt: &ID2D1RenderTarget = &target.rt;
        unsafe {
            rt.SetDpi(dpi, dpi);
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2::identity());
            rt.Clear(Some(&color(scene.p.bg, 1.0)));
        }
        let painter = Painter {
            rt,
            brush: &target.brush,
            layer: &target.layer,
            gfx: self,
            p: scene.p,
            alpha: 1.0,
        };
        painter.sidebar(scene);
        let content = Painter {
            alpha: scene.content_alpha,
            ..painter
        };
        unsafe {
            rt.SetTransform(&Matrix3x2::translation(0.0, scene.content_dy));
        }
        content.content(scene);
        let result = unsafe { rt.EndDraw(None, None) };
        if result.is_err() {
            self.target = None;
        }
        result
    }
}

impl Measure for Gfx {
    fn width(&self, s: &str, font: Font) -> f32 {
        let key = (s.to_owned(), font);
        if let Some(w) = self.widths.borrow().get(&key) {
            return *w;
        }
        let wide: Vec<u16> = s.encode_utf16().collect();
        let w = self
            .formats
            .get(&(font, Align::Left))
            .and_then(|format| unsafe {
                let style = TextStyle::of(format, font);
                let layout = self.line(&wide, style, 4096.0, 64.0).ok()?;
                let mut m = DWRITE_TEXT_METRICS::default();
                layout.GetMetrics(&mut m).ok()?;
                Some(m.widthIncludingTrailingWhitespace.ceil())
            })
            .unwrap_or_else(|| super::settings_layout::Estimate.width(s, font));
        self.widths.borrow_mut().insert(key, w);
        w
    }
}

#[derive(Clone, Copy)]
struct Painter<'a> {
    rt: &'a ID2D1RenderTarget,
    brush: &'a ID2D1SolidColorBrush,
    /// Made on `rt`; `cat` fades the cat through it.
    layer: &'a ID2D1Layer,
    gfx: &'a Gfx,
    p: WindowPalette,
    /// Multiplies every colour's alpha (the section crossfade).
    alpha: f32,
}

/// A text format with the OpenType features and spacing its lines carry.
#[derive(Clone, Copy)]
struct TextStyle<'a> {
    format: &'a IDWriteTextFormat,
    tabular: bool,
    /// The font size to track by (`fonts::DISPLAY_TRACKING_EM`): display
    /// fonts only.
    tracked: Option<f32>,
}

impl<'a> TextStyle<'a> {
    /// `format`, made for `font`, with that font's features and tracking.
    fn of(format: &'a IDWriteTextFormat, font: Font) -> Self {
        TextStyle {
            format,
            tabular: font.tabular(),
            tracked: font.display().then(|| font.size()),
        }
    }
}

impl Painter<'_> {
    fn set(&self, rgb: u32, a: f32) {
        unsafe { self.brush.SetColor(&color(rgb, a * self.alpha)) };
    }

    /// `0xRRGGBBAA`.
    fn set_rgba(&self, rgba: u32) {
        self.set(rgba >> 8, (rgba & 0xFF) as f32 / 255.0);
    }

    fn fill_round(&self, r: Rect, radius: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        unsafe {
            self.rt
                .FillRoundedRectangle(&rounded(r, radius), self.brush);
        }
    }

    /// A line of `width` just inside `r`, in the current brush colour.
    fn outline(&self, r: Rect, radius: f32, width: f32) {
        let half = width / 2.0;
        let inner = Rect {
            x: r.x + half,
            y: r.y + half,
            w: r.w - width,
            h: r.h - width,
        };
        unsafe {
            self.rt
                .DrawRoundedRectangle(&rounded(inner, radius - half), self.brush, width, None);
        }
    }

    /// A hairline just inside `r`.
    fn stroke_round(&self, r: Rect, radius: f32, rgba: u32) {
        self.set_rgba(rgba);
        self.outline(r, radius, 1.0);
    }

    fn circle(&self, cx: f32, cy: f32, radius: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        unsafe {
            self.rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2::new(cx, cy),
                    radiusX: radius,
                    radiusY: radius,
                },
                self.brush,
            );
        }
    }

    /// A pie slice of the disc at (`cx`, `cy`): from `from` to `to` degrees,
    /// clockwise from 12 o'clock. Built per paint; Settings paints rarely.
    fn pie(&self, cx: f32, cy: f32, radius: f32, from: f32, to: f32, rgb: u32) {
        let at = |deg: f32| {
            let a = deg.to_radians();
            Vector2::new(cx + radius * a.sin(), cy - radius * a.cos())
        };
        let build = || -> Result<ID2D1PathGeometry> {
            unsafe {
                let path = self.rt.GetFactory()?.CreatePathGeometry()?;
                let sink = path.Open()?;
                sink.BeginFigure(Vector2::new(cx, cy), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(at(from));
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: at(to),
                    size: D2D_SIZE_F {
                        width: radius,
                        height: radius,
                    },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: if to - from > 180.0 {
                        D2D1_ARC_SIZE_LARGE
                    } else {
                        D2D1_ARC_SIZE_SMALL
                    },
                });
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                sink.Close()?;
                Ok(path)
            }
        };
        if let Ok(path) = build() {
            self.set(rgb, 1.0);
            unsafe { self.rt.FillGeometry(&path, self.brush, None) };
        }
    }

    fn ring(&self, cx: f32, cy: f32, radius: f32, width: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        unsafe {
            self.rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2::new(cx, cy),
                    radiusX: radius,
                    radiusY: radius,
                },
                self.brush,
                width,
                None,
            );
        }
    }

    fn draw_str(&self, s: &str, style: TextStyle, r: Rect, rgb: u32, a: f32) {
        if s.is_empty() || r.w <= 0.0 {
            return;
        }
        let wide: Vec<u16> = s.encode_utf16().collect();
        // A layout rather than `DrawText`, to carry OpenType features; same
        // box, trimming and clip.
        let Ok(layout) = self.gfx.line(&wide, style, r.w, r.h) else {
            return;
        };
        self.set(rgb, a);
        unsafe {
            self.rt.DrawTextLayout(
                Vector2::new(r.x, r.y),
                &layout,
                self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
        }
    }

    /// A Studio icon `size` px square centred in `r`, faded with the painter;
    /// `duo` adds the window accent's duotone fill.
    fn icon(&self, icon: Icon, r: Rect, size: f32, rgb: u32, a: f32, duo: bool) {
        let accent = duo.then_some(self.p.accent);
        let alpha = a * self.alpha;
        let _ = self
            .gfx
            .icons
            .draw(self.rt, self.brush, icon, r, size, rgb, alpha, accent);
    }

    fn style(&self, font: Font, align: Align) -> TextStyle<'_> {
        TextStyle::of(&self.gfx.formats[&(font, align)], font)
    }

    fn text(&self, t: &Text) {
        let rgb = self.tone(t.tone);
        self.draw_str(&t.text, self.style(t.font, t.align), t.rect, rgb, 1.0);
    }

    /// Status colours from the Studio palette: sage, ochre and the window's
    /// error red. Light windows take the darker accent shade, pulled a little
    /// further toward the text so small type stays readable on the cards.
    fn tone(&self, tone: Tone) -> u32 {
        let p = &self.p;
        let status = |a: Accent| {
            if p.light {
                mix(a.light_rgb(), p.text, 0.25)
            } else {
                a.rgb()
            }
        };
        match tone {
            Tone::Primary => p.text,
            Tone::Secondary => p.secondary,
            Tone::Good => status(Accent::Sage),
            Tone::Warn => status(Accent::Ochre),
            Tone::Bad => p.error,
        }
    }

    /// Accent text on the window (light windows already use the darker shade).
    fn link(&self) -> u32 {
        self.p.accent
    }

    /// The selected sidebar item's fill (the design's "Pilih" token): the
    /// hover fill of a light window's buttons, the button fill of a dark one.
    fn selection(&self) -> u32 {
        if self.p.light {
            self.p.button_hover
        } else {
            self.p.button
        }
    }

    /// A fill one step off `base` toward the text colour (hover, press).
    fn tint(&self, base: u32, amount: f32) -> u32 {
        mix(base, self.p.text, amount)
    }

    // ---- Sidebar ----

    fn sidebar(&self, s: &Scene) {
        let l = s.layout;
        self.logo(l.logo);
        if let Some(brand) = &l.brand {
            self.text(brand);
        }
        if l.nav.is_empty() {
            return;
        }
        let sel = self.selection();
        for n in &l.nav {
            let selected = n.section == s.section;
            if !selected && s.hover == Some(Hit::Nav(n.section)) {
                let a = if s.pressed == s.hover { 0.8 } else { 0.5 };
                self.fill_round(n.pill, CONTROL_RADIUS, sel, a);
            }
        }
        // The selection slides between rows.
        let pill = rect_at(&l.nav.iter().map(|n| n.pill).collect::<Vec<_>>(), s.nav_pos);
        self.fill_round(pill, CONTROL_RADIUS, sel, 1.0);
        let style = self.style(Font::Nav, Align::Left);
        for n in &l.nav {
            self.icon(n.section.icon(), n.glyph, 16.0, self.p.text, 1.0, true);
            self.draw_str(&n.label.text, style, n.label.rect, self.p.text, 1.0);
        }
    }

    /// The app logo, the cat head (assets/logo.svg), scaled into `r`.
    fn logo(&self, r: Rect) {
        if r.w <= 0.0 {
            return;
        }
        let k = r.w / 256.0;
        let rt = self.rt;
        unsafe {
            let mut previous = Matrix3x2::default();
            rt.GetTransform(&mut previous);
            let local = Matrix3x2 {
                M11: k,
                M12: 0.0,
                M21: 0.0,
                M22: k,
                M31: r.x,
                M32: r.y,
            };
            rt.SetTransform(&(local * previous));
            // Ears: triangles with a 23-unit round stroke, behind the head.
            self.set(LOGO_HEAD, 1.0);
            rt.FillGeometry(&self.gfx.ears, self.brush, None);
            rt.DrawGeometry(&self.gfx.ears, self.brush, 23.0, &self.gfx.round);
            rt.FillGeometry(&self.gfx.head, self.brush, None);
            // Two plain pill eyes, 26 x 46.
            for x in [78.0, 152.0] {
                self.fill_round(
                    Rect {
                        x,
                        y: 147.5,
                        w: 26.0,
                        h: 46.0,
                    },
                    13.0,
                    LOGO_EYE,
                    1.0,
                );
            }
            rt.SetTransform(&previous);
        }
    }

    // ---- Content ----

    fn content(&self, s: &Scene) {
        let l = s.layout;
        if let Some(title) = &l.title {
            self.text(title);
        }
        // A scrolled section draws inside its viewport only.
        if let Some(v) = l.viewport {
            unsafe {
                self.rt
                    .PushAxisAlignedClip(&d2d_rect(v), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            }
        }
        self.cards_and_controls(s);
        if l.viewport.is_some() {
            unsafe { self.rt.PopAxisAlignedClip() };
            if let Some(bar) = l.scrollbar() {
                self.fill_round(bar, FULL_RADIUS, self.p.secondary, 0.35);
            }
        }
    }

    /// Everything of the section below its title.
    fn cards_and_controls(&self, s: &Scene) {
        let l = s.layout;
        for card in &l.cards {
            self.fill_round(*card, CARD_RADIUS, self.p.card, 1.0);
            self.stroke_round(*card, CARD_RADIUS, self.p.border);
        }
        for d in &l.dividers {
            self.set_rgba(self.p.border);
            unsafe { self.rt.FillRectangle(&d2d_rect(*d), self.brush) };
        }
        // The row a drag holds sits on a raised plate over the others.
        if let Some(r) = l.lifted {
            self.fill_round(r, CONTROL_RADIUS, self.p.field, 1.0);
            self.stroke_round(r, CONTROL_RADIUS, self.p.border);
        }
        for g in &l.glyphs {
            let a = if g.quiet { 0.45 } else { 1.0 };
            self.icon(g.icon, g.rect, g.rect.w, self.tone(g.tone), a, false);
        }
        if let Some(r) = l.cat_preview {
            self.cat_preview(r, s);
        }
        if let Some(r) = l.preview {
            self.preview(r, s);
        }
        for t in &l.tiles {
            self.widget_tile(t, s);
        }
        if let Some(r) = l.about_logo {
            self.logo(r);
        }
        if let Some(r) = l.note_glyph {
            self.icon(Icon::Info, r, 14.0, self.p.secondary, 1.0, true);
        }
        for (i, t) in l.texts.iter().enumerate() {
            match s.hover {
                // The accent row names the swatch under the pointer.
                Some(Hit::Accent(a)) if l.accent_desc == Some(i) => {
                    let name = text(s.lang, accent_key(a));
                    let style = self.style(t.font, t.align);
                    self.draw_str(name, style, t.rect, self.p.text, 1.0);
                }
                _ => self.text(t),
            }
        }
        for chip in &l.chips {
            self.chip(chip);
        }
        for tag in &l.tags {
            self.tag(tag);
        }
        for bar in &l.bars {
            self.bar(*bar);
        }
        for (field, r) in &l.fields {
            self.field(*r, s.focused == Some(*field));
        }
        for c in &l.controls {
            match c {
                Control::Segmented {
                    group,
                    track,
                    items,
                    selected,
                    enabled,
                } => {
                    let pos = s
                        .segments
                        .iter()
                        .find(|(g, _)| g == group)
                        .map_or(*selected as f32, |(_, v)| *v);
                    self.dimmed(*enabled).segmented(*track, items, pos, s);
                }
                Control::Toggle {
                    hit,
                    rect,
                    on,
                    enabled,
                } => {
                    let v = s
                        .toggles
                        .iter()
                        .find(|(h, _)| h == hit)
                        .map_or(f32::from(u8::from(*on)), |(_, v)| *v);
                    let hover = *enabled && s.hover == Some(*hit);
                    self.dimmed(*enabled).toggle(*rect, v, hover)
                }
                Control::Stepper {
                    minus,
                    plus,
                    value,
                    enabled,
                } => {
                    self.step_button(minus.0, Icon::Min, minus.1, s);
                    self.step_button(plus.0, Icon::Plus, plus.1, s);
                    let a = if *enabled { 1.0 } else { 0.45 };
                    let style = self.style(value.font, value.align);
                    self.draw_str(&value.text, style, value.rect, self.p.text, a);
                }
                Control::Swatches { items, selected } => {
                    for (accent, r) in items {
                        self.swatch(
                            *r,
                            *accent,
                            *accent == *selected,
                            s.hover == Some(Hit::Accent(*accent)),
                        );
                    }
                }
                Control::SurfaceSwatches {
                    items,
                    selected,
                    enabled,
                } => {
                    let p = self.dimmed(*enabled);
                    for (surface, r) in items {
                        let hover = *enabled && s.hover == Some(Hit::Surface(*surface));
                        p.surface_swatch(*r, *surface, *surface == *selected, hover, s);
                    }
                }
                Control::CatSwatches {
                    items,
                    selected,
                    enabled,
                } => {
                    let p = self.dimmed(*enabled);
                    for (color, r) in items {
                        let hover = *enabled && s.hover == Some(Hit::CatColor(*color));
                        p.cat_swatch(*r, *color, *color == *selected, hover);
                    }
                }
                Control::AccessoryTiles {
                    items,
                    selected,
                    enabled,
                } => {
                    let p = self.dimmed(*enabled);
                    for (accessory, r) in items {
                        let hover = *enabled && s.hover == Some(Hit::CatAccessory(*accessory));
                        p.accessory_tile(*r, *accessory, *accessory == *selected, hover, s);
                    }
                }
                Control::Slider {
                    id,
                    track,
                    value,
                    min,
                    max,
                    enabled,
                } => {
                    let hit = Hit::Slider(*id);
                    let hovered = *enabled && s.hover == Some(hit);
                    let held = hovered && s.pressed == Some(hit);
                    self.dimmed(*enabled)
                        .slider(*track, (*value, *min, *max), hovered, held);
                }
                Control::Button(b) => self.button(b, s),
                Control::HotkeyField {
                    hit,
                    rect,
                    caps,
                    recording,
                } => self.hotkey_field(*rect, caps, *recording, *hit, s),
                Control::Mini {
                    rect,
                    rects,
                    selected,
                    canvas,
                } => self.mini(*rect, rects, *selected, *canvas),
                Control::MonitorMap { items, lit, .. } => self.monitor_map(items, *lit, s),
                Control::Radio {
                    hit,
                    rect,
                    selected,
                } => self.radio(*rect, *hit, *selected, s),
                // Only a click area: its card is `Layout::tiles`.
                Control::Area { .. } => {}
                Control::IconButton {
                    hit,
                    rect,
                    icon,
                    enabled,
                    bare,
                } => {
                    if *bare {
                        // A drag handle: only dots, darker under the pointer.
                        let near = s.hover == Some(*hit) || s.pressed == Some(*hit);
                        let ink = if near { self.p.text } else { self.p.secondary };
                        self.icon(*icon, *rect, 16.0, ink, 1.0, false);
                    } else {
                        self.step_button(*rect, *icon, enabled.then_some(*hit), s);
                    }
                }
            }
        }
    }

    /// The cat filling the square `area` at this painter's alpha. The
    /// passes (rim, silhouette, patches, face) overlap, so they are drawn
    /// opaque and the whole cat is faded at once; at less than full alpha
    /// each pass would show through the next.
    fn cat(&self, area: Rect, expr: Expr, pose: Pose, look: CatLook) {
        let paint = || {
            self.gfx
                .robot
                .paint(self.rt, self.brush, area, expr, pose, 1.0, look);
        };
        if self.alpha >= 1.0 {
            paint();
            return;
        }
        if self.alpha <= 0.0 {
            return;
        }
        // Half the cat's size round it: room for the ears, the accessories
        // and the tilt, while the layer stays the size of the tile.
        let m = area.w / 2.0;
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: D2D_RECT_F {
                left: area.x - m,
                top: area.y - m,
                right: area.x + area.w + m,
                bottom: area.y + area.h + m,
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: self.alpha,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS_NONE,
        };
        unsafe { self.rt.PushLayer(&params, self.layer) };
        paint();
        unsafe { self.rt.PopLayer() };
    }

    /// This painter, faded for a control that is off.
    fn dimmed(&self, enabled: bool) -> Painter<'_> {
        Painter {
            alpha: if enabled {
                self.alpha
            } else {
                self.alpha * 0.45
            },
            ..*self
        }
    }

    /// A fully rounded status chip on a faint wash of its tone.
    /// A small label on a wash of the accent ("disarankan").
    fn tag(&self, t: &Text) {
        let (ink, wash) = tag_colors(&self.p);
        self.fill_round(t.rect, FULL_RADIUS, self.p.accent, wash);
        self.draw_str(&t.text, self.style(t.font, t.align), t.rect, ink, 1.0);
    }

    /// A download's progress: a faint track, the part done in the accent.
    fn bar(&self, bar: Bar) {
        self.fill_round(bar.rect, FULL_RADIUS, self.p.secondary, BAR_TRACK);
        let done = Rect {
            w: bar.rect.w * bar.fill.clamp(0.0, 1.0),
            ..bar.rect
        };
        if done.w > 0.0 {
            self.fill_round(done, FULL_RADIUS, self.p.accent, 1.0);
        }
    }

    fn chip(&self, chip: &Chip) {
        let rgb = self.tone(chip.label.tone);
        let wash = if self.p.light { 0.12 } else { 0.16 };
        self.fill_round(chip.rect, FULL_RADIUS, rgb, wash);
        let d = chip.dot;
        self.circle(d.x + d.w / 2.0, d.y + d.h / 2.0, d.w / 2.0, rgb, 1.0);
        self.text(&chip.label);
    }

    /// A hotkey row's field: the segmented control's track, the combo as
    /// small rounded key caps with a "+" between (plain text when the caps do
    /// not fit), "Tidak ada" when the row is off, and while recording the
    /// prompt under an accent border. A hover state tints the track.
    fn hotkey_field(&self, r: Rect, caps: &[String], recording: bool, hit: Hit, s: &Scene) {
        draw_segment_track(self.rt, self.brush, &self.p, r, self.alpha);
        if !recording && s.hover == Some(hit) {
            let a = if s.pressed == s.hover { 0.10 } else { 0.06 };
            self.fill_round(r, CONTROL_RADIUS, self.p.text, a);
        }
        if recording {
            self.set(self.p.accent, 1.0);
            self.outline(r, CONTROL_RADIUS, 1.5);
        }
        let style = self.style(Font::Control, Align::Center);
        if recording {
            let prompt = text(s.lang, Key::HotkeyRecording);
            self.draw_str(prompt, style, r, self.p.text, 1.0);
        } else if caps.is_empty() {
            let none = text(s.lang, Key::HotkeyNone);
            self.draw_str(none, style, r, self.p.secondary, 1.0);
        } else if hotkey_caps_w(self.gfx, caps) <= r.w {
            let total = hotkey_caps_w(self.gfx, caps) - 2.0 * HOTKEY_FIELD_PAD;
            let mut x = r.x + (r.w - total) / 2.0;
            for (i, cap) in caps.iter().enumerate() {
                let w = self.gfx.width(cap, Font::Control) + 2.0 * HOTKEY_CAP_PAD;
                let cap_r = Rect {
                    x,
                    y: r.y + (r.h - HOTKEY_CAP_H) / 2.0,
                    w,
                    h: HOTKEY_CAP_H,
                };
                self.fill_round(cap_r, 6.0, self.p.button, 1.0);
                self.stroke_round(cap_r, 6.0, self.p.border);
                self.draw_str(cap, style, cap_r, self.p.text, 1.0);
                x += w;
                if i + 1 < caps.len() {
                    let plus = Rect {
                        x,
                        y: r.y,
                        w: HOTKEY_PLUS_W,
                        h: r.h,
                    };
                    self.draw_str("+", style, plus, self.p.secondary, 1.0);
                    x += HOTKEY_PLUS_W;
                }
            }
        } else {
            // Plain text; a smaller face when even that is wider than the field.
            let plain = caps.join("+");
            let small = self.gfx.width(&plain, Font::Control) > r.w - 2.0 * HOTKEY_FIELD_PAD;
            let style = if small {
                self.style(Font::Desc, Align::Center)
            } else {
                style
            };
            self.draw_str(&plain, style, r, self.p.text, 1.0);
        }
    }

    /// The track (window background, hairline, radius 10) with the selected
    /// segment (`segment_fill`, radius 8) sliding inside it.
    fn segmented(&self, track: Rect, items: &[Segment], pos: f32, s: &Scene) {
        draw_segment_track(self.rt, self.brush, &self.p, track, self.alpha);
        let rects: Vec<Rect> = items.iter().map(|i| i.rect).collect();
        for item in items {
            let current = (pos - index_of(items, item) as f32).abs() < 0.5;
            if !current && s.hover == Some(item.hit) {
                let a = if s.pressed == s.hover { 0.12 } else { 0.07 };
                self.fill_round(item.rect, SEGMENT_RADIUS, self.p.text, a);
            }
        }
        let sel = rect_at(&rects, pos);
        self.fill_round(sel, SEGMENT_RADIUS, segment_fill(&self.p), 1.0);
        let style = self.style(Font::Control, Align::Center);
        for item in items {
            // Text turns to ink as the selection slides under it.
            let covered = overlap(sel, item.rect) / item.rect.w.max(1.0);
            let rgb = mix(self.p.text, self.p.on_accent, covered.clamp(0.0, 1.0));
            self.draw_str(&item.label, style, item.rect, rgb, 1.0);
        }
    }

    /// On: an accent track with an `on_accent` knob. Off: a 1.5 px outline
    /// and a smaller knob, both in the secondary text colour. Cross-faded by `v`.
    fn toggle(&self, r: Rect, v: f32, hover: bool) {
        draw_toggle(self.rt, self.brush, &self.p, r, v, hover, self.alpha);
    }

    /// Susun's miniature: the tiles of a layout in `r`; the editor's canvas
    /// (`canvas`) has a field background and lights the selected tile.
    fn mini(
        &self,
        r: Rect,
        rects: &[crate::arrange::NRect],
        selected: Option<usize>,
        canvas: bool,
    ) {
        if canvas {
            self.fill_round(r, 8.0, self.p.field, 1.0);
            self.stroke_round(r, 8.0, self.p.border);
        }
        let gap = if canvas { 2.0 } else { 1.0 };
        let radius = if canvas { 6.0 } else { 3.0 };
        for (i, n) in rects.iter().enumerate() {
            let tile = Rect {
                x: r.x + n.x * r.w + gap,
                y: r.y + n.y * r.h + gap,
                w: (n.w * r.w - 2.0 * gap).max(1.0),
                h: (n.h * r.h - 2.0 * gap).max(1.0),
            };
            let lit = selected == Some(i);
            let (ink, alpha) = if lit {
                (self.p.accent, 0.35)
            } else {
                (self.p.secondary, 0.28)
            };
            self.fill_round(tile, radius, ink, alpha);
            if lit {
                self.set(self.p.accent, 1.0);
                self.outline(tile, radius, 2.0);
            }
        }
    }

    /// The Monitor map: each screen a rounded rectangle (its number is a
    /// text of the layout). The one the notch is on is filled and outlined in
    /// the accent; one that cannot be chosen is faint.
    fn monitor_map(&self, items: &[MapItem], lit: Option<usize>, s: &Scene) {
        const RADIUS: f32 = 4.0;
        for item in items {
            let is_lit = lit == Some(item.index);
            let hit = Hit::NotchMonitor(MonitorPick::Entry(item.index as u8));
            let (ink, alpha) = if is_lit {
                (self.p.accent, 0.35)
            } else if item.selectable {
                (self.p.secondary, 0.28)
            } else {
                (self.p.secondary, 0.12)
            };
            self.fill_round(item.rect, RADIUS, ink, alpha);
            if item.selectable && !is_lit && s.hover == Some(hit) {
                let a = if s.pressed == s.hover { 0.12 } else { 0.07 };
                self.fill_round(item.rect, RADIUS, self.p.text, a);
            }
            if is_lit {
                self.set(self.p.accent, 1.0);
                self.outline(item.rect, RADIUS, 2.0);
            }
            let ink = if item.selectable {
                self.p.text
            } else {
                self.p.secondary
            };
            let style = self.style(Font::Value, Align::Center);
            self.draw_str(&item.number.to_string(), style, item.rect, ink, 1.0);
        }
    }

    /// One entry of the Monitor list: a radio dot (the chosen one filled in
    /// the accent) and a soft wash under the pointer. The label and the muted
    /// note are texts of the layout.
    fn radio(&self, r: Rect, hit: Option<Hit>, selected: bool, s: &Scene) {
        if hit.is_some() && s.hover == hit {
            let a = if s.pressed == s.hover { 0.10 } else { 0.06 };
            self.fill_round(r, CONTROL_RADIUS, self.p.text, a);
        }
        // On a tall row the dot stays by its first line.
        let (cx, cy) = (r.x + 16.0, r.y + (r.h / 2.0).min(16.0));
        if selected {
            self.ring(cx, cy, 6.75, 1.5, self.p.accent, 1.0);
            self.circle(cx, cy, 3.5, self.p.accent, 1.0);
        } else {
            let a = if hit.is_some() { 0.85 } else { 0.4 };
            self.ring(cx, cy, 6.75, 1.5, self.p.secondary, a);
        }
    }

    /// A slider: a rail with the part up to the thumb in the accent, and a
    /// round accent thumb that grows a pixel under the pointer or while held.
    /// `range` is (value, min, max).
    fn slider(&self, track: Rect, range: (i32, i32, i32), hovered: bool, held: bool) {
        let (value, min, max) = range;
        let cy = track.y + track.h / 2.0;
        let x = slider_x(track, value, min, max);
        let rail = Rect {
            x: track.x + SLIDER_THUMB / 2.0,
            y: cy - SLIDER_RAIL_H / 2.0,
            w: track.w - SLIDER_THUMB,
            h: SLIDER_RAIL_H,
        };
        self.fill_round(rail, FULL_RADIUS, self.p.secondary, 0.3);
        let filled = Rect {
            w: (x - rail.x).max(0.0),
            ..rail
        };
        self.fill_round(filled, FULL_RADIUS, self.p.accent, 1.0);
        let grow = if held || hovered { 1.0 } else { 0.0 };
        let r = SLIDER_THUMB / 2.0 - 1.5 + grow;
        // A card-coloured halo, so the thumb stands off the accent part.
        self.circle(x, cy, r + 1.5, self.p.card, 1.0);
        self.circle(x, cy, r, self.p.accent, 1.0);
    }

    /// A round `−` / `+` button: card fill and hairline, darker on hover.
    fn step_button(&self, r: Rect, icon: Icon, hit: Option<Hit>, s: &Scene) {
        let hovered = hit.is_some() && s.hover == hit;
        let pressed = hovered && s.pressed == hit;
        let fill = if pressed {
            self.p.button_hover
        } else if hovered {
            self.p.button
        } else {
            self.p.card
        };
        let a = if hit.is_some() { 1.0 } else { 0.45 };
        self.fill_round(r, FULL_RADIUS, fill, a);
        self.stroke_round(r, FULL_RADIUS, self.p.border);
        self.icon(icon, r, 12.0, self.p.text, a, true);
    }

    /// A gallery card: its plate, and on it the still picture of a note (a
    /// cream sticky note with a title and two checklist lines; never animated).
    fn widget_tile(&self, t: &WidgetTile, s: &Scene) {
        let hover = t.active && s.hover == Some(Hit::WidgetCard);
        self.fill_round(t.rect, CONTROL_RADIUS + 2.0, self.p.bg, 1.0);
        if hover {
            self.set(self.p.accent, 1.0);
            self.outline(t.rect, CONTROL_RADIUS + 2.0, 1.5);
        } else {
            self.stroke_round(t.rect, CONTROL_RADIUS + 2.0, self.p.border);
        }
        // The desk the note sits on.
        self.fill_round(t.preview, CONTROL_RADIUS, self.p.button, 1.0);
        let ink = 0x2B211A;
        let (nw, nh) = (136.0f32.min(t.preview.w - 16.0), 84.0f32.min(t.preview.h - 12.0));
        let note = Rect {
            x: t.preview.x + (t.preview.w - nw) / 2.0,
            y: t.preview.y + (t.preview.h - nh) / 2.0,
            w: nw,
            h: nh,
        };
        self.fill_round(Rect { y: note.y + 2.0, ..note }, 8.0, 0x000000, 0.12);
        self.fill_round(note, 8.0, 0xF4E7C6, 1.0);
        let title = Rect {
            x: note.x + 10.0,
            y: note.y + 6.0,
            w: note.w - 20.0,
            h: 18.0,
        };
        self.draw_str(
            "Belanja",
            self.style(Font::Label, Align::Left),
            title,
            ink,
            1.0,
        );
        for (i, (label, done)) in [("Bata ekspos", true), ("Kaca low-e", false)].into_iter().enumerate() {
            let y = note.y + 28.0 + i as f32 * 20.0;
            let bx = Rect {
                x: note.x + 10.0,
                y: y + 3.0,
                w: 11.0,
                h: 11.0,
            };
            if done {
                self.fill_round(bx, 3.0, Accent::Terracotta.light_rgb(), 1.0);
                self.set(0xF4E7C6, 1.0);
                unsafe {
                    self.rt.DrawLine(
                        Vector2::new(bx.x + 2.6, bx.y + 5.6),
                        Vector2::new(bx.x + 4.6, bx.y + 7.6),
                        self.brush,
                        1.4,
                        None,
                    );
                    self.rt.DrawLine(
                        Vector2::new(bx.x + 4.6, bx.y + 7.6),
                        Vector2::new(bx.x + 8.6, bx.y + 3.4),
                        self.brush,
                        1.4,
                        None,
                    );
                }
            } else {
                self.set(0x6B5A4C, 1.0);
                self.outline(bx, 3.0, 1.2);
            }
            let line = Rect {
                x: bx.right() + 7.0,
                y,
                w: note.right() - bx.right() - 14.0,
                h: 17.0,
            };
            let alpha = if done { 0.62 } else { 1.0 };
            self.draw_str(label, self.style(Font::Desc, Align::Left), line, ink, alpha);
            if done {
                let w = self.gfx.width(label, Font::Desc).min(line.w);
                self.set(ink, 0.62);
                self.fill_round(
                    Rect {
                        x: line.x,
                        y: y + 8.0,
                        w,
                        h: 1.0,
                    },
                    0.0,
                    ink,
                    0.62,
                );
            }
        }
    }

    /// An accent dot; the chosen one gets a text-coloured ring with a
    /// card-coloured gap, the one under the pointer a fainter ring.
    fn swatch(&self, r: Rect, accent: Accent, selected: bool, hover: bool) {
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let ring = r.w / 2.0 + SWATCH_RING_GAP + SWATCH_RING_W / 2.0;
        if selected {
            self.ring(cx, cy, ring, SWATCH_RING_W, self.p.text, 1.0);
        } else if hover {
            self.ring(cx, cy, ring, SWATCH_RING_W, self.p.secondary, 0.7);
        }
        self.circle(cx, cy, r.w / 2.0, accent.rgb(), 1.0);
    }

    /// The ring a chosen swatch or tile gets: text-coloured, with a gap; the
    /// one under the pointer a fainter one.
    fn choice_ring(&self, cx: f32, cy: f32, radius: f32, selected: bool, hover: bool) {
        if selected {
            self.ring(cx, cy, radius, SWATCH_RING_W, self.p.text, 1.0);
        } else if hover {
            self.ring(cx, cy, radius, SWATCH_RING_W, self.p.secondary, 0.7);
        }
    }

    /// A base-color dot: the surface itself, with a hairline so a dark fill
    /// stands off the card. "Ikuti kucing" is the color it gives now, with a
    /// small cat outline.
    fn surface_swatch(&self, r: Rect, surface: Surface, selected: bool, hover: bool, s: &Scene) {
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let ring = r.w / 2.0 + SWATCH_RING_GAP + SWATCH_RING_W / 2.0;
        self.choice_ring(cx, cy, ring, selected, hover);
        let shown = surface.resolve(s.cat.color, s.robot);
        self.circle(cx, cy, r.w / 2.0, shown.rgb(), 1.0);
        self.ring(cx, cy, r.w / 2.0 - 0.5, 1.0, self.p.secondary, 0.5);
        if surface == Surface::Auto {
            let ink = theme::island(Accent::default(), shown).secondary;
            self.icon(Icon::Cat, r, 14.0, ink, 1.0, false);
        }
    }

    /// A coat dot: the cat's head colour (Calico a conic of cream, orange and
    /// dark, Black its rim).
    fn cat_swatch(&self, r: Rect, color: CatColor, selected: bool, hover: bool) {
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let ring = r.w / 2.0 + SWATCH_RING_GAP + SWATCH_RING_W / 2.0;
        self.choice_ring(cx, cy, ring, selected, hover);
        let look = CatLook {
            color,
            accessory: CatAccessory::None,
        };
        let radius = r.w / 2.0;
        if let Some(rim) = look.rim() {
            self.circle(cx, cy, radius, rim, 1.0);
            self.circle(cx, cy, radius - 1.6, look.head_rgb(), 1.0);
        } else {
            self.circle(cx, cy, radius, look.head_rgb(), 1.0);
        }
        if color == CatColor::Calico {
            // The site's conic chip: slice 0 is the cream disc already drawn;
            // each later colour paints from its start to 12 o'clock again, so
            // the overlap hides any seam between slices.
            for &(from, _, rgb) in &super::cat_style::calico_swatch_slices()[1..] {
                self.pie(cx, cy, radius, from, 360.0, rgb);
            }
        }
        self.ring(cx, cy, radius - 0.5, 1.0, self.p.secondary, 0.35);
    }

    /// An accessory's tile: the cat in the current coat wearing it, on the
    /// island's own surface; the chosen one is outlined.
    fn accessory_tile(
        &self,
        r: Rect,
        accessory: CatAccessory,
        selected: bool,
        hover: bool,
        s: &Scene,
    ) {
        let fill = s.island.surface;
        self.fill_round(r, CONTROL_RADIUS, fill, 1.0);
        if selected {
            self.set(self.p.accent, 1.0);
            self.outline(r, CONTROL_RADIUS, 2.0);
        } else if hover {
            self.set(self.p.secondary, 0.7);
            self.outline(r, CONTROL_RADIUS, 1.5);
        } else {
            self.stroke_round(r, CONTROL_RADIUS, self.p.border);
        }
        let size = 40.0;
        let area = Rect {
            x: r.x + (r.w - size) / 2.0,
            y: r.y + (r.h - size) / 2.0,
            w: size,
            h: size,
        };
        self.cat(
            area,
            Expr::Idle,
            Pose::default(),
            CatLook {
                color: s.cat.color,
                accessory,
            },
        );
    }

    /// The Kucing preview: the cat about 64 px on the surface it will have,
    /// the idle notch at its real size (style, width, height, fill) with the
    /// clock beside the cat, and the face the chips chose. Still: nothing here
    /// moves or runs a timer.
    fn cat_preview(&self, card: Rect, s: &Scene) {
        let pal = s.island;
        // The tile: the surface with the cat on it.
        let tile = Rect {
            x: card.x + 16.0,
            y: card.y + 12.0,
            w: CAT_TILE,
            h: CAT_TILE,
        };
        self.fill_round(tile, CARD_RADIUS, pal.surface, 1.0);
        self.stroke_round(tile, CARD_RADIUS, self.p.border);
        let (expr, blink) = s.cat_face.expr();
        let pose = Pose {
            blink,
            ..Pose::default()
        };
        let cat = Rect {
            x: tile.x + (tile.w - 64.0) / 2.0,
            y: tile.y + (tile.h - 64.0) / 2.0,
            w: 64.0,
            h: 64.0,
        };
        // With the cat off it is faded like the controls that are off, as
        // the notch beside it drops the cat.
        self.dimmed(s.robot).cat(cat, expr, pose, s.cat);

        // The screen sketch, with the notch at the top.
        let screen = Rect {
            x: tile.right() + 12.0,
            y: tile.y,
            w: card.right() - 16.0 - (tile.right() + 12.0),
            h: CAT_TILE,
        };
        let wash = if self.p.light { 0.18 } else { 0.14 };
        let (top, bottom) = (self.p.button, mix(self.p.bg, pal.accent, wash));
        self.gradient_round(screen, CONTROL_RADIUS, top, bottom);
        self.stroke_round(screen, CONTROL_RADIUS, self.p.border);
        let base = shape_with_robot_sized(s.style, s.notch, ShapeKind::Idle, s.robot);
        let shape = super::geometry::ShapeRect {
            x: screen.x + (screen.w - base.w) / 2.0,
            y: screen.y + base.y,
            ..base
        };
        if s.notch_alpha < 1.0 {
            self.checker(Rect {
                x: shape.x - 24.0,
                y: screen.y,
                w: shape.w + 48.0,
                h: shape.y - screen.y + shape.h + 8.0,
            });
        }
        self.set(pal.surface, s.notch_alpha);
        unsafe {
            match s.style {
                Style::Pill => {
                    self.rt.FillRoundedRectangle(
                        &rounded(
                            Rect {
                                x: shape.x,
                                y: shape.y,
                                w: shape.w,
                                h: shape.h,
                            },
                            shape.h / 2.0,
                        ),
                        self.brush,
                    );
                }
                Style::Notch => {
                    if let Ok(path) = notch_path(&self.gfx.d2d, shape) {
                        self.rt.FillGeometry(&path, self.brush, None);
                    }
                }
            }
        }
        let l = idle_layout(shape, s.robot, true);
        if let Some(area) = l.robot {
            self.cat(area, expr, pose, s.cat);
        }
        if let Some(clock) = l.clock {
            let style = TextStyle {
                format: &self.gfx.island_text,
                tabular: true,
                tracked: None,
            };
            self.draw_str("14:32", style, clock, pal.text, 0.9);
        }
    }

    fn button(&self, b: &Button, s: &Scene) {
        let hovered = b.enabled && s.hover == Some(b.hit);
        let pressed = hovered && s.pressed == Some(b.hit);
        let a = if b.enabled { 1.0 } else { 0.4 };
        let style = self.style(Font::Control, Align::Center);
        // Secondary fill: the button token, one step on hover and press.
        let quiet = if pressed {
            self.tint(self.p.button_hover, 0.06)
        } else if hovered {
            self.p.button_hover
        } else {
            self.p.button
        };
        match b.kind {
            ButtonKind::Primary => {
                let base = primary_fill(&self.p);
                let fill = if pressed {
                    self.tint(base, 0.2)
                } else if hovered {
                    self.tint(base, 0.1)
                } else {
                    base
                };
                self.fill_round(b.rect, FULL_RADIUS, fill, a);
                self.draw_str(&b.label, style, b.rect, self.p.on_accent, a);
            }
            ButtonKind::Secondary | ButtonKind::Confirm => {
                // The confirming "Yakin?" keeps the quiet fill in error red.
                let ink = if b.kind == ButtonKind::Confirm {
                    self.p.error
                } else {
                    self.p.text
                };
                let fill = if b.kind == ButtonKind::Confirm {
                    confirm_fill(&self.p, hovered, pressed)
                } else {
                    quiet
                };
                self.fill_round(b.rect, FULL_RADIUS, fill, a);
                self.draw_str(&b.label, style, b.rect, ink, a);
            }
            ButtonKind::Link | ButtonKind::Disclosure(_) => {
                if hovered {
                    let fa = if pressed { 0.1 } else { 0.06 };
                    self.fill_round(b.rect, FULL_RADIUS, self.p.text, fa);
                }
                let rgb = self.link();
                let label = Rect {
                    w: b.rect.w - LINK_GLYPH_W,
                    ..b.rect
                };
                let inset = Rect {
                    x: label.x + 8.0,
                    w: label.w - 8.0,
                    ..label
                };
                self.draw_str(
                    &b.label,
                    self.style(Font::Control, Align::Left),
                    inset,
                    rgb,
                    a,
                );
                let glyph = Rect {
                    x: b.rect.right() - LINK_GLYPH_W - 6.0,
                    w: LINK_GLYPH_W,
                    ..b.rect
                };
                let icon = match b.kind {
                    ButtonKind::Disclosure(true) => Icon::ChevD,
                    ButtonKind::Disclosure(false) => Icon::ChevR,
                    _ => Icon::Ext,
                };
                self.icon(icon, glyph, 12.0, rgb, a, true);
            }
            ButtonKind::External => {
                if hovered {
                    self.fill_round(b.rect, FULL_RADIUS, quiet, a);
                }
                let ink = if hovered {
                    self.p.text
                } else {
                    self.p.secondary
                };
                self.icon(Icon::Ext, b.rect, EXTERNAL_GLYPH, ink, a, false);
            }
        }
    }

    /// A rounded rect filled with a top-to-bottom gradient.
    fn gradient_round(&self, r: Rect, radius: f32, top: u32, bottom: u32) {
        let stops = [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: color(top, self.alpha),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: color(bottom, self.alpha),
            },
        ];
        unsafe {
            let Ok(stops) = self.rt.CreateGradientStopCollection(
                &stops,
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            ) else {
                return self.fill_round(r, radius, bottom, 1.0);
            };
            let Ok(brush) = self.rt.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: Vector2::new(r.x, r.y),
                    endPoint: Vector2::new(r.x, r.bottom()),
                },
                None,
                &stops,
            ) else {
                return self.fill_round(r, radius, bottom, 1.0);
            };
            self.rt.FillRoundedRectangle(&rounded(r, radius), &brush);
        }
    }

    /// The rounded box a native edit sits in (field fill, hairline, radius
    /// 10); an accent outline while focused.
    fn field(&self, r: Rect, focused: bool) {
        self.fill_round(r, CONTROL_RADIUS, self.p.field, 1.0);
        if focused {
            self.set(self.p.accent, 1.0);
            self.outline(r, CONTROL_RADIUS, 1.5);
        } else {
            self.stroke_round(r, CONTROL_RADIUS, self.p.border);
        }
    }

    /// The compact island on a sketch of the top of a screen, in the chosen
    /// style, fill and accent.
    fn preview(&self, card: Rect, s: &Scene) {
        let screen = Rect {
            x: card.x + 12.0,
            y: card.y + 12.0,
            w: card.w - 24.0,
            h: card.h - 24.0,
        };
        // A wallpaper-like wash from the window's tones that picks up the
        // accent toward the bottom.
        let wash = if self.p.light { 0.18 } else { 0.14 };
        let (top, bottom) = (self.p.button, mix(self.p.bg, s.island.accent, wash));
        self.gradient_round(screen, CONTROL_RADIUS, top, bottom);
        self.stroke_round(screen, CONTROL_RADIUS, self.p.border);

        // The island, at its real size (the sliders' width and height; they
        // top out well inside the card), centered at the top of the "screen".
        let base = shape_with_robot_sized(s.style, s.notch, ShapeKind::MediaCompact, s.robot);
        let shape = super::geometry::ShapeRect {
            x: screen.x + (screen.w - base.w) / 2.0,
            y: screen.y + base.y,
            ..base
        };
        let pal = s.island;
        // A see-through fill needs something behind it to show: a checker
        // strip across the top of the "screen".
        if s.notch_alpha < 1.0 {
            self.checker(Rect {
                x: shape.x - 24.0,
                y: screen.y,
                w: shape.w + 48.0,
                h: shape.y - screen.y + shape.h + 8.0,
            });
        }
        self.set(pal.surface, s.notch_alpha);
        unsafe {
            match s.style {
                Style::Pill => {
                    self.rt.FillRoundedRectangle(
                        &rounded(
                            Rect {
                                x: shape.x,
                                y: shape.y,
                                w: shape.w,
                                h: shape.h,
                            },
                            shape.h / 2.0,
                        ),
                        self.brush,
                    );
                }
                Style::Notch => {
                    if let Ok(path) = notch_path(&self.gfx.d2d, shape) {
                        self.rt.FillGeometry(&path, self.brush, None);
                    }
                }
            }
        }
        let l = if s.robot {
            compact_layout_robot(shape)
        } else {
            compact_layout(shape)
        };
        let art = l.art;
        if s.robot {
            // The cat listening, as on the island.
            self.cat(art, Expr::Music, Pose::default(), s.cat);
        } else {
            // Album art: an accent tile, darker toward the bottom, with a note.
            self.fill_round(art, 5.0, mix(pal.accent, pal.surface, 0.35), 1.0);
            self.fill_round(
                Rect {
                    h: art.h * 0.55,
                    ..art
                },
                5.0,
                pal.accent,
                0.9,
            );
            self.icon(Icon::Music, art, 12.0, pal.on_accent, 0.95, false);
        }
        let label = text(s.lang, Key::PreviewTrack);
        let style = TextStyle {
            format: &self.gfx.island_text,
            tabular: false,
            tracked: None,
        };
        // The island's own shadow under its text and bars when it is thin.
        let shadow = legibility(s.notch_alpha);
        if shadow > 0.0 {
            let lower = Rect {
                y: l.text.y + 1.0,
                ..l.text
            };
            self.draw_str(label, style, lower, 0x000000, 0.55 * shadow);
        }
        self.draw_str(label, style, l.text, pal.text, 0.7);
        for (i, h) in PREVIEW_BARS.into_iter().enumerate() {
            let bar = Rect {
                x: l.bars.x + i as f32 * (BAR_W + BAR_GAP),
                y: l.bars.bottom() - h,
                w: BAR_W,
                h,
            };
            if shadow > 0.0 {
                let lower = Rect {
                    y: bar.y + 1.0,
                    ..bar
                };
                self.fill_round(lower, 1.0, 0x000000, 0.55 * shadow);
            }
            self.fill_round(bar, 1.0, pal.accent, 1.0);
        }
    }

    /// Light and dark squares over `r`, cut at its edges.
    fn checker(&self, r: Rect) {
        const CELL: f32 = 8.0;
        let (light, dark) = (
            mix(self.p.bg, self.p.text, 0.30),
            mix(self.p.bg, self.p.text, 0.10),
        );
        let mut row = 0;
        let mut y = r.y;
        while y < r.bottom() {
            let h = CELL.min(r.bottom() - y);
            let mut x = r.x;
            let mut col = 0;
            while x < r.right() {
                let w = CELL.min(r.right() - x);
                let rgb = if (row + col) % 2 == 0 { light } else { dark };
                self.set(rgb, 1.0);
                unsafe {
                    self.rt
                        .FillRectangle(&d2d_rect(Rect { x, y, w, h }), self.brush)
                };
                x += w;
                col += 1;
            }
            y += h;
            row += 1;
        }
    }
}

/// Window-palette primitives on any render target, shared with the editors
/// (`editor_paint`): every colour's alpha is multiplied by `alpha`.
#[derive(Clone, Copy)]
pub(super) struct Pen<'a> {
    pub rt: &'a ID2D1RenderTarget,
    pub brush: &'a ID2D1SolidColorBrush,
    pub alpha: f32,
}

impl Pen<'_> {
    pub fn set(&self, rgb: u32, a: f32) {
        unsafe { self.brush.SetColor(&color(rgb, a * self.alpha)) };
    }

    pub fn fill_round(&self, r: Rect, radius: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        unsafe {
            self.rt
                .FillRoundedRectangle(&rounded(r, radius), self.brush)
        };
    }

    /// A line of `width` just inside `r`, in the current brush colour.
    pub fn outline(&self, r: Rect, radius: f32, width: f32) {
        let half = width / 2.0;
        let inner = Rect {
            x: r.x + half,
            y: r.y + half,
            w: r.w - width,
            h: r.h - width,
        };
        unsafe {
            self.rt
                .DrawRoundedRectangle(&rounded(inner, radius - half), self.brush, width, None);
        }
    }

    pub fn circle(&self, cx: f32, cy: f32, radius: f32, rgb: u32, a: f32) {
        self.set(rgb, a);
        unsafe {
            self.rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: Vector2::new(cx, cy),
                    radiusX: radius,
                    radiusY: radius,
                },
                self.brush,
            );
        }
    }
}

/// The settings toggle. On: an accent track with an `on_accent` knob. Off:
/// a 1.5 px outline and a smaller knob, both in the secondary text colour.
/// Cross-faded by `v` (0 off, 1 on).
pub(super) fn draw_toggle(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    p: &WindowPalette,
    r: Rect,
    v: f32,
    hover: bool,
    alpha: f32,
) {
    let pen = Pen { rt, brush, alpha };
    let radius = r.h / 2.0;
    if v < 1.0 {
        if hover {
            pen.fill_round(r, radius, p.text, 0.06 * (1.0 - v));
        }
        pen.set(p.secondary, 1.0 - v);
        pen.outline(r, radius, 1.5);
    }
    if v > 0.0 {
        let fill = if hover {
            mix(p.accent, p.text, 0.12)
        } else {
            p.accent
        };
        pen.fill_round(r, radius, fill, v);
    }
    let t = ease(v);
    let cx = r.x + radius + (r.w - r.h) * t;
    let rgb = mix(p.secondary, p.on_accent, v);
    let (off, on) = (r.h * 0.28, r.h / 2.0 - 3.0);
    let knob = off + (on - off) * t + if hover { 0.5 } else { 0.0 };
    pen.circle(cx, r.y + radius, knob, rgb, 1.0);
}

/// A segmented control's track: window background, hairline, radius 10.
/// The selected segment goes on top in the accent at `SEGMENT_RADIUS`.
pub(super) fn draw_segment_track(
    rt: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    p: &WindowPalette,
    track: Rect,
    alpha: f32,
) {
    let pen = Pen { rt, brush, alpha };
    pen.fill_round(track, CONTROL_RADIUS, p.bg, 1.0);
    pen.set(p.border >> 8, (p.border & 0xFF) as f32 / 255.0);
    pen.outline(track, CONTROL_RADIUS, 1.0);
}

/// `(start, length)` of each run of ASCII digits in UTF-16 `text`.
fn digit_runs(text: &[u16]) -> Vec<(u32, u32)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (i, c) in text.iter().enumerate() {
        let digit = (u16::from(b'0')..=u16::from(b'9')).contains(c);
        match (digit, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                runs.push((s as u32, (i - s) as u32));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s as u32, (text.len() - s) as u32));
    }
    runs
}

fn index_of(items: &[Segment], item: &Segment) -> usize {
    items
        .iter()
        .position(|i| std::ptr::eq(i, item))
        .unwrap_or(0)
}

/// The rect at fractional index `pos` along `rects` (blending neighbours).
pub fn rect_at(rects: &[Rect], pos: f32) -> Rect {
    if rects.is_empty() {
        return Rect::default();
    }
    let max = (rects.len() - 1) as f32;
    let pos = pos.clamp(0.0, max);
    let i = pos.floor() as usize;
    let j = (i + 1).min(rects.len() - 1);
    let t = pos - i as f32;
    let (a, b) = (rects[i], rects[j]);
    let m = |x: f32, y: f32| x + (y - x) * t;
    Rect {
        x: m(a.x, b.x),
        y: m(a.y, b.y),
        w: m(a.w, b.w),
        h: m(a.h, b.h),
    }
}

fn overlap(a: Rect, b: Rect) -> f32 {
    (a.right().min(b.right()) - a.x.max(b.x)).max(0.0)
}

/// Cubic ease-out.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// The share of the secondary ink a progress bar's track takes.
const BAR_TRACK: f32 = 0.15;

/// The confirming "Yakin?" button's fill. Hover and press move it toward the
/// card (lighter in a light window, darker in a dark one), the way the error
/// label gains contrast; the usual step toward the text would drop it below
/// 4.5:1 (light 4.31, Terminal dark 4.37).
pub fn confirm_fill(p: &WindowPalette, hovered: bool, pressed: bool) -> u32 {
    if pressed {
        p.card
    } else if hovered {
        mix(p.button, p.card, 0.5)
    } else {
        p.button
    }
}

/// A primary button's fill. A light window darkens the accent a step: its
/// light label on the light accent itself is only 4.26:1 (Terracotta).
pub fn primary_fill(p: &WindowPalette) -> u32 {
    if p.light {
        mix(p.accent, p.text, 0.15)
    } else {
        p.accent
    }
}

/// The selected segment of a segmented control: the primary buttons' fill,
/// so its label reads the same (4.26:1 on the plain light accent).
pub fn segment_fill(p: &WindowPalette) -> u32 {
    primary_fill(p)
}

/// A tag's ink, and how much of the accent washes its pill: the accent
/// pulled toward the text so the small word reads on the wash.
pub fn tag_colors(p: &WindowPalette) -> (u32, f32) {
    (mix(p.accent, p.text, 0.25), 0.12)
}

/// `a` blended toward `b` by `t` (0xRRGGBB).
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |shift: u32| {
        let (x, y) = (((a >> shift) & 0xFF) as f32, ((b >> shift) & 0xFF) as f32);
        ((x + (y - x) * t).round() as u32) << shift
    };
    ch(16) | ch(8) | ch(0)
}

pub(super) fn color(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    let channel = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
    D2D1_COLOR_F {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: alpha,
    }
}

pub(super) fn d2d_rect(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}

pub(super) fn rounded(r: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    D2D1_ROUNDED_RECT {
        rect: d2d_rect(r),
        radiusX: radius,
        radiusY: radius,
    }
}

/// The logo's ears (assets/logo.svg): (22,157) (36,32.5) (105,106) and mirrored.
fn ears_geometry(d2d: &ID2D1Factory) -> Result<ID2D1PathGeometry> {
    let pt = |x: f32, y: f32| Vector2::new(x, y);
    unsafe {
        let path = d2d.CreatePathGeometry()?;
        let sink = path.Open()?;
        for [a, b, c] in [
            [(22.0, 157.0), (36.0, 32.5), (105.0, 106.0)],
            [(234.0, 157.0), (220.0, 32.5), (151.0, 106.0)],
        ] {
            sink.BeginFigure(pt(a.0, a.1), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLine(pt(b.0, b.1));
            sink.AddLine(pt(c.0, c.1));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        }
        sink.Close()?;
        Ok(path)
    }
}

/// The logo's head (assets/logo.svg): a dome on a flat base with small bottom
/// corners, x 13..243, y 97..235.
fn head_geometry(d2d: &ID2D1Factory) -> Result<ID2D1PathGeometry> {
    let pt = |x: f32, y: f32| Vector2::new(x, y);
    let quad = |c: (f32, f32), e: (f32, f32)| D2D1_QUADRATIC_BEZIER_SEGMENT {
        point1: pt(c.0, c.1),
        point2: pt(e.0, e.1),
    };
    unsafe {
        let path = d2d.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(pt(13.0, 216.5), D2D1_FIGURE_BEGIN_FILLED);
        sink.AddLine(pt(13.0, 175.0));
        sink.AddQuadraticBezier(&quad((13.0, 97.0), (128.0, 97.0)));
        sink.AddQuadraticBezier(&quad((243.0, 97.0), (243.0, 175.0)));
        sink.AddLine(pt(243.0, 216.5));
        sink.AddQuadraticBezier(&quad((243.0, 235.0), (224.6, 235.0)));
        sink.AddLine(pt(31.4, 235.0));
        sink.AddQuadraticBezier(&quad((13.0, 235.0), (13.0, 216.5)));
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        Ok(path)
    }
}

/// The island's Notch outline (as `render.rs` draws it) for a compact shape.
fn notch_path(d2d: &ID2D1Factory, s: super::geometry::ShapeRect) -> Result<ID2D1PathGeometry> {
    let r = 16.0f32.min(s.h - SHOULDER);
    let (left, right, top, bottom) = (s.x, s.x + s.w, s.y, s.y + s.h);
    let arc = |x: f32, y: f32, radius: f32, sweep: D2D1_SWEEP_DIRECTION| D2D1_ARC_SEGMENT {
        point: Vector2::new(x, y),
        size: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F {
            width: radius,
            height: radius,
        },
        rotationAngle: 0.0,
        sweepDirection: sweep,
        arcSize: D2D1_ARC_SIZE_SMALL,
    };
    let (cw, ccw) = (
        D2D1_SWEEP_DIRECTION_CLOCKWISE,
        D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
    );
    unsafe {
        let path = d2d.CreatePathGeometry()?;
        let sink = path.Open()?;
        sink.BeginFigure(Vector2::new(left - SHOULDER, top), D2D1_FIGURE_BEGIN_FILLED);
        sink.AddArc(&arc(left, top + SHOULDER, SHOULDER, cw));
        sink.AddLine(Vector2::new(left, bottom - r));
        sink.AddArc(&arc(left + r, bottom, r, ccw));
        sink.AddLine(Vector2::new(right - r, bottom));
        sink.AddArc(&arc(right, bottom - r, r, ccw));
        sink.AddLine(Vector2::new(right, top + SHOULDER));
        sink.AddArc(&arc(right + SHOULDER, top, SHOULDER, cw));
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        Ok(path)
    }
}

/// One line, vertically centered, trimmed with "…".
pub(super) fn line_format(
    dwrite: &IDWriteFactory,
    face: &Face,
    weight: DWRITE_FONT_WEIGHT,
    size: f32,
    align: DWRITE_TEXT_ALIGNMENT,
) -> Result<IDWriteTextFormat> {
    let format = face.format(dwrite, weight, size)?;
    unsafe {
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        format.SetTextAlignment(align)?;
        let ellipsis = dwrite.CreateEllipsisTrimmingSign(&format)?;
        let trimming = DWRITE_TRIMMING {
            granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            delimiter: 0,
            delimiterCount: 0,
        };
        format.SetTrimming(&trimming, &ellipsis)?;
        Ok(format)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, w: f32) -> Rect {
        Rect {
            x,
            y: 0.0,
            w,
            h: 10.0,
        }
    }

    /// `Painter::cat` at `alpha` on a 64 px `bg` square, as premultiplied
    /// BGRA, drawn by Direct2D's software rasterizer into a WIC bitmap.
    fn render_cat(alpha: f32, look: CatLook, bg: u32) -> Vec<u8> {
        use windows::Win32::Graphics::Direct2D::Common::D2D1_ALPHA_MODE_PREMULTIPLIED;
        use windows::Win32::Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
            WICBitmapCacheOnLoad,
        };
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        };
        const SIZE: u32 = 64;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let gfx = Gfx::new(UiFont::Sans).expect("gfx");
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .expect("wic");
            let bitmap = wic
                .CreateBitmap(
                    SIZE,
                    SIZE,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapCacheOnLoad,
                )
                .expect("bitmap");
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let rt = gfx
                .d2d
                .CreateWicBitmapRenderTarget(&bitmap, &props)
                .expect("target");
            let brush = rt
                .CreateSolidColorBrush(&color(0, 1.0), None)
                .expect("brush");
            let layer = rt.CreateLayer(None).expect("layer");
            rt.BeginDraw();
            rt.Clear(Some(&color(bg, 1.0)));
            let painter = Painter {
                rt: &rt,
                brush: &brush,
                layer: &layer,
                gfx: &gfx,
                p: theme::window(Accent::default(), theme::WindowTheme::Dark, false),
                alpha,
            };
            let area = Rect {
                x: 8.0,
                y: 8.0,
                w: 48.0,
                h: 48.0,
            };
            painter.cat(area, Expr::Idle, Pose::default(), look);
            rt.EndDraw(None, None).expect("end draw");
            let mut out = vec![0u8; (SIZE * SIZE * 4) as usize];
            bitmap
                .CopyPixels(std::ptr::null(), SIZE * 4, &mut out)
                .expect("pixels");
            out
        }
    }

    /// A faded cat is the opaque cat faded as one piece: every pixel is the
    /// opaque picture mixed into the background, so no pass (the Black
    /// cat's rim, the ears' stroke under the head, the calico patches'
    /// cream) shows through the one drawn over it. The disabled accessory
    /// tiles and the section fade-in draw the cat this way.
    #[test]
    fn a_faded_cat_fades_as_one_piece() {
        let bg = 0x2A2140;
        let alpha = 0.45;
        let channel = |rgb: u32, s: u32| ((rgb >> s) & 0xFF) as f32;
        for &color in CatColor::ALL.iter() {
            for &accessory in CatAccessory::ALL.iter() {
                let look = CatLook { color, accessory };
                let opaque = render_cat(1.0, look, bg);
                let faded = render_cat(alpha, look, bg);
                let mut worst = 0.0f32;
                for (o, f) in opaque.chunks(4).zip(faded.chunks(4)) {
                    // BGRA, opaque everywhere (the background is).
                    for (i, shift) in [(0, 0), (1, 8), (2, 16)] {
                        let want = channel(bg, shift) + (o[i] as f32 - channel(bg, shift)) * alpha;
                        worst = worst.max((f[i] as f32 - want).abs());
                    }
                }
                assert!(
                    worst <= 3.0,
                    "{color:?} with {accessory:?}: a pixel is {worst} off the opaque cat faded"
                );
            }
        }
    }

    #[test]
    fn preview_bars_fit_the_equalizer_box() {
        let tallest = PREVIEW_BARS.into_iter().fold(0.0, f32::max);
        assert_eq!(tallest, super::super::geometry::BAR_MAX_H);
        assert!(PREVIEW_BARS.iter().all(|&h| h > 0.0));
    }

    #[test]
    fn rect_at_blends_neighbours() {
        let rects = [r(0.0, 10.0), r(10.0, 20.0), r(30.0, 10.0)];
        assert_eq!(rect_at(&rects, 0.0), rects[0]);
        assert_eq!(rect_at(&rects, 2.0), rects[2]);
        assert_eq!(rect_at(&rects, 0.5), r(5.0, 15.0));
        assert_eq!(rect_at(&rects, 9.0), rects[2], "clamped");
    }

    #[test]
    fn digit_runs_cover_only_the_digits() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(digit_runs(&w("25 mnt")), [(0, 2)]);
        assert_eq!(digit_runs(&w("20%")), [(0, 2)]);
        assert_eq!(digit_runs(&w("a1b23")), [(1, 1), (3, 2)]);
        assert!(digit_runs(&w("mnt")).is_empty());
    }

    /// The Transkrip card's pairs read in the light, warm dark and Terminal
    /// windows with every accent: the primary label (Unduh), the selected
    /// segment's label (Bahasa) and the tag at 4.5:1 on what they sit on,
    /// the done part of a bar at 3:1 on its track.
    #[test]
    fn primary_labels_tags_and_bars_read_in_every_window() {
        use crate::theme::{WindowTheme, window_for};
        use crate::widgets::sticky::contrast;
        let over = |base: u32, top: u32, a: f32| mix(base, top, a);
        for accent in Accent::ALL {
            for (surface, mode) in [
                (Surface::Espresso, WindowTheme::Light),
                (Surface::Espresso, WindowTheme::Dark),
                (Surface::Terminal, WindowTheme::Dark),
            ] {
                let p = window_for(*accent, surface, mode, false);
                let label = contrast(p.on_accent, primary_fill(&p));
                assert!(label >= 4.5, "{accent:?} {surface:?} {mode:?}: Unduh {label:.2}");
                let segment = contrast(p.on_accent, segment_fill(&p));
                assert!(segment >= 4.5, "{accent:?} {surface:?} {mode:?}: segment {segment:.2}");
                let (ink, wash) = tag_colors(&p);
                let tag = contrast(ink, over(p.card, p.accent, wash));
                assert!(tag >= 4.5, "{accent:?} {surface:?} {mode:?}: tag {tag:.2}");
                let track = over(p.card, p.secondary, BAR_TRACK);
                let bar = contrast(p.accent, track);
                assert!(bar >= 3.0, "{accent:?} {surface:?} {mode:?}: bar {bar:.2}");
            }
        }
    }

    /// The "Yakin?" label (error red) reads at 4.5:1 on its rest, hover and
    /// pressed fills in light, warm dark and Terminal dark, for every accent.
    #[test]
    fn the_confirm_label_reads_on_rest_hover_and_pressed() {
        use crate::theme::{WindowTheme, window_for};
        use crate::widgets::sticky::contrast;
        for accent in Accent::ALL {
            for (surface, mode) in [
                (Surface::Espresso, WindowTheme::Light),
                (Surface::Espresso, WindowTheme::Dark),
                (Surface::Terminal, WindowTheme::Dark),
            ] {
                let p = window_for(*accent, surface, mode, false);
                for (hovered, pressed) in [(false, false), (true, false), (true, true)] {
                    let r = contrast(p.error, confirm_fill(&p, hovered, pressed));
                    assert!(
                        r >= 4.5,
                        "{accent:?} {surface:?} {mode:?} hover {hovered} press {pressed}: {r:.2}"
                    );
                }
            }
        }
    }

    #[test]
    fn mix_and_ease_ends() {
        assert_eq!(mix(0x000000, 0xFFFFFF, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xFFFFFF, 1.0), 0xFFFFFF);
        assert_eq!(mix(0x102030, 0x102030, 0.5), 0x102030);
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!(ease(0.5) > 0.5, "ease-out runs ahead");
    }

    #[test]
    fn link_text_reads_on_light_cards_for_every_accent() {
        // Links use the light window's accent shade as is.
        for a in Accent::ALL {
            let p = crate::theme::window(*a, crate::theme::WindowTheme::Light, true);
            let ink = p.accent;
            let lum = |rgb: u32| {
                let c = |s: u32| {
                    let v = ((rgb >> s) & 0xFF) as f64 / 255.0;
                    if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                };
                0.2126 * c(16) + 0.7152 * c(8) + 0.0722 * c(0)
            };
            let contrast = (lum(p.card) + 0.05) / (lum(ink) + 0.05);
            assert!(contrast >= 3.0, "{a:?}: {contrast:.2}");
            // Status text (sage, ochre) on the cards, as `Painter::tone` mixes it.
            for status in [Accent::Sage, Accent::Ochre] {
                let ink = mix(status.light_rgb(), p.text, 0.25);
                let contrast = (lum(p.card) + 0.05) / (lum(ink) + 0.05);
                assert!(contrast >= 4.5, "{status:?}: {contrast:.2}");
            }
        }
    }
}
