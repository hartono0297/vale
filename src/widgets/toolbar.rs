//! The format bar of a sticky note: the tools, where each button sits at a
//! given note width, what a point hits, and the keyboard shortcuts. Pure; the
//! drawing is `paint`, the popover and the overflow menu are `fmt_pop`.
//!
//! The bar is a 42 dip block at the bottom of the note (it replaces the 24 dip
//! footer while the note has the keyboard). Inside it sits the bar itself,
//! inset 6 dip on all sides (30 tall, radius 8), with 4 dip of padding, 24 dip
//! square buttons and 1 dip between every two elements. A separator is a 1 dip
//! line with 3 dip of room on each side. Everything is left-aligned, and
//! `layout` answers in dip from the block's top-left.

use crate::i18n::Key;
use crate::ui::geometry::Rect;

/// The block the bar takes at the bottom of the note (dip).
pub const BAR_BLOCK: f32 = 42.0;
/// The bar's inset inside its block, its height, its corner radius, the
/// padding inside it.
pub const BAR_INSET: f32 = 6.0;
pub const BAR_H: f32 = 30.0;
pub const BAR_RADIUS: f32 = 8.0;
pub const BAR_PAD: f32 = 4.0;
/// A button (a square), the gap between two elements and a separator's parts.
pub const BTN: f32 = 24.0;
pub const GAP: f32 = 1.0;
pub const SEP_SPACE: f32 = 3.0;
pub const SEP_W: f32 = 1.0;
/// The Marker button: its left part applies the last highlight, its right
/// part (with the chevron) opens the popover.
pub const MARKER_APPLY_W: f32 = 16.0;

/// A tool of the bar, in bar order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Bold,
    Italic,
    Underline,
    Strike,
    Bullet,
    Check,
    Marker,
    Image,
}

impl Tool {
    /// Bar order. A separator follows `Strike` and `Check`.
    pub const ALL: [Tool; 8] = [
        Tool::Bold,
        Tool::Italic,
        Tool::Underline,
        Tool::Strike,
        Tool::Bullet,
        Tool::Check,
        Tool::Marker,
        Tool::Image,
    ];

    /// A separator is drawn right after this tool.
    pub fn sep_after(self) -> bool {
        matches!(self, Tool::Strike | Tool::Check)
    }

    pub fn label_key(self) -> Key {
        match self {
            Tool::Bold => Key::FmtBold,
            Tool::Italic => Key::FmtItalic,
            Tool::Underline => Key::FmtUnderline,
            Tool::Strike => Key::FmtStrike,
            Tool::Bullet => Key::FmtBullet,
            Tool::Check => Key::FmtCheck,
            Tool::Marker => Key::FmtMarker,
            Tool::Image => Key::FmtImage,
        }
    }
}

/// The shortcut shown in the tooltip and the overflow menu ("" for none).
pub fn shortcut(t: Tool) -> &'static str {
    match t {
        Tool::Bold => "Ctrl+B",
        Tool::Italic => "Ctrl+I",
        Tool::Underline => "Ctrl+U",
        Tool::Strike => "Ctrl+Shift+X",
        Tool::Bullet => "Ctrl+Shift+8",
        Tool::Check => "Ctrl+Shift+9",
        Tool::Marker => "Ctrl+Shift+H",
        Tool::Image => "",
    }
}

/// The tool a key press with Ctrl held is. Plain Ctrl+X, Ctrl+C and Ctrl+V
/// are not tools (they cut, copy and paste), and with Shift Ctrl+B, I and U
/// are not either. With Alt down it is nothing: AltGr reaches a key down as
/// Ctrl+Alt, and AltGr+B, U, I or Shift+X type a character on several layouts
/// (Hungarian, Czech, Slovak, Polish), which must not flip the formatting.
pub fn shortcut_of(vk: u16, ctrl: bool, shift: bool, alt: bool) -> Option<Tool> {
    if !ctrl || alt {
        return None;
    }
    match (vk, shift) {
        (0x42, false) => Some(Tool::Bold),
        (0x49, false) => Some(Tool::Italic),
        (0x55, false) => Some(Tool::Underline),
        (0x58, true) => Some(Tool::Strike),
        (0x38, true) => Some(Tool::Bullet),
        (0x39, true) => Some(Tool::Check),
        (0x48, true) => Some(Tool::Marker),
        _ => None,
    }
}

