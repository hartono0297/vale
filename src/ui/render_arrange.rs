//! Drawing of the Susun page and its notice. A child module of `render`, so
//! it shares the renderer's private state and helpers (like `render_panel`).
//! Positions come from `panel_layout::arrange_layout`; colours only from the
//! island `Palette`.

use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE;
use windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::core::Result;

use super::*;
use crate::arrange::NRect;
use crate::i18n::{Key, text};
use crate::model::ArrangeSave;
use crate::ui::geometry::notice_layout;
use crate::ui::panel_layout::{ArrangeHead, Region};

/// How strongly a tinted badge shows its colour (the notices' badge).
const BADGE_TINT: f32 = 0.2;

/// A template card or saved row's preview and name.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeCardView {
    pub name: String,
    pub rects: Vec<NRect>,
}

/// One saved arrangement: its preview, name and the apps in it.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeSavedView {
    pub name: String,
    pub rects: Vec<NRect>,
    pub apps: String,
}

/// One row of the pick list.
#[derive(Clone, Debug, PartialEq)]
pub struct ArrangeWindowView<'a> {
    pub title: String,
    /// The icon cache key (the exe path, lower case).
    pub key: &'a str,
    pub icon: Option<&'a Thumb>,
}

/// The Arranged notice's name field while it is up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrangeNameView<'a> {
    pub text: &'a str,
    pub selection: (usize, usize),
}

/// What the Susun page (and its notice) shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArrangeView<'a> {
    /// The pick step: the tile being filled (0-based) of how many.
    pub picking: Option<(usize, usize)>,
    /// One label per monitor ("1 · 2560×1440"); the chips show with two or
    /// more.
    pub monitors: Vec<String>,
    pub monitor: usize,
    pub cards: Vec<ArrangeCardView>,
    pub saved: Vec<ArrangeSavedView>,
    pub windows: Vec<ArrangeWindowView<'a>>,
    pub scroll: f32,
    /// The template card under the pointer (its outline is on the screen).
    pub hover_card: Option<usize>,
    pub name: Option<ArrangeNameView<'a>>,
}

/// What the Arranged notice says apart from its title and body.
const CHIP_PAD: f32 = 11.0;
const CHIP_H: f32 = 24.0;
const NAME_H: f32 = 26.0;

impl Renderer {
    /// The header's widths: the title, each monitor chip and "Lewati ubin".
    pub(in crate::ui::render) fn arrange_head(&self, view: &ArrangeView) -> ArrangeHead {
        let lang = i18n::lang();
        let title = match view.picking {
            Some((at, total)) => i18n::arrange_tile_of(lang, at + 1, total),
            None => text(lang, Key::PageArrange).to_owned(),
        };
        let chips = if view.picking.is_none() && view.monitors.len() > 1 {
            view.monitors
                .iter()
                .map(|m| self.text_width(m, &self.fonts.small).ceil() + 2.0 * CHIP_PAD_X)
                .collect()
        } else {
            Vec::new()
        };
        let skip = self
            .text_width(text(lang, Key::ArrangeSkip), &self.fonts.button)
            .ceil();
        ArrangeHead {
            title_w: self.text_width(&title, &self.fonts.heading).ceil() + 1.0,
            chip_w: chips,
            skip_w: skip + 2.0 * DEV_BTN_PAD_X,
        }
    }

    /// The tiles of a layout as a miniature in `area`: each tile a rounded
    /// block with a hairline gap, in `ink` at `alpha`.
    fn paint_mini(&self, area: Rect, rects: &[NRect], ink: u32, alpha: f32) {
        for n in rects {
            let tile = Rect {
                x: area.x + n.x * area.w,
                y: area.y + n.y * area.h,
                w: n.w * area.w,
                h: n.h * area.h,
            };
            self.set_color(ink, alpha);
            self.fill_round(inset(tile, 1.0, 1.0), 2.5);
        }
    }

    /// The Susun page: the template step (monitor chips, a card per
    /// template, the saved arrangements) or the pick step (the window list).
    pub(in crate::ui::render) fn paint_arrange(&self, l: &ArrangeLayout, view: &ArrangeView) {
        let lang = i18n::lang();
        let p = self.palette;
        match view.picking {
            Some((at, total)) => {
                let title = i18n::arrange_tile_of(lang, at + 1, total);
                self.heading(&title, l.header);
            }
            None => self.heading(text(lang, Key::PageArrange), l.header),
        }
        for (i, (chip, label)) in l.chips.iter().zip(&view.monitors).enumerate() {
            let on = i == view.monitor;
            self.set_color(if on { p.accent } else { p.raised }, 1.0);
            self.fill_round(*chip, chip.h / 2.0);
            let ink = if on { p.on_accent } else { p.secondary };
            self.text_colored(label, &self.fonts.small_center, *chip, ink, 1.0);
        }
        if let Some(skip) = l.skip {
            self.paint_labelled_button(
                skip,
                None,
                text(lang, Key::ArrangeSkip),
                (p.raised, p.text),
                1.0,
            );
        }
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe {
            target
                .rt
                .PushAxisAlignedClip(&d2d_rect(l.list), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        for (i, (lay, card)) in l.cards.iter().zip(&view.cards).enumerate() {
            let hot = view.hover_card == Some(i);
            self.set_color(p.raised, 1.0);
            self.fill_round(lay.card, radius::CARD);
            if hot {
                self.stroke_round_width(
                    inset(lay.card, 0.75, 0.75),
                    radius::CARD - 0.75,
                    p.accent,
                    1.5,
                );
            }
            self.paint_mini(
                lay.preview,
                &card.rects,
                if hot { p.accent } else { p.secondary },
                if hot { 0.55 } else { 0.35 },
            );
            self.text_colored(
                &card.name,
                &self.fonts.small_center,
                lay.name,
                if hot { p.text } else { p.secondary },
                1.0,
            );
        }
        if let Some(title) = l.saved_title {
            self.text_colored(
                text(lang, Key::ArrangeSaved),
                &self.fonts.small_medium,
                title,
                p.secondary,
                1.0,
            );
        }
        for (lay, saved) in l.saved.iter().zip(&view.saved) {
            self.set_color(p.raised, 1.0);
            self.fill_round(lay.row, radius::BLOCK);
            self.paint_mini(lay.preview, &saved.rects, p.secondary, 0.4);
            self.text(&saved.name, &self.fonts.body_medium, lay.name, 1.0);
            self.text_colored(&saved.apps, &self.fonts.small, lay.apps, p.secondary, 1.0);
            self.set_color(p.accent, 1.0);
            self.fill_round(lay.play, lay.play.h / 2.0);
            self.icon_colored(Icon::Play, lay.play, 12.0, p.on_accent, 1.0, None);
        }
        // Icons of rows no longer shown go.
        self.arrange_icons
            .borrow_mut()
            .retain(|(key, _)| view.windows.iter().any(|w| w.key == key));
        for (lay, win) in l.windows.iter().zip(&view.windows) {
            self.set_color(p.raised, 1.0);
            self.fill_round(lay.row, radius::BLOCK);
            let bitmap = win
                .icon
                .filter(|_| !win.key.is_empty())
                .and_then(|thumb| self.arrange_icon(win.key, thumb));
            match bitmap {
                Some(bitmap) => unsafe {
                    target.rt.DrawBitmap(
                        &bitmap,
                        Some(&d2d_rect(lay.icon)),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    );
                },
                // A generic icon until the real one is read.
                None => self.icon_colored(Icon::Windows, lay.icon, 16.0, p.secondary, 1.0, None),
            }
            self.text(&win.title, &self.fonts.body_medium, lay.title, 1.0);
        }
        unsafe { target.rt.PopAxisAlignedClip() };
        if let Some(empty) = l.empty {
            let area = Rect {
                y: empty.y + empty.h / 2.0 - 10.0,
                h: 20.0,
                ..empty
            };
            self.text_colored(
                text(lang, Key::ArrangeNoWindows),
                &self.fonts.eleven_center,
                area,
                p.secondary,
                1.0,
            );
        }
        if let Some(hint) = l.hint {
            self.text_colored(
                text(lang, Key::ArrangeHint),
                &self.fonts.small,
                hint,
                p.secondary,
                1.0,
            );
        }
        if let Some(bar) = l.scrollbar {
            self.set_color(p.secondary, 0.55);
            self.fill_round(bar, CLIP_SCROLLBAR_W / 2.0);
        }
    }

    /// The app's icon as a bitmap of this target, made once.
    fn arrange_icon(&self, key: &str, thumb: &Thumb) -> Option<ID2D1Bitmap> {
        if let Some((_, b)) = self.arrange_icons.borrow().iter().find(|(k, _)| k == key) {
            return Some(b.clone());
        }
        let bitmap = self.thumb_bitmap(thumb).ok().flatten()?;
        self.arrange_icons
            .borrow_mut()
            .push((key.to_owned(), bitmap.clone()));
        Some(bitmap)
    }

    /// The notice after an arrangement: the badge (or Bata), the title over
    /// the body, and "Batalkan" and "Simpan…" at the right; while the name
    /// field is up, a hint over the field and "Batalkan" alone. The buttons
    /// are hit regions.
    pub(in crate::ui::render) fn paint_arranged(
        &mut self,
        shape: ShapeRect,
        notice: &Notice,
        save: ArrangeSave,
        view: &ArrangeView,
        robot: Option<RobotFrame>,
    ) -> Result<()> {
        let p = self.palette;
        let lang = i18n::lang();
        let layout = notice_layout(shape, None);
        match robot {
            Some(r) => self.paint_robot(layout.badge, r),
            None => {
                self.set_color(p.accent, BADGE_TINT);
                self.fill_round(layout.badge, radius::BADGE);
                self.solid_ink(|| {
                    self.icon_colored(Icon::Layout, layout.badge, 20.0, p.accent, 1.0, None);
                });
            }
        }
        let edge = shape.x + shape.w - 20.0;
        let mid = shape.y + shape.h / 2.0;
        let chip = |w: f32, right: f32| Rect {
            x: right - w,
            y: mid - CHIP_H / 2.0,
            w,
            h: CHIP_H,
        };
        if let Some(name) = view.name {
            // "Batalkan" stays at the right (the undo works while the field
            // is up); the hint small above the field, the field the rest of
            // the row.
            let label = text(lang, Key::ArrangeUndo);
            let w = self.text_width(label, &self.fonts.button).ceil() + 2.0 * CHIP_PAD;
            let undo = chip(w, edge);
            self.set_color(p.accent, 1.0);
            self.fill_round(undo, undo.h / 2.0);
            self.solid_ink(|| {
                self.text_colored(label, &self.fonts.button_center, undo, p.on_accent, 1.0);
            });
            let left = layout.title.x;
            let right = undo.x - 10.0;
            let hint = Rect {
                y: mid - 26.0,
                h: 14.0,
                w: (right - left).max(0.0),
                ..layout.title
            };
            self.text_colored(
                text(lang, Key::ArrangeNameHint),
                &self.fonts.small,
                hint,
                p.secondary,
                1.0,
            );
            let field = Rect {
                x: left,
                y: mid - 8.0,
                w: (right - left).max(0.0),
                h: NAME_H,
            };
            self.paint_name_field(field, name);
            self.regions.regions = vec![Region {
                rect: undo,
                hit: Hit::ArrangeUndo,
            }];
            return Ok(());
        }
        // The chips, right to left.
        let mut right = edge;
        let mut regions = Vec::new();
        let mut first_left = edge;
        let buttons = [
            (
                Key::ArrangeSaveAs,
                Hit::ArrangeSave,
                save != ArrangeSave::None,
                save != ArrangeSave::Offer,
            ),
            (Key::ArrangeUndo, Hit::ArrangeUndo, true, false),
        ];
        for (key, hit, shown, dim) in buttons {
            if !shown {
                continue;
            }
            let label = text(lang, key);
            let w = self.text_width(label, &self.fonts.button).ceil() + 2.0 * CHIP_PAD;
            let r = chip(w, right);
            let alpha = if dim { 0.4 } else { 1.0 };
            let (fill, ink) = if hit == Hit::ArrangeUndo {
                (p.accent, p.on_accent)
            } else {
                (p.raised_hi, p.text)
            };
            self.set_color(fill, alpha);
            self.fill_round(r, r.h / 2.0);
            self.solid_ink(|| {
                self.text_colored(label, &self.fonts.button_center, r, ink, alpha);
            });
            if !dim {
                regions.push(Region { rect: r, hit });
            }
            right = r.x - 6.0;
            first_left = r.x;
        }
        let right_edge = first_left - 10.0;
        let fit = |r: Rect| Rect {
            w: (right_edge - r.x).max(0.0),
            ..r
        };
        self.text(&notice.title, &self.fonts.title, fit(layout.title), 1.0);
        let body = if save == ArrangeSave::Full {
            text(lang, Key::ArrangeFull)
        } else {
            &notice.body
        };
        self.text_colored(body, &self.fonts.small, fit(layout.body), p.secondary, 1.0);
        self.regions.regions = regions;
        Ok(())
    }

    /// The name field: a rounded box with the text, a selection and a caret.
    fn paint_name_field(&self, r: Rect, name: ArrangeNameView) {
        let p = self.palette;
        self.set_color(p.raised, 1.0);
        self.fill_round(r, 10.0);
        self.stroke_round_width(inset(r, 0.75, 0.75), 9.25, p.accent, FOCUS_W);
        let field = Rect {
            x: r.x + 10.0,
            w: (r.w - 20.0).max(0.0),
            ..r
        };
        let format = &self.fonts.clip_search;
        let mid = field.y + field.h / 2.0;
        let len = name.text.encode_utf16().count();
        let (a, b) = (name.selection.0.min(len), name.selection.1.min(len));
        let x_of = |n: usize| self.text_width(utf16_prefix(name.text, n), format);
        let caret_x = x_of(b);
        let origin = field.x - (caret_x + 2.0 - field.w).max(0.0);
        let target = self.target.as_ref().expect("target ensured by paint");
        unsafe {
            target
                .rt
                .PushAxisAlignedClip(&d2d_rect(field), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
        if a != b {
            let xa = x_of(a);
            self.set_color(p.accent, 0.32);
            self.fill_round(
                Rect {
                    x: origin + xa,
                    y: mid - CARET_H / 2.0,
                    w: caret_x - xa,
                    h: CARET_H,
                },
                2.0,
            );
        }
        let typed = Rect {
            x: origin,
            w: 4096.0,
            ..field
        };
        self.text(name.text, format, typed, 1.0);
        self.set_color(p.accent, 1.0);
        self.fill_round(
            Rect {
                x: origin + caret_x - 0.75,
                y: mid - CARET_H / 2.0,
                w: 1.5,
                h: CARET_H,
            },
            0.0,
        );
        unsafe { target.rt.PopAxisAlignedClip() };
    }
}