/// The control character a shortcut leaves in the character stream (the
/// EDIT would type or act on it): B 0x02, I 0x09 (Tab), U 0x15, Shift+X 0x18
/// (cut), Shift+H 0x08 (backspace). The digits make none. `None` for a key
/// that makes no character.
pub fn control_char_of(t: Tool) -> Option<u16> {
    match t {
        Tool::Bold => Some(0x02),
        Tool::Italic => Some(0x09),
        Tool::Underline => Some(0x15),
        Tool::Strike => Some(0x18),
        Tool::Marker => Some(0x08),
        Tool::Bullet | Tool::Check | Tool::Image => None,
    }
}

/// Where the bar's parts are, in dip from the block's top-left.
#[derive(Clone, Debug, PartialEq)]
pub struct BarLayout {
    pub bar: Rect,
    /// The tools that fit, with their rects.
    pub items: Vec<(Tool, Rect)>,
    /// The separators' rects (a 1 dip wide line, as tall as a button).
    pub seps: Vec<Rect>,
    /// The overflow button, when something does not fit.
    pub more: Option<Rect>,
    /// The tools that moved into the overflow menu, in order.
    pub hidden: Vec<Tool>,
}

/// The bar inside its block for a note `w` wide.
pub fn bar_rect(w: f32) -> Rect {
    Rect {
        x: BAR_INSET,
        y: BAR_INSET,
        w: (w - 2.0 * BAR_INSET).max(0.0),
        h: BAR_H,
    }
}

/// Lays the bar out for a note `w` wide: every tool when the row ends inside
/// the bar's interior (`w - 10`), else as many leading tools as fit followed by
/// a `More` button; a separator next to the cut is dropped.
pub fn layout(w: f32) -> BarLayout {
    let bar = bar_rect(w);
    let left = bar.x + BAR_PAD;
    let right = bar.right() - BAR_PAD;
    let top = bar.y + (BAR_H - BTN) / 2.0;
    // The row of the first `k` tools: (x, rect) of each, the seps between
    // them, and where it ends.
    let row = |k: usize| {
        let mut x = left;
        let mut items = Vec::new();
        let mut seps = Vec::new();
        for (i, t) in Tool::ALL.iter().take(k).enumerate() {
            if i > 0 {
                x += GAP;
            }
            items.push((*t, Rect { x, y: top, w: BTN, h: BTN }));
            x += BTN;
            if t.sep_after() && i + 1 < k {
                x += GAP + SEP_SPACE;
                seps.push(Rect { x, y: top, w: SEP_W, h: BTN });
                x += SEP_W + SEP_SPACE;
            }
        }
        (items, seps, x)
    };
    let all = Tool::ALL.len();
    let (items, seps, end) = row(all);
    if end <= right {
        return BarLayout {
            bar,
            items,
            seps,
            more: None,
            hidden: Vec::new(),
        };
    }
    // Keep as many leading tools as leave room for the More button.
    for k in (0..all).rev() {
        let (items, seps, end) = row(k);
        let at = if k == 0 { left } else { end + GAP };
        if at + BTN <= right || k == 0 {
            return BarLayout {
                bar,
                items,
                seps,
                more: Some(Rect { x: at, y: top, w: BTN, h: BTN }),
                hidden: Tool::ALL[k..].to_vec(),
            };
        }
    }
    unreachable!("k == 0 always returns")
}

/// What a point of the bar is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarHit {
    Tool(Tool),
    /// The Marker button's chevron: opens the popover.
    MarkerMenu,
    /// The overflow button.
    More,
    /// The bar's own background (a gap, a separator, its padding).
    Bar,
}

fn inside(r: &Rect, x: f32, y: f32) -> bool {
    r.contains(x, y)
}

/// What the point (`x`, `y`, dip from the block's top-left) is on; `None`
/// outside the bar.
pub fn hit(l: &BarLayout, x: f32, y: f32) -> Option<BarHit> {
    if !inside(&l.bar, x, y) {
        return None;
    }
    for (t, r) in &l.items {
        if inside(r, x, y) {
            if *t == Tool::Marker && x >= r.x + MARKER_APPLY_W {
                return Some(BarHit::MarkerMenu);
            }
            return Some(BarHit::Tool(*t));
        }
    }
    if l.more.as_ref().is_some_and(|r| inside(r, x, y)) {
        return Some(BarHit::More);
    }
    Some(BarHit::Bar)
}

/// The rect of `t` in the layout, if it is shown (not in the overflow menu).
pub fn rect_of(l: &BarLayout, t: Tool) -> Option<Rect> {
    l.items.iter().find(|(i, _)| *i == t).map(|(_, r)| *r)
}

/// The footer message's height (dip).
pub const TOOLTIP_H: f32 = 22.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    fn check_sane(w: f32) -> BarLayout {
        let l = layout(w);
        let interior_right = l.bar.right() - BAR_PAD;
        let interior_left = l.bar.x + BAR_PAD;
        let mut rects: Vec<Rect> = l.items.iter().map(|(_, r)| *r).collect();
        rects.extend(l.seps.iter().copied());
        rects.extend(l.more);
        for r in &rects {
            assert!(r.x >= interior_left - 0.001, "w {w}: {r:?} left of the interior");
            assert!(r.right() <= interior_right + 0.001, "w {w}: {r:?} past the interior");
            assert!(r.y >= l.bar.y && r.bottom() <= l.bar.bottom(), "w {w}: {r:?} outside the bar");
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!overlap(a, b), "w {w}: {a:?} overlaps {b:?}");
            }
        }
        // Shown plus hidden is always every tool, in order.
        let mut all: Vec<Tool> = l.items.iter().map(|(t, _)| *t).collect();
        all.extend(&l.hidden);
        assert_eq!(all, Tool::ALL.to_vec(), "w {w}");
        l
    }

    #[test]
    fn the_tools_are_in_the_documented_order_with_two_separators() {
        assert_eq!(
            Tool::ALL,
            [
                Tool::Bold,
                Tool::Italic,
                Tool::Underline,
                Tool::Strike,
                Tool::Bullet,
                Tool::Check,
                Tool::Marker,
                Tool::Image
            ]
        );
        let seps: Vec<Tool> = Tool::ALL.into_iter().filter(|t| t.sep_after()).collect();
        assert_eq!(seps, [Tool::Strike, Tool::Check]);
    }

    #[test]
    fn the_bar_is_inset_inside_its_block() {
        let l = layout(240.0);
        assert_eq!(l.bar, Rect { x: 6.0, y: 6.0, w: 228.0, h: 30.0 });
        assert_eq!(BAR_BLOCK, 42.0);
        assert_eq!(l.bar.bottom() + BAR_INSET, BAR_BLOCK);
        let (_, first) = l.items[0];
        assert_eq!(first.x, 6.0 + 4.0, "4 dip of padding");
        assert_eq!((first.w, first.h), (24.0, 24.0));
        let (_, second) = l.items[1];
        assert_eq!(second.x - first.right(), 1.0, "1 dip gaps");
    }

    #[test]
    fn everything_fits_from_235_dip_and_the_default_note_shows_all_eight() {
        for w in [235.0, 240.0, 300.0, 640.0] {
            let l = check_sane(w);
            assert_eq!(l.items.len(), 8, "w {w}");
            assert!(l.more.is_none() && l.hidden.is_empty(), "w {w}");
            assert_eq!(l.seps.len(), 2, "w {w}");
        }
        // The last button ends exactly at the interior at 235.
        let l = layout(235.0);
        let (_, last) = *l.items.last().unwrap();
        assert!((last.right() - (235.0 - 10.0)).abs() < 0.001);
        // One dip less and the row no longer fits.
        assert!(layout(234.0).more.is_some());
    }

    #[test]
    fn a_narrow_note_keeps_the_leading_tools_and_a_more_button() {
        for w in [160.0, 180.0, 200.0, 220.0, 234.0] {
            let l = check_sane(w);
            assert!(l.more.is_some(), "w {w}");
            assert!(!l.hidden.is_empty());
            assert_eq!(l.more.unwrap().w, 24.0);
            // The More button follows the last shown tool.
            let last = l.items.last().map(|(_, r)| r.right()).unwrap_or(0.0);
            assert!(l.more.unwrap().x >= last, "w {w}");
        }
        // 160: Bold, Italic, Underline, Strike and More.
        let l = layout(160.0);
        assert_eq!(l.items.len(), 4);
        assert_eq!(l.hidden, [Tool::Bullet, Tool::Check, Tool::Marker, Tool::Image]);
        assert!(l.seps.is_empty(), "the separator next to the cut is dropped");
        // 200: the first separator and two more tools are kept.
        let l = layout(200.0);
        assert!(l.items.len() > 4 && l.items.len() < 8, "{}", l.items.len());
        // A separator is only drawn between shown tools.
        for s in &l.seps {
            let before = l.items.iter().any(|(_, r)| r.right() < s.x);
            let after = l.items.iter().any(|(_, r)| r.x > s.right());
            assert!(before && after);
        }
    }

    #[test]
    fn fewer_tools_fit_as_the_note_narrows() {
        let narrow = layout(160.0).items.len();
        let wide = layout(260.0).items.len();
        assert!(narrow < wide);
    }

    #[test]
    fn hits_split_the_marker_button_and_find_the_gaps() {
        let l = layout(240.0);
        let bold = rect_of(&l, Tool::Bold).unwrap();
        assert_eq!(hit(&l, bold.x + 5.0, bold.y + 5.0), Some(BarHit::Tool(Tool::Bold)));
        let m = rect_of(&l, Tool::Marker).unwrap();
        assert_eq!(hit(&l, m.x + 2.0, m.y + 12.0), Some(BarHit::Tool(Tool::Marker)));
        assert_eq!(hit(&l, m.x + 15.9, m.y + 12.0), Some(BarHit::Tool(Tool::Marker)));
        assert_eq!(hit(&l, m.x + 16.0, m.y + 12.0), Some(BarHit::MarkerMenu));
        assert_eq!(hit(&l, m.right() - 0.1, m.y + 12.0), Some(BarHit::MarkerMenu));
        // The gap between two buttons, and a separator, are the bar's own.
        let b2 = rect_of(&l, Tool::Italic).unwrap();
        assert_eq!(hit(&l, bold.right() + 0.5, bold.y + 5.0), Some(BarHit::Bar));
        assert!(b2.x > bold.right());
        let sep = l.seps[0];
        assert_eq!(hit(&l, sep.x + 0.5, sep.y + 5.0), Some(BarHit::Bar));
        // The bar's padding and the background above the buttons.
        assert_eq!(hit(&l, l.bar.x + 1.0, l.bar.y + 1.0), Some(BarHit::Bar));
        assert_eq!(hit(&l, bold.x + 5.0, l.bar.y + 1.0), Some(BarHit::Bar));
        // The right part of a wide bar, past the tools.
        assert_eq!(hit(&l, l.bar.right() - 1.0, l.bar.y + 12.0), Some(BarHit::Bar));
        // Outside the bar (its inset, below it, the block's left edge).
        assert_eq!(hit(&l, 2.0, 20.0), None);
        assert_eq!(hit(&l, 100.0, 2.0), None);
        assert_eq!(hit(&l, 100.0, BAR_BLOCK - 2.0), None);
        assert_eq!(hit(&l, -1.0, 20.0), None);
    }

    #[test]
    fn the_more_button_is_hit_and_a_hidden_tool_is_not() {
        let l = layout(160.0);
        let more = l.more.unwrap();
        assert_eq!(hit(&l, more.x + 3.0, more.y + 3.0), Some(BarHit::More));
        assert!(rect_of(&l, Tool::Image).is_none());
        // The marker is hidden at 160: nothing there hits it.
        for x in 0..160 {
            for y in 0..42 {
                assert_ne!(hit(&l, x as f32, y as f32), Some(BarHit::MarkerMenu));
            }
        }
    }

    #[test]
    fn every_dip_of_the_bar_hits_exactly_what_it_draws() {
        // No point is on two buttons, and each button's centre answers it.
        for w in [160.0, 200.0, 240.0] {
            let l = layout(w);
            for (t, r) in &l.items {
                let h = hit(&l, r.x + 2.0, r.y + 2.0);
                assert_eq!(h, Some(BarHit::Tool(*t)), "w {w}");
            }
        }
    }

    #[test]
    fn shortcuts_map_to_their_tools() {
        assert_eq!(shortcut(Tool::Bold), "Ctrl+B");
        assert_eq!(shortcut(Tool::Strike), "Ctrl+Shift+X");
        assert_eq!(shortcut(Tool::Bullet), "Ctrl+Shift+8");
        assert_eq!(shortcut(Tool::Check), "Ctrl+Shift+9");
        assert_eq!(shortcut(Tool::Marker), "Ctrl+Shift+H");
        assert_eq!(shortcut(Tool::Image), "");
        for t in Tool::ALL {
            let sc = shortcut(t);
            if sc.is_empty() {
                continue;
            }
            let shift = sc.contains("Shift");
            let vk = u16::from(sc.as_bytes()[sc.len() - 1]);
            assert_eq!(shortcut_of(vk, true, shift, false), Some(t), "{sc}");
        }
        assert_eq!(shortcut_of(0x42, false, false, false), None, "no Ctrl");
        assert_eq!(shortcut_of(0x42, true, true, false), None, "Ctrl+Shift+B is nothing");
        assert_eq!(shortcut_of(0x58, true, false, false), None, "plain Ctrl+X cuts");
        assert_eq!(shortcut_of(0x43, true, false, false), None, "Ctrl+C copies");
        assert_eq!(shortcut_of(0x56, true, false, false), None, "Ctrl+V pastes");
        assert_eq!(shortcut_of(0x48, true, false, false), None, "plain Ctrl+H is backspace");
        assert_eq!(shortcut_of(0x41, true, false, false), None, "Ctrl+A is the EDIT's");
        assert_eq!(shortcut_of(0x39, true, false, false), None);
        // AltGr is Ctrl+Alt: it types a character, it is never a shortcut.
        for (vk, shift) in [(0x42, false), (0x49, false), (0x55, false), (0x58, true), (0x38, true), (0x39, true), (0x48, true)] {
            assert_eq!(shortcut_of(vk, true, shift, true), None, "AltGr+{vk:#x}");
        }
    }

    #[test]
    fn the_control_characters_the_shortcuts_leave_are_known() {
        assert_eq!(control_char_of(Tool::Bold), Some(0x02));
        assert_eq!(control_char_of(Tool::Italic), Some(0x09), "Tab");
        assert_eq!(control_char_of(Tool::Underline), Some(0x15));
        assert_eq!(control_char_of(Tool::Strike), Some(0x18), "would cut");
        assert_eq!(control_char_of(Tool::Marker), Some(0x08), "would backspace");
        assert_eq!(control_char_of(Tool::Bullet), None);
        assert_eq!(control_char_of(Tool::Check), None);
    }
}
